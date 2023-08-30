use core::time::Duration;
use criterion::{criterion_group, criterion_main, Bencher, Criterion, Throughput};
use ethercrab::{Command, PduStorage, RetryBehaviour};

const DATA: [u8; 8] = [0x11u8, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd];

fn do_bench(b: &mut Bencher) {
    const FRAME_OVERHEAD: usize = 28;

    // 1 frame, up to 128 bytes payload
    let storage = PduStorage::<1, 128>::new();

    let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

    let mut packet_buf = [0u8; 1536];
    let mut written_packet = Vec::new();
    written_packet.resize(FRAME_OVERHEAD + DATA.len(), 0);

    b.iter(|| {
        //  --- Prepare frame

        let frame_fut = pdu_loop.pdu_tx_readwrite(
            Command::fpwr(0x5678, 0x1234),
            &DATA,
            Duration::from_secs(1),
            RetryBehaviour::None,
        );

        // --- Send frame

        let send_fut = async {
            let frame = tx.next_sendable_frame().unwrap();

            frame
                .send(&mut packet_buf, |bytes| async {
                    written_packet.copy_from_slice(bytes);

                    Ok(bytes.len())
                })
                .await
                .unwrap();
        };

        let _ =
            futures_lite::future::block_on(embassy_futures::select::select(frame_fut, send_fut));

        // --- Receive frame

        let _ = rx.receive_frame(&written_packet);
    })
}

pub fn tx_rx(c: &mut Criterion) {
    let mut group = c.benchmark_group("pdu_loop");

    group.throughput(Throughput::Elements(1));

    group.bench_function("elements", do_bench);

    group.throughput(Throughput::Bytes(DATA.len() as u64));

    group.bench_function("payload bytes", do_bench);

    group.finish();
}

criterion_group!(pdu_loop, tx_rx);
criterion_main!(pdu_loop);
