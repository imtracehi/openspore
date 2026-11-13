//! Civilization city music: the player's anthem played by the retail
//! `music_city_planner` patch and the rival cities' anthem samples
//! (`notes\civ-polish-re.md` M1-M6).
//!
//! RETAIL (march2017 addresses, VERIFIED unless marked):
//! * `cCivilization::AddCity` (0xBF44B0) gives a civ its city music when it
//!   gets its first city and starts it there: the player's civ a
//!   `cPlayerCityMusic` (ctor 0xEB0E70, which rolls a random anthem 0xEB0430
//!   from `audio~!playercitymusic` / `!playercitymusicbeats`), an NPC civ a
//!   `cNPCCityMusic` (0xEAB3E0) that loops one `cityhall_*` sample of its type
//!   (`audio~!npccitymusic`, least-used pick 0xEAB530) as a 3D sound. Every
//!   city the player adds plays the anthem once (`city_music_melody_play 0`,
//!   0xEACE80).
//! * `cPlayerCityMusic::Start` (0xEAD6C0) plays `music_city_planner`, sets
//!   `city_music_position_x/y/z` (0xEACD20) and sends the anthem (0xEAD170):
//!   ambience layers, rhythm, instrument, the three gains, every note
//!   (`note_id`, `note_pitch`, `note_duration`), `num_notes`, the loop flag.
//! * The patch plays the ambience layers everywhere (`is3d 0`), the drums and
//!   the melody in 3D at the capital (0x03F51892: 100 / 200 units, curve), and
//!   the whole anthem at a downbeat every 10..45 s while neither
//!   `mixmode_cityplanner` nor `mixmode_assetbrowser` is on.
//! * `npc_city_distance` (0xEA8AE0): listener to the nearest other city; the
//!   patch ducks its ambience with it.
//!
//! OpenSpore: the anthem is rolled once per campaign with the retail rules and
//! persisted in `CivCampaign::anthem` (ADAPTED: OpenSpore's own RNG, seeded from
//! the clock like the retail -1 seed, so the notes differ from any retail run;
//! there is no anthem editor and no `.cmp` import). The rival samples start at
//! each rival nation's city and stop when it becomes the player's.
//!
//! Conversion music (`notes\civ-planner-re.md` R1, VERIFIED 0xE89F60): a
//! religious vehicle group preaching at a city plays `music_civ_recruit` in
//! 3D at the group and sets its `civ_recruit_strength_level` 1..3 from the
//! group's power (0xBD8710); it stops when the group stops preaching.

use crate::civ::{Approach, CivCampaign};
use crate::game::RunProgress;
use crate::pd::engine::VoiceId;
use crate::pd::fnv;
use crate::pd_audio::PdMusic;
use crate::stages::GameStage;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The patch sound the player's city music plays (soundProp + bundle).
pub const CITY_PLANNER_SOUND: &str = "music_city_planner";
/// The conversion music (0xE8A04F `PlayAudio3D(0x2873C7EE, ...)`).
pub const RECRUIT_SOUND: &str = "music_civ_recruit";
/// Its instance parameter (0xE8A151 `SetProperty(0x187FFEDD, level)`).
pub const RECRUIT_LEVEL: &str = "civ_recruit_strength_level";
/// Its "good / bad" switch (0xE8A17C `SetProperty(0xFFB10599, 1 | 2)`): the
/// patch plays `music_civ_recruita/b/c` for 1 and the (not installed)
/// `music_civ_recruit_bada/b/c` for 2; 0 plays nothing.
pub const RECRUIT_EFFECTIVENESS: &str = "civ_recruit_effectiveness";
/// `city_music_melody_play`: play the anthem from a note (0xEACE80).
pub const MELODY_PLAY: &str = "city_music_melody_play";
/// The setup props (group `audio~` 0x021407EE, prop type).
pub const SETUP_PROP: u32 = 0xC603_60D0;
pub const BEATS_PROP: u32 = 0x7000_5117;
pub const NPC_PROP: u32 = 0xBA8F_3430;
const PROP_TYPE: u32 = 0x00B1_B104;
const AUDIO_GROUP: u32 = 0x0214_07EE;

/// The anthem editor's setup (0xEB1490 reads these properties).
#[derive(Clone, Debug, PartialEq)]
pub struct AnthemSetup {
    pub num_ambiences: u32,
    pub num_rhythms: u32,
    pub num_instruments: u32,
    pub max_layers: u32,
    pub min_notes: u32,
    pub max_notes: u32,
    /// 0x2AA962C3: weight of each note count `min_notes..=max_notes`.
    pub note_count_weights: Vec<f32>,
    pub beat_increment: f32,
    pub max_beats: f32,
    pub tonic: i32,
    pub min_pitch: i32,
    pub max_pitch: i32,
    /// 0x24ADC124: semitones of the seven scale degrees.
    pub scale: Vec<i32>,
    /// `NextDegree1..7`: weights of the next degree after each degree.
    pub next_degree: Vec<Vec<f32>>,
    /// `Beats0..9`: onset probability of each of the 16 quarter-beat steps.
    pub beats: Vec<Vec<f32>>,
}

