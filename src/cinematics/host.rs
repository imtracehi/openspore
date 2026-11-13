//! The Bevy side of the cinematic player: executes the runtime's commands in
//! the game world (UI, camera, effects, sounds, actors), reports blockers,
//! locks input and hides the HUD while a cinematic plays.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{mpsc, Mutex};

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::ecs::world::Mut;
use bevy::prelude::*;

use super::actors::{self, ClipJob, ClipTrack};
use super::camera::{spore_to_bevy, yaw_of, CameraJob, CameraPose, CameraSpec, CameraState, TargetFrame};
use super::command::{
    clear_flags, ActorType, AddActor, AddObject, Command, EffectSpec, LetterboxStyle, MoveActor, PlayAnimation,
    PositionType, RelPos, SoundMode, StartVignette,
};
use super::runtime::{ContinueUi, Op, PauseMode, Program, SoundStop};
use super::script::Library;
use super::{
    AudioParamSet, CinematicEnded, CinematicMessage, CinematicMix, CinematicStarted, CinematicTarget, CinematicToken,
    Cinematics, PlayCinematic,
};
use crate::game::{Npc, Player};
use crate::spui::view::{SpuiAssets, SpuiState};
use crate::stages::{GameStage, StageOwned};
use crate::swarm_fx::{SwarmEffect, SwarmLibrary};

/// `Tutorial_letterbox_txt` (layouts 0x008A27D4), loaded by 0xAD8280.
pub const LAYOUT: &str = "Tutorial_letterbox_txt";
/// Window control ids of the layout (UI fields of 0xAD8280).
pub mod win {
    pub const TEXT: u32 = 0x0445_C9AB;
    pub const TEXT_UPPER: u32 = 0x0798_0D2C;
    pub const SCROLLING: u32 = 0x067A_0FC6;
    pub const BUTTON: u32 = 0x0445_C99D;
    pub const CLOSE: u32 = 0xFFFF_FFF2;
    pub const NEXT: u32 = 0x062E_F353;
    pub const CONTAINER: u32 = 0x05B5_A2C8;
    pub const LETTERBOX_TOP: u32 = 0x0450_12CA;
    pub const LETTERBOX_BOTTOM: u32 = 0x0450_23D0;
    pub const SPEECH: u32 = 0x0756_3379;
    pub const THOUGHT: u32 = 0x0756_3386;
    pub const CAPTION: u32 = 0x078D_4130;
    pub const RADIO: u32 = 0x07C7_6404;
    pub const BLACK: u32 = 0x05DB_9C49;
}

/// Swarm app flag 6 (`kAppFlagCinematics`, set by PlayCinematic).
const APP_FLAG_CINEMATICS: u32 = 1 << 6;
/// Mirror of terrain.rs `SEA_Y`: the sea surface of OpenSpore's island.
const WATER_Y: f32 = -0.9;
/// Mirror of terrain.rs `BASE_Y` (the flat-world baseline of grounding).
const BASE_Y: f32 = -0.6;
/// ADAPTED: walking speed of actors that are not the avatar, and turn rate.
const ACTOR_SPEED: f32 = 3.0;
const TURN_RATE: f32 = 2.0 * PI;
/// ADAPTED: animations whose length cannot be read block this long.
const UNKNOWN_ANIMATION_SECS: f32 = 2.0;
/// ADAPTED: a `-forContinue` wait whose game event OpenSpore does not send
/// (and no clip event is coming for) continues after this long.
const CONTINUE_FALLBACK_SECS: f32 = 3.0;
/// ADAPTED: how much of a clip's upward root motion a rigid body shows.
const POSE_LIFT: f32 = 0.3;
/// ADAPTED (only with the Swarm filters off): desaturation of the camera
/// under `TutorialFreezeFrame` / `DeathFreezeFrame`.
const FREEZE_DESATURATION: f32 = 0.7;
const FREEZE_DESATURATION_DEATH: f32 = 1.0;

/// The UI root of the cinematic layout.
#[derive(Component)]
pub struct CinematicUiRoot;

/// An actor spawned by `addActor` (removed when the cinematic ends).
#[derive(Component)]
pub struct CinematicActor;

/// A sound started by `startSound`.
#[derive(Component)]
pub struct CinematicSound {
    key: u32,
    mode: SoundMode,
}

#[derive(Clone, Copy, Debug)]
struct ActorSlot {
    entity: Entity,
    spawned: bool,
}

struct NamedEffect {
    name: u32,
    /// Installed effect id.
    effect: u32,
    entity: Entity,
    blocker: Option<u32>,
    persist: bool,
}

struct MoveJob {
    entity: Entity,
    /// `None`: turn only.
    goal: Option<Vec3>,
    stop_distance: f32,
    face: Option<Vec3>,
    speed: f32,
    blocker: Option<u32>,
    done: bool,
}

struct SoundJob {
    id: u32,
    name: &'static str,
}

struct SoundDecoded {
    id: u32,
    result: Result<(f32, f32, Vec<(Vec<u8>, f32)>), String>,
}

struct PendingSound {
    id: u32,
    name: String,
    key: u32,
    mode: SoundMode,
    bus: crate::game_audio::Bus,
    asked: f32,
}

#[derive(Clone)]
struct LoadedSound {
    gain: f32,
    pitch: f32,
    clips: Vec<Handle<AudioSource>>,
}

/// Host state of the playing cinematic.
#[derive(Resource, Default)]
pub struct Scene {
    token: u32,
    playing: bool,
    ui_root: Option<Entity>,
    letterbox: bool,
    style: LetterboxStyle,
    text: Option<String>,
    text_upper: Option<String>,
    scrolling: Option<String>,
    /// `scrollingText`: the scene clock at its start and its duration.
    scroll_from: f32,
    scroll_duration: f32,
    black: bool,
    continue_ui: Option<ContinueUi>,
    continue_shown_for: f32,
    camera: Option<CameraJob>,
    camera_blocker: Option<u32>,
    hold: Option<CameraPose>,
    /// The pose the cinematic camera showed last frame (the stage cameras
    /// overwrite the camera transform in Update, so jobs start from this).
    last_pose: Option<CameraPose>,
    start_camera: Option<CameraState>,
    saved_projection: Option<(f32, f32, f32)>,
    actors: HashMap<u32, ActorSlot>,
    objects: HashMap<u32, (Vec3, f32)>,
    effects: Vec<NamedEffect>,
    /// `-persist` effects that outlived their cinematic (`black_screen` of
    /// the death restart); the next screen effect on their layer ends them.
    persisted: Vec<NamedEffect>,
    moves: Vec<MoveJob>,
    /// Clips the actors play (`playAnimation`).
    clips: Vec<ClipJob>,
    /// The root offset each playing clip gives its actor this frame (Bevy
    /// axes, metres of a nominal creature), applied on top of the lock.
    poses: HashMap<Entity, (Vec3, Quat)>,
    /// Actors under cinematic control: the x/z position and yaw re-applied
    /// every frame (the stage's own controls and bounds clamps run in
    /// Update; the cinematic wins in PostUpdate).
    locks: HashMap<Entity, (Vec3, f32)>,
    /// Height of each locked actor above the surface, kept when the lock was
    /// made: the lock puts the actor at surface + clearance every frame, so a
    /// clip's root offset on top of it never accumulates.
    clearance: HashMap<Entity, f32>,
    screen_effect: Option<Entity>,
    hidden_ui: Vec<Entity>,
    pause: PauseMode,
    /// Desaturation the freeze-frame look is heading to (0 = none) and the
    /// camera's current value.
    grade_target: f32,
    grade: f32,
    sound_jobs: Option<mpsc::Sender<SoundJob>>,
    sound_results: Option<Mutex<mpsc::Receiver<SoundDecoded>>>,
    sounds_loaded: HashMap<u32, Option<LoadedSound>>,
    pending_sounds: Vec<PendingSound>,
    clock: f32,
    for_continue: f32,
    /// The `wait -forContinue` state whose game behaviour clips were started
    /// (`start_continue_clips`).
    continue_clips: Option<String>,
    /// Seconds played (captures, diagnostics).
    pub elapsed: f32,
    /// Vignette roles' handheld items per clip (`vi_effects_n`, 0xBC58B0;
    /// `tribal_vignettes::handheld_effect` ids): (clip, item) in play order.
    vignette_items: HashMap<Entity, Vec<(u32, Option<u32>)>>,
}

impl Scene {
    /// The clip an actor plays now: (action id, seconds into it).
    pub fn actor_clip(&self, entity: Entity) -> Option<(u32, f32)> {
        self.clips.iter().rev().filter(|j| j.entity == entity).find_map(|j| j.current())
    }

    /// The handheld item effect a vignette role shows now (`Some(None)`: an
    /// empty hand; `None`: the member's own item): the item of the current
    /// clip's action, kept until a later action changes it.
    pub fn actor_item(&self, entity: Entity) -> Option<Option<u32>> {
        let list = self.vignette_items.get(&entity)?;
        let (clip, _) = self.actor_clip(entity)?;
        let at = list.iter().position(|(c, _)| *c == clip)?;
        list[..=at].iter().rev().find_map(|(_, item)| *item).map(crate::tribal_vignettes::handheld_effect)
    }

    pub fn continue_ui(&self) -> Option<ContinueUi> {
        self.continue_ui
    }

