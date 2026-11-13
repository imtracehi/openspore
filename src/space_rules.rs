//! Retail Space-stage rules in one place: the original's tables and formulas
//! as named constants and pure functions that `space.rs` (and the HUD) call.
//!
//! Source: `notes\space-re-notes.md` sections S0-S3 and S8 (reverse-engineered
//! from the original executable and the installed `PatchData.package` tuning;
//! addresses are the ModAPI march2017 (Steam) addresses, decompilations in
//! `notes\space-re\dec\`, dumps in `notes\space-re\*.txt`). "VERIFIED" marks a
//! formula read in the code image (the notes, or by `space-rules` itself:
//! the archetype table 0x13F3DF0, ApplyRelationship 0xD05DD0 / 0xD01730 and
//! the relationship decay 0xD06070); "tuning" marks a value read from an
//! installed property whose use was traced. Whatever the executable does not
//! give us is marked `ADAPTED:` so it can be replaced later.
//!
//! Sections: difficulty, S1 progression (money, tools, archetype cards,
//! tutorial, missions, badges and ranks, galactic core, ship destruction),
//! S2 core rules (health and energy, repair and recharge, jumps, weapons, NPC
//! ships and loot, superpowers, store, spice and colonies), S3 empires
//! (relationship model, thresholds, gifts and tribute, peace and purchase
//! offers, awareness and raid timers, empire AI, personalities).
// Many named retail values are kept for the HUD / comm-screen / galaxy passes
// that will use them and are only exercised by the tests.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::space::SpaceArchetype;

// ================================================================= difficulty

/// The game-wide difficulty (`CellSave::difficulty`: 0 easy, 1 normal, 2 hard)
/// as the Space tuning arrays index it (`DifficultyTuning` 0x02CA2581).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub fn from_index(index: i32) -> Self {
        match index {
            i32::MIN..=0 => Difficulty::Easy,
            1 => Difficulty::Normal,
            _ => Difficulty::Hard,
        }
    }

    /// Index into the three-entry `difficultySpace*` arrays.
    pub fn index(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
        }
    }

    pub fn pick(self, values: [f32; 3]) -> f32 {
        values[self.index()]
    }
}

/// `difficultySpacePlayerHealth` (tuning, used in 0xFFD780).
pub const DIFFICULTY_PLAYER_HEALTH: [f32; 3] = [1.5, 1.0, 0.3];
/// `difficultySpaceAllyHealth`.
pub const DIFFICULTY_ALLY_HEALTH: [f32; 3] = [1.5, 1.0, 0.5];
/// `difficultySpaceSpiceCost`: spice sell-price multiplier.
pub const DIFFICULTY_SPICE_COST: [f32; 3] = [1.666, 1.333, 1.333];
/// `difficultySpaceNPCAwareness`.
pub const DIFFICULTY_NPC_AWARENESS: [f32; 3] = [0.7, 1.0, 1.5];
/// `difficultySpaceTradeToolCost`.
pub const DIFFICULTY_TRADE_TOOL_COST: [f32; 3] = [1.0, 1.0, 1.0];
/// `difficultySpaceMissionTime`: mission time-limit multiplier.
pub const DIFFICULTY_MISSION_TIME: [f32; 3] = [2.0, 1.0, 0.75];
/// `difficultySpaceMissionRewardCash`.
pub const DIFFICULTY_MISSION_REWARD_CASH: [f32; 3] = [1.0, 1.0, 1.0];
/// `difficultySpaceTradeReoffer`.
pub const DIFFICULTY_TRADE_REOFFER: [f32; 3] = [1.0, 1.0, 1.0];
/// `difficultySpaceEnemyHealth`.
pub const DIFFICULTY_ENEMY_HEALTH: [f32; 3] = [1.0, 1.0, 1.0];
/// `difficultySpaceDisaster`.
pub const DIFFICULTY_DISASTER: [f32; 3] = [20.0, 6.0, 2.0];
/// `difficultySpiceProduction` (used in 0xC720A0).
pub const DIFFICULTY_SPICE_PRODUCTION: [f32; 3] = [1.0, 1.0, 0.8];

// ============================================================== S1 money, tools

/// `spaceEconomyStartingMoney` (tuning; 0xBB14D0 -> 0x102FCC0): every empire
/// record, the player's included, is created with this many Sporebucks.
pub const STARTING_MONEY: f32 = 100_000.0;

/// ADAPTED: the conversion applied to the few OpenSpore costs that have no
/// retail counterpart (adapted terraform / food-web steps, colony city
/// buildings without a tuning price). The old prototype economy counted in
/// units roughly a thousandth of the retail Sporebuck amounts (colony 120 vs
/// 150,000, missions 100-200 vs 15,000-400,000).
pub const LEGACY_SPICE_TO_SPOREBUCKS: f32 = 1000.0;

/// Retail tool ids (`spacetools~` instance names) used by the rules.
pub mod tool {
    pub const RADAR: &str = "radar";
    pub const SCAN: &str = "scan";
    pub const LASER_1: &str = "laserlevel1";
    pub const LASER_2: &str = "laserlevel2";
    pub const LASER_3: &str = "laserlevel3";
    pub const PROTON_MISSILE_1: &str = "protonmissilelevel1";
    pub const PROTON_MISSILE_2: &str = "protonmissilelevel2";
    pub const PROTON_MISSILE_3: &str = "protonmissilelevel3";
    pub const PULSE_1: &str = "pulselevel1";
    pub const ABDUCT: &str = "abduct";
    pub const DROP_CARGO: &str = "dropcargo";
    pub const HEALTH_1: &str = "healthupgrade1";
    pub const ENERGY_1: &str = "energyupgrade1";
    pub const INTERPLANETARY_DRIVE: &str = "interplanetarydrive";
    pub const INTERSTELLAR_DRIVE: &str = "interstellardrive";
    pub const SETI: &str = "seti";
    pub const PLACE_COLONY: &str = "placecolony";
    pub const GENESIS_DEVICE: &str = "genesisdevice";
    pub const PLACE_HAPPINESS_BOOSTER: &str = "placehappinessbooster";
    pub const MONOLITH: &str = "monolith";
    pub const PLACE_UBER_TURRET: &str = "placeuberturret";
    pub const ANTIMATTER_BOMB: &str = "antimatterbomb";
    pub const PLANET_BUSTER: &str = "planetbusterbomb";
    pub const ASTEROID: &str = "asteroid";
    pub const ICE_STORM: &str = "icestorm";
    pub const HEAT_RAY: &str = "heatray";
    pub const COLD_RAY: &str = "coldray";
    pub const CLOUD_RAY: &str = "cloudray";
    pub const CLOUD_VACUUM: &str = "cloudvacuum";
    pub const CREATURE_EDIT: &str = "creatureedit";
    pub const HAPPY_RAY_1: &str = "happyraylevel1";
    pub const ATMOSPHERE_GENERATOR: &str = "atmospheregenerator";
    pub const DROUGHT_GENERATOR: &str = "droughtgenerator";
}

/// `spaceInitTools` (0xFF5D90): the first tool the player is given.
pub const INIT_TOOLS: &[&str] = &[tool::RADAR];
/// `spaceEnabledTools`: the default loadout.
pub const ENABLED_TOOLS: &[&str] = &[
    tool::SCAN,
    tool::LASER_1,
    tool::ABDUCT,
    tool::DROP_CARGO,
    tool::HEALTH_1,
    tool::ENERGY_1,
];
/// `spaceDecline101Tools`: what declining the Flight 101 tutorial gives.
pub const DECLINE_101_TOOLS: &[&str] = &[tool::INTERPLANETARY_DRIVE];
/// `TutorialStory201.missionUnlockToolsSpecial`: unlocked when Story 201 starts.
pub const STORY201_UNLOCK_TOOLS: &[&str] = &[tool::INTERSTELLAR_DRIVE, tool::PROTON_MISSILE_1];
/// `TutorialStory201.missionGenericRewardUnlock`.
pub const STORY201_REWARD_UNLOCK: &str = tool::SETI;
/// `meaningOfLifeRewardTools` (ToolConfig): the candidate rewards of the core,
/// loaded into the inventory's list by its constructor (0xFF5040 -> 0xFF4C00).
/// Steve gives each one whose `toolMeaningOfLifeRewardCount` is above 0
/// ([`CORE_REWARD_COUNTS`]): only the Staff of Life (VERIFIED 0xFF3CA0).
pub const CORE_REWARD_TOOLS: &[&str] = &[
    tool::GENESIS_DEVICE,
    tool::PLACE_HAPPINESS_BOOSTER,
    tool::MONOLITH,
    tool::PLACE_UBER_TURRET,
    tool::ANTIMATTER_BOMB,
];

/// `toolMeaningOfLifeRewardCount` of each [`CORE_REWARD_TOOLS`] entry (tool
/// property files, VERIFIED): 42, 0, 0, 0, 0.
pub const CORE_REWARD_COUNTS: [u32; 5] = [42, 0, 0, 0, 0];

/// The tools Steve gives and the charge count each is set to.
pub fn core_reward_grants() -> Vec<(&'static str, u32)> {
    CORE_REWARD_TOOLS.iter().zip(CORE_REWARD_COUNTS).filter(|(_, n)| *n > 0).map(|(t, n)| (*t, n)).collect()
}

/// Before Steve was met (flag 0x061F1454), the interstellar drive's reach near
/// the core shrinks to `galacticCoreTravelLimit` 3 (VERIFIED 0xFFC6A0, tuning
/// 0x1106D054): inside `galacticCoreTravelRadii` (70, 100) the reach is
/// smoothstep(70, 100, d) x (range - 3) + 3, d = the star's distance to the
/// core in galaxy units (0xB0EFC0 is the smoothstep).
pub const GALACTIC_CORE_TRAVEL_LIMIT: f32 = 3.0;
pub const GALACTIC_CORE_TRAVEL_RADII: (f32, f32) = (70.0, 100.0);

pub fn core_travel_range(range: f32, distance_to_core: f32, met_steve: bool) -> f32 {
    let (inner, outer) = GALACTIC_CORE_TRAVEL_RADII;
    if met_steve || distance_to_core >= outer || range <= GALACTIC_CORE_TRAVEL_LIMIT {
        return range;
    }
    let t = ((distance_to_core - inner) / (outer - inner)).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    s * (range - GALACTIC_CORE_TRAVEL_LIMIT) + GALACTIC_CORE_TRAVEL_LIMIT
}

/// The galactic-object rare "Talked to Steve" (`rare-galacticobject10`,
/// 0xA9E151E4), found by Steve's gift; 1..9 are found by the first visit of a
/// special star (0xFDD590).
pub const RARE_TALKED_TO_STEVE: u8 = 10;

/// The tools a new Space game starts with (init + enabled lists).
pub fn starting_tools() -> Vec<String> {
    INIT_TOOLS
        .iter()
        .chain(ENABLED_TOOLS)
        .map(|t| t.to_string())
        .collect()
}

// ======================================================= S1 archetype (cards)

/// Colour of an earlier stage's consequence card: red (cell carnivore,
/// creature attack, tribe attack, civ military), blue (omnivore, mix, mix,
/// economic), green (herbivore, social, social, religious).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardColor {
    Red,
    Blue,
    Green,
}

impl CardColor {
    /// The archetype table's column value (0x556440: meat / attack /
    /// military 1, mix / economic 2, plant / social / religious 3).
    fn table_value(card: Option<CardColor>) -> usize {
        match card {
            None => 0,
            Some(CardColor::Red) => 1,
            Some(CardColor::Blue) => 2,
            Some(CardColor::Green) => 3,
        }
    }
}

/// The four earlier consequence cards (`None` = stage skipped / no card).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StageCards {
    #[serde(default)]
    pub cell: Option<CardColor>,
    #[serde(default)]
    pub creature: Option<CardColor>,
    #[serde(default)]
    pub tribe: Option<CardColor>,
    #[serde(default)]
    pub civ: Option<CardColor>,
}

impl StageCards {
    pub fn count(&self) -> usize {
        [self.cell, self.creature, self.tribe, self.civ]
            .iter()
            .filter(|c| c.is_some())
            .count()
    }
}

/// VERIFIED: the archetype table at 0x13F3DF0, read by 0x556440 with the
/// entry index `creature + 4 * cell + 16 * tribe + 64 * civ` (0 none, 1 red,
/// 2 blue, 3 green per stage; the card ids hash to clg_meat/mix/plant,
/// crg_attack/mix/social, trg_attack/mix/social, cvg_military/economic/
/// religious). Letters: W warrior, T trader, S shaman, C scientist, Z zealot,
/// D diplomat, K knight, B bard, E ecologist, N wanderer. Dumped from the
/// code image by `space-rules` (all 256 entries).
const ARCHETYPE_TABLE: [&str; 4] = [
    "NWTSWWCZTCTDSZDSWWCZWWCZCCCWZZWZTCTDCCCTTCTDDTDDSZDSZZSZDSDDSZDS",
    "WWCZWWCZCCCWZZWZWWCZWWWWCWCKZWKZCCCWCWCKCCTBWKBEZZWZZWKZWKBEZZES",
    "TCTDCCCTTCTDDTDDCCCTCWCKCCTBTKBETCTDCCTBTTTTDBTDDTDDTKBEDBTDDEDS",
    "SZDSZZSZDSDDSZDSZZSZZWKZSKBEZZESDSDDSKBEDBTDDEDSSZDSZZESDEDSSSSS",
];

/// The player's archetype from the four cards (0x556440 / 0xC75E60 /
/// 0x1005C20). Four reds -> Warrior, two red + two blue -> Scientist, ...;
/// no cards at all -> Wanderer.
pub fn archetype_from_cards(cards: StageCards) -> SpaceArchetype {
    let creature = CardColor::table_value(cards.creature);
    let cell = CardColor::table_value(cards.cell);
    let tribe = CardColor::table_value(cards.tribe);
    let civ = CardColor::table_value(cards.civ);
    let index = creature + 4 * cell + 16 * tribe + 64 * civ;
    let letter = ARCHETYPE_TABLE[index / 64].as_bytes()[index % 64];
    match letter {
        b'W' => SpaceArchetype::Warrior,
        b'T' => SpaceArchetype::Trader,
        b'S' => SpaceArchetype::Shaman,
        b'C' => SpaceArchetype::Scientist,
        b'Z' => SpaceArchetype::Zealot,
        b'D' => SpaceArchetype::Diplomat,
        b'K' => SpaceArchetype::Knight,
        b'B' => SpaceArchetype::Bard,
        b'E' => SpaceArchetype::Ecologist,
        _ => SpaceArchetype::Wanderer,
    }
}

/// Superpower flag bits (`cSimulatorSpaceGame + 0xC4`, built in 0x10056A0).
pub mod superpower_bit {
    pub const CELL_CARNIVORE: u32 = 0x1;
    pub const CELL_HERBIVORE: u32 = 0x2;
    pub const CELL_OMNIVORE: u32 = 0x4;
    pub const CREATURE_ATTACK: u32 = 0x8;
    pub const CREATURE_SOCIAL: u32 = 0x10;
    pub const CREATURE_MIX: u32 = 0x20;
    pub const TRIBE_ATTACK: u32 = 0x40;
    pub const TRIBE_SOCIAL: u32 = 0x80;
    pub const TRIBE_MIX: u32 = 0x100;
    pub const CIV_MILITARY: u32 = 0x200;
    pub const CIV_ECONOMIC: u32 = 0x400;
    pub const CIV_RELIGIOUS: u32 = 0x800;
}

/// The superpower flags the four cards set (0x10056A0).
pub fn superpower_flags(cards: StageCards) -> u32 {
    use superpower_bit::*;
    let pick = |card: Option<CardColor>, red: u32, blue: u32, green: u32| match card {
        Some(CardColor::Red) => red,
        Some(CardColor::Blue) => blue,
        Some(CardColor::Green) => green,
        None => 0,
    };
    pick(cards.cell, CELL_CARNIVORE, CELL_OMNIVORE, CELL_HERBIVORE)
        | pick(cards.creature, CREATURE_ATTACK, CREATURE_MIX, CREATURE_SOCIAL)
        | pick(cards.tribe, TRIBE_ATTACK, TRIBE_MIX, TRIBE_SOCIAL)
        | pick(cards.civ, CIV_MILITARY, CIV_ECONOMIC, CIV_RELIGIOUS)
}

/// Superpower multipliers (tuning `gametuning~!0x8DA597E4`, read via 0x1005B00).
pub const SUPERPOWER_RAID_FREQUENCY_MULTIPLIER: f32 = 3.0;
pub const SUPERPOWER_HEALTH_MULTIPLIER: f32 = 1.5;
pub const SUPERPOWER_SPICE_INCREASE_MULTIPLIER: f32 = 1.5;
pub const SUPERPOWER_BIO_DISASTER_FREQUENCY_MULTIPLIER: f32 = 3.0;
pub const SUPERPOWER_SPEED_MULTIPLIER: f32 = 2.0;
pub const SUPERPOWER_ENERGY_MULTIPLIER: f32 = 1.5;
pub const SUPERPOWER_TOOL_COST_MULTIPLIER: f32 = 0.8;
/// Tool categories discounted x0.8 by the herbivore (1), omnivore (2), tribe
/// attack (6) and tribe mix (8) cards; the category ids were not mapped to tool
/// classes (S8).
pub const TOOL_COST_CATEGORY_BY_BIT: [(u32, u32); 4] = [
    (superpower_bit::CELL_HERBIVORE, 1),
    (superpower_bit::CELL_OMNIVORE, 2),
    (superpower_bit::TRIBE_ATTACK, 6),
    (superpower_bit::TRIBE_MIX, 8),
];

/// Flight speed multiplier from the creature-mix card (x2).
pub fn speed_multiplier(flags: u32) -> f32 {
    if flags & superpower_bit::CREATURE_MIX != 0 {
        SUPERPOWER_SPEED_MULTIPLIER
    } else {
        1.0
    }
}

/// A Space archetype's superpower tool (one per archetype, from 0x10056A0 and
/// the `spacetools~` rows: recharge `RechargeRate`, relationship deltas
/// `toolRelationshipDelta` / `toolGalacticRelationshipDelta`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuperpowerTool {
    pub id: &'static str,
    /// English in-game name (retail text; the localized strings are not read).
    pub name: &'static str,
    pub recharge_seconds: f32,
    pub relationship_delta: f32,
    pub galactic_relationship_delta: f32,
}

/// The archetype's superpower; the Wanderer has none.
pub fn archetype_superpower(archetype: SpaceArchetype) -> Option<SuperpowerTool> {
    let tool = |id, name, recharge_seconds, relationship_delta, galactic_relationship_delta| {
        Some(SuperpowerTool {
            id,
            name,
            recharge_seconds,
            relationship_delta,
            galactic_relationship_delta,
        })
    };
    match archetype {
        SpaceArchetype::Bard => tool("soothingsong", "Soothing Song", 600.0, 0.0, 0.0),
        SpaceArchetype::Diplomat => tool("peacebomb", "Static Cling", 1200.0, 0.0, 0.0),
        SpaceArchetype::Ecologist => tool("safarivacuum", "Safari Vacuum", 1200.0, 0.0, 0.0),
        SpaceArchetype::Knight => tool("callforbackup", "Summon Mini-U", 120.0, 0.0, 0.0),
        SpaceArchetype::Scientist => {
            tool("gravitationwave", "Gravitation Wave", 1200.0, -20.0, -4.0)
        }
        SpaceArchetype::Shaman => tool("portal", "Return Ticket", 60.0, 0.0, 0.0),
        SpaceArchetype::Trader => tool("cashinfusion", "Cash Infusion", 1200.0, 0.0, 0.0),
        SpaceArchetype::Warrior => tool("raiderrally", "Raider Rally", 1200.0, 0.0, 0.0),
        SpaceArchetype::Zealot => {
            tool("planetconversion", "Fanatical Frenzy", 2400.0, -10.0, -7.0)
        }
        SpaceArchetype::Wanderer => None,
    }
}

// ================================================================ S1 tutorial

/// The tutorial chain: Flight 101 (4 sub-missions on the home planet), Story
/// 201 (the closest star), then the "expand" teaching missions the
/// MissionManager offers (`missionManagerTeachingExpandMissions`).
/// ADAPTED: the order of the teaching missions after Story 201 and the fact
/// that they are offered one at a time by the homeworld (not traced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TutorialStep {
    #[default]
    Flight101VisitCities,
    Flight101Scan,
    Flight101FetchAnimal,
    Flight101EradicateAnimals,
    Story201,
    TeachColonize,
    TeachTerraform,
    TeachTradeSpice,
    Done,
}

impl TutorialStep {
    pub const ORDER: [TutorialStep; 9] = [
        TutorialStep::Flight101VisitCities,
        TutorialStep::Flight101Scan,
        TutorialStep::Flight101FetchAnimal,
        TutorialStep::Flight101EradicateAnimals,
        TutorialStep::Story201,
        TutorialStep::TeachColonize,
        TutorialStep::TeachTerraform,
        TutorialStep::TeachTradeSpice,
        TutorialStep::Done,
    ];

    /// The retail mission id (`spaceevents~` instance name).
    pub fn mission_id(self) -> &'static str {
        match self {
            TutorialStep::Flight101VisitCities => "TutorialFlight101_VisitCities",
            TutorialStep::Flight101Scan => "TutorialFlight101_Scan",
            TutorialStep::Flight101FetchAnimal => "TutorialFlight101_FetchAnimal",
            TutorialStep::Flight101EradicateAnimals => "TutorialFlight101_EradicateAnimals",
            TutorialStep::Story201 => "TutorialStory201",
            TutorialStep::TeachColonize => "TutorialColonize",
            TutorialStep::TeachTerraform => "TutorialTerraform",
            TutorialStep::TeachTradeSpice => "TutorialTradespice",
            TutorialStep::Done => "",
        }
    }

    /// Short player-facing name.
    pub fn title(self) -> &'static str {
        match self {
            TutorialStep::Flight101VisitCities => "Flight 101: Visit your cities",
            TutorialStep::Flight101Scan => "Flight 101: Scan",
            TutorialStep::Flight101FetchAnimal => "Flight 101: Fetch an animal",
            TutorialStep::Flight101EradicateAnimals => "Flight 101: Eradicate",
            TutorialStep::Story201 => "Story 201: The closest star",
            TutorialStep::TeachColonize => "Colonize a planet",
            TutorialStep::TeachTerraform => "Terraform a planet",
            TutorialStep::TeachTradeSpice => "Trade spice",
            TutorialStep::Done => "Free exploration",
        }
    }

    pub fn is_flight101(self) -> bool {
        matches!(
            self,
            TutorialStep::Flight101VisitCities
                | TutorialStep::Flight101Scan
                | TutorialStep::Flight101FetchAnimal
                | TutorialStep::Flight101EradicateAnimals
        )
    }

    pub fn next(self) -> TutorialStep {
        let i = Self::ORDER.iter().position(|s| *s == self).unwrap_or(8);
        Self::ORDER[(i + 1).min(8)]
    }

    /// Objectives the step needs (`missionNumCitiesToVisit` 7,
    /// `missionEradicateNumKillsNeeded` 5, one fetch, one scan ...).
    /// ADAPTED: OpenSpore's solar view has no city hoops, so "visit cities"
    /// is one visit to the homeworld; the eradicate kills count hostile ships
    /// (the system has no diseased creatures to shoot).
    pub fn target(self) -> u32 {
        match self {
            TutorialStep::Flight101VisitCities => FLIGHT101_CITY_VISITS,
            TutorialStep::Flight101EradicateAnimals => FLIGHT101_ERADICATE_KILLS,
            TutorialStep::Story201 => STORY201_SCANS,
            TutorialStep::Done => 0,
            _ => 1,
        }
    }

    /// Money paid on completion (`missionGenericRewardMoney`).
    pub fn reward_money(self) -> f32 {
        match self {
            TutorialStep::TeachColonize | TutorialStep::TeachTerraform => 10_000.0,
            TutorialStep::TeachTradeSpice => 5_000.0,
            _ => 0.0,
        }
    }

    /// Tools handed over when the step starts (`missionGiveOnAcceptTools`,
    /// `missionUnlockToolsSpecial`).
    pub fn tools_on_accept(self) -> &'static [&'static str] {
        match self {
            TutorialStep::Flight101Scan => &[tool::SCAN],
            TutorialStep::Flight101FetchAnimal => &[tool::DROP_CARGO, tool::ABDUCT],
            TutorialStep::Flight101EradicateAnimals => &[tool::LASER_1],
            TutorialStep::Story201 => STORY201_UNLOCK_TOOLS,
            _ => &[],
        }
    }
}

