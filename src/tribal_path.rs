//! Path finding and roads on the Tribal planet (tribal-rules3; notes
//! `tribal-re-notes.md` T8D / T8E).
//!
//! Retail routes over the planet with a path-finding service (the tribe road
//! generator 0xB9CBF0 asks it, 0xB3D390 / 0xAC68A0, for a polyline between two
//! surface points; its grid is not decoded). OpenSpore keeps a navigation grid
//! over the Tribal chart (the hemisphere around the hut, `CHART_LIMIT`):
//! ADAPTED cells of [`CELL`] units, blocked where the cell centre is sea or
//! the ground is steeper than [`STEEP_DEGREES`], dearer over hills. Walkers
//! (`tribal_rts::step_toward`) go straight when the straight line is clear and
//! otherwise follow a cached distance field toward the goal, aiming at the
//! farthest cell of the descent they can see (string pulling), so members,
//! raiders, villagers and herds walk around lakes and bays instead of sliding
//! along the shore. Tribe members (and raiders) swim instead (tribal-swim, T9J:
//! retail `WalkTo` 0xC1C720 is a straight goal and a creature that enters water
//! swims, 0xC04E30 -> 0xC14F70): they use a second grid, [`NavGrid::build_swim`],
//! where water is open ground and only cliffs (ADAPTED, retail's cliff handling
//! is not traced) block, so their route is the straight line across lakes and
//! bays. The land grid stays the one the roads (a plant-free corridor between
//! tribes, never over water) and the herd animals (no swim clips) use.
//!
//! Roads (VERIFIED 0xB9CBF0): the generation list (`TribeGameLevelGenerationList`)
//! asks for up to `maxNumTribeRoads` 2 roads from the player's tribe to the
//! nearest NPC tribes whose centres are closer than `maxTribeRoadDistance`
//! 225 (tribes sorted by distance; the first one too far ends the list); each
//! runs from `roadStartDistanceFromTribe` 15 out of the player's tribe to 15
//! short of the other tribe along the path finder's route, `roadWidth` 13 wide.
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::creature_planet::Land;
use crate::planet_query::StagePlanet;
use crate::tribal_rules as rules;

/// ADAPTED: grid cell size (chart units).
pub const CELL: f32 = 8.0;
/// Cells from the hut to the edge of the chart.
pub const HALF_CELLS: i32 = (crate::tribal_planet::CHART_LIMIT / CELL) as i32;
/// Cells per side.
pub const SIDE: usize = (2 * HALF_CELLS + 1) as usize;
/// ADAPTED: ground steeper than this (degrees) is a cliff nobody walks.
pub const STEEP_DEGREES: f32 = 45.0;
/// ADAPTED: ground steeper than this costs [`HILL_COST`] per unit.
pub const HILL_DEGREES: f32 = 20.0;
pub const HILL_COST: u8 = 3;
/// Distance fields kept (one per goal cell).
const FIELD_CACHE: usize = 64;
/// Cells of the descent looked at for the farthest visible one.
const LOOKAHEAD: usize = 12;

/// The navigation grid of one Tribal layout.
pub struct NavGrid {
    /// Per cell: 0 = blocked, else the cost of crossing it (1 flat, 3 hills).
    cost: Vec<u8>,
    fields: Mutex<HashMap<usize, Arc<Vec<f32>>>>,
}

impl std::fmt::Debug for NavGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let open = self.cost.iter().filter(|c| **c > 0).count();
        f.debug_struct("NavGrid").field("open", &open).field("cells", &self.cost.len()).finish()
    }
}

#[derive(Copy, Clone, PartialEq)]
struct Node(f32, usize);
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(self.1.cmp(&other.1))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const NEIGHBOURS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

impl NavGrid {
    /// A grid from a per-cell cost function (0 = blocked).
    pub fn from_fn(cost: impl Fn(Vec2) -> u8) -> Self {
        let mut cells = vec![0u8; SIDE * SIDE];
        for (i, c) in cells.iter_mut().enumerate() {
            let p = Self::centre(i);
            if p.length() <= crate::tribal_planet::CHART_LIMIT {
                *c = cost(p);
            }
        }
        Self { cost: cells, fields: Mutex::new(HashMap::new()) }
    }

