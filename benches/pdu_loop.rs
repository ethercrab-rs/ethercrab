use core::future::poll_fn;
use core::task::Poll;
use criterion::{Bencher, Criterion, Throughput, criterion_group, criterion_main};
use ethercrab::{Command, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::{pin::pin, time::Duration};

const DATA: [u8; 8] = [0x11u8, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd];

fn do_bench(b: &mut Bencher) {
    const FRAME_OVERHEAD: usize = 28;

    // 1 frame, up to 128 bytes payload
    let storage = PduStorage::<1, { PduStorage::element_size(128) }>::new();

    let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts {
            pdu: Duration::from_millis(1000),
            ..Timeouts::default()
        },
        MainDeviceConfig::default(),
    );

    let mut written_packet = [0u8; { FRAME_OVERHEAD + DATA.len() }];

    b.iter(|| {
        //  --- Prepare frame

        let mut frame_fut =
            pin!(Command::fpwr(0x5678, 0x1234).send_receive::<()>(&maindevice, &DATA));

        // Poll future once to register it with sender
        cassette::block_on(poll_fn(|ctx| {
            let _ = frame_fut.as_mut().poll(ctx);

            Poll::Ready(())
        }));

        let frame = tx.next_sendable_frame().expect("Next frame");

        frame
            .send_blocking(|bytes| {
                written_packet.copy_from_slice(bytes);

                Ok(bytes.len())
            })
            .expect("TX");

        // --- Receive frame

        // Turn master sent MAC into receiving MAC
        written_packet[6] = 0x12;
        // Bump working counter so we don't error out
        written_packet[written_packet.len() - 2] = 1;

        rx.receive_frame(&written_packet).expect("RX");

        let _ = cassette::block_on(frame_fut);
    })
}

pub fn tx_rx(c: &mut Criterion) {
    let mut group = c.benchmark_group("pdu_loop");

    group.throughput(Throughput::Elements(1));

    group.bench_function("elements", do_bench);

    let overhead = {
        // Ethernet header
        (6 + 6 + 2) +
        // EtherCAT header
        2 +
        // PDU header
        10 +
        // Working counter
        2
    };

    group.throughput(Throughput::Bytes(DATA.len() as u64 + overhead));

    group.bench_function("ethernet frame bytes", do_bench);

    group.finish();
}

criterion_group!(pdu_loop, tx_rx);
criterion_main!(pdu_loop);
