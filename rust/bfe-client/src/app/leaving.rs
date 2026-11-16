//! The player's LEAVING BATTLEFIELD warning: the map's boundary from its
//! `ai-nav.json` (`boundary.references` name `paths`, each a polygon), tested against the hull or body
//! the player controls, and the warning's flashing alpha as the client's page works it out. The world
//! keeps the 10 s count and does the killing; this is only what the player sees.
use bfe_presentation::{leaving_warning::Warning, page_layout::warning as w};
use bfe_sim::ai::nav::{boundary::Boundary, math::Vec3};
use std::path::Path;

pub(super) struct LeavingFeed {
    boundary: Boundary,
    warning: Warning,
}

impl Default for LeavingFeed {
    fn default() -> Self {
        Self { boundary: Boundary::default(), warning: Warning::default() }
    }
}

/// The polygons `boundaryFromNav` takes from a map's `ai-nav.json`: a path per boundary reference, missing
/// paths skipped; points are `[x, y, z, ...]` arrays or `{x, z}` objects.
pub(super) fn polygons(nav: &serde_json::Value) -> Vec<Vec<Vec3>> {
    let paths = nav["paths"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();
    for reference in nav["boundary"]["references"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let Some(index) = reference["path"].as_u64() else { continue };
        let Some(points) = paths.get(index as usize).and_then(|p| p["points"].as_array()).filter(|p| !p.is_empty()) else {
            continue;
        };
        out.push(
            points
                .iter()
                .map(|p| match p.as_array() {
                    Some(a) => Vec3 { x: a[0].as_f64().unwrap_or(0.), y: a[1].as_f64().unwrap_or(0.), z: a[2].as_f64().unwrap_or(0.) },
                    None => Vec3 { x: p["x"].as_f64().unwrap_or(0.), y: 0., z: p["z"].as_f64().unwrap_or(0.) },
                })
                .collect(),
        );
    }
    out
}

impl LeavingFeed {
    /// The map's boundary; a map without one (both Yavin maps) has no outside.
    pub fn load(content_dir: &Path, map: &str) -> Self {
        let text = std::fs::read_to_string(content_dir.join("maps").join(map).join("ai-nav.json")).ok();
        let nav = text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let boundary = nav.map_or_else(Boundary::default, |nav| Boundary::new(&polygons(&nav)));
        Self { boundary, warning: Warning::default() }
    }

    /// One frame: `place` is where what the player controls is (the hull he sits in, else his body) while
    /// he is alive, `tick` the world tick on show. Returns the warning's alpha, 0 hidden.
    pub fn alpha(&mut self, tick: f64, place: Option<[f64; 3]>) -> f64 {
        let outside = place.is_some_and(|p| !self.boundary.empty() && self.boundary.outside(Vec3 { x: p[0], y: p[1], z: p[2] }));
        self.warning.row[w::TICK] = tick;
        self.warning.row[w::OUTSIDE] = f64::from(u8::from(outside));
        self.warning.frame();
        self.warning.row[w::ALPHA]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_outside_every_polygon_starts_the_flashing_warning() {
        let nav = serde_json::json!({
            "boundary": {"references": [{"path": 1}, {"path": 9}]},
            "paths": [{"points": []}, {"points": [[0., 0., 0.], [0., 0., 100.], [100., 0., 100.], [100., 0., 0.]]}],
        });
        let mut feed = LeavingFeed { boundary: Boundary::new(&polygons(&nav)), warning: Warning::default() };
        assert_eq!(feed.alpha(1., Some([50., 0., 50.])), 0.);
        let first = feed.alpha(2., Some([500., 0., 50.]));
        assert!(first > 0.9, "the warning starts near full: {first}");
        // The spring pulls it toward half over the following ticks.
        let later = feed.alpha(20., Some([500., 0., 50.]));
        assert!(later > 0. && later <= 1.);
        assert_eq!(feed.alpha(21., Some([50., 0., 50.])), 0.);
        assert_eq!(feed.alpha(22., None), 0.);
    }

    #[test]
    fn a_map_without_a_boundary_has_no_outside() {
        let mut feed = LeavingFeed::default();
        assert_eq!(feed.alpha(1., Some([1e9, 0., 1e9])), 0.);
    }
}