/// ADAPTED: one homeworld visit stands for `missionNumCitiesToVisit` 7.
pub const FLIGHT101_CITY_VISITS: u32 = 1;
/// `missionNumCitiesToVisit` of TutorialFlight101_VisitCities (tuning).
pub const RETAIL_FLIGHT101_CITIES: u32 = 7;
/// `missionEradicateNumKillsNeeded` of TutorialFlight101_EradicateAnimals.
pub const FLIGHT101_ERADICATE_KILLS: u32 = 5;
/// `missionDropOffRadius` of the Flight 101 fetch.
pub const FLIGHT101_DROP_OFF_RADIUS: f32 = 75.0;
/// Flight 101 relationship reward (`missionGenericRewardRelation`).
pub const FLIGHT101_REWARD_RELATION: f32 = 4.0;
/// Story 201 range (`missionRangeMax`, galaxy units; picks the closest star).
pub const STORY201_RANGE: f32 = 6.0;
/// Story 201's two scans (0xC602B0, VERIFIED): the UFO wreck (scan object
/// 0x611A05F on the wreck planet, mission state 1 -> 2) and then the ruin
/// (0x611A05E on the mission's target planet, state 3 -> reward).
/// ADAPTED: the wreck is the first world scanned at the target star, the ruin
/// any other world of that star (OpenSpore places no wreck / ruin objects).
pub const STORY201_SCANS: u32 = 2;

/// Tutorial progress (persisted with the campaign).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TutorialState {
    #[serde(default)]
    pub step: TutorialStep,
    /// Objectives done inside the current step.
    #[serde(default)]
    pub progress: u32,
    /// Story 201's target star (galaxy index), once picked.
    #[serde(default)]
    pub story_star: Option<u32>,
    /// Story 201: the world whose scan found the UFO wreck (the ruin is then
    /// scanned on another world of the star).
    #[serde(default)]
    pub story_wreck: Option<u32>,
    /// TutorialColonize's target planet (cMission +0x1E8, picked when the
    /// mission starts; `space::colonize_mission_target`).
    #[serde(default)]
    pub colonize_target: Option<u32>,
    /// The "once" bits of the Space tutorial manager (`space_hud::tutorial::flag`;
    /// retail keeps them as profile flags, ADAPTED: per campaign).
    #[serde(default)]
    pub ui_flags: u32,
    /// The forced hint chain's state (`space_hud::tutorial::Chain`, retail
    /// `SpaceGameUI +0x618`).
    #[serde(default)]
    pub forced: u8,
}

impl TutorialState {
    pub fn remaining(&self) -> u32 {
        self.step.target().saturating_sub(self.progress)
    }
}

// ================================================================ S1 missions

/// Procedural mission types in the generator's order (S1E; the order indexes
/// `missionGeneratorWeight`), plus raid-ally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MissionKind {
    FetchArtifact,
    EradicateAnimals,
    EradicateCityCitizens,
    War,
    SampleCollection,
    Terraform,
    MultiDelivery,
    FetchCommodity,
    DestroyTurrets,
    FetchPlant,
    FetchAnimal,
    Balance,
    FetchTribeCitizen,
    FetchCivCitizen,
    FetchEmpireCitizen,
    ScanFetch,
    RaidAlly,
}

/// The generator order (index = position in `missionGeneratorWeight`).
pub const GENERATOR_ORDER: [MissionKind; 16] = [
    MissionKind::FetchArtifact,
    MissionKind::EradicateAnimals,
    MissionKind::EradicateCityCitizens,
    MissionKind::War,
    MissionKind::SampleCollection,
    MissionKind::Terraform,
    MissionKind::MultiDelivery,
    MissionKind::FetchCommodity,
    MissionKind::DestroyTurrets,
    MissionKind::FetchPlant,
    MissionKind::FetchAnimal,
    MissionKind::Balance,
    MissionKind::FetchTribeCitizen,
    MissionKind::FetchCivCitizen,
    MissionKind::FetchEmpireCitizen,
    MissionKind::ScanFetch,
];

/// One mission row of `spaceevents~` (tuning): generic reward money (all four
/// giver personalities pay the same), duration (0 = no limit), max range in
/// galaxy units, generic reward relation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MissionRow {
    pub id: &'static str,
    pub money: f32,
    pub duration: f32,
    pub range_max: f32,
    pub relation: f32,
    /// `missionTimerStartsOnFirstEntry`.
    pub timer_on_first_entry: bool,
}

pub fn mission_row(kind: MissionKind) -> MissionRow {
    let row = |id, money, duration, range_max, relation, timer_on_first_entry| MissionRow {
        id,
        money,
        duration,
        range_max,
        relation,
        timer_on_first_entry,
    };
    match kind {
        MissionKind::FetchArtifact => row("fetchartifact", 20_000.0, 180.0, 6.0, 3.0, false),
        MissionKind::EradicateAnimals => row("eradicateanimals", 25_000.0, 90.0, 15.0, 3.0, true),
        MissionKind::EradicateCityCitizens => {
            row("eradicatecitycitizens", 30_000.0, 90.0, 15.0, 3.0, false)
        }
        MissionKind::War => row("war", 200_000.0, 600.0, 15.0, 4.0, true),
        MissionKind::SampleCollection => row("samplecollection", 20_000.0, 90.0, 15.0, 3.0, false),
        MissionKind::Terraform => row("terraform", 400_000.0, 0.0, 15.0, 4.0, false),
        MissionKind::MultiDelivery => row("multidelivery", 40_000.0, 0.0, 15.0, 3.0, false),
        MissionKind::FetchCommodity => row("fetchcommodity", 25_000.0, 300.0, 15.0, 3.0, false),
        MissionKind::DestroyTurrets => row("destroyturrets", 50_000.0, 300.0, 15.0, 4.0, false),
        MissionKind::FetchPlant => row("fetchplant", 15_000.0, 180.0, 6.0, 3.0, false),
        MissionKind::FetchAnimal => row("fetchanimal", 15_000.0, 180.0, 6.0, 3.0, true),
        MissionKind::Balance => row("balance", 65_000.0, 0.0, 15.0, 3.0, false),
        MissionKind::FetchTribeCitizen => {
            row("fetchtribecitizen", 20_000.0, 180.0, 15.0, 3.0, false)
        }
        MissionKind::FetchCivCitizen => row("fetchcivcitizen", 20_000.0, 180.0, 15.0, 3.0, false),
        MissionKind::FetchEmpireCitizen => {
            row("fetchempirecitizen", 30_000.0, 180.0, 15.0, 3.0, false)
        }
        MissionKind::ScanFetch => row("scanfetch_fetch", 40_000.0, 0.0, 15.0, 3.0, false),
        MissionKind::RaidAlly => row("raidally", 35_000.0, 0.0, 2.5, 0.0, false),
    }
}

/// `missionRewardEmpireMultiplier` by the giver's empire-size tier.
pub const MISSION_REWARD_EMPIRE_MULTIPLIER: [f32; 6] = [1.0, 1.5, 2.0, 3.0, 4.0, 5.0];
/// `missionDurationEmpireMultiplier` by the giver's empire-size tier.
pub const MISSION_DURATION_EMPIRE_MULTIPLIER: [f32; 6] = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5];
/// `missionRejectPunishmentRelation`, `missionAbortPunishmentRelation`,
/// `missionFailPunishmentRelation`.
pub const MISSION_REJECT_RELATION: f32 = -0.5;
pub const MISSION_ABORT_RELATION: f32 = -1.2;
pub const MISSION_FAIL_RELATION: f32 = -1.5;
/// MissionManager (0xE8E2F4B2).
pub const MISSION_MANAGER_MAX_MISSIONS: usize = 40;
pub const MISSION_GAP_NPC_SECONDS: f32 = 120.0;
pub const MISSION_GAP_COLONY_SECONDS: f32 = 60.0;
pub const MISSION_MIN_EMPIRE_SIZE_FOR_ARCHETYPE: u32 = 4;

/// Money paid for a mission: generic reward x empire-size multiplier x rank
/// multiplier x `difficultySpaceMissionRewardCash`.
pub fn mission_reward(kind: MissionKind, empire_tier: usize, rank: u8, difficulty: Difficulty) -> f32 {
    mission_row(kind).money
        * MISSION_REWARD_EMPIRE_MULTIPLIER[empire_tier.min(5)]
        * rank_reward_multiplier(rank)
        * difficulty.pick(DIFFICULTY_MISSION_REWARD_CASH)
}

/// Time limit in seconds (None = unlimited): duration x empire-size multiplier
/// x rank multiplier x `difficultySpaceMissionTime`.
pub fn mission_duration(
    kind: MissionKind,
    empire_tier: usize,
    rank: u8,
    difficulty: Difficulty,
) -> Option<f32> {
    let base = mission_row(kind).duration;
    (base > 0.0).then(|| {
        base * MISSION_DURATION_EMPIRE_MULTIPLIER[empire_tier.min(5)]
            * rank_duration_multiplier(rank)
            * difficulty.pick(DIFFICULTY_MISSION_TIME)
    })
}

/// The score of a planet of tech level `tech` as the target of a fetch mission
/// (`None` = the -1 of the scoring; `cMission` fetch scoring 0xC523E0..0xC52695,
/// VERIFIED): the tribe / civilization / empire citizen missions
/// (`missionTypeOfFetch` TribeCitizen 0xFB3B1635, CivCitizen 0x3E994B2B,
/// EmpireCitizen 0x69412983) take a planet of tech level 2 / 4 / 5 (score 1.0);
/// the artifact mission (0x19EC2C8B) any planet, 1.2 for an empire's, else 1.0.
/// Other kinds are not fetch missions of the port.
pub fn fetch_target_score(kind: MissionKind, tech: crate::space_worldgen::TechLevel) -> Option<f32> {
    use crate::space_worldgen::TechLevel as T;
    match kind {
        MissionKind::FetchTribeCitizen => (tech == T::Tribe).then_some(1.0),
        MissionKind::FetchCivCitizen => (tech == T::Civilization).then_some(1.0),
        MissionKind::FetchEmpireCitizen => (tech == T::Empire).then_some(1.0),
        MissionKind::FetchArtifact => (tech != T::None).then_some(if tech == T::Empire { 1.2 } else { 1.0 }),
        _ => None,
    }
}

/// The score of a planet as the target of the scored mission kinds the port
/// plays besides the fetch kinds above (`None` = the -1 of the scoring):
/// * fetch commodity (0xC523E0 calls 0xC70400: the planet holds a commodity
///   node; Initialize 0xC544A0 asks for an Empire-tech planet) and the buyer of
///   a multi-delivery (0xC57040 keeps buyers of tech 5): ADAPTED, an Empire
///   planet stands for the commodity node;
/// * eradicate animals (0xC4F210, type Animals): the planet's animal list is not
///   empty and it has no sanctuary, 1.0; ADAPTED: tech above none stands for the
///   animal list (the port keeps no species list of foreign planets);
/// * scan-fetch (`scanfetch_fetch`, like the sample fetch 0xC524ED): the bullseye
///   T-score is 1 with animals present, 1.2 for tech 5 else 1.0.
pub fn kind_target_score(kind: MissionKind, tech: crate::space_worldgen::TechLevel, t_score: u8) -> Option<f32> {
    use crate::space_worldgen::TechLevel as T;
    match kind {
        MissionKind::FetchCommodity | MissionKind::MultiDelivery => (tech == T::Empire).then_some(1.0),
        MissionKind::EradicateAnimals => (tech != T::None).then_some(1.0),
        MissionKind::ScanFetch => (tech != T::None && t_score == 1).then_some(if tech == T::Empire { 1.2 } else { 1.0 }),
        _ => fetch_target_score(kind, tech),
    }
}

/// `missionEradicateNumCreatures` of `eradicateanimals` (the creatures to
/// destroy); `missionEradicateMaxNumHealthyKills` is 10 (a healthy animal
/// killed; the port has none).
pub const ERADICATE_NUM_CREATURES: u32 = 5;
/// `missionNumOfRowsRequired` (0xF924EBF8) of `balance`: the T-score level the
/// planet must reach (0xC4B8D0 compares 0xBBCFB0 with it).
pub const BALANCE_ROWS_REQUIRED: u32 = 1;
/// `missionMultiDeliveryMinNumItems` / `MaxNumItems` of `multidelivery`.
pub const MULTI_DELIVERY_ITEMS: u32 = 3;

/// The eight `missionFetchItem` objects of `fetchartifact` (the keys name them:
/// goldenidol, chest, crown, gnome, grail, llama, plate, scepter).
pub const FETCH_ARTIFACT_ITEMS: [&str; 8] =
    ["golden idol", "chest", "crown", "gnome", "grail", "llama", "plate", "scepter"];

/// The `fetchartifact` object of the target world (ADAPTED: retail picks one of
/// the eight when the mission is made; the port derives it from the world).
pub fn fetch_artifact_item(world_id: u32) -> &'static str {
    FETCH_ARTIFACT_ITEMS[(world_id as usize) % FETCH_ARTIFACT_ITEMS.len()]
}

/// What a fetch mission asks for, in words.
pub fn fetch_item_text(kind: MissionKind) -> &'static str {
    match kind {
        MissionKind::FetchTribeCitizen => "a tribe citizen",
        MissionKind::FetchCivCitizen => "a civilization citizen",
        MissionKind::FetchEmpireCitizen => "an empire citizen",
        _ => "an artifact",
    }
}

/// Weighted pick from 16 generator weights; `roll` in 0..1. Falls back to the
/// first kind with a weight when every weight is zero.
pub fn pick_mission_kind(weights: &[u32; 16], roll: f32) -> MissionKind {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return GENERATOR_ORDER[0];
    }
    let mut target = (roll.clamp(0.0, 0.999_999) * total as f32) as u32;
    for (kind, weight) in GENERATOR_ORDER.iter().zip(weights) {
        if target < *weight {
            return *kind;
        }
        target -= weight;
    }
    GENERATOR_ORDER[15]
}

// ======================================================= S1 badges and ranks

/// The counters retail badge requirements read: the requirement property of
/// each badge (`badgeRequirementList` 0x02CC641B names it; VERIFIED in every
/// one of the 144 `space_badges~` 0x02CB089D files, see
/// `notes\space-endgame-notes.md` E2) and the `cBadgeManager` event that adds
/// to it (`BadgeManagerEvent`, AddToBadgeProgress 0xFE52C0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BadgeCounter {
    ArchetypeSwitches,
    EradicateMissions,
    RaresCollected,
    PlanetsColonized,
    AlliancesFormed,
    EcoDisasterMissions,
    StarsExplored,
    FetchMissions,
    TradeVolume,
    MissionsComplete,
    PlanetsTerraformed,
    EmpiresMet,
    WarsStarted,
    FoodWebsComplete,
    /// `badgeFriendsDefended` (event 0x12, 0xC5BB60: a raid on a world of the
    /// player or an ally ended well).
    FriendsDefended,
    /// `badgePlanetsconquered` (event 0x1B, 0x102B6A0).
    PlanetsConquered,
    /// `badgeSightseer` (event 0x15, 0xFDD590: the first visit of a special
    /// star: the core, a black hole, a proto-planetary disk or a binary).
    Sightseer,
    /// `badgeCreaturespromoted` (event 0x14, 0x100A000: a species uplifted).
    CreaturesPromoted,
    /// `badgeSystemspurchased` (event 0x1A, 0x102D1C0: a system bought).
    SystemsPurchased,
    /// `badgeEmpiresize` (stars of the player's empire).
    EmpireSize,
    /// `badgeFlightsmade` (event 0x16, 0x1002040: every interstellar jump).
    FlightsMade,
    /// `badgeReachedCash` (Sporebucks held).
    ReachedCash,
    /// `badgeToolspurchased` (event 0x1C, 0x103F000: every store purchase).
    ToolsPurchased,
    /// `badgeAestheticTools` (event 0x17, cToolStrategy::OnHit 0x10593B0:
    /// colour / sculpt tool uses).
    AestheticTools,
    /// `badgeTraderoutes` (event 0x19, 0x102C990 / 0x102DF30: routes made).
    TradeRoutes,
    /// `badgeStorybooksvisited` (event 0x18: Galactic Adventures; OpenSpore
    /// has none, the counter stays 0).
    StorybooksVisited,
}

/// A five-level badge family (`space_badges~`): requirement thresholds and
/// the badge points (`badgePointValue` 0x043333DD) each level is worth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeFamily {
    pub id: &'static str,
    pub counter: BadgeCounter,
    pub thresholds: [u64; 5],
    pub points: [u32; 5],
    /// Display name without the level (`badgeName`, `spacegame_badges`
    /// 0x4CFB825A).
    pub name: &'static str,
}

const P2: [u32; 5] = [2, 4, 6, 8, 10];
const P_BIG: [u32; 5] = [2, 4, 7, 12, 20];

/// Every badge family of the data (VERIFIED requirement values and points).
pub const BADGE_FAMILIES: [BadgeFamily; 26] = [
    BadgeFamily { id: "archetypeswitch", counter: BadgeCounter::ArchetypeSwitches, thresholds: [1, 2, 4, 5, 6], points: [5, 10, 15, 20, 25], name: "Split Personality" },
    BadgeFamily { id: "bodyguard", counter: BadgeCounter::FriendsDefended, thresholds: [2, 5, 10, 20, 35], points: P2, name: "Body Guard" },
    BadgeFamily { id: "brainsurgeon", counter: BadgeCounter::CreaturesPromoted, thresholds: [1, 5, 10, 20, 40], points: P2, name: "Brain Surgeon" },
    BadgeFamily { id: "cleaner", counter: BadgeCounter::EradicateMissions, thresholds: [5, 10, 20, 40, 70], points: P2, name: "Cleaner" },
    BadgeFamily { id: "collector", counter: BadgeCounter::RaresCollected, thresholds: [3, 8, 20, 50, 100], points: P2, name: "Collector" },
    BadgeFamily { id: "colonist", counter: BadgeCounter::PlanetsColonized, thresholds: [5, 20, 50, 100, 200], points: P_BIG, name: "Colonist" },
    BadgeFamily { id: "conquerer", counter: BadgeCounter::PlanetsConquered, thresholds: [2, 5, 10, 20, 50], points: P_BIG, name: "Conquerer" },
    BadgeFamily { id: "diplomat", counter: BadgeCounter::AlliancesFormed, thresholds: [1, 2, 5, 10, 20], points: P2, name: "Diplomat" },
    BadgeFamily { id: "ecohero", counter: BadgeCounter::EcoDisasterMissions, thresholds: [2, 5, 10, 25, 50], points: P2, name: "Eco Hero" },
    BadgeFamily { id: "economist", counter: BadgeCounter::SystemsPurchased, thresholds: [1, 3, 6, 10, 15], points: P_BIG, name: "Economist" },
    BadgeFamily { id: "empire", counter: BadgeCounter::EmpireSize, thresholds: [3, 7, 13, 22, 35], points: P2, name: "Empire" },
    BadgeFamily { id: "explorer", counter: BadgeCounter::StarsExplored, thresholds: [15, 50, 100, 250, 500], points: P2, name: "Explorer" },
    BadgeFamily { id: "frequentflyer", counter: BadgeCounter::FlightsMade, thresholds: [50, 150, 400, 800, 1500], points: P2, name: "Frequent Flyer" },
    BadgeFamily { id: "goldentouch", counter: BadgeCounter::ReachedCash, thresholds: [500_000, 1_000_000, 2_500_000, 5_000_000, 10_000_000], points: P2, name: "Golden Touch" },
    BadgeFamily { id: "gopher", counter: BadgeCounter::FetchMissions, thresholds: [5, 10, 20, 40, 70], points: P2, name: "Gopher" },
    BadgeFamily { id: "jackofalltrades", counter: BadgeCounter::ToolsPurchased, thresholds: [5, 20, 50, 100, 200], points: P2, name: "Jack Of All Trades" },
    BadgeFamily { id: "merchant", counter: BadgeCounter::TradeVolume, thresholds: [500_000, 2_000_000, 4_000_000, 7_000_000, 15_000_000], points: P2, name: "Merchant" },
    BadgeFamily { id: "missionista", counter: BadgeCounter::MissionsComplete, thresholds: [5, 10, 18, 30, 50], points: P2, name: "Missionista" },
    BadgeFamily { id: "planetpicasso", counter: BadgeCounter::AestheticTools, thresholds: [5, 25, 75, 150, 250], points: P2, name: "Planet Artiste" },
    BadgeFamily { id: "sightseer", counter: BadgeCounter::Sightseer, thresholds: [2, 10, 20, 40, 70], points: P2, name: "Sightseer" },
    BadgeFamily { id: "terrawrangler", counter: BadgeCounter::PlanetsTerraformed, thresholds: [2, 5, 10, 20, 40], points: [2, 3, 6, 10, 15], name: "Terra-Wrangler" },
    BadgeFamily { id: "trader", counter: BadgeCounter::TradeRoutes, thresholds: [2, 5, 10, 20, 40], points: P2, name: "Trader" },
    BadgeFamily { id: "traveler", counter: BadgeCounter::EmpiresMet, thresholds: [3, 10, 20, 50, 100], points: [1, 2, 3, 4, 5], name: "Traveler" },
    BadgeFamily { id: "warmonger", counter: BadgeCounter::WarsStarted, thresholds: [1, 3, 6, 10, 15], points: [1, 2, 4, 6, 10], name: "Warmonger" },
    BadgeFamily { id: "wonderlandwanderer", counter: BadgeCounter::StorybooksVisited, thresholds: [1, 2, 4, 7, 12], points: P2, name: "Wonderland Wanderer" },
    BadgeFamily { id: "zoologist", counter: BadgeCounter::FoodWebsComplete, thresholds: [3, 10, 20, 40, 70], points: [1, 3, 5, 7, 15], name: "Zoologist" },
];

/// The `captain` badge (1 point): VERIFIED requirement `badgeFlight101Complete`
/// 1 (event 0x11), so it is earned by finishing Flight 101, not held from the
/// start. Earning it posts message 0x064E43B9 (0xFE58B0).
pub const CAPTAIN_BADGE_POINTS: u32 = 1;

pub fn badge_family(id: &str) -> Option<&'static BadgeFamily> {
    BADGE_FAMILIES.iter().find(|f| f.id == id)
}

/// Levels earned (0..=5) for a counter value.
pub fn badge_level(family: &BadgeFamily, count: u64) -> u8 {
    family.thresholds.iter().filter(|t| count >= **t).count() as u8
}

/// Badge points of a family at `level`: every earned badge adds its points
/// (`badgeReqBadgePointsEarned` accumulates per badge earned).
pub fn badge_points(family: &BadgeFamily, level: u8) -> u32 {
    family.points.iter().take(level.min(5) as usize).sum()
}

/// `badgecollector-1..10`: badge points needed for each promotion.
pub const RANK_BADGE_POINTS: [u32; 10] = [5, 15, 30, 50, 75, 105, 140, 180, 225, 275];
/// Title before any promotion, then the ten ranks.
pub const RANK_TITLES: [&str; 11] = [
    "Captain",
    "Commander",
    "Commodore",
    "Admiral",
    "The Celebrated",
    "The Renowned",
    "The Great",
    "The Legendary",
    "All-Powerful",
    "Ultimate Being",
    "Omnipotent",
];
/// Mission reward multiplier of each rank badge (`rewardx`).
pub const RANK_REWARD_MULTIPLIER: [f32; 10] = [1.05, 1.1, 1.2, 1.4, 1.7, 2.0, 2.5, 3.1, 3.9, 5.0];
/// Mission duration multiplier of each rank badge (`durx`).
pub const RANK_DURATION_MULTIPLIER: [f32; 10] = [0.99, 0.98, 0.95, 0.9, 0.84, 0.77, 0.7, 0.64, 0.57, 0.5];

