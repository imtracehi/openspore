//! The Tribal stage on the original's planet (planet-re 4E step 5).
//!
//! Layout (notes\tribal-re-notes.md T4, `noun_placement_markers~` of the
//! TribeGameLevelGenerationList, distances from the stage origin, measured as
//! retail does: straight-line chords between surface points):
//!
//! * the player's tribe (`playertribe`): 40-60 from the origin, water 30-50
//!   away, 0.5-35 above the sea, slope at most 20 degrees;
//! * the NPC tribes (`nearbyNPCTribe` 175-225, `farNPCTribe` 175-350,
//!   `fillerNPCTribe` 200-450, `failsafeNPCTribe` 200-500), on land, at
//!   least their `WaterDistance` from the sea and their exclusive radius
//!   (115) from every other tribe;
//! * every tribe levels its ground (`AddPlayerModification` with
//!   `brush_tribe_flattener`, planet-re 2D): a flatten stamp on the planet.
//!
//! Frame: the Tribal stage puts the player's hut at the world +Y pole (the
//! planet frame of planet-re 4C turned to the hut instead of the origin), so
//! the village code keeps its hut at (0, 0, 0) and its chart coordinates
//! (world x, z on the `LandSurface` facade). The whole layout fits the
//! hemisphere around the hut (the failsafe ring ends ~560 units, 68 degrees,
//! from it). ADAPTED: units integrate in that chart (steps are stretched to
//! retail surface lengths, see [`TribalLayout::stretch`]) and stay on land
//! within [`CHART_LIMIT`] of the hut; the camera is not bound to the chart
//! and can circle the planet (`tribal_controls`). The Creature stage's
//! floating origin (`creature_planet::reanchor`) is not needed for that.
//!
//! The layout is computed when the stage is entered (`prepare`, from
//! `tribal::setup_tribal`, before anything is spawned) and kept per app as the
//! [`StageLayout`] resource: `tribal::village_position`, `fishing_spots`,
//! `keep_on_land` and `tribal_rts::step_toward` read it through [`current`],
//! and `terrain::prepare` takes the levelled planet from it. The Tribal
//! systems enter the app's layout with [`scope`] / [`scope_world`] (a
//! thread-local for the duration of the system), so parallel apps (unit
//! tests) never see each other's layout; outside a scope the game falls back
//! to the last installed layout (test builds: the flat one). Without a planet (headless runs,
//! `OPENSPORE_FLAT_TERRAIN=1`) the flat layout puts the tribes at the middle
//! of their rings around the hut and keeps the prototype island's shore for
//! fishing.

use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::creature_planet::Land;
use crate::planet_query::{tangent_basis, Planet, PlanetFrame, StagePlanet};
use crate::tribal_rules as rules;

/// The rival villages (one NPC tribe per tier).
pub const RIVALS: usize = crate::tribal::RIVAL_VILLAGES;

/// ADAPTED: half width of the flatten stamp under each village: the tribe's
/// zoning radius (30, cTribe +0x330). The installed `brush_tribe_flattener`
/// effect is not in the planet effect directory that is decoded.
pub const PAD_HALF_WIDTH: f32 = rules::TRIBE_ZONING_RADIUS;

/// ADAPTED: how far from the hut the fishing spots are looked for on the
/// real shore (the player tribe stands 30-50 from the water).
pub const FISHING_REACH: f32 = 120.0;
/// Fishing spots on the shore (one per rival village, as before).
pub const FISHING_SPOTS: usize = RIVALS;
/// ADAPTED: units stay within this chart distance of the hut (about 74
/// degrees of arc, beyond every placement ring); the vertical-line chart
/// ends at the planet's limb.
pub const CHART_LIMIT: f32 = 480.0;

/// Where everything of the Tribal stage stands, in the stage chart.
#[derive(Clone, Debug)]
pub struct TribalLayout {
    pub seed: u64,
    /// The levelled planet (land and sea queries); flat without a planet.
    pub land: Land,
    /// The levelled stage planet for `terrain::prepare`.
    pub planet: Option<Arc<StagePlanet>>,
    /// Chart point of the stage origin the placement rings are measured from.
    pub origin: Vec2,
    pub villages: [Vec3; RIVALS],
    /// Shore points the fishers stand at (land, just before the water).
    pub fishing: Vec<Vec3>,
    /// The navigation grid walkers route over (`tribal_path`); `None` without
    /// a planet (everything is open ground).
    pub nav: Option<Arc<crate::tribal_path::NavGrid>>,
    /// The swimmers' grid (`NavGrid::build_swim`: water is open, cliffs block);
    /// `None` without a planet.
    pub nav_swim: Option<Arc<crate::tribal_path::NavGrid>>,
    /// The tribe roads (chart polylines, `tribal_path::tribe_roads`).
    pub roads: Vec<Vec<Vec2>>,
}

