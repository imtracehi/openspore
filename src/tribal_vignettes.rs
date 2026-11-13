//! Tribal vignettes played by the game code (not by a cinematic script) and
//! the tribe clips of the rival villagers / raiders (helper `tribal-visuals`,
//! notes T9L in `notes\tribal-final-re-notes.md`).
//!
//! RETAIL (VERIFIED in code unless marked):
//! * Vignette data (loader 0xBC58B0, one record of 0x7B4 bytes per PROP): per
//!   sequence n the actions `vi_animations_n` with, per action, `vi_loops_n`
//!   (default 0), `vi_effects_n` (default -1: a handheld item id of 0xC250A0,
//!   0 = empty hand) and `vi_effect_limbs_n` (default 0); `vi_actor_sequences`
//!   (default: actor i plays sequence i), `vi_actor_offsets`,
//!   `vi_actor_orientations` (degrees), `vi_actor_alternate_color`,
//!   `vi_min_happy` / `vi_max_happy` (0 / 100), `vi_city_specialty`,
//!   `vi_capture_event`, `vi_time_of_day` (0 any, 1 day, 2 night),
//!   `vi_protest_event`, `vi_attitude_towards_player` (-1 any),
//!   `vi_has_mouth_type`, `vi_actor_offsets_unscaled`, `vi_fourCC`,
//!   `vi_required_tools` (a tool-type bit mask), `vi_effects_general` with
//!   `vi_effects_general_offsets` (effects at the vignette's centre).
//! * Celebration (the tribe level-up 0xCDA4F0, with `TRG_Upgrade`): the
//!   chieftain and the members that are alive, unflagged, not busy and within
//!   the tribe radius of its centre take places (at most 15; the rest are sent
//!   to 1.5 x the radius); the vignette is `Vignettes_Tribe!celebrate_<N>guys`
//!   for N = 2..15 of them, at the tribe's position and orientation.
//! * Idle tribe vignettes (citizen behaviour evaluator 0xDB60C0, start
//!   0xDAE230): a citizen within the tribe radius of a tribe with no running
//!   vignette picks at random among `Vignettes_Tribe_1~` (0x055172D5) those
//!   whose happy range holds the tribe's happiness, whose time of day is 0 or
//!   the current one, whose attitude is -1 or the tribe's toward the player
//!   (any for the player's own tribe), whose places are no more than the
//!   tribe's free adult members in the radius, and whose required tools the
//!   tribe owns; the vignette starts at the tribe's position / orientation and
//!   the members join it while places are free.
//!
//! ADAPTED (marked at the code): the vignette centre is the tribe's camp fire
//! (the retail tribe position, `MasterTribeLayout` puts the fire at the
//! origin), offsets are metres 1:1; an idle evaluation every
//! [`IDLE_SLOT_SECONDS`] per tribe with the chance `idleVignettePercentage`
//! (0.75) (the behaviour tree's own cadence is not decoded); each actor plays
//! its action list once (`vi_loops` times per action) and leaves; happiness is
//! not simulated (the default range always passes); a vignette member is held
//! at its place and leaves the moment its task changes.
//!
//! Rival villagers and raiders are another species: the installed creature
//! model posed by `locomotion_preview` with the creature clips. They now get
//! the tribe clips through its `PoseClip` request (`rival_plan`): idles, the
//! attack walk / raid sneak / carry walks, the raid-party yell and battle-ready
//! loop with their tool's building or attack clips (0xDBAAF0), the steal-food
//! congregate (0x05DCC520) and the food pick-up, the fight with their tool's
//! ability clips. Walking without a special walk keeps the species' own
//! locomotion (walk / run by speed).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use crate::game::{Health, Player, RunProgress, TribeMate};
use crate::locomotion_preview::PoseClip;
use crate::outfit_crowd::{attack_clips, building_clips, clip_id, idle_clip, tool_anims, tribe_clip, PlanCtx};
use crate::stages::{GameStage, StageOwned};
use crate::swarm_fx::SwarmEffect;
use crate::tribal::{RaidPhase, Raider};
use crate::tribal_rts::{RivalMember, RivalMode, TribeUnit, UnitTask};

/// `Vignettes_Tribe` (the game-code celebration, the cinematics' vignettes).
pub const TRIBE_GROUP: u32 = 0x1BA8_B527;
/// `Vignettes_Tribe_1~`: the idle tribe vignettes (0xDB60C0).
pub const TRIBE_IDLE_GROUP: u32 = 0x0551_72D5;
/// `celebrate_02guys` .. `celebrate_15guys` (0xCDA4F0's switch).
pub const CELEBRATE: [u32; 14] = [
    0x3638_236D, 0x056F_0C80, 0xDB08_9AAB, 0x0E27_71CE, 0x13AF_4511, 0x1B55_0384, 0x56E1_A74F, 0x90BB_C732,
    0x1C15_46B0, 0x11F5_1F5D, 0x3F16_DB3A, 0x04ED_0917, 0xDDC5_4B74, 0x0E8E_6141,
];
/// `Vignettes_Tribe_2~`: the pair chats at the tribe's chat areas (0xDB6610).
pub const TRIBE_CHAT_GROUP: u32 = 0x0552_3DDC;
/// `MasterTribeLayout!TribeChatAreas` (x, y in the tribe frame; the third value
/// is 5 for every area).
pub const CHAT_AREAS: [(f32, f32); 6] = [(7.5, 8.0), (7.5, -8.0), (-5.0, 7.0), (-5.0, -7.0), (0.0, 8.0), (0.0, -8.0)];
/// A chat vignette's lifetime (0xDAD050: 20.0 at +0x1AC).
pub const CHAT_SECONDS: f32 = 20.0;
/// ADAPTED: the layout's metres in OpenSpore's compressed villages (the camp
/// fire ring of shacks sits at about 0.4 of the retail tool positions).
pub const CHAT_AREA_SCALE: f32 = 0.45;
/// `idleVignettePercentage` (TribeTuning).
pub const IDLE_VIGNETTE_PERCENTAGE: f32 = 0.75;
/// ADAPTED: seconds between a tribe's idle-vignette evaluations (retail idle
/// behaviours run 10 or 15 s, 0xDA7C10).
pub const IDLE_SLOT_SECONDS: f32 = 15.0;
/// ADAPTED: how far from its centre a member may be to join (the tribe radius;
/// OpenSpore's compressed villages).
pub const TRIBE_RADIUS: f32 = 12.0;
/// `raid_congregate_01` (0xCE1690).
pub const RAID_CONGREGATE: u32 = 0x05DC_C520;
/// `pickupfood` (0xDBC770 / 0xD73FD0).
pub const PICKUP_FOOD: u32 = 0x0279_6B30;
/// `com_battle_ready_1`, looped by RaidHuts (0xDBAAF0).
pub const BATTLE_READY: u32 = 0x0567_DB46;
/// `idle_charge_angry01`: the raid party's charge (0xDBAAF0; not sequenced:
/// `PoseClip` plays one intro).
#[allow(dead_code)]
pub const CHARGE: u32 = 0x0004_CFFB;
/// The idle a vignette member falls back to between actions.
const FALLBACK_IDLE: u32 = clip_id::IDLE;

