# nanit — Rust CLI for the Nanit baby monitor

A Rust rewrite of the original TypeScript nanit-node project. Provides REST API access, WebSocket camera control, local RTMP streaming, and motion detection with Snoo bassinet calibration.

## Build & Run

```bash
cargo build              # debug build
cargo build --release    # optimized build
cargo test               # run all unit tests
cargo clippy             # lint
cargo run -- --help      # show CLI usage
```

Requires `protoc` (protobuf compiler) for build. On macOS: `brew install protobuf`.

## Project Structure

```
Cargo.toml
build.rs                    # prost-build compiles proto/nanit.proto
proto/nanit.proto            # Protobuf v2 definitions for WebSocket protocol
ts-ref/                      # Original TypeScript source (reference only)
src/
  main.rs                    # tokio::main, clap parse, dispatch
  util.rs                    # get_local_ip(), prompt_input()
  api/
    mod.rs
    client.rs                # NanitClient: REST (reqwest)
    types.rs                 # Baby, NanitMessage, AuthRequest/Response, constants
    error.rs                 # NanitError enum (thiserror)
  session/
    mod.rs                   # SessionStore: JSON load/save, token expiry, tests
  ws/
    mod.rs
    connection.rs            # NanitWebSocket: tokio channels, keepalive, correlation
    codec.rs                 # Protobuf encode/decode helpers, request builders
  proto/
    mod.rs                   # include!(prost-generated code)
  motion/
    mod.rs
    detector.rs              # Frame differencing, intensity scoring, debounce
    calibrator.rs            # SnooCalibrator: rolling mean+2σ baseline
    pipeline.rs              # ffmpeg spawn, raw grayscale frame reader
  cli/
    mod.rs                   # Clap App + subcommand definitions
    login.rs                 # nanit login (with MFA support)
    babies.rs                # nanit babies
    messages.rs              # nanit messages <uid>
    sensors.rs               # nanit sensors <uid> (WebSocket sensor data)
    stream.rs                # nanit stream <uid> (RTMP + ffplay)
    watch.rs                 # nanit watch <uid> (motion detection)
```

## Auth Quirks (Critical)

These are reverse-engineered and must be preserved exactly:

- **REST auth**: `Authorization: {token}` — NO "Bearer" prefix
- **WebSocket auth**: `Authorization: Bearer {token}` — WITH "Bearer" prefix
- **Login endpoint**: `POST /login` with header `nanit-api-version: 2`
  - 201 = success, 401 = bad credentials, 482 = MFA required
- **Token refresh**: `POST /tokens/refresh` with `{refresh_token}` body
  - 200 = ok, 404 = expired (re-login required)
- **Session file**: `~/.nanit/session.json`, camelCase keys, `revision: 1` guard

## WebSocket Protocol

- URL: `wss://api.nanit.com/focus/cameras/{camera_uid}/user_connect`
- Binary protobuf v2 messages (see `proto/nanit.proto`)
- Keepalive every 20 seconds
- Request/response correlation by integer ID
- Reconnect delays: 30s → 2m → 15m → 1h

## Motion Detection (Snoo Normalization)

The baby is in a Snoo smart bassinet that rocks constantly. Simple frame differencing would always trigger. The approach:

1. **Calibration** (first 10s): Collect frame-to-frame pixel intensity values, compute `mean + 2σ` as baseline (captures 95% of rocking)
2. **Detection**: Rolling average over ~0.15s window, alert when `rolling_avg > baseline + threshold_offset` (default 0.008)
3. **Debounce**: 0.15s sustained elevation required to avoid false positives

Pipeline: Camera → RTMP push → ffmpeg (scale+grayscale) → raw frames → calibrate/detect → stdout

## Commands

```bash
nanit login                           # authenticate (supports MFA)
nanit babies                          # list babies
nanit messages <baby_uid>             # fetch recent events
nanit sensors <baby_uid>              # live sensor data via WebSocket
nanit stream <baby_uid> [-o file]     # RTMP stream (play or record)
nanit watch <baby_uid>                # motion detection with Snoo calibration
  [--calibration-secs 10]
  [--threshold 0.008]
  [--width 320] [--height 240]
  [--port 1935] [--ip <addr>]
```

## Dependencies

External tools required at runtime:
- `ffmpeg` — for RTMP listening and frame extraction
- `ffplay` — for live playback (stream command without -o)
