//! Creature-stage NPC ecology from the original's own data (notes C4).
//!
//! The retail stage populates the planet from `CreatureGameLevelGenerationList`
//! (`noun_placement_markers~`): each entry of its `speciesFiles` list is a herd
//! archetype of `herdtypes~` (0x02F98B67), and each archetype says how big the
//! herd is, how far from the player's nest it lives, how tough it is, how many
//! friends or kills finish the species, what it pays in DNA, and when it eats,
//! sleeps and drinks. This module holds those records as named constants (the
//! resolved props, `notes\creature-re\herdtypes_resolved.txt`, plus the three
//! "2" variants read from the install), the daily schedules
//! (`SchedulerCreatures`), the casting-manager tuning that activates herds near
//! the player, the day clock (`GlobalTuning`), and the pure rules the stage
//! systems call (`species.rs`, `creature.rs`, `creature_actions.rs`).
//!
//! VERIFIED (read in code, `0xB9AB60`, the placement routine fed with the
//! `speciesFiles` array): each archetype is placed with the probability
//! `PlacementPercentage` (times a per-difficulty factor of 1.0), with
//! `round(U[NumNests])` nests, each at a distance band of the archetype
//! (`OriginDistance`, then `optionalOriginDistances`). The archetype's
//! `HitpointOverride`, `DamageMultiplier` and DNA fields are used as written in
//! notes C1B/C2A/C2D. Since planet-re 4E step 4 the stage plays on the
//! original's planet (radius 500): the bands are retail chord distances on it
//! (`creature_planet::Land`), nests sit on land at their `WaterDistance` from
//! the sea, and the casting thresholds are the retail 100 / 150.
//!
//! The module is pure apart from the small Bevy surface at the end (the day
//! clock, the herd-member component and the casting state).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::species::Diet;

// ============================================================ world scale

/// The one factor between the original's planet-layout distances and the
/// rewrite's Creature world: every placement distance (archetype
/// `OriginDistance` bands, exclusive areas, territories, the casting
/// thresholds, the night ambush) is the original value times this. The
/// original planet is a sphere of radius 500 and the bands are chord distances
/// on it (planet-re-notes 1A/1E); the stage now plays on that planet, so the
/// factor is 1 (it was 0.1 on the flat island). Creature sizes, attack and
/// perception distances were always 1:1 (`creature_rules`).
pub const CREATURE_WORLD_SCALE: f32 = 1.0;
/// The home nest the bands are measured from (`creature_lifecycle` default).
pub const HOME: [f32; 2] = [0.0, 8.0];
/// No herd nest closer than this to the home nest, whatever the band says:
/// `avatarNestSafeZone` 15 (notes C1F; its use is not traced, ADAPTED as the
/// nest-free ring). The Carcass (10-15 m) is food, not a nest, and keeps its
/// band.
pub const HOME_CLEARANCE: f32 = 15.0;

/// ADAPTED: archetype `OverrideSize` to actor scale. NPC visuals are fitted to
/// a 2.5-unit footprint and the avatar to 1.9, so a size-1.5 creature (the
/// starter herds) is drawn as large as the level-0 avatar.
pub const SIZE_TO_SCALE: f32 = (1.9 / 2.5) / 1.5;
/// Safety bound of an actor scale, no longer a visible cap: the largest
/// archetype is the epic predator (`OverrideSize` 20 against the starter
/// herds' 1.5, 13 times their size), which now keeps its retail proportion
/// (20 x [`SIZE_TO_SCALE`] = 10.1; it was capped at 5.0, half its size, for the
/// flat island's camera and pathing; the stage plays on the radius-500 planet).
pub const MAX_ACTOR_SCALE: f32 = 12.0;
/// `creatureScale` (speciestuning): random per-creature scale range.
pub const CREATURE_SCALE_RANGE: (f32, f32) = crate::creature_rules::RANDOM_SCALE_RANGE;

// ============================================================ tuning

/// `CastingManager_DistanceThresholds` (CreatureGameTuning): herds activate
/// within the first distance and deactivate beyond the second (retail metres).
pub const CASTING_DISTANCE_THRESHOLDS: (f32, f32) = (100.0, 150.0);
/// `CastingManager_UpdateTime` (s).
pub const CASTING_UPDATE_TIME: f32 = 1.0;
/// `CastingManager_MaxHerdsActivatedPerInterval`.
pub const CASTING_MAX_HERDS_PER_INTERVAL: usize = 2;
/// `CastingManager_MaxAnimalPoolSize`: living NPC creatures at most.
pub const CASTING_MAX_ANIMAL_POOL: usize = 100;
/// `alphaCreaturePercentageOfHerd` (CreatureGameTuning): the share used when an
/// archetype leaves it at -1.
pub const DEFAULT_ALPHA_SHARE: f32 = 0.1;
/// `numExtraSocialsForAlphas` (CreatureGameTuning): one more social round.
pub const EXTRA_SOCIALS_FOR_ALPHAS: u8 = 1;
/// CreatureGameTuning 0xFB2020B9 (value 1, name not in the registries): a baby
/// asks for this many fewer social rounds (0xDA4360).
pub const FEWER_SOCIALS_FOR_BABIES: u8 = 1;
/// `alphaCreatureSkinPaintColorIndex` and `alphaCreatureHSVColorMultipliers`
/// (CreatureGameTuning, globals 0x1582F4C / 0x1583338..40, read in 0xC03750).
pub const ALPHA_SKIN_PAINT_COLOR_INDEX: usize = 2;
pub const ALPHA_HSV_COLOR_MULTIPLIERS: [f32; 3] = [0.0, 1.5, 1.0];
/// `creatureAlphaUnlockChance` / `creatureNonAlphaUnlockChance`: parts drop
/// from alphas only.
pub const ALPHA_UNLOCK_CHANCE: f32 = 1.0;
pub const NON_ALPHA_UNLOCK_CHANCE: f32 = 0.0;
/// `creatureGuardNestDistance` (tuning, name only).
pub const GUARD_NEST_DISTANCE: f32 = 8.0;
/// `maxNumEggs`, `egg_hatch_time`, `creatureNPCMinBabyTime` (tuning).
pub const MAX_NUM_EGGS: u32 = 2;
pub const EGG_HATCH_TIME: f32 = 180.0;
pub const NPC_MIN_BABY_TIME: f32 = 180.0;

/// `defaultDayLength` (GlobalTuning 00000000!39393837): seconds per day.
pub const DEFAULT_DAY_LENGTH: f32 = 960.0;
/// `timeOfDayMultiplierCreature` (GlobalTuning): one entry per hour 0..=24.
/// VERIFIED (solar-system update 0xC85030..0xC850FE, creature-audit-a): every
/// frame the local fraction of the day (0xBC2CD0) is scaled by (entries - 1),
/// the table is interpolated linearly between its floor and ceil entries and
/// the result goes to `cTimeOfDay::SetSpeedMultiplier` (0xBC2A30, the planet
/// turns `multiplier` times faster: hours per second = 24 x multiplier /
/// `DEFAULT_DAY_LENGTH`). Nothing normalises the day: with this table it lasts
/// 40 x integral(dh / multiplier) = 1092 s, not 960 s. Dawn (4) and dusk (20)
/// pass quickest, noon and midnight linger.
pub const TIME_OF_DAY_MULTIPLIER_CREATURE: [f32; 25] = [
    0.25, 0.75, 2.0, 3.0, 8.0, 6.0, 4.0, 2.0, 1.0, 0.75, 0.75, 0.25, 0.25, 0.25, 0.75, 0.75, 1.0,
    2.0, 4.0, 6.0, 8.0, 3.0, 2.0, 0.75, 0.25,
];
/// `creatureStartTimeOfDay` (CreatureGameTuning).
pub const START_TIME_OF_DAY: f32 = 7.0;
/// Night for the Tribal vignette filter (`tribal_vignettes`): from dusk to
/// dawn, the two fastest hours of the multiplier table. ADAPTED (the Tribal
/// vignette reader of `vi_time_of_day` was not traced); the Creature stage's
/// own night is the `NightPredators` event window below.
pub const DUSK_HOUR: f32 = 20.0;
pub const DAWN_HOUR: f32 = 4.0;
/// `creature_events~!NightPredators` (0x0EBCFBC7!0xB9E11B03, VERIFIED):
/// `ActivationType` 3 is a time-of-day event whose `ActivationThreshold`
/// (21.0, 3.0) is the window in hours (the event update 0xD4E450 scales both by
/// 3600 and compares them with the seconds of the day at the avatar, wrapping
/// past midnight; the event stops when the window ends).
pub const NIGHT_EVENT_START_HOUR: f32 = 21.0;
pub const NIGHT_EVENT_END_HOUR: f32 = 3.0;
/// `NightPredatorHerdDistance` (0x3551D2EA) of the same event prop, 75.0 (the
/// code default 50 is overridden by the prop; read in 0xD4F290 where the
/// night-predator herd is created): the herd appears this far from the avatar.
pub const NIGHT_PREDATOR_AMBUSH_DISTANCE: f32 = 75.0;

// ============================================================ schedules

/// A daily schedule of `SchedulerCreatures` (hours, start inclusive, end
/// exclusive; a start after the end wraps past midnight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Schedule {
    pub name: &'static str,
    pub eat: &'static [(f32, f32)],
    pub sleep: &'static [(f32, f32)],
    pub drink: &'static [(f32, f32)],
}

/// What a schedule has a herd do at an hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Eat,
    Sleep,
    Drink,
}

pub fn in_window(hour: f32, (start, end): (f32, f32)) -> bool {
    if start <= end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end
    }
}

impl Schedule {
    /// The activity at `hour` (sleep wins over eating and drinking when windows
    /// overlap; no window is idle).
    pub fn activity(&self, hour: f32) -> Activity {
        let hour = hour.rem_euclid(24.0);
        if self.sleep.iter().any(|w| in_window(hour, *w)) {
            Activity::Sleep
        } else if self.eat.iter().any(|w| in_window(hour, *w)) {
            Activity::Eat
        } else if self.drink.iter().any(|w| in_window(hour, *w)) {
            Activity::Drink
        } else {
            Activity::Idle
        }
    }
}

pub const DEFAULT_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "DefaultAnimalSchedule",
    eat: &[(14.0, 20.0)],
    sleep: &[(0.0, 7.0)],
    drink: &[(10.0, 15.0)],
};
pub const SLEEP_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "SleepAnimalSchedule",
    eat: &[(0.0, 0.0)],
    sleep: &[(22.0, 5.0)],
    drink: &[(0.0, 0.0)],
};
pub const HERB_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "HerbAnimalSchedule",
    eat: &[(4.0, 7.0), (12.0, 15.0), (20.0, 23.0)],
    sleep: &[(0.0, 2.0), (7.0, 9.0), (15.0, 17.0)],
    drink: &[(2.0, 3.0), (9.0, 11.0), (17.0, 19.0)],
};
pub const HERB1_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "Herb1AnimalSchedule",
    eat: &[(0.0, 3.0), (8.0, 11.0), (16.0, 19.0)],
    sleep: &[(3.0, 5.0), (11.0, 13.0), (19.0, 21.0)],
    drink: &[(5.0, 7.0), (13.0, 15.0), (21.0, 23.0)],
};
pub const HERB2_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "Herb2AnimalSchedule",
    eat: &[(2.0, 5.0), (10.0, 13.0), (18.0, 21.0)],
    sleep: &[(5.0, 7.0), (13.0, 15.0), (21.0, 23.0)],
    drink: &[(0.0, 1.0), (7.0, 9.0), (15.0, 17.0)],
};
/// The carnivore schedules are the herbivore ones under other names.
pub const CARN_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "CarnAnimalSchedule",
    ..HERB_ANIMAL_SCHEDULE
};
pub const CARN1_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "Carn1AnimalSchedule",
    ..HERB1_ANIMAL_SCHEDULE
};
pub const CARN2_ANIMAL_SCHEDULE: Schedule = Schedule {
    name: "Carn2AnimalSchedule",
    ..HERB2_ANIMAL_SCHEDULE
};
pub const MINI_BOSS_SCHEDULE: Schedule = Schedule {
    name: "mini_boss",
    ..HERB_ANIMAL_SCHEDULE
};
pub const MINI_BOSS1_SCHEDULE: Schedule = Schedule {
    name: "mini_boss1",
    ..HERB1_ANIMAL_SCHEDULE
};
/// Night predators eat around the clock.
pub const NIGHT_PREDATOR_SCHEDULE: Schedule = Schedule {
    name: "NightPredatorSchedule",
    eat: &[(0.0, 24.0)],
    sleep: &[],
    drink: &[],
};

