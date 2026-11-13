//! The stacking rules of the rigid editors (building, vehicle and UFO): which
//! block a dragged part can rest on and how it settles on it. Retail code
//! (march2017 addresses, `notes/editors-stacking-notes.md`, `editors-snap2`):
//!
//! * `EditorRigblock::CanInteractWith` 0x438760: a part interacts with another
//!   block when its `modelTypesToInteractWith` list is empty or names the
//!   other block's `modelRigBlockType`, and `modelTypesNotToInteractWith` is
//!   empty or does not name it. The type comparison (`TypeMatches` 0x438900,
//!   list chosen by the attribute index 0x16 stay-above, 0x17 snap-down-to,
//!   0x1A / 0x1B / 0x35 align, 0x1C / 0x1D snap-to-parent, 0x2B snap-replace)
//!   matches the key's group exactly and treats an instance or type of 0 in
//!   the list entry as a wildcard.
//! * The pin ray (`FUN_004A5270`, called by the Pinning manipulator's mouse
//!   move 0x5B2E90 through 0x499910) excludes from the model-world ray cast
//!   the part itself, its symmetric twin, its children, vertebra / skin
//!   blocks and every block `CanInteractWith` refuses; the filter requires the
//!   `Rigblock` group and collision mode 2 for `PinningPhysics`, 4 (mesh
//!   triangles) for `PinningGeometry` (`mModelPinningTypeGroup` +0x3C0).
//! * `FUN_00493320` (`TrySnapToParent`, called through the combiner 0x490F20):
//!   in the parent's local frame the horizontal distance from the block's
//!   origin to the parent's centre (`GetCenter` 0x43D700) is compared with
//!   0.08 x the horizontal diagonal of the parent's world bounding box (x the
//!   caller's scale, 1.0). Under it, **when the part sits on top of the parent**
//!   (the manipulator passes its `onTop` flag as the `snapCentre` argument of
//!   0x490F20), and when the part has `modelSnapToParentCenter` and the parent's
//!   type is in its `modelSnapToParentTypes`, the block moves onto the centre
//!   line. With `modelSnapToParentSnapVectors` it also tries the parent's snap
//!   vectors: the table at +0x234 built by `FUN_0044CB60` at load time (one
//!   entry per `modelSnapAxes` angle / `modelNumberOfSnapAxes`: the outline
//!   point of the part's own mesh, found by a ray along the axis at
//!   `modelSnapAxisPickHeightPercent` of its height), the face whose angular
//!   sector holds the block is found by `FUN_0044DEB0` (tolerance 0.25 x the
//!   centre tolerance, measured along the wall) and `FUN_00492D80` slides the
//!   block along the wall to the face's centre point and `FUN_00490B90` turns
//!   its X axis along the wall.
//! * `FUN_00494760` (applies the orientation rules after the surface
//!   orientation): on top of a parent with `modelAllowTopBehaviors` the
//!   orientation comes from the parent's nearest face (`FUN_0049B960`) and the
//!   snap vector; `RemainUpright` / `RemainUprightOnTop` keep the part standing
//!   (row 1 flattened, row 2 = row 0 x row 1, else the global up); `PointForward`
//!   turns the part's X axis along the global forward axis (negated on the left
//!   half, `GetSide` 0x44F6F0); a symmetric editor puts a block on the plane
//!   of symmetry into the canonical orientation of `FUN_00494190`.
//!   `MoveBottomEdgeToSurface` (the dormer windows) pushes the part back along
//!   its mount axis until the midpoint of its bottom edge is on the parent.
//! * `FUN_00491FF0` (called by the combiner 0x490F20 with scale 1.2): `StayAbove` /
//!   `StayAboveGround` lift a part that sank below the ground or a listed block,
//!   `SnapDownTo` drops a part onto the listed types (and the ground) when it is
//!   within 0.8 x 1.2 of them.
//! * The Align rules (`FUN_00490950` collects the siblings of the listed types,
//!   `FUN_00490390` height, `FUN_0048F3E0` lateral, `FUN_0048EA40` XY and the centre
//!   of the editor) and the whole combiner `FUN_00490F20` (`align_height`,
//!   `align_lateral`, `align_xy`, `combine`): a window or door slides onto the centre
//!   line of the neighbours of its wall and takes their height, a structure or roof
//!   aligns with the others on the studio base and snaps to the editor's centre line.
//! * `FUN_00493AF0` (the bounds, `modelIsAllowedOutOfBounds`: the position stays inside
//!   a circle of radius `editorBoundSize / 2`) and `FUN_00436200` (a block must be
//!   attached to a root: it or an ancestor has `modelCanBeParentless`; a block that is
//!   not is a bad location, the release is refused).
//!
//! Frames: the retail rotation matrices are Z-up with +Y forward and their
//! rows are the world images of the part's own axes (`v x M`); the editor's
//! meshes carry the importer's basis (retail (x, y, z) -> (x, z, -y)), so the
//! code below converts at its borders (`to_retail`, `rows_of`) and reads like
//! the disassembly in between. A part's mount axis (the side that stands out of
//! the surface) is the retail local -Y (`FUN_0049B960` makes row 1 point down
//! on a top face), the editor's +Z.
//!
//! * Stored parent links (`EditorRigblock::mpParent` +0x33C / `mChildren` +0x340): every rigid block carries its id and
//!   its parent's id (`RigidBlock::id` / `parent`); a creation file's `childlist`s are the links (`maxis_creation`), a
//!   dragged block gets the block the pin ray hit as its parent (`SetParent` 0x4A2EB0 / 0x4987B0) and none when the ray
//!   hit no block, the Align siblings and the root check read them, and a moved or turned block takes its descendants
//!   along (`FUN_0049F4D0`, called by `SetTransform` 0x4A00E0).
//! * `FUN_0049BDC0` / `FUN_0049C1D0` / `FUN_0049C600` / `FUN_004AC170` (the Pinning manipulator, `0x5B2E90`, for a block without
//!   a parent): the block's origin follows the camera ray on a vertical plane facing the camera (through the block's own
//!   origin, or through the nearest other block's origin / half the editor's bounds toward the camera for a block that rests
//!   on the ground, or the plane of symmetry when looking along it) or, when that is higher than the plane point, at the
//!   height that stands its box (with its descendants) on the ground (`free_plane`, `free_position`).
//! * `FUN_0048D510` (the Havok shape cast of snap down / stay above): the part's shape is dropped from
//!   `|origin - bottom| + 2` above its position for 100 units; ported as a vertex / face sweep (`sweep_down`).
//!
//! ADAPTED (marked at each use): the snap vector table is built from the part's
//! current scaled mesh instead of once at load; the shape cast sweeps the part's render mesh
//! (vertex on face contacts; edge on edge contacts are not found) instead of Havok's collision shape;
//! `PinningPhysics` parts are picked against the mesh triangles like
//! `PinningGeometry` parts; retail applies the user's ring rotation after the
//! rules, the editor keeps one rotation; the `csnap` bone centre is the
//! bounding-box centre; the symmetric side of a block is the sign of its x; a design saved before the links were
//! stored gets them derived from the poses once (`legacy_links`); the Combiner and the centre-of-editor snap still run
//! for a parentless block (retail runs them in the Stacking manipulator, not decoded) and a block that leaves
//! the bounds loses its parent without the parentless pose being recomputed in that step.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use bevy::prelude::*;

use crate::{
    asset_catalog::ResourceIndex,
    cell_editor_data::{key_list, key_of, number, prop_key},
    creature_editor_edit::ray_hull,
    dbpf::ResourceKey,
    game::PartInstance,
    rigid_editor::Hull,
    rigid_editor_edit::{ray_block, Surface},
};

/// Part PROP keys (`reg_property.txt`; the type lists keep their name hash).
pub mod prop {
    pub const RIG_BLOCK_TYPE: u32 = 0x0186_609D;
    pub const REMAIN_UPRIGHT: u32 = 0x0201_636B;
    pub const SNAP_TO_PARENT_CENTER: u32 = 0x0213_A120;
    pub const TYPES_NOT_TO_INTERACT_WITH: u32 = 0x044D_CD0A;
    pub const NUMBER_OF_SNAP_AXES: u32 = 0x04EC_E315;
    pub const REMAIN_UPRIGHT_ON_TOP: u32 = 0x04ED_8506;
    pub const CIRCULAR_TOP_ALIGNMENT: u32 = 0x04ED_4C40;
    pub const SNAP_AXIS_PICK_HEIGHT_PERCENT: u32 = 0x04F7_AE2C;
    pub const SNAP_AXIS_ROTATION: u32 = 0x04F7_B20C;
    pub const SNAP_AXES: u32 = 0x04F7_DC1D;
    pub const ALLOW_TOP_BEHAVIORS: u32 = 0x05F9_8A72;
    pub const NO_FLIP_ON_PLANE: u32 = 0x05C0_8C60;
    pub const IS_ALLOWED_OUT_OF_BOUNDS: u32 = 0x0538_A895;
    pub const BOTTOM_EDGE_DISTANCE_ADJUSTMENT: u32 = 0x165F_0E1E;
    pub const POINT_FORWARD: u32 = 0x31B7_B15B;
    pub const MOVE_BOTTOM_EDGE_TO_SURFACE: u32 = 0x657A_035C;
    pub const TYPES_TO_INTERACT_WITH: u32 = 0x6FFD_ED92;
    pub const SNAP_DOWN_TO: u32 = 0x86E4_091F;
    pub const CAN_BE_PARENTLESS: u32 = 0x8E31_D974;
    pub const STAY_ABOVE_GROUND: u32 = 0xEC61_1FC7;
    pub const STAY_ABOVE: u32 = 0xEE0F_A2CE;
    pub const SNAP_TO_PARENT_TYPES: u32 = 0xE380_3D2A;
    pub const SNAP_TO_PARENT_SNAP_VECTORS: u32 = 0xF87A_BEA3;
    pub const ACT_LIKE_FIN_ON_PLANE: u32 = 0xFB23_B7D1;
    pub const ALIGN_LATERAL_WITH: u32 = 0x467E_F4E0;
    pub const ALIGN_HEIGHT_WITH: u32 = 0xA3DB_17BE;
    pub const ALIGN_XY_WITH: u32 = 0x05F5_7A28;
    pub const TYPES_TO_SNAP_REPLACE: u32 = 0x048E_2A2E;
    pub const SNAP_TO_CENTER_OF_EDITOR: u32 = 0xDAFB_E434;
    pub const IS_PLANT_ROOT: u32 = 0x0D88_00EB;
    pub const REPLACE_SNAP_DELTA: u32 = 0x0492_18C6;
}

/// The horizontal tolerance of the centre snap, as a fraction of the parent's
/// horizontal bounding-box diagonal (constant at 0x13EF530 in `FUN_00493320`).
pub const CENTER_SNAP_FRACTION: f32 = 0.08;
/// The snap vector pick uses a quarter of the centre tolerance (0x150C4EC).
pub const VECTOR_SNAP_FRACTION: f32 = 0.25;
/// `FUN_00494760`: the surface normal is the top face within this dot with
/// the up axis (0x13F7550) and the pin point within `TOP_FACE_HEIGHT` of the
/// parent's top (0x151294C), both read from the pin manipulator 0x5B2E90.
pub const TOP_FACE_NORMAL_DOT: f32 = 0.99;
pub const TOP_FACE_HEIGHT: f32 = 0.5;
/// `FUN_00490F20` passes 1.2 as the snap scale of `FUN_00491FF0`; a snap down
/// happens within 0.8 of it (0x13EF4CC).
pub const SNAP_DOWN_SCALE: f32 = 1.2;
pub const SNAP_DOWN_FRACTION: f32 = 0.8;

/// A rigblock type key (`building!roof.00000000`): the group names the model
/// class, the instance the part type, the type field the sub-type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeKey {
    pub group: u32,
    pub instance: u32,
    pub kind: u32,
}

impl From<ResourceKey> for TypeKey {
    fn from(k: ResourceKey) -> Self {
        Self { group: k.group_id, instance: k.instance_id, kind: k.type_id }
    }
}

/// `TypeMatches` 0x438900: some entry of `list` has the key's group and either
/// a 0 instance / type or the key's.
pub fn type_matches(key: TypeKey, list: &[TypeKey]) -> bool {
    list.iter().any(|e| e.group == key.group && (e.instance == 0 || e.instance == key.instance) && (e.kind == 0 || e.kind == key.kind))
}

/// The type of the studio base block (`0!ground`, the first entry of the
/// door's and stair's `modelSnapDownTo`).
pub fn ground_type() -> TypeKey {
    TypeKey { group: 0, instance: crate::world_objects::name_id("ground"), kind: 0 }
}

/// What a part's PROP (with its templates) says about stacking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackRules {
    /// `modelRigBlockType`.
    pub block_type: TypeKey,
    pub interact_with: Vec<TypeKey>,
    pub not_interact_with: Vec<TypeKey>,
    pub snap_to_parent: Vec<TypeKey>,
    pub snap_down_to: Vec<TypeKey>,
    /// `modelStayAbove` (attribute 0x16 is this list non-empty or `stay_above_ground`).
    pub stay_above: Vec<TypeKey>,
    pub stay_above_ground: bool,
    pub snap_to_parent_center: bool,
    pub snap_vectors: bool,
    pub snap_axes: i32,
    /// `modelSnapAxes`: the axis angles in degrees (overrides `snap_axes`).
    pub snap_axes_list: Vec<f32>,
    /// `modelSnapAxisRotation` (degrees).
    pub snap_axis_rotation: f32,
    /// `modelSnapAxisPickHeightPercent`.
    pub snap_axis_pick: Option<f32>,
    pub remain_upright: bool,
    pub remain_upright_on_top: bool,
    pub allow_top_behaviors: bool,
    pub circular_top: bool,
    pub point_forward: bool,
    pub act_like_fin: bool,
    pub no_flip_on_plane: bool,
    pub move_bottom_edge: bool,
    pub bottom_edge_adjustment: f32,
    /// `modelIsAllowedOutOfBounds` (attribute 0x33): without it the pin position is clamped into the
    /// editor's bounds (`FUN_00493AF0` called by `FUN_0049A7A0`).
    pub allowed_out_of_bounds: bool,
    /// `modelCanBeParentless` (attribute 0x23): the block may stand on the studio base without a
    /// parent; any other block must chain to one that may (`FUN_00436200`).
    pub can_be_parentless: bool,
    /// `modelAlignLateralWith` / `modelAlignHeightWith` / `modelAlignXYWith` (attributes 0x1A / 0x1B / 0x35
    /// are set by the parser when the list is not empty).
    pub align_lateral: Vec<TypeKey>,
    pub align_height: Vec<TypeKey>,
    pub align_xy: Vec<TypeKey>,
    /// `modelTypesToSnapReplace` (`TypeMatches` kind 0x2B).
    pub snap_replace: Vec<TypeKey>,
    /// `modelSnapToCenterOfEditor` (attribute 0x1E).
    pub snap_to_editor_centre: bool,
    /// `modelIsPlantRoot` (attribute 8, flora editor; scales the XY align tolerance by 10).
    pub plant_root: bool,
    /// `modelReplaceSnapDelta` (default 0.4, `EditorRigblock` +0x228).
    pub replace_snap_delta: f32,
}

fn float_list(p: &crate::prop::PropertyList, id: u32) -> Vec<f32> {
    p.0.get(&id)
        .map(|p| p.values.iter().filter(|v| v.len() >= 4).map(|v| f32::from_be_bytes(v[..4].try_into().unwrap())).collect())
        .unwrap_or_default()
}

impl StackRules {
    /// Read the rules of the part PROP `key` (group + instance), templates
    /// resolved by the index.
    pub fn load(index: &ResourceIndex, key: ResourceKey) -> Self {
        let Ok(p) = index.properties(prop_key(key.group_id, key.instance_id)) else {
            return Self::default();
        };
        let flag = |id: u32| number(&p, id).is_some_and(|v| v != 0.0);
        let list = |id: u32| key_list(&p, id).into_iter().map(TypeKey::from).collect::<Vec<_>>();
        Self {
            block_type: key_of(&p, prop::RIG_BLOCK_TYPE).map(TypeKey::from).unwrap_or_default(),
            interact_with: list(prop::TYPES_TO_INTERACT_WITH),
            not_interact_with: list(prop::TYPES_NOT_TO_INTERACT_WITH),
            snap_to_parent: list(prop::SNAP_TO_PARENT_TYPES),
            snap_down_to: list(prop::SNAP_DOWN_TO),
            stay_above: list(prop::STAY_ABOVE),
            stay_above_ground: flag(prop::STAY_ABOVE_GROUND),
            snap_to_parent_center: flag(prop::SNAP_TO_PARENT_CENTER),
            snap_vectors: flag(prop::SNAP_TO_PARENT_SNAP_VECTORS),
            snap_axes: number(&p, prop::NUMBER_OF_SNAP_AXES).map_or(0, |v| v.round() as i32),
            snap_axes_list: float_list(&p, prop::SNAP_AXES),
            snap_axis_rotation: number(&p, prop::SNAP_AXIS_ROTATION).unwrap_or(0.0),
            snap_axis_pick: number(&p, prop::SNAP_AXIS_PICK_HEIGHT_PERCENT),
            remain_upright: flag(prop::REMAIN_UPRIGHT),
            remain_upright_on_top: flag(prop::REMAIN_UPRIGHT_ON_TOP),
            allow_top_behaviors: flag(prop::ALLOW_TOP_BEHAVIORS),
            circular_top: flag(prop::CIRCULAR_TOP_ALIGNMENT),
            point_forward: flag(prop::POINT_FORWARD),
            act_like_fin: flag(prop::ACT_LIKE_FIN_ON_PLANE),
            no_flip_on_plane: flag(prop::NO_FLIP_ON_PLANE),
            move_bottom_edge: flag(prop::MOVE_BOTTOM_EDGE_TO_SURFACE),
            bottom_edge_adjustment: number(&p, prop::BOTTOM_EDGE_DISTANCE_ADJUSTMENT).unwrap_or(0.0),
            allowed_out_of_bounds: flag(prop::IS_ALLOWED_OUT_OF_BOUNDS),
            can_be_parentless: flag(prop::CAN_BE_PARENTLESS),
            align_lateral: list(prop::ALIGN_LATERAL_WITH),
            align_height: list(prop::ALIGN_HEIGHT_WITH),
            align_xy: list(prop::ALIGN_XY_WITH),
            snap_replace: list(prop::TYPES_TO_SNAP_REPLACE),
            snap_to_editor_centre: flag(prop::SNAP_TO_CENTER_OF_EDITOR),
            plant_root: flag(prop::IS_PLANT_ROOT),
            replace_snap_delta: number(&p, prop::REPLACE_SNAP_DELTA).unwrap_or(0.4),
        }
    }

    /// `EditorRigblock::CanInteractWith` 0x438760 (this = the moving part).
    pub fn can_interact_with(&self, other: &StackRules) -> bool {
        let wanted = self.interact_with.is_empty() || type_matches(other.block_type, &self.interact_with);
        let refused = !self.not_interact_with.is_empty() && type_matches(other.block_type, &self.not_interact_with);
        wanted && !refused
    }

