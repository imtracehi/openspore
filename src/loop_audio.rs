//! Looping named sounds with an owner, and distance attenuation of positional named sounds
//! (helper `audio-codec`).
//!
//! `cell_audio::NamedSounds` starts one-shots only. Retail also loops sounds that belong to
//! something that can go away (the armed super-weapon loops `sfx_*_armed_lp`, the hologram scout
//! `sfx_hologram_scout_lp`, a turret or crowd hum, ...). A loop is requested with
//! [`LoopSounds::start`] for an *owner entity* and a sound name; it runs until
//! [`LoopSounds::stop`] / [`LoopSounds::stop_owner`] is called or the owner is despawned (INFERRED:
//! retail ends an object's audio track with the object). Decoding goes through the same
//! `NamedSounds` worker as the one-shots (a looped request decodes one variant), the soundProp's
//! gain, pitch and `silent` flag apply as for one-shots.
//!
//! Positional sounds: the soundProp's `is3D`, `minDistance`, `maxDistance` and `attenuation` curve
//! are read by `AudioLibrary::resolve` and evaluated by `SoundDef::attenuation` (INFERRED reading,
//! shared with the Pure Data `Spatial` objects: the distance as a fraction of min..max through the
//! curve; linear without a curve). The distance is measured in retail units from the listener to the
//! sound: world distance / `unit` (the stage's scale: 1.0 in the planet-based land stages).
//! ADAPTED: the listener is the game camera (as the effect sounds do), overridable with
//! [`ListenerSpot`]. [`Hearing`] gives any system that distance, and [`Hearing::play`] a
//! ready-to-queue [`Play`] with it set.
//!
//! A loop whose sound is not decoded yet starts when it is; one the library cannot decode stays
//! silent (no retry storm: the request is made once). Loops follow their owner every frame when
//! [`LoopAt::Owner`] is used and are rescaled by the listener's distance every frame; the voice
//! entities are `StageOwned`, so a stage switch removes them.

use std::collections::HashMap;

use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode, PlaybackSettings, Volume},
    ecs::system::SystemParam,
    prelude::*,
};

use crate::{
    cell_audio::{NamedSounds, Play},
    game_audio::{Bus, GameAudio, SoundDef},
    stages::StageOwned,
    world_objects::name_id,
    GameCamera,
};

/// Where a loop is heard from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopAt {
    /// Not positional (full level whatever the listener does).
    Everywhere,
    /// At the owner entity's `GlobalTransform`, followed every frame.
    Owner,
    /// At a fixed world position.
    Point(Vec3),
}

/// How a loop plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopSpec {
    pub at: LoopAt,
    /// Extra gain on top of the soundProp's.
    pub gain: f32,
    pub bus: Bus,
    /// World units per retail unit at the sound (distances are divided by it).
    pub unit: f32,
}

impl LoopSpec {
    pub fn everywhere() -> Self {
        Self { at: LoopAt::Everywhere, gain: 1.0, bus: Bus::Sfx, unit: 1.0 }
    }

    pub fn at_owner() -> Self {
        Self { at: LoopAt::Owner, ..Self::everywhere() }
    }

    pub fn at_point(point: Vec3) -> Self {
        Self { at: LoopAt::Point(point), ..Self::everywhere() }
    }

    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    pub fn bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self
    }

    pub fn unit(mut self, unit: f32) -> Self {
        self.unit = unit;
        self
    }
}

struct Wanted {
    name: &'static str,
    spec: LoopSpec,
    voice: Option<Entity>,
    /// The soundProp sets gain 0 itself (patch-driven levels): never played directly.
    silent: bool,
}

/// The loops that should be running, keyed by (owner entity, sound id).
#[derive(Resource, Default)]
pub struct LoopSounds {
    wanted: HashMap<(Entity, u32), Wanted>,
    /// Voices of stopped loops, despawned by the next update.
    stopping: Vec<Entity>,
    /// Voices spawned so far (tests, debug).
    pub started: u64,
}

