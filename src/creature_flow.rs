//! Creature stage flow: the moments (first-time hints, brain upgrades) and the
//! end of the stage (Evolve), after the retail cinematic scripts.
//!
//! Retail (`notes\creature-re-notes.md` C1D, `notes\xstage-re\cinematics`):
//! * every brain level the stage total reaches (100 / 300 / 600 / 1000 DNA)
//!   plays `CRG_BrainUpgrade<n>` (fade, the `music_brain_upgrade` music, the
//!   avatar's celebration clips), pauses the game and shows the level's text
//!   until the player presses the button; at level 4 the Advance ("Evolve")
//!   button appears;
//! * the first time DNA is earned, a creature is targeted, a pack member joins
//!   or a part is unlocked (and on the first failed social round) the
//!   `CRG_FirstDNA` / `CRG_TargetedCreature` / `CRG_FirstPosse` /
//!   `CRG_FirstPartUnlock` / `CRG_SocialFailure` moments freeze the game with a
//!   text window;
//! * Advance at the last level plays `CRG2TRG` (the stick, the campfire; the
//!   `crg_to_trg` mixer event and `music_crg_to_trg`), then the freeze frame
//!   text, then the stage timeline (history) window, and Tribe starts.
//!
//! With the data-driven cinematic player on (`crate::cinematics`, the
//! interactive game) every moment and the Evolve sequence are played by it
//! from the installed scripts: this module only detects them and hands them
//! over ([`crate::cinematics::request`]). Without it (smoke test, headless
//! runs, unit tests) the table of [`crate::creature_cinematics`] stands in:
//! each moment keeps its sounds, mixer events and text window and skips the
//! camera and animation part. The game is held (`pause_menu::simulation_active`)
//! while a text window is up, as `pauseGame` does. The windows need the SPUI
//! HUD; without it (headless runs, missing layouts) there is nothing to read or
//! click, so the moments pass without a window and the game never waits for a
//! dismissal. Enter (or the window's check button, which presses it) closes a
//! window.
//!
//! The first-time moments go through the retail gate (0xD399B0, VERIFIED): the
//! hint is shown only while the Tutorials option (config option 0x04EA96CB,
//! Settings > Game > Tutorials) is on, only once (the player flag of the hint
//! is written when it is shown, not when the event happens: with tutorials off
//! the event is still there when they are turned on), and never in a scenario.
//! `CRG_SocialFailure` tests the option itself (0xD3D386) before its flag
//! (0x33E07240). The brain upgrades, `CLG2CRG` and the death / migration
//! cinematics are not hints and play whatever the option says.
//!
//! ADAPTED: the retail triggers are code paths not decoded (the first-time
//! hints are one-shot flags of the player object there, `cPlayer::mOneTimeEventFlags`,
//! saved with the game); here the flags are `CreatureFlowSave` fields of the save, adopted silently when an old save is
//! loaded mid-stage, and the triggers are the gameplay state changes the
//! hint texts describe.

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    creature_actions::Encounter,
    creature_cinematics as cinematics,
    creature_hud::CreatureHudActive,
    creature_progression::MAX_BRAIN_LEVEL,
    game::RunProgress,
    stages::GameStage,
};

/// Advance (Evolve) button key and the key that closes a text window.
pub const ADVANCE_KEY: KeyCode = crate::creature_hud::ADVANCE_KEY;
pub const DISMISS_KEY: KeyCode = crate::cell_flow::DISMISS_KEY;

/// Persisted one-shot flags and counters of the stage flow.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreatureFlowSave {
    /// The flags below were adopted from the state of the save.
    pub initialized: bool,
    pub intro_shown: bool,
    pub first_dna: bool,
    pub first_part: bool,
    pub first_pack: bool,
    pub first_target: bool,
    pub first_social_failure: bool,
    /// Seconds played in the stage (the timeline clock).
    pub play_seconds: f32,
}

/// What the flow watches, once per frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub dna: f32,
    pub brain: u8,
    pub parts: usize,
    pub pack: usize,
    pub targeting: bool,
    /// The last encounter message reports a failed social round.
    pub social_failure: bool,
}

