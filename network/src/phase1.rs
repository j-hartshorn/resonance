use crate::events::NetworkEvent;
use crate::protocol::{self, PeerInfo, Phase1Message, PROTOCOL_VERSION};
use bincode;
use crypto::CryptoProvider;
use log::{debug, error, info, trace, warn};
use room_core::{Error, PeerId, RoomId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use x25519_dalek::{EphemeralSecret, PublicKey}; // Ensure bincode is imported if used directly

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
    /// Our own public key used in the DH exchange for this peer
    our_public_key: Option<PublicKey>,
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
            our_public_key: None,
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
        let self_peer_id = self.peer_id;
        let network_state = Arc::new(RwLock::new((self.room_id, self.is_server)));

        tokio::spawn(async move {
            let mut buf = vec![0u8; protocol::MAX_UDP_PAYLOAD_SIZE];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let message_bytes = &buf[..len];
                        match protocol::deserialize(message_bytes) {
                            Ok(message) => {
                                let incoming_peer_id_opt = {
                                    let addresses = peer_addresses.read().await;
                                    addresses.get(&addr).cloned()
                                };
                                let (room_id, is_server) = *network_state.read().await;
                                if let Err(e) = Phase1Network::handle_message(
                                    message,
                                    addr,
                                    incoming_peer_id_opt,
                                    self_peer_id,
                                    room_id,
                                    is_server,
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

    /// Handle a received message (Static method version)
    async fn handle_message(
        message: Phase1Message,
        addr: SocketAddr,
        incoming_peer_id_opt: Option<PeerId>,
        self_peer_id: PeerId,
        current_room_id: Option<RoomId>,
        is_server: bool,
        peers: &Arc<RwLock<HashMap<PeerId, PeerConnection>>>,
        peer_addresses: &Arc<RwLock<HashMap<SocketAddr, PeerId>>>,
        event_sender: &mpsc::Sender<NetworkEvent>,
        socket: &Arc<UdpSocket>,
    ) -> Result<(), Error> {
        match message {
            Phase1Message::HelloInitiate {
                version,
                room_id,
                peer_id: initiating_peer_id,
            } => {
                if version != PROTOCOL_VERSION {
                    warn!(
                        "Protocol version mismatch: received {}, expected {}",
                        version, PROTOCOL_VERSION
                    );
                    return Ok(());
                }

                // We are the server in this case (assuming this message is only processed by the server)
                // The `is_server` flag should be set by `create_room` on the instance.

                let server_peer_id = self_peer_id;

                // Add the new peer
                let mut peers_lock = peers.write().await;
                let mut addresses_lock = peer_addresses.write().await;

                let mut peer_conn = PeerConnection::new(initiating_peer_id, addr);
                peer_conn.state = ConnectionState::HelloExchanged;

                let (server_private_key, server_public_key) = CryptoProvider::generate_dh_keypair();
                peer_conn.our_public_key = Some(server_public_key);
                peer_conn.private_key = Some(DebugEphemeralSecret(server_private_key));

                peers_lock.insert(initiating_peer_id, peer_conn);
                addresses_lock.insert(addr, initiating_peer_id);

                // Release locks before sending event and subsequent messages
                drop(peers_lock);
                drop(addresses_lock);

                // Send event AFTER releasing locks
                let event = NetworkEvent::PeerConnected {
                    peer_id: initiating_peer_id,
                    address: addr,
                };
                event_sender.send(event).await.map_err(|e| {
                    Error::Network(format!("Failed to send PeerConnected event: {}", e))
                })?;

                // Send HelloAck
                let hello_ack = Phase1Message::HelloAck {
                    version: PROTOCOL_VERSION,
                    room_id,
                    peer_id: server_peer_id,
                };
                let bytes = protocol::serialize(&hello_ack)?;
                socket.send_to(&bytes, addr).await?;

                // Send *our* (server's) DH public key
                let dh_message = Phase1Message::DHPubKey {
                    pub_key: server_public_key.to_bytes(),
                };
                let bytes = protocol::serialize(&dh_message)?;
                socket.send_to(&bytes, addr).await?;

                info!(
                    "Processed HelloInitiate from {}, sent HelloAck & DHPubKey",
                    initiating_peer_id
                );
            }

            Phase1Message::HelloAck {
                version,
                room_id,
                peer_id: acking_peer_id,
            } => {
                if version != PROTOCOL_VERSION {
                    warn!(
                        "Protocol version mismatch: received {}, expected {}",
                        version, PROTOCOL_VERSION
                    );
                    return Ok(());
                }

                let client_peer_id = self_peer_id;

                // Store the peer (server)
                let mut peers_lock = peers.write().await;
                let mut addresses_lock = peer_addresses.write().await;

                let mut peer_conn = PeerConnection::new(acking_peer_id, addr);
                peer_conn.state = ConnectionState::HelloExchanged;

                peers_lock.insert(acking_peer_id, peer_conn);
                addresses_lock.insert(addr, acking_peer_id);

                // Release locks before sending event and subsequent messages
                drop(peers_lock);
                drop(addresses_lock);

                // Send event AFTER releasing locks
                let event = NetworkEvent::PeerConnected {
                    peer_id: acking_peer_id,
                    address: addr,
                };
                event_sender.send(event).await.map_err(|e| {
                    Error::Network(format!("Failed to send PeerConnected event: {}", e))
                })?;

                // Generate *our* (client's) DH keypair
                let (client_private_key, client_public_key) = CryptoProvider::generate_dh_keypair();

                // Send *our* (client's) DH public key
                let dh_message = Phase1Message::DHPubKey {
                    pub_key: client_public_key.to_bytes(),
                };
                let bytes = protocol::serialize(&dh_message)?;
                socket.send_to(&bytes, addr).await?;

                // Update the peer connection state for the server
                let mut peers_lock = peers.write().await;
                if let Some(server_conn) = peers_lock.get_mut(&acking_peer_id) {
                    server_conn.our_public_key = Some(client_public_key);
                    server_conn.private_key = Some(DebugEphemeralSecret(client_private_key));
                }
                info!("Processed HelloAck from {}, sent DHPubKey", acking_peer_id);
            }

            Phase1Message::DHPubKey { pub_key } => {
                if let Some(peer_id) = incoming_peer_id_opt {
                    info!("Received DHPubKey from {}", peer_id);
                    let mut peers_lock = peers.write().await;

                    if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                        peer_conn.update_received_time();
                        let received_peer_public_key = PublicKey::from(pub_key);
                        info!("Stored peer public key for {}", peer_id);
                        peer_conn.peer_public_key = Some(received_peer_public_key);

                        // Check if ready to derive keys
                        if peer_conn.private_key.is_some() && peer_conn.our_public_key.is_some() {
                            info!("Keys available, attempting derivation for {}", peer_id);
                            // Use a block to manage variable scope and take ownership cleanly
                            let maybe_derived_keys = {
                                // Take ownership of private key within this scope
                                let our_private_key =
                                    peer_conn.private_key.take().unwrap().into_inner();
                                let our_public_key = peer_conn.our_public_key.unwrap(); // Should be Some
                                let peer_public_key = peer_conn.peer_public_key.unwrap(); // Should be Some

                                // Compute shared secret
                                let shared_secret = CryptoProvider::compute_shared_secret(
                                    our_private_key,
                                    &peer_public_key,
                                );

                                // Retrieve RoomId and Peer IDs for KDF context
                                let room_id = current_room_id.ok_or_else(|| {
                                    error!("Room ID not set when processing DHPubKey");
                                    Error::InvalidState(
                                        "Room ID not set when processing DHPubKey".to_string(),
                                    )
                                })?;
                                let room_id_bytes = bincode::serialize(&room_id).map_err(|e| {
                                    Error::Serialization(format!(
                                        "Failed to serialize room_id for KDF: {}",
                                        e
                                    ))
                                })?;

                                // Determine key derivation order based on PeerIds
                                let (enc_context, dec_context) = if self_peer_id <= peer_id {
                                    (b"room-A-to-B", b"room-B-to-A")
                                } else {
                                    (b"room-B-to-A", b"room-A-to-B")
                                };

                                // Derive keys using shared secret, room_id context, and peer-order contexts
                                info!(
                                    "Deriving keys for {} <=> {} using room {}",
                                    self_peer_id, peer_id, room_id
                                );
                                info!("  Encrypt Context: {:?}", std::str::from_utf8(enc_context));
                                info!("  Decrypt Context: {:?}", std::str::from_utf8(dec_context));

                                let encryption_key = CryptoProvider::derive_key(
                                    &shared_secret,
                                    &room_id_bytes,
                                    enc_context,
                                    32,
                                )?;
                                let decryption_key = CryptoProvider::derive_key(
                                    &shared_secret,
                                    &room_id_bytes,
                                    dec_context,
                                    32,
                                )?;
                                let hmac_key = CryptoProvider::derive_key(
                                    &shared_secret,
                                    &room_id_bytes,
                                    b"room-hmac",
                                    32,
                                )?; // HMAC key can be common
                                info!("Keys derived successfully for {}", peer_id);

                                // Return derived keys and shared secret
                                Some((
                                    shared_secret,
                                    encryption_key,
                                    decryption_key,
                                    hmac_key,
                                    our_public_key,
                                    peer_public_key,
                                ))
                            };

                            if let Some((
                                shared_secret,
                                encryption_key,
                                decryption_key,
                                hmac_key,
                                our_public_key,
                                peer_public_key,
                            )) = maybe_derived_keys
                            {
                                // Store derived keys in the connection state
                                peer_conn.shared_secret = Some(shared_secret);
                                peer_conn.encryption_key = Some(encryption_key);
                                peer_conn.decryption_key = Some(decryption_key);
                                peer_conn.hmac_key = Some(hmac_key.clone());

                                // Create and send AuthTag
                                // HMAC over concatenated keys: sort keys first for consistency
                                let key_a = our_public_key.as_bytes();
                                let key_b = peer_public_key.as_bytes();
                                let data_to_hmac = {
                                    let mut data = Vec::with_capacity(64);
                                    if key_a <= key_b {
                                        data.extend_from_slice(key_a);
                                        data.extend_from_slice(key_b);
                                    } else {
                                        data.extend_from_slice(key_b);
                                        data.extend_from_slice(key_a);
                                    }
                                    data
                                };
                                info!("Calculating AuthTag HMAC for {}", peer_id);
                                let tag = CryptoProvider::hmac(&hmac_key, &data_to_hmac)?;
                                let auth_tag_msg = Phase1Message::AuthTag { tag };
                                let bytes = protocol::serialize(&auth_tag_msg)?;
                                let peer_addr = peer_conn.info.address;

                                // Release lock before await
                                info!("Releasing lock to send AuthTag to {}", peer_id);
                                drop(peers_lock);

                                socket.send_to(&bytes, peer_addr).await?;
                                info!("Sent AuthTag to {}", peer_id);

                                // Update state locally (re-acquire lock)
                                let mut peers_lock = peers.write().await;
                                if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                                    peer_conn.state = ConnectionState::KeyExchanged;
                                    info!("Set state to KeyExchanged for {}", peer_id);
                                }
                            }
                            // else: KDF failed, error already propagated by `?`
                        } else {
                            // Keys not ready yet, just stored the received key
                            info!(
                                "Received DHPubKey from {}, but local keys not ready yet.",
                                peer_id
                            );
                            peer_conn.state = ConnectionState::HelloExchanged; // Remain in HelloExchanged
                        }
                    }
                } else {
                    warn!("Received DHPubKey from unknown address: {}", addr);
                }
            }

            Phase1Message::AuthTag { tag } => {
                if let Some(peer_id) = incoming_peer_id_opt {
                    info!("Received AuthTag from {}", peer_id);
                    let mut peers_lock = peers.write().await;
                    if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                        peer_conn.update_received_time();

                        // Check if ready to verify
                        if let (Some(hmac_key), Some(our_public_key), Some(peer_public_key)) = (
                            &peer_conn.hmac_key,
                            &peer_conn.our_public_key,
                            &peer_conn.peer_public_key,
                        ) {
                            info!(
                                "Keys available, attempting AuthTag verification for {}",
                                peer_id
                            );
                            // Concatenate keys in the *same order* as calculation:
                            // sort keys first for consistency
                            let key_a = our_public_key.as_bytes();
                            let key_b = peer_public_key.as_bytes();
                            let data_to_verify = {
                                let mut data = Vec::with_capacity(64);
                                if key_a <= key_b {
                                    data.extend_from_slice(key_a);
                                    data.extend_from_slice(key_b);
                                } else {
                                    data.extend_from_slice(key_b);
                                    data.extend_from_slice(key_a);
                                }
                                data
                            };

                            match CryptoProvider::verify_hmac(hmac_key, &data_to_verify, &tag) {
                                Ok(()) => {
                                    info!("AuthTag verified successfully for {}", peer_id);
                                    peer_conn.state = ConnectionState::Authenticated;
                                    info!("Set state to Authenticated for {}", peer_id);

                                    // Send AuthenticationSucceeded event
                                    let auth_event =
                                        NetworkEvent::AuthenticationSucceeded { peer_id };
                                    let peer_addr = peer_conn.info.address; // Get address before releasing lock
                                    let encryption_key_opt = peer_conn.encryption_key.clone(); // Clone key before releasing
                                    drop(peers_lock); // Release lock before await

                                    info!("Sending AuthenticationSucceeded event for {}", peer_id);
                                    if let Err(e) = event_sender.send(auth_event).await {
                                        error!(
                                            "Failed to send AuthenticationSucceeded event: {}",
                                            e
                                        );
                                        return Err(Error::Network(format!(
                                            "Failed to send event: {}",
                                            e
                                        )));
                                    }

                                    // If we are the client, send JoinRequest
                                    if !is_server {
                                        info!(
                                            "Client {} authenticated, sending JoinRequest",
                                            peer_id
                                        );
                                        let join_request = Phase1Message::JoinRequest {
                                            peer_id: self_peer_id,
                                            name: "User".to_string(),
                                        };
                                        let encryption_key =
                                            encryption_key_opt.ok_or_else(|| {
                                                Error::InvalidState(
                                                    "Missing encryption key after auth".to_string(),
                                                )
                                            })?;
                                        let plaintext = protocol::serialize(&join_request)?;
                                        let encrypted = CryptoProvider::encrypt(
                                            &encryption_key,
                                            &plaintext,
                                            &[],
                                        )?;
                                        let encrypted_msg =
                                            Phase1Message::EncryptedMessage { payload: encrypted };
                                        let bytes = protocol::serialize(&encrypted_msg)?;
                                        socket.send_to(&bytes, peer_addr).await?;
                                        info!("Sent encrypted JoinRequest to {}", peer_id);

                                        // Update state to join requested (re-acquire lock)
                                        let mut peers_lock = peers.write().await;
                                        if let Some(peer_conn) = peers_lock.get_mut(&peer_id) {
                                            peer_conn.state = ConnectionState::JoinRequested;
                                            info!("Set state to JoinRequested for {}", peer_id);
                                        }
                                    } else {
                                        // Server side: Authentication complete.
                                        info!("Server authenticated client {}, waiting for JoinRequest", peer_id);
                                    }
                                }
                                Err(e) => {
                                    error!("HMAC verification failed for {}: {}", peer_id, e);
                                    peer_conn.state = ConnectionState::None; // Reset state on failure?
                                    drop(peers_lock); // Release lock before await
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
                        } else {
                            warn!(
                                "Received AuthTag from {} but missing keys/state for verification. State: {:?}, Has HMAC Key: {}, Has Our Key: {}, Has Peer Key: {}",
                                peer_id,
                                peer_conn.state,
                                peer_conn.hmac_key.is_some(),
                                peer_conn.our_public_key.is_some(),
                                peer_conn.peer_public_key.is_some()
                            );
                            // Should we change state here? Maybe back to HelloExchanged?
                        }
                    }
                } else {
                    warn!("Received AuthTag from unknown address: {}", addr);
                }
            }

            Phase1Message::EncryptedMessage { payload } => {
                if let Some(connection_peer_id) = incoming_peer_id_opt {
                    let peers_lock = peers.read().await;
                    if let Some(peer_conn) = peers_lock.get(&connection_peer_id) {
                        // Check state *before* logging potentially sensitive info
                        if peer_conn.state == ConnectionState::Authenticated
                            || peer_conn.state == ConnectionState::Joined
                            || peer_conn.state == ConnectionState::JoinRequested
                        {
                            info!(
                                "Received EncryptedMessage from {} (state: {:?}), attempting decryption...",
                                connection_peer_id,
                                peer_conn.state
                            );
                            // Decrypt the message
                            if let Some(decryption_key) = &peer_conn.decryption_key {
                                match CryptoProvider::decrypt(decryption_key, &payload, &[]) {
                                    Ok(plaintext) => {
                                        info!(
                                            "Decryption successful for message from {}",
                                            connection_peer_id
                                        );
                                        // Deserialize the decrypted message
                                        match protocol::deserialize(&plaintext) {
                                            Ok(inner_message) => {
                                                info!(
                                                    "Deserialized inner message from {}: {:?}",
                                                    connection_peer_id, inner_message
                                                );
                                                // Drop read lock before potentially acquiring write lock or sending event
                                                drop(peers_lock);

                                                // Process the inner message
                                                match inner_message {
                                                    Phase1Message::JoinRequest {
                                                        peer_id: requesting_peer_id,
                                                        name,
                                                    } => {
                                                        info!(
                                                            "Processing JoinRequest from {}",
                                                            requesting_peer_id
                                                        );
                                                        let event = NetworkEvent::JoinRequested {
                                                            peer_id: requesting_peer_id,
                                                            name,
                                                            address: addr,
                                                        };
                                                        event_sender.send(event).await.map_err(|e| Error::Network(format!("Failed to send JoinRequested event: {}", e)))?
                                                    }
                                                    Phase1Message::JoinResponse {
                                                        approved,
                                                        reason,
                                                    } => {
                                                        info!("Processing JoinResponse (approved: {}) from {}", approved, connection_peer_id);
                                                        // Update peer state if approved
                                                        if approved {
                                                            let mut peers_lock =
                                                                peers.write().await;
                                                            if let Some(peer_conn) = peers_lock
                                                                .get_mut(&connection_peer_id)
                                                            {
                                                                peer_conn.state =
                                                                    ConnectionState::Joined;
                                                                info!(
                                                                    "Set state to Joined for {}",
                                                                    connection_peer_id
                                                                );
                                                            }
                                                        }
                                                        // Notify about join response
                                                        let event =
                                                            NetworkEvent::JoinResponseReceived {
                                                                approved,
                                                                reason,
                                                            };
                                                        event_sender.send(event).await.map_err(|e| Error::Network(format!("Failed to send JoinResponseReceived event: {}", e)))?;
                                                    }
                                                    Phase1Message::Ping {
                                                        peer_id: pinging_peer,
                                                    } => {
                                                        info!(
                                                            "Processing Ping from {}",
                                                            pinging_peer
                                                        );
                                                        // Send Pong
                                                        let pong = Phase1Message::Pong {
                                                            peer_id: self_peer_id,
                                                        };
                                                        let bytes = protocol::serialize(&pong)?;
                                                        let mut pong_sent = false;
                                                        {
                                                            // Re-acquire read lock to get encryption key and address
                                                            let peers_lock = peers.read().await;
                                                            if let Some(peer_conn) =
                                                                peers_lock.get(&connection_peer_id)
                                                            {
                                                                if let Some(encryption_key) =
                                                                    &peer_conn.encryption_key
                                                                {
                                                                    let encrypted_pong_payload =
                                                                        CryptoProvider::encrypt(
                                                                            encryption_key,
                                                                            &bytes,
                                                                            &[],
                                                                        )?;
                                                                    let encrypted_pong_msg = Phase1Message::EncryptedMessage { payload: encrypted_pong_payload };
                                                                    let final_bytes =
                                                                        protocol::serialize(
                                                                            &encrypted_pong_msg,
                                                                        )?;
                                                                    let target_addr =
                                                                        peer_conn.info.address;
                                                                    drop(peers_lock);
                                                                    socket
                                                                        .send_to(
                                                                            &final_bytes,
                                                                            target_addr,
                                                                        )
                                                                        .await?;
                                                                    info!("Sent encrypted Pong to {} in response to Ping", connection_peer_id);
                                                                    pong_sent = true;
                                                                } else {
                                                                    warn!("Cannot send Pong to {}: Missing encryption key", connection_peer_id);
                                                                }
                                                            } else {
                                                                warn!("Cannot send Pong to {}: Peer disappeared", connection_peer_id);
                                                            }
                                                        }
                                                        // Also forward the original Ping message event
                                                        if pong_sent {
                                                            // Only forward if we successfully sent Pong
                                                            let event =
                                                                NetworkEvent::MessageReceived {
                                                                    peer_id: connection_peer_id,
                                                                    message: Phase1Message::Ping {
                                                                        peer_id: pinging_peer,
                                                                    }, // Reconstruct Ping
                                                                };
                                                            event_sender.send(event).await.map_err(|e| Error::Network(format!("Failed to send Ping MessageReceived event: {}", e)))?;
                                                            info!("Sent MessageReceived event for Ping from {}", pinging_peer);
                                                        }
                                                    }
                                                    Phase1Message::Pong {
                                                        peer_id: ponging_peer,
                                                    } => {
                                                        info!(
                                                            "Processing Pong from {}",
                                                            ponging_peer
                                                        );
                                                        let mut valid_pong = false;
                                                        {
                                                            // Handle Pong (update last received time)
                                                            let mut peers_lock =
                                                                peers.write().await;
                                                            if let Some(peer_conn) = peers_lock
                                                                .get_mut(&connection_peer_id)
                                                            {
                                                                if peer_conn.info.peer_id
                                                                    == ponging_peer
                                                                {
                                                                    peer_conn
                                                                        .update_received_time();
                                                                    info!("Updated last received time for {}", ponging_peer);
                                                                    valid_pong = true;
                                                                } else {
                                                                    warn!("Pong inner peer ID {} does not match connection ID {}", ponging_peer, connection_peer_id);
                                                                }
                                                            } else {
                                                                warn!("Received Pong for non-existent peer connection {}", connection_peer_id);
                                                            }
                                                        }

                                                        if valid_pong {
                                                            // Forward the Pong message event
                                                            let event =
                                                                NetworkEvent::MessageReceived {
                                                                    peer_id: connection_peer_id, // ID of the peer who sent the encrypted message
                                                                    message: Phase1Message::Pong {
                                                                        peer_id: ponging_peer,
                                                                    }, // Reconstruct the Pong message
                                                                };
                                                            event_sender.send(event).await.map_err(|e| Error::Network(format!("Failed to send Pong MessageReceived event: {}", e)))?;
                                                            info!("Sent MessageReceived event for Pong from {}", ponging_peer);
                                                        }
                                                    }
                                                    // Handle other inner message types...
                                                    _ => {
                                                        info!(
                                                            "Forwarding other message type from {}",
                                                            connection_peer_id
                                                        );
                                                        let event = NetworkEvent::MessageReceived {
                                                            peer_id: connection_peer_id,
                                                            message: inner_message,
                                                        };
                                                        event_sender.send(event).await.map_err(|e| Error::Network(format!("Failed to send MessageReceived event: {}", e)))?;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to deserialize decrypted message from {}: {}",
                                                    connection_peer_id, e
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to decrypt message from {}: {}", addr, e);
                                    }
                                }
                            } else {
                                warn!(
                                    "Received EncryptedMessage from {}, but missing decryption key",
                                    connection_peer_id
                                );
                            }
                        } else {
                            warn!(
                                "Received EncryptedMessage from {} but peer state is {:?}, ignoring",
                                connection_peer_id,
                                peer_conn.state
                            );
                        }
                    }
                } else {
                    warn!("Received EncryptedMessage from unknown address: {}", addr);
                }
            }
            // Catch-all for other unencrypted message types received unexpectedly
            other => {
                warn!(
                    "Received unexpected unencrypted message type {:?} from {}. Ignoring.",
                    other, addr
                );
            }
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
