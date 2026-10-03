//! FERMI on llama.cpp: the prompt exactly as in training, the decision head, the answers, and the averaging of choice
//! questions over several orders of their options. A port of the `fermi` Python package (prompt.py, model.py) and
//! of the old engine's expand/merge, checked against PyTorch on the evaluation questions (app/tools/compare.py).

use std::path::Path;

use serde_json::{json, Map, Value};

use super::runtime::{LlamaModel, Runtime};

pub const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// tokens per example: a longer state is shortened (head + tail), as in training
pub const MAX_LEN: usize = 2048;
const SYSTEM: &str = "<|im_start|>system\nRead the state, then answer the question about it. Judge only from what the state shows.<|im_end|>\n<|im_start|>user\nState:\n";
const END: &str = "<|im_end|>\n<|im_start|>assistant\n";

pub type Questions = Map<String, Value>;

fn type_index(t: &str) -> Option<usize> {
    match t {
        "noul" => Some(0),
        "choice" => Some(1),
        "score" => Some(2),
        _ => None,
    }
}

/// A JSON value as Python's json.dumps(v, ensure_ascii=False, indent=1) writes it (the training format), or the text.
pub fn state_text(state: &Value) -> String {
    if let Value::String(s) = state {
        return s.clone();
    }
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    serde::Serialize::serialize(state, &mut ser).expect("JSON");
    String::from_utf8(out).expect("UTF-8")
}

fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn check_question(qid: &str, q: &Value) -> Result<(), String> {
    let t = q.get("type").and_then(Value::as_str).unwrap_or("");
    if type_index(t).is_none() {
        return Err(format!("{qid}: type must be noul, choice or score"));
    }
    if q.get("instructions").map(plain).unwrap_or_default().is_empty() {
        return Err(format!("{qid}: instructions missing"));
    }
    let n = LETTERS.len();
    if t == "choice" {
        let ok = q.get("criteria").and_then(Value::as_object).map(|c| (2..=n).contains(&c.len())).unwrap_or(false);
        if !ok {
            return Err(format!("{qid}: choice needs criteria as {{option: description}} with 2-{n} options"));
        }
    }
    if t == "score" {
        let ok = q.get("criteria").and_then(Value::as_array).map(|c| (2..=n).contains(&c.len())).unwrap_or(false);
        if !ok {
            return Err(format!("{qid}: score needs criteria as a list of 2-{n} levels, lowest first"));
        }
    }
    Ok(())
}

pub fn check_questions(qs: &Questions) -> Result<(), String> {
    if qs.is_empty() {
        return Err("no questions".into());
    }
    qs.iter().try_for_each(|(k, q)| check_question(k, q))
}

/// (header, option lines) of a question, as text.
fn question_parts(q: &Value) -> (String, Vec<String>) {
    let t = q["type"].as_str().unwrap_or("");
    let instr = plain(&q["instructions"]);
    let letter = |i: usize| LETTERS[i] as char;
    match t {
        "noul" => {
            let c = &q["criteria"];
            let opt = |k: &str| c.get(k).map(plain).filter(|s| !s.is_empty()).map(|s| format!(": {s}")).unwrap_or_default();
            (format!("\n\nQuestion (yes or no): {instr}\n"), vec![format!("A. yes{}\n", opt("true")), format!("B. no{}\n", opt("false"))])
        }
        "choice" => {
            let lines = q["criteria"].as_object().map(|c| {
                c.iter().enumerate().map(|(i, (k, d))| {
                    let d = plain(d);
                    format!("{}. {}{}\n", letter(i), k, if d.is_empty() { String::new() } else { format!(": {d}") })
                }).collect()
            }).unwrap_or_default();
            (format!("\n\nQuestion (pick one option): {instr}\n"), lines)
        }
        _ => {
            let lines = q["criteria"].as_array().map(|c| {
                c.iter().enumerate().map(|(i, lv)| format!("{}. {}\n", letter(i), plain(lv))).collect()
            }).unwrap_or_default();
            (format!("\n\nQuestion (pick one level of the scale): {instr}\n"), lines)
        }
    }
}

// ------------------------------------------------------------------------------------------------------------ head
/// score_i = v . gelu(A h_option_i + B h_end + type)
pub struct Head {
    dim: usize,
    n_embd: usize,
    a_w: Vec<f32>,
    a_b: Vec<f32>,
    b_w: Vec<f32>,
    tipo: Vec<f32>,
    v_w: Vec<f32>,
    v_b: f32,
}

