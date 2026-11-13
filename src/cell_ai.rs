//! NPC behaviour (`notes\cell-re-notes.md` section 3). Every installed
//! creature uses AI type Eater (0x1006: Rescan 0xE70E30 + Behave 0xE79C30) or
//! Parts (0x1001, the player's own species: 0xE71300 / 0xE70F20).
//!
//! Original: AI block selection by difficulty; perception radii scaled by
//! G(awareness_dilation, realSize ratio); ChooseTargets (closest threat within
//! the flee radius, else closest prey within the chase radius); Threat / Prey
//! / CanEat; the 0.25 s rescan; flee / chase timers and their rests; spawn and
//! digestion output; wander probes every 0.25 + U[0, 1/16) s toward pos +
//! dir * 10; axial movement; the end-of-stage escape; the Parts awake/sleep
//! cycle with forced fears.
//!
//! The wander probe is the retail one (0xE87310 + 0xE56DC0, `wander_probe`);
//! `fearsNearbyDamage` / `protect` / `fearsNearbyDeath` are the retail reactions of
//! friends (0xE71CE0 / 0xE71F00, `cell_rules::friend_fear`); `Leak` cells count their
//! 4 s against the parent (0xE7A0D0, `leaks`). Other AI types (unused by installed
//! creatures) wander. VERIFIED (cell-rules2): flee (0xE6F940 -> 0xE6F850: pos -
//! normalize(threat - pos) * 10); the electric term of Threat (`charge +0x15C > 0 &&
//! fearsElectric`) never fires with the installed data (`+0x15C` = electricDischarge /
//! caps+0x1264, 0 in every record, 0xE71980), so it is left out; flee rest / digest
//! issue no MoveToTarget (0xE79C30), as here.

use bevy::prelude::*;
use rand::Rng;
use std::collections::HashMap;

use crate::{
    cell_contract::{CellAnimState, CellAnimation},
    cell_data::{AiType, CellAi, CellDatabase, CellGraphs, CellRecord, CellScale, CellType, MovementStyle},
    cell_engine::{
        anim_code, heading_dir, real_size, request_anim, scale_diff, CellArtCache, CellFlock, CellGame, CellGrid,
        CellKind, CellMateObject, CellObject, CellSpawnQueue, SpawnRequest, SpawnRole, UNIT,
    },
    cell_motion::{CellCaps, Drive},
    game::RunProgress,
};

/// Scan period of the Eater rescan (s).
pub const SCAN_PERIOD: f32 = 0.25;
/// Wander probe period: 0.25 + U[0, 0.0625) s.
pub const WANDER_PERIOD: f32 = 0.25;
pub const WANDER_JITTER: f32 = 0.0625;
/// Wander target distance (retail units).
pub const WANDER_DISTANCE: f32 = 10.0;
/// Wander probe length (retail units, 0xE6FF60 pushes .rdata 0x13EED94 = 1.5).
pub const WANDER_PROBE: f32 = 1.5;
/// A leak cell (plant puncture, torn-off chunk) lasts this long while its parent
/// lives (.rdata 0x148531C = 4.0, 0xE7A0D0).
pub const LEAK_SECONDS: f32 = 4.0;
/// Flee distance (retail units). VERIFIED (RE 11.5, 0xE6F850: target = pos -
/// normalize(threat - pos) x 10).
pub const FLEE_DISTANCE: f32 = 10.0;
/// Parts AI flee duration on a new threat.
pub const PARTS_FLEE_TIME: f32 = 5.0;

/// AI block for the difficulty (0xE528F0): easy -> ai_easy, hard -> ai_hard
/// when that block's type is set (!= -1); otherwise ai.
pub fn ai_block(record: &CellRecord, difficulty: usize) -> &CellAi {
    match difficulty {
        0 if record.ai_easy.kind != AiType::None => &record.ai_easy,
        2 if record.ai_hard.kind != AiType::None => &record.ai_hard,
        _ => &record.ai,
    }
}

fn or(value: f32, fallback: f32) -> f32 {
    if value != 0.0 {
        value
    } else {
        fallback
    }
}

/// What the NPC is doing (drives its animation state).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiMode {
    #[default]
    Wander,
    Flee,
    Chase,
    Rest,
    Digest,
    Asleep,
    Escape,
}

/// Per-NPC AI state (the cCellObjectData AI fields).
#[derive(Component, Clone, Debug)]
pub struct CellAiState {
    pub cell: u32,
    pub kind: AiType,
    pub difficulty: usize,
    pub mode: AiMode,
    /// What locomotion uses this frame (speed, turn factor, style).
    pub drive: Drive,
    /// Animation state while moving (18 / 19 scared / 20 mad).
    pub loop_code: i32,
    pub flee_target: Option<Entity>,
    pub chase_target: Option<Entity>,
    pub food_target: Option<Entity>,
    /// +0x21C / +0x230 / +0x224 / +0x228 / +0x238 / +0x234.
    pub flee_time: f32,
    pub chase_time: f32,
    pub flee_rest: f32,
    pub digest: f32,
    pub scan_timer: f32,
    pub spawn_timer: f32,
    pub wander_timer: f32,
    /// Parts AI sleep / awake timers (+0x1F4 / +0x1F8).
    pub sleep_timer: f32,
    pub awake_timer: f32,
    /// Meals since the last growth / digestion (+0x240).
    pub meals: i32,
    /// DigestOutput is due (digestion without a timer).
    pub digest_output: bool,
    /// Egg hatch timer (+0x1F0, Caterpillar AI); 0 = not started.
    pub hatch_timer: f32,
    /// Seconds left of the spawn event (type 0x32, 0xE79AF0): the clip of request 42.
    /// While it runs the cell is flagged spawning (+0x180) and starts no second one.
    pub spawning: f32,
    /// Seconds until the spawn event's callback (0xE79A00: the spawn output appears)
    /// runs: the clip's `contact` marker, or at once for a cell without a model;
    /// `None` once it ran.
    pub spawn_emit: Option<f32>,
}

impl CellAiState {
    pub fn new(cell: u32, ai: &CellAi) -> Self {
        Self {
            cell,
            kind: ai.kind,
            difficulty: 1,
            mode: AiMode::Wander,
            drive: Drive::default(),
            loop_code: anim_code::MOVE,
            flee_target: None,
            chase_target: None,
            food_target: None,
            flee_time: 0.0,
            chase_time: 0.0,
            flee_rest: 0.0,
            digest: 0.0,
            scan_timer: 0.0,
            spawn_timer: ai.spawn_time,
            wander_timer: 0.0,
            sleep_timer: 0.0,
            awake_timer: ai.awake_time,
            meals: 0,
            digest_output: false,
            hatch_timer: 0.0,
            spawning: 0.0,
            spawn_emit: None,
        }
    }
}

