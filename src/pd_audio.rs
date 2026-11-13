//! The Pure Data music engine in the game: mix modes start patch instances,
//! the game feeds audio parameters, patch voices play through the original
//! samples.
//!
//! RETAIL (`notes\pd-music-re-notes.md`): a patch runs as a sound. The Cell
//! game starts `cell_motion`, `cellgame_seed_music` (-> bundle
//! `cellgame_seed_music6`) and `cell_amb` from code (cCellGFX::StartDisplay
//! 0xE55120); every other mode starts the `startsounds` of its `mode` record
//! (`mixmode_creaturegame` -> `music_creature_modified`, `mixmode_tribegame`
//! -> `tribe_drums` / `music_tribe_social`, `mixmode_global` -> the mixer
//! patches). A sound stops with its soundProp `fadeout`. The game sets
//! global float parameters with `0x5CA970(FNV(name), value)` (Cell:
//! 0xE73F60 every frame), patches send `sendgame` values back, and the
//! active mix modes' `pausegroups` pause the instances and voices whose
//! soundProp mixer groups match.
//!
//! OpenSpore: the engine runs by default (`OPENSPORE_PD_MUSIC=0` turns it
//! off; the hand port in `cell_audio` and the music director's loop
//! approximations play then):
//! * Cell: the three patches run instead of `cell_audio`'s score layers, fed
//!   from `CellAudioParams` every frame (`sound2` animation triggers such as
//!   `cell_egg_lay` included);
//! * Creature / Tribe: the mode's startsounds run (patches as instances,
//!   plain loops such as `planetambiloop` as voices, except the camera-curve
//!   wind beds [`CURVE_DRIVEN_BEDS`]) and the director's music and beds are
//!   muted; [`feeds`] sets their inputs from the stage at the retail moments
//!   (per-frame Creature update, on-change parameter providers, events) and
//!   drives the Tribal performance from `music_tribe_social`'s beat.
//!
//! The Tribe positional music (`tribal_audio::feed_raid_music`): the raid and
//! gift loops are plain voices that move with their source
//! ([`PdMusic::start_positional_voice`], [`PdMusic::move_voice`]) and roll off
//! from the camera ([`PdMusic::ears`]).
//!
//! API: [`PdMusic::set_audio_param`] / [`PdMusic::set_audio_param_hash`]
//! (also the target of the cinematic `mixEvent`), [`PdSendGame`] messages for
//! `sendgame`, [`PdMusic::mixer_gain`] for code that plays sounds of a mixer
//! group itself. `OPENSPORE_PD_DEBUG=1` prints every host command.

use crate::cell_audio::CellAudioParams;
use crate::game_audio::{AudioLibrary, Bus, GameAudio};
use crate::pd::engine::{Engine, HostCmd, InstId, VoiceId};
use crate::pd::library::{Instrument, PdLibrary, Spatial};
use crate::pd::voice::{Envelope, Pcm, PdVoiceAsset, VoiceCtl};
use crate::pd::{fnv, Atom, Sym};
use crate::stages::GameStage;
use crate::Originals;
use bevy::audio::{AddAudioSource, AudioPlayer, PlaybackSettings};
use bevy::prelude::*;
use rand::Rng;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

/// `OPENSPORE_PD_MUSIC=1`: the Pd engine plays the Cell, Creature and Tribe
/// music instead of the hand-ported / approximated scores.
/// The engine runs by default: the Cell stage plays the retail patches (the
/// headless trace of `cellgame_seed_music6` matches the original's
/// behaviour). `OPENSPORE_PD_MUSIC=0` turns it off (the hand-ported Cell score
/// of `cell_audio` plays then).
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_PD_MUSIC").map_or(true, |v| !v.is_empty() && v != "0"))
}

/// The Creature and Tribe patches: on by default since their inputs are fed
/// from the stage state at the retail moments (`pd_audio::feeds`,
/// `notes\pd-music-re-notes.md` 6); `OPENSPORE_PD_MUSIC=0` turns them off
/// with the rest of the engine.
pub fn all_stages() -> bool {
    enabled()
}

/// The Civilization mode: on by default since the city music is fed
/// (`civ_music`: the player's anthem through `music_city_planner`, the rival
/// cities' anthem samples; `notes\civ-polish-re.md` M1-M6);
/// `OPENSPORE_PD_CIV=0` brings back the director's Civ score.
pub fn civ_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    enabled() && *ON.get_or_init(|| std::env::var("OPENSPORE_PD_CIV").map_or(true, |v| !v.is_empty() && v != "0"))
}

/// The editors' music: on by default since every editor's events are fed
/// (`feeds::feed_editor`: mode, palette, parts, skin / block paint, swatch,
/// colour; `notes\audio-sweep-notes.md` E1) and every editor patch plays
/// with them (`--pd-trace` runs in the notes); `OPENSPORE_PD_EDITORS=0`
/// brings back the director's creator score.
pub fn editors_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    enabled() && *ON.get_or_init(|| std::env::var("OPENSPORE_PD_EDITORS").map_or(true, |v| !v.is_empty() && v != "0"))
}

/// The editor setup's `editorBackgroundMusicPatch` (0xDA51EC17): the patch
/// sound the editor starts when it opens (VERIFIED 0x58EEC6 -> 0x436390).
pub fn editor_music_patch(index: &crate::asset_catalog::ResourceIndex, setup: &str) -> Option<u32> {
    let key = crate::cell_editor_data::prop_key(crate::cell_editor_data::EDITOR_SETUP_GROUP, crate::world_objects::name_id(setup));
    let p = index.properties(key).ok()?;
    let v = p.0.get(&0xDA51_EC17)?.values.first()?.clone();
    Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?))
}

/// Whether the engine plays this stage's patches.
pub fn stage_uses_pd(stage: &GameStage) -> bool {
    match stage {
        // The galaxy screen's mix mode (mixmode_gge: galaxy_loop, gge_spin).
        GameStage::MainMenu | GameStage::Cell => enabled(),
        GameStage::Creature | GameStage::Tribal => all_stages(),
        // mixmode_spacegame + the galaxy / solar / planet contexts.
        GameStage::Space => enabled(),
        GameStage::Civilization => civ_enabled(),
    }
}

/// Plain (non-patch) mode startsounds the engine does not start itself:
/// their soundProps scale the gain with the camera altitude (`curve1` ...),
/// which a plain voice does not apply. `amb_trg_highalt_wind` is played with
/// its curve by `tribal_audio`; the two Creature wind beds are silent at the
/// gameplay camera heights (their curves start at altitude 505 / skydive).
pub const CURVE_DRIVEN_BEDS: [&str; 3] = ["amb_crg_highalt_wind", "amb_crg_planetzoom_wind", "amb_trg_highalt_wind"];

fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OPENSPORE_PD_DEBUG").is_some())
}

/// Whether the Pd engine plays this stage's music and mode sounds.
pub fn replaces_score(stage: &GameStage, creator_open: bool) -> bool {
    if creator_open {
        return editors_enabled();
    }
    stage_uses_pd(stage)
}

/// The music director's plan with the parts the mix mode plays itself
/// removed: the score (Pd patches) and, for Creature / Tribe, the ambience
/// beds (the mode's own startsounds: `ambience_dawn_dusk`, wind and rain
/// patches, `planetambiloop`, ...).
pub fn adjust_plan(stage: &GameStage, creator_open: bool, plan: &mut crate::game_audio::MusicPlan) {
    if !replaces_score(stage, creator_open) {
        return;
    }
    plan.music = None;
    if creator_open || matches!(stage, GameStage::Creature | GameStage::Tribal | GameStage::Civilization) {
        plan.bed = None;
        plan.bed2 = None;
    }
}

