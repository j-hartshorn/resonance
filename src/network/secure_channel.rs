use anyhow::{anyhow, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305,
};
use rand::{thread_rng, RngCore};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use x25519_dalek::{PublicKey, StaticSecret};

use super::p2p::ConnectionState;

/// A key pair for asymmetric encryption
pub struct Keypair {
    pub secret: StaticSecret,
    pub public: PublicKey,
}

impl Keypair {
    /// Generate a new keypair
    pub fn generate() -> Self {
        let secret = StaticSecret::new(thread_rng());
        let public = PublicKey::from(&secret);

        Self { secret, public }
    }

    /// Perform Diffie-Hellman key exchange
    pub fn dh(&self, peer_public: &PublicKey) -> [u8; 32] {
        let shared_secret = self.secret.diffie_hellman(peer_public);
        *shared_secret.as_bytes()
    }
}

/// Session message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Initial handshake
    Handshake {
        public_key: [u8; 32],
        session_id: String,
    },
    /// Joining a session
    Join { name: String, public_key: [u8; 32] },
    /// Audio data
    Audio { data: Vec<u8>, timestamp: u64 },
    /// Position update
    Position { x: f32, y: f32, z: f32 },
    /// Heartbeat to keep connection alive
    Heartbeat,
    /// Error message
    Error { code: u32, message: String },
    /// List of peers in a session (from host to peers)
    PeerList {
        peers: Vec<crate::app::session::Peer>,
    },
    /// New peer joined the session (from host to peers)
    NewPeer { peer: crate::app::session::Peer },
    /// Peer left the session (from peer to all)
    PeerLeft { peer_id: String },
}

/// Rate limiting configuration
struct RateLimiter {
    /// Maximum number of messages per time window
    max_tokens: usize,
    /// Current token count
    tokens: usize,
    /// Last refill time
    last_refill: Instant,
    /// Refill rate (tokens per second)
    refill_rate: f64,
}

impl RateLimiter {
    fn new(max_tokens: usize, refill_rate: f64) -> Self {
        Self {
            max_tokens,
            tokens: max_tokens,
            last_refill: Instant::now(),
            refill_rate,
        }
    }

    fn consume(&mut self, count: usize) -> bool {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let new_tokens = (elapsed * self.refill_rate) as usize;

        if new_tokens > 0 {
            self.tokens = (self.tokens + new_tokens).min(self.max_tokens);
            self.last_refill = now;
        }

        // Check if we have enough tokens
        if self.tokens >= count {
            self.tokens -= count;
            true
        } else {
            false
        }
    }
}

/// A secure communication channel over UDP
pub struct SecureChannel {
    /// Underlying UDP socket
    socket: Arc<UdpSocket>,
    /// Remote endpoint
    remote: SocketAddr,
    /// Encryption key pair
    keypair: Keypair,
    /// Shared secret (after key exchange)
    shared_secret: Option<[u8; 32]>,
    /// Session ID
    pub session_id: String,
    /// Connection state
    state: ConnectionState,
    /// Rate limiter
    rate_limiter: Arc<Mutex<RateLimiter>>,
    /// Last heartbeat time
    last_heartbeat: Instant,
}

