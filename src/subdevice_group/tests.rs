use super::*;
use crate::{
    MainDeviceConfig, PduStorage, Timeouts,
    ethernet::{EthernetAddress, EthernetFrame},
    pdu_loop::ReceivedFrame,
    subdevice_group::sync_unsafe_cell::MySyncUnsafeCell,
};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::{sync::Arc, thread};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn tx_rx_miri() {
    const MAX_SUBDEVICES: usize = 16;
    const MAX_PDU_DATA: usize = PduStorage::element_size(8);
    const MAX_FRAMES: usize = 128;
    const MAX_PDI: usize = 128;

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    crate::test_logger();

    let (mock_net_tx, mock_net_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

    let (mut tx, mut rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let stop = Arc::new(AtomicBool::new(false));

    let stop1 = stop.clone();

    let tx_handle = thread::spawn(move || {
        fmt::info!("Spawn TX task");

        while !stop1.load(Ordering::Relaxed) {
            while let Some(frame) = tx.next_sendable_frame() {
                fmt::info!("Sendable frame");

                frame
                    .send_blocking(|bytes| {
                        mock_net_tx.send(bytes.to_vec()).unwrap();

                        Ok(bytes.len())
                    })
                    .unwrap();

                thread::yield_now();
            }

            thread::sleep(Duration::from_millis(1));
        }
    });

    let stop1 = stop.clone();

    let rx_handle = thread::spawn(move || {
        fmt::info!("Spawn RX task");

        while let Ok(ethernet_frame) = mock_net_rx.recv() {
            fmt::info!("RX task received packet");

            // Let frame settle for a mo
            thread::sleep(Duration::from_millis(1));

            // Munge fake sent frame into a fake received frame
            let ethernet_frame = {
                let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                frame.into_inner()
            };

            while rx.receive_frame(&ethernet_frame).is_err() {}

            thread::yield_now();

            if stop1.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            pdu: Duration::from_secs(1),
            wait_loop_delay: Duration::ZERO,
            ..Timeouts::default()
        },
        MainDeviceConfig::default(),
    ));

    let group: SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, crate::DefaultLock, PreOpPdi, NoDc> =
        SubDeviceGroup {
            id: GroupId(0),
            pdi: RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
            read_pdi_len: 32,
            pdi_len: 96,
            inner: MySyncUnsafeCell::new(GroupInner {
                subdevices: heapless::Vec::new(),
                pdi_start: PdiOffset::default(),
            }),
            dc_conf: NoDc,
            _state: PhantomData,
        };

    let out = group.tx_rx(&maindevice).await;

    // No subdevices so no WKC, but success
    assert_eq!(
        out,
        Ok(TxRxResponse {
            working_counter: 0,
            subdevice_states: heapless::Vec::new(),
            extra: ()
        })
    );

    stop.store(true, Ordering::Relaxed);

    tx_handle.join().unwrap();
    rx_handle.join().unwrap();
}

#[test]
fn multi_state_checks_single_frame() {
    const MAX_FRAMES: usize = 1;
    const MAX_PDU_DATA: usize = PduStorage::element_size(AlControl::PACKED_LEN);
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    crate::test_logger();

    let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut frame = pdu_loop.alloc_frame().expect("No frame");

    assert!(
        frame.can_push_pdu_payload(AlControl::PACKED_LEN),
        "should be possible to push one status check PDU"
    );
    assert!(
        !frame.can_push_pdu_payload(AlControl::PACKED_LEN + 12),
        "test requires the frame to fit exactly one status check PDU"
    );

    let single_sd = vec![SubDevice {
        ..SubDevice::default()
    }];

    let subdevices = single_sd.iter();

    let (rest, num_pushed) =
        push_state_checks(subdevices, &mut frame).expect("Could not push status check");

    assert_eq!(rest.count(), 0);
    assert_eq!(num_pushed, single_sd.len());

    assert!(!frame.can_push_pdu_payload(1), "frame should be full");
}

#[test]
fn multi_state_checks_space_left_over() {
    // 1 byte left. AlControl takes 2 bytes.
    const SPACE_LEFT: usize = 1;

    const MAX_FRAMES: usize = 1;
    const MAX_PDU_DATA: usize = (AlControl::PACKED_LEN + CreatedFrame::PDU_OVERHEAD_BYTES) * 2
            + (SPACE_LEFT + CreatedFrame::PDU_OVERHEAD_BYTES)
            // Ethernet and EtherCAT frame headers
            + 16;
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    crate::test_logger();

    let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut frame = pdu_loop.alloc_frame().expect("No frame");

    let sds = vec![
        SubDevice {
            ..SubDevice::default()
        },
        SubDevice {
            ..SubDevice::default()
        },
        SubDevice {
            ..SubDevice::default()
        },
    ];

    let subdevices = sds.iter();

    let (rest, num_pushed) =
        push_state_checks(subdevices, &mut frame).expect("Could not push status check");

    assert_eq!(num_pushed, 2, "frame should hold two SD status checks");
    assert_eq!(rest.count(), 1, "frame can only hold two SD status checks");

    assert!(
        frame.can_push_pdu_payload(SPACE_LEFT),
        "frame has {} bytes available",
        SPACE_LEFT
    );
}

// This records the behaviour of a DC setup of the following 16 SubDevices:
//
// - EK1100
// - EL2828
// - EL2889
// - EL2004
// - EL1004
// - EL1018
// - EL1008
// - EL1004
// - EL2004
// - EL2008
// - EL1008
// - EL2008
// - EL2008
// - EL2522
// - EL1258
// - EL9505
#[test]
fn large_group_frame_split() {
    const MAX_SUBDEVICES: usize = 32;
    const MAX_PDU_DATA: usize = PduStorage::element_size(256);
    const MAX_FRAMES: usize = 32;
    const MAX_PDI: usize = 512;
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    crate::test_logger();

    let (mock_net_tx, mock_net_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

    let (mut tx, mut rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig::default(),
    ));

    let stop = Arc::new(AtomicBool::new(false));

    let stop1 = stop.clone();

    let tx_handle = thread::spawn(move || {
        fmt::info!("Spawn TX task");

        while !stop1.load(Ordering::Relaxed) {
            while let Some(frame) = tx.next_sendable_frame() {
                fmt::info!("Sendable frame");

                frame
                    .send_blocking(|bytes| {
                        mock_net_tx.send(bytes.to_vec()).unwrap();

                        Ok(bytes.len())
                    })
                    .unwrap();

                thread::yield_now();
            }
        }
    });

    let stop1 = stop.clone();

    let rx_handle = thread::spawn(move || {
        fmt::info!("Spawn RX task");

        while let Ok(ethernet_frame) = mock_net_rx.recv() {
            fmt::info!("RX task received packet");

            // Munge fake sent frame into a fake received frame
            let ethernet_frame = {
                let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                frame.into_inner()
            };

            while rx.receive_frame(&ethernet_frame).is_err() {}

            thread::yield_now();

            if stop1.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    fn sd(addr: u16) -> SubDevice {
        SubDevice {
            configured_address: addr,
            ..SubDevice::default()
        }
    }

    let subdevices = heapless::Vec::<_, MAX_SUBDEVICES>::from_slice(&[
        sd(0x1000),
        sd(0x1001),
        sd(0x1002),
        sd(0x1003),
        sd(0x1004),
        sd(0x1005),
        sd(0x1006),
        sd(0x1007),
        sd(0x1008),
        sd(0x1009),
        sd(0x100a),
        sd(0x100b),
        sd(0x100c),
        sd(0x100d),
        sd(0x100e),
        sd(0x100f),
    ])
    .unwrap();

    // Test setup had 16 devices
    assert_eq!(subdevices.len(), 16);

    let group = SubDeviceGroup::<MAX_SUBDEVICES, MAX_PDI, crate::DefaultLock, Op, HasDc> {
        id: GroupId(0),
        pdi: RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
        read_pdi_len: 406,
        pdi_len: 474,
        inner: MySyncUnsafeCell::new(GroupInner {
            subdevices,
            pdi_start: PdiOffset { start_address: 0 },
        }),
        dc_conf: HasDc {
            sync0_period: 100_000,
            sync0_shift: 0,
            reference: 0,
        },
        _state: PhantomData::<Op>,
    };

    cassette::block_on(group.tx_rx_dc(&maindevice)).unwrap();

    stop.store(true, Ordering::Relaxed);

    tx_handle.join().unwrap();
    rx_handle.join().unwrap();

    const PDI_FRAME_0: usize = 236;
    const PDI_FRAME_1: usize = 238;

    assert_eq!(PDI_FRAME_0 + PDI_FRAME_1, 474);

    // Expected PDU lengths for each frame
    let expected_pdus = [
        [
            8,           // DC FRMW
            PDI_FRAME_0, // Consume rest of frame with PDI
        ]
        .as_slice(),
        &[
            PDI_FRAME_1, // Entire frame filled with PDI
            2,           // First status check
        ],
        // 15 remaining SubDevice status checks
        &[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    ];

    // We should have sent 3 frames in this test
    for (i, expected_lens) in expected_pdus.iter().enumerate() {
        let f = maindevice
            .pdu_loop
            .test_only_storage_ref()
            .frame_at_index(i);

        let idx = AtomicU8::new(i as u8);

        let b = ReceivedFrame::from_frame_element_for_test_only(f, &idx, MAX_PDU_DATA);

        let expected_pdu_count = expected_lens.len();
        let mut actual_pdu_count = 0;

        for (pdu_idx, pdu) in b.into_pdu_iter().enumerate() {
            let pdu = pdu.unwrap();

            actual_pdu_count += 1;

            assert_eq!(
                pdu.len(),
                expected_lens[pdu_idx],
                "frame {}, PDU {} length",
                i,
                pdu_idx
            );
        }

        assert_eq!(
            actual_pdu_count, expected_pdu_count,
            "frame {} PDU count",
            i
        );
    }

    let f = maindevice
        .pdu_loop
        .test_only_storage_ref()
        .frame_at_index(3);
    let idx = AtomicU8::new(3);
    let b = ReceivedFrame::from_frame_element_for_test_only(f, &idx, MAX_PDU_DATA);

    // 4th frame should be empty as we only sent 3
    assert_eq!(b.into_pdu_iter().count(), 0);
}
