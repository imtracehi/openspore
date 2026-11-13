//! Planet flora: the theme's nine plants placed on the stage planet the way the
//! retail terrain places them (helper `planet-flora`, notes
//! `notes\planet-flora-notes.md`). Creature, Tribal and Civilization.
//!
//! Original data / code (VERIFIED, ModAPI march2017 addresses):
//! - Every planet theme lists the flora effect 0x0A7DC62F in
//!   `terrainScriptAmbientEffectSelect1` 0x040CF84E. In the planet effdir
//!   (0x40450200!0x5F848353) it is effect 219 -> effect 218 = terrainDistribute
//!   #17 (cDistGrid level 2 = 4 x 4 cells per cube face, distance 3000, facing
//!   0.65, type All) -> effect 215, and #18 (level 2, 5000) -> effect 217 (one
//!   message per grove for the game plants, same seed as the visual groves).
//! - cDistGrid (ctor 0xF66080, mask fill 0xF65A40, traversal 0xF66790, cell
//!   spawn 0xF66510 / 0xF64240): a 5-level quadtree per face (1, 4, 16, 64, 256
//!   cells). A cell is active while the camera is within the level distance of
//!   the cell's surface point and `0.5 - 0.5 * dot(cellDir, toCamera)` is at
//!   most the level facing. Its effect gets the cell transform (offset = cell
//!   direction x radius, scale 1 / 2^level), the cell index as random seed, and
//!   the override table of the flora slots present in the cell (cell mask =
//!   the planet record's 4 x 4 per face plant grid, 0xB2A250): override set
//!   10 x (class + 1) + 30 k, +0 modelMeshLOD1, +1 modelMeshLOD2, +2 modelSprite0
//!   (side), +3 modelSprite1 (top), +4 modelMeshLOD3 (key order .rdata 0x148F19C;
//!   modelMeshLOD0 is never used on the planet).
//! - Effect 215 (lodDistances 0 / 350 / 4000): near the cell, distribute 512 =
//!   3 groves (effect 214 at size 6, square source 0.6); beyond 350 only the
//!   large and medium sprite distributes (514 .. 520, same seeds).
//! - Effect 214 (lodDistances 0 / 55 / 70 / 130 / 200 / 4000): large plants
//!   density 8 in a square of 4 (LOD1 < 55, LOD2 < 70, LOD3 < 200, sprites
//!   beyond), medium density 6 on a ring of 6, small density 12 on a ring of 7
//!   (LOD1 < 55, LOD2 < 70, LOD3 < 130, sprites beyond); sizes 0.8 / 1.0 / 1.2
//!   (large) and 0.9 / 1.0 / 1.1; heading curve 0..1 (any direction); emit map
//!   Flora 0x02C9C764 within (0.1, 1].
//! - Flora map sampler 0xF70670: ground above cTerrainMapSet water level (+0x3C)
//!   + beach level (+0x44, 0.025), and the cGamePlantManager map 0x107AE878
//!   (128^2 per face, value >= 0.5, wrapper 0xFF34A0) non-zero.
//! - Flora definitions (.flr 0x40666200): modelMeshLOD1..3 (0x40666201..3),
//!   modelSprite0 / 1 (0x40662801 side 2 x 2 view atlas / 0x40662800 top),
//!   floraImposterScale (sprite half size), obstacleCollisionHeight.
//!
//! ADAPTED / INFERRED (see the notes):
//! - Swarm distribute (VERIFIED, cDistributeEffect 0xA8C7B0 / generator
//!   0xA8D3B0 / clusters 0xA8DE00): positions are Halton(2, 3) points (index
//!   start + i + 1) mapped by the source type (square 2x - 1, circle / ring by
//!   the concentric disk map) and scaled by sourceScale x the effect scale;
//!   instance sizes / headings sample their curves at (i + 0.5) / density.
//!   The cell's distribute 512 runs in cluster mode with numClusters 3, so the
//!   3 groves sit at the same cell-relative points in every cell, and every
//!   grove has the same layout (the seeds are per component, not per cell).
//! - INFERRED: the grove effect's scale is its instance size 6 (large square
//!   half width 24, rings 36 / 42; the grove message of #18 is size 26); the
//!   cluster index starts one past the seed; yaw curves in turns.
//! - ADAPTED: a plant's species is a hash among the present slots of its size
//!   class (Swarm's override-set choice is not decoded); rings are a band
//!   0.75..1 of the radius (ring parameter not read).
//! - ADAPTED: the game plant map is "everywhere on land" except clearings
//!   around nests, villages, cities and geysers (its writer is the plant
//!   manager's noun population, not decoded).
//! - ADAPTED: sprites are crossed quads (views 0 and 1 of the side atlas) plus
//!   a horizontal top quad; retail draws camera-facing particles.
//! - ADAPTED: a near plant between the camera and the player fades (retail has
//!   an `obstacleAlphaModelID` alpha model).
//!
//! On by default; `OPENSPORE_PLANET_FLORA=0` turns it off (the old stand-in
//! scatter returns). Landing 065 had made it opt-in because the default
//! Creature start view was bare; round 2 decoded the Swarm distribute (the
//! density is retail: 3 groves per level-2 cell) and adds the ADAPTED start
//! grove (`add_start_grove`).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::dbpf::ResourceKey;
use crate::planet_query::{face_point, tangent_basis, Planet, StagePlanet};
use crate::stages::GameStage;

/// terrainDistribute #17: level 2 cells per face edge (2^2).
pub const CELLS_PER_EDGE: usize = 4;
/// terrainDistribute #17 level 2 distance and facing.
pub const CELL_DISTANCE: f32 = 3000.0;
pub const CELL_FACING: f32 = 0.65;
/// Effect 215 lodDistances [0, 350, 4000]: full groves near the cell.
pub const NEAR_CELL_DISTANCE: f32 = 350.0;
/// Distribute 512: density 3, square source 0.6, size 6.
pub const GROVES_PER_CELL: usize = 3;
pub const GROVE_SOURCE: f32 = 0.6;
pub const GROVE_SIZE: f32 = 6.0;
/// Distribute 512 `start` and numClusters.
pub const GROVE_START: u32 = 1705;
pub const GROVE_CLUSTERS: u32 = 3;
/// Effect 214 lodDistances [0, 55, 70, 130, 200, 4000].
pub const GROVE_LODS: [f32; 5] = [55.0, 70.0, 130.0, 200.0, 4000.0];
/// cTerrainMapSet mBeachLevel (0xF929F0): the Flora map starts above it.
pub const BEACH_LEVEL: f32 = 0.025;
/// ADAPTED: inner edge of a ring source (fraction of its radius).
pub const RING_INNER: f32 = 0.75;

/// One size class of effect 214 (large / medium / small).
#[derive(Clone, Copy, Debug)]
pub struct ClassDef {
    /// Distribute density (instances per grove).
    pub density: usize,
    /// Ring source (sourceType 2), else square (0).
    pub ring: bool,
    /// sourceScale.
    pub scale: f32,
    /// size curve.
    pub sizes: &'static [f32],
    /// `start`: first Halton index of the distribute.
    pub start: u32,
}

/// Distributes 494 (large), 501 (medium), 507 (small) of effect 214.
pub const CLASSES: [ClassDef; 3] = [
    ClassDef { density: 8, ring: false, scale: 4.0, sizes: &[0.8, 1.0, 1.2], start: 96 },
    ClassDef { density: 6, ring: true, scale: 6.0, sizes: &[0.9, 1.0, 1.1], start: 419 },
    ClassDef { density: 12, ring: true, scale: 7.0, sizes: &[0.9, 1.0, 1.1], start: 7736 },
];

