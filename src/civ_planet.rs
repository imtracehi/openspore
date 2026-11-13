//! The Civilization stage on the original's planet (planet-re 4E step 6).
//!
//! Retail Civ plays on the same R = 500 sphere as the other stages, with its
//! tuning distances unscaled (`civ_rules::CIV_WORLD_SCALE` 1.0): cities 123-348
//! from the level origin and at least 180 apart (civ-re V4), the camera 221.8
//! from its anchor (V6). The stage spans the visible hemisphere, where the flat
//! chart of the earlier stages breaks down (planet-re 1E / 4C), so Civ works
//! in 3D:
//!
//! * [`Shape`]: the sphere's pure geometry (local up, the tangent frame at a
//!   point, the retail chord distance, steps along the surface). The campaign
//!   keeps it (`CivCampaign::sphere`) so pure campaign code measures exactly
//!   like the live systems.
//! * [`Globe`]: a `Shape` plus the height field (`ToSurface`, `IsInWater`,
//!   ray picking) for the systems.
//! * Positions are world points on the surface. Vehicles integrate their
//!   velocity in the tangent plane at their position (the frame is the
//!   minimal rotation from +Y to the local up) and `terrain::ground` puts every
//!   `terrain::Radial` entity back on `ToSurface` of its own radial line plus
//!   its clearance along the local up (planet-re 4C, the end state).
//! * [`lay_out`]: the cities (seaports at the coast) and the spice geysers on
//!   the planet by the retail placement markers; [`wild_spots`] for the epic
//!   predators and the animals.
//! * [`MapFrame`]: the planet map of the HUD minimap.
//!
//! Without a planet (headless tests, `OPENSPORE_FLAT_TERRAIN=1`) everything is
//! the plane with +Y up and planar distances, so the stage still plays.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::civ_rules;
use crate::planet_query::{PlanetFrame, StagePlanet};

/// ADAPTED: OpenSpore Civ distances without a retail value were tuned on the
/// island at `CIV_WORLD_SCALE` 28/180 (AI radii, the build reach); this puts
/// them back at retail scale.
pub const RETAIL_FROM_ISLAND: f32 = 180.0 / 28.0;
/// ADAPTED: the cities, vehicles and their effects were authored for the
/// island (the plot ring 4 units from the hall). The retail plot ring is
/// `RadiusFromCityHall` 15 from the hall, and a wall of `MinNumWallSegments` 5
/// with posts every `DistBetweenWallPosts` 28 has a circumradius of 23.8 (the
/// corner turrets sit 6.4 x 3.75 = 24 out): one factor draws them at that size.
pub const CITY_SCALE: f32 = civ_rules::RADIUS_FROM_CITY_HALL / 4.0;
/// `terrain::BASE_Y`: the floor of a world without a sampled surface.
const BASE_Y: f32 = -0.6;
/// Bumped when the placement rules change: campaigns laid out by an older
/// rule are laid out again (positions only).
pub const LAYOUT_VERSION: u64 = 2;

// ---------------------------------------------------------------------------
// Pure sphere geometry
// ---------------------------------------------------------------------------

/// The shortest rotation taking unit vector `a` to unit vector `b`, exact
/// for tiny angles too (`Quat::from_rotation_arc` returns the identity below
/// about 1.4e-3 rad, which loses small steps that add up).
pub fn arc(a: Vec3, b: Vec3) -> Quat {
    let axis = a.cross(b);
    let sin = axis.length();
    if sin < 1e-12 {
        return if a.dot(b) >= 0.0 {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_arc(a, b)
        };
    }
    Quat::from_axis_angle(axis / sin, sin.atan2(a.dot(b)))
}

/// The stage's surface shape: a sphere (centre, radius) or the flat plane.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shape {
    pub centre: Option<Vec3>,
    pub radius: f32,
}

impl Shape {
    pub const FLAT: Self = Self {
        centre: None,
        radius: 0.0,
    };

    pub fn sphere(centre: Vec3, radius: f32) -> Self {
        Self {
            centre: Some(centre),
            radius,
        }
    }

    /// From `CivCampaign::sphere` (centre xyz, radius).
    pub fn from_saved(saved: Option<[f32; 4]>) -> Self {
        match saved {
            Some(v) if v[3] > 1.0 && v.iter().all(|x| x.is_finite()) => {
                Self::sphere(Vec3::new(v[0], v[1], v[2]), v[3])
            }
            _ => Self::FLAT,
        }
    }

    pub fn saved(&self) -> Option<[f32; 4]> {
        self.centre.map(|c| [c.x, c.y, c.z, self.radius])
    }

    pub fn on_sphere(&self) -> bool {
        self.centre.is_some()
    }

    /// Local up at a world point (planet-re 1D: normalize(position)).
    pub fn up(&self, p: Vec3) -> Vec3 {
        match self.centre {
            Some(c) => (p - c).normalize_or(Vec3::Y),
            None => Vec3::Y,
        }
    }

    /// The tangent frame at `p`: the minimal rotation taking +Y to the local
    /// up (identity on the plane). Local x / z span the tangent plane.
    pub fn frame(&self, p: Vec3) -> Quat {
        match self.centre {
            Some(_) => arc(Vec3::Y, self.up(p)),
            None => Quat::IDENTITY,
        }
    }

    /// Retail gameplay distance: the chord between the two points' radial
    /// projections on the sphere (heights ignored, planet-re 1E); planar on
    /// the plane.
    pub fn distance(&self, a: Vec3, b: Vec3) -> f32 {
        match self.centre {
            Some(c) => ((a - c).normalize_or(Vec3::Y) - (b - c).normalize_or(Vec3::Y)).length() * self.radius,
            None => Vec2::new(a.x - b.x, a.z - b.z).length(),
        }
    }

    /// A world vector in the tangent frame at `p` (local x, z).
    pub fn to_tangent(&self, p: Vec3, v: Vec3) -> Vec2 {
        let local = self.frame(p).inverse() * v;
        Vec2::new(local.x, local.z)
    }

    /// A tangent-frame vector at `p` back in the world.
    pub fn from_tangent(&self, p: Vec3, v: Vec2) -> Vec3 {
        self.frame(p) * Vec3::new(v.x, 0.0, v.y)
    }

    /// A tangent direction at `p` for a local bearing (0 = local +X, pi/2 =
    /// local +Z: the planar convention of the island code).
    pub fn bearing(&self, p: Vec3, angle: f32) -> Vec3 {
        self.from_tangent(p, Vec2::new(angle.cos(), angle.sin()))
    }

    /// The local yaw of a world direction at `p` (`CivUnit::yaw`: 0 faces -Z).
    pub fn yaw_of(&self, p: Vec3, dir: Vec3) -> f32 {
        let t = self.to_tangent(p, dir);
        (-t.x).atan2(-t.y)
    }

    /// World rotation of an object at `p` facing local `yaw` on the local up.
    pub fn heading(&self, p: Vec3, yaw: f32) -> Quat {
        self.frame(p) * Quat::from_rotation_y(yaw)
    }

    /// `p` moved by a world step along the surface: the step's tangent part,
    /// then back to `p`'s distance from the centre (3D tangent-space
    /// integration; `terrain::ground` then snaps it onto the surface).
    pub fn advance(&self, p: Vec3, step: Vec3) -> Vec3 {
        match self.centre {
            Some(c) => {
                let r = (p - c).length();
                let up = self.up(p);
                let t = step - up * step.dot(up);
                c + (p + t - c).normalize_or(up) * r
            }
            None => Vec3::new(p.x + step.x, p.y, p.z + step.z),
        }
    }