/// What the AI knows of another cell this frame.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub entity: Entity,
    pub pos: Vec2,
    pub radius: f32,
    pub cell: u32,
    pub kind: CellKind,
    pub scale: CellScale,
    pub dead: bool,
    pub cell_type: CellType,
    pub flags: i32,
    pub caps: CellCaps,
    pub modelled: bool,
    pub living: bool,
    pub avatar: bool,
    pub eggkid: bool,
    pub flee: Option<Entity>,
    pub chase: Option<Entity>,
    pub flee_time: f32,
    pub chase_time: f32,
}

/// `rel` from `a` to `b`: clamp(scale(b) - scale(a) + 2, 0, 4).
pub fn rel(a: CellScale, b: CellScale) -> usize {
    (b.index() - a.index() + 2).clamp(0, 4) as usize
}

/// `CanEat(e, f)` (0xE68470).
pub fn can_eat(e: &Seen, f: &Seen, player: CellScale) -> bool {
    if !e.modelled {
        return false;
    }
    let r = rel(e.scale, f.scale);
    if f.flags & 1 != 0 && r == 1 {
        return true;
    }
    let e_diff = scale_diff(e.scale, player);
    if e.caps.herbivore && f.cell_type == CellType::Liquid && (r == 1 || (r == 2 && e_diff == 1)) {
        return true;
    }
    if e.caps.carnivore && f.cell_type == CellType::Solid {
        return match r {
            0 | 1 => true,
            // 0xE6854E reads `byte [e + 0x112]` (the EATER's dead flag, esi =
            // arg0; earlier notes called it "f+0x116", which nothing accesses):
            // a same-level meal needs a living eater.
            2 => !e.dead,
            3 => e_diff == 1 && (f.dead || e.eggkid),
            _ => false,
        };
    }
    false
}

/// `Threat(c, o)` (0xE68870).
pub fn is_threat(db: &CellDatabase, c: &Seen, o: &Seen, ai: &CellAi, player: CellScale) -> bool {
    if o.dead
        || crate::cell_rules::should_not_attack(db, c.cell, c.scale, c.dead, o.cell, o.avatar, o.dead, player)
        || rel(c.scale, o.scale) < 2
    {
        return false;
    }
    (o.caps.spike && ai.fears_weapons)
        || (o.caps.carnivore && ai.fears_mouths)
        || (o.caps.poison > 0.0 && scale_diff(o.scale, player) >= 2 && ai.fears_poison)
}

/// `Prey(c, o)` (0xE68910).
pub fn is_prey(db: &CellDatabase, c: &Seen, o: &Seen, ai: &CellAi, player: CellScale) -> bool {
    let r = rel(c.scale, o.scale);
    !crate::cell_rules::should_not_attack(db, c.cell, c.scale, c.dead, o.cell, o.avatar, o.dead, player)
        && can_eat(c, o, player)
        && r != 0
        && (!ai.ignores_food || r >= 2)
        && !(o.caps.spike && !c.caps.spike && ai.fears_weapons)
}

/// Perception radii (0xE686C0), retail units: (chase, flee).
pub fn perception(c: &Seen, ai: &CellAi, graphs: &CellGraphs, player: CellScale) -> (f32, f32) {
    if c.avatar {
        return (1.0, 1.0);
    }
    let ratio = real_size(c.scale) / real_size(player);
    let f = graphs.eval_or(CellGraphs::AWARENESS_DILATION, ratio, ratio.clamp(0.5, 16.0));
    if !c.modelled {
        return (3.0 * f, 3.0 * f);
    }
    if ai.kind == AiType::Parts {
        return (c.caps.eyes * f, c.caps.eyes * f);
    }
    (
        or(ai.awareness_radius_food, ai.awareness_radius) * f,
        or(ai.awareness_radius_predator, ai.awareness_radius) * f,
    )
}

/// Movement style for an NPC (0xE67C40).
fn style_of(c: &Seen, ai: &CellAi) -> u32 {
    if !c.modelled {
        1
    } else if ai.movement_style.0 != 0 {
        ai.movement_style.0
    } else {
        c.caps.style
    }
}

fn axial(pos: Vec2, target: Vec2, ai: &CellAi) -> Vec2 {
    if !ai.axial_movement {
        return target;
    }
    let d = target - pos;
    if d.x.abs() >= d.y.abs() {
        Vec2::new(target.x, pos.y)
    } else {
        Vec2::new(pos.x, target.y)
    }
}

/// The probe volume of 0xE87310 (`FindCellsInRadius` broad phase, then this
/// exact test per cell): a cell with centre `c` and radius `R` is hit by the
/// capsule from `o` along the unit direction `dir`, length `length`, radius
/// `r` iff `-R <= t <= length + R` (t = dir . (c - o)) and the distance of
/// `c` from the axis is at most `R + r`. VERIFIED (0xE8744A..0xE874CD).
pub fn probe_hits(o: Vec2, dir: Vec2, length: f32, r: f32, c: Vec2, cell_radius: f32) -> bool {
    let d = c - o;
    let t = dir.dot(d);
    if t < -cell_radius || t > length + cell_radius {
        return false;
    }
    (d - dir * t).length() <= cell_radius + r
}

/// Wander probe (0xE6FF60 calling 0xE87310 with the foreground pool, length
/// 1.5 world units, radius = the cell's own size): the point ahead is blocked
/// when the probe hits more than the cell itself. 0xE56DC0 then only accepts
/// lists whose members other than the avatar are poison clouds, which never
/// holds because the cell's own entry is in the list, so any other foreground
/// cell blocks (VERIFIED, cell-rules2 / cell-ai3).
fn blocked(grid: &CellGrid, me: Entity, pos: Vec2, radius: f32, dir: Vec2) -> bool {
    let length = WANDER_PROBE * UNIT;
    let mut hit = false;
    grid.query(pos + dir * (length * 0.5), length * 0.5 + radius + 2.0 * grid.max_radius, |item| {
        if !hit && item.entity != me && probe_hits(pos, dir, length, radius, item.position, item.radius) {
            hit = true;
        }
    });
    hit
}

