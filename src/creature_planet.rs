//! The Creature stage on the original's planet (planet-re 4E step 4).
//!
//! The stage code keeps its chart coordinates: an entity's world (x, z) on the
//! `LandSurface` facade (planet centre (0, -R, 0), the stage pole at world +Y,
//! `terrain::ground` puts y on the surface along the vertical line). This
//! module answers what the stage asks about that chart from the planet's own
//! height field:
//!
//! * land or sea: `IsInWater` of the ground under the point (0xB7E530);
//! * retail distances: straight-line chords between surface points (planet-re
//!   1E: OriginDistance bands, the casting thresholds and every other tuning
//!   distance are 3D distances);
//! * the point at a chord distance and bearing from another (nest placement);
//! * the stay-on-land rule that replaces the island's BOUNDS clamps
//!   (ADAPTED: retail lets the avatar swim, and deep water calls the sea
//!   monster, notes C1F; neither is implemented, so creatures stop at the
//!   waterline).
//!
//! Without a planet (headless runs, `OPENSPORE_FLAT_TERRAIN=1`) every point is
//! land and distances are planar.
//!
//! Far from the stage pole the chart degrades (the vertical-line facade
//! tilts, and it ends at the planet's limb), while retail creatures can walk
//! around the whole planet. ADAPTED floating origin (`reanchor`): when the
//! avatar is `REANCHOR_DISTANCE` from the pole, the planet and everything on
//! it turn about the planet centre so the avatar's ground is the pole again;
//! stored positions (nests, lairs, home, goals, the camera rig) turn with it
//! and the new frame is saved (`SpeciesWorld::planet_frame`). Gameplay keeps
//! measuring retail chords, so nothing changes for the rules, and the chart
//! the stage code integrates in never reaches farther than ~300 units.

use std::sync::Arc;

use bevy::prelude::*;

use crate::planet_query::{PlanetFrame, StagePlanet};
use crate::stages::{GameStage, StageOwned};

/// ADAPTED: the avatar's chart distance from the stage pole that turns the
/// planet under it (herds activate within 100 of the avatar, so active
/// creatures stay within ~250 units of the pole).
pub const REANCHOR_DISTANCE: f32 = 150.0;

/// `REANCHOR_DISTANCE`, or the dev override `OPENSPORE_REANCHOR=<units>`
/// (at least 5; for watching the floating origin in short captures).
fn reanchor_distance() -> f32 {
    static DISTANCE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *DISTANCE.get_or_init(|| {
        std::env::var("OPENSPORE_REANCHOR")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|d| d.is_finite() && *d >= 5.0)
            .unwrap_or(REANCHOR_DISTANCE)
    })
}

/// The last re-anchor (for systems that keep world positions in `Local`
/// state): its count this visit and the rigid rotation about the centre.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Reanchored {
    pub generation: u32,
    pub centre: Vec3,
    pub rotation: Quat,
}

impl Default for Reanchored {
    fn default() -> Self {
        Self {
            generation: 0,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }
}

impl Reanchored {
    /// A world position riding with the planet through the last re-anchor.
    pub fn apply(&self, v: Vec3) -> Vec3 {
        self.centre + self.rotation * (v - self.centre)
    }
}

/// One re-anchor: the stage planet before it and the rotation about its
/// centre.
pub struct Shift {
    old: Arc<StagePlanet>,
    rotation: Quat,
}

impl Shift {
    pub fn new(old: Arc<StagePlanet>, rotation: Quat) -> Self {
        Self { old, rotation }
    }

    /// A world position that rides with the planet (entity transforms).
    pub fn exact(&self, v: Vec3) -> Vec3 {
        let c = self.old.frame.centre;
        c + self.rotation * (v - c)
    }

    /// A stored position: on the chart's hemisphere it is first put on the
    /// old surface along the vertical line (stored heights there are often
    /// baselines such as 0.4), beyond it (below the centre) it already is a
    /// 3D point. The result holds the real height.
    pub fn point(&self, v: Vec3) -> Vec3 {
        if !v.is_finite() {
            return v;
        }
        let c = self.old.frame.centre;
        let lifted = if v.y < c.y {
            v
        } else {
            Vec3::new(v.x, self.old.surface_y(v.x, v.z), v.z)
        };
        self.exact(lifted)
    }