impl AnthemSetup {
    /// The installed values (`audio~!playercitymusic` / `!playercitymusicbeats`),
    /// used when the props cannot be read.
    pub fn retail() -> Self {
        let b = |v: [f32; 16]| v.to_vec();
        AnthemSetup {
            num_ambiences: 20,
            num_rhythms: 10,
            num_instruments: 10,
            max_layers: 4,
            min_notes: 0,
            max_notes: 16,
            note_count_weights: vec![0.0, 0.0, 0.0, 0.3, 0.5, 0.5, 1.0, 1.0, 1.0, 0.5, 0.5, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0],
            beat_increment: 0.25,
            max_beats: 4.0,
            tonic: 60,
            min_pitch: 60,
            max_pitch: 84,
            scale: vec![0, 2, 4, 5, 7, 9, 11],
            next_degree: vec![
                vec![0.0, 0.1, 0.1, 0.35, 0.35, 0.35, 0.0],
                vec![0.05, 0.0, 0.05, 0.1, 0.7, 0.1, 0.0],
                vec![0.05, 0.05, 0.0, 0.15, 0.05, 0.7, 0.0],
                vec![0.1, 0.1, 0.05, 0.0, 0.7, 0.05, 0.0],
                vec![0.7, 0.05, 0.05, 0.1, 0.0, 0.1, 0.0],
                vec![0.1, 0.35, 0.1, 0.1, 0.35, 0.0, 0.0],
                vec![0.35, 0.1, 0.35, 0.05, 0.05, 0.1, 0.0],
            ],
            beats: vec![
                b([1.0, 0.0, 0.27, 0.0, 0.87, 0.0, 0.36, 0.0, 0.84, 0.0, 0.24, 0.0, 0.79, 0.0, 0.26, 0.0]),
                b([1.0, 0.0, 0.0, 0.0, 0.33, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.31, 0.0, 0.0, 0.0]),
                b([1.0, 0.0, 0.0, 0.0, 0.48, 0.0, 0.0, 0.0, 0.47, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0]),
                b([1.0, 0.0, 0.2, 0.0, 0.2, 0.0, 0.9, 0.0, 0.2, 0.0, 0.19, 0.0, 0.71, 0.0, 0.14, 0.0]),
                b([1.0, 0.0, 0.54, 0.0, 0.0, 0.0, 0.51, 0.0, 0.0, 0.0, 0.69, 0.0, 0.39, 0.0, 0.0, 0.0]),
                b([1.0, 0.0, 0.12, 0.0, 0.74, 0.0, 0.15, 0.0, 0.73, 0.0, 0.17, 0.0, 1.0, 0.0, 0.2, 0.0]),
                b([1.0, 0.0, 0.06, 0.0, 0.05, 0.0, 0.04, 0.0, 0.03, 0.0, 0.79, 0.0, 0.03, 0.0, 0.76, 0.0]),
                b([1.0, 0.0, 0.41, 0.0, 0.05, 0.0, 0.49, 0.0, 0.03, 0.0, 0.79, 0.0, 0.03, 0.0, 0.38, 0.0]),
                b([1.0, 0.0, 0.46, 0.0, 0.74, 0.0, 0.47, 0.0, 0.73, 0.0, 0.55, 0.0, 1.0, 0.0, 0.52, 0.0]),
                b([1.0, 0.0, 0.12, 0.0, 0.42, 0.0, 0.15, 0.0, 0.73, 0.0, 0.17, 0.0, 0.44, 0.0, 0.2, 0.0]),
            ],
        }
    }

    /// The setup from the installed props (fields missing there keep the
    /// [`Self::retail`] value).
    pub fn from_props(main: &crate::prop::PropertyList, beats: &crate::prop::PropertyList) -> Self {
        let mut s = Self::retail();
        let num = |id: u32| main.0.get(&id).and_then(|p| p.number());
        let int = |id: u32, v: &mut u32| {
            if let Some(n) = num(id) {
                *v = n.max(0.0) as u32;
            }
        };
        int(0x75B1_5A06, &mut s.num_ambiences);
        int(0x8FE7_E6BA, &mut s.num_rhythms);
        int(0xBAFD_17BB, &mut s.num_instruments);
        int(0x6679_061F, &mut s.max_layers);
        int(0xED9E_0342, &mut s.min_notes);
        int(0x8127_9214, &mut s.max_notes);
        if let Some(v) = num(0x5F8D_CA10).filter(|v| *v > 0.0) {
            s.beat_increment = v;
        }
        if let Some(v) = num(0xF01D_A328) {
            s.max_beats = v;
        }
        for (id, v) in [(0x6FA7_FDAC, &mut s.tonic), (0xDDD1_9567, &mut s.min_pitch), (0xD861_C661, &mut s.max_pitch)] {
            if let Some(n) = num(id) {
                *v = n as i32;
            }
        }
        if let Some(w) = main.0.get(&0x2AA9_62C3).map(floats).filter(|w| !w.is_empty()) {
            s.note_count_weights = w;
        }
        if let Some(sc) = main.0.get(&0x24AD_C124).map(ints).filter(|v| v.len() == 7) {
            s.scale = sc;
        }
        for d in 0..7 {
            if let Some(w) = main.0.get(&fnv(&format!("NextDegree{}", d + 1))).map(floats).filter(|w| w.len() == 7) {
                s.next_degree[d] = w;
            }
        }
        let patterns: Vec<Vec<f32>> =
            (0..10).filter_map(|i| beats.0.get(&fnv(&format!("Beats{i}"))).map(floats)).filter(|p| !p.is_empty()).collect();
        if !patterns.is_empty() {
            s.beats = patterns;
        }
        s
    }

    pub fn from_index(index: &crate::asset_catalog::ResourceIndex) -> Self {
        let key = |i: u32| crate::dbpf::ResourceKey { group_id: AUDIO_GROUP, instance_id: i, type_id: PROP_TYPE };
        match (index.properties(key(SETUP_PROP)), index.properties(key(BEATS_PROP))) {
            (Ok(main), Ok(beats)) => Self::from_props(&main, &beats),
            _ => Self::retail(),
        }
    }
}

