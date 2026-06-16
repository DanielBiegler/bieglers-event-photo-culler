//! Core data model: ratings, view mode, and the filter predicate. The filter
//! logic mirrors the React app's `passes` (App.tsx) exactly so behavior matches.

use serde::{Deserialize, Serialize};

/// Persisted per image. Serializes to `{ "stars": n, "reject": bool }` to match
/// the existing `.cull.json` sidecar.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Rating {
    #[serde(default)]
    pub stars: u8,
    #[serde(default)]
    pub reject: bool,
}

impl Rating {
    /// Reviewed = rated or rejected (drives timeline ready-strip + HUD counts).
    pub fn handled(&self) -> bool {
        self.stars > 0 || self.reject
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Grid,
    Loupe,
}

/// How the star filter compares the rating against the threshold.
#[derive(PartialEq, Clone, Copy)]
pub enum StarFilterMode {
    Gte, // at least
    Eq,  // exactly
    Lt,  // less than
}

#[derive(PartialEq, Clone, Copy)]
pub enum RejectFilter {
    All,
    Hide,
    Only,
}

/// Does this rating pass the active filter? `None` is treated as 0 stars /
/// not rejected. Mirrors App.tsx `passes` (the star comparison, then the
/// reject filter).
pub fn passes(
    rating: Option<&Rating>,
    min_stars: u8,
    mode: StarFilterMode,
    reject_filter: RejectFilter,
) -> bool {
    let stars = rating.map(|r| r.stars).unwrap_or(0);
    let reject = rating.map(|r| r.reject).unwrap_or(false);

    let star_fail = match mode {
        StarFilterMode::Eq => stars != min_stars,
        StarFilterMode::Lt => stars >= min_stars,
        StarFilterMode::Gte => stars < min_stars,
    };
    if star_fail {
        return false;
    }
    match reject_filter {
        RejectFilter::Hide if reject => false,
        RejectFilter::Only if !reject => false,
        _ => true,
    }
}
