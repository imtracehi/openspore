//! Creature Creator part handles (`notes/skin-re-notes.md` section 4,
//! `notes/creator-re-notes.md` section 2.5): deform handles and rotation
//! rings on a mounted part and its symmetric twin, with undo.
//!
//! * Deform handle (manipulator 0x9F48C094, drag 0x5ACFC0, VERIFIED maths):
//!   the handle's drag line P0 -> P1 is the part model's RW4 `RWMorphHandle`
//!   start/end transformed with the part; the mouse ray (C, D) meets the plane
//!   through P0 that contains A = normalize(P1 - P0) and faces the camera,
//!   `n = normalize((D x A) x A)`; the weight is
//!   `clamp(((H - P0) . A) / |P1 - P0|, 0, 1)`, written to the part and its
//!   twin. The posed mesh is the part's own morph (`morph.rs` through
//!   `editor::creation_mesh`). Handles: `modelHandleKeys`
//!   (DeformAxisLeft / Forward / Up), hidden by `modelHideDeformHandles`.
//! * Rotation ring (manipulator 0x855DE44D, drag 0x5B5B30, VERIFIED maths):
//!   view not edge-on (|D . n| >= 0.3): the signed angle around the ring axis
//!   between the press vector and the current ray-plane hit, plus the start
//!   angle; edge-on: start + 5 x (displacement along the ring tangent) /
//!   distance. Snap: when one of the part's axes is within `1 - dot < 0.05` of
//!   a creature axis the angle snaps to it for up to 1.35 s of continued
//!   dragging; the shown angle approaches the target at rate 14 (35 while
//!   snapping). Rings: `modelRotationRing{X,Y,Z}Axis` (a ring model, `None` =
//!   no ring), hidden by `modelHideRotationHandles` / `...IsHidden`.
//!
//! * Handle models (`HandleTuning`, VERIFIED from the data and the code
//!   image): the deform handle is `CE_basic_ui_teardrop` with its
//!   `_overdraw` model at scale 1.3 (the base class `SetScale` 0x47F8E0 sets
//!   the model scale to the caller's scale times `tuningEditorHandleDefaultScale`;
//!   `Load` 0x47E030 creates the overdraw model only when
//!   `tuningEditorHandleHasOverdraw` is set, which the ring and the rotation
//!   ball do not), coloured and alpha'd per state (default / rollover /
//!   active). The deform handle's update (0x4828B0) puts it at the lerp of the
//!   line at the weight (`GetPosition` 0x4824A0) and turns it with
//!   `FUN_004A8F00(out, n, up)` (0x4827D0): the model's Y along the line
//!   `P1 - P0` turned by the block's orientation, X = `n x up`. Hover and press
//!   test the mouse ray against the models' triangles (the hover pass builds
//!   its `FilterSettings` with collision mode 4 at 0x57B26x for the groups
//!   DeformHandle 0x1BA53EA, RotationRing 0x31390732, RotationBall 0x31390733,
//!   BallConnector 0x900C6CDD), not a pixel radius.
//!
//! ADAPTED (marked inline): the ring's size and place (the part's mesh extent;
//! the ring handle's position / radius code 0x484AD0 was not decoded), the
//! overdraw pass's depth state (an x-ray pass), the edge-on distance (camera
//! to ring), which part shows handles (the hovered one, kept while the cursor
//! stays near its handles). The free-angle gain (1.0, ctor 0x5B54C0) and the
//! approach curve (`FUN_0069B840`) are ported from the retail code.

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::CREATURE_PARTS,
    cell_editor_data::{key_list, key_of, number, prop_key},
    creature_editor::{CreatureEditor, CreatureEditorOwned, Mode},
    editor::EditorState,
    game::{PartInstance, Player, RunProgress},
    stages::StageOwned,
    Originals,
};

/// The Creature Creator's edits of one mounted part (saved with it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartEdits {
    /// Rotation-ring orientation (quaternion x, y, z, w) after the editor yaw;
    /// `None` = no ring rotation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
    /// Deform handle weights: (RW4 morph handle id = FNV of `DeformAxis*`,
    /// weight 0..1); handles not listed keep their default.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<(u32, f32)>,
    /// The limb chain of a limb assembly (`creature_limbs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limb: Option<crate::creature_limbs::Limb>,
    /// This part ends the limb with this id (a foot or hand); 0 = on the body.
    #[serde(skip_serializing_if = "is_zero")]
    pub on_limb: u32,
    /// With `on_limb`: 0 = the end part; k >= 1 = the k-th rigid segment of
    /// that limb (a `ce_limb_exo_*` mesh riding joints k-1 .. k).
    #[serde(skip_serializing_if = "is_zero")]
    pub segment: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// Twin limb ids differ in bit 31 (`PartEdits::mirrored`).
pub const TWIN_BIT: u32 = 0x8000_0000;

impl PartEdits {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    /// The ring rotation (identity when none).
    pub fn ring(&self) -> Quat {
        self.rotation.map_or(Quat::IDENTITY, |q| Quat::from_array(q).normalize())
    }
    pub fn set_ring(&mut self, q: Quat) {
        let q = q.normalize();
        self.rotation = (q.angle_between(Quat::IDENTITY) > 1e-5).then(|| q.to_array());
    }
    pub fn weight(&self, id: u32) -> Option<f32> {
        self.handles.iter().find(|h| h.0 == id).map(|h| h.1)
    }
    pub fn set_weight(&mut self, id: u32, w: f32) {
        match self.handles.iter_mut().find(|h| h.0 == id) {
            Some(h) => h.1 = w,
            None => self.handles.push((id, w)),
        }
    }
    /// The edits of the symmetric twin (mirrored across the creature's
    /// sagittal plane x = 0): the ring rotation reflected (`Mx R Mx`), the same
    /// handle weights (retail writes the weight to the twin too), the limb's
    /// joints reflected, limb ids with [`TWIN_BIT`] flipped.
    pub fn mirrored(&self) -> Self {
        Self {
            rotation: self.rotation.map(|[x, y, z, w]| [x, -y, -z, w]),
            handles: self.handles.clone(),
            limb: self.limb.as_ref().map(crate::creature_limbs::Limb::mirrored),
            on_limb: if self.on_limb == 0 { 0 } else { self.on_limb ^ TWIN_BIT },
            segment: self.segment,
        }
    }
}

