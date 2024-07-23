use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite, EtherCrabWireWriteSized};

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 16)]
struct NormalTypes {
    #[wire(bytes = 4)]
    foo: u32,

    #[wire(bytes = 2)]
    bar: u16,

    #[wire(bytes = 2)]
    baz: u16,

    #[wire(bytes = 8)]
    huge: u64,
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 5)]
struct Mixed {
    #[wire(bits = 1)]
    a: u8,
    #[wire(bits = 2)]
    b: u8,
    #[wire(bits = 3)]
    c: u8,
    #[wire(bits = 2)]
    d: u8,

    /// Whole u8
    #[wire(bytes = 1)]
    e: u8,

    #[wire(bits = 3)]
    f: u8,
    #[wire(bits = 3)]
    g: u8,
    #[wire(bits = 2)]
    h: u8,

    /// Whole u16
    #[wire(bytes = 2)]
    i: u16,
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 2)]
struct Packed {
    #[wire(bits = 1)]
    a: u8,
    #[wire(bits = 2)]
    b: u8,
    #[wire(bits = 3)]
    c: u8,
    #[wire(bits = 2)]
    d: u8,
    #[wire(bits = 3)]
    f: u8,
    #[wire(bits = 3)]
    g: u8,
    #[wire(bits = 2)]
    h: u8,
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
enum OneByte {
    Foo = 0x01,
    Bar = 0x02,
    Baz = 0x03,
    Quux = 0xab,
    #[wire(catch_all)]
    Unknown(u8),
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 12)]
struct Inner {
    #[wire(bytes = 2)]
    foo: Packed,
    #[wire(bytes = 4)]
    bar: u32,
    #[wire(bytes = 5)]
    baz: Mixed,
    #[wire(bytes = 1)]
    quux: OneByte,
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 36)]
struct LotsOfNesting {
    #[wire(bytes = 2)]
    something: u16,
    #[wire(bytes = 12)]
    inner: Inner,
    #[wire(bytes = 16)]
    normal: NormalTypes,
    #[wire(bytes = 5)]
    mixed: Mixed,
    #[wire(bytes = 1)]
    one_byte: OneByte,
}

pub fn nested(c: &mut Criterion) {
    let input_data = [
        0x47, 0x0e, 0xb2, 0x9d, 0x66, 0x83, 0x21, 0xf3, 0xae, 0xa4, 0x38, 0x89, 0xaf, 0xae, 0xf7,
        0x19, 0x44, 0x8c, 0x7d, 0x80, 0xc2, 0x36, 0xd0, 0xb3, 0x40, 0x35, 0x76, 0x9c, 0xf3, 0xe3,
        0x7d, 0x70, 0x4f, 0x11, 0xa5, 0x60,
    ];

    c.bench_function("nested struct unpack", |b| {
        b.iter(|| LotsOfNesting::unpack_from_slice(black_box(&input_data)))
    });

    let instance = LotsOfNesting::unpack_from_slice(&input_data).unwrap();

    c.bench_function("nested struct pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("nested struct pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 36];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("nested struct pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 36];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

criterion_group!(large, nested);
criterion_main!(large);
