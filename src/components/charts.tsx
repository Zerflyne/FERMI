import type { ReactNode } from "react";
import { PALETTE, pct, shortLevel, type Question, type ResultRow } from "../api";

/** the answer of one row to one question, as a label (yes/no, option, level) */
export function labelOf(q: Question, r: ResultRow, qid: string): string | null {
  const a = r.answers?.[qid];
  if (!a) return null;
  if (a.type === "noul") return a.p_yes >= 0.5 ? "sì" : "no";
  if (a.type === "choice") return a.answer;
  return shortLevel(((q.criteria as string[]) || [])[a.answer] ?? String(a.answer));
}
/** the labels a question can produce, in order */
export function labelsOf(q: Question): string[] {
  if (q.type === "noul") return ["sì", "no"];
  if (q.type === "choice") return Object.keys((q.criteria || {}) as Record<string, string>);
  return ((q.criteria as string[]) || []).map(shortLevel);
}
/** a number per row: p(yes), the probability of the chosen option, or the expected level */
export function valueOf(r: ResultRow, qid: string): number | null {
  const a = r.answers?.[qid];
  if (!a) return null;
  if (a.type === "noul") return a.p_yes;
  if (a.type === "choice") return a.probabilities[a.answer];
  return a.expected;
}

// ---------------------------------------------------------------------------------------------------- one question
export function QuestionChart({ qid, q, rows, filter, onFilter }: {
  qid: string; q: Question; rows: ResultRow[]; filter: string | null; onFilter: (label: string | null) => void }) {
  const labels = labelsOf(q);
  const counts = Object.fromEntries(labels.map(l => [l, 0])) as Record<string, number>;
  let n = 0;
  for (const r of rows) {
    const l = labelOf(q, r, qid);
    if (l == null) continue;
    counts[l] = (counts[l] || 0) + 1;
    n++;
  }
  const max = Math.max(1, ...Object.values(counts));
  let kpi = "";
  if (q.type === "noul") kpi = n ? pct(counts["sì"] / n, 0) + " sì" : "–";
  if (q.type === "choice") { const top = labels.reduce((a, b) => (counts[b] > counts[a] ? b : a), labels[0]); kpi = n ? top : "–"; }
  if (q.type === "score") {
    const ex = rows.map(r => valueOf(r, qid)).filter((x): x is number => x != null);
    kpi = ex.length ? (ex.reduce((a, b) => a + b, 0) / ex.length).toFixed(2) : "–";
  }
  return (
    <div className="card chart-card">
      <div className="chart-head">
        <b>{qid}</b>
        <span className="dim small truncate" style={{ maxWidth: 300 }} title={q.instructions}>{q.instructions}</span>
        <span className="kpi">{kpi}</span>
      </div>
      {q.type === "score" ? <Columns labels={labels} counts={counts} n={n} filter={filter} onFilter={onFilter} />
        : q.type === "noul" ? <NoulView counts={counts} n={n} rows={rows} qid={qid} filter={filter} onFilter={onFilter} />
        : (
          <div className="hbar">
            {labels.map((l, i) => (
              <Hrow key={l} label={l} c={counts[l]} n={n} max={max} color={PALETTE[i % PALETTE.length]} on={filter === l}
                    onClick={() => onFilter(filter === l ? null : l)} />
            ))}
          </div>
        )}
      {q.type === "score" && <div className="dim small">valore atteso medio (0 = {labels[0]}, {labels.length - 1} = {labels[labels.length - 1]})</div>}
    </div>
  );
}

function Hrow({ label, c, n, max, color, on, onClick }: { label: string; c: number; n: number; max: number; color: string; on: boolean; onClick: () => void }) {
  return (
    <>
      <div className={`lab truncate ${on ? "on" : ""}`} title={label}>{label}</div>
      <div className="track" onClick={onClick} title="Filtra la tabella"><i style={{ width: `${(100 * c) / max}%`, background: color }} /></div>
      <div className="v"><b>{c}</b> · {n ? pct(c / n, 0) : "–"}</div>
    </>
  );
}

