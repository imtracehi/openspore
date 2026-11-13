//! The player's species as a crowd: the tribe's members and chieftain in the
//! Tribal stage and the citizens of the capital in Civilization, drawn with
//! the outfitted skinned creature (the retail metaball skin with its bone
//! weights, the species' parts and the stage's accessories) and posed by the
//! retail creature clips (`creature_clips`).
//!
//! One skinned copy of the species' skin mesh (`creature_blocks` +
//! `creature_skin_view`, as `creature_clips::drive_player` builds it for the
//! avatar) is shared by every actor; each actor has its own joints (children,
//! posed in the creature's frame with identity bind poses), its own clip
//! player and request. The mounted parts (species parts and the outfit's
//! accessories) ride the block they sit on: a species part its own block,
//! an accessory the nearest block (ADAPTED: retail pins accessories to the
//! block under them, `EditorRigblock` pinning, which the port does not keep).
//!
//! Clips (ORIGINAL data: the tribe TLSA groups of `_final/tribe/`): the retail
//! citizen action table [`tribal_plan`] (tribal-final, notes T9K: every
//! PlayAnimation site of a tribe group was found in the code image and read).
//! Idles and walks toward a target come from the tool PROPs
//! (`toolData_DefaultToolIdle` / `LookaroundToolIdle` / `EnRouteAnim`; 0xDA7C10,
//! 0xDAAF40, 0xDB7120), attacks from the tool abilities
//! (`creatureAbilityAnimationIDs`, `...BuildingSpecificAnimationID` on huts;
//! 0xC1E5C0) with `attack_idle_gen02` between them (0xD69740), fishing is
//! start / loop / pull / catch / dump by tool and chieftain (0xD73FD0), eating
//! `mot_pickupfood_toeat` then `mot_eathand`, gathering `gathr_picktree` /
//! `gathr_treesmacker` / `chief_gathr_treesmacker` (0xD75F70), the social
//! minigame start / `_loop_a` / conductor / no-tool dance (0xDA3700) with the
//! `_loop_solo` solos on an answered note (0xDA0100), healing `shaman_heal_01`
//! toward a patient (0xDB9AC0), taking a tool `get_tool` (0xC264E0). A member
//! in water (tribal-swim, T9J: a creature that enters water swims, 0xC04E30 ->
//! 0xC14F70) plays the creature swim groups `gen_swim` (moving) and
//! `gen_swim_inplace` (at rest), the ones the Creature avatar plays, and keeps
//! its handheld item (0xC14F70 only restarts the default locomotion through
//! 0xC148B0; no code read drops the item).
//!
//! Hands (VERIFIED, T9H): a member holds its tool's handheld item
//! (cCreatureCitizen::GetHandheldItemForTool 0xC23740) as the retail effect
//! the item names (0xC250A0 -> 0xC150D0 `CreateVisualEffect`), attached to its
//! grasper; the chieftain holds his staff for the attack, gathering, fishing
//! and chieftain tools and nothing else. While eating it holds the eaten food
//! (`trg_eating_*`); a fisher with a catch holds `trg_fishing_tool1_fish`.
//! The carried bundle (a cGameBundle) is its platter model on the bundle
//! grasper (`tribal_village::carried_platters`, VERIFIED 0xE8CD10 / 0xC35CC0).
//! The favoured grasper (+0xFC4) and the bundle grasper (+0xFC8) of a tribe
//! member are chosen per creature (0xC24270, VERIFIED): entry `id % n` of the
//! creature's appendage list, and the next entry after it whose flag byte
//! (+0x18) differs (the next entry when none does); see [`grasper_pair`].
//! ADAPTED: the list is the species' graspers in block order and the flag is
//! the side of the body (the layout of the retail entries is INFERRED).
//!
//! Level of detail: actors behind the camera are not posed, actors farther
//! than [`FAR_POSE_DISTANCE`] are posed every third frame.
//!
//! Switch: `OPENSPORE_CROWD_CLIPS=0` keeps the static bodies.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::*,
};

use crate::{
    creature_clips::{self as clips, Bound, ClipPlayer, ClipRig, Joint},
    creature_skin::BlockKind,
    game::{PartInstance, RunProgress},
    stages::{GameStage, StageOwned},
    tribal_rts::{TribeUnit, UnitTask},
    tribal_tools::ToolKind,
    Originals,
};

/// Tribe TLSA groups (`_final/tribe/...`). (The fishing, instrument and
/// dance groups below are superseded by [`tribe_clip`]; kept for the
/// installed-data bind test.)
#[allow(dead_code)]
pub mod clip_id {
    pub const IDLE: u32 = 0x02C3_9001; // soc_idle_neutral2
    pub const WALK: u32 = 0x0248_1DB1; // walk_neutral
    pub const WALK_CARRY: u32 = 0x02C3_8D91; // walk_carry
    pub const PLATTER_OVERLAY: u32 = 0x05F0_8F08; // platter_grasper_overlay
    pub const GATHER_HANDS: u32 = 0x02DF_4B0A; // gathr_picktree
    pub const GATHER_STICK: u32 = 0x02DF_4B12; // gathr_treesmacker
    pub const CHIEF_GATHER: u32 = 0x0579_081C; // chief_gathr_treesmacker
    pub const MEAT: u32 = 0x02C3_9201; // hunt_meat_hack
    pub const FISH: u32 = 0x02C3_9215; // fish_loop
    pub const FISH_SPEAR: u32 = 0x02C3_921B; // fish_spear
    pub const TORCH: u32 = 0x056D_2CF0; // torch_poke
    pub const AXE: u32 = 0x056D_2CF2; // axe_attack
    pub const SPEAR: u32 = 0x056D_2CF4; // spear_stab
    pub const CHIEF_ATTACK: u32 = 0x0576_6A10; // chief_attack_01
    pub const SHAMAN_ATTACK: u32 = 0x056E_9711; // shaman_attack_01
    pub const NO_TOOL_ATTACK: u32 = 0x0591_B019; // com_battle_club_1
    pub const HORN: u32 = 0x5508_F836; // soc_horn01_loop_solo
    pub const MARACAS: u32 = 0x7508_F905; // soc_shakern01_loop_solo (0xDA0100)
    pub const DIDGERIDOO: u32 = 0xB508_F8DA; // soc_didger01_loop_solo (0xDA0100)
    pub const DANCE: u32 = 0x02C3_9812; // soc_fire_dance_2_60med
    pub const HEAL: u32 = 0x0562_CFBF; // shaman_heal_01
    pub const EAT: u32 = 0x0279_6B38; // mot_eathand
    pub const TAME: u32 = 0x0565_4DF5; // tame_give_apple
    /// 0xC264E0: a citizen whose handheld item changes plays `get_tool`.
    pub const TOOL_RACK: u32 = 0x02C3_9200; // get_tool
    /// Swimming (the Creature avatar's groups, `creature_support`).
    pub const SWIM: u32 = crate::creature_support::SWIM; // gen_swim
    pub const SWIM_IN_PLACE: u32 = crate::creature_support::SWIM_IN_PLACE; // gen_swim_inplace

    /// Every group the crowd plays (the installed-data test decodes them).
    pub const ALL: [u32; 27] = [
        IDLE, WALK, WALK_CARRY, GATHER_HANDS, GATHER_STICK, CHIEF_GATHER, MEAT, FISH, FISH_SPEAR, TORCH, AXE, SPEAR,
        CHIEF_ATTACK, SHAMAN_ATTACK, NO_TOOL_ATTACK, HORN, MARACAS, DIDGERIDOO, DANCE, HEAL, EAT, TAME, TOOL_RACK,
        super::clips::RUN, SWIM, SWIM_IN_PLACE, PLATTER_OVERLAY,
    ];
}

/// The handheld items' effects (0xC250A0; the ids are the FNV names of the
/// effects `CreateVisualEffect` starts on the grasper).
pub mod handheld {
    /// trg_hunting_tool1..3 (torch, axe, spear).
    pub const HUNTING_TOOL: [u32; 3] = [0xDBFD_6844, 0xDBFD_6847, 0xDBFD_6846];
    pub const FISHING_TOOL: u32 = 0x1CF8_4D57;
    pub const FISHING_TOOL_FISH: u32 = 0xCE7C_D1C8;
    pub const CHIEFTAIN_STAFF_FISH: u32 = 0x1762_FC58;
    pub const EATING_FISH: u32 = 0x3A88_7470;
    pub const EATING_FRUIT: u32 = 0xAE02_4C94;
    pub const EATING_MEAT: u32 = 0xDC87_FF23;
    pub const MARACA: u32 = 0xB931_9AC6;
    pub const HORN: u32 = 0x5986_7820;
    pub const DIDGERIDOO: u32 = 0xF812_2731;
    pub const TREE_SMACKER: u32 = 0x759B_0E13;
    pub const CHIEFTAIN_STAFF: u32 = 0x8643_9B47;
    pub const HEALING_STAFF: u32 = 0x792E_9926;
    /// Tag of the handheld effects (`SwarmStop`).
    pub const TAG: u64 = 0x7472_675F_6861_6E64;
}

/// The handheld item effect of a tool (0xC23740 + 0xC250A0): the chieftain
/// shows his staff for the attack, gathering and fishing tools (and with no
/// tool: his own staff, type 11), nothing for the others.
pub fn tool_handheld(tool: Option<ToolKind>, chieftain: bool) -> Option<u32> {
    use handheld::*;
    let kind = tool.map(crate::tribal_rules::tool_type).unwrap_or(11);
    if chieftain {
        return matches!(kind, 1 | 2 | 7 | 8 | 11).then_some(CHIEFTAIN_STAFF);
    }
    Some(match kind {
        1 => HUNTING_TOOL[0],
        2 => HUNTING_TOOL[1],
        3 => HUNTING_TOOL[2],
        4 => HORN,
        5 => MARACA,
        6 => DIDGERIDOO,
        7 => TREE_SMACKER,
        8 => FISHING_TOOL,
        9 => HEALING_STAFF,
        _ => return None,
    })
}

/// What a tribe unit holds in its favoured grasper: the eaten food while
/// eating, a fisher's catch on its spear, else its tool's item. The carried
/// load is a bundle on the other (bundle) grasper, drawn as its platter
/// (`tribal_village::carried_platters`, the bundle view 0xE8CD10).
pub fn tribal_handheld(task: &UnitTask, unit: &TribeUnit, tool: Option<ToolKind>) -> Option<u32> {
    use crate::tribal_rts::FoodKind;
    use handheld::*;
    let fishing = tool == Some(ToolKind::FishingSpears) || unit.chieftain;
    if matches!(task, UnitTask::Recover) {
        return Some(EATING_FRUIT);
    }
    if unit.carrying > 0.0 && unit.carry_kind == FoodKind::Fish && fishing {
        return Some(if unit.chieftain { CHIEFTAIN_STAFF_FISH } else { FISHING_TOOL_FISH });
    }
    tool_handheld(tool, unit.chieftain)
}

/// A crowd actor's grasper anchor: the handheld effect rides it.
#[derive(Component)]
pub struct HandAnchor;

/// A crowd actor's bundle-grasper anchor: the carried platter rides it.
#[derive(Component)]
pub struct BundleAnchor;

/// Actors farther than this from the camera are posed every third frame.
pub const FAR_POSE_DISTANCE: f32 = 70.0;
/// Below this ground speed (leg lengths per second) an actor stands.
const STAND: f32 = 0.15;

/// The crowd is on unless `OPENSPORE_CROWD_CLIPS=0`.
pub fn enabled() -> bool {
    std::env::var("OPENSPORE_CROWD_CLIPS").map_or(true, |v| v != "0") && clips::enabled("crowd")
}

/// Index of a mounted part in `run.parts` followed by the stage's outfit
/// (`creature::mount_part`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountIndex(pub usize);

/// A skinning joint of a crowd actor.
#[derive(Component)]
pub struct CrowdJoint;

/// A citizen of the player's capital (Civilization): strolls between points
/// of the city (ADAPTED placement and paths; the retail city citizens'
/// behaviour was not traced).
#[derive(Component, Clone, Debug)]
pub struct CivCitizen {
    pub home: Vec3,
    pub target: Vec3,
    pub wait: f32,
    pub seed: u32,
}