fn matvec(w: &[f32], x: &[f32], rows: usize) -> Vec<f32> {
    let n = x.len();
    (0..rows).map(|r| {
        let row = &w[r * n..(r + 1) * n];
        // four partial sums: lets the compiler vectorize and keeps the rounding close to a BLAS dot product
        let mut s = [0f32; 4];
        let chunks = n / 4;
        for i in 0..chunks {
            for j in 0..4 {
                s[j] += row[i * 4 + j] * x[i * 4 + j];
            }
        }
        let mut t = s[0] + s[1] + s[2] + s[3];
        for i in chunks * 4..n {
            t += row[i] * x[i];
        }
        t
    }).collect()
}

fn gelu(x: f32) -> f32 {
    0.5 * x * (1.0 + libm::erff(x / std::f32::consts::SQRT_2))
}

impl Head {
    fn from_meta(m: &super::runtime::Meta, n_embd: usize) -> Result<Head, String> {
        let dim = m.int("fermi.head.dim").ok_or("not a FERMI model: fermi.head.dim missing")? as usize;
        let get = |k: &str, len: usize| -> Result<Vec<f32>, String> {
            let v = m.f32s(&format!("fermi.head.{k}")).ok_or(format!("fermi.head.{k} missing"))?;
            if v.len() != len {
                return Err(format!("fermi.head.{k}: {} values, expected {len}", v.len()));
            }
            Ok(v)
        };
        Ok(Head {
            dim,
            n_embd,
            a_w: get("a.weight", dim * n_embd)?,
            a_b: get("a.bias", dim)?,
            b_w: get("b.weight", dim * n_embd)?,
            tipo: get("tipo.weight", 3 * dim)?,
            v_w: get("v.weight", dim)?,
            v_b: get("v.bias", 1)?[0],
        })
    }

    /// Probabilities of the options: h holds the hidden state of every option line, then of the last token.
    fn probs(&self, h: &[f32], n_opt: usize, kind: usize) -> Vec<f64> {
        let d = self.n_embd;
        let end = &h[n_opt * d..(n_opt + 1) * d];
        let be = matvec(&self.b_w, end, self.dim);
        let ty = &self.tipo[kind * self.dim..(kind + 1) * self.dim];
        let scores: Vec<f64> = (0..n_opt).map(|i| {
            let a = matvec(&self.a_w, &h[i * d..(i + 1) * d], self.dim);
            let mut s = self.v_b;
            for j in 0..self.dim {
                s += self.v_w[j] * gelu(a[j] + self.a_b[j] + be[j] + ty[j]);
            }
            s as f64
        }).collect();
        let mx = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let e: Vec<f64> = scores.iter().map(|s| (s - mx).exp()).collect();
        let z: f64 = e.iter().sum();
        e.iter().map(|x| x / z).collect()
    }
}

// ----------------------------------------------------------------------------------------------------------- model
pub struct Fermi {
    pub lm: LlamaModel,
    head: Head,
    sys: Vec<i32>,
    end: Vec<i32>,
    ellipsis: Vec<i32>,
    pub config: Value,
}

/// Index of the largest value, the first one on ties (like Python's max).
fn argmax(p: &[f64]) -> usize {
    let mut best = 0;
    for (i, x) in p.iter().enumerate() {
        if *x > p[best] {
            best = i;
        }
    }
    best
}

pub fn round(x: f64, d: i32) -> f64 {
    let m = 10f64.powi(d);
    (x * m).round() / m
}

struct Item {
    state: usize,
    qid: String,
    ids: Vec<i32>,
    pos: Vec<i32>,
}

impl Fermi {
    /// The FERMI configuration of a GGUF file (fermi.config), or None if the file is not a FERMI model.
    pub fn read_config(rt: &Runtime, path: &Path) -> Option<Value> {
        let m = rt.meta(path)?;
        m.int("fermi.head.dim")?;
        let mut cfg: Value = m.str("fermi.config").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
        if let Some(n) = m.str("fermi.name") {
            cfg["name"] = json!(n);
        }
        if let Some(ft) = m.int("general.file_type") {
            cfg["file_type"] = json!(ft);
        }
        Some(cfg)
    }

    pub fn load(rt: &Runtime, path: &Path, device: i32, n_ctx: i32, n_seq: i32, threads: i32) -> Result<Fermi, String> {
        let config = Self::read_config(rt, path).ok_or("not a FERMI model (no fermi.* metadata in the file)")?;
        let ubatch = if device >= 0 { 2048 } else { 512 };    // big passes on a GPU: each pass has a fixed cost
        let lm = rt.load_model(path, device, n_ctx, n_seq, ubatch, threads)?;
        let meta = rt.meta(path).ok_or("cannot read the model's metadata")?;
        let head = Head::from_meta(&meta, lm.n_embd)?;
        let (start, end) = match config.get("prompt_format").and_then(Value::as_array) {
            Some(f) if f.len() == 2 => (plain(&f[0]), plain(&f[1])),
            _ => (SYSTEM.to_string(), END.to_string()),
        };
        let sys = lm.tokenize(&start);
        let end = lm.tokenize(&end);
        let mut ellipsis = lm.tokenize(" […] ");
        ellipsis.truncate(3);
        Ok(Fermi { lm, head, sys, end, ellipsis, config })
    }