impl LoopSounds {
    /// Keep `name` looping for `owner`. Calling it again for a running loop only updates how it
    /// plays (position mode, gain, unit); it never restarts the sample.
    pub fn start(&mut self, owner: Entity, name: &'static str, spec: LoopSpec) {
        let id = name_id(name);
        match self.wanted.get_mut(&(owner, id)) {
            Some(w) => w.spec = spec,
            None => {
                self.wanted.insert((owner, id), Wanted { name, spec, voice: None, silent: false });
            }
        }
    }

    /// Change the extra gain of a running loop (fades, ducking by the caller).
    pub fn set_gain(&mut self, owner: Entity, name: &str, gain: f32) {
        if let Some(w) = self.wanted.get_mut(&(owner, name_id(name))) {
            w.spec.gain = gain;
        }
    }

    /// Stop one loop of `owner`. Returns whether it was running.
    pub fn stop(&mut self, owner: Entity, name: &str) -> bool {
        match self.wanted.remove(&(owner, name_id(name))) {
            Some(w) => {
                self.stopping.extend(w.voice);
                true
            }
            None => false,
        }
    }

    /// Stop every loop of `owner`.
    pub fn stop_owner(&mut self, owner: Entity) {
        let keys: Vec<_> = self.wanted.keys().filter(|(o, _)| *o == owner).copied().collect();
        for key in keys {
            if let Some(w) = self.wanted.remove(&key) {
                self.stopping.extend(w.voice);
            }
        }
    }

    pub fn is_active(&self, owner: Entity, name: &str) -> bool {
        self.wanted.contains_key(&(owner, name_id(name)))
    }

    /// Loops running (or waiting for their sample) in all.
    pub fn len(&self) -> usize {
        self.wanted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wanted.is_empty()
    }

    /// Names of the loops `owner` has.
    pub fn names(&self, owner: Entity) -> Vec<&'static str> {
        let mut names: Vec<_> = self.wanted.iter().filter(|((o, _), _)| *o == owner).map(|(_, w)| w.name).collect();
        names.sort_unstable();
        names
    }
}

/// Where positional sounds are heard from when it is not the game camera (set it to the
/// avatar, a cinematic camera, ...). `None` falls back to the camera.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct ListenerSpot(pub Option<Vec3>);

/// The listener, for systems that play positional sounds.
#[derive(SystemParam)]
pub struct Hearing<'w, 's> {
    camera: Query<'w, 's, &'static GlobalTransform, With<GameCamera>>,
    spot: Option<Res<'w, ListenerSpot>>,
}

impl Hearing<'_, '_> {
    /// World position of the listener.
    pub fn position(&self) -> Option<Vec3> {
        self.spot
            .as_ref()
            .and_then(|s| s.0)
            .or_else(|| self.camera.single().ok().map(|t| t.translation()))
    }

    /// Distance from the listener to `at` in retail units (`unit` world units each).
    pub fn distance(&self, at: Vec3, unit: f32) -> Option<f32> {
        distance_in_units(self.position()?, at, unit)
    }

    /// A one-shot named sound at a world position: the soundProp's falloff applies when
    /// `NamedSounds` starts it.
    pub fn play(&self, name: &'static str, source: &'static str, at: Vec3, unit: f32) -> Play {
        let mut play = Play::named(name, source);
        play.distance = self.distance(at, unit);
        play
    }
}

/// Retail units between two world positions.
pub fn distance_in_units(listener: Vec3, at: Vec3, unit: f32) -> Option<f32> {
    let d = listener.distance(at);
    (unit > 0.0 && d.is_finite()).then_some(d / unit)
}

/// Linear level of a loop: bus volume, the soundProp's gain, the caller's gain and, for a
/// positional sound, the falloff at `distance` (retail units). Not clamped above 1.
pub fn loop_level(def: &SoundDef, bus_volume: f32, gain: f32, distance: Option<f32>) -> f32 {
    bus_volume * def.gain * gain * distance.map_or(1.0, |d| def.attenuation(d))
}

/// A started loop voice.
#[derive(Component, Debug)]
pub struct LoopVoice {
    pub owner: Entity,
    pub id: u32,
    /// The level last given to the sink (tests, debug).
    pub level: f32,
}