/// `sendgame <name>` from a patch (Tribe social rhythm: `trg_social_beat`,
/// `trg_round_length`, ...).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct PdSendGame {
    pub name: String,
    pub value: f32,
}

/// The Cell game's code-started patch sounds (cCellGFX::StartDisplay).
pub const CELL_SOUNDS: [&str; 3] = ["cell_motion", "cellgame_seed_music", "cell_amb"];

/// The game parameters 0xE73F60 sets every Cell frame (names of the patch
/// `gfparam`s).
pub fn cell_params(p: &CellAudioParams) -> [(&'static str, f32); 9] {
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    [
        ("cell_level", p.cell_level),
        ("cell_health", p.cell_health),
        ("flow_field", p.flow_field),
        ("cell_chase", p.cell_chase),
        ("cell_speed", p.cell_speed),
        ("cell_chase_loop", p.cell_chase_loop),
        ("cell_game_mate", b(p.cell_game_mate)),
        ("cell_game_evolve", b(p.cell_game_evolve)),
        ("cell_size", p.cell_size),
    ]
}

/// The bus a patch's voices mix into (OpenSpore mix levels).
pub fn patch_bus(name: &str) -> Bus {
    let n = name.to_lowercase();
    if n.contains("music") || n.contains("drums") {
        Bus::Music
    } else if n.contains("amb") || n.contains("motion") || n.contains("wind") || n.contains("rain") {
        Bus::Ambience
    } else {
        Bus::Sfx
    }
}

struct Running {
    inst: InstId,
    sound: u32,
    label: String,
    context: &'static str,
    groups: Vec<u32>,
    fade_out: f32,
    bus: Bus,
    /// Roll-off of the instance's positional voices (`is3d 1` + `x y z`).
    spatial: Option<Arc<Spatial>>,
    /// The instance's place in the world (`PlayAudio3D`), for voices that
    /// send no `is3d` / `x y z` of their own.
    position: Option<Vec3>,
}

/// A sound a voice plays, resolved from its name.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub sample: u32,
    pub gain: f32,
    pub pitch: f32,
    pub looped: bool,
    pub fade_in: f32,
    pub fade_out: f32,
    pub env: Envelope,
    pub groups: Vec<u32>,
}

/// Resolve the sound of a `Start`: an instrument note (`notenumber`,
/// `notevelocity`, `noteduration`), else a soundProp or SNR sample.
pub fn resolve_voice(
    lib: &PdLibrary,
    audio: &AudioLibrary,
    sound: &str,
    params: &HashMap<String, Atom>,
    instruments: &mut HashMap<String, Option<Instrument>>,
    rng: &mut impl Rng,
) -> Option<Resolved> {
    let p = |k: &str| params.get(k).map(|a| a.float());
    let inst = instruments.entry(sound.to_lowercase()).or_insert_with(|| lib.instrument(sound)).clone();
    if let Some(inst) = inst {
        let note = p("notenumber").unwrap_or(60.0);
        let vel = p("notevelocity").unwrap_or(100.0);
        let z = inst.zone(note, vel)?;
        if !lib.contains(crate::game_audio::TYPE_SNR, z.sample) {
            return None;
        }
        let dur = p("noteduration").filter(|d| *d > 0.0).map(|d| d / 1000.0);
        return Some(Resolved {
            sample: z.sample,
            gain: (vel / 127.0).clamp(0.0, 1.0),
            pitch: 2f32.powf((note - z.root) / 12.0),
            looped: false,
            fade_in: z.attack,
            fade_out: z.release,
            env: Envelope { attack: z.attack, hold: z.hold, decay: z.decay, sustain: z.sustain, release: z.release, note: dur },
            groups: vec![],
        });
    }
    let def = audio.resolve(sound).ok()?;
    let sample = def.samples[rng.random_range(0..def.samples.len())];
    Some(Resolved {
        sample,
        gain: def.gain,
        pitch: def.pitch,
        // A patch may override the soundProp's loop flag (`islooped 1`).
        looped: p("islooped").map_or(def.looped, |v| v != 0.0),
        fade_in: def.fade_in,
        fade_out: def.fade_out,
        env: Envelope::fade_in(def.fade_in),
        groups: lib.sound_groups(sound.strip_prefix('#').and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| fnv(sound))),
    })
}

struct Voice {
    /// `u32::MAX` for a plain mode startsound (no patch instance).
    inst: InstId,
    plain_bus: Option<Bus>,
    sound: Sym,
    params: HashMap<String, Atom>,
    resolved: Option<Resolved>,
    ctl: Arc<VoiceCtl>,
    entity: Option<Entity>,
    asked: f64,
    stop_fade: Option<f32>,
    stopped: bool,
    paused: bool,
    /// Headless: when the voice ends by itself.
    ends_at: Option<f64>,
    started: bool,
    /// A positional plain voice's roll-off (patch voices use their instance's).
    spatial: Option<Arc<Spatial>>,
}

enum PcmState {
    Pending,
    Ready(Arc<Pcm>, f64),
    Failed,
}

/// The engine, its running patches and their voices.
#[derive(Resource)]
pub struct PdMusic {
    pub engine: Engine,
    library: Option<Arc<PdLibrary>>,
    audio: Option<Arc<AudioLibrary>>,
    running: Vec<Running>,
    voices: HashMap<VoiceId, Voice>,
    mixers: HashMap<u32, HashMap<String, f32>>,
    pcm: HashMap<u32, PcmState>,
    jobs: Option<mpsc::Sender<u32>>,
    results: Option<Mutex<mpsc::Receiver<(u32, Result<Arc<Pcm>, String>)>>>,
    instruments: HashMap<String, Option<Instrument>>,
    teardown: HashMap<InstId, f32>,
    contexts: HashSet<&'static str>,
    /// Plain (non-patch) mode startsounds: voice and context.
    plain: Vec<(VoiceId, &'static str)>,
    next_plain: VoiceId,
    pause_groups: HashSet<u32>,
    /// The editor patch the `editor` context runs (by setup).
    editor_patch: Option<u32>,
    editor_patches: HashMap<String, Option<u32>>,
    clock: f64,
    playback: bool,
    /// Last lines of the host log (diagnostics, F3 overlays).
    pub log: VecDeque<String>,
    /// The audio listener (the camera) for positional voices; `None` = no
    /// distance roll-off (set by the stages that feed positions: Civ).
    pub listener: Option<Vec3>,
    /// The camera for the voices started with [`PdMusic::start_positional_voice`]
    /// (the Tribe positional music): they roll off from here while the other
    /// stages' positional voices keep using [`Self::listener`].
    pub ears: Option<Vec3>,
    /// The game clock is paused (`mixmode_simulationpause` is added, see
    /// [`feed_simulation_pause`]): its mix mode's pause groups are paused.
    pub simulation_paused: bool,
}

impl Default for PdMusic {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
            .unwrap_or(1_489_853_723);
        PdMusic {
            engine: Engine::new(seed),
            library: None,
            audio: None,
            running: vec![],
            voices: HashMap::new(),
            mixers: HashMap::new(),
            pcm: HashMap::new(),
            jobs: None,
            results: None,
            instruments: HashMap::new(),
            teardown: HashMap::new(),
            contexts: HashSet::new(),
            plain: vec![],
            next_plain: 0x8000_0000,
            pause_groups: HashSet::new(),
            editor_patch: None,
            editor_patches: HashMap::new(),
            clock: 0.0,
            playback: false,
            log: VecDeque::new(),
            listener: None,
            ears: None,
            simulation_paused: false,
        }
    }
}