static CURRENT: RwLock<Option<Arc<TribalLayout>>> = RwLock::new(None);

/// The app's Tribal layout (inserted by `tribal::setup_tribal`).
#[derive(Resource, Clone)]
pub struct StageLayout(pub Arc<TribalLayout>);

thread_local! {
    static SCOPED: std::cell::RefCell<Option<Arc<TribalLayout>>> = const { std::cell::RefCell::new(None) };
}

/// While alive, [`current`] on this thread returns the scoped layout; the
/// previous scope is restored on drop.
pub struct Scope(Option<Arc<TribalLayout>>);

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPED.with(|s| *s.borrow_mut() = previous);
    }
}

/// Enter `layout` (no change when `None`).
pub fn scope_arc(layout: Option<Arc<TribalLayout>>) -> Scope {
    match layout {
        Some(layout) => Scope(SCOPED.with(|s| s.borrow_mut().replace(layout))),
        None => Scope(SCOPED.with(|s| s.borrow().clone())),
    }
}

/// Enter the app's layout resource (systems: `Option<Res<StageLayout>>`).
pub fn scope(layout: Option<&StageLayout>) -> Scope {
    scope_arc(layout.map(|l| l.0.clone()))
}

/// Enter the layout of `world` (exclusive systems, tests).
pub fn scope_world(world: &World) -> Scope {
    scope(world.get_resource::<StageLayout>())
}

/// The layout in scope, else the installed one, else the flat layout.
pub fn current() -> Arc<TribalLayout> {
    if let Some(layout) = SCOPED.with(|s| s.borrow().clone()) {
        return layout;
    }
    if let Some(layout) = CURRENT.read().ok().and_then(|g| g.clone()) {
        return layout;
    }
    static FLAT: std::sync::OnceLock<Arc<TribalLayout>> = std::sync::OnceLock::new();
    FLAT.get_or_init(|| Arc::new(flat_layout(0))).clone()
}

/// Make `layout` the stage's layout. The process-wide fallback is only
/// written by the game: test apps run in parallel and read their own
/// [`StageLayout`] through [`scope`].
pub fn install(layout: TribalLayout) -> Arc<TribalLayout> {
    let layout = Arc::new(layout);
    #[cfg(not(test))]
    if let Ok(mut slot) = CURRENT.write() {
        *slot = Some(layout.clone());
    }
    layout
}

/// The levelled planet of the installed layout for `seed` (`terrain::prepare`).
pub fn stage_planet(seed: u64) -> Option<Arc<StagePlanet>> {
    let layout = current();
    (layout.seed == seed).then(|| layout.planet.clone()).flatten()
}

impl TribalLayout {
    pub fn on_planet(&self) -> bool {
        self.land.on_planet()
    }

    pub fn village(&self, index: usize) -> Vec3 {
        self.villages[index.min(RIVALS - 1)]
    }

    /// Surface length of one chart unit along `dir` at `p` (1 near the hut,
    /// more toward the limb, where the chart is compressed radially).
    pub fn stretch(&self, p: Vec2, dir: Vec2) -> f32 {
        if !self.on_planet() || !p.is_finite() || !dir.is_finite() {
            return 1.0;
        }
        let k = self.land.chord(p, p + dir * 2.0) / 2.0;
        if k.is_finite() {
            k.clamp(1.0, 4.0)
        } else {
            1.0
        }
    }

    /// One step of a walker from `from` to `to` (world, y left alone): it
    /// stays on land (the Creature rule: a step onto the sea slides along the
    /// coast or stops) and within [`CHART_LIMIT`] of the hut. Herd animals and
    /// pets walk like this; tribe members and raiders use [`Self::swim_step`].
    pub fn walk(&self, from: Vec3, to: Vec3) -> Vec3 {
        if self.beyond_chart(from, to) {
            return from;
        }
        self.land.step(from, to)
    }

