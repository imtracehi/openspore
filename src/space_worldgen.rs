//! Space stage world generation from the original data and code (S4).
//!
//! Everything here is a deterministic function of the galaxy seed and the star
//! index, never of the order a player visits stars, so a save only needs its
//! `current_system` and world ids to rebuild the same worlds.
//!
//! Original data (reverse-engineering notes `notes\space-re-notes.md` S4,
//! `notes\space-re\dec\SP_*.c`, `space-galaxy\target\rea\out\*.c`):
//! * the galaxy effect `SG_galaxy_messages~` (one `distribute` per star type:
//!   `subdivide N` x `density D` stars, positions from the 1-bit emit maps
//!   `SG_base_star_map` x `SG_noise_tile_*` / `SG_dustlane_map`);
//! * `cStarManager::StarGenerationMessageHandler` 0xBB5F00 (tech level, rare),
//!   `GeneratePlanetsForStar` 0xBB42A0, `CalculatePlanetScores` 0xBA65F0,
//!   the orbit pass 0xBAFC70 / 0xBADC70, empires 0xBB8270 / 0xBB27B0,
//!   `cSpaceTrading::AssignPlanetSpice` 0x103CA50, `CalculateTScore` 0xFC2040;
//! * `cSpaceNames::GenerateRandomName` 0x5ED000 (a letter-trigram generator
//!   trained on the lexicons of `NameGeneration.package`).
//! Anything the code was not traced for is marked `// ADAPTED:`.

#![allow(dead_code)] // the data API also serves callers added later (planet engine, HUD)

use bevy::prelude::Vec2;

mod layout;
mod maps;
mod names;
mod system;

pub use layout::{generate_layout, EmpireArchetype, EmpireRecord, GalaxyStar};
pub use maps::{SectorGrid, StarMaps, BIT_IMAGE_TYPE};
pub use names::{NameKind, SpaceNames};
pub use system::{generate_system, StarContext, SystemRecord, MAX_WORLD_SLOTS};

// ---------------------------------------------------------------------------
// Units and tuning (all original data unless noted)
// ---------------------------------------------------------------------------

/// Side of the star grid (`cStarManager` global 0x156C638): 64 x 64 sectors.
pub const GALAXY_GRID_SIZE: f32 = 1400.0;
/// Sectors per side of the star grid.
pub const GALAXY_SECTORS: usize = 64;
/// Half the grid: every star lies within this distance of the core.
pub const GALAXY_EXTENT: f32 = GALAXY_GRID_SIZE / 2.0;
/// World units spanned by the galaxy emit maps (`effectMap` rect +-1000).
pub const EMIT_MAP_HALF: f32 = 1000.0;
/// World units of one repeat of `SG_noise_tile_*` (`effectMap` rect +-100, tiled).
pub const NOISE_TILE: f32 = 200.0;

/// Solar-view scene units per retail solar unit (`space_view::SOLAR_SCALE`):
/// the retail orbit radii (star radius + temperature x 85 / 195) are scaled by
/// this to the scene.
pub const SCENE_PER_SOLAR_UNIT: f32 = 0.26;

