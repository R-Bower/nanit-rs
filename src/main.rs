mod api;
mod cli;
mod motion;
mod proto;
mod session;
mod util;
mod ws;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let session_path = cli.session.clone();

    match cli.command {
        Command::Login => cli::login::run(&session_path).await,
        Command::Babies => cli::babies::run(&session_path).await,
        Command::Messages { baby_uid, limit } => {
            cli::messages::run(&session_path, &baby_uid, limit).await
        }
        Command::Sensors { baby_uid } => cli::sensors::run(&session_path, &baby_uid).await,
        Command::Stream(args) => cli::stream::run(&session_path, args).await,
        Command::Watch(args) => cli::watch::run(&session_path, args).await,
    }
}
