//! The Cell body as a skinned vertebra chain (helper `cell-body`, 2026-10-27).
//!
//! Retail's Cell Creator edits a chain of `cl_vertebra` rigblocks (`CellTemplate`
//! `editorUseSkin` 1, `editorInitSpine` 1, `editorVertebraModel`
//! `cell_parts~!cl_vertebra`, manipulators `CellPinning`, `CellTorso`,
//! `DeformHandle`, `RotationHandle`, `SpineScaling`) and skins it with the same
//! metaball Skinner as a creature (`creature_skin`), at
//! `editorSkinResolutionRealtime` = `...HighQuality` = 0.035. The installed
//! starter body (baked GMDL `0x40617100!A1C46DF2`) is that skin's output for the
//! neutral anatomy `0x40616200!A1C46DF2`, six vertebrae.
//!
//! * [`Vertebra`] / [`chain_of_anatomy`]: the chain, read from the anatomy's
//!   type-5 rigblocks (the neutral cell; the `.cll` startup creations are the
//!   same record layout in XML).
//! * [`wheel`]: `SpineScaling`, the mouse wheel over the torso: retail
//!   `cEditor::sub_581F70` 0x582250 (VERIFIED, `re\wheel_582250.asm`): vertebra
//!   size += 0.05 x notch (0x4A6040 -> SetScale 0x49EBB0 clamps to
//!   `modelMinScale` 0.4 .. `modelMaxScale` 1.2 of `cl_vertebra`), then up to
//!   five vertebrae each way along the chain follow with a falloff weight
//!   (`editorSpineScalingNeighborInfluence` is the 2 of the template, not read
//!   by this code path). Vertebrae never move when one is scaled: SetScale's
//!   child pass 0x49E340 skips vertebra children and moves only the attached
//!   parts radially ([`follow_parts`]).
//! * [`build`] / [`render_mesh`]: the skin of a chain and its render mesh in the
//!   player's body frame (`cell_visuals::player_body`'s fit), UVs from retail's own
//!   chart flood and atlas packing (`cEditor` 0x4CB890: 0x5094F0 adjacency, 0x50A140
//!   flood over the six cube-map directions, 0x50AB80 packing; ported as
//!   `creature_skin::unwrap_retail`, VERIFIED to reproduce the baked starter body's
//!   atlas exactly, `notes\cell-body-notes.md`). The mesh itself comes from the Cell-only
//!   entry point `creature_skin::build_retail`: retail's own marching-cubes polygon walk
//!   (0x53FE30, tables of 0x53D2B0), so the triangles (not only the vertices) are the
//!   retail skin's and the chart borders of an edited body fall where retail's would.
//!   ADAPTED: [`NEUTRAL_PART_RECTS`], the starter's four part rectangles share the atlas
//!   whatever is mounted.
//! * [`CellBodyPlugin`]: writes the edited body into the mesh asset the stage,
//!   the Creator and the HUD portrait already share (the baked body when the
//!   chain is the neutral one, so an unedited cell looks exactly as before).

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::{ResourceIndex, CELL_PARTS},
    cell_editor_data::{prop_id, prop_key},
    creature_skin::{self as skin, BlockKind, SkinRecord},
    dbpf::ResourceKey,
    game::{PartInstance, RunProgress},
    world_objects::name_id,
    Originals,
};

/// The neutral cell's anatomy (`NeutralCell`, group `0x40616200`).
pub const NEUTRAL_ANATOMY: ResourceKey = ResourceKey {
    group_id: 0x4061_6200,
    instance_id: 0xA1C4_6DF2,
    type_id: crate::creature_data::TYPE,
};
/// `cell_parts~!cl_vertebra` (`editorVertebraModel`).
pub const VERTEBRA_PART: u32 = 0xC15C_FA84;
/// `editorSkinResolutionRealtime` / `HighQuality` of `CellTemplate` (VERIFIED,
/// `cell-editor\research\templates_dump.txt`): the metaball step.
pub const SKIN_RESOLUTION: f32 = 0.035;
/// `cl_vertebra` `modelMinScale` / `modelMaxScale` (read from the PROP when the
/// install is there; these are its values).
pub const MIN_SCALE: f32 = 0.4;
pub const MAX_SCALE: f32 = 1.2;
/// Vertebra size added per wheel notch: `0.1 * notch`, halved by the store into
/// the step variable before 0x4A6040 (0x5822EC `fmul 0.5`, `fst` back).
pub const WHEEL_STEP: f32 = 0.05;
/// Neighbours that follow a scaled vertebra, each way (0x582363 `cmp ebp, 5`).
pub const NEIGHBOR_REACH: usize = 5;
/// ADAPTED: a chain within this (per value) of the neutral one is the neutral
/// one (the baked body is shown and no chain is saved).
const SAME: f32 = 1e-4;

/// One vertebra record: what the Skinner reads of a `cl_vertebra` rigblock
/// (`RigblockData` +0x2C scale, +0x30.. orientation rows, +0x54 position), in
/// creature space (Z up, front -Y).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertebra {
    pub scale: f32,
    pub position: [f32; 3],
    pub rows: [[f32; 3]; 3],
}

impl Vertebra {
    fn record(&self, index: usize) -> SkinRecord {
        SkinRecord {
            index: index as i16,
            parent: index as i16 - 1,
            flags: skin::flag::USES_SKIN,
            kind: BlockKind::Vertebra,
            scale: self.scale,
            orientation: self.rows.map(Vec3::from_array),
            position: Vec3::from_array(self.position),
            ..Default::default()
        }
    }
    /// The skin ball: centre and support radius (0x4A6220 / 0x4A6150 -> 0x4A6180
    /// / 0x4A6110, `SkinRecord::vertebra_ball`).
    pub fn ball(&self) -> (Vec3, f32) {
        self.record(0).vertebra_ball()
    }
    fn at(&self) -> Vec3 {
        Vec3::from_array(self.position)
    }
    /// Local axes (row-vector convention, `SkinRecord::rotate`).
    fn rotate(&self, v: Vec3) -> Vec3 {
        Vec3::from_array(self.rows[0]) * v.x + Vec3::from_array(self.rows[1]) * v.y + Vec3::from_array(self.rows[2]) * v.z
    }
    fn unrotate(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            v.dot(Vec3::from_array(self.rows[0])),
            v.dot(Vec3::from_array(self.rows[1])),
            v.dot(Vec3::from_array(self.rows[2])),
        )
    }
}

pub type Chain = Vec<Vertebra>;

/// The chain of an anatomy: its leading run of vertebra rigblocks in order.
pub fn chain_of_anatomy(data: &crate::creature_data::CreatureData) -> Chain {
    skin::records_from_creature_data(data)
        .iter()
        .take_while(|r| r.kind == BlockKind::Vertebra)
        .map(|r| Vertebra {
            scale: r.scale,
            position: r.position.to_array(),
            rows: r.orientation.map(|v| v.to_array()),
        })
        .collect()
}

/// Side length of the atlas rectangle of every part rigblock of an anatomy, in rigblock
/// order (`cEditor` 0x4CB890, VERIFIED: for each record of the editor model whose flag
/// word (+0x08) has bit 0 clear, i.e. not a vertebra: `0.4 x |bbox max - bbox min|
/// (+0x14 / +0x20) x scale (+0x2C) + 0.08`, the size times 2.2 first when the record's
/// capability tags include `mout`, a mouth). The rectangles share the atlas with the
/// body's charts (`creature_skin::unwrap_retail`).
pub fn part_rect_sizes(data: &crate::creature_data::CreatureData) -> Vec<f32> {
    data.rigblocks
        .iter()
        .filter(|b| b.flags() & 1 == 0)
        .map(|b| {
            let bb = b.bounding_box_bits().map(f32::from_bits);
            let size = (Vec3::new(bb[3], bb[4], bb[5]) - Vec3::new(bb[0], bb[1], bb[2])).length();
            let mut side = size * f32::from_bits(b.scale_bits());
            let first = b.capability_index().max(0) as usize;
            let count = b.capability_count().max(0) as usize;
            if data.capability_ids.get(first..first + count).is_some_and(|tags| tags.iter().any(|t| t == b"mout")) {
                side *= 2.2;
            }
            side * 0.4 + 0.08
        })
        .collect()
}