/// Promotions earned (0 = Captain .. 10 = Omnipotent).
pub fn rank_from_points(points: u32) -> u8 {
    RANK_BADGE_POINTS.iter().filter(|p| points >= **p).count() as u8
}

pub fn rank_title(rank: u8) -> &'static str {
    RANK_TITLES[(rank as usize).min(10)]
}

/// The current rank's mission reward multiplier (1 before any promotion).
pub fn rank_reward_multiplier(rank: u8) -> f32 {
    match rank {
        0 => 1.0,
        r => RANK_REWARD_MULTIPLIER[(r as usize - 1).min(9)],
    }
}

pub fn rank_duration_multiplier(rank: u8) -> f32 {
    match rank {
        0 => 1.0,
        r => RANK_DURATION_MULTIPLIER[(r as usize - 1).min(9)],
    }
}

/// Fraction of the way from the current rank to the next (the shared
/// progress bar, 0xFE6640); 1.0 at the last rank.
pub fn rank_progress(points: u32) -> f32 {
    let rank = rank_from_points(points) as usize;
    if rank >= 10 {
        return 1.0;
    }
    let low = if rank == 0 { 0 } else { RANK_BADGE_POINTS[rank - 1] };
    let high = RANK_BADGE_POINTS[rank];
    (points.saturating_sub(low) as f32 / (high - low) as f32).clamp(0.0, 1.0)
}

/// Escort ships the player may command: VERIFIED `cPlayer::mMaxPosseSize`
/// (+0x124C) starts at 0 (0xC7B930) and every promotion badge with
/// `badgeEarnPosse` (badgecollector-2, -4, -6, -8, -10) adds one, capped at 5
/// (0xC75760); so one per two ranks.
pub fn posse_limit(rank: u8) -> usize {
    (rank as usize / 2).min(5)
}

/// `SPG_MasterBadge`'s `textFromCode "MasterBadgeText"` (key 0x6A9C6745):
/// the rank's line in `cinematics_spg` 0x974299BE (0xFE3D20, VERIFIED).
pub const MASTER_BADGE_TEXT_KEY: &str = "MasterBadgeText";
pub const MASTER_BADGE_TEXT: [u32; 10] =
    [0x063B_B6D0, 0x063B_B6D6, 0x063B_B6DA, 0x063B_B6DF, 0x063B_B6E3, 0x063B_B6E8, 0x063B_B6EE, 0x063B_B6F2, 0x063B_B6F6, 0x063B_B6FA];
/// Played by the badge-earned handler 0xFE58B0 for every promotion badge.
pub const CINEMATIC_MASTER_BADGE: &str = "SPG_MasterBadge";

// ============================================================ S1 core, death

/// One-time galactic-core flag (0x0681C28E) and cinematic (0x1002DD0).
pub const CINEMATIC_CENTER_OF_GALAXY: &str = "SPG_CenterOfGalaxy";
pub const HINT_TRAVEL_NEAR_CORE: &str = "SPG_TravelNearCore";
/// Staff of Life (`genesisdevice`): Steve's gift sets its charges to the
/// tool's `toolMeaningOfLifeRewardCount` 42 (VERIFIED: 0xFF3CA0 on message
/// 0x06526395; `MaxAmmoCount` 99 is the cap, `InitialAmmoCount` 5 is what a
/// bought one starts with).
pub const STAFF_OF_LIFE_CHARGES: u32 = 42;
pub const STAFF_OF_LIFE_MAX_CHARGES: u32 = 99;
pub const STAFF_OF_LIFE_RECHARGE_SECONDS: f32 = 120.0;

/// Arrival cinematics (0xFF5D90).
pub const CINEMATIC_CVG2SPG: &str = "CVG2SPG";
pub const CINEMATIC_CVG2SPG_SHORT: &str = "CVG2SPG_short";
/// Ship destroyed (0xC3C680): by context, before / after a posse exists.
pub const CINEMATIC_DESTROYED_PRE_POSSE_GALAXY: &str = "SPG_PlayerDestroyed_PrePosse_Galaxy";
pub const CINEMATIC_DESTROYED_PRE_POSSE_NOT_GALAXY: &str = "SPG_PlayerDestroyed_PrePosse_NotGalaxy";
pub const CINEMATIC_DESTROYED_POST_POSSE: &str = "SPG_PlayerDestroyed_PostPosse";
/// Story 201 / tutorial cinematics (names only; triggers not traced).
pub const CINEMATIC_STORY201_ENTERED_GALAXY: &str = "SPG_EnteredGalaxy_201";
pub const CINEMATIC_FLIGHT_SCHOOL_COMPLETE: &str = "SPG_FlightSchool101Complete";

/// The destruction cinematic for the context.
pub fn destroyed_cinematic(has_posse: bool, in_galaxy: bool) -> &'static str {
    if has_posse {
        CINEMATIC_DESTROYED_POST_POSSE
    } else if in_galaxy {
        CINEMATIC_DESTROYED_PRE_POSSE_GALAXY
    } else {
        CINEMATIC_DESTROYED_PRE_POSSE_NOT_GALAXY
    }
}

/// NPC ships: respawn and reinforcement timers (SpaceCombat).
pub const NPC_RESPAWN_SECONDS: f32 = 90.0;
pub const NPC_REINFORCE_SECONDS: f32 = 60.0;
pub const NPC_REINFORCE_SECOND_SECONDS: f32 = 120.0;
pub const NPC_REINFORCE_COUNT: u32 = 2;

// ===================================================== S2 health and energy

/// `UFOHealthPlayer` (0xFFD780).
pub const UFO_HEALTH_PLAYER: f32 = 1000.0;
/// Energy per energy-upgrade unit (0xFFD920).
pub const UFO_ENERGY_BASE: f32 = 100.0;
/// `healthupgrade1..5` `MaxRange` (the level L).
pub const HEALTH_UPGRADES: [(&str, f32); 5] = [
    ("healthupgrade1", 1.0),
    ("healthupgrade2", 2.0),
    ("healthupgrade3", 3.0),
    ("healthupgrade4", 4.0),
    ("healthupgrade5", 5.0),
];
/// `energyupgrade1..5` `MaxRange` (the level E).
pub const ENERGY_UPGRADES: [(&str, f32); 5] = [
    ("energyupgrade1", 1.0),
    ("energyupgrade2", 1.5),
    ("energyupgrade3", 2.0),
    ("energyupgrade4", 2.5),
    ("energyupgrade5", 3.0),
];

fn best_level<S: AsRef<str>>(tools: &[S], table: &[(&str, f32)]) -> f32 {
    table
        .iter()
        .filter(|(id, _)| tools.iter().any(|t| t.as_ref() == *id))
        .map(|(_, level)| *level)
        .fold(0.0, f32::max)
}

/// L: MaxRange of the best owned health upgrade; ADAPTED 1 with none (a save
/// without the default loadout).
pub fn health_level<S: AsRef<str>>(tools: &[S]) -> f32 {
    best_level(tools, &HEALTH_UPGRADES).max(1.0)
}

/// E: MaxRange of the best owned energy upgrade; ADAPTED 1 with none.
pub fn energy_level<S: AsRef<str>>(tools: &[S]) -> f32 {
    best_level(tools, &ENERGY_UPGRADES).max(1.0)
}

/// Player UFO maximum health (0xFFD780): 1000 x L x difficulty x 1.5 with the
/// creature-attack card.
pub fn player_max_health(level: f32, difficulty: Difficulty, flags: u32) -> f32 {
    let card = if flags & superpower_bit::CREATURE_ATTACK != 0 {
        SUPERPOWER_HEALTH_MULTIPLIER
    } else {
        1.0
    };
    UFO_HEALTH_PLAYER * level * difficulty.pick(DIFFICULTY_PLAYER_HEALTH) * card
}

/// Player UFO maximum energy (0xFFD920): 100 x E x 1.5 with the cell-carnivore card.
pub fn player_max_energy(level: f32, flags: u32) -> f32 {
    let card = if flags & superpower_bit::CELL_CARNIVORE != 0 {
        SUPERPOWER_ENERGY_MULTIPLIER
    } else {
        1.0
    };
    UFO_ENERGY_BASE * level * card
}

/// `spaceEconomyEnergyRechargeRate` 0 and `...RechargeDelay` 5 s: the code
/// would add max x 1% x dt x rate after the delay; with rate 0 there is no
/// passive recharge.
pub const ENERGY_RECHARGE_RATE: f32 = 0.0;
pub const ENERGY_RECHARGE_DELAY: f32 = 5.0;

pub fn passive_energy_recharge(max_energy: f32, seconds_since_use: f32, dt: f32) -> f32 {
    if seconds_since_use < ENERGY_RECHARGE_DELAY {
        0.0
    } else {
        max_energy * 0.01 * dt * ENERGY_RECHARGE_RATE
    }
}

// ================================================= S2 repair and recharge

/// `spaceEconomyRechargeCost` (energy, Sporebucks per point) by cost index.
pub const RECHARGE_COST: [f32; 6] = [0.0, 10.0, 30.0, 70.0, 150.0, 310.0];
/// `spaceEconomyRepairCost` (health, Sporebucks per point) by cost index.
pub const REPAIR_COST: [f32; 6] = [0.0, 1.0, 3.0, 7.0, 15.0, 31.0];

/// Who owns the planet the player repairs at (0x102F830 cost index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanetOwner {
    Homeworld,
    Colony,
    Ally,
    /// Another empire with a friendly stance (3-4).
    Friendly,
    /// Another empire, neutral (stance 2).
    Neutral,
    /// Another empire, hostile (stance 0-1).
    Hostile,
}

impl PlanetOwner {
    pub fn cost_index(self) -> usize {
        match self {
            PlanetOwner::Homeworld => 0,
            PlanetOwner::Colony => 1,
            PlanetOwner::Ally => 2,
            PlanetOwner::Friendly => 3,
            PlanetOwner::Neutral => 4,
            PlanetOwner::Hostile => 5,
        }
    }
}

/// Buy back up to `missing` points at `price` per point with `money`
/// (0xFFD120 / 0x1001840): cost = ceil(points x price); with too little money
/// only the affordable part is bought. Returns (points, cost).
pub fn repair_purchase(missing: f32, price: f32, money: f32) -> (f32, f32) {
    let missing = missing.max(0.0);
    if missing <= 0.0 {
        return (0.0, 0.0);
    }
    if price <= 0.0 {
        return (missing, 0.0);
    }
    let full = (missing * price).ceil();
    if money >= full {
        return (missing, full);
    }
    let points = (money.max(0.0) / price).floor().min(missing);
    (points, (points * price).ceil())
}

// ========================================================== S2 jumps, drives

/// Interstellar drive ranges (`MaxRange`, galaxy units).
pub const DRIVES: [(&str, f32); 5] = [
    ("interstellardrive", 3.0),
    ("interstellardrive_upgrade1", 5.0),
    ("interstellardrive_upgrade2", 8.0),
    ("interstellardrive_upgrade3", 12.0),
    ("interstellardrive_upgrade4", 20.0),
];

/// Jump range in galaxy units (0 without an interstellar drive).
pub fn drive_range<S: AsRef<str>>(tools: &[S]) -> f32 {
    best_level(tools, &DRIVES)
}

/// Map units per retail galaxy unit: 1. The galaxy map now is the retail
/// galaxy (306,301 stars within 700 units of the core, `space_worldgen`), so
/// drive range 3..20, empire radius 20 and detection radius 5 are all in the
/// map's own units.
pub const MAP_UNITS_PER_GALAXY_UNIT: f32 = 1.0;

/// `spaceEconomyTravelFuelCost`: energy per jump, regardless of distance (0x1002040).
pub const JUMP_ENERGY: f32 = 5.0;
/// `spaceEconomyTravelDamage`: damage (type 5) when jumping with no energy.
pub const EMPTY_JUMP_DAMAGE: f32 = 200.0;
/// `spaceEconomyTravelCost` 0 with alliance discount 0.2 and colony discount 0
/// (0xFFC390): jumps cost no money.
pub const TRAVEL_COST_PER_UNIT: f32 = 0.0;
pub const ALLIANCE_TRAVEL_DISCOUNT: f32 = 0.2;
pub const COLONY_TRAVEL_DISCOUNT: f32 = 0.0;
/// Awareness bump radius on arrival (S2B / S3C).
pub const EMPIRE_DETECTION_RADIUS: f32 = 5.0;
pub const GROX_DETECTION_RADIUS: f32 = 8.0;

/// What one jump costs: (energy spent, damage taken). With energy left the
/// jump spends `JUMP_ENERGY` (at most what is left); an empty reactor jumps
/// anyway and the hull takes `EMPTY_JUMP_DAMAGE`.
pub fn jump_cost(energy: f32) -> (f32, f32) {
    if energy <= 0.0 {
        (0.0, EMPTY_JUMP_DAMAGE)
    } else {
        (JUMP_ENERGY.min(energy), 0.0)
    }
}

/// Money a jump of `distance` galaxy units costs (0 with the retail tuning).
pub fn jump_money_cost(distance: f32, allied_star: bool, colony_star: bool) -> f32 {
    let mut cost = distance.max(0.0) * TRAVEL_COST_PER_UNIT;
    if allied_star {
        cost *= 1.0 - ALLIANCE_TRAVEL_DISCOUNT;
    }
    if colony_star {
        cost *= 1.0 - COLONY_TRAVEL_DISCOUNT;
    }
    cost
}

// =============================================================== S2 weapons

/// One weapon row of `spacetools~`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponStats {
    pub id: &'static str,
    pub damage: f32,
    pub energy: f32,
    pub recharge: f32,
    /// Seconds between auto-fire shots (-1 = single shot).
    pub autofire: f32,
    pub min_range: f32,
    pub max_range: f32,
    pub area: f32,
}

const fn weapon(id: &'static str, damage: f32, energy: f32, recharge: f32, autofire: f32, min_range: f32, max_range: f32, area: f32) -> WeaponStats {
    WeaponStats { id, damage, energy, recharge, autofire, min_range, max_range, area }
}

/// Player weapons (tuning).
pub const PLAYER_WEAPONS: [WeaponStats; 12] = [
    weapon("laserlevel1", 40.0, 0.25, 0.0, 0.1, -1.0, 200.0, 0.5),
    weapon("laserlevel2", 80.0, 0.5, 0.0, 0.1, -1.0, 200.0, 0.5),
    weapon("laserlevel3", 240.0, 0.75, 0.0, 0.1, -1.0, 200.0, 0.5),
    weapon("protonmissilelevel1", 30.0, 0.5, 1.0, 1.0, 10.0, 200.0, 8.0),
    weapon("protonmissilelevel2", 60.0, 1.0, 1.0, 1.0, 10.0, 200.0, 8.0),
    weapon("protonmissilelevel3", 180.0, 1.5, 1.0, 1.0, 10.0, 200.0, 8.0),
    weapon("pulselevel1", 75.0, 0.5, 1.0, 1.0, -1.0, 150.0, 5.0),
    weapon("pulselevel2", 150.0, 1.0, 1.0, 1.0, -1.0, 150.0, 5.0),
    weapon("pulselevel3", 450.0, 1.5, 1.0, 1.0, -1.0, 150.0, 5.0),
    weapon("dumbbomblevel1", 300.0, 0.5, 1.0, 1.0, -1.0, 150.0, 12.0),
    weapon("dumbbomblevel2", 600.0, 1.0, 1.0, 1.0, -1.0, 150.0, 12.0),
    weapon("dumbbomblevel3", 1800.0, 1.5, 1.0, 1.0, -1.0, 150.0, 12.0),
];

/// The weapon row of a retail tool name, if the player can fire it (the
/// lasers, the proton missiles, the pulse weapons and the bombs).
pub fn player_weapon(id: &str) -> Option<WeaponStats> {
    PLAYER_WEAPONS.iter().copied().find(|w| w.id == id)
}

/// The lasers are beams (`DefaultBeam` strategy FC20C32D: damage a rate per
/// second of contact); the other player weapons are `DefaultProjectile`
/// (2FD3EB55: damage per shot, `toolProjectileSpeed` flight).
pub fn is_beam_weapon(id: &str) -> bool {
    id.starts_with("laserlevel")
}

/// Projectile speed of the projectile weapons in retail units per second
/// (`toolProjectileSpeed`: pulse 150, proton missile 150, bombs 40).
pub fn projectile_speed(id: &str) -> f32 {
    if id.starts_with("dumbbomb") {
        40.0
    } else {
        150.0
    }
}

/// Laser level 1..=3.
pub fn laser(level: u8) -> WeaponStats {
    PLAYER_WEAPONS[(level.clamp(1, 3) - 1) as usize]
}

/// The best laser level owned (1 with none: the default loadout has laser 1).
pub fn laser_level<S: AsRef<str>>(tools: &[S]) -> u8 {
    [(tool::LASER_3, 3u8), (tool::LASER_2, 2), (tool::LASER_1, 1)]
        .iter()
        .find(|(id, _)| tools.iter().any(|t| t.as_ref() == *id))
        .map(|(_, l)| *l)
        .unwrap_or(1)
}

/// Shield / cloak (energy 20, recharge 180 / 270 s); potions (25 points,
/// recharge 3 s; mega 100 points, 10 s); scan range 150 (auto-fire 0.4 s),
/// abduct range 200; max lock-on range 500.
pub const SHIELD_ENERGY: f32 = 20.0;
pub const SHIELD_RECHARGE: f32 = 180.0;
pub const CLOAK_RECHARGE: f32 = 270.0;
pub const POTION_POINTS: f32 = 25.0;
pub const MEGA_POTION_POINTS: f32 = 100.0;
pub const SCAN_RANGE: f32 = 150.0;
pub const ABDUCT_RANGE: f32 = 200.0;
pub const MAX_LOCK_ON_RANGE: f32 = 500.0;

/// ADAPTED: retail ranges are in planet-scale units (laser 200, NPC laser 50,
/// lock-on 500); OpenSpore's solar system uses worlds a few units wide, so one
/// OpenSpore unit stands for this many retail units in weapon ranges.
pub const RETAIL_UNITS_PER_WORLD_UNIT: f32 = 6.0;

// ========================================================= S2 NPC ships, loot

/// `UFOHealthFighter` .. by NPC tier (SpaceCombat).
pub const UFO_HEALTH_FIGHTER: [f32; 6] = [50.0, 75.0, 125.0, 200.0, 300.0, 400.0];
pub const UFO_HEALTH_BOMBER: [f32; 6] = [250.0, 375.0, 625.0, 1000.0, 1500.0, 2000.0];
pub const UFO_HEALTH_DEFENDER: [f32; 6] = [300.0, 450.0, 750.0, 1200.0, 1800.0, 2400.0];
pub const UFO_HEALTH_INTERCEPTOR: [f32; 6] = [100.0, 150.0, 250.0, 400.0, 600.0, 800.0];
pub const UFO_HEALTH_ALLY: [f32; 6] = [300.0, 450.0, 750.0, 1200.0, 1800.0, 2400.0];
pub const UFO_HEALTH_PIRATE: [f32; 3] = [250.0, 500.0, 1000.0];
pub const UFO_HEALTH_UBER_TURRET: f32 = 50_000.0;
pub const TURRET_HEALTH: [f32; 6] = [200.0, 300.0, 500.0, 800.0, 1200.0, 1600.0];
pub const COLONY_OBJECT_HEALTH: f32 = 10_000.0;
/// `spaceTurretNPCDamageMultiplier`.
pub const TURRET_NPC_DAMAGE_MULTIPLIER: f32 = 0.5;
/// `alliedUFOHealthMultiplier` (SpaceRelationship).
pub const ALLIED_UFO_HEALTH_MULTIPLIER: f32 = 25.0;
/// `enemylaser1..6` damage per 0.1 s shot, range 50.
pub const ENEMY_LASER_DAMAGE: [f32; 6] = [15.0, 22.5, 37.5, 60.0, 90.0, 120.0];
/// `allylaser1..6` damage per 0.1 s shot.
pub const ALLY_LASER_DAMAGE: [f32; 6] = [10.0, 15.0, 25.0, 40.0, 60.0, 80.0];
/// `turretlaser1..6` damage per 0.1 s shot.
pub const TURRET_LASER_DAMAGE: [f32; 6] = [25.0, 37.5, 62.5, 100.0, 150.0, 225.0];
pub const NPC_LASER_AUTOFIRE: f32 = 0.1;
pub const NPC_LASER_RANGE: f32 = 50.0;
/// `NPCLaserOnTimeSec` / `NPCLaserOffTimeSec`: NPC lasers fire in bursts.
pub const NPC_LASER_ON_SECONDS: f32 = 3.0;
pub const NPC_LASER_OFF_SECONDS: f32 = 1.0;
/// `spaceshipWeaponEmpireSize`: star counts that raise the NPC tier.
pub const NPC_TIER_EMPIRE_SIZES: [u32; 4] = [4, 6, 9, 13];
/// The sixth tier (index 5): VERIFIED (space-audit-a, cEmpire tier 0xC31900) tier =
/// tier at the home star's position (0: `spaceshipWeaponRadii` is empty in the
/// data) + the thresholds the star count reaches + 1 for the Grox, so only the
/// Grox (2400 stars) reaches index 5. The 100-star test below selects the same
/// empire, since no other empire can grow that large.
pub const NPC_TIER_HUGE_EMPIRE: u32 = 100;

/// NPC tier (0..=5) from an empire's star count.
pub fn empire_tier(stars: u32) -> usize {
    if stars >= NPC_TIER_HUGE_EMPIRE {
        return 5;
    }
    NPC_TIER_EMPIRE_SIZES.iter().filter(|s| stars >= **s).count()
}

/// Beam weapons (lasers, NPC / ally / turret lasers): `MinDamage` is damage
/// points per SECOND of beam contact, applied in `AutoFireRate` ticks (damage
/// x tick). VERIFIED (space-audit-a): `cDefaultBeamProjectile::Initialize`
/// (0xCB5880, called by the beam tool's setup 0x1054E70 with
/// `ftol(MinDamage x mDamageMultiplier)`) stores `mMsPerDamagePoint = 1000 /
/// points` at 0xCB58B3..0xCB58C4, one damage point every `1000 / MinDamage` ms.
/// Projectile weapons (proton missile, pulse) deal their damage per shot.
///
/// The energy drain (VERIFIED, this round): `EnergyCost` is energy PER USE of
/// the tool, not per second. `cSpaceToolData::Use` 0x1050080 (called by the
/// hold handler 0x1050500 every `AutoFireRate`, and by `TryUse` 0x1050BC0 on
/// the press) ends with `cGameDataUFO::ChangeEnergy(-EnergyCost)` (0xC38B50)
/// at 0x10504A1, so a held beam of `AutoFireRate` 0.1 s costs ten uses a
/// second: laser 1 spends 2.5 energy / s, laser 3 7.5 / s, the happy ray 1 10
/// / s, a heat ray 5 / s. See [`tool_use_energy`] and [`ToolFireClock`].
pub fn beam_damage_per_tick(damage_per_second: f32, tick: f32) -> f32 {
    damage_per_second * tick
}

/// The energy gate of `TryUse` (0x1050BC0): at 0x1050C4F `comiss energy,
/// cost; jb fail`, so a tool is usable when the reactor holds at least its
/// `EnergyCost` (the failure shows the event log 0xF04AA52A, "not enough
/// energy", at 0x105107B).
pub fn tool_can_afford_energy(energy: f32, energy_cost: f32) -> bool {
    energy >= energy_cost
}

/// Energy one `Use` of a tool spends (0x105040C..0x10504A1): `EnergyCost`, or
/// `EnergyCost x dt` when `AutoFireRate x 1000` truncates to 0 (a tool with
/// `AutoFireRate` 0 fires every frame and is charged per second; the turret
/// flak / missile tools, which cost no energy, are the only such tools).
pub fn tool_use_energy(energy_cost: f32, autofire: f32, dt: f32) -> f32 {
    if (autofire * 1000.0) as i32 == 0 {
        energy_cost * dt
    } else {
        energy_cost
    }
}

