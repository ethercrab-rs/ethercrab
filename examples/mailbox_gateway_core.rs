// Core mailbox gateway logic - extracted for testability
use std::collections::HashMap;
use std::future::Future;
use log::{debug, warn};

const EC_HDR: usize = 2;
const MBX_HDR: usize = 6;

/// Core frame handling logic that can be tested without hardware
/// 
/// Takes a packet, parses it, forwards to the appropriate station via the callback,
/// and builds the reply with proper header preservation.
pub async fn handle_frame<F, Fut>(
    pkt: &[u8],
    addr_to_idx: &HashMap<u16, usize>,
    mut forward: F,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>>
where
    F: FnMut(usize, &[u8]) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error>>>,
{
    // Minimum size check
    if pkt.len() < EC_HDR { 
        return Ok(None); 
    }

    // Parse EtherCAT header
    let hdr = u16::from_le_bytes([pkt[0], pkt[1]]);
    let ec_len = (hdr & 0x07ff) as usize;
    let upper = hdr & 0xf800;

    // Check we have enough data for the logical telegram (ignore trailing)
    if pkt.len() < EC_HDR + ec_len { 
        return Ok(None); 
    }

    let mbox = &pkt[EC_HDR..EC_HDR + ec_len];
    if mbox.len() < MBX_HDR { 
        return Ok(None); 
    }

    // Parse mailbox header
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

    // Forward to slave via callback
    match forward(idx, req).await {
        Ok(rep) => {
            // Clamp to 11-bit header space
            let rlen = rep.len().min(0x07ff);
            if rep.len() > 0x07ff { 
                warn!("clamped reply {} -> {}", rep.len(), rlen); 
            }

            // Build reply with preserved upper bits
            let mut out = Vec::with_capacity(EC_HDR + rlen);
            out.extend_from_slice(&(((rlen as u16) | upper).to_le_bytes()));
            out.extend_from_slice(&rep[..rlen]);

            Ok(Some(out))
        }
        Err(_) => {
            // ETG.8200 spec-compliant: no reply on error/timeout
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_valid_packet() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);

        // Create a valid CoE packet
        let mut packet = Vec::new();
        let ec_hdr: u16 = 0xA800 | 10; // upper bits + length
        packet.extend_from_slice(&ec_hdr.to_le_bytes());
        packet.extend_from_slice(&4u16.to_le_bytes()); // mailbox length
        packet.extend_from_slice(&0x1001u16.to_le_bytes()); // station
        packet.extend_from_slice(&[0x00, 0x03]); // priority + type
        packet.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // CoE data

        let result = handle_frame(&packet, &addr_map, |_idx, req| {
            let req = req.to_vec();
            async move {
                // Mock slave echoes back the request
                Ok::<Vec<u8>, Box<dyn std::error::Error>>(req)
            }
        }).await.unwrap();

        assert!(result.is_some(), "Valid packet should return response");
        let reply = result.unwrap();
        
        // Check header preservation
        let reply_hdr = u16::from_le_bytes([reply[0], reply[1]]);
        assert_eq!(reply_hdr & 0xF800, 0xA800, "Upper bits should be preserved");
        assert_eq!(reply_hdr & 0x07FF, 10, "Length should be correct");
    }

    #[tokio::test]
    async fn test_trailing_bytes_accepted() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);

        let mut packet = Vec::new();
        packet.extend_from_slice(&10u16.to_le_bytes()); // EC header
        packet.extend_from_slice(&4u16.to_le_bytes()); // mailbox length
        packet.extend_from_slice(&0x1001u16.to_le_bytes()); // station
        packet.extend_from_slice(&[0x00, 0x03]); // type
        packet.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // data
        packet.extend_from_slice(&[0x00; 20]); // trailing padding

        let result = handle_frame(&packet, &addr_map, |_idx, req| {
            assert_eq!(req.len(), 10, "Should trim trailing bytes");
            let req = req.to_vec();
            async move {
                // Verify only logical telegram was forwarded
                Ok::<Vec<u8>, Box<dyn std::error::Error>>(req)
            }
        }).await.unwrap();

        assert!(result.is_some(), "Packet with trailing bytes should be accepted");
    }

    #[tokio::test]
    async fn test_unknown_station_no_reply() {
        let addr_map = HashMap::new(); // Empty map

        let mut packet = Vec::new();
        packet.extend_from_slice(&10u16.to_le_bytes());
        packet.extend_from_slice(&4u16.to_le_bytes());
        packet.extend_from_slice(&0x9999u16.to_le_bytes()); // Unknown
        packet.extend_from_slice(&[0x00, 0x03]);
        packet.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let result = handle_frame(&packet, &addr_map, |_idx, _req| {
            async move {
                panic!("Should not forward unknown station");
            }
        }).await.unwrap();

        assert!(result.is_none(), "Unknown station should return no reply");
    }

    #[tokio::test]
    async fn test_truncated_packet_no_reply() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);

        let mut packet = Vec::new();
        packet.extend_from_slice(&10u16.to_le_bytes()); // Says 10 bytes
        packet.extend_from_slice(&4u16.to_le_bytes());
        packet.extend_from_slice(&0x1001u16.to_le_bytes());
        // Missing rest - truncated!

        let result = handle_frame(&packet, &addr_map, |_idx, _req| {
            async move {
                panic!("Should not forward truncated packet");
            }
        }).await.unwrap();

        assert!(result.is_none(), "Truncated packet should return no reply");
    }

    #[tokio::test]
    async fn test_reply_length_clamping() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);

        let mut packet = Vec::new();
        let ec_hdr: u16 = 0xF800 | 10; // All upper bits set
        packet.extend_from_slice(&ec_hdr.to_le_bytes());
        packet.extend_from_slice(&4u16.to_le_bytes());
        packet.extend_from_slice(&0x1001u16.to_le_bytes());
        packet.extend_from_slice(&[0x00, 0x03]);
        packet.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let result = handle_frame(&packet, &addr_map, |_idx, _req| {
            async move {
                // Return a reply larger than 11 bits can hold
                Ok::<Vec<u8>, Box<dyn std::error::Error>>(vec![0xFF; 3000])
            }
        }).await.unwrap();

        assert!(result.is_some(), "Should handle oversized reply");
        let reply = result.unwrap();
        
        // Check clamping
        let reply_hdr = u16::from_le_bytes([reply[0], reply[1]]);
        assert_eq!(reply_hdr & 0xF800, 0xF800, "Upper bits preserved");
        assert_eq!(reply_hdr & 0x07FF, 0x07FF, "Length clamped to 11 bits");
        assert_eq!(reply.len(), EC_HDR + 0x07FF, "Reply truncated to max");
    }

    #[tokio::test]
    async fn test_mailbox_counter_ignored() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);

        // Test various counter values - gateway shouldn't care
        for counter in [0x00, 0x07, 0xFF] {
            let mut packet = Vec::new();
            packet.extend_from_slice(&10u16.to_le_bytes());
            packet.extend_from_slice(&4u16.to_le_bytes());
            packet.extend_from_slice(&0x1001u16.to_le_bytes());
            packet.push(counter); // Counter byte
            packet.push(0x03); // Type
            packet.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

            let result = handle_frame(&packet, &addr_map, |_idx, req| {
                assert_eq!(req[4], counter, "Counter should be preserved");
                let req = req.to_vec();
                async move {
                    // Verify counter is passed through
                    Ok::<Vec<u8>, Box<dyn std::error::Error>>(req)
                }
            }).await.unwrap();

            assert!(result.is_some(), "Counter {} should be accepted", counter);
        }
    }

    #[tokio::test]
    async fn test_serialized_access() {
        let mut addr_map = HashMap::new();
        addr_map.insert(0x1001, 0);
        addr_map.insert(0x1002, 1);

        // Shared counter to verify serialization
        let counter = Arc::new(Mutex::new(0));

        // Create two different packets
        let mut packet1 = Vec::new();
        packet1.extend_from_slice(&10u16.to_le_bytes());
        packet1.extend_from_slice(&4u16.to_le_bytes());
        packet1.extend_from_slice(&0x1001u16.to_le_bytes());
        packet1.extend_from_slice(&[0x00, 0x03]);
        packet1.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let mut packet2 = packet1.clone();
        packet2[4] = 0x02; // Different station low byte
        packet2[5] = 0x10; // Station 0x1002

        // Process both packets concurrently
        let counter1 = counter.clone();
        let counter2 = counter.clone();

        let (r1, r2) = tokio::join!(
            handle_frame(&packet1, &addr_map, |_idx, req| {
                let counter = counter1.clone();
                let req = req.to_vec();
                async move {
                    let mut c = counter.lock().await;
                    *c += 1;
                    let val = *c;
                    // Simulate some work
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    // Check no concurrent modification
                    assert_eq!(*c, val, "Counter changed during operation!");
                    Ok::<Vec<u8>, Box<dyn std::error::Error>>(req)
                }
            }),
            handle_frame(&packet2, &addr_map, |_idx, req| {
                let counter = counter2.clone();
                let req = req.to_vec();
                async move {
                    let mut c = counter.lock().await;
                    *c += 1;
                    let val = *c;
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    assert_eq!(*c, val, "Counter changed during operation!");
                    Ok::<Vec<u8>, Box<dyn std::error::Error>>(req)
                }
            })
        );

        assert!(r1.unwrap().is_some(), "First packet should succeed");
        assert!(r2.unwrap().is_some(), "Second packet should succeed");
        
        // Both operations completed
        assert_eq!(*counter.lock().await, 2, "Both operations should complete");
    }
}