const SOCIAL_SCHEDULES: &[&Schedule] = &[&SLEEP_ANIMAL_SCHEDULE];
const HERB_SCHEDULES: &[&Schedule] = &[
    &HERB_ANIMAL_SCHEDULE,
    &HERB1_ANIMAL_SCHEDULE,
    &HERB2_ANIMAL_SCHEDULE,
];
const CARN_SCHEDULES: &[&Schedule] = &[
    &CARN_ANIMAL_SCHEDULE,
    &CARN1_ANIMAL_SCHEDULE,
    &CARN2_ANIMAL_SCHEDULE,
];

// ============================================================ archetypes

/// The herd archetypes the Creature stage uses (`herdtypes~` instance names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Archetype {
    Carcass,
    Social,
    Social2,
    Combat,
    SmallSocial,
    SmallSocial2,
    SmallCombat,
    SmallCombat2,
    MediumSocial,
    MediumSocial2,
    MediumCombat,
    MediumCombat2,
    LargeSocial,
    LargeSocial2,
    LargeCombat,
    NightPredator,
    MiniBoss,
    EpicPredator,
}

/// One resolved herd archetype record (medium difficulty).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HerdArchetype {
    pub name: &'static str,
    pub food_chain: u8,
    pub num_nests: (f32, f32),
    pub num_creatures: (f32, f32),
    pub group_size: (f32, f32),
    /// `HitpointOverride`: the NPC's base HP (C2A).
    pub hit_points: f32,
    /// `DamageMultiplier`: every hit the NPC lands (C2D).
    pub damage_multiplier: f32,
    /// `OverrideSize`.
    pub size: f32,
    /// `RespawnRate` (s): a lost member is replaced after this long.
    pub respawn_rate: f32,
    pub impress_curious: u32,
    pub impress_friendly: u32,
    pub impress_best_friends: u32,
    /// `NumToKillForExtinction`; -1 never.
    pub kills_to_extinction: i32,
    pub kills_for_fearful: u32,
    /// `numSocialProposals`: social rounds.
    pub social_proposals: u8,
    pub social_power: f32,
    pub social_time_before_drain: f32,
    pub social_drain_rate: f32,
    pub social_decay: f32,
    /// `evoPointsRewarded` (kill), `evoPointsRewardedSocial` (befriend),
    /// `evoPointsSocialBonus` (C1B).
    pub evo_kill: f32,
    pub evo_social: f32,
    pub evo_social_bonus: f32,
    /// `extinctionReward` / `relationshipBestFriendsReward` (species finished).
    pub extinction_reward: f32,
    pub best_friends_reward: f32,
    /// `dnaPointsEvolutionThresholds`: the player's stage total at which the
    /// herd evolves (0 or 10000 never).
    pub evolution_thresholds: (f32, f32),
    pub part_unlock_level: u32,
    /// `alphaCreaturePercentageOfHerd` (-1 = the tuning default).
    pub alpha_share: f32,
    pub babies: (f32, f32),
    pub origin_distance: (f32, f32),
    pub optional_origin_distances: &'static [(f32, f32)],
    /// `WaterDistance` minimum (retail metres).
    pub water_distance: f32,
    pub altitude: (f32, f32),
    pub max_slope: f32,
    pub placement: f32,
    pub territory: f32,
    pub initial_relationship: u8,
    pub help_radius: f32,
    pub max_species_to_alert: u32,
    pub fight_group_size: u32,
    pub epic: bool,
    pub miniboss: bool,
    pub has_nest: bool,
    pub schedules: &'static [&'static Schedule],
    pub guard_share: f32,
    pub guard_radius: (f32, f32),
    pub guard_locations: u32,
    /// `guardsPerLocation` and `guardPositionDurationRange` (s): guard posts
    /// are `NumGuardLocations` x `guardsPerLocation` (`percentGuards` is never
    /// read by the code, notes C8A).
    pub guards_per_location: u32,
    pub guard_duration: (f32, f32),
    /// `exclusiveAreaRadiusGroup{Small,Medium,Large}` and their test flags.
    pub exclusive_radius: [f32; 3],
    pub exclusive_test: [bool; 3],
    pub social_group_size: u32,
    pub food_value: f32,
    /// The diet the `carnivore` / `herbivore` capability ranges ask for (flag
    /// 0 = constrained, 2 = free): social herds need a herbivore mouth, combat
    /// herds a carnivore one.
    pub diet: Diet,
}

/// The `default` record every archetype inherits from.
pub const DEFAULT_ARCHETYPE: HerdArchetype = HerdArchetype {
    name: "default",
    food_chain: 5,
    num_nests: (1.0, 1.0),
    num_creatures: (1.0, 2.0),
    group_size: (4.0, 6.0),
    hit_points: 0.0,
    damage_multiplier: 1.0,
    size: 0.0,
    respawn_rate: 60.0,
    impress_curious: 1,
    impress_friendly: 3,
    impress_best_friends: 6,
    kills_to_extinction: 6,
    kills_for_fearful: 3,
    social_proposals: 3,
    social_power: 50.0,
    social_time_before_drain: 4.0,
    social_drain_rate: 0.1,
    social_decay: 2.0,
    evo_kill: 1.0,
    evo_social: 1.0,
    evo_social_bonus: 10.0,
    extinction_reward: 20.0,
    best_friends_reward: 20.0,
    evolution_thresholds: (0.0, 0.0),
    part_unlock_level: 0,
    alpha_share: -1.0,
    babies: (0.2, 0.4),
    origin_distance: (5.0, 100.0),
    optional_origin_distances: &[],
    water_distance: 5.0,
    altitude: (0.0, 1000.0),
    max_slope: 60.0,
    placement: 1.0,
    territory: 0.0,
    initial_relationship: 3,
    help_radius: 10.0,
    max_species_to_alert: 0,
    fight_group_size: 6,
    epic: false,
    miniboss: false,
    has_nest: true,
    schedules: &[&DEFAULT_ANIMAL_SCHEDULE],
    guard_share: 0.3,
    guard_radius: (10.0, 15.0),
    guard_locations: 6,
    guards_per_location: 1,
    guard_duration: (60.0, 120.0),
    exclusive_radius: [5.0, 5.0, 5.0],
    exclusive_test: [true, true, false],
    social_group_size: 1,
    food_value: 100.0,
    diet: Diet::Omnivore,
};

pub const CARCASS: HerdArchetype = HerdArchetype {
    name: "carcass",
    food_chain: 2,
    num_creatures: (1.0, 1.0),
    hit_points: 1.0,
    size: 2.45,
    respawn_rate: 0.0,
    babies: (0.0, 0.0),
    origin_distance: (10.0, 15.0),
    altitude: (1.0, 10000.0),
    max_slope: 40.0,
    territory: 125.0,
    has_nest: false,
    exclusive_radius: [1.0, 2.0, 2.0],
    exclusive_test: [true, false, false],
    ..DEFAULT_ARCHETYPE
};