/// What the hold handler asks for this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolFire {
    /// Nothing is due.
    #[default]
    None,
    /// The press that starts a use (`TryUse`).
    First,
    /// The button is held and `AutoFireRate` has elapsed (0x105058C..0x10505F5).
    Repeat,
}

/// The timers of one tool (`cSpaceToolData` +0x88 recharge, +0xA8 auto-fire).
/// The hold handler 0x1050500 calls `Use` again when the button is held and
/// `AutoFireRate x 1000` ms have passed since the last use; a negative
/// `AutoFireRate` never repeats (one use per press, the cargo, the colony
/// pack, the consumable tools); `RechargeRate` seconds after a use the tool
/// cannot be used again (0x1050A70 `IsRecharging`). Releasing the button
/// resets the hold (0x104FAA0 `OnMouseUp`). Call [`ToolFireClock::step`]
/// every frame, then [`ToolFireClock::commit`] when the use went through (the
/// gates of `TryUse`: energy, money, target; a refused use leaves the timers).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolFireClock {
    since_use: f32,
    recharge_left: f32,
    held: bool,
}

impl ToolFireClock {
    /// Advance the timers by `dt` seconds and say whether a use is due.
    pub fn step(&mut self, autofire: f32, pressed: bool, just_pressed: bool, dt: f32) -> ToolFire {
        self.recharge_left = (self.recharge_left - dt).max(0.0);
        self.since_use += dt;
        if !pressed {
            self.held = false;
            return ToolFire::None;
        }
        if self.recharge_left > 0.0 {
            return ToolFire::None;
        }
        if just_pressed || !self.held {
            return ToolFire::First;
        }
        if autofire >= 0.0 && self.since_use >= autofire {
            return ToolFire::Repeat;
        }
        ToolFire::None
    }

    /// A use went through: the auto-fire timer restarts and the recharge
    /// timer starts (`RechargeRate` seconds).
    pub fn commit(&mut self, recharge: f32) {
        self.since_use = 0.0;
        self.held = true;
        self.recharge_left = recharge.max(0.0);
    }

    /// Seconds of recharge left (the HUD's charging bar).
    pub fn recharge_left(&self) -> f32 {
        self.recharge_left
    }
}

/// The use parameters of a tool record (`spacetools~`, VERIFIED data read
/// from the install and checked by an ignored test): energy per use,
/// auto-fire and recharge seconds (negative auto-fire: one use per press),
/// `toolRelationshipDelta` per use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolUse {
    pub id: &'static str,
    pub energy: f32,
    pub autofire: f32,
    pub recharge: f32,
    pub relationship: f32,
}

const fn tool_use_row(id: &'static str, energy: f32, autofire: f32, recharge: f32, relationship: f32) -> ToolUse {
    ToolUse { id, energy, autofire, recharge, relationship }
}

/// The planetary tools of the Space module (`space::PlanetaryTool`) and the
/// player's beams, by retail tool name.
pub const TOOL_USE: &[ToolUse] = &[
    tool_use_row("laserlevel1", 0.25, 0.1, 0.0, -0.02),
    tool_use_row("laserlevel2", 0.5, 0.1, 0.0, -0.02),
    tool_use_row("laserlevel3", 0.75, 0.1, 0.0, -0.02),
    tool_use_row("heatray", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("coldray", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("cloudray", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("cloudvacuum", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("hotcloudray", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("coldcloudvacuum", 0.5, 0.1, 0.0, 0.0),
    tool_use_row("happyraylevel1", 1.0, 0.1, 0.0, 0.005),
    tool_use_row("happyraylevel2", 2.0, 0.1, 0.0, 0.01),
    tool_use_row("happyraylevel3", 3.0, 0.1, 0.0, 0.06),
    tool_use_row("dropcargo", 0.0, -1.0, 0.0, 0.0),
    tool_use_row("genesisdevice", 0.0, -1.0, 120.0, 0.0),
    tool_use_row("monolith", 0.0, -1.0, 5.0, 0.0),
    tool_use_row("planetbusterbomb", 0.0, -1.0, 15.0, -20.0),
    tool_use_row("asteroid", 0.0, -1.0, 5.0, 0.0),
    tool_use_row("creatureedit", 0.0, -1.0, 5.0, 0.0),
];

/// The use parameters of a tool by name: the installed record (all 363 tools,
/// `space_tools::INSTALLED_TOOLS`), else the curated row, else a single use
/// per press with no energy.
pub fn tool_use(id: &str) -> ToolUse {
    if let Some(record) = crate::space_tools::record(id) {
        return ToolUse {
            id: record.name,
            energy: record.energy_cost,
            autofire: record.auto_fire_rate,
            recharge: record.recharge_rate,
            relationship: record.relationship_delta,
        };
    }
    TOOL_USE
        .iter()
        .copied()
        .find(|t| t.id == id)
        .unwrap_or(ToolUse { id: "", energy: 0.0, autofire: -1.0, recharge: 0.0, relationship: 0.0 })
}

/// The terraforming rays: the velocity a hit adds to the planet's marble
/// along (atmosphere, temperature) per use (`toolHitGround*Velocity`,
/// VERIFIED data; the ground, water, combatant and air values of these tools
/// are equal). The tool's `OnHit` 0x10593B0 hands them to
/// `cTerraformingManager` (0xBBC4B0 adds to +0x1FD4, the atmosphere
/// velocity of the planet object; the temperature velocity goes to +0x1FD8),
/// where the marble update 0xC7FC50 integrates them with the bullseye force,
/// drag and damping ([`crate::space_terraform`]).
pub const RAY_VELOCITY: &[(&str, f32, f32)] = &[
    ("heatray", 0.0, 0.01),
    ("coldray", 0.0, -0.01),
    ("cloudray", 0.01, 0.0),
    ("cloudvacuum", -0.01, 0.0),
    ("hotcloudray", 0.0075, 0.0075),
    ("coldcloudray", 0.0075, -0.0075),
    ("hotcloudvacuum", -0.0075, 0.0075),
    ("coldcloudvacuum", -0.0075, -0.0075),
];

/// What one use of the ray `id` adds to the marble (the record's ground hit),
/// if it is a terraforming beam.
pub fn ray_hit(id: &str) -> Option<crate::space_tools::HitVelocity> {
    let record = crate::space_tools::record(id)?;
    (record.class() == crate::space_tools::ToolClass::DefaultBeam && record.is_terraformer())
        .then(|| record.hit_velocity(crate::space_tools::SpaceToolHit::Ground))
}

/// Velocity (atmosphere, temperature) one use of the ray `id` adds, if it is
/// a terraforming ray.
pub fn ray_velocity(id: &str) -> Option<(f32, f32)> {
    ray_hit(id).map(|hit| (hit.atmosphere, hit.temperature))
}

/// Damage per second an NPC laser deals while its burst is on (3 s of 4).
pub fn npc_laser_dps(tier: usize) -> f32 {
    ENEMY_LASER_DAMAGE[tier.min(5)]
}

/// Whether an NPC laser is firing at `clock` seconds into its burst cycle.
pub fn npc_laser_on(clock: f32) -> bool {
    clock.rem_euclid(NPC_LASER_ON_SECONDS + NPC_LASER_OFF_SECONDS) < NPC_LASER_ON_SECONDS
}

/// Kinds of ship that can drop money when destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootShip {
    Raider,
    Bomber,
    Interceptor,
    Defender,
    Thief,
}

/// `loot*MoneyChance`.
pub fn loot_chance(kind: LootShip) -> f32 {
    match kind {
        LootShip::Raider => 0.25,
        LootShip::Bomber => 0.5,
        LootShip::Interceptor => 0.2,
        LootShip::Defender => 0.4,
        LootShip::Thief => 0.7,
    }
}

/// `lootMinMoneyDrop` / `lootMaxMoneyDrop` by tier.
pub const LOOT_MIN_MONEY: [f32; 6] = [1000.0, 1500.0, 2500.0, 4000.0, 6000.0, 8500.0];
pub const LOOT_MAX_MONEY: [f32; 6] = [10000.0, 12000.0, 15000.0, 20000.0, 30000.0, 50000.0];
/// `lootMinCityMoneyDrop` / `lootMaxCityMoneyDrop` (chance 1.0).
pub const LOOT_MIN_CITY_MONEY: [f32; 6] = [30000.0, 35000.0, 45000.0, 60000.0, 80000.0, 150000.0];
pub const LOOT_MAX_CITY_MONEY: [f32; 6] = [60000.0, 70000.0, 90000.0, 120000.0, 160000.0, 250000.0];

/// Money a destroyed ship drops: with `chance_roll` < chance, uniform between
/// min and max by tier (`amount_roll` 0..1).
pub fn ship_loot(kind: LootShip, tier: usize, chance_roll: f32, amount_roll: f32) -> Option<f32> {
    if chance_roll >= loot_chance(kind) {
        return None;
    }
    let t = tier.min(5);
    let a = amount_roll.clamp(0.0, 1.0);
    Some((LOOT_MIN_MONEY[t] + (LOOT_MAX_MONEY[t] - LOOT_MIN_MONEY[t]) * a).round())
}

/// Money a destroyed city drops (always).
pub fn city_loot(tier: usize, amount_roll: f32) -> f32 {
    let t = tier.min(5);
    let a = amount_roll.clamp(0.0, 1.0);
    (LOOT_MIN_CITY_MONEY[t] + (LOOT_MAX_CITY_MONEY[t] - LOOT_MIN_CITY_MONEY[t]) * a).round()
}

/// `spaceStolenSpicePerSecond` and `spacePirateMoneyPenalty`.
pub const PIRATE_STOLEN_SPICE_PER_SECOND: f32 = 0.025;
pub const PIRATE_MONEY_PENALTY: f32 = 100.0;

/// ADAPTED: pirate tier from the player's rank (pirates are not empire ships;
/// the tier source is not traced): Captain..Commodore 0, Admiral..Great 1,
/// higher 2.
pub fn pirate_tier(rank: u8) -> usize {
    match rank {
        0..=2 => 0,
        3..=6 => 1,
        _ => 2,
    }
}

// ================================================================== S2 store

/// One store row (`space_store_prices.txt`): base price, required tier and
/// badges, required previous tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoreItem {
    /// Store id (`spacetrading~`).
    pub id: &'static str,
    /// The tool it gives (`spaceTradingExchangeFor`).
    pub tool: &'static str,
    pub cost: f32,
    pub tier: Option<u8>,
    pub badge: Option<&'static str>,
    pub badge_or: Option<&'static str>,
    pub requires: Option<&'static str>,
    /// `type=tool` items are consumed on use; `upgrade` items are kept.
    pub consumable: bool,
}

const fn item(id: &'static str, tool: &'static str, cost: f32, tier: Option<u8>, badge: Option<&'static str>, badge_or: Option<&'static str>, requires: Option<&'static str>, consumable: bool) -> StoreItem {
    StoreItem { id, tool, cost, tier, badge, badge_or, requires, consumable }
}

/// The store items the rules use (base prices, tuning).
pub const STORE: &[StoreItem] = &[
    item("toolinterplanetarydrive", "interplanetarydrive", 10_000.0, Some(1), None, None, None, false),
    item("toolplacecolony", "placecolony", 150_000.0, Some(1), None, None, Some("interplanetarydrive"), true),
    item("toolinterstellardrive2", "interstellardrive_upgrade1", 150_000.0, Some(3), Some("frequentflyer-2"), Some("gopher-1"), Some("interstellardrive"), false),
    item("toolinterstellardrive3", "interstellardrive_upgrade2", 300_000.0, Some(3), Some("frequentflyer-3"), Some("gopher-2"), Some("interstellardrive_upgrade1"), false),
    item("toolinterstellardrive4", "interstellardrive_upgrade3", 750_000.0, Some(3), Some("frequentflyer-4"), Some("gopher-3"), Some("interstellardrive_upgrade2"), false),
    item("toolinterstellardrive5", "interstellardrive_upgrade4", 2_000_000.0, Some(3), Some("frequentflyer-5"), Some("gopher-4"), Some("interstellardrive_upgrade3"), false),
    item("toolhealth2", "healthupgrade2", 150_000.0, Some(3), Some("conquerer-1"), Some("colonist-1"), Some("healthupgrade1"), false),
    item("toolhealth3", "healthupgrade3", 300_000.0, Some(3), Some("conquerer-2"), Some("colonist-2"), Some("healthupgrade2"), false),
    item("toolhealth4", "healthupgrade4", 450_000.0, Some(3), Some("conquerer-3"), Some("colonist-3"), Some("healthupgrade3"), false),
    item("toolhealth5", "healthupgrade5", 600_000.0, Some(3), Some("conquerer-4"), Some("colonist-4"), Some("healthupgrade4"), false),
    item("toolenergy2", "energyupgrade2", 150_000.0, Some(3), Some("missionista-1"), Some("colonist-1"), Some("energyupgrade1"), false),
    item("toolenergy3", "energyupgrade3", 300_000.0, Some(3), Some("missionista-2"), Some("colonist-2"), Some("energyupgrade2"), false),
    item("toolenergy4", "energyupgrade4", 450_000.0, Some(3), Some("missionista-3"), Some("colonist-3"), Some("energyupgrade3"), false),
    item("toolenergy5", "energyupgrade5", 600_000.0, Some(3), Some("missionista-4"), Some("colonist-4"), Some("energyupgrade4"), false),
    item("toolcargoupgrade1", "cargoupgrade1", 100_000.0, None, Some("collector-1"), Some("merchant-2"), None, false),
    item("toolcargoupgrade2", "cargoupgrade2", 200_000.0, None, Some("collector-2"), Some("merchant-3"), Some("cargoupgrade1"), false),
    item("toolcargoupgrade3", "cargoupgrade3", 500_000.0, None, Some("collector-3"), Some("merchant-4"), Some("cargoupgrade2"), false),
    item("toollaser1", "laserlevel1", 100_000.0, Some(3), None, None, None, false),
    item("toollaser2", "laserlevel2", 500_000.0, Some(3), Some("conquerer-2"), Some("ecohero-1"), Some("laserlevel1"), false),
    item("toollaser3", "laserlevel3", 1_500_000.0, Some(4), Some("conquerer-3"), Some("ecohero-2"), Some("laserlevel2"), false),
    item("toolprotonmissilelevel1", "protonmissilelevel1", 100_000.0, Some(4), None, None, None, false),
    item("toolprotonmissilelevel2", "protonmissilelevel2", 400_000.0, Some(4), Some("conquerer-2"), Some("bodyguard-1"), Some("protonmissilelevel1"), false),
    item("toolprotonmissilelevel3", "protonmissilelevel3", 1_200_000.0, Some(4), Some("conquerer-3"), Some("bodyguard-2"), Some("protonmissilelevel2"), false),
    item("toolpulse1", "pulselevel1", 200_000.0, Some(2), Some("conquerer-1"), Some("colonist-1"), None, false),
    item("toolshield", "shield", 500_000.0, Some(5), Some("warmonger-4"), Some("colonist-3"), None, false),
    item("toolcloak", "cloak", 250_000.0, Some(5), Some("missionista-5"), Some("trader-5"), None, false),
    item("toolrallycall", "rallycall", 750_000.0, Some(5), Some("warmonger-3"), Some("diplomat-3"), None, false),
    item("toolrepairpotion", "repairpotion", 10_000.0, Some(4), Some("missionista-2"), Some("trader-2"), None, true),
    item("toolenergypotion", "energypotion", 10_000.0, Some(3), Some("missionista-1"), Some("trader-1"), None, true),
    item("toolheatray", "heatray", 450_000.0, Some(2), Some("terrawrangler-3"), Some("empire-3"), None, false),
    item("toolcoldray", "coldray", 450_000.0, Some(2), Some("terrawrangler-3"), Some("empire-3"), None, false),
    item("toolcloudray", "cloudray", 450_000.0, Some(2), Some("terrawrangler-2"), Some("empire-2"), None, false),
    item("toolcloudvacuum", "cloudvacuum", 450_000.0, Some(2), Some("terrawrangler-2"), Some("empire-2"), None, false),
    item("toolatmospheregenerator", "atmospheregenerator", 150_000.0, Some(4), Some("missionista-1"), Some("empire-1"), None, true),
    item("tooldroughtgenerator", "droughtgenerator", 150_000.0, Some(4), Some("missionista-1"), Some("empire-1"), None, true),
    item("tooliceStorm", "icestorm", 150_000.0, Some(4), Some("missionista-2"), Some("empire-2"), None, true),
    item("toolasteroid", "asteroid", 225_000.0, Some(4), Some("missionista-3"), Some("empire-3"), None, true),
    item("toolhappyray1", "happyraylevel1", 200_000.0, Some(3), Some("economist-2"), Some("diplomat-2"), None, false),
    item("toolplaceembassy", "placeembassy", 500_000.0, Some(4), Some("diplomat-5"), Some("traveler-5"), None, true),
    item("toolplacespicestorage", "placespicestorage", 500_000.0, Some(4), Some("trader-2"), Some("colonist-3"), None, true),
    item("toolplaceuberturret", "placeuberturret", 750_000.0, Some(4), Some("warmonger-3"), Some("colonist-4"), None, true),
    item("toolplanetbuster", "planetbusterbomb", 5_000_000.0, Some(5), Some("conquerer-5"), Some("colonist-5"), None, true),
    item("toolwormhole", "wormholekey", 3_000_000.0, Some(5), Some("frequentflyer-3"), Some("traveler-3"), None, false),
    item("toolmonolith", "monolith", 500_000.0, Some(2), Some("traveler-2"), Some("zoologist-2"), None, true),
    item("toolcreatureedit", "creatureedit", 500_000.0, Some(4), Some("traveler-2"), Some("cleaner-3"), None, true),
    item("toolcreaturecreate", "creaturecreate", 1_500_000.0, Some(4), Some("traveler-4"), Some("cleaner-5"), None, true),
];

pub fn store_item_for_tool(tool_id: &str) -> Option<&'static StoreItem> {
    STORE.iter().find(|i| i.tool == tool_id)
}

/// Base price of the store item that gives `tool_id` (None when not sold).
pub fn store_price(tool_id: &str) -> Option<f32> {
    store_item_for_tool(tool_id).map(|i| i.cost)
}

/// Price at an NPC store: base x the seller archetype's
/// `spaceTradingCostMultiplier<Arch>` x `difficultySpaceTradeToolCost`.
/// VERIFIED (space-audit-a, 0x103E3A0..0x103E526): the shown price is the
/// store row's price x the seller's `spaceTradingCostMultiplier<Arch>` x
/// `difficultySpaceTradeToolCost` (0x35EDC257 through 0xC75DD0) x the
/// superpower tool-cost multiplier (0x1005B00(2), 1.0 unless the card's
/// category matches); all four are multiplied, order is irrelevant.
pub fn store_price_at(base: f32, seller_multiplier: f32, difficulty: Difficulty, discount: bool) -> f32 {
    let card = if discount { SUPERPOWER_TOOL_COST_MULTIPLIER } else { 1.0 };
    (base * seller_multiplier * difficulty.pick(DIFFICULTY_TRADE_TOOL_COST) * card).round()
}

// ======================================================= S2 spice, colonies

/// Spice base prices (`spice1..6`): red, yellow, blue, green, pink, purple.
pub const SPICE_BASE_PRICE: [f32; 6] = [3375.0, 4500.0, 5230.0, 10125.0, 11242.0, 14625.0];
/// `spaceEconomyTradeMaxSpiceBought`: units one NPC buys.
pub const NPC_MAX_SPICE_BOUGHT: u32 = 200;
/// `spaceEconomyMaxCargoCount`: one cargo slot stacks this many.
pub const MAX_CARGO_STACK: u8 = 99;
/// Cargo slots of a new ship (ModAPI inventory). ADAPTED: slots added per
/// cargo upgrade are not in the data (S8); +5 per upgrade.
pub const CARGO_SLOTS: u8 = 10;
pub const CARGO_SLOTS_PER_UPGRADE: u8 = 5;

/// Sell price per unit at an NPC: base x `difficultySpaceSpiceCost`.
/// ADAPTED: the demand modifier (the NPC's spice "need" texts) is not traced.
pub fn spice_sell_price(base: f32, difficulty: Difficulty) -> f32 {
    (base * difficulty.pick(DIFFICULTY_SPICE_COST)).round()
}

/// `spaceEconomySpiceProductionMultiplier` (0xC720A0).
pub const SPICE_PRODUCTION_MULTIPLIER: f32 = 0.0005;
/// `spaceEconomySpiceHomeworldMultiplier`.
pub const SPICE_HOMEWORLD_MULTIPLIER: f32 = 0.025;
/// `spaceEconomySpiceStorageMultiplier` (with a spice storage).
pub const SPICE_STORAGE_MULTIPLIER: f32 = 2.0;
/// `colonyMaxSpiceStoredPerColony` (tuning; the getter's default is 10).
pub const MAX_SPICE_PER_COLONY: f32 = 5.0;
/// `spaceEconomySpiceModifiers` red / yellow / blue (0xC721D0).
pub const SPICE_STAR_MODIFIERS: [f32; 3] = [1.0, 1.0, 1.0];
/// `secondsBetweenColonySimulatorTicks`.
pub const COLONY_SIM_TICK_SECONDS: f32 = 120.0;
/// Colony base production (VERIFIED, `notes\space-final-notes.md` F3): the
/// colony estimate 0xC721D0 passes 0xBF70E0, the civilization income, to
/// 0xC720A0: the sum of `cCity::GetIncome` 0xBD8CD0 = `mFinalIncome` = 400
/// per factory linked to a house or the city hall (`civ-rules-re.md`; the
/// party / riot x2 / half applies outside Space only) plus owned spice
/// derricks (none in Space). One link fills a spice every 300 s.
pub const BASE_PRODUCTION_PER_FACTORY: f32 = 400.0;

/// Spice produced per second by one colony (0xC720A0): base x 0.0005 / 60,
/// x 0.025 on the homeworld else x 4.5 / n when n > 4, x 1.5 with the civ
/// economic card, x `difficultySpiceProduction`, x the star-colour modifier.
pub fn colony_spice_rate(
    base_production: f32,
    homeworld: bool,
    planet_count: u32,
    flags: u32,
    difficulty: Difficulty,
    star_modifier: f32,
) -> f32 {
    let mut rate = base_production.max(0.0) * SPICE_PRODUCTION_MULTIPLIER / 60.0;
    if homeworld {
        rate *= SPICE_HOMEWORLD_MULTIPLIER;
    } else if planet_count > 4 {
        rate *= 4.5 / planet_count as f32;
    }
    rate *= star_modifier;
    if flags & superpower_bit::CIV_ECONOMIC != 0 {
        rate *= SUPERPOWER_SPICE_INCREASE_MULTIPLIER;
    }
    rate * difficulty.pick(DIFFICULTY_SPICE_PRODUCTION)
}

/// Spice a colony stores before production stops.
pub fn colony_spice_cap(spice_storage: bool) -> f32 {
    MAX_SPICE_PER_COLONY * if spice_storage { SPICE_STORAGE_MULTIPLIER } else { 1.0 }
}

/// Which spice a planet makes (cSpaceTrading::AssignPlanetSpice 0x103CA50):
/// one draw (`roll` 0..1) against the cumulative chances for the star colour;
/// "hard" planets use the Hard column. Returns the spice index (0 red ..
/// 5 purple). `star`: 0 red (M, MM, others), 1 yellow (G, GG, GM), 2 blue (O, OO, OM, OG).
pub fn planet_spice(star: usize, hard: bool, roll: f32) -> usize {
    // Chances per spice (red, yellow, blue, green, pink, purple).
    let table: [[f32; 6]; 6] = [
        [0.96, 0.0, 0.0, 0.02, 0.02, 0.0],  // red easy
        [0.5, 0.44, 0.0, 0.02, 0.02, 0.02], // red hard
        [0.0, 0.94, 0.0, 0.02, 0.02, 0.02], // yellow easy
        [0.0, 0.74, 0.0, 0.22, 0.02, 0.02], // yellow hard
        [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],     // blue easy
        [0.0, 0.0, 0.2, 0.3, 0.35, 0.15],   // blue hard
    ];
    let row = &table[star.min(2) * 2 + usize::from(hard)];
    let mut sum = 0.0;
    for (spice, chance) in row.iter().enumerate() {
        sum += chance;
        if roll < sum {
            return spice;
        }
    }
    row.iter().rposition(|c| *c > 0.0).unwrap_or(0)
}

/// Colony building prices (SpaceEconomy).
pub const PRODUCTION_BUILDING_COST: f32 = 20_000.0;
pub const DEFENSIVE_BUILDING_COST: f32 = 10_000.0;
pub const CIVIC_OBJECT_COST: f32 = 25.0;
pub const VEHICLE_COST: f32 = 500.0;
/// `spaceEconomyHomeWorldBuildingMultiply`.
pub const HOMEWORLD_BUILDING_MULTIPLIER: f32 = 16.0;
/// Colony size by terraform level (TerraformTuning `spaceTerraformT1/T2/T3ColonyMaxBuildings`
/// 6 / 12 / 20): max buildings of one city, T1 / T2 / T3.
pub const COLONY_MAX_BUILDINGS: [u32; 3] = [6, 12, 20];
/// What a city may hold at T0 (and on a wildlife sanctuary): one building
/// (`cTerraformingManager::GetMaxBuildings` 0xBBE760, VERIFIED: any T-score
/// outside 1..=3 returns 1).
pub const COLONY_MAX_BUILDINGS_T0: u32 = 1;

/// `cTerraformingManager::GetMaxBuildings` 0xBBE760 (VERIFIED): the building
/// limit of a city on a planet whose T-score is `t_score` (the getters
/// 0x1049E80 / 0x1049EC0 / 0x1049F00 read the three tuning ids): 6 / 12 / 20 at
/// T1 / T2 / T3, one at T0. (Retail also reads a Grox star by its distance
/// tier: the ring count of `spaceshipWeaponRadii`, empty in the install, so
/// tier 0 and one building; the port has no Grox colonies.)
pub fn colony_max_buildings(t_score: u8) -> usize {
    match t_score {
        1..=3 => COLONY_MAX_BUILDINGS[usize::from(t_score) - 1] as usize,
        _ => COLONY_MAX_BUILDINGS_T0 as usize,
    }
}

/// The reach of a tool's galactic relationship delta: the record's
/// `toolGalacticRelationshipRadius` capped at 30 (the 0x13F0E68 constant in
/// `cToolStrategy::OnHit` 0x1059A81, VERIFIED), in galaxy units around the
/// active star.
pub fn galactic_delta_radius(record_radius: f32) -> f32 {
    record_radius.min(GALACTIC_DELTA_RADIUS_CAP)
}
pub const GALACTIC_DELTA_RADIUS_CAP: f32 = 30.0;

/// Trade routes (SpaceEconomy / spacenpcai).
pub const TRADE_ROUTE_MAX: usize = 5;
pub const TRADE_ROUTE_SHIP_INTERVAL_GALAXY: f32 = 1.0;
pub const TRADE_ROUTE_SHIP_INTERVAL_SOLAR: f32 = 10.0;
pub const TRADE_ROUTE_BUNDLE_INTERVAL: f32 = 240.0;
/// `tradeCapturePerMS`, `tradeCaptureDistanceFactor`, `tradeCaptureMinRate` /
/// `MaxRate`, `tradeCaptureDistanceBase` (SpaceEconomy).
pub const TRADE_CAPTURE_PER_MS: f32 = 2e-5;
pub const TRADE_CAPTURE_DISTANCE_FACTOR: f32 = 1.0;
pub const TRADE_CAPTURE_MIN_RATE: f32 = 4.2e-5;
pub const TRADE_CAPTURE_MAX_RATE: f32 = 6.6e-5;
pub const TRADE_CAPTURE_DISTANCE_BASE: f32 = 10.0;

/// Progress (percent of a route's maturity per millisecond) of a route of
/// `distance` galaxy units. VERIFIED (space-audit-a, `cSpaceTradeRouteManager`
/// update 0x1038420, at 0x1038D39..0x1038E07): `rate = clamp(tradeCapturePerMS /
/// (1 + tradeCaptureDistanceFactor x distance / tradeCaptureDistanceBase),
/// MinRate, MaxRate)`; the route's progress (0..100, capped at 100.0 =
/// 0x13EC4B0) grows by `dt_ms x rate`. With the installed numbers the quotient
/// is always below `MinRate`, so every route matures at `MinRate`: 4.2e-5 per ms.
pub fn trade_capture_rate(distance: f32) -> f32 {
    let divisor = 1.0 + TRADE_CAPTURE_DISTANCE_FACTOR * distance / TRADE_CAPTURE_DISTANCE_BASE;
    (TRADE_CAPTURE_PER_MS / divisor).clamp(TRADE_CAPTURE_MIN_RATE, TRADE_CAPTURE_MAX_RATE)
}

/// Trade-route maturity gained in `dt` seconds as a fraction of 100 % (the
/// retail progress runs 0..100; the port stores 0..1): `dt_ms x rate / 100`.
/// A route takes 100 / 4.2e-5 ms, about 2381 s, to mature. The distance does
/// not change the rate with the installed tuning (see `trade_capture_rate`),
/// so the port does not need to carry one per route.
pub fn trade_capture_progress(dt: f32) -> f32 {
    dt * 1000.0 * trade_capture_rate(0.0) / 100.0
}

// ============================================================ S3 relationship

/// Relationship events (`space_npc_relationship_effects~`, property names
/// `RelationshipEvent<Name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RelationshipEvent {
    Compliment,
    Insult,
    DeclaredWar,
    Gift,
    Trade,
    JoinedAlliance,
    FoughtEnemy,
    MissionComplete,
    MissionFailed,
    MissionRejected,
    MissionAborted,
    MissionStarted,
    GiveGift,
    AcceptGift,
    CreateAlliance,
    BreakAlliance,
    TradeComplete,
    TradeDeclined,
    TradeRouteSpice,
    CityPanicked,
    TerraformImproved,
    TerraformWorsened,
    TerraformExtinction,
    DestroyBuilding,
    DestroyUfo,
    DestroyAllyUfo,
    BadToolUse,
    GoodToolUse,
    FloodCity,
    AbductCitizen,
    StealCommodity,
    UpliftedCiv,
    BadSystemPurchaseOffer,
    GoodSystemPurchaseOffer,
    BadPeaceOffer,
    GoodPeaceOffer,
    BeNice,
    BeNasty,
    PushedTooFar,
    CapturedASystem,
    WasAtWar,
    EmbassyBonus,
    WitholdTribute,
    CommunicatedNice,
    CommunicatedMean,
    AvoidedContact,
    StartedWar,
    ArchetypeNice,
    ArchetypeMean,
    PersonalityNice,
    PersonalityMean,
    SuperPower,
    AlliedWithGrob,
    CommittedAtrocity,
    /// OpenSpore only: an old save's -100..100 standing, carried over once
    /// (row: no decay, -10..10).
    Legacy,
}

/// One event row: [initial, add per event, min, max, decay, size weight].
/// VERIFIED (0xD01730): the contribution is clamp(initial + X x add x f, min,
/// max), X the accumulated event scale, f = 1 + |w| x (clamp(size ratio, 0.5,
/// 2) - 1) in Space; (0xD06070): each decay tick moves X toward 0 by |decay|.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventRow {
    pub initial: f32,
    pub add: f32,
    pub min: f32,
    pub max: f32,
    pub decay: f32,
    pub size_weight: f32,
}

