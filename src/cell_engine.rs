//! Retail Cell-stage engine core: units, the scale ladder, the world-scale
//! mechanic, cell objects and their art, spawning, camera and registration.
//! Rules, AI, motion, population and the stage flow live in the sibling
//! `cell_rules`, `cell_ai`, `cell_motion`, `cell_population`, `cell_flow`.
//!
//! Original behaviour implemented here (see `notes\cell-re-notes.md`):
//! - progression is `cCellSerializableData::mFoodProgression` (0..1000) and the
//!   player's level is `ScaleInfoFor(food)` over the `cCellScaleInfo` table
//!   (.rdata 0x1483B70, 21 entries, RE 4A);
//! - the player is not enlarged in absolute units: when the global world scale
//!   changes by `r`, the whole world is rescaled by `1/r` around the origin
//!   (RE 4A, 0xE6DB70), the scale being smoothed in log space;
//! - object sizes come from the per-`CellStageScale` "real size" table
//!   (.rdata 0x1483B10).
//! Everything marked `ADAPTED:` is a stand-in until the RE notes cover it.

use bevy::{ecs::system::SystemParam, prelude::*};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

use crate::{
    asset_catalog::{Capabilities, PartDefinition, CELL_PARTS, RW4},
    cell_contract::{CellAnimState, CellEffectRequest, CellHudState, CellWorldState},
    cell_data::{
        AiType, AttachmentType, CellDatabase, CellRecord, CellScale, CellType, Density, UnlockType,
    },
    cell_visuals::{CellVisualAssets, OriginalCellVisual, CELL_FILE, CELL_MODEL_GROUP},
    dbpf::ResourceKey,
    game::{Health, Player, RunProgress},
    stages::{GameStage, StageOwned},
    OriginalModel, Originals,
};

// ---------------------------------------------------------------------------
// Units and the scale ladder
// ---------------------------------------------------------------------------

/// `playerSizeFor` base (0xE4EED0): 0.275 * (+0x0C / +0x10).
pub const HERO_SIZE: f32 = 0.275;
/// The avatar's steady-state `mTargetSize` on a level's first sub-step:
/// SetSize gives sqrt(3) * playerSizeFor (RE 4B).
pub const PLAYER_BASE_SIZE: f32 = 1.732_050_8 * HERO_SIZE;
/// Bevy units per retail Cell world unit. ADAPTED (presentation only): the
/// player keeps a constant world size in retail, and OpenSpore's player body,
/// mounted parts and editor slots are authored for a 0.9 radius, so the
/// avatar's 0.476 retail size maps to that radius.
pub const UNIT: f32 = crate::cell::CELL_BODY_RADIUS / PLAYER_BASE_SIZE;
/// Camera height above the play plane at zoom factor 1. Retail offsets the
/// camera by (0, 0, 20) world units (RE 4C) with the 0x7C5400(50) FOV; the
/// height frames the same view (13.99 world units high, see
/// `cell::CAMERA_HEIGHT_BASE`); the zoom factor scales it exactly as retail
/// scales its offset.
pub const CAMERA_HEIGHT: f32 = crate::cell::CAMERA_HEIGHT_BASE;
/// Retail viewer: eye and target are both shifted by (0, -0.5, 0) x the ramp
/// 0xE513B0(20, 0, offset.z * zoomFactor, 1, 0) = zoomFactor (0xE600A0), so
/// the look point sits this many world units above the screen centre at zoom 1.
pub const CAMERA_SCREEN_SHIFT: f32 = 0.5;
/// Retail visible box (`cCellGFX::sVisibleBackgroundBBox`), half extents in
/// world units around the look point, rewritten every simulation step
/// (0xE507F0 from 0xE7F3A0, also at LevelInit): independent of zoom and
/// window shape. Spawning, the level-change cull and the mate spawn use it.
pub const VISIBLE_BOX_HALF: Vec2 = Vec2::new(10.0, 7.5);

/// Depth below the play plane a background-layer cell is drawn at, Bevy
/// units. VERIFIED (0xE549B0, every frame after the main viewer): the
/// background world has its own viewer, eye = look point + the camera offset
/// WITHOUT the zoom factor + (0, 0, backgroundDistance), same FOV, so zooming
/// magnifies the foreground only. One OpenSpore camera at
/// `CAMERA_HEIGHT * zoom` reproduces that view by pushing the background
/// down by the height the camera came down: `depth + CAMERA_HEIGHT * (1 - zoom)`.
/// The stored depth is the zoom-1 depth (elevation, 0xE6DB70 never rescales it).
pub fn background_draw_depth(depth: f32, zoom_factor: f32) -> f32 {
    depth + CAMERA_HEIGHT * (1.0 - zoom_factor)
}
/// `mZoom` range, wheel step and key rate (RE 4C).
pub const ZOOM_MAX: f32 = 960.0;
pub const ZOOM_WHEEL_STEP: f32 = 120.0;
pub const ZOOM_KEY_RATE: f32 = 1200.0;
/// The default camera offset height (`sCellUI+0x74` / target `+0x80`, LevelInit 0xE801D6 writes
/// 20 = 0x1481754; the clamp of the wheel zoom 0xE50F40 starts at it).
pub const CAMERA_OFFSET_Z: f32 = 20.0;
/// zoomFactor follows G(mouse_zoom, mZoom) at this rate (1 - e^(-4 dt)).
pub const ZOOM_FOLLOW_RATE: f32 = 4.0;

/// One `cCellScaleInfo` row (RE 4A, exact .rdata values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleInfo {
    /// Player `CellStageScale` index for this sub-step.
    pub scale: i32,
    pub required_food: i32,
    pub field_8: i32,
    /// Absolute player size (grows by sqrt(3) per sub-step).
    pub size: f32,
    /// `+C` of the scale's first sub-step.
    pub level_size: f32,
    pub field_14: u32,
    /// 0, .25, .5, .75, 1 per biome pair.
    pub field_18: f32,
}

const fn si(
    scale: i32,
    required_food: i32,
    field_8: i32,
    size: f32,
    level_size: f32,
    field_14: u32,
    field_18: f32,
) -> ScaleInfo {
    ScaleInfo {
        scale,
        required_food,
        field_8,
        size,
        level_size,
        field_14,
        field_18,
    }
}

/// `cCellScaleInfo` table at .rdata 0x1483B70 (RE 4A).
pub const SCALE_INFO: [ScaleInfo; 21] = [
    si(1, 0, 0, 1.0, 1.0, 0x1D2E_C0A4, 0.0),
    si(1, 50, 5, 1.73, 1.0, 0x1D2E_C0A4, 0.0),
    si(2, 100, 10, 3.0, 3.0, 0x1D2E_C0A4, 0.0),
    si(2, 150, 13, 5.2, 3.0, 0x1D2E_C0A4, 0.0),
    si(3, 200, 16, 10.0, 10.0, 0x1D2E_C0A5, 0.25),
    si(3, 250, 19, 17.3, 10.0, 0x1D2E_C0A5, 0.25),
    si(4, 300, 22, 30.0, 30.0, 0x1D2E_C0A5, 0.25),
    si(4, 350, 25, 52.0, 30.0, 0x1D2E_C0A5, 0.25),
    si(5, 400, 28, 100.0, 100.0, 0x1D2E_C0A6, 0.5),
    si(5, 450, 31, 173.0, 100.0, 0x1D2E_C0A6, 0.5),
    si(6, 500, 34, 300.0, 300.0, 0x1D2E_C0A6, 0.5),
    si(6, 550, 37, 520.0, 300.0, 0x1D2E_C0A6, 0.5),
    si(7, 600, 40, 1000.0, 1000.0, 0x1D2E_C0A7, 0.75),
    si(7, 650, 43, 1730.0, 1000.0, 0x1D2E_C0A7, 0.75),
    si(8, 700, 46, 3000.0, 3000.0, 0x1D2E_C0A7, 0.75),
    si(8, 750, 49, 5200.0, 3000.0, 0x1D2E_C0A7, 0.75),
    si(9, 800, 52, 10000.0, 10000.0, 0x1D2E_C0A0, 1.0),
    si(9, 850, 55, 17300.0, 10000.0, 0x1D2E_C0A0, 1.0),
    si(10, 900, 58, 30000.0, 30000.0, 0x1D2E_C0A0, 1.0),
    si(10, 950, 61, 52000.0, 30000.0, 0x1D2E_C0A0, 1.0),
    si(10, 1000, 65, 52000.0, 30000.0, 0x1D2E_C0A0, 1.0),
];

/// Food progression that completes the stage.
pub const STAGE_FOOD: i32 = 1000;

/// `ScaleInfoFor(food)`: 1000 -> entry 20, else the last entry whose
/// requiredFood <= food (RE 4A).
pub fn scale_info_index(food: i32) -> usize {
    if food >= STAGE_FOOD {
        return SCALE_INFO.len() - 1;
    }
    SCALE_INFO
        .iter()
        .rposition(|s| s.required_food <= food)
        .unwrap_or(0)
}

pub fn scale_info(food: i32) -> &'static ScaleInfo {
    &SCALE_INFO[scale_info_index(food)]
}

/// Player `CellStageScale` for a food progression.
pub fn player_scale(food: i32) -> CellScale {
    CellScale(scale_info(food).scale)
}

/// Biome pair 0..=4 (ice, rock, plant, water, beach) of a food progression.
pub fn biome(food: i32) -> usize {
    ((scale_info(food).field_18 * 4.0).round() as usize).min(4)
}

/// "Real size" per `CellStageScale` index (.rdata 0x1483B10, RE 4A):
/// 10, 30, 100, 300, 1e3 ... alternating x3 / x3.33.
pub fn real_size(scale: CellScale) -> f32 {
    let i = scale.index().clamp(0, CellScale::MAX);
    let decade = 10f32.powi(1 + i / 2);
    if i % 2 == 0 {
        decade
    } else {
        3.0 * decade
    }
}

/// `worldScaleFor(food)` (0xE4EE80): the scale-info +0x10 column * 0.5.
pub fn world_scale_for(food: i32) -> f32 {
    scale_info(food).level_size * 0.5
}

/// `playerSizeFor(food)` (0xE4EED0): 0.275 * (+0x0C / +0x10).
pub fn player_size_for(food: i32) -> f32 {
    let info = scale_info(food);
    HERO_SIZE * info.size / info.level_size
}

/// The avatar's absolute size (`abs` frame = world units * world scale):
/// SetSize's mTargetSize = sqrt(3) * playerSizeFor * target / current, i.e.
/// sqrt(3) * playerSizeFor in the world of its level (RE 4B, 0xE57170).
pub fn player_abs_size(food: i32) -> f32 {
    1.732_050_8 * player_size_for(food) * world_scale_for(food)
}

/// `CreateCellObject` size without an explicit size (0xE74A20): realSize[s] /
/// 30 / worldScale * U[size0, size1] * sizeFactor, in the `abs` frame (so
/// without the world scale); scale None uses base 1 (times the world scale).
pub fn object_abs_size(scale: CellScale, size: f32, size_factor: f32, world_scale: f32) -> f32 {
    if scale.is_none() {
        size * size_factor * world_scale
    } else {
        real_size(scale) / 30.0 * size * size_factor
    }
}

/// Retail `ScaleDifference` (0 MuchSmaller .. 4 MuchLarger):
/// `clamp(s - p + 2, 0, 4)` (RE 2A, 0xE57340).
pub fn scale_diff(cell: CellScale, player: CellScale) -> usize {
    (cell.index() - player.index() + 2).clamp(0, 4) as usize
}

// ---------------------------------------------------------------------------
// Cell objects
// ---------------------------------------------------------------------------

/// What a cell object is, derived from its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellKind {
    Player,
    /// `eater` (and other AI) creatures.
    Creature,
    /// AI `parts`: the player's own species and the mate.
    OwnSpecies,
    PlantFood,
    Meat,
    Rock,
    Bubble,
    /// Unlock pickup (`cell_unlock_*`, flags 0).
    Unlock,
    /// Meteor fragment (Unlock, flags 2).
    Fragment,
    Poison,
    Egg,
    Other,
}

impl CellKind {
    pub fn is_creature(self) -> bool {
        matches!(self, CellKind::Player | CellKind::Creature | CellKind::OwnSpecies)
    }
    pub fn is_food(self) -> bool {
        matches!(self, CellKind::PlantFood | CellKind::Meat)
    }
}

pub fn classify(record: &CellRecord) -> CellKind {
    let ai = record.ai.kind;
    match record.cell_type {
        CellType::Unlock if record.flags == 2 => CellKind::Fragment,
        CellType::Unlock => CellKind::Unlock,
        CellType::Poison => CellKind::Poison,
        _ if ai == AiType::Eater => CellKind::Creature,
        _ if ai == AiType::Parts => CellKind::OwnSpecies,
        _ if ai == AiType::Caterpillar => CellKind::Egg,
        // Leak cells (plant puncture, torn-off chunk) are effect carriers.
        _ if ai == AiType::Leak => CellKind::Other,
        CellType::Liquid => CellKind::PlantFood,
        CellType::Solid if ai == AiType::Player || ai == AiType::None => CellKind::Meat,
        CellType::Rock => CellKind::Rock,
        CellType::Air => CellKind::Bubble,
        _ if ai != AiType::Player && ai != AiType::None => CellKind::Creature,
        _ => CellKind::Other,
    }
}

