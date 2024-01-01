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

#[test]
fn basic_enum_byte() {
    #[derive(Debug, EtherCatWire, num_enum::TryFromPrimitive)]
    #[repr(u8)]
    #[wire(bits = 8)]
    enum Check {
        Foo = 0x01,
        Bar = 0x02,
        Baz = 0xaa,
        Quux = 0xef,
    }

    dbg!(Check::try_from_primitive(0xaau8));

    dbg!(u8::from(Check::Foo));
}
