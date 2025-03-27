# resonance.rs

A rust application to allow small groups of people to communicate via hi-fedelity, low-latency audio. 
It should feel like the people are in a room together, with multiple people talking at once and clearly audible, and positional audio simulation spreading out the participants in virtaul space.

## Key features

- Hi-fidelity audio (as much as is practical to maintin low latency over a good internet connection)
- Positional audio, using virtual rooom simulation
- everyone can talk at once and still be heard. Audio is balanced and lifelike.
- Designed for headphones
- operated via an interactive CLI use interface
- peer to peer secure communication using a temporary link and end to end encrypted connection

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

### `audionumbus`

A Rust wrapper around Steam Audio that provides spatial audio capabilities with realistic occlusion, reverb, and HRTF effects, accounting for physical attributes and scene geometry.

### `ratatui.rs`

- Terminal interface
- showing settings menue to allow config to be changed on the fly
- a nice visualisation of the spectorgraph of each of the prticipants