impl PdMusic {
    /// `0x5CA970(FNV(name), value)`: set a global audio parameter.
    pub fn set_audio_param(&mut self, name: &str, value: f32) {
        self.engine.set_global_param(name, value);
    }

    /// The same by id (cinematic `mixEvent`, ids from the executable).
    pub fn set_audio_param_hash(&mut self, hash: u32, value: f32) {
        self.engine.set_global_param_hash(hash, value);
    }

    /// The patch sound the open editor runs (its setup's
    /// `editorBackgroundMusicPatch`).
    pub fn editor_patch_name(&self) -> Option<u32> {
        self.editor_patch
    }

    /// Whether a mix-mode context (`cell`, `creature`, `tribe`, `gge`,
    /// `global`) is running.
    pub fn context_active(&self, context: &str) -> bool {
        self.contexts.contains(context)
    }

    /// Tests: mark a mix-mode context running without the installed data.
    #[cfg(test)]
    pub fn force_context(&mut self, context: &'static str) {
        self.contexts.insert(context);
    }

    /// Set an instance parameter (`fparam <name>`) of a running patch sound
    /// (the owner of the sound instance sets them: `startstop`, `speed` ...).
    pub fn set_sound_param(&mut self, sound: &str, name: &str, value: f32) {
        self.set_sound_param_id(fnv(sound), name, value);
    }

    /// The same by sound id.
    pub fn set_sound_param_id(&mut self, id: u32, name: &str, value: f32) {
        let insts: Vec<InstId> = self.running.iter().filter(|r| r.sound == id).map(|r| r.inst).collect();
        for inst in insts {
            self.engine.set_instance_param(inst, name, value);
        }
    }

    /// Names of the running patch instances.
    pub fn running_patches(&self) -> Vec<String> {
        self.running.iter().map(|r| r.label.clone()).collect()
    }

    /// Product of the `gain` of a mixer group set by the patches (1 when
    /// no patch set it).
    pub fn mixer_gain(&self, group: &str) -> f32 {
        self.mixers.get(&fnv(group)).and_then(|m| m.get("gain")).copied().unwrap_or(1.0)
    }

    /// The level the patches' mixers give a sound in `groups` (ids of the
    /// group names, `SoundDef::groups`): the product of the groups' `gain`
    /// (1 without a mixer value). Plain named sounds (clip events, effect
    /// sounds, collisions, loops) use it as the Pd voices do, so
    /// `mixer chase_group` ducks the Cell's effect / voice / collision
    /// sounds and the transition patches' `sfx_group` / `ui_group` /
    /// `ambience_group` fades reach them too.
    pub fn group_gain(&self, groups: &[u32]) -> f32 {
        self.mixer_value(groups, "gain").unwrap_or(1.0).max(0.0)
    }

