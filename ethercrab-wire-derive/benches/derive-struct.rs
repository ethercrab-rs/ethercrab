use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite, EtherCrabWireWriteSized};

pub fn normal_types_repr_packed(c: &mut Criterion) {
    #[derive(ethercrab_wire::EtherCrabWireReadWrite)]
    #[wire(bytes = 16)]
    #[repr(C, packed)]
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

    let input_data = [
        0xbf, 0x7e, 0xc0, 0x07, 0xab, 0xa1, 0xc2, 0x22, 0x45, 0x23, 0xaa, 0x68, 0x47, 0xbf, 0xff,
        0xea,
    ];

    c.bench_function("u* [repr(C,packed)] struct unpack", |b| {
        b.iter(|| NormalTypes::unpack_from_slice(black_box(&input_data)))
    });

    let instance = NormalTypes::unpack_from_slice(&input_data).unwrap();

    c.bench_function("u* [repr(C,packed)] struct pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("u* [repr(C,packed)] struct pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("u* [repr(C,packed)] struct pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

pub fn normal_types(c: &mut Criterion) {
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

    let input_data = [
        0xbf, 0x7e, 0xc0, 0x07, 0xab, 0xa1, 0xc2, 0x22, 0x45, 0x23, 0xaa, 0x68, 0x47, 0xbf, 0xff,
        0xea,
    ];

    c.bench_function("u* struct unpack", |b| {
        b.iter(|| NormalTypes::unpack_from_slice(black_box(&input_data)))
    });

    let instance = NormalTypes::unpack_from_slice(&input_data).unwrap();

    c.bench_function("u* struct pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("u* struct pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("u* struct pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 16];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

pub fn packed_and_normal(c: &mut Criterion) {
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

    let input_data = [0xc7, 0x0c, 0xfe, 0xc6, 0x50];

    c.bench_function("mixed struct unpack", |b| {
        b.iter(|| Mixed::unpack_from_slice(black_box(&input_data)))
    });

    let instance = Mixed::unpack_from_slice(&input_data).unwrap();

    c.bench_function("mixed struct pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("mixed struct pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 5];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("mixed struct pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 5];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

pub fn packed(c: &mut Criterion) {
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

    let input_data = [0xc7, 0x0c, 0xfe, 0xc6, 0x50];

    c.bench_function("packed struct unpack", |b| {
        b.iter(|| Packed::unpack_from_slice(black_box(&input_data)))
    });

    let instance = Packed::unpack_from_slice(&input_data).unwrap();

    c.bench_function("packed struct pack array", |b| {
        b.iter(|| black_box(instance.pack()))
    });

    c.bench_function("packed struct pack slice unchecked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 5];

            instance.pack_to_slice_unchecked(black_box(&mut buf));
        })
    });

    c.bench_function("packed struct pack slice checked", |b| {
        b.iter(|| {
            let mut buf = [0u8; 5];

            let _ = instance.pack_to_slice(black_box(&mut buf));
        })
    });
}

criterion_group!(
    structs,
    normal_types,
    normal_types_repr_packed,
    packed_and_normal,
    packed
);
criterion_main!(structs);
