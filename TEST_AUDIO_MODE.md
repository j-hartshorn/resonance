# Testing with Virtual Audio Device

This document explains how to test network and room connection functionality on a single machine without requiring multiple physical audio devices.

## Test Audio Mode

The application now supports a `--test-audio` flag that enables a virtual audio device. This mode generates a test tone instead of using your real microphone input, and simulates audio playback without sending it to your speakers.

## Testing Steps

To test room connection functionality with two instances:

1. Open two terminal windows/tabs
2. In the first terminal, run:
   ```
   cargo run -- --test-audio
   ```
3. In the second terminal, run:
   ```
   cargo run -- --test-audio
   ```
4. In the first app instance:
   - Press 'c' to create a room
   - Press 'g' to generate a join link (it will be copied to your clipboard)
5. In the second app instance:
   - Press 'j' to join a room
   - Paste the join link and press Enter
6. Back in the first app instance:
   - When you see the join request, press 'a' to approve it
7. Now the two instances should be connected and able to exchange audio

## How Test Audio Mode Works

When test audio mode is enabled:

1. The application creates a virtual audio device that doesn't use your physical audio hardware
2. For input: It generates a simple sine wave tone at 440Hz (A4 note)
3. For output: It receives audio data but doesn't play it through speakers

This allows you to test the full functionality of the app without interfering with your system's actual audio devices.

## Logging

The application now writes all logs to a file instead of printing them to the console, which prevents them from interfering with the Terminal UI. By default, a timestamped log file is created in your system's temporary directory.

### Log Options

- Default: Logs are written to a file like `room_rs_1712341234.log` in your system's temp directory
- Custom log file: Specify a log file path with `--log-file PATH`
- Debug logging: Use `--debug` to enable more detailed logs

### Viewing Logs

To view logs while the app is running (in a separate terminal):
```
tail -f /tmp/room_rs_*.log
```

Or for a custom log path:
```
tail -f YOUR_CUSTOM_PATH
```

## Limitations

- This is for testing purposes only and doesn't produce real audio output
- The test tone is a simple sine wave, not representative of real voice or music
- If you want to actually hear audio, use the regular mode without `--test-audio` flag 