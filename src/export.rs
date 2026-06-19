//! Keeper export. Writes a destination folder with two deliverables for the
//! photographer's RAW handoff:
//!   - `keepers.txt` — one stem (filename without extension) per line, for
//!     images at/above the keeper threshold; scripts append their own RAW
//!     extension to locate the matching RAWs on the SD card.
//!   - `<stem>.xmp` — a Lightroom/darktable-compatible sidecar per keeper that
//!     carries the star rating (`xmp:Rating`), so ratings made here transfer to
//!     darktable/Lightroom once the RAWs are loaded.

use crate::model::Rating;
use crate::scan::Entry;
use std::collections::HashMap;
use std::path::Path;

/// Write `keepers.txt` + one `.xmp` per keeper into `out_dir`. Returns the
/// number of keepers exported (for the toast).
pub fn export_keepers(
    out_dir: &Path,
    entries: &[Entry],
    ratings: &HashMap<String, Rating>,
    threshold: u8,
) -> std::io::Result<usize> {
    let mut list = String::new();
    let mut count = 0usize;
    for e in entries {
        let stars = ratings.get(&e.name).map(|r| r.stars).unwrap_or(0);
        if stars < threshold {
            continue;
        }
        // Stem = filename without extension (IMG_1234.JPG -> IMG_1234), so the
        // list and sidecar names compose with any RAW extension. Fall back to
        // the full name if there's somehow no stem.
        let stem = Path::new(&e.name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
        let stem = stem.as_deref().unwrap_or(&e.name);

        list.push_str(stem);
        list.push('\n');

        // Append `.xmp` rather than `with_extension` so a stem that itself
        // contains a dot (`photo.2`) isn't truncated to `photo.xmp`.
        std::fs::write(out_dir.join(format!("{stem}.xmp")), xmp_for(stars))?;
        count += 1;
    }
    std::fs::write(out_dir.join("keepers.txt"), list)?;
    Ok(count)
}

/// Minimal Lightroom/darktable-compatible XMP sidecar carrying a star rating.
fn xmp_for(stars: u8) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="bieglers-photo-filter">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/">
   <xmp:Rating>{stars}</xmp:Rating>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#
    )
}
