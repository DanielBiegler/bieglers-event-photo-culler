import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ScanResult } from "./types";

export async function pickFolder(): Promise<string | null> {
  const res = await open({ directory: true, multiple: false });
  return typeof res === "string" ? res : null;
}

export function scanFolder(folder: string): Promise<ScanResult> {
  return invoke<ScanResult>("scan_folder", { folder });
}

// Module-level cache so each thumbnail is only fetched once per session.
const thumbCache = new Map<string, Promise<string | null>>();

export function getThumbnail(path: string): Promise<string | null> {
  let p = thumbCache.get(path);
  if (!p) {
    p = invoke<string | null>("get_thumbnail", { path });
    thumbCache.set(path, p);
  }
  return p;
}

export function saveSidecar(folder: string, contents: string): Promise<void> {
  return invoke("save_sidecar", { folder, contents });
}

export async function pickExportPath(defaultName: string): Promise<string | null> {
  const res = await save({
    defaultPath: defaultName,
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  return res ?? null;
}

export function exportCsv(dest: string, contents: string): Promise<void> {
  return invoke("export_csv", { dest, contents });
}

export function copyImage(path: string): Promise<void> {
  return invoke("copy_image", { path });
}

/** Asset-protocol URL the webview can load a full-res image from. */
export function imageSrc(path: string): string {
  return convertFileSrc(path);
}