/// The flora definition groups (`.flr` -> LOD meshes and sprites).
pub const FLORA_DEFINITION_GROUP: u32 = 0x40666200;
pub const PROP_MESH_LOD1: u32 = 0x00F9EFBC;
pub const PROP_MESH_LOD2: u32 = 0x00F9EFBD;
pub const PROP_MESH_LOD3: u32 = 0x00F9EFBE;
pub const PROP_SPRITE_SIDE: u32 = 0xD124FCE4;
pub const PROP_SPRITE_TOP: u32 = 0xD124FCE7;
pub const PROP_IMPOSTER_SCALE: u32 = 0x04057881;
pub const PROP_COLLISION_HEIGHT: u32 = 0x0254CF97;
pub const PROP_CANOPY_RADIUS: u32 = 0x0254CF8F;

/// Planet flora on (default) unless `OPENSPORE_PLANET_FLORA=0`.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_PLANET_FLORA").map_or(true, |v| v != "0"))
}

/// Whether the stage scatter (`baked_model::scenery_where`) leaves its flora
/// to this module: the stages spread it wider than the island (`spread` > 1)
/// only on the planet.
pub fn replaces_scatter(spread: f32) -> bool {
    enabled() && spread > 1.0
}

/// One plant: planet-local surface point, heading, size and flora slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plant {
    pub pos: Vec3,
    pub yaw: f32,
    pub scale: f32,
    pub slot: u8,
}

impl Plant {
    pub fn class(&self) -> usize {
        self.slot as usize / 3
    }
    /// Planet-local rotation: model up (+Y after the GMDL basis) along the
    /// radial up (terrainDistribute verticalWeight 1), then the heading.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_arc(Vec3::Y, self.pos.normalize_or(Vec3::Y)) * Quat::from_rotation_y(self.yaw)
    }
}

/// A grove (one effect 214 instance).
#[derive(Clone, Debug)]
pub struct Grove {
    pub centre: Vec3,
    pub cell: usize,
    pub plants: Vec<Plant>,
}

/// A cDistGrid level-2 cell: unit direction and surface point.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub dir: Vec3,
    pub point: Vec3,
}

#[derive(Clone, Debug, Default)]
pub struct FloraPlan {
    pub cells: Vec<Cell>,
    pub groves: Vec<Grove>,
}

impl FloraPlan {
    pub fn plant_count(&self) -> usize {
        self.groves.iter().map(|g| g.plants.len()).sum()
    }
}

/// Piecewise-linear Swarm curve at `t` in [0, 1].
pub fn curve(values: &[f32], t: f32) -> f32 {
    match values.len() {
        0 => 1.0,
        1 => values[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            values[i] + (values[i + 1] - values[i]) * (x - i as f32)
        }
    }
}

/// The Flora effect map (0xF70670) on the planet's height field, with the
/// ADAPTED plant map (clearings are tested by the caller).
pub fn flora_map(planet: &Planet, dir: Vec3) -> bool {
    planet.h_at(dir) > planet.water_level + BEACH_LEVEL
}

/// Every grove and plant of the planet (planet-local), deterministic in
/// `seed`. `present` = flora slots that exist (the theme's nine); `clear` =
/// planet-local surface points and radii with no flora.
pub fn plan(planet: &Planet, seed: u64, present: [bool; 9], clear: &[(Vec3, f32)]) -> FloraPlan {
    let mut out = FloraPlan::default();
    let n = CELLS_PER_EDGE;
    // Param 0x11 (0xF64240): the source box of a cell is centred on the cell
    // with half extents sourceScale x 2 / n in face coordinates (SetParam
    // 0xA923B0); the cube point goes onto the sphere (0xA88360) and is pinned
    // to the terrain surface.
    let half_box = GROVE_SOURCE * 2.0 / n as f32;
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let (uc, vc) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
                let dir = face_point(face, uc, vc).normalize();
                let cell_index = (n * n * face + n * y + x) as u64;
                out.cells.push(Cell { dir, point: dir * planet.height(dir) });
                let cell = out.cells.len() - 1;
                for gi in 0..GROVES_PER_CELL {
                    // Distribute 512 in cluster mode (0xA8DE00): instance i sits
                    // at the cluster centre Halton((start + i) % numClusters +
                    // clustersStart); clustersFactor 0.00577 keeps it there.
                    let (hx, hy) = halton((GROVE_START + gi as u32) % GROVE_CLUSTERS + 1);
                    // Face coordinates are 2 x uv - 1.
                    let u = uc + (hx * 2.0 - 1.0) * half_box * 0.5;
                    let v = vc + (hy * 2.0 - 1.0) * half_box * 0.5;
                    let gdir = face_point(face, u, v).normalize();
                    // The grove instance's heading: yaw curve [0, 1] at
                    // (i + 0.5) / density (index-sampled curves, 0xA88760).
                    let grove_yaw = curve(&[0.0, 1.0], (gi as f32 + 0.5) / GROVES_PER_CELL as f32)
                        * std::f32::consts::TAU;
                    let grove = make_grove(planet, gdir, grove_yaw, cell, cell_index * 8 + gi as u64, seed, present, clear);
                    out.groves.push(grove);
                }
            }
        }
    }
    out
}

/// One grove (effect 214 instance) centred on the surface under `gdir`.
#[allow(clippy::too_many_arguments)]
fn make_grove(
    planet: &Planet,
    gdir: Vec3,
    grove_yaw: f32,
    cell: usize,
    key: u64,
    seed: u64,
    present: [bool; 9],
    clear: &[(Vec3, f32)],
) -> Grove {
    let centre = gdir * planet.height(gdir).max(planet.water_radius());
    let (t0, b0) = tangent_basis(gdir);
    let (t, b) = (
        t0 * grove_yaw.cos() + b0 * grove_yaw.sin(),
        b0 * grove_yaw.cos() - t0 * grove_yaw.sin(),
    );
    let mut plants = Vec::new();
    for (class, def) in CLASSES.iter().enumerate() {
        let slots: Vec<u8> =
            (0..3).map(|k| (class * 3 + k) as u8).filter(|s| present[*s as usize]).collect();
        if slots.is_empty() {
            continue;
        }
        let extent = def.scale * GROVE_SIZE;
        for i in 0..def.density {
            // 0xA8D3B0: Halton(2, 3) point start + i + 1, mapped by
            // the source type, x sourceScale x the grove's size.
            let (hx, hy) = halton(def.start + i as u32 + 1);
            let (sx, sy) = if def.ring {
                let (dx, dy) = concentric_disk(hx, hy);
                let r = (dx * dx + dy * dy).sqrt();
                // ADAPTED ring band (the ring parameter, description
                // +0x5C, is not read from the effdir).
                let ring = (r * (1.0 - RING_INNER) + RING_INNER) / r.max(1e-6);
                (dx * ring, dy * ring)
            } else {
                (hx * 2.0 - 1.0, hy * 2.0 - 1.0)
            };
            let offset = (t * sx + b * sy) * extent;
            let pdir = (gdir * planet.radius + offset).normalize();
            if !flora_map(planet, pdir) {
                continue;
            }
            let pos = pdir * planet.height(pdir);
            if clear.iter().any(|(c, radius)| pos.distance(*c) < *radius) {
                continue;
            }
            let t_index = (i as f32 + 0.5) / def.density as f32;
            // ADAPTED: the species of a class (Swarm's choice among
            // the override sets 10 / 40 / 70 is not decoded).
            let pick = hash3(seed, key, (class * 16 + i) as u64);
            plants.push(Plant {
                pos,
                yaw: grove_yaw + curve(&[0.0, 1.0], t_index) * std::f32::consts::TAU,
                scale: curve(def.sizes, t_index),
                slot: slots[(pick % slots.len() as u64) as usize],
            });
        }
    }
    Grove { centre, cell, plants }
}

