//! egui spike for the photo-culler native-UI evaluation.
//!
//! Implements only the two surfaces that decide whether a native rewrite is
//! worth it:
//!   1. Virtualized thumbnail grid backed by GPU textures we create on demand.
//!   2. Loupe with a ±N sliding window of full-res textures we *explicitly*
//!      evict — the memory guarantee the webview can't give us.
//!
//! The HUD (top bar) reports resident full-res textures + estimated VRAM so you
//! can watch memory stay bounded while cycling. Benchmark this against the
//! shipping app on real 24MP files; expect the grid + memory to win and the
//! loupe to roughly break even (the webview already GPU-decodes single images).
//!
//! Keys: ←/→ navigate · Q fit↔100% · G grid · E loupe · click thumb → loupe ·
//! click image → toggle zoom · drag (zoomed) → pan.

mod loader;
mod scan;

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use loader::{Kind, Loader};
use scan::Entry;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const WINDOW: usize = 6; // ±N full-res frames kept resident around the current one
const CELL: Vec2 = Vec2::new(176.0, 132.0);
const ACCENT: Color32 = Color32::from_rgb(91, 191, 106);
const GRID_SCROLL_GAIN: f32 = 3.0; // multiplies trackpad/pixel scroll in the grid

// Lucide icon glyphs (private-use codepoints baked into assets/lucide.ttf).
const IC_OPEN: &str = "\u{e247}"; // folder-open
const IC_GRID: &str = "\u{e0ff}"; // layout-grid
const IC_LOUPE: &str = "\u{e0f6}"; // image
const IC_SEED: &str = "\u{e412}"; // sparkles

#[derive(PartialEq)]
enum View {
    Grid,
    Loupe,
}

/// In-memory only — the spike has no sidecar. Enough to drive the timeline.
#[derive(Clone, Copy, Default)]
struct Rating {
    stars: u8,
    reject: bool,
}

struct App {
    entries: Vec<Entry>,
    loader: Loader,

    thumb_tex: HashMap<usize, TextureHandle>,
    thumb_requested: HashSet<usize>,
    full_tex: HashMap<usize, TextureHandle>,
    full_requested: HashSet<usize>,

    ratings: HashMap<usize, Rating>,
    bin_minutes: i64, // 5 / 10 / 15 — timeline-local control
    threshold: u8,    // coverage counts stars >= this; independent of any view filter

    current: usize,
    view: View,
    zoom_100: bool,
    pan: Vec2,
}