    pub fn letterbox(&self) -> bool {
        self.letterbox
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Loading and requests
// ---------------------------------------------------------------------------

/// Load the installed scripts once the resource index exists.
pub fn load_scripts(mut cinematics: ResMut<Cinematics>, originals: Option<Res<crate::Originals>>) {
    if cinematics.loaded() {
        return;
    }
    let Some(originals) = originals else { return };
    if originals.resource_index.records.is_empty() {
        return;
    }
    let library = Library::from_index(&originals.resource_index);
    let program = Program::compile(&library);
    cinematics.set_program(program);
    info!("cinematics: {}", cinematics.report);
}

/// [`PlayCinematic`] messages.
pub fn play_requests(mut cinematics: ResMut<Cinematics>, mut requests: MessageReader<PlayCinematic>) {
    for request in requests.read() {
        if let Some(token) = cinematics.play(&request.name, request.pause_game, request.queue) {
            for (name, target) in &request.bindings {
                cinematics.bind_object(token, name, *target);
            }
        }
    }
}

/// Input lock (`cGameInputManager.field_110++`): while a cinematic plays the
/// game sees no keyboard or mouse input. Escape skips (0xAD7D70), the Continue
/// button (or Enter / Space, ADAPTED shortcuts) sends 0x445F729.
pub fn input_gate(
    mut cinematics: ResMut<Cinematics>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: ResMut<Messages<bevy::input::mouse::MouseWheel>>,
    mut motion: ResMut<Messages<bevy::input::mouse::MouseMotion>>,
    ui: Query<&SpuiState, With<CinematicUiRoot>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    if !cinematics.is_playing() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cinematics.skip();
    }
    if cinematics.runtime.continue_shown() {
        let mut go = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
        if mouse.just_pressed(MouseButton::Left) {
            let cursor = windows.single().ok().and_then(|w| w.cursor_position());
            if let (Some(p), Ok(state)) = (cursor, ui.single()) {
                for control in [win::BUTTON, win::NEXT, win::CONTAINER] {
                    if let Some(id) = state.find(control) {
                        let r = state.rect(id);
                        if state.visible(id) && p.x >= r.x1 && p.x <= r.x2 && p.y >= r.y1 && p.y <= r.y2 {
                            go = true;
                        }
                    }
                }
            }
        }
        if go {
            cinematics.continue_cinematic();
        }
    }
    keys.reset_all();
    mouse.reset_all();
    wheel.clear();
    motion.clear();
}

// ---------------------------------------------------------------------------
// The per-frame driver
// ---------------------------------------------------------------------------

/// Cinematic manager Update (0xAE0050) plus everything the host does per
/// frame. PostUpdate, before transforms propagate, so the cinematic camera
/// wins over the stage cameras.
pub fn drive(world: &mut World) {
    if !world.contains_resource::<Cinematics>() || !world.contains_resource::<Scene>() {
        return;
    }
    let dt = world.get_resource::<Time<Real>>().map_or(0.0, |t| t.delta_secs()).min(0.25);
    world.resource_scope(|world, mut scene: Mut<Scene>| {
        world.resource_scope(|world, mut cin: Mut<Cinematics>| {
            scene.clock += dt;
            let actor_dt = if scene.pause == PauseMode::CinematicAll { 0.0 } else { dt };
            step_clips(world, &mut scene, &mut cin, actor_dt);
            poll_blockers(world, &mut scene, &mut cin, dt);
            // `wait -forContinue` ends on a game event: a clip's
            // `cinematic_continue` animation event (the retail sender for
            // `CRG2TRG`), the end of a clip the game behaviour plays (the
            // avatar's brain-up clip 0xD7C360, the chieftain's alliance
            // celebration 0xD9B4C0, the mate clip 0xD6CDB0), else ADAPTED
            // fallbacks for events OpenSpore does not send.
            let continue_coming = scene.clips.iter().any(|j| j.continue_ahead());
            if let Some(kind) = cin.runtime.waiting_for_continue() {
                match kind {
                    super::command::WaitKind::ForContinue => {
                        let state = cin.runtime.current_state().unwrap_or("").to_string();
                        if scene.continue_clips.as_deref() != Some(state.as_str()) {
                            let token = cin.runtime.token().unwrap_or(0);
                            start_continue_clips(world, &mut scene, &cin, token, &state);
                            scene.continue_clips = Some(state);
                        }
                        scene.for_continue += dt;
                        let fallback = cin.settings.continue_fallback.map(|s| continue_seconds(world, &cin, s));
                        if !continue_coming && fallback.is_some_and(|s| scene.for_continue >= s) {
                            cin.continue_cinematic();
                        }
                    }
                    _ => {
                        if cin.runtime.continue_shown() {
                            scene.continue_shown_for += dt;
                            if cin.settings.auto_continue.is_some_and(|s| scene.continue_shown_for >= s) {
                                cin.continue_cinematic();
                            }
                        }
                    }
                }
            } else {
                scene.for_continue = 0.0;
                scene.continue_shown_for = 0.0;
                scene.continue_clips = None;
            }
            cin.runtime.update(dt);
            let ops = cin.runtime.take_ops();
            for op in ops {
                apply_op(world, &mut scene, &mut cin, op);
            }
            if scene.playing {
                scene.elapsed += dt;
            }
            step_actors(world, &mut scene, actor_dt);
            step_camera(world, &mut scene, &cin, dt);
            apply_grade(world, &mut scene, dt);
            sync_ui(world, &mut scene);
            hide_hud(world, &mut scene);
            pump_sounds(world, &mut scene);
            if scene.playing {
                if let Some(mut flags) = world.get_resource_mut::<crate::swarm_fx::SwarmAppFlags>() {
                    flags.0 |= APP_FLAG_CINEMATICS;
                }
            }
        });
    });
    world.flush();
}

fn poll_blockers(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, dt: f32) {
    if let (Some(job), Some(blocker)) = (scene.camera.as_ref(), scene.camera_blocker) {
        if !job.transitioning() {
            cin.runtime.finish(blocker);
            scene.camera_blocker = None;
        }
    }
    for effect in &mut scene.effects {
        if let Some(blocker) = effect.blocker {
            let alive = world.get::<SwarmEffect>(effect.entity).is_some_and(|e| e.alive);
            if !alive {
                cin.runtime.finish(blocker);
                effect.blocker = None;
            }
        }
    }
    for job in &mut scene.moves {
        if job.done {
            if let Some(blocker) = job.blocker.take() {
                cin.runtime.finish(blocker);
            }
        }
    }
    scene.moves.retain(|j| !j.done);
    for job in &mut scene.clips {
        if job.done {
            if let Some(blocker) = job.blocker.take() {
                cin.runtime.finish(blocker);
            }
        }
    }
    scene.clips.retain(|j| !j.done);
}

fn apply_op(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, op: Op) {
    match op {
        Op::Started { token, name } => {
            scene.token = token;
            scene.playing = true;
            scene.elapsed = 0.0;
            scene.last_pose = None;
            scene.start_camera = current_camera(world, scene);
            info!("cinematic {name} started");
            world.write_message(CinematicStarted { token: CinematicToken(token), name });
        }
        Op::Run { token, command, blocker } => run_command(world, scene, cin, token, command, blocker),
        Op::Pause(mode) => {
            // The simulation set stops for every pause (`simulation_active`);
            // with `-pauseGraphics` the cinematic's own actors freeze too.
            // ADAPTED: `Time<Virtual>` keeps running, because the SPUI fades
            // of the text card and the freeze-frame screen effect run on it
            // (retail runs them outside the paused game clock).
            scene.pause = mode;
        }
        Op::ScreenEffect(effect) => {
            // The freeze-frame effects (`TutorialFreezeFrame` 0xDFE90CAD,
            // `DeathFreezeFrame` 0x537CE5DB) are Swarm filter chains on the
            // whole frame (mode 10 screens: colorize, blur1D passes, masked
            // add; `swarm_fx::filters`): the effect entity below draws them.
            // ADAPTED fallback only with the filters off (`OS_FILTERS=0`):
            // the camera's post saturation stands in for the desaturation.
            let chains = crate::swarm_fx::filters::filters_enabled();
            scene.grade_target = match effect {
                _ if chains => 0.0,
                Some(id) if id == crate::world_objects::name_id("DeathFreezeFrame") => FREEZE_DESATURATION_DEATH,
                Some(_) => FREEZE_DESATURATION,
                None => 0.0,
            };
            if let Some(old) = scene.screen_effect.take() {
                if let Some(mut e) = world.get_mut::<SwarmEffect>(old) {
                    e.stop = true;
                    e.hard_stop = true;
                }
            }
            if let Some(id) = effect {
                let entity = world.spawn((SwarmEffect::new(id), StageOwned, Name::new("cinematic screen effect"))).id();
                scene.screen_effect = Some(entity);
            }
        }
        Op::ShowContinue(ui) => {
            scene.continue_ui = Some(ui);
            scene.continue_shown_for = 0.0;
        }
        Op::HideContinue => scene.continue_ui = None,
        Op::StopSounds(kind) => {
            let mut stop = Vec::new();
            let mut q = world.query::<(Entity, &CinematicSound)>();
            for (e, s) in q.iter(world) {
                let hit = match kind {
                    SoundStop::Escape => s.mode == SoundMode::EscStop,
                    SoundStop::End => s.mode != SoundMode::NoStop,
                };
                if hit {
                    stop.push(e);
                }
            }
            for e in stop {
                world.despawn(e);
            }
            scene.pending_sounds.retain(|p| match kind {
                SoundStop::Escape => p.mode != SoundMode::EscStop,
                SoundStop::End => p.mode == SoundMode::NoStop,
            });
        }
        Op::ReleaseCamera => {
            scene.camera = None;
            scene.camera_blocker = None;
            scene.hold = None;
            restore_projection(world, scene);
        }
        Op::Ended { token, name, skipped, preserve_view } => {
            end_cleanup(world, scene, cin, token, preserve_view);
            info!("cinematic {name} ended{}", if skipped { " (skipped)" } else { "" });
            world.write_message(CinematicEnded { token: CinematicToken(token), name, skipped });
        }
    }
}

fn end_cleanup(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, preserve_view: bool) {
    // Actors created for the cinematic go away (ADAPTED: retail keeps the
    // simulator creatures; OpenSpore's copies have no AI of their own).
    for slot in scene.actors.values() {
        if slot.spawned && world.get_entity(slot.entity).is_ok() {
            world.despawn(slot.entity);
        }
    }
    scene.actors.clear();
    scene.objects.clear();
    for effect in std::mem::take(&mut scene.effects) {
        if effect.persist {
            scene.persisted.push(effect);
            continue;
        }
        if let Some(mut e) = world.get_mut::<SwarmEffect>(effect.entity) {
            e.stop = true;
        }
    }
    // Bodies a clip leaned or lifted go back to their lock's pose.
    let locked: Vec<(Entity, Vec3, f32)> = scene.locks.iter().map(|(e, (p, y))| (*e, *p, *y)).collect();
    for (entity, pos, yaw) in locked {
        if scene.poses.contains_key(&entity) && world.get_entity(entity).is_ok() {
            apply_lock(world, scene, entity, pos, yaw, None);
        }
    }
    scene.moves.clear();
    scene.clips.clear();
    scene.vignette_items.clear();
    scene.poses.clear();
    scene.locks.clear();
    scene.clearance.clear();
    scene.letterbox = false;
    scene.text = None;
    scene.text_upper = None;
    scene.scrolling = None;
    scene.black = false;
    scene.continue_ui = None;
    scene.camera = None;
    scene.camera_blocker = None;
    scene.hold = None;
    scene.last_pose = None;
    if !preserve_view {
        restore_projection(world, scene);
    } else {
        scene.saved_projection = None;
    }
    scene.playing = false;
    scene.pause = PauseMode::None;
    scene.grade_target = 0.0;
    cin.forget_bindings(token);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn run_command(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, command: Command, blocker: Option<u32>) {
    match command {
        Command::Letterbox { on, style } => {
            scene.letterbox = on;
            scene.style = style;
        }
        Command::StartCamera(spec) => start_camera(world, scene, cin, token, *spec, blocker),
        Command::StopCamera => {
            // The camera stays where the job left it until the cinematic ends.
            if let Some(mut job) = scene.camera.take() {
                let frame = camera_frame(world, scene, cin, token, &job.spec);
                let ground = ground_fn(world);
                scene.hold = Some(job.pose(&frame, ground));
            }
            scene.camera_blocker = None;
        }
        Command::Text { upper, table, id, dev } => {
            let text = localized(world, table, id).unwrap_or(dev);
            if upper {
                scene.text_upper = Some(text);
            } else {
                scene.text = Some(text);
            }
        }
        Command::TextHide { upper } => {
            if upper {
                scene.text_upper = None;
            } else {
                scene.text = None;
            }
        }
        Command::TextFromCode { key } => scene.text = Some(cin.text_from_code(&key)),
        Command::ScrollingText { table, id, duration } => {
            // 0xAE2C45 (message 0x67A195B): 0xAD7640 resets the scroller of
            // window 0x067A0FC6 (an earlier scrolling text ends), 0xAD75F0
            // starts this one for `duration` s (fade 0.25); see
            // `scroll_window`.
            scene.scrolling = localized(world, table, id);
            scene.scroll_from = scene.clock;
            scene.scroll_duration = duration;
        }
        Command::StartEffect(spec) => start_effect(world, scene, cin, token, *spec, blocker),
        Command::StopEffect { name, soft } => {
            for effect in scene.effects.iter_mut().filter(|e| e.name == name) {
                if let Some(mut e) = world.get_mut::<SwarmEffect>(effect.entity) {
                    e.stop = true;
                    e.hard_stop = !soft;
                }
                effect.persist = false;
            }
            scene.effects.retain(|e| e.name != name || e.blocker.is_some());
        }
        Command::SendMessage { id, data } => {
            world.write_message(CinematicMessage { token: CinematicToken(token), id, data });
        }
        Command::StartSound { key, sound, name, mode } => {
            let bus = if name.starts_with("music") || name.starts_with("mus_") {
                crate::game_audio::Bus::Music
            } else {
                crate::game_audio::Bus::Sfx
            };
            request_sound(world, scene, sound, &name);
            scene.pending_sounds.push(PendingSound { id: sound, name, key, mode, bus, asked: scene.clock });
        }
        Command::StopSound { key } => {
            let mut stop = Vec::new();
            let mut q = world.query::<(Entity, &CinematicSound)>();
            for (e, s) in q.iter(world) {
                if s.key == key {
                    stop.push(e);
                }
            }
            for e in stop {
                world.despawn(e);
            }
            scene.pending_sounds.retain(|p| p.key != key);
        }
        Command::MixEvent { hash, value, .. } => {
            world.resource_mut::<CinematicMix>().0.insert(hash, value);
            world.write_message(AudioParamSet { hash, value });
        }
        Command::BlackScreen(on) => scene.black = on,
        Command::ClearAreaAroundObject { object, radius, flags } => {
            clear_area(world, scene, cin, token, object, radius, flags);
        }
        Command::AddActor(actor) => add_actor(world, scene, cin, token, *actor),
        Command::AddObject(object) => add_object(world, scene, cin, token, *object),
        Command::MoveActor(m) => move_actor(world, scene, cin, token, *m, blocker),
        Command::PlayAnimation(p) => play_animation(world, scene, cin, p, blocker),
        Command::StartVignette(v) => start_vignette(world, scene, cin, token, *v, blocker),
        // ADAPTED: no obstacle fading, timeline UI, main-HUD toggle (the HUD is
        // hidden for the whole cinematic), physics/plumping toggles or
        // snapshots in this build.
        Command::AlphaObstacles { .. }
        | Command::ShowTimeline { .. }
        | Command::ShowUi(_)
        | Command::PhysicsCollisions
        | Command::TogglePlumping
        | Command::TakeSnapshot => {
            if let Some(b) = blocker {
                cin.runtime.finish(b);
            }
        }
        _ => {
            if let Some(b) = blocker {
                cin.runtime.finish(b);
            }
        }
    }
}

fn localized(world: &mut World, table: u32, id: u32) -> Option<String> {
    if !world.contains_resource::<SpuiAssets>() || !world.contains_resource::<crate::Originals>() {
        return None;
    }
    world
        .resource_scope(|world, mut assets: Mut<SpuiAssets>| {
            let originals = world.resource::<crate::Originals>();
            if originals.resource_index.records.is_empty() {
                return None;
            }
            assets.localized(&originals.resource_index, table, id)
        })
        .map(|t| fill_tokens(&t, &[]))
}

/// A locale string of an installed table by the table's name, with its
/// `~name~` tokens filled from `values`.
pub fn localized_by_name(world: &mut World, table: &str, id: u32, values: &[(&str, String)]) -> Option<String> {
    let raw = world.contains_resource::<SpuiAssets>().then(|| {
        world.resource_scope(|world, mut assets: Mut<SpuiAssets>| {
            let originals = world.get_resource::<crate::Originals>()?;
            if originals.resource_index.records.is_empty() {
                return None;
            }
            assets.localized(&originals.resource_index, crate::world_objects::name_id(table), id)
        })
    });
    raw.flatten().map(|t| fill_tokens(&t, values))
}

/// Fill the `~name~` tokens of a locale string: the given values, then
/// generic words for the ones the game code would name (`~br~` is a line
/// break).
pub fn fill_tokens(text: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('~') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('~') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
            let value = values.iter().find(|(k, _)| *k == token).map(|(_, v)| v.as_str()).unwrap_or(match token {
                "br" => "\n",
                // ADAPTED: names the retail code fills in from the game.
                "ufo_name" => "UFO",
                "tribe_name" => "tribe",
                "planet_name" => "planet",
                "race" => "alien race",
                "tribe_creature_species" => "creatures",
                "max_population" => "more",
                _ => "",
            });
            out.push_str(value);
            rest = &after[end + 1..];
        } else {
            out.push('~');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Names, positions and frames
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Frame {
    position: Vec3,
    yaw: f32,
    entity: Option<Entity>,
}

fn entity_frame(world: &World, entity: Entity) -> Option<Frame> {
    let t = world.get::<Transform>(entity)?;
    Some(Frame { position: t.translation, yaw: yaw_of(t.rotation), entity: Some(entity) })
}

/// A script name: actors and objects of the cinematic, then the stage's
/// bindings, then the named effects.
fn resolve_name(world: &World, scene: &Scene, cin: &Cinematics, token: u32, name: u32) -> Option<Frame> {
    if let Some(slot) = scene.actors.get(&name) {
        if let Some(f) = entity_frame(world, slot.entity) {
            return Some(f);
        }
    }
    if let Some(target) = cin.binding(token, name) {
        return match target {
            CinematicTarget::Entity(e) => entity_frame(world, e),
            CinematicTarget::Point { position, yaw } => Some(Frame { position, yaw, entity: None }),
        };
    }
    if let Some(&(position, yaw)) = scene.objects.get(&name) {
        return Some(Frame { position, yaw, entity: None });
    }
    if let Some(effect) = scene.effects.iter().find(|e| e.name == name) {
        if let Some(e) = world.get::<SwarmEffect>(effect.entity) {
            let position = match e.attach_to.and_then(|a| world.get::<Transform>(a)) {
                Some(t) => t.translation + t.rotation * e.position,
                None => e.position,
            };
            return Some(Frame { position, yaw: 0.0, entity: None });
        }
    }
    None
}

fn surface_height(world: &World, x: f32, z: f32) -> Option<f32> {
    let surface = world.get_resource::<crate::terrain::LandSurface>()?;
    let stage = world.get_resource::<State<GameStage>>()?;
    (*stage.get() == surface.stage).then(|| surface.height(x, z))
}

/// Terrain / water height for the camera's eye clamp.
fn ground_fn(world: &World) -> impl Fn(f32, f32) -> f32 + '_ {
    let surface = world.get_resource::<crate::terrain::LandSurface>();
    let active = world
        .get_resource::<State<GameStage>>()
        .zip(surface)
        .is_some_and(|(s, f)| *s.get() == f.stage);
    move |x, z| match (surface, active) {
        (Some(s), true) => s.height(x, z).max(WATER_Y),
        _ => -1.0e6,
    }
}

/// The nearest water (or land) point to `from`, facing inland (from the
/// point toward `from`). ADAPTED: radial search of OpenSpore's island surface.
fn nearest(world: &World, from: Vec3, water: bool) -> Option<(Vec3, f32)> {
    let surface = world.get_resource::<crate::terrain::LandSurface>()?;
    let is_target = |x: f32, z: f32| {
        let h = surface.height(x, z);
        if water {
            h < WATER_Y
        } else {
            h >= WATER_Y + 0.05
        }
    };
    if is_target(from.x, from.z) {
        return Some((from, 0.0));
    }
    let mut best: Option<(f32, Vec3)> = None;
    for i in 0..72 {
        let a = i as f32 / 72.0 * std::f32::consts::TAU;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let mut r = 0.5;
        while r < 120.0 {
            let p = from + dir * r;
            if is_target(p.x, p.z) {
                if best.is_none_or(|(d, _)| r < d) {
                    best = Some((r, p));
                }
                break;
            }
            r += 0.5;
        }
    }
    let (_, p) = best?;
    let inland = if water { from - p } else { p - from };
    let y = surface.height(p.x, p.z).max(WATER_Y);
    Some((Vec3::new(p.x, y, p.z), crate::creature::facing_yaw(inland)))
}

/// The stage's nest / hut for the `actorNest`, `actorHut`, `actorTribe`
/// position types. ADAPTED: OpenSpore has one player nest and one home hut.
fn home_frame(world: &mut World, kind: PositionType) -> Option<Frame> {
    match kind {
        PositionType::ActorNest => {
            let mut q = world.query_filtered::<Entity, With<crate::creature_home::HomeNest>>();
            let e = q.iter(world).next()?;
            entity_frame(world, e)
        }
        PositionType::ActorHut | PositionType::ActorTribe => {
            let mut q = world.query::<(Entity, &crate::tribal_rts::HomeProp)>();
            let hut = q
                .iter(world)
                .find(|(_, p)| p.kind == crate::tribal_rts::HomePropKind::Hut)
                .map(|(e, _)| e);
            match hut {
                Some(e) => entity_frame(world, e),
                None => Some(Frame { position: crate::tribal::home_position(), yaw: 0.0, entity: None }),
            }
        }
        _ => None,
    }
}

/// A `<posType> "<name>" (offset)` position. `self_frame` is used for the
/// default type and a missing name.
fn resolve_relpos(
    world: &mut World,
    scene: &Scene,
    cin: &Cinematics,
    token: u32,
    rel: &RelPos,
    self_frame: Option<Frame>,
) -> Option<Frame> {
    let named = if rel.name == 0 { self_frame } else { resolve_name(world, scene, cin, token, rel.name) };
    let base = match rel.kind {
        PositionType::NearestWater | PositionType::NearestLand => {
            let from = named.or(self_frame)?;
            let (p, yaw) = nearest(world, from.position, rel.kind == PositionType::NearestWater)?;
            Some(Frame { position: p, yaw, entity: None })
        }
        PositionType::ActorNest | PositionType::ActorHut | PositionType::ActorTribe => {
            home_frame(world, rel.kind).or(named)
        }
        PositionType::Default => named.or(self_frame),
        _ => named,
    }?;
    let offset = Quat::from_rotation_y(base.yaw) * stage_offset(world, spore_to_bevy(rel.offset));
    Some(Frame { position: base.position + offset, yaw: base.yaw, entity: base.entity })
}

/// A script length (metres of the retail world) in the stage's world.
/// ADAPTED: the Civ island is `retail x CIV_WORLD_SCALE` (`civ_rules`, the
/// distances between cities and the planet), but its buildings and vehicles
/// keep their model size, so the camera distances that frame an object shrink
/// by the square root of the factor and the ones that frame the island by the
/// factor itself (blended between 20 and 300 m). The other stages are 1:1;
/// with `CIV_WORLD_SCALE` 1 (the planet engine) so is Civ.
fn stage_length(world: &World, meters: f32) -> f32 {
    match world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) {
        Some(GameStage::Civilization) => civ_length(meters),
        // The galaxy screen's cinematics are in galaxy units.
        Some(GameStage::MainMenu) => meters * crate::menu_visuals::GALAXY_SCALE,
        // `SPG_CenterOfGalaxy` frames the core at its drawn size
        // (`space_endgame::SpaceCinematicScale`, absolute lengths only);
        // other Space scripts are 1:1.
        Some(GameStage::Space) => meters * world.get_resource::<crate::space_endgame::SpaceCinematicScale>().map_or(1.0, |s| s.0),
        _ => meters,
    }
}

fn civ_length(meters: f32) -> f32 {
    let far = crate::civ_rules::CIV_WORLD_SCALE;
    let near = far.sqrt();
    let t = ((meters.abs().max(1.0).ln() - 20f32.ln()) / (300f32.ln() - 20f32.ln())).clamp(0.0, 1.0);
    meters * (near + (far - near) * t)
}

/// A Spore-axes offset in the stage's world.
fn stage_offset(world: &World, v: Vec3) -> Vec3 {
    Vec3::new(stage_length(world, v.x), stage_length(world, v.y), stage_length(world, v.z))
}

/// `max(radius, 1)`-style multipliers of an entity. ADAPTED: OpenSpore bodies
/// are about one unit across at scale 1 (no bounding data is kept).
fn entity_scale(world: &World, entity: Option<Entity>) -> (f32, f32) {
    let s = entity.and_then(|e| world.get::<Transform>(e)).map_or(1.0, |t| t.scale.max_element());
    (s.max(1.0), s)
}

fn camera_frame(world: &mut World, scene: &Scene, cin: &Cinematics, token: u32, spec: &CameraSpec) -> TargetFrame {
    let frame = spec.target.and_then(|t| resolve_name(world, scene, cin, token, t));
    let Some(frame) = frame else {
        if let Some(p) = spec.target_pos {
            return TargetFrame::at(spore_to_bevy(p));
        }
        let look = scene.camera.as_ref().map(|j| j.look()).or(scene.hold.map(|h| h.look));
        return TargetFrame::at(look.unwrap_or(Vec3::ZERO));
    };
    let scale_entity = spec
        .radius_target
        .and_then(|r| resolve_name(world, scene, cin, token, r))
        .and_then(|f| f.entity)
        .or(frame.entity);
    let (scale, raw) = entity_scale(world, scale_entity);
    let ground = surface_height(world, frame.position.x, frame.position.z)
        .map_or(frame.position.y, |h| h.max(WATER_Y));
    TargetFrame {
        position: frame.position,
        yaw: frame.yaw,
        scale: if spec.use_radius || spec.use_footprint || spec.use_height { scale } else { 1.0 },
        // ADAPTED: the head sits about half a body above the origin.
        head: 0.5 * raw,
        ground,
    }
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

fn game_camera(world: &mut World) -> Option<Entity> {
    let mut q = world.query_filtered::<Entity, With<crate::GameCamera>>();
    q.iter(world).next()
}

fn current_camera(world: &mut World, scene: &Scene) -> Option<CameraState> {
    if let (true, Some(pose)) = (scene.playing, scene.last_pose) {
        return Some(CameraState { eye: pose.eye, look: pose.look, fov: pose.fov });
    }
    let camera = game_camera(world)?;
    let t = *world.get::<Transform>(camera)?;
    let fov = match world.get::<Projection>(camera) {
        Some(Projection::Perspective(p)) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let look = match (&scene.camera, scene.hold) {
        (Some(job), _) => job.look(),
        (None, Some(h)) => h.look,
        _ => {
            let mut q = world.query_filtered::<&Transform, With<Player>>();
            let d = q.iter(world).next().map_or(10.0, |p| p.translation.distance(t.translation).max(1.0));
            t.translation + t.forward().as_vec3() * d
        }
    };
    Some(CameraState { eye: t.translation, look, fov })
}

fn start_camera(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, spec: CameraSpec, blocker: Option<u32>) {
    // Space: a spec framing an actor (the UFO) keeps its retail lengths; only
    // the absolute ones follow `space_endgame::SpaceCinematicScale`.
    let space_actor = spec.target.is_some()
        && world.get_resource::<State<GameStage>>().is_some_and(|s| *s.get() == GameStage::Space);
    let spec = if space_actor { spec } else { spec.scaled(|d| stage_length(world, d)) };
    let Some(now) = current_camera(world, scene) else {
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    };
    if scene.saved_projection.is_none() {
        if let Some(camera) = game_camera(world) {
            if let Some(Projection::Perspective(p)) = world.get::<Projection>(camera) {
                scene.saved_projection = Some((p.fov, p.near, p.far));
            }
        }
    }
    let frame = camera_frame(world, scene, cin, token, &spec);
    let job = CameraJob::new(spec, now, frame, scene.start_camera);
    scene.camera = Some(job);
    scene.camera_blocker = blocker;
    scene.hold = None;
}

fn step_camera(world: &mut World, scene: &mut Scene, cin: &Cinematics, dt: f32) {
    if !scene.playing {
        return;
    }
    let pose = if let Some(mut job) = scene.camera.take() {
        job.step(dt);
        let frame = camera_frame(world, scene, cin, scene.token, &job.spec);
        let ground = ground_fn(world);
        let pose = job.pose(&frame, ground);
        let (near, far) = (job.spec.near_clip, job.spec.far_clip);
        scene.camera = Some(job);
        Some((pose, near, far))
    } else {
        scene.hold.map(|p| (p, None, None))
    };
    let Some((pose, near, far)) = pose else { return };
    scene.last_pose = Some(pose);
    let Some(camera) = game_camera(world) else { return };
    if let Some(mut t) = world.get_mut::<Transform>(camera) {
        *t = pose.transform();
    }
    if let Some(mut projection) = world.get_mut::<Projection>(camera) {
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = pose.fov.clamp(0.05, 3.0);
            if let Some(n) = near {
                p.near = n;
            }
            if let Some(f) = far {
                p.far = f;
            }
        }
    }
}

/// The freeze-frame desaturation on the game camera (see [`FREEZE_DESATURATION`]).
fn apply_grade(world: &mut World, scene: &mut Scene, dt: f32) {
    use bevy::render::view::{ColorGrading, ColorGradingGlobal};
    if (scene.grade - scene.grade_target).abs() < 1e-3 {
        return;
    }
    let step = 3.0 * dt;
    scene.grade += (scene.grade_target - scene.grade).clamp(-step, step);
    let Some(camera) = game_camera(world) else { return };
    let grading = ColorGrading {
        global: ColorGradingGlobal { post_saturation: 1.0 - scene.grade, ..default() },
        ..default()
    };
    world.entity_mut(camera).insert(grading);
}

fn restore_projection(world: &mut World, scene: &mut Scene) {
    let Some((fov, near, far)) = scene.saved_projection.take() else { return };
    let Some(camera) = game_camera(world) else { return };
    if let Some(mut projection) = world.get_mut::<Projection>(camera) {
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = fov;
            p.near = near;
            p.far = far;
        }
    }
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

fn start_effect(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, spec: EffectSpec, blocker: Option<u32>) {
    let installed = world.get_resource::<SwarmLibrary>().is_some_and(|l| !l.loaded || l.find(spec.effect).is_some());
    if !installed {
        warn!("cinematic effect {} is not installed", spec.effect_name);
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    }
    let target = spec.target.and_then(|t| resolve_name(world, scene, cin, token, t));
    let mut effect = SwarmEffect::new(spec.effect);
    effect.tag = 0xC1E0_0000_0000_0000 | spec.name as u64;
    if let Some(frame) = target {
        let (_, raw) = entity_scale(world, frame.entity);
        let mut local = stage_offset(world, spore_to_bevy(spec.offset));
        if spec.use_radius {
            local *= raw.max(1.0);
        }
        if spec.use_height {
            local.y *= raw.max(1.0);
        }
        let extra = stage_offset(world, spore_to_bevy(spec.extra_offset));
        match (spec.attached, frame.entity) {
            (true, Some(e)) => {
                effect.attach_to = Some(e);
                // The attached entity's frame (its scale applies) holds the offset.
                let s = world.get::<Transform>(e).map_or(Vec3::ONE, |t| t.scale);
                effect.position = (local + extra) / s.max(Vec3::splat(1e-3));
            }
            _ => {
                let mut p = frame.position + Quat::from_rotation_y(frame.yaw) * local + extra;
                if spec.pin_to_surface {
                    if let Some(h) = surface_height(world, p.x, p.z) {
                        p.y = h.max(WATER_Y);
                    }
                }
                effect.position = p;
                effect.rotation = Quat::from_rotation_y(frame.yaw);
            }
        }
    }
    // ADAPTED: a new full-screen effect takes the place of an older one on
    // the same screen layer (the scripts start `fade_from_black_*` without
    // stopping a sustained `fade_to_black_*`, e.g. CRG_BrainUpgrade1/1a).
    let layers = world.get_resource::<SwarmLibrary>().map(|l| screen_layers(l, spec.effect)).unwrap_or_default();
    if !layers.is_empty() {
        let mut replaced = Vec::new();
        for (i, old) in scene.effects.iter().enumerate() {
            let old_layers = world.get_resource::<SwarmLibrary>().map(|l| screen_layers(l, old.effect)).unwrap_or_default();
            if old_layers.iter().any(|l| layers.contains(l)) {
                replaced.push(i);
            }
        }
        for i in replaced.into_iter().rev() {
            let old = scene.effects.remove(i);
            if let Some(mut e) = world.get_mut::<SwarmEffect>(old.entity) {
                e.stop = true;
                e.hard_stop = true;
            }
            if let Some(b) = old.blocker {
                cin.runtime.finish(b);
            }
        }
        // A persisting screen effect of an earlier cinematic ends the same way.
        let mut kept = Vec::new();
        for old in std::mem::take(&mut scene.persisted) {
            let old_layers = world.get_resource::<SwarmLibrary>().map(|l| screen_layers(l, old.effect)).unwrap_or_default();
            if old_layers.iter().any(|l| layers.contains(l)) {
                if let Some(mut e) = world.get_mut::<SwarmEffect>(old.entity) {
                    e.stop = true;
                    e.hard_stop = true;
                }
            } else {
                kept.push(old);
            }
        }
        scene.persisted = kept;
    }
    let entity = world.spawn((effect, StageOwned, Name::new(format!("cinematic fx {}", spec.effect_name)))).id();
    scene.effects.push(NamedEffect { name: spec.name, effect: spec.effect, entity, blocker, persist: spec.persist });
}

/// Screen layers of an effect's own screen components.
fn screen_layers(lib: &SwarmLibrary, effect: u32) -> Vec<i16> {
    let Some(r) = lib.find(effect) else { return Vec::new() };
    let dir = lib.dir(r.dir);
    let Some(def) = dir.effects.get(r.index as usize) else { return Vec::new() };
    def.blocks
        .iter()
        .filter(|b| b.kind == crate::swarm_fx::format::kind::SCREEN && b.index >= 0)
        .filter_map(|b| dir.screens.get(b.index as usize))
        .filter(|s| matches!(s.mode, 0..=3))
        .map(|s| s.layer)
        .collect()
}

// ---------------------------------------------------------------------------
// Actors
// ---------------------------------------------------------------------------

fn player(world: &mut World) -> Option<Entity> {
    let mut q = world.query_filtered::<Entity, With<Player>>();
    q.iter(world).next()
}

fn add_actor(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, actor: AddActor) {
    let Some(avatar) = player(world) else {
        warn!("cinematic actor {}: no avatar in this stage", actor.label);
        return;
    };
    let (entity, spawned) = match actor.kind {
        // Space: the avatar is the UFO (`homestar` needs more of the galaxy
        // and falls through to the nearest creature).
        ActorType::Avatar | ActorType::Ufo => (avatar, false),
        // ADAPTED: OpenSpore has no simulator creatures of the player's
        // species; a copy of the avatar's body and parts stands in.
        ActorType::PlayerSpecies | ActorType::Posse => (spawn_avatar_copy(world, avatar, &actor.label), true),
        // ADAPTED: npc / rare / targetNpc pick the nearest wild creature; the
        // Space actors (ufo, homestar) are not available yet.
        _ => {
            let at = world.get::<Transform>(avatar).map_or(Vec3::ZERO, |t| t.translation);
            let mut q = world.query_filtered::<(Entity, &Transform), (With<Npc>, Without<Player>)>();
            match q
                .iter(world)
                .min_by(|a, b| a.1.translation.distance(at).total_cmp(&b.1.translation.distance(at)))
                .map(|(e, _)| e)
            {
                Some(e) => (e, false),
                None => {
                    warn!("cinematic actor {} ({:?}) not available", actor.label, actor.kind);
                    return;
                }
            }
        }
    };
    if let Some(rel) = actor.initial {
        let self_frame = entity_frame(world, avatar);
        if let Some(frame) = resolve_relpos(world, scene, cin, token, &rel, self_frame) {
            place(world, entity, frame.position, Some(frame.yaw));
        }
    }
    if let Some(t) = world.get::<Transform>(entity) {
        scene.locks.insert(entity, (t.translation, yaw_of(t.rotation)));
    }
    note_clearance(world, scene, entity);
    // The avatar's death: it topples where it fell (see `ClipTrack::fall`).
    if actor.kind == ActorType::Avatar && matches!(cin.runtime.name(), "CRG_AvatarDied" | "CRG_AvatarDied_Starvation") {
        scene.clips.retain(|j| j.entity != entity);
        scene.clips.push(ClipJob::new(entity, vec![ClipTrack::fall()], None));
    }
    scene.actors.insert(actor.name, ActorSlot { entity, spawned });
}

/// Copy the avatar's drawn hierarchy (body, skinned visuals with their own
/// joint entities, and the parts that ride on it) into a new, static entity.
pub fn spawn_avatar_copy(world: &mut World, avatar: Entity, label: &str) -> Entity {
    use bevy::mesh::skinning::SkinnedMesh;
    let root = *world.get::<Transform>(avatar).unwrap_or(&Transform::IDENTITY);
    let root_global = world.get::<GlobalTransform>(avatar).copied().unwrap_or_else(|| GlobalTransform::from(root));
    // The avatar's subtree, parents before children.
    let mut order = vec![avatar];
    let mut i = 0;
    while i < order.len() {
        if let Some(children) = world.get::<Children>(order[i]) {
            order.extend(children.iter());
        }
        i += 1;
    }
    // Same clearance above the ground as the avatar (terrain::ground keeps it).
    let clearance = surface_height(world, root.translation.x, root.translation.z)
        .map_or(root.translation.y - BASE_Y, |h| root.translation.y - h);
    let mut map: HashMap<Entity, Entity> = HashMap::new();
    for &source in &order {
        let transform = if source == avatar {
            let mut t = root;
            t.translation.y = BASE_Y + clearance;
            t
        } else {
            world.get::<Transform>(source).copied().unwrap_or_default()
        };
        let copy = world.spawn((transform, Visibility::Inherited)).id();
        if source == avatar {
            world.entity_mut(copy).insert((StageOwned, CinematicActor, Name::new(format!("cinematic actor {label}"))));
        } else if let Some(parent) = world.get::<ChildOf>(source).and_then(|p| map.get(&p.parent()).copied()) {
            world.entity_mut(copy).insert(ChildOf(parent));
        }
        map.insert(source, copy);
    }
    for &source in &order {
        let copy = map[&source];
        let mesh = world.get::<Mesh3d>(source).cloned();
        let material = world.get::<MeshMaterial3d<StandardMaterial>>(source).cloned();
        let skin = world.get::<SkinnedMesh>(source).map(|s| SkinnedMesh {
            inverse_bindposes: s.inverse_bindposes.clone(),
            joints: s.joints.iter().map(|j| map.get(j).copied().unwrap_or(Entity::PLACEHOLDER)).collect(),
        });
        let hidden = world.get::<Visibility>(source).is_some_and(|v| *v == Visibility::Hidden);
        if hidden {
            world.entity_mut(copy).insert(Visibility::Hidden);
        }
        match (mesh, material, skin) {
            (Some(mesh), Some(material), Some(skin)) => {
                // A skin whose joints live outside the avatar cannot be copied.
                if !skin.joints.contains(&Entity::PLACEHOLDER) {
                    world.entity_mut(copy).insert((mesh, material, skin));
                }
            }
            (Some(mesh), Some(material), None) => {
                world.entity_mut(copy).insert((mesh, material));
            }
            _ => {}
        }
    }
    // Parts that follow the avatar from outside its hierarchy (`Accessory`).
    let inverse = root_global.affine().inverse();
    let copy_root = map[&avatar];
    let mut q = world.query_filtered::<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform), With<crate::creature::Accessory>>();
    let parts: Vec<_> = q.iter(world).map(|(e, m, mat, g)| (e, m.clone(), mat.clone(), *g)).collect();
    // A skinned part (`creature_morph`) is copied as its plain mesh.
    let parts: Vec<_> = parts.into_iter().map(|(e, m, mat, g)| (crate::creature_morph::plain_mesh(world, e).map_or(m, Mesh3d), mat, g)).collect();
    for (mesh, material, global) in parts {
        let local = Transform::from_matrix(Mat4::from(inverse * global.affine()));
        let child = world.spawn((mesh, material, local, Visibility::Inherited)).id();
        world.entity_mut(child).insert(ChildOf(copy_root));
    }
    copy_root
}

/// Move an entity on the ground (its clearance above the terrain is kept).
fn place(world: &mut World, entity: Entity, position: Vec3, yaw: Option<f32>) {
    let Some(t) = world.get::<Transform>(entity).copied() else { return };
    let y = match (surface_height(world, t.translation.x, t.translation.z), surface_height(world, position.x, position.z)) {
        (Some(from), Some(to)) => t.translation.y + to - from,
        _ => position.y,
    };
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.translation = Vec3::new(position.x, y, position.z);
        if let Some(yaw) = yaw {
            t.rotation = Quat::from_rotation_y(yaw);
        }
    }
}

