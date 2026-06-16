//! Copy the current image to the OS clipboard as raw RGBA. Ported from the
//! Tauri app's `copy_image` (arboard), now orientation-correct. Heavy decode —
//! always call from a background thread.

use std::borrow::Cow;
use std::path::Path;

pub fn copy_image(path: &Path, orientation: u16) -> Result<(), String> {
    let mut img = image::open(path).map_err(|e| e.to_string())?;
    crate::loader::apply_orientation(&mut img, orientation);
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let data = arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: Cow::Owned(rgba.into_raw()),
    };
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_image(data))
        .map_err(|e| e.to_string())
}
