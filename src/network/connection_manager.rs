use anyhow::{anyhow, Result};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    Mutex,
};
use tokio::task::JoinHandle;

use super::p2p::{establish_direct_udp_connection, ConnectionState};
use super::secure_channel::{Message, SecureChannel};

/// Manages connections to remote peers
pub struct ConnectionManager {
    /// Connection details for reconnection
    remote_ip: IpAddr,
    remote_port: u16,
    session_id: String,
    remote_key: [u8; 32],

    /// Primary secure channel
    channel: Arc<Mutex<Option<SecureChannel>>>,

    /// Connection state
    state: Arc<Mutex<ConnectionState>>,

    /// Heartbeat timer
    last_heartbeat: Arc<Mutex<Instant>>,

    /// Message queue for outgoing messages
    message_tx: Sender<Message>,
    message_rx: Arc<Mutex<Option<Receiver<Message>>>>,

    /// Background tasks
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(
        remote_ip: IpAddr,
        remote_port: u16,
        session_id: String,
        remote_key: [u8; 32],
    ) -> Self {
        let (tx, rx) = mpsc::channel(100);

        Self {
            remote_ip,
            remote_port,
            session_id,
            remote_key,
            channel: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            last_heartbeat: Arc::new(Mutex::new(Instant::now())),
            message_tx: tx,
            message_rx: Arc::new(Mutex::new(Some(rx))),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Connect to the remote peer
    pub async fn connect(&self) -> Result<()> {
        // Update state
        let mut state = self.state.lock().await;
        *state = ConnectionState::Connecting;
        drop(state);

        // Establish UDP connection
        let socket = establish_direct_udp_connection(self.remote_ip, self.remote_port).await?;

        // Create secure channel
        let remote_addr = SocketAddr::new(self.remote_ip, self.remote_port);
        let mut channel = SecureChannel::new(socket, remote_addr).await;

        // Set session ID
        channel.session_id = self.session_id.clone();

        // Perform key exchange
        channel.perform_key_exchange(Some(self.remote_key)).await?;

        // Store channel
        let mut channel_guard = self.channel.lock().await;
        *channel_guard = Some(channel);
        drop(channel_guard);

        // Update state
        let mut state = self.state.lock().await;
        *state = ConnectionState::Connected;
        drop(state);

        // Start background tasks
        self.start_background_tasks();

        Ok(())
    }

    /// Start background tasks for heartbeats and reconnection
    fn start_background_tasks(&self) {
        let heartbeat_task = self.start_heartbeat_task();
        let reconnect_task = self.start_reconnect_task();
        let message_task = self.start_message_task();

        let tasks_clone = self.tasks.clone();
        tokio::spawn(async move {
            let mut tasks = tasks_clone.lock().await;
            tasks.push(heartbeat_task);
            tasks.push(reconnect_task);
            tasks.push(message_task);
        });
    }

    /// Start heartbeat task
    fn start_heartbeat_task(&self) -> JoinHandle<()> {
        let state_clone = self.state.clone();
        let channel_clone = self.channel.clone();
        let last_heartbeat_clone = self.last_heartbeat.clone();

        tokio::spawn(async move {
            loop {
                // Sleep for a short time
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Check if we need to send a heartbeat
                let mut needs_heartbeat = false;
                {
                    let now = Instant::now();
                    let mut last = last_heartbeat_clone.lock().await;
                    if now.duration_since(*last) > Duration::from_secs(5) {
                        needs_heartbeat = true;
                        *last = now;
                    }
                }

                if needs_heartbeat {
                    let state_guard = state_clone.lock().await;
                    let current_state = state_guard.clone();
                    drop(state_guard);

                    if current_state == ConnectionState::Connected {
                        // Try to send heartbeat
                        let channel_guard = channel_clone.lock().await;
                        if let Some(ref channel) = *channel_guard {
                            let result = channel.send_heartbeat().await;
                            drop(channel_guard);

                            if let Err(e) = result {
                                eprintln!("Heartbeat failed: {}", e);
                                let mut state = state_clone.lock().await;
                                *state = ConnectionState::Connecting;
                            }
                        }
                    }
                }
            }
        })
    }

    /// Start reconnection task
    fn start_reconnect_task(&self) -> JoinHandle<()> {
        let state_clone = self.state.clone();
        let channel_clone = self.channel.clone();
        let remote_ip = self.remote_ip;
        let remote_port = self.remote_port;
        let session_id = self.session_id.clone();
        let remote_key = self.remote_key;

        tokio::spawn(async move {
            loop {
                // Sleep for a short time
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Check if we need to reconnect
                let state_guard = state_clone.lock().await;
                let current_state = state_guard.clone();
                drop(state_guard);

                if current_state == ConnectionState::Connecting {
                    // Try to reconnect
                    match establish_direct_udp_connection(remote_ip, remote_port).await {
                        Ok(socket) => {
                            // Create new secure channel
                            let remote_addr = SocketAddr::new(remote_ip, remote_port);
                            let mut new_channel = SecureChannel::new(socket, remote_addr).await;
                            new_channel.session_id = session_id.clone();

                            // Try to perform key exchange
                            match new_channel.perform_key_exchange(Some(remote_key)).await {
                                Ok(_) => {
                                    // Reconnection successful
                                    {
                                        let mut channel = channel_clone.lock().await;
                                        *channel = Some(new_channel);
                                    }

                                    {
                                        let mut state = state_clone.lock().await;
                                        *state = ConnectionState::Connected;
                                    }

                                    println!("Reconnected to peer");
                                }
                                Err(e) => {
                                    eprintln!("Key exchange failed during reconnection: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Reconnection attempt failed: {}", e);
                        }
                    }
                }
            }
        })
    }

    /// Start message processing task
    fn start_message_task(&self) -> JoinHandle<()> {
        let state_clone = self.state.clone();
        let channel_clone = self.channel.clone();
        let message_rx = self.message_rx.clone();

        tokio::spawn(async move {
            // Take ownership of the receiver
            let rx = {
                let mut rx_guard = message_rx.lock().await;
                rx_guard.take()
            };

            let Some(mut rx) = rx else { return };

            loop {
                // Wait for a message
                let message = match rx.recv().await {
                    Some(msg) => msg,
                    None => break,
                };

                // Try to send the message
                let mut retry_count = 0;
                let max_retries = 3;

                loop {
                    // Get current state
                    let state_guard = state_clone.lock().await;
                    let current_state = state_guard.clone();
                    drop(state_guard);

                    if current_state == ConnectionState::Connected {
                        // Check if we have a channel
                        let channel_guard = channel_clone.lock().await;
                        let has_channel = channel_guard.is_some();

                        if has_channel {
                            // Get channel reference
                            let channel = channel_guard.as_ref().unwrap();
                            let result = channel.send(&message).await;
                            drop(channel_guard);

                            match result {
                                Ok(_) => break, // Message sent successfully
                                Err(e) => {
                                    eprintln!("Failed to send message: {}", e);
                                    retry_count += 1;

                                    if retry_count >= max_retries {
                                        // Give up after max retries
                                        eprintln!("Giving up after {} retries", max_retries);
                                        break;
                                    }

                                    // Mark as connecting for reconnection
                                    let mut state = state_clone.lock().await;
                                    *state = ConnectionState::Connecting;
                                    drop(state);

                                    // Wait for reconnection
                                    tokio::time::sleep(Duration::from_secs(1)).await;
                                }
                            }
                        } else {
                            drop(channel_guard);

                            // No channel, wait for reconnection
                            let mut state = state_clone.lock().await;
                            *state = ConnectionState::Connecting;
                            drop(state);

                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    } else {
                        // Not connected, wait for connection
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        })
    }

    /// Send a message reliably
    pub async fn send_reliable(&self, message: Message) -> Result<()> {
        self.message_tx
            .send(message)
            .await
            .map_err(|_| anyhow!("Failed to queue message"))
    }

    /// Send audio data reliably
    pub async fn send_audio(&self, audio_data: &[u8], timestamp: u64) -> Result<()> {
        let message = Message::Audio {
            data: audio_data.to_vec(),
            timestamp,
        };

        self.send_reliable(message).await
    }

    /// Listen for incoming messages
    pub async fn start_listening<F>(&self, mut handler: F) -> JoinHandle<()>
    where
        F: FnMut(Message) -> Result<()> + Send + 'static,
    {
        let channel_clone = self.channel.clone();

        tokio::spawn(async move {
            loop {
                // Check if we have a channel
                let channel_guard = channel_clone.lock().await;
                let current_socket = match &*channel_guard {
                    Some(ch) => Some(ch.clone_socket()),
                    None => None,
                };
                drop(channel_guard);

                if let Some(socket) = current_socket {
                    // We have a channel, try to receive
                    let mut buf = [0u8; 65536];
                    match socket.recv_from(&mut buf).await {
                        Ok((size, addr)) => {
                            // Process the packet
                            let channel_guard = channel_clone.lock().await;
                            if let Some(ref ch) = *channel_guard {
                                if addr == ch.remote_addr() {
                                    // TODO: Process the packet
                                }
                            }
                            drop(channel_guard);
                        }
                        Err(e) => {
                            eprintln!("Error receiving: {}", e);
                        }
                    }
                } else {
                    // No channel, wait for connection
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        })
    }

    /// Get the current connection state
    pub async fn connection_state(&self) -> ConnectionState {
        let state = self.state.lock().await;
        state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = ConnectionManager::new(
            "192.0.2.1".parse().unwrap(),
            12345,
            "test-session".to_string(),
            [0u8; 32],
        );

        assert_eq!(
            manager.connection_state().await,
            ConnectionState::Disconnected
        );
    }
}
