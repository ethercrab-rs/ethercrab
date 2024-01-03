//! Distributed Clocks (DC).

use crate::{
    command::Command,
    error::Error,
    fmt,
    register::RegisterAddress,
    slave::{ports::Topology, Slave},
    Client, SlaveRef,
};

/// Send a broadcast to all slaves to latch in DC receive time, then store it on the slave structs.
async fn latch_dc_times(client: &Client<'_>, slaves: &mut [Slave]) -> Result<(), Error> {
    let num_slaves_with_dc: usize = slaves
        .iter()
        .filter(|slave| slave.flags.dc_supported)
        .count();

    // Latch receive times into all ports of all slaves.
    Command::bwr(RegisterAddress::DcTimePort0.into())
        .wrap(client)
        .with_wkc(num_slaves_with_dc as u16)
        .send_receive(0u32)
        .await?;

    // Read receive times for all slaves and store on slave structs
    for slave in slaves.iter_mut().filter(|slave| slave.flags.dc_supported) {
        let sl = SlaveRef::new(client, slave.configured_address, ());

        // NOTE: Defined as a u64, not i64, in the spec
        // TODO: Remember why this is i64 here. SOEM uses i64 I think, and I seem to remember things
        // breaking/being weird if it wasn't i64? Was it something to do with wraparound/overflow?
        let dc_receive_time = sl
            .read(RegisterAddress::DcReceiveTime)
            .ignore_wkc()
            .receive::<i64>()
            .await?;

        let [time_p0, time_p1, time_p2, time_p3] = sl
            .read(RegisterAddress::DcTimePort0)
            // 4 * u32
            .receive::<[u8; 4 * 4]>()
            .await
            .map(|slice| {
                let chunks = slice.chunks_exact(4);

                let mut res = [0u32; 4];

                for (i, chunk) in chunks.enumerate() {
                    res[i] = u32::from_le_bytes(fmt::unwrap!(chunk.try_into()));
                }

                res
            })
            .map_err(|e| {
                fmt::error!(
                    "Failed to read DC times for slave {:#06x}: {}",
                    slave.configured_address,
                    e
                );

                e
            })?;

        slave.dc_receive_time = dc_receive_time;

        slave
            .ports
            .set_receive_times(time_p0, time_p3, time_p1, time_p2);
    }

    Ok(())
}

/// Write DC system time offset and propagation delay to the slave memory.
async fn write_dc_parameters(
    client: &Client<'_>,
    slave: &Slave,
    dc_receive_time: i64,
    now_nanos: i64,
) -> Result<(), Error> {
    Command::fpwr(
        slave.configured_address,
        RegisterAddress::DcSystemTimeOffset.into(),
    )
    .wrap(client)
    .send(-dc_receive_time + now_nanos)
    .await?;

    Command::fpwr(
        slave.configured_address,
        RegisterAddress::DcSystemTimeTransmissionDelay.into(),
    )
    .wrap(client)
    .send(slave.propagation_delay)
    .await?;

    Ok(())
}

