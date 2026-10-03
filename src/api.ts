// Client of the FERMI engine (src-tauri/src/engine), on 127.0.0.1. In the desktop app the port comes from Tauri.
import { engineInfo } from "./native";

export type QType = "noul" | "choice" | "score";
export interface Question {
  type: QType;
  instructions: string;
  criteria?: Record<string, string> | string[] | { true?: string; false?: string };
}
export type Questions = Record<string, Question>;

export interface NoulAnswer { type: "noul"; answer: "yes" | "no"; p_yes: number }
export interface ChoiceAnswer { type: "choice"; answer: string; probabilities: Record<string, number>; orders?: number }
export interface ScoreAnswer { type: "score"; answer: number; answer_text: string; probabilities: number[]; expected: number; expected_01: number }
export type Answer = NoulAnswer | ChoiceAnswer | ScoreAnswer;
export type Answers = Record<string, Answer>;

export interface ModelEntry {
  id: string; name: string; size: string; path: string; source: "downloaded" | "added" | string; file_bytes: number; quant: string;
  base_model: string; training_step?: number; status: "unloaded" | "loading" | "ready" | "error"; error?: string | null;
  note?: string | null; vram_bytes?: number | null; load_seconds?: number | null; device?: string | null;
  evaluation: { agreement?: number; cross_entropy?: number; reference?: number; ece?: number;
                by_type?: Record<string, number | undefined> };
}
export interface ServerStatus { enabled: boolean; host: string; port: number; key: boolean; error: string | null; lan: string[]; url: string }
export interface Download { state: "running" | "done" | "error" | "cancelled"; done: number; total: number; speed: number; error: string | null }
export interface CatalogEntry {
  id: string; name: string; size: string; bytes: number; memory: number; description: string; url: string;
  installed: boolean; fits_gpu: boolean | null; download: Download | null;
}
export interface Device { index: number; name: string; description: string; kind: "cpu" | "gpu" | "igpu" | "other"; free: number; total: number }
export interface RuntimeStatus { ok: boolean; error: string | null; dir: string | null; devices: Device[] }
export interface Status {
  engine: string; uptime: number; data_dir: string; models: ModelEntry[]; default: string | null; catalog: CatalogEntry[];
  runtime: RuntimeStatus; server: ServerStatus; counter: { requests: number; questions: number }; active_jobs: Job[];
}
export interface SystemInfo {
  engine: string; platform: string; arch: string; cpu: string | null; threads: number;
  gpu: { name: string; vram_total: number; vram_free: number; vram_used: number } | null;
  ram: { total: number; available: number }; devices: Device[]; runtime: RuntimeStatus;
}
export interface EngineSettings { device: string; threads: number; default_model: string | null; autoload: string[];
                                  question_sets: QuestionSet[]; server: { api_key: string } }
export interface JobSpec {
  name?: string; model?: string; orders?: "none" | "reverse";
  source: { folder: string; recursive?: boolean; extensions?: string[]; limit?: number | null };
  hidden?: string[]; questions: Questions;
}
export interface Job {
  id: string; name: string; created: number; spec: JobSpec; status: "queued" | "running" | "paused" | "done" | "error" | "interrupted";
  total: number | null; done: number; errors: number; started: number | null; finished: number | null; elapsed: number;
  error: string | null; model_name: string; rate: number | null; eta: number | null;
}
export interface ResultRow { sid: string; meta: Record<string, unknown>; text: string; answers?: Answers; ms?: number; error?: string }
export interface ScanField { name: string; present: number; avg_len: number; distinct: number | null; examples: string[]; label_like: boolean }
export interface ScanResult { files: number; states: number; by_ext: Record<string, number>; sample: { id: string; text: string }[];
                              fields: ScanField[]; complete: boolean }
export interface RequestLog { t: number; server: string; client: string; method: string; path: string; status: number; ms: number; model: string | null; questions: number }

