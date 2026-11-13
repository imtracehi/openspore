//! The retail star objects of the galaxy view: invisible hit proxies, not drawn stars
//! (helper `space-stars2`, `notes/space-stars2-notes.md`).
//!
//! Decoded from the code image (march2017 addresses, `swarm-volume\target\re\dis.py`):
//!
//! * The UFO (`cSimulatorPlayerUFO`, constructor 0x10005E0) creates the model world `"Star"` (id
//!   0x56723400, kept at UFO+0x6C) and fills it with **1,000 models** loaded from the model property
//!   list **0x02E81370** (groups 0x40802000 / 0x5C4FCE4C in `Spore_Graphics.package`: name `Unit
//!   Sphere`, bounding radius 1, bounding box +-1 and no mesh), all invisible (`SetVisible(false)`) and
//!   kept in a vector at UFO+0x58. They are the "star objects" the galaxy pointer hits; nothing of a
//!   star is drawn by them (the stars of the galaxy view are the `sg_galaxy` particles: `stars.rs`,
//!   `disks.rs`).
//! * 0x1001200 (called after every jump, 0x10022DA / 0x102128C / 0x102188A / 0x106D3A2) assigns the pool:
//!   first the stars the star manager keeps in its list at `cStarManager+0xF0` (the saved-game stars)
//!   except the active one, then `cStarManager::FindStars` around the ship's star with the filter
//!   (every type 0x1FFF, every tech level 0x3F) and `maxDistance = UFO.mMaxScanDistance` (+0x4C: the
//!   `spaceToolDetectableRange` 20 of the best interstellar drive, 5.0 without one). Each model sits
//!   at the star's position, owned by the star record (model+0x64, cast to `cStarRecord` 0x3E4353B),
//!   scale 0.2 at first, visible to the ray query.
//! * 0xFFCEA0 (per pick, when the camera moved by more than 0.03): the model's transform scale is
//!   `min(max(base, lo x d), hi x d)` with `d` the camera-to-star distance; the galactic core
//!   (`cStarRecord::mType == 1`) takes `(0.3, 0.02, 0.3)`, every other star `(0.1, 0.015, 0.03)`
//!   (constants 0x15B6230 .. 0x15B6240, read only). The scale of a unit sphere is its radius: the hit
//!   target keeps about the same size on screen at every zoom.
//! * 0xFFF780 (the pick, galaxy context only; callers: the click handler 0xFDC5CD for the command 0x3E8,
//!   the hover 0x106CA71 and the UI update 0x1074640): the pointer ray (0x7C47C0) is clamped to 1,000
//!   units and handed to the model world (vtable +0x34) which returns the models it hits; the first
//!   hit whose star record has `kStarFlagSaveGame` (0x2) or whose star is closer to the camera than 100
//!   units (`dist^2 < 10000`, 0xFFFA46) is the picked star. The click (0xFDC5CD): an Empire star
//!   (tech level 5) starts the hail, any other star but the active one runs the reach check
//!   (0xFFC780) and the jump (0x1002040).
//!
//! ADAPTED: which stars carry the saved flag (the home star, the ship's star and the visited stars,
//! as `hover.rs`), whether `FindStars` returns the star it is centred on (it is included), and the
//! order of the hits (nearest along the ray first: the model world's own order is not decoded).

use bevy::prelude::*;

use super::hover::{pick_accepted, RAY_LENGTH};

/// `UFO+0x58`: the number of hit proxies the UFO keeps (0x1000774: `push 0x3E8`).
pub const POOL_SIZE: usize = 1000;
/// Bounding radius of the proxy model `Unit Sphere` (property 0xF9EFB9 = 1.0): the scale is the radius.
pub const MODEL_RADIUS: f32 = 1.0;
/// The scale of a freshly assigned proxy (0x13EC45C); the pick resizes it at once (`proxy_scale`).
#[allow(dead_code)]
pub const FRESH_SCALE: f32 = 0.2;

/// 0xFFCEA0: (base, lo, hi) of the proxy scale for the galactic core, then for every other star.
const CORE: (f32, f32, f32) = (0.3, 0.02, 0.3);
const OTHER: (f32, f32, f32) = (0.1, 0.015, 0.03);

/// 0xFFCEA0: the scale (= hit radius) of a star's proxy at a camera distance.
pub fn proxy_scale(distance: f32, core: bool) -> f32 {
    let (base, lo, hi) = if core { CORE } else { OTHER };
    base.max(lo * distance).min(hi * distance)
}

/// One star of the pool as the pick sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub index: u32,
    /// Position in the map layer's space (galaxy x, 0, galaxy y).
    pub position: Vec3,
    /// `cStarRecord::mType == 1`.
    pub core: bool,
    /// `kStarFlagSaveGame`.
    pub saved: bool,
}