#[allow(clippy::too_many_arguments)]
fn drive_loops(
    mut commands: Commands,
    audio: Res<GameAudio>,
    mut sounds: ResMut<NamedSounds>,
    mut loops: ResMut<LoopSounds>,
    owners: Query<Option<&GlobalTransform>>,
    hearing: Hearing,
    mut voices: Query<(&mut LoopVoice, Option<&mut AudioSink>)>,
    pd: Option<Res<crate::pd_audio::PdMusic>>,
) {
    for voice in std::mem::take(&mut loops.stopping) {
        commands.entity(voice).try_despawn();
    }
    let mute = if audio.muted { 0.0 } else { 1.0 };
    let listener = hearing.position();
    let mut gone = Vec::new();
    let mut started = 0;
    for (&(owner, id), wanted) in loops.wanted.iter_mut() {
        let Ok(owner_at) = owners.get(owner) else {
            gone.push((owner, id));
            continue;
        };
        if wanted.silent {
            continue;
        }
        // The voice may have been removed from outside (stage switch): start it again.
        if wanted.voice.is_some_and(|v| voices.get(v).is_err()) {
            wanted.voice = None;
        }
        let at = match wanted.spec.at {
            LoopAt::Everywhere => None,
            LoopAt::Owner => owner_at.map(|t| t.translation()),
            LoopAt::Point(p) => Some(p),
        };
        let distance = match (listener, at) {
            (Some(l), Some(a)) => distance_in_units(l, a, wanted.spec.unit),
            _ => None,
        };
        sounds.request(id, Some(wanted.name), true);
        let Some(loaded) = sounds.loaded(id) else {
            if sounds.missing(id) {
                wanted.silent = true;
            }
            continue;
        };
        // The Pd mixers of the sound's groups scale a loop as they scale a patch voice.
        let mixer = pd.as_ref().map_or(1.0, |pd| pd.group_gain(&loaded.def.groups));
        let level = loop_level(&loaded.def, wanted.spec.bus.volume(), wanted.spec.gain, distance) * mixer * mute;
        match wanted.voice {
            Some(voice) => {
                if let Ok((mut state, sink)) = voices.get_mut(voice) {
                    state.level = level;
                    if let Some(mut sink) = sink {
                        sink.set_volume(Volume::Linear(level));
                    }
                }
            }
            None => {
                if loaded.def.silent {
                    wanted.silent = true;
                    continue;
                }
                // Spawned even at level 0 (out of range): the sink is needed to raise it again.
                let (handle, _) = loaded.clips[0].clone();
                let voice = commands
                    .spawn((
                        Name::new(format!("loop {}", wanted.name)),
                        AudioPlayer::new(handle),
                        PlaybackSettings {
                            mode: PlaybackMode::Loop,
                            volume: Volume::Linear(level),
                            speed: loaded.def.pitch.clamp(0.1, 4.0),
                            ..PlaybackSettings::ONCE
                        },
                        LoopVoice { owner, id, level },
                        StageOwned,
                    ))
                    .id();
                wanted.voice = Some(voice);
                started += 1;
            }
        }
    }
    loops.started += started;
    for key in gone {
        if let Some(w) = loops.wanted.remove(&key) {
            if let Some(voice) = w.voice {
                commands.entity(voice).try_despawn();
            }
        }
    }
}

pub struct LoopAudioPlugin;