pub const NPC_SOCIAL: HerdArchetype = HerdArchetype {
    name: "npc_social",
    food_chain: 1,
    num_creatures: (4.0, 4.0),
    hit_points: 10.0,
    damage_multiplier: 0.75,
    size: 1.5,
    respawn_rate: 240.0,
    impress_friendly: 2,
    impress_best_friends: 3,
    kills_to_extinction: 3,
    kills_for_fearful: 1,
    social_proposals: 1,
    social_power: 50.0,
    social_time_before_drain: 5.0,
    social_decay: 0.0,
    evo_kill: 7.0,
    evo_social: 10.0,
    evo_social_bonus: 5.0,
    extinction_reward: 20.0,
    best_friends_reward: 25.0,
    evolution_thresholds: (50.0, 200.0),
    part_unlock_level: 1,
    alpha_share: 0.25,
    babies: (0.0, 0.0),
    origin_distance: (35.0, 100.0),
    water_distance: 20.0,
    altitude: (1.0, 10000.0),
    max_slope: 55.0,
    territory: 20.0,
    help_radius: 0.0,
    fight_group_size: 1,
    schedules: SOCIAL_SCHEDULES,
    guard_radius: (5.0, 10.0),
    guard_locations: 4,
    guard_duration: (10.0, 30.0),
    exclusive_radius: [1.0, 60.0, 90.0],
    diet: Diet::Herbivore,
    ..DEFAULT_ARCHETYPE
};
pub const NPC_SOCIAL2: HerdArchetype = HerdArchetype {
    name: "npc_social2",
    origin_distance: (90.0, 150.0),
    exclusive_radius: [1.0, 60.0, 60.0],
    ..NPC_SOCIAL
};
pub const NPC_COMBAT: HerdArchetype = HerdArchetype {
    name: "npc_combat",
    food_chain: 2,
    social_power: 25.0,
    social_time_before_drain: 3.0,
    extinction_reward: 25.0,
    best_friends_reward: 20.0,
    origin_distance: (100.0, 200.0),
    water_distance: 10.0,
    max_slope: 40.0,
    initial_relationship: 2,
    help_radius: 5.0,
    guard_locations: 3,
    exclusive_radius: [1.0, 75.0, 60.0],
    diet: Diet::Carnivore,
    ..NPC_SOCIAL
};
pub const NPC_SMALL_SOCIAL: HerdArchetype = HerdArchetype {
    name: "npc_small_social",
    food_chain: 3,
    num_creatures: (6.0, 6.0),
    group_size: (2.0, 3.0),
    hit_points: 15.0,
    damage_multiplier: 1.0,
    size: 2.45,
    respawn_rate: 240.0,
    impress_friendly: 2,
    impress_best_friends: 3,
    kills_to_extinction: 4,
    kills_for_fearful: 1,
    social_proposals: 2,
    social_power: 50.0,
    social_time_before_drain: 3.0,
    social_decay: 2.0,
    evo_kill: 8.0,
    evo_social: 12.0,
    evo_social_bonus: 6.0,
    extinction_reward: 25.0,
    best_friends_reward: 30.0,
    evolution_thresholds: (150.0, 450.0),
    part_unlock_level: 2,
    alpha_share: 0.333,
    babies: (0.0, 0.167),
    origin_distance: (40.0, 120.0),
    water_distance: 20.0,
    altitude: (1.0, 10000.0),
    max_slope: 40.0,
    placement: 0.43,
    territory: 100.0,
    help_radius: 15.0,
    fight_group_size: 1,
    schedules: HERB_SCHEDULES,
    guard_radius: (10.0, 15.0),
    guard_locations: 4,
    guard_duration: (10.0, 45.0),
    exclusive_radius: [1.0, 75.0, 60.0],
    food_value: 130.0,
    diet: Diet::Herbivore,
    ..DEFAULT_ARCHETYPE
};
pub const NPC_SMALL_SOCIAL2: HerdArchetype = HerdArchetype {
    name: "npc_small_social2",
    origin_distance: (125.0, 325.0),
    water_distance: 50.0,
    placement: 1.0,
    exclusive_radius: [1.0, 60.0, 60.0],
    ..NPC_SMALL_SOCIAL
};
pub const NPC_SMALL_COMBAT: HerdArchetype = HerdArchetype {
    name: "npc_small_combat",
    food_chain: 4,
    kills_for_fearful: 2,
    social_time_before_drain: 1.0,
    extinction_reward: 30.0,
    best_friends_reward: 25.0,
    alpha_share: 0.167,
    origin_distance: (10.0, 20.0),
    max_slope: 55.0,
    placement: 0.29,
    initial_relationship: 2,
    max_species_to_alert: 1,
    guard_duration: (10.0, 30.0),
    fight_group_size: 2,
    schedules: CARN_SCHEDULES,
    exclusive_radius: [1.0, 65.0, 60.0],
    diet: Diet::Carnivore,
    ..NPC_SMALL_SOCIAL
};
pub const NPC_SMALL_COMBAT2: HerdArchetype = HerdArchetype {
    name: "npc_small_combat2",
    origin_distance: (150.0, 350.0),
    water_distance: 60.0,
    placement: 1.0,
    exclusive_radius: [1.0, 60.0, 60.0],
    ..NPC_SMALL_COMBAT
};
pub const NPC_MEDIUM_SOCIAL: HerdArchetype = HerdArchetype {
    name: "npc_medium_social",
    food_chain: 5,
    num_creatures: (8.0, 8.0),
    group_size: (2.0, 4.0),
    hit_points: 25.0,
    damage_multiplier: 1.0,
    size: 3.45,
    respawn_rate: 240.0,
    impress_friendly: 2,
    impress_best_friends: 3,
    kills_to_extinction: 5,
    kills_for_fearful: 1,
    social_proposals: 3,
    social_power: 200.0,
    social_time_before_drain: 0.5,
    social_decay: 0.0,
    evo_kill: 9.0,
    evo_social: 14.0,
    evo_social_bonus: 7.0,
    extinction_reward: 30.0,
    best_friends_reward: 40.0,
    evolution_thresholds: (400.0, 650.0),
    part_unlock_level: 3,
    alpha_share: 0.375,
    babies: (0.0, 0.125),
    origin_distance: (40.0, 120.0),
    water_distance: 20.0,
    altitude: (0.0, 1000.0),
    max_slope: 60.0,
    placement: 0.42,
    territory: 200.0,
    help_radius: 25.0,
    max_species_to_alert: 3,
    fight_group_size: 3,
    schedules: HERB_SCHEDULES,
    guard_radius: (15.0, 20.0),
    guard_locations: 4,
    guards_per_location: 2,
    guard_duration: (10.0, 45.0),
    exclusive_radius: [1.0, 70.0, 60.0],
    social_group_size: 3,
    food_value: 160.0,
    diet: Diet::Herbivore,
    ..DEFAULT_ARCHETYPE
};
pub const NPC_MEDIUM_SOCIAL2: HerdArchetype = HerdArchetype {
    name: "npc_medium_social2",
    origin_distance: (125.0, 325.0),
    placement: 1.0,
    exclusive_radius: [1.0, 60.0, 60.0],
    ..NPC_MEDIUM_SOCIAL
};
pub const NPC_MEDIUM_COMBAT: HerdArchetype = HerdArchetype {
    name: "npc_medium_combat",
    food_chain: 6,
    kills_for_fearful: 3,
    social_power: 300.0,
    extinction_reward: 40.0,
    best_friends_reward: 30.0,
    origin_distance: (10.0, 20.0),
    water_distance: 30.0,
    altitude: (1.0, 10000.0),
    max_slope: 55.0,
    placement: 0.9,
    initial_relationship: 5,
    schedules: CARN_SCHEDULES,
    guard_radius: (20.0, 25.0),
    guard_duration: (10.0, 30.0),
    exclusive_radius: [1.0, 75.0, 40.0],
    diet: Diet::Carnivore,
    ..NPC_MEDIUM_SOCIAL
};
pub const NPC_MEDIUM_COMBAT2: HerdArchetype = HerdArchetype {
    name: "npc_medium_combat2",
    origin_distance: (175.0, 325.0),
    placement: 1.0,
    ..NPC_MEDIUM_COMBAT
};
pub const NPC_LARGE_SOCIAL: HerdArchetype = HerdArchetype {
    name: "npc_large_social",
    food_chain: 7,
    num_creatures: (10.0, 10.0),
    group_size: (3.0, 5.0),
    hit_points: 40.0,
    size: 4.7,
    kills_to_extinction: 7,
    social_power: 600.0,
    evo_kill: 10.0,
    evo_social: 16.0,
    evo_social_bonus: 8.0,
    extinction_reward: 40.0,
    best_friends_reward: 70.0,
    evolution_thresholds: (0.0, 0.0),
    part_unlock_level: 4,
    alpha_share: 0.4,
    babies: (0.0, 0.2),
    origin_distance: (150.0, 250.0),
    placement: 0.65,
    help_radius: 30.0,
    max_species_to_alert: 4,
    guard_radius: (20.0, 25.0),
    exclusive_radius: [1.0, 75.0, 40.0],
    social_group_size: 4,
    food_value: 190.0,
    ..NPC_MEDIUM_SOCIAL
};
pub const NPC_LARGE_SOCIAL2: HerdArchetype = HerdArchetype {
    name: "npc_large_social2",
    origin_distance: (150.0, 350.0),
    placement: 1.0,
    ..NPC_LARGE_SOCIAL
};
pub const NPC_LARGE_COMBAT: HerdArchetype = HerdArchetype {
    name: "npc_large_combat",
    food_chain: 8,
    kills_for_fearful: 5,
    social_power: 800.0,
    extinction_reward: 70.0,
    best_friends_reward: 40.0,
    alpha_share: 0.1,
    babies: (0.0, 0.1),
    origin_distance: (50.0, 120.0),
    water_distance: 40.0,
    altitude: (1.0, 10000.0),
    max_slope: 55.0,
    placement: 1.0,
    initial_relationship: 5,
    schedules: CARN_SCHEDULES,
    guard_radius: (25.0, 30.0),
    guard_duration: (10.0, 30.0),
    exclusive_radius: [1.0, 90.0, 90.0],
    diet: Diet::Carnivore,
    ..NPC_LARGE_SOCIAL
};
pub const NPC_NIGHT_PREDATOR: HerdArchetype = HerdArchetype {
    name: "npc_night_predator",
    food_chain: 6,
    num_creatures: (3.0, 3.0),
    group_size: (1.0, 1.0),
    hit_points: 15.0,
    size: 2.5,
    respawn_rate: 100_000.0,
    impress_friendly: 2,
    impress_best_friends: 3,
    kills_to_extinction: 3,
    kills_for_fearful: 2,
    social_proposals: 2,
    social_power: 25.0,
    social_time_before_drain: 0.5,
    social_decay: 0.0,
    evo_kill: 8.0,
    evo_social: 12.0,
    evo_social_bonus: 6.0,
    extinction_reward: 35.0,
    best_friends_reward: 25.0,
    evolution_thresholds: (10_000.0, 10_000.0),
    part_unlock_level: 2,
    babies: (0.0, 0.0),
    origin_distance: (35.0, 50.0),
    water_distance: 10.0,
    altitude: (1.0, 10000.0),
    max_slope: 40.0,
    territory: 100.0,
    initial_relationship: 5,
    help_radius: 25.0,
    max_species_to_alert: 3,
    fight_group_size: 2,
    schedules: &[&NIGHT_PREDATOR_SCHEDULE],
    exclusive_radius: [10.0, 10.0, 10.0],
    social_group_size: 2,
    food_value: 130.0,
    diet: Diet::Carnivore,
    ..DEFAULT_ARCHETYPE
};
pub const NPC_MINIBOSS: HerdArchetype = HerdArchetype {
    name: "npc_miniboss",
    food_chain: 9,
    num_creatures: (1.0, 1.0),
    group_size: (1.0, 1.0),
    hit_points: 250.0,
    damage_multiplier: 2.0,
    size: 5.7,
    respawn_rate: 300.0,
    impress_friendly: 1,
    impress_best_friends: 1,
    kills_to_extinction: 1,
    kills_for_fearful: 1,
    social_proposals: 4,
    social_power: 800.0,
    social_time_before_drain: 0.1,
    social_decay: 0.0,
    evo_kill: 0.0,
    evo_social: 0.0,
    evo_social_bonus: 0.0,
    extinction_reward: 100.0,
    best_friends_reward: 100.0,
    evolution_thresholds: (10_000.0, 10_000.0),
    part_unlock_level: 4,
    alpha_share: 0.0,
    origin_distance: (20.0, 200.0),
    water_distance: 25.0,
    altitude: (1.0, 10000.0),
    max_slope: 55.0,
    placement: 0.13,
    territory: 200.0,
    miniboss: true,
    has_nest: false,
    schedules: &[&MINI_BOSS_SCHEDULE, &MINI_BOSS1_SCHEDULE],
    exclusive_radius: [1.0, 40.0, 40.0],
    food_value: 250.0,
    diet: Diet::Carnivore,
    ..DEFAULT_ARCHETYPE
};
pub const NPC_EPIC_PREDATOR: HerdArchetype = HerdArchetype {
    name: "npc_epic_predator",
    food_chain: 10,
    num_creatures: (1.0, 1.0),
    hit_points: 1000.0,
    damage_multiplier: 100.0,
    size: 20.0,
    respawn_rate: 600.0,
    kills_to_extinction: -1,
    social_power: 5000.0,
    social_decay: 100.0,
    evo_kill: 100.0,
    evo_social: 100.0,
    evo_social_bonus: 50.0,
    part_unlock_level: 4,
    babies: (0.0, 0.0),
    origin_distance: (50.0, 80.0),
    optional_origin_distances: &[(120.0, 300.0), (120.0, 300.0)],
    water_distance: 25.0,
    altitude: (0.0, 20.0),
    max_slope: 35.0,
    placement: 0.08,
    territory: 150.0,
    initial_relationship: 5,
    epic: true,
    has_nest: false,
    schedules: CARN_SCHEDULES,
    exclusive_radius: [1.0, 50.0, 75.0],
    exclusive_test: [true, true, true],
    food_value: 300.0,
    diet: Diet::Carnivore,
    ..DEFAULT_ARCHETYPE
};

impl Archetype {
    pub const ALL: [Archetype; 18] = [
        Archetype::Carcass,
        Archetype::Social,
        Archetype::Social2,
        Archetype::Combat,
        Archetype::SmallSocial,
        Archetype::SmallSocial2,
        Archetype::SmallCombat,
        Archetype::SmallCombat2,
        Archetype::MediumSocial,
        Archetype::MediumSocial2,
        Archetype::MediumCombat,
        Archetype::MediumCombat2,
        Archetype::LargeSocial,
        Archetype::LargeSocial2,
        Archetype::LargeCombat,
        Archetype::NightPredator,
        Archetype::MiniBoss,
        Archetype::EpicPredator,
    ];