/// Wander (0xE6FF60): every 0.25 + U[0, 1/16) s probe ahead; blocked -> up
/// to 10 random directions; all blocked -> idle; else target pos + dir * 10.
fn wander(st: &mut CellAiState, o: &mut CellObject, me: &Seen, grid: &CellGrid, ai: &CellAi, rng: &mut impl Rng, dt: f32) {
    st.wander_timer -= dt;
    if st.wander_timer > 0.0 && o.target.is_some() {
        return;
    }
    st.wander_timer = WANDER_PERIOD + rng.random_range(0.0..WANDER_JITTER);
    let mut dir = heading_dir(o.heading);
    let mut tries = 0;
    while blocked(grid, me.entity, me.pos, o.radius, dir) {
        tries += 1;
        if tries > 10 {
            o.target = Some(me.pos);
            return;
        }
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        dir = Vec2::new(a.cos(), a.sin());
    }
    o.target = Some(axial(me.pos, me.pos + dir * WANDER_DISTANCE * UNIT, ai));
}

fn flee_from(o: &mut CellObject, me: &Seen, from: Vec2, ai: &CellAi) {
    let away = (me.pos - from).normalize_or(heading_dir(o.heading));
    o.target = Some(axial(me.pos, me.pos + away * FLEE_DISTANCE * UNIT, ai));
}

/// ChooseTargets (0xE68990): (flee, chase).
fn choose_targets(
    db: &CellDatabase,
    grid: &CellGrid,
    seen: &HashMap<Entity, Seen>,
    me: &Seen,
    ai: &CellAi,
    radii: (f32, f32),
    player: CellScale,
) -> (Option<Entity>, Option<Entity>) {
    let (chase_r, flee_r) = (radii.0 * UNIT, radii.1 * UNIT);
    let mut flee: Option<(f32, Entity)> = None;
    let mut chase: Option<(f32, Entity)> = None;
    grid.query(me.pos, chase_r.max(flee_r), |item| {
        if item.entity == me.entity {
            return;
        }
        let Some(o) = seen.get(&item.entity) else {
            return;
        };
        let d = o.pos.distance(me.pos);
        if d <= flee_r && flee.is_none_or(|(best, _)| d < best) && is_threat(db, me, o, ai, player) {
            flee = Some((d, o.entity));
        }
        if d <= chase_r && chase.is_none_or(|(best, _)| d < best) && is_prey(db, me, o, ai, player) {
            chase = Some((d, o.entity));
        }
    });
    match flee {
        Some((_, e)) => (Some(e), None),
        None => (None, chase.map(|(_, e)| e)),
    }
}

/// Emit an AI output reference (a loot table or a cell) at `at`.
fn emit(db: &CellDatabase, output: u32, scale: CellScale, at: Vec2, radius: f32, rng: &mut impl Rng, queue: &mut CellSpawnQueue) {
    if output == 0 {
        return;
    }
    if db.loot_tables.contains_key(&output) {
        let mut out = Vec::new();
        crate::cell_rules::roll_loot(db, output, scale, at, radius, rng, &|_| false, &mut out, 0);
        queue.0.extend(out);
    } else if db.cell(output).is_some() {
        queue.0.push(SpawnRequest {
            cell: output,
            scale,
            position: at,
            heading: rng.random_range(0.0..std::f32::consts::TAU),
            size_roll: rng.random_range(0.0..1.0),
            role: SpawnRole::Loot,
            ..default()
        });
    }
}

/// The spawn event (type 0x32, 0xE79AF0) on its own clock: its callback (0xE79A00) emits the
/// spawn output when the clip posts `contact` (returns true that frame), its finaliser
/// (0xE57D10) sets the next spawn timer (+0x234) and the rest timer (+0x224) from the AI
/// block's spawnTime / spawnRestTime when the clip ends.
fn spawn_event_step(st: &mut CellAiState, ai: &CellAi, dt: f32, rng: &mut impl Rng) -> bool {
    if st.spawning <= 0.0 {
        return false;
    }
    st.spawning -= dt;
    if let Some(t) = st.spawn_emit.as_mut() {
        *t -= dt;
    }
    // The callback runs at `contact`, at the latest when the event ends.
    let emit = st.spawn_emit.is_some_and(|t| t <= 0.0) || (st.spawning <= 0.0 && st.spawn_emit.is_some());
    if emit {
        st.spawn_emit = None;
    }
    if st.spawning <= 0.0 {
        st.spawning = 0.0;
        // The jitter (+U[0, 10 %] of each time): 0xE57D10 adds clamp(RandomFloat() * 0.1 * time,
        // 0, 0.1 * time) and RandomFloat (0x936040, VERIFIED) is uniform on [0, 1) (a
        // multiplicative LCG, 0x278DDE6D, seeded from rdtsc), so the distribution is the
        // retail one; only the random stream differs (it cannot be reproduced anyway).
        st.spawn_timer = ai.spawn_time * (1.0 + 0.1 * rng.random::<f32>());
        st.flee_rest = ai.spawn_rest_time * (1.0 + 0.1 * rng.random::<f32>());
    }
    emit
}