const fn row(v: [f32; 6]) -> EventRow {
    EventRow { initial: v[0], add: v[1], min: v[2], max: v[3], decay: v[4], size_weight: v[5] }
}

/// Default personality rows (`_defaultpersonality` = `space_npc_relationship_effects~!0x4E5855B9`).
pub fn default_event_row(event: RelationshipEvent) -> EventRow {
    use RelationshipEvent::*;
    match event {
        Compliment => row([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        Insult => row([-1.0, -0.5, -3.0, 0.0, 0.0, 0.0]),
        DeclaredWar => row([-5.0, -1.0, -10.0, 0.0, 0.0, 0.0]),
        Gift => row([0.4, 0.05, 0.0, 12.0, -0.1, 0.0]),
        Trade => row([0.3, 0.2, 0.0, 5.0, 0.0, 0.0]),
        JoinedAlliance => row([0.0, 1.0, 0.0, 10.0, 0.0, 0.0]),
        FoughtEnemy => row([1.0, 0.02, 0.0, 7.0, 0.0, 0.0]),
        MissionComplete => row([0.0, 1.0, -10.0, 10.0, -0.1, 1.0]),
        MissionFailed => row([0.0, -1.0, -5.0, 10.0, -0.25, 0.5]),
        MissionRejected => row([0.0, -1.0, -5.0, 10.0, -0.25, 0.5]),
        MissionAborted => row([0.0, -1.0, -5.0, 10.0, -0.25, 0.5]),
        MissionStarted => row([1.0, 0.0, 0.0, 1.0, -0.5, 0.0]),
        GiveGift => row([0.0, 1.0, -10.0, 5.0, -0.5, 0.5]),
        AcceptGift => row([0.0, 2.0, 0.0, 10.0, -0.1, 0.5]),
        CreateAlliance => row([0.0, 5.0, -10.0, 5.0, -0.5, 1.0]),
        BreakAlliance => row([-5.0, -5.0, -20.0, 10.0, -0.25, 0.5]),
        TradeComplete => row([0.0, 0.25, -1.0, 1.5, -0.5, 0.0]),
        TradeDeclined => row([0.0, 0.0, -5.0, 5.0, -0.5, 0.0]),
        TradeRouteSpice => row([0.0, 1.0, -5.0, 3.0, -0.5, 0.0]),
        CityPanicked => row([0.0, -2.0, -10.0, 10.0, -0.5, 0.0]),
        TerraformImproved => row([0.0, 3.0, -10.0, 10.0, -0.5, 0.5]),
        TerraformWorsened => row([0.0, -3.0, -10.0, 10.0, -0.5, 1.0]),
        TerraformExtinction => row([0.0, -5.0, -10.0, 10.0, -0.5, 0.5]),
        DestroyBuilding => row([0.0, -1.5, -10.0, 10.0, -0.5, 0.5]),
        DestroyUfo => row([0.0, -1.5, -10.0, 10.0, -0.5, 0.5]),
        DestroyAllyUfo => row([0.0, -0.5, -10.0, 10.0, -0.5, 0.5]),
        BadToolUse => row([0.0, -1.0, -10.0, 10.0, -0.5, 0.5]),
        GoodToolUse => row([0.0, 1.0, -10.0, 5.0, -0.5, 0.5]),
        FloodCity => row([0.0, -8.0, -10.0, 10.0, -0.5, 0.5]),
        AbductCitizen => row([0.0, -0.5, -3.0, 10.0, -0.5, 0.5]),
        StealCommodity => row([0.0, -0.5, -3.0, 5.0, -0.5, 0.5]),
        UpliftedCiv => row([0.0, 5.0, 0.0, 5.0, 0.0, 0.0]),
        BadSystemPurchaseOffer => row([-1.0, -1.0, -10.0, 0.0, -0.1, 0.5]),
        GoodSystemPurchaseOffer => row([1.0, 1.0, 0.0, 10.0, -0.1, 0.5]),
        BadPeaceOffer => row([0.5, 0.5, 0.0, 10.0, -0.1, 0.5]),
        GoodPeaceOffer => row([1.0, 1.0, 0.0, 10.0, -0.5, 0.5]),
        BeNice => row([0.0, 2.0, -5.0, 10.0, -0.5, 0.0]),
        BeNasty => row([0.0, -4.0, -20.0, 0.0, -0.5, 0.0]),
        PushedTooFar => row([0.0, -10.0, -10.0, 0.0, -0.5, 0.3]),
        CapturedASystem => row([-10.0, -5.0, -10.0, 0.0, -0.25, 0.5]),
        WasAtWar => row([-2.0, -1.0, -10.0, 0.0, -0.25, 0.3]),
        EmbassyBonus => row([0.0, 1.0, 0.0, 5.0, -0.5, 0.3]),
        WitholdTribute => row([-2.5, -1.5, -10.0, 0.0, -0.5, 0.0]),
        CommunicatedNice => row([1.0, 0.0, 0.0, 1.0, -0.5, 0.0]),
        CommunicatedMean => row([-1.0, 0.0, -1.0, 0.0, -0.5, 0.0]),
        AvoidedContact => row([-0.5, -0.5, -2.0, 0.0, -0.5, 0.0]),
        StartedWar => row([-5.0, 0.0, -20.0, 0.0, -0.2, 0.0]),
        ArchetypeNice => row([0.0, 1.0, 0.0, 5.0, -0.25, 0.0]),
        ArchetypeMean => row([0.0, -1.0, -5.0, 0.0, -0.25, 0.0]),
        PersonalityNice => row([0.0, 0.0, 0.0, 0.0, -0.25, 0.0]),
        PersonalityMean => row([0.0, 0.0, 0.0, 0.0, -0.25, 0.0]),
        SuperPower => row([0.0, 1.0, 0.0, 1.0, 0.0, 0.0]),
        AlliedWithGrob => row([-20.0, -5.0, -20.0, 0.0, -0.05, 0.0]),
        CommittedAtrocity => row([0.0, -1.0, -20.0, 10.0, -0.5, 0.5]),
        Legacy => row([0.0, 1.0, -10.0, 10.0, 0.0, 0.0]),
    }
}

/// An NPC empire's relationship personality (`spg_<archetype>` +
/// `_easy` / `_medium` / `_hard`, chosen by 0xC30FE0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Personality {
    #[default]
    Default,
    Bard,
    Diplomat,
    Ecologist,
    Grox,
    Scientist,
    Shaman,
    Trader,
    Warrior,
    Zealot,
}

impl Personality {
    /// The personality of an NPC empire of `archetype` (`is_grox` for the Grox).
    /// ADAPTED: Knight and Wanderer empires (no retail personality) use the default.
    pub fn of(archetype: SpaceArchetype, is_grox: bool) -> Self {
        if is_grox {
            return Personality::Grox;
        }
        match archetype {
            SpaceArchetype::Bard => Personality::Bard,
            SpaceArchetype::Diplomat => Personality::Diplomat,
            SpaceArchetype::Ecologist => Personality::Ecologist,
            SpaceArchetype::Scientist => Personality::Scientist,
            SpaceArchetype::Shaman => Personality::Shaman,
            SpaceArchetype::Trader => Personality::Trader,
            SpaceArchetype::Warrior => Personality::Warrior,
            SpaceArchetype::Zealot => Personality::Zealot,
            SpaceArchetype::Knight | SpaceArchetype::Wanderer => Personality::Default,
        }
    }

    /// `surrenderChance` / `surrenderChanceNearDeath`.
    pub fn surrender_chance(self) -> (f32, f32) {
        match self {
            Personality::Default => (1.0, 1.0),
            Personality::Bard | Personality::Ecologist | Personality::Scientist | Personality::Shaman => (0.5, 0.5),
            Personality::Diplomat | Personality::Trader => (0.75, 1.0),
            Personality::Warrior | Personality::Zealot => (0.25, 0.0),
            Personality::Grox => (0.0, 0.0),
        }
    }

    /// `missionGeneratorWeight` (generator order). VERIFIED (space-audit-a): all
    /// 16 values of every row read from the installed personality files
    /// (`space_npc_relationship_effects~` 0x0568DE14, `spg_<arch>` and
    /// `_defaultpersonality`); the earlier dump was cut after 14-15 values.
    pub fn mission_weights(self) -> [u32; 16] {
        match self {
            Personality::Default => [60, 20, 20, 0, 10, 0, 10, 10, 0, 10, 10, 10, 10, 10, 10, 10],
            Personality::Bard => [100, 50, 40, 40, 50, 40, 50, 40, 50, 40, 40, 40, 100, 100, 70, 40],
            Personality::Diplomat => [50, 40, 50, 40, 40, 40, 70, 40, 70, 40, 40, 40, 50, 70, 70, 50],
            Personality::Ecologist => [40, 100, 40, 40, 90, 100, 40, 40, 40, 70, 70, 100, 40, 40, 40, 50],
            Personality::Grox => [30, 30, 80, 100, 30, 0, 30, 30, 100, 30, 30, 0, 30, 60, 100, 30],
            Personality::Scientist => [100, 70, 40, 40, 100, 70, 50, 100, 40, 100, 100, 50, 50, 40, 40, 70],
            Personality::Shaman => [70, 40, 40, 40, 70, 70, 50, 50, 40, 50, 50, 70, 50, 50, 40, 50],
            Personality::Trader => [70, 40, 40, 40, 70, 40, 100, 40, 40, 40, 40, 40, 40, 40, 40, 40],
            Personality::Warrior => [70, 40, 100, 100, 40, 40, 50, 70, 100, 40, 40, 40, 40, 50, 90, 10],
            Personality::Zealot => [100, 40, 70, 50, 40, 40, 40, 40, 50, 40, 40, 40, 100, 40, 40, 10],
        }
    }

    /// `RelationshipEventSpaceArchetypeMods`: how this personality treats a
    /// player of each archetype, in the order warrior, trader, scientist,
    /// shaman, bard, zealot, diplomat, ecologist (+1 ArchetypeNice for its
    /// own archetype; warriors -1 ArchetypeMean towards warriors).
    pub fn archetype_mods(self) -> [f32; 8] {
        match self {
            Personality::Warrior => [-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Personality::Trader => [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Personality::Scientist => [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Personality::Shaman => [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            Personality::Bard => [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            Personality::Zealot => [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Personality::Diplomat => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            Personality::Ecologist => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            Personality::Default | Personality::Grox => [0.0; 8],
        }
    }

    /// Event row for this personality at `difficulty` (the overrides of the
    /// `spg_*` / `_easy` / `_medium` rows; the plain and `_hard` rows match).
    pub fn event_row(self, event: RelationshipEvent, difficulty: Difficulty) -> EventRow {
        use RelationshipEvent::*;
        let zero = row([0.0; 6]);
        // Difficulty overrides common to every archetype personality.
        if self != Personality::Default {
            match (difficulty, event) {
                (Difficulty::Easy, MissionRejected | MissionAborted) => return zero,
                (Difficulty::Normal, MissionAborted) => return zero,
                (Difficulty::Easy, WitholdTribute) => return row([-0.5, -0.5, -10.0, 0.0, -0.5, 0.0]),
                (Difficulty::Normal, WitholdTribute) => return row([-1.75, -0.75, -10.0, 0.0, -0.5, 0.0]),
                _ => {}
            }
        }
        let over = match (self, event) {
            (Personality::Bard | Personality::Shaman, AbductCitizen) => Some([0.0, -1.0, -5.0, 10.0, -0.5, 0.5]),
            (Personality::Diplomat | Personality::Warrior, BreakAlliance) => Some([-5.0, -5.0, -20.0, 10.0, -0.125, 0.5]),
            (Personality::Diplomat | Personality::Warrior | Personality::Shaman, CreateAlliance) => Some([0.0, 5.0, -10.0, 5.0, -0.05, 1.0]),
            (Personality::Diplomat, GoodPeaceOffer) => Some([2.0, 2.0, 0.0, 10.0, 0.0, 0.5]),
            (Personality::Diplomat | Personality::Trader, PersonalityNice) => Some([3.0, 0.0, 0.0, 3.0, 0.0, 0.0]),
            (Personality::Ecologist, TerraformWorsened) => Some([0.0, -6.0, -10.0, 10.0, -0.1, 0.5]),
            (Personality::Ecologist, TerraformImproved) => Some([0.0, 6.0, -10.0, 10.0, -0.1, 0.5]),
            (Personality::Ecologist, TerraformExtinction) => Some([0.0, -10.0, -10.0, 10.0, -0.1, 0.5]),
            (Personality::Scientist, MissionComplete) => Some([0.0, 1.0, -10.0, 10.0, -0.005, 0.5]),
            (Personality::Scientist, MissionFailed) => Some([0.0, -2.0, -5.0, 10.0, -0.5, 0.5]),
            (Personality::Shaman, GiveGift) => Some([0.0, 2.0, -10.0, 5.0, -0.1, 0.5]),
            (Personality::Trader, Trade) => Some([0.5, 0.5, 0.0, 2.0, -0.05, 0.0]),
            (Personality::Trader, GiveGift) => Some([0.0, 2.0, -10.0, 5.0, -0.05, 0.5]),
            (Personality::Trader, TradeComplete) => Some([0.0, 1.0, -3.0, 2.0, -0.1, 0.5]),
            (Personality::Trader, DestroyBuilding) => Some([0.0, -3.0, -10.0, 10.0, -0.2, 1.0]),
            (Personality::Trader, FloodCity) => Some([0.0, -16.0, -10.0, 10.0, -0.1, 1.0]),
            (Personality::Trader, StealCommodity) => Some([0.0, -1.0, -6.0, 5.0, -0.5, 1.0]),
            (Personality::Trader, BadSystemPurchaseOffer) => Some([-1.0, -2.0, -10.0, 0.0, -0.5, 1.0]),
            (Personality::Trader, GoodSystemPurchaseOffer) => Some([1.0, 2.0, 0.0, 10.0, -0.5, 1.0]),
            (Personality::Trader, TradeRouteSpice) => Some([0.0, 0.5, -5.0, 4.0, -0.05, 0.0]),
            (Personality::Warrior, DestroyAllyUfo) => Some([0.0, -1.0, -10.0, 10.0, -0.1, 0.5]),
            (Personality::Warrior, BadPeaceOffer) => Some([0.25, 0.25, 0.0, 10.0, -0.1, 0.5]),
            (Personality::Warrior | Personality::Zealot, PersonalityMean) => Some(match difficulty {
                Difficulty::Easy => [-1.5, 0.0, -1.5, 0.0, 0.0, 0.0],
                _ => [-3.0, 0.0, -3.0, 0.0, 0.0, 0.0],
            }),
            (Personality::Zealot, CityPanicked) => Some([0.0, -2.0, -5.0, 10.0, -0.1, 0.5]),
            (Personality::Grox, e) => grox_row(e),
            _ => None,
        };
        over.map(row).unwrap_or_else(|| default_event_row(event))
    }
}

/// `spg_grob` overrides (the Grox personality).
fn grox_row(event: RelationshipEvent) -> Option<[f32; 6]> {
    use RelationshipEvent::*;
    Some(match event {
        MissionComplete => [0.0, 0.25, -10.0, 5.0, -0.1, 0.0],
        MissionFailed => [0.0, -3.0, -10.0, 0.0, -0.1, 0.0],
        GiveGift => [0.0, 0.05, -10.0, 1.0, -0.5, 0.0],
        BreakAlliance => [0.0, -5.0, -10.0, 0.0, -0.1, 0.0],
        CreateAlliance => [0.0, 5.0, -10.0, 3.0, -0.5, 0.0],
        TradeComplete => [0.0, 0.25, -3.0, 0.0, -0.1, 0.0],
        TradeDeclined => [0.0, 0.0, -5.0, 0.0, -0.1, 0.0],
        CityPanicked => [0.0, -2.0, -10.0, 0.0, -0.1, 0.0],
        TerraformWorsened | TerraformImproved | TerraformExtinction => [0.0, -5.0, -20.0, 0.0, -0.1, 0.0],
        DestroyBuilding => [0.0, -10.0, -10.0, 0.0, -0.1, 0.0],
        DestroyAllyUfo => [0.0, -3.0, -10.0, 0.0, -0.5, 0.0],
        BadToolUse => [0.0, -1.0, -10.0, 0.0, -0.1, 0.0],
        GoodToolUse => [0.0, 0.1, -10.0, 3.0, -0.5, 0.0],
        FloodCity => [0.0, -8.0, -10.0, 0.0, -0.1, 0.0],
        AbductCitizen => [0.0, -1.0, -3.0, 0.0, -0.1, 0.0],
        StealCommodity => [0.0, -10.0, -10.0, 0.0, -0.1, 0.0],
        UpliftedCiv => [0.0, 10.0, 0.0, 0.0, -0.25, 0.0],
        BadSystemPurchaseOffer => [-1.0, -5.0, -10.0, 0.0, -0.1, 0.0],
        GoodSystemPurchaseOffer => [1.0, 1.0, 0.0, 0.0, -1.0, 0.0],
        BadPeaceOffer => [0.5, -3.0, -10.0, 0.0, -0.5, 0.0],
        GoodPeaceOffer => [1.0, 1.0, 0.0, 1.0, -0.5, 0.0],
        BeNice => [0.0, 2.0, -10.0, 1.0, -0.25, 0.0],
        BeNasty => [0.0, -3.0, -10.0, 0.0, -0.1, 0.0],
        PushedTooFar => [0.0, -10.0, -10.0, 0.0, -0.2, 0.0],
        CapturedASystem => [-10.0, -5.0, -20.0, 0.0, -0.1, 0.0],
        WasAtWar => [-2.0, -1.0, -10.0, 0.0, 0.0, 0.0],
        EmbassyBonus => [0.0, 1.0, 0.0, 1.0, -0.5, 0.0],
        DestroyUfo => [0.0, -1.0, -10.0, 0.0, -0.1, 0.0],
        WitholdTribute => [0.0, -8.0, -10.0, 0.0, -0.1, 0.0],
        AcceptGift => [0.0, 0.5, 0.0, 1.0, -1.0, 0.0],
        MissionStarted => [1.0, 0.0, 0.0, 0.5, -0.5, 0.0],
        CommunicatedNice | PersonalityNice | AlliedWithGrob => [0.0; 6],
        PersonalityMean => [-7.0, 0.0, -10.0, 0.0, 0.0, 0.0],
        AvoidedContact => [-1.0, -1.0, -2.0, 0.0, -0.1, 0.0],
        StartedWar => [-5.0, 0.0, -20.0, 0.0, -0.1, 0.0],
        CommittedAtrocity => [0.0, 0.2, 0.0, 5.0, 0.1, 0.0],
        _ => return None,
    })
}

/// Index of the player's archetype in `archetype_mods` (None for Knight / Wanderer).
pub fn archetype_mod_index(archetype: SpaceArchetype) -> Option<usize> {
    match archetype {
        SpaceArchetype::Warrior => Some(0),
        SpaceArchetype::Trader => Some(1),
        SpaceArchetype::Scientist => Some(2),
        SpaceArchetype::Shaman => Some(3),
        SpaceArchetype::Bard => Some(4),
        SpaceArchetype::Zealot => Some(5),
        SpaceArchetype::Diplomat => Some(6),
        SpaceArchetype::Ecologist => Some(7),
        SpaceArchetype::Knight | SpaceArchetype::Wanderer => None,
    }
}

/// The empire-size factor f (0xD01730): 1 + |w| x (clamp(ratio, 0.5, 2) - 1),
/// `ratio` the size ratio picked by the sign of w.
pub fn size_factor(weight: f32, ratio: f32) -> f32 {
    if weight == 0.0 {
        return 1.0;
    }
    1.0 + weight.abs() * (ratio.clamp(0.5, 2.0) - 1.0)
}

/// The decay period of the relationship manager (S3A).
pub const RELATIONSHIP_DECAY_PERIOD: f32 = 60.0;
/// The relationship as the attitude reads it is capped to +-10 (0xD00BD0).
pub const RELATIONSHIP_CAP: f32 = 10.0;

/// One empire's relationship with the player: the accumulated scale X of
/// every event that happened (cRelationshipData::mRelationshipEvents).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Relationship {
    #[serde(default)]
    pub events: BTreeMap<RelationshipEvent, f32>,
    /// The first-contact personality events have been applied.
    #[serde(default)]
    pub met: bool,
}

impl Relationship {
    /// Contribution of one event (0xD01730).
    pub fn contribution(row: EventRow, x: f32, size_ratio: f32) -> f32 {
        (row.initial + x * row.add * size_factor(row.size_weight, size_ratio)).clamp(row.min, row.max)
    }

    /// The uncapped sum of every event's contribution
    /// (CalculateRelationshipAbsolute 0xD05A60).
    pub fn absolute(&self, personality: Personality, difficulty: Difficulty) -> f32 {
        self.events
            .iter()
            .map(|(event, x)| {
                Self::contribution(personality.event_row(*event, difficulty), *x, 1.0)
            })
            .sum()
    }

    /// The relationship value the attitude and the thresholds use (+-10).
    pub fn value(&self, personality: Personality, difficulty: Difficulty) -> f32 {
        self.absolute(personality, difficulty)
            .clamp(-RELATIONSHIP_CAP, RELATIONSHIP_CAP)
    }

    /// ApplyRelationship (0xD05DD0): X += scale (inserted when new, erased
    /// when |X| < 0.01); returns the change of the capped value.
    pub fn apply(
        &mut self,
        event: RelationshipEvent,
        scale: f32,
        personality: Personality,
        difficulty: Difficulty,
    ) -> f32 {
        if scale == 0.0 {
            return 0.0;
        }
        let before = self.value(personality, difficulty);
        // 0xD05EE2..0xD05F27 (ApplyRelationship's worker 0xD05DD0): a NEW entry
        // takes the delta as it is, even below 0.01 (a happy ray tick of 0.005
        // starts one); an existing entry adds the delta and is erased when its
        // |X| falls under 0.01.
        match self.events.get_mut(&event) {
            None => {
                self.events.insert(event, scale);
            }
            Some(x) => {
                *x += scale;
                if x.abs() < 0.01 {
                    self.events.remove(&event);
                }
            }
        }
        self.value(personality, difficulty) - before
    }

    /// One decay tick (0xD06070): every X moves toward 0 by |decay| (to 0 when
    /// closer), entries reaching 0 are erased.
    pub fn decay_tick(&mut self, personality: Personality, difficulty: Difficulty) {
        let mut gone = Vec::new();
        for (event, x) in self.events.iter_mut() {
            let decay = personality.event_row(*event, difficulty).decay.abs();
            if decay == 0.0 {
                continue;
            }
            if x.abs() <= decay {
                *x = 0.0;
            } else {
                *x -= decay * x.signum();
            }
            if x.abs() < 0.01 {
                gone.push(*event);
            }
        }
        for event in gone {
            self.events.remove(&event);
        }
    }

    /// First contact: the personality's standing events (PersonalityNice /
    /// PersonalityMean) and the archetype mods for the player's archetype.
    /// ADAPTED: the moment retail applies them is not traced; here once, at
    /// first contact.
    pub fn first_contact(
        &mut self,
        personality: Personality,
        player: SpaceArchetype,
        difficulty: Difficulty,
    ) {
        if self.met {
            return;
        }
        self.met = true;
        for event in [RelationshipEvent::PersonalityNice, RelationshipEvent::PersonalityMean] {
            let r = personality.event_row(event, difficulty);
            if r.initial != 0.0 || r.add != 0.0 {
                self.apply(event, 1.0, personality, difficulty);
            }
        }
        if let Some(i) = archetype_mod_index(player) {
            let m = personality.archetype_mods()[i];
            if m > 0.0 {
                self.apply(RelationshipEvent::ArchetypeNice, m, personality, difficulty);
            } else if m < 0.0 {
                self.apply(RelationshipEvent::ArchetypeMean, -m, personality, difficulty);
            }
        }
    }
}

/// Five attitudes (0xD00BD0) from four thresholds at manager +0x10..+0x1C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attitude {
    Furious,
    Annoyed,
    Cautious,
    Pleased,
    Friendly,
}

impl Attitude {
    pub fn title(self) -> &'static str {
        match self {
            Attitude::Furious => "Furious",
            Attitude::Annoyed => "Annoyed",
            Attitude::Cautious => "Cautious",
            Attitude::Pleased => "Pleased",
            Attitude::Friendly => "Friendly",
        }
    }
}

/// The four attitude thresholds: `RolloverRltnshpVBadCap` -7, `BadCap` -3,
/// `NeutralCap` 3, `GoodCap` 7, read into the relationship manager's +0x10..+0x1C
/// by 0xD00F10 from the global property list 0x5C770DB7 (VERIFIED, space-audit-a
/// and notes	ribal-final-re-notes.md T9K-2).
pub const ATTITUDE_THRESHOLDS: [f32; 4] = [-7.0, -3.0, 3.0, 7.0];

/// Attitude of a (capped) relationship value (0xD00BD0's comparison order).
pub fn attitude(value: f32) -> Attitude {
    let [t0, t1, t2, t3] = ATTITUDE_THRESHOLDS;
    let v = value.clamp(-RELATIONSHIP_CAP, RELATIONSHIP_CAP);
    if t1 < v && v < t2 {
        Attitude::Cautious
    } else if t2 <= v && v < t3 {
        Attitude::Pleased
    } else if t3 <= v {
        Attitude::Friendly
    } else if v <= t1 && v > t0 {
        Attitude::Annoyed
    } else {
        Attitude::Furious
    }
}

/// Diplomacy thresholds (SpaceRelationship 0x86017D53).
pub const WAR_THRESHOLD: f32 = -7.0;
pub const WAR_END_THRESHOLD: f32 = -7.0;
pub const WAR_PASSIVE_THRESHOLD: f32 = -7.0;
pub const WAR_TIME_DELAY: f32 = 1.0;
pub const CREATE_ALLIANCE_THRESHOLD: f32 = 7.0;
pub const CREATE_ALLIANCE_CHANCE: f32 = 1.0;
pub const END_ALLIANCE_THRESHOLD: f32 = 3.0;
pub const HOSTILE_THRESHOLD: f32 = -3.0;
pub const ESCORT_THRESHOLD: f32 = -3.0;
pub const MISSION_THRESHOLD: f32 = -7.0;
pub const TRADE_ROUTE_WARN: f32 = 3.0;
pub const TRADE_ROUTE_END: f32 = 0.0;
pub const GROX_INITIAL_RELATIONSHIP: f32 = 0.0;
pub const EMBASSY_UPDATE_SECONDS: f32 = 30.0;

/// The AI declares war at or below the war threshold.
pub fn declares_war(value: f32) -> bool {
    value <= WAR_THRESHOLD
}

/// A war ends when the relationship climbs above the war-end threshold.
pub fn war_ends(value: f32) -> bool {
    value > WAR_END_THRESHOLD
}

/// An alliance is offered at or above `createAllianceBase`.
pub fn offers_alliance(value: f32) -> bool {
    value >= CREATE_ALLIANCE_THRESHOLD
}

/// An alliance breaks below `endAlliance`.
pub fn alliance_breaks(value: f32) -> bool {
    value < END_ALLIANCE_THRESHOLD
}

/// The empire is hostile (sends escorts) at or below -3.
pub fn is_hostile(value: f32) -> bool {
    value <= HOSTILE_THRESHOLD
}

/// The empire offers missions only above `missionThreshold`.
pub fn offers_missions(value: f32) -> bool {
    value > MISSION_THRESHOLD
}

// ================================================= S3 gifts, tribute, offers

/// A gift the player sends (comm screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftSize {
    Small,
    Medium,
    Large,
}

impl GiftSize {
    pub const ALL: [GiftSize; 3] = [GiftSize::Small, GiftSize::Medium, GiftSize::Large];

    /// `small/medium/largeGiftAmount`.
    pub fn amount(self) -> f32 {
        match self {
            GiftSize::Small => 10_000.0,
            GiftSize::Medium => 50_000.0,
            GiftSize::Large => 100_000.0,
        }
    }

    /// `...GiftRelationshipMultiplier`: the GiveGift event scale.
    pub fn relationship_scale(self) -> f32 {
        match self {
            GiftSize::Small => 0.5,
            GiftSize::Medium => 2.5,
            GiftSize::Large => 5.0,
        }
    }

    /// The largest gift `money` covers.
    pub fn largest_affordable(money: f32) -> Option<GiftSize> {
        Self::ALL.iter().rev().copied().find(|g| money >= g.amount())
    }
}

/// `npcGiftAmount`, `npcTributeAmount`, `npdTributeTimeLimit`.
pub const NPC_GIFT_AMOUNT: f32 = 10_000.0;
pub const NPC_TRIBUTE_AMOUNT: f32 = 20_000.0;
pub const NPC_TRIBUTE_TIME_LIMIT: f32 = 300.0;

/// `min/maxTributeMultiplier` and `min/maxPeaceOfferMultiplier` (11 entries);
/// `_hard` (= plain) rows, `_medium` half, `_easy` quarter. VERIFIED
/// (space-audit-a, 0x10307B0 as used by the peace price 0x1030940, the tribute
/// 0x10407E0 and the verdict thresholds 0x1030AB0 / 0x10309D0 / 0x1030A40): the
/// index is the PLAYER'S rank count (`0xFE4290(cSimulatorSpaceGame.mpBadgeManager)`,
/// 0 = Captain .. 10 = Omnipotent), not the empire's size tier; the value is a
/// random draw between the min and max rows, which are equal in the data.
pub const TRIBUTE_SIZE_MULTIPLIER: [f32; 11] = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 35.0, 55.0, 80.0, 110.0, 150.0];

/// The multiplier for the player's `rank` (0..=10; the parameter was named
/// `tier` while the index was believed to be the empire's size tier).
pub fn size_multiplier(rank: usize, difficulty: Difficulty) -> f32 {
    let base = TRIBUTE_SIZE_MULTIPLIER[rank.min(10)];
    base * match difficulty {
        Difficulty::Easy => 0.25,
        Difficulty::Normal => 0.5,
        Difficulty::Hard => 1.0,
    }
}

/// The tribute an NPC demands (0x10407E0): `npcTributeAmount` x the rank
/// multiplier (the min and max rows are equal, so the draw is exact).
pub fn tribute_amount(rank: usize, difficulty: Difficulty) -> f32 {
    (NPC_TRIBUTE_AMOUNT * size_multiplier(rank, difficulty)).round()
}

/// `peaceOfferPrices`: the five offers the player can make.
pub const PEACE_OFFER_PRICES: [f32; 5] = [40_000.0, 50_000.0, 60_000.0, 70_000.0, 80_000.0];
pub const PEACE_GOOD_OFFER_DELTA: f32 = 10_000.0;
pub const PEACE_BAD_OFFER_DELTA: f32 = -5_000.0;
pub const PEACE_VERY_BAD_OFFER_DELTA: f32 = -11_000.0;
/// `peacePriceRelationshipFactor` (the other peace price factors are 0).
pub const PEACE_PRICE_RELATIONSHIP_FACTOR: f32 = -6_250.0;

/// The price an empire at war asks for peace: -6,250 per relationship point
/// (the size / capture / time factors are 0 in the tuning).
pub fn peace_price(relationship: f32) -> f32 {
    (PEACE_PRICE_RELATIONSHIP_FACTOR * relationship).max(0.0)
}

/// How an offer compares to the asking price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferVerdict {
    Good,
    Fair,
    Bad,
    VeryBad,
}

/// Grade an offer against a price with the good / bad / very bad deltas:
/// at least `good` above the price is good, down to `bad` below it fair, down
/// to `very_bad` below it bad, anything lower very bad.
pub fn grade_offer(offer: f32, price: f32, good: f32, bad: f32, very_bad: f32) -> OfferVerdict {
    let d = offer - price;
    if d >= good {
        OfferVerdict::Good
    } else if d > bad {
        OfferVerdict::Fair
    } else if d > very_bad {
        OfferVerdict::Bad
    } else {
        OfferVerdict::VeryBad
    }
}

/// ADAPTED: a peace offer is accepted unless it is very bad; a good or fair
/// offer applies GoodPeaceOffer, a bad one BadPeaceOffer (both raise the
/// relationship in the tuning).
pub fn peace_offer(offer: f32, relationship: f32) -> OfferVerdict {
    grade_offer(
        offer,
        peace_price(relationship),
        PEACE_GOOD_OFFER_DELTA,
        PEACE_BAD_OFFER_DELTA,
        PEACE_VERY_BAD_OFFER_DELTA,
    )
}

/// Economic capture (Purchase System): `tradeCaptureCostT0..T3`.
pub const SYSTEM_PURCHASE_COST: [f32; 4] = [100_000.0, 400_000.0, 1_200_000.0, 5_000_000.0];
/// `tradeCaptureOfferPrices`.
pub const SYSTEM_PURCHASE_OFFERS: [f32; 5] = [500_000.0, 1_000_000.0, 3_000_000.0, 5_000_000.0, 10_000_000.0];
pub const PURCHASE_GOOD_OFFER_DELTA: f32 = 200_000.0;
pub const PURCHASE_BAD_OFFER_DELTA: f32 = -200_000.0;
pub const PURCHASE_VERY_BAD_OFFER_DELTA: f32 = -400_000.0;

/// Price of buying a system of terraform tier 0..3.
pub fn system_purchase_cost(tier: u8) -> f32 {
    SYSTEM_PURCHASE_COST[(tier as usize).min(3)]
}

/// War and capture (SpaceCombat / spacenpcai).
pub const WAR_SURRENDER_THRESHOLD: f32 = 0.35;
pub const MILITARY_CAPTURE_FACTOR: f32 = 0.5;
pub const MILITARY_CAPTURE_DECAY_SECONDS: f32 = 300.0;
pub const DESTROY_ON_CAPTURE_CHANCE: f32 = 0.2;
pub const OUTPOST_DESTROY_ON_CAPTURE_CHANCE: f32 = 1.0;
/// Attack requests (SpaceEconomy): base 50,000 + 2,000 per building + 3,000 per turret.
pub const ATTACK_REQUEST_BASE_COST: f32 = 50_000.0;
pub const ATTACK_REQUEST_BUILDING_COST: f32 = 2_000.0;
pub const ATTACK_REQUEST_TURRET_COST: f32 = 3_000.0;

// ================================================ S3 awareness, raids, Grox

/// Awareness (UniverseSimulatorTuning).
pub const AWARENESS_THRESHOLDS: [f32; 3] = [1.0, 5.0, 10.0];
pub const AWARENESS_CAP: f32 = 50.0;
pub const EMPIRE_AWARENESS_DECAY: f32 = 0.0;
pub const GROX_AWARENESS_DECAY: f32 = 0.001;
pub const AWARENESS_VISITED_STAR_NEAR: f32 = 1.0;
pub const AWARENESS_ATTACKED_UFO: f32 = 1.0;
pub const AWARENESS_VISITED_STAR: f32 = 1.0;
pub const AWARENESS_ANGERED: f32 = 5.0;
pub const AWARENESS_PLACED_COLONY: f32 = 1.0;
pub const GROX_SHOCK_SECONDS: f32 = 100.0;
pub const GROX_HIGH_AWARENESS_UPDATE_SECONDS: f32 = 300.0;

/// Awareness level: 0 not aware, 1 aware, 2 very aware, 3 extremely aware.
pub fn awareness_level(awareness: f32) -> u8 {
    AWARENESS_THRESHOLDS.iter().filter(|t| awareness >= **t).count() as u8
}

/// Add an awareness bump (x `difficultySpaceNPCAwareness`), capped.
pub fn bump_awareness(awareness: f32, bump: f32, difficulty: Difficulty) -> f32 {
    (awareness + bump * difficulty.pick(DIFFICULTY_NPC_AWARENESS)).min(AWARENESS_CAP)
}

/// The SETI tool (`spacetools~` seti 0x19E07A2C, VERIFIED data):
/// `spaceToolSetiChanceFalsePositiveEmpire` (0x05B07984, a star without an
/// empire signals) and `spaceToolSetiChanceFalseNegativeEmpire` (0x05B07986,
/// an empire star does not), both 0.0 installed.
pub const SETI_FALSE_POSITIVE_EMPIRE: f32 = 0.0;
pub const SETI_FALSE_NEGATIVE_EMPIRE: f32 = 0.0;

/// A SETI star (0xBBA120 / 0xBBA020, VERIFIED): not the player's (star flag
/// 0x10, INFERRED: kStarFlagIsOwned = the player's), and an empire star
/// unless its one-time roll (`roll` in 0..1, stored in the star flags
/// 0x2000 / 0x4000) falls under the false-negative chance, or a star without
/// an empire whose roll falls under the false-positive chance.
pub fn seti_star(empire_star: bool, player_star: bool, roll: f32) -> bool {
    if player_star {
        return false;
    }
    let flip = roll < if empire_star { SETI_FALSE_NEGATIVE_EMPIRE } else { SETI_FALSE_POSITIVE_EMPIRE };
    empire_star != flip
}

/// Universe simulator timers (seconds; UniverseSimulatorTuning).
pub const PIRATE_RAID_MIN_TIME: f32 = 600.0;
pub const PIRATE_RAID_FREQUENCY: f32 = 1200.0;
pub const PIRATE_RAID_ALLY_MIN_TIME: f32 = 300.0;
pub const PIRATE_RAID_ALLY_FREQUENCY: f32 = 900.0;
pub const PIRATE_RAID_PLUNDER_MIN_TIME: f32 = 600.0;
pub const PIRATE_RAID_PLUNDER_FREQUENCY: f32 = 1200.0;
pub const WAR_ATTACK_MIN_TIME: f32 = 300.0;
pub const WAR_ATTACK_FREQUENCY: f32 = 900.0;
pub const GROX_WAR_ATTACK_MIN_TIME: f32 = 240.0;
pub const GROX_WAR_ATTACK_FREQ_EXTREMELY_AWARE: f32 = 120.0;
pub const GROX_WAR_ATTACK_FREQ_AWARE: f32 = 240.0;
pub const GROX_TEASE_ATTACK_MIN_TIME: f32 = 300.0;
pub const GROX_TEASE_ATTACK_FREQUENCY: f32 = 900.0;
pub const BIOSPHERE_COLLAPSE_TIME: f32 = 1200.0;
pub const BIOSPHERE_COLLAPSE_FREQUENCY: f32 = 600.0;
pub const MIN_TIME_BETWEEN_DISASTERS: f32 = 120.0;
pub const MIN_TIME_AFTER_COLONY_PLACEMENT: f32 = 300.0;
pub const HAPPINESS_DISASTER_MIN_TIME: f32 = 90.0;
pub const HAPPINESS_DISASTER_FREQUENCY: f32 = 90.0;
pub const NEW_EVENT_FREQUENCY: f32 = 120.0;
pub const ARTIFACT_FREQUENCY: f32 = 180.0;
/// Raid shape (S2H): 300 s, up to 12 buildings lost, each turret saves one,
/// 3-5 fighters; ally raids wait 60 s.
pub const RAID_DURATION: f32 = 300.0;
pub const RAID_MAX_BUILDINGS_LOST: u32 = 12;
/// The mission property 0x039787B3 of `raidally` (02AE1A8E!A4B0A2DF's file,
/// "planetary raid for player allies event"): the size of the MissionComplete
/// event 0xC5BB60 applies to the raided planet's owner when the raid is
/// completed (`raidwar` 2, `raidwarplayer` 0; the default raid event has none).
pub const RAID_ALLY_COMPLETE_RELATION: f32 = 3.0;
pub const RAID_FIGHTERS: (u32, u32) = (3, 5);
pub const ALLY_RAID_WAIT: f32 = 60.0;

/// One raid mission row (`RaidEvent` class, vtable 0x146F828; the properties
/// are read by the raid class and its Update 0xC5E410, VERIFIED): the raid
/// arrives after `wait` seconds (`missionRaidAllyWaitDuration` 0x03A38ABE,
/// default 10; 60 in every raid row) or at once when the player is on the
/// planet; raiders still alive `wait + duration` seconds after the mission
/// started (`missionRaidDuration` 0x02B9E524) fail it (0xC5E7E2..0xC5E839),
/// raiders all gone complete it. `leave_on_arrival`
/// (`missionRaidUFOsLeaveOnArrival` 0x039787B4): the UFOs leave
/// [`RAID_LEAVE_AFTER`] seconds after the player entered (0xC5E64C..0xC5E66E)
/// and the raid counts as completed without the completion rewards (0xC5BB66).
/// `buildings_destroyed` / `saved_per_turret` are `missionRaidBuildingsDestroyed`
/// / `missionRaidBuildingsSavedPerTurret`; `ally_fail_delta` is
/// `missionRaidAllyFailRelationshipDelta` 0x039787AE (0xC5BC50: the MissionFailed
/// event of |delta| for an allied owner when the player was away).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaidRow {
    pub id: &'static str,
    pub wait: f32,
    pub duration: f32,
    pub min_fighters: u32,
    pub max_fighters: u32,
    pub leave_on_arrival: bool,
    pub buildings_destroyed: u32,
    pub saved_per_turret: u32,
    pub ally_fail_delta: f32,
}