    /// `modelSnapToParentCenter` applies to `parent` (`FUN_00493320`: the
    /// attribute and `TypeMatches(parent, 0x1C)`; an empty list never matches).
    pub fn snaps_to_center_of(&self, parent: &StackRules) -> bool {
        self.snap_to_parent_center && type_matches(parent.block_type, &self.snap_to_parent)
    }

    /// `modelSnapToParentSnapVectors` applies to `parent` (`TypeMatches(parent, 0x1D)`
    /// reads the same `modelSnapToParentTypes` list).
    pub fn snaps_to_vectors_of(&self, parent: &StackRules) -> bool {
        self.snap_vectors && type_matches(parent.block_type, &self.snap_to_parent)
    }

    /// `FUN_00494760` ATTR 0x11 / 0x32 selection: on the top face of a parent
    /// with `modelAllowTopBehaviors` the on-top flag replaces the plain one.
    pub fn stays_upright(&self, parent: Option<&StackRules>, on_top: bool) -> bool {
        let top = on_top && parent.is_some_and(|p| p.allow_top_behaviors);
        if top {
            self.remain_upright_on_top
        } else {
            self.remain_upright
        }
    }

    /// Attribute 0x16 (`HasStayAbove`): `modelStayAboveGround` or a `modelStayAbove` list.
    pub fn has_stay_above(&self) -> bool {
        self.stay_above_ground || !self.stay_above.is_empty()
    }

    /// Attribute 0x17 (`HasSnapDownTo`): a `modelSnapDownTo` list.
    pub fn has_snap_down(&self) -> bool {
        !self.snap_down_to.is_empty()
    }

    /// The number of snap vectors the part builds (`FUN_0044CB60`: the
    /// `modelSnapAxes` list when present, else `modelNumberOfSnapAxes`).
    pub fn snap_axis_count(&self) -> usize {
        if self.snap_axes_list.is_empty() {
            self.snap_axes.max(0) as usize
        } else {
            self.snap_axes_list.len()
        }
    }
}

/// The ray filter of the pin cast: a block can be hit unless the moving part
/// cannot interact with it (`FUN_004A5270`). A block without rules (a part
/// that is not an original definition) interacts with everything.
pub fn pin_allows(moving: Option<&StackRules>, other: Option<&StackRules>) -> bool {
    match (moving, other) {
        (Some(m), Some(o)) => m.can_interact_with(o),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// The retail frame
// ---------------------------------------------------------------------------

/// A retail orientation matrix: row i is the world image of the part's own
/// axis i (`v x M`), Z up, +Y forward.
pub type Rows = [Vec3; 3];

pub const IDENTITY_ROWS: Rows = [Vec3::X, Vec3::Y, Vec3::Z];
/// The global up (0x15D6324) and forward (0x15D63F4) axes, and the lateral
/// axis (0x15D6370).
pub const UP: Vec3 = Vec3::Z;
pub const FORWARD: Vec3 = Vec3::Y;
pub const LATERAL: Vec3 = Vec3::X;

/// An editor-frame vector in retail axes: the importer maps retail (x, y, z) to (x, z, -y).
pub fn to_retail(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// A retail vector in the editor's axes.
pub fn from_retail(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

/// The retail rows of an editor rotation: the part's retail X axis is the
/// editor x, Y the editor -z, Z the editor y.
pub fn rows_of(q: Quat) -> Rows {
    [to_retail(q * Vec3::X), to_retail(q * Vec3::NEG_Z), to_retail(q * Vec3::Y)]
}

/// The editor rotation of retail rows (orthonormalised keeping rows 0 and 1).
pub fn rotation_of(rows: Rows) -> Quat {
    let r0 = rows[0].normalize_or(Vec3::X);
    let r1 = (rows[1] - r0 * r0.dot(rows[1])).normalize_or(r0.any_orthonormal_vector());
    let r2 = r0.cross(r1);
    Quat::from_mat3(&Mat3::from_cols(from_retail(r0), from_retail(r2), -from_retail(r1))).normalize()
}

/// `FUN_0044A0F0`: normalise with a 1e-8 epsilon under the root (a zero vector stays zero).
fn normalize_eps(v: Vec3) -> Vec3 {
    v / (v.length_squared() + 1e-8).sqrt()
}

/// `FUN_004371A0`: plain normalisation (a zero vector gives NaN).
fn normalize_strict(v: Vec3) -> Vec3 {
    v / v.length()
}

fn scale_rows(a: Rows, b: Rows) -> Rows {
    // `FUN_0041DDE0`: matrix product, row i of the result = sum_j a[i][j] * b.row_j.
    [0, 1, 2].map(|i| b[0] * a[i].x + b[1] * a[i].y + b[2] * a[i].z)
}

// ---------------------------------------------------------------------------
// Orientation rules (FUN_00494760)
// ---------------------------------------------------------------------------

/// Everything `FUN_00494760` reads besides the orientation itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct OrientPlan {
    /// The block has a parent (every rule needs one).
    pub has_parent: bool,
    /// On top of a parent with `modelAllowTopBehaviors` (the `onTop` argument after its
    /// 0x37 test, attribute 0x37 of the parent).
    pub on_top: bool,
    /// `FUN_0049B960`'s orientation, when the part is on top and not centre-snapped.
    pub top_rows: Option<Rows>,
    /// The snap vector the block snapped to (`snapType` >= 0): its world direction and
    /// the parent's up axis, for `FUN_00490B90`.
    pub snap_align: Option<(Vec3, Vec3)>,
    /// The selected `RemainUpright` / `RemainUprightOnTop` flag.
    pub remain_upright: bool,
    pub point_forward: bool,
    pub act_like_fin: bool,
    pub no_flip: bool,
    /// `modelOrientToSurfaces` (attribute 0).
    pub orient_to_surface: bool,
    /// The editor mirrors parts across x = 0 (the model's flag 0x4AE160).
    pub symmetric: bool,
    /// `GetSide` 0x44F6F0: -1 left, 0 on the plane, 1 right.
    pub side: i32,
    /// The pin surface's normal (retail axes), `mSurfaceNormal` +0x138.
    pub surface_normal: Vec3,
    /// The block's stored base orientation (+0xA8): a part without OrientToSurfaces ends with it
    /// whatever the other rules did (0x495ACB).
    pub base: Option<Rows>,
}

/// `FUN_00494760` RemainUpright (0x494BAF..0x494E14): the part keeps its right
/// axis (row 0), its forward axis (row 1) becomes horizontal and row 2 is
/// `row0 x row1`; when that is degenerate (normalised length under 0.9, or NaN)
/// the up axis and `up x row0` are used.
pub fn remain_upright_rows(rows: Rows) -> Rows {
    let mut r1 = rows[1];
    r1.z = 0.0;
    // ADAPTED: a vertical row 1 within float noise (the surface rotation of a top face leaves 1e-8 of it
    // in x / y, which retail's epsilon normalisation turns into a coin toss between the two branches)
    // is treated as the zero vector it stands for.
    let vertical = r1.length_squared() < 1e-6;
    let r1 = normalize_eps(r1);
    let r2 = normalize_eps(rows[0].cross(r1));
    if vertical || r2.length_squared() < 0.9 || !r2.is_finite() {
        return [rows[0], normalize_eps(UP.cross(rows[0])), UP];
    }
    // ADAPTED: retail leaves row 0 as it is (not exactly perpendicular to the flattened row 1
    // when the part is rolled); the matrix is orthonormalised here for the quaternion.
    let r0 = r1.cross(r2).normalize_or(rows[0]);
    [r0, r1, r2]
}

/// `FUN_00494760` PointForward (0x494EF9..0x49523B, the side is not 0 or the
/// editor is not symmetric): row 0 becomes the forward axis (negated for the left
/// half), row 1 the old row 1 without its y part, row 2 = row 0 x row 1 (else up).
pub fn point_forward_rows(rows: Rows, side: i32) -> Rows {
    let v = if side == -1 { -FORWARD } else { FORWARD };
    let mut r1 = rows[1];
    r1.y = 0.0;
    let along = r1.length_squared() < 1e-6; // ADAPTED, as in `remain_upright_rows`
    let r1 = normalize_strict(r1);
    let r2 = normalize_eps(v.cross(r1));
    if along || r2.length_squared() < 0.9 || !r2.is_finite() {
        return [v, normalize_eps(UP.cross(v)), UP];
    }
    [v, r1, r2]
}

/// `FUN_00494190`: the orientation of a block on the plane of symmetry of a
/// symmetric editor.
pub fn plane_orientation(rows: Rows, plan: &OrientPlan) -> Rows {
    let pf = plan.point_forward || plan.act_like_fin;
    if plan.remain_upright && pf {
        return [-FORWARD, LATERAL, UP];
    }
    if plan.remain_upright {
        return IDENTITY_ROWS;
    }
    let mut r1 = rows[1];
    r1.x = 0.0;
    if r1.length() == 0.0 {
        r1 = -UP;
    }
    let r1 = normalize_strict(r1);
    let (r0, r2) = if pf { (normalize_strict(r1.cross(LATERAL)), LATERAL) } else { (LATERAL, normalize_strict(LATERAL.cross(r1))) };
    let mut out = [r0, r1, r2];
    if !pf && !plan.no_flip && plan.surface_normal.y > 0.0 {
        // 0x44C5B0 with neither flag: rows 0 and 2 are negated.
        out[0] = -out[0];
        out[2] = -out[2];
    }
    out
}

/// `FUN_00490B90`: the block is turned so that its X axis runs along the wall
/// of the snap vector (`dir` x `up`) and its other axes stay in the plane
/// perpendicular to the wall.
pub fn align_to_snap_vector(rows: Rows, dir: Vec3, up: Vec3) -> Rows {
    let n = normalize_strict(dir.cross(up));
    if !n.is_finite() {
        return rows;
    }
    if rows[0].dot(n).abs() <= 1.525_878_9e-5 {
        // Row 0 is already perpendicular to the wall's tangent: turn the matrix about its row 1
        // by the angle between row 0 and the tangent's part perpendicular to row 1 (retail turns by
        // the unsigned angle; the sense of its axis-angle routine 0x6BAB50 / 0x453FF0 is the
        // right-handed one about the axis, `editors-snap3`).
        let m = normalize_strict(n - rows[1] * n.dot(rows[1]));
        let angle = m.dot(rows[0]).clamp(-1.0, 1.0).acos();
        if !angle.is_finite() {
            return rows;
        }
        let q = Quat::from_axis_angle(rows[1], angle);
        return [q * rows[0], rows[1], q * rows[2]];
    }
    let r1 = normalize_strict(rows[1] - n * rows[1].dot(n));
    let r2 = normalize_strict(rows[2] - n * rows[2].dot(n));
    let r0 = normalize_strict(r1.cross(r2));
    if !(r0.is_finite() && r1.is_finite() && r2.is_finite()) {
        return rows;
    }
    [r0, r1, r2]
}

/// The result of the orientation rules.
#[derive(Clone, Copy, Debug)]
pub struct Oriented {
    pub rows: Rows,
    /// Retail's return value: some rule changed the orientation.
    pub modified: bool,
}

/// `FUN_00494760` on the retail rows `input` (the surface orientation).
pub fn apply_orientation_rules(input: Rows, plan: &OrientPlan) -> Oriented {
    let mut rows = input;
    let mut modified = false;
    if !plan.has_parent {
        return Oriented { rows, modified };
    }
    if plan.on_top {
        if let Some(top) = plan.top_rows {
            rows = top;
            if let Some((dir, up)) = plan.snap_align {
                rows = align_to_snap_vector(rows, dir, up);
            }
            modified = true;
        }
    }
    let on_plane = plan.symmetric && plan.side == 0;
    if plan.remain_upright {
        rows = if on_plane { plane_orientation(rows, plan) } else { remain_upright_rows(rows) };
        modified = true;
    }
    if plan.point_forward {
        rows = if on_plane { plane_orientation(rows, plan) } else { point_forward_rows(rows, plan.side) };
        modified = true;
    }
    // 0x495A0C: a block on the plane of a symmetric editor that no rule touched.
    if on_plane && !modified && plan.orient_to_surface {
        rows = plane_orientation(rows, plan);
        modified = true;
    }
    // 0x495ACB: no OrientToSurfaces and a parent: the stored base orientation, whatever happened above
    // (a roof on top of a structure keeps its own orientation, only the parts that orient are turned).
    if !plan.orient_to_surface {
        if let Some(base) = plan.base {
            rows = base;
            modified = true;
        }
    }
    Oriented { rows, modified }
}

// ---------------------------------------------------------------------------
// Snap vectors (FUN_0044CB60 / FUN_0044DEB0 / FUN_00492D80)
// ---------------------------------------------------------------------------

/// One entry of the snap vector table (+0x234, 32 bytes): the axis direction
/// (retail local XY plane), the outline point of the part on it (z = 0) and
/// the cardinal flag (+0x1C).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapVector {
    pub dir: Vec3,
    pub point: Vec3,
    pub cardinal: bool,
}

/// The axis angles of a part (`FUN_0044CB60`): the `modelSnapAxes` list in
/// degrees, else `modelNumberOfSnapAxes` equal steps from `modelSnapAxisRotation`.
/// The flag marks the exact quarter turns.
pub fn snap_axis_angles(rules: &StackRules) -> Vec<(f32, bool)> {
    use std::f32::consts::{FRAC_PI_2, TAU};
    if !rules.snap_axes_list.is_empty() {
        return rules.snap_axes_list.iter().map(|a| (a / 360.0 * TAU, a % 90.0 == 0.0)).collect();
    }
    let n = rules.snap_axes.max(0) as usize;
    if n == 0 {
        return Vec::new();
    }
    let step = TAU / n as f32;
    let mut angle = rules.snap_axis_rotation / 360.0 * TAU;
    (0..n)
        .map(|_| {
            let out = (angle, (angle % FRAC_PI_2).abs() <= f32::MIN_POSITIVE);
            angle += step;
            out
        })
        .collect()
}

/// `FUN_0044CB60`: the table of a part from its scaled mesh in the retail local
/// frame (`tris`, bounding box `min` / `max`). Each axis is cast from 100 units
/// out at the pick height towards the centre; one miss empties the table.
pub fn build_snap_vectors(rules: &StackRules, tris: &[[Vec3; 3]], min: Vec3, max: Vec3) -> Vec<SnapVector> {
    let axes = snap_axis_angles(rules);
    if axes.is_empty() {
        return Vec::new();
    }
    let center = (min + max) * 0.5;
    let pick_z = match rules.snap_axis_pick {
        Some(p) => min.z + (max.z - min.z) * p.clamp(0.0, 1.0),
        None => center.z,
    };
    let mut out = Vec::with_capacity(axes.len());
    for (angle, cardinal) in axes {
        let dir = normalize_strict(Vec3::new(angle.cos(), angle.sin(), 0.0));
        let origin = Vec3::new(center.x + 100.0 * dir.x, center.y + 100.0 * dir.y, pick_z);
        let Some((_, hit, _)) = ray_hull(tris, origin, -dir) else {
            return Vec::new();
        };
        let mut p = Vec3::new(hit.x, hit.y, 0.0);
        if cardinal {
            if p.x.abs() > 0.5 {
                p.y = center.y;
            } else if p.y.abs() > 0.5 {
                p.x = center.x;
            }
        }
        out.push(SnapVector { dir, point: p, cardinal });
    }
    out
}

/// `FUN_0044EB10`: the parameter along `dir` from `origin` to the plane (`normal`, `d`), for a hit ahead.
fn ray_plane(origin: Vec3, dir: Vec3, normal: Vec3, d: f32) -> Option<f32> {
    let denom = dir.dot(normal);
    if denom == 0.0 {
        return None;
    }
    let s = -(normal.dot(origin) + d) / denom;
    (s >= 0.0).then_some(s)
}

/// The polar angle (0..2 pi) of a flat direction, as `FUN_0044DEB0` reads it (acos of the x part,
/// mirrored for a negative y).
fn polar(v: Vec3) -> f32 {
    let n = normalize_strict(Vec3::new(v.x, v.y, 0.0));
    if !n.is_finite() {
        return 0.0;
    }
    let a = n.x.clamp(-1.0, 1.0).acos();
    if n.y < 0.0 {
        std::f32::consts::TAU - a
    } else {
        a
    }
}

/// `FUN_0044DEB0(P, tolerance)`: the face of the table whose sector (from its corner with the
/// previous wall to its corner with the next wall, around the centre) holds `q` (the block's
/// position in the parent's local frame), the one whose outline point is nearest along the
/// wall. With a tolerance above 0 the distance along the wall must be under it.
pub fn find_snap_vector(table: &[SnapVector], center: Vec3, q: Vec3, tolerance: f32) -> Option<usize> {
    use std::f32::consts::TAU;
    let n = table.len();
    if n == 0 {
        return None;
    }
    let flat_center = Vec3::new(center.x, center.y, 0.0);
    let angle0 = polar(q - center);
    let (mut best, mut best_dist) = (None, f32::MAX);
    for (i, e) in table.iter().enumerate() {
        let tangent = e.dir.cross(UP);
        let dist = (q - e.point).dot(tangent).abs();
        let (prev, next) = (&table[(i + n - 1) % n], &table[(i + 1) % n]);
        let plane = |v: &SnapVector| (v.dir, -v.point.dot(v.dir));
        let (pn, pd) = plane(prev);
        let (nn, nd) = plane(next);
        let corner_prev = ray_plane(e.point, tangent, pn, pd).map(|s| e.point + tangent * s);
        let corner_next = ray_plane(e.point, -tangent, nn, nd).map(|s| e.point - tangent * s);
        let (Some(c1), Some(c2)) = (corner_prev, corner_next) else {
            continue;
        };
        let a1 = polar(c1 - flat_center);
        let a2 = polar(c2 - flat_center);
        let mut rel = angle0 - a1;
        let mut span = a2 - a1;
        if rel < 0.0 {
            rel += TAU;
        }
        if span < 0.0 {
            span += TAU;
        }
        if rel >= 0.0 && span > rel && dist < best_dist {
            best_dist = dist;
            best = Some(i);
        }
    }
    if tolerance > 0.0 {
        return (tolerance > best_dist).then_some(best).flatten();
    }
    best
}

/// `FUN_00490E80`: `a` without its part along `b`.
fn reject(a: Vec3, b: Vec3) -> Vec3 {
    a - b * a.dot(b)
}

/// `FUN_00492D80` position: the block's local position slides along the wall of face `idx` to the
/// face's outline point (its distance from the wall and its height stay).
pub fn snap_vector_position(table: &[SnapVector], idx: usize, q: Vec3) -> Vec3 {
    let e = &table[idx];
    let tangent = e.dir.cross(UP);
    let mut w = reject(q - e.point, tangent) + e.point;
    w.z = q.z;
    w
}

/// The parent as the snap code sees it: its pose in the retail frame, the centre of its
/// scaled box, the snap vector table and the horizontal diagonal of its world box.
#[derive(Clone, Debug)]
pub struct SnapTarget {
    /// Editor world position of the parent's origin.
    pub position: Vec3,
    pub rows: Rows,
    /// `GetCenter(parent, local)`: the centre of the scaled box in the parent's local retail frame.
    pub center: Vec3,
    pub table: Vec<SnapVector>,
    /// Horizontal diagonal of the parent's world bounding box (`FUN_0044B2D0` mode 0).
    pub diagonal: f32,
}

/// The eight corners of a hull's box.
fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [0, 1, 2, 3, 4, 5, 6, 7].map(|i| Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z }))
}

