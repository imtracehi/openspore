//! Creature stage music: the seam for the Pure Data engine.
//!
//! RETAIL (`notes\pd-music-re-notes.md`, `notes\xstage-re\pd\mixmodes.txt`,
//! `patch_summary.txt`): entering the Creature game mode starts the sounds of
//! `mixmode_creaturegame`: the score **`music_creature_modified`** (a Pure Data
//! patch: generative melody, pattern and bass players over the instrument
//! banks, the fight percussion `music_fight_{toms,taiko,cymbal,bass}`, the
//! `sfx_heartbeat_lp` loop at low health, `amb_migration_lp`, `trepidation`
//! stings), the ambience `ambience_dawn_dusk` (`amb_dawn_lp` / `amb_dusk_lp`),
//! `crg_rain`, the mixer patches `mix_event_*` and the two wind beds. The patch
//! reads the game through global float parameters (`gfparam <name>`, set by
//! `Audio::SetParameter` every frame: the Creature update at 0xD41F23..0xD42524
//! and the stance / social / cinematic code). [`GFPARAMS`] lists the patch's
//! inputs exactly (patch_summary.txt, `music_creature_modified`) and
//! [`Params::gfparam`] answers them from the game state, so the Pd engine of
//! the engine helpers can take over by reading [`CreatureAudioParams`] and
//! starting [`PATCH`]; nothing else here needs to change.
//!
//! The Pd engine plays the patch by default (`pd_audio::feeds` sets its
//! inputs from [`CreatureAudioParams`] at the retail moments).
//! FALLBACK (`OPENSPORE_PD_MUSIC=0`, ADAPTED): [`plan`] approximates the score
//! with the existing loop-based approach of `game_audio` (sparse notes from
//! the patch's own instrument banks, the fight percussion samples, the
//! migration music), the retail dawn ambience loop and the heartbeat loop,
//! switched by the same parameters the patch reads. The generative melody,
//! tempo/key wandering, pattern seeds and the mix events are not reproduced.
//!
//! Parameter semantics (decoded from the Creature update 0xD41F23..):
//! `creature_health` is the HP fraction, `creature_health_low` / `_critical`
//! are 1 below 30% / 10% of the HP (MotiveLow / MotiveCritical),
//! `creature_hunger` is the hunger meter / 100, `nearest_predator` and
//! `nearest_epic_predator` the distance to the nearest attacker / epic,
//! `brain_level` the brain level, `distance_to_migration_point` the distance to
//! the migration nest (a large value without one), `ispregnant`, `isbaby`,
//! `instealth`, `infight`, `creature_social_level` (a social ability level
//! times 0.1) and `ismating` flags, `ui_social` / `ui_fight` the stance
//! (0xD2E55F: SetAbilityMode), `socialbar` / `npcsocialbar` the friendship
//! bars (0xD9BB39 / 0xD9BB62). `creature_social_level` is the species profile's
//! `mSocialSum` (+0x5AC) x 0.025 x 100 (0x4D4710) x 0.1 (0xD423CA), see
//! [`social_level`]. ADAPTED: OpenSpore has no babies, `infight` (the writer of
//! the int at `[object + 0x24]` is not found) is 1 while engaged in the combat
//! stance, and `mSocialSum` is the sum of the four social ability ranks.
//!
//! The event inputs (`creature-sound2`, VERIFIED call sites, march2017):
//! * `crg_meteors` is 1.0 while the meteor strike event runs: its start
//!   (0xD51840) and stop (0xD50910) are the only writers (0xD51862 / 0xD50932);
//!   the patch's `meteors` subpatch starts `sfx_meteorshower_loop` (after 4 s) and
//!   the fight percussion, then the large meteor close / far sounds. OpenSpore:
//!   `creature_events::EventRuntime::is_active(MeteorStrike)`.
//! * `rain_density` is the weather manager's storm message (0x553FE73, sent by
//!   `cWeatherManager` 0xFC6910 / 0xFC6BCD, received by the provider class of
//!   vtable 0x1487CF0 whose slot 4 registers the listener): 1.0 when the camera
//!   enters a local storm cloud (state 0 -> 1, 0xFC6AA4), 0.0 when it leaves
//!   (state 1 -> 2, 0xFC6B84); nothing in between (the `crg_rain` patch restarts
//!   its 3 s start delay on every 1: a 1 starts `sfx_weather_rain_lp` after 3 s,
//!   a 0 stops it and starts `sfx_weather_rainbirds_lp`). ADAPTED: the cloud's bounds are not
//!   decoded, so "inside" is the avatar within [`STORM_RADIUS`] of the point the
//!   storm began at (the camera follows the avatar).
//! * `missioncomplete` is set to 1.0 at every completed mission (0xC2E745, the
//!   routine that plays `ui_mission_complete`): [`MissionPulse`].
//! * `crg_ufo` is the UFO abduction event's: its start sets 1.0 (0xD4FABC, and
//!   the restore 0xD4D1A1; game mode Creature only) and its stop sets 0.0
//!   (0xD4FAD0, the inlined 0x5CA970 with `fldz` at 0xD4FB02: `creature-sound2`
//!   missed it), so `mix_event_crg_ufo` plays `sfx_ufo_arrive` and then
//!   `sfx_ufo_depart`. OpenSpore: `creature_events::ufo` writes it into
//!   `MixEvents` (the same path as the cinematic mixer events, `Params::mix`).

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::{
    creature_actions::{Encounter, EncounterMode},
    creature_flow::MixEvents,
    creature_hud::CreatureHudState,
    epic_creatures::RoamerActor,
    game::{Health, Hunger, Npc, Player, RunProgress},
    game_audio::{Cue, Layer, MusicPlan},
    stages::GameStage,
};

