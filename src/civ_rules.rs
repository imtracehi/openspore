//! Retail Civilization-stage rules in one place: the original's tables and
//! formulas as named constants and pure functions that `civ`, `civ_rts` and
//! `civ_wildlife` call.
//!
//! Sources: `notes\civ-re-notes.md` V1-V4 (from the original executable and
//! data, ModAPI march2017 addresses) and the extra tracing in
//! `notes\civ-rules-re.md` R1-R8 (done for this module; decompilations in the
//! helper's `target\re\out`). "VERIFIED" = read in the executable; "tuning" =
//! an installed property (`CityGameGeneralTuning` 0xB6A5C63D,
//! `CityGameBuildingTuning` 0xB7E0FF19, `CivGameLevelGenerationList`
//! 0x3BCE99E9, `spacetools~`, `space_npc_relationship_effects~`) whose use was
//! traced. What the code or data does not give is marked `ADAPTED:`.
//!
//! Sections: world scale, V1 stage flow (start, tech level, play style,
//! cinematics), V2 economy (money update, building links, mood, derricks,
//! buildings, vehicles, refunds), V3 diplomacy (relationship ledger, capture
//! timings, buying a city, NPC city types), V4 world (placement, epic
//! predators, animal casting) and the consequence abilities / superweapons.
// Many named retail values are kept for the passes that will use them (HUD,
// cinematics, comm screen) and are only exercised by the tests.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub use crate::creature_rules::Difficulty;

/// Pick the value for a difficulty (easy, medium, hard).
fn pick<T: Copy>(difficulty: Difficulty, easy: T, medium: T, hard: T) -> T {
    match difficulty {
        Difficulty::Easy => easy,
        Difficulty::Medium => medium,
        Difficulty::Hard => hard,
    }
}

// ================================================================= world scale

/// The original's Civ distances are chord distances on the R = 500 planet
/// (`notes\planet-re-notes.md` 1A/1E); the stage plays on that planet
/// (`civ_planet`), so every distance taken from the data is used as is. (The
/// island builds used 28/180, which put `MinDistanceBetweenCities` 180 at 28.)
pub const CIV_WORLD_SCALE: f32 = 1.0;

/// A retail distance in world units.
pub fn world(retail: f32) -> f32 {
    retail * CIV_WORLD_SCALE
}

// ================================================================ V1 stage flow

/// `InitialWealth` (tuning +0x0C): the money a civilization starts with.
pub const INITIAL_WEALTH: f32 = 1000.0;
/// `InitialCreatures`, `InitialTribeCreatures`, `InitialTribes` (tuning).
pub const INITIAL_CREATURES: u32 = 20;
pub const INITIAL_TRIBE_CREATURES: u32 = 7;
pub const INITIAL_TRIBES: u32 = 1;
/// `MaxCityUpgradeLevel` 2 with `CityUpgrade1/2` 30 / 60 (names only).
pub const MAX_CITY_UPGRADE_LEVEL: u32 = 2;
pub const CITY_UPGRADE_THRESHOLDS: [f32; 2] = [30.0, 60.0];
/// `SecondsNextCity` (tuning +0x1F8, read by 0xCEEDB0): new NPC cities appear
/// on this cadence (`CVG_PRE_NewCityAppears`, `civ_reveal`).
pub const SECONDS_NEXT_CITY: f32 = 20.0;

/// VERIFIED (level setup 0xB9D970): it sets `water_fraction`, evaluates the
/// Arithmetica `0x024A4F5A!0x18657795` and reads `num_cities`, which is the
/// script's constant 10 (its `num_cities_near_water` = 5.9 is never read; the
/// code also reads `num_npc_cities_on_player_continent`, `population_min /
/// max` 15 and `wealth_min / max` 4000). The slot count of the planet is
/// `min(num_cities, city markers of the level)`, the player's city included
/// (singleton 0x0167A60C +0xAC = slots).
pub const LEVEL_NUM_CITIES: usize = 10;
/// VERIFIED (0xB9D970, `num_npc_cities_on_player_continent` 3): the first
/// NPC slots go to markers on the player's continent, three of them.
pub const NPC_CITIES_ON_PLAYER_CONTINENT: usize = 3;

/// Where the level setup puts the next NPC slot (VERIFIED 0xB9D970: the
/// shuffled marker list is consumed in three steps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotWant {
    /// A marker on the player's continent (the first three NPC slots).
    PlayerContinent,
    /// A marker near water (within 70, 0xB8BCC0) on another continent: every
    /// other continent gets one first, in the order of the continent table.
    CoastalOn(usize),
    /// Any marker of the other continent with the largest `size / cities
    /// already given to it` (integer division, the first of equals).
    Continent(usize),
}

/// VERIFIED (0xB9D970): the order in which the `npc_slots` NPC slots are
/// filled. `others` lists the continents other than the player's as
/// `(continent, size)` in the retail table's order (0xB7EB50 gives the size).
/// ADAPTED: when the planet has no other continent retail places only the
/// player-continent slots and leaves the rest of `num_cities` unplaced; this
/// rewrite keeps every slot (the campaign has a fixed city count) and puts
/// the remainder on the player's continent.
pub fn npc_slot_wants(npc_slots: usize, others: &[(usize, usize)]) -> Vec<SlotWant> {
    let mut wants = Vec::with_capacity(npc_slots);
    while wants.len() < NPC_CITIES_ON_PLAYER_CONTINENT.min(npc_slots) {
        wants.push(SlotWant::PlayerContinent);
    }
    for &(continent, _) in others {
        if wants.len() >= npc_slots {
            break;
        }
        wants.push(SlotWant::CoastalOn(continent));
    }
    // Cities each other continent holds (the player-continent entry starts
    // at one; the others reach one with their coastal slot).
    let mut given = vec![1usize; others.len()];
    while wants.len() < npc_slots {
        let mut best: Option<(usize, usize)> = None;
        for (i, &(_, size)) in others.iter().enumerate() {
            let score = size / given[i].max(1);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, i));
            }
        }
        match best {
            Some((_, i)) => {
                wants.push(SlotWant::Continent(others[i].0));
                given[i] += 1;
            }
            None => wants.push(SlotWant::PlayerContinent),
        }
    }
    wants
}

/// VERIFIED (0xBF7A10): a civilization starts at vehicle tech level 1.
pub const INITIAL_VEHICLE_TECH_LEVEL: u8 = 1;
/// VERIFIED (0xBF44B0): one to three cities give level 2.
pub const CITY_TECH_LEVEL: u8 = 2;
/// VERIFIED (0xCFA220 -> 0xCF9510 `CVG_UnlockAir`): air vehicles need a tech
/// level above 2; 0xBF44B0 grants level 3 with more than three cities.
pub const AIR_TECH_LEVEL: u8 = 3;
/// VERIFIED (0xBF44B0, 0xBEF8C0, 0xCF7730): level 4 is the whole planet: the
/// civilization holds every city (`cities >= singleton 0x0167A60C +0xAC`). It
/// completes the goal `CaptureAll` and is the "planet won" test of the
/// capture-stop branch (0xCFDD35); it is NOT the superweapon gate (that is
/// the ability flags, [`ability_flags`]).
pub const PLANET_TECH_LEVEL: u8 = 4;
/// Kept for the callers that named level 4 after the old (ADAPTED) rule.
pub const SUPERWEAPON_TECH_LEVEL: u8 = PLANET_TECH_LEVEL;
/// VERIFIED (0xBDB9D0): sea vehicles need tech level 2 and a city with a boat
/// spawn point (a coastal city: OpenSpore `CityState::harbor`).
pub const SEA_TECH_LEVEL: u8 = CITY_TECH_LEVEL;
/// Cities above which 0xBF44B0 raises the tech level to 3.
pub const AIR_CITY_THRESHOLD: usize = 3;

/// Vehicle tech level for a city count on a planet of `total` city slots
/// (VERIFIED 0xBF44B0: `n >= slots` -> 4, `n >= 4` -> 3, `n >= 1` -> 2; it
/// never falls, see [`raise_tech_level`]).
pub fn vehicle_tech_level(cities: usize, total: usize) -> u8 {
    if cities >= total.max(1) {
        PLANET_TECH_LEVEL
    } else if cities > AIR_CITY_THRESHOLD {
        AIR_TECH_LEVEL
    } else if cities > 0 {
        CITY_TECH_LEVEL
    } else {
        INITIAL_VEHICLE_TECH_LEVEL
    }
}

/// 0xBEF8C0: the level only rises.
pub fn raise_tech_level(current: u8, cities: usize, total: usize) -> u8 {
    current.max(vehicle_tech_level(cities, total)).max(INITIAL_VEHICLE_TECH_LEVEL)
}

pub fn air_unlocked(tech_level: u8) -> bool {
    tech_level >= AIR_TECH_LEVEL
}

pub fn sea_unlocked(tech_level: u8) -> bool {
    tech_level >= SEA_TECH_LEVEL
}

/// VERIFIED (0xCF7730): level 4 = every city of the planet is the player's.
pub fn planet_won(tech_level: u8) -> bool {
    tech_level >= PLANET_TECH_LEVEL
}

// ------------------------------------------------------------ ability flags

/// The twelve ability flags of `cCivilization` +0x484 .. +0x48F (counted by
/// 0xBEFC00): nine consequence abilities of the earlier stages and the three
/// superweapons.
pub const ABILITY_COUNT: usize = 12;

/// VERIFIED (0xBEFAD0): the flag index of a consequence trait id.
pub fn ability_index(trait_id: u32) -> Option<usize> {
    Some(match trait_id {
        0x2CDB_D6A1 => 0,  // cvg_crg_attack (Mighty Bomb)
        0x0082_26D0 => 1,  // cvg_trg_attack (Gadget Bomb)
        0x3621_3C6A => 2,  // cvg_trg_attackwin (ICBM)
        0x045C_DF1A => 3,  // cvg_clg_plant (Healing Aura)
        0xFD05_AB6D => 4,  // cvg_trg_social (Black Cloud)
        0x8129_9805 => 5,  // cvg_trg_socialwin (Messianic Rising)
        0x3221_E759 => 6,  // cvg_crg_mix (Bribe Bomb)
        0x08A9_1F1E => 7,  // cvg_trg_mix (Ad Blitz)
        0xFC71_6A94 => 8,  // cvg_trg_mixwin (Global Merger)
        0xC061_7B2F => 9,  // cvg_clg_mix (Static Bomb)
        0x0756_D422 => 10, // cvg_clg_meat (Invulnerability)
        0x4A78_672C => 11, // cvg_crg_social (Diplo Dervish)
        _ => return None,
    })
}

/// The superweapon flags (military `attackwin`, religious `socialwin`, economic
/// `mixwin`): cleared when the civilization holds the whole planet (0xBF2706).
pub const SUPERWEAPON_ABILITIES: [usize; 3] = [2, 5, 8];

/// VERIFIED (0xBEFBA0 and the jump tables of 0xBF2510): the cities the
/// civilization must hold before each ability's flag is set. Cell abilities
/// (3, 9, 10) need 1, Creature abilities (0, 6, 11) 2, Tribal abilities
/// (1, 4, 7) 4, the superweapons (2, 5, 8) 6.
pub const ABILITY_CITIES: [usize; ABILITY_COUNT] = [2, 4, 6, 1, 4, 6, 2, 4, 6, 1, 1, 2];

/// Cities needed for ability `index` (VERIFIED, [`ABILITY_CITIES`]: 1 / 2 / 4
/// / 6). `total` is the planet's slot count: with the retail ten
/// ([`LEVEL_NUM_CITIES`]) the superweapon's 6 is reachable, so no count is
/// adapted any more; the flags are cleared once every slot is held
/// ([`ability_flags`]).
pub fn ability_cities(index: usize, _total: usize) -> usize {
    ABILITY_CITIES.get(index).copied().unwrap_or(1)
}

/// VERIFIED (0xBF2510): recompute the ability flags. `traits` are the flag
/// indices of the three consequence traits the player earned (Cell, Creature,
/// Tribe; [`ability_index`]); each sets its flag once the civilization holds
/// enough cities. A civilization whose traits hold no superweapon gets the
/// one of its first city's type (`capital_weapon`: military 2, religious 5,
/// economic 8). With every city of the planet held the superweapon flags are
/// cleared.
pub fn ability_flags(
    traits: &[usize],
    capital_weapon: Option<usize>,
    cities: usize,
    total: usize,
) -> [bool; ABILITY_COUNT] {
    let mut flags = [false; ABILITY_COUNT];
    let mut weapon_trait = false;
    for &index in traits {
        if index >= ABILITY_COUNT {
            continue;
        }
        weapon_trait |= SUPERWEAPON_ABILITIES.contains(&index);
        if cities >= ability_cities(index, total) {
            flags[index] = true;
        }
    }
    if !weapon_trait && cities > 0 {
        if let Some(index) = capital_weapon.filter(|i| *i < ABILITY_COUNT) {
            if cities >= ability_cities(index, total) {
                flags[index] = true;
            }
        }
    }
    if cities == total {
        for index in SUPERWEAPON_ABILITIES {
            flags[index] = false;
        }
    }
    flags
}

/// How a city was taken: the counters of `cCivilization` +0x4C0 / +0x4C4 /
/// +0x4C8 (`mMilitaryCaptureCount`, `mReligiousCaptureCount`,
/// `mEconomicCaptureCount`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureCounts {
    pub military: u32,
    pub religious: u32,
    pub economic: u32,
}