/// Place an actor and keep it there (and so facing) while the cinematic runs.
fn place_actor(world: &mut World, scene: &mut Scene, entity: Entity, position: Vec3, yaw: Option<f32>) {
    place(world, entity, position, yaw);
    if let Some(t) = world.get::<Transform>(entity) {
        scene.locks.insert(entity, (t.translation, yaw_of(t.rotation)));
    }
    note_clearance(world, scene, entity);
}

/// Remember how high above the surface a locked actor stands.
fn note_clearance(world: &mut World, scene: &mut Scene, entity: Entity) {
    let Some(t) = world.get::<Transform>(entity).map(|t| t.translation) else { return };
    if let Some(h) = surface_height(world, t.x, t.z) {
        scene.clearance.insert(entity, t.y - h);
    }
}

/// Put a locked actor at its place (surface + clearance, yaw only), with a
/// clip's root offset on top when given.
fn apply_lock(world: &mut World, scene: &Scene, entity: Entity, pos: Vec3, yaw: f32, pose: Option<(Vec3, Quat)>) {
    let y = match (surface_height(world, pos.x, pos.z), scene.clearance.get(&entity)) {
        (Some(h), Some(c)) => h + *c,
        _ => pos.y,
    };
    let size = entity_scale(world, Some(entity)).1;
    let facing = Quat::from_rotation_y(yaw);
    let Some(mut t) = world.get_mut::<Transform>(entity) else { return };
    t.translation = Vec3::new(pos.x, y, pos.z);
    t.rotation = facing;
    if let Some((offset, lean)) = pose {
        // ADAPTED: retail keeps the feet planted while the root rises
        // (rearing up); a rigid body would float, so lifts are reduced.
        let offset = Vec3::new(offset.x, offset.y.min(0.0) + offset.y.max(0.0) * POSE_LIFT, offset.z);
        t.translation += facing * (offset * size);
        t.rotation = facing * lean;
    }
}