/// `raidwar` (the war attacks of the universe simulator).
pub const RAID_ROW_WAR: RaidRow = RaidRow { id: "raidwar", wait: 60.0, duration: 300.0, min_fighters: 3, max_fighters: 5, leave_on_arrival: false, buildings_destroyed: 12, saved_per_turret: 1, ally_fail_delta: -1.0 };
/// `grobwarraid` (the Grox war attacks): 600 s, 3 to 10 fighters, no
/// `missionRaidAllyWaitDuration` in the row (the property's default, 10 s).
pub const RAID_ROW_GROX_WAR: RaidRow = RaidRow { id: "grobwarraid", wait: 10.0, duration: 600.0, min_fighters: 3, max_fighters: 10, leave_on_arrival: false, buildings_destroyed: 12, saved_per_turret: 1, ally_fail_delta: 0.0 };
/// `grobteaserraid` (the Grox tease attack): 900 s, the UFOs leave on arrival,
/// no building is destroyed; no `missionRaidAllyWaitDuration` in the row (10 s).
pub const RAID_ROW_GROX_TEASER: RaidRow = RaidRow { id: "grobteaserraid", wait: 10.0, duration: 900.0, min_fighters: 3, max_fighters: 5, leave_on_arrival: true, buildings_destroyed: 0, saved_per_turret: 0, ally_fail_delta: 0.0 };
/// `raidally` (a raid on an ally's planet): fail delta -2, complete +3.
pub const RAID_ROW_ALLY: RaidRow = RaidRow { id: "raidally", wait: 60.0, duration: 300.0, min_fighters: 3, max_fighters: 5, leave_on_arrival: false, buildings_destroyed: 12, saved_per_turret: 1, ally_fail_delta: -2.0 };
/// 0x1576768 (a .data tuning value read at 0xC5E668): 0x61A8 ms.
pub const RAID_LEAVE_AFTER: f32 = 25.0;

