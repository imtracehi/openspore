//! Original Spore music, ambience and sound effects.
//!
//! Spore names every sound. A `soundProp` (type 0x02B9F662) or SNR sample
//! (type 0x01A527DB) in the `audio~` group (0x021407EE) has the lowercase
//! FNV-1 hash of its name as instance ID. soundProps inherit through
//! `parentAudio` and list `samples` (one is picked per play) plus gain,
//! loop, pitch and fade parameters; streamed music keeps its blocks in an
//! SNS record (0x01EEF63A) with the same instance.
//!
//! Startup only filters the already-loaded resource index (no extra I/O and
//! nothing decoded). Each cue is resolved and decoded the first time it is
//! wanted on one of two background workers (music/ambience, effects/UI),
//! then kept as an in-memory `AudioSource`: no files are written anywhere.
//!
//! Spore's adaptive score runs Pure Data patches (`pd` records). Where a
//! patch only plays fixed stems (`play music_cell_loopa`, `play solar_long`,
//! `play music_shopping_sample`...) the same stems are used here, rotated
//! and crossfaded the way the patch does. The generative note patches
//! (`music_planet`, the creator scores) cannot run without a Pd engine; the
//! creature stage approximates them with sparse random notes from the
//! patch's own instrument banks over the authored planet ambience, and the
//! creators use the patch-independent shopping score.
//!
//! Gameplay is observed, never modified: systems watch `RunProgress`,
//! stage resources and the player's `Health` and request cues through
//! [`PlayCue`] messages.

use crate::{
    asset_catalog::ResourceIndex,
    audio::{self, DecodeWindow},
    cell, civ, creature_actions,
    dbpf::{self, DbpfEntry},
    editor::EditorState,
    game::{Health, Player, RunProgress},
    prop, space,
    stages::GameStage,
    tribal,
    world_objects::name_id,
    Originals,
};
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings, Volume,
};
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use rand::{seq::SliceRandom, Rng};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

/// `audio~` group shared by every audio resource.
pub const AUDIO_GROUP: u32 = 0x0214_07EE;
pub const TYPE_SNR: u32 = 0x01A5_27DB;
pub const TYPE_SNS: u32 = 0x01EE_F63A;
pub const TYPE_SOUND_PROP: u32 = 0x02B9_F662;
/// soundProp property IDs (SporeModder-FX reg_property.txt).
const PROP_PARENT: u32 = 0x5F63_17D5;
const PROP_SAMPLES: u32 = 0x701E_D91E;
const PROP_GAIN: u32 = 0x25DF_0108;
const PROP_LOOPED: u32 = 0x6DD0_8218;
const PROP_PITCH: u32 = 0x71BC_3009;
const PROP_FADE_IN: u32 = 0x98AC_8996;
const PROP_FADE_OUT: u32 = 0xE8AB_99B9;
/// Positional playback and voice limits (soundProp `is3d`, `maxdistance`,
/// `mindistance`, `polyphony`, `startdelay`).
const PROP_IS_3D: u32 = 0x12D2_A4D4;
const PROP_MAX_DISTANCE: u32 = 0x0359_3710;
const PROP_MIN_DISTANCE: u32 = 0x0F61_6B72;
const PROP_POLYPHONY: u32 = 0xBAF7_F4F9;
const PROP_START_DELAY: u32 = 0x29E8_B9F8;
const PROP_ATTENUATION: u32 = 0x1E4D_B1EB;
// Mixer groups of a sound (`groups` / `addgroups` / `removegroups`, as `pd::library::PdLibrary::sound_groups`).
const PROP_GROUPS: u32 = 0x7CB8_1A35;
const PROP_ADD_GROUPS: u32 = 0xF58C_A0BE;
const PROP_REMOVE_GROUPS: u32 = 0x1404_3549;

/// Mix levels (linear). A voice plays at
/// `MASTER_VOLUME * bus volume * authored soundProp gain * cue trim`.
pub const MASTER_VOLUME: f32 = 0.85;
pub const MUSIC_VOLUME: f32 = 0.55;
pub const AMBIENCE_VOLUME: f32 = 0.45;
pub const SFX_VOLUME: f32 = 0.9;
pub const UI_VOLUME: f32 = 0.7;
/// Toggles all game audio.
pub const MUTE_KEY: KeyCode = KeyCode::F9;
/// Simultaneous one-shot effects.
const MAX_SFX_VOICES: usize = 24;
/// A request whose sound is still decoding plays if ready within this time.
const LATE_PLAY_SECS: f64 = 0.75;
/// Music reacts to danger for this long after the last hit.
const DANGER_HOLD_SECS: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    Music,
    Ambience,
    Sfx,
    Ui,
    /// Creature / citizen voices (the Settings "Voices" slider; same mix level as `Sfx`).
    Voice,
}

impl Bus {
    pub fn volume(self) -> f32 {
        // The player's Music / SFX volumes (settings dialog, `front_end`).
        MASTER_VOLUME
            * crate::front_end::settings::bus_gain(self)
            * match self {
                Bus::Music => MUSIC_VOLUME,
                Bus::Ambience => AMBIENCE_VOLUME,
                Bus::Sfx | Bus::Voice => SFX_VOLUME,
                Bus::Ui => UI_VOLUME,
            }
    }
}

/// How a cue maps onto original resources.
#[derive(Clone, Copy, Debug)]
pub struct CueSpec {
    /// OpenSpore key (logs and CHANGES.md).
    pub key: &'static str,
    /// Original soundProp/SNR names. The first that resolves and decodes
    /// wins; with `pool` every name contributes one clip instead.
    pub names: &'static [&'static str],
    pub bus: Bus,
    /// Extra gain on top of the authored one.
    pub trim: f32,
    /// Minimum seconds between two plays (effects).
    pub min_interval: f32,
    /// Long tracks: decode `len` seconds from a random start below
    /// `max_start` seconds (the Pd patches seek randomly the same way).
    pub window: Option<(f32, f32)>,
    /// Authored sample variants decoded for random picks.
    pub variants: usize,
    pub pool: bool,
}

impl CueSpec {
    const DEFAULT: CueSpec = CueSpec {
        key: "",
        names: &[],
        bus: Bus::Sfx,
        trim: 1.0,
        min_interval: 0.08,
        window: None,
        variants: 4,
        pool: false,
    };
}

macro_rules! cues {
    ($($variant:ident = $key:literal, $bus:ident, [$($name:literal),+ $(,)?] $(, $field:ident: $value:expr)*;)+) => {
        /// Every sound OpenSpore plays (see CHANGES.md for the full table).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Cue { $($variant),+ }
        impl Cue {
            pub const ALL: &'static [Cue] = &[$(Cue::$variant),+];
            pub fn spec(self) -> CueSpec {
                match self {
                    $(Cue::$variant => CueSpec {
                        key: $key,
                        names: &[$($name),+],
                        bus: Bus::$bus,
                        $($field: $value,)*
                        ..CueSpec::DEFAULT
                    }),+
                }
            }
        }
    };
}

