//! Positional Cell voices that are re-shaped every frame (helper `cell-audio4`).
//!
//! RETAIL: a sound's track updates its voice on every audio update: the distance to the listener
//! (listener = the avatar, or the camera look point during a cinematic) gives the roll-off gain
//! (0xA41C10), the `lowpass` / `hipass` cutoffs, the `wetlevel` reverb send (0xA1C9B0) and, through
//! the voice's 3D update, the Pan2D1 gains (0xA12740..0xA12A47, 0x11515E0); looped and one-shot
//! sounds alike, and a moving source (a cell that swims past, an effect that travels) changes all
//! of it continuously. The chain of the voice is `SnP1 Rch0 Rsp0 Pau0 Rmd0 Chr0 Dly0 LI20 HI20 Pn21
//! Sen0[reverb] Sen0` (voice template `sound`); the `reverb` submix is a ReverbModel1 of 2.5 s and
//! 30 m (`notes\cell-audio4-notes.md`, [`crate::cell_acoustics`]).
//!
//! This module is that voice: [`SpatialAsset`] is a rodio source that plays a decoded WAV (mono or
//! stereo, one-shot or looped, at the soundProp's pitch) through a low-pass, a high-pass, the two
//! speaker panner and a send into the global reverb ([`ReverbBus`] -> [`ReverbSource`]); its
//! controls ([`SpatialCtl`]) are atomics that [`drive_spatial`] rewrites every frame from the
//! source's current position ([`Anchor`]) and the listener. Positional one-shots of the Cell
//! (`cell_audio::play_sounds`) and the looped effect sounds of the Cell (`effect_audio`) use it;
//! other stages keep their flat voices (no [`Anchor`] is given there).
//!
//! VERIFIED (helper `cell-audio5`): the biquads are RBJ filters at Q = 1 with the plugins' clamps; the
//! reverb has the retail comb lengths (prime table, 344.8 m/s) and allpass values per channel.
//! VERIFIED (helper `cell-final4`): the rest of ReverbModel1 - the feedback tables by rate and length
//! (0x1139A30), the loop gains (0x1139DB0), the comb section (kernel 0x114DAE0: a feedback comb with
//! a one-pole low-pass in the loop and the weight 1/6), the all-pass kernel 0x114DD90 and the output
//! level (the block process 0x114D450); see [`Reverb`].
//!
//! VERIFIED (helper `cell-final5`): the send into the reverb is L + R of the panned pair (the mono `reverb`
//! submix folds the send's two channels by summing; see the header of `cell_acoustics`).
//!
//! ADAPTED (see the header of `cell_acoustics` for what is retail data): the roughly 10 ms smoothing of the
//! controls (the platform ramps its parameters too: the send's gain is ramped across each 256 frame block in 64 steps of
//! 4 frames, 0x115D570; the other plugins' ramps and the update cadence are not decoded). No Doppler: VERIFIED that nothing drives it (the track's Doppler
//! curve is property 0x9BFC37AC; none of the 11005 installed soundProps carries it, census test
//! `cell_acoustics::installed_sound_props_doppler_census`).

use std::{
    f32::consts::{SQRT_2, TAU},
    num::NonZero,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
};

use bevy::{
    asset::Asset,
    audio::{AudioPlayer, Decodable, PlaybackSettings},
    ecs::system::SystemParam,
    prelude::*,
    reflect::TypePath,
};

use crate::{
    cell_acoustics::{self, Acoustics},
    cell_audio::{CellVoice, Loaded},
    cell_engine::{CellGame, UNIT},
    cinematics::Cinematics,
    game::Player,
    game_audio::{GameAudio, SoundDef},
    stages::{GameStage, StageOwned},
    swarm_fx::sound::SwarmSounds,
};

// ---------------------------------------------------------------------------
// What a voice follows and what it hears
// ---------------------------------------------------------------------------

/// Where a positional voice's source is (world plane: x, z of the Cell world, world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// A fixed point (a collision's contact point).
    Point(Vec2),
    /// A cell: the voice follows its transform every frame. The point is where the cell was when
    /// the sound was asked for (used if the cell is gone before the voice starts).
    Cell(Entity, Vec2),
    /// A sound component of a running effect (`LiveSound::voice`) with its location update period
    /// (`invSample`): the voice follows the component, refreshing its position at that period; after
    /// the effect ends the voice keeps its last position. The point is the component's position
    /// when the sound was asked for.
    Effect(u64, f32, Vec2),
}

impl Anchor {
    /// Where the source was when the sound was asked for.
    fn fallback(&self) -> Vec2 {
        match self {
            Anchor::Point(p) | Anchor::Cell(_, p) | Anchor::Effect(_, _, p) => *p,
        }
    }
}

/// The listener and the source positions of the Cell world.
#[derive(SystemParam)]
pub struct CellEars<'w, 's> {
    player: Query<'w, 's, &'static Transform, With<Player>>,
    cells: Query<'w, 's, &'static Transform>,
    game: Option<Res<'w, CellGame>>,
    cinematics: Option<Res<'w, Cinematics>>,
    swarm: Option<Res<'w, SwarmSounds>>,
}

impl CellEars<'_, '_> {
    /// The listener on the world plane (0xE73F60): the camera look point while a cinematic plays,
    /// else the avatar.
    pub fn listener(&self) -> Option<Vec2> {
        let look = self.game.as_ref().map_or(Vec2::ZERO, |g| g.camera_look);
        cell_acoustics::listener_plane(
            self.cinematics.as_ref().is_some_and(|c| c.is_playing()),
            look,
            self.player.single().ok().map(|t| Vec2::new(t.translation.x, t.translation.z)),
        )
    }

    /// Where an anchor is now (`None`: the cell is gone, the effect component left the list).
    pub fn position(&self, anchor: &Anchor) -> Option<Vec2> {
        match anchor {
            Anchor::Point(p) => Some(*p),
            Anchor::Cell(e, _) => self.cells.get(*e).ok().map(|t| Vec2::new(t.translation.x, t.translation.z)),
            Anchor::Effect(uid, _, _) => self
                .swarm
                .as_ref()
                .and_then(|s| s.live.iter().rev().find(|l| l.voice == *uid))
                .map(|l| Vec2::new(l.origin.x, l.origin.z)),
        }
    }
}

// ---------------------------------------------------------------------------
// Controls (main thread -> audio thread)
// ---------------------------------------------------------------------------

/// What the track tells the voice at one update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceState {
    /// Level: bus x soundProp gain x play gain x mixer x roll-off.
    pub gain: f32,
    /// `[input channel][left, right]` gains of the panner.
    pub pan: [[f32; 2]; 2],
    /// Low-pass cutoff in Hz (0 = open).
    pub lowpass: f32,
    /// High-pass cutoff in Hz (0 = off).
    pub hipass: f32,
    /// `wetlevel`: the reverb send (times the level).
    pub wet: f32,
}

/// The voice state of a source at `offset` (retail units, source minus listener, screen right = +x,
/// screen up = -y) from the listener: the retail track update. `base` is every distance-independent
/// factor of the level; `range` is the (min, max) distance of the voice's state curves (a Cell
/// creature voice, `cell_voices`: fixed for the voice's life by the state the event carried, while
/// the roll-off against the changing distance is evaluated on every update), `None` the
/// soundProp's own.
pub fn voice_state(
    def: &SoundDef,
    acoustics: &Acoustics,
    offset: Vec2,
    base: f32,
    stereo: bool,
    range: Option<(f32, f32)>,
) -> VoiceState {
    let distance = offset.length();
    VoiceState {
        gain: base * cell_acoustics::roll_off(def, range, distance),
        pan: cell_acoustics::pan_gains(offset, def.min_distance, def.max_distance, stereo),
        lowpass: acoustics.lowpass_at(distance).unwrap_or(0.0),
        hipass: acoustics.hipass_at(distance).unwrap_or(0.0),
        wet: acoustics.wet_at(distance),
    }
}