/// Eater (0xE7A0A0 = Rescan 0xE70E30 + Behave 0xE79C30).
#[allow(clippy::too_many_arguments)]
fn eater(
    db: &CellDatabase,
    grid: &CellGrid,
    seen: &HashMap<Entity, Seen>,
    game: &mut CellGame,
    queue: &mut CellSpawnQueue,
    st: &mut CellAiState,
    o: &mut CellObject,
    me: &Seen,
    flock: Option<&CellFlock>,
    leader: Option<&Seen>,
    ai: &CellAi,
    player: CellScale,
    player_at: Option<Vec2>,
    anim: &mut Option<Mut<CellAnimState>>,
    clock: &crate::cell_rules::PlayClock,
    dt: f32,
) {
    if spawn_event_step(st, ai, dt, &mut game.rng) {
        let at = me.pos + heading_dir(o.heading) * o.radius;
        emit(db, ai.spawn_output, o.scale, at, o.radius, &mut game.rng, queue);
    }
    // Rescan.
    if st.flee_rest <= 0.0 && st.digest <= 0.0 {
        let live = |e: Option<Entity>| e.and_then(|e| seen.get(&e)).is_some_and(|s| !s.dead);
        let keep = (st.flee_time > 0.0 && live(st.flee_target)) || (st.chase_time > 0.0 && live(st.chase_target));
        st.scan_timer -= dt;
        if !keep && st.scan_timer <= 0.0 {
            st.scan_timer = SCAN_PERIOD;
            if let (Some(_), Some(l)) = (flock.filter(|f| !f.leader), leader) {
                // A flock member copies its leader's flee / chase state.
                st.flee_target = l.flee;
                st.chase_target = l.chase;
                st.flee_time = l.flee_time;
                st.chase_time = l.chase_time;
            } else {
                let radii = perception(me, ai, &db.graphs, player);
                let (flee, chase) = choose_targets(db, grid, seen, me, ai, radii, player);
                if let Some(f) = flee {
                    if st.flee_target != Some(f) {
                        // Startle reaction (0xE70950): state 0x15.
                        if let Some(a) = anim.as_mut() {
                            request_anim(a, anim_code::SCARED, CellAnimation::CellNpcStartled);
                        }
                    }
                    st.flee_target = Some(f);
                    st.flee_time = ai.flee_time;
                } else if let Some(c) = chase {
                    if st.chase_target != Some(c) {
                        // "Mad" reaction (0xE70BC0).
                        if let Some(a) = anim.as_mut() {
                            request_anim(a, anim_code::MAD, CellAnimation::CellNpcMad);
                        }
                    }
                    st.chase_target = Some(c);
                    st.chase_time = ai.chase_time;
                }
            }
        }
    }
    // Behave.
    let chase_speed = or(ai.chase_speed, ai.speed);
    let wander_speed = or(ai.wander_speed, ai.speed);
    let flee_speed = or(ai.flee_speed, ai.speed);
    let style = style_of(me, ai);
    let drive = |speed: f32| Drive {
        speed,
        speed_factor: ai.turn_factor,
        style,
    };
    if game.escape {
        st.mode = AiMode::Escape;
        st.drive = drive(wander_speed);
        st.loop_code = anim_code::MOVE_SCARED;
        if let Some(p) = player_at {
            flee_from(o, me, p, ai);
        }
    } else if st.flee_rest > 0.0 {
        st.mode = AiMode::Rest;
        st.flee_rest -= dt;
        st.drive = drive(wander_speed);
        st.loop_code = anim_code::MOVE;
        o.target = None;
    } else if st.digest > 0.0 || st.digest_output {
        st.mode = AiMode::Digest;
        st.digest -= dt;
        o.target = None;
        if st.digest <= 0.0 {
            st.digest = 0.0;
            st.digest_output = false;
            // DigestOutput (0xE791F0): ai.digestionOutput at pos + facing *
            // size, source size * 0.25.
            let at = me.pos + heading_dir(o.heading) * o.radius;
            emit(db, ai.digestion_output, o.scale, at, o.radius * 0.25, &mut game.rng, queue);
        }
    } else if st.flee_time > 0.0 {
        st.mode = AiMode::Flee;
        st.drive = drive(flee_speed);
        st.loop_code = anim_code::MOVE_SCARED;
        if let Some(from) = st.flee_target.and_then(|e| seen.get(&e)).map(|s| s.pos) {
            flee_from(o, me, from, ai);
        }
        st.flee_time -= dt;
        if st.flee_time <= 0.0 {
            st.flee_rest = ai.flee_rest_time;
            st.scan_timer = 0.0;
            st.flee_target = None;
        }
    } else if st.chase_time > 0.0 {
        st.mode = AiMode::Chase;
        st.drive = drive(chase_speed);
        st.loop_code = anim_code::MOVE_MAD;
        if let Some(to) = st.chase_target.and_then(|e| seen.get(&e)).map(|s| s.pos) {
            o.target = Some(axial(me.pos, to, ai));
        }
        st.chase_time -= dt;
        if st.chase_time <= 0.0 {
            st.flee_rest = ai.chase_rest_time;
            st.scan_timer = 0.0;
            st.chase_target = None;
        }
    } else {
        st.mode = AiMode::Wander;
        if ai.spawn_time > 0.0 && st.spawning <= 0.0 {
            st.spawn_timer -= dt;
            if st.spawn_timer <= 0.0 {
                // Spawn (0xE79AF0): request 42 and event 0x32 for its clip; the spawn
                // output appears at the clip's `contact` marker (0.57 x G), and the
                // timers restart when the event ends (above).
                if let Some(a) = anim.as_mut() {
                    request_anim(a, anim_code::SPAWN, CellAnimation::CellHatchLayEggNpc);
                }
                match clock.seconds(o, anim_code::SPAWN, 0) {
                    Some(clip) => {
                        st.spawning = clip.max(1e-3);
                        // A callback runs at once for a cell without a model (0xE59C10),
                        // else when the clip posts `contact`, else when the event ends.
                        st.spawn_emit = Some(if me.modelled { clock.contact(o, anim_code::SPAWN, 0).map_or(clip, |c| c.min(clip)) } else { 0.0 });
                    }
                    None => {
                        // No clip table (no installed clips): the old fixed behaviour.
                        st.spawn_timer = ai.spawn_time + ai.spawn_rest_time;
                        let at = me.pos + heading_dir(o.heading) * o.radius;
                        emit(db, ai.spawn_output, o.scale, at, o.radius, &mut game.rng, queue);
                    }
                }
            }
        }
        st.flee_target = None;
        st.chase_target = None;
        st.drive = drive(wander_speed);
        st.loop_code = anim_code::MOVE;
        wander(st, o, me, grid, ai, &mut game.rng, dt);
    }
}

