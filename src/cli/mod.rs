pub mod babies;
pub mod login;
pub mod messages;
pub mod sensors;
pub mod stream;
pub mod watch;

use clap::Parser;

#[derive(Parser)]
#[command(name = "nanit", about = "CLI for the Nanit baby monitor API", version)]
pub struct Cli {
    /// Path to session file
    #[arg(long, default_value_t = default_session_path())]
    pub session: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Login to Nanit with email and password
    Login,
    /// List babies associated with the account
    Babies,
    /// Fetch recent messages/events for a baby
    Messages {
        baby_uid: String,
        /// Number of messages to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: u32,
    },
    /// Connect to camera via WebSocket and stream sensor data
    Sensors {
        baby_uid: String,
    },
    /// Stream camera via local RTMP
    Stream(StreamArgs),
    /// Watch camera with motion detection
    Watch(WatchArgs),
}

#[derive(clap::Args, Clone)]
pub struct StreamArgs {
    pub baby_uid: String,
    /// Output file (omit to play with ffplay)
    #[arg(short, long)]
    pub output: Option<String>,
    /// Local RTMP listen port
    #[arg(short, long, default_value_t = 1935)]
    pub port: u16,
    /// LAN IP for camera to reach (auto-detected)
    #[arg(long)]
    pub ip: Option<String>,
}

#[derive(clap::Args, Clone)]
pub struct WatchArgs {
    pub baby_uid: String,
    /// Local RTMP listen port
    #[arg(short, long, default_value_t = 1935)]
    pub port: u16,
    /// LAN IP for camera to reach (auto-detected)
    #[arg(long)]
    pub ip: Option<String>,
    /// Calibration duration in seconds
    #[arg(long, default_value_t = 10)]
    pub calibration_secs: u64,
    /// Additive threshold offset above baseline
    #[arg(long, default_value_t = 0.008)]
    pub threshold: f64,
    /// Frame width for analysis
    #[arg(long, default_value_t = 320)]
    pub width: u32,
    /// Frame height for analysis
    #[arg(long, default_value_t = 240)]
    pub height: u32,
    /// Grid columns for motion detection
    #[arg(long, default_value_t = 16)]
    pub grid_cols: u32,
    /// Grid rows for motion detection
    #[arg(long, default_value_t = 12)]
    pub grid_rows: u32,
    /// Adaptive baseline time constant in seconds (0 = disabled)
    #[arg(long, default_value_t = 10.0)]
    pub adapt_tau: f64,
}

fn default_session_path() -> String {
    let home = dirs_home();
    format!("{home}/.nanit/session.json")
}

fn dirs_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
}