/// ADAPTED: the start site's grove. Retail's groves stand at the same three
/// cell-relative points in every cell (VERIFIED, see `plan`); the retail
/// start site (nest / hut, `planet-re` 4F-3) is not traced, and OpenSpore's
/// start site can fall between groves. When fewer than `START_VIEW_PLANTS`
/// plants stand within `view` of the start (the stage camera's opening view),
/// one grove of the same composition is added `view` away, on the side that
/// keeps most plants, so the opening shows plants as the retail Creature /
/// Tribal openings do.
pub const START_VIEW_PLANTS: usize = 6;
/// Opening view radius around the start: Creature (camera 15 behind the
/// nest), Tribal (the top-down opening frame over the hut is ~50 x 36 units).
pub const START_VIEW_CREATURE: f32 = 45.0;
pub const START_VIEW_TRIBAL: f32 = 20.0;

pub fn add_start_grove(
    plan: &mut FloraPlan,
    planet: &Planet,
    start: Vec3,
    view: f32,
    seed: u64,
    present: [bool; 9],
    clear: &[(Vec3, f32)],
) {
    let in_view = plan.groves.iter().flat_map(|g| &g.plants).filter(|p| p.pos.distance(start) < view).count();
    if in_view >= START_VIEW_PLANTS || plan.cells.is_empty() {
        return;
    }
    let up = start.normalize_or(Vec3::Y);
    let cell = (0..plan.cells.len())
        .max_by(|a, b| plan.cells[*a].dir.dot(up).total_cmp(&plan.cells[*b].dir.dot(up)))
        .unwrap_or(0);
    let (t, b) = tangent_basis(up);
    let best = (0..12)
        .map(|k| {
            let angle = k as f32 / 12.0 * std::f32::consts::TAU;
            let gdir = (up * planet.radius + (t * angle.cos() + b * angle.sin()) * view).normalize();
            make_grove(planet, gdir, angle, cell, 0xFFFF_0000 + k as u64, seed, present, clear)
        })
        .max_by_key(|g| g.plants.iter().filter(|p| p.pos.distance(start) < view).count());
    if let Some(grove) = best.filter(|g| g.plants.len() >= START_VIEW_PLANTS) {
        plan.groves.push(grove);
    }
}

/// Swarm's quasi-random point (0xA7C330 / 0xA7C2B0): the Halton sequence,
/// x = base-2 radical inverse (0xA7C260 / 0xA7C1E0), y = base 3.
pub fn halton(index: u32) -> (f32, f32) {
    let radical = |mut k: u32, base: u32| {
        let (mut f, mut r) = (1.0 / base as f32, 0.0f32);
        while k > 0 {
            r += f * (k % base) as f32;
            k /= base;
            f /= base as f32;
        }
        r
    };
    (radical(index, 2), radical(index, 3))
}

/// Swarm's square -> disk mapping of the circle / ring sources (0xA7C590 /
/// 0xA883F0): concentric (Shirley-Chiu) mapping of (2x - 1, 2y - 1).
pub fn concentric_disk(x: f32, y: f32) -> (f32, f32) {
    let (a, b) = (x * 2.0 - 1.0, y * 2.0 - 1.0);
    let q = std::f32::consts::FRAC_PI_4;
    let (r, phi) = if a > -b {
        if a > b {
            (a, q * (b / a))
        } else {
            (b, q * (2.0 - a / b))
        }
    } else if a < b {
        (-a, q * (4.0 + b / a))
    } else if b != 0.0 {
        (-b, q * (6.0 - a / b))
    } else {
        (0.0, 0.0)
    };
    (r * phi.cos(), r * phi.sin())
}

fn hash3(a: u64, b: u64, c: u64) -> u64 {
    let mut h = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ c.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    h
}

/// What a grove shows for the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// The cell is inactive (beyond the distance or facing away).
    Hidden,
    /// Beyond effect 215's first LOD: large and medium sprites only.
    Far,
    /// Full grove, effect 214 LOD band 0..=4.
    Grove(u8),
}

/// What one plant draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Draw {
    Nothing,
    /// 0 = modelMeshLOD1, 1 = LOD2, 2 = LOD3.
    Mesh(usize),
    Sprite,
}

/// Effect 214 LOD band of a camera distance.
pub fn band(distance: f32) -> u8 {
    GROVE_LODS.iter().position(|d| distance < *d).unwrap_or(GROVE_LODS.len() - 1) as u8
}

/// cDistGrid level-2 activity and effect 215 / 214 LOD for a grove.
pub fn show_for(camera: Vec3, cell: &Cell, centre: Vec3) -> Show {
    let to_camera = camera - cell.point;
    let d = to_camera.length();
    let facing = 0.5 - 0.5 * cell.dir.dot(to_camera / (d + 1e-6));
    if d > CELL_DISTANCE || facing > CELL_FACING {
        return Show::Hidden;
    }
    if d >= NEAR_CELL_DISTANCE {
        return Show::Far;
    }
    Show::Grove(band(camera.distance(centre)))
}

/// Effect 214's per-class LOD blocks: large (1,2) LOD1 / (2,3) LOD2 / (3,5)
/// LOD3 / (5,6) sprites; medium and small (1,2) / (2,3) / (3,4) / (4,6).
/// Beyond the near cell effect 215 keeps the large sprites (514 / 518, every
/// game mode) and the medium sprites (516 / 520) only for the app flags 2048
/// and 0x100000 (`far_medium`; VERIFIED by the ModAPI `AppFlags` enum: bit 0xB `kAppFlagCivGameMode` = 0x800 =
/// Civilization, bit 0x14 `kAppFlagSpaceGamePlanetMode` = 0x100000, 0x100 Creature, 0x200 Tribe, 0x20000
/// `kAppFlagSpaceGameOutOfUFO`).
pub fn draw_for(class: usize, show: Show, far_medium: bool) -> Draw {
    match show {
        Show::Hidden => Draw::Nothing,
        Show::Far => {
            if class == 0 || (class == 1 && far_medium) {
                Draw::Sprite
            } else {
                Draw::Nothing
            }
        }
        Show::Grove(0) => Draw::Mesh(0),
        Show::Grove(1) => Draw::Mesh(1),
        Show::Grove(2) => Draw::Mesh(2),
        Show::Grove(3) if class == 0 => Draw::Mesh(2),
        Show::Grove(_) => Draw::Sprite,
    }
}

/// One flora slot's installed assets.
#[derive(Clone)]
struct Species {
    /// modelMeshLOD1..3.
    lods: [Option<crate::OriginalModel>; 3],
    side: Option<Handle<StandardMaterial>>,
    top: Option<Handle<StandardMaterial>>,
    /// floraImposterScale: sprite half size.
    imposter: f32,
    /// obstacleCollisionHeight / obstacleCanopyCollisionRadius (cutaway).
    height: f32,
    canopy: f32,
}

/// Live planet flora of the stage.
#[derive(Resource)]
pub struct PlanetFlora {
    render_root: Entity,
    root: Entity,
    pub plan: FloraPlan,
    species: Vec<Option<Species>>,
    cross: Handle<Mesh>,
    top: Handle<Mesh>,
    groves: Vec<GroveLive>,
    last_camera: Option<Vec3>,
    faded: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    clear_signature: u64,
    clear_timer: f32,
    stage: GameStage,
    /// Planet-local start site (Creature home / Tribal hut) and its opening
    /// view radius, for the ADAPTED start grove.
    start: Option<(Vec3, f32)>,
}

#[derive(Default)]
struct GroveLive {
    show: Option<Show>,
    /// Per plant: its entities (mesh surfaces or sprite quads) and whether
    /// they are faded.
    parts: Vec<(usize, Vec<(Entity, Handle<StandardMaterial>)>, bool)>,
}

/// Root of the planet flora (child of the planet's land root, so it turns
/// with the floating origin and leaves with the stage).
#[derive(Component)]
pub struct PlanetFloraRoot;

