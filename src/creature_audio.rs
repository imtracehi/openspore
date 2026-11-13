//! Creature stage sounds (C7 of `notes\creature-re-notes.md`) and the sounds
//! of the stage flow.
//!
//! RETAIL direct calls (code addresses in the notes):
//! * DNA gained (0xD2D2E1): `cell_gold_d` when the amount is above 20 (the
//!   `creatureEvoPoints_XL` tuning), `cell_gold_c` above 10, `cell_gold_b`
//!   above 5, else `cell_gold_a` (the same sounds as the Cell's gold pickups);
//! * a verb tray button clicked (0xD49671): `editor_general_click`;
//! * egg appears (0xD6D102): `sfx_egg_appear`, which is NOT installed (like
//!   the retail install it is silent);
//! * minimap zoom buttons (0xD2C686 / 0xD2C915): `ui_camera_zoom` and
//!   `ui_camera_zoom_mouseover`. The retail minimap panel (`GlobalUICreature`
//!   0x0562A0F7) is hidden in OpenSpore's HUD (the legacy world map is shown),
//!   so there is no button to click;
//! * the stance change sets the audio parameters `ui_social` / `ui_fight`
//!   (0xD2E55F); the score patch reads them: see `creature_music`;
//! * a mission completes (0xC2E700, one routine for the Creature, Tribe and
//!   Civ missions): the parameter `missioncomplete` is set to 1 and
//!   `ui_mission_complete` (0x0C871D6E) plays. ADAPTED: OpenSpore's goal cards
//!   are not retail mission objects, so the completions it plays it for are
//!   the first befriended or defeated species and a full pack ([`MissionState`]);
//!   the parameter is driven too: each completion counts in
//!   [`crate::creature_music::MissionPulse`] and the Pd feed sets it to 1 (the
//!   patch's `missionsuccess` bang reacts to every set, not to a change);
//! * `creature_newsize9`, listed by the audio audit at 0x93CCA1, is no sound
//!   call: the bytes are an immediate of the UTF-8 encoder (0x93CC9B..0x93CCFF).
//!
//! RETAIL animation events: the ability clips carry their sounds (found by
//! scanning the installed ANIM resources): `scom_bite`, `sfx_crg_strike_lvl<n>_swish`,
//! `scom_charge_rush`, `sfx_crg_mucus_launch`, `ssoc_dance|charm|pose_lvl0<n>`,
//! with the level (rank) in the name; the voices `bsoc_sing_lvl0<n>RoXCr`,
//! `bcom_bite_lvl0<n>RoXCr`, `bcom_strike_lvl0<n>RoXCr`,
//! `bsoc_dance_lvl0<n>RoXCr`. With the clip player driving the body
//! (`creature_clips`, the retail clip ids of the ability props) the events
//! play at their frame times through `anim_audio`. FALLBACK (ADAPTED, only
//! without a clip drive: headless runs and tests): [`ability_sounds`] plays
//! the same names when the ability is used (the cooldown starts).
//!
//! The cinematics' sounds (`creature_cinematics`) start with the moment
//! (`FlowEvent::Start`): `music_brain_upgrade`, `music_crg_to_trg`,
//! `c2crg_vo_appear` (first of the three landing voices), ...
//!
//! Sounds are decoded and played by `cell_audio::NamedSounds` (soundProp gain,
//! pitch, polyphony, start delay, bus). Set `OPENSPORE_CELL_AUDIO_DEBUG=1` to
//! print every sound started (its `source` says why).

use bevy::prelude::*;

use crate::{
    cell_audio::{self, NamedSounds, Play},
    creature_actions::{self, Encounter, EncounterMode},
    creature_cinematics as cinematics,
    creature_flow::{FlowEvent, UiSound},
    game::RunProgress,
    game_audio::Bus,
    stages::GameStage,
    world_objects::name_id,
};

