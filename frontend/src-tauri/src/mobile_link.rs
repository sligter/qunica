use qunica_mobile_link::server::{MobileServer, Pairing, Status};
use std::sync::Arc;
use tauri::State;

pub struct MobileLink(pub Result<Arc<MobileServer>, String>);
impl MobileLink {
    fn server(&self) -> Result<&Arc<MobileServer>, String> {
        self.0.as_ref().map_err(Clone::clone)
    }
}

#[tauri::command]
pub async fn mobile_link_status(state: State<'_, MobileLink>) -> Result<Status, String> {
    state.server()?.status().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn mobile_link_start(
    state: State<'_, MobileLink>,
    address: String,
    advertised_endpoint: Option<String>,
) -> Result<Status, String> {
    let server = state.server()?;
    if let Some(endpoint) = advertised_endpoint {
        server.start_relay(endpoint.trim()).await
    } else {
        server.start(&address).await
    }.map_err(|e| e.to_string())?;
    server.status().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn mobile_link_offer(state: State<'_, MobileLink>) -> Result<Pairing, String> {
    state.server()?.offer().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn mobile_link_stop(state: State<'_, MobileLink>) -> Result<Status, String> {
    let server = state.server()?;
    server.stop().await;
    server.status().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn mobile_link_revoke(
    state: State<'_, MobileLink>,
    id: String,
) -> Result<Status, String> {
    let server = state.server()?;
    server.revoke(&id).await.map_err(|e| e.to_string())?;
    server.status().await.map_err(|e| e.to_string())
}
