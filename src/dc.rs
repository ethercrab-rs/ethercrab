//! Distributed Clocks (DC).

use crate::{
    error::Error,
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    slave::{ports::Topology, slave_client::SlaveClient, Slave},
    timer_factory::TimerFactory,
    Client,
};

/// Configure distributed clocks.
pub async fn configure_dc<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>(
    client: &Client<'_, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    slaves: &mut [Slave],
) -> Result<(), Error>
where
    TIMEOUT: TimerFactory,
{
    let num_slaves = slaves.len();

    // Latch receive times into all ports of all slaves.
    client
        .bwr(RegisterAddress::DcTimePort0, 0u32)
        .await
        .expect("Broadcast time")
        .wkc(num_slaves as u16, "Broadcast time")
        .unwrap();

    // let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

    // TODO: Allow passing in of an initial value
    // let now_nanos =
    //     chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());
    let now_nanos = 0;

    let mut delay_accum = 0;

    for i in 0..slaves.len() {
        let (parents, rest) = slaves.split_at_mut(i);
        let mut slave = rest.first_mut().ok_or(Error::Internal)?;

        {
            // Walk back up EtherCAT chain and find parent of this slave
            let mut parents_it = parents.iter().rev();

            // TODO: Check topology with two EK1100s, one daisychained off the other. I have a
            // feeling that won't work properly.
            while let Some(parent) = parents_it.next() {
                // Previous parent in the chain is a leaf node in the tree, so we need to
                // continue iterating to find the common parent, i.e. the split point
                if parent.ports.topology() == Topology::LineEnd {
                    let split_point = parents_it
                        .find(|slave| slave.ports.topology() == Topology::Fork)
                        .ok_or_else(|| {
                            log::error!(
                                "Did not find parent for slave {}",
                                slave.configured_address
                            );

                            Error::Topology
                        })?;

                    slave.parent_index = Some(split_point.index);
                } else {
                    slave.parent_index = Some(parent.index);

                    break;
                }
            }
        }

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

        let sl = SlaveClient::new(client, slave.configured_address);

        // NOTE: Defined as a u64 in the spec
        let (dc_receive_time, _wkc) = sl
            .read_ignore_wkc::<i64>(RegisterAddress::DcReceiveTime)
            .await?;

        let [time_p0, time_p1, time_p2, time_p3] = sl
            .read::<[u32; 4]>(RegisterAddress::DcTimePort0, "Port receive times")
            .await?;

        slave.ports.0[0].dc_receive_time = time_p0;
        slave.ports.0[1].dc_receive_time = time_p1;
        slave.ports.0[2].dc_receive_time = time_p2;
        slave.ports.0[3].dc_receive_time = time_p3;

        let d01 = time_p1 - time_p0;
        let d12 = time_p2 - time_p1;
        let d32 = time_p3 - time_p2;

        let loop_propagation_time = slave.ports.propagation_time();
        let child_delay = slave.ports.child_delay().unwrap_or(0);

        log::debug!("--> Times {time_p0} ({d01}) {time_p1} ({d12}) {time_p2} ({d32}) {time_p3}");
        log::debug!(
            "--> Propagation time {loop_propagation_time:?} ns, child delay {child_delay} ns"
        );

        if let Some(parent_idx) = slave.parent_index {
            let parent = parents
                .iter_mut()
                .find(|parent| parent.index == parent_idx)
                .unwrap();

            let assigned_port_idx = parent
                .ports
                .assign_next_downstream_port(slave.index)
                .expect("No free ports. Logic error.");

            let parent_port = parent.ports.0[assigned_port_idx];
            let prev_parent_port = parent.ports.prev_open_port(&parent_port).unwrap();
            let parent_time = parent_port.dc_receive_time - prev_parent_port.dc_receive_time;

            let entry_port = slave.ports.entry_port().unwrap();
            let prev_port = slave.ports.prev_open_port(&entry_port).unwrap();
            let my_time = prev_port.dc_receive_time - entry_port.dc_receive_time;

            // The delay between the previous slave and this one
            let delay = (parent_time - my_time) / 2;

            delay_accum += delay;

            // If parent has children but we're not one of them, add the children's delay to
            // this slave's offset.
            if let Some(child_delay) = parent
                .ports
                .child_delay()
                .filter(|_| parent.ports.is_last_port(&parent_port))
            {
                log::debug!("--> Child delay of parent {}", child_delay);

                delay_accum += child_delay / 2;
            }

            slave.propagation_delay = delay_accum;

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

        // Write system clock offset
        // NOTE: Spec says this register is a u64
        sl.write_ignore_wkc::<i64>(
            RegisterAddress::DcSystemTimeOffset,
            -dc_receive_time + now_nanos,
        )
        .await?;

        // Write propagation delay
        sl.write_ignore_wkc::<u32>(
            RegisterAddress::DcSystemTimeTransmissionDelay,
            slave.propagation_delay,
        )
        .await?;
    }

    // TODO: Set a flag so we can periodically send a FRMW to keep clocks in sync. Maybe add a
    // config item to set minimum tick rate?

    // TODO: See if it's possible to programmatically read the maximum cycle time from slave info

    Ok(())
}