impl CrowdActor {
    /// The mesh the actor drew before it was posed (the species' skin).
    pub fn original_mesh(&self) -> &Handle<Mesh> {
        &self.original_mesh
    }
}

/// The player's species speaking for its own colonies in Space (the comm
/// portrait, `space_hud::comm::update_portrait`): the creature in its Space
/// outfit, idling.
#[derive(Component)]
pub struct SpaceCaptain;

/// The portrait subject of the player's species at `at` on `layer`: the
/// outfitted skinned creature (body, species parts, the Space outfit, its
/// paint). Returns it and its bounds, or `None` without the retail skin (the
/// caller then shows the installed creature model).
pub fn spawn_captain(
    commands: &mut Commands,
    originals: &mut Originals,
    run: &RunProgress,
    (meshes, images, materials): (&mut Assets<Mesh>, &mut Assets<Image>, &mut ResMut<Assets<StandardMaterial>>),
    at: Vec3,
    layer: bevy::camera::visibility::RenderLayers,
) -> Option<(Entity, Vec3, Vec3)> {
    if !enabled() {
        return None;
    }
    let model = crate::baked_model::prepare_body(originals, &run.body, meshes, images, materials).ok()?;
    originals.baked_body.as_ref()?.skin.as_ref()?;
    let material = model.material.clone()?;
    let tris = crate::creature_skin_view::body_triangles(originals, &run.body);
    let (lo, hi) = tris
        .iter()
        .flat_map(|t| t.iter())
        .fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let e = commands
        .spawn((
            StageOwned,
            SpaceCaptain,
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(material),
            Transform::from_translation(at),
            Visibility::Visible,
            layer,
            bevy::light::NotShadowCaster,
            Name::new("space captain"),
        ))
        .id();
    crate::creature::mount_saved_parts(commands, materials, e, originals, run, 0, 1.0);
    // The mounted parts reach above the skin (hats): frame a little more.
    Some((e, at + lo - Vec3::Y * 0.45, at + hi + Vec3::Y * 0.6))
}

/// The captain's parts and joints draw on its portrait layer only.
fn captain_layers(
    mut commands: Commands,
    captains: Query<(&bevy::camera::visibility::RenderLayers, &Children), With<SpaceCaptain>>,
    unlayered: Query<(), (Without<bevy::camera::visibility::RenderLayers>, Without<CrowdJoint>)>,
) {
    for (layer, children) in &captains {
        for child in children.iter() {
            if unlayered.contains(child) {
                commands.entity(child).insert((layer.clone(), bevy::light::NotShadowCaster));
            }
        }
    }
}

/// A posed actor.
#[derive(Component)]
pub struct CrowdActor {
    joints: Vec<Entity>,
    /// Mounted part entity, its block and its rest transform (actor frame).
    parts: Vec<(Entity, usize, Transform)>,
    original_mesh: Handle<Mesh>,
    clips: ClipPlayer,
    last_pos: Vec3,
    /// Ground speed (creature units per second).
    speed: f32,
    generation: u64,
    seed: u32,
    /// Clip of the last request (for tests and logs).
    pub playing: u32,
    /// The grasper anchor and the handheld effect shown on it.
    hand: Option<Entity>,
    /// The bundle grasper's anchor (the carried platter rides it).
    bundle: Option<Entity>,
    /// The blocks of the favoured and the bundle graspers ([`grasper_pair`]).
    hand_block: Option<usize>,
    bundle_block: Option<usize>,
    pub handheld: Option<u32>,
    /// The retail clip sequence of a tribe member (intro, one-shots, main).
    seq: Sequencer,
    /// The overlay laid over the stack (`platter_grasper_overlay`): the clip,
    /// its clock and how far it has faded in.
    overlay: Option<OverlayLayer>,
}

/// An overlay clip and its fade (see [`ClipPlan::overlay`]).
struct OverlayLayer {
    id: u32,
    bound: Arc<Bound>,
    time: f32,
    weight: f32,
}

/// Seconds an overlay takes to fade in or out.
const OVERLAY_FADE: f32 = 0.25;

impl CrowdActor {
    /// The actor's clip layers (`anim_audio` plays their events).
    pub fn clip_player(&self) -> &ClipPlayer {
        &self.clips
    }
}

struct SharedSkin {
    /// The skin's draw mesh (what an un-posed actor shows).
    source: Handle<Mesh>,
    mesh: Handle<Mesh>,
    inverse: Handle<SkinnedMeshInverseBindposes>,
    rig: Arc<ClipRig>,
    unit: f32,
    joints: usize,
    /// Part index -> block.
    part_blocks: HashMap<usize, usize>,
    /// Block positions in the actor frame (rest).
    points: Vec<Vec3>,
    /// The species' grasper blocks in block order with their x position in
    /// the actor frame (the side of the body).
    graspers: Vec<(usize, f32)>,
    bound: HashMap<u32, Arc<Bound>>,
}

/// The species' shared skinned body (per app).
#[derive(Resource, Default)]
pub struct CrowdSkin {
    key: Option<(crate::body::BodyProfile, Vec<PartInstance>)>,
    generation: u64,
    shared: Option<SharedSkin>,
    frame: u32,
    /// Creatures posed in the last frame and the time it took (log).
    pub cost: (usize, f64),
    /// The performance last frame (right notes, the called note): an answered
    /// instrument note makes that instrument's players solo (0xDA0100).
    song: (u32, u32),
}

fn build_shared(
    originals: &Originals,
    run: &RunProgress,
    meshes: &mut Assets<Mesh>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<SharedSkin> {
    let source = originals.baked_body.as_ref()?.skin.as_ref()?.mesh.clone();
    let (blocks, b) = crate::creature_blocks::creature_blocks(originals, run)?;
    let joints: Vec<Joint> = blocks
        .iter()
        .map(|bl| match bl.record.and_then(|r| b.records.get(r)) {
            Some(r) => Joint {
                kind: r.kind,
                parent: bl.parent,
                pivot: r.position,
                at: if r.kind == BlockKind::Limb { r.segment_end() } else { r.position },
                caps: bl.caps.clone(),
            },
            None => {
                let p = clips::bevy_to_spore(bl.position) / b.unit.max(1e-4);
                Joint { kind: BlockKind::Standard, parent: bl.parent, pivot: p, at: p, caps: bl.caps.clone() }
            }
        })
        .collect();
    let ground = b.mesh.positions.iter().map(|p| p.z).fold(f32::MAX, f32::min);
    let rig = Arc::new(ClipRig::new(joints, ground).map_err(|e| warn!("crowd clip rig: {e}")).ok()?);
    let mut mesh = crate::creature_skin_view::render_mesh(&b);
    let src = &b.unwrapped.source;
    let joint_attr: Vec<[u16; 4]> = src.iter().map(|&s| b.mesh.bones[s as usize].map(|x| x as u16)).collect();
    let weights: Vec<[f32; 4]> = src.iter().map(|&s| b.mesh.weights[s as usize]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joint_attr));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    mesh.generate_skinned_mesh_bounds().ok()?;
    let inverse = bindposes.add(vec![Mat4::IDENTITY; b.records.len()]);
    let part_blocks = blocks.iter().enumerate().filter_map(|(i, bl)| Some((bl.part?, i))).collect();
    let points: Vec<Vec3> = rig.joints.iter().map(|j| clips::bevy_to_spore(j.pivot) * b.unit).collect();
    // Graspers carry the `grsp` capability (block flag 0x40).
    let graspers: Vec<(usize, f32)> = blocks
        .iter()
        .enumerate()
        .filter(|(_, bl)| bl.flags & 0x40 != 0 || bl.caps.iter().any(|c| c == b"grsp"))
        .filter_map(|(i, _)| points.get(i).map(|p| (i, p.x)))
        .collect();
    info!(
        "crowd skin: {} blocks, {} skin joints, leg length {:.2}",
        rig.len(),
        b.records.len(),
        rig.leg_length
    );
    Some(SharedSkin {
        source,
        mesh: meshes.add(mesh),
        inverse,
        rig,
        unit: b.unit,
        joints: b.records.len(),
        part_blocks,
        points,
        graspers,
        bound: HashMap::new(),
    })
}

/// The favoured and the bundle grasper of a tribe member (`seed` stands for
/// the creature's id, `cCreatureCitizen` +0x24), 0xC24270 (VERIFIED, march2017):
/// `i = id % n`, favoured = entry `i` of the appendage list (+0xE40, `n` =
/// 0xC0F650), bundle = the first entry after `i` (cyclic) whose flag byte
/// (+0x18) differs from entry `i`'s, else entry `(i + 1) % n`. ADAPTED: the
/// list is the grasper blocks in block order (`graspers`: block, x) and the
/// flag is the side of the body (the sign of x), so a two-armed species holds
/// the tool in one hand and the platter in the other, and which hand is the
/// favoured one differs from member to member as in retail (the port used the
/// rightmost hand for every member).
pub fn grasper_pair(graspers: &[(usize, f32)], seed: u32) -> (Option<usize>, Option<usize>) {
    let n = graspers.len();
    if n == 0 {
        return (None, None);
    }
    let i = seed as usize % n;
    let side = |k: usize| graspers[k].1 >= 0.0;
    let bundle = (1..n).map(|d| (i + d) % n).find(|&k| side(k) != side(i)).unwrap_or((i + 1) % n);
    (Some(graspers[i].0), Some(graspers[bundle].0))
}

impl SharedSkin {
    fn bound(&mut self, originals: &Originals, id: u32) -> Option<Arc<Bound>> {
        if let Some(b) = self.bound.get(&id) {
            return Some(b.clone());
        }
        let c = clips::clip(originals, id)?;
        let b = Arc::new(self.rig.bind(c));
        self.bound.insert(id, b.clone());
        Some(b)
    }
}

/// What a tribe unit plays (clip group, looped): the main clip of
/// [`tribal_plan`] for a fresh actor.
#[cfg(test)]
pub fn tribal_request(task: &UnitTask, unit: &TribeUnit, tool: Option<ToolKind>, moving: bool) -> (u32, bool) {
    let plan = tribal_plan(task, unit, tool, moving, &PlanCtx::default());
    (plan.main, plan.looped)
}

/// What a swimming member plays (tribal-swim): the swim stroke while it moves,
/// treading water at rest (looped).
pub fn swim_request(moving: bool) -> (u32, bool) {
    (if moving { clip_id::SWIM } else { clip_id::SWIM_IN_PLACE }, true)
}

// ======================================================= the retail clip table
//
// tribal-final (notes T9K): the citizen actions' PlayAnimation sites were
// found by scanning the code image for every `_final/tribe/` group id and
// decompiling the behaviours around them. Most clips are constants or
// `base + offset` in the action code; the idles, the walks toward a target
// and the attacks come from data (the tool PROPs and the tool abilities).