/// Copy part `index`'s edits (mirrored) to its symmetric twins.
pub fn sync_twins(parts: &mut [PartInstance], index: usize) {
    let Some(p) = parts.get(index).filter(|p| p.pair_id != 0) else { return };
    let (pair, edits) = (p.pair_id, p.edits.mirrored());
    for (i, q) in parts.iter_mut().enumerate() {
        if i != index && q.pair_id == pair {
            q.edits = edits.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Maths (pure, VERIFIED unless marked)
// ---------------------------------------------------------------------------

/// Deform handle weight for the mouse ray (origin `c`, direction `d`) and the
/// handle line `p0 -> p1` (FUN_005ACFC0). `None` when the plane hit is behind
/// the camera or degenerate.
pub fn deform_weight(c: Vec3, d: Vec3, p0: Vec3, p1: Vec3, lo: f32, hi: f32) -> Option<f32> {
    let len = p0.distance(p1);
    if len < 1e-6 {
        return None;
    }
    let a = (p1 - p0) / len;
    let n = d.cross(a).cross(a).normalize_or_zero();
    let nd = n.dot(d);
    if n == Vec3::ZERO || nd.abs() < 1e-6 {
        return None;
    }
    let t = -n.dot(c - p0) / nd;
    if t < 0.0 {
        return None;
    }
    let h = c + d * t;
    Some(((h - p0).dot(a) / len).clamp(lo, hi))
}

/// `FUN_0069B760`: signed angle from `a` to `b` around `axis`.
pub fn signed_angle(a: Vec3, b: Vec3, axis: Vec3) -> f32 {
    let n = axis.normalize_or_zero();
    let a = a - n * a.dot(n);
    let b = b - n * b.dot(n);
    a.cross(b).dot(n).atan2(a.dot(b))
}

/// Retail edge-on limit: `|D . n| < 0.3` drags along the ring's tangent.
pub const EDGE_ON: f32 = 0.3;
/// `DAT_01512F28`: radians per unit of tangent displacement over distance.
pub const EDGE_ON_GAIN: f32 = 5.0;
/// `param_1 + 0x13C` gain of the free angle: the ring manipulator's constructor 0x5B54C0 writes 1.0
/// (0x5B5536) and it is the only store of that field in 0x5B2000..0x5B8000 (VERIFIED).
pub const RING_GAIN: f32 = 1.0;
/// Snap zone: `1 - dot < 0.05` between a part axis and a creature axis.
pub const SNAP_ZONE: f32 = 0.05;
/// Snapping holds for at most this long while dragging on.
pub const SNAP_SECONDS: f32 = 1.35;
/// Approach rates of the shown angle (FUN_0069B840).
pub const APPROACH_RATE: f32 = 14.0;
pub const APPROACH_RATE_SNAP: f32 = 35.0;

/// The ring angle for a mouse ray (origin `c`, direction `d`): `centre` and
/// `axis` of the ring, `press` = the press vector (hit - centre) in the ring
/// plane, `press_point` = the press hit for the edge-on case, `distance` = the
/// edge-on divisor (ADAPTED: camera to ring centre).
pub fn ring_angle(c: Vec3, d: Vec3, centre: Vec3, axis: Vec3, press: Vec3, press_point: Vec3, start: f32, distance: f32) -> Option<f32> {
    let n = axis.normalize_or_zero();
    let dn = d.dot(n);
    if dn.abs() >= EDGE_ON {
        let t = -(c - centre).dot(n) / dn;
        if t < 0.0 {
            return None;
        }
        let hit = c + d * t - centre;
        return Some(signed_angle(press, hit, n) * RING_GAIN + start);
    }
    // Edge-on: the camera-facing plane through the press point.
    let dd = d.length_squared();
    if dd < 1e-12 {
        return None;
    }
    let t = -(c - press_point).dot(d) / dd;
    if t < 0.0 {
        return None;
    }
    let moved = c + d * t - press_point;
    let tangent = n.cross(d).normalize_or_zero();
    Some(start + EDGE_ON_GAIN * moved.dot(tangent) / distance.max(1e-3))
}

/// Snap of a ring rotation: `total(angle)` is the part's orientation at a ring
/// angle, `axis` the ring axis (creature frame). When one of the part's axes
/// is within [`SNAP_ZONE`] of a creature axis, the angle that puts it exactly
/// on that axis.
pub fn snap_angle(total: impl Fn(f32) -> Quat, axis: Vec3, angle: f32) -> Option<f32> {
    let q = total(angle);
    let n = axis.normalize_or_zero();
    let world = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
    let mut best: Option<(f32, f32)> = None;
    for e in [Vec3::X, Vec3::Y, Vec3::Z] {
        let u = q * e;
        for w in world {
            let gap = 1.0 - u.dot(w);
            if gap >= SNAP_ZONE {
                continue;
            }
            // Turning about the ring axis can only align the parts of both
            // vectors in the ring plane.
            let (pu, pw) = (u - n * u.dot(n), w - n * w.dot(n));
            if pu.length_squared() < 1e-4 || pw.length_squared() < 1e-4 {
                continue;
            }
            let fix = signed_angle(pu, pw, n);
            if best.is_none_or(|(g, _)| gap < g) {
                best = Some((gap, angle + fix));
            }
        }
    }
    best.map(|(_, a)| a)
}

/// `FUN_00699730`: the angular distance of two angles (both wrapped to +-pi, then the
/// shorter way round).
pub fn angle_distance(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let wrap = |x: f32| {
        let m = x % TAU;
        if m >= PI {
            m - TAU
        } else {
            m
        }
    };
    let (a, b) = (wrap(a), wrap(b));
    let (lo, hi) = (a.min(b), a.max(b));
    (hi - lo).min(TAU - hi + lo)
}

/// Approach of the shown ring angle to the target, ported from `FUN_0069B840(shown, target,
/// step)` as the ring manipulator's update calls it (0x5B6550; VERIFIED): `step = angle
/// distance x rate x dt` (`FUN_00699730`), so it is an exponential approach, and it jumps to
/// the target once the step reaches the distance. Otherwise the shown angle moves by `step`
/// the shorter way round the circle (the candidate nearer the target) and is wrapped to
/// +-pi (`fmod 2 pi`, subtract 2 pi from values >= pi).
pub fn approach(shown: f32, target: f32, rate: f32, dt: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let distance = angle_distance(target, shown);
    let step = distance * rate * dt;
    if step >= distance {
        return target;
    }
    let (up, down) = (shown + step, shown - step);
    let next = if angle_distance(down, target) <= angle_distance(up, target) { down } else { up };
    let m = next % TAU;
    if m >= PI {
        m - TAU
    } else {
        m
    }
}

// ---------------------------------------------------------------------------
// Part handle data
// ---------------------------------------------------------------------------

/// `modelHandleKeys`.
pub const HANDLE_KEYS: u32 = 0x8ECB_344A;
/// `modelHideDeformHandles`.
pub const HIDE_DEFORM: u32 = 0x9416_CEA1;
/// `modelHideRotationHandles`.
pub const HIDE_ROTATION: u32 = 0x0493_691F;
/// `modelRotationRing{X,Y,Z}Axis` (ring model key) and `...IsHidden`.
pub const RING_AXES: [u32; 3] = [0x98D0_EA44, 0x98D0_EA46, 0x98D0_EA48];
pub const RING_HIDDEN: [u32; 3] = [0x0460_4B00, 0x0460_4B01, 0x0460_4B02];

/// One deform handle in the part model's own coordinates (Spore, Z up).
#[derive(Clone, Debug, PartialEq)]
pub struct HandleDef {
    pub id: u32,
    pub start: Vec3,
    pub end: Vec3,
    pub default: f32,
}

/// What a part definition offers to the handle manipulators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartHandles {
    pub handles: Vec<HandleDef>,
    /// Rings around the model's X / Y / Z axes.
    pub rings: [bool; 3],
    /// Model -> mesh (`editor::assembled_mesh`): `basis * (v + offset)`.
    pub basis: Quat,
    pub offset: Vec3,
}

impl PartHandles {
    pub fn to_mesh(&self, v: Vec3) -> Vec3 {
        self.basis * (v + self.offset)
    }
}

pub fn load_part_handles(index: &crate::asset_catalog::ResourceIndex, def: &crate::asset_catalog::PartDefinition) -> PartHandles {
    let basis = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Quat::from_euler(
            EulerRot::XYZ,
            def.model_rotation[0].to_radians(),
            def.model_rotation[1].to_radians(),
            def.model_rotation[2].to_radians(),
        );
    let mut out = PartHandles { basis, offset: Vec3::from_array(def.model_offset), ..default() };
    let Ok(p) = index.properties(prop_key(def.key.group_id, def.key.instance_id)) else { return out };
    let flag = |id: u32| number(&p, id).is_some_and(|v| v != 0.0);
    if !flag(HIDE_ROTATION) {
        let none = crate::world_objects::name_id("None");
        for i in 0..3 {
            out.rings[i] = key_of(&p, RING_AXES[i]).is_some_and(|k| k.instance_id != 0 && k.instance_id != none && k.instance_id != u32::MAX)
                && !flag(RING_HIDDEN[i]);
        }
    }
    if flag(HIDE_DEFORM) {
        return out;
    }
    let keys: Vec<u32> = key_list(&p, HANDLE_KEYS).iter().map(|k| k.instance_id).collect();
    let Ok(bytes) = index.read(def.model) else { return out };
    let Ok(data) = crate::rw4::extract_morph_data(&bytes) else { return out };
    out.handles = data
        .handles
        .iter()
        .filter(|h| keys.is_empty() || keys.contains(&h.id))
        .map(|h| HandleDef {
            id: h.id,
            start: Vec3::new(h.start[0] as f32, h.start[1] as f32, h.start[2] as f32),
            end: Vec3::new(h.end[0] as f32, h.end[1] as f32, h.end[2] as f32),
            default: h.default_time.clamp(0.0, 1.0),
        })
        .filter(|h| h.start.distance(h.end) > 1e-5)
        .collect();
    out
}

/// Per-definition handle data and posed meshes (per app).
#[derive(Resource, Default)]
pub struct HandleCache {
    parts: HashMap<u32, PartHandles>,
    /// The installed handle models (`None` = not tried yet, `Some(None)` = unavailable).
    models: Option<Option<Arc<HandleModels>>>,
    /// (definition, quantized weights) -> posed mesh.
    posed: HashMap<(u32, Vec<(u32, u16)>), Option<Handle<Mesh>>>,
}

impl HandleCache {
    /// The installed handle models, loaded on first use.
    pub fn models(&mut self, originals: &Originals, meshes: &mut Assets<Mesh>) -> Option<Arc<HandleModels>> {
        self.models
            .get_or_insert_with(|| match load_handle_models(&originals.resource_index, meshes) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    warn!("handle models: {e}");
                    None
                }
            })
            .clone()
    }

    pub fn part(&mut self, originals: &Originals, p: &PartInstance) -> Option<&PartHandles> {
        let def = originals.part_catalog.by_part_in(CREATURE_PARTS, p)?;
        let id = def.key.instance_id;
        if !self.parts.contains_key(&id) {
            let h = load_part_handles(&originals.resource_index, def);
            self.parts.insert(id, h);
        }
        self.parts.get(&id)
    }
}

