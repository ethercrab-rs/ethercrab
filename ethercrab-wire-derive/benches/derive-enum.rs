use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite, EtherCrabWireWriteSized};

pub fn fallible(c: &mut Criterion) {
    #[derive(Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
    #[repr(u8)]
    enum OneByte {
        Foo = 0x01,
        Bar = 0x02,
        Baz = 0x03,
        Quux = 0xab,
    }

    let input_data = [0xab];

    c.bench_function("enum 1 byte unpack", |b| {
        b.iter(|| OneByte::unpack_from_slice(black_box(&input_data)))
    });

    let instance = OneByte::unpack_from_slice(&input_data).unwrap();

    c.bench_function("enum 1 byte pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("enum 1 byte pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("enum 1 byte pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

pub fn infallible(c: &mut Criterion) {
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

    let input_data = [0xff];

    c.bench_function("enum 1 byte unknown unpack", |b| {
        b.iter(|| OneByte::unpack_from_slice(black_box(&input_data)))
    });

    let instance = OneByte::unpack_from_slice(&input_data).unwrap();

    c.bench_function("enum 1 byte unknown pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("enum 1 byte unknown pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("enum 1 byte unknown pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

criterion_group!(enums, fallible, infallible);
criterion_main!(enums);
