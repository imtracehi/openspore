//! Civilization stage: your nation against the planet's other cities, played
//! as Spore's real-time strategy layer.
//!
//! The campaign data here is the source of truth (cities, their building
//! rings and wall turrets, spice geysers and derricks, the rival nations and a
//! snapshot of every vehicle); `civ_rts` owns the live vehicles, orders,
//! combat and the rival AI, `city_presentation` / `spice_presentation` draw
//! the cities and geysers from this data.
//!
//! Controls:
//! - Camera (`civ_controls`): WASD / arrows pan, `,` `.` rotate, PageUp /
//!   PageDown pitch, `=` `-` / the wheel zoom, middle-drag (or left + right
//!   drag) orbits, right-drag pans. `Space` cycles through your vehicles (or
//!   groups made with Ctrl + digit), `Backspace` goes to the closest city,
//!   `Tab` switches the stance of the selection, `Delete` decommissions it,
//!   `Esc` deselects.
//! - Left-click selects a vehicle (Ctrl or Shift adds, double-click selects that
//!   type), left-drag box-selects, `Q` selects the current doctrine's fleet.
//! - Right-click orders the selection: ground = move, rival vehicle = attack,
//!   rival city = attack / preach / trade (by vehicle type), spice geyser =
//!   drive there and build a spice derrick.
//! - `F` sends the selection (or the whole fleet) against the rival city
//!   nearest the camera; `X` claims the nearest free spice geysers.
//! - `G` builds a land vehicle, `H` an air vehicle (from three cities on), at
//!   the city nearest the camera; vehicles take that city's type. `1`/`2`/`3`
//!   set the capital's doctrine: Military, Religious or Economic.
//! - `B` builds the most useful building on the nearest owned city's ring,
//!   `4`/`5`/`6` force a House / Factory / Entertainment, `T` adds a wall
//!   turret, `E` upgrades every vehicle's engine.
//! - Every rival city can be taken three ways: shot down (military), converted
//!   by preaching vehicles (religious) or opened to trade and bought outright
//!   (economic). Only a fully unified planet advances to Space.
//!
//! Assembly: `V` toggles the editor between the vehicle and the city skyline.
//! In both modes the grammar is the creature assembler's: `N` mounts an
//! original catalog part, `[`/`]` swaps the focused one, `,`/`.` changes the
//! slot, `Tab` focuses, `Backspace` removes, `R` rotates, `-`/`=` scales and
//! `IJKL`/`UO` slide. Both modes select original `VEHICLE_PARTS` /
//! `BUILDING_PARTS` definitions and import them on demand through
//! `mechanical_assets`, so a selection only commits once its authored
//! geometry decoded. Vehicle parts ride every one of your vehicles.
//!
//! The numbers come from the original through `civ_rules` (start money,
//! building / vehicle prices, the money update every 6 s from factory-house
//! links and derricks, city mood with parties and riots, vehicle caps and tech
//! level, buy prices, relationships, consequence ability costs and effects);
//! distances are retail values on the original's planet (`civ_planet`: cities
//! at the retail rings, vehicles in 3D on the sphere). What the original does
//! not give stays OpenSpore's and is marked ADAPTED.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::*;
use crate::stages::{GameStage, StageOwned};
use crate::civ_planet::{Shape, CITY_SCALE, RETAIL_FROM_ISLAND};
use crate::{asset_catalog, civ_rules, mechanical_assets, Originals};

/// Camera pan speed (world units per second).
pub const PAN_SPEED: f32 = 22.0;

/// City 0 is the player capital; 1..`RIVAL_COUNT` are the rivals. A rival
/// nation is identified by its city's index for the whole stage.
///
/// The planet has retail's ten city slots (VERIFIED, level setup 0xB9D970:
/// the arithmetica `num_cities` = 10, `civ_rules::LEVEL_NUM_CITIES`): the
/// player's capital and nine NPC cities that appear one by one
/// (`civ_reveal`), three on the capital's continent first
/// (`civ_rules::npc_slot_wants`, `civ_planet::lay_out_sites`).
pub const HOME_INDEX: usize = 0;
pub const CITY_COUNT: usize = civ_rules::LEVEL_NUM_CITIES;
pub const RIVAL_COUNT: usize = CITY_COUNT - HOME_INDEX - 1;
/// "By a city": the reach of the build keys and of the HUD's city readout
/// (ADAPTED, island 9 x `CITY_SCALE`).
pub const CITY_RANGE: f32 = 9.0 * CITY_SCALE;
/// Build keys act on the owned city nearest the camera within this reach
/// (ADAPTED, island 18 at retail scale).
pub const BUILD_REACH: f32 = 18.0 * RETAIL_FROM_ISLAND;
/// City roots hover this far above the ground (the city layout's floor is 2
/// local units under the root, drawn `CITY_SCALE` times larger).
pub const CITY_ROOT_LIFT: f32 = 2.0 * CITY_SCALE;

/// A city's "armour" (`CityState::hp`, the HUD's bars) is the damage still
/// needed to take it by the retail capture rule: the hall's 1000 (100
/// capture points at 0.1 per damage point, VERIFIED 0xBE9C70) plus each
/// standing building's health (`CivilianBuildingHealth` 800).
pub const CITY_HP: f32 = civ_rules::CAPTURE_FULL / civ_rules::CAPTURE_PER_DAMAGE;
pub const CITY_HP_PER_BUILDING: f32 = BUILDING_HP;
/// `CivilianBuildingHealth` 800 (VERIFIED 0xBD1970).
pub const BUILDING_HP: f32 = civ_rules::CIVILIAN_BUILDING_HEALTH;
/// Money paid for taking a city: none. VERIFIED (civ-audit-a): the 17 callers of
/// `cCivilization::AddMoney` 0xBEFED0 are the start wealth (0xB60D1A), the NPC
/// city creation (0xB99F67), the trade delivery (0xBE1A21 / 0xBE1C22), the
/// building placement (0xBE88AE), the money update (0xBF7361), the vehicle
/// (0xCAA7E5), the sell refund (0xCF39DF), the comm actions (0xCFF103 ..
/// 0xCFFADF), the planner (0xD07D08), the trade vehicles (0xDCA870 / 0xDCADB4)
/// and 0xFF69FE; none lies in `cCity::SetOwner` 0xBE9430, the capture rule
/// 0xBE9AD0 / 0xBE9C70 or the surrender takeover 0xBE9E00. The earlier 150
/// spice per city was OpenSpore's (0.82). The constant and the `reward_paid`
/// flag stay for saves and tests.
pub const CAPTURE_REWARD: f32 = 0.0;
/// The money update (VERIFIED 0xBF7310: every 6th one-second civ update).
pub const INCOME_PERIOD: f32 = civ_rules::MONEY_UPDATE_SECONDS;
/// The turret cap of the earlier four-tower layout; saves written with it keep
/// loading (`CityState::max_turrets` is the cap now: the walls' authored mounts).
pub const LEGACY_DEFENSE_MAX: u8 = 4;
/// A turret's health: `MilitaryBuildingHealth` 600 of the global tuning prop
/// 0xA4C2F6BD (VERIFIED: cTurret 0xBD16F0 outside Space; default 1000).
pub const TURRET_HP: f32 = civ_rules::MILITARY_BUILDING_HEALTH;
/// The turret health of saves written before the retail capture rule.
const OLD_TURRET_HP: f32 = 60.0;
/// `DefenseCost` 2000.
pub const TURRET_COST: f32 = civ_rules::TURRET_COST;
/// Opening a trade route costs nothing: retail opens it when an economic vehicle
/// reaches the city and the nation accepts (0xDCA090 / 0xCEEF90), and no
/// `AddMoney` call (0xBEFED0) lies on that path.
pub const TRADE_COST: f32 = 0.0;
/// Legacy purchase marker written by `capture_city`.
pub const BUY_COST: f32 = 100.0;
/// `InitialWealth` 1000.
pub const STARTING_SPICE: f32 = civ_rules::INITIAL_WEALTH;
/// ADAPTED: the rivals start with the same `InitialWealth`.
pub const RIVAL_STARTING_SPICE: f32 = civ_rules::INITIAL_WEALTH;
/// Vehicle prices (VERIFIED 0xBF22B0): land 1000, air 3000, amphibious 1500.
pub const LAND_VEHICLE_COST: f32 = 1000.0;
pub const AIR_VEHICLE_COST: f32 = 3000.0;
pub const SEA_VEHICLE_COST: f32 = 1500.0;
/// A vehicle builds the derrick on a free geyser at no charge (command 0xCF3EDB:
/// "claim an unclaimed node, whatever its purpose"); the 500 of `HarvesterCost`
/// is the price of the purpose-3 harvester vehicle, not of the derrick.
pub const DERRICK_COST: f32 = 0.0;
pub const DERRICK_HP: f32 = 60.0;
/// Money a fresh derrick pays per update (0.1 x 1000, VERIFIED 0xCEF130).
pub const DERRICK_INCOME: f32 = civ_rules::HARVEST_MAX_LOAD * civ_rules::INCOME_FACTOR;
/// Cities at which aircraft unlock: the first count above three (VERIFIED
/// 0xBF44B0 -> tech level 3, `civ_rules::AIR_CITY_THRESHOLD`).
pub const AIR_UNLOCK_CITIES: usize = civ_rules::AIR_CITY_THRESHOLD + 1;
/// Sea vehicles open with tech level 2 (one city, VERIFIED 0xBF44B0) in a
/// coastal city (VERIFIED 0xBDB9D0, [`CivCampaign::is_coastal`]).
pub const SEA_UNLOCK_CITIES: usize = 1;
/// The eight-plot ring of the earlier layout; saves written with it are
/// re-laid on the authored slots by `CityState::ensure_layout`.
pub const LEGACY_PLOT_COUNT: usize = 8;
/// ADAPTED: spice geyser counts are not in the generation list; two per city
/// slot (the earlier five-city planet had ten).
pub const GEYSER_COUNT: usize = 2 * CITY_COUNT;
/// Seconds after a hit that the cinematics' "city attacked" edge watches
/// (`cinematics::moments`; ADAPTED, 0.82). The mood rolls and the rival
/// construction read the retail 20 s window (`CityState::under_attack`).
pub const REGEN_DELAY: f32 = 6.0;
/// Spice paid when a vehicle is lost is not refunded; kept for old saves.
pub const LOSS_PENALTY: f32 = 0.25;

/// Consequence abilities carried over from the Cell stage (names, texts,
/// tools, costs: `civ_rules` / locale 0x9A5ACF10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CivCellPower {
    /// Cell carnivore (`cvg_clg_meat`, tool `i_field`): your vehicles take no
    /// damage for 15 s.
    Invulnerability,
    /// Cell herbivore (`cvg_clg_plant`, tool `faithheal`): repairs all your
    /// vehicles and buildings.
    HealingAura,
    /// Cell omnivore (`cvg_clg_mix`, tool `emp`): immobilizes vehicles,
    /// turrets and buildings in its radius for 15 s.
    StaticBomb,
}

impl CivCellPower {
    pub fn data(&self) -> &'static civ_rules::PowerData {
        match self {
            Self::Invulnerability => &civ_rules::INVULNERABILITY,
            Self::HealingAura => &civ_rules::HEALING_AURA,
            Self::StaticBomb => &civ_rules::STATIC_BOMB,
        }
    }

    pub fn name(&self) -> &'static str {
        self.data().name
    }

    pub fn description(&self) -> &'static str {
        self.data().description
    }

    /// The tool's `RechargeRate` (60 s for every Cell ability).
    pub fn cooldown(&self) -> f32 {
        self.data().recharge
    }

    pub fn cost(&self) -> f32 {
        self.data().cost
    }
}

/// Consequence abilities carried over from the Creature stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CivCreaturePower {
    /// Creature predator (`cvg_crg_attack`, tool `napalm`): 600 damage to the
    /// buildings and vehicles in its radius.
    MightyBomb,
    /// Creature social (`cvg_crg_social`, tool `diplodervish`): raises your
    /// relationship with the targeted nation.
    DiploDervish,
    /// Creature adaptable (`cvg_crg_mix`, tool `espionage`): enemy vehicles in
    /// its radius attack each other for 20 s.
    BribeBomb,
}

impl CivCreaturePower {
    pub fn data(&self) -> &'static civ_rules::PowerData {
        match self {
            Self::MightyBomb => &civ_rules::MIGHTY_BOMB,
            Self::DiploDervish => &civ_rules::DIPLO_DERVISH,
            Self::BribeBomb => &civ_rules::BRIBE_BOMB,
        }
    }

    pub fn name(&self) -> &'static str {
        self.data().name
    }

    pub fn description(&self) -> &'static str {
        self.data().description
    }

    /// `RechargeRate` 180 s.
    pub fn cooldown(&self) -> f32 {
        self.data().recharge
    }

    pub fn cost(&self) -> f32 {
        self.data().cost
    }
}

/// Consequence abilities carried over from the Tribal stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CivTribalPower {
    /// Tribal aggressive (`cvg_trg_attack`, tool `tacnuke`): 6000 damage
    /// around the targeted city; helps capture it.
    GadgetBomb,
    /// Tribal friendly (`cvg_trg_social`, tool `blackcloud`): shuts down the
    /// targeted city's entertainment and turrets for 120 s.
    BlackCloud,
    /// Tribal industrious (`cvg_trg_mix`, tool `adblitz`): faster buyout of
    /// the targeted city.
    AdBlitz,
}

impl CivTribalPower {
    pub fn data(&self) -> &'static civ_rules::PowerData {
        match self {
            Self::GadgetBomb => &civ_rules::GADGET_BOMB,
            Self::BlackCloud => &civ_rules::BLACK_CLOUD,
            Self::AdBlitz => &civ_rules::AD_BLITZ,
        }
    }

    pub fn name(&self) -> &'static str {
        self.data().name
    }

    pub fn description(&self) -> &'static str {
        self.data().description
    }

    /// `RechargeRate` 300 s.
    pub fn cooldown(&self) -> f32 {
        self.data().recharge
    }

    pub fn cost(&self) -> f32 {
        self.data().cost
    }
}

/// The Civilization superweapon (`cvg_trg_*win`), by the capital's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CivSuperweapon {
    /// Military (`icbm`): destroys (takes) every remaining city.
    Icbm,
    /// Religious (`rising`): converts every remaining city.
    FanaticalUprising,
    /// Economic (`globaladblitz`): buys every remaining city.
    GlobalMerger,
}

impl CivSuperweapon {
    pub fn data(&self) -> &'static civ_rules::PowerData {
        match self {
            Self::Icbm => &civ_rules::ICBM,
            Self::FanaticalUprising => &civ_rules::FANATICAL_UPRISING,
            Self::GlobalMerger => &civ_rules::GLOBAL_MERGER,
        }
    }

    pub fn name(&self) -> &'static str {
        self.data().name
    }

    pub fn description(&self) -> &'static str {
        self.data().description
    }

    /// One shot (`RechargeRate` 0 and `mbLauncedIWinWeapon`).
    pub fn cooldown(&self) -> f32 {
        self.data().recharge
    }

    /// 12,000 per city still to take (VERIFIED 0xBF6DF0).
    pub fn cost(&self, cities_not_owned: usize) -> f32 {
        civ_rules::superweapon_cost(cities_not_owned)
    }
}

/// Which ability slot a key fires (7/U, 8/I, 9/O, 0/P).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSlot {
    Cell,
    Creature,
    Tribal,
    Superweapon,
}

/// What an ability does to the live vehicles (`civ_rts` applies it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitEffect {
    /// Repair every one of your vehicles by this much.
    Heal(f32),
    /// Damage rival vehicles inside the radius.
    Damage { at: Vec3, radius: f32, amount: f32 },
    /// Rival vehicles inside the radius can neither move nor shoot.
    Stun { at: Vec3, radius: f32, seconds: f32 },
    /// Rival vehicles inside the radius attack each other.
    Bribe { at: Vec3, radius: f32, seconds: f32 },
}

/// The result of using an ability.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerUse {
    pub message: String,
    pub effect: Option<UnitEffect>,
    /// Where the effect lands (city centre or the camera focus).
    pub at: Vec3,
    /// The tool used (`i_field`, `faithheal`, `emp`, `napalm`, ..., or the
    /// superweapon's tool name); the Civ voices (`civ_voices`) key on it.
    pub tool: &'static str,
}

/// How an ability was aimed (helper `civ-target`): at the camera focus (the key fired it at
/// once: `OPENSPORE_CIV_ARMING=0`, the superweapons, the tests) or with a click on the planet
/// (retail 0xCF688E: the clicked ground point; `city` is the rival city that point selects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerAim {
    #[default]
    Camera,
    Click { city: Option<usize> },
}

/// Pure helper to resolve consequence powers from prior stage metrics.
pub fn resolve_consequence_powers(
    diet_carnivore: f32,
    diet_herbivore: f32,
    creature_allied: usize,
    creature_extinct: usize,
    tribal_conquered: usize,
    tribal_allied: usize,
) -> (CivCellPower, CivCreaturePower, CivTribalPower) {
    let cell = if diet_carnivore > diet_herbivore * 1.25 {
        CivCellPower::Invulnerability
    } else if diet_herbivore > diet_carnivore * 1.25 {
        CivCellPower::HealingAura
    } else {
        CivCellPower::StaticBomb
    };

    let creature = if creature_extinct > creature_allied {
        CivCreaturePower::MightyBomb
    } else if creature_allied > creature_extinct {
        CivCreaturePower::DiploDervish
    } else if diet_carnivore > diet_herbivore {
        CivCreaturePower::MightyBomb
    } else if diet_herbivore > diet_carnivore {
        CivCreaturePower::DiploDervish
    } else {
        CivCreaturePower::BribeBomb
    };

    let tribal = if tribal_conquered > tribal_allied {
        CivTribalPower::GadgetBomb
    } else if tribal_allied > tribal_conquered {
        CivTribalPower::BlackCloud
    } else {
        CivTribalPower::AdBlitz
    };

    (cell, creature, tribal)
}

pub fn resolve_superweapon(approach: Approach) -> CivSuperweapon {
    match approach {
        Approach::Military => CivSuperweapon::Icbm,
        Approach::Religious => CivSuperweapon::FanaticalUprising,
        Approach::Economic => CivSuperweapon::GlobalMerger,
    }
}


/// Hard ceiling on one assembly (the vehicle, or one city skyline).
pub const MAX_CIV_PARTS: usize = 48;
/// Largest axis of one mounted part in world units, before its own multiplier.
pub const VEHICLE_PART_CAP: f32 = 1.8;
pub const BUILDING_PART_CAP: f32 = 3.2;
/// The per-part multiplier the editor may reach; the authored scale stays on
/// top of it, so a small authored part can still be built up by the player.
pub const PART_SCALE_MIN: f32 = 0.25;
pub const PART_SCALE_MAX: f32 = 4.0;

// Fit the default hall within its central plot, leaving room for the building
// ring inside the 11.5-unit city perimeter. Custom skyline parts keep sizes.
const CITY_PRESET_MAX_EXTENT: f32 = 3.5;
/// Imported model units to stage units (authored sizes are far larger).
pub(crate) const VEHICLE_PART_BASE: f32 = 0.25;
pub(crate) const CITY_PART_BASE: f32 = 0.5;

/// Retained for the shared schedule; geysers no longer deplete.
#[derive(Resource)]
pub struct CivTimer(pub Timer);
impl Default for CivTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(6.0, TimerMode::Repeating))
    }
}

/// A city's specialisation, a vehicle's type and the capital's doctrine.
/// Keys `1`/`2`/`3` pick the capital's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Approach {
    #[default]
    Military,
    Religious,
    Economic,
}

impl Approach {
    pub fn title(&self) -> &'static str {
        match self {
            Approach::Military => "Military",
            Approach::Religious => "Religious",
            Approach::Economic => "Economic",
        }
    }

    /// How the doctrine wants to be used, shown when it is selected.
    pub fn hint(&self) -> String {
        match self {
            Approach::Military => {
                "Military: build vehicles (G), select them and right-click a rival city to shoot its turrets and walls down".into()
            }
            Approach::Religious => {
                "Religious: park preaching vehicles by a rival city until its people convert".into()
            }
            Approach::Economic => format!(
                "Economic: send a vehicle to a rival city to open trade ({TRADE_COST:.0} spice), then buy the city from its communication screen"
            ),
        }
    }
}

/// Who answers for a city. Ownership is persisted and drives the map tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CityOwner {
    #[default]
    Rival,
    Player,
}

/// Spore's three city buildings, raised on the ring around the city hall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Building {
    House,
    Factory,
    Entertainment,
}

impl Building {
    pub const ALL: [Building; 3] = [Building::House, Building::Factory, Building::Entertainment];

    pub fn title(self) -> &'static str {
        match self {
            Building::House => "House",
            Building::Factory => "Factory",
            Building::Entertainment => "Entertainment",
        }
    }

    /// `SleepCost` 1600, `IndustryCost` 1200, `EntertainmentCost` 800.
    pub fn cost(self) -> f32 {
        match self {
            Building::House => civ_rules::HOUSE_COST,
            Building::Factory => civ_rules::FACTORY_COST,
            Building::Entertainment => civ_rules::ENTERTAINMENT_COST,
        }
    }

    /// The building in the retail link model.
    pub fn model(self) -> civ_rules::CityBuilding {
        match self {
            Building::House => civ_rules::CityBuilding::House,
            Building::Factory => civ_rules::CityBuilding::Factory,
            Building::Entertainment => civ_rules::CityBuilding::Entertainment,
        }
    }
}

/// Every city opens with the same four buildings on the authored layout's
/// slots 1, 3, 4 and 5 (plots 0, 2, 3, 4; both wall sizes link them alike): a
/// house and an entertainment building beside the factory, which links to the
/// hall only, so the opening economy, mood and housing are those of the
/// earlier ring's starters (income 40, mood 60, two houses and the hall).
/// ADAPTED: retail's opening buildings of a city were not traced.
pub const STARTER_PLOTS: [(usize, Building); 4] = [
    (0, Building::House),
    (2, Building::Factory),
    (3, Building::House),
    (4, Building::Entertainment),
];

/// A spice geyser and its derrick, index-aligned with `geyser_position`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeyserState {
    /// Nation that built a derrick here (0 = you, else the rival city index).
    pub owner: Option<usize>,
    pub hp: f32,
    /// The geyser's `mResourcePoints` (100 when new, -0.3 per paid update).
    pub points: f32,
    /// A pump under construction (`cCommodityNode::mMineState` 1, `civ_pump`).
    pub claim: Option<GeyserClaim>,
}

/// The construction of a derrick (`cCommodityNode::StartConstruction` 0xBFE590,
/// the node update 0xBFF8E0 state 1; VERIFIED, `civ_pump`): the nation that is
/// building it, the seconds until it stands (`mMineStateTimeout` 5000 ms) and
/// whether the pump capsule has been fired (`+0x1E7`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeyserClaim {
    pub nation: usize,
    pub left: f32,
    pub launched: bool,
    /// Where the claiming vehicle stood when it started (the capsule's origin).
    pub from: [f32; 3],
}

impl Default for GeyserState {
    fn default() -> Self {
        Self {
            owner: None,
            hp: 0.0,
            points: civ_rules::DERRICK_START_POINTS,
            claim: None,
        }
    }
}

impl GeyserState {
    /// A derrick just built on this geyser.
    pub fn derrick(owner: usize, points: f32) -> Self {
        Self {
            owner: Some(owner),
            hp: DERRICK_HP,
            points,
            claim: None,
        }
    }

    /// No owner and no pump on its way: `mMineState` 0 (the claim command starts only here,
    /// 0xDCB2E5; a node in construction keeps the later vehicles waiting).
    pub fn free(&self) -> bool {
        self.owner.is_none() && self.claim.is_none()
    }

    /// `cCommodityNode::StartConstruction` (0xBFE590): the pump is on its way, finished after
    /// `civ_pump::BUILD_SECONDS`. Returns false when the node is not free.
    pub fn start_claim(&mut self, nation: usize, from: Vec3) -> bool {
        if !self.free() {
            return false;
        }
        self.claim = Some(GeyserClaim {
            nation,
            left: crate::civ_pump::BUILD_SECONDS,
            launched: false,
            from: from.to_array(),
        });
        true
    }

    /// The node update's state 1 timer (0xBFF9F6 .. 0xBFFA3B): when the timeout has passed the
    /// owner becomes the constructing political id (0xBFF410) and the node moves to state 2.
    /// Returns the nation that now holds the derrick.
    pub fn tick_claim(&mut self, dt: f32) -> Option<usize> {
        let claim = self.claim.as_mut()?;
        claim.left -= dt;
        if claim.left > 0.0 {
            return None;
        }
        let nation = claim.nation;
        *self = Self::derrick(nation, self.points);
        Some(nation)
    }

    /// Money the derrick pays per update (0.1 x 0xCEF130).
    pub fn income(&self) -> f32 {
        civ_rules::derrick_income(self.points) * civ_rules::INCOME_FACTOR
    }
}

/// A rival nation's purse and temper (index = its city; 0 is unused).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NationState {
    pub spice: f32,
    /// At war with the player: turrets and vehicles fire on sight.
    pub war: bool,
    /// Seconds until the next raid on the player (military nations).
    pub raid: f32,
    /// The relationship with the player (sum of event contributions).
    #[serde(default)]
    pub relation: civ_rules::Relationship,
    /// The domestic budget (`cCivilization::mDomesticBudget` +0x4AC): what
    /// the nation's cities build buildings and turrets with (VERIFIED 0xBF7370
    /// / 0xBE5890, `npc_build_second`); `spice` is the rest of the purse.
    #[serde(default)]
    pub domestic: f32,
    /// The city this nation's military (0) and cultural (1) vehicles are sent against
    /// (`cCivilization` +0x458 / +0x45C, chosen by 0xBFA7A0: `civ_comm::attack_decision`).
    #[serde(default)]
    pub attack_target: [Option<usize>; 2],
}

/// What a vehicle was last told to do. Persisted with the vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum UnitOrder {
    #[default]
    Idle,
    /// Drive to a ground point (XZ; saves from the island builds).
    Move([f32; 2]),
    /// Act on a city by vehicle type: attack, preach or trade; an owned city
    /// is simply garrisoned.
    City(usize),
    /// Attack (military) or follow another vehicle by its id.
    Unit(u32),
    /// Claim a spice geyser, or shoot a hostile derrick off it.
    Geyser(usize),
    /// Drive to a world point on the planet.
    Goto([f32; 3]),
    /// The retail attack order (type 1, 0xDCC5A0) on an epic predator by its id: close in and
    /// fire until it falls (`civ_rts`, goodie-hut helper).
    Epic(u32),
    /// The retail capture order (type 8, behaviour 0x03F148D1, update 0xDCAAB0) on a goodie hut
    /// by its index in `CivCampaign::hut_sites`.
    Hut(usize),
}

/// Spore's vehicle operational domains: Land, Air and Sea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VehicleDomain {
    #[default]
    Land,
    Air,
    Sea,
}

impl VehicleDomain {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Land => "land",
            Self::Air => "air",
            Self::Sea => "sea",
        }
    }
}

/// A persisted vehicle. `civ_rts` keeps the live entities and snapshots them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UnitState {
    pub id: u32,
    pub nation: usize,
    pub kind: Approach,
    pub air: bool,
    pub position: [f32; 2],
    pub yaw: f32,
    pub hp: f32,
    pub order: UnitOrder,
    #[serde(default)]
    pub domain: VehicleDomain,
    /// World position on the planet (`position` keeps x, z for old readers).
    #[serde(default)]
    pub world: Option<[f32; 3]>,
    /// The stats the vehicle was built with (its class design's, 0xCAAB20);
    /// none = the `cVehicle` default 0.33 (rivals, saves before them).
    #[serde(default)]
    pub stats: Option<crate::civ_combat::VehicleStats>,
    /// An economic vehicle's trade shuttle (leg, cargo, wait, home city), so
    /// a save resumes the round trip (`civ_rts`).
    #[serde(default)]
    pub shuttle: Option<ShuttleState>,
}

/// The persisted part of a trade shuttle (0xDCC900 / 0xBE19B0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ShuttleState {
    /// On the way back to your city.
    pub homeward: bool,
    /// Cargo on board (0xCEEC30).
    pub cargo: f32,
    /// Seconds waited at the current end (2 s each).
    pub wait: f32,
    /// The home end of the route.
    pub home: Option<usize>,
}

impl UnitState {
    /// Where the vehicle stands: the world point, else the island's x, z.
    pub fn at(&self) -> Vec3 {
        self.world
            .map(Vec3::from_array)
            .filter(|p| p.is_finite())
            .unwrap_or(Vec3::new(self.position[0], 0.0, self.position[1]))
    }

    pub fn domain(&self) -> VehicleDomain {
        if self.domain != VehicleDomain::Land {
            self.domain
        } else if self.air {
            VehicleDomain::Air
        } else {
            VehicleDomain::Land
        }
    }

    pub fn max_hp(&self) -> f32 {
        crate::civ_combat::combat(self.domain(), self.stats.unwrap_or_default()).max_health
    }
}

pub fn unit_max_hp(air: bool) -> f32 {
    unit_domain_max_hp(if air { VehicleDomain::Air } else { VehicleDomain::Land })
}

/// Health of a vehicle with the default stats (0x4E74D0: land / air 249,
/// water 332).
pub fn unit_domain_max_hp(domain: VehicleDomain) -> f32 {
    crate::civ_combat::combat(domain, Default::default()).max_health
}

/// Retail Civ has no engine upgrades: a vehicle's speed is `5 + 20 s` (`10 + 40 s`
/// in the air) from its creation's speed stat (VERIFIED 0x4E74D0, called by the
/// vehicle setup 0xCAAB20 through 0xCEED70), nothing else scales it. OpenSpore's
/// 0.82 `E` purchase (12 % per level) is gone (civ-audit-a); the argument and
/// `RunProgress::engine` stay so saves and callers keep compiling.
pub fn engine_factor(_engine: u8) -> f32 {
    1.0
}

/// Standard speed of a default vehicle (0x4E74D0: land / water 11.6, air
/// 23.2 world units per second) with the engine upgrades.
pub fn unit_speed(air: bool, engine: u8) -> f32 {
    unit_domain_speed(if air { VehicleDomain::Air } else { VehicleDomain::Land }, engine)
}

pub fn unit_domain_speed(domain: VehicleDomain, engine: u8) -> f32 {
    crate::civ_combat::combat(domain, Default::default()).speed * engine_factor(engine)
}