/// Weight steps of the posed-mesh cache.
const WEIGHT_STEPS: f32 = 256.0;

fn posed_mesh(cache: &mut HandleCache, originals: &Originals, p: &PartInstance, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
    let def = originals.part_catalog.by_part_in(CREATURE_PARTS, p)?.clone();
    let mut q: Vec<(u32, u16)> = p.edits.handles.iter().map(|(id, w)| (*id, (w.clamp(0.0, 1.0) * WEIGHT_STEPS).round() as u16)).collect();
    q.sort_unstable();
    let key = (def.key.instance_id, q.clone());
    if let Some(h) = cache.posed.get(&key) {
        return h.clone();
    }
    let weights: Vec<(u32, f32)> = q.iter().map(|(id, s)| (*id, *s as f32 / WEIGHT_STEPS)).collect();
    let section = crate::editor::model_for_part(originals, p).and_then(|m| m.surfaces.first().map(|s| s.source_section));
    let made = match crate::editor::creation_mesh(originals, &def, &weights) {
        Ok((combined, surfaces, applied)) => {
            let mut mesh = section.and_then(|s| surfaces.into_iter().find(|(sec, _)| *sec == s).map(|(_, m)| m)).unwrap_or(combined);
            debug!("posed {}: {:?} -> {} vertices moved, extent {:?}", def.name, weights, applied.moved_vertices, mesh_bounds(&mesh));
            let _ = mesh.generate_tangents();
            Some(meshes.add(mesh))
        }
        Err(e) => {
            warn!("deform handles of {}: {e}", def.name);
            None
        }
    };
    if cache.posed.len() > 512 {
        cache.posed.clear();
    }
    cache.posed.insert(key, made.clone());
    made
}

/// Every mounted part with handle weights shows its posed mesh (creators and
/// the Creature stage; `editor::refresh_parts` and `creature` spawn the
/// unposed one).
pub fn apply_posed_meshes(
    run: Option<Res<RunProgress>>,
    originals: Res<Originals>,
    mut cache: ResMut<HandleCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<(&crate::creature::Accessory, &mut Mesh3d), Without<crate::creature_morph::PartMorph>>,
) {
    let Some(run) = run else { return };
    for (acc, mut mesh) in &mut parts {
        let Some(p) = run.parts.get(acc.part).filter(|p| !p.edits.handles.is_empty()) else { continue };
        if let Some(h) = posed_mesh(&mut cache, &originals, p, &mut meshes) {
            if mesh.0 != h {
                mesh.0 = h;
            }
        }
    }
}

/// Capture option `--handles-demo`: when no mounted part has deform handles,
/// mount the first installed part with at least two (on the top of the head)
/// so the capture shows them.
#[allow(clippy::too_many_arguments)]
pub fn demo_part(
    state: Option<Res<State<crate::stages::GameStage>>>,
    editor: Option<Res<CreatureEditor>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    mut cache: ResMut<HandleCache>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    mut editor_state: ResMut<EditorState>,
    mut done: Local<bool>,
) {
    if *done || state.is_none_or(|s| *s.get() != crate::stages::GameStage::Creature) || editor.is_none_or(|e| !e.active) {
        return;
    }
    *done = true;
    if !std::env::args().any(|a| a == "--handles-demo") {
        return;
    }
    if run.parts.iter().any(|p| cache.part(&originals, p).is_some_and(|h| !h.handles.is_empty())) {
        return;
    }
    let Some(def) = originals
        .part_catalog
        .parts
        .iter()
        .filter(|d| d.key.group_id == CREATURE_PARTS && !d.is_null && d.category() != "Mouths")
        .find(|d| load_part_handles(&originals.resource_index, d).handles.len() >= 2)
        .cloned()
    else {
        return;
    };
    if let Err(e) = crate::editor::ensure_model(&mut originals, &def, &mut meshes, &mut images, &mut materials) {
        warn!("handles demo part: {e}");
        return;
    }
    let body = run.body.clone();
    let slot = crate::baked_model::attachment_point(&originals, &body, 0);
    let top = crate::baked_model::nearest_body_surface(&originals, &body, slot + Vec3::new(0.0, 0.6, 0.35));
    let mut part = PartInstance::new(def.model.instance_id, 0).with_definition(def.key.instance_id);
    part.offset = crate::game::clamp_offset((top - slot).to_array(), crate::game::OFFSET_LIMIT);
    info!("handles demo: mounted {}", def.name);
    run.parts.push(part);
    editor_state.visuals_dirty = true;
}

// ---------------------------------------------------------------------------
// Handle models (`HandleTuning`): the teardrop, the ring and the sphere
// ---------------------------------------------------------------------------

/// Group of the editor's UI models (`0x40606000`); `HandleTuning` names its
/// models with group 0 (`0!CE_basic_ui_teardrop`), they are installed here.
pub const HANDLE_MODEL_GROUP: u32 = 0x4060_6000;
/// `CE_basic_ui_teardrop` / `_overdraw`, `ce_simple_sphere` / `_overdraw`,
/// `ui_rotateringcreature` / `_overdraw` (registered names, FNV-1).
pub const TEARDROP: u32 = 0xBA7C_9E01;
pub const TEARDROP_OVERDRAW: u32 = 0xBCD8_D200;
pub const SPHERE: u32 = 0x0CA4_0C72;
pub const SPHERE_OVERDRAW: u32 = 0x4D44_58FD;
pub const RING: u32 = 0x0C1E_F506;
pub const RING_OVERDRAW: u32 = 0x34F2_BA39;
/// `tuningEditorHandleDefaultScale` of the tuning files (`HandleTuning`, dump
/// `handle_tuning_10CB4FA8.txt`): the base class `SetScale` 0x47F8E0 sets the
/// model's scale to the caller's scale times this (`+0x4C`, read by 0x47DE50).
pub const DEFORM_SCALE: f32 = 1.3;
/// `HandleTuning!BuildingDeformHandle`: the building setup's
/// `editorDeformHandleTuningFile` (`BuildingTemplate` D817CD63).
pub const BUILDING_DEFORM_SCALE: f32 = 1.2;
/// `HandleTuning!defaultspinehandle` (the two end handles of the spine).
pub const SPINE_END_SCALE: f32 = 2.0;
/// `HandleTuning!defaultrotationringhandle` / `defaultrotationballhandle` /
/// `defaultballconnectorhandle`.
pub const RING_MODEL_SCALE: f32 = 0.5;
pub const ROTATION_BALL_SCALE: f32 = 0.16666;
pub const BALL_CONNECTOR_SCALE: f32 = 0.25;
/// Model alpha per state (default, rollover, active) of `DefaultHandle`
/// (`tuningEditorHandleAlpha*`) and of the overdraw model
/// (`tuningEditorOverdrawHandleAlpha*`).
pub const STATE_ALPHA: [f32; 3] = [0.83, 1.0, 1.0];
pub const STATE_OVERDRAW_ALPHA: [f32; 3] = [0.25, 1.0, 1.0];
/// The spine handle's own tuning (`defaultspinehandle`): alpha 1, overdraw 0.85.
pub const SPINE_ALPHA: [f32; 3] = [1.0, 1.0, 1.0];
pub const SPINE_OVERDRAW_ALPHA: [f32; 3] = [0.85, 1.0, 1.0];

