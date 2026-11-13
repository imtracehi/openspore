//! Procedural walk cycle on native skin weights for installed creature GMDLs.
//!
//! This is not a decoded Spore motion; native ANIM evaluation stays separate
//! (see `creature_motion`). The same-instance CreatureData anatomy names the
//! spine (block type 5), limb segments (3), rigged parts (2) and decorations
//! (1). GMDL root palette slots follow the non-decoration rigblocks in order,
//! and every later binding group belongs to the next part/decoration
//! rigblock (the order the game builds its bone groups in, FUN_00742E30;
//! keyed part bindings name their part, checked on the installed models).
//! Limb chains whose vertices reach the ground are legs: each foot follows a
//! planted stance and raised swing, solved with planar two-bone IK about the
//! native joint pivots. Sides alternate (diagonal pairs for more legs) and the
//! remaining limbs counter-swing. NPC bodies also bob, sway, bank and breathe
//! on their visual children, because terrain grounding owns the actor root's
//! height. Gait tuning is adapted, not recovered Spore data.
//!
//! Clips (`creature_clips`): when the rig also has a clip rig (the anatomy's
//! rigblocks and capability table), the installed clips pose every rigblock
//! instead of the procedural gait: idle, walk / run speed-matched to the
//! ground speed, a bite when the creature attacks (its attack timer starts)
//! and the requested social action while it performs. Which installed models
//! get a rig: the starter (checked contract), and every species whose GMDL
//! binding has the retail structure (root slots = non-decoration rigblocks in
//! order, part bindings = part/decoration rigblocks in order, each keyed part
//! binding naming the instance of its rigblock: `part_bindings_match`).
//!
//! Deformation runs on the GPU: each source section gets one shared skinned
//! copy whose joints are the anatomy rigblocks (plus a fixed rest joint), and
//! each walking actor owns only a small set of joint entities under its first
//! visual child. Per frame the CPU poses a few dozen joints per actor; cached
//! original geometry is never modified.
use crate::{baked_model, creature_data::CreatureData, dbpf::ResourceKey, Originals};
use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    math::Affine3A,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::*,
};
use std::{
    collections::{HashMap, HashSet},
    f32::consts::{PI, TAU},
    sync::Arc,
};

/// Native rigblock kinds (CreatureData block type byte).
const SPINE: i8 = 5;
const LIMB: i8 = 3;
const PART: i8 = 2;
const DECORATION: i8 = 1;
/// A limb whose skinned vertices come this close to the model's lowest point
/// (as a fraction of model height) carries weight: it is a leg.
const LEG_GROUND_FRACTION: f32 = 0.1;
/// Joint rotations are clamped so an unreachable target cannot fold a limb.
const MAX_JOINT_ANGLE: f32 = 1.2;
/// Feet may overreach their straight-leg range by this factor; the body sink
/// at double support absorbs it.
const OVERREACH: f32 = 1.25;
/// The former ADAPTED gate: a fraction of the drawn vertices lying nearest
/// (among the three nearest pivots) to their owning rigblock. It rejected
/// 127 correctly bound models (limb vertices sit near the next joint,
/// decorations crowd the pivots); the structural binding replaced it and the
/// value is only logged and reported by the census.
#[cfg(test)]
const MIN_AGREEMENT: f32 = 0.7;
/// Bevy's GPU skinning palette limit.
const MAX_SKIN_JOINTS: usize = 256;
/// Pose level of detail: actors beyond this camera distance re-pose every
/// third frame, and actors outside the view cone keep their phase but are
/// not re-posed at all.
const FAR_POSE_DISTANCE: f32 = 40.;
const VIEW_HALF_ANGLE: f32 = 0.95;

/// Per-model gait rigs and shared skinned draw meshes, built once per source
/// GMDL. A `None` rig marks a model whose anatomy could not be bound to its
/// palette; its actors keep body motion only.
#[derive(Resource, Default)]
pub struct WalkTemplates {
    rigs: HashMap<ResourceKey, Option<Arc<GaitRig>>>,
    skins: HashMap<ResourceKey, SkinAssets>,
    frame: u32,
    /// Part rigs of the models' keyed bindings (`creature_morph`).
    part_rigs: crate::creature_morph::RigCache,
}

#[derive(Default)]
struct SkinAssets {
    inverse_bindposes: Option<Handle<SkinnedMeshInverseBindposes>>,
    /// Skinned variant per original surface mesh.
    meshes: HashMap<AssetId<Mesh>, Handle<Mesh>>,
}

#[derive(Clone, Copy, Debug)]
struct Joint {
    pivot: Vec3,
    parent: Option<usize>,
}

#[derive(Clone, Debug)]
struct Leg {
    hip: usize,
    knee: Option<usize>,
    foot: Option<usize>,
    /// Rest point that is planted on the ground (ankle or lowest limb point).
    effector: Vec3,
    /// Phase offset as a fraction of the cycle.
    offset: f32,
    /// Largest stance excursion this leg can reach ahead of / behind rest.
    reach: f32,
}

#[derive(Clone, Debug)]
struct Arm {
    root: usize,
    offset: f32,
    /// Swing scale: hanging arms swing fully, wings and raised limbs barely.
    hang: f32,
}

/// Joint palette for every vertex of one source section. Joint indices are
/// rigblock slots; vertices without a resolvable influence use the extra rest
/// joint at `joints.len()`.
#[derive(Clone, Debug)]
struct SectionSkin {
    section: usize,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

#[derive(Debug)]
struct GaitRig {
    joints: Vec<Joint>,
    /// Parents before children.
    order: Vec<usize>,
    legs: Vec<Leg>,
    arms: Vec<Arm>,
    /// Vertebrae trailing behind the rear-leg vertebra, base first.
    tail: Vec<usize>,
    /// Joints inside an animated leg, arm or tail subtree.
    moving: Vec<bool>,
    sections: Vec<SectionSkin>,
    /// Mean hip height above the model's ground, in model units.
    leg_length: f32,
    height: f32,
    /// Clip rig of the same rigblocks (`creature_clips`).
    clips: Option<Arc<crate::creature_clips::ClipRig>>,
    /// Fraction of drawn vertices nearest (within three) to their owner.
    agreement: f32,
    /// Part rig bones with their own joints (after the rigblocks and the rest
    /// joint), posed by the clips' deform channels (`creature_morph`).
    morph: Option<Arc<crate::creature_morph::MorphSet>>,
    /// Joints of the skinned copy: rigblocks, the rest joint, morph joints.
    joint_total: usize,
}

/// Stride shape for one pose, in model units.
#[derive(Clone, Copy, Debug, Default)]
struct Stride {
    /// Cycle position in radians.
    phase: f32,
    blend: f32,
    /// Body travel per full cycle.
    length: f32,
    /// Fraction of the cycle a foot is planted.
    duty: f32,
    /// Swing-foot clearance.
    lift: f32,
    /// Extra foot height compensating a body lowered outside skinning.
    sink: f32,
    /// Counter-swing amplitude for non-leg limbs (radians).
    swing: f32,
}

impl AdaptedWalk {
    /// The creature's clip layers (`anim_audio` plays their events).
    pub fn clip_player(&self) -> &crate::creature_clips::ClipPlayer {
        &self.clips.player
    }

    /// Move the remembered root position with the planet (the Creature
    /// stage's floating origin), so the turn does not read as a stride.
    pub fn shift(&mut self, map: impl Fn(Vec3) -> Vec3) {
        self.previous = map(self.previous);
    }
}

#[derive(Component, Default)]
pub struct AdaptedWalk {
    previous: Vec3,
    previous_yaw: f32,
    phase: f32,
    blend: f32,
    duty: f32,
    /// Smoothed ground speed, model units per second.
    speed: f32,
    /// Smoothed yaw rate (radians per second, positive turns left).
    turn: f32,
    clock: f32,
    seed: f32,
    /// Skeleton joints currently hold a non-rest pose.
    posed: bool,
    /// Clip state while clips pose the creature.
    clips: NpcClips,
    /// Clips moved every joint (not only the gait's).
    clip_posed: bool,
    /// Smoothed ground speed also while editing (Test Drive), model units/s.
    raw_speed: f32,
    /// The smoothed external look target (model frame, `creature_look`).
    look: Option<Vec3>,
}

/// A creature's clip layers and what started its actions.
#[derive(Default)]
struct NpcClips {
    player: crate::creature_clips::ClipPlayer,
    running: bool,
    attack: f32,
    /// The `CreatureMind::attack_serial` last seen.
    attack_serial: u32,
    social: f32,
    action: Option<u32>,
    /// The clip that follows ction (charge: the run-up, then the hit).
    then: Option<u32>,
    /// The installed starter as the avatar: its abilities and eating.
    avatar: crate::creature_clips::AvatarActions,
    /// Test Drive: the request's elapsed time last frame.
    elapsed: f32,
    /// Test Drive: the idle after a walk (`creature_clips::StopWalk`).
    stop: crate::creature_clips::StopWalk,
    /// The last `PoseClip::serial` started.
    pose_serial: u32,
    /// The overlay laid over the stack (`PoseOverlay`): the clip, its clock and
    /// how far it has faded in.
    overlay: Option<NpcOverlay>,
    /// The last `CreatureMind::cue_serial` started (a group social joiner
    /// copying its proposer's demonstration).
    cue_serial: u32,
    /// The creature was calling its herd last frame (group social).
    gathering: bool,
}

/// An overlay clip of one creature and its fade (see [`PoseOverlay`]).
struct NpcOverlay {
    id: u32,
    bound: Arc<crate::creature_clips::Bound>,
    time: f32,
    weight: f32,
}

/// Seconds an overlay takes to fade in or out (as `outfit_crowd`'s).
const OVERLAY_FADE: f32 = 0.25;

/// A clip laid over the creature's own clips on the joints it animates (the bundle
/// grasper's `platter_grasper_overlay` 0x05F08F08 over the walk of a creature that
/// carries something, `creature_clips::ClipRig::pose_layered`). Removing the
/// component fades it out.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PoseOverlay(pub u32);

/// A clip sequence asked for from outside (the Space comm portrait's emotion,
/// `space_hud::comm::portrait_emotion`): the action once, then the idle
/// looped; a new `serial` starts it again.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PoseClip {
    pub action: Option<u32>,
    pub idle: u32,
    pub serial: u32,
}

fn pose_clip_request(c: &mut NpcClips, originals: &Originals, p: &PoseClip) -> (u32, bool, bool) {
    let action = p.action.filter(|a| crate::creature_clips::clip(originals, *a).is_some());
    if c.pose_serial != p.serial {
        c.pose_serial = p.serial;
        c.action = action;
        return (action.unwrap_or(p.idle), action.is_none(), true);
    }
    if c.action.is_some() && c.player.current_id() == c.action && c.player.finished() {
        c.action = None;
    }
    match c.action {
        Some(a) => (a, false, false),
        None => (p.idle, true, false),
    }
}

