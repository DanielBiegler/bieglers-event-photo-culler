//! Photo Culler — a native egui desktop app for fast event-photo culling.
//!
//! Two decode surfaces with deliberately different memory strategies:
//!   1. Grid: virtualized thumbnails from embedded EXIF thumbnails, each a small
//!      GPU texture created on demand.
//!   2. Loupe: a ±WINDOW sliding window of full-res textures we *explicitly*
//!      evict, so resident VRAM stays bounded however long you cull.
//!
//! Ratings persist to a per-folder `.cull.json` sidecar; the last folder + image
//! resume on launch. See SPEC.md for the product design and CLAUDE.md for the
//! module map.

mod clipboard;
mod export;
mod loader;
mod model;
mod persist;
mod scan;

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use loader::{Kind, Loader};
use model::{passes, Rating, RejectFilter, StarFilterMode, View};
use persist::ResumeConfig;
use scan::Entry;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// Native file dialogs are blocking, so they run on a background thread and
/// send their result back here (`None` = cancelled) to be handled on the UI
/// thread — keeping the event loop responsive while a dialog is open.
enum DialogResult {
    Open(Option<PathBuf>),
    Export(Option<PathBuf>),
}

const WINDOW: usize = 6; // ±N full-res frames kept resident around the current one
const CELL: Vec2 = Vec2::new(176.0, 132.0);
const ACCENT: Color32 = Color32::from_rgb(91, 191, 106);
const GRID_SCROLL_GAIN: f32 = 3.0; // multiplies trackpad/pixel scroll in the grid
const SAVE_DEBOUNCE: Duration = Duration::from_millis(600); // idle gap before a sidecar write
const ZOOM: f32 = 1.5; // loupe magnification over 100% (matches the original app)

// Keyboard shortcuts, shown in the help overlay.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1 – 5", "Set rating"),
    ("0", "Clear rating"),
    ("X", "Toggle reject"),
    ("← / →", "Previous / next (filter-aware, wraps)"),
    ("N / Shift+N", "Next / previous unrated image"),
    ("Ctrl + ← / →", "Jump to previous / next time-bin"),
    ("Q / click", "Toggle 1.5× zoom (drag to pan)"),
    ("G / E", "Grid / loupe view"),
    ("Ctrl + C / right-click", "Copy image to clipboard"),
    ("?", "Toggle this help"),
    ("Esc", "Close help / zoom out"),
    ("Ctrl + Q", "Quit"),
];

// Lucide icon glyphs (private-use codepoints baked into assets/lucide.ttf).
const IC_OPEN: &str = "\u{e247}"; // folder-open
const IC_GRID: &str = "\u{e0ff}"; // layout-grid
const IC_LOUPE: &str = "\u{e0f6}"; // image
const IC_EXPORT: &str = "\u{e0b2}"; // download
const IC_HELP: &str = "\u{e082}"; // circle-help
const IC_REJECT: &str = "\u{e051}"; // ban

const TOAST_LIFE: Duration = Duration::from_millis(2600); // transient notification lifetime

struct App {
    entries: Vec<Entry>,
    loader: Loader,

    thumb_tex: HashMap<usize, TextureHandle>,
    thumb_requested: HashSet<usize>,
    full_tex: HashMap<usize, TextureHandle>,
    full_requested: HashSet<usize>,

    // Ratings are keyed by filename to match the `.cull.json` sidecar.
    ratings: HashMap<String, Rating>,
    folder: Option<PathBuf>,
    config: ResumeConfig,
    dirty: bool,          // ratings changed since the last save
    last_change: Instant, // when `dirty` was last set (drives the debounce)

    // Filters (apply to the grid + filter-aware navigation).
    min_stars: u8,
    star_filter_mode: StarFilterMode,
    reject_filter: RejectFilter,
    auto_advance: bool,

    bin_minutes: i64, // 5 / 10 / 15 — timeline-local control
    threshold: u8, // keeper threshold: timeline coverage + CSV export (independent of the view filter)

    current: usize, // index into the FULL entries list
    view: View,
    zoom: bool,
    pan: Vec2,
    show_help: bool,
    toast: Option<(String, Instant)>, // transient bottom-left notification
    grid_prev_current: usize,         // detect external current change → scroll grid

    // Off-thread native file dialogs.
    dialog_tx: Sender<DialogResult>,
    dialog_rx: Receiver<DialogResult>,
    dialogs_open: u32,
}

impl App {
    fn new() -> Self {
        let config = ResumeConfig::load();
        let (dialog_tx, dialog_rx) = channel();
        let mut app = Self {
            entries: Vec::new(),
            loader: Loader::new(),
            thumb_tex: HashMap::new(),
            thumb_requested: HashSet::new(),
            full_tex: HashMap::new(),
            full_requested: HashSet::new(),
            ratings: HashMap::new(),
            folder: None,
            config,
            dirty: false,
            last_change: Instant::now(),
            min_stars: 0,
            star_filter_mode: StarFilterMode::Gte,
            reject_filter: RejectFilter::All,
            auto_advance: true,
            bin_minutes: 10,
            threshold: 3,
            current: 0,
            view: View::Loupe,
            zoom: false,
            pan: Vec2::ZERO,
            show_help: false,
            toast: None,
            grid_prev_current: usize::MAX,
            dialog_tx,
            dialog_rx,
            dialogs_open: 0,
        };
        // Resume into the last folder if it still exists.
        if let Some(last) = app.config.last_folder.clone() {
            let path = PathBuf::from(&last);
            if path.is_dir() {
                app.open(path);
            }
        }
        app
    }