fn add_object(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, object: AddObject) {
    let Some(rel) = object.initial else { return };
    let Some(mut frame) = resolve_relpos(world, scene, cin, token, &rel, None) else {
        return;
    };
    if let Some(face) = object.face {
        if let Some(f) = resolve_relpos(world, scene, cin, token, &face, Some(frame)) {
            let d = f.position - frame.position;
            if d.length_squared() > 1e-6 {
                frame.yaw = crate::creature::facing_yaw(d);
            }
        }
    }
    scene.objects.insert(object.name, (frame.position, frame.yaw));
}

fn move_actor(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, m: MoveActor, blocker: Option<u32>) {
    let Some(slot) = scene.actors.get(&m.actor).copied() else {
        // Unbound actor: nothing to move (retail looks it up and does nothing).
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    };
    let Some(actor_frame) = entity_frame(world, slot.entity) else {
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    };
    let reference = match &m.relative {
        Some(rel) => resolve_relpos(world, scene, cin, token, rel, Some(actor_frame)),
        None => Some(actor_frame),
    };
    let mut multiplier = 1.0;
    if m.use_footprint_radius {
        multiplier *= entity_scale(world, Some(slot.entity)).0;
    }
    if m.use_target_width || m.use_target_height {
        multiplier *= entity_scale(world, reference.and_then(|r| r.entity)).0;
    }
    let step = stage_offset(world, spore_to_bevy(m.position));
    let goal = reference.map(|r| r.position + Quat::from_rotation_y(r.yaw) * step * multiplier);
    let face = m
        .face
        .as_ref()
        .and_then(|f| resolve_relpos(world, scene, cin, token, f, Some(actor_frame)))
        .map(|f| f.position);
    if m.snap {
        if let Some(goal) = goal {
            place_actor(world, scene, slot.entity, goal, None);
        }
        if let Some(face) = face {
            let at = world.get::<Transform>(slot.entity).map_or(Vec3::ZERO, |t| t.translation);
            let d = face - at;
            if Vec2::new(d.x, d.z).length_squared() > 1e-6 {
                place_actor(world, scene, slot.entity, at, Some(crate::creature::facing_yaw(d)));
            }
        }
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    }
    let speed = world.get::<crate::game::Mover>(slot.entity).map_or(ACTOR_SPEED, |m| m.speed.max(0.5));
    // A goal at the actor's own position is a turn in place.
    let goal = goal.filter(|g| Vec2::new(g.x - actor_frame.position.x, g.z - actor_frame.position.z).length() > 0.05);
    scene.moves.retain(|j| j.entity != slot.entity);
    scene.moves.push(MoveJob {
        entity: slot.entity,
        goal,
        stop_distance: m.stop_distance.max(0.0),
        face,
        speed,
        blocker,
        done: false,
    });
}

