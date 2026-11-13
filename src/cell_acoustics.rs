//! Cell acoustics (helpers `cell-audio3`, `cell-audio4`): what the retail audio engine does to a
//! positional Cell sound beyond the roll-off gain, plus the listener the Cell game feeds it. The
//! per-frame voice that applies it is `cell_spatial`.
//!
//! RETAIL (code image march2017 addresses + installed data, `notes\cell-audio3-notes.md`,
//! `notes\cell-audio4-notes.md`):
//! * **Track update (0xA1C9B0..0xA1CCCB, the track class of vtable 0x1453C68), every update.** The
//!   track's distance (+0x48) is the Euclidean norm of `track position - listener position`
//!   (0xA1C0A0..0xA1C136; the listener comes from `AudioSystem::Get()` vtable +0x7C), in world
//!   units. While the track is positional (+0xCE4 bit 0x4000) it evaluates three curves at that
//!   distance with the curve evaluator 0xA1A090 and sends the results to the voice:
//!   `lowpass` (0x647A7836) = min(`atmospheric` curve, +0x78), `hipass` (0xA26A765F) = max(curve
//!   0x09D0B38E, +0x7C), `wetlevel` (0x49EB68DB) = max(`wetlevelcurve` 0x173C2710, +0x74). The curve
//!   slots are bound to their property ids at 0xA2035D..0xA20389; the three `+0x74/78/7C` values
//!   are the soundProp's own `wetlevel` / `lowpass` / `hipass` (pushed at 0xA201BD / 0xA2xxxx).
//! * **The voice (template `sound` -> resource 0x2EA8FB98, a plugin chain text):** `SnP1 Rch0 Rsp0
//!   Pau0 Rmd0 Chr0 Dly0 LI20 HI20 Pn21 Sen0[reverb] Sen0`: the sample player, resampler / pitch,
//!   pause, ring modulator, chorus, delay, **LowPassIir2** (`lowpass=LI20:0`, default 96000 Hz),
//!   **HighPassIir2** (`hipass=HI20:0`), the stereo panner **Pan2D1** (`pan`, `pandistance`,
//!   `pansize`, `pantwist`, `panlfe`), a send to the submix named `reverb` (`wetlevel=Sen0[reverb]`)
//!   and the normal send (`send` of the soundProp, `all_submix` by default). The root soundProp
//!   `sound` sets `pandistance 0` (a flat sound is centred) and `wetlevelcurve (0,0)-(1000,0)`.
//! * **Filters (VERIFIED, helper `cell-audio5`).** LowPassIir2 (process 0x1153570, coefficients
//!   0x11534D0) and HighPassIir2 (0x1153ED0 / 0x1153E30) are second order RBJ cookbook filters at
//!   **Q = 1** (`alpha = sin(w) / 2`, `w = 2 pi cutoff / rate`), direct form I (1e-18 added against
//!   denormals); the low-pass is bypassed from `w >= 3.138451` (99.9 % of Nyquist) and held at
//!   `w >= 0.003141593`, the high-pass is bypassed up to `w <= 0.003141593` and held at 3.138451
//!   (the embedded documentation's 99 % / 1 % of Nyquist are 99.9 % / 0.1 % in the code).
//! * **The pan (0xA12740..0xA12A47, the 3D update of the voice class).** The source offset from the
//!   listener is projected on the listener's axes (columns 0 = right, 1 = forward of the listener
//!   matrix); `pan` = atan2(right, forward) in degrees (0..360, clockwise from straight ahead),
//!   `pandistance` = distance / `mindistance` below `mindistance`, else 1, and for a multichannel
//!   sample `pansize` = 1 - clamp((distance - min) / (max - min), 0, 1). Cell: +X = screen right,
//!   +Y = screen up, the listener has no rotation (message 0x052D9BAB only carries a position), so
//!   `sin(pan)` is the offset's x over its length and the pan ignores the distance's height.
//! * **Pan2D1 on two speakers (0x1150BF0 positions, 0x11515E0 gains).** A source at angle `a`
//!   (clockwise from ahead) and radius `r` = `pandistance` sits at (x', y') = (r cos a, -r sin a);
//!   the gains of an input channel are `t = (y' + 1) / 2`, left = `t / n`, right = `(1 - t) / n`,
//!   `n = sqrt(t^2 + (1 - t)^2)`, times the normalisation of the input channels (mono 1, a stereo
//!   sample 1/sqrt 2, the constructor default SQRT_NUM_INPUT_CHANNELS). The two channels of a stereo
//!   sample are put at the position +- `pansize` * (sin a, cos a) (tangential, rotated by
//!   `pantwist`, 0 here) and a position outside the unit circle is scaled onto it. This is a
//!   constant-power law with linear amplitude interpolation, not sine / cosine.
//! * **Reverb (`wetlevel`).** The `reverb` submix (`reverbsubmix` = `Sub0 RM10 Sen0`, `reverbtime`
//!   2.5 s, `reverbsize` 30 m, `reverbbrightness` default) is a ReverbModel1 (six parallel comb
//!   filters followed by an allpass per output channel). VERIFIED (helper `cell-audio5`, constructor
//!   0x1139E60, parameter update 0x113A030, delay lengths 0x1139970; `cell-final4`: the rest, see
//!   [`crate::cell_spatial::Reverb`]): the six comb lengths run
//!   from 0.8 x to 1.2 x the size (24 .. 36 m) in five steps, 344.8 m/s, rounded up to the next
//!   prime of a table (primes below 14000); two output channels get one allpass each, (feedback 0.63,
//!   6.667 ms) and (0.7778, 5.4 ms). A voice sends `wetlevel * gain` of its signal into it after the
//!   filters / panner. In the Cell the voices (`vo_base`: `wetlevelcurve` 0 at 0 units rising to 0.06
//!   at 8.6, 0.11 at 36 and 1.0 at 100), the three `sfx_cell_mate_*` calls, `sfx_cell_explosion`
//!   (0.2) and the chemical drops (0.1 .. 1.0) have a send; every other Cell sound sends nothing.
//! * **Roll-off (0xA41C10, track vtable +0xB0).** `d / scale`; inside `mindistance` 1, beyond
//!   `maxdistance` 0, else the `attenuation` curve at `(d - min) / (max - min)` (no roll-off when
//!   the rolloff argument is 0).
//! * **Curve evaluator 0xA1A090.** Segment table of 5 floats per point `[x, y, x_prev, y_prev,
//!   slope]` (built at 0xA1A572..0xA1A93F; slope 0 for a zero-width segment): binary search for the
//!   first point with `x_i > x`; below the first point the first y, from the last point on the
//!   last y, else `(x - x_prev) * slope + y_prev`: piecewise linear with held ends.
//! * **Submix send (nothing to apply).** `_cell_sfx_base` (every `sfx_cell_*`, the collisions, the
//!   jets) and `_cell_vox_base` (every `vcl_*` voice) carry `send cell_vo_submix` (404 installed
//!   sounds; none sends to `cell_vo_small_submix`), the submix record `cell_vo` -> `all_submix` ->
//!   master. `mixmode_cellgame` starts the soundProp `mix_event_cell_vo`, whose `patch` key is the
//!   bundle `mix_event_cell_vo_small` (hash 0xCE9A748F): it only configures `cell_vo_small_submix`.
//!   The bundle `mix_event_cell_vo` (ring 400 Hz and a 100 ms echo on `cell_vo_submix`) is not
//!   started by any soundProp, mode or code, so **no ring modulation or echo on Cell sounds**.
//! * **Listener (0xE73F60 -> message 0x052D9BAB, every frame).** While the cinematic manager is
//!   starting or running a cinematic (`cCinematicManager` +0x2C is 1 or 2) the position is the
//!   camera look point (`sCellUI + 0x48`, [`crate::cell_engine::CellGame::camera_look`]), otherwise
//!   the avatar's position.
//!
//! The ReverbModel1 internals are VERIFIED too (helper `cell-final4`; [`crate::cell_spatial::Reverb`]:
//! the feedback tables by rate and length 0x1139A30, the loop gains 0x1139DB0, the comb kernel
//! 0x114DAE0, the all-pass kernel 0x114DD90, the block process 0x114D450).
//!
//! VERIFIED (helper `cell-final5`): the reverb send. The chain of a Cell voice is `... Pn21 Sen0[reverb] Sen0`, so
//! the send takes the panner's two output channels; the `reverb` submix record carries `numchannels` 1
//! (0xED4C265F; the master has 6) and ReverbModel1 takes a single (mono) source, so the send plugin (descriptor
//! 'Sen0' 0x15BBCD0, process 0x114CBD0) folds two channels into one with the platform's mix matrix
//! (`0x115D4E0` / the ramped `0x115D570`, table 0x14DD49A indexed destination channels + 6 x source
//! channels, entries 0x14DD4F0): both sources into destination 0 at coefficient 1.0, i.e. **L + R of the
//! panner** at the send gain (`gain * wetlevel`). The plugin's own process (0x114D450) then reads channel 0 of
//! that one-channel bus. [`crate::cell_spatial`] sends `(l + r) / sqrt 2` of its sqrt-2-scaled gains.
//!
//! ADAPTED: the gains are scaled by sqrt 2 so that a centred mono sound keeps unity per channel like every
//! other OpenSpore voice (retail's panner gives 1/sqrt 2 per channel at the centre for every voice of the
//! mix); a stereo source is rotated with the Pan2D1 tangential rule but without `pantwist`.

