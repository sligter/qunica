use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use clap::Parser;
use qunica_computer_supervisor::{docker::DockerProvider, Supervisor};

#[derive(Parser)]
struct Options {
    #[arg(
        long,
        env = "QUNICA_SUPERVISOR_LISTEN",
        default_value = "127.0.0.1:9000"
    )]
    listen: SocketAddr,
    #[arg(
        long,
        env = "QUNICA_SUPERVISOR_STATE",
        default_value = "computer-state"
    )]
    state: PathBuf,
    #[arg(long, env = "QUNICA_SUPERVISOR_TOKEN_FILE")]
    token_file: PathBuf,
    #[arg(
        long,
        env = "QUNICA_COMPUTER_IMAGE",
        default_value = "qunica-computer:local"
    )]
    image: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    let token = std::fs::read_to_string(&options.token_file)?;
    let provider = Arc::new(
        DockerProvider::connect(options.image)
            .await?
            .with_services(std::env::var("QUNICA_COMPUTER_SERVICES").as_deref() == Ok("true"))
            .with_attestation(token.trim().to_string()),
    );
    let supervisor = Supervisor::open(options.state, token.trim(), provider)?;
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    axum::serve(listener, supervisor.router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