impl Plugin for LoopAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoopSounds>()
            .init_resource::<ListenerSpot>()
            .add_systems(Update, drive_loops.run_if(resource_exists::<NamedSounds>).run_if(resource_exists::<GameAudio>));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_audio::SoundDef;

    fn def(is_3d: bool, min: f32, max: f32, curve: Vec<(f32, f32)>) -> SoundDef {
        SoundDef {
            name: "test_lp".into(),
            samples: vec![1],
            gain: 0.5,
            looped: true,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 0,
            is_3d,
            min_distance: min,
            max_distance: max,
            polyphony: 0,
            start_delay: 0.0,
            curve,
            silent: false,
            groups: vec![],
        }
    }

    #[test]
    fn positional_loops_follow_the_soundprop_falloff() {
        let near = def(true, 10.0, 110.0, vec![]);
        // Inside min distance: full; halfway: linear 0.5; beyond max: silent.
        assert!((loop_level(&near, 1.0, 1.0, Some(5.0)) - 0.5).abs() < 1e-5);
        assert!((loop_level(&near, 1.0, 1.0, Some(60.0)) - 0.25).abs() < 1e-5);
        assert_eq!(loop_level(&near, 1.0, 1.0, Some(500.0)), 0.0);
        // The soundProp's own curve (x = fraction of min..max).
        let curved = def(true, 0.0, 100.0, vec![(0.0, 1.0), (0.5, 0.2), (1.0, 0.0)]);
        let mid = loop_level(&curved, 1.0, 1.0, Some(50.0));
        assert!((mid - 0.1).abs() < 0.02, "{mid}");
        // Not positional: no attenuation whatever the distance; no distance: full level.
        let flat = def(false, 10.0, 110.0, vec![]);
        assert!((loop_level(&flat, 1.0, 1.0, Some(500.0)) - 0.5).abs() < 1e-5);
        assert!((loop_level(&near, 0.8, 2.0, None) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn distances_are_in_retail_units() {
        let d = distance_in_units(Vec3::ZERO, Vec3::new(30.0, 0.0, 40.0), 10.0).unwrap();
        assert!((d - 5.0).abs() < 1e-5);
        assert!(distance_in_units(Vec3::ZERO, Vec3::X, 0.0).is_none());
    }

    #[test]
    fn owners_start_and_stop_their_loops() {
        let mut world = World::new();
        let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut loops = LoopSounds::default();
        loops.start(a, "sfx_civ_bribe_armed_lp", LoopSpec::at_owner());
        loops.start(a, "sfx_civ_bribe_armed_lp", LoopSpec::at_owner().gain(0.5)); // idempotent
        loops.start(a, "sfx_faithheal_armed_lp", LoopSpec::everywhere());
        loops.start(b, "sfx_civ_bribe_armed_lp", LoopSpec::at_point(Vec3::ONE));
        assert_eq!(loops.len(), 3);
        assert_eq!(loops.names(a), vec!["sfx_civ_bribe_armed_lp", "sfx_faithheal_armed_lp"]);
        assert!(loops.is_active(b, "sfx_civ_bribe_armed_lp"));
        assert!(loops.stop(a, "sfx_faithheal_armed_lp"));
        assert!(!loops.stop(a, "sfx_faithheal_armed_lp"));
        loops.stop_owner(a);
        assert_eq!(loops.len(), 1);
        assert!(!loops.is_active(a, "sfx_civ_bribe_armed_lp"));
        loops.set_gain(b, "sfx_civ_bribe_armed_lp", 0.25);
        assert_eq!(loops.wanted.values().next().unwrap().spec.gain, 0.25);
    }

    /// The whole system on a decoder-less `NamedSounds` (a fake decoded sound): a loop starts once,
    /// follows its owner's distance to the camera and ends when the owner is despawned.
    #[test]
    fn loop_voices_follow_their_owner_and_die_with_it() {
        let mut app = App::new();
        app.init_resource::<GameAudio>().init_resource::<NamedSounds>().init_resource::<LoopSounds>();
        app.add_systems(Update, drive_loops);
        app.world_mut().spawn((GameCamera, GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0))));
        let owner = app.world_mut().spawn(GlobalTransform::from(Transform::from_xyz(5.0, 0.0, 0.0))).id();
        let name = "sfx_hologram_scout_lp";
        app.world_mut().resource_mut::<NamedSounds>().insert_ready_for_test(
            name_id(name),
            def(true, 10.0, 110.0, vec![]),
            Handle::default(),
        );
        app.world_mut().resource_mut::<LoopSounds>().start(owner, name, LoopSpec::at_owner());
        app.update();
        let level_of = |app: &mut App| {
            let mut q = app.world_mut().query::<&LoopVoice>();
            q.iter(app.world()).map(|v| v.level).collect::<Vec<_>>()
        };
        // 5 units away is inside min distance: the soundProp's gain only (bus volume 1 in tests
        // is not guaranteed, so compare against the same formula).
        let bus = Bus::Sfx.volume();
        let levels = level_of(&mut app);
        assert_eq!(levels.len(), 1);
        assert!((levels[0] - bus * 0.5).abs() < 1e-4, "{levels:?}");
        assert_eq!(app.world().resource::<LoopSounds>().started, 1);
        // Moving the owner out of range silences the same voice, no second voice starts.
        app.world_mut().entity_mut(owner).insert(GlobalTransform::from(Transform::from_xyz(300.0, 0.0, 0.0)));
        app.update();
        assert_eq!(level_of(&mut app), vec![0.0]);
        assert_eq!(app.world().resource::<LoopSounds>().started, 1);
        // Despawning the owner ends the loop and removes its voice.
        app.world_mut().entity_mut(owner).despawn();
        app.update();
        app.update();
        assert!(level_of(&mut app).is_empty());
        assert!(app.world().resource::<LoopSounds>().is_empty());
    }

    #[test]
    fn explicit_stop_removes_the_voice() {
        let mut app = App::new();
        app.init_resource::<GameAudio>().init_resource::<NamedSounds>().init_resource::<LoopSounds>();
        app.add_systems(Update, drive_loops);
        let owner = app.world_mut().spawn_empty().id();
        let name = "sfx_civ_bribe_armed_lp";
        app.world_mut().resource_mut::<NamedSounds>().insert_ready_for_test(
            name_id(name),
            def(false, 0.0, 0.0, vec![]),
            Handle::default(),
        );
        app.world_mut().resource_mut::<LoopSounds>().start(owner, name, LoopSpec::everywhere());
        app.update();
        let voices = |app: &mut App| app.world_mut().query::<&LoopVoice>().iter(app.world()).count();
        assert_eq!(voices(&mut app), 1);
        assert!(app.world_mut().resource_mut::<LoopSounds>().stop(owner, name));
        app.update();
        assert_eq!(voices(&mut app), 0);
        // Silent soundProps (gain 0 themselves) never start a voice.
        let mut silent = def(false, 0.0, 0.0, vec![]);
        silent.silent = true;
        app.world_mut().resource_mut::<NamedSounds>().insert_ready_for_test(name_id("sfx_x_lp"), silent, Handle::default());
        app.world_mut().resource_mut::<LoopSounds>().start(owner, "sfx_x_lp", LoopSpec::everywhere());
        app.update();
        assert_eq!(voices(&mut app), 0);
    }

    /// Falloff data of the retail sounds the stage code loops or places in the world (printed as
    /// `POS|name|looped|3d|min|max|curve points|gain`; `notes/audio-codec.md` section 4).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_positional_and_looped_stage_sounds() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let names = [
            "sfx_civ_bribe_armed_lp",
            "sfx_blackcloud_armed_lp",
            "sfx_civ_adblitz_armed_lp",
            "sfx_faithheal_armed_lp",
            "sfx_tact_icbm_target_armed_lp",
            "sfx_civ_target_walla_lp",
            "sfx_civ_turret_rotateb_lp",
            "sfx_hologram_scout_lp",
            "space_blackhole_lp",
            "Mili_Takefire",
            "Mili_Gtg",
            "sfx_creature_stealth",
            "sfx_trg_gift_start",
            "sfx_civ_losecity",
            "sfx_trg_com_sp_hi",
            "sfx_trg_com_sp_throwmiss",
            "sfx_ufo_abduct_success",
            "sfx_genesis_energy_blast",
            "ui_space_alert",
            "ui_veh_select_land",
        ];
        let mut looped = 0;
        for name in names {
            let Ok(def) = library.resolve(name) else {
                println!("POS|{name}|unresolved");
                continue;
            };
            looped += usize::from(def.looped);
            println!(
                "POS|{name}|{}|{}|{:.0}|{:.0}|{}|{:.2}",
                def.looped,
                def.is_3d,
                def.min_distance,
                def.max_distance,
                def.curve.len(),
                def.gain
            );
        }
        assert!(looped >= 5, "the armed / target sounds are loops ({looped})");
    }
}