use std::{collections::HashSet, f32::consts::SQRT_2};

use bevy::prelude::Vec2;

use crate::{
    dbpf::ResourceKey,
    game_audio::{AudioLibrary, SoundDef, TYPE_SOUND_PROP},
    prop::{Property, PropertyList},
    world_objects::name_id,
};

/// Property ids of the soundProp (FNV-1 of the names, `reg_property.txt`).
const P_PARENT: u32 = 0x5F63_17D5;
/// `atmospheric`: (distance, low-pass cutoff in Hz) points.
const P_ATMOSPHERIC: u32 = 0x0F70_317C;
/// Unnamed curve bound next to `atmospheric` (0xA20354): (distance, high-pass cutoff in Hz).
const P_DISTANCE_HIPASS: u32 = 0x09D0_B38E;
/// `send`: the submix the sound plays through (UTF-16 text).
const P_SEND: u32 = 0x2FE0_9C83;
/// `wetlevelcurve`: (distance, reverb send) points.
const P_WETLEVEL_CURVE: u32 = 0x173C_2710;
/// `wetlevel`: the static reverb send (the lower bound of the track's `wetlevel`).
const P_WETLEVEL: u32 = 0x49EB_68DB;

/// The submix every Cell sound is sent to.
pub const CELL_VO_SUBMIX: &str = "cell_vo_submix";

