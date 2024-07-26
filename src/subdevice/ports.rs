use crate::{fmt, SubDevice};
use core::fmt::Debug;

/// Flags showing which ports are active or not on the SubDevice.
#[derive(Default, Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Port {
    pub active: bool,
    pub dc_receive_time: u32,
    /// The EtherCAT port number, ordered as 0 -> 3 -> 1 -> 2.
    pub number: u8,
    /// Holds the index of the downstream SubDevice this port is connected to.
    pub downstream_to: Option<u16>,
}

impl Port {
    // TODO: Un-pub
    pub(crate) fn index(&self) -> usize {
        match self.number {
            0 => 0,
            3 => 1,
            1 => 2,
            2 => 3,
            n => unreachable!("Invalid port number {}", n),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Topology {
    /// The SubDevice has two open ports, with only upstream and downstream subdevices.
    Passthrough,
    /// The SubDevice is the last device in its fork of the topology tree, with only one open port.
    LineEnd,
    /// The SubDevice forms a fork in the topology, with 3 open ports.
    Fork,
    /// The SubDevice forms a cross in the topology, with 4 open ports.
    Cross,
}

impl Topology {
    pub fn is_junction(&self) -> bool {
        matches!(self, Self::Fork | Self::Cross)
    }
}

#[derive(Default, Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Ports(pub [Port; 4]);

impl core::fmt::Display for Ports {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ports [ ")?;

        for p in self.0 {
            if p.active {
                f.write_str("open ")?;
            } else {
                f.write_str("closed ")?;
            }
        }

        f.write_str("]")?;

        Ok(())
    }
}

impl Ports {
    pub(crate) fn new(active0: bool, active3: bool, active1: bool, active2: bool) -> Self {
        Self([
            Port {
                active: active0,
                number: 0,
                ..Port::default()
            },
            Port {
                active: active3,
                number: 3,
                ..Port::default()
            },
            Port {
                active: active1,
                number: 1,
                ..Port::default()
            },
            Port {
                active: active2,
                number: 2,
                ..Port::default()
            },
        ])
    }

    /// Set port DC receive times, given in EtherCAT port order 0 -> 3 -> 1 -> 2
    pub(crate) fn set_receive_times(
        &mut self,
        time_p0: u32,
        time_p3: u32,
        time_p1: u32,
        time_p2: u32,
    ) {
        // NOTE: indexes vs EtherCAT port order
        self.0[0].dc_receive_time = time_p0;
        self.0[1].dc_receive_time = time_p3;
        self.0[2].dc_receive_time = time_p1;
        self.0[3].dc_receive_time = time_p2;
    }

    /// TEST ONLY: Set downstream ports.
    #[cfg(test)]
    pub(crate) fn set_downstreams(
        &mut self,
        d0: Option<u16>,
        d3: Option<u16>,
        d1: Option<u16>,
        d2: Option<u16>,
    ) -> Self {
        self.0[0].downstream_to = d0;
        self.0[1].downstream_to = d3;
        self.0[2].downstream_to = d1;
        self.0[3].downstream_to = d2;

        *self
    }

    fn open_ports(&self) -> u8 {
        self.active_ports().count() as u8
    }

    fn active_ports(&self) -> impl Iterator<Item = &Port> + Clone {
        self.0.iter().filter(|port| port.active)
    }

    /// The port of the SubDevice that first sees EtherCAT traffic.
    pub fn entry_port(&self) -> Port {
        fmt::unwrap_opt!(self
            .active_ports()
            .min_by_key(|port| port.dc_receive_time)
            .copied())
    }

    /// Get the last open port.
    pub fn last_port(&self) -> Option<&Port> {
        self.active_ports().last()
    }

    /// Find the next port that hasn't already been assigned as the upstream port of another
    /// SubDevice.
    fn next_assignable_port(&mut self, this_port: &Port) -> Option<&mut Port> {
        let this_port_index = this_port.index();

        let next_port_index = self
            .active_ports()
            .cycle()
            // Start at the next port
            .skip(this_port_index + 1)
            .take(4)
            .find(|next_port| next_port.downstream_to.is_none())?
            .index();

        self.0.get_mut(next_port_index)
    }

    /// Link a downstream device to the current device using the next open port from the entry port.
    pub fn assign_next_downstream_port(&mut self, downstream_subdevice_index: u16) -> Option<u8> {
        let entry_port = self.entry_port();

        let next_port = self.next_assignable_port(&entry_port)?;

        next_port.downstream_to = Some(downstream_subdevice_index);

        Some(next_port.number)
    }

    /// Find the port assigned to the given SubDevice.
    pub fn port_assigned_to(&self, subdevice: &SubDevice) -> Option<&Port> {
        self.active_ports()
            .find(|port| port.downstream_to == Some(subdevice.index))
    }

    pub fn topology(&self) -> Topology {
        match self.open_ports() {
            1 => Topology::LineEnd,
            2 => Topology::Passthrough,
            3 => Topology::Fork,
            4 => Topology::Cross,
            n => unreachable!("Invalid topology {}", n),
        }
    }

    pub fn is_last_port(&self, port: &Port) -> bool {
        self.last_port().filter(|p| *p == port).is_some()
    }

    /// The time in nanoseconds for a packet to completely traverse all active ports of a SubDevice.
    pub fn total_propagation_time(&self) -> Option<u32> {
        let times = self
            .0
            .iter()
            .filter_map(|port| port.active.then_some(port.dc_receive_time));

        times
            .clone()
            .max()
            .and_then(|max| times.min().map(|min| max - min))
            .filter(|t| *t > 0)
    }

    /// Propagation time between active ports in this SubDevice.
    pub fn intermediate_propagation_time_to(&self, port: &Port) -> u32 {
        // If a pair of ports is open, they have a propagation delta between them, and we can sum
        // these deltas up to get the child delays of this SubDevice (fork or cross have children)
        self.0
            .windows(2)
            .map(|window| {
                // Silly Rust
                let [a, b] = window else { return 0 };

                // Stop iterating as we've summed everything before the target port
                if a.index() >= port.index() {
                    return 0;
                }

                // Both ports must be active to have a delta
                if a.active && b.active {
                    b.dc_receive_time.saturating_sub(a.dc_receive_time)
                } else {
                    0
                }
            })
            .sum::<u32>()
    }

    /// Get the propagation time taken from entry to this SubDevice up to the given port.
    pub fn propagation_time_to(&self, this_port: &Port) -> Option<u32> {
        let entry_port = self.entry_port();

        // Find active ports between entry and this one
        let times = self
            .active_ports()
            .filter(|port| port.index() >= entry_port.index() && port.index() <= this_port.index())
            .map(|port| port.dc_receive_time);

        times
            .clone()
            .max()
            .and_then(|max| times.min().map(|min| max - min))
            .filter(|t| *t > 0)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const ENTRY_RECEIVE: u32 = 1234;

    pub(crate) fn make_ports(active0: bool, active3: bool, active1: bool, active2: bool) -> Ports {
        let mut ports = Ports::new(active0, active3, active1, active2);

        ports.0[0].dc_receive_time = ENTRY_RECEIVE;
        ports.0[1].dc_receive_time = ENTRY_RECEIVE + 100;
        ports.0[2].dc_receive_time = ENTRY_RECEIVE + 200;
        ports.0[3].dc_receive_time = ENTRY_RECEIVE + 300;

        ports
    }

    #[test]
    fn open_ports() {
        // EK1100 with children attached to port 3 and downstream devices on port 1
        let ports = make_ports(true, true, true, false);
        // Normal SubDevice has no children, so no child delay
        let passthrough = make_ports(true, true, false, false);

        assert_eq!(ports.open_ports(), 3);
        assert_eq!(passthrough.open_ports(), 2);
    }

    #[test]
    fn topologies() {
        let passthrough = make_ports(true, true, false, false);
        let passthrough_skip_port = make_ports(true, false, true, false);
        let fork = make_ports(true, true, true, false);
        let line_end = make_ports(true, false, false, false);
        let cross = make_ports(true, true, true, true);

        assert_eq!(passthrough.topology(), Topology::Passthrough);
        assert_eq!(passthrough_skip_port.topology(), Topology::Passthrough);
        assert_eq!(fork.topology(), Topology::Fork);
        assert_eq!(line_end.topology(), Topology::LineEnd);
        assert_eq!(cross.topology(), Topology::Cross);
    }

    #[test]
    fn entry_port() {
        // EK1100 with children attached to port 3 and downstream devices on port 1
        let ports = make_ports(true, true, true, false);

        assert_eq!(
            ports.entry_port(),
            Port {
                active: true,
                number: 0,
                dc_receive_time: ENTRY_RECEIVE,
                ..Port::default()
            }
        );
    }

    #[test]
    fn propagation_time() {
        // Passthrough SubDevice
        let ports = make_ports(true, true, false, false);

        assert_eq!(ports.total_propagation_time(), Some(100));
    }

    #[test]
    fn propagation_time_fork() {
        // Fork, e.g. EK1100 with modules AND downstream devices
        let ports = make_ports(true, true, true, false);

        assert_eq!(ports.total_propagation_time(), Some(200));
    }

    #[test]
    fn propagation_time_cross() {
        // Cross, e.g. EK1122 in a module chain with both ports connected
        let ports = make_ports(true, true, true, true);

        assert_eq!(ports.topology(), Topology::Cross);
        assert_eq!(ports.total_propagation_time(), Some(300));
    }

    #[test]
    fn assign_downstream_port() {
        let mut ports = make_ports(true, true, true, false);

        assert_eq!(
            ports.entry_port(),
            Port {
                active: true,
                dc_receive_time: ENTRY_RECEIVE,
                number: 0,
                downstream_to: None
            }
        );

        let port_number = ports.assign_next_downstream_port(1);

        assert_eq!(port_number, Some(3), "assign SubDevice idx 1");

        let port_number = ports.assign_next_downstream_port(2);

        assert_eq!(port_number, Some(1), "assign SubDevice idx 2");

        pretty_assertions::assert_eq!(
            ports,
            Ports([
                // Entry port
                Port {
                    active: true,
                    dc_receive_time: ENTRY_RECEIVE,
                    number: 0,
                    downstream_to: None,
                },
                Port {
                    active: true,
                    dc_receive_time: ENTRY_RECEIVE + 100,
                    number: 3,
                    downstream_to: Some(1),
                },
                Port {
                    active: true,
                    dc_receive_time: ENTRY_RECEIVE + 200,
                    number: 1,
                    downstream_to: Some(2),
                },
                Port {
                    active: false,
                    dc_receive_time: ENTRY_RECEIVE + 300,
                    number: 2,
                    downstream_to: None,
                }
            ])
        )
    }

    #[test]
    fn propagation_time_to_last_port() {
        let ports = make_ports(true, false, true, true);

        let last = ports.last_port().unwrap();

        assert_eq!(
            ports.propagation_time_to(last),
            ports.total_propagation_time()
        );
    }

    #[test]
    fn propagation_time_to_intermediate_port() {
        // Cross topology
        let ports = make_ports(true, true, true, true);

        let up_to = &ports.0[2];

        assert_eq!(ports.propagation_time_to(up_to), Some(200));
    }

    #[test]
    fn propagation_time_cross_first() {
        // Cross topology, e.g. EK1122
        let mut ports = make_ports(true, true, true, true);

        // Deltas are 1340ns, 1080ns and 290ns
        ports.set_receive_times(3699944655, 3699945995, 3699947075, 3699947365);

        // Device connected to EtherCAT port number 3 (second index)
        let up_to = &ports.0[1];

        assert_eq!(ports.propagation_time_to(up_to), Some(1340));
    }

    #[test]
    fn propagation_time_cross_second() {
        // Cross topology, e.g. EK1122
        let mut ports = make_ports(true, true, true, true);

        // Deltas are 1340ns, 1080ns and 290ns
        ports.set_receive_times(3699944655, 3699945995, 3699947075, 3699947365);

        // Device connected to EtherCAT port number 3 (second index)
        let up_to = &ports.0[2];

        assert_eq!(ports.propagation_time_to(up_to), Some(1340 + 1080));
    }
}
