//! The retail Creature Creator, part B: the editing manipulators on top of
//! `editor.rs`'s data model (parts on slot bones with offsets, the spine as
//! `BodyProfile` points) and the palette unlock rules.
//!
//! Retail behaviour from `notes/creator-re-notes.md` sections 1.7 and 2:
//! * placement: a part carried from the palette or dragged on the body sticks
//!   to the skin under the cursor; parts with `modelOrientToSurfaces` face
//!   along the skin normal; on the plane of symmetry (within
//!   `modelSymmetrySnapDelta`) a part is single and centred, off it a free
//!   mirrored twin follows (one price, one refund); dragging a mounted part
//!   back onto the palette sells it; Alt + drag clones it (pays again);
//! * mouse wheel (`FUN_005858F0` + `cEditor::sub_581F70`): a hovered part
//!   scales by `2^(0.075 n)`, a vertebra under the cursor by `0.05 n` with
//!   the retail falloff to up to five neighbours each way; one undo entry
//!   per wheel operation (`editorMouseWheelTimeout` 0.6 s idle or 10 px);
//!   Shift or Test Drive leave the wheel to the camera, Paint swallows it;
//! * spine: a press on the torso shows the vertebrae and the two end
//!   handles (`cTuningSpine` colours); dragging a vertebra bends the chain
//!   within the `SpineEditor` hinge limits (-55..45 deg), dragging an end
//!   handle outward adds vertebrae (`spineNewVertebraRelativeScale` 0.9),
//!   inward removes them, sideways curls the end;
//! * unlocks (`cCollectableItems`): see [`unlocks`].
//!
//! The two end handles are the retail `defaultspinehandle` models: the
//! teardrop `CE_basic_ui_teardrop` and its overdraw model (scale 2, colours and
//! alphas of `HandleTuning`: the spine handle's vtable 0x13F6EA0 takes
//! `GetColor` / `GetAlpha` from the base class, i.e. from the tuning file),
//! picked against their triangles.
//!
//! ADAPTED (labelled below): the spine chain is a direct angle-limited chain
//! instead of the Havok ragdoll, vertebra handles are spheres on the back (the
//! retail ones draw through the skin), the end handle's orientation (its own
//! matrix `+0x5C` was not decoded: the tip points outward along the spine) and
//! size argument, the vertebra scale is the spine radius over
//! [`VERTEBRA_UNIT_RADIUS`].

use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
};

use crate::{
    body::{BodyProfile, SpinePoint},
    creature_editor::{scale_part, CreatureEditor, CreatureEditorOwned, Mode},
    creature_editor_data::{CreatureEditorData, CreaturePartInfo, SetupKind, VERTEBRA_COMPLEXITY},
    editor::{Action, EditorState},
    game::{self, PartInstance, Player, RunProgress},
    Originals,
};

// ---------------------------------------------------------------------------
// Retail constants
// ---------------------------------------------------------------------------

/// Last-resort `modelSymmetrySnapDelta` (the limb template's 0.06, in player
/// units) for a part whose model cannot be read (no install, tests); with the
/// install every part has either its authored delta or the retail default of
/// [`default_snap_delta`].
pub const DEFAULT_SNAP_DELTA: f32 = 0.06;
/// ADAPTED: parts with `modelPreferToBeOnPlaneOfSymmetry` (mouths) keep to
/// the centre line over a wider band.
pub const PREFER_PLANE_SNAP: f32 = 0.2;
/// Vertebra wheel step and falloff (`cEditor::sub_581F70`).
pub const VERTEBRA_WHEEL_STEP: f32 = 0.05;
pub const VERTEBRA_NEIGHBOURS: usize = 5;
/// `ce_vertebra` scale limits.
pub const VERTEBRA_MIN_SCALE: f32 = 0.28;
pub const VERTEBRA_MAX_SCALE: f32 = 2.2;
/// ADAPTED: spine radius of a vertebra at scale 1 (the default body's
/// radii 0.25..0.72 are scales 0.5..1.44).
pub const VERTEBRA_UNIT_RADIUS: f32 = 0.5;
/// `spineNewVertebraRelativeScale`.
pub const NEW_VERTEBRA_RELATIVE_SCALE: f32 = 0.9;
/// `SpineEditor` hinge limits (degrees, + = the chain bends up toward the head).
pub const HINGE_MIN_DEG: f32 = -55.0;
pub const HINGE_MAX_DEG: f32 = 45.0;
/// ADAPTED: fewest spine points kept (retail stops at a limb).
pub const MIN_VERTEBRAE: usize = 3;
/// `BodyProfile` limit.
pub const MAX_VERTEBRAE: usize = 32;
/// `cTuningSpine`: end handle colour, size, distance from the end vertebra.
pub const SPINE_HANDLE_COLOR: [f32; 3] = [0.933, 0.321, 0.258];
pub const SPINE_HANDLE_SIZE: f32 = 1.3;
pub const SPINE_HANDLE_DISTANCE: f32 = 0.15;
/// `cTuningSpine` vertebra colour and alphas (rollover, selected, highlight).
pub const VERTEBRA_COLOR: [f32; 3] = [0.75, 0.68, 0.65];
pub const VERTEBRA_ALPHA: [f32; 3] = [0.35, 0.8, 1.0];
/// `editorMouseWheelTimeout` / `editorMouseWheelDistance` defaults.
pub const WHEEL_TIMEOUT: f32 = 0.6;
pub const WHEEL_DISTANCE: f32 = 10.0;
/// ADAPTED: pick distances (screen pixels for handles, world units around a
/// part centre as `editor::drag`).
const HANDLE_PICK_PX: f32 = 14.0;
const PART_PICK: f32 = 0.4;
/// ADAPTED: handle sphere radius (editor units) before the tuning size.
const HANDLE_RADIUS: f32 = 0.06;

// ---------------------------------------------------------------------------
// Picking (pure)
// ---------------------------------------------------------------------------

/// Distance along the ray to triangle `a b c` (Moller-Trumbore, both faces).
pub fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
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

/// Nearest hit of a ray on a triangle soup: (distance, point, normal facing
/// the ray origin).
pub fn ray_hull(tris: &[[Vec3; 3]], origin: Vec3, dir: Vec3) -> Option<(f32, Vec3, Vec3)> {
    let mut best: Option<(f32, usize)> = None;
    for (i, t) in tris.iter().enumerate() {
        if let Some(d) = ray_triangle(origin, dir, t[0], t[1], t[2]) {
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
    }
    let (d, i) = best?;
    let t = tris[i];
    let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
    if n.dot(dir) > 0.0 {
        n = -n;
    }
    Some((d, origin + dir * d, n))
}

/// Yaw that turns a part's front (-Z at yaw 0) toward the skin normal's
/// horizontal direction (VERIFIED: the Pinning manipulator's base orientation
/// `FUN_00449980` builds the part's frame from the normal with its z zeroed and
/// normalised, `FUN_004A8F00(h, Z)`, a yaw-only frame; `FUN_004A8F00` only
/// falls back when that vector has no length, `|h x Z| < 0.005` at 0x4A8F95, so
/// there is no near-vertical dead zone). `None` only for a normal that is
/// vertical (a numeric guard), where the part keeps its yaw (ADAPTED: the
/// part data holds a yaw only, retail's fallback frame is fixed).
pub fn surface_yaw(normal: Vec3) -> Option<f32> {
    if Vec2::new(normal.x, normal.z).length() < 1e-3 {
        return None;
    }
    Some((-normal.x).atan2(-normal.z))
}

/// The retail default `modelSymmetrySnapDelta` of a part model whose x extent
/// (max x - min x of its model box) is `x_extent`: the rigblock loader
/// `FUN_00441910` tests the PROP for 0x8FFC9E66 at 0x4460EF and, when the
/// property is absent, stores `clamp(0.3 x extent, 0.01, 0.13)` at
/// `rigblock +0x220` (0x446146: `mulss 0.3`, `maxss 0.01`, `minss 0.13`).
/// The box is `FUN_0044B2D0(out, 2, 0, 0)`: mode 2 applies neither the block's
/// position nor its scale, so it is the model's own box. VERIFIED code.
pub fn default_snap_delta(x_extent: f32) -> f32 {
    (0.3 * x_extent).clamp(0.01, 0.13)
}

/// x extent of a part's model in its own space (the box of `FUN_0044B2D0` mode
/// 2, see [`default_snap_delta`]): the union of the model's RW4 meshes. Read
/// once per part definition. INFERRED: that the RW4 vertex box is the box the
/// retail model object reports (vtable +0xE4 / `IBakeManager`), before the
/// part's `modelRotation`; `None` without the install.
pub fn model_x_extent(originals: &Originals, info: &CreaturePartInfo) -> Option<f32> {
    use std::{collections::HashMap, sync::Mutex};
    static CACHE: Mutex<Option<HashMap<(u32, u32), Option<f32>>>> = Mutex::new(None);
    let key = (info.key.group_id, info.key.instance_id);
    if let Some(known) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key)) {
        return *known;
    }
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let definition = originals
        .part_catalog
        .parts
        .iter()
        .find(|d| d.key == info.key)
        .or_else(|| originals.part_catalog.by_model(info.model));
    let extent = definition.and_then(|d| {
        let bytes = originals.resource_index.read(d.model).ok()?;
        let meshes = crate::rw4::extract_meshes(&bytes, &mut crate::rw4::MeshStats::default());
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in meshes.iter().flat_map(|m| m.positions.iter()).filter(|p| p[0].is_finite()) {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
        }
        (hi > lo).then_some(hi - lo)
    });
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, extent);
    extent
}