fn step_actors(world: &mut World, scene: &mut Scene, dt: f32) {
    for job in &mut scene.moves {
        let Some(t) = world.get::<Transform>(job.entity).copied() else {
            job.done = true;
            continue;
        };
        let (mut pos, mut yaw) = scene.locks.get(&job.entity).copied().unwrap_or((t.translation, yaw_of(t.rotation)));
        let mut arrived = true;
        if let Some(goal) = job.goal {
            let to = Vec3::new(goal.x - pos.x, 0.0, goal.z - pos.z);
            let distance = to.length();
            if distance > job.stop_distance.max(0.05) {
                arrived = false;
                let want = crate::creature::facing_yaw(to);
                yaw = turn(yaw, want, TURN_RATE * dt);
                let step = (job.speed * dt).min(distance - job.stop_distance.max(0.0));
                pos += to / distance * step.max(0.0);
            }
        }
        if arrived {
            job.goal = None;
            match job.face {
                Some(face) => {
                    let d = Vec3::new(face.x - pos.x, 0.0, face.z - pos.z);
                    if d.length_squared() > 1e-6 {
                        let want = crate::creature::facing_yaw(d);
                        yaw = turn(yaw, want, TURN_RATE * dt);
                        if super::camera::wrap(want - yaw).abs() < 0.01 {
                            job.done = true;
                        }
                    } else {
                        job.done = true;
                    }
                }
                None => job.done = true,
            }
        }
        scene.locks.insert(job.entity, (pos, yaw));
    }
    // Re-apply every locked actor (the stage moved it in Update), then the
    // clip's root offset on top.
    let locks: Vec<(Entity, Vec3, f32)> = scene.locks.iter().map(|(e, (p, y))| (*e, *p, *y)).collect();
    for (entity, pos, yaw) in locks {
        if world.get_entity(entity).is_err() {
            scene.locks.remove(&entity);
            continue;
        }
        let pose = scene.poses.get(&entity).copied();
        apply_lock(world, scene, entity, pos, yaw, pose);
    }
}

fn turn(from: f32, to: f32, max: f32) -> f32 {
    let d = super::camera::wrap(to - from);
    if d.abs() <= max {
        to
    } else {
        from + d.signum() * max
    }
}

/// Seconds an animation id plays: the action's TLSA choice (sum of its
/// clips), else the ANIM clip of that instance.
pub fn animation_seconds(originals: &crate::Originals, id: u32) -> Option<f32> {
    // The first action list that defines the id (the per-species lists
    // repeat the generic actions).
    let group = originals
        .action_lists
        .lists
        .iter()
        .flat_map(|(_, list)| &list.groups)
        .find(|g| g.id == id);
    if let Some(group) = group {
        if let Some(choice) = group.choices.first() {
            let mut total = 0.0;
            for a in &choice.animations {
                let secs = if a.duration > 0.0 {
                    a.duration * a.duration_scale.max(0.01)
                } else {
                    clip_seconds(originals, a.id).unwrap_or(0.0)
                };
                // The animations of a choice are the rig variants of one
                // action (equal length, same events), not a sequence.
                total = f32::max(total, secs);
            }
            if total > 0.0 {
                return Some(total);
            }
        }
    }
    clip_seconds(originals, id)
}

fn clip_seconds(originals: &crate::Originals, id: u32) -> Option<f32> {
    let key = originals.animation_catalog.resolve(id, None).ok()?;
    let bytes = originals.animation_catalog.read(&originals.resource_index, key).ok()?;
    let anim = crate::animation::parse(&bytes).ok()?;
    let secs = anim.duration_seconds();
    (secs.is_finite() && secs > 0.0).then_some(secs)
}

fn play_animation(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, p: Box<PlayAnimation>, blocker: Option<u32>) {
    // An actor the script never created or bound does nothing (retail looks
    // it up and finds nothing): a blocking command is over at once. A name
    // the stage code bound to a creature (the hut-destroyed "attacker"s)
    // plays on that creature.
    let token = cin.runtime.token().unwrap_or(0);
    let entity = scene.actors.get(&p.actor).map(|s| s.entity).or_else(|| match cin.binding(token, p.actor) {
        Some(CinematicTarget::Entity(e)) if world.get_entity(e).is_ok() => Some(e),
        _ => None,
    });
    let Some(entity) = entity else {
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    };
    let mut tracks = Vec::new();
    if let Some(originals) = world.get_resource::<crate::Originals>() {
        for id in &p.ids {
            match actors::load_track(originals, *id) {
                Some(track) => tracks.push(track),
                // ADAPTED: an action without a readable clip holds a blocking
                // state for a short while.
                None if blocker.is_some() => tracks.push(ClipTrack::timer(UNKNOWN_ANIMATION_SECS)),
                None => {}
            }
        }
    }
    // A new clip list replaces the actor's running one (its wait is over).
    for old in scene.clips.iter_mut().filter(|j| j.entity == entity) {
        old.done = true;
    }
    if tracks.is_empty() {
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
        return;
    }
    // `-target`: the clip is played at the target (the stick on the ground,
    // the nest the buddies look at). ADAPTED: the actor turns to face it (the
    // retail action aims the clip at it, the aim channels are not shown);
    // with `-processAnimEventsUsingTarget` the clip's events play at the
    // target instead of on the actor.
    let self_frame = entity_frame(world, entity);
    let target = p.target.as_ref().and_then(|rel| resolve_relpos(world, scene, cin, token, rel, None));
    if let (Some(target), Some(me)) = (target, self_frame) {
        let d = target.position - me.position;
        if Vec3::new(d.x, 0.0, d.z).length_squared() > 1e-4 {
            place_actor(world, scene, entity, me.position, Some(crate::creature::facing_yaw(d)));
        }
    }
    let mut job = ClipJob::new(entity, tracks, blocker);
    if p.events_on_target {
        job.event_target = target.map(|t| t.position);
    }
    scene.clips.push(job);
}

/// Who takes the places of a vignette (`startVignette "<group>"`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum VignetteGroup {
    /// The player's tribe; a member named by the binding comes first.
    PlayerTribe(Option<Entity>),
    /// The members of an NPC tribe (village index); a named one first.
    Village(usize, Option<Entity>),
    /// The citizens of the player's capital (Civ).
    Citizens,
}

/// The group of a vignette. VERIFIED (the vignette start 0xAD3EF0, Tribe mode
/// 0x1654C02): the bound object is a tribe (cast 0x17F243B, type 0x18C6D19)
/// -> its members; a creature (0x18EB4B7) -> it takes the first place and its
/// tribe (0xC230B0) the others; members are taken while alive (0xC0B8D0), not
/// flagged (+0xB5E) and not already an actor, until the places are filled
/// (retail then creates members for the rest, 0xC8E730: ADAPTED, not done).
/// In Civ / Space (0x1654C04 / 05) the city's citizens take them (0xAC85F0).
/// "PlayerTribe" is the tribe the Tribal code binds (0xCD4400). ADAPTED:
/// "NewBaby" (`TRG_FirstBaby`) is bound to the home hut (no baby creature
/// exists before it hatches), which stands for the player's tribe.
fn vignette_group(world: &World, cin: &Cinematics, token: u32, actor: u32) -> Option<VignetteGroup> {
    use crate::world_objects::name_id;
    let stage = world.get_resource::<State<GameStage>>().map(|s| s.get().clone());
    if actor == name_id("PlayerTribe") {
        return Some(VignetteGroup::PlayerTribe(None));
    }
    if actor == name_id("") {
        return (stage == Some(GameStage::Civilization)).then_some(VignetteGroup::Citizens);
    }
    if stage != Some(GameStage::Tribal) {
        return None;
    }
    let Some(CinematicTarget::Entity(e)) = cin.binding(token, actor) else { return None };
    if let Some(v) = world.get::<crate::tribal::RivalVillage>(e) {
        return Some(VignetteGroup::Village(v.0, None));
    }
    if let Some(h) = world.get::<crate::tribal::RivalHut>(e) {
        return Some(VignetteGroup::Village(h.0, None));
    }
    if let Some(m) = world.get::<crate::tribal_rts::RivalMember>(e) {
        return Some(VignetteGroup::Village(m.village, Some(e)));
    }
    if world.get::<crate::game::TribeMate>(e).is_some() || world.get::<Player>(e).is_some() {
        return Some(VignetteGroup::PlayerTribe(Some(e)));
    }
    if world.get::<crate::tribal_rts::HomeProp>(e).is_some() {
        return Some(VignetteGroup::PlayerTribe(None));
    }
    None
}

