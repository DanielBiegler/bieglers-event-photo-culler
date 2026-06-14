# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A Tauri 2 desktop app for fast event-photo culling: rate (1–5★) / reject JPEGs, filter, and read a capture-time coverage timeline. Frontend is React 18 + TypeScript (Vite); core is Rust. See @SPEC.md for the full design rationale and decisions.

## Commands

- **Package manager is `bun`** (not npm/pnpm). `bun install` to set up.
- `bun run tauri dev` — launch the app with HMR. Frontend edits hot-reload; Rust edits auto-recompile and relaunch.
- `bunx tsc --noEmit` — typecheck the frontend. Run this after frontend edits; the project is TS strict with `noUnusedLocals`/`noUnusedParameters`.
- `bun run build` — production frontend build (runs `tsc` then `vite build`).
- Rust: `cargo check` / `cargo build` inside `src-tauri/`.
- **Linux system libs required** (already installed here): `libwebkit2gtk-4.1-dev`, `libsoup-3.0`, `javascriptcoregtk-4.1`, plus the standard Tauri set.

## Architecture

- All frontend state lives in `src/App.tsx` (image list, ratings, filters, keyboard handling, persistence). Components: `Loupe`, `Grid` (virtualized), `Timeline`, `Toolbar`, `HelpOverlay`, `Thumbnail`. Rust commands are wrapped in `src/api.ts`; Rust lives in `src-tauri/src/lib.rs`.
- **JPEG-only.** No RAW handling.
- Loupe loads full images via the Tauri **asset protocol** (`convertFileSrc`, scope `["**"]`) with a sliding ±N decode window — no disk cache.
- Grid thumbnails come from the **embedded EXIF thumbnail** (extracted in Rust, no full decode), fetched lazily and cached in memory at the `api.ts` layer.
- Ratings persist to a per-folder `.cull.json` **sidecar** (debounced, atomic write). Last folder + per-folder last image are remembered in `localStorage` for resume-on-startup.
- Capture time = EXIF `DateTimeOriginal` (mtime fallback); drives chronological sort and the timeline bins.

## Critical gotchas

- **Heavy Tauri commands MUST be `async` + `tauri::async_runtime::spawn_blocking`.** Synchronous commands run on the UI thread and freeze the whole app (e.g. decoding a 24MP JPEG). See `copy_image` for the pattern.
- **Do not disturb the dependency pins in `src-tauri/Cargo.toml`** (`alloc-no-stdlib`, `alloc-stdlib`, `brotli-decompressor`). They resolve a broken `brotli` (via `tauri-codegen`) dependency graph that otherwise fails to compile. Avoid blanket `cargo update`; `Cargo.lock` is committed intentionally.
- **No CSS animations or transitions** — globally disabled in `src/styles.css` (`transition/animation: none !important`). Responsiveness is the core UX goal; don't add them.
- Keep the UI **icon-first and concise** (see `src/components/Icons.tsx`); prefer icons over text buttons.
- Navigation is **filter-aware** (skips filtered-out images) and **wraps** at both ends. The grid renders the filtered list; the loupe indexes the full list. Preserve both when touching nav logic.
