import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  pickFolder,
  scanFolder,
  saveSidecar,
  pickExportPath,
  exportCsv,
  copyImage,
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
import Toast, { type ToastMsg } from "./components/Toast";

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

  // Transient bottom-left notification (fades in/out). A new id retriggers it
  // even when the text is identical (e.g. pressing N twice).
  const [toast, setToast] = useState<ToastMsg | null>(null);
  const toastId = useRef(0);
  const notify = useCallback((text: string) => {
    setToast({ id: ++toastId.current, text });
  }, []);

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
      if (
        starFilterMode === "eq"
          ? stars !== minStars
          : starFilterMode === "lt"
            ? stars >= minStars
            : stars < minStars
      )
        return false;
      if (rejectFilter === "hide" && reject) return false;
      if (rejectFilter === "only" && !reject) return false;
      return true;
    },
    [ratings, minStars, starFilterMode, rejectFilter]
  );

  const filtered = useMemo(() => images.filter(passes), [images, passes]);
  const reviewedCount = useMemo(
    () =>
      images.reduce((n, i) => {
        const r = ratings[i.name];
        return n + ((r?.stars ?? 0) > 0 || r?.reject ? 1 : 0);
      }, 0),
    [images, ratings]
  );
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

  // Jump to the beginning of the adjacent non-empty time-bin (Ctrl+←/→).
  const goBin = useCallback(
    (dir: number) => {
      const n = images.length;
      if (n === 0) return;
      const start = images[0].captureTime;
      const binSec = binMinutes * 60;
      const binOf = (ct: number) => Math.floor((ct - start) / binSec);
      let i = images.findIndex((im) => im.name === currentName);
      if (i < 0) i = 0;
      const curBin = binOf(images[i].captureTime);
      if (dir > 0) {
        for (let j = i + 1; j < n; j++) {
          if (binOf(images[j].captureTime) > curBin) {
            setCurrentName(images[j].name);
            return;
          }
        }
        // Past the last bin: wrap to the first image of the first bin.
        setCurrentName(images[0].name);
      } else {
        // Step back into an earlier bin, then to that bin's first image.
        let j = i - 1;
        while (j >= 0 && binOf(images[j].captureTime) >= curBin) j--;
        if (j >= 0) {
          const prevBin = binOf(images[j].captureTime);
          while (j - 1 >= 0 && binOf(images[j - 1].captureTime) === prevBin) j--;
          setCurrentName(images[j].name);
          return;
        }
        // Before the first bin: wrap to the first image of the last bin.
        const lastBin = binOf(images[n - 1].captureTime);
        let k = n - 1;
        while (k - 1 >= 0 && binOf(images[k - 1].captureTime) === lastBin) k--;
        setCurrentName(images[k].name);
      }
    },
    [images, currentName, binMinutes]
  );

  // Jump to the next unrated image (no stars, not rejected) to resume culling.
  // Ignores the active filter on purpose — the point is to find work to do even
  // when the view is filtered to picks/rejects. Scans the full list in `dir`
  // (+1 forward, -1 backward) and wraps once; the bounded loop (at most n steps)
  // cannot spin forever, and it reports the outcome via the notification system
  // either way.
  const goNextUnrated = useCallback(
    (dir: 1 | -1 = 1) => {
      const n = images.length;
      if (n === 0) return;
      const i = images.findIndex((im) => im.name === currentName); // -1 → start at 0
      for (let step = 1; step <= n; step++) {
        const j = (((i + dir * step) % n) + n) % n;
        const r = ratings[images[j].name];
        if ((r?.stars ?? 0) === 0 && !r?.reject) {
          setCurrentName(images[j].name);
          setView("loupe");
          setZoom(false);
          const label = dir === 1 ? "Next unrated" : "Prev unrated";
          notify(`${label} · ${images[j].name} (${j + 1}/${n})`);
          return;
        }
      }
      notify("No unrated images left — all reviewed");
    },
    [images, currentName, ratings, notify]
  );

  const setStars = useCallback(
    (name: string, n: number) => {
      const prevStars = ratings[name]?.stars ?? 0;
      setRatings((prev) => {
        const cur = prev[name] ?? { stars: 0, reject: false };
        return { ...prev, [name]: { ...cur, stars: n } };
      });
      // Notify only when actually clearing an existing rating (0 on a blank
      // image is a no-op the user doesn't need to be told about).
      if (n === 0 && prevStars > 0) notify(`Cleared rating · ${name}`);
      if (autoAdvance && n > 0 && view === "loupe" && name === currentName) go(1);
    },
    [ratings, autoAdvance, view, currentName, go, notify]
  );

  const toggleReject = useCallback(
    (name: string) => {
      const nowRejected = !(ratings[name]?.reject ?? false);
      setRatings((prev) => {
        const cur = prev[name] ?? { stars: 0, reject: false };
        return { ...prev, [name]: { ...cur, reject: nowRejected } };
      });
      if (!nowRejected) notify(`Cleared reject · ${name}`);
      if (autoAdvance && view === "loupe" && name === currentName) {
        // advance only when flagging (not when clearing) so review stays put
        setTimeout(() => nowRejected && go(1), 0);
      }
    },
    [ratings, autoAdvance, view, currentName, go, notify]
  );

  const jump = useCallback((name: string) => {
    setCurrentName(name);
    setView("loupe");
    setZoom(false);
  }, []);

  const copyCurrent = useCallback(() => {
    const img = images.find((i) => i.name === currentName);
    if (!img) return;
    copyImage(img.path)
      .then(() => notify(`Copied · ${img.name}`))
      .catch(console.error);
  }, [images, currentName, notify]);

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

      if ((e.ctrlKey || e.metaKey) && (e.key === "c" || e.key === "C")) {
        copyCurrent();
        e.preventDefault();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "ArrowRight") {
        goBin(1);
        e.preventDefault();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "ArrowLeft") {
        goBin(-1);
        e.preventDefault();
        return;
      }
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
      } else if (e.key === "n" || e.key === "N") {
        goNextUnrated(e.shiftKey ? -1 : 1);
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [currentName, setStars, toggleReject, go, goBin, goNextUnrated, view, showHelp, zoom, copyCurrent]);

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
            onCopy={copyCurrent}
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
          <span className="counts" title="Reviewed (rated or rejected) / total">
            ✓ {reviewedCount}/{images.length} (
            {images.length ? Math.round((reviewedCount / images.length) * 100) : 0}%)
          </span>
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

      <Toast toast={toast} />

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}