/// `gametuning~!SpaceSolarSystem` 0xCDF8D12F.
pub const NUM_PLANETS_MIN: u32 = 1;
pub const NUM_PLANETS_MAX: u32 = 5;
pub const WEIGHT_BARREN: f32 = 1000.0;
pub const WEIGHT_ASTEROID_BELT: f32 = 400.0;
pub const WEIGHT_GAS_GIANT: f32 = 300.0;
pub const WEIGHT_TERRESTRIAL_1: f32 = 300.0;
pub const WEIGHT_TERRESTRIAL_2: f32 = 3.0;
pub const WEIGHT_TERRESTRIAL_3: f32 = 1.0;
pub const WEIGHT_ART_DIRECTED: f32 = 5.0;
pub const CHANCE_GAS_GIANT_MOON: f32 = 0.6;
pub const CHANCE_GAS_GIANT_RINGS: f32 = 0.8;
pub const CHANCE_TERRESTRIAL_MOON: f32 = 0.01;
pub const CHANCE_TERRESTRIAL_RINGS: f32 = 0.1;
pub const CHANCE_ADDITIONAL_PLANETS_SPACE_TECH: f32 = 0.3;
pub const TSCORE_TRIBE_PLANET: u8 = 2;
pub const TSCORE_CIV_PLANET: u8 = 3;
pub const TSCORE_COLONY_PLANET: u8 = 2;
pub const TSCORE_EMPIRE_HOMEWORLD: u8 = 3;
pub const TSCORE_GROB_PLANET: u8 = 0;
pub const MAX_ASTEROID_BELTS: u32 = 2;
pub const MAX_GAS_GIANTS: u32 = 3;
/// Width of the habitable band one T1-T3 planet uses up (0xBB42A0: 18); a
/// moon of such a planet uses 30 more.
const HABITABLE_BAND_PER_PLANET: f32 = 18.0;
const HABITABLE_BAND_PER_MOON: f32 = 30.0;
/// `grobOnlyRadius`, `grobStarsSpreadRadius`, `grobEmpireSize`.
pub const GROB_ONLY_RADIUS: f32 = 100.0;
pub const GROB_SPREAD_RADIUS: f32 = 105.0;
pub const GROB_EMPIRE_SIZE: usize = 2400;
/// `empireSizeWeights`: relative chance of an empire having `index` stars.
pub const EMPIRE_SIZE_WEIGHTS: [u32; 16] = [0, 20, 50, 100, 120, 120, 100, 30, 15, 10, 5, 5, 4, 3, 2, 1];
/// `percentChanceStarHasRare` (`SpaceGalacticConstants`): a star or binary
/// carries one rare with this chance.
pub const PERCENT_CHANCE_STAR_HAS_RARE: f32 = 25.0;
/// Tech level weights outside the Grox radius (0xBB39A0: creature 100 + the
/// empire weight 50 merged in, tribe 10, civilization 10).
const TECH_WEIGHTS: [(TechLevel, f32); 3] = [
    (TechLevel::Creature, 150.0),
    (TechLevel::Tribe, 10.0),
    (TechLevel::Civilization, 10.0),
];
/// `GalaxyStarterWorldMin/MaxDistanceFromCore` (`gge` 0x288CFA78).
pub const STARTER_MIN_DISTANCE: f32 = 250.0;
pub const STARTER_MAX_DISTANCE: f32 = 450.0;
/// Where `GenerateSolSystem` (0xBB2BF0) looks for the Sol star: the closest
/// yellow star to this galactic position.
pub const SOL_POSITION: Vec2 = Vec2::new(257.348, 257.348);

/// Retail star tuning (`SolarSystem!starg/staro/starm`): radius, mass,
/// temperature factor.
const STAR_G: (f32, f32, f32) = (30.0, 1.0, 1.0);
const STAR_O: (f32, f32, f32) = (50.0, 2.5, 2.0);
const STAR_M: (f32, f32, f32) = (18.0, 0.4, 0.5);

/// `SG_galaxy_messages~` (0x035DD6E7): (kind, subdivisions, density) per
/// `distribute`; a kind makes `subdivisions x density` stars (the effect runs
/// at most one subdivision per `ManualUpdate`, ten at most, 0xBB79A0).
/// The core is a single star (`density 1`, no subdivision). The order is the
/// star index order: yellow first so the home star and Sol (indices 0 and 42)
/// are yellow stars.
pub const STAR_COUNTS: [(StarKind, u32, u32); 12] = [
    (StarKind::Yellow, 10, 6000),
    (StarKind::Red, 10, 10000),
    (StarKind::Blue, 8, 16000),
    (StarKind::BinaryOO, 6, 150),
    (StarKind::BinaryOM, 6, 200),
    (StarKind::BinaryOG, 6, 200),
    (StarKind::BinaryGG, 6, 200),
    (StarKind::BinaryGM, 6, 200),
    (StarKind::BinaryMM, 6, 200),
    (StarKind::BlackHole, 6, 400),
    (StarKind::ProtoPlanetary, 6, 1500),
    (StarKind::GalacticCore, 1, 1),
];

/// Number of stars in the galaxy: 306,301.
pub const fn galaxy_star_count() -> usize {
    let mut total = 0usize;
    let mut i = 0;
    while i < STAR_COUNTS.len() {
        total += (STAR_COUNTS[i].1 * STAR_COUNTS[i].2) as usize;
        i += 1;
    }
    total
}

pub type EmpireId = u16;

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// Retail `StarType` (core, black hole, proto-planetary disk, G yellow, O blue,
/// M red and the six binaries). Colours are display values (ADAPTED).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StarKind {
    GalacticCore,
    BlackHole,
    ProtoPlanetary,
    Yellow,
    Blue,
    Red,
    BinaryOO,
    BinaryOM,
    BinaryOG,
    BinaryGG,
    BinaryGM,
    BinaryMM,
}