    fn note(&mut self, line: String) {
        if debug() {
            eprintln!("pd: {line}");
        }
        if self.log.len() >= 200 {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// Start a patch sound (soundProp or bundle name) in a context.
    pub fn start_sound(&mut self, name_id: u32, label: &str, context: &'static str) -> Option<InstId> {
        if self.running.iter().any(|r| r.sound == name_id) {
            return None;
        }
        let lib = self.library.clone()?;
        let patch = lib.patch_sound(name_id)?;
        let bundle = lib.bundle(patch.bundle)?;
        let resolver = lib.resolver(&bundle);
        let inst = self.engine.create_instance(&bundle.name, &bundle.top, &resolver, &[("gain", patch.gain), ("pitch", patch.pitch)]);
        let bus = patch_bus(label);
        let spatial = lib.spatial(name_id).map(Arc::new);
        self.note(format!("start patch {label} -> {} (instance {inst}, gain {:.2}, groups {:?})", bundle.name, patch.gain, patch.groups));
        self.running.push(Running { inst, sound: name_id, label: label.to_string(), context, groups: patch.groups, fade_out: patch.fade_out, bus, spatial, position: None });
        Some(inst)
    }

    /// Stop one running patch sound (`StopAudio(track)`), with its fade.
    pub fn stop_sound(&mut self, name_id: u32) {
        let (gone, keep): (Vec<Running>, Vec<Running>) = std::mem::take(&mut self.running).into_iter().partition(|r| r.sound == name_id);
        self.running = keep;
        for r in gone {
            self.note(format!("stop patch {} (fade {:.1} s)", r.label, r.fade_out));
            self.teardown.insert(r.inst, r.fade_out);
            self.engine.destroy_instance(r.inst);
        }
    }

    /// Place a running patch sound in the world (`PlayAudio3D(sound, pos)`):
    /// its voices that send no `is3d` / `x y z` of their own roll off from
    /// there with the sound's 0x03F51892 curve (INFERRED: the track position
    /// applies to the track's voices).
    pub fn set_sound_position(&mut self, name_id: u32, pos: Vec3) {
        for r in self.running.iter_mut().filter(|r| r.sound == name_id) {
            r.position = Some(pos);
        }
    }

    /// Start a sound in a context as the mix modes do: a patch sound as an
    /// instance, any other sound as a plain voice (effect `sound`
    /// components such as the planet's ambient sound effect).
    pub fn start_mode_sound(&mut self, id: u32, label: &str, context: &'static str) {
        let Some(lib) = self.library.clone() else { return };
        if let Some(p) = lib.patch_sound(id) {
            let label = lib.bundle(p.bundle).map(|b| b.name.clone()).unwrap_or_else(|| label.to_string());
            self.start_sound(id, &label, context);
        } else {
            self.start_plain(id, context);
        }
    }

    /// A mode startsound that is a plain sound (an ambience loop): played
    /// directly, without a patch.
    fn start_plain(&mut self, id: u32, context: &'static str) {
        let (Some(lib), Some(audio)) = (self.library.clone(), self.audio.clone()) else { return };
        let sound = format!("#{id:08X}");
        let mut rng = rand::rng();
        let Some(res) = resolve_voice(&lib, &audio, &sound, &HashMap::new(), &mut self.instruments, &mut rng) else {
            self.note(format!("mode sound {sound}: not installed"));
            return;
        };
        if !self.pcm.contains_key(&res.sample) {
            if let Some(jobs) = &self.jobs {
                if jobs.send(res.sample).is_ok() {
                    self.pcm.insert(res.sample, PcmState::Pending);
                }
            }
        }
        let voice = self.next_plain;
        self.next_plain = self.next_plain.wrapping_add(1).max(0x8000_0000);
        self.note(format!("mode sound {sound} (looped {}) in {context}", res.looped));
        self.voices.insert(
            voice,
            Voice {
                inst: u32::MAX,
                plain_bus: Some(Bus::Ambience),
                sound: crate::pd::sym(&sound),
                params: HashMap::new(),
                resolved: Some(res),
                ctl: Arc::new(VoiceCtl::default()),
                entity: None,
                asked: self.clock,
                stop_fade: None,
                stopped: false,
                paused: false,
                ends_at: None,
                started: false,
                spatial: None,
            },
        );
        self.plain.push((voice, context));
    }

    /// A positional plain sound of a context (a rival city's anthem sample,
    /// retail `PlayProceduralAudio(sample, position)`): `is3d 1` at `pos`
    /// with the sound's own roll-off. Returns the voice to stop it later.
    pub fn start_spatial_plain(&mut self, id: u32, context: &'static str, pos: Vec3) -> Option<VoiceId> {
        let before = self.next_plain;
        self.start_plain(id, context);
        if self.next_plain == before {
            return None;
        }
        let spatial = self.library.as_ref().and_then(|l| l.spatial(id)).map(Arc::new);
        let v = self.voices.get_mut(&before)?;
        v.params.insert("is3d".into(), Atom::F(1.0));
        v.params.insert("x".into(), Atom::F(pos.x));
        v.params.insert("y".into(), Atom::F(pos.y));
        v.params.insert("z".into(), Atom::F(pos.z));
        v.spatial = spatial;
        Some(before)
    }

    /// Stop a plain voice started by [`Self::start_spatial_plain`].
    pub fn stop_voice(&mut self, voice: VoiceId) {
        self.plain.retain(|(v, _)| *v != voice);
        if let Some(v) = self.voices.get_mut(&voice) {
            v.stopped = true;
        }
    }

    /// A moving positional plain sound on `bus` that rolls off from [`Self::ears`]
    /// (retail `Audio::PlayProceduralAudio(sound, position)` + `SetPosition`
    /// every update, 0xBC23B0 / 0xBC2540): the sound's own loop flag, gain,
    /// fade-out and 0x03F51892 distances, [`Self::move_voice`] moves it,
    /// [`Self::stop_voice`] ends it with the sound's `fadeout`. `None` when the
    /// sound is not installed or its `polyphony` limit (most simultaneous
    /// voices of the sound) is reached: ADAPTED, the retail polyphony rule
    /// (oldest voice kept, newer ones refused) was not traced.
    pub fn start_positional_voice(&mut self, id: u32, context: &'static str, pos: Vec3, bus: Bus) -> Option<VoiceId> {
        let sound = format!("#{id:08X}");
        let limit = self.audio.as_ref().and_then(|a| a.resolve(&sound).ok()).map_or(0, |d| d.polyphony as usize);
        if limit > 0 {
            let name = crate::pd::sym(&sound);
            let live = self.voices.values().filter(|v| v.plain_bus.is_some() && !v.stopped && v.sound == name).count();
            if live >= limit {
                return None;
            }
        }
        let voice = self.start_spatial_plain(id, context, pos)?;
        let v = self.voices.get_mut(&voice)?;
        v.plain_bus = Some(bus);
        v.params.insert("ears".into(), Atom::F(1.0));
        Some(voice)
    }

    /// Move a voice started by [`Self::start_spatial_plain`] /
    /// [`Self::start_positional_voice`] (retail 0x9FBB90).
    pub fn move_voice(&mut self, voice: VoiceId, pos: Vec3) {
        if let Some(v) = self.voices.get_mut(&voice) {
            v.params.insert("x".into(), Atom::F(pos.x));
            v.params.insert("y".into(), Atom::F(pos.y));
            v.params.insert("z".into(), Atom::F(pos.z));
        }
    }

    /// Whether a plain voice is still playing (it ends by itself when its
    /// sample cannot be decoded).
    pub fn voice_alive(&self, voice: VoiceId) -> bool {
        self.voices.get(&voice).is_some_and(|v| !v.stopped)
    }

    /// The patch instance of a running sound.
    pub fn instance_of(&self, sound: u32) -> Option<InstId> {
        self.running.iter().find(|r| r.sound == sound).map(|r| r.inst)
    }

    fn stop_context(&mut self, context: &'static str) {
        self.stop_context_keeping(context, &HashMap::new());
    }

    /// Stop a context; a patch sound that a context about to run also starts
    /// keeps running and moves to it (`space_music_patch` is a startsound of
    /// both the galaxy and the solar context: switching views does not
    /// restart it). INFERRED: the mix mode switch only stops sounds the new
    /// modes do not start.
    fn stop_context_keeping(&mut self, context: &'static str, keep_in: &HashMap<u32, &'static str>) {
        let plain: Vec<VoiceId> = self.plain.iter().filter(|(_, c)| *c == context).map(|(v, _)| *v).collect();
        self.plain.retain(|(_, c)| *c != context);
        for v in plain {
            if let Some(v) = self.voices.get_mut(&v) {
                v.stopped = true;
            }
        }
        for r in self.running.iter_mut().filter(|r| r.context == context) {
            if let Some(next) = keep_in.get(&r.sound) {
                r.context = next;
            }
        }
        let (gone, keep): (Vec<Running>, Vec<Running>) = std::mem::take(&mut self.running).into_iter().partition(|r| r.context == context);
        self.running = keep;
        for r in gone {
            self.note(format!("stop patch {} (fade {:.1} s)", r.label, r.fade_out));
            self.teardown.insert(r.inst, r.fade_out);
            self.engine.destroy_instance(r.inst);
        }
    }

    fn start_context(&mut self, context: &'static str) {
        let Some(lib) = self.library.clone() else { return };
        if context == "cell" {
            for name in CELL_SOUNDS {
                self.start_sound(fnv(name), name, context);
            }
        }
        if context == "editor" {
            if let Some(id) = self.editor_patch {
                self.start_mode_sound(id, "editor_music", context);
            }
        }
        if let Some(mode) = mode_of(context) {
            self.engine.set_global_param(mode, 1.0);
            if let Some(def) = lib.mode(mode) {
                for id in def.startsounds {
                    if let Some(p) = lib.patch_sound(id) {
                        let label = lib.bundle(p.bundle).map(|b| b.name.clone()).unwrap_or_else(|| format!("#{id:08X}"));
                        self.start_sound(id, &label, context);
                    } else if context != "global" && context != "cell" && !CURVE_DRIVEN_BEDS.iter().any(|n| fnv(n) == id) {
                        self.start_plain(id, context);
                    }
                }
            }
        }
    }

    fn running_of(&self, inst: InstId) -> Option<&Running> {
        self.running.iter().find(|r| r.inst == inst)
    }

    fn mixer_value(&self, groups: &[u32], name: &str) -> Option<f32> {
        let mut v: Option<f32> = None;
        for g in groups {
            if let Some(x) = self.mixers.get(g).and_then(|m| m.get(name)) {
                v = Some(match (name, v) {
                    ("gain", Some(a)) => a * x,
                    ("lowpass", Some(a)) => a.min(*x),
                    (_, Some(a)) => a.max(*x),
                    (_, None) => *x,
                });
            }
        }
        v
    }

    /// Apply one host command.
    fn host(&mut self, cmd: HostCmd, now: f64) {
        match cmd {
            HostCmd::Start { voice, inst, sound, params } => {
                let params: HashMap<String, Atom> = params.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                let (Some(lib), Some(audio)) = (self.library.clone(), self.audio.clone()) else {
                    self.engine.voice_ended(voice);
                    return;
                };
                let mut rng = rand::rng();
                let resolved = resolve_voice(&lib, &audio, &sound, &params, &mut self.instruments, &mut rng);
                let Some(res) = resolved else {
                    self.note(format!("voice {voice}: {sound} not installed (silent)"));
                    self.engine.voice_ended(voice);
                    return;
                };
                if !self.pcm.contains_key(&res.sample) {
                    if let Some(jobs) = &self.jobs {
                        if jobs.send(res.sample).is_ok() {
                            self.pcm.insert(res.sample, PcmState::Pending);
                        }
                    }
                }
                let seek = params.get("seektime").map(|a| a.float());
                self.note(format!("voice {voice} start {sound} (inst {inst}, t {now:.2}, seek {seek:?})"));
                self.voices.insert(
                    voice,
                    Voice {
                        inst,
                        plain_bus: None,
                        sound,
                        params,
                        resolved: Some(res),
                        ctl: Arc::new(VoiceCtl::default()),
                        entity: None,
                        asked: now,
                        stop_fade: None,
                        stopped: false,
                        paused: false,
                        ends_at: None,
                        started: false,
                        spatial: None,
                    },
                );
            }
            HostCmd::Stop { voice } => {
                let fade = self.voices.get(&voice).and_then(|v| self.teardown.get(&v.inst).copied());
                if let Some(v) = self.voices.get_mut(&voice) {
                    v.stopped = true;
                    if let Some(f) = fade.filter(|f| *f > 0.0) {
                        v.stop_fade = Some(f);
                    }
                }
            }
            HostCmd::Param { voice, name, value } => {
                if let Some(v) = self.voices.get_mut(&voice) {
                    if &*name == "seektime" {
                        if let (Some(pcm), true) = (v.resolved.as_ref().and_then(|r| match self.pcm.get(&r.sample) {
                            Some(PcmState::Ready(p, _)) => Some(p.clone()),
                            _ => None,
                        }), v.started)
                        {
                            v.ctl.seek_frames((value.float().max(0.0) * pcm.rate as f32) as u64);
                        }
                    }
                    v.params.insert(name.to_string(), value);
                }
            }
            HostCmd::Pause { voice, paused } => {
                if let Some(v) = self.voices.get_mut(&voice) {
                    v.paused = paused;
                }
            }
            HostCmd::Mixer { group, name, value, .. } => {
                self.mixers.entry(fnv(&group)).or_default().insert(name.to_string(), value.float());
            }
            HostCmd::SendGame { name, value, .. } => {
                self.note(format!("sendgame {name} {value}"));
            }
            HostCmd::Print { text, .. } => self.note(text),
        }
    }
}

fn mode_of(context: &str) -> Option<&'static str> {
    Some(match context {
        "global" => "mixmode_global",
        "cell" => "mixmode_cellgame",
        "creature" => "mixmode_creaturegame",
        "tribe" => "mixmode_tribegame",
        "gge" => "mixmode_gge",
        "civ" => "mixmode_civgame",
        "space" => "mixmode_spacegame",
        "galaxy" => "mixmode_galaxycontext",
        "solar" => "mixmode_solarcontext",
        "planet" => "mixmode_planetcontext",
        "trading" => "mixmode_spacetrading",
        "timeline" => "mixmode_timeline",
        "editor" => "mixmode_editor",
        _ => return None,
    })
}

pub struct PdAudioPlugin;

impl Plugin for PdAudioPlugin {
    fn build(&self, app: &mut App) {
        let playback = app.is_plugin_added::<bevy::audio::AudioPlugin>();
        if playback {
            app.add_audio_source::<PdVoiceAsset>();
        }
        app.insert_resource(PdMusic { playback, ..Default::default() })
            .init_resource::<crate::tribal_audio::RaidMusic>()
            .add_message::<PdSendGame>()
            .add_message::<crate::cinematics::AudioParamSet>()
            .add_systems(
                Update,
                (
                    setup,
                    feed_simulation_pause,
                    contexts,
                    feed_cell,
                    feed_stage_params,
                    feeds::feed_creature,
                    feeds::feed_tribe,
                    crate::tribal_audio::feed_raid_music,
                    feeds::feed_space,
                    feeds::feed_ufo,
                    feeds::feed_civ,
                    crate::civ_music::feed_city_music,
                    crate::civ_music::feed_recruit_music,
                    feeds::feed_editor,
                    tick,
                )
                    .chain()
                    .run_if(|| enabled()),
            );
    }
}

pub mod feeds;

/// Feed the cinematic `mixEvent`s (`cinematics::AudioParamSet`, the retail
/// 0x5CA970 call) into the engine. The stage inputs are fed by [`feeds`].
fn feed_stage_params(mut pd: ResMut<PdMusic>, mut mix: MessageReader<crate::cinematics::AudioParamSet>) {
    for set in mix.read() {
        pd.set_audio_param_hash(set.hash, set.value);
    }
}

/// `mixmode_simulationpause` (VERIFIED, march2017): the game time manager's
/// `UpdateFlags` 0xB320B0 posts message 0x3867294 with the new "game clock
/// paused" bit (flags & 1) whenever it changes (0xB3215E; every `Pause` request
/// of the pause menu, the cinematics' `pauseGame`, the comm screens, the launch
/// screen goes through it); the audio mix mode manager's message handler
/// (0xEA9DA0, case 0xEAA043) adds the mix mode 0xA1638DF0 = FNV
/// `mixmode_simulationpause` (0xEA9A10, its parameter 1.0) when the message
/// carries true and removes it (0xEA9A50, parameter 0.0) when it carries false.
/// The ambience patches read the parameter; the mode's pause groups
/// (`simulationpause_group`, `gamepause_group`, `ufo_group`) pause with it
/// (`contexts`). Sent on change only, like retail.
fn feed_simulation_pause(
    mut pd: ResMut<PdMusic>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    flow: Option<Res<crate::creature_flow::CreatureFlow>>,
    civ_comm: Option<Res<crate::civ_comm::CivComm>>,
    launch: Option<Res<crate::front_end::launch_screen::LaunchScreen>>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    mut last: Local<bool>,
) {
    let paused = simulation_paused(
        pause.is_some_and(|p| p.open),
        flow.is_some_and(|f| f.holding()),
        civ_comm.is_some_and(|c| c.open),
        launch.is_some_and(|l| l.holds_game()),
        cinematics.is_some_and(|c| c.is_playing() && c.pause_mode() != crate::cinematics::PauseMode::None),
    );
    pd.simulation_paused = paused;
    if paused != *last {
        *last = paused;
        pd.set_audio_param(SIMULATION_PAUSE, if paused { 1.0 } else { 0.0 });
    }
}

/// The mix mode the game time manager's pause message switches.
pub const SIMULATION_PAUSE: &str = "mixmode_simulationpause";

/// The game clock is paused (`cGameTimeManager` flags & 1): OpenSpore's pause
/// requests are the pause menu, a held Creature moment (`pauseGame`), the Civ
/// comm screen (`ShowCommEvent` 0xAEBE10), the stage launch screen (0xE3E350)
/// and a cinematic's `pauseGame` (the same gates as `pause_menu::simulation_active`
/// and `cinematics::simulation_active`).
pub fn simulation_paused(menu: bool, flow_holding: bool, civ_comm: bool, launch: bool, cinematic_pause: bool) -> bool {
    menu || flow_holding || civ_comm || launch || cinematic_pause
}

/// Index the installed Pd data once the audio library exists.
fn setup(mut pd: ResMut<PdMusic>, audio: Res<GameAudio>, originals: Option<Res<Originals>>) {
    if pd.library.is_some() {
        return;
    }
    let (Some(library), Some(originals)) = (audio.library(), originals) else { return };
    let lib = PdLibrary::from_index(&originals.resource_index);
    info!(
        "pd engine: {} pdr bundles, {} pd patches, {} modes, {} instruments",
        lib.count(crate::pd::library::TYPE_PDR),
        lib.count(crate::pd::library::TYPE_PD),
        lib.count(crate::pd::library::TYPE_MODE),
        lib.count(crate::pd::library::TYPE_INSTRUMENT)
    );
    let (done, results) = mpsc::channel();
    let (jobs, queue) = mpsc::channel::<u32>();
    let worker = library.clone();
    let spawned = std::thread::Builder::new()
        .name("pd-audio-decode".into())
        .spawn(move || {
            let mut rng = rand::rng();
            for sample in queue {
                let r = worker
                    .decode(sample, None, &mut rng)
                    .map(|s| Arc::new(Pcm { samples: s.samples, channels: s.channels, rate: s.rate }));
                if done.send((sample, r)).is_err() {
                    break;
                }
            }
        })
        .is_ok();
    if spawned {
        pd.jobs = Some(jobs);
        pd.results = Some(Mutex::new(results));
    }
    pd.engine.data_files = lib.data_files();
    pd.library = Some(Arc::new(lib));
    pd.audio = Some(library);
}

/// Start / stop the patch contexts of the game state; pause groups.
fn contexts(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    editor: Option<Res<crate::editor::EditorState>>,
    flow: Option<Res<crate::cell_flow::CellFlow>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    creature_flow: Option<Res<crate::creature_flow::CreatureFlow>>,
    space_view: Option<Res<crate::space_view::SpaceView>>,
    comm: Option<Res<crate::space_comm::CommSession>>,
    editors: (
        Option<Res<crate::creature_editor::CreatureEditor>>,
        Option<Res<crate::rigid_editor::RigidEditor>>,
        Option<Res<crate::outfitter::Outfitter>>,
        Option<Res<Originals>>,
    ),
) {
    if pd.library.is_none() {
        return;
    }
    // The stage-end History window runs the timeline mode (`timeline`,
    // `timeline_scrub`; it pauses `timelinepause_group`).
    let timeline = flow.as_ref().is_some_and(|f| f.summary.is_some()) || creature_flow.as_ref().is_some_and(|f| f.summary.is_some());
    let cell_editing = flow.as_ref().is_some_and(|f| f.phase == crate::cell_flow::FlowPhase::Editing);
    let editing = editor.is_some_and(|e| e.open) || cell_editing;
    // The open editor's setup (its music patch is data: the setup's
    // `editorBackgroundMusicPatch`).
    let (creature_editor, rigid, outfitter, originals) = editors;
    let setup: Option<&'static str> = if cell_editing {
        Some("CellTemplate")
    } else if let Some(r) = rigid.as_ref().filter(|r| r.active) {
        Some(r.kind.setup_name())
    } else if let Some(o) = outfitter.as_ref().filter(|o| o.active) {
        Some(match o.stage {
            Some(GameStage::Civilization) => "CivAccessoriesEditorSetup",
            Some(GameStage::Space) => "SpaceAccessoriesEditorSetup",
            _ => "TribalAccessoriesEditorSetup",
        })
    } else if let Some(c) = creature_editor.as_ref().filter(|c| c.active) {
        Some(c.kind.setup_name())
    } else if editing {
        Some("CreatureEditorExtraLarge")
    } else {
        None
    };
    // Any editor session replaces the stage's mode (mixmode_editor).
    let editing = editing || setup.is_some();
    let mut want: HashSet<&'static str> = HashSet::new();
    if stage_uses_pd(stage.get()) {
        want.insert("global");
    }
    match stage.get() {
        GameStage::MainMenu if !editing => {
            want.insert("gge");
        }
        GameStage::Cell if !editing => {
            want.insert("cell");
        }
        GameStage::Creature if !editing && all_stages() => {
            want.insert("creature");
        }
        GameStage::Tribal if !editing && all_stages() => {
            want.insert("tribe");
        }
        GameStage::Civilization if !editing && civ_enabled() => {
            want.insert("civ");
        }
        GameStage::Space if !editing && enabled() => {
            want.insert("space");
            // The Space camera context switches with the view.
            if let Some(view) = &space_view {
                want.insert(match view.zoom() {
                    crate::space_view::SpaceZoom::Galaxy => "galaxy",
                    crate::space_view::SpaceZoom::System => "solar",
                    crate::space_view::SpaceZoom::Planet => "planet",
                });
            }
            if comm.as_ref().is_some_and(|c| c.open && c.trade.is_some()) {
                want.insert("trading");
            }
        }
        _ => {}
    }
    if timeline && !want.is_empty() {
        want.insert("timeline");
    }
    let mut have = pd.contexts.clone();
    if editors_enabled() {
        if let (Some(setup), Some(originals)) = (setup, originals.as_ref()) {
            want.insert("global");
            want.insert("editor");
            let patch = *pd
                .editor_patches
                .entry(setup.to_string())
                .or_insert_with(|| editor_music_patch(&originals.resource_index, setup));
            if pd.editor_patch != patch && have.contains("editor") {
                // Another editor (class) opened: its own patch.
                pd.stop_context("editor");
                have.remove("editor");
            }
            pd.editor_patch = patch;
        }
    }
    // Sounds the remaining and new contexts start (kept across a switch).
    let mut keep_in: HashMap<u32, &'static str> = HashMap::new();
    if let Some(lib) = pd.library.clone() {
        for c in &want {
            if let Some(def) = mode_of(c).and_then(|m| lib.mode(m)) {
                for id in def.startsounds {
                    keep_in.entry(id).or_insert(c);
                }
            }
        }
    }
    for c in have.difference(&want) {
        if *c == "trading" {
            // The trade ends: `fparam startstop 0` brings the city music
            // back if the trade loop played.
            pd.set_sound_param("music_space_trade", "startstop", 0.0);
        }
        pd.stop_context_keeping(c, &keep_in);
        if let Some(mode) = mode_of(c) {
            pd.engine.set_global_param(mode, 0.0);
        }
    }
    let mut order: Vec<&'static str> = want.difference(&have).copied().collect();
    order.sort_unstable_by_key(|c| (*c != "global", *c));
    for c in order {
        pd.start_context(c);
    }
    pd.contexts = want;
    // Pause groups: the user pause mode while the pause menu is open, the
    // timeline mode's while the History window is shown.
    let paused_menu = pause.is_some_and(|p| p.open);
    let mut groups: HashSet<u32> = HashSet::new();
    for (on, mode) in [
        (paused_menu, "mixmode_userpause"),
        (timeline, "mixmode_timeline"),
        (pd.simulation_paused, SIMULATION_PAUSE),
    ] {
        if on {
            groups.extend(pd.library.as_ref().and_then(|l| l.mode(mode)).map(|m| m.pausegroups).unwrap_or_default());
        }
    }
    if groups != pd.pause_groups {
        pd.pause_groups = groups.clone();
        let insts: Vec<(InstId, bool)> = pd.running.iter().map(|r| (r.inst, r.groups.iter().any(|g| groups.contains(g)))).collect();
        for (inst, p) in insts {
            pd.engine.pause_instance(inst, p);
        }
    }
}

