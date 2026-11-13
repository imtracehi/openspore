//! Tribal stage audio beyond the score and the diplomacy observers of
//! `game_audio` (notes\tribal-re-notes.md T7).
//!
//! NATIVE (installed data and the call sites of `cTribeInputStrategy`):
//! * One sound per citizen action (`sfx_trg_action_*`, the table of `PlayAudio`
//!   calls at 0xCCFEBA-0xCD003A): played once per command, whatever the number of
//!   members that take it (`CitizenAction::sound`).
//! * `sfx_trg_mode_social` / `sfx_trg_mode_attack` when the stance changes
//!   (the HUD's stance buttons, click proc 0xCDBEA0), `ui_trg_buy_baby` when a
//!   baby is bought, `ui_camera_zoom` when a HUD camera button goes down
//!   (0xCC9B30), `mus_trg_campfire_powerup` when the firepit upgrades with the
//!   tribe level (the call at 0xD79E7C), `mus_trg_death_sting` when a member
//!   dies. `music_trg_to_civ_start` and `music_trg_to_civ_end` are the pair of
//!   stings of the Tribal -> Civilization transition: the director plays the
//!   first at the stage change and the second when it is over
//!   (`game_audio::intro` / `intro_follow`).
//! * A goal card completes (`tribal_hud::tutorial`, the retail `CompleteGoal`
//!   0xB708A0): `ui_mission_complete` plays and the Pd parameter
//!   `missioncomplete` is set to 1 (see [`goal_sounds`]).
//! * The positional raid and gift music (`music_raid`, `music_raidfood`, `music_gifting`:
//!   looping 3D sounds that follow their command group) and the Pd input
//!   `distance_to_travel_music` (the camera's distance to the nearest of them, which
//!   ducks `tribe_drums3`): the Tribe manager's 23-slot table (0xBC23B0 / 0xBC2540 /
//!   0xBC22F0, chooser 0xC238B0) in [`MusicSlots`], [`wanted_sources`] and
//!   [`feed_raid_music`].
//! * `amb_trg_highalt_wind`: the soundProp's `curve1` is silent below an
//!   altitude of 500, 0.2 at 550 and 0.35 at 580; the sample is the shoreline
//!   wind. Played as one looping voice whose volume follows the camera.
//!
//! The wind curve reads the camera's height over its look point
//! (`TribalRig::altitude`, tribal_controls: the stand-in for the retail
//! `height_above_water` 0xB13B90); the retail zoom ladder tops out at 90 units, so
//! the curve (silent below 500) never rises in the Tribal stage, in retail or here
//! (tribal-audit-b: the old zoom-fraction mapping is only kept by
//! [`retail_altitude`] for its tests). Still ADAPTED: `music_tribe_social` (with `tribe_drums3` and the planet ambience) is run
//! by the Pd engine (`pd_audio::feeds`); `music_tribe_editor` is a Pd patch
//! not started yet. The wind stays here: its soundProp curve needs the
//! camera altitude (`pd_audio::CURVE_DRIVEN_BEDS`).

use crate::{
    cell_audio::{NamedSounds, Play},
    game::RunProgress,
    game_audio::{Bus, Cue, GameAudio, PlayCue},
    pd::engine::VoiceId,
    pd_audio::PdMusic,
    stages::{GameStage, StageOwned},
    tribal,
    tribal_controls::{CitizenAction, TribalCamera, TribalControls},
    tribal_plans::{GiftBearer, GiftPhase},
    tribal_rts::{RivalMember, TribeUnit, UnitTask},
};
use std::collections::HashMap;
use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    diagnostic::FrameCount,
    prelude::*,
};

/// The soundProp `curve1` of `amb_trg_highalt_wind`: (altitude, volume).
pub const WIND_CURVE: [(f32, f32); 4] = [(0.0, 0.0), (500.0, 0.0), (550.0, 0.2), (580.0, 0.35)];
/// The altitude range of the earlier zoom-fraction mapping ([`retail_altitude`],
/// tests only; the wind itself reads `TribalRig::altitude`).
pub const RETAIL_ELEVATION_NEAR: f32 = 20.0;
pub const RETAIL_ELEVATION_FAR: f32 = 600.0;

/// Pure: the wind volume at a retail altitude (linear between the curve's
/// points, flat beyond the last).
pub fn wind_gain(altitude: f32) -> f32 {
    let (mut px, mut py) = WIND_CURVE[0];
    if altitude <= px {
        return py;
    }
    for (x, y) in WIND_CURVE.iter().skip(1) {
        if altitude <= *x {
            let t = if *x > px { (altitude - px) / (*x - px) } else { 1.0 };
            return py + (y - py) * t;
        }
        px = *x;
        py = *y;
    }
    py
}

/// Pure: a zoom fraction (0 close, 1 far) as a retail altitude (the stretch
/// the earlier free zoom range used; the retail ladder is discrete, see
/// `tribal_controls::TribalRig::altitude`).
#[allow(dead_code)]
pub fn retail_altitude(zoom_fraction: f32) -> f32 {
    RETAIL_ELEVATION_NEAR
        + zoom_fraction.clamp(0.0, 1.0) * (RETAIL_ELEVATION_FAR - RETAIL_ELEVATION_NEAR)
}

/// The cue of a citizen action's sound (none for the silent actions).
pub fn action_cue(action: CitizenAction) -> Option<Cue> {
    Some(match action {
        CitizenAction::Eat => Cue::TribalActionEat,
        CitizenAction::Fish => Cue::TribalActionFish,
        CitizenAction::Gather => Cue::TribalGather,
        CitizenAction::Heal => Cue::TribalHeal,
        CitizenAction::Attack => Cue::TribalActionAttack,
        CitizenAction::RaidFood | CitizenAction::RaidHuts => Cue::TribalActionRaid,
        CitizenAction::GiftFood | CitizenAction::GiftMember => Cue::TribalGift,
        CitizenAction::Hunt => Cue::TribalActionHunt,
        CitizenAction::Mate => Cue::TribalActionMate,
        CitizenAction::Party => Cue::TribalPerformGood,
        CitizenAction::CollectEgg => Cue::TribalActionCollectEgg,
        CitizenAction::Bundle => Cue::TribalActionBundle,
        CitizenAction::FeedWild => Cue::TribalActionFeedWild,
        CitizenAction::Repair => Cue::TribalActionRepair,
        CitizenAction::Tame => Cue::TribalActionTame,
        CitizenAction::Recruit => Cue::TribalActionRecruit,
        CitizenAction::GatherMeat => Cue::TribalActionGatherMeat,
        CitizenAction::GrabTool
        | CitizenAction::WalkTo
        | CitizenAction::Sleep
        | CitizenAction::ReturnToTribe => return None,
    })
}

/// One sound per command given this frame.
fn command_sounds(controls: Res<TribalControls>, mut out: MessageWriter<PlayCue>) {
    for issued in &controls.issued {
        if let Some(cue) = action_cue(issued.action) {
            out.write(PlayCue(cue));
        }
    }
}

/// What the observers compare between frames.
#[derive(Clone, Default)]
struct Snapshot {
    social: bool,
    births: usize,
    members: usize,
    livestock: usize,
    hut_level: usize,
    hud_camera: bool,
}

