//! Desktop-only listener. It never serves plaintext HTTP or a public pairing API.
use crate::*;
use axum::{
    body::{Body, HttpBody},
    http::{Request, Uri},
    Router,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex as AsyncMutex, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const PORT: u16 = 8766;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hash(secret: &str) -> String {
    encode(&Sha256::digest(secret.as_bytes()))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Registered {
    device: Device,
    hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    private_key: String,
    public_key: String,
    devices: Vec<Registered>,
}
struct Pending {
    hash: String,
    expires: u64,
}
struct Active {
    cancel: CancellationToken,
    slots: Arc<Semaphore>,
}
struct State {
    stored: Stored,
    pending: Option<Pending>,
    listener: Option<(SocketAddr, CancellationToken)>,
    devices: HashMap<String, Active>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use futures_util::{stream, StreamExt};

    async fn fixture() -> (tempfile::TempDir, Arc<MobileServer>, SocketAddr) {
        let dir = tempfile::tempdir().unwrap();
        let router = Router::new()
            .route(
                "/api/v2/echo",
                post(|body: axum::body::Bytes| async move { body }),
            )
            .route(
                "/api/v2/private",
                get(|headers: HeaderMap| async move {
                    if headers
                        .get("authorization")
                        .is_some_and(|v| v == "Bearer account-token")
                    {
                        StatusCode::OK
                    } else {
                        StatusCode::UNAUTHORIZED
                    }
                }),
            )
            .route(
                "/api/v2/events",
                get(|| async {
                    let frames = stream::once(async {
                        Ok::<_, std::io::Error>("id: stream:1\ndata: live\n\n")
                    })
                    .chain(stream::pending());
                    (
                        [("content-type", "text/event-stream")],
                        Body::from_stream(frames),
                    )
                }),
            );
        let server = MobileServer::load(dir.path().join("devices.json"), router).unwrap();
        server.bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let address = server.state.lock().await.listener.as_ref().unwrap().0;
        (dir, server, address)
    }
    async fn channel(server: &MobileServer, address: SocketAddr) -> (Receiver, Sender) {
        let key = decode(&server.state.lock().await.stored.public_key).unwrap();
        connect(address, &key).await.unwrap()
    }
    async fn claim(server: &MobileServer, address: SocketAddr, uri: &str) -> Result<String> {
        let offer: Offer =
            serde_json::from_slice(&decode(uri.strip_prefix("qunica://pair?data=").unwrap())?)?;
        let (mut rx, mut tx) = channel(server, address).await;
        tx.json(&Auth {
            credential: offer.code,
            claim: true,
            name: "Test phone".into(),
        })
        .await?;
        let reply: Authorized = rx.json().await?;
        Ok(reply.credential.unwrap())
    }
    async fn send(
        server: &MobileServer,
        address: SocketAddr,
        credential: &str,
        path: &str,
        body: &[u8],
    ) -> Result<(ResponseHead, Receiver, Sender)> {
        let (mut rx, mut tx) = channel(server, address).await;
        tx.json(&Auth {
            credential: credential.into(),
            claim: false,
            name: String::new(),
        })
        .await?;
        let _: Authorized = rx.json().await?;
        tx.json(&RequestHead {
            method: if path.ends_with("echo") {
                "POST"
            } else {
                "GET"
            }
            .into(),
            path: path.into(),
            headers: vec![],
        })
        .await?;
        for bytes in body.chunks(CHUNK) {
            tx.frame(DATA, bytes).await?;
        }
        tx.frame(END, &[]).await?;
        let head = rx.json().await?;
        Ok((head, rx, tx))
    }
    #[tokio::test]
    async fn pairing_is_single_use_rotates_and_expires() {
        let (_dir, server, address) = fixture().await;
        let old = server.offer().await.unwrap();
        let fresh = server.offer().await.unwrap();
        assert!(claim(&server, address, &old.uri).await.is_err());
        assert!(claim(&server, address, &fresh.uri).await.is_ok());
        assert!(claim(&server, address, &fresh.uri).await.is_err());
        let expired = server.offer().await.unwrap();
        server.state.lock().await.pending.as_mut().unwrap().expires = now() - 1;
        assert!(claim(&server, address, &expired.uri).await.is_err());
        server.stop().await;
    }
    #[tokio::test]
    async fn binary_roundtrip_streaming_and_account_auth_are_preserved() {
        let (_dir, server, address) = fixture().await;
        let credential = claim(&server, address, &server.offer().await.unwrap().uri)
            .await
            .unwrap();
        let body: Vec<u8> = (0..100_000).map(|n| (n % 256) as u8).collect();
        let (head, mut rx, _tx) = send(&server, address, &credential, "/api/v2/echo", &body)
            .await
            .unwrap();
        assert_eq!(head.status, 200);
        let mut result = vec![];
        loop {
            let (kind, data) = rx.frame().await.unwrap();
            if kind == END {
                break;
            }
            if kind == DATA {
                result.extend(data)
            }
        }
        assert_eq!(result, body);
        let (head, _, _) = send(&server, address, &credential, "/api/v2/private", &[])
            .await
            .unwrap();
        assert_eq!(
            head.status, 401,
            "Pairing must not bypass account authentication"
        );
        let (head, mut rx, _tx) = send(&server, address, &credential, "/api/v2/events", &[])
            .await
            .unwrap();
        assert!(head
            .headers
            .iter()
            .any(|(k, v)| k == "content-type" && v == "text/event-stream"));
        loop {
            let (kind, data) = rx.frame().await.unwrap();
            if kind == DATA {
                assert!(String::from_utf8(data).unwrap().contains("id: stream:1"));
                break;
            }
        }
        let id = server.status().await.unwrap().devices[0].id.clone();
        server.revoke(&id).await.unwrap();
        assert!(
            timeout(Duration::from_secs(2), async {
                while rx.frame().await.is_ok() {}
            })
            .await
            .is_ok(),
            "Revocation must close an existing stream"
        );
        assert!(send(&server, address, &credential, "/api/v2/private", &[])
            .await
            .is_err());
        server.stop().await;
    }
    #[tokio::test]
    async fn persisted_pairing_survives_restart_but_listener_starts_off() {
        let (dir, server, address) = fixture().await;
        let credential = claim(&server, address, &server.offer().await.unwrap().uri)
            .await
            .unwrap();
        server.stop().await;
        let restarted =
            MobileServer::load(dir.path().join("devices.json"), server.router.clone()).unwrap();
        assert!(restarted.status().await.unwrap().endpoint.is_none());
        let auth = Auth {
            credential,
            claim: false,
            name: String::new(),
        };
        assert!(restarted.authorize(auth).await.is_ok());
        let stored = std::fs::read_to_string(dir.path().join("devices.json")).unwrap();
        assert!(!stored.contains("account-token"));
    }
    #[tokio::test]
    async fn wrong_server_key_unpaired_clients_and_non_api_paths_are_rejected() {
        let (_dir, server, address) = fixture().await;
        assert!(connect(address, &keypair().unwrap().public).await.is_err());
        assert!(
            send(&server, address, &secret().unwrap(), "/api/v2/private", &[])
                .await
                .is_err()
        );
        let credential = claim(&server, address, &server.offer().await.unwrap().uri)
            .await
            .unwrap();
        for path in [
            "/",
            "http://127.0.0.1:8765/api/v2/private",
            "/api/v2/private#fragment",
        ] {
            assert!(send(&server, address, &credential, path, &[])
                .await
                .is_err());
        }
        server.stop().await;
    }
    #[tokio::test]
    async fn stop_disconnects_streams_and_invalidates_pending_offer() {
        let (_dir, server, address) = fixture().await;
        let credential = claim(&server, address, &server.offer().await.unwrap().uri)
            .await
            .unwrap();
        let (_, mut rx, _tx) = send(&server, address, &credential, "/api/v2/events", &[])
            .await
            .unwrap();
        server.offer().await.unwrap();
        server.stop().await;
        assert!(server.state.lock().await.pending.is_none());
        assert!(timeout(Duration::from_secs(2), async {
            while rx.frame().await.is_ok() {}
        })
        .await
        .is_ok());
    }
}
#[derive(Clone, Serialize)]
pub struct Interface {
    pub name: String,
    pub address: String,
}
#[derive(Serialize)]
pub struct Status {
    pub endpoint: Option<String>,
    pub interfaces: Vec<Interface>,
    pub devices: Vec<Device>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub uri: String,
    pub expires_at: u64,
}

pub struct MobileServer {
    state: AsyncMutex<State>,
    path: PathBuf,
    router: Router,
}
impl MobileServer {
    pub fn load(path: PathBuf, router: Router) -> Result<Arc<Self>> {
        let stored: Stored = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?)?
        } else {
            let pair = keypair()?;
            Stored {
                private_key: encode(&pair.private),
                public_key: encode(&pair.public),
                devices: vec![],
            }
        };
        ensure!(
            decode(&stored.private_key)?.len() == 32 && decode(&stored.public_key)?.len() == 32,
            "Invalid mobile identity"
        );
        let server = Arc::new(Self {
            state: AsyncMutex::new(State {
                stored: stored.clone(),
                pending: None,
                listener: None,
                devices: HashMap::new(),
            }),
            path,
            router,
        });
        server.save(&stored)?;
        Ok(server)
    }
    fn save(&self, stored: &Stored) -> Result<()> {
        use std::io::Write;
        let parent = self
            .path
            .parent()
            .context("Missing mobile state directory")?;
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        temp.write_all(&serde_json::to_vec(stored)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path)?;
        Ok(())
    }
    pub fn interfaces() -> Result<Vec<Interface>> {
        let mut found = vec![];
        for entry in if_addrs::get_if_addrs()? {
            let name = entry.name.to_lowercase();
            if [
                "docker",
                "wsl",
                "vethernet",
                "vmware",
                "virtualbox",
                "vbox",
                "singbox",
                "sing-box",
                "tun",
                "loopback",
            ]
            .iter()
            .any(|part| name.contains(part))
            {
                continue;
            }
            if let std::net::IpAddr::V4(ip) = entry.ip() {
                if ip.is_private() {
                    found.push(Interface {
                        name: entry.name,
                        address: ip.to_string(),
                    });
                }
            }
        }
        found.sort_by(|a, b| a.address.cmp(&b.address));
        found.dedup_by(|a, b| a.address == b.address);
        Ok(found)
    }
    pub async fn status(&self) -> Result<Status> {
        let state = self.state.lock().await;
        Ok(Status {
            endpoint: state.listener.as_ref().map(|(a, _)| a.to_string()),
            interfaces: Self::interfaces()?,
            devices: state
                .stored
                .devices
                .iter()
                .map(|d| d.device.clone())
                .collect(),
        })
    }
    pub async fn start(self: &Arc<Self>, address: &str) -> Result<()> {
        ensure!(
            Self::interfaces()?.iter().any(|i| i.address == address),
            "Select an available LAN interface"
        );
        self.bind(SocketAddr::new(address.parse::<Ipv4Addr>()?.into(), PORT))
            .await
    }
    async fn bind(self: &Arc<Self>, address: SocketAddr) -> Result<()> {
        let mut state = self.state.lock().await;
        ensure!(
            state.listener.is_none(),
            "Stop sharing before changing interfaces"
        );
        // Publish only after the OS has successfully bound the selected interface.
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let cancel = CancellationToken::new();
        state.listener = Some((address, cancel.clone()));
        let server = self.clone();
        tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(64));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = listener.accept() => {
                        let Ok((stream, _)) = result else { break };
                        let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
                        let server = server.clone();
                        let cancel = cancel.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            // No credentials, pairing codes, or request contents in logs.
                            tokio::select! { _ = cancel.cancelled() => {}, _ = server.connection(stream) => {} }
                        });
                    }
                }
            }
        });
        Ok(())
    }
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        if let Some((_, cancel)) = state.listener.take() {
            cancel.cancel();
        }
        for (_, active) in state.devices.drain() {
            active.cancel.cancel();
        }
        state.pending = None;
    }
    pub async fn offer(&self) -> Result<Pairing> {
        let mut state = self.state.lock().await;
        let endpoint = state
            .listener
            .as_ref()
            .context("Enable LAN sharing first")?
            .0
            .to_string();
        ensure!(
            state.stored.devices.len() < 32,
            "Remove an old device before pairing another"
        );
        let code = secret()?;
        let expires = now() + 120;
        let uri = Offer {
            v: 1,
            endpoint,
            public_key: state.stored.public_key.clone(),
            code: code.clone(),
        }
        .uri()?;
        state.pending = Some(Pending {
            hash: hash(&code),
            expires,
        });
        Ok(Pairing {
            uri,
            expires_at: expires,
        })
    }
    pub async fn revoke(&self, id: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut next = state.stored.clone();
        next.devices.retain(|d| d.device.id != id);
        self.save(&next)?;
        state.stored = next;
        if let Some(active) = state.devices.remove(id) {
            active.cancel.cancel();
        }
        Ok(())
    }
    async fn authorize(
        &self,
        auth: Auth,
    ) -> Result<(Authorized, CancellationToken, Arc<Semaphore>)> {
        ensure!(auth.credential.len() <= 128, "Invalid credential");
        let mut state = self.state.lock().await;
        let credential_hash = hash(&auth.credential);
        let (id, credential) = if auth.claim {
            let pending = state
                .pending
                .as_ref()
                .context("Pairing code expired or already used")?;
            ensure!(
                pending.expires > now() && pending.hash == credential_hash,
                "Pairing code expired or invalid"
            );
            ensure!(state.stored.devices.len() < 32, "Device limit reached");
            let credential = secret()?;
            let id = secret()?;
            let name: String = auth
                .name
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            let device = Device {
                id: id.clone(),
                name: if name.trim().is_empty() {
                    "Android".into()
                } else {
                    name
                },
                created: now(),
            };
            let mut next = state.stored.clone();
            next.devices.push(Registered {
                device,
                hash: hash(&credential),
            });
            self.save(&next)?;
            state.stored = next;
            state.pending = None;
            (id, Some(credential))
        } else {
            (
                state
                    .stored
                    .devices
                    .iter()
                    .find(|d| d.hash == credential_hash)
                    .context("Device is not paired or was revoked")?
                    .device
                    .id
                    .clone(),
                None,
            )
        };
        let active = state.devices.entry(id).or_insert_with(|| Active {
            cancel: CancellationToken::new(),
            slots: Arc::new(Semaphore::new(24)),
        });
        Ok((
            Authorized { credential },
            active.cancel.clone(),
            active.slots.clone(),
        ))
    }
    async fn connection(&self, stream: TcpStream) -> Result<()> {
        let key = decode(&self.state.lock().await.stored.private_key)?;
        let (mut rx, mut tx) = accept(stream, &key).await?;
        let auth: Auth = timeout(Duration::from_secs(10), rx.json()).await??;
        let claim = auth.claim;
        let (authorized, cancel, slots) = self.authorize(auth).await?;
        let _permit = slots.try_acquire_owned()?;
        tx.json(&authorized).await?;
        if claim {
            return Ok(());
        }
        tokio::select! {
            _ = cancel.cancelled() => Ok(()),
            result = self.serve_request(rx, tx) => result,
        }
    }
    async fn serve_request(&self, mut rx: Receiver, mut tx: Sender) -> Result<()> {
        let request = timeout(Duration::from_secs(45), async {
            let head: RequestHead = rx.json().await?;
            ensure!(
                head.path.starts_with("/api/v2/") && !head.path.contains('#'),
                "Only API v2 routes are allowed"
            );
            let uri: Uri = head.path.parse()?;
            ensure!(
                uri.scheme().is_none() && uri.authority().is_none(),
                "Absolute URLs are forbidden"
            );
            ensure!(
                ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]
                    .contains(&head.method.as_str()),
                "Unsupported method"
            );
            let mut request = Request::builder().method(head.method.as_str()).uri(uri);
            for (key, value) in head.headers {
                if ["authorization", "content-type", "accept", "last-event-id"]
                    .contains(&key.to_ascii_lowercase().as_str())
                {
                    request = request.header(key, value);
                }
            }
            let mut body = vec![];
            loop {
                let (kind, bytes) = rx.frame().await?;
                if kind == END {
                    ensure!(bytes.is_empty(), "Invalid end marker");
                    break;
                }
                ensure!(
                    kind == DATA && body.len() + bytes.len() <= MAX_BODY,
                    "Invalid or oversized upload"
                );
                body.extend(bytes);
            }
            anyhow::Ok(request.body(Body::from(body))?)
        })
        .await??;
        // A disconnected phone must release a long-running response, including SSE.
        tokio::select! {
            _ = rx.disconnected() => Ok(()),
            result = async {
                let response = timeout(Duration::from_secs(45), self.router.clone().oneshot(request)).await??;
                let (parts, mut body) = response.into_parts();
                tx.json(&ResponseHead { status: parts.status.as_u16(), headers: parts.headers.iter().filter_map(|(k,v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string()))).collect() }).await?;
                let mut pulse = tokio::time::interval(Duration::from_secs(20));
                loop {
                    tokio::select! {
                        _ = pulse.tick() => tx.frame(PING, &[]).await?,
                        frame = std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)) => {
                            let Some(frame) = frame else { break };
                            if let Ok(data) = frame?.into_data() {
                                for chunk in data.chunks(CHUNK) { tx.frame(DATA, chunk).await?; }
                            }
                        }
                    }
                }
                tx.frame(END, &[]).await?;
                anyhow::Ok(())
            } => result
        }
    }
}