/// 0xE73F60: the Cell audio parameters, every frame.
fn feed_cell(mut pd: ResMut<PdMusic>, stage: Res<State<GameStage>>, params: Option<ResMut<CellAudioParams>>) {
    if *stage.get() != GameStage::Cell || !pd.contexts.contains("cell") {
        return;
    }
    let Some(mut params) = params else { return };
    for (name, value) in cell_params(&params) {
        pd.set_audio_param(name, value);
    }
    // `sound2` animation events (cell_egg_lay): set once when they fire.
    for (hash, value) in std::mem::take(&mut params.triggers) {
        pd.set_audio_param_hash(hash, value);
    }
}

/// Run the scheduler and the voices.
fn tick(
    mut commands: Commands,
    mut pd: ResMut<PdMusic>,
    time: Res<Time<Real>>,
    audio: Res<GameAudio>,
    assets: Option<ResMut<Assets<PdVoiceAsset>>>,
    mut sendgame: MessageWriter<PdSendGame>,
) {
    if pd.library.is_none() {
        return;
    }
    let pd = &mut *pd;
    pd.clock += time.delta_secs_f64();
    let now = pd.clock;
    // Decoded samples.
    let ready: Vec<_> = pd.results.as_ref().and_then(|r| r.lock().ok().map(|r| r.try_iter().collect())).unwrap_or_default();
    for (sample, result) in ready {
        match result {
            Ok(pcm) => {
                pd.pcm.insert(sample, PcmState::Ready(pcm, now));
            }
            Err(e) => {
                pd.note(format!("sample {sample:08X}: {e}"));
                pd.pcm.insert(sample, PcmState::Failed);
            }
        }
    }
    pd.engine.tick(now);
    for cmd in pd.engine.take_commands() {
        if let HostCmd::SendGame { name, value, .. } = &cmd {
            sendgame.write(PdSendGame { name: name.to_string(), value: *value });
        }
        pd.host(cmd, now);
    }
    pd.teardown.retain(|inst, _| pd.engine.instance(*inst).is_none() && pd.voices.values().any(|v| v.inst == *inst));
    // Voices.
    let mute = if audio.muted { 0.0 } else { 1.0 };
    let mut assets = assets;
    let mut ended: Vec<VoiceId> = vec![];
    let ids: Vec<VoiceId> = pd.voices.keys().copied().collect();
    for id in ids {
        let (bus, inst_groups, inst_spatial, inst_position) = {
            let v = &pd.voices[&id];
            match (pd.running_of(v.inst), v.plain_bus) {
                (Some(r), _) => (r.bus, r.groups.clone(), r.spatial.clone(), r.position),
                (None, Some(b)) => (b, vec![], None, None),
                (None, None) => (patch_bus(&v.sound), vec![], None, None),
            }
        };
        let Some(v) = pd.voices.get(&id) else { continue };
        let Some(res) = v.resolved.clone() else {
            ended.push(id);
            continue;
        };
        let p = |k: &str| v.params.get(k).map(|a| a.float());
        let mut groups = res.groups.clone();
        groups.extend(inst_groups.iter().copied());
        let mixer_gain = pd.mixer_value(&groups, "gain").unwrap_or(1.0);
        let paused = v.paused || res.groups.iter().any(|g| pd.pause_groups.contains(g));
        // Positional voices (`is3d 1` + `x y z`) roll off with the distance to
        // the listener (the instance's / sound's 0x03F51892 curve).
        let ears = if p("ears").is_some() { pd.ears } else { pd.listener };
        let roll_off = match (ears, v.spatial.as_ref().or(inst_spatial.as_ref())) {
            (Some(listener), Some(s)) if p("is3d").unwrap_or(0.0) != 0.0 => match (p("x"), p("y"), p("z")) {
                (Some(x), Some(y), Some(z)) => s.gain(listener.distance(Vec3::new(x, y, z))),
                _ => 1.0,
            },
            // A placed instance (`set_sound_position`) for voices without
            // their own 3D setting.
            (Some(listener), Some(s)) if p("is3d").is_none() && inst_position.is_some() => {
                s.gain(listener.distance(inst_position.unwrap_or_default()))
            }
            _ => 1.0,
        };
        let gain = bus.volume() * res.gain * p("gain").unwrap_or(1.0).max(0.0) * mixer_gain.max(0.0) * mute * roll_off;
        let pitch = res.pitch * p("pitch").unwrap_or(1.0);
        let lowpass = match (p("lowpass").filter(|f| *f > 0.0), pd.mixer_value(&groups, "lowpass").filter(|f| *f > 0.0)) {
            (Some(a), Some(b)) => a.min(b),
            (a, b) => a.or(b).unwrap_or(0.0),
        };
        let hipass = p("hipass").unwrap_or(0.0).max(pd.mixer_value(&groups, "hipass").unwrap_or(0.0));
        let pan = p("pan").or_else(|| p("panangle").map(|a| a.to_radians().sin())).unwrap_or(0.0);
        let ring_wet = p("ringmodwetmix").or_else(|| pd.mixer_value(&groups, "ringmodwetmix")).unwrap_or(0.0);
        let ring_freq = p("ringmodfreq").or_else(|| pd.mixer_value(&groups, "ringmodfreq")).unwrap_or(0.0);
        let release = v.stop_fade.or_else(|| p("fadeout").filter(|f| *f > 0.0)).unwrap_or(res.fade_out);
        let ctl = v.ctl.clone();
        ctl.set_gain(gain);
        ctl.set_pitch(pitch.clamp(0.01, 8.0));
        ctl.set_lowpass(lowpass);
        ctl.set_hipass(hipass);
        ctl.set_pan(pan);
        ctl.set_ring_wet(ring_wet);
        ctl.set_ring_freq(ring_freq);
        ctl.set_release(release);
        ctl.set_paused(paused);
        let (started, stopped, asked) = (v.started, v.stopped, v.asked);
        if started {
            if stopped && !ctl.stopping() {
                ctl.stop();
            }
            let done = ctl.finished() || v.ends_at.is_some_and(|t| now >= t + if stopped { release as f64 } else { 0.0 }) || (stopped && v.ends_at.is_some() && !pd.playback);
            if done {
                ended.push(id);
            }
            continue;
        }
        if stopped {
            ended.push(id);
            continue;
        }
        // Not started yet: wait for the samples.
        let pcm = match pd.pcm.get(&res.sample) {
            Some(PcmState::Ready(pcm, _)) => pcm.clone(),
            Some(PcmState::Failed) | None => {
                ended.push(id);
                continue;
            }
            Some(PcmState::Pending) => {
                // A long one-shot (a streamed music stem: `solar_long` is 150 s) may take
                // longer to decode than a sound effect: it starts late
                // rather than ending at once (the patches restart a stem
                // whose voice ended).
                let long = pd.library.as_ref().is_some_and(|l| {
                    l.contains(crate::game_audio::TYPE_SNS, res.sample)
                        || l.record_size(crate::game_audio::TYPE_SNR, res.sample).is_some_and(|n| n > 400_000)
                });
                let patience = if res.looped || long { 20.0 } else { 1.0 };
                if now - asked > patience {
                    ended.push(id);
                }
                continue;
            }
        };
        let fade_in = p("fadein").filter(|f| *f > 0.0).unwrap_or(res.fade_in);
        let mut env = res.env;
        if env.note.is_none() {
            env.attack = fade_in;
        }
        let start_frame = (p("seektime").unwrap_or(0.0).max(0.0) * pcm.rate as f32) as usize;
        let mut entity = None;
        let mut ends_at = None;
        match (pd.playback, assets.as_mut()) {
            (true, Some(assets)) => {
                let handle = assets.add(PdVoiceAsset { pcm: pcm.clone(), ctl: ctl.clone(), looped: res.looped, start_frame, env });
                entity = Some(commands.spawn((Name::new(format!("pd voice {}", v.sound)), AudioPlayer(handle), PlaybackSettings::DESPAWN)).id());
            }
            _ => {
                // Headless: the voice ends after its length.
                if !res.looped {
                    let len = env.note.map(|n| n + env.release).unwrap_or(pcm.secs() / pitch.max(0.01));
                    ends_at = Some(now + len as f64);
                } else {
                    ends_at = Some(f64::INFINITY);
                }
            }
        }
        if let Some(v) = pd.voices.get_mut(&id) {
            v.started = true;
            v.entity = entity;
            v.ends_at = ends_at;
        }
    }
    for id in ended {
        if debug() {
            if let Some(v) = pd.voices.get(&id) {
                eprintln!("pd: voice {id} ended {} (t {now:.2}, started {}, stopped {})", v.sound, v.started, v.stopped);
            }
        }
        pd.plain.retain(|(v, _)| *v != id);
        if let Some(v) = pd.voices.remove(&id) {
            if let Some(e) = v.entity {
                if let Ok(mut ec) = commands.get_entity(e) {
                    if !v.ctl.finished() {
                        ec.despawn();
                    }
                }
            }
        }
        pd.engine.voice_ended(id);
    }
    // Forget decoded samples nobody played for two minutes.
    let in_use: HashSet<u32> = pd.voices.values().filter_map(|v| v.resolved.as_ref().map(|r| r.sample)).collect();
    pd.pcm.retain(|s, st| match st {
        PcmState::Ready(_, at) => in_use.contains(s) || now - *at < 120.0,
        _ => true,
    });
    for v in pd.voices.values() {
        if let Some(r) = &v.resolved {
            if let Some(PcmState::Ready(_, at)) = pd.pcm.get_mut(&r.sample) {
                *at = now;
            }
        }
    }
}