    /// One step of a swimmer (a tribe member or raider; T9J: retail `WalkTo`
    /// is a straight goal and a creature entering water swims): the same step
    /// within [`CHART_LIMIT`], over land and water alike.
    pub fn swim_step(&self, from: Vec3, to: Vec3) -> Vec3 {
        if self.beyond_chart(from, to) {
            return from;
        }
        to
    }

    /// Whether the step leaves the chart (a step back in is always allowed).
    fn beyond_chart(&self, from: Vec3, to: Vec3) -> bool {
        self.on_planet() && to.xz().length() > from.xz().length() && to.xz().length() > CHART_LIMIT
    }

    /// Whether chart point `p` is over water (a member there swims; the
    /// Creature stage's `IsInWater` threshold, ADAPTED: the retail depth at
    /// which the locomotion strategy switches to swimming is not traced).
    pub fn in_water(&self, p: Vec2) -> bool {
        self.on_planet() && !self.land.is_land(p)
    }

    /// Where a walker at `from` heading for `to` aims now: the next visible
    /// corner of its route around water and cliffs (`tribal_path`), or `to`.
    pub fn waypoint(&self, from: Vec3, to: Vec3) -> Vec3 {
        match &self.nav {
            Some(nav) => {
                let w = nav.waypoint(from.xz(), to.xz());
                Vec3::new(w.x, to.y, w.y)
            }
            None => to,
        }
    }

    /// Where a swimmer at `from` heading for `to` aims now: `to` itself unless
    /// a cliff is in the way (`tribal_path`, the swimmers' grid).
    pub fn swim_waypoint(&self, from: Vec3, to: Vec3) -> Vec3 {
        match &self.nav_swim {
            Some(nav) => {
                let w = nav.waypoint(from.xz(), to.xz());
                Vec3::new(w.x, to.y, w.y)
            }
            None => to,
        }
    }

    /// A goal on land: points out at sea are pulled back along the line from
    /// the hut to the last land point, and into the chart.
    pub fn keep_on_land(&self, pos: Vec3) -> Vec3 {
        if !self.on_planet() || !pos.is_finite() {
            return pos;
        }
        let mut p = pos.xz();
        if p.length() > CHART_LIMIT {
            p = p.normalize() * CHART_LIMIT;
        }
        let p = self.land.land_toward(Vec2::ZERO, p);
        Vec3::new(p.x, pos.y, p.y)
    }
}

/// The layout without a planet: the tribes at the middle of their rings
/// around the hut, evenly spread; fishing on the prototype island's shore.
pub fn flat_layout(seed: u64) -> TribalLayout {
    let mut villages = [Vec3::ZERO; RIVALS];
    for (index, v) in villages.iter_mut().enumerate() {
        let (lo, hi) = rules::village_marker(index).distance_from_origin;
        let angle = (index as f32 + 0.5) * std::f32::consts::TAU / RIVALS as f32;
        let ring = (lo + hi) * 0.5;
        *v = Vec3::new(angle.cos() * ring, 0.0, angle.sin() * ring);
    }
    let fishing = (0..FISHING_SPOTS)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / FISHING_SPOTS as f32;
            let dir = Vec2::new(angle.cos(), angle.sin());
            // tribal_rts puts each ripple 1.6 units farther out.
            let r = crate::tribal::coastline_radius(seed, dir) - 1.2;
            Vec3::new(dir.x * r, 0.0, dir.y * r)
        })
        .collect();
    TribalLayout {
        seed,
        land: Land::flat(),
        planet: None,
        origin: Vec2::ZERO,
        villages,
        fishing,
        nav: None,
        nav_swim: None,
        roads: Vec::new(),
    }
}

