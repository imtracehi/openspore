//! Cell-stage audio, wired the way the retail game does it.
//!
//! NATIVE (installed data + `notes/cell-re-notes.md` section 14 and the audio
//! analysis in `CHANGES.md`):
//! * Animation events. The Cell animations carry their sounds as `sound`
//!   events (type 0, id = FNV-1 of the soundProp name) at key frames; the clip
//!   that plays on a cell fires them as it passes the frame. `sound2` events
//!   (type 6) set a named game audio parameter (`cell_egg_lay`). Names that are
//!   not installed are silent in the retail install too. Event names that start
//!   with `v` (`vcl_*` creature voices, `vfx_*`) are voices: they play as
//!   `<event>_<species code>` of the creature's voice id (`cell_voices`, helper
//!   `cell-voices`; the 255 installed `vcl_*` sounds).
//! * Effect sounds. Swarm effects embed `SoundEffect` components; every effect
//!   instance that starts plays its sounds (loops live as long as the effect).
//! * Direct calls: `ui_mission_complete` on a part found / mission completed,
//!   `cell_collision_<a>_<b>_impact` for the player's contacts (class pair from
//!   the cell record's `sound` / cell type, `minImpactCollisionSpeed` 5).
//! * The three Pure Data patches started at stage entry: `cell_motion`
//!   (`cell_speed` -> loop gain / pitch curves), `cell_amb` (`cell_level` ->
//!   four mixed ambience loops), `cellgame_seed_music6` (one loop per level
//!   from its `coll` table, 2 s / 1 s crossfade, patch gain envelope, beach
//!   loop at level 10, death slow-down, level switch / death / egg-lay
//!   sounds, mate-approach chase loop). The `function` objects of the patches
//!   are the curve tables below (x and y normalised to 0..1, y scaled by the
//!   object's maximum); the game audio parameters (`cell_level`,
//!   `cell_speed`, ...) are computed every frame as 0xE73F60 does.
//!
//! * Acoustics (`cell_acoustics`, `cell_spatial`, helpers `cell-audio3` / `cell-audio4`, VERIFIED in the
//!   retail track update 0xA1C9B0 and the voice's 3D update 0xA12740): a positional sound (one-shot
//!   or loop, clip event, collision or effect component) is a voice that is re-shaped every frame
//!   from its source's current position and the listener: roll-off gain, the soundProp's
//!   `atmospheric` low-pass and distance high-pass, the Pan2D1 stereo gains, the `wetlevel` send
//!   into the 2.5 s / 30 m reverb. The listener is the camera look point during a cinematic, else
//!   the avatar (0xE73F60). The `cell_vo_submix` bus the Cell sounds send to has no effect
//!   configured in retail (the started patch `mix_event_cell_vo_small` only touches
//!   `cell_vo_small_submix`), so nothing is applied for the send.
//!
//! The Pd filters (`lowpass`, `hipass`, `pan`, `ringmod*`) of the patch voices are applied by the
//! Pd engine (`pd::voice`) on the default path; the static filters below serve the hand-ported
//! score of `OPENSPORE_PD_MUSIC=0`.
//!
//! * The mate-intensity music input (`cell_chase_loop`) belongs to the escape mission ([`GoalTarget`],
//!   helper `cell-audio5`), not to the called mate: see that type.
//!
//! ADAPTED: the roughly 10 ms control smoothing of the positional voices (`cell_acoustics` /
//! `cell_spatial` headers; the biquad design and the whole ReverbModel1 - comb lengths, feedback
//! tables, loop gains, comb and all-pass kernels, output level - are VERIFIED now); no Doppler (VERIFIED: no installed
//! soundProp carries the track's Doppler curve 0x9BFC37AC, so nothing drives it); the sounds that are
//! not anchored (UI, Pd patch voices, other stages) are flat; the chase loop of the legacy score
//! starts at its beginning instead
//! of a random seek, the chase singles and `chase_vo` are not played. The Pd mixers (`chase_group`,
//! `sfx_group`, `ui_group`, ...) scale every named one-shot / loop of their groups through
//! `SoundDef::groups` when the Pd engine runs (helper `cell-audit`); without the engine only the
//! collision sounds follow the hand-ported `chase_group` ([`ChaseGroup`]).
//!
//! Set `OPENSPORE_CELL_AUDIO_DEBUG=1` to print every sound the module starts.

use std::{
    collections::{HashMap, HashSet},
    sync::{mpsc, Arc, Mutex, OnceLock},
};

use bevy::{
    audio::{
        AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings,
        Volume,
    },
    prelude::*,
};
use rand::Rng;

use crate::{
    cell_acoustics::{self, Acoustics},
    cell_spatial::Anchor,
    cell_anim::CellAnimPlayback,
    cell_contract::CellAnimState,
    cell_data::{CellDatabase, CellSound, CellType},
    cell_engine::{CellGame, CellObject, UNIT},
    cell_flow::{CellFlow, FlowPhase},
    cell_rules::CellContacts,
    game::{Player, RunProgress},
    game_audio::{AudioLibrary, Bus, ClipData, CueSpec, GameAudio, SoundDef},
    stages::{GameStage, StageOwned},
    world_objects::name_id,
    Originals,
};
#[cfg(test)]
use crate::swarm_fx::{
    format::{kind, sound_flag, IMPORT_MASK},
    EffectRef, SwarmLibrary,
};

// ---------------------------------------------------------------------------
// Original data: patch curves and tables
// ---------------------------------------------------------------------------

/// A `function` object of a Pd patch: the first point is `(0, y0)`, the
/// others `(x, y)`; everything normalised to 0..1, the result is scaled by
/// `ymax` (every installed Cell function has `ymin` 0).
#[derive(Clone, Copy, Debug)]
pub struct Curve {
    pub ymax: f32,
    pub y0: f32,
    pub pts: &'static [(f32, f32)],
}

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let (mut px, mut py) = (0.0f32, self.y0);
        for &(qx, qy) in self.pts {
            if x <= qx {
                let t = if qx > px { (x - px) / (qx - px) } else { 1.0 };
                return (py + (qy - py) * t) * self.ymax;
            }
            px = qx;
            py = qy;
        }
        py * self.ymax
    }
}

// cell_motion: input `cell_speed` (0..1, smoothed 250 ms).
pub const MOTION_GAIN: Curve = Curve {
    ymax: 1.0,
    y0: 0.0,
    pts: &[
        (0.0263158, 0.244444),
        (0.105263, 0.533333),
        (0.242105, 0.733333),
        (0.447368, 0.888889),
        (0.836841, 1.0),
        (1.0, 1.0),
    ],
};
pub const MOTION_PITCH: Curve = Curve {
    ymax: 1.1,
    y0: 0.955555,
    pts: &[
        (0.0842105, 0.844444),
        (0.131579, 0.833333),
        (0.194737, 0.822222),
        (0.310526, 0.877778),
        (0.468421, 0.944445),
        (0.7, 1.0),
        (1.0, 1.0),
    ],
};
pub const FLOW_GAIN: Curve = Curve {
    ymax: 0.8,
    y0: 0.0111111,
    pts: &[
        (0.0368421, 0.288889),
        (0.068421, 0.455556),
        (0.147368, 0.533333),
        (0.3, 0.622222),
        (0.373684, 0.722222),
        (0.5, 0.944444),
        (0.573684, 1.0),
        (1.0, 1.0),
    ],
};
// cell_amb: input x = clip(cell_level + 0.01 - 1, 0, 1) (smoothed 800 ms).
pub const AMB_NEW_GAIN: Curve = Curve {
    ymax: 1.0,
    y0: 1.0,
    pts: &[(0.321052, 1.0), (0.468421, 0.933333), (0.6, 0.811111), (0.726316, 0.611111), (1.0, 0.0)],
};
pub const AMB_NEW_PITCH: Curve = Curve {
    ymax: 2.0,
    y0: 0.366666,
    pts: &[(0.436842, 0.455556), (0.694737, 0.488889), (1.0, 0.5)],
};
pub const AMB_WAVES_GAIN: Curve = Curve {
    ymax: 2.0,
    y0: 0.0,
    pts: &[(0.252632, 0.0), (0.836842, 0.244444), (1.0, 0.299998)],
};
pub const AMB_WAVES_PITCH: Curve = Curve {
    ymax: 2.0,
    y0: 0.344444,
    pts: &[(1.0, 0.477777)],
};
pub const AMB_LIFE_GAIN: Curve = Curve {
    ymax: 1.2,
    y0: 0.0,
    pts: &[(0.378947, 0.155556), (0.557895, 0.211111), (0.789474, 0.211111), (1.0, 0.122223)],
};
pub const AMB_LIFE_PITCH: Curve = Curve {
    ymax: 2.0,
    y0: 0.11111,
    pts: &[
        (0.147369, 0.166666),
        (0.294737, 0.222222),
        (0.626316, 0.411111),
        (0.773684, 0.455556),
        (1.0, 0.499999),
    ],
};
pub const AMB_OCEAN_GAIN: Curve = Curve {
    ymax: 3.0,
    y0: 0.155554,
    pts: &[(0.6, 0.0), (0.868421, 0.0), (1.0, 0.0)],
};
// cellgame_seed_music6 / chase: gain of `music_cell_chaseloop` by the chase
// parameter.
pub const CHASE_GAIN: Curve = Curve {
    ymax: 1.0,
    y0: 0.444444,
    pts: &[(0.421053, 0.544445), (0.752632, 0.733334), (1.0, 0.844444)],
};

/// Patch master gains (the soundProps `cell_motion` 0.8, `cell_amb` 0.65,
/// `cellgame_seed_music` 0.7).
pub const MOTION_PATCH_GAIN: f32 = 0.8;
pub const AMB_PATCH_GAIN: f32 = 0.65;
pub const MUSIC_PATCH_GAIN: f32 = 0.7;

/// `coll 11` of `main_loops`: the loop of each level 0..=9 (level 10 stops the
/// level loop; the beach loop takes over).
pub const LEVEL_LOOPS: [&str; 10] = [
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

/// `coll 11` of the egg lay patch: 1 plays `sfx_cell_egglay`, 2 the minor
/// variant (levels 4..=6, the loops in the minor key).
pub fn egg_lay_class(level: i32) -> u8 {
    match level {
        4..=6 => 2,
        _ => 1,
    }
}

/// Level index of the music patch (`cell_level_convert_to_int`): the integer
/// part of `(cell_level - 1 + 0.0001) * 10` with `cell_level = 1 + food /
/// 1000` and 0 while the avatar is dying, so -9 means dead, 10 means the
/// stage is complete.
pub fn level_int(food: i32, dying: bool) -> i32 {
    let cell_level = if dying { 0.0f32 } else { 1.0 + food.clamp(0, 1000) as f32 / 1000.0 };
    ((cell_level - 1.0 + 0.0001) * 10.0) as i32
}

/// Pd `line`: a value that moves linearly to a target in a given time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line {
    pub value: f32,
    target: f32,
    rate: f32,
    remaining: f32,
}

impl Line {
    pub fn at(value: f32) -> Self {
        Self { value, target: value, rate: 0.0, remaining: 0.0 }
    }
    pub fn set(&mut self, value: f32) {
        *self = Self::at(value);
    }
    pub fn go(&mut self, target: f32, seconds: f32) {
        if seconds <= 0.0 {
            self.set(target);
        } else {
            self.target = target;
            self.remaining = seconds;
            self.rate = (target - self.value) / seconds;
        }
    }
    pub fn step(&mut self, dt: f32) {
        if self.remaining > 0.0 {
            let s = dt.min(self.remaining);
            self.value += self.rate * s;
            self.remaining -= s;
            if self.remaining <= 0.0 {
                self.value = self.target;
            }
        }
    }
}