/// The Civ play style decided at Advance (0xCF82C0): the consequence trait
/// card the player earns for Space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayStyle {
    Military,
    Economic,
    Religious,
}

impl PlayStyle {
    /// FNV-1 ids 0xCF82C0 stores in `cPlayer` +0x1108 (`cvg_military`,
    /// `cvg_economic`, `cvg_religious`).
    pub fn id(self) -> u32 {
        match self {
            PlayStyle::Military => 0x0A35_D0F5,
            PlayStyle::Economic => 0x3360_727F,
            PlayStyle::Religious => 0x6AEB_96A1,
        }
    }

    /// The consequence trait record (`consequence_traits~`).
    pub fn consequence_trait(self) -> &'static str {
        match self {
            PlayStyle::Military => "cvg_military",
            PlayStyle::Economic => "cvg_economic",
            PlayStyle::Religious => "cvg_religious",
        }
    }

    /// The card id 0xCF82C0 shows (0x99F0D1DA / 0xAAF6AAAC / 0xDCA976D0).
    pub fn card_id(self) -> u32 {
        match self {
            PlayStyle::Military => 0x99F0_D1DA,
            PlayStyle::Economic => 0xAAF6_AAAC,
            PlayStyle::Religious => 0xDCA9_76D0,
        }
    }

    /// The en-us trait name (locale 0x9A5ACF10 0x51 / 0x53 / 0x55).
    pub fn title(self) -> &'static str {
        match self {
            PlayStyle::Military => "Military",
            PlayStyle::Economic => "Economic",
            PlayStyle::Religious => "Religious",
        }
    }
}

/// VERIFIED (0xCF82C0 with 0xBF19C0): fractions of the largest capture count;
/// military wins only when strictly above both others, economic likewise,
/// every other case (ties included) is religious. When nothing was ever
/// captured the counts fall back to the cities the civilization holds of each
/// specialty (0xBF0330), `[military, religious, economic]`.
pub fn play_style(captures: CaptureCounts, cities_by_kind: [usize; 3]) -> PlayStyle {
    let (mut m, mut r, mut e) = (
        captures.military as f32,
        captures.religious as f32,
        captures.economic as f32,
    );
    if m.max(r).max(e) == 0.0 {
        m = cities_by_kind[0] as f32;
        r = cities_by_kind[1] as f32;
        e = cities_by_kind[2] as f32;
    }
    if m > e && m > r {
        PlayStyle::Military
    } else if e > r && e > m {
        PlayStyle::Economic
    } else {
        PlayStyle::Religious
    }
}

/// The capital's type from the Tribal stage's city-hall card
/// (`TRG2CVG_ShowCityHall`: attack -> MILI, social -> RELI, mix -> ECON).
/// The retail card comes from the tribe's final relationships
/// (`tribal_rules::play_style_from_relations`, recorded as
/// `TribalCampaign::city_hall_card`, which `civ::setup_civ` reads first). This
/// carving count is the fallback for a tribe saved before the card was
/// recorded; `None` when the tribe left no history (a Civ start without a
/// Tribal stage keeps the save's type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TribalCard {
    Attack,
    Social,
    Mix,
}

pub fn tribal_card(conquered: usize, allied: usize) -> Option<TribalCard> {
    if conquered == 0 && allied == 0 {
        None
    } else if conquered > allied {
        Some(TribalCard::Attack)
    } else if allied > conquered {
        Some(TribalCard::Social)
    } else {
        Some(TribalCard::Mix)
    }
}

/// Retail ids of the stage-flow controls and messages (V1).
pub mod ids {
    /// Advance-to-Space button of the Civ HUD (click proc ~0xE05D7B).
    pub const ADVANCE_BUTTON: u32 = 0x0645_3746;
    /// Glow shown with the Advance button.
    pub const ADVANCE_GLOW: u32 = 0x0652_A628;
    /// Message 0xCF82C0 sends with the earned trait card.
    pub const TRAIT_CARD_MESSAGE: u32 = 0x073E_46F6;
    /// `event_CVGSuccess` sent by 0xCF4620 before the switch to Space.
    pub const EVENT_CVG_SUCCESS: u32 = 0x3D1A_FC95;
    /// `kGameSpace` game mode.
    pub const GAME_MODE_SPACE: u32 = 0x0165_4C05;
    /// Cinematic target: the city hall.
    pub const CINEMATIC_CITY_HALL: u32 = 0x115F_A309;
    /// Flag set once `CVG_UnlockAir` played.
    pub const AIR_UNLOCK_SHOWN_FLAG: u32 = 0x050A_2E65;
}

/// Cinematic names of the stage flow (the `cinematics` module plays them).
pub mod cinematics {
    /// Tribal end: city hall / vehicle editors, then the city hall card.
    pub const PRE_EDITOR: &str = "TRG2CVG_PreEditor";
    pub const SHOW_CITY_HALL: &str = "TRG2CVG_ShowCityHall";
    /// Civ mode state 6 (0xCFBD70).
    pub const CIV_START: &str = "TRG2CVG_CivStart";
    /// 0xCF9510: tech level above 2.
    pub const UNLOCK_AIR: &str = "CVG_UnlockAir";
    /// 0xCF82C0: the Advance button.
    pub const UNLOCK_UFO: &str = "CVG_UnlockUFO";
    /// 0xC00C40: a player city is attacked.
    pub const PLAYER_CITY_ATTACKED: &str = "CVG_PlayerCityAttacked";
    /// 0xCF9D00: tutorial, send a vehicle to a spice geyser.
    pub const SEND_VEHICLE_TO_COMMODITY: &str = "CVG_SendVehicleToCommodity";
    /// 0xAE61C0: a new NPC city appears.
    pub const NEW_CITY_APPEARS: &str = "CVG_PRE_NewCityAppears";

    pub const ALL: [&str; 8] = [
        PRE_EDITOR,
        SHOW_CITY_HALL,
        CIV_START,
        UNLOCK_AIR,
        UNLOCK_UFO,
        PLAYER_CITY_ATTACKED,
        SEND_VEHICLE_TO_COMMODITY,
        NEW_CITY_APPEARS,
    ];
}

// ================================================================== V2 economy

/// VERIFIED (0xAE61C0): `cCivilization::Update` runs once per game second.
pub const CIV_UPDATE_SECONDS: f32 = 1.0;
/// VERIFIED (0xBF7310): money is paid every 6th update.
pub const MONEY_UPDATE_TICKS: u32 = 6;
pub const MONEY_UPDATE_SECONDS: f32 = CIV_UPDATE_SECONDS * MONEY_UPDATE_TICKS as f32;
/// VERIFIED (0xBF7310): the update pays 0.1 x the summed income.
pub const INCOME_FACTOR: f32 = 0.1;
/// VERIFIED (0xBEFED0): wealth never exceeds 999,999.
pub const WEALTH_CAP: f32 = 999_999.0;

/// A building in the city link model (`cCity::ProcessBuildingUpdate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CityBuilding {
    CityHall,
    House,
    Factory,
    Entertainment,
}

/// VERIFIED (0xBE2590): money a factory earns per link to a house or the
/// city hall.
pub const FACTORY_LINK_MONEY: i32 = 400;
/// VERIFIED (0xBE2590): `happiness = (happy - unhappy + 5) x 10`, 0..100.
pub const HAPPINESS_BASE: i32 = 5;
pub const HAPPINESS_PER_POINT: i32 = 10;
pub const HAPPINESS_MAX: i32 = 100;
/// The neutral mood the party / riot rules compare with.
pub const HAPPINESS_NEUTRAL: i32 = 50;

/// What the link model gives a city.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CityModel {
    /// `mMoneyCount` = `mFinalIncome`.
    pub money: i32,
    pub happy: i32,
    pub unhappy: i32,
}

impl CityModel {
    /// `cCity` +0x304, clamped to 0..100.
    pub fn happiness(&self) -> i32 {
        ((self.happy - self.unhappy + HAPPINESS_BASE) * HAPPINESS_PER_POINT).clamp(0, HAPPINESS_MAX)
    }
}

/// VERIFIED (0xBE2590): the buildings connected to a city hall through links
/// (the hall itself included).
pub fn connected_buildings(nodes: &[Option<CityBuilding>], links: &[(usize, usize)]) -> Vec<bool> {
    let n = nodes.len();
    let linked = |a: usize, b: usize| links.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a));
    let mut connected: Vec<bool> = nodes
        .iter()
        .map(|b| *b == Some(CityBuilding::CityHall))
        .collect();
    loop {
        let mut changed = false;
        for i in 0..n {
            if nodes[i].is_none() || connected[i] {
                continue;
            }
            if (0..n).any(|j| connected[j] && nodes[j].is_some() && linked(i, j)) {
                connected[i] = true;
                changed = true;
            }
        }
        if !changed {
            return connected;
        }
    }
}

/// VERIFIED (0xBE2590): only buildings connected to the city hall through the
/// links count; a factory earns 400 per link to a house or the hall and is
/// unhappy (+1, +1 per link to an entertainment building); an entertainment
/// building is happy (+1, +1 per link to a house or the hall).
/// `entertainment_off` models Black Cloud (entertainment shut down).
pub fn evaluate_city(
    nodes: &[Option<CityBuilding>],
    links: &[(usize, usize)],
    entertainment_off: bool,
) -> CityModel {
    let n = nodes.len();
    let linked = |a: usize, b: usize| links.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a));
    let connected = connected_buildings(nodes, links);
    let mut model = CityModel::default();
    for i in 0..n {
        let Some(building) = nodes[i] else { continue };
        if !connected[i] {
            continue;
        }
        let neighbours = (0..n).filter(|&j| j != i && linked(i, j)).filter_map(|j| nodes[j]);
        match building {
            CityBuilding::Factory => {
                model.unhappy += 1;
                for other in neighbours {
                    match other {
                        CityBuilding::House | CityBuilding::CityHall => model.money += FACTORY_LINK_MONEY,
                        CityBuilding::Entertainment if !entertainment_off => model.unhappy += 1,
                        _ => {}
                    }
                }
            }
            CityBuilding::Entertainment if !entertainment_off => {
                model.happy += 1;
                model.happy += neighbours
                    .filter(|b| matches!(b, CityBuilding::House | CityBuilding::CityHall))
                    .count() as i32;
            }
            _ => {}
        }
    }
    model
}

/// VERIFIED (0xBD8CD0 `GetIncome`): the final income doubles while the city
/// parties above neutral mood and halves (integer) while it riots below it.
pub fn city_income(final_income: i32, partying: bool, rioting: bool, happiness: i32) -> i32 {
    let mut income = final_income;
    if partying && happiness > HAPPINESS_NEUTRAL {
        income *= 2;
    }
    if rioting && happiness < HAPPINESS_NEUTRAL {
        income /= 2;
    }
    income
}

/// VERIFIED (0xBD8D20): a calm city rolls for a party every 10 s and for a
/// riot every 5 s; either lasts 60 s (a party also ends when the city is
/// attacked).
pub const PARTY_ROLL_SECONDS: f32 = 10.0;
pub const RIOT_ROLL_SECONDS: f32 = 5.0;
pub const MOOD_EVENT_SECONDS: f32 = 60.0;
/// The rolls are `rand(200)`.
pub const MOOD_ROLL_RANGE: u32 = 200;

/// Party starts when `roll < happiness - 50` (+10 at happiness 100).
pub fn party_threshold(happiness: i32) -> i32 {
    happiness - HAPPINESS_NEUTRAL + if happiness == HAPPINESS_MAX { 10 } else { 0 }
}

/// Riot starts when `roll < 50 - happiness`.
pub fn riot_threshold(happiness: i32) -> i32 {
    HAPPINESS_NEUTRAL - happiness
}

pub fn party_starts(happiness: i32, roll: u32) -> bool {
    (roll as i32) < party_threshold(happiness)
}

pub fn riot_starts(happiness: i32, roll: u32) -> bool {
    (roll as i32) < riot_threshold(happiness)
}

/// VERIFIED (0xCEF130): a geyser's `mResourcePoints` start at 100.
pub const DERRICK_START_POINTS: f32 = 100.0;
/// `HarvestMinLoad`, `HarvestMaxLoad`, `HarvestResourceLoss` (tuning
/// +0x1D4..+0x1DC): the names are misleading, 0xCEF130 uses them as the
/// minimum payout, the payout scale (x0.01 per point) and the loss per update.
pub const HARVEST_MIN_LOAD: f32 = 100.0;
pub const HARVEST_MAX_LOAD: f32 = 1000.0;
pub const HARVEST_RESOURCE_LOSS: f32 = 0.3;
/// `ResourceRegenerationPerSecond` 0: geysers never refill.
pub const RESOURCE_REGENERATION_PER_SECOND: f32 = 0.0;

/// VERIFIED (0xCEF130): a derrick's income before the 0.1 factor: points
/// under 100 round up to the next ten, x `HarvestMaxLoad` x 0.01, at least
/// `HarvestMinLoad`.
pub fn derrick_income(points: f32) -> f32 {
    let mut p = if points.is_finite() { points as i32 } else { 0 };
    if p < 100 {
        p = (p / 10 + 1) * 10;
    }
    (p as f32 * HARVEST_MAX_LOAD * 0.01).max(HARVEST_MIN_LOAD)
}

