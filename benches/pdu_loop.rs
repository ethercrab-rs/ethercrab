use core::task::Poll;
use criterion::{criterion_group, criterion_main, Bencher, Criterion, Throughput};
use ethercrab::{internals::Command, PduStorage};

const DATA: [u8; 8] = [0x11u8, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd];

fn do_bench(b: &mut Bencher) {
    const FRAME_OVERHEAD: usize = 28;

    // 1 frame, up to 128 bytes payload
    let storage = PduStorage::<1, 128>::new();

    let (tx, rx, pdu_loop) = storage.try_split().unwrap();

    let mut packet_buf = [0u8; 1536];
    let mut written_packet = Vec::new();
    written_packet.resize(FRAME_OVERHEAD + DATA.len(), 0);

    b.iter(|| {
        //  --- Prepare frame

        let frame_fut = futures_lite::future::poll_once(pdu_loop.pdu_tx_readwrite(
            Command::Fpwr {
                address: 0x5678,
                register: 0x1234,
            },
            &DATA,
        ));

        let _ = futures_lite::future::block_on(frame_fut);

        // --- Send frame

        let send_fut = futures_lite::future::poll_once(core::future::poll_fn::<(), _>(|ctx| {
            tx.send_frames_blocking(ctx.waker(), &mut packet_buf, |frame| {
                written_packet.copy_from_slice(frame);

                Ok(())
            })
            .unwrap();

            Poll::Ready(())
        }));

        let _ = futures_lite::future::block_on(send_fut);

        assert_eq!(written_packet.len(), FRAME_OVERHEAD + DATA.len());

        // --- Receive frame

        let _ = rx.receive_frame(&written_packet);
    })
}

pub fn tx_rx(c: &mut Criterion) {
    let mut group = c.benchmark_group("pdu-loop");

    group.throughput(Throughput::Elements(1));

    group.bench_function("elements", do_bench);

    group.throughput(Throughput::Bytes(DATA.len() as u64));

    group.bench_function("payload bytes", do_bench);

    group.finish();
}

criterion_group!(pdu_loop, tx_rx);
criterion_main!(pdu_loop);