/// Skin records of a chain (spine first, chain-start flag and bone numbers as
/// retail's order `FUN_00461B50`).
pub fn records(chain: &[Vertebra]) -> Vec<SkinRecord> {
    skin::order_records(chain.iter().enumerate().map(|(i, v)| v.record(i)).collect())
}

/// Whether two chains are the same body.
pub fn same_chain(a: &[Vertebra], b: &[Vertebra]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            (x.scale - y.scale).abs() < SAME
                && x.position.iter().zip(&y.position).all(|(p, q)| (p - q).abs() < SAME)
                && x.rows.iter().flatten().zip(y.rows.iter().flatten()).all(|(p, q)| (p - q).abs() < SAME)
        })
}

// ---------------------------------------------------------------------------
// SpineScaling: the wheel over the torso (0x582250)
// ---------------------------------------------------------------------------

/// Scale vertebra `index` by `notches` wheel notches and let its neighbours
/// follow (`cEditor::sub_581F70` 0x582250, VERIFIED):
/// 1. the vertebra's size changes by `0.05 * notches` (clamped to the part's
///    scale limits, 0x49E1B0);
/// 2. walking away from it along the chain (children first, then parents), up
///    to five vertebrae: `d` = distance from the neighbour's position to the
///    scaled vertebra's skin ball, `r` its support radius (0x4A6150 of the new
///    size); `w = 1 - 0.7 d / r` inside the radius, else `0.3 r / d`; `w` is
///    multiplied by 0.4 when `(step > 0) != (scaled.size > neighbour.size)`,
///    capped at 0.7 x the previous weight (the first compares against 1) and
///    the walk stops below 0.1; the neighbour's size changes by `w * step`.
///
/// Returns whether any size changed.
pub fn wheel(chain: &mut [Vertebra], index: usize, notches: f32, min: f32, max: f32) -> bool {
    if index >= chain.len() || notches == 0.0 {
        return false;
    }
    let step = WHEEL_STEP * notches;
    let before: Vec<f32> = chain.iter().map(|v| v.scale).collect();
    chain[index].scale = (chain[index].scale + step).clamp(min, max);
    let (centre, radius) = chain[index].ball();
    let scaled = chain[index].scale;
    let walk = |chain: &mut [Vertebra], order: Vec<usize>| {
        let mut previous = 1.0f32;
        for i in order.into_iter().take(NEIGHBOR_REACH) {
            let d = (chain[i].at() - centre).length();
            let mut w = if d < radius { 1.0 - 0.7 * d / radius } else { radius / d * 0.3 };
            if (step > 0.0) != (scaled > chain[i].scale) {
                w *= 0.4;
            }
            w = w.min(previous * 0.7);
            if w < 0.1 {
                break;
            }
            chain[i].scale = (chain[i].scale + w * step).clamp(min, max);
            previous = w;
        }
    };
    let len = chain.len();
    walk(chain, (index + 1..len).collect());
    walk(chain, (0..index).rev().collect());
    chain.iter().zip(&before).any(|(v, b)| (v.scale - b).abs() > 1e-6)
}

/// The vertebra a point (creature space) is over: the nearest skin ball.
/// ADAPTED: retail picks the vertebra's hull mesh with the cursor ray (model
/// groups 0x9138FD8D / 0x513CDFC1); the hull geometry is not decoded, so the
/// vertebra whose ball is nearest to where the ray meets the skin is taken.
pub fn vertebra_at(chain: &[Vertebra], point: Vec3) -> Option<usize> {
    chain
        .iter()
        .enumerate()
        .map(|(i, v)| (i, (v.ball().0 - point).length() / v.ball().1.max(1e-3)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Where a mounted part goes when vertebrae change size (SetScale's child pass
/// 0x49E340, VERIFIED shape): the part's offset from its parent vertebra, in
/// the vertebra's frame, is scaled by `new / old` in x and z and kept in y.
/// The parent is the nearest vertebra (ADAPTED: retail keeps the parent the
/// pinning manipulator chose when the part was placed; OpenSpore saves only the
/// position).
pub fn follow_point(old: &[Vertebra], new: &[Vertebra], point: Vec3) -> Vec3 {
    let Some(i) = vertebra_at(old, point) else {
        return point;
    };
    let (a, b) = (&old[i], &new[i]);
    if (a.scale - b.scale).abs() < 1e-7 || a.scale.abs() < 1e-6 {
        return point;
    }
    let ratio = b.scale / a.scale;
    let local = a.unrotate(point - a.at());
    b.at() + b.rotate(Vec3::new(local.x * ratio, local.y, local.z * ratio))
}

// ---------------------------------------------------------------------------
// The vertebra overlay: rollover colours, fade and hull picking
// ---------------------------------------------------------------------------

/// The colours and fade of the vertebra overlay: `CellTemplate`
/// `tuningVertebraTorsoRolloverColor` (0.75, 0.68, 0.65, 0.35), `...SelectedColor`
/// (alpha 0.8), `tuningVertebraHighlightColor` (alpha 1), `...UnselectedColor`
/// (alpha 0) and `tuningVertebraFadeTime` 0.5 s, parsed by
/// `Editor_TuningSpine_ParseProp` 0x5DBD60 into `cTuningSpine` +0x20 / +0x30 / +0x40
/// / +0x50 and +0xB4 (VERIFIED: the property ids 0x4FE1603 .. 0x4FE1607 in that
/// order, the code reads the fields at those offsets in 0x575520).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertebraTuning {
    pub rollover: [f32; 4],
    pub selected: [f32; 4],
    pub highlight: [f32; 4],
    pub unselected: [f32; 4],
    pub fade: f32,
}

impl Default for VertebraTuning {
    fn default() -> Self {
        Self {
            rollover: [0.75, 0.68, 0.65, 0.35],
            selected: [0.75, 0.68, 0.65, 0.8],
            highlight: [0.75, 0.68, 0.65, 1.0],
            unselected: [0.75, 0.68, 0.65, 0.0],
            fade: 0.5,
        }
    }
}

impl VertebraTuning {
    pub fn from_prop(p: &crate::prop::PropertyList) -> Self {
        let mut out = Self::default();
        let colour = |name: &str, slot: &mut [f32; 4]| {
            if let Some(c) = p.0.get(&name_id(name)).and_then(|x| x.vector4()) {
                *slot = c;
            }
        };
        colour("tuningVertebraTorsoRolloverColor", &mut out.rollover);
        colour("tuningVertebraTorsoSelectedColor", &mut out.selected);
        colour("tuningVertebraHighlightColor", &mut out.highlight);
        colour("tuningVertebraUnselectedColor", &mut out.unselected);
        if let Some(t) = p.0.get(&name_id("tuningVertebraFadeTime")).and_then(|x| x.number()) {
            out.fade = t;
        }
        out
    }

    /// The colour a vertebra's overlay goes to (`cEditor` 0x575520, VERIFIED): the
    /// highlight colour for the vertebra under the cursor (the hovered rigblock,
    /// `+0xCC`), else the selected colour while the torso is selected (`+0xE9`),
    /// else the rollover colour while the cursor is over the torso (`+0xE8`), else the
    /// unselected one. (The code also takes the selected colour when the Creature's
    /// `Spine` / `SpineResize` manipulators are enabled; the Cell has neither.)
    pub fn target(&self, rollover: bool, selected: bool, highlighted: bool) -> [f32; 4] {
        if highlighted {
            self.highlight
        } else if selected {
            self.selected
        } else if rollover {
            self.rollover
        } else {
            self.unselected
        }
    }
}

/// One step of the overlay's alpha towards its target (0x575520): the colour is set
/// at once, the alpha moves `dt / fade` per step (a zero fade time jumps), and lands on
/// the target when the step reaches it.
pub fn fade_alpha(current: f32, target: f32, dt: f32, fade: f32) -> f32 {
    let step = if fade != 0.0 { dt / fade } else { 1.0 };
    if (target - current).abs() < step {
        target
    } else if current < target {
        current + step
    } else {
        current - step
    }
}

/// The vertebra's overlay model (`cl_vertebra`, `modelMeshLOD0`) and its picking hull
/// (`ce_vertebra-Hull`, `modelMeshHull`), in the vertebra's own frame (x, z across the
/// spine, y along it; the model sits 0.16 above the skin ball's centre).
#[derive(Clone, Debug, Default)]
pub struct VertebraModels {
    /// Triangles of the hull.
    pub hull: Vec<[Vec3; 3]>,
    /// The overlay model: positions, normals, triangle indices.
    pub overlay: Option<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>)>,
}

impl VertebraModels {
    pub fn load(index: &ResourceIndex) -> Self {
        let read = |name: &str| {
            let key = ResourceKey { group_id: 0x4060_6000, instance_id: name_id(name), type_id: crate::asset_catalog::RW4 };
            index.read(key).ok().map(|bytes| {
                let mut stats = crate::rw4::MeshStats::default();
                crate::rw4::extract_meshes(&bytes, &mut stats)
            })
        };
        let mut out = Self::default();
        for mesh in read("ce_vertebra-Hull").unwrap_or_default() {
            let p = |i: u16| mesh.positions.get(i as usize).map(|v| Vec3::from_array(*v));
            out.hull.extend(mesh.indices.chunks_exact(3).filter_map(|t| Some([p(t[0])?, p(t[1])?, p(t[2])?])));
        }
        let (mut positions, mut normals, mut indices) = (vec![], vec![], vec![]);
        for mesh in read("cl_vertebra").unwrap_or_default() {
            let base = positions.len() as u32;
            let n = mesh.positions.len();
            positions.extend(mesh.positions.iter().copied());
            normals.extend((0..n).map(|i| mesh.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0])));
            indices.extend(mesh.indices.iter().map(|i| base + *i as u32));
        }
        if !indices.is_empty() {
            out.overlay = Some((positions, normals, indices));
        }
        out
    }
}

