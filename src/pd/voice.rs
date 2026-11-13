//! The host side voice: what the retail audio engine does per voice for the
//! parameters a patch sends (`play_messages.txt`): gain, pitch, low/high-pass
//! cutoffs, pan, fades, seek, ring modulation and instrument notes.
//!
//! A voice is a rodio source fed from decoded PCM (registered with Bevy as a
//! custom audio source). Its controls are atomics the driver updates every
//! frame; the source smooths gain and pitch per sample, recomputes its
//! filters when a cutoff changes and runs the envelope (fade in / out, or an
//! instrument zone's attack-hold-decay-sustain-release).
//!
//! ADAPTED (host semantics not traced, `notes\pd-music-re-notes.md` 5):
//! `pan` is a -1..1 balance, `panangle` degrees mapped through sin(), the
//! filters are 12 dB/oct Butterworth biquads, `wetlevel`/`wetlevel2` (reverb
//! sends), `pansize`, `priority`, `is3d`/`x y z`, `send <submix>` and the
//! mixer delay lines have no effect; instrument envelopes use the zone
//! columns literally (seconds, sustain level 0..1).

use bevy::asset::Asset;
use bevy::audio::Decodable;
use bevy::reflect::TypePath;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Decoded interleaved PCM.
#[derive(Debug, Default)]
pub struct Pcm {
    pub samples: Vec<i16>,
    pub channels: usize,
    pub rate: u32,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
    pub fn secs(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
}

fn f32_atomic(v: f32) -> AtomicU32 {
    AtomicU32::new(v.to_bits())
}

/// Controls shared between the driver (main thread) and the source (audio
/// thread).
#[derive(Debug)]
pub struct VoiceCtl {
    gain: AtomicU32,
    pitch: AtomicU32,
    lowpass: AtomicU32,
    hipass: AtomicU32,
    pan: AtomicU32,
    ring_freq: AtomicU32,
    ring_wet: AtomicU32,
    release: AtomicU32,
    stop: AtomicBool,
    paused: AtomicBool,
    finished: AtomicBool,
    /// Seek request in frames + 1 (0 = none).
    seek: AtomicU64,
}

impl Default for VoiceCtl {
    fn default() -> Self {
        VoiceCtl {
            gain: f32_atomic(0.0),
            pitch: f32_atomic(1.0),
            lowpass: f32_atomic(0.0),
            hipass: f32_atomic(0.0),
            pan: f32_atomic(0.0),
            ring_freq: f32_atomic(0.0),
            ring_wet: f32_atomic(0.0),
            release: f32_atomic(0.0),
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            seek: AtomicU64::new(0),
        }
    }
}

macro_rules! fset {
    ($name:ident, $field:ident) => {
        pub fn $name(&self, v: f32) {
            self.$field.store(v.to_bits(), Ordering::Relaxed);
        }
    };
}

impl VoiceCtl {
    fset!(set_gain, gain);
    fset!(set_pitch, pitch);
    fset!(set_lowpass, lowpass);
    fset!(set_hipass, hipass);
    fset!(set_pan, pan);
    fset!(set_ring_freq, ring_freq);
    fset!(set_ring_wet, ring_wet);
    // Seconds of the release when the voice stops.
    fset!(set_release, release);
    fn get(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Ordering::Relaxed))
    }
    pub fn gain(&self) -> f32 {
        Self::get(&self.gain)
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::Relaxed);
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
    pub fn mark_finished(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }
    pub fn seek_frames(&self, frames: u64) {
        self.seek.store(frames + 1, Ordering::Relaxed);
    }
}

/// Envelope of a voice: a plain sound fades in over `attack` and holds 1;
/// an instrument note runs its zone's envelope and releases after the note
/// duration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    /// Instrument release (the stop release comes from the controls).
    pub release: f32,
    /// Seconds until the note releases by itself.
    pub note: Option<f32>,
}

impl Envelope {
    pub fn fade_in(secs: f32) -> Self {
        Envelope { attack: secs.max(0.0), hold: 0.0, decay: 0.0, sustain: 1.0, release: 0.0, note: None }
    }
}

/// One voice as an asset (cheap: shared PCM and controls).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct PdVoiceAsset {
    pub pcm: Arc<Pcm>,
    pub ctl: Arc<VoiceCtl>,
    pub looped: bool,
    pub start_frame: usize,
    pub env: Envelope,
}

