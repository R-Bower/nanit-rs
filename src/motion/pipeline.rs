use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

/// Spawns ffmpeg to listen for RTMP and output raw grayscale frames.
pub struct FramePipeline {
    child: Child,
    stdout: tokio::process::ChildStdout,
    frame_size: usize,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

impl FramePipeline {
    /// Spawn ffmpeg listening on the given RTMP URL, outputting raw grayscale frames.
    pub fn spawn(rtmp_url: &str, width: u32, height: u32) -> std::io::Result<Self> {
        let mut child = Command::new("ffmpeg")
            .args([
                "-listen",
                "1",
                "-i",
                rtmp_url,
                "-vf",
                &format!("scale={width}:{height}"),
                "-pix_fmt",
                "gray",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let frame_size = (width * height) as usize;

        Ok(Self {
            child,
            stdout,
            frame_size,
            width,
            height,
        })
    }

    /// Read the next raw grayscale frame. Returns None on EOF.
    pub async fn next_frame(&mut self) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; self.frame_size];
        let mut offset = 0;
        while offset < self.frame_size {
            match self.stdout.read(&mut buf[offset..]).await {
                Ok(0) => return None, // EOF
                Ok(n) => offset += n,
                Err(_) => return None,
            }
        }
        Some(buf)
    }

    /// Kill the ffmpeg process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}