/// Tribe TLSA groups played by the citizen action code (address of the
/// VERIFIED site in each comment).
#[allow(dead_code)]
pub mod tribe_clip {
    // Walking toward a target (0xDB7120, the WalkTo update of a citizen).
    /// `com_battle_walk_1`: an attack walk without an attack tool.
    pub const BATTLE_WALK: u32 = 0x0591_B056;
    /// `chief_attack_walk`: the chieftain's attack walk.
    pub const CHIEF_ATTACK_WALK: u32 = 0x062F_F867;
    /// `soc_walk`: a social walk without a social tool.
    pub const SOC_WALK: u32 = 0x062E_ED44;
    /// `raid_sneak` (RaidFood, action 8: 0xDBC903).
    pub const RAID_SNEAK: u32 = 0x05DC_C50C;
    // Idles (0xDA7C10: random of three, 10 / 15 s; 0xDAAF40: lookaround).
    /// `soc_idle_neutral2`, `soc_idle_happy1`, `idle_happy`.
    pub const IDLES: [u32; 3] = [0x02C3_9001, 0x02C3_9002, 0x02C3_9003];
    /// `idle_lookaround` (the creature group; `defaulttool`'s lookaround).
    pub const IDLE_LOOKAROUND: u32 = 0x0278_44AD;
    /// `Idle_pickup_look`: no handheld item but a bundle (0xDAAF40).
    pub const IDLE_BUNDLE: u32 = 0x05F0_F5C8;
    // Fishing (0xD73FD0): `base + has the fishing tool + 0x20 for the
    // chieftain` (0xC23840(8), 0xC23430).
    /// `fish_start` / +1 `fish_idle_start` / +0x21 `chief_fish_idle_start`.
    pub const FISH_START: u32 = 0x02C3_9210;
    /// `fish_loop` / `fish_idle` / `chief_fish_idle`.
    pub const FISH_LOOP: u32 = 0x02C3_9215;
    /// `fish_pull` / `fish_spear` / `chief_fish_spear`.
    pub const FISH_PULL: u32 = 0x02C3_921A;
    /// `fish_succ_1..3` (+ random(3); chieftain +0x20).
    pub const FISH_SUCC: u32 = 0x02C3_9220;
    /// `fish_fail` (chieftain +0x20).
    pub const FISH_FAIL: u32 = 0x02C3_9223;
    /// `fish_dump` (chieftain +0x20): the catch goes on the bundle.
    pub const FISH_DUMP: u32 = 0x02C3_9224;
    /// The chieftain's offset in the fishing groups.
    pub const CHIEF_FISH: u32 = 0x20;
    // Eating (0xD73FD0 states 0x15..0x19, 0xD75F70 0x10..0x18).
    /// `mot_pickupfood_toeat`, then `mot_eathand` (looped).
    pub const PICKUP_TO_EAT: u32 = 0x0279_6B31;
    // Combat (0xC1E5C0 attack choice, 0xD69740 attack behaviour, 0xDBAAF0
    // RaidHuts).
    /// `attack_idle_gen02`: between attacks (0xD69740).
    pub const ATTACK_IDLE: u32 = 0x0673_78DD;
    /// `spear_throw`: the spear (ability type 43) beyond its rushing range.
    pub const SPEAR_THROW: u32 = 0x02C3_9370;
    /// `spear_get`: a new spear after a throw (0xD69740).
    pub const SPEAR_GET: u32 = 0x056D_43DE;
    /// `com_battle_yell_1`: the raiders' yell before the charge (0xDBAAF0).
    pub const BATTLE_YELL: u32 = 0x0567_DB4B;
    // The social minigame (0xDA3700..0xDA38D0) and the solos (0xDA0100).
    /// `chief_soc_conductor_01`: the chieftain conducts.
    pub const CHIEF_CONDUCTOR: u32 = 0xB51E_24D4;
    /// `soc_notool_dance_01`: a member without an instrument dances.
    pub const NOTOOL_DANCE: u32 = 0x946E_D014;
    /// Start, base loop and solo per instrument: horn (`soc_flute_start`,
    /// `soc_horn01_loop_a`, `soc_horn01_loop_solo`), maracas
    /// (`soc_shakern01_start`, `soc_shakern01_loop_a`, `..._loop_solo`),
    /// didgeridoo (`soc_didger_start`, `soc_didger01_loop_a`, `..._loop_solo`).
    pub const INSTRUMENT: [(u32, u32, u32); 3] = [
        (0x1472_CE9A, 0xB508_F7E9, 0x5508_F836),
        (0x5474_36F1, 0xF508_F8EE, 0x7508_F905),
        (0x1474_36A2, 0x5508_F8CD, 0xB508_F8DA),
    ];

    /// Every group above (the installed-data test binds them).
    pub const ALL: [u32; 30] = [
        BATTLE_WALK, CHIEF_ATTACK_WALK, SOC_WALK, RAID_SNEAK, IDLES[0], IDLES[1], IDLES[2], IDLE_LOOKAROUND,
        IDLE_BUNDLE, FISH_START, FISH_START + 1, FISH_LOOP, FISH_LOOP + 1, FISH_PULL, FISH_PULL + 1, FISH_SUCC,
        FISH_DUMP, PICKUP_TO_EAT, ATTACK_IDLE, SPEAR_THROW, SPEAR_GET, BATTLE_YELL, CHIEF_CONDUCTOR, NOTOOL_DANCE,
        INSTRUMENT[0].0, INSTRUMENT[0].1, INSTRUMENT[1].0, INSTRUMENT[1].1, INSTRUMENT[2].0, INSTRUMENT[2].1,
    ];
}

/// The animations a tool's PROP names (ORIGINAL data, `tribe_tools~`
/// 0x04292F52: `toolData_DefaultToolIdle`, `toolData_LookaroundToolIdle`,
/// `toolData_EnRouteAnim`; cTribeToolData +0x28 / +0x2C / +0x34). The idle
/// behaviours 0xDA7C10 / 0xDAAF40 read the first two, the WalkTo update
/// 0xDB7120 the third.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolAnims {
    pub idle: u32,
    pub lookaround: u32,
    pub en_route: u32,
}

/// [`ToolAnims`] of a tool type (0 = `defaulttool`, 11 = the chieftain's).
pub fn tool_anims(tool_type: u8) -> ToolAnims {
    let (idle, lookaround, en_route) = match tool_type {
        1 => (0x03CD_ACD0, 0x03CD_ACC4, 0x062E_E751), // torch_idle_01, idle_lookaround_torch, torch_attack_walk
        2 => (0x05E7_DF20, 0x03CD_ACC4, 0x062E_E781), // axe_idle_01, -, axe_attack_walk
        3 => (0x03CD_ACD2, 0x03CD_ACC4, 0x062E_EA6C), // soc_idle_spear01, -, spear_attack_walk
        4 => (0x05E7_DD21, 0x03CD_ACC4, 0x062E_ED7D), // flute_idle_01, -, flute_walk_to_soc
        5 => (0x05E7_DD22, 0x03CD_ACC4, 0x062E_EF25), // shaker_idle_01, -, shakern_walk_to_soc
        6 => (0x05E7_DD20, 0x03CD_ACC4, 0x062E_EF46), // didger_idle_01, -, didger_walk_to_soc
        7 => (0x05F0_F079, 0x05F0_F07A, 0),           // treesmaker_idle_01, idle_lookaround_treesmacker
        8 => (0x05F1_0A22, 0x03CD_ACC5, 0),           // fish_spear_idle_01, idle_lookaround_fish
        9 => (0x05F0_F078, 0x03CD_ACC4, 0),           // shaman_idle_01
        11 => (0x04C6_CB02, 0x03CD_ACC4, 0),          // idle_chief_wave01
        _ => (0, tribe_clip::IDLE_LOOKAROUND, 0),     // defaulttool (and the camp fire)
    };
    ToolAnims { idle, lookaround, en_route }
}

/// The citizen's tool type (cCreatureCitizen +0x102C as 0xC22F20 reads it):
/// the tool it holds; the chieftain without one has his staff (type 11).
pub fn citizen_tool_type(tool: Option<ToolKind>, chieftain: bool) -> u8 {
    match tool {
        Some(kind) => crate::tribal_rules::tool_type(kind),
        None if chieftain => 11,
        None => 0,
    }
}

/// The attack animations of a tool's ability (ORIGINAL data,
/// `tribeabilities` 0xC85C1F8B `creatureAbilityAnimationIDs`; 0xC1E5C0 plays a
/// random one): torch `torch_poke`, axe `axe_attack`, spear `spear_stab`,
/// shaman `shaman_attack_01`, chieftain `chief_attack_01` / `_02`. ADAPTED: a
/// member without an attack tool swings `com_battle_club_1` (retail members
/// fight with a tool only, see `tribal_rules::UNARMED`).
pub fn attack_clips(tool_type: u8) -> &'static [u32] {
    match tool_type {
        1 => &[clip_id::TORCH],
        2 => &[clip_id::AXE],
        3 => &[clip_id::SPEAR],
        9 => &[clip_id::SHAMAN_ATTACK],
        11 => &[clip_id::CHIEF_ATTACK, 0x0576_6A11],
        _ => &[clip_id::NO_TOOL_ATTACK],
    }
}

/// The ability's `creatureAbilityBuildingSpecificAnimationID` (ORIGINAL data),
/// played instead of the attack when the target is a hut or a tool (0xC1E5C0:
/// target noun 0x18C8F0C / 0x1E4DAAE): `torch_hut`, `axe_attack_hut01`,
/// `spear_attack_hut01`, `attack_shaman_hut01`, `chief_attack_hut01`.
pub fn building_clips(tool_type: u8) -> Option<&'static [u32]> {
    Some(match tool_type {
        1 => &[0x05E7_ED36],
        2 => &[0x0601_FB4F],
        3 => &[0x0601_FC3D],
        9 => &[0x0601_FDF4],
        11 => &[0x0601_FCF7],
        _ => return None,
    })
}

/// Repeated attacks: one of `clips` (then `follow`) every `refresh` seconds,
/// the plan's main clip in between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackCycle {
    pub refresh: f32,
    pub clips: &'static [u32],
    pub follow: Option<u32>,
}

/// What a tribe unit plays: `main` (looped unless said), an `intro` played
/// once when the plan starts, and an attack cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipPlan {
    pub main: u32,
    pub looped: bool,
    pub intro: Option<u32>,
    pub attack: Option<AttackCycle>,
    /// A clip laid over the main one on the joints it animates
    /// (`platter_grasper_overlay` for a bundle carrier).
    pub overlay: Option<u32>,
}

impl ClipPlan {
    fn looped(main: u32) -> Self {
        Self { main, looped: true, intro: None, attack: None, overlay: None }
    }
    fn with_intro(main: u32, intro: u32) -> Self {
        Self { intro: Some(intro), ..Self::looped(main) }
    }
}

/// Where the social game stands for a troupe member (the performance states of
/// 0xDA2F30 and the members' stages of 0xDA3656..0xDA3A44).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SongPhase {
    /// No song is running (the plain performing stance).
    #[default]
    None,
    /// The troupe walks to its spots and waits for the others (states 0..4):
    /// an instrument player plays its start clip, then everyone its waiting idle.
    Gathering,
    /// The game runs (state 5).
    Playing,
    /// The song ended with the bar at `bar` (0..10): the instrument players cheer
    /// and everyone plays the ending clip (`tribal_rules::social_ending_clips`).
    Ending { bar: f32 },
}

/// Per-actor context of the choice: a seed and a clock for the idle
/// variation, whether the actor (a shaman) has an injured member in reach, and
/// where the social game stands.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlanCtx {
    pub seed: u32,
    pub clock: f32,
    pub healing: bool,
    pub song: SongPhase,
}

/// Seconds one idle choice lasts (ADAPTED: retail's idle behaviours run
/// 10 or 15 s each, 0xDA7C10; the behaviour tree's order is not decoded).
pub const IDLE_SLOT: f32 = 12.0;

fn mix(a: u32, b: u32) -> u32 {
    let h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D) ^ (h >> 13)
}

/// The idle of a standing citizen (0xDA7C10 / 0xDAAF40): with a tool whose
/// PROP names idles, its idle or (half the time) its lookaround; without one
/// a random neutral idle or `idle_lookaround`; empty-handed with a bundle
/// `Idle_pickup_look`.
pub fn idle_clip(tool_type: u8, carrying: bool, ctx: &PlanCtx) -> u32 {
    let slot = ((ctx.clock + (ctx.seed % 997) as f32 * 0.37) / IDLE_SLOT).floor().max(0.0) as u32;
    let roll = mix(ctx.seed, slot);
    let anims = tool_anims(tool_type);
    if anims.idle != 0 {
        if anims.lookaround != 0 && roll % 2 == 1 {
            anims.lookaround
        } else {
            anims.idle
        }
    } else if carrying {
        tribe_clip::IDLE_BUNDLE
    } else if roll % 4 == 3 {
        tribe_clip::IDLE_LOOKAROUND
    } else {
        tribe_clip::IDLES[(roll % 4) as usize % 3]
    }
}

/// The fishing groups' offset (0xD73FD0): +1 with the fishing tool (the
/// chieftain's staff counts, T9J), +0x20 for the chieftain.
pub fn fish_offset(tool: Option<ToolKind>, chieftain: bool) -> u32 {
    let tool = (tool == Some(ToolKind::FishingSpears) || chieftain) as u32;
    tool + if chieftain { tribe_clip::CHIEF_FISH } else { 0 }
}