    pub fn array(&self, a: [f32; 3]) -> [f32; 3] {
        self.point(Vec3::from_array(a)).to_array()
    }

    /// A stored chart point (x, z) of an active creature.
    pub fn chart(&self, x: f32, z: f32) -> (f32, f32) {
        let p = self.point(Vec3::new(x, 0.0, z));
        (p.x, p.z)
    }
}

/// Whether two stored world points lie on the same side of the planet as
/// far as the chart is concerned (ADAPTED guard for the stage's planar
/// distance checks: a point beyond the limb shares its x, z with one in
/// front of it, but not its height).
pub fn same_side(a: [f32; 3], b: [f32; 3]) -> bool {
    (a[1] - b[1]).abs() < 60.0
}

/// The planet under the Creature stage as a cheap shared handle.
#[derive(Clone, Default)]
pub struct Land {
    planet: Option<Arc<StagePlanet>>,
}

impl std::fmt::Debug for Land {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Land").field("planet", &self.planet.is_some()).finish()
    }
}

/// A fruit-bearing tree of a grove (`Land::fruit_groves`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FruitTree {
    pub at: Vec2,
    /// Flora class: 0 large, 1 medium.
    pub class: usize,
    pub scale: f32,
}

impl Land {
    /// No planet: all land, planar distances.
    pub fn flat() -> Self {
        Self::default()
    }

    pub fn new(planet: Arc<StagePlanet>) -> Self {
        Self { planet: Some(planet) }
    }

    /// The stage surface's planet (none on the prototype island).
    pub fn from_surface(surface: Option<&crate::terrain::LandSurface>) -> Self {
        Self {
            planet: surface.and_then(|s| s.planet_arc()),
        }
    }

    pub fn on_planet(&self) -> bool {
        self.planet.is_some()
    }

    /// The planet's forests on land within chord `radius` of chart `home`, as
    /// chart points, in the forest list's order. VERIFIED (0xB9C980): the level generation
    /// walks the planet's forest list (the marked fruit groves, which
    /// `planet_flora::plan` places: 3 per level-2 cell) and keeps those within
    /// the radius on the start's land region. ADAPTED: "land region" is
    /// tested as dry land at the grove centre (0xB886E0's regions are not
    /// decoded). Empty without a planet.
    pub fn forests(&self, home: Vec2, radius: f32) -> Vec<Vec2> {
        let Some(sp) = &self.planet else {
            return Vec::new();
        };
        let plan = crate::planet_flora::plan(&sp.planet, 0, [true; 9], &[]);
        plan
            .groves
            .iter()
            .filter(|g| !g.plants.is_empty())
            .filter_map(|g| {
                let w = sp.frame.to_world(g.centre);
                if w.y <= sp.frame.centre.y {
                    return None;
                }
                let p = Vec2::new(w.x, w.z);
                let d = self.chord(home, p);
                (self.is_land(p) && d <= radius).then_some(p)
            })
            .collect()
    }

