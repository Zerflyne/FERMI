import type { ReactNode } from "react";
import type { Question, Questions, QType } from "../api";
import { Ico } from "./icons";
import { Seg } from "./ui";

const LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

function blank(type: QType): Question {
  if (type === "noul") return { type, instructions: "", criteria: { true: "", false: "" } };
  if (type === "choice") return { type, instructions: "", criteria: { "": "", " ": "" } };
  return { type, instructions: "", criteria: ["Basso", "Medio", "Alto"] };
}

function freeId(qs: Questions) {
  let i = Object.keys(qs).length + 1;
  while (qs["domanda_" + i]) i++;
  return "domanda_" + i;
}

/** Editor of a set of typed questions (id → question). Keeps the order of the object. */
export function QuestionEditor({ value, onChange }: { value: Questions; onChange: (q: Questions) => void }) {
  const entries = Object.entries(value);
  const setAt = (idx: number, id: string, q: Question) => {
    const next: Questions = {};
    entries.forEach(([k, v], i) => { if (i === idx) next[id] = q; else next[k] = v; });
    onChange(next);
  };
  const remove = (idx: number) => onChange(Object.fromEntries(entries.filter((_, i) => i !== idx)));
  const add = (type: QType) => onChange({ ...value, [freeId(value)]: blank(type) });

  return (
    <div className="col">
      {entries.map(([id, q], idx) => (
        <OneQuestion key={idx} id={id} q={q} onChange={(nid, nq) => setAt(idx, nid, nq)} onRemove={() => remove(idx)} />
      ))}
      <div className="row">
        <span className="dim small">Aggiungi:</span>
        <button className="sm" onClick={() => add("noul")}>{Ico.plus} Sì / no</button>
        <button className="sm" onClick={() => add("choice")}>{Ico.plus} Scelta</button>
        <button className="sm" onClick={() => add("score")}>{Ico.plus} Scala</button>
      </div>
    </div>
  );
}

function OneQuestion({ id, q, onChange, onRemove }: {
  id: string; q: Question; onChange: (id: string, q: Question) => void; onRemove: () => void }) {
  const setType = (t: QType) => {
    if (t === q.type) return;
    onChange(id, { ...blank(t), instructions: q.instructions });
  };
  return (
    <div className="q">
      <div className="q-head">
        <input className="qid" value={id} onChange={e => onChange(e.target.value.replace(/\s+/g, "_"), q)} title="Nome della domanda (chiave nel risultato)" />
        <Seg value={q.type} onChange={setType} options={[["noul", "sì / no"], ["choice", "scelta"], ["score", "scala"]]} />
        <span style={{ flex: 1 }} />
        <button className="sm ghost" onClick={onRemove} title="Elimina">{Ico.trash}</button>
      </div>
      <input value={q.instructions} placeholder="La domanda, per esempio: il cliente chiede un rimborso?"
             onChange={e => onChange(id, { ...q, instructions: e.target.value })} />
      {q.type === "noul" && <NoulCriteria q={q} onChange={nq => onChange(id, nq)} />}
      {q.type === "choice" && <ChoiceCriteria q={q} onChange={nq => onChange(id, nq)} />}
      {q.type === "score" && <ScoreCriteria q={q} onChange={nq => onChange(id, nq)} />}
    </div>
  );
}

function NoulCriteria({ q, onChange }: { q: Question; onChange: (q: Question) => void }) {
  const c = (q.criteria || {}) as { true?: string; false?: string };
  return (
    <div className="q-opts noul">
      <span className="letter">sì</span>
      <input value={c.true || ""} placeholder="(facoltativo) cosa conta come sì" onChange={e => onChange({ ...q, criteria: { ...c, true: e.target.value } })} />
      <span className="letter">no</span>
      <input value={c.false || ""} placeholder="(facoltativo) cosa conta come no" onChange={e => onChange({ ...q, criteria: { ...c, false: e.target.value } })} />
    </div>
  );
}

function ChoiceCriteria({ q, onChange }: { q: Question; onChange: (q: Question) => void }) {
  // options are kept as an array while editing so that empty or duplicate names do not collapse
  const opts = Object.entries((q.criteria || {}) as Record<string, string>);
  const set = (list: [string, string][]) => {
    const o: Record<string, string> = {};
    list.forEach(([k, v]) => { let key = k || " "; while (key in o) key += " "; o[key] = v; });
    onChange({ ...q, criteria: o });
  };
  return (
    <div className="q-opts">
      {opts.map(([k, v], i) => (
        <Frag key={i}>
          <span className="letter">{LETTERS[i]}</span>
          <input value={k.trim()} placeholder="opzione" onChange={e => set(opts.map((x, j) => j === i ? [e.target.value, x[1]] : x))} />
          <input value={v} placeholder="(facoltativo) descrizione" onChange={e => set(opts.map((x, j) => j === i ? [x[0], e.target.value] : x))} />
          <button className="sm ghost" disabled={opts.length <= 2} onClick={() => set(opts.filter((_, j) => j !== i))}>{Ico.x}</button>
        </Frag>
      ))}
      {opts.length < 26 && <><span /><button className="sm ghost" style={{ justifySelf: "start" }} onClick={() => set([...opts, ["", ""]])}>{Ico.plus} opzione</button></>}
    </div>
  );
}

function ScoreCriteria({ q, onChange }: { q: Question; onChange: (q: Question) => void }) {
  const lv = (q.criteria || []) as string[];
  return (
    <>
      <div className="dim small">Livelli dal più basso al più alto</div>
      <div className="q-opts score">
        {lv.map((s, i) => (
          <Frag key={i}>
            <span className="letter">{i}</span>
            <input value={s} onChange={e => onChange({ ...q, criteria: lv.map((x, j) => j === i ? e.target.value : x) })} />
            <button className="sm ghost" disabled={lv.length <= 2} onClick={() => onChange({ ...q, criteria: lv.filter((_, j) => j !== i) })}>{Ico.x}</button>
          </Frag>
        ))}
        {lv.length < 26 && <><span /><button className="sm ghost" style={{ justifySelf: "start" }} onClick={() => onChange({ ...q, criteria: [...lv, ""] })}>{Ico.plus} livello</button></>}
      </div>
    </>
  );
}

const Frag = ({ children }: { children: ReactNode }) => <>{children}</>;