/// A city's persisted state. `index` matches the `CityMarker` on its entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CityState {
    pub index: usize,
    pub owner: CityOwner,
    /// World point of the city's centre on the ground (the planet layout of
    /// `civ_planet::lay_out`); the entity hovers `CITY_ROOT_LIFT` above it.
    pub position: [f32; 3],
    /// Military route: armour left. Refilled to full when the city is taken.
    pub hp: f32,
    /// Religious route: 0.0..1.0 conversion.
    pub conversion: f32,
    /// Economic route: trade rights bought (required before purchase).
    pub trade: bool,
    /// Economic route: spice already offered.
    pub spent: f32,
    /// Buildings the player has bought here (the starter ring is free).
    pub buildings: u8,
    /// Wall turrets standing or ruined on this city's corners.
    pub defense: u8,
    /// Guard against paying the capture reward twice.
    pub reward_paid: bool,
    /// Unused since turrets fire one by one; kept so old saves load.
    pub retaliate: f32,
    /// The city's type: which vehicles it builds and how its walls look.
    #[serde(default)]
    pub kind: Approach,
    /// ADAPTED: a captured city whose earlier eight-plot ring held more
    /// buildings than the small walls have slots keeps the large walls, so
    /// that an old save loses none of them (set once by `ensure_layout`).
    #[serde(default)]
    pub kept_large_walls: bool,
    /// The building slots of the city's authored layout (slots 1.. of
    /// `city_layout::WallLayout`, 5 behind small walls, 11 behind large).
    #[serde(default)]
    pub plots: Vec<Option<Building>>,
    /// Armour of each wall turret; destroyed turrets stay at zero.
    #[serde(default)]
    pub turret_hp: Vec<f32>,
    /// Seconds since the city last took damage.
    #[serde(default)]
    pub calm: f32,
    /// Seconds of party left (0: none). VERIFIED 0xBD8D20: 60 s, income x2.
    #[serde(default)]
    pub party: f32,
    /// Seconds of riot left (0: none). VERIFIED 0xBD8D20: 60 s, income / 2.
    #[serde(default)]
    pub riot: f32,
    /// Seconds since the last party / riot roll (10 s / 5 s cadence).
    #[serde(default)]
    pub party_roll: f32,
    #[serde(default)]
    pub riot_roll: f32,
    /// Trade value the player's route has built up (subtracted from the buy
    /// price, at most 4000).
    #[serde(default)]
    pub trade_value: f32,
    /// Seconds a Static Bomb still holds the city's turrets and buildings.
    #[serde(default)]
    pub stunned: f32,
    /// Seconds the city has been "almost" taken (2 s before it changes hands).
    #[serde(default)]
    pub almost: f32,
    /// A seaport's harbour: the sea point where its sea vehicles roll out
    /// (`civ_planet::lay_out`); none for inland cities.
    #[serde(default)]
    pub harbor: Option<[f32; 3]>,
    /// One of your cities being preached to by a rival's religious vehicles
    /// (0..1, `civ_rts`) and by which nation; counter-conversion (0xBE1340)
    /// wins it back citizen by citizen.
    #[serde(default)]
    pub rival_conversion: f32,
    #[serde(default)]
    pub converter: Option<usize>,
    /// Damage taken by the building on each plot (`plots` order); at
    /// `BUILDING_HP` it is a ruin (removed after 30 s of quiet).
    #[serde(default)]
    pub building_damage: Vec<f32>,
    /// Seconds since each ruined building / turret fell (0xBE3C20 timers).
    #[serde(default)]
    pub ruin_age: Vec<f32>,
    #[serde(default)]
    pub turret_ruin_age: Vec<f32>,
    /// Military capture points 0..100 (`cCity::mCapturePercent` +0x748) and
    /// the nation earning them (`mCaptureID` +0x74C).
    #[serde(default)]
    pub capture: f32,
    #[serde(default)]
    pub capturer: Option<usize>,
    /// Seconds the city has stood as a rival city (retail cCity timer +0x6E8,
    /// started by the constructor 0xBE6460): rival turrets wait for it.
    #[serde(default)]
    pub age: f32,
    /// Civic objects on the city's decoration spots (`civ_decor::ITEMS`
    /// indices; `cCity::mCivicObjects` +0x308).
    #[serde(default)]
    pub decorations: Vec<Option<u8>>,
    /// Seconds left of the city's cultural-attack memory (VERIFIED 0xC00600
    /// slot 1: stamped by 0xC00C40(1) from 0xBEB310, the once-a-second
    /// conversion of a religious group performing at the city, and read for
    /// 20 s like the military slot 0 that `calm` keeps).
    #[serde(default)]
    pub preached: f32,
    /// `cCity::mbFirstHarvester` (+0x760): the city's free first vehicle has
    /// been handed out (VERIFIED 0xBE5890). Saves that predate it count as
    /// done (their rivals got OpenSpore's earlier two-vehicle opening).
    #[serde(default = "first_vehicle_done")]
    pub first_vehicle: bool,
    /// The citizens' allegiance (VERIFIED, `cCity` map +0x5A0): how many citizens
    /// answer to each nation (index = nation). Empty until first needed (all of
    /// them belong to the owner). See `civ_rules` "the conversion model".
    #[serde(default)]
    pub buckets: Vec<u32>,
    /// The leading converting faith (`cCity` +0x754, 0xBEB1E0), -1 when none.
    #[serde(default)]
    pub leader: Option<usize>,
}

fn first_vehicle_done() -> bool {
    true
}

/// The rival cities' types a campaign starts with, before the planet is laid
/// out. `civ_planet::lay_out` replaces them on a fresh campaign with retail's
/// roll (VERIFIED 0xCF8D00, `civ_rules::roll_npc_specialties`: 25 % military,
/// 25 % religious, 50 % economic per NPC slot, re-rolled until a military city
/// stands on the player's continent). (The `RandomInt(3)` the NPC slot writer
/// 0xB9AA20 stores at `+0x34` is not the specialty, which lives at `+0x28`.)
/// This table (a cycle of the three) is what saves from before the roll keep,
/// and what a campaign that was never laid out shows.
pub const RIVAL_KINDS: [Approach; RIVAL_COUNT] = [
    Approach::Military,
    Approach::Religious,
    Approach::Economic,
    Approach::Military,
    Approach::Religious,
    Approach::Economic,
    Approach::Military,
    Approach::Religious,
    Approach::Economic,
];

impl CityState {
    /// Fresh city at its deterministic map position.
    pub fn new(index: usize) -> Self {
        let kind = if index == HOME_INDEX {
            Approach::Military
        } else {
            RIVAL_KINDS[(index - 1) % RIVAL_COUNT]
        };
        let defense = match (index == HOME_INDEX, kind) {
            (true, _) => 1,
            (false, Approach::Military) => 3,
            (false, _) => 2,
        };
        let mut city = Self {
            index,
            owner: if index == HOME_INDEX {
                CityOwner::Player
            } else {
                CityOwner::Rival
            },
            position: city_position(index).to_array(),
            kind,
            defense,
            // A new city has never been attacked (0xC00600 reads a zero
            // stamp as "not within 20 s").
            calm: civ_rules::ATTACK_MEMORY_SECONDS,
            first_vehicle: false,
            ..Default::default()
        };
        city.ensure_layout();
        city.hp = city.max_hp();
        city
    }

    /// The planet layout's specialty roll (`civ_planet::lay_out`) gives a fresh
    /// rival city its type: the type and the turrets that come with it
    /// (`CityState::new`). Nothing happens to an owned city or the capital.
    pub fn retype(&mut self, kind: Approach) {
        if self.owned() || self.index == HOME_INDEX {
            return;
        }
        self.kind = kind;
        self.defense = if kind == Approach::Military { 3 } else { 2 };
        self.ensure_layout();
    }

    pub fn owned(&self) -> bool {
        self.owner == CityOwner::Player
    }

    /// VERIFIED 0xC00600(0) || 0xC00600(1): hit by a military attack or
    /// preached to by a religious group in the last 20 s (the mood rolls
    /// 0xBD8D20 and the rival construction 0xBE5890 read both slots).
    pub fn under_attack(&self) -> bool {
        self.calm < civ_rules::ATTACK_MEMORY_SECONDS || self.preached > 0.0
    }

    /// A religious group performs at the city this second (0xBEB310 ->
    /// 0xC00C40(1)).
    pub fn mark_preached(&mut self) {
        self.preached = civ_rules::ATTACK_MEMORY_SECONDS;
    }

    // ---- the citizens' allegiance (VERIFIED 0xBEB1E0 / 0xBE2260 / 0xBE1340;
    // `civ_rules` "the conversion model") ----

    /// Make the buckets match the city. Retail starts every city with all its
    /// citizens in the owner's bucket (`AddCitizens` 0xBE3440 adds to it,
    /// `RemoveCitizen` 0xBD8020 takes from it). A campaign saved with the old
    /// conversion meter gets the faith's share of the citizens that meter stood
    /// for (ADAPTED: the meter was `converted / (owner + converted)` doubled).
    pub fn ensure_buckets(&mut self) {
        let owner = self.nation().min(CITY_COUNT - 1);
        let citizens = self.citizens();
        if self.buckets.len() != CITY_COUNT {
            self.buckets = vec![0; CITY_COUNT];
            self.buckets[owner] = citizens;
            let (meter, by) = if self.owned() {
                (self.rival_conversion, self.converter)
            } else {
                (self.conversion, Some(HOME_INDEX))
            };
            if let Some(by) = by.filter(|b| *b != owner && *b < CITY_COUNT) {
                if meter > 0.0 && meter < 1.0 {
                    let moved = ((meter * 0.5 * citizens as f32).round() as u32).clamp(1, citizens);
                    self.buckets[owner] -= moved;
                    self.buckets[by] = moved;
                    self.leader = Some(by);
                }
            }
        }
        let sum: u32 = self.buckets.iter().sum();
        if sum < citizens {
            self.buckets[owner] += citizens - sum;
        } else if sum > citizens {
            let mut extra = sum - citizens;
            let order = std::iter::once(owner).chain((0..CITY_COUNT).filter(|i| *i != owner));
            for id in order {
                let take = extra.min(self.buckets[id]);
                self.buckets[id] -= take;
                extra -= take;
            }
        }
    }

    /// 0xBE2260's conversion half: forget a leading faith that is gone or is the
    /// owner, compute the conversion percent (200 x leader / (leader + owner)),
    /// and publish it as the meters the rest of the stage reads (`conversion`
    /// of a rival city the player preaches to, `rival_conversion` / `converter`
    /// of one of the player's cities).
    pub fn refresh_allegiance(&mut self) {
        if self.buckets.len() != CITY_COUNT {
            return;
        }
        let owner = self.nation().min(CITY_COUNT - 1);
        if self.leader.is_some_and(|l| l == owner || l >= CITY_COUNT) {
            self.leader = None;
        }
        let pct = self.leader.map_or(0.0, |l| civ_rules::conversion_percent(self.buckets[l], self.buckets[owner]));
        if self.leader.is_some() && pct <= 0.0 {
            self.leader = None;
        }
        if self.owned() {
            self.rival_conversion = pct / 100.0;
            self.converter = self.leader;
        } else {
            self.conversion = if self.leader == Some(HOME_INDEX) { pct / 100.0 } else { 0.0 };
        }
    }

    /// 0xBEB1E0: `count` citizens change allegiance to nation `id`. A second
    /// faith first sends the leading faith's followers back to the owner (and
    /// takes the lead only when none are left); the leader then converts from
    /// the owner's bucket.
    pub fn convert_citizens(&mut self, id: usize, count: u32) {
        self.ensure_buckets();
        let owner = self.nation().min(CITY_COUNT - 1);
        if count == 0 || id == owner || id >= CITY_COUNT {
            return;
        }
        let mut count = count;
        if self.leader.is_none() {
            self.leader = Some(id);
        }
        if let Some(lead) = self.leader.filter(|l| *l != id) {
            let n = count.min(self.buckets[lead]);
            self.buckets[lead] -= n;
            self.buckets[owner] += n;
            count -= n;
            if count > 0 {
                self.leader = Some(id);
            }
        }
        if self.leader == Some(id) {
            let n = count.min(self.buckets[owner]);
            self.buckets[owner] -= n;
            self.buckets[id] += n;
        }
        self.refresh_allegiance();
    }

    /// 0xBE1340 (every `CounterConversionInterval` seconds): the owner wins one
    /// citizen back from each faith with the odds `(mood / 100)^3 + 0.2`
    /// (0xCEEEB0; `roll` gives 0..1 per faith). ADAPTED: a faith whose
    /// nation no longer exists (retail returns all its citizens) is not modelled.
    pub fn counter_conversion(&mut self, mood: i32, mut roll: impl FnMut() -> f32) {
        if self.buckets.len() != CITY_COUNT {
            return;
        }
        let owner = self.nation().min(CITY_COUNT - 1);
        let chance = crate::civ_comm::counter_conversion_chance(mood);
        for id in 0..CITY_COUNT {
            if id != owner && self.buckets[id] >= 1 && roll() < chance {
                self.buckets[id] -= 1;
                self.buckets[owner] += 1;
            }
        }
        self.refresh_allegiance();
    }

    /// A city that changed hands (0xBEAB80 / 0xBE9AD0 reset the three maps and
    /// give every citizen to the new owner, `SetOwner` 0xBE9430 clears the
    /// leaders).
    pub fn reset_allegiance(&mut self) {
        self.buckets = vec![0; CITY_COUNT];
        self.leader = None;
        let owner = self.nation().min(CITY_COUNT - 1);
        self.buckets[owner] = self.citizens();
    }

    /// The nation a city answers to: 0 for you, its own index for a rival.
    pub fn nation(&self) -> usize {
        if self.owned() {
            HOME_INDEX
        } else {
            self.index
        }
    }

    pub fn center(&self) -> Vec3 {
        Vec3::from_array(self.position)
    }

    /// RETAIL (`cCity::Init` 0xBE7D40 -> `cCityWalls::SetWallSize` 0xBECC10,
    /// the planet preload 0xBED5B0): the player's own city (slot 0) stands
    /// behind the large walls, every rival city behind the small ones, and
    /// keeps them when it changes hands (`field_519` is written once).
    pub fn walls_large(&self) -> bool {
        self.index == HOME_INDEX || self.kept_large_walls
    }

    /// The authored layout of this city's walls (`city_layout`): the slots,
    /// turret mounts, decoration spots and the link matrix of its type.
    pub fn layout(&self) -> &'static crate::city_layout::WallLayout {
        crate::city_layout::wall_layout(self.kind, self.walls_large())
    }

    /// City-local XZ of building plot `plot` (retail slot `plot + 1`).
    pub fn plot_site(&self, plot: usize) -> Vec2 {
        self.layout().plot_site(plot)
    }

    /// City-local XZ of wall turret `turret` (the authored `Turrets`).
    pub fn turret_site(&self, turret: usize) -> Vec2 {
        self.layout().turret_site(turret)
    }

    /// City-local XZ of decoration spot `spot` (the authored `Decorations`).
    pub fn decoration_site(&self, spot: usize) -> Vec2 {
        self.layout().decoration_site(spot)
    }

    /// VERIFIED 0xBD8BF0: the city's turret slots (its walls' `Turrets`).
    pub fn max_turrets(&self) -> usize {
        self.layout().max_turrets()
    }

    /// Idempotent: lay out the starter buildings for saves that predate the
    /// authored layout (the earlier eight-plot ring, or none), spending any
    /// legacy purchases on the free plots, and size the turret armour to the
    /// saved turret count (at most the walls' turret mounts). Never changes
    /// `buildings`/`defense`.
    pub fn ensure_layout(&mut self) {
        // An owned city of the earlier ring (or of a save before plots) that
        // holds more buildings than the small walls have slots keeps large ones.
        if !self.kept_large_walls && self.index != HOME_INDEX && self.owned() {
            let small = crate::city_layout::wall_layout(self.kind, false).plots();
            let held = match self.plots.len() {
                LEGACY_PLOT_COUNT => self.plots.iter().flatten().count(),
                0 => STARTER_PLOTS.len() + self.buildings as usize,
                _ => 0,
            };
            self.kept_large_walls = held > small;
        }
        let slots = self.layout().plots();
        let had_damage = !self.building_damage.is_empty();
        if self.plots.len() != slots {
            // A saved ring of any older size keeps its buildings by ring
            // position as far as the new layout has slots; the starters fill
            // the empty hall-linked ones.
            let old = std::mem::take(&mut self.plots);
            let old_damage = std::mem::take(&mut self.building_damage);
            let mut plots: Vec<Option<Building>> = vec![None; slots];
            let mut damage = vec![0.0; slots];
            if old.len() == LEGACY_PLOT_COUNT {
                // The ring's plot `i` stood at the angle 45 degrees x (i + 1):
                // the nearest authored slot by angle takes its building.
                let mut taken = vec![false; slots];
                for (i, building) in old.iter().enumerate() {
                    let Some(building) = building else { continue };
                    let angle = std::f32::consts::FRAC_PI_4 * (1 + i) as f32;
                    let want = Vec2::new(angle.cos(), angle.sin());
                    let best = (0..slots)
                        .filter(|j| !taken[*j])
                        .max_by(|a, b| {
                            let da = self.layout().plot_site(*a).normalize_or_zero().dot(want);
                            let db = self.layout().plot_site(*b).normalize_or_zero().dot(want);
                            da.total_cmp(&db)
                        });
                    if let Some(j) = best {
                        taken[j] = true;
                        plots[j] = Some(*building);
                        damage[j] = old_damage.get(i).copied().unwrap_or(0.0);
                    }
                }
            } else {
                for (plot, building) in STARTER_PLOTS {
                    if plot < slots {
                        plots[plot] = Some(building);
                    }
                }
            }
            let mut legacy = self.buildings;
            for (i, plot) in plots.iter_mut().enumerate() {
                if legacy == 0 {
                    break;
                }
                if plot.is_none() {
                    *plot = Some(if i % 4 == 1 {
                        Building::House
                    } else {
                        Building::Factory
                    });
                    legacy -= 1;
                }
            }
            self.plots = plots;
            self.building_damage = damage;
            self.ruin_age = vec![0.0; slots];
        }
        let turrets = (self.defense as usize).min(self.max_turrets());
        if self.turret_hp.len() != turrets {
            self.turret_hp.resize(turrets, TURRET_HP);
        }
        // Saves from before the retail capture rule: every built plot stands
        // undamaged and the 0.82 turret armour (60) scales to 600.
        if !had_damage {
            for hp in &mut self.turret_hp {
                if *hp <= OLD_TURRET_HP {
                    *hp *= TURRET_HP / OLD_TURRET_HP;
                }
            }
        }
        self.building_damage.resize(self.plots.len(), 0.0);
        self.ruin_age.resize(self.plots.len(), 0.0);
        self.turret_ruin_age.resize(self.turret_hp.len(), 0.0);
        // The authored decoration spots (20 behind small walls, 30 behind
        // large): a saved list keeps its items by spot index.
        let spots = self.layout().decoration_spots();
        self.decorations.resize(spots, None);
        self.refresh_armour();
    }

    /// The building on `plot` stands (built and not in ruins).
    pub fn standing(&self, plot: usize) -> bool {
        self.plots.get(plot).is_some_and(|p| p.is_some())
            && self.building_damage.get(plot).is_none_or(|d| *d < BUILDING_HP)
    }

    /// Health left of the building on `plot` (0 when empty or ruined).
    pub fn building_health(&self, plot: usize) -> f32 {
        if self.standing(plot) {
            BUILDING_HP - self.building_damage.get(plot).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    }

    /// A destroyed building still lying on its plot.
    pub fn ruined(&self, plot: usize) -> bool {
        self.plots.get(plot).is_some_and(|p| p.is_some()) && !self.standing(plot)
    }

    pub fn standing_buildings(&self) -> usize {
        (0..self.plots.len()).filter(|p| self.standing(*p)).count()
    }

    /// VERIFIED 0xBD9E70: the city hall counts damage only once every other
    /// building of the city is destroyed.
    pub fn hall_exposed(&self) -> bool {
        self.standing_buildings() == 0
    }

    /// Damage still needed to take the city: the standing buildings' health
    /// plus the hall's share of the capture points not yet earned.
    pub fn armour(&self) -> f32 {
        let buildings: f32 = (0..self.plots.len())
            .map(|p| self.building_health(p))
            .sum();
        buildings + (civ_rules::CAPTURE_FULL - self.capture).max(0.0) / civ_rules::CAPTURE_PER_DAMAGE
    }

    fn refresh_armour(&mut self) {
        self.hp = self.armour();
    }

    /// The nation holding a full capture (the city changes hands).
    pub fn captured_by(&self) -> Option<usize> {
        self.capturer.filter(|_| self.capture >= civ_rules::CAPTURE_FULL)
    }

    /// 0xBE9C70 on this city. ADAPTED: your cities are never lost and rival
    /// nations do not take each other's cities (one nation per city slot), so
    /// only the player's points may reach 100.
    pub fn add_capture(&mut self, amount: f32, attacker: usize) {
        let (capture, capturer) =
            civ_rules::add_capture(self.capture, self.capturer, self.nation(), attacker, amount);
        self.capture = capture;
        self.capturer = capturer;
        // 0xBE9AD0: percent >= 100 changes the owner at once, for any attacker.
        // ADAPTED: the capital (slot 0) cannot be held by a rival here (a rival
        // city's nation is its slot), so a rival's points on it stop short, and a
        // rival nation's points on another rival's city do too.
        let player_takes = self.capturer == Some(HOME_INDEX) && !self.owned();
        let rival_takes = self.owned() && self.index != HOME_INDEX && self.capturer.is_some_and(|c| c != HOME_INDEX);
        if !player_takes && !rival_takes {
            self.capture = self.capture.min(civ_rules::CAPTURE_FULL - 0.01);
        }
        self.refresh_armour();
    }

    /// cBuilding::TakeDamage 0xBCEE50 (Civ): a building loses health; when it
    /// falls the attacker earns 80 (8 capture points). Returns true when this
    /// hit destroyed it.
    pub fn damage_building(&mut self, plot: usize, damage: f32, attacker: usize) -> bool {
        self.calm = 0.0;
        if !self.standing(plot) || attacker == self.nation() {
            return false;
        }
        let taken = &mut self.building_damage[plot];
        *taken = (*taken + damage).min(BUILDING_HP);
        let fell = *taken >= BUILDING_HP;
        if fell {
            self.ruin_age[plot] = 0.0;
            self.add_capture(civ_rules::CAPTURE_BUILDING_VALUE, attacker);
        }
        self.refresh_armour();
        fell
    }

    /// The city hall's TakeDamage 0xBCF860 (Civ): while another building
    /// stands the hit is lost; afterwards every damage point is 0.1 capture
    /// point (the hall's own health does not drop). Returns whether it
    /// counted.
    pub fn damage_hall(&mut self, damage: f32, attacker: usize) -> bool {
        self.calm = 0.0;
        if !self.hall_exposed() || attacker == self.nation() {
            return false;
        }
        self.add_capture(damage, attacker);
        true
    }

    /// cTurret::TakeDamage 0xBD0660: health only (a fallen turret earns no
    /// capture points). Returns true when this hit destroyed it.
    pub fn damage_turret(&mut self, turret: usize, damage: f32) -> bool {
        self.calm = 0.0;
        let Some(hp) = self.turret_hp.get_mut(turret) else {
            return false;
        };
        if *hp <= 0.0 {
            return false;
        }
        *hp = (*hp - damage).max(0.0);
        let fell = *hp <= 0.0;
        if fell {
            if let Some(age) = self.turret_ruin_age.get_mut(turret) {
                *age = 0.0;
            }
        }
        fell
    }

    /// Area damage (a bomb) on every standing building and turret of the
    /// city; the hall is spared (ADAPTED: a bomb softens a city, vehicles
    /// take it). Returns the buildings destroyed.
    pub fn blast(&mut self, damage: f32, attacker: usize) -> usize {
        let mut fallen = 0;
        for plot in 0..self.plots.len() {
            if self.damage_building(plot, damage, attacker) {
                fallen += 1;
            }
        }
        for turret in 0..self.turret_hp.len() {
            self.damage_turret(turret, damage);
        }
        fallen
    }

    /// Repair every standing building and turret by `amount` (capped).
    pub fn repair(&mut self, amount: f32) {
        for plot in 0..self.plots.len() {
            if self.standing(plot) {
                self.building_damage[plot] = (self.building_damage[plot] - amount).max(0.0);
            }
        }
        for hp in &mut self.turret_hp {
            if *hp > 0.0 {
                *hp = (*hp + amount).min(TURRET_HP);
            }
        }
        self.refresh_armour();
    }

    /// One city update second (0xBE2260 + 0xBE3C20, after 20 s without an
    /// attack): capture points fall by 0.1, standing buildings heal 2 and
    /// turrets 1, ruins older than 30 s (turrets 120 s) are removed.
    pub fn city_second(&mut self) {
        let step = civ_rules::CIV_UPDATE_SECONDS;
        for age in self.ruin_age.iter_mut().chain(self.turret_ruin_age.iter_mut()) {
            *age += step;
        }
        let quiet = self.calm >= civ_rules::ATTACK_MEMORY_SECONDS;
        let (capture, capturer) = civ_rules::decay_capture(self.capture, self.capturer, quiet);
        self.capture = capture;
        self.capturer = capturer;
        if quiet {
            for plot in 0..self.plots.len() {
                if self.standing(plot) {
                    self.building_damage[plot] =
                        (self.building_damage[plot] - civ_rules::BUILDING_REPAIR_PER_SECOND * step).max(0.0);
                } else if self.ruined(plot) && self.ruin_age[plot] >= civ_rules::RUINED_BUILDING_CLEAR_SECONDS {
                    self.plots[plot] = None;
                    self.building_damage[plot] = 0.0;
                    self.buildings = self.buildings.saturating_sub(1);
                }
            }
            let mut i = 0;
            while i < self.turret_hp.len() {
                if self.turret_hp[i] > 0.0 {
                    self.turret_hp[i] =
                        (self.turret_hp[i] + civ_rules::TURRET_REPAIR_PER_SECOND * step).min(TURRET_HP);
                } else if self.turret_ruin_age[i] >= civ_rules::RUINED_TURRET_CLEAR_SECONDS {
                    self.turret_hp.remove(i);
                    self.turret_ruin_age.remove(i);
                    self.defense = self.defense.saturating_sub(1);
                    continue;
                }
                i += 1;
            }
        }
        self.refresh_armour();
    }

    pub fn built(&self) -> usize {
        self.plots.iter().flatten().count()
    }

    pub fn count(&self, kind: Building) -> usize {
        self.plots.iter().filter(|p| **p == Some(kind)).count()
    }

    /// The retail link graph: node 0 is the city hall, node `1 + p` plot `p`
    /// (retail slot `p + 1`), the links the authored `BuildingLink0..` matrix
    /// of the city's walls (`city_layout`, the 14 x 14 matrix of `cCityWalls`
    /// +0x274 read at 0xBEE5A0). A ruined building is no node (INFERRED:
    /// 0xBCEE50 re-runs ProcessBuildingUpdate when a building falls).
    pub fn link_graph(&self) -> (Vec<Option<civ_rules::CityBuilding>>, Vec<(usize, usize)>) {
        let layout = self.layout();
        let mut nodes = vec![Some(civ_rules::CityBuilding::CityHall)];
        nodes.extend(
            self.plots
                .iter()
                .enumerate()
                .map(|(i, p)| p.filter(|_| self.standing(i)).map(Building::model)),
        );
        // A saved list longer or shorter than the layout (before
        // `ensure_layout` ran) only links the slots both have.
        nodes.resize(layout.slots(), None);
        (nodes, layout.link_pairs())
    }

    /// `cCity::ProcessBuildingUpdate` on this city (`entertainment_off` while
    /// a Black Cloud hangs over it).
    pub fn model_with(&self, entertainment_off: bool) -> civ_rules::CityModel {
        let (nodes, links) = self.link_graph();
        civ_rules::evaluate_city(&nodes, &links, entertainment_off)
    }

    pub fn model(&self) -> civ_rules::CityModel {
        self.model_with(false)
    }

    /// The armour of the city untouched: the hall's 1000 plus 800 per
    /// building (ruins included until they are cleared).
    pub fn max_hp(&self) -> f32 {
        CITY_HP + self.built() as f32 * CITY_HP_PER_BUILDING
    }

    /// The city's mood, 0..100 (`cCity` +0x304; 50 is neutral).
    pub fn happiness(&self) -> i32 {
        self.model().happiness()
    }

    pub fn partying(&self) -> bool {
        self.party > 0.0
    }

    /// A riot halves the income (VERIFIED 0xBD8CD0).
    pub fn rioting(&self) -> bool {
        self.riot > 0.0
    }

    /// Money per update before mood and sieges: 0.1 x 400 per factory link
    /// to a house or the hall.
    pub fn raw_production(&self) -> f32 {
        self.model().money as f32 * civ_rules::INCOME_FACTOR
    }

    /// A city held by a Static Bomb pays nothing (its destroyed buildings
    /// already left the link model).
    pub fn producing(&self) -> bool {
        self.stunned <= 0.0
    }

    /// Money per update with the party / riot rule.
    pub fn income_with(&self, entertainment_off: bool) -> f32 {
        if !self.producing() {
            return 0.0;
        }
        let model = self.model_with(entertainment_off);
        civ_rules::city_income(model.money, self.partying(), self.rioting(), model.happiness()) as f32
            * civ_rules::INCOME_FACTOR
    }

    pub fn income(&self) -> f32 {
        self.income_with(false)
    }

    /// The citizens of the city (`cCity` +0x64C, VERIFIED: `AddCitizens` 0xBE3440
    /// counts up to 100, `RemoveCitizen` 0xBD8020 down to 20). ADAPTED: taken as
    /// the capacity of the connected houses and the hall, 15 rooms each
    /// (0xBD8AB0), which the retail city fills as it grows; the loading value of
    /// the economic vehicles (0xCEEC30 `n`) and the conversion shares read it.
    pub fn citizens(&self) -> u32 {
        (self.housing() * civ_rules::ROOMS_PER_HOUSE).min(100)
    }

    /// Houses connected to the hall plus the hall itself (0xBD8AB0): what the
    /// vehicle cap counts.
    pub fn housing(&self) -> u32 {
        let (nodes, links) = self.link_graph();
        let connected = civ_rules::connected_buildings(&nodes, &links);
        nodes
            .iter()
            .zip(connected)
            .filter(|(b, c)| {
                *c && matches!(b, Some(civ_rules::CityBuilding::House | civ_rules::CityBuilding::CityHall))
            })
            .count() as u32
    }

    /// Kept for old callers: the vehicles this city adds to its type's cap
    /// (`VehicleCapPerCity x 2` + housing).
    pub fn vehicle_capacity(&self) -> usize {
        (civ_rules::VEHICLE_CAP_PER_CITY * 2 + self.housing()) as usize
    }

    /// Retail city value (0xBE6EE0): buildings x1.2, live turrets x0.5, +500,
    /// doubled for a non-neutral owner (every OpenSpore rival).
    pub fn value(&self) -> f32 {
        let buildings: f32 = self.plots.iter().flatten().map(|b| b.cost()).sum();
        let turrets = self.live_turrets() as f32 * TURRET_COST;
        civ_rules::city_value(buildings, turrets, 0.0, false)
    }

    /// What buying the city costs (VERIFIED 0xCEECE0): the owner holds one
    /// city (each OpenSpore rival is one city), minus the trade value.
    pub fn buy_price(&self) -> f32 {
        civ_rules::buy_price(self.value(), 1, self.trade_value)
    }

    pub fn live_turrets(&self) -> usize {
        self.turret_hp.iter().filter(|hp| **hp > 0.0).count()
    }

    pub fn label(&self) -> String {
        city_label(self.index)
    }

    /// Planning score: income now, with the mood as a tie-breaker and a
    /// heavy penalty for riot risk.
    fn plan_score(&self) -> f32 {
        // Income per update, plus mood up to the party range; a mood under 50
        // risks riots (VERIFIED 0xBD8D20), heavily penalised.
        let mood = self.happiness();
        self.raw_production() + 0.4 * mood.min(80) as f32
            - if mood < civ_rules::HAPPINESS_NEUTRAL { 40.0 } else { 0.0 }
    }

    /// The free plot where `kind` scores best (lowest index on ties).
    pub fn best_plot(&self, kind: Building) -> Option<(usize, f32)> {
        let before = self.plan_score();
        let mut best: Option<(usize, f32)> = None;
        for plot in 0..self.plots.len() {
            if self.plots[plot].is_some() {
                continue;
            }
            let mut trial = self.clone();
            trial.plots[plot] = Some(kind);
            let gain = trial.plan_score() - before;
            if best.is_none_or(|(_, g)| gain > g + 1e-4) {
                best = Some((plot, gain));
            }
        }
        best
    }

    /// The building `B` raises: the best gain per spice over every free plot.
    pub fn smart_building(&self) -> Option<(Building, usize)> {
        let mut best: Option<(Building, usize, f32)> = None;
        for kind in Building::ALL {
            if let Some((plot, gain)) = self.best_plot(kind) {
                let value = gain / kind.cost();
                if best.is_none_or(|(_, _, v)| value > v + 1e-6) {
                    best = Some((kind, plot, value));
                }
            }
        }
        best.map(|(kind, plot, _)| (kind, plot))
    }
}