/// The clip a creature plays: its attack ability's own clip when it attacks
/// (`CreatureMind::attack_clip`, the ability prop's
/// `creatureAbilityAnimationIDs`; a bite for an attack without one), its
/// requested social ability at its own level while it demonstrates, its
/// impatience (0x061E0670) while its side of the social bar drains, else
/// walk / run / idle by ground speed.
fn npc_request(
    c: &mut NpcClips,
    originals: &Originals,
    speed: f32,
    rig: &crate::creature_clips::ClipRig,
    mind: Option<&crate::creature_actions::CreatureMind>,
) -> (u32, bool, bool) {
    use crate::creature_actions::{ability_clip, EncounterMode};
    let mut restart = false;
    if let Some(m) = mind {
        let catalog = &originals.creature_abilities;
        if m.attack_serial != c.attack_serial {
            c.attack_serial = m.attack_serial;
            if m.attack_clip != 0 {
                // Charge runs up first (`creatureAbilityRushingAnimationID`).
                let lead = crate::creature_actions::lead_clip_of(catalog, m.attack_clip)
                    .filter(|lead| crate::creature_clips::clip(originals, *lead).is_some());
                c.action = Some(lead.unwrap_or(m.attack_clip));
                c.then = lead.map(|_| m.attack_clip);
                restart = true;
            }
        } else if m.attack_timer > c.attack + 0.25 && m.attack_clip == 0 {
            c.action = ability_clip(catalog, EncounterMode::Combat, 0, 1);
            restart = true;
        }
        c.attack = m.attack_timer;
        if m.response_time > crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT && m.response_time > c.social + 0.25 {
            c.action = ability_clip(catalog, EncounterMode::Social, m.requested as usize, m.requested_level.max(1));
            c.then = None;
            restart = true;
        }
        c.social = m.response_time;
        // Group socials (0xDA4950): the proposer calls (0x061E0673) and then
        // waits (0x061E0674, repeated) while its herd gathers; a joiner plays
        // the clip its proposer broadcast (its demonstration, 0xD9B190).
        let has = |id: u32| crate::creature_clips::clip(originals, id).is_some();
        if m.gather > 0.0 {
            if !c.gathering && has(crate::creature_rules::SOCIAL_CLIP_GROUP_CALL) {
                c.action = Some(crate::creature_rules::SOCIAL_CLIP_GROUP_CALL);
                c.then = has(crate::creature_rules::SOCIAL_CLIP_GROUP_WAIT)
                    .then_some(crate::creature_rules::SOCIAL_CLIP_GROUP_WAIT);
                restart = true;
            } else if c.action.is_none() && has(crate::creature_rules::SOCIAL_CLIP_GROUP_WAIT) {
                c.action = Some(crate::creature_rules::SOCIAL_CLIP_GROUP_WAIT);
                restart = true;
            }
        }
        c.gathering = m.gather > 0.0;
        if m.cue_serial != c.cue_serial {
            c.cue_serial = m.cue_serial;
            if m.cue_clip != 0 && has(m.cue_clip) {
                // A reaction asked for in the same frame as the next
                // demonstration (a wrong answer, 0xDA4CAA state 7 -> 4) plays
                // first; the demonstration follows.
                let follow = if restart { c.action.filter(|a| *a != m.cue_clip) } else { None };
                c.action = Some(m.cue_clip);
                c.then = follow;
                restart = true;
            }
        }
        if m.draining && c.action.is_none() && crate::creature_clips::clip(originals, crate::creature_rules::SOCIAL_CLIP_IMPATIENT).is_some() {
            c.action = Some(crate::creature_rules::SOCIAL_CLIP_IMPATIENT);
            restart = true;
        }
    }
    if !restart && c.action.is_some() && c.player.current_id() == c.action && c.player.finished() {
        c.action = c.then.take();
        restart = c.action.is_some();
    }
    if let Some(id) = c.action {
        return (id, false, restart);
    }
    (crate::creature_clips::locomotion_request(originals, speed, rig, &mut c.running), true, false)
}
/// Joint entities that drive an actor's skinned surfaces. They are children
/// of `holder` (the actor's first visual child), so body motion on that child
/// carries the whole skeleton.
#[derive(Component)]
pub struct GaitSkeleton {
    holder: Entity,
    joints: Vec<Entity>,
}

impl GaitSkeleton {
    /// The visual child the joints hang under.
    pub fn holder(&self) -> Entity {
        self.holder
    }
}
impl AdaptedWalk {
    /// Body-bob phase locked to the stride: two bobs per cycle, highest when
    /// a planted foot passes under the body (mid-stance).
    pub fn bob_phase(&self) -> f32 {
        2. * (self.phase - PI * self.duty) + std::f32::consts::FRAC_PI_2
    }
}
/// An NPC actor posed every frame wherever the game camera looks (the Space
/// comm screen's speaker portrait renders it with its own camera).
#[derive(Component)]
pub struct PoseAlways;
/// Marks a surface drawn through the shared skinned copy of `original`.
#[derive(Component, Clone)]
pub struct OwnedWalkSurface {
    pub original: Handle<Mesh>,
}
/// Rest transform of an NPC visual child before body motion is applied.
#[derive(Component, Clone, Copy)]
pub struct GaitRest(Transform);

fn basis(p: [f32; 3]) -> Vec3 {
    Vec3::new(-p[0], p[2], p[1])
}
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}
/// Sagittal-plane coordinates: an X rotation by `a` is a CCW turn by `a`.
fn yz(v: Vec3) -> Vec2 {
    Vec2::new(v.y, v.z)
}
fn angle_of(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}
fn around(pivot: Vec3, angle: f32) -> Affine3A {
    Affine3A::from_translation(pivot)
        * Affine3A::from_rotation_x(angle)
        * Affine3A::from_translation(-pivot)
}
fn no_rig(key: ResourceKey) -> bool {
    key.group_id == u32::MAX && key.instance_id == u32::MAX && key.type_id == u32::MAX
}

/// Mapping from GMDL palette slots to anatomy rigblocks: root slots to the
/// non-decoration blocks in order, then each later binding group to the next
/// part/decoration by order. Checked against the installed data: every keyed
/// part binding names its rigblock's part (`part_bindings_match`) and the
/// skinned vertices of each root slot lie on its block's bone segment
/// (`installed_palette_structure_census`). The baked-deform records
/// themselves are still not interpreted (parts ride their block rigidly).
#[cfg_attr(not(test), allow(dead_code))]
fn bone_owners(model: &baked_model::Model, anatomy: &CreatureData) -> Result<Vec<usize>, String> {
    bone_layout(model, anatomy).map(|(owners, _)| owners)
}

/// `bone_owners` and the part bone groups: (binding index, part rigblock,
/// first palette slot, bone count) for every binding after the root group.
fn bone_layout(model: &baked_model::Model, anatomy: &CreatureData) -> Result<(Vec<usize>, Vec<(usize, usize, usize, usize)>), String> {
    let kinds: Vec<i8> = anatomy.rigblocks.iter().map(|b| b.block_type()).collect();
    let boned: Vec<usize> = (0..kinds.len())
        .filter(|&i| kinds[i] != DECORATION)
        .collect();
    let parts: Vec<usize> = (0..kinds.len())
        .filter(|&i| matches!(kinds[i], PART | DECORATION))
        .collect();
    if boned.len() != model.root_bones as usize {
        return Err(format!(
            "{} boned rigblocks for {} root slots",
            boned.len(),
            model.root_bones
        ));
    }
    let reserved = model.anim_data.first().ok_or("no reserved bone group")?;
    if !no_rig(reserved.key) || (reserved.field_70 as u8 as i8) as isize != boned.len() as isize {
        return Err("reserved bone group does not match the root slots".into());
    }
    if model.anim_data.len() != parts.len() + 1 {
        return Err(format!(
            "{} part bindings for {} part rigblocks",
            model.anim_data.len() - 1,
            parts.len()
        ));
    }
    let mut owners = boned;
    let mut groups = Vec::with_capacity(parts.len());
    for (binding, (data, &part)) in model.anim_data[1..].iter().zip(&parts).enumerate() {
        let count = if no_rig(data.key) {
            let count = data.field_70 as u8 as i8;
            if count < 0 {
                return Err("negative part bone count".into());
            }
            count as usize
        } else if data.baked_deforms.len() == 1 {
            data.baked_deforms[0].len()
        } else {
            return Err("part binding without one baked bone group".into());
        };
        groups.push((binding + 1, part, owners.len(), count));
        owners.extend(std::iter::repeat_n(part, count));
    }
    Ok((owners, groups))
}

/// The GMDL's part bindings name their rigblocks (ORIGINAL data link): a
/// binding with its own rig (the part's RW4 model, `baked_deforms` = its
/// bones) carries the part's model, which is the part/decoration rigblock
/// `bone_owners` gives it: the same instance id, or the model its part prop
/// draws (`modelMeshLOD0`, `model_of(group, instance)`; e.g. part 3A3C49A5
/// draws CEC04E25). Returns how many keyed bindings agree; any disagreement
/// rejects the mapping.
fn part_bindings_match(
    model: &baked_model::Model,
    anatomy: &CreatureData,
    model_of: &dyn Fn(u32, u32) -> Option<u32>,
) -> Result<usize, String> {
    let parts: Vec<usize> = (0..anatomy.rigblocks.len())
        .filter(|&i| matches!(anatomy.rigblocks[i].block_type(), PART | DECORATION))
        .collect();
    if model.anim_data.len() != parts.len() + 1 {
        return Err(format!("{} part bindings for {} part rigblocks", model.anim_data.len().saturating_sub(1), parts.len()));
    }
    let mut keyed = 0;
    for (binding, (data, &part)) in model.anim_data[1..].iter().zip(&parts).enumerate() {
        if no_rig(data.key) {
            continue;
        }
        let rigblock = &anatomy.rigblocks[part];
        let block = rigblock.instance_id();
        if data.key.instance_id != block && model_of(rigblock.group_id(), block) != Some(data.key.instance_id) {
            return Err(format!(
                "part binding {} names part {:08X}, its rigblock {part} is {block:08X}",
                binding + 1,
                data.key.instance_id
            ));
        }
        keyed += 1;
    }
    Ok(keyed)
}

/// Pure: a foot's place on its stepping loop at cycle fraction `u`, as
/// (ahead, lift). `ahead` spans [-0.5, 0.5] of the stance excursion; stance
/// glides back linearly under the moving body, swing eases forward on a raised
/// arc. `lift` is 0 while planted and peaks at 1 mid-swing.
fn foot_path(u: f32, duty: f32) -> (f32, f32) {
    let u = u.rem_euclid(1.);
    let duty = duty.clamp(0.2, 0.9);
    if u < duty {
        (0.5 - u / duty, 0.)
    } else {
        let s = (u - duty) / (1. - duty);
        (-0.5 + s * s * (3. - 2. * s), (PI * s).sin())
    }
}

/// Pure: planar two-bone IK in sagittal (y, z) coordinates. Returns the hip
/// rotation and the knee rotation relative to the hip that carry `effector`
/// (hanging from `knee` below `hip`) to `target`. The knee keeps the side it
/// bends toward at rest; out-of-reach targets straighten toward them.
fn two_bone(hip: Vec2, knee: Vec2, effector: Vec2, target: Vec2) -> (f32, f32) {
    let upper = (knee - hip).length();
    let lower = (effector - knee).length();
    let to = target - hip;
    if upper < 1e-5 || lower < 1e-5 || to.length() < 1e-5 {
        return (0., 0.);
    }
    let distance = to
        .length()
        .clamp((upper - lower).abs() + 1e-4, (upper + lower) * 0.999);
    let cosine = ((upper * upper + distance * distance - lower * lower) / (2. * upper * distance))
        .clamp(-1., 1.);
    let bend = (effector - hip).perp_dot(knee - hip);
    let side = if bend.abs() < 1e-6 { 1. } else { bend.signum() };
    let direction = to.normalize();
    let new_knee = hip + Vec2::from_angle(side * cosine.acos()).rotate(direction) * upper;
    let reached = hip + direction * distance;
    let hip_angle = wrap(angle_of(new_knee - hip) - angle_of(knee - hip));
    let lower_angle = wrap(angle_of(reached - new_knee) - angle_of(effector - knee));
    (hip_angle, wrap(lower_angle - hip_angle))
}

/// Pure: stride shape for a ground speed in leg lengths per second: length of
/// body travel per cycle, duty factor and swing clearance. Faster gaits take
/// longer, flatter, more airborne strides; `reach` caps the stance excursion
/// (with `OVERREACH`), raising cadence instead of overstretching legs.
fn stride_for(relative_speed: f32, leg_length: f32, reach: f32) -> (f32, f32, f32) {
    let run = smoothstep(1.8, 4.5, relative_speed);
    let duty = 0.62 - 0.22 * run;
    let mut length = leg_length * (1.3 + 0.32 * relative_speed).clamp(1.3, 3.4);
    if reach > 0. {
        length = length.min(2. * OVERREACH * reach / duty);
    }
    (length.max(1e-3), duty, leg_length * (0.16 + 0.12 * run))
}

impl GaitRig {
    /// Build from the parsed GMDL and its same-instance anatomy.
    #[cfg_attr(not(test), allow(dead_code))]
    fn build(model: &baked_model::Model, anatomy: &CreatureData) -> Result<Self, String> {
        Self::build_with(model, anatomy, &[])
    }

