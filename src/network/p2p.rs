use anyhow::{anyhow, Result};
use rand::{thread_rng, RngCore};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use uuid::Uuid;

/// Public endpoint information
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub ip: IpAddr,
    pub port: u16,
}

/// Connection state for P2P networking
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// Peer information
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub endpoint: Endpoint,
    pub public_key: [u8; 32],
    pub last_seen: Instant,
}

/// STUN packet handling for public IP discovery
pub async fn discover_public_endpoint() -> Result<Endpoint> {
    // Create UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    // Use multiple STUN servers for reliability
    let stun_servers = [
        "stun1.l.google.com:19302",
        "stun2.l.google.com:19302",
        "stun.stunprotocol.org:3478",
    ];

    for server in stun_servers {
        // Create STUN binding request
        let request = create_stun_binding_request();

        // Send to STUN server
        socket.send_to(&request, server).await?;

        // Receive response with timeout
        let mut buf = [0u8; 512];
        match tokio::time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await {
            Ok(Ok((size, _))) => {
                if let Some((ip, port)) = parse_stun_response(&buf[..size]) {
                    return Ok(Endpoint { ip, port });
                }
            }
            _ => continue,
        }
    }

    Err(anyhow!("Failed to discover public endpoint"))
}

/// Create a STUN binding request packet
fn create_stun_binding_request() -> Vec<u8> {
    let mut request = vec![
        0x00, 0x01, // Message Type: Binding Request
        0x00, 0x00, // Message Length: 0 (no attributes)
        0x21, 0x12, 0xA4, 0x42, // Magic Cookie
    ];

    // Transaction ID (12 bytes)
    let mut transaction_id = [0u8; 12];
    thread_rng().fill_bytes(&mut transaction_id);
    request.extend_from_slice(&transaction_id);

    request
}

/// Parse a STUN response to extract mapped address
fn parse_stun_response(data: &[u8]) -> Option<(IpAddr, u16)> {
    // Minimum packet size check
    if data.len() < 20 {
        return None;
    }

    // Check if it's a STUN response
    if data[0] != 0x01 || data[1] != 0x01 {
        return None;
    }

    // Extract length
    let length = ((data[2] as usize) << 8) | (data[3] as usize);

    // Make sure we have enough data
    if data.len() < 20 + length {
        return None;
    }

    // STUN attributes start after 20-byte header
    let mut pos = 20;
    while pos + 4 <= data.len() {
        let attr_type = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        let attr_length = ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize);

        pos += 4;

        // Check if we have the XOR-MAPPED-ADDRESS attribute (0x0020)
        if attr_type == 0x0020 && pos + attr_length <= data.len() {
            // Skip first byte (reserved) and get family
            let family = data[pos + 1];

            // Get port (XOR with magic cookie first 2 bytes)
            let xor_port = ((data[pos + 2] as u16) << 8) | (data[pos + 3] as u16);
            let port = xor_port ^ 0x2112;

            if family == 0x01 {
                // IPv4
                let xor_ip = [
                    data[pos + 4] ^ 0x21,
                    data[pos + 5] ^ 0x12,
                    data[pos + 6] ^ 0xA4,
                    data[pos + 7] ^ 0x42,
                ];

                return Some((IpAddr::from(xor_ip), port));
            } else if family == 0x02 && pos + 8 + 16 <= data.len() {
                // IPv6 (XOR with magic cookie + transaction ID)
                let mut xor_ip = [0u8; 16];

                // XOR with magic cookie + transaction ID
                xor_ip[0] = data[pos + 4] ^ 0x21;
                xor_ip[1] = data[pos + 5] ^ 0x12;
                xor_ip[2] = data[pos + 6] ^ 0xA4;
                xor_ip[3] = data[pos + 7] ^ 0x42;

                // XOR the rest with transaction ID
                for i in 4..16 {
                    xor_ip[i] = data[pos + 4 + i] ^ data[4 + i];
                }

                return Some((IpAddr::from(xor_ip), port));
            }
        }

        pos += attr_length;
        // Attributes are padded to 4 bytes
        if attr_length % 4 != 0 {
            pos += 4 - (attr_length % 4);
        }
    }

    None
}