/// Retail `cCellObjectData` essentials for one cell in the world.
#[derive(Component, Clone, Debug)]
pub struct CellObject {
    /// `mCellResource` (cell record instance ID).
    pub cell: u32,
    pub kind: CellKind,
    /// `mScale` (the player's follows its food progression).
    pub scale: CellScale,
    /// `mTargetSize` in the `abs` frame (world units * world scale).
    pub abs_size: f32,
    /// Current world radius (Bevy units): the transform scale following
    /// abs_size / world scale * UNIT through the size spring.
    pub radius: f32,
    /// Size spring velocity (Bevy units per second).
    pub size_vel: f32,
    /// `mHealthPoints` (retail maximum 6).
    pub hp: i32,
    /// `Density(cell)` (RE 4A).
    pub density: f32,
    /// `mass = Density * mTargetSize^2` in world units (RE 4A).
    pub mass: f32,
    /// Velocity on the play plane (Bevy x, z per second).
    pub velocity: Vec2,
    /// Facing (yaw, radians; 0 = -Z).
    pub heading: f32,
    /// `mTargetPosition` on the plane, `None` when idle.
    pub target: Option<Vec2>,
    /// Depth below the play plane (background levels), Bevy units.
    pub depth: f32,
    pub background: bool,
    /// Population tile that owns it (released with the tile).
    pub tile: Option<crate::cell_population::TileKey>,
    pub age: f32,
    /// Grace after hatching/spawning in which it cannot be hurt.
    pub invulnerable: f32,
    /// Retail `field_112`-style "ignore attacks" for special objects.
    pub passive: bool,
    /// Set when it must be removed this frame (eaten, popped, collected).
    pub dead: bool,
}

impl CellObject {
    pub fn plane(&self, t: &Transform) -> Vec2 {
        Vec2::new(t.translation.x, t.translation.z)
    }
}

/// Bevy yaw that faces a model along a plane direction (x, z).
pub fn heading_of(dir: Vec2) -> f32 {
    (-dir.x).atan2(-dir.y)
}

/// Unit plane direction of a yaw.
pub fn heading_dir(heading: f32) -> Vec2 {
    Vec2::new(-heading.sin(), -heading.cos())
}

/// Density constant for a cell (RE 4A: by `density`, or by `cellType` when the
/// density is None).
pub fn density_of(record: &CellRecord, globals: &crate::cell_data::CellGlobals) -> f32 {
    match record.density {
        Density::None => match record.cell_type {
            CellType::Rock => globals.density_rock,
            CellType::Solid | CellType::Unlock => globals.density_solid,
            CellType::Liquid | CellType::Poison | CellType::Nanite => globals.density_liquid,
            CellType::Air => globals.density_air,
            CellType::None => 1.0,
        },
        other => globals.density(other),
    }
}

// ---------------------------------------------------------------------------
// Game state
// ---------------------------------------------------------------------------

/// Simulation-wide Cell state (retail `cCellGame` essentials).
#[derive(Resource)]
pub struct CellGame {
    /// `mpCurrentWorld` / `mpCurrentWorldBackground`.
    pub world: u32,
    pub world_background: u32,
    /// 0 easy, 1 medium, 2 hard (`cCellSerializableData::difficulty`).
    pub difficulty: usize,
    /// Global world scale (`cCellGame+0x514C`), smoothed toward the player's
    /// scale-info size.
    pub world_scale: f32,
    pub world_scale_target: f32,
    /// Absolute position of the world origin (retail recentres the world
    /// when it drifts far): `abs = origin + world / UNIT * world_scale`.
    pub origin_abs: Vec2,
    pub player: Option<Entity>,
    pub time: f32,
    /// Advect state (RE 1C): current/base strength, variance, period, phase.
    pub flow: [f32; 5],
    pub flow_map: Option<std::sync::Arc<crate::cell_data::AdvectMap>>,
    pub flow_scale: CellScale,
    /// The water (`cCellGame+0x4108`, RE 1C): the SPH particle fluid every
    /// current reader (`FlowAt`, `flow_field`, effect map 0x01010003) samples.
    pub fluid: crate::cell_fluid::CellFluid,
    /// The cells' outline pins in the water (`cell_softbody`, retail cell
    /// fields +0x24C..+0x254).
    pub bodies: crate::cell_softbody::SoftBodies,
    pub rng: ChaCha8Rng,
    /// User zoom (`cCellUI::mZoom`, 0..=960).
    pub zoom: f32,
    /// `zoomFactor` (+0x908): follows G(mouse_zoom, mZoom).
    pub zoom_factor: f32,
    /// World-scale smoothing state (+0x5154).
    pub world_scale_vel: f32,
    /// Camera look point and follow points (`sCellUI+0x48/+0x54/+0x60`), Bevy
    /// plane coordinates.
    pub camera_look: Vec2,
    pub camera_lead: Vec2,
    pub camera_focus: Vec2,
    /// Hatch point of the current level: start-tile exclusion zone centre
    /// (absolute) while the level is fresh.
    pub start_abs: Option<Vec2>,
    /// Cinematic lock (`sCellGame->field_5158`): steering is ignored.
    pub locked: bool,
    /// `cCellGame+0x51DC`: every Eater NPC flees the player (mating / end).
    pub escape: bool,
    /// The current loot unlock pickup (cCellGame+0x516C).
    pub unlock_pickup: Option<Entity>,
    /// Camera held on a point (ending cinematic / mating: ui.field_E4/E8).
    pub camera_hold: Option<Vec2>,
    /// `sCellUI+0xF4`: the speed (world units per second) the follow points
    /// chase the held point at (3.5 from LevelInit 0xE7FD00, 3 in the ending
    /// 0xE53C50 and in the mating 0xE75C30).
    pub camera_speed: f32,
    /// The clip-driven part of the camera (0xE60A12): see [`CameraClip`].
    pub camera_clip: CameraClip,
    /// `sCellUI+0xE0` resolved to a cell that is not the avatar (the Pokey scene,
    /// 0xE798E4: the Pokey, then the part pickup, 0xE5F160): its position, set every
    /// frame by the scene. The focus object branch of 0xE600A0 (0xE601E3) chases it
    /// with both follow points at [`CellGame::camera_speed`] and applies neither
    /// the box clamp of the look point nor of the focus.
    pub camera_object: Option<Vec2>,
    /// `sCellUI+0xE0` is the avatar (0xE7968F, the natural end of the Pokey scene,
    /// and the dying avatar): both follow points chase the avatar at the speed.
    pub camera_focus_avatar: bool,
    /// `sCellUI+0x74` / `+0x80`: the camera offset's height and its target (default
    /// 20, LevelInit 0xE801D6; the Pokey scene sets 10 and 20, 0xE5F204 / 0xE7961F).
    /// Mode 0 of 0xE60A39 eases the offset toward the target at the zoom rate.
    pub offset_z: f32,
    pub offset_z_target: f32,
    /// Cells flagged `+0x354` (kept out of the cull 0xE7C3BB, 0xE7C080): the Pokey
    /// scene's two special cells while it runs.
    pub protected: Vec<Entity>,
    pub seed: u64,
}

impl Default for CellGame {
    fn default() -> Self {
        Self {
            world: 0,
            world_background: 0,
            difficulty: 1,
            world_scale: 1.0,
            world_scale_target: 1.0,
            origin_abs: Vec2::ZERO,
            player: None,
            time: 0.0,
            flow: [0.0; 5],
            flow_map: None,
            flow_scale: CellScale(1),
            fluid: Default::default(),
            bodies: Default::default(),
            rng: ChaCha8Rng::seed_from_u64(0),
            zoom: 0.0,
            zoom_factor: 1.0,
            world_scale_vel: 0.0,
            camera_look: Vec2::ZERO,
            camera_lead: Vec2::ZERO,
            camera_focus: Vec2::ZERO,
            start_abs: None,
            locked: false,
            escape: false,
            unlock_pickup: None,
            camera_hold: None,
            camera_speed: CAMERA_SPEED_DEFAULT,
            camera_clip: CameraClip::default(),
            camera_object: None,
            camera_focus_avatar: false,
            offset_z: CAMERA_OFFSET_Z,
            offset_z_target: CAMERA_OFFSET_Z,
            protected: Vec::new(),
            seed: 0,
        }
    }
}

impl CellGame {
    /// The main camera's height as a fraction of its zoom-1 height: the zoom factor
    /// times the clip pull-in scale (0xE60A12 shrinks a local copy of the offset; the
    /// background viewer 0xE549B0 reads the stored offset, so the background keeps
    /// its view while the foreground magnifies, exactly as for the zoom factor).
    ///
    /// The background viewer's eye is the stored offset (height `offset_z`) without the
    /// zoom factor, so a lower offset (the Pokey scene's 10) magnifies the background
    /// too: the push-down of the background layers is `CAMERA_HEIGHT x offset_scale x
    /// (1 - view)`, which `background_draw_depth` writes as `CAMERA_HEIGHT x (1 - f)`
    /// with `f = 1 - offset_scale (1 - view)`; 1 x view at the default offset.
    pub fn view_factor(&self) -> f32 {
        1.0 - self.offset_scale() * (1.0 - self.zoom_factor * self.camera_clip.effect().offset_scale)
    }

    /// The camera offset height as a fraction of its default (`sCellUI+0x74` / 20).
    pub fn offset_scale(&self) -> f32 {
        self.offset_z / CAMERA_OFFSET_Z
    }

    pub fn to_abs(&self, world: Vec2) -> Vec2 {
        self.origin_abs + world / UNIT * self.world_scale
    }
    pub fn to_world(&self, abs: Vec2) -> Vec2 {
        (abs - self.origin_abs) * UNIT / self.world_scale
    }
    /// World radius (Bevy units) of an `abs` size.
    pub fn world_radius(&self, abs_size: f32) -> f32 {
        abs_size / self.world_scale * UNIT
    }
    /// Retail world length of a Bevy length.
    pub fn retail(&self, bevy: f32) -> f32 {
        bevy / UNIT
    }
    /// Convert a retail world length (in units of the current world) to Bevy.
    pub fn len(&self, retail: f32) -> f32 {
        retail * UNIT
    }
}