cues! {
    // --- Music (score stems, stings) -------------------------------------
    MenuGalaxy = "music.menu.galaxy", Music, ["galaxy_long"], window: Some((300.0, 150.0)), trim: 0.7, variants: 1;
    CellIntro = "music.cell.intro", Music, ["music_gge_cell_trans"], variants: 1;
    CreatureNotes = "music.creature.notes", Music, [
        "marimba_A3-1", "marimba_C4-1", "marimba_Eb4-1", "marimba_F#4-1", "marimba_A4-1",
        "marimba_C5-1", "highblip_A3", "highblip_C3", "highblip_Eb3", "highblip_Gb3",
    ], pool: true, trim: 0.32;
    CreatureFight = "music.creature.fight", Music, [
        "music_fight_taiko1", "music_fight_taiko2", "music_fight_taiko3", "music_fight_taiko4",
        "music_fight_toms2", "music_fight_toms3", "music_fight_toms4", "music_fight_toms5",
        "music_fight_bass1", "music_fight_bass2", "music_fight_cymbal1",
    ], pool: true, trim: 0.6;
    CreatureMigrate = "music.creature.migrate", Music, ["music_crg_migrate"], variants: 1;
    CreatureMating = "music.creature.mating", Music, ["matemusic"], variants: 1;
    CreatureClaimNest = "music.creature.claim_nest", Music, ["music_crg_claimnest"], variants: 1, min_interval: 20.0;
    TribalIntro = "music.tribal.intro", Music, ["music_cin_tribe_start"], variants: 1;
    TribalOpenRange = "music.tribal.open_range", Music, ["mus_trg_open_range_lp"], variants: 1;
    // The firepit power-up sting (the tribe level up), not part of the score.
    TribalCampfire = "music.tribal.campfire", Music, ["mus_trg_campfire_powerup"], variants: 1, min_interval: 20.0;
    TribalDeathSting = "music.tribal.death_sting", Music, ["mus_trg_death_sting"], variants: 1, min_interval: 6.0;
    // `music_trg_to_civ_end` is the Civilization entry sting (`CivIntro`).
    TribalToCivStart = "music.tribal.to_civ_start", Music, ["music_trg_to_civ_start"], variants: 1;
    TribalSiege = "music.tribal.raid", Music, ["music_raid"], variants: 1;
    TribalRaidFood = "music.tribal.raid_food", Music, ["music_raidfood"], variants: 1;
    CivIntro = "music.civ.intro", Music, ["music_trg_to_civ_end"], variants: 1;
    CivMilitary = "music.civ.anthem_military", Music, ["cityhall_kindaangry"], window: Some((180.0, 120.0)), variants: 1;
    CivReligious = "music.civ.anthem_religious", Music, ["cityhall_westreligious"], window: Some((0.0, 130.0)), variants: 1;
    CivEconomic = "music.civ.anthem_economic", Music, ["cityhall_coolio"], window: Some((180.0, 120.0)), variants: 1;
    SpaceIntro = "music.space.intro", Music, ["music_sfx_civ_to_space_b"], variants: 1;
    SpaceNeutral = "music.space.neutral", Music, ["music_planet_space_neutral"], variants: 1;
    SpaceSolar = "music.space.solar", Music, ["solar_long"], window: Some((300.0, 150.0)), trim: 0.7, variants: 1;
    SpaceWar = "music.space.war", Music, ["music_space_war_loopa"], variants: 1;
    CreatorScore = "music.creator.shopping", Music, ["music_shopping_sample"], window: Some((400.0, 150.0)), trim: 0.35, variants: 1;
    // --- Ambience beds ----------------------------------------------------
    MenuAmbience = "amb.menu.galaxy", Ambience, ["amb_space_galaxy_lp"], variants: 1;
    CellAmbience = "amb.cell.water", Ambience, ["cell_amb_loop"], variants: 1;
    CellularLoop = "amb.cell.cellular", Ambience, ["cellularloop"], variants: 1;
    PlanetAmbience = "amb.planet", Ambience, ["amb_t3_planetambi"], variants: 1;
    ShoreWind = "amb.planet.wind", Ambience, ["amb_shoreline_wind"], variants: 1;
    // tribal_audio plays it as one voice that follows the camera altitude.
    TribalHighAltWind = "amb.tribal.highalt_wind", Ambience, ["amb_trg_highalt_wind"], variants: 1;
    SpaceAmbience = "amb.space", Ambience, ["amb_space_inter_lp"], variants: 1;
    CreatorForest = "amb.creator.forest", Ambience, ["editor_ambience_forest"], variants: 1;
    CreatorScifi = "amb.creator.scifi", Ambience, ["editor_ambience_scifi"], variants: 1;
    // --- Cell ---------------------------------------------------------------
    CellToLand = "sfx.cell.transition", Sfx, ["sfx_cell_transition_whoosh"], variants: 1;
    // --- Creature -------------------------------------------------------
    // The retail Creature ambience (`ambience_dawn_dusk`: the stage starts at 7 h) and the
    // low-health heartbeat loop of the score patch (`sfx_heartbeat_lp`).
    CreatureAmbienceDawn = "amb.creature.dawn", Ambience, ["amb_dawn_lp"], variants: 1;
    CreatureHeartbeat = "amb.creature.heartbeat", Ambience, ["sfx_heartbeat_lp"], variants: 1;
    CreatureEatFruit = "sfx.creature.eat_fruit", Sfx, ["sfx_eat_fruit_mouth_grnd", "smot_fruit_eat_chew"], min_interval: 1.6;
    CreatureEatMeat = "sfx.creature.eat_meat", Sfx, ["sfx_eat_meat_mouth_tear", "sfx_eat_meat_mouth"], min_interval: 0.8;
    CreatureHurt = "sfx.creature.hurt", Sfx, ["bcom_woundedroxcr"], min_interval: 1.5, trim: 0.8;
    CreatureDeath = "sfx.creature.death", Sfx, ["bgen_death_downroxcr"], min_interval: 0.5, trim: 0.8;
    CreatureBefriend = "sfx.creature.befriend", Sfx, ["bbreedgrnd_celebrateRoXCr"], trim: 0.8;
    CreaturePartFound = "sfx.creature.part_found", Sfx, ["sfx_crg_unlock_part"], trim: 2.0;
    CreatureDig = "sfx.creature.dig", Sfx, ["sfx_crg_fossil_dig", "sfx_crg_dig"], trim: 1.5;
    CreatureMateCall = "sfx.creature.mate_call", Sfx, ["bcsa_actn_matecallRoXCr"], min_interval: 2.0, trim: 0.8;
    CreatureRecruit = "sfx.creature.recruit", Sfx, ["bsoc_recruit_callRoXCr"], trim: 0.8;
    // --- Tribal ---------------------------------------------------------
    TribalGather = "sfx.tribal.gather", Sfx, ["sfx_trg_action_gather", "sfx_trg_picktree"], min_interval: 1.5;
    TribalPerformStart = "sfx.tribal.perform_start", Sfx, ["sfx_trg_mode_social"];
    TribalPerformGood = "sfx.tribal.perform_good", Sfx, ["sfx_trg_action_party"];
    TribalPerformBad = "sfx.tribal.perform_bad", Ui, ["ui_attention_negative"], trim: 2.5;
    TribalAttackHit = "sfx.tribal.attack_hit", Sfx, ["sfx_trg_com_club_hi"], min_interval: 0.45, trim: 0.8;
    TribalAttackStart = "sfx.tribal.attack_start", Sfx, ["sfx_trg_action_attack", "sfx_trg_mode_attack"], min_interval: 4.0;
    TribalHutRazed = "sfx.tribal.hut_razed", Sfx, ["sfx_trg_hut_raze"];
    TribalAllied = "sfx.tribal.allied", Sfx, ["civ_ui_form_alliance"];
    TribalConquered = "sfx.tribal.conquered", Sfx, ["ui_tribe_takenover", "sfx_civ_conquer_newregime"];
    TribalGift = "sfx.tribal.gift", Sfx, ["sfx_trg_action_gift", "sfx_trg_gift_given"];
    // `ui_trg_buy_baby` is the buy-a-baby button's sound (and the hatch);
    // `sfx_trg_action_recruit` belongs to the Recruit action (below).
    TribalRecruit = "sfx.tribal.recruit", Sfx, ["ui_trg_buy_baby"];
    TribalHeal = "sfx.tribal.heal", Sfx, ["sfx_trg_action_heal"];
    TribalRaidAlarm = "sfx.tribal.raid_horn", Sfx, ["sfx_trg_raid_horn", "sfx_trg_raidfood_start"], min_interval: 5.0;
    // tribal_audio: one sound per citizen action (the rest are above: gather,
    // heal, gift, party), the stance buttons, the HUD camera buttons.
    TribalActionEat = "sfx.tribal.action_eat", Sfx, ["sfx_trg_action_eat"], min_interval: 0.4;
    TribalActionFish = "sfx.tribal.action_fish", Sfx, ["sfx_trg_action_fish"], min_interval: 0.4;
    TribalActionAttack = "sfx.tribal.action_attack", Sfx, ["sfx_trg_action_attack"], min_interval: 0.4;
    TribalActionRaid = "sfx.tribal.action_raid", Sfx, ["sfx_trg_action_raid"], min_interval: 0.4;
    TribalActionHunt = "sfx.tribal.action_hunt", Sfx, ["sfx_trg_action_hunt"], min_interval: 0.4;
    TribalActionMate = "sfx.tribal.action_mate", Sfx, ["sfx_trg_action_mate"], min_interval: 0.4;
    TribalActionCollectEgg = "sfx.tribal.action_collect_egg", Sfx, ["sfx_trg_action_collect_egg"], min_interval: 0.4;
    TribalActionBundle = "sfx.tribal.action_bundle", Sfx, ["sfx_trg_action_bundle"], min_interval: 0.4;
    TribalActionFeedWild = "sfx.tribal.action_feed_wild", Sfx, ["sfx_trg_action_feed_wild"], min_interval: 0.4;
    TribalActionRepair = "sfx.tribal.action_repair", Sfx, ["sfx_trg_action_repair"], min_interval: 0.4;
    TribalActionTame = "sfx.tribal.action_tame", Sfx, ["sfx_trg_action_tame"], min_interval: 0.4;
    TribalActionRecruit = "sfx.tribal.action_recruit", Sfx, ["sfx_trg_action_recruit"], min_interval: 0.4;
    TribalActionGatherMeat = "sfx.tribal.action_gather_meat", Sfx, ["sfx_trg_action_gather_meat"], min_interval: 0.4;
    TribalModeSocial = "ui.tribal.mode_social", Ui, ["sfx_trg_mode_social"];
    TribalModeAttack = "ui.tribal.mode_attack", Ui, ["sfx_trg_mode_attack"];
    CameraZoom = "ui.camera_zoom", Ui, ["ui_camera_zoom"], min_interval: 0.3;
    // --- Civilization ------------------------------------------------------
    CivMilitaryShot = "sfx.civ.military", Sfx, ["sfx_civ_gunshot"], min_interval: 0.5, trim: 0.8;
    CivReligiousPulse = "sfx.civ.religious", Sfx, ["civ_cultural_attack_icon"], min_interval: 1.6, trim: 0.7;
    CivEconomicOffer = "sfx.civ.economic", Sfx, ["ui_civ_target_diplo_trade"], min_interval: 1.2;
    CivCityHit = "sfx.civ.city_hit", Sfx, ["sfx_civ_damage_building"], min_interval: 3.0, trim: 0.6;
    CivCapture = "sfx.civ.capture", Sfx, ["sfx_civ_conquer_newregime", "ui_spg_city_surrender"];
    CivBuild = "sfx.civ.build", Sfx, ["sfx_civ_building_letgo_close", "ui_Road_plop"], trim: 2.5;
    CivIncome = "sfx.civ.income", Ui, ["ui_money_in", "ui_getmoney_small"], min_interval: 10.0, trim: 2.0;
    CivVehicleLost = "sfx.civ.vehicle_lost", Sfx, ["sfx_civcity_destruct_close", "colony_explode"];
    CivHurt = "sfx.civ.hurt", Sfx, ["sfx_spg_ufo_chunk_impact_close"], min_interval: 0.8;
    // Civilization controls and audio (`civ_controls`, `civ_audio`): the invalid
    // order sound, the target sounds of an order, the city alarm, mission
    // complete, the conversion music gates (streamed stems, looped by
    // `civ_audio`) and the concert / defeat music (notes\civ-re-notes.md V7).
    CivError = "ui.civ.error", Ui, ["ui_civ_error"], min_interval: 0.25;
    CivTargetMilitary = "ui.civ.target_military", Ui, ["ui_civ_target_military"], min_interval: 0.15;
    CivTargetCult = "ui.civ.target_cult", Ui, ["ui_civ_target_cult"], min_interval: 0.15;
    CivTargetTrade = "ui.civ.target_trade", Ui, ["ui_civ_target_diplo_trade"], min_interval: 0.15;
    CivUnderAttack = "sfx.civ.under_attack", Sfx, ["sfx_civ_underattack_alarm_close"], min_interval: 8.0;
    CivAlarmLoop = "ui.civ.alarm_loop", Ui, ["ui_civ_alarm_lp"], variants: 1;
    CivMissionComplete = "ui.civ.mission_complete", Ui, ["ui_mission_complete"], min_interval: 1.0;
    CivHutRotate = "ui.civ.hut_rotate", Ui, ["ui_hut_rotate"], min_interval: 0.1;
    CivReligConcert = "music.civ.relig_concert", Music, ["music_civ_relig_concert"], variants: 1, min_interval: 30.0;
    CivLose = "music.civ.lose", Music, ["music_civ_lose"], variants: 1, min_interval: 30.0;
    CivLoseSting = "sfx.civ.lose_game", Sfx, ["Sfx_civ_lose_game"], variants: 1, min_interval: 30.0;
    CivRecruitA = "music.civ.recruit_a", Music, ["music_civ_recruita"], window: Some((0.0, 90.0)), variants: 1;
    CivRecruitB = "music.civ.recruit_b", Music, ["music_civ_recruitb"], window: Some((0.0, 90.0)), variants: 1;
    CivRecruitC = "music.civ.recruit_c", Music, ["music_civ_recruitc"], window: Some((0.0, 90.0)), variants: 1;
    // --- Space ------------------------------------------------------------
    SpaceFire = "sfx.space.fire", Sfx, ["sfx_ufo_blaster_low"], min_interval: 0.1, trim: 0.7;
    SpaceHit = "sfx.space.hit", Sfx, ["sfx_ufo_blaster_hit"], min_interval: 0.3, trim: 0.6;
    SpacePirateDown = "sfx.space.pirate_destroyed", Sfx, ["sfx_ufo_blowingup"];
    SpaceHurt = "sfx.space.hurt", Sfx, ["sfx_ufo_danger_damage", "sfx_ufo_blaster_hit"], min_interval: 0.8, trim: 1.5;
    SpaceShipLost = "sfx.space.ship_lost", Sfx, ["sfx_ufo_blowingup"], min_interval: 2.0;
    SpaceScan = "sfx.space.scan", Sfx, ["ufo_scan_success", "sfx_space_radar_beep"];
    SpaceTerraform = "sfx.space.terraform", Sfx, ["sfx_space_terraform_fire"];
    SpaceColony = "sfx.space.colony", Sfx, ["sfx_spg_colonize_star"], trim: 4.0;
    SpaceDock = "sfx.space.dock", Ui, ["ui_ufo_com_open"], trim: 2.0;
    SpaceMission = "sfx.space.mission", Sfx, ["sfx_spg_mission_success", "ui_mission_complete"];
    // Space (S7, `space_audio`): the zoom whoosh, loot, scan and abduct sounds, the
    // score stems the `space_music_patch` mixes by zoom and the war patch's voices.
    SpaceZoomOut = "sfx.space.zoom_out", Ui, ["ui_camera_zoom_out"], variants: 1, min_interval: 0.4;
    // The counterpart of the whoosh above (retail 0x101AF23, audio-audit).
    SpaceZoomIn = "sfx.space.zoom_in", Ui, ["ui_camera_zoom_in"], variants: 1, min_interval: 0.4;
    SpaceLoot = "sfx.space.loot", Sfx, ["sfx_spg_loot_reward"];
    UfoScanSuccess = "sfx.space.scan_success", Sfx, ["ufo_scan_success"];
    UfoAbductCollectHit = "sfx.space.abduct_hit", Sfx, ["ufo_abduct_collect_hit"], min_interval: 0.15;
    UfoAbductSuccess = "sfx.space.abduct_success", Sfx, ["sfx_ufo_abduct_success"];
    SpaceStemGalaxy = "music.space.stem_galaxy", Music, ["galaxy_long"], window: Some((300.0, 150.0)), trim: 0.7, variants: 1;
    SpaceStemInterstellar = "music.space.stem_interstellar", Music, ["interstellar_long"], window: Some((300.0, 150.0)), trim: 0.7, variants: 1;
    SpaceWarLoopB = "music.space.war_loopb", Music, ["music_space_war_loopb"], variants: 1;
    SpaceWarLow = "music.space.war_low", Music, ["music_space_war_low"], variants: 1;
    SpaceWarEnd = "music.space.war_end", Music, ["music_space_war_end"], variants: 1, min_interval: 0.0;
    SpaceWarGong = "music.space.war_gong", Music, ["music_space_war_gong1", "music_space_war_gong2", "music_space_war_gong3"], pool: true, min_interval: 0.0;
    SpaceTradeLoop = "music.space.trade", Music, ["music_space_trade_loop"], variants: 1;
    AmbSpaceSystem = "amb.space.system", Ambience, ["amb_space_system_lp"], variants: 1;
    AmbSpaceWindFar = "amb.space.wind_far", Ambience, ["sfx_amb_wind_far_lp"], variants: 1;
    // Space HUD and comm sounds (S7): the HUD plays them through `PlayCue`.
    SpgError = "ui.spg.error", Ui, ["ui_spg_error"];
    SpaceFilterClick = "ui.space.filter_click", Ui, ["ui_space_filter_click"];
    StarmapFilterMouseoff = "ui.space.filter_mouseoff", Ui, ["sfx_starmapfilter_mouseoff"];
    // The galaxy map's star hover (0x106B340): `space` for a SETI star, `standard` for any other.
    StarmapFilterStandard = "ui.space.filter_standard", Ui, ["sfx_starmapfilter_standard"];
    StarmapFilterSpace = "ui.space.filter_space", Ui, ["sfx_starmapfilter_space"];
    SpgFoodwebSelect = "ui.spg.foodweb_select", Ui, ["ui_spg_foodweb_select"];
    SpgFoodwebDeselect = "ui.spg.foodweb_deselect", Ui, ["ui_spg_foodweb_deselect"];
    SpgFoodwebRollover = "ui.spg.foodweb_rollover", Ui, ["ui_spg_foodweb_rollover"];
    SpaceJettison = "ui.space.jettison", Ui, ["ui_space_jettison"];
    SpgMenuCategory = "ui.spg.menu_category", Ui, ["ui_spg_menu_category"];
    SpgMenuOpen = "ui.spg.menu_open", Ui, ["ui_spg_menu_open"];
    SpgMenuClose = "ui.spg.menu_close", Ui, ["ui_spg_menu_close"];
    SpgToolRollover = "ui.spg.tool_rollover", Ui, ["ui_spg_tool_rollover"];
    SpgToolDisabledRollover = "ui.spg.tool_disabled_rollover", Ui, ["ui_spg_tool_disabled_rollover"];
    BadgeDialogOpen = "ui.badge_dialog_open", Ui, ["ui_badge_dialog_open"];
    GlobalPlannerRollover = "ui.global_planner_rollover", Ui, ["ui_global_planner_rollover"];
    MyCollectionsSelect = "ui.mycollections_select", Ui, ["ui_mycollections_select"];
    MyCollectionsRollover = "ui.mycollections_rollover", Ui, ["ui_mycollections_rollover"];
    MenuScroll = "ui.menu_scroll", Ui, ["ui_menu_scroll"], min_interval: 0.05;
    UfoComSelect = "ui.ufo_com.select", Ui, ["ui_ufo_com_select"];
    UfoComTradeBuy = "ui.ufo_com.trade_buy", Ui, ["ui_ufo_com_trade_buy"];
    UfoComTradeSell = "ui.ufo_com.trade_sell", Ui, ["ui_ufo_com_trade_sell"];
    UfoComTradeIncrement = "ui.ufo_com.trade_increment", Ui, ["ui_ufo_com_trade_increment"], min_interval: 0.03;
    // Comm screen (retail 0xDD4D15 / 0xDD562D / 0xDD57E3 / 0xDD5830 swap, 0xDD5446 repair, 0xDD549D recharge,
    // 0xAEA041 the screen powers down).
    UfoComSwap = "ui.ufo_com.swap", Ui, ["ui_ufo_com_swap"];
    UfoComBuyHealth = "ui.ufo_com.buy_health", Ui, ["ui_ufo_com_buy_health"];
    UfoComBuyEnergy = "ui.ufo_com.buy_energy", Ui, ["ui_ufo_com_buy_energy"];
    UfoComPowerdown = "ui.ufo_com.powerdown", Ui, ["ui_ufo_com_powerdown"];
    // --- Interface and creators ----------------------------------------------
    UiClick = "ui.click", Ui, ["menu_click", "editor_general_click"], min_interval: 0.05;
    UiPositive = "ui.positive", Ui, ["ui_attention_positive"], trim: 2.5, min_interval: 0.5;
    CreatorClick = "ui.creator.click", Ui, ["editor_click", "editor_general_click"], min_interval: 0.05;
    CreatorOpen = "ui.creator.open", Ui, ["editor_trans_whoosh"], variants: 1, trim: 0.6;
    CreatorClose = "ui.creator.close", Ui, ["editor_test_transition", "editor_mode_switch"], trim: 2.0;
    CreatorPartPlaced = "ui.creator.part_placed", Ui, ["editor_snap", "editor_part_letgo"], min_interval: 0.1;
    // Selling a part: `editor_partsell` (the name table 0xB3BF1C.., sold parts 0xD11128 / 0xD1144A / 0xD118BF;
    // `editor_trash` belongs to the Test Drive photo tray, `editor_delete` is no retail code name).
    CreatorPartRemoved = "ui.creator.part_removed", Ui, ["editor_partsell"], min_interval: 0.1;
    // The Cell Creator's part clips (`sfx_cell_ed_*`), one per part kind. The
    // pooled ones are raw samples (gain 1): trimmed to the authored 0.3-0.4 of
    // the soundProps of the mouths and weapons.
    CellEdFilter = "sfx.cell_ed.filter", Sfx, ["sfx_cell_ed_mouth_filt"], variants: 1;
    CellEdJaw = "sfx.cell_ed.jaw", Sfx, ["sfx_cell_ed_mouth_jaw"], variants: 1;
    CellEdProboscis = "sfx.cell_ed.proboscis", Sfx, ["sfx_cell_ed_mouth_prob"], variants: 1;
    CellEdEyes = "sfx.cell_ed.eyes", Sfx, ["sfx_cell_ed_eyes1", "sfx_cell_ed_eyes2"], pool: true, trim: 0.4;
    CellEdFlagella = "sfx.cell_ed.flagella", Sfx, ["sfx_cell_ed_move_flag1", "sfx_cell_ed_move_flag2", "sfx_cell_ed_move_flag3"], pool: true, trim: 0.4;
    CellEdCilia = "sfx.cell_ed.cilia", Sfx, ["sfx_cell_ed_move_cil1", "sfx_cell_ed_move_cil2", "sfx_cell_ed_move_cil3"], pool: true, trim: 0.4;
    CellEdJet = "sfx.cell_ed.jet", Sfx, ["sfx_cell_ed_move_jet1", "sfx_cell_ed_move_jet2"], pool: true, trim: 0.4;
    CellEdSpike = "sfx.cell_ed.spike", Sfx, ["sfx_cell_ed_spike1", "sfx_cell_ed_spike2", "sfx_cell_ed_spike3"], pool: true, trim: 0.4;
    CellEdElectric = "sfx.cell_ed.electric", Sfx, ["sfx_cell_ed_attack_elec"], variants: 1;
    CellEdPoison = "sfx.cell_ed.poison", Sfx, ["sfx_cell_ed_attack_poison"], variants: 1;
    // Verb tray clips (`ed_cell_*` ANIMs): the `sound` events they carry.
    CellEdVtrayCilia = "sfx.cell_ed.vtray.cilia", Sfx, ["sfx_cell_ed_vtray_cil"], variants: 1;
    CellEdVtrayFlagella = "sfx.cell_ed.vtray.flagella", Sfx, ["sfx_cell_ed_vtray_flg"], variants: 1;
    CellEdVtrayJet = "sfx.cell_ed.vtray.jet", Sfx, ["sfx_cell_ed_vtray_jet"], variants: 1;
    CellEdVtraySpike = "sfx.cell_ed.vtray.spike", Sfx, ["sfx_cell_ed_vtray_spike"], variants: 1;
    CellEdVtrayCrack = "sfx.cell_ed.vtray.crack", Sfx, ["sfx_cell_ed_vtray_crack"], variants: 1;
    CellEdVtrayElectric = "sfx.cell_ed.vtray.electric", Sfx, ["sfx_cell_ed_vtray_elec"], variants: 1;
    CellEdVtrayPoison = "sfx.cell_ed.vtray.poison", Sfx, ["sfx_cell_ed_vtray_posion"], variants: 1;
}

