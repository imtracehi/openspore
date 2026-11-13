//! Retail Creature-stage rules in one place: the original's tables and formulas
//! as named constants and pure functions that the Creature modules call.
//!
//! Source: `notes\creature-re-notes.md` sections C0-C3 (reverse-engineered by
//! `cell-re` from the original executable; addresses are the ModAPI march2017
//! (Steam) addresses, decompilations in `notes\creature-re\*.c`). "VERIFIED"
//! means the number or formula was read in the executable code; "tuning" means
//! the value comes from an installed property record (`CreatureGame!*`,
//! `speciestuning`, `CreatureAbilities!*`) whose use was traced. Whatever the
//! executable does not give us (species records, physics scale, units of a few
//! tuning values) is marked `ADAPTED:` so it can be replaced later.
//!
//! Sections: difficulty, C1 progression (brain level, DNA counters, DNA
//! rewards, level-up, size), C2 core rules (health, hunger, abilities, damage
//! pipeline, speed gears), C3 social round and combat distances, and the Bevy
//! plugin that applies a brain level-up.
// Many named retail values are kept for the passes that will use them (HUD,
// cinematics, flee/call behaviour) and are only exercised by the tests.
#![allow(dead_code)]

use crate::asset_catalog::Capabilities;
use crate::game::{Health, Hunger, Player, RunProgress};
use crate::stages::GameStage;
use bevy::prelude::*;

// ------------------------------------------------------------------ difficulty

/// The Creature game's difficulty tuning (`CreatureGameTuning`, `_easy`, `_hard`
/// and the matching `MotiveTuning` records; easy/hard override a few values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Medium,
    Hard,
}

impl Difficulty {
    /// `CellSave::difficulty` is the game-wide setting: 0 easy, 1 normal, 2 hard.
    pub fn from_index(index: i32) -> Self {
        match index {
            i32::MIN..=0 => Difficulty::Easy,
            1 => Difficulty::Medium,
            _ => Difficulty::Hard,
        }
    }

    /// Pick the value for this difficulty (easy, medium, hard).
    fn pick<T: Copy>(self, easy: T, medium: T, hard: T) -> T {
        match self {
            Difficulty::Easy => easy,
            Difficulty::Medium => medium,
            Difficulty::Hard => hard,
        }
    }
}

// ================================================================= C1 progression

/// `creatureEvoPointsToLevel_1..4` (tuning): the DNA needed to climb from brain
/// level 0 to 1, 1 to 2, 2 to 3 and 3 to 4. VERIFIED (0xD2E380): these are
/// per-level costs, not cumulative cut-offs.
pub const EVO_POINTS_TO_LEVEL: [f32; 4] = [100.0, 200.0, 300.0, 400.0];
/// The last brain level; the stage ends when the player evolves at this level.
pub const MAX_BRAIN_LEVEL: u8 = 4;
/// VERIFIED (0xD2E830): the brain level is read from the STAGE TOTAL, so the
/// levels start at the cumulative sums 100 / 300 / 600 / 1000.
pub const BRAIN_LEVEL_TOTALS: [f32; 4] = [100.0, 300.0, 600.0, 1000.0];
/// Stage total at which the last brain level is reached (the Evolve button).
pub const STAGE_TOTAL_DNA: f32 = BRAIN_LEVEL_TOTALS[3];
/// `creatureMaxEvoPoints` (tuning, used in 0xD2E8A0): cap of the spendable DNA.
pub const MAX_EVO_POINTS: f32 = 9999.0;
/// `MigrationDNALevels` (tuning, 0xD40FA0): the player's species migrates when
/// the stage total reaches each of these.
pub const MIGRATION_DNA_LEVELS: [f32; 2] = [150.0, 650.0];

/// `GetEvoPointsToNextBrainLevel` (0xD2E380): cost of the next level, 0 at the
/// last level (and for any level outside 0..=3).
pub fn evo_points_to_next_level(level: i32) -> f32 {
    usize::try_from(level)
        .ok()
        .and_then(|l| EVO_POINTS_TO_LEVEL.get(l))
        .copied()
        .unwrap_or(0.0)
}

/// Brain level from the stage-total DNA (0xD2E830): the first level whose
/// cumulative cost exceeds the total, 4 once all four are paid. A non-finite
/// total reads as 0 (OpenSpore's usual input sanitation).
pub fn brain_level_from_total(total: f32) -> u8 {
    if !total.is_finite() {
        return 0;
    }
    let mut sum = 0.0;
    for (level, cost) in EVO_POINTS_TO_LEVEL.iter().enumerate() {
        sum += cost;
        if total < sum {
            return level as u8;
        }
    }
    MAX_BRAIN_LEVEL
}

/// Stage total at which `level` starts (sum of the costs below it).
pub fn total_for_level(level: u8) -> f32 {
    EVO_POINTS_TO_LEVEL
        .iter()
        .take(level.min(MAX_BRAIN_LEVEL) as usize)
        .sum()
}

/// DNA earned inside the current brain level (0xD2E410): the total minus the
/// costs of the levels below.
pub fn dna_into_level(total: f32, level: u8) -> f32 {
    (total - total_for_level(level)).max(0.0)
}

/// Progress-bar fraction (0..=1): DNA into the level over that level's cost;
/// full at the last level.
pub fn level_progress(total: f32) -> f32 {
    let level = brain_level_from_total(total);
    let cost = evo_points_to_next_level(level as i32);
    if cost <= 0.0 {
        1.0
    } else {
        (dna_into_level(total, level) / cost).clamp(0.0, 1.0)
    }
}

/// Level-up trigger of the DNA effect queue (0xD2D0B0): the avatar's current
/// level has a cost, the DNA into it covers that cost, and the level is below 4.
pub fn level_up_due(total: f32, avatar_level: u8) -> bool {
    let cost = evo_points_to_next_level(avatar_level as i32);
    cost > 0.0 && cost <= dna_into_level(total, avatar_level) && avatar_level < MAX_BRAIN_LEVEL
}

/// Number of migrations the stage total has unlocked (0, 1 or 2).
pub fn migrations_unlocked(total: f32) -> usize {
    MIGRATION_DNA_LEVELS.iter().filter(|need| total >= **need).count()
}

/// `AddEvolutionPoints` (0xD2E8A0) on the two DNA counters: the spendable pool
/// (editor DNA) is capped at 9999, the stage total only ever grows. Zero,
/// negative and non-finite amounts change nothing (spending goes through
/// `spend_dna`; the retail routine is only called with gains).
pub fn add_evolution_points(spendable: f32, total: f32, points: f32) -> (f32, f32) {
    if !points.is_finite() || points <= 0.0 {
        return (spendable, total);
    }
    ((spendable + points).min(MAX_EVO_POINTS), total + points)
}

/// Spending DNA in the editor only touches the spendable pool; the stage total
/// (brain level, migration) never decreases.
pub fn spend_dna(spendable: f32, cost: f32) -> f32 {
    (spendable - cost.max(0.0)).max(0.0)
}

/// `avatarDeathEvoPointsKeptPercentage` (tuning): the avatar keeps all its DNA.
pub const AVATAR_DEATH_EVO_POINTS_KEPT: f32 = 1.0;
/// `avatarDeathMaxProgressEvoPointsRemoved` (tuning): no progress is removed.
pub const AVATAR_DEATH_MAX_PROGRESS_REMOVED: f32 = 0.0;

/// DNA after the avatar dies: (spendable, stage total). Retail 0xD401E0 (death
/// step 1): the spendable DNA is scaled by `avatarDeathEvoPointsKeptPercentage`
/// (1.0) and the stage total drops to `total - fmod(total, max(1.0,
/// avatarDeathMaxProgressEvoPointsRemoved))`: with the installed 0.0 only the
/// fractional DNA (the `DNAPointsMultipliers` give fractions) is lost, never a
/// whole point.
pub fn dna_after_death(spendable: f32, total: f32) -> (f32, f32) {
    let step = AVATAR_DEATH_MAX_PROGRESS_REMOVED.max(1.0);
    let total = total.max(0.0);
    (spendable * AVATAR_DEATH_EVO_POINTS_KEPT, total - total % step)
}

// ---- DNA rewards (0xC042A0 per creature, 0xC035A0 level multiplier)

/// `DNAPointsMultipliers` (tuning) per difficulty; the index is
/// `2 * player brain level - target strength rating`.
pub const DNA_POINTS_MULTIPLIERS_MEDIUM: [f32; 11] =
    [1.0, 0.3, 0.15, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01];
pub const DNA_POINTS_MULTIPLIERS_EASY: [f32; 11] =
    [1.0, 0.5, 0.25, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01];
pub const DNA_POINTS_MULTIPLIERS_HARD: [f32; 11] =
    [1.0, 0.2, 0.1, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01];

pub fn dna_points_multipliers(difficulty: Difficulty) -> &'static [f32; 11] {
    difficulty.pick(
        &DNA_POINTS_MULTIPLIERS_EASY,
        &DNA_POINTS_MULTIPLIERS_MEDIUM,
        &DNA_POINTS_MULTIPLIERS_HARD,
    )
}

/// 0xC035A0: the multiplier for a target of `target_strength` met by a player of
/// `player_brain_level`. A negative index (a target far above the player) pays
/// in full; an index past the table reads its last entry.
pub fn dna_points_multiplier(
    player_brain_level: i32,
    target_strength: i32,
    difficulty: Difficulty,
) -> f32 {
    let index = player_brain_level.saturating_mul(2).saturating_sub(target_strength);
    if index < 0 {
        return 1.0;
    }
    let table = dna_points_multipliers(difficulty);
    table[(index as usize).min(table.len() - 1)]
}

/// Species DNA base when no species record exists (0xC042A0 starts from 100).
/// Only for legacy (non-retail) species: a retail herd member or roamer pays
/// its archetype's `evoPointsRewarded` / `evoPointsRewardedSocial`
/// (+0x320 / +0x324, `creature_ecology::kill_dna`), see `creature_actions`.
pub const DEFAULT_SPECIES_DNA: f32 = 100.0;
/// `creaturePercentEvoForBabies` (tuning): babies pay a quarter.
pub const DNA_FOR_BABIES: f32 = 0.25;
/// `creaturePercentEvoForAlphas` (tuning): alphas pay 25% more.
pub const DNA_FOR_ALPHAS: f32 = 1.25;
/// `creatureEvoPointsPerEgg` (tuning): DNA for an egg.
pub const EVO_POINTS_PER_EGG: f32 = 25.0;
/// `creatureEvoPointsPerFruit` / `PerCarcass` (tuning) and every diet ability's
/// `mDNAPoints`: eating gives no DNA.
pub const EVO_POINTS_PER_FRUIT: f32 = 0.0;
pub const EVO_POINTS_PER_CARCASS: f32 = 0.0;

/// DNA for killing or befriending one creature (0xC042A0): the species base,
/// times a quarter for a baby or 1.25 for an alpha, times the level multiplier,
/// at least 1.
pub fn creature_dna_reward(
    species_base: f32,
    baby: bool,
    alpha: bool,
    player_brain_level: u8,
    target_strength: i32,
    difficulty: Difficulty,
) -> f32 {
    let mut base = species_base;
    if baby {
        base *= DNA_FOR_BABIES;
    } else if alpha {
        base *= DNA_FOR_ALPHAS;
    }
    let multiplier = dna_points_multiplier(player_brain_level as i32, target_strength, difficulty);
    (base * multiplier).max(1.0)
}