/// The distance along the ray (clamped to [`RAY_LENGTH`]) at which it meets a candidate's proxy sphere,
/// when it does.
pub fn ray_hit(origin: Vec3, dir: Vec3, candidate: &Candidate) -> Option<f32> {
    let distance = origin.distance(candidate.position);
    let radius = proxy_scale(distance, candidate.core) * MODEL_RADIUS;
    let along = (candidate.position - origin).dot(dir).clamp(0.0, RAY_LENGTH);
    let closest = origin + dir * along;
    (closest.distance(candidate.position) <= radius).then_some(along)
}

/// 0xFFF780 over a pool: the hit stars nearest along the ray first (ADAPTED order), the first one the
/// acceptance rule (0xFFFA46) lets through.
pub fn pick(origin: Vec3, dir: Vec3, pool: &[Candidate]) -> Option<u32> {
    let mut hits: Vec<(f32, &Candidate)> = pool
        .iter()
        .filter_map(|c| ray_hit(origin, dir, c).map(|t| (t, c)))
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.index.cmp(&b.1.index)));
    hits.into_iter()
        .find(|(_, c)| pick_accepted(origin.distance(c.position), c.saved))
        .map(|(_, c)| c.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(index: u32, x: f32, z: f32, saved: bool) -> Candidate {
        Candidate { index, position: Vec3::new(x, 0.0, z), core: false, saved }
    }

    #[test]
    fn the_proxy_scale_follows_the_constants_of_0xffcea0() {
        // Near the camera the base size, then a share of the distance, capped at a larger share.
        assert_eq!(proxy_scale(1.0, false), 0.03);
        assert_eq!(proxy_scale(5.0, false), 0.1);
        assert!((proxy_scale(200.0, false) - 3.0).abs() < 1e-5);
        assert!((proxy_scale(1200.0, false) - 18.0).abs() < 1e-4);
        assert_eq!(proxy_scale(0.5, true), 0.15);
        assert_eq!(proxy_scale(10.0, true), 0.3);
        assert!((proxy_scale(200.0, true) - 4.0).abs() < 1e-5);
        assert!(proxy_scale(0.0, false).abs() < 1e-9);
    }

    #[test]
    fn a_ray_hits_the_proxy_sphere_of_the_star_it_passes() {
        let eye = Vec3::new(0.0, 200.0, 0.0);
        let down = Vec3::NEG_Y;
        // Straight below: a hit at the distance of the star.
        let hit = ray_hit(eye, down, &star(1, 0.0, 0.0, true)).unwrap();
        assert!((hit - 200.0).abs() < 1e-3);
        // 2.9 units aside is inside the 3 unit sphere, 3.1 outside.
        assert!(ray_hit(eye, down, &star(1, 2.9, 0.0, true)).is_some());
        assert!(ray_hit(eye, down, &star(1, 3.1, 0.0, true)).is_none());
        // Behind the camera nothing is hit (the closest point of the segment is the eye).
        assert!(ray_hit(eye, Vec3::Y, &star(1, 0.0, 0.0, true)).is_none());
        // Beyond the 1,000 unit segment only the end point counts.
        let far = Vec3::new(0.0, -2000.0, 0.0);
        let beyond = Candidate { index: 2, position: far, core: false, saved: true };
        assert!(ray_hit(eye, down, &beyond).is_none());
    }

    #[test]
    fn the_acceptance_rule_keeps_far_stars_out_unless_saved() {
        // 200 units out only the saved star counts, 50 units out both do.
        let pool = [star(1, 0.0, 0.0, false), star(2, 0.0, 0.0, true)];
        assert_eq!(pick(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y, &pool), Some(2));
        assert_eq!(pick(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y, &pool[..1]), None);
        assert_eq!(pick(Vec3::new(0.0, 50.0, 0.0), Vec3::NEG_Y, &pool[..1]), Some(1));
    }

    #[test]
    fn the_nearest_hit_along_the_ray_wins() {
        let eye = Vec3::new(0.0, 60.0, 0.0);
        // Two stars on the same ray (a tilted view): the nearer one is picked.
        let dir = Vec3::new(0.0, -1.0, 0.0);
        let near = Candidate { index: 7, position: Vec3::new(0.0, 0.0, 0.0), core: false, saved: false };
        let far = Candidate { index: 3, position: Vec3::new(0.0, -30.0, 0.0), core: false, saved: false };
        assert_eq!(pick(eye, dir, &[far, near]), Some(7));
        // A miss by a wide margin picks nothing.
        assert_eq!(pick(eye, dir, &[star(9, 40.0, 0.0, true)]), None);
    }
}