    /// `build`, giving the part bones of `rigged` bindings (binding index,
    /// part rig, rig -> model, model -> rig) their own joints while the
    /// palette has room (ORIGINAL binding: a keyed binding's group is its part
    /// rig's bones, FUN_00742E30).
    fn build_with(
        model: &baked_model::Model,
        anatomy: &CreatureData,
        rigged: &[(usize, Arc<crate::cell_anim::PartRig>, Affine3A, Affine3A)],
    ) -> Result<Self, String> {
        let count = anatomy.rigblocks.len();
        if count == 0 || model.pieces.len() != model.bone_ranges.len() {
            return Err("empty anatomy or missing piece bone ranges".into());
        }
        if count + 1 > MAX_SKIN_JOINTS {
            return Err(format!("{count} rigblocks exceed the skinning palette"));
        }
        let (owners, groups) = bone_layout(model, anatomy)?;
        // Palette slot -> skinning joint: the owning rigblock, or a morph
        // joint of its part's bone.
        let mut slot_joint: Vec<usize> = owners.clone();
        let mut next_joint = count + 1;
        let mut morph_groups = Vec::new();
        for &(binding, part, first, bones) in &groups {
            let Some((_, rig, to_model, from_model)) = rigged.iter().find(|r| r.0 == binding) else { continue };
            if rig.bone_count() != bones || next_joint + bones > MAX_SKIN_JOINTS || first + bones > slot_joint.len() {
                continue;
            }
            for k in 0..bones {
                slot_joint[first + k] = next_joint + k;
            }
            morph_groups.push(crate::creature_morph::MorphGroup::new(binding, part, next_joint, rig.clone(), *to_model, *from_model));
            next_joint += bones;
        }
        let mut joints = Vec::with_capacity(count);
        for (slot, block) in anatomy.rigblocks.iter().enumerate() {
            let parent = block.parent_index();
            if block.index() != slot as i16 || parent >= count as i16 || parent < -1 {
                return Err("rigblock indices are not contiguous".into());
            }
            let pivot = basis(block.position_bits().map(f32::from_bits));
            if !pivot.is_finite() {
                return Err("non-finite rigblock position".into());
            }
            joints.push(Joint {
                pivot,
                parent: usize::try_from(parent).ok(),
            });
        }
        let kinds: Vec<i8> = anatomy.rigblocks.iter().map(|b| b.block_type()).collect();
        let mut children = vec![Vec::new(); count];
        let mut order = Vec::with_capacity(count);
        for (j, joint) in joints.iter().enumerate() {
            match joint.parent {
                Some(p) => children[p].push(j),
                None => order.push(j),
            }
        }
        let mut next = 0;
        while next < order.len() {
            order.extend(children[order[next]].iter().copied());
            next += 1;
        }
        if order.len() != count {
            return Err("cyclic rigblock hierarchy".into());
        }

        // Each drawn vertex's dominant rigblock gives limb extents and bounds.
        let mut lowest = vec![(f32::INFINITY, Vec3::ZERO); count];
        let (mut agree, mut checked) = (0usize, 0usize);
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut drawn_sections = Vec::with_capacity(model.pieces.len());
        for (piece, range) in model.pieces.iter().zip(&model.bone_ranges) {
            let range_end = range[0]
                .checked_add(range[1])
                .ok_or("piece bone range overflow")?;
            if range_end as usize > owners.len() {
                return Err("piece bone range exceeds the assembled palette".into());
            }
            let drawn = crate::animation_layout::drawn_vertices(piece)?;
            if piece.bones.len() != piece.positions.len()
                || piece.weights.len() != piece.positions.len()
                || piece.normals.len() != piece.positions.len()
            {
                return Err("piece influence table does not match its vertices".into());
            }
            for (v, _) in drawn.iter().enumerate().filter(|(_, d)| **d) {
                let position = basis(piece.positions[v]);
                min = min.min(position);
                max = max.max(position);
                for i in 0..4 {
                    let weight = piece.weights[v][i];
                    if !weight.is_finite() || weight < 0.0 {
                        return Err("drawn vertex has an invalid skin weight".into());
                    }
                    if weight > 0.0 {
                        owner(&owners, *range, piece.bones[v][i])?;
                    }
                }
                let dominant = (0..4)
                    .filter(|&i| piece.weights[v][i] > 0.)
                    .max_by(|&a, &b| piece.weights[v][a].total_cmp(&piece.weights[v][b]))
                    .map(|i| owner(&owners, *range, piece.bones[v][i]))
                    .transpose()?;
                if let Some(joint) = dominant {
                    if position.y < lowest[joint].0 {
                        lowest[joint] = (position.y, position);
                    }
                    let own = joints[joint].pivot.distance_squared(position);
                    let closer = joints.iter().filter(|j| j.pivot.distance_squared(position) < own).count();
                    agree += usize::from(closer < 3);
                    checked += 1;
                }
            }
            drawn_sections.push(drawn);
        }
        if !min.is_finite() || !max.is_finite() {
            return Err("model has no drawn vertices".into());
        }
        let ground = min.y;
        let height = (max.y - min.y).max(1e-3);
        let body_length = (max.z - min.z).max(1e-3);
        let mut subtree_low = lowest.clone();
        for &j in order.iter().rev() {
            if let Some(p) = joints[j].parent {
                if subtree_low[j].0 < subtree_low[p].0 {
                    subtree_low[p] = subtree_low[j];
                }
            }
        }
        let grounded = |j: usize| subtree_low[j].0 <= ground + LEG_GROUND_FRACTION * height;
        let lowest_child = |j: usize, kind: &dyn Fn(i8) -> bool| {
            children[j]
                .iter()
                .copied()
                .filter(|&c| kind(kinds[c]) && subtree_low[c].0.is_finite())
                .min_by(|&a, &b| subtree_low[a].0.total_cmp(&subtree_low[b].0))
        };

        let mut legs = Vec::new();
        let mut arm_roots = Vec::new();
        for root in 0..count {
            let parent_kind = joints[root].parent.map(|p| kinds[p]);
            if kinds[root] != LIMB || parent_kind == Some(LIMB) {
                continue;
            }
            if !grounded(root) {
                if subtree_low[root].0.is_finite() {
                    arm_roots.push(root);
                }
                continue;
            }
            let mut chain = vec![root];
            while let Some(next) = lowest_child(*chain.last().unwrap(), &|k| k == LIMB) {
                chain.push(next);
            }
            let last = *chain.last().unwrap();
            let foot = lowest_child(last, &|k| k == PART)
                .or_else(|| lowest_child(last, &|k| k == DECORATION));
            let effector = foot.map_or(subtree_low[last].1, |f| joints[f].pivot);
            legs.push(Leg {
                hip: root,
                knee: chain.get(1).copied(),
                foot,
                effector,
                offset: 0.,
                reach: 0.,
            });
        }
        if legs.is_empty() {
            // Pads or feet mounted straight on the torso.
            for root in 0..count {
                if kinds[root] == PART
                    && joints[root].parent.is_some_and(|p| kinds[p] == SPINE)
                    && joints[root].pivot.y < ground + 0.4 * height
                    && grounded(root)
                {
                    legs.push(Leg {
                        hip: root,
                        knee: None,
                        foot: None,
                        effector: joints[root].pivot,
                        offset: 0.,
                        reach: 0.,
                    });
                }
            }
        }
        let leg_length = if legs.is_empty() {
            0.5 * height
        } else {
            legs.iter()
                .map(|leg| (joints[leg.hip].pivot.y - ground).max(0.05 * height))
                .sum::<f32>()
                / legs.len() as f32
        };
        // Alternate sides; consecutive ranks (front to back) alternate too,
        // so quadrupeds trot on diagonals and hexapods use tripods.
        let side = |x: f32| if x < -1e-3 { 1 } else { 0 };
        let mut by_z: Vec<usize> = (0..legs.len()).collect();
        by_z.sort_by(|&a, &b| {
            joints[legs[a].hip]
                .pivot
                .z
                .total_cmp(&joints[legs[b].hip].pivot.z)
        });
        let (mut rank, mut rank_z) = (0, f32::NEG_INFINITY);
        for &l in &by_z {
            let z = joints[legs[l].hip].pivot.z;
            if rank_z.is_finite() && z - rank_z > 0.2 * body_length {
                rank += 1;
                rank_z = z;
            } else if !rank_z.is_finite() {
                rank_z = z;
            }
            legs[l].offset = ((side(joints[legs[l].hip].pivot.x) + rank) % 2) as f32 * 0.5;
        }
        for leg in &mut legs {
            let hip = joints[leg.hip].pivot;
            leg.reach = match leg.knee {
                Some(knee) => {
                    // Authored legs often stand nearly straight, so the pure
                    // circle-of-reach estimate is a floor, not the stride.
                    let knee = joints[knee].pivot;
                    let span = 0.98
                        * ((yz(knee) - yz(hip)).length() + (yz(leg.effector) - yz(knee)).length());
                    let across = (span * span - (hip.y - leg.effector.y).powi(2))
                        .max(0.)
                        .sqrt();
                    (leg.effector.z - (hip.z - across))
                        .min(hip.z + across - leg.effector.z)
                        .max(0.35 * leg_length)
                }
                None if (leg.effector - hip).length() > 1e-4 => {
                    0.45 * (yz(leg.effector) - yz(hip)).length()
                }
                None => 0.15 * leg_length,
            };
        }
        let arms = arm_roots
            .into_iter()
            .map(|root| {
                let reach = (subtree_low[root].1 - joints[root].pivot).normalize_or_zero();
                Arm {
                    root,
                    offset: side(joints[root].pivot.x) as f32 * 0.5,
                    hang: 0.25 + 0.75 * (-reach.y).max(0.),
                }
            })
            .collect::<Vec<_>>();

        // A tail is the spine continuing behind (+Z) the vertebra that carries
        // the rearmost leg; upright torsos and forward-curling spines have none.
        let mut tail = Vec::new();
        let pelvis = legs
            .iter()
            .max_by(|a, b| joints[a.hip].pivot.z.total_cmp(&joints[b.hip].pivot.z))
            .and_then(|leg| joints[leg.hip].parent)
            .filter(|&p| kinds[p] == SPINE);
        if let Some(pelvis) = pelvis {
            let mut at = pelvis;
            while let Some(next) = children[at].iter().copied().find(|&c| kinds[c] == SPINE) {
                tail.push(next);
                at = next;
            }
            let end = tail
                .last()
                .map_or(f32::NEG_INFINITY, |&j| joints[j].pivot.z);
            if end < joints[pelvis].pivot.z + 0.15 * body_length {
                tail.clear();
            }
        }
        let mut moving = vec![false; count];
        for root in legs
            .iter()
            .map(|l| l.hip)
            .chain(arms.iter().map(|a| a.root))
            .chain(tail.first().copied())
        {
            let mut stack = vec![root];
            while let Some(j) = stack.pop() {
                moving[j] = true;
                stack.extend(children[j].iter().copied());
            }
        }
        // Every vertex gets a joint palette; a piece's undrawn slots in a
        // shared vertex buffer belong to another piece's palette, so they and
        // unresolvable influences stay on the fixed rest joint.
        let rest_joint = count as u16;
        let mut sections = Vec::with_capacity(model.pieces.len());
        for (section, ((piece, range), drawn)) in model
            .pieces
            .iter()
            .zip(&model.bone_ranges)
            .zip(&drawn_sections)
            .enumerate()
        {
            let mut skin = SectionSkin {
                section,
                joints: vec![[rest_joint, 0, 0, 0]; piece.positions.len()],
                weights: vec![[1., 0., 0., 0.]; piece.positions.len()],
            };
            for v in (0..piece.positions.len()).filter(|&v| drawn[v]) {
                let (mut joints, mut weights) = ([0u16; 4], [0f32; 4]);
                let mut total = 0.;
                for i in 0..4 {
                    let weight = piece.weights[v][i];
                    if !(weight > 0.) {
                        continue;
                    }
                    owner(&owners, *range, piece.bones[v][i])?;
                    let joint = slot_joint[(range[0] + u32::from(piece.bones[v][i] / 3)) as usize];
                    joints[i] = joint as u16;
                    weights[i] = weight;
                    total += weight;
                }
                if total > 0. && total.is_finite() {
                    skin.joints[v] = joints;
                    skin.weights[v] = weights.map(|w| w / total);
                }
            }
            sections.push(skin);
        }
        let clips = crate::creature_clips::rig_of_creature_data(anatomy)
            .ok()
            .filter(|r| r.len() == count)
            .map(Arc::new);
        Ok(Self {
            joints,
            order,
            legs,
            arms,
            tail,
            moving,
            sections,
            leg_length,
            height,
            clips,
            agreement: if checked == 0 { 0. } else { agree as f32 / checked as f32 },
            morph: (!morph_groups.is_empty()).then(|| {
                // Rest entries and baked handle records (`creature_morph`).
                let mut set = crate::creature_morph::MorphSet { groups: morph_groups };
                set.attach_baked(model, anatomy);
                Arc::new(set)
            }),
            joint_total: next_joint,
        })
    }

    /// Morph joints at their blocks' matrices (no deform: the rigid part).
    fn rigid_morph(&self, blocks: &[Affine3A]) -> Vec<(usize, Affine3A)> {
        let mut out = Vec::new();
        if let Some(m) = &self.morph {
            for g in &m.groups {
                let b = blocks.get(g.block).copied().unwrap_or(Affine3A::IDENTITY);
                out.extend((0..g.rig.bone_count()).map(|k| (g.first_joint + k, b)));
            }
        }
        out
    }

    /// Longest stance excursion every leg can reach.
    fn reach(&self) -> f32 {
        self.legs
            .iter()
            .map(|leg| leg.reach)
            .fold(f32::INFINITY, f32::min)
            .min(1e6)
    }

    fn animated(&self) -> bool {
        !self.legs.is_empty() || !self.arms.is_empty()
    }