    /// The grid of a levelled stage planet: sea and cliffs blocked, hills
    /// dearer; the tribes' pads (`keep_open`, within the zoning radius) stay
    /// open wherever they are land.
    pub fn build(land: &Land, planet: &StagePlanet, keep_open: &[Vec2]) -> Self {
        Self::build_with(land, planet, keep_open, false)
    }

    /// The grid of the swimmers (tribe members, raiders): like [`Self::build`]
    /// but the sea is open ground at the cost of flat land, so a straight line
    /// across a lake is clear and only cliffs make them go around.
    pub fn build_swim(land: &Land, planet: &StagePlanet, keep_open: &[Vec2]) -> Self {
        Self::build_with(land, planet, keep_open, true)
    }

    fn build_with(land: &Land, planet: &StagePlanet, keep_open: &[Vec2], swim: bool) -> Self {
        let centre = planet.frame.centre;
        let sea = planet.planet.water_radius();
        let elevation = |p: Vec2| (land.world_point(p) - centre).length() - sea;
        let steep = STEEP_DEGREES.to_radians().tan();
        let hill = HILL_DEGREES.to_radians().tan();
        Self::from_fn(|p| {
            if !land.is_land(p) {
                // Swimmers: the water surface is level, nothing to climb.
                return u8::from(swim);
            }
            if keep_open.iter().any(|k| k.distance(p) <= rules::TRIBE_ZONING_RADIUS) {
                return 1;
            }
            let h = CELL * 0.5;
            let dx = (elevation(p + Vec2::X * h) - elevation(p - Vec2::X * h)) / CELL;
            let dz = (elevation(p + Vec2::Y * h) - elevation(p - Vec2::Y * h)) / CELL;
            let grade = (dx * dx + dz * dz).sqrt();
            if !grade.is_finite() || grade > steep {
                0
            } else if grade > hill {
                HILL_COST
            } else {
                1
            }
        })
    }

    fn centre(i: usize) -> Vec2 {
        let (x, z) = ((i % SIDE) as i32 - HALF_CELLS, (i / SIDE) as i32 - HALF_CELLS);
        Vec2::new(x as f32 * CELL, z as f32 * CELL)
    }

    fn coords(p: Vec2) -> Option<(i32, i32)> {
        if !p.is_finite() {
            return None;
        }
        let x = (p.x / CELL).round() as i32 + HALF_CELLS;
        let z = (p.y / CELL).round() as i32 + HALF_CELLS;
        let side = SIDE as i32;
        (0..side).contains(&x).then_some(())?;
        (0..side).contains(&z).then_some((x, z))
    }

    fn index(p: Vec2) -> Option<usize> {
        Self::coords(p).map(|(x, z)| z as usize * SIDE + x as usize)
    }

    fn cost_at(&self, i: usize) -> u8 {
        self.cost.get(i).copied().unwrap_or(0)
    }

    /// Whether chart point `p` lies in an open cell.
    pub fn open(&self, p: Vec2) -> bool {
        Self::index(p).is_some_and(|i| self.cost_at(i) > 0)
    }

    /// Open cells of the grid.
    pub fn open_cells(&self) -> usize {
        self.cost.iter().filter(|c| **c > 0).count()
    }