#[cfg(test)]
mod mixer_tests {
    use super::*;

    #[test]
    fn plain_sounds_follow_the_group_mixers() {
        let mut pd = PdMusic::default();
        let chase = fnv("chase_group");
        let sfx = fnv("sfx_group");
        // No patch has set a mixer: full level.
        assert_eq!(pd.group_gain(&[chase, sfx]), 1.0);
        assert_eq!(pd.group_gain(&[]), 1.0);
        pd.mixers.entry(chase).or_default().insert("gain".into(), 0.25);
        assert_eq!(pd.group_gain(&[chase]), 0.25);
        // A sound in two ducked groups gets the product (as a Pd voice does).
        pd.mixers.entry(sfx).or_default().insert("gain".into(), 0.5);
        assert!((pd.group_gain(&[chase, sfx]) - 0.125).abs() < 1e-6);
        // Other groups are untouched.
        assert_eq!(pd.group_gain(&[fnv("ui_group")]), 1.0);
    }

    #[test]
    fn the_simulation_pause_parameter_follows_the_pause_flag_on_change() {
        assert_eq!(fnv(SIMULATION_PAUSE), 0xA163_8DF0, "the mix mode id of 0xEAA04A");
        assert!(!simulation_paused(false, false, false, false, false));
        for k in 0..5 {
            let mut on = [false; 5];
            on[k] = true;
            assert!(simulation_paused(on[0], on[1], on[2], on[3], on[4]), "request {k}");
        }
        let mut app = App::new();
        app.insert_resource(PdMusic::default())
            .insert_resource(crate::pause_menu::PauseMenu::default())
            .add_systems(Update, feed_simulation_pause);
        let value = |app: &App| app.world().resource::<PdMusic>().engine.gvalues.get(&fnv(SIMULATION_PAUSE)).copied();
        app.update();
        assert_eq!(value(&app), None, "never set before a pause (retail sends on change only)");
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        app.update();
        assert_eq!(value(&app), Some(1.0));
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = false;
        app.update();
        assert_eq!(value(&app), Some(0.0));
    }
}