/// The `chase_group` mixer gain of `cellgame_seed_music6` (the `chase` and
/// `mating_moment` subpatches; VERIFIED by the headless run of the retail
/// patch, `--pd-trace cellgame_seed_music6`): 1 at load; the chase parameter
/// (`cell_chase_loop` > 0) going up ramps it to 0 over 1 s, going down sets it
/// to 0 and ramps it to 1 over 3 s; the mating moment (`cell_game_mate`)
/// ramps it to 0 over 1 s and, when it ends, to 1 over 3 s. Every sound that
/// belongs to the group (the Cell's `_cell_vfx_*`, `_cell_vox_*` and
/// `_cell_col_base` descendants) is scaled by it. With the Pd engine on the
/// engine's own mixer value is used ([`crate::pd_audio::PdMusic::group_gain`]);
/// this model serves the hand-ported fallback (`OPENSPORE_PD_MUSIC=0`).
#[derive(Clone, Copy, Debug)]
pub struct ChaseGroup {
    chasing: bool,
    mating: bool,
    line: Line,
}

impl Default for ChaseGroup {
    fn default() -> Self {
        Self { chasing: false, mating: false, line: Line::at(1.0) }
    }
}

impl ChaseGroup {
    pub fn value(&self) -> f32 {
        self.line.value.clamp(0.0, 1.0)
    }

    /// One frame: the chase parameter is up, the mating moment is on.
    pub fn update(&mut self, chasing: bool, mating: bool, dt: f32) {
        if chasing != self.chasing {
            self.chasing = chasing;
            if chasing {
                self.line.go(0.0, 1.0);
            } else {
                self.line.set(0.0);
                self.line.go(1.0, 3.0);
            }
        }
        if mating != self.mating {
            self.mating = mating;
            self.line.go(if mating { 0.0 } else { 1.0 }, if mating { 1.0 } else { 3.0 });
        }
        self.line.step(dt);
    }
}

/// `mate intensity` (0xE5BE80), the value behind the `cell_chase_loop`
/// parameter while the mate mission is shown: the smaller of a distance
/// term (1 up to distance 1, 0.75 at 8, 0.1 from 25) and the ramp
/// `clamp(timer / 5, 0, 1)` of the goal timer (+0x51B8, [`GoalTarget::timer`]).
pub fn mate_intensity(distance: f32, timer: f32) -> f32 {
    let remap = |a: f32, b: f32, x: f32, y0: f32, y1: f32| {
        let t = if (b - a).abs() > f32::EPSILON { ((x - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        y0 + t * (y1 - y0)
    };
    let near = if distance > 8.0 {
        remap(8.0, 25.0, distance, 0.75, 0.1)
    } else {
        remap(1.0, 8.0, distance, 1.0, 0.75)
    };
    near.min(remap(5.0, 0.0, timer, 1.0, 0.0))
}

/// The `cell_collision_<a>_<b>_impact` sound of a contact between two cell
/// classes (1 creature, 2 plant, 3 rock, 4 bubble: bubble pairs are silent).
pub fn collision_sound(class_a: u8, class_b: u8) -> Option<&'static str> {
    let (a, b) = (class_a.min(class_b), class_a.max(class_b));
    Some(match (a, b) {
        (1, 1) => "cell_collision_creature_creature_impact",
        (1, 2) => "cell_collision_creature_plant_impact",
        (1, 3) => "cell_collision_creature_rock_impact",
        (2, 2) => "cell_collision_plant_plant_impact",
        (2, 3) => "cell_collision_plant_rock_impact",
        (3, 3) => "cell_collision_rock_rock_impact",
        _ => return None,
    })
}

/// Collision class of a cell record (0xE64A00): the record's `sound` when
/// set, else by cell type (rock 3, solid 1, air 4, anything else 2).
pub fn collision_class(sound: CellSound, cell_type: CellType) -> u8 {
    match sound {
        CellSound::Creature => 1,
        CellSound::Plant => 2,
        CellSound::Rock => 3,
        CellSound::Bubble => 4,
        _ => match cell_type {
            CellType::Rock => 3,
            CellType::Solid => 1,
            CellType::Air => 4,
            _ => 2,
        },
    }
}

/// Whether an animation event at clip time `time` is passed when the clip
/// moves from `from` to `to` (seconds of unwrapped play time; cyclic clips
/// repeat their events every `length`).
pub fn event_due(from: f32, to: f32, length: f32, cyclic: bool, time: f32) -> bool {
    if !cyclic || length <= 0.0 {
        return time > from && time <= to;
    }
    let k = ((from - time) / length).floor() + 1.0;
    let at = time + k.max(0.0) * length;
    at > from && at <= to
}

/// `minImpactCollisionSpeed` (Globals, installed value 5).
pub const MIN_IMPACT_SPEED: f32 = 5.0;
/// `ui_mission_complete` (RE 14: part found, tutorial mission complete).
const MISSION_COMPLETE: &str = "ui_mission_complete";

/// Sounds the stage plays all the time, decoded at stage entry so the first
/// event is not late (most common first).
const PREFETCH: &[&str] = &[
    "sfx_cell_eat",
    "sfx_cell_dmgsolid_mand",
    "sfx_cell_squirt",
    "sfx_cell_avatar_die",
    "sfx_cell_avatar_die_pop",
    "sfx_cell_chum",
    "sfx_cell_hatch",
    "sfx_cell_eggpop",
    "sfx_cell_egg_hatch",
    "sfx_cell_eventbubbles_low",
    "sfx_cell_avatar_takedmg",
    "sfx_cell_youlose",
    "sfx_cell_youlose_avatar",
    "sfx_cell_takedmg_stab",
    "sfx_cell_grow_small",
    "sfx_cell_explosion",
    "sfx_cell_solidcrumble",
    "sfx_cell_canteat",
    "sfx_cell_dmgsolid_spike",
    "sfx_cell_npcsquirt",
    "cell_eat_npc",
    "sfx_cell_mand_big",
    "sfx_cell_mand_big_far",
    "sfx_cell_damage_electro",
    "sfx_cell_glance_mand",
    "sfx_cell_glance_spike",
    "sfx_cell_spikeonspike",
    "sfx_cell_puncture_impact",
    "sfx_cell_water_splash",
    "sfx_cell_part_pickup",
    "sfx_cell_chemical_drop",
    "sfx_cell_mate_call_a",
    "sfx_cell_mate_call_b",
    "sfx_cell_mate_call_start",
    "sfx_cell_mate_response_start",
    "sfx_cell_level_switch",
    "xplosion_distant",
    "bubblepop",
    "ui_mission_complete",
    "cell_collision_creature_creature_impact",
    "cell_collision_creature_plant_impact",
    "cell_collision_creature_rock_impact",
    "cell_collision_plant_plant_impact",
    "cell_collision_plant_rock_impact",
    "cell_collision_rock_rock_impact",
];

// ---------------------------------------------------------------------------
// The goal target: the mate-intensity music input (helper `cell-audio5`)
// ---------------------------------------------------------------------------

/// `cell_chase_loop` (0x4CD9E37F) is fed by 0xE73F60 as `goal state == 3 ? mate intensity : 0`
/// (0xE74377..0xE74392). The state (`cCellGame+0x51B4`), the target cell (+0x51B0), the timer
/// (+0x51B8), the cooldown (+0x51BC) and the "already triggered" flag (+0x51C0) belong to the
/// **escape mission**, not to the mating call (VERIFIED, march2017):
/// * The level init (0xE80BA0, 0xE81044..0xE81089) clears the target, sets the state to -1, the timer
///   to 0, the cooldown to the goal duration (0xE52B50) and the flag to 0.
/// * The proximity scan 0xE5D5B0 (every frame): `cooldown -= dt`; while the target lives the cooldown is
///   held at the duration; otherwise, when the gate 0xE53260 passes (difficulty +0x7C of the serialisable
///   data != 0, `+0x51DC` clear = [`CellGame::escape`], the avatar's scale level 0xE4EE40(food) >= 3,
///   not (already triggered and difficulty 1), cooldown <= 0), the first cell around the visible box
///   centre (radius `escapeMaxDistance`, globals +0xF8) that is inside the view (0xE5CD00), within
///   `escapeMinDistance` (+0xF4) of the avatar and passes 0xE5D580 (a model, scale difference 3 = one
///   class larger than the avatar, the record's `triggersEscapeMission` byte +0x318) becomes the
///   target: flag set, state 3, timer = `missionTime` (+0xEC).
/// * The updater 0xE5FF70 (every frame, state != -1): the mission ends (cooldown = duration, state -1,
///   target 0, 0xE53440) when the target is gone or marked (+0x112 = 1), the avatar is gone or the
///   scale difference is no longer 3; while the target is outside the view the timer runs down by dt
///   and ends the mission at 0, while it is inside the view the timer is raised to `missionResetTime`
///   (+0xF0, 0xE50040).
/// * The mating start 0xE75EA0 and the mission-complete branch 0xE6E9B0 end an active mission the
///   same way (state -1, target 0, cooldown = duration); the special goal cell 0xE81120 of the tutorial
///   sets state 3 with a timer of 15 s (not modelled).
/// * Mate intensity 0xE5BE80: `min(distance term, ramp)` with the distance term 1 up to 1 unit, 0.75
///   at 8, 0.1 from 25 and the ramp = clamp(timer / 5, 0, 1) (the last five seconds fade it out):
///   [`mate_intensity`].
///
/// The earlier OpenSpore port took the called mate for the goal target; retail's mating start clears
/// it instead (0xE75F56..0xE75FA6 write state -1 and target 0), so the mate called by the player
/// never drives the layer.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GoalTarget {
    /// The target while the state is 3 (the escape mission is on).
    pub target: Option<Entity>,
    /// `+0x51B8`: seconds left (the mission fades out over its last five).
    pub timer: f32,
    /// `+0x51BC`: seconds until the next mission may start.
    pub cooldown: f32,
    /// `+0x51C0`: a mission has started in this level.
    pub triggered: bool,
    started: bool,
}

/// 0xE52B50: how long a finished or running mission blocks the next one, by difficulty (1 medium:
/// `escapeDelayMedium` +0xFC, 2 hard: `escapeTimerHard` +0x100, else 20 s).
pub fn goal_duration(difficulty: usize, escape_delay_medium: f32, escape_timer_hard: f32) -> f32 {
    match difficulty {
        1 => escape_delay_medium,
        2 => escape_timer_hard,
        _ => 20.0,
    }
}

/// The gate of the proximity scan (0xE53260).
pub fn goal_gate(difficulty: usize, escape: bool, food: i32, triggered: bool, cooldown: f32) -> bool {
    difficulty != 0
        && !escape
        && crate::cell_engine::player_scale(food).index() >= 3
        && !(triggered && difficulty == 1)
        && cooldown <= 0.0
}

/// 0xE5CD00: the cell is outside the visible background box widened by `size * 1.7 * 0.5` (distance
/// of its position to the box, in the play plane). `half` is the box's half extent around `centre`.
/// ADAPTED: `size` is the cell's world radius (retail +0x58, the cell's size field).
pub fn outside_view(position: Vec2, size: f32, centre: Vec2, half: Vec2) -> bool {
    let out = (position - centre).abs() - half;
    let dx = out.x.max(0.0);
    let dy = out.y.max(0.0);
    let reach = size * 1.7 * 0.5;
    dx * dx + dy * dy > reach * reach
}

/// What the goal target machine reads about a cell.
#[derive(Clone, Copy, Debug)]
pub struct GoalCell {
    /// `GetScaleDifferenceWithPlayer` (0xE57340 / 0xE573E0).
    pub scale_diff: usize,
    /// The cell is marked (+0x112 == 1) or being removed.
    pub marked: bool,
    pub outside_view: bool,
}

impl GoalTarget {
    /// The level init (0xE80BA0).
    pub fn reset(&mut self, duration: f32) {
        *self = Self { cooldown: duration, started: true, ..default() };
    }

    /// 0xE53440(3) and the state / target writes: the mission is over.
    pub fn end(&mut self, duration: f32) {
        self.cooldown = duration;
        self.target = None;
    }

    /// 0xE5FF70 for the current `target` (`cell` is `None` when it no longer exists); `avatar` says
    /// whether the avatar exists.
    pub fn update(&mut self, dt: f32, duration: f32, reset_time: f32, avatar: bool, cell: Option<GoalCell>) {
        if self.target.is_none() {
            return;
        }
        let Some(cell) = cell.filter(|c| avatar && !c.marked && c.scale_diff == 3) else {
            self.end(duration);
            return;
        };
        if cell.outside_view {
            self.timer -= dt;
            if self.timer <= 0.0 {
                self.end(duration);
            }
        } else {
            self.timer = self.timer.max(reset_time);
        }
    }

    /// 0xE5D5B0 up to the candidate choice: the cooldown runs, a living target holds it at the
    /// duration. Returns whether the scan should look for a candidate (the gate passes).
    pub fn scan_ready(&mut self, dt: f32, duration: f32, target_alive: bool, gate: impl FnOnce(bool, f32) -> bool) -> bool {
        self.cooldown -= dt;
        if target_alive {
            self.cooldown = duration;
            return false;
        }
        gate(self.triggered, self.cooldown)
    }

    /// 0xE5D5B0 once a candidate passed: the mission starts.
    pub fn start(&mut self, cell: Entity, mission_time: f32) {
        self.triggered = true;
        self.target = Some(cell);
        self.timer = mission_time;
    }
}

/// The escape mission every frame (0xE5D5B0 scan, 0xE5FF70 updater, the mating start's clear).
#[allow(clippy::too_many_arguments)]
fn update_goal_target(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    flow: Res<CellFlow>,
    game: Res<CellGame>,
    db: Res<CellDatabase>,
    mut goal: ResMut<GoalTarget>,
    players: Query<(&CellObject, &Transform), With<Player>>,
    cells: Query<(Entity, &CellObject, &Transform), Without<Player>>,
) {
    let Some(globals) = db.globals.as_ref() else { return };
    let duration = goal_duration(game.difficulty, globals.escape_delay_medium, globals.escape_timer_hard);
    let Ok((avatar, avatar_t)) = players.single() else {
        // No avatar (between levels / lives): the level init state.
        if goal.started || goal.target.is_some() {
            *goal = GoalTarget::default();
        }
        return;
    };
    if !goal.started {
        goal.reset(duration);
    }
    let dt = time.delta_secs();
    let centre = game.camera_look;
    let half = crate::cell_engine::VISIBLE_BOX_HALF * UNIT;
    let player_scale = crate::cell_engine::player_scale(run.cell.food_progression);
    let here = avatar.plane(avatar_t);
    let view = |object: &CellObject, t: &Transform| GoalCell {
        scale_diff: crate::cell_engine::scale_diff(object.scale, player_scale),
        marked: object.passive || object.dead,
        outside_view: outside_view(object.plane(t), object.radius, centre, half),
    };
    let before = goal.target;
    // The mating start ends a running mission (0xE75F56..0xE75FA6).
    if flow.phase == FlowPhase::Mating && goal.target.is_some() {
        goal.end(duration);
    }
    // 0xE5FF70.
    let current = goal.target.and_then(|e| cells.get(e).ok()).map(|(_, o, t)| view(o, t));
    goal.update(dt, duration, globals.mission_reset_time, true, current);
    if before.is_some() && goal.target.is_none() && debug_enabled() {
        eprintln!("cell audio: escape mission ends (cooldown {:.1} s)", goal.cooldown);
    }
    // 0xE5D5B0.
    let alive = goal.target.is_some_and(|e| cells.contains(e));
    let ready = goal.scan_ready(dt, duration, alive, |triggered, cooldown| {
        goal_gate(game.difficulty, game.escape, run.cell.food_progression, triggered, cooldown)
    });
    if !ready {
        return;
    }
    let max = globals.escape_max_distance * UNIT;
    let min = globals.escape_min_distance * UNIT;
    let found = cells.iter().find(|(_, object, t)| {
        if object.background || object.dead {
            return false;
        }
        let there = object.plane(t);
        if there.distance(centre) > max || outside_view(there, object.radius, centre, half) || there.distance(here) > min {
            return false;
        }
        // 0xE5D580: scale difference 3 and the record's `triggersEscapeMission`.
        crate::cell_engine::scale_diff(object.scale, player_scale) == 3
            && db.cell(object.cell).is_some_and(|r| r.triggers_escape_mission)
    });
    if let Some((entity, object, _)) = found {
        goal.start(entity, globals.mission_time);
        if debug_enabled() {
            eprintln!("cell audio: escape mission starts: {:08X} (scale {:?}), timer {:.1} s", object.cell, object.scale, goal.timer);
        }
    }
}

// ---------------------------------------------------------------------------
// Game audio parameters (0xE73F60, every frame)
// ---------------------------------------------------------------------------

/// The values the retail Cell game feeds the audio system. Names are the
/// `gfparam` names of the installed patches (hash ids in the executable).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CellAudioParams {
    /// `cell_level` (0xF83DAE6E): 1 + progress, 0 while the avatar dies.
    pub cell_level: f32,
    /// `cell_health` (0xB0B3CAF6): hp / 6.
    pub cell_health: f32,
    /// `flow_field` (0x07632190): clamp(0.1 * current strength, 0, 1).
    pub flow_field: f32,
    /// `cell_chase` (0xA807AC16): an NPC chases the avatar (no installed
    /// patch listens to it).
    pub cell_chase: f32,
    /// `cell_speed` (0xE6A63E05): mean of the last four clamp(|v| / 8, 0, 1).
    pub cell_speed: f32,
    /// `cell_chase_loop` (0x4CD9E37F): mate intensity while the mate mission
    /// shows.
    pub cell_chase_loop: f32,
    /// `cell_game_mate` (0x01E08F6A): set during the mating sequence.
    pub cell_game_mate: bool,
    /// `cell_game_evolve` (0x8A4D210E): set during the ending cinematic.
    pub cell_game_evolve: bool,
    /// `cell_size` (0xF03E3DDF): motion track parameter.
    pub cell_size: f32,
    /// Level index derived from `cell_level` (-9 dead, 10 complete).
    pub level: i32,
    /// Parameters set by `sound2` animation events since the last frame
    /// (name hash, value).
    pub triggers: Vec<(u32, f32)>,
}

