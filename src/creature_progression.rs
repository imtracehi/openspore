//! Creature-stage brain ranks, posse capacity, and nest migration.
//!
//! Brain levels and migration thresholds are the retail rules (see
//! `creature_rules`, notes C1): the level is read from the STAGE TOTAL against
//! the cumulative costs 100 / 300 / 600 / 1000 and the species migrates at the
//! totals 150 and 650. The migration is decided when the avatar is born at its
//! nest (0xD40FA0: after a death and on the return from the editor), against
//! the DNA the previous migration was taken at (`last_migration_dna`, the
//! player flag 0x054FB6BF), see [`CreatureProgression::migration_due`].
//! Pack-capacity rank gates and migration destination selection are OpenSpore
//! adaptations: the executable code read so far has no posse cap per brain
//! level (`cPlayer::mMaxPosseSize` is only written by the Space stage).

use crate::creature_rules as rules;
use serde::{Deserialize, Serialize};

/// `creatureEvoPointsToLevel_1..4`: the DNA each brain level COSTS (retail
/// 0xD2E380). The levels start at the cumulative sums of these (100, 300, 600,
/// 1000); see `brain_level`, `dna_into_level` and `level_progress`.
pub const BRAIN_LEVEL_DNA: [f32; 4] = rules::EVO_POINTS_TO_LEVEL;
/// The stage totals at which brain levels 1..=4 start (the cumulative sums of
/// `BRAIN_LEVEL_DNA`): 100, 300, 600, 1000.
pub const BRAIN_LEVEL_TOTALS: [f32; 4] = rules::BRAIN_LEVEL_TOTALS;
pub const MAX_BRAIN_LEVEL: u8 = rules::MAX_BRAIN_LEVEL;

/// The retail SDK's player object has `mMaxPosseSize`, but the local SDK and
/// PROP references do not define how that value scales with brain rank.
/// Adapt the prototype's three-member cap to native brain ranks: pack play is
/// locked at rank 0, then expands to one, two, and three slots at ranks 1, 2,
/// and 3. Rank 4 retains three slots so the final Creature-stage brain level
/// is still reachable while the player can use their full pack.
pub const MAX_PACK_MEMBERS: usize = 3;
const PACK_CAPACITY_BY_BRAIN_LEVEL: [usize; 5] = [0, 1, 2, 3, 3];

/// `MigrationDNALevels` (CreatureGameTuning, retail 0xD40FA0): the player's
/// species migrates when the stage total reaches 150 and again at 650.
pub const MIGRATION_DNA_LEVELS: [f32; 2] = rules::MIGRATION_DNA_LEVELS;
/// Minimum squared XZ displacement used to reject a destination equal to home.
const MIN_NEST_CHANGE_DISTANCE_SQUARED: f32 = 0.25 * 0.25;
/// The avatar claims the destination nest once it is this close (retail
/// 0xD41510: the squared distance to the nest below `50 * 50`, the global
/// 0x1582FA4); it plays `CRG_ClaimNest` and the nest becomes its own.
pub const CLAIM_RADIUS: f32 = 50.0;

/// The brain-level range of a nest of the player's species (retail cHerd
/// `mActivateBrainLevel` +0x120 / `mDeactivateBrainLevel` +0x124): the nest
/// is the player's choice only while the avatar's level lies in it. A new herd
/// starts with (0, 5), i.e. every level (the herd constructor 0xC6C2B0); the
/// migration retires the old nest with (5, -1), which holds no level
/// (0xD40FA0 writes it). 0xD40AF0 picks the nearest such nest of the species
/// for the avatar's level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestRange {
    pub activate: i32,
    pub deactivate: i32,
}

impl NestRange {
    /// A herd's range from the start: every brain level 0..=5.
    pub const DEFAULT: NestRange = NestRange { activate: 0, deactivate: 5 };
    /// A nest the species has left.
    pub const RETIRED: NestRange = NestRange { activate: 5, deactivate: -1 };

    /// 0xD40AF0: `activate <= level <= deactivate`.
    pub fn holds(self, level: u8) -> bool {
        let level = level as i32;
        self.activate <= level && level <= self.deactivate
    }
}

