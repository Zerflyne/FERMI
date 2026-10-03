//! Folder analyses: every state found in a folder gets the same questions, in the background. Results are written
//! line by line (results.jsonl), so an analysis stopped by closing the app or by a power cut resumes where it was.
//! Same files and formats as the Python engine, so analyses made with it still open.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use serde_json::{json, Map, Value};

use super::fermi::{check_questions, expand, merge, state_text};
use super::models::Models;
use super::{log, now};

const TEXT_EXT: &[&str] = &["txt", "md", "log", "eml", "html", "htm", "xml", "yaml", "yml", "rst", "ini", "cfg", "py", "js",
                            "ts", "java", "c", "cpp", "go", "rs", "sql", "sh"];
const DATA_EXT: &[&str] = &["json", "jsonl", "ndjson", "csv", "tsv"];
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "__pycache__", "site-packages", ".cache", "target", "dist"];
const MAX_FILE: u64 = 2_000_000; // bytes read from a text file (the model keeps 2,048 tokens anyway)
const KEEP_TEXT: usize = 4000; // characters of each state kept in the results, for the table and the viewer

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(p)
}

fn read_text(p: &Path) -> std::io::Result<String> {
    let mut raw = Vec::new();
    fs::File::open(p)?.take(MAX_FILE).read_to_end(&mut raw)?;
    Ok(match String::from_utf8(raw) {
        Ok(s) => s,
        // not UTF-8: Windows-1252 (Latin-1 for the bytes that matter), like the Python engine
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The files of a source: {folder, recursive?, extensions?}.
pub fn list_files(src: &Value) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let folder = expand_home(src["folder"].as_str().ok_or("folder missing")?);
    let recursive = src["recursive"].as_bool().unwrap_or(true);
    let exts: Vec<String> = src["extensions"].as_array().map(|a| {
        a.iter().filter_map(Value::as_str).map(|e| e.trim_start_matches('.').to_lowercase()).collect()
    }).filter(|v: &Vec<String>| !v.is_empty()).unwrap_or_else(|| TEXT_EXT.iter().chain(DATA_EXT).map(|s| s.to_string()).collect());
    if folder.is_file() {
        return Ok((folder.parent().map(Path::to_path_buf).unwrap_or_default(), vec![folder]));
    }
    if !folder.is_dir() {
        return Err(format!("folder not found: {}", folder.display()));
    }
    let mut out = Vec::new();
    fn walk(d: &Path, recursive: bool, exts: &[String], out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let (mut dirs, mut files) = (Vec::new(), Vec::new());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if recursive && !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    dirs.push(p);
                }
            } else if exts.contains(&ext_of(&p)) {
                files.push(p);
            }
        }
        out.extend(files);
        for d in dirs {
            walk(&d, recursive, exts, out);
        }
    }
    walk(&folder, recursive, &exts, &mut out);
    Ok((folder, out))
}