/// C1..C10 on the map; `index` is the campaign's city index (0 = capital).
pub fn city_label(index: usize) -> String {
    format!("C{}", index + 1)
}

/// The flat layout (no planet: headless runs, `OPENSPORE_FLAT_TERRAIN=1`):
/// `civ_planet::lay_out_sites` on the plane, at the retail rings and spacing.
/// On the planet the campaign holds the planet layout (`CityState::position`,
/// `CivCampaign::geyser_sites`).
fn flat_sites() -> &'static crate::civ_planet::Sites {
    static SITES: std::sync::OnceLock<crate::civ_planet::Sites> = std::sync::OnceLock::new();
    SITES.get_or_init(|| crate::civ_planet::lay_out_sites(&crate::civ_planet::Globe::flat(), 0))
}

/// A city's place in the flat layout (index 0 is the capital).
pub fn city_position(index: usize) -> Vec3 {
    flat_sites().cities[index % CITY_COUNT]
}

/// A spice geyser's place in the flat layout.
pub fn geyser_position(index: usize) -> Vec3 {
    flat_sites().geysers[index % GEYSER_COUNT]
}

/// Nation colours: you are teal, every rival nation has its own banner.
/// ADAPTED: ten distinct banners (the retail political colours of
/// `cPoliticalIDManager` were not read); the first five are the earlier ones.
pub fn nation_color(nation: usize) -> Color {
    match nation {
        0 => Color::srgb(0.16, 0.78, 0.66),
        1 => Color::srgb(0.88, 0.2, 0.16),
        2 => Color::srgb(0.96, 0.78, 0.16),
        3 => Color::srgb(0.6, 0.32, 0.9),
        4 => Color::srgb(0.24, 0.46, 0.96),
        5 => Color::srgb(0.96, 0.5, 0.12),
        6 => Color::srgb(0.36, 0.72, 0.22),
        7 => Color::srgb(0.95, 0.46, 0.72),
        8 => Color::srgb(0.84, 0.88, 0.92),
        _ => Color::srgb(0.56, 0.38, 0.22),
    }
}

/// Prototype skyline layout: a hall slot above the tower plus a ring of build
/// plots around it, in city-local units. These mount points are OpenSpore
/// placement, not verified original city architecture.
pub const CITY_SLOTS: &[VehicleSlot] = &[
    VehicleSlot {
        name: "hall",
        offset: Vec3::new(0.0, 2.4, 0.0),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "plaza_N",
        offset: Vec3::new(0.0, 0.0, 5.0),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "plaza_E",
        offset: Vec3::new(5.0, 0.0, 0.0),
        base_yaw: std::f32::consts::FRAC_PI_2,
    },
    VehicleSlot {
        name: "plaza_S",
        offset: Vec3::new(0.0, 0.0, -5.0),
        base_yaw: std::f32::consts::PI,
    },
    VehicleSlot {
        name: "plaza_W",
        offset: Vec3::new(-5.0, 0.0, 0.0),
        base_yaw: -std::f32::consts::FRAC_PI_2,
    },
    VehicleSlot {
        name: "terrace",
        offset: Vec3::new(3.4, 1.6, 3.4),
        base_yaw: std::f32::consts::FRAC_PI_4,
    },
];

/// Pure: world placement of a mounted skyline part (city-local space).
pub fn city_part_transform(slot_index: u32, p: &VehiclePart) -> (Vec3, f32) {
    let slot = CITY_SLOTS
        .get(slot_index as usize)
        .or_else(|| CITY_SLOTS.first())
        .copied()
        .unwrap_or(VehicleSlot {
            name: "?",
            offset: Vec3::ZERO,
            base_yaw: 0.0,
        });
    let off = Vec3::new(p.offset[0], p.offset[1], p.offset[2]);
    (slot.offset + off, slot.base_yaw + p.yaw)
}

/// Which assembly the civ editor writes: the vehicles, or the city skyline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CivEditorMode {
    #[default]
    Vehicle,
    Building,
}

impl CivEditorMode {
    /// The original catalog group this mode selects definitions from.
    pub fn group(self) -> u32 {
        match self {
            CivEditorMode::Vehicle => asset_catalog::VEHICLE_PARTS,
            CivEditorMode::Building => asset_catalog::BUILDING_PARTS,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            CivEditorMode::Vehicle => "vehicle",
            CivEditorMode::Building => "city skyline",
        }
    }

    /// Mount points `,`/`.` cycle through in this mode.
    pub fn slots(self) -> u32 {
        match self {
            CivEditorMode::Vehicle => VEHICLE_SLOTS.len() as u32,
            CivEditorMode::Building => CITY_SLOTS.len() as u32,
        }
    }

    /// World-size cap for one part's largest axis, before its own multiplier.
    pub fn cap(self) -> f32 {
        match self {
            CivEditorMode::Vehicle => VEHICLE_PART_CAP,
            CivEditorMode::Building => BUILDING_PART_CAP,
        }
    }

    /// Imported model units to stage units.
    pub fn base(self) -> f32 {
        match self {
            CivEditorMode::Vehicle => VEHICLE_PART_BASE,
            CivEditorMode::Building => CITY_PART_BASE,
        }
    }
}

/// How a rival city fell; decides the capture message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conquest {
    Military,
    Religious,
    Economic,
}

/// Take a city. Returns true only when the one-time reward was actually paid.
pub fn capture_city(city: &mut CityState, spice: &mut f32) -> bool {
    city.owner = CityOwner::Player;
    city.conversion = 1.0;
    city.spent = BUY_COST.max(city.spent);
    city.ensure_layout();
    // 0xBE9AD0: the captured city's broken buildings are repaired (the hall's
    // Heal(0), ModAPI "repairs broken buildings") and the points reset.
    for plot in 0..city.plots.len() {
        city.building_damage[plot] = 0.0;
        city.ruin_age[plot] = 0.0;
    }
    city.capture = 0.0;
    city.capturer = None;
    // Spore rebuilds a conquered city's defenses under the new flag.
    city.turret_hp = vec![TURRET_HP; (city.defense as usize).min(city.max_turrets())];
    city.turret_ruin_age = vec![0.0; city.turret_hp.len()];
    city.refresh_armour();
    city.calm = civ_rules::ATTACK_MEMORY_SECONDS;
    city.almost = 0.0;
    city.riot = 0.0;
    city.stunned = 0.0;
    if city.reward_paid {
        return false;
    }
    city.reward_paid = true;
    *spice += CAPTURE_REWARD;
    true
}

/// Pure: whichever way a rival city has fallen, if it has.
pub fn conquest_of(city: &CityState) -> Option<Conquest> {
    if city.owned() {
        None
    } else if city.captured_by() == Some(HOME_INDEX) {
        Some(Conquest::Military)
    } else if city.conversion >= 1.0 {
        Some(Conquest::Religious)
    } else if city.trade && city.spent >= city.buy_price() {
        Some(Conquest::Economic)
    } else {
        None
    }
}

/// Pure: raise `kind` on `plot` (or its best plot) of an owned city.
/// Returns `(spice_left, plot)` or the reason it was refused.
pub fn buy_plot(
    city: &mut CityState,
    kind: Building,
    plot: Option<usize>,
    spice: f32,
) -> Result<(f32, usize), String> {
    let name = city.label();
    if !city.owned() {
        return Err(format!("{name} is not yours to build on"));
    }
    city.ensure_layout();
    let plot = match plot.or_else(|| city.best_plot(kind).map(|(p, _)| p)) {
        Some(plot) if city.plots.get(plot) == Some(&None) => plot,
        Some(_) => return Err(format!("{name}: that plot is taken")),
        None => return Err(format!("{name}: every plot is built")),
    };
    let cost = kind.cost();
    if spice < cost {
        return Err(format!("{name}: a {} costs {cost:.0} spice", kind.title()));
    }
    city.plots[plot] = Some(kind);
    city.buildings = city.buildings.saturating_add(1);
    // The new building arrives at full health.
    city.building_damage[plot] = 0.0;
    city.ruin_age[plot] = 0.0;
    city.refresh_armour();
    Ok((spice - cost, plot))
}

/// Pure: sell the building on `plot` (or the last plot built) of an owned
/// city for the difficulty's refund of its price (VERIFIED 0xCF3350 ->
/// 0xCF7BB0: `EasyRefundFraction` 1.0 / medium 0.75 / hard 0.5). Returns
/// (refund, building, plot).
pub fn sell_plot(
    city: &mut CityState,
    plot: Option<usize>,
    difficulty: civ_rules::Difficulty,
) -> Result<(f32, Building, usize), String> {
    let name = city.label();
    if !city.owned() {
        return Err(format!("{name} is not yours to sell from"));
    }
    city.ensure_layout();
    let plot = match plot.or_else(|| (0..city.plots.len()).rev().find(|p| city.standing(*p))) {
        Some(p) if city.standing(p) => p,
        Some(p) if city.ruined(p) => return Err(format!("{name}: that building is in ruins")),
        _ => return Err(format!("{name}: nothing to sell")),
    };
    let kind = city.plots[plot].take().expect("checked");
    city.buildings = city.buildings.saturating_sub(1);
    city.building_damage[plot] = 0.0;
    city.refresh_armour();
    Ok((civ_rules::refund(kind.cost(), difficulty), kind, plot))
}

/// Pure: add a wall turret to an owned city, or rebuild a ruined one.
pub fn buy_turret(city: &mut CityState, spice: f32) -> Result<f32, String> {
    let name = city.label();
    if !city.owned() {
        return Err(format!("{name} is not yours to fortify"));
    }
    city.ensure_layout();
    let ruined = city.turret_hp.iter().position(|hp| *hp <= 0.0);
    if ruined.is_none() && city.defense as usize >= city.max_turrets() {
        return Err(format!("{name}: every tower has a turret"));
    }
    if spice < TURRET_COST {
        return Err(format!("{name}: a turret costs {TURRET_COST:.0} spice"));
    }
    match ruined {
        Some(i) => {
            city.turret_hp[i] = TURRET_HP;
            city.turret_ruin_age[i] = 0.0;
        }
        None => {
            city.defense += 1;
            city.turret_hp.push(TURRET_HP);
            city.turret_ruin_age.push(0.0);
        }
    }
    Ok(spice - TURRET_COST)
}

/// What a rival city raised in one second (`npc_build_second`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcBuilt {
    /// A building on a plot (a free plot or a ruin).
    Building(usize, Building),
    /// A new turret (its index in `turret_hp`).
    Turret(usize),
    /// The city's free first vehicle (`mbFirstHarvester`): one land vehicle
    /// of the city's type, at no charge (`cCity::SpawnVehicle`).
    FirstVehicle,
}

/// The retail vehicle specialty (`cCity` +0x540: 0 military, 1 religious,
/// 2 economic) of a city type.
pub fn specialty(kind: Approach) -> usize {
    match kind {
        Approach::Military => 0,
        Approach::Religious => 1,
        Approach::Economic => 2,
    }
}

/// VERIFIED 0xBDB6E0 on the city's authored link graph (`CityState::link_graph`:
/// the hall and the 5 / 11 building slots of its walls): the best plot for `kind`
/// among the free and ruined ones, with its score (`civ_rules::npc_link_score`;
/// a neighbouring ruin counts as its building, as the retail dead building
/// keeps its noun type until it is removed). `None` when no plot scores
/// above 0.
pub fn npc_slot_score(city: &CityState, kind: Building) -> Option<(usize, i32)> {
    let n = city.plots.len();
    let happiness = city.happiness();
    let earning = city.income() > 0.0;
    // Node 0 is the hall, node 1 + p plot p (`link_graph`).
    let node = |i: usize| -> Option<civ_rules::CityBuilding> {
        if i == 0 {
            Some(civ_rules::CityBuilding::CityHall)
        } else {
            city.plots.get(i - 1).copied().flatten().map(Building::model)
        }
    };
    let (_, links) = city.link_graph();
    let mut best: Option<(usize, i32)> = None;
    for plot in 0..n {
        if city.standing(plot) {
            continue;
        }
        let me = 1 + plot;
        let score: i32 = links
            .iter()
            .filter_map(|&(a, b)| if a == me { Some(b) } else if b == me { Some(a) } else { None })
            .map(|other| civ_rules::npc_link_score(kind.model(), node(other), happiness, earning))
            .sum();
        if score > best.map_or(0, |(_, s)| s) {
            best = Some((plot, score));
        }
    }
    best
}

/// VERIFIED 0xBE5890: one second of a rival city's construction with the
/// nation's domestic budget (`rolls` are the two RandomInt draws: a building
/// 1 in 8c, a quiet-time turret 1 in 10c). Under attack (0xC00600 slot 0 or
/// 1: hit, or preached to, in the last 20 s, `CityState::under_attack`) only
/// the emergency turret (normal / hard, fewer than 2, after the turret wait);
/// then, once per city, the free first vehicle (`mbFirstHarvester`) when the
/// nation has room for one of the city's type (`vehicle_room`: 0xBF2240 cap
/// minus fleet > 0, below 36 vehicles; the land tech 0xBDB9D0 holds for every
/// city); otherwise the building kind whose best plot scores highest (house,
/// entertainment, factory on ties) when affordable, else maybe a turret
/// toward the quiet-time goal. Turrets are capped by the city's walls' turret
/// mounts (VERIFIED 0xBD8BF0, `CityState::max_turrets`: 8 / 6 / 4 behind large
/// walls, 5 / 4 / 3 behind the small ones every rival city has).
pub fn npc_build_second(
    city: &mut CityState,
    domestic: &mut f32,
    difficulty: civ_rules::Difficulty,
    rolls: [u32; 2],
    vehicle_room: bool,
) -> Option<NpcBuilt> {
    if *domestic < civ_rules::NPC_BUILD_MIN_BUDGET || city.owned() {
        return None;
    }
    city.ensure_layout();
    let waited = city.age > civ_rules::npc_turret_wait_seconds(difficulty);
    let slots = city.max_turrets();
    let turrets = city.turret_hp.len();
    if city.under_attack() {
        if difficulty != civ_rules::Difficulty::Easy
            && waited
            && turrets < slots
            && turrets < civ_rules::NPC_EMERGENCY_TURRETS
        {
            return npc_add_turret(city, domestic);
        }
        return None;
    }
    if !city.first_vehicle && vehicle_room {
        city.first_vehicle = true;
        return Some(NpcBuilt::FirstVehicle);
    }
    if rolls[0] == 0 || city.income() <= 0.0 {
        let mut pick: Option<(Building, usize, i32)> = None;
        for kind in [Building::House, Building::Entertainment, Building::Factory] {
            if let Some((plot, score)) = npc_slot_score(city, kind) {
                if score > pick.map_or(0, |(_, _, s)| s) {
                    pick = Some((kind, plot, score));
                }
            }
        }
        if let Some((kind, plot, _)) = pick {
            if kind.cost() <= *domestic {
                *domestic -= kind.cost();
                city.plots[plot] = Some(kind);
                city.building_damage[plot] = 0.0;
                city.ruin_age[plot] = 0.0;
                city.refresh_armour();
                return Some(NpcBuilt::Building(plot, kind));
            }
        }
    }
    if rolls[1] != 0 || !waited {
        return None;
    }
    let goal = civ_rules::npc_quiet_turret_goal(slots, city.standing_buildings(), difficulty).min(slots);
    if turrets < goal {
        return npc_add_turret(city, domestic);
    }
    None
}

/// 0xBE39A0: a turret in the next free tower, paid from the domestic budget
/// (`DefenseCost` 2000, read by 0xBE5890 from DAT_0169C1B0).
fn npc_add_turret(city: &mut CityState, domestic: &mut f32) -> Option<NpcBuilt> {
    if *domestic < TURRET_COST || city.turret_hp.len() >= city.max_turrets() {
        return None;
    }
    *domestic -= TURRET_COST;
    city.defense += 1;
    city.turret_hp.push(TURRET_HP);
    city.turret_ruin_age.push(0.0);
    Some(NpcBuilt::Turret(city.turret_hp.len() - 1))
}

/// Pure: what the HUD reports about a rival city's resistance.
pub fn city_status(city: &CityState) -> String {
    if city.owned() {
        let mood = city.happiness();
        let state = if !city.producing() {
            "besieged"
        } else if city.rioting() {
            "rioting"
        } else if city.partying() {
            "celebrating"
        } else {
            "working"
        };
        return format!(
            "{} {} (yours): +{:.0} spice, mood {mood}%, {state}",
            city.label(),
            city.kind.title(),
            city.income(),
        );
    }
    let trade = if city.trade {
        format!(
            "bought {:.0}% of {:.0}",
            (city.spent / city.buy_price()).clamp(0.0, 1.0) * 100.0,
            city.buy_price()
        )
    } else {
        "no trade".into()
    };
    format!(
        "{} {}: buildings {}/{}, captured {:.0}%, turrets {}/{}, faith {:.0}%, {trade}",
        city.label(),
        city.kind.title(),
        city.standing_buildings(),
        city.built(),
        if city.capturer == Some(HOME_INDEX) { city.capture } else { 0.0 },
        city.live_turrets(),
        city.turret_hp.len(),
        city.conversion.clamp(0.0, 1.0) * 100.0,
    )
}

/// The persisted Civilization campaign. New fields default so old saves load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CivCampaign {
    /// The capital's doctrine: the type of vehicles it builds.
    pub approach: Approach,
    /// Fixed order: index 0 is the capital, then the rivals.
    pub cities: Vec<CityState>,
    /// Last line of feedback, shown under the objective line.
    pub message: String,
    /// Unused since the doctrine is carried by vehicles; kept for old saves.
    pub action_cooldown: f32,
    /// Seconds of spice income banked so far.
    pub income_timer: f32,
    /// Unused since vehicles are rebuilt instead of towed; kept for old saves.
    pub recovery: f32,
    pub vehicles_lost: u32,
    /// True once every city is the player's.
    pub unified: bool,
    /// Latch: the stage is left exactly once.
    pub advanced: bool,
    /// The rival city nearest the camera, for the HUD readout.
    pub last_target: Option<usize>,
    /// Which assembly the editor grammar writes (`V` toggles it).
    pub editor_mode: CivEditorMode,
    /// Selection cursors: the last chosen original model per mode, kept apart
    /// from the mounted loadout so a model that fails to import can never
    /// trap the cycle on an otherwise empty build.
    pub vehicle_cursor: u32,
    pub building_cursor: u32,
    /// Rival nations by city index (entry 0 is unused).
    pub nations: Vec<NationState>,
    /// Spice geysers and their derricks.
    pub geysers: Vec<GeyserState>,
    /// Every vehicle on the planet, snapshotted from the live entities.
    pub units: Vec<UnitState>,
    pub next_unit: u32,
    /// The opening fleet, derricks and purse have been handed out.
    pub started: bool,
    /// Stage time in seconds (rival AI pacing).
    pub clock: f32,
    /// Active Cell consequence power.
    #[serde(default)]
    pub cell_power: Option<CivCellPower>,
    /// Active Creature consequence power.
    #[serde(default)]
    pub creature_power: Option<CivCreaturePower>,
    /// Active Tribal consequence power.
    #[serde(default)]
    pub tribal_power: Option<CivTribalPower>,
    /// Cooldown for the Cell consequence power (seconds).
    #[serde(default)]
    pub power_cell_cooldown: f32,
    /// Cooldown for the Creature consequence power (seconds).
    #[serde(default)]
    pub power_creature_cooldown: f32,
    /// Cooldown for the Tribal consequence power (seconds).
    #[serde(default)]
    pub power_tribal_cooldown: f32,
    /// Cooldown for the ultimate superweapon (seconds).
    #[serde(default)]
    pub superweapon_cooldown: f32,
    /// Temporary invulnerability duration remaining (seconds).
    #[serde(default)]
    pub invulnerability_timer: f32,
    /// Temporary static bomb EMP shock duration remaining (seconds).
    #[serde(default)]
    pub static_bomb_timer: f32,
    /// Diplomatic peace pact duration remaining (seconds).
    #[serde(default)]
    pub diplo_dervish_timer: f32,
    /// City index and remaining duration under Black Cloud suppression.
    #[serde(default)]
    pub black_cloud_city: Option<(usize, f32)>,
    /// `mVehicleTechLevel` (VERIFIED 0xBF44B0; 0 in saves that predate it).
    #[serde(default)]
    pub tech_level: u8,
    /// Cities taken by each method (`cCivilization` +0x4C0..+0x4C8).
    #[serde(default)]
    pub captures: civ_rules::CaptureCounts,
    /// The play style card earned at Advance (0xCF82C0).
    #[serde(default)]
    pub play_style: Option<civ_rules::PlayStyle>,
    /// One-second civ updates run so far (0xAE61C0) and the part second.
    #[serde(default)]
    pub ticks: u32,
    #[serde(default)]
    pub civ_second: f32,
    /// The player's civilization has "developed" (0xBFBD30; scales NPC income).
    #[serde(default)]
    pub player_developed: bool,
    /// The superweapon was launched (`mbLauncedIWinWeapon`).
    #[serde(default)]
    pub superweapon_launched: bool,
    /// Stats of the player's vehicle class designs (`civ_combat`).
    #[serde(default)]
    pub vehicle_stats: Vec<(Approach, VehicleDomain, crate::civ_combat::VehicleStats)>,
    /// Conversation state with the rivals (`civ_comm`).
    #[serde(default)]
    pub comm: crate::civ_comm::CommLedger,
    /// The epic predators roaming the planet (`civ_wildlife`).
    #[serde(default)]
    pub epics: Vec<crate::civ_wildlife::EpicState>,
    /// Which layout the positions belong to (`civ_planet::layout_key`; 0 =
    /// the island of earlier builds, laid out again on entry).
    #[serde(default)]
    pub layout_key: u64,
    /// The planet's sphere in the stage (centre xyz, radius), none on the
    /// plane: `shape()`.
    #[serde(default)]
    pub sphere: Option<[f32; 4]>,
    /// The goodie huts' world points (`civ_planet::hut_sites`; none on the flat layout).
    #[serde(default)]
    pub hut_sites: Vec<[f32; 3]>,
    /// The huts already captured (retail destroys the hut's tribe at 0xDCB2A0, the hut goes).
    #[serde(default)]
    pub huts_taken: Vec<usize>,
    /// The spice geysers' world points (index-aligned with `geysers`).
    #[serde(default)]
    pub geyser_sites: Vec<[f32; 3]>,    /// The player's city anthem (`civ_music`: rolled once with the retail
    /// rules of 0xEB0430, then kept).
    #[serde(default)]
    pub anthem: Option<crate::civ_music::CityAnthem>,
    /// Rival cities not built yet: they appear over time (VERIFIED 0xAE2EF0 /
    /// 0xAE3AB0, `civ_reveal`). Empty in saves that predate it (every city
    /// there from the start).
    #[serde(default)]
    pub hidden: Vec<usize>,
    /// The new-city timer (seconds run; `None` while stopped).
    #[serde(default)]
    pub reveal_timer: Option<f32>,
    /// `CVG_PRE_NewCityAppears` plays and will place the city with its
    /// message (runtime only).
    #[serde(skip)]
    pub reveal_pending: bool,
    /// The reveal order key of each city slot (VERIFIED 0xAE3430: slot +0x2C,
    /// lowest first; `civ_reveal::order_keys`, written with the layout).
    /// Empty in older saves: the city index is the order.
    #[serde(default)]
    pub reveal_order: Vec<u32>,
    /// The continent of each city slot (VERIFIED: the retail slot +0x20
    /// continent id, 0xB886E0; here cities joined by a land path share one,
    /// `civ_planet::lay_out`). Empty in older saves: one continent.
    #[serde(default)]
    pub continents: Vec<u8>,
    /// The goal manager and the tutorial flags (`civ_goals`).
    #[serde(default)]
    pub goals: crate::civ_goals::GoalChain,
}

/// The vehicles of one nation performing at a city: the group of 0xBEB310 (`size`
/// vehicles, `power` = the sum of `civ_rules::vehicle_group_power`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreachGroup {
    pub city: usize,
    pub nation: usize,
    pub size: u32,
    pub power: f32,
}

impl CivCampaign {
    /// The stage's surface shape (the planet's sphere, else the plane).
    pub fn shape(&self) -> Shape {
        Shape::from_saved(self.sphere)
    }

    /// Retail distance between two world points (`Shape::distance`).
    pub fn distance(&self, a: Vec3, b: Vec3) -> f32 {
        self.shape().distance(a, b)
    }

    /// One of your cities whose `rival_conversion` is full stays "almost
    /// converted" for 2 s (`TimeInAlmostConvert`), then goes back to the
    /// nation it was taken from. ADAPTED: OpenSpore keeps one nation per
    /// city slot (the converter's faith wins it for that nation) and your
    /// capital is never lost (its meter stops short of full). Returns the
    /// message of a city lost.
    pub fn convert_away(&mut self, dt: f32) -> Option<String> {
        let mut lost = None;
        for index in 0..self.cities.len().min(CITY_COUNT) {
            let city = &mut self.cities[index];
            if !city.owned() || city.rival_conversion < 1.0 {
                continue;
            }
            if index == HOME_INDEX {
                // ADAPTED: the capital is never lost (a rival city's nation is
                // its slot, and slot 0 is yours): one citizen is sent back so
                // the faith stops short of a majority.
                if let Some(lead) = city.leader.filter(|_| city.buckets.len() == CITY_COUNT) {
                    city.buckets[lead] = city.buckets[lead].saturating_sub(1);
                    city.buckets[HOME_INDEX] += 1;
                    city.refresh_allegiance();
                } else {
                    city.rival_conversion = 0.99;
                }
                continue;
            }
            city.almost += dt;
            if city.almost < civ_rules::TIME_IN_ALMOST_CONVERT {
                continue;
            }
            let by = city.converter.unwrap_or(index);
            city.owner = CityOwner::Rival;
            city.reset_allegiance();
            city.conversion = 0.0;
            city.rival_conversion = 0.0;
            city.converter = None;
            city.almost = 0.0;
            city.trade = false;
            city.spent = 0.0;
            city.calm = 0.0;
            lost = Some(format!("{} was converted by {}'s faith and left your nation", city_label(index), city_label(by)));
        }
        lost
    }

    /// 0xBE9AD0 on one of the player's cities: a rival's capture points reached 100
    /// and the city changes hands (`SetOwner` method 0): its buildings are
    /// repaired and its turrets rebuilt under the new flag, every citizen goes to
    /// the new owner. ADAPTED: the city returns to the nation of its slot (one
    /// nation per slot), whichever rival took it. Returns the message.
    pub fn lose_city(&mut self, index: usize, by: usize) -> String {
        let city = &mut self.cities[index];
        city.owner = CityOwner::Rival;
        city.capture = 0.0;
        city.capturer = None;
        city.conversion = 0.0;
        city.rival_conversion = 0.0;
        city.converter = None;
        city.almost = 0.0;
        city.trade = false;
        city.spent = 0.0;
        city.calm = 0.0;
        city.stunned = 0.0;
        city.riot = 0.0;
        for plot in 0..city.plots.len() {
            city.building_damage[plot] = 0.0;
            city.ruin_age[plot] = 0.0;
        }
        city.turret_hp = vec![TURRET_HP; (city.defense as usize).min(city.max_turrets())];
        city.turret_ruin_age = vec![0.0; city.turret_hp.len()];
        city.refresh_armour();
        city.reset_allegiance();
        format!("{} fell to {}'s army and left your nation", city_label(index), city_label(by))
    }

    /// The conversion step of 0xBEB310 for every group of vehicles performing at a
    /// city this second (`civ_rules` "the conversion model"): the group's odds
    /// of converting one citizen, rolled from the clock.
    pub fn preach_second(&mut self, groups: &[PreachGroup]) {
        let stamp = (self.clock * 1000.0) as u32;
        for g in groups {
            let Some(city) = self.cities.get_mut(g.city) else {
                continue;
            };
            if g.nation == city.nation() || g.nation >= CITY_COUNT {
                continue;
            }
            city.ensure_buckets();
            let reaction = civ_rules::crowd_reaction(civ_rules::conversion_strength(g.power, g.size), city.happiness());
            let odds = civ_rules::conversion_odds(reaction);
            let roll = city_roll(g.city, stamp, 0xC017 ^ g.nation as u32, 10_000) as f32 / 10_000.0;
            if roll < odds {
                city.convert_citizens(g.nation, 1);
            } else {
                city.refresh_allegiance();
            }
        }
    }

    /// Goodie hut `h` (`None` past the laid-out huts).
    pub fn hut_at(&self, h: usize) -> Option<Vec3> {
        self.hut_sites.get(h).map(|p| Vec3::from_array(*p))
    }

    /// Whether hut `h` still stands (laid out and not captured).
    pub fn hut_standing(&self, h: usize) -> bool {
        h < self.hut_sites.len() && !self.huts_taken.contains(&h)
    }

    /// Spice geyser `g` (the planet layout, else the flat one).
    pub fn geyser_at(&self, g: usize) -> Vec3 {
        self.geyser_sites
            .get(g)
            .map(|p| Vec3::from_array(*p))
            .unwrap_or_else(|| geyser_position(g))
    }

    /// Where a city's new vehicles roll out: just outside its walls, facing
    /// the capital (the capital's own pad is in front of it, toward the
    /// opening camera).
    pub fn vehicle_pad(&self, index: usize) -> Vec3 {
        let shape = self.shape();
        let Some(city) = self.cities.get(index) else {
            return Vec3::ZERO;
        };
        let center = city.center();
        let out = if index == HOME_INDEX {
            shape.bearing(center, std::f32::consts::FRAC_PI_2)
        } else {
            let capital = self.cities[HOME_INDEX].center();
            let to = shape.from_tangent(center, shape.to_tangent(center, capital - center));
            if to.length_squared() > 1e-6 {
                to
            } else {
                shape.bearing(center, 0.0)
            }
        };
        shape.toward(center, out, 8.5 * CITY_SCALE)
    }

    /// Idempotent: builds the city list, nations and geysers if a save
    /// predates them, and lays out every city's building ring.
    pub fn ensure_cities(&mut self) {
        if self.cities.len() != CITY_COUNT {
            if self.cities.is_empty() || self.cities.len() > CITY_COUNT {
                self.cities = (0..CITY_COUNT).map(CityState::new).collect();
            } else {
                // A save of the earlier five-city planet keeps its cities;
                // the new slots wait hidden and appear like any NPC city
                // (`civ_reveal`), and the planet lays out again (positions
                // only, `civ_planet::LAYOUT_VERSION`).
                let first = self.cities.len();
                self.cities.extend((first..CITY_COUNT).map(CityState::new));
                self.hidden.extend(first..CITY_COUNT);
                self.layout_key = 0;
            }
        }
        for city in &mut self.cities {
            city.ensure_layout();
        }
        if self.nations.len() != CITY_COUNT {
            self.nations.resize(
                CITY_COUNT,
                NationState {
                    spice: RIVAL_STARTING_SPICE,
                    war: false,
                    raid: 60.0,
                    ..Default::default()
                },
            );
        }
        if self.geysers.len() != GEYSER_COUNT {
            self.geysers.resize(GEYSER_COUNT, GeyserState::default());
        }
        // The consequence powers are resolved from the earlier stages by
        // `civ_campaign_update` (0.82 filled defaults here, which hid them).
        self.tech_level = civ_rules::raise_tech_level(self.tech_level, self.owned_count(), CITY_COUNT);
    }

