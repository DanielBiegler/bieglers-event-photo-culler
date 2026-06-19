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

- **Native egui** (Rust, `eframe` glow backend) — single binary, no webview.
  (Originally Tauri 2 + React; rewritten to egui for performance + memory
  determinism. See CLAUDE.md.)
- **Grid virtualization:** render only visible rows (`egui::ScrollArea::show_rows`)
  — required for 500–3000 items without jank.
- **Rust crates:** `kamadak-exif` (capture time + orientation + embedded
  thumbnail), `image` (JPEG decode), `eframe`/`egui`, `rayon`, `rfd`, `arboard`,
  `directories`, `serde`.
- **Target OS:** Linux (primary). Cross-platform comes free with winit but is
  not a goal.

## Decode & memory pipeline

Two surfaces with deliberately different strategies. **No disk cache anywhere.**
JPEGs are decoded off the UI thread in `loader.rs`; the UI uploads GPU textures
it owns and explicitly evicts. EXIF orientation is applied at decode time.

### Loupe (single big image, the cull loop)
- Full JPEGs are decoded by a background worker pool and uploaded as GPU
  textures.
- Maintain a **sliding window** of ±`WINDOW` (≈6) full-res textures around the
  current index; evict outside the window (`maintain_window`). Cycling hits
  already-resident frames → zero perceived latency, and VRAM stays bounded.

### Grid (overview / navigation)
- Rationale: a decoded 24MP image is ~96 MB in RAM (`w × h × 4`), so the grid
  must never hold full images.
- Source thumbnails from the **embedded EXIF thumbnail** (~160×120) baked into
  each camera JPEG — extracted in Rust with no full decode, decoded off-thread,
  and held as small GPU textures created on demand. No generated thumbnails, no
  disk cache.
- Cells are small; slight softness if enlarged is accepted.

### Zoom
- Toggle **fit-to-screen ↔ 1.5×** (key `Q` / click) with pan, using the
  already-resident full-resolution texture, for critical-focus checks.

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
- `Q` → toggle fit ↔ 1.5× zoom; drag to pan when zoomed.
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

Pick a destination folder; the app writes two deliverables for every keeper
(image at/above the chosen threshold):

- **Keeper list (`keepers.txt`):** one **stem** per line — basename without
  extension (e.g. `IMG_1234`). No header, no rating column. Stems compose with
  any RAW extension, so scripts append `.CR2`/`.NEF`/… to locate the matching
  RAWs on the SD card.
- **XMP sidecars (`<stem>.xmp`):** a minimal Lightroom/darktable-compatible
  sidecar carrying the star rating (`xmp:Rating`). The RAW extension is unknown
  here, so sidecars use the Lightroom basename form (`IMG_1234.xmp`), which
  darktable also reads. Dropping a sidecar next to its RAW (same basename)
  transfers the rating into darktable/Lightroom. **Export only** — we never read
  XMP back.

## Folder scope

- Open **one flat folder** at a time; show the JPEGs directly within it.
- No subfolder recursion in v1.

## Non-goals (v1)

- RAW decoding / RAW handling of any kind.
- Compare / side-by-side burst view.
- Reading XMP / Lightroom / Capture One metadata back in (we *write* rating-only
  XMP sidecars on export, but never import them).
- Deleting or moving image files (reject is flag-only).
- Subfolder recursion.
- On-disk thumbnail cache or generated thumbnails.

## Resolved details

- Timeline bin granularity: user-configurable 5 / 10 / 15 minutes.
- Export format: a destination folder holding `keepers.txt` (one stem per line)
  + one `<stem>.xmp` rating sidecar per keeper.
- Keybindings: defaults as listed above; trivially reconfigurable later.
