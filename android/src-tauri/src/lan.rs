use qunica_mobile_link::{self as link, Connection, RequestHead, ResponseHead};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::State;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

struct Request {
    connection: Connection,
    cancel: CancellationToken,
    stream: AsyncMutex<Option<(link::Receiver, link::Sender)>>,
}
#[derive(Default)]
pub struct Lan {
    connection: Mutex<Option<Connection>>,
    requests: Mutex<HashMap<String, Arc<Request>>>,
}
impl Lan {
    fn get(&self, id: &str) -> Result<Arc<Request>, String> {
        self.requests
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| "Request was closed".into())
    }
    fn close(&self, id: &str) {
        if let Some(request) = self.requests.lock().unwrap().remove(id) {
            request.cancel.cancel();
        }
    }
}
pub async fn pair(offer: String, info: link::DeviceInfo) -> Result<link::Paired, String> {
    let offer = link::Offer::parse(&offer).map_err(|e| e.to_string())?;
    // Build.MANUFACTURER is commonly lowercase ("samsung"); Build.MODEL may already
    // carry the brand ("OnePlus PKX110"). Produce one readable label without repeats.
    let mut chars = info.manufacturer.trim().chars();
    let manufacturer = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    let model = info.model.trim();
    let name = if manufacturer.is_empty() || model.to_lowercase().starts_with(&manufacturer.to_lowercase()) {
        model.to_string()
    } else { format!("{manufacturer} {model}") };
    link::pair_with_device(offer, name, Some(info))
        .await
        .map_err(|_| "Pairing failed. Check desktop sharing, LAN or VPS relay connectivity and generate a fresh QR code.".into())
}
#[tauri::command]
pub async fn mobile_lan_verify(connection: Connection) -> Result<(), String> {
    link::verify_connection(&connection).await
        .map_err(|_| "Cannot verify the paired desktop at this address. Check the relay and sharing mode.".into())
}
#[tauri::command]
pub fn mobile_lan_configure(
    state: State<'_, Lan>,
    connection: Option<Connection>,
) -> Result<(), String> {
    if let Some(c) = &connection {
        link::endpoint_parts(&c.endpoint).map_err(|e| e.to_string())?;
        if link::decode(&c.public_key)
            .map_err(|e| e.to_string())?
            .len()
            != 32
            || link::decode(&c.credential)
                .map_err(|e| e.to_string())?
                .len()
                != 32
        {
            return Err("Invalid pairing identity".into());
        }
    }
    for (_, request) in state.requests.lock().unwrap().drain() {
        request.cancel.cancel();
    }
    *state.connection.lock().unwrap() = connection;
    Ok(())
}
#[tauri::command]
pub fn mobile_lan_prepare(state: State<'_, Lan>) -> Result<String, String> {
    let connection = state
        .connection
        .lock()
        .unwrap()
        .clone()
        .ok_or("Pair a desktop first")?;
    let mut requests = state.requests.lock().unwrap();
    if requests.len() >= 64 {
        return Err("Too many mobile requests".into());
    }
    let id = link::secret().map_err(|e| e.to_string())?;
    requests.insert(
        id.clone(),
        Arc::new(Request {
            connection,
            cancel: CancellationToken::new(),
            stream: AsyncMutex::new(None),
        }),
    );
    Ok(id)
}
#[tauri::command]
pub async fn mobile_lan_open(
    state: State<'_, Lan>,
    id: String,
    head: RequestHead,
    body: String,
) -> Result<ResponseHead, String> {
    let request = state.get(&id)?;
    let result = async {
        if body.len() > link::MAX_BODY * 4 / 3 + 4 {
            return Err("Upload exceeds 32 MiB".into());
        }
        let body = link::decode(&body).map_err(|e| e.to_string())?;
        let (head, rx, tx) = link::request(&request.connection, head, &body)
            .await
            .map_err(|_| {
                "Encrypted connection failed. Check desktop sharing, pairing and LAN or VPS relay routing."
                    .to_string()
            })?;
        *request.stream.lock().await = Some((rx, tx));
        Ok(head)
    };
    let result = tokio::select! { _ = request.cancel.cancelled() => Err("Request cancelled".into()), result = result => result };
    if result.is_err() {
        state.close(&id);
    }
    result
}
#[derive(Serialize)]
pub struct Chunk {
    end: bool,
    data: String,
}
#[tauri::command]
pub async fn mobile_lan_read(state: State<'_, Lan>, id: String) -> Result<Chunk, String> {
    let request = state.get(&id)?;
    let result = async {
        let mut stream = request.stream.lock().await;
        let (rx, _tx) = stream.as_mut().ok_or("Response has not opened")?;
        loop {
            let (kind, data) = rx
                .frame()
                .await
                .map_err(|_| "Encrypted stream interrupted".to_string())?;
            match kind {
                link::DATA => {
                    return Ok(Chunk {
                        end: false,
                        data: link::encode(&data),
                    })
                }
                link::END if data.is_empty() => {
                    return Ok(Chunk {
                        end: true,
                        data: String::new(),
                    })
                }
                link::PING if data.is_empty() => continue,
                _ => return Err("Invalid response frame".into()),
            }
        }
    };
    let result = tokio::select! { _ = request.cancel.cancelled() => Err("Request cancelled".into()), result = result => result };
    if result.as_ref().map(|c| c.end).unwrap_or(true) {
        state.close(&id);
    }
    result
}
#[tauri::command]
pub fn mobile_lan_close(state: State<'_, Lan>, id: String) {
    state.close(&id);
}