impl RaidRow {
    /// Fighters of a raid for a roll in 0..1 (`missionRaidMinFighters` ..
    /// `missionRaidMaxFighters`, 0xC5C4C0).
    pub fn fighters(&self, roll: f32) -> u32 {
        let span = self.max_fighters - self.min_fighters + 1;
        self.min_fighters + ((roll.clamp(0.0, 0.999_999) * span as f32) as u32).min(span - 1)
    }

    /// Seconds from the start of the mission to its failure deadline.
    pub fn deadline(&self) -> f32 {
        self.wait + self.duration
    }

    /// Buildings the raid destroys on a planet defended by `turrets`.
    pub fn buildings_lost(&self, turrets: u32) -> u32 {
        self.buildings_destroyed.saturating_sub(turrets * self.saved_per_turret)
    }
}
/// Galaxy intercepts (range 3, fire range 3, damage 30).
pub const GALAXY_INTERCEPT_RANGE: f32 = 3.0;
pub const GALAXY_INTERCEPT_DAMAGE: f32 = 30.0;

/// Seconds until the next pirate raid: the first after `min_time`, then every
/// `frequency`. VERIFIED (space-audit-a, 0x100F080 and the three sibling
/// timers 0x100F280 / 0x100F340 / 0x100F8xx): the card multiplier
/// (`SuperPowerRaidFrequencyMultiplier` 3.0, bit 0x200 through 0x1005B00(9)) is
/// multiplied into the *frequency* property
/// (`universeSimulatorPirateRaidFrequency`), which is the denominator of the
/// per-frame hazard `1 - exp(-0.00030103 dt_ms / frequency')`; so the civ-military
/// card makes raids three times RARER (interval x 3), and the `MinTime` wait
/// before the first raid is not scaled.
/// ADAPTED: the port keeps a fixed interval instead of retail's random draw
/// (the hazard also scales with a list of the player's holdings, +0x30 of the
/// space game's cached event data, which is not modelled).
pub fn pirate_raid_interval(first: bool, flags: u32) -> f32 {
    if first {
        return PIRATE_RAID_MIN_TIME;
    }
    if flags & superpower_bit::CIV_MILITARY != 0 {
        PIRATE_RAID_FREQUENCY * SUPERPOWER_RAID_FREQUENCY_MULTIPLIER
    } else {
        PIRATE_RAID_FREQUENCY
    }
}

/// Seconds between bio-disasters (first after 1200 s, then every 600 s).
/// VERIFIED (0x100F8E7): the civ-religious card (bit 0x800, 0x1005B00(11),
/// `SuperPowerBioDisasterFrequencyMultiplier` 3.0) multiplies
/// `universeSimulatorBiosphereCollapseFrequency`, so disasters are three times
/// rarer; the first wait is not scaled.
pub fn biosphere_collapse_interval(first: bool, flags: u32) -> f32 {
    if first {
        return BIOSPHERE_COLLAPSE_TIME;
    }
    if flags & superpower_bit::CIV_RELIGIOUS != 0 {
        BIOSPHERE_COLLAPSE_FREQUENCY * SUPERPOWER_BIO_DISASTER_FREQUENCY_MULTIPLIER
    } else {
        BIOSPHERE_COLLAPSE_FREQUENCY
    }
}

/// Seconds between Grox war attacks by awareness level (None when unaware).
pub fn grox_attack_interval(awareness: f32) -> Option<f32> {
    match awareness_level(awareness) {
        0 => None,
        1 | 2 => Some(GROX_WAR_ATTACK_FREQ_AWARE),
        _ => Some(GROX_WAR_ATTACK_FREQ_EXTREMELY_AWARE),
    }
}

/// Grox (SpaceSolarSystem / UniverseSimulatorTuning): one empire of 2,400
/// stars within 105 units of the core (Grox-only radius 100).
pub const GROX_STARS: u32 = 2400;
pub const GROX_RADIUS: f32 = 105.0;
pub const GROX_ONLY_RADIUS: f32 = 100.0;

// ============================================================ S3 empire AI

/// NPC empire AI (spacenpcai~).
pub const EMPIRE_RADIUS: f32 = 20.0;
pub const EMPIRE_RADIUS_VARIANCE: f32 = 10.0;
pub const HOME_STAR_BUFFER: f32 = 5.0;
pub const EMPIRE_GROWTH_STEP_SECONDS: f32 = 6.0;
pub const EMPIRE_GROWTH_STEP_RANGE: (f32, f32) = (5.0, 10.0);
pub const EMPIRE_GROWTH_MAX_PROBABILITY: f32 = 0.75;
pub const EMPIRE_EVOLUTION_MEAN_SECONDS: f32 = 120.0;
/// Self-colour and relationship weights of the action rules.
pub const AI_SELF_COLOR_WEIGHTS: [f32; 3] = [0.0, 5.0, 10.0];
pub const AI_RELATIONSHIP_WEIGHTS: [f32; 9] = [-10.0, -10.0, -10.0, -6.0, -6.0, -6.0, -1.0, -1.0, -1.0];

/// ADAPTED: one empire growth step (every 6 s of universe time): the chance an
/// empire adds a star is the max probability scaled down as the empire nears
/// its radius budget (the rule evaluation itself is not traced, S8).
pub fn empire_grows(stars: u32, budget: u32, roll: f32) -> bool {
    if stars >= budget {
        return false;
    }
    let fill = stars as f32 / budget.max(1) as f32;
    roll < EMPIRE_GROWTH_MAX_PROBABILITY * (1.0 - fill)
}

// ============================================================ S3 comm screen

/// Comm-screen ids the HUD needs (CommScreen-3 0x85498E7D, S3E).
pub mod comm {
    pub const TAB_TRADE: u32 = 0x057D_F5CA;
    pub const TAB_REPAIR: u32 = 0x057D_F5C8;
    pub const TAB_RECHARGE: u32 = 0x057D_F5C9;
    pub const REPAIR_PRICE: u32 = 0x066F_C580;
    pub const RECHARGE_PRICE: u32 = 0x066F_C590;
    pub const ACCEPT: u32 = 0x05E4_F788;
    pub const PRICE: u32 = 0x04C1_B588;
    pub const ADD_ONE: u32 = 0x04C1_510F;
    pub const ADD_MANY: u32 = 0x05DB_90D0;
    pub const REMOVE_ONE: u32 = 0x04C1_510D;
    pub const REMOVE_MANY: u32 = 0x05DB_90D1;
    pub const COUNT: u32 = 0x04C1_510E;
    pub const BUY_ITEM: u32 = 0x04C1_510A;
    pub const SELL_ITEM: u32 = 0x04C1_510C;
    pub const PLAYER_INVENTORY: u32 = 0x05DA_83B9;
    pub const NPC_INVENTORY: u32 = 0x05DA_83B8;
    pub const MESSAGE: u32 = 0x04D1_49A8;
    pub const NATION_NAME: u32 = 0x0493_82E8;
}

// ================================================================ systems

/// Registers the Space rules systems that act on the ship entity: owed
/// damage, destruction and restore, the maximum-health model, retail NPC
/// health, Story 201's target star and the Flight 101 home tether.
pub struct SpaceRulesPlugin;

impl bevy::prelude::Plugin for SpaceRulesPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        use bevy::prelude::*;
        app.add_systems(
            Update,
            (retail_npc_health, apply_ship_rules)
                .chain()
                .after(crate::space::space_campaign_update)
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(crate::stages::GameStage::Space)),
        );
    }
}

/// ADAPTED: without the Interplanetary Drive (Flight 101 not finished) the
/// UFO stays near its home planet, this far beyond the planet's radius
/// (retail keeps it in the home planet's atmosphere).
pub const HOME_TETHER_RANGE: f32 = 12.0;

/// New hostile ships get the retail health: pirates `UFOHealthPirate` by
/// tier (from the player's rank), Grox `UFOHealthInterceptor` at the top tier,
/// x `difficultySpaceEnemyHealth`.
pub fn retail_npc_health(
    run: bevy::prelude::Res<crate::game::RunProgress>,
    mut ships: bevy::prelude::Query<&mut crate::space::Pirate, bevy::prelude::Added<crate::space::Pirate>>,
) {
    let campaign = &run.space_campaign;
    let scale = campaign.difficulty().pick(DIFFICULTY_ENEMY_HEALTH);
    for mut ship in &mut ships {
        let hp = if ship.is_grox {
            UFO_HEALTH_INTERCEPTOR[5]
        } else {
            UFO_HEALTH_PIRATE[pirate_tier(campaign.captain_rank())]
        } * scale;
        ship.hp = hp;
        ship.max = hp;
    }
}