/// Effects worth decoding as soon as their stage starts.
pub fn stage_cues(stage: &GameStage) -> &'static [Cue] {
    use Cue::*;
    match stage {
        GameStage::MainMenu => &[UiClick, UiPositive],
        // The Cell sounds are decoded by `cell_audio` (animation events and
        // effects name them); only the stage-exit whoosh is a cue.
        GameStage::Cell => &[CellToLand],
        GameStage::Creature => &[
            CreatureEatFruit,
            CreatureEatMeat,
            CreatureHurt,
            CreatureDeath,
            CreatureBefriend,
            CreaturePartFound,
            CreatureDig,
            CreatureMateCall,
            CreatureRecruit,
        ],
        GameStage::Tribal => &[
            TribalGather,
            TribalPerformStart,
            TribalPerformGood,
            TribalPerformBad,
            TribalAttackHit,
            TribalAttackStart,
            TribalHutRazed,
            TribalAllied,
            TribalConquered,
            TribalGift,
            TribalRecruit,
            TribalHeal,
            TribalRaidAlarm,
            TribalActionEat,
            TribalActionFish,
            TribalActionAttack,
            TribalActionRaid,
            TribalActionHunt,
            TribalActionCollectEgg,
            TribalActionBundle,
            TribalActionFeedWild,
            TribalActionRepair,
            TribalActionTame,
            TribalActionRecruit,
            TribalActionGatherMeat,
            TribalModeSocial,
            TribalModeAttack,
            CameraZoom,
            CreatureHurt,
        ],
        GameStage::Civilization => &[
            CivMilitaryShot,
            CivReligiousPulse,
            CivEconomicOffer,
            CivCityHit,
            CivCapture,
            CivBuild,
            CivIncome,
            CivVehicleLost,
            CivHurt,
            CivError,
            CivTargetMilitary,
            CivTargetCult,
            CivTargetTrade,
            CivUnderAttack,
            CivMissionComplete,
        ],
        GameStage::Space => &[
            SpaceFire,
            SpaceHit,
            SpacePirateDown,
            SpaceHurt,
            SpaceShipLost,
            SpaceScan,
            SpaceTerraform,
            SpaceColony,
            SpaceDock,
            SpaceMission,
            SpaceZoomOut,
            SpaceZoomIn,
            SpaceLoot,
            SpgError,
        ],
    }
}

// ---------------------------------------------------------------------------
// Installed audio index and resolution (no Bevy, testable)
// ---------------------------------------------------------------------------

/// Name-hash index of the installed audio records, cut from the resource
/// index the game already loaded (patch priority included).
#[derive(Default)]
pub struct AudioLibrary {
    records: HashMap<(u32, u32), (PathBuf, DbpfEntry)>,
}

fn decode_window_with_fallback(
    snr: &[u8],
    sns: Option<&[u8]>,
    window: Option<DecodeWindow>,
) -> Result<audio::DecodedSound, String> {
    match audio::decode_snr_sound(snr, sns, window) {
        Ok(sound) => Ok(sound),
        Err(window_error) => {
            let Some(window) = window else {
                return Err(window_error);
            };
            // Some long EA Layer 3 streams cannot be decoded from every
            // random block boundary. Retry the same installed sample from
            // its opening window before rejecting it so load_cue can move
            // on to another authored sample variant if needed.
            audio::decode_snr_sound(
                snr,
                sns,
                Some(DecodeWindow {
                    start: 0,
                    len: window.len,
                }),
            )
            .map_err(|fallback_error| {
                format!(
                    "random window failed ({window_error}); opening window failed ({fallback_error})"
                )
            })
        }
    }
}

/// A resolved sound: authored playback parameters plus its SNR samples.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundDef {
    pub name: String,
    /// SNR instance IDs; the game picks one at random per play.
    pub samples: Vec<u32>,
    pub gain: f32,
    pub looped: bool,
    pub pitch: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    /// soundProps walked through `parentAudio` (0 = bare SNR sample).
    pub inheritance: usize,
    /// Positional sound: full level inside `min_distance`, silent beyond
    /// `max_distance` (retail units around the listener).
    pub is_3d: bool,
    pub min_distance: f32,
    pub max_distance: f32,
    /// Most simultaneous voices of this sound (0 = unlimited).
    pub polyphony: u32,
    /// Seconds between the request and the first sample.
    pub start_delay: f32,
    /// soundProp `attenuation` (0x1E4DB1EB): (x, gain) points of the
    /// roll-off between `min_distance` and `max_distance`; empty = linear.
    pub curve: Vec<(f32, f32)>,
    /// The soundProp sets `gain` 0 itself (34 installed sounds, e.g.
    /// `sfx_cell_explosion`): played directly by an event it is silent. The
    /// mixed `gain` above maps it to 1 for patch-driven loops, whose level a
    /// Pure Data patch sets.
    pub silent: bool,
    /// Mixer groups the soundProp chain puts the sound in (`groups` of the
    /// nearest definer, every `addgroups`, minus `removegroups`; ids of the
    /// group names). The Pure Data mixers (`mixer chase_group` ...) scale every
    /// sound of a group, patch voice or not (VERIFIED: Cell `_cell_vfx_base`,
    /// `_cell_col_base`, `_cell_vox_*` add `chase_group`).
    pub groups: Vec<u32>,
}

impl SoundDef {
    /// Level of a positional sound at `distance` from the listener: full
    /// inside `min_distance`, silent beyond `max_distance`, the soundProp's
    /// `attenuation` curve in between (VERIFIED, track vtable +0xB0 at
    /// 0xA41C10: x = (distance - min) / (max - min) of the distance in world
    /// units, evaluated with the piecewise linear curve evaluator 0xA1A090;
    /// linear without a curve); non-positional sounds are not attenuated.
    pub fn attenuation(&self, distance: f32) -> f32 {
        if !self.is_3d || self.max_distance <= self.min_distance {
            return 1.0;
        }
        let x = ((distance - self.min_distance) / (self.max_distance - self.min_distance)).clamp(0.0, 1.0);
        if self.curve.len() < 2 {
            return 1.0 - x;
        }
        // The soundProp's own roll-off (same reading as `pd::library::Spatial`).
        crate::pd::library::Spatial { min: 0.0, max: 1.0, curve: self.curve.clone() }.gain(x)
    }
}