/// The members of a vignette group, the named one first, then by distance
/// to `center`.
fn group_members(world: &mut World, group: VignetteGroup, center: Vec3) -> Vec<Entity> {
    let (mut members, first): (Vec<(Entity, f32)>, Option<Entity>) = match group {
        VignetteGroup::Citizens => {
            let mut q = world.query_filtered::<(Entity, &Transform), With<crate::outfit_crowd::CivCitizen>>();
            (q.iter(world).map(|(e, t)| (e, t.translation.distance(center))).collect(), None)
        }
        VignetteGroup::PlayerTribe(first) => {
            let mut q = world.query_filtered::<(Entity, &Transform), (With<crate::game::TribeMate>, Without<Player>)>();
            (q.iter(world).map(|(e, t)| (e, t.translation.distance(center))).collect(), first)
        }
        VignetteGroup::Village(index, first) => {
            let mut q = world.query::<(Entity, &Transform, &crate::tribal_rts::RivalMember, Option<&crate::game::Health>)>();
            let members = q
                .iter(world)
                .filter(|(_, _, m, h)| m.village == index && !h.is_some_and(|h| h.dead()))
                .map(|(e, t, _, _)| (e, t.translation.distance(center)))
                .collect();
            (members, first)
        }
    };
    members.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut out: Vec<Entity> = first.into_iter().collect();
    out.extend(members.into_iter().map(|(e, _)| e).filter(|e| Some(*e) != first));
    out
}

/// `startVignette`: the scripted behaviour of a group (CinematicVignetteActionData).
/// The group's members (`vignette_group`) take the places the vignette gives
/// them (offsets and orientations in the frame facing the `-face` position)
/// and loop their action lists (root motion and events; the player's tribe
/// and the citizens show the clips on their bodies, `outfit_crowd`).
fn start_vignette(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, token: u32, v: StartVignette, blocker: Option<u32>) {
    let finish = |cin: &mut Cinematics| {
        if let Some(b) = blocker {
            cin.runtime.finish(b);
        }
    };
    // The Civ vignettes (`Vignettes_Civ!open_vi` in TRG2CVG_CivStart_a,
    // `!upgrade_air` / `!upgrade_water` in the unlock cinematics,
    // `!cvg_vi_death` in CVG_LoseA1, `!c2s_vignette_part1` in
    // CVG_UnlockUFO_3a) name no group (""): the citizens of the capital take
    // the places (civ-final; ADAPTED: OpenSpore simulates only the capital's
    // citizens).
    let Some(group) = vignette_group(world, cin, token, v.actor) else {
        debug!("cinematic vignette {} skipped (no group of that name)", v.prop);
        return finish(cin);
    };
    let places = world.get_resource::<crate::Originals>().and_then(|o| actors::load_vignette(o, &v.prop));
    let Some(places) = places else {
        debug!("cinematic vignette {} is not installed", v.prop);
        return finish(cin);
    };
    let reference = match &v.relative {
        Some(rel) => resolve_relpos(world, scene, cin, token, rel, None),
        None => None,
    };
    let Some(reference) = reference else { return finish(cin) };
    let center = reference.position + Quat::from_rotation_y(reference.yaw) * stage_offset(world, spore_to_bevy(v.position));
    // The group's frame faces the `-face` position.
    let face = v.face.as_ref().and_then(|f| resolve_relpos(world, scene, cin, token, f, Some(reference))).map(|f| f.position);
    let frame_yaw = match face {
        Some(p) if (p - center).length_squared() > 1e-6 => crate::creature::facing_yaw(p - center),
        _ => reference.yaw,
    };
    let mut members = group_members(world, group, center);
    // The full record (handheld items per action, centre effects; 0xBC58B0).
    let def = world
        .get_resource::<crate::Originals>()
        .and_then(|o| crate::tribal_vignettes::load_vignette_spec(&o.resource_index, &v.prop));
    // Places the tribe cannot fill get new members (0xAD3EF0: 0xC8E730(1) +
    // 0xC95F00; ADAPTED: temporary bodies the cinematic removes at its end).
    let owner = match group {
        VignetteGroup::PlayerTribe(_) => Some(crate::tribal_vignettes::VignetteOwner::Player),
        VignetteGroup::Village(i, _) => Some(crate::tribal_vignettes::VignetteOwner::Village(i)),
        VignetteGroup::Citizens => None,
    };
    if let Some(owner) = owner {
        for n in members.len()..places.len() {
            let local = stage_offset(world, spore_to_bevy([places[n].offset[0], places[n].offset[1], 0.0]));
            let at = center + Quat::from_rotation_y(frame_yaw) * local;
            if let Some(e) = crate::tribal_vignettes::spawn_filler(world, owner, at) {
                scene.actors.insert(0xF111_0000 ^ (token << 8) ^ n as u32, ActorSlot { entity: e, spawned: true });
                members.push(e);
                info!("cinematic vignette {}: filler {} of {} places ({:?})", v.prop, n + 1, places.len(), owner);
            }
        }
    }
    if let Some(def) = &def {
        for (effect, offset) in &def.effects {
            let local = stage_offset(world, spore_to_bevy([offset.x, offset.y, 0.0]));
            let mut fx = SwarmEffect::new(*effect);
            fx.position = center + Quat::from_rotation_y(frame_yaw) * local;
            fx.rotation = Quat::from_rotation_y(frame_yaw);
            let entity = world.spawn((fx, StageOwned, Name::new(format!("vignette fx {}", def.name)))).id();
            scene.effects.push(NamedEffect { name: 0, effect: *effect, entity, blocker: None, persist: false });
        }
    }
    for (i, (entity, actor)) in members.iter().zip(places.iter()).enumerate() {
        if let Some(place) = def.as_ref().and_then(|d| d.places.get(i)) {
            if place.actions.iter().any(|a| a.item.is_some()) {
                scene.vignette_items.insert(*entity, place.actions.iter().map(|a| (a.clip, a.item)).collect());
            }
        }
        let local = stage_offset(world, spore_to_bevy([actor.offset[0], actor.offset[1], 0.0]));
        let at = center + Quat::from_rotation_y(frame_yaw) * local;
        let yaw = frame_yaw + actor.orientation.to_radians();
        place_actor(world, scene, *entity, at, Some(yaw));
        let tracks: Vec<ClipTrack> = world
            .get_resource::<crate::Originals>()
            .map(|o| actor.animations.iter().filter_map(|id| actors::load_track(o, *id)).collect())
            .unwrap_or_default();
        scene.clips.retain(|j| j.entity != *entity);
        if !tracks.is_empty() {
            scene.clips.push(ClipJob::looped(*entity, tracks));
        }
    }
    // The vignettes of the scripts all run with -noWait.
    finish(cin);
}

/// Advance every clip: its root offset for this frame and its animation
/// events (sounds, effects, `cinematic_continue`).
fn step_clips(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, dt: f32) {
    let mut events: Vec<(Entity, String, Option<Vec3>)> = Vec::new();
    scene.poses.clear();
    for job in &mut scene.clips {
        for name in job.step(dt) {
            events.push((job.entity, name, job.event_target));
        }
        if let Some(pose) = job.pose() {
            scene.poses.insert(job.entity, pose);
        }
    }
    for (entity, name, at) in events {
        animation_event(world, scene, cin, entity, &name, at);
    }
}

/// One animation event (the retail effects / audio managers receive them by
/// name hash): `cinematic_continue` is the game event of `wait -forContinue`;
/// otherwise an installed effect or sound of that name starts at the actor
/// (at `at` for `-processAnimEventsUsingTarget`).
fn animation_event(world: &mut World, scene: &mut Scene, cin: &mut Cinematics, entity: Entity, name: &str, at: Option<Vec3>) {
    if name == "cinematic_continue" {
        if cin.runtime.waiting_for_continue() == Some(super::command::WaitKind::ForContinue) {
            cin.continue_cinematic();
        }
        return;
    }
    let id = crate::world_objects::name_id(name);
    let installed = world.get_resource::<SwarmLibrary>().is_some_and(|l| l.loaded && l.find(id).is_some());
    if installed {
        let mut effect = SwarmEffect::new(id);
        effect.tag = 0xC1E0_0000_0000_0000 | id as u64;
        match at {
            Some(p) => effect.position = p,
            None => effect.attach_to = Some(entity),
        }
        world.spawn((effect, StageOwned, Name::new(format!("clip event fx {name}"))));
    }
    let has_sound = world
        .get_resource::<crate::game_audio::GameAudio>()
        .and_then(|a| a.library())
        .is_some_and(|l| l.resolve(name).is_ok());
    if has_sound {
        let bus = if name.starts_with("music") || name.contains("_music") {
            crate::game_audio::Bus::Music
        } else {
            crate::game_audio::Bus::Sfx
        };
        request_sound(world, scene, id, name);
        scene.pending_sounds.push(PendingSound { id, name: name.to_string(), key: id, mode: SoundMode::Default, bus, asked: scene.clock });
    }
}

/// The brain-up clip (TLSA action) whose end sends the continue of
/// `CRG_BrainUpgrade<n>b`: 0xD7C360 plays it on the avatar
/// (`cCreatureBase::PlayAnimation(0x05405EA0)`) and sends 0x445F729 when it
/// has finished (0xC0E420).
pub const BRAIN_UP_CONTINUE_ANIMATION: u32 = 0x0540_5EA0;

/// The mating behaviour 0xD6CDB0 (VERIFIED): it plays the mate action
/// 0x05E3E754 (with 0x05E3E75B), lays the egg when that clip's animation event
/// 0x58113B7B fires (or the clip has finished, 0xC0E420), binds the egg
/// (0x28930A2C "Egg") and, for the avatar (general flag bit 9), sends 0x445F729:
/// the continue of `CRG_LaidEggA`.
pub const LAY_EGG_ANIMATION: u32 = 0x05E3_E754;
pub const LAY_EGG_EVENT: u32 = 0x5811_3B7B;

/// The alliance celebration (VERIFIED 0xD9CED0 -> behaviour flag 0x800000 on
/// every member of both tribes, priority 5.0, source the allied tribe = the
/// state 0xB5399D36: enter 0xD9E3C0, update 0xD9B4C0, exit 0xD9B470). The
/// chieftain (0xC23430: the creature is the avatar) plays `chief_soc_win01`
/// and sends 0x445F729 when it has finished (0xC0E420) or when the state is
/// left before (exit); this is the continue of `TRG_TotemPole_Social1`.
pub const CHIEF_SOCIAL_WIN: u32 = 0x0657_8207;
/// The other members play the win clip of their held tool (0xD9E3C0 by the
/// tool type +0x102C: 4 horn `soc_flute_win01`, 5 maracas
/// `soc_shakern_win01`, 6 didgeridoo `soc_didger_win01`, else
/// `soc_notool_win01`).
pub const SOCIAL_WIN: [u32; 4] = [0x0657_820B, 0x0657_820E, 0x0657_8208, 0x0657_8211];
/// ADAPTED: the celebration's continue when `chief_soc_win01` cannot be read.
const SOCIAL_WIN_FALLBACK_SECS: f32 = 8.0;

/// The win clip of a member holding `tool` (see `SOCIAL_WIN`).
pub fn social_win_clip(tool: Option<crate::tribal_tools::ToolKind>) -> u32 {
    use crate::tribal_tools::ToolKind;
    match tool {
        Some(ToolKind::WoodenHorns) => SOCIAL_WIN[0],
        Some(ToolKind::Maracas) => SOCIAL_WIN[1],
        Some(ToolKind::Aerophones) => SOCIAL_WIN[2],
        _ => SOCIAL_WIN[3],
    }
}

/// Play a clip of the game's own behaviour on a creature during a
/// cinematic (not a script command: nothing waits on it; the actor stays
/// where the simulation put it).
fn play_behaviour_clip(world: &World, scene: &mut Scene, entity: Entity, id: u32) {
    let Some(track) = world.get_resource::<crate::Originals>().and_then(|o| actors::load_track(o, id)) else { return };
    for old in scene.clips.iter_mut().filter(|j| j.entity == entity) {
        old.done = true;
    }
    scene.clips.push(ClipJob::new(entity, vec![track], None));
}

/// The bound entity of a script name, or an actor of that name.
fn named_entity(world: &World, scene: &Scene, cin: &Cinematics, token: u32, name: &str) -> Option<Entity> {
    let id = crate::world_objects::name_id(name);
    scene.actors.get(&id).map(|s| s.entity).or_else(|| match cin.binding(token, id) {
        Some(CinematicTarget::Entity(e)) if world.get_entity(e).is_ok() => Some(e),
        _ => None,
    })
}

