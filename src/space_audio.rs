//! Space music and ambience (S7), the Pure Data patches the Space game mode
//! starts, ported as small state machines (`notes\space-ctrl-notes.md` 6):
//!
//! * `space_music_patch`: the three score stems (`solar_long`,
//!   `interstellar_long`, `galaxy_long`) always play and their gains follow
//!   the `spacezoom` parameter through the patch's three `function` curves
//!   (solar view 0 .. 0.5, galaxy view 0.5 .. 1: 0x1003240 sends it from the
//!   camera radius). The context beds are `amb_space_inter_lp` (solar) and
//!   `amb_space_galaxy_lp` (galaxy); over a planet `atmosphere_sweep_space`
//!   mixes `amb_space_system_lp` and `sfx_amb_wind_far_lp` by the planet zoom.
//! * `music_space_war_patch`: the war level 1..4 of 0xFFAA70 (`war_level`)
//!   debounced for four seconds, a 4x4 table from (old, new) level to one of
//!   17 states, the 98 bpm beat clock that makes the loops and the stinger
//!   enter and leave on the beat, the gongs on enemy deaths at level 4, a
//!   battle group fader (3 s in, 0.5 s out while the level is above 1) and the
//!   slow `fadeovertime` of the whole patch. Muted in the planet context.
//! * `music_space_trade`: its loop while a trade is open.
//!
//! The HUD plays its sounds (`ui_spg_error` ...) through `PlayCue` with the
//! `game_audio::Cue` variants; the camera's zoom whoosh is played from here.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;
use std::collections::HashMap;

use crate::cell_audio::{Curve, Line};
use crate::game::RunProgress;
use crate::game_audio::{Bus, Cue, GameAudio, PlayCue};
use crate::space_view::{SpaceView, SpaceZoom, ZoomCue, GALAXY, SOLAR};
use crate::stages::{GameStage, StageOwned};

// ---------------------------------------------------------------------------
// space_music_patch
// ---------------------------------------------------------------------------

/// `spacezoom` (0x1003240): the camera radius across its range, solar 0..0.5,
/// galaxy 0.5..1.
pub fn spacezoom(galaxy: bool, radius: f32) -> f32 {
    let lim = if galaxy { &GALAXY } else { &SOLAR };
    let t = (radius.clamp(lim.min, lim.max) - lim.min) / (lim.max - lim.min);
    t * 0.5 + if galaxy { 0.5 } else { 0.0 }
}

/// The patch's `function` objects (x = spacezoom, output = gain).
pub const STEM_SOLAR: Curve = Curve {
    ymax: 1.0,
    y0: 1.0,
    pts: &[
        (0.273684, 1.0),
        (0.368421, 1.0),
        (0.436842, 0.755556),
        (0.452632, 0.333333),
        (0.536842, 0.0888889),
        (0.621052, 0.0),
        (1.0, 0.0),
    ],
};
pub const STEM_INTERSTELLAR: Curve = Curve {
    ymax: 1.0,
    y0: 0.0,
    pts: &[
        (0.242105, 0.0),
        (0.357895, 0.188889),
        (0.410526, 0.699999),
        (0.5, 1.0),
        (0.705263, 1.0),
        (0.768421, 0.877778),
        (0.842105, 0.644445),
        (0.868421, 0.288889),
        (0.963158, 0.0),
        (1.0, 0.0),
    ],
};
pub const STEM_GALAXY: Curve = Curve {
    ymax: 1.0,
    y0: 0.0,
    pts: &[
        (0.552632, 0.0),
        (0.668421, 0.177778),
        (0.736842, 0.455556),
        (0.773684, 0.844444),
        (0.863158, 1.0),
        (1.0, 1.0),
    ],
};

// atmosphere_sweep_space: x = planetzoom / 1500.
pub const SWEEP_SYSTEM_GAIN: Curve = Curve {
    ymax: 2.5,
    y0: 0.0,
    pts: &[(0.0894737, 0.0), (0.142105, 0.0), (0.289474, 0.0), (0.442106, 0.6), (1.0, 0.6)],
};
pub const SWEEP_WIND_GAIN: Curve = Curve {
    ymax: 1.4,
    y0: 0.0,
    pts: &[
        (0.0263158, 0.0),
        (0.105263, 0.111111),
        (0.205263, 0.2),
        (0.331579, 0.233333),
        (0.415789, 0.2),
        (0.473684, 0.111111),
        (0.521052, 0.0),
        (1.0, 0.0),
    ],
};
pub const SWEEP_WIND_PITCH: Curve = Curve {
    ymax: 2.0,
    y0: 0.166667,
    pts: &[
        (0.0578948, 0.38889),
        (0.0947368, 0.455556),
        (0.142105, 0.466667),
        (0.184211, 0.444444),
        (0.236842, 0.366667),
        (0.373685, 0.233333),
        (1.0, 0.233332),
    ],
};
/// The sweep's range for the planet zoom (the altitude above the surface).
pub const SWEEP_RANGE: f32 = 1500.0;

// ---------------------------------------------------------------------------
// The war level (0xFFAA70)
// ---------------------------------------------------------------------------