/// Points left after one money update.
pub fn derrick_points_after(points: f32) -> f32 {
    (points - HARVEST_RESOURCE_LOSS).max(0.0)
}

/// VERIFIED (0xBEFED0): NPC income multiplier by difficulty, whether the
/// player's civilization has "developed" and whether the NPC is neutral. The
/// player's own income is never scaled.
pub fn npc_income_multiplier(difficulty: Difficulty, player_developed: bool, neutral: bool) -> f32 {
    match (player_developed, neutral) {
        (true, false) => pick(difficulty, 0.4, 1.0, 1.2),
        (false, true) => pick(difficulty, 0.1, 0.2, 0.4),
        _ => pick(difficulty, 0.2, 0.4, 0.8),
    }
}

/// The stats 0xBFBD30 checks to mark a civilization "developed" (once true it
/// stays true).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DevelopmentStats {
    pub cities: usize,
    pub vehicles: usize,
    pub buildings: usize,
    /// Money + vehicle costs + building costs (0xBF2C20).
    pub worth: f32,
    pub derricks: usize,
}

/// VERIFIED (0xBFBD30): developed when any threshold is reached: easy cities
/// 9 / vehicles 25 / buildings 61 / worth > 40,000 / derricks > 4, medium 5 /
/// 13 / 31 / 20,000 / 2, hard 5 / 9 / 21 / 10,000 / 1.
pub fn has_developed(difficulty: Difficulty, s: DevelopmentStats) -> bool {
    let (cities, vehicles, buildings, worth, derricks) = pick(
        difficulty,
        (9, 25, 61, 40_000.0, 4),
        (5, 13, 31, 20_000.0, 2),
        (5, 9, 21, 10_000.0, 1),
    );
    s.cities >= cities
        || s.vehicles >= vehicles
        || s.buildings >= buildings
        || s.worth > worth
        || s.derricks > derricks
}

/// VERIFIED (0xBFBD30): on easy, each second has a 1 in 10 chance to give a
/// stuck player (no vehicle, under 1000 money, zero income) a free land
/// vehicle at the capital.
pub const FREE_VEHICLE_ODDS: u32 = 10;
pub fn free_vehicle_eligible(difficulty: Difficulty, vehicles: usize, money: f32, income: f32) -> bool {
    difficulty == Difficulty::Easy && vehicles == 0 && money < 1000.0 && income == 0.0
}

/// VERIFIED (0xBFA7A0, the head of the rivals' attack decision, both the military
/// and the cultural purpose; `civ-rules3-re.md` W5): there is no declared war and
/// no timer of the human's. The rival keeps its target and restarts a 40 s
/// (military) / 50 s (cultural) retarget stopwatch (`civ_comm::ATTACK_INTERVAL_*`)
/// that runs from the moment it exists, the decision only begins once the player
/// has developed (the civilization's byte +0x8D, `CivCampaign::player_developed`),
/// and its candidates are the nations it is Annoyed or worse with. On Easy and
/// Medium (the difficulty value 0 / 1, 0xC755C0 <= 1) a rival that has no target
/// yet holds back, clears its targets and restarts the stopwatch while it owns more
/// cities than the player (the two city vectors, 0xBFA805..0xBFA823), and on
/// Medium only while the player has never had two cities (+0x4D0 < 2, 0xBFA83E).
pub fn attack_held_back(
    difficulty: Difficulty,
    player_cities: usize,
    rival_cities: usize,
    player_had_two_cities: bool,
    has_target: bool,
) -> bool {
    if has_target || difficulty == Difficulty::Hard {
        return false;
    }
    if player_cities >= rival_cities {
        return false;
    }
    difficulty == Difficulty::Easy || !player_had_two_cities
}

// ---- buildings (CityGameBuildingTuning; Space prices kept for the Space pass)

/// `SleepCost` (house), `IndustryCost` (factory), `EntertainmentCost`,
/// `DefenseCost` (turret).
pub const HOUSE_COST: f32 = 1600.0;
pub const FACTORY_COST: f32 = 1200.0;
pub const ENTERTAINMENT_COST: f32 = 800.0;
pub const TURRET_COST: f32 = 2000.0;
pub const HOUSE_COST_SPACE: f32 = 25600.0;
pub const FACTORY_COST_SPACE: f32 = 19200.0;
pub const ENTERTAINMENT_COST_SPACE: f32 = 12800.0;
pub const TURRET_COST_SPACE: f32 = 16000.0;
/// `SleepCapacity` / `RoomsPerHouse`: a house holds 15 (the vehicle cap counts
/// houses as rooms / 15, 0xBD8AB0).
pub const ROOMS_PER_HOUSE: u32 = 15;
/// City-game buildings the Civ stage does not build (names only).
pub const FARM_COST: f32 = 25.0;
pub const MARKET_COST: f32 = 25.0;
pub const CULTURE_COST: f32 = 50.0;
pub const MILITARY_BUILDING_COST: f32 = 50.0;
pub const DIPLOMACY_COST: f32 = 50.0;
pub const CIVIC_OBJECT_COST: f32 = 25.0;
/// `RoadCost`, `RoadRefundFraction`, `RoadSpeedMultiplier` (names only).
pub const ROAD_COST: f32 = 3.0;
pub const ROAD_REFUND_FRACTION: f32 = 0.9;
pub const ROAD_SPEED_MULTIPLIER: f32 = 1.5;

/// VERIFIED (0xCF7BB0 / 0xCF3350): selling refunds this share of the cost.
pub fn refund_fraction(difficulty: Difficulty) -> f32 {
    pick(difficulty, 1.0, 0.75, 0.5)
}

pub fn refund(cost: f32, difficulty: Difficulty) -> f32 {
    cost * refund_fraction(difficulty)
}

// ---- vehicles

/// `VehiclePurpose` (ModAPI CommonIDs.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehiclePurpose {
    Military,
    Cultural,
    Economic,
    /// Colony / harvester vehicles.
    Colony,
}

/// `VehicleLocomotion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locomotion {
    Land,
    Amphibious,
    Air,
}

/// VERIFIED (0xBF22B0, id table 0x01469250): vehicle cost by purpose and
/// locomotion; purpose 3 (harvester) costs `HarvesterCost` 500.
pub fn vehicle_cost(purpose: VehiclePurpose, locomotion: Locomotion) -> f32 {
    if purpose == VehiclePurpose::Colony {
        return HARVESTER_COST;
    }
    match locomotion {
        Locomotion::Land => 1000.0,
        Locomotion::Amphibious => 1500.0,
        Locomotion::Air => 3000.0,
    }
}

/// `HarvesterCost` / `HarvesterCapPerCity`: the purpose-3 vehicle (not in
/// OpenSpore; derricks are built by any vehicle).
pub const HARVESTER_COST: f32 = 500.0;
pub const HARVESTER_CAP_PER_CITY: u32 = 2;
/// `VehicleCapPerCity` (tuning +0x104, read by 0xBF2110).
pub const VEHICLE_CAP_PER_CITY: u32 = 1;
/// VERIFIED (0xBF2110): the capital's specialty gets 3 extra vehicles, the
/// total is capped at 36, NPC houses count up to 6 per city.
pub const CAPITAL_VEHICLE_BONUS: u32 = 3;
pub const VEHICLE_CAP_MAX: u32 = 36;
pub const NPC_HOUSE_VEHICLE_CAP: u32 = 6;
/// `VehicleWeaponCycleTime` (cVehicle::Load 0xCA67C0): one shot every 2 s.
pub const VEHICLE_WEAPON_CYCLE: f32 = 2.0;
/// `VehicleGiftCost` and the editor stat limits (names only).
pub const VEHICLE_GIFT_COST: f32 = 100.0;
pub const MAX_VEHICLE_STAT_LIMIT: f32 = 20.0;
pub const MAX_VEHICLE_STAT_TOTAL: f32 = 20.0;
/// cVehicle +0xC50..+0xC58 default power / defense / speed.
pub const DEFAULT_VEHICLE_STAT: f32 = 0.33;

/// One city of a vehicle purpose, for the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapCity {
    /// Connected houses plus the city hall (0xBD8AB0).
    pub houses: u32,
    /// `cCity` +0x519 (OpenSpore cities: false).
    pub minor: bool,
}

/// VERIFIED (0xBF2110): vehicles of one purpose a civilization may field:
/// 3 when it is the capital's purpose, plus per city of that purpose
/// `VehicleCapPerCity x 2` (x1 for +0x519 cities) and its houses (NPC capped
/// at 6, or 3 for +0x519 cities), at most 36.
pub fn vehicle_cap(capital_purpose: bool, cities: &[CapCity], player: bool) -> u32 {
    let mut cap = if capital_purpose { CAPITAL_VEHICLE_BONUS } else { 0 };
    for city in cities {
        cap += VEHICLE_CAP_PER_CITY * if city.minor { 1 } else { 2 };
        cap += if player {
            city.houses
        } else {
            city.houses.min(if city.minor { 3 } else { NPC_HOUSE_VEHICLE_CAP })
        };
    }
    cap.min(VEHICLE_CAP_MAX)
}

/// City turrets (`spacetools~!turret_at_mil / _rel / _ecn`): damage 25 / 20 /
/// 15 by city type, range 100.
pub const TURRET_TOOL_DAMAGE: [f32; 3] = [25.0, 20.0, 15.0];
pub const TURRET_TOOL_RANGE: f32 = 100.0;
/// Vehicle weapons (`military_template`, `mil_13..19`): range 100 (min 8);
/// cultural tools (`cult_*`) range 100.
pub const VEHICLE_TOOL_RANGE: f32 = 100.0;

// ======================================================= military capture (civ-planner)

/// City defence health (global tuning prop 0x00000000!0xA4C2F6BD, VERIFIED):
/// a turret `MilitaryBuildingHealth` 600 (cTurret 0xBD16F0 outside Space), a
/// house / factory / entertainment building `CivilianBuildingHealth` 800
/// (0xBD1970), the city hall `CityHallBuildingHealth` 3600 (0xBCF6A0; in Civ
/// the hall's health never drops, see `CAPTURE_PER_DAMAGE`).
pub const MILITARY_BUILDING_HEALTH: f32 = 600.0;
pub const CIVILIAN_BUILDING_HEALTH: f32 = 800.0;
pub const CITY_HALL_BUILDING_HEALTH: f32 = 3600.0;
/// `cCity::mCapturePercent` (+0x748): the city changes hands at 100
/// (VERIFIED 0xBE9C70 -> 0xBE9AD0, at once).
pub const CAPTURE_FULL: f32 = 100.0;
/// VERIFIED 0xBE9C70: capture points per point of damage counted.
pub const CAPTURE_PER_DAMAGE: f32 = 0.1;
/// VERIFIED cBuilding::TakeDamage 0xBCEE50: a house / factory / entertainment
/// building destroyed counts 80 (0xBD8100) -> 8 capture points.
pub const CAPTURE_BUILDING_VALUE: f32 = 80.0;
/// VERIFIED 0xBE2260 (city update, once per second, 0xAE61C0 -> 0xAE57C0):
/// capture points fall by 0.1 a second while the city has not been attacked
/// for 20 s (0xC00600 slot 0, stamped by 0xC00C40 from the buildings' and
/// turrets' AddHostileUnit 0xBCDA00 / 0xBCE7D0).
pub const CAPTURE_DECAY_PER_SECOND: f32 = 0.1;
pub const ATTACK_MEMORY_SECONDS: f32 = 20.0;
/// VERIFIED 0xBE3C20 (same once-a-second city update, only after 20 s
/// without an attack): standing buildings heal 2 a second, turrets 1; a
/// destroyed building is removed once 30 s (30,001 ms) passed since its last
/// damage, a destroyed turret after 120 s (120,001 ms).
pub const BUILDING_REPAIR_PER_SECOND: f32 = 2.0;
pub const TURRET_REPAIR_PER_SECOND: f32 = 1.0;
pub const RUINED_BUILDING_CLEAR_SECONDS: f32 = 30.0;
pub const RUINED_TURRET_CLEAR_SECONDS: f32 = 120.0;

/// VERIFIED 0xBE9C70 (Civ mode): add `amount` (damage, or 80 for a fallen
/// building) from nation `attacker` to a city of nation `owner` holding
/// `capture` points for `capturer`. The owner's own damage counts nothing; a
/// third nation's damage first wears the current capturer's points down and
/// takes over once they pass zero; 100 at most.
pub fn add_capture(
    capture: f32,
    capturer: Option<usize>,
    owner: usize,
    attacker: usize,
    amount: f32,
) -> (f32, Option<usize>) {
    if attacker == owner {
        return (capture, capturer);
    }
    let id = capturer.unwrap_or(attacker);
    let mut value = if id == attacker {
        capture + amount * CAPTURE_PER_DAMAGE
    } else {
        capture - amount * CAPTURE_PER_DAMAGE
    };
    let mut id = id;
    if value < 0.0 {
        value = -value;
        id = attacker;
    }
    (value.min(CAPTURE_FULL), Some(id))
}