    /// The resolved record.
    pub fn record(self) -> &'static HerdArchetype {
        match self {
            Archetype::Carcass => &CARCASS,
            Archetype::Social => &NPC_SOCIAL,
            Archetype::Social2 => &NPC_SOCIAL2,
            Archetype::Combat => &NPC_COMBAT,
            Archetype::SmallSocial => &NPC_SMALL_SOCIAL,
            Archetype::SmallSocial2 => &NPC_SMALL_SOCIAL2,
            Archetype::SmallCombat => &NPC_SMALL_COMBAT,
            Archetype::SmallCombat2 => &NPC_SMALL_COMBAT2,
            Archetype::MediumSocial => &NPC_MEDIUM_SOCIAL,
            Archetype::MediumSocial2 => &NPC_MEDIUM_SOCIAL2,
            Archetype::MediumCombat => &NPC_MEDIUM_COMBAT,
            Archetype::MediumCombat2 => &NPC_MEDIUM_COMBAT2,
            Archetype::LargeSocial => &NPC_LARGE_SOCIAL,
            Archetype::LargeSocial2 => &NPC_LARGE_SOCIAL2,
            Archetype::LargeCombat => &NPC_LARGE_COMBAT,
            Archetype::NightPredator => &NPC_NIGHT_PREDATOR,
            Archetype::MiniBoss => &NPC_MINIBOSS,
            Archetype::EpicPredator => &NPC_EPIC_PREDATOR,
        }
    }

    /// FNV-1 instance id of the archetype's `herdtypes~` prop.
    pub fn instance_id(self) -> u32 {
        crate::world_objects::name_id(self.record().name)
    }

    /// The herd attacks the player on sight at the start: its
    /// `initialAvatarRelationship` is 2 (hated) or 5 (hostile) (VERIFIED
    /// 0xC0B9C0, `creature_rules::attacks_on_sight`). The relationship moves
    /// with the player's befriends and kills (`creature_actions::
    /// species_relationship_of`); this is its value before either.
    pub fn hostile(self) -> bool {
        crate::creature_rules::attacks_on_sight(self.record().initial_relationship)
    }

    /// A herd of creatures (not the carcass food item, not a roamer).
    pub fn is_herd(self) -> bool {
        !matches!(
            self,
            Archetype::Carcass | Archetype::MiniBoss | Archetype::EpicPredator
        )
    }

    /// ADAPTED: which unlock tier a species drawn for the archetype builds
    /// from (the archetypes ask for total social or attack ranks 2-8, 5-16 and
    /// 11-20; `species::MAX_UNLOCK_TIER` is 3).
    pub fn unlock_tier(self) -> u32 {
        match self.record().num_creatures.1 as u32 {
            0..=4 => 1,
            5..=6 => 2,
            _ => 3,
        }
    }
}

/// `CreatureGameLevelGenerationList` `speciesFiles` (PatchData 0302A1C9!2BEB12F4),
/// in file order: 22 placements.
pub const SPECIES_FILES: [Archetype; 22] = [
    Archetype::Carcass,
    Archetype::Social,
    Archetype::Social,
    Archetype::Social,
    Archetype::Social,
    Archetype::Social,
    Archetype::Social2,
    Archetype::Social2,
    Archetype::Combat,
    Archetype::Combat,
    Archetype::Combat,
    Archetype::SmallSocial2,
    Archetype::SmallSocial2,
    Archetype::SmallSocial2,
    Archetype::SmallSocial2,
    Archetype::SmallCombat2,
    Archetype::SmallCombat2,
    Archetype::SmallCombat2,
    Archetype::MediumSocial2,
    Archetype::MediumSocial2,
    Archetype::MediumCombat2,
    Archetype::LargeSocial2,
];

/// The generation list's four key sets (0x253B3636, 0x7920D9E6, 0x7C8428BC,
/// 0xE110AFF5) are its `forestSpeciesRules` (0x73CC6DF5 lists exactly these
/// keys). VERIFIED (level generation 0xB9CBF0 -> 0xB9C980): for every forest
/// of the planet (the marked fruit groves, planet +0x1E5F0, records of 0x19A8
/// bytes) within 200 m (prop 0xF64F11BE, default 200^2 compared squared) of
/// the start and on the start's land region (0xB886E0), the generator places
/// the `forestNouns` there and runs the herd placement (0xB9AB60) once per
/// set with the forest as the origin: each archetype rolls its
/// `PlacementPercentage` and its `OriginDistance` band is measured from the
/// forest. This is where the epic predator (0.08), the miniboss (0.13) and
/// the extra small / medium / large herds come from. Not placed yet: the
/// species world is generated before the planet's groves are known.
pub const EXTRA_SETS: [&[Archetype]; 4] = [
    &[Archetype::MediumCombat, Archetype::MediumSocial],
    &[Archetype::LargeSocial, Archetype::LargeCombat],
    &[
        Archetype::EpicPredator,
        Archetype::MiniBoss,
        Archetype::SmallCombat,
    ],
    &[Archetype::SmallSocial],
];

// ============================================================ hashing

/// splitmix64 finalizer (same as `species::mix64`): every decision is a pure
/// function of the world seed.
pub fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Uniform value in `0..1` from a key.
pub fn unit(key: u64) -> f32 {
    (mix64(key) >> 40) as f32 / (1u64 << 24) as f32
}

fn lerp_range((lo, hi): (f32, f32), t: f32) -> f32 {
    lo + (hi - lo) * t
}

/// `round(U[lo, hi])` the way the placement routine converts its ranges
/// (`cvtss2si`, round to nearest).
pub fn rounded_range(range: (f32, f32), key: u64) -> u32 {
    lerp_range(range, unit(key)).round().max(0.0) as u32
}

// ============================================================ population plan

/// One herd the generator decided to place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HerdPlan {
    pub archetype: Archetype,
    /// Index into the generation list (`SPECIES_FILES`, then the extra sets).
    pub slot: usize,
}

/// Which archetypes a world with `seed` places, in order (VERIFIED loop of
/// 0xB9AB60 over `speciesFiles`: a placement roll against
/// `PlacementPercentage`, 1.0 for every entry of the list).
pub fn plan_population(seed: u64) -> Vec<HerdPlan> {
    let mut plan = Vec::new();
    for (slot, archetype) in SPECIES_FILES.iter().enumerate() {
        if unit(seed ^ 0x9A1C_0000 ^ slot as u64) < archetype.record().placement {
            plan.push(HerdPlan {
                archetype: *archetype,
                slot,
            });
        }
    }
    plan
}

// ============================================================ placement

/// Herd size class (up to 4 creatures small, up to 8 medium, more large).
/// No longer used by the exclusive areas: retail has no per-herd class there
/// (see `exclusive_clearance`); kept for callers that sort herds by size.
pub fn group_class(archetype: Archetype) -> usize {
    match archetype.record().num_creatures.1 as u32 {
        0..=4 => 0,
        5..=8 => 1,
        _ => 2,
    }
}

/// A placed herd (or carcass, or roamer lair) in the stage chart (world x, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub archetype: Archetype,
    pub position: [f32; 2],
}

/// Distance band of nest `nest` in world units (0xB9AEB6: the bands are
/// `OriginDistance` followed by `optionalOriginDistances`, used in turn).
pub fn band(archetype: Archetype, nest: usize) -> (f32, f32) {
    let record = archetype.record();
    let count = 1 + record.optional_origin_distances.len();
    let (lo, hi) = match nest % count {
        0 => record.origin_distance,
        i => record.optional_origin_distances[i - 1],
    };
    (lo * CREATURE_WORLD_SCALE, hi * CREATURE_WORLD_SCALE)
}

#[cfg(test)]
fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Exclusive-area clearance a new herd of `archetype` needs from an `existing`
/// one (world units). VERIFIED (0xB90FF0, 0xB9AB60 / 0xB97870): every placed
/// herd marks the planet grid cells within its three radii
/// (`exclusiveAreaRadiusGroupSmall/Medium/Large`) with flags 0x10 / 0x20 /
/// 0x40; a herd placed later avoids the cells carrying a flag it tests (its
/// three test properties 0x80CD38C6 / 0x3A509D24 / 0x835025DA, defaults true /
/// true in the Creature game / false). So the existing herd's radii and the
/// new herd's flags decide; there is no size class of either herd.
pub fn exclusive_clearance(archetype: Archetype, existing: Archetype) -> f32 {
    let test = archetype.record().exclusive_test;
    let radius = existing.record().exclusive_radius;
    (0..3)
        .filter(|&k| test[k])
        .map(|k| radius[k])
        .fold(0.0, f32::max)
        * CREATURE_WORLD_SCALE
}

/// Place one nest of `archetype` in its band: at the band's chord distance
/// from the home nest, on land at least `WaterDistance` from the sea, and
/// outside the exclusive areas of `placed`. Deterministic; if no candidate
/// satisfies everything the exclusive areas relax (half, then none) and
/// finally (ADAPTED) the band widens and the water distance is dropped, so a
/// herd is never lost to the coast.
pub fn place_nest(
    seed: u64,
    key: u64,
    archetype: Archetype,
    nest: usize,
    home: [f32; 2],
    placed: &[Placement],
    land: &crate::creature_planet::Land,
) -> [f32; 2] {
    place_in_band(seed, key, archetype, band(archetype, nest), home, placed, land)
}

/// `place_nest` with an explicit band (world units).
pub fn place_in_band(
    seed: u64,
    key: u64,
    archetype: Archetype,
    (lo, hi): (f32, f32),
    home: [f32; 2],
    placed: &[Placement],
    land: &crate::creature_planet::Land,
) -> [f32; 2] {
    let _ = seed;
    let water = archetype.record().water_distance * CREATURE_WORLD_SCALE;
    let clear = if archetype == Archetype::Carcass { 0.0 } else { HOME_CLEARANCE };
    let origin = Vec2::new(home[0], home[1]);
    const ATTEMPTS: u64 = 48;
    let mut best: Option<([f32; 2], f32)> = None;
    for relax in 0..4u32 {
        let exclusive_scale = match relax {
            0 => 1.0,
            1 => 0.5,
            _ => 0.0,
        };
        let (lo, hi, water) = if relax < 3 {
            (lo.max(clear), hi.max(clear + 1.0), water)
        } else {
            ((lo * 0.5).max(clear), hi.max(clear + 1.0) * 1.5, 0.0)
        };
        for attempt in 0..ATTEMPTS {
            let k = key ^ ((relax as u64) << 40) ^ (attempt << 20);
            let angle = unit(k ^ 0xA9) * std::f32::consts::TAU;
            let r = lerp_range((lo, hi), unit(k ^ 0x5D));
            let at = land.toward(origin, r, angle);
            if !land.is_land(at) || land.water_within(at, water) {
                continue;
            }
            let p = [at.x, at.y];
            // Smallest margin over the exclusive areas of the herds placed so far.
            let margin = placed
                .iter()
                .map(|other| {
                    let need = exclusive_clearance(archetype, other.archetype) * exclusive_scale;
                    land.chord(at, Vec2::new(other.position[0], other.position[1])) - need.max(1.0)
                })
                .fold(f32::INFINITY, f32::min);
            if margin >= 0.0 {
                return p;
            }
            if best.is_none_or(|(_, m)| margin > m) {
                best = Some((p, margin));
            }
        }
    }
    best.map(|(p, _)| p).unwrap_or([home[0], home[1] - clear])
}

/// Place every planned herd (in plan order, so earlier herds claim their
/// space first, as the retail list does).
pub fn place_population(
    seed: u64,
    plan: &[HerdPlan],
    home: [f32; 2],
    land: &crate::creature_planet::Land,
) -> Vec<Placement> {
    let mut placed: Vec<Placement> = Vec::with_capacity(plan.len());
    for herd in plan {
        let position = place_nest(
            seed,
            seed ^ 0x9E57_0000_0000 ^ ((herd.slot as u64) << 8),
            herd.archetype,
            0,
            home,
            &placed,
            land,
        );
        placed.push(Placement {
            archetype: herd.archetype,
            position,
        });
    }
    placed
}

// ============================================================ forests

/// `forestSpeciesRules` in file order (0x73CC6DF5 = [0x7C8428BC, 0x253B3636,
/// 0xE110AFF5, 0x7920D9E6]): the sets placed at every nearby forest.
pub const FOREST_SPECIES_RULES: [&[Archetype]; 4] = [EXTRA_SETS[2], EXTRA_SETS[0], EXTRA_SETS[3], EXTRA_SETS[1]];
/// Prop 0xF64F11BE default (compared squared, 0x471C4000 = 40000): forests
/// within 200 m of the start get the forest sets (0xB9CBF0).
pub const FOREST_RADIUS: f32 = 200.0;
/// First generation-list slot of the forest placements (keys only).
pub const FOREST_SLOT_BASE: usize = 64;

/// A herd (or roamer) placed at a forest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestHerd {
    pub archetype: Archetype,
    pub slot: usize,
    pub position: [f32; 2],
    pub forest: usize,
}

