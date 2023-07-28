//! Distributed Clocks (DC).

use crate::{
    error::Error,
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    slave::{ports::Topology, Slave, SlaveRef},
    Client,
};

/// Send a broadcast to all slaves to latch in DC receive time, then store it on the slave structs.
async fn latch_dc_times(client: &Client<'_>, slaves: &mut [Slave]) -> Result<(), Error> {
    let num_slaves_with_dc: usize = slaves
        .iter()
        .filter(|slave| slave.flags.dc_supported)
        .count();

    // Latch receive times into all ports of all slaves.
    client
        .bwr(RegisterAddress::DcTimePort0, 0u32)
        .await?
        .wkc(num_slaves_with_dc as u16, "Broadcast time")?;

    // Read receive times for all slaves and store on slave structs
    for slave in slaves.iter_mut().filter(|slave| slave.flags.dc_supported) {
        let sl = SlaveRef::new(client, slave.configured_address, ());

        // NOTE: Defined as a u64, not i64, in the spec
        // TODO: Remember why this is i64 here. SOEM uses i64 I think, and I seem to remember things
        // breaking/being weird if it wasn't i64? Was it something to do with wraparound/overflow?
        let (dc_receive_time, _wkc) = sl
            .read_ignore_wkc::<i64>(RegisterAddress::DcReceiveTime)
            .await?;

        let [time_p0, time_p1, time_p2, time_p3] = sl
            .read::<[u32; 4]>(RegisterAddress::DcTimePort0, "Port receive times")
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to read DC times for slave {:#06x}: {}",
                    slave.configured_address,
                    e
                );

                e
            })?;

        slave.dc_receive_time = dc_receive_time;
        slave.ports.0[0].dc_receive_time = time_p0;
        slave.ports.0[1].dc_receive_time = time_p1;
        slave.ports.0[2].dc_receive_time = time_p2;
        slave.ports.0[3].dc_receive_time = time_p3;
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
    let sl = SlaveRef::new(client, slave.configured_address, ());

    sl.write_ignore_wkc::<i64>(
        RegisterAddress::DcSystemTimeOffset,
        -dc_receive_time + now_nanos,
    )
    .await?;

    sl.write_ignore_wkc::<u32>(
        RegisterAddress::DcSystemTimeTransmissionDelay,
        slave.propagation_delay,
    )
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
                .find(|slave| slave.ports.topology() == Topology::Fork)
                .ok_or_else(|| {
                    log::error!(
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
        log::error!(
            "Did not find parent for slave {:#06x}",
            slave.configured_address
        );

        Err(Error::Topology)
    }
}

/// Calculate and assign a slave device's propagation delay, i.e. the time it takes for a packet to
/// reach it when sent from the master.
fn configure_slave_offsets(
    slave: &mut Slave,
    parents: &[Slave],
    delay_accum: &mut u32,
) -> Result<(), Error> {
    // Just for debug
    {
        let time_p0 = slave.ports.0[0].dc_receive_time;
        let time_p1 = slave.ports.0[1].dc_receive_time;
        let time_p2 = slave.ports.0[2].dc_receive_time;
        let time_p3 = slave.ports.0[3].dc_receive_time;

        // Deltas between port receive times
        let d01 = time_p1.saturating_sub(time_p0);
        let d21 = time_p2.saturating_sub(time_p1);
        let d32 = time_p3.saturating_sub(time_p2);

        let loop_propagation_time = slave.ports.propagation_time();
        let child_delay = slave.ports.child_delay();

        log::debug!("--> Topology {:?}, {}", slave.ports.topology(), slave.ports);
        log::debug!("--> Receive times {time_p0} ns (Δ {d01} ns) {time_p1} (Δ {d21} ns) {time_p2} (Δ {d32} ns) {time_p3}");
        log::debug!(
            "--> Loop propagation time {loop_propagation_time:?} ns, child delay {child_delay:?} ns"
        );
    }

    if let Some(parent_idx) = slave.parent_index {
        let parent = parents
            .iter()
            .find(|parent| parent.index == parent_idx)
            .expect("Parent");

        // If we're a child of this parent (e.g. a module in an EK1100 chain), use the parent's
        // child delay. If we're downstream of it, use the whole propagation time.
        let parent_time = parent
            .ports
            .child_delay()
            .filter(|_| slave.is_child_of(&parent))
            .or(parent.ports.propagation_time());

        let slave_delay =
            parent_time.map(|parent| (parent - slave.ports.propagation_time().unwrap_or(0)) / 2);

        *delay_accum += slave_delay.unwrap_or(0);

        slave.propagation_delay = *delay_accum;
    }

    log::debug!("--> Propagation delay {} ns", slave.propagation_delay);

    if !slave.flags.has_64bit_dc {
        // TODO?
        log::warn!("--> Slave uses seconds instead of ns?");
    }

    Ok(())
}

/// Assign parent/child relationships between slave devices, and compute propagation delays for all
/// salves.
fn assign_parent_relationships(slaves: &mut [Slave]) -> Result<(), Error> {
    let mut delay_accum = 0;

    for i in 0..slaves.len() {
        let (parents, rest) = slaves.split_at_mut(i);
        let slave = rest.first_mut().ok_or(Error::Internal)?;

        slave.parent_index = find_slave_parent(parents, slave)?;

        log::debug!(
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
            let parent = parents
                .iter_mut()
                .find(|parent| parent.index == parent_idx)
                .expect("Parent");

            parent
                .ports
                .assign_next_downstream_port(slave.index)
                .expect("No free ports on parent. Logic error.");
        }

        if slave.flags.dc_supported {
            configure_slave_offsets(slave, parents, &mut delay_accum)?;
        } else {
            log::trace!(
                "--> Skipping DC config for slave {:#06x}: DC not supported",
                slave.configured_address
            );
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

    log::debug!("Distributed clock config complete");

    // TODO: Set a flag so we can periodically send a FRMW to keep clocks in sync. Maybe add a
    // config item to set minimum tick rate?

    Ok(first_dc_slave)
}

pub(crate) async fn run_dc_static_sync(
    client: &Client<'_>,
    dc_reference_slave: &Slave,
    iterations: u32,
) -> Result<(), Error> {
    log::debug!(
        "Performing static drift compensation using slave {:#06x} {} as reference. This can take some time...",
        dc_reference_slave.configured_address,
        dc_reference_slave.name
    );

    // Static drift compensation - distribute reference clock through network until slave clocks
    // settle
    for _ in 0..iterations {
        let (_reference_time, _wkc) = client
            .frmw::<u64>(
                dc_reference_slave.configured_address,
                RegisterAddress::DcSystemTime,
            )
            .await?;
    }

    log::debug!("Static drift compensation complete");

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
            name: "Default".into(),
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
                name: "LAN9252".into(),
                index: 0,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x1100,
                ports: ports_fork(),
                name: "EK1100".into(),
                index: 1,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2004".into(),
                index: 2,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL3004".into(),
                index: 3,
                ..slave_defaults.clone()
            },
        ];

        let me = Slave {
            configured_address: 0x9252,
            ports: ports_eol(),
            name: "LAN9252".into(),
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
            name: "Default".into(),
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
                name: "EK1100".into(),
                index: 1,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2004".into(),
                index: 2,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL3004".into(),
                index: 3,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x1100,
                ports: ports_fork(),
                name: "EK1100_2".into(),
                index: 4,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x2004,
                ports: ports_passthrough(),
                name: "EL2828".into(),
                index: 5,
                ..slave_defaults.clone()
            },
            Slave {
                configured_address: 0x3004,
                ports: ports_eol(),
                name: "EL2889".into(),
                index: 6,
                ..slave_defaults.clone()
            },
        ];

        let ek1100_2_parents = &parents[0..3];
        let ek1100_2 = &parents[3];

        let el2828_parents = &parents[0..4];
        let el2828 = &parents[4];

        assert_eq!(
            find_slave_parent(&ek1100_2_parents, &ek1100_2).unwrap(),
            Some(1)
        );
        assert_eq!(
            find_slave_parent(&el2828_parents, &el2828).unwrap(),
            Some(ek1100_2.index)
        );
    }

    #[test]
    fn first_in_chain() {
        let slave_defaults = Slave {
            configured_address: 0x1000,
            ports: Ports::default(),
            name: "Default".into(),
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
            name: "EK1100".into(),
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
            name: "Default".into(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
            ..Default::default()
        };

        let mut parents = [];

        let mut delay_accum = 0u32;

        configure_slave_offsets(&mut slave, &mut parents, &mut delay_accum).expect("bad config");

        assert_eq!(slave.dc_receive_time, 0i64);
    }
}