fn atomic(v: f32) -> AtomicU32 {
    AtomicU32::new(v.to_bits())
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// Controls shared between [`drive_spatial`] (main thread) and the source (audio thread).
#[derive(Debug)]
pub struct SpatialCtl {
    gain: AtomicU32,
    pan: [AtomicU32; 4],
    lowpass: AtomicU32,
    hipass: AtomicU32,
    wet: AtomicU32,
    stop: AtomicBool,
}

impl SpatialCtl {
    pub fn new(state: &VoiceState) -> Self {
        let ctl = Self {
            gain: atomic(0.0),
            pan: [atomic(0.0), atomic(0.0), atomic(0.0), atomic(0.0)],
            lowpass: atomic(0.0),
            hipass: atomic(0.0),
            wet: atomic(0.0),
            stop: AtomicBool::new(false),
        };
        ctl.set(state);
        ctl
    }

    pub fn set(&self, state: &VoiceState) {
        self.gain.store(state.gain.to_bits(), Ordering::Relaxed);
        for (slot, v) in self.pan.iter().zip(state.pan.iter().flatten()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.lowpass.store(state.lowpass.to_bits(), Ordering::Relaxed);
        self.hipass.store(state.hipass.to_bits(), Ordering::Relaxed);
        self.wet.store(state.wet.to_bits(), Ordering::Relaxed);
    }

    /// End the voice at once (tests; a despawned entity ends it in the game).
    #[cfg(test)]
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// The two speaker filters (LowPassIir2 / HighPassIir2)
// ---------------------------------------------------------------------------

/// A second order section whose cutoff can change while it runs (state is kept).
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
    on: bool,
}

impl Biquad {
    /// Cutoff in Hz, 0 = bypassed. VERIFIED (LowPassIir2 0x1153570 / 0x11534D0, HighPassIir2
    /// 0x1153ED0 / 0x1153E30): `w = 2 pi cutoff / rate`; the low-pass is bypassed from `w >= 3.138451`
    /// (99.9 % of Nyquist) and held at `w >= 0.003141593` (0.1 % of Nyquist, 24 Hz at 48 kHz; the
    /// embedded documentation says 99 % and 1 %), the high-pass is bypassed up to `w <= 0.003141593` and held at
    /// 3.138451; the coefficients are the RBJ cookbook filters with **Q = 1** (`alpha = sin(w) / 2`,
    /// `a0 = 1 + alpha`, `a1 = -2 cos(w) / a0`, `a2 = (1 - alpha) / a0`, low-pass `b0 = b2 =
    /// (1 - cos w) / 2 a0`, `b1 = (1 - cos w) / a0`, high-pass `b0 = b2 = (1 + cos w) / 2 a0`,
    /// `b1 = -(1 + cos w) / a0`), run as direct form I with 1e-18 added against denormals.
    fn design(&mut self, cutoff: f32, rate: f32, high: bool) {
        const W_MIN: f32 = 0.003_141_593;
        const W_MAX: f32 = 3.138_451;
        let w = TAU * cutoff / rate;
        if cutoff <= 0.0 || (!high && w >= W_MAX) || (high && w <= W_MIN) {
            self.on = false;
            return;
        }
        let w = if high { w.min(W_MAX) } else { w.max(W_MIN) };
        let (s, c) = w.sin_cos();
        let alpha = s * 0.5;
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high { ((1.0 + c) / 2.0, -(1.0 + c)) } else { ((1.0 - c) / 2.0, 1.0 - c) };
        self.b = [b0 / a0, b1 / a0, b0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - alpha) / a0];
        if !self.on {
            self.z = [0.0; 2];
        }
        self.on = true;
    }

    fn run(&mut self, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

// ---------------------------------------------------------------------------
// The reverb submix
// ---------------------------------------------------------------------------

/// Sample rate of the reverb voice.
pub const REVERB_RATE: u32 = 44_100;
/// `reverbtime` of the `reverb` submix (`reverbsubmix`: `reverbtime=RM10:0,2.5`).
pub const REVERB_TIME: f32 = 2.5;
/// `reverbsize` of the `reverb` submix (`reverbsize=RM10:1,30`), metres.
pub const REVERB_SIZE: f32 = 30.0;

const RING: usize = 1 << 12;

/// The mono input of the reverb: voices add their sends a frame ahead of the reverb's read head
/// (lock free: both run in the output mixer's thread, the main thread never touches it).
#[derive(Debug)]
pub struct ReverbBus {
    slots: Vec<AtomicU32>,
    head: AtomicUsize,
}

impl Default for ReverbBus {
    fn default() -> Self {
        Self { slots: (0..RING).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0) }
    }
}

impl ReverbBus {
    /// Add `v` to the slot `ahead + 1` frames after the read head.
    pub fn send(&self, ahead: usize, v: f32) {
        let at = (self.head.load(Ordering::Relaxed) + 1 + ahead) & (RING - 1);
        let slot = &self.slots[at];
        let mut current = slot.load(Ordering::Relaxed);
        loop {
            let next = (f32::from_bits(current) + v).to_bits();
            match slot.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
    }

    /// The sum of the sends of this frame; advances the head.
    fn take(&self) -> f32 {
        let head = self.head.load(Ordering::Relaxed);
        let v = self.slots[head & (RING - 1)].swap(0, Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Relaxed);
        f32::from_bits(v)
    }
}

/// The global reverb input of the Cell.
#[derive(Resource, Default)]
pub struct CellReverb(pub Arc<ReverbBus>);

/// The plugin's denormal guard (`(v + 1e-18) - 1e-18`, 0x114DD90 / 0x114DAE0's neighbours).
fn flush(v: f32) -> f32 {
    (v + 1e-18) - 1e-18
}

/// One section of the parallel bank, VERIFIED (kernel 0x114DAE0 through the wrapper 0x114DC90, the
/// parameters stored by 0x114DD20 in 0x113A030): a feedback comb whose loop carries the gain `damp`
/// and a one-pole low-pass of pole `pole`, read back from a delay line of `D + 1` samples with the
/// two taps `L[n] = u[n - D - 1]`, `L[n + 1] = u[n - D]`:
/// `t[n] = x[n] + damp L[n+1]`, `u[n] = t[n] + pole u[n-1]` (written to the line),
/// `y[n] = (L[n+1] - pole L[n]) / 6`, i.e. `t[n - D] / 6` (the one-pole cancels out of the output).
struct Comb {
    /// `u` history: slot `n mod len` holds `u[n - D - 2]` before sample `n` overwrites it.
    buf: Vec<f32>,
    at: usize,
    /// `u[n-1]` (the section state, `section + 0x20`).
    state: f32,
    /// The feedback table value `c_j` (0x1139A30, `section + 0x10 = -c_j`).
    pole: f32,
    /// `(1 - c_j)(1 - 0.366 / reverbtime)` (0x1139DB0, `section + 0x14 = -damp_j`).
    damp: f32,
}

/// The six equal output weights of the bank (`section + 0x1C`, 0x113A1F2: 1/6).
const COMB_WEIGHT: f32 = 1.0 / 6.0;

impl Comb {
    fn new(len: usize, pole: f32, damp: f32) -> Self {
        Self { buf: vec![0.0; len + 2], at: 0, state: 0.0, pole, damp }
    }

    fn process(&mut self, x: f32) -> f32 {
        let n = self.buf.len();
        // The slot of sample n holds u[n - D - 2]; the next one u[n - D - 1] = L[n]; then u[n - D] = L[n+1].
        let l0 = self.buf[(self.at + 1) % n];
        let l1 = self.buf[(self.at + 2) % n];
        let t = x + self.damp * l1;
        let u = flush(t + self.pole * self.state);
        self.state = u;
        self.buf[self.at] = u;
        self.at = (self.at + 1) % n;
        (l1 - self.pole * l0) * COMB_WEIGHT
    }
}

/// The output all-pass of a channel, VERIFIED (kernel 0x114DD90 through the wrapper 0x114E220, set
/// by 0x113A4AF): `v[n] = x[n] - g L[n]` (written to the line), `y[n] = (g v[n] + L[n]) gain`,
/// `L[n] = v[n - A]`, with the denormal guard on `L` and `v`; `gain` is `2 / channels` (0x113A743).
struct Allpass {
    buf: Vec<f32>,
    at: usize,
    g: f32,
    gain: f32,
}

impl Allpass {
    fn new(len: usize, g: f32, gain: f32) -> Self {
        Self { buf: vec![0.0; len.max(1)], at: 0, g, gain }
    }

    fn process(&mut self, x: f32) -> f32 {
        let d = flush(self.buf[self.at]);
        let v = flush(x - self.g * d);
        self.buf[self.at] = v;
        self.at = (self.at + 1) % self.buf.len();
        (self.g * v + d) * self.gain
    }
}

fn is_prime(n: usize) -> bool {
    n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0)
}

/// Speed of sound of ReverbModel1 (the plugin descriptor at 0x14B22E8 holds 344.8 m/s, used by
/// 0x1139970 to turn the comb lengths from metres into samples).
pub const REVERB_SPEED_OF_SOUND: f32 = 344.8;

/// The six comb lengths of the space in metres (VERIFIED, the parameter update 0x113A030): the size is
/// clamped to 2 .. 83.3 m, the lengths run from 0.8 x size to 1.2 x size in five equal steps (a size
/// whose 1.2 x exceeds 100 m is replaced by 66.67 .. 100 m).
pub fn comb_metres(size: f32) -> [f32; 6] {
    let size = size.clamp(2.0, 83.3);
    let (lo, hi) = if size * 0.8 * 1.5 > 100.0 { (66.666_67, 100.0) } else { (size * 0.8, size * 0.8 * 1.5) };
    let step = (hi - lo) * 0.2;
    [lo, lo + step, lo + 2.0 * step, lo + 3.0 * step, lo + 4.0 * step, hi]
}

/// A comb length in samples (VERIFIED, 0x1139970): `metres / 344.8 * min(rate, 48000)` rounded up to the
/// next prime of the plugin's table (the primes below 14000, 0x14B23E0), times `rate / 48000` above
/// 48 kHz.
pub fn comb_samples(metres: f32, rate: f32) -> usize {
    let t = metres / REVERB_SPEED_OF_SOUND * rate.min(48_000.0);
    let mut n = t.floor().max(1.0) as usize;
    while !(is_prime(n) && n as f32 > t) {
        n += 1;
    }
    if rate > 48_000.0 {
        (n as f32 * (rate / 48_000.0)) as usize
    } else {
        n
    }
}

/// The feedback tables of ReverbModel1 (`.rdata` 0x14B232C, one row per rate of 10 / 25 / 50 kHz, each
/// the values at the lengths 0, 12.5, .. 100 m of [`FEEDBACK_KNOTS`]; the knots sit between the rows).
const FEEDBACK_CURVE: [[f32; 9]; 3] = [
    [0.0, 0.03, 0.07, 0.11, 0.14, 0.16, 0.18, 0.22, 0.24],
    [0.0, 0.12, 0.23, 0.30, 0.35, 0.40, 0.43, 0.46, 0.50],
    [0.0, 0.31, 0.45, 0.53, 0.57, 0.61, 0.64, 0.67, 0.70],
];
/// The comb lengths in metres of the table columns (0x14B230C .. 0x14B2328 and 100).
const FEEDBACK_KNOTS: [f32; 9] = [0.0, 12.5, 25.0, 37.5, 50.0, 62.5, 75.0, 87.5, 100.0];

/// The feedback curve at a sample rate (VERIFIED, the first half of 0x1139A30): the two table rows next
/// to the rate are mixed with the weights `(25000 - rate) / 15000` (10 / 25 kHz) or `(50000 - rate) / 25000`
/// (25 / 50 kHz), the rate clamped to 10 000 .. 50 000.
pub fn feedback_curve(rate: f32) -> [f32; 9] {
    let rate = rate.clamp(10_000.0, 50_000.0);
    let (row, w) = if rate > 25_000.0 {
        (1, (50_000.0 - rate) * f32::from_bits(0x3827_C5AC))
    } else {
        (0, (25_000.0 - rate) * f32::from_bits(0x388B_CF65))
    };
    let rest = 1.0 - w;
    let mut out = [0.0f32; 9];
    for (k, v) in out.iter_mut().enumerate() {
        *v = FEEDBACK_CURVE[row][k] * w + FEEDBACK_CURVE[row + 1][k] * rest;
    }
    out
}

/// The six comb values `c_j` (VERIFIED, 0x1139A30): the curve interpolated linearly at each comb length
/// in metres (`this+0x164`), then divided by the brightness (`this+0x38`, raised first to the
/// longest comb's value + 0.001). The plugin divides only on the first update after a change of the
/// brightness (0x1139CB7); the Cell never sets it (default 1).
pub fn comb_poles(metres: &[f32; 6], rate: f32, brightness: f32) -> [f32; 6] {
    let curve = feedback_curve(rate);
    let mut out = [0.0f32; 6];
    for (o, p) in out.iter_mut().zip(metres) {
        // First knot at or above the length (at least the second one).
        let i = (1..8).find(|i| FEEDBACK_KNOTS[*i] >= *p).unwrap_or(8);
        let t = (FEEDBACK_KNOTS[i] - p) * 0.08;
        *o = (1.0 - t) * curve[i] + curve[i - 1] * t;
    }
    let brightness = brightness.max(out[5] + 0.001);
    for o in &mut out {
        *o /= brightness;
    }
    out
}

/// The six loop gains (VERIFIED, 0x1139DB0): `(1 - c_j)(1 - 0.366 / reverbtime)`, the reverb time
/// raised to 0.366 s first. With the one-pole of pole `c_j` in the loop the DC loop gain is
/// `1 - 0.366 / reverbtime` for every comb.
pub fn comb_damping(poles: &[f32; 6], time: f32) -> [f32; 6] {
    let k = 1.0 - 0.366 / time.max(0.366);
    poles.map(|c| (1.0 - c) * k)
}

/// ReverbModel1 (the `reverb` submix, RWAudioCore): six parallel feedback combs ([`Comb`]) into one
/// all-pass per output channel ([`Allpass`]). VERIFIED end to end (constructor 0x1139E60, parameter
/// update 0x113A030, delay lengths 0x1139970, feedback / damping 0x1139A30 / 0x1139DB0, the block
/// process 0x114D450 that runs the six sections on the mono input into one buffer (the first
/// section writes, the others accumulate, kernel 0x114DAE0) and then the channel all-pass nodes
/// 0x114E220 on that buffer: the left channel is the node of feedback 0.63 and 6.667 ms, the right
/// one 0.7778 and 5.4 ms, "slightly different parameters ... on one output channel versus another"
/// (the documentation); the output gain 2 / channels = 1). Parameters: `time` = `this+0x28`
/// (`reverbtime`), `size` = `this+0x30` (`reverbsize`), `brightness` = `this+0x38`.
pub struct Reverb {
    combs: Vec<Comb>,
    left: Allpass,
    right: Allpass,
}

impl Reverb {
    pub fn new(rate: f32, time: f32, size: f32, brightness: f32) -> Self {
        let metres = comb_metres(size);
        let poles = comb_poles(&metres, rate, brightness);
        let damps = comb_damping(&poles, time);
        let combs = (0..6).map(|j| Comb::new(comb_samples(metres[j], rate), poles[j], damps[j])).collect();
        // cvtss2si (round to nearest) of rate x 0.006666667 / 0.0054 (0x113A39A.. / 0x113A411..).
        let ap = |seconds: f32, g: f32| Allpass::new((seconds * rate).round() as usize, g, 2.0 / 2.0);
        Self { combs, left: ap(0.006_666_667, 0.63), right: ap(0.0054, 0.777_777_8) }
    }

    pub fn process(&mut self, x: f32) -> [f32; 2] {
        let mut sum = 0.0;
        for comb in &mut self.combs {
            sum += comb.process(x);
        }
        [self.left.process(sum), self.right.process(sum)]
    }
}

/// The reverb voice as an asset.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct ReverbAsset {
    pub bus: Arc<ReverbBus>,
}

impl Decodable for ReverbAsset {
    type Decoder = ReverbSource;
    fn decoder(&self) -> Self::Decoder {
        ReverbSource {
            bus: self.bus.clone(),
            reverb: Reverb::new(REVERB_RATE as f32, REVERB_TIME, REVERB_SIZE, 1.0),
            frame: [0.0; 2],
            idx: 2,
        }
    }
}

/// Never ends: the reverb tail of the `reverb` submix.
pub struct ReverbSource {
    bus: Arc<ReverbBus>,
    reverb: Reverb,
    frame: [f32; 2],
    idx: usize,
}

impl Iterator for ReverbSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.idx >= 2 {
            self.frame = self.reverb.process(self.bus.take());
            self.idx = 0;
        }
        let s = self.frame[self.idx];
        self.idx += 1;
        Some(s)
    }
}

