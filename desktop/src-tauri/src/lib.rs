//! GKOS Engine Desktop — Tauri 2 tray shell.
//!
//! Responsibilities (Repo B of the desktop build spec):
//!   * persist settings (notes_dir, default_sensitivity, port, enabled,
//!     wizard_completed) to the app config dir as JSON;
//!   * supervise the `kosmos-agent` SEA sidecar (spawn on enable, kill on
//!     disable/quit, restart-with-backoff max 3 then surface an error state);
//!   * expose commands to the frontend (pick folder, get/set settings,
//!     start/stop sidecar, read status file + token, open windows);
//!   * a tray with: Open Settings, Copy MCP connect snippet, Quit.
//!
//! Loopback-only and unsigned are enforced upstream (the sidecar hardcodes
//! 127.0.0.1; signing is deliberately absent from tauri.conf.json).

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

const MAX_RESTARTS: u32 = 3;
const DEFAULT_PORT: u16 = 4814;
/// Loopback host the viewer's `?api=` points at (matches the sidecar's hardcoded bind).
const LOOPBACK_HOST: &str = "127.0.0.1";
const MCP_SERVER_NAME: &str = "kosmos-oden";
const SENSITIVITY_LEVELS: [&str; 7] = [
    "public",
    "internal",
    "restricted",
    "confidential",
    "regulated",
    "phi",
    "secret",
];

// ----------------------------- settings ------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub notes_dir: Option<String>,
    pub default_sensitivity: String,
    pub port: u16,
    pub enabled: bool,
    pub wizard_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notes_dir: None,
            default_sensitivity: "secret".into(),
            port: DEFAULT_PORT,
            enabled: false,
            wizard_completed: false,
        }
    }
}

impl Settings {
    /// Fail-closed normalization: invalid sensitivity → secret; invalid port →
    /// default. Mirrors the frontend `normalizeSettings`.
    fn normalized(mut self) -> Self {
        if !SENSITIVITY_LEVELS.contains(&self.default_sensitivity.as_str()) {
            self.default_sensitivity = "secret".into();
        }
        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        if let Some(dir) = &self.notes_dir {
            if dir.is_empty() {
                self.notes_dir = None;
            }
        }
        self
    }
}

/// A partial patch from the frontend `set_settings`. Absent fields are kept.
#[derive(Debug, Default, Deserialize)]
pub struct SettingsPatch {
    pub notes_dir: Option<String>,
    pub default_sensitivity: Option<String>,
    pub port: Option<u16>,
    pub enabled: Option<bool>,
    pub wizard_completed: Option<bool>,
}

// --------------------------- supervisor state -------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SupervisorStatus {
    Stopped,
    Indexing,
    Serving,
    Error,
}

struct Supervisor {
    child: Option<CommandChild>,
    status: SupervisorStatus,
    restarts: u32,
    /// Set when the shell intentionally stops the sidecar, so the terminate
    /// event is not treated as a crash worth restarting.
    manual_stop: bool,
    /// Monotonic generation; a stale supervision loop (from a killed child)
    /// checks this before acting so an old loop can't respawn a new one.
    generation: u64,
}

impl Default for Supervisor {
    fn default() -> Self {
        Supervisor {
            child: None,
            status: SupervisorStatus::Stopped,
            restarts: 0,
            manual_stop: false,
            generation: 0,
        }
    }
}

pub struct AppState {
    settings: Mutex<Settings>,
    supervisor: Mutex<Supervisor>,
}

#[derive(Serialize)]
pub struct ConnectInfo {
    port: u16,
    token: String,
}

// ------------------------------- paths --------------------------------------

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no app config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir config dir: {e}"))?;
    Ok(dir)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("settings.json"))
}

fn status_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("desktop-agent.status.json"))
}

/// The sidecar derives the token path as `dirname(status_file)/desktop-agent.token`.
fn token_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("desktop-agent.token"))
}

fn load_settings(app: &AppHandle) -> Settings {
    let path = match settings_path(app) {
        Ok(p) => p,
        Err(_) => return Settings::default(),
    };
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Settings>(&text)
            .map(Settings::normalized)
            .unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("write settings: {e}"))
}

// -------------------------- sidecar supervision -----------------------------