/// The moments the change from `prev` to `now` starts, in the order they play
/// (and the flags they set). The first call of a lineage adopts the state
/// without events (`save.initialized`). `tutorials` is the Tutorials option:
/// off, no first-time hint is shown and none of their flags is set (0xD399B0).
pub fn detect(
    prev: Option<&Snapshot>,
    now: &Snapshot,
    save: &mut CreatureFlowSave,
    tutorials: bool,
) -> Vec<&'static str> {
    let mut out = Vec::new();
    let Some(prev) = prev.filter(|_| save.initialized) else {
        // A save loaded mid-stage: what has happened is not shown again.
        save.initialized = true;
        save.intro_shown |= now.dna > 0.0 || now.brain > 0;
        if tutorials {
            save.first_dna |= now.dna > 0.0;
            save.first_part |= now.parts > 0;
            save.first_pack |= now.pack > 0;
        }
        return out;
    };
    if tutorials {
        if !save.first_dna && now.dna > prev.dna && now.dna > 0.0 {
            save.first_dna = true;
            out.push("CRG_FirstDNA");
        }
        if !save.first_target && now.targeting && !prev.targeting {
            save.first_target = true;
            out.push("CRG_TargetedCreature");
        }
        if !save.first_part && now.parts > prev.parts {
            save.first_part = true;
            out.push("CRG_FirstPartUnlock");
        }
        if !save.first_pack && now.pack > prev.pack {
            save.first_pack = true;
            out.push("CRG_FirstPosse");
        }
        if !save.first_social_failure && now.social_failure && !prev.social_failure {
            save.first_social_failure = true;
            out.push("CRG_SocialFailure");
        }
    }
    for level in (prev.brain + 1)..=now.brain.min(MAX_BRAIN_LEVEL) {
        if let Some(name) = cinematics::brain_upgrade(level) {
            out.push(name);
        }
    }
    out
}

/// Whether an encounter message reports a failed social proposal: every round
/// used without a full bar (0xDA4CAA state 8, after a match or a wrong answer:
/// "press E to try again") or the unanswered round running out ("lost
/// interest"). A wrong answer with rounds left only costs that round (state
/// 7 -> 4) and is no failure.
pub fn is_social_failure(message: &str) -> bool {
    message.contains("press E to try again") || message.contains("lost interest")
}

/// The Evolve sequence run by the cinematic player: `CRG2TRG` was requested
/// (the counter is the frames waited for it to start), then plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvolveByPlayer {
    Requested(u32),
    Playing,
}

/// Where the flow is. `Evolving` is the `CRG2TRG` cinematic (text window
/// without the player), `Summary` the stage timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Play,
    Evolving,
    Summary,
}

/// The stage timeline's facts (what the retail history events count).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreatureSummary {
    pub game_time_seconds: f32,
    /// `crg_killedsomething` / `crg_befriendedsomething` / `crg_death`.
    pub kills: u32,
    pub befriended: u32,
    pub deaths: u32,
    /// `crg_grow`: brain levels reached (0..=4).
    pub brain_level: u8,
    /// `crg_discoverpart`, `crg_packmemberadded`, `crg_migrated`.
    pub parts: usize,
    pub pack: usize,
    pub migrations: u8,
    /// `crg_alliedspecies` / `crg_extinctedspecies`.
    pub allied_species: usize,
    pub extinct_species: usize,
    /// Slot of the earned trait: 0 social, 1 mix, 2 attack
    /// (`crg_earnedtraitsocial/mix/attack`).
    pub trait_slot: usize,
}