    /// The opening position, once per campaign: a purse and a three-vehicle
    /// fleet of the capital's type; a rival present from the start gets its
    /// first budget (`rival_opening`). Returns whether anything was handed out.
    ///
    /// No nation starts with a derrick. VERIFIED (march2017, `notes\civ-rows3-notes.md` 2): a
    /// spice node's owner is written only by `cCommodityNode::SetOwner` 0xBFF410, whose callers
    /// release the node (owner -1: 0xBFF820 from 0xBC00B0 / 0xBC023A, 0xBFF890 from 0xBF7EC0), run
    /// in its own update (0xBFF8E0, 0xBFFF41) or land the pump a vehicle built (0xCB958D, started
    /// by 0xBFE590 from the harvest command 0xDCAF7E); neither the level setup 0xB9D970 (its direct calls
    /// into 0xBFC000 .. 0xC01000 and 0xCB9000 .. 0xCBA000 were scanned: only the accessors 0xBFC730 / 0xBFC5A0) nor the civilization constructor 0xBF7A10 / start wealth 0xB60D1A reaches
    /// them, and the tutorial's first goal (`CaptureNode`, completed by that very SetOwner for
    /// the player, `notes\civ-goals-notes.md` 1) asks the player to send a vehicle to a free
    /// geyser. Rivals claim through 0xBF9960 like the player (`civ_rts::rts_ai`).
    pub fn begin(&mut self, spice: &mut f32) -> bool {
        self.ensure_cities();
        if self.started {
            return false;
        }
        self.started = true;
        *spice += STARTING_SPICE;
        self.cities[HOME_INDEX].kind = self.approach;
        self.cities[HOME_INDEX].ensure_layout();
        let shape = self.shape();
        let pad = self.vehicle_pad(HOME_INDEX);
        let side = shape.bearing(pad, 0.0);
        for k in 0..3 {
            let at = shape.toward(pad, side, (k as f32 - 1.0) * 2.2 * CITY_SCALE);
            self.add_unit(HOME_INDEX, self.approach, false, at);
        }
        for nation in 1..CITY_COUNT {
            // A rival city that has not appeared gets its opening with it.
            if self.present(nation) {
                self.rival_opening(nation);
            }
        }
        true
    }

    /// Allocate a persisted vehicle (the live entity is spawned by `civ_rts`).
    pub fn add_unit(&mut self, nation: usize, kind: Approach, air: bool, at: Vec3) -> UnitState {
        self.add_unit_domain(
            nation,
            kind,
            if air {
                VehicleDomain::Air
            } else {
                VehicleDomain::Land
            },
            at,
        )
    }

    /// Allocate a persisted vehicle with an explicit domain (Land, Air, Sea).
    pub fn add_unit_domain(
        &mut self,
        nation: usize,
        kind: Approach,
        domain: VehicleDomain,
        at: Vec3,
    ) -> UnitState {
        self.next_unit += 1;
        let shape = self.shape();
        let yaw = match self.cities.get(HOME_INDEX) {
            Some(capital) if nation != HOME_INDEX => shape.yaw_of(at, capital.center() - at),
            _ => 0.0,
        };
        let unit = UnitState {
            id: self.next_unit,
            nation,
            kind,
            air: domain == VehicleDomain::Air,
            domain,
            world: Some(at.to_array()),
            position: [at.x, at.z],
            yaw,
            hp: 0.0,
            order: UnitOrder::Idle,
            // The player's vehicles take their class design's stats.
            stats: if nation == HOME_INDEX { self.class_stats(kind, domain) } else { None },
            shuttle: None,
        };
        let unit = UnitState { hp: unit.max_hp(), ..unit };
        self.units.push(unit.clone());
        unit
    }

    /// Stats of the player's design for a vehicle class, if one is designed.
    pub fn class_stats(&self, kind: Approach, domain: VehicleDomain) -> Option<crate::civ_combat::VehicleStats> {
        self.vehicle_stats.iter().find(|(a, d, _)| *a == kind && *d == domain).map(|(_, _, s)| *s)
    }

    /// The vehicle tech level now (never below what the cities give; VERIFIED
    /// 0xBF44B0: more than three cities -> 3, the whole planet -> 4).
    pub fn vehicle_tech_level(&self) -> u8 {
        civ_rules::raise_tech_level(self.tech_level, self.owned_count(), CITY_COUNT)
    }

    /// A coastal city: it has a boat spawn point (VERIFIED 0xBDB9D0 reads
    /// `cCity` +0x32C; OpenSpore `harbor`). ADAPTED: the flat fallback (no
    /// planet) has no coast, so every city counts there.
    pub fn is_coastal(&self, index: usize) -> bool {
        self.cities.get(index).is_some_and(|c| c.harbor.is_some()) || !self.shape().on_sphere()
    }

    /// Sea vehicles are available (VERIFIED 0xBDB9D0 / 0xBF03A0: tech level 2
    /// and at least one of the player's cities is coastal).
    pub fn sea_unlocked(&self) -> bool {
        civ_rules::sea_unlocked(self.vehicle_tech_level())
            && self.cities.iter().any(|c| c.owned() && self.is_coastal(c.index))
    }

    /// The ability flags (VERIFIED 0xBF2510, `civ_rules::ability_flags`): the
    /// three consequence abilities of the earlier stages open at 1 / 2 / 4
    /// cities, the superweapon of the capital's type at 6.
    pub fn ability_flags(&self) -> [bool; civ_rules::ABILITY_COUNT] {
        let traits: Vec<usize> = [PowerSlot::Cell, PowerSlot::Creature, PowerSlot::Tribal]
            .into_iter()
            .filter_map(|slot| self.power_index(slot))
            .collect();
        civ_rules::ability_flags(
            &traits,
            self.power_index(PowerSlot::Superweapon),
            self.owned_count(),
            CITY_COUNT,
        )
    }

    /// The ability flag index of a slot's power (`civ_rules::ability_index`).
    pub fn power_index(&self, slot: PowerSlot) -> Option<usize> {
        let id = |name: &str| crate::world_objects::name_id(name);
        let name = match slot {
            PowerSlot::Cell => match self.cell_power? {
                CivCellPower::Invulnerability => "cvg_clg_meat",
                CivCellPower::HealingAura => "cvg_clg_plant",
                CivCellPower::StaticBomb => "cvg_clg_mix",
            },
            PowerSlot::Creature => match self.creature_power? {
                CivCreaturePower::MightyBomb => "cvg_crg_attack",
                CivCreaturePower::DiploDervish => "cvg_crg_social",
                CivCreaturePower::BribeBomb => "cvg_crg_mix",
            },
            PowerSlot::Tribal => match self.tribal_power? {
                CivTribalPower::GadgetBomb => "cvg_trg_attack",
                CivTribalPower::BlackCloud => "cvg_trg_social",
                CivTribalPower::AdBlitz => "cvg_trg_mix",
            },
            PowerSlot::Superweapon => match resolve_superweapon(self.approach) {
                CivSuperweapon::Icbm => "cvg_trg_attackwin",
                CivSuperweapon::FanaticalUprising => "cvg_trg_socialwin",
                CivSuperweapon::GlobalMerger => "cvg_trg_mixwin",
            },
        };
        civ_rules::ability_index(id(name))
    }

    /// The ability's flag is set (the retail `CanUseTool` test, 0xBF8B40).
    pub fn power_unlocked(&self, slot: PowerSlot) -> bool {
        self.power_index(slot).is_some_and(|i| self.ability_flags()[i])
    }

    /// Cities the slot's ability needs (VERIFIED 0xBEFBA0,
    /// `civ_rules::ability_cities`).
    pub fn power_cities(&self, slot: PowerSlot) -> usize {
        self.power_index(slot).map_or(1, |i| civ_rules::ability_cities(i, CITY_COUNT))
    }

    /// The superweapon's flag is set and it was not launched yet (one shot,
    /// `mbLauncedIWinWeapon`).
    pub fn superweapon_unlocked(&self) -> bool {
        self.power_unlocked(PowerSlot::Superweapon) && !self.superweapon_launched
    }

    pub fn tick(&mut self, dt: f32) {
        self.clock += dt;
        for nation in &mut self.nations {
            nation.relation.tick(dt);
        }
        for city in &mut self.cities {
            city.stunned = (city.stunned - dt).max(0.0);
        }
        self.power_cell_cooldown = (self.power_cell_cooldown - dt).max(0.0);
        self.power_creature_cooldown = (self.power_creature_cooldown - dt).max(0.0);
        self.power_tribal_cooldown = (self.power_tribal_cooldown - dt).max(0.0);
        self.superweapon_cooldown = (self.superweapon_cooldown - dt).max(0.0);
        self.invulnerability_timer = (self.invulnerability_timer - dt).max(0.0);
        self.static_bomb_timer = (self.static_bomb_timer - dt).max(0.0);
        self.diplo_dervish_timer = (self.diplo_dervish_timer - dt).max(0.0);
        if let Some((city, mut timer)) = self.black_cloud_city {
            timer -= dt;
            if timer <= 0.0 {
                self.black_cloud_city = None;
            } else {
                self.black_cloud_city = Some((city, timer));
            }
        }
        // Pumps under construction stand after `BUILD_SECONDS` (civ_pump).
        for g in 0..self.geysers.len() {
            if let Some(nation) = self.geysers[g].tick_claim(dt) {
                if nation == HOME_INDEX {
                    self.message = format!(
                        "Spice derrick built: +{:.0} spice/cycle ({} derricks)",
                        self.geysers[g].income(),
                        self.derricks(HOME_INDEX)
                    );
                }
            }
        }
    }


    /// The play style card of the Advance rule (VERIFIED 0xCF82C0): the
    /// capture method used most, falling back to the cities held by type.
    pub fn advance_play_style(&self) -> civ_rules::PlayStyle {
        let held = |kind: Approach| self.cities.iter().filter(|c| c.owned() && c.kind == kind).count();
        civ_rules::play_style(
            self.captures,
            [held(Approach::Military), held(Approach::Religious), held(Approach::Economic)],
        )
    }

    /// The saved selection cursor of the mode's group (0 = nothing picked).
    pub fn cursor(&self) -> u32 {
        match self.editor_mode {
            CivEditorMode::Vehicle => self.vehicle_cursor,
            CivEditorMode::Building => self.building_cursor,
        }
    }

    pub fn set_cursor(&mut self, instance: u32) {
        match self.editor_mode {
            CivEditorMode::Vehicle => self.vehicle_cursor = instance,
            CivEditorMode::Building => self.building_cursor = instance,
        }
    }

    pub fn owned_count(&self) -> usize {
        self.cities.iter().filter(|c| c.owned()).count()
    }

    pub fn rivals_left(&self) -> usize {
        self.cities.len() - self.owned_count()
    }

    pub fn is_unified(&self) -> bool {
        !self.cities.is_empty() && self.owned_count() == self.cities.len()
    }

    /// A rival nation is gone once its city is yours.
    pub fn nation_alive(&self, nation: usize) -> bool {
        nation == HOME_INDEX || (self.cities.get(nation).is_some_and(|c| !c.owned()) && !self.hidden.contains(&nation))
    }

    /// The city is on the map: yours always, a rival's once it has appeared
    /// (`civ_reveal`).
    pub fn present(&self, index: usize) -> bool {
        self.cities.get(index).is_some_and(|c| c.owned() || !self.hidden.contains(&index))
    }

    /// Rival cities still to appear.
    pub fn hidden_count(&self) -> usize {
        (0..self.cities.len()).filter(|i| !self.present(*i)).count()
    }

    /// The reveal order key of a city slot (`reveal_order`, else its index).
    pub fn reveal_key(&self, index: usize) -> u32 {
        self.reveal_order.get(index).copied().unwrap_or(index as u32)
    }

    /// A rival nation's start: its purse is its first budget (VERIFIED
    /// 0xBF7370: the first positive wealth goes wholly to the domestic
    /// budget), so its city's first construction second hands out the free
    /// first vehicle (0xBE5890, `npc_build_second`) and the rest builds.
    /// ADAPTED: the purse itself (`RIVAL_STARTING_SPICE`) is OpenSpore's (the
    /// retail cCivilization constructor 0xBF7A10 starts at 0 and the city
    /// creation 0xB999F0 adds 4 x that); retail gives no opening fleet.
    pub fn rival_opening(&mut self, nation: usize) {
        if let Some(n) = self.nations.get_mut(nation) {
            n.domestic += n.spice;
            n.spice = 0.0;
        }
    }

    /// 0xAE3AB0: the hidden city with the lowest order key appears with its
    /// first budget (`rival_opening`). VERIFIED: a revealed rival owns no derrick
    /// (the city creation 0xB999F0 adds none and no other call site gives an NPC
    /// civilization a node); it claims geysers through 0xBF9960 like any nation
    /// (`civ_rts::rts_ai`), which the retail AI starts as soon as its first
    /// vehicle exists. Returns it and the vehicles for the live scene (none: its
    /// first vehicle comes from its construction second).
    pub fn reveal_next(&mut self) -> Option<(usize, Vec<UnitState>)> {
        let nation = (0..self.cities.len())
            .filter(|i| !self.present(*i))
            .min_by_key(|i| (self.reveal_key(*i), *i))?;
        self.hidden.retain(|h| *h != nation);
        self.rival_opening(nation);
        Some((nation, Vec::new()))
    }

    /// 0xBF2240 > 0 for the city type's vehicles: the nation fields fewer
    /// than 36 vehicles and fewer of `kind` than its cap (0xBF2110).
    pub fn vehicle_room(&self, nation: usize, kind: Approach) -> bool {
        self.fleet(nation) < civ_rules::VEHICLE_CAP_MAX as usize
            && self.fleet_of(nation, kind) < self.vehicle_cap_for(nation, kind)
    }

    /// VERIFIED 0xBF7190 / 0xBF5860: some city of `nation` lies on the same
    /// continent as a city of another nation (cities that have appeared;
    /// `continents` from the layout, `civ_planet::lay_out`). Saves without
    /// continent ids count every city on one continent.
    pub fn shares_continent(&self, nation: usize) -> bool {
        let continent = |i: usize| self.continents.get(i).copied();
        let present: Vec<usize> = (0..self.cities.len()).filter(|i| self.present(*i)).collect();
        present.iter().filter(|i| self.cities[**i].nation() == nation).any(|mine| {
            present
                .iter()
                .filter(|o| self.cities[**o].nation() != nation)
                .any(|other| match (continent(*mine), continent(*other)) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                })
        })
    }

    pub fn at_war(&self, nation: usize) -> bool {
        nation != HOME_INDEX && self.nations.get(nation).is_some_and(|n| n.war)
    }

    pub fn derricks(&self, nation: usize) -> usize {
        self.geysers
            .iter()
            .filter(|g| g.owner == Some(nation))
            .count()
    }

    /// Money every `INCOME_PERIOD` (6 s) for a nation before the NPC scaling:
    /// 0.1 x (its cities' incomes + its derricks, VERIFIED 0xBF70E0). Trade
    /// routes pay per delivery of an economic vehicle (0xBE19B0, `civ_rts`),
    /// not here.
    pub fn income_for(&self, nation: usize) -> f32 {
        let cloud = self.black_cloud_city.filter(|(_, t)| *t > 0.0).map(|(c, _)| c);
        let cities: f32 = self
            .cities
            .iter()
            .filter(|c| c.nation() == nation)
            .map(|c| c.income_with(cloud == Some(c.index)))
            .sum();
        let derricks: f32 = self
            .geysers
            .iter()
            .filter(|g| g.owner == Some(nation))
            .map(GeyserState::income)
            .sum();
        cities + derricks
    }

    /// Spice every cycle from the capital, every city taken so far, derricks
    /// and trade routes.
    pub fn income_per_cycle(&self) -> f32 {
        self.income_for(HOME_INDEX)
    }

    /// Vehicles of one type a nation may field (VERIFIED 0xBF2110): 3 for the
    /// capital's type, plus per city of that type 2 and its housing (NPC houses
    /// capped at 6), at most 36.
    pub fn vehicle_cap_for(&self, nation: usize, kind: Approach) -> usize {
        let capital = self
            .cities
            .get(nation)
            .map(|c| c.kind)
            .unwrap_or_default();
        let cities: Vec<civ_rules::CapCity> = self
            .cities
            .iter()
            .filter(|c| c.nation() == nation && c.kind == kind)
            .map(|c| civ_rules::CapCity {
                houses: c.housing(),
                minor: false,
            })
            .collect();
        civ_rules::vehicle_cap(capital == kind, &cities, nation == HOME_INDEX) as usize
    }

    /// Every type's cap together (the HUD's fleet limit).
    pub fn vehicle_cap(&self, nation: usize) -> usize {
        [Approach::Military, Approach::Religious, Approach::Economic]
            .into_iter()
            .map(|kind| self.vehicle_cap_for(nation, kind))
            .sum()
    }

    pub fn fleet_of(&self, nation: usize, kind: Approach) -> usize {
        self.units
            .iter()
            .filter(|u| u.nation == nation && u.kind == kind)
            .count()
    }

    pub fn fleet(&self, nation: usize) -> usize {
        self.units.iter().filter(|u| u.nation == nation).count()
    }

    /// Tech level above 2 (`CVG_UnlockAir`).
    pub fn air_unlocked(&self) -> bool {
        civ_rules::air_unlocked(self.vehicle_tech_level())
    }

    pub fn nearest_rival(&self, from: Vec3, range: f32) -> Option<usize> {
        self.nearest(from, range, false)
    }

    pub fn nearest_owned(&self, from: Vec3, range: f32) -> Option<usize> {
        self.nearest(from, range, true)
    }

    fn nearest(&self, from: Vec3, range: f32, owned: bool) -> Option<usize> {
        let shape = self.shape();
        self.cities
            .iter()
            .filter(|c| c.owned() == owned && self.present(c.index))
            .map(|c| (c.index, shape.distance(from, c.center())))
            .filter(|(_, d)| *d <= range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }

    /// Hand a fallen rival city to the player: reward, derricks, the capture
    /// counter of the method (play style) and the tech level, message.
    pub fn take_city(&mut self, index: usize, how: Conquest, spice: &mut f32) -> String {
        let name = city_label(index);
        let weapon_before = self.superweapon_unlocked();
        // (The superweapon takes the cities still hidden too.)
        self.hidden.retain(|h| *h != index);
        let paid = capture_city(&mut self.cities[index], spice);
        // 0xBE9AD0 / 0xBEAB80: every citizen goes to the new owner.
        self.cities[index].reset_allegiance();
        for geyser in &mut self.geysers {
            if geyser.owner == Some(index) {
                geyser.owner = Some(HOME_INDEX);
            }
        }
        if let Some(nation) = self.nations.get_mut(index) {
            nation.war = false;
        }
        match how {
            Conquest::Military => self.captures.military += 1,
            Conquest::Religious => self.captures.religious += 1,
            Conquest::Economic => self.captures.economic += 1,
        }
        let before = self.tech_level;
        self.tech_level = civ_rules::raise_tech_level(self.tech_level, self.owned_count(), CITY_COUNT);
        let how = match how {
            Conquest::Military => "falls to your army",
            Conquest::Religious => "converts to your faith",
            Conquest::Economic => "is bought outright",
        };
        let mut line = if paid && CAPTURE_REWARD > 0.0 {
            format!("{name} {how} (+{CAPTURE_REWARD:.0} spice)")
        } else {
            format!("{name} {how}")
        };
        if !civ_rules::air_unlocked(before) && civ_rules::air_unlocked(self.tech_level) {
            // The retail announcement is `CVG_UnlockAir` after the capture
            // cinematic (0xCFA220, `cinematics::moments`); this line is the
            // message-bar twin.
            line.push_str(" - aircraft unlocked (H)");
        }
        if !weapon_before && self.superweapon_unlocked() {
            line.push_str(" - superweapon available (0)");
        }
        line
    }

    /// The rival city an ability aims at: the one under the camera, else the
    /// nearest to `at`. ADAPTED: the original targets with the cursor.
    pub fn power_target(&self, at: Vec3) -> Option<usize> {
        self.last_target
            .filter(|c| self.cities.get(*c).is_some_and(|city| !city.owned()))
            .or_else(|| {
                self.cities
                    .iter()
                    .filter(|c| !c.owned())
                    .min_by(|a, b| {
                        self.distance(at, a.center())
                            .total_cmp(&self.distance(at, b.center()))
                    })
                    .map(|c| c.index)
            })
    }

    /// An offensive ability hit a nation: it counts as a hit on its buildings
    /// (`Relationship::attacked`, the Hostility event) and the nation is at war
    /// once that makes it Annoyed or worse (as a vehicle attack does in `civ_rts`).
    pub fn bombed(&mut self, nation: usize) {
        if nation == HOME_INDEX || !self.nation_alive(nation) {
            return;
        }
        if let Some(state) = self.nations.get_mut(nation) {
            state.relation.attacked();
            if state.relation.hostile() {
                state.war = true;
            }
        }
    }

    /// Use the ability in `slot` at `at` (the camera focus), paying its cost
    /// from `money` (VERIFIED costs 0xBF6DF0, tool values `spacetools~`).
    pub fn use_power(&mut self, slot: PowerSlot, money: &mut f32, at: Vec3) -> Result<PowerUse, String> {
        self.use_power_at(slot, money, at, PowerAim::Camera)
    }

    /// `use_power` with the aim: a click names the target city itself (the retail click handler
    /// 0xCF688E passes the clicked ground point to `cCivilization::UseTool` 0xBF8BB0).
    pub fn use_power_at(
        &mut self,
        slot: PowerSlot,
        money: &mut f32,
        at: Vec3,
        aim: PowerAim,
    ) -> Result<PowerUse, String> {
        self.ensure_cities();
        // VERIFIED (0xBF8B40 `CanUseTool`): the ability's flag must be set,
        // i.e. the civilization holds the cities its stage asks for
        // (`civ_rules::ABILITY_CITIES`).
        if slot != PowerSlot::Superweapon && self.power_index(slot).is_some() && !self.power_unlocked(slot) {
            let name = match slot {
                PowerSlot::Cell => self.cell_power.map(|p| p.name()),
                PowerSlot::Creature => self.creature_power.map(|p| p.name()),
                _ => self.tribal_power.map(|p| p.name()),
            };
            return Err(format!(
                "{} locked: it needs {} cities (you hold {})",
                name.unwrap_or("Ability"),
                self.power_cities(slot),
                self.owned_count()
            ));
        }
        let (data, cooldown, cost) = match slot {
            PowerSlot::Cell => {
                let p = self.cell_power.ok_or("No Cell consequence ability")?;
                (p.data(), self.power_cell_cooldown, p.cost())
            }
            PowerSlot::Creature => {
                let p = self.creature_power.ok_or("No Creature consequence ability")?;
                (p.data(), self.power_creature_cooldown, p.cost())
            }
            PowerSlot::Tribal => {
                let p = self.tribal_power.ok_or("No Tribal consequence ability")?;
                (p.data(), self.power_tribal_cooldown, p.cost())
            }
            PowerSlot::Superweapon => {
                let w = resolve_superweapon(self.approach);
                if self.superweapon_launched {
                    return Err(format!("{} was already launched", w.name()));
                }
                if !self.superweapon_unlocked() {
                    return Err(format!(
                        "{} locked: it needs {} cities (you hold {})",
                        w.name(),
                        self.power_cities(PowerSlot::Superweapon),
                        self.owned_count()
                    ));
                }
                (w.data(), self.superweapon_cooldown, w.cost(self.rivals_left()))
            }
        };
        if cooldown > 0.0 {
            return Err(format!("{} recharging ({:.0}s)", data.name, cooldown));
        }
        if *money < cost {
            return Err(format!("{} costs {cost:.0} spice (have {:.0})", data.name, *money));
        }
        let target = match aim {
            PowerAim::Camera => self.power_target(at),
            PowerAim::Click { city } => city.filter(|c| self.cities.get(*c).is_some_and(|city| !city.owned())),
        };
        let needs_target = matches!(data.tool, "diplodervish" | "tacnuke" | "blackcloud" | "adblitz")
            || slot == PowerSlot::Superweapon;
        // A clicked Gadget Bomb may fall on open ground (retail: an area blast at the click).
        let open_ground = data.tool == "tacnuke" && matches!(aim, PowerAim::Click { city: None });
        if needs_target && target.is_none() && !open_ground {
            return Err("No rival city is left to target".into());
        }
        let mut effect = None;
        let mut point = at;
        let message = match data.tool {
            "i_field" => {
                self.invulnerability_timer = data.min_damage;
                format!(
                    "{}: your vehicles are impervious to damage for {:.0}s",
                    data.name, data.min_damage
                )
            }
            "faithheal" => {
                // ADAPTED: the low end of the tool's 150..300 repairs.
                let heal = data.min_damage;
                for city in self.cities.iter_mut().filter(|c| c.owned()) {
                    city.repair(heal);
                }
                for unit in self.units.iter_mut().filter(|u| u.nation == HOME_INDEX) {
                    unit.hp = (unit.hp + heal).min(unit.max_hp());
                }
                effect = Some(UnitEffect::Heal(heal));
                format!("{}: your vehicles and buildings are repaired", data.name)
            }
            "emp" => {
                let radius = data.radius();
                let mut hit = Vec::new();
                let shape = self.shape();
                for city in self.cities.iter_mut().filter(|c| !c.owned()) {
                    if shape.distance(at, city.center()) <= radius + 6.4 * CITY_SCALE {
                        city.stunned = data.min_damage;
                        hit.push(city.index);
                    }
                }
                for nation in hit {
                    self.bombed(nation);
                }
                self.static_bomb_timer = data.min_damage;
                effect = Some(UnitEffect::Stun {
                    at,
                    radius,
                    seconds: data.min_damage,
                });
                format!(
                    "{}: vehicles, turrets and buildings nearby are immobilized for {:.0}s",
                    data.name, data.min_damage
                )
            }
            "napalm" => {
                let radius = data.radius();
                let mut hit = Vec::new();
                let shape = self.shape();
                for city in self.cities.iter_mut().filter(|c| !c.owned()) {
                    if shape.distance(at, city.center()) <= radius + 6.4 * CITY_SCALE {
                        // Every building and turret in the blast takes the
                        // damage (ADAPTED: the whole city; the hall is
                        // spared, vehicles take the city).
                        city.blast(data.min_damage, HOME_INDEX);
                        hit.push(city.index);
                    }
                }
                for nation in hit {
                    self.bombed(nation);
                }
                effect = Some(UnitEffect::Damage {
                    at,
                    radius,
                    amount: data.min_damage,
                });
                format!("{}: everything in the blast is damaged", data.name)
            }
            "espionage" => {
                effect = Some(UnitEffect::Bribe {
                    at,
                    radius: data.radius(),
                    seconds: data.min_damage,
                });
                format!(
                    "{}: enemy vehicles nearby turn on each other for {:.0}s",
                    data.name, data.min_damage
                )
            }
            "diplodervish" => {
                let c = target.unwrap_or_default();
                point = self.cities[c].center();
                let nation = &mut self.nations[c];
                nation.relation.apply(civ_rules::RelationKind::Dervish);
                let peace = nation.war && nation.relation.peaceful();
                if peace {
                    nation.war = false;
                }
                format!(
                    "{}: {} warms to you (relationship {:+.1}){}",
                    data.name,
                    city_label(c),
                    nation.relation.value(),
                    if peace { ", the war is over" } else { "" }
                )
            }
            "tacnuke" if target.is_none() => {
                // ADAPTED: the blast lands on open ground; only vehicles in its radius feel it.
                effect = Some(UnitEffect::Damage {
                    at,
                    radius: data.radius(),
                    amount: data.min_damage,
                });
                format!("{} falls on open ground: nothing is left to take", data.name)
            }
            "tacnuke" => {
                let c = target.unwrap_or_default();
                point = self.cities[c].center();
                let radius = data.radius();
                let city = &mut self.cities[c];
                city.blast(data.min_damage, HOME_INDEX);
                for t in 0..city.turret_hp.len() {
                    city.damage_turret(t, TURRET_HP);
                }
                self.bombed(c);
                effect = Some(UnitEffect::Damage {
                    at: point,
                    radius,
                    amount: data.min_damage,
                });
                format!(
                    "{} on {}: its defences are wrecked - send vehicles to take it",
                    data.name,
                    city_label(c)
                )
            }
            "blackcloud" => {
                let c = target.unwrap_or_default();
                point = self.cities[c].center();
                self.black_cloud_city = Some((c, data.min_damage));
                self.bombed(c);
                format!(
                    "{} over {}: entertainment and turrets shut down for {:.0}s",
                    data.name,
                    city_label(c),
                    data.min_damage
                )
            }
            "adblitz" => {
                let c = target.unwrap_or_default();
                point = self.cities[c].center();
                let city = &mut self.cities[c];
                // ADAPTED: opens trade and fills the trade value the price
                // formula subtracts (the most it can, 4000).
                city.trade = true;
                city.trade_value = civ_rules::TRADE_VALUE_MAX;
                format!(
                    "{} in {}: buyout price down to {:.0}",
                    data.name,
                    city_label(c),
                    city.buy_price()
                )
            }
            _ => {
                // Superweapons take every remaining city.
                let how = match resolve_superweapon(self.approach) {
                    CivSuperweapon::Icbm => Conquest::Military,
                    CivSuperweapon::FanaticalUprising => Conquest::Religious,
                    CivSuperweapon::GlobalMerger => Conquest::Economic,
                };
                let rivals: Vec<usize> = self
                    .cities
                    .iter()
                    .filter(|c| !c.owned())
                    .map(|c| c.index)
                    .collect();
                *money -= cost;
                for c in &rivals {
                    self.take_city(*c, how, money);
                }
                self.superweapon_launched = true;
                return Ok(PowerUse {
                    message: format!("{}: {} cities change hands", data.name, rivals.len()),
                    effect: None,
                    at: point,
                    tool: data.tool,
                });
            }
        };
        *money -= cost;
        match slot {
            PowerSlot::Cell => self.power_cell_cooldown = data.recharge,
            PowerSlot::Creature => self.power_creature_cooldown = data.recharge,
            PowerSlot::Tribal => self.power_tribal_cooldown = data.recharge,
            PowerSlot::Superweapon => self.superweapon_cooldown = data.recharge,
        }
        Ok(PowerUse {
            message,
            effect,
            at: point,
            tool: data.tool,
        })
    }

    /// The civ objective, rendered by `main::hud_text`.
    pub fn hud(&self) -> String {
        if self.cities.is_empty() {
            return format!(
                "CIVILIZATION | {} doctrine | no cities yet",
                self.approach.title()
            );
        }
        let owned = self.owned_count();
        let total = self.cities.len();
        let goal = if self.is_unified() {
            "planet unified - launch to Space".to_string()
        } else {
            format!("take the other {} cities ({owned}/{total})", total - owned)
        };
        let income = self.income_per_cycle();
        let losses = if self.vehicles_lost > 0 {
            format!(" | lost {}", self.vehicles_lost)
        } else {
            String::new()
        };
        let target = self
            .last_target
            .and_then(|i| self.cities.get(i))
            .map(|c| format!("\n{}", city_status(c)))
            .unwrap_or_default();
        let powers = if self.cell_power.is_some() || self.creature_power.is_some() || self.tribal_power.is_some() {
            let cp_str = self
                .cell_power
                .map(|p| {
                    if self.power_cell_cooldown > 0.0 {
                        format!("{}({:.0}s)", p.name(), self.power_cell_cooldown)
                    } else {
                        format!("{}[7] {:.0}", p.name(), p.cost())
                    }
                })
                .unwrap_or_default();
            let crp_str = self
                .creature_power
                .map(|p| {
                    if self.power_creature_cooldown > 0.0 {
                        format!("{}({:.0}s)", p.name(), self.power_creature_cooldown)
                    } else {
                        format!("{}[8] {:.0}", p.name(), p.cost())
                    }
                })
                .unwrap_or_default();
            let tp_str = self
                .tribal_power
                .map(|p| {
                    if self.power_tribal_cooldown > 0.0 {
                        format!("{}({:.0}s)", p.name(), self.power_tribal_cooldown)
                    } else {
                        format!("{}[9] {:.0}", p.name(), p.cost())
                    }
                })
                .unwrap_or_default();
            let sw = resolve_superweapon(self.approach);
            let sw_str = if !self.superweapon_unlocked() {
                format!("{}(locked)", sw.name())
            } else if self.superweapon_cooldown > 0.0 {
                format!("{}({:.0}s)", sw.name(), self.superweapon_cooldown)
            } else {
                format!("{}[0] {:.0}", sw.name(), sw.cost(self.rivals_left()))
            };
            format!("\nPowers: {cp_str} | {crp_str} | {tp_str} | {sw_str}")
        } else {
            String::new()
        };
        format!(
            "CIVILIZATION | {} doctrine | {goal} | income {income:.0}/{}s | fleet {}/{} | derricks {}{losses}\n{}{}{}",
            self.approach.title(),
            INCOME_PERIOD,
            self.fleet(HOME_INDEX),
            self.vehicle_cap(HOME_INDEX),
            self.derricks(HOME_INDEX),
            self.message,
            target,
            powers,
        )
    }
}