/// Spawn the sidecar for the current settings and attach a supervision loop.
/// Any previously-running child must already be killed by the caller.
fn spawn_sidecar(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();

    let notes_dir = match &settings.notes_dir {
        Some(dir) if !dir.is_empty() => dir.clone(),
        _ => return Err("Choose a notes folder first.".into()),
    };
    let status_file = status_path(app)?;

    let args = vec![
        "--notes".to_string(),
        notes_dir,
        "--default-sensitivity".to_string(),
        settings.default_sensitivity.clone(),
        "--port".to_string(),
        settings.port.to_string(),
        "--status-file".to_string(),
        status_file.to_string_lossy().to_string(),
    ];

    let (mut rx, child) = app
        .shell()
        .sidecar("kosmos-agent")
        .map_err(|e| format!("sidecar not found: {e}"))?
        .args(args)
        .spawn()
        .map_err(|e| format!("spawn sidecar: {e}"))?;

    let generation = {
        let mut sup = state.supervisor.lock().unwrap();
        sup.child = Some(child);
        sup.status = SupervisorStatus::Indexing;
        sup.manual_stop = false;
        sup.generation += 1;
        sup.generation
    };

    // Supervision loop: mark serving on first output, restart-with-backoff on
    // an unexpected terminate up to MAX_RESTARTS, then surface an error state.
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(_) | CommandEvent::Stderr(_) => {
                    let state = app_handle.state::<AppState>();
                    let mut sup = state.supervisor.lock().unwrap();
                    if sup.generation == generation && sup.status == SupervisorStatus::Indexing {
                        sup.status = SupervisorStatus::Serving;
                    }
                }
                CommandEvent::Terminated(_) => {
                    handle_termination(&app_handle, generation);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}

/// Decide what to do when the sidecar process ends.
fn handle_termination(app: &AppHandle, generation: u64) {
    let state = app.state::<AppState>();
    let (should_restart, backoff_ms) = {
        let mut sup = state.supervisor.lock().unwrap();
        // A stale loop (its child was already replaced) must do nothing.
        if sup.generation != generation {
            return;
        }
        sup.child = None;
        if sup.manual_stop {
            sup.status = SupervisorStatus::Stopped;
            (false, 0)
        } else if sup.restarts < MAX_RESTARTS {
            sup.restarts += 1;
            sup.status = SupervisorStatus::Indexing;
            (true, 500u64 * sup.restarts as u64)
        } else {
            sup.status = SupervisorStatus::Error;
            (false, 0)
        }
    };

    if should_restart {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio_sleep(backoff_ms).await;
            // Only restart if still enabled and nobody stopped us meanwhile.
            let enabled = {
                let state = app_handle.state::<AppState>();
                // Bind the Copy value out so the MutexGuard temporary is
                // dropped before `state` (block-local) — otherwise the guard
                // borrow of `state` outlives `state` at the block's end (E0597).
                let is_enabled = state.settings.lock().unwrap().enabled;
                is_enabled
            };
            if enabled {
                if let Err(e) = spawn_sidecar(&app_handle) {
                    let state = app_handle.state::<AppState>();
                    let mut sup = state.supervisor.lock().unwrap();
                    sup.status = SupervisorStatus::Error;
                    eprintln!("sidecar restart failed: {e}");
                }
            } else {
                let state = app_handle.state::<AppState>();
                state.supervisor.lock().unwrap().status = SupervisorStatus::Stopped;
            }
        });
    }
}

async fn tokio_sleep(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Kill the running sidecar (intentional stop) and reset the restart counter.
fn kill_sidecar(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut sup = state.supervisor.lock().unwrap();
    sup.manual_stop = true;
    sup.restarts = 0;
    sup.generation += 1; // orphan any live supervision loop
    if let Some(child) = sup.child.take() {
        let _ = child.kill();
    }
    sup.status = SupervisorStatus::Stopped;
}

// ------------------------------- commands -----------------------------------

#[tauri::command]
fn get_settings(app: AppHandle, state: State<AppState>) -> Settings {
    let s = load_settings(&app).normalized();
    *state.settings.lock().unwrap() = s.clone();
    s
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<AppState>, patch: SettingsPatch) -> Result<Settings, String> {
    let mut s = state.settings.lock().unwrap().clone();
    if let Some(v) = patch.notes_dir {
        s.notes_dir = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = patch.default_sensitivity {
        s.default_sensitivity = v;
    }
    if let Some(v) = patch.port {
        s.port = v;
    }
    if let Some(v) = patch.enabled {
        s.enabled = v;
    }
    if let Some(v) = patch.wizard_completed {
        s.wizard_completed = v;
    }
    let s = s.normalized();
    *state.settings.lock().unwrap() = s.clone();
    save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    // Run the native picker off the async runtime; blocking_pick_folder blocks
    // the calling thread until the user chooses.
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(path);
    });
    let chosen = rx.recv().map_err(|e| e.to_string())?;
    Ok(chosen.map(|p| p.to_string()))
}