    /// The point at chord `distance` from `from` along the tangent direction
    /// `dir` (world), at `from`'s distance from the centre.
    pub fn toward(&self, from: Vec3, dir: Vec3, distance: f32) -> Vec3 {
        match self.centre {
            Some(c) => {
                let r = (from - c).length().max(1.0);
                let u = (from - c) / r;
                let t = (dir - u * dir.dot(u)).normalize_or(u.any_orthonormal_vector());
                let theta = 2.0 * (distance / (2.0 * r)).clamp(0.0, 1.0).asin();
                c + (u * theta.cos() + t * theta.sin()) * r
            }
            None => {
                let d = Vec2::new(dir.x, dir.z).normalize_or(Vec2::X) * distance;
                Vec3::new(from.x + d.x, from.y, from.z + d.y)
            }
        }
    }

    /// `local` (a frame with +Y up at `p`, e.g. a city's layout) in the world.
    pub fn local_point(&self, p: Vec3, local: Vec3) -> Vec3 {
        p + self.frame(p) * local
    }

    /// A spawn position `clearance` above the surface point `p`: along the
    /// local up on the sphere; on the plane the island convention (y =
    /// `terrain::BASE_Y` + clearance; `terrain::ground` adds the terrain).
    pub fn place_on(&self, p: Vec3, clearance: f32) -> Vec3 {
        match self.centre {
            Some(_) => p + self.up(p) * clearance,
            None => Vec3::new(p.x, BASE_Y + clearance, p.z),
        }
    }

    /// Whether the surface point `p` faces `eye` (in front of the planet's
    /// limb, with a small margin for relief).
    pub fn visible_from(&self, eye: Vec3, p: Vec3) -> bool {
        match self.centre {
            Some(c) => (p - c).normalize_or(Vec3::Y).dot((eye - p).normalize_or(Vec3::Y)) > -0.12,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// The planet's surface
// ---------------------------------------------------------------------------

/// A `Shape` with the stage's height field: the planet (Civ frame), the
/// island, or nothing (the plane at `terrain::BASE_Y`).
#[derive(Clone, Copy, Default)]
pub struct Globe<'a> {
    planet: Option<&'a StagePlanet>,
    island: Option<&'a crate::terrain::LandSurface>,
}

impl<'a> Globe<'a> {
    pub fn flat() -> Self {
        Self::default()
    }

    pub fn on(planet: &'a StagePlanet) -> Self {
        Self {
            planet: Some(planet),
            island: None,
        }
    }

    pub fn from_surface(surface: Option<&'a crate::terrain::LandSurface>) -> Self {
        match surface {
            Some(s) => match s.planet() {
                Some(planet) => Self::on(planet),
                None => Self {
                    planet: None,
                    island: Some(s),
                },
            },
            None => Self::flat(),
        }
    }

    pub fn on_planet(&self) -> bool {
        self.planet.is_some()
    }

    pub fn planet(&self) -> Option<&'a StagePlanet> {
        self.planet
    }

    pub fn shape(&self) -> Shape {
        match self.planet {
            Some(sp) => Shape::sphere(sp.frame.centre, sp.planet.radius),
            None => Shape::FLAT,
        }
    }

    pub fn up(&self, p: Vec3) -> Vec3 {
        self.shape().up(p)
    }

    /// `ToSurface` under `p` along its radial line (the water surface over the
    /// sea); on the island / plane the ground under (x, z).
    pub fn surface(&self, p: Vec3) -> Vec3 {
        match self.planet {
            Some(sp) => sp.to_surface_world(p),
            None => Vec3::new(p.x, self.island.map_or(BASE_Y, |s| s.height(p.x, p.z)), p.z),
        }
    }

    /// The world point `clearance` above the surface under `p` (effects and
    /// other entities nothing grounds).
    pub fn lift(&self, p: Vec3, clearance: f32) -> Vec3 {
        let s = self.surface(p);
        s + self.up(s) * clearance
    }

    /// Dry land (`IsInWater` false); everything is land without a planet.
    pub fn is_land(&self, p: Vec3) -> bool {
        match self.planet {
            Some(sp) => !sp.is_in_water_world(p),
            None => true,
        }
    }

    /// Ground height above the sea level (units; 0 without a planet).
    pub fn altitude(&self, p: Vec3) -> f32 {
        match self.planet {
            Some(sp) => {
                let local = sp.frame.to_local(p);
                sp.planet.height(local) - sp.planet.water_radius()
            }
            None => 0.0,
        }
    }

    /// The nearest water along sixteen bearings, sampled every `step` units
    /// up to `reach` (`WaterDistance` of the placement markers); infinity
    /// when none (always without a planet).
    pub fn water_distance(&self, p: Vec3, reach: f32, step: f32) -> f32 {
        if self.planet.is_none() {
            return f32::INFINITY;
        }
        if !self.is_land(p) {
            return 0.0;
        }
        let shape = self.shape();
        let mut nearest = f32::INFINITY;
        for k in 0..16 {
            let dir = shape.bearing(p, k as f32 / 16.0 * TAU);
            let mut d = step;
            while d <= reach && d < nearest {
                if !self.is_land(shape.toward(p, dir, d)) {
                    nearest = d;
                    break;
                }
                d += step;
            }
        }
        nearest
    }

    /// The first water point from `p` along the bearing of the nearest water
    /// within `reach`, and that bearing's direction (seaport harbours).
    pub fn shore(&self, p: Vec3, reach: f32, step: f32) -> Option<(Vec3, Vec3)> {
        self.planet?;
        let shape = self.shape();
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for k in 0..32 {
            let dir = shape.bearing(p, k as f32 / 32.0 * TAU);
            let mut d = step;
            while d <= reach && best.is_none_or(|b| d < b.0) {
                let q = shape.toward(p, dir, d);
                if !self.is_land(q) {
                    best = Some((d, q, dir));
                    break;
                }
                d += step;
            }
        }
        best.map(|(_, q, dir)| (q, dir))
    }

    /// Whether the great circle from `a` to `b` stays on land (samples every
    /// `step` units).
    pub fn land_path(&self, a: Vec3, b: Vec3, step: f32) -> bool {
        if self.planet.is_none() {
            return true;
        }
        let shape = self.shape();
        let length = shape.distance(a, b);
        let n = (length / step.max(1.0)).ceil().max(1.0) as usize;
        let dir = b - a;
        (1..n).all(|i| self.is_land(shape.toward(a, dir, length * i as f32 / n as f32)))
    }

    /// Ground normal at `p` (world); +Y without a planet.
    pub fn ground_normal(&self, p: Vec3) -> Vec3 {
        match self.planet {
            Some(sp) => sp.ground_normal_world(p),
            None => Vec3::Y,
        }
    }

    /// First point where a ray meets the ground or the sea (picking). On the
    /// planet: the height field's ray cast (planet-re 1B) against the water
    /// sphere; elsewhere a march over the island / plane.
    pub fn ray(&self, origin: Vec3, direction: Vec3) -> Option<Vec3> {
        let d = direction.normalize_or_zero();
        if d == Vec3::ZERO {
            return None;
        }
        match self.planet {
            Some(sp) => {
                let o = sp.frame.to_local(origin);
                let dl = sp.frame.dir_to_local(d);
                let land = sp.planet.raycast(o, dl, 6000.0);
                let r = sp.planet.water_radius();
                let b = o.dot(dl);
                let disc = b * b - (o.length_squared() - r * r);
                let water = (disc >= 0.0)
                    .then(|| -b - disc.sqrt())
                    .filter(|t| *t > 0.0);
                let t = match (land, water) {
                    (Some(a), Some(b)) => a.min(b),
                    (a, b) => a.or(b)?,
                };
                Some(origin + d * t)
            }
            None => {
                if d.y >= -1e-4 {
                    return None;
                }
                let floor = |p: Vec3| self.island.map_or(BASE_Y, |s| s.height(p.x, p.z));
                let mut previous = origin;
                let step = 0.5f32.max(origin.y.abs() / 400.0);
                for i in 1..4000 {
                    let p = origin + d * (i as f32 * step);
                    if p.y <= floor(p) {
                        let (mut lo, mut hi) = (previous, p);
                        for _ in 0..14 {
                            let mid = (lo + hi) * 0.5;
                            if mid.y <= floor(mid) {
                                hi = mid;
                            } else {
                                lo = mid;
                            }
                        }
                        return Some(hi);
                    }
                    previous = p;
                }
                None
            }
        }
    }

    /// The ground's height in a grounded root's own frame under its local
    /// point (x, 0, z) (child meshes sit on it; scale and tilt included).
    pub fn local_ground(&self, root: &Transform, local_xz: Vec2) -> f32 {
        let w = root.transform_point(Vec3::new(local_xz.x, 0.0, local_xz.y));
        let g = match self.planet {
            Some(_) => self.surface(w),
            None => Vec3::new(w.x, self.island.map_or(BASE_Y, |s| s.height(w.x, w.z)), w.z),
        };
        root.to_matrix().inverse().transform_point3(g).y
    }
}

/// The stage planet of this Civ visit before `terrain::prepare` builds the
/// surface (setup and the wildlife spawn lay things out on it); `None` on the
/// plane.
#[derive(Resource, Clone, Default)]
pub struct CivSite(pub Option<std::sync::Arc<StagePlanet>>);

impl CivSite {
    pub fn globe(&self) -> Globe<'_> {
        self.0.as_deref().map_or_else(Globe::flat, Globe::on)
    }
}