/// What the war level looks at. The empire and building parts are filled by
/// the rules (`SpaceWarInputs`); `threat_ships` is counted here from the
/// pirates and Grox ships of the scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WarSituation {
    /// The planet's empire is at war with the player.
    pub at_war_planet: bool,
    /// A hostile raider / fighter, Grox ship or other threat is present.
    pub threat_ships: bool,
    /// A bomber is aimed at the player.
    pub bomber_on_player: bool,
    /// Enemy turrets / buildings of the planet attacking right now.
    pub attackers: u32,
    /// The player's UFO is flagged safe (0xC38660): the level is 1.
    pub safe: bool,
}

/// `spg_war_music_level`: 1 peace, 2 at war with the planet but nothing is
/// attacking, 3 threatened, 4 more than one attacker.
pub fn war_level(s: &WarSituation) -> i32 {
    let threat = s.threat_ships || s.bomber_on_player;
    let mut level = 1;
    if s.at_war_planet && s.attackers == 0 {
        level = if threat { 3 } else { 2 };
    } else if s.attackers != 1 {
        if threat {
            level = 3;
        }
    } else {
        level = 3;
    }
    if s.attackers > 1 {
        level = 4;
    }
    if s.safe {
        level = 1;
    }
    level
}

/// The rules fill this in (it changes nothing here until they do).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SpaceWarInputs {
    pub at_war_planet: bool,
    pub attackers: u32,
    pub bomber_on_player: bool,
    pub safe: bool,
}

// ---------------------------------------------------------------------------
// music_space_war_patch
// ---------------------------------------------------------------------------

/// `tempo 98`: a beat every 612 ms; `counter 2` makes every second beat the
/// switch beat.
pub const TEMPO_BPM: f32 = 98.0;
pub const BEAT_SECONDS: f32 = 60.0 / TEMPO_BPM;
/// Seconds without a level change before a new level is let through (the
/// patch's metro counts past 3).
pub const LEVEL_QUIET_TICKS: u32 = 3;

/// State of (old level, new level), the patch's 4x4 `coll`.
pub const STATE_TABLE: [[u8; 4]; 4] = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 12, 13], [14, 15, 16, 17]];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WarVoice {
    Low,
    LoopA,
    LoopB,
}