/// Strength rating of an OpenSpore NPC species (retail `mStrengthRating`,
/// +0xB88, which the executable does not derive in the code read so far).
/// ADAPTED: twice the species' best combat or social ability level, so a
/// species whose signature ability equals the player's brain level pays in full
/// (index 0) and weaker ones pay the table's lower entries; capped at the
/// table's last index.
pub fn npc_strength_rating(abilities: Capabilities) -> i32 {
    let best = [
        abilities.bite,
        abilities.charge,
        abilities.strike,
        abilities.spit,
        abilities.sing,
        abilities.dance,
        abilities.charm,
        abilities.pose,
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    (2 * best as i32).min(NPC_MAX_STRENGTH)
}
/// Last index of the `DNAPointsMultipliers` table.
pub const NPC_MAX_STRENGTH: i32 = 10;

// ---- brain level-up (action 0x053F0369, handler 0xD3FCA0)

/// The cinematics the retail stage plays; OpenSpore has none yet (later
/// cinematic work), so these names are only listed for it.
pub mod cinematics {
    /// Brain level-up to level 1..=4 (index = new level - 1).
    pub const BRAIN_UPGRADE: [&str; 4] = [
        "CRG_BrainUpgrade1",
        "CRG_BrainUpgrade2",
        "CRG_BrainUpgrade3",
        "CRG_BrainUpgrade4",
    ];
    /// Evolve pressed at the last level: the stage ends and the Tribe stage starts.
    pub const STAGE_END: &str = "CRG2TRG";
    /// Stage start: the cell crawls onto land.
    pub const STAGE_START: &str = "CLG2CRG";
    /// The player's species migrates to a new nest (0xD40E20) and the fade in.
    pub const MIGRATE: &str = "CRG_SpeciesMigrate";
    pub const FADE_FROM_BLACK_EGG: &str = "CRG_Fade_From_Black_Egg";
    pub const CLAIM_NEST: &str = "CRG_ClaimNest";
    /// Death (0xD38E60): cause 5 starvation, cause 9 deep water, else generic.
    pub const AVATAR_DIED: &str = "CRG_AvatarDied";
    pub const AVATAR_DIED_STARVATION: &str = "CRG_AvatarDied_Starvation";
    pub const AVATAR_DIED_SEA_MONSTER: &str = "CRG_AvatarDied_SeaMonster";
    /// After mating.
    pub const LAID_EGG: &str = "CRG_LaidEgg";
}

/// The brain-upgrade cinematic for a new level (1..=4).
pub fn brain_upgrade_cinematic(new_level: u8) -> Option<&'static str> {
    (1..=MAX_BRAIN_LEVEL)
        .contains(&new_level)
        .then(|| cinematics::BRAIN_UPGRADE[new_level as usize - 1])
}

// ---- size (C2E)

/// `creatureScaleForBrainLevel0..5` (speciestuning): the size factor of a brain
/// level, applied to the avatar and its posse (0xC0B5A0).
pub const SCALE_FOR_BRAIN_LEVEL: [f32; 6] = [1.0, 1.15, 1.3, 1.45, 1.6, 1.75];
/// `creatureBabyScale`, `alphaCreatureScale`, `creatureScale` (random range).
pub const BABY_SCALE: f32 = 0.6;
pub const ALPHA_SCALE: f32 = 1.1;
pub const RANDOM_SCALE_RANGE: (f32, f32) = (0.8, 1.1);
/// `creatureBrainLevelScales` (tuning): the (min, max) size the avatar is
/// normalised into at each brain level (0xD2E720).
pub const BRAIN_LEVEL_SIZE_RANGES: [(f32, f32); 5] =
    [(0.5, 2.0), (0.75, 2.2), (1.0, 2.4), (1.0, 2.6), (1.0, 2.8)];

/// Size factor of a brain level (levels past 5 read the last entry).
pub fn size_factor(level: u8) -> f32 {
    SCALE_FOR_BRAIN_LEVEL[(level as usize).min(SCALE_FOR_BRAIN_LEVEL.len() - 1)]
}

/// `CalculateAvatarNormalizingScale` (0xD2E720): a species of size `s` is scaled
/// into the brain level's range: min/s when smaller, max/s when larger, else 1.
pub fn normalizing_scale(species_size: f32, brain_level: u8) -> f32 {
    let (min, max) = BRAIN_LEVEL_SIZE_RANGES[(brain_level as usize).min(BRAIN_LEVEL_SIZE_RANGES.len() - 1)];
    if !(species_size.is_finite() && species_size > 0.0) {
        1.0
    } else if species_size < min {
        min / species_size
    } else if species_size > max {
        max / species_size
    } else {
        1.0
    }
}

/// The factor a brain level-up multiplies the avatar's and posse's scale by
/// (0xD3FCA0): size factor ratio, divided by the old and times the new
/// normalizing scale.
pub fn rescale_factor(old_level: u8, new_level: u8, species_size: f32) -> f32 {
    size_factor(new_level) / size_factor(old_level) * normalizing_scale(species_size, new_level)
        / normalizing_scale(species_size, old_level)
}

/// The avatar's scale at a brain level: the lineage size times the level's size
/// factor times the normalizing scale. `species_size` is the avatar's size in
/// the normalizing ranges' units; ADAPTED: OpenSpore uses the lineage size
/// (1.0 for an authored creature), which lies inside every range.
pub fn avatar_scale(lineage_size: f32, brain_level: u8) -> f32 {
    lineage_size * size_factor(brain_level) * normalizing_scale(lineage_size, brain_level)
}

// ================================================================= C2 core rules

// ---- C2A health

/// `creatureBaseHealth_Level0..6` (speciestuning, 0x4D1310): base HP by brain level.
pub const HEALTH_TABLE: [f32; 7] = [10.0, 15.0, 25.0, 40.0, 100.0, 100.0, 100.0];
/// `creatureHealthIncrement` (speciestuning, global 0x150C8AC). VERIFIED
/// (creature-audit-a, species profile statistics 0x4D7E29..0x4D7F34): the
/// profile's +0x56C is `min(healthLevel x increment, creatureMaxHealth)` with
/// `healthLevel` the highest `heal` capability among the parts (0x4D3630,
/// clamped to 5) and `creatureMaxHealth` (global 0x150C8B0) 10 = 5 x 2, plus
/// terms for tribal armour (`tarm` x increment) and a `ahbn` bonus of 100 per
/// part that no creature part carries.
pub const HEALTH_INCREMENT: f32 = 2.0;
/// `creatureAvatarHitpointsMultiplier` (tuning, applied to the avatar).
pub const AVATAR_HITPOINTS_MULTIPLIER: f32 = 1.0;
/// `AlphaCreatureHitPointMultiplier` (tuning).
pub const ALPHA_HIT_POINT_MULTIPLIER: f32 = 1.25;
/// Baby hit-point multiplier `nerfMultForBabies` (global read at 0xC05D50):
/// 0.5 in the data, which overwrites the static 0.25 (`notes/tribal-re-notes.md` T8A).
pub const BABY_HIT_POINT_MULTIPLIER: f32 = 0.5;

/// `creatureBaseHealth` for a brain level (0x4D1310): negative levels read
/// level 0, levels past 6 read 100.
pub fn base_health(level: i32) -> f32 {
    let index = level.max(0) as usize;
    HEALTH_TABLE.get(index).copied().unwrap_or(100.0)
}

/// Health a species' parts add (retail +0x56C): the increment times the best
/// health level among its parts (0 for none), at most 5 levels = 10 (see
/// `HEALTH_INCREMENT`).
pub fn parts_health_bonus(health_level: u8) -> f32 {
    HEALTH_INCREMENT * health_level.min(5) as f32
}

/// Base HP before the multipliers (0xC04170): the player species uses the table
/// for the avatar's brain level; other species the record value if any, else
/// the level-0 entry; plus the parts bonus.
pub fn base_hit_points(table_level: i32, record_value: Option<f32>, parts_bonus: f32) -> f32 {
    match record_value {
        Some(value) if value > 0.0 => value + parts_bonus,
        _ => base_health(table_level) + parts_bonus,
    }
}

/// `GetMaxHitPoints` (0xC05D50): the base times the avatar multiplier, then a
/// half for a baby or 1.25 for an alpha.
pub fn max_hit_points(base: f32, is_avatar: bool, baby: bool, alpha: bool) -> f32 {
    let mut multiplier = if is_avatar { AVATAR_HITPOINTS_MULTIPLIER } else { 1.0 };
    if baby {
        multiplier *= BABY_HIT_POINT_MULTIPLIER;
    } else if alpha {
        multiplier *= ALPHA_HIT_POINT_MULTIPLIER;
    }
    base * multiplier
}

/// The avatar's maximum HP at a brain level with its parts' best health level.
pub fn avatar_max_hp(brain_level: u8, health_level: u8) -> f32 {
    max_hit_points(
        base_hit_points(brain_level as i32, None, parts_health_bonus(health_level)),
        true,
        false,
        false,
    )
}

/// An adult, non-alpha NPC's maximum HP (the species has no record in OpenSpore).
pub fn npc_max_hp(health_level: u8) -> f32 {
    max_hit_points(
        base_hit_points(0, None, parts_health_bonus(health_level)),
        false,
        false,
        false,
    )
}

// ---- C2B hunger, food and healing (MotiveTuning)

/// `MotiveMin` / `MotiveMax`: hunger runs 0..100 (defaults of the motive record;
/// no installed property overrides them).
pub const MOTIVE_MIN: f32 = 0.0;
pub const MOTIVE_MAX: f32 = 100.0;
/// `creatureHungerDrain` per second (easy, medium, hard): 100 -> 0 in 90 s on medium.
pub fn hunger_drain(difficulty: Difficulty) -> f32 {
    difficulty.pick(0.833, 1.111, 1.67)
}
/// `hungerDecayDelay` (s): hunger is held at the maximum this long after it
/// reaches it (easy, medium, hard).
pub fn hunger_decay_delay(difficulty: Difficulty) -> f32 {
    difficulty.pick(15.0, 10.0, 5.0)
}
/// `creatureStarvationPain` (MotiveTuning, read as `[0x1687A1C]` in 0xC0D5A0):
/// HP lost per second while hunger is below 1.
pub const STARVATION_PAIN: f32 = 0.05;
/// `creatureStartingMotive_Hunger{Min,Max}` (fractions of the maximum): the
/// avatar's hunger at creation is drawn from this range
/// (`creature_progression::start_hunger_fraction`).
pub const STARTING_HUNGER_FRACTIONS: (f32, f32) = (0.5, 1.0);
/// `MotiveHigh`, `MotiveTakeAction`, `MotiveLow`, `MotiveCritical`.
pub const MOTIVE_HIGH: f32 = 99.0;
pub const MOTIVE_TAKE_ACTION: f32 = 70.0;
pub const MOTIVE_LOW: f32 = 30.0;
pub const MOTIVE_CRITICAL: f32 = 10.0;

/// The hunger update (0xC0D5A0). At the maximum the creature is held there for
/// `decay_delay` seconds (the retail stopwatch), then hunger falls by `drain`
/// per second. Returns (hunger, seconds held).
pub fn hunger_tick(food: f32, max: f32, hold: f32, dt: f32, drain: f32, decay_delay: f32) -> (f32, f32) {
    if !dt.is_finite() || dt <= 0.0 {
        return (food, hold);
    }
    if food >= max {
        let hold = hold + dt;
        if hold < decay_delay {
            return (max, hold);
        }
        return ((max - drain * dt).clamp(MOTIVE_MIN, max), 0.0);
    }
    ((food - drain * dt).clamp(MOTIVE_MIN, max), 0.0)
}

/// HP lost to starvation this tick (0xC0D5A0): `creatureStarvationPain` per
/// second while hunger is below 1.
pub fn starvation_damage(food: f32, dt: f32) -> f32 {
    if food < 1.0 && dt.is_finite() && dt > 0.0 {
        STARVATION_PAIN * dt
    } else {
        0.0
    }
}

/// `creatureHungerSatisfiedPerMeatPerSec` / `PerFruit` / `PerEgg` (MotiveTuning).
pub const HUNGER_PER_MEAT_PER_SEC: f32 = 15.0;
pub const HUNGER_PER_FRUIT: f32 = 25.0;
pub const HUNGER_PER_EGG: f32 = 50.0;
/// `creatureHealthPointsPerCarcassPerSec` (1.75; hard 1.5), `...PerFruit`,
/// `...PerEgg`: HP gained while eating (HP units).
pub fn health_per_carcass_per_sec(difficulty: Difficulty) -> f32 {
    difficulty.pick(1.75, 1.75, 1.5)
}
pub const HEALTH_PER_FRUIT: f32 = 0.25;
pub const HEALTH_PER_EGG: f32 = 0.33;
/// `deadCreatureFoodValueDecay` (tuning, name only): carcass food decay.
pub const DEAD_CREATURE_FOOD_DECAY: f32 = 0.5;

/// Eating a carcass for `dt` seconds (0xD70A80): the carcass loses
/// `15 * dt` food, hunger gains the same, and HP gains `1.75 * dt`. Returns
/// (hunger gained, HP gained); the food eaten never exceeds what is left.
pub fn eat_carcass(food_left: f32, dt: f32, difficulty: Difficulty) -> (f32, f32) {
    if !dt.is_finite() || dt <= 0.0 || food_left <= 0.0 {
        return (0.0, 0.0);
    }
    (
        (HUNGER_PER_MEAT_PER_SEC * dt).min(food_left),
        health_per_carcass_per_sec(difficulty) * dt,
    )
}