    /// Joint matrices (model space) for one stride pose. A zero blend is the
    /// exact rest pose.
    fn matrices(&self, stride: &Stride) -> Vec<Affine3A> {
        let mut local = vec![Affine3A::IDENTITY; self.joints.len()];
        if stride.blend > 0. {
            let excursion = stride.length * stride.duty;
            for leg in &self.legs {
                let u = stride.phase / TAU + leg.offset;
                let (ahead, lift) = foot_path(u, stride.duty);
                let limit = leg.reach * OVERREACH;
                let ahead = (ahead * excursion).clamp(-limit, limit) * stride.blend;
                let up = (lift * stride.lift + stride.sink) * stride.blend;
                // The authored creature front is -Z.
                let target = leg.effector + Vec3::new(0., up, -ahead);
                self.pose_leg(leg, target, lift * stride.blend, &mut local);
            }
            for arm in &self.arms {
                let (ahead, _) = foot_path(stride.phase / TAU + arm.offset, stride.duty);
                local[arm.root] = around(
                    self.joints[arm.root].pivot,
                    -2. * ahead * stride.swing * arm.hang * stride.blend,
                );
            }
            // The tail swings once per stride as a wave travelling to its tip.
            let spread = (self.tail.len() as f32).sqrt().max(1.);
            for (i, &j) in self.tail.iter().enumerate() {
                let wave = (stride.phase - 0.8 * i as f32).sin();
                let pivot = self.joints[j].pivot;
                local[j] = Affine3A::from_translation(pivot)
                    * Affine3A::from_rotation_y(0.5 * stride.swing * wave * stride.blend / spread)
                    * Affine3A::from_translation(-pivot);
            }
        }
        let mut world = vec![Affine3A::IDENTITY; self.joints.len()];
        for &j in &self.order {
            world[j] = match self.joints[j].parent {
                Some(p) => world[p] * local[j],
                None => local[j],
            };
        }
        world
    }

    fn pose_leg(&self, leg: &Leg, target: Vec3, lift: f32, local: &mut [Affine3A]) {
        let hip = self.joints[leg.hip].pivot;
        let span = leg.effector - hip;
        let (hip_angle, knee_angle, raise) = if span.length() < 1e-4 {
            // A pad on the torso can only step by sliding up and along.
            let offset = target - leg.effector;
            (0., 0., Vec3::new(0., offset.y, offset.z))
        } else if let Some(knee) = leg.knee {
            let knee = self.joints[knee].pivot;
            let (a, b) = two_bone(yz(hip), yz(knee), yz(leg.effector), yz(target));
            (a, b, Vec3::ZERO)
        } else {
            // One rigid segment: swing toward the target, then lift it.
            let a = wrap(angle_of(yz(target - hip)) - angle_of(yz(span)));
            let a = a.clamp(-MAX_JOINT_ANGLE, MAX_JOINT_ANGLE);
            let swung = around(hip, a).transform_point3(leg.effector);
            let rise = (target.y - swung.y).clamp(-0.15 * self.leg_length, 0.3 * self.leg_length);
            (a, 0., Vec3::Y * rise)
        };
        let hip_angle = hip_angle.clamp(-MAX_JOINT_ANGLE, MAX_JOINT_ANGLE);
        let knee_angle = knee_angle.clamp(-MAX_JOINT_ANGLE, MAX_JOINT_ANGLE);
        local[leg.hip] = Affine3A::from_translation(raise) * around(hip, hip_angle);
        if let Some(knee) = leg.knee {
            local[knee] = around(self.joints[knee].pivot, knee_angle);
        }
        if let Some(foot) = leg.foot {
            // Keep the sole level; the toes dip while the foot is airborne.
            local[foot] = around(
                self.joints[foot].pivot,
                -(hip_angle + knee_angle) - 0.3 * lift,
            );
        }
    }
}

fn owner(owners: &[usize], range: [u32; 2], encoded: u8) -> Result<usize, String> {
    if encoded % 3 != 0 {
        return Err(format!("unresolved weighted bone byte {encoded}"));
    }
    let slot = range[0]
        .checked_add(u32::from(encoded / 3))
        .ok_or("weighted bone slot overflow")?;
    let end = range[0]
        .checked_add(range[1])
        .ok_or("piece bone range overflow")?;
    if slot < range[0] || slot >= end {
        return Err(format!(
            "weighted bone slot {slot} falls outside piece range {}..{end}",
            range[0]
        ));
    }
    owners
        .get(slot as usize)
        .copied()
        .ok_or_else(|| format!("weighted bone slot {slot} has no anatomy owner"))
}

/// CPU reference of the GPU skinning sum for one vertex (tests only).
#[cfg(test)]
fn skin_point(matrices: &[Affine3A], joints: [u16; 4], weights: [f32; 4], point: Vec3) -> Vec3 {
    joints
        .iter()
        .zip(weights)
        .filter(|(_, w)| *w > 0.)
        .map(|(&j, w)| {
            matrices
                .get(j as usize)
                .copied()
                .unwrap_or(Affine3A::IDENTITY)
                .transform_point3(point)
                * w
        })
        .sum()
}

/// A skinned copy of one original surface mesh: identical geometry, UVs and
/// tangents plus the rigblock joint palette. The original stays untouched.
fn skinned_surface_mesh(source: &Mesh, skin: &SectionSkin) -> Option<Mesh> {
    let count = match source.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(values) => values.len(),
        _ => return None,
    };
    if count != skin.joints.len() {
        return None;
    }
    let mut mesh = source.clone();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(skin.joints.clone()),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, skin.weights.clone());
    mesh.generate_skinned_mesh_bounds().ok()?;
    Some(mesh)
}

/// The installed GMDL drawn under a native NPC actor, if any.
fn native_source(world: &World, actor: Entity) -> Option<ResourceKey> {
    if world.get::<baked_model::NativeCreature>(actor).is_none() {
        return None;
    }
    world.get::<Children>(actor)?.iter().find_map(|child| {
        world
            .get::<crate::material_binding::OriginalSurfaceChild>(child)
            .and(world.get::<crate::world_objects::OriginalWorldObject>(child))
            .map(|object| object.model)
    })
}

/// The RW4 model instance a part prop draws (`modelMeshLOD0`, 0x00F9EFBB).
fn part_model(index: &crate::asset_catalog::ResourceIndex, group: u32, instance: u32) -> Option<u32> {
    let prop = ResourceKey { type_id: crate::decode::TYPE_PROP, group_id: group, instance_id: instance };
    index.properties(prop).ok()?.0.get(&0x00F9_EFBB).and_then(|v| v.key()).map(|k| k.instance_id)
}

/// The installed models' part bones get their own joints (`creature_morph::
/// enabled`; on by default, `OPENSPORE_CREATURE_MORPH=0` turns it off).
fn morph_enabled() -> bool {
    crate::creature_morph::enabled()
}

fn load_rig(world: &World, key: ResourceKey, part_rigs: &mut crate::creature_morph::RigCache) -> Result<GaitRig, String> {
    // The exact starter layout has a checked structural contract; other
    // species are accepted when their binding has the retail structure.
    if key.group_id != baked_model::CREATURE_GROUP || key.type_id != crate::decode::TYPE_GMDL {
        return Err(format!("{key:?} is not an installed creature model"));
    }
    let originals = world.resource::<Originals>();
    let model = baked_model::parse(&originals.resource_index.read(key)?)?;
    let anatomy = crate::creature_data::parse(&originals.resource_index.read(ResourceKey {
        type_id: crate::creature_data::TYPE,
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: key.instance_id,
    })?)?;
    let rigged = if morph_enabled() { crate::creature_morph::rigged_bindings(&originals.resource_index, &model, part_rigs) } else { Vec::new() };
    if key.instance_id != baked_model::PLAYER_BASELINE_INSTANCE {
        // The binding the game uses: root slots = the non-decoration
        // rigblocks in order (FUN_00742E30 builds the model's bone groups in
        // binding order; the csd keeps rigblock order, FUN_009CBE10), part
        // bindings = the part/decoration rigblocks in order, each keyed
        // binding naming its own part (`part_bindings_match`, 263 of 276
        // installed models). The geometric agreement is logged only.
        part_bindings_match(&model, &anatomy, &|group, instance| part_model(&originals.resource_index, group, instance))?;
        return GaitRig::build_with(&model, &anatomy, &rigged);
    }
    if model.root_bones != 18
        || anatomy.rigblocks.len() != 18
        || model.anim_data.len() != 6
        || model.anim_data[4].key.instance_id != 0x7F815C91
        || model.anim_data[5].key.instance_id != 0x4AF28CE7
        || model.bone_ranges.len() != model.pieces.len()
        || model.bone_ranges.iter().any(|range| range[1] != 34)
    {
        return Err("starter palette/anatomy contract differs; body motion only".into());
    }
    for (index, parent) in [(9, 5), (10, 9), (11, 5), (12, 11), (16, 10), (17, 12)] {
        if anatomy.rigblocks[index].index() != index as i16
            || anatomy.rigblocks[index].parent_index() != parent
        {
            return Err("starter leg hierarchy differs; body motion only".into());
        }
    }
    GaitRig::build_with(&model, &anatomy, &rigged)
}

impl WalkTemplates {
    fn rig(&mut self, world: &World, key: ResourceKey) -> Option<Arc<GaitRig>> {
        self.rigs
            .entry(key)
            .or_insert_with(|| match load_rig(world, key, &mut self.part_rigs) {
                Ok(rig) => {
                    info!(
                        "procedural gait {:08X}: {} legs, {} swinging limbs, {} joints, clips {}, {} morph parts, mapping agreement {:.0}%",
                        key.instance_id,
                        rig.legs.len(),
                        rig.arms.len(),
                        rig.joints.len(),
                        rig.clips.is_some(),
                        rig.morph.as_ref().map_or(0, |m| m.groups.len()),
                        rig.agreement * 100.
                    );
                    Some(Arc::new(rig))
                }
                Err(e) => {
                    warn!(
                        "procedural gait {:08X} unavailable (body motion only): {e}",
                        key.instance_id
                    );
                    None
                }
            })
            .clone()
    }

    /// Shared skinned variant of `original` for one section of `key`.
    fn skinned_mesh(
        &mut self,
        world: &mut World,
        key: ResourceKey,
        rig: &GaitRig,
        section: usize,
        original: &Handle<Mesh>,
    ) -> Option<(Handle<Mesh>, Handle<SkinnedMeshInverseBindposes>)> {
        let skins = self.skins.entry(key).or_default();
        // Joints are posed in model space under the visual child, so every
        // bind pose is the identity (one extra rest joint included).
        let inverse = skins
            .inverse_bindposes
            .get_or_insert_with(|| {
                let handle = world
                    .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
                    .add(vec![Mat4::IDENTITY; rig.joint_total]);
                // Cinematic copies of this skin are found by these bind poses.
                if let (Some(clips), Some(mut drive)) = (&rig.clips, world.get_resource_mut::<crate::creature_clips::ClipDrive>()) {
                    drive.register_skin(handle.id(), clips.clone(), 1.);
                    if let Some(morph) = &rig.morph {
                        drive.register_morph(handle.id(), morph.clone());
                    }
                }
                handle
            })
            .clone();
        if let Some(handle) = skins.meshes.get(&original.id()) {
            return Some((handle.clone(), inverse));
        }
        let skin = rig.sections.iter().find(|s| s.section == section)?;
        let mesh = skinned_surface_mesh(world.resource::<Assets<Mesh>>().get(original)?, skin)?;
        let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
        skins.meshes.insert(original.id(), handle.clone());
        Some((handle, inverse))
    }
}

struct Surface {
    entity: Entity,
    section: usize,
    mesh: Handle<Mesh>,
    owned: bool,
    rest: Option<Transform>,
    transform: Transform,
}

/// Give an actor a joint skeleton under its first visual child and switch
/// every surface to its shared skinned mesh. Returns false (leaving the actor
/// on its original meshes) when the skinned assets are unavailable.
fn attach_skeleton(
    world: &mut World,
    cache: &mut WalkTemplates,
    actor: Entity,
    key: ResourceKey,
    rig: &GaitRig,
    surfaces: &[Surface],
) -> bool {
    if !world.contains_resource::<Assets<SkinnedMeshInverseBindposes>>() {
        return false;
    }
    let mut draws = Vec::with_capacity(surfaces.len());
    for surface in surfaces {
        let original = match world.get::<OwnedWalkSurface>(surface.entity) {
            Some(owned) => owned.original.clone(),
            None => surface.mesh.clone(),
        };
        let Some((mesh, inverse)) = cache.skinned_mesh(world, key, rig, surface.section, &original)
        else {
            return false;
        };
        draws.push((surface.entity, original, mesh, inverse));
    }
    if let Some(old) = world.entity_mut(actor).take::<GaitSkeleton>() {
        for joint in old.joints {
            if let Ok(entity) = world.get_entity_mut(joint) {
                entity.despawn();
            }
        }
    }
    let holder = surfaces[0].entity;
    let joints: Vec<Entity> = (0..rig.joint_total.max(rig.joints.len() + 1))
        .map(|_| {
            world
                .spawn((
                    crate::stages::StageOwned,
                    Name::new("gait joint"),
                    Transform::IDENTITY,
                    ChildOf(holder),
                ))
                .id()
        })
        .collect();
    for (entity, original, mesh, inverse) in draws {
        world.entity_mut(entity).insert((
            Mesh3d(mesh),
            SkinnedMesh {
                inverse_bindposes: inverse,
                joints: joints.clone(),
            },
            DynamicSkinnedMeshBounds,
            OwnedWalkSurface { original },
        ));
    }
    world
        .entity_mut(actor)
        .insert(GaitSkeleton { holder, joints });
    true
}