/// What a tribe unit plays (the retail citizen action table, T9K).
pub fn tribal_plan(task: &UnitTask, unit: &TribeUnit, tool: Option<ToolKind>, moving: bool, ctx: &PlanCtx) -> ClipPlan {
    use clip_id::*;
    let kind = citizen_tool_type(tool, unit.chieftain);
    let class = tool.map(|t| crate::tribal_tools::info(t).class);
    let carrying = unit.carrying > 0.0 || matches!(task, UnitTask::Deliver(_));
    if moving {
        let en_route = tool_anims(kind).en_route;
        let id = match task {
            // 0xDB7120: attack walk (chieftain / the attack tool's walk).
            UnitTask::Hunt(_) | UnitTask::Siege(_) if unit.chieftain => tribe_clip::CHIEF_ATTACK_WALK,
            UnitTask::Hunt(_) | UnitTask::Siege(_) if class == Some(crate::tribal_tools::ToolClass::Attack) && en_route != 0 => en_route,
            UnitTask::Hunt(_) | UnitTask::Siege(_) => tribe_clip::BATTLE_WALK,
            // 0xDB7120: social walk (the social tool's walk).
            UnitTask::Perform(_) if class == Some(crate::tribal_tools::ToolClass::Social) && en_route != 0 => en_route,
            UnitTask::Perform(_) => tribe_clip::SOC_WALK,
            UnitTask::Raid(_) if !carrying => tribe_clip::RAID_SNEAK,
            // VERIFIED (T9K-1): a carrier walks the default locomotion and lays
            // `platter_grasper_overlay` (0x05F08F08, 0xC23460) on the bundle
            // grasper; `walk_carry` is never played.
            _ => WALK,
        };
        return ClipPlan { overlay: carrying.then_some(PLATTER_OVERLAY), ..ClipPlan::looped(id) };
    }
    if !unit.busy {
        return ClipPlan::looped(idle_clip(kind, unit.carrying > 0.0, ctx));
    }
    let refresh = if unit.chieftain {
        crate::tribal_rules::CHIEFTAIN_STAFF.refresh
    } else {
        crate::tribal_rules::weapon_of(tool).refresh
    };
    match task {
        UnitTask::Gather(_) if unit.carry_kind == crate::tribal_rts::FoodKind::Meat => ClipPlan::looped(MEAT),
        // 0xD75F70 state 2: the smack clip (the chieftain's variant) only with
        // the gather tool, else the pick by hand.
        UnitTask::Gather(_) if unit.chieftain && tool == Some(ToolKind::FruitSticks) => {
            ClipPlan::looped(CHIEF_GATHER)
        }
        UnitTask::Gather(_) if tool == Some(ToolKind::FruitSticks) => ClipPlan::looped(GATHER_STICK),
        UnitTask::Gather(_) => ClipPlan::looped(GATHER_HANDS),
        UnitTask::Fish(_) => {
            let o = fish_offset(tool, unit.chieftain);
            ClipPlan::with_intro(tribe_clip::FISH_LOOP + o, tribe_clip::FISH_START + o)
        }
        UnitTask::Hunt(_) => ClipPlan {
            attack: Some(AttackCycle {
                refresh,
                // The spear is thrown from its range (0xC1E5C0: beyond the
                // rushing range), then a new one is taken (0xD69740).
                clips: if kind == 3 { &[tribe_clip::SPEAR_THROW] } else { attack_clips(kind) },
                follow: (kind == 3).then_some(tribe_clip::SPEAR_GET),
            }),
            ..ClipPlan::looped(tribe_clip::ATTACK_IDLE)
        },
        UnitTask::Siege(_) => {
            // The hut is a building target: the ability's building clip.
            let clips = building_clips(kind).unwrap_or_else(|| attack_clips(kind));
            ClipPlan {
                intro: Some(tribe_clip::BATTLE_YELL),
                attack: Some(AttackCycle { refresh, clips, follow: None }),
                ..ClipPlan::looped(tribe_clip::ATTACK_IDLE)
            }
        }
        // Member stages 3..4 (0xDA370A..0xDA3826): at its spot an instrument player
        // plays its start clip, then everyone waits in its tool's waiting idle until
        // the others are in place. The ending (stages 7..9) is queued by the
        // sequencer ([`Sequencer::ending`]); the members stay in the waiting idle
        // around it.
        UnitTask::Perform(_) if matches!(ctx.song, SongPhase::Gathering | SongPhase::Ending { .. }) => {
            let wait = crate::tribal_rules::SOCIAL_WAIT_CHIEF;
            let (wait, start) = match (unit.chieftain, instrument_index(tool)) {
                (true, _) => (wait, None),
                (false, Some(i)) => (crate::tribal_rules::SOCIAL_WAIT[i], Some(crate::tribal_rules::SOCIAL_START[i])),
                (false, None) => (crate::tribal_rules::SOCIAL_WAIT[3], None),
            };
            match (start, ctx.song) {
                (Some(start), SongPhase::Gathering) => ClipPlan::with_intro(wait, start),
                _ => ClipPlan::looped(wait),
            }
        }
        UnitTask::Perform(_) if unit.chieftain => ClipPlan::looped(tribe_clip::CHIEF_CONDUCTOR),
        // Playing (member stage 5, 0xDA3840): the base loop of the tool (the start
        // clip already played at the spot); without a song context the start clip
        // comes first.
        UnitTask::Perform(_) => match instrument_index(tool) {
            Some(i) if ctx.song == SongPhase::Playing => ClipPlan::looped(tribe_clip::INSTRUMENT[i].1),
            Some(i) => ClipPlan::with_intro(tribe_clip::INSTRUMENT[i].1, tribe_clip::INSTRUMENT[i].0),
            None => ClipPlan::looped(tribe_clip::NOTOOL_DANCE),
        },
        UnitTask::Recover => ClipPlan::with_intro(EAT, tribe_clip::PICKUP_TO_EAT),
        UnitTask::Domesticate(_) => ClipPlan::looped(TAME),
        UnitTask::FetchTool(..) => ClipPlan::looped(TOOL_RACK),
        // `shaman_heal_01` toward the patient (0xDB9AC0) while one is in reach.
        _ if tool == Some(ToolKind::HealingRods) && ctx.healing => ClipPlan::looped(HEAL),
        _ => ClipPlan::looped(idle_clip(kind, unit.carrying > 0.0, ctx)),
    }
}

/// Index of a social tool in [`tribe_clip::INSTRUMENT`].
pub fn instrument_index(tool: Option<ToolKind>) -> Option<usize> {
    match tool {
        Some(ToolKind::WoodenHorns) => Some(0),
        Some(ToolKind::Maracas) => Some(1),
        Some(ToolKind::Aerophones) => Some(2),
        _ => None,
    }
}

/// The performance note an instrument answers (`tribal::instrument_bit`: 3
/// horn, 4 maracas, 5 didgeridoo).
fn instrument_note(i: usize) -> u32 {
    3 + i as u32
}

/// An actor's clip sequence: the plan's intro and the queued one-shots
/// (attacks, catches, solos) before its main clip.
#[derive(Debug, Default)]
pub struct Sequencer {
    main: u32,
    intro: Option<u32>,
    queue: std::collections::VecDeque<u32>,
    /// The one-shot now on top of the clip player.
    current: Option<u32>,
    attack_timer: f32,
    last_carry: f32,
    pulled: bool,
    /// The catch clip of this cycle was queued.
    landed: bool,
    /// `unit.work` of the last frame (a smaller value starts a new cycle).
    last_work: f32,
    /// The song's ending clips were queued (0xDA3991 / 0xDA3A03, once per song).
    ended: bool,
    rng: u32,
}

impl Sequencer {
    fn roll(&mut self, n: u32) -> u32 {
        self.rng = self.rng.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F);
        (self.rng >> 8) % n.max(1)
    }

    /// Queue a one-shot.
    pub fn push(&mut self, id: u32) {
        self.queue.push_back(id);
    }

    /// Take `plan`; returns the clip to show, whether it loops and whether it
    /// must restart (a new one-shot). `finished` = the one-shot on top ended.
    pub fn step(&mut self, plan: &ClipPlan, finished: bool, dt: f32) -> (u32, bool, bool) {
        if plan.main != self.main || plan.intro != self.intro {
            self.main = plan.main;
            self.intro = plan.intro;
            self.queue.clear();
            self.current = None;
            self.queue.extend(plan.intro);
            self.attack_timer = 0.0;
        }
        if finished && self.current.is_some() && self.queue.front().copied() == self.current {
            self.queue.pop_front();
            self.current = None;
        }
        if let Some(cycle) = plan.attack {
            self.attack_timer -= dt;
            if self.attack_timer <= 0.0 && self.queue.is_empty() {
                let pick = cycle.clips[self.roll(cycle.clips.len() as u32) as usize];
                self.queue.push_back(pick);
                self.queue.extend(cycle.follow);
                self.attack_timer = cycle.refresh;
            }
        }
        if let Some(&front) = self.queue.front() {
            let fresh = self.current != Some(front);
            self.current = Some(front);
            return (front, false, fresh);
        }
        (plan.main, plan.looped, false)
    }

    /// The song ended (member stages 7..8, 0xDA3944..0xDA3A44): an instrument
    /// player cheers (`soc_*_end`), then everyone plays the ending clip
    /// (`soc_sign_chief_01` / `soc_sign_react_01` when the bar passed 5,
    /// `soc_fail_01` otherwise). Queued once per song; `phase` is the song's phase
    /// this frame.
    pub fn ending(&mut self, phase: SongPhase, group: Option<usize>, chieftain: bool) {
        match phase {
            SongPhase::Ending { bar } if !self.ended => {
                self.ended = true;
                let (cheer, ending) = crate::tribal_rules::social_ending_clips(group, chieftain, bar);
                self.queue.extend(cheer.map(|c| c.0));
                self.queue.push_back(ending.0);
            }
            SongPhase::Ending { .. } => {}
            _ => self.ended = false,
        }
    }

    /// Drop queued clips the rig cannot play.
    fn skip_unplayable(&mut self, mut playable: impl FnMut(u32) -> bool) {
        while let Some(&front) = self.queue.front() {
            if playable(front) {
                break;
            }
            self.queue.pop_front();
            if self.current == Some(front) {
                self.current = None;
            }
        }
    }

    /// Fishing events (0xD73FD0), on the sim's cycle clock (`unit.work`, the
    /// rolled `unit.fish`): the pull when the loop ends, then the rolled
    /// `fish_succ_N`, the dump onto the bundle and a new start.
    fn fishing(&mut self, unit: &TribeUnit, offset: u32) {
        // The catch, the fail and the dump have no tool variant.
        let chief = offset & !1;
        if unit.work + 1e-4 < self.last_work {
            // The cycle wrapped: a new catch begins with its start clip.
            self.pulled = false;
            self.landed = false;
        }
        self.last_work = unit.work;
        let cycle = unit.fish;
        if !self.landed && unit.work >= cycle.catch_at() {
            self.queue.push_back(tribe_clip::FISH_SUCC + cycle.succ as u32 + chief);
            self.queue.push_back(tribe_clip::FISH_DUMP + chief);
            self.queue.push_back(tribe_clip::FISH_START + offset);
            self.landed = true;
            self.pulled = true;
        } else if !self.pulled && unit.busy && unit.work >= cycle.pull_at() && self.queue.is_empty() {
            self.queue.push_back(tribe_clip::FISH_PULL + offset);
            self.pulled = true;
        }
    }
}

/// Shamans with an injured tribe member within healing reach (they play
/// `shaman_heal_01`; the healing itself is `tribal_rts`' aura).
#[derive(Resource, Default)]
pub struct CrowdHealers(pub std::collections::HashSet<Entity>);