/// Find the slave parent device in the list of slaves before it in the linear topology.
///
/// # Implementation detail
///
/// ## Special case: parent is a branch
///
/// Given the following topology, we want to find the parent device of the LAN9252 (index 5).
/// According to EtherCAT topology rules, the parent device is the EK1100 (index 1), however the
/// slave devices are discovered in network order (denoted here by `#N`), meaning there is some
/// special behaviour to consider when traversing backwards through the previous nodes before the
/// LAN9252.
///
/// ```text
/// ┌─────────────┐
/// │   Master    │
/// │     #0      │
/// │             │
/// └─────────────┘
///        ▲
///        ║
///        ╚0▶┌────────────┐      ┌────────────┐      ┌────────────┐──┐
///           │   EK1100   │      │   EL2004   │      │   EL3004   │  │
///           │     #1     │◀3══0▶│     #3     │◀3══0▶│     #4     │  │
///           │            │      │            │      │            │  │
///        ╔1▶└────────────┘      └────────────┘      └────────────┘◀─┘
///        ║
///        ║  ┌────────────┐──┐
///        ║  │  LAN9252   │  │
///        ╚0▶│     #5     │  │
///           │            │  │
///           └────────────┘◀─┘
/// ```
fn find_slave_parent(parents: &[Slave], slave: &Slave) -> Result<Option<usize>, Error> {
    // No parent if we're first in the network, e.g. the EK1100 in the diagram above
    if parents.is_empty() {
        return Ok(None);
    }

    let mut parents_it = parents.iter().rev();

    if let Some(parent) = parents_it.next() {
        // If the previous parent in the chain is a leaf node in the tree, we need to continue
        // iterating to find the common parent, i.e. the split point.
        //
        // Using the doc example above, say we're at #5, the previous device is #4 which is a
        // `LineEnd`. This means we traverse backwards until we find a `Fork` (the EK1100) and use
        // that as the parent.
        if parent.ports.topology() == Topology::LineEnd {
            let split_point = parents_it
                .find(|slave| slave.ports.topology().is_junction())
                .ok_or_else(|| {
                    fmt::error!(
                        "Did not find fork parent for slave {:#06x}",
                        slave.configured_address
                    );

                    Error::Topology
                })?;

            Ok(Some(split_point.index))
        }
        // Otherwise the parent is just the previous node in the tree
        else {
            Ok(Some(parent.index))
        }
    } else {
        fmt::error!(
            "Did not find parent for slave {:#06x}",
            slave.configured_address
        );

        Err(Error::Topology)
    }
}

/// Calculate and assign a slave device's propagation delay, i.e. the time it takes for a packet to
/// reach it when sent from the master.
fn configure_slave_offsets(slave: &mut Slave, parents: &[Slave], delay_accum: &mut u32) {
    // Just for debug
    {
        let time_p0 = slave.ports.0[0].dc_receive_time;
        let time_p3 = slave.ports.0[1].dc_receive_time;
        let time_p1 = slave.ports.0[2].dc_receive_time;
        let time_p2 = slave.ports.0[3].dc_receive_time;

        // Deltas between port receive times
        let d03 = time_p3.saturating_sub(time_p0);
        let d31 = time_p1.saturating_sub(time_p3);
        let d12 = time_p2.saturating_sub(time_p1);

        // let loop_propagation_time = slave.ports.total_propagation_time();
        // let child_delay = slave.ports.fork_child_delay();

        fmt::debug!("--> Topology {:?}, {}", slave.ports.topology(), slave.ports);
        fmt::debug!(
            "--> Receive times {} ns ({} ns) {} ({} ns) {} ({} ns) {} (total {})",
            time_p0,
            if slave.ports.0[1].active { d03 } else { 0 },
            // d03,
            time_p3,
            if slave.ports.0[2].active { d31 } else { 0 },
            // d31,
            time_p1,
            if slave.ports.0[3].active { d12 } else { 0 },
            // d12,
            time_p2,
            slave.ports.total_propagation_time().unwrap_or(0)
        );
    }

    let parent = slave
        .parent_index
        .and_then(|parent_index| parents.iter().find(|parent| parent.index == parent_index));

    if let Some(parent) = parent {
        let parent_port =
            fmt::unwrap_opt!(parent.ports.port_assigned_to(slave), "Parent assigned port");

        // The port the master is connected to on this slave. Must always be entry port.
        let this_port = slave.ports.entry_port();

        fmt::debug!(
            "--> Parent ({:?}) {} port {} assigned to {} port {} (slave is child of parent: {:?})",
            parent.ports.topology(),
            parent.name(),
            parent_port.number,
            slave.name(),
            this_port.number,
            slave.is_child_of(parent)
        );

        let parent_prop_time = parent.ports.total_propagation_time().unwrap_or(0);
        let this_prop_time = slave.ports.total_propagation_time().unwrap_or(0);

        fmt::debug!(
            "--> Parent propagation time {}, my prop. time {} delta {}",
            parent_prop_time,
            this_prop_time,
            parent_prop_time - this_prop_time
        );

        let propagation_delay = match parent.ports.topology() {
            Topology::Passthrough => (parent_prop_time - this_prop_time) / 2,
            Topology::Fork => {
                if slave.is_child_of(parent) {
                    let children_loop_time =
                        parent.ports.propagation_time_to(parent_port).unwrap_or(0);

                    (children_loop_time - this_prop_time) / 2
                } else {
                    (parent_prop_time - this_prop_time) / 2
                }
            }
            Topology::Cross => {
                if slave.is_child_of(parent) {
                    let children_loop_time =
                        parent.ports.intermediate_propagation_time_to(parent_port);

                    (children_loop_time - this_prop_time) / 2
                } else {
                    parent_prop_time - *delay_accum
                }
            }
            // A parent of any device cannot have a `LineEnd` topology as it will always have at
            // least 2 ports open (1 for comms to master, 1 to current slave device)
            Topology::LineEnd => 0,
        };

        *delay_accum += propagation_delay;

        fmt::debug!(
            "--> Propagation delay {} (delta {}) ns",
            delay_accum,
            propagation_delay,
        );

        slave.propagation_delay = *delay_accum;
    }
}

