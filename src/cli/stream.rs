use std::process::Stdio;

use tokio::process::{Child, Command};
use tokio::signal;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::api::client::NanitClient;
use crate::cli::StreamArgs;
use crate::proto;
use crate::session::init_session_store;
use crate::util::get_local_ip;
use crate::ws::connection::NanitWebSocket;

pub async fn run(session_path: &str, args: StreamArgs) -> anyhow::Result<()> {
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

    // Start ffmpeg listening as RTMP server
    println!("Starting ffmpeg RTMP listener on port {}...", args.port);

    let mut ffmpeg_args = vec![
        "-listen".to_string(),
        "1".to_string(),
        "-i".to_string(),
        rtmp_listen_url,
    ];

    if let Some(ref output) = args.output {
        println!("Recording to {output}...");
        ffmpeg_args.extend(["-c".into(), "copy".into(), output.clone()]);
    } else {
        println!("Starting live playback via ffplay...");
        ffmpeg_args.extend([
            "-c".into(),
            "copy".into(),
            "-f".into(),
            "flv".into(),
            "pipe:1".into(),
        ]);
    }

    let mut ffmpeg = Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdin(Stdio::inherit())
        .stdout(if args.output.is_some() {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut ffplay: Option<Child> = None;

    if args.output.is_none() {
        let ffmpeg_stdout = ffmpeg.stdout.take().unwrap();
        let std_stdout: std::process::ChildStdout = ffmpeg_stdout
            .into_owned_fd()
            .map(std::process::ChildStdout::from)
            .expect("failed to convert tokio stdout to std stdout");
        ffplay = Some(
            Command::new("ffplay")
                .args(["-i", "pipe:0", "-infbuf", "-framedrop"])
                .stdin(std_stdout)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?,
        );
    }

    // Give ffmpeg a moment to start listening, but allow CTRL+C
    tokio::select! {
        _ = sleep(Duration::from_secs(1)) => {}
        _ = signal::ctrl_c() => {
            println!("\nInterrupted during startup.");
            kill_processes(&mut ffmpeg, &mut ffplay).await;
            return Ok(());
        }
    }

    // Resolve LAN IP
    let local_ip = args
        .ip
        .clone()
        .or_else(get_local_ip)
        .ok_or_else(|| {
            kill_processes_sync(&mut ffmpeg, &mut ffplay);
            anyhow::anyhow!("Could not detect local IP. Use --ip to specify your LAN address.")
        })?;

    let target_url = format!("rtmp://{local_ip}:{}/live/{}", args.port, baby.uid);

    // Connect WebSocket and tell camera to push RTMP to us
    let mut ws = NanitWebSocket::new(&baby.camera_uid, session.auth_token());

    let connect_result = tokio::select! {
        r = ws.connect() => r,
        _ = signal::ctrl_c() => {
            println!("\nInterrupted during WebSocket connect.");
            kill_processes(&mut ffmpeg, &mut ffplay).await;
            return Ok(());
        }
    };

    if let Err(e) = connect_result {
        kill_processes(&mut ffmpeg, &mut ffplay).await;
        return Err(e.into());
    }

    info!("Connected to camera. Requesting stream to {target_url}");
    println!("Connected to camera. Requesting stream to {target_url}");

    let stream_result = tokio::select! {
        r = ws.put_streaming(&target_url, proto::streaming::Status::Started) => r,
        _ = signal::ctrl_c() => {
            println!("\nInterrupted during stream request.");
            cleanup(&mut ws, &mut ffmpeg, &mut ffplay).await;
            return Ok(());
        }
    };

    if let Err(e) = stream_result {
        cleanup(&mut ws, &mut ffmpeg, &mut ffplay).await;
        return Err(e.into());
    }

    // Wait for SIGINT
    signal::ctrl_c().await?;
    println!("\nStopping stream...");

    cleanup(&mut ws, &mut ffmpeg, &mut ffplay).await;

    Ok(())
}

async fn cleanup(ws: &mut NanitWebSocket, ffmpeg: &mut Child, ffplay: &mut Option<Child>) {
    if ws.is_connected() {
        let _ = ws
            .put_streaming("", proto::streaming::Status::Stopped)
            .await;
    }
    ws.disconnect().await;
    kill_processes(ffmpeg, ffplay).await;
    info!("Cleanup complete");
}

async fn kill_processes(ffmpeg: &mut Child, ffplay: &mut Option<Child>) {
    ffmpeg.kill().await.ok();
    if let Some(ref mut fp) = ffplay {
        fp.kill().await.ok();
    }
}

/// Sync version for use in closures that can't be async
fn kill_processes_sync(ffmpeg: &mut Child, ffplay: &mut Option<Child>) {
    let _ = ffmpeg.start_kill();
    if let Some(ref mut fp) = ffplay {
        let _ = fp.start_kill();
    }
}
