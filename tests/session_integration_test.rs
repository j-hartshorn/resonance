use resonance::app::session::{Peer, Session, SessionManager};
use resonance::network::p2p::Endpoint;
use resonance::ui::Participant;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn test_session_peer_integration() {
    // Create a test session based on existing test pattern
    let _session = Session {
        id: "test-session".to_string(),
        connection_link: "test-link".to_string(),
        participants: vec![Participant::new("Me")],
        is_host: true,
        original_host_id: "host-id".to_string(),
        created_at: 0,
    };

    // Create a session manager
    let session_manager = SessionManager::new();

    // Verify initial state
    assert!(session_manager.current_session().is_none());

    // Create a peer with the required fields
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let _peer = Peer {
        id: "peer-1".to_string(),
        name: "Test Peer".to_string(),
        endpoint: Endpoint {
            ip: "127.0.0.1".parse().unwrap(),
            port: 8080,
        },
        public_key: [0u8; 32],
        position: (0.0, 0.0, 0.0),
        is_host: false,
        joined_at: now,
    };

    // Test has_active_connection - should be false initially
    assert!(!session_manager.has_active_connection().await);

    // Test cloning the session manager
    let cloned_manager = session_manager.clone();

    // Verify the cloned manager has no session
    assert!(cloned_manager.current_session().is_none());
}