impl AudioLibrary {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let records = index
            .records
            .iter()
            .filter(|(k, _)| {
                k.group_id == AUDIO_GROUP
                    && matches!(k.type_id, TYPE_SNR | TYPE_SNS | TYPE_SOUND_PROP)
            })
            .map(|(k, v)| ((k.type_id, k.instance_id), v.clone()))
            .collect();
        Self { records }
    }

    pub fn count(&self, type_id: u32) -> usize {
        self.records.keys().filter(|(t, _)| *t == type_id).count()
    }

    pub fn contains(&self, type_id: u32, instance: u32) -> bool {
        self.records.contains_key(&(type_id, instance))
    }

    pub(crate) fn read(&self, type_id: u32, instance: u32) -> Result<Vec<u8>, String> {
        let (path, entry) = self
            .records
            .get(&(type_id, instance))
            .ok_or_else(|| format!("missing audio record {instance:08x}.{type_id:08x}"))?;
        if entry.mem_size > 64 * 1024 * 1024 {
            return Err("audio record exceeds decode limit".into());
        }
        dbpf::read_entry_bytes(path, entry)
    }

    /// Resolve an original sound name: a soundProp (with inherited
    /// properties) or a bare SNR sample.
    pub fn resolve(&self, name: &str) -> Result<SoundDef, String> {
        // `#XXXXXXXX` names a sound by its id (effect sound components and
        // animation events only carry the hash).
        let id = match name.strip_prefix('#') {
            Some(hex) => u32::from_str_radix(hex, 16).unwrap_or(0),
            None => name_id(name),
        };
        if !self.contains(TYPE_SOUND_PROP, id) {
            if self.contains(TYPE_SNR, id) {
                return Ok(SoundDef {
                    name: name.into(),
                    samples: vec![id],
                    gain: 1.0,
                    looped: false,
                    pitch: 1.0,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    inheritance: 0,
                    is_3d: false,
                    min_distance: 0.0,
                    max_distance: 0.0,
                    polyphony: 0,
                    start_delay: 0.0,
                    curve: vec![],
                    silent: false,
                    groups: vec![],
                });
            }
            return Err(format!("{name}: no soundProp or SNR {id:08x} installed"));
        }
        // Child values win over inherited ones.
        let mut merged: HashMap<u32, prop::Property> = HashMap::new();
        let mut seen = HashSet::new();
        let mut current = id;
        // (groups, addgroups, removegroups) of every level, child first.
        let mut group_levels: Vec<[Option<Vec<u32>>; 3]> = vec![];
        while seen.insert(current) && seen.len() <= 16 {
            let Ok(bytes) = self.read(TYPE_SOUND_PROP, current) else {
                break;
            };
            let list = prop::parse(&bytes).map_err(|e| format!("{name}: soundProp: {e}"))?;
            let parent = list.0.get(&PROP_PARENT).and_then(first_instance);
            let instances = |p: u32| {
                list.0.get(&p).map(|v| {
                    v.values.iter().filter_map(|b| Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))).collect::<Vec<u32>>()
                })
            };
            group_levels.push([instances(PROP_GROUPS), instances(PROP_ADD_GROUPS), instances(PROP_REMOVE_GROUPS)]);
            for (k, v) in list.0 {
                merged.entry(k).or_insert(v);
            }
            match parent {
                Some(p) if p != 0 => current = p,
                _ => break,
            }
        }
        let mut samples: Vec<u32> = merged
            .get(&PROP_SAMPLES)
            .map(|p| {
                p.values
                    .iter()
                    .filter_map(|v| Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?)))
                    .filter(|s| self.contains(TYPE_SNR, *s))
                    .collect()
            })
            .unwrap_or_default();
        if samples.is_empty() && !merged.contains_key(&PROP_SAMPLES) && self.contains(TYPE_SNR, id) {
            // A soundProp without `samples` plays the SNR of its own name
            // (`sfx_heartbeat_lp`).
            samples.push(id);
        }
        if samples.is_empty() {
            return Err(format!(
                "{name}: soundProp has no SNR samples (Pure Data patch or empty)"
            ));
        }
        let number = |prop: u32, default: f32| {
            merged
                .get(&prop)
                .and_then(|p| p.number())
                .filter(|n| n.is_finite())
                .unwrap_or(default)
        };
        let gain = number(PROP_GAIN, 1.0);
        // From the root template down so children override (the order of
        // `PdLibrary::sound_groups`).
        let mut groups: Vec<u32> = vec![];
        for [set, add, remove] in group_levels.iter().rev() {
            if let Some(g) = set {
                groups = g.clone();
            }
            for k in add.iter().flatten() {
                if !groups.contains(k) {
                    groups.push(*k);
                }
            }
            if let Some(rm) = remove {
                groups.retain(|k| !rm.contains(k));
            }
        }
        Ok(SoundDef {
            name: name.into(),
            samples,
            // Gain 0 marks levels the native mixer drives at runtime.
            gain: if gain > 0.0 { gain.min(4.0) } else { 1.0 },
            looped: number(PROP_LOOPED, 0.0) != 0.0,
            pitch: number(PROP_PITCH, 1.0).clamp(0.25, 4.0),
            fade_in: number(PROP_FADE_IN, 0.0).clamp(0.0, 30.0),
            fade_out: number(PROP_FADE_OUT, 0.0).clamp(0.0, 30.0),
            inheritance: seen.len(),
            is_3d: number(PROP_IS_3D, 0.0) != 0.0,
            min_distance: number(PROP_MIN_DISTANCE, 0.0).max(0.0),
            max_distance: number(PROP_MAX_DISTANCE, 0.0).max(0.0),
            polyphony: number(PROP_POLYPHONY, 0.0).max(0.0) as u32,
            start_delay: number(PROP_START_DELAY, 0.0).clamp(0.0, 10.0),
            curve: merged
                .get(&PROP_ATTENUATION)
                .map(|p| {
                    let v: Vec<f32> = p
                        .values
                        .iter()
                        .filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?)))
                        .collect();
                    v.chunks_exact(2).map(|c| (c[0], c[1])).collect()
                })
                .unwrap_or_default(),
            silent: gain <= 0.0,
            groups,
        })
    }

    /// Decode one SNR sample (with its SNS stream). `window` is
    /// `(max random start s, length s)` for long tracks.
    pub fn decode(
        &self,
        sample: u32,
        window: Option<(f32, f32)>,
        rng: &mut impl Rng,
    ) -> Result<audio::DecodedSound, String> {
        let snr = self.read(TYPE_SNR, sample)?;
        let layout = audio::parse_snr_layout(&snr).ok_or("not an EA SNR v0 record")?;
        let sns = if layout.storage == audio::SnrStorage::Ram {
            None
        } else {
            Some(self.read(TYPE_SNS, sample)?)
        };
        let rate = layout.header.sample_rate as f32;
        let total = layout.header.num_samples;
        let window = window.and_then(|(max_start, len)| {
            let len = (len * rate) as usize;
            if total <= len + 1 {
                return None;
            }
            let latest = ((max_start * rate) as usize).min(total - len);
            Some(DecodeWindow {
                start: if latest == 0 {
                    0
                } else {
                    rng.random_range(0..=latest)
                },
                len,
            })
        });
        let mut sound = decode_window_with_fallback(&snr, sns.as_deref(), window)?;
        if window.is_some() {
            sound.fade_edges(2.0);
        }
        Ok(sound)
    }
}

fn first_instance(p: &prop::Property) -> Option<u32> {
    Some(u32::from_le_bytes(
        p.values.first()?.get(..4)?.try_into().ok()?,
    ))
}

/// One decoded variant of a cue, as WAV bytes.
pub struct ClipData {
    pub sample: u32,
    pub wav: Vec<u8>,
    pub secs: f32,
    pub peak: i32,
}