/// The stage planet as the Civ layout sees it before `terrain::prepare`
/// builds the surface: the cached homeworld in the Civ frame (the stage origin
/// at world +Y, as `terrain::stage_planet`), cities not flattened yet.
pub fn site_planet(cache: &crate::terrain::PlanetCache) -> StagePlanet {
    StagePlanet {
        planet: (*cache.base).clone(),
        frame: PlanetFrame::new(cache.base.radius, cache.origin),
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Which layout a campaign holds (0: the island of earlier builds).
pub fn layout_key(on_planet: bool, seed: u64) -> u64 {
    let base = if on_planet {
        seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1
    } else {
        2
    };
    base ^ (LAYOUT_VERSION << 56)
}

/// What a city's placement marker asks (civ-re V4, `noun_placement_markers~`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Marker {
    /// PlayerCity: 123-248 from the origin, water 38-43, altitude 0.5-15.
    Player,
    /// landCity: 123-348, water >= 48.
    Land,
    /// seaportCity: 128-348, at the coast (ADAPTED: the player city's water
    /// range, so the harbour lies just outside the flattened pad).
    Seaport,
}

impl Marker {
    fn ring(self) -> (f32, f32) {
        match self {
            Self::Player => civ_rules::PLAYER_CITY_RING,
            Self::Land => civ_rules::LAND_CITY_RING,
            Self::Seaport => civ_rules::SEAPORT_CITY_RING,
        }
    }

    fn water_ok(self, water: f32) -> bool {
        match self {
            Self::Player | Self::Seaport => (30.0..=55.0).contains(&water),
            Self::Land => water >= 48.0,
        }
    }
}

/// ADAPTED: the marker kind of an NPC slot (retail takes whatever marker the
/// shuffled list holds, `landCity` or `seaportCity`; the generation list's mix
/// is not traced): the coastal slot of a continent is a seaport, the other
/// slots of other continents are land cities, and the player-continent slots
/// alternate land / seaport.
fn npc_marker(want: crate::civ_rules::SlotWant, ordinal: usize) -> Marker {
    use crate::civ_rules::SlotWant;
    match want {
        SlotWant::PlayerContinent => {
            if ordinal % 2 == 0 {
                Marker::Land
            } else {
                Marker::Seaport
            }
        }
        SlotWant::CoastalOn(_) => Marker::Seaport,
        SlotWant::Continent(_) => Marker::Land,
    }
}

/// A site must lie within this distance of water to be "coastal" for the
/// first city of another continent (VERIFIED 0xB9D970: 0xB8BCC0 with 70).
const COASTAL_REACH: f32 = 70.0;

/// Cities (capital first), whether each is a seaport and its harbour (the
/// sea point where its sea vehicles roll out), and the spice geysers.
#[derive(Clone, Debug, Default)]
pub struct Sites {
    pub cities: Vec<Vec3>,
    pub harbors: Vec<Option<Vec3>>,
    pub geysers: Vec<Vec3>,
}

fn hash01(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 10_000.0
}

/// One candidate site with its lazily measured water distance and ground
/// roughness; `ring` / `bearing` place it in the candidate grid.
struct Candidate {
    p: Vec3,
    from_origin: f32,
    water: Option<f32>,
    rough: Option<f32>,
    ring: usize,
    bearing: usize,
}

/// Bearings of one candidate ring.
const BEARINGS: usize = 40;

/// ADAPTED: the continent of every candidate, by flood fill over the ring grid
/// (neighbouring candidates are joined when the straight path between them
/// stays on land). Retail asks the planet for the continent of a position
/// (0xB886E0); candidates outside the rings do not take part. Ids count from
/// zero in candidate order; also returns each continent's candidate count (its
/// "size", retail 0xB7EB50).
fn candidate_continents(globe: &Globe, candidates: &[Candidate]) -> (Vec<usize>, Vec<usize>) {
    let mut cell: std::collections::HashMap<(usize, usize), usize> = std::collections::HashMap::new();
    for (i, c) in candidates.iter().enumerate() {
        cell.insert((c.ring, c.bearing), i);
    }
    let mut parent: Vec<usize> = (0..candidates.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let angle = |c: &Candidate| c.bearing as f32 + 0.5 * (((c.from_origin / 12.5) as i32) % 2) as f32;
    for (i, c) in candidates.iter().enumerate() {
        let a = angle(c);
        let mut neighbours: Vec<usize> = Vec::new();
        // The next bearing on the same ring and the nearest ones of the next ring.
        if let Some(j) = cell.get(&(c.ring, (c.bearing + 1) % BEARINGS)) {
            neighbours.push(*j);
        }
        for db in [BEARINGS - 1, 0, 1] {
            if let Some(j) = cell.get(&(c.ring + 1, (c.bearing + db) % BEARINGS)) {
                let b = angle(&candidates[*j]);
                let diff = ((a - b).abs()).min(BEARINGS as f32 - (a - b).abs());
                if diff <= 0.5 + 1e-3 {
                    neighbours.push(*j);
                }
            }
        }
        for j in neighbours {
            if globe.land_path(c.p, candidates[j].p, 10.0) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj] = ri;
                }
            }
        }
    }
    let mut ids: Vec<usize> = Vec::with_capacity(candidates.len());
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..candidates.len() {
        let r = find(&mut parent, i);
        let id = roots.iter().position(|x| *x == r).unwrap_or_else(|| {
            roots.push(r);
            roots.len() - 1
        });
        ids.push(id);
    }
    let mut sizes = vec![0usize; roots.len()];
    for id in &ids {
        sizes[*id] += 1;
    }
    (ids, sizes)
}

/// What the site search of one slot is restricted to.
#[derive(Clone, Copy)]
enum Target {
    Any,
    Continent(usize),
    Coastal(usize),
}

/// The level origin: the stage pole (the shared stage origin of the earlier
/// stages, planet-re 4F-3).
pub fn level_origin(globe: &Globe) -> Vec3 {
    globe.surface(Vec3::ZERO)
}

