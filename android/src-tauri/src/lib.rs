mod lan;

#[cfg(target_os = "android")]
mod mobile {
    use tauri::{plugin::PluginHandle, Manager, State, Wry};

    struct SecureSession(PluginHandle<Wry>);
    struct FileExport(PluginHandle<Wry>);

    #[tauri::command]
    async fn mobile_file_export(export: State<'_, FileExport>, operation: String, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let method = match operation.as_str() {
            "begin" => "beginExport",
            "append" => "appendExport",
            "save" => "saveExport",
            "discard" => "discardExport",
            _ => return Err("Invalid file export operation".into()),
        };
        export.0.run_mobile_plugin_async(method, payload).await.map_err(|e| e.to_string())
    }

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
            .manage(crate::lan::Lan::default())
            .plugin(tauri_plugin_barcode_scanner::init())
            .plugin(tauri::plugin::Builder::<Wry>::new("file-export")
                .setup(|app, api| {
                    let handle = api.register_android_plugin("app.qunica.mobile", "FileExportPlugin")?;
                    app.manage(FileExport(handle));
                    Ok(())
                }).build())
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
            .invoke_handler(tauri::generate_handler![mobile_session_read, mobile_session_write, mobile_file_export,
                crate::lan::mobile_lan_pair, crate::lan::mobile_lan_configure, crate::lan::mobile_lan_verify,
                crate::lan::mobile_lan_prepare, crate::lan::mobile_lan_open,
                crate::lan::mobile_lan_read, crate::lan::mobile_lan_close])
            .run(tauri::generate_context!())
            .expect("unable to start Qunica Android");
    }
}

#[cfg(target_os = "android")]
pub use mobile::run;
