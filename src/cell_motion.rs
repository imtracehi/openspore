//! Cell steering, locomotion and collision response — the retail controller
//! of `notes\cell-re-notes.md` 1A (input), 1B (capabilities from parts,
//! `MoveToTarget` / `Thrust` / `Integrate`), 1C (currents) and 1D (contact
//! impulse solver). All formulas run in retail world units (Bevy / `UNIT`).
//!
//! The water's current is the SPH fluid of `cell_fluid` (`FlowAt`). Cells with
//! outline pins (`cell_softbody`: every foreground cell of size >= 0.3 except
//! cellType None / Poison) move through them as retail's Integrate 0xE7C080
//! case (a): thrust and accelerations go into the pins' velocities, the water
//! carries them (no `FlowAt` drag), position and velocity are read back as the
//! pins' mean and the heading is the shape-matched rotation. Smaller cells
//! keep the `FlowAt` drag of case (b).
//! The animation-driven factors are the retail ones: the playing clips' Data1..3
//! channels (0xE83380) give the hit knockback, the speed factor `f` (0xE56510) and
//! the cilia / jet thrust K (0xE56300 / 0xE56200); the non-animating constants
//! (cilia 6, jet 250) apply only to a cell whose clip keys none.
//! `OPENSPORE_CELL_STATIC_THRUST=1` restores the constants and f = 1.
//! ADAPTED: the soft body's read-back is extrapolated by the fluid's fixed-step
//! lag (see `cell_softbody`).

use bevy::prelude::*;

use crate::{
    cell_ai::CellAiState,
    cell_contract::{CellAnimState, CellAnimation},
    cell_data::{CellDatabase, CellGlobals, CellGraphs},
    cell_engine::{heading_dir, heading_of, real_size, CellGame, CellGrid, CellKind, CellObject, UNIT},
    game::{Player, RunProgress},
};

/// Keyboard target distance ahead of the avatar (.rdata 0x1481754).
pub const KEYBOARD_TARGET_DISTANCE: f32 = 20.0;
/// Seconds a one-shot animation keeps priority over swim/idle loops.
pub const ONE_SHOT_HOLD: f32 = 0.8;

/// Avatar steering state (`cCellUI` kbDir/kbActive) and the cilia phase.
#[derive(Component, Clone, Debug, Default)]
pub struct CellPlayerMotion {
    /// Keyboard steering direction, retail plane axes (x right, y up).
    pub kb_dir: Vec2,
    pub kb_active: bool,
}

/// Per-cell locomotion state.
#[derive(Component, Clone, Debug)]
pub struct CellMotion {
    /// Cilia animation phase (+0x154), wraps at 0.3 s.
    pub cilia_phase: f32,
    /// Accumulated acceleration (+0x1C4), cleared after integration.
    pub accel: Vec2,
    /// Propulsion strength per effect, [jet (+0x198), cilia (+0x19C), flagella
    /// (+0x194)]: the length of this frame's velocity change (RE 0xE6EEC0),
    /// -1 while the cell has no such propulsion. cell_fx drives the
    /// `cell_propulsion*` effects with it (retail 0xE639D0).
    pub propulsion: [f32; 3],
    /// Knockback direction (+0x1A0), set by the last hit (0xE59010; see
    /// `cell_rules::damage`). The playing clip's Data1 channel drives the
    /// impulse (retail never clears it).
    pub knock: Vec2,
    /// The last animated thrust factor K (+0x1C0): the cilia / jet factors return it
    /// outside the swim states (0xE56200 / 0xE56300); negative until one was made.
    pub thrust_k: f32,
}

impl Default for CellMotion {
    fn default() -> Self {
        CellMotion {
            cilia_phase: 0.0,
            accel: Vec2::ZERO,
            propulsion: [-1.0; 3],
            knock: Vec2::ZERO,
            thrust_k: -1.0,
        }
    }
}

/// `HitImpulse` (0xE56560, VERIFIED): -200 (.data 0x15A853C) x Data1 x the
/// knockback direction x sqrt(invMass); the avatar's is 2.2 times stronger
/// at odd scale-info levels (0xE4EE10 & 1). Added to the acceleration every
/// `MoveToTarget` frame.
pub fn hit_impulse(data1: f32, knock: Vec2, mass: f32, avatar: bool, food: i32) -> Vec2 {
    let mut a = knock * (-200.0 * data1 / mass.max(1e-6).sqrt());
    if avatar && crate::cell_engine::scale_info_index(food) & 1 == 1 {
        a *= 2.2;
    }
    a
}

/// Which propulsion slot a movement style drives: the first matching style
/// bit in the order 8, 4, 1, 2 (RE 0xE6EEC0) -> flagella, jet, flagella, cilia.
pub fn propulsion_slot(style: u32) -> Option<usize> {
    if style & 8 != 0 {
        Some(2)
    } else if style & 4 != 0 {
        Some(0)
    } else if style & 1 != 0 {
        Some(2)
    } else if style & 2 != 0 {
        Some(1)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Capabilities from parts (RE 1B, 0xE52040)
// ---------------------------------------------------------------------------

/// Part type of a Cell part definition (UnlockType numbering, 11 = eye).
pub fn part_type(abilities: &crate::asset_catalog::Capabilities) -> Option<usize> {
    Some(if abilities.herbivore > 0 && abilities.carnivore > 0 {
        4
    } else if abilities.carnivore > 0 {
        2
    } else if abilities.herbivore > 0 {
        3
    } else if abilities.cell_poison > 0 {
        5
    } else if abilities.cell_electric > 0 {
        6
    } else if abilities.cell_spike > 0 {
        7
    } else if abilities.cell_cilia > 0 {
        8
    } else if abilities.cell_flagella > 0 {
        9
    } else if abilities.cell_jet > 0 {
        10
    } else if abilities.eye > 0 {
        11
    } else {
        return None;
    })
}

/// Cell capabilities (`caps`): movement style/speed, mouths, weapons, eyes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellCaps {
    /// n[t] per part type (a mirrored twin counts 1, an unmirrored part 2).
    pub n: [u32; 13],
    /// flagella 1 | cilia 2 | jet 4 (| turn-in-place 8 from AI data).
    pub style: u32,
    pub speed: f32,
    pub carnivore: bool,
    pub herbivore: bool,
    pub spike: bool,
    pub eyes: f32,
    pub poison: f32,
    pub electric_recharge: f32,
    pub electric_range: f32,
    pub electric_jumps: u32,
}

impl CellCaps {
    pub fn from_counts(n: [u32; 13], graphs: &CellGraphs, globals: &CellGlobals) -> Self {
        let f = |t: usize| n[t] as f32;
        let mut style = (if n[10] > 0 { 4 } else { 0 }) | (if n[8] > 0 { 2 } else { 0 }) | (if n[9] > 0 { 1 } else { 0 });
        let mut speed = if n[10] > 0 {
            graphs.eval(
                CellGraphs::JET_COUNT,
                f(10) + globals.flagella_speed_as_jet * f(9) + globals.cilia_speed_as_jet * f(8),
            )
        } else if n[9] > 0 {
            graphs.eval(CellGraphs::FLAGELLA_COUNT, f(9) + globals.cilia_speed_as_flagella * f(8))
        } else if n[8] > 0 {
            graphs.eval(CellGraphs::CILIA_COUNT, f(8))
        } else {
            0.0
        };
        if style == 0 {
            style = 1;
            speed = globals.no_part_speed;
        }
        Self {
            n,
            style,
            speed,
            carnivore: n[2] > 0 || n[4] > 0,
            herbivore: n[3] > 0 || n[4] > 0,
            spike: n[7] > 0,
            eyes: graphs.eval(CellGraphs::EYE_COUNT, f(11)),
            poison: graphs.eval(CellGraphs::POISON_COUNT, f(5)),
            electric_recharge: graphs.eval(CellGraphs::ELECTRIC_RECHARGE_COUNT, f(6)),
            electric_range: graphs.eval(CellGraphs::ELECTRIC_RANGE_COUNT, f(6)),
            electric_jumps: graphs.eval(CellGraphs::ELECTRIC_JUMPS_COUNT, f(6)).round().max(0.0) as u32,
        }
    }
}

/// The player's part counts from `run.cell_parts` (a symmetric pair shares a
/// `pair_id`: each half counts 1; an unpaired part counts 2).
pub fn player_counts(run: &RunProgress, originals: &crate::Originals) -> [u32; 13] {
    let mut n = [0u32; 13];
    for part in &run.cell_parts {
        if let Some(t) = originals
            .part_catalog
            .by_part_in(crate::asset_catalog::CELL_PARTS, part)
            .and_then(|p| part_type(&p.abilities))
        {
            n[t] += if part.pair_id != 0 { 1 } else { 2 };
        }
    }
    n
}

/// Body-plane positions (x, z; body radius 1) of the player's parts of
/// `kind` (`part_type`): where they sit on the membrane (the editor's
/// attachment point, `cell_editor_scene::point_of`).
pub fn player_part_points(run: &RunProgress, originals: &crate::Originals, kind: usize) -> Vec<Vec2> {
    run.cell_parts
        .iter()
        .filter(|part| {
            originals
                .part_catalog
                .by_part_in(crate::asset_catalog::CELL_PARTS, part)
                .and_then(|p| part_type(&p.abilities))
                == Some(kind)
        })
        .map(|part| {
            let p = crate::cell_editor_scene::point_of(part) / crate::cell::CELL_BODY_RADIUS;
            Vec2::new(p.x, p.z)
        })
        .collect()
}

/// Fallback radius of a player part's collision sphere, in body radii per unit
/// of the part's editor scale, used only while the part's model box is not
/// available (no installed model, headless runs). ADAPTED: the editor's part
/// sizing (`part_transform`: the model's bbox diagonal maps to 2 units, x 0.35
/// x part scale = 0.7; the larger of x / z is about 0.6 of it; / 2 = 0.21 Bevy
/// units; / the 0.9 body radius = 0.23).
pub const PART_SPHERE_RADIUS: f32 = 0.23;

/// A part model's rigblock box as retail's creation records hold it (the
/// box every placed part of that model carries in its rigblock, +0x14 .. +0x28):
/// the extents of the part-local x and z axes and the centre along the long
/// local y axis (the outward axis is local -Y), in model units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartBox {
    pub dx: f32,
    pub dz: f32,
    pub cy: f32,
    /// The box centre's model-local x (sideways), see `installed_part_box_centres`.
    pub cx: f32,
}

