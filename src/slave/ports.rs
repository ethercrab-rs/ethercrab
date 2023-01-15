use core::fmt::Debug;

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

#[derive(Default, Copy, Clone, Debug)]
pub struct Ports(pub [Port; 4]);

impl Ports {
    fn open_ports(&self) -> u8 {
        self.0.iter().filter(|port| port.active).count() as u8
    }

    /// The port of the slave that first sees EtherCAT traffic.
    pub fn entry_port(&self) -> Option<Port> {
        self.0
            .into_iter()
            .filter(|port| port.active)
            .min_by_key(|port| port.dc_receive_time)
    }

    fn last_port(&self) -> Option<Port> {
        self.0
            .into_iter()
            .filter(|port| port.active)
            .max_by_key(|port| port.dc_receive_time)
    }

    // fn port_by_number(&self, number: impl Into<usize>) -> &Port {
    //     let number: usize = number.into();

    //     self.0.iter().find(|port| port.number == number).unwrap()
    // }

    // pub fn port_by_number_mut(&mut self, number: impl Into<usize>) -> &mut Port {
    //     let number: usize = number.into();

    //     self.0
    //         .iter_mut()
    //         .find(|port| port.number == number)
    //         .unwrap()
    // }

    /// Find the next port that hasn't already been assigned as the upstream port of another slave.
    fn next_assignable_port(&mut self, port: &Port) -> Option<&mut Port> {
        let mut number = port.number;
        let mut port = None;

        for _ in 0..4 {
            // let next_number = match number {
            //     0 => 1,
            //     3 => 1,
            //     1 => 2,
            //     2 => 0,
            //     _ => unreachable!(),
            // };

            let next_number = (number + 1) % 4;

            let next_port = self.0[next_number];

            if next_port.active && next_port.downstream_to.is_none() {
                port = Some(next_port.number);

                break;
            }

            number = next_number;
        }

        let port = port?;

        self.0.get_mut(port)
    }

    /// Find the next open port after the given port.
    fn next_open_port(&self, port: &Port) -> Option<&Port> {
        let mut number = port.number;

        for _ in 0..4 {
            // let next_number = match number {
            //     0 => 3usize,
            //     3 => 1,
            //     1 => 2,
            //     2 => 0,
            //     _ => unreachable!(),
            // };
            let next_number = (number + 1) % 4;

            let next_port = &self.0[next_number];

            if next_port.active {
                return Some(next_port);
            }

            number = next_number;
        }

        None
    }

    pub fn prev_open_port(&self, port: &Port) -> Option<&Port> {
        let mut number = port.number;

        for _ in 0..4 {
            // let next_number = match number {
            //     0 => 2usize,
            //     2 => 1,
            //     1 => 3,
            //     3 => 0,
            //     _ => unreachable!(),
            // };

            let next_number = if number == 0 { 3 } else { number - 1 };

            let next_port = &self.0[next_number];

            if next_port.active {
                return Some(next_port);
            }

            number = next_number;
        }

        None
    }

    pub fn assign_next_downstream_port(&mut self, downstream_slave_index: usize) -> Option<usize> {
        let entry_port = self.entry_port().expect("No input port? Wtf");

        let next_port = self.next_assignable_port(&entry_port)?;

        next_port.downstream_to = Some(downstream_slave_index);

        Some(next_port.number)
    }

    pub fn topology(&self) -> Topology {
        match self.open_ports() {
            1 => Topology::LineEnd,
            2 => Topology::Passthrough,
            3 => Topology::Fork,
            // TODO: I need test devices!
            4 => todo!("Cross topology not yet supported"),
            _ => unreachable!(),
        }
    }

    pub fn is_last_port(&self, port: &Port) -> bool {
        self.last_port().filter(|p| p == port).is_some()
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

    pub(crate) fn make_ports(active0: bool, active3: bool, active1: bool, active2: bool) -> Ports {
        Ports([
            Port {
                active: active0,
                number: 0,
                dc_receive_time: 0,
                downstream_to: None,
            },
            Port {
                active: active3,
                number: 3,
                dc_receive_time: 0,
                downstream_to: None,
            },
            Port {
                active: active1,
                number: 1,
                dc_receive_time: 0,
                downstream_to: None,
            },
            Port {
                active: active2,
                number: 2,
                dc_receive_time: 0,
                downstream_to: None,
            },
        ])
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
}
