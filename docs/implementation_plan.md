# Implementation Plan: room.rs

This plan outlines a step-by-step approach for implementing the `room.rs` application, emphasizing Test-Driven Development (TDD) principles.

## Phase 0: Project Setup & Core Types

### Action
1.  **Initialize Project:** Set up a Rust workspace and the following member crates: `app_cli`, `room_core`, `audio`, `audio_io`, `spatial`, `network`, `crypto`, `room`, `visualization`, `settings_manager`.
2.  **Define Core Types (`room_core` crate):**
    * `PeerId` struct/type alias.
    * `RoomID` struct/type alias.
    * Unified `Error` enum using `thiserror`.
    * Basic audio buffer format definitions (sample rate, channels, `f32`).
3.  **Add Core Dependencies:** Include `tokio`, `serde`, `thiserror`, `log` facade in relevant `Cargo.toml` files.

### TDD
* **Unit Test (`room_core`):** Verify methods/impls on core types (`Display`, `Error` source chaining, etc.).

### Integration Testing
* N/A

## Phase 1: Configuration & Basic CLI Shell

### Action
1.  **Implement `settings_manager` Crate:**
    * Define `Settings` struct (`serde` derive) for username, audio device preferences (initially strings/defaults), STUN/TURN list.
    * Implement loading from TOML (`config.toml`), handling file-not-found (use defaults).
    * Implement saving `Settings` to TOML.
2.  **Implement Basic TUI Shell (`app_cli`):**
    * Set up `tokio` runtime in `main`.
    * Initialize `ratatui` backend/terminal.
    * Create basic TUI layout (placeholders for Menu, Peers, Viz).
    * Implement basic event loop (handle quit key).
    * Render static placeholder content.

### TDD
* **Unit Test (`settings_manager`):** Test loading defaults, valid TOML, handling invalid TOML, save/reload roundtrip.
* **Unit Test (`app_cli`):** Mock terminal backend. Test layout rendering with placeholders. Test quit event handling.

### Integration Testing
* N/A

## Phase 2: Crypto Primitives

### Action
1.  **Implement `crypto` Crate Functions:**
    * Ephemeral Diffie-Hellman key generation (e.g., x25519).
    * Shared secret computation.
    * Key Derivation Function (KDF, e.g., HKDF-SHA256) using DH secret + link key.
    * Authenticated Encryption with Associated Data (AEAD) interface (e.g., ChaCha20-Poly1305 `encrypt`/`decrypt`).
    * HMAC function (e.g., HMAC-SHA256) for DH exchange authentication.

### TDD
* **Unit Test (`crypto`):** Test key generation. Test shared secret computation against known vectors. Test KDF against known vectors. Test AEAD roundtrip and tamper detection. Test HMAC against known vectors. Rely on vetted underlying crypto libraries.

### Integration Testing
* N/A

## Phase 3: Phase 1 Networking (Bootstrap & Secure Channel)

### Action
1.  **Define Protocol (`network::protocol`):**
    * Define structs/enums for Phase 1 messages (`HelloInitiate`, `HelloAck`, `DHPubKey`, `AuthTag`, `EncryptedMessage`, `JoinRequest`, etc.) using `serde`.
    * Choose and implement serialization (e.g., `bincode`, `postcard`).
2.  **Implement Basic Sockets (`network::phase1`):**
    * Set up basic `tokio::net::UdpSocket` send/receive logic.
3.  **Implement Authenticated Handshake (`network::phase1` using `crypto`):**
    * (Driven by Tests below) Implement logic for exchanging `Hello*`, `DHPubKey`, `AuthTag`.
    * Implement validation of `RoomID`, `AuthTag`.
    * Implement computation of shared secrets and derivation of session keys using `crypto`.
    * Implement logic to transition to AEAD-encrypted communication state.
4.  **Implement Secure Channel Transport (`network::phase1` using `crypto`):**
    * Implement functions to send/receive AEAD-encrypted messages using derived session keys.
    * (Optional: Basic reliability over UDP if needed - keep simple).
5.  **Define `NetworkEvent` Enum:** Define events like `PeerConnected(PeerId)`, `MessageReceived(PeerId, Message)`, `PeerDisconnected(PeerId)`.

### TDD
* **Unit Test (`network::protocol`):** Test serialization/deserialization roundtrip for all messages. Test handling known good/bad byte streams.
* **Unit Test (`network::phase1`):** Test basic socket operations (mocked/loopback).
* **Unit Test (`network::phase1` Handshake Logic):**
    * Test client initiation sending `HelloInitiate`+`DHPubKey`.
    * Test server response logic (`HelloAck`+`DHPubKey`+`AuthTag`) using mock `crypto`.
    * Test client validation logic (`AuthTag`) and key derivation using mock `crypto`.
    * Test server key derivation logic using mock `crypto`.