/// The score patch (`mixmode_creaturegame` starts it), its health-up patch and
/// the Creature Creator's.
pub const PATCH: &str = "music_creature_modified";
pub const HEALTHUP_PATCH: &str = "music_creature_healthup";
pub const EDITOR_PATCH: &str = "music_creature_editor4";

/// The sounds `mixmode_creaturegame` starts (patch_summary / mixmodes.txt).
pub const MIXMODE_STARTSOUNDS: &[&str] = &[
    "music_creature_modified",
    "mix_event_vodelay",
    "mix_event_simspeed",
    "mix_event_vogain",
    "amb_crg_highalt_wind",
    "ambience_dawn_dusk",
    "mix_event_crg_ufo",
    "amb_crg_planetzoom_wind",
    "crg_rain",
    "mix_event_sporepedia_card",
];

/// The global float parameters `music_creature_modified` reads.
pub const GFPARAMS: &[&str] = &[
    "babygrowup",
    "cin_crg_claim_nest",
    "creature_health",
    "creature_health_critical",
    "creature_health_low",
    "creature_social_level",
    "creature_social_state",
    "crg_cin_death",
    "crg_cin_start",
    "crg_epic_pause",
    "crg_meteors",
    "crg_to_trg",
    "distance_to_migration_point",
    "fight_hit",
    "infight",
    "instealth",
    "intendtofight",
    "isbaby",
    "ismating",
    "missioncomplete",
    "mixmode_creaturegame",
    "nearest_epic_predator",
    "npcsocialbar",
    "predator_moment",
    "predator_momentB",
    "socialbar",
    "socialsuccess",
    "timeofday",
    "ui_fight",
    "ui_social",
];

/// Inputs of the patch the rewrite does not drive (they read 0). Retail does
/// not drive them either: no immediate, string or animation event sets the baby
/// growth, the predator moments (stings), the hit pulse, `intendtofight` and
/// `creature_social_state` (`pd_audioeeds.rs`); `socialsuccess` is the feed's.
pub const UNDRIVEN: &[&str] = &[
    "babygrowup",
    "fight_hit",
    "intendtofight",
    "predator_moment",
    "predator_momentB",
    "socialsuccess",
];

/// `distance_to_migration_point` / `nearest_*predator` without a target.
pub const NO_TARGET: f32 = 10_000.0;
/// `creatureStartTimeOfDay` (CreatureGameTuning: 7 h). ADAPTED: the day does
/// not advance.
pub const START_TIME_OF_DAY: f32 = 7.0;
/// `MotiveLow` / `MotiveCritical` as fractions (`creature_health_low`).
pub const HEALTH_LOW: f32 = 0.30;
pub const HEALTH_CRITICAL: f32 = 0.10;
/// Parameters the cinematic scripts set with `mixevent <name> 1/0` (and the
/// stage's mixer code); 0 until set. `crg_epic_pause` and `cin_crg_claim_nest`
/// have no writer in retail (no immediate, string, clip `sound2` event or script
/// sets them: the patch parts behind them are dead), `crg_ufo` is the UFO
/// event's (see the module header).
pub const MIX_PARAMS: &[&str] = &[
    "cin_crg_claim_nest",
    "crg_cin_death",
    "crg_cin_start",
    "crg_epic_pause",
    "crg_migrate_mix",
    "crg_to_trg",
    "crg_ufo",
    "play_mate_music",
];

