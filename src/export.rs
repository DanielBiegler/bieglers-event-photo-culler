//! Keeper CSV export. Ported from the Tauri app's `export_csv` + the React
//! `onExport`: one row per image at/above the keeper threshold, `basename,stars`,
//! used to locate the matching RAWs on the SD card.

use crate::model::Rating;
use crate::scan::Entry;
use std::collections::HashMap;
use std::path::Path;

pub fn export_keepers(
    dest: &Path,
    entries: &[Entry],
    ratings: &HashMap<String, Rating>,
    threshold: u8,
) -> std::io::Result<()> {
    let mut out = String::from("filename,stars\n");
    for e in entries {
        let stars = ratings.get(&e.name).map(|r| r.stars).unwrap_or(0);
        if stars >= threshold {
            out.push_str(&e.name);
            out.push(',');
            out.push_str(&stars.to_string());
            out.push('\n');
        }
    }
    std::fs::write(dest, out)
}
