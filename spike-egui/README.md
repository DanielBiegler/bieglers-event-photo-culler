# spike-egui — throwaway native-UI evaluation

Not part of the shipping app. A minimal **egui** rebuild of the two surfaces
that actually decide whether dropping Tauri for native UI is worth it:

1. **Virtualized thumbnail grid** — embedded EXIF thumbnails decoded in Rust on
   a worker pool and uploaded to GPU textures created on demand. No base64, no
   per-item IPC (contrast `get_thumbnail` in `src-tauri/src/lib.rs`).
2. **Loupe with a ±6 sliding window** of full-res textures that are *explicitly*
   evicted when they leave the window — the bounded-memory guarantee the webview
   can't give. Toggle fit ↔ 100% with pan.
3. **Coverage timeline** (bottom panel) — the feasibility test for dense,
   interactive custom UI: independent ≥-stars and bin-minutes controls,
   density+coverage bars, gap highlighting, the three-state ready strip
   (gray ring / count-left / green dot), a current-position marker, per-bin
   hover tooltips, and click-to-jump. All hand-painted over one allocated rect.
   Hit **Seed demo ratings** to populate it instantly, or rate with `1`–`5`/`0`/`X`.

The top bar is a HUD: image count, position, **count of resident full-res
textures and estimated VRAM**, thumbnail count, and FPS. Watch the resident
count and MB stay flat while you cycle — that's the headline claim under test.

## Run

```bash
cd spike-egui
cargo run            # debug; first build pulls egui + deps (a few minutes)
cargo run --release  # use this for honest decode-latency / memory numbers
```

Then **Open folder…** and point it at a directory of camera JPEGs (ideally a
real 500–3000 image event so the grid and the window are stressed).

## Keys

`←`/`→` navigate · `1`–`5` rate · `0` clear · `X` reject · `Q` fit↔100% · `G` grid
· `E` loupe · click thumbnail → loupe · click image → toggle zoom · drag (when
zoomed) → pan · click a timeline bin → jump.

## De-risking notes (things the webview did for free)

- **EXIF orientation** is applied at decode time (`loader::apply_orientation`),
  driven by the `Orientation` tag read in `scan.rs`. The `image` crate does not
  auto-rotate the way a browser does, so without this every portrait shot would
  display sideways. Test against a folder with portrait frames.
- **Icon-first toolbar** uses the **Lucide** font (`assets/lucide.ttf`) loaded as
  an egui font *fallback*, so an icon is just a private-use `\u{...}` char in a
  normal string — crisp at any size, no per-icon textures, no SVG rasterizer.
  Icons map to codepoints from Lucide's `font/info.json`; see the `IC_*` consts.

## What this is and isn't

- **Is:** a benchmarking harness for perceived latency + memory determinism,
  plus a feasibility probe for the dense custom timeline and the icon-first UI.
- **Isn't:** sidecar persistence, filters, CSV export — intentionally omitted.
  Ratings here are in-memory only (no `.cull.json`). Those are straightforward
  retained-state UI / existing Rust core and don't affect the perf question.
- Uses the **glow** backend via `eframe` for build reliability. A real rewrite
  would use the **wgpu** backend to manage textures even more directly; egui's
  `TextureHandle` create/drop already demonstrates the residency control here.
- `src/scan.rs` is copied from `src-tauri/src/lib.rs`. In a real migration that
  logic moves to a shared `culler-core` crate consumed by both front-ends.

## Reading the result

Expect the **grid and memory** to clearly beat the current build, and the
**loupe to roughly break even** — WebKit already GPU-decodes single images, so
that surface is Tauri's strongest. If the loupe doesn't regress and the grid +
memory improve, the rewrite case is real; if the loupe feels worse on your
24MP files, weigh that against the gains before committing.

## Third-party assets

`assets/lucide.ttf` is the **Lucide** icon font (https://lucide.dev),
distributed under the **ISC License**. Lucide is a fork of Feather Icons.

```
ISC License

Copyright (c) 2020, Lucide Contributors
Copyright (c) 2013-2022, Cole Bemis (Feather, the project Lucide is forked from)

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```

If the egui front-end ships for real, carry this notice (and Lucide's
`LICENSE`) alongside the bundled font.