/// The world box (min, max) of a hull under a pose (editor frame).
pub fn world_box(pose: &Transform, hull: &Hull) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for c in box_corners(hull.min, hull.max) {
        let w = pose.transform_point(c);
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo, hi)
}

impl SnapTarget {
    /// Build the target of a parent block. ADAPTED: the table is rebuilt from the parent's current
    /// (scaled, stretched) mesh; retail builds it once at load at scale 1.
    pub fn of(rules: &StackRules, pose: &Transform, hull: &Hull) -> Self {
        let scaled = |v: Vec3| to_retail(pose.scale * v);
        let corners = box_corners(hull.min, hull.max).map(scaled);
        let min = corners.iter().fold(Vec3::splat(f32::INFINITY), |a, c| a.min(*c));
        let max = corners.iter().fold(Vec3::splat(f32::NEG_INFINITY), |a, c| a.max(*c));
        let table = if rules.snap_axis_count() > 0 {
            let tris: Vec<[Vec3; 3]> = hull.tris.iter().map(|t| t.map(scaled)).collect();
            build_snap_vectors(rules, &tris, min, max)
        } else {
            Vec::new()
        };
        let (lo, hi) = world_box(pose, hull);
        Self {
            position: pose.translation,
            rows: rows_of(pose.rotation),
            center: (min + max) * 0.5,
            table,
            diagonal: Vec2::new(hi.x - lo.x, hi.z - lo.z).length(),
        }
    }

    /// An editor world point in the parent's local retail frame (`(P - pos) x M^-1`).
    pub fn local(&self, p: Vec3) -> Vec3 {
        let d = to_retail(p - self.position);
        Vec3::new(d.dot(self.rows[0]), d.dot(self.rows[1]), d.dot(self.rows[2]))
    }

    /// A point of the parent's local retail frame in the editor's world.
    pub fn world(&self, q: Vec3) -> Vec3 {
        self.position + from_retail(self.rows[0] * q.x + self.rows[1] * q.y + self.rows[2] * q.z)
    }

    /// The tolerance of the centre snap: 0.08 of the horizontal diagonal x `scale`.
    pub fn tolerance(&self, scale: f32) -> f32 {
        self.diagonal * CENTER_SNAP_FRACTION * scale
    }

    /// World (retail axes) direction of the snap vector `idx`.
    pub fn vector_direction(&self, idx: usize) -> Vec3 {
        let d = self.table[idx].dir;
        self.rows[0] * d.x + self.rows[1] * d.y + self.rows[2] * d.z
    }

    /// `FUN_0049B960`: the orientation of a block on top of the parent at editor point `p`: the
    /// part's X along the tangent of the parent's nearest face (or towards the centre when
    /// `modelCircularTopAlignment`), its Y down, its Z along the face's inward direction.
    pub fn top_rows(&self, p: Vec3, circular: bool) -> Rows {
        let q = self.local(p);
        let l = q - self.center;
        let d = if circular {
            normalize_strict(Vec3::new(-l.x, -l.y, 0.0))
        } else if self.table.is_empty() {
            return IDENTITY_ROWS;
        } else {
            let dir = find_snap_vector(&self.table, self.center, q, -1.0).map_or(Vec3::ZERO, |i| self.table[i].dir);
            -dir
        };
        scale_rows([(-UP).cross(d), -UP, d], self.rows)
    }
}

/// What `FUN_00493320` decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentSnap {
    /// The new block position (editor world).
    pub position: Vec3,
    /// Retail's return value: how far the block moved.
    pub distance: f32,
    /// The snap vector (None = the centre).
    pub vector: Option<usize>,
}

/// `FUN_00493320` (`TrySnapToParent`): the centre snap (only with `snap_centre`, the on-top flag
/// of the manipulator) wins over the vector snap. `position` is the block's origin (editor world).
pub fn try_snap_to_parent(moving: &StackRules, parent: &StackRules, target: &SnapTarget, position: Vec3, scale: f32, snap_centre: bool) -> Option<ParentSnap> {
    let q = target.local(position);
    let tol = target.tolerance(scale);
    let dist = Vec2::new(q.x - target.center.x, q.y - target.center.y).length();
    if snap_centre && tol > dist && moving.snaps_to_center_of(parent) {
        let snapped = target.world(Vec3::new(target.center.x, target.center.y, q.z));
        return Some(ParentSnap { position: snapped, distance: dist, vector: None });
    }
    if !target.table.is_empty() && moving.snaps_to_vectors_of(parent) {
        let idx = find_snap_vector(&target.table, target.center, q, tol * VECTOR_SNAP_FRACTION)?;
        let snapped = target.world(snap_vector_position(&target.table, idx, q));
        return Some(ParentSnap { position: snapped, distance: (position - snapped).length(), vector: Some(idx) });
    }
    None
}

/// `FUN_00493320`'s centre snap alone (kept for the part tests): the block's origin `position`
/// moves onto the parent's centre line when its horizontal distance is under `CENTER_SNAP_FRACTION`
/// of the parent's horizontal bounding-box diagonal x `scale`. Returns the new origin and the
/// distance, `None` when the block is too far.
pub fn snap_to_center(position: Vec3, parent: &Transform, hull: &Hull, scale: f32) -> Option<(Vec3, f32)> {
    let target = SnapTarget::of(&StackRules::default(), parent, hull);
    let q = target.local(position);
    let dist = Vec2::new(q.x - target.center.x, q.y - target.center.y).length();
    (target.tolerance(scale) > dist).then(|| (target.world(Vec3::new(target.center.x, target.center.y, q.z)), dist))
}

// ---------------------------------------------------------------------------
// Snap down / stay above (FUN_00491FF0)
// ---------------------------------------------------------------------------

/// Another block as the height rules see it.
#[derive(Clone)]
pub struct OtherBlock {
    /// Its stored parent (`mpParent`, [`parent_table`]; the siblings of the moving block share its parent,
    /// `None` = the studio base).
    pub parent: Option<usize>,
    pub pose: Transform,
    pub hull: Arc<Hull>,
    pub rules: Option<Arc<StackRules>>,
}

impl OtherBlock {
    fn block_type(&self) -> TypeKey {
        self.rules.as_ref().map(|r| r.block_type).unwrap_or_default()
    }
}

/// A vertical sweep's length (the cast of `FUN_0048D510` runs 100 units down: `0x13ec4b0`).
pub const SWEEP_LENGTH: f32 = 100.0;

/// A block's triangles in the XZ plane, bucketed in a grid: the vertical queries of the sweep ask for the
/// surfaces above / below a point.
struct PlanGrid {
    lo: Vec2,
    inv: Vec2,
    n: usize,
    cells: Vec<Vec<u32>>,
    tris: Vec<[Vec3; 3]>,
}

impl PlanGrid {
    fn new(tris: Vec<[Vec3; 3]>) -> Self {
        let plan = |v: &Vec3| Vec2::new(v.x, v.z);
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for v in tris.iter().flatten() {
            lo = lo.min(plan(v));
            hi = hi.max(plan(v));
        }
        let n = ((tris.len() as f32).sqrt() as usize).clamp(1, 48);
        let size = (hi - lo).max(Vec2::splat(1e-6));
        let inv = Vec2::splat(n as f32) / size;
        let mut grid = Self { lo, inv, n, cells: vec![Vec::new(); n * n], tris: Vec::new() };
        for (k, t) in tris.iter().enumerate() {
            let tlo = t.iter().fold(Vec2::splat(f32::INFINITY), |m, v| m.min(plan(v)));
            let thi = t.iter().fold(Vec2::splat(f32::NEG_INFINITY), |m, v| m.max(plan(v)));
            let (a, b) = (grid.cell(tlo), grid.cell(thi));
            for cz in a.1..=b.1 {
                for cx in a.0..=b.0 {
                    grid.cells[cz * n + cx].push(k as u32);
                }
            }
        }
        grid.tris = tris;
        grid
    }

    fn cell(&self, p: Vec2) -> (usize, usize) {
        let c = ((p - self.lo) * self.inv).floor();
        let at = |v: f32| (v.max(0.0) as usize).min(self.n - 1);
        (at(c.x), at(c.y))
    }

    /// The heights at which the vertical line through (x, z) meets the triangles (touching edges count).
    fn heights(&self, x: f32, z: f32, mut each: impl FnMut(f32)) {
        if self.tris.is_empty() || !x.is_finite() || !z.is_finite() {
            return;
        }
        let (cx, cz) = self.cell(Vec2::new(x, z));
        for &k in &self.cells[cz * self.n + cx] {
            let [a, b, c] = self.tris[k as usize];
            let det = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if det.abs() < 1e-12 {
                continue;
            }
            let u = ((x - a.x) * (c.z - a.z) - (c.x - a.x) * (z - a.z)) / det;
            let v = ((b.x - a.x) * (z - a.z) - (x - a.x) * (b.z - a.z)) / det;
            if u >= -1e-5 && v >= -1e-5 && u + v <= 1.0 + 1e-5 {
                each(a.y + u * (b.y - a.y) + v * (c.y - a.y));
            }
        }
    }
}

fn unique_vertices(tris: &[[Vec3; 3]]) -> Vec<Vec3> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in tris.iter().flatten() {
        if seen.insert([v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]) {
            out.push(*v);
        }
    }
    out
}

fn world_tris(pose: &Transform, hull: &Hull) -> Vec<[Vec3; 3]> {
    let affine = pose.compute_affine();
    hull.tris.iter().map(|t| t.map(|v| affine.transform_point3(v))).collect()
}

fn bounds_of(tris: &[[Vec3; 3]]) -> (Vec3, Vec3) {
    tris.iter().flatten().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
}

/// The vertical (editor -y) shape cast of `FUN_0048D510`: how far the triangles `mover` can drop before they
/// touch the triangles of `blocks` (`None`: no contact within `max` units). The first contact of two triangle
/// meshes sliding along a line is a vertex of one on a face of the other: every vertex of the mover is dropped
/// onto the faces below it and every vertex of the blocks is raised into the faces of the mover.
/// ADAPTED: edge-on-edge contacts (two edges crossing in plan with no vertex touching a face) are not found,
/// and the shape is the part's render mesh (retail casts the part's collision shape through Havok).
pub fn sweep_down(mover: &[[Vec3; 3]], blocks: &[&OtherBlock], max: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut take = |t: f32| {
        if (-1e-4..=max).contains(&t) && best.is_none_or(|b| t < b) {
            best = Some(t.max(0.0));
        }
    };
    if mover.is_empty() {
        return None;
    }
    let mover_vertices = unique_vertices(mover);
    let mover_grid = PlanGrid::new(mover.to_vec());
    let (mlo, mhi) = bounds_of(mover);
    for block in blocks {
        let tris = world_tris(&block.pose, &block.hull);
        if tris.is_empty() {
            continue;
        }
        let (lo, hi) = bounds_of(&tris);
        if lo.x > mhi.x || hi.x < mlo.x || lo.z > mhi.z || hi.z < mlo.z {
            continue;
        }
        let grid = PlanGrid::new(tris);
        for p in &mover_vertices {
            let mut nearest = f32::NEG_INFINITY;
            grid.heights(p.x, p.z, |y| {
                if y <= p.y + 1e-4 && y > nearest {
                    nearest = y;
                }
            });
            if nearest.is_finite() {
                take(p.y - nearest);
            }
        }
        for q in unique_vertices(&grid.tris) {
            let mut nearest = f32::INFINITY;
            mover_grid.heights(q.x, q.z, |y| {
                if y >= q.y - 1e-4 && y < nearest {
                    nearest = y;
                }
            });
            if nearest.is_finite() {
                take(nearest - q.y);
            }
        }
    }
    best
}

/// `FUN_00491FF0`: the vertical move (editor y) that the part's `StayAbove` / `SnapDownTo` rules ask
/// of the block at `pose`, `None` when none applies. `others` excludes the part, its twin and its
/// parent (`useParent`); `floor` is the studio plane (the `ground` base block); `on_ground` is the
/// parentless-block case (`field_3C9`, 0x4921AE): the box's bottom goes exactly on the ground.
/// The cast of `FUN_0048D510` starts the part's shape `|origin - bottom| + 2` above its position and
/// runs 100 units down ([`sweep_down`]); the block rests where the shape first touches a listed block (or
/// the ground).
pub fn height_correction(rules: &StackRules, pose: &Transform, hull: &Hull, others: &[OtherBlock], floor: Option<f32>, on_ground: bool) -> Option<f32> {
    let (stay, down) = (rules.has_stay_above(), rules.has_snap_down());
    if !stay && !down {
        return None;
    }
    let (lo, _) = world_box(pose, hull);
    let bottom = lo.y;
    if on_ground && stay {
        return floor.map(|f| f - bottom);
    }
    let above = |b: &OtherBlock| world_box(&b.pose, &b.hull).0.y >= bottom;
    let lift = (pose.translation.y - bottom).abs() + 2.0;
    let raised: Vec<[Vec3; 3]> = world_tris(pose, hull).into_iter().map(|t| t.map(|v| v + Vec3::Y * lift)).collect();
    // The rest height of the block's bottom: where the dropped shape first touches a candidate block,
    // the ground always a candidate (the studio base is not in the lists of 0x491FF0).
    let rest = |wanted: bool, list: &[TypeKey]| -> Option<f32> {
        let candidates: Vec<&OtherBlock> = others.iter().filter(|b| !((wanted && !type_matches(b.block_type(), list)) || above(b))).collect();
        sweep_down(&raised, &candidates, SWEEP_LENGTH)
            .map(|t| bottom + lift - t)
            .into_iter()
            .chain(floor.filter(|f| *f <= bottom + lift))
            .max_by(|a, b| a.total_cmp(b))
    };
    let mut dy = None;
    if down {
        if let Some(target) = rest(true, &rules.snap_down_to) {
            let drop = bottom - target;
            if drop < SNAP_DOWN_FRACTION * SNAP_DOWN_SCALE {
                dy = Some(-drop);
            }
        }
    }
    if stay {
        if let Some(target) = rest(true, &rules.stay_above) {
            if target > bottom {
                dy = Some(target - bottom);
            }
        }
    }
    dy
}

// ---------------------------------------------------------------------------
// MoveBottomEdgeToSurface (FUN_00494760, 0x49523F..0x4959D5)
// ---------------------------------------------------------------------------

/// The position change `MoveBottomEdgeToSurface` asks for (editor world), `None` when it does nothing.
/// `input` is the orientation the rules received (row 1, flattened, is the horizontal axis the part
/// is mounted along), `output` the orientation after the other rules; `position` the block origin
/// (editor world), `hull` / `scale` the part's scaled mesh. The midpoint of the bottom edge on the
/// outer face (corners 0 and 1 of the local box) is raised to 0.1 above the studio floor and the
/// parent's box, a ray along the mount axis from 20 units behind it finds the parent's surface, and
/// when that surface is behind the edge the block is pushed back by the depth.
#[allow(clippy::too_many_arguments)]
pub fn move_bottom_edge(
    rules: &StackRules,
    input: Rows,
    output: Rows,
    position: Vec3,
    scale: Vec3,
    hull: &Hull,
    parent_pose: &Transform,
    parent_hull: &Hull,
    floor: f32,
) -> Option<Vec3> {
    if !rules.move_bottom_edge {
        return None;
    }
    let mut dir = input[1];
    dir.z = 0.0;
    let dir = normalize_eps(dir);
    if dir.length_squared() <= 0.9 {
        return None;
    }
    let scaled = |v: Vec3| to_retail(scale * v);
    let corners = box_corners(hull.min, hull.max).map(scaled);
    let min = corners.iter().fold(Vec3::splat(f32::INFINITY), |a, c| a.min(*c));
    let max = corners.iter().fold(Vec3::splat(f32::NEG_INFINITY), |a, c| a.max(*c));
    let mid = Vec3::new((min.x + max.x) * 0.5, min.y, min.z);
    let origin = to_retail(position);
    let mut edge = origin + output[0] * mid.x + output[1] * mid.y + output[2] * mid.z;
    let parent_bottom = to_retail(Vec3::new(0.0, world_box(parent_pose, parent_hull).0.y, 0.0)).z;
    edge.z = edge.z.max(floor + 0.1).max(parent_bottom + 0.1);
    let start = from_retail(edge - dir * 20.0);
    let (t, _) = ray_block(start, from_retail(dir), parent_pose, parent_hull)?;
    let hit = to_retail(start + from_retail(dir) * t);
    let surface = hit - output[1] * rules.bottom_edge_adjustment;
    let behind = surface - edge;
    if behind.dot(output[1]) < 0.0 {
        return Some(position + from_retail(output[1] * -behind.length()));
    }
    None
}

// ---------------------------------------------------------------------------
// The Align rules and the combiner (FUN_00490950 / 00490390 / 0048F3E0 / 0048EA40 / 00490F20)
// ---------------------------------------------------------------------------

/// The combiner `FUN_00490F20` passes 1.2 (0x1485A2C) as the scale of the height rules, of the Align
/// rules and of `FUN_00493320` (the first port used 1.0 for the last).
pub const COMBINER_SCALE: f32 = 1.2;
/// Reach of the three Align rules: 0.2 (0x13EF4D0) x the scale.
pub const ALIGN_FRACTION: f32 = 0.2;
/// `FUN_0048F3E0`: the two blocks' forward axes must agree within a cosine of 0.95 (0x13EF520).
pub const LATERAL_PARALLEL_DOT: f32 = 0.95;
/// `FUN_0048EA40`: a block with `modelSnapToCenterOfEditor` and no parent goes to the centre line of the
/// editor when its horizontal distance from it is under 0.04 (0x13EF51C) x the editor's bounds x the scale.
pub const EDITOR_CENTRE_FRACTION: f32 = 0.04;
/// `FUN_0048EA40` multiplies its scale by 10 (0x1473BF0) for a plant root (flora editor), else by 1.2.
pub const ALIGN_XY_PLANT_ROOT: f32 = 10.0;
/// `FUN_00493AF0`: the horizontal bound of a block that is not allowed out of bounds is half the
/// editor's `editorBoundSize` (the circle radius `bounds / 2`).
pub const BOUNDS_RADIUS_FRACTION: f32 = 0.5;

/// A block as the Align rules read it, in retail axes (`FUN_00490390` reads the block's position (+0x48),
/// its base orientation (+0xA8) and the centre of its world bounding box, `FUN_0044B2D0` mode 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignBlock {
    pub position: Vec3,
    pub rows: Rows,
    pub centre: Vec3,
}

impl AlignBlock {
    /// The block at an editor pose (the pose's rotation is both orientations: the editor keeps one).
    pub fn of(pose: &Transform, hull: &Hull) -> Self {
        let (lo, hi) = world_box(pose, hull);
        Self { position: to_retail(pose.translation), rows: rows_of(pose.rotation), centre: to_retail((lo + hi) * 0.5) }
    }
}