fn ray_triangle(origin: Vec3, dir: Vec3, tri: &[Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (tri[1] - tri[0], tri[2] - tri[0]);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - tri[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-6).then_some(t)
}

/// The vertebra whose hull a ray (creature space) meets first, with the ray parameter
/// (retail picks the models of the groups 0x9138FD8D / 0x513CDFC1, the vertebra
/// hulls, `cEditor::sub_5858F0` 0x585B2A, and the hovered rigblock follows,
/// 0x58BCxx). Each hull is the model's `ce_vertebra-Hull` placed at the vertebra
/// (rows, position) and scaled by its size.
pub fn pick_vertebra(chain: &[Vertebra], hull: &[[Vec3; 3]], origin: Vec3, dir: Vec3) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (i, v) in chain.iter().enumerate() {
        let s = v.scale.max(1e-4);
        // Into the vertebra's frame; the parameter is the same in both frames.
        let (o, d) = (v.unrotate(origin - v.at()) / s, v.unrotate(dir) / s);
        for tri in hull {
            if let Some(t) = ray_triangle(o, d, tri) {
                if best.is_none_or(|(_, b)| t < b) {
                    best = Some((i, t));
                }
            }
        }
    }
    best
}

impl Vertebra {
    /// The overlay model's placement in the body mesh frame (a `Transform` of the
    /// body's own space): vertebra frame (rows, position, size) -> creature space ->
    /// the fit.
    pub fn overlay_transform(&self, frame: &BodyFrame) -> Transform {
        let s = self.scale;
        let local = Mat4::from_cols(
            (Vec3::from_array(self.rows[0]) * s).extend(0.0),
            (Vec3::from_array(self.rows[1]) * s).extend(0.0),
            (Vec3::from_array(self.rows[2]) * s).extend(0.0),
            self.at().extend(1.0),
        );
        // The GMDL basis change `(x, y, z) -> (-x, z, y)` of `BodyFrame::point`.
        let basis = Mat4::from_cols(Vec4::new(-1.0, 0.0, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0, 0.0), Vec4::new(0.0, 1.0, 0.0, 0.0), Vec4::W);
        let fit = Mat4::from_scale_rotation_translation(frame.fit.scale, frame.fit.rotation, frame.fit.translation);
        Transform::from_matrix(fit * basis * local)
    }
}

// ---------------------------------------------------------------------------
// Skin
// ---------------------------------------------------------------------------

/// A built body: the skin, its unwrapped render vertices and the vertebra each
/// of them belongs to.
pub struct BodySkin {
    pub unwrapped: skin::Unwrapped,
    /// Dominant vertebra (chain index) of every unwrapped vertex.
    pub bones: Vec<u8>,
    pub resolution: f32,
}

/// Atlas rectangle sides of the neutral anatomy's part rigblocks (two eyes, the filter mouth, the
/// flagellum): [`part_rect_sizes`] of `NeutralCell` (VERIFIED data, test
/// `neutral_part_rectangles_are_the_constant`). ADAPTED: retail packs the rectangles of the
/// parts mounted on the cell together with the body's charts; OpenSpore paints the mounted parts
/// into textures of their own and keeps one atlas layout per chain, the layout of the baked
/// starter body (so an edited body has the starter's texel density and chart arrangement).
pub const NEUTRAL_PART_RECTS: [f32; 4] = [0.232_146_7, 0.232_146_7, 0.250_967_68, 0.277_662_7];

/// Skin a chain at `res` with the starter's part rectangles in the atlas ([`NEUTRAL_PART_RECTS`]).
pub fn build(chain: &[Vertebra], res: f32) -> Option<BodySkin> {
    build_with_parts(chain, res, &NEUTRAL_PART_RECTS)
}

/// Skin a chain at `res` (the creature Skinner, 0xFE..: metaball polygonizer with retail's own
/// triangulation, [`skin::build_retail`] (the table walk 0x53FE30 whose tables 0x53D2B0 builds), weights,
/// then the retail chart flood and atlas packing of `cEditor` 0x4CB890, [`skin::unwrap_retail`]). `part_rects` are the atlas rectangles of the part
/// rigblocks that share the atlas with the body ([`part_rect_sizes`]).
pub fn build_with_parts(chain: &[Vertebra], res: f32, part_rects: &[f32]) -> Option<BodySkin> {
    if chain.is_empty() {
        return None;
    }
    let (prims, mesh) = skin::build_retail(&records(chain), res).ok()?;
    let mut unwrapped = skin::unwrap_retail(&mesh, part_rects).unwrapped;
    if unwrapped.triangles.is_empty() {
        unwrapped = skin::unwrap(&prims, &mesh);
    }
    let bones = unwrapped
        .source
        .iter()
        .map(|&s| mesh.bones.get(s as usize).map_or(0, |b| b[0]))
        .collect();
    Some(BodySkin { unwrapped, bones, resolution: mesh.resolution })
}

/// Creature space -> the body mesh frame of `cell_visuals::player_body`: the
/// GMDL basis change `(x, y, z) -> (-x, z, y)` the baked body is imported with,
/// then its fit (`centered_fit`: scale and translation; the player's body is fitted head
/// first, `cell_visuals::head_first`, so creature -Y, the mouth end, is Bevy -Z).
/// VERIFIED (`neutral_skin_matches_the_baked_body`): the baked GMDL's vertex
/// positions are the creature Skinner's output for the neutral anatomy in the
/// anatomy's own coordinates (no extra turn), so a chain is meshed with the
/// body's own fit and the unedited chain lands exactly on the baked mesh.
#[derive(Clone, Copy, Debug)]
pub struct BodyFrame {
    pub fit: Transform,
}

impl BodyFrame {
    pub fn point(&self, creature: Vec3) -> Vec3 {
        self.fit.translation + self.fit.rotation * (Vec3::new(-creature.x, creature.z, creature.y) * self.fit.scale)
    }
    pub fn direction(&self, creature: Vec3) -> Vec3 {
        self.fit.rotation * Vec3::new(-creature.x, creature.z, creature.y)
    }
    /// A body-space ray in creature space (origin, direction; the parameter keeps its
    /// meaning, a unit of the body-space direction).
    pub fn ray(&self, origin: Vec3, dir: Vec3) -> (Vec3, Vec3) {
        let o = self.creature(origin);
        (o, self.creature(origin + dir) - o)
    }
    /// Inverse of [`BodyFrame::point`].
    pub fn creature(&self, body: Vec3) -> Vec3 {
        let b = self.fit.rotation.inverse() * (body - self.fit.translation) / self.fit.scale;
        Vec3::new(-b.x, b.z, b.y)
    }
}