/// Result of observing accumulated Creature-stage evolution points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionUpdate {
    pub brain_level: u8,
    /// Newly crossed rank numbers, emitted once and persisted by the state.
    pub newly_unlocked_brain_levels: Vec<u8>,
}

/// Persistent, serializable Creature progression state.
///
/// Missing fields in old saves produce the default state. Its first
/// observation adopts the save's existing DNA silently, so loading an old
/// save cannot replay unlock notifications or rewards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreatureProgression {
    pub initialized: bool,
    pub observed_dna: f32,
    pub brain_level: u8,
    pub migrations_completed: u8,
    pub pending_migration: Option<PendingMigration>,
    /// The stage total the last migration started at (retail player flag
    /// 0x054FB6BF, written by 0xD40FA0): only a `MigrationDNALevels` entry above
    /// it can start the next one.
    pub last_migration_dna: f32,
    /// The nests the species has left (their range is `NestRange::RETIRED`):
    /// never chosen as a destination again.
    pub retired_nests: Vec<[f32; 3]>,
    /// The Creature events whose `frequency` is spent (instance ids of
    /// `creature_events~!*`, `creature_events::step`): the saved part of the
    /// retail event manager (the rain storms, the meteor strike).
    pub events_started: Vec<u32>,
    /// The avatar's hunger as a fraction of its maximum: drawn once when the
    /// lineage's avatar is created (`start_hunger_fraction`) and then kept
    /// current by the Creature walk, so a stage entry continues it (`None`: not
    /// created yet, or an old save).
    pub avatar_hunger: Option<f32>,
    /// The DNA the player earned by fighting (cPlayer +0x127C: kill rewards,
    /// 0xC07BEC, and a species' extinction reward, 0xC2F0D2) and by social
    /// play (+0x1278: befriend rewards, 0xD9BD16, and a species' best-friends
    /// reward, 0xC2EFC9). Every `AddEvolutionPoints` of those four callers also
    /// adds the same amount to one of the two (0xC75840); the timeline's earned
    /// trait reads their balance (`creature_flow::trait_slot`). A save from
    /// before this field starts both at zero.
    pub attack_dna: f32,
    pub social_dna: f32,
}

impl Default for CreatureProgression {
    fn default() -> Self {
        Self {
            initialized: false,
            observed_dna: 0.0,
            brain_level: 0,
            migrations_completed: 0,
            pending_migration: None,
            last_migration_dna: 0.0,
            retired_nests: Vec::new(),
            events_started: Vec::new(),
            avatar_hunger: None,
            attack_dna: 0.0,
            social_dna: 0.0,
        }
    }
}

/// Pure: the hunger fraction a freshly created avatar starts with, from a uniform
/// `roll` in 0..1. VERIFIED (`cCreatureAnimal::Create` 0xC09B40, 0xC0ACFD..
/// 0xC0ADC5): `RandomFloat` in [`creatureStartingMotive_HungerMin`, `..Max`]
/// = (0.5, 1.0) of the maximum (`MotiveTuning` +0x4C / +0x50, loaded 0xB70EE0 /
/// 0xB70F17); the starting health is [1, 1] x the maximum (a full bar).
pub fn start_hunger_fraction(roll: f32) -> f32 {
    let (min, max) = rules::STARTING_HUNGER_FRACTIONS;
    min + (max - min) * roll.clamp(0.0, 1.0)
}

/// A persisted move from a current nest to a selected destination nest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PendingMigration {
    pub origin: [f32; 3],
    pub destination: [f32; 3],
}

/// Pure: brain rank from the Creature-stage DNA total (retail 0xD2E830:
/// cumulative 100 / 300 / 600 / 1000).
pub fn brain_level(dna: f32) -> u8 {
    rules::brain_level_from_total(dna)
}

/// Pure: DNA earned inside the current brain level (retail 0xD2E410).
pub fn dna_into_level(dna: f32) -> f32 {
    rules::dna_into_level(dna, brain_level(dna))
}

/// Pure: progress-bar fraction, DNA into the level over the level cost
/// (full at the last level).
pub fn level_progress(dna: f32) -> f32 {
    rules::level_progress(dna)
}