/// Place a nest of `archetype` in band `(lo, hi)` around `origin`, or fail
/// (VERIFIED 0xB9AB60: a herd whose spot search 0xB97870 fails is not placed;
/// no relaxing). On land, `WaterDistance` from the sea, outside the marked
/// exclusive areas of `placed` and `HOME_CLEARANCE` from the home nest.
/// ADAPTED: 48 candidate spots (the search grid of 0xB97870 is not decoded).
pub fn try_place_in_band(
    key: u64,
    archetype: Archetype,
    (lo, hi): (f32, f32),
    origin: [f32; 2],
    placed: &[Placement],
    land: &crate::creature_planet::Land,
) -> Option<[f32; 2]> {
    let water = archetype.record().water_distance * CREATURE_WORLD_SCALE;
    let origin = Vec2::from(origin);
    let home = Vec2::from(HOME);
    for attempt in 0..48u64 {
        let k = key ^ (attempt << 20);
        let angle = unit(k ^ 0xA9) * std::f32::consts::TAU;
        let r = lerp_range((lo, hi), unit(k ^ 0x5D));
        let at = land.toward(origin, r, angle);
        if !land.is_land(at) || land.water_within(at, water) || land.chord(at, home) < HOME_CLEARANCE {
            continue;
        }
        let clear = placed.iter().all(|other| {
            land.chord(at, Vec2::from(other.position)) >= exclusive_clearance(archetype, other.archetype).max(1.0)
        });
        if clear {
            return Some([at.x, at.y]);
        }
    }
    None
}

/// The forest sets (VERIFIED 0xB9C980): for every forest (in list order),
/// every set of `FOREST_SPECIES_RULES`, every archetype of the set: its
/// `PlacementPercentage` roll, then `round(U[NumNests])` nests at its bands
/// measured from the forest. `placed` gains every herd placed, so later herds
/// keep out of earlier ones' exclusive areas.
pub fn place_forest_population(
    seed: u64,
    forests: &[[f32; 2]],
    placed: &mut Vec<Placement>,
    land: &crate::creature_planet::Land,
) -> Vec<ForestHerd> {
    let mut out = Vec::new();
    for (fi, forest) in forests.iter().enumerate() {
        for (ri, set) in FOREST_SPECIES_RULES.iter().enumerate() {
            for (ai, archetype) in set.iter().enumerate() {
                let slot = FOREST_SLOT_BASE + fi * 16 + ri * 4 + ai;
                let key = seed ^ 0xF0E5_0000_0000 ^ ((slot as u64) << 8);
                let record = archetype.record();
                if unit(key ^ 0x9A1C) >= record.placement {
                    continue;
                }
                let nests = rounded_range(record.num_nests, key ^ 0x4E57).max(1) as usize;
                for nest in 0..nests {
                    let (lo, hi) = band(*archetype, nest);
                    let Some(position) =
                        try_place_in_band(key ^ ((nest as u64) << 40), *archetype, (lo, hi), *forest, placed, land)
                    else {
                        continue;
                    };
                    placed.push(Placement { archetype: *archetype, position });
                    out.push(ForestHerd { archetype: *archetype, slot, position, forest: fi });
                }
            }
        }
    }
    out
}

// ============================================================ members

/// What a herd member is (C2A/C1B/C3: babies x0.25 HP/DNA/damage and scale
/// 0.6; alphas x1.25 HP/DNA, scale 1.1, one extra social round, part drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemberRole {
    #[default]
    Adult,
    Alpha,
    Baby,
}

impl MemberRole {
    pub fn is_alpha(self) -> bool {
        self == MemberRole::Alpha
    }
    pub fn is_baby(self) -> bool {
        self == MemberRole::Baby
    }
    /// The card prefix the retail HUD uses ("Alpha ~local_creature_name~").
    pub fn prefix(self) -> &'static str {
        match self {
            MemberRole::Adult => "",
            MemberRole::Alpha => "Alpha ",
            MemberRole::Baby => "Baby ",
        }
    }
}

/// Alphas in a herd of `count` (0xC09B40): `ceil(count * share)` in f32.
/// INFERRED: the epic and the miniboss have none (0xC09B40 skips species with
/// the flag at profile +0x588 set; its meaning is not traced, and a lone
/// giant would otherwise always be an alpha by the 0.1 default share).
pub fn alpha_count(archetype: Archetype, count: u32) -> u32 {
    let record = archetype.record();
    if record.epic || record.miniboss {
        return 0;
    }
    let wanted = count as f32 * alpha_share(archetype);
    let rounded = wanted.round();
    (if rounded < wanted { rounded + 1.0 } else { rounded }).max(0.0) as u32
}

/// Members a nest of `archetype` holds: `round(U[NumCreatures])`.
pub fn herd_size(archetype: Archetype, key: u64) -> u32 {
    rounded_range(archetype.record().num_creatures, key).max(1)
}

/// The alpha share of an archetype (-1 reads the tuning default).
pub fn alpha_share(archetype: Archetype) -> f32 {
    let share = archetype.record().alpha_share;
    if share < 0.0 {
        DEFAULT_ALPHA_SHARE
    } else {
        share
    }
}

/// Roles of a herd of `count`: `ceil(share * count)` alphas first (VERIFIED,
/// cCreatureAnimal::Create 0xC09B40: a new herd creature is an alpha while the
/// species has fewer than `ceil(herd size x share)` alphas, the share from
/// 0xC03530 = `alphaCreaturePercentageOfHerd` or the tuning default; the
/// product is taken in f32, so 0.167 of 6 gives 2), then
/// `round(U[percentBabiesRange] * count)` babies at the end, adults between,
/// and up to `NumGuardLocations` x `guardsPerLocation` adult guards.
pub fn member_roles(archetype: Archetype, count: u32, key: u64) -> Vec<(MemberRole, bool)> {
    let record = archetype.record();
    let n = count as usize;
    let alphas = (alpha_count(archetype, count) as usize).min(n);
    let babies = ((lerp_range(record.babies, unit(key ^ 0xBAB1)) * count as f32).round() as usize)
        .min(n - alphas);
    let mut roles: Vec<(MemberRole, bool)> = (0..n)
        .map(|i| {
            let role = if i < alphas {
                MemberRole::Alpha
            } else if i >= n - babies {
                MemberRole::Baby
            } else {
                MemberRole::Adult
            };
            (role, false)
        })
        .collect();
    // Guard posts: `NumGuardLocations` x `guardsPerLocation`, held by adults.
    // ADAPTED: which members guard is not traced (the last adults here).
    let guards = (record.guard_locations * record.guards_per_location) as usize;
    let mut assigned = 0;
    for slot in roles.iter_mut().rev() {
        if assigned >= guards {
            break;
        }
        if slot.0 == MemberRole::Adult {
            slot.1 = true;
            assigned += 1;
        }
    }
    roles
}

/// Base actor scale of an archetype's species (`OverrideSize`, capped).
pub fn species_scale(archetype: Archetype) -> f32 {
    (archetype.record().size * SIZE_TO_SCALE).clamp(0.2, MAX_ACTOR_SCALE)
}

/// A member's actor scale: the species scale, the random `creatureScale`
/// (0.8..1.1) and the role's scale (`creatureBabyScale` 0.6,
/// `alphaCreatureScale` 1.1), capped.
pub fn member_scale(base: f32, role: MemberRole, roll: f32) -> f32 {
    let random = lerp_range(CREATURE_SCALE_RANGE, roll.clamp(0.0, 1.0));
    let role_scale = match role {
        MemberRole::Adult => 1.0,
        MemberRole::Alpha => crate::creature_rules::ALPHA_SCALE,
        MemberRole::Baby => crate::creature_rules::BABY_SCALE,
    };
    (base * random * role_scale).clamp(0.1, MAX_ACTOR_SCALE)
}

/// A member's maximum HP (0xC04170 + 0xC05D50): the archetype's
/// `HitpointOverride`, x0.5 for a baby, x1.25 for an alpha.
pub fn member_max_hp(archetype: Archetype, role: MemberRole) -> f32 {
    crate::creature_rules::max_hit_points(
        crate::creature_rules::base_hit_points(0, Some(archetype.record().hit_points), 0.0),
        false,
        role.is_baby(),
        role.is_alpha(),
    )
}

/// The multiplier on every hit a member lands (0xC20390): the herd's
/// `DamageMultiplier`, a quarter for a baby attacker.
pub fn attack_multiplier(archetype: Archetype, role: MemberRole) -> f32 {
    let baby = if role.is_baby() {
        crate::creature_rules::BABY_ATTACK_MULTIPLIER
    } else {
        1.0
    };
    archetype.record().damage_multiplier * baby
}

/// Social rounds a member asks for (VERIFIED, SocialPropose start 0xDA4360:
/// behaviour +0x1AC): `numSocialProposals`; a baby asks for one fewer
/// (tuning 0xFB2020B9 = 1, global 0x1582F14), at least one; an alpha for
/// `numExtraSocialsForAlphas` more (global 0x1582F10).
pub fn social_rounds(archetype: Archetype, role: MemberRole) -> u8 {
    let base = archetype.record().social_proposals;
    if role.is_baby() {
        base.saturating_sub(FEWER_SOCIALS_FOR_BABIES).max(1)
    } else if role.is_alpha() {
        base + EXTRA_SOCIALS_FOR_ALPHAS
    } else {
        base.max(1)
    }
}

/// ADAPTED: wander radius around the nest. `TerritoryRadius` (20-200) is the
/// herd's claimed area; how far members roam inside it is not traced, so they
/// keep within 10-40 units of their nest and a herd stays recognisable.
/// Guards and babies keep to the guard ring (`guardLocationRadiusRange`).
pub fn territory_radius(archetype: Archetype, guard: bool, role: MemberRole) -> f32 {
    let record = archetype.record();
    let territory = (record.territory * CREATURE_WORLD_SCALE).clamp(10.0, 40.0);
    if guard || role.is_baby() {
        (record.guard_radius.1 * CREATURE_WORLD_SCALE).clamp(2.0, territory)
    } else {
        territory
    }
}

/// ADAPTED: radius of the ring members spawn on around their nest, so a herd
/// of large creatures does not stand inside itself (footprint 2.5 x scale).
pub fn spawn_ring_radius(scale: f32, count: usize) -> f32 {
    if count <= 1 {
        return 0.0;
    }
    let footprint = 2.5 * scale;
    (footprint * count as f32 / std::f32::consts::TAU * 1.1).max(1.1)
}

// ============================================================ relationships and DNA

/// Friends that finish a species (`NumToImpressForBestFriends`), bounded by
/// the herd so a one-creature herd can be finished.
pub fn best_friends_needed(archetype: Archetype, population: u32) -> u32 {
    archetype.record().impress_best_friends.max(1).min(population.max(1))
}

/// `NumToImpressForFriendly`.
pub fn friendly_needed(archetype: Archetype) -> u32 {
    archetype.record().impress_friendly.max(1)
}

/// Kills that make the species extinct (`NumToKillForExtinction`); `None`
/// never (the epic predator).
pub fn kills_to_extinction(archetype: Archetype) -> Option<u32> {
    let kills = archetype.record().kills_to_extinction;
    (kills > 0).then_some(kills as u32)
}

/// DNA for killing one member (0xC042A0 kill path): `evoPointsRewarded` with
/// the baby / alpha and level multipliers, at least 1.
pub fn kill_dna(
    archetype: Archetype,
    role: MemberRole,
    brain_level: u8,
    strength: i32,
    difficulty: crate::creature_rules::Difficulty,
) -> f32 {
    crate::creature_rules::creature_dna_reward(
        archetype.record().evo_kill,
        role.is_baby(),
        role.is_alpha(),
        brain_level,
        strength,
        difficulty,
    )
}