/// Stance, purchase, death, upgrade, tame and the HUD camera buttons, seen as
/// changes of the state between frames.
#[allow(clippy::too_many_arguments)]
fn observe(
    frame: Res<FrameCount>,
    run: Res<RunProgress>,
    controls: Res<TribalControls>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<Snapshot>>,
    mut out: MessageWriter<PlayCue>,
) {
    let campaign = &run.tribal_campaign;
    let now = Snapshot {
        social: campaign.friendly_stance,
        births: campaign.births.len(),
        members: run.tribe.len(),
        livestock: campaign.livestock.len(),
        hut_level: tribal::hut_level(campaign.resolved_count()),
        hud_camera: controls.hud.any(),
    };
    // A gap in frames means the stage was left and re-entered: re-baseline.
    let continuous = ran.is_some_and(|f| frame.0 == f.wrapping_add(1));
    *ran = Some(frame.0);
    let previous = last.replace(now.clone());
    let (Some(prev), true) = (previous, continuous) else {
        return;
    };
    let mut play = |cue| {
        out.write(PlayCue(cue));
    };
    if now.social != prev.social {
        play(if now.social {
            Cue::TribalModeSocial
        } else {
            Cue::TribalModeAttack
        });
    }
    if now.births > prev.births {
        // The buy-a-baby button: the egg is paid for.
        play(Cue::TribalRecruit);
    }
    if now.members < prev.members {
        play(Cue::TribalDeathSting);
    }
    if now.livestock > prev.livestock {
        play(Cue::TribalActionTame);
    }
    if now.hut_level > prev.hut_level {
        play(Cue::TribalCampfire);
    }
    if now.hud_camera && !prev.hud_camera {
        play(Cue::CameraZoom);
    }
}


/// `ui_mission_complete` (id 0x0C871D6E, `Audio::PlayAudio` at 0xC2E76A).
pub const MISSION_SOUND: &str = "ui_mission_complete";

/// Goal cards completed in the Tribal stage since the app started: the Pd feed
/// sets `missioncomplete` to 1 for every step (`pd_audio::feeds::feed_tribe`).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalPulse(pub u32);

/// Pure: goal cards completed between two looks at the tutorial's `done` list
/// (the list only grows; a smaller count is a different save: nothing).
pub fn goals_completed(before: usize, now: usize) -> u32 {
    now.saturating_sub(before) as u32
}

/// A goal card completes: the retail mission-completion routine 0xC2E700 (one
/// function for the Creature / Tribe / Civ missions, picked by the game mode:
/// Tribe -> the manager 0xCD35C0) sets `missioncomplete` to 1.0 (0xC2E745) and
/// plays `ui_mission_complete` (0xC2E76A). The Tribal goal cards are the
/// tutorial missions `AddGoal` (0xB707A0) creates (noun 0x34066DBA) and
/// `CompleteGoal` (0xB708A0) ends, so every `TutorialGoal` that moves to `done`
/// (retail triggers, `tribal_hud::tutorial`) is one completion. The link from
/// `CompleteGoal` to 0xC2E700 goes through the mission class vtables (0xC2E700 is
/// in slot 35 of the three tutorial mission classes at 0x146B078 / 0x146B190 /
/// 0x146B2B8) and was not followed call by call: INFERRED. The per-village
/// `Conquer` / `Ally` cards of `tribal_hud::guide` are OpenSpore's own (ADAPTED)
/// and do not count.
#[allow(clippy::too_many_arguments)]
fn goal_sounds(
    frame: Res<FrameCount>,
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut pulse: ResMut<GoalPulse>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<usize>>,
    mut asked: Local<bool>,
) {
    if !*asked {
        // Decode the jingle before the first goal can complete.
        *asked = true;
        sounds.request(crate::world_objects::name_id(MISSION_SOUND), Some(MISSION_SOUND), false);
    }
    let done = run.tribal_campaign.tutorial.done.len();
    // A gap in frames means the stage was left and re-entered (a loaded save
    // may hold more completed goals): re-baseline.
    let continuous = ran.is_some_and(|f| frame.0 == f.wrapping_add(1));
    *ran = Some(frame.0);
    let before = last.replace(done);
    let (Some(before), true) = (before, continuous) else {
        return;
    };
    let n = goals_completed(before, done);
    if n > 0 {
        let mut play = Play::named(MISSION_SOUND, "tribal goal");
        play.bus = Bus::Ui;
        sounds.play(play, time.elapsed_secs_f64(), 0.0);
        pulse.0 += n;
    }
}

// ---------------------------------------------------------------------------
// The positional raid / gift music (tribal-raidmusic)
// ---------------------------------------------------------------------------

/// The three sounds the music chooser 0xC238B0 returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PositionalMusic {
    /// `music_raid` (0x0AD1A17D, 0xC239B0 / 0xC239C3): Attack in its target's zone, RaidHuts.
    Raid,
    /// `music_raidfood` (0x460FE7C5, 0xC239D7): RaidFood.
    RaidFood,
    /// `music_gifting` (0xF72DD75D, 0xC239CD): GiftFood, GiftMember.
    Gifting,
}

impl PositionalMusic {
    pub const fn sound(self) -> &'static str {
        match self {
            PositionalMusic::Raid => "music_raid",
            PositionalMusic::RaidFood => "music_raidfood",
            PositionalMusic::Gifting => "music_gifting",
        }
    }

    /// The soundProp id the code image pushes.
    pub const fn id(self) -> u32 {
        match self {
            PositionalMusic::Raid => 0x0AD1_A17D,
            PositionalMusic::RaidFood => 0x460F_E7C5,
            PositionalMusic::Gifting => 0xF72D_D75D,
        }
    }
}

/// `cTribe` +0x330, the radius the Attack branch of the chooser compares with
/// (virtual slot 0x60 = 0xC8E980). The constructor sets 30 (0xC98593 from the
/// constant 0x13F0E68), but `SpawnTribe` (slot 0x8C = 0xC9A290, called for the
/// player's tribe 0xC99FD5 and every NPC tribe 0xC93366) stores the global 0.0
/// 0x1581268 over it (0xC9A2B6) and no other instruction writes the field: a
/// tribe of a running game has radius 0, so the test below never passes and an
/// Attack goal plays no music. INFERRED (0x1581268 is a zero vector of `.data`
/// with no writer in the image, read as a constant); the 30 stays in
/// [`CTOR_ZONING_RADIUS`] for the day this is disproved.
pub const ATTACK_ZONING_RADIUS: f32 = 0.0;
/// The constructor's default of the same field (0xC98593).
pub const CTOR_ZONING_RADIUS: f32 = 30.0;

/// The Attack branch of the chooser (0xC23904..0xC239AC): the citizen's target must
/// be a citizen with an owner tribe (`[goal target]` cast to `cCreatureCitizen`
/// 0xAE3310, tribe at +0x1014), and the squared distance between the target and its
/// tribe's position must be below the square of the tribe's radius (`comiss`
/// 0xC239A9: r^2 > d^2).
pub fn attack_in_zone(target: Vec3, tribe: Vec3, radius: f32) -> bool {
    radius * radius > (target - tribe).length_squared()
}

/// The music chooser 0xC238B0 (jump table 0xC239E4, bytes 0xC239F8 for the action
/// types 7..=21): the sound a citizen's first goal plays, `None` for every other
/// action. `attack_zone` is the result of [`attack_in_zone`] for the Attack goal.
pub fn music_for_action(action: CitizenAction, attack_zone: bool) -> Option<PositionalMusic> {
    match action {
        CitizenAction::Attack => attack_zone.then_some(PositionalMusic::Raid),
        CitizenAction::RaidFood => Some(PositionalMusic::RaidFood),
        CitizenAction::GiftFood | CitizenAction::GiftMember => Some(PositionalMusic::Gifting),
        CitizenAction::RaidHuts => Some(PositionalMusic::Raid),
        _ => None,
    }
}