/// `creatureHealingHealthRecovery` (0.075), `creatureRestingHealthRecovery` (0)
/// and `creaturePosseHealthRecovery` (0.1): fractions of the maximum HP per
/// second (read in 0xC02E30: `maxHP * rate * dt`). The healing rate applies
/// while the creature is being healed; resting never heals (0).
pub const HEALING_HEALTH_RECOVERY: f32 = 0.075;
pub const RESTING_HEALTH_RECOVERY: f32 = 0.0;
pub const POSSE_HEALTH_RECOVERY: f32 = 0.1;
/// `creatureHealingRadius` (tuning): the distance within which a nest heals.
pub const HEALING_RADIUS: f32 = 5.0;
/// `creatureBasePerception` (tuning, C3): how far a creature notices things.
pub const BASE_PERCEPTION: f32 = 10.0;

/// HP per second of healing for a creature of `max_hp`.
pub fn healing_per_second(max_hp: f32) -> f32 {
    max_hp * HEALING_HEALTH_RECOVERY
}

// ---- C2C abilities (resolved `CreatureAbilities!<name><level>` props)

/// The four attack abilities in OpenSpore's tray order (bite, charge, strike,
/// spit; `creature_actions::COMBAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatKind {
    Bite,
    Charge,
    Strike,
    Spit,
}

impl CombatKind {
    pub const ALL: [CombatKind; 4] = [
        CombatKind::Bite,
        CombatKind::Charge,
        CombatKind::Strike,
        CombatKind::Spit,
    ];

    pub fn from_choice(choice: usize) -> Option<Self> {
        Self::ALL.get(choice).copied()
    }

    /// `cCreatureAbility` type id (30 bite, 31 charge, 33 strike, 32 spit).
    pub fn ability_type(self) -> u32 {
        match self {
            CombatKind::Bite => 30,
            CombatKind::Charge => 31,
            CombatKind::Strike => 33,
            CombatKind::Spit => 32,
        }
    }
}

pub const BITE_DAMAGE: [f32; 5] = [1.0, 1.5, 2.0, 2.5, 3.0];
pub const BITE_REFRESH: f32 = 1.5;
pub const BITE_RANGE: f32 = 1.0;
pub const CHARGE_DAMAGE: [f32; 5] = [1.0, 1.5, 2.0, 2.5, 3.0];
pub const CHARGE_REFRESH: [f32; 5] = [10.0, 12.0, 10.0, 8.0, 6.0];
pub const CHARGE_RANGE: f32 = 1.0;
/// Minimum distance to start the rush (level 1: 14, higher levels: 10).
pub const CHARGE_RANGE_MIN: [f32; 5] = [14.0, 10.0, 10.0, 10.0, 10.0];
pub const CHARGE_RUSH_RANGE: f32 = 20.0;
pub const CHARGE_RUSH_SPEED: f32 = 20.0;
/// Stun of a charge hit (`EffectDuration`; combat effect index 1 = stun, 100%).
pub const CHARGE_STUN_SECONDS: [f32; 5] = [1.5, 1.75, 2.0, 2.25, 2.5];
pub const SPIT_DAMAGE: [f32; 5] = [0.5, 1.0, 1.5, 2.0, 2.5];
pub const SPIT_REFRESH: f32 = 3.0;
pub const SPIT_RANGE: f32 = 30.0;
pub const SPIT_RANGE_MIN: f32 = 0.5;
pub const SPIT_PROJECTILE_SPEED: f32 = 50.0;
/// Poison of a spit hit (effect kind 1, combat effect index 2 = 100%): this
/// many seconds at `SPIT_POISON_DPS[level]` HP per second.
pub const SPIT_POISON_SECONDS: f32 = 6.0;
pub const SPIT_POISON_DPS: [f32; 5] = [0.25, 0.333, 0.417, 0.5, 0.5833];
pub const STRIKE_DAMAGE: [f32; 5] = [3.0, 4.5, 6.0, 8.0, 9.0];
pub const STRIKE_REFRESH: f32 = 4.5;
pub const STRIKE_RANGE: f32 = 1.0;
/// Strike hits a cone: distance 1, radius 2.
pub const STRIKE_CONE: (f32, f32) = (1.0, 2.0);
/// `creatureAttackAbilityRangeHitBuffer` (tuning): slack added to an ability's
/// range when deciding whether a hit lands.
pub const RANGE_HIT_BUFFER: f32 = 1.5;
/// Epic abilities (`epicstomp`, `epicspit` props; used by epic creatures).
pub const EPIC_STOMP_DAMAGE: f32 = 6.0;
pub const EPIC_STOMP_REFRESH: f32 = 3.0;
pub const EPIC_STOMP_RANGE: f32 = 15.0;
pub const EPIC_SPIT_DAMAGE: f32 = 6.0;
pub const EPIC_SPIT_POISON: (f32, f32) = (4.0, 2.0);

/// Social abilities: sing (vocalize 37), dance (34), charm (36), pose (posture
/// 35). The social power is the ability level 1..=5.
pub const SOCIAL_REFRESH: f32 = 5.0;
pub const SOCIAL_RANGE: f32 = 5.0;
pub fn social_power(level: u8) -> f32 {
    level.clamp(0, 5) as f32
}
/// `block` (duration, refresh) and `call` (range).
pub const BLOCK_DURATION: f32 = 2.0;
pub const BLOCK_REFRESH: f32 = 10.0;
pub const CALL_RANGE: f32 = 10.0;

/// Timed abilities (sprint, stealth): usable for `duration` seconds, and the
/// used time recovers over `recharge` seconds (0xC1AC30: the used-time counter
/// falls by `duration / recharge` per second while the ability is off); after
/// use the ability waits `refresh` seconds before it can start again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedAbility {
    pub duration: f32,
    pub recharge: f32,
    pub refresh: f32,
}

impl TimedAbility {
    /// Seconds of the ability regained per second of rest.
    pub fn recovery_per_second(&self) -> f32 {
        self.duration / self.recharge
    }
}

pub const SPRINT_DURATION: [f32; 5] = [10.0, 15.0, 20.0, 25.0, 30.0];
pub const SPRINT_RECHARGE: [f32; 5] = [120.0, 105.0, 90.0, 75.0, 60.0];
pub const SPRINT_REFRESH: f32 = 25.0;
pub const STEALTH_DURATION: [f32; 5] = [10.0, 11.0, 13.0, 16.0, 20.0];
pub const STEALTH_RECHARGE: [f32; 5] = [60.0, 50.0, 40.0, 35.0, 30.0];
pub const STEALTH_REFRESH: f32 = 25.0;
/// Distance within which a stealthed creature is still noticed, and the
/// distance inside which it is always noticed (level 1..=5).
pub const STEALTH_RANGE: [f32; 5] = [16.0, 12.0, 8.0, 4.0, 2.0];
pub const STEALTH_RANGE_MIN: [f32; 5] = [5.0, 4.0, 2.5, 1.5, 0.75];

fn level_index(level: u8) -> Option<usize> {
    (level >= 1).then(|| (level.min(5) - 1) as usize)
}

pub fn sprint(level: u8) -> Option<TimedAbility> {
    let i = level_index(level)?;
    Some(TimedAbility {
        duration: SPRINT_DURATION[i],
        recharge: SPRINT_RECHARGE[i],
        refresh: SPRINT_REFRESH,
    })
}

pub fn stealth(level: u8) -> Option<TimedAbility> {
    let i = level_index(level)?;
    Some(TimedAbility {
        duration: STEALTH_DURATION[i],
        recharge: STEALTH_RECHARGE[i],
        refresh: STEALTH_REFRESH,
    })
}

/// Stealth detection distance for a level (the sight radius it replaces is
/// larger than every entry); 0 when the creature has no stealth.
pub fn stealth_range(level: u8) -> Option<(f32, f32)> {
    let i = level_index(level)?;
    Some((STEALTH_RANGE[i], STEALTH_RANGE_MIN[i]))
}

/// Jump and glide: `refresh` between jumps and glides, flaps per glide level
/// (`numFlapsForGlideLevel1..5`), `creatureMaxJumpHeight`, `creatureGlideSpeed`.
pub const JUMP_REFRESH: f32 = 0.5;
pub const GLIDE_REFRESH: f32 = 0.1;
pub const FLAPS_FOR_GLIDE_LEVEL: [u32; 5] = [2, 4, 6, 8, 10];
pub const MAX_JUMP_HEIGHT: f32 = 3.5;
pub const GLIDE_SPEED: f32 = 10.0;

/// Flaps a glide level allows (0 without the ability).
pub fn glide_flaps(level: u8) -> u32 {
    level_index(level).map_or(0, |i| FLAPS_FOR_GLIDE_LEVEL[i])
}

/// Jump height for a jump level. ADAPTED: the tuning only gives the maximum
/// (3.5, reached at level 5); lower levels scale linearly.
pub fn jump_height(level: u8) -> f32 {
    MAX_JUMP_HEIGHT * level.min(5) as f32 / 5.0
}

/// One attack ability at one level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatAbilityStats {
    pub kind: CombatKind,
    pub level: u8,
    pub damage: f32,
    /// Seconds before the ability can be used again.
    pub refresh: f32,
    pub range: f32,
    pub range_min: f32,
    pub effect: CombatEffect,
}

/// What a hit does besides damage (`CombatEffectTypes` / `Percentages`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombatEffect {
    None,
    /// Charge: the target is stunned for `seconds`.
    Stun { seconds: f32 },
    /// Spit: poison for `seconds` at `damage_per_second`.
    Poison { seconds: f32, damage_per_second: f32 },
}

impl CombatAbilityStats {
    /// Distance at which the ability lands: its range plus the hit buffer.
    pub fn reach(&self) -> f32 {
        self.range + RANGE_HIT_BUFFER
    }
}

/// The attack ability `kind` at `level` (1..=5; levels past 5 read level 5),
/// `None` for level 0 (the creature has no such part).
pub fn combat_ability(kind: CombatKind, level: u8) -> Option<CombatAbilityStats> {
    let i = level_index(level)?;
    let level = level.min(5);
    Some(match kind {
        CombatKind::Bite => CombatAbilityStats {
            kind,
            level,
            damage: BITE_DAMAGE[i],
            refresh: BITE_REFRESH,
            range: BITE_RANGE,
            range_min: 0.0,
            effect: CombatEffect::None,
        },
        CombatKind::Charge => CombatAbilityStats {
            kind,
            level,
            damage: CHARGE_DAMAGE[i],
            refresh: CHARGE_REFRESH[i],
            range: CHARGE_RANGE,
            range_min: CHARGE_RANGE_MIN[i],
            effect: CombatEffect::Stun {
                seconds: CHARGE_STUN_SECONDS[i],
            },
        },
        CombatKind::Strike => CombatAbilityStats {
            kind,
            level,
            damage: STRIKE_DAMAGE[i],
            refresh: STRIKE_REFRESH,
            range: STRIKE_RANGE,
            range_min: 0.0,
            effect: CombatEffect::None,
        },
        CombatKind::Spit => CombatAbilityStats {
            kind,
            level,
            damage: SPIT_DAMAGE[i],
            refresh: SPIT_REFRESH,
            range: SPIT_RANGE,
            range_min: SPIT_RANGE_MIN,
            effect: CombatEffect::Poison {
                seconds: SPIT_POISON_SECONDS,
                damage_per_second: SPIT_POISON_DPS[i],
            },
        },
    })
}

// ---- C2D damage pipeline (attack hit, 0xC20390)

/// `creatureAvatarDamageMultiplier`, `creatureDamageStealthMultiplier`,
/// `creatureDamageAgainstHelplessMultiplier` and the baby attacker multiplier
/// (tuning / global read in the pipeline).
pub const AVATAR_DAMAGE_MULTIPLIER: f32 = 1.0;
pub const STEALTH_DAMAGE_MULTIPLIER: f32 = 1.5;
pub const DAMAGE_AGAINST_HELPLESS_MULTIPLIER: f32 = 1.0;
pub const BABY_ATTACK_MULTIPLIER: f32 = 0.25;
/// `creatureDamagePercentageMin` / `Max`: both 1.0, so a hit has no randomness.
pub const DAMAGE_PERCENTAGE_MIN: f32 = 1.0;
pub const DAMAGE_PERCENTAGE_MAX: f32 = 1.0;
/// `npcAttackMissPercentage` (tuning): NPCs never miss.
pub const NPC_ATTACK_MISS_PERCENTAGE: f32 = 0.0;