function NoulView({ counts, n, rows, qid, filter, onFilter }: {
  counts: Record<string, number>; n: number; rows: ResultRow[]; qid: string; filter: string | null; onFilter: (l: string | null) => void }) {
  // donut + histogram of p(yes): shows how sure the model is, not only the majority
  const yes = n ? counts["sì"] / n : 0;
  const bins = Array(10).fill(0);
  for (const r of rows) {
    const p = valueOf(r, qid);
    if (p != null) bins[Math.min(9, Math.floor(p * 10))]++;
  }
  const bmax = Math.max(1, ...bins);
  const R = 42, C = 2 * Math.PI * R;
  return (
    <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 18, alignItems: "center" }}>
      <svg viewBox="0 0 110 110" width="110" height="110">
        <circle cx="55" cy="55" r={R} fill="none" stroke="var(--bar)" strokeWidth="14" />
        <circle cx="55" cy="55" r={R} fill="none" stroke="var(--violet)" strokeWidth="14" strokeDasharray={`${C * yes} ${C}`}
                transform="rotate(-90 55 55)" style={{ transition: "stroke-dasharray .5s", cursor: "pointer" }} onClick={() => onFilter(filter === "sì" ? null : "sì")} />
        <text x="55" y="52" textAnchor="middle" fill="var(--txt)" fontSize="19" fontFamily="var(--serif)">{counts["sì"]}</text>
        <text x="55" y="69" textAnchor="middle" fill="var(--dim)" fontSize="10">sì su {n}</text>
      </svg>
      <div>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 3, height: 74 }}>
          {bins.map((b, i) => (
            <div key={i} title={`p(sì) ${i / 10}–${(i + 1) / 10}: ${b}`}
                 style={{ flex: 1, height: `${Math.max(2, (100 * b) / bmax)}%`, borderRadius: 3, transition: "height .5s",
                          background: i >= 5 ? "var(--violet)" : "var(--dim2)", opacity: 0.35 + 0.065 * Math.abs(i - 4.5) * 2 }} />
          ))}
        </div>
        <div className="row small dim" style={{ justifyContent: "space-between", marginTop: 4 }}><span>sicuro no</span><span>incerto</span><span>sicuro sì</span></div>
        <div className="row" style={{ marginTop: 6 }}>
          <button className={`sm ${filter === "sì" ? "primary" : ""}`} onClick={() => onFilter(filter === "sì" ? null : "sì")}>solo sì</button>
          <button className={`sm ${filter === "no" ? "primary" : ""}`} onClick={() => onFilter(filter === "no" ? null : "no")}>solo no</button>
        </div>
      </div>
    </div>
  );
}

function Columns({ labels, counts, n, filter, onFilter }: { labels: string[]; counts: Record<string, number>; n: number; filter: string | null; onFilter: (l: string | null) => void }) {
  const max = Math.max(1, ...Object.values(counts));
  return (
    <div style={{ display: "grid", gridTemplateColumns: `repeat(${labels.length}, 1fr)`, gap: 8, alignItems: "end" }}>
      {labels.map((l, i) => (
        <div key={l} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, cursor: "pointer" }} onClick={() => onFilter(filter === l ? null : l)}>
          <span className="small num" style={{ color: "var(--txt)" }}>{counts[l]}</span>
          <div style={{ width: "100%", height: 84, display: "flex", alignItems: "flex-end" }}>
            <div style={{ width: "100%", height: `${Math.max(2, (100 * counts[l]) / max)}%`, borderRadius: "5px 5px 2px 2px", transition: "height .5s",
                          background: `color-mix(in srgb, var(--clay) ${Math.round((100 * i) / Math.max(1, labels.length - 1))}%, var(--violet))`,
                          outline: filter === l ? "2px solid var(--txt)" : "none" }} />
          </div>
          <span className="small dim truncate" style={{ maxWidth: "100%" }} title={l}>{l}</span>
          <span className="small dim2">{n ? pct(counts[l] / n, 0) : ""}</span>
        </div>
      ))}
    </div>
  );
}

// -------------------------------------------------------------------------------- grouped by a hidden field
export function GroupMatrix({ qid, q, rows, field }: { qid: string; q: Question; rows: ResultRow[]; field: string }) {
  const labels = labelsOf(q);
  const groups = new Map<string, { n: number; c: Record<string, number>; sum: number }>();
  for (const r of rows) {
    const g = String((r.meta || {})[field] ?? "—");
    const l = labelOf(q, r, qid);
    if (l == null) continue;
    if (!groups.has(g)) groups.set(g, { n: 0, c: {}, sum: 0 });
    const x = groups.get(g)!;
    x.n++;
    x.c[l] = (x.c[l] || 0) + 1;
    x.sum += valueOf(r, qid) ?? 0;
  }
  const order = [...groups.keys()].sort((a, b) => levelRank(a) - levelRank(b) || groups.get(b)!.n - groups.get(a)!.n);
  const showMean = q.type !== "choice";
  return (
    <div className="card chart-card">
      <div className="chart-head">
        <b>{qid}</b><span className="dim small">per <span className="mono">{field}</span> · % di ogni riga</span>
      </div>
      <div className="matrix" style={{ gridTemplateColumns: `minmax(90px, 160px) repeat(${labels.length}, 1fr) ${showMean ? "70px" : ""}` }}>
        <span />
        {labels.map(l => <span key={l} className="h" title={l}>{l}</span>)}
        {showMean && <span className="h">{q.type === "noul" ? "p(sì)" : "media"}</span>}
        {order.map(g => {
          const x = groups.get(g)!;
          return (
            <Frag key={g}>
              <span className="rl" title={g}>{g} <span className="dim2">({x.n})</span></span>
              {labels.map(l => {
                const f = (x.c[l] || 0) / x.n;
                return <span key={l} className="c" style={{ background: `color-mix(in srgb, var(--violet) ${Math.round(f * 85)}%, var(--card2))` }}>{pct(f, 0)}</span>;
              })}
              {showMean && <span className="c num" style={{ color: "var(--clay)", fontWeight: 600 }}>{q.type === "noul" ? pct(x.sum / x.n, 0) : (x.sum / x.n).toFixed(2)}</span>}
            </Frag>
          );
        })}
      </div>
    </div>
  );
}
const Frag = ({ children }: { children: ReactNode }) => <>{children}</>;
const LEVELS = ["low", "bassa", "basso", "medium", "media", "medio", "high", "alta", "alto", "critical", "critica"];
const levelRank = (s: string) => { const i = LEVELS.indexOf(s.toLowerCase()); return i < 0 ? 99 : i; };