    /// Whether the straight walk from `a` to `b` crosses only open cells (the
    /// first and last stretch are not checked: a walker at the shore or a goal
    /// at the waterline may stand in a cell whose centre is sea).
    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        let length = a.distance(b);
        if length <= CELL {
            return true;
        }
        let step = CELL * 0.5;
        let samples = (length / step).ceil() as usize;
        (1..samples).all(|k| {
            let d = k as f32 * step;
            if d < CELL * 0.75 || length - d < CELL * 0.5 {
                return true;
            }
            self.open(a.lerp(b, d / length))
        })
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        let (x, z) = ((i % SIDE) as i32, (i / SIDE) as i32);
        NEIGHBOURS.iter().filter_map(move |(dx, dz)| {
            let (nx, nz) = (x + dx, z + dz);
            let side = SIDE as i32;
            if !(0..side).contains(&nx) || !(0..side).contains(&nz) {
                return None;
            }
            let n = nz as usize * SIDE + nx as usize;
            if self.cost_at(n) == 0 {
                return None;
            }
            if *dx != 0 && *dz != 0 {
                // No corner cutting past a blocked cell.
                let a = z as usize * SIDE + nx as usize;
                let b = nz as usize * SIDE + x as usize;
                if self.cost_at(a) == 0 || self.cost_at(b) == 0 {
                    return None;
                }
            }
            let length = if *dx != 0 && *dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            Some((n, length * CELL))
        })
    }

    /// The distance field toward `goal` (Dijkstra over the open cells),
    /// cached per goal cell.
    fn field(&self, goal: usize) -> Arc<Vec<f32>> {
        if let Some(f) = self.fields.lock().ok().and_then(|m| m.get(&goal).cloned()) {
            return f;
        }
        let mut dist = vec![f32::INFINITY; self.cost.len()];
        let mut heap = BinaryHeap::new();
        dist[goal] = 0.0;
        heap.push(Node(0.0, goal));
        while let Some(Node(d, i)) = heap.pop() {
            if d > dist[i] {
                continue;
            }
            let here = self.cost_at(i).max(1) as f32;
            for (n, length) in self.neighbours(i) {
                let next = d + length * 0.5 * (here + self.cost_at(n) as f32);
                if next < dist[n] {
                    dist[n] = next;
                    heap.push(Node(next, n));
                }
            }
        }
        let field = Arc::new(dist);
        if let Ok(mut map) = self.fields.lock() {
            if map.len() >= FIELD_CACHE {
                map.clear();
            }
            map.insert(goal, field.clone());
        }
        field
    }

    /// The open cell nearest to `p` (rings of growing radius).
    fn nearest_open(&self, p: Vec2) -> Option<usize> {
        let (x, z) = Self::coords(p.clamp_length_max(crate::tribal_planet::CHART_LIMIT))?;
        for r in 0..8i32 {
            let mut best: Option<(f32, usize)> = None;
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let (nx, nz) = (x + dx, z + dz);
                    let side = SIDE as i32;
                    if !(0..side).contains(&nx) || !(0..side).contains(&nz) {
                        continue;
                    }
                    let i = nz as usize * SIDE + nx as usize;
                    if self.cost_at(i) > 0 {
                        let d = Self::centre(i).distance(p);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, i));
                        }
                    }
                }
            }
            if let Some((_, i)) = best {
                return Some(i);
            }
        }
        None
    }

    /// The walker's cell on the field: its own when open and connected, else
    /// the best connected cell around it.
    fn start(&self, from: Vec2, field: &[f32]) -> Option<usize> {
        let (x, z) = Self::coords(from)?;
        let own = z as usize * SIDE + x as usize;
        if field[own].is_finite() {
            return Some(own);
        }
        let mut best: Option<(f32, usize)> = None;
        for dz in -2..=2i32 {
            for dx in -2..=2i32 {
                let (nx, nz) = (x + dx, z + dz);
                let side = SIDE as i32;
                if !(0..side).contains(&nx) || !(0..side).contains(&nz) {
                    continue;
                }
                let i = nz as usize * SIDE + nx as usize;
                let score = field[i] + Self::centre(i).distance(from);
                if field[i].is_finite() && best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, i));
                }
            }
        }
        best.map(|(_, i)| i)
    }

    fn descend(&self, i: usize, field: &[f32]) -> Option<usize> {
        self.neighbours(i)
            .filter(|(n, _)| field[*n] < field[i])
            .min_by(|a, b| field[a.0].total_cmp(&field[b.0]))
            .map(|(n, _)| n)
    }

    /// Where a walker at `from` heading for `to` should aim now: `to` itself
    /// when the way is clear (or no route is known), else the farthest cell of
    /// the route it can see.
    pub fn waypoint(&self, from: Vec2, to: Vec2) -> Vec2 {
        if self.line_clear(from, to) {
            return to;
        }
        let Some(goal) = self.nearest_open(to) else {
            return to;
        };
        let field = self.field(goal);
        let Some(start) = self.start(from, &field) else {
            return to;
        };
        let mut aim = Self::centre(start);
        let mut cur = start;
        for _ in 0..LOOKAHEAD {
            if cur == goal {
                if self.line_clear(from, to) {
                    return to;
                }
                break;
            }
            let Some(next) = self.descend(cur, &field) else {
                break;
            };
            cur = next;
            let c = Self::centre(cur);
            if self.line_clear(from, c) {
                aim = c;
            } else {
                break;
            }
        }
        aim
    }

    /// The whole route from `from` to `to` as a polyline (string-pulled), or
    /// `None` when they are not connected.
    pub fn path(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        let goal = self.nearest_open(to)?;
        let field = self.field(goal);
        let mut cur = self.start(from, &field)?;
        let mut cells = vec![from, Self::centre(cur)];
        for _ in 0..SIDE * SIDE {
            if cur == goal {
                break;
            }
            cur = self.descend(cur, &field)?;
            cells.push(Self::centre(cur));
        }
        cells.push(to);
        // String pulling: keep only the corners.
        let mut out = vec![cells[0]];
        let mut anchor = 0;
        let mut k = 1;
        while k < cells.len() {
            if k + 1 < cells.len() && self.line_clear(cells[anchor], cells[k + 1]) {
                k += 1;
                continue;
            }
            out.push(cells[k]);
            anchor = k;
            k += 1;
        }
        Some(out)
    }
}

