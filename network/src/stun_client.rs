use log::{debug, error, info, warn};
use rand::{seq::SliceRandom, thread_rng, Rng};
use room_core::Error;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use tokio::net::UdpSocket;
use tokio::time::{self, Duration};

/// Timeout for STUN requests in seconds
const STUN_TIMEOUT: u64 = 5;

/// Structure representing a STUN server
#[derive(Debug, Clone)]
pub struct StunServer {
    /// Server hostname and port (e.g., "stun.l.google.com:19302")
    pub url: String,
}

impl FromStr for StunServer {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("stun:") {
            Ok(StunServer {
                url: s.trim_start_matches("stun:").to_string(),
            })
        } else {
            Err(Error::Network(format!("Invalid STUN server URL: {}", s)))
        }
    }
}

/// STUN client for resolving public IP address
pub struct StunClient {
    /// List of STUN servers to try
    servers: Vec<StunServer>,
}

impl StunClient {
    /// Create a new STUN client with the given servers
    pub fn new(server_urls: Vec<String>) -> Self {
        let mut servers = Vec::new();
        for url in server_urls {
            if let Ok(server) = StunServer::from_str(&url) {
                servers.push(server);
            } else {
                warn!("Invalid STUN server URL: {}", url);
            }
        }

        if servers.is_empty() {
            warn!("No valid STUN servers provided, using default servers");
            servers = vec![
                StunServer::from_str("stun:stun.l.google.com:19302").unwrap(),
                StunServer::from_str("stun:stun1.l.google.com:19302").unwrap(),
            ];
        }

        Self { servers }
    }

    /// Resolve public IP address using STUN servers
    pub async fn resolve_public_ip(&self) -> Result<SocketAddr, Error> {
        // Randomize the server list
        let mut rng = thread_rng();
        let mut servers = self.servers.clone();
        servers.shuffle(&mut rng);

        // Try each server until one works
        for server in &servers {
            match self.query_stun_server(server).await {
                Ok(addr) => return Ok(addr),
                Err(e) => {
                    debug!(
                        "Failed to resolve IP with STUN server {}: {}",
                        server.url, e
                    );
                    continue;
                }
            }
        }

        Err(Error::Network(
            "Failed to resolve public IP with any STUN server".to_string(),
        ))
    }

    /// Query a specific STUN server to resolve public IP
    async fn query_stun_server(&self, server: &StunServer) -> Result<SocketAddr, Error> {
        // Resolve STUN server hostname
        let server_addr = tokio::net::lookup_host(&server.url)
            .await
            .map_err(|e| {
                Error::Network(format!(
                    "Failed to resolve STUN server {}: {}",
                    server.url, e
                ))
            })?
            .next()
            .ok_or_else(|| {
                Error::Network(format!("No addresses found for STUN server {}", server.url))
            })?;

        // Bind a local UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| Error::Network(format!("Failed to bind UDP socket: {}", e)))?;

        // Generate a random transaction ID
        let mut transaction_id = [0u8; 12];
        thread_rng().fill(&mut transaction_id);

        // Create STUN binding request
        let mut request = vec![
            0x00, 0x01, // Message Type: Binding Request
            0x00, 0x00, // Message Length: 0 bytes (no attributes)
            0x21, 0x12, 0xA4, 0x42, // Magic Cookie
        ];
        request.extend_from_slice(&transaction_id); // Transaction ID

        // Send the request to the STUN server
        socket
            .send_to(&request, server_addr)
            .await
            .map_err(|e| Error::Network(format!("Failed to send STUN request: {}", e)))?;

        // Receive the response with timeout
        let mut buf = vec![0u8; 512];
        let timeout = time::timeout(
            Duration::from_secs(STUN_TIMEOUT),
            socket.recv_from(&mut buf),
        )
        .await;

        match timeout {
            Ok(Ok((len, _))) => {
                let response = &buf[0..len];

                // Parse the STUN response to extract the mapped address
                if response.len() >= 20 && response[0] == 0x01 && response[1] == 0x01 {
                    // Look for XOR-MAPPED-ADDRESS attribute (type 0x0020)
                    let mut i = 20;
                    while i + 8 <= response.len() {
                        let attr_type = ((response[i] as u16) << 8) | (response[i + 1] as u16);
                        let attr_len = ((response[i + 2] as u16) << 8) | (response[i + 3] as u16);

                        if attr_type == 0x0020 && attr_len >= 8 {
                            // XOR-MAPPED-ADDRESS attribute found
                            let family = response[i + 5];

                            if family == 0x01 {
                                // IPv4 address
                                let xport = ((response[i + 6] as u16) << 8
                                    | (response[i + 7] as u16))
                                    ^ 0x2112;

                                let xip = ((response[i + 8] as u32) << 24)
                                    | ((response[i + 9] as u32) << 16)
                                    | ((response[i + 10] as u32) << 8)
                                    | (response[i + 11] as u32);

                                let ip = xip ^ 0x2112A442;

                                let ipaddr = IpAddr::from([
                                    ((ip >> 24) & 0xFF) as u8,
                                    ((ip >> 16) & 0xFF) as u8,
                                    ((ip >> 8) & 0xFF) as u8,
                                    (ip & 0xFF) as u8,
                                ]);

                                return Ok(SocketAddr::new(ipaddr, xport));
                            }
                        }

                        i += 4 + attr_len as usize;
                        // Attributes are padded to 4-byte boundaries
                        if attr_len % 4 != 0 {
                            i += 4 - (attr_len % 4) as usize;
                        }
                    }
                }

                Err(Error::Network("Failed to parse STUN response".to_string()))
            }
            Ok(Err(e)) => Err(Error::Network(format!(
                "Failed to receive STUN response: {}",
                e
            ))),
            Err(_) => Err(Error::Network(format!(
                "STUN request to {} timed out",
                server.url
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stun_server_parsing() {
        let server = StunServer::from_str("stun:stun.l.google.com:19302").unwrap();
        assert_eq!(server.url, "stun.l.google.com:19302");

        let result = StunServer::from_str("invalid:example.com");
        assert!(result.is_err());
    }

    // Note: This test requires network connectivity and may fail in some environments
    #[tokio::test]
    #[ignore]
    async fn test_resolve_public_ip() {
        let client = StunClient::new(vec!["stun:stun.l.google.com:19302".to_string()]);
        let result = client.resolve_public_ip().await;
        assert!(
            result.is_ok(),
            "Failed to resolve public IP: {:?}",
            result.err()
        );

        let addr = result.unwrap();
        println!("Resolved public IP: {}", addr);

        // Verify we got a non-local IP
        assert!(!addr.ip().is_loopback() && !addr.ip().is_unspecified());
    }
}