/// What the retail track reads from the soundProp chain for the per-update filters, the reverb send
/// and the submix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acoustics {
    /// `atmospheric`: (distance, low-pass Hz).
    pub atmospheric: Vec<(f32, f32)>,
    /// (distance, high-pass Hz).
    pub distance_hipass: Vec<(f32, f32)>,
    /// `wetlevelcurve`: (distance, reverb send).
    pub wetlevel_curve: Vec<(f32, f32)>,
    /// `wetlevel`: static reverb send.
    pub wetlevel: f32,
    /// `send` of the nearest definer.
    pub send: Option<String>,
}

fn floats(property: &Property) -> Vec<(f32, f32)> {
    let v: Vec<f32> = property
        .values
        .iter()
        .filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?)))
        .collect();
    v.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

impl Acoustics {
    /// From the property lists of a soundProp and its ancestors, child first (the nearest
    /// definer of a property wins).
    pub fn from_chain(chain: &[PropertyList]) -> Self {
        let find = |id: u32| chain.iter().find_map(|l| l.0.get(&id));
        Self {
            atmospheric: find(P_ATMOSPHERIC).map(floats).unwrap_or_default(),
            distance_hipass: find(P_DISTANCE_HIPASS).map(floats).unwrap_or_default(),
            wetlevel_curve: find(P_WETLEVEL_CURVE).map(floats).unwrap_or_default(),
            wetlevel: find(P_WETLEVEL).and_then(Property::number).unwrap_or(0.0),
            send: find(P_SEND).and_then(|p| p.text()).map(|t| t.2).filter(|s| !s.is_empty()),
        }
    }

    /// Read the chain of a sound (`#XXXXXXXX` names it by id).
    pub fn read(library: &AudioLibrary, name: &str) -> Self {
        let mut current = match name.strip_prefix('#') {
            Some(hex) => u32::from_str_radix(hex, 16).unwrap_or(0),
            None => name_id(name),
        };
        let mut chain: Vec<PropertyList> = vec![];
        let mut seen = HashSet::new();
        while seen.insert(current) && chain.len() < 16 {
            let Some(list) = library.read(TYPE_SOUND_PROP, current).ok().and_then(|b| crate::prop::parse(&b).ok()) else {
                break;
            };
            let parent = list
                .0
                .get(&P_PARENT)
                .and_then(Property::key)
                .map(|k: ResourceKey| k.instance_id)
                .filter(|p| *p != 0);
            chain.push(list);
            match parent {
                Some(p) => current = p,
                None => break,
            }
        }
        Self::from_chain(&chain)
    }

