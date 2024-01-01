//! Traits used to pack/unpack structs and enums from EtherCAT packes on the wire.
//!
//! Internal only, please do not implement outside EtherCrab.

pub trait WireEnum {
    const BITS: usize;
    const BYTES: usize;
}
