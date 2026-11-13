//! The original's data-driven cinematic player (every stage).
//!
//! Retail Spore has no cinematic assets: a cinematic is a named state of the
//! ArgScript state scripts `scripts3!*.trigger` (14 files, 266 states, 41
//! command keywords), played by `cCinematicManager` (notes\cinematics-re-notes.md,
//! by `xstage-re`). This module ports that player:
//!
//! * [`script`] reads the installed scripts (`sinclude`, `state ... end`,
//!   commands with arguments and `-options`);
//! * [`command`] turns each line into a typed [`command::Command`];
//! * [`runtime`] is the state machine of section 2 (entry execution,
//!   blockers, `stateDuration`, `nextState`, exit states, Escape, pause,
//!   `letAIRun`, UI waits), without Bevy;
//! * [`camera`] is the `startCamera` controller of section 3.1;
//! * `host` executes the commands in the game: the `Tutorial_letterbox_txt`
//!   SPUI layout (letterbox bars, text, Continue button, black screen),
//!   Swarm effects (fades, freeze frames), sounds, audio parameters
//!   (`mixEvent` -> [`AudioParamSet`]), game messages (`sendMessage` ->
//!   [`CinematicMessage`]), actors (`addActor`, `moveActor`, `playAnimation`)
//!   as far as OpenSpore's creatures allow, the camera, input lock and HUD
//!   hiding;
//! * `hooks` plays the demo cinematics (`CLG2CRG` at Creature start, the brain
//!   upgrades, `TRG_Begin` at Tribal start) and drives `--cinematic <name>`
//!   quick captures.
//!
//! Stage code API: `ResMut<Cinematics>` ([`Cinematics::play`],
//! [`Cinematics::bind_object`], [`Cinematics::continue_cinematic`],
//! [`Cinematics::set_text_from_code`], [`Cinematics::is_playing`]) or the
//! [`PlayCinematic`] message; listen to [`CinematicStarted`],
//! [`CinematicEnded`] (retail 0x44F1189) and [`CinematicMessage`].

// The API is for stage code; not every entry point has a caller yet.
#![allow(dead_code)]

pub mod camera;
pub mod command;
pub mod runtime;
pub mod script;

mod actors;
mod hooks;
mod host;
mod moments;

pub use hooks::quick_capture;
pub use host::{spawn_avatar_copy, CinematicActor, Scene as CinematicScene};
pub use moments::{available, hold_death, laying_egg, request, request_laid_egg, request_stop, take_skipped, DeathCause};
pub use runtime::{AiMode, PauseMode};

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use crate::world_objects::name_id;
use runtime::{Program, Runtime};

/// A playing (or queued) cinematic: the id of its entry state, like the
/// retail token returned by `PlayCinematic`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CinematicToken(pub u32);

/// What a script name (`"AvatarNest"`, `"TribeFire"`, ...) refers to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CinematicTarget {
    /// A world entity (its transform is read every frame).
    Entity(Entity),
    /// A fixed point with a facing yaw (Bevy rotation about +Y; the object's
    /// front is its local -Z).
    Point { position: Vec3, yaw: f32 },
}

/// "Cinematic started" (retail message 0x604C6E0).
#[derive(Message, Clone, Debug)]
pub struct CinematicStarted {
    pub token: CinematicToken,
    pub name: String,
}

/// "Cinematic stopped" (retail message 0x44F1189, sent with the token).
#[derive(Message, Clone, Debug)]
pub struct CinematicEnded {
    pub token: CinematicToken,
    pub name: String,
    /// Ended through Escape / the exit state.
    pub skipped: bool,
}

/// `sendMessage -id <id> [-data1..4 v]`: the stage message ids the retail
/// stages listen to (kEnterCreatureEditor 0x04D9686F, kRestartAtNest
/// 0x04F60B92, kHatchAvatarEggs 0x06566531, kUpgradeNest 0x06555ABC,
/// kCinematicMessageTransitionToTribe 0x0477F66C, ...). `data` holds the raw
/// 32-bit values (integers as is, floats as their bits).
#[derive(Message, Clone, Debug)]
pub struct CinematicMessage {
    pub token: CinematicToken,
    pub id: u32,
    pub data: [u32; 4],
}