/// Parts (0xE71300 / 0xE70F20): the player's own species.
#[allow(clippy::too_many_arguments)]
fn parts(
    db: &CellDatabase,
    grid: &CellGrid,
    seen: &HashMap<Entity, Seen>,
    game: &mut CellGame,
    st: &mut CellAiState,
    o: &mut CellObject,
    me: &Seen,
    ai: &CellAi,
    player: CellScale,
    anim: &mut Option<Mut<CellAnimState>>,
    dt: f32,
) {
    let style = style_of(me, ai);
    st.drive = Drive {
        speed: me.caps.speed * or(ai.speed, 1.0),
        speed_factor: or(ai.turn_factor, 1.0),
        style,
    };
    if st.sleep_timer > 0.0 {
        st.sleep_timer -= dt;
        st.mode = AiMode::Asleep;
        st.loop_code = anim_code::ASLEEP;
        o.target = None;
        if st.sleep_timer <= 0.0 {
            st.awake_timer = ai.awake_time;
        }
        return;
    }
    if ai.awake_time > 0.0 {
        st.awake_timer -= dt;
        if st.awake_timer <= 0.0 {
            st.sleep_timer = ai.sleep_time;
        }
    }
    // Awake: eyes perception, forced fears, food not ignored.
    let mut forced = ai.clone();
    forced.fears_mouths = true;
    forced.fears_weapons = true;
    forced.fears_damage = true;
    forced.ignores_food = false;
    st.scan_timer -= dt;
    if st.scan_timer <= 0.0 {
        st.scan_timer = WANDER_PERIOD + game.rng.random_range(0.0..WANDER_JITTER);
        let radii = perception(me, &forced, &db.graphs, player);
        let (flee, chase) = choose_targets(db, grid, seen, me, &forced, radii, player);
        if let Some(f) = flee {
            if st.flee_target != Some(f) {
                st.flee_time = PARTS_FLEE_TIME;
                if let Some(a) = anim.as_mut() {
                    request_anim(a, anim_code::SCARED, CellAnimation::CellNpcStartled);
                }
            }
            st.flee_target = Some(f);
        }
        st.chase_target = chase.filter(|c| seen.get(c).is_some_and(|s| s.living));
        st.food_target = chase.filter(|c| seen.get(c).is_some_and(|s| !s.living));
    }
    if st.flee_time > 0.0 {
        st.flee_time -= dt;
        st.mode = AiMode::Flee;
        st.loop_code = anim_code::MOVE_SCARED;
        if let Some(from) = st.flee_target.and_then(|e| seen.get(&e)).map(|s| s.pos) {
            flee_from(o, me, from, &forced);
        }
        return;
    }
    st.flee_target = None;
    let target = st.chase_target.or(st.food_target).and_then(|e| seen.get(&e));
    if let Some(t) = target {
        st.mode = AiMode::Chase;
        st.loop_code = if t.living { anim_code::MOVE_MAD } else { anim_code::MOVE };
        o.target = Some(t.pos);
    } else {
        st.mode = AiMode::Wander;
        st.loop_code = anim_code::MOVE;
        wander(st, o, me, grid, ai, &mut game.rng, dt);
    }
}

/// What one Caterpillar (egg) update decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EggStep {
    /// Nothing happens this frame (waiting for the player, counting down, or
    /// retrying after a refused hatch).
    Wait,
    /// The hatch timer started (retail plays `onStartHatch`).
    Started,
    /// The timer ran out: replace the egg by `keyTransformation` at `scale`
    /// (retail plays `onHatch`).
    Hatch(CellScale),
    /// The timer ran out on an egg without a transformation: it just goes.
    Remove,
}

/// Scales a hatched cell may be created at (0xE56F20): the player's level - 1
/// up to + 3.
pub fn hatch_scale_ok(player: CellScale, scale: CellScale) -> bool {
    scale.index() >= player.index() - 1 && scale.index() <= player.index() + 3
}

/// Caterpillar (0xE78FC0, VERIFIED): an egg does not swim (no MoveToTarget is
/// issued, it drifts with the water). While the timer (+0x1F0) is 0 it waits
/// until the avatar is within `awarenessRadius` (an egg with no
/// `keyTransformation` starts at once); then the timer runs `hatchDuration`.
/// At the end the egg is replaced by `keyTransformation` at mScale +
/// `growAmount` (0xE52A20, clamped 0..19) when that scale is allowed
/// (`hatch_scale_ok`), otherwise the timer restarts; with no transformation
/// the egg is removed. `avatar_distance` is in retail world units (None: no
/// avatar object, the routine returns).
pub fn egg_step(
    timer: &mut f32,
    ai: &CellAi,
    scale: CellScale,
    player: CellScale,
    avatar_distance: Option<f32>,
    dt: f32,
) -> EggStep {
    if *timer > 0.0 {
        *timer -= dt;
        if *timer >= 0.0 {
            return EggStep::Wait;
        }
        if ai.key_transformation == 0 {
            return EggStep::Remove;
        }
        let grown = CellScale((scale.index() + ai.grow_amount).clamp(0, CellScale::MAX));
        if hatch_scale_ok(player, grown) {
            return EggStep::Hatch(grown);
        }
        *timer = ai.hatch_duration;
        return EggStep::Wait;
    }
    let Some(distance) = avatar_distance else {
        return EggStep::Wait;
    };
    if distance * distance > ai.awareness_radius * ai.awareness_radius && ai.key_transformation != 0 {
        return EggStep::Wait;
    }
    *timer = ai.hatch_duration;
    EggStep::Started
}

/// Flee reaction a neighbour gets (0xE70950): the flee target (+0x220) and time
/// (+0x21C) are set by the caller, then the startle request 0x15 plays.
pub fn start_flee(st: &mut CellAiState, anim: Option<&mut CellAnimState>, from: Entity, time: f32) {
    st.flee_target = Some(from);
    st.flee_time = time;
    if let Some(a) = anim {
        request_anim(a, anim_code::SCARED, CellAnimation::CellNpcStartled);
    }
}

/// Protect reaction (0xE70BC0): chase target (+0x22C) and time (+0x230), then
/// the "mad" request.
pub fn start_chase(st: &mut CellAiState, anim: Option<&mut CellAnimState>, target: Entity, time: f32) {
    st.chase_target = Some(target);
    st.chase_time = time;
    if let Some(a) = anim {
        request_anim(a, anim_code::MAD, CellAnimation::CellNpcMad);
    }
}

/// Where a leak cell sits on its parent this frame: the stored offset (parent
/// radii, parent frame) turned by the parent's heading.
pub fn leak_pose(parent: Vec2, parent_heading: f32, parent_radius: f32, attach: &crate::cell_engine::CellAttach) -> (Vec2, f32) {
    let turn = Quat::from_rotation_y(parent_heading) * Vec3::new(attach.local.x, 0.0, attach.local.y);
    (parent + Vec2::new(turn.x, turn.z) * parent_radius, parent_heading + attach.heading)
}

/// Leak AI (0xE7A0D0): the timer (+0x148) grows by dt; while the parent
/// (+0x14C) exists and is neither dead (+0x112) nor dying (+0x113) the leak
/// lasts until 4 s, otherwise it is destroyed at once. The cell is attached to
/// its parent (0xE5F450), so it follows the parent's pose.
pub fn leaks(
    time: Res<Time>,
    parents: Query<(&Transform, &CellObject), Without<crate::cell_engine::CellLeak>>,
    mut leaks: Query<(&mut Transform, &mut CellObject, &mut crate::cell_engine::CellLeak)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (mut t, mut o, mut leak) in &mut leaks {
        leak.age += dt;
        let Ok((pt, po)) = parents.get(leak.attach.parent) else {
            o.dead = true;
            continue;
        };
        if po.dead || leak.age > LEAK_SECONDS {
            o.dead = true;
            continue;
        }
        let (at, heading) = leak_pose(po.plane(pt), po.heading, po.radius, &leak.attach);
        t.translation.x = at.x;
        t.translation.z = at.y;
        t.rotation = Quat::from_rotation_y(heading);
        o.heading = heading;
    }
}

