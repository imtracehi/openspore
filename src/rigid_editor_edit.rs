//! Editing of rigid blocks (`rigid_editor`): the retail manipulators of the
//! vehicle, building and UFO editors on the blocks of `run.parts`.
//!
//! Retail (`notes/rigid-editors-notes.md`, `notes/creator-re-notes.md` 2):
//! * a palette press buys the part (the held press carries it); the part
//!   sticks to the surface of the block under the cursor (Pinning,
//!   `modelPinningType`) or stacks on the studio base (Stacking /
//!   PlanarInterpenetration); parts with `modelOrientToSurfaces` turn their
//!   mount axis (+Z of the part's own space: the origin sits on the surface,
//!   the part stands out toward +Z) along the surface normal, the others rest
//!   their bounding box on it;
//! * `editorUseSymmetry`: on the plane of symmetry (within the snap band) a
//!   block is single and centred, off it a mirrored twin follows for the same
//!   price (one refund, one undo entry); the buildings do not use symmetry;
//! * the wheel over a block scales it by `2^(0.075 n)` within its
//!   `modelMinScale`..`modelMaxScale`; one undo entry per wheel operation;
//! * dropping a carried block on the palette cancels the purchase, dropping a
//!   placed block there sells it;
//! * the rotation ring and the deform handles of the selected block turn and
//!   deform it (`HandleTuning` colours). A part deforms through its own
//!   morph handles (`modelHandleKeys`, the `DeformHandle` manipulator, the
//!   weight rule `FUN_005ACFC0` of the creature handles): a sphere sits on the
//!   handle's line at its weight and the drag moves the weight, which morphs
//!   the part's mesh (`RigidBlock::handles`). The handles are the retail
//!   models of `HandleTuning` (`creature_handles`): the teardrop
//!   `CE_basic_ui_teardrop` (+ `_overdraw`, scale 1.3, 1.2 for the buildings'
//!   `BuildingDeformHandle`) turned along the handle line, the ring
//!   `ui_rotateringcreature`, picked against their triangles.
//!
//! * the stacking rules of `rigid_editor_stack` (retail `EditorRigblock`
//!   rules, decoded in `notes/editors-stacking-notes.md`, sections 1-7): the pin
//!   ray skips the blocks the part cannot interact with
//!   (`modelTypesToInteractWith`, `modelTypesNotToInteractWith`); after the
//!   surface orientation `FUN_00494760` applies the on-top / snap vector
//!   orientation, `RemainUpright` / `RemainUprightOnTop`, `PointForward` (the
//!   vehicle parts: the part's x axis along the forward axis, the left half the
//!   other way) and the plane-of-symmetry orientation; a part with
//!   `modelSnapToParentCenter` snaps onto the parent's centre line when on top of
//!   it and within 8 % of its diagonal, one with `modelSnapToParentSnapVectors`
//!   slides along the nearest face of the parent's snap axes to the face's centre;
//!   `MoveBottomEdgeToSurface` (the dormers) and the height rules (`StayAbove`,
//!   `SnapDownTo`: doors and stairs drop onto the ground within 0.96) follow;
//!   the whole combiner `FUN_00490F20` runs: the Align height / lateral / XY rules
//!   (windows, doors, stairs and balconies take their neighbours' height and centre
//!   line, structures and roofs align with each other and snap to the editor's centre
//!   line), a block not allowed out of bounds stays inside a circle of radius
//!   `editorBoundSize / 2`, and the release of a block without a root (neither it nor
//!   an ancestor has `modelCanBeParentless`) is refused.
//!
//! * parents: a dragged block stands on the block the pin ray hits (`RigidBlock::parent`, stored with the design and in
//!   the creation files); the blocks standing on it follow its moves and turns; a ray that hits no block leaves
//!   it parentless and its origin follows the ray on the studio base's plane (`free_pose`: `FUN_0049BDC0` /
//!   `FUN_0049C1D0`), on the ground when the ray point at the standing height is above the plane point. Selling a block
//!   leaves its children parentless (retail's choice was not read); a scale or stretch of a block does not move them.
//!
//! ADAPTED: the response to a
//! bad location is to undo the drag; the Align / snap rules still run for a parentless block; the handles keep a constant size on
//! screen (the scale the retail editors pass to `SetScale` was not decoded:
//! the tuning scale times the view distance over 5, the creature editor's
//! zoom); a part without morph handles keeps six axis stretch handles
//! (`ce_simple_sphere`; retail has no deform handle on it, row 1 of
//! `editors-audit-b.md`); a morph weight is kept to 1/32 steps while dragged
//! (one morphed mesh per step); the key `R` turns the selected block about
//! the vertical axis (retail has the ring handle only).

use std::sync::Arc;

use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
};

use crate::{
    creature_editor::{refusal, scale_part, CreatureEditor, Mode},
    creature_editor_data::CreatureEditorData,
    creature_editor_edit::{ray_triangle, snap_delta},
    creature_handles::{
        deform_handle_rotation, deform_weight, handle_look, load_handle_models, load_part_handles, HandleLook, HandleModel, HandleModels, OverdrawMaterial,
        PartHandles, BUILDING_DEFORM_SCALE, DEFORM_SCALE, STATE_ALPHA, STATE_OVERDRAW_ALPHA,
    },
    editor::{Action, EditorState},
    game::{PartInstance, RunProgress},
    rigid_editor::*,
    rigid_editor_stack::{
        align_to_snap_vector, apply_orientation_rules, carry_descendants, clamp_to_bounds, combine, connected_to_root, descendants, ensure_id, free_plane, free_position, from_retail,
        height_correction, legacy_links, link_twin, move_bottom_edge, nearest_block_position, next_id, parent_of_table, parent_table, pin_allows, poses_by_id, prune_links,
        remain_upright_rows, rotation_of, rows_of, to_retail, try_snap_to_parent, twins_of, world_box, AlignBlock, Candidates, FreePlane, FreePose, OrientPlan,
        Orientation, StackEnv, StackInput, StackRules, COMBINER_SCALE,
    },
    Originals,
};

/// Retail handle tuning (`HandleTuning!defaultspinehandle`, dump
/// `handle_tuning_10CB4FA8.txt`).
pub const HANDLE_COLOR: [f32; 3] = [0.668, 0.747, 1.2];
pub const HANDLE_ROLLOVER: [f32; 3] = [0.541, 0.808, 0.541];
pub const HANDLE_ACTIVE: [f32; 3] = [1.0, 1.2, 0.416];
/// `editorMouseWheelTimeout` / distance defaults.
pub const WHEEL_TIMEOUT: f32 = 0.6;
pub const WHEEL_DISTANCE: f32 = 10.0;
/// Key turn step of `R` (ADAPTED).
pub const KEY_TURN: f32 = std::f32::consts::PI / 12.0;
/// The retail creature editor's camera zoom (`EditorCameraCreatureUI`): the
/// distance at which the tuning scales are the handles' size on screen.
const REFERENCE_ZOOM: f32 = 5.0;
/// ADAPTED: pick distance of the stretch / ring handles in screen pixels when
/// the handle models are not installed (the models are picked by their triangles).
const HANDLE_PICK_PX: f32 = 16.0;
/// ADAPTED: a dragged morph weight moves in steps of 1/32 (each weight is one
/// morphed mesh, built by `rigid_editor::prepare_morphs`).
const MORPH_STEPS: f32 = 32.0;

// ---------------------------------------------------------------------------
// Pure placement
// ---------------------------------------------------------------------------

/// A surface point and its outward normal (facing the viewer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub point: Vec3,
    pub normal: Vec3,
    /// Distance along the ray.
    pub t: f32,
    /// The block hit (None = the studio base).
    pub block: Option<usize>,
}

/// Nearest hit of a ray (in the preview's space) on the blocks and on the
/// studio base (the plane y = `floor`). `skip` blocks (the one being moved
/// and its twin) are not hit.
pub fn surface_under(
    origin: Vec3,
    dir: Vec3,
    blocks: &[(Transform, Option<Arc<Hull>>)],
    skip: &[usize],
    floor: Option<f32>,
) -> Option<Surface> {
    surface_under_where(origin, dir, blocks, skip, floor, &|_| true)
}

/// `surface_under` for a dragged part: a block the part cannot interact with
/// (`rigid_editor_stack::pin_allows`, retail `FUN_004A5270`) is not hit, the
/// ray passes through it.
pub fn surface_under_where(
    origin: Vec3,
    dir: Vec3,
    blocks: &[(Transform, Option<Arc<Hull>>)],
    skip: &[usize],
    floor: Option<f32>,
    allow: &dyn Fn(usize) -> bool,
) -> Option<Surface> {
    let mut best: Option<Surface> = None;
    for (i, (t, hull)) in blocks.iter().enumerate() {
        if skip.contains(&i) || !allow(i) {
            continue;
        }
        let Some(hull) = hull else { continue };
        if let Some((d, n)) = ray_block(origin, dir, t, hull) {
            if best.is_none_or(|b| d < b.t) {
                best = Some(Surface { point: origin + dir * d, normal: n, t: d, block: Some(i) });
            }
        }
    }
    if let Some(floor) = floor.filter(|_| dir.y.abs() > 1e-6) {
        let d = (floor - origin.y) / dir.y;
        if d > 0.0 && best.is_none_or(|b| d < b.t) {
            best = Some(Surface { point: origin + dir * d, normal: Vec3::Y, t: d, block: None });
        }
    }
    best
}

/// Nearest hit of a ray on one block: (distance along the ray, world normal
/// facing the viewer). The ray is moved into the block's own space, so the
/// block's triangles are tested untransformed.
pub fn ray_block(origin: Vec3, dir: Vec3, t: &Transform, hull: &Hull) -> Option<(f32, Vec3)> {
    let affine = t.compute_affine();
    if affine.matrix3.determinant().abs() < 1e-12 {
        return None;
    }
    let inverse = affine.inverse();
    let o = inverse.transform_point3(origin);
    let d = inverse.transform_vector3(dir);
    let mut best: Option<(f32, usize)> = None;
    for (i, tri) in hull.tris.iter().enumerate() {
        if let Some(h) = ray_triangle(o, d, tri[0], tri[1], tri[2]) {
            if best.is_none_or(|(b, _)| h < b) {
                best = Some((h, i));
            }
        }
    }
    let (h, i) = best?;
    let tri = hull.tris[i].map(|v| affine.transform_point3(v));
    let mut n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
    if n.dot(dir) > 0.0 {
        n = -n;
    }
    Some((h, n))
}

/// Half the extent of a block's oriented box along `dir` (world).
pub fn support(rotation: Quat, scale: Vec3, half: Vec3, dir: Vec3) -> f32 {
    let d = rotation.conjugate() * dir;
    d.x.abs() * scale.x.abs() * half.x + d.y.abs() * scale.y.abs() * half.y + d.z.abs() * scale.z.abs() * half.z
}

/// What the placement needs to know of the block being moved.
#[derive(Clone, Copy, Debug)]
pub struct Moving<'a> {
    pub block: &'a RigidBlock,
    /// Uniform size of the part's own units (`editor_unit * scale`).
    pub unit: f32,
    pub hull_center: Vec3,
    pub hull_half: Vec3,
    /// `modelOrientToSurfaces`.
    pub orient: bool,
    /// `RemainUpright` / `RemainUprightOnTop` applies on this surface
    /// (`rigid_editor_stack::StackInput::upright`); used when `plan` has no parent.
    pub upright: bool,
    /// The orientation rules of `FUN_00494760` for this surface
    /// (`rigid_editor_stack::StackInput::plan`); they need a parent block.
    pub plan: OrientPlan,
    /// The rules' input rows when they are not the surface orientation (the second
    /// `FUN_00494760` pass takes the combiner's output).
    pub input: Option<[Vec3; 3]>,
    /// Vertical limits of the block's bounding box (`editorMinHeight`,
    /// `editorMaxHeight`).
    pub min_y: f32,
    pub max_y: f32,
}

/// The block's rotation when it rests on `surface`: an oriented part turns its
/// mount axis (+Z of the part's own space) along the normal, the others keep theirs.
pub fn surface_rotation(surface: &Surface, m: &Moving) -> Quat {
    let rotation = m.block.rotation();
    if m.orient {
        let from = rotation * Vec3::Z;
        if from.dot(surface.normal) < 0.9999 {
            return Quat::from_rotation_arc(from, surface.normal.normalize_or(Vec3::Y)) * rotation;
        }
    }
    rotation
}

/// [`surface_rotation`] followed by the orientation rules of `FUN_00494760`
/// (`rigid_editor_stack::apply_orientation_rules`: the on-top / snap vector
/// orientation, `RemainUpright`, `PointForward`, the plane of symmetry); the rules
/// need a parent (`plan.has_parent`), without one only the legacy `upright` flag runs.
pub fn pin_rotation(surface: &Surface, m: &Moving) -> Quat {
    let rotation = surface_rotation(surface, m);
    if m.plan.has_parent {
        let rows = m.input.unwrap_or_else(|| rows_of(rotation));
        let out = apply_orientation_rules(rows, &m.plan);
        return if out.modified || m.input.is_some() { rotation_of(out.rows) } else { rotation };
    }
    if m.upright {
        return rotation_of(remain_upright_rows(rows_of(rotation)));
    }
    rotation
}

