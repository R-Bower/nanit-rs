# nanit

Rust CLI for the Nanit baby monitor. Pulls a live RTMP stream from the camera and does motion detection, built to work with the Snoo bassinet.

Inspired by: https://github.com/gregory-m/nanit

## Motion detection

The Snoo rocks nonstop, so normal frame differencing is inaccurate. This uses a grid-based approach with adaptive baselines to separate rocking from actual baby movement.

- baseline: the normal intensity level for each cell, i.e. how much it typically changes frame-to-frame from just the rocking.
- adaptive: it updates itself in response to movement. When the baby shifts position and the rocking looks different to certain cells, those cells' baselines adjust to match within about 30-50 seconds.

**Grid**: The frame is divided into a 16x12 grid of 20x20px cells. Motion is tracked per-cell so small movements don't get lost in a full-frame average.

**Calibration**: On startup, 10 seconds of rocking data is collected. Each cell gets a baseline of mean + 2 standard deviations, which covers about 95% of normal rocking. Cells with very low baselines get a floor of 0.006. The detection threshold is `baseline * multiplier`, so if a cell's baseline is near zero, the threshold is also near zero and everything triggers. The floor prevents this.

**Detection**: A cell is flagged when its intensity exceeds 3x its baseline. Has to stay elevated for ~0.15s to count — single frame spikes are ignored.

**Adaptation**: Baselines aren't static. On frames where nothing is flagged, each cell's baseline slowly adjusts toward what it's currently seeing (exponential moving average, tau=10s). This handles the baby shifting to a new position. The rocking pattern changes and the baselines follow. Takes about 30-50 seconds to settle after a position change.

In practice, rocking produces per-cell intensities of 0.001-0.006 and real movement is 0.020+.

```
[2026-03-27T08:45:12.123Z] MOTION intensity=0.0234 cell=(7,5) elevated_cells=3
```

## Usage

```bash
cargo build --release
nanit login
nanit babies
nanit watch <baby_uid>
```

### Watch options

```
nanit watch <baby_uid>
  --calibration-secs 10    # calibration duration
  --threshold 3.0          # multiplier above baseline
  --grid-cols 16           # grid columns
  --grid-rows 12           # grid rows
  --adapt-tau 10.0         # EMA time constant (0 = off)
  --width 320              # analysis frame width
  --height 240             # analysis frame height
  --port 1935              # RTMP listen port
  --ip <addr>              # LAN IP (auto-detected)
```

### Other commands

```bash
nanit login                    # auth (supports MFA)
nanit babies                   # list babies
nanit messages <baby_uid>      # recent events
nanit sensors <baby_uid>       # live sensor data via WebSocket
nanit stream <baby_uid>        # live playback
nanit stream <baby_uid> -o f   # record to file
```

## Build requirements

- Rust toolchain
- `protoc` — `brew install protobuf` on macOS
- `ffmpeg` — RTMP + frame extraction
- `ffplay` — live playback (stream command only)