/// The DNA gain sound for an amount (retail 0xD2D2E1).
pub fn dna_sound(amount: f32) -> &'static str {
    if amount > 20.0 {
        "cell_gold_d"
    } else if amount > 10.0 {
        "cell_gold_c"
    } else if amount > 5.0 {
        "cell_gold_b"
    } else {
        "cell_gold_a"
    }
}

/// `ui_mission_complete` (0xC2E76A: `Audio::PlayAudio(0x0C871D6E)`).
pub const MISSION_SOUND: &str = "ui_mission_complete";

/// What the mission watch compares between frames: the facts whose rise
/// completes a goal of the Creature stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissionState {
    /// A species has been befriended or driven out (the "Creatures" goal).
    pub met_species: bool,
    /// The pack is full (the "Pack Members" goal).
    pub pack_full: bool,
}

impl MissionState {
    pub fn of(run: &RunProgress) -> Self {
        let outcomes = run.species_world.outcome_counts();
        let capacity = crate::creature_progression::pack_capacity(run.dna_this_stage);
        Self {
            met_species: outcomes.allied + outcomes.defeated > 0,
            pack_full: capacity > 0 && run.creature_pack.members.len() >= capacity,
        }
    }
}

/// How many goals completed between two states.
pub fn missions_completed(before: MissionState, now: MissionState) -> usize {
    usize::from(!before.met_species && now.met_species) + usize::from(!before.pack_full && now.pack_full)
}

const SING: [&str; 5] =
    ["bsoc_sing_lvl01RoXCr", "bsoc_sing_lvl02RoXCr", "bsoc_sing_lvl03RoXCr", "bsoc_sing_lvl04RoXCr", "bsoc_sing_lvl05RoXCr"];
const DANCE: [&str; 5] = ["ssoc_dance_lvl01", "ssoc_dance_lvl02", "ssoc_dance_lvl03", "ssoc_dance_lvl04", "ssoc_dance_lvl05"];
const DANCE_VOICE: [&str; 5] = [
    "bsoc_dance_lvl01RoXCr",
    "bsoc_dance_lvl02RoXCr",
    "bsoc_dance_lvl03RoXCr",
    "bsoc_dance_lvl04RoXCr",
    "bsoc_dance_lvl05RoXCr",
];
const CHARM: [&str; 5] = ["ssoc_charm_lvl01", "ssoc_charm_lvl02", "ssoc_charm_lvl03", "ssoc_charm_lvl04", "ssoc_charm_lvl05"];
const POSE: [&str; 5] = ["ssoc_pose_lvl01", "ssoc_pose_lvl02", "ssoc_pose_lvl03", "ssoc_pose_lvl04", "ssoc_pose_lvl05"];
const BITE_VOICE: [&str; 5] =
    ["bcom_bite_lvl01RoXCr", "bcom_bite_lvl02RoXCr", "bcom_bite_lvl03RoXCr", "bcom_bite_lvl04RoXCr", "bcom_bite_lvl05RoXCr"];
const STRIKE: [&str; 5] = [
    "sfx_crg_strike_lvl1_swish",
    "sfx_crg_strike_lvl2_swish",
    "sfx_crg_strike_lvl3_swish",
    "sfx_crg_strike_lvl4_swish",
    "sfx_crg_strike_lvl5_swish",
];
const STRIKE_VOICE: [&str; 5] = [
    "bcom_strike_lvl01RoXCr",
    "bcom_strike_lvl02RoXCr",
    "bcom_strike_lvl03RoXCr",
    "bcom_strike_lvl04RoXCr",
    "bcom_strike_lvl05RoXCr",
];

