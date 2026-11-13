//! Effect sound components in every stage (Cell, Creature, Tribal, Civ, Space).
//!
//! RETAIL data (`notes\cell-audio-notes.md` section 1, `SoundEffect`): an effect's
//! sound component names a soundProp / SNR and carries the flags 3D 1, loop 2,
//! volume 4, stop-with-effect 8, track 0x10, a location update period
//! (`invSample`, default 0.05 s) and a volume multiplier. The component belongs
//! to a block of the effect, so it starts with the instance that owns it; the
//! simulation (`swarm_fx::sim`) reports every live component with the world
//! position of its block ([`SwarmSounds`]) and this module plays them.
//!
//! * One-shots go through `cell_audio::NamedSounds` (soundProp gain, pitch,
//!   polyphony, start delay), with the distance to the listener; the
//!   soundProp's `is3d` / `mindistance` / `maxdistance` / `attenuation` shape
//!   the level (`SoundDef::attenuation`). VERIFIED census: no installed
//!   component sets the 3D flag, 2455 of 3011 name an `is3d` soundProp.
//! * Loops live as long as their instance: a loop that disappears from the
//!   simulation's list (its effect or the nested instance ended or was
//!   despawned) stops; a stop-with-effect loop also stops when the effect is
//!   told to stop. 3D loops follow the effect's position every `invSample`.
//!   A loop whose level is zero (out of range) is virtual and starts when the
//!   listener comes within range. In the Cell (planar listener) a loop and a one-shot
//!   are positional voices (`cell_spatial`): the track update (roll-off, `atmospheric`
//!   low-pass, distance high-pass, panner, reverb send) is re-evaluated every frame from
//!   the component's position (refreshed every `invSample`) and the listener; the other
//!   stages keep the flat sink level.
//! * Listener: the retail audio listener is the camera (INFERRED from the
//!   provider 0xEA8AE0 and the NPC city music, `notes\civ-polish-re.md` M5). Cell
//!   keeps its established rule (the avatar, flat, in `UNIT`s; VERIFIED 0xE73F60: the camera
//!   look point while a cinematic plays, `cell_acoustics`). Space measures
//!   in retail units (`PlanetView::unit` in the planet view, `SOLAR_SCALE`
//!   in the solar scene); the land stages' world units are retail units.
//!
//! ADAPTED: a one-shot with the stop flag plays out when its effect ends
//! (the queue of `NamedSounds` does not hand back its voices); the track id
//! (flag 0x10, a handle for the game to find the sound again) is not used.
//!
//! `OPENSPORE_FX_DEBUG=1` logs every start / stop with the distance.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode, PlaybackSettings, Volume},
    prelude::*,
};

use crate::{
    cell_audio::{NamedSounds, Play},
    cell_engine::UNIT,
    cell_spatial::{Anchor, SpatialStart, VoiceSpec},
    game::Player,
    game_audio::{Bus, GameAudio},
    stages::{GameStage, StageOwned},
    swarm_fx::{
        format::sound_flag,
        sound::{LiveSound, SwarmSounds},
    },
};

/// Most looped effect sounds heard at once (the rest wait as virtual voices).
pub const MAX_LOOPS: usize = 16;

/// Where sounds are heard from and how world distances become retail units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub position: Vec3,
    /// World units per retail unit.
    pub unit: f32,
    /// Ignore height (the Cell world is flat).
    pub planar: bool,
}

impl Listener {
    /// Distance from the listener to `at`, in retail units.
    pub fn distance(&self, at: Vec3) -> f32 {
        let mut d = at - self.position;
        if self.planar {
            d.y = 0.0;
        }
        d.length() / self.unit.max(1e-6)
    }
}

/// One playing (or waiting) sound component.
struct Voice {
    flags: u32,
    sound: u32,
    volume: f32,
    period: f32,
    /// Looped voice entity once decoded and in range.
    entity: Option<Entity>,
    next_update: f64,
    /// Level of the last location update (before mute).
    level: f32,
    /// A positional Cell voice (`cell_spatial`): it follows the component and sets its own level.
    spatial: bool,
    /// Stopped by its effect or unplayable: stays listed until the component
    /// leaves the simulation's list so it is not started again.
    ended: bool,
    seen: bool,
}