let base: string | null = null;
export async function engineUrl(): Promise<string> {
  if (base) return base;
  const info = await engineInfo();
  base = `http://127.0.0.1:${info?.port ?? 8767}`;
  return base;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const url = (await engineUrl()) + path;
  const r = await fetch(url, {
    method, headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const txt = await r.text();
  let j: unknown = null;
  try { j = JSON.parse(txt); } catch { j = txt; }
  if (!r.ok) throw new Error((j as { error?: string })?.error || r.statusText);
  return j as T;
}

export const api = {
  status: () => call<Status>("GET", "/api/status"),
  system: () => call<SystemInfo>("GET", "/api/system"),
  classify: (req: { model?: string; state: unknown; questions: Questions; orders?: string }) =>
    call<{ answers: Answers; ms: number; model: string }>("POST", "/api/classify", req),
  load: (id: string) => call<ModelEntry>("POST", "/api/models/load", { id }),
  unload: (id: string) => call<ModelEntry>("POST", "/api/models/unload", { id }),
  setDefault: (id: string) => call<{ default: string }>("POST", "/api/models/default", { id }),
  addModel: (path: string) => call<ModelEntry>("POST", "/api/models/add", { path }),
  removeModel: (id: string) => call<unknown>("POST", "/api/models/remove", { id }),
  discover: () => call<{ added: number }>("POST", "/api/models/discover", {}),
  download: (id: string) => call<{ started: boolean }>("POST", "/api/models/download", { id }),
  cancelDownload: (id: string) => call<unknown>("POST", "/api/models/download/cancel", { id }),
  devices: () => call<{ devices: Device[] }>("GET", "/api/devices"),
  settings: () => call<EngineSettings>("GET", "/api/settings"),
  saveSettings: (s: Record<string, unknown>) => call<unknown>("POST", "/api/settings", s),
  server: (s: Partial<{ enabled: boolean; host: string; port: number; api_key: string }>) => call<ServerStatus>("POST", "/api/server", s),
  requests: (since: number) => call<{ requests: RequestLog[] }>("GET", `/api/requests?since=${since}`),
  scan: (source: JobSpec["source"]) => call<ScanResult>("POST", "/api/scan", source),
  jobs: () => call<{ jobs: Job[] }>("GET", "/api/jobs"),
  createJob: (spec: JobSpec) => call<Job>("POST", "/api/jobs", spec),
  results: (id: string, since: number) => call<{ job: Job; results: ResultRow[] }>("GET", `/api/jobs/${id}/results?since=${since}`),
  stopJob: (id: string) => call<Job>("POST", `/api/jobs/${id}/stop`, {}),
  resumeJob: (id: string) => call<Job>("POST", `/api/jobs/${id}/resume`, {}),
  deleteJob: (id: string) => call<unknown>("DELETE", `/api/jobs/${id}`),
  exportJob: (id: string, path: string, format: string) => call<{ path: string }>("POST", `/api/jobs/${id}/export`, { path, format }),
};

export interface QuestionSet { name: string; questions: Questions }

// ---------------------------------------------------------------------------------------------------------- helpers
export const pct = (p: number, d = 1) => (100 * p).toFixed(p >= 0.995 || p < 0.005 ? 0 : d) + "%";
export const fmtBytes = (b?: number | null) => b == null ? "–" : b >= 1e9 ? (b / 1e9).toFixed(1) + " GB" : (b / 1e6).toFixed(0) + " MB";
export const fmtDur = (s?: number | null) => {
  if (s == null || !isFinite(s)) return "–";
  s = Math.round(s);
  if (s < 60) return s + " s";
  if (s < 3600) return Math.floor(s / 60) + " min " + (s % 60) + " s";
  return Math.floor(s / 3600) + " h " + Math.floor((s % 3600) / 60) + " min";
};
export const TYPE_LABEL: Record<QType, string> = { noul: "sì / no", choice: "scelta", score: "scala" };

/** [[label, p], ...] for any answer */
export function answerItems(q: Question, a: Answer): [string, number][] {
  if (a.type === "noul") return [["sì", a.p_yes], ["no", 1 - a.p_yes]];
  if (a.type === "choice") return Object.entries(a.probabilities);
  const lv = (q.criteria as string[]) || [];
  return a.probabilities.map((p, i) => [lv[i] ?? String(i), p]);
}
/** the main answer as short text + its probability */
export function answerLabel(q: Question, a: Answer): [string, number] {
  if (a.type === "noul") return a.p_yes >= 0.5 ? ["sì", a.p_yes] : ["no", 1 - a.p_yes];
  if (a.type === "choice") return [a.answer, a.probabilities[a.answer]];
  const lv = (q.criteria as string[]) || [];
  return [shortLevel(lv[a.answer] ?? String(a.answer)), a.probabilities[a.answer]];
}
export const shortLevel = (s: string) => s.split(/[:;(]/)[0].trim();

/** clean the questions from the editor before sending (drops empty criteria) */
export function cleanQuestions(qs: Questions): Questions {
  const out: Questions = {};
  for (const [id, q] of Object.entries(qs)) {
    if (q.type === "noul") {
      const c = (q.criteria || {}) as { true?: string; false?: string };
      const crit: { true?: string; false?: string } = {};
      if (c.true?.trim()) crit.true = c.true.trim();
      if (c.false?.trim()) crit.false = c.false.trim();
      out[id] = { type: "noul", instructions: q.instructions.trim(), ...(Object.keys(crit).length ? { criteria: crit } : {}) };
    } else if (q.type === "choice") {
      const c = (q.criteria || {}) as Record<string, string>;
      out[id] = { type: "choice", instructions: q.instructions.trim(),
                  criteria: Object.fromEntries(Object.entries(c).filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), (v || "").trim()])) };
    } else {
      out[id] = { type: "score", instructions: q.instructions.trim(), criteria: ((q.criteria as string[]) || []).map(s => s.trim()).filter(Boolean) };
    }
  }
  return out;
}

/** parse the state box: JSON when it is valid JSON (object or array), otherwise plain text */
export function parseState(s: string): { state: unknown; json: boolean } {
  const t = s.trim();
  if (t.startsWith("{") || t.startsWith("[")) {
    try { return { state: JSON.parse(t), json: true }; } catch { /* plain text */ }
  }
  return { state: s, json: false };
}

export const PALETTE = ["var(--violet)", "var(--clay)", "var(--sky)", "var(--olive)", "var(--kraft)", "var(--rose)", "var(--teal)", "var(--sand)"];