/// Length of a polyline.
#[allow(dead_code)] // tests and diagnostics
pub fn polyline_length(points: &[Vec2]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The tribe roads (VERIFIED rule 0xB9CBF0, see the module notes): up to
/// `MAX_NUM_TRIBE_ROADS` routes from 15 out of the hut (chart origin) to 15
/// short of the nearest villages closer than `MAX_TRIBE_ROAD_DISTANCE`.
pub fn tribe_roads(nav: &NavGrid, land: &Land, villages: &[Vec3]) -> Vec<Vec<Vec2>> {
    let mut near: Vec<(f32, Vec2)> = villages.iter().map(|v| (land.chord(Vec2::ZERO, v.xz()), v.xz())).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut roads = Vec::new();
    for (distance, v) in near.into_iter().take(rules::MAX_NUM_TRIBE_ROADS) {
        if !(distance < rules::MAX_TRIBE_ROAD_DISTANCE) {
            break;
        }
        let dir = v.normalize_or(Vec2::X);
        let a = dir * rules::ROAD_START_DISTANCE_FROM_TRIBE;
        let b = v - dir * rules::ROAD_START_DISTANCE_FROM_TRIBE;
        if let Some(path) = nav.path(a, b) {
            roads.push(path);
        }
    }
    roads
}

// ---------------------------------------------------------------------------
// Presentation: the village ground and the roads
// ---------------------------------------------------------------------------

/// The village ground decal of a tribe at hut level `level` (1..=3): the
/// `TribeTuning` preload textures `decal_ground_paths`, `_l2`, `_l3` (RW4
/// rasters in Spore_Graphics: dirt paths between the huts around a fire pit,
/// more worn and walled by level).
pub fn village_decal_name(level: usize) -> &'static str {
    match level {
        0 | 1 => "decal_ground_paths",
        2 => "decal_ground_paths_l2",
        _ => "decal_ground_paths_l3",
    }
}

/// ADAPTED: width of the village ground decal (its retail size is not traced;
/// it covers OpenSpore's compressed village: hut, food mat, totem, shacks).
pub const VILLAGE_DECAL_WIDTH: f32 = 26.0;
/// ADAPTED: road colour (the dirt of the path decals) and lift over the ground.
pub const ROAD_COLOUR: [f32; 3] = [0.56, 0.47, 0.38];
const ROAD_LIFT: f32 = 0.12;
const ROAD_SAMPLE: f32 = 4.0;