impl Decodable for PdVoiceAsset {
    type Decoder = PdVoiceSource;
    fn decoder(&self) -> Self::Decoder {
        PdVoiceSource::new(self.clone())
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
    on: bool,
}

impl Biquad {
    fn design(&mut self, cutoff: f32, rate: f32, high: bool) {
        if cutoff <= 0.0 || (!high && cutoff >= rate * 0.45) || (high && cutoff < 5.0) {
            self.on = false;
            return;
        }
        let w = std::f32::consts::TAU * cutoff.clamp(5.0, rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high { ((1.0 + c) / 2.0, -(1.0 + c)) } else { ((1.0 - c) / 2.0, 1.0 - c) };
        self.b = [b0 / a0, b1 / a0, b0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - alpha) / a0];
        if !self.on {
            self.z = [[0.0; 2]; 2];
        }
        self.on = true;
    }
    fn run(&mut self, ch: usize, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        let z = &mut self.z[ch];
        let y = self.b[0] * x + z[0];
        z[0] = self.b[1] * x - self.a[0] * y + z[1];
        z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The rodio source of a voice (always stereo, at the PCM's rate).
pub struct PdVoiceSource {
    v: PdVoiceAsset,
    pos: f64,
    t: f64,
    gain: f32,
    pitch: f32,
    level: f32,
    releasing: Option<(f32, f32)>,
    lp: Biquad,
    hp: Biquad,
    lp_hz: f32,
    hp_hz: f32,
    ring_phase: f32,
    counter: u32,
    frame: [f32; 2],
    idx: usize,
    done: bool,
}

impl PdVoiceSource {
    pub fn new(v: PdVoiceAsset) -> Self {
        let pos = v.start_frame.min(v.pcm.frames().saturating_sub(1)) as f64;
        let gain = v.ctl.gain();
        let pitch = VoiceCtl::get(&v.ctl.pitch);
        PdVoiceSource {
            v,
            pos,
            t: 0.0,
            gain,
            pitch,
            level: 0.0,
            releasing: None,
            lp: Biquad::default(),
            hp: Biquad::default(),
            lp_hz: -1.0,
            hp_hz: -1.0,
            ring_phase: 0.0,
            counter: 0,
            frame: [0.0; 2],
            idx: 2,
            done: false,
        }
    }

    fn finish(&mut self) {
        self.done = true;
        self.v.ctl.mark_finished();
    }

    /// The envelope level before release.
    fn env_level(&self) -> f32 {
        let e = &self.v.env;
        let t = self.t as f32;
        if t < e.attack {
            return t / e.attack.max(1e-6);
        }
        let t = t - e.attack;
        if t < e.hold {
            return 1.0;
        }
        let t = t - e.hold;
        if e.decay > 0.0 && t < e.decay {
            return 1.0 + (e.sustain - 1.0) * (t / e.decay);
        }
        e.sustain
    }

    fn render(&mut self) -> bool {
        let pcm = self.v.pcm.clone();
        let frames = pcm.frames();
        let rate = pcm.rate.max(1) as f32;
        if self.done || frames == 0 {
            if !self.done {
                self.finish();
            }
            return false;
        }
        let ctl = self.v.ctl.clone();
        if self.counter % 64 == 0 {
            let seek = ctl.seek.swap(0, Ordering::Relaxed);
            if seek > 0 {
                self.pos = ((seek - 1) as f64).min(frames.saturating_sub(1) as f64);
            }
            let lp = VoiceCtl::get(&ctl.lowpass);
            if lp != self.lp_hz {
                self.lp_hz = lp;
                self.lp.design(lp, rate, false);
            }
            let hp = VoiceCtl::get(&ctl.hipass);
            if hp != self.hp_hz {
                self.hp_hz = hp;
                self.hp.design(hp, rate, true);
            }
        }
        self.counter = self.counter.wrapping_add(1);
        if ctl.paused.load(Ordering::Relaxed) {
            self.frame = [0.0; 2];
            return true;
        }
        // Envelope and release.
        let note_off = self.v.env.note.is_some_and(|n| self.t as f32 >= n);
        if self.releasing.is_none() && (ctl.stopping() || note_off) {
            let secs = if ctl.stopping() { VoiceCtl::get(&ctl.release) } else { self.v.env.release };
            if secs <= 0.0 {
                self.finish();
                return false;
            }
            self.releasing = Some((self.level, secs));
        }
        match self.releasing {
            Some((from, secs)) => {
                self.level -= from.max(1e-4) / (secs * rate);
                if self.level <= 0.0 {
                    self.finish();
                    return false;
                }
            }
            None => {
                self.level = self.env_level();
                let e = &self.v.env;
                if e.sustain <= 0.0 && self.t as f32 > e.attack + e.hold + e.decay {
                    self.finish();
                    return false;
                }
            }
        }
        self.t += 1.0 / rate as f64;
        // Smoothed gain / pitch (about 10 ms).
        self.gain += (ctl.gain() - self.gain) * 0.0025;
        self.pitch += (VoiceCtl::get(&ctl.pitch).clamp(0.01, 8.0) - self.pitch) * 0.0025;
        // Read with linear interpolation.
        let ch = pcm.channels.max(1);
        let i0 = self.pos as usize;
        let frac = (self.pos - i0 as f64) as f32;
        let i1 = if i0 + 1 < frames { i0 + 1 } else if self.v.looped { 0 } else { i0 };
        let sample = |i: usize, c: usize| pcm.samples[i * ch + c.min(ch - 1)] as f32 / 32768.0;
        let mut out = [0.0f32; 2];
        for (c, o) in out.iter_mut().enumerate() {
            let s = sample(i0, c) + (sample(i1, c) - sample(i0, c)) * frac;
            let s = self.hp.run(c, self.lp.run(c, s));
            *o = s;
        }
        let ring_wet = VoiceCtl::get(&ctl.ring_wet).clamp(0.0, 1.0);
        if ring_wet > 0.0 {
            let freq = VoiceCtl::get(&ctl.ring_freq);
            let m = (self.ring_phase).sin();
            self.ring_phase = (self.ring_phase + std::f32::consts::TAU * freq / rate) % std::f32::consts::TAU;
            for o in out.iter_mut() {
                *o = *o * (1.0 - ring_wet) + *o * m * ring_wet;
            }
        }
        let pan = VoiceCtl::get(&ctl.pan).clamp(-1.0, 1.0);
        let g = self.gain * self.level;
        self.frame = [out[0] * g * (1.0 - pan).min(1.0), out[1] * g * (1.0 + pan).min(1.0)];
        self.pos += self.pitch as f64;
        if self.pos >= frames as f64 {
            if self.v.looped {
                self.pos %= frames as f64;
            } else {
                self.finish();
                // The last frame still plays.
            }
        }
        true
    }
}

impl Iterator for PdVoiceSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.idx >= 2 {
            if self.done || !self.render() {
                return None;
            }
            self.idx = 0;
        }
        let s = self.frame[self.idx];
        self.idx += 1;
        Some(s)
    }
}

impl bevy::audio::Source for PdVoiceSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        NonZero::new(self.v.pcm.rate.max(8000)).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32) -> Arc<Pcm> {
        let rate = 22050;
        let n = (secs * rate as f32) as usize;
        Arc::new(Pcm {
            samples: (0..n).map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 16000.0) as i16).collect(),
            channels: 1,
            rate,
        })
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn voice_fades_in_and_releases_on_stop() {
        let ctl = Arc::new(VoiceCtl::default());
        ctl.set_gain(1.0);
        ctl.set_release(0.5);
        let asset = PdVoiceAsset { pcm: tone(2.0), ctl: ctl.clone(), looped: true, start_frame: 0, env: Envelope::fade_in(0.5) };
        let mut src = asset.decoder();
        let first: Vec<f32> = (&mut src).take(2 * 2205).collect(); // 0.1 s
        let later: Vec<f32> = (&mut src).take(2 * 22050).skip(2 * 15000).collect();
        assert!(rms(&first) < rms(&later) * 0.5, "fade in: {} vs {}", rms(&first), rms(&later));
        ctl.stop();
        let tail: Vec<f32> = (&mut src).take(2 * 22050).collect();
        assert!(tail.len() < 2 * 22050 && tail.len() > 2 * 10000, "release ~0.5 s: {}", tail.len());
        assert!(ctl.finished());
    }

    #[test]
    fn one_shot_ends_and_pitch_speeds_it_up() {
        let ctl = Arc::new(VoiceCtl::default());
        ctl.set_gain(1.0);
        ctl.set_pitch(2.0);
        let asset = PdVoiceAsset { pcm: tone(1.0), ctl: ctl.clone(), looped: false, start_frame: 0, env: Envelope::fade_in(0.0) };
        let n = asset.decoder().count() / 2;
        // Pitch smoothing starts at the initial pitch: about half the frames.
        assert!((10_000..12_500).contains(&n), "{n}");
        assert!(ctl.finished());
    }

    #[test]
    fn lowpass_removes_highs() {
        let mk = |lp: f32| {
            let ctl = Arc::new(VoiceCtl::default());
            ctl.set_gain(1.0);
            ctl.set_lowpass(lp);
            let asset = PdVoiceAsset { pcm: tone(1.0), ctl, looped: false, start_frame: 0, env: Envelope::fade_in(0.0) };
            let v: Vec<f32> = asset.decoder().skip(4000).take(8000).collect();
            rms(&v)
        };
        assert!(mk(100.0) < mk(0.0) * 0.2);
    }

    #[test]
    fn instrument_note_decays_and_releases() {
        let ctl = Arc::new(VoiceCtl::default());
        ctl.set_gain(1.0);
        let env = Envelope { attack: 0.0, hold: 0.0, decay: 5.0, sustain: 0.0, release: 0.25, note: Some(0.5) };
        let asset = PdVoiceAsset { pcm: tone(3.0), ctl: ctl.clone(), looped: false, start_frame: 0, env };
        let n = asset.decoder().count() / 2;
        // 0.5 s note + 0.25 s release.
        assert!((15_000..17_500).contains(&n), "{n}");
    }
}
