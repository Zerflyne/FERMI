//! The FERMI engine, inside the app: models on llama.cpp (any GPU through Vulkan or Metal, or the CPU), folder
//! analyses in the background, the app's HTTP API and the public API server. It replaces the old Python engine with
//! the same routes and the same files, and needs nothing installed on the computer.

pub mod fermi;
pub mod http;
pub mod jobs;
pub mod models;
pub mod runtime;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use self::fermi::{check_questions, expand, merge};
use self::http::{HttpError, Role};
use self::jobs::Jobs;
use self::models::Models;
use self::runtime::{Device, Runtime};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_LINES: usize = 2000;

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

static LOG: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

/// The engine's log: kept in memory for the Settings page, and on stderr.
pub fn log(msg: &str) {
    let line = format!("{} {msg}", chrono::Local::now().format("%H:%M:%S"));
    eprintln!("{line}");
    let mut l = LOG.get_or_init(|| Mutex::new(VecDeque::new())).lock().unwrap();
    l.push_back(line);
    while l.len() > LOG_LINES {
        l.pop_front();
    }
}

pub fn log_lines() -> Vec<String> {
    LOG.get_or_init(|| Mutex::new(VecDeque::new())).lock().unwrap().iter().cloned().collect()
}

/// CPU threads for llama.cpp: one per performance core (llama.cpp's own default is 4). On Intel hybrid processors
/// the efficiency cores slow everything down (measured on an i7-14700F: 8 threads 400 tokens/s, 20 threads 300).
pub fn auto_threads() -> i32 {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/sys/devices/cpu_core/cpus") {
        // "0-15": the logical CPUs of the performance cores, two per core with hyper-threading
        let n: usize = s.trim().split(',').map(|r| match r.split_once('-') {
            Some((a, b)) => b.parse::<usize>().unwrap_or(0) + 1 - a.parse::<usize>().unwrap_or(0),
            None => 1,
        }).sum();
        if n >= 2 {
            return (n / 2).max(1) as i32;
        }
    }
    let sys = sysinfo::System::new();
    sysinfo::System::physical_core_count(&sys).unwrap_or(4).clamp(1, 16) as i32
}

/// The user's data folder: settings, models, analyses.
pub fn default_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("FERMI")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/FERMI")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("fermi-app")
    }
}

// --------------------------------------------------------------------------------------------------------- settings
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub api_key: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self { enabled: false, host: "127.0.0.1".into(), port: 8765, api_key: String::new() }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "auto" (the first GPU, otherwise the CPU), "cpu", or a device name ("Vulkan0", "Metal"...)
    pub device: String,
    /// CPU threads, 0 = automatic
    pub threads: i32,
    pub default_model: Option<String>,
    pub autoload: Vec<String>,
    /// model files added by hand, anywhere on the computer
    pub extra_models: Vec<String>,
    pub server: ServerSettings,
    pub question_sets: Vec<Value>,
    /// whatever else is in the file (kept as it is)
    #[serde(flatten)]
    pub other: Map<String, Value>,
    #[serde(skip)]
    path: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        Self { device: "auto".into(), threads: 0, default_model: None, autoload: Vec::new(), extra_models: Vec::new(),
               server: ServerSettings::default(), question_sets: Vec::new(), other: Map::new(), path: PathBuf::new() }
    }
}

impl Settings {
    fn load(path: &Path) -> Settings {
        let mut s: Settings = fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        s.other.remove("models"); // the Python engine's list of checkpoints: not used any more
        s.path = path.to_path_buf();
        s
    }