/// Who attacks whom, for `attack_damage`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AttackContext {
    pub attacker_is_avatar: bool,
    pub attacker_is_baby: bool,
    pub attacker_stealthed: bool,
    /// The attacker species profile's `mAdditionalDamage` (+0x63C).
    pub attacker_additional_damage: f32,
    /// A posse member that is not the avatar: the posse record's multiplier (+0x158).
    pub posse_multiplier: Option<f32>,
    pub target_is_baby: bool,
    /// The attacker's damage boost (`mDamageBoostAmount`, +0xB14), if it has one.
    pub damage_boost: Option<f32>,
    /// The target's armor boost (`mArmorBoostAmount`, +0xB1C), if it has one.
    pub armor_boost: Option<f32>,
}

/// Damage of one hit (0xC20390): the ability's damage (+ the attacker's
/// additional damage for effect-kind-0 abilities: bite, charge, strike), times
/// the avatar multiplier, a quarter for a baby attacker, 1.5 when a stealthed
/// attacker bites or strikes, the (constant) damage percentage, the posse
/// record multiplier, the helplessness and damage-boost multipliers, divided by
/// the target's armor boost.
pub fn attack_damage(ability: &CombatAbilityStats, context: &AttackContext) -> f32 {
    let mut damage = ability.damage;
    if ability.kind != CombatKind::Spit {
        damage += context.attacker_additional_damage;
    }
    if context.attacker_is_avatar {
        damage *= AVATAR_DAMAGE_MULTIPLIER;
    }
    if context.attacker_is_baby {
        damage *= BABY_ATTACK_MULTIPLIER;
    }
    if context.attacker_stealthed && matches!(ability.kind, CombatKind::Bite | CombatKind::Strike) {
        damage *= STEALTH_DAMAGE_MULTIPLIER;
    }
    damage *= DAMAGE_PERCENTAGE_MIN;
    if !context.attacker_is_avatar {
        if let Some(posse) = context.posse_multiplier {
            damage *= posse;
        }
    }
    if context.target_is_baby {
        damage *= DAMAGE_AGAINST_HELPLESS_MULTIPLIER;
    }
    if let Some(boost) = context.damage_boost {
        damage *= boost;
    }
    if let Some(armor) = context.armor_boost.filter(|a| *a > 0.0) {
        damage /= armor;
    }
    damage.max(0.0)
}

/// Charge knockback on a non-avatar target (0xC20390): the impulse is
/// `clamp(damage * 0.01, 0.1, 1.0)` times a global (0x1582ED0, not traced).
pub fn charge_knockback_impulse(damage: f32) -> f32 {
    (damage * 0.01).clamp(0.1, 1.0)
}
/// ADAPTED: world units a target is shoved per unit of impulse (the physics
/// global behind retail's impulse is not traced).
pub const KNOCKBACK_DISTANCE_PER_IMPULSE: f32 = 3.0;

/// A spit hit interrupts a charge in progress (retail calls the target's
/// func94h with the effect duration).
pub fn spit_interrupts_charge(attacker: CombatKind, target_is_charging: bool) -> bool {
    attacker == CombatKind::Spit && target_is_charging
}

/// `avatarStunTimeRange` x `avatarStunTimeMultiplier`: how long the avatar is
/// stunned (roll in 0..=1 picks the point in the range).
pub fn avatar_stun_seconds(roll: f32) -> f32 {
    let (low, high) = AVATAR_STUN_TIME_RANGE;
    (low + (high - low) * roll.clamp(0.0, 1.0)) * AVATAR_STUN_TIME_MULTIPLIER
}
pub const AVATAR_STUN_TIME_RANGE: (f32, f32) = (3.0, 4.0);
pub const AVATAR_STUN_TIME_MULTIPLIER: f32 = 0.5;

/// Poison damage over a tick: (HP lost, seconds left).
pub fn poison_tick(remaining: f32, damage_per_second: f32, dt: f32) -> (f32, f32) {
    if !dt.is_finite() || dt <= 0.0 || remaining <= 0.0 {
        return (0.0, remaining.max(0.0));
    }
    let applied = dt.min(remaining);
    (damage_per_second * applied, remaining - applied)
}

// ---- C2C speed gears

/// `globalCreatureSpeedGear0..5` (speciestuning): speed of each gear.
pub const SPEED_GEARS: [f32; 6] = [2.5, 4.0, 5.5, 6.5, 7.5, 8.5];
/// `globalCreatureSprintSpeedBoost` (speciestuning): VERIFIED additive (0xC0D100
/// returns the gear speed plus this while the sprint flag is set).
pub const SPRINT_SPEED_BOOST: f32 = 3.0;

/// Speed of a gear (the speed CAP level 0..=5; higher levels read gear 5).
pub fn gear_speed(gear: u8) -> f32 {
    SPEED_GEARS[(gear as usize).min(SPEED_GEARS.len() - 1)]
}

/// The avatar's walking speed for the best speed level among its parts.
/// ADAPTED: retail picks the gear of a movement mode (a per-mode offset table
/// not present in the code image); OpenSpore uses the gear itself.
pub fn avatar_speed(speed_level: u8) -> f32 {
    gear_speed(speed_level)
}

/// Speed while sprinting: the gear speed plus the boost.
pub fn sprint_speed(base_speed: f32) -> f32 {
    base_speed + SPRINT_SPEED_BOOST
}

// ================================================================= C3 social and combat

/// `creatureSocialResponseTimeout` (tuning, step 0xDA4CAA): seconds the NPC
/// waits for an answer once its demonstration is over; unanswered, the step fails.
pub const SOCIAL_RESPONSE_TIMEOUT: f32 = 15.0;
/// The NPC demonstrates the requested ability for 5 s (the request broadcast,
/// event type 3, 0xD9B190) before the round opens for answers.
pub const SOCIAL_DEMONSTRATION_SECONDS: f32 = 5.0;
/// `incorrectAbilityMultiplier` (tuning, 0xD9B144): answering with a different
/// ability than the requested one still counts this fraction of its power.
pub const INCORRECT_ABILITY_MULTIPLIER: f32 = 0.4;
/// Tuning 0x93110E94 (value 2.0): multiplies an answer when 0xBCA230 finds an
/// active entry (presumably a posse member performing too; the trigger is
/// unverified, OpenSpore uses a pack member that can perform the same ability).
pub const SOCIAL_POSSE_MULTIPLIER: f32 = 2.0;
/// Fallback maximum of the friendship bar in power units, for a creature saved
/// mid-performance before the bar was planned (`social_bar_max` is the retail
/// rule: twice the requested levels).
pub const SOCIAL_BAR_MAX: f32 = 3.0;
/// NPC clips of the social round (VERIFIED, step 0xDA4CAA): the round opens
/// with 0x061E0675; while its side drains the creature plays 0x061E0670; a
/// wrong answer 0x068D8F9C; the rounds used up without a full bar 0x53685D8F.
pub const SOCIAL_CLIP_ROUND_OPEN: u32 = 0x061E_0675;
pub const SOCIAL_CLIP_IMPATIENT: u32 = 0x061E_0670;
pub const SOCIAL_CLIP_WRONG: u32 = 0x068D_8F9C;
pub const SOCIAL_CLIP_ROUNDS_OVER: u32 = 0x5368_5D8F;
/// Befriend reactions (VERIFIED 0xD9BBD0): the befriended creature plays, and
/// broadcasts to its group, `soc_fearful_to_curious` / `soc_angry_to_curious` /
/// `soc_curious_to_friendly` / `soc_friendly_to_bestf` by the species'
/// relationship to the player before the befriend event (0xBA40E0).
pub const SOCIAL_CLIP_FEARFUL_TO_CURIOUS: u32 = 0xF37D_B906;
pub const SOCIAL_CLIP_ANGRY_TO_CURIOUS: u32 = 0xD37D_B8F6;
pub const SOCIAL_CLIP_CURIOUS_TO_FRIENDLY: u32 = 0x538B_F747;
pub const SOCIAL_CLIP_FRIENDLY_TO_BEST: u32 = 0x738B_F75B;

/// Creature-game relationship of a species to the player (0xBA40E0 values,
/// meaning read from the face each one selects, 0xE31C70, and the reaction
/// clips): 1 fearful, 2 wary, 3 curious, 4 friendly, 5 hostile, 6 best
/// friends (0 none).
pub mod relationship {
    pub const FEARFUL: u8 = 1;
    pub const WARY: u8 = 2;
    pub const CURIOUS: u8 = 3;
    pub const FRIENDLY: u8 = 4;
    pub const HOSTILE: u8 = 5;
    pub const BEST_FRIENDS: u8 = 6;
}

/// Whether a creature of a species with this relationship to the avatar attacks
/// it on sight. VERIFIED: `cCreatureAnimal` 0xC0B9C0 (the aggressive test) asks
/// the species relationship manager (0xBA40E0) and is true for 2 (hated) and 5
/// (hostile) only; the attack goal evaluator (0xD6EEC0) walks the creatures the
/// attacker perceives and, for one of another species that is seen, alive (or a
/// carcass with the `[0x1582DF6]` flag) and passes this test, posts the attack
/// goal 0x40000 against it. Diet plays no part (a carnivore with a curious
/// relationship does not attack; the data gives every carnivore herd 2 or 5).
pub fn attacks_on_sight(relationship: u8) -> bool {
    matches!(relationship, relationship::WARY | relationship::HOSTILE)
}

/// The reaction clip of a befriend (0xD9BBD0 switch on the relationship before
/// the event: 1, 2 or 5, 3, anything else).
pub fn befriend_reaction_clip(before: u8) -> u32 {
    match before {
        relationship::FEARFUL => SOCIAL_CLIP_FEARFUL_TO_CURIOUS,
        relationship::WARY | relationship::HOSTILE => SOCIAL_CLIP_ANGRY_TO_CURIOUS,
        relationship::CURIOUS => SOCIAL_CLIP_CURIOUS_TO_FRIENDLY,
        _ => SOCIAL_CLIP_FRIENDLY_TO_BEST,
    }
}

/// The species' relationship to the player from its counters. VERIFIED rules:
/// it starts at the archetype's `initialAvatarRelationship`; a befriend
/// (event 3, 0xBA43E0) counts an impression and sets best friends at
/// `NumToImpressForBestFriends`, friendly at `NumToImpressForFriendly`,
/// curious at `NumToImpressForCurious` (below that it keeps its value); kills
/// (event 0) make it fearful at `numToKillForFearfulRelationship` (hostile
/// instead when it was hostile already). ADAPTED: OpenSpore keeps counts, not
/// the event history, so impressions win over kills (a befriend resets the
/// retail kill counter) and "hostile instead of fearful" is decided by the
/// initial value only (retail also when the species was friendly twice).
pub fn species_relationship(initial: u8, impressed: u32, killed: u32, thresholds: [u32; 4]) -> u8 {
    let [curious, friendly, best, fearful] = thresholds;
    if impressed > 0 && impressed >= best.max(1) {
        relationship::BEST_FRIENDS
    } else if impressed > 0 && impressed >= friendly.max(1) {
        relationship::FRIENDLY
    } else if impressed > 0 && impressed >= curious {
        relationship::CURIOUS
    } else if fearful > 0 && killed >= fearful {
        if initial == relationship::HOSTILE {
            relationship::HOSTILE
        } else {
            relationship::FEARFUL
        }
    } else {
        initial
    }
}
/// Group socials (VERIFIED, SocialPropose 0xDA4360 + its update 0xDA4950,
/// notes C10A): a proposer whose herd archetype has `SocialGroupSize` > 1
/// calls up to `SocialGroupSize - 1` herd mates of its species from its nearby
/// list: it plays 0x061E0673 once, then 0x061E0674, for up to
/// `SOCIAL_GROUP_WAIT_SECONDS` or until every joiner stands in its slot.
pub const SOCIAL_CLIP_GROUP_CALL: u32 = 0x061E_0673;
pub const SOCIAL_CLIP_GROUP_WAIT: u32 = 0x061E_0674;
/// Gathering time (CreatureGameTuning 0x3215F0AF = 5.0 at every difficulty,
/// loader 0xD3EA17 -> global 0x1582F28; code default 10).
pub const SOCIAL_GROUP_WAIT_SECONDS: f32 = 5.0;
/// Joiner slots around a proposer (0xDA3AF0: three slot records, behaviour
/// +0x200). Slot i lies at `2 x (joiner radius + 1)` from the proposer, the
/// proposer-to-target direction turned by 135, 225 and 180 degrees about the
/// local up (rotation half-angles (pi/4 + pi/2) / 2 and (pi/4 + pi) / 2 from
/// 0x1593470..78, the third slot the reversed direction). With fewer than three
/// joiners allowed, the slot straight behind is skipped.
pub const SOCIAL_GROUP_SLOT_TURNS: [f32; 3] = [135.0, 225.0, 180.0];
/// A joiner walks to its slot at standard speed (speed state 2) and stops
/// within 1.0 (0xC1C330(2, slot, 1.0, 2.0)).
pub const SOCIAL_GROUP_SLOT_STOP: f32 = 1.0;

