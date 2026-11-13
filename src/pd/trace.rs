//! Headless Pd runs: `openspore --pd-trace <patch> [--seconds N]` starts a
//! patch sound (soundProp or bundle name) from the installed data, drives it
//! with a scripted parameter timeline and prints every play / stop /
//! parameter / mixer / sendgame event. For the Cell score
//! (`cellgame_seed_music6` / `cellgame_seed_music`) the run follows a scripted
//! Cell session and checks the result against the retail behaviour in
//! `notes\cell-audio-notes.md` 3 (one loop per level from the `coll`, 2 s /
//! 1 s crossfades, chase layer, death, level-switch and egg-lay sounds, beach
//! loop at level 10).

use super::engine::{Engine, HostCmd, InstId, VoiceId};
use super::library::{Instrument, PdLibrary};
use super::{fnv, Atom};
use crate::game_audio::AudioLibrary;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Start { voice: VoiceId, sound: String, params: Vec<(String, String)> },
    Stop { voice: VoiceId },
    Ended { voice: VoiceId },
    Param { voice: VoiceId, name: String, value: f32 },
    Mixer { group: String, name: String, value: f32 },
    SendGame { name: String, value: f32 },
    Print(String),
    Game(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraceEvent {
    pub t: f64,
    pub ev: Ev,
}

struct HVoice {
    sound: String,
    ends: f64,
    stopped_at: Option<f64>,
    release: f32,
}

/// The engine with a simulated host (voices end after their sample length).
pub struct Headless {
    pub engine: Engine,
    lib: Arc<PdLibrary>,
    audio: Arc<AudioLibrary>,
    voices: HashMap<VoiceId, HVoice>,
    instruments: HashMap<String, Option<Instrument>>,
    pub events: Vec<TraceEvent>,
    pub now: f64,
}

impl Headless {
    pub fn new(lib: Arc<PdLibrary>, audio: Arc<AudioLibrary>, seed: u32) -> Self {
        let mut engine = Engine::new(seed);
        engine.data_files = lib.data_files();
        Headless { engine, lib, audio, voices: HashMap::new(), instruments: HashMap::new(), events: vec![], now: 0.0 }
    }

    /// Start a patch sound by soundProp or bundle name.
    pub fn start(&mut self, name: &str) -> Result<InstId, String> {
        let patch = self.lib.patch_sound(fnv(name)).ok_or_else(|| format!("{name}: not a Pd patch sound or bundle"))?;
        let bundle = self.lib.bundle(patch.bundle).ok_or_else(|| format!("{name}: bundle {:08X} unreadable", patch.bundle))?;
        let lib = self.lib.clone();
        let resolver = lib.resolver(&bundle);
        let inst = self.engine.create_instance(&bundle.name, &bundle.top, &resolver, &[("gain", patch.gain), ("pitch", patch.pitch)]);
        self.events.push(TraceEvent {
            t: self.now,
            ev: Ev::Game(format!(
                "instance {inst}: {} ({} objects, soundProp gain {:.2}, fadeout {:.1})",
                bundle.name,
                self.engine.object_count(),
                patch.gain,
                patch.fade_out
            )),
        });
        self.drain();
        Ok(inst)
    }

    pub fn game(&mut self, text: String) {
        self.events.push(TraceEvent { t: self.now, ev: Ev::Game(text) });
    }

    /// One frame: tick the scheduler, apply host commands, end voices.
    pub fn step(&mut self, dt: f64) {
        self.now += dt;
        self.engine.tick(self.now);
        self.drain();
        let now = self.now;
        let mut ended: Vec<VoiceId> = self
            .voices
            .iter()
            .filter(|(_, v)| now >= v.ends || v.stopped_at.is_some_and(|s| now >= s + v.release as f64))
            .map(|(k, _)| *k)
            .collect();
        ended.sort_unstable();
        for v in ended {
            self.voices.remove(&v);
            self.engine.voice_ended(v);
            self.events.push(TraceEvent { t: now, ev: Ev::Ended { voice: v } });
        }
    }

    fn drain(&mut self) {
        let now = self.now;
        for cmd in self.engine.take_commands() {
            let ev = match cmd {
                HostCmd::Start { voice, sound, params, .. } => {
                    let map: HashMap<String, Atom> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
                    let mut rng = rand::rng();
                    let res = crate::pd_audio::resolve_voice(&self.lib, &self.audio, &sound, &map, &mut self.instruments, &mut rng);
                    let (ends, release) = match &res {
                        None => (now, 0.0),
                        Some(r) if r.looped => (f64::INFINITY, r.fade_out),
                        Some(r) => {
                            let len = match r.env.note {
                                Some(n) => n + r.env.release,
                                None => self.lib.sample_seconds(r.sample).unwrap_or(1.0) / (r.pitch * map.get("pitch").map(|a| a.float()).unwrap_or(1.0)).max(0.01),
                            };
                            (now + len as f64, r.fade_out)
                        }
                    };
                    let release = map.get("fadeout").map(|a| a.float()).filter(|f| *f > 0.0).unwrap_or(release);
                    self.voices.insert(voice, HVoice { sound: sound.to_string(), ends, stopped_at: None, release });
                    let mut p: Vec<(String, String)> = params.iter().map(|(k, v)| (k.to_string(), v.text())).collect();
                    if res.is_none() {
                        p.push(("(not installed)".into(), String::new()));
                    }
                    Ev::Start { voice, sound: sound.to_string(), params: p }
                }
                HostCmd::Stop { voice } => {
                    if let Some(v) = self.voices.get_mut(&voice) {
                        v.stopped_at.get_or_insert(now);
                    }
                    Ev::Stop { voice }
                }
                HostCmd::Param { voice, name, value } => {
                    if let Some(v) = self.voices.get_mut(&voice) {
                        if &*name == "fadeout" && value.float() > 0.0 {
                            v.release = value.float();
                        }
                    }
                    Ev::Param { voice, name: name.to_string(), value: value.float() }
                }
                HostCmd::Pause { .. } => continue,
                HostCmd::Mixer { group, name, value, .. } => Ev::Mixer { group: group.to_string(), name: name.to_string(), value: value.float() },
                HostCmd::SendGame { name, value, .. } => Ev::SendGame { name: name.to_string(), value },
                HostCmd::Print { text, .. } => Ev::Print(text),
            };
            self.events.push(TraceEvent { t: now, ev });
        }
    }

    pub fn voice_sound(&self, v: VoiceId) -> Option<&str> {
        self.voices.get(&v).map(|v| v.sound.as_str())
    }
}

/// The level loops of `main_loops` (`coll 11`, level 0..=9).
pub const CELL_LOOPS: [&str; 10] = [
    "music_cell_loopa",
    "music_cell_loopb",
    "music_cell_loopc",
    "music_cell_loope",
    "music_cell_loopd",
    "music_cell_loopf",
    "music_cell_loopg",
    "music_cell_looph",
    "music_cell_loopi",
    "music_cell_loopj",
];

/// The scripted Cell session: `cell_level` per time (1 + food/1000, 0 while
/// dying).
pub fn cell_level_at(t: f64) -> f32 {
    match t {
        t if t < 20.0 => 1.05,
        t if t < 40.0 => 1.15,
        t if t < 60.0 => 1.25,
        t if t < 63.0 => 0.0,
        t if t < 80.0 => 1.25,
        t if t < 100.0 => 1.35,
        t if t < 120.0 => 1.45,
        _ => 2.0,
    }
}

pub fn cell_chase_at(t: f64) -> f32 {
    if (85.0..95.0).contains(&t) {
        ((t - 85.0) / 5.0).min(1.0) as f32
    } else {
        0.0
    }
}

pub const CELL_EGG_LAY_AT: f64 = 50.0;
/// The mating moment (`cell_game_mate` 1 for 4 s; the stage then goes to
/// the editor).
pub const CELL_MATE_AT: f64 = 130.0;

/// Run the scripted Cell session on an engine with the Cell score running.
pub fn run_cell_session(h: &mut Headless, seconds: f64) {
    let dt = 1.0 / 60.0;
    let mut egg = false;
    let mut last_level = f32::NAN;
    let mut last_chase = 0.0f32;
    let mut last_mate = 0.0f32;
    while h.now < seconds {
        let t = h.now;
        let level = cell_level_at(t);
        let chase = cell_chase_at(t);
        let mate = if (CELL_MATE_AT..CELL_MATE_AT + 4.0).contains(&t) { 1.0 } else { 0.0 };
        if level != last_level {
            h.game(format!("game: cell_level = {level} (level {})", ((level - 1.0 + 0.0001) * 10.0) as i32));
            last_level = level;
        }
        if (chase > 0.0) != (last_chase > 0.0) {
            h.game(format!("game: cell_chase_loop {}", if chase > 0.0 { "rises (mate called)" } else { "back to 0" }));
        }
        if mate != last_mate {
            h.game(format!("game: cell_game_mate = {mate}"));
        }
        last_chase = chase;
        last_mate = mate;
        // 0xE73F60: every frame.
        h.engine.set_global_param("cell_level", level);
        h.engine.set_global_param("cell_chase_loop", chase);
        h.engine.set_global_param("cell_game_mate", mate);
        h.engine.set_global_param("cell_speed", 0.3);
        if !egg && t >= CELL_EGG_LAY_AT {
            egg = true;
            h.game("game: cell_egg_lay = 1 (Cell_hatch_layegg sound2 event)".into());
            h.engine.set_global_param("cell_egg_lay", 1.0);
        }
        h.step(dt);
    }
}

/// One check of the Cell trace.
#[derive(Clone, Debug)]
pub struct Check {
    pub ok: bool,
    pub what: String,
}

fn starts<'a>(ev: &'a [TraceEvent], sound: &'a str) -> impl Iterator<Item = (f64, VoiceId, &'a Vec<(String, String)>)> + 'a {
    ev.iter().filter_map(move |e| match &e.ev {
        Ev::Start { voice, sound: s, params } if s == sound => Some((e.t, *voice, params)),
        _ => None,
    })
}