* **Unit Test (`network::phase1` Secure Channel):**
    * Test sending/receiving AEAD encrypted messages after handshake using mock `crypto`. Test decryption failure on bad key/tampered data.

### Integration Testing
* **Integration Test (`network`):** Test two local `network` instances completing the full Phase 1 handshake and key exchange. Test sending/receiving an encrypted Phase 1 message between them over loopback.

## Phase 4: Room State Management

### Action
1.  **Implement `room` Crate Logic:**
    * Define `RoomState` struct (peer map, `RoomID`, pending joins map).
    * Implement state modification functions (`add_peer`, `remove_peer`, `handle_join_request`, `approve_join_request`, `deny_join_request`). Enforce max users (8).

### TDD
* **Unit Test (`room`):** Test initial state. Test add/remove peer logic. Test join request handling (adds to pending). Test approval/denial logic (updates peer state, checks limits). Test all state transitions thoroughly based on simulated inputs/events.

### Integration Testing
* N/A

## Phase 5: Basic TUI Interaction & Orchestration

### Action
1.  **Setup Communication Channels:** Define `tokio::sync::mpsc` channels between `app_cli`, `network`, `room` for events and commands.
2.  **Connect UI Actions (`app_cli` -> `room` / `network`):**
    * Implement TUI menu actions (Create, Join via Link).
    * Parse link input. Trigger `network::connect` or room creation logic.
    * Implement display of Join Requests in TUI (receiving events from `room`).
    * Implement Approve/Deny UI actions sending commands to `room`.
3.  **Connect Network Events (`network` -> `room`):**
    * Implement task in `network` to receive `NetworkEvent`s (e.g., `MessageReceived`, `PeerDisconnected`).
    * Forward relevant events (e.g., `JoinRequest` message) to `room` via channel.
4.  **Connect Room Commands (`room` -> `network`):**
    * Implement task in `network` to receive commands from `room` (e.g., `SendApprovalMessage`).
    * Execute commands (e.g., construct and send encrypted Phase 1 message).
5.  **Update TUI Display (`room` -> `app_cli`):**
    * `room` emits state update events (e.g., `PeerListUpdated`) via channel.
    * `app_cli` receives state updates and refreshes the TUI display (Peer List widget, status messages).

### TDD
* **Unit Test (`app_cli`):** Mock `network`, `room`, channels. Test UI actions trigger correct mock calls/channel sends. Test TUI updates correctly based on simulated events received on channels.
* **Unit Test (`network`):** Test event handling task logic (receives `NetworkEvent`, sends to `room` channel). Test command handling task logic (receives command, sends Phase 1 message).
* **Unit Test (`room`):** Test that state changes correctly emit events/commands via mocked channels.

### Integration Testing
* **Integration Test (`app_cli` <-> `room`):** Test UI action -> `room` state change -> TUI update flow using real channels.
* **Integration Test (`network` <-> `room`):** Test `network` receiving Phase 1 message -> `room` state update flow using real channels.
* **Integration Test (`room` -> `network`):** Test `room` command -> `network` sending Phase 1 message flow using real channels (mock peer/network endpoint).
* **Manual E2E Test:** Two instances connect via link, complete Phase 1 handshake, Join Request/Approval flow. Verify TUI state (Peer List) is consistent on both ends.

## Phase 6: WebRTC Integration & Signaling

### Action
1.  **Add Dependency:** Integrate `webrtc-rs`.
2.  **Implement `network::webrtc_if` Module:**
    * Wrap `webrtc-rs` API (`RTCPeerConnection`, etc.).
    * Implement functions: `create_offer`, `create_answer`, `handle_remote_description`, `add_ice_candidate`, `handle_ice_candidate`.
    * Set up `webrtc-rs` event handlers (`onicecandidate`, `ondatachannel`, `ontrack`, `onconnectionstatechange`).
3.  **Connect Signaling (`network::phase1` <-> `network::webrtc_if`):**
    * Trigger `create_offer` from `webrtc_if` when needed (e.g., post-approval).
    * Translate `webrtc-rs` events (`onicecandidate`, generated SDP) into `network::protocol` messages (`IceCandidate`, `SdpOffer`/`Answer`) and send via secure Phase 1 channel.
    * Receive `SdpOffer`/`Answer`/`IceCandidate` messages via Phase 1 channel, parse using `network::protocol`, call corresponding `webrtc_if.handle_remote_...` methods.

### TDD
* **Unit Test (`network::webrtc_if`):** Mock `webrtc-rs` API. Test state transitions based on function calls and simulated events.
* **Unit Test (`network` Signaling Logic):** Test the translation logic between `webrtc-rs` events/calls and Phase 1 messages, using mocks for `webrtc-rs` and Phase 1 channel.

### Integration Testing
* **Integration Test (`network`):** Two peers complete the full WebRTC signaling handshake using the secure Phase 1 channel implemented in Phase 3. Verify `webrtc-rs` connection state changes to `Connected` via `onconnectionstatechange`.

