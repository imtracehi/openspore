//! Retail Cell object rules: contacts and what they do (eat, bite, stab,
//! swallow, pop, collect, poison), damage, deaths and loot, food progression,
//! growth and DNA, unlocks, missions, and the saved Cell data.
//!
//! Original (`notes\cell-re-notes.md`): `GetDamageAmount` / `ShouldNotAttack`
//! (2A); contact classification by part type and size relation, one
//! interaction per pair, the kind -> action table, bite resolution, `Damage`
//! (kind 1 electric / 3 poison / 5 bite / else stab -> reaction codes
//! 14 / 15 / 17 / 16), swallow, `PlayerAte` (hp regen x2 on easy, cap 6; the
//! eaten cell's foodValue, default 10; isPlant by mouth), `AddFood` (cap 1000,
//! overflow counters, plant progression), poison once per 2 s per cloud
//! (2B); level change and Grow / SetSize (4B); DNA earned = the scale-info +8
//! column interpolated over food (4B); NPC death effects by ScaleDiff and
//! loot only for victims at the player's scale or one above, `EatKill` /
//! eaten effects, RecordUnlock (value 2, mate button timers, mission 1), the
//! loot roll (one-pass weighted pick, count + RandomInt(countDelta), radius
//! U[minRadius, maxRadius or source size], expelForce, levelOffset clamped to
//! 0..19, unlock entries only while not unlocked) (6); NPC meals (3);
//! `Living`; mass = density * size^2 (4A); the save mirrors
//! `cCellSerializableData` (ModAPI).
//!
//! Part contacts follow 11.6 (0xE73390): every part sphere of a cell that
//! touches the other cell's body sphere is a contact with that part's type,
//! the interaction kind comes from `classify` and the contact point is the
//! midpoint of the two surfaces (`touch_point`). Kind 6 (a spike against a
//! plant, 0xE777A0) tears a `leak` cell off the plant at the point; a bite on a
//! cell more than a level above the player does the same with the special cell
//! #27 (0xE7B280 -> 0xE76AF0, scale = the player's scale, size = the parent's size,
//! attached to the parent; the leak AI 0xE7A0D0 removes it after 4 s). The
//! electric search runs per electric part (0xE69190 / 0xE69000 / 0xE693F0),
//! the zap hurts through `GetDamageAmount` only (0xE71520), and `Damage` tells
//! the friends of the victim (0xE71CE0, `friend_fear`).
//!
//! Timed by the clip (RE 2B / 6, 10F; [`PlayClock`] asks
//! `cell_anim::CellClipTimes` what `PlayAnimation` returned): the hit lock
//! (+0x17C / +0x178: the victim's reaction clip), the bite (event 4: it starts
//! with the attacker's bite clip, which locks the attacker, and lands at the
//! clip's `contact` marker, callback 0xE7B280, at the contact point of the
//! bite's start), the stab (event 10: the damage is immediate, the attacker is
//! locked for the clip, +0x17F), the swallow (event 1 at 1/3 of the eat clip,
//! which is also the meal; a plant sucked by a proboscis takes the whole clip),
//! the break death (event 5: the clip of request 8) and the gap between two
//! spike stabs on a plant (the clip of request 0x1C). Without the clip table
//! (no installed clips) the old fixed durations run.
//!
//! The timed-event table 0xE7E6C0 is traced in `notes\cell-timing2-notes.md`: the
//! zap is event 0x11 on the zapper (request 0x1E; it and the first target are
//! held for that clip) plus event 2 per target (request 0xE / 0x3C: the hit
//! lock, and the death check 0xE7A770 when the reaction clip ends), the proboscis
//! drain is event 0x13 (request 13 on the attacker, 0x20 on the victim; its
//! finaliser 0xE7DD90 is the meal and the victim's loss at the end of the
//! clip), an eaten modelled victim plays request 9 (event 7) after the swallow,
//! and the avatar's break death plays request 8 (event 5) before its death flow.
//!
//! The part spheres are retail's (11.6): an NPC's are its creation's rigblocks, the
//! avatar's are its placed parts with the radius `max(|dx|, |dz|) * s / 2` of each
//! part model's own box (`cell_motion::player_part_spheres`, 0xE83E30; the
//! estimated radius `cell_motion::PART_SPHERE_RADIUS` is only the fallback while
//! no model box exists). The weapon points follow the animated pose of an NPC
//! (`cell_anim::CellPartPoses`, 0xE84150): electric sources and poison emit points
//! (pose plus the rigblock's +0x60 offset, 0xE68000). The drained victim is pulled
//! to the mouth part and attached there ([`drain_pull`], 0xE69C10 / 0xE5F4D0 /
//! 0xE62380) and both cells play the retract clips at the drain's end (0xE72530
//! with 0x22 / 0x21) with event 0x15's knockback vector, the mouth axis (0xE67D30)
//! for the drainer and its negation for the victim. The leak's facing is
//! `LookRotation(forward = away from the parent)` (0xE68170 -> 0x69B600: the
//! leak's local +Y points out of the wound, the heading convention of
//! `heading_of`).
//!
//! ADAPTED: a cell without any part data (a headless run) falls back to the
//! direction stand-in (mouths in front, spikes anywhere, else the body); the
//! drained victim's attach transform has no rotation; break pieces roll the
//! record's `pieces` table.

use bevy::{ecs::system::SystemParam, prelude::*};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    cell_anim::{dilation_factor, CellClipTimes, SelectContext},
    cell_contract::{CellAnimState, CellAnimation, CellEffectRequest},
    cell_data::{AiType, CellDatabase, CellRecord, CellScale, CellType, LootEntryType, UnlockType},
    cell_engine::{
        anim_code, heading_dir, player_abs_size, player_size_for, request_anim, scale_diff, world_scale_for,
        CellArtCache, CellGame, CellGrid, CellKind, CellObject, CellSpawnQueue, SpawnRequest, SpawnRole, UNIT,
    },
    cell_motion::{CellCaps, CellMotion},
    game::{Health, Player, RunProgress},
};

/// ModAPI / RE 2A: the maximum HP of every cell.
pub const MAX_HP: i32 = 6;
/// Immunity of the avatar during the hatch / intro events, refreshed every
/// frame. VERIFIED: their per-frame updates (0xE5EC10 / 0xE5EC70) set the
/// already-hit flag +0x17C that `Damage` (0xE7A7C0) skips on; the hatch end
/// (0xE503E0) grants nothing, so the avatar is vulnerable right after
/// (VERIFIED; no grace after the hatch).
pub const HATCH_GRACE: f32 = 0.05;
/// "Already hit" window of a victim (+0x17C): retail holds it for the
/// reaction clip `PlayAnimation` returned (0xE7A7C0 -> 0xE59010); this is the
/// fallback while the clip table is not loaded.
pub const HIT_COOLDOWN: f32 = 0.6;
/// Poison clouds hurt at most once per 2000 ms per cloud (RE 2B).
pub const POISON_PERIOD: f32 = 2.0;
/// Fallback only, for a cell without part spheres (a headless run without the
/// part catalog): its mouths reach a contact within this cone (cosine).
/// ADAPTED: retail has no such rule, every part is a sphere (11.6).
pub const MOUTH_CONE: f32 = 0.5;
/// A spike stab on a plant may start again after the attacker's clip of request
/// 0x1C (the attacker is locked while it and event 0xE run, 0xE593B0); this is the
/// fallback while the clip table is not loaded. ADAPTED: the lock's source
/// (0xE593B0) is not traced.
pub const PUNCTURE_SECONDS: f32 = 1.0;
/// Request codes of the zap and the drain (0xE71520, 0xE727E0; the select rows 30 /
/// 60 / 32 are `CellElecRelease`, `EpicCellElectrocuted` and the proboscis victim).
/// The zapper plays 0x1E for its lock (event 0x11), a zapped cell 0xE (0x3C for
/// `cell_puffish`, event 2), a drained victim 0x20.
pub const ZAP_RELEASE: i32 = 0x1E;
pub const HIT_ELECTRIC_PUFFISH: i32 = 0x3C;
pub const DRAIN_VICTIM: i32 = 0x20;
/// The drain's end plays request 0x22 on the drainer (`CellEatProbRetract`) and 0x21 on
/// the victim (`CellEatProbRetractTrgt`) through 0xE72530 (the finaliser 0xE7DD90 calls it
/// twice, edi = 0x22 / 0x21; table rows 48 / 47, NPC rows 122 / 121).
pub const DRAIN_RETRACT: i32 = 0x22;
pub const DRAIN_RETRACT_VICTIM: i32 = 0x21;
/// Share of the drain's clip the victim takes to be pulled to the mouth: the event's
/// sub-timer 0 is created as 0.125 x the clip (0xE728C6 x .rdata 0x1486CF0 = 0.125) and
/// 0xE69C10 closes the distance `dt / remaining` per frame while it runs.
pub const DRAIN_PULL_SHARE: f32 = 0.125;
/// Food that turns on the mate button with the next meal (RE 2B).
pub const MATE_BUTTON_FOOD: i32 = 149;
/// Damage kinds passed to `Damage` (RE 2B / 7).
pub const DAMAGE_ELECTRIC: u8 = 1;
pub const DAMAGE_STAB: u8 = 2;
pub const DAMAGE_POISON: u8 = 3;
pub const DAMAGE_BITE: u8 = 5;

/// `cCellMission` (state, progress, plantProgress, field_C).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CellMissionSave {
    pub state: i32,
    pub progress: i32,
    pub plant_progress: i32,
    pub field_c: i32,
}

/// Saved Cell state: `cCellSerializableData` fields plus what OpenSpore needs
/// to resume the same world (seeded population, scale, player placement).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CellSave {
    pub game_mode: i32,
    pub food_progression: i32,
    pub plant_food_progression: i32,
    pub over_plant_food_progression: i32,
    pub over_animal_food_progression: i32,
    pub evolution_points_spent: i32,
    /// Indexed by `UnlockType` (2 mandible .. 10 jet, 12 body); RecordUnlock
    /// writes 2.
    pub unlocked_parts: [i32; 13],
    pub part_cinematic_played: bool,
    /// The part guarantee (RE 10A, `cell_part_guide`) is due: set by `add_food`,
    /// consumed when the scene spawns.
    pub part_guide_pending: bool,
    pub show_mate_button: bool,
    pub kill_count: i32,
    pub death_count: i32,
    pub game_time_ms: i64,
    /// Seed of the deterministic population (retail `gameID`).
    pub game_id: u32,
    /// 0 easy, 1 normal, 2 hard.
    pub difficulty: i32,
    /// Tutorial missions: 0 eat 5 times, 1 collect 6 parts, 3 mate.
    pub missions: [CellMissionSave; 6],
    pub player_has_moved: bool,
    pub player_has_eaten: bool,
    pub first_editor_entry: bool,
    pub world_scale: f32,
    pub origin_abs: [f32; 2],
    pub player_position: [f32; 2],
    pub player_heading: f32,
    pub player_hp: i32,
    pub zoom: f32,
    pub generation: u32,
    pub migrated: bool,
    /// `cCellSerializableData` `field_30`: the DNA earned at the last creator visit
    /// (written on the editor return, 0xE809A0); `None` in a save that predates it.
    pub dna_at_creator: Option<i32>,
    /// The history record the End-of-stage timeline is built from (0xE394A0, see
    /// `cell_hud::timeline::events`); empty in a save that predates it.
    pub timeline: Vec<crate::cell_hud::timeline::events::TimelineEvent>,
    /// The creature of every editor visit (`clg_evolveineditor` record number n = generation n + 2): what the
    /// History's generation card shows (retail renders the card's picture from the creation record). Filled by
    /// `cell_hud::timeline::thumbs::record`; `None` for a visit that predates the record.
    pub generations: Vec<Option<crate::cell_hud::timeline::events::GenerationRecord>>,
}

impl Default for CellSave {
    fn default() -> Self {
        Self {
            game_mode: 0,
            food_progression: 0,
            plant_food_progression: 0,
            over_plant_food_progression: 0,
            over_animal_food_progression: 0,
            evolution_points_spent: 0,
            unlocked_parts: [0; 13],
            part_cinematic_played: false,
            part_guide_pending: false,
            show_mate_button: false,
            kill_count: 0,
            death_count: 0,
            game_time_ms: 0,
            game_id: 0,
            difficulty: 1,
            missions: [CellMissionSave::default(); 6],
            player_has_moved: false,
            player_has_eaten: false,
            first_editor_entry: true,
            world_scale: 0.0,
            origin_abs: [0.0; 2],
            player_position: [0.0; 2],
            player_heading: 0.0,
            player_hp: 3,
            zoom: 0.0,
            generation: 0,
            migrated: false,
            dna_at_creator: None,
            timeline: Vec::new(),
            generations: Vec::new(),
        }
    }
}

impl CellSave {
    /// The disposition value of the moment (0xE5C530 = 0xE511C0(overPlant + plant,
    /// overAnimal + food - plant)): -1 herbivore .. +1 carnivore.
    pub fn diet_value(&self) -> f32 {
        let plant = self.over_plant_food_progression + self.plant_food_progression;
        let animal = self.over_animal_food_progression + self.food_progression - self.plant_food_progression;
        crate::cell_flow::diet_balance(plant, animal)
    }
    /// 0xE394A0: one history record with the game time in seconds (0xE5C520 / 1000)
    /// and the disposition value of the moment.
    pub fn record_event(&mut self, kind: crate::cell_hud::timeline::events::EventKind) {
        let event = crate::cell_hud::timeline::events::TimelineEvent {
            kind,
            seconds: (self.game_time_ms / 1000) as i32,
            value: self.diet_value(),
        };
        self.timeline.push(event);
    }
    /// A kill of the player (0xE514A0 records `clg_killedsomething`).
    pub fn count_kill(&mut self) {
        self.kill_count += 1;
        self.record_event(crate::cell_hud::timeline::events::EventKind::Kill);
    }
    /// The player's death (0xE5C9AE records `clg_death`).
    pub fn count_death(&mut self) {
        self.death_count += 1;
        self.record_event(crate::cell_hud::timeline::events::EventKind::Death);
    }
    pub fn unlocked(&self, unlock: UnlockType) -> bool {
        self.unlocked_parts
            .get(unlock.raw() as usize)
            .is_some_and(|v| *v > 0)
    }
    /// Unlock categories collected so far (the goal card counts these).
    pub fn unlocked_count(&self) -> usize {
        self.unlocked_parts.iter().filter(|v| **v > 0).count()
    }
}

/// DNA earned at a food progression (0xE51410): the scale-info +8 column,
/// linear between entries (0 at 0, 5 at 50, 10 at 100 ... 65 at 1000).
pub fn dna_earned(food: i32) -> i32 {
    let table = &crate::cell_engine::SCALE_INFO;
    let i = crate::cell_engine::scale_info_index(food);
    let e = &table[i];
    if food == e.required_food || i + 1 >= table.len() {
        return e.field_8;
    }
    let next = &table[i + 1];
    let t = (food - e.required_food) as f32 / (next.required_food - e.required_food) as f32;
    (e.field_8 as f32 + (next.field_8 - e.field_8) as f32 * t).round() as i32
}

/// Legacy kind strings (`cell_found_parts`) <-> unlock types.
fn kind_unlock(kind: &str) -> Option<UnlockType> {
    Some(match kind {
        "mandible" => UnlockType::Mandible,
        "filter" => UnlockType::Filter,
        "proboscis" => UnlockType::Proboscis,
        "poison" => UnlockType::Chemical,
        "electric" => UnlockType::Electric,
        "spike" => UnlockType::Spike,
        "cilia" => UnlockType::Cilia,
        "flagella" => UnlockType::Flagella,
        "jet" => UnlockType::Jet,
        _ => return None,
    })
}

/// Bring an older save into the retail model (once): DNA progress -> food,
/// found part kinds -> unlocked parts, a population seed.
pub fn migrate_save(run: &mut RunProgress) {
    if run.cell.migrated {
        return;
    }
    if run.cell.food_progression == 0 && run.dna_this_stage > 0.0 {
        run.cell.food_progression =
            ((run.dna_this_stage * 10.0) as i32).clamp(0, crate::cell_engine::STAGE_FOOD);
    }
    for kind in run.cell_found_parts.clone() {
        if let Some(unlock) = kind_unlock(&kind) {
            run.cell.unlocked_parts[unlock.raw() as usize] = 2;
        }
    }
    if run.cell.game_id == 0 {
        run.cell.game_id = 0xCE11;
    }
    run.cell.migrated = true;
}

/// RecordUnlock's bookkeeping (0xE5D7B0): mUnlockedParts[type] = 2 and the
/// legacy editor gate kind. Returns false when already unlocked.
pub fn unlock(run: &mut RunProgress, unlock: UnlockType) -> bool {
    let slot = unlock.raw() as usize;
    if slot >= 13 || slot == 0 || run.cell.unlocked_parts[slot] > 0 {
        return false;
    }
    run.cell.unlocked_parts[slot] = 2;
    // 0xE51560: `clg_discoverpart` with the part in the record.
    run.cell
        .record_event(crate::cell_hud::timeline::events::EventKind::Part(slot as u8));
    if let Some(kind) = crate::cell_engine::unlock_kind(unlock) {
        if !run.cell_found_parts.iter().any(|k| k == kind) {
            run.cell_found_parts.push(kind.to_string());
        }
    }
    run.request_save();
    true
}

/// `GetDamageAmount` (0xE58980, RE 2A). Scale differences are against the
/// player's current scale, as retail computes them.
pub fn damage_amount(
    attacker_is_player: bool,
    attacker: CellScale,
    victim_is_player: bool,
    victim: CellScale,
    player: CellScale,
    difficulty: usize,
) -> i32 {
    const NPC: [[i32; 5]; 5] = [
        [0, 0, 0, 0, 0],
        [6, 2, 1, 0, 0],
        [6, 6, 2, 0, 0],
        [6, 6, 6, 2, 0],
        [6, 6, 6, 6, 2],
    ];
    if attacker_is_player {
        match scale_diff(victim, player) {
            0 | 1 => 6,
            2 => {
                if difficulty != 2 {
                    3
                } else {
                    2
                }
            }
            _ => 1,
        }
    } else if !victim_is_player {
        NPC[scale_diff(attacker, player)][scale_diff(victim, player)]
    } else {
        match scale_diff(attacker, player) {
            0 | 1 => 1,
            2 => 2,
            3 => 3,
            _ => 6,
        }
    }
}

/// `ShouldNotAttack` (0xE57460, RE 2A): true when `a` must not attack `b`.
/// `*_dead` is the corpse byte +0x112 (anything may attack a corpse).
#[allow(clippy::too_many_arguments)]
pub fn should_not_attack(
    db: &CellDatabase,
    a_cell: u32,
    a_scale: CellScale,
    a_dead: bool,
    b_cell: u32,
    b_avatar: bool,
    b_dead: bool,
    player: CellScale,
) -> bool {
    if a_cell == b_cell {
        return true;
    }
    if a_dead || b_dead {
        return false;
    }
    let (Some(ra), Some(rb)) = (db.cell(a_cell), db.cell(b_cell)) else {
        return false;
    };
    if ra.wont_attack_player_when_small && b_avatar && scale_diff(a_scale, player) == 1 {
        return true;
    }
    if ra.wont_attack_player && b_avatar {
        return true;
    }
    ra.friend_group != 0 && rb.friend_group == ra.friend_group
}

