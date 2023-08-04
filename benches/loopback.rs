#[cfg(target_os = "linux")]
const INTERFACE: &str = "lo";

#[cfg(target_os = "macos")]
const INTERFACE: &str = "lo0";

#[cfg(not(unix))]
fn main() {}

/// Due to requiring special permissions for access to raw sockets, this must be run with `just
/// linux-bench loopback`.
#[cfg(target_family = "unix")]
fn main() {
    use criterion::{criterion_group, Bencher, Criterion, Throughput};
    use ethercrab::std::tx_rx_task;
    use ethercrab::PduStorage;
    use ethercrab::{Client, ClientConfig, Timeouts};
    use std::time::Duration;

    const DATA: [u8; 8] = [0x11u8, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd];

    fn do_bench(b: &mut Bencher, rt: &tokio::runtime::Runtime, client: &Client) {
        b.to_async(rt).iter(|| async {
            let _ = client.lwr(0x1234_5678, DATA).await.expect("write");
        })
    }

    pub fn loopback_tx_rx(c: &mut Criterion) {
        const ETHERCAT_FRAME_HEADER_LEN: u64 = 28;

        static STORAGE: PduStorage<16, 128> = PduStorage::new();

        let (pdu_tx, pdu_rx, pdu_loop) = STORAGE.try_split().expect("already split");

        let client = Client::new(
            pdu_loop,
            Timeouts {
                wait_loop_delay: Duration::from_millis(2),
                mailbox_response: Duration::from_millis(1000),
                pdu: Duration::from_millis(1000),
                ..Default::default()
            },
            ClientConfig::default(),
        );

        let rt = tokio::runtime::Runtime::new().expect("runtime");

        rt.spawn(tx_rx_task(INTERFACE, pdu_tx, pdu_rx).expect("spawn tx/rx"));

        let mut group = c.benchmark_group("loopback");

        group.throughput(Throughput::Elements(1));

        group.bench_function("elements", |b| do_bench(b, &rt, &client));

        group.throughput(Throughput::Bytes(
            ETHERCAT_FRAME_HEADER_LEN + DATA.len() as u64,
        ));

        group.bench_function("bytes", |b| do_bench(b, &rt, &client));

        group.finish();
    }

    criterion_group!(loopback, loopback_tx_rx);

    loopback();

    criterion::Criterion::default()
        .configure_from_args()
        .final_summary();
}