/// Half-width of the centre band in which a part is single and centred, in
/// the frame the offsets use: the part's authored `modelSymmetrySnapDelta`, else
/// the retail default of its model's x extent ([`default_snap_delta`]), times
/// `units` (frame units per editor unit: [`crate::creature_editor::editor_units`]
/// for the creature frame, the importer's `editor_unit` for a rigid block).
/// ADAPTED: `units` (the retail delta is in editor units; the factor between
/// the two frames is INFERRED, editors-audit-a row 32); the mouths' wider band
/// (`modelPreferToBeOnPlaneOfSymmetry`, `FUN_00490F20` not decoded) stays a
/// floor of [`PREFER_PLANE_SNAP`].
pub fn snap_delta(info: Option<&CreaturePartInfo>, x_extent: Option<f32>, units: f32) -> f32 {
    let authored = info.and_then(|i| i.symmetry_snap_delta);
    let editor = authored.or_else(|| x_extent.filter(|e| e.is_finite() && *e > 0.0).map(default_snap_delta));
    let local = editor.map(|d| d * units.max(1e-3));
    match info {
        Some(i) if i.prefer_symmetry_plane => PREFER_PLANE_SNAP.max(local.unwrap_or(0.0)),
        _ => local.unwrap_or(DEFAULT_SNAP_DELTA),
    }
}

/// [`snap_delta`] of a creature-frame part (the Creature Creator and the
/// outfitters).
pub fn part_snap_delta(originals: &Originals, kind: SetupKind, info: Option<&CreaturePartInfo>) -> f32 {
    let extent = info.and_then(|i| model_x_extent(originals, i));
    snap_delta(info, extent, crate::creature_editor::editor_units(originals, kind))
}

// ---------------------------------------------------------------------------
// Placement and symmetry (pure on the run data)
// ---------------------------------------------------------------------------

/// After part `index` moved: face the skin normal (orient-to-surface parts),
/// then apply the symmetry rule (centred single part within `snap` of the
/// plane, mirrored twin off it). `slot(bone)` is the bone's attachment point.
/// Returns the part's index afterwards (a removed twin may come before it).
#[allow(clippy::too_many_arguments)]
pub fn settle(
    run: &mut RunProgress,
    index: usize,
    info: Option<&CreaturePartInfo>,
    normal: Option<Vec3>,
    symmetry: bool,
    snap: f32,
    slot: &dyn Fn(u32) -> Vec3,
) -> usize {
    let Some(part) = run.parts.get(index).cloned() else {
        return index;
    };
    let mut part = part;
    if info.is_some_and(|i| i.orient_to_surface) {
        if let Some(yaw) = normal.and_then(surface_yaw) {
            part.yaw = game::wrap_yaw(yaw);
        }
    }
    let mut pos = slot(part.bone) + Vec3::from_array(part.offset);
    let mut index = index;
    if symmetry && pos.x.abs() <= snap {
        pos.x = 0.0;
        part.offset = game::clamp_offset((pos - slot(part.bone)).to_array(), game::OFFSET_LIMIT);
        part.mirrored = false;
        if part.pair_id != 0 {
            let pair = part.pair_id;
            let before = run.parts[..index].iter().filter(|p| p.pair_id == pair).count();
            let mut i = 0;
            run.parts.retain(|p| {
                let keep = i == index || p.pair_id != pair;
                i += 1;
                keep
            });
            index -= before;
            part.pair_id = 0;
        }
        run.parts[index] = part;
    } else if symmetry {
        if part.pair_id == 0 {
            part.pair_id = run.parts.iter().map(|p| p.pair_id).max().unwrap_or(0).saturating_add(1);
        }
        let twin_bone = run
            .parts
            .iter()
            .enumerate()
            .find(|(i, p)| *i != index && p.pair_id == part.pair_id)
            .map(|(_, p)| p.bone)
            .unwrap_or(part.bone);
        let mirrored_pos = Vec3::new(-pos.x, pos.y, pos.z);
        let twin = PartInstance {
            bone: twin_bone,
            yaw: game::wrap_yaw(-part.yaw),
            offset: game::clamp_offset((mirrored_pos - slot(twin_bone)).to_array(), game::OFFSET_LIMIT),
            mirrored: !part.mirrored,
            edits: part.edits.mirrored(),
            ..part.clone()
        };
        let pair = part.pair_id;
        run.parts[index] = part;
        let mut found = false;
        for (i, p) in run.parts.iter_mut().enumerate() {
            if i != index && p.pair_id == pair {
                *p = twin.clone();
                found = true;
            }
        }
        if !found {
            run.parts.push(twin);
        }
    } else {
        run.parts[index] = part;
    }
    run.focused_part = index as u32;
    run.sync_legacy_accessory();
    index
}

// ---------------------------------------------------------------------------
// Wheel scaling of vertebrae (pure)
// ---------------------------------------------------------------------------

/// Size changes of one wheel operation on vertebra `index`
/// (`cEditor::sub_581F70`): the vertebra grows by `0.05 n`; up to five
/// neighbours each way follow with `w = d < r ? 1 - 0.7 d/r : 0.3 r/d`
/// (d = distance to the scaled vertebra, r = its radius), `w *= 0.4` for a
/// neighbour already past it in the direction of the change (INFERRED
/// reading of "on the other side of the change"), `w <= 0.7 w_prev`, and
/// the walk stops when `w < 0.1`.
pub fn vertebra_scaling(positions: &[Vec3], sizes: &[f32], radius: f32, index: usize, notch: f32) -> Vec<f32> {
    let n = positions.len().min(sizes.len());
    let mut out = vec![0.0; n];
    if index >= n || notch == 0.0 {
        return out;
    }
    out[index] = VERTEBRA_WHEEL_STEP * notch;
    for dir in [1isize, -1] {
        let mut w_prev = 1.0f32;
        for k in 1..=VERTEBRA_NEIGHBOURS as isize {
            let j = index as isize + dir * k;
            if j < 0 || j >= n as isize {
                break;
            }
            let j = j as usize;
            let d = positions[j].distance(positions[index]);
            let r = radius.max(1e-4);
            let mut w = if d < r { 1.0 - 0.7 * d / r } else { 0.3 * r / d.max(1e-4) };
            if (notch > 0.0 && sizes[j] > sizes[index]) || (notch < 0.0 && sizes[j] < sizes[index]) {
                w *= 0.4;
            }
            w = w.min(0.7 * w_prev);
            if w < 0.1 {
                break;
            }
            out[j] = w * VERTEBRA_WHEEL_STEP * notch;
            w_prev = w;
        }
    }
    out
}

/// Apply [`vertebra_scaling`] to the spine (radius = scale x
/// [`VERTEBRA_UNIT_RADIUS`], each within the `ce_vertebra` scale limits).
pub fn scale_vertebra(body: &mut BodyProfile, index: usize, notch: f32) -> bool {
    let positions: Vec<Vec3> = body.spine.iter().map(|p| Vec3::from_array(p.position)).collect();
    let sizes: Vec<f32> = body.spine.iter().map(|p| p.radius / VERTEBRA_UNIT_RADIUS).collect();
    let Some(radius) = body.spine.get(index).map(|p| p.radius) else {
        return false;
    };
    let deltas = vertebra_scaling(&positions, &sizes, radius, index, notch);
    body.edit_spine(|spine| {
        let mut changed = false;
        for (p, d) in spine.iter_mut().zip(deltas) {
            let size = (p.radius / VERTEBRA_UNIT_RADIUS + d).clamp(VERTEBRA_MIN_SCALE, VERTEBRA_MAX_SCALE);
            let r = size * VERTEBRA_UNIT_RADIUS;
            if (r - p.radius).abs() > 1e-6 {
                p.radius = r;
                changed = true;
            }
        }
        changed
    })
}