/// How a cell left the world (picks its death effect and loot).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeathKind {
    #[default]
    None,
    /// Damage to 0 hp (0xE7A4A0).
    Killed,
    /// Bitten apart (EatKill 0xE7AEE0).
    Broken,
    /// Swallowed whole (0xE7D880).
    Eaten,
    /// A bubble popped (0xE791A0).
    Popped,
    /// A pickup collected.
    Collected,
}

/// Combat bookkeeping on every cell.
#[derive(Component, Clone, Debug, Default)]
pub struct CellCombat {
    /// "Already hit" window (+0x17C; the same time holds the cell's own actions,
    /// +0x178): the victim's reaction clip.
    pub hit_cooldown: f32,
    /// The cell is inside an attack of its own (+0x17F, +0x178): its bite or
    /// stab clip.
    pub busy: f32,
    pub last_attacker: Option<Entity>,
    pub since_hurt: f32,
    pub killed_by_player: bool,
    pub bitten: bool,
    /// Game time of this cloud's last poison hit (cloud+0x144).
    pub poison_hit: Option<f32>,
    pub death: DeathKind,
    /// Owner of a weapon cloud (+0x10C).
    pub owner: Option<Entity>,
    /// Poison emit accumulator (+0x374).
    pub poison_acc: f32,
    /// Electric cooldown (+0x158); the cell is charged when it is 0.
    pub electric_cooldown: f32,
    /// A spike may stab a plant again when this runs out (the clip of request
    /// 0x1C, `PUNCTURE_SECONDS` without clips).
    pub puncture_cooldown: f32,
    /// Seconds until a zapped cell whose hit points reached 0 dies: retail's event 2
    /// (the target's reaction clip) ends with 0xE7A770, which runs the death
    /// routine 0xE7A4A0 when the hit points are 0 (the zap itself only subtracts).
    pub pending_death: Option<f32>,
    /// The `cell_death_eaten` effect already played at the swallow (0xE7D880 instances
    /// it before the victim's own death clip): `resolve_deaths` must not repeat it.
    pub death_fx_played: bool,
}

/// Parts a contact can involve (UnlockType numbering; 12 = the body sphere).
pub const PART_BODY: u8 = 12;

/// One side of a contact, as `Classify` sees it.
#[derive(Clone, Copy, Debug)]
pub struct Side {
    pub entity: Entity,
    pub cell_type: CellType,
    pub kind: CellKind,
    pub avatar: bool,
    pub dead: bool,
    pub flags: i32,
    pub living: bool,
    /// `ScaleDiff` against the player's level.
    pub scale_diff: usize,
    pub scale: CellScale,
    pub eggkid: bool,
    pub caps: CellCaps,
}

/// `Living` (0xE52940): has AI (or is the avatar), hp left, not dead, solid.
pub fn living(object: &CellObject, ai: AiType, cell_type: CellType) -> bool {
    (ai != AiType::Player || object.kind == CellKind::Player)
        && object.hp > 0
        && !object.dead
        && cell_type == CellType::Solid
}

/// `Classify` (0xE6A3F0) + `BodyContact` (0xE6A250): the interaction kind of
/// part `pa` of `a` touching part `pb` of `b`; `rel` = clamp(scale(b) -
/// scale(a) + 2, 0, 4); `immune` = a owns the cloud or cannot be poisoned.
pub fn classify(a: &Side, pa: u8, b: &Side, pb: u8, rel: usize, immune: bool) -> u8 {
    if pa == 9 || pb == 9 {
        return 0;
    }
    if b.cell_type == CellType::Nanite {
        return if a.avatar { 0x13 } else { 0 };
    }
    if a.cell_type == CellType::Nanite {
        return 0;
    }
    if b.cell_type == CellType::Unlock {
        return if a.avatar { 0x12 } else { 1 };
    }
    let mut kind = 0u8;
    if b.cell_type == CellType::Air {
        kind = if a.cell_type == CellType::Air {
            2
        } else if rel <= 2 && (b.flags != 2 || a.avatar) {
            3
        } else {
            1
        };
    } else {
        match pa {
            3 => {
                if b.cell_type == CellType::Liquid && rel == 1 && !a.dead {
                    kind = 9;
                }
            }
            2 => {
                if b.cell_type == CellType::Solid {
                    kind = match rel {
                        0 if !a.dead && !b.dead => 10,
                        1 if !a.dead && !b.dead => {
                            if b.avatar {
                                7
                            } else {
                                10
                            }
                        }
                        2 => {
                            if pb == 2 {
                                0xF
                            } else if b.scale_diff < 2 && !a.living {
                                1
                            } else {
                                7
                            }
                        }
                        3 if !a.dead => {
                            if a.eggkid {
                                7
                            } else {
                                4
                            }
                        }
                        _ => 0,
                    };
                } else if b.flags & 1 != 0 && rel == 1 && !b.avatar && !a.dead {
                    kind = 10;
                }
            }
            7 => {
                if b.flags & 1 != 0 && rel <= 1 {
                    kind = 8;
                } else if b.cell_type == CellType::Solid {
                    kind = match rel {
                        2 => {
                            if pb == 7 {
                                0xE
                            } else {
                                8
                            }
                        }
                        1 if !a.dead && !b.dead && b.scale_diff >= 2 => 8,
                        3 => {
                            if pb == 2 || b.dead {
                                5
                            } else {
                                8
                            }
                        }
                        _ => 0,
                    };
                } else if b.cell_type == CellType::Liquid {
                    kind = match rel {
                        2 if b.scale_diff >= 2 => 6,
                        4 => 5,
                        _ => 0,
                    };
                }
            }
            4 => {
                if b.cell_type == CellType::Liquid {
                    kind = match rel {
                        1 => 0xB,
                        3 => 4,
                        _ => 0,
                    };
                } else if b.cell_type == CellType::Solid {
                    kind = if (rel == 1 || rel == 2) && (b.living || b.avatar) && !b.dead {
                        if pb == 4 {
                            0x10
                        } else {
                            0xC
                        }
                    } else if rel == 3 {
                        4
                    } else {
                        0
                    };
                }
            }
            _ => {}
        }
    }
    // BodyContact: poison clouds, else a plain contact.
    let body = if b.cell_type == CellType::Poison && !immune {
        if rel != 0 {
            0x11
        } else {
            0x16
        }
    } else {
        1
    };
    kind.max(body)
}

/// The attack handlers of ExecuteInteraction (0xE7E7F0) refuse friends: swallow
/// (kind 9 inline at 0xE7ED42, kind 10 -> 0xE7E020), bite (7 -> 0xE7B470), stab
/// (8 -> 0xE72360) and the proboscis drain (0xC -> 0xE727E0, 0xD -> 0xE725C0)
/// all call ShouldNotAttack(attacker, victim) (same cell file, same friendGroup,
/// a small cell that won't attack the avatar) and fall through to the plain
/// contact response; the swallows also leave an invulnerable victim alone
/// (+0x111). VERIFIED. True when `kind` (actor `x` against `y`) must become a
/// plain contact (kind 1).
#[allow(clippy::too_many_arguments)]
pub fn refuses_attack(
    db: &CellDatabase,
    kind: u8,
    x: &CellObject,
    x_dead: bool,
    y: &CellObject,
    y_avatar: bool,
    y_dead: bool,
    player: CellScale,
) -> bool {
    if !matches!(kind, 7 | 8 | 9 | 10 | 0xC | 0xD) {
        return false;
    }
    should_not_attack(db, x.cell, x.scale, x_dead, y.cell, y_avatar, y_dead, player)
        || (matches!(kind, 9 | 10) && y.invulnerable > 0.0)
}

/// One contact of the frame (for the impulse solver).
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    /// Penetration depth, retail units.
    pub depth: f32,
}

/// Contacts found this frame (solved by `cell_motion::locomote`).
#[derive(Resource, Default)]
pub struct CellContacts {
    pub pairs: Vec<Contact>,
}

/// Parts of a modelled creature whose collision spheres touch the circle
/// (`other`, `other_radius`) (0xE73390, RE 11.6): the part centre is the
/// cell position plus its fitted-frame centre turned by the heading and
/// scaled by the cell radius; the body sphere always counts.
pub fn sphere_parts(
    spheres: &[crate::cell_engine::PartSphere],
    pos: Vec2,
    heading: f32,
    radius: f32,
    other: Vec2,
    other_radius: f32,
) -> Vec<u8> {
    sphere_contacts(spheres, pos, heading, radius, other, other_radius)
        .into_iter()
        .map(|(kind, ..)| kind)
        .collect()
}

/// `sphere_parts` with the sphere itself: (part type, world centre, world
/// radius) of the body sphere and of the first touching sphere of every part
/// type.
pub fn sphere_contacts(
    spheres: &[crate::cell_engine::PartSphere],
    pos: Vec2,
    heading: f32,
    radius: f32,
    other: Vec2,
    other_radius: f32,
) -> Vec<(u8, Vec2, f32)> {
    let mut parts = vec![(PART_BODY, pos, radius)];
    let rot = Quat::from_rotation_y(heading);
    for s in spheres {
        if s.kind == 1 || s.kind == PART_BODY {
            continue;
        }
        let local = rot * Vec3::new(s.center.x, 0.0, s.center.y) * radius;
        let c = pos + Vec2::new(local.x, local.z);
        if c.distance(other) < s.radius * radius + other_radius && !parts.iter().any(|(k, ..)| *k == s.kind) {
            parts.push((s.kind, c, s.radius * radius));
        }
    }
    parts
}

/// Contact point of sphere P (a part or a body) against sphere B (0xE73390):
/// the midpoint of the two surfaces, `(cB + cP + n (rP - rB)) / 2` with
/// n = (cB - cP) / d; spheres at the same point meet at their centre.
pub fn touch_point(c_p: Vec2, r_p: f32, c_b: Vec2, r_b: f32) -> Vec2 {
    let d = c_b - c_p;
    let len = d.length();
    if len < 1.0 / 65536.0 {
        return (c_p + c_b) * 0.5;
    }
    (c_b + c_p + d / len * (r_p - r_b)) * 0.5
}

/// Parts of `side` that reach a contact in direction `toward` (Bevy units;
/// within `inside` of the centre the other cell is inside the body and
/// every part reaches it).
fn parts_toward(side: &Side, facing: Vec2, toward: Vec2, inside: f32) -> Vec<u8> {
    let mut parts = vec![PART_BODY];
    if !side.kind.is_creature() {
        return parts;
    }
    let front = toward.length() < inside || facing.dot(toward.normalize_or_zero()) >= MOUTH_CONE;
    let n = side.caps.n;
    if front {
        for t in [4u8, 2, 3] {
            if n[t as usize] > 0 {
                parts.push(t);
            }
        }
    }
    if n[7] > 0 {
        parts.push(7);
    }
    parts
}

fn main_part(side: &Side, facing: Vec2, toward: Vec2, inside: f32) -> u8 {
    parts_toward(side, facing, toward, inside)
        .into_iter()
        .filter(|p| *p != PART_BODY)
        .max_by_key(|p| match p {
            2 | 4 => 3,
            7 => 2,
            _ => 1,
        })
        .unwrap_or(PART_BODY)
}

impl CellCombat {
    /// +0x178: the cell may not start a swallow, bite or drain (it is in a hit
    /// reaction or in an attack of its own).
    pub fn acting_locked(&self) -> bool {
        self.hit_cooldown > 0.0 || self.busy > 0.0
    }
}

/// What `PlayAnimation` (0xE6D200) answers a rule that times an event with the
/// clip: the clip length x G(anim_dilation, realSize[cell] / realSize[player])
/// of a modelled cell (1.0 for one without a model, 0xE6D208); `None` before
/// the clip table is loaded (the caller keeps its fallback constant).
#[derive(Clone, Copy)]
pub struct PlayClock<'a> {
    pub clips: &'a CellClipTimes,
    pub art: &'a CellArtCache,
    pub player: CellScale,
}

impl PlayClock<'_> {
    /// `parts` is the part mask of the cell for the rows that need one (the
    /// eat clips: the mouth).
    pub fn seconds(&self, o: &CellObject, code: i32, parts: u32) -> Option<f32> {
        if !self.clips.is_loaded() {
            return None;
        }
        let avatar = o.kind == CellKind::Player;
        if !avatar && !self.art.art.get(&o.cell).is_some_and(|a| a.modelled) {
            return Some(1.0);
        }
        let (ctx, dilation) = self.context(o, parts);
        self.clips.play_seconds(code, &ctx, dilation)
    }

    /// When the clip of `code` posts its `contact` event on this cell (the
    /// callback of a timed event runs then, see `CellClipTimes::contact_seconds`);
    /// `None` for a cell without a model or a clip without the marker.
    pub fn contact(&self, o: &CellObject, code: i32, parts: u32) -> Option<f32> {
        if o.kind != CellKind::Player && !self.art.art.get(&o.cell).is_some_and(|a| a.modelled) {
            return None;
        }
        let (ctx, dilation) = self.context(o, parts);
        self.clips.contact_seconds(code, &ctx, dilation)
    }

    fn context(&self, o: &CellObject, parts: u32) -> (SelectContext, f32) {
        let avatar = o.kind == CellKind::Player;
        let ctx = SelectContext {
            avatar,
            parts,
            big: o.radius / UNIT > 2.0,
            epic: scale_diff(o.scale, self.player) == 4,
            background: o.background,
        };
        (ctx, dilation_factor((!avatar).then_some(o.scale), self.player))
    }
}

/// A bite in progress (event 4, 0xE7B470): it lands when the attacker's bite
/// clip posts `contact` (callback 0xE7B280).
#[derive(Component, Clone, Copy, Debug)]
pub struct CellBiting {
    pub victim: Entity,
    /// Seconds to the callback (the bite) or, for a drain, to the end of event 0x13.
    pub timer: f32,
    /// A proboscis drain (event 0x13, 0xE727E0) instead of a bite.
    pub drain: bool,
    /// Where the spheres touched when the bite started (the event's +0x54): the
    /// torn-off chunk of the landed bite appears there.
    pub point: Vec2,
    /// The drainer's mouth part (0xE69C10's part argument) in the drainer's frame:
    /// (along its facing, to its left), Bevy plane units. The contact's part sphere
    /// centre; the body centre without part data.
    pub mouth_local: Vec2,
    /// The mouth part's outward axis (0xE67D30) in the drainer's local plane (the
    /// fitted frame, before its heading and radius).
    pub mouth_axis: Vec2,
    /// Seconds left of the pull (event sub-timer 0); 0 once the victim is attached.
    pub pull: f32,
}

fn play(anim: &mut Option<Mut<CellAnimState>>, code: i32, slot: CellAnimation) {
    if let Some(anim) = anim {
        request_anim(anim, code, slot);
    }
}

/// What a contact does, applied after classification.
#[derive(Clone, Copy, Debug)]
enum Action {
    /// Swallow whole (`mouth`: 3 filter, 2 mandible, 4 proboscis).
    Eat { eater: Entity, food: Entity, mouth: u8 },
    /// `point` = where the spheres touch (0xE73390). The bite starts (0xE7B470).
    Bite { attacker: Entity, victim: Entity, mouth: u8, point: Vec2, part_at: Vec2 },
    /// The bite clip posted `contact`: the callback 0xE7B280 resolves the bite
    /// (the tear-off chunk appears at `point`, where the bite started).
    BiteLand { attacker: Entity, victim: Entity, point: Vec2 },
    /// Event 0x13 ended (finaliser 0xE7DD90): the drainer's meal and the victim's loss;
    /// `axis` is the drainer's mouth axis in its local plane (`CellBiting::mouth_axis`).
    DrainEnd { attacker: Entity, victim: Entity, axis: Vec2 },
    Stab { attacker: Entity, victim: Entity },
    /// Kind 6 (0xE777A0): a spike against a plant tears off its leak cell.
    Leak { attacker: Entity, plant: Entity, point: Vec2 },
    Pop { bubble: Entity },
    Collect { player: Entity, pickup: Entity },
    Poison { victim: Entity, cloud: Entity },
    Push { a: Entity, b: Entity },
    React { cell: Entity, code: i32, slot: CellAnimation },
}

type Objects<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static mut CellObject,
        Option<&'static mut CellCombat>,
        Option<&'static mut CellAnimState>,
        Option<&'static mut CellMotion>,
        Option<&'static mut crate::cell_ai::CellAiState>,
    ),
>;

/// Resources the contact rules read and write.
#[derive(SystemParam)]
pub struct RulesParams<'w> {
    pub time: Res<'w, Time>,
    pub db: Res<'w, CellDatabase>,
    pub grid: Res<'w, CellGrid>,
    pub art: Res<'w, CellArtCache>,
    pub originals: Res<'w, crate::Originals>,
    pub game: ResMut<'w, CellGame>,
    pub run: ResMut<'w, RunProgress>,
    pub notice: ResMut<'w, crate::cell::CellNotice>,
    pub contacts: ResMut<'w, CellContacts>,
    pub queue: ResMut<'w, CellSpawnQueue>,
    pub flow: ResMut<'w, crate::cell_flow::CellFlow>,
    pub clips: Res<'w, CellClipTimes>,
}