/// Lay out the cities and geysers. Retail picks among the markers' rings at
/// random; here the candidates are rings of 40 bearings every 12.5 units
/// (turned by the seed) scored by the marker's rules, then relaxed pass by
/// pass when the planet has no such spot (ADAPTED).
///
/// The slots follow the retail level setup (VERIFIED 0xB9D970,
/// `civ_rules::npc_slot_wants`): the capital first, then three cities on its
/// continent, one coastal city on every other continent and the rest on the
/// other continents by size, [`civ::CITY_COUNT`] slots in all (the arithmetica
/// `num_cities` 10). The continents are flood-filled over the candidates
/// ([`candidate_continents`], ADAPTED). The first passes keep a city on its
/// wanted continent; later ones take any land when the planet has no such
/// spot.
pub fn lay_out_sites(globe: &Globe, seed: u64) -> Sites {
    let shape = globe.shape();
    let origin = level_origin(globe);
    let spin = (seed % 7919) as f32 * 0.618_034 * TAU;
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut d = civ_rules::PLAYER_CITY_RING.0;
    let mut ring = 0usize;
    while d <= civ_rules::LAND_CITY_RING.1 + 1e-3 {
        for k in 0..BEARINGS {
            let a = spin + (k as f32 + 0.5 * ((d / 12.5) as i32 % 2) as f32) / BEARINGS as f32 * TAU;
            let p = globe.surface(shape.toward(origin, shape.bearing(origin, a), d));
            if globe.is_land(p) {
                candidates.push(Candidate {
                    p,
                    from_origin: d,
                    water: None,
                    rough: None,
                    ring,
                    bearing: k,
                });
            }
        }
        d += 12.5;
        ring += 1;
    }
    let (continent, sizes) = candidate_continents(globe, &candidates);
    let mut sites = Sites::default();
    let mut player_continent: Option<usize> = None;
    let mut wants: Vec<crate::civ_rules::SlotWant> = Vec::new();
    for index in 0..crate::civ::CITY_COUNT {
        let (marker, target) = if index == 0 {
            (Marker::Player, Target::Any)
        } else {
            if index == 1 {
                let others: Vec<(usize, usize)> = (0..sizes.len())
                    .filter(|c| Some(*c) != player_continent && sizes[*c] > 0)
                    .map(|c| (c, sizes[c]))
                    .collect();
                wants = crate::civ_rules::npc_slot_wants(crate::civ::CITY_COUNT - 1, &others);
            }
            let want = wants[index - 1];
            let target = match (want, player_continent) {
                (crate::civ_rules::SlotWant::PlayerContinent, Some(c)) => Target::Continent(c),
                (crate::civ_rules::SlotWant::PlayerContinent, None) => Target::Any,
                (crate::civ_rules::SlotWant::CoastalOn(c), _) => Target::Coastal(c),
                (crate::civ_rules::SlotWant::Continent(c), _) => Target::Continent(c),
            };
            (npc_marker(want, index - 1), target)
        };
        let (lo, hi) = marker.ring();
        let mut chosen: Option<usize> = None;
        // Passes: 0 the marker's rules, 1 without the water test, 2 with the
        // spacing relaxed to 120; all three keep the wanted continent. 3 / 4
        // take any land (spacing 180 / 120), 5 the rings and spacing ignored.
        for pass in 0..6 {
            let mut best: Option<(f32, usize)> = None;
            for i in 0..candidates.len() {
                let (p, from_origin) = (candidates[i].p, candidates[i].from_origin);
                let ring_ok = from_origin + 1e-3 >= lo && from_origin <= hi + 1e-3;
                if pass < 5 && !ring_ok {
                    continue;
                }
                let spacing = sites
                    .cities
                    .iter()
                    .map(|q| shape.distance(p, *q))
                    .fold(f32::INFINITY, f32::min);
                let min_spacing = match pass {
                    0 | 1 | 3 => civ_rules::MIN_DISTANCE_BETWEEN_CITIES,
                    2 | 4 => 120.0,
                    _ => 0.0,
                };
                if spacing < min_spacing {
                    continue;
                }
                let altitude = globe.altitude(p);
                if index == 0 && pass < 2 && !(0.5..=15.0).contains(&altitude) {
                    continue;
                }
                let w = if pass < 1 || index == 0 && pass < 2 {
                    let w = *candidates[i]
                        .water
                        .get_or_insert_with(|| globe.water_distance(p, 80.0, 4.0));
                    if !marker.water_ok(w) {
                        continue;
                    }
                    w
                } else {
                    0.0
                };
                if pass < 3 {
                    match target {
                        Target::Any => {}
                        Target::Continent(c) => {
                            if continent[i] != c {
                                continue;
                            }
                        }
                        Target::Coastal(c) => {
                            let near = *candidates[i]
                                .water
                                .get_or_insert_with(|| globe.water_distance(p, 80.0, 4.0));
                            if continent[i] != c || near > COASTAL_REACH {
                                continue;
                            }
                        }
                    }
                }
                // Prefer room around the city (spread from the others), flat
                // ground and, for the capital, water near 40 units.
                let rough = *candidates[i].rough.get_or_insert_with(|| {
                    (0..6)
                        .map(|k| {
                            let q = shape.toward(p, shape.bearing(p, k as f32 / 6.0 * TAU), 30.0);
                            (globe.altitude(q) - altitude).abs()
                        })
                        .fold(0.0f32, f32::max)
                });
                let mut score = spacing.min(320.0) * 0.02 - rough * 0.3;
                if index == 0 {
                    score -= (w - 40.5).abs() * 0.2 + (from_origin - 160.0).abs() * 0.005;
                }
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, i));
                }
            }
            if let Some((_, i)) = best {
                chosen = Some(i);
                break;
            }
        }
        if index == 0 {
            player_continent = chosen.map(|i| continent[i]);
        }
        // No land at all within the rings: a point on the ring anyway.
        let p = chosen.map(|i| candidates[i].p).unwrap_or_else(|| {
            let a = spin + index as f32 * 1.3;
            globe.surface(shape.toward(origin, shape.bearing(origin, a), lo.max(150.0)))
        });
        sites.cities.push(p);
        let harbor = matches!(marker, Marker::Player | Marker::Seaport)
            .then(|| globe.shore(p, 70.0, 3.0))
            .flatten()
            .map(|(q, dir)| globe.surface(shape.toward(q, dir, 12.0)))
            .filter(|q| !globe.is_land(*q));
        sites.harbors.push(harbor);
    }
    sites.geysers = geyser_sites(globe, &sites.cities, seed);
    sites
}