/// Pure: number of posse slots at the current DNA amount.
///
/// Capacity gates are OpenSpore tuning based on verified native brain ranks;
/// the SDK only exposes `cPlayer::mMaxPosseSize`, not its progression rule.
/// Capacity is locked below rank 1, then opens at the 100/300/600 stage-total
/// DNA rank thresholds. The third slot remains available at rank 4 so the
/// final threshold can still be pursued with the full pack.
pub fn pack_capacity(dna: f32) -> usize {
    PACK_CAPACITY_BY_BRAIN_LEVEL[brain_level(dna) as usize].min(MAX_PACK_MEMBERS)
}

impl CreatureProgression {
    /// Reconcile persisted rank state with cumulative Creature DNA. First use
    /// initializes legacy saves without reporting past ranks as new unlocks.
    pub fn observe_dna(&mut self, dna: f32) -> ProgressionUpdate {
        // Earned evolution points and brain rank are permanent lineage
        // progress. Ignore malformed observations and never lower the
        // persisted high-water mark after a spend or bad save value.
        let safe_dna = if dna.is_finite() {
            dna.max(0.0)
        } else if self.initialized && self.observed_dna.is_finite() {
            self.observed_dna.max(0.0)
        } else {
            0.0
        };
        if !self.initialized {
            self.initialized = true;
            self.observed_dna = safe_dna;
            self.brain_level = brain_level(safe_dna);
            return ProgressionUpdate {
                brain_level: self.brain_level,
                newly_unlocked_brain_levels: Vec::new(),
            };
        }

        let previous = self.brain_level.min(MAX_BRAIN_LEVEL);
        let high_water_dna = self.observed_dna.max(safe_dna);
        let current = brain_level(high_water_dna).max(previous);
        let mut newly_unlocked_brain_levels = Vec::new();
        if current > previous {
            newly_unlocked_brain_levels.extend((previous + 1)..=current);
        }
        self.observed_dna = high_water_dna;
        self.brain_level = current;
        ProgressionUpdate {
            brain_level: current,
            newly_unlocked_brain_levels,
        }
    }

    /// Stage total needed for the next migration (150, then 650); `None` once
    /// both migrations are done.
    pub fn next_migration_requirement(&self) -> Option<f32> {
        MIGRATION_DNA_LEVELS
            .get(self.migrations_completed as usize)
            .copied()
    }

    /// The DNA of the last migration. Saves from before `last_migration_dna`
    /// existed carry only the count: the level of the last completed migration
    /// stands in for it.
    pub fn last_dna(&self) -> f32 {
        let done = (self.migrations_completed as usize).min(MIGRATION_DNA_LEVELS.len());
        let floor = if done > 0 { MIGRATION_DNA_LEVELS[done - 1] } else { 0.0 };
        self.last_migration_dna.max(floor)
    }

    /// Retail 0xD40FA0: with the stage total rounded (`cvtss2si`), the species
    /// migrates when some `MigrationDNALevels` entry lies above the DNA of the
    /// last migration and at or below the total. A total past both entries
    /// starts one migration only (the last DNA then covers both).
    pub fn migration_due(&self, total: f32) -> bool {
        if !total.is_finite() {
            return false;
        }
        let now = total.max(0.0).round_ties_even();
        let last = self.last_dna();
        MIGRATION_DNA_LEVELS.iter().any(|&level| level > last && level <= now)
    }

    pub fn can_migrate(&self, dna: f32) -> bool {
        self.pending_migration.is_none() && self.migration_due(dna)
    }