// ---------------------------------------------------------------------------
// Spine chain (pure). ADAPTED: an angle-limited chain stands in for the
// Havok `SpineEditor` ragdoll (hinge limits from the data).
// ---------------------------------------------------------------------------

/// Sagittal angle of a headward segment (0 = level toward the head (-Z),
/// positive = rising).
pub fn pitch(d: Vec3) -> f32 {
    d.y.atan2(-d.z)
}

fn from_pitch(a: f32, len: f32) -> Vec3 {
    Vec3::new(0.0, a.sin() * len, -a.cos() * len)
}

/// Move point `index` to `target` and let the rest of the chain follow with
/// its segment lengths kept and each joint within the hinge limits.
pub fn bend_chain(points: &mut [Vec3], index: usize, target: Vec3) {
    let n = points.len();
    if index >= n {
        return;
    }
    let lengths: Vec<f32> = points.windows(2).map(|w| w[0].distance(w[1]).max(0.02)).collect();
    let (lo, hi) = (HINGE_MIN_DEG.to_radians(), HINGE_MAX_DEG.to_radians());
    points[index] = Vec3::new(0.0, target.y, target.z);
    for j in index + 1..n {
        let mut a = pitch(points[j] - points[j - 1]);
        if j >= 2 {
            let prev = pitch(points[j - 1] - points[j - 2]);
            a = a.clamp(prev + lo, prev + hi);
        }
        points[j] = points[j - 1] + from_pitch(a, lengths[j - 1]);
    }
    for j in (0..index).rev() {
        let mut a = pitch(points[j + 1] - points[j]);
        if j + 2 < n {
            let next = pitch(points[j + 2] - points[j + 1]);
            a = a.clamp(next - hi, next - lo);
        }
        points[j] = points[j + 1] - from_pitch(a, lengths[j]);
    }
}

/// What an end-handle drag did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndEdit {
    None,
    Added,
    Removed,
    Curled,
}

/// One step of an end-handle drag: `along` (editor units, + outward) beyond
/// one segment adds a vertebra at 0.9 of the end's scale, below -0.6 of a
/// segment removes the end; otherwise the end segment turns toward
/// `target` (the end point's goal) within the hinge limits (curl).
pub fn drag_end(spine: &mut Vec<SpinePoint>, head: bool, along: f32, target: Option<Vec3>, max: usize) -> EndEdit {
    let n = spine.len();
    if n < 2 {
        return EndEdit::None;
    }
    let (e, nb) = if head { (n - 1, n - 2) } else { (0, 1) };
    let pe = Vec3::from_array(spine[e].position);
    let pn = Vec3::from_array(spine[nb].position);
    let seg = pe.distance(pn).max(0.02);
    let out = (pe - pn) / seg;
    if along > seg && n < max {
        let point = SpinePoint {
            position: (pe + out * seg).to_array(),
            radius: (spine[e].radius * NEW_VERTEBRA_RELATIVE_SCALE).max(VERTEBRA_MIN_SCALE * VERTEBRA_UNIT_RADIUS),
        };
        if head {
            spine.push(point);
        } else {
            spine.insert(0, point);
        }
        return EndEdit::Added;
    }
    if along < -0.6 * seg && n > MIN_VERTEBRAE {
        spine.remove(e);
        return EndEdit::Removed;
    }
    if let Some(target) = target {
        // Headward angle of the wanted end segment, limited by the hinge
        // with its neighbour segment.
        let (lo, hi) = (HINGE_MIN_DEG.to_radians(), HINGE_MAX_DEG.to_radians());
        let wanted = target - pn;
        if wanted.length_squared() < 1e-8 {
            return EndEdit::None;
        }
        let mut a = pitch(if head { wanted } else { -wanted });
        if n >= 3 {
            let other = if head {
                pitch(pn - Vec3::from_array(spine[n - 3].position))
            } else {
                pitch(Vec3::from_array(spine[2].position) - pn)
            };
            a = if head { a.clamp(other + lo, other + hi) } else { a.clamp(other - hi, other - lo) };
        }
        let d = from_pitch(a, seg);
        let p = if head { pn + d } else { pn - d };
        if p.distance_squared(pe) < 1e-10 {
            return EndEdit::None;
        }
        spine[e].position = [pe.x, p.y, p.z];
        return EndEdit::Curled;
    }
    EndEdit::None
}

// ---------------------------------------------------------------------------
// Unlocks (`cCollectableItems`)
// ---------------------------------------------------------------------------

/// Palette availability in the Creature stage (`notes/creator-re-notes.md`
/// 1.7): every palette item is locked (`itemUnlockFindPercentage` 0.99 < 1)
/// unless owned; the stage starts with the first column of the
/// `editorBaseConfigUnlocks` rows; a found part unlocks the next column of a
/// row (`CreatureGamePartUnlocking` 0xD3BF50: a random row among the rows
/// that still hold locked items, rows the creature uses first,
/// `pctUnlockItemFromRowBeingUsed` 1.0). Newly unlocked items are marked
/// "new" until the next creator session.
pub mod unlocks {
    use super::*;
    use crate::creature_editor_data::Palette;

    /// The part is on the creature (by definition, or by model for old
    /// saves without one).
    pub fn owned(run: &RunProgress, info: &CreaturePartInfo) -> bool {
        let id = info.key.instance_id;
        run.parts
            .iter()
            .any(|p| p.definition_id == Some(id) || (p.definition_id.is_none() && info.model != 0 && p.instance == info.model))
    }

    pub fn available(run: &RunProgress, info: &CreaturePartInfo) -> bool {
        if crate::outfitter::in_session(run) {
            return crate::outfitter::unlocked(run, info);
        }
        info.starts_unlocked || run.creature_found_parts.contains(&info.key.instance_id) || owned(run, info)
    }

    /// Found since the last creator session (shown highlighted).
    pub fn is_new(run: &RunProgress, part: u32) -> bool {
        run.creature_found_parts.contains(&part) && !run.creature_creation.seen_parts.contains(&part)
    }

    /// The palette item the next found part unlocks: in each row, the lowest
    /// column still locked; rows the creature already uses first; one of
    /// them by `roll`. `None` when every item is available.
    pub fn next_unlock(palette: &Palette, data: &CreatureEditorData, run: &RunProgress, roll: u64) -> Option<u32> {
        let unlocked = |part: u32| data.parts.get(&part).is_some_and(|i| available(run, i));
        let used = |part: u32| data.parts.get(&part).is_some_and(|i| owned(run, i));
        let mut candidates: Vec<(u32, bool)> = Vec::new();
        for page in &palette.pages {
            for row in 0..page.used_rows() {
                let mut items: Vec<_> = page.items.iter().filter(|i| i.row == row && data.parts.contains_key(&i.part)).collect();
                items.sort_by_key(|i| i.column);
                if let Some(next) = items.iter().find(|i| !unlocked(i.part)) {
                    candidates.push((next.part, items.iter().any(|i| used(i.part))));
                }
            }
        }
        let preferred: Vec<u32> = candidates.iter().filter(|c| c.1).map(|c| c.0).collect();
        let pool: Vec<u32> = if preferred.is_empty() { candidates.iter().map(|c| c.0).collect() } else { preferred };
        (!pool.is_empty()).then(|| pool[(roll % pool.len() as u64) as usize])
    }
}

// ---------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpineHandle {
    Vertebra(usize),
    Tail,
    Head,
}

struct SpineDrag {
    handle: SpineHandle,
    before: RunProgress,
    /// Drag plane (player frame) and the grab point on it.
    plane_point: Vec3,
    plane_normal: Vec3,
    /// Spine point position at the grab (vertebra drags).
    start: Vec3,
    press: Vec2,
    moved: bool,
}

struct WheelOp {
    before: RunProgress,
    idle: f32,
    travel: f32,
}

/// A spine handle model (a vertebra sphere or an end handle's teardrop).
#[derive(Component)]
pub struct SpineVisual(pub SpineHandle);

#[derive(Default)]
pub struct EditState {
    spine_selected: bool,
    spine_drag: Option<SpineDrag>,
    hover: Option<SpineHandle>,
    wheel: Option<WheelOp>,
    last_moving: Option<(usize, bool)>,
    last_clock: f32,
    budget_applied: bool,
    hull_body: Option<BodyProfile>,
    hull: Vec<[Vec3; 3]>,
    shown: Option<(Vec<[f32; 4]>, Option<SpineHandle>, Option<SpineHandle>)>,
    mesh: Option<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    /// The end handles' looks (`HandleTuning!defaultspinehandle`).
    look: Option<crate::creature_handles::HandleLook>,
}

