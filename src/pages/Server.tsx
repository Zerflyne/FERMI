import { useEffect, useRef, useState } from "react";
import { api, type RequestLog } from "../api";
import { Ico } from "../components/icons";
import { CodeBox, Seg, Switch, useApp } from "../components/ui";
import { getAppSettings, isTauri, setAppSettings, type AppSettings } from "../native";

export default function Server() {
  const { status, refresh, toast } = useApp();
  const srv = status?.server;
  const [port, setPort] = useState(String(srv?.port ?? 8765));
  const [key, setKey] = useState("");
  const [log, setLog] = useState<RequestLog[]>([]);
  const [tab, setTab] = useState("curl");
  const [app, setApp] = useState<AppSettings | null>(null);
  const since = useRef(0);

  useEffect(() => { api.settings().then(s => setKey(s.server?.api_key || "")).catch(() => {}); getAppSettings().then(setApp); }, []);
  useEffect(() => { if (srv) setPort(String(srv.port)); }, [srv?.port]);
  useEffect(() => {
    const f = async () => {
      try {
        const r = await api.requests(since.current);
        if (r.requests.length) {
          since.current = r.requests[r.requests.length - 1].t;
          setLog(l => [...r.requests.reverse(), ...l].slice(0, 200));
        }
      } catch { /* engine restarting */ }
    };
    f();
    const t = setInterval(f, 1000);
    return () => clearInterval(t);
  }, []);

  const set = async (s: Parameters<typeof api.server>[0], msg?: string) => {
    try { const r = await api.server(s); if (r.error) toast(r.error, true); else if (msg) toast(msg); } catch (e) { toast((e as Error).message, true); }
    refresh();
  };
  const setBg = async (close_to_tray: boolean) => { if (app) setApp(await setAppSettings({ ...app, close_to_tray })); };

  const on = !!srv?.enabled;
  const host = srv?.host ?? "127.0.0.1";
  const url = `http://${host === "0.0.0.0" && srv?.lan[0] ? srv.lan[0] : "127.0.0.1"}:${srv?.port ?? port}`;
  const model = status?.models.find(m => m.id === status.default)?.name ?? "FERMI-2B";
  const auth = key && host === "0.0.0.0";
  const body = `{"state": "Il pacco è arrivato rotto e nessuno risponde alle mail.",
 "questions": {
  "reclamo": {"type": "noul", "instructions": "È un reclamo?"},
  "urgenza": {"type": "score", "instructions": "Quanto è urgente?", "criteria": ["Bassa", "Media", "Alta"]}}}`;
  const examples: Record<string, string> = {
    curl: `curl ${url}/api/classify \\\n  -H "Content-Type: application/json" \\${auth ? `\n  -H "Authorization: Bearer ${key}" \\` : ""}\n  -d '${body.replace(/\n\s*/g, " ")}'`,
    python: `import requests\n\nr = requests.post("${url}/api/classify",${auth ? `\n    headers={"Authorization": "Bearer ${key}"},` : ""}\n    json={\n        "model": "${model}",          # facoltativo: il predefinito\n        "state": "Il pacco è arrivato rotto e nessuno risponde alle mail.",\n        "questions": {\n            "reclamo": {"type": "noul", "instructions": "È un reclamo?"},\n            "urgenza": {"type": "score", "instructions": "Quanto è urgente?",\n                        "criteria": ["Bassa", "Media", "Alta"]},\n        },\n    })\nprint(r.json()["answers"])`,
    js: `const r = await fetch("${url}/api/classify", {\n  method: "POST",\n  headers: {"Content-Type": "application/json"${auth ? `, "Authorization": "Bearer ${key}"` : ""}},\n  body: JSON.stringify({\n    state: {subject: "Rimborso", body: "Mi avete addebitato due volte"},\n    questions: {refund: {type: "noul", instructions: "Chiede un rimborso?"}},\n  }),\n});\nconsole.log((await r.json()).answers);`,
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Server API</h1>
          <p>Accendi il server e FERMI risponde anche agli altri programmi: script, app, automazioni. Come il server locale di LM Studio:
            resta acceso finché FERMI è aperto, anche con la finestra chiusa nell'area di notifica.</p>
        </div>
      </div>

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1.05fr) minmax(0, 1fr)", alignItems: "start" }}>
        <div className="col" style={{ gap: 14 }}>
          <div className="card">
            <div className="row" style={{ gap: 16 }}>
              <Switch big on={on} onChange={v => set({ enabled: v, port: +port }, v ? "Server acceso" : "Server spento")} />
              <div>
                <h2>{on ? "In ascolto" : "Spento"}</h2>
                <div className={on ? "" : "dim"} style={{ fontFamily: "var(--mono)", fontSize: 15 }}>{on ? url : "accendilo per esporre i modelli"}</div>
              </div>
              {on && <span className="pill ok" style={{ marginLeft: "auto" }}><span className="dot" />attivo</span>}
            </div>
            {srv?.error && <div className="notice bad" style={{ marginTop: 12 }}>{srv.error}</div>}
            <div className="sep" />
            <div className="row" style={{ gap: 16, alignItems: "flex-end" }}>
              <label className="field">Raggiungibile da
                <Seg value={host} onChange={h => set({ host: h, enabled: on }, h === "0.0.0.0" ? "Visibile nella rete locale" : "Solo questo computer")}
                     options={[["127.0.0.1", "solo questo computer"], ["0.0.0.0", "rete locale"]]} />
              </label>
              <label className="field" style={{ width: 110 }}>Porta
                <input value={port} onChange={e => setPort(e.target.value.replace(/\D/g, ""))} onBlur={() => +port !== srv?.port && set({ port: +port, enabled: on })} className="mono" />
              </label>
            </div>
            {host === "0.0.0.0" && (
              <div className="col" style={{ marginTop: 12 }}>
                <label className="field">Chiave API (richiesta solo a chi si collega da un altro computer)
                  <div className="row" style={{ flexWrap: "nowrap" }}>
                    <input type="password" value={key} onChange={e => setKey(e.target.value)} className="mono" placeholder="nessuna" />
                    <button onClick={() => { const k = "fermi-" + Array.from(crypto.getRandomValues(new Uint8Array(12))).map(b => b.toString(16).padStart(2, "0")).join(""); setKey(k); }}>Genera</button>
                    <button className="primary" onClick={() => set({ api_key: key, enabled: on }, "Chiave salvata")}>Salva</button>
                  </div>
                </label>
                <div className="notice warn small">Nella rete locale chiunque conosca l'indirizzo può usare il modello: imposta una chiave se la rete non è solo tua.</div>
              </div>
            )}
            <div className="sep" />
            <div className="row" style={{ gap: 12 }}>
              <Switch on={!!app?.close_to_tray} onChange={setBg} disabled={!isTauri()} />
              <div>
                <b>Resta attivo in background</b>
                <div className="dim small">Chiudendo la finestra FERMI resta nell'area di notifica con i modelli in memoria e il server acceso. Per uscire davvero: menu dell'icona → Esci.</div>
              </div>
            </div>
            <div className="row" style={{ gap: 12, marginTop: 12 }}>
              <span style={{ width: 46, display: "grid", placeItems: "center", color: "var(--violet)" }}>{Ico.bolt}</span>
              <div className="dim small">Il server si riaccende da solo alla prossima apertura se lo lasci acceso.</div>
            </div>
          </div>

          <div className="card">
            <div className="card-title">Endpoint</div>
            <table className="t">
              <tbody>
                <tr><td><span className="pill run">POST</span></td><td className="mono">/api/classify</td><td className="dim">uno stato e le sue domande → risposte con probabilità</td></tr>
                <tr><td /><td className="mono dim">/v1/classify</td><td className="dim">uguale; con <span className="mono">"states": [...]</span> classifica molti stati con le stesse domande</td></tr>
                <tr><td><span className="pill">GET</span></td><td className="mono">/api/models</td><td className="dim">modelli disponibili e stato</td></tr>
                <tr><td><span className="pill">GET</span></td><td className="mono">/api/info</td><td className="dim">modello predefinito e dispositivo</td></tr>
              </tbody>
            </table>
            <div className="dim small" style={{ marginTop: 10 }}>
              Campi facoltativi: <span className="mono">"model"</span> (per esempio <span className="mono">"FERMI-0.8B"</span>, se no il predefinito) e
              <span className="mono"> "orders": "reverse"</span> per porre le domande a scelta in due ordini e mediarle. Le demo di FERMI usano la porta 8765.
            </div>
          </div>
        </div>

        <div className="col" style={{ gap: 14 }}>
          <div className="card">
            <div className="tabs">
              {[["curl", "cURL"], ["python", "Python"], ["js", "JavaScript"]].map(([k, l]) => <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>{l}</button>)}
            </div>
            <CodeBox code={examples[tab]} />
          </div>
          <div className="card" style={{ padding: 0 }}>
            <div className="card-title" style={{ padding: "14px 16px 0" }}>Richieste in arrivo
              <span className="actions"><button className="sm ghost" onClick={() => setLog([])}>pulisci</button></span></div>
            <div className="scroll" style={{ maxHeight: 420 }}>
              <table className="t">
                <thead><tr><th>ora</th><th>da</th><th>percorso</th><th>modello</th><th>domande</th><th>ms</th><th /></tr></thead>
                <tbody>
                  {log.map((r, i) => (
                    <tr key={i}>
                      <td className="mono dim">{new Date(r.t * 1000).toLocaleTimeString("it-IT")}</td>
                      <td className="mono">{r.client === "127.0.0.1" ? "questo PC" : r.client}<span className="dim2">{r.server === "app" ? " · app" : ""}</span></td>
                      <td className="mono">{r.path}</td>
                      <td>{r.model ?? ""}</td>
                      <td className="num">{r.questions || ""}</td>
                      <td className="num">{r.ms}</td>
                      <td><span className={`pill ${r.status < 300 ? "ok" : "bad"}`}>{r.status}</span></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {!log.length && <div className="empty small">Nessuna richiesta per ora. Prova il comando cURL qui sopra.</div>}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