impl bevy::audio::Source for ReverbSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        NonZero::new(REVERB_RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Marker of the reverb voice entity.
#[derive(Component)]
pub struct ReverbVoice;

/// Keep one reverb voice running in the Cell (the `reverb` submix exists for the whole run).
pub fn ensure_reverb(
    mut commands: Commands,
    reverb: Res<CellReverb>,
    assets: Option<ResMut<Assets<ReverbAsset>>>,
    existing: Query<(), With<ReverbVoice>>,
) {
    let Some(mut assets) = assets else { return };
    if !existing.is_empty() {
        return;
    }
    let handle = assets.add(ReverbAsset { bus: reverb.0.clone() });
    commands.spawn((Name::new("cell reverb"), AudioPlayer(handle), PlaybackSettings::DESPAWN, ReverbVoice, StageOwned));
}

// ---------------------------------------------------------------------------
// The voice
// ---------------------------------------------------------------------------

/// A decoded WAV (PCM16, 44 byte header as `audio::write_wav` writes it) as a voice.
#[derive(Asset, TypePath, Clone)]
pub struct SpatialAsset {
    pub wav: Arc<[u8]>,
    pub ctl: Arc<SpatialCtl>,
    pub looped: bool,
    /// Pitch (the soundProp's x the play's): the read step through the samples.
    pub speed: f32,
    pub reverb: Option<Arc<ReverbBus>>,
}

impl Decodable for SpatialAsset {
    type Decoder = SpatialSource;
    fn decoder(&self) -> Self::Decoder {
        SpatialSource::new(self.clone())
    }
}

/// How fast the audio thread follows the controls (per frame; about 10 ms at 44.1 kHz).
const SMOOTH: f32 = 0.0025;

/// The rodio source of a [`SpatialAsset`]: always stereo, at the sample's rate.
pub struct SpatialSource {
    a: SpatialAsset,
    channels: usize,
    rate: u32,
    frames: usize,
    pos: f64,
    gain: f32,
    pan: [f32; 4],
    lp: [Biquad; 2],
    hp: [Biquad; 2],
    lp_hz: f32,
    hp_hz: f32,
    counter: u32,
    wet_phase: f64,
    frame: [f32; 2],
    idx: usize,
    done: bool,
}

impl SpatialSource {
    pub fn new(a: SpatialAsset) -> Self {
        let wav = &a.wav;
        let (channels, rate, frames) = if wav.len() >= 44 {
            let channels = u16::from_le_bytes([wav[22], wav[23]]).clamp(1, 2) as usize;
            let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]).max(8000);
            (channels, rate, (wav.len() - 44) / (2 * channels))
        } else {
            (1, 22_050, 0)
        };
        let gain = load(&a.ctl.gain);
        let pan = [0, 1, 2, 3].map(|k| load(&a.ctl.pan[k]));
        Self {
            a,
            channels,
            rate,
            frames,
            pos: 0.0,
            gain,
            pan,
            lp: [Biquad::default(); 2],
            hp: [Biquad::default(); 2],
            lp_hz: -1.0,
            hp_hz: -1.0,
            counter: 0,
            wet_phase: 0.0,
            frame: [0.0; 2],
            idx: 2,
            done: false,
        }
    }

    fn sample(&self, frame: usize, channel: usize) -> f32 {
        let at = 44 + 2 * (frame * self.channels + channel.min(self.channels - 1));
        i16::from_le_bytes([self.a.wav[at], self.a.wav[at + 1]]) as f32 / 32768.0
    }

    fn render(&mut self) -> bool {
        if self.done || self.frames == 0 || self.a.ctl.stop.load(Ordering::Relaxed) {
            self.done = true;
            return false;
        }
        let rate = self.rate as f32;
        if self.counter % 32 == 0 {
            let lp = load(&self.a.ctl.lowpass);
            if lp != self.lp_hz {
                self.lp_hz = lp;
                for f in &mut self.lp {
                    f.design(lp, rate, false);
                }
            }
            let hp = load(&self.a.ctl.hipass);
            if hp != self.hp_hz {
                self.hp_hz = hp;
                for f in &mut self.hp {
                    f.design(hp, rate, true);
                }
            }
        }
        self.counter = self.counter.wrapping_add(1);
        self.gain += (load(&self.a.ctl.gain) - self.gain) * SMOOTH;
        for k in 0..4 {
            self.pan[k] += (load(&self.a.ctl.pan[k]) - self.pan[k]) * SMOOTH;
        }
        // Read with linear interpolation (the pitch is the step).
        let i0 = (self.pos as usize).min(self.frames - 1);
        let frac = (self.pos - i0 as f64) as f32;
        let i1 = if i0 + 1 < self.frames { i0 + 1 } else if self.a.looped { 0 } else { i0 };
        let mut x = [0.0f32; 2];
        for (c, out) in x.iter_mut().enumerate().take(self.channels) {
            let s = self.sample(i0, c);
            let s = s + (self.sample(i1, c) - s) * frac;
            *out = self.hp[c].run(self.lp[c].run(s));
        }
        let (l, r) = if self.channels == 1 {
            (x[0] * self.pan[0], x[0] * self.pan[1])
        } else {
            (x[0] * self.pan[0] + x[1] * self.pan[2], x[0] * self.pan[1] + x[1] * self.pan[3])
        };
        self.frame = [l * self.gain, r * self.gain];
        // The send into the reverb: `wetlevel` x the level, one slot per reverb frame. VERIFIED
        // (cell-final5): the chain is `... Pn21 Sen0[reverb] Sen0`, so the send takes the panner's two
        // output channels; the `reverb` submix record has `numchannels` 1 (0xED4C265F), and the send
        // plugin (0x114CBD0) folds two channels into one with the mixer table 0x14DD49A (index
        // destination channels + 6 x source channels, entry bytes at 0x14DD4F0: both sources into
        // destination 0 at coefficient 1.0, 0x115D4E0), i.e. L + R of the panner at the send gain.
        // The panner's gains here carry the extra sqrt 2 of `pan_gains`; retail's do not.
        let wet = load(&self.a.ctl.wet);
        if let (Some(bus), true) = (&self.a.reverb, wet > 0.0) {
            let send = (l + r) * SQRT_2.recip() * self.gain * wet;
            let before = self.wet_phase.floor() as usize;
            self.wet_phase += REVERB_RATE as f64 / self.rate as f64;
            let slots = self.wet_phase.floor() as usize - before;
            for k in 0..slots {
                bus.send(k, send);
            }
            self.wet_phase -= before as f64;
        }
        self.pos += self.a.speed.clamp(0.1, 4.0) as f64;
        if self.pos >= self.frames as f64 {
            if self.a.looped {
                self.pos %= self.frames as f64;
            } else {
                // The frame just rendered still plays.
                self.done = true;
            }
        }
        true
    }
}

