import { useEffect, useMemo, useRef, useState } from "react";
import { api, cleanQuestions, fmtDur, pct, type Job, type JobSpec, type Questions, type QuestionSet, type ResultRow, type ScanResult } from "../api";
import { GroupMatrix, labelOf, QuestionChart, valueOf } from "../components/charts";
import { Ico } from "../components/icons";
import { QuestionEditor } from "../components/QuestionEditor";
import { AnswerCard, Empty, Seg, useApp } from "../components/ui";
import { pickFolder, pickSave } from "../native";
import { QUESTION_SETS } from "../presets";

const STATUS: Record<Job["status"], [string, string]> = {
  queued: ["in coda", "run"], running: ["in corso", "run"], paused: ["in pausa", "warn"], done: ["completata", "ok"],
  error: ["errore", "bad"], interrupted: ["interrotta", "warn"],
};

export default function Analysis() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [sel, setSel] = useState<string>(() => localStorage.getItem("fermi.job") || "new");
  useEffect(() => {
    const f = () => api.jobs().then(r => setJobs(r.jobs)).catch(() => {});
    f();
    const t = setInterval(f, 2000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => { localStorage.setItem("fermi.job", sel); }, [sel]);
  const current = jobs.find(j => j.id === sel);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Analisi cartelle</h1>
          <p>Scegli una cartella piena di stati (file di testo, JSON, JSONL o CSV), fai poche domande e FERMI le pone a ogni stato.
            I risultati si salvano riga per riga: se chiudi l'app o salta la corrente, l'analisi riparte da dove era.</p>
        </div>
      </div>
      <div className="split">
        <div className="col sticky">
          <button className={sel === "new" ? "primary" : ""} onClick={() => setSel("new")}>{Ico.plus} Nuova analisi</button>
          <div className="joblist">
            {jobs.map(j => (
              <button key={j.id} className={`jobitem ${sel === j.id ? "on" : ""}`} onClick={() => setSel(j.id)}>
                <div className="t"><span>{j.name}</span><span className={`pill ${STATUS[j.status][1]}`}>{STATUS[j.status][0]}</span></div>
                {j.total ? <div className="progress"><i style={{ width: `${(100 * j.done) / j.total}%` }} /></div> : null}
                <div className="dim small">{j.done}{j.total ? ` / ${j.total}` : ""} stati · {j.model_name} · {new Date(j.created * 1000).toLocaleString("it-IT", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" })}</div>
              </button>
            ))}
          </div>
        </div>
        <div>
          {sel === "new" || !current ? <NewJob onCreated={id => setSel(id)} /> : <JobView key={current.id} job={current} onDeleted={() => setSel("new")} />}
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------------------------------------------ new job
const EXTS = [".txt", ".md", ".eml", ".log", ".json", ".jsonl", ".csv", ".tsv", ".html"];

function NewJob({ onCreated }: { onCreated: (id: string) => void }) {
  const { status, toast } = useApp();
  const [folder, setFolder] = useState(() => localStorage.getItem("fermi.an.folder") || "");
  const [recursive, setRecursive] = useState(true);
  const [exts, setExts] = useState<string[]>([]);
  const [limit, setLimit] = useState("");
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [hidden, setHidden] = useState<string[]>([]);
  const [questions, setQuestions] = useState<Questions>(() => {
    try { return JSON.parse(localStorage.getItem("fermi.an.questions") || "") as Questions; } catch { return QUESTION_SETS[0].questions; }
  });
  const [saved, setSaved] = useState<QuestionSet[]>([]);
  const [model, setModel] = useState("");
  const [orders, setOrders] = useState<"none" | "reverse">("none");
  const [name, setName] = useState("");
  const [sample, setSample] = useState(0);

  useEffect(() => { api.settings().then(s => setSaved(s.question_sets || [])).catch(() => {}); }, []);
  useEffect(() => { localStorage.setItem("fermi.an.questions", JSON.stringify(questions)); }, [questions]);
  useEffect(() => { localStorage.setItem("fermi.an.folder", folder); setScan(null); }, [folder]);

  const source = () => ({ folder, recursive, extensions: exts.length ? exts : undefined, limit: limit ? +limit : null });
  const doScan = async () => {
    if (!folder) return toast("Scegli una cartella", true);
    setScanning(true);
    try {
      const s = await api.scan(source());
      setScan(s); setSample(0);
      setHidden(s.fields.filter(f => f.label_like && f.name.length < 30).map(f => f.name).filter(n => /label|queue|priority|type|categor|class|rating|stars|target|language|lingua|etichett/i.test(n)));
    } catch (e) { toast((e as Error).message, true); }
    setScanning(false);
  };
  const browse = async () => { const p = await pickFolder("Cartella con gli stati", folder); if (p) setFolder(p); };
  const start = async () => {
    const qs = cleanQuestions(questions);
    if (!Object.keys(qs).length) return toast("Aggiungi almeno una domanda", true);
    try {
      const spec: JobSpec = { name: name || undefined, model: model || undefined, orders, source: source(), hidden, questions: qs };
      const j = await api.createJob(spec);
      toast("Analisi avviata");
      onCreated(j.id);
    } catch (e) { toast((e as Error).message, true); }
  };
  const saveSet = async () => {
    const n = prompt("Nome del set di domande");
    if (!n) return;
    const next = [...saved.filter(s => s.name !== n), { name: n, questions: cleanQuestions(questions) }];
    await api.saveSettings({ question_sets: next });
    setSaved(next);
    toast("Set salvato");
  };
  const sets = [...QUESTION_SETS, ...saved];

  return (
    <div className="col" style={{ gap: 14 }}>
      <div className="card">
        <div className="card-title"><span className="badge">1</span> Cartella</div>
        <div className="row" style={{ flexWrap: "nowrap" }}>
          <input value={folder} onChange={e => setFolder(e.target.value)} placeholder="/percorso/della/cartella" className="mono" />
          <button onClick={browse}>{Ico.folder} Sfoglia</button>
          <button className="primary" onClick={doScan} disabled={scanning}>{scanning ? <span className="spin" /> : Ico.search} Esamina</button>
        </div>
        <div className="row" style={{ marginTop: 10, gap: 14 }}>
          <label className="check"><input type="checkbox" checked={recursive} onChange={e => setRecursive(e.target.checked)} /> anche le sottocartelle</label>
          <span className="fieldchips">
            {EXTS.map(e => <span key={e} className={`chip ${exts.includes(e) ? "on" : ""}`} onClick={() => setExts(exts.includes(e) ? exts.filter(x => x !== e) : [...exts, e])}>{e}</span>)}
          </span>
          <span className="dim small">{exts.length ? "" : "tutti i tipi"}</span>
          <label className="check">al massimo <input type="number" value={limit} onChange={e => setLimit(e.target.value)} style={{ width: 90 }} placeholder="tutti" /> stati</label>
        </div>
        {scan && (
          <div className="col" style={{ marginTop: 14 }}>
            <div className="grid g4">
              <div className="stat"><b>{scan.files}</b><span>file</span></div>
              <div className="stat"><b>{scan.states}{scan.complete ? "" : "+"}</b><span>stati da analizzare</span></div>
              <div className="stat" style={{ gridColumn: "span 2" }}><span>per tipo</span>
                <div className="row" style={{ marginTop: 4 }}>{Object.entries(scan.by_ext).map(([e, n]) => <span key={e} className="pill">{e} {n}</span>)}</div></div>
            </div>
            {scan.fields.length > 0 && (
              <div>
                <div className="dim small" style={{ marginBottom: 6 }}>Campi trovati. Quelli selezionati <b>restano nascosti al modello</b> ma finiscono nei risultati:
                  servono per confrontare FERMI con un'etichetta che hai già (per esempio una priorità assegnata da una persona).</div>
                <div className="fieldchips">
                  {scan.fields.map(f => (
                    <span key={f.name} className={`chip ${hidden.includes(f.name) ? "on" : ""}`} title={f.examples.join(" · ")}
                          onClick={() => setHidden(hidden.includes(f.name) ? hidden.filter(x => x !== f.name) : [...hidden, f.name])}>
                      {hidden.includes(f.name) ? "🙈" : ""} {f.name} {f.distinct != null && <small>{f.distinct} valori</small>}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {scan.sample[sample] && (
              <div>
                <div className="row dim small" style={{ marginBottom: 6 }}>Esempio <span className="mono">{scan.sample[sample].id}</span>
                  <span style={{ flex: 1 }} />
                  {scan.sample.length > 1 && <button className="sm ghost" onClick={() => setSample((sample + 1) % scan.sample.length)}>altro →</button>}
                </div>
                <div className="statebox" style={{ maxHeight: 180 }}>{scan.sample[sample].text}</div>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="card">
        <div className="card-title"><span className="badge">2</span> Domande
          <span className="actions">
            <select value="" onChange={e => { const s = sets.find(x => x.name === e.target.value); if (s) setQuestions(s.questions); }} style={{ width: "auto", padding: "3px 8px", fontSize: 12.5 }}>
              <option value="">Set di domande…</option>
              {sets.map(s => <option key={s.name} value={s.name}>{s.name}</option>)}
            </select>
            <button className="sm" onClick={saveSet}>Salva set</button>
          </span>
        </div>
        <QuestionEditor value={questions} onChange={setQuestions} />
      </div>

      <div className="card">
        <div className="card-title"><span className="badge">3</span> Avvio</div>
        <div className="row" style={{ gap: 14 }}>
          <label className="field" style={{ width: 220 }}>Modello
            <select value={model} onChange={e => setModel(e.target.value)}>
              <option value="">Predefinito</option>
              {(status?.models ?? []).map(m => <option key={m.id} value={m.id}>{m.name}</option>)}
            </select>
          </label>
          <label className="field">Domande a scelta
            <Seg value={orders} onChange={setOrders} options={[["none", "1 ordine (veloce)"], ["reverse", "2 ordini (più preciso)"]]} />
          </label>
          <label className="field" style={{ flex: 1, minWidth: 200 }}>Nome
            <input value={name} onChange={e => setName(e.target.value)} placeholder={folder.split(/[\\/]/).filter(Boolean).pop() || "analisi"} />
          </label>
          <button className="primary" style={{ alignSelf: "flex-end", padding: "9px 20px" }} onClick={start}>{Ico.play} Avvia l'analisi</button>
        </div>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------------------------------------------- job view
function JobView({ job, onDeleted }: { job: Job; onDeleted: () => void }) {
  const { toast } = useApp();
  const [rows, setRows] = useState<ResultRow[]>([]);
  const [filters, setFilters] = useState<Record<string, string>>({});
  const [groupBy, setGroupBy] = useState("");
  const [sort, setSort] = useState<{ k: string; dir: number }>({ k: "", dir: 1 });
  const [open, setOpen] = useState<ResultRow | null>(null);
  const [shown, setShown] = useState(200);
  const rowsRef = useRef<ResultRow[]>([]);
  const qs = job.spec.questions;
  const live = job.status === "running" || job.status === "queued";

  useEffect(() => {
    let stop = false;
    const pull = async () => {
      try {
        const r = await api.results(job.id, rowsRef.current.length);
        if (stop) return;
        if (r.results.length) {
          rowsRef.current = [...rowsRef.current, ...r.results];
          setRows(rowsRef.current);
        }
      } catch { /* engine restarting */ }
    };
    pull();
    const t = setInterval(pull, live ? 800 : 4000);
    return () => { stop = true; clearInterval(t); };
  }, [job.id, live]);

  const metaKeys = useMemo(() => {
    const s = new Set<string>();
    rows.slice(0, 200).forEach(r => Object.keys(r.meta || {}).forEach(k => s.add(k)));
    return [...s];
  }, [rows]);
  useEffect(() => { if (!groupBy && metaKeys.length) setGroupBy(metaKeys[0]); }, [metaKeys, groupBy]);

  const pass = (r: ResultRow, except?: string) =>
    Object.entries(filters).every(([qid, l]) => qid === except || labelOf(qs[qid], r, qid) === l);
  const filtered = rows.filter(r => !r.error && pass(r));
  const sorted = useMemo(() => {
    if (!sort.k) return filtered;
    const key = (r: ResultRow) => (sort.k === "id" ? r.sid : sort.k.startsWith("meta:") ? String(r.meta?.[sort.k.slice(5)] ?? "") : valueOf(r, sort.k) ?? -1);
    return [...filtered].sort((a, b) => { const x = key(a), y = key(b); return (x < y ? -1 : x > y ? 1 : 0) * sort.dir; });
  }, [filtered, sort]);

  const exportAs = async (fmt: string) => {
    const p = await pickSave("Esporta i risultati", `${job.name}.${fmt}`, fmt);
    if (!p) return;
    try { const r = await api.exportJob(job.id, p, fmt); toast("Salvato in " + r.path); } catch (e) { toast((e as Error).message, true); }
  };
  const toggleSort = (k: string) => setSort(s => ({ k, dir: s.k === k ? -s.dir : -1 }));
  const errors = rows.filter(r => r.error);
  const avgMs = rows.length ? rows.reduce((a, r) => a + (r.ms || 0), 0) / rows.length : 0;
  const elapsed = job.status === "running" && job.started ? (job.elapsed || 0) : job.elapsed;

  return (
    <div className="col" style={{ gap: 14 }}>
      <div className="card">
        <div className="row" style={{ alignItems: "flex-start" }}>
          <div>
            <div className="row"><h2>{job.name}</h2><span className={`pill ${STATUS[job.status][1]}`}><span className="dot" />{STATUS[job.status][0]}</span></div>
            <div className="dim small mono">{job.spec.source.folder}</div>
          </div>
          <div className="row" style={{ marginLeft: "auto" }}>
            {live ? <button onClick={() => api.stopJob(job.id)}>{Ico.pause} Pausa</button>
              : job.status !== "done" && <button className="primary" onClick={() => api.resumeJob(job.id)}>{Ico.play} Riprendi</button>}
            <button onClick={() => exportAs("csv")} disabled={!rows.length}>{Ico.download} CSV</button>
            <button onClick={() => exportAs("json")} disabled={!rows.length}>{Ico.download} JSON</button>
            <button className="ghost" onClick={async () => { if (confirm("Eliminare questa analisi e i suoi risultati?")) { await api.deleteJob(job.id); onDeleted(); } }}>{Ico.trash}</button>
          </div>
        </div>
        <div className="grid g4" style={{ marginTop: 14 }}>
          <div className="stat"><b>{job.done}<span className="dim" style={{ fontSize: 15 }}> / {job.total ?? "…"}</span></b><span>stati analizzati</span></div>
          <div className="stat"><b>{job.rate ? job.rate.toFixed(1) : rows.length && elapsed ? (job.done / elapsed).toFixed(1) : "–"}</b><span>stati al secondo · {job.model_name}</span></div>
          <div className="stat"><b>{live ? fmtDur(job.eta) : fmtDur(elapsed)}</b><span>{live ? "alla fine" : "tempo totale"}</span></div>
          <div className="stat"><b>{avgMs ? Math.round(avgMs) + " ms" : "–"}</b><span>per stato, {Object.keys(qs).length} domande</span></div>
        </div>
        {job.total ? <div className="progress" style={{ marginTop: 12 }}><i style={{ width: `${(100 * job.done) / job.total}%` }} /></div> : null}
        {job.error && <div className="notice bad" style={{ marginTop: 10 }}>{job.error}</div>}
        {errors.length > 0 && <div className="notice warn small" style={{ marginTop: 10 }}>{errors.length} stati non letti (per esempio {errors[0].sid}: {errors[0].error})</div>}
      </div>

      {Object.keys(filters).length > 0 && (
        <div className="row">
          <span className="dim small">Filtri:</span>
          {Object.entries(filters).map(([qid, l]) => (
            <span key={qid} className="chip on" onClick={() => setFilters(f => { const n = { ...f }; delete n[qid]; return n; })}>{qid} = {l} ✕</span>
          ))}
          <span className="dim small">{filtered.length} stati</span>
        </div>
      )}

      <div className="grid g2">
        {Object.entries(qs).map(([qid, q]) => (
          <QuestionChart key={qid} qid={qid} q={q} rows={rows.filter(r => !r.error && pass(r, qid))} filter={filters[qid] ?? null}
                         onFilter={l => setFilters(f => { const n = { ...f }; if (l == null) delete n[qid]; else n[qid] = l; return n; })} />
        ))}
      </div>

      {metaKeys.length > 0 && (
        <>
          <div className="row" style={{ marginTop: 6 }}>
            <h3>Confronto con i campi nascosti</h3>
            <select value={groupBy} onChange={e => setGroupBy(e.target.value)} style={{ width: "auto" }}>
              {metaKeys.map(k => <option key={k} value={k}>{k}</option>)}
            </select>
            <span className="dim small">Ogni riga è un valore del campo: come ha risposto FERMI senza vederlo?</span>
          </div>
          <div className="grid g2">
            {Object.entries(qs).map(([qid, q]) => <GroupMatrix key={qid} qid={qid} q={q} rows={filtered} field={groupBy} />)}
          </div>
        </>
      )}

      <div className="card" style={{ padding: 0 }}>
        <div className="card-title" style={{ padding: "14px 16px 0" }}>Stati <span className="dim2" style={{ textTransform: "none", letterSpacing: 0 }}>{sorted.length}</span>
          <span className="actions dim small" style={{ textTransform: "none" }}>clic su un'intestazione per ordinare, su una riga per aprirla</span></div>
        <div className="scroll" style={{ maxHeight: 560 }}>
          <table className="t">
            <thead><tr>
              <th className="sort" onClick={() => toggleSort("id")}>stato</th>
              {Object.keys(qs).map(qid => <th key={qid} className="sort" onClick={() => toggleSort(qid)}>{qid} {sort.k === qid ? (sort.dir < 0 ? "↓" : "↑") : ""}</th>)}
              {metaKeys.map(k => <th key={k} className="sort" onClick={() => toggleSort("meta:" + k)}>🙈 {k}</th>)}
            </tr></thead>
            <tbody>
              {sorted.slice(0, shown).map(r => (
                <tr key={r.sid} className={`click ${open?.sid === r.sid ? "sel" : ""}`} onClick={() => setOpen(r)}>
                  <td style={{ maxWidth: 320 }}><div className="truncate"><span className="mono dim2">{r.sid}</span> {r.text.replace(/\s+/g, " ").slice(0, 120)}</div></td>
                  {Object.entries(qs).map(([qid, q]) => {
                    const l = labelOf(q, r, qid), v = valueOf(r, qid), a = r.answers?.[qid];
                    const w = a?.type === "score" ? a.expected_01 : a?.type === "noul" ? a.p_yes : v ?? 0;
                    return <td key={qid}><span className="cell-ans"><i style={{ ["--w" as string]: `${100 * w}%` }} />{l} <span className="dim2 small">{a?.type === "score" ? a.expected.toFixed(2) : v != null ? pct(a?.type === "noul" && a.p_yes < 0.5 ? 1 - v : v, 0) : ""}</span></span></td>;
                  })}
                  {metaKeys.map(k => <td key={k} className="dim">{String(r.meta?.[k] ?? "")}</td>)}
                </tr>
              ))}
            </tbody>
          </table>
          {!rows.length && <Empty>{live ? "In attesa dei primi risultati…" : "Nessun risultato."}</Empty>}
          {sorted.length > shown && <div style={{ padding: 12, textAlign: "center" }}><button onClick={() => setShown(shown + 500)}>Mostra altri ({sorted.length - shown})</button></div>}
        </div>
      </div>

      {open && (
        <div className="drawer">
          <div className="drawer-head"><b className="mono">{open.sid}</b><span style={{ flex: 1 }} /><button className="ghost sm" onClick={() => setOpen(null)}>{Ico.x}</button></div>
          <div className="drawer-body">
            {Object.keys(open.meta || {}).length > 0 && (
              <div className="row">{Object.entries(open.meta).map(([k, v]) => <span key={k} className="pill">🙈 {k}: {String(v)}</span>)}</div>
            )}
            <div className="statebox">{open.text}</div>
            {Object.entries(qs).map(([qid, q]) => open.answers?.[qid] && <AnswerCard key={qid} qid={qid} q={q} a={open.answers[qid]} />)}
            {open.error && <div className="notice bad">{open.error}</div>}
          </div>
        </div>
      )}
    </div>
  );
}