    fn open(&mut self, folder: PathBuf) {
        let entries = scan::scan_folder(&folder);
        let ratings = persist::load_ratings(&folder);
        // Dropping the texture maps frees every GPU texture from the prior folder.
        self.thumb_tex.clear();
        self.thumb_requested.clear();
        self.full_tex.clear();
        self.full_requested.clear();

        let folder_str = folder.to_string_lossy().to_string();
        // Restore the last-viewed image for this folder if it still exists.
        self.current = self
            .config
            .last_image
            .get(&folder_str)
            .and_then(|name| entries.iter().position(|e| &e.name == name))
            .unwrap_or(0);

        self.entries = entries;
        self.ratings = ratings;
        self.folder = Some(folder);
        self.view = View::Loupe;
        self.zoom = false;
        self.pan = Vec2::ZERO;
        self.dirty = false; // freshly loaded — nothing pending

        self.config.last_folder = Some(folder_str);
        self.config.save();
    }

    /// Rating for an entry index (default when unrated).
    fn rating_at(&self, idx: usize) -> Rating {
        self.entries
            .get(idx)
            .and_then(|e| self.ratings.get(&e.name))
            .copied()
            .unwrap_or_default()
    }

    /// Does entry `idx` pass the active filter?
    fn passes_idx(&self, idx: usize) -> bool {
        let r = self
            .entries
            .get(idx)
            .and_then(|e| self.ratings.get(&e.name));
        passes(r, self.min_stars, self.star_filter_mode, self.reject_filter)
    }

