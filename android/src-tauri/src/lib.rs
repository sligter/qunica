#[cfg(target_os = "android")]
mod mobile {
    use tauri::{plugin::PluginHandle, Manager, State, Wry};

    struct SecureSession(PluginHandle<Wry>);

    #[tauri::command]
    async fn mobile_session_read(session: State<'_, SecureSession>) -> Result<serde_json::Value, String> {
        session.0.run_mobile_plugin("readSession", serde_json::json!({})).map_err(|e| e.to_string())
    }

    #[tauri::command]
    async fn mobile_session_write(session: State<'_, SecureSession>, value: String) -> Result<serde_json::Value, String> {
        session.0.run_mobile_plugin("writeSession", serde_json::json!({ "value": value })).map_err(|e| e.to_string())
    }

    #[tauri::mobile_entry_point]
    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri::plugin::Builder::<Wry>::new("secure-session")
                .setup(|app, api| {
                    let handle = api.register_android_plugin("app.qunica.mobile", "SecureSessionPlugin")?;
                    app.manage(SecureSession(handle));
                    Ok(())
                })
                .on_navigation(|_, url| {
                    // The native bridge belongs only to the bundled UI, never to a remote page.
                    url.scheme() == "https" && url.host_str() == Some("tauri.localhost")
                }).build())
            .invoke_handler(tauri::generate_handler![mobile_session_read, mobile_session_write])
            .run(tauri::generate_context!())
            .expect("unable to start Qunica Android");
    }
}

#[cfg(target_os = "android")]
pub use mobile::run;