fn floats(p: &crate::prop::Property) -> Vec<f32> {
    p.values.iter().filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?))).collect()
}

fn ints(p: &crate::prop::Property) -> Vec<i32> {
    p.values.iter().filter_map(|b| Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?) as i32)).collect()
}

fn default_gains() -> [f32; 3] {
    [1.0; 3]
}

/// A city anthem (`cPlayerCityMusic` +0x114 / +0x1C4 / +0x1C8 / +0x1CC /
/// +0x1E0 / +0x198 / +0x101).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CityAnthem {
    /// Ambience id (1..=NumAmbiences) per layer, 0 = empty layer.
    pub ambience: Vec<u32>,
    /// Drum set 1..=NumRhythms (0 = none).
    pub rhythm: u32,
    /// Melody instrument 1..=NumInstruments (0 = none).
    pub instrument: u32,
    /// MIDI pitch per note.
    pub pitches: Vec<i32>,
    /// Beats per note (the time to the next note).
    pub durations: Vec<f32>,
    /// Ambience, rhythm and melody gains (the ctor's 3 x 1.0).
    #[serde(default = "default_gains")]
    pub gains: [f32; 3],
    #[serde(default)]
    pub looped: bool,
    /// The seed it was rolled from.
    #[serde(default)]
    pub seed: u64,
    /// The beat pattern (`Beats0..9`, +0x154) the melody's onsets follow.
    #[serde(default)]
    pub pattern: u32,
}

/// OpenSpore's stand-in for `Math::RandomNumberGenerator` (xorshift64*;
/// ADAPTED: the retail sequence is not reproduced).
#[derive(Clone, Debug)]
pub struct AnthemRng(u64);

