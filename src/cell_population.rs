//! Population (`notes\cell-re-notes.md` section 5, 0xE7D370): tiles keyed by
//! (layer, scale, x, y) around the visible box, populated from the current
//! world's level entries and their markers.
//!
//! Original: TileFactor by d = s - p (1, .5, .25, .125, .0625, .03125);
//! TileSize = realSize[s] / worldScale * TileFactor * 0.85; wanted scales
//! p-1..p+2 (foreground) and p..p+4 (background); the foreground region is
//! the visible box grown by its own size on every side (3x3 screens); at
//! most 50 new tiles per step unless forced; forced (level start) tiles use
//! the `startTile` level entries; every level entry with that startTile flag
//! and playerSize none or p is placed; markers whose size is the tile's
//! scale; Count per difficulty; n = 2.25 * (0.85 * TileFactor)^2 * Count
//! placements (floor + one more with probability frac); unforced placements
//! are rejected when visible, foreground ones also when overlapping a cell,
//! up to 10 tries; distribute / cluster (a flock, members touching or in a
//! line) / plant spawners; elevation U[zOffset, zOffsetMax]; a world-scale
//! change re-populates every tile.
//!
//! Regions (VERIFIED, 0xE507F0 every step + 0xE5F610): the visible box is
//! the look point +-(10, 7.5) world units (zoom and window shape do not
//! change it); layer 0 tiles cover that box grown by its own width / height
//! on every side (2 x 2 boxes), layer 1 tiles the box scaled by
//! 1 + backgroundDistance / 15 (globals 0x16B3CA0..0x16B3CB0). Visibility
//! (0xE5D410 / 0xE5CDE0): layer 0 = the sphere touches the visible box,
//! layer 1 = the background viewer's frustum (0xE549B0: eye at the look point
//! + the unzoomed offset (0, 0, 20) + (0, 0, backgroundDistance), the cell at
//! its elevation). A level change removes every off-screen cell with a scale.
//!
//! Plant types are the special-cell table of 11.3 / 11.4 (type1 = clover #10-12,
//! type2 = clover2 #13-15, type3 = clover3 #16-18, type4 = vine #14; the
//! installed populates use types 1-3 only). Objects leave the world exactly as
//! retail's `0xE7C080` -> `0xE5F8C0` has it (`released`): a cell with a scale is
//! removed when the tile of ITS OWN scale that its centre stands in is not a
//! wanted tile of its layer (0xE5F810: p-1 <= s <= p+5 and the tile inside
//! the region), unless it is attached, flagged persistent (+0x354: the mate,
//! eggs and the pickup here) or special; one level or more below the player it
//! shrinks out instead (0xE7C8C0).
//!
//! Outline pins follow the visible box, not the population region (0xE78C00,
//! VERIFIED in `notes\cell-pop2-notes.md`): per cell and step retail creates the
//! cell's GFX object (0xE6D8F0, which allocates the pins) only while 0xE5CDE0
//! says the cell is on screen and destroys it, with its pins (0xE66010 ->
//! 0xE4EC10), as soon as it is not (`pin_visible`). The population region is
//! four visible boxes, so about a quarter of the cells that exist hold pins.
//!
//! ADAPTED: the frustum is tested as its cross-section box at the cell's
//! elevation; the background depth uses OpenSpore's camera height; encounter
//! markers are not used by any installed populate and are skipped.

use bevy::prelude::*;
use rand::Rng;
use std::collections::HashMap;

use crate::{
    cell_data::{CellDatabase, CellMarker, CellScale, MarkerType, PlantType},
    cell_engine::{
        keyed_rng, real_size, world_scale_for, CellGame, CellGrid, CellMateObject, CellObject, CellSpawnQueue,
        SpawnRequest, SpawnRole, CAMERA_HEIGHT, UNIT, VISIBLE_BOX_HALF,
    },
    game::{Player, RunProgress},
};

/// A plant child sits this many parent sizes from the parent's centre
/// (0xE61370 multiplies the turned unit vector by .rdata 0x13EED94 = 1.5).
pub const PLANT_ATTACH: f32 = 1.5;
/// At most this many new tiles per step unless forced.
pub const TILES_PER_STEP: usize = 50;
/// Placement tries per object.
pub const PLACE_TRIES: usize = 10;
/// Background depth per retail unit, as a fraction of [`UNIT`]. Retail keeps
/// the background world `backgroundDistance` (35) below a camera 20 units
/// above the play plane (RE 4C/10A); OpenSpore's camera is `CAMERA_HEIGHT`
/// above it, so depths take CAMERA_HEIGHT / 20 Bevy units per retail unit
/// (~0.635 UNIT), which keeps the retail parallax and apparent size of the
/// background. ADAPTED (was 0.6).
pub const BG_DEPTH_SCALE: f32 = CAMERA_HEIGHT / (20.0 * UNIT);
/// Safety cap on the tiles of one layer and scale.
pub const MAX_TILES: i64 = 900;

/// A population tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    /// 0 foreground, 1 background.
    pub layer: u8,
    /// `CellStageScale` index of the markers it places.
    pub scale: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Resource, Default)]
pub struct CellTiles {
    /// Loaded tiles and their startTile flag.
    pub loaded: HashMap<TileKey, bool>,
    /// (player scale index, world-scale target bits) the tiles were built for.
    pub level: Option<(i32, u32)>,
    /// The forced first step of the level has run.
    pub started: bool,
    pub generated: usize,
    pub next_flock: u32,
    /// Steps since the last on-screen census (`OPENSPORE_CELL_DEBUG`).
    pub census_tick: u32,
}

/// `TileFactor(s)` (0xE51FD0) by d = s - p.
pub fn tile_factor(s: i32, p: i32) -> f32 {
    match s - p {
        d if d <= -1 => 1.0,
        0 => 0.5,
        1 => 0.25,
        2 => 0.125,
        3 => 0.0625,
        _ => 0.031_25,
    }
}

/// `TileSize(s)` in the `abs` frame (world units * world scale).
pub fn tile_size_abs(s: i32, p: i32) -> f32 {
    real_size(CellScale(s)) * tile_factor(s, p) * 0.85
}

/// Expected placements per tile and marker: 2.25 * (0.85 * TF)^2 * Count.
pub fn expected_per_tile(s: i32, p: i32, count: f32) -> f32 {
    let f = 0.85 * tile_factor(s, p);
    2.25 * f * f * count
}


