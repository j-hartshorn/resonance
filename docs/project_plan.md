# resonance.rs

A rust application to allow small groups of people to communicate via hi-fedelity, low-latency audio. 
It should feel like the people are in a room together, with multiple people talking at once and clearly audible, and positional audio simulation spreading out the participants in virtaul space.

## Key features

- Hi-fidelity audio (as much as is practical to maintin low latency over a good internet connection)
- Positional audio, using virtual rooom simulation
  - Users are automatically arranged in a virtual circle facing inward, mimicking a natural conversation
  - Spatial positioning is consistent across all users (if A hears B on the left, B hears A on the right)
- Everyone can talk at once and still be heard. Audio is balanced and lifelike.
- Session management:
  - A user can create a session that others can join
  - Users can join existing sessions via a temporary link
  - Sessions continue uninterrupted if individual users leave
  - Full peer-to-peer connections between all users in a session
- User controls:
  - Users can mute themselves
  - Users can adjust volume of others
- Designed for headphones
- Operated via an interactive CLI use interface
- Peer to peer secure communication using a temporary link and end to end encrypted connection

## Libraries

A list of libraries to ultilise and functionality they enable

### `tokio`

(if required, this dependency can change or be removed, it is not essential, merely a suggestion)

Tokio is an asynchronous runtime for the Rust programming language. It provides the building blocks needed for writing network applications. It gives the flexibility to target a wide range of systems, from large servers with dozens of cores to small embedded devices.

- Async runtime

### `webrtc.rs`

A pure Rust implementation of WebRTC stack. Rewrite Pion WebRTC stack in Rust

- Sending and recieving hi-fedelity audio with minimal latency

### `webrtc-audio-processing`

- Processing input audio
    - Echo cancellation
    - Voice detection

### `audionimbus`

A Rust wrapper around Steam Audio that provides spatial audio capabilities with realistic occlusion, reverb, and HRTF effects, accounting for physical attributes and scene geometry.

### `ratatui.rs`

- Terminal interface
- showing settings menue to allow config to be changed on the fly
- a nice visualisation of the spectorgraph of each of the prticipants

## Development

Create a modular architecture and write tests for the functionality as you go.
Follow a TDD approach, and make sure to run tests and correct if things don't function correctly.
