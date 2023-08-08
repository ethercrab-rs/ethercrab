use crate::fmt;
use crate::Slave;
use core::fmt::Debug;

/// Flags showing which ports are active or not on the slave.
#[derive(Default, Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Port {
    pub active: bool,
    pub dc_receive_time: u32,
    /// The EtherCAT port number, ordered as 0 -> 3 -> 1 -> 2.
    pub number: usize,
    /// Holds the index of the downstream slave this port is connected to.
    pub downstream_to: Option<usize>,
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
    /// The slave device has two open ports, with only upstream and downstream slaves.
    Passthrough,
    /// The slave device is the last device in its fork of the topology tree, with only one open
    /// port.
    LineEnd,
    /// The slave device forms a fork in the topology, with 3 open ports.
    Fork,
    /// The slave device forms a cross in the topology, with 4 open ports.
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
        d0: Option<usize>,
        d3: Option<usize>,
        d1: Option<usize>,
        d2: Option<usize>,
    ) {
        self.0[0].downstream_to = d0;
        self.0[1].downstream_to = d3;
        self.0[2].downstream_to = d1;
        self.0[3].downstream_to = d2;
    }

    fn open_ports(&self) -> u8 {
        self.active_ports().count() as u8
    }

    fn active_ports(&self) -> impl Iterator<Item = &Port> + Clone {
        self.0.iter().filter(|port| port.active)
    }

    /// The port of the slave that first sees EtherCAT traffic.
    pub fn entry_port(&self) -> Option<Port> {
        self.active_ports()
            .min_by_key(|port| port.dc_receive_time)
            .copied()
    }

    /// Get the last open port.
    pub fn last_port(&self) -> Option<&Port> {
        self.active_ports().last()
    }

    /// Find the next port that hasn't already been assigned as the upstream port of another slave.
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

    /// Find the next open port after the given port.
    fn next_open_port(&self, port: &Port) -> Option<&Port> {
        let index = port.index();

        self.active_ports().cycle().skip(index + 1).next()
    }

    /// Link a downstream device to the current device using the next open port from the entry port.
    pub fn assign_next_downstream_port(&mut self, downstream_slave_index: usize) -> Option<usize> {
        let entry_port = self.entry_port().expect("No input port? Wtf");

        let next_port = self.next_assignable_port(&entry_port)?;

        next_port.downstream_to = Some(downstream_slave_index);

        Some(next_port.number)
    }

    /// Find the port assigned to the given slave device.
    pub fn port_assigned_to(&self, slave: &Slave) -> Option<&Port> {
        self.active_ports()
            .find(|port| port.downstream_to == Some(slave.index))
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

    /// If the current node is a fork in the network, compute the propagation delay of all the
    /// children.
    ///
    /// Returns `None` if the current node is not a fork.
    pub fn fork_child_delay(&self) -> Option<u32> {
        if self.topology() == Topology::Fork {
            let input_port = self.entry_port()?;

            // Because this is a fork, the slave's children will always be attached to the next open
            // port after the input.
            let children_port = self.next_open_port(&input_port)?;

            // Some(children_port.dc_receive_time - input_port.dc_receive_time)

            self.propagation_time_to(children_port)
        } else {
            None
        }
    }

    /// The time in nanoseconds for a packet to completely traverse all active ports of a slave
    /// device.
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

    /// Propagation time between active ports in this slave.
    pub fn intermediate_propagation_time(&self) -> u32 {
        // let time_p0 = self.0[0].dc_receive_time;
        // let time_p3 = self.0[1].dc_receive_time;
        // let time_p1 = self.0[2].dc_receive_time;
        // let time_p2 = self.0[3].dc_receive_time;

        // // Deltas between port receive times
        // let d03 = time_p3.saturating_sub(time_p0);
        // let d31 = time_p1.saturating_sub(time_p3);
        // let d12 = time_p2.saturating_sub(time_p1);

        // let d03 = if self.0[1].active { d03 } else { 0 };
        // let d31 = if self.0[2].active { d31 } else { 0 };
        // let d12 = if self.0[3].active { d12 } else { 0 };

        // d03 + d31 + d12

        // If a pair of ports is open, they have a propagation delta between them, and we can sum these deltas up to get the child delays of this slave (fork or cross have children)
        self.0
            .windows(2)
            .map(|window| {
                let [a, b] = window else {
            return 0
        };

                // Both ports must be active to have a delta
                if a.active && b.active {
                    b.dc_receive_time.saturating_sub(a.dc_receive_time)
                } else {
                    0
                }
            })
            .sum::<u32>()
    }

    /// Get the propagation time taken from entry to this slave device up to the given port.
    pub fn propagation_time_to(&self, this_port: &Port) -> Option<u32> {
        // If we don't have an entry port we can't be connected to the network so this is probably a
        // logic bug and should panic.
        let entry_port =
            crate::fmt::unwrap_opt!(self.entry_port(), "no entry port. Likely a logic bug");

        // Find active ports between entry and this one
        let times = self
            .active_ports()
            .filter(|port| port.index() >= entry_port.index() && port.index() <= this_port.index())
            .map(|port| port.dc_receive_time);

        let between_ports = times
            .clone()
            .max()
            .and_then(|max| times.min().map(|min| max - min))
            .filter(|t| *t > 0);

        between_ports
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
        // Normal slave has no children, so no child delay
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

        assert_eq!(passthrough.topology(), Topology::Passthrough);
        assert_eq!(passthrough_skip_port.topology(), Topology::Passthrough);
        assert_eq!(fork.topology(), Topology::Fork);
        assert_eq!(line_end.topology(), Topology::LineEnd);
    }

    #[test]
    fn entry_port() {
        // EK1100 with children attached to port 3 and downstream devices on port 1
        let ports = make_ports(true, true, true, false);

        assert_eq!(
            ports.entry_port(),
            Some(Port {
                active: true,
                number: 0,
                dc_receive_time: ENTRY_RECEIVE,
                ..Port::default()
            })
        );
    }

    #[test]
    fn propagation_time() {
        // Passthrough slave
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
    fn child_delay() {
        // EK1100 with children attached to port 3 and downstream devices on port 1
        let ports = make_ports(true, true, true, false);
        // Normal slave has no children, so no child delay
        let passthrough = make_ports(true, true, false, false);

        assert_eq!(ports.fork_child_delay(), Some(100));
        assert_eq!(passthrough.fork_child_delay(), None);
    }

    #[test]
    fn assign_downstream_port() {
        let mut ports = make_ports(true, true, true, false);

        assert_eq!(
            ports.entry_port(),
            Some(Port {
                active: true,
                dc_receive_time: ENTRY_RECEIVE,
                number: 0,
                downstream_to: None
            })
        );

        let port_number = ports.assign_next_downstream_port(1);

        assert_eq!(port_number, Some(3), "assign slave idx 1");

        let port_number = ports.assign_next_downstream_port(2);

        assert_eq!(port_number, Some(1), "assign slave idx 2");

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
}
