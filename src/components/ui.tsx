import { createContext, useContext, useState, type ReactNode } from "react";
import { answerItems, pct, type Answer, type ModelEntry, type Question, type Status } from "../api";
import { Ico } from "./icons";

// ------------------------------------------------------------------------------------------------- app context
export interface Ctx {
  status: Status | null;
  offline: string | null;
  refresh: () => void;
  toast: (msg: string, err?: boolean) => void;
  go: (page: string) => void;
}
export const AppCtx = createContext<Ctx>({ status: null, offline: null, refresh: () => {}, toast: () => {}, go: () => {} });
export const useApp = () => useContext(AppCtx);

// ------------------------------------------------------------------------------------------------------ controls
export function Switch({ on, onChange, big, disabled }: { on: boolean; onChange: (v: boolean) => void; big?: boolean; disabled?: boolean }) {
  return <button className={`switch ${on ? "on" : ""} ${big ? "big" : ""}`} disabled={disabled} onClick={() => onChange(!on)} aria-pressed={on} />;
}

export function Seg<T extends string>({ value, options, onChange, violet }: {
  value: T; options: [T, ReactNode][]; onChange: (v: T) => void; violet?: boolean }) {
  return (
    <span className={`seg ${violet ? "violet" : ""}`}>
      {options.map(([v, l]) => <button key={v} className={v === value ? "on" : ""} onClick={() => onChange(v)}>{l}</button>)}
    </span>
  );
}

export function ModelStatus({ m }: { m: ModelEntry }) {
  if (m.status === "ready") return <span className="pill ok"><span className="dot" />pronto</span>;
  if (m.status === "loading") return <span className="pill run"><span className="dot" />caricamento…</span>;
  if (m.status === "error") return <span className="pill bad" title={m.error || ""}><span className="dot" />errore</span>;
  return <span className="pill"><span className="dot" />non caricato</span>;
}

export function CodeBox({ code }: { code: string }) {
  const { toast } = useApp();
  return (
    <div className="codebox">
      <button className="sm ghost" onClick={() => { navigator.clipboard.writeText(code); toast("Copiato"); }}>{Ico.copy}</button>
      <pre>{code}</pre>
    </div>
  );
}

// -------------------------------------------------------------------------------------------- probability bars
export function Bars({ items, variant = 0, best }: { items: [string, number][]; variant?: number; best?: string }) {
  const top = best ?? items.reduce((a, b) => (b[1] > a[1] ? b : a), items[0] ?? ["", 0])[0];
  return (
    <div className={`bars m${variant}`}>
      {items.map(([l, p]) => (
        <Row key={l} l={l} p={p} win={l === top} />
      ))}
    </div>
  );
}
function Row({ l, p, win }: { l: string; p: number; win: boolean }) {
  const [label, desc] = splitLabel(l);
  return (
    <>
      <div className="lab" title={l}>{label}{desc && <small>{desc}</small>}</div>
      <div className={`b ${win ? "win" : ""}`}><i style={{ width: `${(100 * p).toFixed(1)}%` }} /></div>
      <div className={`v ${win ? "win" : ""}`}>{pct(p)}</div>
    </>
  );
}
const splitLabel = (l: string): [string, string] => {
  const i = l.search(/[:;]/);
  return i > 0 && i < l.length - 1 ? [l.slice(0, i), l.slice(i + 1).trim()] : [l, ""];
};

/** one answer: headline + bars */
export function AnswerCard({ qid, q, a, variant = 0, tag }: { qid: string; q: Question; a: Answer; variant?: number; tag?: string }) {
  const items = answerItems(q, a);
  let head = "";
  if (a.type === "noul") head = a.p_yes >= 0.5 ? `sì · ${pct(a.p_yes)}` : `no · ${pct(1 - a.p_yes)}`;
  else if (a.type === "choice") head = a.answer;
  else head = `livello ${a.expected.toFixed(2)}`;
  return (
    <div className="answer">
      <div className="answer-head">
        {tag && <span className={`model-tag m${variant}`}>{tag}</span>}
        <b>{qid}</b>
        <span className="dim small">{q.instructions}</span>
        <span className="big" style={variant ? { color: "var(--kraft)" } : undefined}>{head}</span>
      </div>
      <Bars items={items} variant={variant} />
    </div>
  );
}

// ------------------------------------------------------------------------------------------------------- toast
export function useToast() {
  const [t, setT] = useState<{ msg: string; err: boolean; id: number } | null>(null);
  const toast = (msg: string, err = false) => {
    const id = Date.now();
    setT({ msg, err, id });
    setTimeout(() => setT(cur => (cur && cur.id === id ? null : cur)), err ? 5000 : 2200);
  };
  const node = t ? <div className={`toast ${t.err ? "err" : ""}`}>{t.msg}</div> : null;
  return { toast, node };
}

export function Empty({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return <div className="empty">{icon}<div>{children}</div></div>;
}