/// DNA for befriending one member (0xD9BBD0, medium confidence): the social
/// base like a kill, plus `mult * evoPointsSocialBonus * max(score - 0.5, 0)`
/// with `score` the friendship bar fraction (1 when the bar is full).
pub fn befriend_dna(
    archetype: Archetype,
    role: MemberRole,
    brain_level: u8,
    strength: i32,
    difficulty: crate::creature_rules::Difficulty,
    score: f32,
) -> f32 {
    let record = archetype.record();
    let base = crate::creature_rules::creature_dna_reward(
        record.evo_social,
        role.is_baby(),
        role.is_alpha(),
        brain_level,
        strength,
        difficulty,
    );
    let mult = crate::creature_rules::dna_points_multiplier(brain_level as i32, strength, difficulty);
    base + mult * record.evo_social_bonus * (score.clamp(0.0, 1.0) - 0.5).max(0.0)
}

/// DNA when a species is finished (0xC2EE80): `round(mult * extinctionReward)`
/// or `round(mult * relationshipBestFriendsReward)`.
pub fn species_finished_dna(
    archetype: Archetype,
    allied: bool,
    brain_level: u8,
    strength: i32,
    difficulty: crate::creature_rules::Difficulty,
) -> f32 {
    let record = archetype.record();
    let reward = if allied {
        record.best_friends_reward
    } else {
        record.extinction_reward
    };
    let mult = crate::creature_rules::dna_points_multiplier(brain_level as i32, strength, difficulty);
    (mult * reward).round().max(1.0)
}

/// How far a herd has evolved for the player's stage total: one step per
/// `dnaPointsEvolutionThresholds` entry passed (0 and 10000 never trigger).
pub fn evolution_stage(archetype: Archetype, stage_total: f32) -> u8 {
    let (first, second) = archetype.record().evolution_thresholds;
    [first, second]
        .into_iter()
        .filter(|t| *t > 0.0 && *t < 10_000.0 && stage_total >= *t)
        .count() as u8
}

// ============================================================ casting

/// One herd's casting decision for this interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cast {
    Keep,
    Activate,
    Deactivate,
}

/// Casting thresholds in world units (retail 100 / 150, chord distances).
pub fn casting_thresholds() -> (f32, f32) {
    (
        CASTING_DISTANCE_THRESHOLDS.0 * CREATURE_WORLD_SCALE,
        CASTING_DISTANCE_THRESHOLDS.1 * CREATURE_WORLD_SCALE,
    )
}

/// The casting manager's pass over the herds (tuning: activate within 100,
/// deactivate beyond 150, at most `max_new` activations per interval, nearest
/// first). `herds` is (active, chord distance) per herd.
pub fn casting_pass(herds: &[(bool, f32)], max_new: usize) -> Vec<Cast> {
    let (near, far) = casting_thresholds();
    let mut out: Vec<Cast> = herds
        .iter()
        .map(|(active, distance)| {
            if *active && *distance > far {
                Cast::Deactivate
            } else {
                Cast::Keep
            }
        })
        .collect();
    let mut candidates: Vec<(usize, f32)> = herds
        .iter()
        .enumerate()
        .filter(|(_, (active, distance))| !*active && *distance <= near)
        .map(|(i, (_, d))| (i, *d))
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    for (i, _) in candidates.into_iter().take(max_new) {
        out[i] = Cast::Activate;
    }
    out
}

// ============================================================ day clock

/// Hours per second at `hour`: the retail speed multiplier (the table
/// interpolated between its hour entries, 0xC85030..0xC850FE) times the base
/// 24 hours per `DEFAULT_DAY_LENGTH` seconds. Not normalised (see the table).
pub fn clock_rate(hour: f32) -> f32 {
    let hour = hour.rem_euclid(24.0);
    let i = (hour.floor() as usize).min(23);
    let t = hour - i as f32;
    let m = TIME_OF_DAY_MULTIPLIER_CREATURE[i] * (1.0 - t) + TIME_OF_DAY_MULTIPLIER_CREATURE[i + 1] * t;
    m * 24.0 / DEFAULT_DAY_LENGTH
}

/// Advance the clock by `dt` seconds (sub-stepped so a long frame does not
/// skip dawn or dusk).
pub fn advance_clock(hour: f32, dt: f32) -> f32 {
    if !hour.is_finite() {
        return START_TIME_OF_DAY;
    }
    if !dt.is_finite() || dt <= 0.0 {
        return hour.rem_euclid(24.0);
    }
    let mut hour = hour.rem_euclid(24.0);
    let mut left = dt;
    while left > 0.0 {
        let step = left.min(1.0);
        hour = (hour + clock_rate(hour) * step).rem_euclid(24.0);
        left -= step;
    }
    hour
}

/// Night for the Tribal vignette filter: from dusk to dawn (ADAPTED, see
/// `DUSK_HOUR`).
pub fn is_night(hour: f32) -> bool {
    in_window(hour.rem_euclid(24.0), (DUSK_HOUR, DAWN_HOUR))
}

/// Whether the Creature stage's `NightPredators` event is active at `hour`
/// (window 21:00 to 03:00 of the event prop).
pub fn night_predators_out(hour: f32) -> bool {
    in_window(hour.rem_euclid(24.0), (NIGHT_EVENT_START_HOUR, NIGHT_EVENT_END_HOUR))
}

/// The schedule a herd follows: one of its archetype's schedules, picked by
/// the species' own index (staggering herds of the same archetype).
pub fn herd_schedule(archetype: Archetype, pick: u32) -> &'static Schedule {
    let list = archetype.record().schedules;
    list[pick as usize % list.len()]
}

/// Where a night-predator herd appears around the avatar: the ambush distance
/// in a direction from the night's seed, on land (ADAPTED: the first of eight
/// bearings that lands ashore, else the shore toward the first; the retail
/// choice of the spot is not traced).
pub fn ambush_point(avatar: [f32; 2], night: u64, land: &crate::creature_planet::Land) -> [f32; 2] {
    let base = unit(night ^ 0x41AB) * std::f32::consts::TAU;
    let r = NIGHT_PREDATOR_AMBUSH_DISTANCE * CREATURE_WORLD_SCALE;
    let from = Vec2::new(avatar[0], avatar[1]);
    for k in 0..8 {
        let at = land.toward(from, r, base + k as f32 * std::f32::consts::TAU / 8.0);
        if land.is_land(at) {
            return [at.x, at.y];
        }
    }
    let at = land.land_toward(from, land.toward(from, r, base));
    [at.x, at.y]
}

// ============================================================ Bevy surface

/// A spawned herd member: its species, nest and role, so the stage systems do
/// not search the species world every frame. Transient (rebuilt on spawn).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HerdMember {
    pub species: u32,
    pub nest: usize,
    pub archetype: Archetype,
    pub role: MemberRole,
    pub guard: bool,
}

/// The casting manager's state for this visit: which nests are active, the
/// interval clock, and whether the last pass saw night.
#[derive(Resource, Debug, Clone, Default)]
pub struct CastingState {
    pub timer: f32,
    pub active: std::collections::HashSet<usize>,
    pub night: bool,
    /// Herd members that are still incubating in a nest egg (`creature_eggs`):
    /// the casting manager does not spawn them until the egg has hatched.
    pub incubating: std::collections::HashSet<u32>,
    /// Eggs laid by the last births, (nest index, member id), drawn by
    /// `creature_eggs::spawn_laid_eggs`.
    pub pending_eggs: Vec<(usize, u32)>,
}

/// Advance the Creature stage's time of day (`GlobalTuning` day length and
/// multipliers). Simulation time only, so a paused game stops the clock.
pub fn advance_day_clock(time: Res<Time>, mut run: ResMut<crate::game::RunProgress>) {
    if !run.species_world.generated {
        return;
    }
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let hour = run.species_world.time_of_day;
    run.species_world.time_of_day = advance_clock(hour, dt);
}