/// Resolve and decode a cue on a worker thread.
pub fn load_cue(
    library: &AudioLibrary,
    spec: &CueSpec,
    rng: &mut impl Rng,
) -> Result<(SoundDef, Vec<ClipData>), String> {
    let clip = |sample: u32, rng: &mut _| {
        library.decode(sample, spec.window, rng).map(|s| ClipData {
            sample,
            wav: s.to_wav(),
            secs: s.secs(),
            peak: s.peak(),
        })
    };
    if spec.pool {
        let mut clips = Vec::new();
        let mut errors = Vec::new();
        for name in spec.names {
            match library
                .resolve(name)
                .and_then(|d| clip(d.samples[0], &mut *rng))
            {
                Ok(c) => clips.push(c),
                Err(e) => errors.push(e),
            }
        }
        if clips.is_empty() {
            return Err(errors.join("; "));
        }
        let def = SoundDef {
            name: format!("{} (+{} notes)", spec.names[0], clips.len() - 1),
            samples: clips.iter().map(|c| c.sample).collect(),
            gain: 1.0,
            looped: false,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 0,
            is_3d: false,
            min_distance: 0.0,
            max_distance: 0.0,
            polyphony: 0,
            start_delay: 0.0,
            curve: vec![],
            silent: false,
            groups: vec![],
        };
        return Ok((def, clips));
    }
    let mut errors = Vec::new();
    for name in spec.names {
        let def = match library.resolve(name) {
            Ok(def) => def,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let mut samples = def.samples.clone();
        samples.shuffle(&mut *rng);
        let mut clips = Vec::new();
        for sample in samples {
            if clips.len() >= spec.variants.max(1) {
                break;
            }
            match clip(sample, &mut *rng) {
                Ok(c) => clips.push(c),
                Err(e) => errors.push(format!("{name} sample {sample:08x}: {e}")),
            }
        }
        if !clips.is_empty() {
            return Ok((def, clips));
        }
    }
    Err(errors.join("; "))
}

// ---------------------------------------------------------------------------
// Background decoding
// ---------------------------------------------------------------------------

struct Ready {
    cue: Cue,
    result: Result<(SoundDef, Vec<ClipData>), String>,
    millis: u128,
}

fn spawn_worker(
    name: &str,
    library: Arc<AudioLibrary>,
    done: mpsc::Sender<Ready>,
) -> Option<mpsc::Sender<Cue>> {
    let (jobs, queue) = mpsc::channel::<Cue>();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let mut rng = rand::rng();
            for cue in queue {
                let started = std::time::Instant::now();
                let result = load_cue(&library, &cue.spec(), &mut rng);
                let millis = started.elapsed().as_millis();
                if done
                    .send(Ready {
                        cue,
                        result,
                        millis,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .ok()?;
    Some(jobs)
}

// ---------------------------------------------------------------------------
// Bevy integration
// ---------------------------------------------------------------------------

/// Request a one-shot cue (effects, UI, stingers).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayCue(pub Cue);

struct LoadedCue {
    def: SoundDef,
    clips: Vec<(Handle<AudioSource>, f32)>,
}

/// One decoded clip of a cue and the soundProp's playback parameters.
#[derive(Clone, Debug)]
pub struct CueClip {
    pub handle: Handle<AudioSource>,
    pub secs: f32,
    pub gain: f32,
    pub pitch: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    /// How many clips the cue decoded.
    pub clips: usize,
}

enum CueState {
    Pending,
    Ready(Arc<LoadedCue>),
    Failed,
}

#[derive(Resource, Default)]
pub struct GameAudio {
    pub muted: bool,
    /// One-line index summary for diagnostics.
    pub summary: String,
    /// Last requested one-shot cues (diagnostics and tests).
    pub recent: VecDeque<Cue>,
    library: Option<Arc<AudioLibrary>>,
    music_jobs: Option<mpsc::Sender<Cue>>,
    sfx_jobs: Option<mpsc::Sender<Cue>>,
    results: Option<Mutex<mpsc::Receiver<Ready>>>,
    cues: HashMap<Cue, CueState>,
    last_played: HashMap<Cue, f64>,
    waiting: Vec<(Cue, f64)>,
}

impl GameAudio {
    /// Queue a cue for decoding once (no-op when cached, pending or failed).
    pub fn request(&mut self, cue: Cue) {
        if self.cues.contains_key(&cue) {
            return;
        }
        let jobs = match cue.spec().bus {
            Bus::Music | Bus::Ambience => self.music_jobs.as_ref(),
            Bus::Sfx | Bus::Ui | Bus::Voice => self.sfx_jobs.as_ref(),
        };
        let sent = jobs.is_some_and(|j| j.send(cue).is_ok());
        self.cues.insert(
            cue,
            if sent {
                CueState::Pending
            } else {
                CueState::Failed
            },
        );
    }

    fn loaded(&self, cue: Cue) -> Option<Arc<LoadedCue>> {
        match self.cues.get(&cue) {
            Some(CueState::Ready(l)) => Some(l.clone()),
            _ => None,
        }
    }

    fn failed(&self, cue: Cue) -> bool {
        matches!(self.cues.get(&cue), Some(CueState::Failed))
    }

    pub fn is_ready(&self, cue: Cue) -> bool {
        self.loaded(cue).is_some()
    }

    /// The decoded first clip of a cue and its authored gain, for modules that
    /// run their own voice (`tribal_audio`'s camera-driven wind, `civ_audio`'s alarm and
    /// conversion loops).
    pub fn clip(&self, cue: Cue) -> Option<(Handle<AudioSource>, f32)> {
        self.loaded(cue)
            .map(|loaded| (loaded.clips[0].0.clone(), loaded.def.gain))
    }

    /// The decoded clip of a cue (the first when `pick` is out of range) with
    /// its authored playback parameters, for modules that run their own
    /// voices (`space_audio`).
    pub fn clip_full(&self, cue: Cue, pick: usize) -> Option<CueClip> {
        let loaded = self.loaded(cue)?;
        let (handle, secs) = loaded.clips.get(pick).or(loaded.clips.first())?.clone();
        Some(CueClip {
            handle,
            secs,
            gain: loaded.def.gain,
            pitch: loaded.def.pitch,
            fade_in: loaded.def.fade_in,
            fade_out: loaded.def.fade_out,
            clips: loaded.clips.len(),
        })
    }

    /// The cue could not be decoded (or there is no audio library).
    pub fn is_failed(&self, cue: Cue) -> bool {
        self.failed(cue)
    }

    /// The installed audio index (`None` until startup found audio records).
    /// Other audio modules (`cell_audio`) resolve and decode sounds by name
    /// with it.
    pub fn library(&self) -> Option<Arc<AudioLibrary>> {
        self.library.clone()
    }

    /// Drop decoded music/ambience the current plan no longer uses (voices
    /// still playing keep their own handle).
    fn release_music(&mut self, keep: &HashSet<Cue>) {
        self.cues.retain(|cue, state| {
            keep.contains(cue)
                || matches!(cue.spec().bus, Bus::Sfx | Bus::Ui)
                || !matches!(state, CueState::Ready(_))
        });
    }
}

/// What the music director is playing (shown in the F3 diagnostics line).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct NowPlaying(pub String);

pub struct GameAudioPlugin;

/// Runs that must stay silent: unit tests, the headless smoke test, captures,
/// `--mute`, or the environment variable `OPENSPORE_MUTE=1`. F9 still toggles
/// sound in a normal game.
pub fn start_muted() -> bool {
    cfg!(test)
        || std::env::var_os("OPENSPORE_MUTE").is_some_and(|v| !v.is_empty() && v != "0")
        || std::env::args().any(|a| {
            matches!(a.as_str(), "--mute" | "--quick-capture" | "--render-smoke" | "--smoke-test")
        })
}

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameAudio>();
        if start_muted() {
            app.world_mut().resource_mut::<GameAudio>().muted = true;
        }
        app
            .init_resource::<MusicDirector>()
            .init_resource::<NowPlaying>()
            .add_message::<PlayCue>()
            .add_systems(
                Update,
                (
                    (receive_decoded, mute_toggle, prefetch_stage),
                    (
                        observe_ui,
                        observe_creator,
                        observe_health,
                        observe_creature.run_if(in_state(GameStage::Creature)),
                        observe_tribal.run_if(in_state(GameStage::Tribal)),
                        observe_civ.run_if(in_state(GameStage::Civilization)),
                        observe_space.run_if(in_state(GameStage::Space)),
                    ),
                    (music_director, play_cues),
                    publish_now_playing.run_if(resource_changed::<NowPlaying>),
                )
                    .chain(),
            );
    }
}

/// Startup (after the resource index is loaded): index installed audio and
/// start the decoders. Replaces the old first-clip `spore_menu.wav` /
/// `spore_music.wav` writers.
pub fn start(mut audio: ResMut<GameAudio>, mut originals: ResMut<Originals>) {
    let library = AudioLibrary::from_index(&originals.resource_index);
    let (props, snr, sns) = (
        library.count(TYPE_SOUND_PROP),
        library.count(TYPE_SNR),
        library.count(TYPE_SNS),
    );
    audio.muted |= start_muted();
    if snr == 0 {
        warn!("original audio: no Spore_Audio records indexed; playing silently");
        return;
    }
    let library = Arc::new(library);
    let (done, results) = mpsc::channel();
    audio.music_jobs = spawn_worker("spore-audio-music", library.clone(), done.clone());
    audio.sfx_jobs = spawn_worker("spore-audio-sfx", library.clone(), done);
    audio.results = Some(Mutex::new(results));
    audio.library = Some(library);
    audio.summary = format!(
        "{props} soundProps, {snr} SNR, {sns} SNS indexed{}",
        if audio.muted { " (muted, F9)" } else { "" }
    );
    info!("original audio: {}", audio.summary);
    originals.audio_file = Some(audio.summary.clone());
    for cue in [Cue::MenuGalaxy, Cue::MenuAmbience, Cue::UiClick] {
        audio.request(cue);
    }
}

fn receive_decoded(mut audio: ResMut<GameAudio>, assets: Option<ResMut<Assets<AudioSource>>>) {
    let Some(receiver) = audio.results.as_ref() else {
        return;
    };
    let ready: Vec<Ready> = match receiver.lock() {
        Ok(r) => r.try_iter().collect(),
        Err(_) => return,
    };
    let Some(mut assets) = assets else {
        for r in ready {
            audio.cues.insert(r.cue, CueState::Failed);
        }
        return;
    };
    for Ready {
        cue,
        result,
        millis,
    } in ready
    {
        let spec = cue.spec();
        let state = match result {
            Ok((def, clips)) => {
                info!(
                    "original audio: {} <- '{}' ({} clip(s), {:.1}s, gain {:.2}, {} ms)",
                    spec.key,
                    def.name,
                    clips.len(),
                    clips.iter().map(|c| c.secs).fold(0.0, f32::max),
                    def.gain,
                    millis
                );
                if clips.iter().all(|c| c.peak == 0) {
                    warn!("original audio: {} decoded to silence", spec.key);
                }
                let clips = clips
                    .into_iter()
                    .map(|c| {
                        (
                            assets.add(AudioSource {
                                bytes: c.wav.into(),
                            }),
                            c.secs,
                        )
                    })
                    .collect();
                CueState::Ready(Arc::new(LoadedCue { def, clips }))
            }
            Err(e) => {
                warn!("original audio: {} unavailable: {e}", spec.key);
                CueState::Failed
            }
        };
        audio.cues.insert(cue, state);
    }
}

fn mute_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut audio: ResMut<GameAudio>,
    mut sfx: Query<&mut AudioSink, With<SfxVoice>>,
) {
    if !keys.just_pressed(MUTE_KEY) {
        return;
    }
    audio.muted = !audio.muted;
    info!(
        "original audio {}",
        if audio.muted { "muted" } else { "unmuted" }
    );
    if audio.muted {
        for mut sink in &mut sfx {
            sink.set_volume(Volume::Linear(0.0));
        }
    }
}

/// Decode the stage's effects as soon as it starts.
fn prefetch_stage(
    stage: Res<State<GameStage>>,
    mut audio: ResMut<GameAudio>,
    mut last: Local<Option<GameStage>>,
    mut out: MessageWriter<PlayCue>,
) {
    if last.as_ref() == Some(stage.get()) || audio.library.is_none() {
        return;
    }
    if last.as_ref() == Some(&GameStage::Cell) && *stage.get() == GameStage::Creature {
        // The cell crawls onto land.
        out.write(PlayCue(Cue::CellToLand));
    }
    *last = Some(stage.get().clone());
    for cue in stage_cues(stage.get()) {
        audio.request(*cue);
    }
    for cue in [
        Cue::UiClick,
        Cue::UiPositive,
        Cue::CreatorClick,
        Cue::CreatorOpen,
    ] {
        audio.request(cue);
    }
}

fn publish_now_playing(now: Res<NowPlaying>, mut originals: ResMut<Originals>) {
    if !now.0.is_empty() {
        originals.music_file = Some(now.0.clone());
    }
}

/// One-shot voice.
#[derive(Component)]
pub struct SfxVoice(pub Cue);

fn play_cues(
    mut requests: MessageReader<PlayCue>,
    mut audio: ResMut<GameAudio>,
    mut commands: Commands,
    time: Res<Time<Real>>,
    active: Query<(), With<SfxVoice>>,
) {
    let now = time.elapsed_secs_f64();
    let mut queue: Vec<(Cue, f64)> = std::mem::take(&mut audio.waiting);
    for PlayCue(cue) in requests.read() {
        if audio.recent.len() >= 64 {
            audio.recent.pop_front();
        }
        audio.recent.push_back(*cue);
        if !queue.iter().any(|(c, _)| c == cue) {
            queue.push((*cue, now));
        }
    }
    let mut voices = active.iter().count();
    let mut rng = rand::rng();
    for (cue, asked) in queue {
        audio.request(cue);
        let Some(loaded) = audio.loaded(cue) else {
            if !audio.failed(cue) && now - asked < LATE_PLAY_SECS {
                audio.waiting.push((cue, asked));
            }
            continue;
        };
        let spec = cue.spec();
        if audio.muted
            || voices >= MAX_SFX_VOICES
            || audio
                .last_played
                .get(&cue)
                .is_some_and(|t| now - t < spec.min_interval as f64)
        {
            continue;
        }
        let (handle, _) = loaded.clips[rng.random_range(0..loaded.clips.len())].clone();
        let gain = spec.bus.volume() * loaded.def.gain * spec.trim;
        commands.spawn((
            Name::new(format!("sfx {}", spec.key)),
            AudioPlayer::new(handle),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(gain))
                .with_speed(loaded.def.pitch),
            SfxVoice(cue),
        ));
        audio.last_played.insert(cue, now);
        voices += 1;
    }
}

// ---------------------------------------------------------------------------
// Music director: stage score, ambience beds, crossfades
// ---------------------------------------------------------------------------

/// What the game state asks of the score.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicContext {
    pub stage: GameStage,
    pub creator_open: bool,
    /// Recently hurt, chased, in combat or near pirates.
    pub danger: bool,
    /// Cell DNA bar full (Spore switches to the beach loop).
    pub evolve_ready: bool,
    pub migrating: bool,
    pub mating: bool,
    /// Raiders are stealing the tribe's food.
    pub raided: bool,
    /// The tribe is besieging a village.
    pub siege: bool,
    pub approach: civ::Approach,
    /// What the Creature score patch reads (`creature_music`).
    pub creature: crate::creature_music::Params,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerKind {
    /// Whole authored loops/phrases, one after another (`gap` seconds of
    /// silence between items, `crossfade` overlap when back to back).
    Playlist { gap: (f32, f32), crossfade: f32 },
    /// Seamless looped bed.
    Bed,
    /// Random clips from a pool every `every` seconds (approximates the Pd
    /// note players and the fight percussion patch).
    Notes { every: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub cues: Vec<Cue>,
    pub kind: LayerKind,
    pub trim: f32,
}

impl Layer {
    pub(crate) fn playlist(cues: &[Cue], gap: (f32, f32), crossfade: f32) -> Option<Self> {
        Some(Self {
            cues: cues.to_vec(),
            kind: LayerKind::Playlist { gap, crossfade },
            trim: 1.0,
        })
    }
    pub(crate) fn bed(cue: Cue, trim: f32) -> Option<Self> {
        Some(Self {
            cues: vec![cue],
            kind: LayerKind::Bed,
            trim,
        })
    }
    pub(crate) fn notes(cue: Cue, every: (f32, f32)) -> Option<Self> {
        Some(Self {
            cues: vec![cue],
            kind: LayerKind::Notes { every },
            trim: 1.0,
        })
    }
}

/// Music slot plus two ambience beds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicPlan {
    pub music: Option<Layer>,
    pub bed: Option<Layer>,
    pub bed2: Option<Layer>,
}

/// Pure: the score for a game situation.
pub fn plan(ctx: &MusicContext) -> MusicPlan {
    use GameStage::*;
    const BACK_TO_BACK: (f32, f32) = (0.0, 0.0);
    if ctx.creator_open && ctx.stage != MainMenu {
        // A creature reaches its creator by mating: keep the mate music.
        let score = if ctx.stage == Creature && ctx.mating {
            Cue::CreatureMating
        } else {
            Cue::CreatorScore
        };
        return MusicPlan {
            music: Layer::playlist(&[score], BACK_TO_BACK, 6.0),
            bed: match ctx.stage {
                Cell => Layer::bed(Cue::CellularLoop, 1.5),
                Creature | Tribal => Layer::bed(Cue::CreatorForest, 1.0),
                _ => Layer::bed(Cue::CreatorScifi, 1.0),
            },
            bed2: None,
        };
    }
    match ctx.stage {
        MainMenu => MusicPlan {
            music: Layer::playlist(&[Cue::MenuGalaxy], BACK_TO_BACK, 6.0),
            bed: Layer::bed(Cue::MenuAmbience, 1.0),
            bed2: None,
        },
        // The retail Cell score (`cellgame_seed_music`), motion bed
        // (`cell_motion`) and ambience (`cell_amb`) are Pure Data patches
        // driven every frame by the game's audio parameters; `cell_audio`
        // runs them (level loops, chase layer, beach loop, speed and
        // level mixed beds), so the director plans nothing for the stage.
        Cell => MusicPlan::default(),
        // The retail score is the Pd patch `music_creature_modified`; until the
        // Pd engine runs it, `creature_music` approximates it with these loops.
        Creature => crate::creature_music::plan(ctx.danger, ctx.migrating, &ctx.creature),
        Tribal => MusicPlan {
            music: if ctx.raided {
                Layer::playlist(&[Cue::TribalRaidFood], BACK_TO_BACK, 4.0)
            } else if ctx.siege {
                Layer::playlist(&[Cue::TribalSiege], BACK_TO_BACK, 4.0)
            } else {
                // `mus_trg_open_range_lp` is the stage's looped main music; the
                // campfire power-up is a sting played at the tribe level up.
                Layer::playlist(&[Cue::TribalOpenRange], BACK_TO_BACK, 3.0)
            },
            bed: Layer::bed(Cue::PlanetAmbience, 1.0),
            // The high-altitude wind follows the camera (`tribal_audio`).
            bed2: None,
        },
        Civilization => MusicPlan {
            music: Layer::playlist(
                &[match ctx.approach {
                    civ::Approach::Military => Cue::CivMilitary,
                    civ::Approach::Religious => Cue::CivReligious,
                    civ::Approach::Economic => Cue::CivEconomic,
                }],
                (6.0, 18.0),
                4.0,
            ),
            bed: Layer::bed(Cue::PlanetAmbience, 0.7),
            bed2: None,
        },
        // The Space score (the `space_music_patch` stems mixed by zoom, the war
        // patch, the context beds) is run by `space_audio`.
        Space => MusicPlan::default(),
    }
}