#[tauri::command]
fn start_sidecar(app: AppHandle) -> Result<(), String> {
    // Always kill-then-spawn so a sensitivity/port/folder change re-projects
    // (sensitivity is ctor-fixed in the engine; restart IS the re-projection).
    kill_sidecar(&app);
    {
        let state = app.state::<AppState>();
        let mut sup = state.supervisor.lock().unwrap();
        sup.manual_stop = false;
        sup.restarts = 0;
    }
    spawn_sidecar(&app)
}

#[tauri::command]
fn stop_sidecar(app: AppHandle) {
    kill_sidecar(&app);
}

#[tauri::command]
fn supervisor_status(state: State<AppState>) -> SupervisorStatus {
    state.supervisor.lock().unwrap().status
}

#[tauri::command]
fn read_status(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let path = status_path(&app)?;
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(_) => Ok(None),
    }
}

fn read_token(app: &AppHandle) -> Option<String> {
    let path = token_path(app).ok()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
fn connect_info(app: AppHandle, state: State<AppState>) -> Option<ConnectInfo> {
    let token = read_token(&app)?;
    let port = state.settings.lock().unwrap().port;
    Some(ConnectInfo { port, token })
}

#[tauri::command]
fn complete_wizard(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut s = state.settings.lock().unwrap().clone();
    s.wizard_completed = true;
    let s = s.normalized();
    *state.settings.lock().unwrap() = s.clone();
    save_settings(&app, &s)
}

#[tauri::command]
fn open_settings_window(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

#[tauri::command]
fn open_3d_view(app: AppHandle) -> Result<(), String> {
    show_3d_view(&app)
}

#[tauri::command]
fn open_3d_view_browser(app: AppHandle) -> Result<(), String> {
    open_3d_view_in_browser(&app)
}

// ------------------------------- windows ------------------------------------

fn show_wizard(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("wizard") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "wizard", WebviewUrl::App("index.html#wizard".into()))
        .title("GKOS Engine Desktop — Setup")
        .inner_size(560.0, 640.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn show_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("GKOS Engine Desktop — Settings")
        .inner_size(720.0, 800.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ------------------------------ 3D view -------------------------------------
//
// The standalone 3D viewer (built in Odenknight/Kosmos-Oden, bundled here as
// `vault-kosmos.html`) reads `?api=<loopback base>&token=<bearer>` from
// `location.search` and auto-connects to the sidecar's read-only loopback API.
//
// Origin / CORS: the viewer must fetch the sidecar cross-origin, so it needs an
// origin the engine's CORS allowlist reflects. Serving it over the Tauri app
// protocol (WebviewUrl::App -> frontend root) gives it `tauri://localhost` /
// `https://tauri.localhost` — exactly the allowlisted origins. The browser
// fallback below loads it from a temp file (`file://`, origin `null`, also
// allowlisted). The asset protocol was deliberately NOT chosen: its origin
// (`asset://localhost`) is not in the allowlist, so its fetches would be
// CORS-blocked.

/// RFC-3986 percent-encode a query VALUE (encode everything that is not an
/// unreserved character). Keeps the sidecar URL's `:` and `/` intact through
/// the query and mirrors the frontend `viewerQuery` (snippets.ts) exactly.
fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// `api=<enc base>&token=<enc token>` — the query the viewer parses. Token may
/// be empty (API never enabled); the viewer then shows its manual connect form.
fn viewer_query(app: &AppHandle) -> String {
    let port = app.state::<AppState>().settings.lock().unwrap().port;
    let api = format!("http://{LOOPBACK_HOST}:{port}");
    let token = read_token(app).unwrap_or_default();
    format!(
        "api={}&token={}",
        encode_query_value(&api),
        encode_query_value(&token)
    )
}

/// Primary "Open 3D View": in-app window over the Tauri app protocol.
fn show_3d_view(app: &AppHandle) -> Result<(), String> {
    let url = format!("vault-kosmos.html?{}", viewer_query(app));
    if let Some(win) = app.get_webview_window("kosmos-3d") {
        // Re-navigate so a freshly-read token/port take effect, then focus.
        let _ = win.eval(&format!(
            "window.location.replace('{}')",
            url.replace('\'', "\\'")
        ));
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "kosmos-3d", WebviewUrl::App(url.into()))
        .title("GKOS Engine Desktop — 3D View")
        .inner_size(1100.0, 820.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Fallback "Open 3D View (browser)": copy the bundled viewer to a temp file
/// and open it in the system browser with the query. The page then runs as a
/// `file://` document (origin `null`, allowlisted) with no app CSP applied —
/// this path works regardless of any Tauri asset-protocol / CSP quirks.
fn open_3d_view_in_browser(app: &AppHandle) -> Result<(), String> {
    let src = app
        .path()
        .resolve("resources/vault-kosmos.html", tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("cannot locate bundled viewer: {e}"))?;
    let bytes = fs::read(&src).map_err(|e| format!("read bundled viewer: {e}"))?;

    let mut tmp = std::env::temp_dir();
    tmp.push("gkos-engine-desktop-3d-view.html");
    fs::write(&tmp, &bytes).map_err(|e| format!("write temp viewer: {e}"))?;

    // Build a file:// URL with forward slashes and the query appended.
    let path_str = tmp.to_string_lossy().replace('\\', "/");
    let file_url = if path_str.starts_with('/') {
        format!("file://{path_str}")
    } else {
        // Windows absolute path like C:/Users/... needs the extra slash.
        format!("file:///{path_str}")
    };
    let full = format!("{file_url}?{}", viewer_query(app));

    open_url_in_default_browser(&full)
}

/// Hand a URL to the OS default handler (the browser, for a file:// URL). Uses
/// the platform launcher directly rather than the shell plugin's `open`, whose
/// default URL validator rejects `file://`. The URL is fully app-constructed
/// (our temp file + our query), so there is no untrusted input here.
fn open_url_in_default_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();

    result.map(|_| ()).map_err(|e| format!("open browser: {e}"))
}

// -------------------------------- tray --------------------------------------

fn copy_snippet(app: &AppHandle) {
    let state = app.state::<AppState>();
    let port = state.settings.lock().unwrap().port;
    match read_token(app) {
        Some(token) => {
            let snippet = format!(
                "claude mcp add --transport http --header \"Authorization: Bearer {token}\" {MCP_SERVER_NAME} \"http://127.0.0.1:{port}/mcp\""
            );
            let _ = app.clipboard().write_text(snippet);
        }
        None => {
            // Not enabled yet — open settings so the user can enable + connect.
            let _ = show_settings(app);
        }
    }
}

fn build_tray(app: &AppHandle) -> Result<(), String> {
    let open_settings = MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let copy_snippet_item =
        MenuItem::with_id(app, "copy_snippet", "Copy MCP connect snippet", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let open_3d = MenuItem::with_id(app, "open_3d", "Open 3D View", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let open_3d_browser =
        MenuItem::with_id(app, "open_3d_browser", "Open 3D View (browser)", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(
        app,
        &[&open_settings, &copy_snippet_item, &open_3d, &open_3d_browser, &quit],
    )
    .map_err(|e| e.to_string())?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("GKOS Engine Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_settings" => {
                let _ = show_settings(app);
            }
            "copy_snippet" => copy_snippet(app),
            "open_3d" => {
                if let Err(e) = show_3d_view(app) {
                    eprintln!("open 3D view failed: {e}");
                }
            }
            "open_3d_browser" => {
                if let Err(e) = open_3d_view_in_browser(app) {
                    eprintln!("open 3D view (browser) failed: {e}");
                }
            }
            "quit" => {
                kill_sidecar(app);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// -------------------------------- run ---------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Second launch: surface the existing app instead of starting anew.
            let settings = load_settings(app);
            if settings.wizard_completed {
                let _ = show_settings(app);
            } else {
                let _ = show_wizard(app);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState {
            settings: Mutex::new(Settings::default()),
            supervisor: Mutex::new(Supervisor::default()),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            pick_folder,
            start_sidecar,
            stop_sidecar,
            supervisor_status,
            read_status,
            connect_info,
            complete_wizard,
            open_settings_window,
            open_3d_view,
            open_3d_view_browser,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = load_settings(&handle).normalized();
            *app.state::<AppState>().settings.lock().unwrap() = settings.clone();

            build_tray(&handle)?;

            // First run → wizard (mandatory gating). Otherwise stay in the tray
            // and, if the user left the API enabled, resume the sidecar.
            if !settings.wizard_completed {
                let _ = show_wizard(&handle);
            } else if settings.enabled && settings.notes_dir.is_some() {
                let _ = spawn_sidecar(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing a window hides it (tray app); it does not quit the app.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running GKOS Engine Desktop");
}