/// Detect contacts, classify them (one per pair) and run their actions.
pub fn interactions(
    mut commands: Commands,
    mut p: RulesParams,
    mut effects: MessageWriter<CellEffectRequest>,
    mut objects: Objects,
    mut biting: Query<(Entity, &mut CellBiting)>,
    poses: Query<&crate::cell_anim::CellPartPoses>,
) {
    let dt = p.time.delta_secs().min(0.1);
    p.contacts.pairs.clear();
    let Some(globals) = p.db.globals.clone() else {
        return;
    };
    let db = &*p.db;
    let player_scale = crate::cell_engine::player_scale(p.run.cell.food_progression);
    let player_caps = crate::cell_motion::player_caps(&p.run, &p.originals, db);
    let difficulty = p.game.difficulty;
    let mut expired: Vec<Entity> = Vec::new();
    for (e, _, _, combat, ..) in &mut objects {
        if let Some(mut c) = combat {
            c.hit_cooldown = (c.hit_cooldown - dt).max(0.0);
            c.busy = (c.busy - dt).max(0.0);
            c.puncture_cooldown = (c.puncture_cooldown - dt).max(0.0);
            c.since_hurt += dt;
            if let Some(t) = c.pending_death {
                let t = t - dt;
                c.pending_death = if t <= 0.0 {
                    expired.push(e);
                    None
                } else {
                    Some(t)
                };
            }
        }
    }
    // The end of a zapped cell's reaction clip (event 2's finaliser 0xE7A770): a cell
    // whose hit points reached 0 dies now (0xE7A4A0: the kill count, the friends'
    // fear). The avatar is taken by `resolve_deaths` once its pending death is over.
    for victim in expired {
        let (by_player, attacker) = {
            let Ok((_, _, mut o, combat, ..)) = objects.get_mut(victim) else {
                continue;
            };
            if o.dead || o.kind == CellKind::Player {
                continue;
            }
            o.dead = true;
            combat.map_or((false, None), |c| (c.killed_by_player, c.last_attacker))
        };
        if by_player {
            p.run.cell.count_kill();
        }
        if let Some(attacker) = attacker {
            friend_fear(db, &mut objects, victim, attacker, player_scale, difficulty, p.game.world_scale, FriendFear::Death);
        }
    }
    let art = &*p.art;
    let clock = PlayClock { clips: &p.clips, art: &p.art, player: player_scale };
    // The avatar's (and its species') part list: 0xE83E30 of its creation.
    let player_spheres = crate::cell_motion::player_part_spheres(&p.run, &p.originals);
    let side_of = |e: Entity, o: &CellObject| -> Side {
        let record = db.cell(o.cell);
        let caps = if o.kind == CellKind::Player || o.kind == CellKind::OwnSpecies {
            player_caps
        } else {
            art.art
                .get(&o.cell)
                .map(|a| CellCaps::from_counts(a.counts, &db.graphs, &globals))
                .unwrap_or_default()
        };
        let cell_type = record.map_or(CellType::None, |r| r.cell_type);
        let ai = record.map_or(AiType::Player, |r| crate::cell_ai::ai_block(r, difficulty).kind);
        Side {
            entity: e,
            cell_type,
            kind: o.kind,
            avatar: o.kind == CellKind::Player,
            dead: o.dead,
            flags: record.map_or(0, |r| r.flags),
            living: living(o, ai, cell_type),
            scale_diff: scale_diff(o.scale, player_scale),
            scale: o.scale,
            eggkid: record.is_some_and(|r| r.name.starts_with("cell_Eggkid")),
            caps,
        }
    };
    // Broad phase: overlapping bounding spheres (1D).
    let max_radius = p.grid.items.iter().map(|i| i.radius).fold(0.0, f32::max);
    let mut pairs = Vec::new();
    for item in &p.grid.items {
        p.grid.query(item.position, item.radius + max_radius, |other| {
            if other.entity > item.entity && other.position.distance(item.position) < item.radius + other.radius {
                pairs.push((item.entity, other.entity));
            }
        });
    }
    let poison_player = crate::world_objects::name_id("cell_weapon_poison_player");
    let mut actions: Vec<Action> = Vec::new();
    for (a, b) in pairs {
        let Ok([(_, ta, oa, ca, ..), (_, tb, ob, cb, ..)]) = objects.get_many([a, b]) else {
            continue;
        };
        if oa.dead || ob.dead || oa.passive || ob.passive {
            continue;
        }
        let (owner_a, owner_b) = (ca.and_then(|c| c.owner), cb.and_then(|c| c.owner));
        let (Some(ra), Some(rb)) = (db.cell(oa.cell), db.cell(ob.cell)) else {
            continue;
        };
        if ra.cell_type == CellType::None || rb.cell_type == CellType::None {
            continue;
        }
        let (pa, pb) = (oa.plane(ta), ob.plane(tb));
        let depth = ((oa.radius + ob.radius) - pa.distance(pb)) / UNIT;
        let (sa, sb) = (side_of(a, &oa), side_of(b, &ob));
        let (fa, fb) = (heading_dir(oa.heading), heading_dir(ob.heading));
        // Best interaction over both directions: one per pair (the higher
        // kind; the avatar's stab wins a tie).
        let mut best: Option<(u8, bool, Vec2, Vec2)> = None;
        for (actor_is_a, x, y, fx, fy, px, py, ox, oy, y_owner) in [
            (true, &sa, &sb, fa, fb, pa, pb, &oa, &ob, owner_b),
            (false, &sb, &sa, fb, fa, pb, pa, &ob, &oa, owner_a),
        ] {
            let rel = (y.scale.index() - x.scale.index() + 2).clamp(0, 4) as usize;
            // BodyContact: a cloud never hurts its owner (+0x10C) or another
            // cloud.
            let immune = y.cell_type == CellType::Poison
                && (x.cell_type == CellType::Poison || y_owner == Some(x.entity) || (oy.cell == poison_player && x.avatar));
            // Exact part spheres (11.6): the NPC's rigblocks, the avatar's and
            // its species' placed parts (`player_part_spheres`, the part models' boxes).
            // Without any part data (a headless run) the direction stand-in
            // (`parts_toward`) is used.
            let spheres = |o: &CellObject| -> Option<&[crate::cell_engine::PartSphere]> {
                match o.kind {
                    CellKind::Creature => art.art.get(&o.cell).map(|a| a.spheres.as_slice()).filter(|s| !s.is_empty()),
                    CellKind::Player | CellKind::OwnSpecies => Some(player_spheres.as_slice()).filter(|s| !s.is_empty()),
                    _ => None,
                }
            };
            let parts_x: Vec<(u8, Vec2, f32)> = match spheres(&**ox) {
                Some(sp) => sphere_contacts(sp, px, ox.heading, ox.radius, py, oy.radius),
                None => parts_toward(x, fx, py - px, ox.radius * 0.35)
                    .into_iter()
                    .map(|part| (part, px, ox.radius))
                    .collect(),
            };
            let target_part = match spheres(&**oy) {
                Some(sp) => sphere_parts(sp, py, oy.heading, oy.radius, px, ox.radius)
                    .into_iter()
                    .filter(|p| *p != PART_BODY)
                    .max_by_key(|p| match p {
                        2 | 4 => 3,
                        7 => 2,
                        _ => 1,
                    })
                    .unwrap_or(PART_BODY),
                None => main_part(y, fy, px - py, oy.radius * 0.35),
            };
            for (part, centre, part_radius) in parts_x {
                let kind = classify(x, part, y, target_part, rel, immune);
                let better = match best {
                    None => kind > 0,
                    Some((k, ..)) => kind > k || (kind == k && kind == 8 && x.avatar),
                };
                if better {
                    best = Some((kind, actor_is_a, touch_point(centre, part_radius, py, oy.radius), centre));
                }
            }
        }
        let Some((kind, actor_is_a, point, part_at)) = best else {
            continue;
        };
        let (x, y) = if actor_is_a { (a, b) } else { (b, a) };
        let (xs, ys) = if actor_is_a { (&sa, &sb) } else { (&sb, &sa) };
        let kind = {
            let (ox, oy) = if actor_is_a { (&oa, &ob) } else { (&ob, &oa) };
            if refuses_attack(db, kind, ox, xs.dead, oy, ys.avatar, ys.dead, player_scale) {
                1
            } else {
                kind
            }
        };
        if !matches!(kind, 3 | 9 | 10 | 0xB | 0x11 | 0x12 | 0x13 | 0x16) {
            p.contacts.pairs.push(Contact { a, b, depth: depth.max(0.0) });
        }
        let mutual = |code: i32, slot: CellAnimation, actions: &mut Vec<Action>| {
            // Mutual reactions (0xE70A90) only when they may attack.
            let (ox, oy) = if actor_is_a { (&oa, &ob) } else { (&ob, &oa) };
            if !should_not_attack(db, ox.cell, ox.scale, xs.dead, oy.cell, ys.avatar, ys.dead, player_scale) {
                actions.push(Action::React { cell: x, code, slot });
                actions.push(Action::React { cell: y, code, slot });
            }
        };
        match kind {
            2 => actions.push(Action::Push { a: x, b: y }),
            3 => actions.push(Action::Pop { bubble: y }),
            4 => actions.push(Action::React {
                cell: x,
                code: anim_code::GLANCE_MAD,
                slot: CellAnimation::CellGlanceMad,
            }),
            5 => actions.push(Action::React {
                cell: x,
                code: anim_code::GLANCE_SPIKE,
                slot: CellAnimation::CellGlanceSpike,
            }),
            6 => actions.push(Action::Leak { attacker: x, plant: y, point }),
            7 => actions.push(Action::Bite { attacker: x, victim: y, mouth: 2, point, part_at }),
            8 => actions.push(Action::Stab { attacker: x, victim: y }),
            9 => actions.push(Action::Eat { eater: x, food: y, mouth: 3 }),
            10 => actions.push(Action::Eat { eater: x, food: y, mouth: 2 }),
            0xB => actions.push(Action::Eat { eater: x, food: y, mouth: 4 }),
            0xC => actions.push(Action::Bite { attacker: x, victim: y, mouth: 4, point, part_at }),
            0xE => mutual(anim_code::CLASH_SPIKE, CellAnimation::CellGlanceSpikeOnSpike, &mut actions),
            0xF => mutual(anim_code::CLASH_JAW, CellAnimation::CellGlanceMad2, &mut actions),
            0x10 => mutual(anim_code::CLASH_PROBOSCIS, CellAnimation::CellGlanceProbOnProb, &mut actions),
            0x11 => actions.push(Action::Poison { victim: x, cloud: y }),
            0x12 => actions.push(Action::Collect { player: x, pickup: y }),
            _ => {}
        }
    }
    // Bites whose clip ended land now (event 4's callback).
    for (attacker, mut bite) in &mut biting {
        bite.timer -= dt;
        if bite.timer <= 0.0 {
            actions.push(if bite.drain {
                Action::DrainEnd { attacker, victim: bite.victim, axis: bite.mouth_axis }
            } else {
                Action::BiteLand { attacker, victim: bite.victim, point: bite.point }
            });
            commands.entity(attacker).remove::<CellBiting>();
        }
    }
    // Run the actions (a bite without clip data lands in the same pass).
    let now = p.game.time;
    let food_before = p.run.cell.food_progression;
    let mut next = 0;
    while next < actions.len() {
        let action = actions[next];
        next += 1;
        match action {
            Action::Push { a, b } => {
                // Kind 2: push accelerations +-3 * min(mass) (1D).
                if let Ok([(_, ta, oa, _, _, ma, _), (_, tb, ob, _, _, mb, _)]) = objects.get_many_mut([a, b]) {
                    let n = (ob.plane(tb) - oa.plane(ta)).normalize_or_zero();
                    let m = oa.mass.min(ob.mass);
                    if let Some(mut ma) = ma {
                        ma.accel -= n * 3.0 * m / oa.mass.max(1e-6);
                    }
                    if let Some(mut mb) = mb {
                        mb.accel += n * 3.0 * m / ob.mass.max(1e-6);
                    }
                }
            }
            Action::Pop { bubble } => {
                if let Ok((_, _, mut o, combat, mut anim, ..)) = objects.get_mut(bubble) {
                    if !o.dead {
                        o.dead = true;
                        if let Some(mut c) = combat {
                            c.death = DeathKind::Popped;
                        }
                        play(&mut anim, anim_code::BUBBLE_POP, CellAnimation::CellBubblePop);
                    }
                }
            }
            Action::React { cell, code, slot } => {
                if let Ok((_, _, _, _, mut anim, ..)) = objects.get_mut(cell) {
                    play(&mut anim, code, slot);
                }
            }
            Action::Collect { player, pickup } => {
                let Ok((_, _, mut o, combat, ..)) = objects.get_mut(pickup) else {
                    continue;
                };
                if o.dead {
                    continue;
                }
                o.dead = true;
                if let Some(mut c) = combat {
                    c.death = DeathKind::Collected;
                }
                let unlock_type = db.cell(o.cell).map_or(UnlockType::None, |r| r.unlock_type);
                if record_unlock(&mut p.run, &mut p.flow, unlock_type) {
                    let name = crate::cell::kind_name(crate::cell_engine::unlock_kind(unlock_type).unwrap_or("part"));
                    p.notice.show(format!("New part found: {name}! Call a mate to add it in the Cell Creator."));
                }
                // Pickup anim request (its effects come from animation events).
                if let Ok((_, _, _, _, mut anim, ..)) = objects.get_mut(player) {
                    play(&mut anim, anim_code::PART_PICKUP, CellAnimation::CellPartDiscover);
                }
            }
            Action::Poison { victim, cloud } => {
                let fresh = match objects.get_mut(cloud) {
                    Ok((_, _, _, Some(mut c), ..)) => {
                        let fresh = c.poison_hit.is_none_or(|t| now - t >= POISON_PERIOD);
                        if fresh {
                            c.poison_hit = Some(now);
                        }
                        fresh
                    }
                    _ => true,
                };
                if fresh {
                    let _ = damage(
                        db,
                        &mut objects,
                        &clock,
                        cloud,
                        victim,
                        DAMAGE_POISON,
                        player_scale,
                        difficulty,
                        p.game.world_scale,
                        &mut p.run,
                    );
                }
            }
            Action::Leak { attacker, plant, point } => {
                // 0xE777A0: refused while the attacker is locked (+0x17F: inside
                // an attack of its own, or the previous puncture's clip); a plant
                // without a `leak` cell (record +0xCC) does nothing. The attacker
                // plays request 0x1C and event 0xE is queued.
                let Ok([(_, _, oa, ca, ..), (_, tp, op, ..)]) = objects.get_many([attacker, plant]) else {
                    continue;
                };
                let leak = db.cell(op.cell).map_or(0, |r| r.leak);
                if leak == 0 || ca.as_ref().is_some_and(|c| c.puncture_cooldown > 0.0 || c.busy > 0.0) {
                    continue;
                }
                // The attacker's scale, or the player's when it has none (0xE777EB).
                let scale = if oa.scale.is_none() { player_scale } else { oa.scale };
                // The attacker stays locked for the clip of request 0x1C
                // (0xE593B0); `PUNCTURE_SECONDS` while the clip table is not loaded.
                let hold = clock.seconds(&oa, anim_code::PUNCTURE_SPIKE, 0).unwrap_or(PUNCTURE_SECONDS);
                let request = leak_request(leak, scale, point, plant, op.plane(tp), op.heading, op.radius);
                p.queue.0.push(request);
                if let Ok((_, _, _, Some(mut c), mut anim, ..)) = objects.get_mut(attacker) {
                    c.puncture_cooldown = hold;
                    play(&mut anim, anim_code::PUNCTURE_SPIKE, CellAnimation::CellPunctureSpike);
                }
            }
            Action::Stab { attacker, victim } => {
                // 0xE72360: a cell inside an attack of its own (+0x17F) stabs
                // nothing; the stab clip (request 4) is event 10's time and the
                // damage is its first sub-timer (0.001 s), so it lands at once.
                let seconds = match objects.get(attacker) {
                    Ok((_, _, oa, ca, ..)) => {
                        if ca.is_some_and(|c| c.busy > 0.0) {
                            continue;
                        }
                        clock.seconds(&oa, anim_code::STAB_SOLID, 0)
                    }
                    Err(_) => continue,
                };
                if let Ok((_, _, _, combat, mut anim, ..)) = objects.get_mut(attacker) {
                    play(&mut anim, anim_code::STAB_SOLID, CellAnimation::CellDmgSolidSpike);
                    if let (Some(mut c), Some(seconds)) = (combat, seconds) {
                        c.busy = seconds;
                    }
                }
                let _ = damage(
                    db,
                    &mut objects,
                    &clock,
                    attacker,
                    victim,
                    DAMAGE_STAB,
                    player_scale,
                    difficulty,
                    p.game.world_scale,
                    &mut p.run,
                );
            }
            Action::Bite { attacker, victim, mouth, point, part_at } => {
                // The bite starts (0xE7B470): no bite on a victim still inside its
                // hit reaction (+0x17C) or by an attacker that is locked (+0x178 /
                // +0x17F). The attacker plays request 5 and the bite lands when
                // that clip posts `contact` (event 4, callback 0xE7B280), at the
                // contact point of now.
                // The proboscis drain (mouth 4, 0xE727E0) is event 0x13: refused while the
                // attacker (+0x179 / +0x17B) or the victim (+0x178) is locked; the attacker
                // plays request 13 and the victim request 0x20 (cell_eatprob_stun); the
                // event lasts the attacker's whole clip, both cells held, and its finaliser
                // (0xE7DD90, `Action::DrainEnd`) is the meal and the victim's loss.
                let Ok([(_, ta, oa, ca, ..), (_, _, ob, cb, ..)]) = objects.get_many([attacker, victim]) else {
                    continue;
                };
                let drain = mouth == 4;
                let mouth_local = {
                    let (f, d) = (heading_dir(oa.heading), part_at - oa.plane(ta));
                    Vec2::new(d.dot(f), d.dot(Vec2::new(-f.y, f.x)))
                };
                // The mouth part's axis (0xE67D30, read at the finaliser): the axis of the
                // proboscis sphere the contact found (the NPC's animated pose when it has
                // one), else outward from the cell's centre through the contact.
                let mouth_axis = {
                    let rot = Quat::from_rotation_y(oa.heading);
                    let at = oa.plane(ta);
                    let world = |c: Vec2| {
                        let w = rot * Vec3::new(c.x, 0.0, c.y) * oa.radius;
                        at + Vec2::new(w.x, w.z)
                    };
                    let candidates: Vec<(Vec2, Vec2)> = if oa.kind == CellKind::Player || oa.kind == CellKind::OwnSpecies {
                        player_spheres.iter().filter(|s| s.kind == 4).map(|s| (s.center, s.axis)).collect()
                    } else if let Some(pose) = poses.get(attacker).ok().filter(|q| q.of_kind(4).next().is_some()) {
                        pose.of_kind(4).map(|q| (q.point, q.axis)).collect()
                    } else {
                        art.art
                            .get(&oa.cell)
                            .map(|a| a.spheres.iter().filter(|s| s.kind == 4).map(|s| (s.center, s.axis)).collect())
                            .unwrap_or_default()
                    };
                    candidates
                        .into_iter()
                        .min_by(|a, b| world(a.0).distance(part_at).total_cmp(&world(b.0).distance(part_at)))
                        .map(|c| c.1)
                        .filter(|axis| *axis != Vec2::ZERO)
                        .unwrap_or_else(|| {
                            let local = Quat::from_rotation_y(-oa.heading) * Vec3::new(part_at.x - at.x, 0.0, part_at.y - at.y);
                            Vec2::new(local.x, local.z).normalize_or(Vec2::NEG_Y)
                        })
                };
                let victim_locked = cb.is_some_and(|c| if drain { c.acting_locked() } else { c.hit_cooldown > 0.0 });
                if oa.dead || ob.dead || victim_locked || ca.is_some_and(|c| c.acting_locked()) {
                    continue;
                }
                let victim_npc = ob.kind != CellKind::Player;
                let (code, slot) = if mouth == 4 {
                    (anim_code::EAT_PROBOSCIS, CellAnimation::CellEatProb)
                } else if oa.kind == CellKind::Player {
                    (anim_code::BITE_SOLID, CellAnimation::CellDmgSolidMand)
                } else {
                    (anim_code::BITE_SOLID, CellAnimation::CellDmgSolidMandNpc)
                };
                let seconds = clock.seconds(&oa, code, 0);
                // A bite's callback runs when the clip posts `contact` (0xE59C10), else
                // when the event ends with the clip; the drain's finaliser runs at the end.
                let lands = seconds.map(|clip| {
                    if drain {
                        clip
                    } else {
                        clock.contact(&oa, code, 0).map_or(clip, |c| c.min(clip))
                    }
                });
                if let Ok((_, _, _, combat, mut anim, ..)) = objects.get_mut(attacker) {
                    play(&mut anim, code, slot);
                    if let (Some(mut c), Some(t)) = (combat, seconds) {
                        c.busy = t;
                    }
                }
                if drain {
                    // The victim is held for the event too (0xE69C10 locks +0x178 / +0x17B /
                    // +0x17F / +0x17C every frame) and pulled to the mouth (`drain_pull`).
                    if let Ok((_, _, _, combat, mut anim, ..)) = objects.get_mut(victim) {
                        let stun = if victim_npc { CellAnimation::CellEatProbStunNpc } else { CellAnimation::CellEatProbStun };
                        play(&mut anim, DRAIN_VICTIM, stun);
                        if let (Some(mut c), Some(t)) = (combat, seconds) {
                            c.hit_cooldown = c.hit_cooldown.max(t);
                            c.busy = c.busy.max(t);
                        }
                    }
                }
                match lands {
                    Some(timer) => {
                        let pull = if drain { timer * DRAIN_PULL_SHARE } else { 0.0 };
                        commands.entity(attacker).insert(CellBiting { victim, timer, drain, point, mouth_local, mouth_axis, pull });
                    }
                    None if drain => actions.push(Action::DrainEnd { attacker, victim, axis: mouth_axis }),
                    None => actions.push(Action::BiteLand { attacker, victim, point }),
                }
            }
            Action::DrainEnd { attacker, victim, axis } => {
                // Event 0x13 ends (finaliser 0xE7DD90): the event is dropped when either
                // cell is gone (0xE69C10). 0xE7D760(drainer, victim, mouth 4) is the meal
                // (PlayerAte / NPC growth of the victim's record), then the victim's hit
                // points drop by GetDamageAmount directly (no Damage gate, no reaction
                // clip) and 0xE7A770 runs the death routine at 0 hit points.
                let Ok([(_, _, oa, ..), (_, _, ob, ..)]) = objects.get_many([attacker, victim]) else {
                    continue;
                };
                if oa.dead || ob.dead {
                    continue;
                }
                // 0xE7DD90 passes the drainer's mouth axis (0xE67D30 in the world: the
                // part axis turned by the cell's rotation) as event 0x15's knockback
                // vector (+0x1A0) of the drainer and its negation for the victim.
                let world_axis = {
                    let w = Quat::from_rotation_y(oa.heading) * Vec3::new(axis.x, 0.0, axis.y);
                    Vec2::new(w.x, w.z).normalize_or_zero()
                };
                let a_player = oa.kind == CellKind::Player;
                let amount = damage_amount(a_player, oa.scale, ob.kind == CellKind::Player, ob.scale, player_scale, difficulty);
                let record = db.cell(ob.cell);
                if let Ok((_, _, mut oe, _, _, _, ai)) = objects.get_mut(attacker) {
                    if a_player {
                        player_ate(record, 4, &mut oe, &mut p.run, &mut p.flow, difficulty);
                    } else if let Some(mut ai) = ai {
                        npc_meal(db, &mut oe, &mut ai, difficulty);
                    }
                }
                let mut died = false;
                if let Ok((_, _, mut ov, combat, ..)) = objects.get_mut(victim) {
                    ov.hp = (ov.hp - amount).max(0);
                    let player_victim = ov.kind == CellKind::Player;
                    if let Some(mut c) = combat {
                        c.last_attacker = Some(attacker);
                        c.since_hurt = 0.0;
                        c.bitten = true;
                        c.killed_by_player = a_player;
                        if ov.hp == 0 && !player_victim {
                            c.death = DeathKind::Killed;
                        }
                    }
                    if ov.hp == 0 && !player_victim {
                        ov.dead = true;
                        died = true;
                        if a_player {
                            p.run.cell.count_kill();
                        }
                    }
                }
                if died {
                    friend_fear(db, &mut objects, victim, attacker, player_scale, difficulty, p.game.world_scale, FriendFear::Death);
                }
                // The follow-up (0xE72530, twice, after the death routine): a cell that is
                // neither dead nor dying plays the retract clip (drainer 0x22, victim 0x21)
                // and is held for it (the event 0x15 it creates, updated by 0xE59640,
                // sets +0x178 and the state every frame). Event 0x15's knockback vector
                // (+0x1A0, consumed by HitImpulse with the retract clip's Data1) is the
                // mouth axis for the drainer and its negation for the victim (0xE7DD90 ->
                // 0xE72530 -> 0xE59640, VERIFIED; the update applies in the drain's own
                // states 0xD / 0x20, which the table of 0xE59640 accepts).
                for (cell, code, knock) in [(attacker, DRAIN_RETRACT, world_axis), (victim, DRAIN_RETRACT_VICTIM, -world_axis)] {
                    let Ok((_, _, o, combat, mut anim, motion, ..)) = objects.get_mut(cell) else {
                        continue;
                    };
                    if let Some(mut m) = motion {
                        m.knock = knock;
                    }
                    if o.dead || o.hp <= 0 {
                        continue;
                    }
                    let npc = o.kind != CellKind::Player;
                    let slot = match (code == DRAIN_RETRACT, npc) {
                        (true, false) => CellAnimation::CellEatProbRetract,
                        (true, true) => CellAnimation::CellEatProbRetractNpc,
                        (false, false) => CellAnimation::CellEatProbRetractTrgt,
                        (false, true) => CellAnimation::CellEatProbRetractTrgtNpc,
                    };
                    play(&mut anim, code, slot);
                    if let (Some(mut c), Some(t)) = (combat, clock.seconds(&o, code, 0)) {
                        c.busy = c.busy.max(t);
                    }
                }
            }
            Action::BiteLand { attacker, victim, point } => {
                // Bite resolution (0xE7B280): both cells still exist and the
                // victim is not inside a hit reaction. The bite animation already
                // played when the bite started (its clip is what delayed this).
                let Ok([(_, _, oa, ..), (_, tb, ob, cb, ..)]) = objects.get_many([attacker, victim]) else {
                    continue;
                };
                if oa.dead || ob.dead || cb.is_some_and(|c| c.hit_cooldown > 0.0) {
                    continue;
                }
                let a_player = oa.kind == CellKind::Player;
                let b_living = db.cell(ob.cell).is_some_and(|r| {
                    living(&ob, crate::cell_ai::ai_block(r, difficulty).kind, r.cell_type)
                });
                let amount = damage_amount(a_player, oa.scale, ob.kind == CellKind::Player, ob.scale, player_scale, difficulty);
                let (b_scale, b_hp, b_cell) = (ob.scale, ob.hp, ob.cell);
                let (b_radius, b_pos, b_heading) = (ob.radius, ob.plane(tb), ob.heading);
                if scale_diff(b_scale, player_scale) < 3 {
                    if b_living && amount < b_hp {
                        let _ = damage(
                            db,
                            &mut objects,
                            &clock,
                            attacker,
                            victim,
                            DAMAGE_BITE,
                            player_scale,
                            difficulty,
                            p.game.world_scale,
                            &mut p.run,
                        );
                    } else {
                        let modelled = p.art.art.get(&b_cell).is_some_and(|a| !a.models.is_empty());
                        eat_kill(&mut commands, &mut objects, &clock, attacker, victim, modelled, &mut p.run, &mut effects);
                    }
                } else if b_living
                    && damage(
                        db,
                        &mut objects,
                        &clock,
                        attacker,
                        victim,
                        DAMAGE_BITE,
                        player_scale,
                        difficulty,
                        p.game.world_scale,
                        &mut p.run,
                    )
                {
                    // The torn-off chunk (0xE7B280 -> 0xE76AF0): special cell #27
                    // `cell_puncture` (a leak cell, 11.3) at the contact point,
                    // scale = the player's own (0xE4EE40), size = the bitten
                    // cell's size, attached to it; once per landed bite.
                    p.queue.0.push(leak_request(
                        crate::world_objects::name_id("cell_puncture"),
                        player_scale,
                        point,
                        victim,
                        b_pos,
                        b_heading,
                        b_radius,
                    ));
                }
            }
            Action::Eat { eater, food, mouth } => {
                // Swallow whole (0xE7E020): the eat animation starts and the
                // victim is held at the mouth until the meal ends (`swallow`).
                let Ok([(_, _, oe, ce, mut eanim, ..), (_, _, mut of, fcombat, ..)]) =
                    objects.get_many_mut([eater, food])
                else {
                    continue;
                };
                // 0xE7E020 / 0xE7ED42: not while the eater is locked (+0x178).
                if of.dead || oe.dead || of.passive || oe.passive || ce.as_ref().is_some_and(|c| c.acting_locked()) {
                    continue;
                }
                of.passive = true;
                if let Some(mut c) = fcombat {
                    c.bitten = true;
                    c.killed_by_player = oe.kind == CellKind::Player;
                }
                let npc = oe.kind != CellKind::Player;
                let (code, slot) = match (mouth, npc) {
                    (3, false) => (anim_code::EAT_FILTER, CellAnimation::CellEatFilter),
                    (3, true) => (anim_code::EAT_FILTER, CellAnimation::CellEatFilterNpc),
                    (4, false) => (anim_code::EAT_PROBOSCIS, CellAnimation::CellEatProb),
                    (4, true) => (anim_code::EAT_PROBOSCIS, CellAnimation::CellEatProbNpc),
                    (_, false) => (anim_code::EAT_MANDIBLE, CellAnimation::CellEatMand),
                    (_, true) => (anim_code::EAT_MANDIBLE, CellAnimation::CellEatMandNpc),
                };
                play(&mut eanim, code, slot);
                // Event 1 is created with 1/3 of the eat clip (constant 0x13F26CC):
                // its callback takes the victim (0xE7DB80 -> 0xE7D880) and its
                // finaliser is the meal (0xE7D800 -> 0xE7D760). A plant sucked by
                // a proboscis (0xB, 0xE72910) takes the whole clip (event 0x12).
                let fraction = if mouth == 4 { 1.0 } else { 1.0 / 3.0 };
                let parts = 1u32 << mouth;
                let duration = clock.seconds(&oe, code, parts).map_or(SWALLOW_SECONDS, |clip| {
                    let timer = clip * fraction;
                    // The victim is taken by the callback, which runs when the
                    // clip posts `contact` if that comes before the timer ends.
                    if mouth == 4 { timer } else { clock.contact(&oe, code, parts).map_or(timer, |c| c.min(timer)) }
                });
                commands.entity(food).insert(CellSwallowed { eater, mouth, timer: 0.0, duration });
            }
        }
    }
    // A drain's meal moved the food: DNA and the level change (4B), as in `swallow`.
    if let Some(grew) = food_moved(&mut p.run, food_before) {
        if let Some(Ok((_, _, mut o, _, anim, ..))) = p.game.player.map(|e| objects.get_mut(e)) {
            o.abs_size = player_abs_size(p.run.cell.food_progression);
            if let (true, Some(mut anim)) = (grew, anim) {
                request_anim(&mut anim, anim_code::GROW, CellAnimation::CellGrow);
            }
        }
    }
}