/// A plant entity of the planet flora.
#[derive(Component)]
pub struct PlanetFloraPart;

/// Unit cross of two vertical quads (x in -1..1, y in 0..2), carrying views 0
/// and 1 of the 2 x 2 side atlas.
fn cross_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for (plane, u0) in [(0, 0.0f32), (1, 0.5f32)] {
        let base = positions.len() as u32;
        for (x, y, u, v) in [(-1.0, 0.0, u0, 0.5), (1.0, 0.0, u0 + 0.5, 0.5), (1.0, 2.0, u0 + 0.5, 0.0), (-1.0, 2.0, u0, 0.0)] {
            if plane == 0 {
                positions.push([x, y, 0.0f32]);
                normals.push([0.0, 0.0, 1.0f32]);
            } else {
                positions.push([0.0, y, x]);
                normals.push([1.0, 0.0, 0.0f32]);
            }
            uvs.push([u, v]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// ADAPTED height of the top sprite (fraction of the side quad's 2 x scale):
/// about the canopy top in the side views (alpha rows 48..128 of 128).
const TOP_SPRITE_HEIGHT: f32 = 1.1;

/// Unit horizontal quad (2 x 2) at the top sprite height.
fn top_mesh() -> Mesh {
    let h = TOP_SPRITE_HEIGHT;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1.0, h, -1.0], [1.0, h, -1.0], [1.0, h, 1.0], [-1.0, h, 1.0f32]],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0f32]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0f32]]);
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    mesh
}

fn sprite_image(mips: Vec<crate::texture::Pixels>) -> Option<Image> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let base = mips.first()?;
    let mut image = Image::new(
        Extent3d { width: base.width, height: base.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        base.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|m| m.rgba).collect());
    Some(image)
}

/// The stage planet's theme flora slots (terrainThemeFlora through the GPT
/// twin theme, `planet_sky::theme_flora`).
fn theme_slots(world: &World) -> Vec<Option<ResourceKey>> {
    let Some(script) = world.get_resource::<crate::terrain::PlanetCache>().map(|c| c.script.clone()) else {
        return Vec::new();
    };
    let Some(originals) = world.get_resource::<crate::Originals>() else {
        return Vec::new();
    };
    let index = &originals.resource_index;
    let descriptor = if script.first_group {
        crate::planet_data::load_descriptor(index, script.terrain_script)
    } else {
        crate::planet_data::load_descriptor_key(index, script.key)
    };
    descriptor.map(|d| crate::planet_sky::theme_flora(index, &d)).unwrap_or_default()
}

/// Load one slot's LOD meshes, sprites and sizes from its `.flr` definition.
fn load_species(world: &mut World, lod0: ResourceKey) -> Option<Species> {
    let definition = ResourceKey {
        group_id: FLORA_DEFINITION_GROUP,
        instance_id: lod0.instance_id,
        type_id: crate::asset_catalog::PROP,
    };
    let props = {
        let originals = world.resource::<crate::Originals>();
        crate::prop::parse(&originals.resource_index.read(definition).ok()?).ok()?
    };
    let key = |id: u32| props.0.get(&id).and_then(|p| p.key());
    let number = |id: u32| props.0.get(&id).and_then(|p| p.number());
    let mesh_keys = [PROP_MESH_LOD1, PROP_MESH_LOD2, PROP_MESH_LOD3].map(|id| {
        key(id).map(|k| ResourceKey { type_id: crate::decode::TYPE_GMDL, ..k })
    });
    let sprite_keys = [PROP_SPRITE_SIDE, PROP_SPRITE_TOP].map(|id| {
        key(id).map(|k| ResourceKey { type_id: crate::decode::TYPE_RASTER, ..k })
    });
    let mut lods: [Option<crate::OriginalModel>; 3] = [None, None, None];
    let mut sprites: [Option<Handle<StandardMaterial>>; 2] = [None, None];
    world.resource_scope(|world, mut originals: Mut<crate::Originals>| {
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                for (slot, k) in mesh_keys.iter().enumerate() {
                    let Some(k) = *k else { continue };
                    match crate::baked_model::ensure(&mut originals, k, &mut meshes, &mut images, &mut materials) {
                        Ok(m) => lods[slot] = Some(m),
                        Err(e) => warn!("planet flora {:08X} LOD{}: {e}", k.instance_id, slot + 1),
                    }
                }
                for (slot, k) in sprite_keys.iter().enumerate() {
                    let Some(k) = *k else { continue };
                    let Ok(mips) = crate::baked_model::texture_mips(&originals.resource_index, k) else {
                        warn!("planet flora {:08X}: sprite {} missing", k.instance_id, slot);
                        continue;
                    };
                    let Some(image) = sprite_image(mips) else { continue };
                    let image = images.add(image);
                    sprites[slot] = Some(materials.add(StandardMaterial {
                        base_color_texture: Some(image),
                        alpha_mode: AlphaMode::Mask(0.5),
                        cull_mode: None,
                        double_sided: true,
                        perceptual_roughness: 0.9,
                        reflectance: 0.05,
                        ..default()
                    }));
                }
            });
        });
    });
    let [side, top] = sprites;
    Some(Species {
        lods,
        side,
        top,
        imposter: number(PROP_IMPOSTER_SCALE).filter(|v| *v > 0.0).unwrap_or(4.0),
        height: number(PROP_COLLISION_HEIGHT).filter(|v| *v > 0.0).unwrap_or(4.0),
        canopy: number(PROP_CANOPY_RADIUS).filter(|v| *v > 0.0).unwrap_or(1.0),
    })
}

/// Where no flora grows (ADAPTED plant map): planet-local surface points and
/// radii of the stage's nests, villages, cities and geysers.
fn clearings(world: &World, stage: &GameStage, sp: &StagePlanet) -> Vec<(Vec3, f32)> {
    let Some(run) = world.get_resource::<crate::game::RunProgress>() else {
        return Vec::new();
    };
    // A stored stage position: on the chart's hemisphere put it on the
    // surface along the vertical line, else it is already a 3D point.
    let local = |p: Vec3| {
        let world_point = if p.y >= sp.frame.centre.y {
            Vec3::new(p.x, sp.surface_y(p.x, p.z), p.z)
        } else {
            p
        };
        let l = sp.frame.to_local(world_point);
        l.normalize_or(Vec3::Y) * sp.planet.height(l)
    };
    let mut out = Vec::new();
    match stage {
        GameStage::Creature => {
            out.push((local(Vec3::from_array(run.creature_lifecycle.home)), 8.0));
            out.extend(run.species_world.nests.iter().map(|n| (local(Vec3::from_array(n.position)), 6.0)));
        }
        GameStage::Tribal => {
            // The player's hut stands at the chart origin (the world +Y pole,
            // `tribal_planet`); `layout.origin` is the stage origin 40-60 away.
            let layout = crate::tribal_planet::current();
            out.push((local(Vec3::ZERO), 14.0));
            out.extend(layout.villages.iter().map(|v| (local(*v), 12.0)));
            // Tribe roads are plant-free corridors (VERIFIED 0xBBD8A0, T9G).
            out.extend(crate::tribal_path::road_clearings(&layout.roads).into_iter().map(|(p, r)| (local(p), r)));
        }
        GameStage::Civilization => {
            let campaign = &run.civ_campaign;
            out.extend(campaign.cities.iter().map(|c| (local(c.center()), 12.0 * crate::civ_planet::CITY_SCALE)));
            out.extend(
                (0..crate::civ::GEYSER_COUNT).map(|g| (local(campaign.geyser_at(g)), 4.0 * crate::civ_planet::CITY_SCALE)),
            );
        }
        _ => {}
    }
    out
}