/// The sounds of the ability in slot `slot` (key 1..4) of the stance's tray at
/// `rank` (1..=5, clamped).
pub fn ability_sounds(mode: EncounterMode, slot: usize, rank: u8) -> Vec<&'static str> {
    let i = rank.clamp(1, 5) as usize - 1;
    match (mode, slot) {
        (EncounterMode::Social, 0) => vec![SING[i]],
        (EncounterMode::Social, 1) => vec![DANCE[i], DANCE_VOICE[i]],
        (EncounterMode::Social, 2) => vec![CHARM[i]],
        (EncounterMode::Social, _) => vec![POSE[i]],
        (EncounterMode::Combat, 0) => vec!["scom_bite", BITE_VOICE[i]],
        (EncounterMode::Combat, 1) => vec!["scom_charge_rush"],
        (EncounterMode::Combat, 2) => vec![STRIKE[i], STRIKE_VOICE[i]],
        (EncounterMode::Combat, _) => vec!["sfx_crg_mucus_launch"],
    }
}

/// Every sound the stage decodes at entry so that the first play is not late.
pub fn prefetch_names() -> Vec<&'static str> {
    let mut out = vec!["cell_gold_a", "cell_gold_b", "cell_gold_c", "cell_gold_d", "editor_general_click", MISSION_SOUND];
    for mode in [EncounterMode::Social, EncounterMode::Combat] {
        for slot in 0..4 {
            for rank in 1..=5 {
                for name in ability_sounds(mode, slot, rank) {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
    }
    out
}

/// The sounds a cinematic starts with its moment. The three landing voices of
/// `CLG2CRG` are timed through the 30 s cinematic (appear, call, buddies); only
/// the first is played with the text window.
pub fn cinematic_sounds(name: &str) -> &'static [&'static str] {
    match cinematics::cinematic(name) {
        Some(c) if name == "CLG2CRG" => &c.sounds[..1.min(c.sounds.len())],
        Some(c) => c.sounds,
        None => &[],
    }
}

/// The bus of a cinematic's sound: music tracks on the music bus.
fn cinematic_bus(sound: &str) -> Bus {
    if sound.starts_with("music_") {
        Bus::Music
    } else {
        Bus::Sfx
    }
}

pub struct CreatureAudioPlugin;

impl Plugin for CreatureAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::creature_music::MissionPulse>().add_systems(
            Update,
            (
                cell_audio::pump_decoder,
                prefetch,
                dna_sounds,
                mission_sounds,
                ability_sounds_system,
                ui_sounds,
                flow_sounds,
                cell_audio::play_sounds,
            )
                .chain()
                .run_if(in_state(GameStage::Creature))
                .run_if(resource_exists::<RunProgress>)
                .run_if(resource_exists::<crate::Originals>),
        );
    }
}

/// Decode the stage's sounds once at entry.
fn prefetch(mut sounds: ResMut<NamedSounds>, audio: Res<crate::game_audio::GameAudio>, mut done: Local<bool>) {
    if *done || audio.library().is_none() {
        return;
    }
    *done = true;
    for name in prefetch_names() {
        sounds.request(name_id(name), Some(name), false);
    }
}

/// DNA gained: the amount picks the sound.
fn dna_sounds(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut last: Local<Option<f32>>,
) {
    let now = run.dna_this_stage;
    if let Some(prev) = last.replace(now) {
        if now > prev + 1e-3 {
            let mut play = Play::named(dna_sound(now - prev), "dna");
            play.bus = Bus::Sfx;
            sounds.play(play, time.elapsed_secs_f64(), 0.0);
        }
    }
}

/// A goal completes (0xC2E700): `ui_mission_complete` plays.
fn mission_sounds(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut pulse: ResMut<crate::creature_music::MissionPulse>,
    mut last: Local<Option<MissionState>>,
) {
    let now = MissionState::of(&run);
    if let Some(before) = last.replace(now) {
        let done = missions_completed(before, now);
        if done > 0 {
            let mut play = Play::named(MISSION_SOUND, "mission");
            play.bus = Bus::Ui;
            sounds.play(play, time.elapsed_secs_f64(), 0.0);
            pulse.0 += done as u32;
        }
    }
}