/// Material handles for the ownership tints; the shared map reuses them so a
/// city's dot on the world map matches its owner, and one per nation colour.
#[derive(Resource)]
pub struct CivTints {
    pub player: Handle<StandardMaterial>,
    pub rival: Handle<StandardMaterial>,
    pub nations: Vec<Handle<StandardMaterial>>,
}

impl FromWorld for CivTints {
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        Self {
            player: materials.add(city_material(CityOwner::Player)),
            rival: materials.add(city_material(CityOwner::Rival)),
            nations: (0..CITY_COUNT)
                .map(|n| materials.add(nation_material(n)))
                .collect(),
        }
    }
}

/// Marks the owner tint a city entity is currently showing, so the update only
/// swaps materials when a city actually changes hands.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityVisual {
    pub owner: CityOwner,
}

/// Stable city identity, read by `navigation::update_map` as `C1`..`C5`.
/// The index is the city's slot in `CivCampaign::cities`, forever.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CityMarker(pub usize);

/// The ownership indicator for one city: the flagpole on the plaza, carrying
/// the owner's tint; its banner (a child) carries the nation colour.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CityOwnershipBeacon(pub usize);

/// The nation-coloured banner flying from a city's flagpole.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityBanner(pub usize);

/// Drawn baked hall, selected by the native building ModelType metadata.
#[derive(Component, Clone, Copy)]
pub struct NativeCityHallSurface(pub crate::dbpf::ResourceKey);

/// The ownership tint of a city, also reused for its dot on the world map.
pub fn city_material(owner: CityOwner) -> StandardMaterial {
    let (base, glow) = if owner == CityOwner::Player {
        (Color::srgb(0.25, 0.75, 0.55), Color::srgb(0.02, 0.12, 0.07))
    } else {
        (Color::srgb(0.8, 0.22, 0.18), Color::srgb(0.16, 0.02, 0.02))
    };
    StandardMaterial {
        base_color: base,
        emissive: glow.into(),
        perceptual_roughness: 0.8,
        ..default()
    }
}

/// A nation's banner colour as a lit, slightly glowing material.
pub fn nation_material(nation: usize) -> StandardMaterial {
    let color = nation_color(nation);
    StandardMaterial {
        base_color: color,
        emissive: (color.to_linear() * 0.25).into(),
        perceptual_roughness: 0.7,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// One city: a root tower carrying the marker, with the roof as a child so it
/// rides along, and a flagpole whose banner flies the nation colour. Returns
/// root, roof and pole, so a mounted prefab can retire the primitive pair.
fn spawn_city(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tints: &CivTints,
    city: &CityState,
    shape: Shape,
    tower: Handle<Mesh>,
    roof: Handle<Mesh>,
) -> (Entity, Entity, Entity) {
    // The city's layout is authored in island units around its root: the
    // root stands on the local up and draws it `CITY_SCALE` times larger.
    let center = city.center();
    let entity = commands
        .spawn((
            StageOwned,
            CityMarker(city.index),
            CityVisual { owner: city.owner },
            Mesh3d(tower),
            MeshMaterial3d(tints.nations[city.nation()].clone()),
            Transform::from_translation(shape.place_on(center, CITY_ROOT_LIFT))
                .with_rotation(shape.frame(center))
                .with_scale(Vec3::splat(CITY_SCALE)),
            crate::terrain::Radial,
            crate::terrain::Grounded::new(CITY_ROOT_LIFT),
        ))
        .id();
    // The children's own entities, not the parent's: `with_children` returns
    // the parent's commands, so the ids have to come from inside the closure.
    let mut roof_entity = None;
    let mut beacon_entity = None;
    let floor = -CITY_ROOT_LIFT / CITY_SCALE;
    commands.entity(entity).with_children(|hall| {
        roof_entity = Some(
            hall.spawn((
                StageOwned,
                Mesh3d(roof),
                Transform::from_xyz(0.0, 2.85, 0.0),
            ))
            .id(),
        );
        let pole = hall
            .spawn((
                StageOwned,
                CityOwnershipBeacon(city.index),
                Mesh3d(meshes.add(Cylinder::new(0.07, 4.2))),
                MeshMaterial3d(materials.add(city_material(city.owner))),
                Transform::from_xyz(-2.0, floor + 2.1, 2.0),
            ))
            .id();
        beacon_entity = Some(pole);
    });
    let pole = beacon_entity.expect("spawn_city spawns exactly one flagpole");
    commands.spawn((
        StageOwned,
        CityBanner(city.index),
        Mesh3d(meshes.add(Cuboid::new(1.5, 0.9, 0.04))),
        MeshMaterial3d(tints.nations[city.nation()].clone()),
        Transform::from_xyz(0.78, 1.55, 0.0),
        ChildOf(pole),
    ));
    (
        entity,
        roof_entity.expect("spawn_city spawns exactly one roof child"),
        pole,
    )
}

/// Native creature draw surfaces on the civic pedestal.
#[derive(Component)]
pub struct NativeCivicCreatureSurface(pub crate::dbpf::ResourceKey);

/// The invisible camera focus that WASD pans. It carries `Player` (so the
/// shared camera, map and checkpoints follow it) but never fights.
#[derive(Component)]
pub struct CivCommander;

pub fn setup_civ(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    tints: Res<CivTints>,
    cameras: Query<Entity, With<crate::GameCamera>>,
    planet_cache: Option<Res<crate::terrain::PlanetCache>>,
) {
    run.enter_stage(GameStage::Civilization);
    // The planet first (planet-re 4E step 6): the homeworld cached for
    // `terrain::prepare`, and the campaign laid out on it once.
    let seed = run.species_world.seed;
    let generated = crate::creature_planet::ensure_site_cache(
        &originals.resource_index,
        planet_cache.as_deref(),
        seed,
    );
    let cache = generated
        .as_ref()
        .or(planet_cache.as_deref().filter(|c| c.seed == seed));
    let site = cache.map(|c| std::sync::Arc::new(crate::civ_planet::site_planet(c)));
    {
        let globe = site
            .as_deref()
            .map_or_else(crate::civ_planet::Globe::flat, crate::civ_planet::Globe::on);
        if crate::civ_planet::lay_out(&mut run.civ_campaign, &globe, seed) {
            // A checkpoint of this stage holds the old layout's focus and geysers.
            if run
                .checkpoint
                .as_ref()
                .is_some_and(|c| c.stage == run.stage_order_index)
            {
                run.checkpoint = None;
            }
            let shape = globe.shape();
            info!(
                "civ: cities laid out on the {} (capital {:.0} from the origin, rivals {})",
                if globe.on_planet() { "planet" } else { "plane" },
                shape.distance(Vec3::ZERO, run.civ_campaign.cities[HOME_INDEX].center()),
                run.civ_campaign.cities[1..]
                    .iter()
                    .map(|c| format!("{:.0}", shape.distance(Vec3::ZERO, c.center())))
                    .collect::<Vec<_>>()
                    .join("/")
            );
            run.request_save();
        }
    }
    if let Some(cache) = generated {
        commands.insert_resource(cache);
    }
    commands.insert_resource(crate::civ_planet::CivSite(site.clone()));
    let shape = run.civ_campaign.shape();
    clear.0 = GameStage::Civilization.accent();
    commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(Cuboid::new(60.0, 1.0, 60.0))),
        crate::terrain::WorldGround,
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.22, 0.25),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -1.1, 0.0),
    ));
    // Cities: your capital plus the rivals, at deterministic positions so a
    // reload puts every city back under the same owner and map label.
    // The capital's type follows the Tribal stage's city-hall card.
    if !run.civ_campaign.started {
        if let Some(card) = run.tribal_campaign.city_hall_card {
            // The card the Tribal stage earned at the Evolve click from the
            // final relationships with the five tribes (VERIFIED 0xCDBA30 /
            // 0xCDBBD0, `tribal_rules::play_style_from_relations`).
            run.civ_campaign.approach = match card {
                crate::tribal_rules::PlayStyle::Military => Approach::Military,
                crate::tribal_rules::PlayStyle::Religious => Approach::Religious,
                crate::tribal_rules::PlayStyle::Economic => Approach::Economic,
            };
        } else {
            // A tribe saved before the card was recorded: the totem carvings.
            let carvings = &run.tribal_campaign.totem_carvings;
            let conquered = carvings
                .iter()
                .filter(|c| **c == crate::tribal::TotemSegment::Conquered)
                .count();
            let allied = carvings
                .iter()
                .filter(|c| **c == crate::tribal::TotemSegment::Allied)
                .count();
            if let Some(card) = civ_rules::tribal_card(conquered, allied) {
                run.civ_campaign.approach = match card {
                    civ_rules::TribalCard::Attack => Approach::Military,
                    civ_rules::TribalCard::Social => Approach::Religious,
                    civ_rules::TribalCard::Mix => Approach::Economic,
                };
            }
        }
    }
    // A new Civ stage starts with the player's city alone; the rivals'
    // cities appear over time (VERIFIED 0xB9D970 / 0xAE2EF0, `civ_reveal`).
    if !run.civ_campaign.started && crate::civ_reveal::enabled() {
        run.civ_campaign.hidden = crate::civ_reveal::fresh_hidden();
        run.civ_campaign.reveal_timer = None;
    }
    let mut spice = run.spice;
    let fresh = run.civ_campaign.begin(&mut spice);
    run.spice = spice;
    if fresh {
        run.civ_campaign.message = run.civ_campaign.approach.hint();
        run.request_save();
    }
    let approach = run.civ_campaign.approach;
    run.civ_campaign.cities[HOME_INDEX].kind = approach;
    run.civ_campaign.cities[HOME_INDEX].ensure_layout();
    let tower = meshes.add(Cuboid::new(4.0, 5.0, 4.0));
    let roof = meshes.add(Cuboid::new(5.4, 0.7, 5.4));
    let cities: Vec<CityState> = run.civ_campaign.cities.clone();
    for city in &cities {
        let (entity, roof, _pole) = spawn_city(
            &mut commands,
            &mut meshes,
            &mut materials,
            &tints,
            city,
            shape,
            tower.clone(),
            roof.clone(),
        );
        // Default original visual: the native city hall replaces the
        // primitive tower and roof when the install provides it. The marker,
        // the nation material and the flag stay, so tints, capture and the
        // economy are untouched.
        mount_default_hall(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            entity,
            roof,
            &city.label(),
        );
    }
    // Original parts are imported on demand, so a fresh process has to import
    // every saved part before it can mount one. A saved instance that no
    // longer imports stays in the save and is reported, never deleted.
    let mut missing: Vec<u32> = Vec::new();
    for p in run.vehicle.parts.clone() {
        import_saved(
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            asset_catalog::VEHICLE_PARTS,
            p.instance,
            &mut missing,
        );
    }
    for p in run.building.parts.clone() {
        import_saved(
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            asset_catalog::BUILDING_PARTS,
            p.instance,
            &mut missing,
        );
    }
    if !missing.is_empty() {
        info!(
            "civ keeps {} saved part(s) that do not import here: {}",
            missing.len(),
            id_list(&missing, 5)
        );
    }
    if run.civ_campaign.message.is_empty() {
        run.civ_campaign.message = approach.hint();
    }
    info!(
        "civ opens with {} cities and {} vehicles",
        cities.len(),
        run.civ_campaign.units.len()
    );
    spawn_civic_statue(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut originals,
        &run,
    );
    // The camera focus: just in front of the capital, like Spore's opening.
    let stats = run.creature_stats();
    commands.insert_resource(crate::creature::CreatureStatsRes(stats));
    let capital = run.civ_campaign.cities[HOME_INDEX].center();
    let focus = shape.toward(
        capital,
        shape.bearing(capital, std::f32::consts::FRAC_PI_2),
        4.0 * CITY_SCALE,
    );
    commands.spawn((
        StageOwned,
        Player,
        CivCommander,
        Name::new("civilization camera focus"),
        Mover { speed: PAN_SPEED },
        Health::new(100.0),
        Transform::from_translation(shape.place_on(focus, 0.6)).with_rotation(shape.frame(focus)),
        crate::terrain::Radial,
        crate::terrain::Grounded::new(0.6),
        Visibility::Hidden,
    ));
    // Spice geysers: their derricks live in the campaign data.
    for i in 0..GEYSER_COUNT {
        let p = run.civ_campaign.geyser_at(i);
        commands.spawn((
            StageOwned,
            SpiceNode { spice_left: 120.0 },
            crate::persistence::WorldId(100 + i as u32),
            Mesh3d(meshes.add(Cylinder::new(0.8, 2.4))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.2, 0.5),
                emissive: Color::srgb(0.4, 0.05, 0.2).into(),
                ..default()
            })),
            Transform::from_translation(shape.place_on(p, 0.8 * CITY_SCALE))
                .with_rotation(shape.frame(p))
                .with_scale(Vec3::splat(CITY_SCALE)),
            crate::terrain::Radial,
            crate::terrain::Grounded::new(0.8 * CITY_SCALE),
        ));
    }
    // The plants of the Civ planet are the retail terrain flora: `planet_flora` places the theme's
    // nine plants from the terrainDistribute groves (effect 219 -> 218 -> 215 / 214, the block of
    // kAppFlagCivGameMode = bit 0xB = 0x800, whose far medium sprites only Civilization and the Space
    // planet view draw), keeps them off the cities and geysers (`planet_flora::clearings`) and
    // replaces this stage's old scatter (`replaces_scatter`: `plants` = 0 here, notes\civ-rows3-notes.md 4).
    // What this call still lays are the island's 14 small rocks, ADAPTED: spread over the cities'
    // hemisphere on land, off the cities and geysers (the retail rock placement was not found).
    {
        let globe = site
            .as_deref()
            .map_or_else(crate::civ_planet::Globe::flat, crate::civ_planet::Globe::on);
        let campaign = &run.civ_campaign;
        let keep = |p: Vec3| {
            let at = match globe.planet() {
                Some(sp) => Vec3::new(p.x, sp.surface_y(p.x, p.z), p.z),
                None => p,
            };
            globe.is_land(at)
                && campaign
                    .cities
                    .iter()
                    .all(|c| shape.distance(at, c.center()) > 12.0 * CITY_SCALE)
                && (0..GEYSER_COUNT).all(|g| shape.distance(at, campaign.geyser_at(g)) > 4.0 * CITY_SCALE)
        };
        crate::baked_model::scenery_where(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            run.species_world.seed,
            RETAIL_FROM_ISLAND,
            &keep,
        );
    }
    crate::civ_rts::setup_rts(
        &mut commands,
        &mut meshes,
        &mut materials,
        &run,
        cameras.iter().next(),
    );
}

/// Your creature as a statue on the capital's plaza: the built creature stays
/// visible in civ (and space), like the original's citizens.
fn spawn_civic_statue(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    originals: &mut Originals,
    run: &RunProgress,
) {
    // On the capital's plaza (city-local units, drawn at the city's scale).
    let shape = run.civ_campaign.shape();
    let capital = run.civ_campaign.cities[HOME_INDEX].center();
    let at = shape.local_point(capital, Vec3::new(-4.6, 0.0, 8.2) * CITY_SCALE);
    let frame = shape.frame(at);
    commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(Cylinder::new(1.0, 0.7))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.65),
            ..default()
        })),
        Transform::from_translation(shape.place_on(at, 0.95 * CITY_SCALE))
            .with_rotation(frame)
            .with_scale(Vec3::splat(CITY_SCALE)),
        crate::terrain::Radial,
        crate::terrain::Grounded::new(0.95 * CITY_SCALE),
    ));
    let critter_scale = 0.55;
    let native = (|| {
        let key = crate::baked_model::player_source_key(originals)?;
        let complete = run.body == crate::body::BodyProfile::default()
            && crate::editor::is_starter_creature_parts(&run.parts, &originals.part_catalog);
        let model = if complete {
            crate::baked_model::ensure(originals, key, meshes, images, materials).ok()?
        } else {
            crate::baked_model::prepare_body(originals, &run.body, meshes, images, materials)
                .ok()?
        };
        let fit = if complete {
            crate::baked_model::player_fitted_transform(&model, meshes).ok()?
        } else {
            Transform::IDENTITY
        };
        Some((key, model, fit, complete))
    })();
    let critter = commands
        .spawn((
            StageOwned,
            Visibility::Inherited,
            // On the pedestal, standing up, facing the camera.
            Transform::from_translation(shape.place_on(at, (1.3 + 1.3 * critter_scale) * CITY_SCALE))
                .with_rotation(frame)
                .with_scale(Vec3::splat(critter_scale * CITY_SCALE)),
            crate::terrain::Radial,
            crate::terrain::Grounded::new((1.3 + 1.3 * critter_scale) * CITY_SCALE),
        ))
        .id();
    let complete = if let Some((key, model, fit, complete)) = native {
        for surface in &model.surfaces {
            commands.spawn((
                StageOwned,
                NativeCivicCreatureSurface(key),
                crate::baked_model::source(key),
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(surface.material.clone()),
                fit,
                ChildOf(critter),
            ));
        }
        complete
    } else {
        commands.entity(critter).insert((
            Mesh3d(meshes.add(crate::body::mesh(&run.body))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.6, 0.4),
                ..default()
            })),
        ));
        false
    };
    if !complete {
        crate::creature::mount_saved_parts(
            commands,
            materials,
            critter,
            originals,
            run,
            0,
            critter_scale,
        );
    }
}

/// Mount the native city hall under `parent` and retire the primitive tower
/// and roof. Without the install the primitives stay, so the no-install path
/// remains playable.
#[allow(clippy::too_many_arguments)]
fn mount_default_hall(
    commands: &mut Commands,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    parent: Entity,
    primitive_roof: Entity,
    label: &str,
) {
    let key = crate::city_presentation::CITY_DISTRICT_CITY_HALL_SOURCE;
    let native =
        crate::baked_model::ensure(originals, key, meshes, images, materials).and_then(|model| {
            let transform =
                crate::city_presentation::fit_central_city_hall(&model, meshes, None, None)?;
            if model.surfaces.is_empty() {
                return Err("native hall has no surfaces".into());
            }
            Ok((model, transform))
        });
    match native {
        Ok((model, transform)) => {
            for surface in &model.surfaces {
                commands.spawn((
                    StageOwned,
                    NativeCityHallSurface(key),
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                    ChildOf(parent),
                ));
            }
            commands.entity(parent).remove::<Mesh3d>();
            commands.entity(primitive_roof).despawn();
        }
        Err(err) => info!("{label} keeps its primitive hall: {err}"),
    }
}


/// Spawn one child entity per saved vehicle part, placed by its slot.
/// A mounted part of the vehicle build (child of a vehicle).
#[derive(Component)]
pub struct VehiclePartRef {
    pub part: usize,
}

/// A mounted part of the city skyline (child of a city root).
#[derive(Component)]
pub struct CityPartRef {
    pub city: usize,
    pub part: usize,
}

/// What a city root is currently showing, so the skyline is rebuilt only when
/// ownership or the saved build actually changed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityAssemblyVisual {
    pub owner: CityOwner,
    pub signature: u32,
}

/// Cheap deterministic identity of a build. Order matters: moving a part to
/// another slot or turning it is a different skyline.
pub fn build_signature(build: &VehicleBuild) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let mut mix = |bits: u32| {
        hash ^= bits;
        hash = hash.wrapping_mul(16777619);
    };
    mix(build.parts.len() as u32);
    mix(build.focused);
    for p in &build.parts {
        mix(p.instance);
        mix(p.slot);
        mix(p.yaw.to_bits());
        mix(p.scale.to_bits());
        for v in p.offset {
            mix(v.to_bits());
        }
        mix(crate::rigid_editor::block_signature(&p.block));
    }
    hash
}

/// Saved instances that no longer resolve to an eligible original definition
/// in `group`. They stay in the save untouched: an older or unsupported build
/// is reported, never silently destroyed.
pub fn unsupported_instances(originals: &Originals, group: u32, build: &VehicleBuild) -> Vec<u32> {
    let eligible: Vec<u32> = mechanical_assets::eligible(originals, group)
        .iter()
        .map(|d| d.model.instance_id)
        .collect();
    let mut out: Vec<u32> = build
        .parts
        .iter()
        .map(|p| p.instance)
        .filter(|i| !eligible.contains(i))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The authored catalog name of a saved model instance. Reading it from the
/// catalog means the HUD can label a part without importing it first.
pub fn part_name(originals: &Originals, group: u32, instance: u32) -> Option<String> {
    originals
        .part_catalog
        .parts
        .iter()
        .find(|d| d.key.group_id == group && d.model.instance_id == instance && !d.is_null)
        .map(|d| d.name.clone())
}

/// The material a mounted part shows: the authored one when the importer
/// produced it, otherwise one neutral tint. Building a full material per part
/// is what this deliberately avoids.
fn part_material(
    materials: &mut Assets<StandardMaterial>,
    model: &crate::OriginalModel,
) -> Handle<StandardMaterial> {
    model.material.clone().unwrap_or_else(|| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.75, 0.8),
            perceptual_roughness: 0.5,
            ..default()
        })
    })
}

/// Mount every resolvable part of `build` under `parent`, using the authored
/// material, a world-size-capped scale and `place` for the local transform.
/// Parts whose model cannot be resolved are skipped and left in the save.
#[allow(clippy::too_many_arguments)]
fn mount_mechanical_parts(
    commands: &mut Commands,
    meshes: &Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    parent: Entity,
    build: &VehicleBuild,
    group: u32,
    mode: CivEditorMode,
    place: impl Fn(&VehiclePart) -> (Vec3, f32),
    free: (f32, Vec3),
) -> Vec<(usize, Entity)> {
    let mut spawned = Vec::new();
    for (i, p) in build.parts.iter().enumerate() {
        if let Some(block) = &p.block {
            // A block of the rigid editors: its own free transform scaled to
            // the stage (`rigid_editor::block_transform`), every authored
            // surface drawn.
            let Some(model) = crate::rigid_editor::block_model(originals, group, p.definition_id, p.instance, &block.handles) else {
                continue;
            };
            let model = &model;
            let unit = crate::rigid_editor::editor_unit(model) * p.scale.clamp(PART_SCALE_MIN, PART_SCALE_MAX);
            let entity = commands
                .spawn((
                    StageOwned,
                    Visibility::Inherited,
                    crate::rigid_editor::block_transform(block, unit, free.0, free.1),
                ))
                .id();
            crate::rigid_editor::spawn_surfaces(commands, entity, model, materials, None, &block.paints);
            commands.entity(parent).add_child(entity);
            spawned.push((i, entity));
            continue;
        }
        let Some(model) = mechanical_assets::cached(originals, group, p.instance) else {
            continue;
        };
        let (off, yaw) = place(p);
        let scale = mechanical_assets::capped_scale(meshes, model, mode.cap())
            * mode.base()
            * p.scale.clamp(PART_SCALE_MIN, PART_SCALE_MAX);
        let entity = commands
            .spawn((
                StageOwned,
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(part_material(materials, model)),
                Transform::from_translation(off)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(scale)),
            ))
            .id();
        commands.entity(parent).add_child(entity);
        spawned.push((i, entity));
    }
    spawned
}

/// Mount the saved vehicle parts on one of your vehicles, tagged with their
/// index in the build. Vehicle slots are laid out for a 2.6-unit hull.
pub fn mount_vehicle_parts(
    commands: &mut Commands,
    meshes: &Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    vehicle: Entity,
    run: &RunProgress,
) {
    mount_vehicle_class_parts(commands, meshes, materials, originals, vehicle, run, None);
}

/// The design a vehicle of this type and domain shows: its class's rigid
/// editor design when it has one, else the single old vehicle build.
pub fn vehicle_build_for(run: &RunProgress, class: Option<(Approach, VehicleDomain)>) -> VehicleBuild {
    match class.and_then(|(a, d)| run.rigid.vehicle_design(a, d)) {
        Some(design) => VehicleBuild {
            parts: design.parts.clone(),
            focused: 0,
        },
        None => run.vehicle.clone(),
    }
}

/// `mount_vehicle_parts` for a vehicle of a known class (`rts_spawn`).
pub fn mount_vehicle_class_parts(
    commands: &mut Commands,
    meshes: &Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    vehicle: Entity,
    run: &RunProgress,
    class: Option<(Approach, VehicleDomain)>,
) {
    let build = vehicle_build_for(run, class);
    let spawned = mount_mechanical_parts(
        commands,
        meshes,
        materials,
        originals,
        vehicle,
        &build,
        asset_catalog::VEHICLE_PARTS,
        CivEditorMode::Vehicle,
        |p| {
            let (offset, yaw) = vehicle_part_transform(p.slot, p);
            (offset * 0.6 + Vec3::Y * 0.7, yaw)
        },
        // One editor unit = one world unit (`civ_rts::VEHICLE_MOUNT_SCALE`).
        (crate::civ_rts::VEHICLE_MOUNT_SCALE, Vec3::ZERO),
    );
    for (part, entity) in spawned {
        commands.entity(entity).insert(VehiclePartRef { part });
    }
}

/// Mount the saved skyline on one city root, tagged with the city so a
/// rebuild touches only that city's parts.
fn mount_city_assembly(
    commands: &mut Commands,
    meshes: &Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    city: Entity,
    city_index: usize,
    run: &RunProgress,
) {
    let spawned = mount_mechanical_parts(
        commands,
        meshes,
        materials,
        originals,
        city,
        &run.building,
        asset_catalog::BUILDING_PARTS,
        CivEditorMode::Building,
        |p| city_part_transform(p.slot, p),
        (crate::rigid_editor::BUILDING_STAGE_SCALE, Vec3::new(0.0, crate::rigid_editor::HALL_FLOOR, 0.0)),
    );
    for (part, entity) in spawned {
        commands.entity(entity).insert(CityPartRef {
            city: city_index,
            part,
        });
    }
}

/// Civ assembly editor over the persisted builds: `V` picks the vehicles or
/// the city skyline, then `N` mounts an original catalog part, `[`/`]` swaps
/// the focused one, `,`/`.` moves it to the next slot, `Tab` focuses,
/// `Backspace` removes, `R` rotates, `-`/`=` scales and `IJKL`/`UO` slides.
/// The grammar is the creature assembler's, but the parts are original
/// `VEHICLE_PARTS`/`BUILDING_PARTS` definitions imported on demand, so a
/// model whose geometry cannot be decoded is reported and skipped instead of
/// being committed to the loadout.
#[allow(clippy::too_many_arguments)]
pub fn civ_vehicle_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    parts: Query<Entity, With<VehiclePartRef>>,
    vehicles: Query<(Entity, Option<&crate::civ_rts::CivUnit>), With<Vehicle>>,
    cities: Query<(Entity, &CityMarker, Option<&CityAssemblyVisual>)>,
    city_parts: Query<(Entity, &CityPartRef)>,
) {
    // `, . - =` Tab and Backspace are camera / stance / camera-to-city keys
    // (`civ_controls`): the editor reads them only with Ctrl held.
    let filtered = crate::civ_controls::assembly_keys(&keys);
    let keys = &filtered;
    let mut mode_changed = false;
    if keys.just_pressed(KeyCode::KeyV) {
        run.civ_campaign.editor_mode = match run.civ_campaign.editor_mode {
            CivEditorMode::Vehicle => CivEditorMode::Building,
            CivEditorMode::Building => CivEditorMode::Vehicle,
        };
        mode_changed = true;
    }
    let mode = run.civ_campaign.editor_mode;
    let group = mode.group();
    let n_slots = mode.slots();
    // The build is edited on a copy: a part that fails to import leaves the
    // saved loadout exactly as it was.
    let stored = active_build(mode, &run).clone();
    let mut build = stored.clone();
    let mut note = String::new();

    if keys.just_pressed(KeyCode::Tab) && !build.parts.is_empty() {
        build.focused = (build.focused + 1) % build.parts.len() as u32;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        if build.parts.len() >= MAX_CIV_PARTS {
            note = format!(
                "{}: the {MAX_CIV_PARTS}-part limit is reached",
                mode.title()
            );
        } else {
            let cursor = run.civ_campaign.cursor();
            match import_next(
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                group,
                cursor,
                1,
            ) {
                Ok(added) => {
                    let instance = added.model.instance;
                    let slot = build.parts.len() as u32 % n_slots;
                    build.parts.push(VehiclePart::new(instance, slot));
                    build.focused = (build.parts.len() - 1) as u32;
                    run.civ_campaign.set_cursor(instance);
                    note = format!(
                        "{}: {} on {}",
                        mode.title(),
                        added.name,
                        slot_name(mode, slot)
                    );
                }
                Err(err) => note = format!("{}: {}", mode.title(), err),
            }
        }
    }
    if keys.just_pressed(KeyCode::Backspace) && !build.parts.is_empty() {
        let i = focus_index(build.focused, build.parts.len()) as usize;
        let removed = build.parts.remove(i);
        build.focused = build
            .focused
            .min(build.parts.len().saturating_sub(1) as u32);
        note = format!("{}: removed {:#010X}", mode.title(), removed.instance);
    }

    let fi = focus_index(build.focused, build.parts.len()) as usize;
    if fi < build.parts.len() {
        if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::BracketRight) {
            let dir = if keys.just_pressed(KeyCode::BracketRight) {
                1
            } else {
                -1
            };
            let current = build.parts[fi].instance;
            match import_next(
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                group,
                current,
                dir,
            ) {
                Ok(next) => {
                    let (instance, name) = (next.model.instance, next.name.clone());
                    if instance != current {
                        build.parts[fi].instance = instance;
                    }
                    run.civ_campaign.set_cursor(instance);
                    note = format!(
                        "{}: {} on {}",
                        mode.title(),
                        name,
                        slot_name(mode, build.parts[fi].slot)
                    );
                }
                // No usable candidate: the mounted part stays as it was.
                Err(err) => note = format!("{}: {}", mode.title(), err),
            }
        }
        let p = &mut build.parts[fi];
        if keys.just_pressed(KeyCode::Comma) || keys.just_pressed(KeyCode::Period) {
            let dir = if keys.just_pressed(KeyCode::Period) {
                1
            } else {
                -1
            };
            p.slot = cycle_bone(p.slot, dir, n_slots);
        }
        if keys.just_pressed(KeyCode::KeyR) {
            p.yaw = wrap_yaw(p.yaw + std::f32::consts::FRAC_PI_4);
        }
        if keys.just_pressed(KeyCode::Minus) {
            p.scale = scaled(p.scale, 0.9);
        }
        if keys.just_pressed(KeyCode::Equal) {
            p.scale = scaled(p.scale, 1.1);
        }
        apply_slide_offset(&keys, &mut p.offset);
    }

    let changed = build != stored;
    if !changed && !mode_changed {
        // The loadout did not move, but the feedback still has to be visible:
        // a failed import, an empty catalog or a full build is reported in the
        // HUD line, not only in the log, and the saved build stays untouched.
        if !note.is_empty() {
            let mut line = note.clone();
            if let Some(p) = build.parts.get(fi) {
                let name = part_name(&originals, group, p.instance)
                    .unwrap_or_else(|| format!("{:#010X}", p.instance));
                line = format!(
                    "{line} | [{fi}] {name} {} yaw {:.0}° scale {:.2}",
                    slot_name(mode, p.slot),
                    p.yaw.to_degrees(),
                    p.scale
                );
            }
            run.civ_campaign.message = line.clone();
            info!("{line}");
        }
        return;
    }
    match mode {
        CivEditorMode::Vehicle => run.vehicle = build.clone(),
        CivEditorMode::Building => run.building = build.clone(),
    }
    run.request_save();
    if mode == CivEditorMode::Vehicle {
        for e in &parts {
            commands.entity(e).despawn();
        }
        for (v, unit) in &vehicles {
            let class = unit.map(|u| (u.kind, u.domain));
            mount_vehicle_class_parts(&mut commands, &meshes, &mut materials, &originals, v, &run, class);
        }
    } else {
        refresh_city_assemblies(
            &mut commands,
            &meshes,
            &mut materials,
            &originals,
            &run,
            &cities,
            &city_parts,
        );
    }
    let mut line = if note.is_empty() {
        format!("editing the {}", mode.title())
    } else {
        note
    };
    if let Some(p) = build.parts.get(fi) {
        let name = part_name(&originals, group, p.instance)
            .unwrap_or_else(|| format!("{:#010X}", p.instance));
        line = format!(
            "{line} | [{fi}] {name} {} yaw {:.0}° scale {:.2}",
            slot_name(mode, p.slot),
            p.yaw.to_degrees(),
            p.scale
        );
    }
    let missing = unsupported_instances(&originals, group, &build);
    if !missing.is_empty() {
        line = format!(
            "{line} | {} unsupported: {}",
            missing.len(),
            id_list(&missing, 3)
        );
    }
    run.civ_campaign.message = line.clone();
    info!("{line}");
}