    /// Full-list indices currently shown by the filter (grid order).
    fn filtered_indices(&self) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| self.passes_idx(i))
            .collect()
    }

    /// Move `delta` steps through the FULL list, skipping filtered-out images
    /// and wrapping at both ends (mirrors App.tsx `go`). No-op if nothing passes.
    fn go(&mut self, delta: i64) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let i = self.current as i64;
        let n_i = n as i64;
        for step in 1..=n_i {
            let j = (((i + delta * step) % n_i) + n_i) % n_i;
            let j = j as usize;
            if self.passes_idx(j) {
                self.current = j;
                self.remember_position();
                return;
            }
        }
    }

    /// Jump to the first image of the adjacent non-empty time-bin (Ctrl+←/→).
    /// Ignores the active filter (it navigates by capture time). Ported from
    /// App.tsx `goBin`.
    fn go_bin(&mut self, dir: i64) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let start = self.entries[0].capture_time;
        let bin_sec = self.bin_minutes * 60;
        let bin_of = |ct: i64| (ct - start) / bin_sec;
        let i = self.current;
        let cur_bin = bin_of(self.entries[i].capture_time);
        if dir > 0 {
            for j in (i + 1)..n {
                if bin_of(self.entries[j].capture_time) > cur_bin {
                    self.current = j;
                    self.remember_position();
                    return;
                }
            }
            self.current = 0; // past the last bin → wrap to the first image
        } else {
            // Step back into an earlier bin, then to that bin's first image.
            let mut j = i as i64 - 1;
            while j >= 0 && bin_of(self.entries[j as usize].capture_time) >= cur_bin {
                j -= 1;
            }
            if j >= 0 {
                let prev_bin = bin_of(self.entries[j as usize].capture_time);
                while j - 1 >= 0 && bin_of(self.entries[(j - 1) as usize].capture_time) == prev_bin
                {
                    j -= 1;
                }
                self.current = j as usize;
                self.remember_position();
                return;
            }
            // Before the first bin → wrap to the first image of the last bin.
            let last_bin = bin_of(self.entries[n - 1].capture_time);
            let mut k = n - 1;
            while k >= 1 && bin_of(self.entries[k - 1].capture_time) == last_bin {
                k -= 1;
            }
            self.current = k;
        }
        self.remember_position();
    }

    /// Jump to the next unrated image (no stars, not rejected) in `dir`,
    /// ignoring the filter so there's always work to resume. Ported from
    /// App.tsx `goNextUnrated`. Returns a status message for the toast.
    fn go_next_unrated(&mut self, dir: i64) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let i = self.current as i64;
        let n_i = n as i64;
        for step in 1..=n_i {
            let j = (((i + dir * step) % n_i) + n_i) % n_i;
            let j = j as usize;
            let r = self.rating_at(j);
            if r.stars == 0 && !r.reject {
                self.current = j;
                self.view = View::Loupe;
                self.zoom = false;
                self.remember_position();
                let label = if dir == 1 {
                    "Next unrated"
                } else {
                    "Prev unrated"
                };
                self.notify(format!(
                    "{} · {} ({}/{})",
                    label,
                    self.entries[j].name,
                    j + 1,
                    n
                ));
                return;
            }
        }
        self.notify("No unrated images left — all reviewed");
    }

    /// Copy the current full image to the clipboard on a background thread.
    fn copy_current(&mut self) {
        if let Some(e) = self.entries.get(self.current) {
            let (path, orientation, name) = (e.path.clone(), e.orientation, e.name.clone());
            std::thread::spawn(move || {
                let _ = clipboard::copy_image(&path, orientation);
            });
            self.notify(format!("Copied · {name}"));
        }
    }

    /// Global keyboard shortcuts. Collects intents inside one `ctx.input`, then
    /// applies them — and ignores everything while a widget wants keyboard input
    /// (the egui equivalent of the React INPUT/SELECT guard).
    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (mut left, mut right, mut q, mut g, mut e) = (false, false, false, false, false);
        let (mut toggle_reject, mut n_key, mut shift) = (false, false, false);
        let (mut esc, mut help, mut copy) = (false, false, false);
        let (mut bin_left, mut bin_right) = (false, false);
        let mut quit = false;
        let mut set_star: Option<u8> = None;
        ctx.input(|i| {
            let ctrl = i.modifiers.command || i.modifiers.ctrl;
            shift = i.modifiers.shift;
            esc = i.key_pressed(egui::Key::Escape);
            // '?' is layout-dependent; read it as a text event instead of a key.
            for ev in &i.events {
                if let egui::Event::Text(t) = ev {
                    if t == "?" {
                        help = true;
                    }
                }
            }
            if ctrl {
                copy = i.key_pressed(egui::Key::C);
                bin_left = i.key_pressed(egui::Key::ArrowLeft);
                bin_right = i.key_pressed(egui::Key::ArrowRight);
                quit = i.key_pressed(egui::Key::Q);
            } else {
                left = i.key_pressed(egui::Key::ArrowLeft);
                right = i.key_pressed(egui::Key::ArrowRight);
                q = i.key_pressed(egui::Key::Q);
                g = i.key_pressed(egui::Key::G);
                e = i.key_pressed(egui::Key::E);
                n_key = i.key_pressed(egui::Key::N);
                toggle_reject = i.key_pressed(egui::Key::X);
                for (key, val) in [
                    (egui::Key::Num0, 0u8),
                    (egui::Key::Num1, 1),
                    (egui::Key::Num2, 2),
                    (egui::Key::Num3, 3),
                    (egui::Key::Num4, 4),
                    (egui::Key::Num5, 5),
                ] {
                    if i.key_pressed(key) {
                        set_star = Some(val);
                    }
                }
            }
        });

        let n = self.entries.len();

        // Ctrl+Q quits; `on_exit` still runs, flushing the debounced sidecar.
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Copy + bin-jump work even while the help overlay is open.
        if copy {
            self.copy_current();
        }
        if n > 0 {
            if bin_left {
                self.go_bin(-1);
            }
            if bin_right {
                self.go_bin(1);
            }
        }
        if help {
            self.show_help = !self.show_help;
        }
        if esc {
            if self.show_help {
                self.show_help = false;
            } else if self.zoom {
                self.zoom = false;
                self.pan = Vec2::ZERO;
            }
        }
        if self.show_help || n == 0 {
            return; // swallow the rest while help is up
        }

        if left {
            self.go(-1);
        }
        if right {
            self.go(1);
        }
        if n_key {
            self.go_next_unrated(if shift { -1 } else { 1 });
        }
        if let Some(s) = set_star {
            let name = self.entries[self.current].name.clone();
            let prev = self.rating_at(self.current).stars;
            // Rating an image implicitly un-rejects it (a keeper isn't a reject).
            self.edit_current(|r| {
                r.stars = s;
                if s > 0 {
                    r.reject = false;
                }
            });
            if s == 0 && prev > 0 {
                self.notify(format!("Cleared rating · {name}"));
            }
            // Auto-advance after setting a star (not when clearing to 0).
            if self.auto_advance && s > 0 && self.view == View::Loupe {
                self.go(1);
            }
        }
        if toggle_reject {
            let name = self.entries[self.current].name.clone();
            self.edit_current(|r| r.reject = !r.reject);
            let now_rejected = self.rating_at(self.current).reject;
            if !now_rejected {
                self.notify(format!("Cleared reject · {name}"));
            }
            // Advance only when flagging (not when clearing) so review stays put.
            if self.auto_advance && self.view == View::Loupe && now_rejected {
                self.go(1);
            }
        }
        if q {
            self.zoom = !self.zoom;
            self.pan = Vec2::ZERO;
        }
        if g {
            self.view = View::Grid;
        }
        if e {
            self.view = View::Loupe;
        }
    }

    /// Apply `f` to the current image's rating, mark dirty, and remember it.
    fn edit_current<F: FnOnce(&mut Rating)>(&mut self, f: F) {
        let Some(name) = self.entries.get(self.current).map(|e| e.name.clone()) else {
            return;
        };
        f(self.ratings.entry(name).or_default());
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
    }

    /// Show a transient bottom-left notification.
    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Remember the current image as the per-folder resume point.
    fn remember_position(&mut self) {
        if let (Some(folder), Some(entry)) = (&self.folder, self.entries.get(self.current)) {
            self.config.remember(&folder.to_string_lossy(), &entry.name);
        }
    }

    /// Write the sidecar (off-thread) and the resume config, clearing `dirty`.
    fn flush_save(&mut self) {
        if let Some(folder) = self.folder.clone() {
            let ratings = self.ratings.clone();
            std::thread::spawn(move || {
                let _ = persist::save_sidecar(&folder, &ratings);
            });
        }
        self.config.save();
        self.dirty = false;
    }

    /// Pull finished decodes off the worker channel and upload them as textures.
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.loader.out_rx.try_recv() {
            let name = format!("{:?}-{}", matches!(done.kind, Kind::Full), done.index);
            let tex = ctx.load_texture(name, done.image, TextureOptions::LINEAR);
            match done.kind {
                Kind::Thumb => {
                    self.thumb_tex.insert(done.index, tex);
                    self.thumb_requested.remove(&done.index);
                }
                Kind::Full => {
                    self.full_tex.insert(done.index, tex);
                    self.full_requested.remove(&done.index);
                }
            }
        }
    }

    /// Keep exactly the ±WINDOW full-res textures resident, request the missing
    /// ones current-first. This is the residency guarantee the spike is about.
    fn maintain_window(&mut self) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let lo = self.current.saturating_sub(WINDOW);
        let hi = (self.current + WINDOW).min(n - 1);
        let keep: HashSet<usize> = (lo..=hi).collect();

        // Evict everything outside the window (frees GPU memory immediately).
        self.full_tex.retain(|k, _| keep.contains(k));
        // Forget out-of-window pending requests so they can be re-issued later.
        self.full_requested.retain(|k| keep.contains(k));

        // Priority order: current, then expanding outward.
        let mut order = vec![self.current];
        for d in 1..=WINDOW {
            if self.current >= d {
                order.push(self.current - d);
            }
            if self.current + d <= hi {
                order.push(self.current + d);
            }
        }
        for idx in order {
            if idx < n && !self.full_tex.contains_key(&idx) && self.full_requested.insert(idx) {
                let e = &self.entries[idx];
                self.loader.request_full(idx, e.path.clone(), e.orientation);
            }
        }
    }
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// Draw `tex` letterboxed to fit `within`, centered. Returns nothing; painting only.
fn paint_fit(painter: &egui::Painter, tex: &TextureHandle, within: Rect) {
    let [tw, th] = tex.size();
    let (tw, th) = (tw as f32, th as f32);
    let scale = (within.width() / tw).min(within.height() / th);
    let size = Vec2::new(tw * scale, th * scale);
    let r = Rect::from_center_size(within.center(), size);
    painter.image(tex.id(), r, full_uv(), Color32::WHITE);
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);

        // Apply any finished file-dialog results; keep polling while one is open
        // (the result arrives with no UI event, so nudge the reactive loop).
        while let Ok(result) = self.dialog_rx.try_recv() {
            self.finish_dialog(result);
        }
        if self.dialogs_open > 0 {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        self.handle_keys(ctx);
        self.maintain_window();

        // Debounced persistence: write once the edits settle, else schedule a
        // repaint so the timer fires even while the app is otherwise idle.
        if self.dirty {
            let elapsed = self.last_change.elapsed();
            if elapsed >= SAVE_DEBOUNCE {
                self.flush_save();
            } else {
                ctx.request_repaint_after(SAVE_DEBOUNCE - elapsed);
            }
        }

        // ------------------------------- toolbar ------------------------------
        // Panel padding lives on the Frame, independent of the global spacing.
        let bar_frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(14.0, 10.0));
        egui::TopBottomPanel::top("bar")
            .frame(bar_frame)
            .show(ctx, |ui| {
                self.toolbar(ui);
            });

        // --------------------------- coverage timeline ------------------------
        // Bottom panel declared before the central panel so it reserves its
        // strip and the content area takes whatever remains.
        if !self.entries.is_empty() {
            let tl_frame = egui::Frame::side_top_panel(&ctx.style())
                .inner_margin(egui::Margin::symmetric(14.0, 10.0));
            egui::TopBottomPanel::bottom("timeline")
                .frame(tl_frame)
                .show(ctx, |ui| {
                    self.timeline(ui);
                });
        }

        // Loupe HUD sits just above the timeline (added after it, so it stacks
        // higher), only while culling in the loupe.
        if self.view == View::Loupe && !self.entries.is_empty() {
            let hud_frame = egui::Frame::side_top_panel(&ctx.style())
                .inner_margin(egui::Margin::symmetric(14.0, 8.0));
            egui::TopBottomPanel::bottom("loupe_hud")
                .frame(hud_frame)
                .show(ctx, |ui| {
                    self.loupe_hud(ui);
                });
        }

        // ------------------------------ content -------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a folder of JPEGs to begin.");
                });
                return;
            }
            match self.view {
                View::Grid => self.grid(ui),
                View::Loupe => self.loupe(ui),
            }
        });

        if self.show_help {
            self.help_overlay(ctx);
        }

        // Transient bottom-left toast, fading out over its final 400ms.
        let expired = self
            .toast
            .as_ref()
            .map(|(_, t)| t.elapsed() >= TOAST_LIFE)
            .unwrap_or(false);
        if expired {
            self.toast = None;
        }
        if let Some((text, t)) = &self.toast {
            let remaining = TOAST_LIFE.saturating_sub(t.elapsed()).as_secs_f32();
            let a = (remaining / 0.4).clamp(0.0, 1.0);
            let alpha = (a * 230.0) as u8;
            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(16.0, -16.0))
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(&ctx.style())
                        .fill(Color32::from_black_alpha((alpha as f32 * 0.9) as u8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(text).color(Color32::from_white_alpha(alpha)),
                            );
                        });
                });
            ctx.request_repaint(); // keep fading
        }

        // Reactive repaint: keep polling the decode channel only while work is
        // outstanding, then go fully idle (no busy-loop, no battery drain).
        if !self.thumb_requested.is_empty() || !self.full_requested.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Flush any unsaved edits synchronously on shutdown so the last <600ms of
    /// rating changes aren't lost to the debounce.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.dirty {
            if let Some(folder) = &self.folder {
                let _ = persist::save_sidecar(folder, &self.ratings);
            }
        }
        self.config.save();
    }
}