    /// Begin a migration to the closest valid candidate nest. The caller
    /// supplies original migration-marker/nest positions from the current
    /// world. Nest choice uses XZ distance, and a destination must differ from
    /// home by at least 0.25 horizontal units.
    pub fn begin_migration(
        &mut self,
        dna: f32,
        current_home: [f32; 3],
        candidate_nests: &[[f32; 3]],
    ) -> Result<[f32; 3], MigrationError> {
        if self.pending_migration.is_some() {
            return Err(MigrationError::AlreadyInProgress);
        }
        let last = self.last_dna();
        let Some(requirement) = MIGRATION_DNA_LEVELS.iter().copied().find(|&level| level > last)
        else {
            return Err(MigrationError::AllMigrationsComplete);
        };
        if !self.migration_due(dna) {
            return Err(MigrationError::InsufficientDna { requirement });
        }
        // 0xD40FA0 retires the avatar's nest (range (5, -1)) before 0xD40AF0
        // looks for the nearest nest whose range holds the level: only the
        // nests that are not retired and not the one the species leaves count.
        let level = self.brain_level;
        let open: Vec<[f32; 3]> = candidate_nests
            .iter()
            .copied()
            .filter(|c| self.nest_range(*c, current_home).holds(level))
            .collect();
        let destination = select_migration_destination(current_home, &open).ok_or(MigrationError::NoDestination)?;
        self.pending_migration = Some(PendingMigration {
            origin: current_home,
            destination,
        });
        if !self.retired_nests.iter().any(|r| distance_xz_squared(*r, current_home) < MIN_NEST_CHANGE_DISTANCE_SQUARED) {
            self.retired_nests.push(current_home);
        }
        // 0xD40FA0 writes the rounded stage total as the last migration DNA.
        self.last_migration_dna = dna.max(0.0).round_ties_even();
        Ok(destination)
    }

    /// The brain-level range of the nest at `nest` while the species lives at
    /// `home`: a retired nest or the one being left holds no level.
    pub fn nest_range(&self, nest: [f32; 3], home: [f32; 3]) -> NestRange {
        let same = |a: [f32; 3], b: [f32; 3]| distance_xz_squared(a, b) < MIN_NEST_CHANGE_DISTANCE_SQUARED;
        if same(nest, home) || self.retired_nests.iter().any(|r| same(*r, nest)) {
            NestRange::RETIRED
        } else {
            NestRange::DEFAULT
        }
    }

    /// Complete the pending move when the player reaches its destination.
    /// `arrival_radius` is supplied by the world integration; the helper
    /// validates it and reports the new home only once.
    pub fn complete_migration(
        &mut self,
        position: [f32; 3],
        arrival_radius: f32,
    ) -> Result<[f32; 3], MigrationError> {
        let Some(pending) = self.pending_migration else {
            return Err(MigrationError::NoMigrationInProgress);
        };
        if !arrival_radius.is_finite() || arrival_radius < 0.0 {
            return Err(MigrationError::InvalidArrivalRadius);
        }
        if !position.iter().all(|v| v.is_finite()) {
            return Err(MigrationError::InvalidPosition);
        }
        if distance_xz_squared(position, pending.destination) > arrival_radius * arrival_radius {
            return Err(MigrationError::DestinationNotReached);
        }
        self.pending_migration = None;
        self.migrations_completed = self
            .migrations_completed
            .saturating_add(1)
            .min(MIGRATION_DNA_LEVELS.len() as u8);
        Ok(pending.destination)
    }

    pub fn cancel_migration(&mut self) -> bool {
        self.pending_migration.take().is_some()
    }
}

/// Select the closest finite candidate by horizontal XZ distance that is a
/// meaningful distance from the current nest. Y is ignored because nest
/// proximity is defined on the ground plane. Stable input order resolves ties.
pub fn select_migration_destination(
    current_home: [f32; 3],
    candidate_nests: &[[f32; 3]],
) -> Option<[f32; 3]> {
    if !current_home.iter().all(|v| v.is_finite()) {
        return None;
    }
    let nearest = |min_squared: f32| {
        candidate_nests
            .iter()
            .copied()
            .filter(|candidate| {
                candidate.iter().all(|v| v.is_finite())
                    && distance_xz_squared(current_home, *candidate) >= min_squared
            })
            .min_by(|a, b| {
                distance_xz_squared(current_home, *a).total_cmp(&distance_xz_squared(current_home, *b))
            })
    };
    // ADAPTED: a nest the avatar must travel to (beyond the claim radius, else
    // the avatar would claim it where it stands the moment it is chosen), the
    // nearest of any distance when none lies that far.
    nearest(CLAIM_RADIUS * CLAIM_RADIUS).or_else(|| nearest(MIN_NEST_CHANGE_DISTANCE_SQUARED))
}