/// The build a mode edits (`run.vehicle` or `run.building`).
fn active_build(mode: CivEditorMode, run: &RunProgress) -> &VehicleBuild {
    match mode {
        CivEditorMode::Vehicle => &run.vehicle,
        CivEditorMode::Building => &run.building,
    }
}

/// Slot label of the mode's table, `?` when a saved slot is out of range.
fn slot_name(mode: CivEditorMode, slot: u32) -> &'static str {
    let table: &[VehicleSlot] = match mode {
        CivEditorMode::Vehicle => VEHICLE_SLOTS,
        CivEditorMode::Building => CITY_SLOTS,
    };
    table
        .get(slot as usize)
        .map(|s| s.name)
        .unwrap_or("out of range")
}

/// `0x1A2B3C4D, ...` for at most `limit` ids (the list is always finite).
fn id_list(ids: &[u32], limit: usize) -> String {
    let shown: Vec<String> = ids
        .iter()
        .take(limit)
        .map(|i| format!("{i:#010X}"))
        .collect();
    if ids.len() > limit {
        format!("{}, +{} more", shown.join(", "), ids.len() - limit)
    } else {
        shown.join(", ")
    }
}

/// Import the next original definition after `current` in `dir`, walking at
/// most one full pass over the eligible models so a definition the importer
/// cannot decode never traps the cycle. The chosen definition is cloned
/// before the cache is borrowed mutably.
pub fn import_next(
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    group: u32,
    current: u32,
    dir: i32,
) -> Result<mechanical_assets::ImportedMechanical, String> {
    let budget = mechanical_assets::eligible(originals, group).len().max(1);
    let mut cursor = current;
    let mut last = format!("no usable original parts in group {group:#010X}");
    for _ in 0..=budget {
        let Some(def) = mechanical_assets::next(originals, group, Some(cursor), dir) else {
            return Err(last);
        };
        let target = def.model.instance_id;
        let definition = def.clone();
        match mechanical_assets::import(originals, &definition, meshes, images, materials) {
            Ok(added) => return Ok(added),
            Err(err) => {
                last = format!("{target:#010X} does not import: {err}");
                cursor = target;
            }
        }
    }
    Err(last)
}

/// Rebuild every city's skyline whose owner or saved build changed. A city
/// with an empty build keeps its procedural hall, which is what the
/// no-install path (and rival cities) shows.
fn refresh_city_assemblies(
    commands: &mut Commands,
    meshes: &Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    run: &RunProgress,
    cities: &Query<(Entity, &CityMarker, Option<&CityAssemblyVisual>)>,
    city_parts: &Query<(Entity, &CityPartRef)>,
) {
    let signature = build_signature(&run.building);
    for (entity, marker, visual) in cities {
        let Some(city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        let want = CityAssemblyVisual {
            owner: city.owner,
            signature,
        };
        if visual == Some(&want) {
            continue;
        }
        for (part, tag) in city_parts {
            if tag.city == marker.0 {
                commands.entity(part).despawn();
            }
        }
        commands.entity(entity).insert(want);
        if city.owned() && !run.building.parts.is_empty() {
            mount_city_assembly(
                commands, meshes, materials, originals, entity, marker.0, run,
            );
        }
    }
}

/// Rebuild the skylines of owned cities. Scheduled after the campaign update
/// so a city taken this frame shows its skyline in the same frame.
pub fn civ_city_assemblies(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<crate::Originals>,
    run: Res<RunProgress>,
    cities: Query<(Entity, &CityMarker, Option<&CityAssemblyVisual>)>,
    city_parts: Query<(Entity, &CityPartRef)>,
) {
    refresh_city_assemblies(
        &mut commands,
        &meshes,
        &mut materials,
        &originals,
        &run,
        &cities,
        &city_parts,
    );
}

/// Keep the live CIV assembly visuals in step with edits made by the modal
/// mechanical creator. This system is only scheduled while that creator is
/// open; the existing keyboard editor continues to own all ordinary gameplay
/// refreshes.
#[allow(clippy::too_many_arguments)]
pub fn sync_creator_assemblies(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<crate::Originals>,
    run: Res<RunProgress>,
    vehicles: Query<(Entity, Option<&crate::civ_rts::CivUnit>), With<Vehicle>>,
    cities: Query<(Entity, &CityMarker, Option<&CityAssemblyVisual>)>,
    city_parts: Query<(Entity, &CityPartRef)>,
    vehicle_parts: Query<Entity, With<VehiclePartRef>>,
    mut previous: Local<Option<CreatorAssemblySnapshot>>,
) {
    use std::collections::HashSet;

    let vehicle_entities: HashSet<Entity> = vehicles.iter().map(|(e, _)| e).collect();
    let city_entities: HashSet<Entity> = cities.iter().map(|(entity, _, _)| entity).collect();
    let first_sync = previous.is_none();
    let vehicle_changed = first_sync
        || previous.as_ref().is_some_and(|snapshot| {
            snapshot.vehicle != run.vehicle
                || snapshot.designs != run.rigid.designs
                || snapshot.vehicles != vehicle_entities
        });
    let skyline_changed = first_sync
        || previous.as_ref().is_some_and(|snapshot| {
            snapshot.building != run.building
                || snapshot.designs != run.rigid.designs
                || snapshot.cities != city_entities
        });

    if vehicle_changed {
        for entity in &vehicle_parts {
            commands.entity(entity).despawn();
        }
        for (vehicle, unit) in &vehicles {
            mount_vehicle_class_parts(
                &mut commands,
                &meshes,
                &mut materials,
                &originals,
                vehicle,
                &run,
                unit.map(|u| (u.kind, u.domain)),
            );
        }
    }
    if skyline_changed {
        refresh_city_assemblies(
            &mut commands,
            &meshes,
            &mut materials,
            &originals,
            &run,
            &cities,
            &city_parts,
        );
    }

    *previous = Some(CreatorAssemblySnapshot {
        vehicle: run.vehicle.clone(),
        building: run.building.clone(),
        designs: run.rigid.designs.clone(),
        vehicles: vehicle_entities,
        cities: city_entities,
    });
}

#[derive(Default)]
pub(crate) struct CreatorAssemblySnapshot {
    vehicle: crate::game::VehicleBuild,
    building: crate::game::VehicleBuild,
    designs: std::collections::BTreeMap<String, crate::rigid_editor::RigidDesign>,
    vehicles: std::collections::HashSet<Entity>,
    cities: std::collections::HashSet<Entity>,
}

/// Import one saved instance so a fresh process can mount it. Never edits the
/// save: an instance that is gone from the install, or whose geometry the
/// importer cannot decode, is only reported.
fn import_saved(
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    group: u32,
    instance: u32,
    missing: &mut Vec<u32>,
) {
    if mechanical_assets::cached(originals, group, instance).is_some() {
        return;
    }
    let Some(definition) = mechanical_assets::eligible(originals, group)
        .into_iter()
        .find(|d| d.model.instance_id == instance)
    else {
        missing.push(instance);
        return;
    };
    let definition = definition.clone();
    if mechanical_assets::import(originals, &definition, meshes, images, materials).is_err() {
        missing.push(instance);
    }
}

/// The assembly editor line: mode, the focused part with its authored name,
/// the controls, and any saved part this install cannot resolve. Pure, so the
/// HUD and the tests read exactly the same text.
pub fn civ_editor_hud(run: &RunProgress, originals: &Originals) -> String {
    let mode = run.civ_campaign.editor_mode;
    let group = mode.group();
    let build = active_build(mode, run);
    let focused = build
        .parts
        .get(focus_index(build.focused, build.parts.len()) as usize);
    let mut line = format!("BUILD {}", mode.title().to_uppercase());
    match focused {
        Some(p) => {
            let name = part_name(originals, group, p.instance)
                .unwrap_or_else(|| format!("{:#010X}", p.instance));
            line = format!(
                "{line} [{name}] {} yaw {:.0}° scale {:.2} ({}/{})",
                slot_name(mode, p.slot),
                p.yaw.to_degrees(),
                p.scale,
                build.parts.len(),
                MAX_CIV_PARTS
            );
        }
        None => line = format!("{line} (empty, {MAX_CIV_PARTS} parts max)"),
    }
    let missing = unsupported_instances(originals, group, build);
    if !missing.is_empty() {
        line = format!(
            "{line} | {} unsupported: {}",
            missing.len(),
            id_list(&missing, 3)
        );
    }
    format!(
        "{line}\nN add  [ ] part  Ctrl+, . slot  Ctrl+Tab focus  Ctrl+Backspace remove  R rotate  Ctrl+- = scale  IJKL UO slide  V switch"
    )
}

/// The `E` engine purchase of 0.82 is not part of retail Civ (see
/// [`engine_factor`]): the system stays registered (`main.rs`) and does nothing,
/// `E` is free for the camera (the original's Q / E strafe keys are not read by
/// the Civ camera either).
pub fn civ_buy_engine() {}

fn ctrl_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
}