/// What a spawned member's schedule has it do now (`Idle` for a legacy
/// member, and for every member of a herd the player has angered).
pub fn member_activity(
    world: &crate::species::SpeciesWorld,
    member: Option<&HerdMember>,
) -> Activity {
    let Some(member) = member else {
        return Activity::Idle;
    };
    if world.species_aggravated(member.species) {
        return Activity::Idle;
    }
    let pick = world.species(member.species).map_or(0, |s| s.schedule);
    herd_schedule(member.archetype, pick).activity(world.time_of_day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archetype_records_match_the_resolved_herdtypes() {
        // Notes C4B, medium difficulty.
        let social = Archetype::Social.record();
        assert_eq!(social.num_creatures, (4.0, 4.0));
        assert_eq!(social.hit_points, 10.0);
        assert_eq!(social.damage_multiplier, 0.75);
        assert_eq!(social.size, 1.5);
        assert_eq!((social.impress_friendly, social.impress_best_friends), (2, 3));
        assert_eq!(social.kills_to_extinction, 3);
        assert_eq!(social.social_proposals, 1);
        assert_eq!((social.evo_kill, social.evo_social, social.evo_social_bonus), (7.0, 10.0, 5.0));
        assert_eq!((social.extinction_reward, social.best_friends_reward), (20.0, 25.0));
        assert_eq!(social.origin_distance, (35.0, 100.0));
        assert_eq!(social.evolution_thresholds, (50.0, 200.0));
        let combat = Archetype::Combat.record();
        assert_eq!(combat.social_power, 25.0);
        assert_eq!((combat.extinction_reward, combat.best_friends_reward), (25.0, 20.0));
        assert_eq!(combat.origin_distance, (100.0, 200.0));
        let small = Archetype::SmallSocial.record();
        assert_eq!((small.num_creatures.0, small.hit_points, small.size), (6.0, 15.0, 2.45));
        assert_eq!((small.kills_to_extinction, small.social_proposals), (4, 2));
        assert_eq!((small.evo_kill, small.evo_social), (8.0, 12.0));
        let medium = Archetype::MediumCombat.record();
        assert_eq!((medium.num_creatures.0, medium.hit_points, medium.size), (8.0, 25.0, 3.45));
        assert_eq!((medium.social_power, medium.kills_to_extinction), (300.0, 5));
        assert_eq!((medium.extinction_reward, medium.best_friends_reward), (40.0, 30.0));
        let large = Archetype::LargeCombat.record();
        assert_eq!((large.num_creatures.0, large.hit_points, large.size), (10.0, 40.0, 4.7));
        assert_eq!((large.extinction_reward, large.best_friends_reward), (70.0, 40.0));
        assert_eq!(large.alpha_share, 0.1);
        let night = Archetype::NightPredator.record();
        assert_eq!((night.num_creatures.0, night.hit_points, night.size), (3.0, 15.0, 2.5));
        assert_eq!(night.schedules[0].activity(3.0), Activity::Eat);
        let boss = Archetype::MiniBoss.record();
        assert_eq!((boss.hit_points, boss.damage_multiplier, boss.size), (250.0, 2.0, 5.7));
        assert_eq!((boss.impress_best_friends, boss.kills_to_extinction), (1, 1));
        assert_eq!((boss.evo_kill, boss.evo_social), (0.0, 0.0));
        assert_eq!((boss.extinction_reward, boss.best_friends_reward), (100.0, 100.0));
        assert_eq!(boss.respawn_rate, 300.0);
        assert!(!boss.has_nest && boss.miniboss);
        let epic = Archetype::EpicPredator.record();
        assert_eq!((epic.hit_points, epic.damage_multiplier, epic.size), (1000.0, 100.0, 20.0));
        assert_eq!(epic.kills_to_extinction, -1);
        assert_eq!((epic.impress_friendly, epic.impress_best_friends), (3, 6));
        assert_eq!((epic.evo_kill, epic.evo_social, epic.evo_social_bonus), (100.0, 100.0, 50.0));
        assert_eq!(epic.respawn_rate, 600.0);
        assert!(!epic.has_nest && epic.epic);
        // The "2" variants differ in their bands (and a few placement fields).
        assert_eq!(Archetype::Social2.record().origin_distance, (90.0, 150.0));
        assert_eq!(Archetype::SmallSocial2.record().origin_distance, (125.0, 325.0));
        assert_eq!(Archetype::SmallCombat2.record().origin_distance, (150.0, 350.0));
        assert_eq!(Archetype::MediumSocial2.record().origin_distance, (125.0, 325.0));
        assert_eq!(Archetype::MediumCombat2.record().origin_distance, (175.0, 325.0));
        assert_eq!(Archetype::LargeSocial2.record().origin_distance, (150.0, 350.0));
        for (a, b) in [
            (Archetype::Social, Archetype::Social2),
            (Archetype::SmallSocial, Archetype::SmallSocial2),
            (Archetype::SmallCombat, Archetype::SmallCombat2),
            (Archetype::MediumSocial, Archetype::MediumSocial2),
            (Archetype::MediumCombat, Archetype::MediumCombat2),
            (Archetype::LargeSocial, Archetype::LargeSocial2),
        ] {
            let (a, b) = (a.record(), b.record());
            assert_eq!(a.hit_points, b.hit_points);
            assert_eq!(a.evo_kill, b.evo_kill);
            assert_eq!(a.kills_to_extinction, b.kills_to_extinction);
            assert_eq!(a.diet, b.diet);
        }
    }

    #[test]
    fn archetype_names_hash_to_the_generation_list_ids() {
        // speciesFiles ids of CreatureGameLevelGenerationList.
        assert_eq!(Archetype::Carcass.instance_id(), 3_049_022_143);
        assert_eq!(Archetype::Social.instance_id(), 2_423_205_556);
        assert_eq!(Archetype::Social2.instance_id(), 319_194_478);
        assert_eq!(Archetype::Combat.instance_id(), 2_411_464_625);
        assert_eq!(Archetype::SmallSocial2.instance_id(), 4_166_439_282);
        assert_eq!(Archetype::SmallCombat2.instance_id(), 1_966_503_149);
        assert_eq!(Archetype::MediumSocial2.instance_id(), 1_927_692_980);
        assert_eq!(Archetype::MediumCombat2.instance_id(), 2_373_438_799);
        assert_eq!(Archetype::LargeSocial2.instance_id(), 746_137_246);
        // The extra sets.
        assert_eq!(Archetype::MediumCombat.instance_id(), 2_471_141_039);
        assert_eq!(Archetype::MediumSocial.instance_id(), 45_997_858);
        assert_eq!(Archetype::LargeSocial.instance_id(), 1_215_306_276);
        assert_eq!(Archetype::LargeCombat.instance_id(), 1_436_173_025);
        assert_eq!(Archetype::EpicPredator.instance_id(), 4_236_317_208);
        assert_eq!(Archetype::MiniBoss.instance_id(), 3_836_483_247);
        assert_eq!(Archetype::SmallCombat.instance_id(), 3_521_642_245);
        assert_eq!(Archetype::SmallSocial.instance_id(), 790_999_488);
    }

    #[test]
    fn the_generation_list_places_the_22_retail_herds() {
        let counts = |plan: &[HerdPlan], a: Archetype| plan.iter().filter(|h| h.archetype == a).count();
        for seed in 0..64u64 {
            let plan = plan_population(seed);
            // Every speciesFiles archetype has PlacementPercentage 1.0.
            assert_eq!(counts(&plan, Archetype::Carcass), 1);
            assert_eq!(counts(&plan, Archetype::Social), 5);
            assert_eq!(counts(&plan, Archetype::Social2), 2);
            assert_eq!(counts(&plan, Archetype::Combat), 3);
            assert_eq!(counts(&plan, Archetype::SmallSocial2), 4);
            assert_eq!(counts(&plan, Archetype::SmallCombat2), 3);
            assert_eq!(counts(&plan, Archetype::MediumSocial2), 2);
            assert_eq!(counts(&plan, Archetype::MediumCombat2), 1);
            assert_eq!(counts(&plan, Archetype::LargeSocial2), 1);
            assert_eq!(plan.len(), 22);
            assert_eq!(plan, plan_population(seed), "pure");
        }
        let plan = plan_population(7);
        let members: u32 = plan
            .iter()
            .filter(|h| h.archetype.is_herd())
            .map(|h| herd_size(h.archetype, h.slot as u64))
            .sum();
        assert!(members >= 116, "the retail list fields ~116 creatures, got {members}");
    }

    #[test]
    fn herds_are_placed_in_their_bands_on_the_planet() {
        // Land within 450 units of the pole: every band (up to 350) fits.
        let land = crate::creature_planet::tests::island_planet(450.0);
        for seed in [1u64, 7, 99, 12345] {
            let plan = plan_population(seed);
            let placed = place_population(seed, &plan, HOME, &land);
            assert_eq!(placed.len(), plan.len());
            let home = Vec2::from(HOME);
            let chord = |p: &Placement| land.chord(Vec2::from(p.position), home);
            let mut in_band = 0;
            for p in &placed {
                assert!(p.position.iter().all(|v| v.is_finite()));
                assert!(land.is_land(Vec2::from(p.position)), "{:?} at sea", p);
                let d = chord(p);
                if p.archetype != Archetype::Carcass {
                    assert!(d >= HOME_CLEARANCE - 1e-2, "{:?} too close to home", p);
                }
                let (lo, hi) = band(p.archetype, 0);
                let clear = if p.archetype == Archetype::Carcass { 0.0 } else { HOME_CLEARANCE };
                if d >= lo.max(clear) - 1e-2 && d <= hi.max(clear + 1.0) + 1e-2 {
                    in_band += 1;
                }
            }
            // Retail bands, measured as chords on the planet.
            assert!(in_band * 10 >= placed.len() * 9, "seed {seed}: {in_band}/{}", placed.len());
            // The starter herds live closest: every NPC_Social nest is nearer
            // home than every small-social2 nest.
            let far = |a| placed.iter().filter(move |p| p.archetype == a).map(chord);
            let social_max = far(Archetype::Social).fold(0.0, f32::max);
            let small2_min = far(Archetype::SmallSocial2).fold(f32::INFINITY, f32::min);
            assert!(social_max <= 100.0 + 1e-2);
            assert!(small2_min > social_max, "seed {seed}");
            assert_eq!(placed, place_population(seed, &plan, HOME, &land), "pure");
        }
    }

    #[test]
    fn nests_keep_their_water_distance_and_never_stand_at_sea() {
        // A small continent: the far bands run into the sea and must give way.
        let land = crate::creature_planet::tests::island_planet(220.0);
        let plan = plan_population(7);
        let placed = place_population(7, &plan, HOME, &land);
        for p in &placed {
            let at = Vec2::from(p.position);
            assert!(land.is_land(at), "{:?} at sea", p);
        }
        // Most herds also keep their WaterDistance from the shore.
        let dry = placed
            .iter()
            .filter(|p| !land.water_within(Vec2::from(p.position), p.archetype.record().water_distance))
            .count();
        assert!(dry * 10 >= placed.len() * 7, "{dry}/{}", placed.len());
    }

    #[test]
    fn herds_mostly_respect_their_exclusive_areas() {
        let seed = 42;
        let plan = plan_population(seed);
        let placed = place_population(seed, &plan, HOME, &crate::creature_planet::Land::flat());
        let mut clear = 0;
        let mut pairs = 0;
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[..i] {
                pairs += 1;
                // `a` was placed after `b`: it kept out of b's marked areas.
                let need = exclusive_clearance(a.archetype, b.archetype) * 0.5;
                if dist2(a.position, b.position) + 1e-3 >= need {
                    clear += 1;
                }
            }
        }
        assert!(clear * 100 >= pairs * 95, "{clear}/{pairs} pairs keep half their exclusive area");
    }

    #[test]
    fn forest_sets_place_their_herds_around_each_forest_without_relaxing() {
        let land = crate::creature_planet::Land::flat();
        assert_eq!(FOREST_SPECIES_RULES[0], &[Archetype::EpicPredator, Archetype::MiniBoss, Archetype::SmallCombat]);
        let forests = [[120.0, 40.0], [-90.0, 150.0]];
        let mut seen = std::collections::HashSet::new();
        for seed in 0..32u64 {
            let mut placed = place_population(seed, &plan_population(seed), HOME, &land);
            let before = placed.len();
            let herds = place_forest_population(seed, &forests, &mut placed, &land);
            assert_eq!(placed.len(), before + herds.len());
            for h in &herds {
                seen.insert(h.archetype);
                // Each herd sits in its band measured from its forest.
                let (lo, hi) = band(h.archetype, 0);
                let d = dist2(h.position, forests[h.forest]);
                assert!(d >= lo - 1e-3 && d <= hi + 1e-3, "{:?} {d} not in {lo}..{hi}", h.archetype);
                assert!(dist2(h.position, HOME) >= HOME_CLEARANCE - 1e-3);
            }
            // Strict exclusive areas: a forest herd keeps out of every herd
            // placed before it.
            for (i, h) in herds.iter().enumerate() {
                let index = before + i;
                for other in &placed[..index] {
                    assert!(dist2(h.position, other.position) + 1e-3 >= exclusive_clearance(h.archetype, other.archetype).max(1.0));
                }
            }
        }
        // Over many worlds the big herds appear (large combat rolls 1.0).
        assert!(seen.contains(&Archetype::LargeCombat), "{seen:?}");
        // No planet, no forests.
        assert!(land.forests(Vec2::from(HOME), FOREST_RADIUS).is_empty());
    }

    #[test]
    fn member_roles_follow_the_archetype_shares() {
        let roles = member_roles(Archetype::Social, 4, 1);
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Alpha).count(), 1, "0.25 of 4");
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Baby).count(), 0);
        assert_eq!(roles[0].0, MemberRole::Alpha, "alphas come first");
        let roles = member_roles(Archetype::SmallSocial, 6, 1);
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Alpha).count(), 2, "0.333 of 6");
        let roles = member_roles(Archetype::MediumCombat, 8, 1);
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Alpha).count(), 3, "0.375 of 8");
        let roles = member_roles(Archetype::LargeSocial, 10, 1);
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Alpha).count(), 4, "0.4 of 10");
        let roles = member_roles(Archetype::LargeCombat, 10, 1);
        assert_eq!(roles.iter().filter(|r| r.0 == MemberRole::Alpha).count(), 1, "0.1 of 10");
        // Babies: up to 0.2 of 10 for large herds.
        let babies: Vec<usize> = (0..64)
            .map(|k| member_roles(Archetype::LargeSocial, 10, k).iter().filter(|r| r.0 == MemberRole::Baby).count())
            .collect();
        assert!(babies.iter().all(|b| *b <= 2));
        assert!(babies.iter().any(|b| *b > 0));
        // Guards: the guard posts (locations x per location), adults only.
        for (a, n) in [(Archetype::Social, 4), (Archetype::MediumSocial, 8), (Archetype::LargeSocial, 10)] {
            let roles = member_roles(a, n, 3);
            let guards = roles.iter().filter(|r| r.1).count();
            let adults = roles.iter().filter(|r| r.0 == MemberRole::Adult).count();
            let record = a.record();
            assert_eq!(guards, ((record.guard_locations * record.guards_per_location) as usize).min(adults));
            assert!(roles.iter().filter(|r| r.1).all(|r| r.0 == MemberRole::Adult));
        }
        // Single roamers have no alpha (miniboss share 0, epic default 0.1 of 1).
        assert!(member_roles(Archetype::MiniBoss, 1, 0).iter().all(|r| r.0 == MemberRole::Adult));
        assert!(member_roles(Archetype::EpicPredator, 1, 0).iter().all(|r| r.0 == MemberRole::Adult));
    }

    #[test]
    fn member_stats_use_the_archetype_hp_damage_and_size() {
        assert_eq!(member_max_hp(Archetype::Social, MemberRole::Adult), 10.0);
        assert_eq!(member_max_hp(Archetype::Social, MemberRole::Alpha), 12.5);
        assert_eq!(member_max_hp(Archetype::Social, MemberRole::Baby), 5.0);
        assert_eq!(member_max_hp(Archetype::SmallCombat2, MemberRole::Adult), 15.0);
        assert_eq!(member_max_hp(Archetype::MediumSocial2, MemberRole::Adult), 25.0);
        assert_eq!(member_max_hp(Archetype::LargeSocial2, MemberRole::Adult), 40.0);
        assert_eq!(member_max_hp(Archetype::MiniBoss, MemberRole::Adult), 250.0);
        assert_eq!(member_max_hp(Archetype::EpicPredator, MemberRole::Adult), 1000.0);
        assert_eq!(attack_multiplier(Archetype::Social, MemberRole::Adult), 0.75);
        assert_eq!(attack_multiplier(Archetype::Social, MemberRole::Baby), 0.75 * 0.25);
        assert_eq!(attack_multiplier(Archetype::MiniBoss, MemberRole::Adult), 2.0);
        assert_eq!(attack_multiplier(Archetype::EpicPredator, MemberRole::Adult), 100.0);
        // Sizes grow with the archetype; the starter herds match the avatar.
        assert!((species_scale(Archetype::Social) * 2.5 - 1.9).abs() < 1e-4);
        let order = [
            Archetype::Social,
            Archetype::SmallSocial,
            Archetype::MediumSocial,
            Archetype::LargeSocial,
            Archetype::MiniBoss,
            Archetype::EpicPredator,
        ];
        for pair in order.windows(2) {
            assert!(species_scale(pair[0]) < species_scale(pair[1]));
        }
        // The epic keeps its retail proportion (size 20 against 1.5), uncapped.
        assert!(
            (species_scale(Archetype::EpicPredator) - 20.0 * SIZE_TO_SCALE).abs() < 1e-4,
            "{}",
            species_scale(Archetype::EpicPredator)
        );
        assert!(species_scale(Archetype::EpicPredator) < MAX_ACTOR_SCALE);
        assert!(
            (species_scale(Archetype::EpicPredator) / species_scale(Archetype::Social) - 20.0 / 1.5).abs() < 1e-3,
            "the epic is 13.3 times the starter herds"
        );
        let base = species_scale(Archetype::Social);
        assert!(member_scale(base, MemberRole::Baby, 0.5) < member_scale(base, MemberRole::Adult, 0.5));
        assert!(member_scale(base, MemberRole::Alpha, 0.5) > member_scale(base, MemberRole::Adult, 0.5));
        assert!((member_scale(1.0, MemberRole::Adult, 0.0) - 0.8).abs() < 1e-6);
        assert!((member_scale(1.0, MemberRole::Adult, 1.0) - 1.1).abs() < 1e-6);
        // Social rounds: proposals, +1 for alphas.
        assert_eq!(social_rounds(Archetype::Social, MemberRole::Adult), 1);
        assert_eq!(social_rounds(Archetype::Social, MemberRole::Alpha), 2);
        assert_eq!(social_rounds(Archetype::SmallSocial2, MemberRole::Adult), 2);
        assert_eq!(social_rounds(Archetype::LargeSocial2, MemberRole::Adult), 3);
        assert_eq!(social_rounds(Archetype::MiniBoss, MemberRole::Adult), 4);
    }

    #[test]
    fn relationship_thresholds_and_dna_follow_the_archetype() {
        use crate::creature_rules::Difficulty;
        assert_eq!(best_friends_needed(Archetype::Social, 4), 3);
        assert_eq!(friendly_needed(Archetype::Social), 2);
        assert_eq!(best_friends_needed(Archetype::MiniBoss, 1), 1);
        assert_eq!(best_friends_needed(Archetype::EpicPredator, 1), 1, "bounded by the herd");
        assert_eq!(kills_to_extinction(Archetype::Social), Some(3));
        assert_eq!(kills_to_extinction(Archetype::SmallCombat2), Some(4));
        assert_eq!(kills_to_extinction(Archetype::MediumSocial2), Some(5));
        assert_eq!(kills_to_extinction(Archetype::LargeSocial2), Some(7));
        assert_eq!(kills_to_extinction(Archetype::MiniBoss), Some(1));
        assert_eq!(kills_to_extinction(Archetype::EpicPredator), None);
        // Brain level 0 meets strength 0: full pay.
        let m = Difficulty::Medium;
        assert_eq!(kill_dna(Archetype::Social, MemberRole::Adult, 0, 0, m), 7.0);
        assert_eq!(kill_dna(Archetype::Social, MemberRole::Alpha, 0, 0, m), 8.75);
        assert_eq!(kill_dna(Archetype::Social, MemberRole::Baby, 0, 0, m), 1.75);
        assert_eq!(kill_dna(Archetype::LargeCombat, MemberRole::Adult, 0, 0, m), 10.0);
        assert_eq!(kill_dna(Archetype::MiniBoss, MemberRole::Adult, 0, 0, m), 1.0, "at least 1");
        // Befriending: social base + half the social bonus at a full bar.
        assert_eq!(befriend_dna(Archetype::Social, MemberRole::Adult, 0, 0, m, 1.0), 12.5);
        assert_eq!(befriend_dna(Archetype::SmallSocial, MemberRole::Adult, 0, 0, m, 1.0), 15.0);
        assert_eq!(befriend_dna(Archetype::Social, MemberRole::Adult, 0, 0, m, 0.4), 10.0);
        // Two brain levels above the target: index 4 -> 0.01, at least 1.
        assert_eq!(kill_dna(Archetype::Social, MemberRole::Adult, 2, 0, m), 1.0);
        // Species finished.
        assert_eq!(species_finished_dna(Archetype::Social, true, 0, 0, m), 25.0);
        assert_eq!(species_finished_dna(Archetype::Social, false, 0, 0, m), 20.0);
        assert_eq!(species_finished_dna(Archetype::LargeCombat, false, 0, 0, m), 70.0);
        assert_eq!(species_finished_dna(Archetype::MiniBoss, true, 0, 0, m), 100.0);
        assert_eq!(species_finished_dna(Archetype::Social, true, 1, 1, m), 8.0, "round(0.3 * 25)");
    }

    #[test]
    fn npc_herds_evolve_at_the_players_dna_thresholds() {
        assert_eq!(evolution_stage(Archetype::Social, 0.0), 0);
        assert_eq!(evolution_stage(Archetype::Social, 50.0), 1);
        assert_eq!(evolution_stage(Archetype::Social, 200.0), 2);
        assert_eq!(evolution_stage(Archetype::SmallSocial2, 449.0), 1);
        assert_eq!(evolution_stage(Archetype::MediumCombat2, 650.0), 2);
        assert_eq!(evolution_stage(Archetype::LargeSocial2, 1000.0), 0, "large herds never evolve");
        assert_eq!(evolution_stage(Archetype::NightPredator, 1000.0), 0);
        assert_eq!(evolution_stage(Archetype::MiniBoss, 1000.0), 0);
    }

    #[test]
    fn casting_activates_near_herds_two_at_a_time_and_drops_far_ones() {
        let (near, far) = casting_thresholds();
        assert_eq!((near, far), (100.0, 150.0));
        let herds = [
            (false, 30.0),
            (false, 90.0),
            (false, 10.0),
            (false, 120.0),
            (true, 140.0),
            (true, 160.0),
        ];
        let pass = casting_pass(&herds, CASTING_MAX_HERDS_PER_INTERVAL);
        assert_eq!(pass[2], Cast::Activate, "nearest first");
        assert_eq!(pass[0], Cast::Activate);
        assert_eq!(pass[1], Cast::Keep, "only two per interval");
        assert_eq!(pass[3], Cast::Keep, "between the thresholds stays inactive");
        assert_eq!(pass[4], Cast::Keep, "between the thresholds stays active");
        assert_eq!(pass[5], Cast::Deactivate);
        let all = casting_pass(&herds, usize::MAX);
        assert_eq!(all[1], Cast::Activate);
    }

    #[test]
    fn the_day_lasts_the_retail_day_length_and_starts_at_seven() {
        // 40 x sum over the hours of ln(b / a) / (b - a) (1 / a when equal), the
        // table interpolated linearly: 1092 s (the base 960 s is the length at
        // multiplier 1).
        const RETAIL_DAY_SECONDS: f32 = 1092.1;
        // Walk one whole day in small steps.
        let mut hour = START_TIME_OF_DAY;
        let mut seconds = 0.0;
        let mut wrapped = false;
        loop {
            let next = advance_clock(hour, 0.25);
            seconds += 0.25;
            if next < hour {
                wrapped = true;
            }
            if wrapped && next >= START_TIME_OF_DAY {
                break;
            }
            hour = next;
            assert!(seconds < 3.0 * RETAIL_DAY_SECONDS);
        }
        assert!(
            (seconds - RETAIL_DAY_SECONDS).abs() < 5.0,
            "one day took {seconds} s"
        );
        // Dawn passes faster than noon.
        assert!(clock_rate(4.0) > clock_rate(12.0) * 10.0);
        assert!(is_night(22.0) && is_night(2.0) && !is_night(12.0) && !is_night(7.0));
        assert!(night_predators_out(21.5) && night_predators_out(0.0) && night_predators_out(2.9));
        assert!(!night_predators_out(20.5) && !night_predators_out(3.0) && !night_predators_out(12.0));
        assert!((clock_rate(12.0) - 0.25 * 24.0 / DEFAULT_DAY_LENGTH).abs() < 1e-6);
        assert_eq!(advance_clock(f32::NAN, 1.0), START_TIME_OF_DAY);
        assert_eq!(advance_clock(25.0, 0.0), 1.0);
    }

    #[test]
    fn schedules_pick_the_activity_of_the_hour() {
        assert_eq!(SLEEP_ANIMAL_SCHEDULE.activity(23.0), Activity::Sleep);
        assert_eq!(SLEEP_ANIMAL_SCHEDULE.activity(4.5), Activity::Sleep);
        assert_eq!(SLEEP_ANIMAL_SCHEDULE.activity(12.0), Activity::Idle);
        assert_eq!(HERB_ANIMAL_SCHEDULE.activity(5.0), Activity::Eat);
        assert_eq!(HERB_ANIMAL_SCHEDULE.activity(8.0), Activity::Sleep);
        assert_eq!(HERB_ANIMAL_SCHEDULE.activity(10.0), Activity::Drink);
        assert_eq!(HERB_ANIMAL_SCHEDULE.activity(11.5), Activity::Idle);
        assert_eq!(HERB1_ANIMAL_SCHEDULE.activity(9.0), Activity::Eat);
        assert_eq!(HERB2_ANIMAL_SCHEDULE.activity(14.0), Activity::Sleep);
        assert_eq!(DEFAULT_ANIMAL_SCHEDULE.activity(16.0), Activity::Eat);
        assert_eq!(NIGHT_PREDATOR_SCHEDULE.activity(0.0), Activity::Eat);
        assert_eq!(NIGHT_PREDATOR_SCHEDULE.activity(23.9), Activity::Eat);
        // Herds of the same archetype are staggered over its schedules.
        assert_ne!(
            herd_schedule(Archetype::SmallSocial2, 0).name,
            herd_schedule(Archetype::SmallSocial2, 1).name
        );
        assert_eq!(herd_schedule(Archetype::Social, 5).name, "SleepAnimalSchedule");
    }

    #[test]
    fn the_night_ambush_lands_near_the_avatar_on_land() {
        let flat = crate::creature_planet::Land::flat();
        for night in 0..32u64 {
            let p = ambush_point([0.0, 8.0], night, &flat);
            assert!((dist2(p, [0.0, 8.0]) - NIGHT_PREDATOR_AMBUSH_DISTANCE).abs() < 1e-3);
        }
        // Near a coast the predators come from the land side.
        let land = crate::creature_planet::tests::island_planet(200.0);
        for night in 0..32u64 {
            let p = ambush_point([150.0, 0.0], night, &land);
            assert!(land.is_land(Vec2::from(p)), "night {night}: {p:?}");
            assert!(land.chord(Vec2::from(p), Vec2::new(150.0, 0.0)) <= NIGHT_PREDATOR_AMBUSH_DISTANCE + 0.1);
        }
    }
}