impl Voice {
    fn looped(&self) -> bool {
        self.flags & sound_flag::LOOP != 0
    }
}

/// The component voices by simulation uid.
#[derive(Resource, Default)]
pub struct EffectVoices {
    voices: HashMap<u64, Voice>,
    /// Components started this run (one-shots queued, loops spawned).
    pub started: u64,
    /// Loops stopped this run.
    pub stopped: u64,
}

impl EffectVoices {
    #[cfg(test)]
    pub fn live(&self) -> usize {
        self.voices.values().filter(|v| !v.ended).count()
    }
}

/// A looped component's voice.
#[derive(Component)]
pub struct EffectLoopVoice;

fn debug_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("OPENSPORE_FX_DEBUG").is_some() || std::env::var_os("OPENSPORE_CELL_AUDIO_DEBUG").is_some()
    })
}

/// The listener of the current stage.
fn listener_of(
    stage: &GameStage,
    player: &Query<&Transform, With<Player>>,
    camera: &Query<&GlobalTransform, With<crate::GameCamera>>,
    view: Option<&crate::space_view::SpaceView>,
    cell_look: Option<Vec2>,
) -> Option<Listener> {
    match stage {
        // 0xE73F60: the avatar, or the camera look point while a cinematic plays
        // (`cell_look` is set then; `cell_acoustics::listener_plane`).
        GameStage::Cell => match cell_look {
            Some(look) => Some(Listener { position: Vec3::new(look.x, 0.0, look.y), unit: UNIT, planar: true }),
            None => player
                .single()
                .ok()
                .map(|t| Listener { position: t.translation, unit: UNIT, planar: true }),
        },
        GameStage::Space => {
            let unit = view
                .and_then(|v| v.planet.as_ref().filter(|_| v.zoom() == crate::space_view::SpaceZoom::Planet))
                .map_or(crate::space_view::SOLAR_SCALE, |p| p.unit().max(1e-6));
            camera.single().ok().map(|c| Listener { position: c.translation(), unit, planar: false })
        }
        _ => camera.single().ok().map(|c| Listener { position: c.translation(), unit: 1.0, planar: false }),
    }
}

/// Pure: the level of a looped sound for its soundProp at `distance`
/// (`None` = not positional): bus x soundProp gain x component volume x the
/// soundProp's roll-off.
pub fn loop_level(def: &crate::game_audio::SoundDef, volume: f32, distance: Option<f32>) -> f32 {
    Bus::Sfx.volume() * def.gain * volume * distance.map_or(1.0, |d| def.attenuation(d))
}