    pub fn save(&self) {
        let tmp = self.path.with_extension("json.tmp");
        if fs::write(&tmp, serde_json::to_string_pretty(self).unwrap()).is_ok() {
            let _ = fs::rename(&tmp, &self.path);
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }

    /// The fields the app may change through POST /api/settings.
    pub fn update(&mut self, b: &Value) {
        if let Some(d) = b["device"].as_str() {
            self.device = d.to_string();
        }
        if let Some(t) = b["threads"].as_i64() {
            self.threads = t.clamp(0, 256) as i32;
        }
        if let Some(a) = b["autoload"].as_array() {
            self.autoload = a.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
        if let Some(q) = b["question_sets"].as_array() {
            self.question_sets = q.clone();
        }
    }
}

// ----------------------------------------------------------------------------------------------------------- engine
pub struct Engine {
    pub data_dir: PathBuf,
    pub runtime: Option<Arc<Runtime>>,
    pub runtime_error: Option<String>,
    pub settings: Arc<Mutex<Settings>>,
    pub models: Arc<Models>,
    pub jobs: Arc<Jobs>,
    pub requests: Mutex<VecDeque<Value>>,
    pub counter: Mutex<Value>,
    api: Mutex<Option<Arc<tiny_http::Server>>>,
    api_error: Mutex<Option<String>>,
    pub started: f64,
    /// set by the desktop app: brings its window to the front (a second launch of the app calls it)
    pub on_show: OnceLock<Box<dyn Fn() + Send + Sync>>,
    me: OnceLock<std::sync::Weak<Engine>>,
}

impl Engine {
    /// runtime_dir: the folder with fermi_llama and the llama.cpp libraries.
    pub fn new(data_dir: PathBuf, runtime_dir: &Path) -> Arc<Engine> {
        let _ = fs::create_dir_all(&data_dir);
        let settings = Arc::new(Mutex::new(Settings::load(&data_dir.join("settings.json"))));
        let (runtime, runtime_error) = match Runtime::load(runtime_dir) {
            Ok(r) => {
                let devs = r.devices();
                log(&format!("runtime {}: {}", runtime_dir.display(),
                             devs.iter().map(|d| format!("{} ({})", d.name, d.description)).collect::<Vec<_>>().join(", ")));
                (Some(Arc::new(r)), None)
            }
            Err(e) => {
                log(&format!("runtime not available: {e}"));
                (None, Some(e))
            }
        };
        let models = Models::new(runtime.clone(), &data_dir, settings.clone());
        let jobs = Jobs::new(&data_dir, models.clone());
        let e = Arc::new(Engine {
            data_dir, runtime, runtime_error, settings, models, jobs,
            requests: Mutex::new(VecDeque::new()),
            counter: Mutex::new(json!({"requests": 0, "questions": 0})),
            api: Mutex::new(None), api_error: Mutex::new(None), started: now(), on_show: OnceLock::new(), me: OnceLock::new(),
        });
        let _ = e.me.set(Arc::downgrade(&e));
        e
    }

    /// Starts the app server on 127.0.0.1:port, the API server if it was on, and the models to load at start.
    pub fn start(self: &Arc<Self>, port: u16) -> Result<(), String> {
        let srv = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
        http::serve(self.clone(), Arc::new(srv), Role::App, 8);
        if self.settings.lock().unwrap().server.enabled {
            self.api_start();
        }
        let auto = self.settings.lock().unwrap().autoload.clone();
        for m in auto {
            if let Err(e) = self.models.load(Some(&m), false) {
                log(&format!("autoload {m}: {e}"));
            }
        }
        log(&format!("FERMI engine {VERSION} ready: http://127.0.0.1:{port} (data: {})", self.data_dir.display()));
        Ok(())
    }

    pub fn devices(&self) -> Vec<Device> {
        models::devices_json(self.runtime.as_ref())
    }

    pub fn runtime_status(&self) -> Value {
        json!({"ok": self.runtime.is_some(), "error": self.runtime_error,
               "dir": self.runtime.as_ref().map(|r| r.dir.to_string_lossy().into_owned()), "devices": self.devices()})
    }

    pub fn log_request(&self, r: Value) {
        let mut q = self.requests.lock().unwrap();
        q.push_back(r);
        while q.len() > 300 {
            q.pop_front();
        }
    }

    /// POST /api/classify: {model?, state | states, questions, orders?}
    pub fn classify(self: &Arc<Self>, req: &Value) -> Result<(Value, usize), HttpError> {
        let questions = req["questions"].as_object().cloned().ok_or("questions missing: {id: {type, instructions, criteria?}}")?;
        check_questions(&questions)?;
        let (id, model) = self.models.get(req["model"].as_str())?;
        let name = self.models.public(&id)["name"].clone();
        let orders = req.get("orders").cloned().unwrap_or(Value::Null);
        let t = std::time::Instant::now();
        let (mut out, nq) = if let Some(states) = req["states"].as_array() {
            // several states, the same questions
            let (exp, plan) = expand(&questions, &orders);
            let pairs: Vec<(&Value, &Map<String, Value>)> = states.iter().map(|s| (s, &exp)).collect();
            let res = model.classify_many(&pairs)?;
            let results: Vec<Value> = res.iter().map(|a| Value::Object(merge(&questions, &plan, a))).collect();
            (json!({"results": results}), states.len() * questions.len())
        } else {
            let state = req.get("state").ok_or("state missing (a text or a JSON object)")?;
            (json!({"answers": model.answer(state, &questions, &orders)?}), questions.len())
        };
        out["ms"] = json!(t.elapsed().as_millis() as u64);
        out["model"] = name;
        let mut c = self.counter.lock().unwrap();
        c["requests"] = json!(c["requests"].as_u64().unwrap_or(0) + 1);
        c["questions"] = json!(c["questions"].as_u64().unwrap_or(0) + nq as u64);
        Ok((out, nq))
    }

    pub fn info(&self) -> Value {
        let def = self.models.default_id();
        let e = def.as_deref().map(|d| self.models.public(d)).unwrap_or(Value::Null);
        json!({"engine": "fermi-engine", "version": VERSION, "model": e["name"],
               "device": e["device"].as_str().unwrap_or("not loaded"), "checkpoint": e["quant"],
               "models": self.models.entries(), "evaluation": e["evaluation"]})
    }

    // the public API server -------------------------------------------------------------------------------------
    pub fn api_running(&self) -> bool {
        self.api.lock().unwrap().is_some()
    }

    pub fn api_status(&self) -> Value {
        let s = self.settings.lock().unwrap().server.clone();
        let host = if s.host == "0.0.0.0" { "127.0.0.1" } else { &s.host };
        json!({"enabled": self.api_running(), "host": s.host, "port": s.port, "key": !s.api_key.is_empty(),
               "error": *self.api_error.lock().unwrap(), "lan": if s.host == "0.0.0.0" { lan_addresses() } else { vec![] },
               "url": format!("http://{host}:{}", s.port)})
    }

    pub fn api_start(&self) -> bool {
        self.api_stop();
        let s = self.settings.lock().unwrap().server.clone();
        match tiny_http::Server::http((s.host.as_str(), s.port)) {
            Ok(srv) => {
                let srv = Arc::new(srv);
                let me = self.me.get().and_then(|w| w.upgrade()).expect("engine");
                http::serve(me, srv.clone(), Role::Api, 8);
                *self.api.lock().unwrap() = Some(srv);
                *self.api_error.lock().unwrap() = None;
                log(&format!("API server on http://{}:{}", s.host, s.port));
                true
            }
            Err(e) => {
                let msg = format!("cannot listen on {}:{}: {e}", s.host, s.port);
                log(&msg);
                *self.api_error.lock().unwrap() = Some(msg);
                false
            }
        }
    }

    pub fn api_stop(&self) {
        if let Some(srv) = self.api.lock().unwrap().take() {
            srv.unblock();
            for _ in 0..8 {
                srv.unblock(); // one per serving thread
            }
            log("API server stopped");
        }
    }

    pub fn system_info(&self) -> Value {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        let devs = self.devices();
        let gpu = devs.iter().find(|d| d.kind == "gpu").or_else(|| devs.iter().find(|d| d.kind == "igpu"));
        json!({
            "engine": VERSION, "platform": format!("{} {}", System::name().unwrap_or_default(), System::os_version().unwrap_or_default()),
            "arch": std::env::consts::ARCH, "cpu": sys.cpus().first().map(|c| c.brand().trim().to_string()), "threads": sys.cpus().len(),
            "ram": {"total": sys.total_memory(), "available": sys.available_memory()},
            "gpu": gpu.map(|g| json!({"name": g.description, "vram_total": g.total, "vram_free": g.free, "vram_used": g.total - g.free})),
            "devices": devs, "runtime": self.runtime_status(),
        })
    }
}

fn lan_addresses() -> Vec<String> {
    // no packet is sent: connecting a UDP socket only picks the outgoing interface
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("10.255.255.255:1").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| vec![a.ip().to_string()])
        .unwrap_or_default()
}
