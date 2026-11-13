//! Space stage: fly your UFO around your home star, survey worlds, terraform
//! the viable ones, found a growing empire, complete the homeworld's
//! tutorial missions and jump to other star systems on the galaxy map.
//! Preserved from 0.43: WASD/QE flight, F combat, H/G upgrades and the ship
//! editor. Controls: WASD fly (with momentum), Q/E down/up, V survey,
//! T cycles planetary tools, Y applies one near a surveyed planet, C colonizes
//! or expands within ecology capacity, M reports the current mission at the
//! homeworld, mouse wheel or Z/X zoom between the planet, solar-system and
//! galaxy views; on the galaxy map WASD picks a star and Enter jumps to it.
//!
//! The rules follow the original (`space_rules.rs`, notes S1-S3): a wallet of
//! 100,000 Sporebucks (`RunProgress::spice` is the wallet) separate from the
//! spice cargo, the retail start tools, the archetype from the earlier
//! consequence cards, Flight 101 -> Story 201 -> teaching missions, then
//! procedural missions from empires; 10 ranks from badge points; UFO health
//! 1000 x level x difficulty and energy 100 x level with no passive recharge
//! (weapons and jumps spend it, planets sell repair / recharge per point);
//! colonies produce spice into capped storage; relationships are the sum of
//! capped event rows with a 60 s decay.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::asset_catalog::{self, PartDefinition};
use crate::creation_assets;
use crate::game::*;
use crate::mechanical_assets;
use crate::space_galaxy::{self, BodyPlan, EmpireId, Galaxy, StarMaps, SystemPlan, HOME_STAR};
use crate::space_rules::{self as rules, tool, RelationshipEvent};
use crate::space_tools;
use crate::space_view::{SpaceView, SpaceViewRequest};
use crate::stages::{GameStage, StageOwned};
use crate::{OriginalModel, Originals};

/// Fallback flight bound when no star system is loaded.
const BOUNDS: f32 = 42.0;
/// Closest the ship may fly to a star's centre, beyond its visible radius.
const STAR_CLEARANCE: f32 = 4.0;
/// Closest the ship may fly to a planet's surface.
const SURFACE_CLEARANCE: f32 = 1.1;
/// Momentum: how quickly thrust reaches top speed, and how quickly a coasting
/// ship bleeds off speed (per second, exponential).
const SHIP_ACCELERATION: f32 = 2.4;
const SHIP_DRAG: f32 = 1.4;
/// Ship altitude band above and below the ecliptic.
const ALTITUDE_MIN: f32 = -6.0;
const ALTITUDE_MAX: f32 = 22.0;
const COLONIES_FOR_INTRO_MISSION: u32 = 5;

/// The galaxy generation a campaign's ids refer to (see `SpaceCampaign::
/// migrate_galaxy`).
pub const GALAXY_VERSION: u8 = 1;

/// Number of worlds the stage spawns. Saves from 0.42 carried only a colony
/// count; they assumed worlds were assigned in WorldId order, and replaying
/// that mapping once keeps those saves playable.
const WORLD_COUNT: u32 = 8;

/// Fallback maximum energy for a ship without the energy model loaded:
/// `space_rules::UFO_ENERGY_BASE` (100 x energy level 1).
const MAX_ENERGY: f32 = rules::UFO_ENERGY_BASE;

// ---------------------------------------------------------------------------
// Ship editor: original vehicle parts only
// ---------------------------------------------------------------------------

/// Catalog group the ship editor may offer. Only authored vehicle parts are
/// valid ship options; creature, building, tool and preview meshes never are.
const SHIP_PART_GROUP: u32 = asset_catalog::VEHICLE_PARTS;
/// Part ceiling, matching the creature creator's 48.
pub const MAX_SHIP_MODULES: usize = 48;
/// Ship-local size multiplier applied to every mounted module (the UFO is a
/// ~1.8 unit box, and the slots sit within ~1.4 units of it).
pub(crate) const MODULE_BASE_FRACTION: f32 = 0.35;
/// Largest world size a module may reach at the default player scale, so a
/// huge authored part cannot swallow the ship.
pub(crate) const SHIP_MODULE_WORLD_CAP: f32 = 1.5;
// Energy and health are bought back at the retail per-point prices
// (`space_rules::RECHARGE_COST` / `REPAIR_COST`, homeworld free) through the
// comm screen's Repair / Recharge tabs (`SpaceCampaign::buy_repair_recharge`).

/// ADAPTED: the wallet has no retail cap in the notes; money is an int32 in
/// the original, so OpenSpore stops at 2 billion Sporebucks.
const SPICE_CAP: f32 = 2_000_000_000.0;

/// Legacy terraform helper (`try_terraform`, kept for old fixtures): an
/// unsuitable world costs more but is never locked. ADAPTED values: the old
/// prototype costs x `space_rules::LEGACY_SPICE_TO_SPOREBUCKS`.
const TERRAFORM_COST: f32 = 100.0 * rules::LEGACY_SPICE_TO_SPOREBUCKS;
/// Extra Sporebucks per suitability point below the threshold.
const TERRAFORM_PENALTY_PER_POINT: f32 = 5.0 * rules::LEGACY_SPICE_TO_SPOREBUCKS;
/// Upper bound on the surcharge, so terraforming is always affordable.
const TERRAFORM_MAX_COST: f32 = 220.0 * rules::LEGACY_SPICE_TO_SPOREBUCKS;
/// Deterministic "natural" suitability threshold, derived from the WorldId.
const TERRAFORM_MIN: f32 = 70.0;
/// ADAPTED: OpenSpore's food-web seeding tools have no retail price (retail
/// seeds with the cargo animals and plants the player carries); the old step
/// cost is converted to Sporebucks. The climate has no step cost any more: the
/// rays are items with no cost per use and the marble moves the climate.
const LIFE_SEED_COST: f32 = 3.0 * rules::LEGACY_SPICE_TO_SPOREBUCKS;
const MAX_ECOLOGY_TIER: u8 = 3;
const CLIMATE_MAX: f32 = 1.0;
const FLORA_NICHES_PER_TIER: u8 = 3;
const HERBIVORES_PER_TIER: u8 = 2;
const CARNIVORES_PER_TIER: u8 = 1;
/// Happy Ray: the retail `happyraylevel1` upgrade (store 200,000), bought on
/// first use and kept.
const HAPPY_RAY_COST: f32 = 200_000.0;
/// Monolith (retail store `toolmonolith`, consumed per use).
pub const MONOLITH_COST: f32 = 500_000.0;
/// Planet Buster (retail store `toolplanetbuster`, consumed per use).
pub const PLANET_BUSTER_COST: f32 = 5_000_000.0;
/// Atmosphere Freeze = the retail Ice Storm (`tooliceStorm`, consumed per use).
pub const ATMOSPHERE_FREEZE_COST: f32 = 150_000.0;
/// Heat Ray (retail `toolheatray` upgrade), bought on first use and kept.
pub const HEAT_RAY_COST: f32 = 450_000.0;
/// Asteroid Strike (retail `toolasteroid`, consumed per use).
pub const ASTEROID_CALLER_COST: f32 = 225_000.0;
/// Creature Tweaker = the retail Creature Editor tool (`toolcreatureedit`).
pub const CREATURE_TWEAKER_COST: f32 = 500_000.0;
/// The smallest retail gift (`smallGiftAmount`; medium 50,000, large 100,000).
pub const DIPLOMACY_GIFT_COST: f32 = 10_000.0;
/// The smallest retail peace offer (`peaceOfferPrices` 40,000..80,000).
pub const DIPLOMACY_PEACE_COST: f32 = 40_000.0;
/// Retail alliances cost nothing: the empire offers one at relationship 7.
pub const ALLIANCE_TREATY_COST: f32 = 0.0;
/// Purchase System at the cheapest system tier (`tradeCaptureCostT0`; T1..T3
/// 400,000 / 1.2M / 5M, `space_rules::system_purchase_cost`).
pub const EMPIRE_BUYOUT_COST: f32 = 100_000.0;
/// Defense of an alien colony (`spaceColonyObjectHealth`); it surrenders at
/// `spaceWarSurrenderThreshold` (35% left).
pub const EMPIRE_COLONY_MAX_HP: f32 = rules::COLONY_OBJECT_HEALTH;
/// Cargo slots of a new UFO (`space_rules::CARGO_SLOTS`); each slot stacks up
/// to `space_rules::MAX_CARGO_STACK` (99) of one item.
pub const MAX_CARGO_CRATES: u8 = rules::CARGO_SLOTS;

/// Six retail Spore spice varieties, from common Red to ultra-rare Purple.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum SpiceColor {
    #[default]
    Red,
    Yellow,
    Blue,
    Green,
    Pink,
    Purple,
}

impl SpiceColor {
    pub const ALL: [Self; 6] = [
        Self::Red,
        Self::Yellow,
        Self::Blue,
        Self::Green,
        Self::Pink,
        Self::Purple,
    ];

    /// Deterministic spice color for a given `WorldId`. The homeworld (`0`)
    /// always produces Red spice, matching retail Spore.
    pub fn for_world(world_id: u32) -> Self {
        if world_id == 0 {
            return Self::Red;
        }
        // The generated planet (S4C `AssignPlanetSpice`): the star colour and
        // the planet's orbit decide the colour.
        if let Some(info) = space_galaxy::world_info(world_id) {
            return match info.spice {
                space_galaxy::Spice::Red => Self::Red,
                space_galaxy::Spice::Yellow => Self::Yellow,
                space_galaxy::Spice::Blue => Self::Blue,
                space_galaxy::Spice::Green => Self::Green,
                space_galaxy::Spice::Pink => Self::Pink,
                space_galaxy::Spice::Purple => Self::Purple,
            };
        }
        match world_id.wrapping_mul(2_654_435_761) % 12 {
            0..=2 => Self::Red,
            3..=4 => Self::Yellow,
            5..=6 => Self::Blue,
            7..=8 => Self::Green,
            9..=10 => Self::Pink,
            _ => Self::Purple,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
        }
    }

    pub fn rgb(self) -> [f32; 3] {
        match self {
            Self::Red => [0.94, 0.28, 0.24],
            Self::Yellow => [0.96, 0.84, 0.22],
            Self::Blue => [0.28, 0.66, 0.98],
            Self::Green => [0.30, 0.90, 0.46],
            Self::Pink => [0.96, 0.44, 0.80],
            Self::Purple => [0.70, 0.36, 0.96],
        }
    }

    pub fn color(self) -> Color {
        let [r, g, b] = self.rgb();
        Color::srgb(r, g, b)
    }

    /// Retail base price per unit (`spice1..6`, Sporebucks).
    pub fn base_value(self) -> f32 {
        rules::SPICE_BASE_PRICE[self as usize]
    }

    /// Price per unit an NPC empire pays (`space_rules::spice_sell_price`:
    /// base x `difficultySpaceSpiceCost`).
    pub fn sell_price(self, difficulty: rules::Difficulty) -> f32 {
        rules::spice_sell_price(self.base_value(), difficulty)
    }

    /// Selling price per unit at `buyer_world_id`: empires buy at the retail
    /// price (normal difficulty); the player's own colonies do not buy spice.
    /// The old local/foreign arbitrage premium was invented and is gone.
    pub fn trade_price_at(self, _buyer_world_id: u32, is_empire_buyer: bool) -> f32 {
        if is_empire_buyer {
            self.sell_price(rules::Difficulty::Normal)
        } else {
            0.0
        }
    }
}

/// The retail tutorial chain (`space_rules::TutorialStep`): Flight 101's four
/// sub-missions, Story 201, then the "expand" teaching missions. Completing it
/// never ends Space; procedural missions from empires follow.
pub const MISSION_ORDER: &[&str] = &[
    "Flight 101: Visit your cities",
    "Flight 101: Scan",
    "Flight 101: Fetch an animal",
    "Flight 101: Eradicate",
    "Story 201: The closest star",
    "Colonize a planet",
    "Terraform a planet",
    "Trade spice",
];

/// Distinct owned worlds docked at (trade-route bookkeeping only now).
const TRADE_TARGET: u32 = 3;

/// Sporebucks a tutorial step pays, by its index in `MISSION_ORDER`
/// (`missionGenericRewardMoney`: Flight 101 and Story 201 pay none, the
/// colonize / terraform teaching missions 10,000, trade spice 5,000).
pub fn mission_reward(index: usize) -> f32 {
    rules::TutorialStep::ORDER
        .get(index)
        .map(|s| s.reward_money())
        .unwrap_or(0.0)
}

/// Retail scanning pays nothing (the scan tool reveals a planet); the old
/// survey bounty was the prototype's opening economy.
pub fn survey_bounty(_radius: f32) -> f32 {
    0.0
}

/// Spice to terraform a world. Unsuitable worlds cost more, never impossible.
pub fn terraform_cost(id: u32) -> f32 {
    let deficit = (TERRAFORM_MIN - terraform_suitability(id)).max(0.0);
    (TERRAFORM_COST + deficit * TERRAFORM_PENALTY_PER_POINT).min(TERRAFORM_MAX_COST)
}

/// Deterministic terraform suitability of a world, from its stable `WorldId`
/// only — never from its (shuffled) display name. Returns a percentage.
pub fn terraform_suitability(id: u32) -> f32 {
    // xorshift32-style scramble: stable across runs, platforms and saves.
    let mut x = id.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0xC2B2_AE3D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x27D4_EB2F);
    x ^= x >> 16;
    50.0 + (x % 46) as f32
}

/// Player-facing result of a world action.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldAction {
    /// The action happened; the string is the feedback line.
    Done(String),
    /// The action was refused; the string explains why (never silent).
    Refused(String),
}

/// Selected planetary tool. Climate and food-web actions are intentionally
/// separate so a single key press cannot instantly terraform a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlanetaryTool {
    #[default]
    Warm,
    Cool,
    ThickenAtmosphere,
    ThinAtmosphere,
    SeedFlora,
    SeedHerbivore,
    SeedCarnivore,
    HappyRay,
    StaffOfLife,
    Monolith,
    PlanetBuster,
    AtmosphereFreeze,
    HeatRay,
    AsteroidCaller,
    CreatureTweaker,
    // The rest of the retail terraforming tools (`spacetools~` type
    // terraforming; `space_tools::record` holds their numbers): two more rays
    // and the throwables whose hit adds an impulse to the planet's marble.
    ColdCloudRay,
    HotCloudVacuum,
    Volcano,
    IceComet,
    IceCapSeeder,
    RainOfFire,
    DryIce,
    AtmosphereGenerator,
    DroughtGenerator,
}

impl PlanetaryTool {
    /// How many planetary tools there are (the length of [`PlanetaryTool::ALL`]).
    pub const COUNT: usize = 24;

    pub const ALL: [Self; 24] = [
        Self::Warm,
        Self::Cool,
        Self::ThickenAtmosphere,
        Self::ThinAtmosphere,
        Self::SeedFlora,
        Self::SeedHerbivore,
        Self::SeedCarnivore,
        Self::HappyRay,
        Self::StaffOfLife,
        Self::Monolith,
        Self::PlanetBuster,
        Self::AtmosphereFreeze,
        Self::HeatRay,
        Self::AsteroidCaller,
        Self::CreatureTweaker,
        Self::ColdCloudRay,
        Self::HotCloudVacuum,
        Self::Volcano,
        Self::IceComet,
        Self::IceCapSeeder,
        Self::RainOfFire,
        Self::DryIce,
        Self::AtmosphereGenerator,
        Self::DroughtGenerator,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Warm => "Warm",
            Self::Cool => "Cool",
            Self::ThickenAtmosphere => "Add atmosphere",
            Self::ThinAtmosphere => "Thin atmosphere",
            Self::SeedFlora => "Seed flora",
            Self::SeedHerbivore => "Seed herbivores",
            Self::SeedCarnivore => "Seed carnivores",
            Self::HappyRay => "Happy Ray",
            Self::StaffOfLife => "Staff of Life",
            Self::Monolith => "Monolith",
            Self::PlanetBuster => "Planet Buster",
            Self::AtmosphereFreeze => "Atmosphere Freeze",
            Self::HeatRay => "Heat Ray",
            Self::AsteroidCaller => "Asteroid Strike",
            Self::CreatureTweaker => "Creature Tweaker",
            Self::ColdCloudRay => "Cold Cloud Ray",
            Self::HotCloudVacuum => "Hot Cloud Vacuum",
            Self::Volcano => "Volcano",
            Self::IceComet => "Ice Comet",
            Self::IceCapSeeder => "Ice Cap Seeder",
            Self::RainOfFire => "Rain of Fire",
            Self::DryIce => "Dry Ice",
            Self::AtmosphereGenerator => "Atmosphere Generator",
            Self::DroughtGenerator => "Drought Generator",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Position in [`PlanetaryTool::ALL`] (the index of the tool's clock).
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0)
    }

    /// The terraforming and social rays hold fire: `AutoFireRate` 0.1 s
    /// (`heatray`, `coldray`, `cloudray`, `cloudvacuum`, `hotcloudray`,
    /// `coldcloudvacuum`, `happyraylevelN`; the cargo, the tools that throw
    /// something and the consumables have `AutoFireRate` -1: one use per press).
    pub fn is_ray(self) -> bool {
        matches!(
            self,
            Self::Warm
                | Self::Cool
                | Self::ThickenAtmosphere
                | Self::ThinAtmosphere
                | Self::HappyRay
                | Self::AtmosphereFreeze
                | Self::HeatRay
                | Self::ColdCloudRay
                | Self::HotCloudVacuum
        )
    }

    /// The throwing terraforming tools: a use is one `OnHit` (`DefaultProjectile`
    /// / `DeepSpaceProjectile`, ammo from the stores) that adds the tool's
    /// atmosphere / temperature velocities to the planet's marble.
    pub fn is_throwable(self) -> bool {
        matches!(
            self,
            Self::Volcano
                | Self::IceComet
                | Self::IceCapSeeder
                | Self::RainOfFire
                | Self::DryIce
                | Self::AtmosphereGenerator
                | Self::DroughtGenerator
        )
    }
}

/// Ten canonical Space stage badge categories from retail Spore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SpaceBadge {
    Explorer,
    Colonist,
    Terraformer,
    Warmonger,
    Trader,
    Diplomat,
    Sightseer,
    Collector,
    BodyGuard,
    Cleanser,
}

impl SpaceBadge {
    pub const ALL: [Self; 10] = [
        Self::Explorer,
        Self::Colonist,
        Self::Terraformer,
        Self::Warmonger,
        Self::Trader,
        Self::Diplomat,
        Self::Sightseer,
        Self::Collector,
        Self::BodyGuard,
        Self::Cleanser,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::Colonist => "Colonist",
            Self::Terraformer => "Terraformer",
            Self::Warmonger => "Warmonger",
            Self::Trader => "Trader",
            Self::Diplomat => "Diplomat",
            Self::Sightseer => "Sightseer",
            Self::Collector => "Collector",
            Self::BodyGuard => "Body Guard",
            Self::Cleanser => "Cleanser",
        }
    }
}

/// Ten canonical Space stage archetypes from retail Spore.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum SpaceArchetype {
    #[default]
    Warrior,
    Zealot,
    Scientist,
    Ecologist,
    Trader,
    Bard,
    Diplomat,
    Knight,
    Shaman,
    Wanderer,
}

impl SpaceArchetype {
    pub const ALL: [Self; 10] = [
        Self::Warrior,
        Self::Zealot,
        Self::Scientist,
        Self::Ecologist,
        Self::Trader,
        Self::Bard,
        Self::Diplomat,
        Self::Knight,
        Self::Shaman,
        Self::Wanderer,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Warrior => "Warrior",
            Self::Zealot => "Zealot",
            Self::Scientist => "Scientist",
            Self::Ecologist => "Ecologist",
            Self::Trader => "Trader",
            Self::Bard => "Bard",
            Self::Diplomat => "Diplomat",
            Self::Knight => "Knight",
            Self::Shaman => "Shaman",
            Self::Wanderer => "Wanderer",
        }
    }

    /// The archetype's superpower (retail English names: `space_rules::
    /// archetype_superpower`); the Wanderer has none.
    pub fn superpower_name(self) -> &'static str {
        rules::archetype_superpower(self).map_or("None", |t| t.name)
    }

    pub fn superpower_description(self) -> &'static str {
        // ADAPTED: OpenSpore's effects of the retail superpowers (the retail
        // effects themselves are not traced beyond their tools and recharge).
        match self {
            Self::Warrior => "Summons a pirate raider fleet to cause havoc in the system",
            Self::Zealot => "Fanatically converts every planet of an alien empire's star system to your empire",
            Self::Scientist => "Demolishes all planetary defense structures with a gravitational wave",
            Self::Ecologist => "Beams up 2 of every flora and animal specimen into cargo hold",
            Self::Trader => "Instantly completes the maturity of an existing trade route for immediate system buyout",
            Self::Bard => "Soothes every empire for 2 minutes: no attitude falls below neutral, so no war is declared",
            Self::Diplomat => "Disables and freezes all hostile ships and defenses with static cling",
            Self::Knight => "Calls a Mini-U wingman fighter of your own empire to fight at your side until the tool is ready again",
            Self::Shaman => "Instantly warps your UFO back to your home star system",
            Self::Wanderer => "Wanderers have no archetype superpower",
        }
    }

    /// Recharge of the archetype's superpower tool (`RechargeRate`: 60 s
    /// portal .. 2,400 s planet conversion); 0 for the Wanderer.
    pub fn cooldown_secs(self) -> f32 {
        rules::archetype_superpower(self).map_or(0.0, |t| t.recharge_seconds)
    }

    /// The archetype from the earlier stages' consequence cards (the retail
    /// table 0x13F3DF0; see `cards_from_run`). Wanderer without cards.
    pub fn from_run_progress(run: &RunProgress) -> Self {
        rules::archetype_from_cards(cards_from_run(run))
    }
}

/// Read the four consequence cards from the earlier stages' saved state.
/// Cell: the retail diet class of the Cell save (`cell_flow::diet_class`,
/// 0xE5DA10) when the Cell stage was played, else the run's diet totals.
/// Creature: more species made extinct than allied -> attack (red), more
/// allied -> social (green), a tie -> mix (blue). Tribe: totem carvings
/// conquered vs allied the same way. Civ: the capital's doctrine.
/// ADAPTED: OpenSpore's Creature / Tribal / Civ stages do not store the
/// retail end-of-stage card; these are the inputs its Civ stage already uses
/// for the consequence powers. A stage with no record gives no card.
pub fn cards_from_run(run: &RunProgress) -> rules::StageCards {
    use rules::CardColor;
    let c = &run.cell;
    let cell_played = c.plant_food_progression != 0
        || c.food_progression != 0
        || c.over_plant_food_progression != 0
        || c.over_animal_food_progression != 0;
    let cell = if cell_played {
        Some(match crate::cell_flow::diet_class(run) {
            "clg_meat" => CardColor::Red,
            "clg_plant" => CardColor::Green,
            _ => CardColor::Blue,
        })
    } else if run.diet_carnivore + run.diet_herbivore > 0.0 {
        Some(if run.diet_carnivore > run.diet_herbivore * 1.25 {
            CardColor::Red
        } else if run.diet_herbivore > run.diet_carnivore * 1.25 {
            CardColor::Green
        } else {
            CardColor::Blue
        })
    } else {
        None
    };
    let by_counts = |red: usize, green: usize| -> Option<CardColor> {
        if red + green == 0 {
            None
        } else if red > green {
            Some(CardColor::Red)
        } else if green > red {
            Some(CardColor::Green)
        } else {
            Some(CardColor::Blue)
        }
    };
    let world = &run.species_world;
    let allied = world.species.iter().filter(|s| world.species_allied(s.id)).count();
    let extinct = world.species.iter().filter(|s| world.species_extinct(s.id)).count();
    let creature = by_counts(extinct, allied);
    let carvings = &run.tribal_campaign.totem_carvings;
    let conquered = carvings
        .iter()
        .filter(|t| **t == crate::tribal::TotemSegment::Conquered)
        .count();
    let befriended = carvings
        .iter()
        .filter(|t| **t == crate::tribal::TotemSegment::Allied)
        .count();
    let tribe = by_counts(conquered, befriended);
    // The Civ card is the play style earned at Advance (`civ_rules`); before
    // it was recorded (older saves) the capital doctrine stands in.
    let civ = match run.civ_campaign.play_style {
        Some(crate::civ_rules::PlayStyle::Military) => Some(CardColor::Red),
        Some(crate::civ_rules::PlayStyle::Economic) => Some(CardColor::Blue),
        Some(crate::civ_rules::PlayStyle::Religious) => Some(CardColor::Green),
        None => run.civ_campaign.started.then(|| match run.civ_campaign.approach {
            crate::civ::Approach::Military => CardColor::Red,
            crate::civ::Approach::Economic => CardColor::Blue,
            crate::civ::Approach::Religious => CardColor::Green,
        }),
    };
    rules::StageCards { cell, creature, tribe, civ }
}

/// Resolve an alien empire's canonical space philosophy / archetype.
pub fn empire_archetype(emp_idx: EmpireId) -> SpaceArchetype {
    match emp_idx {
        0 => SpaceArchetype::Wanderer, // Grox
        1 => SpaceArchetype::Zealot,   // Zelem
        2 => SpaceArchetype::Warrior,  // Kroxid
        3 => SpaceArchetype::Scientist,// Avorni
        4 => SpaceArchetype::Diplomat, // Thelos
        5 => SpaceArchetype::Trader,   // Ubrik
        other => match other % 10 {
            0 => SpaceArchetype::Wanderer,
            1 => SpaceArchetype::Zealot,
            2 => SpaceArchetype::Warrior,
            3 => SpaceArchetype::Scientist,
            4 => SpaceArchetype::Diplomat,
            5 => SpaceArchetype::Trader,
            6 => SpaceArchetype::Bard,
            7 => SpaceArchetype::Ecologist,
            8 => SpaceArchetype::Knight,
            _ => SpaceArchetype::Shaman,
        },
    }
}

/// Authentic archetype philosophical greeting from retail Spore.
pub fn archetype_philosophical_greeting(archetype: SpaceArchetype, _empire_name: &str) -> &'static str {
    match archetype {
        SpaceArchetype::Zealot => "Praise Spode! Will you heed the sacred scriptures of the Almighty, or be cleansed by holy fire?",
        SpaceArchetype::Warrior => "Halt, weakling! We are supreme conquerors of the sector! Bow before our dread fleet or face orbital annihilation!",
        SpaceArchetype::Scientist => "Empirical scan initialized. State your cosmological hypothesis and present verified data for rigorous analysis.",
        SpaceArchetype::Trader => "Greetings, valued client! Every star system has its price, and every trade route flows with lucrative spice credits!",
        SpaceArchetype::Diplomat => "Warm greetings, honored ambassadors. We extend our hands in interstellar friendship, cultural exchange, and enduring peace.",
        SpaceArchetype::Ecologist => "Welcome, caretakers. We safeguard the sacred biospheres of living worlds. Walk gently upon the cosmic food web.",
        SpaceArchetype::Bard => "Tra-la-la, joyous star-wanderer! We sing the magnificent cosmic symphony and revel in interstellar festivities!",
        SpaceArchetype::Knight => "Hail, noble traveler! We stand as righteous guardians of honor, valor, and the defenseless. State your quest!",
        SpaceArchetype::Shaman => "The cosmic winds whisper of your arrival. Walk in spiritual harmony with the great galactic pulse.",
        SpaceArchetype::Wanderer => "CRITICAL ALERT: INFERIOR ORGANIC LIFEFORM DETECTED. BIOLOGICAL INFESTATION SCHEDULED FOR PURGING.",
    }
}

/// An allied or colonial escort ship accompanying the player UFO.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EscortShip {
    pub name: String,
    #[serde(default)]
    pub empire: Option<EmpireId>,
    pub hp: f32,
    pub max_hp: f32,
}

/// Live wild plant and animal specimens stored in the UFO Cargo Hold.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WildlifeCargo {
    #[serde(default)]
    pub flora: u8,
    #[serde(default)]
    pub herbivores: u8,
    #[serde(default)]
    pub carnivores: u8,
}

impl WildlifeCargo {
    pub fn total(self) -> u8 {
        self.flora + self.herbivores + self.carnivores
    }
}

/// Four canonical municipal building structures in retail Spore colony cities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CityBuilding {
    /// Residential housing supplying workforce and citizens (+morale when adjacent to entertainment, +productivity when adjacent to factory).
    House,
    /// Industrial factory processing spice (+1.0 base spice rate, -1 morale to adjacent buildings).
    Factory,
    /// Recreational entertainment arena/park (+1 morale per adjacent house).
    Entertainment,
    /// Surface-to-air defense laser turret protecting the colony from raiders (+4 laser damage, -0.08s firing cooldown).
    Turret,
}

impl CityBuilding {
    pub const ALL: [Self; 4] = [
        Self::House,
        Self::Factory,
        Self::Entertainment,
        Self::Turret,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::House => "House",
            Self::Factory => "Factory",
            Self::Entertainment => "Entertainment",
            Self::Turret => "Turret",
        }
    }

    /// Retail building prices (SpaceEconomy): production building 20,000,
    /// defensive building (turret) 10,000. ADAPTED: houses and entertainment
    /// have no tuning price; they cost a production building's.
    pub fn cost(self) -> f32 {
        match self {
            Self::House => rules::PRODUCTION_BUILDING_COST,
            Self::Factory => rules::PRODUCTION_BUILDING_COST,
            Self::Entertainment => rules::PRODUCTION_BUILDING_COST,
            Self::Turret => rules::DEFENSIVE_BUILDING_COST,
        }
    }

    pub fn refund(self) -> f32 {
        (self.cost() * 0.5).round()
    }
}

/// A colony city layout: building slots in a ring around City Hall. The number
/// of slots is the city's building limit, which retail sets by the planet's
/// T-score (`cTerraformingManager::GetMaxBuildings` 0xBBE760: one building at
/// T0, 6 / 12 / 20 at T1 / T2 / T3, [`rules::colony_max_buildings`]); the ring
/// itself (left / right neighbours) is the port's model of the building links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityLayout {
    /// Municipal building slots surrounding City Hall (at least the six of
    /// the first versions; saves keep their six).
    pub slots: Vec<Option<CityBuilding>>,
    /// The building limit the city was last fitted to ([`Self::fit_capacity`]);
    /// 0 for a layout saved before the limits existed.
    #[serde(default)]
    pub capacity: u8,
    /// Where the colony tool landed: the city's planet-local direction (the
    /// aim point of the colony pack, `space_planets::colony_landing`); `None`
    /// for a colony not founded in the planet view (a seeded land site then).
    #[serde(default)]
    pub site: Option<[f32; 3]>,
}

impl Default for CityLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl CityLayout {
    pub fn new() -> Self {
        Self { slots: vec![None; 6], capacity: 0, site: None }
    }

    /// Slots a building may be placed in: the fitted limit, or the six of a
    /// layout that was never fitted.
    pub fn capacity(&self) -> usize {
        if self.capacity == 0 {
            self.slots.len()
        } else {
            usize::from(self.capacity)
        }
    }

    /// Buildings standing in the city.
    pub fn building_count(&self) -> usize {
        self.slots.iter().filter(|b| b.is_some()).count()
    }

    /// Fit the city to its planet's building limit `max`
    /// ([`rules::colony_max_buildings`]): the ring grows to `max` slots; when
    /// the limit falls (the planet's T-score dropped) retail removes the excess
    /// buildings (0xBBE8B0 calls 0xFF1C50(city, max) for every city of a
    /// tech-5 planet holding more than `max`). A layout saved before the
    /// limits existed keeps its buildings until a real drop. Returns the
    /// buildings removed. ADAPTED: which buildings go (the highest slots; the
    /// 0xFF1C50 choice was not read) and no refund.
    pub fn fit_capacity(&mut self, max: usize) -> usize {
        let max = max.clamp(1, usize::from(u8::MAX));
        let had = self.capacity;
        self.capacity = max as u8;
        let mut removed = 0;
        if had != 0 && max < usize::from(had) {
            for slot in self.slots.iter_mut().skip(max) {
                removed += usize::from(slot.take().is_some());
            }
            self.slots.truncate(max);
        }
        if self.slots.len() < max {
            self.slots.resize(max, None);
        }
        removed
    }

    /// The neighbours of slot `i` (left, right): the slots are rings of six
    /// around City Hall, as in the first versions (a six-slot city is one
    /// ring), and a bigger city adds further rings of six, so growing the
    /// city never changes the synergies it already has. ADAPTED: retail has
    /// building links (`maabBuildingLinks`), not a ring.
    fn neighbours(&self, i: usize) -> (Option<CityBuilding>, Option<CityBuilding>) {
        let start = i / 6 * 6;
        let len = (self.slots.len() - start).min(6);
        let k = i - start;
        match len {
            0 | 1 => (None, None),
            2 => (self.slots[start + (k + 1) % 2], None),
            _ => (self.slots[start + (k + len - 1) % len], self.slots[start + (k + 1) % len]),
        }
    }

    /// Total factory count in this city.
    pub fn factories(&self) -> usize {
        self.slots.iter().filter(|b| **b == Some(CityBuilding::Factory)).count()
    }

    /// Total house count in this city.
    pub fn houses(&self) -> usize {
        self.slots.iter().filter(|b| **b == Some(CityBuilding::House)).count()
    }

    /// Total entertainment arenas in this city.
    pub fn entertainment(&self) -> usize {
        self.slots.iter().filter(|b| **b == Some(CityBuilding::Entertainment)).count()
    }

    /// Total defense turrets in this city.
    pub fn turrets(&self) -> usize {
        self.slots.iter().filter(|b| **b == Some(CityBuilding::Turret)).count()
    }

    /// Calculate spice production efficiency multiplier / bonus from this city.
    pub fn productivity(&self) -> f32 {
        let mut prod = 0.0f32;
        for (i, slot) in self.slots.iter().enumerate() {
            if *slot == Some(CityBuilding::Factory) {
                prod += 1.0;
                let (left, right) = self.neighbours(i);
                if left == Some(CityBuilding::House) {
                    prod += 1.0;
                }
                if right == Some(CityBuilding::House) {
                    prod += 1.0;
                }
            }
        }
        prod
    }

    /// Calculate citizen morale / happiness (-10..=10) from adjacency synergies.
    pub fn happiness(&self) -> i32 {
        let mut happy = 0i32;
        for (i, slot) in self.slots.iter().enumerate() {
            match *slot {
                Some(CityBuilding::Entertainment) => {
                    happy += 1; // civic base
                    let (left, right) = self.neighbours(i);
                    if left == Some(CityBuilding::House) {
                        happy += 2;
                    }
                    if right == Some(CityBuilding::House) {
                        happy += 2;
                    }
                    if left == Some(CityBuilding::Factory) {
                        happy -= 1;
                    }
                    if right == Some(CityBuilding::Factory) {
                        happy -= 1;
                    }
                }
                Some(CityBuilding::Factory) => {
                    let (left, right) = self.neighbours(i);
                    if left == Some(CityBuilding::House) {
                        happy -= 1;
                    }
                    if right == Some(CityBuilding::House) {
                        happy -= 1;
                    }
                }
                _ => {}
            }
        }
        happy.clamp(-10, 10)
    }

    pub fn summary(&self) -> String {
        format!(
            "Buildings: H:{} F:{} E:{} T:{} | Prod: +{:.1} | Morale: {}{}",
            self.houses(),
            self.factories(),
            self.entertainment(),
            self.turrets(),
            self.productivity(),
            if self.happiness() >= 0 { "+" } else { "" },
            self.happiness()
        )
    }
}

/// OpenSpore's earlier ten invented rare collections, kept so older saves
/// load (`vault_collections`, `completed_sets`, old `ArtifactItem`s). New
/// pickups are the retail rares of `space_rares`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ArtifactSet {
    ScrollsOfFaith,
    ScrollsOfScience,
    ScrollsOfOrder,
    ScrollsOfHarmony,
    ScrollsOfForce,
    GalacticFossils,
    RareGems,
    AncientStatues,
    MeteoriteShards,
    Storybooks,
}

impl ArtifactSet {
    pub const ALL: [Self; 10] = [
        Self::ScrollsOfFaith,
        Self::ScrollsOfScience,
        Self::ScrollsOfOrder,
        Self::ScrollsOfHarmony,
        Self::ScrollsOfForce,
        Self::GalacticFossils,
        Self::RareGems,
        Self::AncientStatues,
        Self::MeteoriteShards,
        Self::Storybooks,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::ScrollsOfFaith => "Scrolls of Faith",
            Self::ScrollsOfScience => "Scrolls of Science",
            Self::ScrollsOfOrder => "Scrolls of Order",
            Self::ScrollsOfHarmony => "Scrolls of Harmony",
            Self::ScrollsOfForce => "Scrolls of Force",
            Self::GalacticFossils => "Galactic Fossils",
            Self::RareGems => "Rare Gems",
            Self::AncientStatues => "Ancient Statues",
            Self::MeteoriteShards => "Meteorite Shards",
            Self::Storybooks => "Storybooks",
        }
    }

    pub fn roman(piece: u8) -> &'static str {
        match piece {
            1 => "I",
            2 => "II",
            3 => "III",
            4 => "IV",
            5 => "V",
            6 => "VI",
            7 => "VII",
            8 => "VIII",
            9 => "IX",
            10 => "X",
            _ => "?",
        }
    }
}

/// A rare galactic artifact or ancient relic recovered from a planet surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactItem {
    pub name: String,
    pub world_id: u32,
    pub value: u32,
    #[serde(default)]
    pub set: Option<ArtifactSet>,
    #[serde(default)]
    pub piece: Option<u8>,
    /// The retail rare item (`spacetrading~` name, `space_rares::RARES`).
    #[serde(default)]
    pub rare: Option<String>,
}

/// Category of an active galactic distress call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistressKind {
    PirateRaid,
    BiohazardOutbreak,
}

/// An active emergency transmission requiring interstellar intervention.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct DistressSignal {
    pub system_id: u32,
    pub world_id: u32,
    pub kind: DistressKind,
    pub time_remaining: f32,
}

/// Category of a procedural mission contract offered by an alien empire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlienMissionKind {
    /// Eliminate rogue pirate raiders threatening shipping lanes.
    PirateHunt,
    /// Abduct wild flora or animal specimens in UFO cargo for planetary archives.
    SpecimenCollection,
    /// Terraform a designated world to T1 or higher for colony expansion.
    TerraformWorld,
    /// The retail `fetch` mission class (02AE1A8E files `fetchartifact`,
    /// `fetchtribecitizen`, `fetchcivcitizen`, `fetchempirecitizen`): abduct the
    /// target (an artifact, or a citizen of a tribe / civilization / empire planet)
    /// from the target world with the tractor beam and bring it to the giver.
    Fetch,
    /// The retail `eradicate` class with `missionEradicateType` Animals
    /// (`eradicateanimals`, 0xC4F880 / 0xC4F290 / 0xC4F210): destroy
    /// `missionEradicateNumCreatures` of the target planet's creatures.
    Eradicate,
    /// The retail `balance` class (0xC4B8D0 / 0xC4B830): bring a creature-level
    /// planet to `missionNumOfRowsRequired` rows of the T-score.
    Balance,
    /// The retail `multidelivery` class (0xC57040 / 0xC57E90 / 0xC59CF0): collect
    /// the mission's items from planets and deliver them to a buyer planet.
    MultiDelivery,
    /// The retail multi-step `scanfetch` mission (`scanfetch_scan` 0xC5EA60, then
    /// `scanfetch_fetch`, a fetch of type 0x736C417F) on one planet.
    ScanFetch,
}

impl AlienMissionKind {
    pub const ALL: [Self; 8] = [
        Self::PirateHunt,
        Self::SpecimenCollection,
        Self::TerraformWorld,
        Self::Fetch,
        Self::Eradicate,
        Self::Balance,
        Self::MultiDelivery,
        Self::ScanFetch,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::PirateHunt => "Pirate Hunt",
            Self::SpecimenCollection => "Specimen Collection",
            Self::TerraformWorld => "Terraform World",
            Self::Fetch => "Fetch",
            Self::Eradicate => "Eradicate Animals",
            Self::Balance => "Balance",
            Self::MultiDelivery => "Multi Delivery",
            Self::ScanFetch => "Scan and Fetch",
        }
    }
}

/// What the remaining mission kinds keep besides `progress` / `target_world`
/// (all `#[serde(default)]`: an older save has none of it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MissionExtra {
    /// Fetch commodity: the commodity (the spice colour of the target world;
    /// the item of cargo type 4, 0xC52B90).
    #[serde(default)]
    pub commodity: Option<SpiceColor>,
    /// Multi-delivery: the planets that still hold one of the items.
    #[serde(default)]
    pub item_worlds: Vec<u32>,
    /// Multi-delivery: items in the hold, waiting for the buyer.
    #[serde(default)]
    pub carried: u32,
    /// Multi-delivery: the buyer's planet (`missionMultiDeliveryMaxNumBuyers`).
    #[serde(default)]
    pub buyer_world: Option<u32>,
    /// What the texts call the item (`~mission_item~` /
    /// `~mission_multi_delivery_item~`).
    #[serde(default)]
    pub item: String,
    /// Eradicate: the creatures were placed (the player entered the system).
    #[serde(default)]
    pub spawned: bool,
    /// Eradicate: the time limit that starts on the first entry
    /// (`missionTimerStartsOnFirstEntry`).
    #[serde(default)]
    pub duration: Option<f32>,
    /// Scan-fetch: 0 while the scan sub-mission runs, 1 for the fetch
    /// sub-mission (the conversation flags Step1 / Step2).
    #[serde(default)]
    pub stage: u8,
}

/// An active procedural mission contract accepted from an alien empire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlienMission {
    pub empire: EmpireId,
    pub kind: AlienMissionKind,
    pub reward_spice: f32,
    pub progress: u32,
    pub target_count: u32,
    pub target_world: Option<u32>,
    pub description: String,
    /// The retail mission row it stands for (reward, limit, relation).
    #[serde(default)]
    pub retail: Option<rules::MissionKind>,
    /// Seconds left before it fails (None = no limit).
    #[serde(default)]
    pub time_left: Option<f32>,
    /// The state of the remaining kinds (commodity, delivery items, creatures).
    #[serde(default)]
    pub extra: MissionExtra,
}

impl Default for AlienMission {
    fn default() -> Self {
        AlienMission {
            empire: 0,
            kind: AlienMissionKind::PirateHunt,
            reward_spice: 0.0,
            progress: 0,
            target_count: 0,
            target_world: None,
            description: String::new(),
            retail: None,
            time_left: None,
            extra: MissionExtra::default(),
        }
    }
}

impl AlienMission {
    /// Whether the player's hold has the item of a fetch mission (0xC52B90): a
    /// commodity needs one unit of its colour; the other kinds hold the item as
    /// soon as the mission's `progress` says it was abducted.
    fn holds_fetch_item(&self, campaign: &SpaceCampaign) -> bool {
        match (self.retail, self.extra.commodity) {
            (Some(rules::MissionKind::FetchCommodity), Some(color)) => campaign.cargo_spice.get(&color).copied().unwrap_or(0) > 0,
            _ => true,
        }
    }

    /// The target planet of a scan-fetch has been scanned (the scan
    /// sub-mission, 0xC5EA60) ADAPTED: the port's scan covers the whole world.
    pub fn scan_done(&self, campaign: &SpaceCampaign) -> bool {
        self.target_world.is_some_and(|w| campaign.surveyed.contains(&w) || campaign.colonies.contains(&w))
    }

    pub fn is_complete(&self, campaign: &SpaceCampaign) -> bool {
        match self.kind {
            AlienMissionKind::PirateHunt => self.progress >= self.target_count,
            AlienMissionKind::SpecimenCollection => {
                (campaign.cargo_wildlife.herbivores as u32 + campaign.cargo_wildlife.carnivores as u32) >= self.target_count
                    || campaign.cargo_wildlife.total() as u32 >= self.target_count
            }
            AlienMissionKind::TerraformWorld => {
                if let Some(target_wid) = self.target_world {
                    campaign.ecology_for(target_wid).t_score() >= self.target_count as u8
                } else {
                    false
                }
            }
            // The target was abducted (`try_tractor_beam`); the hold carries it home.
            AlienMissionKind::Fetch => self.progress >= self.target_count && self.holds_fetch_item(campaign),
            // The creatures were destroyed (0xC4F290).
            AlienMissionKind::Eradicate => self.progress >= self.target_count,
            // 0xC4B8D0: the planet's T-score level reached the required rows.
            AlienMissionKind::Balance => self
                .target_world
                .is_some_and(|w| campaign.ecology_for(w).stable_foodweb_tier() as u32 >= self.target_count),
            // Every item was delivered to the buyer; the giver pays.
            AlienMissionKind::MultiDelivery => self.progress >= self.target_count,
            // The scan sub-mission (0xC5EA60) is done; then the fetch sub-mission is
            // done when the hold has a specimen.
            AlienMissionKind::ScanFetch => {
                if self.extra.stage == 0 {
                    self.scan_done(campaign)
                } else {
                    campaign.cargo_wildlife.total() > 0
                }
            }
        }
    }

    pub fn current_progress(&self, campaign: &SpaceCampaign) -> u32 {
        match self.kind {
            AlienMissionKind::PirateHunt => self.progress,
            AlienMissionKind::SpecimenCollection => {
                (campaign.cargo_wildlife.herbivores as u32 + campaign.cargo_wildlife.carnivores as u32)
                    .max(campaign.cargo_wildlife.total() as u32)
                    .min(self.target_count)
            }
            AlienMissionKind::TerraformWorld => {
                if let Some(target_wid) = self.target_world {
                    (campaign.ecology_for(target_wid).t_score() as u32).min(self.target_count)
                } else {
                    0
                }
            }
            AlienMissionKind::Fetch => self.progress.min(self.target_count),
            AlienMissionKind::Eradicate | AlienMissionKind::MultiDelivery => self.progress.min(self.target_count),
            AlienMissionKind::Balance => self
                .target_world
                .map_or(0, |w| (campaign.ecology_for(w).stable_foodweb_tier() as u32).min(self.target_count)),
            AlienMissionKind::ScanFetch => {
                if self.extra.stage == 0 {
                    u32::from(self.scan_done(campaign))
                } else {
                    1 + u32::from(campaign.cargo_wildlife.total() > 0)
                }
            }
        }
    }
}

/// TutorialColonize (02AE1A8E!398ACA5E: `missionRangeMin` 0, `missionRangeMax`
/// 6, `missionPickClosestStar` 1) picks its target planet with the mission's
/// scoring 0xC4E3B0 (VERIFIED): the base 0xC44E60 (-1 for a destroyed planet,
/// one with a colony or a sanctuary; 1.0, or 0.1 when another mission targets
/// it), then -1 unless the star has creature-level life (tech 1), no empire,
/// and the planet itself has creature-level life; then the T-score check
/// (0xC4E440 via 0xBBE730: the T-score level 0xBBCFB0 > 0 against the mission's flag +0x1F4, the property
/// `missionColonizePlanetFullRow` 0x36758B07, read by 0xC4E4C0 and absent in
/// TutorialColonize and its template, so false): with the flag clear a
/// planet that already has a T-score scores -1, one without keeps its 1.0 /
/// 0.1. The closest star with a candidate wins; its best planet (the first of
/// equals) is the target. ADAPTED: the port's `GetTScore` is the food-web tier
/// ([`PlanetEcology::max_buildings`]); the destroyed / sanctuary tests have
/// no data here; moons are not worlds.
pub fn colonize_mission_target(campaign: &SpaceCampaign, galaxy: &space_galaxy::Galaxy) -> Option<u32> {
    const RANGE_MAX: f32 = 6.0;
    let here = campaign.current_system;
    let stars = std::iter::once((here, 0.0)).chain(galaxy.stars_within(here, RANGE_MAX));
    // 0xC44E60: a planet another mission already targets scores 0.1.
    let other_target = campaign.active_alien_mission.as_ref().and_then(|m| m.target_world);
    for (star, _) in stars {
        let Some(s) = galaxy.star(star) else {
            continue;
        };
        let owned = s.empire.is_some()
            || campaign.universe.grown.contains_key(&star)
            || star == space_galaxy::HOME_STAR
            || campaign.colonies.iter().any(|w| space_galaxy::system_of_world(*w) == star);
        if owned || s.tech != crate::space_worldgen::TechLevel::Creature {
            continue;
        }
        let record = galaxy.system(star);
        let mut best: Option<(u32, f32)> = None;
        for p in record.planets.iter() {
            let Some(slot) = p.world_slot else {
                continue;
            };
            let world = space_galaxy::world_id(star, slot as u32);
            if p.moon_of.is_some() || p.tech != crate::space_worldgen::TechLevel::Creature || campaign.colonies.contains(&world) {
                continue;
            }
            if campaign.ecology_for(world).stable_foodweb_tier() > 0 {
                continue;
            }
            let score = if other_target == Some(world) { 0.1 } else { 1.0 };
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((world, score));
            }
        }
        if let Some((world, _)) = best {
            return Some(world);
        }
    }
    None
}

/// The retail rare lying on an uncharted foreign world (`space_rares`: the
/// star's rare world, 0x103AA10 roll); worlds without galaxy data keep the
/// old hash rule for whether they carry one.
pub fn world_artifact(world_id: u32) -> Option<ArtifactItem> {
    if world_id < space_galaxy::FOREIGN_WORLD_BASE {
        return None;
    }
    let h = world_id.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    let has_relic = match space_galaxy::world_info(world_id) {
        Some(info) => info.rare,
        None => h % 5 == 2,
    };
    let rare = crate::space_rares::world_rare(world_id, has_relic)?;
    Some(ArtifactItem {
        name: rare.title.to_string(),
        world_id,
        // `spaceTradingBaseCost` (the sale price before the archetype
        // multipliers, which are not applied here).
        value: rare.cost as u32,
        set: None,
        piece: None,
        rare: Some(rare.name.to_string()),
    })
}

/// The runtime state of a world's terraform marble (`space_terraform`): the
/// planet object's two velocities and the facts the bullseye force reads once
/// per visit. Not saved.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarbleBody {
    /// `+0x1FD4`: atmosphere velocity.
    pub v_atmosphere: f32,
    /// `+0x1FD8`: temperature velocity.
    pub v_temperature: f32,
    pub zone: crate::space_terraform::Zone,
    /// A Grox world (the extra atmosphere divergence of 0xC7FB4F).
    pub grox: bool,
}

/// Persistent, adapted planet ecology. Climate axes use a continuous 0..=1
/// range; the installed Guide does not expose the full numeric simulation.
/// Food-web counts and inner T2/T3 bands are explicitly prototype rules.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanetEcology {
    pub world_id: u32,
    /// Continuous climate axes, each in 0.0..=1.0 with a centered optimum.
    pub temperature: f32,
    pub atmosphere: f32,
    pub flora: u8,
    pub herbivores: u8,
    pub carnivores: u8,
    /// Colony units on this world. Zero means uncolonized.
    #[serde(default)]
    pub colony_count: u8,
}

impl PlanetEcology {
    /// The retail T-score (`CalculateTScore` 0xFC2040): how close the
    /// temperature / atmosphere point lies to the bullseye centre.
    pub fn environment_tier(self) -> u8 {
        crate::space_worldgen::t_score(self.temperature, self.atmosphere)
    }

    pub fn foodweb_tier(self) -> u8 {
        (self.flora / FLORA_NICHES_PER_TIER)
            .min(self.herbivores / HERBIVORES_PER_TIER)
            .min(self.carnivores / CARNIVORES_PER_TIER)
            .min(MAX_ECOLOGY_TIER)
    }

    /// Climate T-score is the weaker temperature/atmosphere axis. Foodweb
    /// stabilization is reported separately and gates colony growth.
    pub fn t_score(self) -> u8 {
        self.environment_tier()
    }

    pub fn stable_foodweb_tier(self) -> u8 {
        self.foodweb_tier().min(self.t_score())
    }

    /// The building limit of one city on this world: retail
    /// `GetMaxBuildings` 0xBBE760 reads the planet's T-score (the level 0xBBCFB0:
    /// the bullseye level of `GetTScore` 0xBBC7E0, held back by the species
    /// count: 3 x level species of the planet's animal list, the plant list on a
    /// sanctuary); one building at T0, 6 / 12 / 20 at T1 / T2 / T3 (TerraformTuning).
    /// ADAPTED: the port's food-web tier ([`Self::stable_foodweb_tier`]: the
    /// bullseye tier held back by the stable food web) stands in for the
    /// species-count gate, whose lists the port keeps as three counters.
    pub fn max_buildings(self) -> usize {
        rules::colony_max_buildings(self.stable_foodweb_tier())
    }

    /// How many cities (settlement units) the world holds. ADAPTED: no retail
    /// count was found (the `GetMaxBuildings` limit above is per city and the
    /// colony tool's own gate was not read), so the port keeps its earlier
    /// rule: T0 supports an outpost; T1/T2/T3 support one/two/three units as
    /// the food web stabilizes.
    pub fn colony_capacity(self) -> u8 {
        if self.t_score() == 0 {
            1
        } else {
            self.t_score().min(self.stable_foodweb_tier().max(1))
        }
    }

    fn completed(world_id: u32) -> Self {
        Self {
            world_id,
            temperature: 0.5,
            atmosphere: 0.5,
            flora: FLORA_NICHES_PER_TIER * MAX_ECOLOGY_TIER,
            herbivores: HERBIVORES_PER_TIER * MAX_ECOLOGY_TIER,
            carnivores: CARNIVORES_PER_TIER * MAX_ECOLOGY_TIER,
            colony_count: 0,
        }
    }

    pub fn summary(self) -> String {
        format!(
            "T{} temp {:.2} ({}) atmo {:.2} ({}) food web {}/{}/{} (stable T{}) colonies {}/{}",
            self.t_score(),
            self.temperature,
            climate_axis_guidance(self.temperature, false),
            self.atmosphere,
            climate_axis_guidance(self.atmosphere, true),
            self.flora,
            self.herbivores,
            self.carnivores,
            self.stable_foodweb_tier(),
            self.colony_count,
            self.colony_capacity(),
        )
    }
}

fn climate_axis_guidance(value: f32, atmosphere: bool) -> &'static str {
    if value < 0.33 {
        if atmosphere {
            "add"
        } else {
            "warm"
        }
    } else if value > 0.66 {
        if atmosphere {
            "thin"
        } else {
            "cool"
        }
    } else {
        "in range"
    }
}

/// Persisted space campaign: world identities, missions, energy and economy.
/// Old saves (no field, or only a colony count) are reconciled into it once.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpaceCampaign {
    /// WorldIds the player has scanned.
    pub surveyed: Vec<u32>,
    /// WorldIds terraformed by the player.
    pub terraformed: Vec<u32>,
    /// Per-world climate and food-web development, keyed by stable WorldId.
    #[serde(default)]
    pub ecology: Vec<PlanetEcology>,
    /// Missing on pre-ecology saves; zero triggers one-time legacy migration.
    #[serde(default)]
    pub ecology_version: u8,
    /// Current tool cycled with T and applied with Y near a surveyed world.
    #[serde(default)]
    pub planetary_tool: PlanetaryTool,
    /// WorldIds the player owns, including the homeworld (WorldId 0).
    pub colonies: Vec<u32>,
    /// WorldIds the player has docked at (trade mission evidence).
    pub docked: Vec<u32>,
    /// Index into `MISSION_ORDER` of the mission being worked on.
    pub mission: usize,
    /// How many missions have already been claimed with M.
    pub claimed: usize,
    /// Pirate ships destroyed. Grants one bounty each.
    pub kills: u32,
    /// Travel energy: spent by distance, restored by docking or the trickle.
    /// `0.0` is a valid saved state (a ship run dry).
    pub energy: f32,
    /// True once the reactor has been filled. This is what tells a fresh
    /// campaign apart from one saved with zero energy.
    #[serde(default)]
    pub fuel_initialised: bool,
    /// Seconds accumulated toward the next colony income payout.
    pub income_timer: f32,
    /// Last feedback line, so the HUD can explain the current situation.
    pub message: String,
    /// Last ship-editor feedback line. Kept here rather than in `RunProgress`
    /// so `ship_editor_hud` can name the authored part just fitted (or explain
    /// a refused one) without a new shared progress field. Never gameplay.
    #[serde(default)]
    pub ship_message: String,
    /// Galaxy star index of the system the ship is in (0 is home). Saved so
    /// a reload rebuilds the same system its checkpoint was taken in.
    #[serde(default)]
    pub current_system: u32,
    /// The galaxy the star and world ids refer to: 0 (or absent) is the
    /// 2,600-star prototype galaxy, 1 the retail galaxy of `space_worldgen`.
    #[serde(default)]
    pub galaxy_version: u8,
    /// Empire index (`0..EMPIRES.len()`) controlling `current_system`, if any.
    #[serde(default)]
    pub current_system_empire: Option<EmpireId>,
    /// Star systems the ship has reached, for the galaxy map.
    #[serde(default)]
    pub visited: Vec<u32>,
    /// Stored spice crates waiting for pickup at each colonized WorldId.
    #[serde(default)]
    pub colony_spice: std::collections::BTreeMap<u32, u8>,
    /// Spice crates currently loaded into the player's UFO Cargo Hold.
    #[serde(default)]
    pub cargo_spice: std::collections::BTreeMap<SpiceColor, u8>,
    /// Diplomatic standing (`-100..=100`) with each alien empire (`0..6`).
    #[serde(default)]
    pub empire_relations: std::collections::BTreeMap<EmpireId, i32>,
    /// Active empire trade routes keyed by foreign WorldId, value `0.0..=1.0`
    /// representing buyout maturity progress.
    #[serde(default)]
    pub trade_routes: std::collections::BTreeMap<u32, f32>,
    /// Remaining defense hitpoints of damaged alien empire colonies.
    #[serde(default)]
    pub empire_colony_hp: std::collections::BTreeMap<u32, f32>,
    /// Alien empire worlds conquered or bought out by the player.
    #[serde(default)]
    pub conquered_worlds: Vec<u32>,
    /// Rare galactic relics stored in the ship's vault.
    #[serde(default)]
    pub artifacts: Vec<ArtifactItem>,
    /// WorldIds where the artifact has already been beamed up.
    #[serde(default)]
    pub collected_artifacts: Vec<u32>,
    /// The retail rares found, detector readings and ambushes (`space_rares`).
    #[serde(default)]
    pub rares: crate::space_rares::RareState,
    /// Remaining charges of the Staff of Life (granted at Galactic Core).
    #[serde(default)]
    pub staff_of_life_charges: u8,
    /// Active emergency radio distress call, if any.
    #[serde(default)]
    pub active_distress: Option<DistressSignal>,
    /// Timer pacing incoming distress transmissions.
    #[serde(default)]
    pub distress_timer: f32,
    /// Colony pirate raids successfully defended.
    #[serde(default)]
    pub pirate_raids_defended: u32,
    /// Raids on an ally's world repelled (`FriendsDefended`: the raid mission's
    /// `OnMissionCompleted` 0xC5BB60 counts a planet of the player or an ally).
    #[serde(default)]
    pub ally_raids_defended: u32,
    /// Biohazard eco-disaster outbreaks cleansed.
    #[serde(default)]
    pub biohazards_cleansed: u32,
    /// Space archetype determining player philosophy and superpower.
    #[serde(default)]
    pub archetype: SpaceArchetype,
    /// Seconds remaining before the archetype superpower can be activated again.
    #[serde(default)]
    pub superweapon_cooldown: f32,
    /// Active duration remaining of the Bard's soothing song (120 s).
    #[serde(default)]
    pub siren_song_active: f32,
    /// Active duration remaining for Diplomat's Static Cling EMP.
    #[serde(default)]
    pub static_cling_active: f32,
    /// Allied and colony escort wingmen currently in formation.
    #[serde(default)]
    pub fleet: Vec<EscortShip>,
    /// Live wild specimens held in the UFO Cargo Hold.
    #[serde(default)]
    pub cargo_wildlife: WildlifeCargo,
    /// Uplifted species colonies keyed by WorldId to their generated empire index.
    #[serde(default)]
    pub uplifted_empires: std::collections::BTreeMap<u32, EmpireId>,
    /// Number of primitive species uplifted into spacefaring civilizations via Monolith.
    #[serde(default)]
    pub species_uplifted: u32,
    /// Number of worlds obliterated via Planet Buster superweapon.
    #[serde(default)]
    pub planets_busted: u32,
    /// Formal military allied empire indices.
    #[serde(default)]
    pub allies: Vec<EmpireId>,
    /// Active procedural mission contract from an alien empire.
    #[serde(default)]
    pub active_alien_mission: Option<AlienMission>,
    /// Total alien empire missions completed.
    #[serde(default)]
    pub alien_missions_completed: u32,
    /// Whether the player allied with the Grox, triggering perpetual galactic war.
    #[serde(default)]
    pub danced_with_the_devil: bool,
    /// Whether the player discovered Earth in the Sol system ("Our Own Home" achievement).
    #[serde(default)]
    pub discovered_earth: bool,
    /// Whether the player destroyed Earth with the Planet Buster ("Oh the Humanity!" secret achievement).
    #[serde(default)]
    pub destroyed_earth: bool,
    /// City municipal building layouts keyed by WorldId, one CityLayout per colony on that world.
    #[serde(default)]
    pub city_layouts: std::collections::BTreeMap<u32, Vec<CityLayout>>,
    /// Active vault collections: pieces collected for each ArtifactSet (1..=10).
    #[serde(default)]
    pub vault_collections: std::collections::BTreeMap<ArtifactSet, std::collections::BTreeSet<u8>>,
    /// Completed artifact collections.
    #[serde(default)]
    pub completed_sets: Vec<ArtifactSet>,
    // ----------------------------------------------- retail rules (space-rules)
    /// 0 for a save from before the retail rules; `adopt_retail_rules` maps
    /// its old fields once and sets `ECONOMY_VERSION`.
    #[serde(default)]
    pub economy_version: u8,
    /// Retail tool ids the UFO owns (`space_rules::tool`): drives, upgrades,
    /// weapons and unlocked tools.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Charges of consumable tools (Colony Incredi-Pak `placecolony`, ...).
    #[serde(default)]
    pub tool_charges: std::collections::BTreeMap<String, u32>,
    /// The earlier stages' consequence cards, read on arrival.
    #[serde(default)]
    pub cards: rules::StageCards,
    /// Superpower flags of the cards (0x10056A0 bits).
    #[serde(default)]
    pub superpower_flags: u32,
    /// Game difficulty (0 easy, 1 normal, 2 hard), copied from the run.
    #[serde(default)]
    pub difficulty: i32,
    /// Flight 101 / Story 201 / teaching mission progress.
    #[serde(default)]
    pub tutorial: rules::TutorialState,
    /// Retail relationship (event accumulators) with each empire.
    #[serde(default)]
    pub relationships: std::collections::BTreeMap<EmpireId, rules::Relationship>,
    /// Empires at war with the player.
    #[serde(default)]
    pub wars: Vec<EmpireId>,
    /// Seconds toward the next relationship decay tick (60 s).
    #[serde(default)]
    pub relationship_clock: f32,
    /// Counters the retail badges read.
    #[serde(default)]
    pub badge_stats: BadgeStats,
    /// Fractional spice produced toward the next stored unit, per colony.
    #[serde(default)]
    pub colony_spice_progress: std::collections::BTreeMap<u32, f32>,
    /// A cinematic the presentation should play (retail name), taken once.
    #[serde(default)]
    pub pending_cinematic: Option<String>,
    /// Hull damage the rules owe the ship (an empty-reactor jump: 200).
    #[serde(default)]
    pub pending_damage: f32,
    /// Full health and energy on the next frame (galactic core, restore).
    #[serde(default)]
    pub pending_restore: bool,
    /// Times the UFO was destroyed (no game over: it is restored).
    #[serde(default)]
    pub ships_lost: u32,
    /// The galactic core's one-time reward was given (flag 0x0681C28E).
    #[serde(default)]
    pub reached_core: bool,
    /// An empire's open tribute demand.
    #[serde(default)]
    pub tribute_demand: Option<TributeDemand>,
    /// Seconds of Space play (the universe simulator's clock).
    #[serde(default)]
    pub universe_clock: f32,
    /// Universe time of the next pirate raid on a colony.
    #[serde(default)]
    pub next_pirate_raid: f32,
    /// Universe time of the next bio-disaster on a colony.
    #[serde(default)]
    pub next_bio_disaster: f32,
    /// Awareness of the player per empire (0..50).
    #[serde(default)]
    pub awareness: std::collections::BTreeMap<EmpireId, f32>,
    /// Star counts per empire (from the galaxy; rebuilt on entry, not saved).
    #[serde(skip)]
    pub empire_sizes: Vec<u32>,
    /// A repair / recharge bought on the comm screen, applied to the ship
    /// next frame at that owner's price (not saved).
    #[serde(skip)]
    pub repair_request: Option<rules::PlanetOwner>,
    /// The timers of the player's laser (`F` held; `space_fire`) and of every
    /// planetary tool (`Y` held; one per `PlanetaryTool::ALL` entry), the
    /// recharge / auto-fire clocks of `cSpaceToolData` (not saved).
    #[serde(skip)]
    pub fire_clock: rules::ToolFireClock,
    /// The combat tool F fires, chosen in the palette (a retail tool name of
    /// `rules::PLAYER_WEAPONS`); the best laser when `None` or not owned.
    #[serde(default)]
    pub weapon_tool: Option<String>,
    #[serde(skip)]
    pub tool_clocks: [rules::ToolFireClock; PlanetaryTool::COUNT],
    /// The terraform marbles of the worlds the ship has hovered over: the
    /// velocities of the planet object (`+0x1FD4` / `+0x1FD8`) and the orbit /
    /// owner facts the force reads (not saved: retail keeps them on the planet
    /// object, which is gone when the player leaves).
    #[serde(skip)]
    pub marbles: std::collections::BTreeMap<u32, MarbleBody>,
    /// A ship tool the palette asked to use (`toolUseOnActivate`: shield, cloak,
    /// potions; served by `space_tool_use::space_self_tools`), not saved.
    #[serde(skip)]
    pub pending_self_tool: Option<String>,
    /// Seconds of shield (`mbInvincible`, 60 s) / cloak (UFO flag 1, 90 s) left
    /// (VERIFIED 0x1009F20, see `space_tool_use`), not saved.
    #[serde(skip)]
    pub shield_left: f32,
    #[serde(skip)]
    pub cloak_left: f32,
    /// Seconds of the Rally Call's doubled damage left (the tool's recharge, 60 s,
    /// VERIFIED 0xBFCF10, see `space_tool_use::rally_multiplier`), not saved.
    #[serde(skip)]
    pub rally_left: f32,
    /// Posse voices the rules raised (rally, under attack, dead) for
    /// `space_voices` to play, not saved.
    #[serde(skip)]
    pub posse_events: Vec<crate::space_tool_use::PosseEvent>,
    /// The recharge clocks of the generic palette tools by retail name (not saved).
    #[serde(skip)]
    pub generic_clocks: std::collections::BTreeMap<String, rules::ToolFireClock>,
    /// The universe simulation (space-comm): contacts, transmissions, raids,
    /// empire growth.
    #[serde(default)]
    pub universe: crate::space_universe::UniverseState,
    /// The galactic core, badges and promotions (`space_endgame`).
    #[serde(default)]
    pub endgame: crate::space_endgame::EndgameState,
}

/// The retail rules' save version (`SpaceCampaign::economy_version`).
pub const ECONOMY_VERSION: u8 = 1;

/// Counters for the retail badge requirements (`badgeReq*`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BadgeStats {
    pub missions_complete: u32,
    pub fetch_missions: u32,
    pub eradicate_missions: u32,
    pub eco_disaster_missions: u32,
    pub alliances_formed: u32,
    pub wars_started: u32,
    /// Sporebucks of spice and rares sold.
    pub trade_volume: f64,
    pub empires_met: Vec<EmpireId>,
    pub archetype_switches: u32,
}

/// An NPC's tribute demand (`npcTributeAmount` x size, 300 s limit).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TributeDemand {
    pub empire: EmpireId,
    pub amount: f32,
    pub time_left: f32,
}

/// A campaign that has never been played: an empty hold and a ship with a
/// full reactor. `Default` also backs `#[serde(default)]`, so a save without
/// the field loads as this — and only `initialise_fuel` (from `setup_space`)
/// records that the reactor has been filled. A save that already carries
/// `fuel_initialised` keeps whatever it recorded, including `0.0`.
impl Default for SpaceCampaign {
    fn default() -> Self {
        Self {
            surveyed: Vec::new(),
            terraformed: Vec::new(),
            ecology: Vec::new(),
            ecology_version: 1,
            planetary_tool: PlanetaryTool::Warm,
            colonies: Vec::new(),
            docked: Vec::new(),
            mission: 0,
            claimed: 0,
            kills: 0,
            energy: MAX_ENERGY,
            fuel_initialised: false,
            income_timer: 0.0,
            message: String::new(),
            ship_message: String::new(),
            current_system: HOME_STAR,
            galaxy_version: 0,
            current_system_empire: None,
            visited: Vec::new(),
            colony_spice: std::collections::BTreeMap::new(),
            cargo_spice: std::collections::BTreeMap::new(),
            empire_relations: std::collections::BTreeMap::new(),
            trade_routes: std::collections::BTreeMap::new(),
            empire_colony_hp: std::collections::BTreeMap::new(),
            conquered_worlds: Vec::new(),
            artifacts: Vec::new(),
            collected_artifacts: Vec::new(),
            rares: Default::default(),
            staff_of_life_charges: 0,
            active_distress: None,
            distress_timer: 0.0,
            pirate_raids_defended: 0,
            ally_raids_defended: 0,
            biohazards_cleansed: 0,
            archetype: SpaceArchetype::Warrior,
            superweapon_cooldown: 0.0,
            siren_song_active: 0.0,
            static_cling_active: 0.0,
            fleet: Vec::new(),
            cargo_wildlife: WildlifeCargo::default(),
            uplifted_empires: std::collections::BTreeMap::new(),
            species_uplifted: 0,
            planets_busted: 0,
            allies: Vec::new(),
            active_alien_mission: None,
            alien_missions_completed: 0,
            danced_with_the_devil: false,
            discovered_earth: false,
            destroyed_earth: false,
            city_layouts: std::collections::BTreeMap::new(),
            vault_collections: std::collections::BTreeMap::new(),
            completed_sets: Vec::new(),
            economy_version: 0,
            tools: Vec::new(),
            tool_charges: std::collections::BTreeMap::new(),
            cards: rules::StageCards::default(),
            superpower_flags: 0,
            difficulty: 1,
            tutorial: rules::TutorialState::default(),
            relationships: std::collections::BTreeMap::new(),
            wars: Vec::new(),
            relationship_clock: 0.0,
            badge_stats: BadgeStats::default(),
            colony_spice_progress: std::collections::BTreeMap::new(),
            pending_cinematic: None,
            pending_damage: 0.0,
            pending_restore: false,
            ships_lost: 0,
            reached_core: false,
            tribute_demand: None,
            universe_clock: 0.0,
            next_pirate_raid: rules::PIRATE_RAID_MIN_TIME,
            next_bio_disaster: rules::BIOSPHERE_COLLAPSE_TIME,
            awareness: std::collections::BTreeMap::new(),
            empire_sizes: Vec::new(),
            repair_request: None,
            fire_clock: Default::default(),
            weapon_tool: None,
            tool_clocks: Default::default(),
            marbles: Default::default(),
            pending_self_tool: None,
            shield_left: 0.0,
            cloak_left: 0.0,
            rally_left: 0.0,
            posse_events: Vec::new(),
            generic_clocks: Default::default(),
            universe: Default::default(),
            endgame: Default::default(),
        }
    }
}

impl SpaceCampaign {
    /// Level (`0..=5`) of one of the ten legacy badge categories, now read
    /// from the matching retail badge family (`space_rules::BADGE_FAMILIES`).
    pub fn badge_tier(&self, badge: SpaceBadge) -> u8 {
        let family = match badge {
            SpaceBadge::Explorer => "explorer",
            SpaceBadge::Colonist => "colonist",
            SpaceBadge::Terraformer => "terrawrangler",
            SpaceBadge::Warmonger => "warmonger",
            SpaceBadge::Trader => "merchant",
            SpaceBadge::Diplomat => "diplomat",
            SpaceBadge::Sightseer => "sightseer",
            SpaceBadge::Collector => "collector",
            SpaceBadge::BodyGuard => "bodyguard",
            SpaceBadge::Cleanser => "cleaner",
        };
        rules::badge_family(family).map(|f| self.badge_family_level(f)).unwrap_or(0)
    }

    /// The value a retail badge requirement reads.
    pub fn badge_counter(&self, counter: rules::BadgeCounter) -> u64 {
        use rules::BadgeCounter as B;
        let s = &self.badge_stats;
        match counter {
            B::ArchetypeSwitches => s.archetype_switches as u64,
            B::EradicateMissions => s.eradicate_missions as u64,
            // Distinct retail rares found (0x103FC20 adds progress only for a
            // rare not found before), the galactic objects, and the pieces of
            // the old invented sets in older saves.
            B::RaresCollected => (self.rares.found.len()
                + self.endgame.galactic_objects.len()
                + self.vault_collections.values().map(|p| p.len()).sum::<usize>()) as u64,
            B::PlanetsColonized => self.new_colony_count() as u64,
            B::AlliancesFormed => s.alliances_formed as u64,
            B::EcoDisasterMissions => (s.eco_disaster_missions + self.biohazards_cleansed) as u64,
            B::StarsExplored => self.visited.len() as u64,
            B::FetchMissions => s.fetch_missions as u64,
            B::TradeVolume => s.trade_volume.max(0.0) as u64,
            B::MissionsComplete => s.missions_complete as u64,
            B::PlanetsTerraformed => self.terraformed.len() as u64,
            B::EmpiresMet => s.empires_met.len() as u64,
            B::WarsStarted => s.wars_started as u64,
            B::FoodWebsComplete => self
                .ecology
                .iter()
                .filter(|e| e.world_id != 0 && e.stable_foodweb_tier() >= 1)
                .count() as u64,
            // The code-driven families (`space_endgame` keeps their counters).
            // `badgeFriendsDefended` (event 0x12): the raid mission's
            // `OnMissionCompleted` 0xC5BB60 adds one when the raided planet
            // belongs to the player or to an ally (VERIFIED); a raid repelled
            // on the player's or an ally's world is that completion
            // (ADAPTED: the port ends a raid when the fighters are destroyed).
            B::FriendsDefended => (self.pirate_raids_defended + self.ally_raids_defended) as u64,
            B::PlanetsConquered => (self.conquered_worlds.len() as u64).saturating_sub(u64::from(self.endgame.purchased_worlds)),
            B::Sightseer => self.endgame.sightseer_stars.len() as u64,
            B::CreaturesPromoted => u64::from(self.species_uplifted),
            B::SystemsPurchased => u64::from(self.endgame.systems_purchased),
            B::EmpireSize => crate::space_endgame::empire_stars(self),
            B::FlightsMade => u64::from(self.endgame.flights_made),
            B::ReachedCash => self.endgame.peak_cash.max(0.0) as u64,
            B::ToolsPurchased => u64::from(self.endgame.tools_purchased),
            B::AestheticTools => u64::from(self.endgame.aesthetic_tools),
            B::TradeRoutes => self.endgame.route_worlds.len() as u64,
            B::StorybooksVisited => 0,
        }
    }

    /// Level of a badge family: the counters' level or the one already
    /// earned (an earned badge stays, `space_endgame::record_badges`).
    pub fn badge_family_level(&self, family: &rules::BadgeFamily) -> u8 {
        rules::badge_level(family, self.badge_counter(family.counter)).max(self.endgame.level(family.id))
    }

    /// Badge points: the `captain` badge (1, earned by Flight 101) plus every
    /// badge earned in every family (each level adds its points;
    /// `ReqBadgePointsEarned` accumulates them, 0xFE58B0).
    pub fn total_badge_points(&self) -> u32 {
        let captain = self.endgame.has_special("captain") || crate::space_endgame::flight101_complete(self);
        u32::from(captain) * rules::CAPTAIN_BADGE_POINTS
            + rules::BADGE_FAMILIES.iter().map(|f| rules::badge_points(f, self.badge_family_level(f))).sum::<u32>()
    }

    /// Promotions earned (`0..=10`, `badgecollector-1..10` at 5..275 points).
    pub fn captain_rank(&self) -> u8 {
        rules::rank_from_points(self.total_badge_points())
    }

    /// Retail title: "Captain", then Commander .. Omnipotent.
    pub fn captain_rank_title(&self) -> &'static str {
        rules::rank_title(self.captain_rank())
    }

    /// Fraction toward the next promotion (the shared progress bar).
    pub fn rank_progress(&self) -> f32 {
        rules::rank_progress(self.total_badge_points())
    }

    pub fn difficulty(&self) -> rules::Difficulty {
        rules::Difficulty::from_index(self.difficulty)
    }

    /// True when the UFO owns `tool_id`.
    pub fn has_tool(&self, tool_id: &str) -> bool {
        self.tools.iter().any(|t| t == tool_id)
    }

    /// Give a persistent tool (idempotent). Returns true when it was new.
    /// The weapon F fires: the palette's choice when the UFO owns it, else the
    /// best laser (the default loadout has laser 1).
    pub fn selected_weapon(&self) -> rules::WeaponStats {
        // A kept weapon, or a consumable one with charges left (the antimatter bomb).
        self.weapon_tool
            .as_deref()
            .filter(|id| self.has_tool(id) || self.charges(id) > 0)
            .and_then(crate::space_tool_use::weapon)
            .unwrap_or_else(|| rules::laser(rules::laser_level(&self.tools)))
    }

    pub fn grant_tool(&mut self, tool_id: &str) -> bool {
        if self.has_tool(tool_id) {
            return false;
        }
        self.tools.push(tool_id.to_string());
        true
    }

    /// Charges of a consumable tool.
    pub fn charges(&self, tool_id: &str) -> u32 {
        self.tool_charges.get(tool_id).copied().unwrap_or(0)
    }

    pub fn add_charges(&mut self, tool_id: &str, count: u32) {
        *self.tool_charges.entry(tool_id.to_string()).or_insert(0) += count;
    }

    /// Use one charge; false when none is left.
    pub fn take_charge(&mut self, tool_id: &str) -> bool {
        match self.tool_charges.get_mut(tool_id) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }

    /// Cargo slots (10, ADAPTED +5 per cargo upgrade owned); every slot holds
    /// a stack of up to 99 of one item.
    pub fn cargo_capacity_limit(&self) -> u8 {
        let upgrades = ["cargoupgrade1", "cargoupgrade2", "cargoupgrade3"]
            .iter()
            .filter(|t| self.has_tool(t))
            .count() as u8;
        MAX_CARGO_CRATES + upgrades * rules::CARGO_SLOTS_PER_UPGRADE
    }

    /// Slots in use: one per (started) stack of 99 of a spice colour or a
    /// wildlife kind.
    pub fn cargo_slots_used(&self) -> u8 {
        let stack = rules::MAX_CARGO_STACK as u32;
        let wild = [
            self.cargo_wildlife.flora,
            self.cargo_wildlife.herbivores,
            self.cargo_wildlife.carnivores,
        ];
        let used: u32 = self
            .cargo_spice
            .values()
            .chain(wild.iter())
            .map(|&n| (n as u32).div_ceil(stack))
            .sum();
        used.min(u8::MAX as u32) as u8
    }

    /// Units of `color` that still fit in the hold.
    pub fn cargo_room_for_spice(&self, color: SpiceColor) -> u8 {
        self.room_in_stack(self.cargo_spice.get(&color).copied().unwrap_or(0))
    }

    /// Specimens of one wildlife kind (0 flora, 1 herbivore, 2 carnivore)
    /// that still fit.
    pub fn cargo_room_for_wildlife(&self, kind: usize) -> u8 {
        let held = match kind {
            0 => self.cargo_wildlife.flora,
            1 => self.cargo_wildlife.herbivores,
            _ => self.cargo_wildlife.carnivores,
        };
        self.room_in_stack(held)
    }

    fn room_in_stack(&self, held: u8) -> u8 {
        let stack = rules::MAX_CARGO_STACK as u32;
        let in_stack = held as u32 % stack;
        let topping = if held > 0 && in_stack != 0 { stack - in_stack } else { 0 };
        let free_slots = self.cargo_capacity_limit().saturating_sub(self.cargo_slots_used()) as u32;
        (free_slots * stack + topping).min((u8::MAX - held) as u32) as u8
    }

    /// Maximum energy (0xFFD920): 100 x energy level x 1.5 with the
    /// cell-carnivore card. (Rank no longer grows it.)
    pub fn max_reactor_energy(&self) -> f32 {
        rules::player_max_energy(rules::energy_level(&self.tools), self.superpower_flags)
    }

    /// Maximum UFO health (0xFFD780): 1000 x health level x difficulty x 1.5
    /// with the creature-attack card.
    pub fn max_hull(&self) -> f32 {
        rules::player_max_health(
            rules::health_level(&self.tools),
            self.difficulty(),
            self.superpower_flags,
        )
    }

    /// Interstellar drive reach in galaxy-map units: the drive's range (3, 5,
    /// 8, 12, 20 galaxy units; 0 without a drive) x `MAP_UNITS_PER_GALAXY_UNIT` (1.0: the
    /// generated star positions are already in galaxy units).
    pub fn max_jump_distance(&self) -> f32 {
        rules::drive_range(&self.tools) * rules::MAP_UNITS_PER_GALAXY_UNIT
    }

    /// Energy as a 0..1 fraction.
    pub fn fuel_fraction(&self) -> f32 {
        (self.energy / self.max_reactor_energy().max(1.0)).clamp(0.0, 1.0)
    }

    /// Record the first fill of the reactor, topping it up only if it is
    /// empty. Returns true when anything changed, so the caller can persist
    /// the flag. A campaign already carrying the flag is never touched: a
    /// state saved at `0.0` (a ship run dry) survives a reload exactly as it
    /// was recorded, while a save predating the flag keeps the fuel it has.
    pub fn initialise_fuel(&mut self) -> bool {
        if self.fuel_initialised {
            return false;
        }
        self.fuel_initialised = true;
        if self.energy <= 0.0 {
            self.energy = self.max_reactor_energy();
        }
        true
    }

    /// Adopt the retail rules once (`economy_version` 0 -> 1) and refresh
    /// the difficulty. A new Space game: wallet 100,000 Sporebucks (S1B: the
    /// player's empire record is created with `spaceEconomyStartingMoney`;
    /// Civ money does not carry over), the retail start tools, the archetype
    /// and superpower flags from the cards, Flight 101 first, full energy.
    /// A save from before the retail rules (it already played Space): keeps
    /// its money plus the 100,000 it never received, gets the start tools,
    /// health / laser upgrades for its old hull / weapon levels, both drives
    /// and a finished tutorial (the old intro let it travel freely), and its
    /// old -100..100 standings as a `Legacy` relationship event (value / 10).
    /// Returns true when anything changed.
    pub fn adopt_retail_rules(
        &mut self,
        wallet: &mut f32,
        cards: rules::StageCards,
        difficulty: i32,
        hull: u8,
        weapon: u8,
    ) -> bool {
        let refreshed = self.difficulty != difficulty;
        self.difficulty = difficulty;
        if self.economy_version >= ECONOMY_VERSION {
            return refreshed;
        }
        let legacy = self.fuel_initialised;
        *wallet = if legacy {
            (wallet.max(0.0) + rules::STARTING_MONEY).min(SPICE_CAP)
        } else {
            rules::STARTING_MONEY
        };
        for t in rules::starting_tools() {
            self.grant_tool(&t);
        }
        if legacy {
            for (i, (id, _)) in rules::HEALTH_UPGRADES.iter().enumerate() {
                if i <= hull as usize {
                    self.grant_tool(id);
                }
            }
            if weapon >= 1 {
                self.grant_tool(tool::LASER_2);
            }
            if weapon >= 2 {
                self.grant_tool(tool::LASER_3);
            }
            self.grant_tool(tool::INTERPLANETARY_DRIVE);
            self.grant_tool(tool::INTERSTELLAR_DRIVE);
            self.tutorial.step = rules::TutorialStep::Done;
            self.tutorial.progress = 0;
            let old: Vec<(EmpireId, i32)> = self.empire_relations.iter().map(|(e, v)| (*e, *v)).collect();
            for (emp, value) in old {
                let rel = self.relationships.entry(emp).or_default();
                rel.met = true;
                rel.events.insert(RelationshipEvent::Legacy, value as f32 / 10.0);
            }
        }
        self.cards = cards;
        self.superpower_flags = rules::superpower_flags(cards);
        self.archetype = rules::archetype_from_cards(cards);
        self.superweapon_cooldown = 0.0;
        let max_energy = self.max_reactor_energy();
        self.energy = if legacy { self.energy.min(max_energy) } else { max_energy };
        self.economy_version = ECONOMY_VERSION;
        self.sync_mission_index();
        true
    }

    /// Buy back missing health and energy at `owner`'s per-point prices
    /// (0xFFD120 / 0x1001840; S2C: ceil(points x price), only the affordable
    /// part with too little money). Returns the Sporebucks spent.
    pub fn buy_repair_recharge(
        &mut self,
        owner: rules::PlanetOwner,
        health: &mut Health,
        wallet: &mut f32,
    ) -> f32 {
        let index = owner.cost_index();
        let missing = (self.max_reactor_energy() - self.energy).max(0.0);
        let (energy, energy_cost) = rules::repair_purchase(missing, rules::RECHARGE_COST[index], *wallet);
        self.energy += energy;
        *wallet -= energy_cost;
        let (hp, hp_cost) = rules::repair_purchase(health.max - health.hp, rules::REPAIR_COST[index], *wallet);
        health.heal(hp);
        *wallet -= hp_cost;
        if energy > 0.0 || hp > 0.0 {
            self.message = format!(
                "Repaired {hp:.0} health and recharged {energy:.0} energy for {:.0} Sporebucks",
                energy_cost + hp_cost
            );
        }
        energy_cost + hp_cost
    }

    /// Mirror the tutorial step into the legacy `mission` / `claimed` fields
    /// (index into `MISSION_ORDER`, steps finished).
    fn sync_mission_index(&mut self) {
        let index = rules::TutorialStep::ORDER
            .iter()
            .position(|s| *s == self.tutorial.step)
            .unwrap_or(MISSION_ORDER.len());
        self.mission = index.min(MISSION_ORDER.len());
        self.claimed = self.mission;
    }

    /// Upgrade a pre-ecology save without discarding its existing world flags.
    /// Legacy terraformed/owned worlds become T1 colonies with one stable
    /// food-web tier, preserving their former terraform and income behavior.
    pub fn migrate_ecology(&mut self) -> bool {
        let legacy_campaign = self.ecology_version == 0
            || (self.ecology.is_empty()
                && (!self.terraformed.is_empty() || !self.colonies.is_empty()));
        if !legacy_campaign {
            return false;
        }
        let legacy_worlds: Vec<u32> = self
            .terraformed
            .iter()
            .chain(self.colonies.iter())
            .copied()
            .collect();
        for id in legacy_worlds {
            if self.ecology.iter().any(|entry| entry.world_id == id) {
                continue;
            }
            let mut ecology = PlanetEcology {
                world_id: id,
                temperature: 0.30,
                atmosphere: 0.70,
                flora: FLORA_NICHES_PER_TIER,
                herbivores: HERBIVORES_PER_TIER,
                carnivores: CARNIVORES_PER_TIER,
                colony_count: 0,
            };
            if self.colonies.contains(&id) {
                ecology.colony_count = 1;
            }
            self.ecology.push(ecology);
        }
        self.ecology_version = 1;
        true
    }

    fn ecology_index(&self, id: u32) -> Option<usize> {
        self.ecology.iter().position(|entry| entry.world_id == id)
    }

    pub fn ensure_ecology(&mut self, id: u32) -> usize {
        if let Some(index) = self.ecology_index(id) {
            return index;
        }
        let mut ecology = if id == 0 || id == space_galaxy::earth_world_id(space_galaxy::SOL_STAR) {
            PlanetEcology::completed(id)
        } else {
            let (temperature, atmosphere) = space_galaxy::adapted_climate_baseline(id);
            PlanetEcology {
                world_id: id,
                temperature,
                atmosphere,
                flora: 0,
                herbivores: 0,
                carnivores: 0,
                colony_count: 0,
            }
        };
        ecology.colony_count = u8::from(id == 0 || self.colonies.contains(&id));
        self.ecology.push(ecology);
        self.ecology.len() - 1
    }

    /// Read-only ecology snapshot. Unseen worlds use their stable deterministic
    /// baseline without writing to the save.
    pub fn ecology_for(&self, id: u32) -> PlanetEcology {
        self.ecology
            .iter()
            .find(|entry| entry.world_id == id)
            .copied()
            .unwrap_or_else(|| {
                if id == 0 {
                    let mut ecology = PlanetEcology::completed(id);
                    ecology.colony_count = 1;
                    ecology
                } else {
                    let (temperature, atmosphere) = space_galaxy::adapted_climate_baseline(id);
                    PlanetEcology {
                        world_id: id,
                        temperature,
                        atmosphere,
                        flora: 0,
                        herbivores: 0,
                        carnivores: 0,
                        colony_count: u8::from(self.colonies.contains(&id)),
                    }
                }
            })
    }

    pub fn ecology_summary(&self, id: u32) -> String {
        self.ecology_for(id).summary()
    }

    /// Total spice crates currently in the ship's Cargo Hold.
    pub fn cargo_spice_total(&self) -> u8 {
        self.cargo_spice
            .values()
            .copied()
            .fold(0u8, u8::saturating_add)
    }

    /// Total items currently in the ship's Cargo Hold (spice crates + wildlife specimens).
    pub fn cargo_total(&self) -> u8 {
        self.cargo_spice_total()
            .saturating_add(self.cargo_wildlife.total())
    }

    /// Escort ships the player may command (`space_rules::posse_limit`: one
    /// per two promotions, `badgeEarnPosse`).
    pub fn max_fleet_size(&self) -> usize {
        rules::posse_limit(self.captain_rank())
    }

    /// Cycle to the next archetype among the 10 canonical Spore philosophies
    /// (debug helper; retail changes archetype through missions).
    pub fn cycle_archetype(&mut self) {
        let i = SpaceArchetype::ALL
            .iter()
            .position(|&a| a == self.archetype)
            .unwrap_or(0);
        self.archetype = SpaceArchetype::ALL[(i + 1) % SpaceArchetype::ALL.len()];
        self.message = format!(
            "Archetype set to {}: superpower '{}'",
            self.archetype.title(),
            self.archetype.superpower_name()
        );
    }

    /// Set a specific archetype.
    pub fn set_archetype(&mut self, archetype: SpaceArchetype) {
        self.archetype = archetype;
    }

    /// Adds wild specimens into the Cargo Hold while they fit. Returns number added.
    pub fn add_safari_specimens(&mut self, flora: u8, herbivores: u8, carnivores: u8) -> u8 {
        let mut added = 0u8;
        for (kind, want) in [flora, herbivores, carnivores].into_iter().enumerate() {
            let take = want.min(self.cargo_room_for_wildlife(kind));
            match kind {
                0 => self.cargo_wildlife.flora += take,
                1 => self.cargo_wildlife.herbivores += take,
                _ => self.cargo_wildlife.carnivores += take,
            }
            added += take;
        }
        added
    }

    /// `R`: Activates the signature superpower of the player's Space Archetype
    /// (retail recharge per tool; the Wanderer has none).
    pub fn try_activate_superweapon(
        &mut self,
        run: &mut RunProgress,
        target_world: Option<u32>,
    ) -> WorldAction {
        let Some(power) = rules::archetype_superpower(self.archetype) else {
            return WorldAction::Refused("Wanderers have no archetype superpower".into());
        };
        if self.superweapon_cooldown > 0.0 {
            return WorldAction::Refused(format!(
                "{} is recharging ({:.0}s remaining)",
                power.name, self.superweapon_cooldown
            ));
        }

        match self.archetype {
            SpaceArchetype::Warrior => {
                // ADAPTED: the retail use (`cToolStrategy::OnHit` 0x1059DF0, VERIFIED) asks
                // the mission manager for the "default planetary raid event" (mission
                // 0xBE436935, `missionRaidDuration` 600 s, ally wait 60 s) on the
                // active planet at the hit position and signals the space game (0xFF8CB0,
                // message 0x638207A); its gate wants an Empire-level planet. The rewrite's
                // raid machinery (`space_universe` attacks) only raids the player's and
                // allied colonies, so the use is recorded (recharge, save) and announced
                // without a raid.
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = "Raider Rally activated! Pirate raiders summoned into the system to cause chaos!".to_string();
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Zealot => {
                let Some(id) = target_world else {
                    return WorldAction::Refused(
                        "Fanatical Frenzy requires targeting an alien empire colony world".into(),
                    );
                };
                let Some(emp) = self.world_empire(id) else {
                    return WorldAction::Refused(format!(
                        "World {id} is not an alien empire colony; choose an alien world"
                    ));
                };
                if let Err(refusal) = self.check_gate("PlanetConversion", id) {
                    return refusal;
                }
                // `cToolStrategy::OnHit` 0x1059C95..0x1059D35 (VERIFIED): the event log entry
                // `SPG_CapturedStar`, then 0xC353A0(active star, player empire) moves the whole
                // STAR from its empire to the player's (RemoveStar / AddStar, the star's
                // planets leave the comm manager's lists): every planet the empire held in
                // the system is converted, not only the one aimed at.
                let sys = space_galaxy::system_of_world(id);
                let held: Vec<u32> = (0..space_galaxy::WORLDS_PER_SYSTEM)
                    .map(|slot| space_galaxy::world_id(sys, slot))
                    .filter(|&w| self.world_empire(w).is_some())
                    .collect();
                for &w in &held {
                    push_unique(&mut self.conquered_worlds, w);
                    self.empire_colony_hp.remove(&w);
                    self.trade_routes.remove(&w);
                    self.own(w);
                }
                run.colonies = self.new_colony_count();
                let note = self.apply_tool_relationship(emp, power);
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Fanatical Frenzy! The {} system is captured: {} world(s) converted to your empire! {note}",
                    Self::empire_name(emp),
                    held.len()
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Scientist => {
                let Some(id) = target_world else {
                    return WorldAction::Refused(
                        "Gravitation Wave requires targeting an alien empire colony world".into(),
                    );
                };
                let Some(emp) = self.world_empire(id) else {
                    return WorldAction::Refused(format!(
                        "World {id} is not an alien empire colony; choose an alien world"
                    ));
                };
                // ADAPTED: the wave (launch routine 0xCC1A10, VERIFIED) calls
                // `DestroyCity` for every city of the planet in the half-space ahead of the
                // wave (the cities' direction from the hit point against the wave's) and
                // kills every tribe object there (TakeDamage with its full health); the
                // rewrite's empire colonies are one defense-hit-point pool, flattened whole.
                self.empire_colony_hp.insert(id, 0.0);
                let note = self.apply_tool_relationship(emp, power);
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Gravitation Wave unleashed on world {id}! All planetary defense structures obliterated! {note} Press C to claim"
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Ecologist => {
                let added = self.add_safari_specimens(2, 2, 2);
                if added == 0 {
                    return WorldAction::Refused(
                        "Safari Vacuum: Cargo Hold is already full! Deploy specimens with Y or sell spice".into(),
                    );
                }
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Safari Vacuum beamed up {added} wildlife specimens into your Cargo Hold ({}/{} slots)!",
                    self.cargo_slots_used(),
                    self.cargo_capacity_limit()
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Trader => {
                let Some(id) = target_world else {
                    return WorldAction::Refused(
                        "Cash Infusion requires targeting an alien empire world".into(),
                    );
                };
                if self.world_empire(id).is_none() {
                    return WorldAction::Refused(format!(
                        "World {id} is not an alien empire world; choose an alien world"
                    ));
                }
                // The gate (0x1056130) wants a trade route with the planet's empire
                // (log 0xCCBE06F7); `OnHit` 0x1059BC4 (VERIFIED) then sets that route's
                // maturity (0x1037EC0: the route record's float +0x2C) to 100, it creates none.
                if let Err(refusal) = self.check_gate("cashinfusion", id) {
                    return refusal;
                }
                self.trade_routes.insert(id, 1.0);
                push_unique(&mut self.docked, id);
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Cash Infusion deployed! Trade route on world {id} reached 100% maturity; press C to buy out!"
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Bard => {
                // 120 s (VERIFIED 0xC757F0). While it plays no empire's relationship
                // with the player reads below 0 (`relationship`); the pacified pirates
                // and turrets below are ADAPTED (no AI consumer of the song was found).
                self.siren_song_active = crate::space_tool_use::SOOTHING_SONG_SECONDS;
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Soothing Song resonated across the stars! No empire's attitude falls below neutral for {:.0} seconds!",
                    crate::space_tool_use::SOOTHING_SONG_SECONDS
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Diplomat => {
                self.static_cling_active = 45.0;
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = "Static Cling deployed! All enemy ships and planetary defenses disabled for 45 seconds!".to_string();
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Knight => {
                // The gate (0x1056130): a posse of five is full (log 0x72CB7F2C). `OnHit`
                // 0x1059D3A (VERIFIED) opens a temporary posse slot when the posse is at
                // its limit (0xC75760), so the ship comes whatever the rank allows, and joins
                // a ship of the PLAYER's empire (`AddPosse` 0x1002650). The ship leaves when
                // the tool is ready again (`space_tool_use::expire_backup`, 120 s).
                if let Err(refusal) = self.check_gate("callforbackup", target_world.unwrap_or(0)) {
                    return refusal;
                }
                let tier = rules::empire_tier(self.player_star_count());
                let hp = rules::UFO_HEALTH_ALLY[tier] * self.difficulty().pick(rules::DIFFICULTY_ALLY_HEALTH);
                self.fleet.push(EscortShip {
                    name: format!("Mini-U Drone #{}", self.fleet.len() + 1),
                    empire: None,
                    hp,
                    max_hp: hp,
                });
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = format!(
                    "Summon Mini-U deployed an autonomous wingman fighter! Fleet: {}",
                    self.fleet.len()
                );
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Shaman => {
                self.current_system = HOME_STAR;
                self.superweapon_cooldown = power.recharge_seconds;
                run.request_save();
                let msg = "Return Ticket punched! Hyperspace rift opened, warping you back to your home star!".to_string();
                self.message = msg.clone();
                WorldAction::Done(msg)
            }
            SpaceArchetype::Wanderer => unreachable!("the Wanderer has no superpower"),
        }
    }

    /// Stars the player's empire spans (home plus colonised systems).
    pub fn player_star_count(&self) -> u32 {
        let mut systems: Vec<u32> = self
            .colonies
            .iter()
            .map(|&w| space_galaxy::system_of_world(w))
            .collect();
        systems.push(HOME_STAR);
        systems.sort_unstable();
        systems.dedup();
        systems.len() as u32
    }

    /// Spice a colony on `world_id` stores before production stops
    /// (`colonyMaxSpiceStoredPerColony` 5 per colonised world).
    pub fn colony_spice_capacity(&self, world_id: u32) -> u8 {
        if !self.colonies.contains(&world_id) && world_id != 0 {
            return 0;
        }
        rules::colony_spice_cap(false) as u8
    }

    /// Stored spice crates currently waiting on `world_id`.
    pub fn colony_spice_stored(&self, world_id: u32) -> u8 {
        self.colony_spice.get(&world_id).copied().unwrap_or(0)
    }

    /// Returns the alien empire index (`0..EMPIRES.len()`) controlling `world_id`,
    /// if `world_id` is an unconquered primary foreign world in an empire star system,
    /// or a world uplifted into an empire via Monolith.
    pub fn world_empire(&self, world_id: u32) -> Option<EmpireId> {
        if world_id < space_galaxy::FOREIGN_WORLD_BASE
            || self.colonies.contains(&world_id)
            || self.conquered_worlds.contains(&world_id)
        {
            return None;
        }
        if let Some(&emp) = self.uplifted_empires.get(&world_id) {
            return Some(emp);
        }
        let slot = (world_id - space_galaxy::FOREIGN_WORLD_BASE) % space_galaxy::WORLDS_PER_SYSTEM;
        // The empire holds the first planet of its stars and the colonies
        // `chanceAdditionalPlanetsAreSpaceTech` makes (generated worlds of
        // Empire tech level).
        let held = slot == 0
            || space_galaxy::world_info(world_id)
                .is_some_and(|info| info.tech == space_galaxy::TechLevel::Empire);
        if !held {
            return None;
        }
        let sys = space_galaxy::system_of_world(world_id);
        if sys == self.current_system {
            self.current_system_empire
        } else {
            None
        }
    }

    pub fn empire_name(empire: EmpireId) -> &'static str {
        space_galaxy::EMPIRES
            .get(empire as usize)
            .map(|e| e.0)
            .unwrap_or("Alien")
    }

    /// The empire's relationship personality (its archetype; empire 0 is the Grox).
    pub fn personality(empire: EmpireId) -> rules::Personality {
        rules::Personality::of(empire_archetype(empire), empire == 0)
    }

    /// Star count of an empire (from the galaxy; 1 when unknown).
    pub fn empire_size(&self, empire: EmpireId) -> u32 {
        if empire == 0 {
            return rules::GROX_STARS;
        }
        self.empire_sizes.get(empire as usize).copied().unwrap_or(1).max(1)
    }

    /// The retail relationship value (-10..10) with `empire`; an empire not
    /// met yet reads as it will at first contact.
    pub fn relationship(&self, empire: EmpireId) -> f32 {
        let personality = Self::personality(empire);
        let difficulty = self.difficulty();
        let value = match self.relationships.get(&empire) {
            Some(rel) if rel.met => rel.value(personality, difficulty),
            other => {
                let mut rel = other.cloned().unwrap_or_default();
                rel.first_contact(personality, self.archetype, difficulty);
                rel.value(personality, difficulty)
            }
        };
        // The Bard's soothing song (0xC757F0, 120 s): `CalculateRelationshipAbsolute`
        // 0xD05A60 reads a negative relationship as 0 unless the Grox are a side.
        crate::space_tool_use::soothed_relationship(value, self.siren_song_active > 0.0, empire == 0)
    }

    /// Meet an empire (first contact applies its personality); counts it for
    /// the traveler badge and settles its stance.
    pub fn meet_empire(&mut self, empire: EmpireId) -> Option<String> {
        let personality = Self::personality(empire);
        let difficulty = self.difficulty();
        let archetype = self.archetype;
        let rel = self.relationships.entry(empire).or_default();
        if rel.met {
            return None;
        }
        rel.first_contact(personality, archetype, difficulty);
        if !self.badge_stats.empires_met.contains(&empire) {
            self.badge_stats.empires_met.push(empire);
        }
        self.update_stance_from(empire, 0.0)
    }

    /// Apply a relationship event to `empire` (ApplyRelationship with a
    /// scale) and settle war / alliance; returns a stance message if it changed.
    pub fn apply_relationship(
        &mut self,
        empire: EmpireId,
        event: RelationshipEvent,
        scale: f32,
    ) -> Option<String> {
        let first = self.meet_empire(empire);
        let personality = Self::personality(empire);
        let difficulty = self.difficulty();
        let before = self.relationship(empire);
        self.relationships
            .entry(empire)
            .or_default()
            .apply(event, scale, personality, difficulty);
        self.update_stance_from(empire, before).or(first)
    }

    /// Settle the stance as if the relationship just fell (see `update_stance_from`).
    pub fn update_stance(&mut self, empire: EmpireId) -> Option<String> {
        self.update_stance_from(empire, f32::INFINITY)
    }

    /// 0x1013370 (VERIFIED), run by the universe update 0x1013650 for every
    /// empire with a relationship to the player: allied and below
    /// `endAlliance` (manager +0x54) -> BreakAlliance (event 0x4CA76FBD);
    /// allied and just fallen below `createAllianceBase` (+0x50) -> the
    /// warning event 0x2194609D; at war and above the peace threshold (+0x3C)
    /// -> DeclarePeace (event 0xBDFD6EC3); not at war and below
    /// `warThreshold` (+0x38, strict) -> DeclareWar (badge WarsStarted).
    /// `last` is the value seen at the previous update. All three war
    /// thresholds are -7 in the data.
    pub fn stance_update(&mut self, empire: EmpireId, last: Option<f32>) -> Option<String> {
        let value = self.relationship(empire);
        let name = Self::empire_name(empire);
        if self.allies.contains(&empire) {
            if value < rules::END_ALLIANCE_THRESHOLD {
                self.allies.retain(|&a| a != empire);
                return Some(format!("The {name} empire broke off its alliance"));
            }
            if value < rules::CREATE_ALLIANCE_THRESHOLD && last.is_some_and(|l| l >= rules::CREATE_ALLIANCE_THRESHOLD) {
                return Some(format!("Relations with your ally the {name} empire are cooling"));
            }
        }
        let at_war = self.wars.contains(&empire);
        if at_war && value > rules::WAR_END_THRESHOLD {
            self.wars.retain(|&w| w != empire);
            return Some(format!("The {name} empire ended its war with you"));
        }
        if !at_war && value < rules::WAR_THRESHOLD {
            self.wars.push(empire);
            self.allies.retain(|&a| a != empire);
            self.badge_stats.wars_started += 1;
            return Some(format!("The {name} empire declared WAR on you!"));
        }
        None
    }

    /// War at relationship <= -7 (`warThreshold`), peace above -7
    /// (`warEndThreshold`), an alliance breaks below 3 (`endAlliance`).
    /// The AI declares war when the relationship falls to the threshold (from
    /// `before`), so a bought peace holds while the value recovers. This is the
    /// event-driven edge of the retail player-stance update (0x1013650 ->
    /// 0x1013370, VERIFIED in `space_universe::tick`, which re-evaluates every
    /// empire each tick); the two agree, this one only reacts a tick sooner.
    pub fn update_stance_from(&mut self, empire: EmpireId, before: f32) -> Option<String> {
        let value = self.relationship(empire);
        let name = Self::empire_name(empire);
        let at_war = self.wars.contains(&empire);
        if !at_war && rules::declares_war(value) && value < before - 1e-6 {
            self.wars.push(empire);
            self.allies.retain(|&a| a != empire);
            return Some(format!("The {name} empire declared WAR on you!"));
        }
        if at_war && rules::war_ends(value) {
            self.wars.retain(|&w| w != empire);
            return Some(format!("The {name} empire ended its war with you"));
        }
        if self.allies.contains(&empire) && rules::alliance_breaks(value) {
            self.allies.retain(|&a| a != empire);
            return Some(format!("The {name} empire broke off its alliance"));
        }
        None
    }

    /// A tool's relationship deltas: `toolRelationshipDelta` on the target
    /// (BadToolUse / GoodToolUse scaled by its size) and
    /// `toolGalacticRelationshipDelta` on the empires near the active star.
    fn apply_tool_relationship(&mut self, target: EmpireId, power: rules::SuperpowerTool) -> String {
        // `toolGalacticRelationshipRadius` of the record (10 for the three tools that have one).
        let radius = space_tools::record(power.id).map_or(10.0, |r| r.galactic_radius);
        self.apply_hit_deltas_in(target, power.relationship_delta, power.galactic_relationship_delta, radius)
    }

    /// `cToolStrategy::OnHit` 0x10593B0 (VERIFIED 0x105987C): a hit on a city of
    /// `target` applies `delta` as GoodToolUse (above 0) or BadToolUse (below 0)
    /// of its size; the galactic delta is told to the empires near the active
    /// star ([`Self::apply_galactic_delta`]). The radius of the three tools that
    /// carry a galactic delta (planet buster, gravitation wave, planet
    /// conversion) is 10.
    pub fn apply_hit_deltas(&mut self, target: EmpireId, delta: f32, galactic: f32) -> String {
        self.apply_hit_deltas_in(target, delta, galactic, 10.0)
    }

    /// [`Self::apply_hit_deltas`] with the tool's own
    /// `toolGalacticRelationshipRadius`.
    pub fn apply_hit_deltas_in(&mut self, target: EmpireId, delta: f32, galactic: f32, radius: f32) -> String {
        let mut notes = Vec::new();
        if let Some((event, scale)) = crate::space_tool_use::relationship_event(delta) {
            notes.extend(self.apply_relationship(target, event, scale));
        }
        notes.extend(self.apply_galactic_delta(galactic, radius));
        notes.join(" ")
    }

    /// The second half of `OnHit` (0x1059A1A..0x1059BAD, VERIFIED): a galactic
    /// delta above 0 is a GoodToolUse of that size, below 0 a
    /// **CommittedAtrocity** (0x068B2971) of its magnitude, applied to every
    /// empire except the player that owns an Empire-level star within
    /// `min(radius, 30)` of the active star (`FindStars` around the star's
    /// position with the tech filter 0x20): the owner of the star being hit
    /// is among them, the rest of the galaxy hears nothing.
    pub fn apply_galactic_delta(&mut self, galactic: f32, radius: f32) -> Vec<String> {
        let Some((event, scale)) = (if galactic > 0.0 {
            Some((RelationshipEvent::GoodToolUse, galactic))
        } else if galactic < 0.0 {
            Some((RelationshipEvent::CommittedAtrocity, -galactic))
        } else {
            None
        }) else {
            return Vec::new();
        };
        let mut notes = Vec::new();
        for emp in self.galactic_witnesses_within(radius) {
            notes.extend(self.apply_relationship(emp, event, scale));
        }
        notes
    }

    /// The empires owning a star within `min(radius, 30)` galaxy units of the
    /// active star (the star itself included): the audience of a tool's
    /// galactic delta (0x1059A81). ADAPTED: a run without a galaxy (headless
    /// tests) has no stars, so the empires met stand in for them.
    pub fn galactic_witnesses_within(&self, radius: f32) -> Vec<EmpireId> {
        let radius = rules::galactic_delta_radius(radius);
        let Some(galaxy) = space_galaxy::registered_galaxy() else {
            return self.galactic_witnesses();
        };
        let here = self.current_system;
        let mut empires = std::collections::BTreeSet::new();
        let near = galaxy.stars_within(here, radius).into_iter().map(|(star, _)| star);
        for star in std::iter::once(here).chain(near) {
            if let Some(owner) = galaxy.star(star).filter(|s| s.tech == crate::space_worldgen::TechLevel::Empire).and_then(|s| s.empire) {
                empires.insert(owner);
            }
        }
        empires.into_iter().collect()
    }

    /// The empires that hear of an alliance with the Grox: `DeclareAlliance`
    /// 0xD06A6D calls 0xD06900(manager, empire, AlliedWithGrob, 1.0), which
    /// applies the event to every relationship pair of the allying empire;
    /// the pairs with the player count only for an empire the player has met
    /// (0xD064F0 asks the player state's per-empire flag, 0xC7AA30 bit 5, VERIFIED).
    /// So: every empire met, not the galaxy at large (a farther empire is
    /// met later with no penalty, as in retail).
    pub fn galactic_witnesses(&self) -> Vec<EmpireId> {
        let mut ids: std::collections::BTreeSet<EmpireId> = self.badge_stats.empires_met.iter().copied().collect();
        ids.extend(self.relationships.iter().filter(|(_, r)| r.met).map(|(e, _)| *e));
        ids.into_iter().collect()
    }

    /// Weapons of mass destruction: the planet buster's hit (`toolGalacticRelationshipDelta`
    /// -7, radius 10) on the planet being destroyed, told to the empires near
    /// the active star ([`Self::apply_galactic_delta`]). Returns the stance news.
    pub fn violate_galactic_code(&mut self, run: &mut RunProgress) -> String {
        let record = space_tools::record("planetbusterbomb");
        let (galactic, radius) = record.map_or((-7.0, 10.0), |r| (r.galactic_delta, r.galactic_radius));
        let news = self.apply_galactic_delta(galactic, radius);
        run.request_save();
        if news.is_empty() {
            "(Atrocity: the empires nearby took note)".into()
        } else {
            format!("(Atrocity! {})", news.join(" "))
        }
    }

    /// Diplomatic standing on the legacy -100..100 scale: the retail
    /// relationship x 10 (kept for the HUD and old callers).
    pub fn empire_relation(&self, empire: EmpireId) -> i32 {
        (self.relationship(empire) * 10.0).round() as i32
    }

    /// "At War", "Allied", else the retail attitude (Furious .. Friendly).
    pub fn relation_label(&self, empire: EmpireId) -> &'static str {
        if self.wars.contains(&empire) {
            "At War"
        } else if self.allies.contains(&empire) {
            "Allied"
        } else {
            rules::attitude(self.relationship(empire)).title()
        }
    }

    /// Attitude of a legacy -100..100 score (the retail value x 10).
    pub fn score_label(score: i32) -> &'static str {
        rules::attitude(score as f32 / 10.0).title()
    }

    /// Remaining defense HP of an alien empire colony (`0.0..=EMPIRE_COLONY_MAX_HP`).
    pub fn empire_defense_hp(&self, world_id: u32) -> f32 {
        self.empire_colony_hp
            .get(&world_id)
            .copied()
            .unwrap_or(EMPIRE_COLONY_MAX_HP)
    }

    /// True once a colony's defenses fell to `spaceWarSurrenderThreshold`.
    pub fn empire_colony_surrendered(&self, world_id: u32) -> bool {
        self.empire_defense_hp(world_id) <= EMPIRE_COLONY_MAX_HP * rules::WAR_SURRENDER_THRESHOLD
    }

    /// True if `system` has any active trade dock or empire trade route.
    pub fn has_trade_in_system(&self, system: u32) -> bool {
        self.docked
            .iter()
            .any(|&w| space_galaxy::system_of_world(w) == system)
            || self
                .trade_routes
                .keys()
                .any(|&w| space_galaxy::system_of_world(w) == system)
    }

    /// Damage an alien empire colony's defenses (`F` weapon fire near an
    /// empire world). Each laser shot is a bad tool use (`laserlevel1`
    /// `toolRelationshipDelta` -0.02); the first attack on an empire not at
    /// war starts one (`StartedWar`, warmonger badge). ADAPTED trigger.
    pub fn damage_empire_colony(&mut self, world_id: u32, damage: f32) -> Option<String> {
        self.damage_empire_colony_with(world_id, damage, -0.02, 0.0)
    }

    /// [`Self::damage_empire_colony`] by a tool with its own `toolRelationshipDelta`
    /// and galactic delta (`OnHit` 0x105987C: a bomb's -0.5 hurts more than a
    /// laser's -0.02, the superweapons also tell the galaxy).
    pub fn damage_empire_colony_with(&mut self, world_id: u32, damage: f32, delta: f32, galactic: f32) -> Option<String> {
        let emp = self.world_empire(world_id)?;
        let emp_name = Self::empire_name(emp);
        let hit_news = self.apply_hit_deltas(emp, delta, galactic);
        let mut news = (!hit_news.is_empty()).then_some(hit_news);
        if !self.wars.contains(&emp) {
            self.badge_stats.wars_started += 1;
            self.apply_relationship(emp, RelationshipEvent::StartedWar, 1.0);
            if !self.wars.contains(&emp) {
                self.wars.push(emp);
                self.allies.retain(|&a| a != emp);
            }
            news = Some(format!("You started a war with the {emp_name} empire!"));
        }
        let hp = (self.empire_defense_hp(world_id) - damage.max(0.0)).max(0.0);
        self.empire_colony_hp.insert(world_id, hp);
        let msg = if self.empire_colony_surrendered(world_id) {
            format!(
                "{emp_name} colony defenses on world {world_id} surrendered! Press C to capture"
            )
        } else {
            format!(
                "Attacked {emp_name} colony on world {world_id} (defenses {hp:.0}/{EMPIRE_COLONY_MAX_HP:.0}, {}){}",
                self.relation_label(emp),
                news.map(|n| format!(" {n}")).unwrap_or_default()
            )
        };
        self.message = msg.clone();
        Some(msg)
    }

    /// `B` near a colony or empire world: at an owned colony load its stored
    /// spice into the hold; at an empire sell the hold's spice (retail price
    /// x difficulty, at most 200 units per sale) and the vault's rares.
    pub fn try_collect_or_trade_spice(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        let owned = self.colonies.contains(&id) || id == 0;
        let emp = self.world_empire(id);
        if !owned && emp.is_none() {
            return WorldAction::Refused(format!(
                "World {id} has no colony or trading post; colonise it first"
            ));
        }
        if owned {
            let local_color = SpiceColor::for_world(id);
            let stored = self.colony_spice_stored(id);
            if stored == 0 {
                return WorldAction::Refused(format!(
                    "No {} spice stored on world {id} yet (cargo {}/{} slots)",
                    local_color.label(),
                    self.cargo_slots_used(),
                    self.cargo_capacity_limit()
                ));
            }
            let take = stored.min(self.cargo_room_for_spice(local_color));
            if take == 0 {
                return WorldAction::Refused(
                    "Cargo hold is full; fly to an empire and press B to sell".into(),
                );
            }
            self.colony_spice.insert(id, stored - take);
            *self.cargo_spice.entry(local_color).or_insert(0) += take;
            run.request_save();
            let msg = format!(
                "Loaded {take} {} spice from world {id} (cargo {}/{} slots)",
                local_color.label(),
                self.cargo_slots_used(),
                self.cargo_capacity_limit()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        let emp_idx = emp.expect("empire world");
        let emp_name = Self::empire_name(emp_idx);
        self.meet_empire(emp_idx);
        if self.wars.contains(&emp_idx) {
            return WorldAction::Refused(format!(
                "The {emp_name} empire is at war with you; make peace with K first"
            ));
        }
        let difficulty = self.difficulty();
        let mut budget = rules::NPC_MAX_SPICE_BOUGHT;
        let mut sold = 0u32;
        let mut revenue = 0.0f32;
        for color in SpiceColor::ALL {
            let held = self.cargo_spice.get(&color).copied().unwrap_or(0) as u32;
            let count = held.min(budget);
            if count == 0 {
                continue;
            }
            budget -= count;
            sold += count;
            revenue += color.sell_price(difficulty) * count as f32;
            let left = (held - count) as u8;
            if left == 0 {
                self.cargo_spice.remove(&color);
            } else {
                self.cargo_spice.insert(color, left);
            }
        }
        let sold_artifacts = self.artifacts.len();
        for artifact in self.artifacts.drain(..) {
            revenue += artifact.value as f32;
        }
        if sold == 0 && sold_artifacts == 0 {
            return WorldAction::Refused(
                "Cargo hold is empty; collect spice at your colonies with B first".into(),
            );
        }
        run.spice = (run.spice + revenue).min(SPICE_CAP);
        self.badge_stats.trade_volume += revenue as f64;
        let news = self.apply_relationship(emp_idx, RelationshipEvent::TradeComplete, 1.0);
        if sold > 0 {
            self.advance_tutorial(rules::TutorialStep::TeachTradeSpice, run);
        }
        run.request_save();
        let artifact_msg = if sold_artifacts > 0 {
            format!(" & {sold_artifacts} rare(s)")
        } else {
            String::new()
        };
        let msg = format!(
            "Sold {sold} spice{artifact_msg} to the {emp_name} empire for {revenue:.0} Sporebucks (cargo {}/{} slots){}",
            self.cargo_slots_used(),
            self.cargo_capacity_limit(),
            news.map(|n| format!(" {n}")).unwrap_or_default()
        );
        self.message = msg.clone();
        WorldAction::Done(msg)
    }

    /// A procedural mission from `emp_idx`: the kind is drawn with the empire
    /// personality's `missionGeneratorWeight` among the retail kinds OpenSpore
    /// can play (ADAPTED groups: fetch animal / plant / sample -> specimen
    /// collection; terraform -> terraform world; war / destroy turrets /
    /// eradicate citizens -> pirate hunt; fetch artifact / tribe / civ / empire
    /// citizen / commodity -> the `fetch` mission of [`Self::fetch_target`];
    /// eradicate animals, balance, multi-delivery and scan-fetch have their own
    /// kind), paid and timed by the retail rows and multipliers. A kind with a
    /// target scoring (`cMission` vtable slot 0x184) is offered only when a
    /// target lies in range (a kind without one has weight 0).
    pub fn generate_alien_mission(&self, emp_idx: EmpireId) -> AlienMission {
        self.generate_alien_mission_with(emp_idx, None)
    }

    /// [`Self::generate_alien_mission`] with the retail kind forced when it is
    /// offerable (capture flag `--space-comm-mission`).
    pub fn generate_alien_mission_with(&self, emp_idx: EmpireId, force: Option<rules::MissionKind>) -> AlienMission {
        use rules::MissionKind as K;
        let emp_name = Self::empire_name(emp_idx);
        let weights = Self::personality(emp_idx).mission_weights();
        let w = |kind: K| {
            rules::GENERATOR_ORDER
                .iter()
                .position(|k| *k == kind)
                .map_or(0, |i| weights[i])
        };
        // A scored kind is offered only when a target of its type lies in range
        // (retail scores every candidate planet, -1 for a wrong one, 0xC523E0).
        let targeted: Vec<(K, u32)> = [
            K::FetchArtifact,
            K::FetchTribeCitizen,
            K::FetchCivCitizen,
            K::FetchEmpireCitizen,
            K::FetchCommodity,
            K::EradicateAnimals,
            K::ScanFetch,
        ]
        .into_iter()
        .filter_map(|kind| self.fetch_target(kind, emp_idx).map(|world| (kind, world)))
        .collect();
        let balance = self.balance_target();
        let delivery = self.multi_delivery_plan(emp_idx);
        let target_weight = |kind: K| if targeted.iter().any(|(k, _)| *k == kind) { w(kind) } else { 0 };
        let mut groups = [
            (AlienMissionKind::SpecimenCollection, K::FetchAnimal, w(K::FetchAnimal) + w(K::FetchPlant) + w(K::SampleCollection)),
            (AlienMissionKind::TerraformWorld, K::Terraform, w(K::Terraform)),
            (AlienMissionKind::PirateHunt, K::DestroyTurrets, w(K::War) + w(K::DestroyTurrets) + w(K::EradicateCityCitizens)),
            (AlienMissionKind::Fetch, K::FetchArtifact, target_weight(K::FetchArtifact)),
            (AlienMissionKind::Fetch, K::FetchTribeCitizen, target_weight(K::FetchTribeCitizen)),
            (AlienMissionKind::Fetch, K::FetchCivCitizen, target_weight(K::FetchCivCitizen)),
            (AlienMissionKind::Fetch, K::FetchEmpireCitizen, target_weight(K::FetchEmpireCitizen)),
            (AlienMissionKind::Fetch, K::FetchCommodity, target_weight(K::FetchCommodity)),
            (AlienMissionKind::Eradicate, K::EradicateAnimals, target_weight(K::EradicateAnimals)),
            (AlienMissionKind::ScanFetch, K::ScanFetch, target_weight(K::ScanFetch)),
            (AlienMissionKind::Balance, K::Balance, if balance.is_some() { w(K::Balance) } else { 0 }),
            (AlienMissionKind::MultiDelivery, K::MultiDelivery, if delivery.is_some() { w(K::MultiDelivery) } else { 0 }),
        ];
        if let Some(k) = force {
            for g in groups.iter_mut() {
                g.2 = u32::from(g.1 == k && g.2 > 0);
            }
        }
        let total: u32 = groups.iter().map(|g| g.2).sum::<u32>().max(1);
        let roll = (emp_idx as u32)
            .wrapping_mul(2_654_435_761)
            .wrapping_add(self.alien_missions_completed.wrapping_mul(40_503))
            % total;
        let mut acc = 0;
        let (kind, retail) = groups
            .iter()
            .find(|g| {
                acc += g.2;
                roll < acc
            })
            .map(|g| (g.0, g.1))
            .unwrap_or((AlienMissionKind::SpecimenCollection, K::FetchAnimal));
        let tier = rules::empire_tier(self.empire_size(emp_idx));
        let rank = self.captain_rank();
        let reward_spice = rules::mission_reward(retail, tier, rank, self.difficulty());
        let mut time_left = rules::mission_duration(retail, tier, rank, self.difficulty());
        let target_of = |kind: K| targeted.iter().find(|(k, _)| *k == kind).map(|(_, world)| *world);
        let planet_name = |world: Option<u32>| {
            world
                .and_then(space_galaxy::world_info)
                .map_or_else(|| "a distant world".to_string(), |i| i.name)
        };
        let mut extra = MissionExtra::default();
        let (target_count, target_world, description) = match kind {
            AlienMissionKind::PirateHunt => (
                3,
                None,
                format!("{emp_name} High Command contracted you to destroy 3 hostile raiders."),
            ),
            AlienMissionKind::SpecimenCollection => (
                1,
                None,
                format!("{emp_name} Bio-Archive requested a living specimen in your cargo hold."),
            ),
            AlienMissionKind::TerraformWorld => {
                let target = space_galaxy::world_id(self.current_system, 1);
                (
                    1,
                    Some(target),
                    format!("{emp_name} Colonists requested terraforming world {target} to T1 or higher."),
                )
            }
            AlienMissionKind::Fetch if retail == K::FetchCommodity => {
                let world = target_of(retail);
                // ADAPTED: retail takes the commodity from the target planet's
                // trade node (0xC70400); the port beams one unit of the spice
                // of that world into the hold, as the other fetch kinds do.
                let color = world.map_or(SpiceColor::default(), SpiceColor::for_world);
                extra.commodity = Some(color);
                extra.item = format!("{} spice", color.label().to_lowercase());
                (
                    1,
                    world,
                    format!(
                        "{emp_name} Command requested {} from {}; take it with the tractor beam and bring it here.",
                        extra.item,
                        planet_name(world)
                    ),
                )
            }
            AlienMissionKind::Fetch => {
                let world = target_of(retail);
                (
                    1,
                    world,
                    format!(
                        "{emp_name} Command requested {} from {}; abduct it with the tractor beam and bring it here.",
                        rules::fetch_item_text(retail),
                        planet_name(world)
                    ),
                )
            }
            AlienMissionKind::Eradicate => {
                let world = target_of(retail);
                // `missionTimerStartsOnFirstEntry` 1: the limit starts when the
                // player enters the target world's system.
                extra.duration = time_left.take();
                (
                    rules::ERADICATE_NUM_CREATURES,
                    world,
                    format!(
                        "{emp_name} High Command asked you to destroy {} diseased animals on {}.",
                        rules::ERADICATE_NUM_CREATURES,
                        planet_name(world)
                    ),
                )
            }
            AlienMissionKind::Balance => (
                rules::BALANCE_ROWS_REQUIRED,
                balance,
                format!(
                    "{emp_name} Colonists asked you to bring the ecosystem of {} into balance.",
                    planet_name(balance)
                ),
            ),
            AlienMissionKind::MultiDelivery => {
                let (buyer, items) = delivery.clone().unwrap_or((0, Vec::new()));
                extra.item = rules::fetch_artifact_item(buyer).to_string();
                extra.buyer_world = Some(buyer);
                extra.item_worlds = items;
                (
                    rules::MULTI_DELIVERY_ITEMS,
                    Some(buyer),
                    format!(
                        "{emp_name} Command asked you to collect {} {} artifacts and deliver them to {}.",
                        rules::MULTI_DELIVERY_ITEMS,
                        extra.item,
                        planet_name(Some(buyer))
                    ),
                )
            }
            AlienMissionKind::ScanFetch => {
                let world = target_of(retail);
                (
                    2,
                    world,
                    format!(
                        "{emp_name} Command asked you to scan {} and bring back one of its specimens.",
                        planet_name(world)
                    ),
                )
            }
        };
        AlienMission {
            empire: emp_idx,
            kind,
            reward_spice,
            progress: 0,
            target_count,
            target_world,
            description,
            retail: Some(retail),
            time_left,
            extra,
        }
    }

    /// The candidates of a scored mission kind within `range` of the active
    /// star (nearest star first, each star's planets in orbit order), the best
    /// score first.
    fn scored_candidates(&self, range: f32, mut score: impl FnMut(u32, &space_galaxy::WorldInfo) -> Option<f32>) -> Vec<(u32, f32)> {
        let Some(galaxy) = space_galaxy::registered_galaxy() else {
            return Vec::new();
        };
        let here = self.current_system;
        let mut candidates: Vec<(u32, f32)> = Vec::new();
        let stars = std::iter::once((here, 0.0)).chain(galaxy.stars_within(here, range));
        for (star, _) in stars {
            if star == space_galaxy::HOME_STAR {
                continue;
            }
            for slot in 0..space_galaxy::WORLDS_PER_SYSTEM {
                let world = space_galaxy::world_id(star, slot);
                let Some(info) = galaxy.world_info(world) else {
                    continue;
                };
                if self.colonies.contains(&world) {
                    continue;
                }
                if let Some(value) = score(world, &info) {
                    candidates.push((world, value));
                }
            }
        }
        // The best score first, the nearest first among equals (the walk is in distance order).
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
        candidates
    }

    /// ADAPTED: one of the nearest best-scored candidates is drawn by the
    /// completed-mission count (retail draws among the scored planets).
    fn draw_candidate(&self, candidates: &[(u32, f32)]) -> Option<u32> {
        if candidates.is_empty() {
            return None;
        }
        let pick = (self.alien_missions_completed as usize) % candidates.len().min(5);
        candidates.get(pick).map(|c| c.0)
    }

    /// The target world of a scored mission of `kind` for the giver `giver`
    /// (`cMission` scoring slot 0x184, VERIFIED 0xC523E0 for the fetch kinds,
    /// 0xC4F210 for eradicate animals): a citizen mission wants a planet of the
    /// tribe / civilization / empire tech level (2 / 4 / 5), an artifact any
    /// planet (an empire's scores 1.2 against 1.0), a commodity an empire planet
    /// (the commodity node, 0xC70400; ADAPTED: the tech level stands for it),
    /// creatures a planet with animals (ADAPTED: tech above none stands for the
    /// animal list), scan-fetch a planet of bullseye T-score 1 with animals; the
    /// planet lies within `missionRangeMax` (6 artifact, 15 the others) of the
    /// star the giver is talked to at. ADAPTED: one of the nearest best-scored
    /// candidates is drawn by the completed-mission count (retail draws among
    /// the scored planets); the player's own worlds, the giver's own worlds (for
    /// a citizen) and the player's home star are never targets.
    pub fn fetch_target(&self, kind: rules::MissionKind, giver: EmpireId) -> Option<u32> {
        let range = rules::mission_row(kind).range_max;
        let citizen = matches!(
            kind,
            rules::MissionKind::FetchTribeCitizen | rules::MissionKind::FetchCivCitizen | rules::MissionKind::FetchEmpireCitizen
        );
        let candidates = self.scored_candidates(range, |world, info| {
            if citizen && self.world_empire(world) == Some(giver) {
                return None;
            }
            rules::kind_target_score(kind, info.tech, info.t_score)
        });
        self.draw_candidate(&candidates)
    }

    /// The target planet of a balance mission (0xC4B830, VERIFIED): a
    /// creature-level planet (`GetTechLevel` 1) of a star without an empire
    /// whose T-score level (0xBBCFB0) is below `missionNumOfRowsRequired`.
    /// ADAPTED: the port's T-score level is the stable food-web tier.
    pub fn balance_target(&self) -> Option<u32> {
        let galaxy = space_galaxy::registered_galaxy()?;
        let owned_star = |star: u32| {
            galaxy.star(star).is_none_or(|s| s.empire.is_some())
                || self.universe.grown.contains_key(&star)
                || self.colonies.iter().any(|w| space_galaxy::system_of_world(*w) == star)
        };
        let candidates = self.scored_candidates(rules::mission_row(rules::MissionKind::Balance).range_max, |world, info| {
            let star = space_galaxy::system_of_world(world);
            (info.tech == crate::space_worldgen::TechLevel::Creature
                && !owned_star(star)
                && (self.ecology_for(world).stable_foodweb_tier() as u32) < rules::BALANCE_ROWS_REQUIRED)
                .then_some(1.0)
        });
        self.draw_candidate(&candidates)
    }

    /// The buyer planet and the planets that hold the items of a multi-delivery
    /// mission (0xC59CF0 picks `missionMultiDeliveryMinNumItems` items for
    /// `missionMultiDeliveryMaxNumBuyers` buyers; a buyer is an Empire-level
    /// planet, 0xC57040). ADAPTED: the items lie on the nearest planets
    /// (retail's choice, 0xC593E0 / 0xC596E0, is not decoded).
    pub fn multi_delivery_plan(&self, giver: EmpireId) -> Option<(u32, Vec<u32>)> {
        let buyer = self.fetch_target(rules::MissionKind::MultiDelivery, giver)?;
        let range = rules::mission_row(rules::MissionKind::MultiDelivery).range_max;
        let mut items: Vec<u32> = self
            .scored_candidates(range, |world, _| (world != buyer).then_some(1.0))
            .into_iter()
            .map(|(world, _)| world)
            .collect();
        items.truncate(rules::MULTI_DELIVERY_ITEMS as usize);
        (items.len() == rules::MULTI_DELIVERY_ITEMS as usize).then_some((buyer, items))
    }

    /// `K` near an alien empire world or owned colony: Space Communications.
    /// At an empire: first contact; turn in a finished mission; pay a pending
    /// tribute; at war, make a peace offer (retail prices); below the trade
    /// warning (3) send the small gift (10,000, +0.5); at 7 accept its
    /// alliance; allied, recruit an escort; take a mission; open a trade
    /// route. At an owned world: colony report.
    pub fn try_communicate(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        if let Some(emp_idx) = self.world_empire(id) {
            let emp_name = Self::empire_name(emp_idx);
            let archetype = empire_archetype(emp_idx);
            let quote = archetype_philosophical_greeting(archetype, emp_name);
            let contact = self.meet_empire(emp_idx);
            let rel = self.relationship(emp_idx);

            // 0. The buyer of a multi-delivery mission takes the items carried
            // (the dialog of 0xC57E90, `action_mission_delivery`).
            if let Some(msg) = self.try_multi_delivery(id, run) {
                return WorldAction::Done(msg);
            }

            // 1. A finished mission from this empire.
            if let Some(mission) = self.active_alien_mission.clone() {
                if mission.empire == emp_idx && mission.is_complete(self) {
                    // The scan sub-mission of a scan-fetch ends here (its row pays nothing,
                    // `scanfetch_scan` money 0): the fetch sub-mission follows on the same
                    // planet (conversation flags Step1 -> Step2).
                    if mission.kind == AlienMissionKind::ScanFetch && mission.extra.stage == 0 {
                        let planet = mission.target_world.and_then(space_galaxy::world_info).map_or_else(|| "the world".to_string(), |i| i.name);
                        if let Some(m) = self.active_alien_mission.as_mut() {
                            m.extra.stage = 1;
                            m.description = format!("{emp_name} Command asked you to bring back one of the specimens of {planet}.");
                        }
                        run.request_save();
                        let msg = format!("The scan of {planet} is delivered to the {emp_name} empire: now bring them one of its specimens");
                        self.message = msg.clone();
                        return WorldAction::Done(msg);
                    }
                    let reward = mission.reward_spice;
                    // 0xC52740 (the fetch class's OnMissionCompleted) takes one unit of
                    // the commodity out of the hold.
                    if let (Some(rules::MissionKind::FetchCommodity), Some(color)) = (mission.retail, mission.extra.commodity) {
                        let left = self.cargo_spice.get(&color).copied().unwrap_or(0).saturating_sub(1);
                        if left == 0 {
                            self.cargo_spice.remove(&color);
                        } else {
                            self.cargo_spice.insert(color, left);
                        }
                    }
                    if matches!(mission.kind, AlienMissionKind::SpecimenCollection | AlienMissionKind::ScanFetch) {
                        // The scan-fetch's fetch sub-mission takes one specimen.
                        let mut remaining = if mission.kind == AlienMissionKind::ScanFetch { 1 } else { mission.target_count };
                        for slot in [
                            &mut self.cargo_wildlife.herbivores,
                            &mut self.cargo_wildlife.carnivores,
                            &mut self.cargo_wildlife.flora,
                        ] {
                            let take = (*slot as u32).min(remaining);
                            *slot -= take as u8;
                            remaining -= take;
                        }
                    }
                    self.active_alien_mission = None;
                    self.alien_missions_completed += 1;
                    self.badge_stats.missions_complete += 1;
                    match mission.kind {
                        AlienMissionKind::SpecimenCollection => self.badge_stats.fetch_missions += 1,
                        AlienMissionKind::PirateHunt => self.badge_stats.eradicate_missions += 1,
                        AlienMissionKind::Fetch => self.badge_stats.fetch_missions += 1,
                        // `scanfetch_fetch` is a fetch class mission (0xC527E8: badge 7).
                        AlienMissionKind::ScanFetch => self.badge_stats.fetch_missions += 1,
                        // 0xC4F2DF: the eradicate badge (8) is added when the creatures
                        // are destroyed, not at the turn-in (see `register_ship_kill`).
                        AlienMissionKind::Eradicate => {}
                        AlienMissionKind::TerraformWorld | AlienMissionKind::Balance | AlienMissionKind::MultiDelivery => {}
                    }
                    run.spice = (run.spice + reward).min(SPICE_CAP);
                    let scale = mission.retail.map_or(3.0, |k| rules::mission_row(k).relation);
                    let news = self.apply_relationship(emp_idx, RelationshipEvent::MissionComplete, scale);
                    run.request_save();
                    let msg = format!(
                        "Mission completed! The {emp_name} empire [{}] paid {reward:.0} Sporebucks for {} ({}; {} missions) {}",
                        archetype.title(),
                        mission.kind.title(),
                        self.relation_label(emp_idx),
                        self.alien_missions_completed,
                        news.unwrap_or_default()
                    );
                    self.message = msg.clone();
                    return WorldAction::Done(msg);
                }
            }

            // 2. A pending tribute demand from this empire.
            if let Some(demand) = self.tribute_demand.filter(|d| d.empire == emp_idx) {
                if run.spice >= demand.amount {
                    run.spice -= demand.amount;
                    self.tribute_demand = None;
                    run.request_save();
                    let msg = format!(
                        "Paid the {emp_name} empire's tribute of {:.0} Sporebucks",
                        demand.amount
                    );
                    self.message = msg.clone();
                    return WorldAction::Done(msg);
                }
            }

            // 3. At war: a peace offer at the retail prices.
            if self.wars.contains(&emp_idx) {
                let price = rules::peace_price(rel);
                let offer = rules::PEACE_OFFER_PRICES
                    .iter()
                    .copied()
                    .find(|p| *p >= price)
                    .unwrap_or(rules::PEACE_OFFER_PRICES[4]);
                if run.spice < offer {
                    return WorldAction::Refused(format!(
                        "The {emp_name} empire [{}] is at war with you: \"{quote}\" Peace costs {offer:.0} Sporebucks (have {:.0})",
                        archetype.title(),
                        run.spice
                    ));
                }
                let verdict = rules::peace_offer(offer, rel);
                if verdict == rules::OfferVerdict::VeryBad {
                    return WorldAction::Refused(format!(
                        "The {emp_name} empire rejects a {offer:.0} peace offer (they ask {price:.0})"
                    ));
                }
                run.spice -= offer;
                let event = if verdict == rules::OfferVerdict::Bad {
                    RelationshipEvent::BadPeaceOffer
                } else {
                    RelationshipEvent::GoodPeaceOffer
                };
                // 0x102CF20 on success: ResetRelationship, DeclarePeace, then
                // the offer's event (as the comm screen's peace offer).
                self.relationships.remove(&emp_idx);
                self.meet_empire(emp_idx);
                self.wars.retain(|&w| w != emp_idx);
                self.apply_relationship(emp_idx, event, 1.0);
                self.wars.retain(|&w| w != emp_idx);
                run.request_save();
                let msg = format!(
                    "Peace with the {emp_name} empire for {offer:.0} Sporebucks ({})",
                    self.relation_label(emp_idx)
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }

            // 4. Low trust: the small gift.
            if rel < rules::TRADE_ROUTE_WARN {
                let gift = rules::GiftSize::Small;
                if run.spice < gift.amount() {
                    return WorldAction::Refused(format!(
                        "Hailed the {emp_name} empire [{}] ({}): \"{quote}\" A gift costs {:.0} Sporebucks (have {:.0})",
                        archetype.title(),
                        self.relation_label(emp_idx),
                        gift.amount(),
                        run.spice
                    ));
                }
                run.spice -= gift.amount();
                let news = self.apply_relationship(emp_idx, RelationshipEvent::GiveGift, gift.relationship_scale());
                run.request_save();
                let msg = format!(
                    "Sent a {:.0} Sporebuck gift to the {emp_name} empire [{}]: {} \"{quote}\" {}{}",
                    gift.amount(),
                    archetype.title(),
                    self.relation_label(emp_idx),
                    contact.unwrap_or_default(),
                    news.unwrap_or_default()
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }

            // 5. The empire offers an alliance at 7 (`createAllianceBase`).
            if rules::offers_alliance(rel) && !self.allies.contains(&emp_idx) {
                self.allies.push(emp_idx);
                self.badge_stats.alliances_formed += 1;
                self.apply_relationship(emp_idx, RelationshipEvent::CreateAlliance, 1.0);
                let msg = if emp_idx == 0 {
                    // Allying with the Grox: -20 with every other empire.
                    self.danced_with_the_devil = true;
                    for other in self.galactic_witnesses().into_iter().filter(|e| *e != 0) {
                        self.apply_relationship(other, RelationshipEvent::AlliedWithGrob, 1.0);
                    }
                    self.allies.retain(|&a| a == 0);
                    "Allied with the GROX! [Dance with the Devil] Every other empire is horrified.".to_string()
                } else {
                    format!(
                        "Formed an alliance with the {emp_name} empire [{}] \"{quote}\"",
                        archetype.title()
                    )
                };
                run.request_save();
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }

            // 6. Allies join the posse (ADAPTED: free, up to the rank's limit).
            let max_fleet = self.max_fleet_size();
            if self.allies.contains(&emp_idx) && self.fleet.len() < max_fleet {
                let tier = rules::empire_tier(self.empire_size(emp_idx));
                let hp = rules::UFO_HEALTH_ALLY[tier] * self.difficulty().pick(rules::DIFFICULTY_ALLY_HEALTH);
                self.fleet.push(EscortShip {
                    name: format!("{emp_name} Escort #{}", self.fleet.len() + 1),
                    empire: Some(emp_idx),
                    hp,
                    max_hp: hp,
                });
                run.request_save();
                let msg = format!(
                    "An allied {emp_name} ship joined your posse ({}/{max_fleet})",
                    self.fleet.len()
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }

            // 7. Take a mission (above `missionThreshold`) and open / report the trade route.
            let mut mission_note = String::new();
            if self.active_alien_mission.is_none() && rules::offers_missions(rel) {
                let mission = self.generate_alien_mission(emp_idx);
                mission_note = format!(
                    " Mission accepted: {} ({:.0} Sporebucks{})",
                    mission.description,
                    mission.reward_spice,
                    mission
                        .time_left
                        .map(|t| format!(", {t:.0} s"))
                        .unwrap_or_default()
                );
                self.active_alien_mission = Some(mission);
                self.apply_relationship(emp_idx, RelationshipEvent::MissionStarted, 1.0);
            } else if let Some(ref m) = self.active_alien_mission {
                if m.empire == emp_idx {
                    mission_note = format!(
                        " Mission in progress: {} ({}/{})",
                        m.kind.title(),
                        m.current_progress(self),
                        m.target_count
                    );
                }
            }
            let opened = !self.trade_routes.contains_key(&id);
            let progress = *self.trade_routes.entry(id).or_insert(0.0);
            push_unique(&mut self.docked, id);
            run.request_save();
            let msg = if progress >= 1.0 {
                format!(
                    "{emp_name} trade route on world {id} reached 100%! Press C to purchase the system for {:.0} Sporebucks.{mission_note}",
                    rules::system_purchase_cost(self.ecology_for(id).t_score())
                )
            } else if opened {
                format!(
                    "Opened a trade route with the {emp_name} empire on world {id}.{mission_note} [{}] \"{quote}\"",
                    archetype.title()
                )
            } else {
                format!(
                    "{emp_name} trade route on world {id}: {:.0}% toward purchase.{mission_note}",
                    progress * 100.0
                )
            };
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.colonies.contains(&id) || id == 0 {
            // The colony's Repair / Recharge tabs (applied to the ship by
            // `space_rules::apply_ship_rules` at the colony price).
            self.repair_request = Some(if id == 0 {
                rules::PlanetOwner::Homeworld
            } else {
                rules::PlanetOwner::Colony
            });
            let color = SpiceColor::for_world(id);
            let stored = self.colony_spice_stored(id);
            let cap = self.colony_spice_capacity(id);
            let msg = format!(
                "Colonial Command (world {id}): {} spice {stored}/{cap} • Next: {}",
                color.label(),
                self.next_action()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        WorldAction::Refused(format!(
            "No civilization on world {id} answers your hail; survey with V or colonise with C"
        ))
    }

    /// Cycle the active planetary tool. The caller binds this to T; applying a
    /// tool is a separate action so planet work is never one-key terraform.
    pub fn cycle_planetary_tool(&mut self) -> PlanetaryTool {
        self.planetary_tool = self.planetary_tool.next();
        self.message = format!(
            "Planetary tool: {} (press Y to apply)",
            self.planetary_tool.label()
        );
        self.planetary_tool
    }

    /// Apply one climate or food-web operation to a surveyed world. Costs are
    /// small adapted spice tuning; each action is saved only on success.
    pub fn try_apply_planetary_tool(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        // The press of a ray is its first use (`TryUse` 0x1050BC0 -> `Use`
        // 0x1050080, no Sporebucks: `UseCost` 0), the same hit as every repeat.
        if self.planetary_tool.is_ray() {
            return self.try_ray_repeat(id, run);
        }
        // `cToolStrategy::Update` 0x1056130: the tool's availability here (the
        // homeworld is closed to every tool whose record does not open it).
        if let Err(refusal) = self.check_gate(crate::space_tool_fx::planetary_tool_id(self.planetary_tool), id) {
            return refusal;
        }
        // ADAPTED: the homeworld's own ecology (the Earth stand-in) takes no
        // seeding or editing, whatever the tool record says.
        if id == 0 {
            return WorldAction::Refused("The homeworld's ecology is protected; choose another surveyed planet".into());
        }
        if self.planetary_tool == PlanetaryTool::StaffOfLife {
            if self.staff_of_life_charges == 0 {
                return WorldAction::Refused(
                    "Staff of Life has 0 charges remaining (journey to Galactic Core to recharge)"
                        .into(),
                );
            }
            self.migrate_ecology();
            let index = self.ensure_ecology(id);
            let mut ecology = self.ecology[index];
            self.staff_of_life_charges = self.staff_of_life_charges.saturating_sub(1);
            ecology.temperature = 0.5;
            ecology.atmosphere = 0.5;
            self.marbles.remove(&id);
            ecology.flora = FLORA_NICHES_PER_TIER * MAX_ECOLOGY_TIER;
            ecology.herbivores = HERBIVORES_PER_TIER * MAX_ECOLOGY_TIER;
            ecology.carnivores = CARNIVORES_PER_TIER * MAX_ECOLOGY_TIER;
            self.ecology[index] = ecology;
            self.mark_terraformed(id, run);
            run.request_save();
            let msg = format!(
                "Staff of Life unleashed on world {id}! Instantly terraformed to perfect T3 paradise ({} charge{} left). {}",
                self.staff_of_life_charges,
                if self.staff_of_life_charges == 1 { "" } else { "s" },
                ecology.summary()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.planetary_tool == PlanetaryTool::Monolith {
            if !self.surveyed.contains(&id) {
                return WorldAction::Refused(format!("Survey world {id} with V first"));
            }
            if self.colonies.contains(&id) {
                return WorldAction::Refused(format!(
                    "World {id} is already your colony; choose an uncolonized world"
                ));
            }
            if self.world_empire(id).is_some() {
                return WorldAction::Refused(format!(
                    "World {id} already hosts a civilized empire"
                ));
            }
            let ecology = self.ecology_for(id);
            if ecology.flora == 0 && ecology.herbivores == 0 && ecology.t_score() == 0 {
                return WorldAction::Refused(format!(
                    "Monolith requires living flora or fauna to uplift on world {id} (raise T-score to T1+ first)"
                ));
            }
            if let Err(refusal) = self.use_consumable(tool::MONOLITH, MONOLITH_COST, "Monolith", run) {
                return refusal;
            }
            let emp_idx = space_galaxy::pick_empire(id);
            let emp_name = space_galaxy::EMPIRES
                .get(emp_idx as usize)
                .map(|e| e.0)
                .unwrap_or("Alien");
            self.uplifted_empires.insert(id, emp_idx);
            self.species_uplifted += 1;
            self.apply_relationship(emp_idx, RelationshipEvent::UpliftedCiv, 1.0);
            let idx = self.ensure_ecology(id);
            self.ecology[idx].colony_count = 1;
            run.request_save();
            let msg = format!(
                "Monolith deployed on world {id}! Primitive species uplifted into the {emp_name} Space Empire! (UpliftedCiv +5)"
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.planetary_tool == PlanetaryTool::PlanetBuster {
            if let Err(refusal) = self.use_consumable(tool::PLANET_BUSTER, PLANET_BUSTER_COST, "Planet Buster", run) {
                return refusal;
            }
            // planetbusterbomb toolRelationshipDelta -20 on the owner.
            if let Some(owner) = self.world_empire(id) {
                self.apply_relationship(owner, RelationshipEvent::BadToolUse, 20.0);
            }
            self.colonies.retain(|&c| c != id);
            self.conquered_worlds.retain(|&c| c != id);
            self.trade_routes.remove(&id);
            self.empire_colony_hp.remove(&id);
            self.colony_spice.remove(&id);
            self.uplifted_empires.remove(&id);
            let idx = self.ensure_ecology(id);
            self.ecology[idx].temperature = 1.0;
            self.ecology[idx].atmosphere = 0.0;
            self.marbles.remove(&id);
            self.ecology[idx].flora = 0;
            self.ecology[idx].herbivores = 0;
            self.ecology[idx].carnivores = 0;
            self.ecology[idx].colony_count = 0;
            self.planets_busted += 1;
            self.city_layouts.remove(&id);
            let is_earth = id == space_galaxy::earth_world_id(space_galaxy::SOL_STAR);
            if is_earth {
                self.destroyed_earth = true;
            }
            let violation_msg = self.violate_galactic_code(run);
            let msg = if is_earth {
                format!(
                    "ACHIEVEMENT UNLOCKED: 'Oh the Humanity!' - You obliterated Earth with the Planet Buster superweapon! Planetary surface incinerated! {violation_msg}"
                )
            } else {
                format!(
                    "PLANET BUSTER DETONATED on world {id}! Planetary surface incinerated! {violation_msg}"
                )
            };
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.planetary_tool == PlanetaryTool::AsteroidCaller {
            if let Err(refusal) = self.use_consumable(tool::ASTEROID, ASTEROID_CALLER_COST, "Asteroid Strike", run) {
                return refusal;
            }
            // `OnHit`: the asteroid's hit velocities roll the planet's marble.
            self.apply_throw_hit(id, tool::ASTEROID);
            if let Some(distress) = self.active_distress {
                if distress.world_id == id && distress.kind == DistressKind::PirateRaid {
                    self.active_distress = None;
                    self.pirate_raids_defended += 1;
                    let bounty = 2_000.0f32;
                    run.spice = (run.spice + bounty).min(SPICE_CAP);
                    run.request_save();
                    let msg = format!(
                        "Orbital Asteroid Strike pulverized pirate raider fleet on world {id}! +{bounty:.0} bounty spice! ({} raids defended)",
                        self.pirate_raids_defended
                    );
                    self.message = msg.clone();
                    return WorldAction::Done(msg);
                }
            }
            if let Some(emp_idx) = self.world_empire(id) {
                // asteroid MinDamage 1000.
                self.damage_empire_colony(id, 1000.0);
                let next_hp = self.empire_defense_hp(id);
                let emp_name = Self::empire_name(emp_idx);
                run.request_save();
                let msg = format!(
                    "Orbital Asteroid Strike bombarded {emp_name} colony on world {id}! Defenses: {next_hp:.0}/{EMPIRE_COLONY_MAX_HP:.0} HP",
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }
            run.request_save();
            let msg = format!("Asteroid Strike bombarded world {id}!");
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.planetary_tool.is_throwable() {
            // `DefaultProjectile` / `DeepSpaceProjectile` terraforming tools: one
            // use takes one ammo (`AmmoUsedPerShot`, bought at the store price
            // when none is carried: the ADAPTED shortcut of `use_consumable`),
            // and the hit adds the tool's velocities to the planet's marble.
            let name = crate::space_tool_fx::planetary_tool_id(self.planetary_tool);
            let price = rules::store_price(name)
                .or_else(|| space_tools::record(name).map(|r| r.buy_cost as f32))
                .unwrap_or(0.0);
            if let Err(refusal) = self.use_consumable(name, price, self.planetary_tool.label(), run) {
                return refusal;
            }
            self.migrate_ecology();
            let index = self.ensure_ecology(id);
            self.apply_throw_hit(id, name);
            if self.ecology[index].t_score() >= 1 {
                self.mark_terraformed(id, run);
            }
            run.request_save();
            let msg = format!("{} hit world {id}: {}", self.planetary_tool.label(), self.ecology[index].summary());
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        if self.planetary_tool == PlanetaryTool::CreatureTweaker {
            self.migrate_ecology();
            let index = self.ensure_ecology(id);
            let mut ecology = self.ecology[index];
            let t = ecology.t_score();
            if t == 0 {
                return WorldAction::Refused(format!(
                    "World {id} is T0; terraform to at least T1 before Creature Tweaker can stabilize native genetics"
                ));
            }
            if let Err(refusal) = self.use_consumable(tool::CREATURE_EDIT, CREATURE_TWEAKER_COST, "Creature Tweaker", run) {
                return refusal;
            }
            let target_flora = (t * FLORA_NICHES_PER_TIER).max(ecology.flora);
            let target_herbivores = (t * HERBIVORES_PER_TIER).max(ecology.herbivores);
            let target_carnivores = (t * CARNIVORES_PER_TIER).max(ecology.carnivores);
            ecology.flora = target_flora;
            ecology.herbivores = target_herbivores;
            ecology.carnivores = target_carnivores;
            self.ecology[index] = ecology;
            run.request_save();
            let msg = format!(
                "Creature Tweaker spliced and completed native food web on world {id}! {}",
                ecology.summary()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        self.migrate_ecology();
        if !self.surveyed.contains(&id) && !self.colonies.contains(&id) {
            return WorldAction::Refused(format!("Survey world {id} with V first"));
        }
        let index = self.ensure_ecology(id);
        let mut ecology = self.ecology[index];
        let (cost, success) = match self.planetary_tool {
            PlanetaryTool::SeedFlora => {
                let max = ecology.t_score().saturating_mul(FLORA_NICHES_PER_TIER);
                if ecology.flora >= max {
                    return WorldAction::Refused(format!(
                        "World {id} needs a higher T-score before it can support another flora niche"
                    ));
                }
                ecology.flora += 1;
                if self.cargo_wildlife.flora > 0 {
                    self.cargo_wildlife.flora -= 1;
                    (0.0, "Seeded cargo flora (free) on")
                } else {
                    (LIFE_SEED_COST, "Seeded flora on")
                }
            }
            PlanetaryTool::SeedHerbivore => {
                let max = (ecology.flora / FLORA_NICHES_PER_TIER)
                    .min(ecology.t_score())
                    .saturating_mul(HERBIVORES_PER_TIER);
                if ecology.herbivores >= max {
                    return WorldAction::Refused(format!(
                        "World {id} needs a complete flora tier before it can support herbivores"
                    ));
                }
                ecology.herbivores += 1;
                if self.cargo_wildlife.herbivores > 0 {
                    self.cargo_wildlife.herbivores -= 1;
                    (0.0, "Seeded cargo herbivores (free) on")
                } else {
                    (LIFE_SEED_COST, "Seeded herbivores on")
                }
            }
            PlanetaryTool::SeedCarnivore => {
                let max = (ecology.herbivores / HERBIVORES_PER_TIER).min(ecology.t_score());
                if ecology.carnivores >= max {
                    return WorldAction::Refused(format!(
                        "World {id} needs a complete herbivore tier before it can support carnivores"
                    ));
                }
                ecology.carnivores += 1;
                if self.cargo_wildlife.carnivores > 0 {
                    self.cargo_wildlife.carnivores -= 1;
                    (0.0, "Seeded cargo carnivores (free) on")
                } else {
                    (LIFE_SEED_COST, "Seeded carnivores on")
                }
            }
            _ => {
                return WorldAction::Refused(format!(
                    "{} cannot progress on world {id}",
                    self.planetary_tool.label()
                ))
            }
        };
        if run.spice < cost {
            return WorldAction::Refused(format!(
                "Need {cost:.0} Sporebucks for {} (have {:.0})",
                self.planetary_tool.label(),
                run.spice
            ));
        }
        run.spice -= cost;
        self.ecology[index] = ecology;
        if ecology.t_score() >= 1 {
            self.mark_terraformed(id, run);
        } else {
            self.terraformed.retain(|&world| world != id);
        }
        run.request_save();
        let msg = format!("{success} world {id}: {}", ecology.summary());
        self.message = msg.clone();
        WorldAction::Done(msg)
    }

    /// `cToolStrategy::OnHit` of a throwing tool on the ground of world `id`:
    /// the tool's ground atmosphere / temperature velocities go to the marble
    /// (0xBBC4B0). The water velocity (0xBBC4E0 adds to the water score, which
    /// the rewrite derives from the climate) is not applied.
    pub fn apply_throw_hit(&mut self, id: u32, name: &str) {
        if let Some(record) = space_tools::record(name) {
            let hit = record.hit_velocity(space_tools::SpaceToolHit::Ground);
            if hit.atmosphere != 0.0 || hit.temperature != 0.0 {
                self.hit_marble(id, hit.atmosphere, hit.temperature);
            }
        }
    }

    /// One use of a ray: the press (`TryUse` 0x1050BC0) and every repeat of the
    /// held button (the hold handler 0x1050500 calls `Use` again every
    /// `AutoFireRate`, 0.1 s) are the same use, and each use is one `OnHit`
    /// of the tool. A ray's `UseCost` is 0, so no Sporebucks are charged (the
    /// caller spends the energy); the tool's availability is the gate of
    /// `cToolStrategy::Update` ([`space_tools::tool_gate`]).
    /// * Happy Ray on an empire's world: a GoodToolUse of `toolRelationshipDelta`
    ///   (VERIFIED 0x105987C).
    /// * The terraforming rays: `OnHit` adds the tool's
    ///   `toolHitGround{Atmosphere,Temperature}Velocity` to the planet's marble
    ///   (VERIFIED data and 0xBBC4B0; see [`crate::space_terraform`]): the
    ///   climate moves when the marble rolls, not at the hit.
    pub fn try_ray_repeat(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        let name = crate::space_tool_fx::planetary_tool_id(self.planetary_tool);
        if let Err(refusal) = self.check_gate(name, id) {
            return refusal;
        }
        // The base `cToolStrategy::WhileFiring` 0x1056880 of every use ends a cloak.
        crate::space_tool_use::break_cloak(self);
        if self.planetary_tool == PlanetaryTool::HappyRay {
            let Some(empire) = self.world_empire(id) else {
                // The beam fires (and spends its energy) on a colony or an empty
                // world; only an empire's city reacts (`OnHit` 0x105987C).
                return WorldAction::Done(self.message.clone());
            };
            // ADAPTED: bought the first time it is used (the store shortcut).
            if let Err(refusal) = self.buy_upgrade(tool::HAPPY_RAY_1, HAPPY_RAY_COST, "Happy Ray", run) {
                return refusal;
            }
            let news = self.apply_relationship(
                empire,
                RelationshipEvent::GoodToolUse,
                rules::tool_use(name).relationship,
            );
            let msg = format!(
                "Happy Ray cheers the {} empire on world {id}: {}{}",
                Self::empire_name(empire),
                self.relation_label(empire),
                news.map(|n| format!(" {n}")).unwrap_or_default()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        let Some(hit) = rules::ray_hit(name) else {
            return WorldAction::Refused(format!("{} does not hold fire", self.planetary_tool.label()));
        };
        // ADAPTED: the rays the old palette priced (Heat Ray / Atmosphere Freeze
        // stand-ins) are bought the first time they are used (the store shortcut
        // of `buy_upgrade`); the retail rays are inventory items with no cost per use.
        let bought = match self.planetary_tool {
            PlanetaryTool::HeatRay => Some((tool::HEAT_RAY, HEAT_RAY_COST, "Heat Ray")),
            PlanetaryTool::AtmosphereFreeze => Some((tool::ICE_STORM, ATMOSPHERE_FREEZE_COST, "Atmosphere Freeze")),
            _ => None,
        };
        if let Some((id_, cost, label)) = bought {
            if let Err(refusal) = self.buy_upgrade(id_, cost, label, run) {
                return refusal;
            }
        }
        self.migrate_ecology();
        let index = self.ensure_ecology(id);
        self.hit_marble(id, hit.atmosphere, hit.temperature);
        // Using a terraforming tool on a world that stands at T1 or better counts
        // for the Terraform mission (as the old press did).
        if self.ecology[index].t_score() >= 1 {
            self.mark_terraformed(id, run);
        }
        let msg = format!("{} on world {id}: {}", self.planetary_tool.label(), self.ecology[index].summary());
        self.message = msg.clone();
        WorldAction::Done(msg)
    }

    /// The gate of `cToolStrategy::Update` 0x1056130 for the retail tool `name`
    /// used over world `id` ([`space_tools::tool_gate`]); the refusal carries the
    /// gate's text.
    fn check_gate(&self, name: &str, id: u32) -> Result<(), WorldAction> {
        let Some(record) = space_tools::record(name) else { return Ok(()) };
        space_tools::tool_gate(record, &self.gate_context(id, name)).map_err(|fail| WorldAction::Refused(fail.text.into()))
    }

    /// What the gate reads of world `id`: the homeworld, the planet's tech
    /// level (an empire's or the player's world is Empire level, the rest
    /// Creature: ADAPTED, the rewrite's planets carry no tech level of their
    /// own), the T-score, animals and the posse.
    fn gate_context(&self, id: u32, tool: &str) -> space_tools::GateContext {
        use space_tools::Tech;
        let ecology = self.ecology_for(id);
        let player = id == 0 || self.colonies.contains(&id);
        // The owner lookup reads the galaxy: only the tools whose rule asks for
        // the planet's tech level need it (a ray is used ten times a second).
        let needs_owner = matches!(tool, "PlanetConversion" | "cashinfusion" | "raiderrally" | "monolith" | "zoo");
        let empire = needs_owner && self.world_empire(id).is_some();
        let tech = if player || empire { Tech::Empire } else { Tech::Creature };
        space_tools::GateContext {
            homeworld: id == 0,
            tech,
            player_star: player,
            t_score: ecology.t_score(),
            has_animals: ecology.herbivores + ecology.carnivores > 0,
            posse: self.fleet.len(),
            trade_route: self.trade_routes.contains_key(&id),
            star_tech: if empire { Tech::Empire } else { Tech::Creature },
            ..Default::default()
        }
    }

    /// The marble body of world `id`, created on the first look (the orbit
    /// zone and the Grox owner are read once: the galaxy lookup is not cheap).
    fn marble_body(&mut self, id: u32) -> &mut MarbleBody {
        if !self.marbles.contains_key(&id) {
            use crate::space_terraform::Zone;
            // ADAPTED: the planet record's orbit flags (0x1000 hot band, 0x2000
            // cold band) are `WorldInfo::hard` plus the generated temperature
            // (the worldgen keeps one `hard` flag for both bands).
            let zone = match space_galaxy::world_info(id) {
                Some(info) if info.hard => {
                    if info.temperature > 0.5 {
                        Zone::Hot
                    } else {
                        Zone::Cold
                    }
                }
                _ => Zone::Habitable,
            };
            let grox = self.world_empire(id) == Some(0);
            self.marbles.insert(id, MarbleBody { zone, grox, ..Default::default() });
        }
        self.marbles.get_mut(&id).expect("just inserted")
    }

    /// `cTerraformingManager::OnHit`: add a hit's atmosphere / temperature
    /// velocities to world `id`'s marble (0xBBC4B0).
    pub fn hit_marble(&mut self, id: u32, atmosphere: f32, temperature: f32) {
        let body = self.marble_body(id);
        body.v_atmosphere += atmosphere;
        body.v_temperature += temperature;
    }

    /// One update of world `id`'s terraform marble (0xC7FC50) with `dt`
    /// seconds: the planet's atmosphere / temperature scores are the marble's
    /// position and the bullseye force, drag and damping move it. Returns
    /// whether the T-score changed (the Terraform mission and the terraformed
    /// list follow it).
    pub fn step_marble(&mut self, id: u32, dt: f32, run: &mut RunProgress) -> bool {
        use crate::space_terraform::{plant_classes, Conditions, Marble};
        let Some(index) = self.ecology_index(id) else { return false };
        let ecology = self.ecology[index];
        let body = *self.marble_body(id);
        let before = ecology.t_score();
        let mut marble = Marble {
            temperature: ecology.temperature,
            atmosphere: ecology.atmosphere,
            v_temperature: body.v_temperature,
            v_atmosphere: body.v_atmosphere,
        };
        let cond = Conditions { tier: before, plants: plant_classes(ecology.flora), zone: body.zone, grox: body.grox };
        marble.step(dt * 1000.0, cond);
        self.ecology[index].temperature = marble.temperature;
        self.ecology[index].atmosphere = marble.atmosphere;
        let body = self.marble_body(id);
        body.v_atmosphere = marble.v_atmosphere;
        body.v_temperature = marble.v_temperature;
        let after = self.ecology[index].t_score();
        if after == before {
            return false;
        }
        if after >= 1 {
            self.mark_terraformed(id, run);
        } else {
            self.terraformed.retain(|&world| world != id);
        }
        true
    }

    /// For capture and test drivers: the planetary tool (a free ray) to hold
    /// and for how many seconds over world `id` to bring its marble towards the
    /// bullseye ([`crate::space_terraform::plan_ray`]).
    pub fn plan_terraform_ray(&mut self, id: u32) -> Option<(PlanetaryTool, u32)> {
        use crate::space_terraform::{plan_ray, plant_classes, Conditions, Marble};
        let index = self.ecology_index(id)?;
        let ecology = self.ecology[index];
        let body = *self.marble_body(id);
        let marble = Marble {
            temperature: ecology.temperature,
            atmosphere: ecology.atmosphere,
            v_temperature: body.v_temperature,
            v_atmosphere: body.v_atmosphere,
        };
        let cond = Conditions { tier: ecology.t_score(), plants: plant_classes(ecology.flora), zone: body.zone, grox: body.grox };
        let (name, seconds) = plan_ray(marble, cond)?;
        let tool = PlanetaryTool::ALL.iter().copied().find(|tool| crate::space_tool_fx::planetary_tool_id(*tool) == name)?;
        Some((tool, seconds))
    }

    /// Whether world `id`'s marble sits well inside the ring of `tier` (the
    /// stop condition of the drivers that push it with rays).
    pub fn terraform_goal_reached(&self, id: u32, tier: u8) -> bool {
        let ecology = self.ecology_for(id);
        let marble = crate::space_terraform::Marble::at(ecology.temperature, ecology.atmosphere);
        ecology.t_score() >= tier && marble.radius() < crate::space_terraform::goal_radius(tier)
    }

    /// Let world `id`'s marble roll for `seconds` in 1/60 s steps (what a
    /// player waiting at the planet gets; used by the capture and test drivers
    /// to skip the wait after holding a ray).
    pub fn coast_marble(&mut self, id: u32, seconds: f32, run: &mut RunProgress) {
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            self.step_marble(id, 1.0 / 60.0, run);
        }
    }

    /// `U` near a planet: Tractor Beam!
    /// 1. Cleanse biohazard eco-disaster outbreaks on this world.
    /// 2. Recover rare galactic artifacts / ancient relics and store them in the ship's vault.
    /// 3. Beam up stored colony spice crates into the Cargo Hold.
    /// 4. Abduct wild plant and animal specimens into the Cargo Hold.
    pub fn try_tractor_beam(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        // 1. Check for active biohazard eco-disaster outbreak.
        if let Some(distress) = self.active_distress {
            if distress.world_id == id && distress.kind == DistressKind::BiohazardOutbreak {
                // `biospherecollapse` pays no money (relation +3 with the owner).
                self.active_distress = None;
                self.biohazards_cleansed += 1;
                run.request_save();
                let msg = format!(
                    "Tractor Beam eradicated biohazard outbreak on world {id}! ({} eco-disasters cleansed)",
                    self.biohazards_cleansed
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }
        }

        // 1b. The target of a `fetch` mission (artifact, citizen or commodity) on
        // its world, and an item of a multi-delivery mission.
        if let Some(msg) = self.try_fetch_abduction(id, run) {
            return WorldAction::Done(msg);
        }
        if let Some(msg) = self.try_multi_delivery_pickup(id, run) {
            return WorldAction::Done(msg);
        }

        // 2. Check for rare galactic relic / artifact.
        if let Some(artifact) = world_artifact(id) {
            if !self.collected_artifacts.contains(&id) {
                push_unique(&mut self.collected_artifacts, id);
                let name = artifact.name.clone();
                let val = artifact.value;
                if let Some(rare) = artifact.rare.as_deref().and_then(crate::space_rares::rare) {
                    // 0x103FC20: found (mRareMap), badge progress when new. The retail
                    // handler pays nothing for a finished group (`spaceTradingRareGroup`
                    // is read only as UI text, 0x104A65B), so no set bonus is paid; the
                    // completion is still recorded for the collections view.
                    self.rares.obtain(rare);
                    let _ = self.rares.newly_completed(rare.group);
                }
                self.artifacts.push(artifact);
                run.request_save();
                let msg = format!(
                    "Tractor Beam recovered rare galactic artifact: '{name}' (worth {val} Sporebucks)! Stored in vault",
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }
        }

        // 2b. A tool artifact lying on the world (`space_tool_artifacts`:
        // the abduct beam on an artifact of item type 2, 0x105A0D0).
        if let Some(msg) = crate::space_tool_artifacts::pick_up(self, id) {
            run.request_save();
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }

        // 3. Check for colony spice pickup.
        let owned = self.colonies.contains(&id) || id == 0;
        if owned {
            let stored = self.colony_spice_stored(id);
            let color = SpiceColor::for_world(id);
            let free_space = self.cargo_room_for_spice(color);
            if free_space > 0 && stored > 0 {
                let take = stored.min(free_space);
                self.colony_spice.insert(id, stored - take);
                *self.cargo_spice.entry(color).or_insert(0) += take;
                run.request_save();
                let msg = format!(
                    "Tractor Beam scooped {take} {} spice into the Cargo Hold ({}/{} slots)",
                    color.label(),
                    self.cargo_slots_used(),
                    self.cargo_capacity_limit()
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            } else if free_space == 0 && stored > 0 {
                return WorldAction::Refused(
                    "Cargo hold is full; sell spice at an empire post with B".into(),
                );
            }
        }

        // 4. Check for wild specimen abduction.
        let eco = self.ecology_for(id);
        let has_wildlife = id == 0 || (eco.flora > 0 || eco.herbivores > 0 || eco.carnivores > 0);
        let kind = if eco.carnivores > 0 && self.cargo_wildlife.carnivores <= self.cargo_wildlife.herbivores {
            2
        } else if (id == 0 || eco.herbivores > 0) && self.cargo_wildlife.herbivores <= self.cargo_wildlife.flora {
            1
        } else {
            0
        };
        let free_space = self.cargo_room_for_wildlife(kind);
        if has_wildlife && free_space > 0 {
            let (kind_label, count) = match kind {
                2 => {
                    self.cargo_wildlife.carnivores += 1;
                    ("Carnivore", self.cargo_wildlife.carnivores)
                }
                1 => {
                    self.cargo_wildlife.herbivores += 1;
                    ("Herbivore", self.cargo_wildlife.herbivores)
                }
                _ => {
                    self.cargo_wildlife.flora += 1;
                    ("Flora", self.cargo_wildlife.flora)
                }
            };
            // Flight 101 fetch (abduct an animal on the home planet). ADAPTED:
            // the drop-off (radius 75) is the abduction itself.
            if kind > 0 && space_galaxy::system_of_world(id) == HOME_STAR {
                self.advance_tutorial(rules::TutorialStep::Flight101FetchAnimal, run);
            }
            run.request_save();
            let msg = format!(
                "Tractor Beam abducted wild {kind_label} specimen into Cargo Hold (holds {count}, {}/{} slots)",
                self.cargo_slots_used(),
                self.cargo_capacity_limit()
            );
            self.message = msg.clone();
            return WorldAction::Done(msg);
        } else if has_wildlife && free_space == 0 {
            return WorldAction::Refused(
                "Cargo hold is full; deploy specimens with Y or sell spice with B".into(),
            );
        }

        if world_artifact(id).is_some() && self.collected_artifacts.contains(&id) {
            WorldAction::Refused(format!(
                "Tractor Beam: artifact on world {id} has already been retrieved"
            ))
        } else {
            WorldAction::Refused(format!(
                "Tractor Beam: no biohazard, artifact, cargo, or wildlife detected on world {id}"
            ))
        }
    }

    /// The abduct beam on the target world of the accepted `fetch` mission
    /// takes the target (the mission then waits for the turn-in at the giver).
    /// A citizen of an empire's planet costs that empire `AbductCitizen`
    /// (0x1030F00 -> ApplyRelationship(owner, player, 0x526E5D4, 1.0), VERIFIED:
    /// nothing is applied for the player's own citizens or while the Soothing
    /// Song plays, 0xC757F0). ADAPTED: the port abducts the mission's target as
    /// a whole when the beam is used over its world (no per-creature beam); a
    /// commodity (cargo type 4, 0xC52B90) is one unit of the target world's
    /// spice put into the hold (retail takes it from the planet's trade node).
    fn try_fetch_abduction(&mut self, id: u32, run: &mut RunProgress) -> Option<String> {
        let (retail, giver, commodity) = {
            let mission = self.active_alien_mission.as_ref()?;
            if mission.kind != AlienMissionKind::Fetch
                || mission.target_world != Some(id)
                || mission.progress >= mission.target_count
            {
                return None;
            }
            (mission.retail, mission.empire, mission.extra.commodity)
        };
        if let Some(color) = commodity.filter(|_| retail == Some(rules::MissionKind::FetchCommodity)) {
            if self.cargo_room_for_spice(color) == 0 {
                return Some("Cargo hold is full; there is no room for the commodity".to_string());
            }
            *self.cargo_spice.entry(color).or_insert(0) += 1;
        }
        if let Some(mission) = self.active_alien_mission.as_mut() {
            mission.progress = mission.target_count;
        }
        let citizen = matches!(
            retail,
            Some(rules::MissionKind::FetchTribeCitizen | rules::MissionKind::FetchCivCitizen | rules::MissionKind::FetchEmpireCitizen)
        );
        let item = match retail {
            Some(kind) if citizen => rules::fetch_item_text(kind).to_string(),
            Some(rules::MissionKind::FetchCommodity) => {
                format!("one unit of {}", self.active_alien_mission.as_ref().map_or(String::new(), |m| m.extra.item.clone()))
            }
            _ => format!("the {}", rules::fetch_artifact_item(id)),
        };
        let mut note = String::new();
        if citizen && self.siren_song_active <= 0.0 {
            if let Some(owner) = self.world_empire(id) {
                if let Some(news) = self.apply_relationship(owner, RelationshipEvent::AbductCitizen, 1.0) {
                    note = format!(" {news}");
                }
            }
        }
        run.request_save();
        let msg = format!(
            "Tractor Beam abducted {item} for the {} empire's mission: take it to them to be paid.{note}",
            Self::empire_name(giver)
        );
        self.message = msg.clone();
        Some(msg)
    }

    /// The beam over a planet that still holds an item of the accepted
    /// multi-delivery mission puts the item into the hold (the mission's
    /// artifact items are cargo type 5, 0xC57E90).
    fn try_multi_delivery_pickup(&mut self, id: u32, run: &mut RunProgress) -> Option<String> {
        let mission = self.active_alien_mission.as_mut()?;
        if mission.kind != AlienMissionKind::MultiDelivery {
            return None;
        }
        let at = mission.extra.item_worlds.iter().position(|w| *w == id)?;
        mission.extra.item_worlds.remove(at);
        mission.extra.carried += 1;
        let msg = format!(
            "Tractor Beam took a {} artifact ({} of {} carried): deliver them to the buyer",
            mission.extra.item,
            mission.extra.carried,
            mission.target_count.saturating_sub(mission.progress)
        );
        run.request_save();
        self.message = msg.clone();
        Some(msg)
    }

    /// Hailing the buyer planet of the accepted multi-delivery mission hands
    /// over the items carried (`action_mission_delivery`; 0xC57040 fails the
    /// mission when a buyer stops being an Empire-level planet).
    pub fn try_multi_delivery(&mut self, id: u32, run: &mut RunProgress) -> Option<String> {
        let mission = self.active_alien_mission.as_mut()?;
        if mission.kind != AlienMissionKind::MultiDelivery || mission.extra.buyer_world != Some(id) || mission.extra.carried == 0 {
            return None;
        }
        mission.progress = (mission.progress + mission.extra.carried).min(mission.target_count);
        mission.extra.carried = 0;
        let (done, need, giver, item) = (mission.progress, mission.target_count, mission.empire, mission.extra.item.clone());
        run.request_save();
        let msg = if done >= need {
            format!("Delivered all the {item} artifacts. Return to the {} empire for your payment.", Self::empire_name(giver))
        } else {
            format!("Delivered {done} of {need} {item} artifacts; {} more are needed.", need - done)
        };
        self.message = msg.clone();
        Some(msg)
    }

    /// The tutorial mission the player is working on ("Free exploration" after).
    pub fn mission_name(&self) -> &'static str {
        self.tutorial.step.title()
    }

    /// Worlds the player has actually scanned. The homeworld (WorldId 0) starts
    /// owned, so it must never satisfy the survey mission's "find new worlds"
    /// objective on its own.
    fn scanned_worlds(&self) -> u32 {
        self.surveyed.iter().filter(|&&id| id != 0).count() as u32
    }

    /// How many objectives the current tutorial mission still needs.
    pub fn mission_progress(&self) -> u32 {
        self.tutorial.remaining()
    }

    /// True once the tutorial chain is done (procedural missions only).
    pub fn missions_finished(&self) -> bool {
        self.tutorial.step == rules::TutorialStep::Done
    }

    /// Count one objective of tutorial step `step` if it is the current one;
    /// a finished step pays its reward and starts the next (its tools on
    /// accept, Story 201 gets its target star from the galaxy on arrival).
    pub fn advance_tutorial(&mut self, step: rules::TutorialStep, run: &mut RunProgress) -> bool {
        if self.tutorial.step != step || step == rules::TutorialStep::Done {
            return false;
        }
        self.tutorial.progress += 1;
        if self.tutorial.progress < step.target() {
            self.message = format!(
                "{}: {}/{}",
                step.title(),
                self.tutorial.progress,
                step.target()
            );
            return true;
        }
        let reward = step.reward_money();
        run.spice = (run.spice + reward).min(SPICE_CAP);
        let next = step.next();
        self.tutorial = rules::TutorialState {
            step: next,
            progress: 0,
            story_star: self.tutorial.story_star,
            story_wreck: self.tutorial.story_wreck,
            // `missionExportTargetPlanetTo`: TutorialColonize hands its target
            // to TutorialTerraform.
            colonize_target: self.tutorial.colonize_target,
            ui_flags: self.tutorial.ui_flags,
            forced: self.tutorial.forced,
        };
        if step == rules::TutorialStep::Flight101EradicateAnimals {
            // ADAPTED: completing Flight 101 grants what declining it would
            // (`spaceDecline101Tools`); its unlock reward list is empty.
            for t in rules::DECLINE_101_TOOLS {
                self.grant_tool(t);
            }
            self.pending_cinematic = Some(rules::CINEMATIC_FLIGHT_SCHOOL_COMPLETE.into());
        }
        if step == rules::TutorialStep::Story201 {
            self.grant_tool(rules::STORY201_REWARD_UNLOCK);
        }
        for t in next.tools_on_accept() {
            self.grant_tool(t);
        }
        if next == rules::TutorialStep::TeachColonize {
            // `TutorialColonize.missionGiveOnAcceptTools`: one Colony Incredi-Pak.
            self.add_charges(tool::PLACE_COLONY, 1);
            // The mission's target planet (0xC4E3B0 scoring, closest star).
            self.tutorial.colonize_target =
                space_galaxy::registered_galaxy().and_then(|g| colonize_mission_target(self, &g));
        }
        self.sync_mission_index();
        run.request_save();
        self.message = if reward > 0.0 {
            format!("Mission complete: {} (+{reward:.0} Sporebucks). Next: {}", step.title(), next.title())
        } else {
            format!("Mission complete: {}. Next: {}", step.title(), next.title())
        };
        true
    }

    /// True when the current mission's target has been met and M can claim it.
    pub fn mission_ready(&self) -> bool {
        self.mission_progress() == 0
    }

    /// Colonies the player founded, excluding the homeworld they start with.
    pub fn new_colony_count(&self) -> u32 {
        self.colonies
            .iter()
            .copied()
            .filter(|&id| id != 0)
            .collect::<std::collections::HashSet<_>>()
            .len() as u32
    }

    /// The tutorial chain is done; this does not finish the Space stage.
    pub fn intro_objectives_complete(&self) -> bool {
        self.missions_finished()
    }

    /// Record a world raised to T1+ (terra-wrangler badge, TutorialTerraform).
    pub fn mark_terraformed(&mut self, id: u32, run: &mut RunProgress) {
        if !self.terraformed.contains(&id) {
            self.terraformed.push(id);
            self.advance_tutorial(rules::TutorialStep::TeachTerraform, run);
        }
    }

    /// Pay `cost` for a permanent store upgrade the first time it is used
    /// (retail upgrades are bought once and kept).
    pub fn buy_upgrade(
        &mut self,
        tool_id: &str,
        cost: f32,
        label: &str,
        run: &mut RunProgress,
    ) -> Result<(), WorldAction> {
        if self.has_tool(tool_id) {
            return Ok(());
        }
        if run.spice < cost {
            return Err(WorldAction::Refused(format!(
                "Need {cost:.0} Sporebucks to buy the {label} (have {:.0})",
                run.spice
            )));
        }
        run.spice -= cost;
        self.grant_tool(tool_id);
        Ok(())
    }

    /// Use one charge of a consumable store tool, buying it at `cost` when
    /// none is carried (ADAPTED shortcut for the comm-screen store).
    pub fn use_consumable(
        &mut self,
        tool_id: &str,
        cost: f32,
        label: &str,
        run: &mut RunProgress,
    ) -> Result<(), WorldAction> {
        if self.take_charge(tool_id) {
            return Ok(());
        }
        if run.spice < cost {
            return Err(WorldAction::Refused(format!(
                "Need {cost:.0} Sporebucks for a {label} (have {:.0})",
                run.spice
            )));
        }
        run.spice -= cost;
        Ok(())
    }

    /// Record an owned, surveyed world (idempotent).
    pub fn own(&mut self, id: u32) {
        push_unique(&mut self.colonies, id);
        push_unique(&mut self.surveyed, id);
        let has_saved_ecology = self.ecology_index(id).is_some();
        let index = self.ensure_ecology(id);
        if !has_saved_ecology && id != 0 {
            // An already-owned world reconstructed from a legacy count or
            // checkpoint keeps a conservative, productive T1 baseline.
            let ecology = &mut self.ecology[index];
            ecology.temperature = 0.35;
            ecology.atmosphere = 0.65;
            ecology.flora = FLORA_NICHES_PER_TIER;
            ecology.herbivores = HERBIVORES_PER_TIER;
            ecology.carnivores = CARNIVORES_PER_TIER;
        }
        if self.ecology[index].colony_count == 0 {
            self.ecology[index].colony_count = 1;
        }
    }

    /// Seed a 0.42-era save, which kept only a colony count. Replays the old
    /// "first worlds in WorldId order" mapping once, and only when no campaign
    /// state exists yet, so a real campaign is never rewritten.
    pub fn reconcile_legacy_colonies(&mut self, legacy_colony_count: u32) {
        self.own(0);
        if self.colonies.len() == 1 && legacy_colony_count > 0 {
            for id in 1..=legacy_colony_count.min(WORLD_COUNT) {
                self.own(id);
            }
            self.message =
                format!("Recovered {legacy_colony_count} colony world(s) from an old save");
        }
    }

    /// Adopt a pre-campaign checkpoint's exact world flags by `WorldId`. This is
    /// the authoritative migration for 0.42/0.43 saves, whose `ObjectCheckpoint`
    /// already recorded `(colonized, surveyed)` per world id.
    pub fn adopt_checkpoint(&mut self, checkpoint: &crate::persistence::Checkpoint) -> bool {
        let mut adopted = false;
        for object in &checkpoint.objects {
            let Some((colonized, surveyed)) = object.planet else {
                continue;
            };
            if colonized && !self.colonies.contains(&object.id) {
                self.own(object.id);
                adopted = true;
            } else if surveyed
                && !self.colonies.contains(&object.id)
                && !self.surveyed.contains(&object.id)
            {
                push_unique(&mut self.surveyed, object.id);
                adopted = true;
            }
        }
        if adopted {
            self.message = "Recovered your world survey and colony records".into();
        }
        adopted
    }

    /// `V` near an un-surveyed world.
    pub fn try_survey(&mut self, id: u32, radius: f32, run: &mut RunProgress) -> WorldAction {
        let system = space_galaxy::system_of_world(id);
        // Flight 101 scan: any scan in the home system (the homeworld counts).
        if self.tutorial.step == rules::TutorialStep::Flight101Scan && system == HOME_STAR {
            self.advance_tutorial(rules::TutorialStep::Flight101Scan, run);
            if self.colonies.contains(&id) || self.surveyed.contains(&id) {
                return WorldAction::Done(self.message.clone());
            }
        }
        // Story 201: scanning a world at the target star: first the UFO
        // wreck, then the ruin on another world (0xC602B0, STORY201_SCANS).
        if self.tutorial.step == rules::TutorialStep::Story201
            && self.tutorial.story_star == Some(system)
            && system != HOME_STAR
        {
            match self.tutorial.story_wreck {
                None => {
                    self.tutorial.story_wreck = Some(id);
                    self.advance_tutorial(rules::TutorialStep::Story201, run);
                    if self.tutorial.step == rules::TutorialStep::Story201 {
                        self.message = format!(
                            "{}: the UFO wreck is scanned; scan the ruin on another world of this star",
                            rules::TutorialStep::Story201.title()
                        );
                    }
                }
                Some(wreck) if wreck != id => {
                    self.advance_tutorial(rules::TutorialStep::Story201, run);
                }
                Some(_) => {}
            }
        }
        if self.colonies.contains(&id) || self.surveyed.contains(&id) {
            return WorldAction::Refused(format!("World {id} is already surveyed"));
        }
        if id == space_galaxy::earth_world_id(space_galaxy::SOL_STAR) && !self.discovered_earth {
            self.discovered_earth = true;
            self.surveyed.push(id);
            run.request_save();
            let msg = "ACHIEVEMENT UNLOCKED: 'Our Own Home'! You discovered Earth in the Sol system!".to_string();
            self.message = msg.clone();
            return WorldAction::Done(msg);
        }
        let _ = survey_bounty(radius);
        self.surveyed.push(id);
        run.request_save();
        self.message = format!(
            "Scanned world {id} ({} worlds scanned)",
            self.scanned_worlds()
        );
        WorldAction::Done(format!("Scanned world {id}"))
    }

    /// Compatibility helper retained for older unit fixtures. Live controls
    /// use `try_apply_planetary_tool`, requiring multiple explicit actions.
    pub fn try_terraform(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        if self.colonies.contains(&id) {
            return WorldAction::Refused(format!("World {id} is already a colony"));
        }
        if !self.surveyed.contains(&id) {
            return WorldAction::Refused(format!("Survey world {id} with V first"));
        }
        if self.terraformed.contains(&id) {
            return WorldAction::Refused(format!("World {id} is already terraformed"));
        }
        let cost = terraform_cost(id);
        if run.spice < cost {
            return WorldAction::Refused(format!(
                "Need {cost:.0} Sporebucks to terraform world {id} (have {:.0})",
                run.spice
            ));
        }
        run.spice -= cost;
        self.mark_terraformed(id, run);
        run.request_save();
        let suitability = terraform_suitability(id);
        self.message = if suitability < TERRAFORM_MIN {
            format!(
                "Terraformed world {id} at a {cost:.0} Sporebuck premium ({suitability:.0}% suitable)"
            )
        } else {
            format!("Terraformed world {id} ({suitability:.0}% suitable)")
        };
        WorldAction::Done(format!("Terraformed world {id} for {cost:.0} Sporebucks"))
    }

    /// `C` near a surveyed world: found an outpost, add a settlement unit
    /// when climate and a stable food web provide capacity, or capture/buy out
    /// an alien empire world.
    pub fn try_colonize(&mut self, id: u32, run: &mut RunProgress) -> WorldAction {
        if id == 0 {
            return WorldAction::Refused(
                "The homeworld is already established; choose another planet".into(),
            );
        }
        // Check if this is an Alien Empire colony (military capture or trade buyout).
        if let Some(emp_idx) = self.world_empire(id) {
            let emp_name = space_galaxy::EMPIRES
                .get(emp_idx as usize)
                .map(|e| e.0)
                .unwrap_or("Alien");
            let hp = self.empire_defense_hp(id);
            if self.empire_colony_surrendered(id) {
                self.apply_relationship(emp_idx, RelationshipEvent::CapturedASystem, 1.0);
                push_unique(&mut self.conquered_worlds, id);
                self.empire_colony_hp.remove(&id);
                self.trade_routes.remove(&id);
                self.own(id);
                self.ensure_city_layouts(id);
                run.colonies = self.new_colony_count();
                run.request_save();
                let msg = format!("Captured {emp_name} colony on world {id}!");
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }
            let maturity = self.trade_routes.get(&id).copied().unwrap_or(0.0);
            if maturity >= 1.0 {
                let price = rules::system_purchase_cost(self.ecology_for(id).t_score());
                if run.spice < price {
                    return WorldAction::Refused(format!(
                        "Need {price:.0} Sporebucks to purchase the {emp_name} system of world {id} (have {:.0})",
                        run.spice
                    ));
                }
                run.spice -= price;
                self.apply_relationship(emp_idx, RelationshipEvent::GoodSystemPurchaseOffer, 1.0);
                self.endgame.systems_purchased += 1;
                self.endgame.purchased_worlds += u32::from(!self.conquered_worlds.contains(&id));
                push_unique(&mut self.conquered_worlds, id);
                self.trade_routes.remove(&id);
                self.own(id);
                let index = self.ensure_ecology(id);
                self.ecology[index].colony_count = 2;
                self.ensure_city_layouts(id);
                run.colonies = self.new_colony_count();
                run.request_save();
                let msg = format!(
                    "Purchased the {emp_name} system of world {id} for {price:.0} Sporebucks!"
                );
                self.message = msg.clone();
                return WorldAction::Done(msg);
            }
            return WorldAction::Refused(format!(
                "World {id} belongs to the {emp_name} empire (defenses {hp:.0}/{EMPIRE_COLONY_MAX_HP:.0}, trade {:.0}%); hail with K or attack with F",
                maturity * 100.0
            ));
        }
        self.migrate_ecology();
        if !self.surveyed.contains(&id) {
            return WorldAction::Refused(format!("Survey world {id} with V first"));
        }
        if !self.has_tool(tool::INTERPLANETARY_DRIVE) {
            return WorldAction::Refused(
                "Colonizing needs the Interplanetary Drive (finish Flight 101)".into(),
            );
        }
        let colony_price = rules::store_price(tool::PLACE_COLONY).unwrap_or(150_000.0);
        if self.charges(tool::PLACE_COLONY) == 0 && run.spice < colony_price {
            return WorldAction::Refused(format!(
                "Need a Colony Incredi-Pak ({colony_price:.0} Sporebucks; have {:.0})",
                run.spice
            ));
        }
        let index = self.ensure_ecology(id);
        let capacity = self.ecology[index].colony_capacity();
        if self.ecology[index].colony_count >= capacity {
            let ecology = self.ecology[index];
            return WorldAction::Refused(format!(
                "World {id} is at colony capacity ({capacity}); raise T-score and stabilize its food web ({})",
                ecology.summary()
            ));
        }
        let paid = if self.take_charge(tool::PLACE_COLONY) {
            0.0
        } else {
            run.spice -= colony_price;
            colony_price
        };
        self.ecology[index].colony_count += 1;
        self.ensure_city_layouts(id);
        let new_world = !self.colonies.contains(&id);
        if new_world {
            self.own(id);
        }
        run.colonies = self.new_colony_count();
        if new_world {
            self.advance_tutorial(rules::TutorialStep::TeachColonize, run);
        }
        run.request_save();
        let ecology = self.ecology[index];
        let how = if paid > 0.0 {
            format!("for {paid:.0} Sporebucks")
        } else {
            "with a Colony Incredi-Pak".to_string()
        };
        self.message = if new_world {
            format!("Founded a colony on world {id} {how}. {}", ecology.summary())
        } else {
            format!(
                "Expanded world {id} to {} settlement(s) {how}. {}",
                ecology.colony_count,
                ecology.summary()
            )
        };
        WorldAction::Done(self.message.clone())
    }

    pub fn colony_count(&self, world_id: u32) -> u8 {
        self.ecology_for(world_id).colony_count
    }

    pub fn ensure_city_layouts(&mut self, world_id: u32) -> &mut Vec<CityLayout> {
        let count = self.colony_count(world_id).max(1) as usize;
        let max = self.ecology_for(world_id).max_buildings();
        let layouts = self.city_layouts.entry(world_id).or_default();
        while layouts.len() < count {
            layouts.push(CityLayout::new());
        }
        for layout in layouts.iter_mut() {
            if layout.capacity() != max || layout.capacity == 0 {
                layout.fit_capacity(max);
            }
        }
        layouts
    }

    /// Fit every city to its planet's building limit (the planet's T-score
    /// changed). Returns the buildings the limit removed.
    pub fn fit_city_layouts(&mut self) -> usize {
        let worlds: Vec<u32> = self.city_layouts.keys().copied().collect();
        let mut removed = 0;
        for world in worlds {
            let max = self.ecology_for(world).max_buildings();
            if let Some(layouts) = self.city_layouts.get_mut(&world) {
                for layout in layouts.iter_mut() {
                    if layout.capacity() != max || layout.capacity == 0 {
                        removed += layout.fit_capacity(max);
                    }
                }
            }
        }
        removed
    }

    pub fn city_layout(&self, world_id: u32, city_idx: usize) -> Option<&CityLayout> {
        self.city_layouts.get(&world_id).and_then(|v| v.get(city_idx))
    }

    pub fn total_defense_turrets(&self, world_id: u32) -> usize {
        self.city_layouts
            .get(&world_id)
            .map(|cities| cities.iter().map(|c| c.turrets()).sum())
            .unwrap_or(0)
    }

    pub fn build_city_structure(
        &mut self,
        world_id: u32,
        city_idx: usize,
        slot_idx: usize,
        building: CityBuilding,
        run: &mut RunProgress,
    ) -> WorldAction {
        let owned = self.colonies.contains(&world_id) || world_id == 0;
        if !owned {
            return WorldAction::Refused(format!("World {world_id} is not colonized by your empire"));
        }
        let count = self.colony_count(world_id).max(1) as usize;
        if city_idx >= count {
            return WorldAction::Refused(format!("World {world_id} only has {count} city/cities"));
        }
        // The city's building limit (`GetMaxBuildings` 0xBBE760): a replaced
        // building does not count twice.
        self.ensure_city_layouts(world_id);
        let max = self.ecology_for(world_id).max_buildings();
        {
            let layout = &self.city_layouts[&world_id][city_idx];
            if slot_idx >= layout.capacity() || slot_idx >= layout.slots.len() {
                return WorldAction::Refused(format!(
                    "Invalid building slot {slot_idx}: a city on world {world_id} holds {max} building(s) (T{}; the limit is 1 / 6 / 12 / 20 at T0 / T1 / T2 / T3)",
                    self.ecology_for(world_id).stable_foodweb_tier()
                ));
            }
        }
        let cost = building.cost();
        if run.spice < cost {
            return WorldAction::Refused(format!(
                "Need {cost:.0} Sporebucks to construct {} (have {:.0})",
                building.name(),
                run.spice
            ));
        }
        self.ensure_city_layouts(world_id);
        let layout = &mut self.city_layouts.get_mut(&world_id).unwrap()[city_idx];
        let old = layout.slots[slot_idx];
        let mut refund = 0.0;
        if let Some(prev) = old {
            refund = prev.refund();
        }
        run.spice = (run.spice - cost + refund).clamp(0.0, SPICE_CAP);
        layout.slots[slot_idx] = Some(building);
        run.request_save();
        let summary = layout.summary();
        let msg = format!(
            "Built {} in city {} slot {} on world {world_id} for {cost:.0} Sporebucks{}. {summary}",
            building.name(),
            city_idx + 1,
            slot_idx + 1,
            if refund > 0.0 { format!(" (refunded {refund:.0} for replaced structure)") } else { String::new() },
        );
        self.message = msg.clone();
        WorldAction::Done(msg)
    }

    pub fn demolish_city_structure(
        &mut self,
        world_id: u32,
        city_idx: usize,
        slot_idx: usize,
        run: &mut RunProgress,
    ) -> WorldAction {
        let owned = self.colonies.contains(&world_id) || world_id == 0;
        if !owned {
            return WorldAction::Refused(format!("World {world_id} is not colonized by your empire"));
        }
        let count = self.colony_count(world_id).max(1) as usize;
        if city_idx >= count {
            return WorldAction::Refused(format!("World {world_id} only has {count} city/cities"));
        }
        self.ensure_city_layouts(world_id);
        let layout = &mut self.city_layouts.get_mut(&world_id).unwrap()[city_idx];
        if slot_idx >= layout.slots.len() {
            return WorldAction::Refused(format!("Invalid building slot {slot_idx} (the city has {})", layout.slots.len()));
        }
        let old = layout.slots[slot_idx];
        if old.is_none() {
            return WorldAction::Refused(format!("City {} slot {} is already vacant", city_idx + 1, slot_idx + 1));
        }
        let prev = old.unwrap();
        let refund = prev.refund();
        run.spice = (run.spice + refund).min(SPICE_CAP);
        layout.slots[slot_idx] = None;
        run.request_save();
        let summary = layout.summary();
        let msg = format!(
            "Demolished {} in city {} slot {} on world {world_id}: salvaged {refund:.0} Sporebucks. {summary}",
            prev.name(),
            city_idx + 1,
            slot_idx + 1,
        );
        self.message = msg.clone();
        WorldAction::Done(msg)
    }

    /// `M` next to the homeworld: talk to the homeworld about the tutorial.
    /// Retail tutorial missions complete by themselves (rewards are paid
    /// when the last objective is met); this reports the current one and
    /// counts the Flight 101 city visit (ADAPTED: one homeworld visit).
    pub fn try_claim_mission(&mut self, near_home: bool, run: &mut RunProgress) -> WorldAction {
        if !near_home {
            return WorldAction::Refused("Your homeworld gives the tutorial missions".into());
        }
        if self.missions_finished() {
            return WorldAction::Refused(
                "No tutorial missions left: hail empires (K) for missions".into(),
            );
        }
        if self.advance_tutorial(rules::TutorialStep::Flight101VisitCities, run) {
            return WorldAction::Done(self.message.clone());
        }
        WorldAction::Refused(format!(
            "{}: {} ({} left)",
            self.mission_name(),
            self.next_action(),
            self.mission_progress()
        ))
    }

    /// A hostile ship was destroyed: a retail money drop by chance (raider
    /// 0.25, amount by tier), tutorial / mission credit, once per ship.
    pub fn register_kill(&mut self, run: &mut RunProgress) {
        self.register_ship_kill(rules::LootShip::Raider, rules::pirate_tier(self.captain_rank()), run);
    }

    /// `register_kill` for a ship type and tier. The loot rolls are a
    /// deterministic hash of the kill count (OpenSpore has no shared RNG here).
    pub fn register_ship_kill(&mut self, ship: rules::LootShip, tier: usize, run: &mut RunProgress) {
        self.kills += 1;
        let hash = self.kills.wrapping_mul(2_654_435_761) ^ 0x9E37_79B9;
        let chance_roll = (hash % 1000) as f32 / 1000.0;
        let amount_roll = ((hash / 1000) % 1000) as f32 / 1000.0;
        let loot = rules::ship_loot(ship, tier, chance_roll, amount_roll).unwrap_or(0.0);
        let bonus = 0.0f32;
        let mut raid_defended = false;
        if let Some(distress) = self.active_distress {
            if distress.kind == DistressKind::PirateRaid
                && distress.system_id == self.current_system
            {
                self.active_distress = None;
                self.pirate_raids_defended += 1;
                raid_defended = true;
            }
        }
        self.advance_tutorial(rules::TutorialStep::Flight101EradicateAnimals, run);
        let mut mission_complete_alert = false;
        if let Some(ref mut mission) = self.active_alien_mission {
            if mission.kind == AlienMissionKind::PirateHunt {
                mission.progress = (mission.progress + 1).min(mission.target_count);
                if mission.progress >= mission.target_count {
                    mission_complete_alert = true;
                }
            }
            // The creatures of an eradicate mission (message 0x1622184 on the active
            // planet, 0xC50870); 0xC4F290 adds the eradicate badge when the last one
            // falls. ADAPTED: the hostile ships placed at the target stand in for the
            // diseased animals (the port has no creatures on a planet).
            if mission.kind == AlienMissionKind::Eradicate
                && mission.extra.spawned
                && mission.progress < mission.target_count
                && mission.target_world.is_some_and(|w| space_galaxy::system_of_world(w) == self.current_system)
            {
                mission.progress += 1;
                if mission.progress >= mission.target_count {
                    mission_complete_alert = true;
                    self.badge_stats.eradicate_missions += 1;
                }
            }
        }
        let total_reward = loot + bonus;
        run.spice = (run.spice + total_reward).min(SPICE_CAP);
        run.request_save();
        let mission_tag = if mission_complete_alert {
            " [CONTRACT COMPLETE: Hail empire to collect reward!]"
        } else {
            ""
        };
        self.message = if raid_defended {
            format!(
                "Colony pirate raid defended! +{total_reward:.0} Sporebucks ({} raids defended, {} kills){mission_tag}",
                self.pirate_raids_defended, self.kills
            )
        } else if total_reward > 0.0 {
            format!(
                "Ship destroyed: it dropped {total_reward:.0} Sporebucks ({} kills){mission_tag}",
                self.kills
            )
        } else {
            format!("Ship destroyed ({} kills){mission_tag}", self.kills)
        };
    }

    /// Record a player action's feedback so it reaches the HUD, not only the
    /// log. Refusals are as loud as successes.
    pub fn apply(&mut self, action: WorldAction) {
        let (WorldAction::Done(msg) | WorldAction::Refused(msg)) = action;
        info!("{msg}");
        self.message = msg;
    }

    /// The concrete next step, with the remaining count for counted missions.
    pub fn next_action(&self) -> String {
        use rules::TutorialStep as T;
        let t = &self.tutorial;
        match t.step {
            T::Flight101VisitCities => "Visit your homeworld's cities: press M at the homeworld".into(),
            T::Flight101Scan => "Scan with V near your homeworld".into(),
            T::Flight101FetchAnimal => "Abduct an animal with the Tractor Beam (U) at your homeworld".into(),
            T::Flight101EradicateAnimals => format!(
                "Destroy hostile ships with F ({}/{})",
                t.progress,
                T::Flight101EradicateAnimals.target()
            ),
            T::Story201 => match t.story_star {
                Some(star) if star != self.current_system => {
                    format!("Jump to the closest star (#{star}) on the galaxy map")
                }
                Some(_) if t.story_wreck.is_none() => "Scan the UFO wreck on a world of this system with V".into(),
                Some(_) => "Scan the ruin on another world of this system with V".into(),
                None => "Open the galaxy map and jump to the closest star".into(),
            },
            T::TeachColonize => format!(
                "Colonize a planet with C ({} Colony Incredi-Pak)",
                self.charges(tool::PLACE_COLONY)
            ),
            T::TeachTerraform => "Raise a planet to T1 with the planetary tools (T, Y)".into(),
            T::TeachTradeSpice => "Collect colony spice (B at a colony) and sell it to an empire (B)".into(),
            T::Done => "Explore, take missions from empires (K), terraform and expand your empire.".into(),
        }
    }

    /// Dock at an owned world and log the trade stop once per world (no refuel
    /// and no repair of the posse: see [`Self::tick`]).
    pub fn dock(&mut self, id: u32) -> Option<String> {
        let fresh = !self.docked.contains(&id);
        push_unique(&mut self.docked, id);
        if fresh && self.docked.len() as u32 == TRADE_TARGET {
            Some("All trade routes opened".into())
        } else {
            None
        }
    }

    /// Retail flight inside a system costs no energy (only tools and jumps
    /// spend it); kept for callers.
    pub fn spend_travel(&mut self, _distance: f32) {}

    /// One interstellar jump (0x1002040): 5 energy, or 200 hull damage
    /// (owed to the ship through `pending_damage`) when the reactor is empty.
    pub fn jump(&mut self) -> (f32, f32) {
        let (energy, damage) = rules::jump_cost(self.energy);
        self.energy = (self.energy - energy).max(0.0);
        self.pending_damage += damage;
        (energy, damage)
    }

    /// Pay for an interstellar jump (checked by `space_galaxy::check_jump`).
    pub fn spend_jump(&mut self, cost: f32) {
        let max_e = self.max_reactor_energy();
        self.energy = (self.energy - cost.max(0.0)).clamp(0.0, max_e);
    }

    /// Move a campaign saved in the prototype galaxy to the retail galaxy
    /// (`GALAXY_VERSION`). Star indices and foreign WorldIds named stars of
    /// the old layout and mean nothing now: the ship returns to the home
    /// system and every record of a foreign world is dropped; home-system
    /// worlds keep their ids while the new home system still has them
    /// (`home_worlds` counts them). Returns true when anything changed.
    pub fn migrate_galaxy(&mut self, home_worlds: u32) -> bool {
        if self.galaxy_version >= GALAXY_VERSION {
            return false;
        }
        self.galaxy_version = GALAXY_VERSION;
        let keep = |id: u32| id < home_worlds;
        let before = (self.colonies.len(), self.surveyed.len(), self.visited.len());
        self.surveyed.retain(|id| keep(*id));
        self.terraformed.retain(|id| keep(*id));
        self.colonies.retain(|id| keep(*id));
        self.docked.retain(|id| keep(*id));
        self.ecology.retain(|e| keep(e.world_id));
        self.colony_spice.retain(|id, _| keep(*id));
        self.colony_spice_progress.retain(|id, _| keep(*id));
        self.trade_routes.retain(|id, _| keep(*id));
        self.empire_colony_hp.retain(|id, _| keep(*id));
        self.conquered_worlds.retain(|id| keep(*id));
        self.uplifted_empires.retain(|id, _| keep(*id));
        self.city_layouts.retain(|id, _| keep(*id));
        self.collected_artifacts.retain(|id| keep(*id));
        self.artifacts.retain(|a| keep(a.world_id));
        self.visited.retain(|s| *s == HOME_STAR);
        let left_home = self.current_system != HOME_STAR;
        self.current_system = HOME_STAR;
        self.current_system_empire = None;
        self.active_distress = None;
        if self
            .active_alien_mission
            .as_ref()
            .is_some_and(|m| m.target_world.is_some_and(|w| !keep(w)))
        {
            self.active_alien_mission = None;
        }
        self.tutorial.story_star = None;
        self.tutorial.story_wreck = None;
        left_home || before != (self.colonies.len(), self.surveyed.len(), self.visited.len())
    }

    /// Arrive in star system `system`: it becomes the saved current system.
    pub fn arrive(&mut self, system: u32) {
        self.current_system = system;
        push_unique(&mut self.visited, system);
    }

    /// Empires with a star within the detection radius of the arrival star (5
    /// units, the Grox 8: S3C) notice the visit (`visited a star near the
    /// empire`, +1 awareness); the star's own empire is handled by
    /// `on_arrival`.
    pub fn on_arrival_near(&mut self, near: &[EmpireId]) {
        for &emp in near {
            if Some(emp) == self.current_system_empire {
                continue;
            }
            let a = self.awareness.entry(emp).or_insert(0.0);
            *a = rules::bump_awareness(
                *a,
                rules::AWARENESS_VISITED_STAR_NEAR,
                rules::Difficulty::from_index(self.difficulty),
            );
        }
    }

    /// Arrival bookkeeping with the galaxy's view of the star: meet its
    /// empire and raise that empire's awareness (visited its star +1), the
    /// galactic core's one-time reward (0x1002DD0), Story 201's star.
    pub fn on_arrival(&mut self, empire: Option<EmpireId>, galactic_core: bool) -> Vec<String> {
        let mut news = Vec::new();
        if let Some(emp) = empire {
            news.extend(self.meet_empire(emp));
            let a = self.awareness.entry(emp).or_insert(0.0);
            *a = rules::bump_awareness(*a, rules::AWARENESS_VISITED_STAR, rules::Difficulty::from_index(self.difficulty));
        }
        // 0x1009C90 -> 0x1002DD0 (once, flag 0x0681C28E): full health and
        // energy, then `SPG_CenterOfGalaxy`; Steve's gift comes from the
        // cinematic (`space_endgame`, message 0x06526395).
        if galactic_core && !self.reached_core {
            self.reached_core = true;
            self.pending_restore = true;
            self.pending_cinematic = Some(rules::CINEMATIC_CENTER_OF_GALAXY.into());
            self.endgame.steve_pending = !self.endgame.met_steve;
            news.push("Entered the Galactic Core.".to_string());
        }
        news
    }

    /// The ship was destroyed (0xC3C680): the posse / context cinematic,
    /// then the UFO is restored (no game over). ADAPTED: the respawn place is
    /// not traced; the caller moves it to the homeworld, full health and energy.
    pub fn on_ship_destroyed(&mut self, in_galaxy: bool) {
        self.ships_lost += 1;
        let cinematic = rules::destroyed_cinematic(!self.fleet.is_empty(), in_galaxy);
        self.pending_cinematic = Some(cinematic.into());
        self.pending_restore = true;
        self.pending_damage = 0.0;
        self.message = "Your UFO was destroyed! It has been rebuilt at your homeworld.".into();
    }

    /// An NPC demands tribute (`npcTributeAmount` x the multiplier of the
    /// player's rank, 0x10407E0; 300 s).
    pub fn demand_tribute(&mut self, empire: EmpireId) {
        let rank = usize::from(self.captain_rank());
        self.tribute_demand = Some(TributeDemand {
            empire,
            amount: rules::tribute_amount(rank, self.difficulty()),
            time_left: rules::NPC_TRIBUTE_TIME_LIMIT,
        });
    }

    /// The universe tick: cooldowns, trade-route progress (`tradeCapturePerMS`),
    /// mission time limits, tribute deadlines, the 60 s relationship decay,
    /// pirate raids and bio-disasters on the retail timers, and colony spice
    /// production into capped storage. There is no passive energy recharge
    /// and no money income (retail colonies only produce spice). `docked` and
    /// `spice` are kept for callers; docking repairs escorts.
    pub fn tick(&mut self, dt: f32, docked: bool, spice: &mut f32, request_save: &mut bool) {
        let _ = spice;
        if self.superweapon_cooldown > 0.0 {
            self.superweapon_cooldown = (self.superweapon_cooldown - dt).max(0.0);
        }
        if self.siren_song_active > 0.0 {
            self.siren_song_active = (self.siren_song_active - dt).max(0.0);
        }
        if self.static_cling_active > 0.0 {
            self.static_cling_active = (self.static_cling_active - dt).max(0.0);
        }
        // The summoned backup ship leaves when `callforbackup` is ready again
        // (0x1000C60, `space_tool_use::expire_backup`).
        if let Some(news) = crate::space_tool_use::expire_backup(self) {
            self.message = news;
            *request_save = true;
        }
        // The escorts do not heal by docking: nothing in the installed tuning
        // (`SpaceCombat` has only the `UFOHealthAlly` values) or in the combat
        // code gives an ally ship a regeneration; the retail ways to mend the
        // posse are the tools (`aoe_repair`, 0x1057FF0). The earlier 25 hp / s
        // while docked had no retail source and is gone.
        let _ = docked;
        let gain = rules::trade_capture_progress(dt);
        for progress in self.trade_routes.values_mut() {
            let before = *progress;
            *progress = (*progress + gain).min(1.0);
            if (*progress - before).abs() > 1e-5 {
                *request_save = true;
            }
        }
        self.universe_clock += dt;

        // A planet whose T-score fell holds fewer buildings per city
        // (`GetMaxBuildings` 0xBBE760, the trim of 0xBBE8B0).
        let trimmed = self.fit_city_layouts();
        if trimmed > 0 {
            self.message = format!("The terraform level fell: {trimmed} colony building(s) were lost");
            *request_save = true;
        }

        // Mission time limits (`missionDuration`): failing costs the
        // relationship (`missionFailPunishmentRelation` -1.5).
        let expired = match self.active_alien_mission.as_mut() {
            Some(m) => match m.time_left.as_mut() {
                Some(t) => {
                    *t -= dt;
                    *t <= 0.0
                }
                None => false,
            },
            None => false,
        };
        if expired {
            if let Some(m) = self.active_alien_mission.take() {
                self.apply_relationship(m.empire, RelationshipEvent::MissionFailed, -rules::MISSION_FAIL_RELATION);
                self.message = format!(
                    "Mission failed: the {} empire's {} ran out of time",
                    Self::empire_name(m.empire),
                    m.kind.title()
                );
                *request_save = true;
            }
        }

        // Tribute deadline (`npdTributeTimeLimit` 300 s): withholding it
        // applies `WitholdTribute`.
        if let Some(mut demand) = self.tribute_demand {
            demand.time_left -= dt;
            if demand.time_left <= 0.0 {
                self.tribute_demand = None;
                let news = self.apply_relationship(demand.empire, RelationshipEvent::WitholdTribute, 1.0);
                self.message = format!(
                    "You withheld the {} empire's tribute. {}",
                    Self::empire_name(demand.empire),
                    news.unwrap_or_default()
                );
                *request_save = true;
            } else {
                self.tribute_demand = Some(demand);
            }
        }

        // Relationship decay (0xD06070), one tick per 60 s.
        self.relationship_clock += dt;
        while self.relationship_clock >= rules::RELATIONSHIP_DECAY_PERIOD {
            self.relationship_clock -= rules::RELATIONSHIP_DECAY_PERIOD;
            let difficulty = self.difficulty();
            let empires: Vec<EmpireId> = self.relationships.keys().copied().collect();
            for emp in empires {
                let before = self.relationship(emp);
                if let Some(rel) = self.relationships.get_mut(&emp) {
                    rel.decay_tick(Self::personality(emp), difficulty);
                }
                if let Some(news) = self.update_stance_from(emp, before) {
                    self.message = news;
                }
            }
            *request_save = true;
        }

        // Emergencies: pirate raids (first after 600 s, then every 1200 s;
        // every 3600 s with the civ-military card, which multiplies the
        // frequency) and bio-disasters (first after 1200 s, then every 600 s)
        // on the player's colonies.
        if let Some(ref mut distress) = self.active_distress {
            distress.time_remaining -= dt;
            if distress.time_remaining <= 0.0 {
                let expired = self.active_distress.take().unwrap();
                *request_save = true;
                match expired.kind {
                    DistressKind::PirateRaid => {
                        // Pirates steal 0.025 spice per second for the raid.
                        let stolen = (rules::PIRATE_STOLEN_SPICE_PER_SECOND * rules::RAID_DURATION).round() as u8;
                        let stored = self.colony_spice_stored(expired.world_id);
                        self.colony_spice.insert(expired.world_id, stored.saturating_sub(stolen));
                        self.message = format!(
                            "SOS EXPIRED: Pirates raided colony on world {} and looted spice!",
                            expired.world_id
                        );
                    }
                    DistressKind::BiohazardOutbreak => {
                        if let Some(idx) = self.ecology_index(expired.world_id) {
                            self.ecology[idx].herbivores =
                                self.ecology[idx].herbivores.saturating_sub(1);
                            self.ecology[idx].carnivores =
                                self.ecology[idx].carnivores.saturating_sub(1);
                        }
                        self.message = format!(
                            "SOS EXPIRED: Biohazard outbreak wiped out species on world {}!",
                            expired.world_id
                        );
                    }
                }
            }
        } else if let Some(target_world) = self.colonies.iter().copied().find(|&w| w != 0) {
            let kind = if self.universe_clock >= self.next_pirate_raid {
                self.next_pirate_raid =
                    self.universe_clock + rules::pirate_raid_interval(false, self.superpower_flags);
                Some(DistressKind::PirateRaid)
            } else if self.universe_clock >= self.next_bio_disaster {
                self.next_bio_disaster = self.universe_clock
                    + rules::biosphere_collapse_interval(false, self.superpower_flags);
                Some(DistressKind::BiohazardOutbreak)
            } else {
                None
            };
            if let Some(kind) = kind {
                let target_sys = space_galaxy::system_of_world(target_world);
                // Raids last `RAID_DURATION` (300 s); a biosphere collapse
                // kills its animals after `missionBiosphereCollapseAnimalTime` 240 s.
                let time_remaining = match kind {
                    DistressKind::PirateRaid => rules::RAID_DURATION,
                    DistressKind::BiohazardOutbreak => 240.0,
                };
                self.active_distress = Some(DistressSignal {
                    system_id: target_sys,
                    world_id: target_world,
                    kind,
                    time_remaining,
                });
                *request_save = true;
                self.message = match kind {
                    DistressKind::PirateRaid => format!(
                        "EMERGENCY SOS: Pirate raid attacking colony on world {target_world} (system {target_sys})! Press G for Galaxy map"
                    ),
                    DistressKind::BiohazardOutbreak => format!(
                        "ECO-DISASTER ALERT: Biohazard outbreak on world {target_world} (system {target_sys})! Use Tractor Beam (U) to cleanse"
                    ),
                };
            }
        }

        // Colony spice production (0xC720A0) into storage capped at 5.
        let owned: Vec<u32> = self.colonies.clone();
        let difficulty = self.difficulty();
        for world_id in owned {
            let cap = self.colony_spice_capacity(world_id);
            let stored = self.colony_spice_stored(world_id);
            if stored >= cap {
                self.colony_spice_progress.remove(&world_id);
                continue;
            }
            // n = the planet's cities (0xC721D0: the planet record's city
            // list; x 4.5 / n beyond four).
            let rate = rules::colony_spice_rate(
                self.colony_base_production(world_id),
                world_id == 0,
                u32::from(self.colony_count(world_id).max(1)),
                self.superpower_flags,
                difficulty,
                rules::SPICE_STAR_MODIFIERS[0],
            );
            let progress = self.colony_spice_progress.entry(world_id).or_insert(0.0);
            *progress += rate * dt;
            if *progress >= 1.0 {
                let units = (*progress).floor();
                *progress -= units;
                let after = (stored as f32 + units).min(cap as f32) as u8;
                self.colony_spice.insert(world_id, after);
                *request_save = true;
            }
        }
    }

    /// A colony's base production (0xBF70E0, VERIFIED):
    /// `BASE_PRODUCTION_PER_FACTORY` (400) per factory link of its cities
    /// (`productivity`: each factory links to the hall, +1 per adjacent
    /// house). ADAPTED: at least one link (a new colony's own buildings are
    /// not placed), x16 on the homeworld (`spaceEconomyHomeWorldBuildingMultiply`).
    pub fn colony_base_production(&self, world_id: u32) -> f32 {
        let production: f32 = self
            .city_layouts
            .get(&world_id)
            .map(|cities| cities.iter().map(|c| c.productivity()).sum())
            .unwrap_or(0.0);
        let base = rules::BASE_PRODUCTION_PER_FACTORY * production.max(1.0);
        if world_id == 0 {
            base * rules::HOMEWORLD_BUILDING_MULTIPLIER
        } else {
            base
        }
    }

    /// Money colonies pay per period: none in retail (colonies produce
    /// spice; `spaceEconomyBuildingIncome` is not referenced by the code).
    pub fn colony_income_per_period(&self) -> f32 {
        0.0
    }

    /// One line of HUD explaining the objective and the current status.
    pub fn hud(&self) -> String {
        let mut s = format!(
            "Space — {} (Rank: {}, {} badge pts) | {} colonies | energy {:.0}/{:.0} | cargo {}/{} slots",
            self.mission_name(),
            self.captain_rank_title(),
            self.total_badge_points(),
            self.new_colony_count(),
            self.energy,
            self.max_reactor_energy(),
            self.cargo_slots_used(),
            self.cargo_capacity_limit(),
        );
        s.push_str(&format!(
            "\nArchetype: {} | superpower: {}",
            self.archetype.title(),
            self.archetype.superpower_name()
        ));
        s.push_str(&format!("\nNext: {}", self.next_action()));
        if let Some(ref m) = self.active_alien_mission {
            let emp = Self::empire_name(m.empire);
            let status = if m.is_complete(self) {
                "COMPLETE - Hail to claim reward!"
            } else {
                "In Progress"
            };
            s.push_str(&format!(
                "\nActive Contract: {} [{emp}] ({}/{}) — {:.0} Sporebucks ({status}){}",
                m.kind.title(),
                m.current_progress(self),
                m.target_count,
                m.reward_spice,
                m.time_left.map(|t| format!(" {t:.0} s left")).unwrap_or_default()
            ));
        }
        s.push_str(&format!(
            "\nPlanetary tool: {} (T cycles, Y applies; U tractor beam; C colonise/buyout; B spice trade; K comms)",
            self.planetary_tool.label()
        ));
        if !self.message.is_empty() {
            s.push('\n');
            s.push_str(&self.message);
        }
        if !self.ship_message.is_empty() {
            s.push('\n');
            s.push_str(&self.ship_message);
        }
        s
    }
}

fn push_unique(list: &mut Vec<u32>, value: u32) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// Autonomous planetary surface-to-air defense turret protecting a colony from raiders.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ColonyTurret {
    pub fire_cooldown: f32,
    pub range: f32,
}

impl Default for ColonyTurret {
    fn default() -> Self {
        Self {
            fire_cooldown: 0.8,
            range: 16.0,
        }
    }
}

/// Player beam projectile.
#[derive(Component)]
pub struct Projectile {
    pub vel: Vec3,
    pub damage: f32,
    pub life: f32,
    /// `DamageArea` of the tool in world units: every ship within it of the
    /// impact takes the damage (0: only the ship that was hit).
    pub area: f32,
}

/// Pirate or Grox interceptor ship: hunts the player, flees when broken.
#[derive(Component)]
pub struct Pirate {
    pub hp: f32,
    pub max: f32,
    pub speed: f32,
    /// True once its kill has been credited, so a bounty is granted once only.
    pub rewarded: bool,
    pub is_grox: bool,
}

#[derive(Resource)]
pub struct SpaceTimer {
    pub fire: Timer,
    pub spawn: Timer,
}

impl Default for SpaceTimer {
    fn default() -> Self {
        Self {
            fire: Timer::from_seconds(0.35, TimerMode::Repeating),
            spawn: Timer::from_seconds(18.0, TimerMode::Repeating),
        }
    }
}

/// A body circling the system's star on a flat circular orbit.
#[derive(Component, Clone, Copy, Debug)]
pub struct Orbit {
    pub radius: f32,
    /// Current angle, radians.
    pub angle: f32,
    /// Angular speed, radians per second.
    pub speed: f32,
    /// Where this system last put the body. A different translation means
    /// something else (a checkpoint restore) moved it, and the orbit adopts
    /// that angle instead of snapping back.
    last: Vec3,
}

impl Orbit {
    pub fn new(radius: f32, angle: f32) -> Self {
        let mut orbit = Self {
            radius,
            angle,
            speed: space_galaxy::orbit_speed(radius),
            last: Vec3::NAN,
        };
        orbit.last = orbit.position();
        orbit
    }

    pub fn position(&self) -> Vec3 {
        Vec3::new(self.angle.cos(), 0.0, self.angle.sin()) * self.radius
    }
}

/// A moon circling a planet or gas giant.
#[derive(Component, Clone, Copy, Debug)]
pub struct Moon {
    pub host: Entity,
    pub distance: f32,
    pub angle: f32,
    pub speed: f32,
    /// Orbit tilt, so moons pass above and below their host.
    pub tilt: f32,
    /// Its terrain script (group, instance), when the system record has one;
    /// `space_planets` draws the moon from it.
    pub script: Option<(u32, u32)>,
}

/// A gas giant: scenery with no campaign identity.
#[derive(Component, Clone, Copy, Debug)]
pub struct GasGiant {
    pub radius: f32,
    pub ring: bool,
    pub seed: u32,
}

/// The system's star at the origin.
#[derive(Component, Clone, Copy, Debug)]
pub struct SystemStar {
    pub index: u32,
    pub kind: space_galaxy::StarKind,
    pub radius: f32,
}

/// Everything belonging to the current star system; despawned on a jump.
#[derive(Component)]
pub struct SystemBody;

/// The loaded star system's layout.
#[derive(Resource, Clone, Debug)]
pub struct CurrentSystem(pub SystemPlan);

impl CurrentSystem {
    /// Flight bound radius around the star.
    pub fn extent(&self) -> f32 {
        self.0.extent
    }
}

/// The UFO's momentum: thrust builds velocity, letting go lets it coast.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShipMotion {
    pub velocity: Vec3,
    pub thrusting: bool,
    pub autopilot_target: Option<u32>,
    /// Heading / bank / nose in the solar view (`space_endgame::step_attitude`).
    pub attitude: crate::space_endgame::UfoAttitude,
}

/// Root of the Space stage Terraforming Bullseye & Food-Web / Spice Cargo Console.
#[derive(Component)]
pub struct SpaceConsoleRoot;

/// Live dot moving on the 2D Temperature × Atmosphere T-score bullseye matrix.
#[derive(Component)]
pub struct SpaceBullseyeDot;

/// Multi-line readout displaying target world, foodweb tiers, local spice, and ship cargo.
#[derive(Component)]
pub struct SpaceConsoleText;

/// The home planet's name: generated by the original name generator
/// (`cSpaceNames` kPlanet) from a fixed seed, so every stage shows the same
/// name. (The old list of locale words, "Accessories" among them, was a
/// mislabel.)
pub fn campaign_world_names(originals: &Originals) -> Vec<String> {
    vec![home_planet_name(originals)]
}

/// The name generators of the installed game (`NameGeneration.package`), else
/// the built-in list.
pub fn space_names(originals: &Originals) -> space_galaxy::SpaceNames {
    space_galaxy::SpaceNames::from_install(|group, instance, type_id| {
        originals
            .resource_index
            .read(crate::dbpf::ResourceKey {
                group_id: group,
                instance_id: instance,
                type_id,
            })
            .ok()
    })
    .unwrap_or_else(space_galaxy::SpaceNames::builtin)
}

/// The name of the player's home planet.
pub fn home_planet_name(originals: &Originals) -> String {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        space_names(originals).name_for(space_galaxy::wg_name_kind_planet(), 0x4F4D_E5EE)
    })
    .clone()
}

/// The galaxy, generated once per process: the original emit maps steer the
/// star placement when they are installed, the original lexicons name the
/// stars and planets.
pub fn ensure_galaxy(galaxy: Option<&Galaxy>, originals: &Originals) -> Galaxy {
    if let Some(galaxy) = galaxy {
        return galaxy.clone();
    }
    let maps = StarMaps::from_install(|group, instance, type_id| {
        originals
            .resource_index
            .read(crate::dbpf::ResourceKey {
                group_id: group,
                instance_id: instance,
                type_id,
            })
            .ok()
    })
    .unwrap_or_else(StarMaps::analytic);
    let galaxy = Galaxy::generate_with(&maps, space_names(originals));
    space_galaxy::register_galaxy(&galaxy);
    galaxy
}

/// Spawn every body of `plan`: the star, its planets (campaign worlds keyed
/// by WorldId, coloured by the campaign's canonical state), gas giants and
/// moons. Visual dressing is added by `space_visuals`. Returns where the ship
/// should appear: beside the homeworld at home, else at the system's edge.
pub fn spawn_system(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    art: Option<&crate::space_scene::SpaceArt>,
    tints: &WorldTints,
    campaign: &SpaceCampaign,
    plan: &SystemPlan,
) -> Vec3 {
    commands.insert_resource(CurrentSystem(plan.clone()));
    commands.spawn((
        StageOwned,
        SystemBody,
        Name::new("star"),
        SystemStar {
            index: plan.star,
            kind: plan.kind,
            radius: plan.star_radius,
        },
        Transform::IDENTITY,
        Visibility::default(),
    ));
    let mut rng = ChaCha8Rng::seed_from_u64(0x0B17 ^ plan.star as u64);
    let mut arrival = None;
    let mut planets: Vec<(Entity, BodyPlan)> = Vec::new();
    for body in &plan.bodies {
        let orbit = Orbit::new(body.orbit, body.phase);
        let position = orbit.position();
        let entity = if let Some(id) = body.world {
            let colony = campaign.colonies.contains(&id) || id == 0;
            let scanned_here = colony || campaign.surveyed.contains(&id);
            let material = if colony {
                tints.owned.clone()
            } else if scanned_here {
                tints.surveyed.clone()
            } else {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(
                        rng.random_range(0.2..0.9),
                        rng.random_range(0.2..0.8),
                        rng.random_range(0.2..0.8),
                    ),
                    ..default()
                })
            };
            if arrival.is_none() || id == 0 {
                // Hover just off the world's sunward side, inside docking
                // reach, so the first view frames sun, ship and lit world.
                let out = position.normalize_or(Vec3::Z);
                arrival = Some(position - out * (body.radius + 4.5) + Vec3::Y * 3.0);
            }
            let mut planet_cmds = commands.spawn((
                StageOwned,
                SystemBody,
                crate::persistence::WorldId(id),
                Planet {
                    radius: body.radius,
                    colonized: colony,
                    name: body.name.clone(),
                    // `persistence::restore` runs after this and re-applies
                    // the checkpoint's flags; `space_campaign_update`
                    // reconciles both.
                    surveyed: scanned_here,
                },
                orbit,
                Mesh3d(meshes.add(Sphere::new(body.radius).mesh().uv(64, 32))),
                MeshMaterial3d(material),
                Transform::from_translation(position),
            ));
            if colony {
                planet_cmds.insert(ColonyTurret::default());
            }
            planet_cmds.id()
        } else {
            commands
                .spawn((
                    StageOwned,
                    SystemBody,
                    Name::new(body.name.clone()),
                    GasGiant {
                        radius: body.radius,
                        ring: body.ring,
                        seed: plan
                            .star
                            .wrapping_mul(31)
                            .wrapping_add(planets.len() as u32),
                    },
                    orbit,
                    Transform::from_translation(position),
                    Visibility::default(),
                ))
                .id()
        };
        for m in 0..body.moons {
            let distance = body.radius * 1.9 + 2.2 + m as f32 * 2.1;
            let size = rng.random_range(0.45..0.95) * (body.radius / 4.0).clamp(0.6, 1.3);
            commands.spawn((
                StageOwned,
                SystemBody,
                Name::new(format!("{} moon {}", body.name, m + 1)),
                Moon {
                    host: entity,
                    distance,
                    angle: rng.random_range(0.0..std::f32::consts::TAU),
                    speed: 0.9 / distance.sqrt(),
                    tilt: rng.random_range(-0.35..0.35),
                    script: body.moon_scripts.get(m as usize).copied().flatten(),
                },
                Transform::from_translation(position + Vec3::X * distance)
                    .with_scale(Vec3::splat(size)),
                Visibility::default(),
            ));
        }
        planets.push((entity, body.clone()));
    }
    crate::space_scene::spawn_system_visuals(commands, meshes, materials, art, plan, &planets);
    arrival.unwrap_or(Vec3::new(0.0, 3.0, plan.extent * 0.6))
}

pub fn setup_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (galaxy, request, art): (
        Option<Res<Galaxy>>,
        Option<Res<SpaceViewRequest>>,
        Option<Res<crate::space_scene::SpaceArt>>,
    ),
) {
    run.enter_stage(GameStage::Space);
    clear.0 = GameStage::Space.accent();

    clear.0 = Color::srgb(0.002, 0.004, 0.012);
    // Space rasters are decoded once per process and shared by both views.
    let art = art.map(|a| a.clone()).unwrap_or_else(|| {
        crate::space_scene::SpaceArt::load(&originals.resource_index, &mut images)
    });
    commands.insert_resource(art.clone());
    crate::space_scene::spawn_sky(&mut commands, &mut meshes, &mut materials, Some(&art));
    let galaxy = ensure_galaxy(galaxy.as_deref(), &originals);
    // Canonical world state comes from the campaign's exact WorldIds, never
    // from a position in the spawn list: reloading must not grant a world the
    // player never owned.
    let mut campaign = std::mem::take(&mut run.space_campaign);
    // A campaign saved in the prototype galaxy returns to the home system.
    let home_worlds = space_galaxy::system_plan(&galaxy, HOME_STAR, &[])
        .bodies
        .iter()
        .filter(|b| b.world.is_some())
        .count() as u32;
    if campaign.migrate_galaxy(home_worlds) {
        run.request_save();
    }
    // Retail rules (space_rules): wallet, start tools, cards -> archetype,
    // difficulty; an old save's fields are mapped once. Runs before the
    // reactor fill, which tells a played campaign from a new one.
    let cards = cards_from_run(&run);
    let (difficulty, hull, weapon) = (run.cell.difficulty, run.hull, run.weapon);
    if campaign.adopt_retail_rules(&mut run.spice, cards, difficulty, hull, weapon) {
        run.request_save();
    }
    // A fresh campaign gets a full reactor exactly once; a save that
    // legitimately recorded an empty reactor keeps its zero.
    if campaign.initialise_fuel() {
        run.request_save();
    }
    // Pre-campaign saves: migrate the checkpoint's per-world flags by WorldId.
    // Only fall back to the old sorted-count mapping when the save has no exact
    // Space snapshot at all.
    let space_checkpoint = run
        .checkpoint
        .as_ref()
        .filter(|c| c.stage == RunProgress::stage_order().len() - 1)
        .cloned();
    let mut adopted = false;
    if let Some(checkpoint) = space_checkpoint.as_ref() {
        adopted = campaign.adopt_checkpoint(checkpoint);
    } else {
        let legacy = run.colonies;
        campaign.reconcile_legacy_colonies(legacy);
        adopted = campaign.colonies.len() > 1;
    }
    if adopted {
        run.request_save();
    }
    run.colonies = campaign.new_colony_count();
    // A save naming a star this galaxy does not have returns home.
    if galaxy.star(campaign.current_system).is_none() {
        campaign.current_system = HOME_STAR;
    }
    let system = campaign.current_system;
    campaign.arrive(system);
    campaign.current_system_empire = galaxy.star(system).and_then(|s| s.empire);
    campaign.empire_sizes = vec![1; space_galaxy::EMPIRES.len()];
    for empire in galaxy.empires.iter() {
        campaign.empire_sizes[empire.id as usize] = empire.stars;
    }
    campaign.empire_sizes[0] = galaxy.grox_stars;
    let empire_here = campaign.current_system_empire;
    for news in campaign.on_arrival(empire_here, galaxy.is_galactic_core(system)) {
        campaign.message = news;
    }
    campaign.on_arrival_near(&galaxy.empires_noticing(system));

    // Shared world tints, built once so the per-frame sync only compares
    // handles and never allocates a new material.
    let tints = WorldTints {
        owned: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.8, 0.45),
            emissive: Color::srgb(0.05, 0.22, 0.08).into(),
            ..default()
        }),
        surveyed: materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.7, 0.3),
            emissive: Color::srgb(0.2, 0.16, 0.03).into(),
            ..default()
        }),
    };
    commands.insert_resource(tints.clone());

    // Prefer original game strings for world names (1:1 flavor), else fallback.
    let world_names = campaign_world_names(&originals);
    let plan = space_galaxy::system_plan(&galaxy, system, &world_names);
    let arrival = spawn_system(
        &mut commands,
        &mut meshes,
        &mut materials,
        Some(&art),
        &tints,
        &campaign,
        &plan,
    );
    run.space_campaign = campaign;
    let focus = galaxy
        .star(system)
        .map(|s| s.position)
        .unwrap_or(Vec2::ZERO);
    commands.insert_resource(SpaceView::entering(
        system,
        focus,
        request.as_deref().map(|r| r.0),
    ));
    crate::space_galaxy::spawn_galaxy_map(
        &mut commands,
        &mut meshes,
        &mut materials,
        &art,
        &galaxy,
    );
    commands.insert_resource(galaxy);
    let mut rng = ChaCha8Rng::seed_from_u64(777);
    // Player UFO: creature stats + ship loadout tune speed and hull.
    let stats = run.creature_stats();
    commands.insert_resource(crate::creature::CreatureStatsRes(stats));
    // Retail UFO parts are cosmetic: the loadout no longer adds hull or
    // weapon damage (its thrust bonus stays with the flight controls).
    let (_, _, extra_speed) = ship_bonus(&run.ship);
    let speed_card = rules::speed_multiplier(run.space_campaign.superpower_flags);
    let max_hull = run.space_campaign.max_hull();
    let ship_ent = commands
        .spawn((
            StageOwned,
            Player,
            Mover {
                speed: ((16.0 * (0.9 + stats.speed / 40.0)) + extra_speed) * speed_card,
            },
            ShipMotion::default(),
            Health::new(max_hull),
            ShipWeapon(1.0),
            Mesh3d(meshes.add(Cuboid::new(1.8, 0.5, 1.8))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.9, 1.0),
                emissive: Color::srgb(0.2, 0.4, 0.8).into(),
                ..default()
            })),
            Transform::from_translation(arrival),
        ))
        .id();
    // Try to mount the original genufo spaceship preset as the visual body.
    // On success, remove the primitive cuboid and use the preset instead.
    // On failure, keep the primitive — gameplay is unaffected either way.
    match creation_assets::mount_preset(
        &mut commands,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        ship_ent,
        creation_assets::PresetKind::Spaceship,
        2.4,
    ) {
        Ok(true) => {
            commands.entity(ship_ent).remove::<Mesh3d>();
            commands
                .entity(ship_ent)
                .remove::<MeshMaterial3d<StandardMaterial>>();
            info!("ship visual: original genufo preset mounted");
        }
        Ok(false) => {
            info!("ship visual: no creation catalog available, using primitive");
        }
        Err(e) => {
            info!("ship visual: preset mount failed ({e}), using primitive");
        }
    }
    // Loadout modules (child entities on the hull). Startup only scans a
    // handful of preview meshes, so a reloaded build has to be imported again
    // here or its parts would silently vanish on a fresh process. This never
    // rewrites the build: records the original catalog cannot supply are
    // reported and kept exactly as the player saved them.
    let report = import_saved_ship_parts(
        &mut originals,
        &run.ship,
        &mut meshes,
        &mut images,
        &mut materials,
    );
    // Design blocks with handle weights are drawn morphed (`rigid_editor`).
    for m in run.ship.modules.iter().filter(|m| m.block.is_some()) {
        crate::rigid_editor::ensure_morph(&originals, &mut meshes, SHIP_PART_GROUP, m.definition_id, m.instance, &m.block.as_ref().unwrap().handles);
    }
    if !report.is_empty() {
        let text = format!(
            "Ship parts unavailable from the original vehicle catalog: {}",
            report.join("; ")
        );
        info!("{text}");
        run.space_campaign.ship_message = text;
    }
    // One shared flat material for the rare part that has no authored diffuse
    // map. Built once per stage entry, so rebuilding the mount list later
    // never allocates a generic material per module.
    let fallback = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.7, 0.85),
        perceptual_roughness: 0.4,
        ..default()
    });
    commands.insert_resource(ShipPartFallback(fallback.clone()));
    let unsupported = mount_ship_modules(
        &mut commands,
        &mut materials,
        &meshes,
        Some(&fallback),
        ship_ent,
        &originals,
        &run,
    );
    if !unsupported.is_empty() {
        info!(
            "ship parts kept but not rendered: {}",
            unsupported
                .iter()
                .map(|i| format!("{i:08X}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !run.ship.modules.is_empty() {
        info!("ship loadout: {} module(s)", run.ship.modules.len());
    }
    // Hostile ships patrol the system: Grox interceptors in deep core systems, else pirates.
    let is_grox = run.space_campaign.current_system_empire == Some(0);
    for _ in 0..4 {
        if is_grox {
            spawn_grox_interceptor(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut rng,
                arrival,
                plan.extent,
            );
        } else {
            spawn_pirate(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut rng,
                arrival,
                plan.extent,
            );
        }
    }
    spawn_space_console(&mut commands);
}

/// Spawns the bottom-left Terraforming T-Score Bullseye and Food-Web / Spice Cargo Dashboard.
pub fn spawn_space_console(commands: &mut Commands) {
    commands
        .spawn((
            StageOwned,
            SpaceConsoleRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(216.0),
                bottom: Val::Px(36.0),
                width: Val::Px(310.0),
                height: Val::Px(120.0),
                padding: UiRect::all(Val::Px(6.0)),
                column_gap: Val::Px(8.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.06, 0.16, 0.90)),
            BorderColor::all(Color::srgba(0.40, 0.70, 1.0, 0.75)),
        ))
        .with_children(|root| {
            // Left: 2D T-Score Bullseye radar (104x104 px)
            root.spawn((
                Node {
                    width: Val::Px(104.0),
                    height: Val::Px(104.0),
                    position_type: PositionType::Relative,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.01, 0.04, 0.10, 0.95)),
                BorderColor::all(Color::srgba(0.3, 0.5, 0.8, 0.5)),
            ))
            .with_children(|radar| {
                // T1 outer circle
                radar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(92.0),
                        height: Val::Px(92.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Percent(50.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.85, 0.65, 0.20, 0.6)),
                ));
                // T2 middle circle
                radar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(60.0),
                        height: Val::Px(60.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Percent(50.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.55, 0.85, 0.25, 0.7)),
                ));
                // T3 inner target disc
                radar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Percent(50.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.12, 0.50, 0.25, 0.45)),
                    BorderColor::all(Color::srgba(0.25, 0.95, 0.45, 0.9)),
                ));
                // Crosshairs: horizontal (Temp) and vertical (Atmos)
                radar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(104.0),
                        height: Val::Px(1.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.4, 0.6, 0.9, 0.3)),
                ));
                radar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(1.0),
                        height: Val::Px(104.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.4, 0.6, 0.9, 0.3)),
                ));
                // Bullseye indicator dot
                radar.spawn((
                    SpaceBullseyeDot,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(48.0),
                        top: Val::Px(48.0),
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        border_radius: BorderRadius::all(Val::Percent(50.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.95, 0.3)),
                ));
            });

            // Right: Planet readout & Food-Web status column
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceEvenly,
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    SpaceConsoleText,
                    Text::new("TERRAFORMING CONSOLE\nScanning system..."),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.96, 1.0)),
                ));
            });
        });
}

/// Spawn an enemy ship: standard pirate or high-threat Grox interceptor.
pub fn spawn_enemy_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut impl Rng,
    around: Vec3,
    extent: f32,
    is_grox: bool,
) {
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let distance = rng.random_range(22.0..34.0);
    let mut position = around + Vec3::new(angle.cos(), 0.0, angle.sin()) * distance;
    position.y = around.y.clamp(2.0, 14.0);
    let flat = Vec2::new(position.x, position.z).clamp_length_max(extent);
    let (hp, speed, base_color, emissive) = if is_grox {
        (90.0, 11.5, Color::srgb(0.95, 0.08, 0.08), Color::srgb(0.5, 0.02, 0.02))
    } else {
        (60.0, 9.0, Color::srgb(0.8, 0.1, 0.1), Color::BLACK)
    };
    commands.spawn((
        StageOwned,
        SystemBody,
        Pirate {
            hp,
            max: hp,
            speed,
            rewarded: false,
            is_grox,
        },
        Mesh3d(meshes.add(Cuboid::new(1.2, 0.4, 1.2))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color,
            emissive: emissive.into(),
            ..default()
        })),
        Transform::from_xyz(flat.x, position.y, flat.y),
    ));
}

/// A pirate warps in 22-34 units from `around`, inside the system's bound.
pub fn spawn_pirate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut impl Rng,
    around: Vec3,
    extent: f32,
) {
    spawn_enemy_ship(commands, meshes, materials, rng, around, extent, false);
}

/// A Grox deep core interceptor warps in 22-34 units from `around`.
pub fn spawn_grox_interceptor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut impl Rng,
    around: Vec3,
    extent: f32,
) {
    spawn_enemy_ship(commands, meshes, materials, rng, around, extent, true);
}

/// Planets and gas giants circle their star; moons circle their hosts; the
/// ship rides along with the world it is hovering beside, as a docked UFO
/// keeps station. A restored checkpoint position is adopted, not overwritten.
pub fn space_orbits(
    time: Res<Time>,
    view: Option<Res<SpaceView>>,
    mut bodies: Query<
        (Entity, &mut Transform, &mut Orbit, Option<&Planet>, Option<&crate::persistence::WorldId>),
        Without<Player>,
    >,
    mut moons: Query<(&mut Transform, &mut Moon), (Without<Orbit>, Without<Player>)>,
    mut player: Query<&mut Transform, (With<Player>, Without<Orbit>, Without<Moon>)>,
) {
    let dt = time.delta_secs();
    // The system keeps turning on the galaxy map, but slowly enough that a
    // short look at the stars never leaves the ship far from its world.
    let rate = if view.as_ref().is_some_and(|v| v.galaxy) {
        0.25
    } else {
        1.0
    };
    let mut moved: Vec<(Entity, Vec3, Vec3, Option<f32>)> = Vec::new();
    // The world under the planet view holds still (its surface is drawn in
    // its frame, `space_planets`).
    let held = view.as_ref().and_then(|v| v.planet.as_ref()).map(|p| p.world);
    for (entity, mut t, mut orbit, planet, id) in &mut bodies {
        if held.is_some() && id.map(|w| w.0) == held {
            moved.push((entity, t.translation, Vec3::ZERO, planet.map(|p| p.radius)));
            continue;
        }
        if t.translation.distance_squared(orbit.last) > 1e-6 {
            let flat = Vec2::new(t.translation.x, t.translation.z);
            if flat.length_squared() > 1e-6 {
                orbit.angle = flat.y.atan2(flat.x);
            }
            t.translation = orbit.position();
            orbit.last = t.translation;
        }
        let before = t.translation;
        orbit.angle = (orbit.angle + orbit.speed * dt * rate).rem_euclid(std::f32::consts::TAU);
        t.translation = orbit.position();
        orbit.last = t.translation;
        // Worlds turn on their axis; clouds drift on top of that.
        t.rotate_y(0.035 * dt * rate);
        moved.push((
            entity,
            t.translation,
            t.translation - before,
            planet.map(|p| p.radius),
        ));
    }
    for (mut t, mut moon) in &mut moons {
        let Some((_, host, _, _)) = moved.iter().find(|(e, ..)| *e == moon.host) else {
            continue;
        };
        moon.angle = (moon.angle + moon.speed * dt * rate).rem_euclid(std::f32::consts::TAU);
        let (s, c) = moon.angle.sin_cos();
        t.translation = *host + Vec3::new(c, s * moon.tilt, s) * moon.distance;
    }
    let Ok(mut ship) = player.single_mut() else {
        return;
    };
    let carrier = moved
        .iter()
        .filter_map(|(_, at, delta, radius)| {
            let reach = (*radius)? + ACTION_RANGE;
            let d = ship.translation.distance(*at - *delta);
            (d < reach).then_some((d, *delta))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, delta)) = carrier {
        ship.translation += delta;
    }
}

pub fn space_fly(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    view: Option<Res<SpaceView>>,
    system: Option<Res<CurrentSystem>>,
    mut player: Query<(&mut Transform, &Mover, Option<&mut ShipMotion>), With<Player>>,
    planets: Query<(&Transform, &Planet, Option<&crate::persistence::WorldId>), Without<Player>>,
    mut run: ResMut<RunProgress>,
    mut controls: Option<ResMut<crate::space_controls::SpaceControls>>,
) {
    let Ok((mut t, mover, motion)) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    // Retail flight inside a system needs no energy (tools and jumps spend it).
    let throttle = 1.0;
    let extent = system.as_ref().map_or(BOUNDS, |s| s.extent());
    let before = t.translation;
    let Some(mut motion) = motion else {
        // A ship without momentum state (older test fixtures) flies directly.
        move_player_on_plane(&keys, &mut t, mover.speed * throttle, dt, extent);
        run.space_campaign
            .spend_travel(before.distance(t.translation));
        return;
    };
    // The planet flight (space_controls) already moved the ship this frame.
    if controls.as_ref().is_some_and(|c| c.ship_handled) {
        return;
    }
    // Retail controls: the solar system has no keyboard flight, the ship flies
    // where it is sent; the galaxy map never steers.
    let (steering, manual_dir, cancel) = crate::space_controls::manual_flight(
        &keys,
        controls.as_deref_mut(),
        view.as_deref(),
        t.translation,
    );
    if cancel {
        motion.autopilot_target = None;
    }

    let mut dir = manual_dir.normalize_or_zero();
    if dir == Vec3::ZERO && steering {
        if let Some(target_id) = motion.autopilot_target {
            if let Some((pt, planet, _)) = planets
                .iter()
                .find(|(_, _, wid)| wid.map_or(false, |w| w.0 == target_id))
            {
                let to_target = pt.translation - t.translation;
                let target_dist = to_target.length();
                let arrive_dist = planet.radius + ACTION_RANGE * 0.5;
                if (target_dist - arrive_dist).abs() < 1.0 || target_dist < arrive_dist {
                    motion.autopilot_target = None;
                } else {
                    let desired_pos =
                        pt.translation - to_target.normalize_or(Vec3::Z) * arrive_dist;
                    let to_desired = desired_pos - t.translation;
                    dir = Vec3::new(to_desired.x, 0.0, to_desired.z).normalize_or_zero();
                }
            } else {
                motion.autopilot_target = None;
            }
        }
    }
    let top = mover.speed * throttle * view.as_ref().map_or(1.0, |v| v.flight_scale());
    let target = dir * top;
    motion.thrusting = dir != Vec3::ZERO;
    let rate = if motion.thrusting {
        SHIP_ACCELERATION
    } else {
        SHIP_DRAG
    };
    let horizontal = Vec3::new(motion.velocity.x, 0.0, motion.velocity.z);
    let horizontal = horizontal + (target - horizontal) * (1.0 - (-rate * dt).exp());
    // Settle to an exact stop instead of creeping forever.
    let horizontal = if !motion.thrusting && horizontal.length() < 0.05 {
        Vec3::ZERO
    } else {
        horizontal
    };
    motion.velocity = horizontal;
    t.translation += horizontal * dt;
    if steering && controls.is_none() {
        let lift = mover.speed * 0.7 * dt * throttle;
        if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::ShiftLeft) {
            t.translation.y += lift;
        }
        if keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::Space) {
            t.translation.y -= lift;
        }
    }
    t.translation.y = t.translation.y.clamp(ALTITUDE_MIN, ALTITUDE_MAX);
    // Stay inside the system, out of the star and above every surface.
    let mut flat = Vec2::new(t.translation.x, t.translation.z);
    if flat.length() > extent {
        flat = flat.clamp_length_max(extent);
        motion.velocity *= 0.5;
    }
    t.translation.x = flat.x;
    t.translation.z = flat.y;
    let star = system
        .as_ref()
        .map_or(0.0, |s| s.0.star_radius + STAR_CLEARANCE);
    if star > 0.0 && t.translation.length() < star {
        t.translation = t.translation.normalize_or(Vec3::Z) * star;
    }
    for (pt, planet, _) in &planets {
        let offset = t.translation - pt.translation;
        let floor = planet.radius + SURFACE_CLEARANCE;
        if offset.length() < floor {
            t.translation = pt.translation + offset.normalize_or(Vec3::Y) * floor;
        }
    }
    // The UFO turns its front to the flight direction, banks into turns and
    // noses into thrust (`spaceufo~` tilt limits).
    let velocity = motion.velocity;
    let thrusting = motion.thrusting;
    t.rotation = crate::space_endgame::step_attitude(&mut motion.attitude, velocity, top, thrusting, dt);
    run.space_campaign
        .spend_travel(before.distance(t.translation));
}

/// V surveys a nearby world for spice (once per world).
pub fn space_survey(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId)>,
) {
    if !keys.just_pressed(KeyCode::KeyV) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    if let Some((_, planet, id)) = planets
        .iter()
        .filter(|(pose, planet, _)| {
            !planet.colonized
                && !planet.surveyed
                && pt.translation.distance(pose.translation) < planet.radius + 6.0
        })
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
                .then(a.2 .0.cmp(&b.2 .0))
        })
    {
        // Move the campaign out so the action cannot double-borrow the run;
        // apply the feedback separately so neither call needs it mutably at
        // the same time as the other.
        let mut campaign = std::mem::take(&mut run.space_campaign);
        let action = campaign.try_survey(id.0, planet.radius, &mut run);
        campaign.apply(action);
        run.space_campaign = campaign;
        crate::space_tool_fx::request(&mut commands, "scan", Some(id.0));
    } else if run.space_campaign.tutorial.step == crate::space_rules::TutorialStep::Flight101Scan {
        // Flight 101's scan happens on the home planet, which is already owned.
        if let Some((_, planet, id)) = planets
            .iter()
            .filter(|(pose, planet, _)| pt.translation.distance(pose.translation) < planet.radius + 6.0)
            .min_by(|a, b| {
                pt.translation
                    .distance(a.0.translation)
                    .total_cmp(&pt.translation.distance(b.0.translation))
            })
        {
            let mut campaign = std::mem::take(&mut run.space_campaign);
            let action = campaign.try_survey(id.0, planet.radius, &mut run);
            campaign.apply(action);
            run.space_campaign = campaign;
            crate::space_tool_fx::request(&mut commands, "scan", Some(id.0));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn space_colonize(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId)>,
    view: Option<Res<crate::space_view::SpaceView>>,
    controls: Option<Res<crate::space_controls::SpaceControls>>,
    ground: Option<Res<crate::space_planets::PlanetViewGround>>,
) {
    let Ok(pt) = player.single() else { return };
    // Repair and recharge are bought while docked (space_campaign_update).
    if run.finished || !keys.just_pressed(KeyCode::KeyC) {
        return;
    }
    if let Some((_, _, id)) = planets
        .iter()
        .filter(|(pose, planet, _)| pt.translation.distance(pose.translation) < planet.radius + 4.0)
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
                .then(a.2 .0.cmp(&b.2 .0))
        })
    {
        let mut campaign = std::mem::take(&mut run.space_campaign);
        let (before, alien) = (campaign.colony_count(id.0), campaign.world_empire(id.0).is_some());
        let action = campaign.try_colonize(id.0, &mut run);
        if matches!(action, WorldAction::Done(_)) {
            crate::space_tool_fx::request(&mut commands, "placecolony", Some(id.0));
            // The new colony stands where the colony pack landed.
            let after = campaign.colony_count(id.0);
            if after > before && !alien {
                let landing = crate::space_planets::colony_landing(
                    view.as_deref(),
                    ground.as_deref(),
                    controls.as_ref().and_then(|c| c.ray),
                    id.0,
                );
                if let (Some(dir), Some(layout)) = (
                    landing,
                    campaign.city_layouts.get_mut(&id.0).and_then(|l| l.get_mut(after as usize - 1)),
                ) {
                    layout.site = Some(dir.to_array());
                }
            }
        }
        campaign.apply(action);
        run.space_campaign = campaign;
    }
}

/// P plans or cycles municipal building construction on an owned colony world.
pub fn space_city_planning(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId)>,
) {
    if !keys.just_pressed(KeyCode::KeyP) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    if let Some((_, _, wid)) = planets
        .iter()
        .filter(|(pose, planet, _)| pt.translation.distance(pose.translation) < planet.radius + 8.0)
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
                .then(a.2 .0.cmp(&b.2 .0))
        })
    {
        let owned = run.space_campaign.colonies.contains(&wid.0) || wid.0 == 0;
        if !owned {
            run.space_campaign.message = format!("World {} is not an owned colony (colonize with C first)", wid.0);
            return;
        }
        let mut campaign = std::mem::take(&mut run.space_campaign);
        campaign.ensure_city_layouts(wid.0);
        let count = campaign.colony_count(wid.0).max(1) as usize;
        let mut placed = false;
        'city_loop: for c_idx in 0..count {
            let layout = &campaign.city_layouts.get(&wid.0).unwrap()[c_idx];
            for slot_idx in 0..layout.capacity().min(layout.slots.len()) {
                if layout.slots[slot_idx].is_none() {
                    let b = if layout.houses() == 0 {
                        CityBuilding::House
                    } else if layout.houses() > layout.factories() {
                        CityBuilding::Factory
                    } else if layout.happiness() <= 0 && layout.entertainment() < layout.houses() {
                        CityBuilding::Entertainment
                    } else if layout.turrets() == 0 {
                        CityBuilding::Turret
                    } else if layout.factories() <= layout.entertainment() {
                        CityBuilding::Factory
                    } else {
                        CityBuilding::House
                    };
                    let action = campaign.build_city_structure(wid.0, c_idx, slot_idx, b, &mut run);
                    campaign.apply(action);
                    placed = true;
                    break 'city_loop;
                }
            }
        }
        if !placed {
            let summary = campaign
                .city_layouts
                .get(&wid.0)
                .map(|cities| {
                    cities
                        .iter()
                        .enumerate()
                        .map(|(i, c)| format!("City {} ({}/{} buildings): {}", i + 1, c.building_count(), c.capacity(), c.summary()))
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .unwrap_or_default();
            campaign.message = format!("World {} municipal grid fully built: {}", wid.0, summary);
        }
        run.space_campaign = campaign;
    }
}

/// U activates the Tractor Beam: recovers rare artifacts, cleanses biohazards, or beams up colony spice.
pub fn space_tractor_beam(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId)>,
) {
    if !keys.just_pressed(KeyCode::KeyU) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    if let Some((_, _planet, id)) = planets
        .iter()
        .filter(|(pose, planet, _)| pt.translation.distance(pose.translation) < planet.radius + 6.0)
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
                .then(a.2 .0.cmp(&b.2 .0))
        })
    {
        let mut campaign = std::mem::take(&mut run.space_campaign);
        let action = campaign.try_tractor_beam(id.0, &mut run);
        campaign.apply(action);
        run.space_campaign = campaign;
        crate::space_tool_fx::request(&mut commands, "abduct", Some(id.0));
    }
}

/// R activates the signature superpower of the player's Space Archetype.
pub fn space_superweapon(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId)>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    let target_world = planets
        .iter()
        .filter(|(pose, planet, _)| pt.translation.distance(pose.translation) < planet.radius + 12.0)
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
                .then(a.2 .0.cmp(&b.2 .0))
        })
        .map(|(_, _, wid)| wid.0);

    let mut campaign = std::mem::take(&mut run.space_campaign);
    let action = campaign.try_activate_superweapon(&mut run, target_world);
    if matches!(action, WorldAction::Done(_)) {
        if let Some(power) = rules::archetype_superpower(campaign.archetype) {
            crate::space_tool_fx::request(&mut commands, power.id, target_world);
        }
    }
    campaign.apply(action);
    run.space_campaign = campaign;
}

/// F fires the selected combat tool (`SpaceCampaign::selected_weapon`: the best
/// laser, or the pulse / proton / bomb chosen in the palette) at the nearest
/// pirate or alien empire colony. Held, the tool repeats every `AutoFireRate`
/// (0.1 s for the lasers, 1 s for the projectile weapons: the hold handler
/// 0x1050500); each use spends `EnergyCost` energy (0x10504A1) and needs that
/// much in the reactor (0x1050C4F); `RechargeRate` seconds follow a use.
pub fn space_fire(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut run: ResMut<RunProgress>,
    ship_weapon: Query<&ShipWeapon, With<Player>>,
    player: Query<&Transform, With<Player>>,
    pirates: Query<&Transform, With<Pirate>>,
    planets: Query<(&Transform, &Planet, &crate::persistence::WorldId), Without<Player>>,
) {
    let Ok(pt) = player.single() else { return };
    let Ok(weapon_mult) = ship_weapon.single() else {
        return;
    };
    // A laser is a beam (damage a rate per second of contact applied in
    // `AutoFireRate` ticks); the pulse, proton and bomb tools are projectiles
    // (damage per shot). Energy is per use either way.
    let laser_level = rules::laser_level(&run.space_campaign.tools);
    let laser = run.space_campaign.selected_weapon();
    let beam = rules::is_beam_weapon(laser.id);
    // The retail record of the tool: its relationship deltas (`OnHit` 0x105987C)
    // and whether a use spends one ammo (`UsesAmmo`, 0x1050080).
    let record = space_tools::record(laser.id);
    let uses_ammo = record.is_some_and(|r| r.uses_ammo());
    let (hit_delta, hit_galactic) = record.map_or((-0.02, 0.0), |r| (r.relationship_delta, r.galactic_delta));
    let interval = laser.autofire;
    let fire = run.space_campaign.fire_clock.step(
        interval,
        keys.pressed(KeyCode::KeyF),
        keys.just_pressed(KeyCode::KeyF),
        time.delta_secs(),
    );
    if fire == rules::ToolFire::None {
        return;
    }
    let damage = if beam {
        rules::beam_damage_per_tick(laser.damage, interval) * weapon_mult.0
    } else {
        laser.damage * weapon_mult.0
    };
    // The Rally Call's `MaxDamage` multiplier on the player's side (0xBFCF10).
    let damage = damage * crate::space_tool_use::rally_multiplier(&run.space_campaign);
    let energy_cost = rules::tool_use_energy(laser.energy, interval, time.delta_secs());
    if !rules::tool_can_afford_energy(run.space_campaign.energy, energy_cost) {
        run.space_campaign.message = "Out of energy: recharge at a planet".into();
        return;
    }
    if uses_ammo && run.space_campaign.charges(laser.id) == 0 {
        run.space_campaign.message = format!("No {} left", laser.id);
        return;
    }
    // The base `cToolStrategy::WhileFiring` 0x1056880 of every use ends a cloak; a
    // weapon-type tool also ends the soothing song (0x1050FC9 -> 0xC757E0).
    crate::space_tool_use::break_cloak(&mut run.space_campaign);
    if record.is_some_and(crate::space_tool_use::ends_soothing_song) {
        run.space_campaign.siren_song_active = 0.0;
    }

    // Nearest pirate in range (`MinRange` .. `MaxRange` in retail units: laser
    // 200, pulse 150, proton missile 10 .. 200).
    let range = laser.max_range / rules::RETAIL_UNITS_PER_WORLD_UNIT;
    let min_range = laser.min_range.max(0.0) / rules::RETAIL_UNITS_PER_WORLD_UNIT;
    let nearest_pirate = pirates
        .iter()
        .filter(|t| {
            let d = t.translation.distance(pt.translation);
            d < range && d >= min_range
        })
        .min_by(|a, b| {
            a.translation
                .distance_squared(pt.translation)
                .partial_cmp(&b.translation.distance_squared(pt.translation))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(target) = nearest_pirate {
        let Some(dir) = aim_dir(pt.translation, target.translation) else {
            return;
        };
        run.space_campaign.energy -= energy_cost;
        run.space_campaign.fire_clock.commit(laser.recharge);
        if uses_ammo {
            run.space_campaign.take_charge(laser.id);
        }
        if !beam {
            // A projectile weapon: the retail tool's muzzle, projectile and
            // explosion effects fly to the target at `toolProjectileSpeed`; the
            // hit test is the (hidden) sphere flying with it. ADAPTED: damage
            // lands on the one ship it hits (the blast `DamageArea` is not
            // applied).
            let speed = crate::space_tool_use::projectile_speed(laser.id) / rules::RETAIL_UNITS_PER_WORLD_UNIT;
            let from = pt.translation + dir * 1.2f32.min(pt.translation.distance(target.translation) * 0.5);
            commands.spawn((
                StageOwned,
                Projectile { vel: dir * speed, damage, life: range / speed + 0.3, area: laser.area / rules::RETAIL_UNITS_PER_WORLD_UNIT },
                Mesh3d(meshes.add(Sphere::new(0.18))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.8, 0.3),
                    emissive: Color::srgb(1.0, 0.6, 0.1).into(),
                    ..default()
                })),
                Visibility::Hidden,
                Transform::from_translation(from),
            ));
            crate::space_tool_fx::request_shot(&mut commands, laser.id, pt.translation, target.translation);
            return;
        }
        commands.spawn((
            StageOwned,
            Projectile {
                vel: dir * 40.0,
                damage,
                life: 1.2,
                area: 0.0,
            },
            crate::space_weapon_fx::LaserShot { level: laser_level, to: target.translation },
            Mesh3d(meshes.add(Sphere::new(0.18))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.6, 1.0, 0.6),
                emissive: Color::srgb(0.3, 1.0, 0.3).into(),
                ..default()
            })),
            // A point-blank target: start the beam short of it, never past it.
            Transform::from_translation(pt.translation + dir * 1.2f32.min(pt.translation.distance(target.translation) * 0.5)),
        ));
        return;
    }

    // If no pirate is near, check if ship is in reach of an alien empire world to bombard.
    let empire_world = planets
        .iter()
        .filter(|(t, p, wid)| {
            run.space_campaign.world_empire(wid.0).is_some()
                && pt.translation.distance(t.translation) < p.radius + 14.0
        })
        .min_by(|a, b| {
            pt.translation
                .distance(a.0.translation)
                .total_cmp(&pt.translation.distance(b.0.translation))
        });

    if let Some((wt, planet, wid)) = empire_world {
        let world_id = wid.0;
        let Some(dir) = aim_dir(pt.translation, wt.translation) else {
            return;
        };
        run.space_campaign.energy -= energy_cost;
        run.space_campaign.fire_clock.commit(laser.recharge);
        if uses_ammo {
            run.space_campaign.take_charge(laser.id);
        }
        if beam {
            commands.spawn((
                StageOwned,
                Projectile {
                    vel: dir * 35.0,
                    damage,
                    life: 0.8,
                    area: 0.0,
                },
                crate::space_weapon_fx::LaserShot { level: laser_level, to: wt.translation - dir * planet.radius },
                Mesh3d(meshes.add(Sphere::new(0.24))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.4, 0.2),
                    emissive: Color::srgb(1.0, 0.2, 0.1).into(),
                    ..default()
                })),
                Transform::from_translation(pt.translation + dir * 1.2),
            ));
        } else {
            crate::space_tool_fx::request_shot(&mut commands, laser.id, pt.translation, wt.translation - dir * planet.radius);
        }
        let surrendered_before = run.space_campaign.empire_colony_surrendered(world_id);
        let feedback = run.space_campaign.damage_empire_colony_with(world_id, damage, hit_delta, hit_galactic);
        let hp_after = run.space_campaign.empire_defense_hp(world_id);
        if !surrendered_before && run.space_campaign.empire_colony_surrendered(world_id) {
            let msg = format!(
                "{} colony surrendered! Press C to claim military victory.",
                planet.name
            );
            info!("{msg}");
            run.space_campaign.message = msg;
        } else {
            let msg = format!(
                "Bombarding {} colony! Defense: {:.0}/{EMPIRE_COLONY_MAX_HP:.0}",
                planet.name, hp_after
            );
            run.space_campaign.message = feedback.unwrap_or(msg);
        }
        run.request_save();
        return;
    }

    // Nothing to shoot at: the use is consumed without a beam (no energy) so
    // the hold keeps its cadence.
    run.space_campaign.fire_clock.commit(laser.recharge);
}

/// Escort wingman component managing fleet formation and auto-targeting support fire.
#[derive(Component)]
pub struct EscortWingman {
    pub slot: usize,
    pub fire_cooldown: f32,
}

pub fn space_escort_combat(
    time: Res<Time>,
    view: Option<Res<SpaceView>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    run: Res<RunProgress>,
    player: Query<&Transform, With<Player>>,
    mut escorts: Query<(Entity, &mut Transform, &mut EscortWingman), Without<Player>>,
    pirates: Query<&Transform, (With<Pirate>, Without<Player>, Without<EscortWingman>)>,
) {
    if view.as_ref().is_some_and(|v| v.galaxy) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    let dt = time.delta_secs();
    let fleet_count = run.space_campaign.fleet.len();

    // 1. Sync escort entities with campaign fleet count.
    let escort_count = escorts.iter().len();
    if escort_count < fleet_count {
        for slot in escort_count..fleet_count {
            let offset = escort_formation_offset(slot);
            let pos = pt.translation + pt.rotation * offset;
            commands.spawn((
                StageOwned,
                EscortWingman {
                    slot,
                    fire_cooldown: 0.5 + slot as f32 * 0.2,
                },
                Mesh3d(meshes.add(Cuboid::new(1.1, 0.35, 1.1))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.2, 0.8, 0.9),
                    emissive: Color::srgb(0.05, 0.3, 0.5).into(),
                    perceptual_roughness: 0.3,
                    ..default()
                })),
                Transform::from_translation(pos).with_rotation(pt.rotation),
            ));
        }
    } else if escort_count > fleet_count {
        for (e, _, wingman) in &escorts {
            if wingman.slot >= fleet_count {
                commands.entity(e).despawn();
            }
        }
    }

    // 2. Formation flying and auto-targeting support fire.
    let siren = run.space_campaign.siren_song_active > 0.0;
    let hostiles: Vec<Vec3> = pirates.iter().map(|p| p.translation).collect();
    for (_e, mut t, mut wingman) in &mut escorts {
        if wingman.slot >= run.space_campaign.fleet.len() {
            continue;
        }
        let target_offset = escort_formation_offset(wingman.slot);
        let desired_pos = pt.translation + pt.rotation * target_offset;
        let curr = t.translation;
        t.translation = curr + (desired_pos - curr) * (1.0 - (-6.0 * dt).exp());
        t.rotation = pt.rotation;

        // Support fire at the player's target (0xDBFE10) if not pacified by Siren Song;
        // ADAPTED: what the target is and the reach (`space_tool_use::posse_target`).
        wingman.fire_cooldown -= dt;
        if !siren && wingman.fire_cooldown <= 0.0 {
            if let Some(target) = crate::space_tool_use::posse_target(pt.translation, t.translation, &hostiles, 32.0) {
                if let Some(dir) = aim_dir(t.translation, target) {
                    commands.spawn((
                        StageOwned,
                        Projectile {
                            vel: dir * 42.0,
                            // `allylaser1` 10 per second over the 0.85 s cadence; the Rally
                            // Call multiplies a posse ship's damage too (0xBFCF10).
                            damage: rules::beam_damage_per_tick(rules::ALLY_LASER_DAMAGE[0], 0.85)
                                * crate::space_tool_use::rally_multiplier(&run.space_campaign),
                            life: 1.0,
                            area: 0.0,
                        },
                        Mesh3d(meshes.add(Sphere::new(0.14))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(0.3, 0.9, 1.0),
                            emissive: Color::srgb(0.1, 0.5, 0.9).into(),
                            ..default()
                        })),
                        // The sphere is the hit test; the retail `allylaser1` effects are what shows.
                        Visibility::Hidden,
                        Transform::from_translation(t.translation + dir * 1.0),
                    ));
                    crate::space_tool_fx::request_shot(&mut commands, "allylaser1", t.translation, target);
                    wingman.fire_cooldown = 0.85;
                }
            }
        }
    }
}

fn escort_formation_offset(slot: usize) -> Vec3 {
    match slot {
        0 => Vec3::new(-3.2, 0.3, 2.5),
        1 => Vec3::new(3.2, 0.3, 2.5),
        2 => Vec3::new(-5.5, 0.6, 5.0),
        3 => Vec3::new(5.5, 0.6, 5.0),
        _ => Vec3::new(0.0, 1.8, 3.8),
    }
}

/// Autonomous planetary surface-to-air defense turrets protecting colonies from raiders.
pub fn space_colony_turrets(
    time: Res<Time>,
    view: Option<Res<SpaceView>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    run: Res<RunProgress>,
    mut turrets: Query<(&Transform, &Planet, &crate::persistence::WorldId, &mut ColonyTurret)>,
    pirates: Query<&Transform, (With<Pirate>, Without<Planet>)>,
) {
    if view.as_ref().is_some_and(|v| v.galaxy) {
        return;
    }
    let dt = time.delta_secs();
    let is_siren = run.space_campaign.siren_song_active > 0.0;
    if is_siren {
        return;
    }

    let active_distress = run.space_campaign.active_distress;

    for (planet_t, planet, world_id, mut turret) in &mut turrets {
        if !planet.colonized {
            continue;
        }
        turret.fire_cooldown = (turret.fire_cooldown - dt).max(0.0);
        if turret.fire_cooldown > 0.0 {
            continue;
        }

        // Atmosphere altitude threshold is planet.radius + turret.range.
        let max_dist = planet.radius + turret.range;
        let planet_pos = planet_t.translation;

        let is_raid_target = active_distress.as_ref().map_or(false, |d| {
            d.world_id == world_id.0 && d.kind == DistressKind::PirateRaid
        });

        // Find nearest pirate within atmospheric engagement range
        let mut nearest_pirate: Option<Vec3> = None;
        let mut min_dist_sq = max_dist * max_dist;

        for pirate_t in &pirates {
            let dist_sq = pirate_t.translation.distance_squared(planet_pos);
            if dist_sq <= min_dist_sq {
                min_dist_sq = dist_sq;
                nearest_pirate = Some(pirate_t.translation);
            }
        }

        if let Some(target_pos) = nearest_pirate {
            let to_target = (target_pos - planet_pos).normalize_or_zero();
            let muzzle_pos = planet_pos + to_target * (planet.radius + 0.6);
            let municipal_turrets = run.space_campaign.total_defense_turrets(world_id.0);
            let extra_dmg = (municipal_turrets as f32) * 4.0;
            let cd_reduction = (municipal_turrets as f32 * 0.08).min(0.4);
            if let Some(dir) = aim_dir(muzzle_pos, target_pos) {
                commands.spawn((
                    StageOwned,
                    Projectile {
                        vel: dir * 46.0,
                        // `turretlaser1` 25 per second x `spaceTurretNPCDamageMultiplier`
                        // 0.5 per shot second; ADAPTED +4 per municipal turret.
                        damage: (rules::TURRET_LASER_DAMAGE[0] * rules::TURRET_NPC_DAMAGE_MULTIPLIER + extra_dmg)
                            * crate::space_tool_use::rally_multiplier(&run.space_campaign),
                        life: 0.85,
                        area: 0.0,
                    },
                    Mesh3d(meshes.add(Sphere::new(0.18))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.4, 0.1),
                        emissive: Color::srgb(0.9, 0.2, 0.05).into(),
                        ..default()
                    })),
                    // The sphere is the hit test; the retail `turretlaser1` effects are what shows.
                    Visibility::Hidden,
                    Transform::from_translation(muzzle_pos),
                ));
                crate::space_tool_fx::request_shot(&mut commands, "turretlaser1", muzzle_pos, target_pos);
                let base_cd = if is_raid_target { 0.6 } else { 1.0 };
                turret.fire_cooldown = (base_cd - cd_reduction).max(0.2);
            }
        }
    }
}

/// Beams fly, expire, and damage the pirates they touch.
pub fn space_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    mut projectiles: Query<(Entity, &mut Transform, &mut Projectile), Without<Pirate>>,
    mut pirates: Query<(&Transform, &mut Pirate), Without<Projectile>>,
) {
    let dt = time.delta_secs();
    for (e, mut t, mut p) in &mut projectiles {
        p.life -= dt;
        t.translation += p.vel * dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let hit = pirates.iter().any(|(pt, pirate)| pirate.hp > 0.0 && t.translation.distance(pt.translation) < 1.6);
        if hit {
            // `DefaultProjectile` explodes with the tool's `DamageArea` (the
            // blast of `cDefaultAoEArea`): every ship inside it takes the
            // damage once, the ship that was hit included. ADAPTED: the blast
            // is the full radius at once (`toolProjectileExplosionExpansionRate`
            // grows it over a few frames in retail).
            for (pt, mut pirate) in &mut pirates {
                let d = t.translation.distance(pt.translation);
                if pirate.hp > 0.0 && (d < 1.6 || d <= p.area) {
                    pirate.hp -= p.damage;
                }
            }
            commands.entity(e).despawn();
        }
    }
}

/// Pirates hunt the player, ram the hull, and flee when broken. New ships
/// warp in over time so the sky stays dangerous. A destroyed ship pays one
/// bounty and one mission credit, once.
pub fn space_pirates(
    time: Res<Time>,
    mut timer: ResMut<SpaceTimer>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    (view, system): (Option<Res<SpaceView>>, Option<Res<CurrentSystem>>),
    mut player: Query<&mut Transform, With<Player>>,
    mut pirates: Query<(Entity, &mut Transform, &mut Pirate), Without<Player>>,
    worlds: Query<
        (&Transform, &Planet, &crate::persistence::WorldId),
        (Without<Player>, Without<Pirate>),
    >,
    mut ph: Query<&mut Health, With<Player>>,
    escorts: Query<(&Transform, &EscortWingman), (Without<Player>, Without<Pirate>)>,
    mut run: ResMut<RunProgress>,
) {
    use rand::SeedableRng;
    // Raids hold off while the captain studies the galaxy map or jumps.
    if view.as_ref().is_some_and(|v| v.galaxy) {
        return;
    }
    let extent = system.as_ref().map_or(BOUNDS, |s| s.extent());
    let siren = run.space_campaign.siren_song_active > 0.0;
    let frozen = run.space_campaign.static_cling_active > 0.0;
    // VERIFIED (`space_tool_use`): the cloak is UFO flag 1, which every nearest-UFO
    // search of the NPC behaviours skips (0xCC2820); the shield is `mbInvincible`,
    // the first test of the UFO's `TakeDamage` (0xC3CF29).
    let cloaked = run.space_campaign.cloak_left > 0.0;
    let shielded = run.space_campaign.shield_left > 0.0;
    let is_grox = run.space_campaign.current_system_empire == Some(0);
    timer.spawn.tick(time.delta());
    let pirate_count = pirates.iter().len();
    if timer.spawn.just_finished() && pirate_count < 6 {
        let mut rng = ChaCha8Rng::seed_from_u64(time.elapsed_secs() as u64 + 91);
        let around =
            player
                .single()
                .map(|t| t.translation)
                .unwrap_or(Vec3::new(0.0, 4.0, extent * 0.5));
        if is_grox {
            spawn_grox_interceptor(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut rng,
                around,
                extent,
            );
            info!("Grox interceptor incoming!");
        } else {
            spawn_pirate(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut rng,
                around,
                extent,
            );
            info!("pirate ship incoming!");
        }
    }
    let Ok(pt) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let rank = run.space_campaign.captain_rank();
    // NPC lasers (`enemylaser1..6`, range 50 retail units) fire in 3 s on /
    // 1 s off bursts (`NPCLaserOnTimeSec` / `OffTimeSec`).
    let laser_range = rules::NPC_LASER_RANGE / rules::RETAIL_UNITS_PER_WORLD_UNIT;
    // Fire this frame on each posse ship (applied after the loop: `TakeDamage` 0xC3CEC0).
    let mut escort_fire = vec![0.0_f32; run.space_campaign.fleet.len()];
    for (e, mut t, mut pirate) in &mut pirates {
        let tier = if pirate.is_grox { 5 } else { rules::pirate_tier(rank) };
        if pirate.hp <= 0.0 {
            if !pirate.rewarded {
                pirate.rewarded = true;
                let mut campaign = std::mem::take(&mut run.space_campaign);
                let kind = if pirate.is_grox {
                    rules::LootShip::Interceptor
                } else {
                    rules::LootShip::Raider
                };
                campaign.register_ship_kill(kind, tier, &mut run);
                run.space_campaign = campaign;
            }
            commands.entity(e).despawn();
            info!("enemy ship destroyed!");
            continue;
        }
        let to_p = pt.translation - t.translation;
        let d = to_p.length();
        if siren {
            // Bard Siren Song: hostile ships are pacified and gently drift away (ADAPTED: the
            // retail song only holds the relationships at neutral, 0xD05A60)
            t.translation -= to_p.normalize_or_zero() * pirate.speed * 0.5 * dt;
            continue;
        }
        if frozen || cloaked {
            // Diplomat Static Cling: hostile ships are immobilized and cannot move or attack
            // (the cloak: they cannot find the ship, VERIFIED 0xCC2820)
            continue;
        }
        if pirate_flees(pirate.hp, pirate.max) {
            // Broken engines: run away from the player; escape once clear.
            t.translation -= to_p.normalize_or_zero() * pirate.speed * dt;
            if d > 34.0 {
                commands.entity(e).despawn();
                info!("pirate escaped!");
                continue;
            }
        } else if d < 24.0 && d > 0.01 {
            t.translation += to_p.normalize() * pirate.speed * dt;
        }
        let flat = Vec2::new(t.translation.x, t.translation.z).clamp_length_max(extent);
        t.translation = Vec3::new(flat.x, t.translation.y.clamp(2.0, 14.0), flat.y);
        let burst_clock = time.elapsed_secs() + (e.to_bits() % 7) as f32 * 0.37;
        if rules::npc_laser_on(burst_clock) {
            // A hostile ship fires at the nearest ship of the player's side in range:
            // the posse's ships are UFOs the pirates may hurt (ADAPTED: the target choice
            // of the NPC behaviours, nearest-UFO search 0xCC2820, is not decoded).
            let escort_target = escorts
                .iter()
                .map(|(et, w)| (et.translation.distance(t.translation), w.slot))
                .filter(|(ed, slot)| *ed < laser_range && *slot < escort_fire.len())
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match escort_target {
                Some((ed, slot)) if ed < d || d >= laser_range => {
                    escort_fire[slot] += rules::npc_laser_dps(tier) * dt;
                }
                _ if d < laser_range && !shielded => {
                    if let Ok(mut h) = ph.single_mut() {
                        // Destruction and restore: space_rules::apply_ship_rules.
                        h.hurt(rules::npc_laser_dps(tier) * dt);
                    }
                }
                _ => {}
            }
        }
        let _ = &worlds;
    }
    if escort_fire.iter().any(|f| *f > 0.0) {
        let campaign = &mut run.space_campaign;
        if let Some(news) = crate::space_tool_use::apply_escort_fire(campaign, &escort_fire).pop() {
            campaign.message = news;
        }
    }
}

// Upgrades (health 2..5, lasers 2..3, energy, drives, cargo) are bought from
// the empire stores on the comm screen (`space_comm`); retail has no hotkeys
// for them.

/// The ship's weapon damage multiplier (from its loadout).
#[derive(Component)]
pub struct ShipWeapon(pub f32);

/// Shared flat material for the rare imported part that has no authored
/// diffuse map. Built once per stage entry so rebuilding the mount list never
/// allocates a generic material per module.
#[derive(Resource, Clone)]
pub struct ShipPartFallback(pub Handle<StandardMaterial>);

/// Owned copies of every eligible original vehicle part, in the helper's
/// deterministic order. Cloning frees the catalog borrow so a selection can be
/// held across `mechanical_assets::import(&mut Originals)`.
fn ship_part_options(originals: &Originals) -> Vec<PartDefinition> {
    mechanical_assets::eligible(originals, SHIP_PART_GROUP)
        .into_iter()
        .cloned()
        .collect()
}

/// The authored name of a fitted part, or its model id when the record is no
/// longer in the catalog. Legacy records are reported, never renamed or hidden.
fn ship_part_name(originals: &Originals, instance: u32) -> String {
    ship_part_options(originals)
        .into_iter()
        .find(|d| d.model.instance_id == instance)
        .map(|d| d.name)
        .unwrap_or_else(|| format!("{instance:08X}"))
}

/// Candidate parts in the helper's own cycling order, starting after `after`
/// in `dir`, each unique model identity visited at most once.
///
/// The ring is walked with `mechanical_assets::next`, so duplicate definitions
/// that share one model can never trap the cycle; `limit` bounds the walk to
/// exactly one lap. `skip` removes identities that are already fitted, and the
/// identity in `after` is never offered as its own replacement - so a failed
/// swap cannot "succeed" by re-equipping the part that is already there.
fn ship_part_candidates(
    originals: &Originals,
    after: Option<u32>,
    dir: i32,
    skip: &[u32],
) -> Vec<PartDefinition> {
    let limit = mechanical_assets::eligible(originals, SHIP_PART_GROUP).len() + 1;
    let mut out = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    let mut cursor = after;
    for _ in 0..limit {
        let Some(def) = mechanical_assets::next(originals, SHIP_PART_GROUP, cursor, dir) else {
            break;
        };
        let id = def.model.instance_id;
        // One lap complete, or back at the fitted part: stop.
        if seen.contains(&id) || Some(id) == after {
            break;
        }
        seen.push(id);
        cursor = Some(id);
        if !skip.contains(&id) {
            out.push(def.clone());
        }
    }
    out
}

/// Where a new part goes in the build.
#[derive(Clone, Copy)]
enum EquipTarget {
    /// `N`: append a module mounted on `slot`.
    Append { slot: u32 },
    /// `[`/`]`: replace the module the cursor is focused on.
    ReplaceFocused,
}

/// Try each candidate in order and commit the first one that imports.
///
/// Nothing in `build` is written until an import has succeeded, so a failed
/// part never touches the save; the walk then continues to the next candidate
/// instead of stalling on the broken one. Returns the authored part name.
fn equip_ship_part(
    build: &mut ShipBuild,
    target: EquipTarget,
    candidates: &[PartDefinition],
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<String, String> {
    if matches!(target, EquipTarget::Append { .. }) {
        if build.modules.len() >= MAX_SHIP_MODULES {
            return Err(format!(
                "the ship already holds the {MAX_SHIP_MODULES} part limit"
            ));
        }
    } else if build.modules.is_empty() {
        return Err("no fitted part to replace yet - press N to add one".into());
    }
    if candidates.is_empty() {
        return Err(match target {
            EquipTarget::Append { .. } => "no unmounted original vehicle part is left".to_string(),
            EquipTarget::ReplaceFocused => "no other original vehicle part is available".into(),
        });
    }
    let mut last = String::new();
    let mut tried = 0usize;
    for def in candidates {
        tried += 1;
        match mechanical_assets::import(originals, def, meshes, images, materials) {
            Ok(_) => {
                // The saved loadout changes only now, and only for the part
                // that actually imported.
                let instance = def.model.instance_id;
                match target {
                    EquipTarget::Append { slot } => {
                        build.modules.push(ShipModule::new(instance, slot));
                        build.focused = build.modules.len() as u32 - 1;
                    }
                    EquipTarget::ReplaceFocused => {
                        let fi = focus_index(build.focused, build.modules.len()) as usize;
                        build.modules[fi].instance = instance;
                        build.focused = fi as u32;
                    }
                }
                return Ok(def.name.clone());
            }
            // Keep walking: a broken part must never trap the selection.
            Err(err) => last = format!("{}: {err}", def.name),
        }
    }
    Err(format!(
        "none of the {tried} candidate part(s) could be imported ({last})"
    ))
}

/// Re-import every saved module so a fresh process renders it: startup only
/// scans a handful of preview meshes, so a reloaded build is not in the cache.
///
/// Modules whose definition cannot be resolved are reported and **kept** - the
/// player's build data is never deleted to make an error go away.
fn import_saved_ship_parts(
    originals: &mut Originals,
    build: &ShipBuild,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<String> {
    let options = ship_part_options(originals);
    let mut report = Vec::new();
    for m in &build.modules {
        match options.iter().find(|d| d.model.instance_id == m.instance) {
            Some(def) => {
                if mechanical_assets::import(originals, def, meshes, images, materials).is_err() {
                    report.push(format!(
                        "{:08X} ({}) could not be imported",
                        m.instance, def.name
                    ));
                }
            }
            None => report.push(format!(
                "{:08X} is not an original vehicle part (kept in the build)",
                m.instance
            )),
        }
    }
    report
}

/// Mounted size: the part's largest axis stays within
/// `SHIP_MODULE_WORLD_CAP` world units at the default player scale, while
/// parts under the cap keep their authored relative scale exactly. The helper
/// already bounds the model itself; this adds the slot-level fraction.
fn module_scale(meshes: &Assets<Mesh>, model: &OriginalModel, player_scale: f32) -> f32 {
    mechanical_assets::capped_scale(meshes, model, SHIP_MODULE_WORLD_CAP / MODULE_BASE_FRACTION)
        * MODULE_BASE_FRACTION
        * player_scale
}

/// Spawn one child per loadout module, placed by its slot.
///
/// Parts resolve through `mechanical_assets::cached`, so a creature or preview
/// mesh can never be shown as a ship part, and the authored diffuse/tint
/// material is used whenever the part has one (the shared fallback is only
/// touched by parts that do not). Returns the module ids that could not be
/// resolved - those records stay in the build and are reported by the HUD.
fn mount_ship_modules(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    meshes: &Assets<Mesh>,
    fallback: Option<&Handle<StandardMaterial>>,
    ship: Entity,
    originals: &Originals,
    run: &RunProgress,
) -> Vec<u32> {
    let mut unsupported = Vec::new();
    let mut generic: Option<Handle<StandardMaterial>> = None;
    // The UFO design is the whole ship (`rigid_editor::ship_design_fit`).
    let (design_scale, design_lift) = crate::rigid_editor::ship_design_fit(meshes, originals, &run.ship.modules)
        .unwrap_or((crate::rigid_editor::SHIP_STAGE_SCALE, Vec3::ZERO));
    for (i, m) in run.ship.modules.iter().enumerate() {
        if let Some(block) = &m.block {
            // A block of the UFO editor (`rigid_editor`): its free transform
            // scaled to the ship, every authored surface drawn.
            let Some(model) = crate::rigid_editor::block_model(originals, SHIP_PART_GROUP, m.definition_id, m.instance, &block.handles) else {
                unsupported.push(m.instance);
                continue;
            };
            let model = &model;
            let unit = crate::rigid_editor::editor_unit(model) * m.scale.clamp(0.1, 4.0);
            let entity = commands
                .spawn((
                    StageOwned,
                    ShipModuleRef { module: i },
                    Visibility::Inherited,
                    // The design's front (+Z in the editor) faces the
                    // ship's flight direction (-Z): turned half a turn.
                    Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                        * crate::rigid_editor::block_transform(block, unit, design_scale, design_lift),
                ))
                .id();
            crate::rigid_editor::spawn_surfaces(commands, entity, model, materials, None, &block.paints);
            commands.entity(ship).add_child(entity);
            continue;
        }
        let Some(model) = mechanical_assets::cached(originals, SHIP_PART_GROUP, m.instance) else {
            unsupported.push(m.instance);
            continue;
        };
        let material = if let Some(authored) = &model.material {
            authored.clone()
        } else if let Some(h) = fallback {
            h.clone()
        } else {
            generic
                .get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.6, 0.7, 0.85),
                        perceptual_roughness: 0.4,
                        ..default()
                    })
                })
                .clone()
        };
        let (off, yaw) = ship_module_transform(m.slot, m);
        let scale = module_scale(meshes, model, m.scale);
        commands.entity(ship).with_children(|parent| {
            parent.spawn((
                StageOwned,
                ShipModuleRef { module: i },
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(material),
                Transform::from_translation(off)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(scale)),
            ));
        });
    }
    unsupported
}

/// Refresh the live ship model and loadout-derived stats after a modal creator
/// edit. The saved `ShipBuild` is the only source of changes; this system does
/// not process input or mutate the recipe. Root schedules it only while the
/// mechanical creator is open.
#[allow(clippy::too_many_arguments)]
pub fn sync_creator_loadout(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    meshes: Res<Assets<Mesh>>,
    fallback: Option<Res<ShipPartFallback>>,
    originals: Res<Originals>,
    run: Res<RunProgress>,
    modules: Query<Entity, With<ShipModuleRef>>,
    mut ship: Query<(Entity, &mut Mover, &mut Health, &mut ShipWeapon), With<Player>>,
    mut previous: Local<Option<(ShipBuild, Entity)>>,
) {
    let Ok((ship_entity, mut mover, mut health, mut weapon)) = ship.single_mut() else {
        return;
    };
    let changed = previous
        .as_ref()
        .is_none_or(|(build, entity)| build != &run.ship || *entity != ship_entity);
    if !changed {
        return;
    }

    let (weapon_mult, extra_hull, extra_speed) = ship_bonus(&run.ship);
    mover.speed = 16.0 * (0.9 + run.creature_stats().speed / 40.0) + extra_speed;
    health.max = hull_max_hp(run.hull) + (run.creature_stats().max_hp - 120.0) * 0.25 + extra_hull;
    health.hp = health.hp.min(health.max);
    weapon.0 = weapon_mult;

    for entity in &modules {
        commands.entity(entity).despawn();
    }
    let unsupported = mount_ship_modules(
        &mut commands,
        &mut materials,
        &meshes,
        fallback.as_ref().map(|resource| &resource.0),
        ship_entity,
        &originals,
        &run,
    );
    for instance in unsupported {
        info!(
            "ship part {instance:08X} could not be rendered after creator sync (kept in the loadout)"
        );
    }
    *previous = Some((run.ship.clone(), ship_entity));
}

/// A mounted ship module (child of the UFO).
#[derive(Component)]
pub struct ShipModuleRef {
    pub module: usize,
}

/// Ship loadout editor — the same key grammar as the creature/civ assemblers:
/// `[`/`]` module part, `,`/`.` slot, `Tab` focus, `N` add, `Backspace`
/// remove, `R` rotate, `-`/`=` scale, `IJKL`/`UO` slide.
///
/// Every part offered is an authored **vehicle** part from the original
/// catalog, imported through `mechanical_assets` before it is written into
/// the build, so a failed import can never be equipped and a broken candidate
/// can never trap a key. Persisted; bonuses re-apply to the live ship.
pub fn space_ship_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    fallback: Option<Res<ShipPartFallback>>,
    modules: Query<Entity, With<ShipModuleRef>>,
    mut ship: Query<(Entity, &mut Mover, &mut Health, &mut ShipWeapon), With<Player>>,
    controls: Option<Res<crate::space_controls::SpaceControls>>,
) {
    // The retail camera owns `,` `.` `-` `=` and Backspace: the loadout keys
    // that share them need Ctrl+Alt held.
    let clash_ok = controls.is_none() || crate::space_controls::debug_chord(&keys);
    let n_slots = SHIP_SLOTS.len() as u32;
    let mut changed = false;
    let mut message: Option<String> = None;
    if keys.just_pressed(KeyCode::Tab) && !run.ship.modules.is_empty() {
        run.ship.focused = (run.ship.focused + 1) % run.ship.modules.len() as u32;
        changed = true;
    }
    // N — the next unmounted original vehicle part. The candidate order comes
    // from the helper (deterministic, aliases collapsed), and the import walk
    // inside `equip_ship_part` skips any candidate that fails, so an
    // unsupported first model can never trap an empty build on `N`.
    if keys.just_pressed(KeyCode::KeyN) {
        let mounted: Vec<u32> = run.ship.modules.iter().map(|m| m.instance).collect();
        let candidates = ship_part_candidates(&originals, mounted.last().copied(), 1, &mounted);
        let slot = run.ship.modules.len() as u32 % n_slots;
        message = Some(
            match equip_ship_part(
                &mut run.ship,
                EquipTarget::Append { slot },
                &candidates,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
            ) {
                Ok(name) => {
                    changed = true;
                    format!("Fitted part: {name}")
                }
                Err(err) => err,
            },
        );
    }
    // [ / ] — replace the focused part, continuing through the catalog in
    // `dir`. The fitted model is never offered as its own replacement, so a
    // failed swap reports instead of silently re-equipping the same part.
    if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::BracketRight) {
        let dir = if keys.just_pressed(KeyCode::BracketRight) {
            1
        } else {
            -1
        };
        let fi = focus_index(run.ship.focused, run.ship.modules.len()) as usize;
        let current = run.ship.modules.get(fi).map(|m| m.instance);
        let candidates = ship_part_candidates(&originals, current, dir, &[]);
        message = Some(
            match equip_ship_part(
                &mut run.ship,
                EquipTarget::ReplaceFocused,
                &candidates,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
            ) {
                Ok(name) => {
                    changed = true;
                    format!("Equipped part: {name}")
                }
                Err(err) => err,
            },
        );
    }
    if clash_ok && keys.just_pressed(KeyCode::Backspace) && !run.ship.modules.is_empty() {
        let i = focus_index(run.ship.focused, run.ship.modules.len()) as usize;
        run.ship.modules.remove(i);
        run.ship.focused = run
            .ship
            .focused
            .min(run.ship.modules.len().saturating_sub(1) as u32);
        changed = true;
    }
    let fi = focus_index(run.ship.focused, run.ship.modules.len()) as usize;
    if fi < run.ship.modules.len() {
        let m = &mut run.ship.modules[fi];
        if clash_ok && (keys.just_pressed(KeyCode::Comma) || keys.just_pressed(KeyCode::Period)) {
            let dir = if keys.just_pressed(KeyCode::Period) {
                1
            } else {
                -1
            };
            m.slot = cycle_bone(m.slot, dir, n_slots);
            changed = true;
        }
        if keys.just_pressed(KeyCode::KeyR) {
            m.yaw = wrap_yaw(m.yaw + std::f32::consts::FRAC_PI_4);
            changed = true;
        }
        if clash_ok && keys.just_pressed(KeyCode::Minus) {
            m.scale = scaled(m.scale, 0.9);
            changed = true;
        }
        if clash_ok && keys.just_pressed(KeyCode::Equal) {
            m.scale = scaled(m.scale, 1.1);
            changed = true;
        }
        if apply_slide_offset(&keys, &mut m.offset) {
            changed = true;
        }
    }
    // Record feedback even when the build did not move, so a refused part or
    // a failed import is explained rather than silently swallowed.
    if let Some(text) = message {
        info!("{text}");
        run.space_campaign.ship_message = text;
    }
    if !changed {
        return;
    }
    run.request_save();
    // Re-apply bonuses + respawn the module visuals.
    let (weapon_mult, extra_hull, extra_speed) = ship_bonus(&run.ship);
    if let Ok((_, mut mover, mut health, mut weapon)) = ship.single_mut() {
        mover.speed = 16.0 * (0.9 + run.creature_stats().speed / 40.0) + extra_speed;
        health.max =
            hull_max_hp(run.hull) + (run.creature_stats().max_hp - 120.0) * 0.25 + extra_hull;
        health.hp = health.hp.min(health.max);
        weapon.0 = weapon_mult;
    }
    for e in &modules {
        commands.entity(e).despawn();
    }
    if let Ok((ship_ent, _, _, _)) = ship.single() {
        let unsupported = mount_ship_modules(
            &mut commands,
            &mut materials,
            &meshes,
            fallback.as_ref().map(|f| &f.0),
            ship_ent,
            &originals,
            &run,
        );
        for instance in &unsupported {
            info!(
                "ship part {:08X} could not be rendered (kept in the loadout)",
                instance
            );
        }
    }
    let fi = focus_index(run.ship.focused, run.ship.modules.len()) as usize;
    if let Some(m) = run.ship.modules.get(fi) {
        let slot_name = SHIP_SLOTS
            .get(m.slot as usize)
            .map(|s| s.name)
            .unwrap_or("?");
        info!(
            "ship module[{fi}] {} ({:#X}) slot {} (weapon x{:.2}, +{:.0} hull, +{:.1} speed)",
            ship_part_name(&originals, m.instance),
            m.instance,
            slot_name,
            weapon_mult,
            extra_hull,
            extra_speed
        );
    }
}

/// Extra HUD block for the ship loadout, meant to be appended by the stage
/// HUD while the ship editor is available: hull occupancy, the focused part's
/// **authored** name and slot, the control grammar, and any saved record the
/// original vehicle catalog cannot supply.
///
/// Always returns at least the occupancy line and the controls, so a brand
/// new player can discover `N` before anything is fitted. Kept to short
/// single-line rows so the block stays compact in the top-left corner.
pub fn ship_editor_hud(run: &RunProgress, originals: &Originals) -> String {
    let build = &run.ship;
    let feedback = &run.space_campaign.ship_message;
    let mut lines = Vec::new();
    let fi = focus_index(build.focused, build.modules.len()) as usize;
    if build.modules.is_empty() {
        lines.push(format!("Ship: 0/{MAX_SHIP_MODULES} parts"));
    } else {
        lines.push(format!(
            "Ship: {}/{} parts | focus {}/{}",
            build.modules.len(),
            MAX_SHIP_MODULES,
            fi + 1,
            build.modules.len()
        ));
        if let Some(m) = build.modules.get(fi) {
            let slot = SHIP_SLOTS
                .get(m.slot as usize)
                .map(|s| s.name)
                .unwrap_or("?");
            lines.push(format!(
                "Fitted: {} ({}) scale {:.2}",
                ship_part_name(originals, m.instance),
                slot,
                m.scale
            ));
        }
    }
    lines.push("Controls: N add | [ ] part | , . slot | Tab focus | Backspace remove".to_string());
    lines.push("Transform: R rotate | - = scale | IJKL/UO slide".to_string());
    // Saved records the catalog cannot resolve are explained, never hidden.
    let options = ship_part_options(originals);
    let unsupported: Vec<String> = build
        .modules
        .iter()
        .filter(|m| !options.iter().any(|d| d.model.instance_id == m.instance))
        .map(|m| format!("{:#X}", m.instance))
        .collect();
    if !unsupported.is_empty() {
        lines.push(format!(
            "Not an original vehicle part (kept exactly as saved): {}",
            unsupported.join(", ")
        ));
    }
    if !feedback.is_empty() {
        lines.push(feedback.clone());
    }
    lines.join("\n")
}

pub fn colonies_for_intro_mission() -> u32 {
    COLONIES_FOR_INTRO_MISSION
}

/// What a world is to the player right now, derived only from stable WorldIds.
struct WorldView {
    id: u32,
    position: Vec3,
    radius: f32,
    colonized: bool,
    surveyed: bool,
}

impl WorldView {
    fn distance_to(&self, point: Vec3) -> f32 {
        (point - self.position).length()
    }
    /// Within "standing over it" reach of the world.
    fn in_reach(&self, point: Vec3) -> bool {
        self.distance_to(point) < self.radius + ACTION_RANGE
    }
}

/// How near the ship must be to act on a world with V/T/M.
const ACTION_RANGE: f32 = 6.0;

/// Reach beyond a world's radius within which the ship can act on it.
pub fn action_range() -> f32 {
    ACTION_RANGE
}

/// Shared world tint materials. Built once in `setup_space` so the per-frame
/// sync in `space_campaign_update` only compares handles and never allocates.
#[derive(Resource, Clone)]
pub struct WorldTints {
    pub owned: Handle<StandardMaterial>,
    pub surveyed: Handle<StandardMaterial>,
}

/// Snapshot the worlds the ship is near, sorted nearest first.
fn nearby_worlds(worlds: &[WorldView], point: Vec3) -> Vec<&WorldView> {
    let mut near: Vec<&WorldView> = worlds.iter().filter(|w| w.in_reach(point)).collect();
    near.sort_by(|a, b| a.distance_to(point).total_cmp(&b.distance_to(point)));
    near
}

/// Owns the campaign systems: T terraform, M mission claims, docking/refuel,
/// bounded colony income and world-identity reconciliation. Space stays playable
/// after every introductory mission is claimed.
/// Runs last in the stage's simulation chain.
pub fn space_campaign_update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Option<Res<crate::space_view::SpaceView>>,
    mut run: ResMut<RunProgress>,
    player: Query<&Transform, With<Player>>,
    tints: Option<Res<WorldTints>>,
    mut commands: Commands,
    mut planets: Query<
        (
            Entity,
            &crate::persistence::WorldId,
            &Transform,
            &mut Planet,
            &mut MeshMaterial3d<StandardMaterial>,
            Option<&crate::space_visuals::PlanetSurface>,
            Option<&ColonyTurret>,
        ),
        Without<Player>,
    >,
    (mut console_roots, mut bullseye_dots, mut console_texts): (
        Query<&mut Visibility, (With<SpaceConsoleRoot>, Without<Player>)>,
        Query<&mut Node, (With<SpaceBullseyeDot>, Without<SpaceConsoleRoot>)>,
        Query<&mut Text, (With<SpaceConsoleText>, Without<SpaceConsoleRoot>)>,
    ),
) {
    // The ship's old system position remains during the jump transition.
    // It cannot dock or recharge there while travelling to another star.
    if view.as_ref().is_some_and(|view| view.travel.is_some()) {
        return;
    }
    let Ok(pt) = player.single() else { return };
    let dt = time.delta_secs();
    let worlds: Vec<WorldView> = planets
        .iter()
        .map(|(_, id, t, p, _, _, _)| WorldView {
            id: id.0,
            position: t.translation,
            radius: p.radius,
            colonized: p.colonized,
            surveyed: p.surveyed,
        })
        .collect();

    // Move the campaign out so the actions below can borrow the rest of the run.
    let mut campaign = std::mem::take(&mut run.space_campaign);
    let mut save = campaign.migrate_ecology();

    // --- reconcile world identities by WorldId ---
    // `persistence::restore` runs after `setup_space`, so a checkpoint may own
    // or mark worlds the campaign has not seen yet. Adopt them instead of
    // losing the world's progress; the campaign otherwise stays canonical.
    for w in &worlds {
        if w.colonized && !campaign.colonies.contains(&w.id) {
            campaign.own(w.id);
            save = true;
        } else if w.surveyed
            && !campaign.colonies.contains(&w.id)
            && !campaign.surveyed.contains(&w.id)
        {
            push_unique(&mut campaign.surveyed, w.id);
            save = true;
        }
        let index = campaign.ensure_ecology(w.id);
        if w.colonized && campaign.ecology[index].colony_count == 0 {
            campaign.ecology[index].colony_count = 1;
            save = true;
        }
    }
    // The legacy colony counter always mirrors the campaign.
    let colonies = campaign.new_colony_count();
    if run.colonies != colonies {
        run.colonies = colonies;
        save = true;
    }

    // --- docking, refuelling and colony income ---
    let docked = worlds
        .iter()
        .filter(|w| campaign.colonies.contains(&w.id))
        .filter(|w| w.in_reach(pt.translation))
        .min_by(|a, b| {
            a.distance_to(pt.translation)
                .total_cmp(&b.distance_to(pt.translation))
        });
    campaign.tick(dt, docked.is_some(), &mut run.spice, &mut save);
    // The homeworld and the colonies sell repair and recharge on the comm
    // screen (K, or the Repair / Recharge tabs): 0xFFD120 / 0x1001840 at the
    // owner's per-point price, free at the homeworld (S2C cost index 0). There
    // is no automatic top-up while hovering.
    if let Some(world) = docked {
        let known = campaign.docked.len();
        let complete = campaign.dock(world.id);
        if campaign.docked.len() != known {
            // A new trade route is a real change: persist it and tell the
            // player, instead of only acknowledging the third one.
            save = true;
            campaign.message = complete.unwrap_or_else(|| {
                format!(
                    "Trade route opened ({}/{TRADE_TARGET})",
                    campaign.docked.len()
                )
            });
        }
    }

    // --- T cycles the planetary tool; Y applies it to the nearest world ---
    // Y held repeats the use at the tool's `AutoFireRate` (the hold handler
    // 0x1050500 calls `cSpaceToolData::Use` 0x1050080 again once the auto-fire
    // timer has run out; a negative rate is one use per press); each use
    // needs `EnergyCost` in the reactor (the gate of `TryUse`, 0x1050C4F) and
    // spends it (0x10504A1).
    if keys.just_pressed(KeyCode::KeyT) {
        campaign.cycle_planetary_tool();
        save = true;
    }
    {
        let selected = campaign.planetary_tool;
        let tool = crate::space_tool_fx::planetary_tool_id(selected);
        let spec = rules::tool_use(tool);
        let fire = campaign.tool_clocks[selected.index()].step(
            spec.autofire,
            keys.pressed(KeyCode::KeyY),
            keys.just_pressed(KeyCode::KeyY),
            dt,
        );
        if fire != rules::ToolFire::None {
            if let Some(world) = nearby_worlds(&worlds, pt.translation).first() {
                let cost = rules::tool_use_energy(spec.energy, spec.autofire, dt);
                if !rules::tool_can_afford_energy(campaign.energy, cost) {
                    campaign.message = format!("Not enough energy for the {} (recharge at a planet)", selected.label());
                } else {
                    // The press is the first use and the held repeats are more
                    // of the same (`TryUse` 0x1050BC0, hold handler 0x1050500);
                    // a ray's use is one `OnHit` on the planet's marble.
                    let action = if selected.is_ray() {
                        campaign.try_ray_repeat(world.id, &mut run)
                    } else {
                        campaign.try_apply_planetary_tool(world.id, &mut run)
                    };
                    let succeeded = matches!(action, WorldAction::Done(_));
                    campaign.apply(action);
                    save |= succeeded;
                    if succeeded {
                        campaign.energy = (campaign.energy - cost).max(0.0);
                        campaign.tool_clocks[selected.index()].commit(spec.recharge);
                        if spec.autofire >= 0.0 {
                            crate::space_tool_fx::request_held(&mut commands, tool, Some(world.id));
                        } else {
                            crate::space_tool_fx::request(&mut commands, tool, Some(world.id));
                        }
                    }
                }
            }
        }
    }

    // --- the terraform marble rolls on the world the ship is over (0xC7FC50) ---
    if let Some(world) = nearby_worlds(&worlds, pt.translation).first() {
        if campaign.step_marble(world.id, dt.min(0.25), &mut run) {
            save = true;
        }
    }

    // --- B collects spice from owned colonies or trades with alien empires ---
    if keys.just_pressed(KeyCode::KeyB) {
        if let Some(world) = nearby_worlds(&worlds, pt.translation).first() {
            let action = campaign.try_collect_or_trade_spice(world.id, &mut run);
            let succeeded = matches!(action, WorldAction::Done(_));
            campaign.apply(action);
            save |= succeeded;
        }
    }

    // --- K opens diplomatic comms with an alien empire ---
    if keys.just_pressed(KeyCode::KeyK) {
        if let Some(world) = nearby_worlds(&worlds, pt.translation).first() {
            let action = campaign.try_communicate(world.id, &mut run);
            let succeeded = matches!(action, WorldAction::Done(_));
            campaign.apply(action);
            save |= succeeded;
        }
    }

    // --- M claims the finished mission next to the homeworld ---
    if keys.just_pressed(KeyCode::KeyM) {
        let near_home = worlds
            .iter()
            .any(|w| w.id == 0 && w.in_reach(pt.translation));
        let action = campaign.try_claim_mission(near_home, &mut run);
        campaign.apply(action);
        save = true;
    }

    if save {
        run.request_save();
    }

    // --- push the canonical state back onto the world entities ---
    // The checkpoint stores these flags, so the minimap, the visible ownership
    // colours and the save must all agree with the campaign.
    for (entity, id, _, mut planet, mut material, surface, turret) in &mut planets {
        let owned_here = campaign.colonies.contains(&id.0) || id.0 == 0;
        let scanned_here = owned_here || campaign.surveyed.contains(&id.0);
        if planet.colonized != owned_here {
            planet.colonized = owned_here;
            save = true;
        }
        if owned_here && turret.is_none() {
            commands.entity(entity).insert(ColonyTurret::default());
        } else if !owned_here && turret.is_some() {
            commands.entity(entity).remove::<ColonyTurret>();
        }
        if planet.surveyed != scanned_here {
            planet.surveyed = scanned_here;
            save = true;
        }
        // Only assign the shared handles when the state actually changed, so a
        // steady frame does no work and never allocates a material.
        let want = tints.as_ref().and_then(|t| {
            if owned_here {
                Some(t.owned.clone())
            } else if scanned_here {
                Some(t.surveyed.clone())
            } else {
                None
            }
        });
        if let Some(want) = want.filter(|_| surface.is_none()) {
            if material.0 != want {
                material.0 = want;
            }
        }
    }

    // --- update Terraforming T-Score Bullseye and Food-Web / Spice Console ---
    let in_galaxy = view.as_ref().is_some_and(|v| v.galaxy);
    for mut root_vis in &mut console_roots {
        let want = if in_galaxy {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *root_vis != want {
            *root_vis = want;
        }
    }
    if !in_galaxy {
        let focused_world = nearby_worlds(&worlds, pt.translation)
            .first()
            .copied()
            .or_else(|| {
                worlds.iter().min_by(|a, b| {
                    a.distance_to(pt.translation)
                        .total_cmp(&b.distance_to(pt.translation))
                })
            });

        if let Some(w) = focused_world {
            let ecology = campaign.ecology_for(w.id);
            let t_score = ecology.t_score();
            let spice = SpiceColor::for_world(w.id);
            let temp = ecology.temperature.clamp(0.0, 1.0);
            let atmos = ecology.atmosphere.clamp(0.0, 1.0);

            for mut dot in &mut bullseye_dots {
                dot.left = Val::Px(temp * 84.0 + 10.0 - 4.0);
                dot.top = Val::Px((1.0 - atmos) * 84.0 + 10.0 - 4.0);
            }

            for mut text in &mut console_texts {
                let name = planets
                    .iter()
                    .find(|(_, id, ..)| id.0 == w.id)
                    .map(|(_, _, _, p, ..)| p.name.as_str())
                    .unwrap_or("World");

                let owner_str = if w.id == 0 {
                    "Homeworld [Your Empire]".to_string()
                } else if campaign.colonies.contains(&w.id) {
                    format!("Colony ({}/3 Cities)", ecology.colony_count)
                } else if let Some(e) = campaign.world_empire(w.id) {
                    let rel = campaign.relation_label(e);
                    let hp = campaign.empire_defense_hp(w.id);
                    let ally_tag = if campaign.allies.contains(&e) { " [ALLY]" } else { "" };
                    format!(
                        "{} Empire [{}, Def {:.0}]{ally_tag}",
                        crate::space_galaxy::EMPIRES[e as usize].0,
                        rel,
                        hp
                    )
                } else {
                    "Unclaimed World".to_string()
                };

                let foodweb_str = format!(
                    "Food-Web T{}: P{}/9 H{}/6 C{}/3 (Tier {})",
                    t_score,
                    ecology.flora,
                    ecology.herbivores,
                    ecology.carnivores,
                    ecology.stable_foodweb_tier()
                );

                let stored = campaign.colony_spice_stored(w.id);
                let cap = campaign.colony_spice_capacity(w.id);
                let is_empire = campaign.world_empire(w.id).is_some();
                let price = spice.trade_price_at(w.id, is_empire);
                let spice_str = format!(
                    "{} Spice: {}/{} stored (sells for {:.0})",
                    spice.label(),
                    stored,
                    cap,
                    price
                );

                let cargo_str = format!(
                    "Cargo: {}/{} slots [Spice: {}, Wild: P{} H{} C{}] • Vault: {} relic{}",
                    campaign.cargo_slots_used(),
                    campaign.cargo_capacity_limit(),
                    campaign.cargo_spice_total(),
                    campaign.cargo_wildlife.flora,
                    campaign.cargo_wildlife.herbivores,
                    campaign.cargo_wildlife.carnivores,
                    campaign.artifacts.len(),
                    if campaign.artifacts.len() == 1 { "" } else { "s" }
                );

                let rank_str = format!(
                    "Rank: {} ({}/10) • {} badge pts • Fleet: {}/{}",
                    campaign.captain_rank_title(),
                    campaign.captain_rank(),
                    campaign.total_badge_points(),
                    campaign.fleet.len(),
                    campaign.max_fleet_size(),
                );

                let power_str = if campaign.superweapon_cooldown > 0.0 {
                    format!(
                        "Power [R]: {} ({:.0}s CD)",
                        campaign.archetype.superpower_name(),
                        campaign.superweapon_cooldown
                    )
                } else {
                    format!("Power [R]: {} (READY)", campaign.archetype.superpower_name())
                };

                let tool_str = format!(
                    "Tool: {} [T/Y] • {} • Beam [U] • Trade [B] • Comms [K]{}",
                    campaign.planetary_tool.label(),
                    power_str,
                    if campaign.staff_of_life_charges > 0 {
                        format!(" (Life: {}x)", campaign.staff_of_life_charges)
                    } else {
                        String::new()
                    }
                );

                let sos_str = if let Some(ref d) = campaign.active_distress {
                    match d.kind {
                        DistressKind::PirateRaid => format!(
                            "\n[! SOS: Pirate raid on world {} ({:.0}s) !]",
                            d.world_id,
                            d.time_remaining.max(0.0)
                        ),
                        DistressKind::BiohazardOutbreak => format!(
                            "\n[! SOS: Biohazard on world {} ({:.0}s) !]",
                            d.world_id,
                            d.time_remaining.max(0.0)
                        ),
                    }
                } else {
                    String::new()
                };

                let contract_str = if let Some(ref m) = campaign.active_alien_mission {
                    let emp = crate::space_galaxy::EMPIRES
                        .get(m.empire as usize)
                        .map(|e| e.0)
                        .unwrap_or("Alien");
                    let ready = if m.is_complete(&campaign) { " READY" } else { "" };
                    format!(
                        "\nContract: {} [{emp}] ({}/{}) — {:.0} Sporebucks{ready}",
                        m.kind.title(),
                        m.current_progress(&campaign),
                        m.target_count,
                        m.reward_spice
                    )
                } else {
                    String::new()
                };

                let updated = format!(
                    "{} • T{}\n{}\n{}\n{}\n{}\n{}\n{}{}{}",
                    name, t_score, owner_str, foodweb_str, spice_str, cargo_str, rank_str, tool_str, sos_str, contract_str
                );
                if text.0 != updated {
                    text.0 = updated;
                }
            }
        }
    }

    if save {
        run.request_save();
    }
    run.space_campaign = campaign;
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::dbpf::{DbpfEntry, ResourceKey};
    use std::path::PathBuf;

    #[test]
    fn survey_and_flee_thresholds() {
        // Payout scales with world size and stays bounded.
        assert!((survey_reward(4.0) - 16.0).abs() < 1e-5);
        assert_eq!(survey_reward(0.0), 8.0);
        assert_eq!(survey_reward(1000.0), 40.0);
        // Pirates bail below 30% hull.
        assert!(pirate_flees(17.9, 60.0));
        assert!(!pirate_flees(18.0, 60.0));
    }

    #[test]
    fn projectile_flight() {
        // Pure integration of the velocity used by space_projectiles.
        let mut pos = Vec3::ZERO;
        let vel = Vec3::new(3.0, 0.0, 4.0); // 5 u/s
        let dt = 0.5;
        let mut life = 1.2;
        life -= dt;
        pos += vel * dt;
        assert!((pos.length() - 2.5).abs() < 1e-5);
        assert!((life - 0.7).abs() < 1e-5);
    }

    // ------------------------------------------------------------------
    // Space campaign
    // ------------------------------------------------------------------

    /// A stage index for Space, to build checkpoints in tests.
    const SPACE_STAGE: usize = 4;

    fn object_checkpoint(
        id: u32,
        colonized: bool,
        surveyed: bool,
    ) -> crate::persistence::ObjectCheckpoint {
        crate::persistence::ObjectCheckpoint {
            rotation: None,
            wander: None,
            creature: None,
            id,
            position: [0.0, 0.0, 0.0],
            food_left: None,
            spice_left: None,
            planet: Some((colonized, surveyed)),
            hp: None,
        }
    }

    #[test]
    fn every_world_has_a_finite_terraform_cost() {
        // The legacy terraform helper: every world can be terraformed at some
        // (now Sporebuck) cost; no world is a permanent dead end.
        for id in 1..=WORLD_COUNT {
            let cost = terraform_cost(id);
            assert!(cost.is_finite() && cost >= TERRAFORM_COST, "world {id}");
            assert!(cost <= TERRAFORM_MAX_COST, "world {id} costs {cost}");
        }
        assert_eq!(terraform_suitability(3), terraform_suitability(3));
        assert!((1..=WORLD_COUNT)
            .map(terraform_suitability)
            .all(|s| (50.0..=95.0).contains(&s)));
        let poor = (1..=WORLD_COUNT)
            .find(|id| terraform_suitability(*id) < TERRAFORM_MIN)
            .expect("the mix should include hard worlds");
        assert!(terraform_cost(poor) > TERRAFORM_COST);
    }

    /// A campaign that adopted the retail rules as a new Space game.
    fn retail_campaign(run: &mut RunProgress) -> SpaceCampaign {
        let mut c = SpaceCampaign::default();
        assert!(c.adopt_retail_rules(&mut run.spice, rules::StageCards::default(), 1, 0, 0));
        c.own(0);
        c
    }

    #[test]
    fn a_new_space_game_gets_the_retail_wallet_tools_and_archetype() {
        let mut run = RunProgress::default();
        run.spice = 7_777.0; // Civ money does not carry over (S1B).
        let c = retail_campaign(&mut run);
        assert_eq!(run.spice, rules::STARTING_MONEY);
        for t in rules::starting_tools() {
            assert!(c.has_tool(&t), "{t}");
        }
        assert!(!c.has_tool(tool::INTERPLANETARY_DRIVE));
        assert!(!c.has_tool(tool::INTERSTELLAR_DRIVE));
        assert_eq!(c.max_jump_distance(), 0.0, "no jumps before Story 201");
        assert_eq!(c.archetype, SpaceArchetype::Wanderer, "no cards -> Wanderer");
        assert_eq!(c.max_reactor_energy(), 100.0);
        assert_eq!(c.energy, 100.0);
        assert_eq!(c.max_hull(), 1000.0);
        assert_eq!(c.tutorial.step, rules::TutorialStep::Flight101VisitCities);
        assert_eq!(c.captain_rank_title(), "Captain");
        // Adopting again changes nothing (the save is already retail).
        let mut again = c.clone();
        let mut wallet = run.spice;
        assert!(!again.adopt_retail_rules(&mut wallet, rules::StageCards::default(), 1, 0, 0));
        assert_eq!(wallet, rules::STARTING_MONEY);
    }

    #[test]
    fn an_old_space_save_keeps_its_progress_under_the_retail_rules() {
        let mut run = RunProgress::default();
        run.spice = 300.0;
        let mut c = SpaceCampaign::default();
        c.fuel_initialised = true; // it played Space before the retail rules
        c.energy = 80.0;
        c.empire_relations.insert(2, -50);
        c.colonies = vec![0, 1];
        let cards = rules::StageCards {
            civ: Some(rules::CardColor::Green),
            ..Default::default()
        };
        assert!(c.adopt_retail_rules(&mut run.spice, cards, 2, 2, 1));
        assert_eq!(run.spice, 300.0 + rules::STARTING_MONEY);
        assert!(c.has_tool("healthupgrade3"), "hull level 2 -> health upgrade 3");
        assert!(c.has_tool(tool::LASER_2), "weapon level 1 -> laser 2");
        assert!(c.has_tool(tool::INTERSTELLAR_DRIVE));
        assert!(c.missions_finished(), "the old intro let it travel freely");
        assert_eq!(c.archetype, SpaceArchetype::Shaman, "civ religious only -> Shaman");
        assert!((c.relationship(2) - -5.0).abs() < 1e-4, "-50 -> -5");
        assert_eq!(c.energy, 80.0);
        assert!((c.max_hull() - 3.0 * 1000.0 * 0.3).abs() < 1e-3, "hard difficulty");
    }

    #[test]
    fn a_fresh_run_plays_flight101_story201_and_the_teaching_missions() {
        let mut run = RunProgress::default();
        let mut c = retail_campaign(&mut run);
        // Flight 101: visit the cities (M at home), scan, fetch, eradicate.
        assert!(matches!(c.try_claim_mission(false, &mut run), WorldAction::Refused(_)));
        assert!(matches!(c.try_claim_mission(true, &mut run), WorldAction::Done(_)));
        assert_eq!(c.tutorial.step, rules::TutorialStep::Flight101Scan);
        assert!(matches!(c.try_survey(0, 2.2, &mut run), WorldAction::Done(_)));
        assert_eq!(c.tutorial.step, rules::TutorialStep::Flight101FetchAnimal);
        assert!(matches!(c.try_tractor_beam(0, &mut run), WorldAction::Done(_)));
        assert_eq!(c.tutorial.step, rules::TutorialStep::Flight101EradicateAnimals);
        for _ in 0..rules::FLIGHT101_ERADICATE_KILLS {
            c.register_kill(&mut run);
        }
        assert_eq!(c.tutorial.step, rules::TutorialStep::Story201);
        assert!(c.has_tool(tool::INTERPLANETARY_DRIVE), "Flight 101 opens the system");
        assert!(c.has_tool(tool::INTERSTELLAR_DRIVE), "Story 201 unlocks the drive");
        assert!(c.has_tool(tool::PROTON_MISSILE_1));
        assert_eq!(c.max_jump_distance(), 3.0 * rules::MAP_UNITS_PER_GALAXY_UNIT);
        // Story 201: scan the wreck, then the ruin on another world of the
        // closest star (a second scan of the wreck world does not count).
        let star = 7;
        c.tutorial.story_star = Some(star);
        c.arrive(star);
        let foreign = space_galaxy::world_id(star, 1);
        assert!(matches!(c.try_survey(foreign, 2.2, &mut run), WorldAction::Done(_)));
        assert_eq!(c.tutorial.step, rules::TutorialStep::Story201);
        assert_eq!(c.tutorial.story_wreck, Some(foreign));
        let _ = c.try_survey(foreign, 2.2, &mut run);
        assert_eq!(c.tutorial.step, rules::TutorialStep::Story201);
        let ruin = space_galaxy::world_id(star, 2);
        assert!(matches!(c.try_survey(ruin, 2.2, &mut run), WorldAction::Done(_)));
        assert_eq!(c.tutorial.step, rules::TutorialStep::TeachColonize);
        assert!(c.has_tool(tool::SETI));
        assert_eq!(c.charges(tool::PLACE_COLONY), 1, "TutorialColonize gives a colony pack");
        // Colonize with the free pack: +10,000.
        let wallet = run.spice;
        assert!(matches!(c.try_survey(1, 2.2, &mut run), WorldAction::Done(_)));
        assert!(matches!(c.try_colonize(1, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, wallet + 10_000.0);
        assert_eq!(c.tutorial.step, rules::TutorialStep::TeachTerraform);
        // Terraform: +10,000 (minus the adapted terraform price).
        let world = (2..=WORLD_COUNT)
            .find(|id| terraform_cost(*id) <= run.spice)
            .expect("an affordable world");
        assert!(matches!(c.try_survey(world, 2.2, &mut run), WorldAction::Done(_)));
        let wallet = run.spice;
        assert!(matches!(c.try_terraform(world, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, wallet - terraform_cost(world) + 10_000.0);
        assert_eq!(c.tutorial.step, rules::TutorialStep::TeachTradeSpice);
        // Trade spice with an empire: 3 red at 4,499 + 5,000.
        let empire_star = 11;
        c.arrive(empire_star);
        c.current_system_empire = Some(3);
        c.cargo_spice.insert(SpiceColor::Red, 3);
        let wallet = run.spice;
        let post = space_galaxy::world_id(empire_star, 0);
        assert!(matches!(c.try_collect_or_trade_spice(post, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, wallet + 3.0 * 4_499.0 + 5_000.0);
        assert!(c.intro_objectives_complete());
        assert_eq!(c.mission_name(), "Free exploration");
        assert!(matches!(c.try_claim_mission(true, &mut run), WorldAction::Refused(_)));
        assert!(!run.finished);
    }

    #[test]
    fn colonize_needs_the_drive_and_a_colony_pack_or_the_store_price() {
        let mut run = RunProgress::default();
        let mut c = retail_campaign(&mut run);
        c.surveyed.push(1);
        assert!(matches!(
            c.try_colonize(1, &mut run),
            WorldAction::Refused(msg) if msg.contains("Interplanetary")
        ));
        c.grant_tool(tool::INTERPLANETARY_DRIVE);
        run.spice = 149_999.0;
        assert!(matches!(
            c.try_colonize(1, &mut run),
            WorldAction::Refused(msg) if msg.contains("Incredi-Pak")
        ));
        assert!(c.colonies.iter().all(|&w| w == 0), "a refused colonise changes nothing");
        run.spice = 150_000.0;
        assert!(matches!(c.try_colonize(1, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, 0.0, "the Colony Incredi-Pak store price is charged");
        c.surveyed.push(2);
        c.add_charges(tool::PLACE_COLONY, 1);
        assert!(matches!(c.try_colonize(2, &mut run), WorldAction::Done(_)));
        assert_eq!(c.charges(tool::PLACE_COLONY), 0, "a carried pack is used first");
    }

    #[test]
    fn colonize_requires_survey_and_stable_capacity_before_expansion() {
        let mut run = RunProgress::default();
        let mut c = retail_campaign(&mut run);
        c.grant_tool(tool::INTERPLANETARY_DRIVE);
        assert!(matches!(c.try_colonize(1, &mut run), WorldAction::Refused(_)));
        assert!(matches!(c.try_terraform(1, &mut run), WorldAction::Refused(_)));
        run.spice = 1_000_000.0;
        assert!(matches!(c.try_survey(1, 2.2, &mut run), WorldAction::Done(_)));
        let before = run.spice;
        assert!(matches!(c.try_colonize(1, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, before - 150_000.0);
        assert_eq!(c.ecology_for(1).colony_count, 1);
        assert_eq!(c.ecology_for(1).stable_foodweb_tier(), 0);
        let after = run.spice;
        assert!(matches!(
            c.try_colonize(1, &mut run),
            WorldAction::Refused(msg) if msg.contains("capacity")
        ));
        assert_eq!(run.spice, after, "a refused expansion cannot charge again");
        assert_eq!(run.colonies, 1);
    }

    #[test]
    fn the_homeworld_reports_the_tutorial_and_refuses_when_done() {
        let mut run = RunProgress::default();
        let mut c = retail_campaign(&mut run);
        assert!(matches!(c.try_claim_mission(false, &mut run), WorldAction::Refused(_)));
        assert!(matches!(c.try_claim_mission(true, &mut run), WorldAction::Done(_)));
        // The scan step is not done by talking.
        assert!(matches!(
            c.try_claim_mission(true, &mut run),
            WorldAction::Refused(msg) if msg.contains("Scan")
        ));
        assert_eq!(c.mission, 1);
        assert_eq!(c.claimed, 1);
        c.tutorial.step = rules::TutorialStep::Done;
        let spice = run.spice;
        assert!(matches!(c.try_claim_mission(true, &mut run), WorldAction::Refused(_)));
        assert_eq!(run.spice, spice, "a finished tutorial grants nothing more");
    }

    #[test]
    fn colonies_alone_do_not_finish_the_tutorial() {
        let mut c = SpaceCampaign::default();
        c.colonies = vec![0, 1, 2, 3, 4, 5];
        c.terraformed.push(1);
        assert!(!c.intro_objectives_complete(), "the tutorial missions are outstanding");
        c.tutorial.step = rules::TutorialStep::Done;
        assert!(c.intro_objectives_complete());
    }

    #[test]
    fn energy_has_no_passive_recharge_and_jumps_cost_five_or_damage() {
        let mut run = RunProgress::default();
        let mut c = retail_campaign(&mut run);
        assert_eq!(c.energy, 100.0);
        // Flying costs nothing; no trickle and no free docking refill.
        c.spend_travel(BOUNDS * 2.0);
        assert_eq!(c.energy, 100.0);
        c.energy = 10.0;
        let mut spice = 0.0;
        c.tick(30.0, true, &mut spice, &mut false);
        assert_eq!(c.energy, 10.0, "no passive recharge (spaceEconomyEnergyRechargeRate 0)");
        assert_eq!(c.jump(), (5.0, 0.0));
        assert_eq!(c.energy, 5.0);
        assert_eq!(c.jump(), (5.0, 0.0));
        assert_eq!(c.jump(), (0.0, 200.0), "an empty reactor jumps and costs 200 hull");
        assert_eq!(c.pending_damage, 200.0);
        assert_eq!(c.fuel_fraction(), 0.0);
    }

    #[test]
    fn colonies_produce_spice_into_capped_storage_and_pay_no_money() {
        let mut c = SpaceCampaign::default();
        c.colonies = vec![0, 1];
        c.next_pirate_raid = f32::MAX; // no raids stealing spice in this test
        c.next_bio_disaster = f32::MAX;
        let mut spice = 0.0;
        let mut save = false;
        // One factory link (400, 0xBD8CD0): one spice per 300 s.
        c.tick(299.0, false, &mut spice, &mut save);
        assert_eq!(c.colony_spice_stored(1), 0);
        c.tick(1.5, false, &mut spice, &mut save);
        assert_eq!(c.colony_spice_stored(1), 1);
        assert_eq!(spice, 0.0, "retail colonies earn no money directly");
        assert_eq!(c.colony_income_per_period(), 0.0);
        for _ in 0..100 {
            c.tick(60.0, false, &mut spice, &mut save);
        }
        assert_eq!(c.colony_spice_stored(1), 5, "storage caps at colonyMaxSpiceStoredPerColony");
        assert!(c.colony_spice_stored(0) <= 5);
    }

    #[test]
    fn kills_drop_retail_loot_by_chance() {
        let mut run = RunProgress::default();
        let mut c = SpaceCampaign::default();
        let mut drops = 0;
        for _ in 0..40 {
            let before = run.spice;
            c.register_kill(&mut run);
            let gained = run.spice - before;
            if gained > 0.0 {
                drops += 1;
                assert!((1_000.0..=10_000.0).contains(&gained), "tier-1 loot {gained}");
            }
        }
        assert_eq!(c.kills, 40);
        assert!(drops > 0 && drops < 40, "raiders drop money 25% of the time ({drops})");
    }

    #[test]
    fn docking_records_each_trade_route_once() {
        let mut c = SpaceCampaign::default();
        assert!(c.dock(0).is_none());
        assert!(c.dock(1).is_none());
        assert_eq!(c.docked.len(), 2);
        c.dock(0);
        assert_eq!(c.docked.len(), 2, "a repeat dock is not a new trade route");
        assert!(c.dock(2).is_some());
        assert_eq!(c.docked.len(), 3);
    }

    #[test]
    fn world_identity_migrates_from_a_checkpoint_by_world_id() {
        let checkpoint = crate::persistence::Checkpoint {
            encounter: None,
            stage: SPACE_STAGE,
            player: crate::persistence::PlayerCheckpoint {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                hp: 100.0,
                food: None,
            },
            objects: vec![
                object_checkpoint(0, true, true),
                // World 2 is the colony, world 1 is only scanned: an exact,
                // id-keyed mapping, not a positional one.
                object_checkpoint(2, true, true),
                object_checkpoint(1, false, true),
            ],
        };
        let mut c = SpaceCampaign::default();
        assert!(c.adopt_checkpoint(&checkpoint));
        assert_eq!(c.colonies, vec![0, 2]);
        assert_eq!(c.surveyed, vec![0, 2, 1]);
        assert_eq!(c.new_colony_count(), 1);
        // Idempotent: a second adopt changes nothing.
        assert!(!c.adopt_checkpoint(&checkpoint));

        // A save with no Space checkpoint falls back to the sorted-count map.
        let mut legacy = SpaceCampaign::default();
        legacy.reconcile_legacy_colonies(2);
        assert_eq!(legacy.colonies, vec![0, 1, 2]);

        // A live campaign is never rewritten by the legacy path.
        let mut live = SpaceCampaign::default();
        live.colonies = vec![0, 5];
        live.reconcile_legacy_colonies(4);
        assert_eq!(
            live.colonies,
            vec![0, 5],
            "a real campaign keeps its worlds"
        );
    }

    #[test]
    fn a_colonised_world_survives_a_save_round_trip() {
        let mut run = RunProgress::default();
        run.space_campaign.energy = 37.0;
        run.space_campaign.fuel_initialised = true;
        run.space_campaign.surveyed = vec![0, 2, 4];
        run.space_campaign.colonies = vec![0, 2];
        run.space_campaign.terraformed = vec![2];
        run.space_campaign.kills = 2;
        run.space_campaign.claimed = 2;
        run.colonies = 1;
        let text = serde_json::to_string(&run).expect("serialise");
        let loaded: RunProgress = serde_json::from_str(&text).expect("deserialise");
        assert_eq!(loaded.space_campaign.colonies, vec![0, 2]);
        assert_eq!(loaded.space_campaign.surveyed, vec![0, 2, 4]);
        assert_eq!(loaded.space_campaign.terraformed, vec![2]);
        assert_eq!(loaded.space_campaign.kills, 2);
        assert_eq!(loaded.space_campaign.claimed, 2);
        assert_eq!(
            loaded.space_campaign.energy, 37.0,
            "saved fuel is preserved"
        );
        assert_eq!(loaded.colonies, 1);
        // A filled save is never touched again on entry.
        let mut again = loaded.space_campaign.clone();
        assert!(!again.initialise_fuel(), "the flag survives the round trip");
        assert_eq!(again.energy, 37.0, "recorded fuel is left exactly as saved");

        // A save without the field at all still yields a playable campaign.
        let stripped: RunProgress = serde_json::from_str(
            &text.replace("\"space_campaign\":", "\"space_campaign_absent\":"),
        )
        .unwrap();
        assert_eq!(stripped.space_campaign.energy, MAX_ENERGY);
        assert_eq!(stripped.space_campaign.colonies.len(), 0);

        // The fuel contract, in all three states:
        // a save that recorded an empty reactor keeps it, one that recorded
        // fuel but predates the flag keeps that fuel, and only an untouched
        // campaign is topped up.
        let mut dry = SpaceCampaign {
            energy: 0.0,
            fuel_initialised: true,
            ..Default::default()
        };
        assert!(
            !dry.initialise_fuel(),
            "a recorded empty reactor is left alone"
        );
        assert_eq!(dry.energy, 0.0, "a saved zero-energy state survives");
        let mut old = SpaceCampaign {
            energy: 55.0,
            ..Default::default()
        };
        assert!(old.initialise_fuel(), "the missing flag is recorded");
        assert_eq!(old.energy, 55.0, "fuel from an older save is kept");
        let mut empty = SpaceCampaign {
            energy: 0.0,
            ..Default::default()
        };
        assert!(empty.initialise_fuel(), "an unrecorded reactor is filled");
        assert_eq!(empty.energy, MAX_ENERGY);
        assert!(!empty.initialise_fuel(), "the fill happens exactly once");
    }

    #[test]
    fn hud_explains_the_objective_and_the_next_step() {
        let mut c = SpaceCampaign::default();
        let text = c.hud();
        assert!(text.contains("Flight 101: Visit your cities"), "{text}");
        assert!(text.contains("Rank: Captain"), "{text}");
        assert!(text.contains("energy 100/100"), "{text}");
        assert!(text.contains("press M at the homeworld"), "{text}");
        // Failures must reach the HUD, not only the log.
        let mut run = RunProgress::default();
        let action = c.try_colonize(1, &mut run);
        c.apply(action);
        assert!(
            c.hud().contains("Survey world 1 with V first"),
            "{}",
            c.hud()
        );
        c.tutorial.step = rules::TutorialStep::Done;
        let complete = c.hud();
        assert!(complete.contains("Free exploration"));
        assert!(complete.contains("take missions from empires"));
    }

    /// An actual runtime test: the campaign survives a headless stage entry and
    /// reconciling a checkpoint grants exactly the recorded worlds.
    #[test]
    fn headless_stage_entry_reconciles_worlds_by_world_id() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Space);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Space);
        app.update();
        app.update();
        let world = app.world_mut();
        let mut ids: Vec<u32> = world
            .query::<&crate::persistence::WorldId>()
            .iter(world)
            .map(|id| id.0)
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            home_world_ids(),
            "Space spawns the homeworld plus the worlds of the generated home system"
        );
        // The homeworld is owned and surveyed; every other world is untouched.
        let states: Vec<(u32, bool, bool)> = world
            .query::<(&crate::persistence::WorldId, &Planet)>()
            .iter(world)
            .map(|(id, p)| (id.0, p.colonized, p.surveyed))
            .collect();
        let home = states.iter().find(|(id, ..)| *id == 0).unwrap();
        assert!(home.1 && home.2, "the homeworld starts as a colony");
        assert_eq!(
            states.iter().filter(|(_, colonized, _)| *colonized).count(),
            1,
            "a fresh run owns only the homeworld"
        );
    }

    // ------------------------------------------------------------------
    // Solar system, flight and galaxy travel
    // ------------------------------------------------------------------

    /// A run whose Space campaign already follows the retail rules and owns
    /// both drives (`drive`: the interstellar drive tool id).
    fn retail_run_with_drive(drive: &str) -> RunProgress {
        let mut run = RunProgress::default();
        let c = &mut run.space_campaign;
        c.economy_version = ECONOMY_VERSION;
        c.tools = rules::starting_tools();
        c.grant_tool(tool::INTERPLANETARY_DRIVE);
        c.grant_tool(drive);
        c.tutorial.step = rules::TutorialStep::Done;
        run
    }

    fn space_app(run: RunProgress) -> App {
        let mut app = crate::runtime::headless_app(run);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Space);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Space);
        app.update();
        app.update();
        app
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    /// Z steps the discrete zoom out: the solar camera pulls out to its
    /// maximum and the galaxy map opens when it arrives.
    fn zoom_out_to_the_galaxy(app: &mut App) {
        // Home is the retail discrete zoom-out key (Z is unbound in Space).
        tap(app, KeyCode::Home);
        for _ in 0..1200 {
            if app.world().resource::<SpaceView>().galaxy {
                break;
            }
            app.update();
        }
    }

    fn ship(app: &mut App) -> (Vec3, Vec3) {
        let world = app.world_mut();
        let (t, m) = world
            .query_filtered::<(&Transform, &ShipMotion), With<Player>>()
            .single(world)
            .unwrap();
        (t.translation, m.velocity)
    }

    fn world_position(app: &mut App, id: u32) -> Vec3 {
        let world = app.world_mut();
        world
            .query::<(&crate::persistence::WorldId, &Transform)>()
            .iter(world)
            .find(|(w, _)| w.0 == id)
            .map(|(_, t)| t.translation)
            .unwrap()
    }

    /// WorldIds of the generated home system (the homeworld is 0).
    fn home_world_ids() -> Vec<u32> {
        let plan = space_galaxy::system_plan(&Galaxy::generate(None), HOME_STAR, &[]);
        (0..plan.bodies.iter().filter(|b| b.world.is_some()).count() as u32).collect()
    }

    #[test]
    fn the_home_system_has_a_star_orbits_and_the_ship_beside_its_homeworld() {
        let mut app = space_app(RunProgress::default());
        let world = app.world_mut();
        let star = world.query::<&SystemStar>().single(world).unwrap();
        assert_eq!(star.index, HOME_STAR);
        let star_radius = star.radius;
        // Every world orbits the star at the origin, outside the star itself.
        let orbits: Vec<(u32, f32, f32)> = world
            .query::<(&crate::persistence::WorldId, &Orbit, &Transform)>()
            .iter(world)
            .map(|(id, o, t)| (id.0, o.radius, t.translation.length()))
            .collect();
        assert_eq!(orbits.len(), home_world_ids().len());
        for (id, radius, distance) in &orbits {
            assert!(
                (radius - distance).abs() < 0.01,
                "world {id} sits on its orbit"
            );
            assert!(*radius > star_radius * 2.0);
        }
        // The ship starts in docking reach of the homeworld.
        let home = world_position(&mut app, 0);
        let (at, _) = ship(&mut app);
        assert!(at.distance(home) < 4.0 + ACTION_RANGE, "{at} vs {home}");
    }

    #[test]
    fn worlds_orbit_adopt_restored_positions_and_carry_a_docked_ship() {
        let mut app = space_app(RunProgress::default());
        let before = world_position(&mut app, 3);
        for _ in 0..120 {
            app.update();
        }
        let after = world_position(&mut app, 3);
        assert!(
            before.distance(after) > 0.05,
            "worlds move along their orbits"
        );
        assert!(
            (before.length() - after.length()).abs() < 0.01,
            "on a circle"
        );
        // A checkpoint restore moves a world to another angle; the orbit
        // continues from there instead of snapping back.
        let world = app.world_mut();
        let (mut t, orbit) = world
            .query::<(&mut Transform, &Orbit)>()
            .iter_mut(world)
            .find(|(t, _)| t.translation == after)
            .map(|(t, o)| (t, *o))
            .unwrap();
        let restored = Vec3::new(-orbit.radius, 0.0, 0.0);
        t.translation = restored;
        app.update();
        let now = world_position(&mut app, 3);
        assert!(
            now.distance(restored) < 0.2,
            "{now} continues from {restored}"
        );
        // A ship hovering in reach rides along with the world.
        let home = world_position(&mut app, 0);
        let offset = Vec3::new(0.0, 3.0, 0.0) - home.normalize() * 8.0;
        {
            let world = app.world_mut();
            world
                .query_filtered::<&mut Transform, With<Player>>()
                .single_mut(world)
                .unwrap()
                .translation = home + offset;
        }
        for _ in 0..90 {
            app.update();
        }
        let home = world_position(&mut app, 0);
        let (at, _) = ship(&mut app);
        assert!(
            at.distance(home + offset) < 0.5,
            "the docked ship keeps station: {at} vs {}",
            home + offset
        );
    }

    #[test]
    fn the_ship_accelerates_coasts_and_comes_to_rest() {
        let mut app = space_app(RunProgress::default());
        let (start, v0) = ship(&mut app);
        assert_eq!(v0, Vec3::ZERO);
        // Retail controls: the ship flies where it is sent (a click or the
        // right button), not with WASD (those turn the camera).
        let goal = start + Vec3::new(40.0, 0.0, 0.0);
        app.world_mut()
            .resource_mut::<crate::space_controls::SpaceControls>()
            .fly_to = Some(goal);
        app.update();
        let (_, first) = ship(&mut app);
        for _ in 0..60 {
            app.update();
        }
        let (_, cruising) = ship(&mut app);
        assert!(
            first.x > 0.0 && cruising.x > first.x * 4.0,
            "thrust builds speed"
        );
        app.world_mut()
            .resource_mut::<crate::space_controls::SpaceControls>()
            .fly_to = None;
        let (released_at, _) = ship(&mut app);
        for _ in 0..20 {
            app.update();
        }
        let (coasted, coasting) = ship(&mut app);
        assert!(coasted.x > released_at.x, "momentum carries the ship on");
        assert!(coasting.x < cruising.x, "and drag slows it");
        for _ in 0..600 {
            app.update();
        }
        let (rest, v) = ship(&mut app);
        assert_eq!(v, Vec3::ZERO, "the ship settles to a stop");
        assert!(rest.x > start.x);
    }

    #[test]
    fn a_galaxy_jump_swaps_the_system_saves_it_and_the_ship_can_return_home() {
        let mut app = space_app(retail_run_with_drive("interstellardrive_upgrade4"));
        let reach = app.world().resource::<RunProgress>().space_campaign.max_jump_distance();
        let galaxy = app.world().resource::<Galaxy>().clone();
        let target = (1..galaxy.stars.len() as u32)
            .filter(|i| galaxy.distance(HOME_STAR, *i) <= reach)
            .min_by(|a, b| {
                galaxy
                    .distance(HOME_STAR, *a)
                    .total_cmp(&galaxy.distance(HOME_STAR, *b))
            })
            .unwrap();
        // Zoom out to the galaxy map and pick the star.
        zoom_out_to_the_galaxy(&mut app);
        assert!(app.world().resource::<SpaceView>().galaxy);
        app.world_mut().resource_mut::<SpaceView>().cursor = target;
        let energy = app.world().resource::<RunProgress>().space_campaign.energy;
        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<SpaceView>().travel.is_some());
        for _ in 0..240 {
            app.update();
        }
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.space_campaign.current_system, target);
        assert!(run.space_campaign.visited.contains(&target));
        assert!(run.space_campaign.energy < energy, "the jump costs energy");
        assert!(
            !app.world().resource::<SpaceView>().galaxy,
            "zoomed back in"
        );
        let world = app.world_mut();
        let ids: Vec<u32> = world
            .query::<&crate::persistence::WorldId>()
            .iter(world)
            .map(|id| id.0)
            .collect();
        assert!(!ids.is_empty());
        assert!(ids
            .iter()
            .all(|id| space_galaxy::system_of_world(*id) == target));
        assert_eq!(
            world.query::<&SystemStar>().single(world).unwrap().index,
            target
        );
        // Surveying a foreign world counts toward the campaign.
        let foreign = ids[0];
        let position = world_position(&mut app, foreign);
        {
            let world = app.world_mut();
            world
                .query_filtered::<&mut Transform, With<Player>>()
                .single_mut(world)
                .unwrap()
                .translation = position;
        }
        tap(&mut app, KeyCode::KeyV);
        assert!(app
            .world()
            .resource::<RunProgress>()
            .space_campaign
            .surveyed
            .contains(&foreign));
        // The scan asks for its retail tool effects (`space_tool_fx`, no player of them in this app).
        let world = app.world_mut();
        assert!(world
            .query::<&crate::space_tool_fx::ToolFxUse>()
            .iter(world)
            .any(|u| u.tool == "scan" && u.world == Some(foreign)));
        // A save taken here reloads into the same system.
        let text = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let loaded: RunProgress = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.space_campaign.current_system, target);
        let mut resumed = space_app(loaded);
        let world = resumed.world_mut();
        assert_eq!(
            world.query::<&SystemStar>().single(world).unwrap().index,
            target
        );
        // A dry reactor can still limp home.
        resumed
            .world_mut()
            .resource_mut::<RunProgress>()
            .space_campaign
            .energy = 0.0;
        zoom_out_to_the_galaxy(&mut resumed);
        resumed.world_mut().resource_mut::<SpaceView>().cursor = HOME_STAR;
        tap(&mut resumed, KeyCode::Enter);
        for _ in 0..240 {
            resumed.update();
        }
        assert_eq!(
            resumed
                .world()
                .resource::<RunProgress>()
                .space_campaign
                .current_system,
            HOME_STAR
        );
        let world = resumed.world_mut();
        let mut ids: Vec<u32> = world
            .query::<&crate::persistence::WorldId>()
            .iter(world)
            .map(|id| id.0)
            .collect();
        ids.sort();
        assert_eq!(ids, home_world_ids());
    }

    #[test]
    fn an_out_of_range_or_unaffordable_jump_is_refused_with_a_reason() {
        // Without an interstellar drive (before Story 201) no jump leaves home.
        let mut fresh = space_app(RunProgress::default());
        // The retail camera zooms out in discrete steps (space_controls).
        zoom_out_to_the_galaxy(&mut fresh);
        fresh.world_mut().resource_mut::<SpaceView>().cursor = 1;
        tap(&mut fresh, KeyCode::Enter);
        assert!(fresh.world().resource::<SpaceView>().travel.is_none());
        let message = fresh.world().resource::<RunProgress>().space_campaign.message.clone();
        assert!(message.contains("Interstellar Drive"), "{message}");
        // The first drive reaches 3 galaxy units.
        let mut app = space_app(retail_run_with_drive("interstellardrive"));
        let galaxy = app.world().resource::<Galaxy>().clone();
        let far = (1..galaxy.stars.len() as u32)
            .find(|i| galaxy.distance(HOME_STAR, *i) > space_galaxy::MAX_JUMP * 1.5)
            .unwrap();
        zoom_out_to_the_galaxy(&mut app);
        app.world_mut().resource_mut::<SpaceView>().cursor = far;
        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<SpaceView>().travel.is_none());
        let message = app
            .world()
            .resource::<RunProgress>()
            .space_campaign
            .message
            .clone();
        assert!(message.contains("drive reaches"), "{message}");
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .space_campaign
                .current_system,
            HOME_STAR
        );
    }

    #[test]
    fn the_ship_rules_restore_a_destroyed_ufo_tether_it_and_arm_npcs() {
        // Destruction (0xC3C680): cinematic, restore, no game over; an
        // empty-reactor jump's 200 damage is owed through pending_damage.
        let mut app = space_app(retail_run_with_drive("interstellardrive"));
        app.update();
        {
            let world = app.world_mut();
            let mut health = world
                .query_filtered::<&mut Health, With<Player>>()
                .single_mut(world)
                .unwrap();
            assert_eq!(health.max, 1000.0, "1000 x level 1 x normal difficulty");
            health.hp = 150.0;
        }
        app.world_mut().resource_mut::<RunProgress>().space_campaign.pending_damage = 200.0;
        app.update();
        {
            let campaign = &app.world().resource::<RunProgress>().space_campaign;
            assert_eq!(campaign.ships_lost, 1);
            assert_eq!(campaign.pending_damage, 0.0);
            assert_eq!(
                campaign.pending_cinematic.as_deref(),
                Some(rules::CINEMATIC_DESTROYED_PRE_POSSE_NOT_GALAXY)
            );
            assert_eq!(campaign.energy, campaign.max_reactor_energy());
        }
        let world = app.world_mut();
        let health = world
            .query_filtered::<&Health, With<Player>>()
            .single(world)
            .unwrap();
        assert_eq!(health.hp, health.max, "the UFO is restored");
        // Hostile ships carry the retail health (UFOHealthPirate tier 1: 250).
        let pirates: Vec<f32> = world
            .query::<&Pirate>()
            .iter(world)
            .map(|p| p.max)
            .collect();
        assert!(!pirates.is_empty());
        assert!(pirates.iter().all(|m| *m == rules::UFO_HEALTH_PIRATE[0]), "{pirates:?}");

        // Without the Interplanetary Drive the UFO stays by its home planet,
        // and Story 201 picks the star closest to home.
        let mut fresh = space_app(RunProgress::default());
        let home = world_position(&mut fresh, 0);
        teleport_ship(&mut fresh, home + Vec3::new(80.0, 0.0, 0.0));
        fresh.update();
        let (at, _) = ship(&mut fresh);
        let radius = {
            let world = fresh.world_mut();
            world
                .query::<(&crate::persistence::WorldId, &Planet)>()
                .iter(world)
                .find(|(id, _)| id.0 == 0)
                .unwrap()
                .1
                .radius
        };
        let home = world_position(&mut fresh, 0);
        assert!(
            at.distance(home) <= radius + rules::HOME_TETHER_RANGE + 1.0,
            "tethered to the homeworld ({})",
            at.distance(home)
        );
        fresh
            .world_mut()
            .resource_mut::<RunProgress>()
            .space_campaign
            .tutorial
            .step = rules::TutorialStep::Story201;
        fresh.update();
        let star = fresh
            .world()
            .resource::<RunProgress>()
            .space_campaign
            .tutorial
            .story_star
            .expect("closest star");
        let galaxy = fresh.world().resource::<Galaxy>().clone();
        let closest = (1..galaxy.stars.len() as u32)
            .min_by(|a, b| galaxy.distance(HOME_STAR, *a).total_cmp(&galaxy.distance(HOME_STAR, *b)))
            .unwrap();
        assert_eq!(star, closest);
    }

    fn teleport_ship(app: &mut App, position: Vec3) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players.single_mut(world).unwrap().translation = position;
    }

    // ------------------------------------------------------------------
    // Ship editor: original vehicle parts only
    // ------------------------------------------------------------------

    /// One authored part definition. `model_instance` is the RW4 link the
    /// loader resolves; several definitions may legitimately share one.
    fn ship_part_def(group: u32, instance: u32, model_instance: u32, name: &str) -> PartDefinition {
        PartDefinition {
            key: ResourceKey {
                type_id: asset_catalog::PROP,
                group_id: group,
                instance_id: instance,
            },
            model: ResourceKey {
                type_id: asset_catalog::RW4,
                group_id: group,
                instance_id: model_instance,
            },
            name: name.to_string(),
            is_null: false,
            price: 10,
            unlock_level: 1,
            min_scale: 0.5,
            max_scale: 2.0,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: asset_catalog::Capabilities::default(),
        }
    }

    /// A catalog holding `defs`, with a resource record behind every model
    /// link. Each record deliberately points at a package that does not exist
    /// in this fixture, so an un-cached import fails during the geometry read
    /// — before anything is allocated, exactly like the real failure path.
    fn ship_catalog(defs: Vec<PartDefinition>) -> Originals {
        let mut originals = Originals::default();
        for d in &defs {
            originals.resource_index.records.insert(
                d.model,
                (
                    PathBuf::from("this_package_does_not_exist.pkg"),
                    DbpfEntry {
                        key: d.model,
                        offset: 0,
                        disk_size: 8,
                        mem_size: 8,
                        is_compressed: false,
                    },
                ),
            );
        }
        originals.part_catalog.parts = defs;
        originals
    }

    fn gpu_stores() -> (Assets<Mesh>, Assets<Image>, Assets<StandardMaterial>) {
        (
            Assets::<Mesh>::default(),
            Assets::<Image>::default(),
            Assets::<StandardMaterial>::default(),
        )
    }

    /// Put an entry in the exact-definition cache exactly as a successful
    /// `mechanical_assets::import` would leave it. This fixture ships no real
    /// RW4 bytes, so a cache hit is the only way to exercise a *successful*
    /// equip headlessly; the decode itself is covered by the helper's own
    /// importer tests and by the parent's real-asset GPU check.
    fn seed_cache(
        originals: &mut Originals,
        def: &PartDefinition,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        originals.mechanical_models.insert(
            def.key,
            OriginalModel {
                handle: meshes.add(Sphere::new(1.0)),
                scale: 1.0,
                instance: def.model.instance_id,
                material: Some(materials.add(StandardMaterial::default())),
                ..Default::default()
            },
        );
    }

    #[test]
    fn ship_editor_only_offers_original_vehicle_parts() {
        let cannon = ship_part_def(SHIP_PART_GROUP, 10, 0x6001_0001, "Laser Cannon");
        let repair = ship_part_def(SHIP_PART_GROUP, 11, 0x6001_0002, "Repair Kit");
        let mouth = ship_part_def(asset_catalog::CREATURE_PARTS, 20, 0x6002_0001, "Mouth");
        let tower = ship_part_def(asset_catalog::BUILDING_PARTS, 30, 0x6003_0001, "Tower");
        let originals = ship_catalog(vec![cannon, repair, mouth, tower]);

        // Only authored vehicle parts are offered, and by their authored name.
        let options = ship_part_options(&originals);
        let offered: Vec<&str> = options.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(offered, ["Laser Cannon", "Repair Kit"]);

        // `N` skips what is already fitted instead of re-adding it...
        let unmounted = ship_part_candidates(&originals, None, 1, &[0x6001_0001]);
        assert_eq!(
            unmounted
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["Repair Kit"]
        );
        // ...and with the whole catalog fitted it refuses rather than loops.
        let all_fitted = ship_part_candidates(
            &originals,
            Some(0x6001_0002),
            1,
            &[0x6001_0001, 0x6001_0002],
        );
        assert!(all_fitted.is_empty());
    }

    #[test]
    fn the_ship_part_limit_refuses_an_oversized_build() {
        let mut build = ShipBuild {
            modules: (0..MAX_SHIP_MODULES)
                .map(|i| ShipModule::new(i as u32, 0))
                .collect(),
            focused: 0,
        };
        let before = build.clone();
        let mut originals = Originals::default();
        let (mut meshes, mut images, mut materials) = gpu_stores();

        // The ceiling is checked before anything is imported, so an oversized
        // build cannot grow further and nothing is allocated for the attempt.
        let err = equip_ship_part(
            &mut build,
            EquipTarget::Append { slot: 0 },
            &[],
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap_err();
        assert!(err.contains("48"), "{err}");
        assert_eq!(build, before);
        assert_eq!((meshes.len(), images.len(), materials.len()), (0, 0, 0));
    }

    #[test]
    fn an_unsupported_first_candidate_is_walked_past_to_a_usable_one() {
        let broken = ship_part_def(SHIP_PART_GROUP, 1, 0x0bad_0001, "Broken Turret");
        let working = ship_part_def(SHIP_PART_GROUP, 2, 0x600d_0001, "Working Radar");
        let mut originals = ship_catalog(vec![broken, working.clone()]);
        let (mut meshes, mut images, mut materials) = gpu_stores();
        // The second candidate is already in the exact cache, as a previous
        // successful import leaves it, so it resolves without decoding.
        seed_cache(&mut originals, &working, &mut meshes, &mut materials);

        // Selection still offers the broken candidate first...
        let candidates = ship_part_candidates(&originals, None, 1, &[]);
        assert_eq!(
            candidates
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["Broken Turret", "Working Radar"]
        );

        // ...but the equip walk steps over it and commits the usable one, so
        // an empty build is never trapped on the first unsupported model.
        let mut build = ShipBuild::default();
        let name = equip_ship_part(
            &mut build,
            EquipTarget::Append { slot: 0 },
            &candidates,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .expect("the usable candidate must be reached");
        assert_eq!(name, "Working Radar");
        assert_eq!(build.modules.len(), 1);
        assert_eq!(build.modules[0].instance, working.model.instance_id);

        // The equipped id resolves back to the exact entry that was imported.
        let cached =
            mechanical_assets::cached(&originals, SHIP_PART_GROUP, build.modules[0].instance);
        assert!(cached.is_some(), "the fitted part must be renderable");
        assert!(cached.unwrap().material.is_some());
    }

    #[test]
    fn a_failed_replacement_leaves_the_loadout_unchanged() {
        let first = ship_part_def(SHIP_PART_GROUP, 1, 0x1111_0001, "Broken First");
        let second = ship_part_def(SHIP_PART_GROUP, 2, 0x1111_0002, "Broken Second");
        let mut originals = ship_catalog(vec![first, second]);
        let (mut meshes, mut images, mut materials) = gpu_stores();

        let mut build = ShipBuild {
            modules: vec![ShipModule::new(0x9999_9999, 0)],
            focused: 0,
        };
        let before = build.clone();

        // The fitted id is unknown to the catalog, so the walk covers every
        // eligible definition and tries each unique model exactly once.
        let candidates = ship_part_candidates(&originals, Some(0x9999_9999), 1, &[]);
        assert_eq!(candidates.len(), 2);

        let err = equip_ship_part(
            &mut build,
            EquipTarget::ReplaceFocused,
            &candidates,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap_err();
        // The report proves the walk reached the second candidate instead of
        // stalling on the first failure.
        assert!(err.contains("2 candidate"), "{err}");
        assert!(err.contains("Broken Second"), "{err}");

        // Nothing was committed and nothing was allocated.
        assert_eq!(build, before);
        assert_eq!((meshes.len(), images.len(), materials.len()), (0, 0, 0));
        assert!(originals.mechanical_models.is_empty());

        // An empty build refuses a replacement outright, with a way out.
        let mut empty = ShipBuild::default();
        let err = equip_ship_part(
            &mut empty,
            EquipTarget::ReplaceFocused,
            &candidates,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap_err();
        assert!(err.contains("press N"), "{err}");
        assert!(empty.modules.is_empty());
    }

    #[test]
    fn repeated_model_aliases_never_trap_cycling() {
        // Two authored definitions legitimately share one RW4 model.
        let alias_a = ship_part_def(SHIP_PART_GROUP, 1, 0x0aaa_0001, "Alias A");
        let alias_b = ship_part_def(SHIP_PART_GROUP, 2, 0x0aaa_0001, "Alias B");
        let distinct = ship_part_def(SHIP_PART_GROUP, 3, 0x0bbb_0001, "Distinct");
        let originals = ship_catalog(vec![distinct, alias_b, alias_a]);
        let ids = |list: &[PartDefinition]| -> Vec<u32> {
            list.iter().map(|d| d.model.instance_id).collect()
        };

        // With nothing fitted, one lap lists each unique model exactly once;
        // the alias is never surfaced as a second, identical-looking option.
        let all = ship_part_candidates(&originals, None, 1, &[]);
        assert_eq!(all.len(), 2);
        assert_eq!(ids(&all), [0x0aaa_0001, 0x0bbb_0001]);
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Alias A", "Distinct"]);

        // Replacing the aliased part steps to the other model in both
        // directions, and never returns the part already fitted.
        assert_eq!(
            ids(&ship_part_candidates(&originals, Some(0x0aaa_0001), 1, &[])),
            [0x0bbb_0001]
        );
        assert_eq!(
            ids(&ship_part_candidates(
                &originals,
                Some(0x0bbb_0001),
                -1,
                &[]
            )),
            [0x0aaa_0001]
        );
    }

    #[test]
    fn a_reloaded_build_imports_saved_parts_and_keeps_unsupported_ones() {
        // A fresh process has only scanned preview meshes, so stage setup
        // re-imports every saved module from its exact catalog definition.
        let ion = ship_part_def(SHIP_PART_GROUP, 1, 0x6001_0001, "Ion Thruster");
        let mut originals = ship_catalog(vec![ion.clone()]);
        let (mut meshes, mut images, mut materials) = gpu_stores();
        // This fixture carries no real package bytes, so the supported part is
        // seeded exactly as a successful import leaves it; the reload path
        // then takes the same cache hit a real decode returns through.
        seed_cache(&mut originals, &ion, &mut meshes, &mut materials);

        // Saved by an older build: no definition for it in the catalog.
        const LEGACY: u32 = 0xfeed_c0de;
        let build = ShipBuild {
            modules: vec![
                ShipModule::new(ion.model.instance_id, 0),
                ShipModule::new(LEGACY, 1),
            ],
            focused: 0,
        };

        let report = import_saved_ship_parts(
            &mut originals,
            &build,
            &mut meshes,
            &mut images,
            &mut materials,
        );

        // The resolvable part imports silently; only the unknown id is
        // reported, and it is named as the unsupported case it is.
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(
            report[0].contains(&format!("{LEGACY:08X}")),
            "report must name the record: {report:?}"
        );
        assert!(
            report[0].contains("not an original vehicle part"),
            "report must explain it: {report:?}"
        );

        // The build is handed over by shared reference, so neither a failed
        // import nor an unsupported id can drop a record the player owns.
        assert_eq!(build.modules.len(), 2);
        assert_eq!(build.modules[0].instance, ion.model.instance_id);
        assert_eq!(build.modules[1].instance, LEGACY);

        // The supported part renders straight from the exact cache...
        assert!(
            mechanical_assets::cached(&originals, SHIP_PART_GROUP, ion.model.instance_id).is_some()
        );
        // ...while the unsupported id resolves to nothing rather than to some
        // other, unrelated part.
        assert!(mechanical_assets::cached(&originals, SHIP_PART_GROUP, LEGACY).is_none());
    }

    #[test]
    fn ship_editor_hud_names_the_authored_part_and_the_controls() {
        let ion = ship_part_def(SHIP_PART_GROUP, 1, 0x6001_0001, "Ion Thruster");
        let originals = ship_catalog(vec![ion.clone()]);
        let mut run = RunProgress::default();
        run.ship
            .modules
            .push(ShipModule::new(ion.model.instance_id, 0));

        let hud = ship_editor_hud(&run, &originals);
        assert!(hud.contains("Ship: 1/48 parts"), "{hud}");
        assert!(hud.contains("Fitted: Ion Thruster"), "{hud}");
        assert!(hud.contains("Controls: N add"), "{hud}");
        assert!(hud.contains("[ ] part"), "{hud}");

        // A saved id the catalog cannot supply is explained, not hidden, and
        // is never dressed up as an authored name we do not have.
        run.ship.modules.push(ShipModule::new(0xfeed_c0de, 1));
        run.ship.focused = 1;
        run.space_campaign.ship_message = "Equipped part: Laser Cannon".to_string();
        let hud = ship_editor_hud(&run, &originals);
        assert!(hud.contains("Fitted: FEEDC0DE"), "{hud}");
        assert!(hud.contains("0xFEEDC0DE"), "{hud}");
        assert!(hud.contains("kept exactly as saved"), "{hud}");
        assert!(hud.contains("Equipped part: Laser Cannon"), "{hud}");

        // A brand new player still sees how to add their first part: the
        // controls are shown even with nothing fitted and nothing to report.
        let hud = ship_editor_hud(&RunProgress::default(), &Originals::default());
        assert!(hud.contains("Ship: 0/48 parts"), "{hud}");
        assert!(hud.contains("Controls: N add"), "{hud}");
        assert!(hud.contains("Transform: R rotate"), "{hud}");
        assert!(!hud.contains("Fitted:"), "nothing is fitted yet: {hud}");
        // Compact enough for the top-left: one status line, two control lines.
        assert_eq!(hud.lines().count(), 3, "{hud}");
    }

    #[test]
    fn homeworld_cannot_spend_tool_or_colony_budget_or_complete_a_mission() {
        let mut run = RunProgress::default();
        run.spice = 1_000.0;
        let mut campaign = SpaceCampaign::default();
        let before = serde_json::to_value(&campaign).unwrap();
        for tool in PlanetaryTool::ALL {
            campaign.planetary_tool = tool;
            let action = campaign.try_apply_planetary_tool(0, &mut run);
            // `happyraylevel1` is open on the homeworld in its record (it only
            // reacts to an empire's city): a use there changes nothing.
            if tool == PlanetaryTool::HappyRay {
                assert!(matches!(action, WorldAction::Done(_)), "{action:?}");
            } else {
                assert!(matches!(action, WorldAction::Refused(_)), "{tool:?}: {action:?}");
            }
            assert_eq!(run.spice, 1_000.0);
        }
        campaign.planetary_tool = PlanetaryTool::default();
        assert!(matches!(
            campaign.try_colonize(0, &mut run),
            WorldAction::Refused(_)
        ));
        assert_eq!(run.spice, 1_000.0);
        assert_eq!(serde_json::to_value(&campaign).unwrap(), before);
    }

    #[test]
    fn climate_score_is_separate_from_foodweb_and_scales_colony_capacity() {
        // The retail T-score is the distance to the bullseye centre.
        assert_eq!(crate::space_worldgen::t_score(0.20, 0.50), 1);
        assert_eq!(crate::space_worldgen::t_score(0.45, 0.50), 3);
        assert_eq!(crate::space_worldgen::t_score(0.50, 0.50), 3);
        assert_eq!(crate::space_worldgen::t_score(0.50, 0.20), 1);
        assert_eq!(crate::space_worldgen::t_score(0.10, 0.50), 0);
        assert_eq!(crate::space_worldgen::t_score(0.50, 0.95), 0);
        assert_eq!(climate_axis_guidance(0.20, false), "warm");
        assert_eq!(climate_axis_guidance(0.80, false), "cool");
        assert_eq!(climate_axis_guidance(0.20, true), "add");
        assert_eq!(climate_axis_guidance(0.80, true), "thin");

        let mut ecology = PlanetEcology {
            world_id: 77,
            temperature: 0.37,
            atmosphere: 0.74,
            flora: 0,
            herbivores: 0,
            carnivores: 0,
            colony_count: 0,
        };
        assert_eq!(ecology.t_score(), 1);
        assert_eq!(ecology.stable_foodweb_tier(), 0);
        assert_eq!(ecology.colony_capacity(), 1);

        ecology.flora = 3;
        ecology.herbivores = 2;
        ecology.carnivores = 1;
        assert_eq!(ecology.t_score(), 1, "food web does not raise climate T");
        assert_eq!(ecology.stable_foodweb_tier(), 1);
        ecology.temperature = 0.40;
        ecology.atmosphere = 0.40;
        assert_eq!(ecology.t_score(), 2);
        assert_eq!(
            ecology.colony_capacity(),
            1,
            "second tier is not stable yet"
        );
        ecology.flora = 6;
        ecology.herbivores = 4;
        ecology.carnivores = 2;
        assert_eq!(ecology.stable_foodweb_tier(), 2);
        assert_eq!(ecology.colony_capacity(), 2);
        ecology.temperature = 0.50;
        ecology.atmosphere = 0.50;
        ecology.flora = 9;
        ecology.herbivores = 6;
        ecology.carnivores = 3;
        assert_eq!(ecology.t_score(), 3);
        assert_eq!(ecology.stable_foodweb_tier(), 3);
        assert_eq!(ecology.colony_capacity(), 3);
        ecology.temperature = 0.05;
        assert_eq!(ecology.t_score(), 0);
        assert_eq!(ecology.colony_capacity(), 1, "T0 still supports an outpost");
    }

    #[test]
    fn planetary_tools_obey_foodweb_order_and_conserve_spice_on_refusal() {
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        let mut c = SpaceCampaign::default();
        c.grant_tool(tool::INTERPLANETARY_DRIVE);
        c.surveyed.push(77);
        c.ecology.push(PlanetEcology {
            world_id: 77,
            temperature: 0.30,
            atmosphere: 0.70,
            flora: 0,
            herbivores: 0,
            carnivores: 0,
            colony_count: 0,
        });

        c.planetary_tool = PlanetaryTool::SeedHerbivore;
        let before = run.spice;
        assert!(matches!(
            c.try_apply_planetary_tool(77, &mut run),
            WorldAction::Refused(_)
        ));
        assert_eq!(run.spice, before);

        c.planetary_tool = PlanetaryTool::SeedFlora;
        for _ in 0..3 {
            assert!(matches!(
                c.try_apply_planetary_tool(77, &mut run),
                WorldAction::Done(_)
            ));
        }
        c.planetary_tool = PlanetaryTool::SeedCarnivore;
        let before = run.spice;
        assert!(matches!(
            c.try_apply_planetary_tool(77, &mut run),
            WorldAction::Refused(_)
        ));
        assert_eq!(run.spice, before);

        c.planetary_tool = PlanetaryTool::SeedHerbivore;
        for _ in 0..2 {
            assert!(matches!(
                c.try_apply_planetary_tool(77, &mut run),
                WorldAction::Done(_)
            ));
        }
        c.planetary_tool = PlanetaryTool::SeedCarnivore;
        assert!(matches!(
            c.try_apply_planetary_tool(77, &mut run),
            WorldAction::Done(_)
        ));
        let ecology = c.ecology_for(77);
        assert_eq!(ecology.t_score(), 1);
        assert_eq!(ecology.stable_foodweb_tier(), 1);
        assert_eq!(run.spice, 1_000_000.0 - 6.0 * LIFE_SEED_COST, "six seed actions");

        assert!(matches!(c.try_colonize(77, &mut run), WorldAction::Done(_)));
        assert!(matches!(
            c.try_colonize(77, &mut run),
            WorldAction::Refused(_)
        ));
        // A ray's press adds its velocity to the planet's marble (no Sporebucks);
        // the climate follows as the marble rolls.
        let wallet = run.spice;
        c.planetary_tool = PlanetaryTool::Warm;
        assert!(matches!(
            c.try_apply_planetary_tool(77, &mut run),
            WorldAction::Done(_)
        ));
        c.planetary_tool = PlanetaryTool::ThinAtmosphere;
        assert!(matches!(
            c.try_apply_planetary_tool(77, &mut run),
            WorldAction::Done(_)
        ));
        assert_eq!(run.spice, wallet, "a ray costs no Sporebucks");
        assert_eq!(c.marbles[&77].v_temperature, 0.01);
        assert_eq!(c.marbles[&77].v_atmosphere, -0.01);
        // Put the marble where the old +-0.05 steps took it.
        let at = c.ecology_index(77).unwrap();
        c.ecology[at].temperature = 0.35;
        c.ecology[at].atmosphere = 0.65;
        c.marbles.clear();
        assert_eq!(c.ecology_for(77).t_score(), 2);
        for tool in [
            PlanetaryTool::SeedFlora,
            PlanetaryTool::SeedHerbivore,
            PlanetaryTool::SeedCarnivore,
        ] {
            c.planetary_tool = tool;
            let amount = match tool {
                PlanetaryTool::SeedFlora => 3,
                PlanetaryTool::SeedHerbivore => 2,
                PlanetaryTool::SeedCarnivore => 1,
                _ => unreachable!(),
            };
            for _ in 0..amount {
                assert!(matches!(
                    c.try_apply_planetary_tool(77, &mut run),
                    WorldAction::Done(_)
                ));
            }
        }
        assert_eq!(c.ecology_for(77).colony_capacity(), 2);
        assert!(matches!(c.try_colonize(77, &mut run), WorldAction::Done(_)));
        assert_eq!(c.ecology_for(77).colony_count, 2);
        assert_eq!(c.colony_income_per_period(), 0.0, "colonies make spice, not money");
    }

    #[test]
    fn a_prototype_galaxy_save_returns_home_and_drops_foreign_worlds() {
        let mut campaign: SpaceCampaign = serde_json::from_str(
            r#"{"surveyed":[1,3,5,20000],"colonies":[0,2,20001],"current_system":1500,"visited":[0,1500,77],"colony_spice":{"2":3,"20001":4},"trade_routes":{"20001":0.5}}"#,
        )
        .expect("an old save deserializes");
        assert_eq!(campaign.galaxy_version, 0);
        assert!(campaign.migrate_galaxy(5));
        assert_eq!(campaign.galaxy_version, GALAXY_VERSION);
        assert_eq!(campaign.current_system, HOME_STAR);
        assert_eq!(campaign.visited, vec![HOME_STAR]);
        // Home worlds that still exist keep their records, foreign ones go.
        assert_eq!(campaign.surveyed, vec![1, 3]);
        assert_eq!(campaign.colonies, vec![0, 2]);
        assert_eq!(campaign.colony_spice.keys().copied().collect::<Vec<_>>(), vec![2]);
        assert!(campaign.trade_routes.is_empty());
        assert!(!campaign.migrate_galaxy(5), "the migration runs once");
        // A save of the new galaxy is left alone.
        let mut fresh: SpaceCampaign = serde_json::from_str(r#"{"galaxy_version":1,"current_system":900}"#).unwrap();
        assert!(!fresh.migrate_galaxy(5));
        assert_eq!(fresh.current_system, 900);
    }

    #[test]
    fn spice_follows_the_generated_planet_not_a_hash() {
        // Generated worlds read their spice from the registered galaxy.
        let galaxy = Galaxy::generate(None);
        let plan = space_galaxy::system_plan(&galaxy, 4321, &[]);
        let body = plan
            .bodies
            .iter()
            .find(|b| b.world.is_some())
            .expect("a world");
        let info = galaxy.world_info(body.world.unwrap()).expect("world data");
        assert_eq!(Some(info.spice), body.spice);
        // The homeworld always makes red spice.
        assert_eq!(SpiceColor::for_world(0), SpiceColor::Red);
    }

    #[test]
    fn old_space_save_migrates_terraform_and_colony_flags_once() {
        let mut campaign: SpaceCampaign =
            serde_json::from_str(r#"{"surveyed":[2],"terraformed":[2],"colonies":[0,2]}"#)
                .expect("legacy space fields deserialize");
        assert!(campaign.migrate_ecology());
        let migrated = campaign.ecology_for(2);
        assert_eq!(migrated.t_score(), 1);
        assert_eq!(migrated.stable_foodweb_tier(), 1);
        assert_eq!(migrated.colony_count, 1);
        assert!(!campaign.migrate_ecology(), "migration is idempotent");
    }

    #[test]
    fn colonies_pay_no_money_and_world_baselines_are_deterministic() {
        let mut campaign = SpaceCampaign::default();
        for id in 1..=4 {
            campaign.own(id);
            let mut ecology = PlanetEcology::completed(id);
            ecology.colony_count = 3;
            campaign.ecology.retain(|entry| entry.world_id != id);
            campaign.ecology.push(ecology);
        }
        assert_eq!(campaign.colony_income_per_period(), 0.0, "retail colonies produce spice only");
        let baseline = space_galaxy::adapted_climate_baseline(44);
        assert_eq!(baseline, space_galaxy::adapted_climate_baseline(44));
        assert!((0.0..=1.0).contains(&baseline.0));
        assert!((0.0..=1.0).contains(&baseline.1));
    }

    #[test]
    fn six_spice_colors_deterministic_assignment_cargo_collection_and_trading() {
        // Deterministic assignment
        let mut found_colors = std::collections::HashSet::new();
        for id in 0..100 {
            let spice = SpiceColor::for_world(id);
            assert_eq!(spice, SpiceColor::for_world(id));
            found_colors.insert(spice);
        }
        assert_eq!(found_colors.len(), 6, "all 6 spice colors appear in the galaxy");

        // Base values scale from red (cheapest) to purple (rarest)
        assert!(SpiceColor::Red.base_value() < SpiceColor::Yellow.base_value());
        assert!(SpiceColor::Yellow.base_value() < SpiceColor::Blue.base_value());
        assert!(SpiceColor::Blue.base_value() < SpiceColor::Green.base_value());
        assert!(SpiceColor::Green.base_value() < SpiceColor::Pink.base_value());
        assert!(SpiceColor::Pink.base_value() < SpiceColor::Purple.base_value());

        // Colony production during tick
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let test_world = 5;
        campaign.own(test_world);
        let index = campaign.ensure_ecology(test_world);
        campaign.ecology[index].colony_count = 2; // the cap is per colonised world
        assert_eq!(campaign.colony_spice_capacity(test_world), 5);
        assert_eq!(campaign.colony_spice_stored(test_world), 0);

        let mut save = false;
        campaign.next_pirate_raid = f32::MAX; // no raids stealing spice in this test
        campaign.next_bio_disaster = f32::MAX;
        // One spice per ~300 s of a one-link colony (0xBD8CD0: 400).
        for _ in 0..3 {
            campaign.tick(301.0, false, &mut run.spice, &mut save);
        }
        assert_eq!(campaign.colony_spice_stored(test_world), 3);

        // Collect spice from colony into ship cargo
        assert_eq!(campaign.cargo_total(), 0);
        let color = SpiceColor::for_world(test_world);
        assert!(matches!(
            campaign.try_collect_or_trade_spice(test_world, &mut run),
            WorldAction::Done(_)
        ));
        assert_eq!(campaign.colony_spice_stored(test_world), 0);
        assert_eq!(campaign.cargo_spice.get(&color), Some(&3));
        assert_eq!(campaign.cargo_total(), 3);

        // Trade cargo with an alien empire
        let alien_system = 1;
        let alien_world = space_galaxy::world_id(alien_system, 0);
        campaign.current_system = alien_system;
        campaign.current_system_empire = Some(1); // Zelem empire
        let alien_spice = SpiceColor::for_world(alien_world);
        let price_per_crate = color.trade_price_at(alien_world, true);
        let before_spice = run.spice;

        assert!(matches!(
            campaign.try_collect_or_trade_spice(alien_world, &mut run),
            WorldAction::Done(_)
        ));
        assert_eq!(campaign.cargo_total(), 0, "cargo sold to alien empire");
        assert_eq!(run.spice, before_spice + price_per_crate * 3.0);
        let _ = alien_spice;
    }

    #[test]
    fn alien_empire_diplomacy_happy_ray_trade_routes_economic_buyout_and_military_capture() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 10_000_000.0;
        let alien_system = 2;
        let alien_world = space_galaxy::world_id(alien_system, 0);
        let empire_id = 4; // Thelos: a Diplomat personality (PersonalityNice +3)
        campaign.current_system = alien_system;
        campaign.current_system_empire = Some(empire_id);
        assert!((campaign.relationship(empire_id) - 3.0).abs() < 1e-4);
        assert_eq!(campaign.empire_relation(empire_id), 30, "legacy x10 scale");
        assert_eq!(campaign.relation_label(empire_id), "Pleased");

        // Comms at 3: no gift needed; a mission and a trade route.
        assert!(matches!(
            campaign.try_communicate(alien_world, &mut run),
            WorldAction::Done(msg) if msg.contains("Opened a trade route") && msg.contains("Mission accepted")
        ));
        assert_eq!(campaign.trade_routes.get(&alien_world), Some(&0.0));
        assert!(campaign.active_alien_mission.is_some());
        assert!(campaign.badge_stats.empires_met.contains(&empire_id));

        // Happy Ray: the upgrade is bought once (200,000), each use is a
        // GoodToolUse of `toolRelationshipDelta` +0.005 (0x105987C).
        campaign.planetary_tool = PlanetaryTool::HappyRay;
        let (before, rel) = (run.spice, campaign.relationship(empire_id));
        assert!(matches!(
            campaign.try_apply_planetary_tool(alien_world, &mut run),
            WorldAction::Done(_)
        ));
        assert_eq!(run.spice, before - HAPPY_RAY_COST);
        assert!((campaign.relationship(empire_id) - rel - 0.005).abs() < 1e-4);
        let before = run.spice;
        campaign.try_apply_planetary_tool(alien_world, &mut run);
        assert_eq!(run.spice, before, "an owned upgrade costs nothing more");

        // Colonize/buyout refused while the trade route is maturing.
        assert!(matches!(
            campaign.try_colonize(alien_world, &mut run),
            WorldAction::Refused(msg) if msg.contains("belongs to the")
        ));
        // 0x1038D39: every route matures at `tradeCaptureMinRate` 4.2e-5 per
        // ms (percent): 100 % after about 2381 s.
        let mut save = false;
        campaign.tick(1.0, false, &mut run.spice, &mut save);
        let step = campaign.trade_routes[&alien_world];
        assert!((step - 1.0 / 2381.0).abs() < 1e-6, "one second of maturity: {step}");
        campaign.trade_routes.insert(alien_world, 1.0 - 2.0 * step);
        campaign.tick(1.0, false, &mut run.spice, &mut save);
        assert!(campaign.trade_routes[&alien_world] < 1.0);
        campaign.tick(2.0, false, &mut run.spice, &mut save);
        assert_eq!(campaign.trade_routes.get(&alien_world), Some(&1.0));

        // Purchase System at the retail tier price.
        let price = rules::system_purchase_cost(campaign.ecology_for(alien_world).t_score());
        let before_spice = run.spice;
        assert!(matches!(
            campaign.try_colonize(alien_world, &mut run),
            WorldAction::Done(msg) if msg.contains("Purchased")
        ));
        assert_eq!(run.spice, before_spice - price);
        assert!(campaign.colonies.contains(&alien_world));
        assert!(campaign.conquered_worlds.contains(&alien_world));

        // Military conquest of a Grox world: the Grox are at war from first contact.
        let hostile_system = 3;
        let hostile_world = space_galaxy::world_id(hostile_system, 0);
        let hostile_empire = 0;
        campaign.current_system = hostile_system;
        campaign.current_system_empire = Some(hostile_empire);
        campaign.meet_empire(hostile_empire);
        assert_eq!(campaign.relation_label(hostile_empire), "At War");
        assert!(campaign.wars.contains(&hostile_empire));
        assert_eq!(campaign.empire_defense_hp(hostile_world), EMPIRE_COLONY_MAX_HP);
        assert!(matches!(
            campaign.try_colonize(hostile_world, &mut run),
            WorldAction::Refused(msg) if msg.contains("defenses")
        ));
        // It surrenders at `spaceWarSurrenderThreshold` (35% left).
        assert!(campaign.damage_empire_colony(hostile_world, EMPIRE_COLONY_MAX_HP / 2.0).is_some());
        assert!(!campaign.empire_colony_surrendered(hostile_world));
        let surrendered_msg = campaign.damage_empire_colony(hostile_world, EMPIRE_COLONY_MAX_HP * 0.2);
        assert!(surrendered_msg.as_ref().is_some_and(|m| m.contains("surrendered")));
        assert!(matches!(
            campaign.try_colonize(hostile_world, &mut run),
            WorldAction::Done(msg) if msg.contains("Captured")
        ));
        assert!(campaign.colonies.contains(&hostile_world));
        assert!(campaign.conquered_worlds.contains(&hostile_world));
    }

    #[test]
    fn autopilot_and_galaxy_filters() {
        let mut motion = ShipMotion::default();
        assert_eq!(motion.autopilot_target, None);
        motion.autopilot_target = Some(3);
        assert_eq!(motion.autopilot_target, Some(3));

        // Galaxy filters cycle cleanly through all modes
        let mut filter = crate::space_view::GalaxyFilter::All;
        assert_eq!(filter.label(), "All Stars");
        filter = filter.next();
        assert_eq!(filter, crate::space_view::GalaxyFilter::Colonies);
        assert_eq!(filter.label(), "My Colonies");
        filter = filter.next();
        assert_eq!(filter, crate::space_view::GalaxyFilter::Empires);
        assert_eq!(filter.label(), "Empires");
        filter = filter.next();
        assert_eq!(filter, crate::space_view::GalaxyFilter::Visited);
        assert_eq!(filter.label(), "Visited");
        filter = filter.next();
        assert_eq!(filter, crate::space_view::GalaxyFilter::Trade);
        assert_eq!(filter.label(), "Trade Routes");
        filter = filter.next();
        assert_eq!(filter, crate::space_view::GalaxyFilter::All);
    }

    #[test]
    fn badges_captain_ranks_and_stat_scaling() {
        let mut campaign = SpaceCampaign::default();
        assert_eq!(campaign.captain_rank(), 0);
        assert_eq!(campaign.captain_rank_title(), "Captain");
        assert_eq!(campaign.total_badge_points(), 0, "the captain badge needs Flight 101");
        campaign.tutorial.step = rules::TutorialStep::Story201;
        assert_eq!(campaign.total_badge_points(), 1, "the captain badge");
        assert_eq!(campaign.cargo_capacity_limit(), 10);
        assert_eq!(campaign.max_reactor_energy(), 100.0);
        assert_eq!(campaign.max_jump_distance(), 0.0);

        campaign.visited = (0..50).collect(); // explorer 15 / 50 -> level 2 (2 + 4)
        assert_eq!(campaign.badge_tier(SpaceBadge::Explorer), 2);
        campaign.colonies = (0..6).collect(); // 5 new colonies -> colonist 1 (2)
        assert_eq!(campaign.badge_tier(SpaceBadge::Colonist), 1);
        campaign.badge_stats.wars_started = 3; // warmonger 1 / 3 -> 1 + 2
        assert_eq!(campaign.badge_tier(SpaceBadge::Warmonger), 2);
        campaign.badge_stats.eradicate_missions = 5; // cleaner 1 (2)
        assert_eq!(campaign.badge_tier(SpaceBadge::Cleanser), 1);
        // Sightseer counts special stars only (`space_endgame`); the empire
        // badge counts systems with a colony (home + 5 new colonies here
        // map to their systems).
        let empire = rules::badge_points(rules::badge_family("empire").unwrap(), campaign.badge_family_level(rules::badge_family("empire").unwrap()));
        assert_eq!(campaign.total_badge_points(), 1 + 6 + 2 + 3 + 2 + empire);
        assert_eq!(campaign.captain_rank(), 1);
        assert_eq!(campaign.captain_rank_title(), "Commander");
        // Ranks no longer grow the ship (cargo, energy, jump range).
        assert_eq!(campaign.cargo_capacity_limit(), 10);
        assert_eq!(campaign.max_reactor_energy(), 100.0);
        assert_eq!(campaign.max_jump_distance(), 0.0);
    }

    #[test]
    fn staff_of_life_terraforming() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let world = 1005;
        campaign.surveyed.push(world);
        campaign.planetary_tool = PlanetaryTool::StaffOfLife;

        // Fails with 0 charges
        assert_eq!(campaign.staff_of_life_charges, 0);
        assert!(matches!(
            campaign.try_apply_planetary_tool(world, &mut run),
            WorldAction::Refused(msg) if msg.contains("0 charges")
        ));

        // Bestow 42 charges
        campaign.staff_of_life_charges = 42;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(action, WorldAction::Done(_)));
        assert_eq!(campaign.staff_of_life_charges, 41);
        assert!(campaign.terraformed.contains(&world));

        let ecology = campaign.ecology_for(world);
        assert_eq!(ecology.t_score(), 3);
        assert_eq!(ecology.stable_foodweb_tier(), 3);
        assert_eq!(ecology.flora, 9);
        assert_eq!(ecology.herbivores, 6);
        assert_eq!(ecology.carnivores, 3);
    }

    #[test]
    fn tractor_beam_and_relic_vault() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();

        // 1. Biohazard outbreak distress call
        let infested_world = 1002;
        campaign.colonies.push(infested_world);
        campaign.active_distress = Some(DistressSignal {
            system_id: 1,
            world_id: infested_world,
            kind: DistressKind::BiohazardOutbreak,
            time_remaining: 100.0,
        });

        let before_spice = run.spice;
        let action = campaign.try_tractor_beam(infested_world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("eradicated")));
        assert_eq!(campaign.active_distress, None);
        assert_eq!(campaign.biohazards_cleansed, 1);
        assert_eq!(run.spice, before_spice, "biospherecollapse pays no money");

        // 2. Deterministic world artifact retrieval
        let mut relic_world = 1000;
        while world_artifact(relic_world).is_none() {
            relic_world += 1;
        }
        let artifact = world_artifact(relic_world).unwrap();

        let action2 = campaign.try_tractor_beam(relic_world, &mut run);
        assert!(matches!(action2, WorldAction::Done(msg) if msg.contains(&artifact.name)));
        assert_eq!(campaign.artifacts.len(), 1);
        assert!(campaign.collected_artifacts.contains(&relic_world));

        // Second beam attempt reports already retrieved
        let action3 = campaign.try_tractor_beam(relic_world, &mut run);
        assert!(matches!(action3, WorldAction::Refused(msg) if msg.contains("already been retrieved")));

        // 3. Trade relic at alien empire world
        let emp_sys = 2;
        let emp_world = space_galaxy::world_id(emp_sys, 0);
        let emp_idx = 1;
        campaign.current_system = emp_sys;
        campaign.current_system_empire = Some(emp_idx);
        campaign.empire_relations.insert(emp_idx, 30);

        let pre_trade_spice = run.spice;
        let trade_action = campaign.try_collect_or_trade_spice(emp_world, &mut run);
        assert!(matches!(trade_action, WorldAction::Done(msg) if msg.contains("rare(s)")));
        assert_eq!(campaign.artifacts.len(), 0);
        assert_eq!(run.spice, pre_trade_spice + artifact.value as f32);
    }

    #[test]
    fn pirate_raid_distress_and_defense() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let colony_world = 1003;
        campaign.colonies.push(colony_world);
        let colony_sys = space_galaxy::system_of_world(colony_world);
        campaign.current_system = colony_sys;

        campaign.active_distress = Some(DistressSignal {
            system_id: colony_sys,
            world_id: colony_world,
            kind: DistressKind::PirateRaid,
            time_remaining: 120.0,
        });

        let pre_spice = run.spice;
        campaign.register_kill(&mut run);
        assert_eq!(campaign.active_distress, None);
        assert_eq!(campaign.pirate_raids_defended, 1);
        assert!(run.spice >= pre_spice, "a kill may drop retail loot, never costs money");
    }

    #[test]
    fn space_archetypes_and_superpowers_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1000.0;

        // Retail recharge per superpower tool; the Wanderer has none.
        for archetype in SpaceArchetype::ALL {
            assert!(!archetype.title().is_empty());
            assert!(!archetype.superpower_name().is_empty());
            assert!(!archetype.superpower_description().is_empty());
        }
        assert_eq!(SpaceArchetype::Shaman.cooldown_secs(), 60.0);
        assert_eq!(SpaceArchetype::Knight.cooldown_secs(), 120.0);
        assert_eq!(SpaceArchetype::Bard.cooldown_secs(), 600.0);
        assert_eq!(SpaceArchetype::Zealot.cooldown_secs(), 2400.0);
        assert_eq!(SpaceArchetype::Wanderer.cooldown_secs(), 0.0);

        // 1. Warrior: Raider Rally (1,200 s).
        campaign.set_archetype(SpaceArchetype::Warrior);
        assert_eq!(campaign.archetype.superpower_name(), "Raider Rally");
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Raider Rally")));
        assert_eq!(campaign.superweapon_cooldown, 1200.0);
        let cooldown_action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(cooldown_action, WorldAction::Refused(msg) if msg.contains("recharging")));
        campaign.superweapon_cooldown = 0.0;

        // 2. Zealot: Fanatical Frenzy converts the world; the target takes the
        //    tool's -10 (BadToolUse) and goes to war.
        campaign.set_archetype(SpaceArchetype::Zealot);
        let emp_sys = 3;
        let emp_world = space_galaxy::world_id(emp_sys, 0);
        let emp_idx = 1;
        campaign.current_system = emp_sys;
        campaign.current_system_empire = Some(emp_idx);
        let action = campaign.try_activate_superweapon(&mut run, Some(emp_world));
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Fanatical Frenzy")));
        assert!(campaign.colonies.contains(&emp_world));
        assert!(campaign.conquered_worlds.contains(&emp_world));
        // OnHit 0x1059C95: the whole star changes empire, not only the planet aimed at.
        let held_before = (0..space_galaxy::WORLDS_PER_SYSTEM)
            .filter(|slot| {
                *slot == 0
                    || space_galaxy::world_info(space_galaxy::world_id(emp_sys, *slot)).is_some_and(|i| i.tech == space_galaxy::TechLevel::Empire)
            })
            .count();
        let converted = (0..space_galaxy::WORLDS_PER_SYSTEM)
            .filter(|slot| campaign.conquered_worlds.contains(&space_galaxy::world_id(emp_sys, *slot)))
            .count();
        assert_eq!(converted, held_before, "every planet the empire held in the system");
        assert!(campaign.relationship(emp_idx) <= rules::WAR_THRESHOLD);
        assert!(campaign.wars.contains(&emp_idx));
        campaign.superweapon_cooldown = 0.0;

        // 3. Scientist: Gravitation Wave flattens the defenses (-20 with the target).
        campaign.set_archetype(SpaceArchetype::Scientist);
        let emp_sys2 = 4;
        let emp_world2 = space_galaxy::world_id(emp_sys2, 0);
        let emp_idx2 = 2;
        campaign.current_system = emp_sys2;
        campaign.current_system_empire = Some(emp_idx2);
        let action = campaign.try_activate_superweapon(&mut run, Some(emp_world2));
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Gravitation Wave")));
        assert_eq!(campaign.empire_defense_hp(emp_world2), 0.0);
        assert!(campaign.relationship(emp_idx2) <= rules::WAR_THRESHOLD);
        campaign.superweapon_cooldown = 0.0;

        // 4. Ecologist: Safari Vacuum.
        campaign.set_archetype(SpaceArchetype::Ecologist);
        let pre_cargo = campaign.cargo_total();
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Safari Vacuum")));
        assert_eq!(campaign.cargo_wildlife.flora, 2);
        assert_eq!(campaign.cargo_wildlife.herbivores, 2);
        assert_eq!(campaign.cargo_wildlife.carnivores, 2);
        assert_eq!(campaign.cargo_total(), pre_cargo + 6);
        campaign.superweapon_cooldown = 0.0;

        // 5. Trader: Cash Infusion.
        campaign.set_archetype(SpaceArchetype::Trader);
        let emp_sys3 = 5;
        let emp_world3 = space_galaxy::world_id(emp_sys3, 0);
        campaign.current_system = emp_sys3;
        campaign.current_system_empire = Some(3);
        // The gate (0x1056130, log 0xCCBE06F7) wants a trade route with the planet's empire;
        // the use creates none (OnHit 0x1059BC4 only sets an existing route's maturity).
        let refused = campaign.try_activate_superweapon(&mut run, Some(emp_world3));
        assert!(matches!(refused, WorldAction::Refused(msg) if msg.contains("trade route")));
        assert!(!campaign.trade_routes.contains_key(&emp_world3));
        assert_eq!(campaign.superweapon_cooldown, 0.0);
        campaign.trade_routes.insert(emp_world3, 0.25);
        let action = campaign.try_activate_superweapon(&mut run, Some(emp_world3));
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Cash Infusion")));
        assert_eq!(campaign.trade_routes[&emp_world3], 1.0);
        campaign.superweapon_cooldown = 0.0;

        // 6. Bard: Soothing Song.
        campaign.set_archetype(SpaceArchetype::Bard);
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Soothing Song")));
        assert_eq!(campaign.siren_song_active, 120.0);
        campaign.superweapon_cooldown = 0.0;

        // 7. Diplomat: Static Cling.
        campaign.set_archetype(SpaceArchetype::Diplomat);
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Static Cling")));
        assert_eq!(campaign.static_cling_active, 45.0);
        campaign.superweapon_cooldown = 0.0;

        // 8. Knight: Summon Mini-U (an ally-health wingman, no rank needed).
        campaign.set_archetype(SpaceArchetype::Knight);
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Mini-U")));
        assert_eq!(campaign.fleet.len(), 1);
        assert_eq!(campaign.fleet[0].name, "Mini-U Drone #1");
        assert_eq!(campaign.fleet[0].max_hp, rules::UFO_HEALTH_ALLY[0]);
        campaign.superweapon_cooldown = 0.0;

        // 9. Shaman: Return Ticket (60 s).
        campaign.set_archetype(SpaceArchetype::Shaman);
        campaign.current_system = 77;
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Return Ticket")));
        assert_eq!(campaign.current_system, HOME_STAR);
        assert_eq!(campaign.superweapon_cooldown, 60.0);

        // 10. Wanderer: no superpower.
        campaign.set_archetype(SpaceArchetype::Wanderer);
        let action = campaign.try_activate_superweapon(&mut run, None);
        assert!(matches!(action, WorldAction::Refused(msg) if msg.contains("no archetype superpower")));

        // Timers count down in the tick.
        let mut save = false;
        campaign.tick(10.0, false, &mut run.spice, &mut save);
        assert_eq!(campaign.superweapon_cooldown, 50.0);
        assert_eq!(campaign.siren_song_active, 110.0);
        assert_eq!(campaign.static_cling_active, 35.0);
    }

    #[test]
    fn fleet_allies_and_wingmen_escorts_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1000.0;

        // Captain: no posse slots; one per two promotions (badgeEarnPosse).
        assert_eq!(campaign.captain_rank(), 0);
        assert_eq!(campaign.max_fleet_size(), 0);
        campaign.tutorial.step = rules::TutorialStep::Story201; // captain 1
        campaign.visited = (0..50).collect(); // explorer 2: 6 points -> Commander
        assert_eq!(campaign.captain_rank(), 1);
        assert_eq!(campaign.max_fleet_size(), 0);
        campaign.badge_stats.wars_started = 10; // warmonger 4: 13 more -> Commodore
        assert_eq!(campaign.captain_rank(), 2);
        assert_eq!(campaign.max_fleet_size(), 1);

        // Colonies give reports, not escorts (retail has no colony posse).
        let action = campaign.try_communicate(0, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Colonial Command")));
        assert!(campaign.fleet.is_empty());

        // An allied empire's ship joins the posse for free.
        let emp_sys = 12;
        let emp_world = space_galaxy::world_id(emp_sys, 0);
        let emp_idx = 4; // Thelos (Diplomat personality, +3 at contact)
        campaign.current_system = emp_sys;
        campaign.current_system_empire = Some(emp_idx);
        campaign.meet_empire(emp_idx);
        campaign.allies.push(emp_idx);
        let action2 = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action2, WorldAction::Done(msg) if msg.contains("joined your posse")));
        assert_eq!(campaign.fleet.len(), 1);
        assert_eq!(campaign.fleet[0].empire, Some(emp_idx));
        assert_eq!(campaign.fleet[0].hp, rules::UFO_HEALTH_ALLY[0]);
        assert_eq!(run.spice, 1000.0, "allies join for free");

        // Docking does not restore escort HP (no retail source, see `tick`).
        campaign.fleet[0].hp = 30.0;
        campaign.dock(0);
        assert_eq!(campaign.fleet[0].hp, 30.0);
        assert_eq!(campaign.fleet.len(), campaign.max_fleet_size());
    }

    #[test]
    fn wildlife_abduction_and_planetary_seeding_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 200.0;

        // 1. Homeworld (world 0) has a complete food web.
        assert_eq!(campaign.cargo_wildlife.total(), 0);
        let initial_cargo = campaign.cargo_total();

        // Abduct specimens with Tractor Beam (U)
        let action1 = campaign.try_tractor_beam(0, &mut run);
        assert!(matches!(action1, WorldAction::Done(msg) if msg.contains("abducted wild")));
        assert_eq!(campaign.cargo_wildlife.total(), 1);
        assert_eq!(campaign.cargo_total(), initial_cargo + 1);

        // Abduct two more specimens
        campaign.try_tractor_beam(0, &mut run);
        campaign.try_tractor_beam(0, &mut run);
        assert_eq!(campaign.cargo_wildlife.total(), 3);

        // 2. Fly to an unseeded terraformed world
        let target_world = 1005;
        campaign.surveyed.push(target_world);
        let idx = campaign.ensure_ecology(target_world);
        campaign.ecology[idx].temperature = 0.50;
        campaign.ecology[idx].atmosphere = 0.50; // T3 climate potential!
        assert_eq!(campaign.ecology[idx].t_score(), 3);
        assert_eq!(campaign.ecology[idx].flora, 0);

        // Apply SeedFlora tool using cargo specimen for FREE (0 spice cost!)
        campaign.planetary_tool = PlanetaryTool::SeedFlora;
        let pre_spice = run.spice;
        let flora_before = campaign.cargo_wildlife.flora;
        if flora_before > 0 {
            let seed_action = campaign.try_apply_planetary_tool(target_world, &mut run);
            assert!(matches!(seed_action, WorldAction::Done(msg) if msg.contains("cargo flora (free)")));
            assert_eq!(run.spice, pre_spice); // 0 spice spent!
            assert_eq!(campaign.cargo_wildlife.flora, flora_before - 1);
            assert_eq!(campaign.ecology[idx].flora, 1);
        }
    }

    #[test]
    fn grox_deep_core_barrier_and_interceptors_test() {
        let galaxy = Galaxy::generate(None);

        // The Grox are one empire of about 2,400 stars scattered within 105
        // units of the core (`grobStarsSpreadRadius`).
        let grox: Vec<_> = galaxy.stars.iter().filter(|s| s.empire == Some(0)).collect();
        assert!(grox.len() > 1000, "Grox must control a large core empire");
        assert_eq!(grox.len() as u32, galaxy.grox_stars);
        assert!(grox.iter().all(|s| s.position.length() <= 105.0));

        // The core itself is not Grox territory (it allows access to the Galactic Core).
        let core_center = galaxy.nearest_to(Vec2::ZERO).unwrap();
        assert!(galaxy.is_galactic_core(core_center));
        assert_eq!(galaxy.star(core_center).unwrap().empire, None);

        // The Grox personality starts at the war threshold (PersonalityMean -7).
        let mut campaign = SpaceCampaign::default();
        assert_eq!(campaign.empire_relation(0), -70);
        assert_eq!(campaign.relation_label(0), "Furious", "not met yet");
        campaign.meet_empire(0);
        assert_eq!(campaign.relation_label(0), "At War");
    }

    #[test]
    fn monolith_uplift_and_species_ascension_test() {
        // The headless world: no registered galaxy, so the Monolith picks one of the legacy
        // empires (1042 -> id 3, the Avorni: Scientist, 0 at contact, so UpliftedCiv gives
        // exactly +5). Another test entering the Space stage registers the process galaxy
        // (`setup_space`), whose empires have other personalities; this test must not see it.
        let _headless = space_galaxy::pin_registered_galaxy(None);
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        let world = 1042;
        campaign.surveyed.push(world);

        // Barren world with 0 flora/fauna cannot be uplifted
        let barren = campaign.ensure_ecology(world);
        campaign.ecology[barren].temperature = 0.0;
        campaign.ecology[barren].atmosphere = 0.0;
        campaign.planetary_tool = PlanetaryTool::Monolith;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        // `cToolStrategy::Update` 0x1056130: the monolith needs animal species on the planet.
        assert!(matches!(action, WorldAction::Refused(msg) if msg.contains("no animals")));
        assert_eq!(run.spice, 1_000_000.0, "a refusal costs nothing");

        let idx = campaign.ensure_ecology(world);
        campaign.ecology[idx].temperature = 0.5;
        campaign.ecology[idx].atmosphere = 0.5;
        campaign.ecology[idx].flora = 1;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(action, WorldAction::Refused(msg) if msg.contains("no animals")), "plants alone are not enough");
        campaign.ecology[idx].herbivores = 1;

        // The retail Monolith (store 500,000, consumed).
        let pre_spice = run.spice;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Monolith deployed")));
        assert_eq!(run.spice, pre_spice - MONOLITH_COST);
        assert_eq!(campaign.species_uplifted, 1);
        // The uplifted empire remembers it (UpliftedCiv +5).
        let emp = campaign.world_empire(world).expect("uplifted empire");
        assert!((campaign.relationship(emp) - 5.0).abs() < 1e-4);
        assert_eq!(campaign.relation_label(emp), "Pleased");
        assert_eq!(campaign.ecology[idx].colony_count, 1);
    }

    /// The other half of the pick: with a galaxy registered the uplifted species joins one
    /// of that galaxy's empires, whose own personality sets how the player starts out with it
    /// (pinned to the generated galaxy so it does not depend on what other tests registered).
    #[test]
    fn monolith_uplift_joins_an_empire_of_the_registered_galaxy() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        let world = 1042;
        campaign.surveyed.push(world);
        let idx = campaign.ensure_ecology(world);
        campaign.ecology[idx].temperature = 0.5;
        campaign.ecology[idx].atmosphere = 0.5;
        campaign.ecology[idx].flora = 1;
        // `cToolStrategy::Update` 0x1056130: the monolith needs animal species (Landing 194).
        campaign.ecology[idx].herbivores = 1;
        campaign.planetary_tool = PlanetaryTool::Monolith;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Monolith deployed")));
        assert_eq!(run.spice, 1_000_000.0 - MONOLITH_COST);
        let emp = campaign.world_empire(world).expect("uplifted empire");
        assert_eq!(emp, space_galaxy::pick_empire(world), "the pick is deterministic");
        assert!(galaxy.empires.iter().any(|e| e.id == emp), "an empire of the registered galaxy");
        // UpliftedCiv raised the standing it would have had at first contact (unless that
        // was already at the +10 cap).
        let at_contact = SpaceCampaign::default().relationship(emp);
        let now = campaign.relationship(emp);
        assert!(now > at_contact || now == rules::RELATIONSHIP_CAP, "{now} vs {at_contact}");
        assert_eq!(campaign.species_uplifted, 1);
        assert_eq!(campaign.ecology[idx].colony_count, 1);
    }

    #[test]
    fn planet_buster_and_galactic_code_tribunal_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 10_000_000.0;

        let world = 1088;
        campaign.colonies.push(world);
        campaign.trade_routes.insert(world, 0.5);
        campaign.colony_spice.insert(world, 5);
        let idx = campaign.ensure_ecology(world);
        campaign.ecology[idx].colony_count = 2;
        campaign.ecology[idx].flora = 3;

        // Planet Buster (store 5,000,000, consumed).
        campaign.planetary_tool = PlanetaryTool::PlanetBuster;
        let action = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("PLANET BUSTER DETONATED")));
        assert_eq!(run.spice, 10_000_000.0 - PLANET_BUSTER_COST);
        assert_eq!(campaign.planets_busted, 1);
        assert!(!campaign.colonies.contains(&world));
        assert!(!campaign.trade_routes.contains_key(&world));
        assert_eq!(campaign.ecology[idx].flora, 0);
        assert_eq!(campaign.ecology[idx].colony_count, 0);

        // The hit's galactic delta (-7, radius 10) is a CommittedAtrocity of 7 told to
        // the empires near the active star (0x1059A1A), not to every empire.
        let far = campaign.relationships.keys().copied().collect::<Vec<_>>();
        assert!(far.iter().all(|e| campaign.relationships[e].events.get(&RelationshipEvent::CommittedAtrocity).is_none_or(|x| *x == 7.0)));
    }

    #[test]
    fn a_galactic_delta_reaches_the_empires_near_the_active_star() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        // A star of an empire, and one far from every empire's stars.
        let star = (0..galaxy.stars.len() as u32)
            .find(|i| {
                let s = galaxy.star(*i).unwrap();
                s.empire.is_some_and(|e| e != 0) && s.tech == crate::space_worldgen::TechLevel::Empire
            })
            .expect("an empire star");
        let owner = galaxy.star(star).unwrap().empire.unwrap();
        let mut campaign = SpaceCampaign::default();
        campaign.current_system = star;
        let near = campaign.galactic_witnesses_within(10.0);
        assert!(near.contains(&owner), "the owner of the hit star hears it: {near:?}");
        // Within 10 galaxy units of the star, every owner of an empire star.
        let mut expected = std::collections::BTreeSet::new();
        expected.insert(owner);
        for (other, _) in galaxy.stars_within(star, 10.0) {
            let s = galaxy.star(other).unwrap();
            if s.tech == crate::space_worldgen::TechLevel::Empire {
                expected.extend(s.empire);
            }
        }
        assert_eq!(near.into_iter().collect::<std::collections::BTreeSet<_>>(), expected);
        // The radius is capped at 30 (0x13F0E68).
        assert_eq!(
            campaign.galactic_witnesses_within(1000.0),
            campaign.galactic_witnesses_within(30.0),
            "a larger record radius reads as 30"
        );
        // The delta is a CommittedAtrocity of its magnitude (0x068B2971), told to each.
        let notes = campaign.apply_galactic_delta(-7.0, 10.0);
        let _ = notes;
        for e in &expected {
            assert_eq!(campaign.relationships[e].events.get(&RelationshipEvent::CommittedAtrocity), Some(&7.0), "empire {e}");
        }
        // A positive galactic delta is a GoodToolUse.
        let mut kind = SpaceCampaign::default();
        kind.current_system = star;
        kind.apply_galactic_delta(2.0, 10.0);
        assert_eq!(kind.relationships[&owner].events.get(&RelationshipEvent::GoodToolUse), Some(&2.0));
        // An empire the player has not met is not told when no star of it is near (and none is
        // met by hearsay): the Grox alliance reaches only the empires met.
        let mut grox = SpaceCampaign::default();
        grox.meet_empire(owner);
        assert_eq!(grox.galactic_witnesses(), vec![owner]);
    }

    #[test]
    fn a_city_holds_the_buildings_its_terraform_level_allows() {
        // `GetMaxBuildings` 0xBBE760: one building at T0, 6 / 12 / 20 at T1 / T2 / T3.
        assert_eq!(
            (0..=3).map(rules::colony_max_buildings).collect::<Vec<_>>(),
            vec![1, 6, 12, 20]
        );
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        let world = 101;
        campaign.colonies.push(world);
        let idx = campaign.ensure_ecology(world);
        campaign.ecology[idx].colony_count = 1;
        campaign.ecology[idx].temperature = 0.0;
        campaign.ecology[idx].flora = 0;
        campaign.ecology[idx].herbivores = 0;
        campaign.ecology[idx].carnivores = 0;
        assert_eq!(campaign.ecology[idx].max_buildings(), 1);
        let first = campaign.build_city_structure(world, 0, 0, CityBuilding::House, &mut run);
        assert!(matches!(first, WorldAction::Done(_)), "{first:?}");
        let second = campaign.build_city_structure(world, 0, 1, CityBuilding::Factory, &mut run);
        assert!(matches!(second, WorldAction::Refused(_)), "T0: one building per city");
        // A stable food web and the bullseye raise the limit (T1 = 6, T3 = 20).
        campaign.ecology[idx] = PlanetEcology { colony_count: 1, ..PlanetEcology::completed(world) };
        campaign.fit_city_layouts();
        assert_eq!(campaign.city_layouts[&world][0].capacity(), 20);
        assert_eq!(campaign.city_layouts[&world][0].slots.len(), 20);
        for slot in 1..20 {
            let action = campaign.build_city_structure(world, 0, slot, CityBuilding::House, &mut run);
            assert!(matches!(action, WorldAction::Done(_)), "slot {slot}: {action:?}");
        }
        assert_eq!(campaign.city_layouts[&world][0].building_count(), 20);
        assert!(matches!(
            campaign.build_city_structure(world, 0, 20, CityBuilding::House, &mut run),
            WorldAction::Refused(_)
        ));
        // The T-score falls to T1: the city keeps 6 buildings (0xBBE8B0 trims the excess).
        campaign.ecology[idx].flora = 3;
        campaign.ecology[idx].herbivores = 2;
        campaign.ecology[idx].carnivores = 1;
        assert_eq!(campaign.ecology[idx].max_buildings(), 6);
        let removed = campaign.fit_city_layouts();
        assert_eq!(removed, 14);
        assert_eq!(campaign.city_layouts[&world][0].building_count(), 6);
        assert_eq!(campaign.city_layouts[&world][0].slots.len(), 6);
        // A layout saved before the limits existed keeps its buildings until a drop.
        let mut legacy = CityLayout::new();
        legacy.slots = vec![Some(CityBuilding::House); 6];
        assert_eq!(legacy.capacity, 0);
        assert_eq!(legacy.fit_capacity(1), 0, "an old save is not trimmed on load");
        assert_eq!(legacy.building_count(), 6);
        assert_eq!(legacy.capacity(), 1, "but nothing more is built");
    }

    #[test]
    fn the_colonise_mission_skips_a_world_that_already_has_a_t_score() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut campaign = SpaceCampaign::default();
        let first = colonize_mission_target(&campaign, &galaxy);
        let Some(first) = first else {
            return;
        };
        // 0xC4E440 against the unset `missionColonizePlanetFullRow`: a planet with a
        // T-score is not a target (the port's T-score is the stable food-web tier).
        let idx = campaign.ensure_ecology(first);
        campaign.ecology[idx] = PlanetEcology::completed(first);
        let second = colonize_mission_target(&campaign, &galaxy);
        assert_ne!(second, Some(first), "a terraformed planet is not offered");
        // Another mission's target scores 0.1 and loses to a planet nobody targets (0xC44E60).
        let mut other = SpaceCampaign::default();
        other.active_alien_mission = Some(AlienMission {
            empire: 1,
            kind: AlienMissionKind::TerraformWorld,
            reward_spice: 0.0,
            progress: 0,
            target_count: 1,
            target_world: Some(first),
            description: String::new(),
            retail: None,
            time_left: None,
            ..Default::default()
        });
        let star = space_galaxy::system_of_world(first);
        let record = galaxy.system(star);
        let candidates = record
            .planets
            .iter()
            .filter(|p| p.world_slot.is_some() && p.moon_of.is_none() && p.tech == crate::space_worldgen::TechLevel::Creature)
            .count();
        let picked = colonize_mission_target(&other, &galaxy);
        if candidates > 1 {
            assert_ne!(picked, Some(first), "the planet another mission targets is the last choice");
        } else {
            assert!(picked.is_some());
        }
    }

    #[test]
    fn fetch_missions_take_a_target_of_their_tech_level_and_pay_on_delivery() {
        use rules::MissionKind as K;
        use crate::space_worldgen::TechLevel as T;
        // 0xC523E0: tribe / civilization / empire citizens want tech 2 / 4 / 5; an artifact any
        // planet (an empire's 1.2).
        assert_eq!(rules::fetch_target_score(K::FetchTribeCitizen, T::Tribe), Some(1.0));
        assert_eq!(rules::fetch_target_score(K::FetchTribeCitizen, T::Civilization), None);
        assert_eq!(rules::fetch_target_score(K::FetchCivCitizen, T::Civilization), Some(1.0));
        assert_eq!(rules::fetch_target_score(K::FetchCivCitizen, T::City), None);
        assert_eq!(rules::fetch_target_score(K::FetchEmpireCitizen, T::Empire), Some(1.0));
        assert_eq!(rules::fetch_target_score(K::FetchEmpireCitizen, T::Tribe), None);
        assert_eq!(rules::fetch_target_score(K::FetchArtifact, T::Empire), Some(1.2));
        assert_eq!(rules::fetch_target_score(K::FetchArtifact, T::Tribe), Some(1.0));

        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut campaign = SpaceCampaign::default();
        // Stand at a star of the middle of the galaxy that has tribe planets in range.
        let here = (1..galaxy.stars.len() as u32)
            .step_by(97)
            .find(|s| {
                campaign.current_system = *s;
                campaign.fetch_target(K::FetchTribeCitizen, 3).is_some()
            })
            .expect("a star near a tribe planet");
        campaign.current_system = here;
        let world = campaign.fetch_target(K::FetchTribeCitizen, 3).unwrap();
        assert_eq!(galaxy.world_info(world).unwrap().tech, T::Tribe);
        let dist = galaxy.distance(here, space_galaxy::system_of_world(world));
        assert!(dist <= rules::mission_row(K::FetchTribeCitizen).range_max + 1e-3, "{dist}");

        // The mission: the beam over the target world abducts the citizen, the giver pays.
        let mut run = RunProgress::default();
        let reward = 20_000.0;
        campaign.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::Fetch,
            reward_spice: reward,
            progress: 0,
            target_count: 1,
            target_world: Some(world),
            description: String::new(),
            retail: Some(K::FetchTribeCitizen),
            time_left: Some(180.0),
            ..Default::default()
        });
        let mission = campaign.active_alien_mission.clone().unwrap();
        assert!(!mission.is_complete(&campaign));
        // Not over the target world: the beam finds nothing to fetch.
        let other_world = if world == 5 { 6 } else { 5 };
        assert!(campaign.try_fetch_abduction(other_world, &mut run).is_none());
        assert!(!campaign.active_alien_mission.as_ref().unwrap().is_complete(&campaign));
        let msg = campaign.try_fetch_abduction(world, &mut run).expect("the citizen is abducted");
        assert!(msg.contains("abducted a tribe citizen"), "{msg}");
        let mission = campaign.active_alien_mission.clone().unwrap();
        assert!(mission.is_complete(&campaign));
        assert_eq!(mission.current_progress(&campaign), 1);
        assert!(campaign.try_fetch_abduction(world, &mut run).is_none(), "once");
        // Hailing the giver turns it in.
        let giver_world = space_galaxy::world_id(space_galaxy::system_of_world(world), 0);
        let before = run.spice;
        campaign.uplifted_empires.insert(giver_world, 3);
        let action = campaign.try_communicate(giver_world, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert!(run.spice >= before + reward);
        assert_eq!(campaign.badge_stats.fetch_missions, 1);
    }

    #[test]
    fn an_ally_raid_repelled_counts_for_the_friends_defended_badge() {
        // 0xC5BB60: the raid mission's completion on an ally's planet adds one to
        // `FriendsDefended` and MissionComplete (the mission's 3) to the ally.
        let mut campaign = SpaceCampaign::default();
        let before = campaign.badge_counter(rules::BadgeCounter::FriendsDefended);
        campaign.ally_raids_defended += 1;
        campaign.pirate_raids_defended += 1;
        assert_eq!(campaign.badge_counter(rules::BadgeCounter::FriendsDefended), before + 2);
    }

    #[test]
    fn peace_treaty_and_ceasefire_negotiations_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let sys = 5;
        let emp_world = space_galaxy::world_id(sys, 0);
        let emp_idx = 3; // Avorni (Scientist personality: 0 at contact)
        campaign.current_system = sys;
        campaign.current_system_empire = Some(emp_idx);
        campaign.apply_relationship(emp_idx, RelationshipEvent::StartedWar, 1.0);
        campaign.apply_relationship(emp_idx, RelationshipEvent::BeNasty, 1.0);
        assert!((campaign.relationship(emp_idx) - -9.0).abs() < 1e-4);
        assert_eq!(campaign.relation_label(emp_idx), "At War");

        // Peace costs 6,250 per point (-9 -> 56,250): the 60,000 offer.
        run.spice = 50_000.0;
        let action = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action, WorldAction::Refused(msg) if msg.contains("Peace costs 60000")));
        run.spice = 100_000.0;
        let action = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Peace with")));
        assert_eq!(run.spice, 40_000.0);
        assert!(!campaign.wars.contains(&emp_idx));
        assert_ne!(campaign.relation_label(emp_idx), "At War");
        // The peace holds while the grudges decay.
        let mut save = false;
        campaign.tick(rules::RELATIONSHIP_DECAY_PERIOD * 3.0, false, &mut run.spice, &mut save);
        assert!(!campaign.wars.contains(&emp_idx));
    }

    #[test]
    fn alliance_treaty_and_dance_with_the_devil_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let sys = 7;
        let emp_world = space_galaxy::world_id(sys, 0);
        let emp_idx = 2; // Kroxid (Warrior personality: -3 at contact)
        campaign.archetype = SpaceArchetype::Trader; // warriors dislike warriors (-1)
        campaign.current_system = sys;
        campaign.current_system_empire = Some(emp_idx);
        campaign.apply_relationship(emp_idx, RelationshipEvent::BeNice, 5.0);
        assert!((campaign.relationship(emp_idx) - 7.0).abs() < 1e-4);

        // At 7 the empire offers an alliance: free.
        run.spice = 150.0;
        let action = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Formed an alliance")));
        assert_eq!(run.spice, 150.0);
        assert!(campaign.allies.contains(&emp_idx));
        assert_eq!(campaign.badge_stats.alliances_formed, 1);

        // Win the Grox over (their rows cap most goodwill low) and ally with them.
        let grox_sys = 30;
        let grox_world = space_galaxy::world_id(grox_sys, 0);
        campaign.current_system = grox_sys;
        campaign.current_system_empire = Some(0);
        campaign.apply_relationship(0, RelationshipEvent::MissionComplete, 20.0);
        campaign.apply_relationship(0, RelationshipEvent::CommittedAtrocity, 25.0);
        campaign.apply_relationship(0, RelationshipEvent::GoodToolUse, 30.0);
        campaign.apply_relationship(0, RelationshipEvent::BeNice, 1.0);
        assert!(campaign.relationship(0) >= rules::CREATE_ALLIANCE_THRESHOLD);
        assert!(!campaign.wars.contains(&0));
        // A second empire met on the way; a third never met (0xD064F0: the player's pairs
        // count only for an empire met).
        campaign.meet_empire(1);
        let grox_action = campaign.try_communicate(grox_world, &mut run);
        assert!(matches!(grox_action, WorldAction::Done(msg) if msg.contains("Dance with the Devil")));
        assert!(campaign.danced_with_the_devil);
        assert!(campaign.allies.contains(&0));
        // AlliedWithGrob: -20 with every empire met, alliances end, war follows.
        assert!(!campaign.allies.contains(&emp_idx));
        assert_eq!(campaign.relation_label(emp_idx), "At War");
        assert!(campaign.relationship(1) <= rules::WAR_THRESHOLD);
        assert!(campaign.relationships.get(&4).is_none(), "an empire never met is not told");
    }

    #[test]
    fn procedural_alien_mission_contracts_and_rewards_test() {
        // Headless: no galaxy places the target of a scored kind (another test may
        // have registered the process galaxy).
        let _headless = space_galaxy::pin_registered_galaxy(None);
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let sys = 10;
        let emp_world = space_galaxy::world_id(sys, 0);
        let emp_idx = 4; // Thelos (+3 at contact)
        campaign.current_system = sys;
        campaign.current_system_empire = Some(emp_idx);
        run.spice = 500.0;

        let action1 = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action1, WorldAction::Done(msg) if msg.contains("trade route") && msg.contains("Mission accepted")));
        let mission = campaign.active_alien_mission.clone().expect("a mission");
        let retail = mission.retail.expect("a retail row");
        assert_eq!(
            mission.reward_spice,
            rules::mission_reward(retail, 0, 0, rules::Difficulty::Normal),
            "retail money x size tier x rank x difficulty"
        );
        assert_eq!(mission.time_left, rules::mission_duration(retail, 0, 0, rules::Difficulty::Normal));
        assert!(campaign.hud().contains("Active Contract"));

        // Fulfil it the way its kind needs.
        match mission.kind {
            AlienMissionKind::PirateHunt => {
                for _ in 0..mission.target_count {
                    campaign.register_kill(&mut run);
                }
            }
            AlienMissionKind::SpecimenCollection => campaign.cargo_wildlife.herbivores = 1,
            AlienMissionKind::TerraformWorld => {
                let idx = campaign.ensure_ecology(mission.target_world.unwrap());
                campaign.ecology[idx].temperature = 0.5;
                campaign.ecology[idx].atmosphere = 0.5;
            }
            AlienMissionKind::Fetch => {
                let world = mission.target_world.expect("a fetch target");
                assert!(campaign.try_fetch_abduction(world, &mut run).is_some());
            }
            // These need a target that only a generated galaxy places.
            AlienMissionKind::Eradicate | AlienMissionKind::Balance | AlienMissionKind::MultiDelivery | AlienMissionKind::ScanFetch => {
                unreachable!("{:?} needs a galaxy", mission.kind)
            }
        }
        assert!(campaign.active_alien_mission.as_ref().unwrap().is_complete(&campaign));
        let (pre_spice, pre_rel) = (run.spice, campaign.relationship(emp_idx));
        let action2 = campaign.try_communicate(emp_world, &mut run);
        assert!(matches!(action2, WorldAction::Done(msg) if msg.contains("Mission completed")));
        assert_eq!(run.spice, pre_spice + mission.reward_spice);
        let gain = campaign.relationship(emp_idx) - pre_rel;
        assert!((gain - rules::mission_row(retail).relation).abs() < 1e-4, "MissionComplete x relation");
        assert_eq!(campaign.alien_missions_completed, 1);
        assert_eq!(campaign.badge_stats.missions_complete, 1);
        assert!(campaign.active_alien_mission.is_none());

        // A timed mission fails when its limit runs out (-1.5 with the giver).
        // (At 7 the empire first offers its alliance.)
        for _ in 0..3 {
            if campaign.active_alien_mission.is_some() {
                break;
            }
            campaign.try_communicate(emp_world, &mut run);
        }
        let next = campaign.active_alien_mission.clone().expect("next mission");
        if let Some(limit) = next.time_left {
            let before = campaign.relationship(emp_idx);
            let mut save = false;
            campaign.tick(limit + 1.0, false, &mut run.spice, &mut save);
            assert!(campaign.active_alien_mission.is_none());
            let _ = before; // the value may sit at the +10 cap
            assert!(campaign.relationships[&emp_idx]
                .events
                .contains_key(&RelationshipEvent::MissionFailed));
        }
    }

    #[test]
    fn galaxy_hover_and_hud_allied_display_test() {
        let galaxy = Galaxy::generate(None);
        let mut campaign = SpaceCampaign::default();
        let mut view = crate::space_view::SpaceView::default();
        view.cursor = 2;
        view.galaxy = true;

        // If star has an empire, test caption
        if let Some(emp) = galaxy.star(view.cursor).and_then(|s| s.empire) {
            let cap1 = crate::space_galaxy::galaxy_caption(&galaxy, &campaign, &view);
            assert!(!cap1.contains("[ALLIED EMPIRE]"));

            // Ally with this empire
            campaign.allies.push(emp);
            let cap2 = crate::space_galaxy::galaxy_caption(&galaxy, &campaign, &view);
            assert!(cap2.contains("[ALLIED EMPIRE]"));

            // Set active TerraformWorld mission targeting world in this system
            let target_w = space_galaxy::world_id(view.cursor, 1);
            campaign.active_alien_mission = Some(AlienMission {
                empire: emp,
                kind: AlienMissionKind::TerraformWorld,
                reward_spice: 3500.0,
                progress: 0,
                target_count: 1,
                target_world: Some(target_w),
                description: "Test mission".into(),
                retail: None,
                time_left: None,
                ..Default::default()
            });
            let cap3 = crate::space_galaxy::galaxy_caption(&galaxy, &campaign, &view);
            assert!(cap3.contains("[MISSION TARGET: Terraform World]"));
        }
    }

    #[test]
    fn earth_discovery_and_achievements_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 6_000_000.0;

        let earth_id = space_galaxy::earth_world_id(space_galaxy::SOL_STAR);
        let eco_idx = campaign.ensure_ecology(earth_id);
        let earth_eco = campaign.ecology[eco_idx];
        assert_eq!(earth_eco.t_score(), 3, "Earth must be a full T3 paradise");
        assert_eq!(earth_eco.flora, 9);
        assert_eq!(earth_eco.herbivores, 6);
        assert_eq!(earth_eco.carnivores, 3);

        // Sightseer counts special stars (`badgeSightseer`), not Earth.
        assert_eq!(campaign.badge_tier(SpaceBadge::Sightseer), 0);

        // Survey Earth
        let pre_spice = run.spice;
        let action = campaign.try_survey(earth_id, 3.3, &mut run);
        assert!(matches!(action, WorldAction::Done(msg) if msg.contains("Our Own Home") && msg.contains("Earth")));
        assert!(campaign.discovered_earth);
        assert_eq!(run.spice, pre_spice, "scanning pays nothing in retail");
        campaign.endgame.sightseer_stars = vec![1, 2];
        assert_eq!(campaign.badge_tier(SpaceBadge::Sightseer), 1);

        // Firing Planet Buster on Earth awards "Oh the Humanity!"
        campaign.planetary_tool = PlanetaryTool::PlanetBuster;
        let buster_action = campaign.try_apply_planetary_tool(earth_id, &mut run);
        assert!(matches!(buster_action, WorldAction::Done(msg) if msg.contains("Oh the Humanity!") && msg.contains("obliterated Earth")));
        assert!(campaign.destroyed_earth);
        assert_eq!(campaign.planets_busted, 1);
    }

    #[test]
    fn colony_defense_turret_combat_test() {
        let turret = ColonyTurret::default();
        assert_eq!(turret.fire_cooldown, 0.8);
        assert_eq!(turret.range, 16.0);

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.insert_resource(RunProgress::default());

        // Spawn colonized planet with ColonyTurret
        let planet_entity = app
            .world_mut()
            .spawn((
                Planet {
                    radius: 3.0,
                    colonized: true,
                    name: "Colony Prime".into(),
                    surveyed: true,
                },
                crate::persistence::WorldId(100),
                Transform::from_xyz(0.0, 0.0, 0.0),
                ColonyTurret {
                    fire_cooldown: 0.0,
                    range: 16.0,
                },
            ))
            .id();

        // Spawn Pirate raider within atmospheric range (dist = 8.0, max = 3.0 + 16.0 = 19.0)
        let pirate_entity = app
            .world_mut()
            .spawn((
                Pirate {
                    hp: 50.0,
                    max: 50.0,
                    speed: 10.0,
                    rewarded: false,
                    is_grox: false,
                },
                Transform::from_xyz(0.0, 8.0, 0.0),
            ))
            .id();

        app.add_systems(Update, space_colony_turrets);
        app.update();

        // Verify a Projectile was spawned with 10.0 damage aimed toward the pirate
        let mut proj_query = app.world_mut().query::<&Projectile>();
        let projs: Vec<&Projectile> = proj_query.iter(app.world()).collect();
        assert_eq!(projs.len(), 1, "turret must fire 1 projectile at invading raider");
        // turretlaser1 25 per second x spaceTurretNPCDamageMultiplier 0.5.
        assert_eq!(projs[0].damage, 12.5);

        // Verify turret cooldown is now > 0.0
        let turret_ref = app.world().get::<ColonyTurret>(planet_entity).unwrap();
        assert!(turret_ref.fire_cooldown > 0.0);

        // Clean up pirate
        app.world_mut().entity_mut(pirate_entity).despawn();
    }

    #[test]
    fn colony_city_layout_building_placement_and_synergies_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 100_000.0;
        let world = 100;
        campaign.colonies.push(world);
        let idx = campaign.ensure_ecology(world);
        // A T3 world: a city holds 20 buildings (a T0 world one, see
        // `a_city_holds_the_buildings_its_terraform_level_allows`).
        campaign.ecology[idx] = PlanetEcology { colony_count: 1, ..PlanetEcology::completed(world) };

        // Verify initial empty city layout
        let layouts = campaign.ensure_city_layouts(world);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].productivity(), 0.0);
        assert_eq!(layouts[0].happiness(), 0);
        assert_eq!(layouts[0].turrets(), 0);

        // Build House in slot 0 (ADAPTED: a production building's 20,000)
        let a1 = campaign.build_city_structure(world, 0, 0, CityBuilding::House, &mut run);
        assert!(matches!(a1, WorldAction::Done(_)));
        assert_eq!(run.spice, 80_000.0);
        assert_eq!(campaign.city_layouts[&world][0].houses(), 1);

        // Build Factory in slot 1 (adjacent to slot 0 House) (production building 20,000)
        let a2 = campaign.build_city_structure(world, 0, 1, CityBuilding::Factory, &mut run);
        assert!(matches!(a2, WorldAction::Done(_)));
        assert_eq!(run.spice, 60_000.0);
        // Base factory (1.0) + adjacent house (1.0) = 2.0 productivity
        assert_eq!(campaign.city_layouts[&world][0].productivity(), 2.0);
        // Factory next to House inflicts -1 happiness
        assert_eq!(campaign.city_layouts[&world][0].happiness(), -1);

        // Build Entertainment in slot 5 (adjacent to slot 0 House)
        let a3 = campaign.build_city_structure(world, 0, 5, CityBuilding::Entertainment, &mut run);
        assert!(matches!(a3, WorldAction::Done(_)));
        assert_eq!(run.spice, 40_000.0);
        // Entertainment has +1 base + 2 (adjacent house) = +3, minus 1 from factory = +2 net happiness!
        assert_eq!(campaign.city_layouts[&world][0].happiness(), 2);

        // Build Turret in slot 2 (defensive building 10,000)
        let a4 = campaign.build_city_structure(world, 0, 2, CityBuilding::Turret, &mut run);
        assert!(matches!(a4, WorldAction::Done(_)));
        assert_eq!(run.spice, 30_000.0);
        assert_eq!(campaign.total_defense_turrets(world), 1);

        // Check income calculation incorporates productivity and happiness
        // Productivity raises the colony's spice production base.
        assert!(campaign.colony_base_production(world) > rules::BASE_PRODUCTION_PER_FACTORY);

        // Demolish Turret in slot 2 (refunds 50% of 75 = 38)
        let d1 = campaign.demolish_city_structure(world, 0, 2, &mut run);
        assert!(matches!(d1, WorldAction::Done(_)));
        assert_eq!(run.spice, 30_000.0 + 5_000.0);
        assert_eq!(campaign.total_defense_turrets(world), 0);
    }

    #[test]
    fn galactic_rare_artifact_collections_and_set_completion_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 100.0;

        assert_eq!(campaign.badge_tier(SpaceBadge::Collector), 0);

        // Simulate collecting all 10 pieces of Scrolls of Faith
        let set = ArtifactSet::ScrollsOfFaith;
        for piece in 1..=10 {
            let item = ArtifactItem {
                name: format!("{} {}", set.title(), ArtifactSet::roman(piece)),
                world_id: 2000 + piece as u32,
                value: 2_000,
                set: Some(set),
                piece: Some(piece),
                rare: None,
            };
            campaign.artifacts.push(item);
            let pieces = campaign.vault_collections.entry(set).or_default();
            pieces.insert(piece);
            if pieces.len() == 10 {
                campaign.completed_sets.push(set);
                run.spice = (run.spice + 50_000.0).min(SPICE_CAP);
            }
        }

        assert_eq!(campaign.completed_sets.len(), 1);
        assert_eq!(run.spice, 100.0 + 50_000.0, "the wallet has no 9,999 cap any more");
        // Retail collector badge: rares collected 3 / 8 / 20 / 50 / 100 (the
        // ten pieces of an older save's invented set count).
        assert_eq!(campaign.badge_tier(SpaceBadge::Collector), 2);
        // Ten distinct retail rares found (a duplicate adds nothing).
        for r in crate::space_rares::members(0) {
            campaign.rares.obtain(r);
        }
        campaign.rares.obtain(crate::space_rares::rare("rare-jewel1").unwrap());
        assert_eq!(campaign.badge_tier(SpaceBadge::Collector), 3);
    }

    #[test]
    fn specialized_planetary_tools_test() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 2_000_000.0;
        let world = 88;
        campaign.surveyed.push(world);
        let idx = campaign.ensure_ecology(world);
        campaign.ecology[idx].temperature = 0.50;
        campaign.ecology[idx].atmosphere = 0.50;

        // 1. Atmosphere Freeze = coldcloudvacuum: bought once, then each use adds
        // its velocities (-0.0075 / -0.0075) to the marble.
        campaign.planetary_tool = PlanetaryTool::AtmosphereFreeze;
        let a1 = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(a1, WorldAction::Done(_)));
        assert_eq!(run.spice, 2_000_000.0 - ATMOSPHERE_FREEZE_COST);
        assert_eq!(campaign.ecology[idx].temperature, 0.50, "the hit moves the marble, not the climate");
        assert_eq!(campaign.marbles[&world].v_temperature, -0.0075);
        assert_eq!(campaign.marbles[&world].v_atmosphere, -0.0075);
        campaign.try_apply_planetary_tool(world, &mut run);
        assert_eq!(run.spice, 2_000_000.0 - ATMOSPHERE_FREEZE_COST, "an owned ray costs nothing more");

        // 2. Heat Ray = hotcloudray (+0.0075 / +0.0075).
        campaign.planetary_tool = PlanetaryTool::HeatRay;
        let a2 = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(a2, WorldAction::Done(_)));
        assert_eq!(run.spice, 2_000_000.0 - ATMOSPHERE_FREEZE_COST - HEAT_RAY_COST);
        assert!((campaign.marbles[&world].v_temperature - (-0.0075 * 2.0 + 0.0075)).abs() < 1e-6);
        campaign.marbles.clear();

        // 3. Creature Tweaker on T3 world
        campaign.planetary_tool = PlanetaryTool::CreatureTweaker;
        campaign.ecology[idx].flora = 1;
        campaign.ecology[idx].herbivores = 0;
        campaign.ecology[idx].carnivores = 0;
        let a3 = campaign.try_apply_planetary_tool(world, &mut run);
        assert!(matches!(a3, WorldAction::Done(_)));
        assert_eq!(campaign.ecology[idx].flora, 9);
        assert_eq!(campaign.ecology[idx].herbivores, 6);
        assert_eq!(campaign.ecology[idx].carnivores, 3);
        assert_eq!(campaign.ecology[idx].stable_foodweb_tier(), 3);

        // 4. Asteroid Strike on foreign alien empire colony
        campaign.planetary_tool = PlanetaryTool::AsteroidCaller;
        let emp_sys = 1;
        let emp_world = space_galaxy::world_id(emp_sys, 0);
        campaign.current_system = emp_sys;
        campaign.current_system_empire = Some(1);
        let cur_hp = campaign.empire_defense_hp(emp_world);
        let a4 = campaign.try_apply_planetary_tool(emp_world, &mut run);
        assert!(matches!(a4, WorldAction::Done(msg) if msg.contains("Orbital Asteroid Strike")));
        assert_eq!(campaign.empire_defense_hp(emp_world), (cur_hp - 1000.0).max(0.0));
    }

    #[test]
    fn a_held_ray_repeats_every_auto_fire_and_spends_energy_per_use() {
        // Hold handler 0x1050500 / Use 0x1050080: a ray (heatray: AutoFireRate
        // 0.1 s, EnergyCost 0.5) is used on the press and every 0.1 s after.
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        campaign.surveyed.push(77);
        let idx = campaign.ensure_ecology(77);
        campaign.ecology[idx].temperature = 0.50;
        campaign.ecology[idx].atmosphere = 0.50;
        campaign.planetary_tool = PlanetaryTool::Warm;
        let spec = rules::tool_use("heatray");
        let clock = &mut campaign.tool_clocks[PlanetaryTool::Warm.index()];
        let mut uses = 0;
        let mut energy = 100.0f32;
        let mut temperature = 0.50f32;
        for frame in 0..60 {
            // One second at 60 frames, the button down from frame 0.
            let fire = clock.step(spec.autofire, true, frame == 0, 1.0 / 60.0);
            if fire != rules::ToolFire::None {
                let cost = rules::tool_use_energy(spec.energy, spec.autofire, 1.0 / 60.0);
                assert!(rules::tool_can_afford_energy(energy, cost));
                energy -= cost;
                clock.commit(spec.recharge);
                uses += 1;
                temperature += rules::ray_velocity("heatray").unwrap().1;
            }
        }
        assert!((10..=11).contains(&uses), "ten uses in a held second: {uses}");
        assert!((energy - (100.0 - 0.5 * uses as f32)).abs() < 1e-4, "0.5 energy per use: {energy}");
        // `try_ray_repeat` hands the retail velocity (+0.01 temperature) to the marble.
        assert!(matches!(campaign.try_ray_repeat(77, &mut run), WorldAction::Done(_)));
        assert_eq!(campaign.marbles[&77].v_temperature, 0.01);
        assert_eq!(campaign.ecology[idx].temperature, 0.50, "the climate moves as the marble rolls");
        assert!(temperature > 0.0);
        // A use costs no Sporebucks, and the press is the same use.
        assert_eq!(run.spice, 1_000_000.0);
        assert!(matches!(campaign.try_apply_planetary_tool(77, &mut run), WorldAction::Done(_)));
        assert!((campaign.marbles[&77].v_temperature - 0.02).abs() < 1e-6);
        assert_eq!(run.spice, 1_000_000.0);
        // The homeworld is closed to the rays (`ToolTemplate` DisableOnHomeWorld).
        assert!(matches!(campaign.try_ray_repeat(0, &mut run), WorldAction::Refused(_)));
    }

    #[test]
    fn the_marble_rolls_the_climate_and_the_t_score_follows() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let idx = campaign.ensure_ecology(77);
        campaign.ecology[idx].temperature = 0.50;
        campaign.ecology[idx].atmosphere = 0.50;
        campaign.ecology[idx].flora = 0;
        // A volcano (+0.75 / +0.75) pushes the marble out of the bullseye: the
        // T-score falls from T3 as it rolls, and the terraformed list follows.
        campaign.mark_terraformed(77, &mut run);
        campaign.hit_marble(77, 0.75, 0.75);
        assert_eq!(campaign.ecology_for(77).t_score(), 3, "the hit moves nothing by itself");
        campaign.coast_marble(77, 90.0, &mut run);
        let after = campaign.ecology_for(77);
        assert!(after.temperature > 0.55 && after.atmosphere > 0.55, "{after:?}");
        assert!(after.t_score() < 3, "{after:?}");
        // A world nobody looked at has no marble and keeps its climate.
        assert!(!campaign.step_marble(99, 1.0, &mut run));
    }

    #[test]
    fn a_throwing_terraformer_takes_ammo_and_pushes_the_marble() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        run.spice = 5_000_000.0;
        campaign.surveyed.push(88);
        campaign.ensure_ecology(88);
        // `asteroid` hit velocities -0.75 atmosphere / +0.75 temperature.
        campaign.planetary_tool = PlanetaryTool::Volcano;
        let price = rules::store_price("volcano").unwrap_or(12_650.0);
        assert!(matches!(campaign.try_apply_planetary_tool(88, &mut run), WorldAction::Done(_)));
        assert_eq!(run.spice, 5_000_000.0 - price, "the store shortcut buys the ammo");
        let body = campaign.marbles[&88];
        assert_eq!((body.v_atmosphere, body.v_temperature), (0.75, 0.75));
        campaign.planetary_tool = PlanetaryTool::IceComet;
        assert!(matches!(campaign.try_apply_planetary_tool(88, &mut run), WorldAction::Done(_)));
        let body = campaign.marbles[&88];
        assert_eq!((body.v_atmosphere, body.v_temperature), (1.5, 0.0), "an ice comet: +0.75 / -0.75");
        // Never on the homeworld.
        let spice = run.spice;
        assert!(matches!(campaign.try_apply_planetary_tool(0, &mut run), WorldAction::Refused(_)));
        assert_eq!(run.spice, spice);
    }

    #[test]
    fn a_projectile_blast_hurts_every_ship_inside_the_damage_area() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)));
        app.add_systems(Update, space_projectiles);
        let pirate = |app: &mut App, at: Vec3| {
            app.world_mut()
                .spawn((Transform::from_translation(at), Pirate { hp: 100.0, max: 100.0, speed: 1.0, rewarded: false, is_grox: false }))
                .id()
        };
        let near = pirate(&mut app, Vec3::new(0.0, 0.0, 0.0));
        let beside = pirate(&mut app, Vec3::new(1.0, 0.0, 0.0));
        let far = pirate(&mut app, Vec3::new(5.0, 0.0, 0.0));
        app.world_mut().spawn((Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)), Projectile { vel: Vec3::ZERO, damage: 40.0, life: 1.0, area: 2.0 }));
        app.update();
        app.update();
        let hp = |app: &App, e: Entity| app.world().get::<Pirate>(e).unwrap().hp;
        assert_eq!(hp(&app, near), 60.0);
        assert_eq!(hp(&app, beside), 60.0, "inside the 2.0 blast");
        assert_eq!(hp(&app, far), 100.0, "outside it");
        // Without a blast area only the ship that was hit takes damage.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)));
        app.add_systems(Update, space_projectiles);
        let near = pirate(&mut app, Vec3::ZERO);
        let beside = pirate(&mut app, Vec3::new(1.0, 0.0, 0.0));
        let third = pirate(&mut app, Vec3::new(3.0, 0.0, 0.0));
        app.world_mut().spawn((Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)), Projectile { vel: Vec3::ZERO, damage: 40.0, life: 1.0, area: 0.0 }));
        app.update();
        app.update();
        assert_eq!(hp(&app, near), 60.0);
        assert_eq!(hp(&app, beside), 60.0, "within the 1.6 hit radius of the shot");
        assert_eq!(hp(&app, third), 100.0);
    }

    #[test]
    fn the_player_weapons_carry_their_damage_area() {
        // pulse 5, proton missile 8, dumb bomb 12 retail units (`DamageArea`).
        assert_eq!(rules::player_weapon("pulselevel1").unwrap().area, 5.0);
        assert_eq!(rules::player_weapon("protonmissilelevel1").unwrap().area, 8.0);
        assert_eq!(rules::player_weapon("dumbbomblevel3").unwrap().area, 12.0);
        for weapon in rules::PLAYER_WEAPONS {
            let record = crate::space_tools::record(weapon.id).unwrap();
            assert_eq!(record.damage_area, weapon.area, "{}", weapon.id);
        }
    }

    #[test]
    fn the_cargo_and_throwing_tools_use_once_per_press_and_recharge() {
        // monolith: AutoFireRate -1, RechargeRate 5: a held button uses it once;
        // pressing again inside the recharge does nothing.
        let spec = rules::tool_use("monolith");
        let mut clock = rules::ToolFireClock::default();
        assert_eq!(clock.step(spec.autofire, true, true, 0.016), rules::ToolFire::First);
        clock.commit(spec.recharge);
        for _ in 0..120 {
            assert_eq!(clock.step(spec.autofire, true, false, 0.016), rules::ToolFire::None, "no repeat while held");
        }
        assert_eq!(clock.step(spec.autofire, false, false, 0.016), rules::ToolFire::None);
        assert_eq!(clock.step(spec.autofire, true, true, 0.016), rules::ToolFire::None, "still recharging (5 s)");
        for _ in 0..400 {
            clock.step(spec.autofire, false, false, 0.016);
        }
        assert_eq!(clock.step(spec.autofire, true, true, 0.016), rules::ToolFire::First, "recharged");
    }

    #[test]
    fn the_selected_weapon_is_the_palette_choice_when_owned_else_the_best_laser() {
        let mut c = SpaceCampaign::default();
        c.grant_tool("laserlevel1");
        assert_eq!(c.selected_weapon().id, "laserlevel1");
        c.weapon_tool = Some("pulselevel1".into());
        assert_eq!(c.selected_weapon().id, "laserlevel1", "a pulse weapon that is not owned is not fired");
        c.grant_tool("pulselevel1");
        assert_eq!(c.selected_weapon().id, "pulselevel1");
        c.grant_tool("laserlevel3");
        assert_eq!(c.selected_weapon().id, "pulselevel1", "the choice stands over a better laser");
        c.weapon_tool = None;
        assert_eq!(c.selected_weapon().id, "laserlevel3");
        c.weapon_tool = Some("no such tool".into());
        assert_eq!(c.selected_weapon().id, "laserlevel3");
        // Beams are the lasers; the rest deal damage per shot at the retail rows.
        assert!(rules::is_beam_weapon("laserlevel2"));
        assert!(!rules::is_beam_weapon("protonmissilelevel1"));
        let proton = rules::player_weapon("protonmissilelevel1").unwrap();
        assert_eq!((proton.damage, proton.energy, proton.autofire, proton.min_range), (30.0, 0.5, 1.0, 10.0));
        assert_eq!(rules::player_weapon("dumbbomblevel3").unwrap().damage, 1800.0);
        assert_eq!(rules::projectile_speed("pulselevel1"), 150.0);
    }

    #[test]
    fn the_laser_drains_energy_per_use_ten_times_a_second() {
        // laserlevel1: EnergyCost 0.25 per use, AutoFireRate 0.1 s -> 2.5 / s.
        let laser = rules::laser(1);
        let per_second = laser.energy / laser.autofire;
        assert!((per_second - 2.5).abs() < 1e-6);
        assert!((rules::laser(3).energy / rules::laser(3).autofire - 7.5).abs() < 1e-6);
        // A reactor with less than the cost refuses the use.
        assert!(!rules::tool_can_afford_energy(0.2, laser.energy));
        assert!(rules::tool_can_afford_energy(0.25, laser.energy));
        // The damage rate is still 40 points a second: 4 per 0.1 s tick.
        assert!((rules::beam_damage_per_tick(laser.damage, laser.autofire) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn alien_philosophy_dialogue_trees_and_archetypes_test() {
        assert_eq!(empire_archetype(0), SpaceArchetype::Wanderer);
        assert_eq!(empire_archetype(1), SpaceArchetype::Zealot);
        assert_eq!(empire_archetype(2), SpaceArchetype::Warrior);
        assert_eq!(empire_archetype(3), SpaceArchetype::Scientist);
        assert_eq!(empire_archetype(4), SpaceArchetype::Diplomat);
        assert_eq!(empire_archetype(5), SpaceArchetype::Trader);

        let zealot_quote = archetype_philosophical_greeting(SpaceArchetype::Zealot, "Zelem");
        assert!(zealot_quote.contains("Spode"));

        let warrior_quote = archetype_philosophical_greeting(SpaceArchetype::Warrior, "Kroxid");
        assert!(warrior_quote.contains("conquerors") || warrior_quote.contains("fleet"));

        let grox_quote = archetype_philosophical_greeting(SpaceArchetype::Wanderer, "Grox");
        assert!(grox_quote.contains("ORGANIC") || grox_quote.contains("PURGING"));
    }

    #[test]
    fn a_hit_applies_the_tools_own_relationship_delta_with_the_sign_of_the_event() {
        // `OnHit` 0x105987C: a bomb (-0.5) sours an empire more than a laser (-0.02),
        // a positive delta (the happy ray) is a GoodToolUse.
        let owner = 3;
        let mut laser = SpaceCampaign::default();
        laser.apply_hit_deltas(owner, -0.02, 0.0);
        let mut bomb = SpaceCampaign::default();
        bomb.apply_hit_deltas(owner, -0.5, 0.0);
        let mut ray = SpaceCampaign::default();
        ray.apply_hit_deltas(owner, 0.5, 0.0);
        assert!(bomb.relationship(owner) < laser.relationship(owner));
        assert!(ray.relationship(owner) > laser.relationship(owner));
        // The galactic delta (gravitation wave -4, radius 10) reaches the empires near the
        // active star (0x1059A1A); a run without a galaxy has no stars, so the empires met
        // stand in for them (ADAPTED), and one never met hears nothing.
        let _headless = space_galaxy::pin_registered_galaxy(None);
        let mut wave = SpaceCampaign::default();
        wave.meet_empire(1);
        let before = wave.relationship(1);
        wave.apply_hit_deltas(owner, -20.0, -4.0);
        assert!(wave.relationship(1) < before, "the empire met takes note");
        assert_eq!(
            wave.relationships[&1].events.get(&RelationshipEvent::CommittedAtrocity),
            Some(&4.0),
            "a CommittedAtrocity of the delta's size, not a BadToolUse"
        );
        assert!(wave.relationships.get(&2).is_none(), "an empire never met is not told");
    }

    #[test]
    fn a_consumable_weapon_is_selected_while_charges_last() {
        let mut campaign = SpaceCampaign::default();
        campaign.grant_tool("laserlevel1");
        campaign.weapon_tool = Some("antimatterbomb".into());
        assert_eq!(campaign.selected_weapon().id, "laserlevel1", "none carried: the laser");
        campaign.add_charges("antimatterbomb", 2);
        assert_eq!(campaign.selected_weapon().id, "antimatterbomb");
        assert!(campaign.take_charge("antimatterbomb") && campaign.take_charge("antimatterbomb"));
        assert_eq!(campaign.selected_weapon().id, "laserlevel1", "used up: back to the laser");
    }

    #[test]
    fn escorts_do_not_heal_by_docking() {
        let mut c = SpaceCampaign::default();
        c.fleet.push(EscortShip { name: "Escort #1".into(), empire: Some(1), hp: 100.0, max_hp: 450.0 });
        let mut spice = 0.0;
        c.tick(30.0, true, &mut spice, &mut false);
        c.dock(0);
        assert_eq!(c.fleet[0].hp, 100.0, "no passive healing and no free repair at a dock");
    }

    /// A star of the galaxy from which `found` holds (the campaign stands there).
    fn star_where(galaxy: &Galaxy, campaign: &mut SpaceCampaign, found: impl Fn(&SpaceCampaign) -> bool) -> u32 {
        let star = (1..galaxy.stars.len() as u32)
            .find(|s| {
                campaign.current_system = *s;
                found(campaign)
            })
            .expect("a star with such a target in range");
        campaign.current_system = star;
        star
    }

    /// A fixture for a mission of `kind` taken from empire 3 at the star the
    /// campaign stands at: the world the giver is hailed at.
    fn giver_world_for(campaign: &mut SpaceCampaign, world: u32) -> u32 {
        let giver_world = space_galaxy::world_id(space_galaxy::system_of_world(world), 0);
        campaign.uplifted_empires.insert(giver_world, 3);
        giver_world
    }

    #[test]
    fn the_remaining_mission_kinds_take_their_targets_by_the_retail_scoring() {
        use crate::space_worldgen::TechLevel as T;
        use rules::MissionKind as K;
        // 0xC523E0 (commodity: a planet with a commodity node, Initialize 0xC544A0 wants
        // tech 5), 0xC4F210 (animals present, no sanctuary), the sample scoring of
        // `scanfetch_fetch` (bullseye T-score 1); the stand-ins are ADAPTED.
        assert_eq!(rules::kind_target_score(K::FetchCommodity, T::Empire, 0), Some(1.0));
        assert_eq!(rules::kind_target_score(K::FetchCommodity, T::Civilization, 0), None);
        assert_eq!(rules::kind_target_score(K::MultiDelivery, T::Empire, 0), Some(1.0));
        assert_eq!(rules::kind_target_score(K::MultiDelivery, T::Tribe, 0), None);
        assert_eq!(rules::kind_target_score(K::EradicateAnimals, T::Creature, 0), Some(1.0));
        assert_eq!(rules::kind_target_score(K::EradicateAnimals, T::None, 0), None);
        assert_eq!(rules::kind_target_score(K::ScanFetch, T::Empire, 1), Some(1.2));
        assert_eq!(rules::kind_target_score(K::ScanFetch, T::Tribe, 1), Some(1.0));
        assert_eq!(rules::kind_target_score(K::ScanFetch, T::Tribe, 2), None);
        assert_eq!(rules::kind_target_score(K::ScanFetch, T::None, 1), None);
        assert_eq!(rules::kind_target_score(K::FetchTribeCitizen, T::Tribe, 0), Some(1.0), "the fetch kinds keep their scoring");
        // The rows and the mission data: rewards, limits and counts.
        assert_eq!(rules::mission_row(K::Balance).money, 65_000.0);
        assert_eq!(rules::mission_row(K::MultiDelivery).money, 40_000.0);
        assert_eq!(rules::mission_row(K::EradicateAnimals).duration, 90.0);
        assert!(rules::mission_row(K::EradicateAnimals).timer_on_first_entry);
        assert_eq!(rules::mission_row(K::FetchCommodity).duration, 300.0);
        assert_eq!((rules::ERADICATE_NUM_CREATURES, rules::MULTI_DELIVERY_ITEMS, rules::BALANCE_ROWS_REQUIRED), (5, 3, 1));

        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        star_where(&galaxy, &mut c, |c| c.fetch_target(K::FetchCommodity, 3).is_some());
        let w = c.fetch_target(K::FetchCommodity, 3).unwrap();
        assert_eq!(galaxy.world_info(w).unwrap().tech, T::Empire);
        star_where(&galaxy, &mut c, |c| c.fetch_target(K::EradicateAnimals, 3).is_some());
        let w = c.fetch_target(K::EradicateAnimals, 3).unwrap();
        assert_ne!(galaxy.world_info(w).unwrap().tech, T::None);
        star_where(&galaxy, &mut c, |c| c.fetch_target(K::ScanFetch, 3).is_some());
        let info = galaxy.world_info(c.fetch_target(K::ScanFetch, 3).unwrap()).unwrap();
        assert_eq!(info.t_score, 1);
        // Balance (0xC4B830): a creature-level planet of a star without an empire whose
        // T-score level is below the required row.
        star_where(&galaxy, &mut c, |c| c.balance_target().is_some());
        let w = c.balance_target().unwrap();
        let star = space_galaxy::system_of_world(w);
        assert!(galaxy.star(star).unwrap().empire.is_none());
        assert_eq!(galaxy.world_info(w).unwrap().tech, T::Creature);
        assert!(c.ecology_for(w).stable_foodweb_tier() < rules::BALANCE_ROWS_REQUIRED as u8);
        // Multi-delivery (0xC59CF0): three items on three planets and one buyer of tech 5.
        star_where(&galaxy, &mut c, |c| c.multi_delivery_plan(3).is_some());
        let (buyer, items) = c.multi_delivery_plan(3).unwrap();
        assert_eq!(galaxy.world_info(buyer).unwrap().tech, T::Empire);
        assert_eq!(items.len(), 3);
        assert!(!items.contains(&buyer));
        assert!(items.iter().enumerate().all(|(i, w)| !items[..i].contains(w)), "three different planets");
    }

    #[test]
    fn a_fetch_commodity_mission_needs_the_spice_in_the_hold_and_takes_one_unit() {
        use rules::MissionKind as K;
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        star_where(&galaxy, &mut c, |c| c.fetch_target(K::FetchCommodity, 3).is_some());
        let world = c.fetch_target(K::FetchCommodity, 3).unwrap();
        let color = SpiceColor::for_world(world);
        let mut run = RunProgress::default();
        let reward = 25_000.0;
        c.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::Fetch,
            reward_spice: reward,
            target_count: 1,
            target_world: Some(world),
            retail: Some(K::FetchCommodity),
            time_left: Some(300.0),
            extra: MissionExtra { commodity: Some(color), item: "red spice".into(), ..Default::default() },
            ..Default::default()
        });
        assert!(c.try_fetch_abduction(world + 1, &mut run).is_none(), "only over the target world");
        let msg = c.try_fetch_abduction(world, &mut run).expect("the commodity is taken");
        assert!(msg.contains("one unit of"), "{msg}");
        assert_eq!(c.cargo_spice.get(&color), Some(&1));
        assert!(c.active_alien_mission.as_ref().unwrap().is_complete(&c));
        // 0xC52B90: the item must still be in the hold; selling it undoes the objective.
        c.cargo_spice.clear();
        assert!(!c.active_alien_mission.as_ref().unwrap().is_complete(&c));
        c.cargo_spice.insert(color, 2);
        assert!(c.active_alien_mission.as_ref().unwrap().is_complete(&c));
        // 0xC52740: the turn-in takes one unit and counts a fetch mission.
        let giver = giver_world_for(&mut c, world);
        let before = run.spice;
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert_eq!(c.cargo_spice.get(&color), Some(&1), "one unit of the two");
        assert!(run.spice >= before + reward);
        assert_eq!(c.badge_stats.fetch_missions, 1);
    }

    #[test]
    fn a_balance_mission_is_done_when_the_planet_reaches_the_required_row() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        star_where(&galaxy, &mut c, |c| c.balance_target().is_some());
        let world = c.balance_target().unwrap();
        let mut run = RunProgress::default();
        c.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::Balance,
            reward_spice: 65_000.0,
            target_count: rules::BALANCE_ROWS_REQUIRED,
            target_world: Some(world),
            retail: Some(rules::MissionKind::Balance),
            ..Default::default()
        });
        let mission = c.active_alien_mission.clone().unwrap();
        assert!(!mission.is_complete(&c));
        assert_eq!(mission.current_progress(&c), 0);
        // 0xC4B8D0: the planet's T-score level reaches the row.
        let idx = c.ensure_ecology(world);
        c.ecology[idx] = PlanetEcology::completed(world);
        assert!(mission.is_complete(&c));
        let giver = giver_world_for(&mut c, world);
        let before = run.spice;
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert!(run.spice >= before + 65_000.0);
        assert_eq!(c.badge_stats.missions_complete, 1);
        assert_eq!(c.badge_stats.fetch_missions, 0, "not a fetch class mission");
    }

    #[test]
    fn a_multi_delivery_mission_collects_three_items_and_gives_them_to_the_buyer() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        star_where(&galaxy, &mut c, |c| c.multi_delivery_plan(3).is_some());
        let (buyer, items) = c.multi_delivery_plan(3).unwrap();
        let mut run = RunProgress::default();
        c.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::MultiDelivery,
            reward_spice: 40_000.0,
            target_count: 3,
            target_world: Some(buyer),
            retail: Some(rules::MissionKind::MultiDelivery),
            extra: MissionExtra { item: "crown".into(), buyer_world: Some(buyer), item_worlds: items.clone(), ..Default::default() },
            ..Default::default()
        });
        assert!(c.try_multi_delivery_pickup(buyer, &mut run).is_none(), "the buyer holds no item");
        assert!(c.try_multi_delivery(buyer, &mut run).is_none(), "nothing carried yet");
        for (n, w) in items.iter().enumerate() {
            let msg = c.try_multi_delivery_pickup(*w, &mut run).expect("an item lies here");
            assert!(msg.contains("crown"), "{msg}");
            assert_eq!(c.active_alien_mission.as_ref().unwrap().extra.carried, n as u32 + 1);
            assert!(c.try_multi_delivery_pickup(*w, &mut run).is_none(), "one item per planet");
        }
        assert!(!c.active_alien_mission.as_ref().unwrap().is_complete(&c), "carried, not delivered");
        // Two items delivered first: the buyer asks for the rest.
        c.active_alien_mission.as_mut().unwrap().extra.carried = 2;
        let msg = c.try_multi_delivery(buyer, &mut run).expect("the buyer takes the items");
        assert!(msg.contains("2 of 3") && msg.contains("1 more"), "{msg}");
        c.active_alien_mission.as_mut().unwrap().extra.carried = 1;
        let msg = c.try_multi_delivery(buyer, &mut run).unwrap();
        assert!(msg.contains("Return to the"), "{msg}");
        let mission = c.active_alien_mission.clone().unwrap();
        assert!(mission.is_complete(&c));
        assert_eq!(mission.current_progress(&c), 3);
        // The giver pays; no fetch badge (the class has no counter of its own).
        let giver = giver_world_for(&mut c, buyer);
        let before = run.spice;
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert!(run.spice >= before + 40_000.0);
        assert_eq!(c.badge_stats.fetch_missions, 0);
    }

    #[test]
    fn a_scan_fetch_mission_needs_the_scan_and_a_specimen() {
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        star_where(&galaxy, &mut c, |c| c.fetch_target(rules::MissionKind::ScanFetch, 3).is_some());
        let world = c.fetch_target(rules::MissionKind::ScanFetch, 3).unwrap();
        let mut run = RunProgress::default();
        c.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::ScanFetch,
            reward_spice: 40_000.0,
            target_count: 2,
            target_world: Some(world),
            retail: Some(rules::MissionKind::ScanFetch),
            ..Default::default()
        });
        let m = c.active_alien_mission.clone().unwrap();
        assert_eq!(m.current_progress(&c), 0);
        c.cargo_wildlife.herbivores = 1;
        assert!(!m.is_complete(&c), "step 1 is the scan: a specimen does not count yet");
        c.surveyed.push(world);
        assert!(m.is_complete(&c), "the scan sub-mission is done");
        assert_eq!(m.current_progress(&c), 1);
        // Step 1 ends at the giver without a payment (`scanfetch_scan` pays 0) and the
        // fetch sub-mission follows (conversation flags Step1 -> Step2).
        let giver = giver_world_for(&mut c, world);
        let before = run.spice;
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("now bring them")), "{action:?}");
        assert_eq!(run.spice, before, "the scan step pays nothing");
        let m = c.active_alien_mission.clone().expect("the mission goes on");
        assert_eq!((m.extra.stage, crate::space_comm::mission_step(&m)), (1, 2));
        assert_eq!(c.badge_stats.fetch_missions, 0);
        // The fetch sub-mission is a fetch class mission: the specimen in the hold ends it,
        // it takes one specimen, pays the row and counts.
        assert!(m.is_complete(&c));
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert!(run.spice >= before + 40_000.0);
        assert_eq!(c.cargo_wildlife.total(), 0, "one specimen taken");
        assert_eq!(c.badge_stats.fetch_missions, 1);
    }

    #[test]
    fn an_eradicate_mission_counts_the_creatures_destroyed_at_its_planet() {
        let mut c = SpaceCampaign::default();
        let mut run = RunProgress::default();
        let world = space_galaxy::world_id(7, 2);
        c.current_system = 7;
        c.active_alien_mission = Some(AlienMission {
            empire: 3,
            kind: AlienMissionKind::Eradicate,
            reward_spice: 25_000.0,
            target_count: rules::ERADICATE_NUM_CREATURES,
            target_world: Some(world),
            retail: Some(rules::MissionKind::EradicateAnimals),
            extra: MissionExtra { duration: Some(90.0), ..Default::default() },
            ..Default::default()
        });
        // Not placed yet (the player has not entered): a kill is not a creature.
        c.register_kill(&mut run);
        assert_eq!(c.active_alien_mission.as_ref().unwrap().progress, 0);
        c.active_alien_mission.as_mut().unwrap().extra.spawned = true;
        // Elsewhere nothing counts (0xC50870: the active planet must be the mission's).
        c.current_system = 8;
        c.register_kill(&mut run);
        assert_eq!(c.active_alien_mission.as_ref().unwrap().progress, 0);
        c.current_system = 7;
        for n in 1..rules::ERADICATE_NUM_CREATURES {
            c.register_kill(&mut run);
            assert_eq!(c.active_alien_mission.as_ref().unwrap().progress, n);
            assert!(!c.active_alien_mission.as_ref().unwrap().is_complete(&c));
        }
        assert_eq!(c.badge_stats.eradicate_missions, 0);
        c.register_kill(&mut run);
        let m = c.active_alien_mission.clone().unwrap();
        assert!(m.is_complete(&c));
        assert_eq!(c.badge_stats.eradicate_missions, 1, "0xC4F2DF: the badge when the last creature falls");
        c.register_kill(&mut run);
        assert_eq!(c.active_alien_mission.as_ref().unwrap().progress, rules::ERADICATE_NUM_CREATURES, "no more than needed");
        assert_eq!(c.badge_stats.eradicate_missions, 1);
        // Its time limit starts on the first entry (the placement sets it); the turn-in at the
        // giver pays and adds nothing to the badge again.
        let giver = giver_world_for(&mut c, world);
        c.current_system = space_galaxy::system_of_world(giver);
        let before = run.spice;
        let action = c.try_communicate(giver, &mut run);
        assert!(matches!(action, WorldAction::Done(ref m) if m.contains("Mission completed")), "{action:?}");
        assert!(run.spice >= before + 25_000.0);
        assert_eq!(c.badge_stats.eradicate_missions, 1);
    }

    #[test]
    fn the_generator_offers_the_remaining_kinds_with_their_rows() {
        use rules::MissionKind as K;
        let galaxy = Galaxy::generate(None);
        let _pinned = space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let mut c = SpaceCampaign::default();
        let mut seen: Vec<AlienMissionKind> = Vec::new();
        let mut commodity = false;
        let stars: Vec<u32> = (1..galaxy.stars.len() as u32).step_by(211).take(4).collect();
        for star in stars {
            c.current_system = star;
            for emp in 1..9 {
                for done in 0..12 {
                    c.alien_missions_completed = done;
                    let m = c.generate_alien_mission(emp);
                    assert!(!m.description.is_empty() && m.target_count > 0);
                    let retail = m.retail.expect("a retail row");
                    assert_eq!(m.reward_spice, rules::mission_reward(retail, rules::empire_tier(c.empire_size(emp)), 0, c.difficulty()));
                    match m.kind {
                        AlienMissionKind::Eradicate => {
                            assert_eq!(m.target_count, 5);
                            assert!(m.time_left.is_none(), "the limit starts on the first entry");
                            assert_eq!(m.extra.duration, rules::mission_duration(K::EradicateAnimals, rules::empire_tier(c.empire_size(emp)), 0, c.difficulty()));
                            assert!(m.target_world.is_some());
                        }
                        AlienMissionKind::Balance => {
                            assert_eq!((m.target_count, m.time_left), (1, None));
                        }
                        AlienMissionKind::MultiDelivery => {
                            assert_eq!((m.target_count, m.time_left), (3, None));
                            assert_eq!(m.extra.item_worlds.len(), 3);
                            assert!(m.extra.buyer_world.is_some());
                        }
                        AlienMissionKind::ScanFetch => assert_eq!(m.target_count, 2),
                        AlienMissionKind::Fetch if retail == K::FetchCommodity => {
                            commodity = true;
                            assert!(m.extra.commodity.is_some() && m.extra.item.ends_with("spice"));
                        }
                        _ => {}
                    }
                    seen.push(m.kind);
                }
            }
        }
        for kind in [AlienMissionKind::Eradicate, AlienMissionKind::Balance, AlienMissionKind::MultiDelivery, AlienMissionKind::ScanFetch] {
            assert!(seen.contains(&kind), "{kind:?} is offered");
        }
        assert!(commodity, "fetch commodity is offered");
    }
}