/// Stage-entry sting (Spore's transition music) played before the score.
pub fn intro(from: &GameStage, to: &GameStage) -> Option<Cue> {
    use GameStage::*;
    match (from, to) {
        (MainMenu, Cell) => Some(Cue::CellIntro),
        (Creature, Tribal) => Some(Cue::TribalIntro),
        // The Tribal exit sting, then `CivIntro` (see `intro_follow`).
        (Tribal, Civilization) => Some(Cue::TribalToCivStart),
        (Civilization, Space) => Some(Cue::SpaceIntro),
        _ => None,
    }
}

/// The sting that follows a stage-entry sting once it has played: the
/// retail pair `music_trg_to_civ_start` and `music_trg_to_civ_end`.
pub fn intro_follow(cue: Cue) -> Option<Cue> {
    match cue {
        Cue::TribalToCivStart => Some(Cue::CivIntro),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Music = 0,
    Bed = 1,
    Bed2 = 2,
}

/// A music or ambience voice with its fade state.
#[derive(Component)]
pub struct MusicVoice {
    slot: Slot,
    gain: f32,
    level: f32,
    fade_in: f32,
    fade_out: f32,
    releasing: bool,
    /// Real time the clip ends (`f64::INFINITY` when looping).
    ends_at: f64,
}

#[derive(Default)]
struct SlotRuntime {
    layer: Option<Layer>,
    voice: Option<(Entity, Cue)>,
    voice_end: f64,
    next_at: f64,
    order: Vec<Cue>,
    pos: usize,
}

#[derive(Resource, Default)]
pub struct MusicDirector {
    slots: [SlotRuntime; 3],
    stage: Option<GameStage>,
    /// (stage, creator open): decoded music outside it is released.
    scope: Option<(GameStage, bool)>,
    intro: Option<(Cue, f64)>,
    intro_until: f64,
    last_danger: f64,
    hp: Option<f32>,
    siege_level: Option<f32>,
    last_siege: f64,
    now_playing: Option<Cue>,
}

fn voice_settings(looped: bool, level: f32, speed: f32) -> PlaybackSettings {
    PlaybackSettings {
        mode: if looped {
            PlaybackMode::Loop
        } else {
            PlaybackMode::Once
        },
        volume: Volume::Linear(level),
        speed,
        ..PlaybackSettings::ONCE
    }
}

#[allow(clippy::too_many_arguments)]
fn music_director(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut audio: ResMut<GameAudio>,
    mut director: ResMut<MusicDirector>,
    mut now_playing: ResMut<NowPlaying>,
    stage: Res<State<GameStage>>,
    editor: Res<EditorState>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    run: Res<RunProgress>,
    encounter: Option<Res<creature_actions::Encounter>>,
    mating: Option<Res<cell::CellMating>>,
    creature_params: Option<Res<crate::creature_music::CreatureAudioParams>>,
    player: Query<(&Transform, &Health), With<Player>>,
    raiders: Query<(), With<tribal::Raider>>,
    pirates: Query<&Transform, With<space::Pirate>>,
    mut voices: Query<(Entity, &mut MusicVoice, Option<&mut AudioSink>)>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    if audio.library.is_none() {
        return;
    }
    let director = &mut *director;
    let stage = stage.get().clone();

    // --- context ------------------------------------------------------------
    let hp = player.single().ok().map(|(_, h)| h.hp);
    if let (Some(hp), Some(prev)) = (hp, director.hp) {
        if hp < prev - 0.01 {
            director.last_danger = now;
        }
    }
    director.hp = hp;
    let player_at = player.single().ok().map(|(t, _)| t.translation);
    let pirate_near =
        player_at.is_some_and(|p| pirates.iter().any(|t| t.translation.distance(p) < 30.0));
    let in_combat = encounter
        .as_ref()
        .is_some_and(|e| e.mode == creature_actions::EncounterMode::Combat && e.target.is_some());
    if (stage == GameStage::Space && pirate_near) || (stage == GameStage::Creature && in_combat) {
        director.last_danger = now;
    }
    let siege_level: f32 = run
        .tribal_campaign
        .villages
        .iter()
        .map(|v| v.huts as f32 * 100.0 - v.hurt)
        .sum();
    if director
        .siege_level
        .is_some_and(|l| siege_level < l - 0.001)
    {
        director.last_siege = now;
    }
    director.siege_level = Some(siege_level);
    let ctx = MusicContext {
        stage: stage.clone(),
        creator_open: editor.open || mechanical.is_some_and(|creator| creator.open),
        danger: director.last_danger > 0.0 && now - director.last_danger < DANGER_HOLD_SECS,
        evolve_ready: mating.as_ref().is_some_and(|m| m.ready_announced),
        migrating: run.creature_progression.pending_migration.is_some(),
        mating: run.creature_lifecycle.mating,
        raided: !raiders.is_empty(),
        siege: director.last_siege > 0.0 && now - director.last_siege < 20.0,
        approach: run.civ_campaign.approach,
        creature: creature_params.map(|p| p.0.clone()).unwrap_or_default(),
    };
    let mut plan = plan(&ctx);
    // OPENSPORE_PD_MUSIC=1: the mix mode's Pd patches and sounds play instead.
    crate::pd_audio::adjust_plan(&stage, ctx.creator_open, &mut plan);

    // --- stage-entry stings ----------------------------------------------------
    let stage_changed = director.stage.as_ref() != Some(&stage);
    if stage_changed {
        if let Some(from) = director.stage.as_ref() {
            director.intro = intro(from, &stage).map(|cue| (cue, now));
            if let Some((cue, _)) = director.intro {
                audio.request(cue);
            }
        }
        director.stage = Some(stage.clone());
    }

    // --- layer changes release the slot's voices ------------------------------
    let mut released = [false; 3];
    for (slot, desired) in [
        (Slot::Music, &plan.music),
        (Slot::Bed, &plan.bed),
        (Slot::Bed2, &plan.bed2),
    ] {
        let rt = &mut director.slots[slot as usize];
        if rt.layer.as_ref() == desired.as_ref() {
            continue;
        }
        released[slot as usize] = true;
        if slot == Slot::Music && !stage_changed {
            // Danger or a creator interrupts a stage-entry sting.
            director.intro = None;
            director.intro_until = 0.0;
        }
        rt.layer = desired.clone();
        rt.voice = None;
        rt.order = desired.as_ref().map(|l| l.cues.clone()).unwrap_or_default();
        rt.order.shuffle(&mut rand::rng());
        rt.pos = 0;
        rt.next_at = now;
    }
    // Leaving a stage or creator frees its decoded score; danger/calm
    // switches inside one scope keep both sides cached.
    let scope = (stage.clone(), ctx.creator_open);
    if director.scope.as_ref() != Some(&scope) {
        director.scope = Some(scope);
        let keep: HashSet<Cue> = [&plan.music, &plan.bed, &plan.bed2]
            .into_iter()
            .flatten()
            .flat_map(|l| l.cues.iter().copied())
            .chain(director.intro.map(|(c, _)| c))
            .collect();
        audio.release_music(&keep);
    }

    // --- voices: fades and expiry -------------------------------------------------
    // The settings' Master volume (0 with Mute All) applies to every voice.
    let mute = if audio.muted { 0.0 } else { crate::front_end::settings::master_gain() };
    for (entity, mut voice, sink) in &mut voices {
        if released[voice.slot as usize] && !voice.releasing {
            voice.releasing = true;
            voice.fade_out = voice.fade_out.max(2.5);
        }
        if voice.releasing {
            voice.level -= dt / voice.fade_out.max(0.05);
        } else if voice.level < 1.0 {
            voice.level += dt / voice.fade_in.max(0.05);
        }
        voice.level = voice.level.clamp(0.0, 1.0);
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(voice.gain * voice.level * mute));
        }
        if (voice.releasing && voice.level <= 0.0) || now > voice.ends_at + 0.5 {
            commands.entity(entity).despawn();
        }
    }

    // --- intro sting --------------------------------------------------------------
    let mut spawned_music = None;
    if let Some((cue, asked)) = director.intro {
        if now < asked {
            // A chained sting waits for the one before it.
        } else if let Some(loaded) = audio.loaded(cue) {
            let (handle, secs) = loaded.clips[0].clone();
            let gain = Bus::Music.volume() * loaded.def.gain * cue.spec().trim;
            commands.spawn((
                Name::new(format!("music {}", cue.spec().key)),
                AudioPlayer::new(handle),
                voice_settings(false, gain * mute, 1.0),
                MusicVoice {
                    slot: Slot::Music,
                    gain,
                    level: 1.0,
                    fade_in: 0.0,
                    fade_out: 3.0,
                    releasing: false,
                    ends_at: now + secs as f64,
                },
            ));
            director.intro = intro_follow(cue).map(|next| (next, now + secs as f64));
            if let Some((next, _)) = director.intro {
                audio.request(next);
            }
            director.intro_until = now + (secs as f64 - 3.0).max(0.0);
            spawned_music = Some(cue);
        } else if audio.failed(cue) || now - asked > 4.0 {
            director.intro = None;
        }
    }
    let holding = director.intro.is_some() || now < director.intro_until;

    // --- schedule each slot --------------------------------------------------------
    let mut rng = rand::rng();
    for slot in [Slot::Music, Slot::Bed, Slot::Bed2] {
        let rt = &mut director.slots[slot as usize];
        let Some(layer) = rt.layer.clone() else {
            continue;
        };
        if slot == Slot::Music && holding {
            // Decode the score's first item while the sting plays.
            if let Some(cue) = rt.order.get(rt.pos % rt.order.len().max(1)) {
                audio.request(*cue);
            }
            continue;
        }
        let bus = if slot == Slot::Music {
            Bus::Music
        } else {
            Bus::Ambience
        };
        match layer.kind {
            LayerKind::Bed => {
                let cue = layer.cues[0];
                if rt.voice.is_some() {
                    continue;
                }
                audio.request(cue);
                let Some(loaded) = audio.loaded(cue) else {
                    continue;
                };
                let (handle, secs) = loaded.clips[0].clone();
                let gain = bus.volume() * loaded.def.gain * cue.spec().trim * layer.trim;
                let fade_in = loaded.def.fade_in.max(2.0);
                let entity = commands
                    .spawn((
                        Name::new(format!("ambience {} ({secs:.0}s)", cue.spec().key)),
                        AudioPlayer::new(handle),
                        voice_settings(true, 0.0, loaded.def.pitch),
                        MusicVoice {
                            slot,
                            gain,
                            level: 0.0,
                            fade_in,
                            fade_out: loaded.def.fade_out.max(2.0),
                            releasing: false,
                            ends_at: f64::INFINITY,
                        },
                    ))
                    .id();
                rt.voice = Some((entity, cue));
            }
            LayerKind::Playlist { gap, crossfade } => {
                if rt.voice.is_some() {
                    if now < rt.voice_end - crossfade as f64 {
                        continue;
                    }
                    // Let the item finish (or fade under the next one).
                    if let Some((e, finished)) = rt.voice.take() {
                        if crossfade > 0.0 {
                            if let Ok((_, mut v, _)) = voices.get_mut(e) {
                                v.releasing = true;
                                v.fade_out = crossfade;
                            }
                        }
                        // Long rotations stream through their stems instead
                        // of keeping every decoded loop in memory.
                        if layer.cues.len() > 3 && !rt.order.is_empty() {
                            let upcoming = rt.order[rt.pos % rt.order.len()];
                            if finished != upcoming {
                                audio.cues.remove(&finished);
                            }
                        }
                    }
                    rt.next_at = now
                        + if gap.1 > gap.0 {
                            rng.random_range(gap.0..gap.1) as f64
                        } else {
                            gap.0 as f64
                        };
                }
                if rt.order.is_empty() || now < rt.next_at {
                    continue;
                }
                let cue = rt.order[rt.pos % rt.order.len()];
                let upcoming = rt.order[(rt.pos + 1) % rt.order.len()];
                audio.request(cue);
                audio.request(upcoming);
                if audio.failed(cue) {
                    rt.pos += 1;
                    continue;
                }
                let Some(loaded) = audio.loaded(cue) else {
                    continue;
                };
                let (handle, secs) = loaded.clips[rng.random_range(0..loaded.clips.len())].clone();
                let gain = bus.volume() * loaded.def.gain * cue.spec().trim * layer.trim;
                let speed = loaded.def.pitch;
                let length = secs as f64 / speed as f64;
                // Only a first entry (or a crossfade) fades in; chained
                // loops start at full level so the seam stays exact.
                let fade_in = if crossfade > 0.0 || rt.pos == 0 {
                    crossfade.max(loaded.def.fade_in).max(1.5)
                } else {
                    0.0
                };
                let entity = commands
                    .spawn((
                        Name::new(format!("music {}", cue.spec().key)),
                        AudioPlayer::new(handle),
                        voice_settings(false, if fade_in > 0.0 { 0.0 } else { gain * mute }, speed),
                        MusicVoice {
                            slot,
                            gain,
                            level: if fade_in > 0.0 { 0.0 } else { 1.0 },
                            fade_in,
                            fade_out: crossfade.max(loaded.def.fade_out).max(1.0),
                            releasing: false,
                            ends_at: now + length,
                        },
                    ))
                    .id();
                rt.voice = Some((entity, cue));
                rt.voice_end = now + length;
                rt.pos += 1;
                if slot == Slot::Music {
                    spawned_music = Some(cue);
                }
            }
            LayerKind::Notes { every } => {
                let cue = layer.cues[0];
                audio.request(cue);
                if now < rt.next_at {
                    continue;
                }
                let Some(loaded) = audio.loaded(cue) else {
                    continue;
                };
                // Occasionally two notes together, like the patch's chords.
                let count = if rng.random::<f32>() < 0.2 { 2 } else { 1 };
                for _ in 0..count {
                    let (handle, secs) =
                        loaded.clips[rng.random_range(0..loaded.clips.len())].clone();
                    let gain = bus.volume()
                        * cue.spec().trim
                        * layer.trim
                        * rng.random_range(0.55f32..1.0);
                    commands.spawn((
                        Name::new(format!("note {}", cue.spec().key)),
                        AudioPlayer::new(handle),
                        voice_settings(false, gain * mute, 1.0),
                        MusicVoice {
                            slot,
                            gain,
                            level: 1.0,
                            fade_in: 0.0,
                            fade_out: 1.5,
                            releasing: false,
                            ends_at: now + secs as f64,
                        },
                    ));
                }
                rt.next_at = now + rng.random_range(every.0..every.1) as f64;
                if director.now_playing != Some(cue) {
                    spawned_music = Some(cue);
                }
            }
        }
    }
    if let Some(cue) = spawned_music.filter(|c| director.now_playing != Some(*c)) {
        director.now_playing = Some(cue);
        if let Some(loaded) = audio.loaded(cue) {
            let text = format!("{} ({})", loaded.def.name, cue.spec().key);
            if now_playing.0 != text {
                info!("original audio: now playing {text}");
                now_playing.0 = text;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event observers (read-only views of gameplay state)
// ---------------------------------------------------------------------------

/// True on a stage change, so observers re-baseline their snapshots.
fn stage_epoch(stage: &GameStage, last: &mut Option<GameStage>) -> bool {
    let fresh = last.as_ref() != Some(stage);
    *last = Some(stage.clone());
    fresh
}

/// Stage observers only run inside their stage: a gap in frames means the
/// stage was left and re-entered, so the old snapshot is stale.
fn ran_last_frame(frame: &FrameCount, last: &mut Option<u32>) -> bool {
    let continuous = last.is_some_and(|l| frame.0 == l.wrapping_add(1));
    *last = Some(frame.0);
    continuous
}

fn observe_ui(
    pressed: Query<(), Added<bevy::ui::Pressed>>,
    editor: Res<EditorState>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    mut out: MessageWriter<PlayCue>,
) {
    if !pressed.is_empty() {
        out.write(PlayCue(
            if editor.open || mechanical.is_some_and(|creator| creator.open) {
                Cue::CreatorClick
            } else {
                Cue::UiClick
            },
        ));
    }
}

fn part_count(run: &RunProgress) -> usize {
    run.cell_parts.len()
        + run.parts.len()
        + run.vehicle.parts.len()
        + run.building.parts.len()
        + run.ship.modules.len()
}

fn observe_creator(
    editor: Res<EditorState>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    run: Res<RunProgress>,
    mut last: Local<Option<(bool, usize)>>,
    mut out: MessageWriter<PlayCue>,
) {
    let now = (
        editor.open || mechanical.is_some_and(|creator| creator.open),
        part_count(&run),
    );
    if let Some((was_open, parts)) = *last {
        if now.0 != was_open {
            out.write(PlayCue(if now.0 {
                Cue::CreatorOpen
            } else {
                Cue::CreatorClose
            }));
        } else if now.0 && now.1 > parts {
            out.write(PlayCue(Cue::CreatorPartPlaced));
        } else if now.0 && now.1 < parts {
            out.write(PlayCue(Cue::CreatorPartRemoved));
        }
    }
    *last = Some(now);
}

/// Player damage and (outside the Cell stage) respawns.
fn observe_health(
    stage: Res<State<GameStage>>,
    editor: Res<EditorState>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    player: Query<&Health, With<Player>>,
    mut last: Local<(Option<GameStage>, Option<(f32, f32)>)>,
    mut out: MessageWriter<PlayCue>,
) {
    let stage = stage.get();
    let current = player.single().ok().map(|h| (h.hp, h.max));
    if stage_epoch(stage, &mut last.0)
        || editor.open
        || mechanical.is_some_and(|creator| creator.open)
    {
        last.1 = current;
        return;
    }
    let (Some((hp, max)), Some((prev, _))) = (current, last.1) else {
        last.1 = current;
        return;
    };
    last.1 = current;
    let hurt = match stage {
        // Cell damage sounds come from the animation events (cell_audio).
        GameStage::Cell => None,
        GameStage::Creature | GameStage::Tribal => Some(Cue::CreatureHurt),
        GameStage::Civilization => Some(Cue::CivHurt),
        GameStage::Space => Some(Cue::SpaceHurt),
        GameStage::MainMenu => None,
    };
    if hp < prev - (0.004 * max).max(0.01) {
        if let Some(cue) = hurt {
            out.write(PlayCue(cue));
        }
    } else if prev < 0.4 * max && hp >= max * 0.999 && hp - prev > 0.5 * max {
        // Creature/Space restore full health only on a respawn.
        match stage {
            GameStage::Creature => {
                out.write(PlayCue(Cue::CreatureDeath));
            }
            GameStage::Space => {
                out.write(PlayCue(Cue::SpaceShipLost));
            }
            _ => {}
        }
    }
}

#[derive(Default, Clone, PartialEq)]
struct CreatureSnapshot {
    cooldowns: [f32; 4],
    herbivore: f32,
    carnivore: f32,
    found: usize,
    piles: usize,
    allied: usize,
    defeated: usize,
    mating: bool,
    pack: usize,
    brain: u8,
    migrations: u8,
}

fn observe_creature(
    frame: Res<FrameCount>,
    run: Res<RunProgress>,
    encounter: Res<creature_actions::Encounter>,
    minds: Query<&creature_actions::CreatureMind>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<CreatureSnapshot>>,
    mut out: MessageWriter<PlayCue>,
) {
    let snapshot = CreatureSnapshot {
        cooldowns: encounter.cooldowns,
        herbivore: run.diet_herbivore,
        carnivore: run.diet_carnivore,
        found: run.creature_found_parts.len(),
        piles: run.creature_bone_piles_taken.len(),
        allied: minds.iter().filter(|m| m.allied).count(),
        defeated: minds.iter().filter(|m| m.defeated).count(),
        mating: run.creature_lifecycle.mating,
        pack: run.creature_pack.members.len(),
        brain: run.creature_progression.brain_level,
        migrations: run.creature_progression.migrations_completed,
    };
    let fresh = !ran_last_frame(&frame, &mut ran);
    let previous = last.replace(snapshot.clone());
    let (Some(prev), false) = (previous, fresh) else {
        return;
    };
    let now = snapshot;
    let mut play = |cue| {
        out.write(PlayCue(cue));
    };
    // Ability sounds (by rank, from the animation events) and the brain upgrade
    // sound are played by `creature_audio`.
    if now.allied > prev.allied {
        play(Cue::CreatureBefriend);
        play(Cue::UiPositive);
    }
    if now.defeated > prev.defeated {
        play(Cue::CreatureDeath);
    }
    if now.herbivore > prev.herbivore {
        play(Cue::CreatureEatFruit);
    }
    if now.carnivore > prev.carnivore {
        play(Cue::CreatureEatMeat);
    }
    if now.piles > prev.piles {
        play(Cue::CreatureDig);
    }
    if now.found > prev.found {
        play(Cue::CreaturePartFound);
    }
    if now.mating && !prev.mating {
        play(Cue::CreatureMateCall);
    }
    if now.pack > prev.pack {
        play(Cue::CreatureRecruit);
    }
    if now.migrations > prev.migrations {
        play(Cue::CreatureClaimNest);
    }
}

#[derive(Default, Clone)]
struct TribalSnapshot {
    performing: bool,
    cooldown: f32,
    recruit_cooldown: f32,
    relations: Vec<f32>,
    status: Vec<tribal::RivalStatus>,
    huts: Vec<u32>,
    hurt: Vec<f32>,
    food: f32,
    tribe: usize,
    raiders: usize,
    message_timer: f32,
    message: String,
}

fn observe_tribal(
    frame: Res<FrameCount>,
    run: Res<RunProgress>,
    raiders: Query<(), With<tribal::Raider>>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<TribalSnapshot>>,
    mut quiet_since: Local<f32>,
    time: Res<Time>,
    mut out: MessageWriter<PlayCue>,
) {
    let c = &run.tribal_campaign;
    let snapshot = TribalSnapshot {
        performing: c.performance.is_some(),
        cooldown: c.cooldown,
        recruit_cooldown: c.recruit_cooldown,
        relations: c.villages.iter().map(|v| v.relations).collect(),
        status: c.villages.iter().map(|v| v.status).collect(),
        huts: c.villages.iter().map(|v| v.huts).collect(),
        hurt: c.villages.iter().map(|v| v.hurt).collect(),
        food: run.dna_this_stage,
        tribe: run.tribe.len(),
        raiders: raiders.iter().count(),
        message_timer: c.message_timer,
        message: c.message.clone(),
    };
    let fresh = !ran_last_frame(&frame, &mut ran);
    let previous = last.replace(snapshot.clone());
    let (Some(prev), false) = (previous, fresh) else {
        return;
    };
    let now = snapshot;
    let mut play = |cue| {
        out.write(PlayCue(cue));
    };
    let villages = now.relations.len().min(prev.relations.len());
    let warmer = (0..villages).any(|i| now.relations[i] > prev.relations[i] + 0.001);
    let answered = now.cooldown > prev.cooldown + 0.5;
    if now.performing && !prev.performing {
        play(Cue::TribalPerformStart);
    } else if prev.performing && !now.performing {
        play(if warmer {
            Cue::TribalPerformGood
        } else {
            Cue::TribalPerformBad
        });
    } else if answered && warmer {
        play(Cue::TribalGift);
    }
    for i in 0..villages.min(now.status.len()).min(prev.status.len()) {
        if now.status[i] != prev.status[i] {
            match now.status[i] {
                tribal::RivalStatus::Allied => play(Cue::TribalAllied),
                tribal::RivalStatus::Conquered => play(Cue::TribalConquered),
                tribal::RivalStatus::Hostile => {}
            }
        }
        if now.huts.get(i) < prev.huts.get(i) {
            play(Cue::TribalHutRazed);
        }
        if now
            .hurt
            .get(i)
            .zip(prev.hurt.get(i))
            .is_some_and(|(a, b)| a > b)
        {
            if *quiet_since > 4.0 {
                play(Cue::TribalAttackStart);
            }
            *quiet_since = 0.0;
            play(Cue::TribalAttackHit);
        }
    }
    *quiet_since += time.delta_secs();
    if now.tribe > prev.tribe {
        play(Cue::TribalRecruit);
    } else if now.recruit_cooldown > prev.recruit_cooldown + 0.5 {
        play(Cue::TribalHeal);
    }
    let gathered = now.food - prev.food;
    if gathered > 0.0 && gathered < 5.0 {
        play(Cue::TribalGather);
    }
    if now.raiders > 0 && prev.raiders == 0 {
        play(Cue::TribalRaidAlarm);
    }
}

#[derive(Default, Clone)]
struct CivSnapshot {
    cooldown: f32,
    owned: usize,
    rival_hp: f32,
    upgrades: u32,
    lost: u32,
    spice: f32,
}

fn observe_civ(
    frame: Res<FrameCount>,
    run: Res<RunProgress>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<CivSnapshot>>,
    mut out: MessageWriter<PlayCue>,
) {
    let c = &run.civ_campaign;
    let snapshot = CivSnapshot {
        cooldown: c.action_cooldown,
        owned: c.owned_count(),
        rival_hp: c.cities.iter().filter(|c| !c.owned()).map(|c| c.hp).sum(),
        upgrades: c
            .cities
            .iter()
            .map(|c| c.buildings as u32 + c.defense as u32)
            .sum(),
        lost: c.vehicles_lost,
        spice: run.spice,
    };
    let fresh = !ran_last_frame(&frame, &mut ran);
    let previous = last.replace(snapshot.clone());
    let (Some(prev), false) = (previous, fresh) else {
        return;
    };
    let now = snapshot;
    let mut play = |cue| {
        out.write(PlayCue(cue));
    };
    let acted = now.cooldown > prev.cooldown + 0.1;
    if acted {
        play(match c.approach {
            civ::Approach::Military => Cue::CivMilitaryShot,
            civ::Approach::Religious => Cue::CivReligiousPulse,
            civ::Approach::Economic => Cue::CivEconomicOffer,
        });
    }
    if now.owned > prev.owned {
        play(Cue::CivCapture);
    } else if now.rival_hp < prev.rival_hp - 0.01 {
        play(Cue::CivCityHit);
    }
    if now.upgrades > prev.upgrades {
        play(Cue::CivBuild);
    }
    if now.lost > prev.lost {
        play(Cue::CivVehicleLost);
    }
    if now.spice > prev.spice + 0.01 && !acted && now.owned == prev.owned {
        play(Cue::CivIncome);
    }
}

#[derive(Default, Clone)]
struct SpaceSnapshot {
    surveyed: usize,
    terraformed: usize,
    colonies: usize,
    docked: usize,
    claimed: usize,
    kills: u32,
    pirate_hp: f32,
}

fn observe_space(
    frame: Res<FrameCount>,
    run: Res<RunProgress>,
    shots: Query<(), Added<space::Projectile>>,
    pirates: Query<&space::Pirate>,
    mut ran: Local<Option<u32>>,
    mut last: Local<Option<SpaceSnapshot>>,
    mut out: MessageWriter<PlayCue>,
) {
    let c = &run.space_campaign;
    let snapshot = SpaceSnapshot {
        surveyed: c.surveyed.len(),
        terraformed: c.terraformed.len(),
        colonies: c.colonies.len(),
        docked: c.docked.len(),
        claimed: c.claimed,
        kills: c.kills,
        pirate_hp: pirates.iter().map(|p| p.hp.max(0.0)).sum(),
    };
    let fresh = !ran_last_frame(&frame, &mut ran);
    let previous = last.replace(snapshot.clone());
    let (Some(prev), false) = (previous, fresh) else {
        return;
    };
    let now = snapshot;
    let mut play = |cue| {
        out.write(PlayCue(cue));
    };
    if !shots.is_empty() {
        play(Cue::SpaceFire);
    }
    if now.kills > prev.kills {
        play(Cue::SpacePirateDown);
        play(Cue::SpaceLoot);
    } else if now.pirate_hp < prev.pirate_hp - 0.01 {
        play(Cue::SpaceHit);
    }
    if now.surveyed > prev.surveyed {
        play(Cue::SpaceScan);
    }
    if now.terraformed > prev.terraformed {
        play(Cue::SpaceTerraform);
    }
    if now.colonies > prev.colonies {
        play(Cue::SpaceColony);
    }
    if now.docked > prev.docked {
        play(Cue::SpaceDock);
    }
    if now.claimed > prev.claimed {
        play(Cue::SpaceMission);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_table_is_complete_and_unique() {
        let mut keys = HashSet::new();
        for cue in Cue::ALL {
            let spec = cue.spec();
            assert!(!spec.names.is_empty(), "{cue:?} has no original names");
            assert!(keys.insert(spec.key), "duplicate key {}", spec.key);
            assert!(spec.trim > 0.0 && spec.variants > 0);
        }
        // Every stage prefetch list only names effects/UI.
        for stage in GameStage::all() {
            for cue in stage_cues(stage) {
                assert!(matches!(cue.spec().bus, Bus::Sfx | Bus::Ui), "{cue:?}");
            }
        }
    }

    #[test]
    fn audio_names_hash_to_the_documented_ids() {
        // Lowercase FNV-1, as SporeModder-FX reg_file.txt / reg_property.txt.
        assert_eq!(
            name_id("sfx_meteorshower_loop"),
            name_id("SFX_MeteorShower_Loop")
        );
        assert_eq!(name_id("gain"), PROP_GAIN);
        assert_eq!(name_id("samples"), PROP_SAMPLES);
        assert_eq!(name_id("islooped"), PROP_LOOPED);
        assert_eq!(name_id("pitch"), PROP_PITCH);
        assert_eq!(name_id("fadein"), PROP_FADE_IN);
        assert_eq!(name_id("fadeout"), PROP_FADE_OUT);
        assert_eq!(name_id("pd"), 0x6177_15D9);
        // The retail soundProp of `cityhall_tibetan` and its SNR share this ID.
        assert_eq!(name_id("cityhall_tibetan"), 0x647E_0697);
        assert_eq!(name_id("music_cell_loopa"), 0xE90A_B1A3);
    }

    #[test]
    fn plan_follows_stage_creator_and_danger() {
        let ctx = |stage| MusicContext {
            stage,
            ..Default::default()
        };
        // The Cell score, motion and ambience patches run in `cell_audio`.
        assert_eq!(plan(&ctx(GameStage::Cell)), MusicPlan::default());
        let chased = plan(&MusicContext {
            danger: true,
            ..ctx(GameStage::Cell)
        });
        assert_eq!(chased, MusicPlan::default());
        let creator = plan(&MusicContext {
            creator_open: true,
            ..ctx(GameStage::Creature)
        });
        assert_eq!(creator.music.unwrap().cues, vec![Cue::CreatorScore]);
        assert_eq!(creator.bed.unwrap().cues, vec![Cue::CreatorForest]);
        let fight = plan(&MusicContext {
            danger: true,
            ..ctx(GameStage::Creature)
        });
        assert_eq!(fight.music.unwrap().cues, vec![Cue::CreatureFight]);
        let religious = plan(&MusicContext {
            approach: civ::Approach::Religious,
            ..ctx(GameStage::Civilization)
        });
        assert_eq!(religious.music.unwrap().cues, vec![Cue::CivReligious]);
        // The Space score and war music belong to `space_audio`.
        let war = plan(&MusicContext {
            danger: true,
            ..ctx(GameStage::Space)
        });
        assert_eq!(war, MusicPlan::default());
        // Every planned cue is a music or ambience cue.
        for stage in GameStage::all() {
            for creator_open in [false, true] {
                for danger in [false, true] {
                    let p = plan(&MusicContext {
                        stage: stage.clone(),
                        creator_open,
                        danger,
                        ..Default::default()
                    });
                    for layer in [p.music, p.bed, p.bed2].into_iter().flatten() {
                        for cue in layer.cues {
                            assert!(matches!(cue.spec().bus, Bus::Music | Bus::Ambience));
                        }
                    }
                }
            }
        }
        assert_eq!(
            intro(&GameStage::MainMenu, &GameStage::Cell),
            Some(Cue::CellIntro)
        );
        assert_eq!(intro(&GameStage::Cell, &GameStage::MainMenu), None);
    }

    /// The observers run in the real headless schedule and turn gameplay
    /// state changes into cues, with no audio device or install.
    #[test]
    fn observers_turn_state_changes_into_cues() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.add_plugins(GameAudioPlugin);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Cell);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            *app.world().resource::<State<GameStage>>().get(),
            GameStage::Cell
        );
        // Cell eat / hatch / death sounds belong to cell_audio (animation
        // events); the legacy diet/notice observers are gone.
        app.world_mut().resource_mut::<RunProgress>().diet_herbivore += 1.0;
        app.update();
        app.world_mut()
            .resource_mut::<cell::CellNotice>()
            .show("Your cell was eaten! A new one hatches from an egg.");
        app.update();
        app.world_mut().resource_mut::<EditorState>().show();
        app.update();
        let recent: Vec<Cue> = app
            .world()
            .resource::<GameAudio>()
            .recent
            .iter()
            .copied()
            .collect();
        assert!(
            recent.contains(&Cue::CreatorOpen),
            "CreatorOpen missing from {recent:?}"
        );
        // No library: nothing decodes or spawns, nothing panics.
        assert!(!app
            .world()
            .resource::<GameAudio>()
            .is_ready(Cue::CreatorOpen));
    }

    fn installed_library() -> Option<AudioLibrary> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        let library = AudioLibrary::from_index(&index);
        (library.count(TYPE_SNR) > 0).then_some(library)
    }

    /// Every cue resolves to installed audio and decodes to real sound.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_cues_resolve_and_decode() {
        let library = installed_library().expect("Spore install with audio");
        assert!(library.count(TYPE_SOUND_PROP) > 8000);
        assert!(library.count(TYPE_SNR) > 20000);
        assert!(library.count(TYPE_SNS) >= 40);
        let mut rng = rand::rng();
        // Exercise fixed shopping-track windows as well as the randomized cue
        // path below; a random start alone can miss a bad stream boundary.
        let shopping = library.resolve("music_shopping_sample").unwrap();
        let shopping_snr = library.read(TYPE_SNR, shopping.samples[0]).unwrap();
        let shopping_layout = audio::parse_snr_layout(&shopping_snr).unwrap();
        let shopping_sns = if shopping_layout.storage == audio::SnrStorage::Ram {
            None
        } else {
            Some(library.read(TYPE_SNS, shopping.samples[0]).unwrap())
        };
        let window_len = 150 * shopping_layout.header.sample_rate as usize;
        if shopping_layout.header.num_samples > window_len + 1 {
            let latest = shopping_layout.header.num_samples - window_len;
            let sns = shopping_sns.as_deref();
            for start in [
                0,
                (120 * shopping_layout.header.sample_rate as usize).min(latest),
                (360 * shopping_layout.header.sample_rate as usize).min(latest),
            ] {
                let sound = audio::decode_snr_sound(
                    &shopping_snr,
                    sns,
                    Some(DecodeWindow {
                        start,
                        len: window_len,
                    }),
                )
                .unwrap_or_else(|error| {
                    panic!("shopping sample window at {start} failed: {error}")
                });
                assert_eq!(sound.frames(), window_len);
                assert!(sound.peak() > 64, "silent shopping window at {start}");
            }
        }
        let mut failures = Vec::new();
        for cue in Cue::ALL {
            let spec = cue.spec();
            let started = std::time::Instant::now();
            match load_cue(&library, &spec, &mut rng) {
                Ok((def, clips)) => {
                    let secs = clips.iter().map(|c| c.secs).fold(0.0, f32::max);
                    let peak = clips.iter().map(|c| c.peak).max().unwrap_or(0);
                    let id = name_id(def.name.split(' ').next().unwrap_or_default());
                    let kind = if library.contains(TYPE_SOUND_PROP, id) {
                        TYPE_SOUND_PROP
                    } else {
                        TYPE_SNR
                    };
                    println!(
                        "CUE|{}|{}|{AUDIO_GROUP:08x}!{id:08x}.{kind:08x}|{}/{}|{secs:.1}|{:.2}|{}|{peak}|{}",
                        spec.key,
                        def.name,
                        clips.len(),
                        def.samples.len(),
                        def.gain,
                        def.looped,
                        started.elapsed().as_millis()
                    );
                    if peak < 64 {
                        failures.push(format!("{}: silent ({peak})", spec.key));
                    }
                }
                Err(e) => failures.push(format!("{}: {e}", spec.key)),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Spot checks of resolution against known retail data.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_sound_props_inherit_and_stream() {
        let library = installed_library().expect("Spore install with audio");
        // Inherited loop flag + streamed EALayer3 stem in Spore_Audio2.
        let loop_a = library.resolve("music_cell_loopa").unwrap();
        assert!(loop_a.looped && loop_a.inheritance >= 2);
        assert!(library.contains(TYPE_SNS, loop_a.samples[0]));
        // Own gain/pitch, single XAS sample.
        let hurt = library.resolve("sfx_cell_avatar_takedmg").unwrap();
        assert!((hurt.gain - 0.3).abs() < 1e-4 && (hurt.pitch - 0.75).abs() < 1e-4);
        // Six variants, as authored.
        assert_eq!(library.resolve("sfx_cell_eat").unwrap().samples.len(), 6);
        // Pure Data patches carry no decodable samples.
        assert!(library.resolve("music_planet").is_err());
        let mut rng = rand::rng();
        let stem = library.decode(loop_a.samples[0], None, &mut rng).unwrap();
        assert_eq!((stem.channels, stem.rate), (2, 44100));
        assert!((stem.secs() - 40.0).abs() < 0.1, "{}", stem.secs());
        assert!(stem.peak() > 1000);
        // Gigasample: SNR prefetch + SNS remainder decode as one track.
        let neutral = library.resolve("music_planet_space_neutral").unwrap();
        let full = library.decode(neutral.samples[0], None, &mut rng).unwrap();
        assert!(full.secs() > 120.0, "{}", full.secs());
        // Windowed decode of a 10-minute stream stays bounded.
        let galaxy = library.resolve("galaxy_long").unwrap();
        let part = library
            .decode(galaxy.samples[0], Some((300.0, 20.0)), &mut rng)
            .unwrap();
        assert!((part.secs() - 20.0).abs() < 0.05, "{}", part.secs());
    }
}
