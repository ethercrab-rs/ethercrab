#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(not(test), derive(ethercrab_wire::EtherCrabWireRead))]
#[cfg_attr(
    test,
    derive(arbitrary::Arbitrary, ethercrab_wire::EtherCrabWireReadWrite)
)]
#[wire(bytes = 2)]
pub struct DlStatus {
    #[wire(bits = 1)]
    pub pdi_operational: bool,
    #[wire(bits = 1)]
    pub watchdog_ok: bool,
    #[wire(bits = 1, post_skip = 1)]
    pub extended_link_detection: bool,
    // pub _reserved: bool,
    /// True if port 0 has a physical link present.
    #[wire(bits = 1)]
    pub link_port0: bool,
    /// True if port 1 has a physical link present.
    #[wire(bits = 1)]
    pub link_port1: bool,
    /// True if port 2 has a physical link present.
    #[wire(bits = 1)]
    pub link_port2: bool,
    /// True if port 3 has a physical link present.
    #[wire(bits = 1)]
    pub link_port3: bool,

    /// True if port 0 forwards to itself (i.e. loopback)
    #[wire(bits = 1)]
    pub loopback_port0: bool,
    /// RX signal detected on port 0
    #[wire(bits = 1)]
    pub signal_port0: bool,
    /// True if port 1 forwards to itself (i.e. loopback)
    #[wire(bits = 1)]
    pub loopback_port1: bool,
    /// RX signal detected on port 1
    #[wire(bits = 1)]
    pub signal_port1: bool,
    /// True if port 2 forwards to itself (i.e. loopback)
    #[wire(bits = 1)]
    pub loopback_port2: bool,
    /// RX signal detected on port 2
    #[wire(bits = 1)]
    pub signal_port2: bool,
    /// True if port 3 forwards to itself (i.e. loopback)
    #[wire(bits = 1)]
    pub loopback_port3: bool,
    /// RX signal detected on port 3
    #[wire(bits = 1)]
    pub signal_port3: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dl_status_fuzz() {
        heckcheck::check(|status: DlStatus| {
            let mut buf = [0u8; 16];

            let packed = status.pack_to_slice(&mut buf).expect("Pack");

            let unpacked = DlStatus::unpack_from_slice(packed).expect("Unpack");

            pretty_assertions::assert_eq!(status, unpacked);

            Ok(())
        });
    }
}
