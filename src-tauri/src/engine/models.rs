//! FERMI models: the catalog of models the app can download (Hugging Face), the models installed on this computer
//! (one .gguf file each, in the data folder or added by hand), and the slots that hold the loaded ones.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::fermi::Fermi;
use super::runtime::{Device, Runtime};
use super::{log, now, Settings};

/// What the app can download. Files on huggingface.co/<repo>/resolve/<revision>/<file>.
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub size: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub description: &'static str,
    /// memory the model needs to run, weights + working buffers (bytes, measured)
    pub memory: u64,
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "fermi-2b",
        name: "FERMI-2B",
        size: "2B",
        repo: "Zerflyne/FERMI-2B",
        revision: "main",
        file: "gguf/FERMI-2B-Q8_0.gguf",
        bytes: 2_688_107_136,
        sha256: "9baa73ff05dc4c5ab6e0ec2a6d77801773e949b966a3c9b981aee4ba7c1597d2",
        description: "Il modello più preciso: 85% di accordo col maestro, 81% sulle domande di riferimento.",
        memory: 3_000_000_000,
    },
    CatalogEntry {
        id: "fermi-0.8b",
        name: "FERMI-0.8B",
        size: "0.8B",
        repo: "Zerflyne/FERMI-0.8B",
        revision: "main",
        file: "gguf/FERMI-0.8B-Q8_0.gguf",
        bytes: 816_048_896,
        sha256: "1e6eab06bbed9575043da2cca5a836006765b77a0254939be9e488d7e11bb04e",
        description: "Il più leggero e veloce: va bene anche su computer senza scheda video.",
        memory: 1_600_000_000,
    },
];

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn quant_name(ft: Option<i64>) -> &'static str {
    match ft {
        Some(0) => "F32",
        Some(1) => "F16",
        Some(7) => "Q8_0",
        Some(32) => "BF16",
        Some(15) => "Q4_K_M",
        Some(17) => "Q5_K_M",
        Some(18) => "Q6_K",
        _ => "",
    }
}

// ---------------------------------------------------------------------------------------------------------- slots
pub struct Slot {
    pub entry: Value,
    pub path: PathBuf,
    pub status: &'static str, // unloaded | loading | ready | error
    pub error: Option<String>,
    pub note: Option<String>,
    pub model: Option<Arc<Fermi>>,
    pub vram: Option<u64>,
    pub load_s: Option<f64>,
    pub device: Option<String>,
}

impl Slot {
    fn public(&self) -> Value {
        let mut e = self.entry.clone();
        e["status"] = json!(self.status);
        e["error"] = json!(self.error);
        e["note"] = json!(self.note);
        e["vram_bytes"] = json!(self.vram);
        e["load_seconds"] = json!(self.load_s);
        e["device"] = json!(self.device);
        e
    }
}

#[derive(Clone, Default)]
pub struct Download {
    pub state: &'static str, // running | done | error | cancelled
    pub done: u64,
    pub total: u64,
    pub speed: f64,
    pub error: Option<String>,
    pub cancel: Arc<AtomicBool>,
}

pub struct Models {
    pub rt: Option<Arc<Runtime>>,
    pub dir: PathBuf,
    pub settings: Arc<Mutex<Settings>>,
    pub slots: Mutex<Vec<Slot>>,
    pub downloads: Mutex<HashMap<String, Download>>,
    load_lock: Mutex<()>,
}

impl Models {
    pub fn new(rt: Option<Arc<Runtime>>, data_dir: &Path, settings: Arc<Mutex<Settings>>) -> Arc<Models> {
        let dir = data_dir.join("models");
        let _ = fs::create_dir_all(&dir);
        let m = Arc::new(Models { rt, dir, settings, slots: Mutex::new(Vec::new()), downloads: Mutex::new(HashMap::new()), load_lock: Mutex::new(()) });
        m.rescan();
        m
    }

