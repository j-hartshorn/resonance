# Project Brief: room.rs - P2P Spatial Audio CLI Chat

## 1. Introduction / Vision

`room.rs` is envisioned as a command-line (CLI) application, built in Rust, enabling small groups of users (up to 8) to communicate via high-quality, low-latency, end-to-end encrypted audio over the internet. The application will operate on a fully peer-to-peer (P2P) basis after initial connection bootstrapping. A key feature will be the integration of spatial audio processing (using Steam Audio via `AudioNimbus`), allowing users to perceive the voices of participants as originating from distinct locations in a virtual space, enhancing clarity and immersion in group conversations. The user interface will be a Terminal User Interface (TUI).

## 2. Core Features (Functional Requirements)

* **P2P Audio Streaming:** Low-latency, high-quality audio streams transmitted directly between peers.
* **Spatial Audio:** Utilize binaural audio processing (Steam Audio via `AudioNimbus`) on the receiving end to arrange participant voices in a simulated 3D space for improved voice separation.
* **Room-Based Communication:** Communication occurs within persistent "Rooms".
* **Room Management:**
    * Users can create new rooms.
    * Rooms are identified by a unique `RoomID`.
    * Maximum of 8 users per room.
* **Distributed Join Mechanism:**
    * Any user currently in a room can generate a unique join link containing their connection details and the current `RoomID`.
    * New users join by providing a link obtained from an existing room member.
* **Join Request/Approval:**
    * When using a link, the joining user connects to the peer specified in the link and sends a "join request".
    * The peer who provided the link receives the request via the TUI and can approve or deny entry into the room.
    * Room members are notified when a new user joins and who approved them.
* **End-to-End Encryption (E2EE):**
    * Initial link verification using a key embedded within the link.
    * Secure P2P channel establishment for signaling.
    * All audio/data communication between peers must be encrypted end-to-end using standardized protocols (DTLS/SRTP via WebRTC).
* **CLI TUI Interface:**
    * Built using `ratatui`.
    * Main areas:
        * Dynamic Menu/Status Area: Create/Join Room, Settings, status messages, join request notifications.
        * Peer List Area: Shows usernames of participants currently in the room.
        * Sound Visualization Area: Displays a real-time spectrogram of the mixed audio.
* **Sound Visualization:** Real-time spectrogram showing frequency bins (X-axis, scaled for equal energy) vs. short-term intensity average (Y-axis).
* **Settings Management:**
    * Persistent configuration (e.g., TOML file).
    * Allow users to set: Username, preferred audio input/output devices, audio quality hints, STUN/TURN server list.
* **Self-Healing Connectivity:** The P2P mesh should attempt to heal itself. Peers periodically exchange information to discover and connect to any missing participants within the room.
* **Adaptive Audio Quality:** The application should attempt to adapt audio streaming quality based on network conditions, favoring high fidelity but reducing quality to maintain low latency if necessary (leveraging WebRTC capabilities).
* **Room History & Rejoin:** The application should remember the last joined room (`RoomID` and potentially peer info) to facilitate rejoining after accidental disconnection. Users should be able to start the app without a link.

## 3. Non-Functional Requirements

* **Low Latency:** Prioritize minimizing audio delay for real-time conversation.
* **Audio Quality:** Support high-fidelity audio (e.g., Opus codec defaults).
* **Resource Efficiency:** Be mindful of CPU and memory usage, particularly regarding audio processing and encryption.
* **Modularity & Extensibility:** Codebase organized into logical crates for maintainability and future feature additions.
* **Testability:** Components designed with clear interfaces amenable to unit and integration testing.
* **Security:** Robust implementation of encryption and connection protocols. No transmission of unencrypted sensitive data.

## 4. Target Platform & Future Compatibility

* **Initial Target:** macOS (Apple Silicon).
* **Future Compatibility:** Design with portability in mind for Linux and Windows (potentially via WSL initially, native later if feasible). Use cross-platform libraries where possible (e.g., `cpal`, `ratatui`).

## 5. High-Level Architecture