    /// Whether the sound plays through the Cell's submix.
    pub fn sent_to_cell_vo(&self) -> bool {
        self.send.as_deref() == Some(CELL_VO_SUBMIX)
    }

    /// The low-pass cutoff the track sends at `distance` (`None`: the filter is open).
    pub fn lowpass_at(&self, distance: f32) -> Option<f32> {
        if self.atmospheric.is_empty() {
            return None;
        }
        Some(curve(&self.atmospheric, distance)).filter(|hz| *hz < OPEN_LOWPASS)
    }

    /// The high-pass cutoff the track sends at `distance` (`None`: the filter is off).
    pub fn hipass_at(&self, distance: f32) -> Option<f32> {
        if self.distance_hipass.is_empty() {
            return None;
        }
        Some(curve(&self.distance_hipass, distance)).filter(|hz| *hz > OFF_HIPASS)
    }

    /// The reverb send the track sets at `distance`: max(`wetlevelcurve`, `wetlevel`), never below 0
    /// (the voices' curve starts at -0.007).
    pub fn wet_at(&self, distance: f32) -> f32 {
        let curved = if self.wetlevel_curve.is_empty() { 0.0 } else { curve(&self.wetlevel_curve, distance) };
        curved.max(self.wetlevel).max(0.0)
    }
}

/// A low-pass above this is not audible on the 22 / 44 kHz samples (left out).
const OPEN_LOWPASS: f32 = 18_000.0;
/// A high-pass below this is not audible (left out).
const OFF_HIPASS: f32 = 20.0;

/// The curve evaluator 0xA1A090 on (x, y) points sorted by x: piecewise linear, the first and
/// last y held outside the points. A zero-width segment is a step (the later point wins).
pub fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(&(x0, y0)) = points.first() else { return 0.0 };
    if x < x0 {
        return y0;
    }
    // The first point with x_i > x (the binary search of the retail routine).
    let upper = points.partition_point(|p| p.0 <= x);
    if upper == 0 {
        return y0;
    }
    if upper >= points.len() {
        return points[points.len() - 1].1;
    }
    let (xa, ya) = points[upper - 1];
    let (xb, yb) = points[upper];
    if xb > xa {
        ya + (yb - ya) * (x - xa) / (xb - xa)
    } else {
        ya
    }
}

/// `pandistance` of the voice's 3D update (0xA12997..0xA129D3): `distance / mindistance` inside
/// `mindistance`, else 1.
pub fn pan_distance(distance: f32, min_distance: f32) -> f32 {
    if min_distance > distance {
        distance / min_distance
    } else {
        1.0
    }
}

/// `pansize` of the voice's 3D update (0xA129D8..0xA12A33), set for multichannel samples only: 1
/// at `mindistance`, 0 at `maxdistance`.
pub fn pan_size(distance: f32, min_distance: f32, max_distance: f32) -> f32 {
    if max_distance == min_distance {
        return 1.0;
    }
    let clamped = distance.clamp(min_distance.min(max_distance), min_distance.max(max_distance));
    1.0 - (clamped - min_distance) / (max_distance - min_distance)
}

/// One input channel's two output gains of Pan2D1 on two speakers (0x11515E0) for a source at
/// (x', y') in the panner's unit circle (x' ahead, y' to the listener's left), times
/// `normalisation`. The position is scaled onto the circle first (0x1150B90).
fn channel_gains(x: f32, y: f32, normalisation: f32) -> [f32; 2] {
    let r2 = x * x + y * y;
    let y = if r2 > 1.0 { y / r2.sqrt() } else { y };
    let t = (y + 1.0) * 0.5;
    let n = normalisation / (t * t + (1.0 - t) * (1.0 - t)).sqrt();
    [t * n, (1.0 - t) * n]
}

