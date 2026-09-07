//! Isolated end-to-end check through an already running TCP reverse proxy.
//! No account database, real credentials, or persistent desktop identity.
#[cfg(feature = "server")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    use axum::{routing::{get, post}, Router};
    use qunica_mobile_link::{self as link, server::MobileServer};
    let endpoint = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("Provide relay host:port"))?;
    let directory = tempfile::tempdir()?;
    let router = Router::new()
        .route("/api/v2/echo", post(|body: axum::body::Bytes| async move { body }))
        .route("/api/v2/events", get(|| async {
            ([("content-type", "text/event-stream")], "id: relay:1\ndata: first\n\nid: relay:2\ndata: second\n\n")
        }));
    let server = MobileServer::load(directory.path().join("identity.json"), router)?;
    server.start_relay(&endpoint).await?;
    let offer = server.offer().await?;
    let connection = link::pair(link::Offer::parse(&offer.uri)?, "Relay smoke".into()).await?;
    anyhow::ensure!(link::pair(link::Offer::parse(&offer.uri)?, "Replay".into()).await.is_err());
    link::verify_connection(&connection).await?;
    let mut wrong = connection.clone();
    wrong.public_key = link::encode(&link::keypair()?.public);
    anyhow::ensure!(link::verify_connection(&wrong).await.is_err());
    let payload: Vec<u8> = (0..100_003).map(|n| (n % 256) as u8).collect();
    for (path, body) in [("/api/v2/echo", payload.as_slice()), ("/api/v2/events", &[][..])] {
        let head = link::RequestHead { method: if body.is_empty() { "GET" } else { "POST" }.into(), path: path.into(), headers: vec![] };
        let (head, mut rx, _tx) = link::request(&connection, head, body).await?;
        anyhow::ensure!(head.status == 200);
        let mut response = vec![];
        loop {
            let (kind, bytes) = rx.frame().await?;
            if kind == link::END { break; }
            if kind == link::DATA { response.extend(bytes); }
        }
        if path.ends_with("echo") { anyhow::ensure!(response == payload); }
        else { anyhow::ensure!(String::from_utf8(response)?.contains("id: relay:2")); }
    }
    server.revoke(&server.status().await?.devices[0].id).await?;
    anyhow::ensure!(link::verify_connection(&connection).await.is_err());
    server.stop().await;
    println!("PASS: frp pairing, single use, pinned identity, 100003-byte binary roundtrip, SSE and revocation");
    Ok(())
}
#[cfg(not(feature = "server"))]
fn main() { eprintln!("Enable the server feature"); }