/// VERIFIED 0xBE2260: one second of capture decay (only while the city has
/// not been attacked for `ATTACK_MEMORY_SECONDS`); at zero the capturer is
/// forgotten.
pub fn decay_capture(capture: f32, capturer: Option<usize>, quiet: bool) -> (f32, Option<usize>) {
    if !quiet || capturer.is_none() {
        return (capture, capturer);
    }
    let value = capture - CAPTURE_DECAY_PER_SECOND;
    if value <= 0.0 {
        (0.0, None)
    } else {
        (value, capturer)
    }
}

// ================================================================ V3 diplomacy

/// Relationship events, rows of `space_npc_relationship_effects~!_Default`
/// `[initial, add per event, min, max, decay]` (shared by Civ and Space; the
/// Civ-only effects of `CityGameGeneralTuning` follow below).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    Compliment,
    Insult,
    Gift,
    Trade,
    JoinedAlliance,
    Religion,
    BuyCityOver,
    BuyCityUnder,
    BribeNode,
    Hostility,
    DeclaredWar,
    DemandRejected,
    /// `CivAttackRelationshipEffect` -4, at most once per 15 s.
    CivAttack,
    /// `CivAttackTradePartnerRelationshipEffect` -2.
    CivAttackTradePartner,
    /// `RaidRelationshipEffect` -1.
    Raid,
    /// `ConvertCitizenRelationshipDelta` -0.1 / `ConvertVehicleRelationshipDelta` -1.
    ConvertCitizen,
    ConvertVehicle,
    /// `VehicleGiftRelationshipEffect` +1 (gift costs 100).
    VehicleGift,
    /// Diplo Dervish (`diplodervish`).
    Dervish,
    /// `RelationshipEventBrokeDeal` (0x05ADB0AA): a nation whose enemy the
    /// player stops trading with (embargo, 0xCFF010).
    BrokeDeal,
}

/// One event row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelationRow {
    pub initial: f32,
    pub add: f32,
    pub min: f32,
    pub max: f32,
    /// Change toward 0 per decay period (the data stores it negative).
    pub decay: f32,
}

const fn row(initial: f32, add: f32, min: f32, max: f32, decay: f32) -> RelationRow {
    RelationRow { initial, add, min, max, decay }
}

impl RelationKind {
    /// Data rows (tuning). ADAPTED: the Civ effects are applied as their own
    /// rows (value per event, clamped to the -10..10 attitude range); Diplo
    /// Dervish adds +5 (half the range; the tool's value 90 is not a
    /// relationship amount and its use is not traced).
    pub fn row(self) -> RelationRow {
        match self {
            RelationKind::Compliment => row(1.0, 0.0, 0.0, 1.0, 0.0),
            RelationKind::Insult => row(-1.0, -0.5, -3.0, 0.0, 0.0),
            RelationKind::Gift => row(0.4, 0.05, 0.0, 12.0, -0.1),
            RelationKind::Trade => row(0.3, 0.2, 0.0, 5.0, 0.0),
            RelationKind::JoinedAlliance => row(0.0, 1.0, 0.0, 10.0, 0.0),
            RelationKind::Religion => row(-2.0, -0.01, -5.0, 0.0, 0.0),
            RelationKind::BuyCityOver => row(0.0, 1.0, 0.0, 5.0, 0.0),
            RelationKind::BuyCityUnder => row(0.0, -1.0, -5.0, 0.0, 0.0),
            RelationKind::BribeNode => row(0.0, -1.0, -4.0, 0.0, 0.0),
            RelationKind::Hostility => row(-3.0, -0.03, -9.0, 0.0, 0.0),
            RelationKind::DeclaredWar => row(-5.0, -1.0, -10.0, 0.0, 0.0),
            RelationKind::DemandRejected => row(-1.0, -1.0, -5.0, 0.0, 0.0),
            RelationKind::CivAttack => row(CIV_ATTACK_EFFECT, CIV_ATTACK_EFFECT, -10.0, 0.0, 0.0),
            RelationKind::CivAttackTradePartner => {
                row(CIV_ATTACK_TRADE_PARTNER_EFFECT, CIV_ATTACK_TRADE_PARTNER_EFFECT, -10.0, 0.0, 0.0)
            }
            RelationKind::Raid => row(RAID_EFFECT, RAID_EFFECT, -10.0, 0.0, 0.0),
            RelationKind::ConvertCitizen => {
                row(CONVERT_CITIZEN_DELTA, CONVERT_CITIZEN_DELTA, -10.0, 0.0, 0.0)
            }
            RelationKind::ConvertVehicle => {
                row(CONVERT_VEHICLE_DELTA, CONVERT_VEHICLE_DELTA, -10.0, 0.0, 0.0)
            }
            RelationKind::VehicleGift => row(VEHICLE_GIFT_EFFECT, VEHICLE_GIFT_EFFECT, 0.0, 10.0, 0.0),
            RelationKind::Dervish => row(5.0, 5.0, 0.0, 10.0, 0.0),
            // VERIFIED (civ-audit-a): property 0x05ADB0AA of
            // `space_npc_relationship_effects~!_DefaultPersonality`
            // (PatchData) = [-1, -3, -10, 0, 0, 0].
            RelationKind::BrokeDeal => row(-1.0, -3.0, -10.0, 0.0, 0.0),
        }
    }
}

/// `CityGameGeneralTuning` relationship effects (+0x58..+0x80, +0x184..+0x18C).
pub const RAID_EFFECT: f32 = -1.0;
pub const TRADE_EFFECT: f32 = 0.1;
pub const RECRUITING_EFFECT: f32 = -0.2;
pub const GIFT_EFFECT: f32 = 2.0;
pub const VEHICLE_GIFT_EFFECT: f32 = 1.0;
pub const VEHICLE_TRADE_EFFECT: f32 = 0.1;
pub const FAILED_MISSION_EFFECT: f32 = -1.0;
pub const CIV_ATTACK_EFFECT: f32 = -4.0;
pub const CIV_ATTACK_TRADE_PARTNER_EFFECT: f32 = -2.0;
pub const CONVERT_CITIZEN_DELTA: f32 = -0.1;
pub const CONVERT_VEHICLE_DELTA: f32 = -1.0;
pub const BRIBE_VEHICLE_DELTA: f32 = 0.0;
/// `TimeBetweenAttacksForRelationshipPenalty`.
pub const TIME_BETWEEN_ATTACKS: f32 = 15.0;
/// `RelationshipCivDecayPeriod` 60,000 ms.
pub const RELATIONSHIP_DECAY_PERIOD: f32 = 60.0;
/// The attitude range (0xD00BD0 clamps to -10..10).
pub const RELATION_MIN: f32 = -10.0;
pub const RELATION_MAX: f32 = 10.0;
/// VERIFIED (civ-audit-a): the attitude thresholds of 0xD00BD0 are four floats
/// the relationship manager loads at 0xD00F10 from the property file 0x5C770DB7
/// (`RolloverRltnshpVBadCap` -7 at +0x10, `BadCap` -3 at +0x14, `NeutralCap` 3 at
/// +0x18, `GoodCap` 7 at +0x1C: Furious <= -7, Annoyed (-7, -3], Cautious (-3, 3),
/// Pleased [3, 7), Friendly >= 7). Retail Civ has no war flag: a rival raids the
/// cities of the nations it is Annoyed or worse with and keeps its target until
/// that nation is Cautious or better (0xBFA7A0), and the war warning comes at
/// an attitude below Cautious (0xBFBD30), so "at war" is an attitude of Annoyed
/// or worse (`value <= -3`) and the war is over above it. (OpenSpore's 0.82
/// reused Space's -7 to start and 0 to end a war.)
pub const WAR_THRESHOLD: f32 = -3.0;

/// A nation's relationship with the player: the sum of event contributions
/// (cRelationshipManager), each clamped to its row and decaying per period.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Relationship {
    pub contributions: Vec<(RelationKind, f32)>,
    /// Event counts applied the retail way (0xD05DD0 adds the scale to the
    /// event's count; 0xD01730 makes it clamp(initial + count x add)), used
    /// by the comm screen actions (`apply_count`).
    #[serde(default)]
    pub counts: Vec<(RelationKind, f32)>,
    /// Seconds until a new attack costs relationship again.
    pub attack_cooldown: f32,
    pub decay_timer: f32,
}

impl Relationship {
    pub fn value(&self) -> f32 {
        let counted: f32 = self
            .counts
            .iter()
            .map(|(k, n)| {
                let r = k.row();
                (r.initial + n * r.add).clamp(r.min, r.max)
            })
            .sum();
        (self.contributions.iter().map(|(_, v)| v).sum::<f32>() + counted).clamp(RELATION_MIN, RELATION_MAX)
    }

    /// `cRelationshipManager::ApplyRelationship(.., event, scale)` (0xD06280
    /// -> 0xD05DD0): the event's count grows by `scale`; an entry whose count
    /// is back near 0 (|n| < 0.01) is dropped.
    pub fn apply_count(&mut self, kind: RelationKind, scale: f32) {
        if scale == 0.0 {
            return;
        }
        match self.counts.iter().position(|(k, _)| *k == kind) {
            Some(i) => {
                self.counts[i].1 += scale;
                if self.counts[i].1.abs() < 0.01 {
                    self.counts.remove(i);
                }
            }
            None => self.counts.push((kind, scale)),
        }
    }

    /// The event's count (0 when never applied).
    pub fn count(&self, kind: RelationKind) -> f32 {
        self.counts.iter().find(|(k, _)| *k == kind).map_or(0.0, |(_, n)| *n)
    }

    pub fn contribution(&self, kind: RelationKind) -> f32 {
        self.contributions
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0.0, |(_, v)| *v)
    }

    /// Apply one event: the first one sets `initial` (plus nothing), later
    /// ones add `add`; the contribution stays inside the row's range.
    pub fn apply(&mut self, kind: RelationKind) {
        let r = kind.row();
        match self.contributions.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, v)) => *v = (*v + r.add).clamp(r.min, r.max),
            None => self.contributions.push((kind, r.initial.clamp(r.min, r.max))),
        }
    }

    /// Apply an event scaled by `times` (several small deliveries at once).
    pub fn apply_scaled(&mut self, kind: RelationKind, amount: f32) {
        let r = kind.row();
        match self.contributions.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, v)) => *v = (*v + amount).clamp(r.min, r.max),
            None => self.contributions.push((kind, amount.clamp(r.min, r.max))),
        }
    }

    /// A hit by the player on this nation's hall, buildings, turrets or
    /// vehicles. VERIFIED (civ-audit-a): in Civ every damaging hit of another
    /// political id applies the `Hostility` event (0x0526E501) with scale 1 -
    /// `ApplyRelationship` 0xD062B0 from the hall's TakeDamage (0xBCF116), the
    /// buildings' (0xBCFC5A), the turrets' (0xBD0807) and the vehicles' (0xCABA7A):
    /// -3 at the first hit, -0.03 for every further one, down to -9. The
    /// `CivAttackRelationshipEffect` -4 / 15 s of the general tuning that 0.82
    /// used has no reader in the code (nothing refers to its field 0x169CAD8).
    /// Always counts.
    pub fn attacked(&mut self) -> bool {
        self.apply(RelationKind::Hostility);
        true
    }

    /// A hit with a cultural weapon (damage type 3) applies the `Religion` event
    /// (0x0526E504, scale 1) instead of `Hostility`: VERIFIED at the damage
    /// handlers 0xBFFD2C (spice node), 0xCABA73 (vehicle), 0xCBAB06 / 0xCBACD9 /
    /// 0xCBAD15 (projectile hits), -2 at the first hit, -0.01 for every further
    /// one, down to -5.
    pub fn cultural_attacked(&mut self) -> bool {
        self.apply(RelationKind::Religion);
        true
    }

    /// Advance the timers; each decay period moves decaying contributions
    /// toward 0 by their decay.
    pub fn tick(&mut self, dt: f32) {
        self.attack_cooldown = (self.attack_cooldown - dt).max(0.0);
        self.decay_timer += dt;
        while self.decay_timer >= RELATIONSHIP_DECAY_PERIOD {
            self.decay_timer -= RELATIONSHIP_DECAY_PERIOD;
            for (kind, v) in &mut self.contributions {
                let d = kind.row().decay.abs();
                if d > 0.0 {
                    *v = if *v > 0.0 { (*v - d).max(0.0) } else { (*v + d).min(0.0) };
                }
            }
        }
    }

    pub fn hostile(&self) -> bool {
        self.value() <= WAR_THRESHOLD
    }

    pub fn peaceful(&self) -> bool {
        self.value() > WAR_THRESHOLD
    }
}

/// `TimeInAlmostConvert`, `TimeInAlmostDestroy`, `TimeInConverted` (tuning):
/// a city whose conversion is done stays "almost" 2 s before it changes
/// hands, then shows "converted" 4 s. (A military capture is immediate:
/// 0xBE9C70 calls 0xBE9AD0 as soon as the points reach 100.)
pub const TIME_IN_ALMOST_CONVERT: f32 = 2.0;
#[allow(dead_code)] // tuning name only: the "almost destroyed" state is not modelled
pub const TIME_IN_ALMOST_DESTROY: f32 = 2.0;
pub const TIME_IN_CONVERTED: f32 = 4.0;
/// `CounterConversionInterval` 30 s, `EstablishTempleSingTime` 4 s, the
/// conversion power modifiers and crowd values (names only; the allegiance
/// bucket model of 0xBE1340 is not reproduced).
pub const COUNTER_CONVERSION_INTERVAL: f32 = 30.0;
pub const ESTABLISH_TEMPLE_SING_TIME: f32 = 4.0;
pub const CONVERSION_GROUP_POWER_MODIFIER: f32 = 0.15;
pub const CONVERSION_GROUP_SIZE_MODIFIER: f32 = 0.15;
pub const BASE_CONVERSION_STRENGTH: f32 = 0.0;
pub const CROWD_DPS_BASE: f32 = 12.0;
pub const CROWD_CONVERT_OFFSET: f32 = 0.25;
/// `CrowdReationModifier` (tuning +0x1BC = 0x169CC1C, the factor 0xCEEA80 reads).
pub const CROWD_REACTION_MODIFIER: f32 = 1.0;