/// An ability starts its cooldown: its clip's sounds play.
fn ability_sounds_system(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    originals: Res<crate::Originals>,
    mut sounds: ResMut<NamedSounds>,
    drive: Option<Res<crate::creature_clips::ClipDrive>>,
    mut last: Local<Option<[f32; 4]>>,
) {
    let previous = last.replace(encounter.cooldowns);
    let Some(previous) = previous else { return };
    // The retail ability clip plays on the body: its own sound events
    // (swishes, bites, voices) play through `anim_audio`.
    if crate::creature_clips::drives_body(drive.as_deref()) {
        return;
    }
    let abilities = creature_actions::player_abilities(&run, &originals);
    let ranks = creature_actions::levels(abilities, encounter.mode);
    for slot in 0..4 {
        if encounter.cooldowns[slot] > previous[slot] + 0.05 {
            for name in ability_sounds(encounter.mode, slot, ranks[slot]) {
                sounds.play(Play::named(name, "ability"), time.elapsed_secs_f64(), 0.0);
            }
        }
    }
}

/// HUD clicks.
fn ui_sounds(time: Res<Time<Real>>, mut events: MessageReader<UiSound>, mut sounds: ResMut<NamedSounds>) {
    for event in events.read() {
        match event {
            UiSound::VerbClick => {
                let mut play = Play::named("editor_general_click", "verb tray");
                play.bus = Bus::Ui;
                sounds.play(play, time.elapsed_secs_f64(), 0.0);
            }
        }
    }
}