/// Every foreground NPC decides where to go (Dispatch 0xE7A190).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn think(
    time: Res<Time>,
    db: Res<CellDatabase>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    art: Res<CellArtCache>,
    clips: Res<crate::cell_anim::CellClipTimes>,
    grid: Res<CellGrid>,
    mut game: ResMut<CellGame>,
    mut queue: ResMut<CellSpawnQueue>,
    mut effects: MessageWriter<crate::cell_contract::CellEffectRequest>,
    mut cells: Query<
        (
            Entity,
            &Transform,
            &mut CellObject,
            Option<&mut CellAiState>,
            Option<&CellFlock>,
            Option<&mut CellAnimState>,
        ),
        Without<CellMateObject>,
    >,
) {
    let dt = time.delta_secs().min(0.1);
    let Some(globals) = db.globals.as_ref() else {
        return;
    };
    let player = crate::cell_engine::player_scale(run.cell.food_progression);
    let clock = crate::cell_rules::PlayClock { clips: &clips, art: &art, player };
    let player_caps = crate::cell_motion::player_caps(&run, &originals, &db);
    let mut caps_cache: HashMap<u32, (CellCaps, bool)> = HashMap::new();
    let mut seen: HashMap<Entity, Seen> = HashMap::new();
    let mut leaders: HashMap<u32, Entity> = HashMap::new();
    let mut player_at = None;
    for (entity, t, o, ai, flock, _) in &cells {
        if o.background {
            continue;
        }
        let record = db.cell(o.cell);
        let avatar = o.kind == CellKind::Player;
        let (caps, modelled) = if avatar {
            player_at = Some(o.plane(t));
            (player_caps, true)
        } else {
            *caps_cache.entry(o.cell).or_insert_with(|| {
                art.art.get(&o.cell).map_or((CellCaps::default(), false), |a| {
                    (CellCaps::from_counts(a.counts, &db.graphs, globals), a.modelled)
                })
            })
        };
        let cell_type = record.map_or(CellType::None, |r| r.cell_type);
        let ai_kind = record.map_or(AiType::Player, |r| ai_block(r, game.difficulty).kind);
        if let Some(f) = flock.filter(|f| f.leader) {
            leaders.insert(f.id, entity);
        }
        seen.insert(
            entity,
            Seen {
                entity,
                pos: o.plane(t),
                radius: o.radius,
                cell: o.cell,
                kind: o.kind,
                scale: o.scale,
                dead: o.dead,
                cell_type,
                flags: record.map_or(0, |r| r.flags),
                caps,
                modelled,
                living: crate::cell_rules::living(&o, ai_kind, cell_type),
                avatar,
                eggkid: record.is_some_and(|r| r.name.starts_with("cell_Eggkid")),
                flee: ai.as_ref().and_then(|a| a.flee_target),
                chase: ai.as_ref().and_then(|a| a.chase_target),
                flee_time: ai.as_ref().map_or(0.0, |a| a.flee_time),
                chase_time: ai.as_ref().map_or(0.0, |a| a.chase_time),
            },
        );
    }
    for (entity, _, mut o, ai, flock, mut anim) in &mut cells {
        let Some(mut st) = ai else {
            continue;
        };
        if o.background {
            continue;
        }
        st.difficulty = game.difficulty;
        let Some(record) = db.cell(o.cell) else {
            continue;
        };
        if o.dead {
            if let Some(a) = anim.as_mut() {
                request_anim(a, anim_code::DYING, CellAnimation::CellDeathNpcIdle);
            }
            continue;
        }
        let ai = ai_block(record, game.difficulty);
        let Some(me) = seen.get(&entity).copied() else {
            continue;
        };
        let leader = flock
            .and_then(|f| leaders.get(&f.id))
            .filter(|l| **l != entity)
            .and_then(|l| seen.get(l));
        match ai.kind {
            AiType::Parts => parts(&db, &grid, &seen, &mut game, &mut st, &mut o, &me, ai, player, &mut anim, dt),
            AiType::Eater => eater(
                &db, &grid, &seen, &mut game, &mut queue, &mut st, &mut o, &me, flock, leader, ai, player, player_at,
                &mut anim, &clock, dt,
            ),
            AiType::Caterpillar => {
                // Eggs sit still (no target) and hatch near the avatar.
                o.target = None;
                st.mode = AiMode::Wander;
                st.loop_code = anim_code::IDLE;
                let distance = player_at.map(|p| p.distance(me.pos) / UNIT);
                let step = egg_step(&mut st.hatch_timer, ai, o.scale, player, distance, dt);
                let structure = db.structure_of(record);
                let mut play = |effect: Option<u32>, depth: f32, radius: f32| {
                    if let Some(effect) = effect.filter(|e| *e != 0) {
                        effects.write(crate::cell_contract::CellEffectRequest {
                            effect,
                            position: Vec3::new(me.pos.x, -depth, me.pos.y),
                            attach_to: None,
                            scale: radius,
                        });
                    }
                };
                match step {
                    EggStep::Wait => {}
                    EggStep::Started => play(structure.map(|s| s.on_start_hatch), o.depth, o.radius),
                    EggStep::Remove => o.dead = true,
                    EggStep::Hatch(scale) => {
                        play(structure.map(|s| s.on_hatch), o.depth, o.radius);
                        // CreateCellObject at the egg's position, random
                        // facing and size roll (0xE74A20).
                        queue.0.push(SpawnRequest {
                            cell: ai.key_transformation,
                            scale,
                            position: me.pos,
                            heading: game.rng.random_range(0.0..std::f32::consts::TAU),
                            size_roll: game.rng.random_range(0.0..1.0),
                            ..default()
                        });
                        o.dead = true;
                    }
                }
            }
            _ => {
                st.drive = Drive {
                    speed: or(ai.wander_speed, ai.speed),
                    speed_factor: or(ai.turn_factor, 1.0),
                    style: style_of(&me, ai),
                };
                st.mode = AiMode::Wander;
                st.loop_code = anim_code::MOVE;
                wander(&mut st, &mut o, &me, &grid, ai, &mut game.rng, dt);
            }
        }
        if st.drive.style == 0 {
            st.drive.style = MovementStyle::FLAGELLA;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(kind: CellKind, cell_type: CellType, scale: i32, caps: CellCaps) -> Seen {
        Seen {
            entity: World::new().spawn_empty().id(),
            pos: Vec2::ZERO,
            radius: 1.0,
            cell: scale as u32 + 100 * cell_type.raw() as u32,
            kind,
            scale: CellScale(scale),
            dead: false,
            cell_type,
            flags: 0,
            caps,
            modelled: true,
            living: kind.is_creature(),
            avatar: kind == CellKind::Player,
            eggkid: false,
            flee: None,
            chase: None,
            flee_time: 0.0,
            chase_time: 0.0,
        }
    }

    #[test]
    fn ai_block_follows_the_difficulty_rule() {
        let mut record = CellRecord::default();
        record.ai.kind = AiType::Eater;
        record.ai.speed = 1.0;
        record.ai_easy.kind = AiType::None;
        record.ai_hard.kind = AiType::Eater;
        record.ai_hard.speed = 3.0;
        assert_eq!(ai_block(&record, 0).speed, 1.0, "unset easy block -> ai");
        assert_eq!(ai_block(&record, 1).speed, 1.0);
        assert_eq!(ai_block(&record, 2).speed, 3.0);
    }

    #[test]
    fn can_eat_and_prey_follow_mouths_and_levels() {
        let p = CellScale(2);
        let herb = CellCaps {
            herbivore: true,
            ..default()
        };
        let carn = CellCaps {
            carnivore: true,
            ..default()
        };
        let grazer = seen(CellKind::Creature, CellType::Solid, 2, herb);
        let hunter = seen(CellKind::Creature, CellType::Solid, 2, carn);
        let plant_small = seen(CellKind::PlantFood, CellType::Liquid, 1, CellCaps::default());
        let plant_same = seen(CellKind::PlantFood, CellType::Liquid, 2, CellCaps::default());
        let prey = seen(CellKind::Creature, CellType::Solid, 1, CellCaps::default());
        let big = seen(CellKind::Creature, CellType::Solid, 3, CellCaps::default());
        assert!(can_eat(&grazer, &plant_small, p));
        assert!(!can_eat(&grazer, &plant_same, p), "same-level plant needs a Smaller eater");
        assert!(can_eat(&hunter, &prey, p));
        assert!(can_eat(&hunter, &hunter, p));
        assert!(!can_eat(&hunter, &big, p));
        assert!(!can_eat(&hunter, &plant_small, p));
        let mut corpse = hunter;
        corpse.dead = true;
        assert!(!can_eat(&corpse, &hunter, p), "a same-level meal needs a living eater (0xE6854E)");
        let mut unmodelled = hunter;
        unmodelled.modelled = false;
        assert!(!can_eat(&unmodelled, &prey, p));
    }

    #[test]
    fn threats_are_armed_cells_of_the_same_size_or_bigger() {
        let db = CellDatabase::default();
        let p = CellScale(2);
        let ai = CellAi {
            fears_mouths: true,
            fears_weapons: true,
            ..default()
        };
        let me = seen(CellKind::Creature, CellType::Solid, 2, CellCaps::default());
        let spiky_small = seen(
            CellKind::Creature,
            CellType::Solid,
            1,
            CellCaps {
                spike: true,
                ..default()
            },
        );
        let jaw_big = seen(
            CellKind::Creature,
            CellType::Solid,
            3,
            CellCaps {
                carnivore: true,
                ..default()
            },
        );
        assert!(!is_threat(&db, &me, &spiky_small, &ai, p));
        assert!(is_threat(&db, &me, &jaw_big, &ai, p));
        let calm = CellAi::default();
        assert!(!is_threat(&db, &me, &jaw_big, &calm, p));
    }

    #[test]
    fn perception_scales_with_relative_size() {
        let graphs = CellGraphs::default();
        let ai = CellAi {
            kind: AiType::Eater,
            awareness_radius: 8.0,
            awareness_radius_food: 0.0,
            awareness_radius_predator: 4.0,
            ..default()
        };
        let same = seen(CellKind::Creature, CellType::Solid, 2, CellCaps::default());
        assert_eq!(perception(&same, &ai, &graphs, CellScale(2)), (8.0, 4.0));
        let bigger = seen(CellKind::Creature, CellType::Solid, 3, CellCaps::default());
        let (chase, _) = perception(&bigger, &ai, &graphs, CellScale(2));
        assert!((chase - 8.0 * 3.0).abs() < 1e-4);
    }

    #[test]
    fn eggs_wait_for_the_player_then_hatch_into_the_key_transformation() {
        let ai = CellAi {
            kind: AiType::Caterpillar,
            awareness_radius: 10.0,
            hatch_duration: 5.0,
            key_transformation: 77,
            ..default()
        };
        let (egg, player) = (CellScale(4), CellScale(4));
        let mut timer = 0.0;
        // Out of range: nothing happens, however long it takes.
        assert_eq!(egg_step(&mut timer, &ai, egg, player, Some(10.5), 0.1), EggStep::Wait);
        assert_eq!(timer, 0.0);
        // No avatar object: the routine returns.
        assert_eq!(egg_step(&mut timer, &ai, egg, player, None, 0.1), EggStep::Wait);
        // Near the player the timer starts and runs hatchDuration.
        assert_eq!(egg_step(&mut timer, &ai, egg, player, Some(9.0), 0.1), EggStep::Started);
        assert_eq!(timer, 5.0);
        assert_eq!(egg_step(&mut timer, &ai, egg, player, Some(30.0), 4.9), EggStep::Wait);
        // The timer keeps running when the player leaves; at the end it hatches.
        assert_eq!(egg_step(&mut timer, &ai, egg, player, Some(30.0), 0.2), EggStep::Hatch(egg));
        // growAmount raises the scale; a scale outside p-1..p+3 retries.
        let mut grown = ai.clone();
        grown.grow_amount = 1;
        let mut timer = 0.01;
        assert_eq!(egg_step(&mut timer, &grown, egg, player, None, 0.1), EggStep::Hatch(CellScale(5)));
        let mut timer = 0.01;
        assert_eq!(egg_step(&mut timer, &grown, CellScale(7), player, None, 0.1), EggStep::Wait);
        assert_eq!(timer, 5.0, "refused: the timer restarts");
        assert!(hatch_scale_ok(player, CellScale(3)) && hatch_scale_ok(player, CellScale(7)));
        assert!(!hatch_scale_ok(player, CellScale(2)) && !hatch_scale_ok(player, CellScale(8)));
        // No transformation: starts anywhere, then the egg is removed.
        let mut plain = ai.clone();
        plain.key_transformation = 0;
        let mut timer = 0.0;
        assert_eq!(egg_step(&mut timer, &plain, egg, player, Some(99.0), 0.1), EggStep::Started);
        assert_eq!(egg_step(&mut timer, &plain, egg, player, Some(99.0), 6.0), EggStep::Remove);
    }

    #[test]
    fn the_wander_probe_is_a_capsule_in_front_of_the_cell() {
        let o = Vec2::ZERO;
        let dir = Vec2::X;
        // Length 1.5, own radius 0.5: a unit cell 1.5 ahead is hit, one 3 ahead is not.
        assert!(probe_hits(o, dir, 1.5, 0.5, Vec2::new(1.5, 0.0), 1.0));
        assert!(!probe_hits(o, dir, 1.5, 0.5, Vec2::new(3.2, 0.0), 1.0));
        // Beside the axis: hit within R + r, missed beyond.
        assert!(probe_hits(o, dir, 1.5, 0.5, Vec2::new(1.0, 1.4), 1.0));
        assert!(!probe_hits(o, dir, 1.5, 0.5, Vec2::new(1.0, 1.6), 1.0));
        // Behind the cell by more than its radius: not hit; overlapping the
        // start of the probe: hit.
        assert!(!probe_hits(o, dir, 1.5, 0.5, Vec2::new(-1.2, 0.0), 1.0));
        assert!(probe_hits(o, dir, 1.5, 0.5, Vec2::new(-0.9, 0.0), 1.0));
    }

    #[test]
    fn leaks_sit_on_their_parent_and_turn_with_it() {
        let attach = crate::cell_engine::CellAttach {
            parent: World::new().spawn_empty().id(),
            local: Vec2::new(0.0, -1.0),
            heading: 0.5,
        };
        // Heading 0 faces -Z: the offset (0, -1) is the parent's front.
        let (at, h) = leak_pose(Vec2::new(2.0, 3.0), 0.0, 2.0, &attach);
        assert!(at.abs_diff_eq(Vec2::new(2.0, 1.0), 1e-5), "{at}");
        assert!((h - 0.5).abs() < 1e-6);
        // Turned a quarter circle the front is -X.
        let (at, _) = leak_pose(Vec2::ZERO, std::f32::consts::FRAC_PI_2, 1.0, &attach);
        assert!(at.abs_diff_eq(Vec2::new(-1.0, 0.0), 1e-5), "{at}");
    }

    #[test]
    fn a_leak_lasts_four_seconds_and_dies_with_its_parent() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::cell_engine::{CellAttach, CellLeak};
        let object = |hp: i32| CellObject {
            cell: 1,
            kind: CellKind::PlantFood,
            scale: CellScale(3),
            abs_size: 1.0,
            radius: 2.0,
            size_vel: 0.0,
            hp,
            density: 1.0,
            mass: 1.0,
            velocity: Vec2::ZERO,
            heading: 0.0,
            target: None,
            depth: 0.0,
            background: false,
            tile: None,
            age: 0.0,
            invulnerable: 0.0,
            passive: false,
            dead: false,
        };
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let parent = world.spawn((Transform::from_xyz(5.0, 0.0, 0.0), object(2))).id();
        let attach = CellAttach {
            parent,
            local: Vec2::new(1.0, 0.0),
            heading: 0.0,
        };
        let leak = world.spawn((Transform::default(), object(2), CellLeak { attach, age: 0.0 })).id();
        let step = |world: &mut World, secs: f32| {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
            world.run_system_once(leaks).unwrap();
        };
        step(&mut world, 0.1);
        let t = world.get::<Transform>(leak).unwrap().translation;
        assert!((t.x - 7.0).abs() < 1e-4 && t.z.abs() < 1e-4, "{t}");
        assert!(!world.get::<CellObject>(leak).unwrap().dead);
        // Still alive just under 4 s (the timer counts every update), dead after.
        for _ in 0..38 {
            step(&mut world, 0.1);
        }
        assert!(!world.get::<CellObject>(leak).unwrap().dead, "3.9 s");
        step(&mut world, 0.1);
        step(&mut world, 0.1);
        assert!(world.get::<CellObject>(leak).unwrap().dead, "past 4 s");
        // A dead parent takes the leak with it at once.
        let leak2 = world.spawn((Transform::default(), object(2), CellLeak { attach, age: 0.0 })).id();
        world.get_mut::<CellObject>(parent).unwrap().dead = true;
        step(&mut world, 0.1);
        assert!(world.get::<CellObject>(leak2).unwrap().dead);
    }

    #[test]
    fn the_spawn_output_comes_at_contact_and_the_timers_restart_when_the_clip_ends() {
        use rand::SeedableRng;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let block = CellAi { spawn_time: 10.0, spawn_rest_time: 4.0, ..default() };
        let mut st = CellAiState::new(0, &block);
        assert!(!spawn_event_step(&mut st, &block, 0.1, &mut rng), "no event, nothing happens");
        // A clip of 1 s with `contact` at 0.57 s.
        st.spawning = 1.0;
        st.spawn_emit = Some(0.57);
        st.spawn_timer = 0.0;
        assert!(!spawn_event_step(&mut st, &block, 0.5, &mut rng));
        assert!(spawn_event_step(&mut st, &block, 0.1, &mut rng), "the callback runs at contact");
        assert!(!spawn_event_step(&mut st, &block, 0.1, &mut rng), "once");
        assert!(st.spawning > 0.0 && st.flee_rest == 0.0 && st.spawn_timer == 0.0, "the timers wait for the event's end");
        assert!(!spawn_event_step(&mut st, &block, 0.5, &mut rng));
        assert_eq!(st.spawning, 0.0);
        assert!((10.0..=11.0).contains(&st.spawn_timer), "{}", st.spawn_timer);
        assert!((4.0..=4.4).contains(&st.flee_rest), "{}", st.flee_rest);
        // A callback that never saw `contact` runs when the event ends.
        st.spawning = 0.2;
        st.spawn_emit = Some(5.0);
        assert!(spawn_event_step(&mut st, &block, 0.3, &mut rng));
    }
}