// ---- the conversion model (VERIFIED 0xBEB310 / 0xBEB1E0 / 0xBE2260 / 0xBE1340)
//
// A city keeps its citizens (`cCity` +0x64C) in per-political-id buckets (the map
// at +0x5A0): all of them belong to the owner until a faith converts them. Once a
// second every group of vehicles performing at the city (the vehicles of one
// political id) takes a step (0xBEB310): the group's strength is
// `(power + 1) x ConversionConversionGroupPowerModifier + size x
// ConversionConversionGroupSizeModifier + BaseConversionStrength` (0xCEEA40) with
// `power` the sum of `(vehicle power stat + 0.5) x 2/3` (0xBD8710) and `size` the
// number of vehicles; the crowd's reaction (0xCEEA80) is that minus half of
// `mood / 100 - 0.5` minus 1, snapped to [-1, 1] with a minimum size of 0.05; and
// one citizen changes allegiance (0xBEB1E0) with the odds `y + 0.1`, `y = 0.25 +
// reaction` above 0 else `(reaction + 1) x 0.25` (0xCEEF10). The city's conversion
// percent (0xBE2260) is `200 x leader / (leader + owner)` of the two buckets, and
// at 100 (as many citizens in the leading faith as in the owner's) the city
// changes hands by faith (0xBEAB80 -> `SetOwner` method 1). Every
// `CounterConversionInterval` (30 s, 0xBE1340) the owner wins back one citizen
// from each faith with the odds `(mood / 100)^3 + 0.2` (0xCEEEB0).

/// `[0x140E8A4]`: the factor of a vehicle's power in the group power (0xBD8710).
pub const GROUP_POWER_FACTOR: f32 = 2.0 / 3.0;

/// One vehicle's share of the group power (0xBD8710): `(power + 0.5) x 2/3`.
pub fn vehicle_group_power(power_stat: f32) -> f32 {
    (power_stat + 0.5) * GROUP_POWER_FACTOR
}

/// 0xCEEA40: the strength of a group of `size` vehicles with `group_power`.
pub fn conversion_strength(group_power: f32, size: u32) -> f32 {
    (group_power + 1.0) * CONVERSION_GROUP_POWER_MODIFIER
        + size as f32 * CONVERSION_GROUP_SIZE_MODIFIER
        + BASE_CONVERSION_STRENGTH
}

/// 0xCEEA80: the crowd's reaction to a group of `strength` at a city of `mood`
/// (0..100), -1..1 and never closer to 0 than 0.05.
pub fn crowd_reaction(strength: f32, mood: i32) -> f32 {
    let m = mood.clamp(0, 100) as f32 * 0.01;
    let v = (strength - 0.5 * (m - 0.5) - CROWD_REACTION_MODIFIER) * CROWD_REACTION_MODIFIER;
    if v > 1.0 {
        1.0
    } else if v >= 0.0 {
        v.max(0.05)
    } else if v > -0.05 {
        -0.05
    } else {
        v.max(-1.0)
    }
}

/// 0xCEEF10: the odds that a group converts one citizen this second.
pub fn conversion_odds(reaction: f32) -> f32 {
    let y = if reaction > 0.0 {
        CROWD_CONVERT_OFFSET + reaction
    } else {
        (reaction + 1.0) * CROWD_CONVERT_OFFSET
    };
    if y > 0.0 {
        y + 0.1
    } else {
        0.0
    }
}

/// 0xBE2260: the conversion percent of a city, 0..100, from the leading faith's
/// bucket and the owner's.
pub fn conversion_percent(leader: u32, owner: u32) -> f32 {
    let total = leader + owner;
    if total == 0 {
        0.0
    } else {
        (leader as f32 * 200.0 / total as f32).clamp(0.0, 100.0)
    }
}

// ---- buying a city (VERIFIED, 0xBE93A0 -> 0xCEECE0, value 0xBE6EE0)

/// `CostToBuyCityModifier` (tuning +0x1AC).
pub const COST_TO_BUY_CITY_MODIFIER: f32 = 1.1;
pub const BUY_PRICE_MIN: f32 = 2000.0;
pub const BUY_PRICE_MAX: f32 = 16000.0;
/// The trade value subtracted from the price is capped at 4000.
pub const TRADE_VALUE_MAX: f32 = 4000.0;

/// City value: 1.2 x building costs + 0.5 x live turret costs + civic objects
/// + 500, doubled when the owner is not a neutral civilization.
pub fn city_value(building_costs: f32, live_turret_costs: f32, civic: f32, owner_neutral: bool) -> f32 {
    let v = 1.2 * building_costs + 0.5 * live_turret_costs + civic + 500.0;
    if owner_neutral {
        v
    } else {
        v * 2.0
    }
}

/// Price: `1.1 x value x (ownerCities + 3) x 0.2` (x2/3 for a one-city
/// owner) minus the trade value (at most 4000), clamped to 2000..16000.
pub fn buy_price(value: f32, owner_cities: usize, trade_value: f32) -> f32 {
    let cities = owner_cities as f32;
    let mut price = COST_TO_BUY_CITY_MODIFIER * value * (cities + 3.0) * 0.2;
    if owner_cities == 1 {
        price = price * 2.0 * 0.333_333_34;
    }
    price -= trade_value.clamp(0.0, TRADE_VALUE_MAX);
    price.clamp(BUY_PRICE_MIN, BUY_PRICE_MAX)
}

/// VERIFIED (0xBE19B0): a trade delivery of `cargo` pays the city's owner
/// 8 x cargo / 6 and the trader 4 x cargo / 6 (a third of that inside one
/// civilization). Returns (owner, trader).
pub fn trade_delivery(cargo: f32, same_civ: bool) -> (f32, f32) {
    let mut v = cargo / 6.0;
    if same_civ {
        v *= 0.333_333_34;
    }
    (v * 8.0, v * 4.0)
}

// ---- NPC city types (VERIFIED, 0xCF8D00)

/// `MilitaryProb` / `CulturalProb` and their major / minor NPC variants: one
/// roll in [0, 1) against cumulative thresholds (military below the first,
/// religious below the second, economic above). These are city types chosen
/// at planet generation, not unit odds.
pub const CITY_TYPE_ODDS: (f32, f32) = (0.333, 0.667);
pub const CITY_TYPE_ODDS_MAJOR_NPC: (f32, f32) = (0.5, 1.0);
pub const CITY_TYPE_ODDS_MINOR_NPC: (f32, f32) = (0.25, 0.5);
/// 0xCF8D00 retries the whole draw up to 30 times until the civilization has a
/// military city.
pub const CITY_TYPE_RETRIES: u32 = 30;

/// 0 military, 1 religious (cultural), 2 economic.
pub fn npc_city_type(roll: f32, odds: (f32, f32)) -> usize {
    if roll < odds.0 {
        0
    } else if roll < odds.1 {
        1
    } else {
        2
    }
}

/// VERIFIED (0xCF8D00, called from the level setup 0xB9EE41 after every slot is
/// placed): roll the specialty of every NPC slot and roll everything again, at
/// most [`CITY_TYPE_RETRIES`] times in all (the counter compared with 0x1E at
/// 0xCF8E2C), until an NPC slot on the player's continent is military
/// (`on_player_continent[i]` tells whether slot `i` lies on it; INFERRED that
/// the argument compared with a slot's `+0x20` is the player's continent id).
/// Every NPC slot rolls against the *minor* odds ([`CITY_TYPE_ODDS_MINOR_NPC`],
/// read at 0x169CC50 / 0x169CC54): the slot constructor 0xAE41B0 sets `+0x25 =
/// 1` and the NPC slot writer 0xB9AA20 (which sets `+0x24 = 0`, the continent
/// `+0x20`, ...) never clears it, so the major odds are never used. Returns
/// 0 military, 1 religious, 2 economic per slot (0xBEFA70 stores them as 0, 2, 1).
pub fn roll_npc_specialties(on_player_continent: &[bool], mut random: impl FnMut() -> f32) -> Vec<u8> {
    let mut kinds = vec![0u8; on_player_continent.len()];
    for _ in 0..CITY_TYPE_RETRIES {
        for kind in kinds.iter_mut() {
            *kind = npc_city_type(random(), CITY_TYPE_ODDS_MINOR_NPC) as u8;
        }
        if kinds.iter().zip(on_player_continent).any(|(k, on)| *k == 0 && *on) {
            break;
        }
    }
    kinds
}

// ======================================================= NPC city building (civ-final)

/// VERIFIED 0xBF7370 (the NPC civ's once-a-second update, before its cities
/// build): the money in the purse (+0x98) goes to the domestic budget
/// (+0x4AC: buildings and turrets) and the expansion budget (+0x4B0: vehicles
/// and harvesters) by the domestic share: everything on the first budget (and
/// always for neutral civs), else 0.8 / 0.6 while the domestic budget is at
/// most 2000, then nothing.
pub const DOMESTIC_FILL_LIMIT: f32 = 2000.0;

/// VERIFIED 0xBF7370: the domestic share of the money. 0.8 for an economic
/// civ (0xBF1080 type 1) or one that shares no continent with another nation
/// (0xBF7190 / 0xBF5860), else 0.6.
pub fn domestic_share(first_budget: bool, economic: bool, shares_continent: bool, domestic: f32) -> f32 {
    if first_budget {
        1.0
    } else if domestic > DOMESTIC_FILL_LIMIT {
        0.0
    } else if economic || !shares_continent {
        0.8
    } else {
        0.6
    }
}

/// VERIFIED 0xBF7370: both budgets are capped at 3000 / 9000 / 27000 (easy /
/// normal / hard).
pub fn domestic_cap(difficulty: Difficulty) -> f32 {
    match difficulty {
        Difficulty::Easy => 3000.0,
        Difficulty::Medium => 9000.0,
        Difficulty::Hard => 27000.0,
    }
}

/// VERIFIED 0xBE5890 (per NPC city, after the budget split): nothing is built
/// below 1000 in the domestic budget.
pub const NPC_BUILD_MIN_BUDGET: f32 = 1000.0;

/// VERIFIED 0xBE5890: turrets wait until the city has stood 200 / 100 / 50 s
/// (its cGonzagoTimer +0x6E8, started by the cCity constructor 0xBE6460;
/// doubled for neutral civs).
pub fn npc_turret_wait_seconds(difficulty: Difficulty) -> f32 {
    match difficulty {
        Difficulty::Easy => 200.0,
        Difficulty::Medium => 100.0,
        Difficulty::Hard => 50.0,
    }
}

/// VERIFIED 0xBE5890: the odds factor `c` = 5 / 4 / 3 (easy / normal / hard):
/// a building is considered with a chance of 1 in `8c` a second (always when
/// the city earns nothing), a quiet-time turret with 1 in `10c`.
pub fn npc_build_odds(difficulty: Difficulty) -> u32 {
    match difficulty {
        Difficulty::Easy => 5,
        Difficulty::Medium => 4,
        Difficulty::Hard => 3,
    }
}

/// VERIFIED 0xBE5890: under attack (0xC00600 slots 0 / 1) a city on normal or
/// hard raises a turret only while it has fewer than 2 (1 for neutral civs).
pub const NPC_EMERGENCY_TURRETS: usize = 2;

/// VERIFIED 0xBD8BF0: turret slots by the city's vehicle specialty (+0x540:
/// 0 military, 1 religious, 2 economic) 8 / 6 / 4 behind large walls, 5 / 4 / 3
/// behind small walls (the city's +0x519 flag, set for the cities of neutral
/// civilizations: `cCity::Init` 0xBE7D40). The installed walls PROPs carry
/// exactly these numbers of `Turrets` (`city_layout`).
pub fn max_turrets(specialty: usize, small_walls: bool) -> usize {
    match (specialty, small_walls) {
        (0, false) => 8,
        (1, false) => 6,
        (0, true) => 5,
        (1, true) => 4,
        (_, false) => 4,
        (_, true) => 3,
    }
}

/// The large-walls turret slots of a specialty (`max_turrets`).
pub fn npc_max_turrets(specialty: usize) -> usize {
    max_turrets(specialty, false)
}

/// VERIFIED 0xBE5890: in quiet times the turret goal is 2/5, 3/5, 4/5 of the
/// turret slots (easy / normal / hard), and at most (standing buildings + 1)
/// / 2 (0xBD9D00; 1 for neutral civs).
pub fn npc_quiet_turret_goal(slots: usize, standing_buildings: usize, difficulty: Difficulty) -> usize {
    let share = match difficulty {
        Difficulty::Easy => slots * 2 / 5,
        Difficulty::Medium => slots * 3 / 5,
        Difficulty::Hard => slots * 4 / 5,
    };
    share.min((standing_buildings + 1) / 2)
}

