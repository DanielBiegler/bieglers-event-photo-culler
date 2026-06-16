//! Persistence: the per-folder `.cull.json` ratings sidecar and a small global
//! resume config. Ported from the Tauri app's `save_sidecar` (atomic temp +
//! rename) and its `localStorage` resume logic, made native.

use crate::model::Rating;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// On-disk sidecar shape, identical to the Tauri app's so existing
/// `.cull.json` files load unchanged: `{ "version": 1, "files": { name: {..} } }`.
#[derive(Serialize, Deserialize)]
pub struct Sidecar {
    pub version: u32,
    pub files: HashMap<String, Rating>,
}

const SIDECAR_NAME: &str = ".cull.json";

/// Read + parse a folder's `.cull.json` into a ratings map. Tolerant of a
/// missing/malformed file — returns empty so a bad sidecar never blocks culling.
pub fn load_ratings(folder: &Path) -> HashMap<String, Rating> {
    fs::read_to_string(folder.join(SIDECAR_NAME))
        .ok()
        .and_then(|json| serde_json::from_str::<Sidecar>(&json).ok())
        .map(|s| s.files)
        .unwrap_or_default()
}

/// Drop blank entries (no stars, not rejected) so the sidecar stays small —
/// mirrors the React app's `pruneRatings`.
fn prune(ratings: &HashMap<String, Rating>) -> HashMap<String, Rating> {
    ratings
        .iter()
        .filter(|(_, r)| r.stars > 0 || r.reject)
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// Atomically write the folder's `.cull.json` (temp file + rename), pruning
/// blanks first. Cheap enough to call on a background thread.
pub fn save_sidecar(folder: &Path, ratings: &HashMap<String, Rating>) -> std::io::Result<()> {
    let sidecar = Sidecar { version: 1, files: prune(ratings) };
    let json = serde_json::to_string(&sidecar).unwrap_or_else(|_| "{}".to_string());
    let target = folder.join(SIDECAR_NAME);
    let tmp = folder.join(".cull.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &target)
}

// --------------------------- resume config --------------------------------

/// Replaces the Tauri app's `localStorage`: remembers the last folder and the
/// last-viewed image per folder so the app reopens where you left off.
#[derive(Serialize, Deserialize, Default)]
pub struct ResumeConfig {
    pub last_folder: Option<String>,
    /// folder path → last-viewed image filename.
    #[serde(default)]
    pub last_image: HashMap<String, String>,
}

fn config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "chimpify", "photo-culler")?;
    Some(dirs.config_dir().join("resume.json"))
}

impl ResumeConfig {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn remember(&mut self, folder: &str, image: &str) {
        self.last_folder = Some(folder.to_string());
        self.last_image.insert(folder.to_string(), image.to_string());
    }
}