/// `mixEvent <name> <value>`: set the game audio parameter FNV(name)
/// (retail 0x5CA970, read by the Pd patches). The Pd engine (`pd-engine`,
/// `src\pd\*`) consumes this message; [`CinematicMix`] keeps the last values.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct AudioParamSet {
    pub hash: u32,
    pub value: f32,
}

/// Last value of every audio parameter a cinematic set.
#[derive(Resource, Default, Debug)]
pub struct CinematicMix(pub HashMap<u32, f32>);

/// Play a cinematic from stage code without holding `ResMut<Cinematics>`.
#[derive(Message, Clone, Debug)]
pub struct PlayCinematic {
    pub name: String,
    pub pause_game: bool,
    pub queue: bool,
    /// Named objects bound right after the start (`BindObject`).
    pub bindings: Vec<(String, CinematicTarget)>,
}

impl PlayCinematic {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), pause_game: false, queue: true, bindings: Vec::new() }
    }

    pub fn bind(mut self, name: &str, target: CinematicTarget) -> Self {
        self.bindings.push((name.to_string(), target));
        self
    }
}

/// Behaviour switches (not retail data).
#[derive(Clone, Debug)]
pub struct CinematicSettings {
    /// Play the demo cinematics (Creature start, brain upgrades, Tribal start).
    pub hooks: bool,
    /// ADAPTED: OpenSpore sends none of the game events that end
    /// `wait -forContinue` (0xD7C360 brain upgrade, 0xD6CDB0 mating, ...);
    /// continue on our own after this many seconds.
    pub continue_fallback: Option<f32>,
    /// Captures: press the Continue button after it has been shown this long.
    pub auto_continue: Option<f32>,
}

impl Default for CinematicSettings {
    fn default() -> Self {
        Self { hooks: false, continue_fallback: Some(3.0), auto_continue: None }
    }
}

/// The cinematic manager: state machine, settings and stage bindings.
#[derive(Resource)]
pub struct Cinematics {
    pub runtime: Runtime,
    pub settings: CinematicSettings,
    loaded: bool,
    text_from_code: HashMap<u32, String>,
    bindings: HashMap<(u32, u32), CinematicTarget>,
    /// Load summary (states, unknown commands, errors).
    pub report: String,
}

impl Default for Cinematics {
    fn default() -> Self {
        Self {
            runtime: Runtime::new(Arc::new(Program::default())),
            settings: CinematicSettings::default(),
            loaded: false,
            text_from_code: HashMap::new(),
            bindings: HashMap::new(),
            report: String::new(),
        }
    }
}

impl Cinematics {
    /// The installed scripts are loaded.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// Load a compiled program (startup, tests).
    pub fn set_program(&mut self, program: Program) {
        self.report = format!(
            "{} states from {} scripts, {} unknown command lines, {} load errors",
            program.states.len(),
            program.files.len(),
            program.unknown.len(),
            program.errors.len()
        );
        self.runtime.set_program(Arc::new(program));
        self.loaded = true;
    }

    /// `PlayCinematic(name, pauseGame, ..., queue)`: the token, or `None`
    /// when the state is unknown, cinematics are disabled, or one is already
    /// playing and `queue` is false.
    pub fn play(&mut self, name: &str, pause_game: bool, queue: bool) -> Option<CinematicToken> {
        self.runtime.play(name, pause_game, queue).map(CinematicToken)
    }

    /// `BindObject(name, object, token)` (0xAE0930): what a script name means
    /// for this cinematic.
    pub fn bind_object(&mut self, token: CinematicToken, name: &str, target: CinematicTarget) {
        self.bindings.insert((token.0, name_id(name)), target);
    }

    /// Message 0x445F729: ends `wait -forButton/-forContinue/-forNext`.
    pub fn continue_cinematic(&mut self) {
        self.runtime.continue_cinematic();
    }

    /// Text for `textFromCode "<key>"` (0xADB350; "ERROR" when missing).
    pub fn set_text_from_code(&mut self, key: &str, text: impl Into<String>) {
        self.text_from_code.insert(name_id(key), text.into());
    }

    pub fn is_playing(&self) -> bool {
        self.runtime.is_playing()
    }