* **Overall Approach:** Peer-to-peer (P2P), command-line application written in Rust using `tokio` for asynchronous operations.
* **Proposed Crate Structure:**
    * **`app_cli`**: Main binary, TUI logic (`ratatui`), event loop, orchestration.
    * **`room_core`**: Shared fundamental types (`PeerId`, `RoomID`, custom errors, etc.).
    * **`audio`**: Handles all audio processing.
        * `audio_io`: Audio capture/playback using `cpal`.
        * `codec`: Audio format definitions (Opus encoding/decoding likely handled by `webrtc-rs`).
        * `spatial`: Spatial audio processing using `AudioNimbus`.
    * **`network`**: Manages P2P connectivity. Handles initial bootstrap connection (via link), P2P signaling message transport, interfaces with `webrtc-rs` library.
    * **`crypto`**: Handles initial link key verification, securing the signaling channel.
    * **`room`**: Manages room state, join requests, peer lists, `RoomID`, enforces user limits. Coordinates peer discovery/updates.
    * **`visualization`**: Spectrogram generation logic.
    * **`settings_manager`**: Loads/saves application settings (e.g., TOML file).
* **Threading Model:**
    * `tokio` multi-threaded runtime for async tasks (networking, TUI events, room state).
    * Dedicated OS thread for audio capture (`cpal`).
    * Dedicated OS thread for audio playback (`cpal`).
    * Dedicated OS thread for spatialization/mixing (`AudioNimbus`) receiving decoded audio via channels.

## 6. Key Technology Choices

* **Language:** Rust (Latest Stable)
* **Async Runtime:** `tokio`
* **TUI Library:** `ratatui`
* **Networking & Media:** `webrtc-rs` (or similar mature Rust WebRTC library) for ICE (STUN/TURN), DTLS, SRTP, Opus support, Data Channels. (STUN: Session Traversal Utilities for NAT, TURN: Traversal Using Relays around NAT, ICE: Interactive Connectivity Establishment, DTLS: Datagram Transport Layer Security, SRTP: Secure Real-time Transport Protocol).
* **Spatial Audio:** `AudioNimbus` (Rust wrapper for Steam Audio)
* **Audio I/O:** `cpal`
* **Serialization:** `serde` (with e.g., `bincode` for network, `toml` for config)
* **Cryptography:** Standard, well-vetted Rust crypto libraries (e.g., `ring`, RustCrypto crates) for link key verification and securing the signaling channel.
* **Configuration Format:** TOML (via `serde_toml`)

## 7. Detailed Component Breakdown & Interactions

* **Room Management (`room` crate):**
    * Generates unique `RoomID` on creation by the first user.
    * Tracks list of connected `PeerId`s and their associated usernames/status.
    * Handles the "Join Request" state machine (pending -> approved/denied).
    * Initiates periodic peer list exchange via `network` P2P channel (either initial or WebRTC Data Channel).
    * Provides peer position information (initially static layout) to `audio::spatial`.
* **Networking & Connectivity (`network`, `webrtc-rs`):**
    * **Phase 1: Bootstrap & Signaling:**
        * Parse link for initial peer IP/port/key and `RoomID`.
        * Establish initial P2P connection (e.g., simple reliable UDP messages) using `tokio::net`. Secure this channel using `crypto` and the link key.
        * Validate `RoomID` against target peer's current room.
        * Transport custom messages: Join Request, Approval/Denial, Peer List Exchange, Notifications ("X joined via Y").
        * Transport opaque WebRTC signaling messages (SDP Offers/Answers, ICE Candidates) generated/consumed by `webrtc-rs`. (SDP: Session Description Protocol).
    * **Phase 2: WebRTC Connection:**
        * Triggered by `webrtc-rs` after successful signaling exchange via Phase 1 channel.
        * `webrtc-rs` handles ICE (using STUN/TURN servers from `settings_manager`) for NAT traversal.
        * `webrtc-rs` establishes DTLS connection for secure key exchange.
        * `webrtc-rs` establishes SRTP channel for encrypted Opus audio stream.
        * `webrtc-rs` establishes optional Data Channel over SCTP/DTLS. (SCTP: Stream Control Transmission Protocol).
    * **Phase 3: Ongoing Communication:**
        * Audio flows via SRTP (managed by `webrtc-rs`).
        * Room updates (peer joins/leaves) can be sent over the WebRTC Data Channel (preferred) or fall back to the Phase 1 channel if needed.