/// DNA earned and the level change / Grow of 4B after the food moved from `before`
/// (shared by the swallow and the drain meals). Returns `Some(grew)` when the
/// avatar's size has to change (`grew`: the body grew, so the grow clip plays).
fn food_moved(run: &mut RunProgress, before: i32) -> Option<bool> {
    let food = run.cell.food_progression;
    if food == before {
        return None;
    }
    let gained = dna_earned(food) - dna_earned(before);
    run.editor_dna += gained as f32;
    run.dna_banked_total += gained.max(0) as f32;
    run.dna_this_stage = food as f32 / 10.0;
    // Level change / Grow (4B): when worldScaleFor or playerSizeFor changes,
    // SetSize(playerSizeFor) (with the grow animation when the body grew); a
    // world-scale change also re-populates (population).
    let grew = world_scale_for(food) > world_scale_for(before) || player_size_for(food) > player_size_for(before);
    let changed =
        grew || world_scale_for(food) != world_scale_for(before) || player_size_for(food) != player_size_for(before);
    changed.then_some(grew)
}

/// The leak cell `0xE76AF0` makes: cell `cell` (a plant's `leak` cell or special
/// #27) at `point` on its parent, at `scale`, with the parent's size as the
/// size factor, attached at that point and turned to the parent's centre
/// (VERIFIED, 0xE68170: the leak's orientation is `0x69B600(&-(parent - self),
/// +Z)` = `LookRotation`, whose matrix row 1 (the local +Y axis, the heading
/// axis) is the direction away from the parent: the wound's outward normal).
pub fn leak_request(
    cell: u32,
    scale: CellScale,
    point: Vec2,
    parent: Entity,
    parent_pos: Vec2,
    parent_heading: f32,
    parent_radius: f32,
) -> SpawnRequest {
    let out = (point - parent_pos).normalize_or(Vec2::NEG_Y);
    let local = Quat::from_rotation_y(-parent_heading) * Vec3::new(point.x - parent_pos.x, 0.0, point.y - parent_pos.y);
    let facing = crate::cell_engine::heading_of(out);
    SpawnRequest {
        cell,
        scale,
        position: point,
        heading: facing,
        size_factor: parent_radius / UNIT,
        role: SpawnRole::Loot,
        attach: Some(crate::cell_engine::CellAttach {
            parent,
            local: Vec2::new(local.x, local.z) / parent_radius.max(1e-6),
            heading: facing - parent_heading,
        }),
        ..default()
    }
}

/// A cell being swallowed whole: held and pulled in at the eater's mouth
/// (0xE7DB80 / 0xE5F4D0) until the meal ends (0xE7D760).
#[derive(Component, Clone, Copy, Debug)]
pub struct CellSwallowed {
    pub eater: Entity,
    /// 3 filter, 2 mandible, 4 proboscis.
    pub mouth: u8,
    pub timer: f32,
    /// Seconds from the swallow to the victim's end and the meal (event 1):
    /// a third of the eat clip or its `contact` marker, whichever comes first.
    pub duration: f32,
}

/// Fallback seconds from the swallow to the end of the meal while the clip
/// table is not loaded (retail: 1/3 of the eat clip, 0xE7E0A0).
pub const SWALLOW_SECONDS: f32 = 0.4;

/// Pull swallowed cells into the mouth and end the meals: the victim is eaten
/// (`cell_death_eaten`; the avatar dies), the player eats (`PlayerAte`, a
/// creature counts as a kill) or the NPC eater digests / grows; then DNA and
/// the level change / Grow of 4B.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn swallow(
    mut commands: Commands,
    time: Res<Time>,
    db: Res<CellDatabase>,
    game: Res<CellGame>,
    mut run: ResMut<RunProgress>,
    mut flow: ResMut<crate::cell_flow::CellFlow>,
    art: Res<CellArtCache>,
    clips: Res<CellClipTimes>,
    mut effects: MessageWriter<CellEffectRequest>,
    mut held: Query<(
        Entity,
        &mut Transform,
        &mut CellObject,
        &mut CellSwallowed,
        Option<&mut CellCombat>,
        Option<&mut CellAnimState>,
    )>,
    mut eaters: Query<
        (&Transform, &mut CellObject, Option<&mut CellAnimState>, Option<&mut crate::cell_ai::CellAiState>),
        Without<CellSwallowed>,
    >,
) {
    let dt = time.delta_secs().min(0.1);
    let difficulty = game.difficulty;
    let food_before = run.cell.food_progression;
    let clock = PlayClock { clips: &clips, art: &art, player: crate::cell_engine::player_scale(food_before) };
    for (victim, mut tv, mut ov, mut s, combat, mut vanim) in &mut held {
        s.timer += dt;
        let Ok((te, mut oe, _, ai)) = eaters.get_mut(s.eater) else {
            // The eater is gone: let go.
            ov.passive = false;
            commands.entity(victim).remove::<CellSwallowed>();
            continue;
        };
        if oe.dead {
            ov.passive = false;
            commands.entity(victim).remove::<CellSwallowed>();
            continue;
        }
        let mouth = oe.plane(te) + heading_dir(oe.heading) * oe.radius * 0.5;
        let at = ov.plane(&tv).lerp(mouth, (dt * 12.0).min(1.0));
        tv.translation.x = at.x;
        tv.translation.z = at.y;
        ov.velocity = oe.velocity;
        if s.timer < s.duration {
            continue;
        }
        // Swallow end (0xE7D880) and the meal (0xE7D760).
        commands.entity(victim).remove::<CellSwallowed>();
        let modelled_victim = ov.kind != CellKind::Player && art.art.get(&ov.cell).is_some_and(|a| a.modelled);
        if let Some(mut c) = combat {
            c.death = DeathKind::Eaten;
            c.death_fx_played = modelled_victim;
        }
        if ov.kind == CellKind::Player {
            // The avatar goes to its death flow (0xE5C8E0; request 9, event 7 = 4.3 s).
            ov.hp = 0;
            ov.passive = false;
        } else if modelled_victim {
            // A modelled victim (0xE7D880): `cell_death_eaten` is instanced on it now, it
            // plays request 9 and event 7 (the clip, 0.367 s x G for an NPC) holds it
            // (0xE59500: every lock) until 0xE7B410 removes it.
            ov.hp = 0;
            effects.write(CellEffectRequest {
                effect: crate::world_objects::name_id("cell_death_eaten"),
                position: Vec3::new(tv.translation.x, -ov.depth, tv.translation.z),
                attach_to: None,
                scale: ov.radius,
            });
            if let Some(anim) = vanim.as_mut() {
                request_anim(anim, anim_code::DEATH_EATEN, CellAnimation::CellDeathEatenNpc);
            }
            let timer = clock.seconds(&ov, anim_code::DEATH_EATEN, 0).unwrap_or(DYING_SECONDS);
            commands.entity(victim).insert(CellDying { timer });
        } else {
            // A cell without a model is removed at once (0xE780A0).
            ov.dead = true;
        }
        let record = db.cell(ov.cell);
        if oe.kind == CellKind::Player {
            if ov.kind.is_creature() {
                run.cell.count_kill();
            }
            player_ate(record, s.mouth, &mut oe, &mut run, &mut flow, difficulty);
        } else if let Some(mut ai) = ai {
            npc_meal(&db, &mut oe, &mut ai, difficulty);
        }
    }
    if let Some(grew) = food_moved(&mut run, food_before) {
        if let Some(Ok((_, mut o, anim, _))) = game.player.map(|p| eaters.get_mut(p)) {
            o.abs_size = player_abs_size(run.cell.food_progression);
            if let (true, Some(mut anim)) = (grew, anim) {
                request_anim(&mut anim, anim_code::GROW, CellAnimation::CellGrow);
            }
        }
    }
}

/// One frame of the pull (0xE5F4D0 / 0xE62380): `before` is the pull sub-timer before this
/// frame; while it lasts past the frame the victim covers `dt / before` of the way to the
/// mouth (the distance closes linearly, reaching it when the sub-timer hits 0), then it
/// sits at the mouth.
pub fn drain_pull_step(victim: Vec2, mouth: Vec2, before: f32, dt: f32) -> Vec2 {
    if before > dt {
        victim.lerp(mouth, (dt / before).clamp(0.0, 1.0))
    } else {
        mouth
    }
}

/// The proboscis drain's pull (event 0x13's update 0xE69C10, VERIFIED): while the event's
/// sub-timer 0 (0.125 x the clip) runs, 0xE5F4D0 moves the victim by `dt / remaining` of
/// the way to the drainer's mouth part (0xE699F0 -> 0xE68000: the part's animated
/// transform) each frame, shifting its pins with it (0xE5E590); when the sub-timer is 0
/// 0xE62380 attaches the victim at the mouth transform, so it follows the drainer. Both
/// cells are held by the locks set in `Action::Bite`. The victim's facing and the attach
/// transform's rotation (0xE5F450) are not ported; the victim takes the drainer's velocity.
pub fn drain_pull(
    time: Res<Time>,
    mut bites: Query<(Entity, &mut CellBiting)>,
    mut cells: Query<(&mut Transform, &mut CellObject)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (attacker, mut bite) in &mut bites {
        if !bite.drain {
            continue;
        }
        let Ok([(ta, oa), (mut tv, mut ov)]) = cells.get_many_mut([attacker, bite.victim]) else {
            continue;
        };
        if oa.dead || ov.dead {
            continue;
        }
        let f = heading_dir(oa.heading);
        let mouth = oa.plane(&ta) + f * bite.mouth_local.x + Vec2::new(-f.y, f.x) * bite.mouth_local.y;
        let before = bite.pull;
        bite.pull = (before - dt).max(0.0);
        let at = drain_pull_step(ov.plane(&tv), mouth, before, dt);
        tv.translation.x = at.x;
        tv.translation.z = at.y;
        ov.velocity = oa.velocity;
    }
}

