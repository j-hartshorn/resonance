// Resonance: P2P Spatial Audio Platform
// Expose public modules for use in integration tests

pub mod app;
pub mod audio;
pub mod network;
pub mod ui;

// Re-export commonly used types for convenience
pub use app::session::{Peer, Session, SessionError, SessionManager};
pub use app::App;
pub use audio::{SpatialAudioProcessor, VoiceProcessor};
pub use network::connection_manager::ConnectionManager;
pub use network::p2p::{ConnectionState, Endpoint};
pub use ui::Participant;