/// A village ground decal (`village` = `None` for the player's village).
#[derive(Component, Debug, Clone, Copy)]
pub struct VillageGround {
    pub village: Option<usize>,
    pub level: usize,
}

/// One road ribbon.
#[derive(Component, Debug, Clone, Copy)]
pub struct TribeRoad;

fn decal_material(
    originals: &crate::Originals,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    level: usize,
) -> Option<Handle<StandardMaterial>> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let key = crate::dbpf::ResourceKey {
        group_id: 0,
        instance_id: crate::world_objects::name_id(village_decal_name(level)),
        type_id: crate::asset_catalog::RW4,
    };
    let bytes = originals.resource_index.read(key).ok()?;
    let mips = crate::texture::decode_rw4_mips(&bytes).filter(|m| !m.is_empty())?;
    let base = &mips[0];
    let image = Image::new(
        Extent3d { width: base.width, height: base.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        base.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    Some(materials.add(StandardMaterial {
        base_color_texture: Some(images.add(image)),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        depth_bias: 8.0,
        ..default()
    }))
}

/// Spawn the ground decal of a village centred on chart `centre` (grounded
/// on the tribe's levelled pad, turned to the local up).
#[allow(clippy::too_many_arguments)]
pub fn spawn_village_ground(
    commands: &mut Commands,
    originals: &crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    centre: Vec3,
    level: usize,
    village: Option<usize>,
) {
    let Some(material) = decal_material(originals, images, materials, level) else {
        return;
    };
    let half = VILLAGE_DECAL_WIDTH * 0.5;
    commands.spawn((
        crate::stages::StageOwned,
        Name::new("village ground"),
        VillageGround { village, level },
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(half)))),
        MeshMaterial3d(material),
        Transform::from_xyz(centre.x, 0.0, centre.z),
        Visibility::Inherited,
        bevy::light::NotShadowCaster,
        crate::terrain::Grounded::new(0.04),
    ));
}