fn signature(clear: &[(Vec3, f32)]) -> u64 {
    clear.iter().fold(clear.len() as u64, |h, (p, r)| {
        let q = (p.x.round() as i64 as u64)
            ^ ((p.y.round() as i64 as u64) << 21)
            ^ ((p.z.round() as i64 as u64) << 42)
            ^ ((*r as u64) << 7);
        h.rotate_left(13) ^ q.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    })
}

/// Build the flora of the stage planet under `render_root`.
fn build_flora(world: &mut World, render_root: Entity, sp: Arc<StagePlanet>, stage: GameStage) -> Option<PlanetFlora> {
    let started = std::time::Instant::now();
    let slots = theme_slots(world);
    if slots.iter().all(Option::is_none) {
        info!("planet flora: no theme flora installed");
        return None;
    }
    let seed = world.get_resource::<crate::terrain::PlanetCache>().map_or(0, |c| c.seed);
    let mut species: Vec<Option<Species>> = Vec::with_capacity(9);
    for slot in 0..9 {
        species.push(slots.get(slot).copied().flatten().and_then(|k| load_species(world, k)));
    }
    let present: [bool; 9] = std::array::from_fn(|s| species[s].is_some());
    let clear = clearings(world, &stage, &sp);
    let mut plan = plan(&sp.planet, seed, present, &clear);
    let view = match stage {
        GameStage::Creature => START_VIEW_CREATURE,
        _ => START_VIEW_TRIBAL,
    };
    let start = matches!(stage, GameStage::Creature | GameStage::Tribal)
        .then(|| clear.first().map(|c| (c.0, view)))
        .flatten();
    if let Some((start, view)) = start {
        add_start_grove(&mut plan, &sp.planet, start, view, seed, present, &clear);
    }
    let uncleared = self::plan(&sp.planet, seed, present, &[]).plant_count();
    let origin = sp.frame.dir_to_local(Vec3::Y) * sp.planet.radius;
    let mut near: Vec<(f32, usize)> =
        plan.groves.iter().filter(|g| !g.plants.is_empty()).map(|g| (g.centre.distance(origin), g.plants.len())).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(5);
    info!("planet flora: nearest groves to the stage origin (distance, plants): {near:.0?}");
    let (cross, top) = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        (meshes.add(cross_mesh()), meshes.add(top_mesh()))
    };
    let root = world
        .spawn((
            PlanetFloraRoot,
            Name::new("planet flora (terrainDistribute groves)"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(render_root),
        ))
        .id();
    info!(
        "planet flora: {} groves, {} plants, {} of 9 slots, {} clearings ({} plants cleared) ({:.0} ms)",
        plan.groves.len(),
        plan.plant_count(),
        present.iter().filter(|p| **p).count(),
        clear.len(),
        uncleared.saturating_sub(plan.plant_count()),
        started.elapsed().as_secs_f32() * 1000.0
    );
    let groves = (0..plan.groves.len()).map(|_| GroveLive::default()).collect();
    Some(PlanetFlora {
        render_root,
        root,
        plan,
        species,
        cross,
        top,
        groves,
        last_camera: None,
        faded: HashMap::new(),
        clear_signature: signature(&clear),
        clear_timer: 0.0,
        stage,
        start,
    })
}

/// Despawn and respawn one grove's entities for `show`.
static RESPAWNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn respawn_grove(world: &mut World, flora: &mut PlanetFlora, gi: usize, show: Show) {
    RESPAWNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for (_, entities, _) in std::mem::take(&mut flora.groves[gi].parts) {
        for (e, _) in entities {
            world.despawn(e);
        }
    }
    flora.groves[gi].show = Some(show);
    let mut parts = Vec::new();
    let grove = &flora.plan.groves[gi];
    for (pi, plant) in grove.plants.iter().enumerate() {
        let Some(species) = flora.species.get(plant.slot as usize).and_then(|s| s.as_ref()) else {
            continue;
        };
        let rotation = plant.rotation();
        let mut entities = Vec::new();
        match draw_for(plant.class(), show, flora.stage == GameStage::Civilization) {
            Draw::Nothing => {}
            Draw::Mesh(level) => {
                // Nearest installed LOD when one is missing.
                let model = (level..3).chain((0..level).rev()).find_map(|l| species.lods[l].as_ref());
                let Some(model) = model else { continue };
                for surface in &model.surfaces {
                    let mut e = world.spawn((
                        PlanetFloraPart,
                        Mesh3d(surface.handle.clone()),
                        MeshMaterial3d(surface.material.clone()),
                        Transform { translation: plant.pos, rotation, scale: Vec3::splat(plant.scale) },
                        ChildOf(flora.root),
                    ));
                    // ADAPTED: only LOD1 / LOD2 cast shadows.
                    if level == 2 {
                        e.insert(bevy::light::NotShadowCaster);
                    }
                    entities.push((e.id(), surface.material.clone()));
                }
            }
            Draw::Sprite => {
                let transform = Transform {
                    translation: plant.pos,
                    rotation,
                    scale: Vec3::splat(species.imposter * plant.scale),
                };
                for (mesh, material) in [(&flora.cross, &species.side), (&flora.top, &species.top)] {
                    let Some(material) = material else { continue };
                    let e = world
                        .spawn((
                            PlanetFloraPart,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            transform,
                            bevy::light::NotShadowCaster,
                            ChildOf(flora.root),
                        ))
                        .id();
                    entities.push((e, material.clone()));
                }
            }
        }
        if !entities.is_empty() {
            parts.push((pi, entities, false));
        }
    }
    flora.groves[gi].parts = parts;
}

/// ADAPTED cutaway: plants of the nearest band between the camera and the
/// player take a faded copy of their material (one copy per source material).
fn cutaway(world: &mut World, flora: &mut PlanetFlora, camera: Vec3, target: Option<Vec3>) {
    for gi in 0..flora.groves.len() {
        if flora.groves[gi].show != Some(Show::Grove(0)) {
            continue;
        }
        for part in 0..flora.groves[gi].parts.len() {
            let (pi, _, faded) = &flora.groves[gi].parts[part];
            let plant = flora.plan.groves[gi].plants[*pi];
            let Some(species) = flora.species[plant.slot as usize].as_ref() else { continue };
            let up = plant.pos.normalize_or(Vec3::Y);
            let centre = plant.pos + up * species.height * 0.5 * plant.scale;
            let radius = (species.canopy.min(species.height * 0.5) * plant.scale).max(1.0);
            let blocked = target.is_some_and(|t| segment_hits_sphere(camera, t, centre, radius));
            if blocked == *faded {
                continue;
            }
            let entities = flora.groves[gi].parts[part].1.clone();
            for (e, original) in entities {
                let material = if blocked {
                    faded_material(world, &mut flora.faded, &original)
                } else {
                    original
                };
                if let Ok(mut ent) = world.get_entity_mut(e) {
                    ent.insert(MeshMaterial3d(material));
                }
            }
            flora.groves[gi].parts[part].2 = blocked;
        }
    }
}

fn faded_material(
    world: &mut World,
    cache: &mut HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    original: &Handle<StandardMaterial>,
) -> Handle<StandardMaterial> {
    if let Some(h) = cache.get(&original.id()) {
        return h.clone();
    }
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    let Some(mut m) = materials.get(original).cloned() else {
        return original.clone();
    };
    m.base_color = m.base_color.with_alpha(0.25);
    m.alpha_mode = AlphaMode::Blend;
    let h = materials.add(m);
    cache.insert(original.id(), h.clone());
    h
}

/// Whether the segment a..b (excluding its last 5 %) passes within `r` of `c`.
pub fn segment_hits_sphere(a: Vec3, b: Vec3, c: Vec3, r: f32) -> bool {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-8 {
        return false;
    }
    let t = ((c - a).dot(d) / len2).clamp(0.0, 0.95);
    (a + d * t).distance_squared(c) < r * r
}