/// `FUN_00490390` (AlignHeightWith): among the blocks of the list whose box centre is within 0.2 x
/// `scale` of this block's centre height, the block moves vertically (retail z) to the nearest one's
/// centre height. Returns the new position (retail) and the distance `|dz| x |centre_o - centre|`; the
/// best is the smallest distance (the first among equals).
pub fn align_height(this: &AlignBlock, siblings: &[AlignBlock], scale: f32) -> Option<(Vec3, f32)> {
    let mut best: Option<(Vec3, f32)> = None;
    for o in siblings {
        let dz = o.centre.z - this.centre.z;
        if ALIGN_FRACTION * scale > dz.abs() {
            let distance = dz.abs() * (o.centre - this.centre).length();
            if best.is_none_or(|(_, d)| d > distance) {
                best = Some((this.position + Vec3::new(0.0, 0.0, dz), distance));
            }
        }
    }
    best
}

/// `FUN_0048F3E0` (AlignLateralWith): among the blocks of the list whose horizontal forward axis is
/// within cos 0.95 of this block's and whose offset along their own x axis is under 0.2 x `scale`, the block
/// moves along that axis onto the nearest one's centre line (its height stays) and its base orientation is
/// turned about the up axis by `acos(dot)` (the sense of `FUN_006BAB50` = `FUN_00453FF0`'s right-handed matrix
/// composed in world space; the unsigned angle is retail's) and passed through `FUN_00490B90`. The centres are
/// the box centres with y (forward) replaced by the position's. Returns (position, orientation, distance).
pub fn align_lateral(this: &AlignBlock, siblings: &[AlignBlock], scale: f32) -> Option<(Vec3, Rows, f32)> {
    let c_this = Vec3::new(this.centre.x, this.position.y, this.centre.z);
    let mut fwd = this.rows[1];
    fwd.z = 0.0;
    let fwd = normalize_strict(fwd);
    let mut best: Option<(Vec3, Rows, f32)> = None;
    for o in siblings {
        let c_o = Vec3::new(o.centre.x, o.position.y, o.centre.z);
        let delta = c_this - c_o;
        let local = Vec3::new(delta.dot(o.rows[0]), delta.dot(o.rows[1]), delta.dot(o.rows[2]));
        let mut o_fwd = o.rows[1];
        o_fwd.z = 0.0;
        let dot = fwd.dot(normalize_strict(o_fwd));
        if !(dot > LATERAL_PARALLEL_DOT && ALIGN_FRACTION * scale > local.x.abs()) {
            continue;
        }
        let position = c_o + o.rows[1] * local.y + o.rows[2] * local.z;
        let mut rows = this.rows;
        if dot != 1.0 {
            let angle = dot.acos();
            if angle.is_finite() {
                let q = Quat::from_axis_angle(UP, angle);
                rows = rows.map(|r| q * r);
            }
        }
        let distance = (c_o - c_this).length() * local.x.abs() * dot;
        if best.is_none_or(|(_, _, d)| d > distance) {
            best = Some((position, rows, distance));
        }
    }
    let (position, rows, distance) = best?;
    Some((position, align_to_snap_vector(this.rows, rows[1], rows[2]), distance))
}

/// The inputs of `FUN_0048EA40` besides the blocks.
#[derive(Clone, Copy, Debug)]
pub struct XyRule {
    /// The block has `modelAlignXYWith` and the list is not empty (attribute 0x35).
    pub use_list: bool,
    /// `modelSnapToCenterOfEditor` on a block without a parent: the radius of the centre snap
    /// (`0.04 x bounds x scale`).
    pub centre_radius: Option<f32>,
}

/// `FUN_0048EA40` (AlignXYWith): in the parent's frame (world for a parentless block) the block moves in x
/// and / or y onto the nearest block of the list whose x / y is within 0.2 x `scale`; the distance is
/// `|move| x |position offset|`. A parentless block with `modelSnapToCenterOfEditor` within the radius of the
/// editor's centre line goes onto it instead (its distance is the retail `|z|`: the length of the vector it
/// builds after zeroing x and y). `parent` = the parent's position and orientation.
pub fn align_xy(this: &AlignBlock, siblings: &[AlignBlock], parent: Option<(Vec3, Rows)>, rule: XyRule, scale: f32) -> Option<(Vec3, f32)> {
    let (pp, m) = parent.unwrap_or((Vec3::ZERO, IDENTITY_ROWS));
    let local = |p: Vec3| {
        let d = p - pp;
        Vec3::new(d.dot(m[0]), d.dot(m[1]), d.dot(m[2]))
    };
    let world = |q: Vec3| m[0] * q.x + m[1] * q.y + m[2] * q.z + pp;
    let q_this = local(this.position);
    let mut aligned: Option<(Vec3, f32)> = None;
    if rule.use_list {
        let mut best: Option<(Vec3, f32)> = None;
        for o in siblings {
            let q_o = local(o.position);
            let (dx, dy) = (q_o.x - q_this.x, q_o.y - q_this.y);
            let mut cand = q_this;
            let mut hit = false;
            if dx.abs() < ALIGN_FRACTION * scale {
                cand.x += dx;
                hit = true;
            }
            if dy.abs() < ALIGN_FRACTION * scale {
                cand.y += dy;
                hit = true;
            }
            if hit {
                let score = (cand - q_this).length() * (q_o - q_this).length();
                if best.is_none_or(|(_, d)| d > score) {
                    best = Some((cand, score));
                }
            }
        }
        aligned = best.map(|(q, d)| (world(q), d));
    }
    if let Some(radius) = rule.centre_radius {
        let horizontal = Vec3::new(this.position.x, this.position.y, 0.0).length();
        if radius > horizontal {
            return Some((Vec3::new(0.0, 0.0, this.position.z), this.position.z.abs()));
        }
    }
    aligned
}

/// The candidates of the combiner `FUN_00490F20`, in retail axes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidates {
    /// The block's position when the combiner is called (+0x48).
    pub position: Vec3,
    /// Stay above / snap down (`FUN_00491FF0`): the vertical displacement and its distance.
    pub stay: Option<(f32, f32)>,
    pub height: Option<(Vec3, f32)>,
    pub lateral: Option<(Vec3, Rows, f32)>,
    pub xy: Option<(Vec3, f32)>,
    /// `FUN_00493320`: the snapped position and its distance.
    pub snap: Option<(Vec3, f32)>,
}

/// Which candidate's orientation the combiner returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Orientation {
    /// The incoming one.
    Keep,
    /// The lateral align's.
    Lateral,
    /// The snap to the parent's (`FUN_00492D80`).
    Snap,
}

/// What the combiner decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Combined {
    pub position: Vec3,
    pub orientation: Orientation,
    /// `SetSnapType` runs only when the snap to the parent is the only x / y source besides the XY
    /// align (0x4914C3 .. 0x491560): in the other branches the block's snap type stays -2.
    pub snap_type_set: bool,
    /// Retail's return value: the sum of the distances of the two groups, `None` = -1.
    pub distance: Option<f32>,
}

/// `FUN_00490F20` after the candidates (0x4912D0 .. 0x4917E3). z comes from the nearer of the stay above /
/// snap down rule (ties: the stay rule) and the height align; x, y from the XY align, overridden by the snap
/// to the parent alone, by the lateral align alone or by the nearer of the two (ties: the snap); the result
/// distance is the sum of the group distances.
pub fn combine(c: &Candidates) -> Combined {
    let mut p = c.position;
    let (d_z, z) = match (c.height, c.stay) {
        (None, None) => (None, p.z),
        (None, Some((dz, d))) => (Some(d), p.z + dz),
        (Some((h, d)), None) => (Some(d), h.z),
        (Some((h, dh)), Some((dz, ds))) => {
            if ds > dh {
                (Some(dh), h.z)
            } else {
                (Some(ds), p.z + dz)
            }
        }
    };
    p.z = z;
    let mut d_xy = None;
    let mut orientation = Orientation::Keep;
    let mut snap_type_set = false;
    if let Some((q, d)) = c.xy {
        p.x = q.x;
        p.y = q.y;
        d_xy = Some(d);
    }
    match (c.lateral, c.snap) {
        (None, Some((q, d))) => {
            p.x = q.x;
            p.y = q.y;
            d_xy = Some(d);
            orientation = Orientation::Snap;
            snap_type_set = true;
        }
        (Some((q, _, d)), None) => {
            p.x = q.x;
            p.y = q.y;
            d_xy = Some(d);
            orientation = Orientation::Lateral;
        }
        (Some((q, _, dl)), Some((s, ds))) => {
            if ds > dl {
                p.x = q.x;
                p.y = q.y;
                d_xy = Some(dl);
                orientation = Orientation::Lateral;
            } else {
                p.x = s.x;
                p.y = s.y;
                d_xy = Some(ds);
                orientation = Orientation::Snap;
            }
        }
        (None, None) => {}
    }
    let distance = match (d_xy, d_z) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => Some(a + b),
    };
    Combined { position: p, orientation, snap_type_set, distance }
}

/// `FUN_00493AF0`'s horizontal bound: a block that is not allowed out of bounds
/// (`modelIsAllowedOutOfBounds` 0, attribute 0x33) keeps its position inside the circle of radius
/// `bounds / 2` (`editorBoundSize`) round the editor's centre line; further out it is pulled onto the circle
/// (retail then also drops its parent). Editor-frame position.
pub fn clamp_to_bounds(position: Vec3, bounds: f32) -> Vec3 {
    let radius = bounds * BOUNDS_RADIUS_FRACTION;
    let horizontal = Vec2::new(position.x, position.z);
    if bounds > 0.0 && horizontal.length() > radius {
        let at = horizontal.normalize() * radius;
        return Vec3::new(at.x, position.y, at.y);
    }
    position
}

// ---------------------------------------------------------------------------
// Parents, the root check and the snap replace test
// ---------------------------------------------------------------------------

/// ADAPTED: how far from its parent's triangles the origin of a block may be to count as standing on it
/// (retail stores every block's parent, `EditorRigblock::mpParent` +0x33C, and the creation files carry the
/// child lists; OpenSpore's rigid blocks keep only their pose).
pub const PARENT_REACH: f32 = 0.5;

/// The closest point of triangle (a, b, c) to p (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The distance from a world point to the surface of a block.
pub fn surface_distance(point: Vec3, pose: &Transform, hull: &Hull) -> f32 {
    let affine = pose.compute_affine();
    hull.tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| affine.transform_point3(v));
            (closest_on_triangle(point, a, b, c) - point).length_squared()
        })
        .fold(f32::INFINITY, f32::min)
        .sqrt()
}

/// ADAPTED: the parent of block `i` = the block it can interact with whose surface is nearest its origin
/// within [`PARENT_REACH`] (`None` = parentless: on the studio base or floating). `skip` blocks (its twin)
/// are not candidates.
pub fn derive_parent(i: usize, rules: &[Option<Arc<StackRules>>], poses: &[(Transform, Option<Arc<Hull>>)], skip: &[usize]) -> Option<usize> {
    let origin = poses.get(i)?.0.translation;
    let me = rules.get(i).and_then(|r| r.as_deref());
    let mut best: Option<(f32, usize)> = None;
    for (j, (pose, hull)) in poses.iter().enumerate() {
        let Some(hull) = hull else { continue };
        if j == i || skip.contains(&j) {
            continue;
        }
        if let (Some(m), Some(o)) = (me, rules.get(j).and_then(|r| r.as_deref())) {
            if !m.can_interact_with(o) {
                continue;
            }
        }
        let (lo, hi) = world_box(pose, hull);
        if origin.cmplt(lo - Vec3::splat(PARENT_REACH)).any() || origin.cmpgt(hi + Vec3::splat(PARENT_REACH)).any() {
            continue;
        }
        let d = surface_distance(origin, pose, hull);
        if d <= PARENT_REACH && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, j));
        }
    }
    best.map(|(_, j)| j)
}

/// `FUN_00436200`: a block is validly attached when it, or an ancestor, may be parentless
/// (`modelCanBeParentless`, attribute 0x23), is a vertebra (7) or a plant root (8); the walk goes up the
/// parents (at most 100). A block without rules (not an original definition) is always valid.
pub fn connected_to_root(start: usize, rules: &[Option<Arc<StackRules>>], parent_of: &dyn Fn(usize) -> Option<usize>) -> bool {
    let mut at = Some(start);
    for _ in 0..100 {
        let Some(i) = at else {
            return false;
        };
        match rules.get(i).and_then(|r| r.as_deref()) {
            None => return true,
            Some(r) if r.can_be_parentless || r.plant_root => return true,
            Some(_) => {}
        }
        at = parent_of(i);
    }
    false
}

/// `EditorRigblock` +0x224: the radius of the replace snap, `clamp(width x modelReplaceSnapDelta, 0.01, 0.3)`
/// (the parser 0x446226; `width` = the model's x extent) x the block's scale (+0x1D8).
#[allow(dead_code)] // no installed building / vehicle / UFO part lists `modelTypesToSnapReplace`
pub fn replace_radius(rules: &StackRules, width: f32, size: f32) -> f32 {
    (width * rules.replace_snap_delta).clamp(0.01, 0.3) * size
}

/// `FUN_004A1DC0(this, other, d)`: the block `other` at distance `d` from the pin point is a replace target
/// of the moving block `this` when `this` lists its type in `modelTypesToSnapReplace`, `other` is visible
/// and `d` is within the replace radius of either block (`FUN_0043C1B0`: `d < radius`; the retail halved
/// second test and the axes test after it can only agree with the first, so they are not reproduced).
#[allow(dead_code)] // see `replace_radius`
pub fn should_snap_replace(this: &StackRules, other: &StackRules, d: f32, this_radius: f32, other_radius: f32) -> bool {
    type_matches(other.block_type, &this.snap_replace) && (d.abs() < this_radius || d.abs() < other_radius)
}

// ---------------------------------------------------------------------------
// Stored parent links (EditorRigblock::mpParent +0x33C / mChildren +0x340)
// ---------------------------------------------------------------------------

/// The id of block `i` (0: none assigned).
pub fn block_id(parts: &[PartInstance], i: usize) -> u32 {
    parts.get(i).and_then(|p| p.rigid.as_ref()).map_or(0, |b| b.id)
}

/// The next unused block id.
pub fn next_id(parts: &[PartInstance]) -> u32 {
    parts.iter().filter_map(|p| p.rigid.as_ref()).map(|b| b.id).max().unwrap_or(0) + 1
}

/// The parent of every block as an index of `parts` (`None`: parentless, or a parent that is not in the design).
pub fn parent_table(parts: &[PartInstance]) -> Vec<Option<usize>> {
    let index: HashMap<u32, usize> = parts.iter().enumerate().filter_map(|(i, p)| p.rigid.as_ref().filter(|b| b.id != 0).map(|b| (b.id, i))).collect();
    parts.iter().map(|p| p.rigid.as_ref().filter(|b| b.parent != 0).and_then(|b| index.get(&b.parent).copied())).collect()
}

/// The indices of the symmetric twin(s) of block `i` (`field_3E0`, `mpSymmetricRigblock`).
pub fn twins_of(parts: &[PartInstance], i: usize) -> Vec<usize> {
    let pair = parts.get(i).map_or(0, |p| p.pair_id);
    parts.iter().enumerate().filter(|(j, p)| *j != i && pair != 0 && p.pair_id == pair).map(|(j, _)| j).collect()
}

/// `FUN_0048CC90`: the blocks below block `i` (its children, their children ...); a cycle in the links is cut.
pub fn descendants(parts: &[PartInstance], i: usize) -> Vec<usize> {
    let parents = parent_table(parts);
    let mut out: Vec<usize> = Vec::new();
    let mut frontier = vec![i];
    while let Some(at) = frontier.pop() {
        for (j, p) in parents.iter().enumerate() {
            if *p == Some(at) && j != i && !out.contains(&j) {
                out.push(j);
                frontier.push(j);
            }
        }
    }
    out
}

/// `FUN_00436200` for the stored links: the parent chain of block `i` as a closure for [`connected_to_root`].
pub fn parent_of_table(parts: &[PartInstance]) -> impl Fn(usize) -> Option<usize> {
    let table = parent_table(parts);
    move |i| table.get(i).copied().flatten()
}

/// Give block `i` an id when it has none.
pub fn ensure_id(parts: &mut [PartInstance], i: usize) -> u32 {
    let id = block_id(parts, i);
    if id != 0 || parts.get(i).is_none_or(|p| p.rigid.is_none()) {
        return id;
    }
    let id = next_id(parts);
    parts[i].rigid.as_mut().unwrap().id = id;
    id
}

/// After block `i` got its parent the symmetric twin follows: it keeps (or gets) its own id and stands on the
/// parent's twin, or on the same parent when that block is single (what the creation files store: each of
/// the pair is in its own parent's `childlist`).
pub fn link_twin(parts: &mut [PartInstance], i: usize) {
    let twins = twins_of(parts, i);
    if twins.is_empty() {
        return;
    }
    let mapped = match parent_table(parts).get(i).copied().flatten() {
        Some(p) => twins_of(parts, p).first().copied().map_or(block_id(parts, p), |q| block_id(parts, q)),
        None => 0,
    };
    for t in twins {
        ensure_id(parts, t);
        if let Some(b) = parts[t].rigid.as_mut() {
            b.parent = mapped;
        }
    }
}

/// Links to blocks that are not in the design (a sold parent) are cleared: the children stay, parentless.
pub fn prune_links(parts: &mut [PartInstance]) {
    let ids: HashSet<u32> = parts.iter().filter_map(|p| p.rigid.as_ref()).map(|b| b.id).filter(|id| *id != 0).collect();
    for p in parts.iter_mut() {
        if let Some(b) = p.rigid.as_mut().filter(|b| b.parent != 0 && !ids.contains(&b.parent)) {
            b.parent = 0;
        }
    }
}

/// A design saved before the links were stored: every block without an id is numbered and its parent derived
/// from the poses once (`derive_parent`), the way the session used to see it. Returns whether anything changed.
/// ADAPTED: only for such old designs; new blocks and creation files carry the parent retail stores.
pub fn legacy_links(parts: &mut [PartInstance], rules: &[Option<Arc<StackRules>>], poses: &[(Transform, Option<Arc<Hull>>)]) -> bool {
    let legacy: Vec<usize> = parts.iter().enumerate().filter(|(_, p)| p.rigid.as_ref().is_some_and(|b| b.id == 0)).map(|(i, _)| i).collect();
    if legacy.is_empty() {
        return false;
    }
    for &i in &legacy {
        ensure_id(parts, i);
    }
    let ids: Vec<u32> = (0..parts.len()).map(|i| block_id(parts, i)).collect();
    for &i in &legacy {
        let parent = derive_parent(i, rules, poses, &twins_of(parts, i));
        if let Some(b) = parts[i].rigid.as_mut() {
            b.parent = parent.map_or(0, |j| ids[j]);
        }
    }
    true
}