/// The rigblock data the installed Cell creations carry per part model: the box
/// (+0x14 .. +0x28) and the poison emit offset (+0x60 .. +0x6C). Both are
/// properties of the part model (every creation that uses the model stores the
/// same numbers), written by the Creator when a part is placed.
#[derive(Default)]
pub struct InstalledBlocks {
    pub boxes: std::collections::HashMap<u32, PartBox>,
    pub offsets: std::collections::HashMap<u32, Vec3>,
}

/// [`InstalledBlocks`] of the installed anatomy files (read once).
pub fn installed_blocks(index: &crate::asset_catalog::ResourceIndex) -> &'static InstalledBlocks {
    use std::sync::OnceLock;
    static BLOCKS: OnceLock<InstalledBlocks> = OnceLock::new();
    static NONE: OnceLock<InstalledBlocks> = OnceLock::new();
    // A headless run without an index must not pin an empty answer.
    if index.records.is_empty() {
        return NONE.get_or_init(InstalledBlocks::default);
    }
    BLOCKS.get_or_init(|| {
        let mut out = InstalledBlocks::default();
        for key in index
            .records
            .keys()
            .filter(|k| k.group_id == 0x4061_6200 && k.type_id == crate::creature_data::TYPE)
        {
            let Ok(data) = index.read(*key).and_then(|b| crate::creature_data::parse(&b)) else {
                continue;
            };
            for block in data.rigblocks.iter().filter(|b| b.group_id() == crate::asset_catalog::CELL_PARTS) {
                let bb = block.bounding_box_bits().map(f32::from_bits);
                if bb.iter().all(|v| v.is_finite()) {
                    out.boxes.entry(block.instance_id()).or_insert(PartBox {
                        dx: bb[3] - bb[0],
                        dz: bb[5] - bb[2],
                        cy: (bb[1] + bb[4]) * 0.5,
                        cx: (bb[0] + bb[3]) * 0.5,
                    });
                }
                let at = |i: usize| {
                    block
                        .raw_bytes
                        .get(0x60 + i * 4..0x64 + i * 4)
                        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
                };
                let off = Vec3::new(at(0), at(1), at(2));
                if off.is_finite() && off != Vec3::ZERO {
                    out.offsets.insert(block.instance_id(), off);
                }
            }
        }
        out
    })
}

/// The poison emit offsets by part model instance (see [`InstalledBlocks`]):
/// only the poison gland 0x64028FE0 has one, (0, -0.257, 0).
pub fn emit_offsets(index: &crate::asset_catalog::ResourceIndex) -> &'static std::collections::HashMap<u32, Vec3> {
    &installed_blocks(index).offsets
}

/// The rigblock box of a part model. A model the installed creations use takes
/// the box they store (retail data). ADAPTED: for a model no installed creation
/// uses, the bounds of the part's geometry times its `modelScale` stand in: they
/// equal the stored box for some parts (cilia, spikes, flagella) and are
/// 5 .. 20 % smaller for others (jaws, eyes, jets) and up to 3x smaller for the
/// proboscis, whose stored box is not its geometry (the hull / morph extent was
/// not identified). Cached per model.
pub fn part_box(index: &crate::asset_catalog::ResourceIndex, def: &crate::asset_catalog::PartDefinition) -> Option<PartBox> {
    use std::sync::{Mutex, OnceLock};
    if let Some(b) = installed_blocks(index).boxes.get(&def.model.instance_id) {
        return Some(*b);
    }
    static CACHE: OnceLock<Mutex<std::collections::HashMap<crate::dbpf::ResourceKey, Option<PartBox>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok()?.get(&def.model) {
        return *found;
    }
    let found = model_bounds(index, def.model).map(|b| PartBox {
        dx: b.dx * def.model_scale,
        dz: b.dz * def.model_scale,
        cy: b.cy * def.model_scale,
        cx: b.cx * def.model_scale,
    });
    cache.lock().ok()?.insert(def.model, found);
    found
}

/// Bounds of a part RW4's geometry in its native coordinates.
pub fn model_bounds(index: &crate::asset_catalog::ResourceIndex, model: crate::dbpf::ResourceKey) -> Option<PartBox> {
    let bytes = index.read(model).ok()?;
    let mut stats = crate::rw4::MeshStats::default();
    let meshes = crate::rw4::extract_meshes(&bytes, &mut stats);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in meshes.iter().flat_map(|m| m.positions.iter()) {
        let p = Vec3::from_array(*p);
        min = min.min(p);
        max = max.max(p);
    }
    (min.is_finite() && max.is_finite()).then(|| PartBox {
        dx: max.x - min.x,
        dz: max.z - min.z,
        cy: (min.y + max.y) * 0.5,
        cx: (min.x + max.x) * 0.5,
    })
}

