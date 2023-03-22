//! Requires at least one EtherCAT device connected to `en2ps0`.

#[cfg(not(unix))]
fn main() {}

/// Due to requiring special permissions for access to raw sockets, this must be run with `just
/// linux-bench loopback`.
#[cfg(unix)]
fn main() {
    use criterion::{criterion_group, Bencher, Criterion, Throughput};
    use ethercrab::std::tx_rx_task;
    use ethercrab::{Client, ClientConfig, Timeouts};
    use ethercrab::{PduStorage, RegisterAddress};
    use std::time::Duration;

    fn do_bench(b: &mut Bencher, rt: &tokio::runtime::Runtime, client: &Client) {
        b.to_async(rt).iter(|| async {
            let _: (u16, u16) = client.brd(RegisterAddress::Type).await.expect("write");
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
                ..Default::default()
            },
            ClientConfig::default(),
        );

        let rt = tokio::runtime::Runtime::new().expect("runtime");

        // TODO: Configurable interface
        rt.spawn(tx_rx_task("enp2s0", pdu_tx, pdu_rx).expect("spawn tx/rx"));

        let mut group = c.benchmark_group("loopback");

        group.throughput(Throughput::Elements(1));

        group.bench_function("elements", |b| do_bench(b, &rt, &client));

        group.throughput(Throughput::Bytes(ETHERCAT_FRAME_HEADER_LEN));

        group.bench_function("bytes", |b| do_bench(b, &rt, &client));

        group.finish();
    }

    criterion_group!(loopback, loopback_tx_rx);

    loopback();

    criterion::Criterion::default()
        .configure_from_args()
        .final_summary();
}