/// The last four `clamp(|v| / 8, 0, 1)` speed samples (ring at cCellGFX+0x161F0).
#[derive(Default)]
struct SpeedRing {
    samples: [f32; 4],
    next: usize,
}

impl SpeedRing {
    fn push(&mut self, sample: f32) -> f32 {
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % 4;
        self.samples.iter().sum::<f32>() * 0.25
    }
}

#[allow(clippy::too_many_arguments)]
fn compute_params(
    run: Res<RunProgress>,
    flow: Res<CellFlow>,
    game: Res<CellGame>,
    mut params: ResMut<CellAudioParams>,
    players: Query<(Entity, &CellObject, &Transform), With<Player>>,
    cells: Query<&Transform>,
    ai: Query<&crate::cell_ai::CellAiState>,
    goal: Res<GoalTarget>,
    mut ring: Local<SpeedRing>,
) {
    let food = run.cell.food_progression;
    let dying = flow.phase == FlowPhase::Dying;
    let triggers = std::mem::take(&mut params.triggers);
    let mut next = CellAudioParams {
        cell_level: if dying { 0.0 } else { 1.0 + food.clamp(0, 1000) as f32 / 1000.0 },
        cell_game_mate: flow.phase == FlowPhase::Mating,
        cell_game_evolve: matches!(flow.phase, FlowPhase::Ending),
        level: level_int(food, dying),
        triggers,
        ..default()
    };
    if let Ok((player, object, transform)) = players.single() {
        let speed = (object.velocity.length() / UNIT * 0.125).clamp(0.0, 1.0);
        next.cell_speed = ring.push(speed);
        next.cell_health = object.hp as f32 / 6.0;
        // `flow_field` (0xE73F60 -> 0xE4E570 at the avatar's position, VERIFIED):
        // clamp(0.1 * the sum of the mean fluid speeds of the four grid cells
        // around the avatar, 0, 1) (`sfx_cell_flowfield` of `cell_motion`),
        // read from the SPH water (`cell_fluid`). `cell_size` uses the avatar's
        // world radius; it feeds no installed patch.
        let here = object.plane(transform) / UNIT;
        next.flow_field = flow_field_param(game.fluid.speed_sum_at(here));
        next.cell_size = ((object.radius / UNIT - 0.5) * (2.0 / 3.0)).clamp(0.0, 1.0);
        next.cell_chase = if ai.iter().any(|s| s.chase_target == Some(player)) { 1.0 } else { 0.0 };
        // 0xE73F60 sends `mate intensity` (0xE5BE80) while the goal-target state
        // `cCellGame+0x51B4` is 3, measured to the goal target (+0x51B0) with the timer
        // +0x51B8: the escape mission of [`GoalTarget`] (the "goal card index" +0x7C of the
        // gate 0xE53260 is the difficulty of the serialisable data, `CellGame::difficulty`).
        // The called mate is not a goal target (its start clears it).
        if let Some(target) = goal.target.and_then(|t| cells.get(t).ok()) {
            let here = Vec2::new(transform.translation.x, transform.translation.z);
            let there = Vec2::new(target.translation.x, target.translation.z);
            next.cell_chase_loop = mate_intensity(here.distance(there) / UNIT, goal.timer);
        }
    } else {
        // No avatar: the retail code samples the visible box's centre (the look
        // point, 0xE741FD); keep the last speed history.
        ring.push(0.0);
        next.flow_field = flow_field_param(game.fluid.speed_sum_at(game.camera_look / UNIT));
    }
    *params = next;
}

/// The `flow_field` game parameter for the water at the avatar: `speed_sum` is
/// the sum of the mean speeds of the four grid cells around it (0xE4E570),
/// `clamp(0.1 * sum, 0, 1)` (0xE73F60).
pub fn flow_field_param(speed_sum: f32) -> f32 {
    (0.1 * speed_sum).clamp(0.0, 1.0)
}