/// The ship side of the rules: health maximum (0xFFD780), damage owed by an
/// empty-reactor jump, destruction (0xC3C680: cinematic, restore at the
/// homeworld, no game over), full restore (core), Story 201's star, tether.
pub fn apply_ship_rules(
    mut run: bevy::prelude::ResMut<crate::game::RunProgress>,
    view: Option<bevy::prelude::Res<crate::space_view::SpaceView>>,
    galaxy: Option<bevy::prelude::Res<crate::space_galaxy::Galaxy>>,
    mut ship: bevy::prelude::Query<
        (&mut bevy::prelude::Transform, &mut crate::game::Health),
        bevy::prelude::With<crate::game::Player>,
    >,
    worlds: bevy::prelude::Query<
        (&bevy::prelude::Transform, &crate::game::Planet, &crate::persistence::WorldId),
        bevy::prelude::Without<crate::game::Player>,
    >,
) {
    use bevy::prelude::*;
    let in_galaxy = view.as_ref().is_some_and(|v| v.galaxy);
    let campaign = &mut run.space_campaign;
    if campaign.tutorial.step == TutorialStep::Story201 && campaign.tutorial.story_star.is_none() {
        if let Some(galaxy) = galaxy.as_ref() {
            // `missionPickClosestStar`: the star nearest home.
            let home = crate::space_galaxy::HOME_STAR;
            campaign.tutorial.story_star = (0..galaxy.stars.len() as u32)
                .filter(|i| *i != home)
                .min_by(|a, b| galaxy.distance(home, *a).total_cmp(&galaxy.distance(home, *b)));
        }
    }
    let Ok((mut transform, mut health)) = ship.single_mut() else {
        return;
    };
    let max = campaign.max_hull();
    if (health.max - max).abs() > 0.5 {
        health.max = max;
        health.hp = health.hp.min(max);
    }
    if campaign.pending_damage > 0.0 {
        health.hurt(campaign.pending_damage);
        campaign.pending_damage = 0.0;
    }
    let destroyed = health.dead();
    if destroyed {
        campaign.on_ship_destroyed(in_galaxy);
    }
    if let Some(owner) = campaign.repair_request.take() {
        let run_ref: &mut crate::game::RunProgress = &mut run;
        let spent = run_ref
            .space_campaign
            .buy_repair_recharge(owner, &mut health, &mut run_ref.spice);
        if spent > 0.0 {
            run_ref.request_save();
        }
    }
    let campaign = &mut run.space_campaign;
    if campaign.pending_restore {
        campaign.pending_restore = false;
        health.hp = health.max;
        campaign.energy = campaign.max_reactor_energy();
        run.request_save();
    }
    let campaign = &run.space_campaign;
    let owned_world = |id: u32| id == 0 || campaign.colonies.contains(&id);
    if destroyed {
        if let Some((wt, planet, _)) = worlds
            .iter()
            .filter(|(_, _, id)| owned_world(id.0))
            .min_by_key(|(_, _, id)| id.0)
        {
            let out = wt.translation.normalize_or(Vec3::Z);
            transform.translation = wt.translation - out * (planet.radius + 4.5) + Vec3::Y * 3.0;
        }
    }
    // Flight 101 tether: the home planet until the interplanetary drive.
    if !campaign.has_tool(tool::INTERPLANETARY_DRIVE)
        && campaign.current_system == crate::space_galaxy::HOME_STAR
    {
        if let Some((wt, planet, _)) = worlds.iter().find(|(_, _, id)| id.0 == 0) {
            let reach = planet.radius + HOME_TETHER_RANGE;
            let offset = transform.translation - wt.translation;
            if offset.length() > reach {
                transform.translation = wt.translation + offset.normalize_or(Vec3::Z) * reach;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn tool_energy_is_per_use_and_per_second_only_without_auto_fire() {
        // 0x105040C..0x10504A1: `AutoFireRate x 1000` truncating to 0 scales the
        // cost by the frame time; every other tool pays its cost per use.
        assert_eq!(tool_use_energy(0.25, 0.1, 0.016), 0.25);
        assert_eq!(tool_use_energy(0.5, 1.0, 0.016), 0.5);
        assert_eq!(tool_use_energy(20.0, -1.0, 0.016), 20.0);
        assert!(close(tool_use_energy(2.0, 0.0, 0.5), 1.0));
        // The gate: a reactor holding exactly the cost can use the tool.
        assert!(tool_can_afford_energy(0.5, 0.5));
        assert!(!tool_can_afford_energy(0.49, 0.5));
    }

    #[test]
    fn a_new_relationship_event_keeps_a_delta_under_one_hundredth() {
        // 0xD05EE2..0xD05F27: a new entry takes the delta whatever its size; only
        // an existing entry that falls under 0.01 is erased. A held happy ray
        // (+0.005 a tick) therefore accumulates.
        let (p, d) = (Personality::Default, Difficulty::Normal);
        let mut rel = Relationship::default();
        rel.apply(RelationshipEvent::GoodToolUse, 0.005, p, d);
        assert!(close(rel.events[&RelationshipEvent::GoodToolUse], 0.005));
        rel.apply(RelationshipEvent::GoodToolUse, 0.005, p, d);
        rel.apply(RelationshipEvent::GoodToolUse, 0.005, p, d);
        assert!(close(rel.events[&RelationshipEvent::GoodToolUse], 0.015));
        // Cancelling an entry down under 0.01 erases it.
        rel.apply(RelationshipEvent::GoodToolUse, -0.01, p, d);
        assert!(!rel.events.contains_key(&RelationshipEvent::GoodToolUse));
    }

    #[test]
    fn the_hold_clock_repeats_at_the_auto_fire_rate_and_stops_on_release() {
        let mut clock = ToolFireClock::default();
        let dt = 0.01;
        assert_eq!(clock.step(0.1, true, true, dt), ToolFire::First);
        clock.commit(0.0);
        let mut repeats = 0;
        for _ in 0..100 {
            if clock.step(0.1, true, false, dt) == ToolFire::Repeat {
                clock.commit(0.0);
                repeats += 1;
            }
        }
        assert!((9..=10).contains(&repeats), "one repeat per 0.1 s over a second: {repeats}");
        // Releasing ends the hold; the next press is a first use again.
        assert_eq!(clock.step(0.1, false, false, dt), ToolFire::None);
        assert_eq!(clock.step(0.1, true, true, dt), ToolFire::First);
        // A refused use (no commit) leaves the timers, so it is asked again.
        assert_eq!(clock.step(0.1, true, false, dt), ToolFire::First);
    }

    #[test]
    fn the_tool_table_has_the_retail_rows() {
        assert_eq!(tool_use("heatray").energy, 0.5);
        assert_eq!(tool_use("laserlevel3").energy, 0.75);
        assert_eq!(tool_use("happyraylevel1").relationship, 0.005);
        assert_eq!(tool_use("planetbusterbomb").recharge, 15.0);
        assert_eq!(tool_use("planetbusterbomb").relationship, -20.0);
        // Unknown tools are one use per press, no energy.
        assert_eq!(tool_use("no such tool").autofire, -1.0);
        assert_eq!(ray_velocity("coldray"), Some((0.0, -0.01)));
        assert_eq!(ray_velocity("cloudvacuum"), Some((-0.01, 0.0)));
        assert_eq!(ray_velocity("laserlevel1"), None);
    }

    #[test]
    fn difficulty_tables_index_easy_normal_hard() {
        assert_eq!(Difficulty::from_index(0), Difficulty::Easy);
        assert_eq!(Difficulty::from_index(1), Difficulty::Normal);
        assert_eq!(Difficulty::from_index(2), Difficulty::Hard);
        assert_eq!(Difficulty::Hard.pick(DIFFICULTY_PLAYER_HEALTH), 0.3);
        assert_eq!(Difficulty::Easy.pick(DIFFICULTY_MISSION_TIME), 2.0);
        assert_eq!(Difficulty::Hard.pick(DIFFICULTY_SPICE_PRODUCTION), 0.8);
    }

    #[test]
    fn a_new_space_game_starts_with_the_retail_purse_and_loadout() {
        assert_eq!(STARTING_MONEY, 100_000.0);
        let tools = starting_tools();
        for t in ["radar", "scan", "laserlevel1", "abduct", "dropcargo", "healthupgrade1", "energyupgrade1"] {
            assert!(tools.iter().any(|x| x == t), "{t}");
        }
        assert!(!tools.iter().any(|x| x == tool::INTERSTELLAR_DRIVE));
        assert!(!tools.iter().any(|x| x == tool::INTERPLANETARY_DRIVE));
        assert_eq!(DECLINE_101_TOOLS, &["interplanetarydrive"]);
        assert_eq!(STORY201_UNLOCK_TOOLS, &["interstellardrive", "protonmissilelevel1"]);
        assert_eq!(CORE_REWARD_TOOLS.len(), 5);
        assert_eq!(core_reward_grants(), vec![(tool::GENESIS_DEVICE, 42)]);
    }

    #[test]
    fn the_card_table_matches_the_retail_archetypes() {
        use CardColor::*;
        let c = |cell, creature, tribe, civ| StageCards { cell, creature, tribe, civ };
        let a = |cards| archetype_from_cards(cards);
        assert_eq!(a(c(None, None, None, None)), SpaceArchetype::Wanderer);
        assert_eq!(a(c(Some(Red), Some(Red), Some(Red), Some(Red))), SpaceArchetype::Warrior);
        assert_eq!(a(c(Some(Blue), Some(Blue), Some(Blue), Some(Blue))), SpaceArchetype::Trader);
        assert_eq!(a(c(Some(Green), Some(Green), Some(Green), Some(Green))), SpaceArchetype::Shaman);
        assert_eq!(a(c(Some(Red), Some(Blue), Some(Red), Some(Blue))), SpaceArchetype::Scientist);
        assert_eq!(a(c(Some(Red), Some(Green), Some(Green), Some(Red))), SpaceArchetype::Zealot);
        assert_eq!(a(c(Some(Blue), Some(Green), Some(Blue), Some(Green))), SpaceArchetype::Diplomat);
        assert_eq!(a(c(Some(Red), Some(Red), Some(Blue), Some(Green))), SpaceArchetype::Knight);
        assert_eq!(a(c(Some(Blue), Some(Red), Some(Blue), Some(Green))), SpaceArchetype::Bard);
        assert_eq!(a(c(Some(Green), Some(Red), Some(Blue), Some(Green))), SpaceArchetype::Ecologist);
        // 3 + 1 -> the majority's archetype.
        assert_eq!(a(c(Some(Red), Some(Blue), Some(Red), Some(Red))), SpaceArchetype::Warrior);
        assert_eq!(a(c(Some(Green), Some(Blue), Some(Blue), Some(Blue))), SpaceArchetype::Trader);
        // Civ only.
        assert_eq!(a(c(None, None, None, Some(Red))), SpaceArchetype::Warrior);
        assert_eq!(a(c(None, None, None, Some(Blue))), SpaceArchetype::Trader);
        assert_eq!(a(c(None, None, None, Some(Green))), SpaceArchetype::Shaman);
        // Retail quirk: one of each colour on three stages -> Warrior.
        assert_eq!(a(c(Some(Blue), Some(Green), Some(Red), None)), SpaceArchetype::Warrior);
        // Every 4-card combination with all colours present yields a 4-card archetype.
        for cell in [Red, Blue, Green] {
            for creature in [Red, Blue, Green] {
                for tribe in [Red, Blue, Green] {
                    for civ in [Red, Blue, Green] {
                        assert_ne!(a(c(Some(cell), Some(creature), Some(tribe), Some(civ))), SpaceArchetype::Wanderer);
                    }
                }
            }
        }
    }

    #[test]
    fn cards_set_the_superpower_bits() {
        use superpower_bit::*;
        let cards = StageCards {
            cell: Some(CardColor::Red),
            creature: Some(CardColor::Blue),
            tribe: Some(CardColor::Green),
            civ: Some(CardColor::Blue),
        };
        assert_eq!(superpower_flags(cards), CELL_CARNIVORE | CREATURE_MIX | TRIBE_SOCIAL | CIV_ECONOMIC);
        assert_eq!(superpower_flags(StageCards::default()), 0);
        assert_eq!(speed_multiplier(CREATURE_MIX), 2.0);
        assert_eq!(speed_multiplier(0), 1.0);
    }

    #[test]
    fn superpowers_have_the_retail_recharge_and_the_wanderer_none() {
        let r = |a| archetype_superpower(a).map(|t| t.recharge_seconds);
        assert_eq!(r(SpaceArchetype::Bard), Some(600.0));
        assert_eq!(r(SpaceArchetype::Diplomat), Some(1200.0));
        assert_eq!(r(SpaceArchetype::Ecologist), Some(1200.0));
        assert_eq!(r(SpaceArchetype::Knight), Some(120.0));
        assert_eq!(r(SpaceArchetype::Scientist), Some(1200.0));
        assert_eq!(r(SpaceArchetype::Shaman), Some(60.0));
        assert_eq!(r(SpaceArchetype::Trader), Some(1200.0));
        assert_eq!(r(SpaceArchetype::Warrior), Some(1200.0));
        assert_eq!(r(SpaceArchetype::Zealot), Some(2400.0));
        assert_eq!(r(SpaceArchetype::Wanderer), None);
        let sci = archetype_superpower(SpaceArchetype::Scientist).unwrap();
        assert_eq!((sci.relationship_delta, sci.galactic_relationship_delta), (-20.0, -4.0));
        let zea = archetype_superpower(SpaceArchetype::Zealot).unwrap();
        assert_eq!((zea.relationship_delta, zea.galactic_relationship_delta), (-10.0, -7.0));
    }

    #[test]
    fn the_tutorial_runs_flight101_then_story201_then_teaching_missions() {
        let mut step = TutorialStep::default();
        let mut seen = vec![step];
        while step != TutorialStep::Done {
            step = step.next();
            seen.push(step);
        }
        assert_eq!(seen, TutorialStep::ORDER.to_vec());
        assert_eq!(TutorialStep::Flight101EradicateAnimals.target(), 5);
        assert_eq!(TutorialStep::Flight101FetchAnimal.tools_on_accept(), &["dropcargo", "abduct"]);
        assert_eq!(TutorialStep::Flight101EradicateAnimals.tools_on_accept(), &["laserlevel1"]);
        assert_eq!(TutorialStep::Story201.tools_on_accept(), STORY201_UNLOCK_TOOLS);
        assert_eq!(TutorialStep::TeachColonize.reward_money(), 10_000.0);
        assert_eq!(TutorialStep::TeachTradeSpice.reward_money(), 5_000.0);
        assert!(TutorialStep::Flight101Scan.is_flight101());
        assert!(!TutorialStep::Story201.is_flight101());
        assert_eq!(TutorialStep::Done.next(), TutorialStep::Done);
    }

    #[test]
    fn mission_rewards_and_limits_follow_the_retail_multipliers() {
        assert_eq!(mission_row(MissionKind::FetchAnimal).money, 15_000.0);
        assert_eq!(mission_row(MissionKind::War).money, 200_000.0);
        assert_eq!(mission_row(MissionKind::Terraform).money, 400_000.0);
        assert_eq!(mission_row(MissionKind::DestroyTurrets).duration, 300.0);
        // Giver tier 2 (x2), rank 3 Admiral (x1.2), normal difficulty.
        assert!(close(mission_reward(MissionKind::FetchAnimal, 2, 3, Difficulty::Normal), 36_000.0));
        assert!(close(mission_reward(MissionKind::War, 5, 10, Difficulty::Easy), 200_000.0 * 5.0 * 5.0));
        // Durations: 180 x 0.8 x 0.95 x 1, easy doubles, hard 0.75.
        assert!(close(mission_duration(MissionKind::FetchAnimal, 2, 3, Difficulty::Normal).unwrap(), 136.8));
        assert!(close(mission_duration(MissionKind::FetchAnimal, 0, 0, Difficulty::Easy).unwrap(), 360.0));
        assert!(close(mission_duration(MissionKind::FetchAnimal, 0, 0, Difficulty::Hard).unwrap(), 135.0));
        assert_eq!(mission_duration(MissionKind::Terraform, 0, 0, Difficulty::Normal), None);
        assert_eq!((MISSION_REJECT_RELATION, MISSION_ABORT_RELATION, MISSION_FAIL_RELATION), (-0.5, -1.2, -1.5));
    }

    #[test]
    fn mission_kinds_are_picked_by_the_generator_weights() {
        let warrior = Personality::Warrior.mission_weights();
        assert_eq!(warrior[2], 100);
        assert_eq!(warrior[3], 100);
        assert_eq!(warrior[8], 100);
        let mut only = [0u32; 16];
        only[10] = 5;
        assert_eq!(pick_mission_kind(&only, 0.0), MissionKind::FetchAnimal);
        assert_eq!(pick_mission_kind(&only, 0.99), MissionKind::FetchAnimal);
        assert_eq!(pick_mission_kind(&[0; 16], 0.5), MissionKind::FetchArtifact);
        let weights = Personality::Default.mission_weights();
        assert_eq!(pick_mission_kind(&weights, 0.0), MissionKind::FetchArtifact);
    }

    #[test]
    fn badges_and_ranks_follow_the_retail_tables() {
        let colonist = badge_family("colonist").unwrap();
        assert_eq!(badge_level(colonist, 4), 0);
        assert_eq!(badge_level(colonist, 5), 1);
        assert_eq!(badge_level(colonist, 200), 5);
        assert_eq!(badge_points(colonist, 2), 6);
        assert_eq!(badge_points(colonist, 5), 45);
        let traveler = badge_family("traveler").unwrap();
        assert_eq!(badge_points(traveler, 5), 15);
        assert_eq!(rank_from_points(4), 0);
        assert_eq!(rank_from_points(5), 1);
        assert_eq!(rank_from_points(274), 9);
        assert_eq!(rank_from_points(275), 10);
        assert_eq!(rank_title(0), "Captain");
        assert_eq!(rank_title(1), "Commander");
        assert_eq!(rank_title(10), "Omnipotent");
        assert_eq!(rank_reward_multiplier(0), 1.0);
        assert_eq!(rank_reward_multiplier(4), 1.4);
        assert_eq!(rank_reward_multiplier(10), 5.0);
        assert_eq!(rank_duration_multiplier(10), 0.5);
        assert!(close(rank_progress(10), 0.5));
        assert_eq!(rank_progress(300), 1.0);
        assert_eq!(posse_limit(0), 0);
        assert_eq!(posse_limit(1), 0);
        assert_eq!(posse_limit(2), 1);
        assert_eq!(posse_limit(9), 4);
        assert_eq!(posse_limit(10), 5);
        // The code-driven families carry their data requirements too.
        assert_eq!(badge_family("frequentflyer").unwrap().thresholds, [50, 150, 400, 800, 1500]);
        assert_eq!(badge_family("empire").unwrap().thresholds[0], 3);
        assert_eq!(badge_family("conquerer").unwrap().points, [2, 4, 7, 12, 20]);
        assert_eq!(BADGE_FAMILIES.len(), 26);
    }

    #[test]
    fn health_and_energy_follow_the_upgrade_level_difficulty_and_cards() {
        let tools: Vec<String> = starting_tools();
        assert_eq!(health_level(&tools), 1.0);
        assert_eq!(energy_level(&tools), 1.0);
        assert_eq!(player_max_health(1.0, Difficulty::Normal, 0), 1000.0);
        assert_eq!(player_max_health(1.0, Difficulty::Easy, 0), 1500.0);
        assert!(close(player_max_health(1.0, Difficulty::Hard, 0), 300.0));
        assert_eq!(player_max_health(3.0, Difficulty::Normal, superpower_bit::CREATURE_ATTACK), 4500.0);
        assert_eq!(player_max_energy(1.0, 0), 100.0);
        assert_eq!(player_max_energy(2.5, superpower_bit::CELL_CARNIVORE), 375.0);
        let upgraded = vec!["healthupgrade3".to_string(), "energyupgrade4".to_string()];
        assert_eq!(health_level(&upgraded), 3.0);
        assert_eq!(energy_level(&upgraded), 2.5);
        assert_eq!(passive_energy_recharge(100.0, 60.0, 1.0), 0.0);
    }

    #[test]
    fn repair_and_recharge_cost_money_per_point_by_owner() {
        assert_eq!(PlanetOwner::Homeworld.cost_index(), 0);
        assert_eq!(PlanetOwner::Hostile.cost_index(), 5);
        assert_eq!(RECHARGE_COST[PlanetOwner::Colony.cost_index()], 10.0);
        assert_eq!(REPAIR_COST[PlanetOwner::Neutral.cost_index()], 15.0);
        assert_eq!(repair_purchase(40.0, 0.0, 0.0), (40.0, 0.0));
        assert_eq!(repair_purchase(40.5, 10.0, 1_000.0), (40.5, 405.0));
        // Only the affordable part is bought.
        assert_eq!(repair_purchase(100.0, 31.0, 100.0), (3.0, 93.0));
        assert_eq!(repair_purchase(0.0, 31.0, 100.0), (0.0, 0.0));
    }

    #[test]
    fn jumps_cost_five_energy_or_damage_and_the_drive_sets_the_range() {
        assert_eq!(jump_cost(100.0), (5.0, 0.0));
        assert_eq!(jump_cost(3.0), (3.0, 0.0));
        assert_eq!(jump_cost(0.0), (0.0, 200.0));
        assert_eq!(jump_money_cost(12.0, true, false), 0.0);
        let none: Vec<String> = starting_tools();
        assert_eq!(drive_range(&none), 0.0);
        let mut tools = none.clone();
        tools.push("interstellardrive".into());
        assert_eq!(drive_range(&tools), 3.0);
        tools.push("interstellardrive_upgrade4".into());
        assert_eq!(drive_range(&tools), 20.0);
        assert_eq!(DRIVES.map(|d| d.1), [3.0, 5.0, 8.0, 12.0, 20.0]);
    }

    #[test]
    fn weapons_npc_ships_and_loot_follow_the_tables() {
        assert_eq!(laser(1).damage, 40.0);
        assert_eq!(laser(3).energy, 0.75);
        assert_eq!(laser(9).id, "laserlevel3");
        assert_eq!(laser_level(&["laserlevel2".to_string()]), 2);
        assert_eq!(laser_level::<String>(&[]), 1);
        assert_eq!(empire_tier(0), 0);
        assert_eq!(empire_tier(6), 2);
        assert_eq!(empire_tier(13), 4);
        assert_eq!(empire_tier(2400), 5);
        assert_eq!(npc_laser_dps(0), 15.0);
        assert_eq!(npc_laser_dps(9), 120.0);
        assert!(close(beam_damage_per_tick(40.0, 0.1), 4.0));
        assert!(npc_laser_on(0.5) && npc_laser_on(2.9) && !npc_laser_on(3.5) && npc_laser_on(4.2));
        assert_eq!(ship_loot(LootShip::Raider, 0, 0.3, 0.5), None);
        assert_eq!(ship_loot(LootShip::Raider, 0, 0.1, 0.0), Some(1000.0));
        assert_eq!(ship_loot(LootShip::Thief, 5, 0.69, 1.0), Some(50_000.0));
        assert_eq!(city_loot(0, 0.5), 45_000.0);
        assert_eq!(pirate_tier(0), 0);
        assert_eq!(pirate_tier(5), 1);
        assert_eq!(pirate_tier(10), 2);
    }

    #[test]
    fn store_and_spice_prices_follow_the_tables() {
        assert_eq!(store_price("placecolony"), Some(150_000.0));
        assert_eq!(store_price("healthupgrade2"), Some(150_000.0));
        assert_eq!(store_price("laserlevel3"), Some(1_500_000.0));
        assert_eq!(store_price("interstellardrive_upgrade4"), Some(2_000_000.0));
        assert_eq!(store_price("planetbusterbomb"), Some(5_000_000.0));
        assert_eq!(store_price("nonexistent"), None);
        let colony = store_item_for_tool("placecolony").unwrap();
        assert!(colony.consumable);
        assert_eq!(colony.requires, Some("interplanetarydrive"));
        assert_eq!(store_price_at(150_000.0, 0.65, Difficulty::Normal, false), 97_500.0);
        assert_eq!(store_price_at(100_000.0, 1.0, Difficulty::Normal, true), 80_000.0);
        assert_eq!(SPICE_BASE_PRICE, [3375.0, 4500.0, 5230.0, 10125.0, 11242.0, 14625.0]);
        assert_eq!(spice_sell_price(3375.0, Difficulty::Normal), 4499.0);
        assert_eq!(spice_sell_price(3375.0, Difficulty::Easy), 5623.0);
        assert_eq!(NPC_MAX_SPICE_BOUGHT, 200);
    }

    #[test]
    fn colonies_produce_spice_into_capped_storage() {
        // One production building: 1000 x 0.0005 / 60 = 1/120 spice per second.
        let rate = colony_spice_rate(1000.0, false, 1, 0, Difficulty::Normal, 1.0);
        assert!(close(rate * 120.0, 1.0));
        // One factory link (0xBD8CD0: 400) fills a spice in 300 s.
        assert!(close(colony_spice_rate(BASE_PRODUCTION_PER_FACTORY, false, 1, 0, Difficulty::Normal, 1.0) * 300.0, 1.0));
        assert!(close(colony_spice_rate(1000.0, true, 1, 0, Difficulty::Normal, 1.0), rate * 0.025));
        assert!(close(colony_spice_rate(1000.0, false, 9, 0, Difficulty::Normal, 1.0), rate * 0.5));
        assert!(close(colony_spice_rate(1000.0, false, 1, superpower_bit::CIV_ECONOMIC, Difficulty::Normal, 1.0), rate * 1.5));
        assert!(close(colony_spice_rate(1000.0, false, 1, 0, Difficulty::Hard, 1.0), rate * 0.8));
        assert_eq!(colony_spice_cap(false), 5.0);
        assert_eq!(colony_spice_cap(true), 10.0);
    }

    #[test]
    fn planet_spice_follows_the_star_colour_table() {
        assert_eq!(planet_spice(0, false, 0.5), 0);
        assert_eq!(planet_spice(0, false, 0.97), 3);
        assert_eq!(planet_spice(1, false, 0.5), 1);
        assert_eq!(planet_spice(2, false, 0.99), 2);
        assert_eq!(planet_spice(2, true, 0.1), 2);
        assert_eq!(planet_spice(2, true, 0.9), 5);
        assert_eq!(planet_spice(0, true, 0.7), 1);
    }

    #[test]
    fn relationship_events_sum_capped_rows_and_decay_toward_zero() {
        let p = Personality::Default;
        let d = Difficulty::Hard;
        let mut rel = Relationship::default();
        // Gift x0.5 (small) -> +0.5; large x5 -> capped at the row max 5.
        assert!(close(rel.apply(RelationshipEvent::GiveGift, 0.5, p, d), 0.5));
        rel.apply(RelationshipEvent::GiveGift, 5.0, p, d);
        assert!(close(rel.value(p, d), 5.0));
        // Declared war: initial -5 + 1 x -1 = -6.
        let mut war = Relationship::default();
        war.apply(RelationshipEvent::DeclaredWar, 1.0, p, d);
        assert!(close(war.value(p, d), -6.0));
        // Captured a system: -10 + -5 clamped to the row min -10; cap at -10 total.
        war.apply(RelationshipEvent::CapturedASystem, 1.0, p, d);
        assert!(close(war.absolute(p, d), -16.0));
        assert!(close(war.value(p, d), -10.0));
        // Decay: GiveGift decays 0.5 per tick from X 5.5.
        rel.decay_tick(p, d);
        assert!(close(*rel.events.get(&RelationshipEvent::GiveGift).unwrap(), 5.0));
        for _ in 0..20 {
            rel.decay_tick(p, d);
        }
        assert!(rel.events.is_empty());
        assert_eq!(rel.value(p, d), 0.0);
        // Events with decay 0 never fade.
        war.decay_tick(p, d);
        assert!(war.events.contains_key(&RelationshipEvent::DeclaredWar));
        // A scale that cancels X erases the entry.
        let mut c = Relationship::default();
        c.apply(RelationshipEvent::BeNice, 1.0, p, d);
        c.apply(RelationshipEvent::BeNice, -1.0, p, d);
        assert!(c.events.is_empty());
    }

    #[test]
    fn personalities_override_rows_by_archetype_and_difficulty() {
        use RelationshipEvent::*;
        assert_eq!(Personality::Default.event_row(MissionAborted, Difficulty::Normal).add, -1.0);
        assert_eq!(Personality::Bard.event_row(MissionAborted, Difficulty::Normal).add, 0.0);
        assert_eq!(Personality::Bard.event_row(MissionRejected, Difficulty::Normal).add, -1.0);
        assert_eq!(Personality::Bard.event_row(MissionRejected, Difficulty::Easy).add, 0.0);
        assert_eq!(Personality::Trader.event_row(WitholdTribute, Difficulty::Easy).initial, -0.5);
        assert_eq!(Personality::Trader.event_row(WitholdTribute, Difficulty::Normal).initial, -1.75);
        assert_eq!(Personality::Trader.event_row(WitholdTribute, Difficulty::Hard).initial, -2.5);
        assert_eq!(Personality::Ecologist.event_row(TerraformImproved, Difficulty::Hard).add, 6.0);
        assert_eq!(Personality::Grox.event_row(WitholdTribute, Difficulty::Hard).add, -8.0);
        assert_eq!(Personality::Grox.event_row(PersonalityMean, Difficulty::Hard).initial, -7.0);
        assert_eq!(Personality::Warrior.surrender_chance(), (0.25, 0.0));
        assert_eq!(Personality::Diplomat.surrender_chance(), (0.75, 1.0));
        assert_eq!(Personality::Grox.surrender_chance(), (0.0, 0.0));
    }

    #[test]
    fn first_contact_applies_the_personality_and_archetype_mods() {
        let d = Difficulty::Hard;
        // The Grox start at war (-7).
        let mut grox = Relationship::default();
        grox.first_contact(Personality::Grox, SpaceArchetype::Trader, d);
        assert!(close(grox.value(Personality::Grox, d), -7.0));
        assert!(declares_war(grox.value(Personality::Grox, d)));
        // Traders like traders: PersonalityNice 3 + ArchetypeNice 1.
        let mut trader = Relationship::default();
        trader.first_contact(Personality::Trader, SpaceArchetype::Trader, d);
        assert!(close(trader.value(Personality::Trader, d), 4.0));
        // Warriors dislike warriors: PersonalityMean -3 + ArchetypeMean -1.
        let mut warrior = Relationship::default();
        warrior.first_contact(Personality::Warrior, SpaceArchetype::Warrior, d);
        assert!(close(warrior.value(Personality::Warrior, d), -4.0));
        assert!(is_hostile(warrior.value(Personality::Warrior, d)));
        // Applied once.
        warrior.first_contact(Personality::Warrior, SpaceArchetype::Warrior, d);
        assert!(close(warrior.value(Personality::Warrior, d), -4.0));
    }

    #[test]
    fn thresholds_and_attitudes_follow_the_tuning() {
        assert!(declares_war(-7.0) && !declares_war(-6.9));
        assert!(war_ends(-6.9) && !war_ends(-7.0));
        assert!(offers_alliance(7.0) && !offers_alliance(6.9));
        assert!(alliance_breaks(2.9) && !alliance_breaks(3.0));
        assert!(is_hostile(-3.0) && !is_hostile(-2.9));
        assert!(offers_missions(-6.9) && !offers_missions(-7.0));
        assert_eq!(attitude(-8.0), Attitude::Furious);
        assert_eq!(attitude(-5.0), Attitude::Annoyed);
        assert_eq!(attitude(0.0), Attitude::Cautious);
        assert_eq!(attitude(5.0), Attitude::Pleased);
        assert_eq!(attitude(9.0), Attitude::Friendly);
        assert_eq!(size_factor(0.0, 3.0), 1.0);
        assert_eq!(size_factor(1.0, 3.0), 2.0);
        assert_eq!(size_factor(0.5, 0.1), 0.75);
    }

    #[test]
    fn gifts_tribute_peace_and_purchase_offers_follow_the_tuning() {
        assert_eq!(GiftSize::Small.amount(), 10_000.0);
        assert_eq!(GiftSize::Large.relationship_scale(), 5.0);
        assert_eq!(GiftSize::largest_affordable(60_000.0), Some(GiftSize::Medium));
        assert_eq!(GiftSize::largest_affordable(9_999.0), None);
        assert_eq!(tribute_amount(1, Difficulty::Hard), 20_000.0);
        assert_eq!(tribute_amount(1, Difficulty::Normal), 10_000.0);
        assert_eq!(tribute_amount(0, Difficulty::Easy), 2_500.0);
        assert_eq!(peace_price(-8.0), 50_000.0);
        assert_eq!(peace_price(2.0), 0.0);
        assert_eq!(peace_offer(60_000.0, -8.0), OfferVerdict::Good);
        assert_eq!(peace_offer(50_000.0, -8.0), OfferVerdict::Fair);
        assert_eq!(peace_offer(46_000.0, -8.0), OfferVerdict::Fair);
        assert_eq!(peace_offer(44_000.0, -8.0), OfferVerdict::Bad);
        assert_eq!(peace_offer(38_000.0, -8.0), OfferVerdict::VeryBad);
        assert_eq!(system_purchase_cost(0), 100_000.0);
        assert_eq!(system_purchase_cost(9), 5_000_000.0);
    }

    #[test]
    fn awareness_and_universe_timers_follow_the_tuning() {
        assert_eq!(awareness_level(0.5), 0);
        assert_eq!(awareness_level(1.0), 1);
        assert_eq!(awareness_level(5.0), 2);
        assert_eq!(awareness_level(10.0), 3);
        assert_eq!(bump_awareness(49.5, 5.0, Difficulty::Normal), 50.0);
        assert!(close(bump_awareness(0.0, 1.0, Difficulty::Hard), 1.5));
        assert_eq!(pirate_raid_interval(true, 0), 600.0);
        assert_eq!(pirate_raid_interval(false, 0), 1200.0);
        // The cards multiply the frequency (0x100F080 / 0x100F8E7): rarer, not more often.
        assert_eq!(pirate_raid_interval(false, superpower_bit::CIV_MILITARY), 3600.0);
        assert_eq!(pirate_raid_interval(true, superpower_bit::CIV_MILITARY), 600.0, "the first wait is not scaled");
        assert_eq!(biosphere_collapse_interval(true, 0), 1200.0);
        assert_eq!(biosphere_collapse_interval(false, superpower_bit::CIV_RELIGIOUS), 1800.0);
        assert_eq!(grox_attack_interval(0.0), None);
        assert_eq!(grox_attack_interval(1.0), Some(240.0));
        assert_eq!(grox_attack_interval(12.0), Some(120.0));
    }

    #[test]
    fn trade_routes_mature_at_the_minimum_rate_whatever_their_length() {
        // 0x1038D39: clamp(2e-5 / (1 + 1.0 x d / 10), 4.2e-5, 6.6e-5) per ms.
        for distance in [0.0, 3.0, 20.0] {
            assert!(close(trade_capture_rate(distance), TRADE_CAPTURE_MIN_RATE));
        }
        // 100 / 4.2e-5 ms = 2381 s to 100 %.
        assert!(close(trade_capture_progress(1.0) * 2381.0, 1.0002));
        assert!(trade_capture_progress(2300.0) < 1.0);
        assert!(trade_capture_progress(2400.0) > 1.0);
    }

    #[test]
    fn empire_growth_slows_as_the_empire_fills_its_budget() {
        assert!(empire_grows(0, 10, 0.7));
        assert!(!empire_grows(0, 10, 0.8));
        assert!(!empire_grows(10, 10, 0.0));
        assert!(empire_grows(5, 10, 0.3));
        assert!(!empire_grows(5, 10, 0.4));
    }

    #[test]
    fn galactic_core_and_destruction_cinematics() {
        assert_eq!(CORE_REWARD_TOOLS[0], tool::GENESIS_DEVICE);
        assert_eq!(STAFF_OF_LIFE_CHARGES, 42);
        // The core's reach limit before Steve: 3 inside 70, full outside 100.
        assert_eq!(core_travel_range(20.0, 50.0, false), 3.0);
        assert_eq!(core_travel_range(20.0, 100.0, false), 20.0);
        assert!((core_travel_range(20.0, 85.0, false) - 11.5).abs() < 1e-4);
        assert_eq!(core_travel_range(20.0, 50.0, true), 20.0);
        assert_eq!(core_travel_range(3.0, 10.0, false), 3.0);
        assert_eq!(destroyed_cinematic(true, true), "SPG_PlayerDestroyed_PostPosse");
        assert_eq!(destroyed_cinematic(false, true), "SPG_PlayerDestroyed_PrePosse_Galaxy");
        assert_eq!(destroyed_cinematic(false, false), "SPG_PlayerDestroyed_PrePosse_NotGalaxy");
    }
}