/// The table of the Tribe manager's `cThemeMusicManager` (+0x1C8, constructor
/// 0xCD4700): entries of (audio handle, squared camera distance) from +0x08, all
/// (0, FLT_MAX). Slot 0 means "none": 0xBC23B0 hands out the entries 1..=23
/// (`i < 0x18`), so 23 sounds at most.
#[derive(Clone, Debug)]
pub struct MusicSlots {
    handle: [u32; Self::ENTRIES],
    squared: [f32; Self::ENTRIES],
    next: u32,
}

impl Default for MusicSlots {
    fn default() -> Self {
        MusicSlots { handle: [0; Self::ENTRIES], squared: [f32::MAX; Self::ENTRIES], next: 1 }
    }
}

impl MusicSlots {
    pub const ENTRIES: usize = 24;

    /// 0xBC23B0 (the free entry and its sound; the sound is the caller's): the first
    /// free slot, `None` (retail 0) when every slot holds a sound.
    pub fn register(&mut self) -> Option<usize> {
        let slot = (1..Self::ENTRIES).find(|i| self.handle[*i] == 0)?;
        self.handle[slot] = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        Some(slot)
    }

    /// 0xBC2540: the sound moves to `position` and the slot stores the squared
    /// distance from the camera (`FLT_MAX` without a camera, 0xBC25DF).
    pub fn update(&mut self, slot: usize, position: Vec3, camera: Option<Vec3>) {
        if slot == 0 || slot >= Self::ENTRIES {
            return;
        }
        self.squared[slot] = camera.map_or(f32::MAX, |c| (c - position).length_squared());
    }

    /// 0xBC22F0: the sound stops (`StopAudio`) and the slot is free again.
    pub fn release(&mut self, slot: usize) {
        if slot == 0 || slot >= Self::ENTRIES {
            return;
        }
        self.handle[slot] = 0;
        self.squared[slot] = f32::MAX;
    }

    /// 0xBC2330 (the Tribe shutdown, 0xCD60CB): every slot free.
    pub fn clear(&mut self) {
        *self = MusicSlots { next: self.next, ..Default::default() };
    }

    /// Slots holding a sound.
    pub fn used(&self) -> usize {
        self.handle.iter().filter(|h| **h != 0).count()
    }

    /// 0xBC2420, the `distance_to_travel_music` provider's value (0xEA7170 passes it
    /// on): the square root of the smallest stored squared distance, a distance
    /// (`sqrt(FLT_MAX)` with no sound).
    pub fn nearest(&self) -> f32 {
        self.squared[1..].iter().copied().fold(f32::MAX, f32::min).sqrt()
    }
}

/// One music source of the frame: a command group (or one attacker) and where its
/// sound plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Want {
    pub key: SourceKey,
    pub music: PositionalMusic,
    pub position: Vec3,
}

/// What a source is: a command group of the player's tribe (the action id and the
/// target village), the NPC tribe's gift group of a village, or one attacker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKey {
    Group(i32, usize),
    NpcGift(usize),
    Attacker(Entity),
}

/// A player's unit as the source list sees it.
#[derive(Clone, Copy, Debug)]
pub struct UnitView {
    pub entity: Entity,
    pub task: UnitTask,
    pub position: Vec3,
}

/// The music sources of the Tribal stage this frame.
///
/// VERIFIED (code image): the registrations are the group's behaviours, not the
/// members': the RaidHuts update (0xDBAD84..0xDBADC9) and the gift routine (0xDBB7D0,
/// registered at 0xDBBC02, followed at 0xDBC098, released at 0xDBC135) keep the slot in
/// the command group's record (+0x1CC of the goal group `[goals+0x600]`) and only the
/// group leader (`BC9C80`, the first member) registers, the walk update (0xDB7120)
/// keeps it in the pooled job entry (+0x248) and releases it for the leader (0xDB7092),
/// so one sound plays per command group; the Attack update (0xD69740) keeps a slot per
/// attacker. The registrations of the Attack update, the RaidHuts update and the walk
/// update only run for a player-owned citizen (`IsPlayerOwned` 0xC24590: the tribe's);
/// the gift routine registers for any tribe's gift (an NPC tribe's gift plan too).
/// So: the player's units commanded to raid huts / steal food / gift play the music of
/// their group at its leader, an attacker plays it only inside its target's zone (never,
/// [`ATTACK_ZONING_RADIUS`]), and an NPC gift group plays `music_gifting` while it carries
/// the food over. NPC raiders play none.
///
/// ADAPTED: OpenSpore has no command groups; the units with the same action and target
/// village form a group and the leader is the unit with the lowest entity id (retail: the
/// first member of the group's array).
pub fn wanted_sources(
    units: &[UnitView],
    target: &dyn Fn(Entity) -> Option<(Vec3, Vec3)>,
    npc_gifts: &[(Entity, usize, Vec3)],
) -> Vec<Want> {
    let mut out: Vec<Want> = Vec::new();
    let mut leaders: HashMap<SourceKey, (u64, PositionalMusic, Vec3)> = HashMap::new();
    for u in units {
        let (action, village) = match u.task {
            UnitTask::Siege(v) => (CitizenAction::RaidHuts, v),
            UnitTask::Raid(v) => (CitizenAction::RaidFood, v),
            UnitTask::Gift(v) => (CitizenAction::GiftFood, v),
            UnitTask::Hunt(t) => {
                // Attack (7): a citizen target inside its own tribe's zone.
                let zone = target(t).is_some_and(|(at, tribe)| attack_in_zone(at, tribe, ATTACK_ZONING_RADIUS));
                if let Some(music) = music_for_action(CitizenAction::Attack, zone) {
                    out.push(Want { key: SourceKey::Attacker(u.entity), music, position: u.position });
                }
                continue;
            }
            _ => continue,
        };
        let Some(music) = music_for_action(action, false) else { continue };
        let key = SourceKey::Group(action.id(), village);
        let bits = u.entity.to_bits();
        let lead = leaders.entry(key).or_insert((bits, music, u.position));
        if bits < lead.0 {
            *lead = (bits, music, u.position);
        }
    }
    out.extend(leaders.into_iter().map(|(key, (_, music, position))| Want { key, music, position }));
    let mut gifts: HashMap<usize, (u64, Vec3)> = HashMap::new();
    for (entity, village, position) in npc_gifts {
        let bits = entity.to_bits();
        let lead = gifts.entry(*village).or_insert((bits, *position));
        if bits < lead.0 {
            *lead = (bits, *position);
        }
    }
    out.extend(gifts.into_iter().map(|(village, (_, position))| Want {
        key: SourceKey::NpcGift(village),
        music: PositionalMusic::Gifting,
        position,
    }));
    // A stable order (the slot numbers follow the first registration).
    out.sort_by_key(|w| match w.key {
        SourceKey::Group(a, v) => (0, a as u64, v as u64),
        SourceKey::NpcGift(v) => (1, 0, v as u64),
        SourceKey::Attacker(e) => (2, e.to_bits(), 0),
    });
    out
}

struct Source {
    music: PositionalMusic,
    slot: Option<usize>,
    voice: Option<VoiceId>,
    /// Frames until the voice may be asked for again (not installed / polyphony).
    wait: u32,
}

/// The positional raid / gift music of the Tribal stage: the slot table of the
/// Tribe manager and the voices of its sources.
#[derive(Resource, Default)]
pub struct RaidMusic {
    pub slots: MusicSlots,
    sources: HashMap<SourceKey, Source>,
    distance: Option<f32>,
    on: bool,
}

/// The input the drums patch ducks with (provider 0xEA7170).
pub const DISTANCE_INPUT: &str = "distance_to_travel_music";

impl RaidMusic {
    /// The sources that hold a slot, with the sound each plays.
    pub fn active(&self) -> Vec<(SourceKey, PositionalMusic)> {
        let mut v: Vec<_> = self.sources.iter().filter(|(_, s)| s.slot.is_some()).map(|(k, s)| (*k, s.music)).collect();
        v.sort_by_key(|(_, m)| m.id());
        v
    }