/// Fill [`CrowdHealers`].
fn mark_healers(
    mut healers: ResMut<CrowdHealers>,
    units: Query<
        (Entity, &Transform, Option<&crate::tribal_tools::HeldToolState>, &crate::game::Health),
        (With<TribeUnit>, Without<crate::tribal_rts::RivalMember>),
    >,
) {
    healers.0.clear();
    let injured: Vec<Vec3> =
        units.iter().filter(|u| u.3.hp < u.3.max && !u.3.dead()).map(|u| u.1.translation).collect();
    if injured.is_empty() {
        return;
    }
    for (e, t, held, _) in &units {
        if held.and_then(|h| h.shown.flatten()) != Some(ToolKind::HealingRods) {
            continue;
        }
        let reach = injured.iter().any(|p| {
            let d = Vec2::new(p.x - t.translation.x, p.z - t.translation.z).length();
            d > 1e-3 && crate::tribal_rules::in_healing_reach(d)
        });
        if reach {
            healers.0.insert(e);
        }
    }
}

pub struct OutfitCrowdPlugin;

impl Plugin for OutfitCrowdPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CrowdSkin>()
            .init_resource::<CrowdHealers>()
            .add_systems(
                Update,
                (spawn_citizens, crowd_demo, walk_citizens, mark_healers, drive_crowd, captain_layers)
                    .chain()
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(resource_exists::<Originals>)
                    .run_if(resource_exists::<Assets<SkinnedMeshInverseBindposes>>)
                    .run_if(resource_exists::<Assets<Mesh>>),
            );
    }
}

/// Citizens strolling around the capital (ADAPTED count).
pub const CITIZENS: usize = 6;
/// Their size on the city's scale (ADAPTED: smaller than the plaza statue,
/// 0.55).
pub const CITIZEN_SCALE: f32 = 0.38;
/// How far from the plaza they stroll (city units, ADAPTED).
const CITIZEN_RANGE: f32 = 9.0;
/// Walking speed (city units per second, ADAPTED).
const CITIZEN_SPEED: f32 = 1.6;

/// Citizens of the capital: the species in its Civ outfit.
#[allow(clippy::too_many_arguments)]
fn spawn_citizens(
    mut commands: Commands,
    state: Option<Res<State<GameStage>>>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(), With<CivCitizen>>,
) {
    if state.is_none_or(|s| *s.get() != GameStage::Civilization) || !existing.is_empty() {
        return;
    }
    if !enabled() || run.civ_campaign.cities.is_empty() || originals.models.is_empty() {
        return;
    }
    let Some(body) = originals.baked_body.as_ref() else { return };
    if body.skin.is_none() {
        return;
    }
    let (mesh, material) = (body.model.draw_mesh(), body.model.material.clone());
    let Some(material) = material else { return };
    let shape = run.civ_campaign.shape();
    let city_scale = crate::civ_planet::CITY_SCALE;
    let capital = run.civ_campaign.cities[crate::civ::HOME_INDEX].center();
    for i in 0..CITIZENS {
        let seed = 0x5EED_C171 ^ (i as u32).wrapping_mul(0x9E37_79B9);
        let local = scatter(seed, CITIZEN_RANGE) * city_scale;
        let at = shape.local_point(capital, local);
        let clearance = 1.3 * CITIZEN_SCALE * city_scale;
        let e = commands
            .spawn((
                StageOwned,
                CivCitizen { home: capital, target: at, wait: (i as f32) * 0.7, seed },
                Visibility::Inherited,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(shape.place_on(at, clearance))
                    .with_rotation(shape.frame(at))
                    .with_scale(Vec3::splat(CITIZEN_SCALE * city_scale)),
                crate::terrain::Radial,
                crate::terrain::Grounded::new(clearance),
                Name::new(format!("citizen {i}")),
            ))
            .id();
        crate::creature::mount_saved_parts(&mut commands, &mut materials, e, &originals, &run, i + 1, CITIZEN_SCALE);
    }
    info!("crowd: {CITIZENS} citizens in the capital");
}

/// A point within `range` of the origin on the local ground plane.
fn scatter(seed: u32, range: f32) -> Vec3 {
    let h = |s: u32| (s.wrapping_mul(0x2C1B_3C6D) ^ (s >> 15)).wrapping_mul(0x297A_2D39);
    let a = (h(seed) % 10_000) as f32 / 10_000.0 * std::f32::consts::TAU;
    let r = (0.35 + 0.65 * (h(seed ^ 0xA5A5) % 10_000) as f32 / 10_000.0) * range;
    Vec3::new(a.cos() * r, 0.0, a.sin() * r)
}

/// Citizens walk to a point, wait, and pick the next one.
fn walk_citizens(
    time: Res<Time>,
    run: Res<RunProgress>,
    state: Option<Res<State<GameStage>>>,
    mut citizens: Query<(&mut Transform, &mut CivCitizen)>,
) {
    if state.is_none_or(|s| *s.get() != GameStage::Civilization) {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let shape = run.civ_campaign.shape();
    let city_scale = crate::civ_planet::CITY_SCALE;
    for (mut t, mut c) in &mut citizens {
        let up = (shape.frame(t.translation) * Vec3::Y).normalize_or(Vec3::Y);
        let to = c.target - t.translation;
        let flat = to - up * to.dot(up);
        if c.wait > 0.0 {
            c.wait -= dt;
            continue;
        }
        let step = CITIZEN_SPEED * city_scale * dt;
        if flat.length() <= step {
            c.seed = c.seed.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F);
            c.target = shape.local_point(c.home, scatter(c.seed, CITIZEN_RANGE) * city_scale);
            c.wait = 1.5 + (c.seed % 300) as f32 / 100.0;
            continue;
        }
        let dir = flat.normalize();
        t.translation += dir * step;
        // Face the way it walks (creature front: local -Z).
        let frame = shape.frame(t.translation);
        let local = frame.inverse() * dir;
        let yaw = Quat::from_rotation_y((-local.x).atan2(-local.z));
        t.rotation = frame * yaw;
    }
}

/// Capture option `--crowd-demo`: the tribe's members go gathering (they
/// walk, strip a bush and carry the bundle home).
fn crowd_demo(
    state: Option<Res<State<GameStage>>>,
    mut frames: Local<u32>,
    mut units: Query<(&TribeUnit, &Transform, &mut UnitTask), Without<crate::tribal_rts::RivalMember>>,
    bushes: Query<(Entity, &Transform), With<crate::game::Bush>>,
) {
    if state.is_none_or(|s| *s.get() != GameStage::Tribal) || !std::env::args().any(|a| a == "--crowd-demo") {
        return;
    }
    *frames += 1;
    if *frames != 30 {
        return;
    }
    for (unit, t, mut task) in &mut units {
        if unit.chieftain {
            continue;
        }
        if let Some((bush, _)) = bushes
            .iter()
            .min_by(|a, b| a.1.translation.distance_squared(t.translation).total_cmp(&b.1.translation.distance_squared(t.translation)))
        {
            *task = UnitTask::Gather(bush);
        }
    }
}

