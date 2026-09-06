//! Isolated device smoke server: no live workspace, accounts or command execution.
//! cargo run --manifest-path mobile-link/Cargo.toml --features server --example lan_smoke -- <LAN IPv4> <offer-file>
#[cfg(feature = "server")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    use axum::{routing::{get, post}, Router};
    use qunica_mobile_link::server::MobileServer;
    let args: Vec<String> = std::env::args().collect();
    let directory = tempfile::tempdir()?;
    let router = Router::new()
        .route("/api/v2/health", get(|| async { axum::Json(serde_json::json!({"service":"qunica-mobile-probe"})) }))
        .route("/api/v2/echo", post(|body: axum::body::Bytes| async move { body }))
        .route("/api/v2/events", get(|| async {
            let frames = futures_util::stream::unfold(0, |id| async move {
                if id >= 3 { return None; }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                Some((Ok::<_, std::io::Error>(format!("id: probe:{id}\ndata: encrypted\n\n")), id + 1))
            });
            ([("content-type", "text/event-stream")], axum::body::Body::from_stream(frames))
        }));
    let server = MobileServer::load(directory.path().join("identity.json"), router)?;
    let interfaces = MobileServer::interfaces()?;
    let address = args.get(1).cloned().or_else(|| interfaces.first().map(|i| i.address.clone())).ok_or_else(|| anyhow::anyhow!("No LAN interface"))?;
    server.start(&address).await?;
    let path = args.get(2).ok_or_else(|| anyhow::anyhow!("Provide an output path for the temporary pairing offer"))?;
    std::fs::write(path, server.offer().await?.uri)?;
    println!("Isolated smoke server ready at {address}:8766; expires after 120 seconds");
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    server.stop().await;
    Ok(())
}
#[cfg(not(feature = "server"))]
fn main() { eprintln!("Enable the server feature"); }