impl Iterator for SpatialSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.idx >= 2 {
            if !self.render() {
                return None;
            }
            self.idx = 0;
        }
        let s = self.frame[self.idx];
        self.idx += 1;
        Some(s)
    }
}

impl bevy::audio::Source for SpatialSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        NonZero::new(self.rate).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// The state of a playing positional voice that [`drive_spatial`] needs.
#[derive(Component)]
pub struct SpatialVoice {
    ctl: Arc<SpatialCtl>,
    loaded: Arc<Loaded>,
    anchor: Anchor,
    /// Last known source position (world plane, world units).
    position: Vec2,
    /// Every distance independent factor of the level (bus x soundProp gain x play gain).
    base: f32,
    stereo: bool,
    /// The (min, max) roll-off distance the voice's state curves gave (`None`: the soundProp's own).
    range: Option<(f32, f32)>,
    /// Seconds since an effect anchor's position was refreshed.
    since: f32,
}

/// What a positional voice plays.
pub struct VoiceSpec<'a> {
    pub loaded: &'a Arc<Loaded>,
    /// Which clip of the sound.
    pub pick: usize,
    pub looped: bool,
    /// The soundProp's pitch x the play's.
    pub speed: f32,
    pub anchor: Anchor,
    /// Bus x soundProp gain x play gain.
    pub base: f32,
    /// The Pd mixers' level of the sound's groups (re-read every frame afterwards).
    pub mixer: f32,
}

/// Starts positional voices (assets, the reverb bus and the listener / source lookups).
#[derive(SystemParam)]
pub struct SpatialStart<'w, 's> {
    pub ears: CellEars<'w, 's>,
    assets: Option<ResMut<'w, Assets<SpatialAsset>>>,
    reverb: Option<Res<'w, CellReverb>>,
}

impl SpatialStart<'_, '_> {
    /// Spawn a positional voice. `None` (play the flat way) without a listener, a source position,
    /// the audio assets (headless) or the decoded WAV (flat sounds keep none).
    pub fn start(&mut self, commands: &mut Commands, label: &str, spec: VoiceSpec) -> Option<Entity> {
        self.start_ranged(commands, label, spec, None)
    }

    /// [`SpatialStart::start`] for a voice with its own roll-off distances (`range`).
    pub fn start_ranged(
        &mut self,
        commands: &mut Commands,
        label: &str,
        spec: VoiceSpec,
        range: Option<(f32, f32)>,
    ) -> Option<Entity> {
        let listener = self.ears.listener()?;
        let position = self.ears.position(&spec.anchor).unwrap_or_else(|| spec.anchor.fallback());
        let wav = spec.loaded.raw.get(spec.pick)?.clone();
        let assets = self.assets.as_deref_mut()?;
        let stereo = wav.len() >= 24 && u16::from_le_bytes([wav[22], wav[23]]) >= 2;
        let offset = (position - listener) / UNIT;
        let state = voice_state(&spec.loaded.def, &spec.loaded.acoustics, offset, spec.base * spec.mixer, stereo, range);
        let ctl = Arc::new(SpatialCtl::new(&state));
        let handle = assets.add(SpatialAsset {
            wav,
            ctl: ctl.clone(),
            looped: spec.looped,
            speed: spec.speed,
            reverb: self.reverb.as_ref().map(|r| r.0.clone()),
        });
        Some(
            commands
                .spawn((
                    Name::new(format!("cell sfx {label}")),
                    AudioPlayer(handle),
                    PlaybackSettings::DESPAWN,
                    SpatialVoice {
                        ctl,
                        loaded: spec.loaded.clone(),
                        anchor: spec.anchor,
                        position,
                        base: spec.base,
                        stereo,
                        range,
                        since: 0.0,
                    },
                    CellVoice,
                    StageOwned,
                ))
                .id(),
        )
    }
}

/// Every frame: re-evaluate the retail track update for every positional voice from its source's
/// current position and the listener (the avatar moves, the sources move).
pub fn drive_spatial(
    time: Res<Time<Real>>,
    audio: Res<GameAudio>,
    ears: CellEars,
    pd: Option<Res<crate::pd_audio::PdMusic>>,
    mut voices: Query<&mut SpatialVoice>,
) {
    let Some(listener) = ears.listener() else { return };
    let dt = time.delta_secs();
    for mut v in &mut voices {
        if let Some(at) = ears.position(&v.anchor) {
            match v.anchor {
                Anchor::Effect(_, period, _) => {
                    v.since += dt;
                    if v.since >= period {
                        v.since = 0.0;
                        v.position = at;
                    }
                }
                _ => v.position = at,
            }
        }
        let mixer = pd.as_ref().map_or(1.0, |pd| pd.group_gain(&v.loaded.def.groups));
        let offset = (v.position - listener) / UNIT;
        let mut state = voice_state(&v.loaded.def, &v.loaded.acoustics, offset, v.base * mixer, v.stereo, v.range);
        if audio.muted {
            state.gain = 0.0;
        }
        v.ctl.set(&state);
    }
}

