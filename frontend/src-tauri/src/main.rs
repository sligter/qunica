#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod terminal;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ag_swarmer_backend::{config::AppConfig, server, telemetry};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::ShellExt;
use tokio::sync::oneshot;

use terminal::manager::TerminalManager;
use terminal::native::NativePtySpawner;
use terminal::process_tree::taskkill_args;
use terminal::{
    terminal_close, terminal_close_all, terminal_create, terminal_resize, terminal_write,
};

struct BackendShutdown(std::sync::Mutex<Option<oneshot::Sender<()>>>);

impl Drop for BackendShutdown {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(sender) = guard.take() {
                let _ = sender.send(());
            }
        }
    }
}

const BACKEND_PORT: u16 = 8765;
const BACKEND_BASE_URL: &str = "http://127.0.0.1:8765";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_MAIN_ID: &str = "open-main";
const TRAY_OPEN_SETTINGS_ID: &str = "open-settings";
const TRAY_OPEN_LOGS_ID: &str = "open-logs";
const TRAY_EXIT_ID: &str = "exit";

#[tauri::command]
fn backend_base_url() -> &'static str {
    BACKEND_BASE_URL
}

#[tauri::command]
async fn pick_workspace_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| path.simplified().to_string()))
    })
    .await
    .map_err(|err| err.to_string())?
}

fn reveal_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        ProcessCommand::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open")
            .args(["-R", path])
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let target = Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        ProcessCommand::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

/// Reveal (and select) an absolute path in the OS file manager.
#[tauri::command]
async fn reveal_in_file_manager(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reveal_path(&path))
        .await
        .map_err(|err| err.to_string())?
}

/// Show a native "Save As" dialog and write the base64-encoded bytes to disk.
/// Returns the saved path, or None if the user cancelled.
#[tauri::command]
async fn save_file(
    app: tauri::AppHandle,
    name: String,
    contents_b64: String,
) -> Result<Option<String>, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(contents_b64.as_bytes())
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app.dialog().file().set_file_name(name).blocking_save_file();
        let Some(file_path) = chosen else {
            return Ok(None);
        };
        let path_str = file_path.simplified().to_string();
        std::fs::write(Path::new(&path_str), &bytes).map_err(|err| err.to_string())?;
        Ok(Some(path_str))
    })
    .await
    .map_err(|err| err.to_string())?
}

fn log_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

fn append_log(log_dir: &Path, file_name: &str, message: impl AsRef<str>) {
    let path = log_dir.join(file_name);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {}", log_timestamp(), message.as_ref());
    }
}

fn append_launcher_log(log_dir: &Path, message: impl AsRef<str>) {
    append_log(log_dir, "launcher.log", message);
}

fn append_backend_log(log_dir: &Path, message: impl AsRef<str>) {
    append_log(log_dir, "backend.log", message);
}

fn append_optional_launcher_log(log_dir: Option<&Path>, message: impl AsRef<str>) {
    if let Some(log_dir) = log_dir {
        append_launcher_log(log_dir, message);
    }
}

fn app_logs_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    let log_dir = app.path().app_data_dir()?.join("logs");
    fs::create_dir_all(&log_dir)?;
    Ok(log_dir)
}

fn open_logs_dir(app: &tauri::AppHandle) {
    if let Ok(log_dir) = app_logs_dir(app) {
        append_launcher_log(
            &log_dir,
            format!("opening logs directory: {}", log_dir.display()),
        );
        #[allow(deprecated)]
        if let Err(err) = app.shell().open(
            log_dir.to_string_lossy().to_string(),
            None::<tauri_plugin_shell::open::Program>,
        ) {
            append_launcher_log(&log_dir, format!("failed to open logs directory: {err}"));
        }
    }
}

fn shutdown_backend(app: &tauri::AppHandle) {
    let log_dir = app_logs_dir(app).ok();
    append_optional_launcher_log(log_dir.as_deref(), "shutting down backend");

    let state = app.state::<BackendShutdown>();
    let sender = state
        .0
        .lock()
        .expect("backend shutdown mutex poisoned")
        .take();
    if let Some(sender) = sender {
        let _ = sender.send(());
    } else {
        append_optional_launcher_log(
            log_dir.as_deref(),
            "backend shutdown requested but no in-process backend was recorded",
        );
    }
}

