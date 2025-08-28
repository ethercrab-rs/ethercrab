// examples/mailbox_gateway.rs  (example for PR)

use ethercrab::{
    MainDevice, MainDeviceConfig, PduStorage, SubDeviceGroup, Timeouts,
    std::{ethercat_now, tx_rx_task},
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::net::UdpSocket;
#[allow(unused_imports)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use log::{info, warn, debug, error};

const GATEWAY_PORT: u16 = 0x88A4;
const MAX_PACKET_SIZE: usize = 1500;
const EC_HDR: usize = 2;
const MBX_HDR: usize = 6;

// Size big enough for mailbox payloads
const MAX_FRAMES: usize = 32;
const MAX_PDU_DATA: usize = PduStorage::element_size(1500);
static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

/// Serialize mailbox transactions across all slaves
static MAILBOX_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let iface = std::env::args().nth(1).expect("usage: mailbox_gateway <iface>");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("pdu split once");
    let maindevice = Arc::new(MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default()));

    let mut net_task = tokio::spawn(async move { 
        let task = tx_rx_task(&iface, tx, rx).expect("spawn TX/RX task");
        task.await
    });

    // Discover slaves and form one PRE‑OP group (no PDI/DC required)
    let mut group = maindevice
        .init_single_group::<128, 1>(ethercat_now) // PDI len 1 as we don’t use it
        .await?;

    info!("Found {} slaves", group.len());

    // Build quick address->index map
    let mut addr_to_idx = HashMap::<u16, usize>::new();
    for (idx, sd) in group.iter(&maindevice).enumerate() {
        addr_to_idx.insert(sd.configured_address(), idx);
        debug!("  {:#06x}", sd.configured_address());
    }

    // Setup UDP socket
    let udp_socket = Arc::new(UdpSocket::bind(("0.0.0.0", GATEWAY_PORT)).await?);
    info!("Mailbox Gateway listening on UDP {}", GATEWAY_PORT);

    // Note: TCP support per ETG.8200 spec would require refactoring to share SubDeviceGroup
    // across connections. For production use, consider using Arc<Mutex<SubDeviceGroup>> or
    // a channel-based architecture.
    
    // Shared state
    let maindevice = Arc::new(maindevice);
    let addr_to_idx = Arc::new(addr_to_idx);

    let mut buf = [0u8; MAX_PACKET_SIZE];

    loop {
        tokio::select! {
            // Handle UDP packets
            r = udp_socket.recv_from(&mut buf) => {
                match r {
                    Ok((len, src)) => {
                        // Process synchronously to avoid cloning buffer issues
                        if let Err(e) = process_udp_packet(&udp_socket, &maindevice, &mut group, &addr_to_idx, &buf[..len], src).await {
                            debug!("drop UDP packet: {}", e);
                        }
                    }
                    Err(e) => { error!("UDP recv error: {}", e); break; }
                }
            }
            // Monitor TX/RX task
            _ = &mut net_task => {
                error!("tx/rx task exited; shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn process_udp_packet<const MAX_SD: usize, const MAX_PDI: usize>(
    socket: &Arc<UdpSocket>,
    maindevice: &Arc<MainDevice<'_>>,
    group: &mut SubDeviceGroup<MAX_SD, MAX_PDI>,
    addr_to_idx: &Arc<HashMap<u16, usize>>,
    pkt: &[u8],
    src: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    match process_mailbox(maindevice, group, addr_to_idx, pkt).await {
        Ok(Some(reply)) => {
            socket.send_to(&reply, src).await?;
        }
        Ok(None) => {
            // No reply on error/timeout
        }
        Err(e) => {
            debug!("UDP packet processing error: {}", e);
        }
    }
    Ok(())
}

// TCP handler would be needed for full ETG.8200 compliance
#[allow(dead_code)]
async fn handle_tcp_client<const MAX_SD: usize, const MAX_PDI: usize>(
    mut stream: tokio::net::TcpStream,
    maindevice: Arc<MainDevice<'_>>,
    mut group: SubDeviceGroup<MAX_SD, MAX_PDI>,
    addr_to_idx: Arc<HashMap<u16, usize>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; MAX_PACKET_SIZE];
    
    loop {
        // Read EtherCAT header first (2 bytes)
        let _n = match stream.read(&mut buf[..EC_HDR]).await {
            Ok(0) => break, // Connection closed
            Ok(n) if n < EC_HDR => break, // Incomplete header
            Ok(n) => n,
            Err(_) => break,
        };
        
        // Parse header to get frame length
        let hdr = u16::from_le_bytes([buf[0], buf[1]]);
        let ec_len = (hdr & 0x07ff) as usize;
        
        if ec_len > MAX_PACKET_SIZE - EC_HDR {
            warn!("TCP frame too large: {} bytes", ec_len);
            break;
        }
        
        // Read the rest of the frame
        let mut total = 0;
        while total < ec_len {
            match stream.read(&mut buf[EC_HDR + total..EC_HDR + ec_len]).await {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(_) => break,
            }
        }
        
        if total < ec_len {
            break; // Incomplete frame
        }
        
        // Process the complete frame
        match process_mailbox(&maindevice, &mut group, &addr_to_idx, &buf[..EC_HDR + ec_len]).await {
            Ok(Some(reply)) => {
                if let Err(_) = stream.write_all(&reply).await {
                    break;
                }
            }
            Ok(None) => {
                // No reply on error/timeout
            }
            Err(e) => {
                debug!("TCP packet processing error: {}", e);
            }
        }
    }
    
    info!("TCP client disconnected");
    Ok(())
}

async fn process_mailbox<const MAX_SD: usize, const MAX_PDI: usize>(
    maindevice: &Arc<MainDevice<'_>>,
    group: &mut SubDeviceGroup<MAX_SD, MAX_PDI>,
    addr_to_idx: &Arc<HashMap<u16, usize>>,
    pkt: &[u8],
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    if pkt.len() < EC_HDR { return Ok(None); }

    // EtherCAT header
    let hdr = u16::from_le_bytes([pkt[0], pkt[1]]);
    let ec_len = (hdr & 0x07ff) as usize;
    let upper = hdr & 0xf800;

    if pkt.len() < EC_HDR + ec_len { return Ok(None); }

    let mbox = &pkt[EC_HDR..EC_HDR + ec_len];
    if mbox.len() < MBX_HDR { return Ok(None); }

    // Mailbox header
    let mlen = u16::from_le_bytes([mbox[0], mbox[1]]) as usize;
    let station = u16::from_le_bytes([mbox[2], mbox[3]]);

    // Check for truncated mailbox (but accept trailing bytes like IgH)
    if MBX_HDR + mlen > mbox.len() {
        debug!("Drop: truncated mailbox (have {}, need {})", 
               mbox.len(), MBX_HDR + mlen);
        return Ok(None);
    }
    
    // Trim to logical telegram size (ignore trailing bytes)
    let req = &mbox[..MBX_HDR + mlen];

    // Resolve station
    let Some(&idx) = addr_to_idx.get(&station) else { 
        debug!("Drop: unknown station {:#06x}", station);
        return Ok(None);
    };

    let _guard = MAILBOX_LOCK.lock().await;

    // Borrow fresh SubDeviceRef for this index and forward
    let sub = group.subdevice(maindevice, idx)?;
    match tokio::time::timeout(Duration::from_secs(10), sub.mailbox_raw_roundtrip(req, Duration::from_secs(10))).await {
        Ok(Ok(rep)) => {
            // Clamp to 11-bit header space
            let rlen = rep.len().min(0x07ff);
            if rep.len() > 0x07ff { warn!("clamped reply {} -> {}", rep.len(), rlen); }

            let mut out = Vec::with_capacity(EC_HDR + rlen);
            out.extend_from_slice(&(((rlen as u16) | upper).to_le_bytes()));
            out.extend_from_slice(&rep[..rlen]);

            Ok(Some(out))
        }
        _ => {
            // spec-like behavior: no reply on error/timeout
            Ok(None)
        }
    }
}
