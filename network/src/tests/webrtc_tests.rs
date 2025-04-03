use crate::protocol::{ApplicationMessage, Phase1Message};
use crate::webrtc_if::WebRtcInterface;
use log::debug;
use room_core::{NetworkEvent, PeerId};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

#[tokio::test]
async fn test_webrtc_interface_creation() {
    let peer_id = PeerId::new();
    let (phase1_tx, _) = mpsc::channel(100);
    let (event_tx, _) = mpsc::channel(100);

    let stun_servers = vec!["stun:stun.l.google.com:19302".to_string()];

    let webrtc_if = WebRtcInterface::new(peer_id, phase1_tx, event_tx, stun_servers);

    // Create a peer connection
    let other_peer_id = PeerId::new();
    let pc = webrtc_if
        .create_peer_connection(other_peer_id)
        .await
        .unwrap();

    // connection_state() returns the state directly, not a Future
    let state = pc.connection_state();
    assert_eq!(state, RTCPeerConnectionState::New);

    // Create a data channel
    let dc = webrtc_if
        .create_data_channel(other_peer_id, "test")
        .await
        .unwrap();
    assert_eq!(dc.label(), "test");
}

#[tokio::test]
async fn test_webrtc_offer_creation() {
    let peer_id = PeerId::new();
    let (phase1_tx, mut phase1_rx) = mpsc::channel(100);
    let (event_tx, _) = mpsc::channel(100);

    let stun_servers = vec!["stun:stun.l.google.com:19302".to_string()];

    let webrtc_if = WebRtcInterface::new(peer_id, phase1_tx, event_tx, stun_servers);

    // Create a peer connection
    let other_peer_id = PeerId::new();
    let _pc = webrtc_if
        .create_peer_connection(other_peer_id)
        .await
        .unwrap();

    // Create an offer
    webrtc_if.create_offer(other_peer_id).await.unwrap();

    // We should receive a Phase1Message with an SDP offer
    let timeout_dur = Duration::from_secs(2);
    match timeout(timeout_dur, phase1_rx.recv()).await {
        Ok(Some((peer_id, message))) => {
            assert_eq!(peer_id, other_peer_id);

            // Check that the message contains an SDP offer
            if let Phase1Message::ApplicationMessage { message: app_msg } = message {
                if let ApplicationMessage::SdpOffer { offer } = app_msg {
                    // Verify that the offer looks like an SDP (starts with v=0)
                    assert!(offer.starts_with("v=0"));
                    debug!("Received valid SDP offer: {}", offer);
                } else {
                    panic!("Expected SdpOffer, got different application message");
                }
            } else {
                panic!("Expected ApplicationMessage, got {:?}", message);
            }
        }
        Ok(None) => panic!("phase1_rx closed unexpectedly"),
        Err(_) => panic!("Timeout waiting for SDP offer"),
    }
}