/// VERIFIED 0xBDB6E0: the score of a free building slot for one building kind
/// is summed over the slots linked to it: a factory next to a house or the
/// hall +50 (+80 while the city earns nothing, 0xBD8CD0), next to an
/// entertainment building -10; an entertainment building next to a house or
/// the hall +(90 - happiness), +20 more at happiness 50 or below (0xBD7F70),
/// next to a factory -10; a house next to a factory as the factory's +50 / 80,
/// next to an entertainment building as its +(90 - happiness) (+20), next to
/// an empty slot +10. The best slot must score above 0.
pub fn npc_link_score(kind: CityBuilding, neighbour: Option<CityBuilding>, happiness: i32, earning: bool) -> i32 {
    use CityBuilding::*;
    let income = if earning { 50 } else { 80 };
    let happy = 90 - happiness + if happiness <= 50 { 20 } else { 0 };
    match (kind, neighbour) {
        (Factory, Some(House | CityHall)) => income,
        (Factory, Some(Entertainment)) => -10,
        (Entertainment, Some(House | CityHall)) => happy,
        (Entertainment, Some(Factory)) => -10,
        (House, Some(Factory)) => income,
        (House, Some(Entertainment)) => happy,
        (House, None) => 10,
        _ => 0,
    }
}

// ==================================================================== V4 world

/// `MinDistanceBetweenCities`, `MinDistanceBetweenTribes`,
/// `MaxDistanceForInteractWithTribes`, `CityFlattenSize`, `RadiusFromCityHall`
/// (tuning).
pub const MIN_DISTANCE_BETWEEN_CITIES: f32 = 180.0;
pub const MIN_DISTANCE_BETWEEN_TRIBES: f32 = 50.0;
pub const MAX_DISTANCE_FOR_INTERACT_WITH_TRIBES: f32 = 110.0;
pub const CITY_FLATTEN_SIZE: f32 = 75.0;
pub const RADIUS_FROM_CITY_HALL: f32 = 15.0;
/// Walls: `MinNumWallSegments` 5, `DistBetweenWallPosts` 28.
pub const MIN_NUM_WALL_SEGMENTS: u32 = 5;
pub const DIST_BETWEEN_WALL_POSTS: f32 = 28.0;
/// City placement markers (`noun_placement_markers~`, distance from the
/// origin): PlayerCity / genericCity / anyLandCity 123-248, landCity 123-348,
/// seaportCity 128-348, spaceSeaportCity 123-323, Civ_GoodyHut 121-573.
pub const PLAYER_CITY_RING: (f32, f32) = (123.0, 248.0);
pub const LAND_CITY_RING: (f32, f32) = (123.0, 348.0);
pub const SEAPORT_CITY_RING: (f32, f32) = (128.0, 348.0);
/// `CivGameLevelGenerationList`: resources keep 90 from cities, obstacle radius
/// +16 / 32 / 58 by herd size, flattening brush 120.
pub const MIN_RESOURCE_RADIUS: f32 = 90.0;
pub const OBSTACLE_RADIUS_INCREASE: [f32; 3] = [16.0, 32.0, 58.0];
/// `speciesFiles`: 8 x `CVG_NPC_Epic_Predator` (species 0x8D7B4F17).
pub const EPIC_PREDATOR_COUNT: usize = 8;
pub const EPIC_PREDATOR_SPECIES: u32 = 2_373_668_631;
/// Casting manager: animals shown inside 150, culled beyond 200; at most 15
/// animals at full density from a pool of 60; animals keep 60 from cities;
/// 2 visualized tribes; slope limit 30; update every 2 s, 2 herds per update.
pub const CASTING_DISTANCE_THRESHOLDS: (f32, f32) = (150.0, 200.0);
pub const ANIMALS_AT_MAX_DENSITY: usize = 15;
pub const MAX_ANIMAL_POOL_SIZE: usize = 60;
pub const CITY_RADIUS_BUFFER: f32 = 60.0;
pub const MAX_VISUALIZED_TRIBES: usize = 2;
pub const ANIMAL_STAGE_MAX_SLOPE: f32 = 30.0;
pub const CASTING_UPDATE_TIME: f32 = 2.0;
pub const MAX_HERDS_ACTIVATED_PER_INTERVAL: usize = 2;

/// The rival cities' ring around the capital: the minimum city spacing.
pub fn rival_ring_radius() -> f32 {
    world(MIN_DISTANCE_BETWEEN_CITIES)
}

/// Whether two city centres keep the retail spacing.
pub fn cities_spaced(distance: f32) -> bool {
    distance + 1e-3 >= world(MIN_DISTANCE_BETWEEN_CITIES)
}

/// Whether a spice geyser keeps `minResourceRadius` from a city.
pub fn resource_spaced(distance: f32) -> bool {
    distance + 1e-3 >= world(MIN_RESOURCE_RADIUS)
}

/// Casting visibility with hysteresis: shown inside 150, hidden beyond 200.
pub fn animal_visible(distance: f32, was_visible: bool) -> bool {
    let (show, hide) = CASTING_DISTANCE_THRESHOLDS;
    if was_visible {
        distance <= world(hide)
    } else {
        distance <= world(show)
    }
}

// ================================================ consequence abilities

/// VERIFIED (0xBF6DF0): use costs by card stage.
pub const CELL_POWER_COST: f32 = 2000.0;
pub const CREATURE_POWER_COST: f32 = 4000.0;
pub const TRIBAL_POWER_COST: f32 = 16000.0;
pub const SUPERWEAPON_COST_PER_CITY: f32 = 12000.0;

/// VERIFIED (0xBF6DF0): a superweapon costs 12,000 per city not yet owned
/// (at least one).
pub fn superweapon_cost(cities_not_owned: usize) -> f32 {
    SUPERWEAPON_COST_PER_CITY * cities_not_owned.max(1) as f32
}

/// Locale table of the ability names and descriptions.
pub const POWER_LOCALE_TABLE: u32 = 0x9A5A_CF10;

/// One Civ ability: the consequence superpower record, its `spacetools~` tool
/// (table 0x014693E8 indexed by 0xBEFAD0) and the en-us strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerData {
    pub consequence: &'static str,
    pub consequence_id: u32,
    pub tool: &'static str,
    pub name_id: u32,
    pub description_id: u32,
    pub name: &'static str,
    pub description: &'static str,
    /// Tool `MinDamage` / `MaxDamage` (a duration in seconds for the timed
    /// abilities; ADAPTED reading where noted at the use).
    pub min_damage: f32,
    pub max_damage: f32,
    /// Tool `DamageArea` (retail radius).
    pub area: f32,
    /// Tool `RechargeRate`: seconds before the next use (0: once).
    pub recharge: f32,
    pub cost: f32,
    pub damages_friendlies: bool,
}

impl PowerData {
    /// The area in world units.
    pub fn radius(&self) -> f32 {
        world(self.area)
    }
}

pub const INVULNERABILITY: PowerData = PowerData {
    consequence: "cvg_clg_meat",
    consequence_id: 0x0756_D422,
    tool: "i_field",
    name_id: 0x0001_0031,
    description_id: 0x0001_0032,
    name: "Invulnerability",
    description: "This ability makes your vehicles impervious to damage for a limited time.",
    min_damage: 15.0,
    max_damage: 15.0,
    area: 100.0,
    recharge: 60.0,
    cost: CELL_POWER_COST,
    damages_friendlies: false,
};
pub const HEALING_AURA: PowerData = PowerData {
    consequence: "cvg_clg_plant",
    consequence_id: 0x045C_DF1A,
    tool: "faithheal",
    name_id: 0x0001_0033,
    description_id: 0x0001_0034,
    name: "Healing Aura",
    description: "This ability repairs all of your vehicles and buildings.",
    min_damage: 150.0,
    max_damage: 300.0,
    area: 100.0,
    recharge: 60.0,
    cost: CELL_POWER_COST,
    damages_friendlies: false,
};
pub const STATIC_BOMB: PowerData = PowerData {
    consequence: "cvg_clg_mix",
    consequence_id: 0xC061_7B2F,
    tool: "emp",
    name_id: 0x0001_0035,
    description_id: 0x0001_0036,
    name: "Static Bomb",
    description: "This ability temporarily immobilizes vehicles, turrets and buildings.",
    min_damage: 15.0,
    max_damage: 15.0,
    area: 64.0,
    recharge: 60.0,
    cost: CELL_POWER_COST,
    damages_friendlies: false,
};
pub const MIGHTY_BOMB: PowerData = PowerData {
    consequence: "cvg_crg_attack",
    consequence_id: 0x2CDB_D6A1,
    tool: "napalm",
    name_id: 0x0001_0037,
    description_id: 0x0001_0038,
    name: "Mighty Bomb",
    description: "This ability damages all buildings and vehicles within its radius.",
    min_damage: 600.0,
    max_damage: 600.0,
    area: 40.0,
    recharge: 180.0,
    cost: CREATURE_POWER_COST,
    damages_friendlies: false,
};
pub const DIPLO_DERVISH: PowerData = PowerData {
    consequence: "cvg_crg_social",
    consequence_id: 0x4A78_672C,
    tool: "diplodervish",
    name_id: 0x0001_0039,
    description_id: 0x0001_003A,
    name: "Diplo Dervish",
    description: "This ability raises your relationship with a nation you target.",
    min_damage: 90.0,
    max_damage: 90.0,
    area: 100.0,
    recharge: 180.0,
    cost: CREATURE_POWER_COST,
    damages_friendlies: false,
};
pub const BRIBE_BOMB: PowerData = PowerData {
    consequence: "cvg_crg_mix",
    consequence_id: 0x3221_E759,
    tool: "espionage",
    name_id: 0x0001_003B,
    description_id: 0x0001_003C,
    name: "Bribe Bomb",
    description: "This ability causes enemy vehicles within the bomb's radius to attack each other.",
    min_damage: 20.0,
    max_damage: 20.0,
    area: 100.0,
    recharge: 180.0,
    cost: CREATURE_POWER_COST,
    damages_friendlies: false,
};
pub const GADGET_BOMB: PowerData = PowerData {
    consequence: "cvg_trg_attack",
    consequence_id: 0x0082_26D0,
    tool: "tacnuke",
    name_id: 0x0001_003D,
    description_id: 0x0001_003E,
    name: "Gadget Bomb",
    description: "This ability causes damage to buildings and vehicles and can help capture cities.",
    min_damage: 6000.0,
    max_damage: 6000.0,
    area: 95.0,
    recharge: 300.0,
    cost: TRIBAL_POWER_COST,
    damages_friendlies: false,
};
pub const BLACK_CLOUD: PowerData = PowerData {
    consequence: "cvg_trg_social",
    consequence_id: 0xFD05_AB6D,
    tool: "blackcloud",
    name_id: 0x0001_003F,
    description_id: 0x0001_0040,
    name: "Black Cloud",
    description: "This ability shuts down entertainment buildings and turrets causing unhappiness.",
    min_damage: 120.0,
    max_damage: 120.0,
    area: 100.0,
    recharge: 300.0,
    cost: TRIBAL_POWER_COST,
    damages_friendlies: true,
};
pub const AD_BLITZ: PowerData = PowerData {
    consequence: "cvg_trg_mix",
    consequence_id: 0x08A9_1F1E,
    tool: "adblitz",
    name_id: 0x0001_0041,
    description_id: 0x0001_0042,
    name: "Ad Blitz",
    description: "This ability enables faster city buyout.",
    min_damage: 100.0,
    max_damage: 100.0,
    area: 100.0,
    recharge: 300.0,
    cost: TRIBAL_POWER_COST,
    damages_friendlies: false,
};
pub const ICBM: PowerData = PowerData {
    consequence: "cvg_trg_attackwin",
    consequence_id: 0x3621_3C6A,
    tool: "icbm",
    name_id: 0x0001_006F,
    description_id: 0x0001_0070,
    name: "ICBM",
    description: "Destroys all cities",
    min_damage: 6000.0,
    max_damage: 6000.0,
    area: 95.0,
    recharge: 0.0,
    cost: SUPERWEAPON_COST_PER_CITY,
    damages_friendlies: false,
};
pub const FANATICAL_UPRISING: PowerData = PowerData {
    consequence: "cvg_trg_socialwin",
    consequence_id: 0x8129_9805,
    tool: "rising",
    name_id: 0x0001_006B,
    description_id: 0x0001_006C,
    name: "Fanatical Uprising",
    description: "Converts all cities",
    min_damage: 500.0,
    max_damage: 500.0,
    area: 100.0,
    recharge: 0.0,
    cost: SUPERWEAPON_COST_PER_CITY,
    damages_friendlies: false,
};
pub const GLOBAL_MERGER: PowerData = PowerData {
    consequence: "cvg_trg_mixwin",
    consequence_id: 0xFC71_6A94,
    tool: "globaladblitz",
    name_id: 0x0001_006D,
    description_id: 0x0001_006E,
    name: "Global Merger",
    description: "Buys all cities",
    min_damage: 500.0,
    max_damage: 500.0,
    area: 100.0,
    recharge: 0.0,
    cost: SUPERWEAPON_COST_PER_CITY,
    damages_friendlies: false,
};