    /// The planet's fruit groves on land within chord `radius` of chart `home`,
    /// nearest first: each is the fruit-bearing trees of one grove (the large and
    /// medium trees of `planet_flora::plan`, small trees carry none:
    /// `maxSmallTreeFruitPerGrove` 0) as chart points with their flora class (0
    /// large, 1 medium) and size. The fruit manager (cGamePlantManager, listener
    /// 0xB00110) receives one message per grove of the theme's fruit
    /// distribute effect (0x030FC4EC, the same groves the flora draws and the
    /// level generation marks as forests: `forests`). `pads` are the stage's
    /// plant-free pads as the flora takes them (`planet_flora::clearings`: the
    /// home nest first, then the other nests, chart point and radius): with them
    /// and the start grove the flora adds when few plants stand in the opening
    /// view (`planet_flora::add_start_grove`) the trees are those drawn.
    /// ADAPTED as in `forests`: "on land" is the dry grove centre, and every
    /// flora slot counts as present. Empty without a planet.
    pub fn fruit_groves(&self, home: Vec2, radius: f32, pads: &[([f32; 3], f32)]) -> Vec<Vec<FruitTree>> {
        let Some(sp) = &self.planet else {
            return Vec::new();
        };
        let local = |p: [f32; 3]| {
            let p = Vec3::from_array(p);
            let world_point = if p.y >= sp.frame.centre.y { Vec3::new(p.x, sp.surface_y(p.x, p.z), p.z) } else { p };
            let l = sp.frame.to_local(world_point);
            l.normalize_or(Vec3::Y) * sp.planet.height(l)
        };
        let clear: Vec<(Vec3, f32)> = pads.iter().map(|(p, r)| (local(*p), *r)).collect();
        let mut plan = crate::planet_flora::plan(&sp.planet, 0, [true; 9], &clear);
        if let Some((start, _)) = clear.first() {
            crate::planet_flora::add_start_grove(
                &mut plan,
                &sp.planet,
                *start,
                crate::planet_flora::START_VIEW_CREATURE,
                0,
                [true; 9],
                &clear,
            );
        }
        let mut groves: Vec<(f32, Vec<FruitTree>)> = plan
            .groves
            .iter()
            .filter(|g| !g.plants.is_empty())
            .filter_map(|g| {
                let w = sp.frame.to_world(g.centre);
                if w.y <= sp.frame.centre.y {
                    return None;
                }
                let centre = Vec2::new(w.x, w.z);
                let d = self.chord(home, centre);
                if !(self.is_land(centre) && d <= radius) {
                    return None;
                }
                let trees: Vec<FruitTree> = g
                    .plants
                    .iter()
                    .filter(|p| p.class() < 2)
                    .filter_map(|p| {
                        let w = sp.frame.to_world(p.pos);
                        let at = Vec2::new(w.x, w.z);
                        self.is_land(at).then_some(FruitTree { at, class: p.class(), scale: p.scale })
                    })
                    .collect();
                (!trees.is_empty()).then_some((d, trees))
            })
            .collect();
        groves.sort_by(|a, b| a.0.total_cmp(&b.0));
        groves.into_iter().map(|g| g.1).collect()
    }

    /// The sea-level sphere point on the vertical line through chart `p`
    /// (world space); `None` beyond the sphere's silhouette.
    fn sphere_point(sp: &StagePlanet, p: Vec2) -> Option<Vec3> {
        let c = sp.frame.centre;
        let r = sp.planet.water_radius();
        let (dx, dz) = (p.x - c.x, p.y - c.z);
        let d2 = dx * dx + dz * dz;
        (d2 < r * r).then(|| Vec3::new(p.x, c.y + (r * r - d2).sqrt(), p.y))
    }

    /// The surface normal at world point `at`: away from the planet's centre,
    /// the stage pole's +Y without a planet.
    pub fn radial(&self, at: Vec3) -> Vec3 {
        match &self.planet {
            Some(sp) => (at - sp.frame.centre).try_normalize().unwrap_or(Vec3::Y),
            None => Vec3::Y,
        }
    }

    /// World point of chart `p` on the surface (`ToSurface`: the water
    /// surface over the sea), or (x, 0.4, z) without a planet (the island's
    /// herd baseline).
    pub fn world_point(&self, p: Vec2) -> Vec3 {
        match &self.planet {
            Some(sp) => Vec3::new(p.x, sp.surface_y(p.x, p.y), p.y),
            None => Vec3::new(p.x, 0.4, p.y),
        }
    }

    /// Whether chart `p` is dry land. The ground is sampled along the
    /// direction of the sea-level point on the vertical line, which is the
    /// exact `IsInWater` test at the waterline and keeps inland points inland.
    pub fn is_land(&self, p: Vec2) -> bool {
        let Some(sp) = &self.planet else {
            return true;
        };
        if !p.is_finite() {
            return false;
        }
        let Some(s) = Self::sphere_point(sp, p) else {
            return false;
        };
        let local = sp.frame.dir_to_local((s - sp.frame.centre).normalize());
        !sp.planet.is_in_water(local)
    }

