use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireWriteSized};

#[test]
fn signed_byte_enum() {
    #[derive(Debug, Copy, Clone, EtherCrabWireReadWrite)]
    #[repr(i8)]
    enum SignedByte {
        Foo = -10,
        Bar,
        Baz,
    }

    #[allow(unused)]
    #[repr(i8)]
    enum NotDerived {
        Foo = -10,
        Bar,
        Baz,
    }

    assert_eq!(NotDerived::Bar as i8, -9);
    assert_eq!(SignedByte::Bar.pack(), [-9i8 as u8]);
    // Just sanity checking my self here
    assert_eq!(SignedByte::Bar.pack(), [247u8]);
}

#[test]
fn signed_enum_i32() {
    #[derive(Debug, PartialEq, Copy, Clone, EtherCrabWireReadWrite)]
    #[repr(i32)]
    enum BigBoy {
        Foo = 0x00bbccdd,
        Bar = -2_147_483_648,
        Baz = -1073741824,
    }

    assert_eq!(BigBoy::unpack_from_slice(&[0, 0, 0, 192]), Ok(BigBoy::Baz));
    assert_eq!(
        BigBoy::unpack_from_slice(&[0xdd, 0xcc, 0xbb, 0x00]),
        Ok(BigBoy::Foo)
    );
}
