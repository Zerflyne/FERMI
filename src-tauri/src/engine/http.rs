//! The two HTTP servers: the app's own (127.0.0.1 only, everything the UI needs) and the public API server, switched
//! on and off from the app (/api/classify, /v1/classify, /api/info, /api/models), like LM Studio's local server.
//! Same routes and answers as the old Python engine: the UI, the demos and the video tests talk to either.

use std::io::{Cursor, Read};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use super::jobs::{export, scan};
use super::{log, now, Engine, VERSION};

const MAX_BODY: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    App,
    Api,
}

pub struct HttpError(pub u16, pub String);

impl From<String> for HttpError {
    fn from(s: String) -> Self {
        let code = if s.starts_with("unknown model") || s.contains("not found") { 404 } else { 400 };
        HttpError(code, s)
    }
}
impl From<&str> for HttpError {
    fn from(s: &str) -> Self {
        HttpError::from(s.to_string())
    }
}

type Reply = Result<(u16, Vec<u8>, &'static str), HttpError>;

fn ok(v: Value) -> Reply {
    Ok((200, serde_json::to_vec(&v).unwrap(), "application/json"))
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

/// Serves a bound server with a few threads until it is unblocked.
pub fn serve(engine: Arc<Engine>, server: Arc<Server>, role: Role, threads: usize) {
    for _ in 0..threads {
        let (e, s) = (engine.clone(), server.clone());
        std::thread::spawn(move || {
            while let Ok(req) = s.recv() {
                handle(&e, req, role);
            }
        });
    }
}

fn is_loopback(req: &Request) -> bool {
    req.remote_addr().map(|a| match a.ip() {
        IpAddr::V4(v) => v.is_loopback(),
        IpAddr::V6(v) => v.is_loopback() || v.to_ipv4_mapped().map(|x| x.is_loopback()).unwrap_or(false),
    }).unwrap_or(true)
}

fn handle(engine: &Arc<Engine>, mut req: Request, role: Role) {
    let t = Instant::now();
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.to_string();
    let q: Vec<(String, String)> = query.split('&').filter(|x| !x.is_empty()).map(|x| {
        let (k, v) = x.split_once('=').unwrap_or((x, ""));
        (k.to_string(), v.to_string())
    }).collect();
    let client = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let mut model = Value::Null;
    let mut nq = 0usize;

    let reply: Reply = if method == Method::Options {
        Ok((204, Vec::new(), "text/plain"))
    } else {
        // the API key is asked only of other computers, never of this one
        let key = engine.settings.lock().unwrap().server.api_key.clone();
        let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
        if role == Role::Api && !key.is_empty() && !is_loopback(&req) && auth != format!("Bearer {key}") {
            Err(HttpError(401, "API key missing or wrong: send Authorization: Bearer <key>".into()))
        } else {
            let mut body = Vec::new();
            let len = req.body_length().unwrap_or(0);
            if len > MAX_BODY {
                Err(HttpError(413, "body too large".into()))
            } else {
                let _ = req.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut body);
                route(engine, role, &method, &path, &q, &body, &mut model, &mut nq)
            }
        }
    };
    let (code, bytes, ctype) = match reply {
        Ok(r) => r,
        Err(HttpError(code, msg)) => (code, serde_json::to_vec(&json!({"error": msg})).unwrap(), "application/json"),
    };
    if path.starts_with("/api/classify") || path.starts_with("/v1/") || role == Role::Api {
        engine.log_request(json!({"t": now(), "server": if role == Role::App { "app" } else { "api" }, "client": client,
                                  "method": method.as_str(), "path": path, "status": code,
                                  "ms": t.elapsed().as_millis() as u64, "model": model, "questions": nq}));
    }
    let resp = Response::new(code.into(), vec![
        header("Content-Type", ctype),
        header("Access-Control-Allow-Origin", "*"),
        header("Access-Control-Allow-Headers", "Content-Type, Authorization"),
        header("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS"),
        header("Cache-Control", "no-store"),
        header("Server", "fermi-engine"),
    ], Cursor::new(bytes.clone()), Some(bytes.len()), None);
    let _ = req.respond(resp);
}

fn parse(body: &[u8]) -> Result<Value, HttpError> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| HttpError(400, format!("invalid JSON: {e}")))
}

