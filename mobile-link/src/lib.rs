//! Certificate-free, pinned Noise transport. HTTP semantics stay above this layer.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    time::timeout,
};

#[cfg(feature = "server")]
pub mod server;

const PATTERN: &str = "Noise_NK_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"qunica-mobile-link/v1";
pub const CHUNK: usize = 32 * 1024;
pub const MAX_BODY: usize = 32 * 1024 * 1024;
pub const DEADLINE: Duration = Duration::from_secs(60);
pub const JSON: u8 = 0;
pub const DATA: u8 = 1;
pub const END: u8 = 2;
pub const PING: u8 = 3;

pub fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}
pub fn decode(value: &str) -> Result<Vec<u8>> {
    Ok(URL_SAFE_NO_PAD.decode(value)?)
}
pub fn keypair() -> Result<snow::Keypair> {
    Ok(snow::Builder::new(PATTERN.parse()?).generate_keypair()?)
}
pub fn secret() -> Result<String> {
    Ok(encode(&keypair()?.private))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Offer {
    pub v: u8,
    pub endpoint: String,
    pub public_key: String,
    pub code: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Connection {
    pub endpoint: String,
    pub public_key: String,
    pub credential: String,
}

/// Account handoff is delivered once over Noise, never inside the QR or device identity.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paired {
    #[serde(flatten)]
    pub connection: Connection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_token: Option<String>,
}

/// Only numeric RFC1918 addresses: no DNS rebinding, proxy routing or public endpoint.
pub fn lan_address(endpoint: &str) -> Result<SocketAddr> {
    let addr: SocketAddr = endpoint
        .parse()
        .context("Expected a LAN IPv4 address and port")?;
    ensure!(
        matches!(addr.ip(), std::net::IpAddr::V4(ip) if ip.is_private()),
        "Only LAN IPv4 addresses are supported"
    );
    ensure!(addr.port() > 0, "Invalid port");
    Ok(addr)
}

/// A destination is only a route. Noise's pinned key remains the server identity.
/// No URL syntax, local-only destinations, or ambiguous numeric host spellings.
pub fn endpoint_parts(endpoint: &str) -> Result<(String, u16)> {
    ensure!(endpoint.len() <= 260, "Address is too long");
    if let Ok(address) = endpoint.parse::<SocketAddr>() {
        ensure!(
            address.port() != 0 && usable_ip(address.ip()),
            "Invalid destination address"
        );
        return Ok((address.ip().to_string(), address.port()));
    }
    let (host, port) = endpoint
        .rsplit_once(':')
        .context("Use host:port, without http://")?;
    let port: u16 = port.parse().context("Invalid port")?;
    ensure!(
        port > 0 && !host.is_empty() && host.len() <= 253,
        "Invalid host or port"
    );
    ensure!(
        host.contains('.') && host.bytes().any(|b| b.is_ascii_alphabetic()),
        "Use a full DNS name or IP address"
    );
    ensure!(
        host.split('.').all(|label| !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')),
        "Invalid DNS name"
    );
    let host = host.to_ascii_lowercase();
    ensure!(
        !host.ends_with(".localhost") && !host.ends_with(".local"),
        "Use a routable destination"
    );
    Ok((host, port))
}

fn usable_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_broadcast()
                && ip.octets()[0] != 0
        }
        std::net::IpAddr::V6(ip) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (ip.segments()[0] & 0xffc0) != 0xfe80
                && ip.to_ipv4_mapped().is_none()
        }
    }
}

async fn connect_endpoint(endpoint: &str, key: &[u8]) -> Result<(Receiver, Sender)> {
    let (host, port) = endpoint_parts(endpoint)?;
    timeout(Duration::from_secs(15), async {
        let addresses = tokio::net::lookup_host((host.as_str(), port)).await?;
        // Resolve once per connection; validate and connect the exact resolved IP.
        // Never retry an HTTP operation here: only the Noise handshake is retried.
        for address in addresses.filter(|a| usable_ip(a.ip())).take(8) {
            if let Ok(Ok(connection)) = timeout(Duration::from_secs(3), connect(address, key)).await
            {
                return Ok(connection);
            }
        }
        anyhow::bail!("Cannot reach the paired desktop at this address")
    })
    .await?
}

