//! FERMI desktop app: the window (React UI), the tray icon, and the FERMI engine running inside the app itself
//! (src/engine: llama.cpp through the bundled runtime, the HTTP API, the folder analyses). Nothing else is needed on
//! the computer: no Python, no drivers beyond the usual graphics driver.
//!
//! Closing the window keeps the app in the tray with the models and the API server on, like LM Studio. A second
//! launch of the app brings the first one's window to the front instead of starting another engine.

pub mod engine;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};

use engine::{log, Engine};

pub const APP_PORT: u16 = 8767;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    close_to_tray: bool,
    start_hidden: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { close_to_tray: true, start_hidden: false }
    }
}

#[derive(Clone, Serialize)]
struct EngineInfo {
    port: u16,
    running: bool,
    error: Option<String>,
    data_dir: String,
    runtime_dir: String,
    version: &'static str,
}

struct AppState {
    engine: Option<Arc<Engine>>,
    error: Option<String>,
    port: u16,
    runtime_dir: PathBuf,
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
    quitting: Mutex<bool>,
}

fn load_settings(path: &Path) -> AppSettings {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_settings(path: &Path, s: &AppSettings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// The folder with the llama.cpp runtime: $FERMI_RUNTIME, the app's resources once installed, app/runtime/dist while
/// developing.
pub fn runtime_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    if let Some(p) = std::env::var_os("FERMI_RUNTIME") {
        return PathBuf::from(p);
    }
    let lib = engine::runtime::lib_name();
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(r) = resource_dir {
        cands.push(r.join("runtime"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            cands.push(d.join("runtime"));
            cands.push(d.to_path_buf());
        }
    }
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("runtime").join("dist"));
    cands.iter().find(|d| d.join(lib).is_file()).cloned().unwrap_or_else(|| cands[0].clone())
}

/// A small HTTP request to a local engine (another copy of the app). Returns the body.
fn local_http(port: u16, method: &str, path: &str) -> Option<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    s.write_all(format!("{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n").as_bytes()).ok()?;
    let mut buf = String::new();
    let _ = s.take(256 * 1024).read_to_string(&mut buf);
    Some(buf)
}

// --------------------------------------------------------------------------------------------------- commands
fn info(state: &AppState) -> EngineInfo {
    EngineInfo {
        port: state.port,
        running: state.engine.is_some(),
        error: state.error.clone(),
        data_dir: state.engine.as_ref().map(|e| e.data_dir.to_string_lossy().into_owned()).unwrap_or_default(),
        runtime_dir: state.runtime_dir.to_string_lossy().into_owned(),
        version: engine::VERSION,
    }
}

#[tauri::command]
fn engine_info(state: State<AppState>) -> EngineInfo {
    info(&state)
}

#[tauri::command]
fn engine_log() -> Vec<String> {
    engine::log_lines()
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(state: State<AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    save_settings(&state.settings_path, &settings)?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        tauri_plugin_opener::open_url(&path, None::<&str>).map_err(|e| e.to_string())
    } else {
        tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
    }
}

// -------------------------------------------------------------------------------------------------------- app
fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn quit(app: &AppHandle) {
    *app.state::<AppState>().quitting.lock().unwrap() = true;
    app.exit(0);
}

/// Without a window: the engine and its API only (for servers). `fermi-app --headless [--port N] [--data DIR]`
pub fn run_headless(args: &[String]) {
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let port = arg("--port").and_then(|p| p.parse().ok()).unwrap_or(APP_PORT);
    let data = arg("--data").map(PathBuf::from).unwrap_or_else(engine::default_data_dir);
    let rt = arg("--runtime").map(PathBuf::from).unwrap_or_else(|| runtime_dir(None));
    let eng = Engine::new(data, &rt);
    if let Err(e) = eng.start(port) {
        eprintln!("{e}");
        std::process::exit(3);
    }
    for m in args.iter().enumerate().filter(|(_, a)| *a == "--load").filter_map(|(i, _)| args.get(i + 1)) {
        let _ = eng.models.load(Some(m), false);
    }
    loop {
        std::thread::park();
    }
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--headless") {
        return run_headless(&args);
    }
    // already running? bring that window to the front and stop here
    if local_http(APP_PORT, "GET", "/api/info").map(|b| b.contains("fermi-engine")).unwrap_or(false) {
        let _ = local_http(APP_PORT, "POST", "/api/window/show");
        return;
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let settings_path = app.path().app_config_dir().map(|d| d.join("app.json")).unwrap_or_else(|_| PathBuf::from("fermi-app.json"));
            let settings = load_settings(&settings_path);
            let start_hidden = settings.start_hidden;
            let rt = runtime_dir(app.path().resource_dir().ok());
            let eng = Engine::new(engine::default_data_dir(), &rt);
            let handle = app.handle().clone();
            let _ = eng.on_show.set(Box::new(move || {
                let h = handle.clone();
                let _ = handle.run_on_main_thread(move || show_window(&h));
            }));
            let (engine, error) = match eng.start(APP_PORT) {
                Ok(()) => (Some(eng), None),
                Err(e) => {
                    log(&e);
                    (None, Some(format!("Il motore di FERMI non è partito: {e}")))
                }
            };
            app.manage(AppState { engine, error, port: APP_PORT, runtime_dir: rt, settings: Mutex::new(settings), settings_path,
                                  quitting: Mutex::new(false) });

            let show = MenuItem::with_id(app, "show", "Apri FERMI", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Esci da FERMI", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit_item])?;
            let mut tray = TrayIconBuilder::with_id("fermi")
                .tooltip("FERMI")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show_window(app),
                    "quit" => quit(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let to_tray = state.settings.lock().unwrap().close_to_tray;
                if to_tray && !*state.quitting.lock().unwrap() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![engine_info, engine_log, get_settings, set_settings, open_path])
        .build(tauri::generate_context!())
        .expect("error while building the FERMI app");
    app.run(|_, _| {});
}