    /// The sources whose sound is playing now.
    pub fn voices(&self) -> Vec<VoiceId> {
        self.sources.values().filter_map(|s| s.voice).collect()
    }

    /// One update of the table and the voices (the retail registration, update and
    /// release calls, 0xBC23B0 / 0xBC2540 / 0xBC22F0): a source that appears takes a free
    /// slot and starts its sound at its place, a source that stays moves its sound and
    /// stores the camera distance, a source that went stops its sound and frees the slot.
    /// With every slot taken a source has no sound and tries again next frame (retail
    /// registers again at every update while the slot is 0).
    pub fn step(&mut self, pd: &mut PdMusic, want: &[Want], camera: Option<Vec3>) {
        let gone: Vec<SourceKey> = self.sources.keys().copied().filter(|k| !want.iter().any(|w| w.key == *k)).collect();
        for key in gone {
            if let Some(s) = self.sources.remove(&key) {
                if let Some(slot) = s.slot {
                    self.slots.release(slot);
                }
                if let Some(v) = s.voice {
                    pd.stop_voice(v);
                }
            }
        }
        for w in want {
            let src = self.sources.entry(w.key).or_insert(Source { music: w.music, slot: None, voice: None, wait: 0 });
            if src.slot.is_none() {
                src.slot = self.slots.register();
            }
            let Some(slot) = src.slot else { continue };
            self.slots.update(slot, w.position, camera);
            match src.voice {
                Some(v) => pd.move_voice(v, w.position),
                None if src.wait == 0 => {
                    src.voice = pd.start_positional_voice(src.music.id(), "tribe", w.position, Bus::Music);
                    src.wait = if src.voice.is_some() { 0 } else { 30 };
                }
                None => src.wait -= 1,
            }
        }
    }
}

/// The positional raid / gift music and its input `distance_to_travel_music`.
///
/// RETAIL (march2017, `notes\tribal-sound2-notes.md` 3 and `notes\tribal-raidmusic-notes.md`):
/// the Tribe manager's slot table (register 0xBC23B0, update 0xBC2540, release 0xBC22F0,
/// clear 0xBC2330 at the Tribe shutdown 0xCD60CB) holds the sounds the command groups play
/// (see [`wanted_sources`] for who registers); `distance_to_travel_music` (provider 0xEA7170,
/// Tribe mode only, every frame, on change) is the camera's distance to the nearest of them
/// ([`MusicSlots::nearest`]; `sqrt(FLT_MAX)` with none), and `tribe_drums3` ducks its voices
/// with it (silent within 100, full from 200). The sounds are `music_raid` (3D 25..75, loop),
/// `music_raidfood` (3D 15..45) and `music_gifting` (3D 15..55): plain looping voices that
/// move with their source and roll off from the camera ([`PdMusic::start_positional_voice`]).
#[allow(clippy::too_many_arguments)]
pub fn feed_raid_music(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    mut music: ResMut<RaidMusic>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    units: Query<(Entity, &UnitTask, &Transform), With<TribeUnit>>,
    rivals: Query<(&RivalMember, &Transform)>,
    bearers: Query<(Entity, &GiftBearer, &RivalMember, &Transform)>,
) {
    if *stage.get() != GameStage::Tribal || !pd.context_active("tribe") {
        if music.on {
            // The Tribe shutdown clears the table; the context stop took the voices.
            *music = RaidMusic::default();
            pd.ears = None;
        }
        return;
    }
    let pd = &mut *pd;
    let music = &mut *music;
    music.on = true;
    let eye = camera.single().ok().map(|c| c.translation());
    pd.ears = eye;
    let views: Vec<UnitView> = units.iter().map(|(entity, task, t)| UnitView { entity, task: *task, position: t.translation }).collect();
    let target = |e: Entity| rivals.get(e).ok().map(|(m, t)| (t.translation, crate::tribal::village_position(m.village)));
    let gifts: Vec<(Entity, usize, Vec3)> = bearers
        .iter()
        .filter(|(_, b, _, _)| b.phase == GiftPhase::Approach)
        .map(|(e, _, m, t)| (e, m.village, t.translation))
        .collect();
    let want = wanted_sources(&views, &target, &gifts);
    music.step(pd, &want, eye);
    let d = music.slots.nearest();
    if music.distance != Some(d) {
        music.distance = Some(d);
        pd.set_audio_param(DISTANCE_INPUT, d);
    }
}

/// The looping high-altitude wind.
#[derive(Component)]
pub struct WindVoice;

/// One looping wind voice for the stage; its volume follows the camera.
#[allow(clippy::too_many_arguments)]
fn wind(
    mut commands: Commands,
    mut audio: ResMut<GameAudio>,
    camera: Res<TribalCamera>,
    voices: Query<Entity, With<WindVoice>>,
    mut sinks: Query<&mut AudioSink, With<WindVoice>>,
) {
    // The stingers are music-bus cues the director frees at a stage change:
    // ask for them again so they are decoded when the moment comes.
    for cue in [Cue::TribalCampfire, Cue::TribalDeathSting, Cue::TribalToCivStart] {
        audio.request(cue);
    }
    // The curve is in retail altitude units: the camera's height over its look
    // point (tribal-cam: the retail zoom ladder tops out at 90 units, so the
    // wind stays silent until another camera, such as a stage-change flight,
    // goes above 500).
    let level = wind_gain(camera.rig.altitude());
    if voices.is_empty() {
        audio.request(Cue::TribalHighAltWind);
        let Some((handle, gain)) = audio.clip(Cue::TribalHighAltWind) else {
            return;
        };
        let base = Bus::Ambience.volume() * gain * Cue::TribalHighAltWind.spec().trim;
        let volume = if audio.muted { 0.0 } else { base * level };
        commands.spawn((
            Name::new("ambience tribal high altitude wind"),
            AudioPlayer::new(handle),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(volume)),
            WindVoice,
            StageOwned,
        ));
        return;
    }
    let Some((_, gain)) = audio.clip(Cue::TribalHighAltWind) else {
        return;
    };
    let base = Bus::Ambience.volume() * gain * Cue::TribalHighAltWind.spec().trim;
    let volume = if audio.muted { 0.0 } else { base * level };
    for mut sink in &mut sinks {
        sink.set_volume(Volume::Linear(volume));
    }
}

pub struct TribalAudioPlugin;