    /// Whether any water lies within `radius` of chart `p` (three rings of
    /// twelve samples; `WaterDistance` checks).
    pub fn water_within(&self, p: Vec2, radius: f32) -> bool {
        if self.planet.is_none() || radius <= 0.0 {
            return false;
        }
        if !self.is_land(p) {
            return true;
        }
        for ring in [0.35, 0.7, 1.0] {
            for k in 0..12 {
                let a = k as f32 / 12.0 * std::f32::consts::TAU + ring;
                if !self.is_land(p + Vec2::new(a.cos(), a.sin()) * radius * ring) {
                    return true;
                }
            }
        }
        false
    }

    /// Retail distance between two chart points: the chord between their
    /// sea-level sphere points (heights ignored: at most a few per cent over
    /// the herd bands), planar without a planet.
    pub fn chord(&self, a: Vec2, b: Vec2) -> f32 {
        match &self.planet {
            Some(sp) => match (Self::sphere_point(sp, a), Self::sphere_point(sp, b)) {
                (Some(a), Some(b)) => a.distance(b),
                _ => f32::INFINITY,
            },
            None => a.distance(b),
        }
    }

    /// The chart point at chord `distance` from `from` along bearing `angle`
    /// (0 = +x, pi/2 = +z, the planar convention), on the sea-level sphere.
    /// Points beyond the near hemisphere of the stage pole are pulled back to
    /// it (the chart cannot hold them).
    pub fn toward(&self, from: Vec2, distance: f32, angle: f32) -> Vec2 {
        let planar = from + Vec2::new(angle.cos(), angle.sin()) * distance;
        let Some(sp) = &self.planet else {
            return planar;
        };
        let Some(s) = Self::sphere_point(sp, from) else {
            return planar;
        };
        let c = sp.frame.centre;
        let r = sp.planet.water_radius();
        let u = (s - c) / r;
        let t0 = Vec3::new(angle.cos(), 0.0, angle.sin());
        let t = (t0 - u * u.dot(t0)).normalize_or(t0);
        let mut theta = 2.0 * (distance / (2.0 * r)).clamp(0.0, 1.0).asin();
        let mut q = u * theta.cos() + t * theta.sin();
        // Keep the result on the chart (at most ~80 degrees from the pole).
        for _ in 0..8 {
            if q.y >= 0.17 {
                break;
            }
            theta *= 0.8;
            q = u * theta.cos() + t * theta.sin();
        }
        let q = c + q * r;
        Vec2::new(q.x, q.z)
    }

    /// The stay-on-land rule for one step from `from` to `to` (world
    /// positions; y is left to the caller): a step onto the sea slides along
    /// the coast on one axis, or stops. Creatures already in the water may
    /// move freely, so nothing is ever trapped.
    pub fn step(&self, from: Vec3, to: Vec3) -> Vec3 {
        if self.planet.is_none() || self.is_land(to.xz()) || !self.is_land(from.xz()) {
            return to;
        }
        let slide_x = Vec3::new(to.x, to.y, from.z);
        if self.is_land(slide_x.xz()) {
            return slide_x;
        }
        let slide_z = Vec3::new(from.x, to.y, to.z);
        if self.is_land(slide_z.xz()) {
            return slide_z;
        }
        Vec3::new(from.x, to.y, from.z)
    }