/// The handheld item effect of item `item` (0xC250A0, item ids 1..23; 0 = an
/// empty hand). The FNV ids of the effects `CreateVisualEffect` starts.
pub fn handheld_effect(item: u32) -> Option<u32> {
    Some(match item {
        1 => 0xDBFD_6844,
        2 => 0xDBFD_6847,
        3 => 0xDBFD_6846,
        4 => 0x1CF8_4D57,
        5 => 0x3A88_7470,
        6 | 8 | 10 => 0x1192_B368,
        7 => 0xAE02_4C94,
        9 => 0xDC87_FF23,
        11 => 0xB4DD_0222,
        12 => 0x5223_CF1E,
        13 => 0xB931_9AC6,
        14 => 0x5986_7820,
        15 => 0xF812_2731,
        16 => 0x759B_0E13,
        17 => 0x8643_9B47,
        18 => 0x792E_9926,
        19 => 0xD72F_5830,
        20 => 0xC471_7433,
        21 => 0xA8F7_47AE,
        22 => 0x2B4D_0331,
        23 => 0x541B_4A0D,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Vignette data
// ---------------------------------------------------------------------------

/// One action of a vignette place: the clip, how often it plays and the
/// handheld item it shows (`None` = the hand as it is, `Some(0)` = empty).
#[derive(Clone, Debug, PartialEq)]
pub struct VignetteAction {
    pub clip: u32,
    pub loops: u32,
    pub item: Option<u32>,
    pub limb: u32,
}

/// One place of a vignette: offset (Spore x right, y forward, metres), how it
/// faces (degrees) and its actions.
#[derive(Clone, Debug, PartialEq)]
pub struct VignettePlace {
    pub offset: Vec2,
    pub orientation: f32,
    pub actions: Vec<VignetteAction>,
}

/// A vignette PROP (ORIGINAL data, 0xBC58B0).
#[derive(Clone, Debug, PartialEq)]
pub struct VignetteDef {
    pub id: u32,
    pub name: String,
    pub places: Vec<VignettePlace>,
    /// `vi_effects_general` at their offsets (the vignette frame).
    pub effects: Vec<(u32, Vec2)>,
    pub unscaled: bool,
    pub time_of_day: u32,
    pub attitude: i32,
    pub required_tools: u32,
    pub happy: (f32, f32),
}

mod prop_ids {
    pub const TABLEAU: u32 = 0x0486_65C7;
    pub const ANIMATIONS: u32 = 0x0486_6690;
    pub const LOOPS: u32 = 0x0486_66A0;
    pub const EFFECTS: u32 = 0x0486_66B0;
    pub const EFFECT_LIMBS: u32 = 0x0486_66C0;
    pub const SEQUENCES: u32 = 0x0487_7BF5;
    pub const OFFSETS: u32 = 0x0487_9D5A;
    pub const NAME: u32 = 0x0487_A133;
    pub const ORIENTATIONS: u32 = 0x049A_2146;
    pub const MIN_HAPPY: u32 = 0x048A_40A6;
    pub const MAX_HAPPY: u32 = 0x048A_40A7;
    pub const TIME_OF_DAY: u32 = 0x04F3_8139;
    pub const ATTITUDE: u32 = 0x0564_CC3E;
    pub const REQUIRED_TOOLS: u32 = 0x056C_D5EA;
    pub const UNSCALED: u32 = 0x05B0_5121;
    pub const EFFECTS_GENERAL: u32 = 0x0508_D625;
    pub const EFFECTS_GENERAL_OFFSETS: u32 = 0x0508_D626;
}

fn be_u32s(list: &crate::prop::PropertyList, id: u32) -> Option<Vec<u32>> {
    let p = list.0.get(&id)?;
    Some(p.values.iter().filter_map(|v| v.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))).collect())
}

fn be_f32s(list: &crate::prop::PropertyList, id: u32) -> Option<Vec<f32>> {
    let p = list.0.get(&id)?;
    Some(p.values.iter().filter_map(|v| v.get(..4).map(|b| f32::from_be_bytes(b.try_into().unwrap()))).collect())
}

fn le_vec2s(list: &crate::prop::PropertyList, id: u32) -> Option<Vec<Vec2>> {
    let p = list.0.get(&id)?;
    Some(
        p.values
            .iter()
            .filter_map(|v| {
                let x = f32::from_le_bytes(v.get(..4)?.try_into().ok()?);
                let y = f32::from_le_bytes(v.get(4..8)?.try_into().ok()?);
                Some(Vec2::new(x, y))
            })
            .collect(),
    )
}

/// Decode a vignette PROP the way 0xBC58B0 does (defaults included).
pub fn parse_vignette(id: u32, list: &crate::prop::PropertyList) -> Option<VignetteDef> {
    use prop_ids::*;
    // The highest sequence with actions (0xBC58B0 scans 12 ids).
    let sequences_with_actions = (0..12u32).filter(|n| list.0.contains_key(&(ANIMATIONS + n))).max().map(|n| n + 1)?;
    let sequence_actions = |n: u32| -> Vec<VignetteAction> {
        let clips = be_u32s(list, ANIMATIONS + n).unwrap_or_default();
        let loops = be_u32s(list, LOOPS + n).unwrap_or_default();
        let items = be_u32s(list, EFFECTS + n).unwrap_or_default();
        let limbs = be_u32s(list, EFFECT_LIMBS + n).unwrap_or_default();
        clips
            .iter()
            .enumerate()
            .map(|(i, &clip)| VignetteAction {
                clip,
                loops: loops.get(i).copied().unwrap_or(0),
                item: items.get(i).copied().filter(|v| *v != u32::MAX),
                limb: limbs.get(i).copied().unwrap_or(0),
            })
            .collect()
    };
    let sequences = be_u32s(list, SEQUENCES).unwrap_or_else(|| (0..sequences_with_actions).collect());
    let offsets = le_vec2s(list, OFFSETS).unwrap_or_default();
    let orientations = be_f32s(list, ORIENTATIONS).unwrap_or_default();
    let places: Vec<VignettePlace> = sequences
        .iter()
        .enumerate()
        .map(|(i, &n)| VignettePlace {
            offset: offsets.get(i).copied().unwrap_or(Vec2::ZERO),
            orientation: orientations.get(i).copied().unwrap_or(0.0),
            actions: if n < 12 { sequence_actions(n) } else { Vec::new() },
        })
        .collect();
    let keys = list.0.get(&EFFECTS_GENERAL).map(|p| {
        p.values
            .iter()
            .filter_map(|v| v.get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap())))
            .collect::<Vec<u32>>()
    });
    let general_offsets = le_vec2s(list, EFFECTS_GENERAL_OFFSETS).unwrap_or_default();
    let effects = keys
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .filter(|(_, e)| *e != 0)
        .map(|(i, e)| (e, general_offsets.get(i).copied().unwrap_or(Vec2::ZERO)))
        .collect();
    let one = |id: u32| be_u32s(list, id).and_then(|v| v.first().copied());
    let flag = |id: u32| list.0.get(&id).and_then(|p| p.values.first()).and_then(|v| v.first().copied()).unwrap_or(0) != 0;
    let name = list.0.get(&NAME).and_then(|p| p.text()).map(|t| t.2).unwrap_or_default();
    let _tableau = one(TABLEAU);
    Some(VignetteDef {
        id,
        name,
        places,
        effects,
        unscaled: flag(UNSCALED),
        time_of_day: one(TIME_OF_DAY).unwrap_or(0),
        attitude: one(ATTITUDE).map_or(-1, |v| v as i32),
        required_tools: be_u32s(list, REQUIRED_TOOLS).unwrap_or_default().iter().fold(0, |m, t| m | 1u32 << (t & 31)),
        happy: (
            be_f32s(list, MIN_HAPPY).and_then(|v| v.first().copied()).unwrap_or(0.0),
            be_f32s(list, MAX_HAPPY).and_then(|v| v.first().copied()).unwrap_or(100.0),
        ),
    })
}