/// How near the avatar must be to the point a rain storm began at for the
/// camera to count as inside the cloud (`rain_density` 1). ADAPTED: the storm
/// effect 0x05555FE0 is drawn from camera distance 100 (`lodDistances` 100,
/// 300, 400, 800), the bounds retail tests the camera against were not read.
pub const STORM_RADIUS: f32 = 100.0;

/// A migration is "active" while its point is nearer than this (otherwise
/// `distance_to_migration_point` is [`NO_TARGET`]).
const MIGRATING_BELOW: f32 = NO_TARGET * 0.5;

/// What the game gives the patch, once per frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub creature_health: f32,
    pub creature_hunger: f32,
    pub nearest_predator: f32,
    pub nearest_epic_predator: f32,
    pub brain_level: f32,
    pub distance_to_migration_point: f32,
    pub ispregnant: bool,
    pub isbaby: bool,
    pub instealth: bool,
    pub infight: bool,
    pub ismating: bool,
    pub creature_social_level: f32,
    pub creature_social_state: bool,
    pub socialbar: f32,
    /// Stance (`ui_social` / `ui_fight`).
    pub ui_fight: bool,
    pub timeofday: f32,
    /// The meteor strike event runs (`crg_meteors`, 0xD51840 / 0xD50910).
    pub meteors: bool,
    /// The camera is inside a local storm (`rain_density`, message 0x553FE73).
    pub rain_density: f32,
    /// Cinematic mixer events (`mixevent <name> 1/0`): `crg_cin_start`,
    /// `crg_cin_death`, `crg_to_trg`, `cin_crg_claim_nest`, ...
    pub mix: BTreeMap<String, f32>,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            creature_health: 1.0,
            creature_hunger: 1.0,
            nearest_predator: NO_TARGET,
            nearest_epic_predator: NO_TARGET,
            brain_level: 0.0,
            distance_to_migration_point: NO_TARGET,
            ispregnant: false,
            isbaby: false,
            instealth: false,
            infight: false,
            ismating: false,
            creature_social_level: 0.0,
            creature_social_state: false,
            socialbar: 0.0,
            ui_fight: false,
            timeofday: START_TIME_OF_DAY,
            meteors: false,
            rain_density: 0.0,
            mix: BTreeMap::new(),
        }
    }
}

fn flag(on: bool) -> f32 {
    if on {
        1.0
    } else {
        0.0
    }
}

impl Params {
    /// The value of the global parameter `name` as the retail game sets it
    /// (`None` for a name the patch does not read).
    pub fn gfparam(&self, name: &str) -> Option<f32> {
        Some(match name {
            "creature_health" => self.creature_health,
            "creature_health_low" => flag(self.creature_health < HEALTH_LOW),
            "creature_health_critical" => flag(self.creature_health < HEALTH_CRITICAL),
            "creature_hunger" => self.creature_hunger,
            "nearest_predator" => self.nearest_predator,
            "nearest_epic_predator" => self.nearest_epic_predator,
            "brain_level" => self.brain_level,
            "distance_to_migration_point" => self.distance_to_migration_point,
            "ispregnant" => flag(self.ispregnant),
            "isbaby" => flag(self.isbaby),
            "instealth" => flag(self.instealth),
            "infight" => flag(self.infight),
            "ismating" => flag(self.ismating),
            "creature_social_level" => self.creature_social_level,
            "creature_social_state" => flag(self.creature_social_state),
            "socialbar" | "npcsocialbar" => self.socialbar,
            "ui_fight" => flag(self.ui_fight),
            "ui_social" => flag(!self.ui_fight),
            "timeofday" => self.timeofday,
            "crg_meteors" => flag(self.meteors),
            "rain_density" => self.rain_density,
            // A pulse (0xC2E745): [`MissionPulse`] sends the 1.0 at each completion.
            "missioncomplete" => 0.0,
            "mixmode_creaturegame" => 1.0,
            _ => {
                if let Some(v) = self.mix.get(name) {
                    return Some(*v);
                }
                if UNDRIVEN.contains(&name) || MIX_PARAMS.contains(&name) {
                    return Some(0.0);
                }
                return None;
            }
        })
    }

    /// A nest migration is under way (the migration music and
    /// `amb_migration_lp` of the patch).
    pub fn migrating(&self) -> bool {
        self.distance_to_migration_point < MIGRATING_BELOW
    }