/// Poison cloud lifetime (event 0x1A) by poison level 1 / 2 / 3 (RE 10F).
pub const POISON_CLOUD_SECONDS: [f32; 3] = [3.0, 5.0, 7.0];

/// Electric values of a cell (RE 12): (long, short, range in Bevy units,
/// jumps); None without electric.
pub fn electric_values(
    db: &CellDatabase,
    object: &CellObject,
    caps: &CellCaps,
    parts_ai: bool,
    difficulty: usize,
    world_scale: f32,
) -> Option<(f32, f32, f32, u32)> {
    let record = db.cell(object.cell)?;
    let block = crate::cell_ai::ai_block(record, difficulty);
    let n = caps.n[6] as f32;
    let long = if block.electric_recharge != -1.0 {
        block.electric_recharge
    } else {
        db.graphs.eval_or(crate::cell_data::CellGraphs::ELECTRIC_RECHARGE_COUNT, n, -1.0)
    };
    if long == -1.0 || (long <= 0.0 && n == 0.0) {
        return None;
    }
    let short = if block.electric_recharge_vs_small != -1.0 {
        block.electric_recharge_vs_small
    } else if record.ai.electric_recharge != -1.0 {
        record.ai.electric_recharge
    } else {
        db.graphs.eval_or(crate::cell_data::CellGraphs::ELECTRIC_RECHARGE_VS_SMALL_COUNT, n, 0.0)
    };
    let base = if object.kind == CellKind::Player || parts_ai {
        caps.electric_range
    } else {
        block.arc_length
    };
    let range = base * crate::cell_engine::real_size(object.scale) / (world_scale * 60.0) * UNIT;
    if range <= 0.0 {
        return None;
    }
    // Chain length (0xE716A5..0xE71718): the avatar and Parts AI cells use the
    // part graph (caps+0x126C), every other cell its AI block's `numArcs`
    // (+0x5C): the installed zappers are buzzy (1 arc) and the needle buzzy (2).
    let jumps = if object.kind == CellKind::Player || parts_ai {
        caps.electric_jumps.max(1)
    } else {
        block.num_arcs.max(0) as u32
    };
    Some((long.max(0.0), short.max(0.0), range, jumps))
}

/// Poison emit points (0xE52040 / 0xE68000): at most one poison part per
/// side of the body (left, centre line, right; the caps keep up to three
/// part indices at +0x1278, count +0x1274), each cloud released at its part
/// (0xE68000: the part's animated transform plus the rigblock's +0x60
/// offset). `local` are the poison parts' body-plane positions (body radius
/// 1, x = the cell's left/right); NPCs pass the animated pose and offset
/// (`cell_anim::CellPartPoses`, [`poison_emit_pairs`]), the avatar's placed
/// parts their placement plus the part model's offset
/// (`cell_motion::player_poison_points`).
pub fn poison_emit_points(local: &[Vec2]) -> Vec<Vec2> {
    poison_emit_pairs(&local.iter().map(|p| (*p, *p)).collect::<Vec<_>>())
}

/// [`poison_emit_points`] over `(part position, emit point)` pairs: the side
/// (and so which parts count) comes from the part's position, the cloud is
/// released at its emit point (the part's animated pose plus the rigblock's
/// +0x60 offset, 0xE68000).
pub fn poison_emit_pairs(parts: &[(Vec2, Vec2)]) -> Vec<Vec2> {
    let mut sides: Vec<i32> = Vec::new();
    let mut out: Vec<Vec2> = Vec::new();
    let side = |p: Vec2| if p.x < -0.05 { 0 } else if p.x > 0.05 { 2 } else { 1 };
    for (at, emit) in parts {
        if !sides.contains(&side(*at)) {
            sides.push(side(*at));
            out.push(*emit);
        }
    }
    out
}

/// 0xE68D20 for one sphere: the distance from the zap's source point to the
/// surface of a sphere (centre `c`, radius `r`) when the sphere reaches within
/// `range` of it, i.e. `(r + range)^2 > |c - point|^2`; the distance is
/// `max(0, |c - point| - r)`.
pub fn zap_reach(point: Vec2, range: f32, c: Vec2, r: f32) -> Option<f32> {
    let d = c.distance(point);
    (r + range > d).then_some((d - r).max(0.0))
}

/// A cell ready to zap this frame.
struct Zapper {
    entity: Entity,
    scale: CellScale,
    /// Radius of the zapper (the search sphere of each electric part).
    radius: f32,
    /// World positions of its electric parts (11.6 part list, type 6).
    points: Vec<Vec2>,
    jumps: u32,
    range: f32,
    long: f32,
    short: f32,
}

/// The electric target search of 0xE69190 / 0xE69000: for every source point
/// (an electric part, or the previous target of a chain) every cell whose sphere
/// overlaps the sphere `source size + range` is a candidate if it is Living, not
/// hit-locked (+0x17C), not already hit, not two levels or more above the
/// zapper unless it is `cell_puffish` (special cell #22, 0xE4CC90(0x16)), has
/// no electric part itself (0xE68CD0: caps+0x1260 != -1) and one of its body /
/// mandible spheres (the avatar's mandibles do not count) reaches within `range`
/// of the point (0xE68D20); the closest wins. Friends are not excluded:
/// retail has no ShouldNotAttack here or in the zap (VERIFIED, 0xE71520
/// subtracts GetDamageAmount directly). Returns the target, the source point
/// it was found from and the distance.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn zap_target(
    db: &CellDatabase,
    art: &CellArtCache,
    grid: &CellGrid,
    objects: &Objects,
    caps_of: &dyn Fn(&CellObject) -> Option<CellCaps>,
    player_spheres: &[crate::cell_engine::PartSphere],
    zapper: Entity,
    zapper_scale: CellScale,
    sources: &[(Vec2, f32)],
    range: f32,
    hit: &[Entity],
    difficulty: usize,
) -> Option<(Entity, Vec2, f32)> {
    let puffish = crate::world_objects::name_id("cell_puffish");
    let mut best: Option<(f32, Entity, Vec2)> = None;
    for &(point, source_size) in sources {
        grid.query(point, source_size + range + grid.max_radius, |item| {
            if item.entity == zapper || hit.contains(&item.entity) {
                return;
            }
            // FindCellsInRadius: the cell's sphere overlaps the search sphere.
            if item.position.distance(point) >= source_size + range + item.radius {
                return;
            }
            let Ok((_, t, o, combat, ..)) = objects.get(item.entity) else {
                return;
            };
            let Some(record) = db.cell(o.cell) else {
                return;
            };
            if o.dead || !living(&o, crate::cell_ai::ai_block(record, difficulty).kind, record.cell_type) {
                return;
            }
            if combat.is_some_and(|c| c.hit_cooldown > 0.0) {
                return;
            }
            if crate::cell_ai::rel(zapper_scale, o.scale) == 4 && o.cell != puffish {
                return;
            }
            if caps_of(&o).is_some_and(|c| c.n[6] > 0) {
                return;
            }
            let centre = o.plane(t);
            let mut reach: Option<f32> = zap_reach(point, range, centre, o.radius);
            if o.kind != CellKind::Player {
                let spheres = match o.kind {
                    CellKind::Creature => art.art.get(&o.cell).map(|a| a.spheres.as_slice()),
                    CellKind::OwnSpecies => Some(player_spheres),
                    _ => None,
                };
                let rot = Quat::from_rotation_y(o.heading);
                for s in spheres.unwrap_or_default().iter().filter(|s| s.kind == 2) {
                    let w = rot * Vec3::new(s.center.x, 0.0, s.center.y) * o.radius;
                    let c = centre + Vec2::new(w.x, w.z);
                    if let Some(d) = zap_reach(point, range, c, s.radius * o.radius) {
                        // 0xE68D20 answers with the FIRST sphere of the part list that
                        // reaches (kind 12 body or kind 2, not for the avatar) and its
                        // distance; that distance ranks the targets (0xE69000: the
                        // smaller one wins). ADAPTED: the body sphere is taken as
                        // first of the list (the rigblock order of the body block is
                        // not mapped).
                        reach.get_or_insert(d);
                    }
                }
            }
            if let Some(d) = reach {
                if best.is_none_or(|(bd, ..)| bd > d) {
                    best = Some((d, item.entity, point));
                }
            }
        });
    }
    best.map(|(d, e, p)| (e, p, d))
}

/// Passive weapons, every cell (0xE76A80, RE 12): poison clouds while
/// swimming and automatic electric zaps that chain.
/// The zap is the retail one: a target per electric part (`zap_target`), a
/// chain of at most `jumps` cells from each previous target (0xE693F0), damage
/// = `GetDamageAmount` with no friend test (`DamageGate::Zap`), the target's
/// velocity cleared (0xE71520 zeroes +0x90..+0x98); "smaller target"
/// (0xE573E0) is rel < 2. ADAPTED: zap effects are the preloaded beam effects
/// at the targets; a cell without part data zaps from its centre.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn weapons(
    time: Res<Time>,
    db: Res<CellDatabase>,
    originals: Res<crate::Originals>,
    art: Res<CellArtCache>,
    clips: Res<CellClipTimes>,
    grid: Res<CellGrid>,
    game: Res<CellGame>,
    mut run: ResMut<RunProgress>,
    mut queue: ResMut<CellSpawnQueue>,
    mut beams: MessageWriter<crate::cell_fx::CellBeamRequest>,
    poses: Query<&crate::cell_anim::CellPartPoses>,
    mut objects: Objects,
) {
    let dt = time.delta_secs().min(0.1);
    let Some(globals) = db.globals.as_ref() else {
        return;
    };
    let p = crate::cell_engine::player_scale(run.cell.food_progression);
    let clock = PlayClock { clips: &clips, art: &art, player: p };
    let player_caps = crate::cell_motion::player_caps(&run, &originals, &db);
    let player_spheres = crate::cell_motion::player_part_spheres(&run, &originals);
    let difficulty = game.difficulty;
    let caps_of = |o: &CellObject| -> Option<CellCaps> {
        if o.kind == CellKind::Player || o.kind == CellKind::OwnSpecies {
            return Some(player_caps);
        }
        let a = art.art.get(&o.cell).filter(|a| a.modelled)?;
        Some(CellCaps::from_counts(a.counts, &db.graphs, globals))
    };
    let mut ready: Vec<Zapper> = Vec::new();
    for (e, t, o, combat, anim, ..) in &mut objects {
        let Some(mut c) = combat else {
            continue;
        };
        if o.background || o.dead || o.passive || o.hp <= 0 {
            c.electric_cooldown = 0.0;
            continue;
        }
        let Some(caps) = caps_of(&o) else {
            continue;
        };
        // Poison (0xE76880): gate 0xE68630, then (2|v| + 1) per second.
        let state = anim.map_or(0, |a| a.request_code);
        if caps.poison > 0.0
            && caps.n[5] > 0
            && state != anim_code::HATCH
            && state != anim_code::HATCH + 1
            && scale_diff(o.scale, p) >= 2
        {
            c.poison_acc += (2.0 * o.velocity.length() / UNIT + 1.0) * dt;
            if c.poison_acc > 1.0 {
                c.poison_acc = 0.0;
                let avatar = o.kind == CellKind::Player;
                let level = (caps.poison.round() as usize).clamp(1, 3);
                // Each cloud leaves at the part's pose plus its +0x60 offset (0xE68000):
                // an NPC's pose is the animated one (`CellPartPoses`), the avatar's the
                // placement of its part with the part model's offset.
                let local: Vec<(Vec2, Vec2)> = if o.kind == CellKind::Player || o.kind == CellKind::OwnSpecies {
                    crate::cell_motion::player_poison_points(&run, &originals)
                } else if let Some(pose) = poses.get(e).ok().filter(|p| p.of_kind(5).next().is_some()) {
                    pose.of_kind(5).map(|part| (part.point, part.emit)).collect()
                } else {
                    // Not rigged yet: the static sphere centre (headless runs).
                    art.art
                        .get(&o.cell)
                        .map(|a| a.spheres.iter().filter(|s| s.kind == 5).map(|s| (s.center, s.center)).collect())
                        .unwrap_or_default()
                };
                let rot = Quat::from_rotation_y(o.heading);
                let mut points: Vec<Vec2> = poison_emit_pairs(&local)
                    .into_iter()
                    .map(|p| {
                        let w = rot * Vec3::new(p.x, 0.0, p.y) * o.radius;
                        o.plane(t) + Vec2::new(w.x, w.z)
                    })
                    .collect();
                if points.is_empty() {
                    // No part positions known (unmodelled cells): the rear.
                    points.push(o.plane(t) - heading_dir(o.heading) * o.radius);
                }
                for position in points {
                    queue.0.push(SpawnRequest {
                        cell: crate::world_objects::name_id(if avatar {
                            "cell_weapon_poison_player"
                        } else {
                            "cell_weapon_poison_npc"
                        }),
                        scale: p,
                        position,
                        owner: Some(e),
                        life: Some(POISON_CLOUD_SECONDS[level - 1]),
                        ..default()
                    });
                }
            }
        }
        // Electric (0xE71980).
        let parts_ai = o.kind == CellKind::OwnSpecies;
        let Some((long, short, range, jumps)) =
            electric_values(&db, &o, &caps, parts_ai, difficulty, game.world_scale)
        else {
            c.electric_cooldown = 0.0;
            continue;
        };
        if c.electric_cooldown > 0.0 {
            c.electric_cooldown = (c.electric_cooldown - dt).max(0.0);
            continue;
        }
        // 0xE71520 returns at once while the zapper is attack-locked (+0x17F): inside a
        // bite, stab or its own zap.
        if c.busy > 0.0 {
            continue;
        }
        // The electric parts' positions (0xE68000 per part of type 6).
        let local: Vec<Vec2> = if o.kind == CellKind::Player || o.kind == CellKind::OwnSpecies {
            crate::cell_motion::player_part_points(&run, &originals, 6)
        } else if let Some(pose) = poses.get(e).ok().filter(|p| p.of_kind(6).next().is_some()) {
            // The animated pose of each electric part (0xE84150).
            pose.of_kind(6).map(|part| part.point).collect()
        } else {
            art.art
                .get(&o.cell)
                .map(|a| a.spheres.iter().filter(|s| s.kind == 6).map(|s| s.center).collect())
                .unwrap_or_default()
        };
        let centre = o.plane(t);
        let rot = Quat::from_rotation_y(o.heading);
        let mut points: Vec<Vec2> = local
            .iter()
            .map(|p| {
                let w = rot * Vec3::new(p.x, 0.0, p.y) * o.radius;
                centre + Vec2::new(w.x, w.z)
            })
            .collect();
        if points.is_empty() {
            points.push(centre);
        }
        ready.push(Zapper {
            entity: e,
            scale: o.scale,
            radius: o.radius,
            points,
            jumps,
            range,
            long,
            short,
        });
    }
    // Targets and chains, then the zaps.
    for z in ready {
        let mut hit: Vec<Entity> = Vec::new();
        let mut sources: Vec<(Vec2, f32)> = z.points.iter().map(|p| (*p, z.radius)).collect();
        for _ in 0..z.jumps {
            let Some((target, from, _)) = zap_target(
                &db,
                &art,
                &grid,
                &objects,
                &caps_of,
                &player_spheres,
                z.entity,
                z.scale,
                &sources,
                z.range,
                &hit,
                difficulty,
            ) else {
                break;
            };
            let (at, target_radius, target_scale) = match objects.get(target) {
                Ok((_, t, o, ..)) => (o.plane(t), o.radius, o.scale),
                Err(_) => break,
            };
            if hit.is_empty() {
                // Cooldown by the first target's size (0xE573E0 < 2: short).
                let rel = crate::cell_ai::rel(z.scale, target_scale);
                if let Ok((_, _, _, Some(mut c), ..)) = objects.get_mut(z.entity) {
                    c.electric_cooldown = if rel < 2 { z.short } else { z.long };
                }
            }
            hit.push(target);
            let _ = damage_with(
                DamageGate::Zap,
                &db,
                &mut objects,
                &clock,
                z.entity,
                target,
                DAMAGE_ELECTRIC,
                p,
                difficulty,
                game.world_scale,
                &mut run,
            );
            if let Ok((_, _, mut o, ..)) = objects.get_mut(target) {
                o.velocity = Vec2::ZERO;
            }
            // The beam runs from the zapped cell to the electric part that found
            // it (or the previous target of a chain), scaled by the zapper's
            // size (cell_fx.rs).
            beams.write(crate::cell_fx::CellBeamRequest {
                primary: hit.len() == 1,
                from: Vec3::new(at.x, 0.0, at.y),
                to: Vec3::new(from.x, 0.0, from.y),
                scale: z.radius,
            });
            sources = vec![(at, target_radius)];
        }
        // Event 0x11 (0xE71520, finaliser none, update 0xE58890): the zapper plays
        // request 0x1E and, for that clip (PlayAnimation's return), it and the first
        // target are held every frame (zapper +0x17F / +0x17C / +0x17B, target +0x17F /
        // +0x178 / +0x17C).
        if let Some(&first) = hit.first() {
            let hold = objects
                .get(z.entity)
                .ok()
                .and_then(|(_, _, o, ..)| clock.seconds(&o, ZAP_RELEASE, 0))
                .unwrap_or(HIT_COOLDOWN);
            if let Ok((_, _, _, Some(mut c), mut anim, ..)) = objects.get_mut(z.entity) {
                c.busy = c.busy.max(hold);
                c.hit_cooldown = c.hit_cooldown.max(hold);
                play(&mut anim, ZAP_RELEASE, CellAnimation::CellElecRelease);
            }
            if let Ok((_, _, _, Some(mut c), ..)) = objects.get_mut(first) {
                c.busy = c.busy.max(hold);
                c.hit_cooldown = c.hit_cooldown.max(hold);
            }
        }
    }
}

/// Timed cells (poison clouds) leave when their event ends.
pub fn lifetimes(time: Res<Time>, mut cells: Query<(&mut CellObject, &mut crate::cell_engine::CellLifetime)>) {
    let dt = time.delta_secs().min(0.1);
    for (mut object, mut life) in &mut cells {
        life.0 -= dt;
        if life.0 <= 0.0 {
            object.dead = true;
        }
    }
}

/// RecordUnlock (0xE5D7B0): mate button timers, tutorial mission 1, the
/// unlock itself. Returns true for a newly unlocked part.
pub fn record_unlock(run: &mut RunProgress, flow: &mut crate::cell_flow::CellFlow, unlock_type: UnlockType) -> bool {
    if !run.cell.show_mate_button {
        flow.mate_countdown = crate::cell_flow::MATE_COUNTDOWN_UNLOCK;
    }
    let mission = &mut run.cell.missions[1];
    if mission.state < 2 {
        mission.state = 1;
        mission.progress = (mission.progress + 1).min(6);
        if mission.progress >= 6 {
            mission.state = 2;
        }
    }
    run.cell.show_mate_button = true;
    run.cell.part_cinematic_played = true;
    unlock(run, unlock_type)
}

/// Which of the two retail damage paths a hit takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DamageGate {
    /// `Damage` (0xE7A7C0): victim Living, not a friend, not already hit /
    /// invulnerable; friends hear it (0xE71CE0).
    Normal,
    /// The zap (0xE71520) subtracts `GetDamageAmount` straight from the hit
    /// points: no friend test, no invulnerability test, no friend reaction. The
    /// target search already refused hit-locked and non-Living cells.
    Zap,
}