/// `sphere radius` of the on-screen test of 0xE5CDE0: size (+0x58) x 1.7 (.rdata 0x13F9524).
pub const ON_SCREEN_RADIUS: f32 = 1.7;

/// 0xE5CDE0 for a foreground cell, negated (VERIFIED): the cell is on screen when the
/// sphere (centre = its position, radius = `size` x 1.7) touches `sVisibleBackgroundBBox`
/// (0xE507F0: the look point +-(10, 7.5), z range [0, 0]; a foreground cell's elevation
/// is 0, so the test is the circle against the box). `look`, `pos` and `size` are in
/// retail world units (the avatar and the mate are always on screen: 0xE5CDE0 returns
/// "not off screen" for them). 0xE78C00 asks it for every cell every step: a visible
/// cell without a GFX object gets one (0xE6D8F0: the outline pins are allocated there), a
/// cell that is not visible loses it and its pins (0xE66010).
pub fn pin_visible(look: Vec2, pos: Vec2, size: f32) -> bool {
    AbsBox::around(look, VISIBLE_BOX_HALF).touches(pos, size * ON_SCREEN_RADIUS)
}

/// Wanted tiles (0xE5FA80): layer 0 at p-1 ..= p+2, layer 1 at p ..= p+4, every tile
/// of the layer's region (`regions[layer]`) at that scale (0xE5F610).
pub fn wanted_tiles(regions: &[AbsBox; 2], p: i32, has_background: bool) -> Vec<TileKey> {
    let mut wanted: Vec<TileKey> = Vec::new();
    for (layer, scales) in [(0u8, (p - 1)..=(p + 2)), (1u8, p..=(p + 4))] {
        if layer == 1 && !has_background {
            continue;
        }
        let region = regions[layer as usize];
        for s in scales {
            if !(0..=CellScale::MAX).contains(&s) {
                continue;
            }
            let ts = tile_size_abs(s, p);
            let (x0, x1) = ((region.min.x / ts).floor() as i64, (region.max.x / ts).ceil() as i64);
            let (y0, y1) = ((region.min.y / ts).floor() as i64, (region.max.y / ts).ceil() as i64);
            if (x1 - x0) * (y1 - y0) > MAX_TILES {
                continue;
            }
            for x in x0..x1 {
                for y in y0..y1 {
                    wanted.push(TileKey {
                        layer,
                        scale: s,
                        x: x as i32,
                        y: y as i32,
                    });
                }
            }
        }
    }
    wanted
}

/// A plant child (0xE78830 -> 0xE76D70 with flag 1, anchor 0xE61370; VERIFIED, `notes\cell-pop2-notes.md`
/// 5): the anchor is `parentPos + R_parent (cos 2 PI t, sin 2 PI t, 0) x 1.5 x parentSize`, i.e. the direction
/// of the parent's local +X axis turned counter-clockwise by `t` turns (0x453FF0 builds the matrix with the
/// row `(cos, sin, 0)`, the rows of a matrix are the local axes). The child is created looking AT the parent
/// (0x69B440: local +Y = the unit vector to the parent, X = f x up, up (0, 0, 1)) and then turned by a quarter
/// circle about +z (0xE76EE9..: the Hamilton product `q_look x q_z(pi/2)`, .data 0x15A7C1C = pi/4 doubled), so
/// its local -Y (the facing, 0xE5EB20) is the anchor direction turned counter-clockwise by 90 degrees and its
/// local +X axis points back at the parent. Returns the child's position (same frame as `parent_at`) and its
/// `CellObject::heading` (facing (-sin h, -cos h); a counter-clockwise turn by `a` lowers the heading by `a`).
/// `parent_heading` is the parent's actual orientation (identity for a fixed-orientation record).
pub fn plant_child(parent_at: Vec2, parent_heading: f32, parent_size: f32, turn: f32) -> (Vec2, f32) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    // The parent's +X axis has the heading `h - pi/2`.
    let anchor = parent_heading - FRAC_PI_2 - turn * TAU;
    let at = parent_at + crate::cell_engine::heading_dir(anchor) * (parent_size * PLANT_ATTACH);
    (at, anchor - FRAC_PI_2)
}

/// `0xE5F8C0`: is a cell of scale `s` standing at `at` (`abs` frame) released?
/// Its tile is `floor(at / TileSize(s))` with the tile size of its own scale
/// (`TileSize = realSize[s] / worldScale x TileFactor(s) x 0.85`, so `tile_size_abs`
/// in the `abs` frame); it is kept when the scale is in `p-1 ..= p+5` and the tile
/// lies in `[floor(min / ts), ceil(max / ts))` on both axes (0xE5F810 with the
/// layer's region from 0xE5F610). A cell below `p-1` is not released here
/// (it shrinks out).
pub fn released(s: i32, p: i32, at: Vec2, region: &AbsBox) -> bool {
    if s + 1 < p {
        return false;
    }
    if s > p + 5 {
        return true;
    }
    let ts = tile_size_abs(s, p);
    let (x0, x1) = ((region.min.x / ts).floor(), (region.max.x / ts).ceil());
    let (y0, y1) = ((region.min.y / ts).floor(), (region.max.y / ts).ceil());
    let (tx, ty) = ((at.x / ts).floor(), (at.y / ts).floor());
    !(tx >= x0 && tx < x1 && ty >= y0 && ty < y1)
}

/// An axis-aligned box in the `abs` frame.
#[derive(Clone, Copy, Debug)]
pub struct AbsBox {
    pub min: Vec2,
    pub max: Vec2,
}

impl AbsBox {
    pub fn around(center: Vec2, half: Vec2) -> Self {
        Self {
            min: center - half,
            max: center + half,
        }
    }
    pub fn grown(self, factor: f32) -> Self {
        let c = (self.min + self.max) * 0.5;
        Self::around(c, (self.max - self.min) * 0.5 * factor)
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
    /// A circle touches the box.
    pub fn touches(&self, p: Vec2, r: f32) -> bool {
        p.clamp(self.min, self.max).distance(p) <= r
    }
}

/// Everything one step's placements need.
struct Placer<'a> {
    db: &'a CellDatabase,
    game: &'a CellGame,
    grid: &'a CellGrid,
    view: View,
    force: bool,
    exclusion: Option<(Vec2, f32, f32)>,
    placed: Vec<(Vec2, f32)>,
    out: Vec<SpawnRequest>,
    next_flock: u32,
    max_radius: f32,
}