impl EditState {
    fn hull(&mut self, originals: &Originals, body: &BodyProfile) -> &[[Vec3; 3]] {
        if self.hull_body.as_ref() != Some(body) {
            self.hull = crate::baked_model::body_triangles(originals, body);
            self.hull_body = Some(body.clone());
        }
        &self.hull
    }
}

fn to_local(pt: &Transform, p: Vec3) -> Vec3 {
    (pt.rotation.conjugate() * (p - pt.translation)) / pt.scale
}

fn to_world(pt: &Transform, p: Vec3) -> Vec3 {
    pt.translation + pt.rotation * (p * pt.scale)
}

fn plane_hit(origin: Vec3, dir: Vec3, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = dir.dot(normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (point - origin).dot(normal) / denom;
    (t > 0.0).then(|| origin + dir * t)
}

/// Handle positions (player frame). Vertebrae: where the line of sight from
/// `eye` to the vertebra centre meets the skin (ADAPTED: retail draws them
/// through the skin); end handles: `SPINE_HANDLE_DISTANCE` past the point
/// where the spine's end direction leaves the skin.
fn handle_points(originals: &Originals, body: &BodyProfile, hull: &[[Vec3; 3]], eye: Vec3) -> Vec<(SpineHandle, Vec3)> {
    let n = body.spine.len();
    let centres: Vec<(Vec3, f32)> = (0..n).map(|i| crate::baked_model::spine_center(originals, body, i)).collect();
    let mut out: Vec<(SpineHandle, Vec3)> = centres
        .iter()
        .enumerate()
        .map(|(i, (c, _))| {
            let d = (*c - eye).normalize_or(Vec3::NEG_Z);
            let p = ray_hull(hull, eye, d).filter(|h| h.0 < c.distance(eye)).map_or(*c, |h| h.1 - d * 0.03);
            (SpineHandle::Vertebra(i), p)
        })
        .collect();
    if n >= 2 {
        for (handle, e, nb) in [(SpineHandle::Tail, 0, 1), (SpineHandle::Head, n - 1, n - 2)] {
            let (ce, re) = centres[e];
            let out_dir = (ce - centres[nb].0).normalize_or(Vec3::Y);
            let skin = ray_hull(hull, ce + out_dir * 5.0, -out_dir).map_or(ce + out_dir * re, |h| h.1);
            out.push((handle, skin + out_dir * SPINE_HANDLE_DISTANCE));
        }
    }
    out
}

/// Placement of an end handle's teardrop in the player frame: at `point`, the
/// model's Y along the spine's outward direction, scaled by the tuning scale of
/// `HandleTuning!defaultspinehandle` (`tuningEditorHandleDefaultScale` 2, applied
/// by `SetScale` 0x47F8E0) times `tuningSpineHandleSize` (ADAPTED: taken as the
/// scale the spine manipulator passes to `SetScale`; the spine handle's own
/// orientation matrix `+0x5C` was not decoded, so the tip is turned outward).
pub fn end_handle_transform(originals: &Originals, body: &BodyProfile, handle: SpineHandle, point: Vec3) -> Transform {
    let n = body.spine.len();
    let out = match (handle, n) {
        (SpineHandle::Tail, n) if n >= 2 => {
            crate::baked_model::spine_center(originals, body, 0).0 - crate::baked_model::spine_center(originals, body, 1).0
        }
        (SpineHandle::Head, n) if n >= 2 => {
            crate::baked_model::spine_center(originals, body, n - 1).0 - crate::baked_model::spine_center(originals, body, n - 2).0
        }
        _ => Vec3::Y,
    };
    let scale = crate::creature_handles::SPINE_END_SCALE * SPINE_HANDLE_SIZE;
    Transform::from_translation(point).with_rotation(Quat::from_rotation_arc(Vec3::Y, out.normalize_or(Vec3::Y))).with_scale(Vec3::splat(scale))
}

fn nearest_vertebra(originals: &Originals, body: &BodyProfile, p: Vec3) -> usize {
    (0..body.spine.len())
        .min_by(|a, b| {
            let da = crate::baked_model::spine_center(originals, body, *a).0.distance_squared(p);
            let db = crate::baked_model::spine_center(originals, body, *b).0.distance_squared(p);
            da.total_cmp(&db)
        })
        .unwrap_or(0)
}

/// Retail sounds of the pinning manipulator's symmetry snap.
pub const SYM_SNAP_SOUND: &str = "editor_sym_snap";
pub const SYM_UNSNAP_SOUND: &str = "editor_sym_unsnap";

/// Which of the two symmetry sounds a drag step plays. The pinning manipulator
/// (`PinningManipulator` mouse move 0x5B2E90, vtable 0x13F7514) keeps a flag at
/// `+0xE8` (cleared by the constructor): when the snap test of the dragged block
/// (`FUN_00490F20`, -1.0 = no snap) succeeds and the flag is clear it plays the sound whose id
/// is the global `0x15E9A0C` (`editor_sym_snap`, 0x5B3A7B) and sets the flag; when the test fails
/// and the flag is set it plays `0x15E99A4` (`editor_sym_unsnap`, 0x5B3A98) and clears it
/// (VERIFIED: the sound names come from the static initializers 0x01284170 / 0x01284190 and the
/// two play sites). INFERRED: the snap test is the symmetry-plane snap (the names say "sym";
/// `FUN_00490F20` itself was not decoded). ADAPTED: "snapped" is the part being single on the
/// centre band ([`settle`] removed its twin); `before` is the flag.
pub fn symmetry_sound(before: bool, snapped: bool) -> Option<&'static str> {
    match (before, snapped) {
        (false, true) => Some(SYM_SNAP_SOUND),
        (true, false) => Some(SYM_UNSNAP_SOUND),
        _ => None,
    }
}

/// Plays [`symmetry_sound`] while a part is carried or dragged in a creator with symmetry.
pub fn symmetry_sounds(
    time: Res<Time<Real>>,
    state: Res<EditorState>,
    editor: Res<CreatureEditor>,
    data: Res<CreatureEditorData>,
    run: Res<RunProgress>,
    sounds: Option<ResMut<crate::cell_audio::NamedSounds>>,
    mut flag: Local<Option<bool>>,
    mut prefetched: Local<bool>,
) {
    let Some(mut sounds) = sounds else { return };
    if !sounds.is_running() {
        return;
    }
    if !*prefetched {
        *prefetched = true;
        for name in [SYM_SNAP_SOUND, SYM_UNSNAP_SOUND] {
            sounds.request(crate::world_objects::name_id(name), Some(name), false);
        }
    }
    let symmetry = data.setup(editor.kind).is_none_or(|s| s.use_symmetry);
    let moving = state.moving_part().filter(|_| symmetry && editor.mode == Mode::Build);
    let Some((index, _)) = moving else {
        *flag = None;
        return;
    };
    let Some(part) = run.parts.get(index) else { return };
    let snapped = part.pair_id == 0;
    let before = flag.replace(snapped).unwrap_or(false);
    if let Some(name) = symmetry_sound(before, snapped) {
        let mut play = crate::cell_audio::Play::named(name, "creature_editor_edit");
        play.bus = crate::game_audio::Bus::Ui;
        sounds.play(play, time.elapsed_secs_f64(), 0.0);
    }
}