/// Herd mates a proposer may call (0xDA4360: behaviour +0x1F4 =
/// max(`SocialGroupSize` - 1, 0)), at most one per slot.
pub fn social_group_joiners(social_group_size: u32) -> usize {
    (social_group_size.saturating_sub(1) as usize).min(SOCIAL_GROUP_SLOT_TURNS.len())
}

/// The joiner slots in use for `allowed` joiners (0xDA3AF0 skips slot 2 unless
/// three may join).
pub fn social_group_slots(allowed: usize) -> &'static [f32] {
    if allowed >= 3 {
        &SOCIAL_GROUP_SLOT_TURNS
    } else {
        &SOCIAL_GROUP_SLOT_TURNS[..2]
    }
}

/// The ground-plane point of joiner slot `turn_degrees` (one of
/// `SOCIAL_GROUP_SLOT_TURNS`): the proposer-to-target direction turned about
/// the up axis, at `2 x (radius + 1)` from the proposer.
pub fn social_group_slot(proposer: Vec2, target: Vec2, turn_degrees: f32, radius: f32) -> Vec2 {
    let toward = (target - proposer).normalize_or(Vec2::Y);
    let (s, c) = turn_degrees.to_radians().sin_cos();
    let turned = Vec2::new(toward.x * c - toward.y * s, toward.x * s + toward.y * c);
    proposer + turned * 2.0 * (radius.max(0.0) + 1.0)
}

/// Friendship (0..=100 bar) one answer adds (0xD9B070 scoring, 0xD9B030 bar):
/// the ability's power (its level 1..=5), doubled when a posse member helps,
/// times 0.4 when it is not the requested ability, over the bar maximum. A
/// wrong answer therefore still adds friendship; nothing is ever subtracted.
pub fn social_answer_gain(level: u8, correct: bool, posse_helping: bool) -> f32 {
    100.0 * social_answer_power(level, correct, posse_helping) / SOCIAL_BAR_MAX
}

/// One answer in power units (0xD9B070): the level, x2 with a posse member,
/// x0.4 when it is not the requested ability.
pub fn social_answer_power(level: u8, correct: bool, posse_helping: bool) -> f32 {
    let mut amount = social_power(level);
    if posse_helping {
        amount *= SOCIAL_POSSE_MULTIPLIER;
    }
    if !correct {
        amount *= INCORRECT_ABILITY_MULTIPLIER;
    }
    amount
}

/// The social bar maximum of a proposal (VERIFIED 0xDA4360: behaviour +0x1BC
/// = 2 x the sum of `round(mDamage)` of the requested abilities; a social
/// ability's `mDamage` (+0x3C) is its level 1..=5). The creature's
/// demonstrations fill one side (its level per round) and the player's
/// answers the other, so answering every round with the same level fills the
/// bar exactly at the last round.
pub fn social_bar_max(levels: impl IntoIterator<Item = u8>) -> f32 {
    2.0 * levels.into_iter().map(social_power).sum::<f32>()
}

/// Seconds of patience and drain of the creature's side of the bar
/// (`socialTimeBeforeDrain` +0x428, `socialDrainRate` +0x42C of the herd
/// archetype). VERIFIED (0xDA4360 copies them into the bar record; 0xDA4CAA
/// arms the timer when the round opens and no answer came; 0xD9B9C0 counts it
/// down and then sets the drain, 0xE32420(row 0, rate)). The rate is a speed
/// in bar spans per second (the tween duration of 0xE32210 is
/// `|delta| / span / rate`), so 0.1 empties a full bar in 10 s.
pub fn social_drain(fill: f32, bar_max: f32, rate: f32, dt: f32) -> f32 {
    if !(dt > 0.0) || !(rate > 0.0) {
        return fill;
    }
    (fill - rate * bar_max * dt).max(0.0)
}

/// CreatureGameTuning distances (C3B) the module's own ranges are derived from.
pub const ATTACK_DISTANCE: f32 = 5.0;
pub const CHASE_DISTANCE: f32 = 5.0;
/// `creatureUntargetDistance`: a target farther than this is let go.
pub const UNTARGET_DISTANCE: f32 = 17.5;
pub const GOAL_DISTANCE_LIMIT: f32 = 50.0;
/// `creatureAttackTetherDistance` (easy 45, medium 60, hard 90).
pub fn attack_tether_distance(difficulty: Difficulty) -> f32 {
    difficulty.pick(45.0, 60.0, 90.0)
}
pub const FIGHT_NUM_SLOTS: u32 = 4;
pub const FIGHT_NO_ACTIVITY_TIME_LIMIT: f32 = 5.0;
/// NPC behaviour: `creatureHitPointsTurnAndFightPercentage`, `creatureWaryDistance`,
/// `creatureHelpAllyInRadius`, `creaturePouncePercentage`, `creatureNoAttackTimer`.
pub const HIT_POINTS_TURN_AND_FIGHT_PERCENTAGE: f32 = 0.75;
pub const WARY_DISTANCE: f32 = 6.0;
pub const HELP_ALLY_IN_RADIUS: f32 = 10.0;
pub const POUNCE_PERCENTAGE: f32 = 0.001;
pub const NO_ATTACK_TIMER: f32 = 30.0;
/// Flee tuning (`creatureFlee*`).
pub const FLEE_DISTANCE: f32 = 25.0;
pub const FLEE_TIME: f32 = 5.0;
pub const FLEE_REMEMBER_TIME: f32 = 90.0;
pub const FLEE_TIME_TO_TIRED: f32 = 3.0;
pub const FLEE_TIRED_TIME: f32 = 3.0;
pub const FLEE_DODGE_DISTANCE: f32 = 0.25;
pub const FLEE_DODGE_TIME: f32 = 3.0;
pub const FLEE_JUMP_FREQUENCY: f32 = 4.0;
pub const FLEE_STEALTH_TIME: f32 = 10.0;
pub const FLEE_WATCH_TIME: f32 = 10.0;
/// Flee direction biases: nest, herd, away from the camera, away from the threat.
pub const FLEE_DIRECTION_BIASES: [f32; 4] = [0.5, 1.0, 1.0, 1.0];
/// Mating / ally calls (`creatureCall*`).
pub const CALL_START_RADIUS: f32 = 10.0;
pub const CALL_EXPANSION_RATE: f32 = 20.0;
pub const CALL_DURATION: f32 = 3.0;
pub const CALL_MAX_LOUDNESS: f32 = 500.0;
pub const CALL_MAX_RESPONDING: (u32, u32, u32) = (7, 7, 2);
pub const CALL_PERCENT_CHANCE_RESPONDER_ATTACKS: f32 = 0.1;
/// Perception: `creatureMaxPerceptionRadius`, `creatureSightRadius`,
/// `creatureSightAngle`, `creatureBaseLoudness` and its multipliers (normal
/// speed, sprinting, in combat).
pub const MAX_PERCEPTION_RADIUS: f32 = 50.0;
pub const SIGHT_RADIUS: f32 = 17.0;
pub const SIGHT_ANGLE: f32 = 120.0;
pub const BASE_LOUDNESS: f32 = 4.0;
pub const LOUDNESS_MULTIPLIERS: (f32, f32, f32) = (1.5, 2.0, 3.0);

// ================================================================= brain level-up system

/// Applies a brain level-up to the Creature avatar (0xD3FCA0): the avatar is
/// rescaled by the size-factor ratio, healed to the new maximum, and its hunger
/// refilled to the maximum (retail calls `SetHunger(MotiveMax)`). The first
/// observation of each avatar (a stage entry spawns a new one, already built
/// for its level) only adopts the current level.
pub fn apply_brain_level(
    run: Res<RunProgress>,
    mut seen: Local<Option<(Entity, u8)>>,
    mut player: Query<(Entity, &mut Health, &mut Hunger, &mut Transform), With<Player>>,
    originals: Res<crate::Originals>,
) {
    let Ok((entity, mut health, mut hunger, mut transform)) = player.single_mut() else {
        return;
    };
    let level = run.creature_progression.brain_level.min(MAX_BRAIN_LEVEL);
    let Some(old) = seen.filter(|(e, _)| *e == entity).map(|(_, level)| level) else {
        *seen = Some((entity, level));
        return;
    };
    if level == old {
        return;
    }
    *seen = Some((entity, level));
    if level < old {
        return;
    }
    let lineage = run.creature_stats().size;
    transform.scale *= rescale_factor(old, level, lineage);
    let abilities = crate::creature_actions::player_abilities(&run, &originals);
    health.max = avatar_max_hp(level, abilities.health);
    health.hp = health.max;
    hunger.food = hunger.max;
    hunger.hold = 0.0;
}

/// Applies the knockback a charge hit left on a creature
/// (`CreatureMind::shove`, filled by `creature_actions` from
/// `charge_knockback_impulse`); the shove is a one-frame displacement.
pub fn apply_knockback(
    mut creatures: Query<
        (&mut Transform, &mut crate::creature_actions::CreatureMind),
        Without<Player>,
    >,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    for (mut transform, mut mind) in &mut creatures {
        if mind.shove == [0.0, 0.0] {
            continue;
        }
        // A shove never pushes a creature into the sea (the island clamped it
        // to its square).
        let shoved = transform.translation + Vec3::new(mind.shove[0], 0.0, mind.shove[1]);
        transform.translation = land.step(transform.translation, shoved);
        mind.shove = [0.0, 0.0];
    }
}

/// Registers the Creature rules' systems.
pub struct CreatureRulesPlugin;