/// Put an actor back on its original surface meshes (no skeleton).
fn detach_skeleton(world: &mut World, actor: Entity, surfaces: &[Surface]) {
    for surface in surfaces {
        let Some(owned) = world.get::<OwnedWalkSurface>(surface.entity).cloned() else { continue };
        world
            .entity_mut(surface.entity)
            .insert(Mesh3d(owned.original))
            .remove::<(SkinnedMesh, DynamicSkinnedMeshBounds, OwnedWalkSurface)>();
    }
    if let Some(old) = world.entity_mut(actor).take::<GaitSkeleton>() {
        for joint in old.joints {
            if let Ok(entity) = world.get_entity_mut(joint) {
                entity.despawn();
            }
        }
    }
}

pub fn update(world: &mut World) {
    if !world.contains_resource::<WalkTemplates>() || !world.contains_resource::<Originals>() {
        return;
    }
    let mut actors: Vec<(Entity, ResourceKey, bool)> = world
        .query::<(Entity, &baked_model::NativePlayerSource)>()
        .iter(world)
        .map(|(e, s)| (e, s.0, true))
        .collect();
    let npcs: Vec<Entity> = world
        .query_filtered::<Entity, With<baked_model::NativeCreature>>()
        .iter(world)
        .collect();
    for npc in npcs {
        if let Some(source) = native_source(world, npc) {
            if !actors.iter().any(|(existing, _, _)| *existing == npc) {
                actors.push((npc, source, false));
            }
        }
    }
    if actors.is_empty() {
        world.resource_mut::<WalkTemplates>().skins.clear();
        if let Some(mut drive) = world.get_resource_mut::<crate::creature_clips::ClipDrive>() {
            drive.native = false;
        }
        return;
    }
    // Derived GPU assets are retained only while at least one current native
    // actor uses that source model. Dropping these handles lets Bevy release
    // their mesh and inverse-bind assets after actor surfaces are gone.
    let active_models: HashSet<ResourceKey> = actors.iter().map(|(_, key, _)| *key).collect();
    let dt = world.resource::<Time>().delta_secs().clamp(0., 0.05);
    let editing = world
        .get_resource::<crate::editor::EditorState>()
        .is_some_and(|editor| editor.open);
    let eye = world
        .query_filtered::<&GlobalTransform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| (t.translation(), t.forward().as_vec3()));
    let mut native_player_posed = false;
    // Look targets (FUN_00C17F40): the avatar and the creature it is engaged
    // with look at each other's heads; the avatar otherwise at the cursor.
    let engaged = world.get_resource::<crate::creature_actions::Encounter>().and_then(|e| e.target);
    let engaged_head = engaged.and_then(|id| {
        world
            .query::<(&crate::persistence::WorldId, &GlobalTransform, Option<&crate::creature_look::PosedHead>)>()
            .iter(world)
            .find(|(w, _, _)| w.0 == id)
            .map(|(_, gt, head)| crate::creature_look::head_of(gt, head))
    });
    let player_head = world
        .query_filtered::<&GlobalTransform, With<crate::game::Player>>()
        .iter(world)
        .next()
        .map(crate::creature_look::head_point);
    let cursor = world.get_resource::<crate::creature_look::CursorGround>().and_then(|c| c.0);
    // Clip cost: creatures posed and seconds spent this frame.
    let mut clip_cost = (0usize, 0f64);
    world.resource_scope(|world, mut cache: Mut<WalkTemplates>| {
        cache.skins.retain(|key, _| active_models.contains(key));
        cache.frame = cache.frame.wrapping_add(1);
        let frame = cache.frame;
        for (slot, (actor, source, player)) in actors.into_iter().enumerate() {
            if world.get::<Visibility>(actor) == Some(&Visibility::Hidden) {
                continue;
            }
            let Some(&root) = world.get::<Transform>(actor) else {
                continue;
            };
            let yaw = root.rotation.to_euler(EulerRot::YXZ).0;
            let rig = cache.rig(world, source);
            let surfaces: Vec<Surface> = world
                .query::<(
                    Entity,
                    &ChildOf,
                    &crate::world_objects::OriginalWorldObject,
                    &crate::material_binding::OriginalSurfaceChild,
                    &Mesh3d,
                    &Transform,
                    Option<&OwnedWalkSurface>,
                    Option<&GaitRest>,
                )>()
                .iter(world)
                .filter(|(_, p, object, _, _, _, _, _)| {
                    p.parent() == actor && object.model == source
                })
                .map(
                    |(entity, _, _, section, mesh, transform, owned, rest)| Surface {
                        entity,
                        section: section.source_section,
                        mesh: mesh.0.clone(),
                        owned: owned.is_some(),
                        rest: rest.map(|r| r.0),
                        transform: *transform,
                    },
                )
                .collect();
            if surfaces.is_empty() {
                continue;
            }
            // Test Drive skinned the installed starter for its clips; the
            // other editor modes show it unskinned again (as before clips).
            let in_test_drive = world
                .get_resource::<crate::creature_editor::CreatureEditor>()
                .is_some_and(|e| e.active && e.mode == crate::creature_editor::Mode::Play);
            if player && editing && !in_test_drive && world.get::<GaitSkeleton>(actor).is_some() {
                detach_skeleton(world, actor, &surfaces);
                if let Some(mut walk) = world.get_mut::<AdaptedWalk>(actor) {
                    walk.posed = false;
                    walk.clip_posed = false;
                    walk.clips = NpcClips::default();
                }
                continue;
            }
            if world.get::<AdaptedWalk>(actor).is_none() {
                let p = root.translation;
                world.entity_mut(actor).insert(AdaptedWalk {
                    previous: p,
                    previous_yaw: yaw,
                    duty: 0.6,
                    seed: (p.x * 1.37 + p.z * 2.71).rem_euclid(TAU),
                    ..default()
                });
                if !player {
                    for surface in &surfaces {
                        world
                            .entity_mut(surface.entity)
                            .insert(GaitRest(surface.transform));
                    }
                }
                continue;
            }
            let model_scale = surfaces
                .iter()
                .map(|s| s.rest.unwrap_or(s.transform).scale.x)
                .next()
                .unwrap_or(1.)
                * root.scale.x.abs();
            let model_scale = model_scale.max(1e-4);
            let leg_length = rig.as_ref().map_or(0.6, |r| r.leg_length);
            let reach = rig.as_ref().map_or(0., |r| r.reach());

            // Gait state: ground distance and turning both advance the stride.
            let (stride, relative, turn, clock, seed, posed) = {
                let mut walk = world.get_mut::<AdaptedWalk>(actor).unwrap();
                let step = (root.translation - walk.previous).with_y(0.).length();
                let dyaw = wrap(yaw - walk.previous_yaw);
                walk.previous = root.translation;
                walk.previous_yaw = yaw;
                if dt > 0. && step < 2. {
                    let raw = step / model_scale / dt;
                    walk.raw_speed += (raw - walk.raw_speed) * (1. - (-8. * dt).exp());
                }
                let valid = !editing && step < 2. && dt > 0.;
                let travel = if valid {
                    step / model_scale + dyaw.abs() * 0.6 * leg_length
                } else {
                    0.
                };
                let speed = if dt > 0. { travel / dt } else { 0. };
                walk.speed += (speed - walk.speed) * (1. - (-8. * dt).exp());
                let yaw_rate = if valid { dyaw / dt } else { 0. };
                walk.turn += (yaw_rate - walk.turn) * (1. - (-6. * dt).exp());
                let relative = walk.speed / leg_length.max(1e-4);
                let target = if valid && relative > 0.15 { 1. } else { 0. };
                let rate = if target > walk.blend { 6. } else { 4. };
                walk.blend = if editing {
                    0.
                } else {
                    walk.blend + (target - walk.blend) * (1. - (-rate * dt).exp())
                };
                if walk.blend < 0.002 && target == 0. {
                    walk.blend = 0.;
                }
                let (length, duty, lift) = stride_for(relative, leg_length, reach);
                walk.duty = duty;
                if travel > 0. {
                    walk.phase = (walk.phase + TAU * travel / length).rem_euclid(TAU);
                }
                walk.clock += dt;
                let run = smoothstep(1.8, 4.5, relative);
                (
                    Stride {
                        phase: walk.phase,
                        blend: walk.blend,
                        length,
                        duty,
                        lift,
                        sink: 0.,
                        swing: 0.3 + 0.2 * run,
                    },
                    relative,
                    walk.turn,
                    walk.clock,
                    walk.seed,
                    walk.posed,
                )
            };
            let mut stride = stride;

            // Clips pose every rigblock when the rig has a clip rig.
            let clip_rig = rig.as_ref().and_then(|r| r.clips.clone()).filter(|_| crate::creature_clips::enabled("native"));
            // Test Drive plays the installed starter's clips too.
            let test_drive = world
                .get_resource::<crate::creature_editor::CreatureEditor>()
                .filter(|e| player && e.active && e.mode == crate::creature_editor::Mode::Play)
                .map(|e| e.play.clone());
            if let (Some(gait), Some(clip_rig), true) = (rig.clone(), clip_rig, !editing || test_drive.is_some()) {
                if !player {
                    for surface in &surfaces {
                        if let (Some(rest), Some(mut transform)) = (surface.rest, world.get_mut::<Transform>(surface.entity)) {
                            *transform = rest;
                        }
                    }
                }
                let mind = world.get::<crate::creature_actions::CreatureMind>(actor).cloned();
                let speed = world
                    .get::<AdaptedWalk>(actor)
                    .map_or(0., |w| if test_drive.is_some() { w.raw_speed } else { w.speed });
                // A cinematic `playAnimation` on this actor wins (its root
                // offset is the cinematic host's).
                let scripted = world.get_resource::<crate::cinematics::CinematicScene>().and_then(|s| s.actor_clip(actor));
                let mut c = std::mem::take(&mut world.get_mut::<AdaptedWalk>(actor).unwrap().clips);
                {
                    let originals = world.resource::<Originals>();
                    match scripted {
                        Some((id, t)) => crate::creature_clips::follow_script(&mut c.player, originals, &clip_rig, id, t),
                        None => {
                            let pose = world.get::<PoseClip>(actor).copied();
                            let (id, looped, restart) = if let Some(p) = pose {
                                pose_clip_request(&mut c, originals, &p)
                            } else if let Some(editor) = &test_drive {
                                let look_clips = crate::creature_clips::clip(originals, crate::creature_clips::LOOKAT_START).is_some()
                                    && crate::creature_clips::clip(originals, crate::creature_clips::LOOKAT_LOOP).is_some();
                                crate::creature_clips::test_drive_request(editor, &mut c.elapsed, &mut c.stop, &c.player, look_clips)
                            } else if player {
                                let run = world.get_resource::<crate::game::RunProgress>();
                                let encounter = world.get_resource::<crate::creature_actions::Encounter>();
                                let hunger = world.get::<crate::game::Hunger>(actor);
                                match run {
                                    Some(run) => c.avatar.request(&c.player, originals, run, encounter, hunger, speed, &clip_rig),
                                    None => npc_request(&mut c, originals, speed, &clip_rig, None),
                                }
                            } else {
                                npc_request(&mut c, originals, speed, &clip_rig, mind.as_ref())
                            };
                            if restart || c.player.current_id() != Some(id) {
                                if let Some(k) = crate::creature_clips::clip(originals, id) {
                                    let bound = Arc::new(clip_rig.bind(k));
                                    if restart {
                                        c.player.restart(bound, looped);
                                    } else {
                                        c.player.play(bound, looped);
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(top) = c.player.current() {
                    if top.looped && top.bound.clip.stance_speed.is_some() {
                        let rate = crate::creature_clips::gait_rate(&top.bound.clip, speed, &clip_rig);
                        c.player.set_rate(rate);
                    }
                }
                if scripted.is_none() {
                    c.player.advance(dt);
                }
                // The overlay layer (`PoseOverlay`): bound when wanted, faded in
                // and out, dropped once it is gone.
                {
                    let wanted = world.get::<PoseOverlay>(actor).map(|o| o.0);
                    if let Some(id) = wanted {
                        if c.overlay.as_ref().is_none_or(|o| o.id != id) {
                            if let Some(k) = crate::creature_clips::clip(world.resource::<Originals>(), id) {
                                c.overlay = Some(NpcOverlay { id, bound: Arc::new(clip_rig.bind(k)), time: 0.0, weight: 0.0 });
                            }
                        }
                    }
                    if let Some(o) = c.overlay.as_mut() {
                        o.time += dt;
                        let goal = if wanted == Some(o.id) { 1.0 } else { 0.0 };
                        let step = dt / OVERLAY_FADE;
                        o.weight = if o.weight < goal { (o.weight + step).min(goal) } else { (o.weight - step).max(goal) };
                        if goal == 0.0 && o.weight <= 0.0 {
                            c.overlay = None;
                        }
                    }
                }
                // Pose level of detail as for the gait (not for an actor posed
                // off camera on purpose, `PoseAlways`).
                let mut due = true;
                if !player && world.get::<PoseAlways>(actor).is_none() {
                    if let Some((eye, forward)) = eye {
                        let to = root.translation - eye;
                        let distance = to.length().max(1e-3);
                        let radius = gait.height * model_scale + 1.;
                        let visible = distance < radius
                            || forward.dot(to / distance) > VIEW_HALF_ANGLE.cos() - radius / distance;
                        due = visible && (distance < FAR_POSE_DISTANCE || (frame + slot as u32) % 3 == 0);
                    }
                }
                let skeleton_ok = world.get::<GaitSkeleton>(actor).is_some_and(|skeleton| {
                    surfaces.iter().any(|s| s.entity == skeleton.holder)
                        && surfaces.iter().all(|s| s.owned)
                        && skeleton.joints.iter().all(|&j| world.get_entity(j).is_ok())
                });
                if due && (skeleton_ok || attach_skeleton(world, &mut cache, actor, source, &gait, &surfaces)) {
                    let started = std::time::Instant::now();
                    let skeleton = world.get::<GaitSkeleton>(actor).unwrap();
                    let (holder, joints) = (skeleton.holder, skeleton.joints.clone());
                    // The external look target in the model frame (the
                    // joints' parent is the visual child `holder`).
                    let world_target = if scripted.is_some() {
                        None
                    } else if player {
                        engaged_head.or(cursor)
                    } else if engaged.is_some() && world.get::<crate::persistence::WorldId>(actor).map(|w| w.0) == engaged {
                        player_head
                    } else {
                        None
                    };
                    let world_target = world.get::<crate::creature_look::LookAt>(actor).map(|l| l.0).or(world_target);
                    let target = if player && test_drive.is_some() && world.get::<crate::creature_look::LookAt>(actor).is_none() {
                        Some(clip_rig.editor_look())
                    } else {
                        world_target.and_then(|w| {
                            let inverse = world.get::<GlobalTransform>(holder)?.affine().inverse();
                            Some(crate::creature_clips::bevy_to_spore(inverse.transform_point3(w)))
                        })
                    };
                    let look = {
                        let mut walk = world.get_mut::<AdaptedWalk>(actor).unwrap();
                        match target {
                            Some(t) => Some(crate::creature_look::smooth(&mut walk.look, t, dt)),
                            None => {
                                walk.look = None;
                                None
                            }
                        }
                    };
                    let mut deforms = crate::creature_clips::BlockDeforms::new();
                    let overlays: Vec<(&crate::creature_clips::Bound, f32, f32)> =
                        c.overlay.iter().map(|o| (&*o.bound, o.weight, o.bound.clip.frame_at(o.time, true))).collect();
                    let pose = clip_rig.pose_layered(&c.player.mix(), &overlays, scripted.is_none(), look, gait.morph.is_some().then_some(&mut deforms));
                    if crate::creature_look::debug() && world.get::<crate::creature_look::LookAt>(actor).is_some() {
                        let heads: Vec<String> = (0..pose.len())
                            .filter(|&j| clip_rig.joints[j].caps.iter().any(|c| c == b"mout"))
                            .map(|j| format!("{j}:{:.2}", crate::creature_clips::rotation_of(&pose[j]) * Vec3::NEG_Y))
                            .collect();
                        info!("look debug {:?}: clip {:08X?} look {:.2?} heads {heads:?} deforms {}", actor, c.player.current_id(), look, deforms.iter().map(|d| d.len()).sum::<usize>());
                    }
                    let mut morph_joints = Vec::new();
                    if let Some(morph) = &gait.morph {
                        // The part slots persist per creature (`creature_morph::MorphState`).
                        let mut state = world.get_mut::<crate::creature_morph::MorphState>(actor).map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
                        morph.pose(&pose, &deforms, &mut state, &mut morph_joints);
                        world.entity_mut(actor).insert(state);
                    }
                    clip_cost.0 += 1;
                    clip_cost.1 += started.elapsed().as_secs_f64();
                    for (j, &entity) in joints.iter().enumerate().take(gait.joints.len()) {
                        if let (Some(m), Some(mut transform)) = (pose.get(j), world.get_mut::<Transform>(entity)) {
                            *transform = Transform::from_matrix(Mat4::from(crate::creature_clips::to_frame(m, 1.)));
                        }
                    }
                    for (j, m) in morph_joints {
                        if let Some(mut transform) = joints.get(j).and_then(|&e| world.get_mut::<Transform>(e)) {
                            *transform = Transform::from_matrix(Mat4::from(crate::creature_clips::to_frame(&m, 1.)));
                        }
                    }
                    // The first head block's posed position (FUN_00C176E0(2, 0)):
                    // what another creature's look target aims at.
                    if let (Some(j), Some(holder_global)) = (
                        clip_rig.joints.iter().position(|joint| joint.caps.iter().any(|c| c == b"mout")),
                        world.get::<GlobalTransform>(holder).copied(),
                    ) {
                        if let Some(m) = pose.get(j) {
                            let local = crate::creature_clips::to_frame(m, 1.).translation;
                            let head = holder_global.affine().transform_point3(Vec3::from(local));
                            if head.is_finite() {
                                world.entity_mut(actor).insert(crate::creature_look::PosedHead(head));
                            }
                        }
                    }
                    if player {
                        native_player_posed = true;
                    }
                }
                if let Some(mut walk) = world.get_mut::<AdaptedWalk>(actor) {
                    walk.clips = c;
                    walk.posed = true;
                    walk.clip_posed = true;
                }
                continue;
            }

            // NPC body motion on the visual children: dip at double support,
            // rock over the planted side, lean into speed and turns, breathe.
            // The player's root already bobs and leans in `creature_walk`.
            if !player {
                let run = smoothstep(1.8, 4.5, relative);
                let walking = stride.blend;
                let legged = rig.as_ref().is_some_and(|r| !r.legs.is_empty());
                let stance = stride.phase - PI * stride.duty;
                let dip = leg_length * (0.04 + 0.05 * run) * walking;
                let bob = -dip * 0.5 * (1. - (2. * stance).cos());
                // Planted feet stay on the ground while the body dips.
                stride.sink = -bob;
                let rock = (0.045 + 0.03 * run) * walking;
                let idle = 1. - walking;
                let roll = -rock * stance.cos()
                    + (turn * 0.06 * walking).clamp(-0.12, 0.12)
                    + 0.015 * idle * (0.8 * clock + seed).sin();
                let pitch =
                    -(0.025 + 0.06 * run) * walking + 0.012 * idle * (0.5 * clock + seed).sin();
                let wiggle = if legged {
                    0.02 * walking * stance.sin()
                } else {
                    // Legless bodies slither instead of stepping.
                    0.08 * walking * stride.phase.sin()
                };
                let breath = 1. + 0.012 * idle * (1.9 * clock + seed).sin();
                for surface in &surfaces {
                    let rest = surface.rest.unwrap_or(surface.transform);
                    let pivot = rest.translation;
                    let pose = Mat4::from_translation(pivot + Vec3::Y * bob * rest.scale.y)
                        * Mat4::from_quat(Quat::from_euler(EulerRot::YXZ, wiggle, pitch, roll))
                        * Mat4::from_scale(Vec3::new(1., breath, 1.))
                        * Mat4::from_translation(-pivot);
                    let posed = Transform::from_matrix(pose * rest.to_matrix());
                    if let Some(mut transform) = world.get_mut::<Transform>(surface.entity) {
                        *transform = posed;
                    }
                }
            }

            let Some(rig) = rig.filter(|r| r.animated()) else {
                continue;
            };
            if stride.blend == 0. && !posed {
                continue;
            }
            let skeleton_ok = world.get::<GaitSkeleton>(actor).is_some_and(|skeleton| {
                surfaces.iter().any(|s| s.entity == skeleton.holder)
                    && surfaces.iter().all(|s| s.owned)
                    && skeleton.joints.iter().all(|&j| world.get_entity(j).is_ok())
            });
            if !skeleton_ok && !attach_skeleton(world, &mut cache, actor, source, &rig, &surfaces) {
                continue;
            }
            let settling = stride.blend == 0.;
            if !player && !settling {
                if let Some((eye, forward)) = eye {
                    let to = root.translation - eye;
                    let distance = to.length().max(1e-3);
                    let radius = rig.height * model_scale + 1.;
                    let visible = distance < radius
                        || forward.dot(to / distance) > VIEW_HALF_ANGLE.cos() - radius / distance;
                    let due = distance < FAR_POSE_DISTANCE || (frame + slot as u32) % 3 == 0;
                    if !visible || !due {
                        continue;
                    }
                }
            }
            let matrices = rig.matrices(&stride);
            let joints = world.get::<GaitSkeleton>(actor).unwrap().joints.clone();
            let all = world
                .get_mut::<AdaptedWalk>(actor)
                .is_some_and(|mut walk| std::mem::take(&mut walk.clip_posed));
            for (j, &entity) in joints.iter().enumerate().take(rig.joints.len()) {
                if !rig.moving[j] && !all {
                    continue;
                }
                if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                    *transform = Transform::from_matrix(Mat4::from(matrices[j]));
                }
            }
            // Part bones with their own joints ride their block rigidly.
            for (j, m) in rig.rigid_morph(&matrices) {
                if let Some(mut transform) = joints.get(j).and_then(|&e| world.get_mut::<Transform>(e)) {
                    *transform = Transform::from_matrix(Mat4::from(m));
                }
            }
            if let Some(mut walk) = world.get_mut::<AdaptedWalk>(actor) {
                walk.posed = !settling;
            }
        }
    });
    if let Some(mut drive) = world.get_resource_mut::<crate::creature_clips::ClipDrive>() {
        drive.native = native_player_posed;
    }
    if clip_cost.0 > 0 && world.resource::<WalkTemplates>().frame % 600 == 0 {
        info!("clip poses: {} creatures this frame, {:.0} us", clip_cost.0, clip_cost.1 * 1e6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_weights_blend_joint_matrices() {
        let mut matrices = vec![Affine3A::IDENTITY; 2];
        matrices[1] = Affine3A::from_translation(Vec3::X * 4.);
        let p = skin_point(&matrices, [1, 0, 0, 0], [0.25, 0.75, 0., 0.], Vec3::Z);
        assert_eq!(p, Vec3::new(1., 0., 1.));
        matrices[1] = Affine3A::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let p = skin_point(&matrices, [1, 0, 0, 0], [1., 0., 0., 0.], Vec3::X);
        assert!(p.abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn skinned_copy_adds_joint_palette_and_keeps_source_mesh() {
        let mut meshes = Assets::<Mesh>::default();
        let original = meshes.add(Cuboid::new(1., 1., 1.));
        let source = meshes.get(&original).unwrap().clone();
        let count = match source.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(values) => values.len(),
            _ => panic!("cuboid positions should be float3"),
        };
        let skin = SectionSkin {
            section: 0,
            joints: (0..count).map(|v| [(v % 3) as u16, 0, 0, 0]).collect(),
            weights: vec![[1., 0., 0., 0.]; count],
        };
        let skinned = skinned_surface_mesh(&source, &skin).unwrap();
        assert!(skinned.attribute(Mesh::ATTRIBUTE_JOINT_INDEX).is_some());
        assert!(skinned.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT).is_some());
        assert!(skinned.skinned_mesh_bounds().is_some());
        assert_eq!(
            skinned.attribute(Mesh::ATTRIBUTE_POSITION),
            source.attribute(Mesh::ATTRIBUTE_POSITION),
            "skinning happens on the GPU; rest geometry is unchanged"
        );
        let original_mesh = meshes.get(&original).unwrap();
        assert!(original_mesh
            .attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
            .is_none());
        // A palette that does not cover the vertex buffer is refused.
        let short = SectionSkin {
            joints: skin.joints[1..].to_vec(),
            weights: skin.weights[1..].to_vec(),
            ..skin
        };
        assert!(skinned_surface_mesh(&source, &short).is_none());
    }

    #[test]
    fn native_creatures_with_source_surface_children_are_eligible() {
        let mut world = World::new();
        let source = ResourceKey {
            group_id: baked_model::CREATURE_GROUP,
            instance_id: 0x0123_4567,
            type_id: crate::decode::TYPE_GMDL,
        };
        let object = crate::world_objects::OriginalWorldObject {
            definition: source,
            model: source,
            name: "NPC surface".into(),
        };
        let surface = || crate::material_binding::OriginalSurfaceChild {
            source_section: 0,
            state_sections: vec![],
        };
        let actor = world.spawn(baked_model::NativeCreature).id();
        world.spawn((ChildOf(actor), object.clone(), surface()));
        assert_eq!(native_source(&world, actor), Some(source));

        let unmarked = world.spawn_empty().id();
        world.spawn((ChildOf(unmarked), object.clone(), surface()));
        assert_eq!(native_source(&world, unmarked), None);

        // A mounted world object that is not a material surface is not a body.
        let bare = world.spawn(baked_model::NativeCreature).id();
        world.spawn((ChildOf(bare), object));
        assert_eq!(native_source(&world, bare), None);
    }

    #[test]
    fn two_bone_ik_reaches_target_and_keeps_knee_side() {
        // Hip above a forward-bent knee, foot below the hip (y, z coordinates).
        let (hip, knee, foot) = (Vec2::new(1., 0.), Vec2::new(0.5, -0.2), Vec2::new(0., 0.));
        for target in [
            Vec2::new(0., -0.3),
            Vec2::new(0., 0.3),
            Vec2::new(0.25, 0.1),
            Vec2::new(0.1, -0.15),
        ] {
            let (a, b) = two_bone(hip, knee, foot, target);
            let knee2 = hip + Vec2::from_angle(a).rotate(knee - hip);
            let foot2 = knee2 + Vec2::from_angle(a + b).rotate(foot - knee);
            assert!(foot2.distance(target) < 1e-3, "{target} -> {foot2}");
            let side = (foot - hip).perp_dot(knee - hip).signum();
            assert_eq!((foot2 - hip).perp_dot(knee2 - hip).signum(), side);
        }
        // Unreachable targets straighten toward the target instead of failing.
        let (a, b) = two_bone(hip, knee, foot, Vec2::new(-5., 0.));
        assert!(a.is_finite() && b.is_finite());
        // The sagittal angle convention matches a Bevy X rotation.
        let v = Vec3::new(0., -1., 0.2);
        let rotated = Quat::from_rotation_x(0.4) * v;
        assert!(wrap(angle_of(yz(rotated)) - angle_of(yz(v)) - 0.4).abs() < 1e-5);
    }

    /// Every installed creature model with a same-instance anatomy: does the
    /// order-based palette mapping build, and how well does it agree with
    /// the geometry (the clip rig gate `MIN_AGREEMENT`)?
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_species_palette_mapping_census() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.group_id == baked_model::CREATURE_GROUP && k.type_id == crate::decode::TYPE_GMDL)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let (mut built, mut accepted, mut errors) = (0, 0, std::collections::BTreeMap::<String, usize>::new());
        let mut agreements = Vec::new();
        for key in &keys {
            let anatomy_key = ResourceKey { type_id: crate::creature_data::TYPE, group_id: crate::original_species::CREATURE_ANATOMY_GROUP, instance_id: key.instance_id };
            let result = (|| -> Result<GaitRig, String> {
                let model = baked_model::parse(&index.read(*key)?)?;
                let anatomy = crate::creature_data::parse(&index.read(anatomy_key)?)?;
                GaitRig::build(&model, &anatomy)
            })();
            match result {
                Ok(rig) => {
                    built += 1;
                    agreements.push(rig.agreement);
                    if rig.agreement >= MIN_AGREEMENT && rig.clips.is_some() {
                        accepted += 1;
                    }
                    if key.instance_id == baked_model::PLAYER_BASELINE_INSTANCE {
                        println!("starter agreement {:.3}, clips {}", rig.agreement, rig.clips.is_some());
                    }
                }
                Err(e) => *errors.entry(e.split(|c: char| c.is_ascii_digit()).next().unwrap_or("").to_string()).or_default() += 1,
            }
        }
        agreements.sort_by(|a, b| a.total_cmp(b));
        let pct = |q: f32| agreements.get(((agreements.len() as f32 - 1.) * q) as usize).copied().unwrap_or(0.);
        println!(
            "{} creature models: {built} built ({accepted} pass {MIN_AGREEMENT}), agreement p10 {:.2} p50 {:.2} p90 {:.2}; errors {errors:?}",
            keys.len(),
            pct(0.1),
            pct(0.5),
            pct(0.9)
        );
        assert!(built > 0);
    }

    /// Every installed creature model: the structural binding (counts and
    /// part rig keys, `part_bindings_match`) and, as a geometric cross-check,
    /// how many skinned vertices (weighted by slot) have a centroid closest,
    /// among the three nearest bone segments, to their owner's segment.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_palette_structure_census() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.group_id == baked_model::CREATURE_GROUP && k.type_id == crate::decode::TYPE_GMDL)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let (mut structural, mut old_gate, mut keyed, mut reordered, mut clipped) = (0, 0, 0usize, 0, 0);
        let mut rows = Vec::new();
        for key in &keys {
            let anatomy_key = ResourceKey { type_id: crate::creature_data::TYPE, group_id: crate::original_species::CREATURE_ANATOMY_GROUP, instance_id: key.instance_id };
            let Ok(model) = index.read(*key).and_then(|b| baked_model::parse(&b)) else { continue };
            let Ok(anatomy) = index.read(anatomy_key).and_then(|b| crate::creature_data::parse(&b)) else { continue };
            reordered += usize::from(anatomy.rigblocks.first().is_some_and(|b| b.parent_index() != -1));
            let Ok(rig) = GaitRig::build(&model, &anatomy) else { continue };
            old_gate += usize::from(rig.agreement >= MIN_AGREEMENT);
            let parts = part_bindings_match(&model, &anatomy, &|group, instance| part_model(&index, group, instance));
            match &parts {
                Ok(n) => {
                    structural += 1;
                    keyed += n;
                    clipped += usize::from(rig.clips.is_some());
                }
                Err(e) => println!("structural {:08X}: {e}", key.instance_id),
            }
            let owners = bone_owners(&model, &anatomy).unwrap();
            let pivots: Vec<Vec3> = anatomy.rigblocks.iter().map(|b| Vec3::from(b.position_bits().map(f32::from_bits))).collect();
            let kinds: Vec<i8> = anatomy.rigblocks.iter().map(|b| b.block_type()).collect();
            let parent: Vec<Option<usize>> = anatomy.rigblocks.iter().map(|b| usize::try_from(b.parent_index()).ok()).collect();
            // Bone segments: pivot -> each child pivot, pivot -> half way to
            // the parent; parts and leaves are points.
            let segment_distance = |j: usize, c: Vec3| -> f32 {
                let seg = |a: Vec3, b: Vec3| {
                    let ab = b - a;
                    let t = if ab.length_squared() > 0. { ((c - a).dot(ab) / ab.length_squared()).clamp(0., 1.) } else { 0. };
                    (a + ab * t).distance(c)
                };
                let mut d = pivots[j].distance(c);
                for (k, p) in parent.iter().enumerate() {
                    if *p == Some(j) && kinds[k] != DECORATION {
                        d = d.min(seg(pivots[j], pivots[k]));
                    }
                }
                if let Some(p) = parent[j] {
                    d = d.min(seg(pivots[j], (pivots[j] + pivots[p]) * 0.5));
                }
                d
            };
            let total: usize = model.bone_ranges.iter().map(|r| (r[0] + r[1]) as usize).max().unwrap_or(0);
            let mut sum = vec![(Vec3::ZERO, 0usize); total.max(1)];
            for (piece, range) in model.pieces.iter().zip(&model.bone_ranges) {
                let Ok(drawn) = crate::animation_layout::drawn_vertices(piece) else { continue };
                for v in (0..piece.positions.len()).filter(|&v| drawn[v]) {
                    let Some(i) = (0..4).filter(|&i| piece.weights[v][i] > 0.).max_by(|&a, &b| piece.weights[v][a].total_cmp(&piece.weights[v][b])) else { continue };
                    let slot = (range[0] + u32::from(piece.bones[v][i] / 3)) as usize;
                    if slot < sum.len() {
                        sum[slot].0 += Vec3::from(piece.positions[v]);
                        sum[slot].1 += 1;
                    }
                }
            }
            let (mut good, mut all) = (0usize, 0usize);
            for slot in 0..(model.root_bones as usize).min(sum.len()) {
                let (p, n) = sum[slot];
                if n == 0 {
                    continue;
                }
                let c = p / n as f32;
                let own = segment_distance(owners[slot], c);
                let closer = (0..pivots.len()).filter(|&j| kinds[j] != DECORATION && j != owners[slot] && segment_distance(j, c) < own).count();
                good += if closer < 3 { n } else { 0 };
                all += n;
            }
            let fit = if all == 0 { 1. } else { good as f32 / all as f32 };
            rows.push((fit, key.instance_id, rig.agreement, parts.err()));
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        for r in rows.iter().take(12) {
            println!("worst {:08X}: segment fit {:.2}, old agreement {:.2}, parts {:?}", r.1, r.0, r.2, r.3);
        }
        let pct = |q: f32| rows.get(((rows.len() as f32 - 1.) * q) as usize).map_or(0., |r| r.0);
        println!(
            "{} models built: {structural} pass the structural binding ({keyed} part rig keys matched, {clipped} with a clip rig), {old_gate} passed the old {MIN_AGREEMENT} gate, {reordered} reorder block 0; segment fit p0 {:.2} p10 {:.2} p50 {:.2}",
            rows.len(),
            pct(0.),
            pct(0.1),
            pct(0.5)
        );
        assert_eq!(structural, rows.len(), "every installed creature model binds");
    }

    /// Research dump (palette slot -> rigblock evidence) for the clip-rig
    /// mapping; prints, asserts nothing.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_palette_slot_dump() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.group_id == baked_model::CREATURE_GROUP && k.type_id == crate::decode::TYPE_GMDL)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let detail: usize = std::env::var("DUMP_DETAIL").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        let mut shown = 0;
        for key in &keys {
            let anatomy_key = ResourceKey { type_id: crate::creature_data::TYPE, group_id: crate::original_species::CREATURE_ANATOMY_GROUP, instance_id: key.instance_id };
            let Ok(model) = index.read(*key).and_then(|b| baked_model::parse(&b)) else { continue };
            let Ok(anatomy) = index.read(anatomy_key).map_err(|e| e.to_string()).and_then(|b| crate::creature_data::parse(&b).map_err(|e| e.to_string())) else { continue };
            let agreement = GaitRig::build(&model, &anatomy).map(|r| r.agreement).unwrap_or(-1.);
            let f2: Vec<i16> = anatomy.rigblocks.iter().map(|b| i16::from_le_bytes([b.raw_bytes[2], b.raw_bytes[3]])).collect();
            let kinds: Vec<i8> = anatomy.rigblocks.iter().map(|b| b.block_type()).collect();
            let flags: Vec<u16> = anatomy.rigblocks.iter().map(|b| b.flags()).collect();
            println!(
                "M {:08X} agree {:.2} blocks {} root_bones {} anim {} ranges {:?} f2 {:?} kinds {:?} flags {:x?}",
                key.instance_id, agreement, kinds.len(), model.root_bones, model.anim_data.len(), model.bone_ranges, f2, kinds, flags
            );
            let only = std::env::var("DUMP_ONLY").ok().and_then(|v| u32::from_str_radix(&v, 16).ok());
            if only.is_some_and(|o| o != key.instance_id)
                || (only.is_none() && !(agreement >= 0. && agreement < 0.7 && shown < detail) && key.instance_id != baked_model::PLAYER_BASELINE_INSTANCE)
            {
                continue;
            }
            shown += 1;
            for (i, b) in anatomy.rigblocks.iter().enumerate() {
                let p = b.position_bits().map(f32::from_bits);
                println!(
                    "  B{i} f2 {} par {} sym {} type {} flags {:x} pos [{:.3} {:.3} {:.3}] scale {:.3} part {:08X}!{:08X}",
                    f2[i], b.parent_index(), b.symmetric_index(), kinds[i], flags[i], p[0], p[1], p[2], f32::from_bits(b.scale_bits()), b.group_id(), b.instance_id()
                );
            }
            for (a, data) in model.anim_data.iter().enumerate() {
                let t = &data.transform;
                let f = |o: usize| f32::from_le_bytes(t[o..o + 4].try_into().unwrap());
                println!(
                    "  A{a} f70 {:x} key {:08X}!{:08X}.{:08X} groups {:?} tflags {:x} off [{:.3} {:.3} {:.3}] scale {:.3}",
                    data.field_70, data.key.group_id, data.key.instance_id, data.key.type_id,
                    data.baked_deforms.iter().map(|g| g.len()).collect::<Vec<_>>(),
                    u16::from_le_bytes([t[0], t[1]]), f(4), f(8), f(12), f(16)
                );
            }
            // Centroid of the drawn vertices per palette slot (dominant influence).
            let total: usize = model.bone_ranges.iter().map(|r| (r[0] + r[1]) as usize).max().unwrap_or(0);
            let mut sum = vec![(Vec3::ZERO, 0usize); total.max(1)];
            for (piece, range) in model.pieces.iter().zip(&model.bone_ranges) {
                let Ok(drawn) = crate::animation_layout::drawn_vertices(piece) else { continue };
                for v in (0..piece.positions.len()).filter(|&v| drawn[v]) {
                    let Some(i) = (0..4).filter(|&i| piece.weights[v][i] > 0.).max_by(|&a, &b| piece.weights[v][a].total_cmp(&piece.weights[v][b])) else { continue };
                    let slot = (range[0] + u32::from(piece.bones[v][i] / 3)) as usize;
                    if slot < sum.len() {
                        sum[slot].0 += Vec3::from(piece.positions[v]);
                        sum[slot].1 += 1;
                    }
                }
            }
            for (s, (p, n)) in sum.iter().enumerate() {
                if *n == 0 {
                    println!("  S{s} empty");
                    continue;
                }
                let c = *p / *n as f32;
                let mut near: Vec<(f32, usize)> = anatomy.rigblocks.iter().enumerate().map(|(i, b)| (Vec3::from(b.position_bits().map(f32::from_bits)).distance(c), i)).collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                println!("  S{s} n {n} c [{:.3} {:.3} {:.3}] near {:?}", c.x, c.y, c.z, &near[..near.len().min(3)]);
            }
        }
    }

    #[test]
    fn foot_path_plants_then_swings_forward() {
        let duty = 0.6;
        assert_eq!(foot_path(0., duty), (0.5, 0.));
        let (ahead, lift) = foot_path(0.3, duty);
        assert!(ahead.abs() < 1e-5 && lift == 0.);
        let (ahead, lift) = foot_path(0.8, duty);
        assert!(ahead.abs() < 1e-5 && (lift - 1.).abs() < 1e-5);
        let (ahead, _) = foot_path(0.999, duty);
        assert!(ahead > 0.49);
        // Stance moves monotonically backward.
        let samples: Vec<f32> = (0..6).map(|i| foot_path(i as f32 * 0.1, duty).0).collect();
        assert!(samples.windows(2).all(|w| w[1] < w[0]));
    }

    #[test]
    fn faster_gaits_take_longer_lower_duty_strides_within_reach() {
        let (walk, walk_duty, walk_lift) = stride_for(1., 1., 0.);
        let (run, run_duty, run_lift) = stride_for(6., 1., 0.);
        assert!(run > walk && run_duty < walk_duty && run_lift > walk_lift);
        let (capped, duty, _) = stride_for(6., 1., 0.2);
        assert!(capped * duty / 2. <= 0.2 * OVERREACH + 1e-5);
    }

    /// Synthetic biped: a two-vertebra spine, thigh/calf legs with feet,
    /// and one decoration part that owns no palette slots.
    fn biped() -> (baked_model::Model, CreatureData) {
        let block = |index: i16, parent: i16, kind: u8, p: Vec3| {
            let mut raw = vec![0u8; 140];
            raw[0..2].copy_from_slice(&index.to_le_bytes());
            raw[4..6].copy_from_slice(&parent.to_le_bytes());
            raw[6..8].copy_from_slice(&(-1i16).to_le_bytes());
            raw[0x0b] = kind;
            // Native basis: Bevy (x, y, z) = (-native x, native z, native y).
            for (i, v) in [-p.x, p.z, p.y].into_iter().enumerate() {
                raw[0x54 + i * 4..0x58 + i * 4].copy_from_slice(&v.to_bits().to_le_bytes());
            }
            crate::creature_data::RigBlock { raw_bytes: raw }
        };
        let rigblocks = vec![
            block(0, -1, 5, Vec3::new(0., 1., 0.)),
            block(1, 0, 5, Vec3::new(0., 0.9, 0.3)),
            block(2, 1, 3, Vec3::new(0.3, 0.8, 0.2)),
            block(3, 2, 3, Vec3::new(0.3, 0.45, -0.05)),
            block(4, 1, 3, Vec3::new(-0.3, 0.8, 0.2)),
            block(5, 4, 3, Vec3::new(-0.3, 0.45, -0.05)),
            block(6, 3, 2, Vec3::new(0.3, 0.1, 0.2)),
            block(7, 5, 2, Vec3::new(-0.3, 0.1, 0.2)),
            block(8, 0, 1, Vec3::new(0., 1.2, -0.2)),
        ];
        let anatomy = CreatureData {
            version: 10,
            properties: vec![0; 128],
            encoded_properties: vec![],
            rigblocks,
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        };
        let native = |p: Vec3| [-p.x, p.z, p.y];
        // One vertex per boned rigblock at its pivot, a toe on each foot
        // group and a floating head decoration.
        let mut positions: Vec<[f32; 3]> = anatomy
            .rigblocks
            .iter()
            .take(8)
            .map(|b| native(basis(b.position_bits().map(f32::from_bits))))
            .collect();
        let mut bones: Vec<[u8; 4]> = (0..8u8).map(|i| [i * 3, 0, 0, 0]).collect();
        positions.push(native(Vec3::new(0.3, 0., 0.)));
        bones.push([8 * 3, 0, 0, 0]);
        positions.push(native(Vec3::new(-0.3, 0., 0.)));
        bones.push([9 * 3, 0, 0, 0]);
        let n = positions.len();
        let no_rig = ResourceKey {
            group_id: u32::MAX,
            instance_id: u32::MAX,
            type_id: u32::MAX,
        };
        let rig = ResourceKey {
            group_id: 1,
            instance_id: 2,
            type_id: crate::asset_catalog::RW4,
        };
        let binding = |key, field_70, bones: usize| baked_model::GameModelAnimData {
            transform: [0; 56],
            field_38: [0; 56],
            field_70,
            key,
            baked_deforms: if bones == 0 {
                vec![]
            } else {
                vec![vec![[0; 160]; bones]]
            },
        };
        let model = baked_model::Model {
            version: 9,
            references: vec![],
            pieces: vec![baked_model::Piece {
                positions,
                normals: vec![[0., 0., 1.]; n],
                uvs: vec![[0.; 2]; n],
                indices: (0..n as u32)
                    .chain([0, 0])
                    .take(n.div_ceil(3) * 3)
                    .collect(),
                bones,
                weights: vec![[1., 0., 0., 0.]; n],
                material_id: 0,
            }],
            materials: vec![],
            bone_ranges: vec![[0, 10]],
            root_bones: 8,
            anim_data: vec![
                binding(no_rig, 8, 0),
                binding(rig, u32::MAX, 1),
                binding(rig, u32::MAX, 1),
                binding(no_rig, 0, 0),
            ],
            unknown_key: no_rig,
            native_geometry: vec![],
        };
        (model, anatomy)
    }

    #[test]
    fn palette_slots_follow_boned_rigblocks_then_part_groups() {
        let (model, anatomy) = biped();
        assert_eq!(
            bone_owners(&model, &anatomy).unwrap(),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 6, 7]
        );
        let mut broken = model.clone();
        broken.root_bones = 9;
        assert!(bone_owners(&broken, &anatomy).is_err());
        let mut broken = model;
        broken.anim_data.pop();
        assert!(bone_owners(&broken, &anatomy).is_err());
    }

    #[test]
    fn drawn_weighted_slots_must_stay_inside_their_piece_range() {
        let (mut model, anatomy) = biped();
        // The final drawn vertex addresses slot 9; a range ending at 9 must
        // fail rather than borrowing an anatomy owner from the next group.
        model.bone_ranges[0] = [0, 9];
        let error = GaitRig::build(&model, &anatomy).unwrap_err();
        assert!(error.contains("falls outside piece range"), "{error}");
    }

    #[test]
    fn grounded_limbs_become_alternating_legs_that_step_and_rest_exactly() {
        let (model, anatomy) = biped();
        let rig = GaitRig::build(&model, &anatomy).unwrap();
        assert_eq!(rig.legs.len(), 2);
        assert!(rig.arms.is_empty());
        let offsets: Vec<f32> = rig.legs.iter().map(|l| l.offset).collect();
        assert!((offsets[0] - offsets[1]).abs() == 0.5, "sides alternate");
        for leg in &rig.legs {
            assert!(leg.knee.is_some() && leg.foot.is_some());
        }
        // Spine and head stay put; legs, feet and their toe groups move.
        assert_eq!(
            rig.moving,
            vec![false, false, true, true, true, true, true, true, false]
        );
        let palette = &rig.sections[0];
        assert_eq!(palette.joints[8][0], 6, "toe group follows its foot");
        assert_eq!(palette.joints[9][0], 7);
        let rest = rig.matrices(&Stride::default());
        assert!(rest.iter().all(|m| *m == Affine3A::IDENTITY));
        let stride = Stride {
            phase: 0.,
            blend: 1.,
            length: 0.8,
            duty: 0.6,
            lift: 0.15,
            sink: 0.,
            swing: 0.3,
        };
        let matrices = rig.matrices(&stride);
        assert!(matrices[0] == Affine3A::IDENTITY && matrices[1] == Affine3A::IDENTITY);
        // At phase 0 the first leg strikes ahead (-Z) on the ground, and the
        // other one is about to swing from behind.
        let lead = &rig.legs[0];
        let foot = matrices[lead.foot.unwrap()].transform_point3(lead.effector);
        let expected = (0.5 * stride.length * stride.duty).min(lead.reach * OVERREACH);
        assert!(
            (foot.z - (lead.effector.z - expected)).abs() < 1e-3,
            "{foot}"
        );
        assert!((foot.y - lead.effector.y).abs() < 1e-3);
        let trail = &rig.legs[1];
        let mid_swing = Stride {
            phase: TAU * 0.3,
            ..stride
        };
        let lifted = rig.matrices(&mid_swing)[trail.foot.unwrap()].transform_point3(trail.effector);
        assert!(
            lifted.y > trail.effector.y + 0.1,
            "swing foot clears ground"
        );
    }

    #[test]
    #[ignore = "requires original Spore installation"]
    fn installed_species_rigs_find_legs_and_preserve_rest() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let keys = baked_model::candidates(&index, baked_model::CREATURE_GROUP);
        let mut picks: Vec<ResourceKey> = (0..12).map(|s| keys[(s * 37) % keys.len()]).collect();
        picks.extend(keys.iter().step_by(keys.len() / 10 + 1).copied());
        picks.push(ResourceKey {
            group_id: baked_model::CREATURE_GROUP,
            instance_id: baked_model::PLAYER_BASELINE_INSTANCE,
            type_id: crate::decode::TYPE_GMDL,
        });
        let (mut bound, mut legged) = (0, 0);
        for key in &picks {
            let model = baked_model::parse(&index.read(*key).unwrap()).unwrap();
            let Ok(anatomy) = index
                .read(ResourceKey {
                    type_id: crate::creature_data::TYPE,
                    group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
                    instance_id: key.instance_id,
                })
                .and_then(|b| crate::creature_data::parse(&b))
            else {
                println!("{:08X}: no anatomy", key.instance_id);
                continue;
            };
            let rig = match GaitRig::build(&model, &anatomy) {
                Ok(rig) => rig,
                Err(e) => {
                    println!("{:08X}: {e}", key.instance_id);
                    continue;
                }
            };
            bound += 1;
            legged += usize::from(!rig.legs.is_empty());
            println!(
                "{:08X}: {} legs {:?} arms {} leg length {:.2} reach {:.2} joints {}",
                key.instance_id,
                rig.legs.len(),
                rig.legs
                    .iter()
                    .map(|l| (l.hip, l.knee, l.foot, l.offset))
                    .collect::<Vec<_>>(),
                rig.arms.len(),
                rig.leg_length,
                rig.reach(),
                rig.joints.len()
            );
            let rest = rig.matrices(&Stride::default());
            let walking = rig.matrices(&Stride {
                phase: 1.3,
                blend: 1.,
                length: rig.leg_length * 2.,
                duty: 0.6,
                lift: rig.leg_length * 0.2,
                sink: 0.,
                swing: 0.3,
            });
            let mut moved = 0;
            for skin in &rig.sections {
                let piece = &model.pieces[skin.section];
                assert_eq!(skin.joints.len(), piece.positions.len());
                for v in 0..piece.positions.len() {
                    let position = basis(piece.positions[v]);
                    let total: f32 = skin.weights[v].iter().sum();
                    assert!((total - 1.).abs() < 1e-4);
                    let p = skin_point(&rest, skin.joints[v], skin.weights[v], position);
                    assert!(p.abs_diff_eq(position, 1e-5));
                    let p = skin_point(&walking, skin.joints[v], skin.weights[v], position);
                    assert!(p.is_finite());
                    moved += usize::from(p.distance(position) > 1e-3);
                }
            }
            assert!(rig.legs.is_empty() || moved > 50, "legs deform: {moved}");
        }
        assert!(bound * 4 >= picks.len() * 3, "most species bind: {bound}");
        assert!(legged * 2 >= bound, "most species have legs: {legged}");
    }
}