/// The poses (editor frame) of the blocks by id, taken before an edit.
pub fn poses_by_id(parts: &[PartInstance]) -> HashMap<u32, (Vec3, Quat)> {
    parts.iter().filter_map(|p| p.rigid.as_ref().filter(|b| b.id != 0).map(|b| (b.id, (b.position(), b.rotation())))).collect()
}

/// `FUN_0049F4D0` (called by `SetTransform` 0x4A00E0 for every pose change): a block under a moved block keeps
/// its place in it. With `delta` = the old orientation's transpose times the new one (row vectors: `M_old^T M_new`)
/// the child goes to `(p - P_old) delta + P_new` and its orientation to `M delta`; in column terms the rigid motion
/// that took the parent from `old` to `new` applied to the child.
pub fn carried(old: (Vec3, Quat), new: (Vec3, Quat), child: (Vec3, Quat)) -> (Vec3, Quat) {
    let delta = new.1 * old.1.inverse();
    (new.0 + delta * (child.0 - old.0), (delta * child.1).normalize())
}

/// Move the descendants of every block of `moved` (indices of `parts`) with it, given the poses before the edit
/// (`poses_by_id`). Blocks that moved by themselves are not carried.
pub fn carry_descendants(parts: &mut [PartInstance], before: &HashMap<u32, (Vec3, Quat)>, moved: &[usize]) {
    for &m in moved {
        let Some(b) = parts.get(m).and_then(|p| p.rigid.as_ref()) else { continue };
        let Some(&old) = before.get(&b.id) else { continue };
        let new = (b.position(), b.rotation());
        if new.0.distance(old.0) < 1e-7 && new.1.dot(old.1).abs() > 1.0 - 1e-7 {
            continue;
        }
        for d in descendants(parts, m) {
            if moved.contains(&d) {
                continue;
            }
            let Some(child) = parts[d].rigid.as_mut() else { continue };
            let Some(&was) = before.get(&child.id) else { continue };
            let (position, rotation) = carried(old, new, was);
            child.position = position.to_array();
            child.set_rotation(rotation);
        }
    }
}

// ---------------------------------------------------------------------------
// The studio base's parentless placement (FUN_0049BDC0 / FUN_0049C1D0 / FUN_0049C600 / FUN_004AC170)
// ---------------------------------------------------------------------------

/// The plane `FUN_0049BDC0` chooses for a block without a parent (retail axes): the block's origin follows the camera
/// ray on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreePlane {
    pub point: Vec3,
    pub normal: Vec3,
}

/// `FUN_004AC170` as `FUN_004AC0E0(position, block, usePositions, flat)` calls it: among the blocks of the model that are
/// not the moving one, its descendants or its twin, the one whose origin is horizontally (retail xy) nearest to `me`;
/// its position (`None`: no other block, retail returns (-1, -1, -1)). Retail axes.
pub fn nearest_block_position(me: Vec3, others: &[Vec3]) -> Option<Vec3> {
    others
        .iter()
        .map(|o| (Vec2::new(o.x - me.x, o.y - me.y).length(), *o))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, o)| o)
}

/// `FUN_0049BDC0(block, rayDir, &point, &normal)`, retail axes. `d` = the ray's forward component; a symmetric editor,
/// attribute 0xF and |d| > 0.8 (looking along the plane of symmetry): the plane of symmetry itself (point 0, normal
/// (1, 0, 0)). A block that rests on the ground (`FUN_0043C080`): the plane through the nearest other block's origin, or
/// through half the editor's bounds toward the camera when there is none; any other block: through its own origin. The
/// normal is the horizontal direction back to the camera, `normalise(-ray.x, -ray.y, 0)` (zero for a ray straight down:
/// no plane, the block stays where it is).
pub fn free_plane(ray_dir: Vec3, block: Vec3, rests_on_ground: bool, nearest: Option<Vec3>, bounds: f32, plane_of_symmetry: bool) -> FreePlane {
    if plane_of_symmetry && ray_dir.y.abs() > 0.8 {
        return FreePlane { point: Vec3::ZERO, normal: Vec3::X };
    }
    let flat = Vec3::new(-ray_dir.x, -ray_dir.y, 0.0);
    let normal = flat.normalize_or_zero();
    let point = if rests_on_ground { nearest.unwrap_or(flat * bounds * 0.5) } else { block };
    FreePlane { point, normal }
}

/// `FUN_0049C1D0(block, rayOrigin, rayDir, point, normal, onGround = 1)`, retail axes: the pose a parentless block takes.
/// `P1` = the ray meets the plane (`FUN_0044EB10`; the block's own origin when it does not). `C0` = the ray point at the
/// height that puts the block's box (with its descendants) on the ground: `below_origin` = origin height - lower z of
/// that box. C0 when it is higher than P1 (the block then rests on the ground: `field_3C9`, `FUN_0049C600`), else P1.
/// The orientation stays the stored one.
pub fn free_position(ray_origin: Vec3, ray_dir: Vec3, plane: &FreePlane, block: Vec3, below_origin: f32) -> (Vec3, bool) {
    let p1 = if plane.normal == Vec3::ZERO {
        block
    } else {
        ray_plane(ray_origin, ray_dir, plane.normal, -plane.point.dot(plane.normal)).map_or(block, |s| ray_origin + ray_dir * s)
    };
    if ray_dir.z.abs() < 1e-6 {
        return (p1, false);
    }
    let t = (below_origin - ray_origin.z) / ray_dir.z;
    let c0 = ray_origin + ray_dir * t;
    if c0.z > p1.z {
        (c0, true)
    } else {
        (p1, false)
    }
}

// ---------------------------------------------------------------------------
// What the placement hands over
// ---------------------------------------------------------------------------

/// The parent of a dragged block as the placement sees it.
#[derive(Clone)]
pub struct ParentInput {
    pub rules: Arc<StackRules>,
    pub pose: Transform,
    pub hull: Arc<Hull>,
    pub target: Arc<SnapTarget>,
}

/// The editor around the dragged block.
#[derive(Clone, Copy, Debug, Default)]
pub struct StackEnv {
    /// The studio plane (`None`: no base).
    pub floor: Option<f32>,
    /// `editorUseSymmetry`: the editor mirrors parts across x = 0.
    pub symmetric: bool,
    /// Half width of the band around x = 0 in which a block of a symmetric editor is on the plane.
    pub plane_band: f32,
    /// The setup's `editorBoundSize` (`EditorModel` +0x38; 0 = unknown / unlimited): the circle of radius
    /// `bounds / 2` that a block without `modelIsAllowedOutOfBounds` stays in, and the base of the
    /// centre-of-editor snap radius.
    pub bounds: f32,
}

/// The three Align rules' answers (retail axes).
#[derive(Clone, Copy, Debug, Default)]
pub struct AlignResults {
    pub height: Option<(Vec3, f32)>,
    pub lateral: Option<(Vec3, Rows, f32)>,
    pub xy: Option<(Vec3, f32)>,
}

/// The pose `FUN_0049C1D0` gave a block without a parent (editor frame): its origin follows the camera ray on the
/// plane of `FUN_0049BDC0` ([`free_position`]); `on_ground` is the flag `field_3C9` it set when the ray point at the
/// height that stands the block on the ground was above the plane point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreePose {
    pub position: Vec3,
    pub on_ground: bool,
}

/// What the stacking rules need to place one block.
#[derive(Clone, Default)]
pub struct StackInput {
    pub rules: Option<Arc<StackRules>>,
    pub parent: Option<ParentInput>,
    /// The other blocks (not the part, its twin, its descendants or its parent).
    pub others: Vec<OtherBlock>,
    pub env: StackEnv,
    /// The parentless pose of the studio base (`Some` when no block was hit: the ray follows the plane).
    pub free: Option<FreePose>,
}

impl StackInput {
    /// The input of a block resting on `surface` (`surface.block` is the parent). `skip` are the
    /// part's own indices (itself, its twin and its descendants); `parents` the stored parent of every block
    /// (`parent_table`): the siblings of the Align rules are the blocks with the moving block's parent.
    pub fn of(
        rules: &[Option<Arc<StackRules>>],
        moving: usize,
        surface: &Surface,
        poses: &[(Transform, Option<Arc<Hull>>)],
        parents: &[Option<usize>],
        skip: &[usize],
        env: StackEnv,
    ) -> Self {
        let parent = surface.block.and_then(|p| {
            let prules = rules.get(p)?.clone()?;
            let pose = poses.get(p)?.0;
            let hull = poses.get(p)?.1.clone()?;
            let target = Arc::new(SnapTarget::of(&prules, &pose, &hull));
            Some(ParentInput { rules: prules, pose, hull, target })
        });
        let mine = rules.get(moving).cloned().flatten();
        let others = poses
            .iter()
            .enumerate()
            .filter(|(i, _)| !skip.contains(i) && Some(*i) != surface.block)
            .filter_map(|(i, (pose, hull))| {
                Some(OtherBlock { parent: parents.get(i).copied().flatten(), pose: *pose, hull: hull.clone()?, rules: rules.get(i).cloned().flatten() })
            })
            .collect();
        Self { rules: mine, parent, others, env, free: None }
    }

    /// The Align rules (`FUN_00490390` height, `FUN_0048F3E0` lateral, `FUN_0048EA40` XY) for the moving block
    /// at its stored pose `this` (retail axes). `parent` is the index of the block it stands on (`None` = the
    /// studio base): the siblings are the other blocks with that parent (`FUN_004908D0`) whose type the
    /// matching list names (`FUN_00490950`).
    pub fn align(&self, this: &AlignBlock, parent: Option<usize>) -> AlignResults {
        let Some(rules) = self.rules.as_deref() else {
            return AlignResults::default();
        };
        let siblings = |list: &[TypeKey]| -> Vec<AlignBlock> {
            if list.is_empty() {
                return Vec::new();
            }
            self.others.iter().filter(|o| o.parent == parent && type_matches(o.block_type(), list)).map(|o| AlignBlock::of(&o.pose, &o.hull)).collect()
        };
        let frame = self.parent.as_ref().map(|p| (to_retail(p.pose.translation), rows_of(p.pose.rotation)));
        let xy_scale = COMBINER_SCALE * if rules.plant_root { ALIGN_XY_PLANT_ROOT } else { COMBINER_SCALE };
        let rule = XyRule {
            use_list: !rules.align_xy.is_empty(),
            centre_radius: (rules.snap_to_editor_centre && self.parent.is_none()).then_some(EDITOR_CENTRE_FRACTION * self.env.bounds * xy_scale),
        };
        AlignResults {
            height: align_height(this, &siblings(&rules.align_height), COMBINER_SCALE),
            lateral: align_lateral(this, &siblings(&rules.align_lateral), COMBINER_SCALE),
            xy: if rule.use_list || rule.centre_radius.is_some() { align_xy(this, &siblings(&rules.align_xy), frame, rule, xy_scale) } else { None },
        }
    }

    /// Is the surface the parent's top face? `FUN_005B2E90`: the normal within 0.99 of the up axis
    /// and the pin point within 0.5 of the top of the parent's world box.
    pub fn on_top(&self, surface: &Surface) -> bool {
        let Some(parent) = &self.parent else {
            return false;
        };
        let top = world_box(&parent.pose, &parent.hull).1.y;
        surface.normal.dot(Vec3::Y) > TOP_FACE_NORMAL_DOT && (top - surface.point.y).abs() < TOP_FACE_HEIGHT
    }

    /// `RemainUpright` / `RemainUprightOnTop` of the moving block for this surface.
    pub fn upright(&self, surface: &Surface) -> bool {
        self.rules.as_ref().is_some_and(|r| r.stays_upright(self.parent.as_ref().map(|p| &*p.rules), self.on_top(surface)))
    }

    /// The side of a block at editor `x` (`GetSide`: the sign of x; 0 on the plane of a symmetric
    /// editor). ADAPTED: retail stores the side per block (the natural side is the sign of x).
    pub fn side_of(&self, x: f32) -> i32 {
        if x.abs() <= 0.001 || (self.env.symmetric && x.abs() <= self.env.plane_band) {
            0
        } else if x < 0.0 {
            -1
        } else {
            1
        }
    }

