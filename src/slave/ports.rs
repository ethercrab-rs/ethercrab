use crate::Slave;
use core::fmt::{self, Debug};

/// Flags showing which ports are active or not on the slave.
#[derive(Default, Debug, PartialEq, Eq, Copy, Clone)]
pub struct Port {
    pub active: bool,
    pub dc_receive_time: u32,
    /// The EtherCAT port number, ordered as 0 -> 3 -> 1 -> 2.
    pub number: usize,
    /// Holds the index of the downstream slave this port is connected to.
    pub downstream_to: Option<usize>,
}

impl Port {
    fn index(&self) -> usize {
        match self.number {
            0 => 0,
            3 => 1,
            1 => 2,
            2 => 3,
            n => unreachable!("Invalid port number {n}"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Topology {
    /// The slave device has two open ports, with only upstream and downstream slaves.
    Passthrough,
    /// The slave device is the last device in its fork of the topology tree, with only one open
    /// port.
    LineEnd,
    /// The slave device forms a fork in the topology, with 3 open ports.
    Fork,
}

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub struct Ports(pub [Port; 4]);

impl fmt::Display for Ports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    fn open_ports(&self) -> u8 {
        self.active_ports().count() as u8
    }

    fn active_ports(&self) -> impl Iterator<Item = &Port> {
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
    fn next_assignable_port(&mut self, port: &Port) -> Option<&mut Port> {
        let mut index = port.index();

        for _ in 0..4 {
            index = (index + 1) % 4;

            let next_port = self.0[index];

            if next_port.active && next_port.downstream_to.is_none() {
                break;
            }
        }

        self.0.get_mut(index)
    }

    /// Find the next open port after the given port.
    fn next_open_port(&self, port: &Port) -> Option<&Port> {
        let mut index = port.index();

        for _ in 0..4 {
            index = (index + 1) % 4;

            let next_port = &self.0[index];

            if next_port.active {
                return Some(next_port);
            }
        }

        None
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
            // TODO: I need test devices!
            4 => todo!("Cross topology not yet supported"),
            n => unreachable!("Invalid topology {n}"),
        }
    }

    pub fn is_last_port(&self, port: &Port) -> bool {
        self.last_port().filter(|p| *p == port).is_some()
    }

    /// If the current node is a fork in the network, compute the propagation delay of all the
    /// children.
    ///
    /// Returns `None` if the current node is not a fork.
    pub fn child_delay(&self) -> Option<u32> {
        if self.topology() == Topology::Fork {
            let input_port = self.entry_port()?;

            // Because this is a fork, the slave's children will always be attached to the next open
            // port after the input.
            let children_port = self.next_open_port(&input_port)?;

            Some(children_port.dc_receive_time - input_port.dc_receive_time)
        } else {
            None
        }
    }

    /// The time in nanoseconds for a packet to completely traverse all active ports of a slave
    /// device.
    pub fn propagation_time(&self) -> Option<u32> {
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
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const ENTRY_RECEIVE: u32 = 1234;

    pub(crate) fn make_ports(active0: bool, active3: bool, active1: bool, active2: bool) -> Ports {
        Ports([
            Port {
                active: active0,
                number: 0,
                dc_receive_time: ENTRY_RECEIVE,
                ..Port::default()
            },
            Port {
                active: active3,
                number: 3,
                dc_receive_time: ENTRY_RECEIVE + 100,
                ..Port::default()
            },
            Port {
                active: active1,
                number: 1,
                dc_receive_time: ENTRY_RECEIVE + 200,
                ..Port::default()
            },
            Port {
                active: active2,
                number: 2,
                dc_receive_time: ENTRY_RECEIVE + 300,
                ..Port::default()
            },
        ])
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

        assert_eq!(ports.propagation_time(), Some(100));
    }

    #[test]
    fn propagation_time_fork() {
        // Fork, e.g. EK1100 with modules AND downstream devices
        let ports = make_ports(true, true, true, false);

        assert_eq!(ports.propagation_time(), Some(200));
    }

    #[test]
    fn child_delay() {
        // EK1100 with children attached to port 3 and downstream devices on port 1
        let ports = make_ports(true, true, true, false);
        // Normal slave has no children, so no child delay
        let passthrough = make_ports(true, true, false, false);

        assert_eq!(ports.child_delay(), Some(100));
        assert_eq!(passthrough.child_delay(), None);
    }

    #[test]
    fn assign_downstream_port() {
        let mut ports = make_ports(true, true, true, false);

        let port_number = ports.assign_next_downstream_port(1);

        assert_eq!(port_number, Some(3), "assign slave idx 1");

        let port_number = ports.assign_next_downstream_port(2);

        assert_eq!(port_number, Some(1), "assign slave idx 2");
    }
}