impl SecureChannel {
    /// Create a new secure channel
    pub async fn new(socket: UdpSocket, remote: SocketAddr) -> Self {
        let keypair = Keypair::generate();

        Self {
            socket: Arc::new(socket),
            remote,
            keypair,
            shared_secret: None,
            session_id: uuid::Uuid::new_v4().to_string(),
            state: ConnectionState::Disconnected,
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new(100, 50.0))),
            last_heartbeat: Instant::now(),
        }
    }

    /// Get the public key
    pub fn public_key(&self) -> [u8; 32] {
        self.keypair.public.to_bytes()
    }

    /// Get the connection state
    pub fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    /// Compute shared secret with remote public key
    pub fn compute_shared_secret(&mut self, remote_public_key: [u8; 32]) -> Result<()> {
        let public_key = PublicKey::from(remote_public_key);
        let shared = self.keypair.dh(&public_key);

        self.shared_secret = Some(shared);
        self.state = ConnectionState::Connected;

        Ok(())
    }

    /// Perform key exchange with remote peer
    pub async fn perform_key_exchange(
        &mut self,
        remote_public_key: Option<[u8; 32]>,
    ) -> Result<()> {
        if let Some(remote_key) = remote_public_key {
            // If we have the remote key, compute shared secret directly
            self.compute_shared_secret(remote_key)?;

            // Send handshake message with our public key
            let handshake = Message::Handshake {
                public_key: self.keypair.public.to_bytes(),
                session_id: self.session_id.clone(),
            };

            // Since we don't have encryption yet, send raw
            let handshake_data = bincode::serialize(&handshake)?;
            self.socket.send_to(&handshake_data, self.remote).await?;

            self.state = ConnectionState::Connected;
        } else {
            // Send initial handshake and wait for response
            self.state = ConnectionState::Connecting;

            let handshake = Message::Handshake {
                public_key: self.keypair.public.to_bytes(),
                session_id: self.session_id.clone(),
            };

            let handshake_data = bincode::serialize(&handshake)?;
            self.socket.send_to(&handshake_data, self.remote).await?;

            // Wait for handshake reply
            let mut buf = [0u8; 1024];
            let (size, addr) =
                tokio::time::timeout(Duration::from_secs(5), self.socket.recv_from(&mut buf))
                    .await??;

            if addr != self.remote {
                return Err(anyhow!("Received handshake from unexpected address"));
            }

            // Decode response
            let response: Message = bincode::deserialize(&buf[..size])?;

            match response {
                Message::Handshake {
                    public_key,
                    session_id,
                } => {
                    self.compute_shared_secret(public_key)?;
                    self.session_id = session_id;
                    self.state = ConnectionState::Connected;
                }
                _ => return Err(anyhow!("Unexpected message during handshake")),
            }
        }

        Ok(())
    }

    /// Validate a packet before processing
    fn validate_packet(&self, packet: &[u8]) -> Result<()> {
        // Check packet minimum size
        if packet.len() < 24 + 16 {
            // nonce + minimum AEAD tag
            return Err(anyhow!("Packet too small"));
        }

        // Apply rate limiting
        let mut limiter = self.rate_limiter.lock().unwrap();
        if !limiter.consume(1) {
            return Err(anyhow!("Rate limit exceeded"));
        }

        Ok(())
    }

    /// Send a message to the remote peer
    pub async fn send(&self, message: &Message) -> Result<()> {
        // Serialize message
        let message_data = bincode::serialize(message)?;

        // Check if secure channel is established
        if let Some(shared_secret) = self.shared_secret {
            // Generate random nonce
            let mut nonce = [0u8; 24];
            thread_rng().fill_bytes(&mut nonce);

            // Encrypt data
            let cipher = XChaCha20Poly1305::new((&shared_secret).into());
            let ciphertext = cipher
                .encrypt(&nonce.into(), message_data.as_ref())
                .map_err(|e| anyhow!("Encryption error: {}", e))?;

            // Combine nonce and ciphertext
            let mut packet = Vec::with_capacity(nonce.len() + ciphertext.len());
            packet.extend_from_slice(&nonce);
            packet.extend_from_slice(&ciphertext);

            // Send encrypted data
            self.socket.send_to(&packet, self.remote).await?;

            Ok(())
        } else {
            Err(anyhow!("Secure channel not established"))
        }
    }

    /// Send raw audio data
    pub async fn send_audio(&self, audio_data: &[u8], timestamp: u64) -> Result<()> {
        let message = Message::Audio {
            data: audio_data.to_vec(),
            timestamp,
        };

        self.send(&message).await
    }

    /// Send heartbeat to keep connection alive
    pub async fn send_heartbeat(&self) -> Result<()> {
        self.send(&Message::Heartbeat).await
    }

    /// Receive a message from the remote peer
    pub async fn receive(&self) -> Result<Message> {
        let mut buf = [0u8; 65536]; // Large buffer for audio data

        let (size, addr) = self.socket.recv_from(&mut buf).await?;

        // Verify sender
        if addr != self.remote {
            return Err(anyhow!("Received packet from unexpected address"));
        }

        // Validate packet
        self.validate_packet(&buf[..size])?;

        // Check if secure channel is established
        if let Some(shared_secret) = self.shared_secret {
            if size < 24 {
                return Err(anyhow!("Received packet too small"));
            }

            // Extract nonce and ciphertext
            let nonce = &buf[..24];
            let ciphertext = &buf[24..size];

            // Decrypt data
            let cipher = XChaCha20Poly1305::new((&shared_secret).into());
            let plaintext = cipher
                .decrypt(nonce.into(), ciphertext)
                .map_err(|e| anyhow!("Decryption error: {}", e))?;

            // Deserialize message
            let message = bincode::deserialize(&plaintext)?;

            Ok(message)
        } else {
            // During initial handshake, messages are not encrypted
            let message = bincode::deserialize(&buf[..size])?;
            Ok(message)
        }
    }

    /// Listen for incoming messages and handle them
    pub async fn listen<F>(&self, mut handler: F) -> Result<()>
    where
        F: FnMut(Message) -> Result<()>,
    {
        loop {
            match self.receive().await {
                Ok(message) => {
                    // Handle heartbeats internally
                    match &message {
                        Message::Heartbeat => {
                            // Just acknowledge heartbeat received
                            continue;
                        }
                        _ => handler(message)?,
                    }
                }
                Err(e) => {
                    // Some errors are expected (timeouts, etc)
                    eprintln!("Error receiving message: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Clone the socket for use in another task
    pub fn clone_socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    /// Get the remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = Keypair::generate();
        let public_bytes = keypair.public.to_bytes();

        // Public key should be 32 bytes
        assert_eq!(public_bytes.len(), 32);

        // Public key shouldn't be all zeros
        assert_ne!(public_bytes, [0u8; 32]);
    }

    #[test]
    fn test_diffie_hellman() {
        let keypair1 = Keypair::generate();
        let keypair2 = Keypair::generate();

        let shared1 = keypair1.dh(&keypair2.public);
        let shared2 = keypair2.dh(&keypair1.public);

        // Both sides should compute the same shared secret
        assert_eq!(shared1, shared2);

        // Shared secret shouldn't be all zeros
        assert_ne!(shared1, [0u8; 32]);
    }

    #[test]
    fn test_message_serialization() {
        let message = Message::Handshake {
            public_key: [42u8; 32],
            session_id: "test-session".to_string(),
        };

        let serialized = bincode::serialize(&message).unwrap();
        let deserialized: Message = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            Message::Handshake {
                public_key,
                session_id,
            } => {
                assert_eq!(public_key, [42u8; 32]);
                assert_eq!(session_id, "test-session");
            }
            _ => panic!("Wrong message type after deserialization"),
        }
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(10, 1.0);

        // Should allow 10 messages immediately
        for _ in 0..10 {
            assert!(limiter.consume(1));
        }

        // Should deny the 11th message
        assert!(!limiter.consume(1));

        // After waiting, should allow more messages
        std::thread::sleep(Duration::from_secs(1));
        assert!(limiter.consume(1));
    }
}