* **Audio Pipeline:**
    1.  **Capture:** `audio_io` (`cpal`) captures PCM audio on dedicated thread.
    2.  **Send Path:** Audio buffer -> `webrtc-rs` (handles Opus encoding, SRTP encryption, packetization) -> `tokio` network task sends UDP packets.
    3.  **Receive Path:** `tokio` network task receives UDP -> `webrtc-rs` (handles SRTP decryption, Opus decoding) -> Decoded PCM buffer sent via channel.
    4.  **Mixing:** Dedicated audio processing thread receives decoded buffers from all peers via channels -> `audio::spatial` (`AudioNimbus`) mixes and spatializes -> Final stereo PCM buffer.
    5.  **Playback:** Final buffer sent via channel -> `audio_io` (`cpal`) plays buffer on dedicated thread.
* **Encryption (`crypto`, `webrtc-rs`):**
    * **Link Verification:** `crypto` crate verifies integrity/authenticity of initial contact using the link key.
    * **Signaling Channel Security:** `crypto` used to encrypt/authenticate messages on the Phase 1 P2P channel.
    * **Media/Data Security:** Handled entirely by DTLS/SRTP within the `webrtc-rs` library.
* **User Interface (`app_cli` / `ratatui`):**
    * Render peer list, status updates, join requests.
    * Handle user input for menu actions, settings changes, join approval.
    * Receive visualization data from `visualization` crate and render spectrogram.
* **Sound Visualization (`visualization`):**
    * Receives mixed audio buffer (or potentially pre-mixed buffers).
    * Performs FFT (Fast Fourier Transform), calculates frequency/intensity data points.
    * Sends data points to `app_cli` for rendering.

## 8. Key Considerations / Challenges

* **NAT Traversal Complexity:** While ICE (STUN/TURN) via WebRTC handles most cases, complex network topologies or restrictive firewalls might still pose challenges. TURN server configuration and availability are crucial for difficult cases.
* **Signaling Robustness:** The custom P2P signaling channel (Phase 1 networking) needs to be reliable enough to complete the WebRTC handshake.
* **Real-time Audio Processing:** Ensuring the audio capture, spatialization/mixing, and playback threads operate with low latency and without glitches requires careful implementation and performance tuning.
* **WebRTC Library Integration:** Effectively integrating `webrtc-rs` and managing its state machine alongside the application's room logic.
* **TUI Responsiveness:** Keeping the TUI interactive and updated smoothly despite background network and audio processing.
* **Error Handling:** Gracefully handling network errors, disconnections, audio device issues, and cryptographic failures.

## 9. Potential Development Phases

1.  **Core Setup:** Project structure, `tokio` integration, basic `settings_manager` loading.
2.  **Initial Networking & Signaling:** Implement Phase 1 P2P channel (`network`, `crypto`), basic link parsing/verification.
3.  **WebRTC Basics:** Integrate `webrtc-rs`, implement P2P signaling exchange (SDP/ICE), establish basic DTLS connection.
4.  **Basic Audio Flow:** Integrate `cpal`, wire up unencrypted/non-spatialized audio capture -> WebRTC (Opus/SRTP) -> playback.
5.  **TUI Basics:** Implement `ratatui` framework, basic menu, peer list display.
6.  **Room Logic:** Implement `RoomID`, join request/approval flow, basic peer list management (`room` crate).
7.  **Spatial Audio:** Integrate `AudioNimbus`, dedicated mixing thread, spatialization logic (`audio::spatial`).
8.  **Visualization:** Implement spectrogram logic (`visualization`) and rendering (`app_cli`).
9.  **Refinements:** Settings UI, room history/rejoin, error handling improvements, adaptive quality tuning, self-healing mesh refinement.
10. **Testing & Polishing:** Comprehensive testing, documentation, performance optimization.