/// Retail orbit radii of a star system (`GetSolarStarOrbitRadius`), in retail
/// solar units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitRadii {
    pub hot: f32,
    pub normal: f32,
    pub cold: f32,
    /// `max(cold + 100, 400)`: the outer limit of the system.
    pub outer: f32,
}

impl StarKind {
    pub const ALL: [StarKind; 12] = [
        StarKind::GalacticCore,
        StarKind::BlackHole,
        StarKind::ProtoPlanetary,
        StarKind::Yellow,
        StarKind::Blue,
        StarKind::Red,
        StarKind::BinaryOO,
        StarKind::BinaryOM,
        StarKind::BinaryOG,
        StarKind::BinaryGG,
        StarKind::BinaryGM,
        StarKind::BinaryMM,
    ];

    /// The retail `StarType` number.
    pub fn retail_type(self) -> u8 {
        match self {
            StarKind::GalacticCore => 1,
            StarKind::BlackHole => 2,
            StarKind::ProtoPlanetary => 3,
            StarKind::Yellow => 4,
            StarKind::Blue => 5,
            StarKind::Red => 6,
            StarKind::BinaryOO => 7,
            StarKind::BinaryOM => 8,
            StarKind::BinaryOG => 9,
            StarKind::BinaryGG => 10,
            StarKind::BinaryGM => 11,
            StarKind::BinaryMM => 12,
        }
    }

    pub fn is_binary(self) -> bool {
        self.retail_type() >= 7
    }

    /// A star system that has planets (single or binary star).
    pub fn has_planets(self) -> bool {
        matches!(self, StarKind::Yellow | StarKind::Blue | StarKind::Red) || self.is_binary()
    }

    /// `GetBinarySystemStarTypes`: the two stars of a binary.
    pub fn components(self) -> Option<(StarKind, StarKind)> {
        use StarKind::*;
        match self {
            BinaryOO => Some((Blue, Blue)),
            BinaryOM => Some((Blue, Red)),
            BinaryOG => Some((Blue, Yellow)),
            BinaryGG => Some((Yellow, Yellow)),
            BinaryGM => Some((Yellow, Red)),
            BinaryMM => Some((Red, Red)),
            _ => None,
        }
    }

    /// Blue stars (O, OO, OM, OG) allow fewer planet types (mask 0x4F).
    pub fn is_blue_system(self) -> bool {
        matches!(
            self,
            StarKind::Blue | StarKind::BinaryOO | StarKind::BinaryOM | StarKind::BinaryOG
        )
    }

    fn single_params(self) -> (f32, f32, f32) {
        match self {
            StarKind::Yellow => STAR_G,
            StarKind::Blue => STAR_O,
            StarKind::Red => STAR_M,
            _ => (30.0, 1.0, 1.0),
        }
    }

    /// `GetSolarStarRadius`: 300 for the core, 100 for black holes and disks,
    /// the `solarStarRadius` property for G / O / M.
    pub fn solar_radius(self) -> f32 {
        match self {
            StarKind::GalacticCore => 300.0,
            StarKind::BlackHole | StarKind::ProtoPlanetary => 100.0,
            k if k.is_binary() => {
                let (a, b) = k.components().unwrap();
                a.solar_radius().max(b.solar_radius())
            }
            k => k.single_params().0,
        }
    }

    /// `GetSolarStarMass` (solar masses): the sum for a binary.
    pub fn mass(self) -> f32 {
        match self {
            StarKind::GalacticCore => 300.0,
            StarKind::BlackHole | StarKind::ProtoPlanetary => 100.0,
            k if k.is_binary() => {
                let (a, b) = k.components().unwrap();
                a.mass() + b.mass()
            }
            k => k.single_params().1,
        }
    }

    /// `GetSolarStarTemperature`: 1 for non-stars, the sum for a binary.
    pub fn temperature(self) -> f32 {
        match self {
            StarKind::GalacticCore | StarKind::BlackHole | StarKind::ProtoPlanetary => 1.0,
            k if k.is_binary() => {
                let (a, b) = k.components().unwrap();
                a.temperature() + b.temperature()
            }
            k => k.single_params().2,
        }
    }

