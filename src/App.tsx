import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Status } from "./api";
import { Ico, Logo } from "./components/icons";
import { CatalogCard, hardwareLine, recommended } from "./components/catalog";
import { AppCtx, useApp, useToast } from "./components/ui";
import { engineInfo, isTauri, pickFile } from "./native";
import Models from "./pages/Models";
import Playground from "./pages/Playground";
import Analysis from "./pages/Analysis";
import Server from "./pages/Server";
import Settings from "./pages/Settings";

const PAGES = [
  { id: "models", label: "Modelli", icon: Ico.models },
  { id: "playground", label: "Playground", icon: Ico.play },
  { id: "analysis", label: "Analisi cartelle", icon: Ico.chart },
  { id: "server", label: "Server API", icon: Ico.server },
  { id: "settings", label: "Impostazioni", icon: Ico.gear },
];

export default function App() {
  const [page, setPage] = useState(() => localStorage.getItem("fermi.page") || "playground");
  const [status, setStatus] = useState<Status | null>(null);
  const [offline, setOffline] = useState<string | null>(null);
  const [theme, setTheme] = useState(() => localStorage.getItem("fermi.theme") || "dark");
  const { toast, node } = useToast();

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.status());
      setOffline(null);
    } catch (e) {
      const info = await engineInfo();
      setOffline(info?.error || (info && !info.running ? "Il motore non è in esecuzione." : "Il motore si sta avviando…"));
    }
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("fermi.theme", theme);
  }, [theme]);
  useEffect(() => { localStorage.setItem("fermi.page", page); }, [page]);
  // the first model just arrived (welcome screen): straight to the Playground
  const nModels = useRef<number | null>(null);
  useEffect(() => {
    const n = status?.models.length;
    if (n == null) return;
    if (nModels.current === 0 && n > 0) setPage("playground");
    nModels.current = n;
  }, [status]);

  const ready = status?.models.filter(m => m.status === "ready") ?? [];
  const running = status?.active_jobs.length ?? 0;

  return (
    <AppCtx.Provider value={{ status, offline, refresh, toast, go: setPage }}>
      <div className="app">
        <aside className="side">
          <div className="brand"><Logo /><div><b>FERMI</b><span>classificatore locale</span></div></div>
          {PAGES.map(p => (
            <button key={p.id} className={`nav ${page === p.id ? "on" : ""}`} onClick={() => setPage(p.id)}>
              {p.icon}{p.label}
              {p.id === "analysis" && running > 0 && <span className="badge">{running}</span>}
              {p.id === "server" && status?.server.enabled && <span className="pill ok" style={{ marginLeft: "auto", padding: "0 6px" }}><span className="dot" /></span>}
            </button>
          ))}
          <div className="grow" />
          <div className="side-foot">
            {offline ? <span className="pill warn"><span className="spin" style={{ width: 10, height: 10 }} />motore</span>
              : <span className="dim">{ready.length ? ready.map(m => m.name).join(" · ") : "nessun modello caricato"}</span>}
            {status && <span className="dim2">{status.counter.requests} richieste · {status.counter.questions} domande</span>}
            <div className="row">
              <button className="sm ghost" onClick={() => setTheme(theme === "dark" ? "light" : "dark")} title="Tema">
                {theme === "dark" ? Ico.sun : Ico.moon}{theme === "dark" ? "chiaro" : "scuro"}
              </button>
              {!isTauri() && <span className="dim2 small">browser</span>}
            </div>
          </div>
        </aside>
        <main className="main">
          {offline && !status ? <Starting msg={offline} /> : status && !status.models.length && page !== "settings" ? <Welcome /> : (
            <>
              {page === "models" && <Models />}
              {page === "playground" && <Playground />}
              {page === "analysis" && <Analysis />}
              {page === "server" && <Server />}
              {page === "settings" && <Settings />}
            </>
          )}
        </main>
      </div>
      {node}
    </AppCtx.Provider>
  );
}

function Starting({ msg }: { msg: string }) {
  return (
    <div className="page" style={{ display: "grid", placeItems: "center", minHeight: "80vh" }}>
      <div className="col" style={{ alignItems: "center", textAlign: "center", maxWidth: 520 }}>
        <Logo size={64} />
        <h2 style={{ marginTop: 10 }}>Avvio del motore</h2>
        <p className="dim">{msg}</p>
        <p className="dim2 small">Il motore è la parte dell'app che tiene in memoria i modelli e risponde alle domande. Di solito
          parte in un secondo; se resta qui, chiudi FERMI e riaprilo.</p>
      </div>
    </div>
  );
}

/** First start: no model on this computer yet. One click downloads the model that suits this computer. */
function Welcome() {
  const { status, refresh, toast } = useApp();
  const rec = recommended(status);
  const cat = [...(status?.catalog ?? [])].sort((a, b) => (a.id === rec ? -1 : b.id === rec ? 1 : 0));
  const add = async () => {
    const p = await pickFile("File del modello FERMI (.gguf)", "gguf");
    if (!p) return;
    try { await api.addModel(p); toast("Modello aggiunto"); } catch (e) { toast(String((e as Error).message), true); }
    refresh();
  };
  return (
    <div className="page" style={{ maxWidth: 980, margin: "0 auto" }}>
      <div className="row" style={{ gap: 16, margin: "24px 0 8px", flexWrap: "nowrap" }}>
        <Logo size={56} />
        <div>
          <h1 style={{ margin: 0 }}>Benvenuto in FERMI</h1>
          <p className="dim" style={{ margin: "4px 0 0" }}>Per iniziare serve un modello. Si scarica una volta sola, poi FERMI funziona
            anche senza internet e nessun dato esce dal computer.</p>
        </div>
      </div>
      <div className="notice ico" style={{ margin: "14px 0 18px" }}>{Ico.chip}<span>{hardwareLine(status)}</span></div>
      <div className="grid g2">{cat.map(c => <CatalogCard key={c.id} c={c} rec={c.id === rec} />)}</div>
      <p className="dim2 small" style={{ marginTop: 18 }}>
        Appena il download finisce il modello si carica da solo e si apre il Playground. Se il download si interrompe, riparte
        da dove era arrivato. Hai già il file?{" "}
        <a href="#" onClick={e => { e.preventDefault(); add(); }}>Scegli un file .gguf</a>
      </p>
    </div>
  );
}