/// Per frame (PostUpdate, after the planet renderer): build the flora when a
/// stage planet appears, then update each grove's LOD when the camera moved.
pub fn update(world: &mut World) {
    if !enabled() {
        return;
    }
    let started = std::time::Instant::now();
    let Some((render_root, sp)) = world
        .get_resource::<crate::planet_render::PlanetRender>()
        .map(|r| (r.root, r.stage.clone()))
    else {
        world.remove_resource::<PlanetFlora>();
        return;
    };
    let Some(stage) = world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) else {
        return;
    };
    if !matches!(stage, GameStage::Creature | GameStage::Tribal | GameStage::Civilization) {
        return;
    }
    let stale = world
        .get_resource::<PlanetFlora>()
        .is_none_or(|f| f.render_root != render_root || f.stage != stage || world.get_entity(f.root).is_err());
    if stale {
        if let Some(old) = world.remove_resource::<PlanetFlora>() {
            if let Ok(e) = world.get_entity_mut(old.root) {
                e.despawn();
            }
        }
        match build_flora(world, render_root, sp.clone(), stage.clone()) {
            Some(f) => world.insert_resource(f),
            None => {
                // Remember the failure so the load is not retried every frame.
                let root = world.spawn((PlanetFloraRoot, Transform::IDENTITY, Visibility::Hidden, ChildOf(render_root))).id();
                world.insert_resource(PlanetFlora {
                    render_root,
                    root,
                    plan: FloraPlan::default(),
                    species: Vec::new(),
                    cross: Handle::default(),
                    top: Handle::default(),
                    groves: Vec::new(),
                    last_camera: None,
                    faded: HashMap::new(),
                    clear_signature: 0,
                    clear_timer: 0.0,
                    stage: stage.clone(),
                    start: None,
                });
            }
        }
    }
    capture_view(world, &sp);
    // Root entities: Transform is the world placement (this runs before the
    // transform propagation).
    let Some(camera) = world
        .query_filtered::<&Transform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
    else {
        return;
    };
    let target = world
        .query_filtered::<&Transform, (With<crate::game::Player>, Without<ChildOf>)>()
        .iter(world)
        .next()
        .map(|t| t.translation);
    let dt = world.resource::<Time>().delta_secs();
    world.resource_scope(|world, mut flora: Mut<PlanetFlora>| {
        if flora.plan.groves.is_empty() {
            return;
        }
        // The stage's own pads change (new cities, moved nests): re-plan.
        flora.clear_timer += dt;
        if flora.clear_timer > 2.0 {
            flora.clear_timer = 0.0;
            let clear = clearings(world, &stage, &sp);
            let signature = signature(&clear);
            if signature != flora.clear_signature {
                flora.clear_signature = signature;
                let present: [bool; 9] = std::array::from_fn(|s| flora.species.get(s).is_some_and(|x| x.is_some()));
                let seed = world.get_resource::<crate::terrain::PlanetCache>().map_or(0, |c| c.seed);
                flora.plan = plan(&sp.planet, seed, present, &clear);
                if let Some((start, view)) = flora.start {
                    add_start_grove(&mut flora.plan, &sp.planet, start, view, seed, present, &clear);
                }
                // Groves may have been added or removed (the start grove).
                let keep = flora.plan.groves.len();
                for gone in flora.groves.iter_mut().skip(keep) {
                    for (_, entities, _) in gone.parts.drain(..) {
                        for (e, _) in entities {
                            world.despawn(e);
                        }
                    }
                }
                flora.groves.resize_with(keep, GroveLive::default);
                for g in flora.groves.iter_mut() {
                    g.show = None;
                }
                flora.last_camera = None;
            }
        }
        let local_camera = sp.frame.to_local(camera);
        let moved = flora.last_camera.is_none_or(|c| c.distance_squared(local_camera) > 0.25);
        if moved {
            flora.last_camera = Some(local_camera);
            for gi in 0..flora.plan.groves.len() {
                let grove = &flora.plan.groves[gi];
                let show = show_for(local_camera, &flora.plan.cells[grove.cell], grove.centre);
                if flora.groves[gi].show != Some(show) {
                    respawn_grove(world, &mut flora, gi, show);
                }
            }
        }
        let local_target = target.map(|t| sp.frame.to_local(t + sp.frame.up(t) * 0.3));
        cutaway(world, &mut flora, local_camera, local_target);
        stats(&flora, local_camera, started.elapsed().as_secs_f32() * 1000.0);
    });
}

/// `OPENSPORE_FLORA_STATS=1`: grove states and drawn entities every 120 frames.
fn stats(flora: &PlanetFlora, camera: Vec3, ms: f32) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("OPENSPORE_FLORA_STATS").is_some()) {
        return;
    }
    static FRAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static WORST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static SUM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let us = (ms * 1000.0) as u32;
    WORST.fetch_max(us, std::sync::atomic::Ordering::Relaxed);
    SUM.fetch_add(us, std::sync::atomic::Ordering::Relaxed);
    if FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 120 != 0 {
        return;
    }
    let worst = WORST.swap(0, std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
    let mean = SUM.swap(0, std::sync::atomic::Ordering::Relaxed) as f32 / 120_000.0;
    let respawns = RESPAWNS.swap(0, std::sync::atomic::Ordering::Relaxed);
    let faded = flora.groves.iter().flat_map(|g| &g.parts).filter(|p| p.2).count();
    let mut states = [0usize; 7];
    let mut entities = 0;
    for g in &flora.groves {
        let i = match g.show {
            None | Some(Show::Hidden) => 0,
            Some(Show::Far) => 1,
            Some(Show::Grove(b)) => 2 + b as usize,
        };
        states[i] += 1;
        entities += g.parts.iter().map(|p| p.1.len()).sum::<usize>();
    }
    let nearest = flora
        .plan
        .groves
        .iter()
        .filter(|g| !g.plants.is_empty())
        .map(|g| g.centre.distance(camera))
        .fold(f32::INFINITY, f32::min);
    eprintln!(
        "flora stats: hidden {} far {} bands {:?}, {} entities, nearest grove {:.0}; update mean {:.3} max {:.3} ms, {} respawns, {} faded",
        states[0],
        states[1],
        &states[2..],
        entities,
        nearest,
        mean,
        worst,
        respawns,
        faded
    );
}

/// `OPENSPORE_CAPTURE_VIEW=grove[,height,back]` (captures): put the game camera
/// `back` units from the grove with the most plants near the stage origin,
/// `height` above the ground, looking at it.
fn capture_view(world: &mut World, sp: &StagePlanet) {
    let Ok(spec) = std::env::var("OPENSPORE_CAPTURE_VIEW") else {
        return;
    };
    let mut parts = spec.split(',');
    if parts.next() != Some("grove") {
        return;
    }
    let height: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let back: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(45.0);
    let Some(flora) = world.get_resource::<PlanetFlora>() else {
        return;
    };
    let origin = sp.frame.dir_to_local(Vec3::Y) * sp.planet.radius;
    let Some(grove) = flora
        .plan
        .groves
        .iter()
        .filter(|g| g.plants.len() >= 18 && g.centre.distance(origin) < 400.0)
        .min_by(|a, b| a.centre.distance(origin).total_cmp(&b.centre.distance(origin)))
    else {
        return;
    };
    let up = grove.centre.normalize();
    let (t, _) = tangent_basis(up);
    let eye_dir = (grove.centre + t * back).normalize();
    let eye_local = eye_dir * (sp.planet.height(eye_dir).max(sp.planet.water_radius()) + height);
    let target_local = grove.centre + up * 6.0;
    let eye = sp.frame.to_world(eye_local);
    let target = sp.frame.to_world(target_local);
    let eye_up = sp.frame.up(eye);
    let cameras: Vec<Entity> = world.query_filtered::<Entity, With<crate::GameCamera>>().iter(world).collect();
    for c in cameras {
        if let Some(mut tr) = world.get_mut::<Transform>(c) {
            *tr = Transform::from_translation(eye).looking_at(target, eye_up);
        }
    }
}