/// Two spice geysers per city, 95-170 from it and at least `minResourceRadius`
/// 90 from every city (civ-re V4; ADAPTED: counts and spacing are not in the
/// generation list), reachable over land from their city when possible.
pub fn geyser_sites(globe: &Globe, cities: &[Vec3], seed: u64) -> Vec<Vec3> {
    let shape = globe.shape();
    let mut out: Vec<Vec3> = Vec::new();
    let per_city = crate::civ::GEYSER_COUNT.div_ceil(cities.len().max(1));
    for (ci, city) in cities.iter().enumerate() {
        for _ in 0..per_city {
            if out.len() >= crate::civ::GEYSER_COUNT {
                break;
            }
            let mut chosen: Option<Vec3> = None;
            for pass in 0..4 {
                let mut best: Option<(f32, Vec3)> = None;
                for ring in 0..7 {
                    let d = 95.0 + ring as f32 * 12.5;
                    for k in 0..24 {
                        let a = (k as f32 + hash01(seed as u32 ^ ci as u32, ring)) / 24.0 * TAU;
                        let p = globe.surface(shape.toward(*city, shape.bearing(*city, a), d));
                        if pass < 3 && !globe.is_land(p) {
                            continue;
                        }
                        let near_city = cities.iter().map(|c| shape.distance(p, *c)).fold(f32::INFINITY, f32::min);
                        if pass < 3 && near_city + 1e-3 < civ_rules::MIN_RESOURCE_RADIUS {
                            continue;
                        }
                        let near_geyser = out.iter().map(|g| shape.distance(p, *g)).fold(f32::INFINITY, f32::min);
                        if pass < 2 && near_geyser < 45.0 {
                            continue;
                        }
                        if pass < 1 && !globe.land_path(*city, p, 8.0) {
                            continue;
                        }
                        let score = near_geyser.min(200.0) * 0.01 - (d - 110.0).abs() * 0.02;
                        if best.is_none_or(|(s, _)| score > s) {
                            best = Some((score, p));
                        }
                    }
                }
                if let Some((_, p)) = best {
                    chosen = Some(p);
                    break;
                }
            }
            out.push(chosen.unwrap_or_else(|| shape.toward(*city, shape.bearing(*city, out.len() as f32), 110.0)));
        }
    }
    while out.len() < crate::civ::GEYSER_COUNT {
        let p = shape.toward(cities[0], shape.bearing(cities[0], out.len() as f32 * 0.7), 130.0);
        out.push(globe.surface(p));
    }
    out
}

/// The goodie huts' spacing and clearance (`noun_placement_markers~!Civ_GoodyHut` 0x05F4AF23, group
/// 0x0302A1C9, property 0x561C98AC = 90, the marker's exclusive radius; the same property reads 300 on
/// `PlayerCity`), and the ring they stand on (property 0x94B1298E = 121 .. 573, `civ_rules`).
pub const HUT_CLEARANCE: f32 = 90.0;
pub const HUT_RING: (f32, f32) = (121.0, 573.0);

/// The goodie huts of a planet (`Civ_GoodyHut` markers: the Civ level setup 0xB9D970 places one marker
/// per city marker through the marker placer 0xB9B1E0, and the tribe builder 0xB942A0 turns each
/// remaining `0x57F09E5E` marker into a tribe with a hut). Land only; none on the flat layout.
/// ADAPTED: the count is the city count (the generator's marker list was not counted), the spots are
/// the same golden-angle search the epic lairs use (`wild_spots`) with the marker's ring and
/// exclusive radius (the retail altitude 0.5-50, water >= 50 and slope 30 limits are not applied).
pub fn hut_sites(globe: &Globe, cities: &[Vec3], geysers: &[Vec3], seed: u64) -> Vec<Vec3> {
    if !globe.on_planet() {
        return Vec::new();
    }
    wild_spots(
        globe,
        cities,
        geysers,
        crate::civ::CITY_COUNT,
        0x600D_C0DE ^ seed as u32,
        HUT_CLEARANCE,
        HUT_CLEARANCE,
        HUT_RING,
    )
}

/// Lay the huts of `campaign` out again (its cities and geysers stay).
pub fn lay_out_huts(campaign: &mut crate::civ::CivCampaign, globe: &Globe, seed: u64) {
    let cities: Vec<Vec3> = campaign.cities.iter().map(|c| c.center()).collect();
    let geysers: Vec<Vec3> = (0..crate::civ::GEYSER_COUNT).map(|g| campaign.geyser_at(g)).collect();
    campaign.hut_sites = hut_sites(globe, &cities, &geysers, seed).iter().map(|p| p.to_array()).collect();
}

/// Put the campaign on `globe`: cities, harbours and geysers from
/// [`lay_out_sites`], the vehicles beside their nation's city. Only when the
/// campaign holds another layout (the island of earlier builds, another
/// planet, a flat run); returns whether anything moved. Ownership, buildings
/// and every other campaign value stay.
pub fn lay_out(campaign: &mut crate::civ::CivCampaign, globe: &Globe, seed: u64) -> bool {
    campaign.ensure_cities();
    let key = layout_key(globe.on_planet(), seed);
    campaign.sphere = globe.shape().saved();
    if campaign.layout_key == key {
        // Saves from before the goodie huts: lay them out on the layout they keep.
        if campaign.hut_sites.is_empty() && globe.on_planet() {
            lay_out_huts(campaign, globe, seed);
        }
        return false;
    }
    // Never laid out and not started: the level setup's turn to give the NPC
    // slots their specialties (below). A running campaign keeps its cities'.
    let fresh = campaign.layout_key == 0 && campaign.clock <= 0.0;
    // The flat layout is the same for every save (`civ::city_position`).
    let sites = lay_out_sites(globe, if globe.on_planet() { seed } else { 0 });
    for (city, (p, harbor)) in campaign
        .cities
        .iter_mut()
        .zip(sites.cities.iter().zip(&sites.harbors))
    {
        city.position = p.to_array();
        city.harbor = harbor.map(|h| h.to_array());
    }
    campaign.geyser_sites = sites.geysers.iter().map(|g| g.to_array()).collect();
    campaign.huts_taken.clear();
    lay_out_huts(campaign, globe, seed);
    let shape = globe.shape();
    // The reveal order (VERIFIED 0xAE3430, `civ_reveal::order_keys`): the
    // continent test is a land path from the capital (ADAPTED for retail's
    // continent ids).
    if let Some(capital) = sites.cities.first().copied() {
        let same: Vec<bool> = sites
            .cities
            .iter()
            .map(|p| !globe.on_planet() || globe.land_path(capital, *p, 10.0))
            .collect();
        let distance: Vec<f32> = sites.cities.iter().map(|p| shape.distance(capital, *p)).collect();
        let salt = (seed as u32) ^ 0xAE34_30AE;
        campaign.reveal_order =
            crate::civ_reveal::order_keys(&same, &distance, |i| (hash01(salt, i as u32) * 100.0) as u32);
    }
    // The continent of every slot (retail +0x20 = 0xB886E0's continent id,
    // read by the rivals' budget split 0xBF7190): cities joined by a land path
    // share one (ADAPTED, as the reveal order's test).
    campaign.continents = continent_ids(sites.cities.len(), |a, b| {
        !globe.on_planet() || globe.land_path(sites.cities[a], sites.cities[b], 10.0)
    });
    // The specialty of every NPC slot (VERIFIED 0xCF8D00 after the slots are
    // placed, `civ_rules::roll_npc_specialties`): 25 % military, 25 % religious,
    // 50 % economic, rolled again until a military city stands on the player's
    // continent. ADAPTED: the roll stream is the layout seed's, not sMainRNG.
    if fresh && campaign.continents.len() > 1 {
        let home = campaign.continents[0];
        let on_player: Vec<bool> = campaign.continents[1..].iter().map(|c| *c == home).collect();
        let salt = (seed as u32) ^ 0xCF8D_0000;
        let mut draws = 0u32;
        let kinds = civ_rules::roll_npc_specialties(&on_player, || {
            draws += 1;
            hash01(salt, draws)
        });
        for (slot, roll) in kinds.into_iter().enumerate() {
            if let Some(city) = campaign.cities.get_mut(slot + 1) {
                city.retype(match roll {
                    0 => crate::civ::Approach::Military,
                    1 => crate::civ::Approach::Religious,
                    _ => crate::civ::Approach::Economic,
                });
            }
        }
    }
    let mut count = [0usize; crate::civ::CITY_COUNT];
    let units = std::mem::take(&mut campaign.units);
    campaign.units = units
        .into_iter()
        .map(|mut u| {
            let nation = u.nation.min(crate::civ::CITY_COUNT - 1);
            let k = count[nation];
            count[nation] += 1;
            let pad = match u.domain() {
                crate::civ::VehicleDomain::Sea => campaign.cities[nation]
                    .harbor
                    .map(Vec3::from_array)
                    .unwrap_or_else(|| campaign.vehicle_pad(nation)),
                _ => campaign.vehicle_pad(nation),
            };
            let side = shape.bearing(pad, k as f32 * 2.39996);
            let at = globe.surface(shape.toward(pad, side, 7.0 * CITY_SCALE * (k as f32).sqrt()));
            u.position = [at.x, at.z];
            u.world = Some(at.to_array());
            if matches!(u.order, crate::civ::UnitOrder::Move(_) | crate::civ::UnitOrder::Goto(_)) {
                u.order = crate::civ::UnitOrder::Idle;
            }
            u
        })
        .collect();
    campaign.layout_key = key;
    true
}

