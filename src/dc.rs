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
    let num_slaves = slaves.len();

    // Latch receive times into all ports of all slaves.
    client
        .bwr(RegisterAddress::DcTimePort0, 0u32)
        .await
        .expect("Broadcast time")
        .wkc(num_slaves as u16, "Broadcast time")?;

    // Read receive times for all slaves and store on slave structs
    for slave in slaves {
        let sl = SlaveRef::new(client, slave.configured_address, ());

        // NOTE: Defined as a u64, not i64, in the spec
        // TODO: Remember why this is i64 here. SOEM uses i64 I think, and I seem to remember things
        // breaking/being weird if it wasn't i64? Was it something to do with wraparound/overflow?
        let (dc_receive_time, _wkc) = sl
            .read_ignore_wkc::<i64>(RegisterAddress::DcReceiveTime)
            .await?;

        let [time_p0, time_p1, time_p2, time_p3] = sl
            .read::<[u32; 4]>(RegisterAddress::DcTimePort0, "Port receive times")
            .await?;

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
    slave: &mut Slave,
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

    while let Some(parent) = parents_it.next() {
        // If the previous parent in the chain is a leaf node in the tree, we need to
        // continue iterating to find the common parent, i.e. the split point
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

            return Ok(Some(split_point.index));
        } else {
            return Ok(Some(parent.index));
        }
    }

    log::error!(
        "Did not find parent for slave {:#06x}",
        slave.configured_address
    );

    Err(Error::Topology)
}

/// Calculate and assign a slave device's propagation delay, i.e. the time it takes for a packet to
/// reach it when sent from the master.
fn configure_slave_offsets(
    slave: &mut Slave,
    parents: &mut [Slave],
    delay_accum: &mut u32,
) -> Result<i64, Error> {
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

    // Convenient variable names
    let dc_receive_time = slave.dc_receive_time;
    let time_p0 = slave.ports.0[0].dc_receive_time;
    let time_p1 = slave.ports.0[1].dc_receive_time;
    let time_p2 = slave.ports.0[2].dc_receive_time;
    let time_p3 = slave.ports.0[3].dc_receive_time;

    // Time deltas between port receive times
    let d01 = time_p1.saturating_sub(time_p0);
    let d12 = time_p2.saturating_sub(time_p1);
    let d32 = time_p3.saturating_sub(time_p2);

    let loop_propagation_time = slave.ports.propagation_time();
    let child_delay = slave.ports.child_delay().unwrap_or(0);

    log::debug!("--> Times {time_p0} ({d01}) {time_p1} ({d12}) {time_p2} ({d32}) {time_p3}");
    log::debug!("--> Propagation time {loop_propagation_time:?} ns, child delay {child_delay} ns");

    if let Some(parent_idx) = slave.parent_index {
        let parent = parents
            .iter_mut()
            .find(|parent| parent.index == parent_idx)
            .expect("Parent");

        let assigned_port_idx = parent
            .ports
            .assign_next_downstream_port(slave.index)
            .expect("No free ports. Logic error.");

        let parent_port = parent.ports.0[assigned_port_idx];
        let prev_parent_port = parent
            .ports
            .prev_open_port(&parent_port)
            .expect("Parent prev open port");
        let parent_time = parent_port.dc_receive_time - prev_parent_port.dc_receive_time;

        let entry_port = slave.ports.entry_port().expect("Entry port");
        let prev_port = slave
            .ports
            .prev_open_port(&entry_port)
            .expect("Prev open port");
        let my_time = prev_port.dc_receive_time - entry_port.dc_receive_time;

        // The delay between the previous slave and this one
        let delay = (my_time.saturating_sub(parent_time)) / 2;

        *delay_accum += delay;

        // If parent has children but we're not one of them, add the children's delay to
        // this slave's offset.
        if let Some(child_delay) = parent
            .ports
            .child_delay()
            .filter(|_| parent.ports.is_last_port(&parent_port))
        {
            log::debug!("--> Child delay of parent {}", child_delay);

            *delay_accum += child_delay / 2;
        }

        slave.propagation_delay = *delay_accum;

        log::debug!(
            "--> Parent time {} ns, my time {} ns, delay {} ns (Δ {} ns)",
            parent_time,
            my_time,
            delay_accum,
            delay
        );
    }

    if !slave.flags.has_64bit_dc {
        // TODO?
        log::warn!("--> Slave uses seconds instead of ns?");
    }

    Ok(dc_receive_time)
}

/// Configure distributed clocks.
///
/// This method walks through the discovered list of devices and sets the system time offset and
/// transmission delay of each device.
pub async fn configure_dc<'slaves>(
    client: &Client<'_>,
    slaves: &'slaves mut [Slave],
) -> Result<Option<&'slaves Slave>, Error> {
    latch_dc_times(client, slaves).await?;

    // let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

    // TODO: Allow passing in of an initial value
    // let now_nanos =
    //     chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());
    let now_nanos = 0;

    // The cumulative delay through all slave devices in the network
    let mut delay_accum = 0;

    for i in 0..slaves.len() {
        let (parents, rest) = slaves.split_at_mut(i);
        let slave = rest.first_mut().ok_or(Error::Internal)?;

        let dc_receive_time = configure_slave_offsets(slave, parents, &mut delay_accum)?;

        write_dc_parameters(client, slave, dc_receive_time, now_nanos).await?;
    }

    let first_dc_slave = slaves.iter().find(|slave| slave.flags.dc_supported);

    log::debug!("Distributed clock config complete");

    // TODO: Set a flag so we can periodically send a FRMW to keep clocks in sync. Maybe add a
    // config item to set minimum tick rate?

    Ok(first_dc_slave)
}

pub async fn run_dc_static_sync(
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
        slave::{
            ports::{tests::make_ports, Port, Ports},
            SlaveConfig, SlaveIdentity,
        },
    };

    fn ports_passthrough() -> Ports {
        make_ports(true, true, false, false)
    }

    fn ports_fork() -> Ports {
        make_ports(true, true, true, false)
    }

    fn ports_eol() -> Ports {
        make_ports(true, false, false, false)
    }

    // Test for topology including an EK1100 that creates a fork in the tree.
    #[test]
    fn parent_is_ek1100() {
        let slave_defaults = Slave {
            configured_address: 0x0000,
            ports: Ports::default(),
            config: SlaveConfig::default(),
            identity: SlaveIdentity::default(),
            name: "Default".into(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
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
            ..slave_defaults.clone()
        };

        let parent_index = find_slave_parent(&parents, &me);

        assert_eq!(parent_index.unwrap(), Some(1));
    }

    #[test]
    fn first_in_chain() {
        let slave_defaults = Slave {
            configured_address: 0x1000,
            ports: Ports::default(),
            config: SlaveConfig::default(),
            identity: SlaveIdentity::default(),
            name: "Default".into(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
        };

        let parents = [];

        let me = Slave {
            configured_address: 0x1100,
            ports: ports_eol(),
            name: "EK1100".into(),
            index: 4,
            ..slave_defaults.clone()
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
            config: SlaveConfig::default(),
            identity: SlaveIdentity::default(),
            name: "Default".into(),
            flags: SupportFlags::default(),
            dc_receive_time: 0,
            index: 0,
            parent_index: None,
            propagation_delay: 0,
        };

        let mut parents = [];

        let mut delay_accum = 0u32;

        let result = configure_slave_offsets(&mut slave, &mut parents, &mut delay_accum);

        assert_eq!(result.unwrap(), 0i64);
    }
}