    /// Health low enough for the heartbeat (`creature_health_low`).
    pub fn health_low(&self) -> bool {
        self.creature_health < HEALTH_LOW
    }
}

/// The parameters of this frame (what the Pd engine reads through `gfparam`).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct CreatureAudioParams(pub Params);

/// `creature_social_level` (the Creature audio update 0xD423BF..0xD423CA,
/// VERIFIED): `0x4D4710(profile)` is `mSocialSum` x 0.025 x 100 and the update
/// multiplies it by 0.1 (0x14887D4), so a rank point is worth 0.25.
/// ADAPTED: the profile's `mSocialSum` (the social part levels) is taken as the
/// sum of the four social ability ranks.
pub fn social_level(ranks: [u8; 4]) -> f32 {
    ranks.iter().map(|r| f32::from(*r)).sum::<f32>() * SOCIAL_LEVEL_PER_RANK
}

/// 0.025 x 100 x 0.1.
const SOCIAL_LEVEL_PER_RANK: f32 = 0.025 * 100.0 * 0.1;

/// `rain_density` (0xFC6BCD): 1 while the camera is inside a running storm's
/// cloud, else 0. ADAPTED: the avatar within [`STORM_RADIUS`] of the storm's
/// start point.
pub fn rain_density(storm: Option<Vec3>, at: Vec3) -> f32 {
    flag(storm.is_some_and(|c| c.distance(at) <= STORM_RADIUS))
}

/// Completed missions (0xC2E700 sets `missioncomplete` 1.0 each time, and the
/// patch's `missionsuccess` bang plays on every set): `creature_audio` counts
/// them, the Pd feed sends the 1.0 when the counter rises.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissionPulse(pub u32);

/// The loop-based approximation of the score for the current situation.
/// `danger`: in or just after a fight (the music director holds it for 8 s);
/// `migrating`: a migration is pending.
pub fn plan(danger: bool, migrating: bool, p: &Params) -> MusicPlan {
    MusicPlan {
        music: if danger || p.infight {
            Layer::notes(Cue::CreatureFight, (1.6, 2.6))
        } else if migrating || p.migrating() {
            Layer::playlist(&[Cue::CreatureMigrate], (2.0, 6.0), 0.0)
        } else {
            Layer::notes(Cue::CreatureNotes, (1.4, 4.5))
        },
        // `ambience_dawn_dusk`: the dawn loop (the stage starts at 7 h).
        bed: Layer::bed(Cue::CreatureAmbienceDawn, 1.0),
        // The patch starts `sfx_heartbeat_lp` while the health is low.
        bed2: if p.health_low() { Layer::bed(Cue::CreatureHeartbeat, 0.8) } else { None },
    }
}

pub struct CreatureMusicPlugin;

impl Plugin for CreatureMusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureAudioParams>().init_resource::<MissionPulse>().add_systems(
            Update,
            publish.run_if(in_state(GameStage::Creature)).after(crate::creature_actions::update),
        );
    }
}

