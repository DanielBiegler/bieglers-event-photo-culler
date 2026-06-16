> [!WARNING]
> This is a work in progress.

# Photo Culler

A native desktop app for fast event-photo culling: rate (1–5★) / reject JPEGs,
filter, and read a capture-time coverage timeline — built to make rating a
500–3000 image shoot a fast, seamless loop. Rust + [egui](https://github.com/emilk/egui),
single binary, no webview.

The photographer copies only the JPEGs off the SD card, culls/rates here, and
exports a keeper-filename CSV that drives which RAWs to pull for post.

## Run

```bash
cargo run --release
```

Then **Open folder…** and point it at a flat folder of camera JPEGs.

## Keys

`1`–`5` rate · `0` clear · `X` reject · `← / →` prev/next (filter-aware) ·
`N` / `Shift+N` next/prev unrated · `Ctrl + ← / →` jump time-bin ·
`Q` / click 1.5× zoom (drag to pan) · `G` / `E` grid / loupe ·
`Ctrl+C` / right-click copy image · `?` help · `Esc` close/zoom-out.

## Notes

- **JPEG-only**, one flat folder at a time (no RAW, no subfolder recursion).
- Ratings persist to a per-folder `.cull.json` sidecar; the last folder + image
  resume on launch.
- See `SPEC.md` for the product design and `CLAUDE.md` for the module map.

Icons: [Lucide](https://lucide.dev) (ISC License), bundled as `assets/lucide.ttf`.