/// The installed vignette `group!instance`.
pub fn load_vignette(index: &crate::asset_catalog::ResourceIndex, group: u32, instance: u32) -> Option<VignetteDef> {
    let key = crate::dbpf::ResourceKey { group_id: group, instance_id: instance, type_id: crate::asset_catalog::PROP };
    let list = crate::prop::parse(&index.read(key).ok()?).ok()?;
    parse_vignette(instance, &list)
}

/// The installed vignette named by a script spec (`Vignettes_Tribe!trg_build.prop`).
pub fn load_vignette_spec(index: &crate::asset_catalog::ResourceIndex, spec: &str) -> Option<VignetteDef> {
    let (group, name) = spec.split_once('!')?;
    let name = name.strip_suffix(".prop").unwrap_or(name);
    load_vignette(index, crate::world_objects::name_id(group), crate::world_objects::name_id(name))
}

/// A temporary member that fills a vignette place its tribe could not fill.
/// VERIFIED (0xAD3EF0): retail creates citizens of the tribe for the free
/// places (0xC8E730(1) + cTribe AddMember 0xC95F00) and registers them with
/// the cinematic (0xAE0A00). ADAPTED: OpenSpore spawns a body-only actor of the
/// tribe's species (no AI, not in the roster) that the cinematic removes at
/// its end (`host::end_cleanup`).
#[derive(Component, Debug, Clone, Copy)]
pub struct VignetteFiller;

/// Spawn a filler of `owner`'s tribe at `at`: the player's species (the crowd
/// skin poses it) or the NPC village's species (posed by `locomotion_preview`).
pub fn spawn_filler(world: &mut World, owner: VignetteOwner, at: Vec3) -> Option<Entity> {
    use bevy::ecs::system::SystemState;
    match owner {
        VignetteOwner::Player => {
            let mut state: SystemState<(Commands, ResMut<Assets<StandardMaterial>>, Res<crate::Originals>, Res<RunProgress>)> =
                SystemState::new(world);
            let entity = {
                let (mut commands, mut materials, originals, run) = state.get_mut(world).ok()?;
                let model = &originals.baked_body.as_ref()?.model;
                let material = model.material.clone()?;
                let scale = run.creature_stats().size;
                let e = commands
                    .spawn((
                        StageOwned,
                        VignetteFiller,
                        Name::new("vignette filler"),
                        Mesh3d(model.draw_mesh()),
                        MeshMaterial3d(material),
                        Transform::from_translation(at).with_scale(Vec3::splat(scale)),
                        Visibility::Inherited,
                    ))
                    .id();
                crate::creature::mount_saved_parts(&mut commands, &mut materials, e, &originals, &run, 1, scale);
                e
            };
            state.apply(world);
            Some(entity)
        }
        VignetteOwner::Village(index) => {
            let mut state: SystemState<(
                Commands,
                ResMut<crate::Originals>,
                ResMut<Assets<Mesh>>,
                ResMut<Assets<Image>>,
                ResMut<Assets<StandardMaterial>>,
            )> = SystemState::new(world);
            let entity = {
                let (mut commands, mut originals, mut meshes, mut images, mut materials) = state.get_mut(world).ok()?;
                let e = crate::tribal_rts::spawn_villager_body(
                    &mut commands,
                    &mut originals,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                    index,
                    at,
                );
                commands.entity(e).insert((VignetteFiller, Name::new("vignette filler")));
                e
            };
            state.apply(world);
            Some(entity)
        }
    }
}

/// Every installed vignette of `group`, in instance order.
pub fn load_group(index: &crate::asset_catalog::ResourceIndex, group: u32) -> Vec<VignetteDef> {
    let mut keys: Vec<u32> = index
        .records
        .keys()
        .filter(|k| k.group_id == group && k.type_id == crate::asset_catalog::PROP)
        .map(|k| k.instance_id)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter().filter_map(|i| load_vignette(index, group, i)).collect()
}

/// 0xDB60C0's filter: the vignettes a tribe may start now. `attitude` is the
/// tribe's attitude toward the player (`None` for the player's own tribe),
/// `tools` the tool types it owns (bit per type).
pub fn idle_candidates(defs: &[VignetteDef], members: usize, night: bool, attitude: Option<u8>, tools: u32, happy: f32) -> Vec<usize> {
    let time = if night { 2 } else { 1 };
    defs.iter()
        .enumerate()
        .filter(|(_, d)| {
            d.happy.0 <= happy
                && happy <= d.happy.1
                && (d.time_of_day == 0 || d.time_of_day == time)
                && (d.attitude == -1 || attitude.is_none_or(|a| d.attitude == a as i32))
                && !d.places.is_empty()
                && d.places.len() <= members
                && (d.required_tools == 0 || d.required_tools & tools == d.required_tools)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The celebration vignette for `n` celebrants (0xCDA4F0: none below 2).
pub fn celebrate_vignette(n: usize) -> Option<u32> {
    (2..=15).contains(&n).then(|| CELEBRATE[n - 2])
}

// ---------------------------------------------------------------------------
// Running vignettes
// ---------------------------------------------------------------------------

/// Whose vignette it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VignetteOwner {
    Player,
    Village(usize),
}

impl VignetteOwner {
    fn slot(self) -> usize {
        match self {
            VignetteOwner::Player => 0,
            VignetteOwner::Village(i) => 1 + i.min(crate::tribal::RIVAL_VILLAGES - 1),
        }
    }
}

/// A member playing a place of a vignette.
#[derive(Clone, Debug)]
pub struct VignetteActor {
    pub entity: Entity,
    pub place: Vec3,
    pub yaw: f32,
    /// (clip, seconds, item) in play order (`vi_loops` repeats expanded).
    actions: Vec<(u32, f32, Option<u32>)>,
    index: usize,
    t: f32,
    pub done: bool,
    serial: u32,
    /// Chat roles play their list again until the vignette's lifetime ends.
    repeat: bool,
}

impl VignetteActor {
    /// The clip playing now and its time.
    pub fn current(&self) -> Option<(u32, f32)> {
        (!self.done).then(|| self.actions.get(self.index).map(|a| (a.0, self.t))).flatten()
    }

    /// The handheld item the current action shows, if it sets one.
    pub fn item(&self) -> Option<u32> {
        if self.done {
            return None;
        }
        // An item stays in the hand until another action changes it.
        self.actions[..=self.index.min(self.actions.len().saturating_sub(1))].iter().rev().find_map(|a| a.2)
    }

    /// Advance by `dt`; true while the actor still plays.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.done {
            return false;
        }
        self.t += dt.max(0.0);
        while let Some(a) = self.actions.get(self.index) {
            if self.t < a.1 {
                return true;
            }
            self.t -= a.1;
            self.index += 1;
            self.serial = self.serial.wrapping_add(1);
            if self.repeat && self.index >= self.actions.len() {
                self.index = 0;
            }
        }
        self.done = true;
        false
    }
}

/// A vignette the game code started.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RunningVignette {
    pub id: u32,
    pub name: String,
    pub owner: VignetteOwner,
    /// The tribe vignette (`None`, tribe+0x26C) or a pair chat at an area.
    pub chat_area: Option<usize>,
    /// Seconds left of a chat (`CHAT_SECONDS`).
    pub lifetime: Option<f32>,
    pub centre: Vec3,
    pub yaw: f32,
    pub actors: Vec<VignetteActor>,
    effects: Vec<Entity>,
}