## Phase 7: Basic Audio Pipeline (No Spatial)

### Action
1.  **Implement `audio::audio_io`:**
    * Use `cpal` to list devices, capture PCM audio to a channel (dedicated thread).
    * Use `cpal` to play PCM audio received from a channel (dedicated thread).
2.  **Connect Capture to WebRTC:**
    * In `network::webrtc_if` (or related task), create audio track (`RTCRtpSender`).
    * Read PCM buffers from `audio_io` capture channel, pass to `webrtc-rs` track for encoding/sending.
3.  **Connect WebRTC to Playback:**
    * Handle `webrtc-rs` `ontrack` event for incoming audio.
    * Get decoded PCM buffers from the track (`RTCRtpReceiver`).
    * Send PCM buffers to `audio_io` playback channel.

### TDD
* **Unit Test (`audio::audio_io`):** Mock `cpal` API. Test starting/stopping loops, channel communication. (Device interaction may need manual test).
* **Unit Test (Capture->WebRTC):** Test task reading from capture channel and calling mock `webrtc-rs` send function.
* **Unit Test (WebRTC->Playback):** Test task handling mock `ontrack` event and sending mock buffers to playback channel.

### Integration Testing
* **Manual E2E Test:** Two peers connect, establish WebRTC audio. Peer A speaks, Peer B hears raw audio (verify latency is reasonable). Vice versa.

## Phase 8: Spatial Audio Integration

### Action
1.  **Add Dependency:** Integrate `AudioNimbus`.
2.  **Implement `audio::spatial` Mixer Thread:**
    * Create task/thread receiving decoded PCM buffers (from `network::webrtc_if`) via multiple per-peer channels.
    * Initialize `AudioNimbus` context and sources. Assign static positions.
    * In loop: process incoming buffers using `AudioNimbus` API, get mixed stereo output buffer.
    * Send mixed buffer to `audio_io` playback channel.
3.  **Reroute Audio Flow:** Change `network::webrtc_if` to send received audio to `audio::spatial` input channels instead of directly to `audio::io`.

### TDD
* **Unit Test (`audio::spatial`):** Mock `AudioNimbus` API. Test receiving from multiple channels, calling mock mixing logic, sending to output channel. Test `AudioNimbus` API usage directly if possible.

### Integration Testing
* **Manual E2E Test:** Three+ peers connect. Verify Peer A and B voices sound spatially distinct to Peer C. Requires listening test.

## Phase 9: Visualization

### Action
1.  **Implement `visualization` Crate:**
    * Add FFT library dependency (e.g., `rustfft`).
    * Implement function: takes audio buffer -> performs FFT -> calculates frequency bin averages -> returns `Vec<f32>` (or similar).
2.  **Feed Audio Data:** Tap into the mixed audio buffer (e.g., just before playback channel) and send copies to `visualization` via a channel.
3.  **Connect to TUI (`app_cli`):**
    * `app_cli` receives visualization data via channel.
    * Implement a `ratatui` widget to render the spectrogram based on received data.

### TDD
* **Unit Test (`visualization`):** Test FFT and averaging logic with known input signals (sine waves, etc.).
* **Unit Test (`app_cli`):** Test spectrogram widget rendering logic with sample data.

### Integration Testing
* **Manual E2E Test:** Speak into mic, verify audio plays and spectrogram widget updates dynamically in the TUI.

## Phase 10: Refinements & Hardening

### Action
1.  **Implement Room History & Rejoin:** Store last `RoomID`/peer info in `settings_manager`, add "Rejoin" option in `app_cli` triggering connection via `network`.
2.  **Implement Settings Menu:** Create TUI screens in `app_cli` to view/modify settings stored via `settings_manager`.
3.  **Implement Peer Discovery/Healing:** `room` logic periodically instructs `network` (via command channel) to send "request peer list" messages; `network` handles responses, informs `room` of discrepancies.
4.  **Implement Fuzz Testing:** Set up `cargo fuzz` targets for `network::protocol` parsing, link parsing. Run regularly.
5.  **Comprehensive Error Handling:** Review error propagation (`core::Error`), ensure graceful handling and user feedback in TUI for network disconnects, crypto errors, device errors etc.
6.  **Performance Testing:** Profile CPU/memory usage with max users (8). Optimize audio pipeline latency if needed.
7.  **Documentation & Cleanup:** Write `README.md`, code comments, refactor where needed.

### TDD
* **Unit/Integration Tests:** Add tests for History/Rejoin logic, Settings menu interactions, Peer Discovery message handling/state updates.
* **Fuzz Testing:** Integrate into CI/testing process. Fix discovered issues.
* **Error Injection Tests:** Modify tests to simulate errors (network disconnects, decryption failures) and verify robust handling.

### Integration Testing
* **Manual E2E Tests:** Test all features comprehensively, including rejoin, settings changes, peer discovery under simulated churn. Test performance under load.