/// Fill [`CreatureAudioParams`] from the stage (the retail Creature audio
/// update: 0xD41F23 and the stance, social and cinematic code).
#[allow(clippy::too_many_arguments)]
fn publish(
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    mix: Res<MixEvents>,
    hud: Res<CreatureHudState>,
    mobility: Option<Res<crate::creature::mobility::CreatureMobility>>,
    events: Option<Res<crate::creature_events::EventRuntime>>,
    player: Query<(&Transform, &Health, Option<&Hunger>), With<Player>>,
    npcs: Query<(&Transform, &Npc)>,
    roamers: Query<(&Transform, &RoamerActor)>,
    mut out: ResMut<CreatureAudioParams>,
) {
    let mut p = Params {
        brain_level: run.creature_progression.brain_level as f32,
        ismating: run.creature_lifecycle.mating,
        ispregnant: run.creature_lifecycle.egg_incubating,
        instealth: mobility.is_some_and(|m| m.sneaking),
        infight: encounter.mode == EncounterMode::Combat && encounter.target.is_some(),
        ui_fight: encounter.mode == EncounterMode::Combat,
        creature_social_state: hud.social_meter.as_ref().is_some_and(|m| m.requested.is_some()),
        socialbar: hud.social_meter.as_ref().map_or(0.0, |m| m.friendship.clamp(0.0, 1.0)),
        creature_social_level: social_level(hud.social.map(|s| s.rank)),
        mix: mix.0.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        meteors: events.as_ref().is_some_and(|e| e.is_active(crate::creature_events::EventKind::MeteorStrike)),
        ..default()
    };
    if let Ok((at, health, hunger)) = player.single() {
        p.creature_health = if health.max > 0.0 { (health.hp / health.max).clamp(0.0, 1.0) } else { 1.0 };
        if let Some(h) = hunger.filter(|h| h.max > 0.0) {
            p.creature_hunger = (h.food / h.max).clamp(0.0, 1.0);
        }
        let here = at.translation;
        p.nearest_predator = npcs
            .iter()
            .filter(|(_, n)| n.hostile && n.hp > 0.0)
            .map(|(t, _)| t.translation.distance(here))
            .chain(roamers.iter().filter(|(_, r)| r.aggressive).map(|(t, _)| t.translation.distance(here)))
            .fold(NO_TARGET, f32::min);
        p.nearest_epic_predator =
            roamers.iter().map(|(t, _)| t.translation.distance(here)).fold(NO_TARGET, f32::min);
        p.rain_density = rain_density(events.as_ref().and_then(|e| e.storm_at()), here);
        if let Some(m) = &run.creature_progression.pending_migration {
            let d = Vec2::new(m.destination[0] - here.x, m.destination[2] - here.z);
            p.distance_to_migration_point = d.length();
        }
    }
    if out.0 != p {
        out.0 = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_of_the_patch_is_answered_or_declared_undriven() {
        let p = Params::default();
        for name in GFPARAMS {
            assert!(p.gfparam(name).is_some(), "{name} has no value");
        }
        assert_eq!(p.gfparam("not_a_param"), None);
        for name in UNDRIVEN {
            assert!(GFPARAMS.contains(name), "{name} is not an input of the patch");
        }
        assert_eq!(p.gfparam("mixmode_creaturegame"), Some(1.0));
        assert_eq!(p.gfparam("timeofday"), Some(7.0));
    }

    #[test]
    fn health_flags_follow_the_motive_thresholds() {
        let mut p = Params::default();
        assert_eq!(p.gfparam("creature_health_low"), Some(0.0));
        p.creature_health = 0.29;
        assert_eq!(p.gfparam("creature_health_low"), Some(1.0));
        assert_eq!(p.gfparam("creature_health_critical"), Some(0.0));
        p.creature_health = 0.05;
        assert_eq!(p.gfparam("creature_health_critical"), Some(1.0));
        assert!(p.health_low());
    }

    #[test]
    fn stance_and_cinematic_events_reach_the_patch() {
        let mut p = Params::default();
        assert_eq!((p.gfparam("ui_social"), p.gfparam("ui_fight")), (Some(1.0), Some(0.0)));
        p.ui_fight = true;
        assert_eq!((p.gfparam("ui_social"), p.gfparam("ui_fight")), (Some(0.0), Some(1.0)));
        p.mix.insert("crg_cin_start".into(), 1.0);
        assert_eq!(p.gfparam("crg_cin_start"), Some(1.0));
        p.socialbar = 0.4;
        assert_eq!((p.gfparam("socialbar"), p.gfparam("npcsocialbar")), (Some(0.4), Some(0.4)));
    }

    #[test]
    fn the_social_level_is_a_quarter_per_rank_point() {
        assert_eq!(social_level([0, 0, 0, 0]), 0.0);
        assert_eq!(social_level([1, 0, 0, 0]), 0.25);
        assert_eq!(social_level([3, 2, 1, 0]), 1.5);
    }

    #[test]
    fn migration_is_active_only_with_a_point() {
        let mut p = Params::default();
        assert!(!p.migrating());
        p.distance_to_migration_point = 120.0;
        assert!(p.migrating());
    }

    #[test]
    fn the_loop_plan_follows_the_parameters_the_patch_reads() {
        let calm = plan(false, false, &Params::default());
        assert_eq!(calm.music.unwrap().cues, vec![Cue::CreatureNotes]);
        assert_eq!(calm.bed.unwrap().cues, vec![Cue::CreatureAmbienceDawn]);
        assert!(calm.bed2.is_none());
        let hurt = Params { creature_health: 0.2, ..default() };
        assert_eq!(plan(false, false, &hurt).bed2.unwrap().cues, vec![Cue::CreatureHeartbeat]);
        let fight = Params { infight: true, ..default() };
        assert_eq!(plan(false, false, &fight).music.unwrap().cues, vec![Cue::CreatureFight]);
        assert_eq!(plan(true, false, &Params::default()).music.unwrap().cues, vec![Cue::CreatureFight]);
        let moving = Params { distance_to_migration_point: 50.0, ..default() };
        assert_eq!(plan(false, false, &moving).music.unwrap().cues, vec![Cue::CreatureMigrate]);
        assert_eq!(plan(false, true, &Params::default()).music.unwrap().cues, vec![Cue::CreatureMigrate]);
        // The fight beats the migration.
        assert_eq!(plan(true, true, &moving).music.unwrap().cues, vec![Cue::CreatureFight]);
    }

    #[test]
    fn the_rain_input_is_one_inside_the_storm_and_zero_outside() {
        let at = Vec3::new(10.0, 0.0, 10.0);
        assert_eq!(rain_density(None, at), 0.0);
        assert_eq!(rain_density(Some(at), at), 1.0);
        assert_eq!(rain_density(Some(at + Vec3::X * (STORM_RADIUS - 1.0)), at), 1.0);
        assert_eq!(rain_density(Some(at + Vec3::X * (STORM_RADIUS + 1.0)), at), 0.0);
        let mut p = Params::default();
        assert_eq!((p.gfparam("rain_density"), p.gfparam("crg_meteors"), p.gfparam("missioncomplete")), (Some(0.0), Some(0.0), Some(0.0)));
        p.rain_density = 1.0;
        p.meteors = true;
        assert_eq!((p.gfparam("rain_density"), p.gfparam("crg_meteors")), (Some(1.0), Some(1.0)));
        // The patch's other event inputs: only the cinematics' mixer events remain undriven here.
        assert!(!UNDRIVEN.contains(&"crg_meteors") && !UNDRIVEN.contains(&"missioncomplete"));
        assert_eq!(p.gfparam("crg_ufo"), Some(0.0));
        assert_eq!(p.gfparam("not_a_param"), None);
    }

    #[test]
    fn the_event_inputs_follow_the_running_events() {
        use crate::creature_events::{EventKind, EventRuntime};
        let mut app = creature_app();
        let here = {
            let world = app.world_mut();
            world.query_filtered::<&Transform, With<Player>>().iter(world).next().map(|t| t.translation).unwrap_or_default()
        };
        let params = |app: &App| app.world().resource::<CreatureAudioParams>().0.clone();
        assert!(!params(&app).meteors && params(&app).rain_density == 0.0);
        // A meteor strike runs: `crg_meteors` is 1 until it stops.
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::MeteorStrike, true, None);
        app.update();
        assert!(params(&app).meteors);
        assert_eq!(params(&app).gfparam("crg_meteors"), Some(1.0));
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::MeteorStrike, false, None);
        app.update();
        assert!(!params(&app).meteors);
        // A rain storm began on the avatar: inside; then the avatar is far from it; then it ends.
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::RainStorm, true, Some(here));
        app.update();
        assert_eq!(params(&app).rain_density, 1.0);
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::RainStorm, true, Some(here + Vec3::X * 500.0));
        app.update();
        assert_eq!(params(&app).rain_density, 0.0);
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::RainStorm, true, Some(here));
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::RainStorm, false, Some(here));
        app.update();
        assert_eq!(params(&app).rain_density, 0.0, "a storm that ended is no storm");
    }

    fn creature_app() -> App {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        for _ in 0..4 {
            app.update();
        }
        app
    }

    #[test]
    fn the_published_parameters_follow_the_game() {
        let mut app = creature_app();
        let p = app.world().resource::<CreatureAudioParams>().0.clone();
        assert_eq!(p.creature_health, 1.0);
        assert_eq!(p.brain_level, 0.0);
        // Hurt, in the combat stance, with a brain level and a migration.
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&mut Health, With<Player>>();
            for mut h in q.iter_mut(world) {
                h.hp = h.max * 0.2;
            }
            world.resource_mut::<Encounter>().mode = EncounterMode::Combat;
            world.resource_mut::<Encounter>().target = Some(1);
        }
        app.update();
        let p = app.world().resource::<CreatureAudioParams>().0.clone();
        assert!((p.creature_health - 0.2).abs() < 0.01, "{}", p.creature_health);
        assert!(p.infight && p.ui_fight);
        assert_eq!(p.gfparam("creature_health_low"), Some(1.0));
        app.world_mut().resource_mut::<RunProgress>().creature_progression.brain_level = 2;
        app.update();
        assert_eq!(app.world().resource::<CreatureAudioParams>().0.brain_level, 2.0);
    }
}
