use ethercrab_wire::{
    EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireWrite, EtherCrabWireWriteSized,
};

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

#[test]
fn cia402_control_word() {
    #[derive(ethercrab_wire::EtherCrabWireReadWrite, Debug, Eq, PartialEq)]
    #[wire(bytes = 2)]
    pub struct ControlWord {
        /// bit 0, switch on
        #[wire(bits = 1)]
        pub so: bool,

        /// bit 1, enable voltage
        #[wire(bits = 1)]
        pub ev: bool,

        /// bit 2, quick stop
        #[wire(bits = 1)]
        pub qs: bool,

        /// bit 3, enable operation
        #[wire(bits = 1)]
        pub eo: bool,

        /// bit 4, operation mode specific
        #[wire(bits = 1)]
        pub oms_1: bool,

        /// bit 5, operation mode specific
        #[wire(bits = 1)]
        pub oms_2: bool,

        /// bit 6, operation mode specific
        #[wire(bits = 1)]
        pub oms_3: bool,

        /// bit 7, fault reset
        #[wire(bits = 1)]
        pub fr: bool,

        /// bit 8, immediate stop
        #[wire(bits = 1)]
        pub halt: bool,

        /// bit 9, immediate stop
        #[wire(bits = 1, post_skip = 6)]
        pub oms_4: bool,
    }

    let mut cw = ControlWord {
        so: true,
        ev: true,
        qs: true,
        eo: true,
        oms_1: true,
        oms_2: false,
        oms_3: false,
        fr: true,
        halt: false,
        oms_4: false,
    };

    let mut buf = cw.pack();

    assert_eq!(buf, [0b1001_1111, 0b0000_0000]);

    // Change some flags, so when we pack to the buffer again we can make sure they're updated
    // properly.
    cw.so = false;
    cw.ev = true;
    cw.qs = true;
    cw.fr = false;

    cw.pack_to_slice(&mut buf).unwrap();

    let cw2 = ControlWord::unpack_from_slice(&buf).unwrap();

    assert_eq!(cw, cw2);
}

#[test]
fn sized() {
    #[derive(ethercrab_wire::EtherCrabWireRead)]
    #[wire(bytes = 9)]
    struct DriveState {
        #[wire(bytes = 4)]
        actual_position: u32,
        #[wire(bytes = 4)]
        actual_velocity: u32,
        #[wire(bits = 4)]
        status_word: u8,
        #[wire(bits = 1)]
        di0: bool,
        #[wire(bits = 1)]
        di1: bool,
        #[wire(bits = 1)]
        di2: bool,
        #[wire(bits = 1)]
        di3: bool,
    }

    #[derive(Copy, Clone, ethercrab_wire::EtherCrabWireWrite)]
    #[wire(bytes = 1)]
    #[repr(u8)]
    enum ControlState {
        Init = 0x01,
        Conf = 0x04,
        Op = 0xaa,
    }

    #[derive(ethercrab_wire::EtherCrabWireWrite)]
    #[wire(bytes = 5)]
    struct DriveControl {
        #[wire(bytes = 4)]
        target_position: u32,
        #[wire(bytes = 1)]
        control_state: ControlState,
    }
}