    /// The orientation plan of the moving block resting on `surface` at `position` (editor world).
    pub fn plan(&self, surface: &Surface, position: Vec3, orient_to_surface: bool, snap: Option<&ParentSnap>) -> OrientPlan {
        let Some(rules) = self.rules.as_deref() else {
            return OrientPlan::default();
        };
        let parent = self.parent.as_ref();
        let on_top = self.on_top(surface) && parent.is_some_and(|p| p.rules.allow_top_behaviors);
        // 0x494974: on top and not centre-snapped (snapType != -1).
        let centred = snap.is_some_and(|s| s.vector.is_none());
        let top_rows = parent.filter(|_| on_top && !centred).map(|p| p.target.top_rows(position, p.rules.circular_top));
        let snap_align = parent.zip(snap.and_then(|s| s.vector)).map(|(p, i)| (p.target.vector_direction(i), p.target.rows[2]));
        OrientPlan {
            has_parent: parent.is_some(),
            on_top,
            top_rows,
            snap_align,
            remain_upright: self.upright(surface),
            point_forward: rules.point_forward,
            act_like_fin: rules.act_like_fin,
            no_flip: rules.no_flip_on_plane,
            orient_to_surface,
            symmetric: self.env.symmetric,
            side: self.side_of(position.x),
            surface_normal: to_retail(surface.normal),
            base: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(group: u32, instance: u32, kind: u32) -> TypeKey {
        TypeKey { group, instance, kind }
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn type_matching_needs_the_group_and_treats_zero_as_a_wildcard() {
        let roof = key(1, 20, 0);
        assert!(type_matches(roof, &[key(1, 20, 0)]));
        assert!(type_matches(roof, &[key(1, 0, 0)]), "a zero instance matches every part type of the group");
        assert!(!type_matches(roof, &[key(2, 20, 0)]), "another group never matches");
        assert!(!type_matches(roof, &[key(1, 21, 0)]));
        assert!(!type_matches(roof, &[]), "an empty list matches nothing (0x438900)");
        // A detail sub-type: the list entry's type field restricts, 0 does not.
        let flag = key(1, 30, 7);
        assert!(type_matches(flag, &[key(1, 30, 0)]));
        assert!(type_matches(flag, &[key(1, 30, 7)]));
        assert!(!type_matches(flag, &[key(1, 30, 8)]));
    }

    #[test]
    fn interaction_needs_a_wanted_type_and_no_refused_one() {
        let structure = StackRules { block_type: key(1, 10, 0), ..default() };
        let roof = StackRules { block_type: key(1, 20, 0), ..default() };
        let free = StackRules::default();
        assert!(free.can_interact_with(&roof), "no lists: everything interacts");
        let only_structure = StackRules { interact_with: vec![key(1, 10, 0)], ..default() };
        assert!(only_structure.can_interact_with(&structure));
        assert!(!only_structure.can_interact_with(&roof));
        let not_roof = StackRules { not_interact_with: vec![key(1, 20, 0)], ..default() };
        assert!(not_roof.can_interact_with(&structure));
        assert!(!not_roof.can_interact_with(&roof));
        let both = StackRules { interact_with: vec![key(1, 0, 0)], not_interact_with: vec![key(1, 20, 0)], ..default() };
        assert!(both.can_interact_with(&structure) && !both.can_interact_with(&roof));
        assert!(pin_allows(Some(&only_structure), None), "a block without rules is always hit");
        assert!(!pin_allows(Some(&only_structure), Some(&roof)));
    }

    #[test]
    fn centre_snapping_needs_the_attribute_and_a_listed_parent_type() {
        let roof = StackRules { block_type: key(1, 20, 0), ..default() };
        let chimney = StackRules { snap_to_parent_center: true, snap_to_parent: vec![key(1, 20, 0)], ..default() };
        assert!(chimney.snaps_to_center_of(&roof));
        let door = StackRules { snap_to_parent_center: false, snap_to_parent: vec![key(1, 20, 0)], ..default() };
        assert!(!door.snaps_to_center_of(&roof));
        let unlisted = StackRules { snap_to_parent_center: true, ..default() };
        assert!(!unlisted.snaps_to_center_of(&roof), "an empty parent-type list never snaps");
    }

    #[test]
    fn the_on_top_flag_selects_the_upright_rule() {
        let parent = StackRules { allow_top_behaviors: true, ..default() };
        let closed = StackRules::default();
        let chimney = StackRules { remain_upright: false, remain_upright_on_top: true, ..default() };
        assert!(chimney.stays_upright(Some(&parent), true));
        assert!(!chimney.stays_upright(Some(&parent), false));
        assert!(!chimney.stays_upright(Some(&closed), true), "the parent must allow top behaviours");
        let door = StackRules { remain_upright: true, remain_upright_on_top: false, ..default() };
        assert!(door.stays_upright(Some(&closed), true), "without top behaviours the plain flag decides");
        assert!(!door.stays_upright(Some(&parent), true));
    }

    #[test]
    fn the_editor_and_retail_frames_agree_on_the_rows() {
        // The identity is the identity in both frames.
        let rows = rows_of(Quat::IDENTITY);
        assert!(near(rows[0], Vec3::X) && near(rows[1], Vec3::Y) && near(rows[2], Vec3::Z), "{rows:?}");
        // A turn about the editor's up axis is a turn about the retail Z axis (rows 0 and 1 swing, row 2 stays).
        let q = Quat::from_rotation_y(0.7);
        let rows = rows_of(q);
        assert!(near(rows[2], Vec3::Z), "{rows:?}");
        assert!((rows[0].z).abs() < 1e-5 && (rows[1].z).abs() < 1e-5);
        // The mount axis (editor +Z) is the retail local -Y.
        assert!(near(from_retail(-rows[1]), q * Vec3::Z));
        // Rows round-trip.
        let q = Quat::from_euler(EulerRot::YXZ, 0.9, -0.4, 0.3);
        assert!(rotation_of(rows_of(q)).dot(q).abs() > 0.9999);
        // Retail forward (+Y) is the editor's -Z, retail up (+Z) the editor's +Y.
        assert!(near(from_retail(FORWARD), Vec3::NEG_Z) && near(from_retail(UP), Vec3::Y));
    }

    #[test]
    fn upright_keeps_the_up_axis_vertical_and_the_heading() {
        // A part tilted forward by 30 degrees about its right axis and turned 40 about up.
        let tilted = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.5);
        let r = rotation_of(remain_upright_rows(rows_of(tilted)));
        assert!((r * Vec3::Y).dot(Vec3::Y) > 0.9999, "up axis vertical: {:?}", r * Vec3::Y);
        let heading = Vec3::new((tilted * Vec3::Z).x, 0.0, (tilted * Vec3::Z).z).normalize();
        assert!((r * Vec3::Z).dot(heading) > 0.9999, "the flattened forward axis is kept");
        // Already upright: unchanged.
        let ok = Quat::from_rotation_y(1.1);
        assert!(rotation_of(remain_upright_rows(rows_of(ok))).dot(ok).abs() > 0.9999);
        // Lying with its forward axis vertical (an oriented part on a top face): retail's fallback,
        // row 2 = up and row 1 = up x row 0 (0x494D18), keeps the right axis.
        let lying = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
        let rows = rows_of(lying);
        let fixed = remain_upright_rows(rows);
        assert!(near(fixed[2], UP) && near(fixed[0], rows[0]), "{fixed:?}");
        assert!(near(fixed[1], UP.cross(rows[0])));
        // A row 1 that is vertical within float noise falls back too (retail's epsilon would toss a coin).
        let noisy = [Vec3::new(0.8, 0.6, 0.0), Vec3::new(-3e-8, 7e-9, -1.0), Vec3::new(0.6, -0.8, 0.0)];
        assert!(near(remain_upright_rows(noisy)[2], UP));
        assert!(near(point_forward_rows([noisy[0], Vec3::new(3e-8, 1.0, 7e-9), noisy[2]], 1)[2], UP));
    }

    #[test]
    fn point_forward_turns_the_x_axis_along_the_forward_axis_and_negates_it_on_the_left() {
        // A wheel on the right of a car: its mount axis (retail -Y) points out along +X, so row 1 = -X.
        let rows = [Vec3::new(0.0, -0.5, 0.8660254), Vec3::NEG_X, Vec3::new(0.0, 0.8660254, 0.5)];
        let right = point_forward_rows(rows, 1);
        assert!(near(right[0], FORWARD), "{right:?}");
        assert!(near(right[1], Vec3::NEG_X), "row 1 keeps its horizontal part: {right:?}");
        assert!(near(right[2], FORWARD.cross(Vec3::NEG_X)));
        let left = point_forward_rows(rows, -1);
        assert!(near(left[0], -FORWARD), "the left half points the other way: {left:?}");
        // The matrix stays orthonormal.
        for m in [right, left] {
            assert!(m[0].dot(m[1]).abs() < 1e-5 && m[1].dot(m[2]).abs() < 1e-5 && (m[0].cross(m[1]) - m[2]).length() < 1e-5);
        }
        // Row 1 along the forward axis (nothing horizontal and lateral left): the fallback up / up x forward.
        let along = point_forward_rows([Vec3::X, Vec3::Y, Vec3::Z], 1);
        assert!(near(along[0], FORWARD) && near(along[2], UP) && near(along[1], UP.cross(FORWARD)), "{along:?}");
    }

    #[test]
    fn the_plane_of_symmetry_has_canonical_orientations() {
        let plan = OrientPlan { has_parent: true, symmetric: true, side: 0, ..default() };
        // RemainUpright + PointForward: [-F, X, up].
        let a = plane_orientation(IDENTITY_ROWS, &OrientPlan { remain_upright: true, point_forward: true, ..plan });
        assert!(near(a[0], -FORWARD) && near(a[1], LATERAL) && near(a[2], UP));
        // RemainUpright only: the identity.
        let b = plane_orientation(rows_of(Quat::from_rotation_y(1.0)), &OrientPlan { remain_upright: true, ..plan });
        assert_eq!(b, IDENTITY_ROWS);
        // Neither: row 0 = X, row 1 = the old row 1 without its x part, row 2 = X x row 1 (flipped when the surface faces +Y).
        let rows = [Vec3::new(0.6, 0.8, 0.0), Vec3::new(0.0, 0.6, 0.8), Vec3::new(-0.8, 0.0, 0.6)];
        let c = plane_orientation(rows, &plan);
        assert!(near(c[0], LATERAL) && near(c[1], Vec3::new(0.0, 0.6, 0.8)) && near(c[2], LATERAL.cross(c[1])), "{c:?}");
        let d = plane_orientation(rows, &OrientPlan { surface_normal: Vec3::Y, ..plan });
        assert!(near(d[0], -LATERAL) && near(d[1], c[1]) && near(d[2], -c[2]), "{d:?}");
        let e = plane_orientation(rows, &OrientPlan { surface_normal: Vec3::Y, no_flip: true, ..plan });
        assert!(near(e[0], LATERAL), "the no-flip attribute keeps the orientation: {e:?}");
        // PointForward on the plane: row 2 = X, row 0 = row 1 x X.
        let f = plane_orientation(rows, &OrientPlan { point_forward: true, ..plan });
        assert!(near(f[2], LATERAL) && near(f[0], c[1].cross(LATERAL)));
    }

    #[test]
    fn the_rules_run_in_retail_order_and_only_with_a_parent() {
        let tilted = rows_of(Quat::from_rotation_y(0.4) * Quat::from_rotation_x(0.3));
        let free = apply_orientation_rules(tilted, &OrientPlan { remain_upright: true, ..default() });
        assert!(!free.modified && free.rows == tilted, "no parent: no rule");
        let upright = apply_orientation_rules(tilted, &OrientPlan { has_parent: true, remain_upright: true, ..default() });
        assert!(upright.modified && upright.rows[1].z.abs() < 1e-5, "row 1 is horizontal");
        // A wheel (upright and point forward on the right half): the forward axis is row 0.
        let wheel = apply_orientation_rules(tilted, &OrientPlan { has_parent: true, remain_upright: true, point_forward: true, side: 1, ..default() });
        assert!(near(wheel.rows[0], FORWARD));
        // A block on the plane of a symmetric editor that no rule touches gets the plane orientation (0x495A0C).
        let on_plane = apply_orientation_rules(tilted, &OrientPlan { has_parent: true, symmetric: true, orient_to_surface: true, ..default() });
        assert!(on_plane.modified && near(on_plane.rows[0], LATERAL));
        // ... but not when the editor is not symmetric.
        let building = apply_orientation_rules(tilted, &OrientPlan { has_parent: true, orient_to_surface: true, ..default() });
        assert!(!building.modified);
        // A part that does not orient to surfaces ends with its stored orientation (0x495ACB): a roof on top
        // of a structure is not stood on its edge by the on-top rows.
        let stored = rows_of(Quat::from_rotation_y(0.3));
        let top = [Vec3::new(0.0, 1.0, 0.0), -UP, Vec3::new(-1.0, 0.0, 0.0)];
        let roof = apply_orientation_rules(tilted, &OrientPlan { has_parent: true, on_top: true, top_rows: Some(top), base: Some(stored), ..default() });
        assert!(roof.modified && roof.rows == stored, "{:?}", roof.rows);
        // ... a part that orients (a chimney) keeps the rows and is stood up by RemainUprightOnTop.
        let chimney = apply_orientation_rules(
            tilted,
            &OrientPlan { has_parent: true, on_top: true, top_rows: Some(top), base: Some(stored), orient_to_surface: true, remain_upright: true, ..default() },
        );
        assert!(near(chimney.rows[2], UP) && near(chimney.rows[0], top[0]), "{:?}", chimney.rows);
    }

    #[test]
    fn a_part_on_top_of_a_roof_takes_the_nearest_face_and_the_snap_vector_alignment() {
        let table = vec![
            SnapVector { dir: Vec3::X, point: Vec3::new(2.0, 0.0, 0.0), cardinal: true },
            SnapVector { dir: Vec3::Y, point: Vec3::new(0.0, 1.0, 0.0), cardinal: true },
            SnapVector { dir: Vec3::NEG_X, point: Vec3::new(-2.0, 0.0, 0.0), cardinal: true },
            SnapVector { dir: Vec3::NEG_Y, point: Vec3::new(0.0, -1.0, 0.0), cardinal: true },
        ];
        let target = SnapTarget { position: Vec3::ZERO, rows: IDENTITY_ROWS, center: Vec3::ZERO, table, diagonal: 4.47 };
        // On the +x half the nearest face is face 0: the rows are (up x ... ) turned to the inward direction -X.
        let rows = target.top_rows(from_retail(Vec3::new(1.5, 0.1, 1.0)), false);
        let d = -Vec3::X;
        assert!(near(rows[2], d) && near(rows[1], -UP) && near(rows[0], (-UP).cross(d)), "{rows:?}");
        // The snap vector alignment keeps row 0 along the wall tangent (dir x up).
        let aligned = align_to_snap_vector(rows, Vec3::X, UP);
        assert!(aligned[0].dot(Vec3::X.cross(UP)).abs() > 0.999, "{aligned:?}");
        // A parent without vectors gives the identity.
        let bare = SnapTarget { table: vec![], ..target.clone() };
        assert_eq!(bare.top_rows(Vec3::ZERO, false), IDENTITY_ROWS);
    }

    /// A 4 x 2 x 1 box roof-like part with its top face at y = 1 (editor frame), as retail local x = width, y = depth.
    fn box_hull(min: Vec3, max: Vec3) -> Hull {
        let c = box_corners(min, max);
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut tris = vec![];
        for q in quads {
            tris.push([c[q[0]], c[q[1]], c[q[2]]]);
            tris.push([c[q[0]], c[q[2]], c[q[3]]]);
        }
        Hull { tris, min, max }
    }


    // ---- stored links, the carry, the parentless pose, the sweep (helper editors-base) ----

    fn linked(id: u32, parent: u32, at: Vec3) -> PartInstance {
        let mut p = PartInstance::new(id, 0);
        p.rigid = Some(crate::rigid_editor::RigidBlock { id, parent, ..crate::rigid_editor::RigidBlock::at(at) });
        p
    }

    #[test]
    fn stored_links_give_parents_descendants_and_survive_a_sold_parent() {
        // 1 <- 2 <- 3, and 4 on its own; 5 stands on 9 (not in the design).
        let mut parts = vec![linked(1, 0, Vec3::ZERO), linked(2, 1, Vec3::X), linked(3, 2, Vec3::X * 2.0), linked(4, 0, Vec3::Z), linked(5, 9, Vec3::Y)];
        assert_eq!(parent_table(&parts), vec![None, Some(0), Some(1), None, None], "a parent outside the design is none");
        assert_eq!(descendants(&parts, 0), vec![1, 2]);
        assert_eq!(descendants(&parts, 1), vec![2]);
        assert!(descendants(&parts, 3).is_empty());
        // A cycle in a damaged file is cut.
        parts[0].rigid.as_mut().unwrap().parent = 3;
        assert_eq!(descendants(&parts, 0), vec![1, 2]);
        parts[0].rigid.as_mut().unwrap().parent = 0;
        // Selling block 2 (index 1): its child stays and is parentless.
        parts.remove(1);
        prune_links(&mut parts);
        assert_eq!(parent_table(&parts), vec![None, None, None, None]);
        assert_eq!(next_id(&parts), 6);
        let mut loose = PartInstance::new(1, 0);
        loose.rigid = Some(Default::default());
        parts.push(loose);
        assert_eq!(ensure_id(&mut parts, 4), 6);
        assert_eq!(ensure_id(&mut parts, 4), 6, "kept");
    }

    #[test]
    fn a_twin_stands_on_the_parents_twin_or_on_a_single_parent() {
        let mut parts = vec![linked(1, 0, Vec3::ZERO), linked(2, 1, Vec3::X), linked(3, 0, -Vec3::X), linked(4, 0, Vec3::Y)];
        // 2 and 3 are the pair of block 2; block 4 is single and is the child of 2.
        parts[1].pair_id = 7;
        parts[2].pair_id = 7;
        parts[3].rigid.as_mut().unwrap().parent = 2;
        link_twin(&mut parts, 1);
        assert_eq!(parts[2].rigid.as_ref().unwrap().parent, 1, "a single parent: the twin stands on it too");
        // A pair (4, 5) on the pair (2, 3): each stands on its own side.
        parts.push(linked(5, 0, Vec3::Y));
        parts[3].pair_id = 8;
        parts[4].pair_id = 8;
        link_twin(&mut parts, 3);
        assert_eq!(parts[4].rigid.as_ref().unwrap().parent, 3, "the twin of the parent");
        // A twin without an id gets one.
        parts[4].rigid.as_mut().unwrap().id = 0;
        link_twin(&mut parts, 3);
        assert_eq!(parts[4].rigid.as_ref().unwrap().id, 5);
        // Parentless: the twin is parentless.
        parts[3].rigid.as_mut().unwrap().parent = 0;
        link_twin(&mut parts, 3);
        assert_eq!(parts[4].rigid.as_ref().unwrap().parent, 0);
    }

    #[test]
    fn a_moved_block_takes_what_stands_on_it_along() {
        // FUN_0049F4D0: the child keeps its place in the parent's frame.
        let old = (Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
        let new = (Vec3::new(5.0, 0.0, 2.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let child = (Vec3::new(2.0, 1.0, 0.0), Quat::IDENTITY);
        let (p, q) = carried(old, new, child);
        // The child was 1 along +x and 1 up from the parent; a quarter turn about y sends +x to -z.
        assert!(near(p, Vec3::new(5.0, 1.0, 1.0)), "{p:?}");
        assert!((q * Vec3::X).dot(Vec3::NEG_Z) > 0.999);
        let mut parts = vec![linked(1, 0, old.0), linked(2, 1, child.0), linked(3, 2, Vec3::new(2.0, 2.0, 0.0)), linked(4, 0, Vec3::splat(9.0))];
        let before = poses_by_id(&parts);
        parts[0].rigid.as_mut().unwrap().position = new.0.to_array();
        parts[0].rigid.as_mut().unwrap().set_rotation(new.1);
        carry_descendants(&mut parts, &before, &[0]);
        let at = |i: usize| parts[i].rigid.as_ref().unwrap().position();
        assert!(near(at(1), Vec3::new(5.0, 1.0, 1.0)));
        assert!(near(at(2), Vec3::new(5.0, 2.0, 1.0)), "grandchildren follow too: {:?}", at(2));
        assert_eq!(at(3), Vec3::splat(9.0), "a block that does not stand on it stays");
        // Nothing moved: nothing carried.
        let before = poses_by_id(&parts);
        carry_descendants(&mut parts, &before, &[0]);
        assert!(near(parts[1].rigid.as_ref().unwrap().position(), Vec3::new(5.0, 1.0, 1.0)));
    }

    #[test]
    fn an_old_design_is_numbered_and_its_links_derived_once() {
        let rules: Vec<Option<Arc<StackRules>>> = vec![None, None, None];
        let wall = Arc::new(box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0)));
        let small = Arc::new(box_hull(Vec3::splat(-0.2), Vec3::splat(0.2)));
        let at = |p: Vec3| Transform::from_translation(p);
        let poses = vec![(at(Vec3::ZERO), Some(wall)), (at(Vec3::new(2.05, 1.5, 0.0)), Some(small.clone())), (at(Vec3::new(9.0, 1.5, 0.0)), Some(small))];
        let mut parts: Vec<PartInstance> = (0..3).map(|i| {
            let mut p = PartInstance::new(i, 0);
            p.rigid = Some(Default::default());
            p
        }).collect();
        assert!(legacy_links(&mut parts, &rules, &poses));
        assert_eq!(parts.iter().map(|p| p.rigid.as_ref().unwrap().id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(parent_table(&parts), vec![None, Some(0), None], "the window on the wall, the other floating");
        assert!(!legacy_links(&mut parts, &rules, &poses), "only once");
    }

    #[test]
    fn a_block_follows_the_camera_ray_on_the_studio_plane_and_stands_on_the_ground_under_it() {
        // Retail axes: z up, y forward. The camera looks forward and down: ray (0, 0.8, -0.6) from (0, -10, 8).
        let ray = (Vec3::new(0.0, -10.0, 8.0), Vec3::new(0.0, 0.8, -0.6));
        // A block that does not rest on the ground: the plane through its own origin, facing the camera.
        let block = Vec3::new(0.0, 2.0, 1.0);
        let plane = free_plane(ray.1, block, false, None, 20.0, false);
        assert_eq!(plane, FreePlane { point: block, normal: Vec3::new(0.0, -1.0, 0.0) });
        // The ray meets that plane at y = 2: z = 8 - 0.6 * (12 / 0.8) = -1 -> below the height that stands the block on
        // the ground (1.5 below its origin: origin at z = 1.5): the block stands on the ground at the ray's z = 1.5.
        let (at, ground) = free_position(ray.0, ray.1, &plane, block, 1.5);
        assert!(ground && (at.z - 1.5).abs() < 1e-5, "{at:?}");
        assert!((at.y - (-10.0 + 0.8 * ((8.0 - 1.5) / 0.6))).abs() < 1e-4, "on the ray: {at:?}");
        // A near plane the ray meets high above the ground: the block floats there.
        let near_plane = FreePlane { point: Vec3::new(0.0, -6.0, 0.0), normal: Vec3::new(0.0, -1.0, 0.0) };
        let (at, ground) = free_position(ray.0, ray.1, &near_plane, block, 1.5);
        assert!(!ground && near(at, Vec3::new(0.0, -6.0, 8.0 - 0.6 * 5.0)), "{at:?}");
        // A block resting on the ground takes the plane through the nearest other block, else half the bounds toward the camera.
        let plane = free_plane(ray.1, block, true, Some(Vec3::new(3.0, 4.0, 0.5)), 20.0, false);
        assert_eq!(plane.point, Vec3::new(3.0, 4.0, 0.5));
        let plane = free_plane(ray.1, block, true, None, 20.0, false);
        assert!(near(plane.point, Vec3::new(0.0, -8.0, 0.0)), "{:?}", plane.point);
        // Looking along the plane of symmetry: the plane of symmetry itself.
        let front = Vec3::new(0.1, 0.99, -0.1);
        assert_eq!(free_plane(front, block, false, None, 20.0, true), FreePlane { point: Vec3::ZERO, normal: Vec3::X });
        assert_ne!(free_plane(front, block, false, None, 20.0, false).normal, Vec3::X);
        // A ray straight down has no horizontal direction: the plane is empty and the block keeps its place unless
        // the ground rule moves it.
        let down = free_plane(Vec3::NEG_Z, block, false, None, 20.0, false);
        assert_eq!(down.normal, Vec3::ZERO);
        let (at, ground) = free_position(Vec3::new(4.0, 4.0, 9.0), Vec3::NEG_Z, &down, block, 1.5);
        assert!(ground && near(at, Vec3::new(4.0, 4.0, 1.5)), "the origin under the cursor at the standing height: {at:?}");
        // The nearest block is measured on the ground plane.
        let me = Vec3::new(0.0, 0.0, 5.0);
        assert_eq!(nearest_block_position(me, &[Vec3::new(3.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 30.0)]), Some(Vec3::new(1.0, 1.0, 30.0)));
        assert_eq!(nearest_block_position(me, &[]), None);
    }

    #[test]
    fn a_dropped_shape_stops_on_the_first_face_or_vertex_it_meets() {
        let others = |hulls: Vec<(Transform, Hull)>| -> Vec<OtherBlock> {
            hulls.into_iter().map(|(pose, hull)| OtherBlock { parent: None, pose, hull: Arc::new(hull), rules: None }).collect()
        };
        let table = others(vec![(Transform::IDENTITY, box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 1.0, 2.0)))]);
        let refs: Vec<&OtherBlock> = table.iter().collect();
        let part = box_hull(Vec3::splat(-0.5), Vec3::splat(0.5));
        let at = |p: Vec3| world_tris(&Transform::from_translation(p), &part);
        // A part above the table drops until its bottom meets the top: from y = 4 (bottom 3.5) onto y = 1.
        let t = sweep_down(&at(Vec3::new(0.0, 4.0, 0.0)), &refs, SWEEP_LENGTH).unwrap();
        assert!((t - 2.5).abs() < 1e-4, "{t}");
        // Half over the edge still lands on the table (the part's vertices on the table are the contact).
        let t = sweep_down(&at(Vec3::new(2.3, 4.0, 0.0)), &refs, SWEEP_LENGTH).unwrap();
        assert!((t - 2.5).abs() < 1e-4, "{t}");
        // Past the edge: nothing below.
        assert!(sweep_down(&at(Vec3::new(3.0, 4.0, 0.0)), &refs, SWEEP_LENGTH).is_none());
        // A pillar standing up into a wide part: the pillar's top vertices meet the part's bottom face
        // (nothing of the part is above the pillar's footprint, no vertex of the part is over the pillar).
        let pillar = others(vec![(Transform::IDENTITY, box_hull(Vec3::new(-0.1, 0.0, -0.1), Vec3::new(0.1, 3.0, 0.1)))]);
        let refs: Vec<&OtherBlock> = pillar.iter().collect();
        let wide = box_hull(Vec3::new(-2.0, -0.2, -2.0), Vec3::new(2.0, 0.2, 2.0));
        let tris = world_tris(&Transform::from_translation(Vec3::new(0.0, 6.0, 0.0)), &wide);
        let t = sweep_down(&tris, &refs, SWEEP_LENGTH).unwrap();
        assert!((t - 2.8).abs() < 1e-4, "bottom 5.8 onto the pillar top 3: {t}");
        // Out of reach.
        assert!(sweep_down(&at(Vec3::new(0.0, 300.0, 0.0)), &others(vec![(Transform::IDENTITY, box_hull(Vec3::splat(-1.0), Vec3::splat(1.0)))]).iter().collect::<Vec<_>>(), SWEEP_LENGTH).is_none());
    }

    #[test]
    fn the_height_rules_rest_a_part_on_the_listed_block_below_it() {
        let table = OtherBlock {
            parent: None,
            pose: Transform::IDENTITY,
            hull: Arc::new(box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 1.0, 2.0))),
            rules: Some(Arc::new(StackRules { block_type: key(1, 10, 0), ..default() })),
        };
        let part = box_hull(Vec3::splat(-0.5), Vec3::splat(0.5));
        let stair = StackRules { snap_down_to: vec![key(1, 10, 0)], ..default() };
        // Hovering 0.3 over the table (bottom at 1.3): snaps down onto it.
        let dy = height_correction(&stair, &Transform::from_translation(Vec3::new(0.0, 1.8, 0.0)), &part, std::slice::from_ref(&table), Some(0.0), false).unwrap();
        assert!((dy + 0.3).abs() < 1e-4, "{dy}");
        // Far above (more than 0.96): no snap.
        assert_eq!(height_correction(&stair, &Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)), &part, std::slice::from_ref(&table), Some(0.0), false), None);
    }

    #[test]
    fn snap_axes_hit_the_outline_of_the_part_at_the_pick_height() {
        let hull = box_hull(Vec3::new(-2.0, 0.0, -1.0), Vec3::new(2.0, 1.0, 1.0));
        let rules = StackRules { snap_axes: 4, snap_axis_pick: Some(0.75), ..default() };
        let pose = Transform::IDENTITY;
        let target = SnapTarget::of(&rules, &pose, &hull);
        assert_eq!(target.table.len(), 4);
        // Retail local: x = editor x, y = -editor z, z = editor y. The box is 4 wide (x), 2 deep (y), 1 high (z).
        let by_dir = |d: Vec3| target.table.iter().find(|e| (e.dir - d).length() < 1e-4).unwrap();
        assert!((by_dir(Vec3::X).point - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4, "{:?}", by_dir(Vec3::X));
        assert!((by_dir(Vec3::Y).point - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4);
        // The flag of the fourth axis is lost to the rounding of `angle += step` (3 x pi/2 is not a multiple
        // of the f32 quarter turn): retail's fmod test misses it too, the first three are cardinal.
        assert!(by_dir(Vec3::X).cardinal && by_dir(Vec3::Y).cardinal && by_dir(Vec3::NEG_X).cardinal && !by_dir(Vec3::NEG_Y).cardinal);
        assert!((target.diagonal - (16.0f32 + 4.0).sqrt()).abs() < 1e-4, "horizontal diagonal of the world box");
        // A part that has no snap axes builds nothing; a ray that misses empties the table.
        assert!(SnapTarget::of(&StackRules::default(), &pose, &hull).table.is_empty());
        assert!(build_snap_vectors(&rules, &[], Vec3::splat(-1.0), Vec3::splat(1.0)).is_empty());
        // The explicit list wins: three axes at 0, 120, 240 degrees; only 0 is a quarter turn.
        let tri = StackRules { snap_axes_list: vec![0.0, 120.0, 240.0], snap_axes: 4, ..default() };
        let angles = snap_axis_angles(&tri);
        assert_eq!(angles.len(), 3);
        assert!(angles[0].1 && !angles[1].1 && !angles[2].1);
        // modelSnapAxisRotation turns the equal steps.
        let turned = snap_axis_angles(&StackRules { snap_axes: 4, snap_axis_rotation: 45.0, ..default() });
        assert!((turned[0].0 - std::f32::consts::FRAC_PI_4).abs() < 1e-6 && !turned[0].1);
    }

    #[test]
    fn the_face_under_the_block_is_found_by_its_sector_and_the_block_slides_to_the_face_centre() {
        let hull = box_hull(Vec3::new(-2.0, 0.0, -1.0), Vec3::new(2.0, 1.0, 1.0));
        let rules = StackRules { snap_axes: 4, ..default() };
        let target = SnapTarget::of(&rules, &Transform::IDENTITY, &hull);
        // A point on the +x face a little off its centre (retail y = 0.15 along the wall).
        let q = Vec3::new(2.1, 0.15, 0.3);
        let idx = find_snap_vector(&target.table, target.center, q, -1.0).expect("a sector");
        assert!((target.table[idx].dir - Vec3::X).length() < 1e-4);
        // Within the tolerance (0.25 x 0.08 x 4.47 = 0.089): not at 0.15, yes at 0.05.
        let tol = target.tolerance(1.0) * VECTOR_SNAP_FRACTION;
        assert!(find_snap_vector(&target.table, target.center, q, tol).is_none());
        let q2 = Vec3::new(2.1, 0.05, 0.3);
        assert_eq!(find_snap_vector(&target.table, target.center, q2, tol), Some(idx));
        // The block slides along the wall onto the face's centre and keeps its distance and height.
        let w = snap_vector_position(&target.table, idx, q2);
        assert!((w - Vec3::new(2.1, 0.0, 0.3)).length() < 1e-4, "{w:?}");
        // A point beyond the corner belongs to the next face.
        let corner = find_snap_vector(&target.table, target.center, Vec3::new(1.0, 1.4, 0.0), -1.0).unwrap();
        assert!((target.table[corner].dir - Vec3::Y).length() < 1e-4);
        // The whole chain through a moved parent: position 10 along x, turned 90 degrees.
        let pose = Transform { translation: Vec3::new(10.0, 0.0, 0.0), rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), ..default() };
        let turned = SnapTarget::of(&rules, &pose, &hull);
        let back = turned.world(turned.local(Vec3::new(10.3, 0.4, 0.2)));
        assert!((back - Vec3::new(10.3, 0.4, 0.2)).length() < 1e-4, "{back:?}");
    }

    #[test]
    fn the_centre_snap_needs_the_top_and_the_vector_snap_the_attribute() {
        let hull = box_hull(Vec3::new(-2.0, 0.0, -1.0), Vec3::new(2.0, 1.0, 1.0));
        let roof = StackRules { block_type: key(1, 20, 0), snap_axes: 4, ..default() };
        let target = SnapTarget::of(&roof, &Transform::IDENTITY, &hull);
        let chimney = StackRules { snap_to_parent_center: true, snap_vectors: true, snap_to_parent: vec![key(1, 20, 0)], ..default() };
        // Near the centre and on top: onto the centre line (height kept).
        let p = Vec3::new(0.2, 1.0, 0.1);
        let top = try_snap_to_parent(&chimney, &roof, &target, p, 1.0, true).expect("centre");
        assert_eq!(top.vector, None);
        assert!((top.position - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4, "{:?}", top.position);
        // Not on top: no centre snap, the vector snap works instead when the block is near a face centre.
        let beside = Vec3::new(2.1, 0.5, 0.05);
        let wall = try_snap_to_parent(&chimney, &roof, &target, beside, 1.0, false).expect("vector");
        assert_eq!(wall.vector.map(|i| target.table[i].dir), Some(Vec3::X));
        assert!((wall.position - Vec3::new(2.1, 0.5, 0.0)).length() < 1e-4, "{:?}", wall.position);
        assert!(try_snap_to_parent(&chimney, &roof, &target, Vec3::new(0.2, 1.0, 0.1), 1.0, false).is_none(), "centre of the top without the on-top flag: no centre snap, no face");
        // A part without the attributes or whose parent type is not listed never snaps.
        let plain = StackRules { snap_to_parent: vec![key(1, 20, 0)], ..default() };
        assert!(try_snap_to_parent(&plain, &roof, &target, p, 1.0, true).is_none());
        let other_parent = StackRules { block_type: key(1, 99, 0), ..roof.clone() };
        assert!(try_snap_to_parent(&chimney, &other_parent, &target, p, 1.0, true).is_none());
        // The tolerance scales (centre snap only: a part without the vector attribute).
        let centre_only = StackRules { snap_vectors: false, ..chimney.clone() };
        assert!(try_snap_to_parent(&centre_only, &roof, &target, Vec3::new(0.5, 1.0, 0.0), 1.0, true).is_none());
        assert!(try_snap_to_parent(&centre_only, &roof, &target, Vec3::new(0.5, 1.0, 0.0), 2.0, true).is_some());
    }

    fn other(pose: Transform, hull: Hull, block_type: TypeKey) -> OtherBlock {
        OtherBlock { parent: None, pose, hull: Arc::new(hull), rules: Some(Arc::new(StackRules { block_type, ..default() })) }
    }

    #[test]
    fn a_door_snaps_down_to_the_ground_and_a_stair_within_reach_but_not_from_far() {
        let door_hull = box_hull(Vec3::new(-0.5, 0.0, -0.1), Vec3::new(0.5, 2.0, 0.1));
        let ground = key(0, crate::world_objects::name_id("ground"), 0);
        assert_eq!(ground_type(), ground);
        let door = StackRules { snap_down_to: vec![ground, key(1, 10, 0), key(1, 40, 0)], stay_above_ground: true, ..default() };
        // 0.5 above the floor: within 0.96, snaps down.
        let low = Transform::from_translation(Vec3::new(0.0, 0.5, 0.0));
        assert!((height_correction(&door, &low, &door_hull, &[], Some(0.0), false).unwrap() + 0.5).abs() < 1e-5);
        // 3 above the floor: too far.
        let high = Transform::from_translation(Vec3::new(0.0, 3.0, 0.0));
        assert_eq!(height_correction(&door, &high, &door_hull, &[], Some(0.0), false), None);
        // Below the ground: lifted (StayAbove) onto it.
        let sunk = Transform::from_translation(Vec3::new(0.0, -0.4, 0.0));
        assert!((height_correction(&door, &sunk, &door_hull, &[], Some(0.0), false).unwrap() - 0.4).abs() < 1e-5);
        // A stair (type 40) under the door: it rests on the stair's top, not the ground; another type is ignored.
        let stair_hull = || box_hull(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.3, 1.0));
        let stair = other(Transform::IDENTITY, stair_hull(), key(1, 40, 0));
        let up = Transform::from_translation(Vec3::new(0.0, 0.7, 0.0));
        let dy = height_correction(&door, &up, &door_hull, std::slice::from_ref(&stair), Some(0.0), false).unwrap();
        assert!((dy + 0.4).abs() < 1e-4, "bottom 0.7 -> stair top 0.3: {dy}");
        let balcony = other(Transform::IDENTITY, stair_hull(), key(1, 77, 0));
        let dy = height_correction(&door, &up, &door_hull, std::slice::from_ref(&balcony), Some(0.0), false).unwrap();
        assert!((dy + 0.7).abs() < 1e-4, "an unlisted block is not a snap target: {dy}");
        // A parentless block that stays above the ground goes exactly onto it.
        let free = Transform::from_translation(Vec3::new(0.0, 1.5, 0.0));
        assert!((height_correction(&door, &free, &door_hull, &[], Some(0.0), true).unwrap() + 1.5).abs() < 1e-5);
        // A window (StayAboveGround only) is lifted from below the ground but never snapped down.
        let window = StackRules { stay_above_ground: true, ..default() };
        assert_eq!(height_correction(&window, &low, &door_hull, &[], Some(0.0), false), None);
        assert!((height_correction(&window, &sunk, &door_hull, &[], Some(0.0), false).unwrap() - 0.4).abs() < 1e-5);
        // A part with neither rule is untouched.
        assert_eq!(height_correction(&StackRules::default(), &sunk, &door_hull, &[], Some(0.0), false), None);
    }

    #[test]
    fn the_dormer_window_is_pushed_back_until_its_bottom_edge_meets_the_roof() {
        // A wall-like roof block: its +x face at x = 0 (the part is mounted along retail +Y, here editor -Z...).
        let roof_hull = box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 2.0, 0.0));
        let roof = Transform::IDENTITY;
        let window_hull = box_hull(Vec3::new(-0.5, 0.0, -0.3), Vec3::new(0.5, 1.0, 0.3));
        let rules = StackRules { move_bottom_edge: true, bottom_edge_adjustment: 0.0, ..default() };
        // Mounted facing +Z (editor): retail row 1 = -Z mount axis -> retail +Y points into the roof (editor -Z).
        let rows = rows_of(Quat::IDENTITY);
        // The window's outer face edge (local y = min = -0.3 -> hull.min.z... ) sits 0.5 in front of the roof's face.
        let pos_front = Vec3::new(0.0, 0.5, 0.5);
        let moved = move_bottom_edge(&rules, rows, rows, pos_front, Vec3::ONE, &window_hull, &roof, &roof_hull, 0.0);
        assert!(moved.is_none(), "an edge in front of the surface is left alone: {moved:?}");
        // Sunk 0.4 into the roof: the edge is behind the surface, so the block moves back out by that depth.
        let pos_in = Vec3::new(0.0, 0.5, -0.4);
        let moved = move_bottom_edge(&rules, rows, rows, pos_in, Vec3::ONE, &window_hull, &roof, &roof_hull, 0.0).expect("pushed back");
        assert!(moved.z > pos_in.z && moved.x == pos_in.x, "{moved:?}");
        // Without the attribute nothing happens.
        assert!(move_bottom_edge(&StackRules::default(), rows, rows, pos_in, Vec3::ONE, &window_hull, &roof, &roof_hull, 0.0).is_none());
    }

    #[test]
    fn centre_snapping_pulls_within_eight_percent_of_the_diagonal() {
        let hull = Hull { tris: vec![], min: Vec3::new(-2.0, 0.0, -1.0), max: Vec3::new(2.0, 1.0, 1.0) };
        let pose = Transform::from_translation(Vec3::new(10.0, 0.0, 0.0));
        // Diagonal sqrt(4^2 + 2^2) = 4.472, tolerance 0.358.
        let near = Vec3::new(10.2, 1.0, 0.1);
        let (p, d) = snap_to_center(near, &pose, &hull, 1.0).expect("inside the tolerance");
        assert!((p - Vec3::new(10.0, 1.0, 0.0)).length() < 1e-5, "{p:?}");
        assert!((d - Vec2::new(0.2, 0.1).length()).abs() < 1e-5);
        assert!(snap_to_center(Vec3::new(10.5, 1.0, 0.0), &pose, &hull, 1.0).is_none(), "0.5 is outside 0.358");
        assert!(snap_to_center(Vec3::new(10.5, 1.0, 0.0), &pose, &hull, 2.0).is_some(), "the caller's scale widens it");
        // A turned parent measures in its own frame.
        let turned = Transform { rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), ..pose };
        let (p, _) = snap_to_center(Vec3::new(10.0, 1.0, 0.3), &turned, &hull, 1.0).expect("0.3 along its own x");
        assert!((p - Vec3::new(10.0, 1.0, 0.0)).length() < 1e-5, "{p:?}");
    }

    #[test]
    fn the_top_face_needs_a_flat_normal_near_the_parents_top() {
        let hull = Arc::new(box_hull(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0)));
        let target = Arc::new(SnapTarget::of(&StackRules::default(), &Transform::IDENTITY, &hull));
        let parent = ParentInput { rules: Arc::new(StackRules::default()), pose: Transform::IDENTITY, hull, target };
        let input = StackInput { parent: Some(parent), ..default() };
        let top = Surface { point: Vec3::new(0.0, 1.0, 0.0), normal: Vec3::Y, t: 1.0, block: Some(0) };
        assert!(input.on_top(&top));
        // A tilted roof face (normal 30 degrees off the vertical) is not the top.
        let tilted = Surface { normal: Vec3::new(0.5, 0.866, 0.0), ..top };
        assert!(!input.on_top(&tilted));
        // A flat face far under the top (a ledge) is not the top either.
        let ledge = Surface { point: Vec3::new(0.0, 0.2, 0.0), ..top };
        assert!(!input.on_top(&ledge));
        assert!(!StackInput::default().on_top(&top), "no parent");
    }

    #[test]
    fn the_side_is_the_sign_of_x_and_the_band_makes_it_the_plane() {
        let input = StackInput { env: StackEnv { symmetric: true, plane_band: 0.2, floor: None, bounds: 0.0 }, ..default() };
        assert_eq!((input.side_of(0.5), input.side_of(-0.5), input.side_of(0.1), input.side_of(-0.1)), (1, -1, 0, 0));
        let building = StackInput::default();
        assert_eq!((building.side_of(0.1), building.side_of(0.0005)), (1, 0), "no symmetry band without a symmetric editor");
    }

    fn block_at(position: Vec3, rows: Rows, centre: Vec3) -> AlignBlock {
        AlignBlock { position, rows, centre }
    }

    #[test]
    fn align_height_moves_a_block_to_the_nearest_centre_height_within_reach() {
        // Retail axes: z is up. This block's box centre is at height 1.0; two windows at 1.1 and 1.5.
        let this = block_at(Vec3::new(0.0, 0.0, 0.8), IDENTITY_ROWS, Vec3::new(0.0, 0.0, 1.0));
        let near = block_at(Vec3::new(2.0, 0.0, 0.9), IDENTITY_ROWS, Vec3::new(2.0, 0.0, 1.1));
        let far = block_at(Vec3::new(4.0, 0.0, 1.3), IDENTITY_ROWS, Vec3::new(4.0, 0.0, 1.5));
        // Reach 0.2 x 1.2 = 0.24: only the first is within it.
        let (position, distance) = align_height(&this, &[far, near], COMBINER_SCALE).expect("aligned");
        assert!((position - Vec3::new(0.0, 0.0, 0.9)).length() < 1e-5, "x and y stay, z moves by dz = 0.1: {position:?}");
        assert!((distance - 0.1 * (near.centre - this.centre).length()).abs() < 1e-5, "|dz| x |centre offset|: {distance}");
        assert!(align_height(&this, &[far], COMBINER_SCALE).is_none(), "0.5 is out of reach");
        assert!(align_height(&this, &[], COMBINER_SCALE).is_none());
        // The smaller distance wins: two in reach, the nearer one.
        let nearer = block_at(Vec3::new(1.0, 0.0, 0.85), IDENTITY_ROWS, Vec3::new(1.0, 0.0, 1.05));
        let (position, _) = align_height(&this, &[near, nearer], COMBINER_SCALE).unwrap();
        assert!((position.z - 0.85).abs() < 1e-5, "{position:?}");
    }

    #[test]
    fn align_lateral_slides_along_the_neighbours_x_axis_when_the_forward_axes_agree() {
        // Both blocks face along retail +Y (row 1); the neighbour stands 5 along x at y = 2 and this block is
        // 0.1 off its centre line along x: it moves onto the line (x -> 5) and keeps its y and height.
        let neighbour = block_at(Vec3::new(5.0, 2.0, 1.0), IDENTITY_ROWS, Vec3::new(5.0, 2.0, 1.5));
        let this = block_at(Vec3::new(5.1, 2.0, 0.5), IDENTITY_ROWS, Vec3::new(5.1, 2.0, 1.0));
        let (position, rows, distance) = align_lateral(&this, &[neighbour], COMBINER_SCALE).expect("aligned");
        assert!((position.x - 5.0).abs() < 1e-5 && (position.y - 2.0).abs() < 1e-5, "{position:?}");
        assert!(distance > 0.0 && (distance - 0.1 * (neighbour.centre - this.centre).length().max(0.0)).abs() < 0.1, "{distance}");
        assert!(rows[0].dot(Vec3::X) > 0.999, "parallel blocks keep their orientation: {rows:?}");
        // More than 0.24 off the line: nothing.
        let off = block_at(Vec3::new(5.4, 2.0, 0.5), IDENTITY_ROWS, Vec3::new(5.4, 2.0, 1.0));
        assert!(align_lateral(&off, &[neighbour], COMBINER_SCALE).is_none());
        // A neighbour that faces away (forward axes 90 degrees apart): nothing.
        let turned = [Vec3::NEG_Y, Vec3::X, Vec3::Z];
        let across = block_at(Vec3::new(5.0, 2.0, 1.0), turned, Vec3::new(5.0, 2.0, 1.5));
        assert!(align_lateral(&this, &[across], COMBINER_SCALE).is_none());
    }

    #[test]
    fn align_lateral_turns_a_nearly_parallel_block_about_the_up_axis_by_the_unsigned_angle() {
        // This block is yawed 10 degrees from the neighbour: retail turns it by acos(dot) = 10 degrees
        // counter-clockwise about up whichever side it was on (a quirk the code has).
        let yawed = |a: f32| {
            let q = Quat::from_axis_angle(UP, a);
            [q * Vec3::X, q * Vec3::Y, q * Vec3::Z]
        };
        let neighbour = block_at(Vec3::new(0.0, 0.0, 1.0), IDENTITY_ROWS, Vec3::new(0.0, 0.0, 1.0));
        let ten = 10f32.to_radians();
        let ccw = block_at(Vec3::new(0.0, 3.0, 1.0), yawed(ten), Vec3::new(0.0, 3.0, 1.0));
        let (_, rows, _) = align_lateral(&ccw, &[neighbour], COMBINER_SCALE).expect("aligned");
        // The matrix goes through FUN_00490B90 whose row 0 follows the rotated x axis: 20 degrees from X.
        let angle = rows[0].dot(Vec3::X).clamp(-1.0, 1.0).acos();
        assert!((angle - 2.0 * ten).abs() < 0.02, "a block already yawed counter-clockwise ends up twice as far: {}", angle.to_degrees());
        let cw = block_at(Vec3::new(0.0, 3.0, 1.0), yawed(-ten), Vec3::new(0.0, 3.0, 1.0));
        let (_, rows, _) = align_lateral(&cw, &[neighbour], COMBINER_SCALE).expect("aligned");
        assert!(rows[0].dot(Vec3::X) > 0.9999, "a block yawed clockwise lands on the neighbour's heading: {rows:?}");
    }

    #[test]
    fn align_xy_works_in_the_parents_frame_and_the_editor_centre_wins() {
        let sibling = block_at(Vec3::new(3.0, 4.0, 0.0), IDENTITY_ROWS, Vec3::new(3.0, 4.0, 0.0));
        let this = block_at(Vec3::new(3.2, 1.0, 0.5), IDENTITY_ROWS, Vec3::new(3.2, 1.0, 0.5));
        let rule = XyRule { use_list: true, centre_radius: None };
        let xy_scale = COMBINER_SCALE * COMBINER_SCALE;
        // Parentless (world frame): x is within 0.2 x 1.44 = 0.288 (0.2), y is 3 away: x aligns, y stays.
        let (position, distance) = align_xy(&this, &[sibling], None, rule, xy_scale).expect("aligned");
        assert!((position - Vec3::new(3.0, 1.0, 0.5)).length() < 1e-5, "{position:?}");
        assert!((distance - 0.2 * (sibling.position - this.position).length()).abs() < 1e-4, "{distance}");
        // In a parent's frame turned 90 degrees about up (its x axis = world y): the aligned local axis is the
        // parent's, the move is along the parent's x = world +y... a sibling 0.1 off in the parent's x.
        let parent = (Vec3::new(10.0, 0.0, 0.0), [Vec3::Y, Vec3::NEG_X, Vec3::Z]);
        let sib = block_at(Vec3::new(10.0, 4.0, 0.0), IDENTITY_ROWS, Vec3::new(10.0, 4.0, 0.0));
        let me = block_at(Vec3::new(9.0, 4.1, 0.0), IDENTITY_ROWS, Vec3::new(9.0, 4.1, 0.0));
        let (position, _) = align_xy(&me, &[sib], Some(parent), rule, xy_scale).expect("aligned in the parent frame");
        assert!((position.y - 4.0).abs() < 1e-5 && (position.x - 9.0).abs() < 1e-5, "local x = world y moved 0.1 back: {position:?}");
        // Nothing within reach: nothing.
        let far = block_at(Vec3::new(30.0, 40.0, 0.0), IDENTITY_ROWS, Vec3::ZERO);
        assert!(align_xy(&this, &[far], None, rule, xy_scale).is_none());
        // Centre of the editor: a parentless block within the radius goes onto the centre line, its distance
        // is |z| (retail builds a vector with x and y zeroed and measures that).
        let centred = XyRule { use_list: true, centre_radius: Some(0.04 * 20.0 * xy_scale) };
        let near_centre = block_at(Vec3::new(0.6, -0.5, 1.5), IDENTITY_ROWS, Vec3::new(0.6, -0.5, 1.5));
        let (position, distance) = align_xy(&near_centre, &[sibling], None, centred, xy_scale).expect("centred");
        assert!((position - Vec3::new(0.0, 0.0, 1.5)).length() < 1e-6 && (distance - 1.5).abs() < 1e-6, "{position:?} {distance}");
        // Outside the radius (1.152): the sibling rule still applies.
        let outside = block_at(Vec3::new(3.2, -3.0, 1.5), IDENTITY_ROWS, Vec3::ZERO);
        assert!(align_xy(&outside, &[sibling], None, centred, xy_scale).is_some_and(|(p, _)| (p.x - 3.0).abs() < 1e-5));
    }

    #[test]
    fn the_combiner_takes_z_from_the_nearer_group_and_x_y_from_the_snap_lateral_or_xy_candidates() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        let base = Candidates { position: p, ..default() };
        assert_eq!(combine(&base).distance, None, "no candidate: retail returns -1 and keeps the position");
        assert_eq!(combine(&base).position, p);
        // z: stay above (0.4) and align height (0.2): the nearer wins; a tie goes to the stay rule.
        let stay = Some((0.5, 0.4));
        let height = Some((Vec3::new(9.0, 9.0, 3.2), 0.2));
        assert_eq!(combine(&Candidates { stay, height, ..base }).position.z, 3.2);
        let tie = Some((Vec3::new(9.0, 9.0, 3.2), 0.4));
        assert_eq!(combine(&Candidates { stay, height: tie, ..base }).position.z, 3.5, "a tie goes to the stay above rule");
        assert_eq!(combine(&Candidates { stay, ..base }).position, Vec3::new(1.0, 2.0, 3.5));
        // x, y: the snap alone, lateral alone, both (nearer, tie to the snap), the XY align under them.
        let snap = Some((Vec3::new(5.0, 6.0, 7.0), 0.3));
        let lateral = Some((Vec3::new(8.0, 9.0, 7.0), IDENTITY_ROWS, 0.1));
        let xy = Some((Vec3::new(-1.0, -2.0, 7.0), 0.7));
        let c = combine(&Candidates { snap, ..base });
        assert_eq!((c.position, c.orientation, c.snap_type_set), (Vec3::new(5.0, 6.0, 3.0), Orientation::Snap, true));
        let c = combine(&Candidates { lateral, ..base });
        assert_eq!((c.position, c.orientation), (Vec3::new(8.0, 9.0, 3.0), Orientation::Lateral));
        let c = combine(&Candidates { snap, lateral, ..base });
        assert_eq!((c.position.x, c.orientation), (8.0, Orientation::Lateral), "the lateral align is nearer");
        let c = combine(&Candidates { snap: Some((Vec3::new(5.0, 6.0, 7.0), 0.1)), lateral, ..base });
        assert_eq!((c.position.x, c.orientation, c.snap_type_set), (5.0, Orientation::Snap, false), "a tie goes to the snap, whose type is then not set");
        let c = combine(&Candidates { xy, ..base });
        assert_eq!((c.position, c.orientation), (Vec3::new(-1.0, -2.0, 3.0), Orientation::Keep));
        let c = combine(&Candidates { xy, lateral, ..base });
        assert_eq!(c.position.x, 8.0, "the lateral align replaces the XY align");
        // The distance sums the two groups.
        let c = combine(&Candidates { stay, snap, ..base });
        assert!((c.distance.unwrap() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn a_block_that_is_not_allowed_out_of_bounds_stays_inside_half_the_bound_size() {
        let inside = Vec3::new(2.0, 5.0, -1.0);
        assert_eq!(clamp_to_bounds(inside, 6.0), inside);
        let out = clamp_to_bounds(Vec3::new(6.0, 5.0, 0.0), 6.0);
        assert!((out - Vec3::new(3.0, 5.0, 0.0)).length() < 1e-6, "{out:?}");
        let diagonal = clamp_to_bounds(Vec3::new(8.0, 1.0, 8.0), 20.0);
        assert!((Vec2::new(diagonal.x, diagonal.z).length() - 10.0).abs() < 1e-4 && diagonal.y == 1.0);
        assert_eq!(clamp_to_bounds(Vec3::new(50.0, 0.0, 0.0), 0.0), Vec3::new(50.0, 0.0, 0.0), "unknown bounds: unlimited");
    }

    #[test]
    fn parents_are_found_by_the_surface_under_the_origin_and_the_root_check_walks_them() {
        // A structure (may be parentless) with a window on its +x wall (origin 0.1 off it) and a door far from anything.
        let wall = box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0));
        let structure = Arc::new(StackRules { block_type: key(1, 10, 0), can_be_parentless: true, ..default() });
        let window = Arc::new(StackRules { block_type: key(1, 50, 0), ..default() });
        let rules = vec![Some(structure), Some(window.clone()), Some(window)];
        let wall_hull = Arc::new(wall);
        let small = Arc::new(box_hull(Vec3::splat(-0.2), Vec3::splat(0.2)));
        let poses = vec![
            (Transform::IDENTITY, Some(wall_hull)),
            (Transform::from_translation(Vec3::new(2.1, 1.5, 0.0)), Some(small.clone())),
            (Transform::from_translation(Vec3::new(8.0, 1.5, 0.0)), Some(small)),
        ];
        assert_eq!(derive_parent(1, &rules, &poses, &[]), Some(0), "0.1 from the wall");
        assert_eq!(derive_parent(2, &rules, &poses, &[]), None, "floating in the air");
        assert_eq!(derive_parent(0, &rules, &poses, &[]), None);
        let parent_of = |i: usize| derive_parent(i, &rules, &poses, &[]);
        assert!(connected_to_root(0, &rules, &parent_of), "a structure may stand alone");
        assert!(connected_to_root(1, &rules, &parent_of), "a window on a structure is attached");
        assert!(!connected_to_root(2, &rules, &parent_of), "a window in the air is not");
        // A part that is not an original definition is always valid.
        assert!(connected_to_root(0, &[None], &|_| None));
        // A block it cannot interact with is not a parent.
        let only_roofs = Arc::new(StackRules { block_type: key(1, 50, 0), interact_with: vec![key(1, 20, 0)], ..default() });
        let rules2 = vec![rules[0].clone(), Some(only_roofs), None];
        assert_eq!(derive_parent(1, &rules2, &poses, &[]), None);
        // The twin is skipped.
        assert_eq!(derive_parent(1, &rules, &poses, &[0]), None);
    }

    #[test]
    fn align_siblings_are_the_blocks_of_the_same_parent_whose_type_the_list_names() {
        let hull = Arc::new(box_hull(Vec3::new(-0.5, 0.0, -0.1), Vec3::new(0.5, 1.0, 0.1)));
        let window_type = key(1, 50, 0);
        let mine = Arc::new(StackRules { block_type: window_type, align_height: vec![window_type], ..default() });
        let other_rules = Some(Arc::new(StackRules { block_type: window_type, ..default() }));
        // Editor frame: y is up. Three windows with centre heights 1.1 (parent 0), 1.05 (parent 1), 1.0 (parentless).
        let at = |y: f32| Transform::from_translation(Vec3::new(2.0, y - 0.5, 0.0));
        let block = |parent, y| OtherBlock { parent, pose: at(y), hull: hull.clone(), rules: other_rules.clone() };
        let input = StackInput { rules: Some(mine), others: vec![block(Some(0), 1.1), block(Some(1), 1.05), block(None, 1.0)], ..default() };
        let this = AlignBlock::of(&Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)), &hull);
        assert!((this.centre.z - 1.0).abs() < 1e-5, "retail z is the editor's y: {:?}", this.centre);
        let on_parent_0 = input.align(&this, Some(0));
        assert!(on_parent_0.height.is_some_and(|(p, _)| (p.z - (this.position.z + 0.1)).abs() < 1e-5), "{on_parent_0:?}");
        let on_parent_1 = input.align(&this, Some(1));
        assert!(on_parent_1.height.is_some_and(|(p, _)| (p.z - (this.position.z + 0.05)).abs() < 1e-5), "{on_parent_1:?}");
        let parentless = input.align(&this, None);
        assert!(parentless.height.is_some_and(|(p, _)| (p.z - this.position.z).abs() < 1e-5), "{parentless:?}");
        assert!(input.align(&this, Some(7)).height.is_none(), "no sibling on another parent");
        assert!(on_parent_0.lateral.is_none() && on_parent_0.xy.is_none(), "the lists of the other two rules are empty");
    }

    #[test]
    fn the_snap_replace_test_needs_the_listed_type_and_the_distance_within_either_radius() {
        let jaw = StackRules { block_type: key(2, 10, 0), snap_replace: vec![key(2, 10, 0)], replace_snap_delta: 0.4, ..default() };
        let other = StackRules { block_type: key(2, 10, 0), replace_snap_delta: 0.4, ..default() };
        let unlisted = StackRules { block_type: key(2, 11, 0), ..default() };
        // radius = clamp(width x 0.4, 0.01, 0.3) x size: width 1.0 -> 0.3 (the clamp), width 0.2 -> 0.08.
        assert!((replace_radius(&jaw, 1.0, 2.0) - 0.6).abs() < 1e-6 && (replace_radius(&jaw, 0.2, 1.0) - 0.08).abs() < 1e-6);
        assert!((replace_radius(&jaw, 0.0, 1.0) - 0.01).abs() < 1e-6, "lower clamp");
        assert!(should_snap_replace(&jaw, &other, 0.25, 0.3, 0.08));
        assert!(should_snap_replace(&jaw, &other, 0.05, 0.01, 0.08), "within the other block's radius");
        assert!(!should_snap_replace(&jaw, &other, 0.5, 0.3, 0.08), "too far from both");
        assert!(!should_snap_replace(&jaw, &unlisted, 0.01, 0.3, 0.3), "the type is not listed");
        assert!(!should_snap_replace(&other, &jaw, 0.01, 0.3, 0.3), "an empty list replaces nothing");
    }

    /// Print the snap tables and flags of the installed building and vehicle parts:
    /// `cargo test rigid_editor_stack::tests::installed_part_rules -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_part_rules() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        for (group, label) in [(crate::asset_catalog::BUILDING_PARTS, "building"), (crate::asset_catalog::VEHICLE_PARTS, "vehicle")] {
            let (mut parts, mut centre, mut upright, mut typed, mut interact) = (0, 0, 0, 0, 0);
            let (mut forward, mut down, mut above, mut edge, mut axes, mut vectors, mut circular) = (0, 0, 0, 0, 0, 0, 0);
            let (mut lateral, mut height, mut xy, mut replace, mut editor_centre, mut bounded, mut parentless) = (0, 0, 0, 0, 0, 0, 0);
            for def in catalog.parts.iter().filter(|d| d.key.group_id == group && !d.is_null) {
                let r = StackRules::load(&index, def.key);
                parts += 1;
                centre += r.snap_to_parent_center as usize;
                upright += (r.remain_upright || r.remain_upright_on_top) as usize;
                typed += (r.block_type != TypeKey::default()) as usize;
                interact += (!r.interact_with.is_empty() || !r.not_interact_with.is_empty()) as usize;
                forward += r.point_forward as usize;
                down += r.has_snap_down() as usize;
                above += r.has_stay_above() as usize;
                edge += r.move_bottom_edge as usize;
                axes += (r.snap_axis_count() > 0) as usize;
                vectors += r.snap_vectors as usize;
                circular += r.circular_top as usize;
                lateral += !r.align_lateral.is_empty() as usize;
                height += !r.align_height.is_empty() as usize;
                xy += !r.align_xy.is_empty() as usize;
                replace += !r.snap_replace.is_empty() as usize;
                editor_centre += r.snap_to_editor_centre as usize;
                bounded += !r.allowed_out_of_bounds as usize;
                parentless += r.can_be_parentless as usize;
            }
            println!(
                "{label}: {parts} parts, {typed} typed, {interact} with interaction lists, {centre} snap to centre, {upright} stay upright, \
                 {forward} point forward, {down} snap down, {above} stay above, {edge} move bottom edge, {axes} with snap axes, {vectors} snap to vectors, {circular} circular top"
            );
            println!(
                "{label}: align lateral {lateral}, align height {height}, align XY {xy}, snap replace {replace}, snap to editor centre {editor_centre},                  clamped to the bounds {bounded}, may be parentless {parentless}"
            );
            assert!(typed > 0, "{label}: rigblock types are read");
        }
    }
}
