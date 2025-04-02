use crate::events::NetworkEvent;
use crate::protocol::{self, PeerInfo, Phase1Message, PROTOCOL_VERSION};
use crypto::CryptoProvider;
use log::{debug, error, info, trace, warn};
use room_core::{Error, PeerId, RoomId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use x25519_dalek::{EphemeralSecret, PublicKey};

/// Default port for Phase 1 connections
pub const DEFAULT_PORT: u16 = 12345;

/// Maximum number of retries for sending messages
const MAX_RETRIES: usize = 3;

/// Timeout for handshake operations in seconds
const HANDSHAKE_TIMEOUT: u64 = 10;

/// Interval for sending ping messages in seconds
const PING_INTERVAL: u64 = 30;

/// Connection state for a peer
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionState {
    /// Initial state, no connection yet
    None,
    /// Hello exchanged, waiting for DH exchange
    HelloExchanged,
    /// DH keys exchanged, waiting for authentication
    KeyExchanged,
    /// Authenticated and secure channel established
    Authenticated,
    /// Join request sent, waiting for response
    JoinRequested,
    /// Joined the room
    Joined,
}

/// A wrapper around EphemeralSecret that implements Debug
struct DebugEphemeralSecret(EphemeralSecret);

impl std::fmt::Debug for DebugEphemeralSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EphemeralSecret")
            .field("bytes", &"[redacted]")
            .finish()
    }
}

impl From<EphemeralSecret> for DebugEphemeralSecret {
    fn from(secret: EphemeralSecret) -> Self {
        Self(secret)
    }
}

impl DebugEphemeralSecret {
    fn inner(&self) -> &EphemeralSecret {
        &self.0
    }

    fn into_inner(self) -> EphemeralSecret {
        self.0
    }
}

/// Represents a peer connection in Phase 1
#[derive(Debug)]
struct PeerConnection {
    /// Peer information
    info: PeerInfo,
    /// Current connection state
    state: ConnectionState,
    /// Our DH private key for this connection
    private_key: Option<DebugEphemeralSecret>,
    /// Peer's DH public key
    peer_public_key: Option<PublicKey>,
    /// Shared secret derived from DH exchange
    shared_secret: Option<[u8; 32]>,
    /// AEAD encryption key derived from shared secret
    encryption_key: Option<Vec<u8>>,
    /// AEAD decryption key derived from shared secret
    decryption_key: Option<Vec<u8>>,
    /// HMAC key for authenticating messages
    hmac_key: Option<Vec<u8>>,
    /// Last time we received a message from this peer
    last_received: std::time::Instant,
}

impl PeerConnection {
    /// Create a new peer connection
    fn new(peer_id: PeerId, address: SocketAddr) -> Self {
        Self {
            info: PeerInfo {
                peer_id,
                address,
                name: None,
            },
            state: ConnectionState::None,
            private_key: None,
            peer_public_key: None,
            shared_secret: None,
            encryption_key: None,
            decryption_key: None,
            hmac_key: None,
            last_received: std::time::Instant::now(),
        }
    }

    /// Update the last received time
    fn update_received_time(&mut self) {
        self.last_received = std::time::Instant::now();
    }

    /// Check if this peer has timed out
    fn is_timed_out(&self, timeout: Duration) -> bool {
        self.last_received.elapsed() > timeout
    }
}

/// Manages Phase 1 networking (UDP-based secure channel)
pub struct Phase1Network {
    /// Our peer ID
    peer_id: PeerId,
    /// Current room ID
    room_id: Option<RoomId>,
    /// UDP socket for communication
    pub(crate) socket: Arc<UdpSocket>,
    /// Connected peers
    peers: Arc<RwLock<HashMap<PeerId, PeerConnection>>>,
    /// Peers indexed by socket address for lookup
    peer_addresses: Arc<RwLock<HashMap<SocketAddr, PeerId>>>,
    /// Channel to send network events to listeners
    event_sender: mpsc::Sender<NetworkEvent>,
    /// Crypto provider for security operations
    crypto: CryptoProvider,
    /// Whether this peer is a "server" (first in room)
    is_server: bool,
}

