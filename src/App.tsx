import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  pickFolder,
  scanFolder,
  saveSidecar,
  pickExportPath,
  exportCsv,
} from "./api";
import type {
  ImageEntry,
  RatingsMap,
  RejectFilter,
  Sidecar,
  StarFilterMode,
  View,
} from "./types";
import Toolbar from "./components/Toolbar";
import Loupe from "./components/Loupe";
import Grid from "./components/Grid";
import Timeline from "./components/Timeline";
import HelpOverlay from "./components/HelpOverlay";

function basename(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}

// Drop default/empty entries so the sidecar stays small.
function pruneRatings(r: RatingsMap): RatingsMap {
  const out: RatingsMap = {};
  for (const [name, v] of Object.entries(r)) {
    if (v.stars > 0 || v.reject) out[name] = v;
  }
  return out;
}

// Per-folder memory of the last-viewed image, keyed by folder path.
function readLastImages(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem("lastImages") || "{}");
  } catch {
    return {};
  }
}
function writeLastImage(folder: string, name: string) {
  try {
    const m = readLastImages();
    m[folder] = name;
    localStorage.setItem("lastImages", JSON.stringify(m));
  } catch {
    /* ignore storage errors */
  }
}

export default function App() {
  const [folder, setFolder] = useState<string | null>(null);
  const [images, setImages] = useState<ImageEntry[]>([]);
  const [ratings, setRatings] = useState<RatingsMap>({});
  const [view, setView] = useState<View>("loupe");
  const [currentName, setCurrentName] = useState<string | null>(null);
  const [zoom, setZoom] = useState(false);
  const [showHelp, setShowHelp] = useState(false);

  // Filters
  const [minStars, setMinStars] = useState(0);
  const [starFilterMode, setStarFilterMode] = useState<StarFilterMode>("gte");
  const [rejectFilter, setRejectFilter] = useState<RejectFilter>("all");
  const [autoAdvance, setAutoAdvance] = useState(true);

  // Timeline / keeper threshold (shared with export)
  const [binMinutes, setBinMinutes] = useState(10);
  const [keeperThreshold, setKeeperThreshold] = useState(3);

  const skipNextSave = useRef(false);

  const passes = useCallback(
    (img: ImageEntry) => {
      const r = ratings[img.name];
      const stars = r?.stars ?? 0;
      const reject = r?.reject ?? false;
      if (starFilterMode === "eq" ? stars !== minStars : stars < minStars) return false;
      if (rejectFilter === "hide" && reject) return false;
      if (rejectFilter === "only" && !reject) return false;
      return true;
    },
    [ratings, minStars, starFilterMode, rejectFilter]
  );

  const filtered = useMemo(() => images.filter(passes), [images, passes]);
  const currentIndex = useMemo(
    () => images.findIndex((i) => i.name === currentName),
    [images, currentName]
  );

  // --- Folder open / scan ---
  const loadFolder = useCallback(async (path: string) => {
    let res;
    try {
      res = await scanFolder(path);
    } catch (e) {
      // Folder gone (e.g. SD card unplugged) — don't crash startup.
      console.error("Failed to open folder", path, e);
      return;
    }
    let loaded: RatingsMap = {};
    if (res.sidecar) {
      try {
        const parsed = JSON.parse(res.sidecar) as Sidecar;
        if (parsed && parsed.files) loaded = parsed.files;
      } catch {
        /* ignore malformed sidecar */
      }
    }
    // Restore the last-viewed image for this folder if it still exists.
    const remembered = readLastImages()[res.folder];
    const initial =
      remembered && res.images.some((i) => i.name === remembered)
        ? remembered
        : res.images[0]?.name ?? null;

    skipNextSave.current = true;
    setFolder(res.folder);
    setImages(res.images);
    setRatings(loaded);
    setCurrentName(initial);
    setView("loupe");
    setZoom(false);
    try {
      localStorage.setItem("lastFolder", res.folder);
    } catch {
      /* ignore storage errors */
    }
  }, []);

  const openFolder = useCallback(async () => {
    const picked = await pickFolder();
    if (picked) await loadFolder(picked);
  }, [loadFolder]);

  // Re-open the last folder on startup.
  useEffect(() => {
    const last = localStorage.getItem("lastFolder");
    if (last) loadFolder(last);
  }, [loadFolder]);

  // Remember the current image (per folder) as you navigate.
  useEffect(() => {
    if (folder && currentName) writeLastImage(folder, currentName);
  }, [folder, currentName]);

  // --- Persistence (debounced) ---
  useEffect(() => {
    if (!folder) return;
    if (skipNextSave.current) {
      skipNextSave.current = false;
      return;
    }
    const id = setTimeout(() => {
      const sidecar: Sidecar = { version: 1, files: pruneRatings(ratings) };
      saveSidecar(folder, JSON.stringify(sidecar)).catch(console.error);
    }, 600);
    return () => clearTimeout(id);
  }, [ratings, folder]);

  // --- Navigation (skips filtered-out images, wraps around the ends) ---
  const go = useCallback(
    (delta: number) => {
      const n = images.length;
      if (n === 0) return;
      let i = images.findIndex((im) => im.name === currentName);
      if (i < 0) i = delta > 0 ? -1 : 0;
      for (let step = 1; step <= n; step++) {
        const j = (((i + delta * step) % n) + n) % n;
        if (passes(images[j])) {
          setCurrentName(images[j].name);
          return;
        }
      }
    },
    [images, currentName, passes]
  );

  const setStars = useCallback(
    (name: string, n: number) => {
      setRatings((prev) => {
        const cur = prev[name] ?? { stars: 0, reject: false };
        return { ...prev, [name]: { ...cur, stars: n } };
      });
      if (autoAdvance && n > 0 && view === "loupe" && name === currentName) go(1);
    },
    [autoAdvance, view, currentName, go]
  );

  const toggleReject = useCallback(
    (name: string) => {
      let nowRejected = false;
      setRatings((prev) => {
        const cur = prev[name] ?? { stars: 0, reject: false };
        nowRejected = !cur.reject;
        return { ...prev, [name]: { ...cur, reject: nowRejected } };
      });
      if (autoAdvance && view === "loupe" && name === currentName) {
        // advance only when flagging (not when clearing) so review stays put
        setTimeout(() => nowRejected && go(1), 0);
      }
    },
    [autoAdvance, view, currentName, go]
  );

  const jump = useCallback((name: string) => {
    setCurrentName(name);
    setView("loupe");
    setZoom(false);
  }, []);

  // --- Export keepers (stars >= keeperThreshold) ---
  const onExport = useCallback(async () => {
    if (!folder) return;
    const keepers = images.filter((i) => (ratings[i.name]?.stars ?? 0) >= keeperThreshold);
    const lines = ["filename,stars"];
    for (const k of keepers) lines.push(`${k.name},${ratings[k.name]?.stars ?? 0}`);
    const dest = await pickExportPath(`${basename(folder)}-keepers.csv`);
    if (!dest) return;
    await exportCsv(dest, lines.join("\n") + "\n");
  }, [folder, images, ratings, keeperThreshold]);

  // --- Keyboard ---
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      if (e.key === "?") {
        setShowHelp((s) => !s);
        return;
      }
      if (e.key === "Escape") {
        if (showHelp) setShowHelp(false);
        else if (zoom) setZoom(false);
        return;
      }
      if (showHelp) return; // swallow other shortcuts while help is open

      if (e.key >= "0" && e.key <= "5") {
        if (currentName) setStars(currentName, Number(e.key));
        e.preventDefault();
      } else if (e.key === "x" || e.key === "X") {
        if (currentName) toggleReject(currentName);
      } else if (e.key === "ArrowRight") {
        go(1);
        e.preventDefault();
      } else if (e.key === "ArrowLeft") {
        go(-1);
        e.preventDefault();
      } else if (e.key === "q" || e.key === "Q") {
        if (view === "loupe") setZoom((z) => !z);
      } else if (e.key === "g" || e.key === "G") {
        setView("grid");
      } else if (e.key === "e" || e.key === "E") {
        setView("loupe");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [currentName, setStars, toggleReject, go, view, showHelp, zoom]);

  const currentRating = currentName ? ratings[currentName] : undefined;

  return (
    <div className="app">
      <Toolbar
        folderName={folder ? basename(folder) : null}
        view={view}
        minStars={minStars}
        starFilterMode={starFilterMode}
        rejectFilter={rejectFilter}
        autoAdvance={autoAdvance}
        shownCount={filtered.length}
        totalCount={images.length}
        position={currentIndex >= 0 ? String(currentIndex + 1) : "–"}
        onOpen={openFolder}
        onView={setView}
        onMinStars={setMinStars}
        onStarFilterMode={setStarFilterMode}
        onRejectFilter={setRejectFilter}
        onAutoAdvance={setAutoAdvance}
        onExport={onExport}
        onHelp={() => setShowHelp(true)}
      />

      <div className="main">
        {images.length === 0 ? (
          <div className="empty-state">
            <button className="big-open" onClick={openFolder}>
              Open a folder of JPEGs
            </button>
          </div>
        ) : view === "loupe" ? (
          <Loupe
            images={images}
            index={currentIndex}
            zoom={zoom}
            onToggleZoom={() => setZoom((z) => !z)}
          />
        ) : (
          <Grid
            images={filtered}
            ratings={ratings}
            currentName={currentName}
            onSelect={setCurrentName}
            onOpen={jump}
          />
        )}
      </div>

      {view === "loupe" && currentName && (
        <div className="loupe-hud">
          <div className="hud-stars">
            {[1, 2, 3, 4, 5].map((n) => (
              <span
                key={n}
                className={`hud-star ${(currentRating?.stars ?? 0) >= n ? "on" : ""}`}
                onClick={() => setStars(currentName, n)}
              >
                ★
              </span>
            ))}
          </div>
          <span
            className={`hud-reject ${currentRating?.reject ? "on" : ""}`}
            title="Reject (X)"
            onClick={() => toggleReject(currentName)}
          >
            ✕
          </span>
          <span
            className={`hud-zoom ${zoom ? "on" : ""}`}
            title="Zoom 1.5× (Q or click image)"
            onClick={() => setZoom((z) => !z)}
          >
            1.5×
          </span>
          <span className="hud-name">{currentName}</span>
        </div>
      )}

      {images.length > 0 && (
        <Timeline
          images={images}
          ratings={ratings}
          binMinutes={binMinutes}
          threshold={keeperThreshold}
          currentName={currentName}
          onJump={jump}
          onBinMinutes={setBinMinutes}
          onThreshold={setKeeperThreshold}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}
