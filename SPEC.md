# Event Photo Culler — Specification (v1)

A personal desktop tool for quickly culling, rating, and assessing coverage of
event photo shoots (500–3000 images per event). Optimized for a single, fast,
seamless rating loop with zero animation.

## Workflow context

The photographer copies **only the JPEGs** from the SD card onto a
disk-constrained laptop, culls/rates them here, then later loads the matching
**RAW** files from the SD card to post-process. JPEG and RAW share a basename
(`IMG_1234.JPG` ↔ `IMG_1234.CR2`), so this app's job is to produce a clean
**keeper-filename list** that drives which RAWs to pull.

## Tech stack

- **Tauri** — Rust core + web frontend, native filesystem access, small binary.
- **Frontend:** React + TypeScript.
- **Grid virtualization:** render only visible thumbnail cells (e.g.
  `@tanstack/react-virtual`) — required for 500–3000 items without jank.
- **Rust crates:** EXIF parsing (e.g. `kamadak-exif`) for capture time +
  embedded thumbnail extraction. Full-image decode is handled by the webview.
- **Target OS:** Linux (primary). Cross-platform comes free with Tauri but is
  not a goal.

## Decode & memory pipeline

Two surfaces with deliberately different strategies. **No disk cache anywhere.**

### Loupe (single big image, the cull loop)
- Webview loads the full JPEG directly via Tauri's asset protocol
  (`convertFileSrc`) — native, GPU-accelerated browser decode.
- Maintain a **sliding window** of ±5–8 fully decoded "fit-to-screen" images in
  memory around the current index; evict outside the window. Cycling therefore
  hits already-decoded frames → zero perceived latency.

### Grid (overview / navigation)
- Rationale: a decoded 24MP image is ~96 MB in RAM (`w × h × 4`), so the grid
  must never hold full images.
- Source thumbnails from the **embedded EXIF thumbnail** (~160×120) baked into
  each camera JPEG — extracted in Rust in ~1 ms with no full decode. All thumbs
  held in memory (a few MB total). No generated thumbnails, no disk cache.
- Cells are small; slight softness if enlarged is accepted.

### Zoom
- Toggle **fit-to-screen ↔ 100%** (key `Q` / click) with pan, using the
  already-loaded full-resolution image, for critical-focus checks.

## Metadata

- **Capture time:** EXIF `DateTimeOriginal`, fallback to file mtime.
- **Default sort:** chronological ascending.

## Ratings & persistence

- Single **sidecar file per folder**: `.cull.json` at the folder root. Travels
  with the photos, easy to inspect/back up, no central state.
- Shape (illustrative):
  ```json
  {
    "version": 1,
    "files": {
      "IMG_1234.JPG": { "stars": 4, "reject": false },
      "IMG_1235.JPG": { "stars": 0, "reject": true }
    }
  }
  ```
- Writes are debounced and atomic (write temp + rename) to avoid corruption.
- **Reject** is a filterable flag only — it never touches files in v1.

## Rating model & interaction

Concise, icon-first UI that stays out of the way. No animations.

- `1`–`5` → set star rating; `0` → clear.
- **Auto-advance** to next image after rating (toggleable).
- `X` → toggle reject flag.
- `←` / `→` → navigate previous/next.
- `Q` → toggle fit ↔ 100% zoom; drag to pan when zoomed.
- `G` → grid view; `E` → loupe view (proposed; adjustable).
- Filtering: by star threshold (e.g. 3★+) and by flag (all / picks / rejects).

## Coverage timeline (signature feature)

- Horizontal **histogram**: photos binned by capture time; bar height = count of
  images **at/above a star threshold** controlled by its own slider (independent
  of the active grid filter).
- **Bin interval is user-configurable: 5 / 10 / 15 minutes** (switchable in-app;
  revisit later if these aren't granular enough).
- **Gaps** (empty/low bins) are visually highlighted so under-covered stretches
  of the event are obvious at a glance.
- Click a bin to **jump** to that point in the set. Current position indicated
  on the timeline.

## Finish / handoff

- **Export keeper filename list:** write a `.csv` of basenames at/above a chosen
  threshold, including the star rating per row (e.g. `IMG_1234.JPG,4`), used to
  locate the corresponding RAWs on the SD card.

## Folder scope

- Open **one flat folder** at a time; show the JPEGs directly within it.
- No subfolder recursion in v1.

## Non-goals (v1)

- RAW decoding / RAW handling of any kind.
- Compare / side-by-side burst view.
- Lightroom / Capture One / XMP interoperability.
- Deleting or moving image files (reject is flag-only).
- Subfolder recursion.
- On-disk thumbnail cache or generated thumbnails.

## Resolved details

- Timeline bin granularity: user-configurable 5 / 10 / 15 minutes.
- Export format: `.csv` with basename + star rating per row.
- Keybindings: defaults as listed above; trivially reconfigurable later.