/// The gains `[input channel][left, right]` of a voice whose source is at `offset` (retail units,
/// source minus listener, screen right = +x, screen up = -y) from the listener. `min_distance` /
/// `max_distance` are the soundProp's; `stereo` is a two channel sample (a mono sample uses row 0).
/// RETAIL Pan2D1 (see the module header) times sqrt 2 (ADAPTED, so a centred mono sound is unity).
pub fn pan_gains(offset: Vec2, min_distance: f32, max_distance: f32, stereo: bool) -> [[f32; 2]; 2] {
    let d = offset.length();
    // sin / cos of the pan angle (clockwise from ahead = screen up): atan2(right, forward).
    let (sin_a, cos_a) = if d > 1e-4 { (offset.x / d, -offset.y / d) } else { (0.0, 1.0) };
    let radius = pan_distance(d, min_distance);
    let (px, py) = (radius * cos_a, -radius * sin_a);
    if !stereo {
        let g = channel_gains(px, py, 1.0);
        return [[g[0] * SQRT_2, g[1] * SQRT_2], [0.0; 2]];
    }
    let size = pan_size(d, min_distance, max_distance);
    // The pair sits at the position +- size * (cos phi, sin phi), phi = pi/2 - a (no twist).
    let (dx, dy) = (size * sin_a, size * cos_a);
    let a = channel_gains(px + dx, py + dy, SQRT_2.recip());
    let b = channel_gains(px - dx, py - dy, SQRT_2.recip());
    [[a[0] * SQRT_2, a[1] * SQRT_2], [b[0] * SQRT_2, b[1] * SQRT_2]]
}

/// The roll-off gain of a positional sound at `distance` (0xA41C10: `(d - min) / (max - min)` through
/// the soundProp's `attenuation` curve, linear without one, 1 for a flat sound or when `max <= min`)
/// with the distances the track really uses: `range` is the (min, max) the voice's state curves
/// give (`cell_voices`, the `primitives` bound to input 0x039FB000), `None` the soundProp's own.
/// The track evaluates it on every update against the current distance.
pub fn roll_off(def: &SoundDef, range: Option<(f32, f32)>, distance: f32) -> f32 {
    let Some((min, max)) = range else { return def.attenuation(distance) };
    if !def.is_3d || max <= min {
        return 1.0;
    }
    let x = ((distance - min) / (max - min)).clamp(0.0, 1.0);
    if def.curve.len() < 2 {
        return 1.0 - x;
    }
    crate::pd::library::Spatial { min: 0.0, max: 1.0, curve: def.curve.clone() }.gain(x)
}