/// The ribbon mesh of one road (chart polyline) over the stage planet, or
/// flat at y 0.4 without one: `ROAD_WIDTH` wide, opaque in the middle,
/// fading at the edges.
pub fn road_mesh(points: &[Vec2], land: &Land, planet: Option<&StagePlanet>) -> Option<Mesh> {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    if points.len() < 2 {
        return None;
    }
    // Resample every ROAD_SAMPLE units.
    let mut samples = vec![points[0]];
    for w in points.windows(2) {
        let n = (w[0].distance(w[1]) / ROAD_SAMPLE).ceil().max(1.0) as usize;
        for k in 1..=n {
            samples.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
    }
    let half = rules::ROAD_WIDTH * 0.5;
    let up_at = |q: Vec3| planet.map_or(Vec3::Y, |sp| sp.frame.up(q));
    let [r, g, b] = ROAD_COLOUR;
    let mut positions = Vec::new();
    let mut colours = Vec::new();
    let mut normals = Vec::new();
    for (i, p) in samples.iter().enumerate() {
        let a = samples[i.saturating_sub(1)];
        let c = samples[(i + 1).min(samples.len() - 1)];
        let along = (c - a).normalize_or(Vec2::X);
        let side = Vec2::new(-along.y, along.x);
        // The ends fade in and out.
        let end = if i == 0 || i + 1 == samples.len() { 0.0 } else { 1.0 };
        for (offset, alpha) in [(-half, 0.0), (-half * 0.55, 0.6), (half * 0.55, 0.6), (half, 0.0)] {
            let at = land.world_point(*p + side * offset);
            let up = up_at(at);
            positions.push((at + up * ROAD_LIFT).to_array());
            colours.push([r, g, b, alpha * end]);
            normals.push(up.to_array());
        }
    }
    let mut indices = Vec::new();
    for i in 0..samples.len() as u32 - 1 {
        for k in 0..3u32 {
            let (a, b) = (i * 4 + k, (i + 1) * 4 + k);
            // Counter-clockwise seen from above (front faces up).
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// Retail draws no road (T9G): 0xCDDB40 paints each road's centre line into
/// the game plant manager's 6 x 128 x 128 map (0xBBD8A0), so a road is a
/// corridor without plants. The ADAPTED dirt ribbon stays available with
/// `OPENSPORE_TRIBE_ROAD_RIBBON=1`.
pub fn road_ribbon_enabled() -> bool {
    std::env::var("OPENSPORE_TRIBE_ROAD_RIBBON").is_ok_and(|v| v == "1")
}

/// INFERRED width of the plant-free corridor: one plant-map cell (128 per
/// cube face edge, ~7.8 units on the R = 500 planet at the hut), as a
/// clearing radius around points every `ROAD_CLEARING_STEP` units.
pub const ROAD_CLEARING_RADIUS: f32 = 4.0;
pub const ROAD_CLEARING_STEP: f32 = 2.0;

/// The flora clearings of the roads: stage points (x, 0, z) along each road
/// and their radius (`planet_flora` keeps plants out of them).
pub fn road_clearings(roads: &[Vec<Vec2>]) -> Vec<(Vec3, f32)> {
    let mut out = Vec::new();
    for road in roads {
        let Some(first) = road.first() else {
            continue;
        };
        out.push((Vec3::new(first.x, 0.0, first.y), ROAD_CLEARING_RADIUS));
        for w in road.windows(2) {
            let n = (w[0].distance(w[1]) / ROAD_CLEARING_STEP).ceil().max(1.0) as usize;
            for k in 1..=n {
                let p = w[0].lerp(w[1], k as f32 / n as f32);
                out.push((Vec3::new(p.x, 0.0, p.y), ROAD_CLEARING_RADIUS));
            }
        }
    }
    out
}

/// Spawn the layout's road ribbons (ADAPTED, opt-in: see
/// [`road_ribbon_enabled`]).
pub fn spawn_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &crate::tribal_planet::TribalLayout,
) {
    if layout.roads.is_empty() || !road_ribbon_enabled() {
        return;
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        depth_bias: 6.0,
        ..default()
    });
    for road in &layout.roads {
        if let Some(mesh) = road_mesh(road, &layout.land, layout.planet.as_deref()) {
            commands.spawn((
                crate::stages::StageOwned,
                Name::new("tribe road"),
                TribeRoad,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                Visibility::Inherited,
                bevy::light::NotShadowCaster,
            ));
        }
    }
}

/// The player's village ground follows the tribe level (1 / 3 tribes allied
/// or conquered: `_l2`, `_l3`).
pub fn village_ground_follow_level(
    run: Res<crate::game::RunProgress>,
    originals: Res<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut grounds: Query<(&mut VillageGround, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let level = run.tribal_campaign.home_hut_level();
    for (mut ground, mut material) in &mut grounds {
        if ground.village.is_some() || ground.level == level {
            continue;
        }
        if let Some(next) = decal_material(&originals, &mut images, &mut materials, level) {
            material.0 = next;
        }
        ground.level = level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open ground with a round lake of radius 40 centred 100 units out on +X.
    fn lake_grid() -> NavGrid {
        NavGrid::from_fn(|p| if p.distance(Vec2::new(100.0, 0.0)) <= 40.0 { 0 } else { 1 })
    }

    #[test]
    fn a_clear_walk_goes_straight() {
        let nav = lake_grid();
        let to = Vec2::new(0.0, 150.0);
        assert_eq!(nav.waypoint(Vec2::ZERO, to), to);
        assert!(nav.line_clear(Vec2::ZERO, to));
        assert!(!nav.line_clear(Vec2::ZERO, Vec2::new(200.0, 0.0)));
    }

    #[test]
    fn walkers_go_around_the_lake_and_never_into_it() {
        let nav = lake_grid();
        let lake = Vec2::new(100.0, 0.0);
        let (from, to) = (Vec2::new(30.0, 0.0), Vec2::new(170.0, 0.0));
        // Walk with the waypoint rule in 1-unit steps.
        let mut p = from;
        let mut walked = 0.0;
        for _ in 0..2000 {
            if p.distance(to) < 1.0 {
                break;
            }
            let aim = nav.waypoint(p, to);
            assert!(aim.distance(lake) > 40.0 - CELL, "aim {aim} inside the lake");
            let step = (aim - p).normalize_or_zero() * 1.0_f32.min(aim.distance(p));
            p += step;
            walked += step.length();
            assert!(p.distance(lake) > 40.0 - CELL * 0.75, "walker at {p} in the lake");
        }
        assert!(p.distance(to) < 1.0, "arrived: {p}");
        // Around the lake: longer than the straight 140, shorter than a detour
        // over the far side of the planet.
        assert!((150.0..260.0).contains(&walked), "walked {walked}");
        let path = nav.path(from, to).expect("connected");
        assert!(path.len() >= 3, "a corner or two: {path:?}");
        assert!((150.0..260.0).contains(&polyline_length(&path)));
    }

    #[test]
    fn a_walled_off_goal_falls_back_to_the_direct_aim() {
        // A ring of cliffs around the goal: no route, the aim is the goal.
        let nav = NavGrid::from_fn(|p| {
            let d = p.distance(Vec2::new(0.0, 200.0));
            if (20.0..40.0).contains(&d) { 0 } else { 1 }
        });
        let to = Vec2::new(0.0, 200.0);
        assert_eq!(nav.waypoint(Vec2::ZERO, to), to);
        assert!(nav.path(Vec2::ZERO, to).is_none());
    }

    #[test]
    fn roads_join_the_nearest_tribes_within_225() {
        let nav = lake_grid();
        let land = Land::flat();
        let villages = [
            Vec3::new(0.0, 0.0, 200.0),
            Vec3::new(220.0, 0.0, 0.0),
            Vec3::new(-150.0, 0.0, -150.0),
            Vec3::new(-300.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -400.0),
        ];
        let roads = tribe_roads(&nav, &land, &villages);
        assert_eq!(roads.len(), rules::MAX_NUM_TRIBE_ROADS);
        // The nearest two: 200 (north) and 212 (south-west); 220 is the third.
        let ends: Vec<Vec2> = roads.iter().map(|r| *r.last().unwrap()).collect();
        assert!(ends[0].distance(Vec2::new(0.0, 185.0)) < 0.01, "{ends:?}");
        assert!(ends[1].distance(Vec2::new(-150.0, -150.0) * (1.0 - 15.0 / 212.13)) < 0.1);
        for road in &roads {
            assert!(road[0].length() > 14.9 && road[0].length() < 15.1);
        }
        // A tribe beyond 225 gets no road.
        let far = [Vec3::new(0.0, 0.0, 300.0)];
        assert!(tribe_roads(&nav, &land, &far).is_empty());
    }

    #[test]
    fn roads_clear_a_plant_free_corridor_and_draw_no_ribbon_by_default() {
        let road = vec![Vec2::new(0.0, 15.0), Vec2::new(0.0, 25.0), Vec2::new(10.0, 25.0)];
        let clear = road_clearings(&[road]);
        // Every point of the centre line is within a clearing.
        for k in 0..=40 {
            let t = k as f32 / 40.0;
            let p = if t < 0.5 { Vec2::new(0.0, 15.0 + 20.0 * t) } else { Vec2::new(20.0 * (t - 0.5), 25.0) };
            assert!(clear.iter().any(|(c, r)| Vec2::new(c.x, c.z).distance(p) <= *r), "{p}");
        }
        assert!(clear.iter().all(|(_, r)| *r == ROAD_CLEARING_RADIUS));
        // A plant 6 units off the road keeps its place.
        assert!(!clear.iter().any(|(c, r)| Vec2::new(c.x, c.z).distance(Vec2::new(6.0, 20.0)) < *r));
        if std::env::var("OPENSPORE_TRIBE_ROAD_RIBBON").is_err() {
            assert!(!road_ribbon_enabled());
        }
    }
}