pub async fn verify_connection(connection: &Connection) -> Result<()> {
    let (mut rx, mut tx) =
        connect_endpoint(&connection.endpoint, &decode(&connection.public_key)?).await?;
    tx.json(&Auth {
        credential: connection.credential.clone(),
        claim: false,
        name: String::new(),
        device_info: None,
    })
    .await?;
    let _: Authorized = rx.json().await?;
    Ok(())
}
impl Offer {
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(value.len() < 2048, "Pairing code is too large");
        let raw = value
            .trim()
            .strip_prefix("qunica://pair?data=")
            .context("Scan a Qunica pairing code")?;
        let offer: Self = serde_json::from_slice(&decode(raw)?)?;
        match offer.v {
            1 => {
                lan_address(&offer.endpoint)?;
            }
            2 => {
                endpoint_parts(&offer.endpoint)?;
            }
            _ => anyhow::bail!("Unsupported pairing version; update the app"),
        }
        ensure!(
            decode(&offer.public_key)?.len() == 32 && decode(&offer.code)?.len() == 32,
            "Invalid pairing key"
        );
        Ok(offer)
    }
    pub fn uri(&self) -> Result<String> {
        Ok(format!(
            "qunica://pair?data={}",
            encode(&serde_json::to_vec(self)?)
        ))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Auth {
    pub credential: String,
    pub claim: bool,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub system_version: String,
    pub sdk_version: u32,
    pub app_version: String,
}
#[derive(Serialize, Deserialize)]
pub struct Authorized {
    pub credential: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_token: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

async fn read_wire<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<Vec<u8>> {
    let len = stream.read_u16().await? as usize;
    ensure!(len > 0, "Empty wire frame");
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn write_wire<W: AsyncWriteExt + Unpin>(stream: &mut W, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= u16::MAX as usize,
        "Frame exceeds limit"
    );
    stream.write_u16(bytes.len() as u16).await?;
    stream.write_all(bytes).await?;
    Ok(())
}

pub struct Receiver {
    stream: OwnedReadHalf,
    noise: Arc<Mutex<snow::TransportState>>,
}
pub struct Sender {
    stream: OwnedWriteHalf,
    noise: Arc<Mutex<snow::TransportState>>,
}
impl Receiver {
    /// After a complete upload, the client sends no more frames. Wait without
    /// an idle deadline; the response may be an indefinitely open SSE stream.
    pub async fn disconnected(&mut self) {
        let _ = self.stream.read_u8().await;
    }
    pub async fn frame(&mut self) -> Result<(u8, Vec<u8>)> {
        let bytes = timeout(DEADLINE, read_wire(&mut self.stream)).await??;
        let mut out = vec![0; u16::MAX as usize];
        let n = self
            .noise
            .lock()
            .map_err(|_| anyhow::anyhow!("Transport unavailable"))?
            .read_message(&bytes, &mut out)?;
        ensure!(n > 0 && n <= CHUNK + 1, "Invalid encrypted frame size");
        Ok((out[0], out[1..n].to_vec()))
    }
    pub async fn json<T: serde::de::DeserializeOwned>(&mut self) -> Result<T> {
        let (kind, body) = self.frame().await?;
        ensure!(kind == JSON, "Expected metadata");
        Ok(serde_json::from_slice(&body)?)
    }
}
impl Sender {
    pub async fn frame(&mut self, kind: u8, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= CHUNK, "Frame exceeds limit");
        let mut plain = Vec::with_capacity(bytes.len() + 1);
        plain.push(kind);
        plain.extend_from_slice(bytes);
        let mut out = vec![0; plain.len() + 16];
        let n = self
            .noise
            .lock()
            .map_err(|_| anyhow::anyhow!("Transport unavailable"))?
            .write_message(&plain, &mut out)?;
        timeout(DEADLINE, write_wire(&mut self.stream, &out[..n])).await??;
        Ok(())
    }
    pub async fn json<T: Serialize>(&mut self, value: &T) -> Result<()> {
        self.frame(JSON, &serde_json::to_vec(value)?).await
    }
}
fn split(stream: TcpStream, noise: snow::HandshakeState) -> Result<(Receiver, Sender)> {
    let noise = Arc::new(Mutex::new(noise.into_transport_mode()?));
    let (read, write) = stream.into_split();
    Ok((
        Receiver {
            stream: read,
            noise: noise.clone(),
        },
        Sender {
            stream: write,
            noise,
        },
    ))
}
pub async fn connect(endpoint: SocketAddr, public_key: &[u8]) -> Result<(Receiver, Sender)> {
    timeout(Duration::from_secs(10), async {
        let mut stream = TcpStream::connect(endpoint).await?;
        stream.set_nodelay(true)?;
        let mut noise = snow::Builder::new(PATTERN.parse()?)
            .prologue(PROLOGUE)?
            .remote_public_key(public_key)?
            .build_initiator()?;
        let mut buf = [0; 256];
        let len = noise.write_message(&[], &mut buf)?;
        write_wire(&mut stream, &buf[..len]).await?;
        noise.read_message(&read_wire(&mut stream).await?, &mut buf)?;
        split(stream, noise)
    })
    .await?
}
pub async fn accept(mut stream: TcpStream, private_key: &[u8]) -> Result<(Receiver, Sender)> {
    timeout(Duration::from_secs(10), async {
        stream.set_nodelay(true)?;
        let mut noise = snow::Builder::new(PATTERN.parse()?)
            .prologue(PROLOGUE)?
            .local_private_key(private_key)?
            .build_responder()?;
        let mut buf = [0; 256];
        noise.read_message(&read_wire(&mut stream).await?, &mut buf)?;
        let len = noise.write_message(&[], &mut buf)?;
        write_wire(&mut stream, &buf[..len]).await?;
        split(stream, noise)
    })
    .await?
}

pub async fn pair(offer: Offer, name: String) -> Result<Paired> {
    pair_with_device(offer, name, None).await
}

pub async fn pair_with_device(offer: Offer, name: String, device_info: Option<DeviceInfo>) -> Result<Paired> {
    let (mut rx, mut tx) = connect_endpoint(&offer.endpoint, &decode(&offer.public_key)?).await?;
    tx.json(&Auth {
        credential: offer.code,
        claim: true,
        name,
        device_info,
    })
    .await?;
    let auth: Authorized = rx.json().await?;
    Ok(Paired { connection: Connection {
        endpoint: offer.endpoint,
        public_key: offer.public_key,
        credential: auth.credential.context("Pairing was rejected")?,
    }, account_token: auth.account_token })
}

pub async fn request(
    connection: &Connection,
    head: RequestHead,
    body: &[u8],
) -> Result<(ResponseHead, Receiver, Sender)> {
    ensure!(body.len() <= MAX_BODY, "Upload exceeds 32 MiB");
    let (mut rx, mut tx) =
        connect_endpoint(&connection.endpoint, &decode(&connection.public_key)?).await?;
    tx.json(&Auth {
        credential: connection.credential.clone(),
        claim: false,
        name: String::new(),
        device_info: None,
    })
    .await?;
    let _: Authorized = rx.json().await?;
    tx.json(&head).await?;
    for chunk in body.chunks(CHUNK) {
        tx.frame(DATA, chunk).await?;
    }
    tx.frame(END, &[]).await?;
    let head = rx.json().await?;
    Ok((head, rx, tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_offers_accept_routable_destinations_but_keep_legacy_lan_rules() {
        for endpoint in [
            "203.0.113.8:18766",
            "relay.example.com:18766",
            "[2001:db8::1]:18766",
            "192.168.1.2:8766",
        ] {
            assert!(endpoint_parts(endpoint).is_ok(), "{endpoint}");
            let offer = Offer {
                v: 2,
                endpoint: endpoint.into(),
                public_key: encode(&keypair().unwrap().public),
                code: secret().unwrap(),
            };
            assert!(Offer::parse(&offer.uri().unwrap()).is_ok());
            if lan_address(endpoint).is_err() {
                assert!(Offer::parse(&Offer { v: 1, ..offer }.uri().unwrap()).is_err());
            }
        }
        for endpoint in [
            "http://example.com:8766",
            "user@example.com:8766",
            "example.com:0",
            "example.com:65536",
            "example.com:8766/path",
            "example.com:8766?token=x",
            "example.com:8766#x",
            " example.com:8766",
            "localhost:8766",
            "a.localhost:8766",
            "a.local:8766",
            "-a.example:8766",
            "a..example:8766",
            "127.0.0.1:8766",
            "0.0.0.0:8766",
            "169.254.169.254:80",
            "224.0.0.1:8766",
            "255.255.255.255:8766",
            "[::1]:8766",
            "[::ffff:127.0.0.1]:8766",
            "[fe80::1]:8766",
            "2130706433:8766",
        ] {
            assert!(endpoint_parts(endpoint).is_err(), "{endpoint}");
        }
    }
    #[test]
    fn pairing_parser_rejects_public_loopback_dns_and_extra_fields() {
        for address in [
            "8.8.8.8:8766",
            "127.0.0.1:8766",
            "100.64.0.2:8766",
            "example.com:8766",
            "[::1]:8766",
            "192.168.1.2:0",
        ] {
            assert!(lan_address(address).is_err(), "{address}");
        }
        let pair = keypair().unwrap();
        let offer = Offer {
            v: 1,
            endpoint: "192.168.1.2:8766".into(),
            public_key: encode(&pair.public),
            code: secret().unwrap(),
        };
        assert!(Offer::parse(&offer.uri().unwrap()).is_ok());
        let mut value = serde_json::to_value(&offer).unwrap();
        value["token"] = "unexpected".into();
        assert!(Offer::parse(&format!(
            "qunica://pair?data={}",
            encode(&serde_json::to_vec(&value).unwrap())
        ))
        .is_err());
        assert!(Offer::parse("https://example.com/").is_err());
    }
    fn cipher_pair() -> (snow::TransportState, snow::TransportState) {
        let keys = keypair().unwrap();
        let mut client = snow::Builder::new(PATTERN.parse().unwrap())
            .prologue(PROLOGUE)
            .unwrap()
            .remote_public_key(&keys.public)
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut server = snow::Builder::new(PATTERN.parse().unwrap())
            .prologue(PROLOGUE)
            .unwrap()
            .local_private_key(&keys.private)
            .unwrap()
            .build_responder()
            .unwrap();
        let mut buf = [0; 256];
        let mut plain = [0; 256];
        let n = client.write_message(&[], &mut buf).unwrap();
        server.read_message(&buf[..n], &mut plain).unwrap();
        let n = server.write_message(&[], &mut buf).unwrap();
        client.read_message(&buf[..n], &mut plain).unwrap();
        (
            client.into_transport_mode().unwrap(),
            server.into_transport_mode().unwrap(),
        )
    }
    #[test]
    fn encrypted_messages_reject_tampering_replay_and_wrong_direction() {
        let (mut client, mut server) = cipher_pair();
        let mut encrypted = [0; 256];
        let mut plain = [0; 256];
        let n = client
            .write_message(b"private approval", &mut encrypted)
            .unwrap();
        assert!(!encrypted[..n].windows(7).any(|v| v == b"private"));
        assert!(client.read_message(&encrypted[..n], &mut plain).is_err());
        let mut tampered = encrypted;
        tampered[0] ^= 1;
        assert!(server.read_message(&tampered[..n], &mut plain).is_err());
        assert_eq!(
            server.read_message(&encrypted[..n], &mut plain).unwrap(),
            16
        );
        assert!(server.read_message(&encrypted[..n], &mut plain).is_err());
    }
}