impl Phase1Network {
    /// Create a new Phase1Network
    pub async fn new(
        peer_id: PeerId,
        bind_addr: Option<SocketAddr>,
        event_sender: mpsc::Sender<NetworkEvent>,
    ) -> Result<Self, Error> {
        let bind_addr = bind_addr.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)));

        let socket = UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| Error::Network(format!("Failed to bind UDP socket: {}", e)))?;

        info!(
            "Phase1Network bound to {}",
            socket
                .local_addr()
                .map_err(|e| Error::Network(format!("Failed to get local address: {}", e)))?
        );

        let crypto = CryptoProvider::new()?;

        Ok(Self {
            peer_id,
            room_id: None,
            socket: Arc::new(socket),
            peers: Arc::new(RwLock::new(HashMap::new())),
            peer_addresses: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            crypto,
            is_server: false,
        })
    }

    /// Start the network processing loop
    pub async fn start(&self) -> Result<(), Error> {
        // Start the receiver task
        self.start_receiver().await?;

        // Start the ping task to keep connections alive
        self.start_ping_task().await?;

        Ok(())
    }

    // Start the receiver task
    async fn start_receiver(&self) -> Result<(), Error> {
        let socket = self.socket.clone();
        let peers = self.peers.clone();
        let peer_addresses = self.peer_addresses.clone();
        let event_sender = self.event_sender.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; protocol::MAX_UDP_PAYLOAD_SIZE];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let message_bytes = &buf[..len];

                        // Process the received message
                        match protocol::deserialize(message_bytes) {
                            Ok(message) => {
                                let peer_id = {
                                    let addresses = peer_addresses.read().await;
                                    addresses.get(&addr).cloned()
                                };

                                // Handle the message
                                if let Err(e) = Self::handle_message(
                                    message,
                                    addr,
                                    peer_id,
                                    &peers,
                                    &peer_addresses,
                                    &event_sender,
                                    &socket,
                                )
                                .await
                                {
                                    error!("Error handling message from {}: {}", addr, e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize message from {}: {}", addr, e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving from socket: {}", e);
                        // Short delay to prevent tight loop on persistent errors
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    // Start the ping task
    async fn start_ping_task(&self) -> Result<(), Error> {
        let socket = self.socket.clone();
        let peers = self.peers.clone();
        let peer_addresses = self.peer_addresses.clone();
        let event_sender = self.event_sender.clone();
        let peer_id = self.peer_id;

        tokio::spawn(async move {
            let ping_interval = Duration::from_secs(PING_INTERVAL);
            let mut interval = time::interval(ping_interval);

            loop {
                interval.tick().await;

                let peers_to_ping = {
                    let peers_lock = peers.read().await;
                    peers_lock
                        .iter()
                        .filter(|(_, conn)| conn.state == ConnectionState::Joined)
                        .map(|(peer_id, conn)| (*peer_id, conn.info.address))
                        .collect::<Vec<_>>()
                };

                for (_, addr) in peers_to_ping {
                    let ping = Phase1Message::Ping { peer_id };

                    if let Ok(bytes) = protocol::serialize(&ping) {
                        if let Err(e) = socket.send_to(&bytes, addr).await {
                            warn!("Failed to send ping to {}: {}", addr, e);
                        }
                    }
                }

                // Check for timed out peers
                let timeout = Duration::from_secs(PING_INTERVAL * 2);
                let timed_out_peers = {
                    let mut peers_lock = peers.write().await;
                    let timed_out: Vec<(PeerId, SocketAddr)> = peers_lock
                        .iter()
                        .filter(|(_, conn)| {
                            conn.state == ConnectionState::Joined && conn.is_timed_out(timeout)
                        })
                        .map(|(peer_id, conn)| (*peer_id, conn.info.address))
                        .collect();

                    // Remove timed out peers
                    for (peer_id, _) in &timed_out {
                        peers_lock.remove(peer_id);
                    }

                    timed_out
                };

                // Update the address map and notify about disconnections
                for (peer_id, addr) in timed_out_peers {
                    let mut addresses = peer_addresses.write().await;
                    addresses.remove(&addr);

                    // Notify about the disconnection
                    let event = NetworkEvent::PeerDisconnected {
                        peer_id,
                        reason: Some("Ping timeout".to_string()),
                    };

                    if let Err(e) = event_sender.send(event).await {
                        error!("Failed to send disconnect event: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle a received message
    async fn handle_message(
        message: Phase1Message,
        addr: SocketAddr,
        peer_id: Option<PeerId>,
        peers: &Arc<RwLock<HashMap<PeerId, PeerConnection>>>,
        peer_addresses: &Arc<RwLock<HashMap<SocketAddr, PeerId>>>,
        event_sender: &mpsc::Sender<NetworkEvent>,
        socket: &Arc<UdpSocket>,
    ) -> Result<(), Error> {
        match message {
            Phase1Message::HelloInitiate {
                version,
                room_id,
                peer_id,
            } => {
                if version != PROTOCOL_VERSION {
                    warn!(
                        "Protocol version mismatch: received {}, expected {}",
                        version, PROTOCOL_VERSION
                    );
                    return Ok(());
                }

                // Add the new peer to our maps
                let mut peers_lock = peers.write().await;
                let mut addresses_lock = peer_addresses.write().await;

                let mut peer_conn = PeerConnection::new(peer_id, addr);
                peer_conn.state = ConnectionState::HelloExchanged;
                peer_conn.update_received_time();

                peers_lock.insert(peer_id, peer_conn);
                addresses_lock.insert(addr, peer_id);

                // Send HelloAck
                let hello_ack = Phase1Message::HelloAck {
                    version: PROTOCOL_VERSION,
                    room_id,
                    peer_id: peer_id, // The server's peer ID
                };

                let bytes = protocol::serialize(&hello_ack).map_err(|e| {
                    Error::Serialization(format!("Failed to serialize HelloAck: {}", e))
                })?;

                socket
                    .send_to(&bytes, addr)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send HelloAck: {}", e)))?;

                // Generate DH keypair and send public key
                let (private_key, public_key) = CryptoProvider::generate_dh_keypair();
                let dh_message = Phase1Message::DHPubKey {
                    pub_key: public_key.to_bytes(),
                };

                let bytes = protocol::serialize(&dh_message).map_err(|e| {
                    Error::Serialization(format!("Failed to serialize DHPubKey: {}", e))
                })?;

                socket
                    .send_to(&bytes, addr)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send DHPubKey: {}", e)))?;

                // Update the peer's state
                if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                    peer_conn.private_key = Some(DebugEphemeralSecret(private_key));
                    peer_conn.state = ConnectionState::KeyExchanged;
                }

                // Notify about the new peer
                let event = NetworkEvent::PeerConnected {
                    peer_id,
                    address: addr,
                };

                event_sender
                    .send(event)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send event: {}", e)))?;
            }

            Phase1Message::HelloAck {
                version,
                room_id,
                peer_id,
            } => {
                if version != PROTOCOL_VERSION {
                    warn!(
                        "Protocol version mismatch: received {}, expected {}",
                        version, PROTOCOL_VERSION
                    );
                    return Ok(());
                }

                // Add the peer to our maps
                let mut peers_lock = peers.write().await;
                let mut addresses_lock = peer_addresses.write().await;

                let mut peer_conn = PeerConnection::new(peer_id, addr);
                peer_conn.state = ConnectionState::HelloExchanged;
                peer_conn.update_received_time();

                peers_lock.insert(peer_id, peer_conn);
                addresses_lock.insert(addr, peer_id);

                // Generate DH keypair and send public key
                let (private_key, public_key) = CryptoProvider::generate_dh_keypair();
                let dh_message = Phase1Message::DHPubKey {
                    pub_key: public_key.to_bytes(),
                };

                let bytes = protocol::serialize(&dh_message).map_err(|e| {
                    Error::Serialization(format!("Failed to serialize DHPubKey: {}", e))
                })?;

                socket
                    .send_to(&bytes, addr)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send DHPubKey: {}", e)))?;

                // Update the peer's state
                if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                    peer_conn.private_key = Some(DebugEphemeralSecret(private_key));
                    peer_conn.state = ConnectionState::KeyExchanged;
                }

                // Notify about the new peer
                let event = NetworkEvent::PeerConnected {
                    peer_id,
                    address: addr,
                };

                event_sender
                    .send(event)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send event: {}", e)))?;
            }

            Phase1Message::DHPubKey { pub_key } => {
                if let Some(peer_id) = peer_id {
                    let mut peers_lock = peers.write().await;

                    if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                        peer_conn.update_received_time();

                        // Store the peer's public key
                        let peer_public_key = PublicKey::from(pub_key);
                        peer_conn.peer_public_key = Some(peer_public_key);

                        // Compute shared secret if we have our private key
                        if let Some(private_key) = peer_conn.private_key.take() {
                            let shared_secret = CryptoProvider::compute_shared_secret(
                                private_key.into_inner(),
                                &peer_public_key,
                            );
                            peer_conn.shared_secret = Some(shared_secret);

                            // Derive keys from shared secret
                            let room_id_bytes = b"room_id"; // placeholder, should use actual room ID bytes

                            // Derive encryption key
                            let encryption_key = CryptoProvider::derive_key(
                                &shared_secret,
                                room_id_bytes,
                                b"encryption",
                                32, // 32 bytes for ChaCha20Poly1305
                            )
                            .map_err(|e| {
                                Error::Crypto(format!("Failed to derive encryption key: {}", e))
                            })?;

                            // Derive decryption key
                            let decryption_key = CryptoProvider::derive_key(
                                &shared_secret,
                                room_id_bytes,
                                b"decryption",
                                32, // 32 bytes for ChaCha20Poly1305
                            )
                            .map_err(|e| {
                                Error::Crypto(format!("Failed to derive decryption key: {}", e))
                            })?;

                            // Derive HMAC key
                            let hmac_key = CryptoProvider::derive_key(
                                &shared_secret,
                                room_id_bytes,
                                b"hmac",
                                32, // 32 bytes for HMAC-SHA256
                            )
                            .map_err(|e| {
                                Error::Crypto(format!("Failed to derive HMAC key: {}", e))
                            })?;

                            peer_conn.encryption_key = Some(encryption_key);
                            peer_conn.decryption_key = Some(decryption_key);
                            peer_conn.hmac_key = Some(hmac_key);

                            // Create and send AuthTag
                            if let Some(hmac_key) = &peer_conn.hmac_key {
                                // Compute HMAC over the concatenated keys
                                let tag = CryptoProvider::hmac(
                                    hmac_key,
                                    &pub_key[..], // Just use the peer's public key bytes
                                )
                                .map_err(|e| {
                                    Error::Crypto(format!("Failed to compute HMAC: {}", e))
                                })?;

                                // Send AuthTag
                                let auth_tag = Phase1Message::AuthTag { tag };
                                let bytes = protocol::serialize(&auth_tag).map_err(|e| {
                                    Error::Serialization(format!(
                                        "Failed to serialize AuthTag: {}",
                                        e
                                    ))
                                })?;

                                socket.send_to(&bytes, addr).await.map_err(|e| {
                                    Error::Network(format!("Failed to send AuthTag: {}", e))
                                })?;

                                // Update state to authenticated
                                peer_conn.state = ConnectionState::Authenticated;
                            }
                        }
                    }
                }
            }

            Phase1Message::AuthTag { tag } => {
                if let Some(peer_id) = peer_id {
                    let mut peers_lock = peers.write().await;

                    if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                        peer_conn.update_received_time();

                        // Verify the AuthTag
                        if let (Some(hmac_key), Some(peer_public_key)) =
                            (&peer_conn.hmac_key, &peer_conn.peer_public_key)
                        {
                            // Verify HMAC
                            match CryptoProvider::verify_hmac(
                                hmac_key,
                                peer_public_key.as_bytes(), // Just use the peer's public key bytes
                                &tag,
                            ) {
                                Ok(()) => {
                                    // HMAC verified, update state to authenticated
                                    peer_conn.state = ConnectionState::Authenticated;

                                    // Now that we have a secure channel, we can send a join request
                                    let join_request = Phase1Message::JoinRequest {
                                        peer_id: peer_id,         // Our peer ID
                                        name: "User".to_string(), // Replace with actual user name
                                    };

                                    // Encrypt the join request
                                    if let Some(encryption_key) = &peer_conn.encryption_key {
                                        let plaintext = protocol::serialize(&join_request)
                                            .map_err(|e| {
                                                Error::Serialization(format!(
                                                    "Failed to serialize JoinRequest: {}",
                                                    e
                                                ))
                                            })?;

                                        let encrypted = CryptoProvider::encrypt(
                                            encryption_key,
                                            &plaintext,
                                            &[], // No associated data for now
                                        )
                                        .map_err(|e| {
                                            Error::Crypto(format!(
                                                "Failed to encrypt JoinRequest: {}",
                                                e
                                            ))
                                        })?;

                                        // Send encrypted message
                                        let encrypted_msg =
                                            Phase1Message::EncryptedMessage { payload: encrypted };

                                        let bytes =
                                            protocol::serialize(&encrypted_msg).map_err(|e| {
                                                Error::Serialization(format!(
                                                    "Failed to serialize EncryptedMessage: {}",
                                                    e
                                                ))
                                            })?;

                                        socket.send_to(&bytes, addr).await.map_err(|e| {
                                            Error::Network(format!(
                                                "Failed to send encrypted JoinRequest: {}",
                                                e
                                            ))
                                        })?;

                                        // Update state to join requested
                                        peer_conn.state = ConnectionState::JoinRequested;
                                    }
                                }
                                Err(e) => {
                                    error!("HMAC verification failed: {}", e);

                                    // Notify about authentication failure
                                    let event = NetworkEvent::AuthenticationFailed {
                                        address: addr,
                                        reason: "HMAC verification failed".to_string(),
                                    };

                                    event_sender.send(event).await.map_err(|e| {
                                        Error::Network(format!(
                                            "Failed to send auth failure event: {}",
                                            e
                                        ))
                                    })?;
                                }
                            }
                        }
                    }
                }
            }

            Phase1Message::EncryptedMessage { payload } => {
                if let Some(peer_id) = peer_id {
                    let peers_lock = peers.read().await;

                    if let Some(peer_conn) = peers_lock.get(&peer_id) {
                        // Only process encrypted messages from authenticated peers
                        if peer_conn.state == ConnectionState::Authenticated
                            || peer_conn.state == ConnectionState::JoinRequested
                            || peer_conn.state == ConnectionState::Joined
                        {
                            // Decrypt the message
                            if let Some(decryption_key) = &peer_conn.decryption_key {
                                match CryptoProvider::decrypt(
                                    decryption_key,
                                    &payload,
                                    &[], // No associated data for now
                                ) {
                                    Ok(plaintext) => {
                                        // Deserialize the decrypted message
                                        match protocol::deserialize(&plaintext) {
                                            Ok(inner_message) => {
                                                // Process the inner message
                                                match inner_message {
                                                    Phase1Message::JoinRequest {
                                                        peer_id,
                                                        name,
                                                    } => {
                                                        // Notify about join request
                                                        let event = NetworkEvent::JoinRequested {
                                                            peer_id,
                                                            name,
                                                            address: addr,
                                                        };

                                                        event_sender.send(event).await
                                                            .map_err(|e| Error::Network(format!("Failed to send join request event: {}", e)))?;
                                                    }
                                                    Phase1Message::JoinResponse {
                                                        approved,
                                                        reason,
                                                    } => {
                                                        // Update peer state if approved
                                                        if approved {
                                                            let mut peers_lock =
                                                                peers.write().await;
                                                            if let Some(peer_conn) =
                                                                peers_lock.get_mut(&peer_id)
                                                            {
                                                                peer_conn.state =
                                                                    ConnectionState::Joined;
                                                            }
                                                        }

                                                        // Notify about join response
                                                        let event =
                                                            NetworkEvent::JoinResponseReceived {
                                                                approved,
                                                                reason,
                                                            };

                                                        event_sender.send(event).await
                                                            .map_err(|e| Error::Network(format!("Failed to send join response event: {}", e)))?;
                                                    }
                                                    // Handle other message types...
                                                    _ => {
                                                        // Forward message to application
                                                        let event = NetworkEvent::MessageReceived {
                                                            peer_id,
                                                            message: inner_message,
                                                        };

                                                        event_sender.send(event).await
                                                            .map_err(|e| Error::Network(format!("Failed to send message received event: {}", e)))?;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to deserialize decrypted message: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to decrypt message from {}: {}", addr, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Phase1Message::Ping {
                peer_id: ping_peer_id,
            } => {
                // Send a pong in response
                let pong = Phase1Message::Pong {
                    peer_id: ping_peer_id,
                };

                let bytes = protocol::serialize(&pong).map_err(|e| {
                    Error::Serialization(format!("Failed to serialize Pong: {}", e))
                })?;

                socket
                    .send_to(&bytes, addr)
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send Pong: {}", e)))?;

                // Update the last received time
                if let Some(received_peer_id) = peer_id {
                    let mut peers_lock = peers.write().await;
                    if let Some(peer_conn) = peers_lock.get_mut(&received_peer_id) {
                        peer_conn.update_received_time();
                    }
                }
            }

            Phase1Message::Pong { peer_id: _ } => {
                // Update the last received time
                if let Some(received_peer_id) = peer_id {
                    let mut peers_lock = peers.write().await;
                    if let Some(peer_conn) = peers_lock.get_mut(&received_peer_id) {
                        peer_conn.update_received_time();
                    }
                }
            }

            // Handle other message types...
            _ => {}
        }

        Ok(())
    }

    /// Connect to a peer
    pub async fn connect(&mut self, room_id: RoomId, address: SocketAddr) -> Result<(), Error> {
        self.room_id = Some(room_id);

        // Send HelloInitiate
        let hello = Phase1Message::HelloInitiate {
            version: PROTOCOL_VERSION,
            room_id,
            peer_id: self.peer_id,
        };

        let bytes = protocol::serialize(&hello).map_err(|e| {
            Error::Serialization(format!("Failed to serialize HelloInitiate: {}", e))
        })?;

        self.socket
            .send_to(&bytes, address)
            .await
            .map_err(|e| Error::Network(format!("Failed to send HelloInitiate: {}", e)))?;

        Ok(())
    }

    /// Create a new room (become server)
    pub async fn create_room(&mut self, room_id: RoomId) -> Result<(), Error> {
        self.room_id = Some(room_id);
        self.is_server = true;

        info!("Created room {}", room_id);

        Ok(())
    }

    /// Send a message to a peer
    pub async fn send_message(&self, peer_id: PeerId, message: Phase1Message) -> Result<(), Error> {
        let peers_lock = self.peers.read().await;

        if let Some(peer_conn) = peers_lock.get(&peer_id) {
            // Only send to authenticated peers
            if peer_conn.state == ConnectionState::Authenticated
                || peer_conn.state == ConnectionState::JoinRequested
                || peer_conn.state == ConnectionState::Joined
            {
                // Encrypt the message
                if let Some(encryption_key) = &peer_conn.encryption_key {
                    let plaintext = protocol::serialize(&message).map_err(|e| {
                        Error::Serialization(format!("Failed to serialize message: {}", e))
                    })?;

                    let encrypted = CryptoProvider::encrypt(
                        encryption_key,
                        &plaintext,
                        &[], // No associated data for now
                    )
                    .map_err(|e| Error::Crypto(format!("Failed to encrypt message: {}", e)))?;

                    // Send encrypted message
                    let encrypted_msg = Phase1Message::EncryptedMessage { payload: encrypted };

                    let bytes = protocol::serialize(&encrypted_msg).map_err(|e| {
                        Error::Serialization(format!("Failed to serialize EncryptedMessage: {}", e))
                    })?;

                    self.socket
                        .send_to(&bytes, peer_conn.info.address)
                        .await
                        .map_err(|e| {
                            Error::Network(format!("Failed to send encrypted message: {}", e))
                        })?;

                    return Ok(());
                }
            }

            return Err(Error::Network(format!(
                "Peer not authenticated: {}",
                peer_id
            )));
        }

        Err(Error::Network(format!("Peer not found: {}", peer_id)))
    }

    /// Send a join response to a peer
    pub async fn send_join_response(
        &self,
        peer_id: PeerId,
        approved: bool,
        reason: Option<String>,
    ) -> Result<(), Error> {
        let response = Phase1Message::JoinResponse { approved, reason };

        self.send_message(peer_id, response).await
    }

    /// Get all connected peers
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        let peers_lock = self.peers.read().await;

        peers_lock
            .values()
            .filter(|conn| conn.state == ConnectionState::Joined)
            .map(|conn| conn.info.clone())
            .collect()
    }

    /// Close connection to a peer
    pub async fn disconnect_peer(&self, peer_id: PeerId) -> Result<(), Error> {
        let mut peers_lock = self.peers.write().await;
        let mut addresses_lock = self.peer_addresses.write().await;

        if let Some(peer_conn) = peers_lock.remove(&peer_id) {
            addresses_lock.remove(&peer_conn.info.address);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_create_phase1_network() {
        let peer_id = PeerId::new();
        let (sender, _receiver) = mpsc::channel(100);

        let bind_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            0, // Use port 0 to automatically assign a free port
        );

        let network = Phase1Network::new(peer_id, Some(bind_addr), sender).await;
        assert!(network.is_ok());

        let network = network.unwrap();
        let local_addr = network.socket.local_addr().unwrap();
        assert_eq!(local_addr.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    // Additional tests would follow for message handling, encryption, etc.
}
