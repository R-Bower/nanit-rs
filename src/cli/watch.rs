use std::time::Instant;

use chrono::Utc;
use tokio::signal;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::api::client::NanitClient;
use crate::cli::WatchArgs;
use crate::motion::calibrator::GridCalibrator;
use crate::motion::detector::{grid_intensities, GridConfig, GridMotionDetector};
use crate::motion::pipeline::FramePipeline;
use crate::proto;
use crate::session::init_session_store;
use crate::util::get_local_ip;
use crate::ws::connection::NanitWebSocket;

pub async fn run(session_path: &str, args: WatchArgs) -> anyhow::Result<()> {
    let mut session = init_session_store(session_path);
    let client = NanitClient::new();

    client.maybe_authorize(&mut session, false).await?;

    let babies = client.ensure_babies(&mut session).await?;
    let baby = babies
        .iter()
        .find(|b| b.uid == args.baby_uid)
        .ok_or_else(|| anyhow::anyhow!("Baby with UID {} not found", args.baby_uid))?
        .clone();

    let rtmp_listen_url = format!("rtmp://0.0.0.0:{}/live/{}", args.port, baby.uid);

    // Spawn ffmpeg for raw grayscale frame output
    println!(
        "Starting ffmpeg RTMP listener on port {} ({}x{} grayscale)...",
        args.port, args.width, args.height
    );
    let mut pipeline = FramePipeline::spawn(&rtmp_listen_url, args.width, args.height)?;

    // Give ffmpeg a moment to start listening
    sleep(Duration::from_secs(1)).await;

    // Resolve LAN IP
    let local_ip = args
        .ip
        .clone()
        .or_else(get_local_ip)
        .ok_or_else(|| {
            anyhow::anyhow!("Could not detect local IP. Use --ip to specify your LAN address.")
        })?;

    let target_url = format!("rtmp://{local_ip}:{}/live/{}", args.port, baby.uid);

    // Connect WebSocket and tell camera to push RTMP to us
    let mut ws = NanitWebSocket::new(&baby.camera_uid, session.auth_token());
    ws.connect().await?;

    println!("Connected to camera. Requesting stream to {target_url}");
    ws.put_streaming(&target_url, proto::streaming::Status::Started)
        .await?;

    // --- Calibration phase ---
    let grid = GridConfig::new(args.width, args.height, args.grid_cols, args.grid_rows);
    let mut cell_buf = vec![0.0f64; grid.num_cells];

    println!(
        "Calibrating for {} seconds ({}x{} grid, {} cells)...",
        args.calibration_secs, args.grid_cols, args.grid_rows, grid.num_cells,
    );

    let mut calibrator = GridCalibrator::new(grid.num_cells);
    let mut prev_frame: Option<Vec<u8>> = None;
    let calibration_deadline = Instant::now() + Duration::from_secs(args.calibration_secs);
    let mut frame_count: u64 = 0;

    loop {
        if Instant::now() >= calibration_deadline {
            break;
        }

        // Use tokio::select to allow ctrl+c during calibration
        let frame = tokio::select! {
            f = pipeline.next_frame() => f,
            _ = signal::ctrl_c() => {
                println!("\nInterrupted during calibration.");
                cleanup(&mut ws, &mut pipeline).await;
                return Ok(());
            }
        };

        let frame = match frame {
            Some(f) => f,
            None => {
                eprintln!("ffmpeg stream ended during calibration");
                break;
            }
        };

        frame_count += 1;

        if let Some(ref prev) = prev_frame {
            grid_intensities(prev, &frame, &grid, &mut cell_buf);
            calibrator.add_samples(&cell_buf);
        }
        prev_frame = Some(frame);
    }

    let elapsed_secs = args.calibration_secs as f64;
    let fps_estimate = if elapsed_secs > 0.0 {
        frame_count as f64 / elapsed_secs
    } else {
        15.0 // fallback
    };

    let cell_stats = calibrator.cell_stats();
    let avg_mean = cell_stats.iter().map(|(m, _)| m).sum::<f64>() / cell_stats.len() as f64;
    let avg_std = cell_stats.iter().map(|(_, s)| s).sum::<f64>() / cell_stats.len() as f64;
    println!(
        "Calibration complete: {} samples, avg_mean={:.6}, avg_std={:.6}, fps≈{:.1}",
        calibrator.sample_count(),
        avg_mean,
        avg_std,
        fps_estimate,
    );

    // --- Detection phase ---
    let cell_stats = if calibrator.sample_count() > 0 {
        cell_stats
    } else {
        vec![(0.01, 0.005); grid.num_cells]
    };
    let mut detector =
        GridMotionDetector::new(cell_stats, args.threshold, fps_estimate, 0.15, args.adapt_tau);

    let mut debug_frame_count: u64 = 0;

    if args.adapt_tau > 0.0 {
        println!(
            "Watching for motion (threshold_multiplier={:.1}, adaptive tau={:.0}s)",
            args.threshold, args.adapt_tau,
        );
    } else {
        println!("Watching for motion (threshold_multiplier={:.1}, adaptive=off)", args.threshold);
    }

    loop {
        let frame = tokio::select! {
            f = pipeline.next_frame() => f,
            _ = signal::ctrl_c() => {
                println!("\nStopping...");
                break;
            }
        };

        let frame = match frame {
            Some(f) => f,
            None => {
                eprintln!("ffmpeg stream ended.");
                break;
            }
        };

        if let Some(ref prev) = prev_frame {
            grid_intensities(prev, &frame, &grid, &mut cell_buf);
            let peak = cell_buf.iter().cloned().fold(0.0f64, f64::max);
            let peak_idx = cell_buf
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            if let Some(event) = detector.update(&cell_buf) {
                let now = Utc::now().to_rfc3339();
                let cell_col = event.max_cell_index % grid.cols as usize;
                let cell_row = event.max_cell_index / grid.cols as usize;
                println!(
                    "[{now}] MOTION intensity={:.4} cell=({},{}) elevated_cells={}",
                    event.max_cell_intensity, cell_col, cell_row, event.num_elevated_cells,
                );
            } else if debug_frame_count % 7 == 0 {
                // Print peak cell intensity ~once per second for tuning
                let col = peak_idx % grid.cols as usize;
                let row = peak_idx / grid.cols as usize;
                println!("  [debug] peak={:.6} cell=({},{})", peak, col, row);
            }
            debug_frame_count += 1;
        }
        prev_frame = Some(frame);
    }

    cleanup(&mut ws, &mut pipeline).await;
    Ok(())
}

async fn cleanup(ws: &mut NanitWebSocket, pipeline: &mut FramePipeline) {
    if ws.is_connected() {
        let _ = ws
            .put_streaming("", proto::streaming::Status::Stopped)
            .await;
    }
    ws.disconnect().await;
    pipeline.kill().await;
    info!("Cleanup complete");
}