impl AnthemRng {
    pub fn new(seed: u64) -> Self {
        AnthemRng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// `RandomInt(n)`: 0..n (0 when n is 0).
    pub fn int(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        ((self.next() >> 32) % n as u64) as u32
    }
    /// `RandomFloat()`: 0..1.
    pub fn float(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// 0x5E8EF0: an index picked with probability `w_i / sum` of the positive
/// weights (uniform when none is positive; one entry -> 0).
pub fn weighted(rng: &mut AnthemRng, w: &[f32]) -> usize {
    let n = w.len();
    if n <= 1 {
        return 0;
    }
    let sum: f32 = w.iter().filter(|x| **x > 0.0).sum();
    if sum <= 0.0 {
        return rng.int(n as u32) as usize;
    }
    let mut r = (rng.float() * sum).max(0.0);
    for (i, x) in w.iter().enumerate() {
        if *x > 0.0 {
            if r < *x {
                return i;
            }
            r -= *x;
        }
    }
    n - 1
}

/// 0xEB0430: the random anthem a new `cPlayerCityMusic` starts with.
pub fn random_anthem(setup: &AnthemSetup, seed: u64) -> CityAnthem {
    let mut rng = AnthemRng::new(seed);
    // Ambience layers: the first always plays, the others may stay empty; an
    // id already used leaves the layer empty.
    let layers = setup.max_layers.max(1) as usize;
    let mut ambience = vec![0u32; layers];
    for i in 0..layers {
        let a = if i == 0 { rng.int(setup.num_ambiences) + 1 } else { rng.int(setup.num_ambiences + 1) };
        if a != 0 && !ambience[..i].contains(&a) {
            ambience[i] = a;
        }
    }
    let rhythm = rng.int(setup.num_rhythms) + 1;
    let instrument = rng.int(setup.num_instruments) + 1;
    // 0xEB00F0: the note count, the beat pattern, then 0xEAF4E0 per note.
    let span = (setup.max_notes.saturating_sub(setup.min_notes) + 1) as usize;
    let weights: Vec<f32> = (0..span).map(|i| setup.note_count_weights.get(i).copied().unwrap_or(0.0)).collect();
    let count = weighted(&mut rng, &weights) + setup.min_notes as usize;
    let pattern = rng.int(setup.beats.len() as u32);
    let mut a = CityAnthem {
        ambience,
        rhythm,
        instrument,
        pitches: Vec::with_capacity(count),
        durations: Vec::with_capacity(count),
        gains: [1.0; 3],
        looped: false,
        seed,
        pattern,
    };
    for _ in 0..count {
        push_note(setup, &mut a, &mut rng);
    }
    a
}

/// The scale degree of a note (`None` when out of the scale).
fn degree_of(setup: &AnthemSetup, pitch: i32) -> Option<usize> {
    setup.scale.iter().position(|d| d + setup.tonic == pitch)
}

/// 0xEAF4E0: append one note: degree = the first note RandomInt(7), else
/// weighted(NextDegree[previous degree]); pitch = scale[degree] + Tonic; its
/// onset the next step the beat pattern takes after the melody's end.
pub fn push_note(setup: &AnthemSetup, a: &mut CityAnthem, rng: &mut AnthemRng) {
    let previous = a.pitches.last().and_then(|p| degree_of(setup, *p));
    let d = match (a.pitches.is_empty(), previous) {
        (true, _) | (false, None) => rng.int(7) as usize,
        (false, Some(prev)) => weighted(rng, setup.next_degree.get(prev).map(Vec::as_slice).unwrap_or(&[])),
    }
    .min(setup.scale.len().saturating_sub(1));
    a.pitches.push(setup.scale.get(d).copied().unwrap_or(0) + setup.tonic);
    let pattern = setup.beats.get(a.pattern as usize).map(Vec::as_slice).unwrap_or(&[1.0]);
    let inc = setup.beat_increment.max(0.01);
    let total: f32 = a.durations.iter().sum();
    // The next onset: from the step after the current one, each step is
    // taken with the pattern's probability.
    let mut step = (total / inc).round_ties_even() as u32 + 1;
    let len = pattern.len().max(1) as u32;
    let mut guard = 0;
    while rng.float() > pattern[(step % len) as usize] && guard < 64 {
        step += 1;
        guard += 1;
    }
    let end = step as f32 * inc;
    a.durations.push(end - total);
}


/// The parameters 0xEAD170(1) sends when the city music starts, in order.
pub fn start_sequence(a: &CityAnthem) -> Vec<(&'static str, f32)> {
    let mut out = vec![];
    for (i, id) in a.ambience.iter().enumerate() {
        if *id != 0 {
            out.push(("city_music_ambi_note", *id as f32));
            out.push(("city_music_ambi_start", (i + 1) as f32));
        }
    }
    if a.rhythm != 0 {
        out.push(("city_music_rhythm_start", a.rhythm as f32));
    }
    if a.instrument != 0 {
        out.push(("city_music_melody_inst_start", a.instrument as f32));
    }
    for (name, g) in ["city_music_ambi_gain", "city_music_rhythm_gain", "city_music_melody_gain"].into_iter().zip(a.gains) {
        out.push((name, g));
    }
    let n = a.pitches.len().min(a.durations.len());
    for i in 0..n {
        out.push(("city_music_melody_note_id", i as f32));
        out.push(("city_music_melody_note_pitch", a.pitches[i] as f32));
        out.push(("city_music_melody_note_duration", a.durations[i]));
    }
    out.push(("city_music_melody_num_notes", n as f32));
    if a.looped {
        out.push(("city_music_melody_loop", 1.0));
        out.push((MELODY_PLAY, 0.0));
    }
    out
}

/// 0xEB00F0 (the anthem editor's "Generate new anthem"): a new melody (note
/// count, beat pattern, notes) for the same ambience, drums and instrument.
pub fn new_melody(setup: &AnthemSetup, a: &mut CityAnthem, seed: u64) {
    let mut rng = AnthemRng::new(seed);
    let span = (setup.max_notes.saturating_sub(setup.min_notes) + 1) as usize;
    let weights: Vec<f32> = (0..span).map(|i| setup.note_count_weights.get(i).copied().unwrap_or(0.0)).collect();
    let count = weighted(&mut rng, &weights) + setup.min_notes as usize;
    a.pattern = rng.int(setup.beats.len() as u32);
    a.pitches.clear();
    a.durations.clear();
    for _ in 0..count {
        push_note(setup, a, &mut rng);
    }
}

/// The parameters the anthem's setters send when it changes from `old` to
/// `new` (the editor's 0xEACD80 drums, 0xEACE00 instrument, 0xEAE0C0 /
/// 0xEADD30 ambience layers, 0xEAD060 gains, 0xEACF40 loop, the notes).
pub fn anthem_diff(old: &CityAnthem, new: &CityAnthem) -> Vec<(&'static str, f32)> {
    let mut out = vec![];
    let layers = old.ambience.len().max(new.ambience.len());
    for i in 0..layers {
        let (a, b) = (old.ambience.get(i).copied().unwrap_or(0), new.ambience.get(i).copied().unwrap_or(0));
        if a != b {
            if a != 0 {
                out.push(("city_music_ambi_stop", (i + 1) as f32));
            }
            if b != 0 {
                out.push(("city_music_ambi_note", b as f32));
                out.push(("city_music_ambi_start", (i + 1) as f32));
            }
        }
    }
    if old.rhythm != new.rhythm {
        if old.rhythm != 0 {
            out.push(("city_music_rhythm_stop", old.rhythm as f32));
        }
        if new.rhythm != 0 {
            out.push(("city_music_rhythm_start", new.rhythm as f32));
        }
    }
    if old.instrument != new.instrument {
        if old.instrument != 0 {
            out.push(("city_music_melody_inst_stop", old.instrument as f32));
        }
        if new.instrument != 0 {
            out.push(("city_music_melody_inst_start", new.instrument as f32));
        }
    }
    for (k, name) in ["city_music_ambi_gain", "city_music_rhythm_gain", "city_music_melody_gain"].into_iter().enumerate() {
        if old.gains[k] != new.gains[k] {
            out.push((name, new.gains[k]));
        }
    }
    if old.pitches != new.pitches || old.durations != new.durations {
        let n = new.pitches.len().min(new.durations.len());
        for i in 0..n {
            out.push(("city_music_melody_note_id", i as f32));
            out.push(("city_music_melody_note_pitch", new.pitches[i] as f32));
            out.push(("city_music_melody_note_duration", new.durations[i]));
        }
        out.push(("city_music_melody_num_notes", n as f32));
    }
    if old.looped != new.looped {
        out.push(("city_music_melody_loop", if new.looped { 1.0 } else { 0.0 }));
    }
    out
}

/// The parameters 0xEAD170(0) sends when the city music stops (the
/// context stop destroys the instance here; kept for completeness).
#[allow(dead_code)]
pub fn stop_sequence(a: &CityAnthem) -> Vec<(&'static str, f32)> {
    let mut out = vec![];
    for (i, id) in a.ambience.iter().enumerate() {
        if *id != 0 {
            out.push(("city_music_ambi_stop", (i + 1) as f32));
        }
    }
    if a.rhythm != 0 {
        out.push(("city_music_rhythm_stop", a.rhythm as f32));
    }
    if a.instrument != 0 {
        out.push(("city_music_melody_inst_stop", a.instrument as f32));
    }
    out.push(("city_music_melody_num_notes", 0.0));
    out.push(("city_music_melody_loop", 0.0));
    out
}

/// `audio~!npccitymusic` list names in the retail order (0x014881E4).
pub const NPC_LISTS: [&str; 5] = ["Military_Samples", "Economic_Samples", "Religious_Samples", "Space_Samples", "Grox_Samples"];

/// 0xBEF790: the NPC city music type of a civ type.
pub fn npc_type(kind: Approach) -> usize {
    match kind {
        Approach::Military => 0,
        Approach::Economic => 1,
        Approach::Religious => 2,
    }
}

/// The NPC city music manager (0xEAB6B0): each list's samples with their use
/// counts.
#[derive(Clone, Debug, Default)]
pub struct NpcMusic {
    pub lists: Vec<Vec<(u32, u32)>>,
}

impl NpcMusic {
    /// The installed lists; the names of the retail prop when it cannot be read.
    pub fn from_index(index: Option<&crate::asset_catalog::ResourceIndex>) -> Self {
        let props = index.and_then(|i| {
            i.properties(crate::dbpf::ResourceKey { group_id: AUDIO_GROUP, instance_id: NPC_PROP, type_id: PROP_TYPE }).ok()
        });
        let fallback: [&[&str]; 5] = [
            &["cityhall_alien", "cityhall_alientechno", "cityhall_kindaangry"],
            &["cityhall_coolio", "cityhall_drughazed", "cityhall_elevator"],
            &["cityhall_eastern", "cityhall_neighbor1", "cityhall_tibetan", "cityhall_westreligious"],
            &[],
            &["alien_music_proposal_three"],
        ];
        let lists = NPC_LISTS
            .iter()
            .zip(fallback)
            .map(|(name, names)| {
                let ids: Vec<u32> = props
                    .as_ref()
                    .and_then(|p| p.0.get(&fnv(name)))
                    .map(|p| ints(p).into_iter().map(|v| v as u32).collect())
                    .filter(|v: &Vec<u32>| !v.is_empty())
                    .unwrap_or_else(|| names.iter().map(|n| fnv(n)).collect());
                ids.into_iter().map(|i| (i, 0)).collect()
            })
            .collect();
        NpcMusic { lists }
    }

    /// 0xEAB530: a uniform pick among the list's least-used samples; its use
    /// count goes up.
    pub fn pick(&mut self, kind: usize, rng: &mut AnthemRng) -> Option<u32> {
        let list = self.lists.get_mut(kind)?;
        let least = list.iter().map(|e| e.1).min()?;
        let ties: Vec<usize> = (0..list.len()).filter(|i| list[*i].1 == least).collect();
        let i = ties[rng.int(ties.len() as u32) as usize];
        list[i].1 += 1;
        Some(list[i].0)
    }

    /// The sample stops: one use fewer.
    pub fn release(&mut self, sample: u32) {
        for l in &mut self.lists {
            if let Some(e) = l.iter_mut().find(|e| e.0 == sample) {
                e.1 = e.1.saturating_sub(1);
                return;
            }
        }
    }
}

/// The city music feed's state while the `civ` context runs.
#[derive(Default)]
pub struct CityMusicFeed {
    on: bool,
    setup: Option<AnthemSetup>,
    npc: Option<NpcMusic>,
    rng: Option<AnthemRng>,
    /// The player's city music runs (instance), at this capital.
    started: Option<usize>,
    owned: usize,
    /// Rival city index -> (voice, sample).
    rivals: HashMap<usize, (VoiceId, u32)>,
    npc_distance: Option<f32>,
    comm: Option<bool>,
    /// The anthem the patch was last sent (the editor's changes are sent as
    /// the setters would, `anthem_diff`).
    sent: Option<CityAnthem>,
    /// The City Planner's mix mode as last sent.
    planner: Option<bool>,
}

/// A fresh anthem seed (retail seeds the anthem RNG with -1: a new anthem each
/// game).
fn clock_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x5EED)
}

/// Make sure the campaign has its anthem (rolled once, then saved).
pub fn ensure_anthem(campaign: &mut CivCampaign, setup: &AnthemSetup) {
    if campaign.anthem.is_none() {
        campaign.anthem = Some(random_anthem(setup, clock_seed()));
    }
}

/// The player's capital: the first city the player owns.
pub fn capital(campaign: &CivCampaign) -> Option<usize> {
    campaign.cities.iter().find(|c| c.owned()).map(|c| c.index)
}

/// Feed the city music while the `civ` context runs: start the player's
/// `music_city_planner` at the capital with the anthem, play the anthem when
/// a city is added, the rival anthem samples, the listener and
/// `npc_city_distance`.
#[allow(clippy::too_many_arguments)]
pub fn feed_city_music(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    mut run: ResMut<RunProgress>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    originals: Option<Res<crate::Originals>>,
    comm: Option<Res<crate::civ_comm::CivComm>>,
    mut feed: Local<CityMusicFeed>,
    planner: Option<Res<crate::civ_planner::CivPlanner>>,
    editor: Option<ResMut<crate::civ_anthem::AnthemEditor>>,
) {
    if *stage.get() != GameStage::Civilization || !pd.context_active("civ") {
        if feed.on {
            // The context stop took the instance and the rival voices.
            feed.on = false;
            feed.started = None;
            feed.rivals.clear();
            feed.npc_distance = None;
            feed.comm = None;
            feed.sent = None;
            feed.planner = None;
            pd.listener = None;
        }
        return;
    }
    let pd = &mut *pd;
    let feed = &mut *feed;
    feed.on = true;
    if feed.setup.is_none() {
        feed.setup = Some(originals.as_deref().map_or_else(AnthemSetup::retail, |o| AnthemSetup::from_index(&o.resource_index)));
        feed.npc = Some(NpcMusic::from_index(originals.as_deref().map(|o| &o.resource_index)));
        feed.rng = Some(AnthemRng::new(clock_seed()));
    }
    if run.civ_campaign.anthem.is_none() {
        let setup = feed.setup.clone().unwrap_or_else(AnthemSetup::retail);
        ensure_anthem(&mut run.civ_campaign, &setup);
    }
    let campaign = &run.civ_campaign;
    // The listener is the camera.
    let listener = camera.single().ok().map(|c| c.translation());
    pd.listener = listener;
    // The player's city music: started at the capital (the civ's first city).
    let owned = campaign.cities.iter().filter(|c| c.owned()).count();
    if feed.started.is_none() {
        if let (Some(cap), Some(anthem)) = (capital(campaign), campaign.anthem.as_ref()) {
            let id = fnv(CITY_PLANNER_SOUND);
            if pd.instance_of(id).is_none() {
                pd.start_sound(id, CITY_PLANNER_SOUND, "civ");
            }
            if pd.instance_of(id).is_some() {
                let p = campaign.cities[cap].center();
                pd.set_audio_param("city_music_position_x", p.x);
                pd.set_audio_param("city_music_position_y", p.y);
                pd.set_audio_param("city_music_position_z", p.z);
                // ADAPTED: retail opens the city planner when the Civ game
                // starts (city hall, vehicle, anthem); its mix mode turning
                // off is what lets `occasionalplay` replay the anthem.
                pd.set_audio_param("mixmode_cityplanner", 0.0);
                for (name, v) in start_sequence(anthem) {
                    pd.set_audio_param(name, v);
                }
                // AddCity: the anthem plays once for the city.
                pd.set_audio_param(MELODY_PLAY, 0.0);
                feed.started = Some(cap);
                feed.owned = owned;
                feed.sent = Some(anthem.clone());
                feed.planner = Some(false);
            }
        }
    } else if owned > feed.owned {
        // Every city the player adds plays the anthem from its first note.
        pd.set_audio_param(MELODY_PLAY, 0.0);
        feed.owned = owned;
    } else {
        feed.owned = owned;
    }
    // The anthem editor: its changes go to the patch as the setters send
    // them, and its play buttons (0xEACE80 / 0xEACEE0).
    if feed.started.is_some() {
        if let (Some(sent), Some(now)) = (feed.sent.as_ref(), campaign.anthem.as_ref()) {
            if sent != now {
                for (name, v) in anthem_diff(sent, now) {
                    pd.set_audio_param(name, v);
                }
                feed.sent = Some(now.clone());
            }
        }
        if let Some(play) = editor.and_then(|mut e| e.play.take()) {
            match play {
                crate::civ_anthem::PlayRequest::FromStart => pd.set_audio_param(MELODY_PLAY, 0.0),
                crate::civ_anthem::PlayRequest::Note(n) => pd.set_audio_param("city_music_melody_play_note", n as f32),
            }
        }
        // INFERRED: the planner screen runs `mixmode_cityplanner` (the patch
        // holds its occasional anthem back while it is on).
        let open = planner.is_some_and(|p| p.is_open());
        if feed.planner != Some(open) {
            feed.planner = Some(open);
            pd.set_audio_param("mixmode_cityplanner", if open { 1.0 } else { 0.0 });
        }
    }
    // Rival anthems: one looped sample per rival nation at its city.
    let rivals: Vec<(usize, Approach, Vec3)> =
        campaign.cities.iter().filter(|c| !c.owned()).map(|c| (c.index, c.kind, c.center())).collect();
    let gone: Vec<usize> = feed.rivals.keys().copied().filter(|i| !rivals.iter().any(|r| r.0 == *i)).collect();
    for i in gone {
        if let Some((voice, sample)) = feed.rivals.remove(&i) {
            pd.stop_voice(voice);
            if let Some(npc) = feed.npc.as_mut() {
                npc.release(sample);
            }
        }
    }
    for (i, kind, at) in &rivals {
        if feed.rivals.contains_key(i) {
            continue;
        }
        let (Some(npc), Some(rng)) = (feed.npc.as_mut(), feed.rng.as_mut()) else { break };
        if let Some(sample) = npc.pick(npc_type(*kind), rng) {
            if let Some(voice) = pd.start_spatial_plain(sample, "civ", *at) {
                feed.rivals.insert(*i, (voice, sample));
            } else {
                npc.release(sample);
                // Not installed: do not retry every frame.
                feed.rivals.insert(*i, (VoiceId::MAX, sample));
            }
        }
    }
    // `npc_city_distance`: the listener to the nearest rival city (on change).
    if let Some(l) = listener {
        if let Some(d) = rivals.iter().map(|r| l.distance(r.2)).min_by(|a, b| a.total_cmp(b)) {
            let d = d.round();
            if feed.npc_distance != Some(d) {
                feed.npc_distance = Some(d);
                pd.set_audio_param("npc_city_distance", d);
            }
        }
    }
    // The comm screen's mix mode (its mixer patch ducks the city music).
    let open = comm.is_some_and(|c| c.open);
    if feed.comm != Some(open) {
        feed.comm = Some(open);
        pd.set_audio_param("mixmode_commscreen", if open { 1.0 } else { 0.0 });
    }
}

/// VERIFIED 0xE89F60 / 0xBD8710: the strength level of a preaching group:
/// each vehicle adds `(power + 0.5) x 2/3` (`cVehicle` +0xC50); the level is
/// `ceil(3 x sum / count)`, 1 for an empty group, clamped to 1..3.
pub fn recruit_strength(powers: &[f32]) -> f32 {
    if powers.is_empty() {
        return 1.0;
    }
    let sum: f32 = powers.iter().map(|p| (p + 0.5) * (2.0 / 3.0)).sum();
    (sum * 3.0 / powers.len() as f32).ceil().clamp(1.0, 3.0)
}

/// What [`feed_recruit_music`] remembers.
#[derive(Default)]
pub struct RecruitFeed {
    playing: bool,
    level: Option<f32>,
    effectiveness: Option<f32>,
}

/// The conversion music (0xE89F60, the cultural group's update): while a
/// group of religious vehicles preaches at a city, `music_civ_recruit` plays
/// at the group (3D, its 0x03F51892 roll-off) with the group's strength
/// level; it stops when the preaching ends. ADAPTED: OpenSpore has no vehicle
/// groups, the vehicles of one nation preaching at one city form the group,
/// and only the group nearest the listener plays (one instance).
pub fn feed_recruit_music(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    units: Query<(&crate::civ_rts::CivUnit, &Transform)>,
    run: Res<RunProgress>,
    mut feed: Local<RecruitFeed>,
) {
    let id = fnv(RECRUIT_SOUND);
    if *stage.get() != GameStage::Civilization || !pd.context_active("civ") {
        // The context stop took the instance.
        *feed = RecruitFeed::default();
        return;
    }
    let mut groups: HashMap<(usize, usize), (Vec3, Vec<f32>)> = HashMap::new();
    for (unit, t) in &units {
        if let crate::civ_rts::UnitAction::Preaching(c) = unit.action {
            if unit.kind == Approach::Religious && unit.hp > 0.0 {
                let g = groups.entry((unit.nation, c)).or_insert((Vec3::ZERO, Vec::new()));
                g.0 += t.translation;
                g.1.push(unit.stats.power);
            }
        }
    }
    let listener = pd.listener.unwrap_or_default();
    let nearest = groups
        .iter()
        .map(|(key, (sum, powers))| (*key, *sum / powers.len().max(1) as f32, powers))
        .min_by(|a, b| a.1.distance(listener).total_cmp(&b.1.distance(listener)));
    match nearest {
        Some(((nation, city), at, powers)) => {
            if pd.instance_of(id).is_none() {
                pd.start_sound(id, RECRUIT_SOUND, "civ");
                feed.level = None;
                feed.effectiveness = None;
            }
            feed.playing = pd.instance_of(id).is_some();
            if feed.playing {
                pd.set_sound_position(id, at);
                let level = recruit_strength(powers);
                if feed.level != Some(level) {
                    feed.level = Some(level);
                    pd.set_sound_param(RECRUIT_SOUND, RECRUIT_LEVEL, level);
                }
                // INFERRED: "good" (1) while the preaching still converts the
                // city (the group's rate +0x20C is not zero), else "bad" (2).
                let progress = run.civ_campaign.cities.get(city).map_or(1.0, |c| {
                    if nation == crate::civ::HOME_INDEX { c.conversion } else { c.rival_conversion }
                });
                let effectiveness = if progress < 1.0 { 1.0 } else { 2.0 };
                if feed.effectiveness != Some(effectiveness) {
                    feed.effectiveness = Some(effectiveness);
                    pd.set_sound_param(RECRUIT_SOUND, RECRUIT_EFFECTIVENESS, effectiveness);
                }
            }
        }
        None if feed.playing => {
            pd.stop_sound(id);
            *feed = RecruitFeed::default();
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recruit_level_follows_the_group_power() {
        // Default vehicles (power 0.33): ceil(2 x 0.33 + 1) = 2.
        assert_eq!(recruit_strength(&[0.33, 0.33, 0.33]), 2.0);
        assert_eq!(recruit_strength(&[0.0]), 1.0);
        assert_eq!(recruit_strength(&[2.0, 2.0]), 3.0);
        assert_eq!(recruit_strength(&[]), 1.0);
        assert_eq!(recruit_strength(&[0.6]), 3.0);
    }

    #[test]
    fn random_anthems_follow_the_setup() {
        let setup = AnthemSetup::retail();
        for seed in 0..200u64 {
            let a = random_anthem(&setup, seed);
            assert_eq!(a.ambience.len(), 4);
            assert!(a.ambience[0] >= 1 && a.ambience[0] <= 20, "the first layer always plays: {a:?}");
            let used: Vec<u32> = a.ambience.iter().copied().filter(|x| *x != 0).collect();
            let mut dedup = used.clone();
            dedup.sort_unstable();
            dedup.dedup();
            assert_eq!(dedup.len(), used.len(), "no ambience twice");
            assert!((1..=10).contains(&a.rhythm) && (1..=10).contains(&a.instrument));
            // Note counts with a positive weight: 3..=11.
            assert!((3..=11).contains(&a.pitches.len()), "{}", a.pitches.len());
            assert_eq!(a.pitches.len(), a.durations.len());
            for p in &a.pitches {
                assert!([60, 62, 64, 65, 67, 69, 71].contains(p), "pitches are scale degrees over the tonic: {p}");
            }
            for (i, d) in a.durations.iter().enumerate() {
                let q = d / 0.25;
                assert!((q - q.round()).abs() < 1e-4 && *d > 0.0, "quarter-beat steps: {d}");
                if i == 0 {
                    assert!(*d >= 0.5, "step 1 is never an onset: {d}");
                }
            }
        }
    }

    #[test]
    fn the_start_sequence_writes_each_note_after_its_id() {
        let a = CityAnthem {
            ambience: vec![3, 0, 7, 0],
            rhythm: 2,
            instrument: 5,
            pitches: vec![60, 67],
            durations: vec![1.0, 0.5],
            gains: [1.0; 3],
            looped: false,
            seed: 0,
            pattern: 0,
        };
        let s = start_sequence(&a);
        let names: Vec<&str> = s.iter().map(|x| x.0).collect();
        assert_eq!(&names[..4], ["city_music_ambi_note", "city_music_ambi_start", "city_music_ambi_note", "city_music_ambi_start"]);
        assert_eq!(s[1].1, 1.0);
        assert_eq!(s[3].1, 3.0, "layer 3 holds ambience 7");
        let id = names.iter().position(|n| *n == "city_music_melody_note_id").unwrap();
        assert_eq!(&names[id..id + 3], ["city_music_melody_note_id", "city_music_melody_note_pitch", "city_music_melody_note_duration"]);
        assert_eq!(*s.last().unwrap(), ("city_music_melody_num_notes", 2.0));
        let stop = stop_sequence(&a);
        assert_eq!(stop[0], ("city_music_ambi_stop", 1.0));
        assert!(stop.contains(&("city_music_rhythm_stop", 2.0)) && stop.contains(&("city_music_melody_inst_stop", 5.0)));
    }

    #[test]
    fn weighted_picks_follow_the_weights() {
        let mut rng = AnthemRng::new(7);
        let w = [0.0, 1.0, 3.0, 0.0];
        let mut counts = [0usize; 4];
        for _ in 0..4000 {
            counts[weighted(&mut rng, &w)] += 1;
        }
        assert_eq!(counts[0] + counts[3], 0);
        let ratio = counts[2] as f32 / counts[1] as f32;
        assert!((2.4..3.6).contains(&ratio), "{counts:?}");
        assert_eq!(weighted(&mut rng, &[5.0]), 0);
    }

    #[test]
    fn npc_music_picks_the_least_used_sample() {
        let mut npc = NpcMusic::from_index(None);
        let mut rng = AnthemRng::new(1);
        let picks: Vec<u32> = (0..3).map(|_| npc.pick(0, &mut rng).unwrap()).collect();
        let mut sorted = picks.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "three military cities hear three different samples");
        npc.release(picks[1]);
        assert_eq!(npc.pick(0, &mut rng), Some(picks[1]));
        assert_eq!(npc_type(Approach::Religious), 2);
    }

    #[test]
    fn the_city_music_rolls_off_with_the_retail_curve() {
        let s = crate::pd::library::Spatial { min: 100.0, max: 200.0, curve: vec![(0.0, 1.0), (0.5, 0.8), (0.8, 0.4), (1.0, 0.0)] };
        assert_eq!(s.gain(50.0), 1.0);
        assert!((s.gain(150.0) - 0.8).abs() < 1e-5);
        assert!((s.gain(180.0) - 0.4).abs() < 1e-5);
        assert_eq!(s.gain(250.0), 0.0);
    }

    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_setup_and_the_patch_play_the_anthem() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        assert_eq!(AnthemSetup::from_index(&index), AnthemSetup::retail(), "the built-in copy matches the installed props");
        let npc = NpcMusic::from_index(Some(&index));
        assert_eq!(npc.lists[0].len(), 3);
        assert_eq!(npc.lists[3].len(), 20);
        let lib = std::sync::Arc::new(crate::pd::library::PdLibrary::from_index(&index));
        let spatial = lib.spatial(fnv(CITY_PLANNER_SOUND)).expect("music_city_planner is positional");
        assert_eq!((spatial.min, spatial.max), (100.0, 200.0));
        let audio = std::sync::Arc::new(crate::game_audio::AudioLibrary::from_index(&index));
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start(CITY_PLANNER_SOUND).unwrap();
        h.step(1.0 / 60.0);
        let anthem = random_anthem(&AnthemSetup::retail(), 42);
        h.engine.set_global_param("mixmode_cityplanner", 0.0);
        for (name, v) in start_sequence(&anthem) {
            h.engine.set_global_param(name, v);
        }
        h.engine.set_global_param(MELODY_PLAY, 0.0);
        while h.now < 90.0 {
            h.step(1.0 / 60.0);
        }
        let mut drums = 0;
        let mut notes = 0;
        let mut ambience = 0;
        let mut downbeats = 0;
        let mut active = 0;
        for e in &h.events {
            match &e.ev {
                crate::pd::trace::Ev::Start { sound, params, .. } => {
                    const DRUMS: [&str; 10] = [
                        "astonish120_", "convince140_", "euromech90_", "fascinate130_", "haunted100_", "interest90_", "techno110_", "tribal120_",
                        "trip_shaman110_", "wonder90_",
                    ];
                    if params.iter().any(|p| p.0 == "notenumber") {
                        notes += 1;
                    } else if DRUMS.iter().any(|d| sound.starts_with(d)) {
                        drums += 1;
                    } else {
                        ambience += 1;
                    }
                }
                crate::pd::trace::Ev::SendGame { name, .. } if name == "city_music_rhythm_downbeat" => downbeats += 1,
                crate::pd::trace::Ev::SendGame { name, value } if name == "city_music_melody_active_note" && *value > 0.0 => active += 1,
                _ => {}
            }
        }
        eprintln!("city music 90 s: {drums} drum loops, {notes} melody notes, {ambience} ambience voices, {downbeats} downbeats, {active} active notes");
        assert!(drums > 0 && downbeats > 0, "the rhythm plays and beats");
        assert!(notes >= anthem.pitches.len(), "the anthem's notes play");
        assert!(ambience > 0, "the ambience layers play");
    }
}