/// The game behaviours that run while a `wait -forContinue` state waits for
/// their 0x445F729 (VERIFIED senders), started once per state:
/// * `CRG_BrainUpgrade<n>b`: 0xD7C360 plays the brain-up clip 0x05405EA0 on
///   the avatar ("TheAvatar");
/// * `TRG_TotemPole_Social1`: the alliance celebration (`CHIEF_SOCIAL_WIN`):
///   the chieftain, the player's tribe by tool, and the allied tribe (bound
///   as "Tribe" by the moment; ADAPTED: its members' tools are not simulated,
///   they play `soc_notool_win01`).
/// The continue itself comes from `continue_seconds` (the clip's length).
fn start_continue_clips(world: &mut World, scene: &mut Scene, cin: &Cinematics, token: u32, state: &str) {
    let state = state.to_ascii_lowercase();
    if state.starts_with("crg_brainupgrade") && state.ends_with('b') {
        if let Some(avatar) = named_entity(world, scene, cin, token, "TheAvatar") {
            play_behaviour_clip(world, scene, avatar, BRAIN_UP_CONTINUE_ANIMATION);
        }
        return;
    }
    if state != "trg_totempole_social1" {
        return;
    }
    let chieftain = named_entity(world, scene, cin, token, "chieftain");
    if let Some(e) = chieftain {
        play_behaviour_clip(world, scene, e, CHIEF_SOCIAL_WIN);
    }
    let mut q = world.query_filtered::<(Entity, Option<&crate::tribal_tools::HeldToolState>), With<crate::game::TribeMate>>();
    let mut plays: Vec<(Entity, u32)> = q
        .iter(world)
        .filter(|(e, _)| Some(*e) != chieftain)
        .map(|(e, held)| (e, social_win_clip(held.and_then(|h| h.shown.flatten()))))
        .collect();
    if let Some(VignetteGroup::Village(index, _)) = vignette_group(world, cin, token, crate::world_objects::name_id("Tribe")) {
        let mut q = world.query::<(Entity, &crate::tribal_rts::RivalMember)>();
        plays.extend(q.iter(world).filter(|(_, m)| m.village == index).map(|(e, _)| (e, social_win_clip(None))));
    }
    for (e, id) in plays {
        play_behaviour_clip(world, scene, e, id);
    }
}

/// Seconds from the start of the mate action to the egg (see
/// `LAY_EGG_ANIMATION`).
fn lay_egg_seconds(originals: &crate::Originals) -> Option<f32> {
    let track = actors::load_track(originals, LAY_EGG_ANIMATION)?;
    let event = track.events.iter().find(|(_, n)| crate::world_objects::name_id(n) == LAY_EGG_EVENT).map(|(t, _)| *t);
    event.or((track.seconds > 0.0).then_some(track.seconds))
}

/// How long a `-forContinue` wait lasts when no game event ends it. ADAPTED:
/// retail waits for the stage's own event; the senders found are timed here
/// (0xD7C360 brain-up, 0xD6CDB0 mating); 0xCDF080 (Tribal) is a timer of the
/// moments (`moments::TOTEM_AFTER_CONQUEST_SECONDS`).
fn continue_seconds(world: &World, cin: &Cinematics, default: f32) -> f32 {
    let state = cin.runtime.current_state().unwrap_or("").to_ascii_lowercase();
    if state.starts_with("crg_brainupgrade") && state.ends_with('b') {
        // VERIFIED sender (0xD7C360): the end of the brain-up clip.
        if let Some(secs) = world
            .get_resource::<crate::Originals>()
            .and_then(|o| animation_seconds(o, BRAIN_UP_CONTINUE_ANIMATION))
        {
            return secs;
        }
    }
    match state.as_str() {
        // VERIFIED sender (0xD6CDB0): the egg is laid at the mate clip's event.
        "crg_laidegga" => world.get_resource::<crate::Originals>().and_then(lay_egg_seconds).unwrap_or(6.0),
        // VERIFIED sender (celebration state 0xB5399D36, update 0xD9B4C0): the
        // end of the chieftain's `chief_soc_win01` (the camera orbit of 55 s
        // in the script is cut short by it).
        "trg_totempole_social1" => world
            .get_resource::<crate::Originals>()
            .and_then(|o| animation_seconds(o, CHIEF_SOCIAL_WIN))
            .unwrap_or(SOCIAL_WIN_FALLBACK_SECS),
        // Steve's journey and the way back: the effects' end continues them
        // (cPlayer 0xC77ED0, `space_endgame` sends the continue).
        "spg_centerofgalaxy_journey" | "spg_centerofgalaxy_returnback" => crate::space_endgame::CONTINUE_WAIT_SECS,
        _ => default,
    }
}