impl Plugin for TribalAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GoalPulse>().add_systems(
            Update,
            (command_sounds, observe, wind)
                .in_set(crate::GameSystems::Presentation)
                .run_if(in_state(GameStage::Tribal))
                .run_if(resource_exists::<GameAudio>)
                .run_if(resource_exists::<Messages<PlayCue>>),
        );
        app.add_systems(
            Update,
            goal_sounds
                .in_set(crate::GameSystems::Presentation)
                .run_if(in_state(GameStage::Tribal))
                .run_if(resource_exists::<NamedSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_is_silent_until_altitude_500() {
        assert_eq!(wind_gain(0.0), 0.0);
        assert_eq!(wind_gain(20.0), 0.0);
        assert_eq!(wind_gain(500.0), 0.0);
        assert!((wind_gain(525.0) - 0.1).abs() < 1e-6);
        assert!((wind_gain(550.0) - 0.2).abs() < 1e-6);
        assert!((wind_gain(580.0) - 0.35).abs() < 1e-6);
        assert!((wind_gain(5000.0) - 0.35).abs() < 1e-6);
        assert!(wind_gain(-4.0) == 0.0);
    }

    #[test]
    fn only_the_farthest_zoom_blows() {
        assert_eq!(retail_altitude(0.0), 20.0);
        assert_eq!(retail_altitude(1.0), 600.0);
        assert_eq!(wind_gain(retail_altitude(0.0)), 0.0);
        assert_eq!(wind_gain(retail_altitude(0.5)), 0.0);
        assert_eq!(wind_gain(retail_altitude(0.8)), 0.0);
        let far = wind_gain(retail_altitude(1.0));
        assert!((far - 0.35).abs() < 1e-6);
        assert!(wind_gain(retail_altitude(0.9)) > 0.0);
        assert_eq!(retail_altitude(-3.0), 20.0);
    }

    #[test]
    fn every_action_sound_has_a_cue_of_the_same_name() {
        let mut cued = 0;
        for action in CitizenAction::ALL {
            match (action.sound(), action_cue(action)) {
                (Some(name), Some(cue)) => {
                    cued += 1;
                    assert!(
                        cue.spec().names.contains(&name),
                        "{action:?}: {name} is not a name of {cue:?}"
                    );
                }
                (None, None) => {}
                (sound, cue) => panic!("{action:?}: sound {sound:?} but cue {cue:?}"),
            }
        }
        assert_eq!(cued, 19, "every sounding action has a cue");
    }

    // --- the observers in a small app -----------------------------------------

    fn tiny_app() -> App {
        let mut app = App::new();
        app.add_message::<PlayCue>()
            .insert_resource(RunProgress::default())
            .insert_resource(TribalControls::default())
            .insert_resource(FrameCount(0));
        app
    }

    /// One more frame; returns the cues requested during it.
    fn frame(app: &mut App) -> Vec<Cue> {
        let next = app.world().resource::<FrameCount>().0.wrapping_add(1);
        app.world_mut().insert_resource(FrameCount(next));
        app.update();
        app.world_mut()
            .resource_mut::<Messages<PlayCue>>()
            .drain()
            .map(|PlayCue(cue)| cue)
            .collect()
    }

    #[test]
    fn a_command_plays_its_action_sound_once_however_many_members_take_it() {
        use crate::tribal_controls::Issued;
        let mut app = tiny_app();
        app.add_systems(Update, command_sounds);
        app.update();
        app.world_mut().resource_mut::<TribalControls>().issued = vec![
            Issued {
                action: CitizenAction::Fish,
                count: 5,
                point: Vec3::ZERO,
            },
            Issued {
                action: CitizenAction::WalkTo,
                count: 3,
                point: Vec3::ZERO,
            },
            Issued {
                action: CitizenAction::GatherMeat,
                count: 1,
                point: Vec3::ZERO,
            },
        ];
        assert_eq!(
            frame(&mut app),
            vec![Cue::TribalActionFish, Cue::TribalActionGatherMeat],
            "one sound per sounding command, none for the walk"
        );
    }

    #[test]
    fn stance_purchase_death_upgrade_and_camera_buttons_have_their_sounds() {
        let mut app = tiny_app();
        app.add_systems(Update, observe);
        app.update();
        // The first frames only take the baseline.
        assert!(frame(&mut app).is_empty());
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.friendly_stance = !run.tribal_campaign.friendly_stance;
        }
        let toggled = app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .friendly_stance;
        assert_eq!(
            frame(&mut app),
            vec![if toggled {
                Cue::TribalModeSocial
            } else {
                Cue::TribalModeAttack
            }]
        );
        assert!(frame(&mut app).is_empty(), "a held stance is silent");
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .friendly_stance = !toggled;
        assert_eq!(
            frame(&mut app),
            vec![if toggled {
                Cue::TribalModeAttack
            } else {
                Cue::TribalModeSocial
            }]
        );
        // A member dies.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribe
            .push(crate::game::MateAssignment::default());
        frame(&mut app);
        app.world_mut().resource_mut::<RunProgress>().tribe.pop();
        assert_eq!(frame(&mut app), vec![Cue::TribalDeathSting]);
        // A HUD camera button goes down: once per press.
        app.world_mut().resource_mut::<TribalControls>().hud.zoom_in = true;
        assert_eq!(frame(&mut app), vec![Cue::CameraZoom]);
        assert!(frame(&mut app).is_empty(), "held, not repeated");
        app.world_mut().resource_mut::<TribalControls>().hud.zoom_in = false;
        frame(&mut app);
        app.world_mut().resource_mut::<TribalControls>().hud.rotate_left = true;
        assert_eq!(frame(&mut app), vec![Cue::CameraZoom]);
    }

    #[test]
    fn a_gap_in_frames_rebaselines_the_observers() {
        let mut app = tiny_app();
        app.add_systems(Update, observe);
        app.update();
        frame(&mut app);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .friendly_stance = true;
        // The stage was left and re-entered: frames skipped, nothing played.
        app.world_mut().insert_resource(FrameCount(500));
        app.update();
        let cues: Vec<Cue> = app
            .world_mut()
            .resource_mut::<Messages<PlayCue>>()
            .drain()
            .map(|PlayCue(cue)| cue)
            .collect();
        assert!(cues.is_empty());
    }

    // --- a completed goal card -------------------------------------------------

    #[test]
    fn goals_completed_counts_the_new_entries_only() {
        assert_eq!(goals_completed(0, 0), 0);
        assert_eq!(goals_completed(1, 2), 1);
        assert_eq!(goals_completed(1, 3), 2);
        assert_eq!(goals_completed(3, 1), 0, "a smaller list is another save");
    }

    #[test]
    fn a_completed_goal_card_plays_the_mission_sound_and_steps_the_pulse() {
        use crate::tribal_hud::tutorial::TutorialGoal;
        let mut app = App::new();
        app.insert_resource(RunProgress::default())
            .insert_resource(FrameCount(0))
            .init_resource::<Time<Real>>()
            .init_resource::<NamedSounds>()
            .init_resource::<GoalPulse>()
            .add_systems(Update, goal_sounds);
        let step = |app: &mut App| {
            let next = app.world().resource::<FrameCount>().0.wrapping_add(1);
            app.world_mut().insert_resource(FrameCount(next));
            app.update();
        };
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        for _ in 0..3 {
            step(&mut app);
        }
        // Only the prefetch request so far: nothing is waiting to play.
        assert!(queued(&app).is_empty());
        assert_eq!(app.world().resource::<GoalPulse>().0, 0);
        // The Outfit goal is added (no sound) and completed (the sound).
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.tutorial.add(TutorialGoal::Outfit);
        }
        step(&mut app);
        assert!(queued(&app).is_empty(), "adding a card is silent");
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.tutorial.complete(TutorialGoal::Outfit);
        step(&mut app);
        assert_eq!(queued(&app), vec![crate::world_objects::name_id(MISSION_SOUND)]);
        assert_eq!(app.world().resource::<GoalPulse>().0, 1);
        step(&mut app);
        assert_eq!(queued(&app).len(), 1, "once per completion");
        // Two goals at once: one jingle, the pulse steps twice.
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let t = &mut run.tribal_campaign.tutorial;
            t.add(TutorialGoal::Diet);
            t.add(TutorialGoal::Populate);
            t.complete(TutorialGoal::Diet);
            t.complete(TutorialGoal::Populate);
        }
        step(&mut app);
        assert_eq!(app.world().resource::<GoalPulse>().0, 3);
        // A gap in frames (the stage was left): a save with more done goals is not a completion.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.tutorial.done.push(TutorialGoal::Domesticate);
        app.world_mut().insert_resource(FrameCount(900));
        app.update();
        assert_eq!(app.world().resource::<GoalPulse>().0, 3);
    }

    #[test]
    fn the_tribe_list_select_all_plays_the_name_select_sound() {
        // 0xE23D8E pushes `ui_trg_name_select` (0xA715736E); the list's Select All command is 0x03DAE412.
        assert_eq!(crate::world_objects::name_id("ui_trg_name_select"), 0xA715_736E);
        assert!(crate::ui_sounds::rules()
            .iter()
            .any(|r| r.layout == "PosseBar" && r.control == 0x03DA_E412 && r.sound == "ui_trg_name_select"));
        assert_eq!(crate::tribal_hud::world::id::POSSE_SELECT_ALL, 0x03DA_E412);
    }

    // --- the positional raid / gift music (tribal-raidmusic) -------------------------

    #[test]
    fn the_chooser_returns_the_retail_music_of_each_action() {
        // The ids the code image pushes (0xC239B0, 0xC239CD, 0xC239D7) are the names' FNV ids.
        for music in [PositionalMusic::Raid, PositionalMusic::RaidFood, PositionalMusic::Gifting] {
            assert_eq!(crate::world_objects::name_id(music.sound()), music.id(), "{music:?}");
        }
        for action in CitizenAction::ALL {
            let expect = match action.id() {
                8 => Some(PositionalMusic::RaidFood),
                9 | 21 => Some(PositionalMusic::Gifting),
                15 => Some(PositionalMusic::Raid),
                _ => None,
            };
            assert_eq!(music_for_action(action, false), expect, "{action:?}");
        }
        // Attack (7): the music only inside the target's zone.
        assert_eq!(music_for_action(CitizenAction::Attack, true), Some(PositionalMusic::Raid));
        assert_eq!(music_for_action(CitizenAction::Attack, false), None);
    }

    #[test]
    fn an_attack_plays_only_inside_the_zone_of_its_targets_tribe() {
        let tribe = Vec3::new(100.0, 4.0, 100.0);
        // The constructor's radius 30: inside / outside (squared distances, all three axes).
        assert!(attack_in_zone(tribe + Vec3::new(20.0, 3.0, 0.0), tribe, CTOR_ZONING_RADIUS));
        assert!(!attack_in_zone(tribe + Vec3::new(29.0, 0.0, 9.0), tribe, CTOR_ZONING_RADIUS));
        assert!(!attack_in_zone(tribe + Vec3::new(0.0, 31.0, 0.0), tribe, CTOR_ZONING_RADIUS));
        // The radius `SpawnTribe` leaves (0.0): not even the camp fire is inside.
        assert!(!attack_in_zone(tribe, tribe, ATTACK_ZONING_RADIUS));
        assert_eq!(ATTACK_ZONING_RADIUS, 0.0);
    }

    #[test]
    fn the_slot_table_hands_out_23_slots_and_reports_the_nearest_distance() {
        let mut slots = MusicSlots::default();
        assert_eq!(slots.nearest(), f32::MAX.sqrt(), "no sound: sqrt(FLT_MAX)");
        // Slot 0 is "none": the first sound gets 1, the 23rd gets 23, the 24th gets none.
        let got: Vec<Option<usize>> = (0..24).map(|_| slots.register()).collect();
        assert_eq!(got[0], Some(1));
        assert_eq!(got[22], Some(23));
        assert_eq!(got[23], None);
        assert_eq!(slots.used(), 23);
        // The update stores the squared camera distance, the minimum is reported as a distance.
        let camera = Some(Vec3::ZERO);
        slots.update(3, Vec3::new(30.0, 0.0, 40.0), camera);
        slots.update(7, Vec3::new(0.0, 12.0, 5.0), camera);
        assert!((slots.nearest() - 13.0).abs() < 1e-4, "{}", slots.nearest());
        // No camera: FLT_MAX (0xBC25DF); a released slot is free again and no longer counts.
        slots.update(7, Vec3::ZERO, None);
        assert!((slots.nearest() - 50.0).abs() < 1e-4);
        slots.release(3);
        assert_eq!(slots.nearest(), f32::MAX.sqrt());
        assert_eq!(slots.register(), Some(3), "the freed slot is the first free one");
        // Slot 0 and out-of-range slots do nothing.
        slots.update(0, Vec3::ZERO, camera);
        slots.release(99);
        assert_eq!(slots.nearest(), f32::MAX.sqrt());
        slots.clear();
        assert_eq!(slots.used(), 0);
    }

    fn entities(n: usize) -> Vec<Entity> {
        let mut world = World::new();
        (0..n).map(|_| world.spawn_empty().id()).collect()
    }

    #[test]
    fn the_players_raid_food_and_gift_groups_play_at_their_leader() {
        let e = entities(8);
        let unit = |i: usize, task: UnitTask, at: Vec3| UnitView { entity: e[i], task, position: at };
        let no_target = |_: Entity| None;
        let views = [
            // Two members raid the huts of village 1: one group, at the lowest entity.
            unit(3, UnitTask::Siege(1), Vec3::new(30.0, 0.0, 0.0)),
            unit(2, UnitTask::Siege(1), Vec3::new(10.0, 0.0, 0.0)),
            // Another village's huts, a food thief and a gift carrier: their own groups.
            unit(4, UnitTask::Siege(2), Vec3::new(0.0, 0.0, 7.0)),
            unit(5, UnitTask::Raid(1), Vec3::new(0.0, 0.0, 9.0)),
            unit(6, UnitTask::Gift(0), Vec3::new(0.0, 0.0, 11.0)),
            // No music for work, a walk, a performance or a hunt of a creature.
            unit(0, UnitTask::Idle(Vec3::ZERO), Vec3::ZERO),
            unit(1, UnitTask::MoveTo(Vec3::ONE), Vec3::ZERO),
            unit(7, UnitTask::Perform(1), Vec3::ZERO),
        ];
        let want = wanted_sources(&views, &no_target, &[]);
        let kinds: Vec<(PositionalMusic, Vec3)> = want.iter().map(|w| (w.music, w.position)).collect();
        assert_eq!(want.len(), 4, "{want:?}");
        // The leader is the member with the lowest entity id (ADAPTED), whichever that is.
        let leader_at = if e[2].to_bits() < e[3].to_bits() { Vec3::new(10.0, 0.0, 0.0) } else { Vec3::new(30.0, 0.0, 0.0) };
        assert!(kinds.contains(&(PositionalMusic::Raid, leader_at)), "the leader's place: {kinds:?}");
        assert!(kinds.contains(&(PositionalMusic::Raid, Vec3::new(0.0, 0.0, 7.0))));
        assert!(kinds.contains(&(PositionalMusic::RaidFood, Vec3::new(0.0, 0.0, 9.0))));
        assert!(kinds.contains(&(PositionalMusic::Gifting, Vec3::new(0.0, 0.0, 11.0))));
    }

    #[test]
    fn a_fight_plays_no_music_while_a_spawned_tribe_has_radius_zero() {
        let e = entities(3);
        // A member fights a rival citizen standing on its own camp fire.
        let rival = e[1];
        let target = move |t: Entity| (t == rival).then_some((Vec3::new(5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, 5.0)));
        let views = [UnitView { entity: e[0], task: UnitTask::Hunt(rival), position: Vec3::ZERO }];
        assert!(wanted_sources(&views, &target, &[]).is_empty(), "radius 0: nothing is inside");
        // The same attack with the constructor's radius would play: the rule is the retail test.
        assert!(attack_in_zone(Vec3::new(5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, 5.0), CTOR_ZONING_RADIUS));
    }

    #[test]
    fn an_npc_gift_group_plays_while_it_carries_the_food_over() {
        let e = entities(3);
        let gifts = [(e[2], 4usize, Vec3::new(1.0, 0.0, 1.0)), (e[1], 4, Vec3::new(2.0, 0.0, 2.0)), (e[0], 6, Vec3::new(3.0, 0.0, 3.0))];
        let want = wanted_sources(&[], &|_| None, &gifts);
        assert_eq!(want.len(), 2, "one group per village: {want:?}");
        assert!(want.iter().all(|w| w.music == PositionalMusic::Gifting));
        let village4 = want.iter().find(|w| w.key == SourceKey::NpcGift(4)).unwrap();
        let lowest = if e[1].to_bits() < e[2].to_bits() { Vec3::new(2.0, 0.0, 2.0) } else { Vec3::new(1.0, 0.0, 1.0) };
        assert_eq!(village4.position, lowest, "the lowest entity leads");
    }

    fn music_app() -> App {
        let mut app = App::new();
        app.insert_resource(State::new(GameStage::Tribal))
            .insert_resource(PdMusic::default())
            .init_resource::<RaidMusic>()
            .add_systems(Update, feed_raid_music);
        app.world_mut().resource_mut::<PdMusic>().force_context("tribe");
        app
    }

    fn drums_input(app: &App) -> Option<f32> {
        app.world().resource::<PdMusic>().engine.gvalues.get(&crate::pd::fnv(DISTANCE_INPUT)).copied()
    }

    /// A raid starts the positional music (a slot per command group) and the drums' input
    /// follows the camera's distance to the nearest source.
    #[test]
    fn a_raid_starts_the_positional_music_and_the_drums_input_follows_the_camera() {
        let mut app = music_app();
        let camera = app.world_mut().spawn((crate::GameCamera, GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0)))).id();
        let place = |app: &mut App, at: Vec3| {
            app.world_mut().entity_mut(camera).insert(GlobalTransform::from(Transform::from_translation(at)));
        };
        app.update();
        assert_eq!(drums_input(&app), Some(f32::MAX.sqrt()), "no source: sent once at the start");
        assert!(app.world().resource::<RaidMusic>().active().is_empty());
        // The player sends two members to raid village 0 and one to steal food.
        let raider = |app: &mut App, task: UnitTask, at: Vec3| {
            app.world_mut().spawn((TribeUnit::new(false, 3.0), task, Transform::from_translation(at))).id()
        };
        let a = raider(&mut app, UnitTask::Siege(0), Vec3::new(30.0, 0.0, 40.0));
        let _b = raider(&mut app, UnitTask::Siege(0), Vec3::new(30.0, 0.0, 40.0));
        app.update();
        {
            let m = app.world().resource::<RaidMusic>();
            assert_eq!(m.active().len(), 1, "one sound for the group: {:?}", m.active());
            assert_eq!(m.active()[0].1, PositionalMusic::Raid);
            assert_eq!(m.slots.used(), 1);
        }
        assert!((drums_input(&app).unwrap() - 50.0).abs() < 1e-3, "{:?}", drums_input(&app));
        assert_eq!(app.world().resource::<PdMusic>().ears, Some(Vec3::ZERO), "the voices roll off from the camera");
        // The camera moves: the input follows (the group's sound sits at its leader).
        place(&mut app, Vec3::new(30.0, 0.0, 80.0));
        app.update();
        assert!((drums_input(&app).unwrap() - 40.0).abs() < 1e-3);
        // A food thief joins: the nearer of the two sources counts.
        let thief = raider(&mut app, UnitTask::Raid(1), Vec3::new(30.0, 0.0, 95.0));
        app.update();
        assert_eq!(app.world().resource::<RaidMusic>().active().len(), 2);
        assert!((drums_input(&app).unwrap() - 15.0).abs() < 1e-3);
        // The thief gets home: its sound ends and the slot is free; the raid is the nearest again.
        app.world_mut().entity_mut(thief).insert(UnitTask::Idle(Vec3::ZERO));
        app.update();
        assert_eq!(app.world().resource::<RaidMusic>().active().len(), 1);
        assert!((drums_input(&app).unwrap() - 40.0).abs() < 1e-3);
        // The raid ends: the drums are free again.
        app.world_mut().entity_mut(a).insert(UnitTask::Idle(Vec3::ZERO));
        for e in app.world_mut().query_filtered::<Entity, With<TribeUnit>>().iter(app.world()).collect::<Vec<_>>() {
            app.world_mut().entity_mut(e).insert(UnitTask::Idle(Vec3::ZERO));
        }
        app.update();
        assert_eq!(app.world().resource::<RaidMusic>().slots.used(), 0);
        assert_eq!(drums_input(&app), Some(f32::MAX.sqrt()));
        // Leaving the stage clears the table (0xBC2330) and the ears.
        raider(&mut app, UnitTask::Gift(2), Vec3::new(1.0, 0.0, 1.0));
        app.update();
        assert_eq!(app.world().resource::<RaidMusic>().slots.used(), 1);
        app.world_mut().insert_resource(State::new(GameStage::Civilization));
        app.update();
        assert_eq!(app.world().resource::<RaidMusic>().slots.used(), 0);
        assert_eq!(app.world().resource::<PdMusic>().ears, None);
    }

    #[test]
    fn at_most_23_command_groups_hold_a_slot() {
        let mut app = music_app();
        app.world_mut().spawn((crate::GameCamera, GlobalTransform::default()));
        for village in 0..30 {
            app.world_mut().spawn((TribeUnit::new(false, 3.0), UnitTask::Gift(village), Transform::from_xyz(village as f32 * 10.0 + 10.0, 0.0, 0.0)));
        }
        app.update();
        let m = app.world().resource::<RaidMusic>();
        assert_eq!(m.slots.used(), 23);
        assert_eq!(m.active().len(), 23, "the other groups have no sound");
    }

    #[test]
    fn the_ui_and_stinger_cues_use_the_original_names() {
        assert_eq!(Cue::TribalModeSocial.spec().names, &["sfx_trg_mode_social"]);
        assert_eq!(Cue::TribalModeAttack.spec().names, &["sfx_trg_mode_attack"]);
        assert_eq!(Cue::CameraZoom.spec().names, &["ui_camera_zoom"]);
        assert_eq!(Cue::TribalRecruit.spec().names, &["ui_trg_buy_baby"]);
        assert_eq!(
            Cue::TribalDeathSting.spec().names,
            &["mus_trg_death_sting"]
        );
        assert_eq!(
            Cue::TribalCampfire.spec().names,
            &["mus_trg_campfire_powerup"]
        );
        assert_eq!(
            Cue::TribalToCivStart.spec().names,
            &["music_trg_to_civ_start"]
        );
        assert_eq!(
            Cue::TribalHighAltWind.spec().names,
            &["amb_trg_highalt_wind"]
        );
        assert_eq!(Cue::CivIntro.spec().names, &["music_trg_to_civ_end"]);
        // The transition stings play in order: start at the stage change, end
        // when it is over.
        assert_eq!(
            crate::game_audio::intro(&GameStage::Tribal, &GameStage::Civilization),
            Some(Cue::TribalToCivStart)
        );
        assert_eq!(
            crate::game_audio::intro_follow(Cue::TribalToCivStart),
            Some(Cue::CivIntro)
        );
        assert_eq!(crate::game_audio::intro_follow(Cue::CivIntro), None);
    }

    /// The retail sounds of this module and the Tribal leftovers of the audit exist, and how the
    /// positional Tribal music plays (printed: 3D, roll-off distances, loop).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_tribal_sounds_resolve_and_are_described() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        for name in [
            MISSION_SOUND,
            "music_raid",
            "music_raidfood",
            "music_gifting",
            "music_sting_babyborn",
            "sfx_trg_gift_start",
            "sfx_trg_gift_given",
            "sfx_trg_raid_horn",
            "ui_trg_action_accept",
            "ui_trg_name_select",
            "ui_trg_cheat_smack",
            "ui_trg_target_select",
        ] {
            match library.resolve(name) {
                Ok(d) => eprintln!(
                    "{name}: samples {} gain {:.2} loop {} 3d {} dist {:.0}..{:.0} polyphony {} fade {:.1}/{:.1}",
                    d.samples.len(), d.gain, d.looped, d.is_3d, d.min_distance, d.max_distance, d.polyphony, d.fade_in, d.fade_out
                ),
                Err(e) => eprintln!("{name}: not installed ({e})"),
            }
        }
        assert!(library.resolve(MISSION_SOUND).is_ok());
        assert!(library.resolve("ui_trg_action_accept").is_ok());
    }

    /// Sounds the played Tribal clips name that no installed package has (the sweep's data facts).
    const SILENT_IN_RETAIL: [&str; 10] = [
        "sx_trg_dirt_slide",
        "tr_recruit_horn1",
        "smot_pickupfood",
        "sfx_trg_treesmack_gong",
        "no_VOX_needed",
        "ssoc_horn01_loop",
        "ssoc_horn03_loop",
        "ssoc_didger01_loop",
        "ssoc_shaker02_loop",
        "music_csa_dance",
    ];

    /// Sweep (`tribal-sound2`): the sounds the Tribal clips carry. The clips the stage plays (the
    /// crowd's groups, the tool idles and walks, the attack / building clips) have events whose
    /// sounds are installed (`anim_audio::fire` queues every one of them for a crowd actor); the
    /// Tribal-family sound names of every other installed clip are listed with the clip ids, to
    /// see what OpenSpore never reaches.
    #[test]
    #[ignore = "needs the installed Spore animation and audio packages"]
    fn installed_tribal_clip_sounds_are_installed_and_listed() {
        use crate::anim_audio::{clip_cues, Cue as ClipCue};
        use crate::outfit_crowd::{attack_clips, building_clips, clip_id, tool_anims, tribe_clip};
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let mut played: std::collections::BTreeSet<u32> = clip_id::ALL.iter().chain(tribe_clip::ALL.iter()).copied().collect();
        for tool in 0..=11u8 {
            let a = tool_anims(tool);
            played.extend([a.idle, a.lookaround, a.en_route].into_iter().filter(|c| *c != 0));
            played.extend(attack_clips(tool).iter().copied());
            played.extend(building_clips(tool).into_iter().flatten().copied());
        }
        // The village vignettes (installed `Vignettes_Tribe*` props: the clips of their actions), the
        // celebrations and the raid / pickup constants the stage plays through its crowd actors.
        {
            use crate::tribal_vignettes as v;
            let mut defs = v::load_group(&originals.resource_index, v::TRIBE_IDLE_GROUP);
            defs.extend(v::load_group(&originals.resource_index, v::TRIBE_CHAT_GROUP));
            defs.extend(v::load_group(&originals.resource_index, v::TRIBE_GROUP));
            eprintln!("vignettes loaded: {}", defs.len());
            for def in &defs {
                for place in &def.places {
                    played.extend(place.actions.iter().map(|a| a.clip));
                }
            }
            played.extend([v::RAID_CONGREGATE, v::PICKUP_FOOD, v::BATTLE_READY, v::CHARGE]);
        }
        // The social game's clips (`tribal_rules`: start, waits, cheers, endings).
        {
            use crate::tribal_rules as r;
            played.extend(r::SOCIAL_START);
            played.extend(r::SOCIAL_WAIT);
            played.insert(r::SOCIAL_WAIT_CHIEF);
            played.extend(r::SOCIAL_CHEER.iter().map(|c| c.0));
            played.extend([r::SOCIAL_ENDING_CHIEF.0, r::SOCIAL_ENDING_CROWD.0, r::SOCIAL_ENDING_FAIL.0]);
        }
        // An action id is a TLSA group: every animation of every choice can play.
        let mut anims: std::collections::BTreeSet<u32> = Default::default();
        // (As `creature_clips::load` finds it: the group of that id in any list.)
        for id in &played {
            let group = originals.action_lists.lists.iter().flat_map(|(_, l)| &l.groups).find(|g| g.id == *id);
            match group {
                Some(group) => anims.extend(group.choices.iter().flat_map(|c| c.animations.iter().map(|a| a.id))),
                None => {
                    anims.insert(*id);
                }
            }
        }
        let played = anims;
        let mut problems: Vec<String> = vec![];
        let mut by_sound: std::collections::BTreeMap<String, usize> = Default::default();
        let mut params: std::collections::BTreeMap<String, usize> = Default::default();
        let mut missing_clips = 0;
        for id in &played {
            let Some(clip) = crate::creature_clips::clip(&originals, *id) else {
                missing_clips += 1;
                eprintln!("clip {id:08X}: not found");
                continue;
            };
            for c in clip_cues(&clip.anim) {
                match c.cue {
                    ClipCue::Sound { name, .. } => {
                        *by_sound.entry(name.clone()).or_default() += 1;
                        // The voices resolve through `voice_banks`; the foot step is a selector, not a sound.
                        if !name.starts_with('v') && name != "foot_step" && library.resolve(&name).is_err() {
                            problems.push(format!("clip {id:08X}: {name} is not installed"));
                        }
                    }
                    ClipCue::Param { name, .. } => *params.entry(name).or_default() += 1,
                    ClipCue::Footstep { .. } => {}
                }
            }
        }
        eprintln!("played clips: {} ({} not found); distinct sounds {}", played.len(), missing_clips, by_sound.len());
        for (n, c) in &by_sound {
            eprintln!("  played {n} x{c} installed {}", library.resolve(n).is_ok());
        }
        eprintln!("sound2 parameters of the played clips: {params:?}");
        // Every other installed clip with a Tribal-family sound.
        const FAMILIES: [&str; 8] = ["sfx_trg_", "ssoc_", "scom_att", "sfx_tribe", "ui_trg", "mus_trg", "amb_trg", "smot_trg"];
        let keys: Vec<_> = originals.resource_index.records.keys().filter(|k| k.type_id == crate::decode::TYPE_ANIM).copied().collect();
        let mut unreached: std::collections::BTreeMap<String, Vec<u32>> = Default::default();
        let mut all_params: std::collections::BTreeMap<String, usize> = Default::default();
        for key in keys {
            let Ok(anim) = originals.resource_index.read(key).map_err(|e| e.to_string()).and_then(|b| crate::animation::parse(&b)) else { continue };
            for c in clip_cues(&anim) {
                match c.cue {
                    ClipCue::Sound { name, .. } if FAMILIES.iter().any(|f| name.starts_with(f)) && !played.contains(&key.instance_id) => {
                        let e = unreached.entry(name).or_default();
                        if !e.contains(&key.instance_id) {
                            e.push(key.instance_id);
                        }
                    }
                    ClipCue::Param { name, .. } => *all_params.entry(name).or_default() += 1,
                    _ => {}
                }
            }
        }
        for (name, clips) in &unreached {
            eprintln!(
                "unreached {name} (installed {}): clips {:08X?}",
                library.resolve(name).is_ok(),
                &clips[..clips.len().min(6)]
            );
        }
        eprintln!("sound2 parameters of every installed clip: {all_params:?}");
        // Names the played clips reference that are not installed sounds: silent in retail too (the
        // instrument loops are the Pd patch's, `music_tribe_social`, which plays the `_long` / `_solo`
        // players; the rest are leftovers of the clip data).
        let mut unexpected: Vec<&String> = problems.iter().filter(|p| !SILENT_IN_RETAIL.iter().any(|n| p.ends_with(&format!(" {n} is not installed")))).collect();
        unexpected.dedup();
        assert!(unexpected.is_empty(), "{unexpected:#?}");
    }
}