/// `Damage` (0xE7A7C0).
#[allow(clippy::too_many_arguments)]
fn damage(
    db: &CellDatabase,
    objects: &mut Objects,
    clock: &PlayClock,
    attacker: Entity,
    victim: Entity,
    kind: u8,
    player_scale: CellScale,
    difficulty: usize,
    world_scale: f32,
    run: &mut RunProgress,
) -> bool {
    damage_with(
        DamageGate::Normal,
        db,
        objects,
        clock,
        attacker,
        victim,
        kind,
        player_scale,
        difficulty,
        world_scale,
        run,
    )
}

/// `damage` through either gate; the hit lock (+0x17C) of a normal hit is the
/// victim's reaction clip (`clock`).
#[allow(clippy::too_many_arguments)]
fn damage_with(
    gate: DamageGate,
    db: &CellDatabase,
    objects: &mut Objects,
    clock: &PlayClock,
    attacker: Entity,
    victim: Entity,
    kind: u8,
    player_scale: CellScale,
    difficulty: usize,
    world_scale: f32,
    run: &mut RunProgress,
) -> bool {
    let normal = gate == DamageGate::Normal;
    {
        let Ok([(_, _, oa, ..), (_, _, ov, cv, ..)]) = objects.get_many([attacker, victim]) else {
            return false;
        };
        if ov.dead || ov.hp <= 0 || (normal && ov.invulnerable > 0.0) {
            return false;
        }
        if normal && cv.as_ref().is_some_and(|c| c.hit_cooldown > 0.0) {
            return false;
        }
        // The first tests of 0xE7A7C0: the victim must be Living (0xE52940: AI or
        // the avatar, hp left, solid) and must not be a friend of the attacker:
        // ShouldNotAttack(victim, attacker) (same cell file or friendGroup; or a
        // victim one level smaller than the player flagged wontAttackPlayerWhenSmall
        // when the avatar is the attacker). VERIFIED (argument order read from the
        // push order at 0xE7A7DF; a poison cloud or zap has its own cell file).
        let Some(record) = db.cell(ov.cell) else {
            return false;
        };
        if !living(&ov, crate::cell_ai::ai_block(record, difficulty).kind, record.cell_type) {
            return false;
        }
        if normal
            && should_not_attack(
                db,
                ov.cell,
                ov.scale,
                ov.dead,
                oa.cell,
                oa.kind == CellKind::Player,
                oa.dead,
                player_scale,
            )
        {
            return false;
        }
    }
    if normal {
        // 0xE71CE0 runs before the damage amount is known: the victim's friends
        // react even when no damage is dealt.
        friend_fear(db, objects, victim, attacker, player_scale, difficulty, world_scale, FriendFear::Damage);
    }
    let killed = {
        let Ok([(_, ta, oa, ..), (_, tv, mut ov, cv, mut anim, mv, _)]) = objects.get_many_mut([attacker, victim]) else {
            return false;
        };
        let a_player = oa.kind == CellKind::Player;
        let d = damage_amount(a_player, oa.scale, ov.kind == CellKind::Player, ov.scale, player_scale, difficulty);
        if d <= 0 && normal {
            return false;
        }
        ov.hp = (ov.hp - d).max(0);
        // Reaction request: kind 1 -> 14, 3 -> 15, 5 -> 17, else 16. Its clip is
        // the time of event 2 and of the hit lock (0xE7A94C -> 0xE59010).
        let npc = ov.kind != CellKind::Player;
        // The zap (0xE71520) plays request 0xE on its targets, 0x3C on `cell_puffish`.
        let puffish = !normal && ov.cell == crate::world_objects::name_id("cell_puffish");
        let (code, slot) = match (kind, npc) {
            (DAMAGE_ELECTRIC, _) if puffish => (HIT_ELECTRIC_PUFFISH, CellAnimation::EpicCellElectrocuted),
            (DAMAGE_ELECTRIC, false) => (anim_code::HIT_ELECTRIC, CellAnimation::CellTakeDmgElec),
            (DAMAGE_ELECTRIC, true) => (anim_code::HIT_ELECTRIC, CellAnimation::CellTakeDmgElecNpc),
            (DAMAGE_POISON, false) => (anim_code::HIT_POISON, CellAnimation::CellTakeDmgPoison),
            (DAMAGE_POISON, true) => (anim_code::HIT_POISON, CellAnimation::CellTakeDmgPoisonNpc),
            (DAMAGE_BITE, false) => (anim_code::HIT_BITE, CellAnimation::CellTakeDmgMand),
            (DAMAGE_BITE, true) => (anim_code::HIT_BITE, CellAnimation::CellTakeDmgMandNpc),
            (_, false) => (anim_code::HIT_STAB, CellAnimation::CellTakeDmgStab),
            (_, true) => (anim_code::HIT_STAB, CellAnimation::CellTakeDmgStabNpc),
        };
        if let Some(mut c) = cv {
            // Event 2 (a zap makes it too, 0xE71520) holds the victim (+0x17C / +0x178)
            // for the reaction clip.
            c.hit_cooldown = clock.seconds(&ov, code, 0).unwrap_or(HIT_COOLDOWN);
            c.last_attacker = Some(attacker);
            c.since_hurt = 0.0;
            c.bitten = kind == DAMAGE_BITE;
            c.killed_by_player = a_player;
            if ov.hp == 0 && ov.kind != CellKind::Player {
                c.death = DeathKind::Killed;
            }
            if !normal && ov.hp == 0 {
                // The zap only subtracts: event 2's finaliser (0xE7A770) runs the death
                // routine when the reaction clip ends.
                c.pending_death = Some(c.hit_cooldown);
            }
        }
        if ov.hp == 0 {
            if !normal {
                play(&mut anim, code, slot);
                return true;
            }
            if ov.kind == CellKind::Player {
                return true;
            }
            ov.dead = true;
            if a_player {
                run.cell.count_kill();
            }
            true
        } else {
            play(&mut anim, code, slot);
            // Knockback (0xE59010 stores minus Damage's direction argument at
            // +0x1A0); `cell_motion::hit_impulse` applies it with the reaction clip's
            // Data1 channel while the clip plays. The installed hit reactions key
            // Data1 mostly negative (CellTakeDmgStab -0.58..0.04, CellTakeDmgMand
            // -0.52..0.04), so with the -200 factor the direction points from the
            // attacker to the victim. INFERRED: the orientation of the contact normal
            // retail passes (the victim is pushed away).
            // A zap passes the zero vector (0xE71520: event +0x48 is the global zero): no push.
            if let (true, Some(mut m)) = (normal, mv) {
                m.knock = (ov.plane(tv) - oa.plane(ta)).normalize_or_zero();
            }
            return true;
        }
    };
    if killed {
        // 0xE7A4A0 (death of a non-avatar cell) -> 0xE71F00.
        friend_fear(db, objects, victim, attacker, player_scale, difficulty, world_scale, FriendFear::Death);
    }
    true
}

/// Which friend reaction `friend_fear` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FriendFear {
    /// `Damage` -> 0xE71CE0.
    Damage,
    /// The death routine -> 0xE71F00.
    Death,
}

/// The friends of a hurt or dying cell react (VERIFIED 0xE71CE0 / 0xE71F00).
/// The victim's AI block gives the radii (block offsets, ModAPI
/// `cCellResource`): +0x28 `fearsNearbyDamageRadius` / +0x30 time and +0x38
/// `protectRadius` / +0x3C time (0xE71CE0), +0x2C `fearsNearbyDeathRadius` /
/// +0x34 time (0xE71F00; the earlier note had read the protect pair as the
/// death pair). `FindCellsInRadius` (0xE87210) takes every cell whose sphere
/// overlaps the search sphere, radius realSize[victim scale] / worldScale /
/// 30 x R (a rescale of R by the victim's level, kept as retail has it).
/// Damage: a cell for which ShouldNotAttack(victim, other) holds (a friend or
/// the same cell file) whose surface distance (centres minus both sizes) is
/// below the damage radius flees the attacker for the damage time, else below
/// the protect radius turns on the attacker for the protect time. Death: every
/// other cell of the same cell file in the sphere flees the killer for the
/// death time.
#[allow(clippy::too_many_arguments)]
fn friend_fear(
    db: &CellDatabase,
    objects: &mut Objects,
    victim: Entity,
    attacker: Entity,
    player_scale: CellScale,
    difficulty: usize,
    world_scale: f32,
    which: FriendFear,
) {
    let Ok((_, tv, ov, ..)) = objects.get(victim) else {
        return;
    };
    let Some(record) = db.cell(ov.cell) else {
        return;
    };
    let ai = crate::cell_ai::ai_block(record, difficulty).clone();
    let reach = match which {
        FriendFear::Damage => ai.fears_nearby_damage_radius.max(ai.protect_radius),
        FriendFear::Death => ai.fears_nearby_death_radius,
    };
    if reach == 0.0 {
        return;
    }
    let (v_cell, v_scale, v_dead, v_radius, v_pos) = (ov.cell, ov.scale, ov.dead, ov.radius, ov.plane(tv));
    let level = if v_scale.is_none() { player_scale } else { v_scale };
    let broad = crate::cell_engine::real_size(level) / 30.0 / world_scale.max(1e-6) * reach * UNIT;
    for (entity, t, o, _, mut anim, _, st) in objects.iter_mut() {
        if entity == victim || o.background {
            continue;
        }
        let Some(mut st) = st else {
            continue;
        };
        let distance = o.plane(t).distance(v_pos);
        if distance >= broad + o.radius {
            continue;
        }
        match which {
            FriendFear::Death => {
                if o.cell == v_cell {
                    crate::cell_ai::start_flee(&mut st, anim.as_deref_mut(), attacker, ai.fears_nearby_death_time);
                }
            }
            FriendFear::Damage => {
                if !should_not_attack(db, v_cell, v_scale, v_dead, o.cell, o.kind == CellKind::Player, o.dead, player_scale) {
                    continue;
                }
                let surface = (distance - v_radius - o.radius).max(0.0) / UNIT;
                if surface < ai.fears_nearby_damage_radius {
                    crate::cell_ai::start_flee(&mut st, anim.as_deref_mut(), attacker, ai.fears_nearby_damage_time);
                } else if surface < ai.protect_radius {
                    crate::cell_ai::start_chase(&mut st, anim.as_deref_mut(), attacker, ai.protect_time);
                }
            }
        }
    }
}

/// `EatKill` (0xE7AEE0): the bitten cell breaks apart.
#[allow(clippy::too_many_arguments)]
fn eat_kill(
    commands: &mut Commands,
    objects: &mut Objects,
    clock: &PlayClock,
    attacker: Entity,
    victim: Entity,
    modelled: bool,
    run: &mut RunProgress,
    effects: &mut MessageWriter<CellEffectRequest>,
) {
    let Ok([(_, _, oa, ..), (_, tv, mut ov, cv, mut anim, ..)]) = objects.get_many_mut([attacker, victim]) else {
        return;
    };
    let at = ov.plane(tv);
    effects.write(CellEffectRequest {
        effect: crate::world_objects::name_id("cell_death_break"),
        position: Vec3::new(at.x, 0.0, at.y),
        attach_to: None,
        scale: ov.radius,
    });
    let by_player = oa.kind == CellKind::Player;
    let avatar = ov.kind == CellKind::Player;
    let clip = clock.seconds(&ov, anim_code::DEATH_BROKEN, 0).unwrap_or(DYING_SECONDS);
    if let Some(mut c) = cv {
        c.bitten = true;
        c.last_attacker = Some(attacker);
        c.killed_by_player = by_player;
        c.death = DeathKind::Broken;
        if avatar {
            // Event 5 holds the avatar (0xE59260: +0x178 / +0x17B / +0x17C) for the clip
            // of request 8; `resolve_deaths` starts its death flow (request 9, the
            // Break 0xE7ADD0 -> 0xE72200 path) when that clip is over.
            c.hit_cooldown = c.hit_cooldown.max(clip);
            c.pending_death = Some(clip);
        }
    }
    if avatar {
        ov.hp = 0;
        play(&mut anim, anim_code::DEATH_BROKEN, CellAnimation::CellDmgSolidDeath);
        return;
    }
    play(&mut anim, anim_code::DEATH_BROKEN, CellAnimation::CellDmgSolidDeathNpc);
    if by_player {
        run.cell.count_kill();
    }
    ov.hp = 0;
    if modelled {
        // State 8 for the clip (event 5), then Break (`dying`).
        ov.passive = true;
        commands.entity(victim).insert(CellDying { timer: clip });
    } else {
        // Without a model: Break at once.
        ov.dead = true;
    }
}

/// A cell playing its break death (request 8, event 5) before Break.
#[derive(Component, Clone, Copy, Debug)]
pub struct CellDying {
    pub timer: f32,
}

/// Event 5 duration: the clip of request 8 (AAAA0079 for NPCs, 0.133 s x the
/// dilation factor); this is the fallback while the clip table is not loaded.
pub const DYING_SECONDS: f32 = 0.6;

/// Count the break deaths down; at the end the cell breaks (resolve_deaths
/// drops `solid_pieces`).
pub fn dying(
    time: Res<Time>,
    mut commands: Commands,
    mut cells: Query<(Entity, &mut CellObject, &mut CellDying)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, mut object, mut dying) in &mut cells {
        dying.timer -= dt;
        if dying.timer <= 0.0 {
            object.dead = true;
            commands.entity(entity).remove::<CellDying>();
        }
    }
}

/// `PlayerAte` (0xE7D070) -> `AddFood` (0xE7CE10).
fn player_ate(
    record: Option<&CellRecord>,
    mouth: u8,
    player: &mut CellObject,
    run: &mut RunProgress,
    flow: &mut crate::cell_flow::CellFlow,
    difficulty: usize,
) {
    if player.hp < MAX_HP {
        let heal = match record {
            Some(d) if d.eat.hp_value > 0 => d.eat.hp_value * if difficulty == 0 { 2 } else { 1 },
            Some(_) => 0,
            None => 2,
        };
        player.hp = (player.hp + heal).min(MAX_HP);
    }
    let food = record.map_or(10, |d| d.eat.food_value);
    if food == 0 {
        return;
    }
    run.cell.player_has_eaten = true;
    if !run.cell.show_mate_button && run.cell.food_progression > MATE_BUTTON_FOOD {
        flow.mate_countdown = crate::cell_flow::MATE_COUNTDOWN_MEAL;
        run.cell.show_mate_button = true;
    }
    // Tutorial mission 0: eat 5 times.
    let mission = &mut run.cell.missions[0];
    if mission.state < 2 {
        mission.state = 1;
        mission.progress = (mission.progress + 1).min(5);
        if mission.progress >= 5 {
            mission.state = 2;
        }
    }
    let is_plant = match record {
        None => true,
        Some(d) if d.flags & 1 == 0 => d.cell_type == CellType::Liquid,
        Some(d) => mouth == 3 || (mouth != 2 && mouth != 4 && d.cell_type == CellType::Liquid),
    };
    add_food(run, food, is_plant);
}

/// `AddFood` (0xE7CE10): cap 1000, overflow into the "over" counters.
pub fn add_food(run: &mut RunProgress, amount: i32, is_plant: bool) {
    let old = run.cell.food_progression;
    // Part guarantee (RE 10A): checked on the food before the meal.
    if crate::cell_part_guide::due(&run.cell, old) {
        run.cell.part_guide_pending = true;
    }
    let new = (old + amount).min(crate::cell_engine::STAGE_FOOD);
    let over = old + amount - new;
    if over > 0 {
        if is_plant {
            run.cell.over_plant_food_progression += over;
        } else {
            run.cell.over_animal_food_progression += over;
        }
    }
    if new != old {
        if is_plant {
            run.cell.plant_food_progression += new - old;
            run.diet_herbivore += 1.0;
        } else {
            run.diet_carnivore += 1.0;
        }
        run.cell.food_progression = new;
    }
    // 0xE7D070 records `clg_eatplant` / `clg_eatmeat` after AddFood (0xE7CE10), so
    // the value includes the meal.
    run.cell
        .record_event(crate::cell_hud::timeline::events::EventKind::Meal { plant: is_plant });
}

/// NPC meal (0xE79320): growth after `growCount` meals, else digestion.
fn npc_meal(db: &CellDatabase, eater: &mut CellObject, ai: &mut crate::cell_ai::CellAiState, difficulty: usize) {
    let Some(record) = db.cell(eater.cell) else {
        return;
    };
    let a = crate::cell_ai::ai_block(record, difficulty);
    ai.meals += 1;
    if a.grow_count >= 1 && ai.meals == a.grow_count {
        eater.abs_size *= 2.154_434_7;
        ai.meals = 0;
    } else if ai.meals >= a.digestion_count {
        ai.meals = 0;
        if a.digestion_time > 0.0 {
            ai.digest = a.digestion_time;
        } else {
            ai.digest_output = true;
        }
    }
}

/// Loot (0xE771D0): one weighted pick over the eligible entries (a running
/// total; entry i replaces the pick with probability w_i / total), then
/// `count + RandomInt(countDelta)` recursions (table) or spawns (cell) at
/// U[minRadius, maxRadius or the source size] with velocity expelForce,
/// scale clamp(source + levelOffset, 0, 19). `eligible(unlock)` filters
/// part pickups (not yet unlocked, inside the central screen area).
#[allow(clippy::too_many_arguments)]
pub fn roll_loot(
    db: &CellDatabase,
    table: u32,
    scale: CellScale,
    at: Vec2,
    radius: f32,
    rng: &mut impl Rng,
    owned: &dyn Fn(UnlockType) -> bool,
    out: &mut Vec<SpawnRequest>,
    depth: u32,
) {
    let Some(loot) = db.loot_tables.get(&table) else {
        return;
    };
    if depth > 4 {
        return;
    }
    let eligible = |e: &crate::cell_data::CellLootEntry| match e.kind {
        LootEntryType::Cell => db
            .cell(e.cell)
            .is_none_or(|r| r.cell_type != CellType::Unlock || r.unlock_type == UnlockType::None || !owned(r.unlock_type)),
        _ => true,
    };
    let mut total = 0.0f32;
    let mut pick = None;
    for entry in loot.entries.iter().filter(|e| eligible(e)) {
        let w = entry.weight.max(0.0);
        total += w;
        if total > 0.0 && rng.random_range(0.0..total) < w {
            pick = Some(entry);
        }
    }
    let Some(entry) = pick else {
        return;
    };
    let count = entry.count + if entry.count_delta > 0 { rng.random_range(0..entry.count_delta) } else { 0 };
    for _ in 0..count.max(0) {
        match entry.kind {
            LootEntryType::Nothing => {}
            LootEntryType::Table => roll_loot(db, entry.table, scale, at, radius, rng, owned, out, depth + 1),
            LootEntryType::Cell => {
                // Radii are retail units; maxRadius 0 = the source size.
                let max = if loot.max_radius > 0.0 { loot.max_radius * UNIT } else { radius };
                let min = (loot.min_radius * UNIT).min(max);
                let r = if max > min { rng.random_range(min..=max) } else { max };
                let a = rng.random_range(0.0..std::f32::consts::TAU);
                let dir = Vec2::new(a.cos(), a.sin());
                let b = rng.random_range(0.0..std::f32::consts::TAU);
                let level = (scale.index() + entry.level_offset).clamp(0, CellScale::MAX);
                out.push(SpawnRequest {
                    cell: entry.cell,
                    scale: CellScale(level),
                    position: at + dir * r,
                    velocity: Vec2::new(b.cos(), b.sin()) * loot.expel_force * UNIT,
                    heading: rng.random_range(0.0..std::f32::consts::TAU),
                    size_roll: rng.random_range(0.0..1.0),
                    invulnerable: 0.3,
                    role: SpawnRole::Loot,
                    ..default()
                });
            }
        }
    }
}