/// Every ability in the order of tool table 0x014693E8 (0xBEFAD0 indices).
pub const POWER_TABLE: [PowerData; 12] = [
    MIGHTY_BOMB,
    GADGET_BOMB,
    ICBM,
    HEALING_AURA,
    BLACK_CLOUD,
    FANATICAL_UPRISING,
    BRIBE_BOMB,
    AD_BLITZ,
    GLOBAL_MERGER,
    STATIC_BOMB,
    INVULNERABILITY,
    DIPLO_DERVISH,
];

// ======================================================================= tests

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(s: &str) -> u32 {
        crate::world_objects::name_id(s)
    }

    #[test]
    fn the_conversion_model_follows_the_retail_formulas() {
        // 0xBD8710: a vehicle adds (power stat + 0.5) x 2/3 to its group's power.
        assert!((vehicle_group_power(0.33) - 0.553_333).abs() < 1e-4);
        // 0xCEEA40: (power + 1) x 0.15 + size x 0.15 + 0.
        let strength = conversion_strength(vehicle_group_power(0.33), 1);
        assert!((strength - 0.383).abs() < 1e-3, "{strength}");
        assert!((conversion_strength(0.0, 0) - 0.15).abs() < 1e-6);
        // 0xCEEA80 at mood 50: the strength minus 1; a happy city resists, an unhappy one yields.
        let neutral = crowd_reaction(strength, 50);
        assert!((neutral + 0.617).abs() < 1e-3, "{neutral}");
        assert!(crowd_reaction(strength, 100) < neutral && neutral < crowd_reaction(strength, 0));
        // Snapped to [-1, 1] and never closer to 0 than 0.05.
        assert_eq!(crowd_reaction(1.0, 50), 0.05);
        assert_eq!(crowd_reaction(0.99, 50), -0.05);
        assert_eq!(crowd_reaction(5.0, 50), 1.0);
        assert_eq!(crowd_reaction(0.0, 100), -1.0);
        // 0xCEEF10: y = 0.25 + r above 0 else (r + 1) x 0.25, odds y + 0.1 (none at y = 0).
        assert!((conversion_odds(neutral) - 0.1957).abs() < 1e-3);
        assert!((conversion_odds(0.05) - 0.4).abs() < 1e-6);
        assert!(conversion_odds(1.0) > 1.0, "a strong group converts every second");
        assert_eq!(conversion_odds(-1.0), 0.0);
        // 0xBE2260: 200 x leader / (leader + owner), 100 at an equal split.
        assert_eq!(conversion_percent(0, 0), 0.0);
        assert_eq!(conversion_percent(15, 15), 100.0);
        assert_eq!(conversion_percent(5, 15), 50.0);
        assert_eq!(conversion_percent(20, 10), 100.0);
    }

    #[test]
    fn rivals_hold_back_on_easy_and_medium_while_they_outnumber_the_player() {
        use Difficulty::*;
        assert!(attack_held_back(Easy, 1, 3, false, false));
        assert!(attack_held_back(Easy, 1, 3, true, false), "easy always");
        assert!(attack_held_back(Medium, 1, 3, false, false));
        assert!(!attack_held_back(Medium, 1, 3, true, false), "medium once the player had two cities");
        assert!(!attack_held_back(Hard, 1, 3, false, false));
        assert!(!attack_held_back(Easy, 3, 3, false, false), "not while the player has as many");
        assert!(!attack_held_back(Easy, 1, 3, false, true), "a rival with a target keeps it");
    }

    #[test]
    fn a_cultural_hit_is_the_religion_event_not_hostility() {
        let mut r = Relationship::default();
        assert!(r.cultural_attacked());
        assert!((r.contribution(RelationKind::Religion) + 2.0).abs() < 1e-5);
        assert_eq!(r.contribution(RelationKind::Hostility), 0.0);
        for _ in 0..600 {
            r.cultural_attacked();
        }
        assert_eq!(r.contribution(RelationKind::Religion), -5.0, "floor -5");
    }

    #[test]
    fn world_scale_keeps_the_retail_distances() {
        assert!((rival_ring_radius() - 180.0).abs() < 1e-4);
        assert!((world(100.0) - 100.0).abs() < 1e-3);
        assert!(cities_spaced(180.0) && !cities_spaced(179.0));
        assert!(resource_spaced(world(90.0)) && !resource_spaced(89.0));
        // Casting hysteresis: shown inside 150, kept until 200.
        assert!(animal_visible(world(140.0), false));
        assert!(!animal_visible(world(170.0), false));
        assert!(animal_visible(world(170.0), true));
        assert!(!animal_visible(world(210.0), true));
    }

    #[test]
    fn start_values_and_the_tech_level_follow_the_original() {
        assert_eq!(INITIAL_WEALTH, 1000.0);
        assert_eq!((INITIAL_CREATURES, INITIAL_TRIBE_CREATURES, INITIAL_TRIBES), (20, 7, 1));
        // VERIFIED 0xBF44B0 on the ten-city planet: more than three cities open
        // air (level 3), the whole planet is level 4.
        assert_eq!(vehicle_tech_level(0, 10), 1);
        assert_eq!(vehicle_tech_level(1, 10), 2);
        assert_eq!(vehicle_tech_level(3, 10), 2);
        assert_eq!(vehicle_tech_level(4, 10), AIR_TECH_LEVEL);
        assert_eq!(vehicle_tech_level(9, 10), AIR_TECH_LEVEL);
        assert_eq!(vehicle_tech_level(10, 10), PLANET_TECH_LEVEL);
        // A planet of fewer slots follows the same rule.
        assert_eq!(vehicle_tech_level(5, 5), PLANET_TECH_LEVEL);
        assert!(air_unlocked(3) && !air_unlocked(2));
        assert!(sea_unlocked(2) && !sea_unlocked(1));
        assert!(planet_won(4) && !planet_won(3));
        // The level never falls when a city is lost.
        assert_eq!(raise_tech_level(4, 1, 5), 4);
        assert_eq!(raise_tech_level(0, 0, 5), 1);
    }

    #[test]
    fn the_ability_flags_follow_the_writer_of_0xbf2510() {
        // Trait ids -> flag index (0xBEFAD0), through the installed names.
        for (name, index) in [
            ("cvg_crg_attack", 0),
            ("cvg_trg_attack", 1),
            ("cvg_trg_attackwin", 2),
            ("cvg_clg_plant", 3),
            ("cvg_trg_social", 4),
            ("cvg_trg_socialwin", 5),
            ("cvg_crg_mix", 6),
            ("cvg_trg_mix", 7),
            ("cvg_trg_mixwin", 8),
            ("cvg_clg_mix", 9),
            ("cvg_clg_meat", 10),
            ("cvg_crg_social", 11),
        ] {
            assert_eq!(ability_index(fnv(name)), Some(index), "{name}");
        }
        assert_eq!(ability_index(0x1234), None);
        // Cell 1 city, Creature 2, Tribal 4, superweapons 6 (retail).
        assert_eq!(ABILITY_CITIES, [2, 4, 6, 1, 4, 6, 2, 4, 6, 1, 1, 2]);
        // A civilization with a Cell, a Creature and a Tribal trait and a
        // military capital, on a ten-city planet.
        let traits = [10, 0, 1];
        let at = |n| ability_flags(&traits, Some(2), n, 10);
        assert_eq!(at(0), [false; ABILITY_COUNT]);
        assert!(at(1)[10] && !at(1)[0] && !at(1)[1] && !at(1)[2]);
        assert!(at(2)[10] && at(2)[0] && !at(2)[1]);
        assert!(at(4)[1] && !at(4)[2]);
        assert!(at(6)[2], "the superweapon of the capital's type at six cities");
        // Every city held: the superweapon flags are cleared (0xBF2706).
        assert!(!ability_flags(&traits, Some(2), 10, 10)[2]);
        // A trait that is itself a superweapon replaces the capital's.
        let flags = ability_flags(&[5], Some(2), 6, 10);
        assert!(flags[5] && !flags[2]);
        // The superweapon count is retail's 6 on the ten-slot planet.
        assert_eq!(ability_cities(2, LEVEL_NUM_CITIES), 6);
        assert_eq!(ability_cities(1, LEVEL_NUM_CITIES), 4);
        assert!(!ability_flags(&traits, Some(2), 5, 10)[2]);
        assert!(ability_flags(&traits, Some(2), 6, 10)[2]);
        assert!(ability_flags(&traits, Some(2), 9, 10)[2]);
    }

    #[test]
    fn the_level_setup_fills_the_npc_slots_in_the_retail_order() {
        assert_eq!(LEVEL_NUM_CITIES, 10);
        // Three on the player's continent, one coastal city per other
        // continent, then the continent with the largest size / cities.
        let others = [(1usize, 40usize), (2, 100), (3, 10)];
        let wants = npc_slot_wants(LEVEL_NUM_CITIES - 1, &others);
        assert_eq!(wants.len(), 9);
        assert_eq!(wants[..3], [SlotWant::PlayerContinent; 3]);
        assert_eq!(
            wants[3..6],
            [SlotWant::CoastalOn(1), SlotWant::CoastalOn(2), SlotWant::CoastalOn(3)]
        );
        // 100 / 1 wins, then 100 / 2 = 50 beats 40 / 1, then 100 / 3 = 33 loses to 40.
        assert_eq!(
            wants[6..],
            [SlotWant::Continent(2), SlotWant::Continent(2), SlotWant::Continent(1)]
        );
        // No other continent: every slot stays (ADAPTED), on the player's.
        let alone = npc_slot_wants(9, &[]);
        assert!(alone.iter().all(|w| *w == SlotWant::PlayerContinent));
        // Fewer slots than the three player-continent ones.
        assert_eq!(npc_slot_wants(2, &others).len(), 2);
    }

    #[test]
    fn npc_specialties_follow_the_minor_thresholds_and_reroll_for_a_military_neighbour() {
        // 0xCF8D00: below 0.25 military, below 0.5 religious, else economic.
        let of = |r: f32| npc_city_type(r, CITY_TYPE_ODDS_MINOR_NPC);
        assert_eq!([of(0.0), of(0.249), of(0.25), of(0.499), of(0.5), of(0.99)], [0, 0, 1, 1, 2, 2]);
        // The first roll already has a military slot on the player's continent: kept.
        let mut stream = [0.1f32, 0.6, 0.6].into_iter();
        let kinds = roll_npc_specialties(&[true, false, false], || stream.next().unwrap());
        assert_eq!(kinds, vec![0, 2, 2]);
        // A military slot on another continent does not count: every slot is rolled
        // again (6 draws), the second roll gives the player's continent one.
        let mut stream = [0.1f32, 0.1, 0.6, 0.3, 0.9, 0.1].into_iter();
        let kinds = roll_npc_specialties(&[false, false, true], || stream.next().unwrap());
        assert_eq!(kinds, vec![1, 2, 0]);
        // Never satisfied: 30 rolls, the last one stays.
        let mut draws = 0usize;
        let kinds = roll_npc_specialties(&[true, true], || {
            draws += 1;
            0.9
        });
        assert_eq!((draws, kinds), (2 * CITY_TYPE_RETRIES as usize, vec![2, 2]));
        // The odds over many slots: 25 / 25 / 50 percent.
        let mut state = 12345u32;
        let mut next = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut counts = [0usize; 3];
        for _ in 0..4000 {
            counts[npc_city_type(next(), CITY_TYPE_ODDS_MINOR_NPC)] += 1;
        }
        assert!((900..1100).contains(&counts[0]), "{counts:?}");
        assert!((900..1100).contains(&counts[1]), "{counts:?}");
        assert!((1800..2200).contains(&counts[2]), "{counts:?}");
    }

    #[test]
    fn the_play_style_is_the_capture_method_used_most_ties_religious() {
        let c = |m, r, e| CaptureCounts { military: m, religious: r, economic: e };
        assert_eq!(play_style(c(3, 1, 1), [0; 3]), PlayStyle::Military);
        assert_eq!(play_style(c(1, 1, 3), [0; 3]), PlayStyle::Economic);
        assert_eq!(play_style(c(1, 3, 1), [0; 3]), PlayStyle::Religious);
        // Ties never favour military or economic.
        assert_eq!(play_style(c(2, 0, 2), [0; 3]), PlayStyle::Religious);
        assert_eq!(play_style(c(2, 2, 0), [0; 3]), PlayStyle::Religious);
        // Nothing captured: the cities held by specialty decide.
        assert_eq!(play_style(c(0, 0, 0), [1, 0, 0]), PlayStyle::Military);
        assert_eq!(play_style(c(0, 0, 0), [0, 0, 2]), PlayStyle::Economic);
        assert_eq!(play_style(c(0, 0, 0), [0, 0, 0]), PlayStyle::Religious);
        for s in [PlayStyle::Military, PlayStyle::Economic, PlayStyle::Religious] {
            assert_eq!(s.id(), fnv(s.consequence_trait()));
        }
        assert_eq!(tribal_card(0, 0), None);
        assert_eq!(tribal_card(2, 1), Some(TribalCard::Attack));
        assert_eq!(tribal_card(1, 2), Some(TribalCard::Social));
        assert_eq!(tribal_card(1, 1), Some(TribalCard::Mix));
        assert!(cinematics::ALL.contains(&"CVG_UnlockUFO"));
        assert!(cinematics::ALL.contains(&"CVG_UnlockAir"));
    }

    #[test]
    fn the_link_model_pays_factories_by_house_links_and_sets_the_mood() {
        use CityBuilding::*;
        // Hall 0; plots 1..: F(1) linked to hall and house 2; E(3) linked to
        // hall; house 2 linked to hall.
        let nodes = [Some(CityHall), Some(Factory), Some(House), Some(Entertainment), Some(Factory)];
        let links = [(0, 1), (1, 2), (0, 2), (0, 3), (3, 1)];
        let m = evaluate_city(&nodes, &links, false);
        // Factory 1: hall + house = 800; factory 4 is not connected.
        assert_eq!(m.money, 800);
        // Entertainment: 1 + hall link = 2; factory 1: 1 + entertainment link.
        assert_eq!((m.happy, m.unhappy), (2, 2));
        assert_eq!(m.happiness(), 50);
        // Black Cloud shuts the entertainment down.
        let off = evaluate_city(&nodes, &links, true);
        assert_eq!((off.happy, off.unhappy), (0, 1));
        assert_eq!(off.happiness(), 40);
        // Mood clamps to 0..100.
        assert_eq!(CityModel { money: 0, happy: 9, unhappy: 0 }.happiness(), 100);
        assert_eq!(CityModel { money: 0, happy: 0, unhappy: 9 }.happiness(), 0);
    }

    #[test]
    fn income_parties_riots_and_derricks_use_the_retail_formulas() {
        assert_eq!(city_income(800, true, false, 60), 1600);
        assert_eq!(city_income(800, true, false, 50), 800);
        assert_eq!(city_income(801, false, true, 40), 400);
        assert_eq!(city_income(800, false, true, 50), 800);
        assert_eq!(MONEY_UPDATE_SECONDS, 6.0);
        assert_eq!(INCOME_FACTOR, 0.1);
        assert_eq!(party_threshold(60), 10);
        assert_eq!(party_threshold(100), 60);
        assert!(party_starts(60, 9) && !party_starts(60, 10));
        assert_eq!(riot_threshold(30), 20);
        assert!(riot_starts(30, 19) && !riot_starts(50, 0));
        // A fresh derrick pays 1000 (100 after the 0.1 factor) and fades.
        assert_eq!(derrick_income(100.0), 1000.0);
        assert_eq!(derrick_income(99.7), 1000.0);
        assert_eq!(derrick_income(55.0), 600.0);
        assert_eq!(derrick_income(0.0), 100.0);
        assert!((derrick_points_after(100.0) - 99.7).abs() < 1e-5);
        assert_eq!(derrick_points_after(0.1), 0.0);
    }

    #[test]
    fn npc_income_scaling_and_development_follow_the_difficulty() {
        use Difficulty::*;
        assert_eq!(npc_income_multiplier(Medium, true, false), 1.0);
        assert_eq!(npc_income_multiplier(Hard, true, false), 1.2);
        assert_eq!(npc_income_multiplier(Easy, true, false), 0.4);
        assert_eq!(npc_income_multiplier(Medium, false, false), 0.4);
        assert_eq!(npc_income_multiplier(Medium, true, true), 0.4);
        assert_eq!(npc_income_multiplier(Medium, false, true), 0.2);
        let s = |cities, derricks| DevelopmentStats { cities, derricks, ..Default::default() };
        assert!(!has_developed(Medium, s(4, 2)));
        assert!(has_developed(Medium, s(5, 0)));
        assert!(has_developed(Medium, s(1, 3)));
        assert!(!has_developed(Easy, s(8, 4)));
        assert!(has_developed(Hard, s(1, 2)));
        assert!(free_vehicle_eligible(Easy, 0, 500.0, 0.0));
        assert!(!free_vehicle_eligible(Medium, 0, 500.0, 0.0));
        assert!(!free_vehicle_eligible(Easy, 1, 500.0, 0.0));
    }

    #[test]
    fn building_vehicle_and_refund_prices_are_the_data() {
        assert_eq!((HOUSE_COST, FACTORY_COST, ENTERTAINMENT_COST, TURRET_COST), (1600.0, 1200.0, 800.0, 2000.0));
        for p in [VehiclePurpose::Military, VehiclePurpose::Cultural, VehiclePurpose::Economic] {
            assert_eq!(vehicle_cost(p, Locomotion::Land), 1000.0);
            assert_eq!(vehicle_cost(p, Locomotion::Amphibious), 1500.0);
            assert_eq!(vehicle_cost(p, Locomotion::Air), 3000.0);
        }
        assert_eq!(vehicle_cost(VehiclePurpose::Colony, Locomotion::Air), 500.0);
        assert_eq!(refund(1600.0, Difficulty::Easy), 1600.0);
        assert_eq!(refund(1600.0, Difficulty::Medium), 1200.0);
        assert_eq!(refund(1600.0, Difficulty::Hard), 800.0);
        assert_eq!(VEHICLE_WEAPON_CYCLE, 2.0);
    }

    #[test]
    fn the_vehicle_cap_counts_capital_cities_and_houses() {
        let city = |houses| CapCity { houses, minor: false };
        // Capital type: 3 + 2 per city + houses (hall included).
        assert_eq!(vehicle_cap(true, &[city(3)], true), 8);
        assert_eq!(vehicle_cap(false, &[city(3), city(1)], true), 8);
        assert_eq!(vehicle_cap(false, &[], true), 0);
        // NPC houses count up to 6 (3 in a +0x519 city).
        assert_eq!(vehicle_cap(true, &[city(9)], false), 3 + 2 + 6);
        assert_eq!(vehicle_cap(false, &[CapCity { houses: 9, minor: true }], false), 1 + 3);
        assert_eq!(vehicle_cap(true, &[city(40)], true), 36);
    }

    #[test]
    fn relationships_sum_clamp_decay_and_rate_limit_attacks() {
        let mut r = Relationship::default();
        r.apply(RelationKind::Gift);
        assert!((r.value() - 0.4).abs() < 1e-5);
        r.apply(RelationKind::Gift);
        assert!((r.value() - 0.45).abs() < 1e-5);
        // Decay: the gift fades by 0.1 per 60 s period.
        r.tick(60.0);
        assert!((r.value() - 0.35).abs() < 1e-5);
        // Hits (0xBCF116 ..): Hostility, -3 at once, -0.03 a further hit, floor -9. A
        // friendly nation (0.35 from gifts) is still Cautious after one hit.
        assert!(r.attacked());
        assert!((r.contribution(RelationKind::Hostility) + 3.0).abs() < 1e-5);
        assert!(!r.hostile(), "{}", r.value());
        let mut h = Relationship::default();
        assert!(h.attacked());
        assert_eq!(h.value(), -3.0);
        assert!(h.hostile());
        for _ in 0..10 {
            h.attacked();
        }
        assert!((h.contribution(RelationKind::Hostility) + 3.3).abs() < 1e-4);
        for _ in 0..1000 {
            h.attacked();
        }
        assert_eq!(h.contribution(RelationKind::Hostility), -9.0);
        // The threshold is the Annoyed boundary (-3): hostile at -3, peaceful above it.
        let mut edge = Relationship::default();
        edge.apply(RelationKind::Insult);
        assert_eq!(edge.value(), -1.0);
        assert!(!edge.hostile() && edge.peaceful());
        for _ in 0..4 {
            edge.apply(RelationKind::Insult);
        }
        assert_eq!(edge.contribution(RelationKind::Insult), -3.0);
        assert!(edge.hostile() && !edge.peaceful(), "an insulted nation is Annoyed at -3");
        // The trade row caps at 5, insults at -3.
        let mut t = Relationship::default();
        for _ in 0..100 {
            t.apply(RelationKind::Trade);
            t.apply(RelationKind::Insult);
        }
        assert_eq!(t.contribution(RelationKind::Trade), 5.0);
        assert_eq!(t.contribution(RelationKind::Insult), -3.0);
        assert!((t.value() - 2.0).abs() < 1e-5);
        // A Dervish lifts a declared war back to neutral.
        let mut w = Relationship::default();
        w.apply(RelationKind::DeclaredWar);
        assert!(!w.peaceful());
        w.apply(RelationKind::Dervish);
        assert!(w.peaceful());
    }

    #[test]
    fn buying_a_city_uses_the_retail_price_formula() {
        // 4 buildings (5200) + 2 live turrets (4000), not neutral.
        let value = city_value(5200.0, 4000.0, 0.0, false);
        assert!((value - 2.0 * (6240.0 + 2000.0 + 500.0)).abs() < 1e-3);
        let one = buy_price(value, 1, 0.0);
        assert!((one - 1.1 * value * 0.8 * 2.0 / 3.0).abs() < 1.0, "{one}");
        assert!((buy_price(value, 1, 9000.0) - (one - 4000.0)).abs() < 1.0);
        assert_eq!(buy_price(100.0, 1, 0.0), BUY_PRICE_MIN);
        assert_eq!(buy_price(1e6, 5, 0.0), BUY_PRICE_MAX);
        assert_eq!(city_value(0.0, 0.0, 0.0, true), 500.0);
        let (owner, trader) = trade_delivery(60.0, false);
        assert_eq!((owner, trader), (80.0, 40.0));
        assert_eq!(TIME_IN_ALMOST_CONVERT + TIME_IN_CONVERTED, 6.0);
    }

    #[test]
    fn npc_city_types_use_cumulative_thresholds() {
        assert_eq!(npc_city_type(0.2, CITY_TYPE_ODDS), 0);
        assert_eq!(npc_city_type(0.5, CITY_TYPE_ODDS), 1);
        assert_eq!(npc_city_type(0.9, CITY_TYPE_ODDS), 2);
        // Major NPCs are never economic; minor NPCs half the time.
        assert_eq!(npc_city_type(0.99, CITY_TYPE_ODDS_MAJOR_NPC), 1);
        assert_eq!(npc_city_type(0.6, CITY_TYPE_ODDS_MINOR_NPC), 2);
        assert_eq!(npc_city_type(0.3, CITY_TYPE_ODDS_MINOR_NPC), 1);
    }

    #[test]
    fn the_consequence_table_matches_the_records_tools_and_costs() {
        let ids = [
            "cvg_crg_attack",
            "cvg_trg_attack",
            "cvg_trg_attackwin",
            "cvg_clg_plant",
            "cvg_trg_social",
            "cvg_trg_socialwin",
            "cvg_crg_mix",
            "cvg_trg_mix",
            "cvg_trg_mixwin",
            "cvg_clg_mix",
            "cvg_clg_meat",
            "cvg_crg_social",
        ];
        for (power, name) in POWER_TABLE.iter().zip(ids) {
            assert_eq!(power.consequence, name);
            assert_eq!(power.consequence_id, fnv(name), "{name}");
        }
        // Costs by card stage (0xBF6DF0).
        assert_eq!(INVULNERABILITY.cost, 2000.0);
        assert_eq!(BRIBE_BOMB.cost, 4000.0);
        assert_eq!(AD_BLITZ.cost, 16000.0);
        assert_eq!(superweapon_cost(0), 12000.0);
        assert_eq!(superweapon_cost(3), 36000.0);
        // Tool values.
        assert_eq!((STATIC_BOMB.recharge, STATIC_BOMB.min_damage), (60.0, 15.0));
        assert_eq!(GADGET_BOMB.recharge, 300.0);
        assert_eq!(MIGHTY_BOMB.min_damage, 600.0);
        assert_eq!(BLACK_CLOUD.min_damage, 120.0);
        assert!(BLACK_CLOUD.damages_friendlies);
        assert!((STATIC_BOMB.radius() - 64.0 * CIV_WORLD_SCALE).abs() < 1e-4);
        assert_eq!(ICBM.description, "Destroys all cities");
    }

    #[test]
    fn capture_points_follow_0xbe9c70_and_decay_after_quiet() {
        // The owner's own fire counts nothing.
        assert_eq!(add_capture(10.0, Some(0), 2, 2, 500.0), (10.0, Some(0)));
        // A fallen building is worth 8 points, hall damage 0.1 per point.
        let (c, id) = add_capture(0.0, None, 2, 0, CAPTURE_BUILDING_VALUE);
        assert_eq!((c, id), (8.0, Some(0)));
        let (c, id) = add_capture(c, id, 2, 0, 920.0);
        assert!((c - 100.0).abs() < 1e-3 && id == Some(0));
        // A third nation wears the points down, then takes over.
        let (c, id) = add_capture(30.0, Some(0), 2, 3, 200.0);
        assert!((c - 10.0).abs() < 1e-3 && id == Some(0));
        let (c, id) = add_capture(c, id, 2, 3, 300.0);
        assert!((c - 20.0).abs() < 1e-3 && id == Some(3));
        assert_eq!(add_capture(95.0, Some(0), 2, 0, 1e5).0, CAPTURE_FULL);
        // Decay only while quiet; zero forgets the capturer.
        assert_eq!(decay_capture(5.0, Some(0), false), (5.0, Some(0)));
        let (c, _) = decay_capture(5.0, Some(0), true);
        assert!((c - 4.9).abs() < 1e-4);
        assert_eq!(decay_capture(0.05, Some(0), true), (0.0, None));
        assert_eq!((MILITARY_BUILDING_HEALTH, CIVILIAN_BUILDING_HEALTH, CITY_HALL_BUILDING_HEALTH), (600.0, 800.0, 3600.0));
    }
}
