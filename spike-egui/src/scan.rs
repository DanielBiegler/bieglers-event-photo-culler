//! Pure folder-scan + EXIF helpers, copied verbatim-in-spirit from
//! `src-tauri/src/lib.rs` (minus the `#[tauri::command]` wrappers and the
//! base64/IPC hop). This is the code a real rewrite would lift into a shared
//! `culler-core` crate so both the Tauri and native front-ends call it.

use rayon::prelude::*;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// Epoch seconds — chronological sort key (timeline binning not needed here).
    pub capture_time: i64,
    /// EXIF orientation tag (1–8, default 1). The webview auto-applies this; a
    /// native decode does not, so we carry it and rotate at decode time.
    pub orientation: u16,
}

fn is_jpeg(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("jpg") | Some("jpeg")
    )
}

fn mtime_secs(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// One EXIF parse per file → (capture_time, orientation). Capture time is
/// DateTimeOriginal as a pseudo-UTC epoch, falling back to mtime; orientation
/// defaults to 1 (normal) when absent.
fn read_exif_meta(path: &Path) -> (i64, u16) {
    if let Ok(file) = fs::File::open(path) {
        let mut reader = BufReader::new(file);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) {
            let time = exif
                .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
                .and_then(|f| match &f.value {
                    exif::Value::Ascii(vals) => {
                        vals.first().map(|b| String::from_utf8_lossy(b).into_owned())
                    }
                    _ => None,
                })
                .and_then(|raw| {
                    chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y:%m:%d %H:%M:%S").ok()
                })
                .map(|dt| dt.and_utc().timestamp());
            let orientation = exif
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                .map(|v| v as u16)
                .unwrap_or(1);
            return (time.or_else(|| mtime_secs(path)).unwrap_or(0), orientation);
        }
    }
    (mtime_secs(path).unwrap_or(0), 1)
}

/// Embedded EXIF thumbnail JPEG bytes, extracted without a full decode.
/// This is the byte stream that, in the Tauri app, gets base64'd and shipped
/// over IPC per item — the path this spike exists to eliminate.
pub fn embedded_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let offset = exif
        .get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let len = exif
        .get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let buf = exif.buf();
    buf.get(offset..offset.checked_add(len)?).map(|s| s.to_vec())
}

/// Parallel scan of one flat folder, chronological ascending. Mirrors the
/// main app's `scan_folder`, returning native structs instead of JSON.
pub fn scan_folder(folder: &Path) -> Vec<Entry> {
    let paths: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_jpeg(p)).collect(),
        Err(_) => return Vec::new(),
    };

    let mut images: Vec<Entry> = paths
        .par_iter()
        .map(|p| {
            let (capture_time, orientation) = read_exif_meta(p);
            Entry {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p.clone(),
                capture_time,
                orientation,
            }
        })
        .collect();

    images.sort_by(|a, b| a.capture_time.cmp(&b.capture_time).then_with(|| a.name.cmp(&b.name)));
    images
}
