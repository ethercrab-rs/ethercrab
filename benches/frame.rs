use criterion::{criterion_group, criterion_main, Criterion};
use ethercrab::{Command, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use futures_lite::FutureExt;
use std::{future::poll_fn, pin::pin, task::Poll};

const DATA: [u8; 8] = [0x11u8, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd];

pub fn push_pdu(c: &mut Criterion) {
    // 1 frame, up to 128 bytes payload
    let storage = PduStorage::<1, { PduStorage::element_size(128) }>::new();

    let (_tx, _rx, pdu_loop) = storage.try_split().unwrap();

    let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

    c.bench_function("frame push pdu", |b| {
        b.iter(|| {
            let mut f = pin!(Command::fpwr(0x5678, 0x1234).send_receive_slice(&maindevice, &DATA));

            cassette::block_on(poll_fn(|ctx| {
                let _ = f.poll(ctx);

                Poll::Ready(())
            }));
        })
    });
}

criterion_group!(frame, push_pdu);
criterion_main!(frame);