pub struct CellSpatialPlugin;

impl Plugin for CellSpatialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CellReverb>();
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            use bevy::audio::AddAudioSource;
            app.add_audio_source::<SpatialAsset>().add_audio_source::<ReverbAsset>();
        }
        app.add_systems(
            Update,
            (ensure_reverb, drive_spatial)
                .chain()
                .run_if(in_state(GameStage::Cell))
                .run_if(resource_exists::<GameAudio>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_wav(hz: f32, secs: f32, channels: usize, rate: u32) -> Arc<[u8]> {
        let frames = (secs * rate as f32) as usize;
        let mut pcm = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            let s = ((i as f32 * hz * TAU / rate as f32).sin() * 12_000.0) as i16;
            for _ in 0..channels {
                pcm.push(s);
            }
        }
        crate::audio::write_wav(&pcm, channels, rate).into()
    }

    fn state(gain: f32, pan: [[f32; 2]; 2], lowpass: f32, wet: f32) -> VoiceState {
        VoiceState { gain, pan, lowpass, hipass: 0.0, wet }
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    fn channel(samples: &[f32], c: usize) -> Vec<f32> {
        samples.chunks(2).map(|f| f[c]).collect()
    }

    fn asset(wav: Arc<[u8]>, ctl: &Arc<SpatialCtl>, looped: bool, bus: Option<Arc<ReverbBus>>) -> SpatialAsset {
        SpatialAsset { wav, ctl: ctl.clone(), looped, speed: 1.0, reverb: bus }
    }

    #[test]
    fn a_voice_follows_its_controls_while_it_plays() {
        let ctl = Arc::new(SpatialCtl::new(&state(1.0, [[1.0, 1.0], [0.0; 2]], 0.0, 0.0)));
        let mut src = asset(tone_wav(2_000.0, 4.0, 1, 44_100), &ctl, true, None).decoder();
        let a: Vec<f32> = (&mut src).take(2 * 8_000).collect();
        assert!(rms(&channel(&a, 0)) > 0.2 && (rms(&channel(&a, 0)) - rms(&channel(&a, 1))).abs() < 1e-3, "centred");
        // The source moves to the right and far: right only, low-passed at 300 Hz.
        ctl.set(&state(1.0, [[0.0, 1.4], [0.0; 2]], 300.0, 0.0));
        let _settle: Vec<f32> = (&mut src).take(2 * 10_000).collect();
        let b: Vec<f32> = (&mut src).take(2 * 8_000).collect();
        assert!(rms(&channel(&b, 0)) < 0.01, "left is silent");
        assert!(rms(&channel(&b, 1)) < rms(&channel(&a, 1)) * 0.2, "2 kHz is gone behind a 300 Hz low-pass: {}", rms(&channel(&b, 1)));
        // And back to a clean right-hand voice: the right channel carries it all.
        ctl.set(&state(1.0, [[0.0, 1.4], [0.0; 2]], 0.0, 0.0));
        let _settle: Vec<f32> = (&mut src).take(2 * 10_000).collect();
        let c: Vec<f32> = (&mut src).take(2 * 8_000).collect();
        assert!(rms(&channel(&c, 1)) > rms(&channel(&a, 1)) * 1.3 && rms(&channel(&c, 0)) < 0.01);
    }

    #[test]
    fn a_one_shot_ends_and_pitch_changes_its_length() {
        let ctl = Arc::new(SpatialCtl::new(&state(1.0, [[1.0, 1.0], [0.0; 2]], 0.0, 0.0)));
        let wav = tone_wav(500.0, 1.0, 1, 22_050);
        let n = asset(wav.clone(), &ctl, false, None).decoder().count() / 2;
        assert!((22_050..22_060).contains(&n), "{n}");
        let mut fast = asset(wav, &ctl, false, None);
        fast.speed = 2.0;
        let n = fast.decoder().count() / 2;
        assert!((11_020..11_040).contains(&n), "{n}");
        // A stopped voice ends at once.
        let mut src = asset(tone_wav(500.0, 1.0, 1, 22_050), &ctl, true, None).decoder();
        assert!((&mut src).take(100).count() == 100);
        ctl.stop();
        assert!(src.next().is_none());
    }

    #[test]
    fn a_stereo_sample_goes_through_the_two_input_rows() {
        // Left channel only (silence on the right): row 0 to the left speaker, row 1 to the right.
        let frames = 4_000;
        let mut pcm = Vec::new();
        for i in 0..frames {
            pcm.push(((i as f32 * 0.2).sin() * 12_000.0) as i16);
            pcm.push(0);
        }
        let wav: Arc<[u8]> = crate::audio::write_wav(&pcm, 2, 44_100).into();
        let ctl = Arc::new(SpatialCtl::new(&state(1.0, [[1.0, 0.0], [0.0, 1.0]], 0.0, 0.0)));
        let out: Vec<f32> = asset(wav, &ctl, false, None).decoder().collect();
        assert!(rms(&channel(&out, 0)) > 0.1 && rms(&channel(&out, 1)) < 1e-3);
    }

    #[test]
    fn the_send_feeds_the_reverb_and_the_reverb_rings_on() {
        let bus = Arc::new(ReverbBus::default());
        let ctl = Arc::new(SpatialCtl::new(&state(1.0, [[1.0, 1.0], [0.0; 2]], 0.0, 0.5)));
        // A short burst at 22.05 kHz (two reverb frames per voice frame).
        let mut src = asset(tone_wav(800.0, 0.1, 1, 22_050), &ctl, false, Some(bus.clone())).decoder();
        let mut reverb = ReverbAsset { bus: bus.clone() }.decoder();
        // The output mixer pulls both: two reverb frames per voice frame.
        let (mut dry, mut first) = (vec![], vec![]);
        while let (Some(l), Some(r)) = (src.next(), src.next()) {
            dry.extend([l, r]);
            first.extend((&mut reverb).take(4));
        }
        assert!(rms(&dry) > 0.1);
        // The reverb voice reads what was sent and keeps sounding long after the burst ended.
        first.extend((&mut reverb).take(2 * (12_000 - first.len() / 2)));
        let peak = first.chunks(2 * 2_000).map(rms).fold(0.0, f32::max);
        let late: Vec<f32> = (&mut reverb).skip(2 * 8_000).take(2 * 4_000).collect();
        assert!(peak > 0.005, "the sent burst arrives: {peak}");
        assert!(rms(&late) > 1e-3 && rms(&late) < peak, "a decaying tail: {} vs {peak}", rms(&late));
        // No send, no reverb.
        let quiet = Arc::new(ReverbBus::default());
        let none = Arc::new(SpatialCtl::new(&state(1.0, [[1.0, 1.0], [0.0; 2]], 0.0, 0.0)));
        let mut dry = asset(tone_wav(800.0, 0.1, 1, 22_050), &none, false, Some(quiet.clone())).decoder();
        let mut wet = ReverbAsset { bus: quiet }.decoder();
        let mut silent = vec![];
        while dry.next().is_some() {
            silent.extend((&mut wet).take(2));
        }
        assert_eq!(rms(&silent), 0.0);
    }

    #[test]
    fn the_send_is_the_sum_of_the_panned_pair_at_the_send_gain() {
        // `pan_gains` carries an extra sqrt 2 over Pan2D1; the send folds the panner's left and right
        // output into the mono `reverb` submix by adding them (0x115D4E0, table 0x14DD49A), so a
        // centred mono voice (0.7071 per side in retail) sends 1.414 x, a hard-right one 1.0 x.
        let sent = |pan: [[f32; 2]; 2]| -> f32 {
            let bus = Arc::new(ReverbBus::default());
            let ctl = Arc::new(SpatialCtl::new(&state(1.0, pan, 0.0, 1.0)));
            let mut src = asset(tone_wav(800.0, 0.05, 1, 22_050), &ctl, false, Some(bus.clone())).decoder();
            let mut total = 0.0f32;
            while let (Some(_), Some(_)) = (src.next(), src.next()) {
                total += bus.take().abs() + bus.take().abs();
            }
            total + (0..4).map(|_| bus.take().abs()).sum::<f32>()
        };
        let centre = sent([[SQRT_2 * 0.5f32.sqrt(); 2], [0.0; 2]]);
        let right = sent([[0.0, SQRT_2], [0.0; 2]]);
        assert!(centre > 0.1 && right > 0.1, "{centre} {right}");
        let ratio = centre / right;
        assert!((ratio - SQRT_2).abs() < 0.02, "centre / hard right = {ratio}");
    }

    /// The retail difference equations written out over whole arrays (the decoded kernels 0x114DAE0 and
    /// 0x114DD90): the reference for the ring-buffer implementation.
    fn reference_reverb(rate: f32, time: f32, size: f32, input: &[f32]) -> Vec<[f32; 2]> {
        let metres = comb_metres(size);
        let poles = comb_poles(&metres, rate, 1.0);
        let damps = comb_damping(&poles, time);
        let n = input.len();
        let mut sum = vec![0.0f32; n];
        for j in 0..6 {
            let d = comb_samples(metres[j], rate);
            let mut u = vec![0.0f32; n];
            // u[m] is zero for m < 0.
            let at = |u: &Vec<f32>, m: isize| if m < 0 { 0.0 } else { u[m as usize] };
            for i in 0..n {
                let l1 = at(&u, i as isize - d as isize);
                let t = input[i] + damps[j] * l1;
                let prev = at(&u, i as isize - 1);
                u[i] = t + poles[j] * prev;
            }
            for i in 0..n {
                let l1 = at(&u, i as isize - d as isize);
                let l0 = at(&u, i as isize - d as isize - 1);
                sum[i] += (l1 - poles[j] * l0) / 6.0;
            }
        }
        let allpass = |len: usize, g: f32| {
            let mut v = vec![0.0f32; n];
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                let d = if i >= len { v[i - len] } else { 0.0 };
                v[i] = sum[i] - g * d;
                out[i] = g * v[i] + d;
            }
            out
        };
        let left = allpass((0.006_666_667 * rate).round() as usize, 0.63);
        let right = allpass((0.0054 * rate).round() as usize, 0.777_777_8);
        (0..n).map(|i| [left[i], right[i]]).collect()
    }

    #[test]
    fn the_reverb_feedback_tables_are_the_plugin_rows() {
        // 0x1139A30: the rate picks and mixes the rows of 0x14B232C (10 / 25 / 50 kHz).
        assert_eq!(feedback_curve(10_000.0), FEEDBACK_CURVE[0]);
        assert_eq!(feedback_curve(5_000.0), FEEDBACK_CURVE[0], "clamped below 10 kHz");
        assert_eq!(feedback_curve(50_000.0), FEEDBACK_CURVE[2]);
        assert_eq!(feedback_curve(96_000.0), FEEDBACK_CURVE[2], "clamped above 50 kHz");
        assert_eq!(feedback_curve(25_000.0), FEEDBACK_CURVE[1]);
        let mid = feedback_curve(37_500.0);
        for k in 0..9 {
            assert!((mid[k] - 0.5 * (FEEDBACK_CURVE[1][k] + FEEDBACK_CURVE[2][k])).abs() < 1e-6);
        }
        let low = feedback_curve(17_500.0);
        for k in 0..9 {
            assert!((low[k] - 0.5 * (FEEDBACK_CURVE[0][k] + FEEDBACK_CURVE[1][k])).abs() < 1e-6);
        }
        // Interpolation in the length: at a knot the table value, in between the straight line.
        let poles = comb_poles(&[12.5, 25.0, 37.5, 50.0, 62.5, 100.0], 25_000.0, 1.0);
        for (p, want) in poles.iter().zip([0.12, 0.23, 0.30, 0.35, 0.40, 0.50]) {
            assert!((p - want).abs() < 1e-6, "{poles:?}");
        }
        let between = comb_poles(&[18.75; 6], 25_000.0, 1.0);
        assert!((between[0] - 0.175).abs() < 1e-6);
        // The Cell's reverb (30 m at 44.1 kHz): lengths 24 .. 36 m give 0.387 .. 0.466.
        let cell = comb_poles(&comb_metres(REVERB_SIZE), REVERB_RATE as f32, 1.0);
        assert!((cell[0] - 0.3873).abs() < 2e-3 && (cell[5] - 0.4664).abs() < 2e-3, "{cell:?}");
        assert!(cell.windows(2).all(|w| w[0] < w[1]));
        // A brightness below the longest comb's value + 0.001 is raised to it (0x1139CD6); above, it divides.
        let raised = comb_poles(&comb_metres(REVERB_SIZE), REVERB_RATE as f32, 0.1);
        assert!((raised[5] - cell[5] / (cell[5] + 0.001)).abs() < 1e-5);
        let darker = comb_poles(&comb_metres(REVERB_SIZE), REVERB_RATE as f32, 2.0);
        assert!((darker[0] - cell[0] / 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_comb_loop_gain_is_one_minus_the_floor_over_the_reverb_time() {
        // 0x1139DB0: damp_j = (1 - c_j)(1 - 0.366 / T); the one-pole of pole c_j in the loop has a DC gain of
        // 1 / (1 - c_j), so the DC loop gain is 1 - 0.366 / T for every comb.
        let poles = comb_poles(&comb_metres(REVERB_SIZE), REVERB_RATE as f32, 1.0);
        let damps = comb_damping(&poles, REVERB_TIME);
        for (c, d) in poles.iter().zip(damps) {
            assert!((d / (1.0 - c) - (1.0 - 0.366 / 2.5)).abs() < 1e-5);
        }
        // The reverb time is raised to 0.366 s: no feedback left at all.
        for d in comb_damping(&poles, 0.1) {
            assert_eq!(d, 0.0);
        }
    }

    #[test]
    fn the_reverb_runs_the_decoded_difference_equations() {
        let rate = REVERB_RATE as f32;
        let mut seed = 7u32;
        let input: Vec<f32> = (0..30_000)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if i == 0 {
                    1.0
                } else if i % 7919 == 0 {
                    (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
                } else {
                    0.0
                }
            })
            .collect();
        let reference = reference_reverb(rate, REVERB_TIME, REVERB_SIZE, &input);
        let mut reverb = Reverb::new(rate, REVERB_TIME, REVERB_SIZE, 1.0);
        let mut worst = 0.0f32;
        for (i, x) in input.iter().enumerate() {
            let got = reverb.process(*x);
            for c in 0..2 {
                worst = worst.max((got[c] - reference[i][c]).abs());
            }
        }
        assert!(worst < 1e-5, "ring buffers against the arrays: {worst}");
        // The first echo: comb 0 answers t[0] / 6 after D_0 samples (nothing earlier), through the
        // output all-pass of each channel (y = g v on the first sample: 0.63 left, 0.7778 right).
        let d0 = comb_samples(comb_metres(REVERB_SIZE)[0], rate);
        assert_eq!(d0, 3079);
        assert!(reference[..d0].iter().all(|f| f[0] == 0.0 && f[1] == 0.0));
        assert!((reference[d0][0] - 0.63 / 6.0).abs() < 1e-6 && (reference[d0][1] - 0.777_777_8 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn the_reverb_energy_and_decay_follow_the_transfer_function() {
        // Parseval: the energy of the impulse response equals the mean of |H|^2 over the circle, with
        // H = sum_j z^-D_j (1 - c_j z^-1) / (1 - c_j z^-1 - damp_j z^-D_j) / 6 (the all-passes are unity).
        let rate = REVERB_RATE as f32;
        let metres = comb_metres(REVERB_SIZE);
        let poles = comb_poles(&metres, rate, 1.0);
        let damps = comb_damping(&poles, REVERB_TIME);
        let lens: Vec<usize> = metres.iter().map(|m| comb_samples(*m, rate)).collect();
        let points = 1usize << 19;
        let mut mean = 0.0f64;
        for k in 0..points {
            let w = std::f64::consts::TAU * (k as f64 + 0.5) / points as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for j in 0..6 {
                let (c, d) = (poles[j] as f64, damps[j] as f64);
                let dz = (-w * lens[j] as f64).sin_cos();
                let (dre, dim) = (dz.1, dz.0);
                let z1 = ((-w).cos(), (-w).sin());
                // numerator z^-D (1 - c z^-1); denominator 1 - c z^-1 - damp z^-D
                let (nre, nim) = (1.0 - c * z1.0, -c * z1.1);
                let (zre, zim) = (dre * nre - dim * nim, dre * nim + dim * nre);
                let (ere, eim) = (1.0 - c * z1.0 - d * dre, -c * z1.1 - d * dim);
                let q = ere * ere + eim * eim;
                re += (zre * ere + zim * eim) / q / 6.0;
                im += (zim * ere - zre * eim) / q / 6.0;
            }
            mean += re * re + im * im;
        }
        mean /= points as f64;
        let mut reverb = Reverb::new(rate, REVERB_TIME, REVERB_SIZE, 1.0);
        let n = (rate * 14.0) as usize;
        let mut energy = vec![0.0f32; n / 2205 + 1];
        let mut total = 0.0f64;
        for i in 0..n {
            let [l, _r] = reverb.process(if i == 0 { 1.0 } else { 0.0 });
            energy[i / 2205] += l * l;
            total += (l * l) as f64;
        }
        assert!((total / mean - 1.0).abs() < 0.02, "impulse energy {total} against the transfer function {mean}");
        // The tail: -60 dB (energy) after about the reverb time; the DC loop gain 1 - 0.366 / T per
        // pass of 0.07 .. 0.09 s decays the lows slower than T, the one-pole damping the highs faster.
        let peak = energy.iter().cloned().fold(0.0, f32::max);
        let loudest = energy.iter().position(|e| *e == peak).unwrap_or(0);
        let at = energy.iter().skip(loudest).position(|e| *e < peak * 1e-6).map(|w| (w + loudest) as f32 * 0.05).unwrap_or(99.0);
        assert!((1.2..4.5).contains(&at), "-60 dB after {at} s");
    }

    #[test]
    fn filters_clamp_like_the_platform() {
        let mut lp = Biquad::default();
        lp.design(0.0, 44_100.0, false);
        assert!(!lp.on, "0 means open here");
        lp.design(22_000.0, 44_100.0, false);
        assert!(lp.on, "99.8 % of Nyquist is still filtered (the limit is w = 3.138451 = 99.9 %)");
        lp.design(22_045.0, 44_100.0, false);
        assert!(!lp.on, "above 99.9 % of Nyquist: bypass");
        lp.design(5.0, 44_100.0, false);
        assert!(lp.on, "clamped to 0.1 % of Nyquist (0x1153570), not bypassed");
        let mut hp = Biquad::default();
        hp.design(0.0, 44_100.0, true);
        assert!(!hp.on);
        hp.design(10.0, 44_100.0, true);
        assert!(!hp.on, "a high-pass at or below 0.1 % of Nyquist is bypassed (0x1153ED0)");
        hp.design(30_000.0, 44_100.0, true);
        assert!(hp.on, "above Nyquist: clamped, stays on");
    }

    /// |H(e^jw)| of a section.
    fn magnitude(f: &Biquad, w: f32) -> f32 {
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num = (f.b[0] + f.b[1] * c1 + f.b[2] * c2, -(f.b[1] * s1 + f.b[2] * s2));
        let den = (1.0 + f.a[0] * c1 + f.a[1] * c2, -(f.a[0] * s1 + f.a[1] * s2));
        ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1)).sqrt()
    }

    #[test]
    fn the_filters_are_the_rbj_design_at_q_one() {
        // LowPassIir2 / HighPassIir2 (0x11534D0 / 0x1153E30): alpha = sin(w) / 2, so |H| = Q = 1 at
        // the cutoff (not the Butterworth 0.707), unity in the pass band, 12 dB per octave beyond.
        let rate = 44_100.0;
        let w0 = TAU * 500.0 / rate;
        let mut lp = Biquad::default();
        lp.design(500.0, rate, false);
        assert!((magnitude(&lp, 1e-3) - 1.0).abs() < 1e-3, "unity at DC");
        assert!((magnitude(&lp, w0) - 1.0).abs() < 0.02, "gain 1 at the cutoff: {}", magnitude(&lp, w0));
        let (one, two) = (magnitude(&lp, 4.0 * w0), magnitude(&lp, 8.0 * w0));
        assert!((one / two - 4.0).abs() < 0.6, "12 dB per octave: {}", one / two);
        let mut hp = Biquad::default();
        hp.design(500.0, rate, true);
        assert!((magnitude(&hp, std::f32::consts::PI * 0.999) - 1.0).abs() < 0.02, "unity at Nyquist");
        assert!((magnitude(&hp, w0) - 1.0).abs() < 0.02, "gain 1 at the cutoff: {}", magnitude(&hp, w0));
        assert!(magnitude(&hp, w0 * 0.25) < 0.1);
    }

    #[test]
    fn the_track_update_shapes_gain_filters_pan_and_send() {
        // `_cell_sfx_base` numbers: min 4, max 12, atmospheric 20000 / 10000 / 2500 at 0 / 5 / 10.
        let def = SoundDef {
            name: "t".into(),
            samples: vec![1],
            gain: 1.5,
            looped: false,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 1,
            is_3d: true,
            min_distance: 4.0,
            max_distance: 12.0,
            polyphony: 0,
            start_delay: 0.0,
            curve: vec![],
            silent: false,
            groups: vec![],
        };
        let acoustics = Acoustics {
            atmospheric: vec![(0.0, 20_000.0), (5.0, 10_000.0), (10.0, 2_500.0)],
            wetlevel_curve: vec![(0.0, 0.0), (10.0, 0.1)],
            ..Default::default()
        };
        let near = voice_state(&def, &acoustics, Vec2::new(0.5, 0.0), 1.5, false, None);
        let mid = voice_state(&def, &acoustics, Vec2::new(8.0, 0.0), 1.5, false, None);
        let far = voice_state(&def, &acoustics, Vec2::new(14.0, 0.0), 1.5, false, None);
        assert_eq!(near.lowpass, 0.0, "open near the listener");
        assert!((mid.lowpass - 5_500.0).abs() < 1.0 && far.lowpass == 2_500.0);
        assert!((near.gain - 1.5).abs() < 1e-5 && mid.gain < near.gain && far.gain == 0.0);
        assert!(mid.pan[0][1] > mid.pan[0][0] && near.pan[0][1] - near.pan[0][0] < mid.pan[0][1] - mid.pan[0][0]);
        assert!(mid.wet > near.wet && (mid.wet - 0.08).abs() < 1e-5);
        // A voice with its own distances (a creature voice at a state whose curves give 2 .. 6):
        // the roll-off is evaluated against the moving distance with those, not the soundProp's.
        let own = Some((2.0, 6.0));
        let (inside, edge, beyond) = (
            voice_state(&def, &acoustics, Vec2::new(1.5, 0.0), 1.0, false, own),
            voice_state(&def, &acoustics, Vec2::new(4.0, 0.0), 1.0, false, own),
            voice_state(&def, &acoustics, Vec2::new(7.0, 0.0), 1.0, false, own),
        );
        assert!((inside.gain - 1.0).abs() < 1e-6 && (edge.gain - 0.5).abs() < 1e-6 && beyond.gain == 0.0);
        let own_static = voice_state(&def, &acoustics, Vec2::new(7.0, 0.0), 1.0, false, None);
        assert!(own_static.gain > 0.0, "the soundProp's own distances still reach 7 units");
    }

    fn cell_def(min: f32, max: f32) -> SoundDef {
        SoundDef {
            name: "t".into(),
            samples: vec![1],
            gain: 1.0,
            looped: false,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 1,
            is_3d: true,
            min_distance: min,
            max_distance: max,
            polyphony: 0,
            start_delay: 0.0,
            curve: vec![],
            silent: false,
            groups: vec![],
        }
    }

    #[test]
    fn a_voice_follows_a_moving_cell_and_a_moving_listener_every_frame() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<GameAudio>()
            .init_resource::<Time<Real>>()
            .init_resource::<CellReverb>()
            .insert_resource(Assets::<SpatialAsset>::default())
            .add_systems(Update, drive_spatial);
        let avatar = app.world_mut().spawn((Player, Transform::from_xyz(0.0, 0.0, 0.0))).id();
        // A cell 8 retail units to the right (world +x), 2 ahead.
        let cell = app.world_mut().spawn(Transform::from_xyz(8.0 * UNIT, 0.0, -2.0 * UNIT)).id();
        let loaded = Arc::new(Loaded {
            def: cell_def(4.0, 40.0),
            clips: vec![],
            raw: vec![tone_wav(300.0, 1.0, 1, 22_050)],
            acoustics: Acoustics {
                atmospheric: vec![(0.0, 20_000.0), (10.0, 4_000.0), (30.0, 500.0)],
                wetlevel_curve: vec![(0.0, 0.0), (30.0, 0.3)],
                ..Default::default()
            },
        });
        let hold = loaded.clone();
        let entity = app
            .world_mut()
            .run_system_once(move |mut commands: Commands, mut spatial: SpatialStart| {
                let spec = VoiceSpec { loaded: &hold, pick: 0, looped: true, speed: 1.0, anchor: Anchor::Cell(cell, Vec2::ZERO), base: 1.0, mixer: 1.0 };
                spatial.start(&mut commands, "test", spec)
            })
            .unwrap()
            .expect("a voice starts with a listener, a source and the audio assets");
        app.update();
        let ctl = app.world().get::<SpatialVoice>(entity).unwrap().ctl.clone();
        let (gain, left, right, lowpass, wet) = (load(&ctl.gain), load(&ctl.pan[0]), load(&ctl.pan[1]), load(&ctl.lowpass), load(&ctl.wet));
        assert!(left < 0.1 && right > 1.3, "on the right: {left} / {right}");
        assert!(gain > 0.8 && lowpass > 6_000.0 && lowpass < 7_500.0 && wet > 0.0, "gain {gain} lowpass {lowpass} wet {wet}");
        // The cell swims to the far left: the image follows, the sound gets duller and wetter.
        app.world_mut().get_mut::<Transform>(cell).unwrap().translation = Vec3::new(-30.0 * UNIT, 0.0, 0.0);
        app.update();
        let (left2, right2, lowpass2, wet2) = (load(&ctl.pan[0]), load(&ctl.pan[1]), load(&ctl.lowpass), load(&ctl.wet));
        assert!(right2 < 0.1 && left2 > 1.3, "on the left now: {left2} / {right2}");
        assert!(lowpass2 < lowpass * 0.5 && wet2 > wet, "lowpass {lowpass2} wet {wet2}");
        // The listener moves next to the cell: it is centred, close and bright again.
        app.world_mut().get_mut::<Transform>(avatar).unwrap().translation = Vec3::new(-30.0 * UNIT, 0.0, 0.5 * UNIT);
        app.update();
        let (left3, right3, lowpass3) = (load(&ctl.pan[0]), load(&ctl.pan[1]), load(&ctl.lowpass));
        assert!((left3 - right3).abs() < 0.2 * (left3 + right3), "centred: {left3} / {right3}");
        assert!(lowpass3 == 0.0 || lowpass3 > 15_000.0, "open: {lowpass3}");
        // The cell is eaten: the voice keeps its last position.
        app.world_mut().entity_mut(cell).despawn();
        app.world_mut().get_mut::<Transform>(avatar).unwrap().translation = Vec3::new(-30.0 * UNIT, 0.0, 25.0 * UNIT);
        app.update();
        assert!(load(&ctl.lowpass) > 0.0 && load(&ctl.lowpass) < 2_000.0, "still follows the listener: {}", load(&ctl.lowpass));
        // Muted: silent.
        app.world_mut().resource_mut::<GameAudio>().muted = true;
        app.update();
        assert_eq!(load(&ctl.gain), 0.0);
    }

    /// Offline render of a moving source through the real chain (the production state function and
    /// source, the controls rewritten every 10 ms as `drive_spatial` does every frame) for installed
    /// Cell sounds: a looped electric sound and a creature voice swim past the listener from 14
    /// units on the left to 14 on the right; written to `qc/acoustics/moving_<sound>.wav` (stereo) of
    /// this checkout. Checks: the image travels from the left speaker to the right one, the sound
    /// is loudest and brightest in the middle, the far ends are duller and quieter.
    ///
    /// The game picks one of a sound's authored samples at random per play, so the test renders
    /// every sample of each sound (lowest id first; the shuffle in `load_cue` is seeded and its
    /// order does not matter) instead of whichever one a random pick lands on. Only the first
    /// sample's pass is written as `moving_<sound>.wav`, so the artifact is stable too.
    #[test]
    #[ignore = "needs the installed Spore audio packages; writes qc/acoustics/*.wav"]
    fn render_a_moving_source_through_the_voice() {
        use crate::game_audio::{Bus, CueSpec};
        use rand::SeedableRng;
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qc").join("acoustics");
        std::fs::create_dir_all(&out).unwrap();
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(155);
        for name in ["sfx_cell_electricity_lp", "vcl_idle_pk", "sfx_cell_eat", "vcl_idle_ch"] {
            let spec = CueSpec {
                key: name,
                names: Box::leak(vec![name].into_boxed_slice()),
                bus: Bus::Sfx,
                trim: 1.0,
                min_interval: 0.0,
                window: None,
                variants: usize::MAX,
                pool: false,
            };
            let (def, mut clips) = crate::game_audio::load_cue(&library, &spec, &mut rng).unwrap();
            clips.sort_by_key(|c| c.sample);
            let acoustics = Acoustics::read(&library, name);
            for (variant, clip) in clips.iter().enumerate() {
                let label = format!("{name} sample {:08x}", clip.sample);
                let wav: Arc<[u8]> = clip.wav.clone().into();
                let stereo = wav.len() >= 24 && u16::from_le_bytes([wav[22], wav[23]]) >= 2;
                let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
                // A creature voice plays with the values its state curves give at the event's state (the
                // chomper's `vcl_idle_ch` at state 22 = mad: gain, pitch and the roll-off distances
                // 15 .. 25 instead of the soundProp's 25 .. 30) and travels past its max distance.
                let voice = (name == "vcl_idle_ch").then(|| crate::cell_voices::VoiceCurves::read(&index, name).at(22, &def));
                let range = voice.map(|p| (p.min_distance, p.max_distance));
                let base = voice.map_or(1.0, |p| p.gain);
                let pitch = voice.map_or(1.0, |p| p.pitch);
                let (secs, travel) = (4.0f32, if name == "vcl_idle_ch" { 60.0f32 } else { 28.0f32 });
                let looped = def.looped || clip.secs < secs;
                let at = |t: f32| Vec2::new(-travel / 2.0 + travel * t / secs, -2.0);
                let ctl = Arc::new(SpatialCtl::new(&voice_state(&def, &acoustics, at(0.0), base, stereo, range)));
                let bus = Arc::new(ReverbBus::default());
                let mut src = SpatialSource::new(SpatialAsset { wav, ctl: ctl.clone(), looped, speed: pitch, reverb: Some(bus.clone()) });
                let mut reverb = ReverbAsset { bus }.decoder();
                let chunk = (rate / 100) as usize;
                let mut mix: Vec<f32> = vec![];
                let mut phase = 0.0f64;
                let mut rows: Vec<(f32, f32, f32)> = vec![];
                for k in 0..(secs * 100.0) as usize {
                    let t = k as f32 / 100.0;
                    ctl.set(&voice_state(&def, &acoustics, at(t), base, stereo, range));
                    // The output mixer pulls the voice and the reverb in step (the send is written a
                    // frame ahead of the reverb's read head).
                    // The mix is written at the reverb's rate (the voice is held between its frames).
                    let mut dry: Vec<f32> = vec![];
                    for _ in 0..chunk {
                        let (Some(l), Some(r)) = (src.next(), src.next()) else { break };
                        dry.extend([l, r]);
                        phase += REVERB_RATE as f64 / rate as f64;
                        let frames = phase.floor() as usize;
                        phase -= frames as f64;
                        let wet: Vec<f32> = (&mut reverb).take(2 * frames).collect();
                        for f in wet.chunks(2) {
                            mix.extend([l + f[0], r + f[1]]);
                        }
                    }
                    if dry.is_empty() {
                        break;
                    }
                    let (l, r) = (rms(&channel(&dry, 0)), rms(&channel(&dry, 1)));
                    let bright = {
                        let x = channel(&dry, 0).iter().zip(channel(&dry, 1)).map(|(a, b)| a + b).collect::<Vec<f32>>();
                        let e: f32 = x.iter().map(|v| v * v).sum::<f32>().max(1e-12);
                        x.windows(2).map(|p| (p[1] - p[0]).powi(2)).sum::<f32>() / e
                    };
                    rows.push((l, r, bright));
                }
                let pcm: Vec<i16> = mix.iter().map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16).collect();
                if variant == 0 {
                    std::fs::write(out.join(format!("moving_{name}.wav")), crate::audio::write_wav(&pcm, 2, REVERB_RATE)).unwrap();
                }
                let third = rows.len() / 3;
                let avg = |rows: &[(f32, f32, f32)], f: &dyn Fn(&(f32, f32, f32)) -> f32| rows.iter().map(f).sum::<f32>() / rows.len().max(1) as f32;
                let (head, mid, tail) = (&rows[..third], &rows[third..2 * third], &rows[2 * third..]);
                println!(
                    "{label}: channels {} rate {rate} looped {looped} rows {} | L/R head {:.3}/{:.3} mid {:.3}/{:.3} tail {:.3}/{:.3} | brightness {:.4} {:.4} {:.4}",
                    if stereo { 2 } else { 1 },
                    rows.len(),
                    avg(head, &|r| r.0),
                    avg(head, &|r| r.1),
                    avg(mid, &|r| r.0),
                    avg(mid, &|r| r.1),
                    avg(tail, &|r| r.0),
                    avg(tail, &|r| r.1),
                    avg(head, &|r| r.2),
                    avg(mid, &|r| r.2),
                    avg(tail, &|r| r.2)
                );
                if !stereo {
                    assert!(avg(head, &|r| r.0) > avg(head, &|r| r.1), "{label}: starts on the left");
                    assert!(avg(tail, &|r| r.1) > avg(tail, &|r| r.0), "{label}: ends on the right");
                }
                assert!(avg(mid, &|r| r.2) >= avg(head, &|r| r.2) * 0.9, "{label}: brighter near the listener than far on the left");
                assert!(avg(mid, &|r| r.2) >= avg(tail, &|r| r.2) * 0.9, "{label}: brighter near the listener than far on the right");
                if let Some((min, max)) = range {
                    // The voice's own range (15 .. 25 at this state): silent beyond `max` from the
                    // first frames (x = -30), audible at the listener, the roll-off following the distance.
                    // "Audible" is the mean level over the blocks inside `min` (full gain there), not
                    // one 10 ms block: a sample has quiet moments of its own (a pause between two
                    // growls), and the block at the exact centre of the pass lands in one for some of
                    // the samples.
                    let first = rows[0];
                    let near: Vec<f32> = rows
                        .iter()
                        .enumerate()
                        .filter(|(k, _)| at(*k as f32 / 100.0).length() <= min)
                        .map(|(_, r)| r.0 + r.1)
                        .collect();
                    assert!(near.len() > 100, "{label}: the pass spends time inside the voice's min distance");
                    let near_mean = near.iter().sum::<f32>() / near.len() as f32;
                    println!("{label}: range {min} .. {max}, first block L/R {:.4}/{:.4}, mean L+R inside {min}: {near_mean:.4}", first.0, first.1);
                    assert!(first.0 + first.1 < 1e-3, "{label}: beyond the voice's max distance it is silent");
                    assert!(near_mean > 0.01, "{label}: audible at the listener");
                }
            }
        }
    }
}