/// The render mesh of a built body in the body frame (UV0 = the paint atlas).
pub fn render_mesh(body: &BodySkin, frame: &BodyFrame) -> Mesh {
    let un = &body.unwrapped;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        un.positions.iter().map(|p| frame.point(*p).to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        un.normals.iter().map(|n| frame.direction(*n).normalize_or(Vec3::Y).to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, un.uvs.clone());
    mesh.insert_indices(Indices::U32(un.triangles.iter().flat_map(|t| *t).collect()));
    if mesh.generate_tangents().is_err() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; un.positions.len()]);
    }
    mesh
}

// ---------------------------------------------------------------------------
// Resource and system
// ---------------------------------------------------------------------------

/// The Cell body data read from the install.
#[derive(Resource, Default)]
pub struct CellBody {
    pub loaded: bool,
    /// The neutral cell's chain (what the baked body is the skin of).
    pub neutral: Chain,
    pub min_scale: f32,
    pub max_scale: f32,
    pub resolution: f32,
    /// The neutral cell's own paint: the brush script id (`skineffect1`,
    /// properties +0x08) and the first colour (+0x20) of its anatomy. VERIFIED
    /// data: `0x791C8201` = `skinpaint_cell_amphibian` in (0.906, 0.749, 0.412),
    /// seed 1234 (+0x14): the baked starter body is that paint, so a body whose
    /// chain was edited and that was never painted shows it again.
    pub neutral_paint: Option<(u32, [f32; 3])>,
    /// Colours and fade of the vertebra overlay (`CellTemplate` `tuningVertebra*`).
    pub tuning: VertebraTuning,
    /// The vertebra's overlay model and picking hull.
    pub models: VertebraModels,
    /// Chain shown now (`None` = the baked body) and the mesh it was written to.
    applied: Option<Option<Chain>>,
    mesh: Option<AssetId<Mesh>>,
    /// Last builds by chain (the wheel revisits chains while undoing).
    cache: Vec<(Chain, std::sync::Arc<BodySkin>)>,
}

impl CellBody {
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self { min_scale: MIN_SCALE, max_scale: MAX_SCALE, resolution: SKIN_RESOLUTION, ..Default::default() };
        if index.records.is_empty() {
            return out;
        }
        out.loaded = true;
        match index.read(NEUTRAL_ANATOMY).and_then(|b| crate::creature_data::parse(&b)) {
            Ok(data) => {
                out.neutral = chain_of_anatomy(&data);
                let script = data.properties.get(8..12).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
                out.neutral_paint = script.filter(|s| *s != 0).zip(data.skin_colors().ok().map(|c| c[0]));
            }
            Err(e) => warn!("Cell body: neutral anatomy: {e}"),
        }
        if let Ok(p) = index.properties(prop_key(CELL_PARTS, VERTEBRA_PART)) {
            let number = |id| p.0.get(&id).and_then(|x| x.number());
            out.min_scale = number(prop_id::MIN_SCALE).unwrap_or(MIN_SCALE);
            out.max_scale = number(prop_id::MAX_SCALE).unwrap_or(MAX_SCALE);
        }
        if let Ok(p) = index.properties(prop_key(crate::cell_editor_data::EDITOR_SETUP_GROUP, name_id("CellTemplate"))) {
            if let Some(r) = p.0.get(&name_id("editorSkinResolutionHighQuality")).and_then(|x| x.number()) {
                if r > 0.0 {
                    out.resolution = r;
                }
            }
            out.tuning = VertebraTuning::from_prop(&p);
        }
        out.models = VertebraModels::load(index);
        out
    }

    /// The chain being edited: the saved one, else the neutral one.
    pub fn chain_of(&self, saved: &Option<Chain>) -> Chain {
        saved.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| self.neutral.clone())
    }

    /// The saved form of a chain: `None` when it is the neutral one.
    pub fn saved_form(&self, chain: Chain) -> Option<Chain> {
        (!same_chain(&chain, &self.neutral)).then_some(chain)
    }

    fn skin_of(&mut self, chain: &Chain) -> Option<std::sync::Arc<BodySkin>> {
        if let Some((_, b)) = self.cache.iter().find(|(c, _)| same_chain(c, chain)) {
            return Some(b.clone());
        }
        let started = std::time::Instant::now();
        let built = std::sync::Arc::new(build(chain, self.resolution)?);
        debug!(
            "Cell body: {} vertebrae -> {} vertices, {} triangles in {} ms",
            chain.len(),
            built.unwrapped.positions.len(),
            built.unwrapped.triangles.len(),
            started.elapsed().as_millis()
        );
        self.cache.insert(0, (chain.clone(), built.clone()));
        self.cache.truncate(4);
        Some(built)
    }
}

/// What the Creator's controls need of the body (one system parameter).
#[derive(bevy::ecs::system::SystemParam)]
pub struct SpineParams<'w> {
    pub body: Res<'w, CellBody>,
    pub visuals: Option<Res<'w, crate::cell_visuals::CellVisualAssets>>,
    pub meshes: Res<'w, Assets<Mesh>>,
}

impl SpineParams<'_> {
    /// The frame the body mesh is in (`None` without the installed body).
    pub fn frame(&self) -> Option<BodyFrame> {
        self.visuals.as_ref().and_then(|v| v.body_fit).map(|fit| BodyFrame { fit })
    }
    /// The body's current mesh as a picking shape.
    pub fn shape(&self) -> Option<crate::cell_editor_scene::BodyShape> {
        let model = self.visuals.as_ref()?.body.as_ref()?;
        self.meshes.get(&model.draw_mesh()).and_then(crate::cell_editor_scene::BodyShape::from_mesh)
    }
    pub fn revision(&self) -> u32 {
        self.visuals.as_ref().map_or(0, |v| v.body_revision)
    }
}

pub struct CellBodyPlugin;

impl Plugin for CellBodyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CellBody>()
            // Before the paint, which repaints the atlas for the new mesh.
            .add_systems(Update, (demo, sync).chain().before(crate::cell_paint::sync));
    }
}

/// `--cell-body-demo VERTEBRA NOTCHES [VERTEBRA NOTCHES ...]` (captures): give the
/// cell a scaled chain once the stage runs, without opening the Creator.
fn demo(mut run: ResMut<RunProgress>, originals: Res<Originals>, mut body: ResMut<CellBody>, mut done: Local<bool>) {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a == "--cell-body-demo") else {
        return;
    };
    if *done || originals.resource_index.records.is_empty() {
        return;
    }
    if !body.loaded {
        *body = CellBody::load(&originals.resource_index);
    }
    *done = true;
    let mut chain = body.neutral.clone();
    let numbers: Vec<f32> = args[at + 1..].iter().map_while(|a| a.parse::<f32>().ok()).collect();
    for pair in numbers.chunks_exact(2) {
        wheel(&mut chain, pair[0] as usize, pair[1], body.min_scale, body.max_scale);
    }
    run.cell_creation.spine = body.saved_form(chain);
}