/// Registered by `planet_render::build`.
pub fn build(app: &mut App) {
    app.add_systems(
        PostUpdate,
        update
            .after(crate::planet_render::update)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, PLANET_RADIUS};

    /// Half land (x > 0 raised), half sea.
    fn half_land() -> Planet {
        let mut cube = HeightCube::filled(64, 0.0);
        for face in 0..6 {
            for row in 0..64 {
                for col in 0..64 {
                    let d = texel_dir(face, row, col, 64);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = h_to_raw(if d.x > 0.0 { 0.2 } else { -0.3 });
                }
            }
        }
        Planet::new(cube, PLANET_RADIUS, 0.0)
    }

    #[test]
    fn plan_has_three_groves_per_level_two_cell_and_is_deterministic() {
        let planet = half_land();
        let a = plan(&planet, 7, [true; 9], &[]);
        let b = plan(&planet, 7, [true; 9], &[]);
        assert_eq!(a.cells.len(), 6 * 16);
        assert_eq!(a.groves.len(), 6 * 16 * GROVES_PER_CELL);
        assert_eq!(a.plant_count(), b.plant_count());
        let land = a.groves.iter().position(|g| g.plants.len() > 10).expect("a grove on land");
        assert_eq!(a.groves[land].plants, b.groves[land].plants);
        let c = plan(&planet, 8, [true; 9], &[]);
        assert_ne!(a.groves[land].plants, c.groves[land].plants);
    }

    #[test]
    fn plants_grow_only_above_the_beach_and_outside_clearings() {
        let planet = half_land();
        let p = plan(&planet, 3, [true; 9], &[]);
        assert!(p.plant_count() > 1000, "{}", p.plant_count());
        for g in &p.groves {
            for plant in &g.plants {
                assert!(planet.h_at(plant.pos) > BEACH_LEVEL);
                // Land is x > 0 (bilinear coast within a texel).
                assert!(plant.pos.x > -20.0, "{:?}", plant.pos);
                assert!((plant.pos.length() - planet.height(plant.pos)).abs() < 1e-3);
            }
        }
        let centre = p.groves.iter().find(|g| g.plants.len() > 10).unwrap().centre;
        let cleared = plan(&planet, 3, [true; 9], &[(centre, 60.0)]);
        assert!(cleared.plant_count() < p.plant_count());
        assert!(cleared.groves.iter().flat_map(|g| &g.plants).all(|pl| pl.pos.distance(centre) >= 60.0));
    }

    #[test]
    fn grove_classes_follow_effect_214_sources() {
        let planet = Planet::flat(0.3);
        let p = plan(&planet, 11, [true; 9], &[]);
        for g in &p.groves {
            let mut counts = [0usize; 3];
            for plant in &g.plants {
                let class = plant.class();
                counts[class] += 1;
                let d = (plant.pos.normalize() * planet.radius).distance(g.centre.normalize() * planet.radius);
                let extent = CLASSES[class].scale * GROVE_SIZE;
                if CLASSES[class].ring {
                    assert!(d > extent * RING_INNER * 0.97 && d < extent * 1.03, "ring {class}: {d}");
                } else {
                    assert!(d < extent * std::f32::consts::SQRT_2 * 1.03, "square: {d}");
                }
                let def = CLASSES[class];
                assert!(plant.scale >= def.sizes[0] - 1e-4 && plant.scale <= def.sizes[def.sizes.len() - 1] + 1e-4);
            }
            assert_eq!(counts, [8, 6, 12]);
        }
        // Only present slots are drawn; positions do not depend on presence.
        let some = plan(&planet, 11, [true, false, false, true, true, true, false, false, true], &[]);
        assert!(some.groves.iter().flat_map(|g| &g.plants).all(|pl| [0u8, 3, 4, 5, 8].contains(&pl.slot)));
        assert_eq!(some.groves[5].plants[0].pos, p.groves[5].plants[0].pos);
    }

    #[test]
    fn cells_follow_the_distance_and_facing_rule() {
        let planet = Planet::flat(0.1);
        let p = plan(&planet, 1, [true; 9], &[]);
        let cell = p.cells[0];
        let grove = p.groves[0].centre;
        // Camera just above the grove: full grove, nearest band.
        let up = grove.normalize();
        assert_eq!(show_for(grove + up * 10.0, &cell, grove), Show::Grove(0));
        // On the far side of the planet the cell faces away.
        assert_eq!(show_for(-cell.point * 1.05, &cell, grove), Show::Hidden);
        // High above the cell (Civ): active but beyond effect 215's near LOD.
        assert_eq!(show_for(cell.dir * (planet.radius + 400.0), &cell, grove), Show::Far);
        assert_eq!(band(10.0), 0);
        assert_eq!(band(60.0), 1);
        assert_eq!(band(100.0), 2);
        assert_eq!(band(150.0), 3);
        assert_eq!(band(250.0), 4);
    }

    #[test]
    fn effect_214_lod_blocks_per_class() {
        assert_eq!(draw_for(0, Show::Grove(0), false), Draw::Mesh(0));
        assert_eq!(draw_for(2, Show::Grove(1), false), Draw::Mesh(1));
        assert_eq!(draw_for(1, Show::Grove(2), false), Draw::Mesh(2));
        assert_eq!(draw_for(0, Show::Grove(3), false), Draw::Mesh(2));
        assert_eq!(draw_for(1, Show::Grove(3), false), Draw::Sprite);
        assert_eq!(draw_for(0, Show::Grove(4), false), Draw::Sprite);
        assert_eq!(draw_for(0, Show::Far, false), Draw::Sprite);
        assert_eq!(draw_for(1, Show::Far, false), Draw::Nothing);
        assert_eq!(draw_for(1, Show::Far, true), Draw::Sprite);
        assert_eq!(draw_for(2, Show::Far, true), Draw::Nothing);
        assert_eq!(draw_for(1, Show::Hidden, true), Draw::Nothing);
    }

    #[test]
    fn swarm_halton_and_disk_mapping() {
        let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        assert!(close(halton(1), (0.5, 1.0 / 3.0)));
        assert!(close(halton(2), (0.25, 2.0 / 3.0)));
        assert!(close(halton(5), (0.625, 7.0 / 9.0)));
        for k in 1..200 {
            let (x, y) = halton(k);
            let (dx, dy) = concentric_disk(x, y);
            assert!(dx * dx + dy * dy <= 1.0 + 1e-5);
        }
        let (cx, cy) = concentric_disk(1.0, 0.5);
        assert!((cx - 1.0).abs() < 1e-6 && cy.abs() < 1e-6);
    }

    #[test]
    fn groves_share_one_cell_relative_layout() {
        // Cluster mode with numClusters 3: the same three points in every cell,
        // and every grove the same plants (component seeds).
        let planet = Planet::flat(0.3);
        let p = plan(&planet, 2, [true; 9], &[]);
        let half_cell = 0.5 / CELLS_PER_EDGE as f32;
        for g in &p.groves {
            let cell = p.cells[g.cell];
            let (fa, ua, va) = crate::planet_query::face_uv(g.centre);
            let (fb, ub, vb) = crate::planet_query::face_uv(cell.dir);
            assert_eq!(fa, fb);
            assert!((ua - ub).abs() <= GROVE_SOURCE * 2.0 * half_cell + 1e-4);
            assert!((va - vb).abs() <= GROVE_SOURCE * 2.0 * half_cell + 1e-4);
        }
        let sizes = |g: &Grove| g.plants.iter().map(|pl| pl.scale).collect::<Vec<_>>();
        assert_eq!(sizes(&p.groves[0]), sizes(&p.groves[100]));
        // Sizes sample the curve at (i + 0.5) / density.
        assert!((p.groves[0].plants[0].scale - curve(CLASSES[0].sizes, 0.5 / 8.0)).abs() < 1e-6);
    }

    #[test]
    fn start_grove_only_when_none_is_near() {
        let planet = Planet::flat(0.3);
        let full = plan(&planet, 4, [true; 9], &[]);
        // No plants in the opening view: one grove is added `view` away.
        let mut empty = FloraPlan { cells: full.cells.clone(), groves: Vec::new() };
        let start = full.cells[5].point;
        let view = START_VIEW_CREATURE;
        add_start_grove(&mut empty, &planet, start, view, 4, [true; 9], &[(start, 8.0)]);
        assert_eq!(empty.groves.len(), 1);
        let g = &empty.groves[0];
        // `view` along the tangent at radius R, carried up to the ground radius.
        let expected = view * start.length() / planet.radius;
        assert!((g.centre.distance(start) - expected).abs() < 1.0, "{}", g.centre.distance(start));
        assert!(g.plants.iter().all(|pl| pl.pos.distance(start) >= 8.0));
        assert!(g.plants.iter().filter(|pl| pl.pos.distance(start) < view).count() >= START_VIEW_PLANTS);
        // Plants already in view: nothing is added.
        let mut again = empty.clone();
        add_start_grove(&mut again, &planet, start, view, 4, [true; 9], &[(start, 8.0)]);
        assert_eq!(again.groves.len(), 1);
    }

    #[test]
    fn curves_and_cutaway_segment() {
        assert!((curve(&[0.8, 1.0, 1.2], 0.25) - 0.9).abs() < 1e-5);
        assert_eq!(curve(&[6.0], 0.7), 6.0);
        assert!(segment_hits_sphere(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(5.0, 0.5, 0.0), 1.0));
        assert!(!segment_hits_sphere(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(5.0, 2.0, 0.0), 1.0));
        // The followed body itself (segment end) never counts.
        assert!(!segment_hits_sphere(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), 0.3));
    }

    /// The constants above against the installed planet effdir (the
    /// components decoded with SporeModder-FX's DistributeEffect /
    /// TerrainDistributeEffect layouts).
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_flora_effects_match_the_constants() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let data = index
            .read(ResourceKey { group_id: 0x40450200, instance_id: 0x5F848353, type_id: 0xEA5118B0 })
            .expect("planet effdir");
        let (dists, tds) = effdir_flora::decode(&data).expect("decode");
        assert_eq!(dists.len(), 2700);
        assert_eq!(tds.len(), 57);
        // terrainDistribute #17: level 2, 3000, facing 0.65, type All.
        let l = &tds[17][2];
        assert_eq!((l.0, l.1), (3000.0, 0.65));
        // Distribute 512: 3 groves, square 0.6, size 6.
        let d = &dists[512];
        assert_eq!((d.density, d.source, d.scale, d.size.clone()), (3, 0, 0.6, vec![6.0]));
        for (k, class) in [(494usize, 0usize), (501, 1), (507, 2)] {
            let d = &dists[k];
            let c = CLASSES[class];
            assert_eq!(d.density as usize, c.density, "{k}");
            assert_eq!(d.source == 2, c.ring, "{k}");
            assert_eq!(d.scale, c.scale, "{k}");
            assert_eq!(d.size, c.sizes.to_vec(), "{k}");
            assert_eq!(d.emit_map, (0, 0x02C9C764), "{k}");
            assert_eq!(d.override_set as usize, 10 * (class + 1), "{k}");
        }
    }

    /// Minimal reader of the effdir sections 0x0D (distribute, v4) and 0x2A
    /// (terrainDistribute, v2), for the installed-data test.
    mod effdir_flora {
        pub struct Dist {
            pub density: i32,
            pub source: u8,
            pub scale: f32,
            pub size: Vec<f32>,
            pub emit_map: (u32, u32),
            pub override_set: u8,
        }
        struct R<'a>(&'a [u8], usize);
        impl R<'_> {
            fn take(&mut self, n: usize) -> Option<&[u8]> {
                let s = self.0.get(self.1..self.1 + n)?;
                self.1 += n;
                Some(s)
            }
            fn u8(&mut self) -> Option<u8> {
                Some(self.take(1)?[0])
            }
            fn u16(&mut self) -> Option<u16> {
                Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
            }
            fn i32(&mut self) -> Option<i32> {
                Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
            }
            fn u32(&mut self) -> Option<u32> {
                Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
            }
            fn f(&mut self) -> Option<f32> {
                Some(f32::from_be_bytes(self.take(4)?.try_into().ok()?))
            }
            fn skip(&mut self, n: usize) -> Option<()> {
                self.take(n).map(|_| ())
            }
            fn flist(&mut self) -> Option<Vec<f32>> {
                let n = self.i32()? as usize;
                (0..n).map(|_| self.f()).collect()
            }
        }
        fn dist(r: &mut R) -> Option<Dist> {
            let _flags = r.i32()?;
            let density = r.i32()?;
            let _component = r.i32()?;
            let _component_type = r.u8()?;
            let _start = r.i32()?;
            let source = r.u8()?;
            let scale = r.f()?;
            r.skip(2 + 4 + 36 + 12)?; // transform
            let size = r.flist()?;
            let _size_vary = r.f()?;
            for _ in 0..3 {
                r.flist()?; // pitch, roll, yaw
            }
            r.skip(24)?;
            let colors = r.i32()? as usize;
            r.skip(colors * 12 + 12)?;
            r.flist()?;
            r.skip(4)?;
            let surfaces = r.i32()? as usize;
            for _ in 0..surfaces {
                r.skip(4 + 8 + 20 + 8)?;
                let points = r.i32()? as usize;
                r.skip(points * 12)?;
            }
            let emit_map = (r.u32()?, r.u32()?);
            r.skip(16 + 8)?; // colour / pin maps, altitude range
            r.skip(8 + 4 + 2 + 4 + 8)?; // texture slot
            let override_set = r.u8()?;
            let _message = r.i32()?;
            r.skip(12)?;
            r.flist()?;
            Some(Dist { density, source, scale, size, emit_map, override_set })
        }
        /// Per terrainDistribute: per level (distance, facing).
        fn tdist(r: &mut R) -> Option<Vec<(f32, f32)>> {
            let _flags = r.i32()?;
            let mut levels = Vec::new();
            for _ in 0..5 {
                let distance = r.f()?;
                let _distance_no_effects = r.f()?;
                let _vertical_weight = r.f()?;
                let facing = r.f()?;
                let _facing_no_effects = r.f()?;
                let n = r.i32()? as usize;
                r.skip(n * 15)?;
                levels.push((distance, facing));
            }
            Some(levels)
        }
        pub fn decode(data: &[u8]) -> Option<(Vec<Dist>, Vec<Vec<(f32, f32)>>)> {
            let mut r = R(data, 0);
            let _version = r.u16()?;
            let (mut dists, mut tds) = (Vec::new(), Vec::new());
            loop {
                let kind = r.u16()?;
                if kind == 0xFFFF {
                    break;
                }
                let version = r.u16()?;
                let size = r.u32()? as usize;
                let start = r.1;
                let count = R(data, start).i32()? as usize;
                let mut s = R(data, start + 4);
                match (kind, version) {
                    (0x0D, 4) => dists = (0..count).map(|_| dist(&mut s)).collect::<Option<_>>()?,
                    (0x2A, 2) => tds = (0..count).map(|_| tdist(&mut s)).collect::<Option<_>>()?,
                    _ => {}
                }
                r.1 = start + size;
            }
            Some((dists, tds))
        }
    }
}