/// The game-code vignettes of the Tribal stage.
#[derive(Resource, Default)]
pub struct TribalVignettes {
    pub running: Vec<RunningVignette>,
    idle: Option<Arc<Vec<VignetteDef>>>,
    celebrations: HashMap<u32, Option<Arc<VignetteDef>>>,
    timers: [f32; 1 + crate::tribal::RIVAL_VILLAGES],
    chat_timers: [f32; 1 + crate::tribal::RIVAL_VILLAGES],
    chats: Option<Arc<Vec<VignetteDef>>>,
    last_cinematic: Option<String>,
    rng: u32,
    /// Vignettes started (diagnostics, tests): (name, owner).
    pub started: Vec<(String, VignetteOwner)>,
    /// Capture option: the idle vignette the player's idle members play next
    /// (`--tribal-visuals-demo vignette:<name>`, no filters).
    pub demo: Option<String>,
}

impl TribalVignettes {
    fn actor(&self, e: Entity) -> Option<&VignetteActor> {
        self.running.iter().flat_map(|v| v.actors.iter()).find(|a| a.entity == e && !a.done)
    }

    /// The clip a vignette plays on `e` and its time.
    pub fn actor_clip(&self, e: Entity) -> Option<(u32, f32)> {
        self.actor(e).and_then(|a| a.current())
    }

    /// The handheld item effect a vignette shows on `e` (`Some(None)`: an
    /// empty hand; `None`: the member's own item).
    pub fn actor_item(&self, e: Entity) -> Option<Option<u32>> {
        self.actor(e).and_then(|a| a.item()).map(handheld_effect)
    }

    /// `e` holds a place of a running vignette.
    pub fn busy(&self, e: Entity) -> bool {
        self.actor(e).is_some()
    }

    fn roll(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F);
        ((self.rng >> 8) & 0xFFFF) as f32 / 65536.0
    }

    fn pick(&mut self, n: usize) -> usize {
        ((self.roll() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

/// The actors of `def` placed around `centre` (frame facing `yaw`): the
/// members in order take the places in order (0xAD3EF0 / 0xBC4A80).
pub fn place_actors(def: &VignetteDef, centre: Vec3, yaw: f32, members: &[Entity], seconds: &dyn Fn(u32) -> Option<f32>) -> Vec<VignetteActor> {
    members
        .iter()
        .zip(def.places.iter())
        .filter_map(|(&entity, place)| {
            let local = crate::cinematics::camera::spore_to_bevy([place.offset.x, place.offset.y, 0.0]);
            let actions: Vec<(u32, f32, Option<u32>)> = place
                .actions
                .iter()
                .filter_map(|a| seconds(a.clip).filter(|s| *s > 0.0).map(|s| (a.clip, s, a.item, a.loops.max(1))))
                .flat_map(|(c, s, i, n)| std::iter::repeat_n((c, s, i), n as usize))
                .collect();
            (!actions.is_empty()).then(|| VignetteActor {
                entity,
                place: centre + Quat::from_rotation_y(yaw) * local,
                yaw: yaw + place.orientation.to_radians(),
                actions,
                index: 0,
                t: 0.0,
                done: false,
                serial: 0,
                repeat: false,
            })
        })
        .collect()
}

pub struct TribalVignettesPlugin;

impl Plugin for TribalVignettesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TribalVignettes>()
            .add_systems(OnEnter(GameStage::Tribal), reset_vignettes)
            .add_systems(OnExit(GameStage::Tribal), reset_vignettes)
            .add_systems(
                Update,
                (start_vignettes, drive_vignettes, rival_clips)
                    .chain()
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Tribal))
                    .run_if(resource_exists::<crate::Originals>),
            );
    }
}

fn reset_vignettes(mut vignettes: ResMut<TribalVignettes>) {
    let idle = vignettes.idle.take();
    let chats = vignettes.chats.take();
    let celebrations = std::mem::take(&mut vignettes.celebrations);
    *vignettes = TribalVignettes { idle, chats, celebrations, ..default() };
    vignettes.timers = [IDLE_SLOT_SECONDS * 0.5; 1 + crate::tribal::RIVAL_VILLAGES];
    vignettes.chat_timers = [IDLE_SLOT_SECONDS * 0.75; 1 + crate::tribal::RIVAL_VILLAGES];
    vignettes.rng = 0x1234_5679;
}

/// The vignette centre of a tribe and the yaw its frame faces (ADAPTED: the
/// camp fire, facing away from the hut).
pub fn tribe_centre(owner: VignetteOwner, campaign: &crate::tribal::TribalCampaign) -> (Vec3, f32) {
    match owner {
        VignetteOwner::Player => {
            let hut = crate::tribal::home_position();
            let fire = campaign
                .shacks
                .iter()
                .position(|k| *k == crate::tribal_tools::ToolKind::Campfire)
                .map(crate::tribal_tools::shack_position)
                .unwrap_or_else(|| crate::tribal::food_pile_position());
            let dir = (fire - hut).with_y(0.0);
            (fire, if dir.length_squared() > 1e-4 { crate::creature::facing_yaw(dir) } else { 0.0 })
        }
        VignetteOwner::Village(i) => {
            let hut = crate::tribal::village_position(i);
            let fire = hut + crate::tribal::hut_offset(i, 0);
            let dir = (fire - hut).with_y(0.0);
            (fire, if dir.length_squared() > 1e-4 { crate::creature::facing_yaw(dir) } else { 0.0 })
        }
    }
}

fn clip_seconds(originals: &crate::Originals) -> impl Fn(u32) -> Option<f32> + '_ {
    move |id| crate::creature_clips::clip(originals, id).map(|c| c.seconds)
}