impl Plugin for CreatureRulesPlugin {
    fn build(&self, app: &mut App) {
        // The rescale happens at the level-up itself (0xD3FCA0), under the
        // fade of `CRG_BrainUpgrade<n>`: it is not held with the simulation set
        // that the cinematic pauses.
        app.add_systems(Update, apply_brain_level.run_if(in_state(GameStage::Creature)));
        app.add_systems(
            Update,
            apply_knockback
                .after(crate::creature_actions::update)
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(GameStage::Creature)),
        );
        // The Creature stage's floating origin on the planet
        // (`creature_planet`), after every Update system moved the avatar.
        app.add_systems(
            PostUpdate,
            crate::creature_planet::reanchor
                .before(crate::planet_render::update)
                .before(bevy::transform::TransformSystems::Propagate)
                .run_if(in_state(GameStage::Creature)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn befriend_reactions_follow_the_relationship_before_the_event() {
        use relationship::*;
        // 0xD9BBD0: 1 -> fearful, 2 / 5 -> angry, 3 -> curious, else friendly.
        assert_eq!(befriend_reaction_clip(FEARFUL), 0xF37D_B906);
        assert_eq!(befriend_reaction_clip(WARY), 0xD37D_B8F6);
        assert_eq!(befriend_reaction_clip(HOSTILE), 0xD37D_B8F6);
        assert_eq!(befriend_reaction_clip(CURIOUS), 0x538B_F747);
        assert_eq!(befriend_reaction_clip(FRIENDLY), 0x738B_F75B);
        assert_eq!(befriend_reaction_clip(BEST_FRIENDS), 0x738B_F75B);
        // A social herd (initial 3, thresholds 1 / 2 / 3, fearful at 3).
        let t = [1, 2, 3, 3];
        assert_eq!(species_relationship(3, 0, 0, t), CURIOUS);
        assert_eq!(species_relationship(3, 1, 0, t), CURIOUS);
        assert_eq!(species_relationship(3, 2, 0, t), FRIENDLY);
        assert_eq!(species_relationship(3, 3, 0, t), BEST_FRIENDS);
        assert_eq!(species_relationship(3, 0, 3, t), FEARFUL);
        // Combat herds start wary (2) or hostile (5); kills keep 5 hostile.
        assert_eq!(species_relationship(2, 0, 0, t), WARY);
        assert_eq!(species_relationship(5, 0, 3, t), HOSTILE);
        assert_eq!(species_relationship(5, 1, 0, t), CURIOUS);
    }

    #[test]
    fn attack_on_sight_is_the_hated_and_hostile_relationships_only() {
        use relationship::*;
        // 0xC0B9C0: true for 2 and 5, false for fearful, curious, friendly, best friends.
        for (value, attacks) in [(FEARFUL, false), (WARY, true), (CURIOUS, false), (FRIENDLY, false), (HOSTILE, true), (BEST_FRIENDS, false)] {
            assert_eq!(attacks_on_sight(value), attacks, "relationship {value}");
        }
        // The retail data: every herd that started hostile in OpenSpore (carnivores) is
        // 2 or 5, the social herds and the miniboss are 3.
        use crate::creature_ecology::Archetype as A;
        for archetype in [A::Combat, A::SmallCombat, A::MediumCombat, A::LargeCombat, A::NightPredator, A::EpicPredator] {
            assert!(archetype.hostile(), "{archetype:?}");
        }
        for archetype in [A::Social, A::SmallSocial, A::MediumSocial, A::LargeSocial, A::MiniBoss, A::Carcass] {
            assert!(!archetype.hostile(), "{archetype:?}");
        }
        // A wary herd that is befriended or killed off stops attacking; a hostile one
        // stays hostile under kills (0xBA43E0) and stops once it is impressed.
        let t = [1, 2, 3, 3];
        assert!(attacks_on_sight(species_relationship(2, 0, 0, t)));
        assert!(!attacks_on_sight(species_relationship(2, 1, 0, t)));
        assert!(!attacks_on_sight(species_relationship(2, 0, 3, t)));
        assert!(attacks_on_sight(species_relationship(5, 0, 3, t)));
        assert!(!attacks_on_sight(species_relationship(5, 1, 0, t)));
    }

    #[test]
    fn brain_level_reads_the_stage_total_against_cumulative_costs() {
        // 0xD2E830: first L with total < sum(costs[0..=L]) -> 100 / 300 / 600 / 1000.
        assert_eq!(BRAIN_LEVEL_TOTALS, [100.0, 300.0, 600.0, 1000.0]);
        let mut sum = 0.0;
        for (level, cost) in EVO_POINTS_TO_LEVEL.iter().enumerate() {
            sum += cost;
            assert_eq!(BRAIN_LEVEL_TOTALS[level], sum);
        }
        for (total, level) in [
            (0.0, 0),
            (99.9, 0),
            (100.0, 1),
            (299.9, 1),
            (300.0, 2),
            (599.9, 2),
            (600.0, 3),
            (999.9, 3),
            (1000.0, 4),
            (50_000.0, 4),
        ] {
            assert_eq!(brain_level_from_total(total), level, "total {total}");
        }
        assert_eq!(brain_level_from_total(f32::NAN), 0);
        assert_eq!(brain_level_from_total(f32::INFINITY), 0);
        assert_eq!(STAGE_TOTAL_DNA, 1000.0);
    }

    #[test]
    fn costs_per_level_and_progress_bar_use_dna_into_the_level() {
        assert_eq!(evo_points_to_next_level(0), 100.0);
        assert_eq!(evo_points_to_next_level(3), 400.0);
        assert_eq!(evo_points_to_next_level(4), 0.0);
        assert_eq!(evo_points_to_next_level(-1), 0.0);
        // Level 2 spans totals 300..600: 450 is halfway through the 300 cost.
        assert_eq!(total_for_level(2), 300.0);
        assert_eq!(dna_into_level(450.0, 2), 150.0);
        assert!(close(level_progress(450.0), 0.5));
        assert!(close(level_progress(0.0), 0.0));
        assert!(close(level_progress(100.0), 0.0), "a new level starts empty");
        assert!(close(level_progress(250.0), 0.75));
        assert_eq!(level_progress(1000.0), 1.0, "the last level is full");
        assert_eq!(level_progress(2500.0), 1.0);
    }

    #[test]
    fn level_up_fires_when_the_dna_into_the_level_covers_its_cost() {
        assert!(!level_up_due(99.0, 0));
        assert!(level_up_due(100.0, 0));
        assert!(!level_up_due(299.0, 1));
        assert!(level_up_due(300.0, 1));
        assert!(!level_up_due(5000.0, 4), "no level-up past the last level");
        assert!(level_up_due(1000.0, 3));
    }

    #[test]
    fn migrations_unlock_at_150_and_650_of_the_stage_total() {
        assert_eq!(MIGRATION_DNA_LEVELS, [150.0, 650.0]);
        assert_eq!(migrations_unlocked(149.0), 0);
        assert_eq!(migrations_unlocked(150.0), 1);
        assert_eq!(migrations_unlocked(649.0), 1);
        assert_eq!(migrations_unlocked(650.0), 2);
    }

    #[test]
    fn two_dna_counters_cap_the_spendable_pool_and_never_lower_the_total() {
        assert_eq!(add_evolution_points(10.0, 10.0, 25.0), (35.0, 35.0));
        // 0xD2E8A0: spendable = min(spendable + p, 9999); total += p.
        assert_eq!(add_evolution_points(9990.0, 9990.0, 100.0), (9999.0, 10_090.0));
        assert_eq!(add_evolution_points(5.0, 5.0, 0.0), (5.0, 5.0));
        assert_eq!(add_evolution_points(5.0, 5.0, -3.0), (5.0, 5.0));
        assert_eq!(add_evolution_points(5.0, 5.0, f32::NAN), (5.0, 5.0));
        // Spending touches only the spendable pool.
        assert_eq!(spend_dna(100.0, 30.0), 70.0);
        assert_eq!(spend_dna(10.0, 30.0), 0.0);
        assert_eq!(spend_dna(10.0, -5.0), 10.0);
        // Death loses no DNA.
        assert_eq!(AVATAR_DEATH_EVO_POINTS_KEPT, 1.0);
        assert_eq!(AVATAR_DEATH_MAX_PROGRESS_REMOVED, 0.0);
        assert_eq!(dna_after_death(70.0, 450.0), (70.0, 450.0));
        // 0xD401E0: the total is cut to a multiple of max(1, removed).
        assert_eq!(dna_after_death(70.0, 450.75), (70.0, 450.0));
        assert_eq!(dna_after_death(70.0, 0.4), (70.0, 0.0));
    }

    #[test]
    fn dna_multiplier_follows_two_times_brain_minus_strength() {
        let m = |brain, strength| dna_points_multiplier(brain, strength, Difficulty::Medium);
        assert_eq!(m(0, 0), 1.0);
        assert_eq!(m(1, 0), 0.15, "index 2");
        assert_eq!(m(1, 1), 0.3, "index 1");
        assert_eq!(m(1, 2), 1.0, "index 0");
        assert_eq!(m(2, 0), 0.01, "index 4");
        assert_eq!(m(4, 0), 0.01, "index 8");
        assert_eq!(m(1, 5), 1.0, "a stronger target pays in full (index < 0)");
        assert_eq!(m(0, 10), 1.0);
        // An index past the table reads the last entry instead of panicking.
        assert_eq!(m(100, 0), 0.01);
        // Difficulty only changes the table.
        assert_eq!(dna_points_multiplier(1, 1, Difficulty::Easy), 0.5);
        assert_eq!(dna_points_multiplier(1, 2, Difficulty::Easy) , 1.0);
        assert_eq!(dna_points_multiplier(1, 0, Difficulty::Easy), 0.25);
        assert_eq!(dna_points_multiplier(1, 1, Difficulty::Hard), 0.2);
        assert_eq!(dna_points_multiplier(1, 0, Difficulty::Hard), 0.1);
        for table in [
            DNA_POINTS_MULTIPLIERS_EASY,
            DNA_POINTS_MULTIPLIERS_MEDIUM,
            DNA_POINTS_MULTIPLIERS_HARD,
        ] {
            assert_eq!(table.len(), 11);
            assert!(table[3..].iter().all(|v| *v == 0.01));
        }
    }

    #[test]
    fn dna_reward_scales_the_species_base_and_is_at_least_one() {
        let reward = |base, baby, alpha, brain, strength| {
            creature_dna_reward(base, baby, alpha, brain, strength, Difficulty::Medium)
        };
        assert_eq!(reward(DEFAULT_SPECIES_DNA, false, false, 0, 4), 100.0);
        assert_eq!(reward(DEFAULT_SPECIES_DNA, true, false, 0, 4), 25.0, "baby x0.25");
        assert_eq!(reward(DEFAULT_SPECIES_DNA, false, true, 0, 4), 125.0, "alpha x1.25");
        // A baby is never an alpha: the baby factor wins (0xC042A0 tests age first).
        assert_eq!(reward(DEFAULT_SPECIES_DNA, true, true, 0, 4), 25.0);
        assert!(close(reward(DEFAULT_SPECIES_DNA, false, false, 1, 1), 30.0));
        assert!(close(reward(DEFAULT_SPECIES_DNA, false, false, 1, 0), 15.0));
        // The result is never below 1 (0.01 x 100 = 1; 0.01 x 20 = 0.2 -> 1).
        assert_eq!(reward(DEFAULT_SPECIES_DNA, false, false, 3, 0), 1.0);
        assert_eq!(reward(20.0, false, false, 3, 0), 1.0);
        assert_eq!(reward(0.0, false, false, 0, 0), 1.0);
        // Eating gives nothing; an egg gives 25.
        assert_eq!(EVO_POINTS_PER_FRUIT, 0.0);
        assert_eq!(EVO_POINTS_PER_CARCASS, 0.0);
        assert_eq!(EVO_POINTS_PER_EGG, 25.0);
    }

    #[test]
    fn npc_strength_doubles_the_signature_ability_level() {
        assert_eq!(npc_strength_rating(Capabilities::default()), 0);
        let rank = |bite, sing| Capabilities {
            bite,
            sing,
            ..Default::default()
        };
        assert_eq!(npc_strength_rating(rank(1, 0)), 2);
        assert_eq!(npc_strength_rating(rank(1, 3)), 6);
        assert_eq!(npc_strength_rating(rank(5, 5)), NPC_MAX_STRENGTH);
        // A species whose signature rank equals the player's brain level pays in full.
        assert_eq!(
            dna_points_multiplier(2, npc_strength_rating(rank(2, 0)), Difficulty::Medium),
            1.0
        );
    }

    #[test]
    fn size_tables_and_rescale_follow_the_brain_level() {
        assert_eq!(SCALE_FOR_BRAIN_LEVEL, [1.0, 1.15, 1.3, 1.45, 1.6, 1.75]);
        assert_eq!((BABY_SCALE, ALPHA_SCALE), (0.6, 1.1));
        assert_eq!(size_factor(0), 1.0);
        assert_eq!(size_factor(4), 1.6);
        assert_eq!(size_factor(9), 1.75, "levels past 5 read the last entry");
        // 0xD2E720: a species outside the range is scaled into it.
        assert!(close(normalizing_scale(0.25, 0), 2.0), "min / s");
        assert!(close(normalizing_scale(4.0, 0), 0.5), "max / s");
        assert_eq!(normalizing_scale(1.0, 0), 1.0);
        assert_eq!(normalizing_scale(0.9, 2), 1.0 / 0.9);
        assert_eq!(normalizing_scale(f32::NAN, 2), 1.0);
        assert_eq!(BRAIN_LEVEL_SIZE_RANGES[4], (1.0, 2.8));
        // 0xD3FCA0: factor ratio (old/new normalizing scales cancel for size 1).
        assert!(close(rescale_factor(0, 1, 1.0), 1.15));
        assert!(close(rescale_factor(1, 2, 1.0), 1.3 / 1.15));
        assert!(close(avatar_scale(1.0, 4), 1.6));
        // Rescaling step by step equals the direct scale.
        let stepped = rescale_factor(0, 1, 1.0) * rescale_factor(1, 2, 1.0) * rescale_factor(2, 3, 1.0);
        assert!(close(stepped, size_factor(3)));
        // Small lineages grow into the level's minimum size.
        assert!(close(avatar_scale(0.5, 2), 1.0 * 1.3), "0.5 is lifted to 1.0, then x1.3");
    }

    #[test]
    fn health_table_parts_bonus_and_multipliers() {
        assert_eq!(HEALTH_TABLE, [10.0, 15.0, 25.0, 40.0, 100.0, 100.0, 100.0]);
        assert_eq!(base_health(0), 10.0);
        assert_eq!(base_health(3), 40.0);
        assert_eq!(base_health(4), 100.0);
        assert_eq!(base_health(-1), 10.0);
        assert_eq!(base_health(99), 100.0);
        assert_eq!(parts_health_bonus(0), 0.0);
        assert_eq!(parts_health_bonus(3), 6.0);
        // 0xC04170: table + parts bonus (a record value replaces the table).
        assert_eq!(base_hit_points(2, None, 4.0), 29.0);
        assert_eq!(base_hit_points(2, Some(12.0), 4.0), 16.0);
        assert_eq!(base_hit_points(2, Some(0.0), 4.0), 29.0);
        // 0xC05D50: avatar multiplier 1.0, babies a quarter, alphas 1.25.
        assert_eq!(max_hit_points(40.0, true, false, false), 40.0);
        assert_eq!(max_hit_points(40.0, false, true, false), 20.0);
        assert_eq!(max_hit_points(40.0, false, false, true), 50.0);
        assert_eq!(max_hit_points(40.0, true, true, true), 20.0, "baby wins over alpha");
        assert_eq!(avatar_max_hp(0, 0), 10.0);
        assert_eq!(avatar_max_hp(4, 5), 110.0);
        assert_eq!(npc_max_hp(0), 10.0, "an NPC species starts at the level-0 entry");
        assert_eq!(npc_max_hp(2), 14.0);
    }

    #[test]
    fn hunger_drains_after_the_delay_and_starves_below_one() {
        let drain = hunger_drain(Difficulty::Medium);
        let delay = hunger_decay_delay(Difficulty::Medium);
        assert_eq!((drain, delay), (1.111, 10.0));
        assert_eq!(hunger_drain(Difficulty::Easy), 0.833);
        assert_eq!(hunger_drain(Difficulty::Hard), 1.67);
        assert_eq!(hunger_decay_delay(Difficulty::Easy), 15.0);
        assert_eq!(hunger_decay_delay(Difficulty::Hard), 5.0);
        // Full: held for 10 s, then it falls.
        let (mut food, mut hold) = (MOTIVE_MAX, 0.0);
        for _ in 0..90 {
            (food, hold) = hunger_tick(food, MOTIVE_MAX, hold, 0.1, drain, delay);
        }
        assert_eq!(food, MOTIVE_MAX, "9 s: still held");
        for _ in 0..20 {
            (food, hold) = hunger_tick(food, MOTIVE_MAX, hold, 0.1, drain, delay);
        }
        assert!(food < MOTIVE_MAX && food > 97.0, "2 s past the delay: {food}");
        // 100 -> 0 takes about 90 s of drain on medium.
        let (mut food, mut hold) = (MOTIVE_MAX - 0.001, 0.0);
        let mut seconds = 0.0;
        while food > MOTIVE_MIN && seconds < 200.0 {
            (food, hold) = hunger_tick(food, MOTIVE_MAX, hold, 0.05, drain, delay);
            seconds += 0.05;
        }
        assert!((89.0..91.0).contains(&seconds), "{seconds}");
        assert_eq!(food, MOTIVE_MIN);
        // Eating back to the maximum restarts the hold.
        let (food, hold) = hunger_tick(MOTIVE_MAX, MOTIVE_MAX, 0.0, 1.0, drain, delay);
        assert_eq!((food, hold), (MOTIVE_MAX, 1.0));
        // A zero or invalid step changes nothing.
        assert_eq!(hunger_tick(50.0, 100.0, 0.0, 0.0, drain, delay), (50.0, 0.0));
        assert_eq!(hunger_tick(50.0, 100.0, 0.0, f32::NAN, drain, delay), (50.0, 0.0));
        // Starvation: 0.05 HP/s below hunger 1.
        assert_eq!(starvation_damage(0.9, 2.0), 0.1);
        assert_eq!(starvation_damage(1.0, 2.0), 0.0);
        assert_eq!(starvation_damage(50.0, 2.0), 0.0);
        assert_eq!(starvation_damage(0.0, 0.0), 0.0);
    }

    #[test]
    fn eating_refills_hunger_and_heals_per_the_motive_tuning() {
        assert_eq!(HUNGER_PER_MEAT_PER_SEC, 15.0);
        assert_eq!(HUNGER_PER_FRUIT, 25.0);
        assert_eq!(HUNGER_PER_EGG, 50.0);
        assert_eq!(HEALTH_PER_FRUIT, 0.25);
        assert_eq!(HEALTH_PER_EGG, 0.33);
        let (hunger, hp) = eat_carcass(100.0, 2.0, Difficulty::Medium);
        assert_eq!((hunger, hp), (30.0, 3.5), "15/s hunger and 1.75/s HP");
        let (hunger, hp) = eat_carcass(100.0, 2.0, Difficulty::Hard);
        assert_eq!((hunger, hp), (30.0, 3.0), "hard heals 1.5/s");
        // The last bite is bounded by the food left.
        assert_eq!(eat_carcass(10.0, 2.0, Difficulty::Medium).0, 10.0);
        assert_eq!(eat_carcass(0.0, 2.0, Difficulty::Medium), (0.0, 0.0));
        // Healing and resting fractions of the maximum HP (0xC02E30).
        assert!(close(healing_per_second(40.0), 3.0));
        assert_eq!(RESTING_HEALTH_RECOVERY, 0.0);
        assert_eq!(POSSE_HEALTH_RECOVERY, 0.1);
    }

    #[test]
    fn ability_tables_match_the_resolved_properties() {
        let bite = |level| combat_ability(CombatKind::Bite, level).unwrap();
        let charge = |level| combat_ability(CombatKind::Charge, level).unwrap();
        let strike = |level| combat_ability(CombatKind::Strike, level).unwrap();
        let spit = |level| combat_ability(CombatKind::Spit, level).unwrap();
        assert!(combat_ability(CombatKind::Bite, 0).is_none());
        for (level, damage) in [(1, 1.0), (2, 1.5), (3, 2.0), (4, 2.5), (5, 3.0), (9, 3.0)] {
            assert_eq!(bite(level).damage, damage);
            assert_eq!(charge(level).damage, damage);
        }
        assert_eq!((bite(1).refresh, bite(1).range, bite(1).range_min), (1.5, 1.0, 0.0));
        let refresh: Vec<f32> = (1..=5).map(|l| charge(l).refresh).collect();
        assert_eq!(refresh, [10.0, 12.0, 10.0, 8.0, 6.0]);
        let min: Vec<f32> = (1..=5).map(|l| charge(l).range_min).collect();
        assert_eq!(min, [14.0, 10.0, 10.0, 10.0, 10.0]);
        assert_eq!(charge(3).effect, CombatEffect::Stun { seconds: 2.0 });
        assert_eq!(charge(1).effect, CombatEffect::Stun { seconds: 1.5 });
        assert_eq!(charge(5).effect, CombatEffect::Stun { seconds: 2.5 });
        assert_eq!((CHARGE_RUSH_RANGE, CHARGE_RUSH_SPEED), (20.0, 20.0));
        let strike_damage: Vec<f32> = (1..=5).map(|l| strike(l).damage).collect();
        assert_eq!(strike_damage, [3.0, 4.5, 6.0, 8.0, 9.0]);
        assert_eq!((strike(1).refresh, strike(1).range), (4.5, 1.0));
        assert_eq!(STRIKE_CONE, (1.0, 2.0));
        let spit_damage: Vec<f32> = (1..=5).map(|l| spit(l).damage).collect();
        assert_eq!(spit_damage, [0.5, 1.0, 1.5, 2.0, 2.5]);
        assert_eq!((spit(2).refresh, spit(2).range, spit(2).range_min), (3.0, 30.0, 0.5));
        assert_eq!(
            spit(5).effect,
            CombatEffect::Poison {
                seconds: 6.0,
                damage_per_second: 0.5833
            }
        );
        assert_eq!(
            spit(1).effect,
            CombatEffect::Poison {
                seconds: 6.0,
                damage_per_second: 0.25
            }
        );
        assert_eq!(SPIT_PROJECTILE_SPEED, 50.0);
        // The hit buffer extends every range.
        assert_eq!(bite(1).reach(), 2.5);
        assert_eq!(spit(1).reach(), 31.5);
        // Tray order and cCreatureAbility types.
        assert_eq!(CombatKind::from_choice(0), Some(CombatKind::Bite));
        assert_eq!(CombatKind::from_choice(3), Some(CombatKind::Spit));
        assert_eq!(CombatKind::from_choice(4), None);
        let types: Vec<u32> = CombatKind::ALL.iter().map(|k| k.ability_type()).collect();
        assert_eq!(types, [30, 31, 33, 32]);
    }

    #[test]
    fn social_block_call_and_epic_values() {
        assert_eq!(social_power(1), 1.0);
        assert_eq!(social_power(5), 5.0);
        assert_eq!(social_power(9), 5.0);
        assert_eq!((SOCIAL_REFRESH, SOCIAL_RANGE), (5.0, 5.0));
        assert_eq!((BLOCK_DURATION, BLOCK_REFRESH, CALL_RANGE), (2.0, 10.0, 10.0));
        assert_eq!((EPIC_STOMP_DAMAGE, EPIC_STOMP_REFRESH, EPIC_STOMP_RANGE), (6.0, 3.0, 15.0));
        assert_eq!((EPIC_SPIT_DAMAGE, EPIC_SPIT_POISON), (6.0, (4.0, 2.0)));
    }

    #[test]
    fn sprint_and_stealth_tables() {
        assert!(sprint(0).is_none() && stealth(0).is_none());
        let durations: Vec<f32> = (1..=5).map(|l| sprint(l).unwrap().duration).collect();
        let recharges: Vec<f32> = (1..=5).map(|l| sprint(l).unwrap().recharge).collect();
        assert_eq!(durations, [10.0, 15.0, 20.0, 25.0, 30.0]);
        assert_eq!(recharges, [120.0, 105.0, 90.0, 75.0, 60.0]);
        assert_eq!(sprint(1).unwrap().refresh, 25.0);
        // 0xC1AC30: the used time falls by duration / recharge per second.
        assert!(close(sprint(1).unwrap().recovery_per_second(), 10.0 / 120.0));
        assert!(close(sprint(5).unwrap().recovery_per_second(), 0.5));
        let stealth_durations: Vec<f32> = (1..=5).map(|l| stealth(l).unwrap().duration).collect();
        let stealth_recharges: Vec<f32> = (1..=5).map(|l| stealth(l).unwrap().recharge).collect();
        assert_eq!(stealth_durations, [10.0, 11.0, 13.0, 16.0, 20.0]);
        assert_eq!(stealth_recharges, [60.0, 50.0, 40.0, 35.0, 30.0]);
        assert_eq!(stealth_range(1), Some((16.0, 5.0)));
        assert_eq!(stealth_range(3), Some((8.0, 2.5)));
        assert_eq!(stealth_range(5), Some((2.0, 0.75)));
        assert_eq!(stealth_range(0), None);
    }

    #[test]
    fn jump_glide_and_gear_tables() {
        assert_eq!((JUMP_REFRESH, GLIDE_REFRESH), (0.5, 0.1));
        assert_eq!((MAX_JUMP_HEIGHT, GLIDE_SPEED), (3.5, 10.0));
        let flaps: Vec<u32> = (0..=6).map(glide_flaps).collect();
        assert_eq!(flaps, [0, 2, 4, 6, 8, 10, 10]);
        assert!(close(jump_height(5), 3.5));
        assert!(close(jump_height(1), 0.7));
        assert_eq!(jump_height(0), 0.0);
        assert_eq!(SPEED_GEARS, [2.5, 4.0, 5.5, 6.5, 7.5, 8.5]);
        assert_eq!(SPRINT_SPEED_BOOST, 3.0);
        assert_eq!(gear_speed(0), 2.5);
        assert_eq!(gear_speed(5), 8.5);
        assert_eq!(gear_speed(9), 8.5);
        assert_eq!(avatar_speed(2), 5.5);
        assert_eq!(sprint_speed(avatar_speed(0)), 5.5, "gear 0 sprints like gear 2");
        assert_eq!(sprint_speed(avatar_speed(5)), 11.5);
    }

    #[test]
    fn damage_pipeline_applies_stealth_without_randomness() {
        let bite3 = combat_ability(CombatKind::Bite, 3).unwrap();
        let strike5 = combat_ability(CombatKind::Strike, 5).unwrap();
        let spit5 = combat_ability(CombatKind::Spit, 5).unwrap();
        let charge2 = combat_ability(CombatKind::Charge, 2).unwrap();
        let avatar = AttackContext {
            attacker_is_avatar: true,
            ..Default::default()
        };
        assert_eq!(attack_damage(&bite3, &avatar), 2.0);
        let stealthed = AttackContext {
            attacker_stealthed: true,
            ..avatar
        };
        assert_eq!(attack_damage(&bite3, &stealthed), 3.0, "stealth x1.5 on a bite");
        assert_eq!(attack_damage(&strike5, &stealthed), 13.5, "and on a strike");
        assert_eq!(attack_damage(&spit5, &stealthed), 2.5, "not on spit");
        assert_eq!(attack_damage(&charge2, &stealthed), 1.5, "not on charge");
        // No randomness: the damage percentage range is a single value.
        assert_eq!(DAMAGE_PERCENTAGE_MIN, DAMAGE_PERCENTAGE_MAX);
        assert_eq!(attack_damage(&bite3, &avatar), attack_damage(&bite3, &avatar));
        // A baby attacker deals a quarter.
        let baby = AttackContext {
            attacker_is_baby: true,
            ..Default::default()
        };
        assert_eq!(attack_damage(&bite3, &baby), 0.5);
        // The attacker's additional damage is added to effect-kind-0 abilities only.
        let strong = AttackContext {
            attacker_additional_damage: 1.0,
            ..Default::default()
        };
        assert_eq!(attack_damage(&bite3, &strong), 3.0);
        assert_eq!(attack_damage(&spit5, &strong), 2.5);
        // Posse members use the posse record; the avatar ignores it.
        let posse = AttackContext {
            posse_multiplier: Some(0.5),
            ..Default::default()
        };
        assert_eq!(attack_damage(&bite3, &posse), 1.0);
        assert_eq!(
            attack_damage(
                &bite3,
                &AttackContext {
                    attacker_is_avatar: true,
                    ..posse
                }
            ),
            2.0
        );
        // Boosts: damage x boost, divided by the target's armor.
        let boosted = AttackContext {
            damage_boost: Some(2.0),
            armor_boost: Some(4.0),
            target_is_baby: true,
            ..Default::default()
        };
        assert_eq!(attack_damage(&bite3, &boosted), 1.0);
        assert_eq!(NPC_ATTACK_MISS_PERCENTAGE, 0.0);
    }

    #[test]
    fn knockback_stun_poison_and_charge_interruption() {
        assert_eq!(charge_knockback_impulse(1.0), 0.1, "clamped to the minimum");
        assert!(close(charge_knockback_impulse(30.0), 0.3));
        assert_eq!(charge_knockback_impulse(500.0), 1.0, "clamped to the maximum");
        assert!(spit_interrupts_charge(CombatKind::Spit, true));
        assert!(!spit_interrupts_charge(CombatKind::Spit, false));
        assert!(!spit_interrupts_charge(CombatKind::Bite, true));
        assert_eq!(avatar_stun_seconds(0.0), 1.5, "3 s x 0.5");
        assert_eq!(avatar_stun_seconds(1.0), 2.0, "4 s x 0.5");
        assert_eq!(avatar_stun_seconds(7.0), 2.0);
        // Poison: HP per second for the effect duration, then nothing.
        let (damage, left) = poison_tick(6.0, 0.5, 1.0);
        assert_eq!((damage, left), (0.5, 5.0));
        let (damage, left) = poison_tick(0.25, 0.5, 1.0);
        assert_eq!((damage, left), (0.125, 0.0), "the last tick is bounded");
        assert_eq!(poison_tick(0.0, 0.5, 1.0), (0.0, 0.0));
        let total: f32 = {
            let mut left = SPIT_POISON_SECONDS;
            let mut sum = 0.0;
            while left > 0.0 {
                let (d, l) = poison_tick(left, SPIT_POISON_DPS[4], 0.1);
                sum += d;
                left = l;
            }
            sum
        };
        assert!((total - 6.0 * 0.5833).abs() < 0.01, "level 5 poison totals {total}");
    }

    #[test]
    fn brain_upgrade_cinematics_are_listed_for_the_cinematic_work() {
        assert_eq!(brain_upgrade_cinematic(0), None);
        assert_eq!(brain_upgrade_cinematic(1), Some("CRG_BrainUpgrade1"));
        assert_eq!(brain_upgrade_cinematic(4), Some("CRG_BrainUpgrade4"));
        assert_eq!(brain_upgrade_cinematic(5), None);
        assert_eq!(cinematics::STAGE_END, "CRG2TRG");
        assert_eq!(cinematics::STAGE_START, "CLG2CRG");
    }

    #[test]
    fn a_brain_level_up_rescales_heals_and_refills_the_avatar() {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = App::new();
        app.insert_resource(run)
            .insert_resource(crate::Originals::default())
            .add_systems(Update, apply_brain_level);
        let player = app
            .world_mut()
            .spawn((
                Player,
                Health { hp: 3.0, max: 10.0 },
                Hunger {
                    food: 20.0,
                    max: 100.0,
                    hold: 4.0,
                },
                Transform::from_scale(Vec3::splat(1.0)),
            ))
            .id();
        app.update();
        // The first observation only adopts the current level.
        assert_eq!(app.world().get::<Health>(player).unwrap().hp, 3.0);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_progression
            .brain_level = 1;
        app.update();
        // Level 1: x1.15 size, health table entry 15 (headless parts give no
        // bonus), hunger refilled (0xD3FCA0 calls SetHunger(MotiveMax)).
        let scale = app.world().get::<Transform>(player).unwrap().scale;
        assert!((scale.x - 1.15).abs() < 1e-5, "{scale:?}");
        let health = app.world().get::<Health>(player).unwrap();
        assert_eq!((health.hp, health.max), (15.0, 15.0));
        let hunger = app.world().get::<Hunger>(player).unwrap();
        assert_eq!((hunger.food, hunger.hold), (100.0, 0.0));
        // The next level multiplies by the factor ratio, not the factor.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_progression
            .brain_level = 2;
        app.update();
        let scale = app.world().get::<Transform>(player).unwrap().scale;
        assert!((scale.x - 1.3).abs() < 1e-5, "{scale:?}");
        assert_eq!(app.world().get::<Health>(player).unwrap().max, 25.0);
        // No change, no effect: damage taken stays.
        app.world_mut().get_mut::<Health>(player).unwrap().hp = 4.0;
        app.update();
        assert_eq!(app.world().get::<Health>(player).unwrap().hp, 4.0);
        // A new avatar (a stage re-entry builds it for its level) is only
        // adopted: no second rescale, heal or refill.
        app.world_mut().despawn(player);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_progression
            .brain_level = 4;
        let fresh = app
            .world_mut()
            .spawn((
                Player,
                Health { hp: 3.0, max: 10.0 },
                Hunger {
                    food: 20.0,
                    max: 100.0,
                    hold: 0.0,
                },
                Transform::from_scale(Vec3::splat(1.6)),
            ))
            .id();
        app.update();
        assert_eq!(app.world().get::<Health>(fresh).unwrap().hp, 3.0);
        assert_eq!(app.world().get::<Hunger>(fresh).unwrap().food, 20.0);
        assert!((app.world().get::<Transform>(fresh).unwrap().scale.x - 1.6).abs() < 1e-6);
    }

    #[test]
    fn a_charge_shove_moves_the_creature_once_and_never_into_the_sea() {
        let mut app = App::new();
        app.insert_resource(crate::creature_planet::tests::island_surface(200.0));
        app.add_systems(Update, apply_knockback);
        let mind = crate::creature_actions::CreatureMind {
            shove: [1.5, -0.5],
            ..default()
        };
        let creature = app
            .world_mut()
            .spawn((Transform::from_xyz(2.0, 0.4, 3.0), mind))
            .id();
        let edge = app
            .world_mut()
            .spawn((
                Transform::from_xyz(185.0, 0.4, 0.0),
                crate::creature_actions::CreatureMind {
                    shove: [30.0, 0.0],
                    ..default()
                },
            ))
            .id();
        app.update();
        let at = app.world().get::<Transform>(creature).unwrap().translation;
        assert_eq!((at.x, at.z), (3.5, 2.5));
        assert_eq!(
            app.world()
                .get::<crate::creature_actions::CreatureMind>(creature)
                .unwrap()
                .shove,
            [0.0, 0.0]
        );
        let at = app.world().get::<Transform>(edge).unwrap().translation;
        assert_eq!(at.x, 185.0, "never into the sea");
        app.update();
        let again = app.world().get::<Transform>(creature).unwrap().translation;
        assert_eq!((again.x, again.z), (3.5, 2.5), "a shove applies once");
    }

    #[test]
    fn social_answers_follow_the_retail_power_formula_and_never_subtract() {
        // Power = the ability level; a wrong answer adds 0.4 x power.
        assert!(close(social_answer_gain(1, true, false), 100.0 / 3.0));
        assert!(close(social_answer_gain(3, true, false), 100.0));
        assert!(close(social_answer_gain(5, true, false), 500.0 / 3.0));
        assert!(close(social_answer_gain(5, false, false), 200.0 / 3.0));
        assert!(close(social_answer_gain(1, false, false), 40.0 / 3.0));
        // A helping posse member doubles the amount.
        assert!(close(social_answer_gain(1, true, true), 200.0 / 3.0));
        assert!(close(social_answer_gain(2, false, true), 160.0 / 3.0));
        // Never negative, and no part (level 0) adds nothing.
        assert_eq!(social_answer_gain(0, true, true), 0.0);
        assert_eq!(INCORRECT_ABILITY_MULTIPLIER, 0.4);
        assert_eq!(SOCIAL_POSSE_MULTIPLIER, 2.0);
        assert_eq!((SOCIAL_DEMONSTRATION_SECONDS, SOCIAL_RESPONSE_TIMEOUT), (5.0, 15.0));
    }

    #[test]
    fn combat_flee_call_and_perception_tuning_values() {
        assert_eq!((ATTACK_DISTANCE, CHASE_DISTANCE, UNTARGET_DISTANCE), (5.0, 5.0, 17.5));
        assert_eq!(GOAL_DISTANCE_LIMIT, 50.0);
        assert_eq!(attack_tether_distance(Difficulty::Easy), 45.0);
        assert_eq!(attack_tether_distance(Difficulty::Medium), 60.0);
        assert_eq!(attack_tether_distance(Difficulty::Hard), 90.0);
        assert_eq!((FIGHT_NUM_SLOTS, FIGHT_NO_ACTIVITY_TIME_LIMIT), (4, 5.0));
        assert_eq!(
            (WARY_DISTANCE, HELP_ALLY_IN_RADIUS, HIT_POINTS_TURN_AND_FIGHT_PERCENTAGE),
            (6.0, 10.0, 0.75)
        );
        assert_eq!((POUNCE_PERCENTAGE, NO_ATTACK_TIMER), (0.001, 30.0));
        assert_eq!((FLEE_DISTANCE, FLEE_TIME, FLEE_REMEMBER_TIME), (25.0, 5.0, 90.0));
        assert_eq!((FLEE_TIME_TO_TIRED, FLEE_TIRED_TIME), (3.0, 3.0));
        assert_eq!((FLEE_DODGE_DISTANCE, FLEE_DODGE_TIME, FLEE_JUMP_FREQUENCY), (0.25, 3.0, 4.0));
        assert_eq!((FLEE_STEALTH_TIME, FLEE_WATCH_TIME), (10.0, 10.0));
        assert_eq!(FLEE_DIRECTION_BIASES, [0.5, 1.0, 1.0, 1.0]);
        assert_eq!((CALL_START_RADIUS, CALL_EXPANSION_RATE, CALL_DURATION), (10.0, 20.0, 3.0));
        assert_eq!(CALL_MAX_LOUDNESS, 500.0);
        assert_eq!(CALL_MAX_RESPONDING, (7, 7, 2));
        assert_eq!(CALL_PERCENT_CHANCE_RESPONDER_ATTACKS, 0.1);
        assert_eq!((BASE_PERCEPTION, MAX_PERCEPTION_RADIUS, SIGHT_RADIUS), (10.0, 50.0, 17.0));
        assert_eq!((SIGHT_ANGLE, BASE_LOUDNESS), (120.0, 4.0));
        assert_eq!(LOUDNESS_MULTIPLIERS, (1.5, 2.0, 3.0));
    }

    #[test]
    fn difficulty_comes_from_the_game_setting() {
        assert_eq!(Difficulty::from_index(0), Difficulty::Easy);
        assert_eq!(Difficulty::from_index(1), Difficulty::Medium);
        assert_eq!(Difficulty::from_index(2), Difficulty::Hard);
        assert_eq!(Difficulty::from_index(-3), Difficulty::Easy);
        assert_eq!(Difficulty::from_index(7), Difficulty::Hard);
        assert_eq!(Difficulty::default(), Difficulty::Medium);
    }
}