impl App {
    fn grid(&mut self, ui: &mut egui::Ui) {
        // Trackpad scrolling arrives as pixel deltas that `line_scroll_speed`
        // (wheel-only) can't speed up, so amplify the smoothed delta directly
        // before the ScrollArea below consumes it. Grid-scoped, not global.
        if ui.ui_contains_pointer() {
            ui.input_mut(|i| i.smooth_scroll_delta *= GRID_SCROLL_GAIN);
        }
        // The grid renders the FILTERED list; cells map back to full indices.
        let filtered = self.filtered_indices();
        let spacing = 14.0; // gutter between thumbnail cells
                            // Make the real inter-cell gap match the gap used for the column math.
        ui.spacing_mut().item_spacing = Vec2::splat(spacing);
        let avail_w = ui.available_width();
        let cols = (((avail_w + spacing) / (CELL.x + spacing)).floor() as usize).max(1);
        let rows = filtered.len().div_ceil(cols);
        // `show_rows` wants the row height WITHOUT spacing and adds `item_spacing.y`
        // itself; the on-screen pitch is therefore `CELL.y + spacing`. Use that
        // pitch for the scroll math so it matches `show_rows`' internal layout.
        let row_pitch = CELL.y + spacing;
        let current = self.current;

        // When the selection changed from outside the grid (loupe nav, arrow
        // keys, timeline jump), scroll so the current row is centered in the
        // viewport (clamped to the valid range) — pinning it to the top edge
        // makes every keypress yank the grid and is easy to lose track of.
        let viewport_h = ui.available_height();
        let content_h = rows as f32 * row_pitch - spacing;
        let max_offset = (content_h - viewport_h).max(0.0);
        let scroll_to = (current != self.grid_prev_current)
            .then(|| filtered.iter().position(|&i| i == current))
            .flatten()
            .map(|p| {
                let row_top = (p / cols) as f32 * row_pitch;
                (row_top - (viewport_h - row_pitch) * 0.5).clamp(0.0, max_offset)
            });

        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(offset) = scroll_to {
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, CELL.y, rows, |ui, row_range| {
            for row in row_range {
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        let pos = row * cols + col;
                        if pos >= filtered.len() {
                            break;
                        }
                        let idx = filtered[pos];
                        // Lazily request the thumbnail the first time the
                        // cell scrolls into view (and only once).
                        if !self.thumb_tex.contains_key(&idx) && self.thumb_requested.insert(idx) {
                            let e = &self.entries[idx];
                            self.loader
                                .request_thumb(idx, e.path.clone(), e.orientation);
                        }

                        let (rect, resp) = ui.allocate_exact_size(CELL, Sense::click());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 2.0, Color32::from_gray(22));
                        if let Some(tex) = self.thumb_tex.get(&idx) {
                            paint_fit(&painter, tex, rect.shrink(2.0));
                        } else {
                            painter.text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "…",
                                egui::FontId::proportional(14.0),
                                Color32::DARK_GRAY,
                            );
                        }

                        // Badges: dim + ban icon when rejected, star count.
                        let r = self.rating_at(idx);
                        if r.reject {
                            painter.rect_filled(rect, 2.0, Color32::from_black_alpha(120));
                            painter.text(
                                rect.right_top() + Vec2::new(-5.0, 5.0),
                                egui::Align2::RIGHT_TOP,
                                IC_REJECT,
                                egui::FontId::proportional(15.0),
                                Color32::from_rgb(230, 100, 100),
                            );
                        }
                        if r.stars > 0 {
                            painter.text(
                                rect.left_bottom() + Vec2::new(5.0, -4.0),
                                egui::Align2::LEFT_BOTTOM,
                                "★".repeat(r.stars as usize),
                                egui::FontId::proportional(12.0),
                                Color32::from_rgb(240, 200, 80),
                            );
                        }
                        if idx == current {
                            // `painter_at(rect)` clips to `rect`, and egui centers
                            // the stroke on the edge — so an edge-aligned ring loses
                            // its outer half to the clip and nearly vanishes. Inset
                            // it so the full stroke stays inside and stays visible.
                            let ring = rect.shrink(1.5);
                            painter.rect_stroke(ring, 2.0, Stroke::new(3.0, ACCENT));
                        }

                        // Single-click selects (stay in grid); double-click
                        // opens the loupe (mirrors the React grid).
                        if resp.double_clicked() {
                            self.current = idx;
                            self.view = View::Loupe;
                            self.zoom = false;
                            self.pan = Vec2::ZERO;
                            self.remember_position();
                        } else if resp.clicked() {
                            self.current = idx;
                            self.remember_position();
                        }
                    }
                });
            }
        });
        // Remember what we showed so a *grid-internal* click doesn't re-trigger
        // the scroll next frame (only external changes should scroll).
        self.grid_prev_current = self.current;
    }

    fn loupe(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::BLACK);

        if self.zoom && resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.clicked() {
            if !self.zoom {
                // Zoom in centered on the click: the clicked content point ends
                // up under the viewport center. ZOOM is over 100% (natural), so
                // the offset scales by ZOOM / fit-scale. Ported from Loupe.tsx.
                self.pan = Vec2::ZERO;
                if let (Some(p), Some(tex)) = (
                    resp.interact_pointer_pos(),
                    self.full_tex.get(&self.current),
                ) {
                    let [tw, th] = tex.size();
                    let s = (rect.width() / tw as f32).min(rect.height() / th as f32);
                    if s > 0.0 {
                        self.pan = -(p - rect.center()) * (ZOOM / s);
                    }
                }
                self.zoom = true;
            } else {
                self.zoom = false;
                self.pan = Vec2::ZERO;
            }
        }

        match self.full_tex.get(&self.current) {
            Some(tex) if !self.zoom => paint_fit(&painter, tex, rect),
            Some(tex) => {
                // 1.5× of natural pixels, panned; painter_at clips the overflow.
                let [tw, th] = tex.size();
                let size = Vec2::new(tw as f32 * ZOOM, th as f32 * ZOOM);
                let r = Rect::from_center_size(rect.center() + self.pan, size);
                painter.image(tex.id(), r, full_uv(), Color32::WHITE);
            }
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "decoding…",
                    egui::FontId::proportional(18.0),
                    Color32::GRAY,
                );
            }
        }

        // Right-click → copy the current image.
        let mut do_copy = false;
        resp.context_menu(|ui| {
            if ui.button("Copy image").clicked() {
                do_copy = true;
                ui.close_menu();
            }
        });
        if do_copy {
            self.copy_current();
        }
    }

    /// Status strip under the loupe: review progress, a clickable star rating,
    /// reject + zoom toggles, and the filename. Mirrors the React `loupe-hud`.
    fn loupe_hud(&mut self, ui: &mut egui::Ui) {
        let Some(entry) = self.entries.get(self.current) else {
            return;
        };
        let name = entry.name.clone();
        let rating = self.rating_at(self.current);
        let total = self.entries.len();
        let reviewed = self
            .entries
            .iter()
            .filter(|e| {
                self.ratings
                    .get(&e.name)
                    .map(|r| r.handled())
                    .unwrap_or(false)
            })
            .count();
        let pct = if total > 0 { reviewed * 100 / total } else { 0 };

        ui.horizontal(|ui| {
            ui.monospace(format!("✓ {reviewed}/{total} ({pct}%)"))
                .on_hover_text("Reviewed (rated or rejected) / total");
            ui.separator();

            // Clickable 1–5 star rating.
            let mut set_star = None;
            for s in 1..=5u8 {
                let col = if rating.stars >= s {
                    Color32::from_rgb(240, 200, 80)
                } else {
                    Color32::from_gray(90)
                };
                let star = egui::Label::new(egui::RichText::new("★").size(18.0).color(col))
                    .sense(Sense::click());
                if ui.add(star).clicked() {
                    set_star = Some(s);
                }
            }
            if let Some(s) = set_star {
                // Rating an image implicitly un-rejects it.
                self.edit_current(|r| {
                    r.stars = s;
                    if s > 0 {
                        r.reject = false;
                    }
                });
            }

            ui.separator();
            let rj_col = if rating.reject {
                Color32::from_rgb(220, 90, 90)
            } else {
                Color32::from_gray(90)
            };
            let rj = egui::Label::new(egui::RichText::new(IC_REJECT).size(16.0).color(rj_col))
                .sense(Sense::click());
            if ui.add(rj).on_hover_text("Reject (X)").clicked() {
                self.edit_current(|r| r.reject = !r.reject);
            }

            let z_col = if self.zoom {
                ACCENT
            } else {
                Color32::from_gray(90)
            };
            let z =
                egui::Label::new(egui::RichText::new("1.5×").color(z_col)).sense(Sense::click());
            if ui.add(z).on_hover_text("Zoom (Q or click image)").clicked() {
                self.zoom = !self.zoom;
                self.pan = Vec2::ZERO;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(name);
            });
        });
    }

    fn filtered_count(&self) -> usize {
        (0..self.entries.len())
            .filter(|&i| self.passes_idx(i))
            .count()
    }

    /// Open the native folder picker on a background thread (it blocks).
    fn open_folder_dialog(&mut self) {
        let tx = self.dialog_tx.clone();
        self.dialogs_open += 1;
        std::thread::spawn(move || {
            let _ = tx.send(DialogResult::Open(rfd::FileDialog::new().pick_folder()));
        });
    }

    /// Pick a destination folder for the keeper list + XMP sidecars on a
    /// background thread (the dialog blocks).
    fn export_keepers(&mut self) {
        if self.folder.is_none() {
            return;
        }
        let tx = self.dialog_tx.clone();
        self.dialogs_open += 1;
        std::thread::spawn(move || {
            let dest = rfd::FileDialog::new().pick_folder();
            let _ = tx.send(DialogResult::Export(dest));
        });
    }

    /// Handle a finished dialog (called on the UI thread).
    fn finish_dialog(&mut self, result: DialogResult) {
        self.dialogs_open = self.dialogs_open.saturating_sub(1);
        match result {
            DialogResult::Open(Some(dir)) => self.open(dir),
            DialogResult::Open(None) => {}
            DialogResult::Export(Some(dest)) => {
                match export::export_keepers(&dest, &self.entries, &self.ratings, self.threshold) {
                    Ok(n) => self.notify(format!(
                        "Exported {n} keepers ≥ {}★ (list + .xmp)",
                        self.threshold
                    )),
                    Err(e) => self.notify(format!("Export failed: {e}")),
                }
            }
            DialogResult::Export(None) => {}
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let icon = |s: &str| egui::RichText::new(s).size(18.0);
        ui.horizontal(|ui| {
            if ui
                .button(icon(IC_OPEN))
                .on_hover_text("Open folder")
                .clicked()
            {
                self.open_folder_dialog();
            }
            let folder_name = self
                .folder
                .as_ref()
                .and_then(|f| f.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "No folder".to_string());
            ui.label(folder_name);

            ui.separator();
            ui.selectable_value(&mut self.view, View::Loupe, icon(IC_LOUPE))
                .on_hover_text("Loupe (E)");
            ui.selectable_value(&mut self.view, View::Grid, icon(IC_GRID))
                .on_hover_text("Grid (G)");
            ui.separator();

            // Star filter: comparison mode + threshold.
            egui::ComboBox::from_id_salt("star_mode")
                .width(46.0)
                .selected_text(match self.star_filter_mode {
                    StarFilterMode::Gte => "≥",
                    StarFilterMode::Eq => "=",
                    StarFilterMode::Lt => "<",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.star_filter_mode, StarFilterMode::Gte, "≥");
                    ui.selectable_value(&mut self.star_filter_mode, StarFilterMode::Eq, "=");
                    ui.selectable_value(&mut self.star_filter_mode, StarFilterMode::Lt, "<");
                });
            egui::ComboBox::from_id_salt("min_stars")
                .width(54.0)
                .selected_text(format!("{} ★", self.min_stars))
                .show_ui(ui, |ui| {
                    for s in 0..=5u8 {
                        ui.selectable_value(&mut self.min_stars, s, format!("{s} ★"));
                    }
                });

            // Reject filter.
            egui::ComboBox::from_id_salt("reject_filter")
                .selected_text(match self.reject_filter {
                    RejectFilter::All => "All",
                    RejectFilter::Hide => "Hide rejects",
                    RejectFilter::Only => "Only rejects",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.reject_filter, RejectFilter::All, "All");
                    ui.selectable_value(
                        &mut self.reject_filter,
                        RejectFilter::Hide,
                        "Hide rejects",
                    );
                    ui.selectable_value(
                        &mut self.reject_filter,
                        RejectFilter::Only,
                        "Only rejects",
                    );
                });

            ui.checkbox(&mut self.auto_advance, "auto")
                .on_hover_text("Auto-advance after rating");

            // Right-aligned: counts + export + help.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(icon(IC_HELP))
                    .on_hover_text("Keyboard shortcuts (?)")
                    .clicked()
                {
                    self.show_help = !self.show_help;
                }
                if ui
                    .button(icon(IC_EXPORT))
                    .on_hover_text("Export keepers (list + XMP sidecars)")
                    .clicked()
                {
                    self.export_keepers();
                }
                let total = self.entries.len();
                let pos = if total > 0 {
                    (self.current + 1).to_string()
                } else {
                    "–".into()
                };
                ui.monospace(format!("{}  ·  {}/{}", pos, self.filtered_count(), total));
            });
        });
    }

    /// Modal keyboard-shortcut reference (toggled with `?`).
    fn help_overlay(&mut self, ctx: &egui::Context) {
        egui::Window::new("Keyboard shortcuts")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for (keys, desc) in SHORTCUTS {
                            ui.monospace(*keys);
                            ui.label(*desc);
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    self.show_help = false;
                }
            });
    }

    /// The signature feature, rebuilt to stress-test egui's custom-widget story:
    /// two independent controls, density+coverage bars, gap highlighting, the
    /// three-state ready strip, a current-position marker, per-bin hover tooltip
    /// and click-to-jump — all hand-painted over one allocated rect.
    fn timeline(&mut self, ui: &mut egui::Ui) {
        // --- controls (standard widgets) -------------------------------------
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("tl_threshold")
                .selected_text(format!("≥ {} ★", self.threshold))
                .show_ui(ui, |ui| {
                    for s in 1..=5u8 {
                        ui.selectable_value(&mut self.threshold, s, format!("{s} ★"));
                    }
                });
            egui::ComboBox::from_id_salt("tl_bin")
                .selected_text(format!("{} min", self.bin_minutes))
                .show_ui(ui, |ui| {
                    for m in [5i64, 10, 15] {
                        ui.selectable_value(&mut self.bin_minutes, m, format!("{m} min"));
                    }
                });
            ui.label("coverage = stars ≥ threshold (independent of view)");
        });

        // --- bin the set (cheap to redo each frame for a few thousand items) --
        let start = self.entries.first().map(|e| e.capture_time).unwrap_or(0);
        let end = self.entries.last().map(|e| e.capture_time).unwrap_or(0);
        let bin_sec = self.bin_minutes * 60;
        let count = ((end - start) / bin_sec + 1).max(1) as usize;
        let mut bins = vec![TlBin::default(); count];
        for idx in 0..self.entries.len() {
            let e = &self.entries[idx];
            let b = (((e.capture_time - start) / bin_sec) as usize).min(count - 1);
            let r = self.rating_at(idx);
            let bin = &mut bins[b];
            bin.total += 1;
            if bin.first.is_none() {
                bin.first = Some(idx);
            }
            if r.stars >= self.threshold {
                bin.covered += 1;
            }
            if r.handled() {
                bin.handled += 1;
            }
        }
        let current_bin = self
            .entries
            .get(self.current)
            .map(|e| (((e.capture_time - start) / bin_sec) as usize).min(count - 1));

        // --- paint ------------------------------------------------------------
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 96.0), Sense::click());
        let painter = ui.painter_at(rect);
        let strip_h = 18.0;
        let strip_y = rect.top() + strip_h * 0.5;
        let bars_top = rect.top() + strip_h + 6.0;
        let bars_h = rect.bottom() - bars_top;
        let n = bins.len();
        let bw = rect.width() / n as f32;
        let max_total = bins.iter().map(|b| b.total).max().unwrap_or(1).max(1);
        let hover_bin = resp
            .hover_pos()
            .map(|p| (((p.x - rect.left()) / bw).floor() as usize).min(n - 1));

        for (i, b) in bins.iter().enumerate() {
            let x0 = rect.left() + i as f32 * bw;
            let cx = x0 + bw * 0.5;
            if b.total == 0 {
                continue;
            }
            // Bars: height = shooting density, fill = coverage. A bin you shot
            // but have no keepers in yet is a gap, tinted to stand out.
            let th = (b.total as f32 / max_total as f32) * bars_h;
            let track = Rect::from_min_max(
                Pos2::new(x0 + 1.0, rect.bottom() - th),
                Pos2::new(x0 + bw - 1.0, rect.bottom()),
            );
            let gap = b.covered == 0;
            painter.rect_filled(
                track,
                1.0,
                if gap {
                    Color32::from_rgb(74, 42, 42)
                } else {
                    Color32::from_gray(58)
                },
            );
            if b.covered > 0 {
                let fh = (b.covered as f32 / b.total as f32) * th;
                let fill = Rect::from_min_max(
                    Pos2::new(track.left(), track.bottom() - fh),
                    Pos2::new(track.right(), track.bottom()),
                );
                painter.rect_filled(fill, 1.0, ACCENT);
            }

            // Ready strip: green dot = all rated, else the count still left to
            // rate (a partial fill ring when too narrow for text — adaptive
            // rendering egui handles trivially).
            let c = Pos2::new(cx, strip_y);
            if b.handled == b.total {
                painter.circle_filled(c, 4.0, ACCENT);
            } else if bw >= 16.0 {
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    (b.total - b.handled).to_string(),
                    egui::FontId::monospace(10.0),
                    Color32::from_gray(205),
                );
            } else {
                painter.circle_stroke(c, 4.0, Stroke::new(1.0, Color32::from_gray(110)));
                let frac = b.handled as f32 / b.total as f32;
                painter.circle_filled(c, 4.0 * frac, ACCENT);
            }
        }

        // Current-position marker.
        if let Some(cb) = current_bin {
            let x0 = rect.left() + cb as f32 * bw;
            painter.rect_stroke(
                Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x0 + bw, rect.bottom())),
                0.0,
                Stroke::new(1.5, Color32::WHITE),
            );
        }

        // Hover: highlight column, tooltip, and click-to-jump.
        if let Some(hb) = hover_bin {
            let x0 = rect.left() + hb as f32 * bw;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x0 + bw, rect.bottom())),
                0.0,
                Color32::from_white_alpha(16),
            );
            let b = &bins[hb];
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("tl_tip"), |ui| {
                ui.monospace(format!(
                    "{}  ·  {}/{} keepers  ·  {} left to rate",
                    hhmm(start + hb as i64 * bin_sec),
                    b.covered,
                    b.total,
                    b.total - b.handled,
                ));
            });
            if resp.clicked() {
                if let Some(first) = b.first {
                    self.current = first;
                    self.view = View::Loupe;
                    self.zoom = false;
                    self.remember_position();
                }
            }
        }
    }
}

