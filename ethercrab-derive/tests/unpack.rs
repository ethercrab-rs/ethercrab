use ethercrab::derive::WireField;
use ethercrab_derive::EtherCatWire;

#[test]
fn basic_struct() {
    #[derive(Debug, EtherCatWire, PartialEq)]
    #[wire(bits = 56)]
    struct Check {
        #[wire(bits = 8)]
        foo: u8,
        #[wire(bits = 16)]
        bar: u16,
        #[wire(bits = 32)]
        baz: u32,
    }

    let expected = Check {
        foo: 0xaa,
        bar: 0xbbcc,
        baz: 0x33445566,
    };

    let buf = [
        0xaau8, // u8
        0xcc, 0xbb, // u16
        0x66, 0x55, 0x44, 0x33, // u32
    ];

    let out = Check::unpack_from_slice(&buf).unwrap();

    assert_eq!(out, expected);
}

#[test]
fn pack_struct_nested_enum() {
    #[derive(Debug, Copy, Clone, EtherCatWire, num_enum::TryFromPrimitive)]
    #[repr(u8)]
    enum Nested {
        Foo = 0x01,
        Bar = 0x02,
        Baz = 0x03,
        Quux = 0x04,
    }

    #[derive(Debug, EtherCatWire)]
    #[wire(bits = 8)]
    struct Check {
        #[wire(bits = 2)]
        foo: u8,
        #[wire(bits = 3, ty = "enum")]
        bar: Nested,
        #[wire(bits = 3)]
        baz: u8,
    }

    let check = Check {
        foo: 0b11,
        bar: Nested::Baz,
        baz: 0x010,
    };

    let expected = [0b11 | (0x03 << 2) | (0x010 << 5)];

    let mut buf = [0u8; 16];

    let out = check.pack_to_slice(&mut buf).unwrap();

    assert_eq!(out, &expected);
}

#[test]
fn nested_structs() {
    #[derive(Debug, EtherCatWire)]
    #[wire(bits = 80)]
    struct Check {
        #[wire(bits = 32)]
        foo: u32,
        #[wire(bits = 24)]
        control: Inner,
        #[wire(bits = 24)]
        status: Inner,
    }

    #[derive(Debug, Copy, Clone, EtherCatWire)]
    #[wire(bits = 24)]
    struct Inner {
        #[wire(bits = 1)]
        yes: bool,
        #[wire(bits = 1)]
        no: bool,
        #[wire(pre_skip = 6, bits = 16)]
        stuff: u16,
    }

    let check = Check {
        foo: 0x44556677,
        control: Inner {
            yes: true,
            no: false,
            stuff: 0xaabb,
        },
        status: Inner {
            yes: false,
            no: true,
            stuff: 0xccdd,
        },
    };

    let expected = [
        0x77u8, 0x66u8, 0x55u8, 0x44u8, // u32
        0b01, 0xbb, 0xaa, // Control
        0b10, 0xdd, 0xcc, // Status
    ];

    let mut buf = [0u8; 16];

    let out = check.pack_to_slice(&mut buf).unwrap();

    assert_eq!(out, &expected);
}
