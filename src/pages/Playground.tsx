import { useEffect, useMemo, useState } from "react";
import { api, cleanQuestions, parseState, type Answers, type Questions } from "../api";
import { Ico } from "../components/icons";
import { QuestionEditor } from "../components/QuestionEditor";
import { AnswerCard, CodeBox, Empty, Seg, useApp } from "../components/ui";
import { PRESETS } from "../presets";

interface Run { model: string; answers: Answers; ms: number }
const load = <T,>(k: string, d: T): T => { try { const v = localStorage.getItem(k); return v ? JSON.parse(v) : d; } catch { return d; } };

export default function Playground() {
  const { status, toast, go } = useApp();
  const [stateText, setStateText] = useState<string>(() => load("fermi.pg.state", PRESETS[0].state));
  const [questions, setQuestions] = useState<Questions>(() => load("fermi.pg.questions", PRESETS[0].questions));
  const [target0, setTarget] = useState<string>(() => load("fermi.pg.target", "default"));
  const [orders, setOrders] = useState<string>(() => load("fermi.pg.orders", "none"));
  const [runs, setRuns] = useState<Run[]>([]);
  const [asked, setAsked] = useState<Questions>({});
  const [busy, setBusy] = useState(false);
  const [code, setCode] = useState(false);

  useEffect(() => { localStorage.setItem("fermi.pg.state", JSON.stringify(stateText)); }, [stateText]);
  useEffect(() => { localStorage.setItem("fermi.pg.questions", JSON.stringify(questions)); }, [questions]);
  useEffect(() => { localStorage.setItem("fermi.pg.target", JSON.stringify(target0)); }, [target0]);
  useEffect(() => { localStorage.setItem("fermi.pg.orders", JSON.stringify(orders)); }, [orders]);

  const models = status?.models ?? [];
  const ready = models.filter(m => m.status === "ready");
  // a remembered choice that no longer applies (the model was removed, only one is loaded) falls back to the default
  const target = target0 === "compare" ? (ready.length >= 2 ? target0 : "default")
    : target0 === "default" || models.some(m => m.id === target0) ? target0 : "default";
  const parsed = useMemo(() => parseState(stateText), [stateText]);
  const targets = target === "compare" ? ready.map(m => m.id) : [target === "default" ? (status?.default ?? "") : target];

  const run = async () => {
    const qs = cleanQuestions(questions);
    if (!Object.keys(qs).length) return toast("Aggiungi almeno una domanda", true);
    if (!stateText.trim()) return toast("Scrivi uno stato", true);
    if (target === "compare" && ready.length < 2) return toast("Per confrontare servono almeno due modelli caricati", true);
    setBusy(true);
    try {
      const out: Run[] = [];
      for (const id of targets) {                         // one model after the other: fair timings
        const r = await api.classify({ model: id, state: parsed.state, questions: qs, orders: orders === "none" ? undefined : orders });
        out.push({ model: r.model, answers: r.answers, ms: r.ms });
      }
      setRuns(out);
      setAsked(qs);
    } catch (e) { toast((e as Error).message, true); }
    setBusy(false);
  };

  useEffect(() => {
    const k = (e: KeyboardEvent) => { if ((e.ctrlKey || e.metaKey) && e.key === "Enter") { e.preventDefault(); run(); } };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  const usePreset = (i: number) => { setStateText(PRESETS[i].state); setQuestions(PRESETS[i].questions); setRuns([]); };
  const req = { ...(target === "compare" || target === "default" ? {} : { model: target }), state: parsed.state, questions: cleanQuestions(questions) };
  const port = status?.server.port ?? 8765;

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Playground</h1>
          <p>Scrivi uno stato, fai le tue domande e guarda le probabilità. <span className="kbd">Ctrl</span> + <span className="kbd">Invio</span> per chiedere.</p>
        </div>
        <div className="actions">
          <select value={target} onChange={e => setTarget(e.target.value)} style={{ width: "auto" }}>
            <option value="default">Modello predefinito</option>
            {models.map(m => <option key={m.id} value={m.id}>{m.name}{m.status !== "ready" ? " (si carica alla prima domanda)" : ""}</option>)}
            <option value="compare" disabled={ready.length < 2}>Confronta i modelli caricati</option>
          </select>
          <span title="Le domande a scelta vengono poste con le opzioni in più ordini e mediate: toglie quasi tutto il bias di posizione">
            <Seg value={orders} onChange={setOrders} options={[["none", "1 ordine"], ["reverse", "2 ordini"], ["all", "tutti"]]} />
          </span>
          <button className="primary" onClick={run} disabled={busy}>{busy ? <span className="spin" /> : Ico.play} Chiedi</button>
        </div>
      </div>

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", alignItems: "start" }}>
        <div className="col" style={{ gap: 14 }}>
          <div className="card">
            <div className="card-title">Stato
              <span className={`pill ${parsed.json ? "run" : ""}`}>{parsed.json ? "JSON" : "testo"}</span>
              <span className="actions">
                <select value="" onChange={e => e.target.value && usePreset(+e.target.value)} style={{ width: "auto", padding: "3px 8px", fontSize: 12.5 }}>
                  <option value="">Esempi…</option>
                  {PRESETS.map((p, i) => <option key={p.name} value={i}>{p.name} · {p.hint}</option>)}
                </select>
              </span>
            </div>
            <textarea rows={9} value={stateText} onChange={e => setStateText(e.target.value)} spellCheck={false}
                      placeholder="Un'email, un messaggio, un log, un JSON…" />
          </div>
          <div className="card">
            <div className="card-title">Domande <span className="dim2" style={{ textTransform: "none", letterSpacing: 0 }}>{Object.keys(questions).length}</span>
              <span className="actions"><button className="sm ghost" onClick={() => setCode(!code)}>{"{ }"} codice</button></span>
            </div>
            <QuestionEditor value={questions} onChange={setQuestions} />
          </div>
          {code && (
            <div className="card">
              <div className="card-title">La stessa richiesta dal tuo codice</div>
              <CodeBox code={`curl http://127.0.0.1:${port}/api/classify \\\n  -H "Content-Type: application/json" \\\n  -d '${JSON.stringify(req).replace(/'/g, "'\\''")}'`} />
              <div className="dim small" style={{ marginTop: 8 }}>Serve il server API acceso: <a href="#" onClick={e => { e.preventDefault(); go("server"); }}>Server API</a>.</div>
            </div>
          )}
        </div>

        <div className="col sticky" style={{ gap: 12 }}>
          {runs.length > 0 && (
            <div className="row">
              {runs.map((r, i) => <span key={r.model} className={`model-tag m${i}`} style={{ fontSize: 12, padding: "3px 9px" }}>{r.model} · {r.ms} ms</span>)}
              <span className="dim small">per {Object.keys(asked).length} domande{orders !== "none" ? `, opzioni in ${orders === "all" ? "tutti gli ordini" : "2 ordini"}` : ""}</span>
            </div>
          )}
          {runs.length ? Object.entries(asked).map(([qid, q]) => (
            <div key={qid} className="col" style={{ gap: 6 }}>
              {runs.map((r, i) => r.answers[qid] && (
                <AnswerCard key={r.model} qid={qid} q={q} a={r.answers[qid]} variant={i} tag={runs.length > 1 ? r.model : undefined} />
              ))}
            </div>
          )) : (
            <div className="card"><Empty icon={Ico.play}>Le risposte compaiono qui. Prova un esempio dal menu <b>Esempi</b>.</Empty></div>
          )}
        </div>
      </div>
    </div>
  );
}