/// Pure: continent ids for `n` cities, `joined(a, b)` telling whether two
/// cities share land (ids by first appearance: city 0's continent is 0).
pub fn continent_ids(n: usize, joined: impl Fn(usize, usize) -> bool) -> Vec<u8> {
    let mut id: Vec<Option<u8>> = vec![None; n];
    let mut next = 0u8;
    for a in 0..n {
        if id[a].is_some() {
            continue;
        }
        id[a] = Some(next);
        // Flood over the land links from `a`.
        let mut stack = vec![a];
        while let Some(c) = stack.pop() {
            for b in 0..n {
                if id[b].is_none() && joined(c, b) {
                    id[b] = Some(next);
                    stack.push(b);
                }
            }
        }
        next = next.saturating_add(1);
    }
    id.into_iter().map(|i| i.unwrap_or(0)).collect()
}

/// `count` spots on land around the level origin (golden-angle spiral
/// between `ring` distances), `clearance` from every city, off the geysers,
/// `spacing` apart (the epic predators' lairs, the animals).
pub fn wild_spots(
    globe: &Globe,
    cities: &[Vec3],
    geysers: &[Vec3],
    count: usize,
    salt: u32,
    spacing: f32,
    clearance: f32,
    ring: (f32, f32),
) -> Vec<Vec3> {
    let shape = globe.shape();
    let origin = level_origin(globe);
    let mut out: Vec<Vec3> = Vec::new();
    for pass in 0..2 {
        let mut k = 0u32;
        while out.len() < count && k < 4000 {
            let angle = k as f32 * 2.399_963 + hash01(salt, k) * 0.6;
            let d = ring.0 + (ring.1 - ring.0) * hash01(salt ^ 0x55AA, k).sqrt();
            let p = globe.surface(shape.toward(origin, shape.bearing(origin, angle), d));
            k += 1;
            if pass == 0 && !globe.is_land(p) {
                continue;
            }
            if cities.iter().any(|c| shape.distance(p, *c) < clearance)
                || geysers.iter().any(|g| shape.distance(p, *g) < 3.0 * CITY_SCALE)
                || out.iter().any(|q| shape.distance(p, *q) < spacing)
            {
                continue;
            }
            out.push(p);
        }
        if out.len() >= count {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The planet map (HUD minimap)
// ---------------------------------------------------------------------------

/// The minimap's projection. ADAPTED (the retail map is not decoded): the
/// picture's 231 x 102 holder is about 2:1, so the whole planet is drawn
/// equirectangular, centred on the level origin with the planet's own axis
/// (planet-local +Y) up. Without a planet: the plane, 900 units high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapFrame {
    centre: Vec3,
    radius: f32,
    forward: Vec3,
    north: Vec3,
    east: Vec3,
    flat: bool,
}

/// World units across the flat map's height.
pub const FLAT_MAP_SPAN: f32 = 900.0;

impl MapFrame {
    pub fn new(planet: Option<&StagePlanet>) -> Self {
        match planet {
            Some(sp) => {
                let forward = Vec3::Y;
                let axis = sp.frame.dir_to_world(Vec3::Y);
                let mut north = axis - forward * axis.dot(forward);
                if north.length_squared() < 1e-4 {
                    north = Vec3::NEG_Z;
                }
                let north = north.normalize();
                Self {
                    centre: sp.frame.centre,
                    radius: sp.planet.radius,
                    forward,
                    north,
                    east: north.cross(forward),
                    flat: false,
                }
            }
            None => Self {
                centre: Vec3::ZERO,
                radius: 0.0,
                forward: Vec3::Y,
                north: Vec3::NEG_Z,
                east: Vec3::X,
                flat: true,
            },
        }
    }

    /// Pixel of a world point on a `size` picture.
    pub fn pixel(&self, p: Vec3, size: Vec2) -> Vec2 {
        if self.flat {
            let scale = size.y / FLAT_MAP_SPAN;
            return Vec2::new(size.x * 0.5 + p.x * scale, size.y * 0.5 + p.z * scale);
        }
        let d = (p - self.centre).normalize_or(self.forward);
        let lat = d.dot(self.north).clamp(-1.0, 1.0).asin();
        let lon = d.dot(self.east).atan2(d.dot(self.forward));
        Vec2::new(
            size.x * (0.5 + lon / TAU),
            size.y * (0.5 - lat / PI),
        )
    }

    /// Sea-level world point under a pixel (the plane at y = 0 when flat).
    pub fn world(&self, pixel: Vec2, size: Vec2) -> Vec3 {
        if self.flat {
            let scale = size.y / FLAT_MAP_SPAN;
            return Vec3::new((pixel.x - size.x * 0.5) / scale, 0.0, (pixel.y - size.y * 0.5) / scale);
        }
        let lon = (pixel.x / size.x.max(1.0) - 0.5) * TAU;
        let lat = (0.5 - pixel.y / size.y.max(1.0)) * PI;
        let d = (self.forward * lon.cos() + self.east * lon.sin()) * lat.cos() + self.north * lat.sin();
        self.centre + d * self.radius
    }

    /// Pixels per world unit at the map's centre (the camera box).
    pub fn pixels_per_unit(&self, size: Vec2) -> f32 {
        if self.flat {
            size.y / FLAT_MAP_SPAN
        } else {
            size.y / (PI * self.radius)
        }
    }
}

/// Terrain colours of the planet map. ORIGINAL: the stage planet's height
/// field and sea level (`IsInWater`), the beach band (`mBeachLevel`) and the
/// home planet's palette; ADAPTED: the elevation ramp (0.6 of the altitude
/// range, as the land shader and the Creature map), depth shading and relief.
pub fn paint_globe_map(
    frame: &MapFrame,
    planet: &StagePlanet,
    palette: Option<&crate::world_catalog::PlanetPalette>,
    w: u32,
    h: u32,
) -> Vec<[u8; 3]> {
    let to_u8 = |c: [f32; 3]| {
        let s = Color::srgb(c[0], c[1], c[2]).to_srgba();
        [
            (s.red.clamp(0.0, 1.0) * 255.0) as u8,
            (s.green.clamp(0.0, 1.0) * 255.0) as u8,
            (s.blue.clamp(0.0, 1.0) * 255.0) as u8,
        ]
    };
    let water = to_u8(palette.map_or([0.03, 0.14, 0.24], |p| p.water));
    let size = Vec2::new(w as f32, h as f32);
    let sea = planet.planet.water_radius();
    let range = planet.planet.altitude_range;
    let (wu, hu) = (w as usize, h as usize);
    let mut elevation = vec![0.0f32; (wu + 1) * (hu + 1)];
    for y in 0..=hu {
        for x in 0..=wu {
            let p = frame.world(Vec2::new(x as f32, y as f32), size);
            elevation[y * (wu + 1) + x] = planet.planet.height(planet.frame.to_local(p)) - sea;
        }
    }
    let at = |x: usize, y: usize| elevation[y * (wu + 1) + x];
    let mut out = Vec::with_capacity(wu * hu);
    for y in 0..hu {
        for x in 0..wu {
            let corners = [at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1)];
            let e = corners.iter().sum::<f32>() * 0.25;
            if e < 0.0 {
                let t = (-e / 30.0).clamp(0.0, 1.0);
                out.push(water.map(|c| (c as f32 * (1.0 - 0.45 * t)) as u8));
                continue;
            }
            let rise = (e / (0.6 * range)).clamp(0.0, 1.0);
            let land = palette.map_or_else(
                || {
                    let a = rise.clamp(0.10, 0.92);
                    [0.12 + a * 0.15, 0.24 + a * 0.25, 0.10 + a * 0.12]
                },
                |pal| pal.sample(rise),
            );
            let mixed = match palette {
                Some(pal) => {
                    let t = (e / (crate::planet_query::BEACH_LEVEL * range)).clamp(0.0, 1.0);
                    let beach = 1.0 - t * t * (3.0 - 2.0 * t);
                    std::array::from_fn(|c| land[c] * (1.0 - beach) + pal.beach[c] * beach)
                }
                None => land,
            };
            let gx = (corners[0] + corners[2]) - (corners[1] + corners[3]);
            let gz = (corners[0] + corners[1]) - (corners[2] + corners[3]);
            let shade = (1.0 + (gx + gz) * 0.02).clamp(0.8, 1.2);
            out.push(to_u8(mixed.map(|c| c * shade)));
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, Planet, PLANET_RADIUS};

    /// A test planet in the Civ frame: land within `land_radius` units of arc
    /// around the stage pole (planet +X), sea beyond.
    pub(crate) fn continent(land_radius: f32) -> StagePlanet {
        let res = 64;
        let mut cube = HeightCube::filled(res, -0.3);
        let edge = (land_radius / PLANET_RADIUS).cos();
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = h_to_raw(((d.x - edge) * 3.0).clamp(-0.3, 0.3));
                }
            }
        }
        StagePlanet {
            planet: Planet::new(cube, PLANET_RADIUS, 0.0),
            frame: PlanetFrame::new(PLANET_RADIUS, Vec3::X),
        }
    }

    #[test]
    fn the_flat_shape_is_the_island_geometry() {
        let s = Shape::FLAT;
        let a = Vec3::new(3.0, 7.0, -4.0);
        assert_eq!(s.distance(a, Vec3::new(0.0, -2.0, 0.0)), 5.0);
        assert_eq!(s.up(a), Vec3::Y);
        assert_eq!(s.to_tangent(a, Vec3::new(1.0, 5.0, 2.0)), Vec2::new(1.0, 2.0));
        assert_eq!(s.advance(a, Vec3::new(1.0, 9.0, 1.0)), Vec3::new(4.0, 7.0, -3.0));
        let yaw = s.yaw_of(a, Vec3::new(-1.0, 0.0, 0.0));
        assert!((s.heading(a, yaw) * Vec3::NEG_Z - Vec3::NEG_X).length() < 1e-5);
    }

    #[test]
    fn sphere_steps_keep_the_radius_and_measure_chords() {
        let s = Shape::sphere(Vec3::new(0.0, -500.0, 0.0), 500.0);
        let pole = Vec3::ZERO;
        let p = s.toward(pole, Vec3::X, 200.0);
        assert!((s.distance(pole, p) - 200.0).abs() < 1e-2, "{}", s.distance(pole, p));
        assert!(((p - Vec3::new(0.0, -500.0, 0.0)).length() - 500.0).abs() < 1e-3);
        // Up, frame and tangent round trip far from the pole.
        let up = s.up(p);
        assert!((s.frame(p) * Vec3::Y - up).length() < 1e-5);
        let v = Vec2::new(0.3, -0.8);
        assert!((s.to_tangent(p, s.from_tangent(p, v)) - v).length() < 1e-5);
        assert!(s.from_tangent(p, v).dot(up).abs() < 1e-5);
        // Integrating small tangent steps follows the great circle.
        let mut q = pole;
        for _ in 0..200 {
            let dir = s.from_tangent(q, s.to_tangent(q, p - q).normalize());
            q = s.advance(q, dir * 1.0);
        }
        assert!(s.distance(q, p) < 2.0, "{}", s.distance(q, p));
        // The far side is hidden from a camera over the pole.
        let eye = Vec3::new(0.0, 300.0, 0.0);
        assert!(s.visible_from(eye, pole));
        assert!(!s.visible_from(eye, Vec3::new(0.0, -1000.0, 0.0)));
    }

    /// A test planet in the Civ frame with the home continent (`land_radius`
    /// of arc around the stage pole) and one island `island_at` away from the
    /// pole (toward planet +Z), `island_radius` wide.
    fn continent_and_island(land_radius: f32, island_at: f32, island_radius: f32) -> StagePlanet {
        let res = 64;
        let mut cube = HeightCube::filled(res, -0.3);
        let edge = (land_radius / PLANET_RADIUS).cos();
        let theta = island_at / PLANET_RADIUS;
        let centre = Vec3::new(theta.cos(), 0.0, theta.sin());
        let edge_island = (island_radius / PLANET_RADIUS).cos();
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    let h = ((d.x - edge) * 3.0).max((d.dot(centre) - edge_island) * 3.0);
                    cube.raw_mut()[i] = h_to_raw(h.clamp(-0.3, 0.3));
                }
            }
        }
        StagePlanet {
            planet: Planet::new(cube, PLANET_RADIUS, 0.0),
            frame: PlanetFrame::new(PLANET_RADIUS, Vec3::X),
        }
    }

    #[test]
    fn three_cities_share_the_capitals_continent_and_the_next_goes_overseas() {
        // VERIFIED 0xB9D970: three NPC cities on the player's continent first,
        // then a coastal city on every other continent.
        let planet = continent_and_island(255.0, 350.0, 50.0);
        let globe = Globe::on(&planet);
        let sites = lay_out_sites(&globe, 3);
        assert_eq!(sites.cities.len(), crate::civ::CITY_COUNT);
        let capital = sites.cities[0];
        for i in 1..=crate::civ_rules::NPC_CITIES_ON_PLAYER_CONTINENT {
            assert!(globe.land_path(capital, sites.cities[i], 10.0), "city {i} is on the capital's continent");
        }
        let overseas = crate::civ_rules::NPC_CITIES_ON_PLAYER_CONTINENT + 1;
        assert!(
            !globe.land_path(capital, sites.cities[overseas], 10.0),
            "city {overseas} is the coastal city of the island"
        );
        assert!(globe.is_land(sites.cities[overseas]));
        // The island city is a seaport with a harbour.
        assert!(sites.harbors[overseas].is_some(), "{:?}", sites.harbors);
    }

    #[test]
    fn cities_follow_the_retail_markers_on_a_continent() {
        let planet = continent(420.0);
        let globe = Globe::on(&planet);
        let shape = globe.shape();
        let sites = lay_out_sites(&globe, 7);
        let origin = level_origin(&globe);
        assert_eq!(sites.cities.len(), crate::civ::CITY_COUNT);
        let capital = shape.distance(origin, sites.cities[0]);
        assert!((122.0..=249.0).contains(&capital), "capital {capital}");
        for (i, a) in sites.cities.iter().enumerate() {
            assert!(globe.is_land(*a), "city {i} at sea");
            let d = shape.distance(origin, *a);
            assert!((122.0..=349.0).contains(&d), "city {i} at {d}");
            for b in &sites.cities[i + 1..] {
                assert!(shape.distance(*a, *b) + 0.5 >= 180.0, "cities {i} too close");
            }
        }
        for (g, p) in sites.geysers.iter().enumerate() {
            assert!(globe.is_land(*p), "geyser {g} at sea");
            for c in &sites.cities {
                assert!(shape.distance(*p, *c) + 0.5 >= civ_rules::MIN_RESOURCE_RADIUS, "geyser {g}");
            }
        }
        assert_eq!(sites.geysers.len(), crate::civ::GEYSER_COUNT);
        // Ports have a harbour at sea; deterministic.
        for h in sites.harbors.iter().flatten() {
            assert!(!globe.is_land(*h));
        }
        let again = lay_out_sites(&globe, 7);
        assert_eq!(again.cities, sites.cities);
    }

    #[test]
    fn an_island_campaign_moves_onto_the_planet_once() {
        let planet = continent(420.0);
        let globe = Globe::on(&planet);
        // An island-era save: cities 28 apart, a vehicle with an x, z move order.
        let mut campaign = crate::civ::CivCampaign::default();
        campaign.ensure_cities();
        for (i, city) in campaign.cities.iter_mut().enumerate() {
            city.position = [i as f32 * 20.0, 0.0, 0.0];
        }
        campaign.cities[2].owner = crate::civ::CityOwner::Player;
        let mut unit = campaign.add_unit(2, crate::civ::Approach::Military, false, Vec3::new(5.0, 0.0, 5.0));
        unit.world = None;
        unit.order = crate::civ::UnitOrder::Move([9.0, 9.0]);
        campaign.units = vec![unit];
        assert!(lay_out(&mut campaign, &globe, 3));
        let shape = campaign.shape();
        assert!(shape.on_sphere());
        assert!(campaign.cities[2].owned(), "ownership stays");
        let origin = level_origin(&globe);
        for city in &campaign.cities {
            assert!(shape.distance(origin, city.center()) > 120.0);
        }
        assert_eq!(campaign.geyser_sites.len(), crate::civ::GEYSER_COUNT);
        let moved = &campaign.units[0];
        assert!(shape.distance(moved.at(), campaign.cities[2].center()) < 60.0);
        assert_eq!(moved.order, crate::civ::UnitOrder::Idle);
        // Laid out once: the same planet keeps its positions.
        let before = campaign.cities.clone();
        assert!(!lay_out(&mut campaign, &globe, 3));
        assert_eq!(campaign.cities, before);
        // A flat run lays it out again (and back).
        assert!(lay_out(&mut campaign, &Globe::flat(), 3));
        assert!(!campaign.shape().on_sphere());
    }

    #[test]
    fn a_fresh_planet_rolls_the_rival_specialties_with_a_military_neighbour() {
        use crate::civ::Approach;
        let planet = continent(420.0);
        let globe = Globe::on(&planet);
        let mut kinds_seen = std::collections::HashSet::new();
        for seed in [3u64, 4, 5, 6, 7, 8] {
            let mut campaign = crate::civ::CivCampaign::default();
            campaign.ensure_cities();
            assert!(lay_out(&mut campaign, &globe, seed));
            let home = campaign.continents[0];
            assert!(
                (1..campaign.cities.len())
                    .any(|i| campaign.continents[i] == home && campaign.cities[i].kind == Approach::Military),
                "seed {seed}: a military city on the player's continent"
            );
            for city in &campaign.cities[1..] {
                // The type's turrets (`CityState::new`): three for a military rival, else two.
                assert_eq!(city.defense, if city.kind == Approach::Military { 3 } else { 2 });
                assert_eq!(city.live_turrets(), city.defense as usize);
                kinds_seen.insert(city.kind);
            }
            // Laid out once: the types stay when the same planet is laid out again.
            let before: Vec<Approach> = campaign.cities.iter().map(|c| c.kind).collect();
            campaign.layout_key = 0;
            campaign.clock = 30.0;
            lay_out(&mut campaign, &globe, seed + 100);
            let after: Vec<Approach> = campaign.cities.iter().map(|c| c.kind).collect();
            assert_eq!(before, after, "a running campaign keeps its types");
        }
        assert!(kinds_seen.len() == 3, "{kinds_seen:?}");
    }

    #[test]
    fn the_flat_layout_keeps_the_rings_and_spacing() {
        let globe = Globe::flat();
        let sites = lay_out_sites(&globe, 0);
        let shape = globe.shape();
        let capital = shape.distance(Vec3::ZERO, sites.cities[0]);
        assert!((122.0..=249.0).contains(&capital), "capital {capital}");
        for (i, a) in sites.cities.iter().enumerate() {
            for b in &sites.cities[i + 1..] {
                assert!(shape.distance(*a, *b) + 0.5 >= 180.0);
            }
        }
    }

    #[test]
    fn the_planet_map_round_trips_and_centres_the_origin() {
        let planet = continent(300.0);
        let frame = MapFrame::new(Some(&planet));
        let size = Vec2::new(231.0, 102.0);
        let origin = level_origin(&Globe::on(&planet));
        assert!((frame.pixel(origin, size) - size * 0.5).length() < 0.5);
        for (x, y) in [(20.0, 30.0), (200.0, 80.0), (115.0, 51.0)] {
            let w = frame.world(Vec2::new(x, y), size);
            assert!((frame.pixel(w, size) - Vec2::new(x, y)).length() < 1e-2);
        }
        let flat = MapFrame::new(None);
        let p = Vec3::new(120.0, 0.0, -80.0);
        assert!((flat.world(flat.pixel(p, size), size) - p).length() < 1e-3);
    }

    #[test]
    fn rays_pick_the_ground_and_the_sea() {
        let planet = continent(200.0);
        let globe = Globe::on(&planet);
        let hit = globe.ray(Vec3::new(0.0, 300.0, 0.0), Vec3::NEG_Y).expect("ground");
        assert!(hit.y > 0.0 && hit.y < 40.0, "{hit:?}");
        let sea = globe.shape().toward(Vec3::ZERO, Vec3::X, 320.0);
        let eye = sea + globe.up(sea) * 200.0;
        let hit = globe.ray(eye, -globe.up(sea)).expect("sea");
        assert!(!globe.is_land(hit));
        let flat = Globe::flat().ray(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y).expect("plane");
        assert!((flat.y - BASE_Y).abs() < 0.01);
    }

    #[test]
    fn goodie_huts_stand_on_land_inside_their_ring_clear_of_cities_and_each_other() {
        let planet = continent(450.0);
        let globe = Globe::on(&planet);
        let mut campaign = crate::civ::CivCampaign::default();
        lay_out(&mut campaign, &globe, 5);
        let shape = campaign.shape();
        let huts: Vec<Vec3> = campaign.hut_sites.iter().map(|p| Vec3::from_array(*p)).collect();
        assert_eq!(huts.len(), crate::civ::CITY_COUNT);
        let origin = level_origin(&globe);
        for (i, h) in huts.iter().enumerate() {
            assert!(globe.is_land(*h), "hut {i} is at sea");
            let d = shape.distance(origin, *h);
            assert!(d >= HUT_RING.0 - 1.0 && d <= HUT_RING.1 + 1.0, "hut {i} at {d} from the origin");
            for city in &campaign.cities {
                assert!(shape.distance(*h, city.center()) >= HUT_CLEARANCE, "hut {i} crowds a city");
            }
            for other in &huts[i + 1..] {
                assert!(shape.distance(*h, *other) >= HUT_CLEARANCE, "two huts crowd each other");
            }
            assert!(campaign.hut_standing(i));
        }
        // An old save with a layout but no huts gets them; the flat layout has none.
        campaign.hut_sites.clear();
        assert!(!lay_out(&mut campaign, &globe, 5));
        assert_eq!(campaign.hut_sites.len(), crate::civ::CITY_COUNT);
        assert!(hut_sites(&Globe::flat(), &[], &[], 5).is_empty());
    }
}