    /// The playing cinematic and the name of its current state.
    pub fn current(&self) -> Option<(CinematicToken, &str, &str)> {
        let token = self.runtime.token()?;
        Some((CinematicToken(token), self.runtime.name(), self.runtime.current_state().unwrap_or("")))
    }

    /// Code stop (0xAD7D40, e.g. the "hold" cinematics `CLG_intro`).
    pub fn stop(&mut self) {
        self.runtime.stop_current();
    }

    /// Code stop of the cinematic `name`: the running one ends, a queued one
    /// is dropped before it starts. Whether anything was stopped.
    pub fn stop_named(&mut self, name: &str) -> bool {
        if self.is_playing() && self.runtime.name() == name {
            self.runtime.stop_current();
            return true;
        }
        self.runtime.unqueue(name)
    }

    /// As if the player pressed Escape.
    pub fn skip(&mut self) {
        self.runtime.escape();
    }

    /// The pause the cinematic asks for (`pauseGame`).
    pub fn pause_mode(&self) -> PauseMode {
        self.runtime.pause_mode()
    }

    /// `letAIRun` state: which AI may run while a cinematic plays.
    pub fn ai_mode(&self) -> AiMode {
        self.runtime.ai_mode()
    }

    fn binding(&self, token: u32, name: u32) -> Option<CinematicTarget> {
        self.bindings.get(&(token, name)).copied()
    }

    fn text_from_code(&self, key: &str) -> String {
        self.text_from_code.get(&name_id(key)).cloned().unwrap_or_else(|| "ERROR".to_string())
    }

    fn forget_bindings(&mut self, token: u32) {
        self.bindings.retain(|(t, _), _| *t != token);
    }
}

/// Run condition for the game simulation: false while a cinematic pauses the
/// game (`pauseGame`, `cGameTimeManager::Pause(Cinematic)`) and while it plays
/// without `letAIRun` (retail runs no AI during a cinematic unless the script
/// lets it, M+0x28/0x29). ADAPTED: OpenSpore has no separate AI switch, the
/// simulation set stands for it; `letAIRun -avatarOnly` keeps it stopped too
/// (the avatar is driven by the cinematic's actor actions).
pub fn simulation_active(cinematics: Option<Res<Cinematics>>, stage: Option<Res<State<crate::stages::GameStage>>>) -> bool {
    // ADAPTED: the Cell game (`cCellGame`) is not the simulator's AI: its
    // creatures keep running during a Cell cinematic (`goldies_demise` plays
    // over the live scene) unless the script pauses the game.
    let cell = stage.is_some_and(|s| *s.get() == crate::stages::GameStage::Cell);
    cinematics.is_none_or(|c| {
        !c.is_playing() || (c.pause_mode() == PauseMode::None && (c.ai_mode() == AiMode::All || cell))
    })
}

/// True while a cinematic plays (input locked, HUD hidden).
pub fn playing(cinematics: Option<Res<Cinematics>>) -> bool {
    cinematics.is_some_and(|c| c.is_playing())
}

pub struct CinematicsPlugin;

