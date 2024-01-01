use ethercrab::derive::WireStruct;
use ethercrab_derive::EtherCatWire;
use num_enum::TryFromPrimitive;

// #[test]
// fn one_bit() {
//     #[derive(Debug, EtherCatWire)]
//     struct Bit {
//         #[wire(0..=1)]
//         foo: u8,
//     }
// }

// #[test]
// fn one_byte() {
//     #[derive(Debug, EtherCatWire)]
//     struct Bit {
//         #[wire(0..=8)]
//         foo: u8,
//     }
// }

// #[test]
// fn basic_enum_byte() {
//     #[derive(Debug, EtherCatWire, num_enum::TryFromPrimitive)]
//     #[repr(u8)]
//     #[wire(bits = 8)]
//     enum Check {
//         Foo = 0x01,
//         Bar = 0x02,
//         Baz = 0xaa,
//         Quux = 0xef,
//     }

//     dbg!(Check::try_from_primitive(0xaau8));

//     // Maybe I'll need this
//     // dbg!(u8::from(Check::Foo));
// }

#[test]
fn basic_struct() {
    #[derive(Debug, EtherCatWire)]
    #[wire(bits = 56)]
    struct Check {
        #[wire(bits = 8)]
        foo: u8,
        #[wire(bits = 16)]
        bar: u16,
        #[wire(bits = 32)]
        baz: u32,
    }

    let check = Check {
        foo: 0xaa,
        bar: 0xbbcc,
        baz: 0x33445566,
    };

    let mut buf = [0u8; 16];

    let out = check.pack_to_slice(&mut buf).unwrap();

    let expected = [
        0xaau8, // u8
        0xcc, 0xbb, // u16
        0x66, 0x55, 0x44, 0x33, // u32
    ];

    assert_eq!(out, &expected);
}

#[test]
fn pack_struct_single_byte() {
    #[derive(Debug, EtherCatWire)]
    #[wire(bits = 8)]
    struct Check {
        #[wire(bits = 2)]
        foo: u8,
        #[wire(bits = 3)]
        bar: u8,
        #[wire(bits = 3)]
        baz: u8,
    }

    let check = Check {
        foo: 0b11,
        bar: 0b101,
        baz: 0x010,
    };
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
        #[wire(bits = 3)]
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

// // If I don't need this I won't implement it because it makes things a bunch more complex.
// #[test]
// fn u16_across_bytes() {
//     #[derive(Debug, EtherCatWire)]
//     #[wire(bits = 24)]
//     struct Check {
//         #[wire(bits = 3)]
//         foo: u8,
//         #[wire(bits = 16)]
//         bar: u16,
//         #[wire(bits = 5)]
//         baz: u8,
//     }

//     let check = Check {
//         foo: 0b00,
//         bar: u16::MAX,
//         baz: 0x00000,
//     };
// }