/// The states of one file: a text file is one state; .json one state or a list; .jsonl one per line; .csv/.tsv one
/// per row (the columns as fields).
fn states_in(path: &Path, root: &Path, out: &mut dyn FnMut(String, Value) -> bool) -> std::io::Result<bool> {
    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    match ext_of(path).as_str() {
        "jsonl" | "ndjson" => {
            let f = BufReader::new(fs::File::open(path)?);
            for (i, line) in f.split(b'\n').enumerate() {
                let line = String::from_utf8_lossy(&line?).trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let v = serde_json::from_str(&line).unwrap_or(Value::String(line));
                if !out(format!("{rel}#{}", i + 1), v) {
                    return Ok(false);
                }
            }
        }
        "json" => {
            let text = read_text(path)?;
            match serde_json::from_str::<Value>(&text) {
                Ok(Value::Array(a)) => {
                    for (i, x) in a.into_iter().enumerate() {
                        if !out(format!("{rel}#{}", i + 1), x) {
                            return Ok(false);
                        }
                    }
                }
                Ok(v) => return Ok(out(rel, v)),
                Err(_) => return Ok(out(rel, Value::String(text))),
            }
        }
        e @ ("csv" | "tsv") => {
            let mut r = csv::ReaderBuilder::new().delimiter(if e == "tsv" { b'\t' } else { b',' }).flexible(true).from_path(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            let headers = r.headers().map(|h| h.clone()).unwrap_or_default();
            for (i, rec) in r.records().enumerate() {
                let Ok(rec) = rec else { continue };
                let row: Map<String, Value> = headers.iter().zip(rec.iter()).map(|(k, v)| (k.to_string(), json!(v))).collect();
                if !out(format!("{rel}#{}", i + 1), Value::Object(row)) {
                    return Ok(false);
                }
            }
        }
        _ => return Ok(out(rel, Value::String(read_text(path)?))),
    }
    Ok(true)
}

/// Every state of a source, in order; f returns false to stop.
pub fn iter_states(src: &Value, f: &mut dyn FnMut(String, Value) -> bool) -> Result<(), String> {
    let (root, files) = list_files(src)?;
    let limit = src["limit"].as_u64();
    let mut n = 0u64;
    for p in files {
        let mut go = true;
        let res = states_in(&p, &root, &mut |sid, st| {
            n += 1;
            go = f(sid, st) && limit.map(|l| n < l).unwrap_or(true);
            go
        });
        if let Err(e) = res {
            let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().into_owned();
            go = f(rel, json!({"__error__": e.to_string()}));
        }
        if !go {
            break;
        }
    }
    Ok(())
}

/// Fields hidden from the model (kept as metadata, e.g. a human label to compare with).
fn split_hidden(state: Value, hidden: &[String]) -> (Value, Map<String, Value>) {
    match state {
        Value::Object(mut m) if !hidden.is_empty() => {
            let mut meta = Map::new();
            for k in hidden {
                if let Some(v) = m.remove(k) {
                    meta.insert(k.clone(), v);
                }
            }
            (Value::Object(m), meta)
        }
        s => (s, Map::new()),
    }
}

/// A quick look at a source before the analysis: how many files and states, a sample, and the fields of the states
/// (with a guess of which ones look like labels, to hide them from the model).
pub fn scan(src: &Value) -> Result<Value, String> {
    let (_, files) = list_files(src)?;
    let mut by_ext = Map::new();
    for f in &files {
        let e = format!(".{}", ext_of(f));
        let n = by_ext.get(&e).and_then(Value::as_u64).unwrap_or(0);
        by_ext.insert(e, json!(n + 1));
    }
    struct Field {
        name: String,
        n: u64,
        avg: f64,
        values: Vec<(String, u64)>,
    }
    let mut fields: Vec<Field> = Vec::new();
    let mut sample = Vec::new();
    let mut n = 0u64;
    let t = Instant::now();
    let mut src2 = src.clone();
    src2["limit"] = Value::Null;
    iter_states(&src2, &mut |sid, st| {
        n += 1;
        if sample.len() < 3 {
            sample.push(json!({"id": sid, "text": truncate_chars(&state_text(&st), 1500)}));
        }
        if let (Value::Object(m), true) = (&st, n <= 500) {
            for (k, v) in m {
                let s = match v { Value::String(s) => s.clone(), other => other.to_string() };
                let f = match fields.iter_mut().position(|f| &f.name == k) {
                    Some(i) => &mut fields[i],
                    None => {
                        fields.push(Field { name: k.clone(), n: 0, avg: 0.0, values: Vec::new() });
                        fields.last_mut().unwrap()
                    }
                };
                f.n += 1;
                f.avg += (s.chars().count() as f64 - f.avg) / f.n as f64;
                let key = truncate_chars(&s, 60);
                if f.values.len() <= 30 {
                    match f.values.iter_mut().find(|(v, _)| v == &key) {
                        Some(x) => x.1 += 1,
                        None => f.values.push((key, 1)),
                    }
                }
            }
        }
        t.elapsed().as_secs() < 20 && n < 200_000 // very big folders: stop counting
    })?;
    let out: Vec<Value> = fields.iter().map(|f| {
        let distinct = f.values.len();
        let mut vals = f.values.clone();
        vals.sort_by(|a, b| b.1.cmp(&a.1));
        json!({"name": f.name, "present": f.n, "avg_len": f.avg.round(),
               "distinct": if distinct <= 30 { json!(distinct) } else { Value::Null },
               "examples": vals.iter().take(6).map(|v| v.0.clone()).collect::<Vec<_>>(),
               "label_like": distinct <= 30 && f.avg < 40.0})
    }).collect();
    Ok(json!({"files": files.len(), "states": n, "by_ext": by_ext, "sample": sample, "fields": out,
              "complete": t.elapsed().as_secs() < 20 && n < 200_000}))
}

// ------------------------------------------------------------------------------------------------------------ jobs
const FIELDS: &[&str] = &["id", "name", "created", "spec", "status", "total", "done", "errors", "started", "finished",
                          "elapsed", "error", "model_name"];

pub struct Job {
    pub dir: PathBuf,
    /// the fields saved in job.json
    pub d: Map<String, Value>,
    pub stop: Arc<AtomicBool>,
    pub rate: Option<f64>,
}

impl Job {
    fn get_f(&self, k: &str) -> f64 {
        self.d.get(k).and_then(Value::as_f64).unwrap_or(0.0)
    }

    pub fn public(&self) -> Value {
        let mut d = self.d.clone();
        d.insert("rate".into(), json!(self.rate));
        let running = d.get("status") == Some(&json!("running"));
        let eta = match (self.rate, d.get("total").and_then(Value::as_f64)) {
            (Some(r), Some(t)) if r > 0.0 && running => json!(((t - self.get_f("done")) / r).round()),
            _ => Value::Null,
        };
        d.insert("eta".into(), eta);
        Value::Object(d)
    }

    pub fn save(&self) {
        let keep: Map<String, Value> = FIELDS.iter().filter_map(|k| self.d.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
        let tmp = self.dir.join("job.json.tmp");
        if fs::write(&tmp, serde_json::to_string_pretty(&keep).unwrap()).is_ok() {
            let _ = fs::rename(&tmp, self.dir.join("job.json"));
        }
    }

    fn results_path(&self) -> PathBuf {
        self.dir.join("results.jsonl")
    }

    pub fn read_results(&self, since: usize) -> Vec<Value> {
        let Ok(f) = fs::File::open(self.results_path()) else { return Vec::new() };
        let mut out = Vec::new();
        for (i, line) in BufReader::new(f).lines().enumerate() {
            let Ok(line) = line else { break };
            if i < since {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(v) => out.push(v),
                Err(_) => break, // a half-written last line after a power cut
            }
        }
        out
    }

    fn set(&mut self, k: &str, v: Value) {
        self.d.insert(k.into(), v);
    }
}

pub struct Jobs {
    pub root: PathBuf,
    models: Arc<Models>,
    pub jobs: Mutex<Vec<Arc<Mutex<Job>>>>,
    queue: Mutex<VecDeque<String>>,
    cv: Condvar,
}

impl Jobs {
    pub fn new(data_dir: &Path, models: Arc<Models>) -> Arc<Jobs> {
        let root = data_dir.join("jobs");
        let _ = fs::create_dir_all(&root);
        let mut list = Vec::new();
        if let Ok(rd) = fs::read_dir(&root) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            dirs.sort();
            for d in dirs {
                let Ok(text) = fs::read_to_string(d.join("job.json")) else { continue };
                let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text) else { continue };
                let mut j = Job { dir: d, d: m, stop: Arc::new(AtomicBool::new(false)), rate: None };
                if matches!(j.d.get("status").and_then(Value::as_str), Some("running" | "queued")) {
                    j.set("status", json!("interrupted")); // the app stopped while it ran (closed, power cut)
                    j.save();
                }
                list.push(Arc::new(Mutex::new(j)));
            }
        }
        let jobs = Arc::new(Jobs { root, models, jobs: Mutex::new(list), queue: Mutex::new(VecDeque::new()), cv: Condvar::new() });
        let w = jobs.clone();
        std::thread::spawn(move || w.worker());
        jobs
    }

    pub fn list(&self) -> Vec<Value> {
        let mut v: Vec<Value> = self.jobs.lock().unwrap().iter().map(|j| j.lock().unwrap().public()).collect();
        v.sort_by(|a, b| b["created"].as_f64().unwrap_or(0.0).partial_cmp(&a["created"].as_f64().unwrap_or(0.0)).unwrap());
        v
    }

    pub fn find(&self, id: &str) -> Option<Arc<Mutex<Job>>> {
        self.jobs.lock().unwrap().iter().find(|j| j.lock().unwrap().d.get("id") == Some(&json!(id))).cloned()
    }

    pub fn create(&self, mut spec: Value) -> Result<Value, String> {
        let qs = spec["questions"].as_object().cloned().ok_or("no questions")?;
        check_questions(&qs)?;
        list_files(&spec["source"])?;
        let mid = self.models.resolve(spec["model"].as_str())?;
        spec["model"] = json!(mid);
        let model_name = self.models.public(&mid)["name"].clone();
        let jid = format!("{}-{:04x}", chrono::Local::now().format("%Y%m%d-%H%M%S"), (now().fract() * 65535.0) as u32);
        let dir = self.root.join(&jid);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let folder = spec["source"]["folder"].as_str().unwrap_or("");
        let name = spec["name"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| {
            Path::new(folder.trim_end_matches(['/', '\\'])).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        });
        let mut d = Map::new();
        for (k, v) in [("id", json!(jid)), ("name", json!(name)), ("created", json!(now())), ("spec", spec), ("status", json!("queued")),
                       ("total", Value::Null), ("done", json!(0)), ("errors", json!(0)), ("started", Value::Null),
                       ("finished", Value::Null), ("elapsed", json!(0.0)), ("error", Value::Null), ("model_name", model_name)] {
            d.insert(k.into(), v);
        }
        let j = Arc::new(Mutex::new(Job { dir, d, stop: Arc::new(AtomicBool::new(false)), rate: None }));
        j.lock().unwrap().save();
        self.jobs.lock().unwrap().push(j.clone());
        self.enqueue(&j);
        let p = j.lock().unwrap().public();
        Ok(p)
    }

    pub fn enqueue(&self, j: &Arc<Mutex<Job>>) {
        let id = {
            let mut g = j.lock().unwrap();
            g.set("status", json!("queued"));
            g.set("error", Value::Null);
            g.stop.store(false, Ordering::Relaxed);
            g.save();
            g.d["id"].as_str().unwrap_or("").to_string()
        };
        let mut q = self.queue.lock().unwrap();
        if !q.contains(&id) {
            q.push_back(id);
        }
        self.cv.notify_one();
    }

    pub fn stop(&self, j: &Arc<Mutex<Job>>) {
        let mut g = j.lock().unwrap();
        g.stop.store(true, Ordering::Relaxed);
        let id = g.d["id"].as_str().unwrap_or("").to_string();
        let mut q = self.queue.lock().unwrap();
        if let Some(i) = q.iter().position(|x| x == &id) {
            q.remove(i);
            g.set("status", json!("paused"));
            g.save();
        }
    }

    pub fn delete(&self, j: &Arc<Mutex<Job>>) {
        self.stop(j);
        for _ in 0..50 {
            if j.lock().unwrap().d.get("status") != Some(&json!("running")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let dir = j.lock().unwrap().dir.clone();
        let _ = fs::remove_dir_all(dir);
        self.jobs.lock().unwrap().retain(|x| !Arc::ptr_eq(x, j));
    }

    fn worker(&self) {
        loop {
            let id = {
                let mut q = self.queue.lock().unwrap();
                while q.is_empty() {
                    q = self.cv.wait(q).unwrap();
                }
                q.pop_front().unwrap()
            };
            let Some(j) = self.find(&id) else { continue };
            if let Err(e) = self.run(&j) {
                log(&format!("job {id} failed: {e}"));
                let mut g = j.lock().unwrap();
                g.set("status", json!("error"));
                g.set("error", json!(e));
                g.rate = None;
                g.save();
            }
        }
    }

    fn run(&self, j: &Arc<Mutex<Job>>) -> Result<(), String> {
        let (spec, stop, path) = {
            let mut g = j.lock().unwrap();
            g.set("status", json!("running"));
            if g.d.get("started").map(Value::is_null).unwrap_or(true) {
                g.set("started", json!(now()));
            }
            g.save();
            (g.d["spec"].clone(), g.stop.clone(), g.results_path())
        };
        let (_, model) = self.models.get(spec["model"].as_str())?;
        let prev = j.lock().unwrap().read_results(0);
        let done: HashSet<String> = prev.iter().filter_map(|r| r["sid"].as_str().map(str::to_string)).collect();
        {
            let mut g = j.lock().unwrap();
            g.set("done", json!(done.len()));
            g.set("errors", json!(prev.iter().filter(|r| !r["error"].is_null()).count()));
            if g.d.get("total").map(Value::is_null).unwrap_or(true) {
                let mut n = 0u64;
                iter_states(&spec["source"], &mut |_, _| { n += 1; true })?;
                g.set("total", json!(n));
                g.save();
            }
        }
        let questions = spec["questions"].as_object().cloned().unwrap_or_default();
        let orders = spec.get("orders").cloned().unwrap_or(json!("reverse"));
        let (exp, plan) = expand(&questions, &orders);
        let hidden: Vec<String> = spec["hidden"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let chunk = model.lm.n_seq.max(4) * 2;
        let mut out = fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let d0 = j.lock().unwrap().get_f("done");
        let mut last_save = Instant::now();
        let mut buf: Vec<(String, Value, Map<String, Value>)> = Vec::new();
        let write_rows = |buf: &mut Vec<(String, Value, Map<String, Value>)>, out: &mut fs::File| {
            if buf.is_empty() {
                return;
            }
            let t = Instant::now();
            let pairs: Vec<(&Value, &Map<String, Value>)> = buf.iter().map(|(_, st, _)| (st, &exp)).collect();
            let rows: Vec<Value> = match model.classify_many(&pairs) {
                Ok(res) => {
                    let ms = (t.elapsed().as_millis() as usize / buf.len()) as u64;
                    buf.iter().zip(res).map(|((sid, st, meta), a)| {
                        json!({"sid": sid, "meta": meta, "text": truncate_chars(&state_text(st), KEEP_TEXT), "answers": merge(&questions, &plan, &a), "ms": ms})
                    }).collect()
                }
                Err(_) => buf.iter().map(|(sid, st, meta)| {
                    // one bad batch: one by one, so that only the broken state gets the error
                    match model.classify_many(&[(st, &exp)]) {
                        Ok(a) => json!({"sid": sid, "meta": meta, "text": truncate_chars(&state_text(st), KEEP_TEXT), "answers": merge(&questions, &plan, &a[0]), "ms": 0}),
                        Err(e) => json!({"sid": sid, "meta": meta, "text": truncate_chars(&state_text(st), KEEP_TEXT), "error": e}),
                    }
                }).collect(),
            };
            let mut g = j.lock().unwrap();
            for r in &rows {
                let _ = writeln!(out, "{}", r);
                let done = g.get_f("done") + 1.0;
                g.set("done", json!(done as u64));
                if !r["error"].is_null() {
                    let e = g.get_f("errors") + 1.0;
                    g.set("errors", json!(e as u64));
                }
            }
            let _ = out.flush();
            buf.clear();
        };
        iter_states(&spec["source"], &mut |sid, st| {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            if done.contains(&sid) {
                return true;
            }
            if let Some(e) = st.get("__error__") {
                let _ = writeln!(out, "{}", json!({"sid": sid, "meta": {}, "text": "", "error": e}));
                let mut g = j.lock().unwrap();
                let (d, er) = (g.get_f("done") + 1.0, g.get_f("errors") + 1.0);
                g.set("done", json!(d as u64));
                g.set("errors", json!(er as u64));
                return true;
            }
            let (st, meta) = split_hidden(st, &hidden);
            buf.push((sid, st, meta));
            if buf.len() >= chunk {
                write_rows(&mut buf, &mut out);
                let mut g = j.lock().unwrap();
                let el = t0.elapsed().as_secs_f64();
                g.rate = (el > 0.0).then(|| ((g.get_f("done") - d0) / el * 100.0).round() / 100.0);
                if last_save.elapsed().as_secs_f64() > 3.0 {
                    let e = g.get_f("elapsed") + last_save.elapsed().as_secs_f64();
                    g.set("elapsed", json!(e));
                    last_save = Instant::now();
                    g.save();
                }
            }
            true
        })?;
        if !stop.load(Ordering::Relaxed) {
            write_rows(&mut buf, &mut out);
        }
        let mut g = j.lock().unwrap();
        let e = g.get_f("elapsed") + last_save.elapsed().as_secs_f64();
        g.set("elapsed", json!(e));
        if stop.load(Ordering::Relaxed) {
            g.set("status", json!("paused"));
        } else {
            g.set("status", json!("done"));
            g.set("finished", json!(now()));
        }
        g.rate = None;
        g.save();
        log(&format!("job {} {} {}/{}", g.d["id"], g.d["status"], g.d["done"], g.d["total"]));
        Ok(())
    }
}

// ----------------------------------------------------------------------------------------------------------- export
/// Results as flat rows for CSV: id, metadata, then the answers of every question.
fn flat_rows(j: &Job) -> (Vec<String>, Vec<Map<String, Value>>) {
    let qs = j.d["spec"]["questions"].as_object().cloned().unwrap_or_default();
    let results = j.read_results(0);
    let mut cols: Vec<String> = vec!["id".into()];
    let add = |cols: &mut Vec<String>, k: &str| {
        if !cols.iter().any(|c| c == k) {
            cols.push(k.to_string());
        }
    };
    for r in &results {
        if let Some(m) = r["meta"].as_object() {
            for k in m.keys() {
                add(&mut cols, k);
            }
        }
    }
    let mut rows = Vec::new();
    for r in &results {
        let mut row = Map::new();
        row.insert("id".into(), r["sid"].clone());
        if let Some(m) = r["meta"].as_object() {
            for (k, v) in m {
                row.insert(k.clone(), v.clone());
            }
        }
        for (qid, q) in &qs {
            let x = &r["answers"][qid];
            if x.is_null() {
                continue;
            }
            match q["type"].as_str() {
                Some("noul") => {
                    row.insert(qid.clone(), x["answer"].clone());
                    row.insert(format!("{qid}.p_yes"), x["p_yes"].clone());
                }
                Some("choice") => {
                    row.insert(qid.clone(), x["answer"].clone());
                    if let Some(p) = x["probabilities"].as_object() {
                        for (k, v) in p {
                            row.insert(format!("{qid}.p_{k}"), v.clone());
                        }
                    }
                }
                _ => {
                    row.insert(qid.clone(), x["answer_text"].clone());
                    row.insert(format!("{qid}.expected"), x["expected"].clone());
                    row.insert(format!("{qid}.expected_01"), x["expected_01"].clone());
                }
            }
        }
        if !r["error"].is_null() {
            row.insert("error".into(), r["error"].clone());
        }
        row.insert("text".into(), json!(truncate_chars(r["text"].as_str().unwrap_or(""), 1000)));
        for k in row.keys() {
            add(&mut cols, k);
        }
        rows.push(row);
    }
    (cols, rows)
}

pub fn export(j: &Job, fmt: &str) -> String {
    if fmt == "json" {
        return serde_json::to_string_pretty(&json!({"job": j.public(), "results": j.read_results(0)})).unwrap();
    }
    let (cols, rows) = flat_rows(j);
    let mut w = csv::Writer::from_writer(Vec::new());
    let _ = w.write_record(&cols);
    for r in rows {
        let rec: Vec<String> = cols.iter().map(|c| match r.get(c) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
        }).collect();
        let _ = w.write_record(&rec);
    }
    String::from_utf8(w.into_inner().unwrap_or_default()).unwrap_or_default()
}
