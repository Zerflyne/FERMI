// Bridge to the Tauri shell. In a plain browser (vite dev) every call has a fallback, so the UI can be developed
// and tested against a running engine without the desktop shell.
import { invoke } from "@tauri-apps/api/core";

export const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface EngineInfo { port: number; running: boolean; error: string | null; data_dir: string; runtime_dir: string; version: string }
export interface AppSettings { close_to_tray: boolean; start_hidden: boolean }

export async function engineInfo(): Promise<EngineInfo | null> {
  if (!isTauri()) return null;
  try { return await invoke<EngineInfo>("engine_info"); } catch { return null; }
}
export async function engineLog(): Promise<string[]> {
  if (!isTauri()) return ["(il registro del motore è visibile solo nell'app desktop)"];
  return invoke<string[]>("engine_log");
}
export async function getAppSettings(): Promise<AppSettings | null> {
  if (!isTauri()) return null;
  return invoke<AppSettings>("get_settings");
}
export async function setAppSettings(s: AppSettings): Promise<AppSettings | null> {
  if (!isTauri()) return null;
  return invoke<AppSettings>("set_settings", { settings: s });
}
export async function openPath(path: string) {
  if (isTauri()) return invoke("open_path", { path });
  window.open(path.startsWith("http") ? path : "file://" + path, "_blank");
}

export async function pickFolder(title: string, start?: string): Promise<string | null> {
  if (!isTauri()) return window.prompt(title + " (percorso completo)", start || "") || null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: false, title, defaultPath: start || undefined });
  return typeof r === "string" ? r : null;
}
export async function pickFile(title: string, ext?: string): Promise<string | null> {
  if (!isTauri()) return window.prompt(title + " (percorso completo)") || null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: false, multiple: false, title,
                         filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : undefined });
  return typeof r === "string" ? r : null;
}
export async function pickSave(title: string, name: string, ext: string): Promise<string | null> {
  if (!isTauri()) return window.prompt(title + " (percorso completo)", "~/" + name) || null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return await save({ title, defaultPath: name, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
}
