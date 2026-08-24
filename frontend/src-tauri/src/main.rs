#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod clipboard_history;
mod terminal;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ag_swarmer_backend::{config::AppConfig, server, telemetry};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::PageLoadEvent;
use tauri::window::{Color, Monitor};
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;
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
// Matches `--color-background` in index.css. This covers the native/WebView2
// surface before index.html gets its first chance to paint.
const APP_WINDOW_BACKGROUND: Color = Color(250, 249, 245, 255);
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

/// Show a native OS notification.
///
/// The desktop shell hides to the tray on close, so a reply that lands while
/// the window is away has no other way to reach the user. The error is returned
/// rather than dropped: a toast that silently never appears is the hardest kind
/// of failure to diagnose, and the settings screen offers a test that shows it.
#[tauri::command]
async fn show_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    let identifier = app.config().identifier.clone();
    let display_name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "AG Swarmer".to_string());
    let result = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            let _ = app;
            show_windows_toast(&identifier, &display_name, &title, &body)
        }
        #[cfg(not(windows))]
        {
            let _ = (identifier, display_name);
            app.notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|error| error.to_string())
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err(error) = &result {
        tracing::warn!(target: "ag_swarmer::desktop", error, "failed to show a notification");
    }
    result
}

/// Raise a toast under this app's own identity, registering it first if needed.
///
/// Windows only delivers a toast for a registered AppUserModelID, which an
/// installer creates along with a Start Menu shortcut. A portable build has
/// neither, so it registers the identity under HKCU on demand and then uses the
/// app's own name and icon. PowerShell's always-registered identifier stays as a
/// fallback for machines that deny the registry write.
#[cfg(windows)]
fn show_windows_toast(
    app_id: &str,
    display_name: &str,
    title: &str,
    body: &str,
) -> Result<(), String> {
    use tauri_winrt_notification::Toast;

    let registration_error = register_windows_app_user_model_id(app_id, display_name).err();

    let raise = |id: &str| {
        Toast::new(id)
            .title(title)
            .text1(body)
            .show()
            .map_err(|error| error.to_string())
    };
    raise(app_id).or_else(|identity_error| {
        raise(Toast::POWERSHELL_APP_ID).map_err(|fallback_error| {
            let registration = registration_error
                .map(|error| format!("; AUMID registration: {error}"))
                .unwrap_or_default();
            format!(
                "{app_id}: {identity_error}{registration}; powershell fallback: {fallback_error}"
            )
        })
    })
}

/// Register an AppUserModelID so Windows delivers toasts for a portable exe.
///
/// The installer normally writes the identity via the Start Menu shortcut; a
/// portable build never runs it. Writing the equivalent HKCU registration and
/// claiming the ID on this process lets `CreateToastNotifierWithId` resolve the
/// app's own name and icon instead of showing the raw AppUserModelID.
#[cfg(windows)]
fn register_windows_app_user_model_id(app_id: &str, display_name: &str) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // The notification schema wants a URI; the executable carries the app icon.
    let icon_uri = format!("file:///{}", exe.to_string_lossy().replace('\\', "/"));

    let reg_path = format!(r"SOFTWARE\Classes\AppUserModelId\{app_id}");
    let (key, _disposition) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(&reg_path)
        .map_err(|error| error.to_string())?;
    key.set_value("DisplayName", &display_name)
        .map_err(|error| error.to_string())?;
    key.set_value("IconBackgroundColor", &"0")
        .map_err(|error| error.to_string())?;
    key.set_value("IconUri", &icon_uri)
        .map_err(|error| error.to_string())?;

    // Claim the ID on this process; otherwise Windows can still deliver the
    // toast but labels it with the raw AppUserModelID instead of DisplayName.
    use std::os::windows::ffi::OsStrExt;
    let wide_app_id: Vec<u16> = std::ffi::OsStr::new(app_id)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let hr = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide_app_id.as_ptr())
    };
    if hr != 0 {
        return Err(format!(
            "SetCurrentProcessExplicitAppUserModelID failed: 0x{hr:08X}"
        ));
    }
    Ok(())
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