/// Radius of the player's body in creation units: half the larger horizontal
/// extent of the body the starter creation draws (the extent
/// `cell_visuals::player_body` fits to the 0.9 body radius), which is the unit
/// the part boxes and rigblock positions are in.
pub fn player_body_radius(index: &crate::asset_catalog::ResourceIndex) -> Option<f32> {
    use std::sync::OnceLock;
    static RADIUS: OnceLock<Option<f32>> = OnceLock::new();
    if index.records.is_empty() {
        return None;
    }
    *RADIUS.get_or_init(|| {
        const CELL_GLOBALS: u32 = 0x2a3c_e5b7;
        let key = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == CELL_GLOBALS)
            .min_by_key(|k| (k.group_id, k.instance_id))?;
        let globals = index.read(key).ok()?;
        let creature = u32::from_le_bytes(globals.get(0x38..0x3C)?.try_into().ok()?);
        let model = crate::baked_model::parse(
            &index
                .read(crate::dbpf::ResourceKey {
                    type_id: crate::decode::TYPE_GMDL,
                    group_id: crate::cell_visuals::CELL_MODEL_GROUP,
                    instance_id: creature,
                })
                .ok()?,
        )
        .ok()?;
        // Only the vertebra body counts (the triangles bound to the first
        // `root_bones` bones), as `player_body` keeps them.
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for (piece, range) in model.pieces.iter().zip(&model.bone_ranges) {
            if piece.bones.len() != piece.positions.len() || piece.weights.len() != piece.positions.len() {
                return None;
            }
            for tri in piece.indices.chunks_exact(3) {
                let body = tri.iter().all(|&v| {
                    (0..4).all(|j| {
                        piece.weights[v as usize][j] == 0.0
                            || range[0] + piece.bones[v as usize][j] as u32 / 3 < model.root_bones
                    })
                });
                if body {
                    for &v in tri {
                        let p = Vec3::from_array(piece.positions[v as usize]);
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
        // `centered_fit` takes the larger of the model x and (mesh z =) model y extents.
        let span = (max.x - min.x).max(max.y - min.y);
        (span.is_finite() && span > 0.0).then_some(span * 0.5)
    })
}

/// Collision spheres of the player's parts (the 0xE83E30 part list of the
/// avatar): type, centre on the body plane (body radius 1) and radius.
/// Retail's sphere is `max(|dx|, |dz|) * s / 2` of the part model's rigblock
/// box (halved for mandibles), centred on the block's box centre; here the
/// box is the part model's own ([`part_box`]), `s` the part's editor scale and
/// the radius is expressed in body radii ([`player_body_radius`]). The
/// centre keeps the attachment point moved outward by the box centre's offset
/// along the part's axis (ADAPTED: the box centre's sideways offset is not
/// applied: measured over every installed part model it is at most 0.035
/// creation units, 3.5 % of a body radius and below a sphere radius's
/// resolution (`installed_part_box_centres`); the model-local x direction and
/// mirroring are not mapped).
pub fn player_part_spheres(run: &RunProgress, originals: &crate::Originals) -> Vec<crate::cell_engine::PartSphere> {
    let body = player_body_radius(&originals.resource_index);
    run.cell_parts
        .iter()
        .filter_map(|part| {
            let def = originals.part_catalog.by_part_in(crate::asset_catalog::CELL_PARTS, part);
            let t = def.and_then(|p| part_type(&p.abilities))?;
            let p = crate::cell_editor_scene::point_of(part) / crate::cell::CELL_BODY_RADIUS;
            let scale = part.scale.max(0.1);
            // `yaw_for`: the outward axis of a part is (sin yaw, cos yaw) on the plane.
            let axis = Vec2::new(part.yaw.sin(), part.yaw.cos());
            let mut center = Vec2::new(p.x, p.z);
            let mut radius = PART_SPHERE_RADIUS * scale;
            if let (Some(def), Some(body)) = (def, body) {
                if let Some(b) = part_box(&originals.resource_index, def) {
                    radius = b.dx.abs().max(b.dz.abs()) * scale * 0.5 / body;
                    // Local -Y is outward: a box centre at cy < 0 lies outside the attachment.
                    center += axis * (-b.cy * scale / body);
                    // Retail: x 0.5 for mandibles unless the model is 0xD9007204.
                    if t == 2 && def.model.instance_id != 0xD900_7204 {
                        radius *= 0.5;
                    }
                    return Some(crate::cell_engine::PartSphere { kind: t as u8, center, radius, axis });
                }
            }
            if t == 2 {
                radius *= 0.5;
            }
            Some(crate::cell_engine::PartSphere { kind: t as u8, center, radius, axis })
        })
        .collect()
}

/// Poison emit points of the player's parts, `(placement, emit point)` on the
/// body plane (body radius 1): retail releases a cloud at the part's pose
/// plus the rigblock's +0x60 offset turned by the block's orientation
/// (0xE68000, scale 1 in the part list's 2.5x frame, so 1/2.5 creation unit).
/// The model's offset is along the part's local y axis (outward is -Y), which
/// is the part's yaw axis; ADAPTED: a sideways offset component (none is
/// installed) is not applied.
pub fn player_poison_points(run: &RunProgress, originals: &crate::Originals) -> Vec<(Vec2, Vec2)> {
    let offsets = emit_offsets(&originals.resource_index);
    let body = player_body_radius(&originals.resource_index);
    run.cell_parts
        .iter()
        .filter_map(|part| {
            let def = originals.part_catalog.by_part_in(crate::asset_catalog::CELL_PARTS, part)?;
            (part_type(&def.abilities) == Some(5)).then(|| {
                let p = crate::cell_editor_scene::point_of(part) / crate::cell::CELL_BODY_RADIUS;
                let rest = Vec2::new(p.x, p.z);
                let outward = match (offsets.get(&def.model.instance_id), body) {
                    (Some(off), Some(body)) => -off.y / 2.5 / body,
                    _ => 0.0,
                };
                (rest, rest + Vec2::new(part.yaw.sin(), part.yaw.cos()) * outward)
            })
        })
        .collect()
}

/// Player capabilities. Without a loaded part catalog (headless runs) the
/// cell is an omnivore with one flagellum (as the old fallback allowed).
pub fn player_caps(run: &RunProgress, originals: &crate::Originals, db: &CellDatabase) -> CellCaps {
    let (Some(globals), false) = (db.globals.as_ref(), originals.part_catalog.parts.is_empty()) else {
        let mut n = [0; 13];
        n[4] = 2;
        n[9] = 2;
        return db
            .globals
            .as_ref()
            .map(|g| CellCaps::from_counts(n, &db.graphs, g))
            .unwrap_or(CellCaps {
                n,
                style: 1,
                speed: 3.0,
                carnivore: true,
                herbivore: true,
                ..default()
            });
    };
    CellCaps::from_counts(player_counts(run, originals), &db.graphs, globals)
}

// ---------------------------------------------------------------------------
// Input (RE 1A)
// ---------------------------------------------------------------------------

fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Retail heading of a unit direction `v = (sin h, cos h)` (0xE50C90).
fn retail_heading(v: Vec2) -> f32 {
    let s = v.x.clamp(-1.0, 1.0).asin();
    if v.y > 0.0 {
        s
    } else if s >= 0.0 {
        std::f32::consts::PI - s
    } else {
        -std::f32::consts::PI - s
    }
}

/// `RotateDirToward` (0xE5B0A0): turn `cur` toward `des` at 1 rad/s; turns of
/// 1.1*PI/4 or more snap; never overshoots.
pub fn rotate_dir_toward(cur: Vec2, des: Vec2, dt: f32) -> Vec2 {
    let eps = 1.0 / 65536.0;
    if cur.length_squared() < eps || (cur - des).length_squared() < eps {
        return des;
    }
    let a = retail_heading(cur);
    let b = retail_heading(des);
    let delta = wrap_pi(b - a);
    let s = if delta < 0.0 { -1.0 } else { 1.0 };
    if delta.abs() >= 1.1 * std::f32::consts::FRAC_PI_4 {
        return des;
    }
    let mut n = wrap_pi(a + s * dt);
    let after = wrap_pi(b - n);
    if (if after < 0.0 { -1.0 } else { 1.0 }) != s {
        n = b;
    }
    Vec2::new(n.sin(), n.cos())
}

/// Retail plane (x right, y up) -> Bevy plane (x, z with -z up the screen).
fn to_bevy(v: Vec2) -> Vec2 {
    Vec2::new(v.x, -v.y)
}

/// Where the cursor ray meets the play plane. A click on a HUD button never
/// steers: the SPUI engine consumes it (`SpuiPointer::over_ui` = the pointer is
/// over a visible interactive window of a layout, the Button hit rule of the
/// decoded SPUI engine). Without the SPUI pointer resource (headless runs)
/// nothing is blocked.
fn cursor_on_plane(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    over_ui: bool,
) -> Option<Vec2> {
    let window = windows.iter().find(|w| w.cursor_position().is_some())?;
    let cursor = window.cursor_position()?;
    if over_ui {
        return None;
    }
    let (camera, transform) = cameras.single().ok()?;
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    let t = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    let hit = ray.get_point(t);
    Some(Vec2::new(hit.x, hit.z))
}

/// `MovePlayerToMousePosition` + `KeyboardSteer`.
#[allow(clippy::too_many_arguments)]
pub fn player_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    game: Res<CellGame>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    mut run: ResMut<RunProgress>,
    mut player: Query<(&Transform, &mut CellObject, &mut CellPlayerMotion), With<Player>>,
) {
    if game.locked {
        return;
    }
    let Ok((t, mut avatar, mut motion)) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let here = Vec2::new(t.translation.x, t.translation.z);
    let mut d = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::Numpad8]) {
        d.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown, KeyCode::Numpad2]) {
        d.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft, KeyCode::Numpad4]) {
        d.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight, KeyCode::Numpad6]) {
        d.x += 1.0;
    }
    if d.length_squared() > 1.0 / 65536.0 {
        let dn = d / (d.length_squared() + 1e-8).sqrt();
        motion.kb_dir = rotate_dir_toward(motion.kb_dir, dn, dt);
        avatar.target = Some(here + to_bevy(motion.kb_dir) * KEYBOARD_TARGET_DISTANCE * UNIT);
        motion.kb_active = true;
        run.cell.player_has_moved = true;
    } else if motion.kb_active {
        motion.kb_active = false;
        avatar.target = None;
        motion.kb_dir = Vec2::ZERO;
    }
    // controlMethod 0: steer while the left or right button is held, or always
    // with the Cell control option (`OptionCellControls`, "automatically follow
    // the cursor"; 0xE5B790: steer = opt || lmb || rmb).
    let held = crate::front_end::settings::cell_follows_cursor()
        || mouse
            .as_deref()
            .is_some_and(|m| m.pressed(MouseButton::Left) || m.pressed(MouseButton::Right));
    if held {
        if let Some(hit) = cursor_on_plane(&windows, &cameras, spui.as_deref().is_some_and(|s| s.over_ui)) {
            avatar.target = Some(hit);
            run.cell.player_has_moved = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Locomotion (RE 1B)
// ---------------------------------------------------------------------------

/// `SizeDivisor` (0xE56450): slows NPCs smaller than the player's level.
pub fn size_divisor(scale: crate::cell_data::CellScale, player: crate::cell_data::CellScale, is_player: bool) -> f32 {
    if is_player {
        return 1.0;
    }
    let r = real_size(scale) / real_size(player);
    if r <= 0.5 {
        4.0
    } else if r <= 1.0 {
        4.0 - (r - 0.5) * 4.0
    } else if r <= 2.0 {
        2.0
    } else if r <= 4.0 {
        2.0 - (r - 2.0) * 0.5
    } else {
        1.0
    }
}

/// `TurnRate` (0xE56B50), rad/s.
fn turn_rate(style: u32, dist: f32, cilia_phase: f32, g: &CellGlobals, graphs: &CellGraphs) -> f32 {
    if style & 2 != 0 {
        graphs.eval(CellGraphs::CILIA_GRAPH, cilia_phase) * g.cilia_turn_speed
    } else if style & 9 == 0 {
        g.jet_turn_speed
    } else {
        let t = ((dist - g.flagella_turn_speed_ramp_start)
            / (g.flagella_turn_speed_ramp_end - g.flagella_turn_speed_ramp_start).max(1e-3))
        .clamp(0.0, 1.0);
        g.flagella_turn_speed_min + (g.flagella_turn_speed_max - g.flagella_turn_speed_min) * t
    }
}

/// `OPENSPORE_CELL_STATIC_THRUST=1` (diagnostics, read once): the cells keep the non-animating
/// thrust factors and a speed factor of 1 instead of the animation's Data2 / Data3 channels.
fn static_thrust() -> bool {
    static STATIC: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STATIC.get_or_init(|| std::env::var_os("OPENSPORE_CELL_STATIC_THRUST").is_some_and(|v| v != "0"))
}

/// The cilia / jet thrust factor K (0xE56300 / 0xE56200): in the swim states the
/// playing clip's Data3 x `scale` (cached), the non-animating constant when the clip
/// keys none; outside them the cached value (the constant until one was made).
fn thrust_factor(motion: &mut CellMotion, swim: bool, data3: Option<f32>, scale: f32, fallback: f32) -> f32 {
    if swim {
        if let Some(v) = data3 {
            motion.thrust_k = v * scale;
            return motion.thrust_k;
        }
        return fallback;
    }
    if motion.thrust_k >= 0.0 {
        motion.thrust_k
    } else {
        fallback
    }
}

/// `Thrust` (0xE55C40 flagella/cilia, 0xE558C0 jet): velocity change.
#[allow(clippy::too_many_arguments)]
fn thrust(facing: Vec2, d: Vec2, v: Vec2, flow: Vec2, s: f32, k: f32, speed_factor: f32, kind: u32, also_cilia: bool, dt: f32) -> Vec2 {
    let dist = d.length();
    let nd = d.normalize_or_zero();
    let mut dir = if dist >= 1.0 { facing } else { nd.lerp(facing, dist) };
    let c = dir.normalize_or_zero().dot(nd);
    let align = if kind == 2 {
        if c > 0.9 { 1.0 } else { 0.0 }
    } else {
        (2.0 * c - 1.0).clamp(0.0, 1.0)
    };
    if speed_factor > 1.0 / 65536.0 {
        dir *= align;
    }
    let (lo, hi) = if kind == 4 { (0.0, 1.0) } else { (0.5, 1.0) };
    let r = lo + (hi - lo) * ((dist - 0.5) / 2.5).clamp(0.0, 1.0);
    let x = if (v - flow).length() <= dist || (kind == 4 && !also_cilia) {
        flow + dir * r * s * k
    } else {
        flow - v
    };
    let mut dv = (x - v) * dt;
    if kind == 4 && align == 0.0 && also_cilia {
        dv *= 10.0;
    }
    dv
}

/// `FlowAt` (0xE4E3A0): the SPH water's velocity grid, bilinear over the 20 x 15
/// cell centres (`cell_fluid`; zero until the water exists, as the retail
/// grid is before its first step). `p_world` is in retail world units.
pub fn flow_at(game: &CellGame, p_world: Vec2) -> Vec2 {
    game.fluid.flow_at(p_world)
}

fn is_one_shot(a: CellAnimation) -> bool {
    !matches!(
        a,
        CellAnimation::CellIdle
            | CellAnimation::CellIdleBig
            | CellAnimation::CellMovFlg
            | CellAnimation::CellMovFlgBig
            | CellAnimation::CellMovCil
            | CellAnimation::CellMovCilBig
            | CellAnimation::CellMoveJet
            | CellAnimation::CellMoveNpcScared
            | CellAnimation::CellMoveNpcMad
            | CellAnimation::CellMovCilScared
            | CellAnimation::CellMovCilMad
            | CellAnimation::CellMovFlgBigMad
            | CellAnimation::CellMovCilBigMad
            | CellAnimation::CellBackgrndGuy
            | CellAnimation::CellMovBankLt
            | CellAnimation::CellMovBankRt
            | CellAnimation::CellMovFlgStart
    )
}

/// Request a looping swim/idle state (retail code + legacy slot) unless a
/// one-shot still has priority.
pub fn request_loop(anim: &mut CellAnimState, code: i32, loop_anim: CellAnimation) {
    if !is_one_shot(anim.requested) || anim.finished || anim.elapsed > ONE_SHOT_HOLD {
        crate::cell_engine::request_anim(anim, code, loop_anim);
    }
}

/// Speed / turn factor / style an NPC moves with this frame (set by the AI).
#[derive(Clone, Copy, Debug, Default)]
pub struct Drive {
    pub speed: f32,
    pub speed_factor: f32,
    pub style: u32,
}

/// `MoveToTarget` + `Integrate` for every cell, then the contact solver.
#[allow(clippy::too_many_arguments)]
pub fn locomote(
    time: Res<Time>,
    db: Res<CellDatabase>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    mut game: ResMut<CellGame>,
    grid: Res<CellGrid>,
    contacts: Res<crate::cell_rules::CellContacts>,
    mut objects: Query<(
        Entity,
        &mut Transform,
        &mut CellObject,
        Option<&CellAiState>,
        Option<&CellPlayerMotion>,
        Option<&mut CellMotion>,
        Option<&mut CellAnimState>,
        Option<&crate::cell_anim::CellAnimPlayback>,
        Has<crate::cell_engine::CellMateObject>,
    )>,
    anim_assets: Option<Res<crate::cell_anim::CellAnimAssets>>,
    mut trace_frames: Local<u32>,
    mut trace_speed: Local<(f32, f32, u32)>,
    mut trace_peak: Local<usize>,
) {
    let dt = time.delta_secs().min(0.1);
    if dt <= 0.0 {
        return;
    }
    let Some(g) = db.globals.as_ref() else {
        return;
    };
    let player_caps = player_caps(&run, &originals, &db);
    let player_scale = crate::cell_engine::player_scale(run.cell.food_progression);
    // Cells that are gone, dead or in the background layer give their pins back.
    {
        let live: std::collections::HashSet<Entity> = objects
            .iter()
            .filter(|(_, _, o, ..)| !o.dead && !o.background)
            .map(|(e, ..)| e)
            .collect();
        let CellGame { fluid, bodies, .. } = &mut *game;
        bodies.retain_live(fluid, &|e| live.contains(&e));
    }
    // 0xE507F0: the visible box the outline pins follow (retail world units).
    let look = game.camera_look / UNIT;
    for (entity, mut t, mut o, ai, player, motion, anim, playback, mate) in &mut objects {
        if o.dead {
            continue;
        }
        let mut accel = Vec2::ZERO;
        let Some(mut motion) = motion else {
            continue;
        };
        // HitImpulse: the hit reaction clip's Data1 channel pushes the cell
        // along its knockback direction.
        if let (Some(assets), Some(playback)) = (anim_assets.as_deref(), playback) {
            if motion.knock != Vec2::ZERO && !o.background {
                let data1 = crate::cell_anim::game_channel(assets, playback, crate::cell_anim::DATA1_KNOCKBACK);
                if data1 != 0.0 {
                    accel += hit_impulse(data1, motion.knock, o.mass, player.is_some(), run.cell.food_progression);
                }
            }
        }
        if o.background {
            // Background-layer cells only drift: pos += vel * dt.
            t.translation.x += o.velocity.x * dt;
            t.translation.z += o.velocity.y * dt;
            t.translation.y = -crate::cell_engine::background_draw_depth(o.depth, game.view_factor());
            if let Some(mut anim) = anim {
                anim.elapsed += dt;
                request_loop(&mut anim, crate::cell_engine::anim_code::IDLE, CellAnimation::CellBackgrndGuy);
            }
            continue;
        }
        motion.cilia_phase = (motion.cilia_phase + dt) % 0.3;
        let pos = o.plane(&t) / UNIT;
        let v = o.velocity / UNIT;
        let flow = flow_at(&game, pos);
        // Outline pins (retail 0xE5A1A0 / 0xE6D97B): created, re-allocated by
        // size level, and told about edits other systems made to the cell.
        let record = db.cells.get(&o.cell);
        let size = o.radius / UNIT;
        let fixed = record.is_some_and(|r| r.fixed_orientation);
        // 0xE78C00 -> 0xE5CDE0: the pins belong to the GFX object, which exists only
        // while the cell is on screen (the avatar and the mate always are): a cell that
        // leaves the visible box gives its pins back and swims on with the `FlowAt` drag
        // of case (b) until it is seen again.
        let seen = player.is_some() || mate || crate::cell_population::pin_visible(look, pos, size);
        // `OPENSPORE_NO_SOFTBODY=1` (diagnostics): every cell keeps the FlowAt drag.
        let level = if seen && record.is_some_and(crate::cell_softbody::has_outline) && !crate::cell_softbody::disabled() {
            crate::cell_softbody::level_for_size(size)
        } else {
            0
        };
        let soft = {
            let CellGame { fluid, bodies, .. } = &mut *game;
            bodies.prepare(fluid, entity, level, size, pos, v, o.heading)
        };
        let drive = if player.is_some() {
            Some(Drive {
                speed: player_caps.speed,
                speed_factor: 1.0,
                style: player_caps.style,
            })
        } else {
            ai.map(|a| a.drive)
        };
        // The animation's game channels (0xE83380): Data2 is the speed factor `f`
        // (0xE56510: 1 in the swim states 0x12 / 0x13 / 0x14 / 0x30, else the playing
        // clip's value, 1 when it keys none) and Data3 the cilia (x 20) / jet (x 100)
        // thrust strength K (0xE56300 / 0xE56200: in the swim states the clip's value,
        // the non-animating constant when it keys none or the cell has no animation;
        // elsewhere the last K). A steering cell in state 0 enters a swim state before
        // the thrust is made (the state assignment of 0xE6xxxx precedes `f`).
        let state = anim.as_ref().map_or(0, |a| a.request_code);
        let swim = matches!(state, 18 | 19 | 20 | 48) || (o.target.is_some() && state == 0);
        let animated = !static_thrust();
        let keyed = |id: u32| match (anim_assets.as_deref(), playback) {
            (Some(assets), Some(playback)) if animated => crate::cell_anim::game_channel_keyed(assets, playback, id),
            _ => None,
        };
        let speed_f = if matches!(state, 18 | 19 | 20 | 48) {
            1.0
        } else {
            keyed(crate::cell_anim::DATA2_SPEED).unwrap_or(1.0)
        };
        let f = if swim { 1.0 } else { speed_f };
        let mut dv = Vec2::ZERO;
        let slot = drive.and_then(|d| propulsion_slot(d.style));
        motion.propulsion = [-1.0; 3];
        if let Some(slot) = slot {
            motion.propulsion[slot] = 0.0;
        }
        if let (Some(drive), Some(target)) = (drive, o.target) {
            let d = target / UNIT - pos;
            let dist = d.length();
            if dist < 0.5 {
                o.target = None;
            } else {
                let mut turn = turn_rate(drive.style, dist, motion.cilia_phase, g, &db.graphs);
                let mut speed = drive.speed;
                if player.is_none() {
                    speed *= g.npc_speed_multiplier / size_divisor(o.scale, player_scale, false);
                    turn *= if drive.style & 9 != 0 {
                        g.npc_turn_speed_multiplier_flagella
                    } else if drive.style & 2 != 0 {
                        g.npc_turn_speed_multiplier_cilia
                    } else if drive.style & 4 != 0 {
                        g.npc_turn_speed_multiplier_jet
                    } else {
                        1.0
                    };
                    turn *= db.graphs.eval(
                        CellGraphs::NPC_TURN_FACTOR,
                        real_size(o.scale) / real_size(player_scale),
                    );
                }
                turn *= drive.speed_factor;
                // Rotate the facing toward the target (snaps within the step).
                let want = heading_of(d);
                let a = wrap_pi(want - o.heading);
                // 0xE6F660 turns only in state 0 / 0x28 or while the speed factor is positive.
                let may_turn = state == 0 || state == 0x28 || speed_f >= 1.0 / 65536.0;
                if may_turn && (a.abs() <= 3.0 * std::f32::consts::FRAC_PI_4 || drive.style & 2 == 0) {
                    o.heading += a.clamp(-turn * dt, turn * dt);
                }
                let facing = heading_dir(o.heading);
                // Propulsion: the first matching style bit in the order 8, 4, 1, 2.
                dv = if drive.style & 8 != 0 {
                    let candidate = flow + d.normalize_or_zero() * speed.min(dist) - v;
                    if facing.dot(candidate.normalize_or_zero()).abs() < 0.9 {
                        Vec2::ZERO
                    } else {
                        candidate
                    }
                } else if drive.style & 4 != 0 {
                    let k = thrust_factor(&mut motion, swim, keyed(crate::cell_anim::DATA3_THRUST), 100.0, g.non_animating_jet_movement_factor);
                    thrust(facing, d, v, flow, speed, k, drive.speed_factor, 4, drive.style & 2 != 0, dt)
                } else if drive.style & 1 != 0 {
                    thrust(facing, d, v, flow, speed, 1.0, drive.speed_factor, 1, false, dt)
                } else {
                    let k = thrust_factor(&mut motion, swim, keyed(crate::cell_anim::DATA3_THRUST), 20.0, g.non_animating_cilia_movement_factor);
                    thrust(facing, d, v, flow, speed, k, drive.speed_factor, 2, false, dt)
                };
                // `vel += dv * f` (0xE560D0).
                dv *= f;
            }
        }
        // Propulsion strength (RE 0xE6EEC0): |dv| + 2^-16; the jet's is scaled by
        // how well the push lines up with the facing.
        if let Some(slot) = slot {
            if o.target.is_some() {
                let mut strength = dv.length();
                if slot == 0 {
                    strength *= heading_dir(o.heading).dot(dv.normalize_or_zero()).max(0.0);
                }
                motion.propulsion[slot] = strength + 1.5258789e-5;
            }
        }
        // Integrate (0xE7C080): drag toward the flow, accelerations, position.
        accel += motion.accel;
        motion.accel = Vec2::ZERO;
        if soft {
            // Case (a): the pins carry the cell. Steering turned the facing
            // (0xE5E850: the pins turn about the cell), the shape is matched
            // against the pins' current positions (0xE5A430: displacement for the
            // next water step, rotation = the cell's heading), the thrust and the
            // acceleration go into every pin's velocity (0xE560D0, 0xE7C080), and
            // the cell's position / velocity are the pins' mean (0xE553A0).
            let CellGame { fluid, bodies, .. } = &mut *game;
            if let Some(body) = bodies.map.get_mut(&entity) {
                let turn = crate::cell_softbody::angle_of_heading(o.heading) - body.angle;
                body.rotate(fluid, pos, crate::cell_softbody::wrap_pi(turn));
                let angle = body.match_shape(fluid, size, fixed, dt);
                if !fixed {
                    o.heading = crate::cell_softbody::heading_of_angle(angle);
                }
                body.add_velocity(fluid, dv + accel * dt);
                let (mean, vel) = body.read_back(fluid);
                // ADAPTED: the water steps at a fixed 1/60 s; extrapolate the
                // read-back over the time since the last step so the cell moves
                // smoothly at any frame rate.
                let at = mean + vel * fluid.lag();
                body.pos = at;
                body.vel = vel;
                o.velocity = vel * UNIT;
                t.translation.x = at.x * UNIT;
                t.translation.z = at.y * UNIT;
            }
        } else {
            let mut v = v + dv;
            let s = 1.0 - (-dt).exp();
            v += (flow - v) * s;
            v += accel * dt;
            o.velocity = v * UNIT;
            t.translation.x += o.velocity.x * dt;
            t.translation.z += o.velocity.y * dt;
        }
        t.translation.y = 0.0;
        t.rotation = Quat::from_rotation_y(o.heading);
        // Swim/idle loop slot.
        if let Some(mut anim) = anim {
            anim.elapsed += dt;
            let style = drive.map_or(1, |d| d.style);
            let moving = o.target.is_some() && o.velocity.length() > o.radius * 0.1;
            anim.speed = (o.velocity.length() / (o.radius * 2.0).max(1e-3)).clamp(0.3, 3.0);
            let big = o.kind != CellKind::Player && o.scale.index() > player_scale.index();
            let loop_anim = match (moving, style, big) {
                (false, _, false) => CellAnimation::CellIdle,
                (false, _, true) => CellAnimation::CellIdleBig,
                (true, s, _) if s & 4 != 0 => CellAnimation::CellMoveJet,
                (true, s, false) if s & 2 != 0 && s & 1 == 0 => CellAnimation::CellMovCil,
                (true, s, true) if s & 2 != 0 && s & 1 == 0 => CellAnimation::CellMovCilBig,
                (true, _, false) => CellAnimation::CellMovFlg,
                (true, _, true) => CellAnimation::CellMovFlgBig,
            };
            // Retail states: 18 moving (19 scared / 20 mad from the AI), 0 idle,
            // 23 asleep.
            let asleep = ai.is_some_and(|a| a.mode == crate::cell_ai::AiMode::Asleep);
            let code = if asleep {
                crate::cell_engine::anim_code::ASLEEP
            } else if moving {
                ai.map_or(crate::cell_engine::anim_code::MOVE, |a| a.loop_code)
            } else {
                crate::cell_engine::anim_code::IDLE
            };
            // A locked avatar is held in its event's state by the flow (retail
            // per-frame event updates, e.g. 0xE5EC70 / 0xE5EC10: "avatar locked,
            // state 0x2C / 0x2B"); the swim loop must not replace the event's
            // request (it restarted the hatch and its effect every 0.8 s).
            if !(player.is_some() && game.locked) {
                request_loop(&mut anim, code, loop_anim);
            }
        }
    }
    if std::env::var_os("OPENSPORE_FLUID_TRACE").is_some() {
        *trace_frames += 1;
        if let Some(avatar) = game.player.and_then(|p| objects.get(p).ok()) {
            let speed = avatar.2.velocity.length() / UNIT;
            trace_speed.0 += speed;
            trace_speed.1 = trace_speed.1.max(speed);
            trace_speed.2 += 1;
        }
        *trace_peak = (*trace_peak).max(game.fluid.pinned_count());
        if *trace_frames % 120 == 0 || *trace_frames == 3 {
            // Census of the pins by record (diagnostics: the pool pressure).
            let mut census: std::collections::BTreeMap<(String, usize), (usize, usize, f32, f32, i32)> = Default::default();
            for (entity, body) in game.bodies.map.iter() {
                if let Ok((_, _, o, ..)) = objects.get(*entity) {
                    let record = db.cells.get(&o.cell);
                    let name = format!(
                        "{:08x} {} {:?}",
                        o.cell,
                        record.map_or("", |r| r.name.as_str()),
                        record.map(|r| r.cell_type)
                    );
                    let size = o.radius / UNIT;
                    let e = census.entry((name, body.level)).or_insert((0, 0, f32::MAX, 0.0, o.scale.index()));
                    e.0 += 1;
                    e.1 += body.pins.len();
                    e.2 = e.2.min(size);
                    e.3 = e.3.max(size);
                }
            }
            let mut rows: Vec<_> = census.into_iter().collect();
            rows.sort_by_key(|r| std::cmp::Reverse((r.1).1));
            for ((name, level), (n, pins, lo, hi, scale)) in rows.iter().take(10) {
                eprintln!("  pins {pins:4} = {n:3} x {name} (level {level}, scale {scale}, size {lo:.2}..{hi:.2})");
            }
            eprintln!(
                "avatar speed over the last {} frames: mean {:.3} max {:.3}",
                trace_speed.2,
                trace_speed.0 / trace_speed.2.max(1) as f32,
                trace_speed.1
            );
            *trace_speed = (0.0, 0.0, 0);
            let mut levels = [0usize; 4];
            let mut pins = 0;
            for body in game.bodies.map.values() {
                levels[body.level.min(3)] += 1;
                pins += body.pins.len();
            }
            let avatar = game.player.and_then(|p| objects.get(p).ok()).map(|(_, t, o, ..)| {
                format!(
                    "avatar at ({:.2}, {:.2}) speed {:.3} heading {:.2} pins {}",
                    t.translation.x / UNIT,
                    t.translation.z / UNIT,
                    o.velocity.length() / UNIT,
                    o.heading,
                    game.bodies.map.get(&game.player.unwrap()).map_or(0, |b| b.pins.len())
                )
            });
            // Cells that exist in the foreground, those that could pin (cellType, size)
            // and those on screen (0xE5CDE0): the retail rule pins only the last.
            let look = game.camera_look / UNIT;
            let (mut cells, mut eligible, mut visible) = (0, 0, 0);
            for (_, t, o, ..) in objects.iter() {
                if o.background || o.dead {
                    continue;
                }
                cells += 1;
                let record = db.cells.get(&o.cell);
                let size = o.radius / UNIT;
                if record.is_some_and(crate::cell_softbody::has_outline) && crate::cell_softbody::level_for_size(size) > 0 {
                    eligible += 1;
                    visible += crate::cell_population::pin_visible(look, t.translation.xz() / UNIT, size) as usize;
                }
            }
            eprintln!(
                "softbodies: cells with pins by level 1/2/3 = {}/{}/{}, pins {pins} (pool {} of {}, peak {}), foreground cells {cells}, pin-eligible {eligible}, on screen {visible}, {}",
                levels[1],
                levels[2],
                levels[3],
                game.fluid.pinned_count(),
                crate::cell_fluid::PARTICLES - 1,
                *trace_peak,
                avatar.unwrap_or_default()
            );
        }
    }
    // Contact solver (1D): 10 iterations over the frame's contact pairs.
    if contacts.pairs.is_empty() {
        return;
    }
    let mut lambdas = vec![0.0f32; contacts.pairs.len()];
    for _ in 0..10 {
        for (i, c) in contacts.pairs.iter().enumerate() {
            let Ok([(_, ta, oa, ..), (_, tb, ob, ..)]) = objects.get_many_mut([c.a, c.b]) else {
                continue;
            };
            let (mut oa, mut ob) = (oa, ob);
            if oa.dead || ob.dead {
                continue;
            }
            let delta = (ob.plane(&tb) - oa.plane(&ta)) / UNIT;
            let dist = delta.length();
            let n = if dist > 1e-6 { delta / dist } else { Vec2::X };
            let depth = c.depth;
            let (inv_a, inv_b) = (1.0 / oa.mass.max(1e-6), 1.0 / ob.mass.max(1e-6));
            let eff = 1.0 / (inv_a + inv_b);
            let bias = 0.1 * (depth - 0.1).max(0.0) / dt;
            let vrel = (ob.velocity - oa.velocity).dot(n) / UNIT;
            let new = (lambdas[i] + (bias - vrel) * eff).max(0.0);
            let d = new - lambdas[i];
            lambdas[i] = new;
            oa.velocity -= n * d * inv_a * UNIT;
            ob.velocity += n * d * inv_b * UNIT;
        }
    }
    // The impulses reach the pins (0xE560D0 adds them to every pin of a soft
    // body; the next read-back would overwrite the cell's own velocity).
    {
        let CellGame { fluid, bodies, .. } = &mut *game;
        for (entity, body) in bodies.map.iter_mut() {
            if let Ok((_, _, o, ..)) = objects.get(*entity) {
                let v = o.velocity / UNIT;
                body.add_velocity(fluid, v - body.vel);
                body.vel = v;
            }
        }
    }
    let _ = &grid;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_impulse_follows_data1_and_the_inverse_mass_root() {
        // -200 x Data1 x knock x sqrt(1 / mass).
        let a = hit_impulse(-0.5, Vec2::X, 4.0, false, 0);
        assert!((a - Vec2::new(50.0, 0.0)).length() < 1e-4, "{a:?}");
        assert_eq!(hit_impulse(0.0, Vec2::X, 4.0, false, 0), Vec2::ZERO);
        // The avatar at an odd scale-info level: x 2.2.
        let odd = crate::cell_engine::SCALE_INFO[1].required_food;
        let b = hit_impulse(-0.5, Vec2::X, 4.0, true, odd);
        assert!((b.x - 110.0).abs() < 1e-3, "{b:?}");
        let even = crate::cell_engine::SCALE_INFO[2].required_food;
        assert!((hit_impulse(-0.5, Vec2::X, 4.0, true, even).x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn keyboard_direction_turns_at_one_radian_per_second_and_snaps() {
        let up = Vec2::new(0.0, 1.0);
        let right = Vec2::new(1.0, 0.0);
        assert_eq!(rotate_dir_toward(up, right, 0.016), right, "90 degrees snaps");
        let thirty = Vec2::new(0.5, 3f32.sqrt() / 2.0);
        let step = rotate_dir_toward(up, thirty, 0.1);
        assert!((retail_heading(step) - 0.1).abs() < 1e-4, "{step}");
        let small = Vec2::new(0.05f32.sin(), 0.05f32.cos());
        assert!((retail_heading(rotate_dir_toward(up, small, 0.2)) - 0.05).abs() < 1e-5);
        assert_eq!(rotate_dir_toward(Vec2::ZERO, right, 0.016), right);
    }

    #[test]
    fn retail_heading_inverts_sin_cos() {
        for h in [-3.0f32, -1.2, 0.0, 0.7, 2.5] {
            let v = Vec2::new(h.sin(), h.cos());
            assert!((retail_heading(v) - h).abs() < 1e-4, "{h}");
        }
    }

    #[test]
    fn capabilities_follow_the_part_graphs() {
        let mut graphs = CellGraphs::default();
        graphs.graphs.insert(CellGraphs::FLAGELLA_COUNT, vec![[0.0, 0.0], [2.0, 3.0], [4.0, 4.0]]);
        graphs.graphs.insert(CellGraphs::JET_COUNT, vec![[0.0, 0.0], [2.0, 0.02]]);
        let globals = crate::cell_data::CellGlobals::parse(&{
            let mut b = vec![0u8; 0x114];
            b[0x90..0x94].copy_from_slice(&1.0f32.to_le_bytes()); // noPartSpeed
            b[0x78..0x7C].copy_from_slice(&0.5f32.to_le_bytes()); // flagellaSpeedAsJet
            b
        })
        .unwrap();
        // One unmirrored flagellum (counts 2) -> speed 3, style flagella.
        let mut n = [0; 13];
        n[9] = 2;
        let caps = CellCaps::from_counts(n, &graphs, &globals);
        assert_eq!((caps.style, caps.speed), (1, 3.0));
        // No movement part: style 1 at noPartSpeed.
        let caps = CellCaps::from_counts([0; 13], &graphs, &globals);
        assert_eq!((caps.style, caps.speed), (1, 1.0));
        // Jet + flagella: jet graph over n10 + 0.5*n9.
        n[10] = 2;
        let caps = CellCaps::from_counts(n, &graphs, &globals);
        assert_eq!(caps.style, 5);
        assert!((caps.speed - 0.02).abs() < 1e-6);
    }

    #[test]
    fn npcs_smaller_than_the_player_level_are_slowed() {
        use crate::cell_data::CellScale;
        assert_eq!(size_divisor(CellScale(1), CellScale(2), false), 4.0);
        assert_eq!(size_divisor(CellScale(2), CellScale(2), false), 2.0);
        assert_eq!(size_divisor(CellScale(5), CellScale(2), false), 1.0);
        assert_eq!(size_divisor(CellScale(1), CellScale(9), true), 1.0);
    }

    #[test]
    fn propulsion_slots_follow_the_style_bit_order() {
        // 8 turn-in-place and 1 flagella -> flagella slot, 4 jet, 2 cilia.
        assert_eq!(propulsion_slot(8 | 4 | 1), Some(2));
        assert_eq!(propulsion_slot(4 | 1), Some(0));
        assert_eq!(propulsion_slot(1 | 2), Some(2));
        assert_eq!(propulsion_slot(2), Some(1));
        assert_eq!(propulsion_slot(0), None);
        assert_eq!(CellMotion::default().propulsion, [-1.0; 3]);
    }

    #[test]
    fn thrust_reaches_toward_the_target_along_the_facing() {
        let dv = thrust(Vec2::X, Vec2::new(10.0, 0.0), Vec2::ZERO, Vec2::ZERO, 3.0, 1.0, 1.0, 1, false, 0.5);
        assert!((dv - Vec2::new(1.5, 0.0)).length() < 1e-5, "{dv}");
        // Facing away: no thrust (alignment 0).
        let dv = thrust(Vec2::NEG_X, Vec2::new(10.0, 0.0), Vec2::ZERO, Vec2::ZERO, 3.0, 1.0, 1.0, 1, false, 0.5);
        assert!(dv.length() < 1e-5);
    }

    /// Retail numbers for the avatar's cruise (MoveToTarget 0xE6EEC0, Thrust
    /// 0xE55C40): the speed argument is `G(flagella_count, n9)` (2 -> 3, 4 -> 4,
    /// 6 -> 5 world units per second, `has GFX` 1, `SizeDivisor` 1 for the
    /// avatar); the thrust aims the velocity at `flow + dir * r * speed` with
    /// `r = 1` from 3 units to the target on, by `dv = (X - v) * dt` (a time
    /// constant of one second), and nothing else limits the speed (no clamp in
    /// 0xE7C080 / 0xE59EE0).
    #[test]
    fn a_flagella_cell_cruises_at_the_graph_speed_with_a_time_constant_of_one_second() {
        let mut graphs = CellGraphs::default();
        graphs
            .graphs
            .insert(CellGraphs::FLAGELLA_COUNT, vec![[0.0, 0.0], [2.0, 3.0], [4.0, 4.0], [6.0, 5.0], [8.0, 6.0], [10.0, 7.0]]);
        let globals = crate::cell_data::CellGlobals::parse(&vec![0u8; 0x114]).unwrap();
        for (n9, speed) in [(2u32, 3.0f32), (4, 4.0), (6, 5.0), (8, 6.0)] {
            let mut n = [0; 13];
            n[9] = n9;
            let caps = CellCaps::from_counts(n, &graphs, &globals);
            assert_eq!((caps.style, caps.speed), (1, speed));
            // Open water (flow 0), a target far ahead: v -> speed, dv = (speed - v) dt.
            let dt = 1.0 / 60.0;
            let (mut v, mut pos) = (Vec2::ZERO, Vec2::ZERO);
            let target = Vec2::new(500.0, 0.0);
            for step in 1..=300 {
                v += thrust(Vec2::X, target - pos, v, Vec2::ZERO, caps.speed, 1.0, 1.0, 1, false, dt);
                pos += v * dt;
                let expect = speed * (1.0 - (1.0 - dt).powi(step));
                assert!((v.x - expect).abs() < 1e-3 * speed, "n9 {n9} step {step}: {} vs {expect}", v.x);
            }
            assert!(v.x > 0.99 * speed, "5 s in: {}", v.x);
            // In a current the target velocity is the flow plus the thrust.
            let flow = Vec2::new(0.4, 0.2);
            let mut v = flow;
            for _ in 0..600 {
                v += thrust(Vec2::X, target - pos, v, flow, caps.speed, 1.0, 1.0, 1, false, dt);
            }
            assert!((v - (flow + Vec2::new(speed, 0.0))).length() < 1e-2 * speed, "{v:?}");
        }
        // The ramp r: 0.5 at 0.5 units to the target (a keyboard target is 20 ahead), 1 from 3.
        let near = thrust(Vec2::X, Vec2::new(0.5, 0.0), Vec2::ZERO, Vec2::ZERO, 3.0, 1.0, 1.0, 1, false, 1.0);
        let far = thrust(Vec2::X, Vec2::new(3.0, 0.0), Vec2::ZERO, Vec2::ZERO, 3.0, 1.0, 1.0, 1, false, 1.0);
        assert!((near.x - 1.5).abs() < 1e-5 && (far.x - 3.0).abs() < 1e-5, "{near:?} {far:?}");
    }

    /// The same cruise through the soft body (case (a) of 0xE7C080): the thrust
    /// goes into the velocity of every pin and the cell's velocity is their
    /// mean (0xE553A0). With no water around (the pins of one cell exert no net
    /// force on each other: the SPH pressure and viscosity terms are
    /// antisymmetric) the avatar reaches the same `3 (1 - e^-t)` as the free
    /// cell; the other particles' viscosity is what slows it in thick water.
    #[test]
    fn the_avatar_cruises_through_its_pins_like_the_free_cell_in_open_water() {
        use crate::cell_fluid::{CellFluid, STEP};
        let mut f = CellFluid::with_particles(Vec2::new(-10.0, -7.5), &[]);
        // Retail's avatar: size 0.476, level 1, four pins; start well inside the box.
        let size = 0.476;
        let mut body = crate::cell_softbody::SoftBody::create(&mut f, 1, size, Vec2::new(-8.0, 0.0), Vec2::ZERO, 0.0).unwrap();
        assert_eq!(body.pins.len(), 4);
        for step in 1..=240 {
            body.match_shape(&mut f, size, false, STEP);
            let (_, v) = body.read_back(&f);
            let dv = thrust(Vec2::X, Vec2::new(500.0, 0.0), v, Vec2::ZERO, 3.0, 1.0, 1.0, 1, false, STEP);
            body.add_velocity(&mut f, dv);
            f.step(STEP);
            let (pos, v) = body.read_back(&f);
            let expect = 3.0 * (1.0 - (1.0 - STEP).powi(step));
            assert!((v.x - expect).abs() < 0.02, "step {step}: {} vs {expect}", v.x);
            assert!(pos.x < 10.0, "still inside the box");
        }
        let (pos, v) = body.read_back(&f);
        assert!(v.x > 2.9 && v.x < 3.01 && v.y.abs() < 1e-3, "{v:?}");
        assert!(pos.x > -8.0 + 6.0, "{pos:?}");
    }

    #[test]
    fn screen_up_maps_to_bevy_minus_z() {
        assert_eq!(to_bevy(Vec2::new(0.0, 1.0)), Vec2::new(0.0, -1.0));
    }

    #[test]
    fn the_thrust_factor_follows_the_clip_in_the_swim_states_and_is_cached_outside_them() {
        let mut m = CellMotion::default();
        // No clip data in a swim state: the non-animating constant (6 for cilia).
        assert_eq!(thrust_factor(&mut m, true, None, 20.0, 6.0), 6.0);
        // A keyed Data3 of 0.5: x 20 for the cilia, cached.
        assert_eq!(thrust_factor(&mut m, true, Some(0.5), 20.0, 6.0), 10.0);
        // Outside the swim states the last value stays (0xE56300 returns +0x1C0).
        assert_eq!(thrust_factor(&mut m, false, Some(0.9), 20.0, 6.0), 10.0);
        let mut fresh = CellMotion::default();
        assert_eq!(thrust_factor(&mut fresh, false, None, 100.0, 250.0), 250.0);
    }

    /// Calibration check for the animated thrust factor: the speed S of a one-part avatar
    /// per style against the terminal velocity S x K the constants and the clips' Data3 give
    /// (printed; the retail path is K = Data3 x 20 / x 100, 0xE56300 / 0xE56200).
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_thrust_calibration() {
        let install = crate::spore_install::discover().unwrap();
        let db = CellDatabase::load(&crate::asset_catalog::ResourceIndex::load(&install.packages));
        let g = db.globals.as_ref().expect("globals");
        println!("non-animating K: cilia {} jet {}", g.non_animating_cilia_movement_factor, g.non_animating_jet_movement_factor);
        for (name, slot) in [("cilia", 8usize), ("flagella", 9), ("jet", 10)] {
            for count in [1u32, 2, 4] {
                let mut n = [0u32; 13];
                n[slot] = count;
                let caps = CellCaps::from_counts(n, &db.graphs, g);
                println!("{name} x{count}: style {} speed S {:.3}", caps.style, caps.speed);
            }
        }
    }

    /// Whether any installed part has a sideways box centre (row 82 of the audit): the stored boxes' centre x
    /// per part model and the stand-in's, printed with the largest value.
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_part_box_centres() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let blocks = installed_blocks(&index);
        let (mut worst, mut worst_stand_in) = (0.0f32, 0.0f32);
        for (instance, stored) in &blocks.boxes {
            let Some(def) = catalog.by_model_in(crate::asset_catalog::CELL_PARTS, *instance) else {
                continue;
            };
            let stand_in = model_bounds(&index, def.model).map_or(0.0, |b| b.cx * def.model_scale);
            println!("{instance:08X} {:<14} stored cx {:.4} cy {:.3} | stand-in cx {stand_in:.4}", def.name, stored.cx, stored.cy);
            worst = worst.max(stored.cx.abs());
            worst_stand_in = worst_stand_in.max(stand_in.abs());
        }
        println!("largest |cx|: stored {worst:.4}, stand-in {worst_stand_in:.4} creation units");
    }

    /// The rigblock boxes the installed Cell creations store per part model, against the
    /// stand-in (the model's bounds x modelScale) used for a part no creation uses, and the
    /// starter body's radius in creation units.
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_part_boxes_and_stand_ins() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let blocks = installed_blocks(&index);
        let (mut checked, mut close) = (0, 0);
        for (instance, stored) in &blocks.boxes {
            let Some(def) = catalog.by_model_in(crate::asset_catalog::CELL_PARTS, *instance) else {
                continue;
            };
            let Some(b) = model_bounds(&index, def.model) else { continue };
            let (dx, dz) = (b.dx * def.model_scale, b.dz * def.model_scale);
            let ratio = (stored.dx.abs().max(stored.dz.abs())) / dx.abs().max(dz.abs()).max(1e-6);
            checked += 1;
            close += usize::from((0.8..1.25).contains(&ratio));
            println!("{instance:08X} {:<14} stored dx {:.3} dz {:.3} cy {:.3} | stand-in dx {dx:.3} dz {dz:.3} cy {:.3} | larger-extent ratio {ratio:.2}", def.name, stored.dx, stored.dz, stored.cy, b.cy * def.model_scale);
        }
        println!("{close}/{checked} stand-in boxes within 20 / 25 % of the stored box");
        println!("starter body radius: {:?}", player_body_radius(&index));
        // Only the poison gland carries a +0x60 emit offset, (0, -0.257, 0).
        let off = emit_offsets(&index);
        println!("emit offsets: {off:?}");
        assert!(off.get(&0x6402_8FE0).is_some_and(|v| v.x == 0.0 && v.z == 0.0 && (v.y + 0.257).abs() < 0.001));
        assert!(checked > 10 && blocks.boxes.len() > 10);
        assert!(player_body_radius(&index).is_some_and(|r| r > 0.0));
    }
}