/// Set up, pose and tear down the crowd.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn drive_crowd(
    mut commands: Commands,
    time: Res<Time>,
    state: Option<Res<State<GameStage>>>,
    editor: Option<Res<crate::editor::EditorState>>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    cinematic: Option<Res<crate::cinematics::CinematicScene>>,
    mut crowd: ResMut<CrowdSkin>,
    (mut meshes, mut bindposes): (ResMut<Assets<Mesh>>, ResMut<Assets<SkinnedMeshInverseBindposes>>),
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut actors: Query<
        (
            Entity,
            &Transform,
            &mut Mesh3d,
            Option<&mut CrowdActor>,
            Option<&TribeUnit>,
            Option<&UnitTask>,
            Option<&crate::tribal_tools::HeldToolState>,
            Option<&Visibility>,
            Has<SpaceCaptain>,
            Option<&crate::tribal_swim::Swim>,
        ),
        (
            Or<(With<TribeUnit>, With<CivCitizen>, With<SpaceCaptain>, With<crate::tribal_vignettes::VignetteFiller>)>,
            Without<crate::tribal_rts::RivalMember>,
            Without<CrowdJoint>,
        ),
    >,
    children: Query<&Children>,
    mounted: Query<Option<&MountIndex>, Or<(With<crate::creature::MountedPart>, With<crate::outfitter::OutfitMounted>)>>,
    mut transforms: Query<
        &mut Transform,
        (
            Without<CrowdActor>,
            Without<TribeUnit>,
            Without<CivCitizen>,
            Without<SpaceCaptain>,
            Without<crate::tribal_vignettes::VignetteFiller>,
        ),
    >,
    (swarm, mut plays, mut stops, mut held_models, healers, vignettes): (
        Option<Res<crate::swarm_fx::SwarmLibrary>>,
        MessageWriter<crate::swarm_fx::SwarmPlay>,
        MessageWriter<crate::swarm_fx::SwarmStop>,
        Query<
            (&ChildOf, &mut Visibility),
            (
                With<crate::tribal_tools::HeldTool>,
                Without<TribeUnit>,
                Without<CivCitizen>,
                Without<SpaceCaptain>,
                Without<crate::tribal_vignettes::VignetteFiller>,
            ),
        >,
        Res<CrowdHealers>,
        Option<Res<crate::tribal_vignettes::TribalVignettes>>,
    ),
) {
    let stage = state.map(|s| s.get().clone());
    let on = enabled()
        && matches!(stage, Some(GameStage::Tribal) | Some(GameStage::Civilization) | Some(GameStage::Space))
        && !editor.as_ref().is_some_and(|e| e.open);
    if !on {
        return;
    }
    // The species' shared skin, rebuilt when the creature changes.
    // (Retried every half second while the body template is not ready.)
    crowd.frame = crowd.frame.wrapping_add(1);
    let frame = crowd.frame;
    let key = (run.body.clone(), run.parts.clone());
    if crowd.key.as_ref() != Some(&key) || (crowd.shared.is_none() && frame % 30 == 0) {
        crowd.key = Some(key);
        crowd.generation += 1;
        crowd.shared = build_shared(&originals, &run, &mut meshes, &mut bindposes);
    }
    let clock = time.elapsed_secs();
    // An instrument ordered since last frame (0xDA0100: the players of that
    // instrument play their solo).
    let solo_note = match run.tribal_campaign.performance.as_ref() {
        Some(p) => {
            let (orders, _) = crowd.song;
            crowd.song = (p.orders, p.last_order);
            (p.orders > orders).then_some(p.last_order)
        }
        None => {
            crowd.song = (0, 0);
            None
        }
    };
    // Where the social game stands (the performance object's state, 0xDA2F30).
    let song_phase = match (run.tribal_campaign.performance.as_ref(), run.tribal_campaign.social_end.as_ref()) {
        (Some(p), _) if p.gathering => SongPhase::Gathering,
        (Some(_), _) => SongPhase::Playing,
        (None, Some(end)) => SongPhase::Ending { bar: end.bar },
        _ => SongPhase::None,
    };
    let generation = crowd.generation;
    let Some(shared) = crowd.shared.as_mut() else { return };
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let eye = camera.iter().next().map(|t| (t.translation(), t.forward().as_vec3()));
    let started = std::time::Instant::now();
    let mut posed = 0usize;
    let mut playing: Vec<u32> = Vec::new();
    for (entity, tf, mut mesh3d, actor, unit, task, held, visibility, captain, swim) in &mut actors {
        if visibility == Some(&Visibility::Hidden) {
            continue;
        }
        // A stale actor (the species changed) is set up again.
        if let Some(a) = actor.as_ref().filter(|a| a.generation != generation) {
            mesh3d.0 = a.original_mesh.clone();
            for &j in &a.joints {
                commands.entity(j).despawn();
            }
            if let Some(hand) = a.hand {
                commands.entity(hand).despawn();
            }
            if let Some(bundle) = a.bundle {
                commands.entity(bundle).despawn();
            }
            for (part, _, rest) in &a.parts {
                if let Ok(mut t) = transforms.get_mut(*part) {
                    *t = *rest;
                }
            }
            commands.entity(entity).remove::<(CrowdActor, SkinnedMesh, DynamicSkinnedMeshBounds)>();
            continue;
        }
        let Some(mut a) = actor else {
            if mesh3d.0 != shared.source {
                continue;
            }
            let joints: Vec<Entity> = (0..shared.joints)
                .map(|_| commands.spawn((CrowdJoint, StageOwned, Transform::IDENTITY, ChildOf(entity))).id())
                .collect();
            let mut parts = Vec::new();
            for child in children.get(entity).map(|c| c.iter().collect::<Vec<_>>()).unwrap_or_default() {
                let Ok(index) = mounted.get(child) else { continue };
                let Ok(rest) = transforms.get(child).map(|t| *t) else { continue };
                let block = index.and_then(|i| shared.part_blocks.get(&i.0).copied()).unwrap_or_else(|| {
                    // ADAPTED: an accessory rides the nearest block.
                    shared
                        .points
                        .iter()
                        .enumerate()
                        .min_by(|x, y| x.1.distance_squared(rest.translation).total_cmp(&y.1.distance_squared(rest.translation)))
                        .map_or(0, |(i, _)| i)
                });
                parts.push((child, block, rest));
            }
            let original_mesh = std::mem::replace(&mut mesh3d.0, shared.mesh.clone());
            // The grasper anchors of a tribe member (handheld items).
            let (hand_block, bundle_block) = if unit.is_some() { grasper_pair(&shared.graspers, entity.to_bits() as u32) } else { (None, None) };
            let hand = hand_block.map(|_| {
                commands.spawn((HandAnchor, StageOwned, Transform::IDENTITY, Visibility::Inherited, ChildOf(entity))).id()
            });
            // The bundle grasper (cCreatureCitizen +0xFC8) holds the carried
            // platter (`tribal_village::carried_platters`).
            let bundle = bundle_block.map(|_| {
                commands.spawn((BundleAnchor, StageOwned, Transform::IDENTITY, Visibility::Inherited, ChildOf(entity))).id()
            });
            commands.entity(entity).insert((
                SkinnedMesh { inverse_bindposes: shared.inverse.clone(), joints: joints.clone() },
                DynamicSkinnedMeshBounds,
                CrowdActor {
                    joints,
                    parts,
                    original_mesh,
                    clips: ClipPlayer::default(),
                    last_pos: tf.translation,
                    speed: 0.0,
                    generation,
                    seed: entity.to_bits() as u32,
                    playing: 0,
                    hand,
                    bundle,
                    hand_block,
                    bundle_block,
                    handheld: None,
                    seq: Sequencer { rng: entity.to_bits() as u32 | 1, ..Default::default() },
                    overlay: None,
                },
            ));
            continue;
        };
        // Ground speed in creature units per second.
        let frame_unit = (tf.scale.x.abs() * shared.unit).max(1e-4);
        let step = (tf.translation - a.last_pos).length();
        a.last_pos = tf.translation;
        let v = if dt > 0.0 && step < 5.0 * frame_unit { step / dt / frame_unit } else { 0.0 };
        a.speed += (v - a.speed) * (1.0 - (-8.0 * dt).exp());
        let moving = a.speed > STAND * shared.rig.leg_length;
        let swimming = swim.is_some_and(|s| s.wet());
        // A clip the cinematic host plays on this member (a vignette role,
        // the alliance celebration) wins; the host's clock owns it.
        let scripted = cinematic
            .as_ref()
            .and_then(|s| s.actor_clip(entity))
            // A game-code vignette (`tribal_vignettes`) the same way.
            .or_else(|| vignettes.as_ref().and_then(|v| v.actor_clip(entity)))
            .filter(|(sid, _)| shared.bound(&originals, *sid).is_some());
        let mut fresh = false;
        let mut overlay_wanted: Option<u32> = None;
        let (id, looped) = match (unit, task) {
            _ if scripted.is_some() => (scripted.map_or(clip_id::IDLE, |s| s.0), false),
            _ if swimming => swim_request(moving),
            (Some(unit), Some(task)) => {
                let tool = held.and_then(|h| h.shown.flatten());
                let ctx = PlanCtx { seed: a.seed, clock, healing: healers.0.contains(&entity), song: song_phase };
                let plan = tribal_plan(task, unit, tool, moving, &ctx);
                overlay_wanted = plan.overlay;
                let finished = a.clips.finished();
                let (id, looped, new) = a.seq.step(&plan, finished, dt);
                fresh = new;
                // The troupe's ending clips (a member that stands at its spot).
                if !moving && matches!(task, UnitTask::Perform(_)) {
                    a.seq.ending(song_phase, instrument_index(tool), unit.chieftain);
                } else if !matches!(song_phase, SongPhase::Ending { .. }) {
                    a.seq.ending(song_phase, None, false);
                }
                // Events: the catch cycle of a fisher, an instrument's solo.
                if !moving && matches!(task, UnitTask::Fish(_)) {
                    let offset = fish_offset(tool, unit.chieftain);
                    a.seq.fishing(unit, offset);
                }
                if let (Some(note), UnitTask::Perform(_), Some(i)) = (solo_note, task, instrument_index(tool)) {
                    if !moving && !unit.chieftain && note == instrument_note(i) {
                        a.seq.push(tribe_clip::INSTRUMENT[i].2);
                    }
                }
                a.seq.last_carry = unit.carrying;
                a.seq.skip_unplayable(|c| shared.bound(&originals, c).is_some());
                (id, looped)
            }
            _ => (if moving { clip_id::WALK } else { clip_id::IDLE }, true),
        };
        playing.push(id);
        if fresh || a.clips.current_id() != Some(id) {
            if let Some(b) = shared.bound(&originals, id).or_else(|| shared.bound(&originals, clip_id::IDLE)) {
                if fresh {
                    a.clips.restart(b, looped);
                } else {
                    a.clips.play(b, looped);
                }
            }
            a.playing = id;
        }
        if let Some(top) = a.clips.current() {
            if top.looped && top.bound.clip.stance_speed.is_some() {
                let rate = clips::gait_rate(&top.bound.clip, a.speed, &shared.rig);
                a.clips.set_rate(rate);
            }
        }
        match scripted {
            Some((_, t)) => {
                a.clips.set_time(t);
                a.clips.advance(0.0);
            }
            None => a.clips.advance(dt),
        }
        // The overlay layer (the carrier's bundle grasper): bound when wanted,
        // faded in and out, dropped once it is gone.
        if let Some(id) = overlay_wanted {
            if a.overlay.as_ref().is_none_or(|o| o.id != id) {
                if let Some(bound) = shared.bound(&originals, id) {
                    a.overlay = Some(OverlayLayer { id, bound, time: 0.0, weight: 0.0 });
                }
            }
        }
        if let Some(o) = a.overlay.as_mut() {
            o.time += dt;
            let goal = if overlay_wanted == Some(o.id) { 1.0 } else { 0.0 };
            let step = dt / OVERLAY_FADE;
            o.weight = if o.weight < goal { (o.weight + step).min(goal) } else { (o.weight - step).max(goal) };
            if goal == 0.0 && o.weight <= 0.0 {
                a.overlay = None;
            }
        }
        // The handheld item on the grasper (T9H): the retail effect when it is
        // installed (the tool's field model is hidden then).
        if let (Some(hand), Some(unit), Some(task)) = (a.hand, unit, task) {
            let tool = held.and_then(|h| h.shown.flatten());
            // The held item stays while swimming: OnStartSwimming 0xC14F70 only
            // restarts the default locomotion (0xC148B0), nothing drops it.
            // A vignette action's item (`vi_effects_n`, 0xC250A0 ids) wins.
            let wanted = match cinematic
                .as_ref()
                .and_then(|s| s.actor_item(entity))
                .or_else(|| vignettes.as_ref().and_then(|v| v.actor_item(entity)))
            {
                Some(item) => item,
                None => tribal_handheld(task, unit, tool),
            }
            .filter(|id| swarm.as_ref().is_some_and(|lib| lib.find(*id).is_some()));
            if wanted != a.handheld {
                stops.write(crate::swarm_fx::SwarmStop {
                    effect: 0,
                    attach_to: Some(hand),
                    tag: handheld::TAG,
                    hard: true,
                });
                if let Some(effect) = wanted {
                    plays.write(crate::swarm_fx::SwarmPlay {
                        attach_to: Some(hand),
                        tag: handheld::TAG,
                        ..crate::swarm_fx::SwarmPlay::at(effect, Vec3::ZERO)
                    });
                }
                a.handheld = wanted;
            }
            let shows_effect = tool_handheld(tool, unit.chieftain)
                .is_some_and(|id| swarm.as_ref().is_some_and(|lib| lib.find(id).is_some()));
            for (parent, mut vis) in &mut held_models {
                if parent.parent() == entity {
                    let want = if shows_effect { Visibility::Hidden } else { Visibility::Inherited };
                    if *vis != want {
                        *vis = want;
                    }
                }
            }
        }
        // Level of detail (the captain is seen by its portrait camera).
        if let Some((eye, forward)) = eye.filter(|_| !captain) {
            let to = tf.translation - eye;
            let distance = to.length().max(1e-3);
            let radius = 3.0 * tf.scale.x.abs().max(0.1);
            if distance > radius && forward.dot(to / distance) < 0.2 - radius / distance {
                continue;
            }
            if distance > FAR_POSE_DISTANCE && frame.wrapping_add(a.seed) % 3 != 0 {
                continue;
            }
        }
        let overlays: Vec<(&Bound, f32, f32)> = a
            .overlay
            .iter()
            .map(|o| (&*o.bound, o.weight, o.bound.clip.frame_at(o.time, true)))
            .collect();
        let pose = shared.rig.pose_layered(&a.clips.mix(), &overlays, true, None, None);
        posed += 1;
        for (j, &e) in a.joints.iter().enumerate() {
            if let (Ok(mut t), Some(m)) = (transforms.get_mut(e), pose.get(j)) {
                *t = Transform::from_matrix(Mat4::from(clips::to_frame(m, shared.unit)));
            }
        }
        for (part, block, rest) in &a.parts {
            let (Ok(mut t), Some(m)) = (transforms.get_mut(*part), pose.get(*block)) else { continue };
            *t = Transform::from_matrix(Mat4::from(clips::to_frame(m, shared.unit)) * rest.to_matrix());
        }
        if let (Some(hand), Some(block)) = (a.hand, a.hand_block) {
            if let (Ok(mut t), Some(m), Some(p)) = (transforms.get_mut(hand), pose.get(block), shared.points.get(block)) {
                *t = Transform::from_matrix(Mat4::from(clips::to_frame(m, shared.unit)) * Mat4::from_translation(*p));
            }
        }
        if let (Some(bundle), Some(block)) = (a.bundle, a.bundle_block) {
            if let (Ok(mut t), Some(m), Some(p)) = (transforms.get_mut(bundle), pose.get(block), shared.points.get(block)) {
                *t = Transform::from_matrix(Mat4::from(clips::to_frame(m, shared.unit)) * Mat4::from_translation(*p));
            }
        }
    }
    crowd.cost = (posed, started.elapsed().as_secs_f64());
    if frame % 120 == 0 && !playing.is_empty() {
        playing.sort();
        info!("crowd: {} actors, {posed} posed in {:.0} us, clips {:08X?}", playing.len(), crowd.cost.1 * 1e6, playing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(busy: bool, chieftain: bool) -> TribeUnit {
        let mut u = TribeUnit::new(chieftain, 5.0);
        u.busy = busy;
        u
    }

    #[test]
    fn tribal_actions_pick_their_tribe_clips() {
        let e = Entity::PLACEHOLDER;
        let ctx = PlanCtx::default();
        let plan = |t: &UnitTask, u: &TribeUnit, tool: Option<ToolKind>, moving: bool| tribal_plan(t, u, tool, moving, &ctx);
        let idle = UnitTask::Idle(Vec3::ZERO);
        // 0xDA7C10: no tool -> a neutral idle; a tool -> its PROP idle.
        assert_eq!(tribal_request(&idle, &unit(false, false), None, false).0, clip_id::IDLE);
        assert_eq!(plan(&idle, &unit(false, false), Some(ToolKind::StoneAxes), false).main, 0x05E7_DF20);
        assert_eq!(plan(&idle, &unit(false, true), None, false).main, 0x04C6_CB02);
        assert_eq!(tribal_request(&idle, &unit(false, false), None, true).0, clip_id::WALK);
        let mut carrier = unit(false, false);
        carrier.carrying = 3.0;
        // A carrier walks the default locomotion with the platter overlay on its
        // bundle grasper; `walk_carry` is never played (T9K-1).
        assert_eq!(tribal_request(&UnitTask::Deliver(crate::tribal_rts::After::Idle), &carrier, None, true).0, clip_id::WALK);
        assert_eq!(
            plan(&UnitTask::Deliver(crate::tribal_rts::After::Idle), &carrier, None, true).overlay,
            Some(clip_id::PLATTER_OVERLAY)
        );
        assert_eq!(plan(&idle, &unit(false, false), None, true).overlay, None, "an empty-handed walker has no overlay");
        assert_eq!(plan(&idle, &carrier, None, true).overlay, Some(clip_id::PLATTER_OVERLAY), "a bundle in the hands is enough");
        let g = UnitTask::Gather(e);
        assert_eq!(tribal_request(&g, &unit(true, false), None, false).0, clip_id::GATHER_HANDS);
        assert_eq!(tribal_request(&g, &unit(true, false), Some(ToolKind::FruitSticks), false).0, clip_id::GATHER_STICK);
        // 0xD75F70: only a chieftain with the gather tool plays the smack variant.
        assert_eq!(tribal_request(&g, &unit(true, true), None, false).0, clip_id::GATHER_HANDS);
        assert_eq!(tribal_request(&g, &unit(true, true), Some(ToolKind::FruitSticks), false).0, clip_id::CHIEF_GATHER);
        // Attacks: the ability's clips every refresh, attack_idle_gen02 between.
        let h = UnitTask::Hunt(e);
        let axe = plan(&h, &unit(true, false), Some(ToolKind::StoneAxes), false);
        assert_eq!(axe.main, tribe_clip::ATTACK_IDLE);
        assert_eq!(axe.attack.unwrap().clips, &[clip_id::AXE]);
        assert_eq!(axe.attack.unwrap().refresh, 2.0);
        assert_eq!(plan(&h, &unit(true, false), Some(ToolKind::SpikedClubs), false).attack.unwrap().clips, &[clip_id::TORCH]);
        assert_eq!(plan(&h, &unit(true, true), None, false).attack.unwrap().clips, &[clip_id::CHIEF_ATTACK, 0x0576_6A11]);
        assert_eq!(plan(&h, &unit(true, false), None, false).attack.unwrap().clips, &[clip_id::NO_TOOL_ATTACK]);
        let spear = plan(&h, &unit(true, false), Some(ToolKind::ThrowingSpears), false).attack.unwrap();
        assert_eq!((spear.clips, spear.follow), (&[tribe_clip::SPEAR_THROW][..], Some(tribe_clip::SPEAR_GET)));
        // Huts: the building clip after the yell.
        let siege = plan(&UnitTask::Siege(0), &unit(true, false), Some(ToolKind::StoneAxes), false);
        assert_eq!(siege.intro, Some(tribe_clip::BATTLE_YELL));
        assert_eq!(siege.attack.unwrap().clips, &[0x0601_FB4F]);
        // Walking toward a target (0xDB7120).
        assert_eq!(plan(&h, &unit(false, false), Some(ToolKind::StoneAxes), true).main, 0x062E_E781);
        assert_eq!(plan(&h, &unit(false, false), Some(ToolKind::FishingSpears), true).main, tribe_clip::BATTLE_WALK);
        assert_eq!(plan(&h, &unit(false, true), None, true).main, tribe_clip::CHIEF_ATTACK_WALK);
        let p = UnitTask::Perform(0);
        assert_eq!(plan(&p, &unit(false, false), Some(ToolKind::Maracas), true).main, 0x062E_EF25);
        assert_eq!(plan(&p, &unit(false, false), None, true).main, tribe_clip::SOC_WALK);
        assert_eq!(plan(&UnitTask::Raid(0), &unit(false, false), None, true).main, tribe_clip::RAID_SNEAK);
        // The social minigame: start, base loop; the chieftain conducts.
        let horn = plan(&p, &unit(true, false), Some(ToolKind::WoodenHorns), false);
        assert_eq!((horn.intro, horn.main), (Some(0x1472_CE9A), 0xB508_F7E9));
        assert_eq!(plan(&p, &unit(true, false), None, false).main, tribe_clip::NOTOOL_DANCE);
        assert_eq!(plan(&p, &unit(true, true), None, false).main, tribe_clip::CHIEF_CONDUCTOR);
        // Fishing: base + tool + chieftain (0xD73FD0).
        let fish = UnitTask::Fish(Vec3::ZERO);
        let spear_fish = plan(&fish, &unit(true, false), Some(ToolKind::FishingSpears), false);
        assert_eq!((spear_fish.intro, spear_fish.main), (Some(0x02C3_9211), 0x02C3_9216));
        let chief_fish = plan(&fish, &unit(true, true), None, false);
        assert_eq!((chief_fish.intro, chief_fish.main), (Some(0x02C3_9231), 0x02C3_9236));
        assert_eq!(plan(&fish, &unit(true, false), None, false).main, 0x02C3_9215);
        // Eating, healing.
        let eat = plan(&UnitTask::Recover, &unit(true, false), None, false);
        assert_eq!((eat.intro, eat.main), (Some(tribe_clip::PICKUP_TO_EAT), clip_id::EAT));
        let healer = unit(true, false);
        assert_eq!(plan(&idle, &healer, Some(ToolKind::HealingRods), false).main, 0x05F0_F078);
        let healing = PlanCtx { healing: true, ..PlanCtx::default() };
        assert_eq!(tribal_plan(&idle, &healer, Some(ToolKind::HealingRods), false, &healing).main, clip_id::HEAL);
    }

    #[test]
    fn the_troupe_waits_plays_and_ends_with_the_retail_clips() {
        let p = UnitTask::Perform(0);
        let song = |phase: SongPhase| PlanCtx { song: phase, ..PlanCtx::default() };
        let horn = Some(ToolKind::WoodenHorns);
        // At the spot (stages 3..4): the start clip once, then the waiting idle.
        let gathering = tribal_plan(&p, &unit(true, false), horn, false, &song(SongPhase::Gathering));
        assert_eq!((gathering.intro, gathering.main), (Some(0x1472_CE9A), 0xF477_F9BA));
        let bare = tribal_plan(&p, &unit(true, false), None, false, &song(SongPhase::Gathering));
        assert_eq!((bare.intro, bare.main), (None, 0xF477_F9BC));
        let chief = tribal_plan(&p, &unit(true, true), None, false, &song(SongPhase::Gathering));
        assert_eq!(chief.main, 0xF477_F9B6);
        // Playing: the base loop, no second start clip.
        let playing = tribal_plan(&p, &unit(true, false), horn, false, &song(SongPhase::Playing));
        assert_eq!((playing.intro, playing.main), (None, 0xB508_F7E9));
        // The ending: waiting idle around the clips the sequencer queues once.
        let ending = tribal_plan(&p, &unit(true, false), horn, false, &song(SongPhase::Ending { bar: 8.0 }));
        assert_eq!((ending.intro, ending.main), (None, 0xF477_F9BA));
        let mut s = Sequencer::default();
        s.step(&ending, false, 0.1);
        s.ending(SongPhase::Ending { bar: 8.0 }, Some(0), false);
        assert_eq!(s.step(&ending, false, 0.1), (0xD472_CE9C, false, true), "the cheer by tool type");
        assert_eq!(s.step(&ending, true, 0.1), (0x5474_62FF, false, true), "then the crowd's happy ending");
        assert_eq!(s.step(&ending, true, 0.1), (0xF477_F9BA, true, false), "then the waiting idle");
        s.ending(SongPhase::Ending { bar: 8.0 }, Some(0), false);
        assert_eq!(s.step(&ending, false, 0.1).0, 0xF477_F9BA, "queued once per song");
        // A failed song, a member without an instrument: no cheer, the fail clip.
        let mut t = Sequencer::default();
        t.step(&ending, false, 0.1);
        t.ending(SongPhase::Ending { bar: 2.0 }, None, false);
        assert_eq!(t.step(&ending, false, 0.1), (0x1478_0312, false, true));
        // The next song queues them again.
        t.ending(SongPhase::Gathering, None, false);
        t.ending(SongPhase::Ending { bar: 2.0 }, None, false);
        assert!(!t.queue.is_empty());
    }

    #[test]
    fn idles_alternate_between_the_tools_idle_and_lookaround() {
        let mut seen = std::collections::HashSet::new();
        for slot in 0..40 {
            let ctx = PlanCtx { seed: 7, clock: slot as f32 * IDLE_SLOT, ..PlanCtx::default() };
            seen.insert(idle_clip(7, false, &ctx));
        }
        assert_eq!(seen, [0x05F0_F079, 0x05F0_F07A].into_iter().collect());
        let mut bare = std::collections::HashSet::new();
        for slot in 0..80 {
            let ctx = PlanCtx { seed: 3, clock: slot as f32 * IDLE_SLOT, ..PlanCtx::default() };
            bare.insert(idle_clip(0, false, &ctx));
        }
        assert!(bare.iter().all(|c| tribe_clip::IDLES.contains(c) || *c == tribe_clip::IDLE_LOOKAROUND));
        assert!(bare.len() >= 3, "{bare:08X?}");
        assert_eq!(idle_clip(0, true, &PlanCtx::default()), tribe_clip::IDLE_BUNDLE);
    }

    #[test]
    fn the_sequencer_plays_intro_attacks_and_catches() {
        let mut s = Sequencer::default();
        let ctx = PlanCtx::default();
        let plan = tribal_plan(&UnitTask::Siege(0), &unit(true, false), Some(ToolKind::SpikedClubs), false, &ctx);
        // The yell first, then the hut attack, then the idle between attacks.
        assert_eq!(s.step(&plan, false, 0.1), (tribe_clip::BATTLE_YELL, false, true));
        assert_eq!(s.step(&plan, false, 0.1), (tribe_clip::BATTLE_YELL, false, false));
        assert_eq!(s.step(&plan, true, 0.1), (0x05E7_ED36, false, true));
        assert_eq!(s.step(&plan, true, 0.1), (tribe_clip::ATTACK_IDLE, true, false));
        // The next attack comes after the torch's refresh (1.5 s).
        let mut t = 0.0;
        loop {
            let (id, _, fresh) = s.step(&plan, false, 0.1);
            t += 0.1;
            if id == 0x05E7_ED36 {
                assert!(fresh && (1.2..1.8).contains(&t), "{t}");
                break;
            }
            assert!(t < 3.0);
        }
        // A fisher: start, loop, the pull before the catch, then catch + dump + start.
        let mut f = Sequencer::default();
        let mut fisher = unit(true, false);
        let fish = tribal_plan(&UnitTask::Fish(Vec3::ZERO), &fisher, Some(ToolKind::FishingSpears), false, &ctx);
        assert_eq!(f.step(&fish, false, 0.1).0, 0x02C3_9211);
        assert_eq!(f.step(&fish, true, 0.1).0, 0x02C3_9216);
        fisher.fish = crate::tribal_rules::FishCycle::roll(0.5, 0.5);
        fisher.work = fisher.fish.pull_at() + 0.01;
        f.fishing(&fisher, 1);
        assert_eq!(f.step(&fish, false, 0.1).0, 0x02C3_921B);
        assert_eq!(f.step(&fish, true, 0.1).0, 0x02C3_9216);
        fisher.work = fisher.fish.catch_at() + 0.01;
        f.fishing(&fisher, 1);
        let (succ, _, _) = f.step(&fish, false, 0.1);
        assert_eq!(succ, 0x02C3_9221, "the rolled fish_succ_2");
        assert_eq!(f.step(&fish, true, 0.1).0, 0x02C3_9224);
        assert_eq!(f.step(&fish, true, 0.1).0, 0x02C3_9211);
        // A plan change drops the queue.
        fisher.carrying = 1.5;
        let walk = tribal_plan(&UnitTask::Fish(Vec3::ZERO), &fisher, Some(ToolKind::FishingSpears), true, &ctx);
        assert_eq!(f.step(&walk, false, 0.1).0, clip_id::WALK);
        assert_eq!(walk.overlay, Some(clip_id::PLATTER_OVERLAY), "the carried catch rides the overlay");
    }

    #[test]
    fn swimming_members_play_the_creature_swim_groups() {
        assert_eq!(swim_request(true), (crate::creature_support::SWIM, true));
        assert_eq!(swim_request(false), (crate::creature_support::SWIM_IN_PLACE, true));
        // gen_swim / gen_swim_inplace (notes/creator-re/tlsa_groups.txt).
        assert_eq!(clip_id::SWIM, 0x0293_BB70);
        assert_eq!(clip_id::SWIM_IN_PLACE, 0x02E6_EA7D);
    }

    #[test]
    fn members_hold_the_retail_handheld_items() {
        use handheld::*;
        // 0xC23740: tool type -> item; the chieftain only shows his staff.
        assert_eq!(tool_handheld(Some(ToolKind::SpikedClubs), false), Some(HUNTING_TOOL[0]));
        assert_eq!(tool_handheld(Some(ToolKind::ThrowingSpears), false), Some(HUNTING_TOOL[2]));
        assert_eq!(tool_handheld(Some(ToolKind::Maracas), false), Some(MARACA));
        assert_eq!(tool_handheld(Some(ToolKind::FruitSticks), false), Some(TREE_SMACKER));
        assert_eq!(tool_handheld(Some(ToolKind::FishingSpears), false), Some(FISHING_TOOL));
        assert_eq!(tool_handheld(Some(ToolKind::HealingRods), false), Some(HEALING_STAFF));
        assert_eq!(tool_handheld(None, false), None);
        assert_eq!(tool_handheld(None, true), Some(CHIEFTAIN_STAFF));
        assert_eq!(tool_handheld(Some(ToolKind::StoneAxes), true), Some(CHIEFTAIN_STAFF));
        assert_eq!(tool_handheld(Some(ToolKind::WoodenHorns), true), None);
        // The ids are the FNV names of the handheld effects.
        assert_eq!(crate::world_objects::name_id("trg_hunting_tool1"), HUNTING_TOOL[0]);
        assert_eq!(crate::world_objects::name_id("trg_eating_fruit"), EATING_FRUIT);
        assert_eq!(crate::world_objects::name_id("trg_chieftain_staff"), CHIEFTAIN_STAFF);
        assert_eq!(crate::world_objects::name_id("trg_social_maraca"), MARACA);
        // Carried food and eating: the load is a platter on the bundle
        // grasper; the favoured hand keeps the tool.
        let mut carrier = unit(true, false);
        carrier.carrying = 5.0;
        carrier.carry_kind = crate::tribal_rts::FoodKind::Meat;
        let deliver = UnitTask::Deliver(crate::tribal_rts::After::Idle);
        assert_eq!(tribal_handheld(&deliver, &carrier, None), None);
        assert_eq!(tribal_handheld(&deliver, &carrier, Some(ToolKind::FruitSticks)), Some(TREE_SMACKER));
        carrier.carry_kind = crate::tribal_rts::FoodKind::Fish;
        assert_eq!(tribal_handheld(&deliver, &carrier, Some(ToolKind::FishingSpears)), Some(FISHING_TOOL_FISH));
        assert_eq!(tribal_handheld(&UnitTask::Recover, &unit(true, false), None), Some(EATING_FRUIT));
    }

    /// The handheld items are installed effects (0xC150D0 `CreateVisualEffect`).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_handheld_effects_exist() {
        use handheld::*;
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        let ids = [
            HUNTING_TOOL[0], HUNTING_TOOL[1], HUNTING_TOOL[2], FISHING_TOOL, FISHING_TOOL_FISH, EATING_FISH, EATING_FRUIT,
            EATING_MEAT, MARACA, HORN, DIDGERIDOO, TREE_SMACKER, CHIEFTAIN_STAFF, HEALING_STAFF,
        ];
        let missing: Vec<String> = ids.iter().filter(|id| lib.find(**id).is_none()).map(|id| format!("{id:08X}")).collect();
        println!("handheld effects missing: {missing:?} of {}", ids.len());
        assert!(missing.is_empty(), "missing handheld effects {missing:?}");
    }

    #[test]
    fn scatter_stays_in_range() {
        for s in 0..200u32 {
            let p = scatter(s, 9.0);
            assert!(p.y == 0.0 && p.length() <= 9.0 + 1e-3 && p.length() >= 0.35 * 9.0 - 1e-3);
        }
    }

    /// 0xC24270: entry `id % n` is the favoured grasper and the bundle goes to
    /// the next entry of the other side (the next entry when none is).
    #[test]
    fn each_member_picks_its_favoured_hand_and_the_opposite_bundle_hand() {
        // Two arms (blocks 3 and 5, left and right).
        let arms = [(3usize, -0.4f32), (5, 0.4)];
        assert_eq!(grasper_pair(&arms, 0), (Some(3), Some(5)));
        assert_eq!(grasper_pair(&arms, 1), (Some(5), Some(3)));
        assert_eq!(grasper_pair(&arms, 6), (Some(3), Some(5)), "the id wraps");
        // Four graspers (left, left, right, right): the bundle skips the
        // favoured hand's side.
        let four = [(1usize, -0.5f32), (2, -0.2), (3, 0.2), (4, 0.5)];
        assert_eq!(grasper_pair(&four, 0), (Some(1), Some(3)));
        assert_eq!(grasper_pair(&four, 2), (Some(3), Some(1)));
        assert_eq!(grasper_pair(&four, 3), (Some(4), Some(1)));
        // One grasper holds both; none holds nothing.
        assert_eq!(grasper_pair(&[(7, 0.1)], 5), (Some(7), Some(7)));
        assert_eq!(grasper_pair(&[], 5), (None, None));
        // Over a crowd both hands are the favoured one.
        let favoured: std::collections::BTreeSet<_> = (0..8).filter_map(|id| grasper_pair(&arms, id).0).collect();
        assert_eq!(favoured.len(), 2);
    }

    /// Every tribe clip the crowd plays decodes and binds to the installed
    /// starter's rig.
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_tribe_clips_bind_to_the_starter() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let key = crate::dbpf::ResourceKey {
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
            type_id: crate::creature_data::TYPE,
        };
        let data = crate::creature_data::parse(&originals.resource_index.read(key).unwrap()).unwrap();
        let rig = clips::rig_of_creature_data(&data).unwrap();
        let mut ids: Vec<u32> = clip_id::ALL.into_iter().chain(tribe_clip::ALL).collect();
        for kind in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11] {
            let a = tool_anims(kind);
            ids.extend([a.idle, a.lookaround, a.en_route].into_iter().filter(|&c| c != 0));
            ids.extend(attack_clips(kind));
            ids.extend(building_clips(kind).unwrap_or(&[]));
        }
        for (start, base, solo) in tribe_clip::INSTRUMENT {
            ids.extend([start, base, solo]);
        }
        ids.sort();
        ids.dedup();
        for id in ids {
            let c = clips::clip(&originals, id).unwrap_or_else(|| panic!("clip {id:08X}"));
            let b = rig.bind(c.clone());
            let pose = rig.pose(&[(&b, 1.0, c.frame_at(0.4, true))]);
            assert!(pose.iter().all(|m| m.translation.is_finite()), "{id:08X}");
            println!("{id:08X} {:.2}s cyclic {} channels {}", c.seconds, c.cyclic, b.driven_channels());
        }
    }

    /// An overlay layer only changes the joints its channels animate: laid over
    /// an idle, the walk's channels take over their joints and a weightless
    /// overlay changes nothing. (`platter_grasper_overlay` itself targets the graspers,
    /// which the starter's rig lacks: it binds to no channel there, see
    /// `installed_tribe_clips_bind_to_the_starter`.)
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_overlay_layers_replace_only_the_joints_they_animate() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let key = crate::dbpf::ResourceKey {
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
            type_id: crate::creature_data::TYPE,
        };
        let data = crate::creature_data::parse(&originals.resource_index.read(key).unwrap()).unwrap();
        let rig = clips::rig_of_creature_data(&data).unwrap();
        let walk_clip = clips::clip(&originals, clip_id::WALK).expect("walk_neutral");
        let walk = rig.bind(walk_clip.clone());
        let idle_clip = clips::clip(&originals, clip_id::IDLE).expect("soc_idle_neutral2");
        let idle = rig.bind(idle_clip.clone());
        let walk_frame = walk_clip.frame_at(0.4, true);
        let idle_frame = idle_clip.frame_at(0.4, true);
        let base = rig.pose(&[(&idle, 1.0, idle_frame)]);
        let walking = rig.pose(&[(&walk, 1.0, walk_frame)]);
        let over = rig.pose_layered(&[(&idle, 1.0, idle_frame)], &[(&walk, 1.0, walk_frame)], true, None, None);
        let moved = base.iter().zip(&over).filter(|(a, b)| !a.abs_diff_eq(**b, 1e-4)).count();
        println!("walk over idle: {moved} of {} joints change ({} walk channels)", base.len(), walk.driven_channels());
        assert!(moved > 0, "the overlay animates something");
        assert!(
            over.iter().zip(&walking).any(|(a, b)| a.abs_diff_eq(*b, 1e-4)),
            "the joints the walk drives follow the walk"
        );
        let none = rig.pose_layered(&[(&idle, 1.0, idle_frame)], &[(&walk, 0.0, 0.0)], true, None, None);
        assert!(base.iter().zip(&none).all(|(a, b)| a.abs_diff_eq(*b, 1e-5)), "a weightless overlay changes nothing");
    }

    /// The tool and ability animation tables mirror the installed PROPs.
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_tool_and_ability_animations_match_the_tables() {
        use crate::world_objects::name_id;
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        // Property ids from SporeModder-FX's `reg_property.txt` (registered,
        // not the FNV of the names).
        let pid = |name: &str| match name {
            "toolData_DefaultToolIdle" => 0x0429_4754,
            "toolData_LookaroundToolIdle" => 0x0429_4755,
            "toolData_EnRouteAnim" => 0x062E_CADE,
            "creatureAbilityAnimationIDs" => 0x0520_47F7,
            "creatureAbilityBuildingSpecificAnimationID" => 0x05E7_ECA0,
            _ => name_id(name),
        };
        let u32_of = |list: &crate::prop::PropertyList, name: &str| -> Vec<u32> {
            list.0
                .get(&pid(name))
                .map(|p| {
                    p.values
                        .iter()
                        .filter_map(|v| v.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap())))
                        .collect()
                })
                .unwrap_or_default()
        };
        let prop = |group: u32, name: &str| {
            index
                .properties(crate::dbpf::ResourceKey { group_id: group, instance_id: name_id(name), type_id: 0x00B1_B104 })
                .unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        for (name, kind) in [
            ("attack1", 1u8),
            ("attack2", 2),
            ("attack3", 3),
            ("social1", 4),
            ("social2", 5),
            ("social3", 6),
            ("gather", 7),
            ("fish", 8),
            ("heal", 9),
            ("chieftain", 11),
        ] {
            let list = prop(0x0429_2F52, name);
            let a = tool_anims(kind);
            let first = |n: &str| u32_of(&list, n).first().copied().unwrap_or(0);
            assert_eq!(first("toolData_DefaultToolIdle"), a.idle, "{name} idle");
            assert_eq!(first("toolData_LookaroundToolIdle"), a.lookaround, "{name} lookaround");
            assert_eq!(first("toolData_EnRouteAnim"), a.en_route, "{name} en route");
        }
        for (name, kind) in [
            ("tribe_hunt1", 1u8),
            ("tribe_hunt2", 2),
            ("tribe_hunt3", 3),
            ("tribe_shaman_staff", 9),
            ("tribe_chieftain_staff", 11),
        ] {
            let list = prop(0xC85C_1F8B, name);
            assert_eq!(u32_of(&list, "creatureAbilityAnimationIDs"), attack_clips(kind), "{name} attacks");
            assert_eq!(
                u32_of(&list, "creatureAbilityBuildingSpecificAnimationID"),
                building_clips(kind).unwrap().to_vec(),
                "{name} building"
            );
        }
    }
}