/// The city planner keys: `B` raises the best building on the slots of the
/// owned city nearest the camera, `4`/`5`/`6` force a House, Factory or
/// Entertainment building, `T` adds (or rebuilds) a wall turret and
/// `Shift+B` sells the last building raised (refund by difficulty).
pub fn civ_city_commands(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    focus: Query<&Transform, With<CivCommander>>,
) {
    if ctrl_held(&keys) {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if shift && keys.just_pressed(KeyCode::KeyB) {
        run.civ_campaign.ensure_cities();
        let at = focus.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
        let Some(index) = run.civ_campaign.nearest_owned(at, BUILD_REACH) else {
            run.civ_campaign.message = "Move the camera over one of your cities to sell there".into();
            return;
        };
        let difficulty = civ_rules::Difficulty::from_index(run.cell.difficulty);
        let city = &mut run.civ_campaign.cities[index];
        match sell_plot(city, None, difficulty) {
            Ok((refund, kind, plot)) => {
                let label = city.label();
                run.spice = (run.spice + refund).min(civ_rules::WEALTH_CAP);
                run.civ_campaign.message = format!("{label}: {} on plot {} sold for {refund:.0}", kind.title(), plot + 1);
                run.request_save();
            }
            Err(message) => run.civ_campaign.message = message,
        }
        return;
    }
    let forced = if keys.just_pressed(KeyCode::Digit4) {
        Some(Building::House)
    } else if keys.just_pressed(KeyCode::Digit5) {
        Some(Building::Factory)
    } else if keys.just_pressed(KeyCode::Digit6) {
        Some(Building::Entertainment)
    } else {
        None
    };
    let smart = keys.just_pressed(KeyCode::KeyB);
    let turret = keys.just_pressed(KeyCode::KeyT);
    if forced.is_none() && !smart && !turret {
        return;
    }
    run.civ_campaign.ensure_cities();
    let at = focus.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let Some(index) = run.civ_campaign.nearest_owned(at, BUILD_REACH) else {
        run.civ_campaign.message =
            "Move the camera over one of your cities (C1 is home) to build there".into();
        return;
    };
    let spice = run.spice;
    let city = &mut run.civ_campaign.cities[index];
    let result = if turret {
        buy_turret(city, spice).map(|left| {
            (
                left,
                format!(
                    "{}: turret raised ({} standing)",
                    city.label(),
                    city.live_turrets()
                ),
            )
        })
    } else {
        let choice = match forced {
            Some(kind) => city.best_plot(kind).map(|(plot, _)| (kind, plot)),
            None => city.smart_building(),
        };
        match choice {
            None => Err(format!("{}: every plot is built", city.label())),
            Some((kind, plot)) => buy_plot(city, kind, Some(plot), spice).map(|(left, plot)| {
                (
                    left,
                    format!(
                        "{}: {} on plot {} - +{:.0} spice/cycle, mood {}%",
                        city.label(),
                        kind.title(),
                        plot + 1,
                        city.income(),
                        city.happiness()
                    ),
                )
            }),
        }
    };
    match result {
        Ok((left, message)) => {
            run.spice = left;
            run.civ_campaign.message = message;
            run.request_save();
        }
        Err(message) => run.civ_campaign.message = message,
    }
}

/// The whole Civilization objective, scheduled after the RTS systems so it
/// sees the frame they produced.
///
/// Doctrine (`1`/`2`/`3`) sets the capital's type; every owned city, derrick
/// and trade route pays in on a fixed cadence (rivals bank theirs too);
/// damaged cities repair when left alone; a rival city that has been shot
/// down, converted or bought changes hands; and only a planet where every
/// city answers to C1 advances to Space.
#[allow(clippy::too_many_arguments)]
pub fn civ_campaign_update(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut next: ResMut<NextState<GameStage>>,
    tints: Res<CivTints>,
    mut cities: Query<
        (
            &CityMarker,
            &mut Transform,
            &mut CityVisual,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<Player>,
            Without<CityOwnershipBeacon>,
            Without<CityBanner>,
        ),
    >,
    // Explicitly disjoint from the city roots, so both queries
    // may hold a mutable material borrow without conflicting.
    mut beacons: Query<
        (&CityOwnershipBeacon, &mut MeshMaterial3d<StandardMaterial>),
        (Without<Player>, Without<CityMarker>, Without<CityBanner>),
    >,
    mut banners: Query<
        (&CityBanner, &mut MeshMaterial3d<StandardMaterial>),
        (
            Without<Player>,
            Without<CityMarker>,
            Without<CityOwnershipBeacon>,
        ),
    >,
    mut spawns: Option<ResMut<crate::civ_rts::UnitSpawns>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    run.civ_campaign.ensure_cities();
    run.civ_campaign.tick(dt);

    if run.civ_campaign.cell_power.is_none()
        || run.civ_campaign.creature_power.is_none()
        || run.civ_campaign.tribal_power.is_none()
    {
        let creature_allied = run
            .species_world
            .species
            .iter()
            .filter(|s| run.species_world.species_allied(s.id))
            .count();
        let creature_extinct = run
            .species_world
            .species
            .iter()
            .filter(|s| run.species_world.species_extinct(s.id))
            .count();
        let tribal_conquered = run
            .tribal_campaign
            .totem_carvings
            .iter()
            .filter(|c| **c == crate::tribal::TotemSegment::Conquered)
            .count();
        let tribal_allied = run
            .tribal_campaign
            .totem_carvings
            .iter()
            .filter(|c| **c == crate::tribal::TotemSegment::Allied)
            .count();

        let (cp, crp, tp) = resolve_consequence_powers(
            run.diet_carnivore,
            run.diet_herbivore,
            creature_allied,
            creature_extinct,
            tribal_conquered,
            tribal_allied,
        );
        if run.civ_campaign.cell_power.is_none() {
            run.civ_campaign.cell_power = Some(cp);
        }
        if run.civ_campaign.creature_power.is_none() {
            run.civ_campaign.creature_power = Some(crp);
        }
        if run.civ_campaign.tribal_power.is_none() {
            run.civ_campaign.tribal_power = Some(tp);
        }
    }

    // Doctrine: 1 army, 2 faith, 3 money.
    let selected = if ctrl_held(&keys) {
        None
    } else if keys.just_pressed(KeyCode::Digit1) {
        Some(Approach::Military)
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(Approach::Religious)
    } else if keys.just_pressed(KeyCode::Digit3) {
        Some(Approach::Economic)
    } else {
        None
    };
    if let Some(approach) = selected {
        run.civ_campaign.message = approach.hint();
        if run.civ_campaign.approach != approach {
            run.civ_campaign.approach = approach;
            run.civ_campaign.cities[HOME_INDEX].kind = approach;
            run.civ_campaign.cities[HOME_INDEX].ensure_layout();
            run.request_save();
        }
    }

    // Cities stay on their saved positions and fly their nation's colours, so
    // the map, the world and the campaign always agree on who holds what.
    let shape = run.civ_campaign.shape();
    for (marker, mut transform, mut visual, mut material) in &mut cities {
        let Some(city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        let want = city.center();
        if shape.distance(transform.translation, want) > 0.03 {
            transform.translation = shape.place_on(want, CITY_ROOT_LIFT);
            transform.rotation = shape.frame(want);
        }
        let nation = &tints.nations[city.nation().min(tints.nations.len() - 1)];
        if visual.owner != city.owner || material.0 != *nation {
            visual.owner = city.owner;
            material.0 = nation.clone();
        }
    }
    for (beacon, mut material) in &mut beacons {
        let Some(city) = run.civ_campaign.cities.get(beacon.0) else {
            continue;
        };
        let want = if city.owned() {
            &tints.player
        } else {
            &tints.rival
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
    for (banner, mut material) in &mut banners {
        let Some(city) = run.civ_campaign.cities.get(banner.0) else {
            continue;
        };
        let want = &tints.nations[city.nation().min(tints.nations.len() - 1)];
        if material.0 != *want {
            material.0 = want.clone();
        }
    }

    // Seconds since each city was last attacked (the 20 s window of 0xC00600
    // slot 0; the repairs run in the once-a-second city update below) and
    // the cultural slot 1's memory running out.
    for city in run.civ_campaign.cities.iter_mut() {
        city.calm += dt;
        city.preached = (city.preached - dt).max(0.0);
    }

    // War and peace: a relationship at the war threshold means war; back at
    // neutral (a Diplo Dervish, trade) the war ends. Retail has no declared war
    // and no timer of the human's (0xBFA7A0, `civ_rules::attack_held_back`): the
    // 120 s military declaration of 0.82 is gone.
    for index in 1..CITY_COUNT {
        if !run.civ_campaign.nation_alive(index) {
            continue;
        }
        let nation = &mut run.civ_campaign.nations[index];
        let message = if !nation.war && nation.relation.hostile() {
            nation.war = true;
            Some(format!("{} has had enough of you: war!", city_label(index)))
        } else if nation.war && nation.relation.peaceful() {
            nation.war = false;
            Some(format!("{} makes peace with you", city_label(index)))
        } else {
            None
        };
        if let Some(message) = message {
            run.civ_campaign.message = message;
        }
    }

    // The original's civilization update runs once per game second
    // (VERIFIED 0xAE61C0): city moods roll, and every 6th update pays 0.1 x
    // the cities' and derricks' income (0xBF7310), NPC income scaled by the
    // difficulty (0xBEFED0).
    let difficulty = civ_rules::Difficulty::from_index(run.cell.difficulty);
    run.civ_campaign.civ_second += dt;
    while run.civ_campaign.civ_second >= civ_rules::CIV_UPDATE_SECONDS {
        run.civ_campaign.civ_second -= civ_rules::CIV_UPDATE_SECONDS;
        run.civ_campaign.ticks = run.civ_campaign.ticks.wrapping_add(1);
        let tick = run.civ_campaign.ticks;
        let cloud = run
            .civ_campaign
            .black_cloud_city
            .filter(|(_, t)| *t > 0.0)
            .map(|(c, _)| c);
        for city in run.civ_campaign.cities.iter_mut() {
            let mood = city.model_with(cloud == Some(city.index)).happiness();
            mood_second(city, mood, tick);
            // 0xAE57C0 -> 0xBE3C20 / 0xBE2260: repairs, ruins, capture decay.
            city.city_second();
        }
        civ_development(&mut run, difficulty, spawns.as_deref_mut());
        if tick % civ_rules::MONEY_UPDATE_TICKS == 0 {
            pay_income(&mut run, difficulty);
        }
        // 0xBFBD30 -> 0xBE5890 per city of an NPC civ.
        npc_city_second(&mut run.civ_campaign, difficulty, tick, spawns.as_deref_mut());
    }
    run.civ_campaign.income_timer = (run.civ_campaign.ticks % civ_rules::MONEY_UPDATE_TICKS) as f32
        + run.civ_campaign.civ_second;
    // Then the new-city check (0xAE61C0 -> 0xAE2EF0 / 0xAE3AB0).
    crate::civ_reveal::update(&mut run, dt, difficulty, spawns.as_deref_mut());

    // Captured, converted or bought: a converted city is "almost" taken for
    // 2 s (`TimeInAlmostConvert`), then changes hands; a military capture is
    // immediate (0xBE9C70 -> 0xBE9AD0 at 100 points).
    for index in 1..CITY_COUNT {
        let Some(how) = conquest_of(&run.civ_campaign.cities[index]) else {
            let city = &mut run.civ_campaign.cities[index];
            // (Your own city being converted away keeps its "almost" timer.)
            if !(city.owned() && city.rival_conversion >= 1.0) {
                city.almost = 0.0;
            }
            continue;
        };
        let delay = match how {
            Conquest::Military => 0.0,
            Conquest::Religious => civ_rules::TIME_IN_ALMOST_CONVERT,
            // ADAPTED: a purchase is immediate.
            Conquest::Economic => 0.0,
        };
        let city = &mut run.civ_campaign.cities[index];
        city.almost += dt;
        if city.almost < delay {
            continue;
        }
        let mut spice = run.spice;
        let message = run.civ_campaign.take_city(index, how, &mut spice);
        run.spice = spice;
        run.civ_campaign.message = message;
        run.request_save();
        if run.civ_campaign.is_unified() {
            info!("every city now answers to C1");
        }
    }

    // 0xBE9AD0 on your cities: a rival whose capture points reached 100 takes
    // the city at once (the capital stays, see `CityState::add_capture`).
    for index in 1..CITY_COUNT {
        let city = &run.civ_campaign.cities[index];
        if let (true, Some(by)) = (city.owned(), city.captured_by().filter(|b| *b != HOME_INDEX)) {
            let message = run.civ_campaign.lose_city(index, by);
            run.civ_campaign.message = message;
            run.request_save();
        }
    }

    // A city of yours converted by a rival's preachers leaves your nation.
    if let Some(message) = run.civ_campaign.convert_away(dt) {
        run.civ_campaign.message = message;
        run.request_save();
    }

    // Only a fully unified planet ends the stage, and it ends it once: the
    // Advance rule picks the play style card (0xCF82C0), then `CVG_UnlockUFO`
    // and the switch to Space (0xCF4620). The unified planet shows the retail
    // Advance button (`0645_3746`); the stage ends when it is clicked
    // (`crate::civ_hud::ADVANCE_KEY` is its key). The cinematic `CVG_UnlockUFO` /
    // `CVG2SPG` plays when the Space stage starts (`cinematics::hooks`).
    if run.civ_campaign.is_unified() {
        if !run.civ_campaign.unified {
            run.civ_campaign.unified = true;
            let style = run.civ_campaign.advance_play_style();
            run.civ_campaign.play_style = Some(style);
            run.civ_campaign.message = format!(
                "Planet unified: every city answers to C1 - you earned the {} card",
                style.title()
            );
            info!(
                "civ ends with the {} card ({}), cinematic {}",
                style.title(),
                style.consequence_trait(),
                civ_rules::cinematics::UNLOCK_UFO
            );
            run.request_save();
        }
        if !run.civ_campaign.advanced && keys.just_pressed(crate::civ_hud::ADVANCE_KEY) {
            run.civ_campaign.advanced = true;
            run.award_dna(
                tuning_for(&GameStage::Civilization).dna_to_advance,
                &GameStage::Civilization,
            );
            let _ = run.advance();
            next.set(GameStage::Space);
        }
    }
}

/// Deterministic `rand(range)` for a city and a civ second (no RNG state, so
/// replays and saves agree).
fn city_roll(city: usize, tick: u32, salt: u32, range: u32) -> u32 {
    let mut h = (city as u32).wrapping_mul(0x9E37_79B9) ^ tick.wrapping_mul(0x85EB_CA6B) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h % range.max(1)
}

/// One second of a city's mood (VERIFIED 0xBD8D20): a party is rolled every
/// 10 s (not while attacked) and a riot every 5 s; each lasts 60 s. Attacked
/// = 0xC00600 slot 0 or 1 (hit or preached to in the last 20 s), which also
/// ends a party.
pub fn mood_second(city: &mut CityState, mood: i32, tick: u32) {
    let attacked = city.under_attack();
    city.party_roll += civ_rules::CIV_UPDATE_SECONDS;
    city.riot_roll += civ_rules::CIV_UPDATE_SECONDS;
    if city.party > 0.0 {
        city.party = (city.party - civ_rules::CIV_UPDATE_SECONDS).max(0.0);
        if attacked {
            city.party = 0.0;
        }
    } else if city.party_roll > civ_rules::PARTY_ROLL_SECONDS && !attacked {
        city.party_roll = 0.0;
        let roll = city_roll(city.index, tick, 0x50A7, civ_rules::MOOD_ROLL_RANGE);
        if civ_rules::party_starts(mood, roll) {
            city.party = civ_rules::MOOD_EVENT_SECONDS;
        }
    }
    if city.riot > 0.0 {
        city.riot = (city.riot - civ_rules::CIV_UPDATE_SECONDS).max(0.0);
    } else if city.riot_roll > civ_rules::RIOT_ROLL_SECONDS {
        city.riot_roll = 0.0;
        let roll = city_roll(city.index, tick, 0x7107, civ_rules::MOOD_ROLL_RANGE);
        if civ_rules::riot_starts(mood, roll) {
            city.riot = civ_rules::MOOD_EVENT_SECONDS;
        }
    }
}

/// The once-a-second checks of 0xBFBD30: the player's "developed" latch and,
/// on easy, the free vehicle for a stuck player.
fn civ_development(
    run: &mut RunProgress,
    difficulty: civ_rules::Difficulty,
    spawns: Option<&mut crate::civ_rts::UnitSpawns>,
) {
    let campaign = &run.civ_campaign;
    let vehicles = campaign.fleet(HOME_INDEX);
    if !campaign.player_developed {
        let owned = campaign.cities.iter().filter(|c| c.owned());
        let buildings: usize = owned.clone().map(|c| c.built()).sum();
        let building_worth: f32 = owned.flat_map(|c| c.plots.iter().flatten()).map(|b| b.cost()).sum();
        let stats = civ_rules::DevelopmentStats {
            cities: campaign.owned_count(),
            vehicles,
            buildings,
            worth: run.spice + vehicles as f32 * LAND_VEHICLE_COST + building_worth,
            derricks: campaign.derricks(HOME_INDEX),
        };
        if civ_rules::has_developed(difficulty, stats) {
            run.civ_campaign.player_developed = true;
        }
    }
    let campaign = &run.civ_campaign;
    let income = campaign.income_per_cycle();
    let queued = spawns.as_ref().is_some_and(|s| s.0.iter().any(|u| u.nation == HOME_INDEX));
    if civ_rules::free_vehicle_eligible(difficulty, vehicles, run.spice, income)
        && !queued
        && city_roll(HOME_INDEX, campaign.ticks, 0xF4EE, civ_rules::FREE_VEHICLE_ODDS) == 0
    {
        if let Some(spawns) = spawns {
            let kind = run.civ_campaign.cities[HOME_INDEX].kind;
            let pad = run.civ_campaign.vehicle_pad(HOME_INDEX);
            let unit = run.civ_campaign.add_unit(HOME_INDEX, kind, false, pad);
            spawns.0.push(unit);
            run.civ_campaign.message = "Your capital rolls out a free vehicle".into();
        }
    }
}

/// The rivals' construction second (VERIFIED 0xBFBD30 -> 0xBE5890 for every
/// city of an NPC civ): each present rival city ages and may build with its
/// nation's domestic budget (the effects follow the plots, `civ_fx`); a city's
/// free first vehicle rolls out at its pad (to `spawns` for the live scene).
pub fn npc_city_second(
    campaign: &mut CivCampaign,
    difficulty: civ_rules::Difficulty,
    tick: u32,
    mut spawns: Option<&mut crate::civ_rts::UnitSpawns>,
) {
    let c = civ_rules::npc_build_odds(difficulty);
    for index in 1..CITY_COUNT.min(campaign.cities.len()) {
        if !campaign.nation_alive(index) || campaign.cities[index].owned() {
            continue;
        }
        let nation = campaign.cities[index].nation();
        let Some(mut domestic) = campaign.nations.get(nation).map(|n| n.domestic) else {
            continue;
        };
        let rolls = [city_roll(index, tick, 0x000B_111D, c * 8), city_roll(index, tick, 0x007A_44E7, c * 10)];
        let kind = campaign.cities[index].kind;
        let room = campaign.vehicle_room(nation, kind);
        let city = &mut campaign.cities[index];
        city.age += civ_rules::CIV_UPDATE_SECONDS;
        let built = npc_build_second(city, &mut domestic, difficulty, rolls, room);
        campaign.nations[nation].domestic = domestic;
        if built == Some(NpcBuilt::FirstVehicle) {
            let pad = campaign.vehicle_pad(index);
            let unit = campaign.add_unit(nation, kind, false, pad);
            if let Some(spawns) = spawns.as_deref_mut() {
                spawns.0.push(unit);
            }
        }
        if let Some(built) = built {
            info!("civ: rival city {index} raised {built:?}");
        }
    }
}

/// The money update (VERIFIED 0xBF7310 / 0xBF70E0 / 0xBEFED0 / 0xCEF130):
/// you get 0.1 x (cities + derricks) + trade routes, the rivals the same
/// scaled by the difficulty; every paying derrick's geyser loses 0.3 points;
/// open routes build trade value and goodwill.
fn pay_income(run: &mut RunProgress, difficulty: civ_rules::Difficulty) {
    let gain = run.civ_campaign.income_per_cycle();
    if gain > 0.0 {
        run.spice = (run.spice + gain).min(civ_rules::WEALTH_CAP);
        run.request_save();
    }
    let developed = run.civ_campaign.player_developed;
    let scale = civ_rules::npc_income_multiplier(difficulty, developed, false);
    for nation in 1..CITY_COUNT {
        if run.civ_campaign.nation_alive(nation) {
            let gain = run.civ_campaign.income_for(nation) * scale;
            // 0xBF7370: the money splits into the domestic budget (the
            // cities' buildings and turrets) and the purse for vehicles: 0.8
            // for an economic civ or one alone on its continents (0xBF7190),
            // else 0.6. ADAPTED: split as it is earned (retail splits the
            // whole purse every second; income is its only steady source);
            // the first budget is the opening purse (`rival_opening`).
            let economic = run.civ_campaign.cities[nation].kind == Approach::Economic;
            let shares = run.civ_campaign.shares_continent(nation);
            let n = &mut run.civ_campaign.nations[nation];
            let share = civ_rules::domestic_share(false, economic, shares, n.domestic);
            n.domestic = (n.domestic + gain * share).min(civ_rules::domestic_cap(difficulty));
            n.spice = (n.spice + gain * (1.0 - share)).min(civ_rules::WEALTH_CAP);
        }
    }
    for geyser in &mut run.civ_campaign.geysers {
        if geyser.owner.is_some() {
            geyser.points = civ_rules::derrick_points_after(geyser.points);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::headless_app;

    fn fresh_campaign() -> CivCampaign {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        campaign
    }

    #[test]
    fn the_player_city_has_the_large_walls_and_a_rival_the_small_ones() {
        // RETAIL 0xBE7D40 / 0xBED5B0: slot 0 large, every NPC slot small.
        let capital = CityState::new(HOME_INDEX);
        assert!(capital.walls_large());
        assert_eq!(capital.layout().slots(), 12);
        assert_eq!(capital.plots.len(), 11, "the hall + 11 building slots");
        for index in 1..CITY_COUNT {
            let rival = CityState::new(index);
            assert!(!rival.walls_large());
            assert_eq!(rival.plots.len(), 5, "the hall + 5 building slots");
        }
        // The turret cap and the decoration spots are the walls' own.
        for index in 0..CITY_COUNT {
            let city = CityState::new(index);
            assert_eq!(
                city.max_turrets(),
                civ_rules::max_turrets(specialty(city.kind), !city.walls_large()),
                "city {index}"
            );
            assert_eq!(city.decorations.len(), city.layout().decoration_spots());
            assert_eq!(city.building_damage.len(), city.plots.len());
            assert!(city.turret_hp.len() <= city.max_turrets());
        }
    }

    #[test]
    fn the_link_graph_is_the_authored_matrix_and_the_hall_links_the_starters() {
        let capital = CityState::new(HOME_INDEX);
        let (nodes, links) = capital.link_graph();
        assert_eq!(nodes.len(), 12);
        assert_eq!(links, capital.layout().link_pairs());
        for (plot, building) in STARTER_PLOTS {
            assert_eq!(nodes[plot + 1], Some(building.model()));
        }
        // The factory, a house and the entertainment building link the hall;
        // the house on slot 1 reaches it through the entertainment building.
        for slot in [3, 4, 5] {
            assert!(links.contains(&(0, slot)), "slot {slot} links the hall");
        }
        assert!(links.contains(&(1, 5)) && !links.contains(&(0, 1)));
        // A rival city's graph is the small walls' (6 slots).
        let rival = CityState::new(1);
        let (nodes, links) = rival.link_graph();
        assert_eq!(nodes.len(), 6);
        assert_eq!(links, rival.layout().link_pairs());
        // A ruin is no node.
        let mut ruined = capital.clone();
        ruined.building_damage[1] = BUILDING_HP;
        assert_eq!(ruined.link_graph().0[2], None);
        // The mood model runs on it: the starters give the capital income.
        assert!(capital.income() > 0.0);
    }

    #[test]
    fn a_free_building_goes_to_the_best_linked_slot_of_the_authored_graph() {
        let city = CityState::new(HOME_INDEX);
        let (plot, score) = npc_slot_score(&city, Building::Entertainment).expect("a free linked slot");
        assert!(score > 0 && city.plots[plot].is_none());
        // Every free plot is a candidate on the 11-slot graph.
        assert!(city.plots.iter().filter(|p| p.is_none()).count() >= 7);
    }

    #[test]
    fn saves_with_the_earlier_ring_move_onto_the_authored_slots() {
        // The eight-plot ring of the earlier layout: buildings on 0, 2, 4, 6.
        let mut old = CityState::new(HOME_INDEX);
        old.plots = vec![
            Some(Building::House),
            None,
            Some(Building::Factory),
            None,
            Some(Building::House),
            None,
            Some(Building::Entertainment),
            None,
        ];
        old.building_damage = vec![0.0; LEGACY_PLOT_COUNT];
        old.building_damage[2] = 100.0;
        old.ruin_age = vec![0.0; LEGACY_PLOT_COUNT];
        old.decorations = vec![None, Some(3), None];
        old.ensure_layout();
        assert_eq!(old.plots.len(), 11);
        assert_eq!(old.plots.iter().flatten().count(), 4, "every building keeps a slot");
        assert_eq!(old.plots.iter().flatten().filter(|b| **b == Building::House).count(), 2);
        assert_eq!(old.building_damage.len(), 11);
        assert_eq!(old.building_damage.iter().filter(|d| **d == 100.0).count(), 1, "damage travels with its building");
        assert_eq!(old.decorations.len(), 30);
        assert_eq!(old.decorations[1], Some(3), "decorations keep their spot index");
        let again = old.clone();
        old.ensure_layout();
        assert_eq!(old, again, "ensure_layout is idempotent");
        // A save with no plots at all gets the starters.
        let mut bare = CityState::new(1);
        bare.plots.clear();
        bare.building_damage.clear();
        bare.ensure_layout();
        assert_eq!(bare.plots.len(), 5);
        assert_eq!(bare.built(), STARTER_PLOTS.len());
        // Turret armour is cut to the walls' mounts.
        let mut turrets = CityState::new(HOME_INDEX);
        turrets.kind = Approach::Economic;
        turrets.defense = 7;
        turrets.ensure_layout();
        assert_eq!(turrets.turret_hp.len(), 4, "diplo large walls hold 4 turrets");
    }

    #[test]
    fn the_authored_turret_mounts_cap_what_can_be_bought() {
        let mut city = CityState::new(HOME_INDEX);
        city.kind = Approach::Religious;
        city.ensure_layout();
        let mut spice = 100_000.0;
        while let Ok(left) = buy_turret(&mut city, spice) {
            spice = left;
        }
        assert_eq!(city.turret_hp.len(), 6, "shrine large walls: 6 turrets");
        let mut rival = CityState::new(3);
        rival.kind = Approach::Military;
        rival.ensure_layout();
        assert_eq!(rival.max_turrets(), 5, "castle small walls: 5 turrets");
        let mut domestic = 1.0e6;
        rival.age = 1.0e6;
        let mut built = 0;
        while npc_add_turret(&mut rival, &mut domestic).is_some() {
            built += 1;
        }
        assert_eq!(rival.turret_hp.len(), 5);
        assert!(built >= 1);
    }

    fn step(app: &mut App) {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        step(app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    /// Let time pass without touching the controls.
    fn wait(app: &mut App, frames: usize) {
        for _ in 0..frames {
            step(app);
        }
    }

    fn enter_civ(app: &mut App) {
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Civilization);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Civilization);
        step(app);
        // The opening fleet spawns on the first simulated frame.
        step(app);
    }

    fn stage(app: &App) -> GameStage {
        app.world().resource::<State<GameStage>>().get().clone()
    }

    fn focus(app: &mut App, position: Vec3) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players
            .single_mut(world)
            .expect("one camera focus")
            .translation = position;
    }

    fn spice(app: &App) -> f32 {
        app.world().resource::<RunProgress>().spice
    }

    fn set_spice(app: &mut App, spice: f32) {
        app.world_mut().resource_mut::<RunProgress>().spice = spice;
    }

    fn civ(app: &App) -> CivCampaign {
        app.world().resource::<RunProgress>().civ_campaign.clone()
    }

    fn fleet(app: &mut App, nation: usize) -> Vec<crate::civ_rts::CivUnit> {
        let world = app.world_mut();
        world
            .query::<&crate::civ_rts::CivUnit>()
            .iter(world)
            .filter(|u| u.nation == nation)
            .cloned()
            .collect()
    }

    // ---------------------------------------------------------------- data

    #[test]
    fn the_capital_its_rivals_and_the_geysers_land_on_stable_positions() {
        let campaign = fresh_campaign();
        assert_eq!(campaign.cities.len(), CITY_COUNT);
        assert_eq!(campaign.owned_count(), 1);
        assert_eq!(campaign.rivals_left(), RIVAL_COUNT);
        assert!(campaign.cities[HOME_INDEX].owned());
        for city in &campaign.cities {
            assert_eq!(Vec3::from_array(city.position), city_position(city.index));
            // The retail rings around the level origin (civ-re V4).
            let d = dist_xz(city.center(), Vec3::ZERO);
            assert!((122.0..=349.0).contains(&d), "{city:?} leaves the rings: {d}");
            assert_eq!(city.label(), format!("C{}", city.index + 1));
            assert_eq!(city.plots.len(), city.layout().plots());
            assert_eq!(city.built(), STARTER_PLOTS.len());
            assert_eq!(city.turret_hp.len(), city.defense as usize);
        }
        // Cities sit far apart: a battle at one is never in another's guns.
        for a in 0..CITY_COUNT {
            for b in a + 1..CITY_COUNT {
                assert!(
                    dist_xz(city_position(a), city_position(b)) > 3.0 * CITY_RANGE,
                    "C{} and C{} are too close",
                    a + 1,
                    b + 1
                );
            }
        }
        for g in 0..GEYSER_COUNT {
            let p = geyser_position(g);
            assert!(dist_xz(p, Vec3::ZERO) <= 600.0, "geyser {g} is far off");
            for c in 0..CITY_COUNT {
                assert!(
                    dist_xz(p, city_position(c)) > CITY_RANGE,
                    "geyser {g} sits inside C{}",
                    c + 1
                );
            }
        }
        // Deterministic: a re-entered stage builds the same planet.
        assert_eq!(fresh_campaign().cities, fresh_campaign().cities);
        // One nation of each type at least, all with their own colours.
        for kind in [Approach::Military, Approach::Religious, Approach::Economic] {
            assert!(campaign.cities[1..].iter().any(|c| c.kind == kind));
        }
        for a in 0..CITY_COUNT {
            for b in a + 1..CITY_COUNT {
                assert_ne!(nation_color(a), nation_color(b));
            }
        }
    }

    #[test]
    fn a_partial_campaign_still_loads() {
        let partial: CivCampaign =
            serde_json::from_str(r#"{"approach":"Religious","unified":true}"#).expect("partial");
        assert_eq!(partial.approach, Approach::Religious);
        assert!(partial.unified);
        assert!(partial.cities.is_empty(), "new fields default");
        // A city saved before plots and turrets existed keeps its counts and
        // receives a starter ring plus its legacy purchases.
        let old_city: CityState = serde_json::from_str(
            r#"{"index":2,"owner":"Player","position":[0,0,0],"hp":80,"conversion":0,
                "trade":false,"spent":0,"buildings":2,"defense":1,"reward_paid":true,"retaliate":0}"#,
        )
        .expect("a pre-plot city loads");
        let mut city = old_city.clone();
        city.ensure_layout();
        assert_eq!(city.buildings, 2);
        assert_eq!(city.defense, 1);
        assert_eq!(city.built(), STARTER_PLOTS.len() + 2);
        assert_eq!(city.turret_hp, vec![TURRET_HP]);
        // A save written before the campaign existed still opens.
        let mut value =
            serde_json::to_value(RunProgress::default()).expect("run serializes to an object");
        value
            .as_object_mut()
            .expect("RunProgress is an object")
            .remove("civ_campaign");
        let old: RunProgress = serde_json::from_value(value).expect("a pre-campaign save loads");
        assert_eq!(old.spice, 0.0);
        assert_eq!(old.civ_campaign, CivCampaign::default());
    }

    #[test]
    fn a_mid_game_campaign_survives_a_save_roundtrip() {
        let mut campaign = fresh_campaign();
        let mut spice = 0.0;
        campaign.begin(&mut spice);
        campaign.approach = Approach::Economic;
        campaign.cities[1].owner = CityOwner::Player;
        campaign.cities[1].reward_paid = true;
        campaign.cities[1].plots[1] = Some(Building::House);
        campaign.cities[2].trade = true;
        campaign.cities[2].spent = 30.0;
        campaign.cities[3].turret_hp[0] = 0.0;
        campaign.geysers[4].owner = Some(3);
        campaign.nations[1].war = true;
        campaign.units[0].order = UnitOrder::City(2);
        campaign.income_timer = 1.25;
        campaign.vehicles_lost = 2;
        campaign.message = "C3: bought 30%".to_string();
        campaign.last_target = Some(2);
        let mut run = RunProgress::default();
        run.spice = 240.0;
        run.civ_campaign = campaign.clone();
        let text = serde_json::to_string(&run).expect("serializes");
        let back: RunProgress = serde_json::from_str(&text).expect("parses");
        assert_eq!(back.civ_campaign, campaign);
        assert_eq!(back.spice, 240.0);
    }

    // ------------------------------------------------------------- economy

    #[test]
    fn the_opening_hands_out_a_purse_and_a_fleet_once_and_no_derrick() {
        let mut campaign = CivCampaign::default();
        let mut spice = 0.0;
        assert!(campaign.begin(&mut spice));
        assert_eq!(spice, STARTING_SPICE);
        assert_eq!(campaign.fleet(HOME_INDEX), 3);
        // Rivals: no opening fleet; the purse is their first budget (0xBF7370),
        // their first vehicle comes from their construction second.
        for nation in 1..CITY_COUNT {
            assert_eq!(campaign.fleet(nation), 0);
            assert_eq!(campaign.nations[nation].domestic, RIVAL_STARTING_SPICE);
            assert_eq!(campaign.nations[nation].spice, 0.0);
            assert!(!campaign.cities[nation].first_vehicle);
        }
        // VERIFIED (notes\civ-rows3-notes.md 2): nobody starts with a derrick; every nation claims a geyser.
        for nation in 0..CITY_COUNT {
            assert_eq!(campaign.derricks(nation), 0, "nation {nation}");
        }
        assert!(campaign.geysers.iter().all(|g| g.free()));
        let ids: std::collections::HashSet<u32> = campaign.units.iter().map(|u| u.id).collect();
        assert_eq!(ids.len(), campaign.units.len(), "vehicle ids are unique");
        assert!(
            !campaign.begin(&mut spice),
            "the opening is handed out once"
        );
        assert_eq!(spice, STARTING_SPICE);
    }

    #[test]
    fn linked_buildings_set_production_and_mood() {
        let mut city = CityState::new(HOME_INDEX);
        // The starters on the authored slots (hall = 0): House 1, Factory 3,
        // House 4, Entertainment 5. The factory (3) links the hall, the empty
        // slots 2 and 8 and the entertainment building (5): the factory links
        // only to the hall (VERIFIED 0xBE2590: 400 per factory link to a house
        // or the hall).
        assert_eq!(city.model().money, civ_rules::FACTORY_LINK_MONEY);
        assert_eq!(city.raw_production(), 40.0);
        // Entertainment 1 + its hall and house links = 3, factory 1 + its
        // entertainment link = 2: (3 - 2 + 5) x 10.
        assert_eq!(city.happiness(), 60);
        assert_eq!(city.housing(), 3, "two houses and the hall");
        assert!(city.producing());
        // A house beside the factory (slot 2 = plot 1) links to it.
        city.plots[1] = Some(Building::House);
        assert_eq!(city.model().money, 800);
        assert_eq!(city.housing(), 4);
        // Factories on slots 8 and 9 (plots 7 and 8) reach the hall only through
        // the factory on slot 3: they earn nothing of their own and each adds
        // a factory's unhappiness.
        city.plots[7] = Some(Building::Factory);
        city.plots[8] = Some(Building::Factory);
        assert_eq!(city.model().money, 800, "slots 8 and 9 link no house or hall");
        assert_eq!(city.happiness(), 40, "(3 - 4 + 5) x 10");
        assert_eq!(city.income(), 80.0);
        // A riot below neutral halves the income, a party above doubles it.
        city.riot = 30.0;
        assert!(city.rioting());
        assert_eq!(city.income(), 40.0);
        let mut happy = CityState::new(HOME_INDEX);
        happy.party = 30.0;
        assert_eq!(happy.income(), 80.0);
        // A destroyed factory leaves the link model and stops paying.
        let mut city = CityState::new(HOME_INDEX);
        assert!(city.income() > 0.0);
        assert!(city.damage_building(2, BUILDING_HP, 1), "the starter factory (slot 3) falls");
        assert_eq!(city.income(), 0.0);
        assert!(city.max_hp() > CITY_HP, "buildings armour the city");
    }

    #[test]
    fn the_planner_buys_buildings_and_turrets_only_where_it_may() {
        let mut city = CityState::new(HOME_INDEX);
        // The smart pick never makes the city riot and always gains.
        let (kind, plot) = city.smart_building().expect("free plots");
        let before = city.raw_production() + city.happiness() as f32;
        let (left, at) = buy_plot(&mut city, kind, Some(plot), 10_000.0).expect("affordable");
        assert_eq!(at, plot);
        assert_eq!(left, 10_000.0 - kind.cost());
        assert_eq!(city.buildings, 1);
        assert!(!city.rioting());
        assert!(city.raw_production() + city.happiness() as f32 > before);
        // Too poor, taken plots and full rings are refused.
        assert!(buy_plot(&mut city, Building::House, None, 1.0).is_err());
        assert!(buy_plot(&mut city, Building::House, Some(0), 10_000.0).is_err(), "plot 0 holds a starter");
        let mut spice = 10_000.0;
        while let Ok((left, _)) = buy_plot(&mut city, Building::House, None, spice) {
            spice = left;
        }
        assert_eq!(city.built(), city.plots.len());
        assert!(city.smart_building().is_none());
        // Turrets stop at the walls' turret mounts, ruined ones are rebuilt.
        let mut spice = 100_000.0;
        while let Ok(left) = buy_turret(&mut city, spice) {
            spice = left;
        }
        assert_eq!(city.defense as usize, city.max_turrets());
        city.turret_hp[2] = 0.0;
        assert!(buy_turret(&mut city, 1000.0).is_err(), "a turret costs 2000");
        // Selling refunds by difficulty (0xCF3350): the last building first.
        let mut sold = city.clone();
        let before = sold.built();
        let (refund, kind, _) = sell_plot(&mut sold, None, civ_rules::Difficulty::Medium).unwrap();
        assert_eq!(sold.built(), before - 1);
        assert_eq!(refund, kind.cost() * 0.75);
        assert_eq!(sell_plot(&mut sold.clone(), None, civ_rules::Difficulty::Easy).unwrap().0 > 0.0, true);
        assert!(buy_turret(&mut city, civ_rules::TURRET_COST).is_ok());
        assert_eq!(city.live_turrets(), city.max_turrets());
        // A rival city cannot be built up.
        let mut rival = CityState::new(2);
        assert!(buy_plot(&mut rival, Building::House, None, 1e6).is_err());
        assert!(buy_turret(&mut rival, 1e6).is_err());
    }

    #[test]
    fn income_follows_cities_derricks_and_trade() {
        let mut campaign = fresh_campaign();
        let home = campaign.cities[HOME_INDEX].income();
        assert_eq!(campaign.income_per_cycle(), home);
        campaign.geysers[0].owner = Some(HOME_INDEX);
        assert_eq!(campaign.income_per_cycle(), home + DERRICK_INCOME);
        // A trade route pays per delivery (civ_rts), not in the update.
        campaign.cities[2].trade = true;
        assert_eq!(campaign.income_per_cycle(), home + DERRICK_INCOME);
        campaign.cities[1].owner = CityOwner::Player;
        let taken = campaign.cities[1].income();
        assert!(taken > 0.0);
        assert_eq!(campaign.income_per_cycle(), home + taken + DERRICK_INCOME);
        // Rivals bank their own city and derricks, never yours.
        campaign.geysers[5].owner = Some(3);
        assert_eq!(
            campaign.income_for(3),
            campaign.cities[3].income() + DERRICK_INCOME
        );
        // Both are military cities: 3 for the capital's type + 2 and the
        // housing per city (VERIFIED 0xBF2110).
        assert_eq!(campaign.vehicle_cap(HOME_INDEX), {
            3 + campaign.cities[0].vehicle_capacity() + campaign.cities[1].vehicle_capacity()
        });
        assert_eq!(campaign.vehicle_cap_for(HOME_INDEX, Approach::Religious), 0);
    }

    #[test]
    fn the_retail_capture_rule_takes_a_city_building_by_building() {
        let mut city = CityState::new(1);
        let built = city.built();
        assert_eq!(city.hp, city.max_hp());
        assert_eq!(city.turret_hp, vec![TURRET_HP; 3], "600 per turret");
        // The hall ignores fire while any building stands (0xBD9E70).
        assert!(!city.damage_hall(500.0, HOME_INDEX));
        assert_eq!(city.capture, 0.0);
        // Turrets earn nothing (0xBD0660).
        assert!(city.damage_turret(0, TURRET_HP));
        assert_eq!(city.capture, 0.0);
        // Each building falls at 800 and is worth 8 points (0xBCEE50).
        let plots: Vec<usize> = (0..city.plots.len()).filter(|p| city.standing(*p)).collect();
        assert!(!city.damage_building(plots[0], 799.0, HOME_INDEX));
        assert!(city.damage_building(plots[0], 1.0, HOME_INDEX));
        assert_eq!(city.capture, 8.0);
        // The owner's own fire never counts.
        assert!(!city.damage_building(plots[1], 900.0, 1));
        for &p in &plots[1..] {
            city.damage_building(p, BUILDING_HP, HOME_INDEX);
        }
        assert!(city.hall_exposed());
        assert_eq!(city.capture, 8.0 * built as f32);
        assert!((city.hp - (CITY_HP - 80.0 * built as f32)).abs() < 1e-3, "armour = damage left");
        // Now every damage point on the hall is 0.1 point; 100 takes it.
        assert!(city.damage_hall(CITY_HP - 80.0 * built as f32 - 10.0, HOME_INDEX));
        assert_eq!(conquest_of(&city), None);
        city.damage_hall(10.0, HOME_INDEX);
        assert_eq!(conquest_of(&city), Some(Conquest::Military));
        // Capturing repairs the city and resets the points.
        let mut spice = 0.0;
        capture_city(&mut city, &mut spice);
        assert_eq!((city.standing_buildings(), city.capture, city.capturer), (built, 0.0, None));
        // A rival's points on one of your cities reach 100 as well (0xBE9AD0
        // changes the owner for any attacker) and it falls to them ...
        for p in 0..city.plots.len() {
            city.damage_building(p, BUILDING_HP, 2);
        }
        city.damage_hall(1e6, 2);
        assert_eq!(city.captured_by(), Some(2));
        // ... except the capital (ADAPTED: slot 0 is yours).
        let mut capital = CityState::new(HOME_INDEX);
        for p in 0..capital.plots.len() {
            capital.damage_building(p, BUILDING_HP, 2);
        }
        capital.damage_hall(1e6, 2);
        assert!(capital.captured_by().is_none() && capital.owned());

        // After 20 s without an attack: points decay 0.1/s, buildings heal
        // 2/s, turrets 1/s; ruins go after 30 s, ruined turrets after 120 s.
        let mut city = CityState::new(3);
        let plot = (0..city.plots.len()).find(|p| city.standing(*p)).unwrap();
        let other = (0..city.plots.len()).filter(|p| city.standing(*p)).nth(1).unwrap();
        city.damage_building(plot, BUILDING_HP, HOME_INDEX);
        city.damage_building(other, 100.0, HOME_INDEX);
        city.damage_turret(0, TURRET_HP);
        city.damage_turret(1, 50.0);
        city.calm = 0.0;
        for _ in 0..19 {
            city.calm += 1.0;
            city.city_second();
        }
        assert_eq!(city.capture, 8.0, "no decay while attacked recently");
        assert_eq!(city.building_health(other), 700.0);
        for _ in 0..12 {
            city.calm += 1.0;
            city.city_second();
        }
        assert!(city.capture < 8.0 && city.capture > 6.5);
        assert!(city.building_health(other) > 700.0);
        assert_eq!(city.plots[plot], None, "the ruin is cleared");
        assert!(city.turret_hp[1] > TURRET_HP - 50.0);
        assert_eq!(city.turret_hp.len(), 2, "the ruined turret stands until 120 s");
        for _ in 0..100 {
            city.calm += 1.0;
            city.city_second();
        }
        assert_eq!((city.turret_hp.len(), city.defense), (1, 1));
    }

    #[test]
    fn every_conquest_route_takes_the_city_once() {
        let mut city = CityState::new(1);
        assert_eq!(conquest_of(&city), None);
        city.capture = civ_rules::CAPTURE_FULL;
        city.capturer = Some(HOME_INDEX);
        assert_eq!(conquest_of(&city), Some(Conquest::Military));
        let mut city = CityState::new(2);
        city.conversion = 1.0;
        assert_eq!(conquest_of(&city), Some(Conquest::Religious));
        let mut city = CityState::new(3);
        city.spent = city.buy_price();
        assert_eq!(conquest_of(&city), None, "no purchase without trade rights");
        city.trade = true;
        assert_eq!(conquest_of(&city), Some(Conquest::Economic));

        let mut campaign = fresh_campaign();
        campaign.geysers[7].owner = Some(4);
        campaign.cities[4].turret_hp = vec![0.0; 3];
        campaign.cities[4].capture = civ_rules::CAPTURE_FULL;
        campaign.cities[4].capturer = Some(HOME_INDEX);
        let mut spice = 0.0;
        let message = campaign.take_city(4, Conquest::Military, &mut spice);
        assert!(message.contains("army"), "{message}");
        assert_eq!(spice, CAPTURE_REWARD);
        assert!(campaign.cities[4].owned());
        assert_eq!(
            campaign.geysers[7].owner,
            Some(HOME_INDEX),
            "derricks change hands"
        );
        assert_eq!(campaign.cities[4].live_turrets(), 3, "defenses are rebuilt");
        assert!(!campaign.nation_alive(4));
        assert_eq!(conquest_of(&campaign.cities[4]), None);
        let mut spice = 0.0;
        assert!(!capture_city(&mut campaign.cities[4], &mut spice));
        assert_eq!(spice, 0.0, "a city already yours pays nothing again");
    }

    #[test]
    fn the_stage_wants_cities_not_spice() {
        let mut campaign = fresh_campaign();
        assert!(!campaign.is_unified());
        campaign.cities[1].owner = CityOwner::Player;
        assert!(!campaign.is_unified());
        for index in 2..CITY_COUNT {
            campaign.cities[index].owner = CityOwner::Player;
        }
        assert!(campaign.is_unified());
        assert_eq!(campaign.rivals_left(), 0);
        assert!(
            campaign.hud().contains("planet unified"),
            "{}",
            campaign.hud()
        );
        // A save without a single city cannot skip the stage either.
        let empty = CivCampaign::default();
        assert!(!empty.is_unified());
        assert!(empty.hud().contains("no cities"), "{}", empty.hud());
    }

    #[test]
    fn a_faith_converts_citizens_through_the_buckets_until_it_holds_half() {
        let mut city = CityState::new(2);
        city.ensure_buckets();
        let total = city.citizens();
        assert!(total >= 2);
        assert_eq!(city.buckets[2], total, "every citizen starts with the owner");
        assert_eq!(city.leader, None);
        // The first faith takes the lead and converts from the owner (0xBEB1E0).
        city.convert_citizens(HOME_INDEX, 1);
        assert_eq!((city.leader, city.buckets[HOME_INDEX], city.buckets[2]), (Some(HOME_INDEX), 1, total - 1));
        assert!((city.conversion - 2.0 / total as f32).abs() < 1e-5, "200 x 1 / total percent");
        // A second faith first sends the leader's followers home: back where it began.
        city.convert_citizens(3, 1);
        assert_eq!((city.leader, city.buckets[HOME_INDEX], city.buckets[3], city.buckets[2]), (None, 0, 0, total));
        assert_eq!(city.conversion, 0.0);
        // Half the citizens in the leading faith: the city changes hands by faith.
        for _ in 0..total.div_ceil(2) {
            city.convert_citizens(HOME_INDEX, 1);
        }
        assert!(city.conversion >= 1.0, "{} of {total}", city.buckets[HOME_INDEX]);
        assert_eq!(conquest_of(&city), Some(Conquest::Religious));
        // A city that changed hands gives every citizen to its new owner.
        city.owner = CityOwner::Player;
        city.reset_allegiance();
        assert_eq!((city.leader, city.buckets[HOME_INDEX], city.buckets[2]), (None, total, 0));
    }

    #[test]
    fn the_owner_wins_citizens_back_and_preachers_step_once_a_second() {
        let mut campaign = fresh_campaign();
        // A strong group always converts one citizen (0xCEEF10 odds above 1).
        let group = PreachGroup { city: 2, nation: HOME_INDEX, size: 30, power: 30.0 };
        campaign.preach_second(&[group]);
        campaign.preach_second(&[group]);
        let city = &campaign.cities[2];
        assert_eq!((city.buckets[HOME_INDEX], city.leader), (2, Some(HOME_INDEX)));
        // A group of the city's own nation, or of a nation that does not exist, does nothing.
        let own = PreachGroup { nation: 2, ..group };
        campaign.preach_second(&[own, PreachGroup { nation: 99, ..group }]);
        assert_eq!(campaign.cities[2].buckets[HOME_INDEX], 2);
        // 0xBE1340: with a happy city (odds above 1) the owner takes one back from each faith.
        let city = &mut campaign.cities[2];
        city.counter_conversion(100, || 0.0);
        assert_eq!(city.buckets[HOME_INDEX], 1);
        city.counter_conversion(100, || 0.0);
        assert_eq!((city.buckets[HOME_INDEX], city.leader), (0, None));
        // A roll above the odds wins nothing.
        campaign.preach_second(&[group]);
        let city = &mut campaign.cities[2];
        city.counter_conversion(0, || 0.99);
        assert_eq!(city.buckets[HOME_INDEX], 1);
    }

    #[test]
    fn a_rival_takes_one_of_your_cities_at_a_hundred_capture_points_but_never_the_capital() {
        let mut campaign = fresh_campaign();
        campaign.cities[3].owner = CityOwner::Player;
        campaign.cities[3].add_capture(1e6, 2);
        assert_eq!(campaign.cities[3].captured_by(), Some(2), "0xBE9AD0: 100 points of a rival");
        let message = campaign.lose_city(3, 2);
        assert!(message.contains("fell"), "{message}");
        let city = &campaign.cities[3];
        assert!(!city.owned() && city.capture == 0.0 && city.capturer.is_none());
        assert_eq!(city.buckets[3], city.citizens(), "the citizens go to the new owner");
        // The capital's points stop short (ADAPTED: slot 0 is yours).
        campaign.cities[HOME_INDEX].add_capture(1e6, 2);
        assert_eq!(campaign.cities[HOME_INDEX].captured_by(), None);
        // The player still takes a rival city the same way, and a rival nation
        // does not take another rival's.
        campaign.cities[4].add_capture(1e6, HOME_INDEX);
        assert_eq!(campaign.cities[4].captured_by(), Some(HOME_INDEX));
        campaign.cities[5].add_capture(1e6, 2);
        assert_eq!(campaign.cities[5].captured_by(), None);
    }

    #[test]
    fn hud_names_the_doctrine_goal_and_target() {
        let mut campaign = fresh_campaign();
        campaign.approach = Approach::Religious;
        campaign.cities[2].conversion = 0.12;
        campaign.message = "C3: converting".to_string();
        campaign.last_target = Some(2);
        let hud = campaign.hud();
        assert!(hud.contains("Religious"), "{hud}");
        assert!(hud.contains(&format!("take the other {RIVAL_COUNT} cities")), "{hud}");
        assert!(hud.contains("C3"), "{hud}");
        assert!(hud.contains("faith 12%"), "{hud}");
        assert!(
            hud.contains(&format!(
                "income {:.0}/{}s",
                campaign.income_per_cycle(),
                INCOME_PERIOD
            )),
            "{hud}"
        );
    }

    // ------------------------------------------------------------- runtime

    #[test]
    fn a_fresh_stage_fields_fleets_and_spice_alone_cannot_leave_it() {
        let mut app = headless_app(RunProgress::default());
        enter_civ(&mut app);
        assert_eq!(stage(&app), GameStage::Civilization);
        assert_eq!(spice(&app), STARTING_SPICE);
        assert_eq!(fleet(&mut app, HOME_INDEX).len(), 3);
        // Rivals start without vehicles (no retail opening fleet) ...
        for nation in 1..CITY_COUNT {
            assert_eq!(fleet(&mut app, nation).len(), 0, "nation {nation}");
        }
        // Spice from the capital and its derrick piles up...
        set_spice(&mut app, 100_000.0);
        wait(&mut app, 600);
        // ... and their cities' first construction second hands out the free
        // first vehicle (0xBE5890).
        for nation in 1..CITY_COUNT {
            assert!(!fleet(&mut app, nation).is_empty(), "nation {nation}");
        }
        let run = app.world().resource::<RunProgress>();
        assert!(run.spice > 100_000.0, "income still pays");
        assert_eq!(run.civ_campaign.owned_count(), 1);
        assert!(!run.civ_campaign.unified, "a full purse is not a conquest");
        assert_eq!(
            stage(&app),
            GameStage::Civilization,
            "spice must never advance the stage on its own"
        );
    }

    #[test]
    fn building_needs_a_city_you_hold() {
        let mut app = headless_app(RunProgress::default());
        enter_civ(&mut app);
        set_spice(&mut app, 10_000.0);
        focus(&mut app, city_position(3));
        press(&mut app, KeyCode::KeyB);
        let campaign = civ(&app);
        assert_eq!(campaign.cities[3].built(), STARTER_PLOTS.len());
        assert_eq!(spice(&app), 10_000.0, "a rival city cannot be built up");
        assert!(
            campaign.message.contains("your cities"),
            "{}",
            campaign.message
        );
        focus(&mut app, city_position(HOME_INDEX));
        press(&mut app, KeyCode::KeyB);
        press(&mut app, KeyCode::Digit5);
        press(&mut app, KeyCode::KeyT);
        let campaign = civ(&app);
        let capital = &campaign.cities[HOME_INDEX];
        assert_eq!(
            capital.built(),
            STARTER_PLOTS.len() + 2,
            "the capital builds"
        );
        assert!(capital.count(Building::Factory) >= 2);
        assert_eq!(capital.defense, 2);
        assert!(spice(&app) < 10_000.0);
    }

    #[test]
    fn vehicles_are_built_capped_and_claim_spice() {
        let mut app = headless_app(RunProgress::default());
        enter_civ(&mut app);
        set_spice(&mut app, 10_000.0);
        let cap = civ(&app).vehicle_cap(HOME_INDEX);
        for _ in 0..cap + 2 {
            press(&mut app, KeyCode::KeyG);
        }
        step(&mut app);
        assert_eq!(fleet(&mut app, HOME_INDEX).len(), cap, "the cap holds");
        assert!(
            civ(&app).message.contains("support"),
            "{}",
            civ(&app).message
        );
        press(&mut app, KeyCode::KeyH);
        assert!(civ(&app).message.contains("cities"), "aircraft are locked");
        // Every vehicle claims the free geyser nearest it.
        let before = civ(&app).derricks(HOME_INDEX);
        press(&mut app, KeyCode::KeyQ);
        press(&mut app, KeyCode::KeyX);
        for _ in 0..900 {
            step(&mut app);
            if civ(&app).derricks(HOME_INDEX) > before {
                break;
            }
        }
        assert!(
            civ(&app).derricks(HOME_INDEX) > before,
            "a vehicle reaches a geyser and builds a derrick: {}",
            civ(&app).message
        );
    }

    #[test]
    fn rival_turrets_fire_on_attackers() {
        let mut app = headless_app(RunProgress::default());
        enter_civ(&mut app);
        set_spice(&mut app, 0.0);
        // Park the fleet against C3's walls and order an assault.
        crate::civ_rts::teleport_fleet(
            app.world_mut(),
            HOME_INDEX,
            city_position(2) + Vec3::new(9.0 * CITY_SCALE, 0.0, 0.0),
        );
        focus(&mut app, city_position(2));
        press(&mut app, KeyCode::KeyQ);
        press(&mut app, KeyCode::KeyF);
        wait(&mut app, 240);
        let hurt = fleet(&mut app, HOME_INDEX).iter().any(|u| u.hp < u.max_hp)
            || civ(&app).vehicles_lost > 0;
        assert!(hurt, "a rival city must fire on its attackers");
        let city = &civ(&app).cities[2];
        assert!(
            city.hp < city.max_hp() || city.turret_hp.iter().any(|hp| *hp < TURRET_HP),
            "the assault lands: {}",
            city_status(city)
        );
        assert!(civ(&app).at_war(2), "an assault means war");
    }

    #[test]
    fn a_unified_planet_and_only_that_advances() {
        let mut app = headless_app(RunProgress::default());
        enter_civ(&mut app);
        set_spice(&mut app, 100_000.0);
        for (index, key) in [
            (1, KeyCode::Digit1),
            (2, KeyCode::Digit2),
            (3, KeyCode::Digit3),
            (4, KeyCode::Digit1),
        ] {
            press(&mut app, key);
            // The retail capture rule (600-health turrets, 800 per building,
            // the hall last) makes a siege last minutes, as in Spore.
            for frame in 0..24000 {
                if civ(&app).cities[index].owned() || stage(&app) != GameStage::Civilization {
                    break;
                }
                if frame % 60 == 0 {
                    press(&mut app, KeyCode::KeyG);
                    press(&mut app, KeyCode::KeyQ);
                }
                focus(&mut app, city_position(index));
                press(&mut app, KeyCode::KeyF);
            }
            let campaign = civ(&app);
            assert!(
                campaign.cities[index].owned(),
                "C{} does not fall: {} | {}",
                index + 1,
                campaign.message,
                city_status(&campaign.cities[index])
            );
            assert!(campaign.cities[index].reward_paid);
            if index + 1 < CITY_COUNT {
                assert_eq!(
                    stage(&app),
                    GameStage::Civilization,
                    "a partial conquest cannot skip ahead"
                );
            }
        }
        // Test pacing: the planet has ten city slots; the sieges above cover
        // every doctrine, the other rivals are handed over.
        assert_eq!(stage(&app), GameStage::Civilization, "a partial conquest cannot skip ahead");
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let run = &mut *run;
            for index in 5..CITY_COUNT {
                run.civ_campaign.take_city(index, Conquest::Military, &mut run.spice);
            }
        }
        // The unified planet waits for the Advance button (its key).
        step(&mut app);
        step(&mut app);
        assert_eq!(stage(&app), GameStage::Civilization, "the Advance button ends the stage");
        assert!(civ(&app).unified && !civ(&app).advanced);
        press(&mut app, crate::civ_hud::ADVANCE_KEY);
        // The stage switch is applied by the next state transition.
        step(&mut app);
        assert_eq!(stage(&app), GameStage::Space, "a unified planet moves on");
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.stage_order_index, 4);
        assert!(run.civ_campaign.unified && run.civ_campaign.advanced);
        assert_eq!(run.dna_this_stage, 0.0, "the next stage starts clean");
    }

    #[test]
    fn civ_consequence_powers_and_superweapons_resolve_accurately() {
        // Test Cell consequence power resolution
        assert_eq!(resolve_consequence_powers(1.0, 0.0, 0, 0, 0, 0).0, CivCellPower::Invulnerability);
        assert_eq!(resolve_consequence_powers(0.0, 1.0, 0, 0, 0, 0).0, CivCellPower::HealingAura);
        assert_eq!(resolve_consequence_powers(1.0, 1.0, 0, 0, 0, 0).0, CivCellPower::StaticBomb);

        // Test Creature consequence power resolution
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 0, 1, 0, 0).1, CivCreaturePower::MightyBomb);
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 1, 0, 0, 0).1, CivCreaturePower::DiploDervish);
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 0, 0, 0, 0).1, CivCreaturePower::BribeBomb);

        // Test Tribal consequence power resolution
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 0, 0, 1, 0).2, CivTribalPower::GadgetBomb);
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 0, 0, 0, 1).2, CivTribalPower::BlackCloud);
        assert_eq!(resolve_consequence_powers(0.0, 0.0, 0, 0, 0, 0).2, CivTribalPower::AdBlitz);

        // Test Superweapon resolution
        assert_eq!(resolve_superweapon(Approach::Military), CivSuperweapon::Icbm);
        assert_eq!(resolve_superweapon(Approach::Religious), CivSuperweapon::FanaticalUprising);
        assert_eq!(resolve_superweapon(Approach::Economic), CivSuperweapon::GlobalMerger);
    }

    #[test]
    fn civ_sea_vehicles_and_powers_lifecycle() {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();

        // Sea vehicles open with the first city's tech level (VERIFIED 0xBF44B0:
        // one city -> level 2) and cost 1500 (VERIFIED 0xBF22B0).
        assert!(campaign.sea_unlocked());
        let mut spice = 5000.0;
        let sea_unit =
            crate::civ_rts::try_produce_domain(&mut campaign, &mut spice, 0, 0, VehicleDomain::Sea)
                .unwrap();
        assert_eq!(sea_unit.domain, VehicleDomain::Sea);
        // 0x4E74D0: a default water vehicle has 200 + 400 x 0.33 health.
        assert!((sea_unit.hp - 332.0).abs() < 1e-3, "{}", sea_unit.hp);
        assert!(!sea_unit.air);
        assert_eq!(spice, 5000.0 - SEA_VEHICLE_COST);

        // Abilities cost money (0xBF6DF0) and recharge at the tool rate.
        campaign.cell_power = Some(CivCellPower::Invulnerability);
        let mut money = 1000.0;
        let err = campaign
            .use_power(PowerSlot::Cell, &mut money, Vec3::ZERO)
            .unwrap_err();
        assert!(err.contains("2000"), "{err}");
        money = 5000.0;
        let used = campaign.use_power(PowerSlot::Cell, &mut money, Vec3::ZERO).unwrap();
        assert!(used.message.contains("Invulnerability"));
        assert_eq!(campaign.invulnerability_timer, 15.0);
        assert_eq!(campaign.power_cell_cooldown, 60.0);
        assert_eq!(money, 3000.0);
        assert!(campaign.use_power(PowerSlot::Cell, &mut money, Vec3::ZERO).is_err());
        campaign.tick(5.0);
        assert_eq!(campaign.invulnerability_timer, 10.0);
        assert_eq!(campaign.power_cell_cooldown, 55.0);

        // Diplo Dervish raises the targeted nation's relationship: a declared
        // war ends.
        // VERIFIED 0xBF2510: Creature abilities open at two cities.
        campaign.creature_power = Some(CivCreaturePower::DiploDervish);
        let mut probe = 10_000.0;
        let err = campaign.use_power(PowerSlot::Creature, &mut probe, Vec3::ZERO).unwrap_err();
        assert!(err.contains("needs 2 cities"), "{err}");
        campaign.cities[1].owner = CityOwner::Player;
        campaign.nations[2].war = true;
        campaign.nations[2].relation.apply(civ_rules::RelationKind::DeclaredWar);
        campaign.last_target = Some(2);
        let mut money = 10_000.0;
        let used = campaign
            .use_power(PowerSlot::Creature, &mut money, city_position(2))
            .unwrap();
        assert!(used.message.contains("Diplo Dervish"), "{}", used.message);
        assert!(!campaign.nations[2].war, "the war is over");
        assert_eq!(money, 6000.0);
        assert_eq!(campaign.power_creature_cooldown, 180.0);

        // Gadget Bomb wrecks the target's turrets, leaves the city to the
        // vehicles and means war.
        // Tribal abilities open at four cities.
        campaign.tribal_power = Some(CivTribalPower::GadgetBomb);
        campaign.last_target = Some(3);
        let mut money = 20_000.0;
        let err = campaign.use_power(PowerSlot::Tribal, &mut money, Vec3::ZERO).unwrap_err();
        assert!(err.contains("needs 4 cities"), "{err}");
        campaign.cities[2].owner = CityOwner::Player;
        campaign.cities[4].owner = CityOwner::Player;
        let used = campaign.use_power(PowerSlot::Tribal, &mut money, Vec3::ZERO).unwrap();
        assert!(used.message.contains("Gadget Bomb"));
        assert_eq!(campaign.cities[3].live_turrets(), 0);
        // Every building falls (8 capture points each), the hall is left to
        // the vehicles.
        assert_eq!(campaign.cities[3].standing_buildings(), 0);
        assert_eq!(campaign.cities[3].capture, 8.0 * STARTER_PLOTS.len() as f32);
        assert_eq!(campaign.cities[3].captured_by(), None);
        assert!(campaign.at_war(3));
        assert_eq!(campaign.power_tribal_cooldown, 300.0);
        assert_eq!(money, 4000.0);
        assert!(matches!(used.effect, Some(UnitEffect::Damage { .. })));

        // Superweapon: locked below six cities (VERIFIED 0xBEFBA0), then it
        // takes every remaining city for 12,000 each, once.
        for city in 1..CITY_COUNT {
            campaign.cities[city].owner = CityOwner::Rival;
        }
        campaign.approach = Approach::Military;
        assert_eq!(resolve_superweapon(campaign.approach), CivSuperweapon::Icbm);
        let mut purse = 100_000.0;
        assert!(!campaign.superweapon_unlocked());
        assert!(campaign.use_power(PowerSlot::Superweapon, &mut purse, Vec3::ZERO).is_err());
        // Six of the ten cities (the flags 0xBF2510 are cleared once the whole
        // planet is held).
        for city in 1..=4 {
            campaign.cities[city].owner = CityOwner::Player;
        }
        assert!(!campaign.superweapon_unlocked(), "five cities are not enough");
        campaign.cities[5].owner = CityOwner::Player;
        assert!(campaign.superweapon_unlocked());
        let rivals = campaign.rivals_left();
        let used = campaign
            .use_power(PowerSlot::Superweapon, &mut purse, Vec3::ZERO)
            .unwrap();
        assert!(used.message.contains("ICBM"));
        assert!(campaign.is_unified(), "every city is taken");
        assert_eq!(campaign.captures.military, rivals as u32);
        assert_eq!(
            purse,
            100_000.0 - civ_rules::superweapon_cost(rivals) + CAPTURE_REWARD * rivals as f32
        );
        assert!(!campaign.superweapon_unlocked(), "one shot");
    }

    #[test]
    fn bombs_and_clouds_follow_the_tool_values() {
        let mut campaign = fresh_campaign();
        // Static Bomb: rival cities in its radius stop for 15 s.
        campaign.cell_power = Some(CivCellPower::StaticBomb);
        let mut money = 10_000.0;
        let used = campaign
            .use_power(PowerSlot::Cell, &mut money, city_position(1))
            .unwrap();
        assert_eq!(campaign.cities[1].stunned, 15.0);
        assert_eq!(campaign.cities[2].stunned, 0.0, "out of the radius");
        assert!(!campaign.cities[1].producing());
        assert!(matches!(used.effect, Some(UnitEffect::Stun { seconds, .. }) if seconds == 15.0));
        // Black Cloud (Tribal: four cities): entertainment off for 120 s,
        // so the mood drops.
        for city in [1, 3, 4] {
            campaign.cities[city].owner = CityOwner::Player;
        }
        campaign.tribal_power = Some(CivTribalPower::BlackCloud);
        campaign.last_target = Some(2);
        let mut money = 20_000.0;
        campaign.use_power(PowerSlot::Tribal, &mut money, Vec3::ZERO).unwrap();
        assert_eq!(campaign.black_cloud_city, Some((2, 120.0)));
        let city = &campaign.cities[2];
        assert!(city.model_with(true).happiness() < city.happiness());
        // Ad Blitz: trade opens and the price loses the full trade value.
        let mut campaign = fresh_campaign();
        for city in [1, 2, 4] {
            campaign.cities[city].owner = CityOwner::Player;
        }
        campaign.tribal_power = Some(CivTribalPower::AdBlitz);
        campaign.last_target = Some(3);
        let before = campaign.cities[3].buy_price();
        let mut money = 20_000.0;
        campaign.use_power(PowerSlot::Tribal, &mut money, Vec3::ZERO).unwrap();
        assert!(campaign.cities[3].trade);
        assert_eq!(campaign.cities[3].buy_price(), (before - 4000.0).max(2000.0));
        // Bribe Bomb hands the vehicles an effect, costs 4000.
        campaign.creature_power = Some(CivCreaturePower::BribeBomb);
        let mut money = 4000.0;
        let used = campaign
            .use_power(PowerSlot::Creature, &mut money, Vec3::ZERO)
            .unwrap();
        assert!(matches!(used.effect, Some(UnitEffect::Bribe { seconds, .. }) if seconds == 20.0));
        assert_eq!(money, 0.0);
    }

    #[test]
    fn rival_cities_rebuild_from_their_domestic_budget_as_0xbe5890() {
        use civ_rules::Difficulty::{Easy, Medium};
        let mut city = CityState::new(2);
        assert_eq!(city.kind, Approach::Religious);
        let plot = (0..city.plots.len()).find(|p| city.standing(*p)).unwrap();
        city.damage_building(plot, BUILDING_HP, HOME_INDEX);
        assert!(city.ruined(plot));
        // Below 1000 in the domestic budget nothing is built.
        city.calm = 100.0;
        let mut domestic = 900.0;
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [0, 1], false), None);
        // Under attack: no building; the emergency turret needs normal or
        // hard, the turret wait (100 s on normal) and fewer than 2 turrets.
        domestic = 9000.0;
        city.calm = 0.0;
        city.turret_hp.truncate(1);
        city.turret_ruin_age.truncate(1);
        city.defense = 1;
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [0, 0], false), None, "before the wait");
        city.age = 101.0;
        assert_eq!(npc_build_second(&mut city, &mut domestic, Easy, [0, 0], false), None, "never on easy");
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [0, 0], false), Some(NpcBuilt::Turret(1)));
        assert_eq!(domestic, 9000.0 - TURRET_COST);
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [0, 0], false), None, "two turrets stand");
        // Quiet: the roll picks the best kind on its best plot (the ruin or a
        // free plot) and pays for it.
        city.calm = 100.0;
        let before = domestic;
        let Some(NpcBuilt::Building(at, kind)) = npc_build_second(&mut city, &mut domestic, Medium, [0, 1], false) else {
            panic!("a building");
        };
        assert!(city.standing(at));
        assert_eq!(domestic, before - kind.cost());
        // With every other plot built, the ruin is rebuilt.
        let mut walled = CityState::new(2);
        // A military city behind small walls: 5 turret mounts (goal 5 x 3/5).
        walled.kind = Approach::Military;
        walled.defense = 2;
        walled.ensure_layout();
        let ruin = 0;
        for p in 0..walled.plots.len() {
            walled.plots[p] = Some(if p % 2 == 0 { Building::House } else { Building::Factory });
        }
        walled.damage_building(ruin, BUILDING_HP, HOME_INDEX);
        walled.calm = 100.0;
        let mut purse = 9000.0;
        let built = npc_build_second(&mut walled, &mut purse, Medium, [0, 1], false);
        assert!(matches!(built, Some(NpcBuilt::Building(0, _))), "{built:?}");
        assert!(walled.standing(ruin));
        // No roll and an earning city: maybe a quiet-time turret (goal 5 x 3/5
        // = 3, at most (standing + 1) / 2).
        assert_eq!(walled.turret_hp.len(), 2);
        assert_eq!(npc_build_second(&mut walled, &mut purse, Medium, [1, 0], false), None, "before the wait");
        walled.age = 150.0;
        assert_eq!(npc_build_second(&mut walled, &mut purse, Medium, [1, 0], false), Some(NpcBuilt::Turret(2)));
        assert_eq!(npc_build_second(&mut walled, &mut purse, Medium, [1, 0], false), None, "the goal of 3 is met");
    }

    #[test]
    fn the_slot_score_follows_0xbdb6e0() {
        // A city earning nothing values a factory beside the hall / a house
        // at 80; a house beside empty plots at 10 each.
        let mut city = CityState::new(2);
        city.plots = vec![None; city.plots.len()];
        city.ensure_layout();
        assert!(city.income() <= 0.0);
        let (plot, score) = npc_slot_score(&city, Building::Factory).unwrap();
        assert_eq!((plot, score), (0, 80), "plot 0 links to the hall");
        let (_, house) = npc_slot_score(&city, Building::House).unwrap();
        assert_eq!(house, 20, "two empty neighbours");
        // A ruined rival city rebuilds through the campaign second.
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        campaign.nations[2].domestic = 5000.0;
        let p = (0..campaign.cities[2].plots.len()).find(|p| campaign.cities[2].standing(*p)).unwrap();
        campaign.cities[2].damage_building(p, BUILDING_HP, HOME_INDEX);
        campaign.cities[2].calm = 100.0;
        let built_before = campaign.cities[2].standing_buildings();
        for tick in 0..400 {
            npc_city_second(&mut campaign, civ_rules::Difficulty::Medium, tick, None);
        }
        assert!(campaign.cities[2].standing_buildings() > built_before, "the rival rebuilt");
        assert!(campaign.nations[2].domestic < 5000.0);
    }

    #[test]
    fn rival_ai_details_follow_0xbe5890_and_0xbf7190() {
        use civ_rules::Difficulty::Medium;
        // The free first vehicle (mbFirstHarvester): once, before building,
        // only with room for one and a domestic budget of 1000.
        let mut city = CityState::new(3);
        assert!(!city.under_attack(), "a new city was never attacked");
        let mut domestic = 999.0;
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [0, 0], true), None);
        domestic = 1000.0;
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [1, 1], false), None, "no room: on to building");
        assert!(!city.first_vehicle);
        assert_eq!(npc_build_second(&mut city, &mut domestic, Medium, [1, 1], true), Some(NpcBuilt::FirstVehicle));
        assert_eq!(domestic, 1000.0, "free");
        assert!(city.first_vehicle);
        assert_ne!(npc_build_second(&mut city, &mut domestic, Medium, [0, 1], true), Some(NpcBuilt::FirstVehicle));
        // Preached to (0xC00600 slot 1): attacked for 20 s, no building.
        let mut preached = CityState::new(2);
        preached.first_vehicle = true;
        preached.mark_preached();
        assert!(preached.under_attack());
        let mut budget = 9000.0;
        assert_eq!(npc_build_second(&mut preached, &mut budget, Medium, [0, 1], true), None);
        assert_eq!(budget, 9000.0);
        // The campaign second rolls the vehicle out for the live scene.
        let mut campaign = fresh_campaign();
        let mut spice = 0.0;
        campaign.begin(&mut spice);
        let mut spawns = crate::civ_rts::UnitSpawns(Vec::new());
        npc_city_second(&mut campaign, Medium, 0, Some(&mut spawns));
        for nation in 1..CITY_COUNT {
            assert_eq!(campaign.fleet(nation), 1, "nation {nation}");
            assert!(campaign.cities[nation].first_vehicle);
        }
        assert_eq!(spawns.0.len(), RIVAL_COUNT);
        assert!(spawns.0.iter().all(|u| u.kind == campaign.cities[u.nation].kind && u.domain == VehicleDomain::Land));
        // The continent test: alone on its continent -> 0.8 to domestic.
        assert!(campaign.shares_continent(2), "one continent without ids");
        campaign.continents = vec![0, 1, 2, 0, 0, 0, 0, 0, 0, 0];
        assert!(!campaign.shares_continent(1) && !campaign.shares_continent(2));
        assert!(campaign.shares_continent(3) && campaign.shares_continent(HOME_INDEX));
        assert_eq!(
            crate::civ_planet::continent_ids(5, |a, b| (a % 2) == (b % 2)),
            vec![0, 1, 0, 1, 0]
        );
    }

    #[test]
    fn money_moods_and_trade_follow_the_retail_update() {
        let mut run = RunProgress::default();
        let mut spice = 0.0;
        run.civ_campaign.begin(&mut spice);
        run.spice = spice;
        assert_eq!(run.spice, civ_rules::INITIAL_WEALTH);
        // Nobody starts with a derrick (VERIFIED, notes\civ-rows3-notes.md 2): the capital's first
        // one is claimed by a vehicle, here placed directly.
        assert_eq!(run.civ_campaign.income_per_cycle(), 40.0, "the factory-hall link alone");
        let points = run.civ_campaign.geysers[0].points;
        run.civ_campaign.geysers[0] = GeyserState::derrick(HOME_INDEX, points);
        // Capital: one factory-hall link (40) + its derrick (100) per update.
        assert_eq!(run.civ_campaign.income_per_cycle(), 140.0);
        let rival = run.civ_campaign.nations[2].spice;
        let rival_income = run.civ_campaign.income_for(2);
        run.civ_campaign.cities[3].trade = true;
        pay_income(&mut run, civ_rules::Difficulty::Medium);
        assert_eq!(run.spice, civ_rules::INITIAL_WEALTH + 140.0, "routes pay per delivery");
        // Rivals earn x0.4 on medium while the player has not developed;
        // 0.6 of it goes to the domestic budget (a religious nation, 0xBF7370).
        let domestic = civ_rules::INITIAL_WEALTH;
        assert!((run.civ_campaign.nations[2].spice - (rival + rival_income * 0.4 * 0.4)).abs() < 1e-3);
        assert!((run.civ_campaign.nations[2].domestic - (domestic + rival_income * 0.4 * 0.6)).abs() < 1e-3);
        // Paying derricks lose 0.3 points; trade value comes with deliveries.
        let owned = run.civ_campaign.geysers.iter().find(|g| g.owner == Some(HOME_INDEX)).unwrap();
        assert!((owned.points - 99.7).abs() < 1e-4);
        assert_eq!(run.civ_campaign.cities[3].trade_value, 0.0);
        // Mood: a happy city eventually parties for 60 s, an unhappy one riots.
        let mut city = CityState::new(HOME_INDEX);
        city.calm = civ_rules::ATTACK_MEMORY_SECONDS;
        let mut partied = false;
        for tick in 0..2000 {
            mood_second(&mut city, 100, tick);
            if city.partying() {
                partied = true;
                assert!(city.party <= 60.0);
                break;
            }
        }
        assert!(partied);
        let mut sad = CityState::new(HOME_INDEX);
        let mut rioted = false;
        for tick in 0..2000 {
            mood_second(&mut sad, 0, tick);
            rioted |= sad.rioting();
        }
        assert!(rioted);
        // Neutral mood never parties nor riots.
        let mut calm = CityState::new(HOME_INDEX);
        calm.calm = civ_rules::ATTACK_MEMORY_SECONDS;
        for tick in 0..2000 {
            mood_second(&mut calm, 50, tick);
            assert!(!calm.partying() && !calm.rioting());
        }
    }

    #[test]
    fn sea_vehicles_need_tech_level_two_and_a_coastal_city_of_yours() {
        let mut campaign = fresh_campaign();
        // The flat fallback has no coast: every city counts.
        assert!(campaign.is_coastal(1) && campaign.sea_unlocked());
        // On a planet only a city with a boat spawn point is coastal (0xBDB9D0).
        campaign.sphere = Some([0.0, -500.0, 0.0, 500.0]);
        assert!(!campaign.is_coastal(HOME_INDEX) && !campaign.sea_unlocked());
        campaign.cities[1].harbor = Some([1.0, 0.0, 0.0]);
        assert!(campaign.is_coastal(1));
        assert!(!campaign.sea_unlocked(), "the coastal city is a rival's");
        campaign.cities[HOME_INDEX].harbor = Some([0.0, 0.0, 1.0]);
        assert!(campaign.sea_unlocked());
        assert!(crate::civ_rts::try_produce_domain(&mut campaign, &mut 9000.0, 0, 0, VehicleDomain::Sea).is_ok());
    }

    #[test]
    fn captures_set_the_tech_level_and_the_play_style() {
        let mut campaign = fresh_campaign();
        assert_eq!(campaign.vehicle_tech_level(), civ_rules::CITY_TECH_LEVEL);
        assert!(!campaign.air_unlocked());
        let mut spice = 0.0;
        campaign.take_city(1, Conquest::Economic, &mut spice);
        campaign.take_city(2, Conquest::Military, &mut spice);
        // VERIFIED 0xBF44B0: aircraft need more than three cities.
        assert!(!campaign.air_unlocked(), "three cities: level 2");
        campaign.take_city(3, Conquest::Economic, &mut spice);
        assert!(campaign.air_unlocked(), "four cities: level 3");
        assert!(!campaign.superweapon_unlocked(), "the superweapon needs six cities");
        assert_eq!(campaign.captures.economic, 2);
        assert_eq!(campaign.advance_play_style(), civ_rules::PlayStyle::Economic);
        campaign.take_city(4, Conquest::Military, &mut spice);
        campaign.take_city(5, Conquest::Military, &mut spice);
        assert!(campaign.superweapon_unlocked(), "six of the ten cities");
        // The method used most decides (VERIFIED 0xCF82C0; ties go religious).
        assert_eq!(campaign.advance_play_style(), civ_rules::PlayStyle::Military);
        campaign.take_city(6, Conquest::Economic, &mut spice);
        campaign.take_city(7, Conquest::Economic, &mut spice);
        campaign.take_city(8, Conquest::Economic, &mut spice);
        assert_eq!(campaign.vehicle_tech_level(), civ_rules::AIR_TECH_LEVEL, "nine of ten");
        campaign.take_city(9, Conquest::Economic, &mut spice);
        // The whole planet is level 4 (planet won), and the level never falls.
        assert_eq!(campaign.advance_play_style(), civ_rules::PlayStyle::Economic);
        assert_eq!(campaign.vehicle_tech_level(), civ_rules::PLANET_TECH_LEVEL);
        assert!(!campaign.superweapon_unlocked(), "the flags are cleared once the planet is whole");
        campaign.cities[9].owner = CityOwner::Rival;
        assert_eq!(campaign.vehicle_tech_level(), civ_rules::PLANET_TECH_LEVEL);
    }

    #[test]
    fn cities_and_geysers_keep_the_retail_spacing() {
        for a in 0..CITY_COUNT {
            for b in a + 1..CITY_COUNT {
                assert!(civ_rules::cities_spaced(dist_xz(city_position(a), city_position(b))));
            }
            for g in 0..GEYSER_COUNT {
                assert!(
                    civ_rules::resource_spaced(dist_xz(city_position(a), geyser_position(g))),
                    "geyser {g} crowds C{}",
                    a + 1
                );
            }
        }
        // A rival city's retail price with its starter ring and turrets.
        let city = CityState::new(2);
        let value = civ_rules::city_value(5200.0, city.live_turrets() as f32 * 2000.0, 0.0, false);
        assert!((city.buy_price() - civ_rules::buy_price(value, 1, 0.0)).abs() < 1e-2);
    }
}