/// Start, move and stop the sounds of the effects the simulation runs.
#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    stage: Res<State<GameStage>>,
    audio: Res<GameAudio>,
    frame: Res<SwarmSounds>,
    mut sounds: ResMut<NamedSounds>,
    mut state: ResMut<EffectVoices>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    view: Option<Res<crate::space_view::SpaceView>>,
    mut sinks: Query<Option<&mut AudioSink>, With<EffectLoopVoice>>,
    pd: Option<Res<crate::pd_audio::PdMusic>>,
    cell_game: Option<Res<crate::cell_engine::CellGame>>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    mut spatial: SpatialStart,
) {
    let now = time.elapsed_secs_f64();
    let mute = if audio.muted { 0.0 } else { 1.0 };
    // The Pd mixers of the sound's groups (`chase_group`, `sfx_group`, ...)
    // scale a loop as they scale a patch voice.
    let mixer = |def: &crate::game_audio::SoundDef| pd.as_ref().map_or(1.0, |pd| pd.group_gain(&def.groups));
    let cell_look = cell_game
        .as_ref()
        .filter(|_| cinematics.as_ref().is_some_and(|c| c.is_playing()))
        .map(|g| g.camera_look);
    let listener = listener_of(stage.get(), &player, &camera, view.as_deref(), cell_look);
    let EffectVoices { voices, started, stopped } = &mut *state;
    for v in voices.values_mut() {
        v.seen = false;
    }
    let mut loops = voices.values().filter(|v| v.entity.is_some()).count();
    for live in &frame.live {
        // Every installed component has the 3D flag clear (3011 of 3011 in the
        // census) while 2455 of them name an `is3d` soundProp: the soundProp
        // decides whether the level follows the distance, so the component's
        // position is always handed over.
        let distance = listener.map(|l| l.distance(live.origin));
        // The flat Cell world (planar listener): sounds are positional voices that the track
        // update re-shapes every frame (`cell_spatial`).
        let anchored = listener.is_some_and(|l| l.planar);
        let stop_with_effect = live.flags & sound_flag::STOP_WITH_EFFECT != 0;
        let voice = voices.entry(live.voice).or_insert_with(|| {
            let ended = live.stopped && stop_with_effect;
            if !ended {
                start(live, distance, anchored, &mut sounds, now);
                *started += 1;
            }
            Voice {
                flags: live.flags,
                sound: live.sound,
                volume: live.volume,
                period: live.period,
                entity: None,
                next_update: now,
                level: 0.0,
                spatial: false,
                ended,
                seen: true,
            }
        });
        voice.seen = true;
        if voice.ended || !voice.looped() {
            continue;
        }
        if live.stopped && stop_with_effect {
            end_loop(&mut commands, voice, stopped, "effect stopped");
            continue;
        }
        if voice.entity.is_none() {
            // Decoded and audible: start it (a level of zero waits).
            if sounds.missing(voice.sound) {
                voice.ended = true;
                continue;
            }
            let Some(loaded) = sounds.loaded(voice.sound) else { continue };
            let def = &loaded.def;
            let level = loop_level(def, voice.volume, distance) * mixer(def);
            if def.silent || audio.muted || level <= 0.0 || loops >= MAX_LOOPS {
                continue;
            }
            let (handle, _) = loaded.clips[0].clone();
            // The Cell: a looped sound follows its component and is re-shaped every frame.
            if anchored {
                let spec = VoiceSpec {
                    loaded: &loaded,
                    pick: 0,
                    looped: true,
                    speed: def.pitch,
                    anchor: Anchor::Effect(live.voice, voice.period, Vec2::new(live.origin.x, live.origin.z)),
                    base: Bus::Sfx.volume() * def.gain * voice.volume,
                    mixer: mixer(def),
                };
                if let Some(entity) = spatial.start(&mut commands, "effect loop", spec) {
                    if debug_enabled() {
                        eprintln!(
                            "effect audio: loop {} starts for {:?} (level {level:.2}, d {distance:?}, positional voice)",
                            sounds.label(voice.sound),
                            live.effect
                        );
                    }
                    // Loops do not count against the one-shot voice limit (`MAX_LOOPS` limits them).
                    commands.entity(entity).insert(EffectLoopVoice).remove::<crate::cell_audio::CellVoice>();
                    voice.entity = Some(entity);
                    voice.spatial = true;
                    voice.level = level;
                    loops += 1;
                    *started += 1;
                    continue;
                }
            }
            if debug_enabled() {
                eprintln!(
                    "effect audio: loop {} starts for {:?} (level {level:.2}, d {distance:?})",
                    sounds.label(voice.sound),
                    live.effect
                );
            }
            let entity = commands
                .spawn((
                    Name::new("effect sound loop"),
                    AudioPlayer::new(handle),
                    PlaybackSettings {
                        mode: PlaybackMode::Loop,
                        volume: Volume::Linear(level),
                        speed: def.pitch,
                        ..PlaybackSettings::ONCE
                    },
                    EffectLoopVoice,
                    StageOwned,
                ))
                .id();
            voice.entity = Some(entity);
            voice.level = level;
            voice.next_update = now + voice.period as f64;
            loops += 1;
            *started += 1;
        } else if let Some(entity) = voice.entity {
            // A positional voice sets its own level every frame (`cell_spatial::drive_spatial`).
            if voice.spatial {
                continue;
            }
            // Location update every `invSample` seconds; mute applies at once.
            if now >= voice.next_update {
                voice.next_update = now + voice.period as f64;
                if let Some(loaded) = sounds.loaded(voice.sound) {
                    voice.level = loop_level(&loaded.def, voice.volume, distance) * mixer(&loaded.def);
                }
            }
            if let Ok(Some(mut sink)) = sinks.get_mut(entity) {
                sink.set_volume(Volume::Linear(voice.level * mute));
            }
        }
    }
    // Voices whose component left the list lost their effect instance.
    voices.retain(|_, v| {
        if v.seen {
            return true;
        }
        if v.looped() && !v.ended {
            end_loop(&mut commands, v, stopped, "effect gone");
        }
        false
    });
}