/// A cinematic starts: its sounds play.
fn flow_sounds(time: Res<Time<Real>>, mut events: MessageReader<FlowEvent>, mut sounds: ResMut<NamedSounds>) {
    for event in events.read() {
        if let FlowEvent::Start(name) = event {
            for sound in cinematic_sounds(name) {
                let mut play = Play::named(sound, "cinematic");
                play.bus = cinematic_bus(sound);
                sounds.play(play, time.elapsed_secs_f64(), 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dna_amount_picks_the_gold_sound() {
        assert_eq!(dna_sound(1.0), "cell_gold_a");
        assert_eq!(dna_sound(5.0), "cell_gold_a");
        assert_eq!(dna_sound(5.5), "cell_gold_b");
        assert_eq!(dna_sound(10.0), "cell_gold_b");
        assert_eq!(dna_sound(10.1), "cell_gold_c");
        assert_eq!(dna_sound(20.0), "cell_gold_c");
        assert_eq!(dna_sound(20.1), "cell_gold_d");
        assert_eq!(dna_sound(100.0), "cell_gold_d");
    }

    #[test]
    fn abilities_play_their_clip_sounds_by_rank() {
        use EncounterMode::*;
        assert_eq!(ability_sounds(Social, 0, 1), ["bsoc_sing_lvl01RoXCr"]);
        assert_eq!(ability_sounds(Social, 1, 3), ["ssoc_dance_lvl03", "bsoc_dance_lvl03RoXCr"]);
        assert_eq!(ability_sounds(Social, 2, 5), ["ssoc_charm_lvl05"]);
        assert_eq!(ability_sounds(Social, 3, 2), ["ssoc_pose_lvl02"]);
        assert_eq!(ability_sounds(Combat, 0, 4), ["scom_bite", "bcom_bite_lvl04RoXCr"]);
        assert_eq!(ability_sounds(Combat, 1, 1), ["scom_charge_rush"]);
        assert_eq!(ability_sounds(Combat, 2, 2), ["sfx_crg_strike_lvl2_swish", "bcom_strike_lvl02RoXCr"]);
        assert_eq!(ability_sounds(Combat, 3, 3), ["sfx_crg_mucus_launch"]);
        // Ranks are clamped (rank 0 never plays; above 5 is level 5).
        assert_eq!(ability_sounds(Combat, 2, 0)[0], "sfx_crg_strike_lvl1_swish");
        assert_eq!(ability_sounds(Combat, 2, 9)[0], "sfx_crg_strike_lvl5_swish");
    }

    #[test]
    fn a_goal_completes_when_its_fact_rises() {
        let none = MissionState { met_species: false, pack_full: false };
        let met = MissionState { met_species: true, ..none };
        let full = MissionState { pack_full: true, ..met };
        assert_eq!(missions_completed(none, none), 0);
        assert_eq!(missions_completed(none, met), 1);
        assert_eq!(missions_completed(met, met), 0, "no repeat while the fact stays");
        assert_eq!(missions_completed(met, full), 1);
        assert_eq!(missions_completed(none, full), 2);
        assert_eq!(missions_completed(full, none), 0, "a fact that falls completes nothing");
    }

    #[test]
    fn the_stage_prefetches_every_ability_sound_once() {
        let names = prefetch_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no duplicates");
        for name in ["cell_gold_d", "editor_general_click", "scom_bite", "ssoc_pose_lvl05", "bcom_strike_lvl05RoXCr", "ui_mission_complete"] {
            assert!(names.contains(&name), "{name}");
        }
    }

    #[test]
    fn cinematics_start_their_sounds() {
        assert_eq!(cinematic_sounds("CRG_BrainUpgrade3"), ["music_brain_upgrade"]);
        assert_eq!(cinematic_sounds("CRG2TRG"), ["music_crg_to_trg"]);
        assert_eq!(cinematic_sounds("CLG2CRG"), ["c2crg_vo_appear"], "only the first landing voice");
        assert!(cinematic_sounds("CRG_FirstDNA").is_empty());
        assert!(cinematic_sounds("nonsense").is_empty());
        assert_eq!(cinematic_bus("music_brain_upgrade"), Bus::Music);
        assert_eq!(cinematic_bus("c2crg_vo_appear"), Bus::Sfx);
    }

    /// The systems turn gameplay changes into queued sounds, headless.
    #[test]
    fn dna_abilities_clicks_and_cinematics_queue_their_sounds() {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.add_systems(
            Update,
            (dna_sounds, ability_sounds_system, ui_sounds, flow_sounds).run_if(in_state(GameStage::Creature)),
        );
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        for _ in 0..4 {
            app.update();
        }
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        // DNA: 12 points -> cell_gold_c.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage += 12.0;
        app.update();
        assert!(queued(&app).contains(&name_id("cell_gold_c")), "{:?}", queued(&app));
        // A social ability (dance, key 2) starts its cooldown.
        {
            let mut encounter = app.world_mut().resource_mut::<Encounter>();
            encounter.mode = EncounterMode::Social;
            encounter.cooldowns[1] = 5.0;
        }
        app.update();
        assert!(queued(&app).contains(&name_id("ssoc_dance_lvl01")), "{:?}", queued(&app));
        // The HUD click and the cinematic.
        app.world_mut().resource_mut::<Messages<UiSound>>().write(UiSound::VerbClick);
        app.world_mut().resource_mut::<Messages<FlowEvent>>().write(FlowEvent::Start("CRG_BrainUpgrade1"));
        app.update();
        let q = queued(&app);
        assert!(q.contains(&name_id("editor_general_click")) && q.contains(&name_id("music_brain_upgrade")), "{q:?}");
    }

    /// Sweep (`creature-sound2`): the sounds the installed ability clips carry
    /// (what `anim_audio` plays when the clip drives the body) are installed and
    /// agree with the fallback list; the Creature families of every installed
    /// clip's sound events resolve; the `sound2` parameter names clips set.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_creature_clip_sounds_are_installed_and_listed() {
        use crate::anim_audio::{clip_cues, Cue};
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let mut problems: Vec<String> = vec![];
        for social in [true, false] {
            for slot in 0..4 {
                for rank in 1..=5u8 {
                    let id = crate::creature_clips::ability_action(social, slot, rank).unwrap();
                    let Some(clip) = crate::creature_clips::clip(&originals, id) else {
                        problems.push(format!("no clip {id:08X}"));
                        continue;
                    };
                    let names: Vec<String> = clip_cues(&clip.anim)
                        .into_iter()
                        .filter_map(|c| match c.cue {
                            Cue::Sound { name, .. } => Some(name),
                            _ => None,
                        })
                        .collect();
                    let mode = if social { EncounterMode::Social } else { EncounterMode::Combat };
                    let fallback = ability_sounds(mode, slot, rank);
                    eprintln!("{} slot {slot} rank {rank} clip {id:08X}: events {names:?} fallback {fallback:?}", if social { "social" } else { "combat" });
                    let lib = Some(std::sync::Arc::new(crate::game_audio::AudioLibrary::from_index(&originals.resource_index)));
                    for n in &names {
                        if n.starts_with('v') {
                            // The voices: what the clip path resolves, and the `b` bank the fallback used.
                            let got = crate::voice_banks::pick(&lib, n, "Ro", crate::anim_audio::intern);
                            let swapped = format!("b{}RoXCr", &n[1..]);
                            let b_installed = library.resolve(&swapped).is_ok();
                            eprintln!("    voice {n}: clip path -> {got:?}; {swapped} installed {b_installed}");
                        } else if n != "foot_step" && library.resolve(n).is_err() {
                            problems.push(format!("clip {id:08X} event {n} is not installed"));
                        }
                    }
                    for n in fallback {
                        if !n.starts_with('b') && !names.iter().any(|e| e == n) {
                            eprintln!("    fallback {n} is not an event of clip {id:08X}");
                        }
                    }
                }
            }
        }
        // Every installed clip: the Creature families, and the `sound2` names.
        let mut families: std::collections::BTreeMap<String, (usize, bool)> = Default::default();
        let mut params: std::collections::BTreeMap<String, usize> = Default::default();
        let keys: Vec<_> = originals.resource_index.records.keys().filter(|k| k.type_id == crate::decode::TYPE_ANIM).copied().collect();
        for key in keys {
            let Ok(anim) = originals.resource_index.read(key).map_err(|e| e.to_string()).and_then(|b| crate::animation::parse(&b)) else { continue };
            for c in clip_cues(&anim) {
                match c.cue {
                    Cue::Sound { name, .. } => {
                        const FAMILIES: [&str; 8] = ["sfx_crg_", "sfx_gen_", "scom_", "ssoc_", "smot_", "sfx_eat", "sfx_creature", "sfx_egg"];
                        if FAMILIES.iter().any(|f| name.starts_with(f)) {
                            let e = families.entry(name.clone()).or_insert((0, library.resolve(&name).is_ok()));
                            e.0 += 1;
                        }
                    }
                    Cue::Param { name, .. } => *params.entry(name).or_default() += 1,
                    Cue::Footstep { .. } => {}
                }
            }
        }
        for (name, (count, installed)) in &families {
            eprintln!("family {name}: {count} events, installed {installed}");
        }
        eprintln!("sound2 parameters set by clips: {params:?}");
        let unresolved: Vec<_> = families.iter().filter(|(_, (_, ok))| !ok).map(|(n, _)| n.clone()).collect();
        eprintln!("Creature-family clip sounds that are not installed: {unresolved:?}");
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// Every sound the module plays is installed (the retail egg sound is
    /// not, and is not played).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_creature_sounds_resolve() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let mut missing = vec![];
        for name in prefetch_names() {
            if library.resolve(name).is_err() {
                missing.push(name);
            }
        }
        for c in cinematics::CINEMATICS {
            for sound in cinematic_sounds(c.name) {
                if library.resolve(sound).is_err() {
                    missing.push(sound);
                }
            }
        }
        assert!(missing.is_empty(), "not installed: {missing:?}");
        assert!(library.resolve("sfx_egg_appear").is_err(), "the retail egg sound is not installed");
    }
}