impl App {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            loader: Loader::new(),
            thumb_tex: HashMap::new(),
            thumb_requested: HashSet::new(),
            full_tex: HashMap::new(),
            full_requested: HashSet::new(),
            ratings: HashMap::new(),
            bin_minutes: 10,
            threshold: 3,
            current: 0,
            view: View::Grid,
            zoom_100: false,
            pan: Vec2::ZERO,
        }
    }

    /// Deterministic pseudo-random ratings so the timeline is instantly
    /// populated for visual evaluation (≈30% left unrated, some rejects, a
    /// spread of stars → exercises gaps, partials, and fully-covered bins).
    fn seed_demo(&mut self) {
        self.ratings.clear();
        for i in 0..self.entries.len() {
            let h = (i.wrapping_mul(2_654_435_761)) ^ (i >> 3);
            let r = match h % 10 {
                0 | 1 | 2 => continue, // leave unrated
                3 => Rating { stars: 0, reject: true },
                4 | 5 => Rating { stars: 2, reject: false },
                6 | 7 => Rating { stars: 3, reject: false },
                8 => Rating { stars: 4, reject: false },
                _ => Rating { stars: 5, reject: false },
            };
            self.ratings.insert(i, r);
        }
    }

    fn open(&mut self, folder: PathBuf) {
        self.entries = scan::scan_folder(&folder);
        // Dropping the texture maps frees every GPU texture from the prior folder.
        self.thumb_tex.clear();
        self.thumb_requested.clear();
        self.full_tex.clear();
        self.full_requested.clear();
        self.ratings.clear();
        self.current = 0;
        self.zoom_100 = false;
        self.pan = Vec2::ZERO;
        self.view = View::Grid;
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

        // --- input → intents (collected, then applied to avoid borrow churn) ---
        let (mut left, mut right, mut q, mut g, mut e) = (false, false, false, false, false);
        let mut set_star: Option<u8> = None;
        let mut toggle_reject = false;
        ctx.input(|i| {
            left = i.key_pressed(egui::Key::ArrowLeft);
            right = i.key_pressed(egui::Key::ArrowRight);
            q = i.key_pressed(egui::Key::Q);
            g = i.key_pressed(egui::Key::G);
            e = i.key_pressed(egui::Key::E);
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
        });
        let n = self.entries.len();
        if n > 0 {
            if left {
                self.current = if self.current == 0 { n - 1 } else { self.current - 1 };
            }
            if right {
                self.current = (self.current + 1) % n;
            }
            if let Some(s) = set_star {
                self.ratings.entry(self.current).or_default().stars = s;
            }
            if toggle_reject {
                let r = self.ratings.entry(self.current).or_default();
                r.reject = !r.reject;
            }
        }
        if q {
            self.zoom_100 = !self.zoom_100;
            self.pan = Vec2::ZERO;
        }
        if g {
            self.view = View::Grid;
        }
        if e {
            self.view = View::Loupe;
        }

        self.maintain_window();

        // ----------------------------- top HUD --------------------------------
        // Panel padding lives on the Frame, independent of the global spacing.
        let bar_frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(14.0, 10.0));
        egui::TopBottomPanel::top("bar").frame(bar_frame).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let icon = |s: &str| egui::RichText::new(s).size(18.0);
                if ui.button(icon(IC_OPEN)).on_hover_text("Open folder").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.open(dir);
                    }
                }
                ui.separator();
                ui.selectable_value(&mut self.view, View::Grid, icon(IC_GRID))
                    .on_hover_text("Grid (G)");
                ui.selectable_value(&mut self.view, View::Loupe, icon(IC_LOUPE))
                    .on_hover_text("Loupe (E)");
                ui.separator();
                if ui.button(icon(IC_SEED)).on_hover_text("Seed demo ratings").clicked() {
                    self.seed_demo();
                }
                ui.separator();

                let resident = self.full_tex.len();
                let mb: f64 = self
                    .full_tex
                    .values()
                    .map(|t| {
                        let [w, h] = t.size();
                        (w * h * 4) as f64
                    })
                    .sum::<f64>()
                    / 1e6;
                let fps = 1.0 / ctx.input(|i| i.stable_dt).max(1e-4);
                ui.monospace(format!(
                    "{} imgs · {}/{} · full-res resident: {} (~{:.0} MB) · thumbs: {} · {:.0} fps",
                    n,
                    if n == 0 { 0 } else { self.current + 1 },
                    n,
                    resident,
                    mb,
                    self.thumb_tex.len(),
                    fps,
                ));
            });
        });

        // --------------------------- coverage timeline ------------------------
        // Bottom panel declared before the central panel so it reserves its
        // strip and the content area takes whatever remains.
        if !self.entries.is_empty() {
            let tl_frame = egui::Frame::side_top_panel(&ctx.style())
                .inner_margin(egui::Margin::symmetric(14.0, 10.0));
            egui::TopBottomPanel::bottom("timeline").frame(tl_frame).show(ctx, |ui| {
                self.timeline(ui);
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

        // Reactive repaint: keep polling the decode channel only while work is
        // outstanding, then go fully idle (no busy-loop, no battery drain).
        if !self.thumb_requested.is_empty() || !self.full_requested.is_empty() {
            ctx.request_repaint();
        }
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
        let n = self.entries.len();
        let spacing = 14.0; // gutter between thumbnail cells
        // Make the real inter-cell gap match the gap used for the column math.
        ui.spacing_mut().item_spacing = Vec2::splat(spacing);
        let avail_w = ui.available_width();
        let cols = (((avail_w + spacing) / (CELL.x + spacing)).floor() as usize).max(1);
        let rows = n.div_ceil(cols);
        let row_h = CELL.y + spacing;
        let current = self.current;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            row_h,
            rows,
            |ui, row_range| {
                for row in row_range {
                    ui.horizontal(|ui| {
                        for col in 0..cols {
                            let idx = row * cols + col;
                            if idx >= n {
                                break;
                            }
                            // Lazily request the thumbnail the first time the
                            // cell scrolls into view (and only once).
                            if !self.thumb_tex.contains_key(&idx)
                                && self.thumb_requested.insert(idx)
                            {
                                let e = &self.entries[idx];
                                self.loader.request_thumb(idx, e.path.clone(), e.orientation);
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
                            if idx == current {
                                painter.rect_stroke(rect, 2.0, Stroke::new(2.0, ACCENT));
                            }
                            if resp.clicked() {
                                self.current = idx;
                                self.view = View::Loupe;
                                self.zoom_100 = false;
                                self.pan = Vec2::ZERO;
                            }
                        }
                    });
                }
            },
        );
    }

    fn loupe(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::BLACK);

        if self.zoom_100 && resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.clicked() {
            self.zoom_100 = !self.zoom_100;
            self.pan = Vec2::ZERO;
        }

        match self.full_tex.get(&self.current) {
            Some(tex) if !self.zoom_100 => paint_fit(&painter, tex, rect),
            Some(tex) => {
                // 100%: one texel per screen pixel, panned. painter_at clips the
                // overflow to the panel.
                let [tw, th] = tex.size();
                let size = Vec2::new(tw as f32, th as f32);
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

        let name = &self.entries[self.current].name;
        painter.text(
            rect.left_bottom() + Vec2::new(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{}  {}", name, if self.zoom_100 { "100%" } else { "fit" }),
            egui::FontId::monospace(13.0),
            Color32::from_gray(180),
        );
    }

    /// The signature feature, rebuilt to stress-test egui's custom-widget story:
    /// two independent controls, density+coverage bars, gap highlighting, the
    /// three-state ready strip, a current-position marker, per-bin hover tooltip
    /// and click-to-jump — all hand-painted over one allocated rect.
    fn timeline(&mut self, ui: &mut egui::Ui) {
        // --- controls (standard widgets) -------------------------------------
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("tl_threshold")
                .selected_text(format!("≥ {}★", self.threshold))
                .show_ui(ui, |ui| {
                    for s in 1..=5u8 {
                        ui.selectable_value(&mut self.threshold, s, format!("{s}★"));
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
        for (idx, e) in self.entries.iter().enumerate() {
            let b = (((e.capture_time - start) / bin_sec) as usize).min(count - 1);
            let r = self.ratings.get(&idx).copied().unwrap_or_default();
            let bin = &mut bins[b];
            bin.total += 1;
            if bin.first.is_none() {
                bin.first = Some(idx);
            }
            if r.stars >= self.threshold {
                bin.covered += 1;
            }
            if r.stars > 0 || r.reject {
                bin.handled += 1;
            }
        }
        let current_bin = self
            .entries
            .get(self.current)
            .map(|e| (((e.capture_time - start) / bin_sec) as usize).min(count - 1));

        // --- paint ------------------------------------------------------------
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 96.0),
            Sense::click(),
        );
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
                if gap { Color32::from_rgb(74, 42, 42) } else { Color32::from_gray(58) },
            );
            if b.covered > 0 {
                let fh = (b.covered as f32 / b.total as f32) * th;
                let fill = Rect::from_min_max(
                    Pos2::new(track.left(), track.bottom() - fh),
                    Pos2::new(track.right(), track.bottom()),
                );
                painter.rect_filled(fill, 1.0, ACCENT);
            }

            // Ready strip: gray ring = none rated, green dot = all rated, else
            // the count still left to rate (a partial fill ring when too narrow
            // for text — adaptive rendering egui handles trivially).
            let c = Pos2::new(cx, strip_y);
            if b.handled == 0 {
                painter.circle_stroke(c, 4.0, Stroke::new(1.0, Color32::from_gray(110)));
            } else if b.handled == b.total {
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
        fonts.families.entry(family).or_default().push("lucide".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "photo-culler · egui spike",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new()) as Box<dyn eframe::App>)
        }),
    )
}