/// Start the celebration with `TRG_Upgrade` and the idle tribe vignettes.
#[allow(clippy::too_many_arguments)]
fn start_vignettes(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    clock: Option<Res<crate::planet_sky::StageClock>>,
    layout: Option<Res<crate::tribal_planet::StageLayout>>,
    mut vignettes: ResMut<TribalVignettes>,
    chieftain: Query<(Entity, &Transform, Option<&Health>), With<Player>>,
    members: Query<(Entity, &Transform, &TribeUnit, &UnitTask, Option<&Health>), (With<TribeMate>, Without<Player>)>,
    rivals: Query<(Entity, &Transform, &RivalMember, Option<&Health>), Without<Raider>>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    let _layout = crate::tribal_planet::scope(layout.as_deref());
    let campaign = &run.tribal_campaign;
    let seconds = clip_seconds(&originals);
    // --- the celebration (0xCDA4F0, with TRG_Upgrade) -----------------------
    let now = cinematics.as_ref().and_then(|c| c.current().map(|(_, name, _)| name.to_string()));
    let started_upgrade = now.as_deref() == Some("TRG_Upgrade") && vignettes.last_cinematic.as_deref() != Some("TRG_Upgrade");
    vignettes.last_cinematic = now;
    if started_upgrade {
        let (centre, yaw) = tribe_centre(VignetteOwner::Player, campaign);
        let mut list: Vec<(Entity, f32)> = chieftain
            .iter()
            .filter(|(_, _, h)| !h.is_some_and(|h| h.dead()))
            .map(|(e, t, _)| (e, t.translation.with_y(0.0).distance(centre.with_y(0.0))))
            .filter(|(_, d)| *d < TRIBE_RADIUS)
            .collect();
        let mut rest: Vec<(Entity, f32)> = members
            .iter()
            .filter(|(_, _, u, task, h)| !h.is_some_and(|h| h.dead()) && !u.busy && matches!(task, UnitTask::Idle(_)))
            .map(|(e, t, ..)| (e, t.translation.with_y(0.0).distance(centre.with_y(0.0))))
            .filter(|(_, d)| *d < TRIBE_RADIUS)
            .collect();
        rest.sort_by(|a, b| a.1.total_cmp(&b.1));
        list.extend(rest);
        list.truncate(15);
        if let Some(id) = celebrate_vignette(list.len()) {
            let def = vignettes
                .celebrations
                .entry(id)
                .or_insert_with(|| load_vignette(&originals.resource_index, TRIBE_GROUP, id).map(Arc::new))
                .clone();
            if let Some(def) = def {
                let ents: Vec<Entity> = list.iter().map(|(e, _)| *e).collect();
                start(&mut commands, &mut vignettes, &def, VignetteOwner::Player, centre, yaw, &ents, &seconds);
            }
        }
    }
    // --- idle tribe vignettes (0xDB60C0 / 0xDAE230) --------------------------
    let dt = time.delta_secs();
    if vignettes.idle.is_none() {
        vignettes.idle = Some(Arc::new(load_group(&originals.resource_index, TRIBE_IDLE_GROUP)));
    }
    let defs = vignettes.idle.clone().unwrap_or_default();
    if defs.is_empty() {
        return;
    }
    if let Some(name) = vignettes.demo.take() {
        if let Some(def) = defs.iter().find(|d| d.name == name) {
            let (centre, yaw) = tribe_centre(VignetteOwner::Player, campaign);
            let ents: Vec<Entity> = members
                .iter()
                .filter(|(_, _, u, task, _)| u.carrying <= 0.0 && matches!(task, UnitTask::Idle(_)))
                .map(|(e, ..)| e)
                .collect();
            start(&mut commands, &mut vignettes, def, VignetteOwner::Player, centre, yaw, &ents, &seconds);
        }
    }
    let night = clock.as_ref().is_some_and(|c| crate::creature_ecology::is_night(c.hour));
    let mut owners = vec![VignetteOwner::Player];
    owners.extend((0..crate::tribal::RIVAL_VILLAGES).filter_map(|i| {
        campaign
            .village(i)
            .filter(|v| v.status != crate::tribal::RivalStatus::Conquered)
            .map(|_| VignetteOwner::Village(i))
    }));
    if vignettes.chats.is_none() {
        vignettes.chats = Some(Arc::new(load_group(&originals.resource_index, TRIBE_CHAT_GROUP)));
    }
    let chats = vignettes.chats.clone().unwrap_or_default();
    for owner in owners {
        let slot = owner.slot();
        vignettes.timers[slot] -= dt;
        vignettes.chat_timers[slot] -= dt;
        let tribe_due = vignettes.timers[slot] <= 0.0;
        let chat_due = vignettes.chat_timers[slot] <= 0.0;
        if !tribe_due && !chat_due {
            continue;
        }
        if tribe_due {
            vignettes.timers[slot] = IDLE_SLOT_SECONDS;
        }
        if chat_due {
            vignettes.chat_timers[slot] = IDLE_SLOT_SECONDS;
        }
        let (centre, yaw) = tribe_centre(owner, campaign);
        let near = |t: &Transform| t.translation.with_y(0.0).distance(centre.with_y(0.0));
        let free_members = |vignettes: &TribalVignettes| -> Vec<(Entity, f32, Vec3)> {
            let mut free: Vec<(Entity, f32, Vec3)> = match owner {
                VignetteOwner::Player => members
                    .iter()
                    .filter(|(e, _, u, task, h)| {
                        !h.is_some_and(|h| h.dead())
                            && !u.busy
                            && u.carrying <= 0.0
                            && matches!(task, UnitTask::Idle(_))
                            && !vignettes.busy(*e)
                    })
                    .map(|(e, t, ..)| (e, near(t), t.translation))
                    .collect(),
                VignetteOwner::Village(i) => rivals
                    .iter()
                    .filter(|(e, _, m, h)| {
                        m.village == i && matches!(m.mode, RivalMode::Wander) && !h.is_some_and(|h| h.dead()) && !vignettes.busy(*e)
                    })
                    .map(|(e, t, ..)| (e, near(t), t.translation))
                    .collect(),
            };
            free.retain(|(_, d, _)| *d < TRIBE_RADIUS);
            free.sort_by(|a, b| a.1.total_cmp(&b.1));
            free
        };
        let (attitude, tools) = match owner {
            VignetteOwner::Player => (
                None,
                campaign.shacks.iter().fold(0u32, |m, k| m | 1 << crate::tribal_rules::tool_type(*k)),
            ),
            VignetteOwner::Village(i) => {
                let relations = campaign.village(i).map_or(0.0, |v| v.relations);
                let tribe = crate::tribal_rules::village_tribe(campaign.lineup, i);
                (
                    Some(crate::tribal_plans::attitude(relations) as u8),
                    tribe.tools.iter().fold(1u32 << 10, |m, t| m | 1 << t),
                )
            }
        };
        // The tribe vignette (0xDB60C0 / 0xDAE230), one at a time.
        if tribe_due
            && !vignettes.running.iter().any(|v| v.owner == owner && v.chat_area.is_none())
            && vignettes.roll() <= IDLE_VIGNETTE_PERCENTAGE
        {
            let free = free_members(&vignettes);
            let candidates = idle_candidates(&defs, free.len(), night, attitude, tools, 50.0);
            if !candidates.is_empty() {
                let def = &defs[candidates[vignettes.pick(candidates.len())]];
                let ents: Vec<Entity> = free.iter().map(|(e, ..)| *e).collect();
                start(&mut commands, &mut vignettes, def, owner, centre, yaw, &ents, &seconds);
            }
        }
        // A pair chat at the free chat area nearest the first free member
        // (0xDB6610 / 0xDAD050, `Vignettes_Tribe_2~`, 20 s). ADAPTED: the same
        // cadence and chance as the tribe vignette.
        if chat_due && !chats.is_empty() && vignettes.roll() <= IDLE_VIGNETTE_PERCENTAGE {
            let free = free_members(&vignettes);
            let used: Vec<usize> = vignettes.running.iter().filter(|v| v.owner == owner).filter_map(|v| v.chat_area).collect();
            let area_at = |i: usize| {
                let (x, y) = CHAT_AREAS[i];
                centre + Quat::from_rotation_y(yaw) * crate::cinematics::camera::spore_to_bevy([x * CHAT_AREA_SCALE, y * CHAT_AREA_SCALE, 0.0])
            };
            let first = free.first().map(|f| f.2);
            let area = first.and_then(|p| {
                (0..CHAT_AREAS.len())
                    .filter(|i| !used.contains(i))
                    .min_by(|a, b| area_at(*a).distance_squared(p).total_cmp(&area_at(*b).distance_squared(p)))
            });
            if let Some(area) = area {
                let candidates = idle_candidates(&chats, free.len(), night, attitude, tools, 50.0);
                if !candidates.is_empty() {
                    let def = &chats[candidates[vignettes.pick(candidates.len())]];
                    let at = area_at(area);
                    let to_fire = (centre - at).with_y(0.0);
                    let face = if to_fire.length_squared() > 1e-4 { crate::creature::facing_yaw(to_fire) } else { yaw };
                    let ents: Vec<Entity> = free.iter().map(|(e, ..)| *e).collect();
                    start_at(&mut commands, &mut vignettes, def, owner, Some(area), at, face, &ents, &seconds);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start(
    commands: &mut Commands,
    vignettes: &mut TribalVignettes,
    def: &VignetteDef,
    owner: VignetteOwner,
    centre: Vec3,
    yaw: f32,
    members: &[Entity],
    seconds: &dyn Fn(u32) -> Option<f32>,
) {
    start_at(commands, vignettes, def, owner, None, centre, yaw, members, seconds);
}

#[allow(clippy::too_many_arguments)]
fn start_at(
    commands: &mut Commands,
    vignettes: &mut TribalVignettes,
    def: &VignetteDef,
    owner: VignetteOwner,
    chat_area: Option<usize>,
    centre: Vec3,
    yaw: f32,
    members: &[Entity],
    seconds: &dyn Fn(u32) -> Option<f32>,
) {
    let mut actors = place_actors(def, centre, yaw, members, seconds);
    if actors.is_empty() {
        return;
    }
    for a in &mut actors {
        a.repeat = chat_area.is_some();
    }
    // A new vignette of the tribe replaces its running one (0xCDA4F0 ends the
    // one at tribe+0x26C); a chat replaces the one at its area.
    let old: Vec<usize> = vignettes
        .running
        .iter()
        .enumerate()
        .filter(|(_, v)| v.owner == owner && v.chat_area == chat_area)
        .map(|(i, _)| i)
        .collect();
    for i in old.into_iter().rev() {
        let v = vignettes.running.remove(i);
        stop_effects(commands, &v);
    }
    let effects = def
        .effects
        .iter()
        .map(|(effect, offset)| {
            let local = crate::cinematics::camera::spore_to_bevy([offset.x, offset.y, 0.0]);
            let mut fx = SwarmEffect::new(*effect);
            fx.position = centre + Quat::from_rotation_y(yaw) * local;
            fx.rotation = Quat::from_rotation_y(yaw);
            fx.despawn_when_done = true;
            commands.spawn((fx, StageOwned, Name::new(format!("vignette effect {}", def.name)))).id()
        })
        .collect();
    info!("tribal vignette {} ({:?}): {} of {} places", def.name, owner, actors.len(), def.places.len());
    vignettes.started.push((def.name.clone(), owner));
    vignettes.running.push(RunningVignette {
        id: def.id,
        name: def.name.clone(),
        owner,
        chat_area,
        lifetime: chat_area.map(|_| CHAT_SECONDS),
        centre,
        yaw,
        actors,
        effects,
    });
}

fn stop_effects(commands: &mut Commands, v: &RunningVignette) {
    for &e in &v.effects {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.insert(VignetteEffectEnd);
        }
    }
}

/// Marks a vignette's general effect whose vignette has ended.
#[derive(Component)]
pub struct VignetteEffectEnd;

/// Advance the running vignettes, hold their members at their places and
/// release them when their actions end or their task changes.
#[allow(clippy::too_many_arguments)]
fn drive_vignettes(
    mut commands: Commands,
    time: Res<Time>,
    mut vignettes: ResMut<TribalVignettes>,
    mut members: Query<(&mut Transform, Option<&mut UnitTask>, Option<&TribeUnit>, Option<&mut RivalMember>, Option<&Health>, Has<Raider>)>,
    mut effects: Query<&mut SwarmEffect, With<VignetteEffectEnd>>,
) {
    for mut fx in &mut effects {
        fx.stop = true;
    }
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let mut ended: Vec<usize> = Vec::new();
    for (vi, v) in vignettes.running.iter_mut().enumerate() {
        // A chat ends with its lifetime (0xDAD050, 20 s).
        if let Some(left) = v.lifetime.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                for a in v.actors.iter_mut().filter(|a| !a.done) {
                    a.done = true;
                    if let Ok((_, _, _, Some(mut m), _, _)) = members.get_mut(a.entity) {
                        m.wait = m.wait.min(1.0);
                        commands.entity(a.entity).remove::<PoseClip>();
                    }
                }
            }
        }
        for a in v.actors.iter_mut() {
            if a.done {
                continue;
            }
            let Ok((mut t, task, unit, rival, health, raider)) = members.get_mut(a.entity) else {
                a.done = true;
                continue;
            };
            // The member leaves when it gets a task, dies or raids.
            let free = !health.is_some_and(|h| h.dead())
                && !raider
                && match (task.as_deref(), unit) {
                    (Some(task), Some(unit)) => matches!(task, UnitTask::Idle(_)) && unit.carrying <= 0.0,
                    _ => rival.as_ref().is_none_or(|m| matches!(m.mode, RivalMode::Wander)),
                };
            if !free || !a.step(dt) {
                a.done = true;
                if let Some(mut m) = rival {
                    m.wait = m.wait.min(1.0);
                    commands.entity(a.entity).remove::<PoseClip>();
                }
                continue;
            }
            // Held at the place (the stage's own controls run before).
            t.translation.x = a.place.x;
            t.translation.z = a.place.z;
            t.rotation = Quat::from_rotation_y(a.yaw);
            if let Some(mut task) = task {
                if matches!(*task, UnitTask::Idle(_)) {
                    let offset = unit.map_or(Vec2::ZERO, |u| u.offset);
                    *task = UnitTask::Idle(a.place - Vec3::new(offset.x, 0.0, offset.y));
                }
            }
            if let Some(mut m) = rival {
                m.target = a.place;
                m.wait = m.wait.max(2.0);
                if let Some((clip, _)) = a.current() {
                    commands.entity(a.entity).insert(PoseClip { action: Some(clip), idle: FALLBACK_IDLE, serial: a.serial.wrapping_add(1) });
                }
            }
        }
        if v.actors.iter().all(|a| a.done) {
            ended.push(vi);
        }
    }
    for i in ended.into_iter().rev() {
        let v = vignettes.running.remove(i);
        stop_effects(&mut commands, &v);
    }
}

// ---------------------------------------------------------------------------
// Rival villagers' and raiders' tribe clips
// ---------------------------------------------------------------------------

/// What a rival villager plays: `main` (looped) and, every `refresh`
/// seconds, one of `attacks`; `intro` once when the plan starts. `None`: the
/// species' own locomotion (walk / run by speed) or rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RivalPlan {
    pub main: u32,
    pub intro: Option<u32>,
    pub attacks: &'static [u32],
    pub refresh: f32,
}

impl RivalPlan {
    fn looped(main: u32) -> Self {
        Self { main, intro: None, attacks: &[], refresh: 0.0 }
    }
}

/// The tribe clips of a rival member (the citizen table of T9K applied to the
/// NPC tribes' states; the raid states per 0xDBAAF0 / 0xDBC770 / 0xCE1690).
pub fn rival_plan(mode: &RivalMode, raider: Option<&Raider>, moving: bool, ctx: &PlanCtx) -> Option<RivalPlan> {
    let tool = raider.and_then(|r| r.tool);
    let kind = crate::outfit_crowd::citizen_tool_type(tool, false);
    let attack_tool = tool.is_some_and(|t| crate::tribal_tools::info(t).class == crate::tribal_tools::ToolClass::Attack);
    let refresh = crate::tribal_rules::weapon_of(tool).refresh;
    let fight = RivalPlan { main: tribe_clip::ATTACK_IDLE, intro: None, attacks: attack_clips(kind), refresh };
    let attack_walk = if attack_tool && tool_anims(kind).en_route != 0 { tool_anims(kind).en_route } else { tribe_clip::BATTLE_WALK };
    if let Some(r) = raider {
        use crate::tribal_plans::RaidKind;
        return match (r.phase, moving) {
            // 0xDBC770: sneaking to the stock; the stolen load is carried home on the
            // default locomotion with `platter_grasper_overlay` over the arms
            // (VERIFIED for a bundle carrier, T9K-1: `rival_overlay`).
            (RaidPhase::Approach, true) if r.kind == RaidKind::StealFood => Some(RivalPlan::looped(tribe_clip::RAID_SNEAK)),
            // 0xDB7120: the raid party's attack walk.
            (RaidPhase::Approach | RaidPhase::Fight, true) => Some(RivalPlan::looped(attack_walk)),
            (_, true) => None,
            (RaidPhase::Congregate, false) => Some(RivalPlan::looped(RAID_CONGREGATE)),
            (RaidPhase::Pillage, false) => Some(RivalPlan::looped(PICKUP_FOOD)),
            // 0xDBAAF0: battle-ready loop, the yell, then the tool's building
            // clip (or its attack) on the target.
            (RaidPhase::Raze, false) => Some(RivalPlan {
                main: BATTLE_READY,
                intro: Some(tribe_clip::BATTLE_YELL),
                attacks: building_clips(kind).unwrap_or_else(|| attack_clips(kind)),
                refresh,
            }),
            (RaidPhase::Fight, false) => Some(fight),
            _ => Some(RivalPlan::looped(idle_clip(kind, false, ctx))),
        };
    }
    match (mode, moving) {
        (RivalMode::Defend(_), true) => Some(RivalPlan::looped(tribe_clip::BATTLE_WALK)),
        (RivalMode::Defend(_), false) => Some(fight),
        (_, true) => None,
        _ => Some(RivalPlan::looped(idle_clip(kind, false, ctx))),
    }
}

/// The clip laid over a rival's own walk (`PoseOverlay`): a raider walking home with
/// the stolen food carries it like a bundle carrier of the tribe, the default
/// locomotion with `platter_grasper_overlay` (0x05F08F08) over its arms (T9K-1, the
/// bundle grasper's overlay 0xC23460); `walk_carry` is never played.
pub fn rival_overlay(raider: Option<&Raider>, moving: bool) -> Option<u32> {
    use crate::tribal_plans::RaidKind;
    let r = raider?;
    (moving && r.phase == RaidPhase::Retreat && r.kind == RaidKind::StealFood).then_some(clip_id::PLATTER_OVERLAY)
}

/// Per rival member: where it was, its plan and its attack timer.
#[derive(Component, Default)]
pub struct RivalClipState {
    last: Option<Vec3>,
    speed: f32,
    plan: Option<RivalPlan>,
    timer: f32,
    serial: u32,
    rng: u32,
}

/// Drive the rival members' tribe clips through `PoseClip`.
fn rival_clips(
    mut commands: Commands,
    time: Res<Time>,
    vignettes: Res<TribalVignettes>,
    mut rivals: Query<(
        Entity,
        &Transform,
        &RivalMember,
        Option<&Raider>,
        Option<&mut RivalClipState>,
        Option<&Health>,
        Option<&crate::locomotion_preview::PoseOverlay>,
    )>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let clock = time.elapsed_secs();
    for (entity, t, member, raider, state, health, overlay) in &mut rivals {
        if vignettes.busy(entity) {
            continue;
        }
        let Some(mut s) = state else {
            commands.entity(entity).insert(RivalClipState { rng: entity.to_bits() as u32 | 1, ..default() });
            continue;
        };
        if health.is_some_and(|h| h.dead()) {
            continue;
        }
        let p = t.translation.with_y(0.0);
        let step = s.last.map_or(0.0, |l| l.distance(p));
        s.last = Some(p);
        if dt > 0.0 && step < 3.0 {
            s.speed += (step / dt - s.speed) * (1.0 - (-8.0 * dt).exp());
        }
        let moving = s.speed > 0.4 * t.scale.x.abs().max(0.2);
        let ctx = PlanCtx { seed: entity.to_bits() as u32, clock, ..PlanCtx::default() };
        match (rival_overlay(raider, moving), overlay) {
            (Some(id), cur) if cur.map(|o| o.0) != Some(id) => {
                commands.entity(entity).insert(crate::locomotion_preview::PoseOverlay(id));
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<crate::locomotion_preview::PoseOverlay>();
            }
            _ => {}
        }
        let plan = rival_plan(&member.mode, raider, moving, &ctx);
        if plan != s.plan {
            s.plan = plan;
            s.serial = s.serial.wrapping_add(1);
            s.timer = 0.0;
            match plan {
                Some(p) => {
                    commands.entity(entity).insert(PoseClip { action: p.intro, idle: p.main, serial: s.serial });
                }
                None => {
                    commands.entity(entity).remove::<PoseClip>();
                }
            }
            continue;
        }
        let Some(p) = plan else { continue };
        if p.attacks.is_empty() || p.refresh <= 0.0 {
            continue;
        }
        s.timer -= dt;
        if s.timer <= 0.0 {
            s.timer = p.refresh;
            s.rng = s.rng.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F);
            let pick = p.attacks[((s.rng >> 8) as usize) % p.attacks.len()];
            s.serial = s.serial.wrapping_add(1);
            commands.entity(entity).insert(PoseClip { action: Some(pick), idle: p.main, serial: s.serial });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prop::{Property, PropertyList};

    fn be(v: &[u32]) -> Property {
        Property { kind: 10, array: true, values: v.iter().map(|x| x.to_be_bytes().to_vec()).collect() }
    }

    fn vec2s(v: &[(f32, f32)]) -> Property {
        Property {
            kind: 0x30,
            array: true,
            values: v.iter().map(|(x, y)| [x.to_le_bytes(), y.to_le_bytes()].concat()).collect(),
        }
    }

    #[test]
    fn vignette_props_decode_with_the_loader_defaults() {
        use prop_ids::*;
        // The installed trg_build: two places, the mallet (item 19) in the
        // first action, an empty hand in the second.
        let mut list = PropertyList::default();
        list.0.insert(ANIMATIONS, be(&[0x05C8_052B, 0x05C8_2EFA]));
        list.0.insert(ANIMATIONS + 1, be(&[0x05C8_1767, 0x05C8_2DBD]));
        list.0.insert(EFFECTS, be(&[19, 0]));
        list.0.insert(EFFECTS + 1, be(&[19, 0]));
        list.0.insert(EFFECT_LIMBS, be(&[11]));
        list.0.insert(SEQUENCES, be(&[0, 1]));
        list.0.insert(OFFSETS, vec2s(&[(-12.0, -1.75), (-13.0, 1.5)]));
        list.0.insert(ORIENTATIONS, Property { kind: 13, array: true, values: vec![45f32.to_be_bytes().to_vec(), 120f32.to_be_bytes().to_vec()] });
        list.0.insert(TIME_OF_DAY, Property { kind: 10, array: false, values: vec![2u32.to_be_bytes().to_vec()] });
        list.0.insert(REQUIRED_TOOLS, be(&[1, 3]));
        let d = parse_vignette(0x345E_CB37, &list).unwrap();
        assert_eq!(d.places.len(), 2);
        assert_eq!(d.places[1].offset, Vec2::new(-13.0, 1.5));
        assert_eq!(d.places[0].orientation, 45.0);
        assert_eq!(d.places[0].actions[0].item, Some(19));
        assert_eq!(d.places[0].actions[1].item, Some(0));
        assert_eq!(d.places[0].actions[0].limb, 11);
        assert_eq!(d.places[0].actions[1].limb, 0, "limbs default to 0");
        assert_eq!(d.places[0].actions[1].loops, 0);
        assert_eq!(d.attitude, -1);
        assert_eq!(d.time_of_day, 2);
        assert_eq!(d.required_tools, 0b1010);
        assert_eq!(d.happy, (0.0, 100.0));
        assert_eq!(handheld_effect(19), Some(0xD72F_5830), "repair mallet");
        assert_eq!(handheld_effect(1), Some(0xDBFD_6844), "torch");
        assert_eq!(handheld_effect(0), None);
        // Without sequences, actor i plays sequence i.
        list.0.remove(&SEQUENCES);
        assert_eq!(parse_vignette(1, &list).unwrap().places.len(), 2);
    }

    fn def(places: usize, time: u32, attitude: i32, tools: u32) -> VignetteDef {
        VignetteDef {
            id: 0,
            name: String::new(),
            places: (0..places)
                .map(|_| VignettePlace { offset: Vec2::ZERO, orientation: 0.0, actions: vec![VignetteAction { clip: 1, loops: 0, item: None, limb: 0 }] })
                .collect(),
            effects: vec![],
            unscaled: false,
            time_of_day: time,
            attitude,
            required_tools: tools,
            happy: (0.0, 100.0),
        }
    }

    #[test]
    fn idle_vignettes_filter_like_the_behaviour_evaluator() {
        let defs = vec![def(3, 2, 4, 0), def(1, 0, -1, 0), def(3, 0, 0, 1 << 1), def(5, 0, -1, 0)];
        // Night, a Friendly tribe of three with torches: the dance and the
        // single; the torch vignette needs a Furious attitude; five places
        // are too many.
        assert_eq!(idle_candidates(&defs, 3, true, Some(4), 1 << 1, 50.0), vec![0, 1]);
        // Daytime: the night vignette drops out.
        assert_eq!(idle_candidates(&defs, 3, false, Some(4), 1 << 1, 50.0), vec![1]);
        // Furious with torches: the torch vignette.
        assert_eq!(idle_candidates(&defs, 3, false, Some(0), 1 << 1, 50.0), vec![1, 2]);
        // The player's own tribe takes any attitude.
        assert_eq!(idle_candidates(&defs, 5, true, None, 1 << 1, 50.0), vec![0, 1, 2, 3]);
        assert_eq!(celebrate_vignette(1), None);
        assert_eq!(celebrate_vignette(5), Some(0x0E27_71CE));
        assert_eq!(celebrate_vignette(15), Some(0x0E8E_6141));
        assert_eq!(crate::world_objects::name_id("celebrate_05guys"), 0x0E27_71CE);
        assert_eq!(crate::world_objects::name_id("celebrate_09guys"), 0x90BB_C732);
    }

    #[test]
    fn vignette_actors_play_their_actions_once_and_leave() {
        let mut d = def(2, 0, -1, 0);
        d.places[0].actions = vec![
            VignetteAction { clip: 10, loops: 2, item: Some(19), limb: 11 },
            VignetteAction { clip: 11, loops: 0, item: Some(0), limb: 0 },
        ];
        d.places[1].offset = Vec2::new(0.0, 2.0);
        let mut world = World::new();
        let e = [world.spawn_empty().id(), world.spawn_empty().id()];
        let mut actors = place_actors(&d, Vec3::new(5.0, 0.0, 5.0), 0.0, &e, &|_| Some(1.0));
        assert_eq!(actors.len(), 2);
        // Spore y forward = Bevy -z.
        assert!((actors[1].place - Vec3::new(5.0, 0.0, 3.0)).length() < 1e-5, "{}", actors[1].place);
        let a = &mut actors[0];
        assert_eq!(a.current(), Some((10, 0.0)));
        assert_eq!(a.item(), Some(19));
        assert!(a.step(1.2));
        assert_eq!(a.current().unwrap().0, 10, "vi_loops 2 plays it twice");
        assert!(a.step(1.0));
        assert_eq!(a.current().unwrap().0, 11);
        assert_eq!(a.item(), Some(0), "the empty hand of the second action");
        assert!(!a.step(1.0));
        assert!(a.done && a.current().is_none());
    }

    #[test]
    fn rival_villagers_play_the_tribe_clips() {
        let ctx = PlanCtx::default();
        // Standing at home: an idle of the tribe table; walking: locomotion.
        let idle = rival_plan(&RivalMode::Wander, None, false, &ctx).unwrap();
        assert!(tribe_clip::IDLES.contains(&idle.main) || idle.main == tribe_clip::IDLE_LOOKAROUND);
        assert_eq!(rival_plan(&RivalMode::Wander, None, true, &ctx), None);
        // A food thief sneaks in and carries the load home.
        let mut r = Raider { kind: crate::tribal_plans::RaidKind::StealFood, ..Default::default() };
        assert_eq!(rival_plan(&RivalMode::Raid, Some(&r), true, &ctx).unwrap().main, tribe_clip::RAID_SNEAK);
        r.phase = RaidPhase::Congregate;
        assert_eq!(rival_plan(&RivalMode::Raid, Some(&r), false, &ctx).unwrap().main, RAID_CONGREGATE);
        r.phase = RaidPhase::Retreat;
        // The load goes home on the creature's own walk with the platter overlay.
        assert_eq!(rival_plan(&RivalMode::Raid, Some(&r), true, &ctx), None);
        assert_eq!(rival_overlay(Some(&r), true), Some(clip_id::PLATTER_OVERLAY));
        assert_eq!(rival_overlay(Some(&r), false), None, "standing, nothing is laid over the idle");
        r.phase = RaidPhase::Approach;
        assert_eq!(rival_overlay(Some(&r), true), None, "an empty-handed sneak has no overlay");
        r.phase = RaidPhase::Retreat;
        // A raid party member with an axe: the axe's attack walk, then battle
        // ready + yell and the axe's hut clip at the target.
        let mut party = Raider {
            kind: crate::tribal_plans::RaidKind::Raid,
            tool: Some(crate::tribal_tools::ToolKind::StoneAxes),
            ..Default::default()
        };
        assert_eq!(rival_plan(&RivalMode::Raid, Some(&party), true, &ctx).unwrap().main, 0x062E_E781);
        party.phase = RaidPhase::Raze;
        let raze = rival_plan(&RivalMode::Raid, Some(&party), false, &ctx).unwrap();
        assert_eq!((raze.main, raze.intro), (BATTLE_READY, Some(tribe_clip::BATTLE_YELL)));
        assert_eq!(raze.attacks, &[0x0601_FB4F]);
        assert_eq!(raze.refresh, 2.0, "the axe's refresh");
        party.phase = RaidPhase::Fight;
        assert_eq!(rival_plan(&RivalMode::Raid, Some(&party), false, &ctx).unwrap().attacks, &[clip_id::AXE]);
        // A defender fights bare-handed (rival tools are not simulated).
        let d = rival_plan(&RivalMode::Defend(Entity::PLACEHOLDER), None, false, &ctx).unwrap();
        assert_eq!(d.attacks, &[clip_id::NO_TOOL_ATTACK]);
    }

    /// The installed vignettes decode: every Vignettes_Tribe_1~ entry has
    /// places with readable clips, and the celebrations have one place per
    /// celebrant.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_tribe_vignettes_decode() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let idle = load_group(&index, TRIBE_IDLE_GROUP);
        assert!(idle.len() >= 20, "{} idle vignettes", idle.len());
        for d in &idle {
            assert!(!d.places.is_empty() && d.places.iter().all(|p| !p.actions.is_empty()), "{}", d.name);
            println!("{} places {} tod {} att {} tools {:b} fx {:?}", d.name, d.places.len(), d.time_of_day, d.attitude, d.required_tools, d.effects);
        }
        let fire = idle.iter().find(|d| d.name == "fire_dance").unwrap();
        assert_eq!(fire.time_of_day, 2);
        assert_eq!(fire.effects.len(), 1);
        let torch = idle.iter().find(|d| d.name == "foe_torch_01_03tm").unwrap();
        assert_eq!(torch.attitude, 0);
        assert_eq!(torch.required_tools, 1 << 1);
        assert!(torch.places.iter().all(|p| p.actions[0].item == Some(1)), "torches in hand");
        for n in 2..=15 {
            let d = load_vignette(&index, TRIBE_GROUP, celebrate_vignette(n).unwrap()).unwrap();
            assert!(d.places.len() >= n, "celebrate_{n:02}guys has {} places", d.places.len());
        }
    }
}