#[allow(clippy::too_many_arguments)]
fn route(engine: &Arc<Engine>, role: Role, method: &Method, path: &str, q: &[(String, String)], body: &[u8],
         model: &mut Value, nq: &mut usize) -> Reply {
    let get = *method == Method::Get;
    let post = *method == Method::Post;
    let qv = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());

    // the public routes, on both servers
    if post && (path == "/api/classify" || path == "/v1/classify") {
        let req = parse(body)?;
        let (out, n) = engine.classify(&req)?;
        *model = out["model"].clone();
        *nq = n;
        return ok(out);
    }
    if get && path == "/api/info" {
        return ok(engine.info());
    }
    if get && (path == "/api/models" || path == "/v1/models") {
        return ok(json!({"models": engine.models.entries(), "default": engine.models.default_id()}));
    }
    if role != Role::App {
        return Err(HttpError(404, "not found".into()));
    }

    let models = &engine.models;
    let id_of = |b: &Value| b["id"].as_str().map(str::to_string);
    match (method, path) {
        (Method::Get, "/api/status") => {
            let active: Vec<Value> = engine.jobs.list().into_iter().filter(|j| matches!(j["status"].as_str(), Some("running" | "queued"))).collect();
            ok(json!({"engine": VERSION, "uptime": (now() - engine.started).round(), "data_dir": engine.data_dir.to_string_lossy(),
                      "models": models.entries(), "default": models.default_id(), "catalog": models.catalog(),
                      "runtime": engine.runtime_status(), "server": engine.api_status(),
                      "counter": *engine.counter.lock().unwrap(), "active_jobs": active}))
        }
        (Method::Get, "/api/system") => ok(engine.system_info()),
        (Method::Get, "/api/requests") => {
            let since: f64 = qv("since").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let r: Vec<Value> = engine.requests.lock().unwrap().iter().filter(|r| r["t"].as_f64().unwrap_or(0.0) > since).cloned().collect();
            ok(json!({"requests": r}))
        }
        (Method::Post, "/api/models/load") => {
            let b = parse(body)?;
            let id = models.resolve(id_of(&b).as_deref())?;
            let _ = models.load(Some(&id), false)?;
            ok(models.public(&id))
        }
        (Method::Post, "/api/models/unload") => ok(models.unload(id_of(&parse(body)?).as_deref())?),
        (Method::Post, "/api/models/default") => {
            let id = models.resolve(id_of(&parse(body)?).as_deref())?;
            let mut st = engine.settings.lock().unwrap();
            st.default_model = Some(id.clone());
            st.save();
            ok(json!({"default": id}))
        }
        (Method::Post, "/api/models/add") => {
            let b = parse(body)?;
            let p = b["path"].as_str().or(b["checkpoint"].as_str()).ok_or("path missing")?;
            ok(models.add(p)?)
        }
        (Method::Post, "/api/models/remove") => ok(models.remove(id_of(&parse(body)?).as_deref())?),
        (Method::Post, "/api/models/discover") => {
            let before = models.entries().len();
            models.rescan();
            ok(json!({"added": models.entries().len().saturating_sub(before)}))
        }
        (Method::Post, "/api/models/download") => ok(models.download(&id_of(&parse(body)?).ok_or("id missing")?)?),
        (Method::Post, "/api/models/download/cancel") => {
            models.cancel_download(&id_of(&parse(body)?).ok_or("id missing")?);
            ok(json!({"cancelled": true}))
        }
        (Method::Get, "/api/devices") => ok(json!({"devices": engine.devices()})),
        (Method::Get, "/api/settings") => ok(engine.settings.lock().unwrap().to_json()),
        (Method::Post, "/api/settings") => {
            let b = parse(body)?;
            let (out, moved) = {
                let mut st = engine.settings.lock().unwrap();
                let before = (st.device.clone(), st.threads);
                st.update(&b);
                st.save();
                (st.to_json(), before != (st.device.clone(), st.threads))
            };
            if moved {
                models.reload_ready(); // the loaded models go to the new device now
            }
            ok(out)
        }
        (Method::Get, "/api/server") => ok(engine.api_status()),
        (Method::Post, "/api/server") => {
            let b = parse(body)?;
            let want = {
                let mut st = engine.settings.lock().unwrap();
                if let Some(h) = b["host"].as_str() {
                    if h != "127.0.0.1" && h != "0.0.0.0" {
                        return Err(HttpError(400, "host must be 127.0.0.1 (this computer) or 0.0.0.0 (local network)".into()));
                    }
                    st.server.host = h.to_string();
                }
                if let Some(p) = b["port"].as_u64().or_else(|| b["port"].as_str().and_then(|s| s.parse().ok())) {
                    st.server.port = p as u16;
                }
                if let Some(k) = b["api_key"].as_str() {
                    st.server.api_key = k.to_string();
                }
                let want = b["enabled"].as_bool().unwrap_or(engine.api_running());
                st.server.enabled = want;
                st.save();
                want
            };
            if want {
                engine.api_start();
            } else {
                engine.api_stop();
            }
            ok(engine.api_status())
        }
        (Method::Post, "/api/scan") => ok(scan(&parse(body)?)?),
        (Method::Get, "/api/jobs") => ok(json!({"jobs": engine.jobs.list()})),
        (Method::Post, "/api/jobs") => ok(engine.jobs.create(parse(body)?)?),
        (Method::Post, "/api/window/show") => {
            if let Some(f) = engine.on_show.get() {
                f();
            }
            ok(json!({"shown": engine.on_show.get().is_some()}))
        }
        (Method::Post, "/api/shutdown") => {
            log("shutdown asked through the API");
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(200));
                std::process::exit(0);
            });
            ok(json!({"bye": true}))
        }
        _ if path.starts_with("/api/jobs/") => {
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            let j = engine.jobs.find(parts[2]).ok_or(HttpError(404, format!("job {} not found", parts[2])))?;
            let act = parts.get(3).copied().unwrap_or("");
            match (method, act) {
                (Method::Get, "") => ok(j.lock().unwrap().public()),
                (Method::Get, "results") => {
                    let since = qv("since").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let g = j.lock().unwrap();
                    ok(json!({"job": g.public(), "results": g.read_results(since)}))
                }
                (Method::Post, "stop") => {
                    engine.jobs.stop(&j);
                    let p = j.lock().unwrap().public();
                    ok(p)
                }
                (Method::Post, "resume") => {
                    engine.jobs.enqueue(&j);
                    let p = j.lock().unwrap().public();
                    ok(p)
                }
                (Method::Delete, "") => {
                    let id = parts[2].to_string();
                    engine.jobs.delete(&j);
                    ok(json!({"deleted": id}))
                }
                (Method::Get, "export") => {
                    let fmt = qv("format").unwrap_or_else(|| "csv".into());
                    let text = export(&j.lock().unwrap(), &fmt);
                    Ok((200, text.into_bytes(), if fmt == "csv" { "text/csv; charset=utf-8" } else { "application/json" }))
                }
                (Method::Post, "export") => {
                    let b = parse(body)?;
                    let p = b["path"].as_str().ok_or("path missing")?;
                    let fmt = b["format"].as_str().map(str::to_string).unwrap_or_else(|| {
                        if p.to_lowercase().ends_with(".json") { "json".into() } else { "csv".into() }
                    });
                    let text = export(&j.lock().unwrap(), &fmt);
                    std::fs::write(p, text).map_err(|e| HttpError(400, format!("cannot write {p}: {e}")))?;
                    ok(json!({"path": p, "format": fmt}))
                }
                _ => Err(HttpError(404, "not found".into())),
            }
        }
        _ => Err(HttpError(404, "not found".into())),
    }
}