/// Remove dead objects: death effects and loot (0xE7A4A0); the player's
/// death (0xE5C8E0).
#[allow(clippy::too_many_arguments)]
pub fn resolve_deaths(
    mut commands: Commands,
    db: Res<CellDatabase>,
    mut game: ResMut<CellGame>,
    mut run: ResMut<RunProgress>,
    mut queue: ResMut<CellSpawnQueue>,
    mut flow: ResMut<crate::cell_flow::CellFlow>,
    mut effects: MessageWriter<CellEffectRequest>,
    mut objects: Query<(Entity, &Transform, &mut CellObject, Option<&CellCombat>)>,
    mut player_health: Query<&mut Health, With<Player>>,
) {
    let player_scale = crate::cell_engine::player_scale(run.cell.food_progression);
    let unlocked = run.cell.unlocked_parts;
    let owned = move |u: UnlockType| unlocked.get(u.raw() as usize).is_some_and(|v| *v > 0);
    for (entity, t, mut object, combat) in &mut objects {
        if object.kind == CellKind::Player {
            if let Ok(mut health) = player_health.single_mut() {
                health.max = MAX_HP as f32;
                health.hp = object.hp.max(0) as f32;
            }
            if object.hp <= 0 && !flow.player_dead() && combat.is_none_or(|c| c.pending_death.is_none()) {
                object.dead = false;
                flow.kill_player(combat.is_some_and(|c| c.bitten || c.death == DeathKind::Eaten));
                run.cell.count_death();
                run.request_save();
            }
            continue;
        }
        if !object.dead {
            continue;
        }
        let at = object.plane(t);
        let death = combat.map_or(DeathKind::None, |c| c.death);
        let diff = scale_diff(object.scale, player_scale);
        let effect = match death {
            DeathKind::Killed | DeathKind::Popped => Some(match diff {
                0 | 1 => "cell_death_oneshot_small",
                2 => "cell_death_oneshot",
                _ => "cell_death_oneshot_large",
            }),
            // A modelled victim got its effect at the swallow (0xE7D880).
            DeathKind::Eaten if !combat.is_some_and(|c| c.death_fx_played) => Some("cell_death_eaten"),
            _ => None,
        };
        if let Some(name) = effect {
            effects.write(CellEffectRequest {
                effect: crate::world_objects::name_id(name),
                position: Vec3::new(at.x, -object.depth, at.y),
                attach_to: None,
                scale: object.radius,
            });
        }
        if let Some(record) = db.cell(object.cell) {
            let mut out = Vec::new();
            match death {
                // Loot only for victims at the player's scale or one above.
                DeathKind::Killed | DeathKind::Popped if (2..=3).contains(&diff) && record.loot != 0 => {
                    roll_loot(&db, record.loot, object.scale, at, object.radius, &mut game.rng, &owned, &mut out, 0);
                    // The loot table's own effect plays at the drop (RE 6/9).
                    if let Some(effect) = db.loot_tables.get(&record.loot).map(|t| t.effect).filter(|e| *e != 0) {
                        effects.write(CellEffectRequest {
                            effect,
                            position: Vec3::new(at.x, -object.depth, at.y),
                            attach_to: None,
                            scale: object.radius,
                        });
                    }
                }
                // Break (0xE7ADD0, RE 12): mScale != 0 -> loot table #7
                // `solid_pieces` at clamp(mScale - 1, 0, 19).
                DeathKind::Broken if object.scale.index() > 0 => {
                    let pieces = crate::world_objects::name_id("solid_pieces");
                    let scale = CellScale((object.scale.index() - 1).clamp(0, CellScale::MAX));
                    roll_loot(&db, pieces, scale, at, object.radius, &mut game.rng, &owned, &mut out, 0);
                }
                _ => {}
            }
            queue.0.extend(out);
        }
        if game.unlock_pickup == Some(entity) {
            game.unlock_pickup = None;
        }
        commands.entity(entity).despawn();
    }
}