impl Plugin for CinematicsPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let mut cinematics = Cinematics::default();
        // The smoke test, the render smoke campaign and the unit tests drive
        // the stages themselves: no cinematic may take their input away.
        let automated = cfg!(test) || has("--smoke-test") || has("--render-smoke");
        let capture = has("--quick-capture");
        cinematics.runtime.enabled = !automated;
        cinematics.settings.hooks = !automated && (!capture || has("--cinematics"));
        if capture {
            cinematics.settings.auto_continue = Some(1.5);
        }
        app.insert_resource(cinematics)
            .init_resource::<CinematicMix>()
            .init_resource::<host::Scene>()
            .add_message::<CinematicStarted>()
            .add_message::<CinematicEnded>()
            .add_message::<CinematicMessage>()
            .add_message::<AudioParamSet>()
            .add_message::<PlayCinematic>()
            .add_systems(PreUpdate, host::input_gate.after(bevy::input::InputSystems))
            .add_systems(
                Update,
                (
                    host::load_scripts,
                    moments::publish_availability,
                    host::play_requests,
                    hooks::stage_hooks,
                    moments::update,
                )
                    .chain(),
            )
            .add_systems(Last, moments::gate)
            .add_systems(
                PostUpdate,
                host::drive
                    .before(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{state::app::StatesPlugin, time::TimeUpdateStrategy};
    use command::Command;
    use std::time::Duration;

    /// Every state and command of the installed scripts parses; unknown
    /// keywords are listed (not fatal). The demo cinematics' scripted
    /// lengths match the notes (sections 4 and 5) when every actor action and
    /// camera move finishes at once and the button is pressed at once.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_scripts_parse_and_demo_lengths_match_the_notes() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = script::Library::from_index(&index);
        println!("files: {:?}", library.files);
        for e in &library.errors {
            println!("load: {e}");
        }
        let program = Program::compile(&library);
        let mut keywords = std::collections::BTreeMap::<String, usize>::new();
        let mut lines = 0;
        for state in library.states.values() {
            for line in &state.lines {
                *keywords.entry(line.keyword.clone()).or_default() += 1;
                lines += 1;
            }
        }
        println!("{} states, {} command lines, {} keywords: {keywords:?}", program.states.len(), lines, keywords.len());
        for (file, line, keyword) in &program.unknown {
            println!("unknown command `{keyword}` at {file}:{line}");
        }
        assert_eq!(library.files.len(), 14, "config + the 13 included scripts");
        // The notes count 266 cinematic states; the 14 files hold 719 states
        // in all (Space hints, tests and the Civ/Tribe moments included).
        assert!(program.states.len() >= 700, "states of the installed scripts");
        assert!(program.unknown.is_empty(), "every installed keyword is known");
        for name in [
            "GGE_intro", "GGE2CLG", "CLG_intro", "goldies_demise", "cell_ending", "CLG2CRG", "CRG_FirstDNA",
            "CRG_SpeciesMigrate", "CRG_ClaimNest", "CRG_LaidEgg", "CRG_AvatarDied", "CRG_BrainUpgrade1",
            "CRG_BrainUpgrade4", "CRG2TRG", "TRG_Begin", "TRG_Upgrade", "TRG2CVG_ShowCityHall", "CVG_UnlockSea",
            "CVG2SPG", "SPG_CenterOfGalaxy", "stopCurrentCinematic", "switchToExitState",
        ] {
            assert!(program.states.contains_key(&name_id(name)), "{name}");
        }
        let program = Arc::new(program);
        // (name, scripted seconds from the notes)
        for (name, expected) in [("CLG2CRG", 28.935), ("TRG_Begin", 13.335), ("cell_ending", 9.0), ("goldies_demise", 10.0)] {
            let mut rt = Runtime::new(program.clone());
            assert!(rt.play(name, false, false).is_some(), "{name}");
            let dt = 1.0 / 120.0;
            let mut t = 0.0;
            let mut presses = 0;
            while rt.is_playing() && t < 120.0 {
                for op in rt.take_ops() {
                    if let runtime::Op::Run { blocker: Some(b), .. } = op {
                        rt.finish(b);
                    }
                }
                if rt.continue_shown() {
                    presses += 1;
                    rt.continue_cinematic();
                }
                rt.update(dt);
                t += dt;
            }
            println!("{name}: {t:.3} s scripted, {presses} button(s)");
            assert!(!rt.is_playing(), "{name} ends");
            // One frame per state change on top of the waits.
            assert!(t >= expected && t < expected + 0.2, "{name}: {t} vs {expected}");
            assert!(presses >= 1, "{name} ends on a button");
        }
    }

    fn simulation_active_now(world: &World) -> bool {
        world.resource::<Cinematics>().pause_mode() == PauseMode::None
    }

    fn app(text: &str) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
            .init_state::<crate::stages::GameStage>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_message::<bevy::input::mouse::MouseMotion>()
            .add_plugins(CinematicsPlugin);
        let mut library = script::Library::default();
        library.add_file(
            "global",
            "state stopCurrentCinematic\n stopCinematic\nend\nstate switchToExitState\n switchToExitState\nend\n",
        );
        library.add_file("test", text);
        let mut c = app.world_mut().resource_mut::<Cinematics>();
        c.set_program(Program::compile(&library));
        c.runtime.enabled = true;
        app.update();
        app
    }

    #[derive(Resource, Default)]
    struct Seen {
        started: usize,
        ended: usize,
        params: Vec<(u32, f32)>,
        messages: Vec<u32>,
    }

    fn record(
        mut seen: ResMut<Seen>,
        mut started: MessageReader<CinematicStarted>,
        mut ended: MessageReader<CinematicEnded>,
        mut params: MessageReader<AudioParamSet>,
        mut messages: MessageReader<CinematicMessage>,
    ) {
        seen.started += started.read().count();
        seen.ended += ended.read().count();
        seen.params.extend(params.read().map(|p| (p.hash, p.value)));
        seen.messages.extend(messages.read().map(|m| m.id));
    }

    #[test]
    fn the_bevy_host_plays_a_cinematic_end_to_end() {
        let mut app = app(
            "state Demo\n letterbox -on\n mixevent cell_trans_end 1\n text \"Hello\" nosuchtable!0x1\n wait -secs .5\n nextState Freeze\nend\n\
             state Freeze\n pauseGame -pauseGraphics\n wait -forButton\n nextState Done\nend\n\
             state Done\n unpauseGame\n sendMessage -id 0x04d9686f\nend\n",
        );
        app.init_resource::<Seen>().add_systems(Last, record);
        let token = app.world_mut().resource_mut::<Cinematics>().play("Demo", false, false);
        assert_eq!(token, Some(CinematicToken(name_id("Demo"))));
        app.update();
        {
            let world = app.world();
            assert!(world.resource::<Cinematics>().is_playing());
            let scene = world.resource::<host::Scene>();
            assert!(scene.letterbox());
            assert_eq!(scene.text(), Some("Hello"), "missing table -> the script's text");
            let seen = world.resource::<Seen>();
            assert_eq!(seen.started, 1);
            assert_eq!(seen.params, vec![(name_id("cell_trans_end"), 1.0)]);
        }
        for _ in 0..40 {
            app.update();
        }
        {
            let world = app.world();
            assert_eq!(world.resource::<Cinematics>().current().unwrap().2, "Freeze");
            assert_eq!(world.resource::<Cinematics>().pause_mode(), PauseMode::CinematicAll);
            assert!(!simulation_active_now(world), "pauseGame stops the simulation");
            assert_eq!(world.resource::<host::Scene>().continue_ui(), Some(runtime::ContinueUi::Button));
        }
        // Input is locked: the game never sees the key, Enter continues.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
        for _ in 0..5 {
            app.update();
        }
        let world = app.world();
        assert!(!world.resource::<Cinematics>().is_playing());
        assert!(simulation_active_now(world));
        assert!(!world.resource::<host::Scene>().letterbox());
        let seen = world.resource::<Seen>();
        assert_eq!(seen.ended, 1);
        assert_eq!(seen.messages, vec![0x04D9_686F]);
    }

    #[test]
    fn escape_skips_to_the_exit_state_and_play_requests_bind() {
        let mut app = app(
            "state Long\n setExitState Exit\n letterbox -on\n wait -secs 100\nend\nstate Exit\n letterbox -off\n mixevent skipped 1\nend\n",
        );
        app.init_resource::<Seen>().add_systems(Last, record);
        app.world_mut().write_message(PlayCinematic::new("Long").bind(
            "AvatarNest",
            CinematicTarget::Point { position: Vec3::ONE, yaw: 0.0 },
        ));
        app.update();
        app.update();
        let token = {
            let c = app.world().resource::<Cinematics>();
            assert!(c.is_playing());
            let token = c.runtime.token().unwrap();
            assert!(c.binding(token, name_id("AvatarNest")).is_some());
            token
        };
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        for _ in 0..6 {
            app.update();
        }
        let world = app.world();
        let c = world.resource::<Cinematics>();
        assert!(!c.is_playing());
        assert!(c.binding(token, name_id("AvatarNest")).is_none(), "bindings end with the cinematic");
        assert!(world.resource::<Seen>().params.contains(&(name_id("skipped"), 1.0)));
        assert!(matches!(
            Command::parse(&script::Line::parse("letterbox -off", 1).unwrap()),
            Command::Letterbox { on: false, .. }
        ));
    }
    /// What the demo cinematics use is installed: effects, sounds, texts and
    /// the animation lengths their blocking `playAnimation`s wait for.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_assets_of_the_demo_cinematics() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = script::Library::from_index(&index);
        let program = Program::compile(&library);
        let swarm = crate::swarm_fx::SwarmLibrary::load(&index);
        let audio = crate::game_audio::AudioLibrary::from_index(&index);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let index = &originals.resource_index;
        println!(
            "action lists: {} parsed, {} errors {:?}",
            originals.action_lists.lists.len(),
            originals.action_lists.errors.len(),
            originals.action_lists.errors.iter().take(4).collect::<Vec<_>>()
        );
        for (key, list) in &originals.action_lists.lists {
            if let Some(g) = list.groups.iter().find(|g| g.id == 0x0541_BA87) {
                println!(
                    "{:08X}!{:08X}: group {:08X} {} choices {:?}",
                    key.group_id,
                    key.instance_id,
                    g.id,
                    g.name,
                    g.choices.iter().map(|c| c.animations.iter().map(|a| (a.id, a.duration, a.duration_scale)).collect::<Vec<_>>()).collect::<Vec<_>>()
                );
            }
        }
        let mut ui = crate::spui::view::SpuiAssets::default();
        assert!(ui.layout(index, host::LAYOUT).is_some(), "Tutorial_letterbox_txt");
        let freeze = name_id("TutorialFreezeFrame");
        println!("TutorialFreezeFrame installed: {}", swarm.find(freeze).is_some());
        for effect in ["TutorialFreezeFrame", "DeathFreezeFrame", "fade_from_black_2", "fade_to_black_quick"] {
            let Some(r) = swarm.find(name_id(effect)) else { continue };
            let dir = swarm.dir(r.dir);
            let def = &dir.effects[r.index as usize];
            for block in &def.blocks {
                print!("{effect}: block kind {} index {} flags {:X} time_scale {}", block.kind, block.index, block.flags, block.time_scale);
                if block.kind == crate::swarm_fx::format::kind::SCREEN {
                    if let Some(s) = dir.screens.get(block.index as usize) {
                        print!(
                            " screen mode {} flags {:X} life {} delay {} strength {:?} color {:?} filters {:?} floats {:?}",
                            s.mode,
                            s.flags,
                            s.life,
                            s.delay,
                            s.strength,
                            s.color,
                            s.filters.iter().map(|f| f.kind).collect::<Vec<_>>(),
                            s.params_float
                        );
                    }
                }
                println!();
            }
        }
        let chains = [
            ("CLG2CRG", vec!["CLG2CRG", "CLG2CRG_A", "CLG2CRG_B", "CLG2CRG_E", "CLG2CRG_E2", "creaturegoals", "creaturegoals1", "creaturegoals2", "setupCreature"]),
            ("CRG_BrainUpgrade1", vec!["CRG_BrainUpgrade1", "CRG_BrainUpgrade1a", "CRG_BrainUpgrade1b", "CRG_BrainUpgrade1c", "CRG_BrainUpgrade1d", "CRG_BrainUpgrade1e", "CRG_BrainUpgrade1f", "CRG_BrainUpgrade1g", "CRG_BrainUpgrade1h", "CRG_BrainUpgrade1i"]),
            ("TRG_Begin", vec!["TRG_Begin", "TRG_Begin2b", "TRG_Begin2", "TRG_Begin3", "TRG_FreezeFrame1", "TRG_FreezeFrame2", "TRG_FreezeFrame3", "TRG_Begin_ESC"]),
        ];
        let mut missing_text = Vec::new();
        for (name, states) in chains {
            for state in states {
                let s = program.state(name_id(state)).unwrap_or_else(|| panic!("{state}"));
                for command in &s.commands {
                    match command {
                        Command::StartEffect(e) => {
                            println!("{name}/{state}: effect {} installed {}", e.effect_name, swarm.find(e.effect).is_some());
                        }
                        Command::StartSound { name: sound, .. } => {
                            println!("{name}/{state}: sound {sound} installed {}", audio.resolve(sound).is_ok());
                        }
                        Command::PlayAnimation(p) => {
                            for id in &p.ids {
                                let secs = host::animation_seconds(&originals, *id);
                                println!("{name}/{state}: animation {id:08X} {secs:?} s (blocking {})", p.blocking);
                            }
                        }
                        Command::Text { table, id, .. } => {
                            let text = ui.localized(index, *table, *id);
                            println!("{name}/{state}: text {id:08X} = {text:?}");
                            if text.is_none() {
                                missing_text.push(format!("{state} {id:08X}"));
                            }
                        }
                        Command::PauseGame { screen_effect: Some(e), .. } => {
                            println!("{name}/{state}: screen effect {e:08X} installed {}", swarm.find(*e).is_some());
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(missing_text.is_empty(), "texts: {missing_text:?}");
    }

    #[test]
    fn the_tribal_evolve_waits_for_its_cinematics_then_changes_the_stage() {
        use crate::stages::GameStage;
        let mut app = app(
            "state TRG2CVG_PreEditor\n letterbox -on\n wait -secs .3\nend\nstate TRG2CVG_ShowCityHall\n wait -secs .3\nend\n",
        );
        app.world_mut().resource_mut::<Cinematics>().settings.hooks = true;
        // The stage code asks for a change from an Update system.
        #[derive(Resource, Default)]
        struct Want(Option<GameStage>);
        app.init_resource::<Want>().add_systems(Update, |mut want: ResMut<Want>, mut next: ResMut<NextState<GameStage>>| {
            if let Some(stage) = want.0.take() {
                next.set(stage);
            }
        });
        app.world_mut().resource_mut::<Want>().0 = Some(GameStage::Tribal);
        for _ in 0..4 {
            app.update();
        }
        let stage = |app: &App| app.world().resource::<State<GameStage>>().get().clone();
        assert_eq!(stage(&app), GameStage::Tribal);
        // The Evolve button asks for Civ: the change is held.
        app.world_mut().resource_mut::<Want>().0 = Some(GameStage::Civilization);
        app.update();
        app.update();
        assert_eq!(stage(&app), GameStage::Tribal, "held");
        let mut seen = Vec::new();
        for _ in 0..300 {
            app.update();
            let c = app.world().resource::<Cinematics>();
            if let Some((_, name, _)) = c.current() {
                if seen.last().map(String::as_str) != Some(name) {
                    seen.push(name.to_string());
                }
            }
            if stage(&app) == GameStage::Civilization {
                break;
            }
        }
        assert_eq!(seen, ["TRG2CVG_PreEditor", "TRG2CVG_ShowCityHall"]);
        assert_eq!(stage(&app), GameStage::Civilization);
        // Other changes pass at once.
        app.world_mut().resource_mut::<Want>().0 = Some(GameStage::Creature);
        app.update();
        app.update();
        assert_eq!(stage(&app), GameStage::Creature);
    }

    #[test]
    fn brain_level_up_plays_the_upgrade_cinematic_through_the_hook() {
        let mut app = app(
            "state CRG_BrainUpgrade1\n letterbox -on\n wait -secs 100\nend\nstate CLG2CRG\n wait -secs 100\nend\n",
        );
        app.world_mut().resource_mut::<Cinematics>().settings.hooks = true;
        app.insert_resource(crate::game::RunProgress::default());
        app.world_mut().spawn((crate::game::Player, Transform::default()));
        app.world_mut()
            .resource_mut::<NextState<crate::stages::GameStage>>()
            .set(crate::stages::GameStage::Creature);
        for _ in 0..5 {
            app.update();
        }
        // No nest: the stage-start cinematic waits; nothing plays yet.
        assert!(!app.world().resource::<Cinematics>().is_playing());
        app.world_mut().resource_mut::<crate::game::RunProgress>().creature_progression.brain_level = 1;
        app.update();
        app.update();
        let c = app.world().resource::<Cinematics>();
        assert_eq!(c.current().map(|(_, name, _)| name.to_string()), Some("CRG_BrainUpgrade1".to_string()));
        assert!(!simulation_active_now_ai(c), "no letAIRun: the simulation waits");
    }

    fn simulation_active_now_ai(c: &Cinematics) -> bool {
        !c.is_playing() || (c.pause_mode() == PauseMode::None && c.ai_mode() == AiMode::All)
    }
}