fn pids_listening_on_port(netstat_output: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pids = Vec::new();
    for line in netstat_output.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 5 || !parts[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        let local_address = parts[1];
        let state = parts[3];
        if !local_address.ends_with(&suffix) || !state.eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        if let Ok(pid) = parts[4].parse::<u32>() {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

#[cfg(target_os = "windows")]
fn terminate_process_tree_with_taskkill(log_dir: Option<&Path>, pid: u32) -> bool {
    append_optional_launcher_log(
        log_dir,
        format!("terminating process tree for stale backend listener PID {pid}"),
    );
    let args = taskkill_args(pid);
    match ProcessCommand::new("taskkill")
        .args(args.iter().map(String::as_str))
        .output()
    {
        Ok(result) => {
            append_optional_launcher_log(
                log_dir,
                format!(
                    "taskkill process tree PID {pid} status {:?}; stdout: {}; stderr: {}",
                    result.status.code(),
                    String::from_utf8_lossy(&result.stdout).trim(),
                    String::from_utf8_lossy(&result.stderr).trim()
                ),
            );
            result.status.success()
        }
        Err(err) => {
            append_optional_launcher_log(
                log_dir,
                format!("failed to taskkill process tree for PID {pid}: {err}"),
            );
            false
        }
    }
}

#[cfg(target_os = "windows")]
fn clear_backend_port(log_dir: &Path, port: u16) {
    append_launcher_log(log_dir, format!("clearing TCP port {port} before startup"));
    let output = match ProcessCommand::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            append_launcher_log(log_dir, format!("failed to run netstat: {err}"));
            return;
        }
    };
    if !output.status.success() {
        append_launcher_log(
            log_dir,
            format!(
                "netstat failed with status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        );
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_pid = std::process::id();
    let pids = pids_listening_on_port(&stdout, port)
        .into_iter()
        .filter(|pid| *pid != current_pid)
        .collect::<Vec<_>>();
    if pids.is_empty() {
        append_launcher_log(log_dir, format!("TCP port {port} is free"));
        return;
    }

    for pid in pids {
        append_launcher_log(
            log_dir,
            format!("killing stale PID {pid} listening on TCP port {port}"),
        );
        if !terminate_process_tree_with_taskkill(Some(log_dir), pid) {
            append_launcher_log(
                log_dir,
                format!("taskkill did not confirm TCP port {port} cleanup for PID {pid}"),
            );
        }
    }
    thread::sleep(Duration::from_millis(750));
}

#[cfg(not(target_os = "windows"))]
fn clear_backend_port(log_dir: &Path, port: u16) {
    append_launcher_log(log_dir, format!("port cleanup skipped for TCP port {port}"));
}

fn route_script(route: &str) -> String {
    let route_json = serde_json::to_string(route).expect("route is serializable");
    format!(
        "window.history.pushState(null, '', {route_json}); window.dispatchEvent(new PopStateEvent('popstate'));"
    )
}

fn open_route(app: &tauri::AppHandle, route: &str) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.eval(route_script(route));
    }
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    if app.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("/".into()))
        .title("AG Swarmer")
        .inner_size(1280.0, 800.0)
        .min_inner_size(1024.0, 680.0)
        .build()?;
    Ok(())
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_main =
        MenuItem::with_id(app, TRAY_OPEN_MAIN_ID, "Open Main Page", true, None::<&str>)?;
    let open_settings = MenuItem::with_id(
        app,
        TRAY_OPEN_SETTINGS_ID,
        "Open Settings",
        true,
        None::<&str>,
    )?;
    let open_logs = MenuItem::with_id(
        app,
        TRAY_OPEN_LOGS_ID,
        "Open Logs Directory",
        true,
        None::<&str>,
    )?;
    let exit = MenuItem::with_id(app, TRAY_EXIT_ID, "Exit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&open_main, &open_settings, &open_logs, &separator, &exit],
    )?;

    TrayIconBuilder::with_id("main-tray")
        .icon(tauri::include_image!("icons/32x32.png"))
        .tooltip("AG Swarmer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_MAIN_ID => open_route(app, "/groups"),
            TRAY_OPEN_SETTINGS_ID => open_route(app, "/settings/system"),
            TRAY_OPEN_LOGS_ID => open_logs_dir(app),
            TRAY_EXIT_ID => {
                shutdown_terminal_sessions(app);
                shutdown_backend(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => open_route(tray.app_handle(), "/groups"),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn shutdown_terminal_sessions(app: &tauri::AppHandle) {
    let manager = app.state::<TerminalManager>().inner().clone();
    let result = tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(move || {
        manager.close_all()
    }));
    match result {
        Ok(Ok(())) => tracing::info!(
            lifecycle = "close_all",
            forced_cleanup = true,
            "terminal sessions closed for application exit"
        ),
        Ok(Err(error)) => tracing::warn!(
            lifecycle = "close_all",
            error_code = %error.code,
            forced_cleanup = false,
            "terminal session cleanup failed during application exit"
        ),
        Err(_) => tracing::warn!(
            lifecycle = "close_all",
            error_code = "terminal.cleanup_join_failed",
            forced_cleanup = false,
            "terminal cleanup task failed during application exit"
        ),
    }
}

fn start_in_process_backend(
    app_data_dir: PathBuf,
    log_dir: PathBuf,
) -> Result<oneshot::Sender<()>, String> {
    let config = AppConfig::for_desktop_app_data(app_data_dir, BACKEND_PORT).map_err(|err| {
        let message = err.to_string();
        append_launcher_log(
            &log_dir,
            format!("failed to build backend config: {message}"),
        );
        message
    })?;
    if let Err(err) = telemetry::setup_tracing(&config) {
        append_launcher_log(
            &log_dir,
            format!("tracing already initialized or failed: {err}"),
        );
    }

    let server_config = server::ServerConfig::from(config);
    let (state, listener, addr) = tauri::async_runtime::block_on(async {
        let state = server::build_state(&server_config).await?;
        let (listener, addr) = server::bind_listener(&server_config).await?;
        anyhow::Ok((state, listener, addr))
    })
    .map_err(|err| {
        let message = format!("{err:#}");
        append_launcher_log(
            &log_dir,
            format!("failed to start in-process backend: {message}"),
        );
        message
    })?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let backend_log_dir = log_dir.clone();
    tauri::async_runtime::spawn(async move {
        append_backend_log(&backend_log_dir, "in-process backend starting");
        let result = server::serve_listener_with_shutdown(listener, addr, state, async move {
            let _ = shutdown_rx.await;
        })
        .await;
        match result {
            Ok(()) => append_backend_log(&backend_log_dir, "in-process backend stopped"),
            Err(err) => append_backend_log(
                &backend_log_dir,
                format!("in-process backend stopped with error: {err:#}"),
            ),
        }
    });

    append_launcher_log(&log_dir, "backend task spawned");
    Ok(shutdown_tx)
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(BackendShutdown(std::sync::Mutex::new(None)))
        .manage(TerminalManager::new(Arc::new(NativePtySpawner)))
        .invoke_handler(tauri::generate_handler![
            backend_base_url,
            pick_workspace_folder,
            reveal_in_file_manager,
            save_file,
            terminal_create,
            terminal_write,
            terminal_resize,
            terminal_close,
            terminal_close_all
        ])
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&app_data_dir)?;
            let log_dir = app_data_dir.join("logs");
            fs::create_dir_all(&log_dir)?;
            append_launcher_log(&log_dir, "launcher starting");
            append_launcher_log(
                &log_dir,
                format!("app data directory: {}", app_data_dir.display()),
            );
            append_launcher_log(&log_dir, format!("logs directory: {}", log_dir.display()));

            create_tray(app)?;
            clear_backend_port(&log_dir, BACKEND_PORT);
            let shutdown = start_in_process_backend(app_data_dir, log_dir.clone())?;
            let state = app.state::<BackendShutdown>();
            *state.0.lock().expect("backend shutdown mutex poisoned") = Some(shutdown);
            create_main_window(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            shutdown_terminal_sessions(app);
            shutdown_backend(app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{pids_listening_on_port, taskkill_args};

    #[test]
    fn parses_windows_netstat_listeners_for_exact_port() {
        let output = r#"
  TCP    127.0.0.1:8765         0.0.0.0:0              LISTENING       27672
  TCP    127.0.0.1:18765        0.0.0.0:0              LISTENING       11111
  TCP    [::1]:8765             [::]:0                 LISTENING       27672
  TCP    127.0.0.1:8765         127.0.0.1:50000        ESTABLISHED     22222
"#;

        assert_eq!(pids_listening_on_port(output, 8765), vec![27672]);
    }

    #[test]
    fn builds_taskkill_args_for_process_tree() {
        assert_eq!(
            taskkill_args(27672),
            ["/PID", "27672", "/T", "/F"].map(str::to_string)
        );
    }
}
