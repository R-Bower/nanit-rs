use std::time::Instant;

use chrono::Utc;
use tokio::signal;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::api::client::NanitClient;
use crate::cli::WatchArgs;
use crate::motion::calibrator::SnooCalibrator;
use crate::motion::detector::{frame_intensity, MotionDetector};
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
    println!(
        "Calibrating for {} seconds (collecting Snoo baseline)...",
        args.calibration_secs
    );

    let mut calibrator = SnooCalibrator::new();
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
            let intensity = frame_intensity(prev, &frame);
            calibrator.add_sample(intensity);
        }
        prev_frame = Some(frame);
    }

    let elapsed_secs = args.calibration_secs as f64;
    let fps_estimate = if elapsed_secs > 0.0 {
        frame_count as f64 / elapsed_secs
    } else {
        15.0 // fallback
    };

    let baseline = calibrator.compute_baseline().unwrap_or(0.01);
    println!(
        "Calibration complete: {} samples, mean={:.6}, std_dev={:.6}, baseline={:.6}, fps≈{:.1}",
        calibrator.sample_count(),
        calibrator.mean(),
        calibrator.std_dev(),
        baseline,
        fps_estimate,
    );

    // --- Detection phase ---
    let mut detector = MotionDetector::new(baseline, args.threshold, fps_estimate, 0.3);

    println!("Watching for motion (threshold_multiplier={:.1})...", args.threshold);

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
            let intensity = frame_intensity(prev, &frame);
            if let Some(rolling_avg) = detector.update(intensity) {
                let now = Utc::now().to_rfc3339();
                println!(
                    "[{now}] MOTION intensity={:.4} (baseline={:.4})",
                    rolling_avg,
                    detector.baseline()
                );
            }
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