#[derive(Clone, Default)]
struct TlBin {
    total: usize,
    covered: usize, // stars >= threshold
    handled: usize, // rated or rejected
    first: Option<usize>,
}

fn hhmm(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Global theme tweaks. This is the "regime 2" knob set: one flat style the
/// whole app inherits, which individual widgets can still override per-call.
fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    // Gaps *between* widgets, and the padding *inside* buttons/selectables.
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size.y = 30.0; // min clickable height → roomier rows
                                          // A little rounding so the extra padding reads as deliberate, not accidental.
    let rounding = egui::Rounding::same(4.0);
    for w in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        w.rounding = rounding;
    }
    ctx.set_style(style);

    // Faster mouse-wheel scrolling in the grid (points per wheel notch; the
    // egui default is ~50). Affects wheel/line scrolling, not touchpad pixel
    // scrolling, which already arrives pre-scaled by the OS.
    ctx.options_mut(|o| o.line_scroll_speed = 120.0);
}

/// Register the Lucide icon font as a *fallback*: normal text keeps the default
/// font, and the private-use icon codepoints (which the default font lacks)
/// fall through to Lucide. So an icon is just a `\u{...}` char in any string —
/// no per-icon textures, crisp at any size. This is the icon-first design
/// language from the real app, ported to a font instead of SVG.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "lucide".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/lucide.ttf")),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("lucide".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Photo Culler",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new()) as Box<dyn eframe::App>)
        }),
    )
}
