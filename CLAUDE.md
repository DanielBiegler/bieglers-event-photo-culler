# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A native **egui** desktop app (Rust) for fast event-photo culling: rate (1–5★) / reject JPEGs, filter, and read a capture-time coverage timeline. Single binary, no webview. See @SPEC.md for the full design rationale and decisions.

> History: this began as a Tauri 2 + React/TypeScript app and was rewritten to native egui for performance and memory determinism. The Tauri/React code lives in git history (pre-`egui-rewrite` branch) if you need to compare behavior.

## Commands

- `cargo run` — launch the app (debug). `cargo run --release` for realistic decode latency / memory.
- `cargo build` / `cargo check` — compile. `cargo build --release` for the shipping binary.
- `cargo fmt` / `cargo clippy` — format / lint.
- **Linux system libs**: the standard winit/glow set (X11/Wayland + OpenGL). No webkit2gtk needed anymore.

## Architecture

All UI is immediate-mode egui rendered from `App` in `src/main.rs`. Modules:

- `main.rs` — `App` state, the `update` loop, input/keymap, and the panels: `toolbar`, `grid`, `loupe`, `loupe_hud`, `timeline`, `help_overlay`, plus the toast.
- `scan.rs` — parallel folder scan; one EXIF parse per file yields capture time (`DateTimeOriginal`, mtime fallback) + orientation; embedded-thumbnail extraction (no full decode).
- `loader.rs` — two background decode pools (thumb + full) feeding finished `ColorImage`s to the UI thread; `apply_orientation` rotates/flips per the EXIF tag.
- `model.rs` — `Rating`, `View`, filter enums, and the `passes` predicate.
- `persist.rs` — `.cull.json` sidecar (atomic temp+rename, debounced) + resume config in the OS config dir.
- `export.rs` — keeper handoff: writes a chosen folder with `keepers.txt` (one stem per line, ≥ threshold) + one `<stem>.xmp` Lightroom/darktable sidecar per keeper carrying `xmp:Rating`. `clipboard.rs` — copy image via arboard.

**Decode/memory model:** the loupe keeps a ±`WINDOW` sliding set of full-res GPU textures it explicitly evicts (`maintain_window`); the grid creates one small texture per embedded thumbnail on demand. Decoding happens off-thread; the UI uploads textures it owns — there is no disk cache.

**JPEG-only.** No RAW handling.

## Critical gotchas

- **Decoding is off the UI thread by construction** (`loader.rs` worker pools). Never decode a full JPEG inline in `update` — it stalls the frame. Hand paths to the loader and upload the resulting `ColorImage`.
- **EXIF orientation must be applied at decode time** (`loader::apply_orientation`). A native decode does *not* auto-rotate like a browser, so portrait shots would otherwise display sideways — in both thumbnails and the loupe.
- **Ratings are keyed by filename** (`HashMap<String, Rating>`) to match the `.cull.json` sidecar; that format is unchanged from the Tauri app and must stay compatible.
- **Navigation is filter-aware and wraps.** `current` is an index into the FULL list; the grid renders the *filtered* subset and maps cells back to full indices; `go`/`go_bin`/`go_next_unrated` step the full list. Preserve this split.
- **Keep the UI icon-first** via the bundled **Lucide** font (`assets/lucide.ttf`), loaded as a font fallback; icons are `IC_*` private-use codepoints. Prefer icons over text buttons.
- **No animations** beyond the toast fade — responsiveness is the core UX goal. egui repaints reactively; only `request_repaint` while decode work or the toast is pending.
- Persistence is **debounced** (`SAVE_DEBOUNCE`) and also flushed in `on_exit`; don't write the sidecar on every keystroke.