/// Uniform grid over cell positions, rebuilt every frame (retail
/// `cCellQueryLinkedPool` / `FindCellsInRadius` equivalent).
#[derive(Resource, Default)]
pub struct CellGrid {
    pub cell_size: f32,
    pub buckets: HashMap<(i32, i32), Vec<usize>>,
    pub items: Vec<GridItem>,
    /// The largest radius among the items (broad-phase margin).
    pub max_radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GridItem {
    pub entity: Entity,
    pub position: Vec2,
    pub radius: f32,
    pub kind: CellKind,
    pub cell: u32,
    pub scale: CellScale,
}

impl CellGrid {
    pub fn rebuild(&mut self, cell_size: f32, items: impl Iterator<Item = GridItem>) {
        self.cell_size = cell_size.max(0.01);
        self.buckets.clear();
        self.items.clear();
        self.max_radius = 0.0;
        for item in items {
            self.max_radius = self.max_radius.max(item.radius);
            let key = self.key(item.position);
            self.buckets.entry(key).or_default().push(self.items.len());
            self.items.push(item);
        }
    }
    fn key(&self, p: Vec2) -> (i32, i32) {
        (
            (p.x / self.cell_size).floor() as i32,
            (p.y / self.cell_size).floor() as i32,
        )
    }
    /// Every item whose centre lies within `radius` of `center`.
    pub fn query(&self, center: Vec2, radius: f32, mut f: impl FnMut(&GridItem)) {
        let r = (radius / self.cell_size).ceil() as i32;
        let (cx, cy) = self.key(center);
        let r2 = radius * radius;
        // A query that spans more buckets than there are items (objects much
        // larger than the grid cell right after a multi-level jump) scans the
        // items instead.
        let span = (2 * r as i64 + 1).saturating_pow(2);
        if span > self.items.len() as i64 {
            for item in &self.items {
                if item.position.distance_squared(center) <= r2 {
                    f(item);
                }
            }
            return;
        }
        for x in cx - r..=cx + r {
            for y in cy - r..=cy + r {
                if let Some(bucket) = self.buckets.get(&(x, y)) {
                    for &i in bucket {
                        let item = &self.items[i];
                        if item.position.distance_squared(center) <= r2 {
                            f(item);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Art: models and effects from the cell's structure
// ---------------------------------------------------------------------------

/// Resolved visuals of one cell record.
#[derive(Clone, Default)]
pub struct CellArt {
    /// Models with their local transform, fitted to radius 1.
    pub models: Vec<(OriginalModel, Transform)>,
    /// Primary model key (for `OriginalCellVisual::model`).
    pub model_key: Option<ResourceKey>,
    /// Effect attachments to play on the cell (sent as `CellEffectRequest`).
    pub effects: Vec<u32>,
    /// Creature part capabilities (creatures only).
    pub abilities: Capabilities,
    /// Number of eyes on the creature (for the look/blindness rules).
    pub eyes: u8,
    /// Retail part counts n[t] (RE 1B: a mirrored twin counts 1, an
    /// unmirrored part 2), creatures only.
    pub counts: [u32; 13],
    /// Has a model (retail "modelled" cells: caps, CanEat, part contacts).
    pub modelled: bool,
    /// Per-part collision spheres in the fitted model frame (radius-1 model,
    /// plane x/z), creatures only (RE 11.6).
    pub spheres: Vec<PartSphere>,
    /// Weights of the eligible entries of the record's `RandomCreature`
    /// attachment (empty without one). This art is entry 0; with two or more
    /// entries every spawn picks one by weight and takes the art of that
    /// entry ([`CellArtCache::get_pick`]).
    pub random_weights: Vec<f32>,
}

/// One part collision sphere (0xE83E30 entry): part type, centre and radius
/// in the fitted model frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartSphere {
    pub kind: u8,
    pub center: Vec2,
    pub radius: f32,
    /// The part's outward axis in the same plane (retail 0xE67D30: the local
    /// -Y axis turned by the rigblock orientation); unit length, zero when it
    /// points out of the plane.
    pub axis: Vec2,
}

/// Part spheres of a creation's rigblocks in creation units (RE 11.6): the
/// block's bounding-box centre scaled and rotated, plus its position
/// (flattened to the plane); radius = half the larger of the x / z extents
/// times the scale, halved for mandibles (unless model 0xD9007204). The
/// retail x2.5 factor is uniform and cancels in the fitted frame.
pub fn rig_spheres(data: &crate::creature_data::CreatureData, part_type: impl Fn(u32) -> Option<usize>) -> Vec<PartSphere> {
    let f = f32::from_bits;
    let mut out = Vec::new();
    for block in data.rigblocks.iter().filter(|b| b.group_id() == CELL_PARTS) {
        let Some(t) = part_type(block.instance_id()) else {
            continue;
        };
        let bb = block.bounding_box_bits().map(f);
        let s = f(block.scale_bits());
        let r = block.orientation_bits().map(|row| row.map(f));
        let pos = block.position_bits().map(f);
        let c = Vec3::new((bb[0] + bb[3]) * 0.5, (bb[1] + bb[4]) * 0.5, (bb[2] + bb[5]) * 0.5) * s;
        let rc = Vec2::new(
            r[0][0] * c.x + r[0][1] * c.y + r[0][2] * c.z,
            r[1][0] * c.x + r[1][1] * c.y + r[1][2] * c.z,
        );
        let center = rc + Vec2::new(pos[0], pos[1]);
        let mut radius = (bb[3] - bb[0]).abs().max((bb[5] - bb[2]).abs()) * s * 0.5;
        if t == 2 && block.instance_id() != 0xD900_7204 {
            radius *= 0.5;
        }
        // 0xE67D30: (0, -1, 0) times the block orientation (row-vector layout,
        // v' = v R): minus the second row.
        let axis = Vec2::new(-r[1][0], -r[1][1]).normalize_or_zero();
        if center.is_finite() && radius.is_finite() && radius > 0.0 {
            out.push(PartSphere {
                kind: t as u8,
                center,
                radius,
                axis,
            });
        }
    }
    out
}

/// The random-creature entry a spawn takes: the weighted choice of
/// `cCellRandomCreatureResource`'s eligible entries for `roll` in [0, 1)
/// (cumulative weights, the first entry whose running total passes
/// `roll x total`).
pub fn pick_random_entry(weights: &[f32], roll: f32) -> usize {
    let total: f32 = weights.iter().sum();
    if weights.len() < 2 || total <= 0.0 {
        return 0;
    }
    let target = roll.clamp(0.0, 1.0) * total;
    let mut running = 0.0;
    for (i, w) in weights.iter().enumerate() {
        running += w;
        if target < running {
            return i;
        }
    }
    weights.len() - 1
}

/// Lazily resolved art per cell record (shared by every instance).
#[derive(Resource, Default)]
pub struct CellArtCache {
    pub art: HashMap<u32, CellArt>,
    /// Art of the other `RandomCreature` entries: (cell, entry) -> art.
    pub variants: HashMap<(u32, usize), CellArt>,
    fallback_mesh: Option<Handle<Mesh>>,
    fallback_materials: HashMap<CellKind, Handle<StandardMaterial>>,
    pub errors: Vec<String>,
}

/// Remove `centered_fit`'s half-turn about the up axis.
fn head_first(fit: Transform) -> Transform {
    let undo = Quat::from_rotation_y(std::f32::consts::PI).inverse();
    Transform {
        translation: undo * fit.translation,
        rotation: undo * fit.rotation,
        scale: fit.scale,
    }
}

fn creation_abilities(originals: &Originals, creature: u32) -> (Capabilities, u8, [u32; 13], Vec<PartSphere>) {
    let key = ResourceKey {
        group_id: 0x4061_6200,
        instance_id: creature,
        type_id: crate::creature_data::TYPE,
    };
    let Ok(bytes) = originals.resource_index.read(key) else {
        return (Capabilities::default(), 0, [0; 13], Vec::new());
    };
    let Ok(data) = crate::creature_data::parse(&bytes) else {
        return (Capabilities::default(), 0, [0; 13], Vec::new());
    };
    let spheres = rig_spheres(&data, |id| {
        originals
            .part_catalog
            .by_model_in(CELL_PARTS, id)
            .and_then(|p| crate::cell_motion::part_type(&p.abilities))
    });
    let mut counts = [0u32; 13];
    for block in data.rigblocks.iter().filter(|b| b.group_id() == CELL_PARTS) {
        if let Some(t) = originals
            .part_catalog
            .by_model_in(CELL_PARTS, block.instance_id())
            .and_then(|p| crate::cell_motion::part_type(&p.abilities))
        {
            counts[t] += if block.symmetric_index() >= 0 { 1 } else { 2 };
        }
    }
    let ids: Vec<u32> = data
        .rigblocks
        .iter()
        .filter(|b| b.group_id() == CELL_PARTS)
        .map(|b| b.instance_id())
        .collect();
    let eyes = ids
        .iter()
        .filter(|id| {
            originals
                .part_catalog
                .by_model_in(CELL_PARTS, **id)
                .is_some_and(|p| p.abilities.eye > 0)
        })
        .count() as u8;
    (
        originals.part_catalog.abilities_in(CELL_PARTS, ids.into_iter()),
        eyes,
        counts,
        spheres,
    )
}

impl CellArtCache {
    /// Resolve (once) the art of cell `id`.
    pub fn get(
        &mut self,
        id: u32,
        db: &CellDatabase,
        visuals: Option<&CellVisualAssets>,
        originals: &mut Originals,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> &CellArt {
        if !self.art.contains_key(&id) {
            let art = self.resolve(id, 0, db, visuals, originals, meshes, images, materials);
            self.art.insert(id, art);
        }
        &self.art[&id]
    }

    /// The art of cell `id` for random-creature entry `pick` (0 = [`Self::get`]).
    #[allow(clippy::too_many_arguments)]
    pub fn get_pick(
        &mut self,
        id: u32,
        pick: usize,
        db: &CellDatabase,
        visuals: Option<&CellVisualAssets>,
        originals: &mut Originals,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> &CellArt {
        if pick == 0 {
            return self.get(id, db, visuals, originals, meshes, images, materials);
        }
        if !self.variants.contains_key(&(id, pick)) {
            let art = self.resolve(id, pick, db, visuals, originals, meshes, images, materials);
            self.variants.insert((id, pick), art);
        }
        &self.variants[&(id, pick)]
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve(
        &mut self,
        id: u32,
        pick: usize,
        db: &CellDatabase,
        visuals: Option<&CellVisualAssets>,
        originals: &mut Originals,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> CellArt {
        let mut art = CellArt::default();
        let Some(record) = db.cell(id) else {
            return art;
        };
        if originals.resource_index.records.is_empty() {
            return art;
        }
        let mut stack = vec![record.structure];
        let mut seen = 0;
        while let Some(structure_id) = stack.pop() {
            seen += 1;
            if seen > 8 {
                break;
            }
            let Some(structure) = db.structures.get(&structure_id) else {
                continue;
            };
            for attachment in &structure.attachments {
                match attachment.kind {
                    AttachmentType::Structure => stack.push(attachment.structure),
                    AttachmentType::Creature | AttachmentType::RandomCreature => {
                        let creature = if attachment.kind == AttachmentType::Creature {
                            attachment.resource
                        } else {
                            // The query is evaluated per spawn: the eligible
                            // entries are the creature entries that name a
                            // creature (a pollinated entry takes a creation
                            // from the online pollination database, none
                            // offline); `pick` is the entry this art is for.
                            // ADAPTED: the entries' danger / speed windows are
                            // not applied (every installed creature entry has
                            // 0..0 windows) and the choice is the spawn's
                            // weighted roll (`pick_random_entry`).
                            let eligible: Vec<_> = db
                                .random_creatures
                                .get(&attachment.random_creature)
                                .map(|r| {
                                    r.entries
                                        .iter()
                                        .filter(|e| e.creature != 0 && e.kind == crate::cell_data::RandomCreatureType::Creature)
                                        .collect()
                                })
                                .unwrap_or_default();
                            if art.random_weights.is_empty() {
                                art.random_weights = eligible.iter().map(|e| e.weight.max(0.0)).collect();
                            }
                            eligible.get(pick).or(eligible.first()).map_or(0, |e| e.creature)
                        };
                        if creature == 0 {
                            continue;
                        }
                        let key = ResourceKey {
                            type_id: crate::decode::TYPE_GMDL,
                            group_id: CELL_MODEL_GROUP,
                            instance_id: creature,
                        };
                        // GMDL front (-Y) already maps to Bevy -Z, the travel
                        // direction: undo centered_fit's half-turn so creatures
                        // swim head first (cell-anim, 2026-10-17).
                        match crate::baked_model::ensure(originals, key, meshes, images, materials)
                            .and_then(|model| {
                                crate::cell_visuals::centered_fit(&model, meshes, 1.0)
                                    .map(|fit| (model, head_first(fit)))
                            }) {
                            Ok((model, fit)) => {
                                art.model_key.get_or_insert(key);
                                art.models.push((model, fit));
                                art.modelled = true;
                                let (abilities, eyes, counts, spheres) = creation_abilities(originals, creature);
                                // Into the fitted frame: model (x, y, z) is mesh (-x, z, y)
                                // (`cell_anim::gmdl_basis`), then the fit (uniform scale,
                                // no rotation). The model's +X is the creature's left, so
                                // the creation x is mirrored into the Bevy x.
                                let k = fit.scale.x.max(1e-6);
                                art.spheres.extend(spheres.into_iter().map(|p| PartSphere {
                                    kind: p.kind,
                                    center: Vec2::new(-p.center.x * k + fit.translation.x, p.center.y * k + fit.translation.z),
                                    radius: p.radius * k,
                                    axis: Vec2::new(-p.axis.x, p.axis.y),
                                }));
                                art.abilities.merge(abilities);
                                art.eyes = art.eyes.max(eyes);
                                for (total, n) in art.counts.iter_mut().zip(counts) {
                                    *total += n;
                                }
                            }
                            Err(error) => self.errors.push(format!("cell {id:08X}: {error}")),
                        }
                    }
                    AttachmentType::PlayerCreature => {
                        if let Some(body) = visuals.and_then(|v| v.body.clone()) {
                            art.model_key = body.source_definition.or(art.model_key);
                            art.modelled = true;
                            // The player body mesh is already fitted to the
                            // 0.9 body radius.
                            art.models.push((
                                body,
                                Transform::from_scale(Vec3::splat(
                                    1.0 / crate::cell::CELL_BODY_RADIUS,
                                )),
                            ));
                        }
                    }
                    AttachmentType::Model | AttachmentType::Effect => {
                        // Effects are played by `cell-fx`; models import now.
                        // ADAPTED: an effect whose ID names an installed RW4
                        // model of the same name (rocks, `CG_*`) also shows
                        // that model, so the world is visible without effects.
                        if attachment.kind == AttachmentType::Effect {
                            art.effects.push(attachment.resource);
                        }
                        let key = ResourceKey {
                            type_id: RW4,
                            group_id: 0,
                            instance_id: attachment.resource,
                        };
                        if !originals.resource_index.records.contains_key(&key) {
                            continue;
                        }
                        let mut definition = PartDefinition::raw_model(key);
                        definition.name = format!("cell {id:08X}");
                        let imported = if let Some(model) = originals.world_models.get(&key) {
                            Ok(model.clone())
                        } else {
                            crate::editor::import_definition(
                                originals,
                                &definition,
                                meshes,
                                images,
                                materials,
                            )
                        };
                        match imported.and_then(|model| {
                            crate::cell_visuals::centered_fit(&model, meshes, 1.0)
                                .map(|fit| (model, fit))
                        }) {
                            Ok((model, fit)) => {
                                originals.world_models.insert(key, model.clone());
                                art.model_key.get_or_insert(key);
                                art.models.push((model, fit));
                            }
                            Err(error) => self.errors.push(format!("cell {id:08X}: {error}")),
                        }
                    }
                    AttachmentType::Debug => {}
                }
            }
        }
        art
    }

    /// Procedural stand-in look for objects whose structure is effect-only.
    /// ADAPTED: replaced by `cell-fx` effects where they exist.
    pub fn fallback(
        &mut self,
        kind: CellKind,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        let mesh = self
            .fallback_mesh
            .get_or_insert_with(|| meshes.add(Sphere::new(1.0).mesh().uv(20, 12)))
            .clone();
        let material = self
            .fallback_materials
            .entry(kind)
            .or_insert_with(|| {
                let (color, emissive, alpha) = match kind {
                    CellKind::PlantFood => ([0.45, 0.95, 0.35], [0.05, 0.3, 0.03], 1.0),
                    CellKind::Meat => ([0.9, 0.32, 0.28], [0.3, 0.03, 0.02], 1.0),
                    CellKind::Rock => ([0.5, 0.62, 0.78], [0.02, 0.04, 0.08], 1.0),
                    CellKind::Bubble => ([0.8, 0.95, 1.0], [0.15, 0.25, 0.3], 0.35),
                    CellKind::Unlock | CellKind::Fragment => ([0.6, 0.85, 1.0], [0.6, 0.9, 1.6], 0.9),
                    CellKind::Poison => ([0.55, 0.9, 0.2], [0.2, 0.5, 0.05], 0.45),
                    CellKind::Egg => ([1.0, 0.95, 0.75], [0.2, 0.18, 0.1], 0.9),
                    _ => ([0.7, 0.7, 0.75], [0.05, 0.05, 0.05], 1.0),
                };
                materials.add(StandardMaterial {
                    base_color: Color::srgba(color[0], color[1], color[2], alpha),
                    emissive: LinearRgba::rgb(emissive[0], emissive[1], emissive[2]),
                    alpha_mode: if alpha < 1.0 {
                        AlphaMode::Blend
                    } else {
                        AlphaMode::Opaque
                    },
                    ..default()
                })
            })
            .clone();
        (mesh, material)
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// A cell object to create (populate markers, loot, eggs, the mate...).
#[derive(Clone, Debug)]
pub struct SpawnRequest {
    pub cell: u32,
    pub scale: CellScale,
    /// World position on the plane (Bevy x, z).
    pub position: Vec2,
    pub depth: f32,
    pub velocity: Vec2,
    pub heading: f32,
    /// 0..1: where in the record's `size[min, max]` range it lands.
    pub size_roll: f32,
    /// Explicit `abs` size (cluster members, the mate, loot of a source).
    pub size: Option<f32>,
    /// `sizeFactor` of CreateCellObject (1.0 for population and loot).
    pub size_factor: f32,
    pub tile: Option<crate::cell_population::TileKey>,
    pub invulnerable: f32,
    /// Extra marker component (mate, egg, pickup...).
    pub role: SpawnRole,
    /// Flock (cluster) membership: (flock id, is the leader).
    pub flock: Option<(u32, bool)>,
    /// Owner of a weapon cloud (never hurt by it).
    pub owner: Option<Entity>,
    /// Lifetime in seconds (timed events such as poison clouds, 0x1A).
    pub life: Option<f32>,
    /// Attach to a parent cell (`0xE76AF0` -> `0xE5F450`): a leak cell
    /// follows the cell it was torn from.
    pub attach: Option<CellAttach>,
}

/// Where a spawned cell is attached to its parent (0xE5F450): the offset
/// from the parent's centre in the parent's frame, in parent radii (so it
/// follows the parent's growth and turning).
#[derive(Clone, Copy, Debug)]
pub struct CellAttach {
    pub parent: Entity,
    pub local: Vec2,
    /// Facing relative to the parent's heading.
    pub heading: f32,
}

/// The leak AI state (0xE7A0D0): the parent object id (+0x14C), the timer
/// (+0x148) and where the leak sits on its parent.
#[derive(Component, Clone, Copy, Debug)]
pub struct CellLeak {
    pub attach: CellAttach,
    pub age: f32,
}

/// A cell removed after a fixed time (poison clouds: event 0x1A).
#[derive(Component, Clone, Copy, Debug)]
pub struct CellLifetime(pub f32);

impl Default for SpawnRequest {
    fn default() -> Self {
        Self {
            cell: 0,
            scale: CellScale::NONE,
            position: Vec2::ZERO,
            depth: 0.0,
            velocity: Vec2::ZERO,
            heading: 0.0,
            size_roll: 0.5,
            size: None,
            size_factor: 1.0,
            tile: None,
            invulnerable: 0.0,
            role: SpawnRole::World,
            flock: None,
            owner: None,
            life: None,
            attach: None,
        }
    }
}

/// Cluster flock membership (pool cCellGame+0xA8; cell+0x38C).
#[derive(Component, Clone, Copy, Debug)]
pub struct CellFlock {
    pub id: u32,
    pub leader: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpawnRole {
    #[default]
    World,
    Mate,
    Egg,
    Loot,
}

/// Queue of objects to create at the end of the simulation step.
#[derive(Resource, Default)]
pub struct CellSpawnQueue(pub Vec<SpawnRequest>);

/// Marker on the summoned mate (also tagged `cell::CellMate` for audio).
#[derive(Component)]
pub struct CellMateObject;

#[derive(SystemParam)]
pub struct CellArtParams<'w> {
    pub originals: ResMut<'w, Originals>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub art: ResMut<'w, CellArtCache>,
    pub visuals: Option<Res<'w, CellVisualAssets>>,
}

/// Create the queued cell objects with their art.
pub fn apply_spawns(
    mut commands: Commands,
    mut queue: ResMut<CellSpawnQueue>,
    db: Res<CellDatabase>,
    mut game: ResMut<CellGame>,
    mut params: CellArtParams,
    mut effects: MessageWriter<CellEffectRequest>,
) {
    if queue.0.is_empty() {
        return;
    }
    let Some(globals) = db.globals.as_ref() else {
        queue.0.clear();
        return;
    };
    // Safety net against runaway spawn chains (not a retail rule).
    if queue.0.len() > 2000 {
        warn!("cell spawn queue of {} truncated", queue.0.len());
        queue.0.truncate(2000);
    }
    if std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
        let mut kinds: HashMap<u32, usize> = HashMap::new();
        for r in &queue.0 {
            *kinds.entry(r.cell).or_default() += 1;
        }
        let mut top: Vec<_> = kinds.into_iter().collect();
        top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        top.truncate(4);
        let named: Vec<_> = top
            .iter()
            .map(|(c, n)| (db.cell(*c).map_or(format!("{c:08X}"), |r| r.name.clone()), *n))
            .collect();
        eprintln!("cell spawn: {} queued at t {:.2}: {named:?}", queue.0.len(), game.time);
    }
    let CellArtParams {
        originals,
        meshes,
        images,
        materials,
        art,
        visuals,
    } = &mut params;
    for request in queue.0.drain(..) {
        let Some(record) = db.cell(request.cell) else {
            continue;
        };
        let kind = classify(record);
        // A leak cell without a parent finds none on its first update
        // (0xE7A0D0 destroys it at once): it never appears.
        if record.ai.kind == AiType::Leak && request.attach.is_none() {
            continue;
        }
        // CreateCellObject (0xE74A20): size from the real-size table unless
        // explicit; orientation random about the up axis unless fixed.
        let roll = record.size[0] + (record.size[1] - record.size[0]) * request.size_roll;
        let abs_size = request.size.unwrap_or_else(|| {
            object_abs_size(request.scale, roll, request.size_factor, game.world_scale)
        });
        let radius = game.world_radius(abs_size);
        let density = density_of(record, globals);
        let world_size = abs_size / game.world_scale;
        let heading = if record.fixed_orientation { 0.0 } else { request.heading };
        let background = request.depth > 0.0;
        // Background-layer cells start with velocity = their facing (1B).
        let velocity = if background && request.velocity == Vec2::ZERO {
            heading_dir(heading) * UNIT
        } else {
            request.velocity
        };
        let entity = commands
            .spawn((
                StageOwned,
                CellObject {
                    cell: request.cell,
                    kind,
                    scale: request.scale,
                    abs_size,
                    radius,
                    size_vel: 0.0,
                    hp: record.hp.max(0),
                    density,
                    mass: density * world_size * world_size,
                    velocity,
                    heading,
                    target: None,
                    depth: request.depth,
                    background,
                    tile: request.tile,
                    age: 0.0,
                    invulnerable: request.invulnerable,
                    passive: false,
                    dead: false,
                },
                CellAnimState::default(),
                crate::cell_motion::CellMotion::default(),
                crate::cell_rules::CellCombat {
                    owner: request.owner,
                    ..default()
                },
                Transform::from_xyz(request.position.x, -request.depth, request.position.y)
                    .with_rotation(Quat::from_rotation_y(heading))
                    .with_scale(Vec3::splat(radius)),
                Visibility::default(),
            ))
            .id();
        let ai = crate::cell_ai::ai_block(record, game.difficulty);
        // The leak AI (0xE7A0D0) only counts a timer against its parent: it
        // has no perception or movement state (`CellLeak`).
        if ai.kind == AiType::Leak {
            if let Some(attach) = request.attach {
                commands.entity(entity).insert(CellLeak { attach, age: 0.0 });
            }
        } else if request.role == SpawnRole::Mate
            || (!background && ai.kind != AiType::Player && ai.kind != AiType::None)
        {
            commands
                .entity(entity)
                .insert(crate::cell_ai::CellAiState::new(request.cell, ai));
        }
        if let Some((id, leader)) = request.flock {
            commands.entity(entity).insert(CellFlock { id, leader });
        }
        if let Some(life) = request.life {
            commands.entity(entity).insert(CellLifetime(life));
        }
        // Loot unlock pickups: the previous one (cCellGame+0x516C) is removed,
        // the new one is kept (RE 6).
        if request.role == SpawnRole::Loot && record.cell_type == CellType::Unlock {
            if let Some(previous) = game.unlock_pickup.replace(entity) {
                if let Ok(mut e) = commands.get_entity(previous) {
                    e.despawn();
                }
            }
        }
        match request.role {
            SpawnRole::Mate => {
                commands
                    .entity(entity)
                    .insert((CellMateObject, crate::cell::CellMate { age: 0.0 }));
            }
            SpawnRole::Egg => {
                commands.entity(entity).insert(crate::cell_flow::CellEgg::default());
            }
            _ => {}
        }
        if let Some(name) = db.cell_name(request.cell) {
            commands.entity(entity).insert(Name::new(name));
        }
        let base = art
            .get(
                request.cell,
                &db,
                visuals.as_deref(),
                originals,
                meshes,
                images,
                materials,
            )
            .clone();
        // A RandomCreature attachment is evaluated per spawn: this instance
        // takes the art of its own weighted pick.
        let pick = if base.random_weights.len() > 1 {
            pick_random_entry(&base.random_weights, game.rng.random_range(0.0f32..1.0))
        } else {
            0
        };
        let resolved = if pick == 0 {
            base
        } else {
            art.get_pick(request.cell, pick, &db, visuals.as_deref(), originals, meshes, images, materials)
                .clone()
        };
        if let Some(model) = resolved.model_key {
            commands.entity(entity).insert(OriginalCellVisual {
                rule: ResourceKey {
                    type_id: CELL_FILE,
                    group_id: 0,
                    instance_id: request.cell,
                },
                model,
            });
        }
        if resolved.models.is_empty() {
            let (mesh, material) = art.fallback(kind, meshes, materials);
            commands.entity(entity).with_children(|p| {
                p.spawn((StageOwned, Mesh3d(mesh), MeshMaterial3d(material), Transform::default(), crate::cell_contract::CellFallbackVisual));
            });
        } else {
            for (model, local) in &resolved.models {
                crate::cell_visuals::spawn_model(&mut commands, entity, model, *local);
            }
        }
        for effect in &resolved.effects {
            effects.write(CellEffectRequest {
                effect: *effect,
                position: Vec3::ZERO,
                attach_to: Some(entity),
                scale: 1.0,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Stage setup
// ---------------------------------------------------------------------------

/// Build the database once from the install (cheap; ~8 ms). Headless apps
/// without a loaded resource index (tests, `--smoke-test`) read the install
/// directly, once per process.
fn ensure_database(commands: &mut Commands, originals: &Originals, existing: Option<&CellDatabase>) -> CellDatabase {
    if let Some(db) = existing.filter(|db| db.is_loaded()) {
        return db.clone();
    }
    static INSTALLED: std::sync::OnceLock<CellDatabase> = std::sync::OnceLock::new();
    let db = if originals.resource_index.records.is_empty() {
        INSTALLED
            .get_or_init(|| match crate::spore_install::discover() {
                Ok(install) => CellDatabase::load(&crate::asset_catalog::ResourceIndex::load(&install.packages)),
                Err(_) => CellDatabase::default(),
            })
            .clone()
    } else {
        CellDatabase::load(&originals.resource_index)
    };
    if !db.errors.is_empty() {
        warn!("Cell data: {} decode errors: {:?}", db.errors.len(), &db.errors[..db.errors.len().min(4)]);
    }
    commands.insert_resource(db.clone());
    db
}

/// OnEnter(Cell): world, player, presentation layers.
#[allow(clippy::too_many_arguments)]
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut clear: ResMut<ClearColor>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    existing: Option<Res<CellDatabase>>,
) {
    run.enter_stage(GameStage::Cell);
    let db = ensure_database(&mut commands, &originals, existing.as_deref());
    // Legacy resources the HUD/audio still read.
    commands.insert_resource(crate::cell::CellWeapon::default());
    commands.insert_resource(crate::cell::CellMating::default());
    commands.insert_resource(crate::cell::CellNotice::default());
    // The diet is chosen before the stage (new-game step, RE 10A).
    commands.insert_resource(crate::cell::CellIntro { choosing: false });
    commands.insert_resource(CellSpawnQueue::default());
    commands.insert_resource(CellArtCache::default());
    commands.insert_resource(CellGrid::default());
    commands.insert_resource(crate::cell_population::CellTiles::default());
    // LevelInit (10A): a new stage (food 0) plays the intro, else the hatch.
    commands.insert_resource(crate::cell_flow::CellFlow::start(run.cell.food_progression == 0));
    commands.insert_resource(CellHudState::default());
    commands.insert_resource(CellWorldState::default());

    // Save data: migrate a pre-retail save's DNA progress into food.
    crate::cell_rules::migrate_save(&mut run);
    let save = run.cell.clone();
    let food = save.food_progression;
    let globals = db.globals.clone();
    let world = globals.as_ref().map(|g| g.worlds[0]).unwrap_or(0);
    let world_background = globals.as_ref().map(|g| g.world_backgrounds[0]).unwrap_or(0);
    let seed = if save.game_id != 0 {
        save.game_id as u64
    } else {
        0xCE11
    };
    // The level (re)starts at its settled world scale (0xE7FD00 re-initialises
    // the level from the saved food progression).
    let world_scale = world_scale_for(food);
    let origin_abs = if save.world_scale > 0.0 {
        Vec2::from_array(save.origin_abs)
    } else {
        Vec2::ZERO
    };
    let position = if save.world_scale > 0.0 {
        // Keep the absolute position across a world-scale difference.
        Vec2::from_array(save.player_position) * save.world_scale / world_scale
    } else {
        Vec2::ZERO
    };
    let mut game = CellGame {
        world,
        world_background,
        difficulty: save.difficulty.clamp(0, 2) as usize,
        world_scale,
        world_scale_target: world_scale,
        origin_abs,
        rng: ChaCha8Rng::seed_from_u64(seed ^ 0x5EED),
        zoom: save.zoom.clamp(0.0, ZOOM_MAX),
        camera_look: position,
        camera_lead: position,
        camera_focus: position,
        seed,
        ..default()
    };
    if food == 0 && !save.player_has_moved {
        game.start_abs = Some(game.to_abs(position));
    }
    clear.0 = Color::srgb(0.03, 0.11, 0.33);

    // Presentation layers (owned by `cell-fx`; kept as they were).
    let visuals =
        crate::cell_visuals::prepare(&mut originals, &mut meshes, &mut images, &mut materials);
    let environment =
        crate::cell_environment::prepare(&mut originals, &mut meshes, &mut images, &mut materials);
    crate::cell_environment::spawn(&mut commands, &mut meshes, &mut images, &mut materials, &environment);
    commands.insert_resource(environment);
    let parallax =
        crate::cell_parallax::prepare(&mut originals, &mut meshes, &mut images, &mut materials);
    crate::cell_parallax::spawn_layers(&mut commands, &mut meshes, &mut materials, &parallax, &visuals);
    commands.insert_resource(parallax);

    // The player: the original starting body at the constant player size.
    let start_cell = globals.as_ref().map(|g| g.start_cell).unwrap_or(0);
    let record = db.cell(start_cell);
    let abs_size = player_abs_size(food);
    let radius = game.world_radius(abs_size);
    let world_size = abs_size / world_scale;
    let density = match (record, globals.as_ref()) {
        (Some(r), Some(g)) => density_of(r, g),
        _ => 50.0,
    };
    let fallback_body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.9, 0.5),
        emissive: Color::srgb(0.05, 0.3, 0.1).into(),
        ..default()
    });
    let body = visuals.body.as_ref();
    let player = commands
        .spawn((
            StageOwned,
            Player,
            crate::game::Mover { speed: 0.0 },
            Health::new(crate::cell_rules::MAX_HP as f32),
            CellObject {
                cell: start_cell,
                kind: CellKind::Player,
                scale: player_scale(food),
                abs_size,
                radius,
                size_vel: 0.0,
                // The start cell's record hp (cell_hero_L0: 3; max 6).
                hp: record.map_or(crate::cell_rules::MAX_HP, |r| r.hp.clamp(1, crate::cell_rules::MAX_HP)),
                density,
                mass: density * world_size * world_size,
                velocity: Vec2::ZERO,
                heading: save.player_heading,
                target: None,
                depth: 0.0,
                background: false,
                tile: None,
                age: 0.0,
                invulnerable: crate::cell_rules::HATCH_GRACE,
                passive: false,
                dead: false,
            },
            crate::cell_motion::CellPlayerMotion::default(),
            crate::cell_motion::CellMotion::default(),
            crate::cell_rules::CellCombat::default(),
            CellAnimState::default(),
            Mesh3d(
                body.map(|m| m.draw_mesh())
                    .unwrap_or_else(|| meshes.add(Sphere::new(crate::cell::CELL_BODY_RADIUS))),
            ),
            MeshMaterial3d(body.and_then(|m| m.material.clone()).unwrap_or(fallback_body)),
            Transform::from_xyz(position.x, 0.0, position.y)
                .with_rotation(Quat::from_rotation_y(save.player_heading))
                .with_scale(Vec3::splat(radius / crate::cell::CELL_BODY_RADIUS)),
        ))
        .id();
    if let Some(model) = body.and_then(|b| b.source_definition) {
        commands.entity(player).insert(OriginalCellVisual { rule: model, model });
    }
    game.player = Some(player);
    commands.insert_resource(game);
    commands.insert_resource(visuals);
    info!(
        "cell stage ready: food {food}, level {} (scale {}), world scale {:.2}",
        scale_info_index(food),
        player_scale(food).label(),
        world_scale
    );
}

/// After `persistence::restore`: keep the saved Cell state authoritative.
pub fn after_restore(
    run: Res<RunProgress>,
    mut player: Query<(&mut Transform, &mut Health, &mut CellObject), With<Player>>,
) {
    let Ok((mut t, mut health, mut object)) = player.single_mut() else {
        return;
    };
    t.translation.y = 0.0;
    let hp = run.cell.player_hp.clamp(0, crate::cell_rules::MAX_HP);
    if hp > 0 {
        object.hp = hp;
    }
    health.max = crate::cell_rules::MAX_HP as f32;
    health.hp = object.hp as f32;
}

// ---------------------------------------------------------------------------
// Per-frame core
// ---------------------------------------------------------------------------

/// World scale smoothing + rescale (RE 4A), currents phase (RE 1C), time and
/// the query grid.
pub fn frame_begin(
    time: Res<Time>,
    db: Res<CellDatabase>,
    run: Res<RunProgress>,
    mut game: ResMut<CellGame>,
    mut grid: ResMut<CellGrid>,
    mut objects: Query<(Entity, &mut Transform, &mut CellObject)>,
) {
    let dt = time.delta_secs().min(0.1);
    game.time += dt;
    let food = run.cell.food_progression;
    game.world_scale_target = world_scale_for(food);
    // World scale (RE 4A / 11.2): 0xE509F0(ln cur, v, ln target, 1.0,
    // 0.75 dt), the bang-bang approach.
    let current = game.world_scale.max(1e-6).ln();
    let target = game.world_scale_target.max(1e-6).ln();
    let mut next = current;
    let mut vel = game.world_scale_vel;
    bang_bang(&mut next, &mut vel, target, 1.0, 0.75 * dt);
    game.world_scale_vel = vel;
    let ratio = (next - current).exp();
    if (ratio - 1.0).abs() > 1e-7 {
        // 0xE6DB70: the whole world is rescaled by 1/r around the origin.
        game.world_scale = next.exp();
        for (_, mut t, mut object) in &mut objects {
            t.translation.x /= ratio;
            t.translation.z /= ratio;
            // The elevation (+0x80) is not in 0xE6DB70's list: background
            // cells shrink and close in on the origin but keep their depth.
            t.translation.y = if object.background {
                -background_draw_depth(object.depth, game.view_factor())
            } else {
                -object.depth
            };
            object.velocity /= ratio;
            object.radius /= ratio;
            object.size_vel /= ratio;
            if let Some(target) = object.target.as_mut() {
                *target /= ratio;
            }
        }
        game.camera_look /= ratio;
        game.camera_lead /= ratio;
        game.camera_focus /= ratio;
    }
    // Recentre when far from the origin (retail: |origin|^2 > 1e6).
    let player_at = game
        .player
        .and_then(|p| objects.get(p).ok())
        .map(|(_, t, _)| Vec2::new(t.translation.x, t.translation.z));
    if let Some(p) = player_at.filter(|p| p.length_squared() > 1e6) {
        let shift = game.to_abs(p) - game.origin_abs;
        game.origin_abs += shift;
        for (_, mut t, mut object) in &mut objects {
            t.translation.x -= p.x;
            t.translation.z -= p.y;
            if let Some(target) = object.target.as_mut() {
                *target -= p;
            }
        }
        game.camera_look -= p;
        game.camera_lead -= p;
        game.camera_focus -= p;
    }
    let world_scale = game.world_scale;
    for (_, mut t, mut object) in &mut objects {
        // The transform scale follows mTargetSize through the size spring of
        // 0xE7BFB0 (RE 11.2): acceleration 2 / s^2, the cell's speed cap
        // (background cells shrinking out: 0.6 / s), in world units.
        let target = object.abs_size / world_scale;
        if object.radius <= 0.0 {
            object.radius = target * UNIT;
            object.size_vel = 0.0;
        } else {
            let (mut x, mut v) = (object.radius / UNIT, object.size_vel / UNIT);
            let cap = if object.background && object.abs_size == 0.0 {
                BACKGROUND_SHRINK_SPEED
            } else {
                f32::MAX
            };
            size_spring(&mut x, &mut v, target, cap, SIZE_SPRING_ACCEL, dt);
            object.radius = x * UNIT;
            object.size_vel = v * UNIT;
        }
        let world_size = object.abs_size / world_scale;
        object.mass = (object.density * world_size * world_size).max(1e-6);
        object.age += dt;
        object.invulnerable = (object.invulnerable - dt).max(0.0);
        let s = if object.kind == CellKind::Player {
            object.radius / crate::cell::CELL_BODY_RADIUS
        } else {
            object.radius
        };
        t.scale = Vec3::splat(s.max(1e-4));
    }
    // Currents (RE 1C): the advect entry for the player's scale.
    let scale = player_scale(food);
    if let Some(world) = db.worlds.get(&game.world) {
        let entry = world
            .advect
            .iter()
            .find(|e| e.player_size == scale)
            .or_else(|| world.advect.iter().rev().find(|e| e.player_size.is_none()));
        if let Some(entry) = entry {
            game.flow[1] = entry.strength;
            game.flow[2] = entry.variance;
            game.flow[3] = entry.period;
            game.flow_scale = entry.scale;
            game.flow_map = db.advect.get(&entry.advect).cloned();
        } else {
            game.flow_map = None;
        }
    }
    let [_, base, variance, period, mut phase] = game.flow;
    let current_strength = if period == 0.0 {
        base
    } else {
        phase += dt;
        while phase > period {
            phase -= period;
        }
        base + variance * (std::f32::consts::TAU * phase / period).sin()
    };
    game.flow[0] = current_strength;
    game.flow[4] = phase;
    // Query grid: cells of a few player radii.
    let player_radius = crate::cell::CELL_BODY_RADIUS;
    grid.rebuild(
        player_radius * 6.0,
        objects
            .iter()
            .filter(|(_, _, o)| !o.background && !o.dead)
            .map(|(entity, t, o)| GridItem {
                entity,
                position: Vec2::new(t.translation.x, t.translation.z),
                radius: o.radius,
                kind: o.kind,
                cell: o.cell,
                scale: o.scale,
            }),
    );
}

/// Mouse wheel and +/- zoom (`cCellUI::mZoom`, RE 4C): the wheel adds its
/// delta (120 per notch), numpad +/= and -/- change it by 1200 per second,
/// clamped to [0, 960].
pub fn zoom_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    editor: Res<crate::editor::EditorState>,
    mut game: ResMut<CellGame>,
) {
    if editor.open {
        for _ in wheel.read() {}
        return;
    }
    let mut delta = 0.0;
    let dt = time.delta_secs().min(0.1);
    if keys.pressed(KeyCode::Equal) || keys.pressed(KeyCode::NumpadAdd) {
        delta += ZOOM_KEY_RATE * dt;
    }
    if keys.pressed(KeyCode::Minus) || keys.pressed(KeyCode::NumpadSubtract) {
        delta -= ZOOM_KEY_RATE * dt;
    }
    for event in wheel.read() {
        delta += match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y * ZOOM_WHEEL_STEP,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y,
        };
    }
    if delta != 0.0 {
        game.zoom = (game.zoom + delta).clamp(0.0, ZOOM_MAX);
    }
}

/// Size spring acceleration of 0xE7BFB0: 2e-6 per ms^2 = 2 / s^2 (RE 11.2).
pub const SIZE_SPRING_ACCEL: f32 = 2.0;
/// Speed cap of background cells shrinking out: 0.0006 per ms (RE 11.1).
pub const BACKGROUND_SHRINK_SPEED: f32 = 0.6;
/// Sizes below 2^-16 snap / die (RE 11.1/11.2).
pub const SIZE_EPSILON: f32 = 1.0 / 65536.0;

/// 0xE509F0: bang-bang approach of `x` to `target` with acceleration `a`;
/// returns true when settled (RE 11.2; the overshoot snap only tests v*dt
/// against the distance, as the original).
pub fn bang_bang(x: &mut f32, v: &mut f32, target: f32, a: f32, dt: f32) -> bool {
    if *x == target && *v == 0.0 {
        return true;
    }
    let s = if target >= *x { 1.0 } else { -1.0 };
    let d = (target - *x) * s;
    if *v * dt > d {
        *x = target;
        *v = 0.0;
        return true;
    }
    if d < v.abs() / a * *v * s {
        *v -= s * a * dt;
    } else {
        *v += s * a * dt;
    }
    *x += *v * dt;
    false
}

/// 0x1042580: size spring (RE 11.2).
pub fn size_spring(x: &mut f32, v: &mut f32, target: f32, vmax: f32, a: f32, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let d = (target - *x).abs();
    if d < SIZE_EPSILON {
        *x = target;
        *v = 0.0;
        return;
    }
    let sign = if target <= *x { -1.0 } else { 1.0 };
    let want = (d / dt).min((d * a).sqrt()).min(vmax) * sign;
    *v = (*v + ((want - *v) / dt).clamp(-a, a) * dt).clamp(-vmax, vmax);
    *x += *v * dt;
}

/// `SmoothCD`: critically damped approach of `value` to `target`.
pub fn smooth_cd(value: &mut f32, vel: &mut f32, target: f32, smooth_time: f32, dt: f32) {
    let omega = 2.0 / smooth_time.max(1e-4);
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = *value - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    *value = target + (change + temp) * exp;
}

/// 0xE50EB0: the look box scale, 0.3 at t = 0 easing to 1 at t = 1.
pub fn look_box_ramp(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let s = 3.0 * t * t - 2.0 * t * t * t;
    0.3 + 0.7 * s
}

/// 0xE600A0: shorten `point - center` (keeping its direction) until it lies
/// inside the box of half extents `half` around `center`.
pub fn ray_clamp(point: Vec2, center: Vec2, half: Vec2) -> Vec2 {
    let d = point - center;
    let len = d.length();
    if len <= 1.525_878_9e-5 {
        return point;
    }
    let dir = d / len;
    let mut t = len;
    if dir.x != 0.0 {
        t = t.min((half.x / dir.x).abs());
    }
    if dir.y != 0.0 {
        t = t.min((half.y / dir.y).abs());
    }
    if t >= len {
        return point;
    }
    center + dir * t
}

/// 0xE5BA90 (`chase(point, target, speed, dt)`): nothing below a speed of
/// 1.525879e-5, the target at once when the step covers the distance, else a
/// step of `speed * dt` toward it (Bevy plane units).
fn chase(point: Vec2, target: Vec2, speed: f32, dt: f32) -> Vec2 {
    if speed < 1.525_878_9e-5 {
        return point;
    }
    let step = speed * dt;
    let d = target - point;
    if step * step > d.length_squared() {
        target
    } else {
        point + d / (d.length_squared() + 1e-8).sqrt() * step
    }
}

/// 0x44A0F0: `v / sqrt(|v|^2 + 1e-8)` (the retail normalise never divides by 0).
fn normalize_eps(v: Vec2) -> Vec2 {
    v / (v.length_squared() + 1e-8).sqrt()
}

/// `sCellUI+0xF4` after LevelInit (0xE7FD00): the speed of the follow points
/// while the camera is held on a point or a focus object.
pub const CAMERA_SPEED_DEFAULT: f32 = 3.5;
/// The weight cap of the clip camera pull-in (.rdata 0x13F737C = 0.95).
pub const CAMERA_PULL_MAX: f32 = 0.95;

/// The follow points of the main camera for one step (0xE600A0 up to 0xE6049E),
/// everything in Bevy plane units (retail 0.5 / 1 world units x [`UNIT`]):
///
/// * held on a point (ui+0xE4, 0xE8): lead and focus both chase the point at
///   `hold_speed` (ui+0xF4);
/// * else, with the avatar (this port keeps no focus object): the lead is
///   dragged to the circle of 0.5 around the avatar only while it is outside it
///   (`chase` to the circle point nearest the lead at 1.5 x |velocity|); the
///   focus is dragged to the circle of 1 around the lead while it is outside
///   that circle (same speed); and while |velocity| > 0.25 the focus drifts
///   toward the lead at 0.5 per second. So the lead trails the avatar by up to
///   0.5 and the focus trails the lead by up to 1: their difference is the
///   look-ahead direction.
pub fn follow_points(
    lead: &mut Vec2,
    focus: &mut Vec2,
    player_at: Vec2,
    velocity: f32,
    hold: Option<Vec2>,
    hold_speed: f32,
    dt: f32,
) {
    if let Some(hold) = hold {
        *lead = chase(*lead, hold, hold_speed, dt);
        *focus = chase(*focus, hold, hold_speed, dt);
        return;
    }
    let speed = 1.5 * velocity;
    // 0xE602BE: |lead - player|^2 > 0.25.
    if (player_at - *lead).length_squared() > (0.5 * UNIT) * (0.5 * UNIT) {
        let target = player_at + normalize_eps(*lead - player_at) * (0.5 * UNIT);
        *lead = chase(*lead, target, speed, dt);
    }
    // 0xE603B5: |lead - focus|^2 > 1 (the lead as moved just above).
    if (*lead - *focus).length_squared() > UNIT * UNIT {
        let target = *lead + normalize_eps(*focus - *lead) * UNIT;
        *focus = chase(*focus, target, speed, dt);
    }
    // 0xE60462: the avatar moves faster than 0.25 per second.
    if velocity > 0.25 * UNIT {
        *focus = chase(*focus, *lead, 0.5 * UNIT, dt);
    }
}

/// The clip-driven part of the camera (0xE60A12 jump table on the avatar's
/// playing request code `cell+0x1B0`, minus 7): the weights the avatar's clips
/// key on their root channel. `code` is the playing code, `data4` / `data5` the
/// 0xE83380 values of 0x98C942B3 / 0x98C942B2 (0 when no playing clip keys them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraClip {
    pub code: i32,
    pub data4: f32,
    pub data5: f32,
}

/// What the clip does to the look point and the camera offset: the offset scale
/// (1 = the plain offset; the pull-in lerps the offset toward zero by the weight
/// `min(Data4, 0.95)`) and the share of the way the look point is moved onto the
/// avatar's model (0xE83350, the model position, for code 46).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraClipEffect {
    pub offset_scale: f32,
    pub look_toward_avatar: f32,
}

impl CameraClip {
    /// 0xE60AD4: code 46 (move to the beach): look point toward the avatar's
    /// model by `w`, offset pulled in by `w`. 0xE60C20: code 41 (go to the
    /// editor): look point toward the focus object by Data5 (the retail focus
    /// object is the egg the go-to-editor callback 0xE778B0 sets with the follow
    /// speed 0, so the follow points are frozen there; not ported), then the
    /// pull-in. 0xE60D6D: codes 7 / 8 / 9 (death), 43 / 44 (hatch): the pull-in
    /// only. Every other code leaves the camera alone.
    pub fn effect(&self) -> CameraClipEffect {
        let w = self.data4.min(CAMERA_PULL_MAX);
        match self.code {
            7 | 8 | 9 | 41 | 43 | 44 => CameraClipEffect {
                offset_scale: 1.0 - w,
                look_toward_avatar: 0.0,
            },
            46 => CameraClipEffect {
                offset_scale: 1.0 - w,
                look_toward_avatar: w,
            },
            _ => CameraClipEffect {
                offset_scale: 1.0,
                look_toward_avatar: 0.0,
            },
        }
    }
}

/// Top-down camera (RE 4C, 0xE600A0): the follow points (see
/// [`follow_points`]); the look point leads along the motion by
/// G(camera_lookahead, v) x (lead - focus) and eases at G(camera_lerpfactor, v);
/// the height is the base offset times zoomFactor, which follows
/// G(mouse_zoom, mZoom) at rate 4 (1.0 in death/editor states).
/// The look point is pulled back toward the player along its own direction
/// until it fits the box (7, 5) x f(t) (0xE600A0), f(t) = 0.3 + 0.7
/// smoothstep(t) (0xE50EB0) with t = ui+0xBC ramping at 0.2/s; the focus point
/// is then clamped into the box around the look point (0xE5BA10). INFERRED:
/// f = 1 (every writer of ui+0xBC found sets 1: LevelInit, 0xE750C0, UI load).
/// The camera's look point and zoom factor (simulation side: the population
/// uses the visible box even when no camera renders).
pub fn camera_track(
    time: Res<Time>,
    db: Option<Res<CellDatabase>>,
    flow: Option<Res<crate::cell_flow::CellFlow>>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    anim_assets: Option<Res<crate::cell_anim::CellAnimAssets>>,
    mut game: ResMut<CellGame>,
    player: Query<(&Transform, &CellObject, Option<&crate::cell_anim::CellAnimPlayback>), With<Player>>,
) {
    let Ok((p, object, playback)) = player.single() else {
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let graphs = db.as_ref().map(|db| &db.graphs);
    let eval = |id: u32, x: f32, default: f32| graphs.map_or(default, |g| g.eval_or(id, x, default));
    let player_at = object.plane(p);
    let v = object.velocity.length() / UNIT;
    let lookahead = eval(crate::cell_data::CellGraphs::CAMERA_LOOKAHEAD, v, 0.0);
    let lerp = eval(crate::cell_data::CellGraphs::CAMERA_LERPFACTOR, v, 1.5);
    // 0xE72180 (the avatar's death): `sCellUI+0xE0` = the dying cell, so the follow
    // points chase that cell's position at `+0xF4` (the focus object branch, the
    // same chase as a held point) until LevelInit clears it.
    let dying = flow.as_ref().is_some_and(|f| f.phase == crate::cell_flow::FlowPhase::Dying);
    // The focus object branch (0xE601E3): the dying avatar, the avatar after the Pokey
    // scene's natural end (0xE7968F) or the scene's cell (0xE798E4, 0xE5F1EA); otherwise
    // the held point (ending, mating) or the follow rules.
    let focus_object = if dying || game.camera_focus_avatar { Some(player_at) } else { game.camera_object };
    // (0xE60179: the held point is tested first.)
    let hold = game.camera_hold.or(focus_object);
    let hold_speed = game.camera_speed * UNIT;
    let (mut lead, mut focus) = (game.camera_lead, game.camera_focus);
    follow_points(&mut lead, &mut focus, player_at, object.velocity.length(), hold, hold_speed, dt);
    (game.camera_lead, game.camera_focus) = (lead, focus);
    let goal = game.camera_lead + (game.camera_lead - game.camera_focus) * lookahead;
    let look = game.camera_look + (goal - game.camera_look) * (1.0 - (-lerp * dt).exp());
    let half = Vec2::new(7.0, 5.0) * UNIT * look_box_ramp(1.0);
    // 0xE6055C..0xE607EF: the box clamps only with the avatar and without a focus object.
    if focus_object.is_none() {
        game.camera_look = ray_clamp(look, player_at, half);
        let look = game.camera_look;
        game.camera_focus = game.camera_focus.clamp(look - half, look + half);
    } else {
        game.camera_look = look;
    }
    // 0xE60A39 (mode 0): the offset height eases toward its target like the zoom factor.
    game.offset_z += (game.offset_z_target - game.offset_z) * (1.0 - (-ZOOM_FOLLOW_RATE * dt).exp());
    // 0xE60802: the zoom goal is 1 while a cinematic starts / runs (the
    // cinematic manager's state 1 / 2) or the stage is in cinematic / HUD-hidden
    // mode (`sCellUI+0x936`).
    let frozen = flow.is_some_and(|f| f.camera_frozen()) || cinematics.as_ref().is_some_and(|c| c.is_playing());
    let zoom_goal = if frozen {
        1.0
    } else {
        eval(crate::cell_data::CellGraphs::MOUSE_ZOOM, game.zoom, 1.0)
    };
    game.zoom_factor += (zoom_goal - game.zoom_factor) * (1.0 - (-ZOOM_FOLLOW_RATE * dt).exp());
    // 0xE60A12: the avatar's playing clip code selects the pull-in block; the
    // weights are its root channels (0xE83380).
    game.camera_clip = match playback.and_then(|pb| pb.code.map(|c| (c, pb))) {
        Some((code, pb)) if matches!(code, 7 | 8 | 9 | 41 | 43 | 44 | 46) => {
            let channel = |id: u32| anim_assets.as_ref().map_or(0.0, |a| crate::cell_anim::game_channel(a, pb, id));
            CameraClip {
                code,
                data4: channel(crate::cell_anim::DATA4_CAMERA_PULL),
                data5: channel(crate::cell_anim::DATA5_CAMERA_FOCUS),
            }
        }
        _ => CameraClip::default(),
    };
}

/// Place the game camera from the tracked look point (PostUpdate).
pub fn camera(
    editor: Res<crate::editor::EditorState>,
    game: Option<Res<CellGame>>,
    player: Query<&Transform, (With<Player>, Without<crate::GameCamera>)>,
    mut camera: Query<&mut Transform, (With<crate::GameCamera>, Without<Player>)>,
    capture: Option<Res<crate::runtime::RenderSmokeCamera>>,
) {
    let (Some(game), false, None) = (game, editor.open, capture) else {
        return;
    };
    let (Ok(p), Ok(mut c)) = (player.single(), camera.single_mut()) else {
        return;
    };
    let player_at = Vec2::new(p.translation.x, p.translation.z);
    // 0xE600A0: camera x/y = player + (look - player) x 0xE513B0(0.5, 1, zf,
    // 0.5, 1) = clamp(zf, 0.5, 1).
    let pull = game.zoom_factor.clamp(0.5, 1.0);
    let mut look = player_at.lerp(game.camera_look, pull);
    // 0xE60A12: the avatar's clip pulls the camera in (and, moving to the
    // beach, onto the model).
    let clip = game.camera_clip.effect();
    look = look.lerp(player_at, clip.look_toward_avatar);
    let height = CAMERA_HEIGHT * game.zoom_factor * clip.offset_scale * game.offset_scale();
    // 0xE600A0: eye and target shifted toward the bottom of the screen
    // (screen up is -Z here) by 0.5 x s world units, s = 0xE513B0(20, 0, offset
    // height, 1, 0) = offset height / 20 clamped to 0..1 = zoomFactor x the
    // pull-in scale.
    let shift = (game.zoom_factor * clip.offset_scale * game.offset_scale()).clamp(0.0, 1.0);
    let at = Vec3::new(look.x, 0.0, look.y + CAMERA_SCREEN_SHIFT * shift * UNIT);
    *c = Transform::from_translation(at + Vec3::Y * height).looking_at(at, Vec3::NEG_Z);
}

/// Publish `CellWorldState` for presentation systems.
pub fn publish_world(
    run: Res<RunProgress>,
    game: Res<CellGame>,
    mut state: ResMut<CellWorldState>,
    player: Query<&Transform, With<Player>>,
) {
    let food = run.cell.food_progression;
    state.scale = player_scale(food).index();
    state.camera_height = CAMERA_HEIGHT * game.zoom_factor;
    if let Ok(t) = player.single() {
        state.player = Vec2::new(t.translation.x, t.translation.z);
    }
    state.flow_strength = game.flow[0];
    state.flow_variance = game.flow[2];
    state.flow_period = game.flow[3];
    state.time = game.time;
    state.world_scale = game.world_scale;
    state.food_progression = food;
    state.biome = biome(food);
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the Cell engine (called from `configure_game`).
pub fn register(app: &mut App) {
    app.add_message::<CellEffectRequest>()
        .add_message::<crate::game_audio::PlayCue>()
        .init_resource::<CellHudState>()
        .init_resource::<CellWorldState>()
        .init_resource::<CellGame>()
        .init_resource::<CellGrid>()
        .init_resource::<CellSpawnQueue>()
        .init_resource::<crate::cell_part_guide::PartGuideChase>()
        .init_resource::<crate::cell_part_guide::PartGuideScene>()
        .init_resource::<CellArtCache>()
        .init_resource::<CellDatabase>()
        .init_resource::<crate::cell_population::CellTiles>()
        .init_resource::<crate::cell_flow::CellFlow>()
        .init_resource::<crate::cell_rules::CellContacts>()
        .add_systems(
            OnEnter(GameStage::Cell),
            after_restore.after(crate::persistence::restore),
        )
        .add_systems(
            Update,
            (
                frame_begin,
                crate::cell_fluid::update,
                crate::cell_motion::player_input,
                crate::cell_ai::think,
                crate::cell_motion::locomote,
                crate::cell_ai::leaks,
                camera_track,
                crate::cell_rules::interactions,
                crate::cell_rules::drain_pull,
                crate::cell_rules::swallow,
                crate::cell_rules::dying,
                crate::cell_rules::weapons,
                crate::cell_rules::lifetimes,
                crate::cell_rules::resolve_deaths,
                crate::cell_population::stream,
                crate::cell_flow::update,
                crate::cell_part_guide::part_guide,
                apply_spawns,
                (crate::cell_part_guide::part_guide_chase, crate::cell_part_guide::part_guide_scene).chain(),
                crate::cell_rules::sync_save,
            )
                .chain()
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(GameStage::Cell))
                .run_if(crate::cell::intro_done)
                .run_if(crate::cell_flow::sim_running),
        )
        .add_systems(
            Update,
            (zoom_input, crate::cell_flow::dismiss)
                .in_set(crate::GameSystems::Controls)
                .run_if(in_state(GameStage::Cell)),
        )
        // The diet pick is a new-game step before the stage (10A).
        .add_systems(Update, crate::cell::cell_intro.in_set(crate::GameSystems::Controls))
        .add_systems(
            Update,
            (
                publish_world,
                crate::cell_flow::publish_hud,
                crate::cell_flow::animate,
                crate::cell::cell_dress_pickups,
            )
                .chain()
                .in_set(crate::GameSystems::Presentation)
                .run_if(in_state(GameStage::Cell))
                .run_if(resource_exists::<CellGame>),
        )
        .add_systems(
            PostUpdate,
            camera
                .run_if(in_state(GameStage::Cell))
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

/// Deterministic per-key random generator (tiles, loot rolls).
pub fn keyed_rng(parts: &[u64]) -> ChaCha8Rng {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for byte in p.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    ChaCha8Rng::seed_from_u64(h)
}

/// Random point in a disc of `radius` (uniform by area).
pub fn random_in_disc(rng: &mut impl Rng, radius: f32) -> Vec2 {
    let a = rng.random_range(0.0..std::f32::consts::TAU);
    let r = radius * rng.random_range(0.0f32..1.0).sqrt();
    Vec2::new(a.cos(), a.sin()) * r
}

/// Retail animation request codes (`cell+0x18C` states and the one-shot
/// codes actions pass to `PlayAnimation`; RE 7 and
/// `notes\cell-re\cell_anim_definitions.txt`). `cell-anim` maps a code to a
/// table row; the engine only writes the code the original writes.
#[allow(dead_code)]
pub mod anim_code {
    pub const IDLE: i32 = 0;
    pub const CHEW_FILTER: i32 = 1;
    pub const CHEW_MANDIBLE: i32 = 2;
    pub const CHEW_PROBOSCIS: i32 = 3;
    pub const STAB_SOLID: i32 = 4;
    pub const BITE_SOLID: i32 = 5;
    pub const DEATH: i32 = 7;
    pub const DEATH_BROKEN: i32 = 8;
    pub const DEATH_EATEN: i32 = 9;
    pub const DYING: i32 = 10;
    pub const EAT_FILTER: i32 = 11;
    pub const EAT_MANDIBLE: i32 = 12;
    pub const EAT_PROBOSCIS: i32 = 13;
    /// Hit reactions by damage kind: 1 -> 14, 3 -> 15, 5 -> 17, else 16.
    pub const HIT_ELECTRIC: i32 = 14;
    pub const HIT_POISON: i32 = 15;
    pub const HIT_STAB: i32 = 16;
    pub const HIT_BITE: i32 = 17;
    pub const MOVE: i32 = 18;
    pub const MOVE_SCARED: i32 = 19;
    pub const MOVE_MAD: i32 = 20;
    pub const SCARED: i32 = 21;
    pub const MAD: i32 = 22;
    pub const ASLEEP: i32 = 23;
    pub const GLANCE_MAD: i32 = 26;
    pub const GLANCE_SPIKE: i32 = 27;
    /// A spike stabs a plant (0xE777A0: PlayAnimation(attacker, 0, 0x1C)).
    pub const PUNCTURE_SPIKE: i32 = 28;
    pub const GROW: i32 = 29;
    pub const BUBBLE_POP: i32 = 31;
    pub const STARTLED: i32 = 35;
    pub const CLASH_SPIKE: i32 = 37;
    pub const CLASH_JAW: i32 = 38;
    pub const CLASH_PROBOSCIS: i32 = 39;
    pub const GO_TO_EDITOR: i32 = 41;
    pub const SPAWN: i32 = 42;
    pub const HATCH: i32 = 43;
    pub const HATCH_ICE: i32 = 44;
    pub const MOV_TO_BEACH: i32 = 46;
    pub const CILIA_REVERSE: i32 = 47;
    pub const SWIM_START: i32 = 48;
    pub const PART_PICKUP: i32 = 49;
    pub const MATE_A: i32 = 65;
    pub const MATE_B: i32 = 66;
    pub const MATING_CALL: i32 = 67;
    pub const BRAIN_UPGRADE: i32 = 68;
}

/// Request an animation: the retail code (contract `request_code`) and the
/// matching legacy slot for the current presentation.
pub fn request_anim(anim: &mut CellAnimState, code: i32, slot: crate::cell_contract::CellAnimation) {
    if anim.request_code != code {
        anim.request_code = code;
        anim.elapsed = 0.0;
        anim.finished = false;
    }
    anim.request(slot);
}

/// Unlock type -> legacy `cell_found_parts` kind used by the editor gate.
pub fn unlock_kind(unlock: UnlockType) -> Option<&'static str> {
    Some(match unlock {
        UnlockType::Mandible => "mandible",
        UnlockType::Filter => "filter",
        UnlockType::Proboscis => "proboscis",
        UnlockType::Chemical => "poison",
        UnlockType::Electric => "electric",
        UnlockType::Spike => "spike",
        UnlockType::Cilia => "cilia",
        UnlockType::Flagella => "flagella",
        UnlockType::Jet => "jet",
        UnlockType::Body | UnlockType::None => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_creature_entries_are_picked_by_weight() {
        // Equal weights split the unit roll evenly; a single entry is always entry 0.
        let w = [1.0, 1.0, 1.0, 1.0];
        assert_eq!([0.0, 0.24, 0.26, 0.6, 0.76, 0.99].map(|r| pick_random_entry(&w, r)), [0, 0, 1, 2, 3, 3]);
        assert_eq!(pick_random_entry(&[1.0], 0.9), 0);
        assert_eq!(pick_random_entry(&[], 0.9), 0);
        // A heavier entry takes a bigger share; a zero weight is never picked.
        assert_eq!([0.1, 0.5, 0.9].map(|r| pick_random_entry(&[1.0, 0.0, 3.0], r)), [0, 2, 2]);
    }

    #[test]
    fn scale_info_follows_the_retail_table() {
        assert_eq!(scale_info_index(0), 0);
        assert_eq!(scale_info_index(49), 0);
        assert_eq!(scale_info_index(50), 1);
        assert_eq!(scale_info_index(999), 19);
        assert_eq!(scale_info_index(1000), 20);
        assert_eq!(scale_info_index(5000), 20);
        assert_eq!(player_scale(0).label(), "1");
        assert_eq!(player_scale(150).label(), "2");
        assert_eq!(player_scale(200).label(), "4");
        assert_eq!(player_scale(900).label(), "1K");
        // Ten player sizes, two sub-steps each, as world_ice lists them.
        let scales: Vec<_> = SCALE_INFO.iter().step_by(2).map(|s| s.scale).collect();
        assert_eq!(scales, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10]);
        for pair in SCALE_INFO.windows(2) {
            assert!(pair[1].required_food > pair[0].required_food);
            assert!(pair[1].size >= pair[0].size);
        }
        assert_eq!(biome(0), 0);
        assert_eq!(biome(250), 1);
        assert_eq!(biome(450), 2);
        assert_eq!(biome(650), 3);
        assert_eq!(biome(850), 4);
    }

    #[test]
    fn real_sizes_alternate_three_and_ten_thirds() {
        assert_eq!(real_size(CellScale(0)), 10.0);
        assert_eq!(real_size(CellScale(1)), 30.0);
        assert_eq!(real_size(CellScale(2)), 100.0);
        assert_eq!(real_size(CellScale(3)), 300.0);
        assert_eq!(real_size(CellScale(4)), 1000.0);
        assert!((real_size(CellScale(19)) - 3e10).abs() < 1e5);
        // CreateCellObject: realSize/30 * U * sizeFactor (abs frame).
        assert_eq!(object_abs_size(CellScale(1), 0.25, 1.0, 0.5), 0.25);
        assert_eq!(object_abs_size(CellScale::NONE, 0.25, 2.0, 0.5), 0.25);
    }

    #[test]
    fn levels_body_size_and_world_scale_follow_4b() {
        let ws: Vec<f32> = (0..=20).map(|i| world_scale_for(SCALE_INFO[i].required_food)).collect();
        assert_eq!(&ws[..6], &[0.5, 0.5, 1.5, 1.5, 5.0, 5.0]);
        assert_eq!(ws[20], 15000.0);
        assert!((player_size_for(0) - 0.275).abs() < 1e-6);
        assert!((player_size_for(50) - 0.275 * 1.73).abs() < 1e-5);
        // The avatar is sqrt(3) * playerSizeFor in the world of its level.
        let world = player_abs_size(100) / world_scale_for(100);
        assert!((world - PLAYER_BASE_SIZE).abs() < 1e-5);
        assert!((PLAYER_BASE_SIZE * UNIT - crate::cell::CELL_BODY_RADIUS).abs() < 1e-5);
    }

    #[test]
    fn retail_filters_settle() {
        // 0xE509F0 on ln(world scale): reaches ln 3 and stops.
        let (mut x, mut v) = (0.0f32, 0.0f32);
        let target = 3.0f32.ln();
        let settled = (0..4000).any(|_| bang_bang(&mut x, &mut v, target, 1.0, 0.75 / 60.0));
        assert!(settled && (x - target).abs() < 1e-6 && v == 0.0, "{x} {v}");
        // 0x1042580: the avatar grows a sub-step without overshooting.
        let (mut s, mut sv) = (0.476f32, 0.0f32);
        let mut peak = 0.0f32;
        for _ in 0..600 {
            size_spring(&mut s, &mut sv, 0.825, f32::MAX, SIZE_SPRING_ACCEL, 1.0 / 60.0);
            peak = peak.max(s);
        }
        assert!((s - 0.825).abs() < 1e-3 && peak < 0.84, "{s} {peak}");
        // Background cells shrink out at 0.6 / s at most.
        let (mut b, mut bv) = (1.0f32, 0.0f32);
        for _ in 0..60 {
            size_spring(&mut b, &mut bv, 0.0, BACKGROUND_SHRINK_SPEED, SIZE_SPRING_ACCEL, 1.0 / 60.0);
        }
        assert!(b >= 0.4 - 1e-3 && b < 1.0, "{b}");
    }

    #[test]
    fn smooth_cd_settles_on_the_target() {
        let (mut x, mut v) = (0.0f32, 0.0f32);
        for _ in 0..600 {
            smooth_cd(&mut x, &mut v, 1.0, 1.0, 1.0 / 60.0);
        }
        assert!((x - 1.0).abs() < 1e-3, "{x}");
    }

    #[test]
    fn scale_difference_clamps_two_levels() {
        assert_eq!(scale_diff(CellScale(1), CellScale(1)), 2);
        assert_eq!(scale_diff(CellScale(0), CellScale(1)), 1);
        assert_eq!(scale_diff(CellScale(0), CellScale(4)), 0);
        assert_eq!(scale_diff(CellScale(3), CellScale(2)), 3);
        assert_eq!(scale_diff(CellScale(9), CellScale(2)), 4);
    }

    #[test]
    fn world_frame_round_trips_through_rescale() {
        let mut game = CellGame {
            world_scale: 3.0,
            origin_abs: Vec2::new(5.0, -2.0),
            ..default()
        };
        let world = Vec2::new(1.5, 4.0);
        let abs = game.to_abs(world);
        assert!(game.to_world(abs).abs_diff_eq(world, 1e-5));
        // A rescale by r keeps absolute positions fixed.
        let r = 1.7;
        game.world_scale *= r;
        assert!(game.to_abs(world / r).abs_diff_eq(abs, 1e-4));
        assert!((game.world_radius(PLAYER_BASE_SIZE * 3.0 * r) - crate::cell::CELL_BODY_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn grid_finds_items_in_radius() {
        let mut grid = CellGrid::default();
        let e = World::new().spawn_empty().id();
        grid.rebuild(
            1.0,
            [Vec2::new(0.2, 0.2), Vec2::new(3.0, 0.0), Vec2::new(-0.9, 0.1)]
                .into_iter()
                .map(|position| GridItem {
                    entity: e,
                    position,
                    radius: 0.1,
                    kind: CellKind::Rock,
                    cell: 0,
                    scale: CellScale(1),
                }),
        );
        let mut found = 0;
        grid.query(Vec2::ZERO, 1.0, |_| found += 1);
        assert_eq!(found, 2);
    }

    #[test]
    fn heading_and_direction_agree() {
        for dir in [Vec2::X, Vec2::NEG_Y, Vec2::new(0.6, 0.8)] {
            let d = heading_dir(heading_of(dir));
            assert!(d.abs_diff_eq(dir.normalize(), 1e-5), "{dir} -> {d}");
        }
    }

    #[test]
    fn camera_look_box_is_a_ray_clamp() {
        // 0xE50EB0: 0.3 at t = 0, 1 at t = 1, smoothstep in between.
        assert!((look_box_ramp(0.0) - 0.3).abs() < 1e-6);
        assert!((look_box_ramp(1.0) - 1.0).abs() < 1e-6);
        assert!((look_box_ramp(0.5) - 0.65).abs() < 1e-6);
        // 0xE600A0: the offset keeps its direction and shrinks to the box.
        let half = Vec2::new(7.0, 5.0);
        let p = ray_clamp(Vec2::new(14.0, 5.0), Vec2::ZERO, half);
        assert!(p.abs_diff_eq(Vec2::new(7.0, 2.5), 1e-5), "{p}");
        let q = ray_clamp(Vec2::new(1.0, 1.0), Vec2::ZERO, half);
        assert_eq!(q, Vec2::new(1.0, 1.0));
        // The retail camera ramp constants.
        assert_eq!(VISIBLE_BOX_HALF, Vec2::new(10.0, 7.5));
        assert_eq!(CAMERA_SCREEN_SHIFT, 0.5);
    }

    /// 0xE600A0 / 0xE5BA90: the follow points are only dragged when they leave
    /// their circles (0.5 around the avatar, 1 around the lead), at 1.5 x the
    /// avatar's speed; so while the avatar swims the lead trails it by 0.5 and
    /// the focus trails the lead by 1 (the look-ahead vector), and at rest
    /// nothing moves.
    #[test]
    fn follow_points_trail_the_avatar_by_the_retail_radii() {
        let dt = 1.0 / 60.0;
        let speed = 4.0 * UNIT;
        let (mut lead, mut focus) = (Vec2::ZERO, Vec2::ZERO);
        let mut player = Vec2::ZERO;
        for _ in 0..300 {
            player.x += speed * dt;
            follow_points(&mut lead, &mut focus, player, speed, None, 0.0, dt);
        }
        assert!(((player.x - lead.x) / UNIT - 0.5).abs() < 0.05, "lead trails by 0.5: {}", (player.x - lead.x) / UNIT);
        assert!(((lead.x - focus.x) / UNIT - 1.0).abs() < 0.1, "focus trails by 1: {}", (lead.x - focus.x) / UNIT);
        assert!(lead.y.abs() < 1e-4 && focus.y.abs() < 1e-4);
        // At rest (speed 0 < 1.5e-5) neither point moves, wherever they are.
        let (l0, f0) = (lead, focus);
        follow_points(&mut lead, &mut focus, player, 0.0, None, 0.0, dt);
        assert_eq!((lead, focus), (l0, f0));
        // Inside the circles nothing moves even while swimming slowly.
        let (mut lead, mut focus) = (Vec2::new(0.2, 0.0) * UNIT, Vec2::new(0.0, 0.0));
        follow_points(&mut lead, &mut focus, Vec2::ZERO, 0.1 * UNIT, None, 0.0, dt);
        assert_eq!(lead, Vec2::new(0.2, 0.0) * UNIT);
        assert_eq!(focus, Vec2::ZERO);
        // The held camera: both points chase the held point at the hold speed.
        let hold = Vec2::new(10.0, 0.0) * UNIT;
        let (mut lead, mut focus) = (Vec2::ZERO, Vec2::ZERO);
        follow_points(&mut lead, &mut focus, Vec2::ZERO, 0.0, Some(hold), 3.0 * UNIT, 0.5);
        assert!(lead.abs_diff_eq(Vec2::new(1.5, 0.0) * UNIT, 1e-4), "{lead}");
        assert_eq!(lead, focus);
        // The chase lands on the target when the step covers the distance.
        for _ in 0..10 {
            follow_points(&mut lead, &mut focus, Vec2::ZERO, 0.0, Some(hold), 3.0 * UNIT, 0.5);
        }
        assert_eq!((lead, focus), (hold, hold));
    }

    /// The placed camera: height = base x zoomFactor x the clip's offset scale, and the
    /// screen shift follows the shrunken offset (0xE513B0(20, 0, offset.z, 1, 0)).
    #[test]
    fn the_camera_is_placed_from_the_look_point_and_the_clip_pull_in() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(crate::editor::EditorState::default());
        let mut game = CellGame::default();
        game.camera_look = Vec2::new(4.0, -2.0);
        world.insert_resource(game);
        world.spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player));
        let cam = world.spawn((Transform::default(), crate::GameCamera)).id();
        world.run_system_once(camera).unwrap();
        let c = *world.get::<Transform>(cam).unwrap();
        assert!((c.translation.y - CAMERA_HEIGHT).abs() < 1e-3, "{}", c.translation.y);
        // Zoom 1: x follows the look point fully, z is shifted by half a world unit.
        assert!((c.translation.x - 4.0).abs() < 1e-3);
        assert!((c.translation.z - (-2.0 + CAMERA_SCREEN_SHIFT * UNIT)).abs() < 1e-3);
        // The go-to-editor clip at Data4 = 0.5 halves the offset and the shift.
        world.resource_mut::<CellGame>().camera_clip = CameraClip { code: 41, data4: 0.5, data5: 0.0 };
        world.run_system_once(camera).unwrap();
        let c = *world.get::<Transform>(cam).unwrap();
        assert!((c.translation.y - CAMERA_HEIGHT * 0.5).abs() < 1e-3, "{}", c.translation.y);
        assert!((c.translation.z - (-2.0 + CAMERA_SCREEN_SHIFT * 0.5 * UNIT)).abs() < 1e-3);
    }

    /// 0xE60A12: death (7 / 8 / 9), hatch (43 / 44) and go-to-editor (41) pull
    /// the offset in by min(Data4, 0.95); move-to-beach (46) also moves the look
    /// point onto the model; other codes leave the camera alone.
    #[test]
    fn clip_camera_pulls_in_by_the_data4_weight() {
        for code in [7, 8, 9, 41, 43, 44] {
            let e = CameraClip { code, data4: 0.4, data5: 0.0 }.effect();
            assert!((e.offset_scale - 0.6).abs() < 1e-6 && e.look_toward_avatar == 0.0, "code {code}");
        }
        let e = CameraClip { code: 46, data4: 0.4, data5: 0.0 }.effect();
        assert!((e.offset_scale - 0.6).abs() < 1e-6 && (e.look_toward_avatar - 0.4).abs() < 1e-6);
        let e = CameraClip { code: 9, data4: 3.0, data5: 0.0 }.effect();
        assert!((e.offset_scale - 0.05).abs() < 1e-6, "capped at 0.95");
        for code in [0, 18, 10, 42, 45, 47] {
            let e = CameraClip { code, data4: 0.9, data5: 0.9 }.effect();
            assert_eq!((e.offset_scale, e.look_toward_avatar), (1.0, 0.0), "code {code}");
        }
    }

    fn plain_object(cell: u32) -> CellObject {
        CellObject {
            cell,
            kind: CellKind::Creature,
            scale: CellScale(1),
            abs_size: 1.0,
            radius: 1.0,
            size_vel: 0.0,
            hp: 3,
            density: 50.0,
            mass: 50.0,
            velocity: Vec2::ZERO,
            heading: 0.0,
            target: None,
            depth: 0.0,
            background: false,
            tile: None,
            age: 0.0,
            invulnerable: 0.0,
            passive: false,
            dead: false,
        }
    }

    /// 0xE60A39 (mode 0) and 0xE5F204: the offset height eases toward its target at the zoom
    /// rate, the camera height and the screen shift follow it, and the background (whose
    /// viewer reads the stored offset) is pushed down by `CAMERA_HEIGHT x offset x (1 - view)`.
    #[test]
    fn the_offset_height_scales_the_camera_and_the_background_view() {
        use bevy::ecs::system::RunSystemOnce;
        let mut game = CellGame::default();
        assert_eq!(game.offset_z, CAMERA_OFFSET_Z);
        assert_eq!(game.view_factor(), 1.0, "the default offset leaves the view factor alone");
        game.zoom_factor = 0.6;
        let plain = game.view_factor();
        assert!((plain - 0.6).abs() < 1e-6);
        // Offset 10: the camera comes down by half, the background by half of what it did.
        game.offset_z = 10.0;
        assert!((game.offset_scale() - 0.5).abs() < 1e-6);
        let f = game.view_factor();
        let push = background_draw_depth(0.0, f);
        assert!((push - CAMERA_HEIGHT * 0.5 * (1.0 - 0.6)).abs() < 1e-4, "{push}");
        let mut world = World::new();
        world.insert_resource(crate::editor::EditorState::default());
        game.zoom_factor = 1.0;
        game.camera_look = Vec2::new(0.0, 0.0);
        world.insert_resource(game);
        world.spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player));
        let cam = world.spawn((Transform::default(), crate::GameCamera)).id();
        world.run_system_once(camera).unwrap();
        let c = *world.get::<Transform>(cam).unwrap();
        assert!((c.translation.y - CAMERA_HEIGHT * 0.5).abs() < 1e-3, "{}", c.translation.y);
        assert!((c.translation.z - CAMERA_SCREEN_SHIFT * 0.5 * UNIT).abs() < 1e-3);
    }

    /// The focus object branch of 0xE600A0 (0xE601E3): both follow points chase the object
    /// at `ui+0xF4` and neither box clamp applies, so the look point reaches a scene far
    /// outside the (7, 5) box around the avatar; the offset eases toward its target.
    #[test]
    fn a_focus_object_draws_the_camera_away_from_the_avatar() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let mut game = CellGame::default();
        let scene = Vec2::new(15.0, 0.0) * UNIT;
        game.camera_object = Some(scene);
        game.camera_speed = 10.0;
        game.offset_z_target = 10.0;
        world.insert_resource(game);
        world.spawn((Transform::default(), plain_object(0), Player));
        for _ in 0..60 {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
            world.run_system_once(camera_track).unwrap();
        }
        let game = world.resource::<CellGame>();
        assert!((game.camera_lead - scene).length() < 0.01 * UNIT, "lead {:?}", game.camera_lead);
        assert!((game.camera_focus - scene).length() < 0.01 * UNIT);
        assert!((game.camera_look - scene).length() < 0.1 * UNIT, "no box clamp: {:?}", game.camera_look);
        assert!((game.offset_z - 10.0).abs() < 0.01, "{}", game.offset_z);
        // Without the focus object the box clamp pulls the look point back to the avatar's box.
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let mut game = CellGame::default();
        game.camera_look = scene;
        game.camera_lead = scene;
        game.camera_focus = scene;
        world.insert_resource(game);
        world.spawn((Transform::default(), plain_object(0), Player));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        world.run_system_once(camera_track).unwrap();
        assert!(world.resource::<CellGame>().camera_look.x <= 7.0 * UNIT + 1e-3);
    }
}