/// Assign parent/child relationships between slave devices, and compute propagation delays for all
/// salves.
fn assign_parent_relationships(slaves: &mut [Slave]) -> Result<(), Error> {
    let mut delay_accum = 0;

    for i in 0..slaves.len() {
        let (parents, rest) = slaves.split_at_mut(i);
        let slave = rest.first_mut().ok_or(Error::Internal)?;

        slave.parent_index = find_slave_parent(parents, slave)?;

        fmt::debug!(
            "Slave {:#06x} {} {}",
            slave.configured_address,
            slave.name,
            if slave.flags.dc_supported {
                "has DC"
            } else {
                "no DC"
            }
        );

        // If this slave has a parent, find it, then assign the parent's next open port to this
        // slave, estabilishing the relationship between them by index.
        if let Some(parent_idx) = slave.parent_index {
            let parent =
                fmt::unwrap_opt!(parents.iter_mut().find(|parent| parent.index == parent_idx));

            fmt::unwrap_opt!(
                parent.ports.assign_next_downstream_port(slave.index),
                "no free ports on parent"
            );
        }

        if slave.flags.dc_supported {
            configure_slave_offsets(slave, parents, &mut delay_accum);
        } else {
            fmt::trace!(
                "--> Skipping DC config for slave {:#06x}: DC not supported",
                slave.configured_address
            );
        }
    }

    // Extremely crude output to generate test cases with. `rustfmt` makes it look nice in the test.
    #[cfg(feature = "std")]
    if option_env!("PRINT_TEST_CASE").is_some() {
        for slave in slaves.iter() {
            let p = slave.ports.0;

            let ports = format!(
                "{}, {}, {}, {}, {}, {}, {}, {}",
                p[0].active,
                p[0].dc_receive_time,
                p[1].active,
                p[1].dc_receive_time,
                p[2].active,
                p[2].dc_receive_time,
                p[3].active,
                p[3].dc_receive_time,
            );

            println!(
                r#"Slave {{
                index: {},
                configured_address: {:#06x},
                name: "{}".try_into().unwrap(),
                ports: ports(
                    {}
                ),
                dc_receive_time: {},
                ..defaults.clone()
            }},"#,
                slave.index, slave.configured_address, slave.name, ports, slave.dc_receive_time,
            );
        }

        for slave in slaves.iter() {
            println!(
                "// Index {}: {} ({:?})",
                slave.index,
                slave.name(),
                slave.ports.topology()
            );

            print!("(");

            print!("[");
            for p in slave.ports.0 {
                print!("{:?}, ", p.downstream_to);
            }
            print!("],");

            print!("{:?}, {}, ", slave.parent_index, slave.propagation_delay);

            print!("),");

            println!();
        }
    }

    Ok(())
}