/// Geometry of one handle model in the model's own (Spore, Z up) coordinates.
#[derive(Clone, Debug, Default)]
pub struct RawHandleModel {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// The RW4 geometry of a handle model, untransformed.
pub fn raw_handle_model(bytes: &[u8]) -> Result<RawHandleModel, String> {
    let mut stats = crate::rw4::MeshStats::default();
    let mut out = RawHandleModel::default();
    for m in crate::rw4::extract_meshes(bytes, &mut stats).iter().filter(|m| m.valid()) {
        let base = out.positions.len() as u32;
        out.positions.extend_from_slice(&m.positions);
        if m.normals.len() == m.positions.len() {
            out.normals.extend_from_slice(&m.normals);
        } else {
            out.normals.extend(std::iter::repeat_n([0.0, 1.0, 0.0], m.positions.len()));
        }
        out.indices.extend(m.indices.iter().map(|i| base + *i as u32));
    }
    if out.indices.is_empty() {
        return Err("handle model has no geometry".into());
    }
    Ok(out)
}

/// A handle model: the drawn mesh (model coordinates) and its triangles, which
/// the picking tests (retail picks the handle groups at collision mode 4, the
/// triangle mesh: the filter built at 0x57B26x stores 4 in `FilterSettings`).
#[derive(Clone)]
pub struct HandleModel {
    pub mesh: Handle<Mesh>,
    pub tris: Arc<Vec<[Vec3; 3]>>,
    pub min: Vec3,
    pub max: Vec3,
}

impl HandleModel {
    pub fn from_raw(raw: &RawHandleModel, meshes: &mut Assets<Mesh>) -> Self {
        let p: Vec<Vec3> = raw.positions.iter().map(|v| Vec3::from_array(*v)).collect();
        let tris: Vec<[Vec3; 3]> = raw.indices.chunks_exact(3).map(|t| [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]).collect();
        let (min, max) = p.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, raw.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, raw.normals.clone());
        mesh.insert_indices(bevy::mesh::Indices::U32(raw.indices.clone()));
        Self { mesh: meshes.add(mesh), tris: Arc::new(tris), min, max }
    }

    /// Distance along the ray (origin, direction) to the model placed at `at`
    /// (the nearest triangle, both faces), `None` when it misses.
    pub fn pick(&self, at: &Transform, origin: Vec3, dir: Vec3) -> Option<f32> {
        let affine = at.compute_affine();
        if affine.matrix3.determinant().abs() < 1e-12 {
            return None;
        }
        let inverse = affine.inverse();
        let (o, d) = (inverse.transform_point3(origin), inverse.transform_vector3(dir));
        self.tris
            .iter()
            .filter_map(|t| crate::creature_editor_edit::ray_triangle(o, d, t[0], t[1], t[2]))
            .min_by(f32::total_cmp)
    }

    /// Largest distance of a vertex from the model's Y axis (a ring's outer radius).
    pub fn radius_about_y(&self) -> f32 {
        self.tris.iter().flatten().map(|v| Vec2::new(v.x, v.z).length()).fold(0.0, f32::max)
    }
}

/// The installed handle models.
#[derive(Clone)]
pub struct HandleModels {
    pub teardrop: HandleModel,
    pub teardrop_overdraw: HandleModel,
    pub sphere: HandleModel,
    pub sphere_overdraw: HandleModel,
    pub ring: HandleModel,
}

pub fn load_handle_models(index: &crate::asset_catalog::ResourceIndex, meshes: &mut Assets<Mesh>) -> Result<HandleModels, String> {
    let mut load = |instance: u32| -> Result<HandleModel, String> {
        let key = crate::dbpf::ResourceKey { type_id: crate::asset_catalog::RW4, group_id: HANDLE_MODEL_GROUP, instance_id: instance };
        let raw = raw_handle_model(&index.read(key)?).map_err(|e| format!("{instance:08X}: {e}"))?;
        Ok(HandleModel::from_raw(&raw, meshes))
    };
    Ok(HandleModels {
        teardrop: load(TEARDROP)?,
        teardrop_overdraw: load(TEARDROP_OVERDRAW)?,
        sphere: load(SPHERE)?,
        sphere_overdraw: load(SPHERE_OVERDRAW)?,
        ring: load(RING)?,
    })
}

/// `FUN_004A8F00(out, n, up)` as the deform handle's orientation (0x4827D0):
/// the model's local Y runs along `dir` (the handle line `P1 - P0` turned by the
/// block's orientation), local X is `dir x up` (horizontal) and local Z
/// `X x Y` (the world up as far as `dir` allows). `dir` along the up axis takes
/// the retail degenerate rows ((-1,0,0), (0,0,1), (0,1,0) for +up and
/// (1,0,0), (0,0,-1), (0,1,0) for -up in Spore's frame; here the part
/// importer's frame has Spore Y = -Z). Returns the model-to-world rotation.
pub fn deform_handle_rotation(dir: Vec3) -> Quat {
    let n = dir.normalize_or(Vec3::Y);
    let c = n.cross(Vec3::Y);
    let (x, y, z) = if c.length_squared() < 1e-8 {
        if n.y > 0.0 {
            (Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z)
        } else {
            (Vec3::X, Vec3::NEG_Y, Vec3::NEG_Z)
        }
    } else {
        let x = c.normalize();
        (x, n, x.cross(n))
    };
    Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize()
}

/// Placement of a deform handle's model: at weight `w` on the line `p0 -> p1`
/// (`GetPosition` 0x4824A0: the line's lerp), turned by [`deform_handle_rotation`],
/// scaled uniformly by `scale` (tuning scale x the caller's `SetScale`).
pub fn deform_handle_transform(p0: Vec3, p1: Vec3, w: f32, scale: f32) -> Transform {
    Transform::from_translation(p0.lerp(p1, w)).with_rotation(deform_handle_rotation(p1 - p0)).with_scale(Vec3::splat(scale))
}

/// Material of the overdraw model: the handle drawn only where the scene hides
/// it (the model groups `DeformHandleOverdraw` 0x1BA53EB / `Overdraw`
/// 0x22FFF11 are drawn in their own pass; ADAPTED: that pass's depth state was
/// not decoded, an x-ray pass is the reading of "overdraw").
pub type OverdrawMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, HandleOverdraw>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct HandleOverdraw {
    #[uniform(100)]
    pub tag: Vec4,
}

impl bevy::pbr::MaterialExtension for HandleOverdraw {
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            // Reverse-Z: a fragment behind the scene has the smaller depth.
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Less);
        }
        Ok(())
    }
}

/// Register the overdraw material (render apps: called from `TerrainPlugin::build`;
/// headless test apps have no material plugins and simply draw no handles).
pub fn build(app: &mut App) {
    app.add_plugins(bevy::pbr::MaterialPlugin::<OverdrawMaterial>::default());
}

/// The three looks (default, rollover, active) of a handle: the model pass and
/// the overdraw pass.
#[derive(Clone)]
pub struct HandleLook {
    pub main: [Handle<StandardMaterial>; 3],
    pub overdraw: [Handle<OverdrawMaterial>; 3],
}

/// Build the looks from the tuning colours (default, rollover, active) and the
/// alphas of the model and of the overdraw model per state.
pub fn handle_look(
    colors: [[f32; 3]; 3],
    alpha: [f32; 3],
    overdraw_alpha: [f32; 3],
    materials: &mut Assets<StandardMaterial>,
    overdraw: &mut Assets<OverdrawMaterial>,
) -> HandleLook {
    let base = |c: [f32; 3], a: f32| StandardMaterial {
        // The tuning colours carry HDR channels (1.2): the part above 1 goes to the emission.
        base_color: Color::srgba(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0), a),
        emissive: LinearRgba::rgb(c[0] * 0.5, c[1] * 0.5, c[2] * 0.5),
        perceptual_roughness: 0.55,
        alpha_mode: AlphaMode::Blend,
        ..default()
    };
    let main = std::array::from_fn(|i| materials.add(base(colors[i], alpha[i])));
    let over = std::array::from_fn(|i| {
        let mut m = base(colors[i], overdraw_alpha[i]);
        m.unlit = true;
        overdraw.add(OverdrawMaterial { base: m, extension: HandleOverdraw::default() })
    });
    HandleLook { main, overdraw: over }
}