fn clear_area(world: &mut World, scene: &Scene, cin: &Cinematics, token: u32, object: u32, radius: f32, flags: u8) {
    if flags & clear_flags::ANIMALS == 0 || radius <= 0.0 {
        return;
    }
    let Some(center) = resolve_name(world, scene, cin, token, object) else { return };
    let radius = stage_length(world, radius);
    let keep: Vec<Entity> = scene.actors.values().map(|s| s.entity).collect();
    let mut q = world.query_filtered::<(Entity, &Transform), (With<Npc>, Without<Player>)>();
    let moves: Vec<(Entity, Vec3)> = q
        .iter(world)
        .filter(|(e, _)| !keep.contains(e))
        .filter_map(|(e, t)| {
            let d = Vec3::new(t.translation.x - center.position.x, 0.0, t.translation.z - center.position.z);
            let len = d.length();
            (len < radius).then(|| {
                let dir = if len > 1e-3 { d / len } else { Vec3::X };
                (e, center.position + dir * radius)
            })
        })
        .collect();
    // ADAPTED: animals are warped to the rim (retail gives them a keep-away
    // job unless `-snap`).
    for (e, p) in moves {
        place(world, e, p, None);
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn sync_ui(world: &mut World, scene: &mut Scene) {
    if scene.ui_root.is_some_and(|e| world.get_entity(e).is_err()) {
        scene.ui_root = None;
    }
    if scene.ui_root.is_none() {
        if !scene.playing {
            return;
        }
        let Some(camera) = game_camera(world) else { return };
        let layout = world.resource_scope(|world, mut assets: Mut<SpuiAssets>| {
            let originals = world.get_resource::<crate::Originals>()?;
            if originals.resource_index.records.is_empty() {
                return None;
            }
            assets.layout(&originals.resource_index, LAYOUT)
        });
        let Some(layout) = layout else { return };
        let mut commands = world.commands();
        let root = crate::spui::view::spawn(&mut commands, layout, crate::spui::solve::RootPolicy::Fit, Some(camera), 450);
        commands.entity(root).insert((CinematicUiRoot, Name::new("cinematic letterbox")));
        drop(commands);
        world.flush();
        scene.ui_root = Some(root);
    }
    let Some(root) = scene.ui_root else { return };
    let Some(mut state) = world.get_mut::<SpuiState>(root) else { return };
    let playing = scene.playing;
    let set = |state: &mut SpuiState, control: u32, on: bool| {
        if let Some(id) = state.find(control) {
            if state.visible(id) != on {
                state.set_visible(id, on);
            }
        }
    };
    let letterbox = playing && scene.letterbox;
    set(&mut state, win::LETTERBOX_TOP, letterbox);
    set(&mut state, win::LETTERBOX_BOTTOM, letterbox);
    // The subtitle styles show only with the subtitle option (0x6854E0(2)),
    // which OpenSpore does not have: off.
    for control in [win::SPEECH, win::THOUGHT, win::CAPTION, win::RADIO, win::CLOSE] {
        set(&mut state, control, false);
    }
    // ADAPTED: the text windows 0x0445C9AB / 0x07980D2C are TextEdits, which
    // the SPUI engine does not draw text for; the text goes to the bottom-bar
    // Text window 0x067A0FC6 (same place, the layout's `LetterBox` style).
    // `textUpper` is unused by the installed scripts.
    set(&mut state, win::TEXT, false);
    set(&mut state, win::TEXT_UPPER, false);
    let scrolled = match (scene.scrolling.as_ref(), state.find(win::SCROLLING)) {
        (Some(t), Some(id)) => {
            let r = state.rect(id);
            scroll_window(t, scene.clock - scene.scroll_from, scene.scroll_duration, [r.x2 - r.x1, r.y2 - r.y1])
        }
        _ => None,
    };
    let text = scene.text.as_ref().or(scrolled.as_ref());
    if let (Some(id), Some(text)) = (state.find(win::SCROLLING), text) {
        if state.captions[id].as_deref() != Some(text.as_str()) {
            state.set_text(id, text.clone());
        }
    }
    set(&mut state, win::SCROLLING, playing && text.is_some());
    let button = playing && scene.continue_ui == Some(ContinueUi::Button);
    let next = playing && scene.continue_ui == Some(ContinueUi::Next);
    set(&mut state, win::BUTTON, button);
    set(&mut state, win::NEXT, next);
    set(&mut state, win::CONTAINER, button || next);
    set(&mut state, win::BLACK, playing && scene.black);
}

/// The rows of a `scrollingText` visible `elapsed` s into its `duration` in a
/// window of `size` (layout px): the credits-like scroller 0x832E30 started
/// by 0xAD75F0 moves the text up through window 0x067A0FC6, entering at the
/// bottom and leaving at the top over the duration; afterwards nothing shows.
/// ADAPTED: whole-line steps and the wrap / row count from a nominal glyph
/// box (7 x 17 px), since the SPUI engine has no scrolled text draw.
pub fn scroll_window(text: &str, elapsed: f32, duration: f32, size: [f32; 2]) -> Option<String> {
    const CHAR_W: f32 = 7.0;
    const LINE_H: f32 = 17.0;
    if duration <= 0.0 || !(0.0..duration).contains(&elapsed) {
        return None;
    }
    let cols = ((size[0] / CHAR_W) as usize).max(12);
    let rows = ((size[1] / LINE_H) as usize).max(1);
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > cols {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    // Lines that have entered the window from the bottom.
    let entered = ((elapsed / duration) * (lines.len() + rows) as f32) as usize;
    let rows_text: Vec<&str> = (0..rows)
        .map(|r| {
            let i = (entered + r) as isize - rows as isize;
            if i >= 0 { lines.get(i as usize).map_or("", |s| s.as_str()) } else { "" }
        })
        .collect();
    if rows_text.iter().all(|s| s.is_empty()) {
        return Some(String::new());
    }
    Some(rows_text.join("\n"))
}

/// Hide every other UI root while a cinematic plays; restore afterwards.
fn hide_hud(world: &mut World, scene: &mut Scene) {
    let mine = scene.ui_root;
    if scene.playing {
        // The SPUI composite shows every layout (ours included) rendered by
        // the SPUI camera: keep it, hide the other layouts' roots instead.
        let mut q = world.query_filtered::<(Entity, &mut Visibility), (
            With<Node>,
            Without<ChildOf>,
            Without<crate::spui::surface::SpuiComposite>,
        )>();
        let mut newly = Vec::new();
        for (e, mut v) in q.iter_mut(world) {
            if Some(e) == mine {
                continue;
            }
            if *v != Visibility::Hidden {
                *v = Visibility::Hidden;
                newly.push(e);
            }
        }
        for e in newly {
            if !scene.hidden_ui.contains(&e) {
                scene.hidden_ui.push(e);
            }
        }
    } else if !scene.hidden_ui.is_empty() {
        for e in std::mem::take(&mut scene.hidden_ui) {
            if let Some(mut v) = world.get_mut::<Visibility>(e) {
                if *v == Visibility::Hidden {
                    *v = Visibility::Inherited;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

fn request_sound(world: &mut World, scene: &mut Scene, id: u32, name: &str) {
    if scene.sounds_loaded.contains_key(&id) {
        return;
    }
    if scene.sound_jobs.is_none() {
        let Some(library) = world.get_resource::<crate::game_audio::GameAudio>().and_then(|a| a.library()) else {
            scene.sounds_loaded.insert(id, None);
            return;
        };
        let (jobs, queue) = mpsc::channel::<SoundJob>();
        let (done, results) = mpsc::channel::<SoundDecoded>();
        let spawned = std::thread::Builder::new()
            .name("cinematic-audio".into())
            .spawn(move || {
                let mut rng = rand::rng();
                for job in queue {
                    let spec = crate::game_audio::CueSpec {
                        key: job.name,
                        names: Box::leak(vec![job.name].into_boxed_slice()),
                        bus: crate::game_audio::Bus::Sfx,
                        trim: 1.0,
                        min_interval: 0.0,
                        window: None,
                        variants: 1,
                        pool: false,
                    };
                    let result = crate::game_audio::load_cue(&library, &spec, &mut rng)
                        .map(|(def, clips)| (def.gain, def.pitch, clips.into_iter().map(|c| (c.wav, c.secs)).collect()));
                    if done.send(SoundDecoded { id: job.id, result }).is_err() {
                        break;
                    }
                }
            })
            .is_ok();
        if !spawned {
            scene.sounds_loaded.insert(id, None);
            return;
        }
        scene.sound_jobs = Some(jobs);
        scene.sound_results = Some(Mutex::new(results));
    }
    let name: &'static str = Box::leak(name.to_string().into_boxed_str());
    let sent = scene.sound_jobs.as_ref().is_some_and(|j| j.send(SoundJob { id, name }).is_ok());
    if !sent {
        scene.sounds_loaded.insert(id, None);
    }
}

/// Collect decoded sounds and start the due ones (within 2 s of the request;
/// later ones are dropped, a missing sound is silent like in retail).
fn pump_sounds(world: &mut World, scene: &mut Scene) {
    let ready: Vec<SoundDecoded> = match scene.sound_results.as_ref().map(|r| r.lock().map(|r| r.try_iter().collect())) {
        Some(Ok(v)) => v,
        _ => Vec::new(),
    };
    for decoded in ready {
        let loaded = match decoded.result {
            Ok((gain, pitch, clips)) => {
                let Some(mut assets) = world.get_resource_mut::<Assets<AudioSource>>() else {
                    scene.sounds_loaded.insert(decoded.id, None);
                    continue;
                };
                let clips = clips.into_iter().map(|(wav, _)| assets.add(AudioSource { bytes: wav.into() })).collect();
                Some(LoadedSound { gain, pitch, clips })
            }
            Err(e) => {
                debug!("cinematic sound {:08X} unavailable: {e}", decoded.id);
                None
            }
        };
        scene.sounds_loaded.insert(decoded.id, loaded);
    }
    let muted = world.get_resource::<crate::game_audio::GameAudio>().is_some_and(|a| a.muted);
    let clock = scene.clock;
    let pending = std::mem::take(&mut scene.pending_sounds);
    for p in pending {
        match scene.sounds_loaded.get(&p.id) {
            Some(Some(loaded)) => {
                if muted || loaded.clips.is_empty() || world.get_resource::<Assets<AudioSource>>().is_none() {
                    continue;
                }
                let gain = p.bus.volume() * loaded.gain;
                info!("cinematic sound {} started (gain {gain:.2})", p.name);
                world.spawn((
                    AudioPlayer::new(loaded.clips[0].clone()),
                    PlaybackSettings::DESPAWN
                        .with_volume(Volume::Linear(gain))
                        .with_speed(loaded.pitch.clamp(0.1, 4.0)),
                    CinematicSound { key: p.key, mode: p.mode },
                    Name::new("cinematic sound"),
                ));
            }
            Some(None) => info!("cinematic sound {} is not installed", p.name),
            None => {
                if clock - p.asked < 2.0 {
                    scene.pending_sounds.push(p);
                } else {
                    info!("cinematic sound {} not decoded in time, dropped", p.name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_text_enters_at_the_bottom_and_leaves_at_the_top() {
        let text = "one\ntwo\nthree\nfour";
        // A window of two rows: 4 lines + 2 rows of travel over 6 s.
        let size = [200.0, 34.0];
        assert_eq!(scroll_window(text, 0.0, 6.0, size).as_deref(), Some(""));
        assert_eq!(scroll_window(text, 1.0, 6.0, size).as_deref(), Some("\none"));
        assert_eq!(scroll_window(text, 2.5, 6.0, size).as_deref(), Some("one\ntwo"));
        assert_eq!(scroll_window(text, 5.5, 6.0, size).as_deref(), Some("four\n"));
        assert_eq!(scroll_window(text, 6.0, 6.0, size), None, "gone after the duration");
        // Long paragraphs wrap at the window's width.
        let wrapped = scroll_window(&"word ".repeat(40), 3.0, 6.0, [100.0, 340.0]).unwrap();
        assert!(wrapped.lines().all(|l| l.chars().count() <= 14), "{wrapped}");
    }

    #[test]
    fn civ_lengths_shrink_with_the_world_but_less_when_framing_objects() {
        let far = crate::civ_rules::CIV_WORLD_SCALE;
        if far >= 1.0 {
            // Civ plays on the planet at retail distances (Landing 030): the
            // scripted lengths are used unchanged.
            for m in [10.0f32, 60.0, 650.0, -12.0, 0.0] {
                assert!((civ_length(m) - m).abs() < 1e-3, "{m}");
            }
            return;
        }
        assert!((civ_length(10.0) / 10.0 - far.sqrt()).abs() < 1e-5, "object framing");
        assert!((civ_length(650.0) / 650.0 - far).abs() < 1e-5, "island framing");
        let mid = civ_length(60.0);
        assert!(mid > 60.0 * far && mid < 60.0 * far.sqrt(), "{mid}");
        assert_eq!(civ_length(0.0), 0.0);
        assert!(civ_length(-12.0) < 0.0 && (civ_length(-12.0) + civ_length(12.0)).abs() < 1e-5);
    }

    /// 0xAD3EF0: a vignette's group is the bound tribe's members (an NPC
    /// village, the player's tribe), a bound creature first; the Civ ones the
    /// citizens.
    #[test]
    fn vignette_groups_follow_the_bound_object() {
        use crate::world_objects::name_id;
        let mut world = World::new();
        world.insert_resource(State::new(GameStage::Tribal));
        let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
        let village = world.spawn((at(50.0), crate::tribal::RivalVillage(2))).id();
        let member = |village: usize, x: f32| crate::tribal_rts::RivalMember {
            village,
            home: Vec3::new(x, 0.0, 0.0),
            target: Vec3::ZERO,
            wait: 0.0,
            mode: crate::tribal_rts::RivalMode::Wander,
            stolen: 0.0,
            speed: 1.0,
        };
        let near = world.spawn((at(48.0), member(2, 48.0))).id();
        let far = world.spawn((at(60.0), member(2, 60.0))).id();
        world.spawn((at(49.0), member(1, 49.0)));
        let hut = world
            .spawn((at(0.0), crate::tribal_rts::HomeProp { kind: crate::tribal_rts::HomePropKind::Hut, shown: String::new() }))
            .id();
        let mate_a = world.spawn((at(3.0), crate::game::TribeMate)).id();
        let mate_b = world.spawn((at(1.0), crate::game::TribeMate)).id();
        let mut cin = Cinematics::default();
        let token = CinematicToken(7);
        cin.bind_object(token, "Tribe", CinematicTarget::Entity(village));
        cin.bind_object(token, "NewBaby", CinematicTarget::Entity(hut));
        cin.bind_object(token, "attacker1", CinematicTarget::Entity(far));
        cin.bind_object(token, "chieftainMate", CinematicTarget::Entity(mate_a));
        let group = |world: &World, name: &str| vignette_group(world, &cin, 7, name_id(name));
        assert_eq!(group(&world, "Tribe"), Some(VignetteGroup::Village(2, None)));
        assert_eq!(group(&world, "attacker1"), Some(VignetteGroup::Village(2, Some(far))));
        assert_eq!(group(&world, "NewBaby"), Some(VignetteGroup::PlayerTribe(None)));
        assert_eq!(group(&world, "PlayerTribe"), Some(VignetteGroup::PlayerTribe(None)));
        assert_eq!(group(&world, "chieftainMate"), Some(VignetteGroup::PlayerTribe(Some(mate_a))));
        assert_eq!(group(&world, "Nobody"), None);
        assert_eq!(group(&world, ""), None, "the citizens are Civ's");
        // Members: only that village, nearest first; a named one leads.
        assert_eq!(group_members(&mut world, VignetteGroup::Village(2, None), Vec3::new(50.0, 0.0, 0.0)), [near, far]);
        assert_eq!(group_members(&mut world, VignetteGroup::Village(2, Some(far)), Vec3::new(50.0, 0.0, 0.0)), [far, near]);
        assert_eq!(group_members(&mut world, VignetteGroup::PlayerTribe(None), Vec3::ZERO), [mate_b, mate_a]);
        world.insert_resource(State::new(GameStage::Civilization));
        assert_eq!(group(&world, ""), Some(VignetteGroup::Citizens));
        assert_eq!(group(&world, "Tribe"), None);
    }

    /// 0xD9E3C0: the celebration clip follows the held tool (type 4 / 5 / 6).
    #[test]
    fn the_alliance_celebration_clip_follows_the_tool() {
        use crate::tribal_tools::ToolKind;
        assert_eq!(social_win_clip(Some(ToolKind::WoodenHorns)), 0x0657_820B);
        assert_eq!(social_win_clip(Some(ToolKind::Maracas)), 0x0657_820E);
        assert_eq!(social_win_clip(Some(ToolKind::Aerophones)), 0x0657_8208);
        assert_eq!(social_win_clip(Some(ToolKind::StoneAxes)), 0x0657_8211);
        assert_eq!(social_win_clip(None), 0x0657_8211);
    }

    #[test]
    fn locale_tokens_are_filled() {
        let values = [("tribe_name", "Tribe 2".to_string())];
        assert_eq!(fill_tokens("You made the ~tribe_name~ allies!", &values), "You made the Tribe 2 allies!");
        assert_eq!(fill_tokens("Destroyed ~ufo_name~.~br~Next", &[]), "Destroyed UFO.\nNext");
        assert_eq!(fill_tokens("a ~ b ~ c", &[]), "a ~ b ~ c", "a lone tilde is text");
        assert_eq!(fill_tokens("no tokens", &[]), "no tokens");
        assert_eq!(fill_tokens("open ~tag", &[]), "open ~tag");
    }

    #[test]
    fn turning_is_rate_limited_the_short_way() {
        assert_eq!(turn(0.0, 0.5, 1.0), 0.5);
        assert!((turn(0.0, 2.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((turn(3.0, -3.0, 0.1) - 3.1).abs() < 1e-6);
    }
}

/// One line about the cinematic UI (captures, debugging).
pub fn debug_ui(world: &World) -> String {
    let Some(scene) = world.get_resource::<Scene>() else { return "no scene".into() };
    let Some(root) = scene.ui_root else { return format!("no ui root (playing {})", scene.playing) };
    let size = world.get::<ComputedNode>(root).map(|c| c.size());
    let vis = world.get::<Visibility>(root).copied();
    let target = world.get::<UiTargetCamera>(root).map(|t| t.entity());
    let bars = world.get::<SpuiState>(root).map(|s| {
        let w = |c: u32| {
            s.find(c).map(|i| {
                let e = s.entities.get(i).copied();
                let ev = e.and_then(|e| world.get::<Visibility>(e)).copied();
                let iv = e.and_then(|e| world.get::<InheritedVisibility>(e)).map(|v| v.get());
                let vv = e.and_then(|e| world.get::<ViewVisibility>(e)).map(|v| v.get());
                let bg = e.and_then(|e| world.get::<BackgroundColor>(e)).map(|b| b.0.to_srgba().alpha);
                let parent = e.and_then(|e| world.get::<ChildOf>(e)).map(|p| p.parent());
                let cn = e.and_then(|e| world.get::<ComputedNode>(e)).map(|c| c.size());
                let gt = e.and_then(|e| world.get::<bevy::ui::UiGlobalTransform>(e)).map(|t| t.translation);
                let cam = e.and_then(|e| world.get::<bevy::ui::ComputedUiTargetCamera>(e)).map(|c| c.get());
                (s.visible(i), ev, iv, vv, bg, parent, cn, gt, cam)
            })
        };
        format!("ready {} top {:?} text {:?} button {:?}", s.ready, w(win::LETTERBOX_TOP), w(win::SCROLLING), w(win::BUTTON))
    });
    let iv = world.get::<InheritedVisibility>(root).map(|v| v.get());
    let names: Vec<String> = scene
        .hidden_ui
        .iter()
        .take(40)
        .map(|e| world.get::<Name>(*e).map_or(format!("{e:?}"), |n| format!("{e:?}={n}")))
        .collect();
    let hidden_names = names.join(",");
    let effects: Vec<String> = scene
        .effects
        .iter()
        .map(|e| {
            let fx = world.get::<SwarmEffect>(e.entity);
            let mut screens = Vec::new();
            if let (Some(lib), Some(inst)) = (world.get_resource::<SwarmLibrary>(), fx.and_then(|f| f.instance.as_ref())) {
                inst.for_each_screen(lib, &mut |st, def| screens.push((def.mode, st.strength(def))));
            }
            format!("{:08X}:{:?} screens {screens:?}", e.name, fx.map(|f| (f.alive, f.instance.is_some(), f.position, f.attach_to)))
        })
        .collect();
    let actors: Vec<String> = scene
        .actors
        .iter()
        .map(|(n, a)| {
            let t = world.get::<Transform>(a.entity).map(|t| t.translation);
            let kids = world.get::<Children>(a.entity).map_or(0, |c| c.len());
            format!("{n:08X}:{:?} spawned {} at {t:?} children {kids}", a.entity, a.spawned)
        })
        .collect();
    let _ = (hidden_names, bars, size, vis, iv, target);
    let mut draws = 0;
    let mut shown = 0;
    for (e, _) in world.iter_entities().filter_map(|e| e.get::<crate::swarm_fx::render::SwarmDraw>().map(|d| (e.id(), d))) {
        draws += 1;
        if world.get::<Visibility>(e).is_some_and(|v| *v == Visibility::Visible) {
            shown += 1;
        }
    }
    format!("effects {effects:?} draws {shown}/{draws} actors {} screen {:?}", actors.len(), scene.screen_effect)
}