/// The point the Cell game feeds the audio system as the listener (0xE73F60): the camera look
/// point while a cinematic is starting / running, else the avatar (`None` without one).
pub fn listener_plane(cinematic: bool, camera_look: Vec2, avatar: Option<Vec2>) -> Option<Vec2> {
    if cinematic {
        Some(camera_look)
    } else {
        avatar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_is_piecewise_linear_with_held_ends() {
        // The Cell sfx base: (0, 20000), (5, 10000), (10, 2500).
        let atmospheric = [(0.0, 20_000.0), (5.0, 10_000.0), (10.0, 2_500.0)];
        assert_eq!(curve(&atmospheric, -1.0), 20_000.0);
        assert_eq!(curve(&atmospheric, 0.0), 20_000.0);
        assert_eq!(curve(&atmospheric, 2.5), 15_000.0);
        assert_eq!(curve(&atmospheric, 5.0), 10_000.0);
        assert_eq!(curve(&atmospheric, 7.5), 6_250.0);
        assert_eq!(curve(&atmospheric, 10.0), 2_500.0);
        assert_eq!(curve(&atmospheric, 80.0), 2_500.0, "held beyond the last point");
        // A zero-width segment is a step (the later point wins from its x on).
        let step = [(0.0, 1.0), (0.0, 1.0), (1.0, 0.0), (1.0, 0.5), (2.0, 0.5)];
        assert_eq!(curve(&step, 0.0), 1.0);
        assert_eq!(curve(&step, 0.5), 0.5);
        assert_eq!(curve(&step, 1.0), 0.5);
        assert_eq!(curve(&[], 3.0), 0.0);
        assert_eq!(curve(&[(2.0, 7.0)], 9.0), 7.0);
    }

    #[test]
    fn acoustics_read_the_nearest_definer() {
        use crate::prop::Property;
        let floats = |v: &[f32]| Property { kind: 0x0D, array: true, values: v.iter().map(|x| x.to_be_bytes().to_vec()).collect() };
        let text = |s: &str| Property { kind: 0x13, array: false, values: vec![s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()] };
        let number = |v: f32| Property { kind: 0x0D, array: false, values: vec![v.to_be_bytes().to_vec()] };
        let base = PropertyList([(P_ATMOSPHERIC, floats(&[0.0, 20_000.0, 5.0, 10_000.0, 10.0, 2_500.0])), (P_SEND, text("cell_vo_submix"))].into_iter().collect());
        let child = PropertyList(
            [
                (P_DISTANCE_HIPASS, floats(&[0.0, 0.0, 10.0, 100.0])),
                (P_WETLEVEL_CURVE, floats(&[0.0, -0.007, 10.0, 0.1])),
                (P_WETLEVEL, number(0.05)),
            ]
            .into_iter()
            .collect(),
        );
        let a = Acoustics::from_chain(&[child, base]);
        assert!(a.sent_to_cell_vo());
        assert_eq!(a.lowpass_at(0.5), None, "open near the listener (19000 Hz)");
        assert_eq!(a.lowpass_at(7.5), Some(6_250.0));
        assert_eq!(a.hipass_at(0.0), None);
        assert_eq!(a.hipass_at(5.0), Some(50.0));
        // The reverb send is the larger of the curve and the static value, never negative.
        assert!((a.wet_at(0.0) - 0.05).abs() < 1e-6);
        assert!((a.wet_at(10.0) - 0.1).abs() < 1e-6);
        assert_eq!(Acoustics::default().wet_at(5.0), 0.0);
        assert!(!Acoustics::default().sent_to_cell_vo());
        assert_eq!(Acoustics::default().lowpass_at(5.0), None);
    }

    fn power(g: [f32; 2]) -> f32 {
        g[0] * g[0] + g[1] * g[1]
    }

    #[test]
    fn pan_is_constant_power_with_the_linear_amplitude_law() {
        // Centred (on the listener, straight ahead or behind): unity per channel (retail 1/sqrt 2
        // times the compensation).
        for offset in [Vec2::ZERO, Vec2::new(0.0, -9.0), Vec2::new(0.0, 9.0)] {
            let g = pan_gains(offset, 4.0, 12.0, false)[0];
            assert!((g[0] - 1.0).abs() < 1e-5 && (g[1] - 1.0).abs() < 1e-5, "{offset:?}: {g:?}");
        }
        // Hard right beyond mindistance: only the right speaker, the whole power (2).
        let right = pan_gains(Vec2::new(8.0, 0.0), 4.0, 12.0, false)[0];
        assert!(right[0].abs() < 1e-5 && (right[1] - SQRT_2).abs() < 1e-5, "{right:?}");
        let left = pan_gains(Vec2::new(-8.0, 0.0), 4.0, 12.0, false)[0];
        assert!(left[1].abs() < 1e-5 && (left[0] - SQRT_2).abs() < 1e-5, "{left:?}");
        // Halfway (offset 2 of mindistance 4, pandistance 0.5): t = 0.25 -> (0.25, 0.75) / norm.
        let half = pan_gains(Vec2::new(2.0, 0.0), 4.0, 12.0, false)[0];
        let n = (0.25f32 * 0.25 + 0.75 * 0.75).sqrt();
        assert!((half[0] - 0.25 / n * SQRT_2).abs() < 1e-5 && (half[1] - 0.75 / n * SQRT_2).abs() < 1e-5, "{half:?}");
        // The law is linear amplitude, not sine / cosine: sin / cos would give 0.383 / 0.924.
        assert!((half[0] / half[1] - 1.0 / 3.0).abs() < 1e-5);
        // Constant power everywhere.
        for x in [-9.0, -3.0, -1.0, 0.0, 0.5, 2.0, 7.0] {
            for y in [-6.0, 0.0, 6.0] {
                let g = pan_gains(Vec2::new(x, y), 4.0, 12.0, false)[0];
                assert!((power(g) - 2.0).abs() < 1e-4, "({x}, {y}): {g:?}");
            }
        }
        // A source on the right never moves the image to the left.
        assert!(pan_gains(Vec2::new(5.0, -5.0), 4.0, 12.0, false)[0][1] > pan_gains(Vec2::new(5.0, -5.0), 4.0, 12.0, false)[0][0]);
    }

    #[test]
    fn a_stereo_sample_is_wide_when_near_and_narrow_when_far() {
        // Straight ahead and close: the left channel goes mostly to the left speaker, the right
        // channel to the right.
        let near = pan_gains(Vec2::new(0.0, -2.0), 4.0, 12.0, true);
        assert!(near[0][0] > near[0][1] && near[1][1] > near[1][0], "{near:?}");
        // At maxdistance pansize is 0: both channels sit at the same point (the sum is centred).
        let far = pan_gains(Vec2::new(0.0, -12.0), 4.0, 12.0, true);
        assert!((far[0][0] - far[1][0]).abs() < 1e-5 && (far[0][1] - far[1][1]).abs() < 1e-5, "{far:?}");
        assert!((far[0][0] - far[0][1]).abs() < 1e-5);
        // The pandistance / pansize formulas.
        assert_eq!(pan_distance(2.0, 4.0), 0.5);
        assert_eq!(pan_distance(9.0, 4.0), 1.0);
        assert_eq!(pan_size(4.0, 4.0, 12.0), 1.0);
        assert_eq!(pan_size(8.0, 4.0, 12.0), 0.5);
        assert_eq!(pan_size(30.0, 4.0, 12.0), 0.0);
        assert_eq!(pan_size(7.0, 5.0, 5.0), 1.0);
    }

    #[test]
    fn the_listener_is_the_camera_look_point_during_cinematics() {
        let look = Vec2::new(5.0, -3.0);
        let avatar = Some(Vec2::new(1.0, 2.0));
        assert_eq!(listener_plane(false, look, avatar), avatar);
        assert_eq!(listener_plane(true, look, avatar), Some(look));
        assert_eq!(listener_plane(true, look, None), Some(look));
        assert_eq!(listener_plane(false, look, None), None);
    }

    /// Census of the track's Doppler curve (property 0x9BFC37AC, bound at 0xA20xxx next to
    /// `atmospheric`): how many installed soundProps carry it and with which points.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_sound_props_doppler_census() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let (mut total, mut with) = (0, 0);
        let mut shapes: std::collections::BTreeMap<String, usize> = Default::default();
        for key in index.records.keys().filter(|k| k.type_id == TYPE_SOUND_PROP) {
            let Ok(list) = index.read(*key).and_then(|b| crate::prop::parse(&b)) else {
                continue;
            };
            total += 1;
            if let Some(p) = list.0.get(&0x9BFC_37AC) {
                with += 1;
                *shapes.entry(format!("{:?}", floats(p))).or_default() += 1;
            }
        }
        println!("{with} of {total} soundProps carry a Doppler curve");
        for (shape, n) in shapes {
            println!("  {n} x {shape}");
        }
    }

    /// The installed soundProps carry the data the module reads.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_cell_sounds_carry_the_acoustics() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = AudioLibrary::from_index(&index);
        let eat = Acoustics::read(&library, "sfx_cell_eat");
        assert!(eat.sent_to_cell_vo());
        assert_eq!(eat.atmospheric, [(0.0, 20_000.0), (5.0, 10_000.0), (10.0, 2_500.0)]);
        assert!(eat.distance_hipass.is_empty());
        assert_eq!(eat.wet_at(5.0), 0.0, "the plain effect sounds send nothing to the reverb");
        let voice = Acoustics::read(&library, "vcl_idle_pk");
        assert!(voice.sent_to_cell_vo());
        assert_eq!(voice.atmospheric, [(0.0, 10_000.0), (10.0, 8_000.0), (30.0, 1_500.0)]);
        assert!(voice.distance_hipass.len() >= 8, "voices lose their lows with distance");
        assert!(voice.hipass_at(40.0).is_some());
        // The voices' reverb send grows with the distance (`vo_base` wetlevelcurve).
        assert_eq!(voice.wet_at(0.0), 0.0);
        assert!((voice.wet_at(8.59) - 0.059).abs() < 1e-3, "{}", voice.wet_at(8.59));
        assert!(voice.wet_at(30.0) > voice.wet_at(10.0));
        // Static `wetlevel` sounds.
        assert!((Acoustics::read(&library, "sfx_cell_explosion").wet_at(1.0) - 0.2).abs() < 1e-6);
        assert!((Acoustics::read(&library, "sfx_cell_chemical_drop").wet_at(1.0) - 0.15).abs() < 1e-6);
        // A sound outside the Cell has neither.
        let ui = Acoustics::read(&library, "ui_mission_complete");
        assert!(!ui.sent_to_cell_vo());
    }
}