/// The creator-b manipulators (runs after `editor::drag`, before the editor
/// camera).
#[allow(clippy::too_many_arguments)]
pub fn manipulate(
    mut commands: Commands,
    time: Res<Time>,
    (mouse, keys): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    (mut wheel, mut motion): (MessageReader<MouseWheel>, MessageReader<MouseMotion>),
    pointer: Res<crate::spui::view::SpuiPointer>,
    mut edit: Local<EditState>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    players: Query<&Transform, With<Player>>,
    mut visuals: Query<(Entity, &SpineVisual, &mut Transform), Without<Player>>,
    (mut meshes, mut materials, mut overdraw_materials, handle_cache): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        Option<ResMut<Assets<crate::creature_handles::OverdrawMaterial>>>,
        Option<ResMut<crate::creature_handles::HandleCache>>,
    ),
    (data, originals, limbs): (Res<CreatureEditorData>, Res<Originals>, Option<Res<crate::creature_limbs::LimbState>>),
    mut editor: ResMut<CreatureEditor>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
) {
    // The rigid-block editors have their own manipulators (`rigid_editor_edit`).
    if editor.kind.is_rigid() {
        return;
    }
    let models = handle_cache.and_then(|mut c| c.models(&originals, &mut meshes));
    let dt = time.delta_secs();
    // A new session: the found parts not seen before are "new" for this
    // session; the early creator applies its budget.
    if editor.clock < edit.last_clock || edit.last_clock == 0.0 {
        let new_parts: Vec<u32> = run
            .creature_found_parts
            .iter()
            .copied()
            .filter(|p| unlocks::is_new(&run, *p))
            .collect();
        editor.new_parts = new_parts;
        let found = run.creature_found_parts.clone();
        run.creature_creation.seen_parts = found;
        edit.budget_applied = false;
        edit.spine_selected = false;
        edit.spine_drag = None;
        edit.wheel = None;
        edit.shown = None;
        // Capture option `--creator-new-demo`: every available part counts as newly found (the
        // palette's burst behind the icon).
        if std::env::args().any(|a| a == "--creator-new-demo") {
            editor.new_parts = data.parts.iter().filter(|(_, i)| unlocks::available(&run, i)).map(|(k, _)| *k).collect();
        }
        // Capture option `--creator-demo`: a scripted edit (an eye moved
        // onto the side of the head, a fattened middle vertebra, one more
        // vertebra at the tail, the spine selected).
        if std::env::args().any(|a| a == "--creator-demo") {
            let body = run.body.clone();
            let n = body.spine.len();
            let slot = |bone: u32| crate::baked_model::attachment_point(&originals, &body, bone);
            let eye = run.parts.iter().position(|p| data.info_of(p).is_some_and(|i| i.orient_to_surface && !i.mouth));
            // From the right of the eye's own slot toward the head.
            let hit = eye.and_then(|i| {
                let origin = slot(run.parts[i].bone) + Vec3::from_array(run.parts[i].offset) + Vec3::new(3.0, -0.05, 0.05);
                ray_hull(edit.hull(&originals, &body), origin, Vec3::NEG_X)
            });
            info!("creator-demo eye {eye:?} hit {hit:?}");
            if let (Some((_, point, normal)), Some(i)) = (hit, eye) {
                let p = &mut run.parts[i];
                p.offset = game::clamp_offset((point - slot(p.bone)).to_array(), game::OFFSET_LIMIT);
                let info = data.info_of(&run.parts[i]).cloned();
                let snap = part_snap_delta(&originals, editor.kind, info.as_ref());
                settle(&mut run, i, info.as_ref(), Some(normal), true, snap, &slot);
            }
            scale_vertebra(&mut run.body, n / 2, 6.0);
            run.body.edit_spine(|spine| drag_end(spine, false, 10.0, None, MAX_VERTEBRAE) == EndEdit::Added);
            edit.spine_selected = true;
            state.visuals_dirty = true;
        }
    }
    edit.last_clock = editor.clock.max(1e-6);
    if !edit.budget_applied {
        edit.budget_applied = true;
        if let Some(dna) = session_budget(&data, editor.kind, &run) {
            run.editor_dna = dna;
            if let Some(entry) = editor.entry.as_mut() {
                entry.dna = dna;
            }
        }
    }
    let travel: f32 = motion.read().map(|m| m.delta.length()).sum();
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 40.0,
        })
        .sum();
    let build = editor.mode == Mode::Build && !editor.options_open && !editor.naming;
    if editor.mode == Mode::Paint && notches != 0.0 && !pointer.over_ui {
        // Paint mode: the editor takes the wheel and does nothing with it.
        editor.wheel_consumed = true;
    }
    // A wheel operation ends after the idle timeout or mouse travel.
    let setup = data.setup(editor.kind);
    let timeout = setup.and_then(|s| s.mouse_wheel_timeout).unwrap_or(WHEEL_TIMEOUT);
    let distance = setup.and_then(|s| s.mouse_wheel_distance).unwrap_or(WHEEL_DISTANCE);
    if let Some(op) = edit.wheel.as_mut() {
        op.idle += dt;
        op.travel += travel;
        if op.idle > timeout || op.travel > distance || !build {
            let op = edit.wheel.take().unwrap();
            if op.before.parts != run.parts || op.before.body != run.body {
                state.record_external(op.before);
                run.request_save();
            }
        }
    }
    let use_spine = setup.is_none_or(|s| s.edits_spine());
    let symmetry = setup.is_none_or(|s| s.use_symmetry);
    if !build {
        if let Some(drag) = edit.spine_drag.take() {
            if drag.before.body != run.body {
                state.record_external(drag.before);
            }
        }
        editor.manipulating = false;
        edit.spine_selected = false;
        edit.last_moving = None;
        sync_visuals(&mut commands, &mut edit, &mut visuals, None, None, None, &[], &run.body, &originals, models.as_deref(), &mut meshes, &mut materials, overdraw_materials.as_deref_mut());
        return;
    }
    let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else {
        return;
    };
    let pt = *pt;
    let cursor = window.cursor_position();
    let over_ui = pointer.over_ui || cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_ui(c)));
    let over_palette = cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_palette(c)));
    let ray = cursor.and_then(|c| camera.viewport_to_world(cam, c).ok());
    let local_ray = ray.map(|r| {
        let o = to_local(&pt, r.origin);
        let d = ((pt.rotation.conjugate() * *r.direction) / pt.scale).normalize_or(Vec3::NEG_Z);
        (o, d)
    });
    let body = run.body.clone();
    let hit = local_ray.filter(|_| !over_ui).and_then(|(o, d)| ray_hull(edit.hull(&originals, &body), o, d));
    let slot = |bone: u32| crate::baked_model::attachment_point(&originals, &body, bone);

    // Placement of the carried or dragged part.
    let moving = state.moving_part();
    if let Some((index, _)) = moving {
        let info = run.parts.get(index).and_then(|p| data.info_of(p));
        let mut normal = None;
        if let (Some((_, point, n)), Some(p)) = (hit, run.parts.get_mut(index)) {
            p.offset = game::clamp_offset((point - slot(p.bone)).to_array(), game::OFFSET_LIMIT);
            normal = Some(n);
        }
        let before = run.parts.clone();
        let snap = part_snap_delta(&originals, editor.kind, info);
        let next = settle(&mut run, index, info, normal, symmetry, snap, &slot);
        if next != index {
            state.set_moving_part(next);
        }
        if run.parts != before || normal.is_some() {
            state.visuals_dirty = true;
        }
        // Alt + press on a mounted part: clone it (pays again; the held
        // press carries the copy).
        if mouse.just_pressed(MouseButton::Left)
            && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
            && moving.is_some_and(|m| !m.1)
        {
            if let Some(def) = run.parts.get(next).and_then(|p| data.definition_of(p)) {
                match crate::creature_editor::refusal(&data, editor.kind, &run, &originals, def) {
                    None => state.queued.push(Action::Buy(def)),
                    Some(_) => editor.say("Not enough DNA or complexity to clone this part"),
                }
            }
        }
    }
    // A mounted part dropped onto the palette is sold.
    if let Some((index, false)) = edit.last_moving {
        if moving.is_none() && mouse.just_released(MouseButton::Left) && over_palette && index < run.parts.len() {
            run.focused_part = index as u32;
            state.queued.push(Action::Remove);
        }
    }
    edit.last_moving = state.moving_part();

    // Spine: hover, press, drag.
    let eye = to_local(&pt, cam.translation());
    let handles = if use_spine && edit.spine_selected {
        let hull = edit.hull(&originals, &body);
        handle_points(&originals, &body, hull, eye)
    } else {
        vec![]
    };
    edit.hover = None;
    if let Some(c) = cursor.filter(|_| !over_ui) {
        // The end handles are the teardrop models: picked against their triangles
        // (retail: model groups at collision mode 4). The vertebra handles stay
        // screen-space spheres (ADAPTED, see `handle_points`).
        let ends = models.as_ref().zip(local_ray).and_then(|(m, (o, d))| {
            handles
                .iter()
                .filter(|(h, _)| !matches!(h, SpineHandle::Vertebra(_)))
                .filter_map(|(h, p)| {
                    let tf = end_handle_transform(&originals, &body, *h, *p);
                    m.teardrop.pick(&tf, o, d).map(|t| (*h, t))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h)
        });
        edit.hover = ends.or_else(|| {
            handles
                .iter()
                .filter(|(h, _)| matches!(h, SpineHandle::Vertebra(_)))
                .filter_map(|(h, p)| {
                    let s = camera.world_to_viewport(cam, to_world(&pt, *p)).ok()?;
                    let d = s.distance(c);
                    (d < HANDLE_PICK_PX).then_some((*h, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h)
        });
    }
    if use_spine && mouse.just_pressed(MouseButton::Left) && !over_ui && !state.dragging() {
        let grab = edit
            .hover
            .and_then(|h| handles.iter().find(|x| x.0 == h).map(|x| (h, x.1)))
            .or_else(|| hit.map(|(_, p, _)| (SpineHandle::Vertebra(nearest_vertebra(&originals, &body, p)), p)));
        match (grab, local_ray) {
            (Some((handle, point)), Some((_, d))) => {
                edit.spine_selected = true;
                let n = body.spine.len();
                let index = match handle {
                    SpineHandle::Vertebra(i) => i,
                    SpineHandle::Tail => 0,
                    SpineHandle::Head => n - 1,
                };
                let start = Vec3::from_array(body.spine[index].position);
                edit.spine_drag = Some(SpineDrag {
                    handle,
                    before: run.clone(),
                    plane_point: point,
                    plane_normal: -d,
                    start,
                    press: cursor.unwrap_or_default(),
                    moved: false,
                });
            }
            _ => edit.spine_selected = false,
        }
    }
    editor.manipulating = edit.spine_drag.is_some();
    if let Some(drag) = edit.spine_drag.as_mut() {
        if !mouse.pressed(MouseButton::Left) {
            let drag = edit.spine_drag.take().unwrap();
            if drag.before.body != run.body {
                state.record_external(drag.before);
                run.request_save();
            }
            editor.manipulating = false;
        } else if let (Some(c), Some((o, d))) = (cursor, local_ray) {
            drag.moved |= c.distance(drag.press) > 3.0;
            if let Some(target) = plane_hit(o, d, drag.plane_point, drag.plane_normal).filter(|_| drag.moved) {
                let delta = target - drag.plane_point;
                let complexity = data.complexity(editor.kind, &run.parts, run.body.spine.len());
                let room = complexity.blocks + VERTEBRA_COMPLEXITY <= complexity.block_limit;
                let changed = match drag.handle {
                    SpineHandle::Vertebra(i) => {
                        let target = drag.start + delta;
                        run.body.edit_spine(|spine| {
                            let mut points: Vec<Vec3> = spine.iter().map(|p| Vec3::from_array(p.position)).collect();
                            bend_chain(&mut points, i, target);
                            for (p, q) in spine.iter_mut().zip(points) {
                                p.position = q.to_array();
                            }
                            true
                        })
                    }
                    SpineHandle::Head | SpineHandle::Tail => {
                        let head = drag.handle == SpineHandle::Head;
                        let n = run.body.spine.len();
                        let (e, nb) = if head { (n - 1, n - 2) } else { (0, 1) };
                        let (ce, _) = crate::baked_model::spine_center(&originals, &run.body, e);
                        let (cn, _) = crate::baked_model::spine_center(&originals, &run.body, nb);
                        let out = (ce - cn).normalize_or(Vec3::Z);
                        let along = delta.dot(out);
                        let goal = drag.start + delta;
                        let max = if room { MAX_VERTEBRAE } else { n };
                        let mut result = EndEdit::None;
                        let ok = run.body.edit_spine(|spine| {
                            result = drag_end(spine, head, along, Some(goal), max);
                            result != EndEdit::None
                        });
                        if ok && matches!(result, EndEdit::Added | EndEdit::Removed) {
                            // The grab follows the added or removed segment.
                            let seg = ce.distance(cn);
                            let sign = if result == EndEdit::Added { 1.0 } else { -1.0 };
                            drag.plane_point += out * seg * sign;
                            let m = run.body.spine.len();
                            drag.start = Vec3::from_array(run.body.spine[if head { m - 1 } else { 0 }].position);
                        }
                        ok
                    }
                };
                if changed {
                    state.visuals_dirty = true;
                }
            }
        }
    }

    // Mouse wheel over a part or the torso.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // Over a limb joint or section the wheel is the limb's (`creature_limbs`).
    let over_limb = limbs.as_ref().is_some_and(|l| l.over_limb());
    if notches != 0.0 && !shift && !over_ui && !over_limb {
        let locked = state.locked_parts;
        let hovered = ray.and_then(|r| {
            run.parts
                .iter()
                .enumerate()
                .filter(|(i, _)| *i >= locked)
                .filter_map(|(i, p)| {
                    let center = to_world(&pt, slot(p.bone) + Vec3::from_array(p.offset));
                    let along = (center - r.origin).dot(*r.direction);
                    let d = (center - (r.origin + *r.direction * along)).length();
                    (along > 0.0 && d < PART_PICK).then_some((i, along))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
        let before = run.clone();
        let applied = if let Some(i) = hovered {
            scale_part(&data, &mut run, i, notches)
        } else if let (Some((_, p, _)), true) = (hit, use_spine) {
            let i = nearest_vertebra(&originals, &body, p);
            scale_vertebra(&mut run.body, i, notches)
        } else {
            false
        };
        if hovered.is_some() || (hit.is_some() && use_spine) {
            editor.wheel_consumed = true;
        }
        if applied {
            state.visuals_dirty = true;
            match edit.wheel.as_mut() {
                Some(op) => op.idle = 0.0,
                None => edit.wheel = Some(WheelOp { before, idle: 0.0, travel: 0.0 }),
            }
        }
    }
    let shown = edit.spine_selected.then_some(edit.hover);
    let handles = if shown.is_some() && (run.body != body || handles.is_empty()) {
        let hull = edit.hull(&originals, &run.body);
        handle_points(&originals, &run.body, hull, eye)
    } else {
        handles
    };
    let active = edit.spine_drag.as_ref().map(|d| d.handle);
    sync_visuals(&mut commands, &mut edit, &mut visuals, shown, active, Some(&pt), &handles, &run.body, &originals, models.as_deref(), &mut meshes, &mut materials, overdraw_materials.as_deref_mut());
}

/// Budget of a session that does not run on the stage's DNA: the early
/// creator (`CellToCreatureEditor`) starts from `editorInitialBudget` (145)
/// minus the cost of the loaded creature (`editorSubtractCostOnLoad`).
/// INFERRED from the setup data.
pub fn session_budget(data: &CreatureEditorData, kind: SetupKind, run: &RunProgress) -> Option<f32> {
    if kind != SetupKind::CellToCreature {
        return None;
    }
    let s = data.setup(kind)?;
    let initial = s.initial_budget? as f32;
    let mut pairs = Vec::new();
    let mut cost = 0.0;
    for p in run.parts.iter().filter(|_| s.subtract_cost_on_load) {
        // A symmetric pair is one purchase.
        if p.pair_id != 0 {
            if pairs.contains(&p.pair_id) {
                continue;
            }
            pairs.push(p.pair_id);
        }
        cost += data.info_of(p).map_or(0.0, |i| i.price as f32);
    }
    Some((initial - cost).max(0.0))
}

/// Spawn the spine handles (vertebra spheres, end handle teardrops with their
/// overdraw models) when the spine, selection, hover or drag changes; keep them
/// where `handles` says every frame.
#[allow(clippy::too_many_arguments)]
fn sync_visuals(
    commands: &mut Commands,
    edit: &mut EditState,
    visuals: &mut Query<(Entity, &SpineVisual, &mut Transform), Without<Player>>,
    shown: Option<Option<SpineHandle>>,
    active: Option<SpineHandle>,
    pt: Option<&Transform>,
    handles: &[(SpineHandle, Vec3)],
    body: &BodyProfile,
    originals: &Originals,
    models: Option<&crate::creature_handles::HandleModels>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    overdraw_materials: Option<&mut Assets<crate::creature_handles::OverdrawMaterial>>,
) {
    let key = shown.map(|hover| (body.spine.iter().map(|p| [p.position[0], p.position[1], p.position[2], p.radius]).collect::<Vec<_>>(), hover, active));
    if key == edit.shown && (key.is_some() || visuals.is_empty()) {
        if let Some(pt) = pt {
            for (_, v, mut t) in visuals.iter_mut() {
                if let Some((_, p)) = handles.iter().find(|h| h.0 == v.0) {
                    t.translation = to_world(pt, *p);
                }
            }
        }
        return;
    }
    for (e, _, _) in visuals.iter() {
        commands.entity(e).despawn();
    }
    edit.shown = key.clone();
    let (Some(hover), Some(pt)) = (shown, pt) else {
        return;
    };
    let mesh = edit.mesh.get_or_insert_with(|| meshes.add(Sphere::new(HANDLE_RADIUS))).clone();
    if edit.materials.is_empty() {
        let unlit = |c: [f32; 3], a: f32| StandardMaterial {
            base_color: Color::srgba(c[0], c[1], c[2], a),
            unlit: true,
            alpha_mode: if a < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
            ..default()
        };
        edit.materials = vec![materials.add(unlit(VERTEBRA_COLOR, VERTEBRA_ALPHA[1])), materials.add(unlit(VERTEBRA_COLOR, VERTEBRA_ALPHA[2]))];
    }
    // `HandleTuning!defaultspinehandle`: the base class reads its colours, alphas
    // and models from the tuning file (vtable 0x13F6EA0: GetColor 0x47EE80).
    if edit.look.is_none() {
        if let Some(over) = overdraw_materials {
            edit.look = Some(crate::creature_handles::handle_look(
                crate::creature_handles::default_handle_colors(),
                crate::creature_handles::SPINE_ALPHA,
                crate::creature_handles::SPINE_OVERDRAW_ALPHA,
                materials,
                over,
            ));
        }
    }
    for &(handle, p) in handles {
        let end = !matches!(handle, SpineHandle::Vertebra(_));
        if let (true, Some(models), Some(look)) = (end, models, edit.look.as_ref()) {
            let state = if active == Some(handle) {
                2
            } else if Some(handle) == hover {
                1
            } else {
                0
            };
            let tf = end_handle_transform(originals, body, handle, p);
            let tf = Transform { translation: to_world(pt, p), rotation: pt.rotation * tf.rotation, scale: tf.scale * pt.scale };
            commands.spawn((
                CreatureEditorOwned,
                SpineVisual(handle),
                crate::stages::StageOwned,
                Mesh3d(models.teardrop.mesh.clone()),
                MeshMaterial3d(look.main[state].clone()),
                tf,
                bevy::light::NotShadowCaster,
            ));
            commands.spawn((
                CreatureEditorOwned,
                SpineVisual(handle),
                crate::stages::StageOwned,
                Mesh3d(models.teardrop_overdraw.mesh.clone()),
                MeshMaterial3d(look.overdraw[state].clone()),
                tf,
                bevy::light::NotShadowCaster,
            ));
            continue;
        }
        // ADAPTED: vertebra handles are spheres on the back (retail draws the
        // `ce_vertebra` models through the skin).
        let material = if Some(handle) == hover { 1 } else { 0 };
        commands.spawn((
            CreatureEditorOwned,
            SpineVisual(handle),
            crate::stages::StageOwned,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(edit.materials[material].clone()),
            Transform::from_translation(to_world(pt, p)).with_rotation(pt.rotation).with_scale(pt.scale),
            bevy::light::NotShadowCaster,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetry_sounds_follow_the_snap_flag() {
        // The pinning manipulator's flag starts clear: the first snapped step plays the snap sound
        // once, leaving the plane plays the unsnap sound once, nothing else plays.
        assert_eq!(symmetry_sound(false, true), Some("editor_sym_snap"));
        assert_eq!(symmetry_sound(true, true), None);
        assert_eq!(symmetry_sound(true, false), Some("editor_sym_unsnap"));
        assert_eq!(symmetry_sound(false, false), None);
    }

    #[test]
    fn undo_history_keeps_more_than_the_old_64_steps() {
        // Retail's `CommitEditHistory` (0x586410) has no depth check.
        let mut state = EditorState::default();
        for _ in 0..100 {
            state.record_external(RunProgress::default());
        }
        let mut run = RunProgress::default();
        let mut steps = 0;
        while state.undo_step(&mut run) {
            steps += 1;
        }
        assert_eq!(steps, 100);
        assert!(crate::editor::UNDO_DEPTH >= 100);
    }

    use crate::dbpf::ResourceKey;

    fn info(orient: bool, prefer: bool) -> CreaturePartInfo {
        CreaturePartInfo {
            key: ResourceKey { type_id: 0, group_id: 0, instance_id: 7 },
            name: "eye".into(),
            price: 25,
            complexity: 5,
            bones: 1,
            bake_level: 0,
            geom: 0.0,
            unlock_level: 1,
            find_percentage: 0.99,
            min_scale: 0.5,
            max_scale: 2.0,
            sell_back: 1.0,
            icon: ResourceKey { type_id: 0, group_id: 0, instance_id: 0 },
            model: 70,
            abilities: vec![],
            carnivore: 0,
            herbivore: 0,
            mouth: false,
            orient_to_surface: orient,
            prefer_symmetry_plane: prefer,
            symmetry_snap_delta: None,
            acts_like_foot: false,
            acts_like_grasper: false,
            starts_unlocked: false,
        }
    }

    #[test]
    fn ray_hits_the_nearest_face_with_a_normal_toward_the_viewer() {
        let a = Vec3::new(-1.0, -1.0, 0.0);
        let b = Vec3::new(1.0, -1.0, 0.0);
        let c = Vec3::new(0.0, 1.0, 0.0);
        assert!((ray_triangle(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, a, b, c).unwrap() - 5.0).abs() < 1e-5);
        assert!(ray_triangle(Vec3::new(3.0, 0.0, 5.0), Vec3::NEG_Z, a, b, c).is_none());
        let far = [a - Vec3::Z, b - Vec3::Z, c - Vec3::Z];
        let (d, p, n) = ray_hull(&[far, [a, b, c]], Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
        assert!((d - 5.0).abs() < 1e-5 && p.z.abs() < 1e-5);
        assert!(n.z > 0.99, "normal faces the ray origin: {n}");
    }

    #[test]
    fn parts_face_the_skin_normal_sideways_but_keep_their_yaw_on_top() {
        assert!(surface_yaw(Vec3::NEG_Z).unwrap().abs() < 1e-6);
        let left = surface_yaw(Vec3::NEG_X).unwrap();
        let front = Quat::from_rotation_y(left) * Vec3::NEG_Z;
        assert!(front.distance(Vec3::NEG_X) < 1e-5, "{front}");
        assert!(surface_yaw(Vec3::Y).is_none());
        // No dead zone near the top (retail normalises the horizontal part of any normal).
        assert!(surface_yaw(Vec3::new(0.1, 1.0, 0.0).normalize()).is_some());
        assert_eq!(snap_delta(Some(&info(true, false)), None, 1.0), DEFAULT_SNAP_DELTA);
        assert_eq!(snap_delta(Some(&info(false, true)), None, 1.0), PREFER_PLANE_SNAP);
    }

    #[test]
    fn symmetry_centres_on_the_plane_and_mirrors_off_it_for_one_price() {
        let slot = |_b: u32| Vec3::ZERO;
        let mut run = RunProgress::default();
        run.parts = vec![PartInstance::new(70, 0).with_definition(7)];
        let eye = info(true, false);
        // Off the centre line: a mirrored twin appears.
        run.parts[0].offset = [0.4, 0.5, -0.8];
        let i = settle(&mut run, 0, Some(&eye), Some(Vec3::NEG_X), true, 0.06, &slot);
        assert_eq!((i, run.parts.len()), (0, 2));
        let (a, b) = (&run.parts[0], &run.parts[1]);
        assert!(a.pair_id != 0 && a.pair_id == b.pair_id);
        assert_eq!(b.offset, [-0.4, 0.5, -0.8]);
        assert!(b.mirrored && !a.mirrored);
        // Mirrored yaw (angles wrap to 0..2pi).
        let sum = b.yaw + a.yaw;
        assert!(sum.sin().abs() < 1e-5 && sum.cos() > 0.999, "{} {}", a.yaw, b.yaw);
        // Dragging the twin (second) onto the plane removes the first one.
        run.parts[1].offset = [0.03, 0.5, -0.8];
        let i = settle(&mut run, 1, Some(&eye), None, true, 0.06, &slot);
        assert_eq!((i, run.parts.len()), (0, 1));
        assert_eq!(run.parts[0].offset[0], 0.0);
        assert_eq!(run.parts[0].pair_id, 0);
        assert!(!run.parts[0].mirrored);
        assert_eq!(run.focused_part, 0);
        // Without symmetry nothing is mirrored.
        run.parts[0].offset = [0.4, 0.5, -0.8];
        settle(&mut run, 0, Some(&eye), None, false, 0.06, &slot);
        assert_eq!(run.parts.len(), 1);
    }

    #[test]
    fn the_default_snap_delta_is_a_clamped_third_of_the_model_width() {
        // FUN_00441910 0x446146: clamp(0.3 x extent, 0.01, 0.13).
        assert!((default_snap_delta(0.2) - 0.06).abs() < 1e-6);
        assert_eq!(default_snap_delta(0.0), 0.01);
        assert_eq!(default_snap_delta(5.0), 0.13);
        // An authored delta wins; the default scales with the frame units.
        let mut authored = info(true, false);
        authored.symmetry_snap_delta = Some(0.06);
        assert!((snap_delta(Some(&authored), Some(5.0), 2.0) - 0.12).abs() < 1e-6);
        let plain = info(true, false);
        assert!((snap_delta(Some(&plain), Some(0.2), 2.0) - 0.12).abs() < 1e-6);
        assert!((snap_delta(Some(&plain), Some(5.0), 2.0) - 0.26).abs() < 1e-6);
        // A mouth keeps its wider floor.
        assert_eq!(snap_delta(Some(&info(false, true)), Some(0.2), 1.0), PREFER_PLANE_SNAP);
    }

    #[test]
    fn vertebra_wheel_falls_off_toward_the_neighbours() {
        let positions: Vec<Vec3> = (0..9).map(|i| Vec3::new(0.0, 0.0, i as f32 * 0.1)).collect();
        let sizes = vec![1.0; 9];
        let d = vertebra_scaling(&positions, &sizes, 0.3, 4, 1.0);
        assert!((d[4] - 0.05).abs() < 1e-6);
        // d = 0.1 < r = 0.3: w = 1 - 0.7/3 = 0.767 -> capped at 0.7.
        assert!((d[5] - 0.7 * 0.05).abs() < 1e-5, "{d:?}");
        assert!((d[3] - d[5]).abs() < 1e-6, "symmetric falloff");
        assert!(d[5] > d[6] && d[6] > d[7] && d[7] >= d[8]);
        // A neighbour already larger than the grown vertebra moves less.
        let mut bigger = sizes.clone();
        bigger[5] = 1.5;
        let e = vertebra_scaling(&positions, &bigger, 0.3, 4, 1.0);
        assert!(e[5] < d[5]);
        // Far neighbours (d > r) fall off with 0.3 r / d and stop below 0.1.
        let sparse: Vec<Vec3> = (0..5).map(|i| Vec3::new(0.0, 0.0, i as f32 * 2.0)).collect();
        let f = vertebra_scaling(&sparse, &[1.0; 5], 0.3, 0, -1.0);
        assert_eq!(f[1], 0.0, "{f:?}");
        assert!((f[0] + 0.05).abs() < 1e-6);
    }

    #[test]
    fn spine_bends_within_the_hinge_and_grows_or_shrinks_at_the_ends() {
        let mut points: Vec<Vec3> = (0..5).map(|i| Vec3::new(0.0, 0.0, 1.0 - i as f32 * 0.5)).collect();
        bend_chain(&mut points, 4, Vec3::new(0.0, 3.0, -1.0));
        for w in points.windows(2) {
            assert!((w[0].distance(w[1]) - 0.5).abs() < 1e-4, "lengths kept");
        }
        for w in points.windows(3) {
            let bend = (pitch(w[2] - w[1]) - pitch(w[1] - w[0])).to_degrees();
            assert!((HINGE_MIN_DEG - 0.01..=HINGE_MAX_DEG + 0.01).contains(&bend), "{bend}");
        }
        let mut spine = BodyProfile::default().spine;
        let n = spine.len();
        let end_radius = spine[n - 1].radius;
        assert_eq!(drag_end(&mut spine, true, 10.0, None, MAX_VERTEBRAE), EndEdit::Added);
        assert_eq!(spine.len(), n + 1);
        assert!((spine[n].radius - end_radius * NEW_VERTEBRA_RELATIVE_SCALE).abs() < 1e-6);
        assert_eq!(drag_end(&mut spine, false, -10.0, None, MAX_VERTEBRAE), EndEdit::Removed);
        assert_eq!(spine.len(), n);
        assert_eq!(drag_end(&mut spine, true, 10.0, None, n), EndEdit::None, "no room");
        let before = Vec3::from_array(spine[n - 1].position);
        let goal = before + Vec3::new(0.0, 0.3, 0.0);
        assert_eq!(drag_end(&mut spine, true, 0.0, Some(goal), MAX_VERTEBRAE), EndEdit::Curled);
        assert!(spine[n - 1].position[1] > before.y);
        // The curl is absolute: the same goal again changes nothing.
        assert_eq!(drag_end(&mut spine, true, 0.0, Some(goal), MAX_VERTEBRAE), EndEdit::None);
        while spine.len() > MIN_VERTEBRAE {
            drag_end(&mut spine, true, -10.0, None, MAX_VERTEBRAE);
        }
        assert_eq!(drag_end(&mut spine, true, -10.0, None, MAX_VERTEBRAE), EndEdit::None);
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_unlock_rows_and_pinning_rules() {
        use crate::creature_editor_data::{palette, CREATURE_PAINT_PALETTE};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let data = CreatureEditorData::load(&index);
        let s = data.setup(SetupKind::Creature).unwrap();
        println!("base unlocks {:08X?}", s.base_unlocks);
        let pal = data.parts_palette(SetupKind::Creature).unwrap();
        for c in &pal.categories {
            let free: Vec<&str> = c
                .pages
                .iter()
                .flat_map(|&p| pal.pages[p].items.iter())
                .filter_map(|i| data.parts.get(&i.part).filter(|x| x.starts_unlocked).map(|x| x.name.as_str()))
                .collect();
            println!("category {:08X} {}: {} free {:?}", c.instance, c.name, free.len(), free);
        }
        let free = data.parts.values().filter(|i| i.starts_unlocked).count();
        let c2c: usize = data.parts_palette(SetupKind::CellToCreature).unwrap().pages.iter().map(|p| p.items.len()).sum();
        assert!(free >= c2c + 20, "{free}");
        // Template rules: eyes orient to the skin, mouths prefer the centre.
        let jaw = &data.parts[&crate::world_objects::name_id("ce_mouth_jaw_carnivore_01")];
        assert!(jaw.prefer_symmetry_plane && jaw.orient_to_surface, "{jaw:?}");
        let eyes = pal.page(1, 0).unwrap();
        assert!(eyes.items.iter().any(|i| data.parts.get(&i.part).is_some_and(|x| x.orient_to_surface)));
        // The paint palette's structure (Complete / Partial with Base, Coat,
        // Detail).
        let paint = palette(&index, &mut crate::cell_editor_data::Locale::default(), CREATURE_PAINT_PALETTE);
        for c in &paint.categories {
            println!("paint category {:08X} '{}' seq {} pages {:?} icons {:08X?}", c.instance, c.name, c.sequence, c.pages,
                c.icons.iter().map(|k| (k.group_id, k.instance_id, index.records.contains_key(k))).collect::<Vec<_>>());
        }
        for p in &paint.pages {
            println!(
                "paint page {:08X} '{}' cat {:08X} {}x{} items {} first {:08X?}",
                p.instance,
                p.description,
                p.category,
                p.columns,
                p.rows,
                p.items.len(),
                p.items.first().map(|i| (i.part, i.group))
            );
        }
    }

    #[test]
    fn unlocks_follow_the_next_column_of_a_used_row() {
        use crate::creature_editor_data::{Palette, PaletteCategory, PaletteItem, PalettePage};
        let mut data = CreatureEditorData::default();
        let mut items = vec![];
        for row in 0..3u32 {
            for column in 0..4u32 {
                let part = 100 + row * 10 + column;
                let mut i = info(false, false);
                i.key.instance_id = part;
                i.model = 1000 + part;
                data.parts.insert(part, i);
                items.push(PaletteItem { part, group: 0, row, column });
            }
        }
        let palette = Palette {
            categories: vec![PaletteCategory { instance: 5, pages: vec![0], ..Default::default() }],
            pages: vec![PalettePage { columns: 4, rows: 3, items, ..Default::default() }],
            ..Default::default()
        };
        // Two base rows: their first column starts available.
        let free = crate::creature_editor_data::base_unlocked_parts(&palette, &[(5, 2)]);
        assert_eq!(free, vec![100, 110]);
        for p in &free {
            data.parts.get_mut(p).unwrap().starts_unlocked = true;
        }
        let mut run = RunProgress::default();
        run.parts.clear();
        assert!(unlocks::available(&run, &data.parts[&100]));
        assert!(!unlocks::available(&run, &data.parts[&101]));
        // No row in use: any row's next column (row 2 starts at column 0).
        let all: Vec<u32> = (0..3).filter_map(|r| unlocks::next_unlock(&palette, &data, &run, r)).collect();
        assert_eq!(all, vec![101, 111, 120]);
        // A used row comes first.
        run.parts = vec![PartInstance::new(1110, 0).with_definition(110)];
        assert_eq!(unlocks::next_unlock(&palette, &data, &run, 7), Some(111));
        run.creature_found_parts.push(111);
        assert_eq!(unlocks::next_unlock(&palette, &data, &run, 7), Some(112));
        assert!(unlocks::is_new(&run, 111));
        run.creature_creation.seen_parts.push(111);
        assert!(!unlocks::is_new(&run, 111));
        // Owned parts (old saves by model) count as available.
        run.parts = vec![PartInstance::new(1102, 0)];
        assert!(unlocks::available(&run, &data.parts[&102]));
    }
}
