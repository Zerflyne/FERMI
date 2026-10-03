import { useEffect, useState, type ReactNode } from "react";
import { api, fmtBytes, fmtDur, pct, type ModelEntry, type SystemInfo } from "../api";
import { CatalogCard, recommended } from "../components/catalog";
import { Ico } from "../components/icons";
import { ModelStatus, useApp } from "../components/ui";
import { openPath, pickFile } from "../native";

export default function Models() {
  const { status, refresh, toast } = useApp();
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [autoload, setAutoload] = useState<string[]>([]);
  useEffect(() => { api.settings().then(s => setAutoload(s.autoload || [])).catch(() => {}); }, []);
  const toggleAuto = async (id: string) => {
    const next = autoload.includes(id) ? autoload.filter(x => x !== id) : [...autoload, id];
    setAutoload(next);
    await api.saveSettings({ autoload: next });
    toast(next.includes(id) ? "Si caricherà all'avvio" : "Non si caricherà all'avvio");
  };

  useEffect(() => {
    const f = () => api.system().then(setSys).catch(() => {});
    f();
    const t = setInterval(f, 3000);
    return () => clearInterval(t);
  }, []);

  const act = async (id: string, fn: () => Promise<unknown>, msg?: string) => {
    setBusy(id);
    try { await fn(); if (msg) toast(msg); } catch (e) { toast(String((e as Error).message), true); }
    setBusy(null);
    refresh();
  };
  const add = async () => {
    const p = await pickFile("File del modello FERMI (.gguf)", "gguf");
    if (p) act("add", () => api.addModel(p), "Modello aggiunto");
  };

  const models = status?.models ?? [];
  const toGet = (status?.catalog ?? []).filter(c => !c.installed);
  const rec = recommended(status);
  const gpu = sys?.gpu;
  const used = gpu?.vram_used ?? 0, total = gpu?.vram_total ?? 0;
  const ram = sys?.ram;

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Modelli</h1>
          <p>FERMI legge uno <b>stato</b> (un testo o un JSON) e risponde a domande tipizzate con delle probabilità. Puoi tenere
            in memoria più modelli insieme: il Playground li confronta e il server API li espone ad altri programmi.</p>
        </div>
        <div className="actions">
          <button onClick={() => act("disc", () => api.discover().then(r => toast(r.added ? `Trovati ${r.added} modelli nuovi` : "Nessun modello nuovo")))}>{Ico.refresh} Aggiorna</button>
          <button onClick={add}>{Ico.plus} Aggiungi un file .gguf</button>
        </div>
      </div>

      <div className="grid g4" style={{ marginBottom: 16 }}>
        <div className="stat">
          <span>Calcolo su</span>
          <b style={{ fontSize: 17, marginTop: 2 }}>{gpu?.name ?? (sys ? "processore (CPU)" : "…")}</b>
        </div>
        <div className="stat">
          {gpu ? <span>Memoria video {total ? `${fmtBytes(used)} di ${fmtBytes(total)}` : ""}</span>
               : <span>Memoria {ram ? `${fmtBytes(ram.total - ram.available)} di ${fmtBytes(ram.total)}` : ""}</span>}
          <div className="meter" style={{ marginTop: 10 }}>
            <i style={{ width: gpu ? (total ? `${(100 * used) / total}%` : 0) : ram ? `${(100 * (ram.total - ram.available)) / ram.total}%` : 0 }} />
          </div>
        </div>
        <div className="stat"><b>{status?.counter.questions ?? 0}</b><span>domande servite da quando è aperto</span></div>
        <div className="stat"><b>{fmtDur(status?.uptime)}</b><span>motore attivo da</span></div>
      </div>

      {status && !status.runtime.ok && <div className="notice bad" style={{ marginBottom: 16 }}>
        Il motore di calcolo (llama.cpp) non si è avviato: {status.runtime.error}. Reinstallare l'app di solito risolve.</div>}

      <div className="grid g2">
        {models.map(m => <ModelCard key={m.id} m={m} isDefault={status?.default === m.id} busy={busy === m.id}
                                    auto={autoload.includes(m.id)} onAuto={() => toggleAuto(m.id)}
                                    onLoad={() => act(m.id, () => api.load(m.id))}
                                    onUnload={() => act(m.id, () => api.unload(m.id), m.name + " tolto dalla memoria")}
                                    onDefault={() => act(m.id, () => api.setDefault(m.id), m.name + " è il modello predefinito")}
                                    onRemove={() => confirm(m.source === "downloaded"
                                      ? `Eliminare ${m.name} da questo computer? Potrai riscaricarlo quando vuoi.`
                                      : `Togliere ${m.name} dall'elenco? Il file resta dove è.`) && act(m.id, () => api.removeModel(m.id), m.name + " rimosso")} />)}
      </div>

      {toGet.length > 0 && <>
        <h3 className="card-title" style={{ margin: "26px 0 12px" }}>Da scaricare</h3>
        <div className="grid g2">{toGet.map(c => <CatalogCard key={c.id} c={c} rec={!models.length && c.id === rec} />)}</div>
      </>}
    </div>
  );
}