fn distance_xz_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dz = a[2] - b[2];
    dx * dx + dz * dz
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MigrationError {
    AllMigrationsComplete,
    AlreadyInProgress,
    InsufficientDna { requirement: f32 },
    NoDestination,
    NoMigrationInProgress,
    InvalidArrivalRadius,
    InvalidPosition,
    DestinationNotReached,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_avatar_starts_between_half_and_full_hunger_and_keeps_it_in_the_save() {
        assert_eq!(start_hunger_fraction(0.0), 0.5);
        assert_eq!(start_hunger_fraction(1.0), 1.0);
        assert!((start_hunger_fraction(0.5) - 0.75).abs() < 1e-6);
        assert_eq!(start_hunger_fraction(7.0), 1.0, "a roll is clamped");
        let mut progress = CreatureProgression::default();
        assert_eq!(progress.avatar_hunger, None);
        progress.avatar_hunger = Some(0.62);
        let reloaded: CreatureProgression =
            serde_json::from_str(&serde_json::to_string(&progress).unwrap()).unwrap();
        assert_eq!(reloaded.avatar_hunger, Some(0.62));
        // An old save has no field: the avatar is rolled when it is created.
        let old: CreatureProgression = serde_json::from_str("{\"brain_level\": 2}").unwrap();
        assert_eq!((old.brain_level, old.avatar_hunger), (2, None));
    }

    #[test]
    fn brain_threshold_crossings_are_native_values_and_emit_once() {
        let mut progress = CreatureProgression::default();
        assert_eq!(progress.observe_dna(99.9).brain_level, 0);
        assert!(progress
            .observe_dna(99.9)
            .newly_unlocked_brain_levels
            .is_empty());
        // Retail levels start at the cumulative totals 100 / 300 / 600 / 1000.
        let crossed = progress.observe_dna(600.0);
        assert_eq!(crossed.brain_level, 3);
        assert_eq!(crossed.newly_unlocked_brain_levels, [1, 2, 3]);
        assert!(progress
            .observe_dna(600.0)
            .newly_unlocked_brain_levels
            .is_empty());
        let final_rank = progress.observe_dna(1000.0);
        assert_eq!(final_rank.newly_unlocked_brain_levels, [4]);
    }

    #[test]
    fn legacy_reload_adopts_existing_dna_without_replaying_unlocks() {
        let mut legacy: CreatureProgression = serde_json::from_str("{}").unwrap();
        let initial = legacy.observe_dna(450.0);
        assert_eq!(initial.brain_level, 2);
        assert!(initial.newly_unlocked_brain_levels.is_empty());

        let saved = serde_json::to_string(&legacy).unwrap();
        let mut restored: CreatureProgression = serde_json::from_str(&saved).unwrap();
        assert!(restored
            .observe_dna(450.0)
            .newly_unlocked_brain_levels
            .is_empty());
        assert_eq!(restored.observe_dna(600.0).newly_unlocked_brain_levels, [3]);
    }

    #[test]
    fn rank_and_unlock_events_never_regress_or_replay_after_spend_reload_or_nan() {
        let mut progress = CreatureProgression::default();
        assert!(progress
            .observe_dna(0.0)
            .newly_unlocked_brain_levels
            .is_empty());
        assert_eq!(
            progress.observe_dna(600.0).newly_unlocked_brain_levels,
            [1, 2, 3]
        );
        assert!(progress
            .observe_dna(100.0)
            .newly_unlocked_brain_levels
            .is_empty());
        assert_eq!(progress.brain_level, 3);
        assert_eq!(progress.observed_dna, 600.0);
        assert!(progress
            .observe_dna(600.0)
            .newly_unlocked_brain_levels
            .is_empty());

        let saved = serde_json::to_string(&progress).unwrap();
        let mut restored: CreatureProgression = serde_json::from_str(&saved).unwrap();
        assert!(restored
            .observe_dna(f32::NAN)
            .newly_unlocked_brain_levels
            .is_empty());
        assert_eq!(restored.brain_level, 3);
        assert_eq!(restored.observed_dna, 600.0);
        assert!(restored
            .observe_dna(600.0)
            .newly_unlocked_brain_levels
            .is_empty());
        assert_eq!(
            restored.observe_dna(1000.0).newly_unlocked_brain_levels,
            [4]
        );
    }

    #[test]
    fn pack_capacity_tracks_adapted_native_brain_ranks_and_is_bounded() {
        assert_eq!(pack_capacity(0.0), 0);
        assert_eq!(pack_capacity(99.9), 0);
        assert_eq!(pack_capacity(100.0), 1);
        assert_eq!(pack_capacity(299.9), 1);
        assert_eq!(pack_capacity(300.0), 2);
        assert_eq!(pack_capacity(599.9), 2);
        assert_eq!(pack_capacity(600.0), 3);
        assert_eq!(pack_capacity(999.9), 3);
        assert_eq!(pack_capacity(1000.0), 3);
        assert_eq!(pack_capacity(f32::NAN), 0);
        assert_eq!(pack_capacity(f32::INFINITY), 0);
    }

    #[test]
    fn migration_requires_next_rank_and_persists_destination_once() {
        let mut progress = CreatureProgression::default();
        assert_eq!(progress.next_migration_requirement(), Some(150.0));
        // The total is rounded to the nearest whole DNA (0xD41225 cvtss2si).
        assert_eq!(
            progress.begin_migration(149.4, [0.0, 0.0, 0.0], &[[1.0, 0.0, 0.0]]),
            Err(MigrationError::InsufficientDna { requirement: 150.0 })
        );
        assert_eq!(
            progress
                .begin_migration(
                    150.0,
                    [0.0, 0.0, 0.0],
                    &[[0.0, 500.0, 0.0], [3.0, 0.0, 0.0], [2.0, 100.0, 0.0]],
                )
                .unwrap(),
            [2.0, 100.0, 0.0]
        );
        assert_eq!(
            progress.begin_migration(200.0, [0.0; 3], &[[4.0, 0.0, 0.0]]),
            Err(MigrationError::AlreadyInProgress)
        );
        let saved = serde_json::to_string(&progress).unwrap();
        let mut restored: CreatureProgression = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.complete_migration([2.0, 0.0, 0.0], 1.0),
            Ok([2.0, 100.0, 0.0])
        );
        assert_eq!(restored.migrations_completed, 1);
        assert_eq!(restored.next_migration_requirement(), Some(650.0));
        assert_eq!(
            restored.complete_migration([2.0, 0.0, 0.0], 1.0),
            Err(MigrationError::NoMigrationInProgress)
        );
        // The second migration (stage total 650) is the last one.
        assert_eq!(
            restored.begin_migration(649.0, [2.0, 0.0, 0.0], &[[9.0, 0.0, 0.0]]),
            Err(MigrationError::InsufficientDna { requirement: 650.0 })
        );
        restored
            .begin_migration(650.0, [2.0, 0.0, 0.0], &[[9.0, 0.0, 0.0]])
            .unwrap();
        restored.complete_migration([9.0, 0.0, 0.0], 1.0).unwrap();
        assert_eq!(restored.migrations_completed, 2);
        assert_eq!(restored.next_migration_requirement(), None);
        assert_eq!(
            restored.begin_migration(5000.0, [9.0, 0.0, 0.0], &[[20.0, 0.0, 0.0]]),
            Err(MigrationError::AllMigrationsComplete)
        );
    }

    #[test]
    fn migration_follows_the_last_migration_dna_not_a_key() {
        let mut progress = CreatureProgression::default();
        assert!(!progress.migration_due(149.4));
        assert!(progress.migration_due(149.6), "the total is rounded");
        assert!(progress.migration_due(150.0));
        assert!(progress.can_migrate(300.0));
        progress.begin_migration(300.0, [0.0; 3], &[[5.0, 0.0, 0.0]]).unwrap();
        // The migration records the DNA it started at (flag 0x054FB6BF).
        assert_eq!(progress.last_migration_dna, 300.0);
        assert!(!progress.can_migrate(649.0), "pending, and no level between 300 and 649");
        progress.complete_migration([5.0, 0.0, 0.0], 1.0).unwrap();
        assert!(!progress.migration_due(649.0));
        assert!(progress.migration_due(650.0));
        // A total past both levels takes one migration; the second is skipped
        // (the last DNA already lies above it).
        let mut jump = CreatureProgression::default();
        jump.begin_migration(1000.0, [0.0; 3], &[[5.0, 0.0, 0.0]]).unwrap();
        jump.complete_migration([5.0, 0.0, 0.0], 1.0).unwrap();
        assert!(!jump.migration_due(5000.0));
        assert_eq!(
            jump.begin_migration(5000.0, [5.0, 0.0, 0.0], &[[9.0, 0.0, 0.0]]),
            Err(MigrationError::AllMigrationsComplete)
        );
        // A save from before the field exists: the completed count stands in.
        let old: CreatureProgression = serde_json::from_str("{\"migrations_completed\": 1}").unwrap();
        assert_eq!(old.last_migration_dna, 0.0);
        assert!(!old.migration_due(400.0), "the first migration is done");
        assert!(old.migration_due(650.0));
        // No destination: nothing is recorded, the next birth tries again.
        let mut lonely = CreatureProgression::default();
        assert_eq!(lonely.begin_migration(200.0, [0.0; 3], &[]), Err(MigrationError::NoDestination));
        assert_eq!(lonely.last_migration_dna, 0.0);
        assert!(lonely.migration_due(200.0));
        // The claim radius is the retail 50.
        assert_eq!(CLAIM_RADIUS, 50.0);
    }

    #[test]
    fn a_retired_nest_holds_no_level_and_is_never_chosen_again() {
        // cHerd +0x120 / +0x124: a new herd (0, 5) holds every level, the
        // nest the species leaves gets (5, -1).
        for level in 0..=5 {
            assert!(NestRange::DEFAULT.holds(level));
            assert!(!NestRange::RETIRED.holds(level));
        }
        assert!(!NestRange::DEFAULT.holds(6));
        let (a, b, c) = ([0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [-300.0, 0.0, 0.0]);
        let mut progress = CreatureProgression::default();
        assert_eq!(progress.nest_range(a, a), NestRange::RETIRED, "the nest being left");
        assert_eq!(progress.nest_range(b, a), NestRange::DEFAULT);
        assert_eq!(progress.begin_migration(150.0, a, &[b, c]), Ok(b));
        assert_eq!(progress.retired_nests, vec![a]);
        progress.complete_migration(b, 1.0).unwrap();
        // Second migration: the first home is nearer than c but retired.
        assert_eq!(progress.begin_migration(650.0, b, &[a, c, b]), Ok(c));
        assert_eq!(progress.retired_nests, vec![a, b]);
        // Old saves have no retired list.
        let old: CreatureProgression = serde_json::from_str("{\"migrations_completed\": 1}").unwrap();
        assert!(old.retired_nests.is_empty());
    }

    #[test]
    fn progress_helpers_match_the_retail_bar() {
        assert_eq!(dna_into_level(450.0), 150.0);
        assert!((level_progress(450.0) - 0.5).abs() < 1e-6);
        assert_eq!(level_progress(1000.0), 1.0);
        assert_eq!(BRAIN_LEVEL_DNA, [100.0, 200.0, 300.0, 400.0]);
        assert_eq!(BRAIN_LEVEL_TOTALS, [100.0, 300.0, 600.0, 1000.0]);
        assert_eq!(MIGRATION_DNA_LEVELS, [150.0, 650.0]);
    }

    #[test]
    fn migration_destination_and_arrival_use_xz_ground_distance() {
        let home = [0.0, 0.0, 0.0];
        assert_eq!(
            select_migration_destination(home, &[[0.0, 100.0, 0.0], [0.5, 500.0, 0.0]]),
            Some([0.5, 500.0, 0.0])
        );
        // A nest beyond the claim radius is preferred (the avatar travels to it).
        assert_eq!(
            select_migration_destination(home, &[[10.0, 0.0, 0.0], [80.0, 0.0, 0.0], [200.0, 0.0, 0.0]]),
            Some([80.0, 0.0, 0.0])
        );
        let mut progress = CreatureProgression::default();
        progress
            .begin_migration(150.0, home, &[[2.0, 100.0, 0.0]])
            .unwrap();
        assert_eq!(
            progress.complete_migration([2.0, -40.0, 0.0], 0.1),
            Ok([2.0, 100.0, 0.0])
        );
    }
}