/// Biquad section (RBJ cookbook, Butterworth Q).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(cutoff: f32, rate: f32, high: bool) -> Self {
        let w = std::f32::consts::TAU * cutoff.clamp(10.0, rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high { ((1.0 + c) / 2.0, -(1.0 + c)) } else { ((1.0 - c) / 2.0, 1.0 - c) };
        Self {
            b: [b0 / a0, b1 / a0, b0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The `lowpass` / `hipass` messages a patch sends once at load (cutoffs in
/// Hz) for a sound it plays, applied to the decoded PCM16 WAV (44-byte header
/// as `audio::write_wav` writes it).
fn filter_wav(wav: &mut [u8], lowpass: Option<f32>, highpass: Option<f32>) {
    if wav.len() < 44 || (lowpass.is_none() && highpass.is_none()) {
        return;
    }
    let channels = u16::from_le_bytes([wav[22], wav[23]]).max(1) as usize;
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]) as f32;
    let data = &mut wav[44..];
    let frames = data.len() / (2 * channels);
    for channel in 0..channels {
        let mut stages: Vec<Biquad> = vec![];
        stages.extend(lowpass.map(|f| Biquad::new(f, rate, false)));
        stages.extend(highpass.map(|f| Biquad::new(f, rate, true)));
        for frame in 0..frames {
            let at = (frame * channels + channel) * 2;
            let mut x = i16::from_le_bytes([data[at], data[at + 1]]) as f32;
            for stage in &mut stages {
                x = stage.process(x);
            }
            data[at..at + 2].copy_from_slice(&(x.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
        }
    }
}

/// Fixed filters of the Cell patches: `cell_motion` `hipass 200`, its flow
/// field loop `lowpass 8000`, `cell_amb` `hipass 100` (waves) and `lowpass
/// 1250` (life), `level_switch_sfx` `lowpass 500`. Hand-ported score only
/// (`OPENSPORE_PD_MUSIC=0`; the default path runs the retail patches, which set
/// every cutoff at run time). ADAPTED: the main ambience loop's cutoff is
/// `1200 * curve(level)` in the patch (1000 at load); a fixed 1100 Hz stands in.
/// The cutoffs that follow the speed or the level (`cell_motion` flow field
/// hipass, the waves lowpass) are not applied.
fn static_filter(name: &str) -> Option<(Option<f32>, Option<f32>)> {
    Some(match name {
        "cell_motion_loop" => (None, Some(200.0)),
        "sfx_cell_flowfield" => (Some(8000.0), None),
        "cell_amb_loop_waves" => (None, Some(100.0)),
        "cell_amb_life_large" => (Some(1250.0), None),
        "cell_amb_loop_new" => (Some(1100.0), None),
        "sfx_cell_level_switch" => (Some(500.0), None),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Named sounds: decode by name or id, play one-shots
// ---------------------------------------------------------------------------

/// A decoded sound (shared with `loop_audio`, which starts looped voices from it).
pub(crate) struct Loaded {
    pub(crate) def: SoundDef,
    pub(crate) clips: Vec<(Handle<AudioSource>, f32)>,
    /// The WAV bytes of each clip (shared with its `AudioSource`, no extra memory), kept for
    /// positional sounds so a Cell voice can be shaped every frame (`cell_spatial`); empty for
    /// flat sounds.
    pub(crate) raw: Vec<Arc<[u8]>>,
    /// The soundProp's distance filter curves (and its submix send).
    pub(crate) acoustics: Acoustics,
}

enum NamedState {
    Pending,
    Ready(Arc<Loaded>),
    Missing,
}

struct Job {
    id: u32,
    name: &'static str,
    looped: bool,
}

struct Decoded {
    id: u32,
    name: &'static str,
    result: Result<(SoundDef, Vec<ClipData>), String>,
    acoustics: Acoustics,
    looped: bool,
}

/// One requested one-shot.
#[derive(Clone, Debug)]
pub struct Play {
    pub id: u32,
    /// Name when known (logs, decode); ids resolve through `#XXXXXXXX`.
    pub name: Option<&'static str>,
    pub bus: Bus,
    pub gain: f32,
    pub pitch: f32,
    /// Retail units from the listener; `None` plays unattenuated.
    pub distance: Option<f32>,
    /// The (min, max) roll-off distance the sound's state curves give (a Cell creature voice,
    /// `cell_voices`): used instead of the soundProp's own, for the start gate and, with an
    /// `anchor`, by the voice on every frame (`cell_acoustics::roll_off`).
    pub range: Option<(f32, f32)>,
    /// What asked (debug log).
    pub source: &'static str,
    /// What the source is (a cell, a point, an effect component): the sound is then a positional
    /// Cell voice that the track update re-shapes every frame as the source and the listener move
    /// (roll-off, `atmospheric` low-pass, distance high-pass, panner, reverb send;
    /// `cell_spatial`). The Cell sets it; other stages keep their flat one-shots.
    pub anchor: Option<Anchor>,
}

impl Play {
    pub fn named(name: &'static str, source: &'static str) -> Self {
        Self {
            id: name_id(name),
            name: Some(name),
            bus: Bus::Sfx,
            gain: 1.0,
            pitch: 1.0,
            distance: None,
            range: None,
            source,
            anchor: None,
        }
    }
}

struct Waiting {
    play: Play,
    due: f64,
    asked: f64,
    /// The soundProp's `startdelay` has been applied.
    delayed: bool,
}

/// Decoded sounds and the queue of requested plays.
#[derive(Resource, Default)]
pub struct NamedSounds {
    jobs: Option<mpsc::Sender<Job>>,
    results: Option<Mutex<mpsc::Receiver<Decoded>>>,
    state: HashMap<u32, NamedState>,
    names: HashMap<u32, &'static str>,
    waiting: Vec<Waiting>,
    last_played: HashMap<u32, f64>,
    /// End times of the voices per sound (polyphony).
    voices: HashMap<u32, Vec<f64>>,
    started: bool,
    /// One-shots started this run (tests, debug).
    pub plays: u64,
}

/// A started one-shot voice.
#[derive(Component)]
pub struct CellVoice;

fn debug_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OPENSPORE_CELL_AUDIO_DEBUG").is_some())
}

/// Leak a name once per sound (a few hundred at most).
fn intern(id: u32, name: Option<&'static str>) -> &'static str {
    match name {
        Some(n) => n,
        None => Box::leak(format!("#{id:08X}").into_boxed_str()),
    }
}

impl NamedSounds {
    fn start(&mut self, library: Arc<AudioLibrary>) {
        let (done, results) = mpsc::channel::<Decoded>();
        let (jobs, queue) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("cell-audio-decode".into())
            .spawn(move || {
                let mut rng = rand::rng();
                for job in queue {
                    let spec = CueSpec {
                        key: job.name,
                        names: Box::leak(vec![job.name].into_boxed_slice()),
                        bus: Bus::Sfx,
                        trim: 1.0,
                        min_interval: 0.0,
                        window: None,
                        variants: if job.looped { 1 } else { 3 },
                        pool: false,
                    };
                    let mut result = crate::game_audio::load_cue(&library, &spec, &mut rng);
                    let acoustics = Acoustics::read(&library, job.name);
                    if let (Ok((_, clips)), Some((low, high))) = (&mut result, static_filter(job.name)) {
                        for clip in clips.iter_mut() {
                            filter_wav(&mut clip.wav, low, high);
                        }
                    }
                    if done.send(Decoded { id: job.id, name: job.name, result, acoustics, looped: job.looped }).is_err() {
                        break;
                    }
                }
            })
            .is_ok();
        if spawned {
            self.jobs = Some(jobs);
            self.results = Some(Mutex::new(results));
        }
        self.started = true;
    }

    /// Queue a decode once (no-op when known).
    pub fn request(&mut self, id: u32, name: Option<&'static str>, looped: bool) {
        if self.state.contains_key(&id) {
            return;
        }
        let name = intern(id, name.or_else(|| self.names.get(&id).copied()));
        self.names.insert(id, name);
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|j| j.send(Job { id, name, looped }).is_ok());
        self.state.insert(id, if sent { NamedState::Pending } else { NamedState::Missing });
    }

    /// The interned label of a sound (its name, else `#XXXXXXXX`).
    pub(crate) fn label(&mut self, id: u32) -> &'static str {
        *self.names.entry(id).or_insert_with(|| intern(id, None))
    }

    pub(crate) fn loaded(&self, id: u32) -> Option<Arc<Loaded>> {
        match self.state.get(&id) {
            Some(NamedState::Ready(l)) => Some(l.clone()),
            _ => None,
        }
    }

    pub(crate) fn missing(&self, id: u32) -> bool {
        matches!(self.state.get(&id), Some(NamedState::Missing))
    }

    /// Queue a one-shot (plays when decoded, within `LATE_PLAY` seconds).
    pub fn play(&mut self, play: Play, now: f64, delay: f32) {
        self.request(play.id, play.name, false);
        self.waiting.push(Waiting { play, due: now + delay as f64, asked: now, delayed: false });
    }

    /// Tests: a decoded sound without the decoder thread.
    #[cfg(test)]
    pub(crate) fn insert_ready(&mut self, id: u32, def: SoundDef, secs: f32) {
        self.state.insert(
            id,
            NamedState::Ready(Arc::new(Loaded { def, clips: vec![(Handle::default(), secs)], raw: vec![], acoustics: Acoustics::default() })),
        );
    }

    /// The decoder runs (a sound requested before it is marked missing for good).
    pub fn is_running(&self) -> bool {
        self.started
    }

    /// Tests: queue sounds without a decoder thread.
    #[cfg(test)]
    pub fn pretend_running(&mut self) {
        self.started = true;
    }

    /// Tests: a sound that is already decoded (one clip of 1 s).
    #[cfg(test)]
    pub(crate) fn insert_ready_for_test(&mut self, id: u32, def: SoundDef, clip: Handle<AudioSource>) {
        self.state.insert(
            id,
            NamedState::Ready(Arc::new(Loaded { def, clips: vec![(clip, 1.0)], raw: vec![], acoustics: Acoustics::default() })),
        );
    }

    /// The one-shots waiting to start (ids), oldest first (tests).
    pub fn queued(&self) -> Vec<u32> {
        self.waiting.iter().map(|w| w.play.id).collect()
    }
}

/// A request whose sound is still decoding plays if ready within this time.
const LATE_PLAY: f64 = 0.75;
const MAX_VOICES: usize = 24;

/// Start the decoder once the audio index exists, then collect results.
pub(crate) fn pump_decoder(
    audio: Res<GameAudio>,
    mut sounds: ResMut<NamedSounds>,
    assets: Option<ResMut<Assets<AudioSource>>>,
) {
    if !sounds.started {
        if let Some(library) = audio.library() {
            sounds.start(library);
            for name in PREFETCH {
                sounds.request(name_id(name), Some(name), false);
            }
        }
        return;
    }
    let ready: Vec<Decoded> = match sounds.results.as_ref().map(|r| r.lock().map(|r| r.try_iter().collect())) {
        Some(Ok(v)) => v,
        _ => return,
    };
    let Some(mut assets) = assets else {
        for d in ready {
            sounds.state.insert(d.id, NamedState::Missing);
        }
        return;
    };
    for Decoded { id, name, result, acoustics, looped } in ready {
        let state = match result {
            Ok((def, clips)) => {
                if debug_enabled() {
                    eprintln!(
                        "cell audio: decoded {name} ({} clip(s), gain {:.2}, 3d {}, poly {}{})",
                        clips.len(),
                        def.gain,
                        def.is_3d,
                        def.polyphony,
                        if looped { ", loop" } else { "" }
                    );
                }
                // Positional sounds (one-shots and loops) keep their WAV for the Cell's per-frame
                // voice (`cell_spatial`); the bytes are shared with the `AudioSource`.
                let keep = def.is_3d;
                let mut raw: Vec<Arc<[u8]>> = vec![];
                let clips = clips
                    .into_iter()
                    .map(|c| {
                        let bytes: Arc<[u8]> = c.wav.into();
                        if keep {
                            raw.push(bytes.clone());
                        }
                        (assets.add(AudioSource { bytes }), c.secs)
                    })
                    .collect();
                NamedState::Ready(Arc::new(Loaded { def, clips, raw, acoustics }))
            }
            Err(error) => {
                if debug_enabled() {
                    eprintln!("cell audio: {name} unavailable: {error}");
                }
                NamedState::Missing
            }
        };
        sounds.state.insert(id, state);
    }
}

/// Start the queued one-shots that are due and decoded.
pub(crate) fn play_sounds(
    mut commands: Commands,
    time: Res<Time<Real>>,
    audio: Res<GameAudio>,
    mut sounds: ResMut<NamedSounds>,
    active: Query<(), With<CellVoice>>,
    pd: Option<Res<crate::pd_audio::PdMusic>>,
    mut spatial: crate::cell_spatial::SpatialStart,
) {
    let now = time.elapsed_secs_f64();
    let mut voices = active.iter().count();
    let queue = std::mem::take(&mut sounds.waiting);
    let mut rng = rand::rng();
    for item in queue {
        let Waiting { play, due, asked, delayed } = item;
        if now < due {
            sounds.waiting.push(Waiting { play, due, asked, delayed });
            continue;
        }
        let Some(loaded) = sounds.loaded(play.id) else {
            if !sounds.missing(play.id) && now - asked < LATE_PLAY + (due - asked) {
                sounds.waiting.push(Waiting { play, due, asked, delayed });
            }
            continue;
        };
        let def = &loaded.def;
        if !delayed && def.start_delay > 0.0 {
            // soundProp `startdelay` (e.g. `sfx_cell_youlose` 0.3 s).
            sounds.waiting.push(Waiting {
                play,
                due: now + def.start_delay as f64,
                asked: now,
                delayed: true,
            });
            continue;
        }
        let level = play.distance.map_or(1.0, |d| cell_acoustics::roll_off(def, play.range, d));
        // The Pd mixers of the sound's groups (`chase_group`, `sfx_group`,
        // `ui_group`, ...) scale it as they scale a patch voice.
        let mixer = pd.as_ref().map_or(1.0, |pd| pd.group_gain(&def.groups));
        let gain = play.bus.volume() * def.gain * play.gain * level * mixer;
        if def.silent || level <= 0.0 || gain <= 0.0 || voices >= MAX_VOICES {
            continue;
        }
        // Polyphony (soundProp `polyphony`): at most that many live voices.
        let live = sounds.voices.entry(play.id).or_default();
        live.retain(|end| *end > now);
        if def.polyphony > 0 && live.len() >= def.polyphony as usize {
            continue;
        }
        if sounds.last_played.get(&play.id).is_some_and(|t| now - t < 0.04) {
            continue;
        }
        let pick = rng.random_range(0..loaded.clips.len());
        let (handle, secs) = loaded.clips[pick].clone();
        let speed = (def.pitch * play.pitch).clamp(0.1, 4.0);
        let label = play.name.unwrap_or("?");
        sounds.plays += 1;
        sounds.last_played.insert(play.id, now);
        sounds.voices.entry(play.id).or_default().push(now + (secs / speed) as f64);
        if debug_enabled() {
            eprintln!(
                "cell audio: play {label} <- {} gain {gain:.2} (mixer {mixer:.2}) d {:?} pitch {speed:.2}{}",
                play.source,
                play.distance,
                if play.anchor.is_some() { " (positional)" } else { "" }
            );
        }
        if audio.muted {
            continue;
        }
        // A positional Cell sound becomes a voice the track update re-shapes every frame
        // (`cell_spatial`); without a listener or the audio assets it plays flat.
        if let Some(anchor) = play.anchor {
            let spec = crate::cell_spatial::VoiceSpec {
                loaded: &loaded,
                pick,
                looped: false,
                speed,
                anchor,
                base: play.bus.volume() * def.gain * play.gain,
                mixer,
            };
            if spatial.start_ranged(&mut commands, label, spec, play.range).is_some() {
                voices += 1;
                continue;
            }
            if debug_enabled() {
                eprintln!("cell audio: {label} plays flat (no listener, source or audio assets)");
            }
        }
        commands.spawn((
            Name::new(format!("cell sfx {label}")),
            AudioPlayer::new(handle),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)).with_speed(speed),
            CellVoice,
            StageOwned,
        ));
        voices += 1;
    }
}

// ---------------------------------------------------------------------------
// Animation events
// ---------------------------------------------------------------------------

/// A sound or parameter event of a clip: clip seconds and what it does.
#[derive(Clone, Debug)]
struct ClipEvent {
    time: f32,
    id: u32,
    name: &'static str,
    /// `sound2`: set the game audio parameter `name` to `value`.
    param: Option<f32>,
    /// Event `maxSqrDist` (0 = unlimited).
    max_sqr_distance: f32,
}

/// Per clip the events that matter to audio, and per cell where its clip was
/// last frame.
#[derive(Resource, Default)]
pub struct CellClipSounds {
    clips: HashMap<u32, Arc<Vec<ClipEvent>>>,
    cursors: HashMap<Entity, (u32, f32)>,
}

impl CellClipSounds {
    fn clip(&mut self, clip: u32, originals: &Originals) -> Arc<Vec<ClipEvent>> {
        self.clips
            .entry(clip)
            .or_insert_with(|| {
                let key = crate::dbpf::ResourceKey {
                    type_id: crate::decode::TYPE_ANIM,
                    group_id: 0,
                    instance_id: clip,
                };
                Arc::new(
                    originals
                        .resource_index
                        .read(key)
                        .and_then(|b| crate::animation::parse(&b))
                        .map(|a| clip_events(&a))
                        .unwrap_or_default(),
                )
            })
            .clone()
    }
}

/// The sound (type 0) and parameter (`sound2`, type 6) events of a clip.
fn clip_events(anim: &crate::animation::Animation) -> Vec<ClipEvent> {
    let mut out = vec![];
    for channel in &anim.channels {
        for component in &channel.components {
            let crate::animation::Keys::Info(keys) = &component.keys else {
                continue;
            };
            for k in keys {
                for event in anim.events.iter().skip(k.event_start as usize).take(k.event_count as usize) {
                    if event.id == 0 {
                        continue;
                    }
                    let kind = event.flags & 0x2_0007;
                    if kind != 0 && kind != 6 {
                        continue;
                    }
                    let name: &'static str = Box::leak(event.name.clone().into_boxed_str());
                    out.push(ClipEvent {
                        time: k.frame as f32 * anim.frame_length,
                        id: if kind == 6 { name_id(name) } else { event.id },
                        name,
                        param: (kind == 6).then(|| {
                            if event.flags & 0x300 == 0x200 {
                                f32::from_bits(event.parameter0)
                            } else {
                                event.parameter0 as i32 as f32
                            }
                        }),
                        max_sqr_distance: event.max_squared_distance,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

/// Fire the sound events of the clips cells play as they pass their frames.
#[allow(clippy::too_many_arguments)]
fn clip_sound_events(
    time: Res<Time<Real>>,
    originals: Option<Res<Originals>>,
    mut events: ResMut<CellClipSounds>,
    mut sounds: ResMut<NamedSounds>,
    mut params: ResMut<CellAudioParams>,
    mut voices: ResMut<crate::cell_voices::CellVoices>,
    db: Res<CellDatabase>,
    audio: Res<GameAudio>,
    cells: Query<(Entity, &CellAnimPlayback, &Transform, &CellObject, Has<Player>, Option<&CellAnimState>)>,
    player: Query<&Transform, With<Player>>,
    game: Res<CellGame>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
) {
    let Some(originals) = originals else { return };
    let library = audio.library();
    let now = time.elapsed_secs_f64();
    // The listener (0xE73F60): the camera look point during a cinematic, else the avatar.
    let listener = cell_acoustics::listener_plane(
        cinematics.as_ref().is_some_and(|c| c.is_playing()),
        game.camera_look,
        player.single().ok().map(|t| Vec2::new(t.translation.x, t.translation.z)),
    );
    let mut seen = HashSet::new();
    for (entity, playback, transform, object, is_player, anim_state) in &cells {
        let Some(current) = &playback.current else { continue };
        seen.insert(entity);
        let at = current.time;
        let previous = events.cursors.insert(entity, (current.clip, at));
        if object.background {
            continue;
        }
        let list = events.clip(current.clip, &originals);
        if is_player && debug_enabled() && previous.map(|p| p.0) != Some(current.clip) {
            eprintln!(
                "cell audio: player clip {:08X} ({} audio events, {:.2}s)",
                current.clip,
                list.len(),
                current.length
            );
        }
        if list.is_empty() {
            continue;
        }
        // A new clip, or the same clip restarted (time went back): from the
        // first frame.
        let from = match previous {
            Some((clip, t)) if clip == current.clip && at + 1e-4 >= t => t,
            _ => -1.0,
        };
        let here = Vec2::new(transform.translation.x, transform.translation.z);
        let distance = listener.map(|l| here.distance(l) / UNIT);
        for e in list.iter() {
            if !event_due(from, at, current.length, current.cyclic, e.time) {
                continue;
            }
            if e.max_sqr_distance > 0.0 && distance.is_some_and(|d| d * d > e.max_sqr_distance) {
                continue;
            }
            if let Some(value) = e.param {
                params.triggers.push((e.id, value));
                continue;
            }
            let mut play = Play::named(e.name, "animation");
            play.id = e.id;
            play.distance = distance;
            play.anchor = Some(Anchor::Cell(entity, here));
            if crate::cell_voices::is_voice_event(e.name) {
                // Retail anim sound player 0x9A4FA0: a name starting with `v` is a voice. It plays
                // as `<event>_<species code>` of the creature's voice id (the structure id, the
                // avatar's constant), or not at all (`cell_voices`). The `vfx_*` names are voice
                // events too: those with a species variant play it, the other cells' stay silent.
                let Some(library) = library.as_deref() else { continue };
                let id = crate::cell_voices::voice_id(is_player, crate::cell_voices::structure_voice(&db, object.cell));
                let state = anim_state.map_or(0, |s| s.request_code);
                let Some(voice) = voices.play(&originals.resource_index, library, id, e.name, state, distance) else {
                    continue;
                };
                play = voice;
                play.anchor = Some(Anchor::Cell(entity, here));
            }
            sounds.play(play, now, 0.0);
        }
    }
    events.cursors.retain(|e, _| seen.contains(e));
}

// ---------------------------------------------------------------------------
// Effect sounds
// ---------------------------------------------------------------------------

// The sound components of Swarm effects play in every stage through
// `effect_audio.rs` (driven by the effect simulation: `swarm_fx::sound`).
// What an effect can reach statically is kept for the tests below.

/// One sound an effect plays.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct EffectSound {
    id: u32,
    flags: u32,
    volume: f32,
}

#[cfg(test)]
fn collect_effect_sounds(
    library: &SwarmLibrary,
    effect: EffectRef,
    depth: usize,
    seen: &mut HashSet<(u16, u32)>,
    out: &mut Vec<EffectSound>,
) {
    if depth > 8 || !seen.insert((effect.dir, effect.index)) {
        return;
    }
    let dir = library.dir(effect.dir);
    let Some(def) = dir.effects.get(effect.index as usize) else { return };
    for block in &def.blocks {
        if block.index < 0 {
            continue;
        }
        match block.kind {
            kind::SOUND if block.index & IMPORT_MASK == 0 => {
                if let Some(s) = dir.sounds.get(block.index as usize) {
                    out.push(EffectSound {
                        id: s.sound.instance,
                        flags: s.flags,
                        volume: if s.flags & sound_flag::VOLUME != 0 { s.volume } else { 1.0 },
                    });
                }
            }
            kind::EFFECT => {
                let nested = if block.index & IMPORT_MASK != 0 {
                    dir.imports
                        .get((block.index & !IMPORT_MASK) as usize)
                        .and_then(|name| library.find(*name))
                } else {
                    Some(EffectRef { dir: effect.dir, index: block.index as u32 })
                };
                if let Some(nested) = nested {
                    collect_effect_sounds(library, nested, depth + 1, seen, out);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Direct calls: mission complete, collisions
// ---------------------------------------------------------------------------

/// `ui_mission_complete` when a tutorial mission completes or a part is found
/// (RecordUnlock writes unlockedParts = 2).
fn mission_sounds(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut last: Local<Option<([i32; 6], [i32; 13])>>,
) {
    let now = time.elapsed_secs_f64();
    let missions = run.cell.missions.map(|m| m.state);
    let parts = run.cell.unlocked_parts;
    if let Some((old_missions, old_parts)) = *last {
        let mission_done = missions.iter().zip(old_missions).any(|(n, o)| *n >= 2 && o < 2);
        let part_found = parts.iter().zip(old_parts).any(|(n, o)| *n == 2 && o != 2);
        if mission_done || part_found {
            let mut play = Play::named(MISSION_COMPLETE, "mission");
            play.bus = Bus::Ui;
            sounds.play(play, now, 0.0);
        }
    }
    *last = Some((missions, parts));
}

/// The player's new contacts: `cell_collision_<a>_<b>_impact` when the
/// relative speed exceeds `minImpactCollisionSpeed`.
#[allow(clippy::too_many_arguments)]
fn collision_sounds(
    time: Res<Time<Real>>,
    db: Res<CellDatabase>,
    contacts: Res<CellContacts>,
    mut sounds: ResMut<NamedSounds>,
    players: Query<Entity, With<Player>>,
    cells: Query<(&CellObject, &Transform)>,
    mut previous: Local<HashSet<(Entity, Entity)>>,
    mut chase_group: Local<ChaseGroup>,
    params: Res<CellAudioParams>,
    pd: Option<Res<crate::pd_audio::PdMusic>>,
    game: Res<CellGame>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    // `chase_group` mixer (see [`ChaseGroup`]). With the Pd engine on, the
    // engine's mixer value reaches every sound of the group when `play_sounds`
    // starts it; this model only serves the hand-ported fallback.
    chase_group.update(params.cell_chase_loop > 0.0, params.cell_game_mate, dt);
    let engine_mixes = pd.as_ref().is_some_and(|pd| pd.context_active("cell"));
    let Ok(player) = players.single() else {
        previous.clear();
        return;
    };
    let mut now_pairs = HashSet::new();
    for c in &contacts.pairs {
        if c.a != player && c.b != player {
            continue;
        }
        now_pairs.insert((c.a, c.b));
        if previous.contains(&(c.a, c.b)) {
            continue;
        }
        let Ok([(oa, ta), (ob, tb)]) = cells.get_many([c.a, c.b]) else { continue };
        let speed = (oa.velocity - ob.velocity).length() / UNIT;
        if speed <= MIN_IMPACT_SPEED {
            continue;
        }
        let class = |o: &CellObject| {
            db.cell(o.cell)
                .map(|r| collision_class(r.sound, r.cell_type))
                .unwrap_or(2)
        };
        let Some(name) = collision_sound(class(oa), class(ob)) else { continue };
        let at = (Vec2::new(ta.translation.x, ta.translation.z) + Vec2::new(tb.translation.x, tb.translation.z)) * 0.5;
        let avatar = cells.get(player).ok().map(|(_, t)| Vec2::new(t.translation.x, t.translation.z));
        let here = cell_acoustics::listener_plane(cinematics.as_ref().is_some_and(|c| c.is_playing()), game.camera_look, avatar).unwrap_or(at);
        let mut play = Play::named(name, "collision");
        play.distance = Some(at.distance(here) / UNIT);
        play.anchor = Some(Anchor::Point(at));
        play.gain = if engine_mixes { 1.0 } else { chase_group.value() };
        sounds.play(play, now, 0.0);
    }
    *previous = now_pairs;
}

// ---------------------------------------------------------------------------
// The score, motion bed and ambience (Pd patches)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Layer {
    Motion,
    Flow,
    AmbNew,
    AmbWaves,
    AmbLife,
    AmbOcean,
    Level(i32),
    Beach,
    Chase,
}

/// A looped patch voice with its fade envelope.
#[derive(Component)]
struct LayerVoice {
    layer: Layer,
    id: u32,
    /// Level of the patch graph.
    gain: f32,
    /// Bus level times the sound's own gain.
    base: f32,
    /// Fade envelope 0..1.
    level: f32,
    fade_in: f32,
    fade_out: f32,
    releasing: bool,
}

/// State of the three patches.
#[derive(Resource)]
pub struct CellScore {
    /// Whether the patches run (stage entered, Creator closed).
    active: bool,
    level: Option<i32>,
    /// `spigot`s that swallow the first level after load / death.
    switch_open: bool,
    gain_open: bool,
    /// patch_gain_stuff: initial boost, load fade-in, chase duck.
    a: Line,
    b: Line,
    c: Line,
    a_settle_at: Option<f32>,
    /// main_loops death gain / pitch.
    death_gain: Line,
    death_pitch: Line,
    /// cell_amb level fraction (`line` 800 ms) and cell_motion speed (250 ms).
    amb_x: Line,
    speed: Line,
    chase: bool,
    clock: f32,
}

impl Default for CellScore {
    fn default() -> Self {
        Self::fresh()
    }
}

impl CellScore {
    /// Patch load (`loadbang`): stage entry or back from the Creator.
    pub fn fresh() -> Self {
        let mut b = Line::at(0.0);
        b.go(1.0, 8.0);
        Self {
            active: false,
            level: None,
            switch_open: false,
            gain_open: false,
            a: Line::at(0.7),
            b,
            c: Line::at(1.0),
            a_settle_at: None,
            death_gain: Line::at(1.0),
            death_pitch: Line::at(1.0),
            amb_x: Line::at(0.01),
            speed: Line::at(0.0),
            chase: false,
            clock: 0.0,
        }
    }

    /// patch_gain_stuff: A * B * C.
    pub fn patch_gain(&self) -> f32 {
        self.a.value * self.b.value * self.c.value
    }
}

/// The level changed to `level` (`prev` before): returns whether the level
/// switch sound and the gain boost play (the spigots swallow the first value
/// after load or death).
fn level_changed(score: &mut CellScore, level: i32) -> (bool, bool) {
    let prev = score.level.replace(level);
    if level == -9 {
        // Death: gain and pitch fall over 2 s, the spigots close. Hand-ported
        // score only (the Pd engine applies the patch's pitch ramp to 0 itself,
        // `pd::voice` clamps it at 0.01). ADAPTED: the pitch stops at 0.05 (a
        // rodio sink cannot play at speed 0; the platform voice's handling of
        // pitch 0 was not decoded).
        score.death_gain.go(0.0, 2.0);
        score.death_pitch.go(0.05, 2.0);
        score.switch_open = false;
        score.gain_open = false;
        return (false, false);
    }
    if prev == Some(-9) {
        // After the respawn: gain 0 -> 1 over 4 s, pitch over 1.2 s.
        score.death_gain.set(0.0);
        score.death_gain.go(1.0, 4.0);
        score.death_pitch.set(0.0);
        score.death_pitch.go(1.0, 1.2);
    }
    let switch = std::mem::replace(&mut score.switch_open, true);
    let boost = std::mem::replace(&mut score.gain_open, true);
    (switch && prev.is_some(), boost && prev.is_some())
}

#[allow(clippy::too_many_arguments)]
fn drive_score(
    mut commands: Commands,
    time: Res<Time<Real>>,
    audio: Res<GameAudio>,
    editor: Res<crate::editor::EditorState>,
    flow: Res<CellFlow>,
    mut params: ResMut<CellAudioParams>,
    mut score: ResMut<CellScore>,
    mut sounds: ResMut<NamedSounds>,
    mut voices: Query<(Entity, &mut LayerVoice, Option<&mut AudioSink>)>,
) {
    if crate::pd_audio::enabled() {
        // OPENSPORE_PD_MUSIC=1: the retail patches run in `pd_audio`.
        return;
    }
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    let mute = if audio.muted { 0.0 } else { 1.0 };
    let active = !editor.open && !matches!(flow.phase, FlowPhase::Editing);
    if active != score.active {
        if active {
            // Patch load: everything restarts (OnEnter of the Cell mode).
            *score = CellScore::fresh();
        }
        score.active = active;
    }
    score.clock += dt;
    let triggers = std::mem::take(&mut params.triggers);
    let level = params.level;
    if active {
        score.speed.go(params.cell_speed.clamp(0.0, 1.0), 0.25);
        let x = (params.cell_level + 0.01 - 1.0).clamp(0.0, 1.0);
        if params.cell_level > 1.0 {
            // `sel 0 1` ignores cell_level 0 (dying) and 1 (stage start).
            score.amb_x.go(x, 0.8);
        }
        // Level changes.
        if score.level != Some(level) {
            let (switch_sound, boost) = level_changed(&mut score, level);
            if level == -9 {
                let mut play = Play::named("xplosion_distant", "death");
                play.gain = 3.0;
                play.pitch = 0.8;
                sounds.play(play, now, 0.0);
            }
            if switch_sound {
                let mut play = Play::named("sfx_cell_level_switch", "level switch");
                play.gain = 0.3;
                play.pitch = 3.0;
                sounds.play(play, now, 0.0);
            }
            if boost {
                score.a.go(1.0, 0.5);
                score.a_settle_at = Some(score.clock + 1.0);
            }
        }
        if score.a_settle_at.is_some_and(|t| score.clock >= t) {
            score.a.go(0.7, 15.0);
            score.a_settle_at = None;
        }
        // Egg lay (animation `sound2` cell_egg_lay): the stinger by level
        // class, and the music fades out over 5 s.
        for (hash, value) in &triggers {
            if *hash == name_id("cell_egg_lay") && *value >= 1.0 {
                let name = if egg_lay_class(level) == 2 { "sfx_cell_egglay_minor" } else { "sfx_cell_egglay" };
                let mut play = Play::named(name, "egg lay");
                play.gain = 1.5;
                sounds.play(play, now, 0.0);
                score.b.set(1.0);
                score.b.go(0.0, 5.0);
            }
        }
        // Chase duck: the level music goes to 0.1 over 0.8 s while the chase
        // parameter is up, back over 4 s.
        let chase = params.cell_chase_loop > 0.0;
        if chase != score.chase {
            score.chase = chase;
            if chase {
                score.c.go(0.1, 0.8);
            } else {
                score.c.go(1.0, 4.0);
            }
        }
    }
    {
        let s = &mut *score;
        for line in [&mut s.a, &mut s.b, &mut s.c, &mut s.death_gain, &mut s.death_pitch, &mut s.amb_x, &mut s.speed] {
            line.step(dt);
        }
    }

    // Desired layers: (layer, sound, gain, speed, fade in, fade out, bus).
    let mut desired: Vec<(Layer, &'static str, f32, f32, f32, f32, Bus)> = vec![];
    if active {
        let s = score.speed.value;
        let x = score.amb_x.value;
        desired.push((Layer::Motion, "cell_motion_loop", MOTION_PATCH_GAIN * MOTION_GAIN.eval(s), MOTION_PITCH.eval(s).max(0.01), 0.1, 1.0, Bus::Ambience));
        desired.push((Layer::Flow, "sfx_cell_flowfield", MOTION_PATCH_GAIN * FLOW_GAIN.eval(s), 1.0, 1.0, 1.0, Bus::Ambience));
        desired.push((Layer::AmbNew, "cell_amb_loop_new", AMB_PATCH_GAIN * AMB_NEW_GAIN.eval(x), AMB_NEW_PITCH.eval(x), 0.1, 1.0, Bus::Ambience));
        desired.push((Layer::AmbWaves, "cell_amb_loop_waves", AMB_PATCH_GAIN * AMB_WAVES_GAIN.eval(x), AMB_WAVES_PITCH.eval(x), 0.1, 1.0, Bus::Ambience));
        desired.push((Layer::AmbLife, "cell_amb_life_large", AMB_PATCH_GAIN * AMB_LIFE_GAIN.eval(x), AMB_LIFE_PITCH.eval(x), 0.1, 1.0, Bus::Ambience));
        desired.push((Layer::AmbOcean, "cell_ocean_amb_lp", AMB_PATCH_GAIN * AMB_OCEAN_GAIN.eval(x), 1.0, 0.1, 1.0, Bus::Ambience));
        let music = MUSIC_PATCH_GAIN * score.patch_gain() * score.death_gain.value;
        let pitch = score.death_pitch.value.max(0.05);
        if let Some(l) = score.level {
            if (0..10).contains(&l) {
                desired.push((Layer::Level(l), LEVEL_LOOPS[l as usize], music, pitch, 1.0, 2.0, Bus::Music));
            } else if l == 10 {
                // last_level_loop: gain 1.8 * deathgain * patchgain; the
                // soundProp fades in over 5 s and out over 13 s.
                desired.push((Layer::Beach, "music_cell_beach_loop", MUSIC_PATCH_GAIN * 1.8 * score.patch_gain() * score.death_gain.value, pitch, 5.0, 13.0, Bus::Music));
            }
        }
        if params.cell_chase_loop > 0.0 {
            desired.push((Layer::Chase, "music_cell_chaseloop", MUSIC_PATCH_GAIN * CHASE_GAIN.eval(params.cell_chase_loop), 1.0, 1.0, 3.0, Bus::Music));
        }
    }

    // Reconcile with the running voices.
    let wanted: HashSet<Layer> = desired.iter().map(|d| d.0).collect();
    let mut have: HashSet<Layer> = HashSet::new();
    for (entity, mut voice, sink) in &mut voices {
        if !wanted.contains(&voice.layer) {
            voice.releasing = true;
        }
        let mut speed = None;
        if voice.releasing {
            voice.level -= dt / voice.fade_out.max(0.05);
        } else {
            have.insert(voice.layer);
            if let Some(d) = desired.iter().find(|d| d.0 == voice.layer) {
                voice.gain = d.2;
                speed = Some(d.3.clamp(0.05, 4.0));
            }
            if voice.level < 1.0 {
                voice.level += dt / voice.fade_in.max(0.05);
            }
        }
        voice.level = voice.level.clamp(0.0, 1.0);
        if let Some(mut sink) = sink {
            if let Some(speed) = speed {
                sink.set_speed(speed);
            }
            sink.set_volume(Volume::Linear(voice.gain * voice.base * voice.level * mute));
        }
        if voice.releasing && voice.level <= 0.0 {
            commands.entity(entity).despawn();
            // Long music loops are decoded again when their level returns.
            if matches!(voice.layer, Layer::Level(_) | Layer::Beach | Layer::Chase) {
                sounds.state.remove(&voice.id);
            }
        }
    }
    for (layer, name, gain, speed, fade_in, fade_out, bus) in desired {
        let id = name_id(name);
        sounds.request(id, Some(name), true);
        if have.contains(&layer) {
            continue;
        }
        let Some(loaded) = sounds.loaded(id) else { continue };
        let (handle, _) = loaded.clips[0].clone();
        let base = bus.volume() * loaded.def.gain;
        if debug_enabled() {
            eprintln!("cell audio: layer {layer:?} {name} starts (gain {:.2}, speed {speed:.2})", gain * base);
        }
        commands.spawn((
            Name::new(format!("cell layer {name}")),
            AudioPlayer::new(handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(0.0),
                speed: speed.clamp(0.05, 4.0),
                ..PlaybackSettings::ONCE
            },
            LayerVoice { layer, id, gain, base, level: 0.0, fade_in, fade_out, releasing: false },
            StageOwned,
        ));
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct CellAudioPlugin;

impl Plugin for CellAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::cell_spatial::CellSpatialPlugin)
            .init_resource::<CellAudioParams>()
            .init_resource::<NamedSounds>()
            .init_resource::<CellClipSounds>()
            .init_resource::<crate::cell_voices::CellVoices>()
            .init_resource::<CellScore>()
            .init_resource::<GoalTarget>()
            .add_systems(
                Update,
                (
                    pump_decoder,
                    update_goal_target,
                    compute_params,
                    clip_sound_events,
                    mission_sounds,
                    collision_sounds,
                    drive_score,
                    play_sounds,
                )
                    .chain()
                    .run_if(in_state(GameStage::Cell))
                    .run_if(resource_exists::<CellFlow>)
                    .run_if(resource_exists::<CellGame>)
                    .run_if(resource_exists::<CellDatabase>)
                    .run_if(resource_exists::<CellContacts>)
                    .run_if(resource_exists::<RunProgress>)
                    .run_if(resource_exists::<crate::editor::EditorState>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_follow_the_patch_functions() {
        // cell_amb: the main loop is full up to level fraction 0.32 and gone
        // at the end; the waves start at 0.25 and reach 0.6.
        assert!((AMB_NEW_GAIN.eval(0.0) - 1.0).abs() < 1e-5);
        assert!((AMB_NEW_GAIN.eval(0.321052) - 1.0).abs() < 1e-4);
        assert!(AMB_NEW_GAIN.eval(1.0).abs() < 1e-6);
        let mid = AMB_NEW_GAIN.eval(0.4);
        assert!(mid < 1.0 && mid > 0.933, "{mid}");
        assert_eq!(AMB_WAVES_GAIN.eval(0.2), 0.0);
        assert!((AMB_WAVES_GAIN.eval(1.0) - 0.6).abs() < 1e-3);
        // cell_motion: pitch 1.05 at rest (0.955555 * 1.1), 1.1 at full speed.
        assert!((MOTION_PITCH.eval(0.0) - 1.0511).abs() < 1e-3);
        assert!((MOTION_PITCH.eval(1.0) - 1.1).abs() < 1e-5);
        assert!(MOTION_GAIN.eval(0.0) == 0.0 && (MOTION_GAIN.eval(0.9) - 1.0).abs() < 1e-5);
        // Chase loop gain 0.444 at 0, 0.844 at 1.
        assert!((CHASE_GAIN.eval(0.0) - 0.444444).abs() < 1e-5);
        assert!((CHASE_GAIN.eval(1.0) - 0.844444).abs() < 1e-5);
        // Clamped outside 0..1.
        assert_eq!(AMB_OCEAN_GAIN.eval(2.0), AMB_OCEAN_GAIN.eval(1.0));
        // Ocean loop: 3 * 0.155554 at the start, silent from 0.6.
        assert!((AMB_OCEAN_GAIN.eval(0.0) - 0.466662).abs() < 1e-4);
        assert_eq!(AMB_OCEAN_GAIN.eval(0.7), 0.0);
    }

    #[test]
    fn level_index_follows_cell_level() {
        assert_eq!(level_int(0, false), 0);
        assert_eq!(level_int(99, false), 0);
        assert_eq!(level_int(100, false), 1);
        assert_eq!(level_int(450, false), 4);
        assert_eq!(level_int(999, false), 9);
        assert_eq!(level_int(1000, false), 10);
        assert_eq!(level_int(450, true), -9, "dying reports level -9");
        assert_eq!(egg_lay_class(3), 1);
        assert_eq!(egg_lay_class(4), 2);
        assert_eq!(egg_lay_class(6), 2);
        assert_eq!(egg_lay_class(7), 1);
    }

    #[test]
    fn line_ramps_like_pd() {
        let mut l = Line::at(0.0);
        l.go(1.0, 2.0);
        l.step(1.0);
        assert!((l.value - 0.5).abs() < 1e-6);
        l.step(5.0);
        assert_eq!(l.value, 1.0);
        l.set(0.25);
        l.step(1.0);
        assert_eq!(l.value, 0.25);
    }

    #[test]
    fn mate_intensity_follows_the_remap_chain() {
        // Within 1 unit: 1; 8 units: 0.75; 25 and farther: 0.1.
        assert!((mate_intensity(0.5, 10.0) - 1.0).abs() < 1e-6);
        assert!((mate_intensity(8.0, 10.0) - 0.75).abs() < 1e-6);
        assert!((mate_intensity(25.0, 10.0) - 0.1).abs() < 1e-6);
        assert!((mate_intensity(60.0, 10.0) - 0.1).abs() < 1e-6);
        // The goal timer's last five seconds fade it out (0xE5BE80: ramp clamp(timer / 5, 0, 1)).
        assert!((mate_intensity(0.5, 2.5) - 0.5).abs() < 1e-6);
        assert_eq!(mate_intensity(0.5, 0.0), 0.0);
    }

    #[test]
    fn the_escape_mission_follows_the_retail_scan_gate_and_updater() {
        // Duration by difficulty (0xE52B50): medium / hard globals, else 20 s.
        assert_eq!(goal_duration(1, 45.0, 30.0), 45.0);
        assert_eq!(goal_duration(2, 45.0, 30.0), 30.0);
        assert_eq!(goal_duration(0, 45.0, 30.0), 20.0);
        // Gate (0xE53260): easy never; the escape flag blocks; scale level 3 (food 200 on); medium
        // only once; the cooldown must have run out.
        let food = 600;
        assert!(crate::cell_engine::player_scale(food).index() >= 3, "a grown avatar");
        assert!(goal_gate(1, false, food, false, 0.0));
        assert!(!goal_gate(0, false, food, false, 0.0), "easy: no escape mission");
        assert!(!goal_gate(1, true, food, false, 0.0), "+0x51DC set");
        assert!(!goal_gate(1, false, food, true, 0.0), "medium: once per level");
        assert!(goal_gate(2, false, food, true, 0.0), "hard repeats");
        assert!(!goal_gate(2, false, food, false, 0.5), "cooldown running");
        assert!(!goal_gate(1, false, 0, false, 0.0), "a small avatar");
        // The visible box widened by size * 0.85.
        let (centre, half) = (Vec2::ZERO, Vec2::new(10.0, 7.5));
        assert!(!outside_view(Vec2::new(9.0, 0.0), 1.0, centre, half));
        assert!(!outside_view(Vec2::new(10.5, 0.0), 1.0, centre, half), "inside the widened edge");
        assert!(outside_view(Vec2::new(11.0, 0.0), 1.0, centre, half));
        assert!(outside_view(Vec2::new(11.0, 8.5), 1.0, centre, half), "the corner distance counts");
        // The machine: level init, cooldown, trigger, timer, ends.
        let e = Entity::PLACEHOLDER;
        let mut g = GoalTarget::default();
        g.reset(45.0);
        assert_eq!((g.cooldown, g.timer, g.triggered, g.target), (45.0, 0.0, false, None));
        let gate = |t: bool, c: f32| goal_gate(2, false, 600, t, c);
        assert!(!g.scan_ready(10.0, 45.0, false, gate), "35 s left");
        assert!(g.scan_ready(40.0, 45.0, false, gate), "ran out");
        g.start(e, 25.0);
        assert!(g.triggered && g.target == Some(e) && g.timer == 25.0);
        // A living target holds the cooldown at the duration.
        assert!(!g.scan_ready(1.0, 45.0, true, gate));
        assert_eq!(g.cooldown, 45.0);
        let seen = Some(GoalCell { scale_diff: 3, marked: false, outside_view: false });
        let away = Some(GoalCell { scale_diff: 3, marked: false, outside_view: true });
        // In view the timer is raised to the reset time and never ends the mission.
        g.timer = 3.0;
        g.update(1.0, 45.0, 15.0, true, seen);
        assert_eq!((g.timer, g.target), (15.0, Some(e)));
        // Out of view it runs down; at 0 the mission ends and the cooldown restarts.
        g.update(10.0, 45.0, 15.0, true, away);
        assert_eq!((g.timer, g.target), (5.0, Some(e)));
        assert!((mate_intensity(0.5, g.timer) - 1.0).abs() < 1e-6 && mate_intensity(0.5, 2.5) < 0.6, "the last 5 s fade");
        g.update(6.0, 45.0, 15.0, true, away);
        assert_eq!(g.target, None);
        assert_eq!(g.cooldown, 45.0);
        // Gone, marked, no avatar or another size class: over at once.
        for (avatar, cell) in [
            (true, None),
            (true, Some(GoalCell { marked: true, ..seen.unwrap() })),
            (false, seen),
            (true, Some(GoalCell { scale_diff: 4, ..seen.unwrap() })),
        ] {
            let mut g = GoalTarget::default();
            g.start(e, 25.0);
            g.update(0.016, 45.0, 15.0, avatar, cell);
            assert_eq!(g.target, None, "{avatar} {cell:?}");
        }
    }

    #[test]
    fn collision_names_follow_the_class_pairs() {
        assert_eq!(collision_sound(1, 1), Some("cell_collision_creature_creature_impact"));
        assert_eq!(collision_sound(2, 1), Some("cell_collision_creature_plant_impact"));
        assert_eq!(collision_sound(3, 1), Some("cell_collision_creature_rock_impact"));
        assert_eq!(collision_sound(2, 3), Some("cell_collision_plant_rock_impact"));
        assert_eq!(collision_sound(3, 3), Some("cell_collision_rock_rock_impact"));
        assert_eq!(collision_sound(4, 1), None, "bubble pairs are silent");
        assert_eq!(collision_class(CellSound::Rock, CellType::Solid), 3);
        assert_eq!(collision_class(CellSound::None, CellType::Solid), 1);
        assert_eq!(collision_class(CellSound::None, CellType::Liquid), 2);
        assert_eq!(collision_class(CellSound::None, CellType::Air), 4);
        assert_eq!(collision_class(CellSound::None, CellType::Rock), 3);
    }

    #[test]
    fn animation_events_fire_once_per_pass() {
        // One-shot: an event at 0.37 s fires on the frame that crosses it.
        assert!(!event_due(-1.0, 0.2, 1.1, false, 0.37));
        assert!(event_due(0.2, 0.4, 1.1, false, 0.37));
        assert!(!event_due(0.4, 0.6, 1.1, false, 0.37));
        // An event at time 0 fires on the first frame of a clip.
        assert!(event_due(-1.0, 0.016, 1.1, false, 0.0));
        // Looping clip: the event repeats every pass.
        assert!(event_due(2.3, 2.5, 1.2, true, 0.1), "0.1 + 2 * 1.2 = 2.5");
        assert!(!event_due(2.5, 2.7, 1.2, true, 0.1));
        assert!(event_due(2.9, 3.8, 1.2, true, 0.1), "0.1 + 3 * 1.2 = 3.7");
    }

    #[test]
    fn level_changes_swallow_the_first_value_and_death_ramps() {
        let mut score = CellScore::fresh();
        // Level 0 at load: no switch sound, no boost.
        assert_eq!(level_changed(&mut score, 0), (false, false));
        // The next change plays them.
        assert_eq!(level_changed(&mut score, 1), (true, true));
        // Death: gain and pitch fall, the spigots close.
        assert_eq!(level_changed(&mut score, -9), (false, false));
        score.death_gain.step(2.0);
        assert_eq!(score.death_gain.value, 0.0);
        // After respawn the first level is swallowed again.
        assert_eq!(level_changed(&mut score, 1), (false, false));
        assert_eq!(score.death_gain.value, 0.0, "gain restarts at 0 and rises over 4 s");
        score.death_gain.step(4.0);
        assert_eq!(score.death_gain.value, 1.0);
        assert_eq!(level_changed(&mut score, 2), (true, true));
    }

    #[test]
    fn static_filters_cut_the_stop_band() {
        // A mono PCM16 WAV of a sine at `hz`.
        let wav = |hz: f32| {
            let pcm: Vec<i16> = (0..44100)
                .map(|i| ((i as f32 * hz * std::f32::consts::TAU / 44100.0).sin() * 10000.0) as i16)
                .collect();
            crate::audio::write_wav(&pcm, 1, 44100)
        };
        let rms = |w: &[u8]| {
            // Skip the filter's settling time.
            let s: Vec<f32> = w[44 + 8000..]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32)
                .collect();
            (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
        };
        let mut high = wav(5000.0);
        let before = rms(&high);
        filter_wav(&mut high, Some(500.0), None);
        assert!(rms(&high) < before * 0.05, "lowpass 500 removes 5 kHz");
        let mut low = wav(60.0);
        let before = rms(&low);
        filter_wav(&mut low, None, Some(600.0));
        assert!(rms(&low) < before * 0.05, "hipass 600 removes 60 Hz");
        let mut pass = wav(300.0);
        let before = rms(&pass);
        filter_wav(&mut pass, Some(4000.0), Some(60.0));
        assert!((rms(&pass) / before - 1.0).abs() < 0.1, "the pass band stays");
        assert_eq!(static_filter("cell_motion_loop"), Some((None, Some(200.0))));
        assert_eq!(static_filter("sfx_cell_eat"), None);
    }

    #[test]
    fn speed_ring_averages_the_last_four() {
        let mut ring = SpeedRing::default();
        assert_eq!(ring.push(1.0), 0.25);
        ring.push(1.0);
        ring.push(1.0);
        assert_eq!(ring.push(1.0), 1.0);
        assert_eq!(ring.push(0.0), 0.75);
    }

    #[test]
    fn soundprops_attenuate_by_distance() {
        let def = SoundDef {
            name: "x".into(),
            samples: vec![],
            gain: 1.0,
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
        assert_eq!(def.attenuation(2.0), 1.0);
        assert_eq!(def.attenuation(12.0), 0.0);
        assert!((def.attenuation(8.0) - 0.5).abs() < 1e-6);
        let flat = SoundDef { is_3d: false, ..def };
        assert_eq!(flat.attenuation(100.0), 1.0);
    }

    /// The installed Cell clips carry the sound and parameter events the
    /// retail data names (instances of `Cell_eat_filter`, `Cell_hatch_layegg`).
    #[test]
    #[ignore = "needs the installed Spore animation packages"]
    fn installed_clips_carry_their_sound_events() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let events = |instance: u32| {
            let key = crate::dbpf::ResourceKey {
                type_id: crate::decode::TYPE_ANIM,
                group_id: 0,
                instance_id: instance,
            };
            let anim = crate::animation::parse(&index.read(key).unwrap()).unwrap();
            clip_events(&anim)
        };
        let eat = events(0x0527_5DF7);
        let at = |name: &str| eat.iter().find(|e| e.name == name).map(|e| e.time);
        assert_eq!(at("sfx_cell_eat"), Some(0.0));
        assert!((at("sfx_cell_squirt").unwrap() - 0.3).abs() < 0.02, "{eat:?}");
        assert!(eat.iter().all(|e| e.param.is_none()));
        // Lay egg: a `sound2` event sets the cell_egg_lay parameter to 1.
        let lay = events(0xCDB4_8A08);
        let egg = lay.iter().find(|e| e.name == "cell_egg_lay").expect("sound2 event");
        assert_eq!((egg.id, egg.param), (name_id("cell_egg_lay"), Some(1.0)));
        // Hatch: the hatch sound and the egg pop follow the clip's frames.
        let hatch = events(0x94ED_750E);
        assert!(hatch.iter().any(|e| e.name == "sfx_cell_hatch" && e.time < 0.3));
        assert!(hatch.iter().any(|e| e.name == "sfx_cell_eggpop" && e.time > 1.0));
    }

    /// The installed effect directories decode with their sound components,
    /// and the Cell effects reach the sounds the retail data names.
    #[test]
    #[ignore = "needs the installed Spore effect packages"]
    fn installed_effects_carry_their_sounds() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = SwarmLibrary::load(&index);
        assert!(library.errors.is_empty(), "{:?}", library.errors);
        let sounds_of = |name: &str| {
            let mut out = vec![];
            let root = library.find(name_id(name)).unwrap_or_else(|| panic!("{name} missing"));
            collect_effect_sounds(&library, root, 0, &mut HashSet::new(), &mut out);
            out
        };
        let ids = |name: &str| sounds_of(name).iter().map(|s| s.id).collect::<Vec<_>>();
        let die = ids("cell_death_eaten");
        assert!(die.contains(&name_id("sfx_cell_avatar_die_pop")) && die.contains(&name_id("sfx_cell_chum")), "{die:?}");
        assert!(ids("cell_eggHatch_eventEggWaves").contains(&name_id("sfx_cell_egg_hatch")));
        assert!(ids("cell_call_avatar").contains(&name_id("sfx_cell_mate_call_start")));
        assert!(ids("cell_unlock_spike").contains(&name_id("sfx_cell_part_pickup")));
        let zap = sounds_of("cell_attack_electrical_charged_multiple");
        assert!(zap.iter().any(|s| s.flags & sound_flag::LOOP != 0 && s.id == name_id("sfx_cell_electricity_lp")));
        // Nested effects are followed: the eating bubbles come with the bite
        // effect's children somewhere in the Cell set.
        assert!(!ids("cell_eatFilter_eventBubbles").is_empty());
        assert!(ids("cell_core_Burst_md").is_empty(), "silent effects stay silent");
    }

    #[test]
    #[ignore = "needs the installed Spore audio and effect packages"]
    fn installed_cell_sounds_resolve() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = AudioLibrary::from_index(&index);
        let mut missing = vec![];
        for name in PREFETCH.iter().chain(LEVEL_LOOPS.iter()).chain(
            [
                "cell_motion_loop",
                "sfx_cell_flowfield",
                "cell_amb_loop_new",
                "cell_amb_loop_waves",
                "cell_amb_life_large",
                "cell_ocean_amb_lp",
                "music_cell_beach_loop",
                "music_cell_chaseloop",
                "sfx_cell_egglay",
                "sfx_cell_egglay_minor",
            ]
            .iter(),
        ) {
            if library.resolve(name).is_err() {
                missing.push(*name);
            }
        }
        assert!(missing.is_empty(), "not installed: {missing:?}");
        let eat = library.resolve("sfx_cell_eat").unwrap();
        assert!(eat.is_3d && eat.polyphony == 3 && eat.max_distance > 0.0, "{eat:?}");
        // Pd-patch sounds are not playable samples: the patch logic is ours.
        assert!(library.resolve("cell_motion").is_err());
        // The patch loops and a level loop decode to real sound (and survive
        // the static filters).
        let mut rng = rand::rng();
        for name in ["cell_motion_loop", "cell_amb_loop_new", "music_cell_loopa", "sfx_cell_eat"] {
            let spec = CueSpec {
                key: name,
                names: Box::leak(vec![name].into_boxed_slice()),
                bus: Bus::Sfx,
                trim: 1.0,
                min_interval: 0.0,
                window: None,
                variants: 1,
                pool: false,
            };
            let (_, mut clips) = crate::game_audio::load_cue(&library, &spec, &mut rng).unwrap();
            if let Some((low, high)) = static_filter(name) {
                filter_wav(&mut clips[0].wav, low, high);
            }
            assert!(clips[0].peak > 0 && clips[0].secs > 0.1, "{name}: {} s", clips[0].secs);
        }
    }
}

#[cfg(test)]
mod mixer_tests {
    use super::*;

    fn run(group: &mut ChaseGroup, chasing: bool, mating: bool, seconds: f32) {
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            group.update(chasing, mating, 1.0 / 60.0);
        }
    }

    /// The headless run of the retail patch (`--pd-trace cellgame_seed_music6`):
    /// 1 at load, 0 one second after the chase starts, 0 -> 1 over 3 s after
    /// it ends, and 0 over 1 s at the mating moment.
    #[test]
    fn chase_group_follows_the_retail_patch() {
        let mut g = ChaseGroup::default();
        run(&mut g, false, false, 2.0);
        assert_eq!(g.value(), 1.0);
        run(&mut g, true, false, 0.5);
        assert!((g.value() - 0.5).abs() < 0.03, "halfway through the 1 s duck: {}", g.value());
        run(&mut g, true, false, 0.6);
        assert_eq!(g.value(), 0.0);
        // The chase ends: jumps to 0 and returns over 3 s.
        run(&mut g, false, false, 1.5);
        assert!((g.value() - 0.5).abs() < 0.03, "halfway through the 3 s return: {}", g.value());
        run(&mut g, false, false, 1.6);
        assert_eq!(g.value(), 1.0);
        // The mating moment mutes the group over 1 s and releases it over 3 s.
        run(&mut g, false, true, 1.1);
        assert_eq!(g.value(), 0.0);
        run(&mut g, false, false, 3.1);
        assert_eq!(g.value(), 1.0);
    }

    /// `flow_field` = clamp(0.1 * four cell speeds, 0, 1) (0xE4E570): a calm
    /// spot is quiet, a strong current saturates.
    #[test]
    fn flow_field_follows_the_local_current() {
        assert_eq!(flow_field_param(0.0), 0.0);
        assert!((flow_field_param(1.0) - 0.1).abs() < 1e-6);
        assert!((flow_field_param(5.0) - 0.5).abs() < 1e-6);
        assert_eq!(flow_field_param(10.0), 1.0);
        assert_eq!(flow_field_param(25.0), 1.0);
    }

    /// The installed Cell collision / `vfx_*` / `vcl_*` voice sounds carry the group
    /// the patch's mixer drives (`_cell_vfx_base`, `_cell_col_base`, `_cell_vox_*`
    /// add `chase_group`).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_cell_sounds_are_in_the_chase_group() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let audio = crate::game_audio::AudioLibrary::from_index(&index);
        let chase = name_id("chase_group");
        for name in ["cell_collision_creature_creature_impact", "vfx_cell_move_jet_ed", "vcl_eat_mand_chew_ch"] {
            let def = audio.resolve(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(def.groups.contains(&chase), "{name} groups {:08X?}", def.groups);
        }
        // The sfx_cell_* sounds inherit `_cell_sfx_base` only (sfx_group), as do
        // sounds outside the Cell.
        for name in ["sfx_cell_eat", "ui_mission_complete"] {
            let def = audio.resolve(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!def.groups.contains(&chase), "{name} groups {:08X?}", def.groups);
        }
        assert!(audio.resolve("sfx_cell_eat").unwrap().groups.contains(&name_id("sfx_group")));
    }
}