/// Keep the shared body mesh on `run.cell_creation.spine`.
fn sync(
    run: Res<RunProgress>,
    originals: Res<Originals>,
    visuals: Option<ResMut<crate::cell_visuals::CellVisualAssets>>,
    mut body: ResMut<CellBody>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    if !body.loaded {
        *body = CellBody::load(&originals.resource_index);
    }
    let Some(mut visuals) = visuals else {
        return;
    };
    let (Some(model), Some(fit)) = (visuals.body.as_ref(), visuals.body_fit) else {
        return;
    };
    let handle = model.draw_mesh();
    // A new stage builds a new body mesh: nothing is applied to it yet.
    if body.mesh != Some(handle.id()) {
        body.mesh = Some(handle.id());
        body.applied = None;
    }
    let wanted = run.cell_creation.spine.clone().filter(|c| !c.is_empty() && !same_chain(c, &body.neutral));
    if body.applied.as_ref() == Some(&wanted) {
        return;
    }
    // The baked mesh is kept aside before the shared one is first replaced.
    if visuals.baked_mesh.is_none() {
        visuals.baked_mesh = meshes.get(&handle).cloned();
    }
    match &wanted {
        None => {
            if visuals.body_edit.is_some() {
                if let (Some(baked), Some(mut mesh)) = (visuals.baked_mesh.clone(), meshes.get_mut(&handle)) {
                    *mesh = baked;
                }
                visuals.body_edit = None;
                visuals.body_revision += 1;
            }
        }
        Some(chain) => {
            let Some(built) = body.skin_of(chain) else {
                warn!("Cell body: the {}-vertebra chain could not be skinned", chain.len());
                body.applied = Some(wanted);
                return;
            };
            if let Some(mut mesh) = meshes.get_mut(&handle) {
                *mesh = render_mesh(&built, &BodyFrame { fit });
            }
            visuals.body_edit = Some(built.bones.clone());
            visuals.body_revision += 1;
        }
    }
    body.applied = Some(wanted);
}