/// Configure distributed clocks.
///
/// This method walks through the discovered list of devices and sets the system time offset and
/// transmission delay of each device.
pub(crate) async fn configure_dc<'slaves>(
    client: &Client<'_>,
    slaves: &'slaves mut [Slave],
) -> Result<Option<&'slaves Slave>, Error> {
    latch_dc_times(client, slaves).await?;

    // let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

    // TODO: Allow passing in of an initial value
    // let now_nanos =
    //     chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());
    let now_nanos = 0;

    assign_parent_relationships(slaves)?;

    for slave in slaves.iter() {
        write_dc_parameters(client, slave, slave.dc_receive_time, now_nanos).await?;
    }

    let first_dc_slave = slaves.iter().find(|slave| slave.flags.dc_supported);

    fmt::debug!("Distributed clock config complete");

    // TODO: Set a flag so we can periodically send a FRMW to keep clocks in sync. Maybe add a
    // config item to set minimum tick rate?

    Ok(first_dc_slave)
}

pub(crate) async fn run_dc_static_sync(
    client: &Client<'_>,
    dc_reference_slave: &Slave,
    iterations: u32,
) -> Result<(), Error> {
    fmt::debug!(
        "Performing static drift compensation using slave {:#06x} {} as reference. This can take some time...",
        dc_reference_slave.configured_address,
        dc_reference_slave.name
    );

    // Static drift compensation - distribute reference clock through network until slave clocks
    // settle
    for _ in 0..iterations {
        Command::frmw(
            dc_reference_slave.configured_address,
            RegisterAddress::DcSystemTime.into(),
        )
        .wrap(client)
        .ignore_wkc()
        .receive::<u64>()
        .await?;
    }

    fmt::debug!("Static drift compensation complete");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        register::SupportFlags,
        slave::ports::{tests::make_ports, Port, Ports},
    };

    // A slave device in the middle of the chain
    fn ports_passthrough() -> Ports {
        make_ports(true, true, false, false)
    }

    // EK1100 for example, with in/out ports and a bunch of slaves connected to it
    fn ports_fork() -> Ports {
        make_ports(true, true, true, false)
    }

    // Last slave in the network
    fn ports_eol() -> Ports {
        make_ports(true, false, false, false)
    }

    // Test for topology including an EK1100 that creates a fork in the tree.
    #[test]
    fn parent_is_ek1100() {
        let slave_defaults = Slave {
            configured_address: 0x0000,
            ports: Ports::default(),
            name: "Default".try_into().unwrap(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
            ..Default::default()
        };

        let parents = [
            Slave {
                configured_address: 0x1000,
                ports: ports_passthrough(),
                name: "LAN9252".try_into().unwrap(),
                index: 0,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x1100,
                ports: ports_fork(),
                name: "EK1100".try_into().unwrap(),
                index: 1,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2004".try_into().unwrap(),
                index: 2,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL3004".try_into().unwrap(),
                index: 3,
                ..slave_defaults.clone()
            },
        ];

        let me = Slave {
            configured_address: 0x9252,
            ports: ports_eol(),
            name: "LAN9252".try_into().unwrap(),
            index: 4,
            ..slave_defaults
        };

        let parent_index = find_slave_parent(&parents, &me);

        assert_eq!(parent_index.unwrap(), Some(1));
    }

    // Two forks in the tree
    #[test]
    fn two_ek1100() {
        let slave_defaults = Slave {
            configured_address: 0x0000,
            ports: Ports::default(),
            name: "Default".try_into().unwrap(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
            ..Default::default()
        };

        let parents = [
            Slave {
                configured_address: 0x1100,
                ports: ports_fork(),
                name: "EK1100".try_into().unwrap(),
                index: 1,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2004".try_into().unwrap(),
                index: 2,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL3004".try_into().unwrap(),
                index: 3,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x1100,
                ports: ports_fork(),
                name: "EK1100_2".try_into().unwrap(),
                index: 4,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2828".try_into().unwrap(),
                index: 5,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL2889".try_into().unwrap(),
                index: 6,
                ..slave_defaults.clone()
            },
        ];

        let ek1100_2_parents = &parents[0..3];
        let ek1100_2 = &parents[3];

        let el2828_parents = &parents[0..4];
        let el2828 = &parents[4];

        assert_eq!(
            find_slave_parent(ek1100_2_parents, ek1100_2).unwrap(),
            Some(1)
        );
        assert_eq!(
            find_slave_parent(el2828_parents, el2828).unwrap(),
            Some(ek1100_2.index)
        );
    }

    #[test]
    fn first_in_chain() {
        let slave_defaults = Slave {
            configured_address: 0x1000,
            ports: Ports::default(),
            name: "Default".try_into().unwrap(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
            ..Default::default()
        };

        let parents = [];

        let me = Slave {
            configured_address: 0x1100,
            ports: ports_eol(),
            name: "EK1100".try_into().unwrap(),
            index: 4,
            ..slave_defaults
        };

        let parent_index = find_slave_parent(&parents, &me);

        assert_eq!(parent_index.unwrap(), None);
    }

    #[test]
    fn p0_p1_times_only() {
        // From EC400 in test rig
        let ports = Ports([
            Port {
                active: true,
                dc_receive_time: 641524306,
                number: 0,
                downstream_to: None,
            },
            Port {
                active: false,
                dc_receive_time: 1413563250,
                number: 1,
                downstream_to: None,
            },
            Port {
                active: false,
                dc_receive_time: 0,
                number: 2,
                downstream_to: None,
            },
            Port {
                active: false,
                dc_receive_time: 0,
                number: 3,
                downstream_to: None,
            },
        ]);

        assert_eq!(ports.topology(), Topology::LineEnd);

        let mut slave = Slave {
            configured_address: 0x1000,
            ports,
            name: "Default".try_into().unwrap(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
            ..Default::default()
        };

        let mut parents = [];

        let mut delay_accum = 0u32;

        configure_slave_offsets(&mut slave, &mut parents, &mut delay_accum);

        assert_eq!(slave.dc_receive_time, 0i64);
    }

    /// Create a ports object with active flags and DC receive times.
    fn ports(
        active0: bool,
        t0: u32,
        active3: bool,
        t3: u32,
        active1: bool,
        t1: u32,
        active2: bool,
        t2: u32,
    ) -> Ports {
        let mut ports = Ports::new(active0, active3, active1, active2);

        ports.set_receive_times(t0, t3, t1, t2);

        ports
    }

    // Test that slave parent/child relationships are established, and that propagation delays are
    // computed correctly.
    #[test]
    fn propagation_delay_calc_fork() {
        let _ = env_logger::builder().is_test(true).try_init();

        let defaults = Slave {
            configured_address: 0x999,
            name: "CHANGEME".try_into().unwrap(),
            ports: Ports::default(),
            dc_receive_time: 0,
            index: 0,
            flags: SupportFlags {
                dc_supported: true,
                ..SupportFlags::default()
            },
            ..Slave::default()
        };

        // Input data represents the following topology
        //
        // EK1100
        // --> EK1122
        // --> EL9560
        // EK1914
        // --> EL1008
        let mut slaves = [
            Slave {
                index: 0,
                configured_address: 0x1000,
                name: "EK1100".try_into().unwrap(),
                ports: ports(
                    true, 3380373882, false, 1819436374, true, 3380374482, true, 3380375762,
                ),
                dc_receive_time: 402812332410,
                ..defaults.clone()
            },
            Slave {
                index: 1,
                configured_address: 0x1001,
                name: "EK1122".try_into().unwrap(),
                ports: ports(
                    true, 3384116362, false, 1819436374, false, 1717989224, true, 3384116672,
                ),
                dc_receive_time: 402816074890,
                ..defaults.clone()
            },
            Slave {
                index: 2,
                configured_address: 0x1002,
                name: "EL9560".try_into().unwrap(),
                ports: ports(
                    true, 3383862982, false, 1819436374, false, 1717989224, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
            Slave {
                index: 3,
                configured_address: 0x1003,
                name: "EK1914".try_into().unwrap(),
                ports: ports(
                    true, 3373883962, false, 1819436374, true, 3373884272, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
            Slave {
                index: 4,
                configured_address: 0x1004,
                name: "EL1008".try_into().unwrap(),
                ports: ports(
                    true, 3375060602, false, 1819436374, false, 1717989224, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
        ];

        let downstreams = [
            // Index 0: EK1100 (Fork)
            ([None, None, Some(1), Some(3)], None, 0),
            // Index 1: EK1122 (Passthrough)
            ([None, None, None, Some(2)], Some(0), 145),
            // Index 2: EL9560 (LineEnd)
            ([None, None, None, None], Some(1), 300),
            // Index 3: EK1914 (Passthrough)
            ([None, None, Some(4), None], Some(0), 1085),
            // Index 4: EL1008 (LineEnd)
            ([None, None, None, None], Some(3), 1240),
        ];

        let expected = {
            let mut expected = slaves.clone();

            expected.iter_mut().zip(downstreams).for_each(
                |(slave, ([d0, d3, d1, d2], parent_index, propagation_delay))| {
                    slave.ports.set_downstreams(d0, d3, d1, d2);

                    slave.parent_index = parent_index;
                    slave.propagation_delay = propagation_delay;
                },
            );

            expected
        };

        assign_parent_relationships(&mut slaves).expect("assign");

        pretty_assertions::assert_eq!(slaves, expected);
    }

    #[test]
    fn propagation_delay_calc_cross() {
        let _ = env_logger::builder().is_test(true).try_init();

        let defaults = Slave {
            configured_address: 0x999,
            name: "CHANGEME".try_into().unwrap(),
            ports: Ports::default(),
            dc_receive_time: 0,
            index: 0,
            flags: SupportFlags {
                dc_supported: true,
                ..SupportFlags::default()
            },
            ..Slave::default()
        };

        let mut slaves = [
            Slave {
                index: 0,
                configured_address: 0x1000,
                name: "EK1100".try_into().unwrap(),
                ports: ports(
                    true, 3493061450, false, 1819436374, true, 3493064460, false, 0,
                ),
                dc_receive_time: 3493061450,
                ..defaults.clone()
            },
            Slave {
                index: 1,
                configured_address: 0x1001,
                name: "EK1122".try_into().unwrap(),
                ports: ports(
                    true, 3493293220, true, 3493294570, true, 3493295650, true, 3493295940,
                ),
                dc_receive_time: 3493293220,
                ..defaults.clone()
            },
            Slave {
                index: 2,
                configured_address: 0x1002,
                name: "EK1914".try_into().unwrap(),
                ports: ports(
                    true, 3485337450, false, 1819436374, true, 3485337760, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
            Slave {
                index: 3,
                configured_address: 0x1003,
                name: "EL1008".try_into().unwrap(),
                ports: ports(
                    true, 3488375400, false, 1819436374, false, 1717989224, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
            Slave {
                index: 4,
                configured_address: 0x1004,
                name: "EK1101".try_into().unwrap(),
                ports: ports(
                    true, 3485087810, false, 1819436374, false, 1717989224, false, 0,
                ),
                dc_receive_time: 3485087810,
                ..defaults.clone()
            },
            Slave {
                index: 5,
                configured_address: 0x1005,
                name: "EL9560".try_into().unwrap(),
                ports: ports(
                    true, 3494335890, false, 1819436374, false, 1717989224, false, 0,
                ),
                dc_receive_time: 0,
                ..defaults.clone()
            },
        ];

        let downstreams = [
            // Index 0: EK1100 (Passthrough)
            ([None, None, Some(1), None], None, 0),
            // Index 1: EK1122 (Cross)
            ([None, Some(2), Some(4), Some(5)], Some(0), 145),
            // Index 2: EK1914 (Passthrough)
            ([None, None, Some(3), None], Some(1), 665),
            // Index 3: EL1008 (LineEnd)
            ([None, None, None, None], Some(2), 820),
            // Index 4: EK1101 (LineEnd)
            ([None, None, None, None], Some(1), 2035),
            // Index 5: EL9560 (LineEnd)
            ([None, None, None, None], Some(1), 2720),
        ];

        let expected = {
            let mut expected = slaves.clone();

            expected.iter_mut().zip(downstreams).for_each(
                |(slave, ([d0, d3, d1, d2], parent_index, propagation_delay))| {
                    slave.ports.set_downstreams(d0, d3, d1, d2);

                    slave.parent_index = parent_index;
                    slave.propagation_delay = propagation_delay;
                },
            );

            expected
        };

        assign_parent_relationships(&mut slaves).expect("assign");

        pretty_assertions::assert_eq!(slaves, expected);
    }
}