/// Queue a one-shot component (loops are decoded here and started by `drive`).
fn start(live: &LiveSound, distance: Option<f32>, anchored: bool, sounds: &mut NamedSounds, now: f64) {
    if live.flags & sound_flag::LOOP != 0 {
        sounds.request(live.sound, None, true);
        return;
    }
    let label = sounds.label(live.sound);
    let mut play = Play::named(label, "effect");
    play.id = live.sound;
    play.gain = live.volume;
    play.distance = distance;
    play.anchor = anchored.then_some(Anchor::Effect(live.voice, live.period, Vec2::new(live.origin.x, live.origin.z)));
    if debug_enabled() {
        eprintln!("effect audio: {label} starts (volume {:.2}, d {distance:?})", live.volume);
    }
    sounds.play(play, now, 0.0);
}

fn end_loop(commands: &mut Commands, voice: &mut Voice, stopped: &mut u64, why: &str) {
    if let Some(entity) = voice.entity.take() {
        commands.entity(entity).despawn();
        *stopped += 1;
        if debug_enabled() {
            eprintln!("effect audio: loop {:08X} stops ({why})", voice.sound);
        }
    }
    voice.ended = true;
}

pub struct EffectAudioPlugin;

impl Plugin for EffectAudioPlugin {
    fn build(&self, app: &mut App) {
        // The decoder and the player of `NamedSounds` run in every stage's own
        // audio chain (Cell: `cell_audio`; Creature: `creature_audio`; the
        // others: `anim_audio`); a queued sound starts this frame or the next.
        app.add_plugins(crate::tribal_weapon_audio::TribalWeaponAudioPlugin);
        app.init_resource::<EffectVoices>().add_systems(
            Update,
            drive
                .run_if(|s: Option<Res<State<GameStage>>>| s.is_some_and(|s| *s.get() != GameStage::MainMenu))
                .run_if(resource_exists::<GameAudio>)
                .run_if(resource_exists::<NamedSounds>)
                .run_if(resource_exists::<SwarmSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_distances_follow_the_stage_scale() {
        let flat = Listener { position: Vec3::new(1.0, 5.0, 1.0), unit: 2.0, planar: true };
        // The Cell world is flat: height does not count; two world units per retail unit.
        assert!((flat.distance(Vec3::new(5.0, 0.0, 1.0)) - 2.0).abs() < 1e-6);
        let land = Listener { position: Vec3::new(0.0, 30.0, 0.0), unit: 1.0, planar: false };
        // A land camera hears the ground from its height.
        assert!((land.distance(Vec3::new(0.0, 0.0, 40.0)) - 50.0).abs() < 1e-4);
        let solar = Listener { position: Vec3::ZERO, unit: crate::space_view::SOLAR_SCALE, planar: false };
        assert!((solar.distance(Vec3::new(0.26, 0.0, 0.0)) - 1.0).abs() < 1e-5);
    }

    fn live(voice: u64, flags: u32, origin: Vec3) -> LiveSound {
        LiveSound {
            effect: Entity::PLACEHOLDER,
            voice,
            flags,
            sound: 0x1234_5678,
            volume: 0.5,
            period: 0.05,
            origin,
            stopped: false,
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<GameStage>()
            .init_resource::<GameAudio>()
            .init_resource::<NamedSounds>()
            .init_resource::<SwarmSounds>()
            .init_resource::<Time<Real>>()
            .add_plugins(EffectAudioPlugin);
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        app.update();
        app
    }

    #[test]
    fn a_one_shot_plays_once_and_forgets_its_effect() {
        let mut app = app();
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(1, sound_flag::THREE_D, Vec3::X)];
        app.update();
        assert_eq!(app.world().resource::<NamedSounds>().queued(), vec![0x1234_5678]);
        assert_eq!(app.world().resource::<EffectVoices>().started, 1);
        // Still listed: not started again.
        app.update();
        assert_eq!(app.world().resource::<NamedSounds>().queued().len(), 1);
        // The effect is gone: the voice is forgotten, a new one starts afresh.
        app.world_mut().resource_mut::<SwarmSounds>().live.clear();
        app.update();
        assert_eq!(app.world().resource::<EffectVoices>().live(), 0);
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(2, 0, Vec3::ZERO)];
        app.update();
        assert_eq!(app.world().resource::<NamedSounds>().queued().len(), 2);
    }

    #[test]
    fn a_stop_with_effect_sound_that_is_stopped_before_it_starts_is_skipped() {
        let mut app = app();
        let mut l = live(3, sound_flag::STOP_WITH_EFFECT, Vec3::ZERO);
        l.stopped = true;
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![l];
        app.update();
        assert!(app.world().resource::<NamedSounds>().queued().is_empty());
        // A sound without the flag still plays out.
        let mut l = live(4, 0, Vec3::ZERO);
        l.stopped = true;
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![l];
        app.update();
        assert_eq!(app.world().resource::<NamedSounds>().queued().len(), 1);
    }

    fn loop_def() -> crate::game_audio::SoundDef {
        crate::game_audio::SoundDef {
            name: "test_loop".into(),
            samples: vec![1],
            gain: 1.0,
            looped: true,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 1,
            is_3d: true,
            min_distance: 10.0,
            max_distance: 100.0,
            polyphony: 0,
            start_delay: 0.0,
            curve: vec![],
            silent: false,
            groups: vec![],
        }
    }

    fn loops(app: &mut App) -> usize {
        app.world_mut().query::<&EffectLoopVoice>().iter(app.world()).count()
    }

    #[test]
    fn a_loop_follows_its_effect_and_stops_with_it() {
        let mut app = app();
        app.world_mut().spawn((crate::GameCamera, GlobalTransform::IDENTITY));
        app.world_mut().resource_mut::<NamedSounds>().insert_ready(0x1234_5678, loop_def(), 2.0);
        let flags = sound_flag::LOOP | sound_flag::STOP_WITH_EFFECT;
        // Out of range (beyond 100 units): a virtual voice, nothing spawned.
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(9, flags, Vec3::new(0.0, 0.0, 500.0))];
        app.update();
        assert_eq!(loops(&mut app), 0);
        // The effect moves into range: the loop starts.
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(9, flags, Vec3::new(0.0, 0.0, 20.0))];
        app.update();
        assert_eq!(loops(&mut app), 1);
        assert_eq!(app.world().resource::<EffectVoices>().started, 2, "queued request + spawned loop");
        // Still alive: no second voice.
        app.update();
        assert_eq!(loops(&mut app), 1);
        // The effect is told to stop (stop-with-effect): the loop ends and stays ended.
        let mut stopping = live(9, flags, Vec3::new(0.0, 0.0, 20.0));
        stopping.stopped = true;
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![stopping];
        app.update();
        app.update();
        assert_eq!(loops(&mut app), 0);
        assert_eq!(app.world().resource::<EffectVoices>().stopped, 1);
        // A loop whose effect vanishes (despawned) ends too.
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(10, flags, Vec3::new(0.0, 0.0, 20.0))];
        app.update();
        assert_eq!(loops(&mut app), 1);
        app.world_mut().resource_mut::<SwarmSounds>().live.clear();
        app.update();
        assert_eq!(loops(&mut app), 0);
        assert_eq!(app.world().resource::<EffectVoices>().stopped, 2);
    }

    #[test]
    fn a_loop_without_a_decoder_is_dropped_quietly() {
        let mut app = app();
        app.world_mut().resource_mut::<SwarmSounds>().live = vec![live(5, sound_flag::LOOP | sound_flag::THREE_D, Vec3::ZERO)];
        app.update();
        app.update();
        // No decoder thread in the test: the sound is missing, the voice ended.
        assert_eq!(app.world().resource::<EffectVoices>().live(), 0);
        assert!(app.world().resource::<NamedSounds>().queued().is_empty(), "loops never queue as one-shots");
    }
}