/// `DefaultHandle` colours: default, rollover, active.
pub fn default_handle_colors() -> [[f32; 3]; 3] {
    [[HANDLE_COLOR[0], HANDLE_COLOR[1], HANDLE_COLOR[2]], HANDLE_ROLLOVER, HANDLE_ACTIVE]
}

// ---------------------------------------------------------------------------
// Manipulators (Bevy)
// ---------------------------------------------------------------------------

/// `HandleTuning` 0x10CB4FA8: default (0.668, 0.747, 1.2) alpha 0.83, rollover
/// (0.541, 0.808, 0.541), active (1, 1.2, 0.416) (HDR values kept).
pub const HANDLE_COLOR: [f32; 4] = [0.668, 0.747, 1.2, 0.83];
pub const HANDLE_ROLLOVER: [f32; 3] = [0.541, 0.808, 0.541];
pub const HANDLE_ACTIVE: [f32; 3] = [1.0, 1.2, 0.416];
/// ADAPTED: a part keeps showing its handles while the cursor stays within
/// this many pixels of one (retail shows the handles of the rolled-over /
/// selected block; the picking itself is against the models, not a radius).
const NEAR_PX: f32 = 28.0;
/// ADAPTED: ring radius over the part's extent.
const RING_SCALE: f32 = 0.65;
/// Size of the handle models in the creature's frame, in retail editor units
/// (`HandleTuning` scales are in editor units; the creature frame is the
/// editor's: the camera distance is the retail zoom times the player scale).
const CREATURE_HANDLE_UNIT: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grip {
    Deform(u32),
    Ring(usize),
}

#[derive(Clone, Debug)]
struct RingDrag {
    axis_body: Vec3,
    total: Quat,
    yaw: f32,
    centre: Vec3,
    axis: Vec3,
    press: Vec3,
    press_point: Vec3,
    distance: f32,
    shown: f32,
    snap_time: f32,
}

#[derive(Clone, Debug)]
struct Active {
    part: usize,
    grip: Grip,
    before: RunProgress,
    p0: Vec3,
    p1: Vec3,
    ring: Option<RingDrag>,
}

/// One visible handle of the shown part (world space).
#[derive(Clone, Debug)]
struct Shown {
    grip: Grip,
    at: Vec3,
    /// Rings: centre, world axis, radius.
    ring: Option<(Vec3, Vec3, f32)>,
    /// Deform: the drag line.
    line: Option<(Vec3, Vec3)>,
    /// Placement of the handle's model (model coordinates to world).
    tf: Transform,
}

#[derive(Resource, Default)]
pub struct HandleState {
    part: Option<usize>,
    shown: Vec<Shown>,
    hover: Option<Grip>,
    active: Option<Active>,
    /// Capture option `--handles-demo`: this part shows its handles.
    forced: Option<usize>,
}

impl HandleState {
    /// A handle drag is in progress (the camera and other manipulators wait).
    pub fn held(&self) -> bool {
        self.active.is_some()
    }
}

#[derive(Component)]
pub struct HandleVisual(pub Grip);

/// The overdraw model of a handle (the second pass).
#[derive(Component)]
pub struct OverdrawPass;

fn world_transform_of(pt: &Transform, p: &PartInstance, originals: &Originals, body: &crate::body::BodyProfile) -> Option<Transform> {
    let model = crate::editor::model_for_part(originals, p)?;
    let slot = crate::baked_model::attachment_point(originals, body, p.bone);
    Some(
        Transform::from_translation(pt.translation + pt.rotation * ((slot + Vec3::from_array(p.offset)) * pt.scale))
            .with_rotation(pt.rotation * p.rotation())
            .with_scale(p.mesh_scale(model.scale * 0.5 * p.scale) * pt.scale),
    )
}

fn mesh_bounds(mesh: &Mesh) -> Option<(Vec3, Vec3)> {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let (min, max) = pos.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(Vec3::from_array(*p)), b.max(Vec3::from_array(*p))));
    (min.is_finite() && max.is_finite()).then_some((min, max))
}

/// Centre and half extent of the part's shown mesh (mesh space).
fn mesh_extent(meshes: &Assets<Mesh>, mesh: &Handle<Mesh>) -> (Vec3, f32) {
    match meshes.get(mesh).and_then(mesh_bounds) {
        Some((min, max)) => ((min + max) * 0.5, ((max - min) * 0.5).max_element().max(0.05)),
        None => (Vec3::ZERO, 1.0),
    }
}

fn ray_point_distance(origin: Vec3, dir: Vec3, p: Vec3) -> (f32, f32) {
    let along = (p - origin).dot(dir);
    (along, (p - (origin + dir * along)).length())
}

/// Press on a handle (before `editor::drag`, which would grab the part): the
/// press is taken from the other manipulators.
#[allow(clippy::too_many_arguments)]
pub fn press(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    players: Query<&Transform, With<Player>>,
    editor: Option<Res<CreatureEditor>>,
    state: Res<EditorState>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    mut handles: ResMut<HandleState>,
    run: Res<RunProgress>,
) {
    let Some(editor) = editor.filter(|e| e.active && e.mode == Mode::Build && !e.options_open && !e.naming) else {
        handles.active = None;
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) || handles.active.is_some() || state.dragging() || pointer.over_ui {
        return;
    }
    let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    if state.retail_layout.is_some_and(|l| l.over_ui(cursor)) {
        return;
    }
    let (Some(index), Some(grip)) = (handles.part, handles.hover) else { return };
    let Some(p) = run.parts.get(index) else { return };
    let Some(shown) = handles.shown.iter().find(|s| s.grip == grip).cloned() else { return };
    let Ok(ray) = camera.viewport_to_world(cam, cursor) else { return };
    let mut active = Active { part: index, grip, before: run.clone(), p0: Vec3::ZERO, p1: Vec3::ZERO, ring: None };
    match grip {
        Grip::Deform(_) => {
            let Some((p0, p1)) = shown.line else { return };
            active.p0 = p0;
            active.p1 = p1;
        }
        Grip::Ring(_) => {
            let Some((centre, axis, _)) = shown.ring else { return };
            let d = *ray.direction;
            let t = -(ray.origin - centre).dot(axis) / d.dot(axis);
            let hit = if t.is_finite() && t > 0.0 { ray.origin + d * t } else { shown.at };
            active.ring = Some(RingDrag {
                axis_body: (pt.rotation.conjugate() * axis).normalize_or(Vec3::Y),
                total: p.rotation(),
                yaw: p.yaw,
                centre,
                axis,
                press: hit - centre,
                press_point: shown.at,
                distance: cam.translation().distance(centre),
                shown: 0.0,
                snap_time: 0.0,
            });
        }
    }
    let _ = editor;
    handles.active = Some(active);
    // The press belongs to the handle: the part pick, the spine and the
    // camera do not see it.
    mouse.clear_just_pressed(MouseButton::Left);
}