/// Keep the box within the vertical limits.
pub fn limit_height(mut position: Vec3, rotation: Quat, scale: Vec3, m: &Moving) -> Vec3 {
    let centre_y = position.y + (rotation * (scale * m.hull_center)).y;
    let half_y = support(rotation, scale, m.hull_half, Vec3::Y);
    if centre_y - half_y < m.min_y {
        position.y += m.min_y - (centre_y - half_y);
    } else if centre_y + half_y > m.max_y {
        position.y -= centre_y + half_y - m.max_y;
    }
    position
}

/// The block's pose when it rests on `surface`: an oriented part stands out of
/// the surface with its origin on it (its +Z along the normal), the others rest
/// their bounding box on it, both after the orientation rules. The vertical
/// limits are kept.
pub fn pin(surface: &Surface, m: &Moving) -> RigidBlock {
    let mut block = m.block.clone();
    let rotation = pin_rotation(surface, m);
    let mut scale = block.stretch() * m.unit;
    if block.mirrored {
        scale.x = -scale.x;
    }
    let position = if m.orient {
        surface.point
    } else {
        let s = support(rotation, scale, m.hull_half, surface.normal);
        surface.point + surface.normal * s - rotation * (scale * m.hull_center)
    };
    block.position = limit_height(position, rotation, scale, m).to_array();
    block.set_rotation(rotation);
    block
}