/// Copy the runtime Cell state into the save every frame (cheap).
pub fn sync_save(
    time: Res<Time>,
    game: Res<CellGame>,
    flow: Res<crate::cell_flow::CellFlow>,
    mut run: ResMut<RunProgress>,
    player: Query<(&Transform, &CellObject), With<Player>>,
) {
    if flow.phase == crate::cell_flow::FlowPhase::Advancing {
        // The run already moved on to the Creature stage.
        return;
    }
    let save = &mut run.cell;
    save.game_time_ms += (time.delta_secs() * 1000.0) as i64;
    save.world_scale = game.world_scale;
    save.origin_abs = game.origin_abs.to_array();
    save.zoom = game.zoom;
    save.difficulty = game.difficulty as i32;
    if let Ok((t, o)) = player.single() {
        save.player_position = [t.translation.x, t.translation.z];
        save.player_heading = o.heading;
        save.player_hp = o.hp;
        if o.velocity.length_squared() > 1e-6 {
            save.player_has_moved = true;
        }
    }
    let dna = save.food_progression as f32 / 10.0;
    if (run.dna_this_stage - dna).abs() > 1e-3 {
        run.dna_this_stage = dna;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_table_matches_the_retail_routine() {
        let p = CellScale(3);
        assert_eq!(damage_amount(true, p, false, CellScale(2), p, 1), 6);
        assert_eq!(damage_amount(true, p, false, CellScale(3), p, 1), 3);
        assert_eq!(damage_amount(true, p, false, CellScale(3), p, 2), 2);
        assert_eq!(damage_amount(true, p, false, CellScale(5), p, 1), 1);
        assert_eq!(damage_amount(false, CellScale(2), true, p, p, 1), 1);
        assert_eq!(damage_amount(false, CellScale(3), true, p, p, 1), 2);
        assert_eq!(damage_amount(false, CellScale(4), true, p, p, 1), 3);
        assert_eq!(damage_amount(false, CellScale(9), true, p, p, 1), 6);
        assert_eq!(damage_amount(false, CellScale(2), false, CellScale(3), p, 1), 1);
        assert_eq!(damage_amount(false, CellScale(1), false, CellScale(1), p, 1), 0);
        assert_eq!(damage_amount(false, CellScale(4), false, CellScale(1), p, 1), 6);
        assert_eq!(damage_amount(false, CellScale(3), false, CellScale(3), p, 1), 2);
    }

    #[test]
    fn dna_follows_the_scale_info_column() {
        assert_eq!(dna_earned(0), 0);
        assert_eq!(dna_earned(25), 3, "halfway to 5, rounded");
        assert_eq!(dna_earned(50), 5);
        assert_eq!(dna_earned(100), 10);
        assert_eq!(dna_earned(150), 13);
        assert_eq!(dna_earned(1000), 65);
    }

    #[test]
    fn add_food_caps_and_tracks_the_diet() {
        let mut run = RunProgress::default();
        add_food(&mut run, 12, true);
        assert_eq!((run.cell.food_progression, run.cell.plant_food_progression), (12, 12));
        run.cell.food_progression = 995;
        add_food(&mut run, 10, false);
        assert_eq!(run.cell.food_progression, 1000);
        assert_eq!(run.cell.over_animal_food_progression, 5);
    }

    #[test]
    fn meals_heal_count_and_start_the_mate_countdown() {
        let mut run = RunProgress::default();
        let mut flow = crate::cell_flow::CellFlow::default();
        let mut player = CellObject {
            cell: 0,
            kind: CellKind::Player,
            scale: CellScale(1),
            abs_size: 1.0,
            radius: 1.0,
            size_vel: 0.0,
            hp: 3,
            density: 50.0,
            mass: 50.0,
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
        let mut plant = CellRecord::default();
        plant.cell_type = CellType::Liquid;
        plant.eat.food_value = 12;
        plant.eat.hp_value = 1;
        player_ate(Some(&plant), 3, &mut player, &mut run, &mut flow, 0);
        assert_eq!(player.hp, 5, "hpValue x2 on easy");
        assert_eq!(run.cell.food_progression, 12);
        assert_eq!(run.cell.plant_food_progression, 12);
        assert_eq!(run.cell.missions[0].progress, 1);
        assert!(!run.cell.show_mate_button);
        run.cell.food_progression = 150;
        player_ate(Some(&plant), 3, &mut player, &mut run, &mut flow, 1);
        assert_eq!(player.hp, 6, "capped at 6");
        assert!(run.cell.show_mate_button);
        assert_eq!(flow.mate_countdown, crate::cell_flow::MATE_COUNTDOWN_MEAL);
        // A flag-1 cell eaten by a jaw counts as meat.
        let mut chunk = plant.clone();
        chunk.flags = 1;
        let plant_before = run.cell.plant_food_progression;
        player_ate(Some(&chunk), 2, &mut player, &mut run, &mut flow, 1);
        assert_eq!(run.cell.plant_food_progression, plant_before);
    }

    fn side(cell_type: CellType, kind: CellKind, scale: i32) -> Side {
        Side {
            entity: World::new().spawn_empty().id(),
            cell_type,
            kind,
            avatar: kind == CellKind::Player,
            dead: false,
            flags: 0,
            living: kind.is_creature(),
            scale_diff: 2,
            scale: CellScale(scale),
            eggkid: false,
            caps: CellCaps::default(),
        }
    }

    #[test]
    fn classify_follows_the_retail_contact_table() {
        let player = side(CellType::Solid, CellKind::Player, 2);
        let plant = side(CellType::Liquid, CellKind::PlantFood, 1);
        let bubble = side(CellType::Air, CellKind::Bubble, 2);
        let unlock = side(CellType::Unlock, CellKind::Unlock, 3);
        let prey = side(CellType::Solid, CellKind::Creature, 1);
        let peer = side(CellType::Solid, CellKind::Creature, 2);
        let big = side(CellType::Solid, CellKind::Creature, 3);
        // Filter mouth swallows plants one level smaller.
        assert_eq!(classify(&player, 3, &plant, PART_BODY, 1, false), 9);
        assert_eq!(classify(&player, 3, &plant, PART_BODY, 2, false), 1);
        // Jaw: swallow smaller prey, bite peers, bounce off bigger cells.
        assert_eq!(classify(&player, 2, &prey, PART_BODY, 1, false), 10);
        assert_eq!(classify(&player, 2, &peer, PART_BODY, 2, false), 7);
        assert_eq!(classify(&player, 2, &peer, 2, 2, false), 0xF, "jaw clash");
        assert_eq!(classify(&player, 2, &big, PART_BODY, 3, false), 4);
        // Spikes stab peers; bubbles pop; the avatar collects unlocks.
        assert_eq!(classify(&player, 7, &peer, PART_BODY, 2, false), 8);
        assert_eq!(classify(&player, PART_BODY, &bubble, PART_BODY, 2, false), 3);
        assert_eq!(classify(&player, PART_BODY, &unlock, PART_BODY, 3, false), 0x12);
        assert_eq!(classify(&prey, PART_BODY, &unlock, PART_BODY, 3, false), 1);
        // Flagella never touch; poison clouds hurt unless much smaller.
        assert_eq!(classify(&player, 9, &peer, PART_BODY, 2, false), 0);
        let cloud = side(CellType::Poison, CellKind::Poison, 2);
        assert_eq!(classify(&prey, PART_BODY, &cloud, PART_BODY, 3, false), 0x11);
        assert_eq!(classify(&big, PART_BODY, &cloud, PART_BODY, 0, false), 0x16);
        assert_eq!(classify(&player, PART_BODY, &cloud, PART_BODY, 2, true), 1, "own cloud");
    }

    #[test]
    fn loot_picks_one_entry_and_skips_owned_parts() {
        use crate::cell_data::{CellLootEntry, CellLootTable};
        let mut db = CellDatabase::default();
        let mut part = CellRecord::default();
        part.cell_type = CellType::Unlock;
        part.unlock_type = UnlockType::Spike;
        db.cells.insert(0x100, part);
        db.loot_tables.insert(
            0x200,
            CellLootTable {
                entries: vec![CellLootEntry {
                    kind: LootEntryType::Cell,
                    cell: 0x100,
                    table: 0,
                    weight: 1.0,
                    count: 1,
                    count_delta: 0,
                    level_offset: 0,
                }],
                ..default()
            },
        );
        let mut rng = crate::cell_engine::keyed_rng(&[1]);
        let mut out = Vec::new();
        roll_loot(&db, 0x200, CellScale(2), Vec2::ZERO, 1.0, &mut rng, &|_| false, &mut out, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].scale, CellScale(2));
        out.clear();
        roll_loot(&db, 0x200, CellScale(2), Vec2::ZERO, 1.0, &mut rng, &|u| u == UnlockType::Spike, &mut out, 0);
        assert!(out.is_empty(), "an unlocked part is not eligible");
    }

    #[test]
    fn poison_clouds_leave_at_the_emit_point_of_the_first_part_per_side() {
        // The side comes from the part, the cloud from its emit point.
        let parts = [
            (Vec2::new(-0.6, 0.0), Vec2::new(-0.6, -0.3)),
            (Vec2::new(-0.5, 0.2), Vec2::new(-0.5, 0.0)),
            (Vec2::new(0.6, 0.0), Vec2::new(0.6, -0.3)),
        ];
        assert_eq!(poison_emit_pairs(&parts), vec![Vec2::new(-0.6, -0.3), Vec2::new(0.6, -0.3)]);
    }

    #[test]
    fn poison_clouds_leave_from_one_poison_part_per_side() {
        let local = [
            Vec2::new(-0.6, -0.5),
            Vec2::new(0.6, -0.5),
            Vec2::new(-0.5, 0.2),
            Vec2::new(0.0, -0.9),
            Vec2::new(0.01, 0.4),
        ];
        let points = poison_emit_points(&local);
        assert_eq!(points, vec![Vec2::new(-0.6, -0.5), Vec2::new(0.6, -0.5), Vec2::new(0.0, -0.9)]);
        assert!(poison_emit_points(&[]).is_empty());
    }

    #[test]
    fn part_spheres_pick_the_touching_parts() {
        use crate::cell_engine::PartSphere;
        let spheres = [
            PartSphere {
                kind: 2,
                center: Vec2::new(0.0, -0.9),
                radius: 0.3,
                axis: Vec2::new(0.0, -1.0),
            },
            PartSphere {
                kind: 7,
                center: Vec2::new(0.0, 0.9),
                radius: 0.3,
                axis: Vec2::new(0.0, 1.0),
            },
        ];
        // Heading 0 faces -Z: the jaw is in front, the spike behind.
        let front = sphere_parts(&spheres, Vec2::ZERO, 0.0, 1.0, Vec2::new(0.0, -1.5), 0.4);
        assert!(front.contains(&2) && !front.contains(&7) && front.contains(&PART_BODY));
        let back = sphere_parts(&spheres, Vec2::ZERO, 0.0, 1.0, Vec2::new(0.0, 1.5), 0.4);
        assert!(back.contains(&7) && !back.contains(&2));
        // Turned half a circle, the jaw faces +Z.
        let turned = sphere_parts(&spheres, Vec2::ZERO, std::f32::consts::PI, 2.0, Vec2::new(0.0, 2.6), 0.4);
        assert!(turned.contains(&2));
    }

    #[test]
    fn electric_values_follow_the_ai_block() {
        let mut db = CellDatabase::default();
        let mut r = CellRecord::default();
        r.ai.kind = AiType::Eater;
        r.ai.electric_recharge = 10.0;
        r.ai.electric_recharge_vs_small = -1.0;
        r.ai.arc_length = 3.0;
        r.ai.num_arcs = 2;
        db.cells.insert(5, r);
        let mut o = cell_object(CellKind::Creature);
        o.cell = 5;
        let caps = CellCaps {
            electric_jumps: 3,
            electric_range: 1.0,
            ..default()
        };
        // An NPC chains `numArcs` times, the avatar / a Parts cell by its part graph.
        let (long, short, range, jumps) = electric_values(&db, &o, &caps, false, 1, 0.5).unwrap();
        assert_eq!((long, short, jumps), (10.0, 10.0, 2));
        assert_eq!(electric_values(&db, &o, &caps, true, 1, 0.5).unwrap().3, 3);
        // range = arcLength * realSize[_1] / (worldScale * 60) world units.
        assert!((range - 3.0 * UNIT).abs() < 1e-4, "{range}");
        db.cells.get_mut(&5).unwrap().ai.electric_recharge = -1.0;
        assert!(electric_values(&db, &o, &CellCaps::default(), false, 1, 0.5).is_none(), "no electric");
    }

    fn cell_object(kind: CellKind) -> CellObject {
        CellObject {
            cell: 0,
            kind,
            scale: CellScale(1),
            abs_size: 1.0,
            radius: 1.0,
            size_vel: 0.0,
            hp: 3,
            density: 50.0,
            mass: 50.0,
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
        }
    }

    #[test]
    fn save_defaults_and_old_saves_migrate() {
        let save = CellSave::default();
        assert_eq!(save.difficulty, 1);
        let text = serde_json::to_string(&save).unwrap();
        let back: CellSave = serde_json::from_str(&text).unwrap();
        assert_eq!(back, save);
        let partial: CellSave = serde_json::from_str(r#"{"food_progression": 120}"#).unwrap();
        assert_eq!((partial.food_progression, partial.difficulty), (120, 1));
        let mut run = RunProgress {
            dna_this_stage: 42.0,
            cell_found_parts: vec!["spike".into(), "poison".into()],
            ..default()
        };
        migrate_save(&mut run);
        assert_eq!(run.cell.food_progression, 420);
        assert!(run.cell.unlocked(UnlockType::Spike));
        assert!(run.cell.unlocked(UnlockType::Chemical));
        assert!(!run.cell.unlocked(UnlockType::Jet));
        run.dna_this_stage = 90.0;
        migrate_save(&mut run);
        assert_eq!(run.cell.food_progression, 420, "migration runs once");
    }

    #[test]
    fn unlocking_records_the_slot_mission_and_mate_timer() {
        let mut run = RunProgress::default();
        let mut flow = crate::cell_flow::CellFlow::default();
        assert!(record_unlock(&mut run, &mut flow, UnlockType::Jet));
        assert!(!record_unlock(&mut run, &mut flow, UnlockType::Jet), "already unlocked");
        assert_eq!(run.cell.unlocked_parts[10], 2);
        assert_eq!(run.cell_found_parts, vec!["jet".to_string()]);
        assert_eq!(run.cell.unlocked_count(), 1);
        assert!(run.cell.show_mate_button);
        assert_eq!(flow.mate_countdown, crate::cell_flow::MATE_COUNTDOWN_UNLOCK);
        assert_eq!(run.cell.missions[1].progress, 2);
    }

    /// Friend / foe records for the gating tests: cells 10 and 11 share
    /// friendGroup 21 (retail: chomper / stabber / poisoner), 12 has none.
    fn friends_db() -> CellDatabase {
        let mut db = CellDatabase::default();
        for (id, group) in [(10u32, 21), (11, 21), (12, 0), (13, 21)] {
            let mut r = CellRecord::default();
            r.cell_type = CellType::Solid;
            r.hp = 6;
            r.friend_group = group;
            r.ai.kind = AiType::Eater;
            db.cells.insert(id, r);
        }
        db
    }

    #[test]
    fn friends_and_own_species_refuse_to_attack_each_other() {
        let db = friends_db();
        let p = CellScale(2);
        let mut a = cell_object(CellKind::Creature);
        a.cell = 10;
        a.scale = p;
        let mut same_group = cell_object(CellKind::Creature);
        same_group.cell = 11;
        same_group.scale = p;
        let mut foe = cell_object(CellKind::Creature);
        foe.cell = 12;
        foe.scale = p;
        let mut same_file = cell_object(CellKind::Creature);
        same_file.cell = 10;
        same_file.scale = p;
        // Bite / stab / swallow / drain fall back to a plain contact against a
        // friend or a cell of the same file, and go through against a foe.
        for kind in [7u8, 8, 9, 10, 0xC, 0xD] {
            assert!(refuses_attack(&db, kind, &a, false, &same_group, false, false, p), "kind {kind}");
            assert!(refuses_attack(&db, kind, &a, false, &same_file, false, false, p), "kind {kind}");
            assert!(!refuses_attack(&db, kind, &a, false, &foe, false, false, p), "kind {kind}");
        }
        // Other kinds are not attacks (contacts, pops, clouds, pickups).
        assert!(!refuses_attack(&db, 1, &a, false, &same_group, false, false, p));
        assert!(!refuses_attack(&db, 0x11, &a, false, &same_group, false, false, p));
        // A corpse may be attacked by anyone (field_112).
        assert!(!refuses_attack(&db, 10, &a, false, &same_group, false, true, p));
        // An invulnerable victim is not swallowed (+0x111) but may be bitten.
        let mut shielded = foe.clone();
        shielded.invulnerable = 0.3;
        assert!(refuses_attack(&db, 10, &a, false, &shielded, false, false, p));
        assert!(refuses_attack(&db, 9, &a, false, &shielded, false, false, p));
        assert!(!refuses_attack(&db, 7, &a, false, &shielded, false, false, p));
    }

    #[test]
    fn small_cells_that_will_not_attack_the_player_leave_it_alone() {
        let mut db = friends_db();
        db.cells.get_mut(&12).unwrap().wont_attack_player_when_small = true;
        let p = CellScale(3);
        let mut small = cell_object(CellKind::Creature);
        small.cell = 12;
        small.scale = CellScale(2); // ScaleDiff == Smaller
        let mut avatar = cell_object(CellKind::Player);
        avatar.cell = 13;
        avatar.scale = p;
        assert!(refuses_attack(&db, 8, &small, false, &avatar, true, false, p));
        let mut peer = small.clone();
        peer.scale = p; // same size: the flag only covers Smaller
        assert!(!refuses_attack(&db, 8, &peer, false, &avatar, true, false, p));
    }

    #[test]
    fn the_hit_lock_is_the_reaction_clip_and_a_bite_lands_at_contact() {
        use bevy::ecs::system::RunSystemOnce;
        let player = CellScale(2);
        let mut world = World::new();
        world.insert_resource(friends_db());
        let mut avatar = cell_object(CellKind::Player);
        avatar.cell = 12;
        avatar.scale = player;
        avatar.hp = 6;
        let victim = world.spawn((Transform::default(), avatar.clone(), CellCombat::default())).id();
        let mut biter = cell_object(CellKind::Creature);
        biter.cell = 10;
        biter.scale = player;
        let attacker = world.spawn((Transform::default(), biter, CellCombat::default())).id();
        // Row 18 = request 16 (stab reaction, avatar): 1.233 s x 0.75 for the avatar.
        // Row 8 = request 5 (bite, avatar): 1.1 s with `contact` at 0.13 s.
        let clips = CellClipTimes::for_test(&[(18, 1.233, &[]), (8, 1.1, &[(0.13, 2, crate::cell_anim::CONTACT_EVENT)])]);
        let art = CellArtCache::default();
        let clock = PlayClock { clips: &clips, art: &art, player };
        assert!((clock.seconds(&avatar, 16, 0).unwrap() - 0.92475).abs() < 1e-4);
        assert!((clock.contact(&avatar, 5, 0).unwrap() - 0.0975).abs() < 1e-5);
        assert!((clock.seconds(&avatar, 5, 0).unwrap() - 0.825).abs() < 1e-5);
        // Without the table nothing answers and the fallbacks run.
        let none = CellClipTimes::default();
        assert_eq!(PlayClock { clips: &none, art: &art, player }.seconds(&avatar, 16, 0), None);
        let lock = |world: &mut World, with_clips: bool| {
            let clips = if with_clips { CellClipTimes::for_test(&[(18, 1.233, &[])]) } else { CellClipTimes::default() };
            world
                .run_system_once(move |db: Res<CellDatabase>, mut objects: Objects| {
                    let art = CellArtCache::default();
                    let clock = PlayClock { clips: &clips, art: &art, player };
                    let mut run = RunProgress::default();
                    damage(&db, &mut objects, &clock, attacker, victim, DAMAGE_STAB, player, 1, 0.5, &mut run)
                })
                .unwrap()
        };
        assert!(lock(&mut world, true), "an equal-size NPC hurts the avatar");
        let held = world.get::<CellCombat>(victim).unwrap().hit_cooldown;
        assert!((held - 0.92475).abs() < 1e-4, "reaction clip x 0.75, got {held}");
        assert!(!lock(&mut world, true), "no second hit inside the reaction");
        world.get_mut::<CellCombat>(victim).unwrap().hit_cooldown = 0.0;
        assert!(lock(&mut world, false));
        assert_eq!(world.get::<CellCombat>(victim).unwrap().hit_cooldown, HIT_COOLDOWN, "fallback without clips");
        // A cell in a hit reaction or inside an attack may not start another action.
        let mut combat = CellCombat::default();
        assert!(!combat.acting_locked());
        combat.hit_cooldown = 0.5;
        assert!(combat.acting_locked());
        combat.hit_cooldown = 0.0;
        combat.busy = 0.5;
        assert!(combat.acting_locked());
    }

    #[test]
    fn damage_needs_a_living_foe_victim() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(friends_db());
        let spawn = |world: &mut World, cell: u32, hp: i32| {
            let mut o = cell_object(CellKind::Creature);
            o.cell = cell;
            o.scale = CellScale(2);
            o.hp = hp;
            world.spawn((Transform::default(), o, CellCombat::default())).id()
        };
        let a = spawn(&mut world, 10, 6);
        let friend = spawn(&mut world, 11, 6);
        let foe = spawn(&mut world, 12, 6);
        let hit = |world: &mut World, attacker: Entity, victim: Entity| -> bool {
            world
                .run_system_once(move |db: Res<CellDatabase>, mut objects: Objects| {
                    let mut run = RunProgress::default();
                    let clips = CellClipTimes::default();
                    let art = CellArtCache::default();
                    let clock = PlayClock { clips: &clips, art: &art, player: CellScale(2) };
                    damage(&db, &mut objects, &clock, attacker, victim, DAMAGE_STAB, CellScale(2), 1, 0.5, &mut run)
                })
                .unwrap()
        };
        assert!(!hit(&mut world, a, friend), "same friendGroup: no damage");
        assert_eq!(world.get::<CellObject>(friend).unwrap().hp, 6);
        assert!(hit(&mut world, foe, a), "a foe hurts");
        assert_eq!(world.get::<CellObject>(a).unwrap().hp, 4, "equal-size NPC hit = 2");
        // A victim that is not Living (no AI: a meat chunk or a rock) takes none.
        world.resource_mut::<CellDatabase>().cells.get_mut(&11).unwrap().ai.kind = AiType::Player;
        world.resource_mut::<CellDatabase>().cells.get_mut(&11).unwrap().friend_group = 0;
        assert!(!hit(&mut world, foe, friend), "no AI: not Living");
    }

    #[test]
    fn a_zap_hurts_a_friend_holds_it_for_the_reaction_and_kills_at_the_clips_end() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(friends_db());
        let spawn = |world: &mut World, cell: u32| {
            let mut o = cell_object(CellKind::Creature);
            o.cell = cell;
            o.scale = CellScale(2);
            o.hp = 6;
            world.spawn((Transform::default(), o, CellCombat::default(), crate::cell_motion::CellMotion::default())).id()
        };
        let zapper = spawn(&mut world, 10);
        let friend = spawn(&mut world, 11);
        let hit = |world: &mut World, gate: DamageGate| -> bool {
            world
                .run_system_once(move |db: Res<CellDatabase>, mut objects: Objects| {
                    let clips = CellClipTimes::for_test(&[(18, 1.233, &[])]);
                    let art = CellArtCache::default();
                    let clock = PlayClock { clips: &clips, art: &art, player: CellScale(2) };
                    let mut run = RunProgress::default();
                    damage_with(gate, &db, &mut objects, &clock, zapper, friend, DAMAGE_ELECTRIC, CellScale(2), 1, 0.5, &mut run)
                })
                .unwrap()
        };
        assert!(!hit(&mut world, DamageGate::Normal), "a friend is not hurt by a normal hit");
        assert!(hit(&mut world, DamageGate::Zap), "the zap has no friend test (0xE71520)");
        assert_eq!(world.get::<CellObject>(friend).unwrap().hp, 4, "GetDamageAmount: equal-size NPC = 2");
        // Event 2 holds the target for its reaction clip (an NPC without a model: PlayAnimation
        // answers 1.0, 0xE6D208) and a zap pushes nothing (the event's vector is zero).
        assert_eq!(world.get::<CellCombat>(friend).unwrap().hit_cooldown, 1.0);
        assert_eq!(world.get::<crate::cell_motion::CellMotion>(friend).unwrap().knock, Vec2::ZERO);
        assert!(world.get::<CellCombat>(friend).unwrap().last_attacker.is_some());
        // A zap that takes the last hit points does not kill at once: 0xE7A770 runs the
        // death routine when the reaction clip ends.
        world.get_mut::<CellObject>(friend).unwrap().hp = 2;
        assert!(hit(&mut world, DamageGate::Zap));
        let o = world.get::<CellObject>(friend).unwrap();
        assert_eq!((o.hp, o.dead), (0, false));
        let c = world.get::<CellCombat>(friend).unwrap();
        assert_eq!((c.pending_death, c.death), (Some(1.0), DeathKind::Killed));
    }

    #[test]
    fn the_food_moving_earns_dna_once() {
        let mut run = RunProgress::default();
        let before = run.cell.food_progression;
        assert_eq!(food_moved(&mut run, before), None, "no meal, nothing to do");
        run.cell.food_progression = before + 1000;
        let dna = run.dna_banked_total;
        let changed = food_moved(&mut run, before);
        assert!(run.dna_banked_total > dna, "the meal earned DNA");
        assert!(changed.is_some(), "a thousand food crosses a level");
        assert_eq!(run.dna_this_stage, run.cell.food_progression as f32 / 10.0);
    }

    #[test]
    fn spheres_touch_at_the_midpoint_of_their_surfaces() {
        // 0xE73390: (cB + cP + n (rP - rB)) / 2.
        let p = touch_point(Vec2::ZERO, 1.0, Vec2::new(1.5, 0.0), 0.5);
        assert!(p.abs_diff_eq(Vec2::new(1.0, 0.0), 1e-6), "{p}");
        // Equal spheres: the middle; coincident centres: the centre.
        assert!(touch_point(Vec2::ZERO, 1.0, Vec2::new(1.0, 0.0), 1.0).abs_diff_eq(Vec2::new(0.5, 0.0), 1e-6));
        assert_eq!(touch_point(Vec2::ONE, 1.0, Vec2::ONE, 2.0), Vec2::ONE);
    }

    #[test]
    fn a_zap_reaches_a_sphere_within_range_of_its_surface() {
        // (r + range)^2 > d^2; the distance is to the surface.
        assert_eq!(zap_reach(Vec2::ZERO, 2.0, Vec2::new(2.5, 0.0), 1.0), Some(1.5));
        assert!((zap_reach(Vec2::ZERO, 2.0, Vec2::new(2.9, 0.0), 1.0).unwrap() - 1.9).abs() < 1e-5);
        assert_eq!(zap_reach(Vec2::ZERO, 2.0, Vec2::new(3.0, 0.0), 1.0), None, "exactly at range: no");
        assert_eq!(zap_reach(Vec2::ZERO, 2.0, Vec2::new(4.0, 0.0), 1.0), None);
        assert_eq!(zap_reach(Vec2::ZERO, 2.0, Vec2::new(0.5, 0.0), 1.0), Some(0.0), "inside: distance 0");
    }

    #[test]
    fn a_torn_off_leak_is_attached_to_its_parent_at_the_point() {
        let parent = World::new().spawn_empty().id();
        // Parent at (4, 0), radius 2, heading 0 (faces -Z); the point is on its +X side.
        let r = leak_request(7, CellScale(3), Vec2::new(6.0, 0.0), parent, Vec2::new(4.0, 0.0), 0.0, 2.0);
        assert_eq!(r.cell, 7);
        assert_eq!(r.scale, CellScale(3));
        assert!((r.size_factor - 2.0 / UNIT).abs() < 1e-5, "the parent's size scales the leak");
        let a = r.attach.unwrap();
        assert!(a.local.abs_diff_eq(Vec2::new(1.0, 0.0), 1e-5), "{}", a.local);
        // Turned a quarter circle the point 2 below it lies on the parent's -X side
        // (the same turn `leak_pose` applies on the way back).
        let q = leak_request(7, CellScale(3), Vec2::new(4.0, 2.0), parent, Vec2::new(4.0, 0.0), std::f32::consts::FRAC_PI_2, 2.0);
        assert!(q.attach.unwrap().local.abs_diff_eq(Vec2::new(-1.0, 0.0), 1e-5), "{}", q.attach.unwrap().local);
        let (back, _) = crate::cell_ai::leak_pose(Vec2::new(4.0, 0.0), std::f32::consts::FRAC_PI_2, 2.0, &q.attach.unwrap());
        assert!(back.abs_diff_eq(Vec2::new(4.0, 2.0), 1e-4), "{back}");
    }

    #[test]
    fn friends_of_a_damaged_cell_flee_or_protect_and_the_same_species_flees_its_death() {
        use bevy::ecs::system::RunSystemOnce;
        let mut db = friends_db();
        {
            // Cell 10: damage radius 4 / time 3, protect radius 8 / time 5, death radius 6 / time 2.
            let r = db.cells.get_mut(&10).unwrap();
            r.ai.fears_nearby_damage_radius = 4.0;
            r.ai.fears_nearby_damage_time = 3.0;
            r.ai.protect_radius = 8.0;
            r.ai.protect_time = 5.0;
            r.ai.fears_nearby_death_radius = 6.0;
            r.ai.fears_nearby_death_time = 2.0;
        }
        let mut world = World::new();
        world.insert_resource(db);
        let spawn = |world: &mut World, cell: u32, x: f32| {
            let mut o = cell_object(CellKind::Creature);
            o.cell = cell;
            o.scale = CellScale(2);
            o.radius = 0.5 * UNIT;
            let ai = crate::cell_ai::CellAiState::new(cell, &crate::cell_data::CellAi::default());
            world
                .spawn((Transform::from_xyz(x * UNIT, 0.0, 0.0), o, CellCombat::default(), ai))
                .id()
        };
        let victim = spawn(&mut world, 10, 0.0);
        let near = spawn(&mut world, 11, 2.0); // friend (group 21), surface 1 < 4: flees
        let mid = spawn(&mut world, 13, 6.0); // friend, surface 5: protects (4 <= 5 < 8)
        let far = spawn(&mut world, 11, 30.0); // friend, beyond every radius
        let foe = spawn(&mut world, 12, 1.0); // not a friend
        let attacker = spawn(&mut world, 12, 9.0);
        let run = |world: &mut World, which: FriendFear| {
            world
                .run_system_once(move |db: Res<CellDatabase>, mut objects: Objects| {
                    friend_fear(&db, &mut objects, victim, attacker, CellScale(2), 1, 0.5, which)
                })
                .unwrap()
        };
        run(&mut world, FriendFear::Damage);
        let state = |world: &World, e: Entity| {
            let st = world.get::<crate::cell_ai::CellAiState>(e).unwrap();
            (st.flee_target, st.flee_time, st.chase_target, st.chase_time)
        };
        assert_eq!(state(&world, near), (Some(attacker), 3.0, None, 0.0));
        assert_eq!(state(&world, mid), (None, 0.0, Some(attacker), 5.0));
        assert_eq!(state(&world, far).1, 0.0);
        assert_eq!(state(&world, foe).1, 0.0);
        // Death: cells of the same file flee for the death time (the friend of
        // another file does not).
        run(&mut world, FriendFear::Death);
        let twin = spawn(&mut world, 10, 3.0);
        run(&mut world, FriendFear::Death);
        assert_eq!(state(&world, twin).1, 2.0);
        assert_eq!(state(&world, near).1, 3.0, "another cell file is untouched by the death call");
    }

    #[test]
    fn the_drain_pull_closes_the_distance_linearly_and_then_holds() {
        // 0xE5F4D0: `dt / remaining` of the way per frame, so the victim reaches the mouth
        // when the sub-timer runs out; afterwards it is attached there (0xE62380).
        let (mouth, mut at) = (Vec2::new(4.0, 0.0), Vec2::ZERO);
        let (mut left, dt) = (0.4_f32, 0.1_f32);
        let mut positions = Vec::new();
        for _ in 0..6 {
            at = drain_pull_step(at, mouth, left, dt);
            left = (left - dt).max(0.0);
            positions.push(at.x);
        }
        // Equal steps of a quarter of the distance each, then at the mouth.
        for (got, want) in positions.iter().zip([1.0, 2.0, 3.0, 4.0, 4.0, 4.0]) {
            assert!((got - want).abs() < 1e-4, "{positions:?}");
        }
    }

    #[test]
    fn the_cell_save_without_the_creator_dna_loads_as_none() {
        let save: CellSave = serde_json::from_str("{\"food_progression\": 12}").unwrap();
        assert_eq!(save.dna_at_creator, None);
        assert_eq!(save.food_progression, 12);
        let saved = CellSave { dna_at_creator: Some(40), ..CellSave::default() };
        let back: CellSave = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.dna_at_creator, Some(40));
    }

    #[test]
    fn the_drain_system_attaches_the_victim_at_the_drainers_mouth_part() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let victim = world
            .spawn((Transform::from_xyz(0.0, 0.0, -4.0), cell_object(CellKind::Creature)))
            .id();
        // Heading 0 faces (0, -1); the mouth part sits one unit ahead of the drainer.
        let drainer = world
            .spawn((
                Transform::default(),
                cell_object(CellKind::Creature),
                CellBiting { victim, timer: 4.0, drain: true, point: Vec2::ZERO, mouth_local: Vec2::new(1.0, 0.0), mouth_axis: Vec2::X, pull: 0.5 },
            ))
            .id();
        for _ in 0..8 {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
            world.run_system_once(drain_pull).unwrap();
        }
        let at = world.get::<Transform>(victim).unwrap().translation;
        assert!(Vec2::new(at.x, at.z).abs_diff_eq(Vec2::new(0.0, -1.0), 1e-3), "{at}");
        assert_eq!(world.get::<CellBiting>(drainer).unwrap().pull, 0.0);
        // A turned drainer carries the mouth with it.
        world.get_mut::<CellObject>(drainer).unwrap().heading = std::f32::consts::FRAC_PI_2;
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        world.run_system_once(drain_pull).unwrap();
        let at = world.get::<Transform>(victim).unwrap().translation;
        assert!(Vec2::new(at.x, at.z).abs_diff_eq(Vec2::new(-1.0, 0.0), 1e-3), "{at}");
    }
}