fn stopped(ev: &[TraceEvent], voice: VoiceId) -> Option<f64> {
    ev.iter().find_map(|e| match e.ev {
        Ev::Stop { voice: v } if v == voice => Some(e.t),
        _ => None,
    })
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Compare a Cell session trace with the retail behaviour.
pub fn check_cell(ev: &[TraceEvent]) -> Vec<Check> {
    let mut out = vec![];
    let mut push = |ok: bool, what: String| out.push(Check { ok, what });
    // 1. One loop per level, at the level's start (the level index is read
    //    on the patch's beat clock: allow 4 s).
    let segments: [(f64, usize); 6] = [(0.0, 0), (20.0, 1), (40.0, 2), (63.0, 2), (80.0, 3), (100.0, 4)];
    let mut loop_voices: Vec<(f64, VoiceId)> = vec![];
    for (t0, level) in segments {
        let name = CELL_LOOPS[level];
        // The last start in the window (at load the master toggle restarts
        // the loop in the same instant, stealing the first voice).
        let hit = starts(ev, name).filter(|(t, _, _)| *t >= t0 - 0.01 && *t <= t0 + 4.0).last();
        push(
            hit.is_some(),
            format!("level {level} at {t0:.0} s plays {name}: {}", hit.map_or("no start".into(), |h| format!("start at {:.2} s", h.0))),
        );
        if let Some((t, v, params)) = hit {
            loop_voices.push((t, v));
            if level > 0 || t0 > 0.0 {
                let fi = param(params, "fadein");
                let fo = param(params, "fadeout");
                push(fi == Some("1") && fo == Some("2"), format!("  {name} fadein {fi:?} / fadeout {fo:?} (retail 1 s in, 2 s out)"));
            }
        }
    }
    // The previous loop stops when the next starts (dplay polyphony 1, steal).
    for w in loop_voices.windows(2) {
        let (t1, v1) = w[1];
        let (_, v0) = w[0];
        let s = stopped(ev, v0);
        push(s.is_some_and(|s| (s - t1).abs() < 0.05), format!("  loop voice {v0} stops when voice {v1} starts at {t1:.2} s ({s:?})"));
    }
    // 2. Level 10: the level loop stops, the beach loop plays.
    let beach = starts(ev, "music_cell_beach_loop").find(|(t, _, _)| *t >= 119.99 && *t <= 124.0);
    push(beach.is_some(), format!("level 10 at 120 s plays music_cell_beach_loop: {:?}", beach.map(|b| b.0)));
    if let Some(&(_, last)) = loop_voices.last() {
        let s = stopped(ev, last);
        push(s.is_some_and(|s| (119.99..124.0).contains(&s)), format!("  the level 4 loop stops at level 10 ({s:?})"));
    }
    // 3. Chase layer while the mate approach parameter is up.
    let chase = starts(ev, "music_cell_chaseloop").find(|(t, _, _)| (84.99..86.5).contains(t));
    push(chase.is_some(), format!("chase loop starts with cell_chase_loop at 85 s: {:?}", chase.map(|c| c.0)));
    if let Some((_, v, params)) = chase {
        let s = stopped(ev, v);
        push(s.is_some_and(|s| (94.99..96.5).contains(&s)), format!("  chase loop stops when it returns to 0 at 95 s ({s:?})"));
        let seek = param(params, "seektime").and_then(|s| s.parse::<f32>().ok());
        push(seek.is_some_and(|s| (2.0..=22.0).contains(&s)), format!("  chase loop random seek 2..22 s ({seek:?})"));
    }
    // 4. Death: explosion and the loop slows to a stop over 2 s.
    let boom = starts(ev, "xplosion_distant").find(|(t, _, _)| (59.99..64.0).contains(t));
    push(boom.is_some(), format!("death at 60 s plays xplosion_distant: {:?}", boom.map(|b| b.0)));
    let level2 = loop_voices.iter().find(|(t, _)| (40.0..44.0).contains(t)).map(|x| x.1);
    let slowed = ev.iter().find_map(|e| match &e.ev {
        Ev::Param { voice, name, value } if Some(*voice) == level2 && name == "pitch" && *value <= 0.05 && e.t >= 60.0 => Some(e.t),
        _ => None,
    });
    push(slowed.is_some_and(|t| t < 67.0), format!("  the level loop's pitch falls to 0 after death ({slowed:?})"));
    // 5. Level switch sound on level changes, not at load or after respawn.
    let switches: Vec<f64> = starts(ev, "sfx_cell_level_switch").map(|s| s.0).collect();
    for t0 in [20.0, 40.0, 80.0, 100.0] {
        push(switches.iter().any(|t| (*t - t0) >= -0.01 && (*t - t0) <= 4.0), format!("level switch sound near {t0:.0} s ({switches:?})"));
    }
    push(!switches.iter().any(|t| *t < 3.0 || (63.0..67.0).contains(t)), "no level switch sound at load or respawn".into());
    // 6. Egg lay sting.
    let egg = starts(ev, "sfx_cell_egglay").find(|(t, _, _)| (CELL_EGG_LAY_AT - 0.01..CELL_EGG_LAY_AT + 1.0).contains(t));
    push(egg.is_some(), format!("egg lay at {CELL_EGG_LAY_AT:.0} s plays sfx_cell_egglay: {:?}", egg.map(|e| e.0)));
    // 7. The patch gain fades the first loop in over 8 s.
    if let Some(&(_, first)) = loop_voices.first() {
        let gains: Vec<(f64, f32)> = ev
            .iter()
            .filter_map(|e| match &e.ev {
                Ev::Param { voice, name, value } if *voice == first && name == "gain" => Some((e.t, *value)),
                _ => None,
            })
            .collect();
        let early = gains.iter().filter(|(t, _)| *t < 1.0).map(|g| g.1).fold(f32::NAN, f32::min);
        let later = gains.iter().filter(|(t, _)| (5.0..12.0).contains(t)).map(|g| g.1).fold(f32::NAN, f32::max);
        push(early < 0.15 && later > 0.4, format!("first loop gain fades in over 8 s (t<1 s: {early:.3}, 5-12 s: {later:.3})"));
    }
    // 8. The beach loop is audible, then the mating moment fades the music
    //    (patch gain B 1 -> 0 over 5 s) and ducks the chase_group mixer.
    if let Some((_, v, _)) = beach {
        let g = |a: f64, b: f64| {
            ev.iter()
                .filter_map(|e| match &e.ev {
                    Ev::Param { voice, name, value } if *voice == v && name == "gain" && e.t >= a && e.t < b => Some(*value),
                    _ => None,
                })
                .fold(f32::NAN, |m, x| if m.is_nan() { x } else { m.max(x) })
        };
        let before = g(124.0, CELL_MATE_AT);
        let after = g(CELL_MATE_AT + 5.5, CELL_MATE_AT + 10.0);
        push(before > 0.5 && !(after > 0.05), format!("beach loop gain {before:.3} before mating, {after:.3} 5 s after"));
    }
    let duck = ev.iter().find_map(|e| match &e.ev {
        Ev::Mixer { group, name, value } if group == "chase_group" && name == "gain" && *value <= 0.001 && e.t >= CELL_MATE_AT => Some(e.t),
        _ => None,
    });
    push(duck.is_some_and(|t| t - CELL_MATE_AT < 1.6), format!("mating moment ducks chase_group to 0 over 1 s ({duck:?})"));
    out
}

/// Print an event log, thinning continuous gain / pitch ramps.
pub fn print_events(ev: &[TraceEvent], names: &HashMap<VoiceId, String>) {
    let mut last: HashMap<(VoiceId, String), (f64, f32)> = HashMap::new();
    for e in ev {
        let line = match &e.ev {
            Ev::Start { voice, sound, params } => {
                let p: Vec<String> = params.iter().map(|(k, v)| format!("{k} {v}")).collect();
                format!("start  v{voice} {sound} [{}]", p.join(", "))
            }
            Ev::Stop { voice } => format!("stop   v{voice} ({})", names.get(voice).map(String::as_str).unwrap_or("?")),
            Ev::Ended { voice } => format!("ended  v{voice} ({})", names.get(voice).map(String::as_str).unwrap_or("?")),
            Ev::Param { voice, name, value } => {
                let key = (*voice, name.clone());
                if matches!(name.as_str(), "gain" | "pitch" | "lowpass" | "hipass") {
                    if let Some((t, v)) = last.get(&key) {
                        let rel = (value - v).abs() / v.abs().max(0.05);
                        if rel < 0.1 && e.t - t < 2.0 {
                            continue;
                        }
                    }
                }
                last.insert(key, (e.t, *value));
                format!("param  v{voice} ({}) {name} {value:.3}", names.get(voice).map(String::as_str).unwrap_or("?"))
            }
            Ev::Mixer { group, name, value } => {
                let key = (u32::MAX, format!("{group}/{name}"));
                if let Some((t, v)) = last.get(&key) {
                    let rel = (value - v).abs() / v.abs().max(0.05);
                    if rel < 0.1 && e.t - t < 2.0 && *value != 0.0 {
                        continue;
                    }
                }
                last.insert(key, (e.t, *value));
                format!("mixer  {group} {name} {value:.3}")
            }
            Ev::SendGame { name, value } => format!("sendgame {name} {value}"),
            Ev::Print(t) => format!("print  {t}"),
            Ev::Game(t) => format!("---- {t}"),
        };
        println!("{:8.3}  {line}", e.t);
    }
}

/// `--pd-trace <patch> [--seconds N]`.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(i) = args.iter().position(|a| a == "--pd-trace") else { return 2 };
    let Some(patch) = args.get(i + 1).cloned() else {
        eprintln!("--pd-trace requires a patch sound or bundle name");
        return 2;
    };
    let seconds: f64 = args.iter().position(|a| a == "--seconds").and_then(|j| args.get(j + 1)).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let install = match crate::spore_install::discover() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let lib = Arc::new(PdLibrary::from_index(&index));
    let audio = Arc::new(AudioLibrary::from_index(&index));
    let mut h = Headless::new(lib.clone(), audio, 1_489_853_723);
    h.engine.trace_play = args.iter().any(|a| a == "--pd-plays");
    h.engine.trace_sends = args.iter().position(|a| a == "--pd-sends").and_then(|j| args.get(j + 1)).cloned();
    // `--pd-param name=value[@from-to]` (repeatable): set every frame of a
    // generic run (inside the window; 0 outside it).
    // `a~b` ramps linearly over the window. `@from-to+` sets inside the
    // window only (the value is kept afterwards, as when the game stops
    // setting it); `@t` sets it once at `t` (an event such as `flute_start`).
    // A name `i:<param>` sets the instance parameter (`fparam`) instead.
    let mut fixed: Vec<(String, f32, f32, f64, f64, u8)> = vec![];
    for (j, a) in args.iter().enumerate() {
        if a == "--pd-param" {
            if let Some((k, v)) = args.get(j + 1).and_then(|s| s.split_once('=')) {
                let (v, win) = v.split_once('@').map_or((v, None), |(v, w)| (v, Some(w)));
                let hold = win.is_some_and(|w| w.ends_with('+'));
                let win = win.map(|w| w.trim_end_matches('+'));
                let once = win.filter(|w| !w.contains('-')).and_then(|w| w.parse::<f64>().ok());
                let (from, to) = win
                    .and_then(|w| w.split_once('-'))
                    .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
                    .or(once.map(|t| (t, t)))
                    .unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                let (a, b) = v.split_once('~').map_or((v, v), |(a, b)| (a, b));
                if let (Ok(a), Ok(b)) = (a.parse::<f32>(), b.parse::<f32>()) {
                    let mode = if once.is_some() { 2 } else if hold { 1 } else { 0 };
                    fixed.push((k.to_string(), a, b, from, to, mode));
                }
            }
        }
    }
    let inst = match h.start(&patch) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let cell = matches!(patch.to_lowercase().as_str(), "cellgame_seed_music6" | "cellgame_seed_music");
    if cell {
        run_cell_session(&mut h, if seconds > 0.0 { seconds } else { CELL_MATE_AT + 12.0 });
    } else {
        let until = if seconds > 0.0 { seconds } else { 60.0 };
        for (k, v0, v1, a, b, mode) in &fixed {
            match mode {
                2 => h.game(format!("game: {k} = {v0} once at {a} s")),
                _ => h.game(format!("game: {k} = {v0}..{v1} every frame in {a}..{b} s")),
            }
        }
        let mut fired: Vec<bool> = vec![false; fixed.len()];
        // `--city-anthem <seed>`: the Civ city music start (0xEAD6C0 /
        // 0xEAD170) with the random anthem of that seed (`civ_music`), then
        // AddCity's `city_music_melody_play 0`.
        if let Some(seed) = args.iter().position(|a| a == "--city-anthem").and_then(|j| args.get(j + 1)).and_then(|s| s.parse::<u64>().ok()) {
            h.step(1.0 / 60.0);
            let anthem = crate::civ_music::random_anthem(&crate::civ_music::AnthemSetup::from_index(&index), seed);
            h.game(format!("game: anthem {anthem:?}"));
            h.engine.set_global_param("mixmode_cityplanner", 0.0);
            for (name, v) in crate::civ_music::start_sequence(&anthem) {
                h.engine.set_global_param(name, v);
            }
            h.engine.set_global_param(crate::civ_music::MELODY_PLAY, 0.0);
        }
        while h.now < until {
            for (i, (k, v0, v1, a, b, mode)) in fixed.iter().enumerate() {
                if *mode == 2 {
                    if !fired[i] && h.now >= *a {
                        fired[i] = true;
                        match k.strip_prefix("i:") {
                            Some(name) => h.engine.set_instance_param(inst, name, *v0),
                            None => h.engine.set_global_param(k, *v0),
                        }
                    }
                    continue;
                }
                let inside = h.now >= *a && h.now < *b;
                if inside || (a.is_finite() && *mode == 0) {
                    let f = if b.is_finite() && a.is_finite() { ((h.now - a) / (b - a)) as f32 } else { 0.0 };
                    let v = if inside { v0 + (v1 - v0) * f } else { 0.0 };
                    match k.strip_prefix("i:") {
                        Some(name) => h.engine.set_instance_param(inst, name, v),
                        None => h.engine.set_global_param(k, v),
                    }
                }
            }
            h.step(1.0 / 60.0);
        }
    }
    let mut names = HashMap::new();
    for e in &h.events {
        if let Ev::Start { voice, sound, .. } = &e.ev {
            names.insert(*voice, sound.clone());
        }
    }
    println!("== pd trace: {patch} (instance {inst}), {:.0} s", h.now);
    print_events(&h.events, &names);
    if !h.engine.errors.is_empty() {
        println!("== engine messages ({})", h.engine.errors.len());
        for e in h.engine.errors.iter().take(40) {
            println!("  {e}");
        }
    }
    if cell {
        let checks = check_cell(&h.events);
        let failed = checks.iter().filter(|c| !c.ok).count();
        println!("== Cell music vs retail behaviour (notes/cell-audio-notes.md 3): {} checks, {} failed", checks.len(), failed);
        for c in &checks {
            println!("  [{}] {}", if c.ok { "PASS" } else { "FAIL" }, c.what);
        }
        return if failed == 0 { 0 } else { 1 };
    }
    0
}