/// Move every mounted part with the vertebrae after an edit (`follow_point`
/// through the body frame).
pub fn follow_parts(parts: &mut [PartInstance], old: &[Vertebra], new: &[Vertebra], frame: &BodyFrame) {
    for part in parts {
        let at = crate::cell_editor_scene::point_of(part);
        let moved = frame.point(follow_point(old, new, frame.creature(at)));
        if (moved - at).length() > 1e-6 {
            part.offset = crate::cell_editor_scene::offset_for(moved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn index() -> Option<&'static ResourceIndex> {
        static INDEX: OnceLock<Option<ResourceIndex>> = OnceLock::new();
        INDEX
            .get_or_init(|| crate::spore_install::discover().ok().map(|i| ResourceIndex::load(&i.packages)))
            .as_ref()
            .filter(|i| !i.records.is_empty())
    }

    fn straight(scales: &[f32]) -> Chain {
        scales
            .iter()
            .enumerate()
            .map(|(i, s)| Vertebra { scale: *s, position: [0.0, i as f32 * 0.1117, 0.0], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] })
            .collect()
    }

    #[test]
    fn wheel_grows_the_vertebra_and_its_neighbours_with_the_retail_falloff() {
        let mut chain = straight(&[0.9, 0.9, 0.9, 0.9, 0.9, 0.9]);
        let (_, radius) = chain[2].ball();
        assert!(wheel(&mut chain, 2, 1.0, 0.4, 1.2));
        // The vertebra itself: + 0.05 per notch.
        assert!((chain[2].scale - 0.95).abs() < 1e-6);
        // Neighbours: w = 1 - 0.7 d / r inside the radius (d = 0.1117 from the
        // old position to the new ball), capped at 0.7 of the previous weight
        // and shrunk by 0.4 when the scaled vertebra is not the bigger one.
        let (centre, r) = chain[2].ball();
        assert!((r - radius).abs() > 0.0, "the radius follows the new size");
        let d = (Vec3::new(0.0, 0.1117 * 3.0, 0.0) - centre).length();
        let mut w = if d < r { 1.0 - 0.7 * d / r } else { r / d * 0.3 };
        // Growing and the scaled vertebra (0.95) is bigger than the neighbour (0.9): no 0.4.
        w = w.min(0.7);
        assert!((chain[3].scale - (0.9 + w * 0.05)).abs() < 1e-5, "{} vs {}", chain[3].scale, 0.9 + w * 0.05);
        // Farther vertebrae follow less; the walk stops below 0.1.
        let grown: Vec<f32> = chain.iter().map(|v| v.scale - 0.9).collect();
        assert!(grown[3] > grown[4] && grown[4] >= grown[5]);
        assert!(grown[1] > grown[0]);
        // Shrinking is the mirror image in size, not in the 0.4 rule.
        let mut shrink = straight(&[0.9; 6]);
        assert!(wheel(&mut shrink, 2, -1.0, 0.4, 1.2));
        assert!((shrink[2].scale - 0.85).abs() < 1e-6);
        assert!(shrink[3].scale < 0.9 && shrink[1].scale < 0.9);
    }

    #[test]
    fn wheel_clamps_to_the_part_limits_and_ignores_a_missing_vertebra() {
        let mut chain = straight(&[1.19, 1.19, 1.19]);
        wheel(&mut chain, 1, 3.0, 0.4, 1.2);
        assert!(chain.iter().all(|v| v.scale <= 1.2 + 1e-6));
        assert!((chain[1].scale - 1.2).abs() < 1e-6);
        let mut tiny = straight(&[0.41, 0.41]);
        wheel(&mut tiny, 0, -5.0, 0.4, 1.2);
        assert!(tiny.iter().all(|v| v.scale >= 0.4 - 1e-6));
        let before = chain.clone();
        assert!(!wheel(&mut chain, 9, 1.0, 0.4, 1.2));
        assert!(!wheel(&mut chain, 0, 0.0, 0.4, 1.2));
        assert_eq!(before, chain);
    }

    #[test]
    fn parts_follow_the_vertebra_radially_not_along_the_spine() {
        let old = straight(&[0.9, 1.0, 0.9]);
        let mut new = old.clone();
        new[1].scale = 1.2;
        // A point 0.3 to the side of the middle vertebra and 0.02 along it.
        let p = old[1].at() + Vec3::new(0.3, 0.02, 0.1);
        let q = follow_point(&old, &new, p);
        let ratio = 1.2 / 1.0;
        assert!((q - (new[1].at() + Vec3::new(0.3 * ratio, 0.02, 0.1 * ratio))).length() < 1e-6, "{q:?}");
        // No change, no move.
        assert_eq!(follow_point(&old, &old, p), p);
    }

    #[test]
    fn an_edited_body_is_a_paintable_surface_with_its_own_vertebrae() {
        // The render mesh of a synthetic chain in the body frame, read back by the paint's
        // `BodySurface` with the per-vertex vertebra of the skin.
        let chain = straight(&[0.7, 0.9, 1.1, 1.0, 0.8]);
        let built = build(&chain, SKIN_RESOLUTION).expect("skin");
        let frame = BodyFrame { fit: Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)).with_scale(Vec3::splat(2.0)) };
        let mesh = render_mesh(&built, &frame);
        use bevy::mesh::VertexAttributeValues as V;
        let (Some(V::Float32x3(p)), Some(V::Float32x3(n)), Some(V::Float32x2(uv))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL), mesh.attribute(Mesh::ATTRIBUTE_UV_0))
        else {
            panic!("mesh attributes")
        };
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
        assert_eq!((p.len(), n.len(), uv.len()), (built.unwrapped.positions.len(), built.unwrapped.positions.len(), built.bones.len()));
        let indices: Vec<u32> = built.unwrapped.triangles.iter().flatten().copied().collect();
        let surface = crate::cell_skinpaint::BodySurface::build(p, n, uv, &indices, 2.0, Some(&built.bones)).expect("paintable surface");
        assert_eq!(surface.vertebrae, chain.len(), "every vertebra owns skin");
        // The skin in the body frame comes back in creature space through the inverse.
        let q = Vec3::from_array(p[0]);
        assert!((frame.point(frame.creature(q)) - q).length() < 1e-5);
    }

    /// Developer aid (installed data, `--include-ignored`): the vertebra overlay model and the
    /// picking hull the retail editor draws and picks (`cl_vertebra`, `ce_vertebra-Hull`).
    #[test]
    #[ignore]
    fn vertebra_models_dump() {
        let Some(index) = index() else { return };
        for name in ["cl_vertebra", "ce_vertebra-Hull", "ce_vertebra"] {
            let key = ResourceKey { group_id: 0x4060_6000, instance_id: name_id(name), type_id: crate::asset_catalog::RW4 };
            let Ok(bytes) = index.read(key) else {
                eprintln!("VERT {name}: missing");
                continue;
            };
            let mut stats = crate::rw4::MeshStats::default();
            for m in crate::rw4::extract_meshes(&bytes, &mut stats) {
                let fold = |k: usize, f: fn(f32, f32) -> f32, init: f32| m.positions.iter().map(|p| p[k]).fold(init, f);
                eprintln!(
                    "VERT {name}: section {} {} vertices {} triangles, x {:.3}..{:.3} y {:.3}..{:.3} z {:.3}..{:.3}",
                    m.source_section,
                    m.positions.len(),
                    m.indices.len() / 3,
                    fold(0, f32::min, f32::MAX),
                    fold(0, f32::max, f32::MIN),
                    fold(1, f32::min, f32::MAX),
                    fold(1, f32::max, f32::MIN),
                    fold(2, f32::min, f32::MAX),
                    fold(2, f32::max, f32::MIN)
                );
                if name == "ce_vertebra-Hull" {
                    eprintln!("VERT hull positions {:?}", m.positions);
                    eprintln!("VERT hull indices {:?}", m.indices);
                }
            }
        }
    }

    #[test]
    fn the_overlay_fades_towards_the_tuning_colours_like_0x575520() {
        let t = VertebraTuning::default();
        // Highlight beats selected beats rollover beats unselected.
        assert_eq!(t.target(true, true, true)[3], 1.0);
        assert_eq!(t.target(true, true, false)[3], 0.8);
        assert_eq!(t.target(true, false, false)[3], 0.35);
        assert_eq!(t.target(false, false, false)[3], 0.0);
        // 0.5 s fade: a 0.1 s step moves 0.2 of alpha, and the last step lands.
        assert!((fade_alpha(0.0, 0.35, 0.1, 0.5) - 0.2).abs() < 1e-6);
        assert_eq!(fade_alpha(0.2, 0.35, 0.1, 0.5), 0.35);
        assert!((fade_alpha(1.0, 0.35, 0.1, 0.5) - 0.8).abs() < 1e-6);
        assert_eq!(fade_alpha(0.3, 0.9, 0.1, 0.0), 0.9, "no fade time: at once");
        assert_eq!(fade_alpha(0.7, 0.7, 0.1, 0.5), 0.7);
    }

    #[test]
    fn hull_picking_finds_the_vertebra_the_ray_passes_through() {
        // A box hull (x, z +-0.2, y +-0.05) at three vertebrae along y.
        let (a, b) = (Vec3::new(-0.2, -0.05, -0.2), Vec3::new(0.2, 0.05, 0.2));
        let c = |x: bool, y: bool, z: bool| Vec3::new(if x { b.x } else { a.x }, if y { b.y } else { a.y }, if z { b.z } else { a.z });
        let quad = |p: [Vec3; 4]| [[p[0], p[1], p[2]], [p[0], p[2], p[3]]];
        let mut hull = vec![];
        // The six faces of the box.
        for q in [
            [c(false, true, false), c(true, true, false), c(true, true, true), c(false, true, true)],
            [c(false, false, false), c(true, false, false), c(true, false, true), c(false, false, true)],
            [c(false, false, true), c(true, false, true), c(true, true, true), c(false, true, true)],
            [c(false, false, false), c(true, false, false), c(true, true, false), c(false, true, false)],
            [c(true, false, false), c(true, false, true), c(true, true, true), c(true, true, false)],
            [c(false, false, false), c(false, false, true), c(false, true, true), c(false, true, false)],
        ] {
            hull.extend(quad(q));
        }
        let chain = straight(&[1.0, 1.0, 0.5]);
        // Straight down (-Z) through the middle vertebra (y = 0.1117).
        let hit = pick_vertebra(&chain, &hull, Vec3::new(0.0, 0.1117, 1.0), Vec3::NEG_Z).expect("hit");
        assert_eq!(hit.0, 1);
        // Between the vertebrae (y = 0.055) nothing is hit.
        assert!(pick_vertebra(&chain, &hull, Vec3::new(0.0, 0.0558, 1.0), Vec3::NEG_Z).is_none());
        // Outside the scaled hull of the small vertebra (x = 0.15 > 0.5 * 0.2).
        assert!(pick_vertebra(&chain, &hull, Vec3::new(0.15, 0.2234, 1.0), Vec3::NEG_Z).is_none());
        assert_eq!(pick_vertebra(&chain, &hull, Vec3::new(0.05, 0.2234, 1.0), Vec3::NEG_Z).map(|h| h.0), Some(2));
    }

    #[test]
    fn the_overlay_transform_follows_point_and_scale() {
        let chain = straight(&[0.8, 1.1]);
        let frame = BodyFrame {
            fit: Transform::from_translation(Vec3::new(0.1, -0.2, 0.3)).with_scale(Vec3::splat(2.0)),
        };
        // A model point (0, 0, -0.157): the skin ball's centre, below the origin.
        let local = Vec3::new(0.03, 0.01, -0.157);
        let v = &chain[1];
        let expected = frame.point(v.at() + v.rotate(local * v.scale));
        let got = v.overlay_transform(&frame).transform_point(local);
        assert!((got - expected).length() < 1e-5, "{got:?} {expected:?}");
    }

    #[test]
    fn body_frame_round_trips() {
        let frame = BodyFrame {
            fit: Transform::from_translation(Vec3::new(0.1, -0.2, 0.3))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                .with_scale(Vec3::splat(2.5)),
        };
        let p = Vec3::new(0.12, -0.3, 0.07);
        assert!((frame.creature(frame.point(p)) - p).length() < 1e-5);
    }

    #[test]
    fn chain_serializes_and_old_saves_keep_the_baked_body() {
        let creation: crate::cell_editor::CellCreation = serde_json::from_str(r#"{"name":"Old"}"#).unwrap();
        assert!(creation.spine.is_none());
        let mut edited = creation.clone();
        edited.spine = Some(straight(&[0.5, 0.6]));
        let text = serde_json::to_string(&edited).unwrap();
        let back: crate::cell_editor::CellCreation = serde_json::from_str(&text).unwrap();
        assert_eq!(back, edited);
        // And inside a whole save: the chain is part of `RunProgress`.
        let mut run = RunProgress::default();
        run.cell_creation.spine = Some(straight(&[0.5, 0.6, 0.7]));
        let saved: RunProgress = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        assert_eq!(saved.cell_creation.spine, run.cell_creation.spine);
    }

    /// The skin of the neutral chain IS the baked starter body (installed data):
    /// same bounds, every vertex of either on a vertex of the other.
    #[test]
    fn neutral_skin_matches_the_baked_body() {
        let Some(index) = index() else { return };
        let body = CellBody::load(index);
        assert_eq!(body.neutral.len(), 6, "the neutral cell has six vertebrae");
        assert!((body.min_scale, body.max_scale) == (0.4, 1.2), "cl_vertebra limits {} {}", body.min_scale, body.max_scale);
        assert_eq!(body.resolution, 0.035);
        let (script, color) = body.neutral_paint.expect("the neutral cell's paint");
        assert_eq!(script, crate::world_objects::name_id("skinpaint_cell_amphibian"));
        assert!((color[0] - 0.905882).abs() < 1e-4 && (color[1] - 0.749019).abs() < 1e-4 && (color[2] - 0.411764).abs() < 1e-4);
        let raw = crate::baked_model::parse(
            &index
                .read(ResourceKey { group_id: crate::cell_visuals::CELL_MODEL_GROUP, instance_id: 0xA1C4_6DF2, type_id: crate::decode::TYPE_GMDL })
                .unwrap(),
        )
        .unwrap();
        // The baked body vertices (body-only triangles), in GMDL space.
        let mut baked: Vec<Vec3> = Vec::new();
        for (i, p) in raw.pieces.iter().enumerate() {
            let base = raw.bone_ranges[i][0];
            for tri in p.indices.chunks_exact(3) {
                let ok = tri.iter().all(|&v| (0..4).all(|j| p.weights[v as usize][j] == 0. || base + p.bones[v as usize][j] as u32 / 3 < raw.root_bones));
                if ok {
                    for &v in tri {
                        baked.push(Vec3::from_array(p.positions[v as usize]));
                    }
                }
            }
        }
        baked.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
        baked.dedup();
        let bounds = |pts: &[Vec3]| pts.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        let (bl, bh) = bounds(&baked);
        let built = build(&body.neutral, body.resolution).expect("neutral skin");
        let skin_pts: Vec<Vec3> = built.unwrapped.positions.clone();
        let (sl, sh) = bounds(&skin_pts);
        eprintln!("baked body {} vertices {bl:?} .. {bh:?}; skin {} vertices {sl:?} .. {sh:?}", baked.len(), skin_pts.len());
        assert!((bl - sl).abs().max_element() < 1e-4 && (bh - sh).abs().max_element() < 1e-4, "bounds differ");
        let worst = |a: &[Vec3], b: &[Vec3]| {
            a.iter().step_by(5).map(|p| b.iter().map(|q| p.distance(*q)).fold(f32::MAX, f32::min)).fold(0.0f32, f32::max)
        };
        assert!(worst(&skin_pts, &baked) < 1e-4, "skin vertex off the baked body");
        assert!(worst(&baked, &skin_pts) < 1e-4, "baked vertex off the skin");
        // The unwrap keeps the mesh closed and gives every vertex a vertebra.
        assert_eq!(built.bones.len(), skin_pts.len());
        assert!(built.bones.iter().all(|b| (*b as usize) < 6));
        assert!(built.unwrapped.uvs.iter().flatten().all(|v| (0.0..=1.0).contains(v)));
    }
    /// The baked starter body's triangles with their atlas coordinates (installed data):
    /// `(piece, vertex ids, positions, uvs)`; body triangles only (no part bones) or the parts'.
    fn baked_triangles(index: &ResourceIndex, body: bool) -> Vec<(usize, [u32; 3], [Vec3; 3], [[f32; 2]; 3])> {
        let raw = crate::baked_model::parse(
            &index
                .read(ResourceKey { group_id: crate::cell_visuals::CELL_MODEL_GROUP, instance_id: 0xA1C4_6DF2, type_id: crate::decode::TYPE_GMDL })
                .unwrap(),
        )
        .unwrap();
        let mut out = Vec::new();
        for (i, p) in raw.pieces.iter().enumerate() {
            let base = raw.bone_ranges[i][0];
            for tri in p.indices.chunks_exact(3) {
                let ok = tri.iter().all(|&v| (0..4).all(|j| p.weights[v as usize][j] == 0. || base + p.bones[v as usize][j] as u32 / 3 < raw.root_bones));
                if ok == body {
                    let v = [tri[0], tri[1], tri[2]];
                    out.push((i, v, v.map(|k| Vec3::from_array(p.positions[k as usize])), v.map(|k| p.uvs[k as usize])));
                }
            }
        }
        out
    }

    /// The baked body as a skin mesh (positions welded by value, triangles in the baked order)
    /// and the baked UVs of every triangle corner.
    fn baked_as_skin(index: &ResourceIndex) -> (skin::SkinMesh, Vec<[[f32; 2]; 3]>) {
        let baked = baked_triangles(index, true);
        let mut weld: std::collections::HashMap<[u32; 3], u32> = std::collections::HashMap::new();
        let mut mesh = skin::SkinMesh::default();
        let mut uvs = Vec::new();
        for (_, _, p, uv) in &baked {
            let ids = p.map(|q| {
                *weld.entry(q.to_array().map(f32::to_bits)).or_insert_with(|| {
                    mesh.positions.push(q);
                    mesh.normals.push(Vec3::Z);
                    mesh.positions.len() as u32 - 1
                })
            });
            mesh.triangles.push(ids);
            uvs.push(*uv);
        }
        (mesh, uvs)
    }

    /// The retail chart flood and atlas packing (0x50A140 / 0x50AB80, `creature_skin::unwrap_retail`)
    /// reproduce the baked starter body's atlas EXACTLY: run on the baked body's own triangles with
    /// the neutral anatomy's four part rectangles, every UV of every corner (5208) equals the baked one
    /// (installed data; the baked body is the editor's output for the neutral anatomy).
    #[test]
    fn unwrap_retail_reproduces_the_baked_atlas() {
        let Some(index) = index() else { return };
        let (mesh, baked_uvs) = baked_as_skin(index);
        let data = crate::creature_data::parse(&index.read(NEUTRAL_ANATOMY).unwrap()).unwrap();
        let sizes = part_rect_sizes(&data);
        assert_eq!(sizes.len(), 4, "two eyes, the filter mouth and the flagellum have atlas rectangles");
        assert_eq!(mesh.triangles.len(), 1736);
        let un = skin::unwrap_retail(&mesh, &sizes);
        assert_eq!(un.unwrapped.triangles.len(), 1736);
        let mut charts = un.charts.clone();
        charts.sort_by_key(|c| c.1);
        assert_eq!(charts.iter().map(|c| c.1).collect::<Vec<_>>(), vec![72, 132, 374, 378, 387, 393], "six charts, one per cube-map direction");
        let mut worst = 0.0f32;
        for (t, tri) in un.unwrapped.triangles.iter().enumerate() {
            for k in 0..3 {
                let u = un.unwrapped.uvs[tri[k] as usize];
                worst = worst.max((u[0] - baked_uvs[t][k][0]).abs()).max((u[1] - baked_uvs[t][k][1]).abs());
            }
        }
        assert!(worst < 1e-4, "UV off the baked atlas by {worst}");
        // The part rectangles lie in the same atlas: the baked part triangles sit inside them.
        let parts = baked_triangles(index, false);
        assert_eq!(un.part_rects.len(), 4);
        let inside = parts
            .iter()
            .filter(|(_, _, _, uv)| {
                uv.iter().all(|u| un.part_rects.iter().any(|r| u[0] >= r[0] - 1e-3 && u[0] <= r[2] + 1e-3 && u[1] >= r[1] - 1e-3 && u[1] <= r[3] + 1e-3))
            })
            .count();
        assert!(inside * 10 >= parts.len() * 9, "{inside} of {} baked part triangles inside the part rectangles", parts.len());
    }

    /// The constant the Creator and the history thumbnails build with is the neutral anatomy's.
    #[test]
    fn neutral_part_rectangles_are_the_constant() {
        let Some(index) = index() else { return };
        let data = crate::creature_data::parse(&index.read(NEUTRAL_ANATOMY).unwrap()).unwrap();
        let sizes = part_rect_sizes(&data);
        assert_eq!(sizes.len(), NEUTRAL_PART_RECTS.len());
        for (a, b) in sizes.iter().zip(NEUTRAL_PART_RECTS) {
            assert!((a - b).abs() < 1e-6, "{sizes:?} vs {NEUTRAL_PART_RECTS:?}");
        }
    }

    /// Our vertex number of every baked body triangle corner (the welded vertex at the same position,
    /// within 1e-4), `None` where this crate's mesh has no vertex there.
    fn baked_corners_in_ours(index: &ResourceIndex, mesh: &skin::SkinMesh) -> Vec<[Option<u32>; 3]> {
        let nearest = |p: Vec3| {
            let (i, d) = mesh.positions.iter().enumerate().map(|(i, q)| (i, q.distance(p))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            (d < 1e-4).then_some(i as u32)
        };
        baked_triangles(index, true).iter().map(|(_, _, p, _)| p.map(nearest)).collect()
    }

    /// Retail's marching-cubes triangulation (0x53FE30 with the tables of 0x53D2B0,
    /// `creature_skin::build_retail`) gives the baked starter body's triangles: every baked triangle is
    /// one of ours with the same three vertices in the same winding and corner order (installed data;
    /// the baked body is the editor's output for the neutral anatomy). Before the port only 850 of the
    /// 1736 were. The baked index order is NOT the walk's (its triangles are grouped by chart), so the
    /// order is only printed.
    #[test]
    fn retail_marching_cubes_reproduces_the_baked_triangles() {
        let Some(index) = index() else { return };
        let body = CellBody::load(index);
        let (_, mesh) = skin::build_retail(&records(&body.neutral), body.resolution).expect("neutral skin");
        let baked = baked_corners_in_ours(index, &mesh);
        let missing = baked.iter().flatten().filter(|c| c.is_none()).count();
        let key = |t: [u32; 3]| {
            let mut k = t;
            k.sort_unstable();
            k
        };
        let by_set: std::collections::HashMap<[u32; 3], usize> = mesh.triangles.iter().enumerate().map(|(i, t)| (key(*t), i)).collect();
        let (mut same_set, mut same_cycle, mut same_index) = (0, 0, 0);
        for (b, corners) in baked.iter().enumerate() {
            let Some(ids) = corners.iter().copied().collect::<Option<Vec<u32>>>() else { continue };
            let ids = [ids[0], ids[1], ids[2]];
            let Some(&t) = by_set.get(&key(ids)) else { continue };
            same_set += 1;
            let tri = mesh.triangles[t];
            same_cycle += (0..3).any(|r| (0..3).all(|k| ids[k] == tri[(k + r) % 3])) as usize;
            same_index += (t == b && ids == tri) as usize;
        }
        // The vertices come in the baked order too when the cubes are visited in retail's order.
        let first_use: Vec<u32> = {
            let mut seen = std::collections::HashSet::new();
            baked.iter().flatten().flatten().copied().filter(|v| seen.insert(*v)).collect()
        };
        let numbered = first_use.iter().enumerate().filter(|(i, v)| *i as u32 == **v).count();
        // The vertex positions: the retail bisection and averaging (`root_retail`, 0x4FC120 / 0x53F4E0).
        let baked_tris = baked_triangles(index, true);
        let (mut exact_bits, mut worst_position) = (0usize, 0.0f32);
        for (b, (_, _, p, _)) in baked_tris.iter().enumerate() {
            for k in 0..3 {
                if let Some(v) = baked[b][k] {
                    let q = mesh.positions[v as usize];
                    worst_position = worst_position.max(q.distance(p[k]));
                    exact_bits += (q.to_array().map(f32::to_bits) == p[k].to_array().map(f32::to_bits)) as usize;
                }
            }
        }
        eprintln!("baked corner positions bit-identical to ours: {exact_bits} of {}, worst distance {worst_position:e}", baked_tris.len() * 3);
        eprintln!(
            "baked {} triangles / ours {}: {} baked corners without a vertex of ours, {} with the same vertex triple, {} in the same winding, {} identical at the same index; baked first-use vertex order matches our numbering for {} of {} vertices",
            baked.len(),
            mesh.triangles.len(),
            missing,
            same_set,
            same_cycle,
            same_index,
            numbered,
            first_use.len()
        );
        assert_eq!(missing, 0, "every baked corner is one of our vertices");
        assert_eq!(mesh.triangles.len(), baked.len(), "same number of triangles");
        assert_eq!(same_set, baked.len(), "every baked triangle is one of ours");
        assert_eq!(same_cycle, baked.len(), "same winding");
        let exact = baked.iter().filter(|c| mesh.triangles.contains(&[c[0].unwrap(), c[1].unwrap(), c[2].unwrap()])).count();
        assert_eq!(exact, baked.len(), "the same (v0, v1, v) tuple, start corner included");
        assert!(worst_position < 1e-5, "positions off the baked body by {worst_position}");
    }

    /// The success test of the retail marching-cubes port: the retail unwrap (chart flood + packing) run on
    /// OpenSpore's OWN mesh of the neutral chain reproduces the baked atlas, triangle set and UVs, as it does
    /// on the baked mesh (`unwrap_retail_reproduces_the_baked_atlas`).
    #[test]
    fn unwrap_retail_of_our_own_neutral_mesh_reproduces_the_baked_atlas() {
        let Some(index) = index() else { return };
        let body = CellBody::load(index);
        let (_, mesh) = skin::build_retail(&records(&body.neutral), body.resolution).expect("neutral skin");
        let data = crate::creature_data::parse(&index.read(NEUTRAL_ANATOMY).unwrap()).unwrap();
        let un = skin::unwrap_retail(&mesh, &part_rect_sizes(&data));
        let mut charts = un.charts.clone();
        charts.sort_by_key(|c| c.1);
        assert_eq!(charts.iter().map(|c| c.1).collect::<Vec<_>>(), vec![72, 132, 374, 378, 387, 393], "the baked body's six charts");
        // The baked triangles with their corner UVs, found by our vertex numbers.
        let baked = baked_triangles(index, true);
        let corners = baked_corners_in_ours(index, &mesh);
        let mut by_set: std::collections::HashMap<[u32; 3], usize> = std::collections::HashMap::new();
        for (b, c) in corners.iter().enumerate() {
            let mut k = c.map(|v| v.expect("a baked corner without a vertex of ours"));
            k.sort_unstable();
            by_set.insert(k, b);
        }
        let (mut worst, mut matched) = (0.0f32, 0usize);
        for tri in &un.unwrapped.triangles {
            let ids = tri.map(|v| un.unwrapped.source[v as usize]);
            let mut k = ids;
            k.sort_unstable();
            let Some(&b) = by_set.get(&k) else { continue };
            matched += 1;
            for c in 0..3 {
                // The baked corner at the same vertex of ours.
                let bc = (0..3).find(|&j| corners[b][j] == Some(ids[c])).expect("corner");
                let (ours, theirs) = (un.unwrapped.uvs[tri[c] as usize], baked[b].3[bc]);
                worst = worst.max((ours[0] - theirs[0]).abs()).max((ours[1] - theirs[1]).abs());
            }
        }
        eprintln!("{matched} of {} triangles found in the baked body, worst UV difference {worst}", un.unwrapped.triangles.len());
        assert_eq!(matched, un.unwrapped.triangles.len(), "every triangle of ours is a baked triangle");
        assert!(worst < 1e-4, "UV off the baked atlas by {worst}");
    }

    /// An edited chain gets the retail atlas: UVs inside 0 .. 1, every triangle in a chart of the
    /// six cube-map directions at least, the charts' triangles not sharing atlas cells.
    #[test]
    fn an_edited_body_has_the_retail_atlas() {
        let Some(index) = index() else { return };
        let body = CellBody::load(index);
        let mut chain = body.neutral.clone();
        assert!(wheel(&mut chain, 2, 6.0, body.min_scale, body.max_scale));
        let built = build(&chain, body.resolution).expect("edited skin");
        let un = &built.unwrapped;
        assert!(!un.triangles.is_empty() && un.uvs.len() == un.positions.len() && built.bones.len() == un.positions.len());
        assert!(un.uvs.iter().flatten().all(|v| (0.0..=1.0).contains(v)));
        assert!(un.charts.len() >= 6, "{} charts", un.charts.len());
        let mut cover = vec![u8::MAX; 256 * 256];
        let mut clash = 0;
        for tri in &un.triangles {
            let uv = tri.map(|v| un.uvs[v as usize]);
            let c = [(uv[0][0] + uv[1][0] + uv[2][0]) / 3.0, (uv[0][1] + uv[1][1] + uv[2][1]) / 3.0];
            let chart = un.chart[tri[0] as usize].side;
            let cell = (c[1] * 255.0) as usize * 256 + (c[0] * 255.0) as usize;
            if cover[cell] != u8::MAX && cover[cell] != chart {
                clash += 1;
            }
            cover[cell] = chart;
        }
        assert!(clash * 50 < un.triangles.len(), "{clash} triangle centroids share an atlas cell with another chart");
    }
    /// Exploration (prints): the charts of a fattened body.
    #[test]
    #[ignore]
    fn edited_body_chart_sizes() {
        let Some(index) = index() else { return };
        let body = CellBody::load(index);
        for (v, n) in [(2usize, 8.0f32), (2, 3.0), (4, -4.0), (0, 6.0)] {
            let mut chain = body.neutral.clone();
            assert!(wheel(&mut chain, v, n, body.min_scale, body.max_scale));
            let (_, mesh) = skin::build_retail(&records(&chain), body.resolution).expect("skin");
            let un = skin::unwrap_retail(&mesh, &NEUTRAL_PART_RECTS);
            eprintln!("vertebra {v} x {n}: {} triangles, charts (key, triangles) {:?}", mesh.triangles.len(), un.charts);
        }
    }
}