    /// The model files: the data folder's models/*.gguf and the files added by hand.
    fn files(&self) -> Vec<(PathBuf, &'static str)> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.dir) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "gguf").unwrap_or(false)).collect();
            v.sort();
            out.extend(v.into_iter().map(|p| (p, "downloaded")));
        }
        for p in self.settings.lock().unwrap().extra_models.clone() {
            let p = PathBuf::from(p);
            if !out.iter().any(|(q, _)| q == &p) {
                out.push((p, "added"));
            }
        }
        out
    }

    fn entry_for(&self, path: &Path, source: &str) -> Option<Value> {
        let rt = self.rt.as_ref()?;
        let cfg = Fermi::read_config(rt, path)?;
        let name = cfg["name"].as_str().map(str::to_string).unwrap_or_else(|| {
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        });
        let base = cfg["base_model"].as_str().unwrap_or("").to_string();
        let size = base.rsplit('/').next().unwrap_or("").rsplit('-').next().unwrap_or("").to_string();
        let ev = &cfg["evaluation"];
        let quant = quant_name(cfg["file_type"].as_i64());
        Some(json!({
            "id": slug(&name), "name": name, "size": size, "path": path.to_string_lossy(), "source": source,
            "file_bytes": fs::metadata(path).map(|m| m.len()).unwrap_or(0), "quant": quant,
            "base_model": base, "training_step": cfg["training_step"],
            "evaluation": {"agreement": ev["tutte"]["accordo_qwen"], "cross_entropy": ev["tutte"]["ce"],
                           "reference": ev["jev"]["accordo"], "ece": ev["ece"],
                           "by_type": {"noul": ev["noul"]["accordo_qwen"], "choice": ev["choice"]["accordo_qwen"],
                                       "score": ev["score"]["accordo_qwen"]}},
        }))
    }

    /// Reads the model files again: new files become slots, missing ones go (unless loaded).
    pub fn rescan(&self) {
        let found: Vec<(PathBuf, Value)> = self.files().into_iter().filter_map(|(p, src)| self.entry_for(&p, src).map(|e| (p, e))).collect();
        let mut slots = self.slots.lock().unwrap();
        slots.retain(|s| s.model.is_some() || s.status == "loading" || found.iter().any(|(p, _)| p == &s.path));
        for (p, mut e) in found {
            if slots.iter().any(|s| s.path == p) {
                continue;
            }
            let mut id = e["id"].as_str().unwrap_or("model").to_string();
            if slots.iter().any(|s| s.entry["id"] == json!(id)) {
                id = format!("{id}-{}", slug(e["quant"].as_str().unwrap_or("2")));
                e["id"] = json!(id);
            }
            slots.push(Slot { entry: e, path: p, status: "unloaded", error: None, note: None, model: None, vram: None, load_s: None, device: None });
        }
        // biggest model first
        slots.sort_by(|a, b| b.entry["file_bytes"].as_u64().cmp(&a.entry["file_bytes"].as_u64()));
    }

    pub fn entries(&self) -> Vec<Value> {
        self.slots.lock().unwrap().iter().map(Slot::public).collect()
    }

    pub fn default_id(&self) -> Option<String> {
        let slots = self.slots.lock().unwrap();
        let d = self.settings.lock().unwrap().default_model.clone();
        if let Some(d) = d.filter(|d| slots.iter().any(|s| s.entry["id"] == json!(d))) {
            return Some(d);
        }
        slots.iter().find(|s| s.status == "ready").or(slots.first()).map(|s| s.entry["id"].as_str().unwrap_or("").to_string())
    }

    /// "FERMI-2B", "fermi-2b", "2B": every way the demos and the API name a model.
    pub fn resolve(&self, mid: Option<&str>) -> Result<String, String> {
        let mid = match mid.filter(|m| !m.is_empty()) {
            Some(m) => m.to_string(),
            None => self.default_id().ok_or("no model installed: download one in the Models page")?,
        };
        let low = mid.to_lowercase();
        let slots = self.slots.lock().unwrap();
        for s in slots.iter() {
            let id = s.entry["id"].as_str().unwrap_or("");
            let name = s.entry["name"].as_str().unwrap_or("").to_lowercase();
            let size = s.entry["size"].as_str().unwrap_or("").to_lowercase();
            if low == id || low == name || low == size || low == format!("fermi-{size}") {
                return Ok(id.to_string());
            }
        }
        let have: Vec<String> = slots.iter().map(|s| s.entry["id"].as_str().unwrap_or("").to_string()).collect();
        Err(format!("unknown model '{mid}' (available: {})", have.join(", ")))
    }

    pub fn public(&self, id: &str) -> Value {
        self.slots.lock().unwrap().iter().find(|s| s.entry["id"] == json!(id)).map(Slot::public).unwrap_or(Value::Null)
    }

    /// The device to use: the one chosen in the settings, otherwise the first GPU, otherwise the CPU (-1).
    pub fn pick_device(rt: &Runtime, choice: &str) -> (i32, String) {
        let devs = rt.devices();
        if choice == "cpu" {
            return (-1, "CPU".into());
        }
        if let Some(d) = devs.iter().find(|d| d.name == choice && d.kind != "cpu") {
            return (d.index, d.description.clone());
        }
        let best = devs.iter().find(|d| d.kind == "gpu").or_else(|| devs.iter().find(|d| d.kind == "igpu"));
        best.map(|d| (d.index, d.description.clone())).unwrap_or((-1, "CPU".into()))
    }

    /// Starts loading a model (in the background) and returns at once; with wait, returns when it is ready.
    pub fn load(self: &Arc<Self>, mid: Option<&str>, wait: bool) -> Result<Option<Arc<Fermi>>, String> {
        let id = self.resolve(mid)?;
        let start = {
            let mut slots = self.slots.lock().unwrap();
            let s = slots.iter_mut().find(|s| s.entry["id"] == json!(id)).unwrap();
            if let Some(m) = &s.model {
                return Ok(Some(m.clone()));
            }
            let start = matches!(s.status, "unloaded" | "error");
            if start {
                s.status = "loading";
                s.error = None;
            }
            start
        };
        if start {
            let me = self.clone();
            let id2 = id.clone();
            std::thread::spawn(move || me.load_now(&id2));
        }
        if !wait {
            return Ok(None);
        }
        loop {
            {
                let slots = self.slots.lock().unwrap();
                let s = slots.iter().find(|s| s.entry["id"] == json!(id)).ok_or("model removed")?;
                match s.status {
                    "ready" => return Ok(s.model.clone()),
                    "error" => return Err(format!("{} could not be loaded: {}", s.entry["name"].as_str().unwrap_or(&id), s.error.clone().unwrap_or_default())),
                    "unloaded" => return Err("model unloaded".into()),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn load_now(&self, id: &str) {
        let _one = self.load_lock.lock().unwrap(); // one at a time: the memory of each model is measured alone
        let path = {
            let slots = self.slots.lock().unwrap();
            match slots.iter().find(|s| s.entry["id"] == json!(id)) {
                Some(s) => s.path.clone(),
                None => return,
            }
        };
        let set = |f: &dyn Fn(&mut Slot)| {
            if let Some(s) = self.slots.lock().unwrap().iter_mut().find(|s| s.entry["id"] == json!(id)) {
                f(s);
            }
        };
        let Some(rt) = self.rt.clone() else {
            set(&|s| { s.status = "error"; s.error = Some("the compute runtime (llama.cpp) is missing: reinstall the app".into()); });
            return;
        };
        let (choice, threads) = {
            let st = self.settings.lock().unwrap();
            (st.device.clone(), if st.threads > 0 { st.threads } else { super::auto_threads() })
        };
        let (dev, dev_name) = Self::pick_device(&rt, &choice);
        log(&format!("loading {} on {dev_name}", path.display()));
        let t = Instant::now();
        let free_before = rt.devices().iter().find(|d| d.index == dev).map(|d| d.free);
        let (n_ctx, n_seq) = if dev >= 0 { (8192, 8) } else { (4096, 4) };
        let mut res = Fermi::load(&rt, &path, dev, n_ctx, n_seq, threads);
        let mut note = None;
        let mut used = dev_name.clone();
        if let Err(e) = &res {
            if dev >= 0 && choice == "auto" {
                // not enough video memory (or a broken driver): the CPU always works
                log(&format!("GPU load failed ({e}), trying the CPU"));
                note = Some(format!("Caricato sulla CPU: sulla scheda video non è stato possibile ({e})."));
                used = "CPU".into();
                res = Fermi::load(&rt, &path, -1, 4096, 4, threads);
            }
        }
        match res {
            Ok(m) => {
                let vram = if used != "CPU" {
                    free_before.and_then(|b| rt.devices().iter().find(|d| d.index == dev).map(|d| b.saturating_sub(d.free)))
                } else {
                    None
                };
                let secs = (t.elapsed().as_secs_f64() * 10.0).round() / 10.0;
                log(&format!("ready {id} on {used} in {secs:.1}s"));
                let m = Arc::new(m);
                set(&|s| {
                    s.model = Some(m.clone());
                    s.status = "ready";
                    s.vram = vram;
                    s.load_s = Some(secs);
                    s.device = Some(used.clone());
                    s.note = note.clone();
                });
            }
            Err(e) => {
                log(&format!("cannot load {id}: {e}"));
                set(&|s| { s.status = "error"; s.error = Some(e.clone()); s.model = None; });
            }
        }
    }

    pub fn unload(&self, mid: Option<&str>) -> Result<Value, String> {
        let id = self.resolve(mid)?;
        let model = {
            let mut slots = self.slots.lock().unwrap();
            let s = slots.iter_mut().find(|s| s.entry["id"] == json!(id)).unwrap();
            if s.status == "loading" {
                return Err("still loading".into());
            }
            s.status = "unloaded";
            s.vram = None;
            s.device = None;
            s.note = None;
            s.model.take()
        };
        drop(model); // the memory goes when the last running request using it ends
        Ok(self.public(&id))
    }

    /// Loads the loaded models again (on the device and with the threads now in the settings).
    pub fn reload_ready(self: &Arc<Self>) {
        let ids: Vec<String> = self.slots.lock().unwrap().iter().filter(|s| s.status == "ready")
            .filter_map(|s| s.entry["id"].as_str().map(str::to_string)).collect();
        for id in ids {
            if self.unload(Some(&id)).is_ok() {
                let _ = self.load(Some(&id), false);
            }
        }
    }

    pub fn get(self: &Arc<Self>, mid: Option<&str>) -> Result<(String, Arc<Fermi>), String> {
        let id = self.resolve(mid)?;
        let m = self.load(Some(&id), true)?.ok_or("model not loaded")?;
        Ok((id, m))
    }

    /// Adds a model file from anywhere on the computer.
    pub fn add(&self, path: &str) -> Result<Value, String> {
        let p = PathBuf::from(path);
        if !p.is_file() {
            return Err(format!("file not found: {path}"));
        }
        self.entry_for(&p, "added").ok_or("not a FERMI model file (.gguf with the FERMI head)")?;
        {
            let mut st = self.settings.lock().unwrap();
            if !st.extra_models.iter().any(|x| x == path) {
                st.extra_models.push(path.to_string());
                st.save();
            }
        }
        self.rescan();
        let slots = self.slots.lock().unwrap();
        Ok(slots.iter().find(|s| s.path == p).map(Slot::public).unwrap_or(Value::Null))
    }

    /// Removes a model from the list; a downloaded file is deleted from the disk.
    pub fn remove(&self, mid: Option<&str>) -> Result<Value, String> {
        let id = self.resolve(mid)?;
        self.unload(Some(&id))?;
        let path = {
            let mut slots = self.slots.lock().unwrap();
            let i = slots.iter().position(|s| s.entry["id"] == json!(id)).unwrap();
            slots.remove(i).path
        };
        if path.starts_with(&self.dir) {
            fs::remove_file(&path).map_err(|e| format!("cannot delete {}: {e}", path.display()))?;
        } else {
            let mut st = self.settings.lock().unwrap();
            st.extra_models.retain(|x| Path::new(x) != path);
            st.save();
        }
        Ok(json!({"removed": id}))
    }

    // ------------------------------------------------------------------------------------------------- downloads
    pub fn catalog(&self) -> Vec<Value> {
        let slots = self.slots.lock().unwrap();
        let dls = self.downloads.lock().unwrap();
        let devs = self.rt.as_ref().map(|r| r.devices()).unwrap_or_default();
        let gpu = devs.iter().find(|d| d.kind == "gpu").or_else(|| devs.iter().find(|d| d.kind == "igpu"));
        CATALOG.iter().map(|c| {
            let file = Path::new(c.file).file_name().unwrap();
            let installed = slots.iter().any(|s| s.path.file_name() == Some(file));
            let d = dls.get(c.id);
            json!({
                "id": c.id, "name": c.name, "size": c.size, "bytes": c.bytes, "memory": c.memory, "description": c.description,
                "url": format!("https://huggingface.co/{}", c.repo), "installed": installed,
                "fits_gpu": gpu.map(|g| c.memory == 0 || g.total > c.memory),
                "download": d.map(|d| json!({"state": d.state, "done": d.done, "total": d.total, "speed": d.speed, "error": d.error})),
            })
        }).collect()
    }

    pub fn download(self: &Arc<Self>, id: &str) -> Result<Value, String> {
        let c = CATALOG.iter().find(|c| c.id == id).ok_or(format!("unknown model '{id}'"))?;
        {
            let mut dls = self.downloads.lock().unwrap();
            if dls.get(id).map(|d| d.state == "running").unwrap_or(false) {
                return Ok(json!({"started": false}));
            }
            dls.insert(id.to_string(), Download { state: "running", ..Default::default() });
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let res = me.fetch(c);
            let mut dls = me.downloads.lock().unwrap();
            let d = dls.get_mut(c.id).unwrap();
            match res {
                Ok(()) => d.state = "done",
                Err(e) if d.cancel.load(Ordering::Relaxed) => {
                    d.state = "cancelled";
                    log(&format!("download {} cancelled: {e}", c.id));
                }
                Err(e) => {
                    log(&format!("download {} failed: {e}", c.id));
                    d.state = "error";
                    d.error = Some(e);
                }
            }
            let ok = d.state == "done";
            drop(dls);
            me.rescan();
            // the first model on this computer: load it now and at every start, so the app is ready to use
            let first = ok && !me.slots.lock().unwrap().iter().any(|s| s.status == "ready" || s.status == "loading");
            if first {
                {
                    let mut st = me.settings.lock().unwrap();
                    if st.autoload.is_empty() {
                        st.autoload.push(c.id.to_string());
                        st.save();
                    }
                }
                let _ = me.load(Some(c.id), false);
            }
        });
        Ok(json!({"started": true}))
    }

    pub fn cancel_download(&self, id: &str) {
        if let Some(d) = self.downloads.lock().unwrap().get(id) {
            d.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn progress(&self, id: &str, done: u64, total: u64, speed: f64) -> bool {
        let mut dls = self.downloads.lock().unwrap();
        let d = dls.get_mut(id).unwrap();
        d.done = done;
        d.total = total;
        d.speed = speed;
        !d.cancel.load(Ordering::Relaxed)
    }

    /// Downloads one file, resuming a partial download (.part) left by a closed app or a lost connection.
    fn fetch(&self, c: &CatalogEntry) -> Result<(), String> {
        let name = Path::new(c.file).file_name().unwrap();
        let dest = self.dir.join(name);
        let part = self.dir.join(format!("{}.part", name.to_string_lossy()));
        // HF_ENDPOINT: a Hugging Face mirror, as in the Hugging Face tools
        let host = std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".into());
        let url = format!("{}/{}/resolve/{}/{}", host.trim_end_matches('/'), c.repo, c.revision, c.file);
        log(&format!("downloading {url}"));
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(60))
            .user_agent(concat!("FERMI-app/", env!("CARGO_PKG_VERSION")))
            .build();
        let mut tries = 0;
        loop {
            let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            let mut req = agent.get(&url);
            if have > 0 {
                req = req.set("Range", &format!("bytes={have}-"));
            }
            let resp = match req.call() {
                Ok(r) => r,
                Err(ureq::Error::Status(416, _)) if have > 0 => break, // already complete
                Err(ureq::Error::Status(code, _)) => return Err(format!("il server di Hugging Face ha risposto {code}")),
                Err(e) => {
                    tries += 1;
                    if tries > 5 {
                        return Err(format!("nessuna connessione a internet ({e})"));
                    }
                    std::thread::sleep(Duration::from_secs(3 * tries));
                    continue;
                }
            };
            let resumed = resp.status() == 206;
            let len: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
            let total = if resumed { have + len } else { len };
            let mut f = fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part)
                .map_err(|e| format!("impossibile scrivere {}: {e}", part.display()))?;
            let mut done = if resumed { have } else { 0 };
            let mut r = resp.into_reader();
            let mut buf = vec![0u8; 1 << 20];
            let (mut t, mut d0) = (Instant::now(), done);
            let mut speed = 0.0;
            let res: Result<(), String> = loop {
                match r.read(&mut buf) {
                    Ok(0) => break Ok(()),
                    Ok(n) => {
                        f.write_all(&buf[..n]).map_err(|e| format!("errore del disco (spazio finito?): {e}"))?;
                        done += n as u64;
                        if t.elapsed() > Duration::from_millis(400) {
                            speed = (done - d0) as f64 / t.elapsed().as_secs_f64();
                            t = Instant::now();
                            d0 = done;
                            if !self.progress(c.id, done, total, speed) {
                                return Err("cancelled".into());
                            }
                        }
                    }
                    Err(e) => break Err(e.to_string()),
                }
            };
            f.flush().ok();
            self.progress(c.id, done, total, speed);
            match res {
                Ok(()) if total == 0 || done >= total => break,
                Ok(()) | Err(_) => {
                    tries += 1;
                    if tries > 5 {
                        return Err(res.err().unwrap_or_else(|| "la connessione si è interrotta".into()));
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
        if !c.sha256.is_empty() {
            let mut h = Sha256::new();
            let mut f = fs::File::open(&part).map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let got = format!("{:x}", h.finalize());
            if got != c.sha256 {
                let _ = fs::remove_file(&part);
                return Err("il file scaricato è danneggiato: riprova".into());
            }
        }
        fs::rename(&part, &dest).map_err(|e| e.to_string())?;
        log(&format!("downloaded {}", dest.display()));
        Ok(())
    }
}

pub fn devices_json(rt: Option<&Arc<Runtime>>) -> Vec<Device> {
    rt.map(|r| r.devices()).unwrap_or_default()
}