    /// Token ids and option positions for (state, question), exactly as prompt.Builder.build.
    fn build(&self, state_ids: &[i32], q: &Value) -> Result<(Vec<i32>, Vec<i32>), String> {
        let (head, lines) = question_parts(q);
        let qh = self.lm.tokenize(&head);
        let opts: Vec<Vec<i32>> = lines.iter().map(|l| self.lm.tokenize(l)).collect();
        let rest = self.sys.len() + qh.len() + opts.iter().map(Vec::len).sum::<usize>() + self.end.len();
        if rest + 64 > MAX_LEN {
            return Err(format!("question too long ({rest} tokens without the state)"));
        }
        let room = MAX_LEN - rest;
        let mut ids = self.sys.clone();
        if state_ids.len() > room {
            let a = (room as f64 * 0.7) as usize - 3;
            let b = room - a - 3;
            ids.extend_from_slice(&state_ids[..a]);
            ids.extend_from_slice(&self.ellipsis);
            ids.extend_from_slice(&state_ids[state_ids.len() - b..]);
        } else {
            ids.extend_from_slice(state_ids);
        }
        ids.extend_from_slice(&qh);
        let mut pos = Vec::with_capacity(opts.len());
        for o in &opts {
            ids.extend_from_slice(o);
            pos.push(ids.len() as i32 - 1);
        }
        ids.extend_from_slice(&self.end);
        Ok((ids, pos))
    }

    /// [(state, questions)] -> [answers]: every question of every state in one queue, many sequences per pass.
    pub fn classify_many(&self, pairs: &[(&Value, &Questions)]) -> Result<Vec<Map<String, Value>>, String> {
        let mut items = Vec::new();
        for (si, (state, qs)) in pairs.iter().enumerate() {
            check_questions(qs)?;
            let st = self.lm.tokenize(&state_text(state));
            for (qid, q) in qs.iter() {
                let (ids, pos) = self.build(&st, q)?;
                items.push(Item { state: si, qid: qid.clone(), ids, pos });
            }
        }
        items.sort_by_key(|it| it.ids.len());
        let mut out: Vec<Map<String, Value>> = vec![Map::new(); pairs.len()];
        let mut i = 0;
        while i < items.len() {
            // a group: as many sequences as the context holds
            let mut j = i;
            let mut tokens = 0;
            while j < items.len() && j - i < self.lm.n_seq && tokens + items[j].ids.len() <= self.lm.n_ctx {
                tokens += items[j].ids.len();
                j += 1;
            }
            if j == i {
                return Err("state too long for the model's context".into());
            }
            let group = &items[i..j];
            let wanted: Vec<Vec<i32>> = group.iter().map(|it| {
                let mut p = it.pos.clone();
                p.push(it.ids.len() as i32 - 1);
                p
            }).collect();
            let seqs: Vec<(&[i32], &[i32])> = group.iter().zip(&wanted).map(|(it, w)| (it.ids.as_slice(), w.as_slice())).collect();
            let h = self.lm.hidden(&seqs)?;
            let d = self.lm.n_embd;
            let mut off = 0;
            for (it, w) in group.iter().zip(&wanted) {
                let q = &pairs[it.state].1[&it.qid];
                let kind = type_index(q["type"].as_str().unwrap_or("")).unwrap_or(0);
                let p = self.head.probs(&h[off * d..(off + w.len()) * d], it.pos.len(), kind);
                off += w.len();
                out[it.state].insert(it.qid.clone(), answer(q, &p));
            }
            i = j;
        }
        // in the order of the questions
        Ok(out.into_iter().zip(pairs).map(|(mut a, (_, qs))| {
            qs.keys().filter_map(|k| a.remove(k).map(|v| (k.clone(), v))).collect()
        }).collect())
    }

    pub fn answer(&self, state: &Value, questions: &Questions, orders: &Value) -> Result<Map<String, Value>, String> {
        let (exp, plan) = expand(questions, orders);
        let res = self.classify_many(&[(state, &exp)])?;
        Ok(merge(questions, &plan, &res[0]))
    }
}

