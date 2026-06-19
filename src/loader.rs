//! Background decode pool. The whole point of the spike: decode JPEGs off the
//! UI thread in Rust and hand finished pixel buffers to the main thread, which
//! uploads them to GPU textures it explicitly owns and evicts. No webview, no
//! base64, no asset protocol.
//!
//! Two independent worker pools so a flood of cheap thumbnail jobs can never
//! starve the expensive full-res decodes that the loupe is waiting on.

use crate::scan;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

pub enum Kind {
    Thumb,
    Full,
}

pub struct Loaded {
    pub index: usize,
    pub kind: Kind,
    /// Decoded on the worker thread — `ColorImage` is just `Send` pixel data;
    /// the actual GPU upload happens on the UI thread via `ctx.load_texture`.
    pub image: egui::ColorImage,
}

/// A decode job: which image, where, and its EXIF orientation.
type Job = (usize, PathBuf, u16);

pub struct Loader {
    thumb_tx: Sender<Job>,
    full_tx: Sender<Job>,
    pub out_rx: Receiver<Loaded>,
}

impl Loader {
    pub fn new() -> Self {
        let (out_tx, out_rx) = channel::<Loaded>();
        let thumb_tx = spawn_pool(3, out_tx.clone(), Kind::Thumb);
        let full_tx = spawn_pool(2, out_tx, Kind::Full);
        Loader {
            thumb_tx,
            full_tx,
            out_rx,
        }
    }

    pub fn request_thumb(&self, index: usize, path: PathBuf, orientation: u16) {
        let _ = self.thumb_tx.send((index, path, orientation));
    }

    pub fn request_full(&self, index: usize, path: PathBuf, orientation: u16) {
        let _ = self.full_tx.send((index, path, orientation));
    }
}

/// Spawns `n` workers sharing one job queue, all decoding the given kind.
fn spawn_pool(n: usize, out_tx: Sender<Loaded>, kind: Kind) -> Sender<Job> {
    let (job_tx, job_rx) = channel::<Job>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let is_thumb = matches!(kind, Kind::Thumb);
    for _ in 0..n {
        let job_rx = Arc::clone(&job_rx);
        let out_tx = out_tx.clone();
        thread::spawn(move || loop {
            // Hold the lock only long enough to pull one job.
            let job = {
                let guard = job_rx.lock().unwrap();
                guard.recv()
            };
            let Ok((index, path, orientation)) = job else {
                break;
            };
            let decoded = if is_thumb {
                decode_thumb(&path, orientation)
            } else {
                decode_full(&path, orientation)
            };
            if let Some(image) = decoded {
                let kind = if is_thumb { Kind::Thumb } else { Kind::Full };
                let _ = out_tx.send(Loaded { index, kind, image });
            }
        });
    }
    job_tx
}

/// Rotate/flip the decoded image per its EXIF orientation tag — the step the
/// webview does for free and a native decode skips. Most cameras only ever
/// emit 1/3/6/8, but we handle the mirrored 2/4/5/7 cases too.
pub fn apply_orientation(img: &mut image::DynamicImage, orientation: u16) {
    *img = match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(), // transpose
        6 => img.rotate90(),
        7 => img.rotate270().fliph(), // transverse
        8 => img.rotate270(),
        _ => return, // 1 (normal) or unknown → leave as-is
    };
}

fn to_color(img: image::RgbaImage) -> egui::ColorImage {
    let (w, h) = img.dimensions();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())
}

/// Embedded EXIF thumbnail when present (the ~1ms, no-full-decode happy path);
/// otherwise decode + downscale as a fallback for stripped JPEGs. The embedded
/// thumbnail shares the main image's orientation, so we apply it either way.
fn decode_thumb(path: &Path, orientation: u16) -> Option<egui::ColorImage> {
    if let Some(bytes) = scan::embedded_thumbnail(path) {
        if let Ok(mut img) = image::load_from_memory(&bytes) {
            apply_orientation(&mut img, orientation);
            return Some(to_color(img.to_rgba8()));
        }
    }
    let mut img = image::open(path).ok()?.thumbnail(256, 256);
    apply_orientation(&mut img, orientation);
    Some(to_color(img.to_rgba8()))
}

/// Full-resolution decode — the heavy 24MP path the loupe window depends on.
fn decode_full(path: &Path, orientation: u16) -> Option<egui::ColorImage> {
    let mut img = image::open(path).ok()?;
    apply_orientation(&mut img, orientation);
    Some(to_color(img.to_rgba8()))
}