/// Planet-local direction of the player's tribe: a `playertribe` site
/// 40-60 from the stage origin `origin` with the nearest water 30-50 away,
/// 0.5-35 units above the sea, slope at most 20 degrees and land all around
/// the hut (the marker's "within 30 degrees of the equator" is not used:
/// OpenSpore's planets have no axis yet). The best scored candidate of a
/// deterministic ring set; the origin itself without any land candidate.
pub fn home_site(planet: &Planet, origin: Vec3, seed: u64) -> Vec3 {
    let origin = origin.normalize_or(Vec3::Y);
    let radius = planet.radius;
    let wl = planet.water_level;
    let wet = |q: Vec3| planet.h_at(q) <= wl;
    let at = |base: Vec3, dist: f32, ang: f32| {
        let (t, b) = tangent_basis(base);
        let theta = dist / radius;
        (base * theta.cos() + (t * ang.cos() + b * ang.sin()) * theta.sin()).normalize()
    };
    let marker = &rules::PLAYER_TRIBE_MARKER;
    let (dlo, dhi) = marker.distance_from_origin;
    let (wlo, whi) = marker.water_distance;
    let spin = (seed % 101) as f32 * 0.37;
    let mut best: Option<(f32, Vec3)> = None;
    for ring in 0..5 {
        let dist = dlo + (dhi - dlo) * ring as f32 / 4.0;
        for k in 0..24 {
            let q = at(origin, dist, k as f32 / 24.0 * std::f32::consts::TAU + spin);
            if wet(q) {
                continue;
            }
            let altitude = (planet.h_at(q) - wl) * planet.altitude_range;
            let mut nearest = f32::MAX;
            for j in 0..16 {
                let a = j as f32 / 16.0 * std::f32::consts::TAU;
                let mut d = 5.0;
                while d <= whi + 20.0 && d < nearest {
                    if wet(at(q, d, a)) {
                        nearest = d;
                        break;
                    }
                    d += 5.0;
                }
            }
            let slope = planet.ground_normal(q).dot(q).clamp(-1.0, 1.0).acos().to_degrees();
            let pad = (0..12)
                .filter(|j| {
                    let a = *j as f32 / 12.0 * std::f32::consts::TAU;
                    !wet(at(q, 8.0, a)) && !wet(at(q, 16.0, a))
                })
                .count() as f32
                / 12.0;
            let water_miss = if nearest == f32::MAX {
                40.0
            } else {
                (wlo - nearest).max(0.0) + (nearest - whi).max(0.0)
            };
            let altitude_miss = (0.5 - altitude).max(0.0) + (altitude - 35.0).max(0.0);
            let slope_miss = (slope - 20.0).max(0.0);
            let score = pad * 10.0 - water_miss - altitude_miss * 2.0 - slope_miss * 0.5
                - (dist - 0.5 * (dlo + dhi)).abs() * 0.01;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, q));
            }
        }
    }
    best.map_or(origin, |(_, q)| q)
}

/// Whether the straight line between two chart points stays on land (8-unit
/// samples).
fn land_path(land: &Land, a: Vec2, b: Vec2) -> bool {
    let n = (a.distance(b) / 8.0).ceil().max(1.0) as usize;
    (1..n).all(|i| land.is_land(a.lerp(b, i as f32 / n as f32)))
}

/// The NPC tribes on `land` (chart, hut at the pole), ring distances from
/// the chart point `origin`. Pass 0 keeps every marker rule and a straight
/// walk from the hut on land (ADAPTED, kept so saved layouts do not move:
/// members now swim and walk straight (tribal-swim), and the original only
/// asks the nearby tribe to be on the same continent); later passes relax the water distance, then the
/// exclusive radius, then the walk; the last resort is the middle of the
/// ring at an even bearing.
pub fn place_villages(land: &Land, origin: Vec2, seed: u64) -> [Vec3; RIVALS] {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x7121_BA1E);
    let mut placed: Vec<Vec2> = Vec::new();
    let mut out = [Vec3::ZERO; RIVALS];
    for (index, slot) in out.iter_mut().enumerate() {
        let marker = rules::village_marker(index);
        let (lo, hi) = marker.distance_from_origin;
        let mut chosen = None;
        'passes: for pass in 0..4 {
            let apart = marker.exclusive_radius * [1.0, 1.0, 0.6, 0.3][pass];
            let water = marker.water_distance.0.max(0.0) * [1.0, 0.5, 0.0, 0.0][pass];
            for _ in 0..240 {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let dist = rng.random_range(lo..=hi);
                let p = land.toward(origin, dist, angle);
                if p.length() > CHART_LIMIT - PAD_HALF_WIDTH || !land.is_land(p) {
                    continue;
                }
                if land.chord(p, Vec2::ZERO) < apart || placed.iter().any(|q| land.chord(p, *q) < apart) {
                    continue;
                }
                if water > 0.0 && land.water_within(p, water) {
                    continue;
                }
                if pass < 3 && !land_path(land, Vec2::ZERO, p) {
                    continue;
                }
                chosen = Some(p);
                break 'passes;
            }
        }
        let p = chosen.unwrap_or_else(|| {
            let angle = (index as f32 + 0.5) * std::f32::consts::TAU / RIVALS as f32;
            land.toward(origin, (lo + hi) * 0.5, angle)
        });
        placed.push(p);
        *slot = Vec3::new(p.x, 0.0, p.y);
    }
    out
}

