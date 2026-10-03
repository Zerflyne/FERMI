import { useEffect, useState } from "react";
import { api, fmtBytes, type SystemInfo } from "../api";
import { Logo } from "../components/icons";
import { Seg, Switch, useApp } from "../components/ui";
import { engineInfo, engineLog, getAppSettings, openPath, setAppSettings, type AppSettings, type EngineInfo } from "../native";

const KIND: Record<string, string> = { gpu: "scheda video", igpu: "scheda video integrata", cpu: "processore", other: "acceleratore" };

export default function Settings() {
  const { status, toast } = useApp();
  const [app, setApp] = useState<AppSettings | null>(null);
  const [info, setInfo] = useState<EngineInfo | null>(null);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [device, setDevice] = useState("auto");
  const [threads, setThreads] = useState(0);
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    getAppSettings().then(setApp);
    engineInfo().then(setInfo);
    api.system().then(setSys).catch(() => {});
    api.settings().then(s => { setDevice(s.device || "auto"); setThreads(s.threads || 0); }).catch(() => {});
    const f = () => engineLog().then(setLog);
    f();
    const t = setInterval(f, 2000);
    return () => clearInterval(t);
  }, []);

  const save = async (s: AppSettings) => setApp(await setAppSettings(s));
  const saveEngine = async (s: Record<string, unknown>) => {
    try { await api.saveSettings(s); toast("Salvato: i modelli in memoria si ricaricano"); }
    catch (e) { toast(String((e as Error).message), true); }
  };
  const accel = (sys?.devices ?? []).filter(d => d.kind !== "cpu");
  const loaded = (status?.models ?? []).filter(m => m.status === "ready");

  return (
    <div className="page">
      <div className="page-head"><div><h1>Impostazioni</h1></div></div>
      <div className="grid g2" style={{ alignItems: "start" }}>
        <div className="col" style={{ gap: 14 }}>
          <div className="card">
            <div className="card-title">Calcolo</div>
            <div className="col">
              <label className="field">Dove far girare i modelli
                <Seg value={device} onChange={d => { setDevice(d); saveEngine({ device: d }); }}
                     options={[["auto", "automatico"], ...accel.map(d => [d.name, d.description] as [string, string]), ["cpu", "solo processore"]]} />
              </label>
              <div className="dim small">
                Automatico usa la scheda video se c'è (e se il modello ci sta), altrimenti il processore.
                {loaded.length > 0 && <> Ora: {loaded.map(m => `${m.name} su ${m.device}`).join(", ")}. Cambiando scelta si ricaricano
                  da soli in pochi secondi.</>}
              </div>
              <label className="field">Thread del processore (quando si usa la CPU)
                <div className="row" style={{ flexWrap: "nowrap" }}>
                  <Seg value={threads ? "n" : "auto"} onChange={v => { const t = v === "auto" ? 0 : Math.max(1, Math.floor((sys?.threads || 8) / 2)); setThreads(t); saveEngine({ threads: t }); }}
                       options={[["auto", "automatico"], ["n", "a mano"]]} />
                  {threads > 0 && <input type="number" min={1} max={256} value={threads} style={{ width: 90 }}
                                         onChange={e => setThreads(Math.max(1, +e.target.value || 1))}
                                         onBlur={() => saveEngine({ threads })} />}
                </div>
              </label>
              <div className="dim small">Cartella dei dati (modelli, impostazioni, analisi):
                <a href="#" className="mono" onClick={e => { e.preventDefault(); status && openPath(status.data_dir); }}> {status?.data_dir}</a></div>
              {info?.error && <div className="notice bad small">{info.error}</div>}
              {status && !status.runtime.ok && <div className="notice bad small">Motore di calcolo non disponibile: {status.runtime.error}</div>}
            </div>
          </div>

          <div className="card">
            <div className="card-title">Finestra</div>
            <div className="col">
              <div className="row" style={{ gap: 12 }}>
                <Switch on={!!app?.close_to_tray} disabled={!app} onChange={v => app && save({ ...app, close_to_tray: v })} />
                <div><b>Chiudendo la finestra, resta nell'area di notifica</b><div className="dim small">I modelli restano in memoria e il server API resta acceso.</div></div>
              </div>
              <div className="row" style={{ gap: 12 }}>
                <Switch on={!!app?.start_hidden} disabled={!app} onChange={v => app && save({ ...app, start_hidden: v })} />
                <div><b>Avvia nascosto</b><div className="dim small">Utile se FERMI parte con il sistema e deve solo fare da server.</div></div>
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-title">Sistema</div>
            <table className="t"><tbody>
              <tr><td className="dim">Sistema</td><td className="mono small">{sys ? `${sys.platform} · ${sys.arch}` : "–"}</td></tr>
              <tr><td className="dim">Processore</td><td className="small">{sys?.cpu ?? "–"}{sys ? ` · ${sys.threads} thread` : ""}</td></tr>
              <tr><td className="dim">Memoria</td><td className="small">{sys ? `${fmtBytes(sys.ram.total)} (libera ${fmtBytes(sys.ram.available)})` : "–"}</td></tr>
              {(sys?.devices ?? []).filter(d => d.kind !== "cpu").map(d =>
                <tr key={d.name}><td className="dim">{KIND[d.kind] ?? d.kind}</td><td className="small">{d.description}{d.total ? ` · ${fmtBytes(d.total)}` : ""} <span className="dim2 mono">{d.name}</span></td></tr>)}
              {sys && !accel.length && <tr><td className="dim">Scheda video</td><td className="small">nessuna compatibile: si usa il processore</td></tr>}
              <tr><td className="dim">Motore</td><td className="mono small">FERMI {sys?.engine ?? info?.version ?? ""} · llama.cpp</td></tr>
              <tr><td className="dim">Runtime</td><td className="mono small truncate" title={sys?.runtime.dir ?? ""}>{sys?.runtime.dir ?? info?.runtime_dir ?? "–"}</td></tr>
            </tbody></table>
          </div>

          <div className="card">
            <div className="row" style={{ gap: 14 }}>
              <Logo size={46} />
              <div>
                <h2>FERMI {info?.version ?? ""}</h2>
                <div className="dim small">Modelli e app open source, licenza Apache-2.0.</div>
                <div className="row small" style={{ marginTop: 4 }}>
                  <a href="#" onClick={e => { e.preventDefault(); openPath("https://github.com/Zerflyne/FERMI"); }}>GitHub</a>
                  <a href="#" onClick={e => { e.preventDefault(); openPath("https://huggingface.co/Zerflyne/FERMI-2B"); }}>FERMI-2B</a>
                  <a href="#" onClick={e => { e.preventDefault(); openPath("https://huggingface.co/Zerflyne/FERMI-0.8B"); }}>FERMI-0.8B</a>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className="card">
          <div className="card-title">Registro del motore</div>
          <div className="log">{log.join("\n")}</div>
        </div>
      </div>
    </div>
  );
}
