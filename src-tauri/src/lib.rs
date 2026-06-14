use base64::Engine;
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageEntry {
    name: String,
    path: String,
    /// Epoch seconds, used for chronological sort + timeline binning.
    capture_time: i64,
    /// "exif" when read from DateTimeOriginal, "mtime" when falling back.
    capture_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    folder: String,
    images: Vec<ImageEntry>,
    /// Raw contents of the folder's .cull.json sidecar, if present.
    sidecar: Option<String>,
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

/// Reads EXIF DateTimeOriginal as a pseudo-UTC epoch. Absolute timezone is
/// irrelevant here: we only need consistent ordering and bucketing.
fn read_capture_time(path: &Path) -> Option<i64> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let raw = match &field.value {
        exif::Value::Ascii(vals) => vals.first().map(|b| String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }?;
    let dt = chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y:%m:%d %H:%M:%S").ok()?;
    Some(dt.and_utc().timestamp())
}

/// Extracts the embedded EXIF thumbnail JPEG bytes without decoding the full image.
fn embedded_thumbnail(path: &Path) -> Option<Vec<u8>> {
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
    // EXIF offsets are relative to the start of the TIFF header (== buf start).
    buf.get(offset..offset.checked_add(len)?).map(|s| s.to_vec())
}

/// Fallback for files lacking an embedded thumbnail (e.g. stripped on export):
/// decode and downscale. Rare on camera-original JPEGs.
fn generate_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let img = image::open(path).ok()?;
    let thumb = img.thumbnail(400, 400);
    let mut out = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

#[tauri::command]
fn scan_folder(folder: String) -> Result<ScanResult, String> {
    let dir = Path::new(&folder);
    let paths: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_jpeg(p))
        .collect();

    let mut images: Vec<ImageEntry> = paths
        .par_iter()
        .map(|p| {
            let (capture_time, capture_source) = match read_capture_time(p) {
                Some(t) => (t, "exif"),
                None => (mtime_secs(p).unwrap_or(0), "mtime"),
            };
            ImageEntry {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p.to_string_lossy().into_owned(),
                capture_time,
                capture_source: capture_source.to_string(),
            }
        })
        .collect();

    images.sort_by(|a, b| a.capture_time.cmp(&b.capture_time).then_with(|| a.name.cmp(&b.name)));

    let sidecar = fs::read_to_string(dir.join(".cull.json")).ok();
    Ok(ScanResult { folder, images, sidecar })
}

#[tauri::command]
fn get_thumbnail(path: String) -> Option<String> {
    let p = Path::new(&path);
    let bytes = embedded_thumbnail(p).or_else(|| generate_thumbnail(p))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:image/jpeg;base64,{b64}"))
}

/// Atomically writes the folder's .cull.json (temp file + rename).
#[tauri::command]
fn save_sidecar(folder: String, contents: String) -> Result<(), String> {
    let dir = Path::new(&folder);
    let target = dir.join(".cull.json");
    let tmp = dir.join(".cull.json.tmp");
    fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn export_csv(dest: String, contents: String) -> Result<(), String> {
    fs::write(&dest, contents).map_err(|e| e.to_string())
}

/// Decodes a JPEG and places it on the OS clipboard as raw RGBA image data.
/// Runs on a blocking thread so the heavy decode never stalls the UI thread.
#[tauri::command]
async fn copy_image(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let img = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = img.dimensions();
        let data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: std::borrow::Cow::Owned(img.into_raw()),
        };
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_image(data))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_folder,
            get_thumbnail,
            save_sidecar,
            export_csv,
            copy_image
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