/// Fishing spots on the real shore around the hut: along 24 bearings the
/// last land point before the first water within [`FISHING_REACH`], the
/// nearest first, at least 12 units apart.
pub fn shore_spots(land: &Land) -> Vec<Vec3> {
    let mut found: Vec<(f32, Vec2)> = Vec::new();
    for k in 0..24 {
        let a = k as f32 / 24.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        let mut d = 4.0;
        while d <= FISHING_REACH {
            if !land.is_land(dir * d) {
                found.push((d, land.land_toward(dir * (d - 2.0).max(0.0), dir * d)));
                break;
            }
            d += 2.0;
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spots: Vec<Vec2> = Vec::new();
    for (_, p) in found {
        if spots.len() < FISHING_SPOTS && spots.iter().all(|q| q.distance(p) >= 12.0) {
            spots.push(p);
        }
    }
    spots.into_iter().map(|p| Vec3::new(p.x, 0.0, p.y)).collect()
}

/// Where a world ray first meets the planet: the height field or the sea
/// surface, whichever is nearer (click picking at any camera distance).
pub fn ray_hit(sp: &StagePlanet, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO || !origin.is_finite() {
        return None;
    }
    let o = sp.frame.to_local(origin);
    let dl = sp.frame.dir_to_local(d);
    let ground = sp.planet.raycast(o, dl, 4.0 * sp.planet.radius + o.length());
    let r = sp.planet.water_radius();
    let (b, c) = (o.dot(dl), o.length_squared() - r * r);
    let disc = b * b - c;
    let sea = (disc >= 0.0).then(|| -b - disc.sqrt()).filter(|t| *t > 0.0);
    let t = match (ground, sea) {
        (Some(g), Some(s)) => g.min(s),
        (g, s) => g.or(s)?,
    };
    Some(origin + d * t)
}

/// The planet direction of a chart point (on the sea-level sphere).
fn chart_dir(frame: &PlanetFrame, radius: f32, p: Vec2) -> Vec3 {
    let d2 = p.length_squared();
    let y = (radius * radius - d2).max(0.0).sqrt();
    frame.dir_to_local(Vec3::new(p.x, y, p.y).normalize())
}

/// The layout on the homeworld of `cache`: the hut site, the frame with the
/// hut at the pole, the NPC tribes, the flatten stamps (hut and villages) on
/// a copy of the height cube, and the fishing spots on the levelled shore.
pub fn layout_on(cache: &crate::terrain::PlanetCache, seed: u64) -> TribalLayout {
    let base = &cache.base;
    let home = home_site(base, cache.origin, seed);
    let frame = PlanetFrame::new(base.radius, home);
    let site = Land::new(Arc::new(StagePlanet {
        planet: (**base).clone(),
        frame,
    }));
    let o = frame.to_world(cache.origin.normalize_or(Vec3::Y) * base.water_radius());
    let origin = Vec2::new(o.x, o.z);
    let villages = place_villages(&site, origin, seed);
    let mut cube = base.cube.clone();
    for p in std::iter::once(Vec2::ZERO).chain(villages.iter().map(|v| v.xz())) {
        crate::planet_gen::flatten_pad(&mut cube, chart_dir(&frame, base.radius, p), PAD_HALF_WIDTH, base.radius);
    }
    let planet = Arc::new(StagePlanet {
        planet: Planet::new(cube, base.radius, base.water_level),
        frame,
    });
    let land = Land::new(planet.clone());
    let fishing = shore_spots(&land);
    // tribal-rules3: the walkers' grid and the roads over the levelled planet.
    let pads: Vec<Vec2> = std::iter::once(Vec2::ZERO).chain(villages.iter().map(|v| v.xz())).collect();
    let nav = Arc::new(crate::tribal_path::NavGrid::build(&land, &planet, &pads));
    let nav_swim = Arc::new(crate::tribal_path::NavGrid::build_swim(&land, &planet, &pads));
    let roads = crate::tribal_path::tribe_roads(&nav, &land, &villages);
    TribalLayout {
        seed,
        land,
        planet: Some(planet),
        origin,
        villages,
        fishing,
        nav: Some(nav),
        nav_swim: Some(nav_swim),
        roads,
    }
}

/// Compute and install the stage's layout for `seed`: on the cached
/// homeworld when there is one, else the flat layout.
pub fn prepare(cache: Option<&crate::terrain::PlanetCache>, seed: u64) -> Arc<TribalLayout> {
    let layout = match cache.filter(|c| c.seed == seed) {
        Some(cache) => {
            let start = std::time::Instant::now();
            let layout = layout_on(cache, seed);
            let land = &layout.land;
            let o = layout.origin;
            info!(
                "tribal planet: hut {:.0} from the origin, villages at {:?} from the origin, {} fishing spots, {} open path cells, {} roads, {} ms",
                o.length(),
                layout.villages.map(|v| land.chord(v.xz(), o).round()),
                layout.fishing.len(),
                layout.nav.as_ref().map_or(0, |n| n.open_cells()),
                layout.roads.len(),
                start.elapsed().as_millis()
            );
            for road in &layout.roads {
                info!("tribal road: {:?} -> {:?}, {} corners", road.first(), road.last(), road.len());
            }
            layout
        }
        None => flat_layout(seed),
    };
    install(layout)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, PLANET_RADIUS};

    /// The layout on [`lake_planet`] (seed 7), for the swimming tests.
    pub(crate) fn lake_layout() -> TribalLayout {
        layout_on(&cache(lake_planet()), 7)
    }

    /// A continent of 600 units of arc around planet +X with a round lake
    /// (radius 40) 95 units from +X toward +Z, sea beyond the continent.
    fn lake_planet() -> Planet {
        let res = 128;
        let mut cube = HeightCube::filled(res, -0.3);
        let lake = Vec3::new((95.0f32 / PLANET_RADIUS).cos(), 0.0, (95.0f32 / PLANET_RADIUS).sin());
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = texel_dir(face, row, col, res);
                    let arc = d.x.clamp(-1.0, 1.0).acos() * PLANET_RADIUS;
                    let lake_arc = d.dot(lake).clamp(-1.0, 1.0).acos() * PLANET_RADIUS;
                    let h = if arc < 600.0 && lake_arc > 40.0 { 0.05 } else { -0.3 };
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = h_to_raw(h);
                }
            }
        }
        Planet::new(cube, PLANET_RADIUS, 0.0)
    }

    fn cache(planet: Planet) -> crate::terrain::PlanetCache {
        crate::terrain::PlanetCache {
            seed: 7,
            script: crate::planet_data::PlanetScript {
                key: crate::dbpf::ResourceKey { group_id: 0, instance_id: 0, type_id: 0 },
                description: String::new(),
                terrain_script: 0,
                first_group: true,
                exclude_from_gge: false,
            },
            base: Arc::new(planet),
            origin: Vec3::X,
            stats: Default::default(),
        }
    }

    #[test]
    fn the_player_tribe_stands_near_water_40_to_60_from_the_origin() {
        let planet = lake_planet();
        let home = home_site(&planet, Vec3::X, 7);
        let chord = (home - Vec3::X).length() * PLANET_RADIUS;
        assert!((38.0..=62.0).contains(&chord), "{chord}");
        assert!(!planet.is_in_water(home));
        // The lake is the nearest water: 30-50 away (5-unit search steps).
        let lake = Vec3::new((95.0f32 / PLANET_RADIUS).cos(), 0.0, (95.0f32 / PLANET_RADIUS).sin());
        let to_lake = home.dot(lake).acos() * PLANET_RADIUS - 40.0;
        assert!((25.0..=55.0).contains(&to_lake), "{to_lake}");
    }

    #[test]
    fn rival_tribes_follow_their_markers_on_land() {
        let layout = layout_on(&cache(lake_planet()), 7);
        let land = &layout.land;
        assert!(layout.on_planet());
        // The hut is the pole; the origin 40-60 away.
        assert!((35.0..=65.0).contains(&layout.origin.length()), "{}", layout.origin);
        for (i, v) in layout.villages.iter().enumerate() {
            let p = v.xz();
            let (lo, hi) = rules::village_marker(i).distance_from_origin;
            let d = land.chord(p, layout.origin);
            assert!(d >= lo - 1.0 && d <= hi + 1.0, "village {i}: {d} not in {lo}..{hi}");
            assert!(land.is_land(p), "village {i} on land");
            assert!(land.chord(p, Vec2::ZERO) >= 115.0, "village {i} clear of the hut");
            for (j, w) in layout.villages.iter().enumerate().take(i) {
                assert!(land.chord(p, w.xz()) >= 115.0, "villages {i} and {j} apart");
            }
            assert!(land_path(land, Vec2::ZERO, p), "a walk from the hut to village {i}");
        }
        // Fishing spots on the lake shore: land with water right beside.
        assert!(!layout.fishing.is_empty());
        for f in &layout.fishing {
            assert!(land.is_land(f.xz()) && land.water_within(f.xz(), 4.0), "{f}");
            assert!(f.xz().length() <= FISHING_REACH);
        }
        // The hut and every village stand on a levelled pad.
        let planet = layout.planet.as_ref().unwrap();
        for p in std::iter::once(Vec2::ZERO).chain(layout.villages.iter().map(|v| v.xz())) {
            let n = planet.ground_normal_world(Vec3::new(p.x, planet.surface_y(p.x, p.y), p.y));
            let up = planet.frame.up(Vec3::new(p.x, planet.surface_y(p.x, p.y), p.y));
            assert!(n.dot(up) > 0.99, "flat ground at {p}");
        }
    }

    #[test]
    fn walkers_stay_on_land_and_walk_surface_distances() {
        let layout = layout_on(&cache(lake_planet()), 7);
        let spot = layout.fishing[0];
        let out = spot.xz().normalize() * 6.0;
        let wet = spot + Vec3::new(out.x, 0.0, out.y);
        // A goal in the lake is pulled back ashore; a step into it stops.
        let goal = layout.keep_on_land(wet);
        assert!(layout.land.is_land(goal.xz()), "{goal}");
        let stepped = layout.walk(spot, wet);
        assert!(layout.land.is_land(stepped.xz()), "{stepped}");
        // Near the hut one chart unit is one unit; toward the limb more.
        assert!((layout.stretch(Vec2::new(5.0, 0.0), Vec2::X) - 1.0).abs() < 0.01);
        assert!(layout.stretch(Vec2::new(400.0, 0.0), Vec2::X) > 1.5);
        assert!(layout.stretch(Vec2::new(0.0, 400.0), Vec2::X) < 1.05, "across the radius stays");
        // Nobody walks out past the chart's edge.
        let edge = Vec3::new(CHART_LIMIT - 0.5, 0.0, 0.0);
        assert_eq!(layout.walk(edge, edge + Vec3::X * 2.0), edge);
        // The flat layout keeps everything.
        let flat = flat_layout(3);
        assert_eq!(flat.walk(Vec3::ZERO, Vec3::splat(900.0)), Vec3::splat(900.0));
        assert_eq!(flat.stretch(Vec2::new(400.0, 0.0), Vec2::X), 1.0);
        for (i, v) in flat.villages.iter().enumerate() {
            let (lo, hi) = rules::village_marker(i).distance_from_origin;
            assert!((v.length() - (lo + hi) * 0.5).abs() < 1e-3);
        }
        assert_eq!(flat.fishing.len(), FISHING_SPOTS);
    }

    #[test]
    fn walkers_route_around_the_lake_on_the_planet() {
        let layout = layout_on(&cache(lake_planet()), 7);
        let land = &layout.land;
        let nav = layout.nav.as_ref().expect("a planet layout has a path grid");
        assert!(nav.open_cells() > 1000);
        // The lake: the water nearest to the hut (12 bearings, 4-unit steps).
        let mut lake = None;
        'search: for r in (8..300).step_by(4) {
            for k in 0..24 {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                let p = Vec2::new(a.cos(), a.sin()) * r as f32;
                if !land.is_land(p) {
                    lake = Some(p);
                    break 'search;
                }
            }
        }
        let shore = lake.expect("the lake is near the hut");
        // Its centre: 40 further along the same bearing (radius 40).
        let centre = shore + shore.normalize() * 40.0;
        let side = Vec2::new(-shore.y, shore.x).normalize();
        let (from, to) = (centre + side * 60.0, centre - side * 60.0);
        assert!(land.is_land(from) && land.is_land(to));
        assert!(!land_path(land, from, to), "the straight line crosses the lake");
        // Walk it with the walkers' rule (waypoint + stay on land), 1 unit a step.
        let mut p = Vec3::new(from.x, 0.0, from.y);
        let goal = Vec3::new(to.x, 0.0, to.y);
        for _ in 0..3000 {
            if p.xz().distance(to) < 1.5 {
                break;
            }
            let aim = layout.waypoint(p, goal);
            let dir = (aim - p).xz().normalize_or_zero();
            let next = p + Vec3::new(dir.x, 0.0, dir.y) * 1.0_f32.min((aim - p).xz().length());
            p = layout.walk(p, next);
            assert!(land.is_land(p.xz()), "walker in the lake at {p}");
        }
        assert!(p.xz().distance(to) < 1.5, "arrived around the lake: {p} vs {to}");
        // Roads: at most two, each starting 15 from the hut.
        assert!(layout.roads.len() <= rules::MAX_NUM_TRIBE_ROADS);
        for road in &layout.roads {
            assert!((road[0].length() - rules::ROAD_START_DISTANCE_FROM_TRIBE).abs() < 0.5);
        }
    }

    #[test]
    fn swimmers_cross_the_lake_straight_while_animals_go_around() {
        let layout = lake_layout();
        let land = &layout.land;
        let nav_swim = layout.nav_swim.as_ref().expect("a planet layout has a swimmers' grid");
        let nav = layout.nav.as_ref().unwrap();
        // The lake is open water on the swimmers' grid, blocked on the land grid.
        assert!(nav_swim.open_cells() > nav.open_cells() + 40, "{} vs {}", nav_swim.open_cells(), nav.open_cells());
        // The same lake crossing as the walker test: the straight line is water.
        let mut lake = None;
        'search: for r in (8..300).step_by(4) {
            for k in 0..24 {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                let p = Vec2::new(a.cos(), a.sin()) * r as f32;
                if !land.is_land(p) {
                    lake = Some(p);
                    break 'search;
                }
            }
        }
        let shore = lake.expect("the lake is near the hut");
        let centre = shore + shore.normalize() * 40.0;
        let side = Vec2::new(-shore.y, shore.x).normalize();
        let (from, to) = (centre + side * 60.0, centre - side * 60.0);
        assert!(land.is_land(from) && land.is_land(to));
        assert!(!land_path(land, from, to), "the straight line crosses the lake");
        // Walk it with the swimmers' rule (straight, 1 unit a step).
        let goal = Vec3::new(to.x, 0.0, to.y);
        let mut p = Vec3::new(from.x, 0.0, from.y);
        let mut wet_steps = 0;
        let mut deviation = 0.0f32;
        for _ in 0..400 {
            if p.xz().distance(to) < 1.5 {
                break;
            }
            let aim = layout.swim_waypoint(p, goal);
            let dir = (aim - p).xz().normalize_or_zero();
            let next = p + Vec3::new(dir.x, 0.0, dir.y) * 1.0_f32.min((aim - p).xz().length());
            p = layout.swim_step(p, next);
            if layout.in_water(p.xz()) {
                wet_steps += 1;
            }
            // Distance from the straight line from -> to.
            let line = (to - from).normalize();
            let off = p.xz() - from;
            deviation = deviation.max((off - line * off.dot(line)).length());
        }
        assert!(p.xz().distance(to) < 1.5, "arrived: {p} vs {to}");
        assert!(wet_steps > 40, "swam {wet_steps} steps across the lake (radius 40)");
        assert!(deviation < 4.0, "walked straight: {deviation} off the line");
        // A step into the sea is taken by swimmers and refused to animals.
        let spot = layout.fishing[0];
        let out = spot.xz().normalize() * 6.0;
        let wet = spot + Vec3::new(out.x, 0.0, out.y);
        assert_eq!(layout.swim_step(spot, wet), wet);
        assert!(layout.land.is_land(layout.walk(spot, wet).xz()));
        // The chart edge still holds them.
        let edge = Vec3::new(CHART_LIMIT - 0.5, 0.0, 0.0);
        assert_eq!(layout.swim_step(edge, edge + Vec3::X * 2.0), edge);
        // No planet: nothing changes.
        let flat = flat_layout(3);
        assert_eq!(flat.swim_step(Vec3::ZERO, Vec3::splat(900.0)), Vec3::splat(900.0));
        assert_eq!(flat.swim_waypoint(Vec3::ZERO, Vec3::splat(9.0)), Vec3::splat(9.0));
        // The roads stay off the water: they use the land grid.
        for road in &layout.roads {
            for w in road.windows(2) {
                assert!(land_path(land, w[0], w[1]) || nav.line_clear(w[0], w[1]), "road corner pair over water {w:?}");
            }
        }
    }
}