/// The balance of the player's fight against social play in [-1, 1]
/// (`0xD386A0`): (attack DNA - social DNA) / the sum of the four brain-level
/// costs (the globals 0x1582E38..44 are `creatureEvoPointsToLevel_1..4`, read by
/// 0xD2E3A0), clamped. The two sums are cPlayer +0x127C / +0x1278, filled by
/// every kill / extinction / befriend / best-friends reward
/// (`CreatureProgression::attack_dna` / `social_dna`).
pub fn trait_balance(attack_dna: f32, social_dna: f32) -> f32 {
    let total: f32 = crate::creature_rules::EVO_POINTS_TO_LEVEL.iter().sum();
    if total > 0.0 && attack_dna.is_finite() && social_dna.is_finite() {
        ((attack_dna - social_dna) / total).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The earned trait slot (the retail `timelineEventLevelTraits` order: 0 social,
/// 1 mix, 2 attack). VERIFIED (the Evolve handler 0xD2C6xx picks
/// `crg_earnedtraitsocial` / `mix` / `attack` by 0xD38780): attack when the
/// balance is at least 1/3 (global 0x13F26CC), mix from -1/3 (0x140D10C) up,
/// else social. The timeline highlights the trait of the event the stage ends
/// with (`timelineEventLevelTraitsIndex`); the civ ids 0x0A35D0F5 / 0x3360727F /
/// 0x6AEB96A1 beside it in the timeline UI (0xE47BD0) are `cvg_military` /
/// `cvg_economic` / `cvg_religious`, the Civilization's.
pub fn trait_slot(attack_dna: f32, social_dna: f32) -> usize {
    let balance = trait_balance(attack_dna, social_dna);
    if balance >= 1.0 / 3.0 {
        2
    } else if balance >= -1.0 / 3.0 {
        1
    } else {
        0
    }
}

/// Mixer events the cinematics set (`mixevent <name> 1/0`): audio parameters
/// the music patches read.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct MixEvents(pub BTreeMap<&'static str, f32>);

impl MixEvents {
    pub fn set(&mut self, name: &'static str, on: bool) {
        self.0.insert(name, if on { 1.0 } else { 0.0 });
    }
}

/// Interface clicks of the Creature HUD the stage plays a sound for.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSound {
    /// A verb tray button (retail 0xD49671: `editor_general_click`).
    VerbClick,
}

/// What the audio needs to know about the flow.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowEvent {
    /// A cinematic starts: its sounds are played.
    Start(&'static str),
    /// A cinematic ends (window closed): its mixer events are released.
    End(&'static str),
}

#[derive(Resource, Default)]
pub struct CreatureFlow {
    pub phase: Phase,
    /// The cinematic whose text window is shown, and the ones waiting.
    pub moment: Option<&'static str>,
    pub queue: VecDeque<&'static str>,
    pub summary: Option<CreatureSummary>,
    /// Letterbox bars (every moment script starts with `letterbox -on`).
    pub letterbox: bool,
    /// A cinematic of the player is running: the game is held by it.
    pub player_busy: bool,
    /// `CRG2TRG` is played by the cinematic player.
    pub evolve_player: Option<EvolveByPlayer>,
    snapshot: Option<Snapshot>,
    /// Moments / events in order (tests, debugging).
    pub log: Vec<String>,
}

impl CreatureFlow {
    /// The game is frozen (`pauseGame`): a text window or the timeline is up.
    pub fn holding(&self) -> bool {
        self.moment.is_some() || self.phase != Phase::Play || self.player_busy
    }

    /// The HUD hides behind the stage timeline.
    pub fn hud_hidden(&self) -> bool {
        self.phase == Phase::Summary
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The text of a cinematic's window: its caption and the locale string.
pub fn moment_name(cinematic: &str) -> String {
    format!("moment:{cinematic}")
}

pub struct CreatureFlowPlugin;

impl Plugin for CreatureFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureFlow>()
            .init_resource::<MixEvents>()
            .add_message::<FlowEvent>()
            .add_message::<UiSound>()
            // `kUpgradeNest` (CRG_ClaimNest) redraws the claimed nest.
            .add_systems(Update, crate::creature_home::upgrade_model)
            // An extinct herd's nest is drawn abandoned (0xE8DF30).
            .add_systems(Update, crate::creature_home::abandon_nests)
            .add_systems(
                Update,
                (track, drive)
                    .chain()
                    .after(crate::creature_actions::update)
                    .after(crate::creature_hud::guide::bind_guide),
            );
    }
}

/// Watch the stage, queue moments (`CRG_BrainUpgrade<n>` and the first-time
/// ones) and count the play time.
fn track(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    hud: Res<CreatureHudActive>,
    mut run: ResMut<RunProgress>,
    encounter: Res<Encounter>,
    mut flow: ResMut<CreatureFlow>,
    mut quiet: Local<Option<bool>>,
    mut demo: Local<Option<Option<String>>>,
    player: Option<Res<crate::cinematics::Cinematics>>,
) {
    if *stage.get() != GameStage::Creature {
        if flow.snapshot.is_some() || flow.holding() || !flow.queue.is_empty() {
            flow.reset();
        }
        return;
    }
    if editor.open {
        return;
    }
    flow.player_busy = player.as_deref().is_some_and(|c| c.is_playing());
    // `--creature-hud-quiet` and the offscreen captures (`--quick-capture`,
    // `--render-smoke`): no introduction and no moments (they would freeze and
    // cover the captured scene), as the HUD guide's quiet mode; the demo flag
    // (`--creature-flow-demo`) still shows one.
    let quiet = *quiet.get_or_insert_with(|| {
        std::env::args().any(|a| matches!(a.as_str(), "--creature-hud-quiet" | "--quick-capture" | "--render-smoke"))
    });
    if !flow.holding() {
        run.creature_flow.play_seconds += time.delta_secs();
    }
    let now = Snapshot {
        dna: run.dna_this_stage,
        brain: run.creature_progression.brain_level,
        parts: run.creature_found_parts.len(),
        pack: run.creature_pack.members.len(),
        targeting: encounter.target.is_some(),
        social_failure: is_social_failure(&encounter.message),
    };
    // `--creature-flow-demo <cinematic name | summary>` (captures): show that
    // moment, or the stage timeline with sample numbers, once the HUD is up.
    let demo = demo.get_or_insert_with(|| {
        let args: Vec<String> = std::env::args().collect();
        args.iter().position(|a| a == "--creature-flow-demo").and_then(|i| args.get(i + 1).cloned())
    });
    if let (Some(what), true) = (demo.clone(), hud.0) {
        *demo = None;
        if what == "summary" {
            flow.phase = Phase::Summary;
            flow.summary = Some(CreatureSummary {
                game_time_seconds: 1500.0,
                kills: 14,
                befriended: 9,
                deaths: 2,
                brain_level: 4,
                parts: 3,
                pack: 3,
                migrations: 2,
                trait_slot: trait_slot(540.0, 360.0),
                ..default()
            });
        } else if let Some(def) = cinematics::CINEMATICS.iter().find(|c| c.name == what) {
            flow.queue.push_back(def.name);
        }
    }
    let prev = flow.snapshot.replace(now.clone());
    let fresh_lineage = now.dna <= 0.0 && now.brain == 0 && run.creature_lifecycle.generation <= 1;
    let mut save = std::mem::take(&mut run.creature_flow);
    let mut moments = detect(prev.as_ref(), &now, &mut save, crate::front_end::settings::tutorials_enabled());
    if quiet {
        moments.clear();
    }
    // A new lineage starts with the CLG2CRG text ("It is the beginning of a new
    // day..."), once the HUD shows.
    if fresh_lineage && hud.0 && !save.intro_shown && !quiet {
        save.intro_shown = true;
        moments.insert(0, "CLG2CRG");
    }
    run.creature_flow = save;
    // The data-driven cinematic player (`cinematics::hooks`) plays CLG2CRG and
    // the brain upgrades itself; their text windows here would show twice.
    let player_plays = player
        .as_deref()
        .is_some_and(|c| c.settings.hooks && c.runtime.enabled && c.loaded());
    for name in moments {
        if player_plays && (name == "CLG2CRG" || name.starts_with("CRG_BrainUpgrade")) {
            flow.log.push(format!("played by the cinematic player: {name}"));
            continue;
        }
        // The first-time moments are the player's too (queued behind a
        // running cinematic).
        if player_plays && crate::cinematics::request(name) {
            flow.log.push(format!("handed to the cinematic player: {name}"));
            continue;
        }
        flow.log.push(format!("queue {name}"));
        flow.queue.push_back(name);
    }
}

/// Run the queue: show a moment's window, close it on Enter, and drive the
/// Evolve sequence (Advance -> `CRG2TRG` text -> timeline -> Tribe).
#[allow(clippy::too_many_arguments)]
fn drive(
    keys: Res<ButtonInput<KeyCode>>,
    stage: Res<State<GameStage>>,
    hud: Res<CreatureHudActive>,
    mut run: ResMut<RunProgress>,
    mut flow: ResMut<CreatureFlow>,
    mut mix: ResMut<MixEvents>,
    mut events: MessageWriter<FlowEvent>,
    mut next: ResMut<NextState<GameStage>>,
    player: Option<Res<crate::cinematics::Cinematics>>,
) {
    if *stage.get() != GameStage::Creature {
        return;
    }
    let dismissed = keys.just_pressed(DISMISS_KEY);
    let windows = hud.0;
    // 0. The Evolve sequence played by the cinematic player: when `CRG2TRG`
    // ends (its last state sends kCinematicMessageTransitionToTribe, 0xD35190)
    // the stage timeline follows.
    if let Some(state) = flow.evolve_player {
        let running = player
            .as_deref()
            .is_some_and(|c| c.current().is_some_and(|(_, name, _)| name == "CRG2TRG") || c.runtime.queued() > 0);
        match state {
            EvolveByPlayer::Requested(frames) => {
                if running {
                    flow.evolve_player = Some(EvolveByPlayer::Playing);
                } else if frames > 120 {
                    // It never started: the text window path takes over.
                    flow.evolve_player = None;
                    flow.log.push("CRG2TRG did not start".into());
                    if !start_cinematic("CRG2TRG", &mut flow, &mut mix, &mut events, windows) {
                        flow.phase = Phase::Summary;
                        flow.summary = Some(summarize(&run));
                    }
                } else {
                    flow.evolve_player = Some(EvolveByPlayer::Requested(frames + 1));
                }
            }
            EvolveByPlayer::Playing => {
                if !running {
                    flow.evolve_player = None;
                    flow.log.push("end CRG2TRG".into());
                    flow.phase = Phase::Summary;
                    flow.summary = Some(summarize(&run));
                    flow.log.push("timeline".into());
                    if !windows {
                        finish_stage(&mut run, &mut flow, &mut next);
                    }
                }
            }
        }
        return;
    }
    // 1. Close the window being shown.
    if let Some(name) = flow.moment {
        if dismissed || !windows {
            flow.moment = None;
            flow.letterbox = false;
            end_cinematic(name, &mut mix, &mut events);
            flow.log.push(format!("end {name}"));
            if flow.phase == Phase::Evolving {
                // The `CRG2TRG` freeze frame closed: the stage timeline (the
                // retail 0xE3E350 window), then Tribe.
                flow.phase = Phase::Summary;
                flow.summary = Some(summarize(&run));
                flow.log.push("timeline".into());
                if !windows {
                    // Nothing to read: straight through.
                    finish_stage(&mut run, &mut flow, &mut next);
                }
            }
            return;
        }
        return;
    }
    // 2. The stage timeline is closed with the check button / Enter.
    if flow.phase == Phase::Summary {
        if dismissed || !windows {
            finish_stage(&mut run, &mut flow, &mut next);
        }
        return;
    }
    // 3. Advance at the last brain level (retail action 0x053F0369 with the
    // old level 4): `CRG2TRG`.
    let evolve_ready = run.creature_progression.brain_level >= MAX_BRAIN_LEVEL;
    if keys.just_pressed(ADVANCE_KEY) && evolve_ready && flow.phase == Phase::Play && flow.queue.is_empty() {
        flow.phase = Phase::Evolving;
        flow.log.push("advance".into());
        if player.as_deref().is_some_and(|c| !c.is_playing()) && crate::cinematics::request("CRG2TRG") {
            // The installed script plays the stick and the campfire story.
            flow.evolve_player = Some(EvolveByPlayer::Requested(0));
            return;
        }
        if !start_cinematic("CRG2TRG", &mut flow, &mut mix, &mut events, windows) {
            // No window to read: on to the timeline.
            flow.phase = Phase::Summary;
            flow.summary = Some(summarize(&run));
        }
        return;
    }
    // 4. The next queued moment.
    if flow.phase == Phase::Play {
        if let Some(name) = flow.queue.pop_front() {
            flow.log.push(format!("show {name}"));
            let _ = start_cinematic(name, &mut flow, &mut mix, &mut events, windows);
        }
    }
}

/// Start a cinematic: its mixer events and sounds go out, and its closing text
/// window opens. Returns whether a window was opened.
fn start_cinematic(
    name: &'static str,
    flow: &mut CreatureFlow,
    mix: &mut MixEvents,
    events: &mut MessageWriter<FlowEvent>,
    windows: bool,
) -> bool {
    let def = cinematics::cinematic(name);
    for event in def.map_or(&[][..], |d| d.mix) {
        mix.set(event, true);
    }
    events.write(FlowEvent::Start(name));
    // A moment without a text (or without a HUD to show it) is over at once.
    let text = def.is_some_and(|d| d.text.is_some());
    if text && windows {
        flow.moment = Some(name);
        flow.letterbox = true;
        true
    } else {
        end_cinematic(name, mix, events);
        false
    }
}

fn end_cinematic(name: &'static str, mix: &mut MixEvents, events: &mut MessageWriter<FlowEvent>) {
    if let Some(def) = cinematics::cinematic(name) {
        for event in def.mix {
            mix.set(event, false);
        }
    }
    events.write(FlowEvent::End(name));
}

/// Leave the stage: Tribe through the existing transition.
fn finish_stage(run: &mut RunProgress, flow: &mut CreatureFlow, next: &mut NextState<GameStage>) {
    flow.log.push("tribe".into());
    flow.phase = Phase::Play;
    flow.summary = None;
    run.advance();
    next.set(GameStage::Tribal);
    run.request_save();
}

/// The timeline numbers of the run so far.
pub fn summarize(run: &RunProgress) -> CreatureSummary {
    let outcomes = run.species_world.outcome_counts();
    let world = &run.species_world;
    let kills = outcomes.defeated as u32;
    let befriended = outcomes.allied as u32;
    CreatureSummary {
        game_time_seconds: run.creature_flow.play_seconds,
        kills,
        befriended,
        deaths: run.creature_lifecycle.deaths,
        brain_level: run.creature_progression.brain_level,
        parts: run.creature_found_parts.len(),
        pack: run.creature_pack.members.len(),
        migrations: run.creature_progression.migrations_completed,
        allied_species: world.species.iter().filter(|s| world.species_allied(s.id)).count(),
        extinct_species: world.species.iter().filter(|s| world.species_extinct(s.id)).count(),
        trait_slot: trait_slot(run.creature_progression.attack_dna, run.creature_progression.social_dna),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(dna: f32, brain: u8) -> Snapshot {
        Snapshot { dna, brain, ..default() }
    }

    #[test]
    fn a_loaded_save_adopts_its_state_silently() {
        let mut save = CreatureFlowSave::default();
        let now = Snapshot { dna: 450.0, brain: 2, parts: 1, pack: 2, ..default() };
        assert!(detect(None, &now, &mut save, true).is_empty());
        assert!(save.initialized && save.first_dna && save.first_part && save.first_pack && save.intro_shown);
        // Nothing replays afterwards.
        assert!(detect(Some(&now), &now, &mut save, true).is_empty());
        assert!(!save.first_target, "targeting is a moment of play, not of the state");
    }

    #[test]
    fn first_time_moments_fire_once_in_script_order() {
        let mut save = CreatureFlowSave::default();
        let start = snap(0.0, 0);
        assert!(detect(None, &start, &mut save, true).is_empty());
        let after = Snapshot { dna: 10.0, targeting: true, parts: 1, pack: 1, ..default() };
        assert_eq!(
            detect(Some(&start), &after, &mut save, true),
            ["CRG_FirstDNA", "CRG_TargetedCreature", "CRG_FirstPartUnlock", "CRG_FirstPosse"]
        );
        // Second time: nothing.
        let more = Snapshot { dna: 20.0, parts: 2, pack: 2, targeting: true, ..default() };
        assert!(detect(Some(&after), &more, &mut save, true).is_empty());
        let failure = Snapshot { social_failure: true, ..more.clone() };
        assert_eq!(detect(Some(&more), &failure, &mut save, true), ["CRG_SocialFailure"]);
        let again = Snapshot { social_failure: false, ..more.clone() };
        assert!(detect(Some(&failure), &again, &mut save, true).is_empty());
        assert!(detect(Some(&again), &failure, &mut save, true).is_empty(), "only the first failure");
    }

    #[test]
    fn first_time_hints_need_the_tutorials_option_and_keep_their_flags() {
        // 0xD399B0: option off -> no hint, and the flag stays unset so the hint
        // is still due when the option is turned on.
        let mut save = CreatureFlowSave::default();
        let start = snap(0.0, 0);
        assert!(detect(None, &start, &mut save, false).is_empty());
        let after = Snapshot { dna: 10.0, targeting: true, parts: 1, pack: 1, social_failure: true, ..default() };
        assert!(detect(Some(&start), &after, &mut save, false).is_empty());
        assert!(!save.first_dna && !save.first_target && !save.first_part && !save.first_pack);
        assert!(!save.first_social_failure);
        // The brain upgrades are no hints: they play with the option off.
        let level = Snapshot { dna: 100.0, brain: 1, ..after.clone() };
        assert_eq!(detect(Some(&after), &level, &mut save, false), ["CRG_BrainUpgrade1"]);
        // Turned on later, the next change shows the hints that are still due.
        let more = Snapshot { dna: 110.0, brain: 1, ..after.clone() };
        assert_eq!(detect(Some(&level), &more, &mut save, true), ["CRG_FirstDNA"]);
        // A save loaded with the option off adopts nothing of the hints.
        let mut loaded = CreatureFlowSave::default();
        let late = Snapshot { dna: 450.0, brain: 2, parts: 1, pack: 2, ..default() };
        assert!(detect(None, &late, &mut loaded, false).is_empty());
        assert!(loaded.initialized && !loaded.first_dna && !loaded.first_part && !loaded.first_pack);
    }

    #[test]
    fn every_brain_level_reached_plays_its_upgrade() {
        let mut save = CreatureFlowSave { initialized: true, first_dna: true, ..default() };
        assert_eq!(detect(Some(&snap(99.0, 0)), &snap(100.0, 1), &mut save, true), ["CRG_BrainUpgrade1"]);
        // Two levels in one step play in order.
        assert_eq!(
            detect(Some(&snap(100.0, 1)), &snap(600.0, 3), &mut save, true),
            ["CRG_BrainUpgrade2", "CRG_BrainUpgrade3"]
        );
        assert_eq!(detect(Some(&snap(999.0, 3)), &snap(1000.0, 4), &mut save, true), ["CRG_BrainUpgrade4"]);
        assert!(detect(Some(&snap(1000.0, 4)), &snap(1100.0, 4), &mut save, true).is_empty());
    }

    #[test]
    fn failed_social_rounds_are_recognised_by_the_encounter_message() {
        assert!(is_social_failure("Wrong answer: they performed Dance. Friendship 40% | press E to try again"));
        assert!(is_social_failure("Matched their Sing, but the Grypher has nothing more to show. Friendship 60% | press E to try again"));
        assert!(is_social_failure("The Grypher lost interest and wandered off"));
        // A wrong answer with rounds left only costs the round (state 7 -> 4).
        assert!(!is_social_failure("Wrong answer: they performed Dance. Friendship 40% | answer the next one"));
        assert!(!is_social_failure("Sing: friendship 40%"));
        assert!(!is_social_failure(""));
    }

    #[test]
    fn the_earned_trait_follows_the_balance_of_attack_and_social_dna() {
        // 0xD386A0 / 0xD38780: (attack - social) / 1000 against +-1/3.
        assert_eq!(trait_slot(0.0, 0.0), 1);
        assert_eq!(trait_slot(0.0, 400.0), 0);
        assert_eq!(trait_slot(400.0, 0.0), 2);
        assert_eq!(trait_slot(300.0, 300.0), 1);
        assert_eq!(trait_slot(0.0, 333.0), 1, "just above -1/3");
        assert_eq!(trait_slot(0.0, 334.0), 0);
        assert_eq!(trait_slot(333.0, 0.0), 1);
        assert_eq!(trait_slot(334.0, 0.0), 2);
        assert_eq!(trait_balance(5000.0, 0.0), 1.0);
        assert_eq!(trait_balance(0.0, 5000.0), -1.0);
        assert_eq!(trait_balance(f32::NAN, 1.0), 0.0);
    }

    #[test]
    fn the_summary_reads_the_dna_split_the_rewards_filled() {
        let mut run = RunProgress::default();
        assert_eq!(summarize(&run).trait_slot, 1);
        run.creature_progression.social_dna = 600.0;
        run.creature_progression.attack_dna = 100.0;
        assert_eq!(summarize(&run).trait_slot, 0);
        run.creature_progression.attack_dna = 900.0;
        run.creature_progression.social_dna = 100.0;
        assert_eq!(summarize(&run).trait_slot, 2);
    }

    #[test]
    fn mix_events_toggle_by_name() {
        let mut mix = MixEvents::default();
        mix.set("crg_cin_start", true);
        assert_eq!(mix.0.get("crg_cin_start"), Some(&1.0));
        mix.set("crg_cin_start", false);
        assert_eq!(mix.0.get("crg_cin_start"), Some(&0.0));
    }

    /// Counts the frames the simulation set runs.
    #[derive(Resource, Default)]
    struct SimTicks(u32);

    fn count_sim(mut ticks: ResMut<SimTicks>) {
        ticks.0 += 1;
    }

    /// A Creature stage app whose save has seen the introduction and the first
    /// DNA (the tests set what they need).
    fn creature_app(windows: bool) -> App {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        run.creature_flow = CreatureFlowSave { initialized: true, intro_shown: true, first_dna: true, ..default() };
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<SimTicks>();
        app.add_systems(Update, count_sim.in_set(crate::GameSystems::Simulation));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        app.update();
        app.update();
        // Pretend the HUD is on screen (the layouts need the install).
        app.world_mut().resource_mut::<CreatureHudActive>().0 = windows;
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    fn stage(app: &App) -> GameStage {
        app.world().resource::<State<GameStage>>().get().clone()
    }

    #[test]
    fn a_brain_upgrade_freezes_the_game_until_the_window_is_closed() {
        let mut app = creature_app(true);
        app.update();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 100.0;
            run.creature_progression.observe_dna(100.0);
        }
        for _ in 0..3 {
            app.update();
        }
        let flow = app.world().resource::<CreatureFlow>();
        assert_eq!(flow.moment, Some("CRG_BrainUpgrade1"), "{:?}", flow.log);
        assert!(flow.holding() && flow.letterbox);
        let before = app.world().resource::<SimTicks>().0;
        app.update();
        app.update();
        assert_eq!(app.world().resource::<SimTicks>().0, before, "the simulation is held");
        let mix = app.world().resource::<MixEvents>();
        assert!(mix.0.get("crg_cin_start").is_none(), "the brain upgrade sets no mixer event");
        press(&mut app, DISMISS_KEY);
        let flow = app.world().resource::<CreatureFlow>();
        assert_eq!(flow.moment, None);
        assert!(!flow.holding());
        let before = app.world().resource::<SimTicks>().0;
        app.update();
        assert_eq!(app.world().resource::<SimTicks>().0, before + 1, "the simulation runs again");
    }

    #[test]
    fn evolve_plays_the_tribe_cinematic_then_the_timeline_then_tribe() {
        let mut app = creature_app(true);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 1000.0;
            run.creature_progression.observe_dna(1000.0);
            run.creature_progression.brain_level = 4;
        }
        // The four brain upgrades play first (each window is closed with
        // Enter), then Advance works.
        for level in 1..=4 {
            app.update();
            app.update();
            let flow = app.world().resource::<CreatureFlow>();
            assert_eq!(flow.moment, cinematics::brain_upgrade(level), "{:?}", flow.log);
            press(&mut app, DISMISS_KEY);
        }
        app.update();
        assert!(app.world().resource::<CreatureFlow>().queue.is_empty());
        press(&mut app, ADVANCE_KEY);
        let flow = app.world().resource::<CreatureFlow>();
        assert_eq!(flow.phase, Phase::Evolving);
        assert_eq!(flow.moment, Some("CRG2TRG"));
        assert_eq!(app.world().resource::<MixEvents>().0.get("crg_to_trg"), Some(&1.0));
        assert_eq!(stage(&app), GameStage::Creature);
        press(&mut app, DISMISS_KEY);
        let flow = app.world().resource::<CreatureFlow>();
        assert_eq!(flow.phase, Phase::Summary, "the timeline follows the freeze frame");
        assert!(flow.summary.is_some() && flow.hud_hidden());
        assert_eq!(app.world().resource::<MixEvents>().0.get("crg_to_trg"), Some(&0.0));
        assert_eq!(stage(&app), GameStage::Creature);
        press(&mut app, DISMISS_KEY);
        app.update();
        assert_eq!(stage(&app), GameStage::Tribal, "{:?}", app.world().resource::<CreatureFlow>().log);
    }

    #[test]
    fn evolve_through_the_cinematic_player_shows_the_timeline_when_its_story_ends() {
        use crate::cinematics::{runtime::Program, script::Library, Cinematics};
        let mut app = creature_app(true);
        {
            let mut library = Library::default();
            library.add_file("global", "state stopCurrentCinematic\n stopCinematic\nend\nstate switchToExitState\n switchToExitState\nend\n");
            library.add_file("test", "state CRG2TRG\n wait -secs .3\nend\n");
            let mut c = app.world_mut().resource_mut::<Cinematics>();
            c.set_program(Program::compile(&library));
            c.runtime.enabled = true;
        }
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 1000.0;
            run.creature_progression.observe_dna(1000.0);
            run.creature_progression.brain_level = 4;
        }
        app.update();
        app.update();
        {
            // The four brain upgrades are not the subject here.
            let mut flow = app.world_mut().resource_mut::<CreatureFlow>();
            flow.queue.clear();
            flow.moment = None;
            // `cinematics::request("CRG2TRG")` was granted by the player.
            flow.phase = Phase::Evolving;
            flow.evolve_player = Some(EvolveByPlayer::Requested(0));
        }
        app.world_mut().resource_mut::<Cinematics>().play("CRG2TRG", false, false).expect("the story plays");
        app.update();
        assert_eq!(app.world().resource::<CreatureFlow>().evolve_player, Some(EvolveByPlayer::Playing));
        assert!(app.world().resource::<CreatureFlow>().holding());
        assert_eq!(stage(&app), GameStage::Creature);
        for _ in 0..80 {
            std::thread::sleep(std::time::Duration::from_millis(15));
            app.update();
            if app.world().resource::<CreatureFlow>().phase == Phase::Summary {
                break;
            }
        }
        let flow = app.world().resource::<CreatureFlow>();
        assert_eq!(flow.phase, Phase::Summary, "the timeline follows the story: {:?}", flow.log);
        assert_eq!(flow.evolve_player, None);
        assert!(flow.summary.is_some());
        assert_eq!(stage(&app), GameStage::Creature);
        press(&mut app, DISMISS_KEY);
        app.update();
        assert_eq!(stage(&app), GameStage::Tribal, "{:?}", app.world().resource::<CreatureFlow>().log);
    }

    #[test]
    fn without_a_hud_the_moments_pass_and_advance_goes_straight_through() {
        let mut app = creature_app(false);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 1000.0;
            run.creature_progression.brain_level = 4;
        }
        for _ in 0..8 {
            app.update();
        }
        let flow = app.world().resource::<CreatureFlow>();
        assert!(!flow.holding(), "no window, no hold: {:?}", flow.log);
        assert!(flow.queue.is_empty());
        press(&mut app, ADVANCE_KEY);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(stage(&app), GameStage::Tribal);
    }

    #[test]
    fn advance_needs_the_last_brain_level() {
        let mut app = creature_app(true);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 700.0;
            run.creature_progression.brain_level = 3;
        }
        app.update();
        press(&mut app, ADVANCE_KEY);
        app.update();
        assert_eq!(app.world().resource::<CreatureFlow>().phase, Phase::Play);
        assert_eq!(stage(&app), GameStage::Creature);
    }
}