/// Hover, drag and visuals of the shown part's handles (in the creator chain
/// after the editing manipulators, before the camera).
#[allow(clippy::too_many_arguments)]
pub fn manipulate(
    mut commands: Commands,
    (time, mouse): (Res<Time>, Res<ButtonInput<MouseButton>>),
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    (players, accessories): (
        Query<&Transform, With<Player>>,
        Query<(&crate::creature::Accessory, &Transform), (Without<HandleVisual>, Without<Player>)>,
    ),
    mut visuals: Query<
        (Entity, &HandleVisual, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<Player>, Without<crate::creature::Accessory>, Without<OverdrawPass>),
    >,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut editor: ResMut<CreatureEditor>,
    mut state: ResMut<EditorState>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    mut handles: ResMut<HandleState>,
    mut cache: ResMut<HandleCache>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    (mut assets, mut demo_done, mut overdraw_materials, mut overdraw_visuals): (
        Local<Option<HandleLook>>,
        Local<bool>,
        Option<ResMut<Assets<OverdrawMaterial>>>,
        Query<
            (Entity, &HandleVisual, &mut Transform, &MeshMaterial3d<OverdrawMaterial>),
            (With<OverdrawPass>, Without<Player>, Without<crate::creature::Accessory>),
        >,
    ),
) {
    let build = editor.mode == Mode::Build && !editor.options_open && !editor.naming;
    // Capture option `--handles-demo`: the first part with deform handles gets
    // every handle at full weight (`--handles-demo-ring`: and a 0.5 rad turn
    // on its first ring; `--handles-demo-rest`: neither) and keeps showing its
    // handles.
    if !*demo_done && std::env::args().any(|a| a == "--handles-demo") && !run.parts.is_empty() {
        let pick = (0..run.parts.len()).find(|&i| cache.part(&originals, &run.parts[i]).is_some_and(|h| !h.handles.is_empty()));
        if let Some(i) = pick {
            let mut info = cache.part(&originals, &run.parts[i]).cloned().unwrap_or_default();
            if std::env::args().any(|a| a == "--handles-demo-rest") {
                info.handles.clear();
                info.rings = [false; 3];
            }
            for h in &info.handles {
                run.parts[i].edits.set_weight(h.id, 1.0);
            }
            if let Some(axis) = info.rings.iter().position(|r| *r).filter(|_| std::env::args().any(|a| a == "--handles-demo-ring")) {
                let ring = run.parts[i].edits.ring() * Quat::from_axis_angle(info.basis * [Vec3::X, Vec3::Y, Vec3::Z][axis], 0.5);
                run.parts[i].edits.set_ring(ring);
            }
            sync_twins(&mut run.parts, i);
            handles.forced = Some(i);
            *demo_done = true;
            state.visuals_dirty = true;
            info!("handles demo: part {i} ({} handles, rings {:?})", info.handles.len(), info.rings);
        }
    }
    let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else { return };
    let pt = *pt;
    let cursor = window.cursor_position();
    let ray = cursor.and_then(|c| camera.viewport_to_world(cam, c).ok());

    // Drag.
    if let Some(mut active) = handles.active.take() {
        if !build || !mouse.pressed(MouseButton::Left) {
            if active.before.parts != run.parts {
                state.record_external(active.before);
                run.request_save();
            }
        } else {
            if let (Some(r), Some(p)) = (ray, run.parts.get(active.part)) {
                let (c, d) = (r.origin, *r.direction);
                let mut edits = p.edits.clone();
                match (active.grip, active.ring.as_mut()) {
                    (Grip::Deform(id), _) => {
                        if let Some(w) = deform_weight(c, d, active.p0, active.p1, 0.0, 1.0) {
                            edits.set_weight(id, w);
                        }
                    }
                    (Grip::Ring(_), Some(rd)) => {
                        if let Some(target) = ring_angle(c, d, rd.centre, rd.axis, rd.press, rd.press_point, 0.0, rd.distance) {
                            let total = |a: f32| Quat::from_axis_angle(rd.axis_body, a) * rd.total;
                            let snapped = snap_angle(total, rd.axis_body, target).filter(|_| rd.snap_time <= SNAP_SECONDS);
                            let dt = time.delta_secs();
                            let goal = match snapped {
                                Some(a) => {
                                    rd.snap_time += dt;
                                    a
                                }
                                None => {
                                    if snap_angle(total, rd.axis_body, target).is_none() {
                                        rd.snap_time = 0.0;
                                    }
                                    target
                                }
                            };
                            let rate = if snapped.is_some() { APPROACH_RATE_SNAP } else { APPROACH_RATE };
                            rd.shown = approach(rd.shown, goal, rate, dt);
                            edits.set_ring(Quat::from_rotation_y(-rd.yaw) * total(rd.shown));
                        }
                    }
                    _ => {}
                }
                if edits != run.parts[active.part].edits {
                    run.parts[active.part].edits = edits;
                    sync_twins(&mut run.parts, active.part);
                    state.visuals_dirty = true;
                }
            }
            handles.active = Some(active);
        }
    }
    editor.manipulating |= handles.active.is_some();

    // Which part shows handles: the dragged one, else the hovered part, kept
    // while the cursor stays near one of its handles.
    let hide = !build || state.dragging() || editor.manipulating && handles.active.is_none();
    let over_ui = pointer.over_ui || cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_ui(c)));
    let screen = |p: Vec3| camera.world_to_viewport(cam, p).ok();
    let near_shown = cursor.is_some_and(|c| {
        handles.shown.iter().any(|s| {
            screen(s.at).is_some_and(|q| q.distance(c) < NEAR_PX)
                || s.ring.is_some_and(|(centre, axis, radius)| ring_screen_distance(&screen, centre, axis, radius, c) < NEAR_PX)
        })
    });
    let hovered_part = ray.filter(|_| !over_ui).and_then(|r| {
        run.parts
            .iter()
            .enumerate()
            .filter(|(i, p)| *i >= state.locked_parts && p.edits.limb.is_none())
            .filter_map(|(i, p)| {
                let t = world_transform_of(&pt, p, &originals, &run.body)?;
                let (along, d) = ray_point_distance(r.origin, *r.direction, t.translation);
                (along > 0.0 && d < 0.4).then_some((i, along))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    });
    let part = if hide {
        None
    } else if let Some(a) = &handles.active {
        Some(a.part)
    } else if let Some(f) = handles.forced {
        Some(f)
    } else if near_shown && handles.part.is_some() {
        handles.part
    } else {
        hovered_part
    };
    handles.part = part.filter(|i| *i < run.parts.len());
    handles.shown.clear();
    let models = cache.models(&originals, &mut meshes);
    if let Some(i) = handles.part {
        let p = run.parts[i].clone();
        // The drawn part's own transform (else the same placement computed).
        let drawn = accessories.iter().find(|(a, _)| a.part == i).map(|(_, t)| *t);
        if let (Some(t), Some(model), Some(info)) = (
            drawn.or_else(|| world_transform_of(&pt, &p, &originals, &run.body)),
            crate::editor::model_for_part(&originals, &p),
            cache.part(&originals, &p).cloned(),
        ) {
            for h in &info.handles {
                let p0 = t.transform_point(info.to_mesh(h.start));
                let p1 = t.transform_point(info.to_mesh(h.end));
                let w = p.edits.weight(h.id).unwrap_or(h.default);
                // The model's scale: the tuning scale times the caller's `SetScale` (1), in editor units.
                let scale = DEFORM_SCALE * CREATURE_HANDLE_UNIT * pt.scale.max_element();
                handles.shown.push(Shown { grip: Grip::Deform(h.id), at: p0.lerp(p1, w), ring: None, line: Some((p0, p1)), tf: deform_handle_transform(p0, p1, w, scale) });
            }
            let shown_mesh = posed_mesh(&mut cache, &originals, &p, &mut meshes).filter(|_| !p.edits.handles.is_empty()).unwrap_or_else(|| model.draw_mesh());
            let (centre_m, extent) = mesh_extent(&meshes, &shown_mesh);
            if handles.forced == Some(i) && time.elapsed_secs() < 30.0 {
                debug!("handles of part {i}: entity {:?}, mesh centre {centre_m} extent {extent}, handles {:?}", t, info.handles);
            }
            let centre = t.transform_point(centre_m);
            for axis in 0..3 {
                if !info.rings[axis] {
                    continue;
                }
                let a_m = info.basis * [Vec3::X, Vec3::Y, Vec3::Z][axis];
                let n = (t.rotation * (t.scale * a_m)).normalize_or(Vec3::Y);
                let radius = extent * RING_SCALE * t.scale.abs().max_element();
                let start = n.any_orthonormal_vector();
                // The ring model lies in its XZ plane around +Y: its outer radius is the ring radius.
                let model_radius = models.as_ref().map_or(1.0, |m| m.ring.radius_about_y().max(1e-3));
                let tf = Transform::from_translation(centre).with_rotation(Quat::from_rotation_arc(Vec3::Y, n)).with_scale(Vec3::splat(radius / model_radius));
                handles.shown.push(Shown { grip: Grip::Ring(axis), at: centre + start * radius, ring: Some((centre, n, radius)), line: None, tf });
            }
        }
    }
    // Hover: the nearest handle model the mouse ray meets (retail picks the
    // handle model groups at collision mode 4, the triangle mesh, 0x57B26x).
    handles.hover = handles.active.as_ref().map(|a| a.grip).or_else(|| {
        let r = ray.filter(|_| !over_ui)?;
        let models = models.as_ref()?;
        handles
            .shown
            .iter()
            .filter_map(|s| {
                let model = match s.grip {
                    Grip::Deform(_) => &models.teardrop,
                    Grip::Ring(_) => &models.ring,
                };
                model.pick(&s.tf, r.origin, *r.direction).map(|d| (s.grip, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    });

    // Visuals: the model pass and the overdraw pass of every shown handle (not in
    // apps without the overdraw material, e.g. headless tests).
    let Some(mut overdraw_materials) = overdraw_materials else { return };
    let wanted: Vec<Shown> = if models.is_some() { handles.shown.clone() } else { Vec::new() };
    let look = assets
        .get_or_insert_with(|| handle_look(default_handle_colors(), STATE_ALPHA, STATE_OVERDRAW_ALPHA, &mut materials, &mut overdraw_materials))
        .clone();
    let state_of = |g: Grip| -> usize {
        if handles.active.as_ref().is_some_and(|a| a.grip == g) {
            2
        } else if handles.hover == Some(g) {
            1
        } else {
            0
        }
    };
    let mut existing: HashMap<(u8, u32), Entity> = HashMap::new();
    for (e, v, mut tf, mat) in &mut visuals {
        let key = grip_key(v.0);
        match wanted.iter().find(|s| grip_key(s.grip) == key) {
            Some(s) if existing.insert(key, e).is_none() => {
                *tf = s.tf;
                let m = &look.main[state_of(s.grip)];
                if mat.0 != *m {
                    commands.entity(e).insert(MeshMaterial3d(m.clone()));
                }
            }
            _ => commands.entity(e).despawn(),
        }
    }
    let mut existing_over: HashMap<(u8, u32), Entity> = HashMap::new();
    for (e, v, mut tf, mat) in &mut overdraw_visuals {
        let key = grip_key(v.0);
        match wanted.iter().find(|s| grip_key(s.grip) == key) {
            Some(s) if existing_over.insert(key, e).is_none() => {
                *tf = s.tf;
                let m = &look.overdraw[state_of(s.grip)];
                if mat.0 != *m {
                    commands.entity(e).insert(MeshMaterial3d(m.clone()));
                }
            }
            _ => commands.entity(e).despawn(),
        }
    }
    let Some(models) = models else { return };
    for s in &wanted {
        let key = grip_key(s.grip);
        // `HandleTuning`: the deform handle has an overdraw model, the rotation ring
        // does not (`tuningEditorHandleHasOverdraw` 0).
        let (mesh, over) = match s.grip {
            Grip::Deform(_) => (models.teardrop.mesh.clone(), Some(models.teardrop_overdraw.mesh.clone())),
            Grip::Ring(_) => (models.ring.mesh.clone(), None),
        };
        if !existing.contains_key(&key) {
            commands.spawn((
                StageOwned,
                CreatureEditorOwned,
                HandleVisual(s.grip),
                Mesh3d(mesh),
                MeshMaterial3d(look.main[state_of(s.grip)].clone()),
                s.tf,
                bevy::light::NotShadowCaster,
            ));
        }
        if let (Some(mesh), false) = (over, existing_over.contains_key(&key)) {
            commands.spawn((
                StageOwned,
                CreatureEditorOwned,
                HandleVisual(s.grip),
                OverdrawPass,
                Mesh3d(mesh),
                MeshMaterial3d(look.overdraw[state_of(s.grip)].clone()),
                s.tf,
                bevy::light::NotShadowCaster,
            ));
        }
    }
}

fn grip_key(g: Grip) -> (u8, u32) {
    match g {
        Grip::Deform(id) => (0, id),
        Grip::Ring(a) => (1, a as u32),
    }
}

/// Screen distance from the cursor to a ring (sampled outline).
fn ring_screen_distance(screen: &impl Fn(Vec3) -> Option<Vec2>, centre: Vec3, axis: Vec3, radius: f32, c: Vec2) -> f32 {
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    (0..48)
        .filter_map(|i| {
            let a = i as f32 / 48.0 * std::f32::consts::TAU;
            screen(centre + (u * a.cos() + v * a.sin()) * radius)
        })
        .map(|q| q.distance(c))
        .fold(f32::MAX, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deform_weight_follows_the_retail_plane() {
        // Handle along +X from the origin, length 2; camera looking down -Z.
        let (p0, p1) = (Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0));
        let d = Vec3::NEG_Z;
        let w = deform_weight(Vec3::new(1.0, 0.3, 5.0), d, p0, p1, 0.0, 1.0).unwrap();
        assert!((w - 0.5).abs() < 1e-5);
        assert_eq!(deform_weight(Vec3::new(9.0, 0.0, 5.0), d, p0, p1, 0.0, 1.0), Some(1.0));
        assert_eq!(deform_weight(Vec3::new(-3.0, 0.0, 5.0), d, p0, p1, 0.0, 1.0), Some(0.0));
        // Behind the camera: ignored.
        assert_eq!(deform_weight(Vec3::new(1.0, 0.0, -5.0), d, p0, p1, 0.0, 1.0), None);
        // An oblique view still measures along the handle.
        let w = deform_weight(Vec3::new(1.5, 4.0, 4.0), Vec3::new(0.0, -1.0, -1.0).normalize(), p0, p1, 0.0, 1.0).unwrap();
        assert!((w - 0.75).abs() < 1e-4);
    }

    #[test]
    fn ring_angle_free_and_edge_on() {
        let centre = Vec3::ZERO;
        let axis = Vec3::Z;
        // Looking down the axis: a quarter turn of the hit is a quarter turn.
        let press = Vec3::X;
        let a = ring_angle(Vec3::new(0.0, 1.0, 5.0), Vec3::NEG_Z, centre, axis, press, press, 0.0, 5.0).unwrap();
        assert!((a - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let a = ring_angle(Vec3::new(0.0, -1.0, 5.0), Vec3::NEG_Z, centre, axis, press, press, 0.5, 5.0).unwrap();
        assert!((a - (0.5 - std::f32::consts::FRAC_PI_2)).abs() < 1e-4);
        // Edge-on (camera along +X looking at the ring's rim): tangent drag.
        let d = Vec3::NEG_X;
        let press_point = Vec3::new(0.0, 0.0, 0.0);
        let a = ring_angle(Vec3::new(5.0, 0.1, 0.0), d, centre, axis, press, press_point, 0.0, 5.0).unwrap();
        let tangent = axis.cross(d).normalize();
        assert!((a - EDGE_ON_GAIN * Vec3::new(0.0, 0.1, 0.0).dot(tangent) / 5.0).abs() < 1e-5);
    }

    #[test]
    fn ring_snaps_near_creature_axes() {
        // A part turned 0.1 rad about +Z: its X axis is within the zone of +X.
        let total = |a: f32| Quat::from_rotation_z(a);
        let s = snap_angle(total, Vec3::Z, 0.1).unwrap();
        assert!(s.abs() < 1e-4, "{s}");
        // 0.4 rad (1 - cos 0.4 = 0.079) is outside every zone.
        assert!(snap_angle(total, Vec3::Z, 0.4).is_none());
        // Near a quarter turn it snaps to the quarter turn.
        let s = snap_angle(total, Vec3::Z, 1.5).unwrap();
        assert!((s - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!((approach(0.0, 1.0, APPROACH_RATE, 0.5) - 1.0).abs() < 1e-6);
        assert!((approach(0.0, 1.0, APPROACH_RATE, 0.01) - 0.14).abs() < 1e-5);
        // The shorter way round the circle: from 3.0 towards -3.0 rad the angle keeps rising (through pi).
        let next = approach(3.0, -3.0, APPROACH_RATE, 0.01);
        assert!(next > 3.0 && next < std::f32::consts::PI, "{next}");
        assert!((angle_distance(3.0, -3.0) - (std::f32::consts::TAU - 6.0)).abs() < 1e-5);
    }

    #[test]
    fn twin_edits_mirror() {
        let mut e = PartEdits::default();
        assert!(e.is_default());
        e.set_ring(Quat::from_rotation_y(0.3) * Quat::from_rotation_x(0.2));
        e.set_weight(7, 0.25);
        e.on_limb = 5;
        let m = e.mirrored();
        assert_eq!(m.handles, vec![(7, 0.25)]);
        assert_eq!(m.on_limb, 5 | TWIN_BIT);
        assert_eq!(m.mirrored(), e);
        // Mx R Mx: a yaw reflects to the opposite yaw.
        let mut y = PartEdits::default();
        y.set_ring(Quat::from_rotation_y(0.4));
        assert!(y.mirrored().ring().angle_between(Quat::from_rotation_y(-0.4)) < 1e-5);
        // The reflected rotation maps the reflected vector.
        let mx = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        let v = Vec3::new(0.3, -0.5, 0.8);
        assert!((m.ring() * mx(v) - mx(e.ring() * v)).length() < 1e-5);
        // Twins follow.
        let mut parts = vec![PartInstance::new(1, 1), PartInstance::new(1, 2)];
        parts[0].pair_id = 3;
        parts[1].pair_id = 3;
        parts[0].edits = e.clone();
        sync_twins(&mut parts, 0);
        assert_eq!(parts[1].edits, m);
        // Saved compactly: default edits are not written.
        let json = serde_json::to_string(&PartInstance::new(1, 1)).unwrap();
        assert!(!json.contains("edits"), "{json}");
        let back: PartInstance = serde_json::from_str(&serde_json::to_string(&parts[0]).unwrap()).unwrap();
        assert_eq!(back, parts[0]);
    }

    #[test]
    fn deform_handle_model_points_along_the_line_and_keeps_the_up_axis() {
        // Rows (C, n, C x n) of FUN_004A8F00 with C = n x up: local Y along the line, local X horizontal, local Z up as far as it goes.
        let q = deform_handle_rotation(Vec3::X);
        assert!((q * Vec3::Y).distance(Vec3::X) < 1e-5);
        assert!((q * Vec3::X).distance(Vec3::Z) < 1e-5);
        assert!((q * Vec3::Z).distance(Vec3::Y) < 1e-5);
        let tilted = Vec3::new(1.0, 1.0, 0.0).normalize();
        let q = deform_handle_rotation(tilted);
        assert!((q * Vec3::Y).distance(tilted) < 1e-5);
        assert!((q * Vec3::X).y.abs() < 1e-5, "the local X axis stays horizontal");
        assert!((q * Vec3::Z).y > 0.0);
        // The degenerate rows when the line is vertical.
        let up = deform_handle_rotation(Vec3::Y);
        assert!((up * Vec3::Y).distance(Vec3::Y) < 1e-5 && (up * Vec3::X).distance(Vec3::NEG_X) < 1e-5);
        let down = deform_handle_rotation(Vec3::NEG_Y);
        assert!((down * Vec3::Y).distance(Vec3::NEG_Y) < 1e-5 && (down * Vec3::X).distance(Vec3::X) < 1e-5);
        // The handle sits on its line at the weight, scaled uniformly.
        let t = deform_handle_transform(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), 0.25, 1.3);
        assert!(t.translation.distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-6 && t.scale.distance(Vec3::splat(1.3)) < 1e-6);
    }

    #[test]
    fn handle_models_are_picked_by_their_triangles() {
        // A unit tetrahedron-like fin: one triangle in the XY plane.
        let raw = RawHandleModel {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
        };
        let mut meshes = Assets::<Mesh>::default();
        let model = HandleModel::from_raw(&raw, &mut meshes);
        let at = Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)).with_scale(Vec3::splat(2.0));
        // A ray down -Z through the triangle (scaled to the corner (2, 0) / (0, 2)).
        let hit = model.pick(&at, Vec3::new(0.4, 0.4, 10.0), Vec3::NEG_Z).unwrap();
        assert!((hit - 8.0).abs() < 1e-4, "{hit}");
        // Outside the triangle: a miss, even close to it (retail has no pick radius).
        assert!(model.pick(&at, Vec3::new(1.5, 1.5, 10.0), Vec3::NEG_Z).is_none());
        assert!(model.pick(&at, Vec3::new(-0.01, 0.4, 10.0), Vec3::NEG_Z).is_none());
        assert!((model.radius_about_y() - 1.0).abs() < 1e-6);
    }

    /// Installed data: the handle models of `HandleTuning` load; the teardrop and the ring have
    /// the extents their PROPs record (`modelBoundingBox`).
    #[test]
    fn installed_handle_models_load() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut meshes = Assets::<Mesh>::default();
        let models = load_handle_models(&index, &mut meshes).unwrap();
        for (name, m) in [("teardrop", &models.teardrop), ("teardrop_overdraw", &models.teardrop_overdraw), ("sphere", &models.sphere), ("sphere_overdraw", &models.sphere_overdraw), ("ring", &models.ring)] {
            println!("{name}: {} triangles, bbox {:?} .. {:?}, radius about Y {:.4}", m.tris.len(), m.min, m.max, m.radius_about_y());
        }
        // The teardrop's profile along its axis: where it is thin (the tip) and where it is wide (the bulb).
        let t = &models.teardrop;
        let (lo, hi) = (t.min.y, t.max.y);
        for k in 0..8 {
            let y0 = lo + (hi - lo) * k as f32 / 8.0;
            let y1 = lo + (hi - lo) * (k + 1) as f32 / 8.0;
            let r = t.tris.iter().flatten().filter(|v| v.y >= y0 && v.y <= y1).map(|v| Vec2::new(v.x, v.z).length()).fold(0.0f32, f32::max);
            println!("teardrop y {y0:.3}..{y1:.3}: radius {r:.4}");
        }
        assert!(models.teardrop.tris.len() > 20 && models.ring.tris.len() > 20 && models.sphere.tris.len() > 20);
        // `CE_basic_ui_teardrop` PROP: bbox (-0.0325, -0.1538, -0.0325) .. (0.0325, -0.0001, 0.0325).
        assert!((t.max.x - 0.0325).abs() < 0.01 && (t.min.y + 0.1538).abs() < 0.02 && t.max.y.abs() < 0.01, "{:?} {:?}", t.min, t.max);
        // `ui_rotateringcreature` PROP: bbox (-1.036, -0.074, -1.03) .. (1.036, 0.0, 1.03): a flat ring about Y.
        let r = &models.ring;
        assert!((r.radius_about_y() - 1.03).abs() < 0.08 && r.max.y.abs() < 0.01 && (r.min.y + 0.074).abs() < 0.02, "{:?} {:?}", r.min, r.max);
    }

    /// Installed data: eyes / mouths carry the DeformAxis handles of their RW4
    /// model and a Y ring; posing a handle moves vertices.
    #[test]
    fn installed_parts_have_deform_handles_and_rings() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let mut with_handles = 0;
        let mut with_rings = 0;
        let mut total = 0;
        let names = ["DeformAxisLeft", "DeformAxisForward", "DeformAxisUp"].map(crate::world_objects::name_id);
        let mut deform_axes = 0;
        let mut shown = 0;
        for def in catalog.parts.iter().filter(|p| p.key.group_id == CREATURE_PARTS && !p.is_null) {
            let h = load_part_handles(&index, def);
            total += 1;
            if !h.handles.is_empty() {
                with_handles += 1;
                if h.handles.iter().all(|x| names.contains(&x.id)) {
                    deform_axes += 1;
                    if shown < 4 {
                        shown += 1;
                        println!("{}: {:?} rings {:?}", def.name, h.handles.iter().map(|x| (x.id, x.start, x.end, x.default)).collect::<Vec<_>>(), h.rings);
                    }
                }
            }
            if h.rings.iter().any(|r| *r) {
                with_rings += 1;
            }
        }
        // Starter parts and the converted cell's parts.
        let mut probe: Vec<&crate::asset_catalog::PartDefinition> = crate::starter_parts::recipe(&catalog)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| catalog.by_part_in(CREATURE_PARTS, p))
            .collect();
        probe.extend(catalog.parts.iter().filter(|p| p.key.group_id == CREATURE_PARTS && (p.name == "Filter Mouth" || p.name == "Beady Eye")));
        for def in probe {
            let props = index.properties(prop_key(def.key.group_id, def.key.instance_id)).unwrap();
            let keys: Vec<u32> = key_list(&props, HANDLE_KEYS).iter().map(|k| k.instance_id).collect();
            let rw: Vec<u32> = index.read(def.model).ok().and_then(|b| crate::rw4::extract_morph_data(&b).ok()).map(|d| d.handles.iter().map(|h| h.id).collect()).unwrap_or_default();
            println!("probe {} {:08X}: keys {:08X?} rw4 {:08X?} -> {:?}", def.name, def.key.instance_id, keys, rw, load_part_handles(&index, def).rings);
        }
        println!("{total} parts: {with_handles} with deform handles ({deform_axes} DeformAxis only), {with_rings} with rings");
        assert!(deform_axes > 20);
        assert!(with_rings > total / 3);
    }
}