function ModelCard({ m, isDefault, busy, auto, onAuto, onLoad, onUnload, onDefault, onRemove }: {
  m: ModelEntry; isDefault: boolean; busy: boolean; auto: boolean; onAuto: () => void; onLoad: () => void; onUnload: () => void;
  onDefault: () => void; onRemove: () => void }) {
  const ev = m.evaluation || {};
  const types: [string, string][] = [["noul", "sì / no"], ["choice", "scelta"], ["score", "scala"]];
  return (
    <div className="card" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <div className="row" style={{ alignItems: "flex-start" }}>
        <div>
          <div className="row"><h2 style={{ fontSize: 26 }}>{m.name}</h2>{isDefault && <span className="pill run" title="Usato quando una richiesta non indica il modello">{Ico.star} predefinito</span>}</div>
          <div className="dim small">base <span className="mono">{m.base_model}</span>{m.quant ? ` · ${m.quant}` : ""} · {fmtBytes(m.file_bytes)}</div>
        </div>
        <span style={{ marginLeft: "auto" }}><ModelStatus m={m} /></span>
      </div>
      <div className="grid g3">
        <div className="stat"><b>{ev.agreement != null ? pct(ev.agreement) : "–"}</b><span>accordo con il maestro (27B)</span></div>
        <div className="stat"><b>{ev.reference != null ? pct(ev.reference, 0) : "–"}</b><span>accordo con il riferimento esterno</span></div>
        <div className="stat"><b>{m.status === "ready" ? fmtBytes(m.vram_bytes) : "–"}</b><span>memoria {m.device ? `(${m.device})` : ""}</span></div>
      </div>
      <div className="bars">
        {types.map(([k, l]) => {
          const v = ev.by_type?.[k];
          return v == null ? null : (
            <Frag key={k}><div className="lab">{l}</div><div className="b win"><i style={{ width: `${100 * v}%` }} /></div><div className="v">{pct(v)}</div></Frag>
          );
        })}
      </div>
      {m.note && <div className="notice warn small">{m.note}</div>}
      {m.error && <div className="notice bad small">{m.error}</div>}
      <div className="row">
        {m.status === "ready" ? <button onClick={onUnload} disabled={busy}>Togli dalla memoria</button>
          : <button className="primary" onClick={onLoad} disabled={busy || m.status === "loading"}>
              {m.status === "loading" ? <><span className="spin" /> Caricamento…</> : <>{Ico.bolt} Carica</>}</button>}
        {!isDefault && <button className="ghost" onClick={onDefault}>{Ico.star} Rendi predefinito</button>}
        <span style={{ flex: 1 }} />
        <label className="check"><input type="checkbox" checked={auto} onChange={onAuto} /> carica all'avvio</label>
        <button className="ghost sm" onClick={onRemove} title={m.source === "downloaded" ? "Elimina dal computer" : "Togli dall'elenco"}>{Ico.trash}</button>
      </div>
      <a href="#" className="dim2 small mono truncate" title={m.path} onClick={e => { e.preventDefault(); openPath(m.path.replace(/[\\/][^\\/]*$/, "")); }}>{m.path}</a>
    </div>
  );
}
const Frag = ({ children }: { children: ReactNode }) => <>{children}</>;