    /// `to` when it is land, else the last land point on the segment from
    /// `from` (a goal is never set out at sea).
    pub fn land_toward(&self, from: Vec2, to: Vec2) -> Vec2 {
        if self.is_land(to) || !self.is_land(from) {
            return to;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..10 {
            let mid = 0.5 * (lo + hi);
            if self.is_land(from.lerp(to, mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // A step back from the waterline.
        let length = from.distance(to).max(1e-3);
        from.lerp(to, (lo - 0.8 / length).max(0.0))
    }

    /// The nearest shore within `reach` of `home` (the land point just before
    /// the first water along the closest of twelve bearings, the first one
    /// turned by `roll`), for the herds' drinking trips.
    pub fn shore(&self, home: Vec2, reach: f32, roll: f32) -> Option<Vec2> {
        self.planet.as_ref()?;
        let mut best: Option<(f32, Vec2)> = None;
        let step = 3.0f32;
        for k in 0..12 {
            let a = (k as f32 / 12.0 + roll) * std::f32::consts::TAU;
            let dir = Vec2::new(a.cos(), a.sin());
            let mut d = step;
            while d <= reach && best.is_none_or(|(b, _)| d < b) {
                if !self.is_land(home + dir * d) {
                    let shore = home + dir * (d - step).max(0.0);
                    best = Some((d, self.land_toward(shore, home + dir * d)));
                    break;
                }
                d += step;
            }
        }
        best.map(|(_, p)| p)
    }
}

/// The Creature stage's planet frame: the planet's stage origin at world +Y
/// (planet-re 4C), or the frame the last re-anchor saved.
pub fn creature_frame(cache: &crate::terrain::PlanetCache, saved: Option<[f32; 4]>) -> PlanetFrame {
    let mut frame = PlanetFrame::new(cache.base.radius, cache.origin);
    if let Some(rotation) = saved
        .map(Quat::from_array)
        .filter(|q| q.is_finite() && (q.length_squared() - 1.0).abs() < 1e-3)
    {
        frame.rotation = rotation.normalize();
    }
    frame
}

/// The planet as the species world generation sees it, before
/// `terrain::prepare` builds the stage surface: the cached homeworld of
/// `seed` in the Creature frame (`saved`: `SpeciesWorld::planet_frame`; the
/// site pads are not levelled yet; nests are placed on the unmodified
/// terrain).
pub fn site_land(cache: Option<&crate::terrain::PlanetCache>, seed: u64, saved: Option<[f32; 4]>) -> Land {
    match cache.filter(|c| c.seed == seed) {
        Some(cache) => Land::new(Arc::new(StagePlanet {
            planet: (*cache.base).clone(),
            frame: creature_frame(cache, saved),
        })),
        None => Land::flat(),
    }
}

/// Per frame in the Creature stage on the planet (PostUpdate, before the
/// planet renderer): give the camera rig the avatar's local up, and when the
/// avatar is `REANCHOR_DISTANCE` from the pole, turn the planet and the stage
/// about the planet centre so the avatar stands at the pole again.
pub fn reanchor(world: &mut World) {
    let Some(surface) = world.get_resource::<crate::terrain::LandSurface>() else {
        return;
    };
    if surface.stage != GameStage::Creature
        || world.get_resource::<State<GameStage>>().is_none_or(|s| *s.get() != GameStage::Creature)
    {
        return;
    }
    let Some(old) = surface.planet_arc() else {
        return;
    };
    let seed = surface.seed;
    let mut players = world.query_filtered::<&Transform, With<crate::game::Player>>();
    let Ok(avatar) = players.single(world).map(|t| t.translation) else {
        return;
    };
    if !avatar.is_finite() {
        return;
    }
    let up = old.frame.up(avatar);
    if let Some(mut camera) = world.get_resource_mut::<crate::play_camera::PlayCamera>() {
        if camera.rig().up() != up {
            camera.rig_mut().set_up(up);
        }
    }
    if Vec2::new(avatar.x, avatar.z).length() < reanchor_distance() {
        return;
    }
    let rotation = Quat::from_rotation_arc(up, Vec3::Y);
    let shift = Shift::new(old.clone(), rotation);
    let frame = PlanetFrame {
        centre: old.frame.centre,
        rotation: (rotation * old.frame.rotation).normalize(),
    };
    let planet = Arc::new(StagePlanet {
        planet: old.planet.clone(),
        frame,
    });
    world.insert_resource(crate::terrain::LandSurface::on_planet(GameStage::Creature, seed, planet.clone()));
    crate::planet_render::reframe(world, planet);
    // Everything standing on the planet turns with it (children ride along).
    let mut roots = world.query_filtered::<&mut Transform, (
        Without<ChildOf>,
        Or<(With<StageOwned>, With<crate::GameCamera>)>,
        Without<crate::terrain::WorldGround>,
        Without<Node>,
    )>();
    for mut t in roots.iter_mut(world) {
        t.translation = shift.exact(t.translation);
        t.rotation = (rotation * t.rotation).normalize();
    }
    for mut home in world.query::<&mut crate::creature::NestHome>().iter_mut(world) {
        (home.x, home.z) = shift.chart(home.x, home.z);
    }
    for mut wander in world.query::<&mut crate::creature::Wander>().iter_mut(world) {
        (wander.tx, wander.tz) = shift.chart(wander.tx, wander.tz);
    }
    for mut actor in world.query::<&mut crate::epic_creatures::RoamerActor>().iter_mut(world) {
        actor.lair = shift.point(actor.lair);
        actor.goal = shift.point(actor.goal);
    }
    for mut walk in world.query::<&mut crate::locomotion_preview::AdaptedWalk>().iter_mut(world) {
        walk.shift(|p| shift.exact(p));
    }
    if let Some(mut controls) = world.get_resource_mut::<crate::creature_controls::CreatureControls>() {
        if let Some(goal) = controls.goal.as_mut() {
            goal.point = shift.point(goal.point);
        }
    }
    if let Some(mut camera) = world.get_resource_mut::<crate::play_camera::PlayCamera>() {
        let rig = camera.rig_mut();
        rig.shift(|p| shift.exact(p));
        rig.set_up(Vec3::Y);
    }
    {
        let mut run = world.resource_mut::<crate::game::RunProgress>();
        let run = &mut *run;
        for nest in run.species_world.nests.iter_mut() {
            nest.position = shift.array(nest.position);
        }
        for roamer in run.species_world.roamers.iter_mut() {
            roamer.lair = shift.array(roamer.lair);
        }
        if let Some(carcass) = run.species_world.carcass.as_mut() {
            *carcass = shift.array(*carcass);
        }
        run.creature_lifecycle.home = shift.array(run.creature_lifecycle.home);
        if let Some(stay) = run.creature_pack.stay_position.as_mut() {
            *stay = shift.array(*stay);
        }
        if let Some(migration) = run.creature_progression.pending_migration.as_mut() {
            migration.origin = shift.array(migration.origin);
            migration.destination = shift.array(migration.destination);
        }
        run.species_world.planet_frame = Some(frame.rotation.to_array());
        // The checkpoint is captured again in this frame (before the next
        // save) so saved positions and the saved frame always agree.
        run.request_save();
    }
    let generation = world.get_resource::<Reanchored>().map_or(0, |r| r.generation) + 1;
    info!("creature planet: turned the planet under the avatar ({generation} this visit)");
    world.insert_resource(Reanchored {
        generation,
        centre: frame.centre,
        rotation,
    });
}

/// Make sure the homeworld of `seed` is cached before the species world is
/// generated (`terrain::prepare` reuses it). Returns the cache to insert when
/// it had to be generated.
pub fn ensure_site_cache(
    index: &crate::asset_catalog::ResourceIndex,
    cache: Option<&crate::terrain::PlanetCache>,
    seed: u64,
) -> Option<crate::terrain::PlanetCache> {
    if cache.is_some_and(|c| c.seed == seed) {
        return None;
    }
    crate::terrain::generate_planet_cache(index, seed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, Planet, PLANET_RADIUS};

    /// A test planet in the stage frame: land within `land_radius` units of
    /// arc around the pole (+Y of the frame, here planet +X), sea beyond.
    pub(crate) fn island_planet(land_radius: f32) -> Land {
        let res = 64;
        let mut cube = HeightCube::filled(res, -0.3);
        let edge = (land_radius / PLANET_RADIUS).cos();
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    let h = ((d.x - edge) * 3.0).clamp(-0.3, 0.3);
                    cube.raw_mut()[i] = h_to_raw(h);
                }
            }
        }
        let planet = Planet::new(cube, PLANET_RADIUS, 0.0);
        Land::new(Arc::new(StagePlanet {
            planet,
            frame: PlanetFrame::new(PLANET_RADIUS, Vec3::X),
        }))
    }

    /// `island_planet` as the Creature stage's surface resource.
    pub(crate) fn island_surface(land_radius: f32) -> crate::terrain::LandSurface {
        let land = island_planet(land_radius);
        crate::terrain::LandSurface::on_planet(crate::stages::GameStage::Creature, 0, land.planet.clone().unwrap())
    }

    #[test]
    fn land_and_sea_follow_the_height_field() {
        let land = island_planet(200.0);
        assert!(land.is_land(Vec2::ZERO));
        assert!(land.is_land(Vec2::new(150.0, 0.0)));
        assert!(!land.is_land(Vec2::new(260.0, 0.0)));
        assert!(!land.is_land(Vec2::new(0.0, -300.0)));
        // Beyond the silhouette nothing is land.
        assert!(!land.is_land(Vec2::new(600.0, 0.0)));
        assert!(land.water_within(Vec2::new(180.0, 0.0), 40.0));
        assert!(!land.water_within(Vec2::new(0.0, 0.0), 40.0));
        let flat = Land::flat();
        assert!(flat.is_land(Vec2::new(1e4, 0.0)) && !flat.water_within(Vec2::ZERO, 100.0));
    }

    #[test]
    fn chords_and_bearings_are_retail_distances() {
        let land = island_planet(200.0);
        for (d, a) in [(35.0, 0.3), (120.0, 2.0), (250.0, -1.2), (350.0, 4.0)] {
            let p = land.toward(Vec2::new(0.0, 8.0), d, a);
            let chord = land.chord(Vec2::new(0.0, 8.0), p);
            assert!((chord - d).abs() < 0.05, "{d} -> {chord}");
            // The bearing is the planar one near the pole.
            let dir = (p - Vec2::new(0.0, 8.0)).normalize();
            assert!(dir.dot(Vec2::new(a.cos(), a.sin())) > 0.999, "{dir}");
        }
        // The chart distance is shorter than the chord away from the pole.
        let far = land.toward(Vec2::ZERO, 300.0, 0.0);
        assert!(far.length() < 300.0 && far.length() > 280.0, "{far}");
        let flat = Land::flat();
        assert_eq!(flat.toward(Vec2::ZERO, 10.0, 0.0), Vec2::new(10.0, 0.0));
        assert_eq!(flat.chord(Vec2::ZERO, Vec2::new(3.0, 4.0)), 5.0);
    }

    #[test]
    fn a_re_anchor_turns_stored_points_with_the_planet() {
        let land = island_planet(400.0);
        let old = land.planet.clone().unwrap();
        // The avatar 200 units out along +x becomes the pole.
        let avatar = Vec3::new(200.0, old.surface_y(200.0, 0.0), 0.0);
        let rotation = Quat::from_rotation_arc(old.frame.up(avatar), Vec3::Y);
        let shift = Shift::new(old.clone(), rotation);
        let pole = shift.exact(avatar);
        assert!(pole.x.abs() < 1e-3 && pole.z.abs() < 1e-3, "{pole}");
        // A stored baseline point is lifted to the surface first.
        let home = shift.array([0.0, 0.7, 8.0]);
        let truth = shift.exact(Vec3::new(0.0, old.surface_y(0.0, 8.0), 8.0));
        assert!(Vec3::from_array(home).distance(truth) < 1e-3);
        // Chords between stored points are kept.
        let a = Vec3::new(30.0, old.surface_y(30.0, 40.0), 40.0);
        let b = Vec3::new(-90.0, old.surface_y(-90.0, 5.0), 5.0);
        assert!((shift.point(a).distance(shift.point(b)) - a.distance(b)).abs() < 1e-2);
        // The new frame sees the same ground under the turned points.
        let new = StagePlanet {
            planet: old.planet.clone(),
            frame: PlanetFrame {
                centre: old.frame.centre,
                rotation: rotation * old.frame.rotation,
            },
        };
        let moved = shift.point(a);
        assert!((new.surface_y(moved.x, moved.z) - moved.y).abs() < 0.05);
        // A point beyond the limb keeps its height and stays apart from the
        // chart point with the same x, z.
        let far = [10.0, -900.0, 0.0];
        assert!(!same_side(far, [10.0, 2.0, 0.0]) && same_side([0.0, 0.7, 8.0], [0.0, 12.0, 8.0]));
        let saved = creature_frame(
            &crate::terrain::PlanetCache {
                seed: 1,
                script: crate::planet_data::PlanetScript {
                    key: crate::dbpf::ResourceKey { group_id: 0, instance_id: 0, type_id: 0 },
                    description: String::new(),
                    terrain_script: 0,
                    first_group: true,
                    exclude_from_gge: false,
                },
                base: Arc::new(old.planet.clone()),
                origin: Vec3::X,
                stats: Default::default(),
            },
            Some(new.frame.rotation.to_array()),
        );
        assert!((saved.rotation.dot(new.frame.rotation)).abs() > 0.9999);
    }

    #[test]
    fn walking_far_turns_the_planet_under_the_avatar() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(GameStage::Creature);
        let surface = island_surface(700.0);
        let old = surface.planet_arc().unwrap();
        app.insert_resource(surface);
        app.insert_resource(crate::play_camera::PlayCamera::new(GameStage::Creature));
        let mut run = crate::game::RunProgress::default();
        run.species_world.nests.push(crate::species::Nest {
            species: 0,
            position: [0.0, 0.4, 100.0],
            members: Vec::new(),
            rewarded: false,
            herd_size: 0,
            respawn_timer: 0.0,
        });
        app.insert_resource(run);
        app.update();
        let ground = |x: f32, z: f32| Vec3::new(x, old.surface_y(x, z), z);
        let start = ground(160.0, 30.0);
        let player = app.world_mut().spawn((crate::game::Player, StageOwned, Transform::from_translation(start))).id();
        let tree = app.world_mut().spawn((StageOwned, Transform::from_translation(ground(-20.0, 60.0)))).id();
        let nest = ground(0.0, 100.0);
        // Near the pole nothing turns.
        app.world_mut().get_mut::<Transform>(player).unwrap().translation = ground(40.0, 0.0);
        reanchor(app.world_mut());
        assert!(app.world().get_resource::<Reanchored>().is_none());
        app.world_mut().get_mut::<Transform>(player).unwrap().translation = start;
        reanchor(app.world_mut());
        let at = app.world().get::<Transform>(player).unwrap().translation;
        assert!(Vec2::new(at.x, at.z).length() < 1e-2, "the avatar is at the pole: {at}");
        let tree_at = app.world().get::<Transform>(tree).unwrap().translation;
        assert!((tree_at.distance(at) - ground(-20.0, 60.0).distance(start)).abs() < 1e-2);
        let world = app.world();
        let run = world.resource::<crate::game::RunProgress>();
        let moved = Vec3::from_array(run.species_world.nests[0].position);
        assert!((moved.distance(at) - nest.distance(start)).abs() < 0.05, "chords kept");
        assert!(run.species_world.planet_frame.is_some() && run.save_requested);
        let surface = world.resource::<crate::terrain::LandSurface>();
        let new = surface.planet().unwrap();
        assert!((new.surface_y(at.x, at.z) - at.y).abs() < 0.05, "the ground under the avatar");
        assert!((new.surface_y(moved.x, moved.z) - moved.y).abs() < 0.05, "the ground under the nest");
        assert_eq!(world.resource::<Reanchored>().generation, 1);
        assert!((world.resource::<crate::play_camera::PlayCamera>().rig().up() - Vec3::Y).length() < 1e-4);
    }

    #[test]
    fn creatures_stop_at_the_waterline_and_goals_stay_ashore() {
        let land = island_planet(200.0);
        let from = Vec3::new(185.0, 0.0, 0.0);
        let to = Vec3::new(215.0, 0.0, 0.0);
        let stepped = land.step(from, to);
        assert!(land.is_land(stepped.xz()), "{stepped}");
        // Along the coast a diagonal step keeps its coastwise part.
        let along = land.step(from, Vec3::new(215.0, 0.0, 5.0));
        assert!(land.is_land(along.xz()) && along.z > 4.0, "{along}");
        // Out of the water, every step is allowed.
        assert_eq!(land.step(Vec3::new(300.0, 0.0, 0.0), Vec3::new(301.0, 0.0, 0.0)).x, 301.0);
        let goal = land.land_toward(Vec2::ZERO, Vec2::new(400.0, 0.0));
        assert!(land.is_land(goal) && goal.x > 180.0, "{goal}");
        let shore = land.shore(Vec2::new(150.0, 0.0), 120.0, 0.0).unwrap();
        assert!(land.is_land(shore) && shore.x > 170.0, "{shore}");
        assert!(land.shore(Vec2::ZERO, 50.0, 0.0).is_none());
    }
}