/// The Tribe positional music voices (`tribal_audio::feed_raid_music`, tribal-raidmusic).
#[cfg(test)]
mod raid_voice_tests {
    use super::*;
    use crate::tribal_audio::{PositionalMusic, RaidMusic, SourceKey, Want};

    fn installed_pd() -> Option<PdMusic> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut pd = PdMusic::default();
        pd.library = Some(Arc::new(PdLibrary::from_index(&index)));
        pd.audio = Some(Arc::new(AudioLibrary::from_index(&index)));
        Some(pd)
    }

    #[test]
    fn a_positional_voice_needs_the_installed_sounds() {
        // Without the library nothing starts and nothing panics (headless apps).
        let mut pd = PdMusic::default();
        let at = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(pd.start_positional_voice(PositionalMusic::Raid.id(), "tribe", at, Bus::Music), None);
        pd.move_voice(7, at);
        pd.stop_voice(7);
        assert!(!pd.voice_alive(7));
    }

    /// `music_raid`, `music_raidfood` and `music_gifting` start as looping, moving 3D voices on
    /// the music bus that roll off from the camera with the sounds' own distances; the polyphony
    /// limit of a sound refuses a second voice until the first one is stopped.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_raid_and_gift_music_start_as_moving_looping_positional_voices() {
        let Some(mut pd) = installed_pd() else { return };
        let at = Vec3::new(10.0, 0.0, 0.0);
        let mut ids = vec![];
        for (music, min, max) in [
            (PositionalMusic::Raid, 25.0, 75.0),
            (PositionalMusic::RaidFood, 15.0, 45.0),
            (PositionalMusic::Gifting, 15.0, 55.0),
        ] {
            let v = pd
                .start_positional_voice(music.id(), "tribe", at, Bus::Music)
                .unwrap_or_else(|| panic!("{} did not start", music.sound()));
            let voice = &pd.voices[&v];
            assert!(matches!(voice.plain_bus, Some(Bus::Music)), "{}", music.sound());
            let res = voice.resolved.as_ref().expect("resolved");
            let spatial = voice.spatial.as_ref().expect("3D roll-off");
            eprintln!(
                "{}: looped {} gain {:.2} fade-out {:.1} roll-off {:.0}..{:.0} curve {:?}; gain at 10 / 40 / 60 / 80: {:.2} {:.2} {:.2} {:.2}",
                music.sound(), res.looped, res.gain, res.fade_out, spatial.min, spatial.max, spatial.curve,
                spatial.gain(10.0), spatial.gain(40.0), spatial.gain(60.0), spatial.gain(80.0)
            );
            assert!(res.looped, "{} loops", music.sound());
            assert_eq!((spatial.min, spatial.max), (min, max), "{}", music.sound());
            assert_eq!(spatial.gain(spatial.min - 1.0), 1.0, "full level inside the near distance");
            assert_eq!(spatial.gain(spatial.max + 1.0), 0.0, "silent beyond the far distance");
            assert!(voice.params.contains_key("ears") && voice.params.get("is3d").map(|a| a.float()) == Some(1.0));
            assert_eq!(voice.params.get("x").map(|a| a.float()), Some(10.0));
            ids.push(v);
        }
        // The voice moves with its source.
        pd.move_voice(ids[0], Vec3::new(-4.0, 1.0, 9.0));
        let p = &pd.voices[&ids[0]].params;
        assert_eq!((p["x"].float(), p["y"].float(), p["z"].float()), (-4.0, 1.0, 9.0));
        // `music_raid` has polyphony 1: a second voice is refused until the first is stopped.
        let limit = pd.audio.as_ref().unwrap().resolve("music_raid").unwrap().polyphony;
        eprintln!("music_raid polyphony {limit}");
        if limit == 1 {
            assert_eq!(pd.start_positional_voice(PositionalMusic::Raid.id(), "tribe", at, Bus::Music), None);
            pd.stop_voice(ids[0]);
            assert!(!pd.voice_alive(ids[0]));
            assert!(pd.start_positional_voice(PositionalMusic::Raid.id(), "tribe", at, Bus::Music).is_some());
        }
    }

    /// The feed's table drives the voices: a group's sound starts, follows its leader and stops
    /// with the group; the slot is freed.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_the_group_table_starts_moves_and_stops_the_voices() {
        let Some(mut pd) = installed_pd() else { return };
        let mut world = World::new();
        let e = world.spawn_empty().id();
        let mut music = RaidMusic::default();
        let key = SourceKey::Group(15, 0);
        let want = |x: f32| [Want { key, music: PositionalMusic::Raid, position: Vec3::new(x, 0.0, 0.0) }];
        music.step(&mut pd, &want(30.0), Some(Vec3::ZERO));
        assert_eq!(music.voices().len(), 1, "the raid group's sound plays");
        let v = music.voices()[0];
        assert_eq!(pd.voices[&v].params["x"].float(), 30.0);
        music.step(&mut pd, &want(20.0), Some(Vec3::ZERO));
        assert_eq!(pd.voices[&v].params["x"].float(), 20.0, "it follows the leader");
        assert!((music.slots.nearest() - 20.0).abs() < 1e-4);
        music.step(&mut pd, &[], Some(Vec3::ZERO));
        assert!(!pd.voice_alive(v), "the group's end stops the sound");
        assert_eq!(music.slots.used(), 0);
        // An attacker key does not collide with the group keys.
        assert_ne!(SourceKey::Attacker(e), key);
    }
}