fn open_logs_dir(app: &tauri::AppHandle) -> Result<(), String> {
    let log_dir = app_logs_dir(app).map_err(|error| error.to_string())?;
    append_launcher_log(
        &log_dir,
        format!("opening logs directory: {}", log_dir.display()),
    );
    #[allow(deprecated)]
    app.shell()
        .open(
            log_dir.to_string_lossy().to_string(),
            None::<tauri_plugin_shell::open::Program>,
        )
        .map_err(|error| {
            append_launcher_log(&log_dir, format!("failed to open logs directory: {error}"));
            error.to_string()
        })
}

#[tauri::command]
fn system_logs_snapshot() -> Result<telemetry::SystemLogSnapshot, String> {
    telemetry::log_snapshot(1_000).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_system_log_filter(filter: String) -> Result<(), String> {
    telemetry::set_log_filter(&filter).map_err(|error| error.to_string())
}

#[tauri::command]
fn clear_system_logs() -> Result<(), String> {
    telemetry::clear_logs().map_err(|error| error.to_string())
}

#[tauri::command]
fn open_system_logs_folder(app: tauri::AppHandle) -> Result<(), String> {
    open_logs_dir(&app)
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

/// Labels of the auxiliary windows the shell can host.
const LIBRARY_WINDOW_LABEL: &str = "library";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const ASSISTANT_WINDOW_LABEL: &str = "assistant";
const ASSISTANT_WINDOW_EDGE_MARGIN: f64 = 16.0;

fn assistant_window_monitor(app: &tauri::AppHandle) -> Option<Monitor> {
    app.get_webview_window(MAIN_WINDOW_LABEL)
        .and_then(|window| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

fn bottom_right_window_position(
    work_area_position: PhysicalPosition<i32>,
    work_area_size: PhysicalSize<u32>,
    window_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> PhysicalPosition<i32> {
    let margin = (ASSISTANT_WINDOW_EDGE_MARGIN * scale_factor).round() as i64;
    let trailing_edge = |origin: i32, available: u32, window: u32| {
        let origin = i64::from(origin);
        let desired = origin + i64::from(available) - i64::from(window) - margin;
        desired
            .max(origin)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    PhysicalPosition::new(
        trailing_edge(
            work_area_position.x,
            work_area_size.width,
            window_size.width,
        ),
        trailing_edge(
            work_area_position.y,
            work_area_size.height,
            window_size.height,
        ),
    )
}

fn move_assistant_to_bottom_right(window: &tauri::WebviewWindow, monitor: &Monitor) {
    let Ok(window_size) = window.outer_size() else {
        return;
    };
    let work_area = monitor.work_area();
    let position = bottom_right_window_position(
        work_area.position,
        work_area.size,
        window_size,
        monitor.scale_factor(),
    );
    let _ = window.set_position(position);
}

/// Shared bootstrap for auxiliary windows: reuse an existing one, otherwise
/// build it pointed at `route`.
///
/// Every window loads the same SPA; the route decides which surface renders,
/// so no page logic is duplicated. The auth token lives in `localStorage`,
/// which Tauri persists per origin, and all windows share that origin — an
/// auxiliary window opens already signed in.
fn open_aux_window(
    app: &tauri::AppHandle,
    label: &str,
    title: &str,
    route: &str,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(label) {
        existing.show().map_err(|error| error.to_string())?;
        existing.unminimize().map_err(|error| error.to_string())?;
        existing.set_focus().map_err(|error| error.to_string())?;
        let current = existing
            .url()
            .map_err(|error| error.to_string())?
            .path()
            .to_string();
        if current != route {
            existing
                .eval(route_script(route))
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let revealed = Arc::new(AtomicBool::new(false));
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(route.into()))
        .title(title)
        .inner_size(width, height)
        .min_inner_size(560.0, 420.0)
        .background_color(APP_WINDOW_BACKGROUND)
        .visible(false)
        .focused(false)
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished && !revealed.swap(true, Ordering::Relaxed)
            {
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn is_library_route(route: &str) -> bool {
    [
        "/agents",
        "/providers",
        "/mcp-servers",
        "/skills",
        "/workspaces",
        "/usage",
    ]
    .iter()
    .any(|prefix| route == *prefix || route.starts_with(&format!("{prefix}/")))
}

fn is_settings_route(route: &str) -> bool {
    route == "/settings" || route.starts_with("/settings/")
}

/// Open the resource library as its own top-level window, independent of the
/// main conversation window so both can be used side by side.
#[tauri::command]
async fn open_library_window(app: tauri::AppHandle, route: String) -> Result<(), String> {
    if !is_library_route(&route) {
        return Err("invalid library route".to_string());
    }
    open_aux_window(
        &app,
        LIBRARY_WINDOW_LABEL,
        "AG Swarmer — Library",
        &route,
        1180.0,
        760.0,
    )
}

/// Open settings as its own top-level window, same reasoning as the library's.
#[tauri::command]
async fn open_settings_window(app: tauri::AppHandle, route: String) -> Result<(), String> {
    if !is_settings_route(&route) {
        return Err("invalid settings route".to_string());
    }
    open_aux_window(
        &app,
        SETTINGS_WINDOW_LABEL,
        "AG Swarmer — Settings",
        &route,
        900.0,
        680.0,
    )
}

/// Open (or focus) the Assistant dock as an always-on-top utility window.
///
/// A DOM overlay cannot leave the webview that hosts it; promoting the dock to
/// a native window is what lets it stay visible above every other application
/// window while the user works elsewhere. Browser builds keep the in-page
/// overlay instead.
#[tauri::command]
async fn toggle_assistant_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(ASSISTANT_WINDOW_LABEL) {
        let showing = existing.is_visible().map_err(|error| error.to_string())?;
        if showing {
            existing.hide().map_err(|error| error.to_string())?;
        } else {
            existing.show().map_err(|error| error.to_string())?;
            existing.unminimize().map_err(|error| error.to_string())?;
            existing.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let revealed = Arc::new(AtomicBool::new(false));
    let initial_monitor = assistant_window_monitor(&app);
    WebviewWindowBuilder::new(
        &app,
        ASSISTANT_WINDOW_LABEL,
        WebviewUrl::App("/assistant-dock".into()),
    )
    .title("AG Swarmer — Assistant")
    .inner_size(380.0, 520.0)
    .min_inner_size(300.0, 360.0)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(true)
    .background_color(APP_WINDOW_BACKGROUND)
    .visible(false)
    .focused(false)
    .on_page_load(move |window, payload| {
        if payload.event() == PageLoadEvent::Finished && !revealed.swap(true, Ordering::Relaxed) {
            if let Some(monitor) = initial_monitor.as_ref() {
                move_assistant_to_bottom_right(&window, monitor);
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
    })
    .build()
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    if app.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("/".into()))
        .disable_drag_drop_handler()
        .title("AG Swarmer")
        .inner_size(1280.0, 800.0)
        .min_inner_size(1024.0, 680.0)
        .background_color(APP_WINDOW_BACKGROUND)
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
            TRAY_OPEN_SETTINGS_ID => {
                // WebView2 can deadlock when a webview window is created from
                // this synchronous event handler. Run the same async path as
                // the frontend command on Tauri's runtime instead.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) =
                        open_settings_window(app, "/settings/system".to_string()).await
                    {
                        tracing::warn!(
                            target: "ag_swarmer::desktop",
                            %error,
                            "failed to open settings window from tray"
                        );
                    }
                });
            }
            TRAY_OPEN_LOGS_ID => {
                let _ = open_logs_dir(app);
            }
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(BackendShutdown(std::sync::Mutex::new(None)))
        .manage(TerminalManager::new(Arc::new(NativePtySpawner)))
        .invoke_handler(tauri::generate_handler![
            backend_base_url,
            pick_workspace_folder,
            reveal_in_file_manager,
            save_file,
            show_notification,
            open_library_window,
            open_settings_window,
            toggle_assistant_window,
            system_logs_snapshot,
            set_system_log_filter,
            clear_system_logs,
            open_system_logs_folder,
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
            // Copies made inside WebView2 never reach Windows clipboard
            // history. The Windows-only bridge mirrors copies made while one
            // of this process's windows has focus; other applications are
            // deliberately left untouched.
            #[cfg(target_os = "windows")]
            clipboard_history::start();
            clear_backend_port(&log_dir, BACKEND_PORT);
            let shutdown = start_in_process_backend(app_data_dir, log_dir.clone())?;
            let state = app.state::<BackendShutdown>();
            *state.0.lock().expect("backend shutdown mutex poisoned") = Some(shutdown);
            create_main_window(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    if window.label() == MAIN_WINDOW_LABEL {
                        // The conversation window hides to the tray so a later
                        // click does not lose the open chat. Auxiliary windows
                        // are disposable and must actually close — hiding them
                        // would leave a leftover SPA that still paints the
                        // previous surface the next time they reopen.
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                tauri::WindowEvent::Focused(false) => {
                    // Keep the Assistant above everything even when another
                    // window takes input; Tauri drops always-on-top on some
                    // platforms when a fullscreen window overlaps.
                    if window.label() == ASSISTANT_WINDOW_LABEL {
                        let _ = window.set_always_on_top(true);
                    }
                }
                _ => {}
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
    use std::future::Future;

    use super::{
        bottom_right_window_position, is_library_route, is_settings_route, open_library_window,
        open_settings_window, pids_listening_on_port, taskkill_args, toggle_assistant_window,
    };
    use tauri::{PhysicalPosition, PhysicalSize};

    fn assert_route_window_command_is_async<F, Fut>(_command: F)
    where
        F: Fn(tauri::AppHandle, String) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
    }

    fn assert_window_command_is_async<F, Fut>(_command: F)
    where
        F: Fn(tauri::AppHandle) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
    }

    #[test]
    fn webview_window_commands_stay_async_on_windows() {
        // Tauri documents a WebView2 deadlock when builders run from a
        // synchronous command. These type assertions make a future accidental
        // `async` removal fail to compile on every platform.
        assert_route_window_command_is_async(open_library_window);
        assert_route_window_command_is_async(open_settings_window);
        assert_window_command_is_async(toggle_assistant_window);
    }

    #[test]
    fn auxiliary_window_routes_stay_inside_their_surface() {
        assert!(is_library_route("/agents"));
        assert!(is_library_route("/providers/new"));
        assert!(is_library_route("/usage"));
        assert!(!is_library_route("/settings/system"));
        assert!(!is_library_route("/groups/one/manage"));

        assert!(is_settings_route("/settings/system"));
        assert!(is_settings_route("/settings/logs"));
        assert!(!is_settings_route("/agents"));
    }

    #[test]
    fn assistant_starts_at_the_work_area_bottom_right() {
        let position = bottom_right_window_position(
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            PhysicalSize::new(493, 660),
            1.25,
        );

        assert_eq!(position, PhysicalPosition::new(1407, 360));
    }

    #[test]
    fn assistant_position_supports_monitors_with_negative_origins() {
        let position = bottom_right_window_position(
            PhysicalPosition::new(-1920, -200),
            PhysicalSize::new(1920, 1080),
            PhysicalSize::new(400, 500),
            1.5,
        );

        assert_eq!(position, PhysicalPosition::new(-424, 356));
    }

    /// Proves a portable exe can register its own identity and raise a toast.
    ///
    /// Two things could stop it and neither shows up at compile time: an
    /// unregistered AppUserModelID, and a worker thread with no COM apartment.
    /// Registering first (rather than borrowing PowerShell's identity) is what
    /// lets the toast carry the app's own name and icon, so this raises directly
    /// under the freshly-registered ID to prove the registration is sufficient.
    #[cfg(windows)]
    #[test]
    fn registers_an_identity_and_raises_a_toast() {
        const TEST_APP_ID: &str = "ag-swarmer.test.notification";

        super::register_windows_app_user_model_id(TEST_APP_ID, "AG Swarmer")
            .expect("register test AUMID");

        let raised = std::thread::spawn(|| {
            tauri_winrt_notification::Toast::new(TEST_APP_ID)
                .title("AG Swarmer")
                .text1("Toast probe")
                .show()
                .map_err(|error| error.to_string())
        })
        .join()
        .expect("toast thread panicked");

        // Give the toast time to post before removing the identity it carries.
        std::thread::sleep(std::time::Duration::from_millis(500));

        let registered = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(format!(r"SOFTWARE\Classes\AppUserModelId\{TEST_APP_ID}"))
            .is_ok();
        let _ = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .delete_subkey_all(format!(r"SOFTWARE\Classes\AppUserModelId\{TEST_APP_ID}"));

        assert!(registered, "AUMID registry key was not created");
        assert!(raised.is_ok(), "own identity was not accepted: {raised:?}");
    }

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