    /// `GetBinarySystemBaseRadius` for binaries (ADAPTED: the apoapsis of the
    /// two orbits around the barycentre plus the larger star; the retail
    /// routine also evaluates an orbit position 0xFE82E0 that was not traced),
    /// else the star radius.
    pub fn base_radius(self) -> f32 {
        let Some((a, b)) = self.components() else {
            return self.solar_radius();
        };
        let (ra, rb) = (a.solar_radius(), b.solar_radius());
        let (ma, mb) = (a.mass(), b.mass());
        let q = mb / ma;
        let ecc = if a == b { 0.2 } else { 0.0 };
        // Distances of the stars from the barycentre (retail 0xC845B0).
        let big = ((ra + rb) * 1.5) / (q + 1.0) + 10.0;
        let small = big * q;
        (big.max(small) * (1.0 + ecc)) + ra.max(rb)
    }

    /// `GetSolarStarOrbitRadius` hot / normal / cold (+ `max(cold+100, 400)`).
    pub fn orbit_radii(self) -> OrbitRadii {
        let base = self.base_radius();
        let t = self.temperature();
        let hot = base + 15.0;
        let normal = base + t * 85.0;
        let cold = base + t * 195.0;
        OrbitRadii {
            hot,
            normal,
            cold,
            outer: (cold + 100.0).max(400.0),
        }
    }

    /// Display colour (linear-ish RGB).
    pub fn color(self) -> [f32; 3] {
        match self {
            StarKind::Yellow => [1.0, 0.86, 0.52],
            StarKind::Blue => [0.55, 0.72, 1.0],
            StarKind::Red => [1.0, 0.38, 0.22],
            StarKind::GalacticCore => [1.0, 0.78, 0.5],
            StarKind::BlackHole => [0.58, 0.32, 0.95],
            StarKind::ProtoPlanetary => [0.92, 0.55, 0.3],
            k => {
                let (a, b) = k.components().unwrap();
                let (ca, cb) = (a.color(), b.color());
                [
                    (ca[0] + cb[0]) * 0.5,
                    (ca[1] + cb[1]) * 0.5,
                    (ca[2] + cb[2]) * 0.5,
                ]
            }
        }
    }

    /// Light colour cast on the system's planets (softer than the disc).
    pub fn light(self) -> [f32; 3] {
        let c = self.color();
        [0.55 + c[0] * 0.45, 0.55 + c[1] * 0.45, 0.55 + c[2] * 0.45]
    }

    pub fn label(self) -> &'static str {
        match self {
            StarKind::GalacticCore => "Galactic core",
            StarKind::BlackHole => "Black hole",
            StarKind::ProtoPlanetary => "Proto-planetary disk",
            StarKind::Yellow => "Yellow star",
            StarKind::Blue => "Blue star",
            StarKind::Red => "Red star",
            StarKind::BinaryOO => "Binary star (blue-blue)",
            StarKind::BinaryOM => "Binary star (blue-red)",
            StarKind::BinaryOG => "Binary star (blue-yellow)",
            StarKind::BinaryGG => "Binary star (yellow-yellow)",
            StarKind::BinaryGM => "Binary star (yellow-red)",
            StarKind::BinaryMM => "Binary star (red-red)",
        }
    }

    /// Radius of the visible star sphere in the solar-system view: the retail
    /// radius in scene units (ADAPTED: the core is capped, disks are small).
    pub fn radius(self) -> f32 {
        match self {
            StarKind::GalacticCore => 30.0,
            StarKind::ProtoPlanetary => 3.0,
            k => k.solar_radius() * SCENE_PER_SOLAR_UNIT,
        }
    }
}

/// Retail `TechLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum TechLevel {
    #[default]
    None = 0,
    Creature = 1,
    Tribe = 2,
    City = 3,
    Civilization = 4,
    Empire = 5,
}

impl TechLevel {
    pub fn label(self) -> &'static str {
        match self {
            TechLevel::None => "Uninhabited",
            TechLevel::Creature => "Creatures",
            TechLevel::Tribe => "Tribe",
            TechLevel::City => "City",
            TechLevel::Civilization => "Civilization",
            TechLevel::Empire => "Space empire",
        }
    }
}

/// Retail `PlanetType`: asteroid belt, gas giant, and terraform levels T0..T3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanetKind {
    AsteroidBelt,
    GasGiant,
    T0,
    T1,
    T2,
    T3,
}

impl PlanetKind {
    pub fn is_terrestrial(self) -> bool {
        !matches!(self, PlanetKind::AsteroidBelt | PlanetKind::GasGiant)
    }

