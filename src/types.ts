export interface ImageEntry {
  name: string;
  path: string;
  /** Epoch seconds (EXIF DateTimeOriginal, or file mtime fallback). */
  captureTime: number;
  captureSource: "exif" | "mtime";
}

export interface Rating {
  stars: number; // 0-5
  reject: boolean;
}

export interface ScanResult {
  folder: string;
  images: ImageEntry[];
  sidecar: string | null;
}

export type RatingsMap = Record<string, Rating>;

export type View = "loupe" | "grid";
export type RejectFilter = "all" | "hide" | "only";
/** How the star filter compares: at-least, exactly, or less-than. */
export type StarFilterMode = "gte" | "eq" | "lt";

/** Persisted sidecar shape (.cull.json at the folder root). */
export interface Sidecar {
  version: 1;
  files: RatingsMap;
}