impl WarVoice {
    /// The patch's `fadein` / `fadeout` messages: the low loop 3 s, the
    /// other loops stop in 0.5 s.
    pub fn fades(self) -> (f32, f32) {
        match self {
            WarVoice::Low => (3.0, 3.0),
            _ => (0.0, 0.5),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarEvent {
    Start(WarVoice),
    Stop(WarVoice),
    /// `play music_space_war_end 2`: the stinger.
    End,
    /// `play music_space_war_gong 3`.
    Gong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delayed {
    ClockOn,
    LoopB,
    End,
    StopLoopA,
    StopLoopB,
}

/// The `onebang` objects that wait for the next switch beat.
#[derive(Clone, Copy, Debug, Default)]
struct Armed {
    gong: bool,
    low_start: bool,
    clock_off: bool,
    loopa_start: bool,
    loopa_end: bool,
    loopb_start: bool,
    loopb_end: bool,
    loopb_stop: bool,
    loopa_stop: bool,
}

/// A fader with separate rise and fall times (`fader up down`, ms).
#[derive(Clone, Copy, Debug)]
pub struct Fader {
    pub value: f32,
    up: f32,
    down: f32,
}

impl Fader {
    pub fn new(value: f32, up_ms: f32, down_ms: f32) -> Self {
        Self {
            value,
            up: up_ms / 1000.0,
            down: down_ms / 1000.0,
        }
    }

    pub fn step(&mut self, target: f32, dt: f32) {
        if target > self.value {
            self.value = (self.value + dt / self.up.max(1.0e-3)).min(target);
        } else {
            self.value = (self.value - dt / self.down.max(1.0e-3)).max(target);
        }
    }
}

/// The patch.
#[derive(Clone, Debug)]
pub struct WarPatch {
    /// The level the patch acts on (after the debounce) and the one before.
    pub level: i32,
    pub previous: i32,
    pub state: u8,
    stored: i32,
    quiet_ticks: u32,
    tick_clock: f32,
    spigot_open: bool,
    clock_on: bool,
    beat_clock: f32,
    count: u8,
    spigot49: bool,
    armed: Armed,
    timers: Vec<(f32, Delayed)>,
    /// Gain of the battle group (`spacebattlepause_group`).
    pub group: Fader,
    /// The planet context mutes the patch (1 s in, 0.5 s out).
    pub planet_factor: Fader,
    /// `fadeovertime`: 1 -> 0.7 over 10 s after 10 s above level 2, back to 1
    /// over 5 s after 5 s.
    pub slow: Line,
    slow_timer: Option<(f32, f32, f32)>,
    /// Which loops are playing.
    pub playing: [bool; 3],
}

impl Default for WarPatch {
    fn default() -> Self {
        // The loadbang sends level 1 twice: (0, 1) then (1, 1) = state 1.
        Self {
            level: 1,
            previous: 1,
            state: 1,
            stored: 1,
            quiet_ticks: 4,
            tick_clock: 0.0,
            spigot_open: true,
            clock_on: false,
            beat_clock: 0.0,
            count: 0,
            spigot49: false,
            armed: Armed::default(),
            timers: Vec::new(),
            group: Fader::new(0.0, 3000.0, 500.0),
            planet_factor: Fader::new(1.0, 1000.0, 500.0),
            slow: Line::at(1.0),
            slow_timer: None,
            playing: [false; 3],
        }
    }
}

fn index(voice: WarVoice) -> usize {
    match voice {
        WarVoice::Low => 0,
        WarVoice::LoopA => 1,
        WarVoice::LoopB => 2,
    }
}

impl WarPatch {
    /// The state of a level change (0 outside 1..=4).
    pub fn state_of(from: i32, to: i32) -> u8 {
        if (1..=4).contains(&from) && (1..=4).contains(&to) {
            STATE_TABLE[(from - 1) as usize][(to - 1) as usize]
        } else {
            0
        }
    }

    /// The gain every voice of the patch is multiplied by.
    pub fn gain(&self) -> f32 {
        self.group.value * self.planet_factor.value * self.slow.value
    }

    fn start(&mut self, voice: WarVoice, out: &mut Vec<WarEvent>) {
        if !self.playing[index(voice)] {
            self.playing[index(voice)] = true;
            out.push(WarEvent::Start(voice));
        }
    }

    fn stop(&mut self, voice: WarVoice, out: &mut Vec<WarEvent>) {
        if self.playing[index(voice)] {
            self.playing[index(voice)] = false;
            out.push(WarEvent::Stop(voice));
        }
    }

    fn clock_start(&mut self) {
        self.clock_on = true;
        self.beat_clock = 0.0;
        self.count = 0;
    }

    /// A level change passes the filter: pick the state and run the patch's
    /// `sel` chains.
    fn accept(&mut self, level: i32, out: &mut Vec<WarEvent>) {
        if level == self.level {
            return;
        }
        self.previous = self.level;
        self.level = level;
        let state = Self::state_of(self.previous, level);
        self.state = state;
        // fadeovertime (0 r level -> > 2).
        if level > 2 {
            self.slow_timer = Some((10.0, 0.7, 10.0));
        } else {
            self.slow_timer = Some((5.0, 1.0, 5.0));
        }
        // The low loop (obj 116).
        match state {
            2 => self.start(WarVoice::Low, out),
            10 | 15 => self.armed.low_start = true,
            _ => self.stop(WarVoice::Low, out),
        }
        // The first loop (obj 78 chain).
        match state {
            3 | 7 => self.start(WarVoice::LoopA, out),
            16 => self.armed.loopa_start = true,
            13 => self.armed.loopa_stop = true,
            9 | 10 => self.armed.loopa_end = true,
            _ => {}
        }
        // The beat clock (obj 96 / 95 / 77 chain).
        match state {
            3 | 7 => {
                self.spigot49 = false;
                self.clock_start();
            }
            4 | 8 => {
                self.spigot49 = false;
                self.timers.push((1.75, Delayed::ClockOn));
            }
            9 | 10 | 14 => {
                self.spigot49 = true;
                self.armed.clock_off = true;
            }
            _ => {}
        }
        // The second loop and the stinger (obj 80 chain).
        match state {
            4 | 8 => {
                out.push(WarEvent::End);
                self.timers.push((1.75, Delayed::LoopB));
            }
            13 => self.armed.loopb_start = true,
            16 => self.armed.loopb_stop = true,
            14 | 15 => self.armed.loopb_end = true,
            _ => {}
        }
    }

    fn switch_beat(&mut self, out: &mut Vec<WarEvent>) {
        let armed = std::mem::take(&mut self.armed);
        let mut keep = Armed::default();
        if armed.low_start {
            self.start(WarVoice::Low, out);
        }
        if armed.loopa_start {
            self.start(WarVoice::LoopA, out);
        }
        if armed.loopa_end {
            self.timers.push((0.55, Delayed::End));
            self.timers.push((2.75, Delayed::StopLoopA));
        }
        if armed.loopa_stop {
            self.stop(WarVoice::LoopA, out);
        }
        if armed.loopb_start {
            self.start(WarVoice::LoopB, out);
        }
        if armed.loopb_end {
            self.timers.push((0.55, Delayed::End));
            self.timers.push((2.75, Delayed::StopLoopB));
        }
        if armed.loopb_stop {
            self.stop(WarVoice::LoopB, out);
        }
        if armed.clock_off {
            if self.spigot49 {
                self.clock_on = false;
                self.count = 0;
            } else {
                keep.clock_off = true;
            }
        }
        keep.gong = armed.gong;
        self.armed = Armed {
            gong: keep.gong,
            clock_off: keep.clock_off,
            ..Armed::default()
        };
    }

    /// Advance the patch by `dt` seconds. `level_in` is the game's war level,
    /// `deaths` the enemy ships destroyed this frame, `planet` whether the
    /// planet context is active, `silent` an asset browser / comm pause.
    pub fn step(&mut self, dt: f32, level_in: i32, deaths: u32, planet: bool, silent: bool) -> Vec<WarEvent> {
        let mut out = Vec::new();
        // Level input: debounced (metro 1000, counter > 3).
        if level_in != self.stored {
            self.stored = level_in;
            if self.spigot_open {
                self.accept(level_in, &mut out);
            }
            self.quiet_ticks = 0;
        }
        self.tick_clock += dt;
        while self.tick_clock >= 1.0 {
            self.tick_clock -= 1.0;
            self.quiet_ticks += 1;
            self.spigot_open = self.quiet_ticks > LEVEL_QUIET_TICKS;
            if self.spigot_open {
                self.accept(self.stored, &mut out);
            }
        }
        // Delayed actions.
        let mut due = Vec::new();
        self.timers.retain_mut(|(left, what)| {
            *left -= dt;
            if *left <= 0.0 {
                due.push(*what);
                false
            } else {
                true
            }
        });
        for what in due {
            match what {
                Delayed::ClockOn => self.clock_start(),
                Delayed::LoopB => self.start(WarVoice::LoopB, &mut out),
                Delayed::End => out.push(WarEvent::End),
                Delayed::StopLoopA => self.stop(WarVoice::LoopA, &mut out),
                Delayed::StopLoopB => self.stop(WarVoice::LoopB, &mut out),
            }
        }
        // Enemy deaths arm the gong for the next beat.
        if deaths > 0 {
            self.armed.gong = true;
        }
        // The beat clock.
        if self.clock_on {
            self.beat_clock += dt;
            while self.beat_clock >= BEAT_SECONDS {
                self.beat_clock -= BEAT_SECONDS;
                // Every beat: the gong waits for one (only at level 4).
                if self.armed.gong {
                    self.armed.gong = false;
                    if self.level == 4 {
                        out.push(WarEvent::Gong);
                    }
                }
                self.count = self.count % 2 + 1;
                if self.count == 1 {
                    self.switch_beat(&mut out);
                    if !self.clock_on {
                        break;
                    }
                }
            }
        }
        // fadeovertime.
        if let Some((wait, target, ramp)) = self.slow_timer {
            if wait <= dt {
                self.slow.go(target, ramp);
                self.slow_timer = None;
            } else {
                self.slow_timer = Some((wait - dt, target, ramp));
            }
        }
        self.slow.step(dt);
        // The battle group and the planet context.
        let on = level_in > 1 && !silent;
        self.group.step(if on { 1.0 } else { 0.0 }, dt);
        self.planet_factor.step(if planet { 0.0 } else { 1.0 }, dt);
        out
    }
}

// ---------------------------------------------------------------------------
// Bevy side
// ---------------------------------------------------------------------------

/// A voice of the Space score and its fade envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    StemGalaxy,
    StemInterstellar,
    StemSolar,
    BedInter,
    BedGalaxy,
    BedSystem,
    BedWind,
    Neutral,
    WarLow,
    WarLoopA,
    WarLoopB,
    Trade,
}

impl Slot {
    fn cue(self) -> Cue {
        match self {
            Slot::StemGalaxy => Cue::SpaceStemGalaxy,
            Slot::StemInterstellar => Cue::SpaceStemInterstellar,
            Slot::StemSolar => Cue::SpaceSolar,
            Slot::BedInter => Cue::SpaceAmbience,
            Slot::BedGalaxy => Cue::MenuAmbience,
            Slot::BedSystem => Cue::AmbSpaceSystem,
            Slot::BedWind => Cue::AmbSpaceWindFar,
            Slot::Neutral => Cue::SpaceNeutral,
            Slot::WarLow => Cue::SpaceWarLow,
            Slot::WarLoopA => Cue::SpaceWar,
            Slot::WarLoopB => Cue::SpaceWarLoopB,
            Slot::Trade => Cue::SpaceTradeLoop,
        }
    }

    fn bus(self) -> Bus {
        self.cue().spec().bus
    }
}

#[derive(Component)]
pub struct SpaceVoice {
    slot: Slot,
    /// Bus level times the sound's own gain and the cue's trim.
    base: f32,
    /// Graph gain the patch asks for.
    gain: f32,
    /// Fade envelope (0..1).
    level: f32,
    fade_in: f32,
    fade_out: f32,
    releasing: bool,
}

#[derive(Resource, Default)]
pub struct SpaceAudio {
    pub war: WarPatch,
    /// A trade is open (the HUD sets it with the comm screen's trade tab).
    pub trading: bool,
    /// The comm screen is open: the score pauses.
    pub comm_open: bool,
    voices: HashMap<Slot, Entity>,
    last_kills: Option<u32>,
    pub last_level: i32,
}

/// Seconds a stem takes to appear when the stage starts.
const STEM_FADE_IN: f32 = 6.0;

fn spawn_voice(
    commands: &mut Commands,
    audio: &GameAudio,
    slot: Slot,
    gain: f32,
    fade_in: f32,
    fade_out: f32,
) -> Option<Entity> {
    let clip = audio.clip_full(slot.cue(), 0)?;
    let spec = slot.cue().spec();
    let base = slot.bus().volume() * clip.gain * spec.trim;
    let entity = commands
        .spawn((
            Name::new(format!("space score {}", spec.key)),
            AudioPlayer::new(clip.handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(0.0),
                speed: clip.pitch,
                ..PlaybackSettings::ONCE
            },
            SpaceVoice {
                slot,
                base,
                gain,
                level: 0.0,
                fade_in: fade_in.max(clip.fade_in).max(0.05),
                fade_out: fade_out.max(0.05),
                releasing: false,
            },
            StageOwned,
        ))
        .id();
    Some(entity)
}

/// The Space score: stems, beds, the war patch and the trade loop.
#[allow(clippy::too_many_arguments)]
pub fn space_score(
    mut commands: Commands,
    time: Res<Time>,
    mut audio: ResMut<GameAudio>,
    mut state: ResMut<SpaceAudio>,
    inputs: Res<SpaceWarInputs>,
    view: Option<ResMut<SpaceView>>,
    run: Res<RunProgress>,
    editor: Res<crate::editor::EditorState>,
    pirates: Query<&crate::space::Pirate>,
    mut voices: Query<(Entity, &mut SpaceVoice, Option<&mut AudioSink>)>,
    mut out: MessageWriter<PlayCue>,
) {
    let Some(mut view) = view else { return };
    if audio.library().is_none() {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let mute = if audio.muted { 0.0 } else { 1.0 };
    // The camera's zoom whoosh.
    if view.zoom_cue == ZoomCue::OutStarted {
        out.write(PlayCue(Cue::SpaceZoomOut));
    } else if view.zoom_cue == ZoomCue::InStarted {
        out.write(PlayCue(Cue::SpaceZoomIn));
    }
    view.zoom_cue = ZoomCue::None;
    // The Pd engine runs the Space mix modes (pd_audio): only the whoosh here.
    if crate::pd_audio::stage_uses_pd(&GameStage::Space) {
        return;
    }

    let zoom = view.zoom();
    let planet = zoom == SpaceZoom::Planet;
    let galaxy = zoom == SpaceZoom::Galaxy;
    let radius = if galaxy { view.gal.radius } else { view.solar.radius };
    let sz = spacezoom(galaxy, radius);
    let paused = state.comm_open || state.trading || editor.open;

    // --- the war patch ------------------------------------------------------
    let threat_ships = pirates.iter().any(|p| p.hp > 0.0);
    let situation = WarSituation {
        at_war_planet: inputs.at_war_planet,
        threat_ships,
        bomber_on_player: inputs.bomber_on_player,
        attackers: inputs.attackers,
        safe: inputs.safe,
    };
    let level = war_level(&situation);
    state.last_level = level;
    let kills = run.space_campaign.kills;
    let deaths = state
        .last_kills
        .map_or(0, |before| kills.saturating_sub(before));
    state.last_kills = Some(kills);
    let events = state.war.step(dt, level, deaths, planet, paused);
    for event in events {
        match event {
            WarEvent::Start(voice) => {
                let slot = war_slot(voice);
                let (fade_in, fade_out) = voice.fades();
                audio.request(slot.cue());
                if let Some(old) = state.voices.remove(&slot) {
                    commands.entity(old).despawn();
                }
                if let Some(entity) = spawn_voice(&mut commands, &audio, slot, 1.0, fade_in, fade_out) {
                    state.voices.insert(slot, entity);
                } else {
                    // Not decoded yet: the voice starts when it is (below).
                    state.war.playing[index(voice)] = true;
                }
            }
            WarEvent::Stop(voice) => {
                if let Some(entity) = state.voices.get(&war_slot(voice)) {
                    if let Ok((_, mut v, _)) = voices.get_mut(*entity) {
                        v.releasing = true;
                    }
                }
            }
            WarEvent::End => {
                if !paused && !planet {
                    out.write(PlayCue(Cue::SpaceWarEnd));
                }
            }
            WarEvent::Gong => {
                if !paused && !planet {
                    out.write(PlayCue(Cue::SpaceWarGong));
                }
            }
        }
    }

    // --- the desired gains ----------------------------------------------------
    let pause_gain = if paused { 0.0 } else { 1.0 };
    let altitude = view.planet.as_ref().map_or(0.0, |p| p.altitude);
    let sweep_x = (altitude / SWEEP_RANGE).clamp(0.0, 1.0);
    let mut want: Vec<(Slot, f32, f32, f32)> = Vec::new(); // slot, gain, fade in, fade out
    if !planet {
        want.push((Slot::StemSolar, STEM_SOLAR.eval(sz) * 1.5 * pause_gain, STEM_FADE_IN, 3.0));
        want.push((Slot::StemInterstellar, STEM_INTERSTELLAR.eval(sz) * pause_gain, STEM_FADE_IN, 3.0));
        want.push((Slot::StemGalaxy, STEM_GALAXY.eval(sz) * 2.0 * pause_gain, STEM_FADE_IN, 3.0));
        if galaxy {
            want.push((Slot::BedGalaxy, 1.0, 3.0, 3.0));
        } else {
            want.push((Slot::BedInter, 1.0, 3.0, 3.0));
        }
    } else {
        want.push((Slot::Neutral, 0.9 * pause_gain, 4.0, 3.0));
        want.push((Slot::BedSystem, SWEEP_SYSTEM_GAIN.eval(sweep_x), 2.0, 2.0));
        want.push((Slot::BedWind, SWEEP_WIND_GAIN.eval(sweep_x), 2.0, 2.0));
    }
    if state.trading {
        want.push((Slot::Trade, 1.0, 2.0, 2.0));
    }
    let war_gain = state.war.gain();
    for slot in [Slot::WarLow, Slot::WarLoopA, Slot::WarLoopB] {
        if state.voices.contains_key(&slot) {
            want.push((slot, war_gain, 0.05, 0.5));
        }
    }

    // --- voices: spawn what is wanted, set gains, fade and expire ---------------------
    for (slot, gain, fade_in, fade_out) in &want {
        let is_war = matches!(slot, Slot::WarLow | Slot::WarLoopA | Slot::WarLoopB);
        if is_war {
            continue;
        }
        audio.request(slot.cue());
        let alive = state
            .voices
            .get(slot)
            .is_some_and(|e| voices.get(*e).is_ok_and(|(_, v, _)| !v.releasing));
        if !alive && *gain > 0.001 {
            if let Some(entity) = spawn_voice(&mut commands, &audio, *slot, 0.0, *fade_in, *fade_out) {
                state.voices.insert(*slot, entity);
            }
        }
    }
    // War voices whose clip was not decoded at the start event.
    for voice in [WarVoice::Low, WarVoice::LoopA, WarVoice::LoopB] {
        let slot = war_slot(voice);
        if state.war.playing[index(voice)] && !state.voices.contains_key(&slot) {
            audio.request(slot.cue());
            let (fade_in, fade_out) = voice.fades();
            if let Some(entity) = spawn_voice(&mut commands, &audio, slot, 1.0, fade_in, fade_out) {
                state.voices.insert(slot, entity);
            }
        }
    }
    for (entity, mut voice, sink) in &mut voices {
        let target = want
            .iter()
            .find(|(s, ..)| *s == voice.slot)
            .map_or(0.0, |(_, g, ..)| *g);
        // The patches ramp their gains (the stems' `line` objects take 1-1.5 s).
        let smooth = if is_war_slot(voice.slot) { 1.0 } else { 1.0 - (-dt / 0.6).exp() };
        voice.gain += (target - voice.gain) * smooth;
        let wanted = !voice.releasing && (target > 0.001 || state.voices.get(&voice.slot) == Some(&entity));
        if !wanted || voice.releasing {
            voice.level -= dt / voice.fade_out;
        } else {
            voice.level += dt / voice.fade_in;
        }
        voice.level = voice.level.clamp(0.0, 1.0);
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(voice.base * voice.gain * voice.level * mute));
            if voice.slot == Slot::BedWind {
                // The sweep patch's `pitch` function lowers the far wind.
                sink.set_speed(SWEEP_WIND_PITCH.eval(sweep_x).max(0.1));
            }
        }
        let gone = (voice.releasing && voice.level <= 0.0)
            || (target <= 0.001 && voice.level <= 0.0 && !is_war_slot(voice.slot));
        if gone {
            commands.entity(entity).despawn();
            if state.voices.get(&voice.slot) == Some(&entity) {
                state.voices.remove(&voice.slot);
            }
        }
    }
}

fn is_war_slot(slot: Slot) -> bool {
    matches!(slot, Slot::WarLow | Slot::WarLoopA | Slot::WarLoopB)
}

fn war_slot(voice: WarVoice) -> Slot {
    match voice {
        WarVoice::Low => Slot::WarLow,
        WarVoice::LoopA => Slot::WarLoopA,
        WarVoice::LoopB => Slot::WarLoopB,
    }
}

/// A fresh score each time Space starts (the war patch loads at mode entry).
fn reset(mut state: ResMut<SpaceAudio>) {
    *state = SpaceAudio::default();
}

pub struct SpaceAudioPlugin;

impl Plugin for SpaceAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpaceAudio>()
            .init_resource::<SpaceWarInputs>()
            .add_systems(OnEnter(GameStage::Space), reset)
            .add_systems(
                Update,
                space_score
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Space))
                    .run_if(resource_exists::<GameAudio>)
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 2.0e-3, "{a} != {b}");
    }

    #[test]
    fn spacezoom_is_half_per_view() {
        close(spacezoom(false, SOLAR.min), 0.0);
        close(spacezoom(false, SOLAR.max), 0.5);
        close(spacezoom(true, GALAXY.min), 0.5);
        close(spacezoom(true, GALAXY.max), 1.0);
        close(spacezoom(false, 5000.0), 0.5);
        // The solar discrete level sits at 0.5 * (500 - 5) / 1745.
        close(spacezoom(false, 500.0), 0.5 * 495.0 / 1745.0);
    }

    #[test]
    fn the_stems_cross_fade_with_the_zoom() {
        // In the solar view the solar stem is full, the others silent.
        let near = spacezoom(false, 100.0);
        close(STEM_SOLAR.eval(near), 1.0);
        close(STEM_INTERSTELLAR.eval(near), 0.0);
        close(STEM_GALAXY.eval(near), 0.0);
        // Half way out the interstellar stem peaks.
        close(STEM_INTERSTELLAR.eval(0.5), 1.0);
        close(STEM_INTERSTELLAR.eval(0.6), 1.0);
        // At the far end of the galaxy only the galaxy stem plays.
        close(STEM_GALAXY.eval(1.0), 1.0);
        close(STEM_SOLAR.eval(1.0), 0.0);
        close(STEM_INTERSTELLAR.eval(1.0), 0.0);
        // The solar stem is gone by 0.62, the galaxy one starts at 0.55.
        close(STEM_SOLAR.eval(0.621052), 0.0);
        close(STEM_GALAXY.eval(0.552632), 0.0);
        close(STEM_GALAXY.eval(0.863158), 1.0);
    }

    #[test]
    fn the_planet_sweep_follows_its_functions() {
        close(SWEEP_SYSTEM_GAIN.eval(0.0), 0.0);
        close(SWEEP_SYSTEM_GAIN.eval(0.25), 0.0);
        close(SWEEP_SYSTEM_GAIN.eval(1.0), 1.5);
        close(SWEEP_WIND_GAIN.eval(0.331579), 0.233333 * 1.4);
        close(SWEEP_WIND_GAIN.eval(0.6), 0.0);
        close(SWEEP_WIND_PITCH.eval(0.0), 0.166667 * 2.0);
        close(SWEEP_WIND_PITCH.eval(1.0), 0.233332 * 2.0);
    }

    #[test]
    fn the_war_level_follows_0xffaa70() {
        let none = WarSituation::default();
        assert_eq!(war_level(&none), 1);
        // At war with the planet: level 2, 3 with a threat in the sky.
        let war = WarSituation {
            at_war_planet: true,
            ..none
        };
        assert_eq!(war_level(&war), 2);
        assert_eq!(
            war_level(&WarSituation {
                threat_ships: true,
                ..war
            }),
            3
        );
        // Raiders alone: level 3.
        assert_eq!(
            war_level(&WarSituation {
                threat_ships: true,
                ..none
            }),
            3
        );
        assert_eq!(
            war_level(&WarSituation {
                bomber_on_player: true,
                ..none
            }),
            3
        );
        // One attacker is 3 whatever else; more than one is 4.
        assert_eq!(war_level(&WarSituation { attackers: 1, ..none }), 3);
        assert_eq!(war_level(&WarSituation { attackers: 2, ..war }), 4);
        // A war with attackers around but no threat: attackers != 1 and none: peace?
        assert_eq!(war_level(&WarSituation { attackers: 0, at_war_planet: false, ..none }), 1);
        // The safe flag silences it.
        assert_eq!(
            war_level(&WarSituation {
                attackers: 3,
                safe: true,
                ..war
            }),
            1
        );
    }

    #[test]
    fn the_state_table_maps_old_and_new_level() {
        assert_eq!(WarPatch::state_of(1, 2), 2);
        assert_eq!(WarPatch::state_of(1, 3), 3);
        assert_eq!(WarPatch::state_of(1, 4), 4);
        assert_eq!(WarPatch::state_of(2, 1), 5);
        assert_eq!(WarPatch::state_of(2, 3), 7);
        assert_eq!(WarPatch::state_of(3, 1), 9);
        assert_eq!(WarPatch::state_of(3, 2), 10);
        assert_eq!(WarPatch::state_of(3, 4), 13);
        assert_eq!(WarPatch::state_of(4, 1), 14);
        assert_eq!(WarPatch::state_of(4, 2), 15);
        assert_eq!(WarPatch::state_of(4, 3), 16);
        assert_eq!(WarPatch::state_of(0, 1), 0);
        assert_eq!(WarPatch::state_of(1, 5), 0);
    }

    fn run(patch: &mut WarPatch, seconds: f32, level: i32, deaths_at: &[f32]) -> Vec<(f32, WarEvent)> {
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        let mut log = Vec::new();
        let mut pending: Vec<f32> = deaths_at.to_vec();
        while t < seconds {
            let deaths = if pending.first().is_some_and(|d| *d <= t) {
                pending.remove(0);
                1
            } else {
                0
            };
            for event in patch.step(dt, level, deaths, false, false) {
                log.push((t, event));
            }
            t += dt;
        }
        log
    }

    #[test]
    fn a_new_level_waits_for_four_quiet_seconds_after_a_recent_change() {
        let mut patch = WarPatch::default();
        // The first change after a long quiet passes at once.
        let log = run(&mut patch, 0.2, 2, &[]);
        assert!(log.contains(&(log[0].0, WarEvent::Start(WarVoice::Low))), "{log:?}");
        assert_eq!(patch.level, 2);
        // The metro tick closes the filter; a change now is held back until
        // the input has been quiet for four ticks.
        run(&mut patch, 1.5, 2, &[]);
        let log = run(&mut patch, 1.5, 3, &[]);
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(patch.level, 2);
        let log = run(&mut patch, 4.0, 3, &[]);
        assert_eq!(patch.level, 3, "{log:?}");
        assert!(log.iter().any(|(_, e)| *e == WarEvent::Start(WarVoice::LoopA)));
        // The low loop stops with the change to 3 (state 7).
        assert!(log.iter().any(|(_, e)| *e == WarEvent::Stop(WarVoice::Low)));
    }

    #[test]
    fn level_four_plays_the_stinger_then_the_second_loop_and_the_gong_on_the_beat() {
        let mut patch = WarPatch::default();
        let log = run(&mut patch, 0.1, 4, &[]);
        // State 4 (1 -> 4): the stinger now.
        assert!(log.contains(&(log[0].0, WarEvent::End)), "{log:?}");
        assert_eq!(patch.state, 4);
        // 1.75 s later the second loop starts and the beat clock runs.
        let log = run(&mut patch, 2.0, 4, &[]);
        assert!(log.iter().any(|(_, e)| *e == WarEvent::Start(WarVoice::LoopB)), "{log:?}");
        assert!(patch.playing[2]);
        // A death arms the gong; it sounds on the next beat at level 4 only.
        let log = run(&mut patch, 1.0, 4, &[0.01]);
        let gongs: Vec<_> = log.iter().filter(|(_, e)| *e == WarEvent::Gong).collect();
        assert_eq!(gongs.len(), 1, "{log:?}");
        assert!(gongs[0].0 <= BEAT_SECONDS + 0.05, "the gong waits for a beat: {}", gongs[0].0);
    }

    #[test]
    fn leaving_the_fight_ends_on_the_beat_with_the_stinger_and_a_late_stop() {
        let mut patch = WarPatch::default();
        run(&mut patch, 0.1, 3, &[]);
        assert!(patch.playing[1], "level 3 starts the first loop");
        assert!(patch.clock_on, "and the beat clock");
        // Back to peace after a long quiet: state 9 arms the ending.
        let log = run(&mut patch, 12.0, 1, &[]);
        let end = log.iter().find(|(_, e)| *e == WarEvent::End).map(|(t, _)| *t);
        let stop = log
            .iter()
            .find(|(_, e)| *e == WarEvent::Stop(WarVoice::LoopA))
            .map(|(t, _)| *t);
        let (end, stop) = (end.expect("stinger"), stop.expect("stop"));
        assert!((stop - end - (2.75 - 0.55)).abs() < 0.1, "stop {stop} end {end}");
        assert!(!patch.playing[1]);
        assert!(!patch.clock_on, "the clock stops at the next switch beat");
    }

    #[test]
    fn the_battle_group_fades_in_over_three_seconds_and_out_in_half_a_second() {
        let mut patch = WarPatch::default();
        let dt = 1.0 / 60.0;
        for _ in 0..90 {
            patch.step(dt, 3, 0, false, false);
        }
        assert!(patch.group.value > 0.4 && patch.group.value < 0.6, "{}", patch.group.value);
        for _ in 0..120 {
            patch.step(dt, 3, 0, false, false);
        }
        close(patch.group.value, 1.0);
        for _ in 0..40 {
            patch.step(dt, 1, 0, false, false);
        }
        close(patch.group.value, 0.0);
        // The planet context mutes the patch (fast in, 1 s).
        for _ in 0..90 {
            patch.step(dt, 3, 0, true, false);
        }
        close(patch.planet_factor.value, 0.0);
        assert_eq!(patch.gain(), 0.0);
    }

    #[test]
    fn the_whole_patch_settles_to_seventy_percent_in_a_long_fight() {
        let mut patch = WarPatch::default();
        let dt = 1.0 / 60.0;
        for _ in 0..(25 * 60) {
            patch.step(dt, 3, 0, false, false);
        }
        close(patch.slow.value, 0.7);
        // Peace brings it back to full.
        for _ in 0..(20 * 60) {
            patch.step(dt, 1, 0, false, false);
        }
        close(patch.slow.value, 1.0);
    }

    #[test]
    fn the_cues_exist_for_every_planned_sound() {
        for cue in [
            Cue::SpaceStemGalaxy,
            Cue::SpaceStemInterstellar,
            Cue::SpaceWarLow,
            Cue::SpaceWarLoopB,
            Cue::SpaceWarEnd,
            Cue::SpaceWarGong,
            Cue::SpaceTradeLoop,
            Cue::AmbSpaceSystem,
            Cue::AmbSpaceWindFar,
            Cue::SpgError,
            Cue::SpaceZoomOut,
        ] {
            assert!(!cue.spec().names.is_empty());
        }
        assert_eq!(Cue::SpgError.spec().names, &["ui_spg_error"]);
        assert_eq!(Cue::SpaceZoomOut.spec().names, &["ui_camera_zoom_out"]);
        assert_eq!(Cue::UfoComTradeBuy.spec().names, &["ui_ufo_com_trade_buy"]);
        assert_eq!(Cue::SpaceWarGong.spec().names.len(), 3);
        assert!(Cue::SpaceWarGong.spec().pool);
    }
}