/// The twin rule after block `index` moved (`creature_editor_edit::settle`):
/// within `snap` of the plane x = 0 a block is single and centred (its twin
/// goes), off it a mirrored twin follows for the same price. Returns the
/// block's index afterwards (a removed twin may come before it).
pub fn settle(run: &mut RunProgress, index: usize, symmetry: bool, snap: f32) -> usize {
    let Some(part) = run.parts.get(index).cloned() else {
        return index;
    };
    let mut part = part;
    let mut block = part.rigid.clone().unwrap_or_default();
    // The block in its primary (not reflected) form.
    // (A reflection is a new block for `RigidBlock::reflected`: the pair's own id and parent stay.)
    let primary = |b: &RigidBlock, mirrored: bool| {
        if mirrored {
            RigidBlock { id: b.id, parent: b.parent, ..b.reflected() }
        } else {
            b.clone()
        }
    };
    let mut index = index;
    if symmetry && block.position().x.abs() <= snap {
        let mut b = primary(&block, part.mirrored);
        b.position[0] = 0.0;
        b.mirrored = false;
        b.pair = 0;
        part.mirrored = false;
        part.rigid = Some(b);
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
            prune_links(&mut run.parts);
        }
        run.parts[index] = part;
    } else if symmetry {
        if part.pair_id == 0 {
            part.pair_id = run.parts.iter().map(|p| p.pair_id).max().unwrap_or(0).saturating_add(1);
        }
        block.pair = part.pair_id;
        block.mirrored = part.mirrored;
        part.rigid = Some(block.clone());
        let twin = PartInstance {
            mirrored: !part.mirrored,
            rigid: Some(block.reflected()),
            ..part.clone()
        };
        let pair = part.pair_id;
        run.parts[index] = part;
        let mut found = false;
        for (i, p) in run.parts.iter_mut().enumerate() {
            if i != index && p.pair_id == pair {
                let (id, parent) = p.rigid.as_ref().map_or((0, 0), |b| (b.id, b.parent));
                *p = twin.clone();
                if let Some(b) = p.rigid.as_mut() {
                    b.id = id;
                    b.parent = parent;
                }
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
    index
}

/// Make the twin of block `index` follow it after a turn, a stretch or a
/// scale (the pose of the pair is mirrored).
pub fn sync_twin(run: &mut RunProgress, index: usize) {
    let Some(part) = run.parts.get(index).cloned() else {
        return;
    };
    if part.pair_id == 0 {
        return;
    }
    let block = part.rigid.clone().unwrap_or_default();
    let twin_block = block.reflected();
    for (i, p) in run.parts.iter_mut().enumerate() {
        if i != index && p.pair_id == part.pair_id {
            // The twin keeps its own id and parent (`link_twin` maps them when the parent changes).
            let (id, parent) = p.rigid.as_ref().map_or((0, 0), |b| (b.id, b.parent));
            p.rigid = Some(RigidBlock { id, parent, ..twin_block.clone() });
            p.mirrored = !part.mirrored;
            p.scale = part.scale;
        }
    }
}

/// Poses (preview space) and hulls of the session's blocks.
pub fn block_poses(
    rigid: &mut RigidEditor,
    run: &RunProgress,
    originals: &Originals,
    meshes: &Assets<Mesh>,
    group: u32,
) -> Vec<(Transform, Option<Arc<Hull>>)> {
    run.parts
        .iter()
        .map(|p| {
            let model = model_of(originals, group, p.definition_id, p.instance);
            (pose_of(p, model), rigid.hull(meshes, originals, group, p))
        })
        .collect()
}

/// A block's transform in preview space.
pub fn pose_of(p: &PartInstance, model: Option<&crate::OriginalModel>) -> Transform {
    let unit = model.map_or(1.0, editor_unit) * p.scale;
    let mut b = p.rigid.clone().unwrap_or_default();
    b.mirrored = p.mirrored;
    b.transform(unit)
}

/// Rest block `index` on `surface` (the placement of a drag): the pose, then
/// the twin rule. Returns the block's index afterwards.
#[allow(clippy::too_many_arguments)]
pub fn place_on(
    run: &mut RunProgress,
    index: usize,
    surface: &Surface,
    model: &crate::OriginalModel,
    hull: &Hull,
    info: Option<&crate::creature_editor_data::CreaturePartInfo>,
    symmetry: bool,
    limits: (f32, f32),
) -> usize {
    place_on_stacked(run, index, surface, model, hull, info, symmetry, limits, &StackInput::default())
}

/// The editor around a dragged part for the stacking rules: the studio plane,
/// whether the editor mirrors parts, and the band round x = 0 in which a block is
/// on the plane of symmetry (the part's symmetry snap delta).
pub fn stack_env(symmetry: bool, info: Option<&crate::creature_editor_data::CreaturePartInfo>, hull: &Hull, bounds: f32) -> StackEnv {
    StackEnv { floor: Some(0.0), symmetric: symmetry, plane_band: snap_delta(info, Some(hull.max.x - hull.min.x), 1.0), bounds }
}

/// `place_on` with the stacking rules of the part and of the block it rests on
/// (`rigid_editor_stack`), in the order of the retail manipulator (0x5B2E90,
/// combiner 0x490F20): the pose from the surface with the orientation rules
/// (`FUN_00494760`), then the combiner's candidates at that pose: the height rules
/// (`FUN_00491FF0`: stay above, snap down to), the Align height / lateral / XY rules and
/// `TrySnapToParent` (`FUN_00493320`: the parent's centre on top, else the snap vectors),
/// combined by `rigid_editor_stack::combine`; the second orientation pass takes the snap
/// vector's or the lateral align's orientation; `MoveBottomEdgeToSurface` and the bounds follow.
#[allow(clippy::too_many_arguments)]
pub fn place_on_stacked(
    run: &mut RunProgress,
    index: usize,
    surface: &Surface,
    model: &crate::OriginalModel,
    hull: &Hull,
    info: Option<&crate::creature_editor_data::CreaturePartInfo>,
    symmetry: bool,
    limits: (f32, f32),
    stack: &StackInput,
) -> usize {
    // The poses before the move: the blocks under this one go with it (`FUN_0049F4D0`).
    let before = poses_by_id(&run.parts);
    ensure_id(&mut run.parts, index);
    let part = &run.parts[index];
    let mut b = part.rigid.clone().unwrap_or_default();
    b.mirrored = part.mirrored;
    // A block on the plane of symmetry is single: the rules see it in its primary form (the
    // twin's rotation is the reflected one), `settle` then finds nothing left to convert.
    let on_plane = symmetry && stack.side_of(surface.point.x) == 0;
    if on_plane && b.mirrored {
        b = b.reflected();
        run.parts[index].mirrored = false;
    }
    // A block without a parent keeps its orientation (`FUN_0049C1D0` sets the stored one back): no surface to turn to.
    let free = stack.free.filter(|_| surface.block.is_none());
    let orient = info.is_some_and(|i| i.orient_to_surface) && free.is_none();
    let unit = editor_unit(model) * run.parts[index].scale;
    let mut moving = Moving {
        block: &b,
        unit,
        hull_center: hull.center(),
        hull_half: hull.half(),
        orient,
        upright: stack.upright(surface),
        plan: OrientPlan { base: Some(rows_of(b.rotation())), ..stack.plan(surface, surface.point, orient, None) },
        input: None,
        min_y: limits.0,
        max_y: limits.1,
    };
    // The pose of a block on the studio base is the origin on the camera ray (`FUN_0049C1D0`), not a point of a surface.
    let pin_at = |moving: &Moving| {
        let mut block = pin(surface, moving);
        if let Some(free) = free {
            block.position = free.position.to_array();
        }
        block
    };
    let mut placed = pin_at(&moving);
    let rules = stack.rules.as_deref();
    let parent = stack.parent.as_ref();
    let mut scale = placed.stretch() * unit;
    if placed.mirrored {
        scale.x = -scale.x;
    }
    let mut lost_parent = false;
    if let Some(rules) = rules {
        // The combiner `FUN_00490F20`: every rule is evaluated at the pose the block has now (the stored
        // pose of the retail block is the pin position with the surface orientation, before the rules).
        let pre = placed.clone();
        let on_top = stack.on_top(surface);
        let stored = Transform { rotation: surface_rotation(surface, &moving), ..pre.transform(unit) };
        let align = stack.align(&AlignBlock::of(&stored, hull), surface.block);
        let snap = parent.and_then(|p| try_snap_to_parent(rules, &p.rules, &p.target, pre.position(), COMBINER_SCALE, on_top));
        // `FUN_00491FF0`: stay above / snap down to.
        let stay = height_correction(rules, &pre.transform(unit), hull, &stack.others, stack.env.floor, free.is_some_and(|f| f.on_ground));
        let combined = combine(&Candidates {
            position: to_retail(pre.position()),
            stay: stay.map(|dy| (dy, dy.abs())),
            height: align.height,
            lateral: align.lateral,
            xy: align.xy,
            snap: snap.map(|s| (to_retail(s.position), s.distance)),
        });
        let target = from_retail(combined.position);
        if combined.distance.is_some() {
            debug!("rigid stacking: block {index} moved by the combiner to {target:?} ({:?}, distance {:?})", combined.orientation, combined.distance);
        }
        // The combiner's orientation goes through a second `FUN_00494760` pass: the snap vector's
        // (`FUN_00492D80` turns the matrix to the face it snapped to; the centre snap leaves it) or the lateral
        // align's (`FUN_0048F3E0`, from the block's stored base orientation).
        let rows = rows_of(pre.rotation());
        let second = match (combined.orientation, snap, &stack.parent, align.lateral) {
            (Orientation::Snap, Some(snap), Some(p), _) => {
                let input = match snap.vector {
                    Some(i) => align_to_snap_vector(rows, p.target.vector_direction(i), p.target.rows[2]),
                    None => rows,
                };
                // `SetSnapType` ran only when the snap was the only x / y source besides the XY align.
                Some((input, combined.snap_type_set.then_some(snap)))
            }
            (Orientation::Lateral, _, _, Some((_, lateral, _))) => Some((lateral, None)),
            _ => None,
        };
        if let Some((input, snap_type)) = second {
            moving.input = Some(input);
            moving.plan = OrientPlan { base: moving.plan.base, ..stack.plan(surface, target, orient, snap_type.as_ref()) };
            placed = pin_at(&moving);
        }
        placed.position = (placed.position() + (target - pre.position())).to_array();
        // `MoveBottomEdgeToSurface` (not on top of a parent that allows top behaviours).
        if let (Some(parent), false) = (parent, moving.plan.on_top) {
            let input = rows_of(surface_rotation(surface, &moving));
            let output = rows_of(placed.rotation());
            if let Some(at) = move_bottom_edge(rules, input, output, placed.position(), scale, hull, &parent.pose, &parent.hull, stack.env.floor.unwrap_or(0.0)) {
                placed.position = at.to_array();
            }
        }
        // `FUN_00493AF0` (called by `FUN_0049A7A0` unless `modelIsAllowedOutOfBounds`): the position stays
        // inside the circle of radius `bounds / 2` round the editor's centre line.
        // Retail then drops the parent of a block that had one (0x49B267) and places it as a parentless block; here the
        // clamped pose stays and the block is parentless (ADAPTED: the parentless pose is not recomputed this step).
        if !rules.allowed_out_of_bounds && parent.is_some() {
            let clamped = clamp_to_bounds(placed.position(), stack.env.bounds);
            if clamped != placed.position() {
                placed.position = clamped.to_array();
                lost_parent = true;
            }
        }
    }
    placed.position = limit_height(placed.position(), placed.rotation(), scale, &moving).to_array();
    // `SetParent` (0x4A2EB0 / 0x4987B0): the block hit is the parent, none when the ray met no block.
    let parent_id = match surface.block {
        Some(p) if !lost_parent => ensure_id(&mut run.parts, p),
        _ => 0,
    };
    placed.id = run.parts[index].rigid.as_ref().map_or(placed.id, |b| b.id);
    placed.parent = parent_id;
    if run.parts[index].rigid.as_ref() == Some(&placed) {
        return index;
    }
    run.parts[index].rigid = Some(placed);
    // The retail default delta of a part without one: 0.3 x its model's width
    // in the model's own units (`creature_editor_edit::default_snap_delta`; the
    // hull is the raw mesh). The rigid editors work in the retail editor
    // metres (`editor_unit`), so the delta needs no frame factor.
    let snap = snap_delta(info, Some(hull.max.x - hull.min.x), 1.0);
    let next = settle(run, index, symmetry, snap);
    sync_twin(run, next);
    link_twin(&mut run.parts, next);
    let moved: Vec<usize> = std::iter::once(next).chain(twins_of(&run.parts, next)).collect();
    carry_descendants(&mut run.parts, &before, &moved);
    run.request_save();
    next
}

/// Is block `index` attached to a root when it is released? Retail `FUN_00436200` walks the block's parents
/// (`EditorRigblock::mpParent`, stored in `RigidBlock::parent`) up to one with `modelCanBeParentless`; a block without
/// a root is a bad location.
pub fn release_attached(run: &RunProgress, rules: &[Option<Arc<StackRules>>], index: usize) -> bool {
    connected_to_root(index, rules, &parent_of_table(&run.parts))
}

/// A design saved before the parent links were stored: number its blocks and derive their links once
/// (`rigid_editor_stack::legacy_links`; it needs the blocks' meshes, so it waits until they are all loaded).
pub fn ensure_links(rigid: &mut RigidEditor, run: &mut RunProgress, originals: &Originals, meshes: &Assets<Mesh>, group: u32) {
    if !run.parts.iter().any(|p| p.rigid.as_ref().is_some_and(|b| b.id == 0)) {
        return;
    }
    let poses = block_poses(rigid, run, originals, meshes, group);
    if poses.iter().any(|(_, h)| h.is_none()) {
        return;
    }
    let rules = rigid.all_stack_rules(originals, group, &run.parts);
    if legacy_links(&mut run.parts, &rules, &poses) {
        run.request_save();
    }
}

/// The blocks a drag of block `index` leaves out of the pin ray and the rules: the block, its twin and its
/// descendants (the manipulator's ignore list, `FUN_0048CC90`).
pub fn moving_set(run: &RunProgress, index: usize) -> Vec<usize> {
    let mut skip = twins_of(&run.parts, index);
    skip.push(index);
    for d in descendants(&run.parts, index) {
        if !skip.contains(&d) {
            skip.push(d);
        }
    }
    for t in twins_of(&run.parts, index) {
        for d in descendants(&run.parts, t) {
            if !skip.contains(&d) {
                skip.push(d);
            }
        }
    }
    skip
}

/// The pose of a block the ray left without a parent (`FUN_0049BDC0` + `FUN_0049C1D0`, called by the Pinning manipulator
/// when no block is hit): the plane is chosen at the start of the drag (and again after the block had a parent) and kept;
/// the origin follows the ray on it, or stands on the ground ([`free_position`]). `origin` / `dir` are the ray in the
/// preview's (editor) frame, `floor` the studio plane, `bounds` the setup's `editorBoundSize`.
#[allow(clippy::too_many_arguments)]
pub fn free_pose(
    run: &RunProgress,
    index: usize,
    poses: &[(Transform, Option<Arc<Hull>>)],
    plane: &mut Option<FreePlane>,
    origin: Vec3,
    dir: Vec3,
    floor: f32,
    bounds: f32,
    symmetric: bool,
) -> Option<FreePose> {
    let position = poses.get(index)?.0.translation;
    let lower = |i: usize| poses.get(i).and_then(|(t, h)| h.as_ref().map(|h| world_box(t, h).0.y));
    // The lowest point of the block and of what stands on it (`FUN_0044B2D0` mode 0, children included).
    let family = std::iter::once(index).chain(descendants(&run.parts, index));
    let bottom = family.filter_map(lower).fold(f32::INFINITY, f32::min);
    if !bottom.is_finite() {
        return None;
    }
    let ray = (to_retail(origin), to_retail(dir));
    let plane = *plane.get_or_insert_with(|| {
        let skip = moving_set(run, index);
        let others: Vec<Vec3> = poses.iter().enumerate().filter(|(i, _)| !skip.contains(i)).map(|(_, (t, _))| to_retail(t.translation)).collect();
        let on_plane = symmetric && run.parts[index].pair_id != 0 && position.x.abs() < 0.001;
        free_plane(ray.1, to_retail(position), bottom - floor < 0.01, nearest_block_position(to_retail(position), &others), bounds, on_plane)
    });
    let (at, on_ground) = free_position(ray.0, ray.1, &plane, to_retail(position), position.y - bottom + floor);
    Some(FreePose { position: from_retail(at), on_ground })
}

/// Turn block `index` (and its twin) by `by` about its own origin; the blocks standing on it turn with it.
pub fn turn_block(run: &mut RunProgress, index: usize, by: Quat) {
    let before = poses_by_id(&run.parts);
    let mut b = run.parts[index].rigid.clone().unwrap_or_default();
    b.set_rotation(by * b.rotation());
    run.parts[index].rigid = Some(b);
    sync_twin(run, index);
    let roots: Vec<usize> = std::iter::once(index).chain(twins_of(&run.parts, index)).collect();
    carry_descendants(&mut run.parts, &before, &roots);
}

/// A new block of a part: where a palette purchase starts (resting on the
/// studio base, the carry moves it from there).
pub fn new_block(hull_center: Vec3, hull_half: Vec3, unit: f32, floor: f32) -> RigidBlock {
    // The origin on the centre line, the bounding box's bottom on the floor.
    RigidBlock::at(Vec3::new(0.0, floor + (hull_half.y - hull_center.y) * unit, 0.0))
}

// ---------------------------------------------------------------------------
// Grabs
// ---------------------------------------------------------------------------

/// What the left button holds.
#[derive(Clone)]
pub enum Grab {
    /// A block moving over the surfaces: carried from the palette or dragged.
    /// `plane`: the plane of `FUN_0049BDC0` while the block has no parent (chosen at the start of the drag and
    /// again after the block had a parent).
    Move { index: usize, before: Box<RunProgress>, carried: bool, press: Vec2, moved: bool, plane: Option<FreePlane> },
    /// A rotation ring: the block turns about its own axis `axis` (preview
    /// space `axis_world`) as the pointer runs along the ring.
    Turn {
        index: usize,
        axis: usize,
        before: Box<RunProgress>,
        start: Quat,
        axis_world: Vec3,
        press: Vec2,
        tangent: Vec2,
        radius_px: f32,
    },
    /// A morph handle of the part (`PartHandles::handles[handle]`): the drag
    /// moves its weight along the handle's line `p0 -> p1` (preview space).
    Morph {
        index: usize,
        handle: usize,
        id: u32,
        before: Box<RunProgress>,
        p0: Vec3,
        p1: Vec3,
    },
    /// A deform handle: the block stretches along its own axis `axis`.
    Stretch {
        index: usize,
        axis: usize,
        sign: i8,
        before: Box<RunProgress>,
        start: f32,
        centre: Vec3,
        direction: Vec3,
        half: f32,
        t0: f32,
    },
}

impl Grab {
    pub fn kind(&self) -> Option<HandleKind> {
        match self {
            Grab::Move { .. } => None,
            Grab::Turn { axis, .. } => Some(HandleKind::Turn(*axis)),
            Grab::Stretch { axis, sign, .. } => Some(HandleKind::Stretch(*axis, *sign)),
            Grab::Morph { handle, .. } => Some(HandleKind::Morph(*handle)),
        }
    }
}

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// Rotation ring about the block's own axis (0 x, 1 y, 2 z).
    Turn(usize),
    /// Deform handle on a face: (axis, sign).
    Stretch(usize, i8),
    /// A morph handle of the part: its index in `PartHandles::handles`.
    Morph(usize),
}

/// A handle of the selected block in preview space.
#[derive(Clone, Copy, Debug)]
pub struct HandleSpec {
    pub kind: HandleKind,
    pub position: Vec3,
    /// A ring lies in the plane perpendicular to its axis (a torus has its
    /// axis along Y).
    pub rotation: Quat,
    /// Ring radius or sphere radius.
    pub radius: f32,
    /// World axis of the ring, or the direction the deform handle pulls.
    pub axis: Vec3,
}

/// The nine handles of a block (three rotation rings, six deform spheres), sized
/// to stay the same on screen at `view_distance`.
pub fn handle_specs(pose: &Transform, hull: &Hull, view_distance: f32) -> Vec<HandleSpec> {
    let centre = pose.transform_point(hull.center());
    let half = hull.half() * pose.scale.abs();
    let size = (view_distance * 0.02).max(0.02);
    let ring = (half.max_element() * 1.15).max(size * 4.0);
    let axes = [pose.rotation * Vec3::X, pose.rotation * Vec3::Y, pose.rotation * Vec3::Z];
    let mut out = Vec::with_capacity(9);
    for (i, axis) in axes.iter().enumerate() {
        out.push(HandleSpec {
            kind: HandleKind::Turn(i),
            position: centre,
            rotation: Quat::from_rotation_arc(Vec3::Y, *axis),
            radius: ring,
            axis: *axis,
        });
        for sign in [-1i8, 1] {
            let a = *axis * sign as f32;
            out.push(HandleSpec {
                kind: HandleKind::Stretch(i, sign),
                position: centre + a * (half[i] + size * 1.6),
                rotation: Quat::IDENTITY,
                radius: size,
                axis: a,
            });
        }
    }
    out
}

/// Weight of morph handle `h` in a block's weights (the handle's default
/// until the block sets one).
pub fn morph_weight(weights: &[(u32, f32)], id: u32, default: f32) -> f32 {
    weights.iter().find(|(i, _)| *i == id).map_or(default, |(_, w)| *w)
}

/// The two ends of a morph handle's line in preview space (`FUN_005ACFC0`'s
/// `p0 -> p1`): the part's model coordinates through the mesh basis and the
/// block's pose.
pub fn morph_line(pose: &Transform, part: &PartHandles, h: usize) -> Option<(Vec3, Vec3)> {
    let def = part.handles.get(h)?;
    Some((pose.transform_point(part.to_mesh(def.start)), pose.transform_point(part.to_mesh(def.end))))
}

/// The retail morph handles of a block as spheres on their lines, sized to stay
/// the same on screen at `view_distance` (`weights` = the block's).
pub fn morph_specs(pose: &Transform, part: &PartHandles, weights: &[(u32, f32)], view_distance: f32) -> Vec<HandleSpec> {
    let size = (view_distance * 0.02).max(0.02);
    (0..part.handles.len())
        .filter_map(|i| {
            let (p0, p1) = morph_line(pose, part, i)?;
            let def = &part.handles[i];
            let w = morph_weight(weights, def.id, def.default).clamp(0.0, 1.0);
            Some(HandleSpec { kind: HandleKind::Morph(i), position: p0.lerp(p1, w), rotation: Quat::IDENTITY, radius: size, axis: (p1 - p0).normalize_or_zero() })
        })
        .collect()
}

/// The handles of a selected block: the rings, and either the part's morph
/// handles or, when it has none, the six axis stretch spheres (ADAPTED).
pub fn block_specs(pose: &Transform, hull: &Hull, part: Option<&PartHandles>, weights: &[(u32, f32)], view_distance: f32) -> Vec<HandleSpec> {
    let mut specs = handle_specs(pose, hull, view_distance);
    let morph = part.map(|p| morph_specs(pose, p, weights, view_distance)).unwrap_or_default();
    if !morph.is_empty() {
        specs.retain(|s| !matches!(s.kind, HandleKind::Stretch(..)));
        specs.extend(morph);
    }
    specs
}

/// Points of a ring (preview space).
pub fn ring_points(spec: &HandleSpec, count: usize) -> Vec<Vec3> {
    (0..count)
        .map(|i| {
            let t = i as f32 / count as f32 * std::f32::consts::TAU;
            spec.position + spec.rotation * Vec3::new(t.cos(), 0.0, t.sin()) * spec.radius
        })
        .collect()
}

/// Closest parameter along the line `centre + t * axis` (unit axis) to a ray
/// (unit direction).
pub fn line_param(centre: Vec3, axis: Vec3, origin: Vec3, dir: Vec3) -> Option<f32> {
    let w = centre - origin;
    let b = axis.dot(dir);
    let denom = 1.0 - b * b;
    if denom < 1e-6 {
        return None;
    }
    Some((b * dir.dot(w) - axis.dot(w)) / denom)
}

/// The handle nearest to the cursor and the ring sample / tangent used by a
/// turn grab. `to_screen` projects a preview-space point to the viewport.
pub fn pick_handle(
    specs: &[HandleSpec],
    cursor: Vec2,
    to_screen: &dyn Fn(Vec3) -> Option<Vec2>,
    hits: Option<&dyn Fn(&HandleSpec) -> Option<f32>>,
) -> Option<(HandleKind, Vec2, f32)> {
    // With the handle models: the nearest model the mouse ray meets (retail picks the
    // handle groups at collision mode 4, the triangle mesh). A ring's grip data is the
    // screen tangent and radius at its sample nearest the cursor.
    if let Some(hits) = hits {
        let (spec, _) = specs.iter().filter_map(|s| hits(s).map(|d| (s, d))).min_by(|a, b| a.1.total_cmp(&b.1))?;
        return Some(match spec.kind {
            HandleKind::Turn(_) => {
                let (tangent, radius) = ring_grip(spec, cursor, to_screen).unwrap_or((Vec2::X, 24.0));
                (spec.kind, tangent, radius)
            }
            _ => (spec.kind, Vec2::ZERO, 0.0),
        });
    }
    let mut best: Option<(HandleKind, Vec2, f32, f32)> = None;
    for spec in specs {
        match spec.kind {
            HandleKind::Stretch(..) | HandleKind::Morph(_) => {
                let Some(s) = to_screen(spec.position) else { continue };
                let d = s.distance(cursor);
                if d < HANDLE_PICK_PX && best.is_none_or(|b| d - 4.0 < b.3) {
                    best = Some((spec.kind, Vec2::ZERO, 0.0, d - 4.0));
                }
            }
            HandleKind::Turn(_) => {
                let pts = ring_points(spec, 64);
                let screen: Vec<Option<Vec2>> = pts.iter().map(|p| to_screen(*p)).collect();
                let centre = to_screen(spec.position);
                for i in 0..pts.len() {
                    let (Some(s), Some(c)) = (screen[i], centre) else { continue };
                    let d = s.distance(cursor);
                    if d < HANDLE_PICK_PX * 0.7 && best.is_none_or(|b| d < b.3) {
                        let next = screen[(i + 1) % pts.len()].unwrap_or(s);
                        let prev = screen[(i + pts.len() - 1) % pts.len()].unwrap_or(s);
                        let tangent = (next - prev).normalize_or_zero();
                        best = Some((spec.kind, tangent, s.distance(c).max(24.0), d));
                    }
                }
            }
        }
    }
    best.map(|b| (b.0, b.1, b.2))
}

/// Screen tangent and radius (pixels) of a ring at its sample nearest the cursor.
fn ring_grip(spec: &HandleSpec, cursor: Vec2, to_screen: &dyn Fn(Vec3) -> Option<Vec2>) -> Option<(Vec2, f32)> {
    let pts = ring_points(spec, 64);
    let screen: Vec<Option<Vec2>> = pts.iter().map(|p| to_screen(*p)).collect();
    let centre = to_screen(spec.position)?;
    let i = (0..pts.len()).filter(|i| screen[*i].is_some()).min_by(|a, b| screen[*a].unwrap().distance(cursor).total_cmp(&screen[*b].unwrap().distance(cursor)))?;
    let s = screen[i]?;
    let next = screen[(i + 1) % pts.len()].unwrap_or(s);
    let prev = screen[(i + pts.len() - 1) % pts.len()].unwrap_or(s);
    Some(((next - prev).normalize_or_zero(), s.distance(centre).max(24.0)))
}

/// The retail handle models and their looks (`HandleTuning` colours).
pub struct HandleAssets {
    pub models: Arc<HandleModels>,
    pub look: HandleLook,
}

/// Model of a handle kind: the morph handle is the teardrop, the ring the ring
/// model, a stretch handle (ADAPTED) the ball connector's sphere.
pub fn handle_model(kind: HandleKind, models: &HandleModels) -> &HandleModel {
    match kind {
        HandleKind::Morph(_) => &models.teardrop,
        HandleKind::Turn(_) => &models.ring,
        HandleKind::Stretch(..) => &models.sphere,
    }
}

/// Placement of a handle's model in preview space. A morph handle: its model's Y
/// along the handle line (`deform_handle_rotation`), scaled by the tuning scale
/// (1.3, or 1.2 for the buildings) at the creature editor's size on screen
/// (ADAPTED, see the header); a ring: its model around the spec's axis with the
/// spec's radius; a stretch handle: the sphere model at the spec's radius.
pub fn handle_model_transform(spec: &HandleSpec, models: &HandleModels, deform_scale: f32, view_distance: f32) -> Transform {
    let at = Transform::from_translation(spec.position);
    match spec.kind {
        HandleKind::Morph(_) => at
            .with_rotation(deform_handle_rotation(spec.axis))
            .with_scale(Vec3::splat(deform_scale * (view_distance / REFERENCE_ZOOM).max(0.05))),
        HandleKind::Turn(_) => at.with_rotation(spec.rotation).with_scale(Vec3::splat(spec.radius / models.ring.radius_about_y().max(1e-3))),
        HandleKind::Stretch(..) => {
            let half = ((models.sphere.max - models.sphere.min) * 0.5).max_element().max(1e-3);
            at.with_scale(Vec3::splat(spec.radius / half))
        }
    }
}

/// Marks the overdraw model of a handle (a child of the handle's entity).
#[derive(Component)]
pub struct RigidHandleOverdraw;

/// An open wheel operation: one undo entry for a run of notches.
pub struct WheelOp {
    pub before: Box<RunProgress>,
    pub idle: f32,
    pub travel: f32,
}

// ---------------------------------------------------------------------------
// Scripted edits (captures and the GPU test)
// ---------------------------------------------------------------------------

/// One scripted edit, applied the way the pointer does it.
#[derive(Clone, Debug)]
pub enum Step {
    /// A palette press (the part's PROP instance): buys the part.
    Buy(u32),
    /// Drop the last block where a ray (preview space) meets the model or the
    /// studio base.
    Place { origin: Vec3, dir: Vec3 },
    /// Select a block (`None` clears the selection).
    Select(Option<usize>),
    /// Mouse wheel notches over the selected block.
    Scale(f32),
    /// Turn the selected block about the vertical axis (radians).
    Turn(f32),
    /// Stretch the selected block along its own axis (factor).
    Stretch(usize, f32),
    Remove,
    Undo,
    Redo,
}

/// Apply one scripted step per frame (before `actions`).
#[allow(clippy::too_many_arguments)]
pub fn scripted(
    mut rigid: ResMut<RigidEditor>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    meshes: Res<Assets<Mesh>>,
    data: Res<CreatureEditorData>,
) {
    if !rigid.active || rigid.grab.is_some() {
        return;
    }
    let Some(step) = rigid.script.pop_front() else {
        return;
    };
    let kind = rigid.kind;
    let group = kind.part_group();
    ensure_links(&mut rigid, &mut run, &originals, &meshes, group);
    let setup = data.setup(kind);
    let symmetry = setup.is_some_and(|s| s.use_symmetry);
    let limits = (setup.and_then(|s| s.min_height).unwrap_or(-10.0), setup.and_then(|s| s.max_height).unwrap_or(10.0));
    let bounds = setup.and_then(|s| s.bound_size).unwrap_or(0.0);
    let last = run.parts.len().saturating_sub(1);
    match step {
        Step::Buy(part) => state.queued.push(Action::Buy(part)),
        Step::Undo => state.queued.push(Action::Undo),
        Step::Redo => state.queued.push(Action::Redo),
        Step::Remove => state.queued.push(Action::Remove),
        Step::Select(i) => {
            rigid.selected = i.filter(|i| *i < run.parts.len());
            if let Some(i) = rigid.selected {
                run.focused_part = i as u32;
            }
        }
        Step::Place { origin, dir } => {
            let i = rigid.selected.unwrap_or(last).min(last);
            if run.parts.is_empty() {
                return;
            }
            let before = run.clone();
            let poses = block_poses(&mut rigid, &run, &originals, &meshes, group);
            let part = run.parts[i].clone();
            let skip = moving_set(&run, i);
            let model = model_of(&originals, group, part.definition_id, part.instance);
            let (Some(model), Some(hull)) = (model, poses[i].1.clone()) else {
                return;
            };
            let rules = rigid.all_stack_rules(&originals, group, &run.parts);
            let allow = |j: usize| pin_allows(rules[i].as_deref(), rules.get(j).and_then(|r| r.as_deref()));
            let dir = dir.normalize_or(Vec3::NEG_Y);
            // A ray that meets no block the part can interact with leaves it parentless: the studio base's pose.
            let hit = surface_under_where(origin, dir, &poses, &skip, None, &allow);
            let free = if hit.is_none() { free_pose(&run, i, &poses, &mut None, origin, dir, 0.0, bounds, symmetry) } else { None };
            let surface = hit.or_else(|| free.map(|f| Surface { point: f.position, normal: Vec3::Y, t: 0.0, block: None }));
            if let Some(surface) = surface {
                let mut stack = StackInput::of(&rules, i, &surface, &poses, &parent_table(&run.parts), &skip, stack_env(symmetry, data.info_of(&part), &hull, bounds));
                stack.free = free;
                let next = place_on_stacked(&mut run, i, &surface, model, &hull, data.info_of(&part), symmetry, limits, &stack);
                rigid.selected = Some(next);
                run.focused_part = next as u32;
                if before.parts != run.parts {
                    state.record_external(before);
                }
            }
        }
        Step::Scale(notches) => {
            if let Some(i) = rigid.selected.filter(|i| *i < run.parts.len()) {
                let before = run.clone();
                if scale_part(&data, &mut run, i, notches) {
                    sync_twin(&mut run, i);
                    state.record_external(before);
                }
            }
        }
        Step::Turn(angle) => {
            if let Some(i) = rigid.selected.filter(|i| *i < run.parts.len()) {
                let before = run.clone();
                turn_block(&mut run, i, Quat::from_rotation_y(angle));
                state.record_external(before);
            }
        }
        Step::Stretch(axis, factor) => {
            if let Some(i) = rigid.selected.filter(|i| *i < run.parts.len() && axis < 3) {
                let before = run.clone();
                let mut b = run.parts[i].rigid.clone().unwrap_or_default();
                b.stretch[axis] = (b.stretch[axis] * factor).clamp(STRETCH_MIN, STRETCH_MAX);
                run.parts[i].rigid = Some(b);
                sync_twin(&mut run, i);
                state.record_external(before);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn to_local(root: &Transform, p: Vec3) -> Vec3 {
    root.rotation.conjugate() * (p - root.translation) / root.scale
}

/// The actions the chrome and the keyboard queue: buy, sell, undo, redo
/// (`editor::input` leaves them to the rigid editor).
#[allow(clippy::too_many_arguments)]
pub fn actions(
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<EditorState>,
    mut rigid: ResMut<RigidEditor>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    data: Res<CreatureEditorData>,
    mut editor: ResMut<CreatureEditor>,
) {
    if !rigid.active || !state.rigid {
        return;
    }
    let queued = std::mem::take(&mut state.queued);
    let kind = rigid.kind;
    let group = kind.part_group();
    for action in queued {
        match action {
            Action::Undo | Action::Redo => {
                rigid.grab = None;
                let done = if matches!(action, Action::Undo) { state.undo_step(&mut run) } else { state.redo_step(&mut run) };
                if done {
                    rigid.selected = rigid.selected.filter(|i| *i < run.parts.len());
                    editor.manipulating = false;
                }
            }
            Action::Remove => {
                let i = run.focused_part as usize;
                if i < run.parts.len() && state.locked_parts <= i {
                    let before = run.clone();
                    sell(&mut run, &data, i);
                    rigid.selected = None;
                    rigid.grab = None;
                    state.record_external(before);
                }
            }
            Action::Buy(part) => {
                if let Some(refused) = refusal(&data, kind, &run, &originals, part) {
                    editor.say(format!("{refused:?}"));
                    continue;
                }
                let Some(info) = data.parts.get(&part).cloned() else { continue };
                let before = run.clone();
                let imported = import_block(&mut originals, group, Some(part), info.model, &mut meshes, &mut images, &mut materials);
                if let Err(e) = imported {
                    editor.say(e);
                    continue;
                }
                let Some(model) = model_of(&originals, group, Some(part), info.model) else { continue };
                let unit = editor_unit(model);
                let default_scale = originals
                    .part_catalog
                    .by_definition_in(group, part)
                    .map_or(1.0, |d| d.default_scale.clamp(info.min_scale, info.max_scale.max(info.min_scale)));
                let hull = rigid.hull_of(&meshes, model);
                let mut p = PartInstance::new(info.model, 0).with_definition(part);
                p.scale = default_scale;
                let block = hull.as_ref().map_or_else(RigidBlock::default, |h| new_block(h.center(), h.half(), unit * default_scale, 0.0));
                let mut block = block;
                block.id = next_id(&run.parts);
                p.rigid = Some(block);
                let at = run.parts.len();
                run.parts.push(p);
                let symmetry = data.setup(kind).is_some_and(|s| s.use_symmetry);
                let snap = snap_delta(Some(&info), hull.as_ref().map(|h| h.max.x - h.min.x), 1.0);
                let at = settle(&mut run, at, symmetry, snap);
                link_twin(&mut run.parts, at);
                run.editor_dna -= info.price as f32;
                run.focused_part = at as u32;
                rigid.selected = Some(at);
                run.request_save();
                if mouse.pressed(MouseButton::Left) {
                    // A press that is still held carries the new block; the
                    // undo entry is recorded when it is dropped.
                    rigid.grab = Some(Grab::Move { index: at, before: Box::new(before), carried: true, press: Vec2::ZERO, moved: true, plane: None });
                    editor.manipulating = true;
                } else {
                    state.record_external(before);
                }
                editor.say(info.name.clone());
            }
            _ => {}
        }
    }
}

/// Remove block `index` (and its twin) and refund it (`modelSellBackFactor`).
pub fn sell(run: &mut RunProgress, data: &CreatureEditorData, index: usize) {
    let Some(part) = run.parts.get(index).cloned() else {
        return;
    };
    let refund = data.info_of(&part).map_or(0.0, |i| i.price as f32 * i.sell_back);
    if part.pair_id == 0 {
        run.parts.remove(index);
    } else {
        run.parts.retain(|p| p.pair_id != part.pair_id);
    }
    // The blocks that stood on the sold one stay (retail's choice for them was not read), parentless.
    prune_links(&mut run.parts);
    run.editor_dna += refund;
    run.focused_part = run.focused_part.min(run.parts.len().saturating_sub(1) as u32);
    run.request_save();
}

/// The manipulators: grab, carry and drop, the wheel, the key turn.
#[allow(clippy::too_many_arguments)]
pub fn manipulate(
    time: Res<Time>,
    (mouse, keys): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    (mut wheel, mut motion): (MessageReader<MouseWheel>, MessageReader<MouseMotion>),
    pointer: Res<crate::spui::view::SpuiPointer>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    roots: Query<&Transform, With<RigidPreview>>,
    (data, originals): (Res<CreatureEditorData>, Res<Originals>),
    meshes: Res<Assets<Mesh>>,
    mut rigid: ResMut<RigidEditor>,
    mut editor: ResMut<CreatureEditor>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
) {
    let dt = time.delta_secs();
    let travel: f32 = motion.read().map(|m| m.delta.length()).sum();
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 40.0,
        })
        .sum();
    if !rigid.active || !editor.active {
        return;
    }
    let build = editor.mode == Mode::Build && !editor.options_open && !editor.naming;
    if editor.mode == Mode::Paint && notches != 0.0 && !pointer.over_ui {
        editor.wheel_consumed = true;
    }
    let kind = rigid.kind;
    let group = kind.part_group();
    ensure_links(&mut rigid, &mut run, &originals, &meshes, group);
    let setup = data.setup(kind);
    // A wheel operation ends after the idle timeout or mouse travel.
    let timeout = setup.and_then(|s| s.mouse_wheel_timeout).unwrap_or(WHEEL_TIMEOUT);
    let distance = setup.and_then(|s| s.mouse_wheel_distance).unwrap_or(WHEEL_DISTANCE);
    if let Some(op) = rigid.wheel.as_mut() {
        op.idle += dt;
        op.travel += travel;
        if op.idle > timeout || op.travel > distance || !build {
            let op = rigid.wheel.take().unwrap();
            if op.before.parts != run.parts {
                state.record_external(*op.before);
                run.request_save();
            }
        }
    }
    let symmetry = setup.is_some_and(|s| s.use_symmetry);
    let (min_y, max_y) = (
        setup.and_then(|s| s.min_height).unwrap_or(-10.0),
        setup.and_then(|s| s.max_height).unwrap_or(10.0),
    );
    let bounds = setup.and_then(|s| s.bound_size).unwrap_or(0.0);
    if !build {
        match rigid.grab.take() {
            Some(Grab::Move { before, .. } | Grab::Turn { before, .. } | Grab::Stretch { before, .. } | Grab::Morph { before, .. }) => {
                if before.parts != run.parts {
                    state.record_external(*before);
                }
            }
            None => {}
        }
        rigid.hover = None;
        editor.manipulating = false;
        return;
    }
    let (Ok(window), Ok((camera, cam)), Ok(root)) = (windows.single(), cameras.single(), roots.single()) else {
        return;
    };
    let root = holder_of(root);
    let cursor = window.cursor_position();
    let over_ui = pointer.over_ui || cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_ui(c)));
    let over_palette = cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_palette(c)));
    let ray = cursor.and_then(|c| camera.viewport_to_world(cam, c).ok());
    let local_ray = ray.map(|r| {
        let o = to_local(&root, r.origin);
        let d = (root.rotation.conjugate() * *r.direction).normalize_or(Vec3::NEG_Z);
        (o, d)
    });
    // Poses and hulls of the blocks (preview space).
    let poses = block_poses(&mut rigid, &run, &originals, &meshes, group);

    // The handles of the selected block (`HandleTuning`: rings, deform
    // spheres); they take the press before the blocks do.
    let to_world = |p: Vec3| root.translation + root.rotation * (p * root.scale);
    let to_screen = |p: Vec3| camera.world_to_viewport(cam, to_world(p)).ok();
    let eye = to_local(&root, cam.translation());
    let selected = rigid.selected.filter(|i| *i < run.parts.len());
    let part_handles = selected.and_then(|i| rigid.part_handles(&originals, group, &run.parts[i]));
    let specs: Vec<HandleSpec> = selected
        .and_then(|i| {
            let weights = run.parts[i].rigid.as_ref().map(|b| b.handles.as_slice()).unwrap_or_default();
            poses[i].1.as_ref().map(|h| block_specs(&poses[i].0, h, part_handles.as_deref(), weights, eye.distance(poses[i].0.translation)))
        })
        .unwrap_or_default();
    let models = rigid.handle_assets.as_ref().map(|a| a.models.clone());
    let deform_scale = if kind.is_building() { BUILDING_DEFORM_SCALE } else { DEFORM_SCALE };
    let view_distance = selected.map_or(10.0, |i| eye.distance(poses[i].0.translation));
    let model_hit = |s: &HandleSpec| -> Option<f32> {
        let (m, (o, d)) = (models.as_ref()?, local_ray?);
        handle_model(s.kind, m).pick(&handle_model_transform(s, m, deform_scale, view_distance), o, d)
    };
    let hits: Option<&dyn Fn(&HandleSpec) -> Option<f32>> = models.as_ref().map(|_| &model_hit as &dyn Fn(&HandleSpec) -> Option<f32>);
    let picked = cursor.filter(|_| !over_ui && rigid.grab.is_none()).and_then(|c| pick_handle(&specs, c, &to_screen, hits));
    rigid.hover = picked.map(|p| p.0);
    if mouse.just_pressed(MouseButton::Left) && !over_ui && rigid.grab.is_none() && !state.dragging() {
        if let (Some((kind, tangent, radius_px)), Some(i), Some(c), Some((o, d))) = (picked, selected, cursor, local_ray) {
            let spec = specs.iter().find(|s| s.kind == kind).copied();
            let part = &run.parts[i];
            let block = part.rigid.clone().unwrap_or_default();
            match (kind, spec) {
                (HandleKind::Turn(axis), Some(spec)) => {
                    rigid.grab = Some(Grab::Turn {
                        index: i,
                        axis,
                        before: Box::new(run.clone()),
                        start: block.rotation(),
                        axis_world: spec.axis,
                        press: c,
                        tangent,
                        radius_px,
                    });
                }
                (HandleKind::Morph(h), Some(_)) => {
                    let line = part_handles.as_deref().and_then(|p| morph_line(&poses[i].0, p, h).zip(p.handles.get(h)));
                    if let Some(((p0, p1), def)) = line {
                        rigid.grab = Some(Grab::Morph { index: i, handle: h, id: def.id, before: Box::new(run.clone()), p0, p1 });
                    }
                }
                (HandleKind::Stretch(axis, sign), Some(spec)) => {
                    let centre = poses[i].0.transform_point(poses[i].1.as_ref().map_or(Vec3::ZERO, |h| h.center()));
                    let half = poses[i].1.as_ref().map_or(1.0, |h| h.half()[axis] * poses[i].0.scale[axis].abs()).max(0.02);
                    let t0 = line_param(centre, spec.axis, o, d).unwrap_or(half);
                    rigid.grab = Some(Grab::Stretch {
                        index: i,
                        axis,
                        sign,
                        before: Box::new(run.clone()),
                        start: block.stretch[axis],
                        centre,
                        direction: spec.axis,
                        half,
                        t0,
                    });
                }
                _ => {}
            }
        }
    }
    // Press: grab the block under the cursor, or clear the selection.
    if mouse.just_pressed(MouseButton::Left) && !over_ui && rigid.grab.is_none() && !state.dragging() {
        let hit = local_ray.and_then(|(o, d)| surface_under(o, d, &poses, &[], None).filter(|s| s.block.is_some()));
        match hit.and_then(|s| s.block) {
            // Alt + press clones the block (pays again; the held press then
            // carries the copy).
            Some(i) if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) => {
                if let Some(def) = data.definition_of(&run.parts[i]) {
                    match refusal(&data, kind, &run, &originals, def) {
                        None => state.queued.push(Action::Buy(def)),
                        Some(_) => editor.say("Not enough budget or complexity to clone this part"),
                    }
                }
            }
            Some(i) => {
                rigid.selected = Some(i);
                run.focused_part = i as u32;
                rigid.grab = Some(Grab::Move {
                    index: i,
                    before: Box::new(run.clone()),
                    carried: false,
                    press: cursor.unwrap_or_default(),
                    moved: false,
                    plane: None,
                });
            }
            None => rigid.selected = None,
        }
    }
    editor.manipulating = rigid.grab.is_some();
    // Turn and stretch: the handles' drags.
    let mut finished: Option<Box<RunProgress>> = None;
    match rigid.grab.as_ref() {
        Some(Grab::Turn { index, before, start, axis_world, press, tangent, radius_px, .. }) => {
            if !mouse.pressed(MouseButton::Left) {
                finished = Some(before.clone());
            } else if let (Some(c), true) = (cursor, *index < run.parts.len()) {
                let dtheta = (c - *press).dot(*tangent) / radius_px;
                let mut b = run.parts[*index].rigid.clone().unwrap_or_default();
                b.set_rotation(Quat::from_axis_angle(*axis_world, -dtheta) * *start);
                if run.parts[*index].rigid.as_ref() != Some(&b) {
                    let moved = poses_by_id(&run.parts);
                    run.parts[*index].rigid = Some(b);
                    sync_twin(&mut run, *index);
                    let roots: Vec<usize> = std::iter::once(*index).chain(twins_of(&run.parts, *index)).collect();
                    carry_descendants(&mut run.parts, &moved, &roots);
                    run.request_save();
                }
            }
        }
        Some(Grab::Morph { index, id, before, p0, p1, .. }) => {
            if !mouse.pressed(MouseButton::Left) {
                finished = Some(before.clone());
            } else if let (Some((o, d)), true) = (local_ray, *index < run.parts.len()) {
                if let Some(w) = deform_weight(o, d, *p0, *p1, 0.0, 1.0) {
                    let w = (w * MORPH_STEPS).round() / MORPH_STEPS;
                    let mut b = run.parts[*index].rigid.clone().unwrap_or_default();
                    match b.handles.iter_mut().find(|(i, _)| i == id) {
                        Some(entry) => entry.1 = w,
                        None => b.handles.push((*id, w)),
                    }
                    if run.parts[*index].rigid.as_ref() != Some(&b) {
                        run.parts[*index].rigid = Some(b);
                        sync_twin(&mut run, *index);
                        run.request_save();
                    }
                }
            }
        }
        Some(Grab::Stretch { index, axis, before, start, centre, direction, half, t0, .. }) => {
            if !mouse.pressed(MouseButton::Left) {
                finished = Some(before.clone());
            } else if let (Some((o, d)), true) = (local_ray, *index < run.parts.len()) {
                if let Some(t) = line_param(*centre, *direction, o, d) {
                    let factor = ((*half + (t - *t0)) / *half).clamp(0.05, 20.0);
                    let mut b = run.parts[*index].rigid.clone().unwrap_or_default();
                    b.stretch[*axis] = (*start * factor).clamp(STRETCH_MIN, STRETCH_MAX);
                    if run.parts[*index].rigid.as_ref() != Some(&b) {
                        run.parts[*index].rigid = Some(b);
                        sync_twin(&mut run, *index);
                        run.request_save();
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(before) = finished {
        rigid.grab = None;
        editor.manipulating = false;
        if before.parts != run.parts {
            state.record_external(*before);
        }
    }
    // Move: the block rests on the surface under the cursor.
    let mut released: Option<Grab> = None;
    if let Some(Grab::Move { index, carried, press, moved, plane, .. }) = rigid.grab.as_mut() {
        if !mouse.pressed(MouseButton::Left) {
            released = rigid.grab.take();
        } else if let (Some(c), Some((o, d))) = (cursor, local_ray) {
            *moved |= c.distance(*press) > 3.0;
            let live = *moved && !(over_ui && *carried);
            let mut plane_state = plane.take();
            if live && *index < run.parts.len() {
                let i = *index;
                let part = run.parts[i].clone();
                let skip = moving_set(&run, i);
                let model = model_of(&originals, group, part.definition_id, part.instance);
                let (Some(model), Some(hull)) = (model, poses[i].1.clone()) else {
                    return;
                };
                let info = data.info_of(&part);
                let rules = rigid.all_stack_rules(&originals, group, &run.parts);
                let allow = |j: usize| pin_allows(rules.get(i).and_then(|r| r.as_deref()), rules.get(j).and_then(|r| r.as_deref()));
                // No block hit (the ray meets no block the part can interact with): the block has no parent and its
                // origin follows the ray on the studio base's plane (`FUN_0049BDC0` / `FUN_0049C1D0`).
                let hit = surface_under_where(o, d, &poses, &skip, None, &allow);
                if hit.is_some() {
                    plane_state = None;
                }
                let free = if hit.is_none() { free_pose(&run, i, &poses, &mut plane_state, o, d, 0.0, bounds, symmetry) } else { None };
                let surface = hit.or_else(|| free.map(|f| Surface { point: f.position, normal: Vec3::Y, t: 0.0, block: None }));
                if let Some(surface) = surface {
                    let mut stack = StackInput::of(&rules, i, &surface, &poses, &parent_table(&run.parts), &skip, stack_env(symmetry, info, &hull, bounds));
                    stack.free = free;
                    let next = place_on_stacked(&mut run, i, &surface, model, &hull, info, symmetry, (min_y, max_y), &stack);
                    if let Some(Grab::Move { index, .. }) = rigid.grab.as_mut() {
                        *index = next;
                    }
                    rigid.selected = Some(next);
                }
            }
            if let Some(Grab::Move { plane, .. }) = rigid.grab.as_mut() {
                *plane = plane_state;
            }
        }
    }
    if let Some(Grab::Move { index, before, carried, moved, .. }) = released {
        rigid.hover = None;
        editor.manipulating = false;
        if carried && over_palette {
            // Dropped back on the palette: the purchase is cancelled.
            *run = *before;
            rigid.selected = None;
            editor.say("Part returned");
        } else if over_palette && !carried && moved && index < run.parts.len() {
            // A placed block dropped on the palette is sold.
            *run = *before;
            let before_sale = run.clone();
            sell(&mut run, &data, index);
            rigid.selected = None;
            state.record_external(before_sale);
        } else if (moved || carried) && index < run.parts.len() && !release_attached(&run, &rigid.all_stack_rules(&originals, group, &run.parts), index) {
            // The block is not attached to a root (`FUN_00436200`: neither it nor an ancestor has
            // `modelCanBeParentless`): retail marks the location bad (UI state 4 / 5, message 0x03F1BF5D).
            // ADAPTED: the response to the bad location is to refuse the release: the drag is undone (a
            // purchase is cancelled).
            *run = *before;
            rigid.selected = None;
            editor.say("This part has to be attached to a structure");
        } else if before.parts != run.parts || before.editor_dna != run.editor_dna {
            state.record_external(*before);
            run.request_save();
        }
    }
    // Key turn of the selected block (ADAPTED).
    if keys.just_pressed(KeyCode::KeyR) && !editor.naming {
        if let Some(i) = rigid.selected.filter(|i| *i < run.parts.len()) {
            let before = run.clone();
            let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { -KEY_TURN } else { KEY_TURN };
            turn_block(&mut run, i, Quat::from_rotation_y(step));
            state.record_external(before);
            run.request_save();
        }
    }
    // Mouse wheel over a block scales it (`2^(0.075 n)` within its limits).
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if notches != 0.0 && !shift && !over_ui {
        let hovered = local_ray.and_then(|(o, d)| surface_under(o, d, &poses, &[], None).and_then(|s| s.block));
        if let Some(i) = hovered {
            editor.wheel_consumed = true;
            let before = run.clone();
            if scale_part(&data, &mut run, i, notches) {
                sync_twin(&mut run, i);
                match rigid.wheel.as_mut() {
                    Some(op) => op.idle = 0.0,
                    None => rigid.wheel = Some(WheelOp { before: Box::new(before), idle: 0.0, travel: 0.0 }),
                }
            }
        }
    }
}


/// Draw the handles of the selected block (the teardrop, ring and sphere models
/// in the `HandleTuning` colours with their overdraw models; the hovered and the
/// held one change colour).
#[allow(clippy::too_many_arguments)]
pub fn handles(
    mut commands: Commands,
    mut rigid: ResMut<RigidEditor>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    (mut meshes, mut materials, overdraw_assets): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, Option<ResMut<Assets<OverdrawMaterial>>>),
    editor: Res<CreatureEditor>,
    roots: Query<&Transform, With<RigidPreview>>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut transforms: Query<&mut Transform, (With<RigidHandle>, Without<RigidPreview>)>,
    mut handle_materials: Query<&mut MeshMaterial3d<StandardMaterial>, With<RigidHandle>>,
    mut overdraw_materials: Query<(&ChildOf, &mut MeshMaterial3d<OverdrawMaterial>), With<RigidHandleOverdraw>>,
) {
    let selected = rigid.selected.filter(|i| *i < run.parts.len());
    let show = rigid.active && editor.active && editor.mode == Mode::Build && !editor.options_open && selected.is_some();
    let (Ok(root), Ok(cam)) = (roots.single(), cameras.single()) else {
        return;
    };
    let root = holder_of(root);
    if !show {
        for (e, _) in std::mem::take(&mut rigid.handle_entities) {
            commands.entity(e).despawn();
        }
        return;
    }
    let Some(mut overdraw_assets) = overdraw_assets else { return };
    let group = rigid.kind.part_group();
    let i = selected.unwrap();
    let part = run.parts[i].clone();
    let Some(model) = model_of(&originals, group, part.definition_id, part.instance) else {
        return;
    };
    let Some(hull) = rigid.hull_of(&meshes, model) else {
        return;
    };
    let mut block = part.rigid.clone().unwrap_or_default();
    block.mirrored = part.mirrored;
    let pose = block.transform(editor_unit(model) * part.scale);
    let eye = root.rotation.conjugate() * (cam.translation() - root.translation);
    let part_handles = rigid.part_handles(&originals, group, &part);
    let view_distance = eye.distance(pose.translation);
    let specs = block_specs(&pose, &hull, part_handles.as_deref(), &block.handles, view_distance);
    if rigid.handle_assets.is_none() {
        let models = match load_handle_models(&originals.resource_index, &mut meshes) {
            Ok(m) => Arc::new(m),
            Err(e) => {
                warn!("rigid handle models: {e}");
                return;
            }
        };
        let look = handle_look(
            [HANDLE_COLOR, HANDLE_ROLLOVER, HANDLE_ACTIVE],
            STATE_ALPHA,
            STATE_OVERDRAW_ALPHA,
            &mut materials,
            &mut overdraw_assets,
        );
        rigid.handle_assets = Some(HandleAssets { models, look });
    }
    let active = rigid.grab.as_ref().and_then(|g| g.kind());
    let hover = rigid.hover;
    let state_of = |kind: HandleKind| -> usize {
        if active == Some(kind) {
            2
        } else if active.is_none() && hover == Some(kind) {
            1
        } else {
            0
        }
    };
    let deform_scale = if rigid.kind.is_building() { BUILDING_DEFORM_SCALE } else { DEFORM_SCALE };
    if rigid.handle_entities.len() != specs.len() || rigid.handle_entities.iter().zip(&specs).any(|(a, b)| a.1 != b.kind) {
        for (e, _) in std::mem::take(&mut rigid.handle_entities) {
            commands.entity(e).despawn();
        }
        let assets = rigid.handle_assets.as_ref().unwrap();
        let mut entities = Vec::with_capacity(specs.len());
        for spec in &specs {
            let mesh = handle_model(spec.kind, &assets.models).mesh.clone();
            // The models with an overdraw model (`tuningEditorHandleHasOverdraw`): the deform
            // handle and the ball connector's sphere; the rotation ring has none.
            let over = match spec.kind {
                HandleKind::Morph(_) => Some(assets.models.teardrop_overdraw.mesh.clone()),
                HandleKind::Stretch(..) => Some(assets.models.sphere_overdraw.mesh.clone()),
                HandleKind::Turn(_) => None,
            };
            let e = commands
                .spawn((
                    crate::stages::StageOwned,
                    RigidHandle,
                    Mesh3d(mesh),
                    MeshMaterial3d(assets.look.main[state_of(spec.kind)].clone()),
                    Transform::default(),
                    bevy::light::NotShadowCaster,
                    Name::new(format!("rigid handle {:?}", spec.kind)),
                ))
                .id();
            if let Some(over) = over {
                commands.spawn((
                    crate::stages::StageOwned,
                    RigidHandleOverdraw,
                    ChildOf(e),
                    Mesh3d(over),
                    MeshMaterial3d(assets.look.overdraw[state_of(spec.kind)].clone()),
                    Transform::default(),
                    bevy::light::NotShadowCaster,
                ));
            }
            entities.push((e, spec.kind));
        }
        rigid.handle_entities = entities;
        return;
    }
    let assets = rigid.handle_assets.as_ref().unwrap();
    for (spec, (entity, _)) in specs.iter().zip(&rigid.handle_entities) {
        if let Ok(mut t) = transforms.get_mut(*entity) {
            let local = handle_model_transform(spec, &assets.models, deform_scale, view_distance);
            t.translation = root.translation + root.rotation * local.translation;
            t.rotation = root.rotation * local.rotation;
            t.scale = local.scale;
        }
        if let Ok(mut m) = handle_materials.get_mut(*entity) {
            let want = &assets.look.main[state_of(spec.kind)];
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
    }
    for (parent, mut m) in &mut overdraw_materials {
        if let Some((_, kind)) = rigid.handle_entities.iter().find(|(e, _)| *e == parent.parent()) {
            let want = &assets.look.overdraw[state_of(*kind)];
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_hull(min: Vec3, max: Vec3) -> Hull {
        let c = [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(min.x, max.y, max.z),
        ];
        let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [3, 7, 6, 2], [0, 4, 7, 3], [1, 2, 6, 5]];
        let mut tris = vec![];
        for q in quads {
            tris.push([c[q[0]], c[q[1]], c[q[2]]]);
            tris.push([c[q[0]], c[q[2]], c[q[3]]]);
        }
        Hull { tris, min, max }
    }

    fn moving<'a>(block: &'a RigidBlock, hull: &Hull, orient: bool) -> Moving<'a> {
        Moving { block, unit: 1.0, hull_center: hull.center(), hull_half: hull.half(), orient, upright: false, plan: OrientPlan::default(), input: None, min_y: -10.0, max_y: 10.0 }
    }

    #[test]
    fn morph_handles_sit_on_their_lines_and_replace_the_stretch_spheres() {
        use crate::creature_handles::{HandleDef, PartHandles};
        let part = PartHandles {
            handles: vec![HandleDef { id: 7, start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 2.0), default: 0.5 }],
            rings: [false; 3],
            basis: Quat::IDENTITY,
            offset: Vec3::ZERO,
        };
        let pose = Transform::from_translation(Vec3::new(1.0, 0.0, 0.0));
        // The default weight: the middle of the line; the block's own weight moves it.
        let at_default = morph_specs(&pose, &part, &[], 10.0);
        assert_eq!(at_default.len(), 1);
        assert!((at_default[0].position - Vec3::new(1.0, 0.0, 1.0)).length() < 1e-5);
        let moved = morph_specs(&pose, &part, &[(7, 1.0)], 10.0);
        assert!((moved[0].position - Vec3::new(1.0, 0.0, 2.0)).length() < 1e-5);
        assert_eq!(morph_line(&pose, &part, 0), Some((Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 2.0))));
        assert_eq!((morph_weight(&[(7, 0.25)], 7, 0.5), morph_weight(&[], 7, 0.5)), (0.25, 0.5));
        // With morph handles the six stretch spheres go (the rings stay); without,
        // the stand-in spheres remain.
        let hull = box_hull(Vec3::splat(-1.0), Vec3::splat(1.0));
        let count = |specs: &[HandleSpec], f: fn(&HandleKind) -> bool| specs.iter().filter(|s| f(&s.kind)).count();
        let stretch = |k: &HandleKind| matches!(k, HandleKind::Stretch(..));
        let plain = block_specs(&pose, &hull, None, &[], 10.0);
        assert_eq!(count(&plain, stretch), 6);
        let morphable = block_specs(&pose, &hull, Some(&part), &[], 10.0);
        assert_eq!(count(&morphable, stretch), 0);
        assert_eq!(count(&morphable, |k| matches!(k, HandleKind::Turn(_))), 3);
        assert_eq!(count(&morphable, |k| matches!(k, HandleKind::Morph(0))), 1);
        // The drag rule is the creature's (FUN_005ACFC0): a ray through the handle line's end reads 1.
        let w = deform_weight(Vec3::new(1.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 2.0), 0.0, 1.0);
        assert!(w.is_none() || w.is_some_and(|w| (0.0..=1.0).contains(&w)));
    }

    /// Which installed building / vehicle parts have morph handles:
    /// `cargo test rigid_editor_edit::tests::installed_rigid_parts_morph_handles -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_rigid_parts_morph_handles() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        for (group, label) in [(crate::asset_catalog::BUILDING_PARTS, "building"), (crate::asset_catalog::VEHICLE_PARTS, "vehicle")] {
            let (mut parts, mut morphable) = (0, 0);
            for def in catalog.parts.iter().filter(|d| d.key.group_id == group && !d.is_null) {
                parts += 1;
                let handles = load_part_handles(&index, def);
                if !handles.handles.is_empty() {
                    morphable += 1;
                    println!("{label} {} ({:08X}): {} morph handle(s) {:?}", def.name, def.key.instance_id, handles.handles.len(), handles.handles.iter().map(|h| (h.id, h.default)).collect::<Vec<_>>());
                }
            }
            println!("{label}: {morphable} of {parts} parts have morph handles");
        }
    }

    #[test]
    fn a_box_rests_on_the_floor_and_on_the_top_of_another() {
        let hull = box_hull(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 1.0, 2.0));
        let floor = Surface { point: Vec3::new(0.5, 0.0, 0.5), normal: Vec3::Y, t: 1.0, block: None };
        let b = RigidBlock::default();
        let p = pin(&floor, &moving(&b, &hull, false));
        // The pivot is at the bottom of the box, so it rests on the floor.
        assert!((p.position().y).abs() < 1e-5, "{:?}", p.position());
        assert!((p.position().x - 0.5).abs() < 1e-5);
        // On top of a block 1 high: the box's bottom is at the hit height.
        let top = Surface { point: Vec3::new(0.0, 1.0, 0.0), normal: Vec3::Y, t: 1.0, block: Some(0) };
        let p = pin(&top, &moving(&b, &hull, false));
        assert!((p.position().y - 1.0).abs() < 1e-5);
        // Against a vertical face the box stands beside it.
        let side = Surface { point: Vec3::new(1.0, 0.5, 0.0), normal: Vec3::X, t: 1.0, block: Some(0) };
        let p = pin(&side, &moving(&b, &hull, false));
        assert!((p.position().x - 2.0).abs() < 1e-5, "{:?}", p.position());
    }

    #[test]
    fn an_oriented_part_stands_out_of_the_surface_with_its_origin_on_it() {
        let hull = box_hull(Vec3::new(-0.2, -0.2, 0.0), Vec3::new(0.2, 0.2, 0.7));
        let s = Surface { point: Vec3::new(1.0, 0.5, 0.0), normal: Vec3::X, t: 1.0, block: Some(0) };
        let b = RigidBlock::default();
        let p = pin(&s, &moving(&b, &hull, true));
        assert!(p.position().distance(Vec3::new(1.0, 0.5, 0.0)) < 1e-5);
        let out = p.rotation() * Vec3::Z;
        assert!(out.distance(Vec3::X) < 1e-5, "{out}");
        // On the floor it stands up.
        let f = Surface { point: Vec3::ZERO, normal: Vec3::Y, t: 1.0, block: None };
        let p = pin(&f, &moving(&b, &hull, true));
        assert!((p.rotation() * Vec3::Z).distance(Vec3::Y) < 1e-5);
    }

    #[test]
    fn height_limits_keep_the_box_inside() {
        let hull = box_hull(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let b = RigidBlock::default();
        let low = Surface { point: Vec3::new(0.0, -3.0, 0.0), normal: Vec3::Y, t: 1.0, block: None };
        let m = Moving { min_y: 0.0, ..moving(&b, &hull, false) };
        assert!((pin(&low, &m).position().y).abs() < 1e-5);
        let high = Surface { point: Vec3::new(0.0, 20.0, 0.0), normal: Vec3::Y, t: 1.0, block: None };
        let m = Moving { max_y: 10.0, ..moving(&b, &hull, false) };
        assert!((pin(&high, &m).position().y - 9.0).abs() < 1e-5);
    }

    #[test]
    fn rays_pick_the_nearest_block_or_the_floor() {
        let hull = Arc::new(box_hull(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0)));
        let blocks = vec![
            (Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)), Some(hull.clone())),
            (Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)), Some(hull.clone())),
        ];
        // From above, onto the first box.
        let s = surface_under(Vec3::new(0.2, 5.0, 0.1), Vec3::NEG_Y, &blocks, &[], Some(0.0)).unwrap();
        assert_eq!(s.block, Some(0));
        assert!((s.point.y - 1.0).abs() < 1e-5 && s.normal.distance(Vec3::Y) < 1e-5);
        // Beside the boxes: the floor.
        let s = surface_under(Vec3::new(2.5, 5.0, 0.0), Vec3::NEG_Y, &blocks, &[], Some(0.0)).unwrap();
        assert_eq!(s.block, None);
        assert!(s.point.y.abs() < 1e-5);
        // The moved block is skipped.
        let s = surface_under(Vec3::new(0.2, 5.0, 0.1), Vec3::NEG_Y, &blocks, &[0], Some(0.0)).unwrap();
        assert_eq!(s.block, None);
        // A scaled and rotated block: the ray is tested in its own space.
        let t = Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)).with_scale(Vec3::splat(2.0));
        let s = surface_under(Vec3::new(0.0, 9.0, 0.0), Vec3::NEG_Y, &[(t, Some(hull.clone()))], &[], None).unwrap();
        assert!((s.point.y - 4.0).abs() < 1e-4, "{:?}", s.point);
        // No floor hit when the ray looks up.
        assert!(surface_under(Vec3::new(0.0, 1.0, 0.0), Vec3::Y, &[], &[], Some(0.0)).is_none());
    }

    #[test]
    fn the_twin_follows_off_the_plane_and_goes_on_it() {
        let mut run = RunProgress::default();
        run.parts.clear();
        let mut p = PartInstance::new(5, 0).with_definition(5);
        p.rigid = Some({
            let mut b = RigidBlock::at(Vec3::new(1.0, 0.5, 2.0));
            b.set_rotation(Quat::from_rotation_y(0.4));
            b
        });
        run.parts.push(p);
        let i = settle(&mut run, 0, true, 0.06);
        assert_eq!((i, run.parts.len()), (0, 2));
        let (a, b) = (&run.parts[0], &run.parts[1]);
        assert!(a.pair_id != 0 && a.pair_id == b.pair_id);
        assert!(b.mirrored && !a.mirrored);
        assert_eq!(b.rigid.as_ref().unwrap().position, [-1.0, 0.5, 2.0]);
        // The wheel/handles keep the pair together.
        run.parts[0].scale = 1.5;
        sync_twin(&mut run, 0);
        assert_eq!(run.parts[1].scale, 1.5);
        // Dragging the twin onto the plane leaves one centred block.
        run.parts[1].rigid = Some(RigidBlock::at(Vec3::new(0.02, 0.5, 2.0)));
        let i = settle(&mut run, 1, true, 0.06);
        assert_eq!((i, run.parts.len()), (0, 1));
        assert_eq!(run.parts[0].rigid.as_ref().unwrap().position[0], 0.0);
        assert_eq!(run.parts[0].pair_id, 0);
        assert!(!run.parts[0].mirrored);
        // Without symmetry (the buildings) nothing is mirrored.
        run.parts[0].rigid = Some(RigidBlock::at(Vec3::new(3.0, 0.5, 2.0)));
        settle(&mut run, 0, false, 0.06);
        assert_eq!(run.parts.len(), 1);
    }

    #[test]
    fn handles_surround_the_block_and_are_picked_on_screen() {
        let hull = box_hull(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 1.0, 2.0));
        let pose = Transform::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let specs = handle_specs(&pose, &hull, 10.0);
        assert_eq!(specs.len(), 9);
        let right = specs.iter().find(|s| s.kind == HandleKind::Stretch(0, 1)).unwrap();
        assert!((right.position.x - (1.0 + 0.2 * 1.6)).abs() < 1e-5, "{:?}", right.position);
        assert!(right.axis.distance(Vec3::X) < 1e-6);
        let ring = specs.iter().find(|s| s.kind == HandleKind::Turn(1)).unwrap();
        assert!((ring.rotation * Vec3::Y).distance(Vec3::Y) < 1e-6);
        assert!(ring_points(ring, 8).iter().all(|p| ((*p - ring.position).length() - ring.radius).abs() < 1e-4));
        // A top-down "screen": x, z map to pixels (10 px per unit).
        let project = |p: Vec3| Some(Vec2::new(p.x * 10.0 + 100.0, p.z * 10.0));
        let at = project(right.position).unwrap();
        let hit = pick_handle(&specs, at + Vec2::new(3.0, 2.0), &project, None).unwrap();
        assert_eq!(hit.0, HandleKind::Stretch(0, 1));
        assert!(pick_handle(&specs, Vec2::new(-500.0, -500.0), &project, None).is_none());
        // On the y ring (seen from above it is a circle) the tangent is
        // screen-space and the radius in pixels.
        let on_ring = project(ring_points(ring, 64)[0]).unwrap();
        let (kind, tangent, radius) = pick_handle(&specs, on_ring, &project, None).unwrap();
        assert_eq!(kind, HandleKind::Turn(1));
        assert!((tangent.length() - 1.0).abs() < 1e-3 && radius >= 24.0);
        // The closest point on a line to a ray.
        let t = line_param(Vec3::ZERO, Vec3::X, Vec3::new(3.0, 5.0, 0.0), Vec3::NEG_Y).unwrap();
        assert!((t - 3.0).abs() < 1e-5);
        assert!(line_param(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X).is_none());
    }

    #[test]
    fn handle_models_sit_on_their_lines_and_are_picked_by_triangles() {
        use crate::creature_handles::{HandleModel, RawHandleModel};
        let mut meshes = Assets::<Mesh>::default();
        // A teardrop stand-in: a thin triangle along -Y from the origin; a ring stand-in of radius 1.
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| RawHandleModel { positions: vec![a, b, c], normals: vec![[0.0, 1.0, 0.0]; 3], indices: vec![0, 1, 2] };
        let drop = HandleModel::from_raw(&tri([0.0, 0.0, 0.0], [0.03, -0.15, 0.0], [-0.03, -0.15, 0.0]), &mut meshes);
        let ring = HandleModel::from_raw(&tri([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]), &mut meshes);
        let ball = HandleModel::from_raw(&tri([0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]), &mut meshes);
        let models = HandleModels { teardrop: drop.clone(), teardrop_overdraw: drop, sphere: ball.clone(), sphere_overdraw: ball, ring };
        let hull = box_hull(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 1.0, 2.0));
        let pose = Transform::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let part = PartHandles {
            handles: vec![crate::creature_handles::HandleDef { id: 7, start: Vec3::new(0.0, 0.0, 0.0), end: Vec3::new(0.0, 0.0, 2.0), default: 0.5 }],
            basis: Quat::IDENTITY,
            offset: Vec3::ZERO,
            rings: [false; 3],
        };
        let specs = block_specs(&pose, &hull, Some(&part), &[], 10.0);
        let morph = specs.iter().find(|s| matches!(s.kind, HandleKind::Morph(_))).unwrap();
        // The teardrop is turned along the line (model Y -> the line), scaled by 1.3 at the creature editor's zoom (5).
        let t = handle_model_transform(morph, &models, DEFORM_SCALE, 5.0);
        assert!((t.scale.x - 1.3).abs() < 1e-5, "{:?}", t.scale);
        assert!((t.rotation * Vec3::Y).distance(morph.axis) < 1e-5);
        // The buildings' tuning is 1.2 and a farther camera scales the handle up to keep its size on screen.
        assert!((handle_model_transform(morph, &models, BUILDING_DEFORM_SCALE, 10.0).scale.x - 2.4).abs() < 1e-5);
        // The ray picks the triangle (its tip lies behind the origin along -Y, in model space).
        let model = handle_model(morph.kind, &models);
        let hit = model.pick(&t, morph.position + Vec3::new(0.0, 5.0, 0.0) - t.rotation * Vec3::new(0.0, -0.03, 0.0), Vec3::NEG_Y);
        assert!(hit.is_none() || hit.unwrap() > 0.0);
        let on = t.transform_point(Vec3::new(0.0, -0.06, 0.0));
        assert!(model.pick(&t, on + Vec3::new(0.0, 0.0, 0.0) + t.rotation * Vec3::new(0.0, 0.0, 1.0), -(t.rotation * Vec3::Z)).is_some(), "a ray through the model hits");
        assert!(model.pick(&t, on + t.rotation * Vec3::new(0.5, 0.0, 1.0), -(t.rotation * Vec3::Z)).is_none(), "a ray beside it misses (no pick radius)");
        // The hits drive `pick_handle`: only the handle under the ray is picked, whatever the screen distance.
        let project = |p: Vec3| Some(Vec2::new(p.x * 10.0 + 100.0, p.z * 10.0));
        let target = morph.kind;
        let only = |s: &HandleSpec| (s.kind == target).then_some(1.0);
        let far = Vec2::new(-400.0, -400.0);
        assert_eq!(pick_handle(&specs, far, &project, Some(&only)).map(|p| p.0), Some(target));
        assert!(pick_handle(&specs, far, &project, Some(&|_: &HandleSpec| None)).is_none());
        // A ring hit supplies the screen tangent and radius of the ring at the cursor.
        let ring = specs.iter().find(|s| s.kind == HandleKind::Turn(1)).unwrap().kind;
        let on_ring = project(ring_points(specs.iter().find(|s| s.kind == ring).unwrap(), 64)[0]).unwrap();
        let (kind, tangent, radius) = pick_handle(&specs, on_ring, &project, Some(&|s: &HandleSpec| (s.kind == ring).then_some(2.0))).unwrap();
        assert_eq!(kind, ring);
        assert!((tangent.length() - 1.0).abs() < 1e-3 && radius >= 24.0);
    }

    #[test]
    fn a_new_block_rests_on_the_floor() {
        let hull = box_hull(Vec3::new(-1.0, -0.5, -2.0), Vec3::new(1.0, 0.5, 2.0));
        let b = new_block(hull.center(), hull.half(), 2.0, 0.0);
        // The box's bottom (y = -0.5 * 2 below the pivot) is on the floor.
        assert!((b.position().y - 1.0).abs() < 1e-5);
        assert!(b.position().x.abs() < 1e-5 && b.position().z.abs() < 1e-5);
    }

    #[test]
    fn selling_refunds_both_halves_of_a_pair() {
        let mut data = CreatureEditorData::default();
        let mut info = crate::creature_editor_data::CreaturePartInfo {
            key: crate::dbpf::ResourceKey { type_id: 0, group_id: 0, instance_id: 7 },
            name: "wheel".into(),
            price: 200,
            complexity: 2,
            bones: 0,
            bake_level: 0,
            geom: 0.0,
            unlock_level: 1,
            find_percentage: 1.0,
            min_scale: 0.5,
            max_scale: 2.0,
            sell_back: 1.0,
            icon: crate::dbpf::ResourceKey { type_id: 0, group_id: 0, instance_id: 0 },
            model: 70,
            abilities: vec![],
            carnivore: 0,
            herbivore: 0,
            mouth: false,
            orient_to_surface: true,
            prefer_symmetry_plane: false,
            symmetry_snap_delta: None,
            acts_like_foot: false,
            acts_like_grasper: false,
            starts_unlocked: true,
        };
        info.price = 200;
        data.parts.insert(7, info);
        let mut run = RunProgress::default();
        run.parts = vec![
            PartInstance { pair_id: 1, ..PartInstance::new(70, 0).with_definition(7) },
            PartInstance { pair_id: 1, mirrored: true, ..PartInstance::new(70, 0).with_definition(7) },
            PartInstance::new(70, 0).with_definition(7),
        ];
        run.editor_dna = 1000.0;
        sell(&mut run, &data, 0);
        assert_eq!(run.parts.len(), 1);
        assert_eq!(run.editor_dna, 1200.0, "a pair is one refund");
        sell(&mut run, &data, 0);
        assert!(run.parts.is_empty());
        assert_eq!(run.editor_dna, 1400.0);
    }
    fn linked(id: u32, parent: u32, at: Vec3) -> PartInstance {
        let mut p = PartInstance::new(id, 0);
        p.rigid = Some(RigidBlock { id, parent, ..RigidBlock::at(at) });
        p
    }

    fn model() -> crate::OriginalModel {
        crate::OriginalModel { source_definition: None, material: None, handle: Handle::default(), surfaces: vec![], scale: 5.0, instance: 0, root: None, rig_bones: 0 }
    }

    #[test]
    fn a_released_block_needs_a_root_a_structure_may_stand_alone_a_window_may_not() {
        use crate::rigid_editor_stack::TypeKey;
        let key = |instance| TypeKey { group: 1, instance, kind: 0 };
        let structure = Arc::new(StackRules { block_type: key(10), can_be_parentless: true, ..default() });
        let window = Arc::new(StackRules { block_type: key(50), ..default() });
        let rules = vec![Some(structure), Some(window.clone()), Some(window)];
        let mut run = RunProgress::default();
        // The window (2) is stored on the structure (1); the other window (3) stands on nothing.
        run.parts = vec![linked(1, 0, Vec3::ZERO), linked(2, 1, Vec3::new(2.05, 1.5, 0.0)), linked(3, 0, Vec3::new(9.0, 1.5, 0.0))];
        assert!(release_attached(&run, &rules, 0), "a structure on the studio base");
        assert!(release_attached(&run, &rules, 1), "a window on the wall of a structure");
        assert!(!release_attached(&run, &rules, 2), "a window out in the open has no root");
        // The stored link decides, not the geometry: the lone window stored on the first one reaches the root.
        run.parts[2].rigid.as_mut().unwrap().parent = 2;
        assert!(release_attached(&run, &rules, 2));
        // A window whose parent was sold (the link is cleared) has no root again.
        run.parts.remove(0);
        crate::rigid_editor_stack::prune_links(&mut run.parts);
        assert!(!release_attached(&run, &rules[1..], 0));
    }

    #[test]
    fn a_placed_block_stores_what_it_stands_on_and_a_moved_parent_carries_its_children() {
        let wall = Arc::new(box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0)));
        let small = Arc::new(box_hull(Vec3::splat(-0.2), Vec3::splat(0.2)));
        let mut run = RunProgress::default();
        run.parts = vec![linked(1, 0, Vec3::ZERO), linked(2, 0, Vec3::new(8.0, 0.2, 0.0))];
        let rules: Vec<Option<Arc<StackRules>>> = vec![None, None];
        let env = StackEnv { floor: Some(0.0), ..default() };
        let poses = |run: &RunProgress| -> Vec<(Transform, Option<Arc<Hull>>)> {
            run.parts.iter().enumerate().map(|(i, p)| (p.rigid.as_ref().unwrap().transform(1.0), Some(if i == 0 { wall.clone() } else { small.clone() }))).collect()
        };
        // Drop the small block on the wall's top face: the wall is its parent.
        let top = Surface { point: Vec3::new(0.0, 3.0, 0.0), normal: Vec3::Y, t: 1.0, block: Some(0) };
        let (p, skip) = (poses(&run), [1]);
        let stack = StackInput::of(&rules, 1, &top, &p, &parent_table(&run.parts), &skip, env);
        let at = place_on_stacked(&mut run, 1, &top, &model(), &small, None, false, (-10.0, 10.0), &stack);
        assert_eq!(at, 1);
        let on_wall = run.parts[1].rigid.as_ref().unwrap();
        assert_eq!((on_wall.id, on_wall.parent), (2, 1), "the block it was dropped on is stored as its parent");
        assert!((on_wall.position().y - 3.2).abs() < 1e-4, "{:?}", on_wall.position());
        // Drag the wall to another place on the studio base: no block is hit, the pose is the free one.
        let floor = Surface { point: Vec3::new(5.0, 0.0, 0.0), normal: Vec3::Y, t: 1.0, block: None };
        let (p, skip) = (poses(&run), moving_set(&run, 0));
        assert_eq!(skip, vec![0, 1], "the block and what stands on it");
        let mut stack = StackInput::of(&rules, 0, &floor, &p, &parent_table(&run.parts), &skip, env);
        stack.free = Some(FreePose { position: Vec3::new(5.0, 1.5, 0.0), on_ground: true });
        place_on_stacked(&mut run, 0, &floor, &model(), &wall, None, false, (-10.0, 10.0), &stack);
        let wall_at = run.parts[0].rigid.as_ref().unwrap().position();
        assert_eq!(run.parts[0].rigid.as_ref().unwrap().parent, 0, "no block hit: parentless");
        assert!((wall_at - Vec3::new(5.0, 1.5, 0.0)).length() < 1e-4, "the free pose, not the surface point: {wall_at:?}");
        let child = run.parts[1].rigid.as_ref().unwrap();
        assert!((child.position() - Vec3::new(5.0, 4.7, 0.0)).length() < 1e-3, "the block on the wall went along: {:?}", child.position());
        assert_eq!(child.parent, 1, "and still stands on it");
        // The block that stood on nothing stays where it was.
        assert_eq!(run.parts.len(), 2);
        // Turning the wall turns the block on it about the wall's origin.
        turn_block(&mut run, 0, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let child = run.parts[1].rigid.as_ref().unwrap();
        assert!((child.position() - Vec3::new(5.0, 4.7, 0.0)).length() < 1e-3, "on the axis it turns about: {:?}", child.position());
        let mut side = linked(3, 1, Vec3::new(6.0, 1.5, 0.0));
        side.pair_id = 0;
        run.parts.push(side);
        turn_block(&mut run, 0, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let carried = run.parts[2].rigid.as_ref().unwrap().position();
        assert!((carried - Vec3::new(5.0, 1.5, -1.0)).length() < 1e-3, "a block one unit along +x goes round to -z: {carried:?}");
    }

    #[test]
    fn a_block_without_a_parent_follows_the_ray_on_the_plane_chosen_at_the_start_of_the_drag() {
        let wall = Arc::new(box_hull(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0)));
        let small = Arc::new(box_hull(Vec3::splat(-0.2), Vec3::splat(0.2)));
        let mut run = RunProgress::default();
        // The structure (1) rests on the ground, its window (2) stands on it, a loose block (3) lies far to the +x side.
        run.parts = vec![linked(1, 0, Vec3::ZERO), linked(2, 1, Vec3::new(2.0, 1.5, 0.0)), linked(3, 0, Vec3::new(8.0, 0.2, -4.0))];
        let poses: Vec<(Transform, Option<Arc<Hull>>)> = run
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.rigid.as_ref().unwrap().transform(1.0), Some(if i == 0 { wall.clone() } else { small.clone() })))
            .collect();
        // A camera in front (editor +z) and above looking down at the structure.
        let origin = Vec3::new(0.0, 8.0, 10.0);
        let dir = Vec3::new(0.0, -0.6, -0.8);
        let mut plane = None;
        // Drag the structure: it rests on the ground and its origin is on its box bottom, so its origin follows the
        // ray at height 0.
        let pose = free_pose(&run, 0, &poses, &mut plane, origin, dir, 0.0, 20.0, false).unwrap();
        assert!(pose.on_ground, "the ray point at the standing height is above the plane point");
        assert!(pose.position.y.abs() < 1e-4, "the origin of a block whose box starts at its origin stands at the floor: {:?}", pose.position);
        // The ray from 8 up and 10 in front reaches the floor 8 / 0.6 x 0.8 = 10.67 ahead.
        assert!((pose.position.z - (10.0 - 0.8 * (8.0 / 0.6))).abs() < 1e-3, "{:?}", pose.position);
        // The plane was chosen once: it faces the camera (its normal is retail -y of the ray's horizontal part = editor +z)
        // through the nearest other block's origin (the window is a descendant and is left out; the loose block is next).
        let chosen = plane.unwrap();
        assert!((chosen.point - to_retail(Vec3::new(8.0, 0.2, -4.0))).length() < 1e-4, "{:?}", chosen.point);
        assert!((from_retail(chosen.normal) - Vec3::Z).length() < 1e-4, "{:?}", from_retail(chosen.normal));
        // The plane is kept while the block stays parentless: a second ray reuses it.
        let again = free_pose(&run, 0, &poses, &mut plane, origin + Vec3::X, dir, 0.0, 20.0, false).unwrap();
        assert!((again.position.x - 1.0).abs() < 1e-3, "follows the ray sideways: {:?}", again.position);
        assert_eq!(plane, Some(chosen));
    }

    #[test]
    fn a_ray_passes_through_blocks_the_part_cannot_interact_with() {
        let hull = Arc::new(box_hull(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0)));
        let blocks = vec![(Transform::IDENTITY, Some(hull))];
        let origin = Vec3::new(0.0, 5.0, 0.0);
        let hit = surface_under_where(origin, Vec3::NEG_Y, &blocks, &[], Some(0.0), &|_| true).unwrap();
        assert_eq!(hit.block, Some(0));
        let through = surface_under_where(origin, Vec3::NEG_Y, &blocks, &[], Some(0.0), &|_| false).unwrap();
        assert_eq!(through.block, None, "the refused block is not hit: the studio base is");
        assert!(through.point.y.abs() < 1e-5);
    }

    #[test]
    fn a_point_forward_part_on_the_side_of_a_body_points_its_x_axis_forward_and_the_left_half_the_other_way() {
        let hull = box_hull(Vec3::new(-0.5, 0.0, -0.1), Vec3::new(0.5, 1.0, 0.1));
        // Rolled about its mount axis by the user: the rules turn it to the forward axis anyway.
        let mut b = RigidBlock::default();
        b.set_rotation(Quat::from_rotation_z(0.6));
        // A wheel on the right of the car: the surface faces +X, the mount axis (+Z) follows it.
        let wall = Surface { point: Vec3::new(2.0, 0.5, 0.0), normal: Vec3::X, t: 1.0, block: Some(0) };
        let mut m = moving(&b, &hull, true);
        m.plan = OrientPlan { has_parent: true, point_forward: true, remain_upright: true, side: 1, ..default() };
        let right = pin(&wall, &m).rotation();
        // Retail forward is the editor's -Z: the part's x axis runs along it, the mount axis still stands out of the wall.
        assert!((right * Vec3::X).dot(Vec3::NEG_Z) > 0.999, "{:?}", right * Vec3::X);
        assert!((right * Vec3::Z).dot(Vec3::X) > 0.999, "{:?}", right * Vec3::Z);
        assert!((right * Vec3::Y).dot(Vec3::Y) > 0.999, "{:?}", right * Vec3::Y);
        // The left half's twin (its surface faces -X) points the other way: row 0 = -forward.
        let left_wall = Surface { point: Vec3::new(-2.0, 0.5, 0.0), normal: Vec3::NEG_X, ..wall };
        m.plan.side = -1;
        let left = pin(&left_wall, &m).rotation();
        assert!((left * Vec3::X).dot(Vec3::Z) > 0.999, "{:?}", left * Vec3::X);
        assert!((left * Vec3::Z).dot(Vec3::NEG_X) > 0.999);
        // Without a parent no rule runs.
        m.plan.has_parent = false;
        let free = pin(&wall, &m).rotation();
        assert!((free * Vec3::X).dot(Vec3::NEG_Z) < 0.999);
    }

    #[test]
    fn an_upright_oriented_part_keeps_its_up_axis_vertical_on_a_wall_and_on_a_top_face() {
        let hull = box_hull(Vec3::new(-0.5, 0.0, -0.1), Vec3::new(0.5, 1.0, 0.1));
        // A part standing on its base, turned about the vertical axis: retail's rule keeps its right axis
        // (row 0), so its up axis comes out exactly vertical.
        let mut b = RigidBlock::at(Vec3::ZERO);
        b.set_rotation(Quat::from_rotation_y(0.4));
        // On a wall facing +X the mount axis (+Z) follows the normal and the up axis stays vertical.
        let wall = Surface { point: Vec3::new(2.0, 1.0, 0.0), normal: Vec3::X, t: 1.0, block: Some(0) };
        let mut m = moving(&b, &hull, true);
        m.upright = true;
        let on_wall = pin(&wall, &m);
        assert!((on_wall.rotation() * Vec3::Z).dot(Vec3::X) > 0.999, "{:?}", on_wall.rotation() * Vec3::Z);
        assert!((on_wall.rotation() * Vec3::Y).dot(Vec3::Y) > 0.999, "{:?}", on_wall.rotation() * Vec3::Y);
        // Without the flag a part that was tilted about its mount axis leans.
        let mut tilted = RigidBlock::at(Vec3::ZERO);
        tilted.set_rotation(Quat::from_rotation_y(0.4) * Quat::from_rotation_z(0.3));
        let lean = moving(&tilted, &hull, true);
        let leaning = pin(&wall, &lean);
        assert!((leaning.rotation() * Vec3::Y).dot(Vec3::Y) < 0.999);
        // On a top face the oriented part is tipped onto its back by the surface rule and stood up again.
        let top = Surface { point: Vec3::new(0.0, 1.0, 0.0), normal: Vec3::Y, t: 1.0, block: Some(0) };
        m.upright = true;
        let on_top = pin(&top, &m);
        assert!((on_top.rotation() * Vec3::Y).dot(Vec3::Y) > 0.999, "{:?}", on_top.rotation() * Vec3::Y);
    }
}