fn answer(q: &Value, p: &[f64]) -> Value {
    let t = q["type"].as_str().unwrap_or("");
    let best = argmax(p);
    match t {
        "noul" => json!({"type": t, "answer": if p[0] >= 0.5 { "yes" } else { "no" }, "p_yes": round(p[0], 4)}),
        "choice" => {
            let keys: Vec<&String> = q["criteria"].as_object().map(|c| c.keys().collect()).unwrap_or_default();
            let probs: Map<String, Value> = keys.iter().zip(p).map(|(k, x)| ((*k).clone(), json!(round(*x, 4)))).collect();
            json!({"type": t, "answer": keys.get(best).map(|k| k.as_str()).unwrap_or(""), "probabilities": probs})
        }
        _ => {
            let exp: f64 = p.iter().enumerate().map(|(i, x)| i as f64 * x).sum();
            let n = (p.len() as f64 - 1.0).max(1.0);
            json!({"type": t, "answer": best, "answer_text": q["criteria"].get(best).cloned().unwrap_or(Value::Null),
                   "probabilities": p.iter().map(|x| round(*x, 4)).collect::<Vec<_>>(),
                   "expected": round(exp, 3), "expected_01": round(exp / n, 4)})
        }
    }
}

// ------------------------------------------------------------------------------------- averaging over option orders
fn no_orders(mode: &Value) -> bool {
    match mode {
        Value::Null | Value::Bool(false) => true,
        Value::Number(n) => n.as_i64() == Some(1),
        Value::String(s) => s == "1" || s == "none" || s.is_empty(),
        _ => false,
    }
}

fn permutations(keys: &[String]) -> Vec<Vec<String>> {
    if keys.len() <= 1 {
        return vec![keys.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..keys.len() {
        let mut rest = keys.to_vec();
        let k = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, k.clone());
            out.push(p);
        }
    }
    out
}

fn orders_of(keys: &[String], mode: &Value) -> Vec<Vec<String>> {
    if no_orders(mode) {
        return vec![keys.to_vec()];
    }
    if mode.as_str() == Some("all") && keys.len() <= 4 {
        return permutations(keys);
    }
    let mut r = keys.to_vec();
    r.reverse();
    vec![keys.to_vec(), r]                 // "reverse", or "all" with many options
}

/// Choice questions asked with their options in several orders (cancels position bias): the expanded questions and
/// the plan to merge the answers back (qid -> sub-question ids, None for questions asked once).
pub fn expand(questions: &Questions, mode: &Value) -> (Questions, Vec<(String, Option<Vec<String>>)>) {
    let mut out = Map::new();
    let mut plan = Vec::new();
    for (qid, q) in questions {
        let crit = q.get("criteria").and_then(Value::as_object);
        if q.get("type").and_then(Value::as_str) == Some("choice") && crit.is_some() && !no_orders(mode) {
            let crit = crit.unwrap();
            let keys: Vec<String> = crit.keys().cloned().collect();
            let mut subs = Vec::new();
            for (i, o) in orders_of(&keys, mode).into_iter().enumerate() {
                let sid = format!("{qid}\u{0}{i}");
                let mut sq = q.clone();
                sq["criteria"] = Value::Object(o.iter().map(|k| (k.clone(), crit[k].clone())).collect());
                out.insert(sid.clone(), sq);
                subs.push(sid);
            }
            plan.push((qid.clone(), Some(subs)));
        } else {
            out.insert(qid.clone(), q.clone());
            plan.push((qid.clone(), None));
        }
    }
    (out, plan)
}

pub fn merge(questions: &Questions, plan: &[(String, Option<Vec<String>>)], answers: &Map<String, Value>) -> Map<String, Value> {
    let mut res = Map::new();
    for (qid, subs) in plan {
        match subs {
            None => {
                res.insert(qid.clone(), answers.get(qid).cloned().unwrap_or(Value::Null));
            }
            Some(subs) => {
                let keys: Vec<String> = questions[qid]["criteria"].as_object().map(|c| c.keys().cloned().collect()).unwrap_or_default();
                let pr: Vec<f64> = keys.iter().map(|k| {
                    subs.iter().map(|s| answers[s]["probabilities"][k].as_f64().unwrap_or(0.0)).sum::<f64>() / subs.len() as f64
                }).collect();
                let best = argmax(&pr);
                let probs: Map<String, Value> = keys.iter().zip(&pr).map(|(k, p)| (k.clone(), json!(round(*p, 4)))).collect();
                res.insert(qid.clone(), json!({"type": "choice", "answer": keys[best], "probabilities": probs, "orders": subs.len()}));
            }
        }
    }
    res
}
