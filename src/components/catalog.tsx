// The models the app can download (the engine's catalog), with the download progress. Used by the Models page and
// by the welcome screen of the first start.
import { useState } from "react";
import { api, fmtBytes, fmtDur, type CatalogEntry, type Status } from "../api";
import { Ico } from "./icons";
import { useApp } from "./ui";

/** the model to suggest on this computer: the 2B when a graphics card can hold it, otherwise the 0.8B */
export function recommended(status: Status | null): string | null {
  const cat = status?.catalog ?? [];
  const gpu = (status?.runtime.devices ?? []).some(d => d.kind === "gpu" || d.kind === "igpu");
  const big = cat.find(c => c.id === "fermi-2b");
  if (gpu && big?.fits_gpu) return big.id;
  return cat.find(c => c.id === "fermi-0.8b")?.id ?? cat[0]?.id ?? null;
}

/** where the models run, in words */
export function hardwareLine(status: Status | null): string {
  const devs = status?.runtime.devices ?? [];
  const g = devs.find(d => d.kind === "gpu") ?? devs.find(d => d.kind === "igpu");
  if (!status) return "…";
  if (!status.runtime.ok) return "Il motore di calcolo non si è avviato: reinstalla l'app.";
  if (g) return `Scheda video: ${g.description}${g.total ? ` (${Math.round(g.total / 2 ** 30)} GB)` : ""}. FERMI la userà in automatico.`;
  return "Nessuna scheda video compatibile: FERMI userà il processore. Funziona, solo più lentamente.";
}

export function CatalogCard({ c, rec }: { c: CatalogEntry; rec?: boolean }) {
  const { refresh, toast } = useApp();
  const [busy, setBusy] = useState(false);
  const d = c.download;
  const running = d?.state === "running";
  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast(String((e as Error).message), true); }
    setBusy(false);
    refresh();
  };
  const total = d?.total || c.bytes;
  const frac = running && total ? d!.done / total : 0;
  const eta = running && d!.speed > 0 ? (total - d!.done) / d!.speed : null;
  return (
    <div className="card" style={{ display: "flex", flexDirection: "column", gap: 10, borderColor: rec ? "var(--violet)" : undefined }}>
      <div className="row" style={{ alignItems: "baseline" }}>
        <h2 style={{ fontSize: 24 }}>{c.name}</h2>
        {rec && <span className="pill run">{Ico.star} consigliato per questo computer</span>}
        {c.installed && <span className="pill ok" style={{ marginLeft: "auto" }}><span className="dot" />installato</span>}
      </div>
      <div className="dim">{c.description}</div>
      <div className="dim2 small">
        Download {fmtBytes(c.bytes)} · memoria necessaria circa {fmtBytes(c.memory)}
        {c.fits_gpu === false && " · troppo grande per la scheda video: girerà sul processore"}
      </div>
      {running && (
        <div className="col" style={{ gap: 6 }}>
          <div className="meter" style={{ height: 8 }}><i style={{ width: `${100 * frac}%` }} /></div>
          <div className="row small dim">
            <span>{fmtBytes(d!.done)} di {fmtBytes(total)} · {(100 * frac).toFixed(0)}%</span>
            <span>{d!.speed > 0 ? `${fmtBytes(d!.speed)}/s · ancora ${fmtDur(eta)}` : "connessione…"}</span>
          </div>
        </div>
      )}
      {d?.state === "error" && <div className="notice bad small">Download non riuscito: {d.error}. Riprova: riparte da dove si era fermato.</div>}
      {!c.installed && (
        <div className="row">
          {running
            ? <button onClick={() => act(() => api.cancelDownload(c.id))} disabled={busy}>{Ico.x} Annulla</button>
            : <button className="primary" onClick={() => act(() => api.download(c.id))} disabled={busy}>
                {Ico.download} {d?.state === "error" || d?.state === "cancelled" ? "Riprendi il download" : `Scarica (${fmtBytes(c.bytes)})`}</button>}
          <a href="#" className="small dim2" style={{ marginLeft: "auto" }}
             onClick={e => { e.preventDefault(); import("../native").then(n => n.openPath(c.url)); }}>scheda del modello</a>
        </div>
      )}
    </div>
  );
}
