#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

struct BackendChild(Mutex<Option<CommandChild>>);

const BACKEND_PORT: u16 = 8765;
const BACKEND_BASE_URL: &str = "http://127.0.0.1:8765";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_MAIN_ID: &str = "open-main";
const TRAY_OPEN_SETTINGS_ID: &str = "open-settings";
const TRAY_OPEN_LOGS_ID: &str = "open-logs";
const TRAY_EXIT_ID: &str = "exit";

impl Drop for BackendChild {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(child) = guard.take() {
                let _ = child.kill();
            }
        }
    }
}

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

    let state = app.state::<BackendChild>();
    let child = state.0.lock().expect("backend child mutex poisoned").take();
    if let Some(child) = child {
        terminate_backend_child(log_dir.as_deref(), child);
    } else {
        append_optional_launcher_log(
            log_dir.as_deref(),
            "backend shutdown requested but no sidecar child was recorded",
        );
    }
}

fn terminate_backend_child(log_dir: Option<&Path>, child: CommandChild) {
    let pid = child.pid();
    append_optional_launcher_log(log_dir, format!("shutting down backend sidecar PID {pid}"));

    #[cfg(target_os = "windows")]
    {
        if terminate_process_tree_with_taskkill(log_dir, pid) {
            return;
        }
        append_optional_launcher_log(
            log_dir,
            format!("falling back to direct kill for backend sidecar PID {pid}"),
        );
    }

    match child.kill() {
        Ok(()) => append_optional_launcher_log(
            log_dir,
            format!("direct kill sent to backend sidecar PID {pid}"),
        ),
        Err(err) => append_optional_launcher_log(
            log_dir,
            format!("failed to directly kill backend sidecar PID {pid}: {err}"),
        ),
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

#[cfg(any(target_os = "windows", test))]
fn taskkill_process_tree_args(pid: u32) -> Vec<String> {
    vec![
        "/PID".to_string(),
        pid.to_string(),
        "/F".to_string(),
        "/T".to_string(),
    ]
}

#[cfg(target_os = "windows")]
fn terminate_process_tree_with_taskkill(log_dir: Option<&Path>, pid: u32) -> bool {
    append_optional_launcher_log(
        log_dir,
        format!("terminating process tree for backend sidecar PID {pid}"),
    );
    let args = taskkill_process_tree_args(pid);
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
            format!("killing PID {pid} listening on TCP port {port}"),
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

fn wait_for_backend(timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", BACKEND_PORT)) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let request = format!(
                "GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                BACKEND_PORT
            );
            if stream.write_all(request.as_bytes()).is_ok() {
                let mut response = String::new();
                if stream.read_to_string(&mut response).is_ok()
                    && response.starts_with("HTTP/1.1 200")
                    && response.contains("\"status\":\"ok\"")
                {
                    return Ok(());
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "backend did not become healthy within {} seconds",
        timeout.as_secs()
    ))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(BackendChild(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            backend_base_url,
            pick_workspace_folder
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
            let app_data_arg = app_data_dir.to_string_lossy().to_string();
            let port_arg = BACKEND_PORT.to_string();
            let command = app.shell().sidecar("ag-swarmer-backend")?.args([
                "--port",
                &port_arg,
                "--app-data-dir",
                &app_data_arg,
            ]);
            append_launcher_log(&log_dir, "spawning backend sidecar");
            let (mut rx, child) = command.spawn()?;

            let sidecar_log_dir = log_dir.clone();
            thread::spawn(move || {
                while let Some(event) = rx.blocking_recv() {
                    match event {
                        CommandEvent::Stdout(line) => {
                            append_launcher_log(
                                &sidecar_log_dir,
                                format!(
                                    "backend stdout: {}",
                                    String::from_utf8_lossy(&line).trim_end()
                                ),
                            );
                        }
                        CommandEvent::Stderr(line) => {
                            append_launcher_log(
                                &sidecar_log_dir,
                                format!(
                                    "backend stderr: {}",
                                    String::from_utf8_lossy(&line).trim_end()
                                ),
                            );
                        }
                        CommandEvent::Terminated(payload) => {
                            append_launcher_log(
                                &sidecar_log_dir,
                                format!("backend terminated: {:?}", payload),
                            );
                            break;
                        }
                        _ => {}
                    }
                }
            });

            if let Err(message) = wait_for_backend(Duration::from_secs(60)) {
                append_launcher_log(&log_dir, format!("backend startup failed: {message}"));
                let _ = child.kill();
                return Err(message.into());
            }
            append_launcher_log(&log_dir, "backend healthy");

            let state = app.state::<BackendChild>();
            *state.0.lock().expect("backend child mutex poisoned") = Some(child);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{pids_listening_on_port, taskkill_process_tree_args};

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
            taskkill_process_tree_args(27672),
            vec!["/PID", "27672", "/F", "/T"]
        );
    }
}
