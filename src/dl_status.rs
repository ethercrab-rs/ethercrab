use crate::pdu_data::PduRead;
use ethercrab_wire::{EtherCatWire, WireError};

#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[wire(bits = 16)]
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

// impl PackedS/* truct for DlStatus {
//     type ByteArray = [u8; 2];

//     fn pack(&self) -> packed_struct::PackingResult<Self::ByteArray> {
//         let result = self.pdi_operational as u16
//             | (self.watchdog_ok as u16) << 1
//             | (self.extended_link_detection as u16) << 2
//             // | (self._reserved as u16) << 3
//             | (self.link_port0 as u16) << 4
//             | (self.link_port1 as u16) << 5
//             | (self.link_port2 as u16) << 6
//             | (self.link_port3 as u16) << 7
//             | (self.loopback_port0 as u16) << 8
//             | (self.signal_port0 as u16) << 9
//             | (self.loopback_port1 as u16) << 10
//             | (self.signal_port1 as u16) << 11
//             | (self.loopback_port2 as u16) << 12
//             | (self.signal_port2 as u16) << 13
//             | (self.loopback_port3 as u16) << 14
//             | (self.signal_port3 as u16) << 15;

//         Ok(result.to_le_bytes())
//     }

//     fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
//         let raw = u16::from_le_bytes(*src);

//         Ok(Self {
//             pdi_operational: (raw & 1) == 1,
//             watchdog_ok: (raw >> 1 & 1) == 1,
//             extended_link_detection: (raw >> 2 & 1) == 1,
//             // _reserved: (raw >> 3 & 1) == 1,
//             link_port0: (raw >> 4 & 1) == 1,
//             link_port1: (raw >> 5 & 1) == 1,
//             link_port2: (raw >> 6 & 1) == 1,
//             link_port3: (raw >> 7 & 1) == 1,
//             loopback_port0: (raw >> 8 & 1) == 1,
//             signal_port0: (raw >> 9 & 1) == 1,
//             loopback_port1: (raw >> 10 & 1) == 1,
//             signal_port1: (raw >> 11 & 1) == 1,
//             loopback_port2: (raw >> 12 & 1) == 1,
//             signal_port2: (raw >> 13 & 1) == 1,
//             loopback_port3: (raw >> 14 & 1) == 1,
//             signal_port3: (raw >> 15 & 1) == 1,
//         })
//     }
// } */
impl PduRead for DlStatus {
    const LEN: u16 = 2;

    type Error = WireError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dl_status_fuzz() {
        heckcheck::check(|status: DlStatus| {
            let packed = status.pack();

            let unpacked = DlStatus::unpack_from_slice(&packed).expect("Unpack");

            pretty_assertions::assert_eq!(status, unpacked);

            Ok(())
        });
    }
}