/// Establish a direct UDP connection to a remote peer
pub async fn establish_direct_udp_connection(
    remote_ip: IpAddr,
    remote_port: u16,
) -> Result<UdpSocket> {
    // Bind local UDP socket to random port
    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    // Send initial packet for NAT hole punching
    let hello_packet = [1, 2, 3, 4]; // Simple packet pattern

    // Send multiple packets for better hole punching success
    for _ in 0..5 {
        socket
            .send_to(&hello_packet, (remote_ip, remote_port))
            .await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(socket)
}

/// Format a connection link for sharing
pub fn generate_connection_link(
    endpoint: &Endpoint,
    session_id: &str,
    public_key: &[u8; 32],
) -> String {
    // Convert public key to base64 for URL safety
    let key_b64 = base64::encode(public_key);

    // Format as resonance:// link
    format!(
        "resonance://join?ip={}&port={}&sid={}&key={}",
        endpoint.ip, endpoint.port, session_id, key_b64
    )
}

/// Parse a connection link into components
pub fn parse_connection_link(link: &str) -> Result<(IpAddr, u16, String, [u8; 32])> {
    // Basic prefix check
    if !link.starts_with("resonance://join?") {
        return Err(anyhow!("Invalid connection link format"));
    }

    // Extract parameters
    let params_part = &link["resonance://join?".len()..];
    let params: HashMap<String, String> = params_part
        .split('&')
        .filter_map(|kv| {
            let parts: Vec<&str> = kv.splitn(2, '=').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect();

    // Extract required parameters
    let ip = params
        .get("ip")
        .ok_or_else(|| anyhow!("Missing IP address"))?
        .parse()?;

    let port = params
        .get("port")
        .ok_or_else(|| anyhow!("Missing port"))?
        .parse()?;

    let session_id = params
        .get("sid")
        .ok_or_else(|| anyhow!("Missing session ID"))?
        .to_string();

    let key_b64 = params
        .get("key")
        .ok_or_else(|| anyhow!("Missing public key"))?;

    // Decode public key
    let key_bytes = base64::decode(key_b64)?;

    if key_bytes.len() != 32 {
        return Err(anyhow!("Invalid public key length"));
    }

    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&key_bytes);

    Ok((ip, port, session_id, public_key))
}

/// Validates if an IP address should be allowed
pub fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            // Check for private/reserved ranges
            ipv4.is_private()
                || ipv4.is_loopback()
                || ipv4.is_link_local()
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_unspecified()
        }
        IpAddr::V6(ipv6) => ipv6.is_loopback() || ipv6.is_unspecified(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_link_format() {
        let endpoint = Endpoint {
            ip: "192.0.2.1".parse().unwrap(),
            port: 12345,
        };

        let session_id = "test-session-123";
        let public_key = [0u8; 32];

        let link = generate_connection_link(&endpoint, session_id, &public_key);

        assert!(link.starts_with("resonance://join?"));
        assert!(link.contains(&format!("ip={}", endpoint.ip)));
        assert!(link.contains(&format!("port={}", endpoint.port)));
        assert!(link.contains(&format!("sid={}", session_id)));
        assert!(link.contains("key="));
    }

    #[test]
    fn test_parse_connection_link() {
        let original_ip: IpAddr = "192.0.2.1".parse().unwrap();
        let original_port = 12345;
        let original_session_id = "test-session-123".to_string();
        let original_key = [42u8; 32];

        let endpoint = Endpoint {
            ip: original_ip,
            port: original_port,
        };

        let link = generate_connection_link(&endpoint, &original_session_id, &original_key);

        let (ip, port, session_id, key) = parse_connection_link(&link).unwrap();

        assert_eq!(ip, original_ip);
        assert_eq!(port, original_port);
        assert_eq!(session_id, original_session_id);
        assert_eq!(key, original_key);
    }

    #[test]
    fn test_blocked_ip_detection() {
        // Private IPs should be blocked
        assert!(is_blocked_ip(&"192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip(&"10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip(&"172.16.0.1".parse::<IpAddr>().unwrap()));

        // Loopback should be blocked
        assert!(is_blocked_ip(&"127.0.0.1".parse::<IpAddr>().unwrap()));

        // Public IPs should be allowed
        assert!(!is_blocked_ip(&"8.8.8.8".parse::<IpAddr>().unwrap()));
        assert!(!is_blocked_ip(&"8.8.4.4".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_stun_binding_request_format() {
        let request = create_stun_binding_request();

        // Verify STUN message header
        assert_eq!(request[0], 0x00); // Message Type: Binding Request
        assert_eq!(request[1], 0x01);
        assert_eq!(request[2], 0x00); // Message Length: 0
        assert_eq!(request[3], 0x00);
        assert_eq!(request[4], 0x21); // Magic Cookie
        assert_eq!(request[5], 0x12);
        assert_eq!(request[6], 0xA4);
        assert_eq!(request[7], 0x42);

        // Check that we have a transaction ID (12 bytes)
        assert_eq!(request.len(), 20);
    }
}