impl Placer<'_> {
    /// Rejected placement: visible (unless forced), overlapping (foreground),
    /// or inside the level-start exclusion radii.
    fn rejected(&self, layer: u8, at_abs: Vec2, r_abs: f32, depth: f32, creature: bool) -> bool {
        if !self.force && self.view.touches(layer, at_abs, r_abs, depth) {
            return true;
        }
        if let Some((start, nothing, no_creature)) = self.exclusion {
            let d = at_abs.distance(start);
            if d < nothing || (creature && d < no_creature) {
                return true;
            }
        }
        if layer == 0 {
            let at = self.game.to_world(at_abs);
            let r = self.game.world_radius(r_abs);
            let mut hit = false;
            self.grid.query(at, r + self.max_radius, |item| {
                if item.position.distance(at) < r + item.radius {
                    hit = true;
                }
            });
            if hit || self.placed.iter().any(|(p, pr)| p.distance(at) < r + pr) {
                return true;
            }
        }
        false
    }

    /// Queue one object; `r_abs` is its placement radius, `size` an explicit
    /// size (cluster members).
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &mut self,
        key: TileKey,
        cell: u32,
        at_abs: Vec2,
        r_abs: f32,
        size: Option<f32>,
        depth: f32,
        rng: &mut impl Rng,
        flock: Option<(u32, bool)>,
    ) {
        let at = self.game.to_world(at_abs);
        if key.layer == 0 {
            self.placed.push((at, self.game.world_radius(r_abs)));
        }
        self.out.push(SpawnRequest {
            cell,
            scale: CellScale(key.scale),
            position: at,
            depth,
            heading: rng.random_range(0.0..std::f32::consts::TAU),
            size_roll: rng.random_range(0.0..1.0),
            size,
            tile: Some(key),
            role: SpawnRole::World,
            flock,
            ..default()
        });
    }

    fn depth(&self, key: TileKey, marker: &CellMarker, rng: &mut impl Rng) -> f32 {
        let z = if marker.z_offset_max > marker.z_offset {
            rng.random_range(marker.z_offset..=marker.z_offset_max)
        } else {
            marker.z_offset
        };
        if key.layer == 0 {
            return 0.0;
        }
        let base = self.db.globals.as_ref().map_or(35.0, |g| g.background_distance);
        (base - z).max(2.0) * UNIT * BG_DEPTH_SCALE
    }

    /// Tile populate (0xE7C6C0) for one level entry's populate.
    fn place_tile(&mut self, key: TileKey, start_tile: bool, p: i32, difficulty: usize) {
        let world = if key.layer == 0 {
            self.game.world
        } else {
            self.game.world_background
        };
        let Some(world) = self.db.worlds.get(&world) else {
            return;
        };
        let ts = tile_size_abs(key.scale, p);
        let origin = Vec2::new(key.x as f32, key.y as f32) * ts;
        let db = self.db;
        for (li, entry) in world.levels.iter().enumerate() {
            if entry.start_tile != start_tile || !(entry.player_size.is_none() || entry.player_size.index() == p) {
                continue;
            }
            let Some(populate) = db.populates.get(&entry.populate) else {
                continue;
            };
            for (mi, marker) in populate.markers.iter().enumerate() {
                if marker.size.index() != key.scale {
                    continue;
                }
                let Some(count) = marker.count_for(difficulty) else {
                    continue;
                };
                let mut rng = keyed_rng(&[
                    self.game.seed,
                    key.layer as u64,
                    key.scale as u64,
                    key.x as u32 as u64,
                    key.y as u32 as u64,
                    li as u64,
                    mi as u64,
                ]);
                let n = expected_per_tile(key.scale, p, count);
                let k = n.floor() as u32 + rng.random_bool(n.fract().clamp(0.0, 1.0) as f64) as u32;
                for _ in 0..k {
                    match marker.kind {
                        MarkerType::Plant => self.place_plant(key, marker, origin, ts, p, &mut rng),
                        MarkerType::Distribute => {
                            let cell = marker.distribute_cell;
                            let Some(record) = db.cell(cell) else {
                                continue;
                            };
                            // distribute (0xE75900): r = realSize[s] * size[1] / 30.
                            let r_abs = real_size(CellScale(key.scale)) * record.size[1] / 30.0;
                            let creature = crate::cell_engine::classify(record).is_creature();
                            let depth = self.depth(key, marker, &mut rng);
                            for _ in 0..PLACE_TRIES {
                                let at = origin + Vec2::new(rng.random_range(0.0..ts), rng.random_range(0.0..ts));
                                if !self.rejected(key.layer, at, r_abs, depth, creature) {
                                    self.spawn(key, cell, at, r_abs, None, depth, &mut rng, None);
                                    break;
                                }
                            }
                        }
                        MarkerType::Cluster => self.place_cluster(key, marker, origin, ts, &mut rng),
                        MarkerType::Encounter => {}
                    }
                }
            }
        }
    }

    /// plant (0xE78830, RE 11.4): a root at a free point (spacing
    /// size[1] of `cell_flora_clover_L2` * realSize[s] / 15, main pool only),
    /// children one level smaller when the player is at most the root's level,
    /// grandchildren two levels smaller when the player is smaller still. A
    /// child is anchored and turned as `plant_child` says (0xE61370 / 0xE76D70:
    /// `turns` x 2pi counter-clockwise from the parent's local +X axis,
    /// `PLANT_ATTACH` (1.5, .rdata 0x13EED94) parent sizes from its centre,
    /// facing the anchor direction + 90 degrees, size factor 1). No installed
    /// marker uses plant types 5 / 6; they grow as clover.
    fn place_plant(&mut self, key: TileKey, marker: &CellMarker, origin: Vec2, ts: f32, p: i32, rng: &mut impl Rng) {
        use crate::world_objects::name_id;
        let s = key.scale;
        let db = self.db;
        let (root, child, grand, child_turns, grand_turns, vine): (u32, u32, u32, Vec<f32>, Vec<f32>, bool) =
            match marker.plant_type {
                PlantType::Type2 => (
                    name_id("cell_flora_clover2_L2"),
                    name_id("cell_flora_clover2_L1"),
                    name_id("cell_flora_clover2_L0"),
                    (0..7).map(|k| k as f32 / 7.0).collect(),
                    vec![0.4, 0.5, 0.6],
                    false,
                ),
                PlantType::Type3 => (
                    name_id("cell_flora_clover3_l2"),
                    name_id("cell_flora_clover3_l1"),
                    name_id("cell_flora_clover3_l0"),
                    (1..=5).map(|k| k as f32 * 0.2).collect(),
                    vec![0.3, 0.4, 0.5, 0.6, 0.7],
                    false,
                ),
                PlantType::Type4 => (
                    name_id("cell_flora_clover2_L1"),
                    name_id("cell_flora_clover2_L1"),
                    0,
                    vec![],
                    vec![],
                    true,
                ),
                _ => (
                    name_id("cell_flora_clover_L2"),
                    name_id("cell_flora_clover_L1"),
                    name_id("cell_flora_clover_L0"),
                    (0..10).map(|k| k as f32 * 0.1).collect(),
                    vec![0.3, 0.4, 0.5, 0.6, 0.7],
                    false,
                ),
            };
        let Some(root_record) = db.cell(root) else {
            return;
        };
        let spacing_size = db.cell(name_id("cell_flora_clover_L2")).map_or(1.0, |r| r.size[1]);
        let spacing = real_size(CellScale(s)) * spacing_size / 15.0;
        let roll = |rng: &mut dyn rand::RngCore, record: &crate::cell_data::CellRecord, scale: i32| {
            let hi = if record.size[1] > 0.0 { record.size[1] } else { record.size[0] };
            let u = if hi > record.size[0] { rng.random_range(record.size[0]..=hi) } else { record.size[0] };
            real_size(CellScale(scale)) / 30.0 * u
        };
        let root_size = roll(&mut *rng, root_record, s);
        let depth = self.depth(key, marker, rng);
        let mut at = None;
        for _ in 0..PLACE_TRIES {
            let point = origin + Vec2::new(rng.random_range(0.0..ts), rng.random_range(0.0..ts));
            let visible = !self.force && self.view.touches(key.layer, point, root_size, depth);
            let crowded = key.layer == 0 && self.rejected(key.layer, point, spacing.max(root_size), depth, false);
            if !visible && !crowded {
                at = Some(point);
                break;
            }
        }
        let Some(at) = at else {
            return;
        };
        let heading = rng.random_range(0.0..std::f32::consts::TAU);
        self.spawn_plant(key, root, s, at, root_size, heading, depth);
        if p > s || s < 1 {
            return;
        }
        // A fixed-orientation record keeps the identity matrix (CreateCellObject), whatever
        // heading was requested: the anchors read the cell's actual matrix.
        let actual = |record: &crate::cell_data::CellRecord, heading: f32| if record.fixed_orientation { 0.0 } else { heading };
        if vine {
            // vine: 8 links, each at turn 0.75 + 0.1 * clamp(2U - 1) of the
            // previous one; the chain continues from the new link only when
            // the player is smaller.
            let (mut from, mut from_size, mut from_heading) = (at, root_size, actual(root_record, heading));
            for _ in 0..8 {
                let Some(record) = db.cell(child) else {
                    break;
                };
                let size = roll(&mut *rng, record, s);
                let jitter = (2.0 * rng.random_range(0.0f32..1.0) - 1.0).clamp(-1.0, 1.0);
                let (pos, h) = plant_child(from, from_heading, from_size, 0.75 + 0.1 * jitter);
                self.spawn_plant(key, child, s, pos, size, h, depth);
                if p < s {
                    (from, from_size, from_heading) = (pos, size, actual(record, h));
                }
            }
            return;
        }
        for turn in child_turns {
            let Some(record) = db.cell(child) else {
                break;
            };
            let size = roll(&mut *rng, record, s - 1);
            let (pos, h) = plant_child(at, actual(root_record, heading), root_size, turn);
            self.spawn_plant(key, child, s - 1, pos, size, h, depth);
            if p >= s || s < 2 {
                continue;
            }
            for g in &grand_turns {
                let Some(grand_record) = db.cell(grand) else {
                    break;
                };
                let gsize = roll(&mut *rng, grand_record, s - 2);
                let (gpos, gh) = plant_child(pos, actual(record, h), size, *g);
                self.spawn_plant(key, grand, s - 2, gpos, gsize, gh, depth);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_plant(&mut self, key: TileKey, cell: u32, scale: i32, at_abs: Vec2, size: f32, heading: f32, depth: f32) {
        let at = self.game.to_world(at_abs);
        if key.layer == 0 {
            self.placed.push((at, self.game.world_radius(size)));
        }
        self.out.push(SpawnRequest {
            cell,
            scale: CellScale(scale),
            position: at,
            depth,
            heading,
            size: Some(size),
            tile: Some(key),
            role: SpawnRole::World,
            ..default()
        });
    }

    /// cluster (0xE75350): a flock of `parts` members, the first at a
    /// random point, the rest touching a random previous member (or in a
    /// straight chain when `linear`), all inside the tile.
    fn place_cluster(&mut self, key: TileKey, marker: &CellMarker, origin: Vec2, ts: f32, rng: &mut impl Rng) {
        let cell = marker.cluster_cell;
        let Some(record) = self.db.cell(cell) else {
            return;
        };
        let size = real_size(CellScale(key.scale)) / 30.0 * record.size[1];
        let creature = crate::cell_engine::classify(record).is_creature();
        let depth = self.depth(key, marker, rng);
        let tile = AbsBox {
            min: origin,
            max: origin + Vec2::splat(ts),
        };
        let mut first = None;
        for _ in 0..PLACE_TRIES {
            let at = origin + Vec2::new(rng.random_range(0.0..ts), rng.random_range(0.0..ts));
            if !self.rejected(key.layer, at, size, depth, creature) {
                first = Some(at);
                break;
            }
        }
        let Some(first) = first else {
            return;
        };
        self.next_flock += 1;
        let flock = self.next_flock;
        let mut members = vec![first];
        self.spawn(key, cell, first, size, Some(size), depth, rng, Some((flock, true)));
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let line = Vec2::new(a.cos(), a.sin());
        let gap = size * 1.0001 + size;
        for _ in 1..marker.parts.max(1) {
            let next = if marker.linear != 0 {
                members[members.len() - 1] + line * gap
            } else {
                let from = members[rng.random_range(0..members.len())];
                let b = rng.random_range(0.0..std::f32::consts::TAU);
                from + Vec2::new(b.cos(), b.sin()) * gap
            };
            if !tile.contains(next) {
                continue;
            }
            members.push(next);
            self.spawn(key, cell, next, size, Some(size), depth, rng, Some((flock, false)));
        }
    }
}

/// The retail visible box (`sVisibleBackgroundBBox`, 0xE507F0): the look
/// point +-(10, 7.5) world units, `abs` frame.
pub fn visible_box(game: &CellGame) -> AbsBox {
    AbsBox::around(game.to_abs(game.camera_look), VISIBLE_BOX_HALF * game.world_scale)
}

/// Layer-1 tile region scale (0xE507F0): 1 + backgroundDistance / 15.
pub fn background_region_factor(background_distance: f32) -> f32 {
    1.0 + background_distance / 15.0
}

/// What the game treats as on screen, `abs` frame.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Layer 0: the visible box.
    pub visible: AbsBox,
    /// Layer 1: the background viewer's frustum. `look` is its axis on the
    /// plane; `slope` the cross-section half extents per Bevy unit of distance
    /// below the unzoomed camera, in `abs` units.
    pub look: Vec2,
    pub slope: Vec2,
}

impl View {
    pub fn new(game: &CellGame, aspect: f32) -> Self {
        // Same FOV as the main viewer (0x7C5400(50) in both 0xE600A0 and
        // 0xE549B0): the Bevy view footprint per unit of distance.
        let tan_v = std::f32::consts::FRAC_PI_8.tan();
        Self {
            visible: visible_box(game),
            look: game.to_abs(game.camera_look),
            slope: Vec2::new(tan_v * aspect, tan_v) / UNIT * game.world_scale,
        }
    }
    /// The sphere (`at`, `r`) of a cell of that layer (and zoom-1 depth below
    /// the plane, Bevy units) is on screen (0xE5D410 / 0xE5CDE0).
    pub fn touches(&self, layer: u8, at: Vec2, r: f32, depth: f32) -> bool {
        if layer == 0 {
            return self.visible.touches(at, r);
        }
        let half = self.slope * (CAMERA_HEIGHT + depth.max(0.0));
        AbsBox::around(self.look, half).touches(at, r)
    }
}

/// Every step: level changes, wanted tiles, new tiles, objects leaving.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn stream(
    mut commands: Commands,
    db: Res<CellDatabase>,
    run: Res<RunProgress>,
    grid: Res<CellGrid>,
    mut game: ResMut<CellGame>,
    mut tiles: ResMut<CellTiles>,
    mut flow: ResMut<crate::cell_flow::CellFlow>,
    mut queue: ResMut<CellSpawnQueue>,
    part_scene: Option<Res<crate::cell_part_guide::PartGuideScene>>,
    windows: Query<&Window>,
    player: Query<&Transform, With<Player>>,
    mut objects: Query<
        (Entity, &Transform, &mut CellObject, Has<CellMateObject>, Has<crate::cell_flow::CellEgg>),
        Without<Player>,
    >,
) {
    let Some(globals) = db.globals.as_ref() else {
        return;
    };
    let Ok(pt) = player.single() else {
        return;
    };
    let food = run.cell.food_progression;
    let p = crate::cell_engine::player_scale(food).index();
    let level = (p, world_scale_for(food).to_bits());
    let pickup = game.unlock_pickup;
    // `+0x354`: the pickup, the mate, eggs and the Pokey scene's two cells (0xE7C3BB).
    let protected = game.protected.clone();
    let special = move |e: Entity, mate: bool, egg: bool| mate || egg || pickup == Some(e) || protected.contains(&e);
    // Death: the level is re-initialised (the food progression is kept).
    if flow.reset_level {
        flow.reset_level = false;
        for (e, _, _, mate, egg) in &objects {
            if !special(e, mate, egg) {
                commands.entity(e).despawn();
            }
        }
        tiles.loaded.clear();
        tiles.started = false;
        game.start_abs = Some(game.to_abs(Vec2::new(pt.translation.x, pt.translation.z)));
    }
    let aspect = windows
        .iter()
        .next()
        .map(|w| w.width() / w.height().max(1.0))
        .filter(|a| a.is_finite() && *a > 0.1)
        .unwrap_or(16.0 / 9.0);
    let view = View::new(&game, aspect);
    // 0xE5CDE0 (RE 11.1): a cell is on screen when its sphere (size * 1.7)
    // touches the visible box (background: the background frustum).
    let (world_scale, origin_abs) = (game.world_scale, game.origin_abs);
    let on_screen = move |o: &CellObject, t: &Transform| {
        let at = origin_abs + Vec2::new(t.translation.x, t.translation.z) / UNIT * world_scale;
        let r = o.radius / UNIT * world_scale * 1.7;
        view.touches(o.background as u8, at, r, o.depth)
    };
    // Level change (0xE7CD10): every off-screen cell with a scale goes (the
    // mate and the avatar stay), then every tile re-populates.
    if tiles.level != Some(level) {
        if tiles.level.is_some() {
            for (e, t, o, mate, egg) in &objects {
                if !mate && !egg && !o.scale.is_none() && !on_screen(&o, t) {
                    commands.entity(e).despawn();
                }
            }
        }
        tiles.loaded.clear();
        tiles.level = Some(level);
    }
    let force = !tiles.started;
    tiles.started = true;
    // 0xE5F610: layer 0 = the visible box +- its own width / height,
    // layer 1 = the box x (1 + backgroundDistance / 15) (0xE507F0).
    let regions = [
        view.visible.grown(2.0),
        view.visible.grown(background_region_factor(globals.background_distance)),
    ];
    let wanted = wanted_tiles(&regions, p, game.world_background != 0);
    // Delete tiles no longer wanted.
    let wanted_set: std::collections::HashSet<TileKey> = wanted.iter().copied().collect();
    tiles.loaded.retain(|k, _| wanted_set.contains(k));
    // Create the missing ones (nearest first).
    let center = game.to_abs(game.camera_look);
    let mut missing: Vec<TileKey> = wanted.into_iter().filter(|k| !tiles.loaded.contains_key(k)).collect();
    missing.sort_by(|a, b| {
        let da = (Vec2::new(a.x as f32 + 0.5, a.y as f32 + 0.5) * tile_size_abs(a.scale, p)).distance(center);
        let db_ = (Vec2::new(b.x as f32 + 0.5, b.y as f32 + 0.5) * tile_size_abs(b.scale, p)).distance(center);
        da.total_cmp(&db_)
    });
    if !force {
        missing.truncate(TILES_PER_STEP);
    }
    if !missing.is_empty() {
        let exclusion = game.start_abs.filter(|_| force).map(|s| {
            (
                s,
                globals.start_level_no_anything_radius * game.world_scale,
                globals.start_level_no_creature_radius * game.world_scale,
            )
        });
        let max_radius = grid.items.iter().map(|i| i.radius).fold(0.0, f32::max);
        let mut placer = Placer {
            db: &db,
            game: &game,
            grid: &grid,
            view,
            force,
            exclusion,
            placed: Vec::new(),
            out: Vec::new(),
            next_flock: tiles.next_flock,
            max_radius,
        };
        for key in &missing {
            // New layer-0 tiles get startTile = force.
            let start_tile = force && key.layer == 0;
            placer.place_tile(*key, start_tile, p, game.difficulty);
        }
        if std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
            eprintln!(
                "cell pop: p {p} W {:.3} force {force} missing {} out {} loaded {} view {:?}",
                game.world_scale,
                missing.len(),
                placer.out.len(),
                tiles.loaded.len(),
                view.visible
            );
        }
        let (out, next_flock) = (placer.out, placer.next_flock);
        for key in missing {
            tiles.loaded.insert(key, force && key.layer == 0);
        }
        tiles.generated += out.len();
        tiles.next_flock = next_flock;
        // `cCellGame+0x518C` (the part scene runs): the spawn routine 0xE771D0 refuses every
        // cell but the scene's own, so the tiles load empty.
        if !part_scene.as_ref().is_some_and(|s| s.active) {
            queue.0.extend(out);
        }
    }
    if force {
        game.start_abs = None;
    }
    // Debug census of the background giants on screen (comparison with the
    // retail frames): every 120 steps with `OPENSPORE_CELL_DEBUG`.
    tiles.census_tick += 1;
    if tiles.census_tick >= 120 && std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
        tiles.census_tick = 0;
        let mut seen: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for (_, t, o, _, _) in &objects {
            if !o.background {
                continue;
            }
            let at = game.to_abs(Vec2::new(t.translation.x, t.translation.z));
            let r = o.radius / UNIT * game.world_scale;
            let name = db.cell(o.cell).map_or(format!("{:08X}", o.cell), |c| c.name.clone());
            let e = seen.entry(name).or_default();
            e.0 += 1;
            e.1 += view.touches(1, at, r, o.depth) as usize;
        }
        eprintln!("cell bg census: p {p} W {:.3} zoom {:.2} (alive, on screen) {seen:?}", game.world_scale, game.zoom_factor);
    }
    // Per cell (0xE7C8C0 / 0xE78C00, RE 11.1): background cells smaller than
    // the player shrink out; off-screen poison clouds and pickups of parts
    // already unlocked go; anything that shrank below 2^-16 dies (0xE7BFB0).
    // 0xE7C080 -> 0xE5F8C0: a cell whose own-scale tile is no longer wanted
    // leaves the world (`released`).
    for (e, t, mut o, mate, egg) in &mut objects {
        if o.background && !o.scale.is_none() && o.scale.index() < p {
            o.abs_size = 0.0;
        }
        if special(e, mate, egg) {
            continue;
        }
        let at = game.to_abs(Vec2::new(t.translation.x, t.translation.z));
        let layer = o.background as usize;
        let tiny = o.radius / UNIT < crate::cell_engine::SIZE_EPSILON;
        let stale_cloud = o.kind == crate::cell_engine::CellKind::Poison && !on_screen(&o, t);
        let owned_part = matches!(o.kind, crate::cell_engine::CellKind::Unlock | crate::cell_engine::CellKind::Fragment)
            && db.cell(o.cell).is_some_and(|r| run.cell.unlocked(r.unlock_type));
        if (!o.scale.is_none() && released(o.scale.index(), p, at, &regions[layer])) || tiny || stale_cloud || owned_part {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clover_roots_grow_children_by_player_level() {
        use crate::cell_data::{CellRecord, CellType};
        use crate::world_objects::name_id;
        let mut db = CellDatabase::default();
        for name in ["cell_flora_clover_L2", "cell_flora_clover_L1", "cell_flora_clover_L0"] {
            let mut r = CellRecord::default();
            r.cell_type = CellType::Liquid;
            r.size = [0.2, 0.3];
            db.cells.insert(name_id(name), r);
        }
        let game = CellGame::default();
        let grid = CellGrid::default();
        let far = AbsBox::around(Vec2::splat(1e6), Vec2::ONE);
        let mut placer = Placer {
            db: &db,
            game: &game,
            grid: &grid,
            view: View {
                visible: far,
                look: Vec2::splat(1e6),
                slope: Vec2::ZERO,
            },
            force: true,
            exclusion: None,
            placed: Vec::new(),
            out: Vec::new(),
            next_flock: 0,
            max_radius: 0.0,
        };
        let marker = CellMarker {
            kind: MarkerType::Plant,
            plant_type: PlantType::Type1,
            ..default()
        };
        let key = TileKey {
            layer: 0,
            scale: 3,
            x: 0,
            y: 0,
        };
        let mut rng = keyed_rng(&[7]);
        // Player at the root's level: root + 10 children (clover).
        placer.place_plant(key, &marker, Vec2::ZERO, 1000.0, 3, &mut rng);
        assert_eq!(placer.out.len(), 11);
        assert_eq!(placer.out[1].scale, CellScale(2));
        // Smaller player: + 5 grandchildren per child.
        placer.out.clear();
        placer.place_plant(key, &marker, Vec2::new(5000.0, 0.0), 1000.0, 2, &mut rng);
        assert_eq!(placer.out.len(), 1 + 10 + 50);
        // Bigger player: the root alone.
        placer.out.clear();
        placer.place_plant(key, &marker, Vec2::new(-5000.0, 0.0), 1000.0, 4, &mut rng);
        assert_eq!(placer.out.len(), 1);
    }

    #[test]
    fn tiles_follow_the_retail_factors() {
        assert_eq!(tile_factor(0, 1), 1.0);
        assert_eq!(tile_factor(1, 1), 0.5);
        assert_eq!(tile_factor(3, 1), 0.125);
        assert_eq!(tile_factor(9, 1), 0.031_25);
        // TileSize(p) = realSize[p] * 0.5 * 0.85 (abs frame).
        assert!((tile_size_abs(1, 1) - 30.0 * 0.5 * 0.85).abs() < 1e-4);
        // n = 2.25 * (0.85 * TF)^2 * Count.
        assert!((expected_per_tile(0, 1, 4.0) - 2.25 * 0.7225 * 4.0).abs() < 1e-4);
        assert!((expected_per_tile(1, 1, 1.0) - 2.25 * 0.180_625).abs() < 1e-4);
    }

    #[test]
    fn cells_are_released_with_the_tile_of_their_own_scale() {
        // p = 2: scale 2 tiles are realSize[2] * 0.5 * 0.85 = 42.5 wide (abs frame).
        let region = AbsBox::around(Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert!(!released(2, 2, Vec2::new(99.0, 0.0), &region));
        // The region rounds out to whole tiles: x in [floor(-100/42.5), ceil(100/42.5)) = [-3, 3).
        assert!(!released(2, 2, Vec2::new(127.0, 0.0), &region), "tile 2 is still wanted");
        assert!(released(2, 2, Vec2::new(128.0, 0.0), &region), "tile 3 is not");
        assert!(released(2, 2, Vec2::new(-130.0, 0.0), &region), "tile -4 is not");
        // A bigger scale has other tiles (factor .25 at +1: 300 * .25 * .85 = 63.75):
        // tiles -2..2 are wanted, i.e. x in [-127.5, 127.5).
        assert!(!released(3, 2, Vec2::new(120.0, 0.0), &region));
        assert!(released(3, 2, Vec2::new(130.0, 0.0), &region));
        // Too large a scale goes; one level or more below the player never does (it shrinks).
        assert!(released(8, 2, Vec2::ZERO, &region));
        assert!(!released(0, 2, Vec2::new(1e6, 0.0), &region));
        assert!(released(1, 2, Vec2::new(1e6, 0.0), &region), "p-1 is the lowest scale that is released by tile");
    }

    #[test]
    fn pins_follow_the_visible_box_with_a_sphere_of_one_point_seven_sizes() {
        // 0xE5CDE0: the look point +-(10, 7.5), the sphere radius is size x 1.7.
        let look = Vec2::new(100.0, -50.0);
        assert!(pin_visible(look, look, 0.01));
        // Touching from the side: size 1 reaches 1.7 past the box edge.
        assert!(pin_visible(look, look + Vec2::new(11.6, 0.0), 1.0));
        assert!(!pin_visible(look, look + Vec2::new(11.8, 0.0), 1.0));
        // A larger cell reaches further (a rock of size 5 from 8.5 away).
        assert!(pin_visible(look, look + Vec2::new(18.4, 0.0), 5.0));
        assert!(!pin_visible(look, look + Vec2::new(18.6, 0.0), 5.0));
        // The corner is a distance, not a box: (10 + a, 7.5 + a) is sqrt(2) a away.
        assert!(pin_visible(look, look + Vec2::new(11.0, 8.5), 1.0));
        assert!(!pin_visible(look, look + Vec2::new(12.0, 9.5), 1.0));
        // The population region (2 x 2 boxes) is much larger than what is on screen.
        assert!(!pin_visible(look, look + Vec2::new(0.0, 12.0), 0.3));
    }

    /// The pins the forced first step of a level makes (`stream`'s level start, layer 0) for the
    /// player at `food`: (cells, pin-eligible cells, pins of every eligible cell = what this port
    /// allocated before 0xE78C00 was decoded, pins of the cells on screen = retail).
    fn level_start_census(
        db: &CellDatabase,
        food: i32,
        seed: u64,
        sources: &mut std::collections::BTreeMap<String, (usize, usize)>,
    ) -> (usize, usize, usize, usize) {
        use crate::cell_engine::{object_abs_size, player_scale};
        use crate::cell_softbody::{has_outline, level_for_size, pin_count};
        let globals = db.globals.as_ref().expect("globals");
        let p = player_scale(food).index();
        let world_scale = world_scale_for(food);
        let game = CellGame {
            world: globals.worlds[0],
            world_background: globals.world_backgrounds[0],
            difficulty: 1,
            world_scale,
            world_scale_target: world_scale,
            seed,
            ..default()
        };
        let view = View::new(&game, 16.0 / 9.0);
        let regions = [
            view.visible.grown(2.0),
            view.visible.grown(background_region_factor(globals.background_distance)),
        ];
        let grid = CellGrid::default();
        // The hatch exclusion radii only exist at the very start of a new stage (food 0).
        let exclusion = (food == 0).then(|| {
            (
                Vec2::ZERO,
                globals.start_level_no_anything_radius * world_scale,
                globals.start_level_no_creature_radius * world_scale,
            )
        });
        let mut placer = Placer {
            db,
            game: &game,
            grid: &grid,
            view,
            force: true,
            exclusion,
            placed: Vec::new(),
            out: Vec::new(),
            next_flock: 0,
            max_radius: 0.0,
        };
        for key in wanted_tiles(&regions, p, true) {
            if key.layer == 0 {
                placer.place_tile(key, true, p, game.difficulty);
            }
        }
        let (mut cells, mut eligible, mut all, mut seen) = (0, 0, 0, 0);
        for r in &placer.out {
            let record = db.cell(r.cell).expect("record");
            cells += 1;
            let roll = record.size[0] + (record.size[1] - record.size[0]) * r.size_roll;
            let abs = r.size.unwrap_or_else(|| object_abs_size(r.scale, roll, r.size_factor, world_scale));
            let size = abs / world_scale;
            let level = if has_outline(record) { level_for_size(size) } else { 0 };
            if level == 0 {
                continue;
            }
            eligible += 1;
            all += pin_count(level);
            let name = if record.name.is_empty() { format!("{:08x}", r.cell) } else { record.name.clone() };
            let source = sources.entry(format!("{name} scale {} level {level}", r.scale.index())).or_default();
            (source.0, source.1) = (source.0 + 1, source.1 + pin_count(level));
            if pin_visible(Vec2::ZERO, r.position / UNIT, size) {
                seen += pin_count(level);
            }
        }
        (cells, eligible, all, seen)
    }

    /// Pool pressure at every level start (the question of `cell-pop2`): the retail rules
    /// (population region = 2 x 2 visible boxes, pins only for cells on screen) keep the
    /// 1500-particle pool far from full for the ten player sizes, several game ids.
    #[test]
    #[ignore = "requires original Spore installation"]
    fn pinned_particles_at_every_level_start_stay_below_the_pool() {
        let install = crate::spore_install::discover().expect("original installation");
        let db = CellDatabase::load(&crate::asset_catalog::ResourceIndex::load(&install.packages));
        assert!(db.is_loaded());
        let mut worst = 0;
        for food in (0..1000).step_by(100) {
            let (mut max_all, mut max_seen, mut max_cells, mut max_eligible) = (0, 0, 0, 0);
            for seed in [0xCE11u64, 1, 2, 3, 4, 5] {
                let mut sources = std::collections::BTreeMap::new();
                let (cells, eligible, all, seen) = level_start_census(&db, food, seed, &mut sources);
                if seed == 0xCE11 {
                    let mut top: Vec<_> = sources.into_iter().collect();
                    top.sort_by_key(|(_, (_, pins))| std::cmp::Reverse(*pins));
                    top.truncate(4);
                    eprintln!("  food {food:3} game id 0xCE11, main sources (cells, pins in the whole region): {top:?}");
                }
                (max_all, max_seen) = (max_all.max(all), max_seen.max(seen));
                (max_cells, max_eligible) = (max_cells.max(cells), max_eligible.max(eligible));
            }
            eprintln!(
                "level start food {food:3} (scale {}): up to {max_cells} cells, {max_eligible} pin-eligible, {max_all} pins in the whole region, {max_seen} pins on screen",
                crate::cell_engine::player_scale(food).index()
            );
            assert!(max_seen < 1499 - 4, "food {food}: {max_seen} pins on screen would fill the pool");
            assert!(max_seen <= max_all);
            worst = worst.max(max_seen);
        }
        // Retail has no limit and no safety net (0xE5CB60 stores the -1 of a full pool); the
        // ordinary start is far below it (the old whole-region count was 1360-1496).
        assert!(worst < 1000, "{worst}");
    }

    #[test]
    fn plant_children_sit_on_the_parents_x_axis_turned_and_face_it_plus_a_quarter_turn() {
        use crate::cell_engine::heading_dir;
        let near = |a: Vec2, b: Vec2| (a - b).length() < 1e-4;
        // Parent at the origin, identity matrix (heading 0: facing (0, -1), local +X = (1, 0)), size 2.
        let (at, h) = plant_child(Vec2::ZERO, 0.0, 2.0, 0.0);
        assert!(near(at, Vec2::new(3.0, 0.0)), "turn 0: 1.5 sizes along +X, {at:?}");
        // Facing = the outward direction turned +90 degrees: (0, 1); the local +X axis (the facing turned
        // +90 degrees again) points back at the parent.
        assert!(near(heading_dir(h), Vec2::new(0.0, 1.0)), "{:?}", heading_dir(h));
        // A quarter turn counter-clockwise: the anchor is on +Y, the facing (-1, 0).
        let (at, h) = plant_child(Vec2::ZERO, 0.0, 2.0, 0.25);
        assert!(near(at, Vec2::new(0.0, 3.0)), "{at:?}");
        assert!(near(heading_dir(h), Vec2::new(-1.0, 0.0)), "{:?}", heading_dir(h));
        // The child's +X axis (facing turned +90) always points at its parent.
        for (heading, turn) in [(0.7f32, 0.0f32), (-2.1, 0.3), (3.0, 0.9)] {
            let parent = Vec2::new(4.0, -1.0);
            let (at, h) = plant_child(parent, heading, 1.5, turn);
            let facing = heading_dir(h);
            let x_axis = Vec2::new(-facing.y, facing.x);
            assert!(near(x_axis, (parent - at).normalize()), "heading {heading} turn {turn}");
            assert!(((at - parent).length() - 2.25).abs() < 1e-4);
        }
        // Grandchildren of a child fan around its outward direction (turn 0.5 = straight out).
        let (child_at, child_h) = plant_child(Vec2::ZERO, 0.3, 1.0, 0.1);
        let (grand_at, _) = plant_child(child_at, child_h, 0.5, 0.5);
        let out = child_at.normalize();
        assert!(near((grand_at - child_at).normalize(), out), "turn 0.5 of the child's frame is straight out");
    }

    #[test]
    fn boxes_grow_and_touch() {
        let b = AbsBox::around(Vec2::ZERO, Vec2::new(2.0, 1.0));
        assert!(b.touches(Vec2::new(2.5, 0.0), 0.6));
        assert!(!b.touches(Vec2::new(2.5, 0.0), 0.4));
        let g = b.grown(3.0);
        assert!(g.contains(Vec2::new(5.9, 2.9)) && !g.contains(Vec2::new(6.1, 0.0)));
    }

    #[test]
    fn retail_view_regions_and_background_frustum() {
        // 0xE507F0: the visible box is +-(10, 7.5) world units at any zoom;
        // the layer-1 region is that box x (1 + 35 / 15).
        let mut game = CellGame {
            world_scale: 5.0,
            ..default()
        };
        let a = View::new(&game, 16.0 / 9.0);
        game.zoom_factor = 0.5;
        let b = View::new(&game, 16.0 / 9.0);
        assert_eq!(a.visible.max, Vec2::new(50.0, 37.5));
        assert_eq!(b.visible.max, a.visible.max, "zoom does not change the box");
        assert!((background_region_factor(35.0) - 10.0 / 3.0).abs() < 1e-6);
        // Layer 1: the background frustum widens with depth (elevation 30 ->
        // depth 5 x BG_DEPTH_SCALE, elevation 0 -> 35 x BG_DEPTH_SCALE). At the
        // play plane its cross-section is the view (20 x 0.75 tan 25 deg).
        let plane_half = a.slope.y * CAMERA_HEIGHT / game.world_scale;
        assert!((plane_half - 20.0 * 0.75 * 25f32.to_radians().tan()).abs() < 0.01, "{plane_half}");
        let shallow = 5.0 * UNIT * BG_DEPTH_SCALE;
        let deep = 35.0 * UNIT * BG_DEPTH_SCALE;
        // Retail: the cross-section at elevation e is (55 - e) x 0.3497 high.
        let at = Vec2::new(0.0, 0.3497 * 40.0 * game.world_scale);
        assert!(!a.touches(1, at, 0.0, shallow), "elevation 30: 25 units from the eye");
        assert!(a.touches(1, at, 0.0, deep), "elevation 0: 55 units from the eye");
        assert!(!a.touches(0, at, 0.0, deep), "layer 0 uses the visible box");
    }

    #[test]
    fn background_depth_follows_the_unzoomed_background_viewer() {
        use crate::cell_engine::background_draw_depth;
        let d = 10.0;
        assert_eq!(background_draw_depth(d, 1.0), d);
        // Zoomed in to 0.5: the camera came down by half its height, the
        // background stays as far from the camera as before.
        assert!((background_draw_depth(d, 0.5) + CAMERA_HEIGHT * 0.5 - (d + CAMERA_HEIGHT)).abs() < 1e-4);
    }
}