    /// The terraform level 0..=3 of a terrestrial planet.
    pub fn t_level(self) -> Option<u8> {
        match self {
            PlanetKind::T0 => Some(0),
            PlanetKind::T1 => Some(1),
            PlanetKind::T2 => Some(2),
            PlanetKind::T3 => Some(3),
            _ => None,
        }
    }

    pub fn from_t_level(level: u8) -> Self {
        match level {
            0 => PlanetKind::T0,
            1 => PlanetKind::T1,
            2 => PlanetKind::T2,
            _ => PlanetKind::T3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlanetKind::AsteroidBelt => "Asteroid belt",
            PlanetKind::GasGiant => "Gas giant",
            PlanetKind::T0 => "Barren planet (T0)",
            PlanetKind::T1 => "Planet (T1)",
            PlanetKind::T2 => "Planet (T2)",
            PlanetKind::T3 => "Planet (T3)",
        }
    }
}

/// Spice colour of a planet (`cSpaceTrading::AssignPlanetSpice`). Order is the
/// retail `spice1..spice6` order the cumulative draw runs over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Spice {
    Red,
    Blue,
    Yellow,
    Green,
    Pink,
    Purple,
}

/// `spaceTradingChance{Red,Blue,Yellow}{Easy,Hard}` of spice1..spice6 (PROPs
/// `spacetrading~`): rows red, blue, yellow stars; per spice
/// [easy red, hard red, easy blue, hard blue, easy yellow, hard yellow].
const SPICE_CHANCES: [(Spice, [f32; 6]); 6] = [
    (Spice::Red, [0.96, 0.5, 0.0, 0.0, 0.0, 0.0]),
    (Spice::Blue, [0.0, 0.0, 1.0, 0.2, 0.0, 0.0]),
    (Spice::Yellow, [0.0, 0.44, 0.0, 0.0, 0.94, 0.74]),
    (Spice::Green, [0.02, 0.02, 0.0, 0.3, 0.02, 0.22]),
    (Spice::Pink, [0.02, 0.02, 0.0, 0.35, 0.02, 0.02]),
    (Spice::Purple, [0.0, 0.02, 0.0, 0.15, 0.02, 0.02]),
];

/// `AssignPlanetSpice`: one draw `roll` in 0..1 against the cumulative chances
/// of the star colour (0xBA... the type switch: G, GG, GM yellow; O, OO, OM, OG
/// blue; everything else red) and planet difficulty (`hard` = the planet's
/// orbit is hot or cold, flags 0x3000).
pub fn assign_spice(star: StarKind, hard: bool, roll: f32) -> Spice {
    let column = match star {
        StarKind::Yellow | StarKind::BinaryGG | StarKind::BinaryGM => 4,
        StarKind::Blue | StarKind::BinaryOO | StarKind::BinaryOM | StarKind::BinaryOG => 2,
        _ => 0,
    } + usize::from(hard);
    let mut cumulative = 0.0;
    for (spice, chances) in SPICE_CHANCES {
        cumulative += chances[column];
        if roll <= cumulative {
            return spice;
        }
    }
    // The original returns without assigning when the sum falls short.
    SPICE_CHANCES[0].0
}

/// Retail `CalculateTScore` (0xFC2040): the distance of the (temperature,
/// atmosphere) point from the bullseye centre (0.5, 0.5) against the
/// `spaceTerraformLevel1..3Threshold` 0.4 / 0.25 / 0.1 of `TerraformTuning`.
pub fn t_score(temperature: f32, atmosphere: f32) -> u8 {
    let d = ((temperature - 0.5).powi(2) + (atmosphere - 0.5).powi(2)).sqrt();
    if d < 0.1 {
        3
    } else if d < 0.25 {
        2
    } else if d < 0.4 {
        1
    } else {
        0
    }
}

/// `GetPlanetTemperatureType` buckets (0xFC1F90).
pub fn temperature_label(temperature: f32) -> &'static str {
    if temperature < 0.1 {
        "Very cold"
    } else if temperature < 0.4 {
        "Cold"
    } else if temperature < 0.6 {
        "Temperate"
    } else if temperature < 0.85 {
        "Hot"
    } else {
        "Very hot"
    }
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

/// SplitMix64: a cheap hash of a seed and an index.
pub fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A uniform `0..1` float from a hash.
pub fn unit(hash: u64) -> f32 {
    ((hash >> 40) as f32) / ((1u64 << 24) as f32)
}
