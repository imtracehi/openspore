//! Space stage galaxy: the 306,301 stars of the original galaxy, the layout of
//! each system, and the interstellar travel rules between them.
//!
//! The galaxy, its systems and their planets come from `space_worldgen`
//! (deterministic from the galaxy seed and the star index): star positions
//! follow the original emit maps, so the stars sit on the arms the galaxy map
//! draws; system contents depend only on the star index (and its tech level /
//! empire), never on where the player has been, so a save's `current_system`
//! always rebuilds the same worlds. Distances are in galaxy units: drive range
//! 3..20, empire radius 20 and detection radius 5 all live in the same units
//! as the star positions.

use std::collections::{HashMap, HashSet};
use std::ops::Index;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

pub mod disks;
pub mod hover;
pub mod proxies;
pub mod stars;

use crate::dbpf::ResourceKey;
use crate::space_worldgen as wg;
pub use crate::space_worldgen::{
    EmpireId, EmpireRecord, GalaxyStar, PlanetKind, SpaceNames, Spice, StarKind, StarMaps,
    TechLevel,
};
use crate::stages::StageOwned;

/// Number of star systems in the galaxy.
#[allow(dead_code)]
pub const GALAXY_STARS: usize = wg::galaxy_star_count();
/// Radius of the galactic disc in galaxy units: the star grid is 1400 across
/// (`cStarManager` 0x156C638); every star lies inside it.
pub const GALAXY_RADIUS: f32 = wg::GALAXY_EXTENT;
/// Half the side of the galaxy emit-map rectangle (the colour map plane).
pub const GALAXY_MAP_HALF: f32 = wg::EMIT_MAP_HALF;
/// The galaxy map is drawn far below the solar system so both scenes can
/// exist at once: the Space camera's far plane (`space_view::SPACE_FAR`)
/// culls whichever is not being looked at.
pub const GALAXY_ORIGIN: Vec3 = Vec3::new(0.0, -20000.0, 0.0);
/// Height of the layers attached to star positions (the star particles, the disk sprites, territories, highlights
/// and the markers) in the map layer: the plane the galaxy camera looks at (its target is at y = 0), so the star
/// the camera is centred on stays on the screen centre at every zoom. (The earlier landings put the stars at 1 and
/// the markers at 1.2 to 2: below radius 3 the targeted star sat above the screen centre.) The layers are additive
/// without depth writes, so sharing a plane costs nothing.
pub const STAR_PLANE: f32 = 0.0;
/// The player's home star.
pub const HOME_STAR: u32 = 0;
/// Star index of the Sol system (Earth's sun).
pub const SOL_STAR: u32 = 42;
/// WorldIds of worlds outside the home system start here. The home system
/// keeps the ids `0..` (the homeworld is 0).
pub const FOREIGN_WORLD_BASE: u32 = 1000;
/// WorldId slots reserved per foreign star system.
pub const WORLDS_PER_SYSTEM: u32 = wg::MAX_WORLD_SLOTS;
/// The longest drive reach (the last drive: 20 galaxy units).
pub const MAX_JUMP: f32 = 20.0;
/// Interstellar jump energy: the retail flat 5 per jump
/// (`spaceEconomyTravelFuelCost`, `space_rules::JUMP_ENERGY`), any distance.
const JUMP_BASE_COST: f32 = crate::space_rules::JUMP_ENERGY;
const GALAXY_SEED: u64 = 0x5EED_6A1A;
/// Where `GenerateSolSystem` puts the Sol star in the original galaxy.
#[allow(dead_code)]
pub const SOL_POSITION: Vec2 = wg::SOL_POSITION;

/// Original galaxy colour map `SG_galaxy_arms_color` (256 x 256, 32-bit): it
/// spans the same +-1000 rectangle as the star map, so it lies under the stars.
pub const GALAXY_ARMS_COLOR_MAP: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0xE531_F8B0,
    type_id: 0x03E4_21ED,
};

/// A rival empire's display identity on the galaxy map: a name from the
/// tribe-name generator (ADAPTED: the retail empire name source was not
/// traced) and a political colour.
pub struct EmpireTable;

/// Every empire id of the registered galaxy and the legacy ids 0..=5.
pub static EMPIRES: EmpireTable = EmpireTable;

const POLITICAL_PALETTE: [[f32; 3]; 12] = [
    [0.95, 0.70, 0.15],
    [0.80, 0.35, 0.95],
    [0.25, 0.65, 1.00],
    [0.95, 0.45, 0.70],
    [0.55, 0.95, 0.85],
    [0.60, 0.95, 0.30],
    [1.00, 0.55, 0.20],
    [0.45, 0.50, 1.00],
    [0.90, 0.90, 0.35],
    [0.35, 0.95, 0.60],
    [0.95, 0.35, 0.50],
    [0.70, 0.75, 0.85],
];
/// `grobColor` (0.2, 0.2, 0.2) brightened to read on the dark map (ADAPTED).
const GROX_COLOR: [f32; 3] = [0.95, 0.25, 0.20];

type EmpireEntry = (&'static str, [f32; 3]);

fn empire_cache() -> &'static RwLock<HashMap<usize, &'static EmpireEntry>> {
    static CACHE: OnceLock<RwLock<HashMap<usize, &'static EmpireEntry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn fallback_names() -> &'static SpaceNames {
    static NAMES: OnceLock<SpaceNames> = OnceLock::new();
    NAMES.get_or_init(SpaceNames::builtin)
}

/// The political colour of an empire.
pub fn empire_color(id: EmpireId) -> [f32; 3] {
    if id == 0 {
        GROX_COLOR
    } else {
        POLITICAL_PALETTE[wg::mix(0xC0102, id as u64) as usize % POLITICAL_PALETTE.len()]
    }
}

/// The name of an empire (0 is the Grox).
pub fn empire_name(id: EmpireId) -> &'static str {
    EMPIRES.get(id as usize).map_or("Alien", |e| e.0)
}

impl EmpireTable {
    /// Number of empire ids in use: 0 (the Grox) up to the largest id.
    pub fn len(&self) -> usize {
        match registered_galaxy() {
            Some(g) => g.empires.iter().map(|e| e.id as usize + 1).max().unwrap_or(1),
            None => 6,
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn get(&self, id: usize) -> Option<&'static EmpireEntry> {
        if id > EmpireId::MAX as usize {
            return None;
        }
        if let Some(entry) = empire_cache().read().unwrap().get(&id) {
            return Some(*entry);
        }
        let galaxy = registered_galaxy();
        let exists = id < 6
            || galaxy
                .as_ref()
                .is_some_and(|g| g.empire_by_id(id as EmpireId).is_some());
        if !exists {
            return None;
        }
        let name: &'static str = if id == 0 {
            "Grox"
        } else {
            let seed = wg::mix(0xE4B1_5EED, id as u64);
            let name = match galaxy.as_ref() {
                Some(g) => g.names.name_for(wg::NameKind::Tribe, seed),
                None => fallback_names().name_for(wg::NameKind::Tribe, seed),
            };
            Box::leak(name.into_boxed_str())
        };
        let entry: &'static EmpireEntry = Box::leak(Box::new((name, empire_color(id as EmpireId))));
        empire_cache().write().unwrap().insert(id, entry);
        Some(entry)
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &'static EmpireEntry> + '_ {
        (0..self.len()).filter_map(|i| self.get(i))
    }
}

impl Index<usize> for EmpireTable {
    type Output = EmpireEntry;

    fn index(&self, id: usize) -> &EmpireEntry {
        self.get(id).unwrap_or_else(|| {
            // Unknown ids read as an anonymous empire (like `.get` callers do).
            Box::leak(Box::new(("Alien", [0.7, 0.7, 0.7])))
        })
    }
}

/// The planet name generator kind (for callers outside the worldgen module).
pub fn wg_name_kind_planet() -> wg::NameKind {
    wg::NameKind::Planet
}

/// Adapted starting temperature/atmosphere for a world without galaxy data
/// (a headless test world): stable across saves and platforms on a continuous
/// 0..=1 range. Worlds of the registered galaxy use their generated scores
/// (`Galaxy::world_info`).
pub fn adapted_climate_baseline(world_id: u32) -> (f32, f32) {
    if let Some(info) = world_info(world_id) {
        return (info.temperature, info.atmosphere);
    }
    fn scramble(mut x: u32) -> u32 {
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 12;
        x = x.wrapping_mul(0x297A_2D39);
        x ^ (x >> 15)
    }

    let a = scramble(world_id.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B);
    let b = scramble(world_id.wrapping_mul(0xC2B2_AE35) ^ 0x27D4_EB2F);
    // Five-hundredth steps give the adapted climate tools a usable gradient
    // while preserving a deterministic distribution around the target center.
    let temperature = (a % 21) as f32 * 0.05;
    let atmosphere = (b % 21) as f32 * 0.05;
    (temperature, atmosphere)
}

/// The generated data of one world (a terrestrial planet of a system).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct WorldInfo {
    pub system: u32,
    pub slot: u8,
    pub name: String,
    pub kind: PlanetKind,
    pub temperature: f32,
    pub atmosphere: f32,
    pub water: f32,
    pub t_score: u8,
    pub spice: Spice,
    pub hard: bool,
    pub tech: TechLevel,
    pub script: Option<(u32, u32)>,
    pub rare: bool,
}

/// The one galaxy of the process (set when a galaxy is generated for play), so
/// world-id keyed rules (spice colour, climate baseline) can look their planet
/// up. `None` in a headless run before any galaxy exists.
fn registry() -> &'static RwLock<Option<Galaxy>> {
    static REGISTRY: OnceLock<RwLock<Option<Galaxy>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

pub fn registered_galaxy() -> Option<Galaxy> {
    #[cfg(test)]
    if let Some(pinned) = PINNED_GALAXY.with(|p| p.borrow().clone()) {
        return pinned;
    }
    registry().read().unwrap().clone()
}

#[cfg(test)]
thread_local! {
    /// The galaxy this test thread sees (`None` = follow the process registry).
    static PINNED_GALAXY: std::cell::RefCell<Option<Option<Galaxy>>> = const { std::cell::RefCell::new(None) };
}

/// A test thread's own view of the registered galaxy, undone on drop. The registry is
/// process-wide and any test that enters the Space stage (`setup_space`) fills it, so a
/// test of galaxy-keyed rules (`pick_empire`, `world_info`, the climate baseline) that
/// runs in parallel with one of those sees either the headless fallback or the galaxy
/// depending on scheduling. Pinning `None` gives the headless world, `Some` a given
/// galaxy, whatever the other tests register meanwhile (they run on their own threads).
#[cfg(test)]
pub struct PinnedGalaxy;

#[cfg(test)]
pub fn pin_registered_galaxy(galaxy: Option<Galaxy>) -> PinnedGalaxy {
    PINNED_GALAXY.with(|p| *p.borrow_mut() = Some(galaxy));
    PinnedGalaxy
}

#[cfg(test)]
impl Drop for PinnedGalaxy {
    fn drop(&mut self) {
        PINNED_GALAXY.with(|p| *p.borrow_mut() = None);
    }
}

/// Make `galaxy` the process galaxy.
pub fn register_galaxy(galaxy: &Galaxy) {
    *registry().write().unwrap() = Some(galaxy.clone());
}

/// The data of a world of the registered galaxy.
pub fn world_info(world_id: u32) -> Option<WorldInfo> {
    registered_galaxy()?.world_info(world_id)
}

/// A deterministic empire for `seed` (the Monolith uplift): a random empire of
/// the registered galaxy, or one of the legacy ids 1..=5.
pub fn pick_empire(seed: u32) -> EmpireId {
    match registered_galaxy() {
        Some(g) if !g.empires.is_empty() => {
            g.empires[wg::mix(0x0E9, seed as u64) as usize % g.empires.len()].id
        }
        _ => (seed % 5 + 1) as EmpireId,
    }
}

/// The galaxy, generated once per process and shared by every view. Cloning is
/// cheap: the heavy parts are shared.
#[derive(Resource, Clone)]
pub struct Galaxy {
    pub stars: Arc<Vec<GalaxyStar>>,
    /// The raster the star placement followed, when it was installed.
    pub density_source: Option<ResourceKey>,
    /// Index of the Sol System (Earth's sun).
    pub sol_system: u32,
    /// The galactic core star.
    pub core: u32,
    /// Black holes paired as wormholes connecting distant galactic sectors.
    pub wormholes: Arc<Vec<(u32, u32)>>,
    /// The empires (the Grox, id 0, are not listed).
    pub empires: Arc<Vec<EmpireRecord>>,
    /// Stars of the Grox empire.
    pub grox_stars: u32,
    /// Worlds a new player could start on.
    pub possible_starts: Arc<Vec<u32>>,
    grid: Arc<wg::SectorGrid>,
    partners: Arc<HashMap<u32, u32>>,
    empire_index: Arc<HashMap<EmpireId, u32>>,
    empire_homes: Arc<HashSet<u32>>,
    names: Arc<SpaceNames>,
    systems: Arc<Mutex<HashMap<u32, Arc<wg::SystemRecord>>>>,
    seed: u64,
}

impl Galaxy {
    /// Deterministic galaxy. `density` are the star emit maps (the installed
    /// images, or `None` for the analytic fallback used by headless runs).
    pub fn generate(density: Option<&StarMaps>) -> Self {
        match density {
            Some(maps) => Self::generate_with(maps, SpaceNames::builtin()),
            None => {
                static ANALYTIC: OnceLock<Galaxy> = OnceLock::new();
                ANALYTIC
                    .get_or_init(|| {
                        let mut galaxy =
                            Self::generate_with(&StarMaps::analytic(), SpaceNames::builtin());
                        galaxy.density_source = None;
                        galaxy
                    })
                    .clone()
            }
        }
    }

    /// Generate with explicit name generators.
    pub fn generate_with(maps: &StarMaps, names: SpaceNames) -> Self {
        let layout = wg::generate_layout(maps, GALAXY_SEED, SOL_STAR);
        let positions: Vec<Vec2> = layout.stars.iter().map(|s| s.position).collect();
        let grid = wg::SectorGrid::build(positions.len(), |i| positions[i as usize]);
        let mut partners = HashMap::with_capacity(layout.wormholes.len() * 2);
        for &(a, b) in &layout.wormholes {
            partners.insert(a, b);
            partners.insert(b, a);
        }
        let empire_index = layout
            .empires
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i as u32))
            .collect();
        let empire_homes = layout.empires.iter().map(|e| e.home).collect();
        Self {
            stars: Arc::new(layout.stars),
            density_source: maps.installed.then_some(ResourceKey {
                group_id: 0,
                instance_id: crate::world_objects::name_id("SG_base_star_map"),
                type_id: wg::BIT_IMAGE_TYPE,
            }),
            sol_system: layout.sol,
            core: layout.core,
            wormholes: Arc::new(layout.wormholes),
            empires: Arc::new(layout.empires),
            grox_stars: layout.grox_stars,
            possible_starts: Arc::new(layout.possible_starts),
            grid: Arc::new(grid),
            partners: Arc::new(partners),
            empire_index: Arc::new(empire_index),
            empire_homes: Arc::new(empire_homes),
            names: Arc::new(names),
            systems: Arc::new(Mutex::new(HashMap::new())),
            seed: GALAXY_SEED,
        }
    }

    pub fn star(&self, index: u32) -> Option<&GalaxyStar> {
        self.stars.get(index as usize)
    }

    /// The generated name of a star (retail: `cSpaceNames` kStar / kBlackHole;
    /// Sol and the core have their own names).
    pub fn star_name(&self, index: u32) -> String {
        let Some(star) = self.star(index) else {
            return "Unknown".into();
        };
        if index == self.sol_system {
            return "Sol".into();
        }
        match star.kind {
            StarKind::GalacticCore => "Galactic Core".into(),
            StarKind::BlackHole => self
                .names
                .name_for(wg::NameKind::BlackHole, wg::mix(self.seed ^ 0xB1AC, index as u64)),
            _ => self
                .names
                .name_for(wg::NameKind::Star, wg::mix(self.seed ^ 0x57A2, index as u64)),
        }
    }

    #[allow(dead_code)]
    pub fn names(&self) -> &SpaceNames {
        &self.names
    }

    pub fn distance(&self, a: u32, b: u32) -> f32 {
        match (self.star(a), self.star(b)) {
            (Some(a), Some(b)) => a.position.distance(b.position),
            _ => f32::INFINITY,
        }
    }

    /// The star nearest to a galactic point.
    #[allow(dead_code)] // the map picks stars by `proxies::pick` now
    pub fn nearest_to(&self, point: Vec2) -> Option<u32> {
        self.grid
            .nearest(&|i| self.stars[i as usize].position, point, 4000.0, |_| true)
            .map(|(i, _)| i)
    }

    /// The stars within `radius` of star `index` (the star itself excluded),
    /// nearest first.
    pub fn stars_within(&self, index: u32, radius: f32) -> Vec<(u32, f32)> {
        let Some(origin) = self.star(index).map(|s| s.position) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        self.grid
            .for_each_within(&|i| self.stars[i as usize].position, origin, radius, |i, d| {
                if i != index {
                    found.push((i, d));
                }
            });
        found.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        found
    }

    /// The empires that own a star within `radius` of star `index` (the
    /// detection radius 5, 8 for the Grox: S3C), the star's own first, then
    /// nearest first.
    #[allow(dead_code)]
    pub fn empires_within(&self, index: u32, radius: f32) -> Vec<EmpireId> {
        let mut found: Vec<EmpireId> = Vec::new();
        if let Some(e) = self.star(index).and_then(|s| s.empire) {
            found.push(e);
        }
        for (i, _) in self.stars_within(index, radius) {
            if let Some(e) = self.stars[i as usize].empire {
                if !found.contains(&e) {
                    found.push(e);
                }
            }
        }
        found
    }

    /// The empires that notice a ship at star `index`: those with a star
    /// within the detection radius (`EMPIRE_DETECTION_RADIUS` 5 units, the
    /// Grox `GROX_DETECTION_RADIUS` 8), in the map's own units.
    pub fn empires_noticing(&self, index: u32) -> Vec<EmpireId> {
        let mut found: Vec<EmpireId> = Vec::new();
        for (i, d) in self.stars_within(index, crate::space_rules::GROX_DETECTION_RADIUS) {
            if let Some(e) = self.stars[i as usize].empire {
                let radius = if e == 0 {
                    crate::space_rules::GROX_DETECTION_RADIUS
                } else {
                    crate::space_rules::EMPIRE_DETECTION_RADIUS
                };
                if d <= radius && !found.contains(&e) {
                    found.push(e);
                }
            }
        }
        found
    }

    /// The closest star in roughly `direction` from `from` (a 45 degree cone),
    /// used to step the galaxy-map cursor with the keyboard.
    pub fn step_cursor(&self, from: u32, direction: Vec2) -> Option<u32> {
        let origin = self.star(from)?.position;
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO {
            return None;
        }
        // Search outward by growing radii; the cone of the nearest hit is small.
        for radius in [8.0f32, 20.0, 50.0, 120.0, 400.0, 1500.0] {
            let best = self
                .stars_within(from, radius)
                .into_iter()
                .filter_map(|(i, len)| {
                    let d = self.stars[i as usize].position - origin;
                    let along = d.dot(dir);
                    (len > 0.01 && along / len > 0.7).then_some((i, len / (along / len)))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = best {
                return Some(i);
            }
        }
        None
    }

    /// True if `index` is the legendary Galactic Core system.
    pub fn is_galactic_core(&self, index: u32) -> bool {
        index == self.core
    }

    /// True if `index` corresponds to the Sol system.
    pub fn is_sol_system(&self, index: u32) -> bool {
        index == self.sol_system
    }

    /// True if `index` corresponds to a wormhole / black hole system.
    pub fn is_wormhole(&self, index: u32) -> bool {
        self.partners.contains_key(&index)
    }

    /// Returns the paired exit star for a wormhole / black hole, if any.
    pub fn wormhole_partner(&self, index: u32) -> Option<u32> {
        self.partners.get(&index).copied()
    }

    /// True if `from` and `to` are connected by a wormhole bridge.
    pub fn is_wormhole_pair(&self, from: u32, to: u32) -> bool {
        self.wormhole_partner(from) == Some(to)
    }

    pub fn empire_by_id(&self, id: EmpireId) -> Option<&EmpireRecord> {
        self.empire_index.get(&id).map(|i| &self.empires[*i as usize])
    }

    /// Stars of an empire (the Grox count their 2,400).
    pub fn empire_stars(&self, id: EmpireId) -> u32 {
        if id == 0 {
            self.grox_stars
        } else {
            self.empire_by_id(id).map_or(1, |e| e.stars)
        }
    }

    /// The generated planets of a system (cached).
    pub fn system(&self, index: u32) -> Arc<wg::SystemRecord> {
        if let Some(record) = self.systems.lock().unwrap().get(&index) {
            return record.clone();
        }
        let star = self.star(index).copied();
        let ctx = wg::StarContext {
            index,
            kind: star.map_or(StarKind::Yellow, |s| s.kind),
            tech: star.map_or(TechLevel::Creature, |s| s.tech),
            empire_home: self.empire_homes.contains(&index),
            grox: star.is_some_and(|s| s.empire == Some(0)),
            rare: star.is_some_and(|s| s.rare),
            home_system: index == HOME_STAR,
        };
        let record = Arc::new(wg::generate_system(&self.names, self.seed, &ctx, None));
        let mut cache = self.systems.lock().unwrap();
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(index, record.clone());
        record
    }

    /// The generated data of a world by its WorldId (`None` for unknown ids).
    pub fn world_info(&self, world_id: u32) -> Option<WorldInfo> {
        let system = system_of_world(world_id);
        let slot = if world_id < FOREIGN_WORLD_BASE {
            world_id
        } else {
            (world_id - FOREIGN_WORLD_BASE) % WORLDS_PER_SYSTEM
        };
        if self.is_sol_system(system) {
            return sol_world_info(system, slot as u8);
        }
        let record = self.system(system);
        let planet = record
            .planets
            .iter()
            .find(|p| p.world_slot == Some(slot as u8))?;
        Some(WorldInfo {
            system,
            slot: slot as u8,
            name: planet.name.clone(),
            kind: planet.kind,
            temperature: planet.temperature,
            atmosphere: planet.atmosphere,
            water: planet.water,
            t_score: planet.t_score,
            spice: planet.spice.unwrap_or(Spice::Red),
            hard: planet.hard,
            tech: planet.tech,
            script: planet.script,
            rare: planet.rare,
        })
    }
}

/// Hand-made scores of the Sol worlds (slot: 0 Mercury, 1 Venus, 2 Earth,
/// 3 Mars, 4 Pluto): temperature and atmosphere on the 0..1 scale of the
/// terraform bullseye (ADAPTED: the retail Sol planets use art-directed
/// scripts whose water / temperature / atmosphere were not read).
fn sol_world_info(system: u32, slot: u8) -> Option<WorldInfo> {
    let (name, kind, water, temperature, atmosphere) = match slot {
        0 => ("Mercury", PlanetKind::T0, 0.0, 1.0, 0.0),
        1 => ("Venus", PlanetKind::T0, 0.0, 1.0, 1.0),
        2 => ("Earth", PlanetKind::T3, 0.6, 0.5, 0.5),
        3 => ("Mars", PlanetKind::T0, 0.1, 0.0, 0.0),
        4 => ("Pluto", PlanetKind::T0, 0.0, 0.0, 0.0),
        _ => return None,
    };
    let hard = slot != 2;
    let roll = wg::unit(wg::mix(0x501, slot as u64));
    Some(WorldInfo {
        system,
        slot,
        name: name.into(),
        kind,
        temperature,
        atmosphere,
        water,
        t_score: wg::t_score(temperature, atmosphere),
        spice: wg::assign_spice(StarKind::Yellow, hard, roll),
        hard,
        tech: TechLevel::Creature,
        script: None,
        rare: false,
    })
}

/// Energy one jump costs: 5, whatever the distance (0x1002040).
#[allow(dead_code)]
pub fn jump_cost(_distance: f32) -> f32 {
    JUMP_BASE_COST
}

/// Helper returning the WorldId of Earth in the Sol system.
pub fn earth_world_id(sol_system: u32) -> u32 {
    world_id(sol_system, 2)
}

/// Whether the ship may jump from `from` to `to` with `energy` on board and maximum drive reach `max_reach`.
pub fn check_jump_reach(
    galaxy: &Galaxy,
    from: u32,
    to: u32,
    energy: f32,
    max_reach: f32,
) -> Result<f32, String> {
    if galaxy.star(to).is_none() {
        return Err("No star selected".into());
    }
    if from == to {
        return Err(format!("You are already at {}", galaxy.star_name(to)));
    }
    // Retail (0x1002040): a jump spends 5 energy, or with an empty reactor
    // jumps anyway and the hull takes 200 damage (`SpaceCampaign::jump`).
    let cost = crate::space_rules::jump_cost(energy).0;
    // Check if traversing a paired wormhole / black hole conduit
    if galaxy.is_wormhole_pair(from, to) {
        return Ok(cost);
    }
    if max_reach <= 0.0 {
        return Err("You need an Interstellar Drive to leave this system (Story 201)".into());
    }
    let distance = galaxy.distance(from, to);
    if distance > max_reach {
        return Err(format!(
            "{} is {distance:.1} pc away; the drive reaches {max_reach:.0} pc",
            galaxy.star_name(to)
        ));
    }
    Ok(cost)
}

/// Whether the ship may jump from `from` to `to` with `energy` on board
/// (the longest reach `MAX_JUMP`). Returns the energy the jump spends.
pub fn check_jump(galaxy: &Galaxy, from: u32, to: u32, energy: f32) -> Result<f32, String> {
    check_jump_reach(galaxy, from, to, energy, MAX_JUMP)
}

/// Stable WorldId of planet `slot` in star system `system`.
pub fn world_id(system: u32, slot: u32) -> u32 {
    if system == HOME_STAR {
        slot
    } else {
        FOREIGN_WORLD_BASE + system * WORLDS_PER_SYSTEM + slot
    }
}

/// The star system a WorldId belongs to.
pub fn system_of_world(id: u32) -> u32 {
    if id < FOREIGN_WORLD_BASE {
        HOME_STAR
    } else {
        (id - FOREIGN_WORLD_BASE) / WORLDS_PER_SYSTEM
    }
}

/// One orbiting body of a star system.
#[derive(Clone, Debug)]
pub struct BodyPlan {
    /// Campaign identity. Gas giants are scenery and have none.
    pub world: Option<u32>,
    pub name: String,
    /// Orbit radius in solar-view scene units.
    pub orbit: f32,
    /// Starting angle on the orbit, radians.
    pub phase: f32,
    pub radius: f32,
    pub gas_giant: bool,
    pub moons: u8,
    /// Gas giants may carry a ring (terrestrial planets rarely: data only).
    pub ring: bool,
    // Generated planet data (S4B-S4D).
    pub kind: PlanetKind,
    pub temperature: f32,
    pub atmosphere: f32,
    pub water: f32,
    pub t_score: u8,
    pub spice: Option<Spice>,
    pub tech: TechLevel,
    /// The terrain script (group, instance) a visit will use.
    pub script: Option<(u32, u32)>,
    /// Hot or cold orbit: the spice "hard" column.
    pub hard: bool,
    pub rare: bool,
    /// The terrain script of each of its moons (the moon records of the
    /// system, in order), for `space_planets`.
    pub moon_scripts: Vec<Option<(u32, u32)>>,
}

impl BodyPlan {
    fn scenery(name: String, orbit: f32, phase: f32, radius: f32, gas_giant: bool) -> Self {
        Self {
            world: None,
            name,
            orbit,
            phase,
            radius,
            gas_giant,
            moons: 0,
            ring: false,
            kind: if gas_giant { PlanetKind::GasGiant } else { PlanetKind::T0 },
            temperature: 0.0,
            atmosphere: 0.0,
            water: 0.0,
            t_score: 0,
            spice: None,
            tech: TechLevel::None,
            script: None,
            hard: false,
            rare: false,
            moon_scripts: Vec::new(),
        }
    }
}

/// The second star of a binary system, drawn beside the first.
#[derive(Clone, Copy, Debug)]
pub struct Companion {
    pub kind: StarKind,
    pub radius: f32,
    /// Distance from the primary star's centre.
    pub offset: f32,
}

/// Everything that orbits one star.
#[derive(Clone, Debug)]
pub struct SystemPlan {
    pub star: u32,
    pub kind: StarKind,
    pub star_radius: f32,
    pub bodies: Vec<BodyPlan>,
    /// Radius of the first asteroid belt, if the system has one.
    pub belt: Option<f32>,
    /// Radii of every asteroid belt (up to two).
    pub belts: Vec<f32>,
    /// The second star of a binary.
    pub companion: Option<Companion>,
    /// Flight bound: everything of interest lies inside this radius.
    pub extent: f32,
}

/// Starting orbital angle of the homeworld, radians.
const HOME_PHASE: f32 = 0.75;

/// Angular speed of a circular orbit (Kepler-like, slow enough to dock).
pub fn orbit_speed(orbit: f32) -> f32 {
    2.2 / orbit.max(1.0).powf(1.5)
}

/// Scene orbit radius of a planet: the retail orbit (solar units) scaled to
/// the scene, never closer to the star than its surface plus a margin.
fn scene_orbit(retail_orbit: f32, planet_radius: f32, star_radius: f32) -> f32 {
    (retail_orbit * wg::SCENE_PER_SOLAR_UNIT).max(star_radius * 1.5 + planet_radius + 4.0)
}

/// The hand-made Sol system (Mercury .. Pluto; WorldIds from slots 0..=4).
fn sol_plan(index: u32) -> SystemPlan {
    let kind = StarKind::Yellow;
    let star_radius = kind.radius();
    // name, orbit, phase, radius, gas giant, moons, ring, world slot
    let table: [(&str, f32, f32, f32, bool, u8, bool, Option<u32>); 9] = [
        ("Mercury", 18.0, 0.4, 2.2, false, 0, false, Some(0)),
        ("Venus", 27.0, 1.8, 3.1, false, 0, false, Some(1)),
        ("Earth", 38.0, 3.2, 3.3, false, 1, false, Some(2)),
        ("Mars", 50.0, 4.6, 2.5, false, 2, false, Some(3)),
        ("Jupiter", 74.0, 0.9, 8.2, true, 4, false, None),
        ("Saturn", 94.0, 2.5, 7.2, true, 3, true, None),
        ("Uranus", 112.0, 4.1, 5.6, true, 2, true, None),
        ("Neptune", 128.0, 5.4, 5.4, true, 2, false, None),
        ("Pluto", 144.0, 1.2, 1.8, false, 1, false, Some(4)),
    ];
    let bodies = table
        .iter()
        .map(|&(name, orbit, phase, radius, gas, moons, ring, slot)| {
            let mut body = BodyPlan::scenery(name.into(), orbit, phase, radius, gas);
            body.moons = moons;
            body.ring = ring;
            if let Some(slot) = slot {
                body.world = Some(world_id(index, slot));
                if let Some(info) = sol_world_info(index, slot as u8) {
                    body.kind = info.kind;
                    body.temperature = info.temperature;
                    body.atmosphere = info.atmosphere;
                    body.water = info.water;
                    body.t_score = info.t_score;
                    body.spice = Some(info.spice);
                    body.hard = info.hard;
                    body.tech = info.tech;
                }
            }
            body
        })
        .collect();
    SystemPlan {
        star: index,
        kind,
        star_radius,
        bodies,
        belt: Some(62.0),
        belts: vec![62.0],
        companion: None,
        extent: 160.0,
    }
}

/// The deterministic layout of one star system. The home system's first world
/// is the homeworld (WorldId 0) named by `world_names[0]` when given.
pub fn system_plan(galaxy: &Galaxy, index: u32, world_names: &[String]) -> SystemPlan {
    if galaxy.is_sol_system(index) {
        return sol_plan(index);
    }
    let kind = galaxy
        .star(index)
        .map(|s| s.kind)
        .unwrap_or(StarKind::Yellow);
    let (primary, companion_kind) = match kind.components() {
        Some((a, b)) => (a, Some(b)),
        None => (kind, None),
    };
    let star_radius = primary.radius();
    let companion = companion_kind.map(|k| Companion {
        kind: k,
        radius: k.radius(),
        offset: star_radius + k.radius() + 3.0,
    });
    if !kind.has_planets() {
        // Core, black holes and disks have no planets.
        return SystemPlan {
            star: index,
            kind,
            star_radius: kind.radius(),
            bodies: Vec::new(),
            belt: None,
            belts: Vec::new(),
            companion: None,
            extent: kind.radius() * 4.0 + 40.0,
        };
    }
    let record = galaxy.system(index);
    let mut bodies: Vec<BodyPlan> = Vec::new();
    let mut belts: Vec<f32> = Vec::new();
    let planets = &record.planets;
    for (i, planet) in planets.iter().enumerate() {
        if planet.moon_of.is_some() {
            continue;
        }
        let clearance = star_radius + companion.map_or(0.0, |c| c.offset + c.radius);
        let orbit = scene_orbit(planet.orbit, planet.radius, clearance);
        if planet.kind == PlanetKind::AsteroidBelt {
            belts.push(orbit);
            continue;
        }
        let gas_giant = planet.kind == PlanetKind::GasGiant;
        let mut body = BodyPlan::scenery(planet.name.clone(), orbit, planet.phase, planet.radius, gas_giant);
        body.moons = record.moons_of(i).min(255) as u8;
        body.moon_scripts = record
            .planets
            .iter()
            .filter(|p| p.moon_of == Some(i))
            .map(|p| p.script)
            .collect();
        body.ring = planet.ring;
        body.kind = planet.kind;
        body.temperature = planet.temperature;
        body.atmosphere = planet.atmosphere;
        body.water = planet.water;
        body.t_score = planet.t_score;
        body.spice = planet.spice;
        body.tech = planet.tech;
        body.script = planet.script;
        body.hard = planet.hard;
        body.rare = planet.rare;
        if let Some(slot) = planet.world_slot {
            body.world = Some(world_id(index, slot as u32));
            if index == HOME_STAR && slot == 0 {
                body.name = world_names
                    .first()
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| "Homeworld".to_string());
                // The opening camera frames sun, ship and a half-lit home.
                body.phase = HOME_PHASE;
            }
        }
        bodies.push(body);
    }
    let extent = bodies
        .iter()
        .map(|b| b.orbit + b.radius)
        .chain(belts.iter().map(|b| b + 3.0))
        .fold(40.0, f32::max)
        + 14.0;
    SystemPlan {
        star: index,
        kind,
        star_radius,
        belt: belts.first().copied(),
        belts,
        bodies,
        companion,
        extent,
    }
}

// ---------------------------------------------------------------------------
// Galaxy map view
// ---------------------------------------------------------------------------

/// Render-app registration (from `space_planets::build_render`): the star glow
/// material and its shader.
pub fn build_render(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/space_stars.wesl");
    crate::world_presentation::dev_shader_override(app, "space_stars.wesl");
    stars::build(app);
}

/// Root of the galaxy map; its visibility switches the whole map.
#[derive(Component)]
pub struct GalaxyMapRoot;

/// The galaxy's colour map and nebula quads: their opacity follows the camera
/// radius (`galaxy_opacity`). `color` is the layer's colour at full opacity.
#[derive(Component)]
pub struct GalaxyDustLayer {
    pub color: [f32; 4],
}

/// `galaxyOpacityGraph` of `SpaceGalaxyEffect` (02AE0C7E!A0CB680A, property 0x195F000, VERIFIED data):
/// (galaxy camera height, opacity). 0x1034D50 (the cSpaceGfx update, every frame) evaluates it at |z| of the
/// galaxy camera's position and sets the result as parameter 4 of the effect 0x5C830352 (the "lonely stars"
/// outside the galaxy: two `particles` blocks, LOD distances 50 / 200 / 500; `cSpaceGfx+0x20`); no other
/// layer reads it. The four object scale graphs of the same property list (`starScaleGraph` 25 -> 0.95, 500 ->
/// 1.2, and the planet / moon / gas giant ones) scale the solar view's bodies (`stars::ScaleKind`, applied by
/// `space_planets::body_scale`).
pub const GALAXY_OPACITY_GRAPH: [(f32, f32); 5] =
    [(50.0, 0.0), (80.0, 0.2), (150.0, 0.5), (200.0, 1.0), (1100.0, 1.0)];

/// A piecewise-linear graph (property kind 0x30 arrays), clamped at its ends.
pub fn graph_value(graph: &[(f32, f32)], x: f32) -> f32 {
    let Some(first) = graph.first() else {
        return 1.0;
    };
    if x <= first.0 {
        return first.1;
    }
    for pair in graph.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0).max(1.0e-6);
        }
    }
    graph[graph.len() - 1].1
}

/// Opacity of the galaxy's colour map / nebulae at a galaxy camera radius. ADAPTED: retail drives only the
/// lonely-stars effect with the graph (see above, not ported: the effect is not played); the colour map and the
/// nebula quads of the earlier landings stand in for `sg_galaxy_ambience` / `sg_galaxy_screen` and keep fading
/// with it.
pub fn galaxy_opacity(radius: f32) -> f32 {
    graph_value(&GALAXY_OPACITY_GRAPH, radius)
}

/// The one mesh holding a sprite for every star of the galaxy.
#[derive(Component)]
pub struct GalaxyStarLayer;

/// Empire territory tints (one mesh per political colour).
#[derive(Component)]
pub struct GalaxyTerritoryLayer;

/// Stars picked out by the active map filter (rebuilt when it changes).
#[derive(Component)]
pub struct GalaxyHighlightLayer;

/// Map markers that follow the selection, the ship and the route.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum GalaxyMarker {
    /// Ring around the star the ship is in.
    Here,
    /// Ring around the selected star.
    Cursor,
    /// The drive's reach around the ship's star.
    Range,
    /// Line from the ship's star to the selection.
    Route,
    /// The ship in transit.
    Ship,
}

/// The player's territory glow (rebuilt as colonies spread).
#[derive(Component)]
pub struct PlayerTerritory;

/// Top-centre caption naming the current system or the map selection.
#[derive(Component)]
pub struct SpaceCaption;

/// Shared galaxy-map materials that change at run time.
#[derive(Resource, Clone)]
pub struct GalaxyMapMaterials {
    pub territory: Handle<StandardMaterial>,
    pub disc_mesh: Handle<Mesh>,
    pub route_ok: Handle<StandardMaterial>,
    pub route_blocked: Handle<StandardMaterial>,
    /// Material of the all-stars sprite layer (dimmed by a map filter; only drawn without the particle field).
    pub stars: Handle<StandardMaterial>,
    /// The retail particle field is drawn (`stars::spawn_layers`): there is then no sprite layer (retail has no
    /// star objects to draw, `proxies.rs`); without it the sprite layer is the star layer.
    pub glows: bool,
    pub highlight_mesh: Handle<Mesh>,
}

fn flat() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

fn add_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: texture,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    })
}

/// Side of a star's map sprite in galaxy units.
fn sprite_size(kind: StarKind, is_home: bool) -> f32 {
    let size = match kind {
        StarKind::GalacticCore => 9.0,
        StarKind::BlackHole => 2.6,
        StarKind::ProtoPlanetary => 1.4,
        StarKind::Blue => 1.3,
        StarKind::Yellow => 1.0,
        StarKind::Red => 0.8,
        _ => 1.5,
    };
    size * if is_home { 2.0 } else { 1.0 }
}

/// A mesh of flat quads in the galaxy plane: `(centre, side, rgba)` per quad.
fn quad_mesh(quads: impl ExactSizeIterator<Item = (Vec2, f32, [f32; 4])>, height: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let n = quads.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n * 4);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(n * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6);
    for (centre, side, color) in quads {
        let h = side * 0.5;
        let base = positions.len() as u32;
        for (dx, dz, u, v) in [
            (-h, -h, 0.0, 0.0),
            (h, -h, 1.0, 0.0),
            (h, h, 1.0, 1.0),
            (-h, h, 0.0, 1.0),
        ] {
            positions.push([centre.x + dx, height, centre.y + dz]);
            uvs.push([u, v]);
            colors.push(color);
        }
        indices.extend_from_slice(&[base, base + 3, base + 2, base, base + 2, base + 1]);
    }
    // The sprites rarely change: keep them on the GPU only.
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Index of an empire's political colour in `POLITICAL_PALETTE`.
fn palette_index(id: EmpireId) -> usize {
    let colour = empire_color(id);
    POLITICAL_PALETTE
        .iter()
        .position(|c| *c == colour)
        .unwrap_or(0)
}

/// Spawn the (hidden) galaxy map: the original colour map under every star of
/// the galaxy (one mesh of 306,301 sprites), the rival territories (one mesh
/// per political colour), and the markers.
pub fn spawn_galaxy_map(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    art: &crate::space_scene::SpaceArt,
    galaxy: &Galaxy,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let disc_mesh = quad.clone();
    let stars_material = add_material(materials, Color::WHITE, Some(art.star_sprite.image.clone()));
    let territory = add_material(
        materials,
        Color::srgba(0.25, 1.0, 0.45, 0.30),
        Some(art.disc.image.clone()),
    );
    let route_ok = add_material(
        materials,
        Color::srgba(0.45, 1.0, 0.65, 0.9),
        Some(art.route.image.clone()),
    );
    let route_blocked = add_material(
        materials,
        Color::srgba(1.0, 0.35, 0.3, 0.8),
        Some(art.route.image.clone()),
    );
    let highlight_mesh = meshes.add(quad_mesh(
        Vec::<(Vec2, f32, [f32; 4])>::new().into_iter(),
        STAR_PLANE,
    ));
    commands.insert_resource(GalaxyMapMaterials {
        territory,
        disc_mesh: disc_mesh.clone(),
        route_ok: route_ok.clone(),
        route_blocked,
        stars: stars_material.clone(),
        glows: false,
        highlight_mesh: highlight_mesh.clone(),
    });
    let size = GALAXY_MAP_HALF * 2.0;
    let disc = art.galaxy.as_ref().map(|t| t.image.clone());
    let nebulae = art.nebulae.as_ref().map(|t| t.image.clone());
    const ARMS_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.95];
    const NEBULA_COLOR: [f32; 4] = [0.75, 0.55, 0.95, 0.55];
    let layer_color = |c: [f32; 4]| Color::srgba(c[0], c[1], c[2], c[3]);
    let disc_material = add_material(materials, layer_color(ARMS_COLOR), disc.clone());
    let nebula_material =
        nebulae.map(|n| add_material(materials, layer_color(NEBULA_COLOR), Some(n)));
    let ring = meshes.add(crate::space_scene::annulus(0.86, 1.0, 64));
    let range = meshes.add(crate::space_scene::annulus(
        MAX_JUMP - 0.6,
        MAX_JUMP + 0.6,
        192,
    ));
    let ring_here = add_material(materials, Color::srgba(0.35, 0.95, 1.0, 0.95), None);
    let ring_cursor = add_material(materials, Color::srgba(1.0, 0.9, 0.45, 0.95), None);
    let ring_range = add_material(materials, Color::srgba(0.35, 0.65, 1.0, 0.35), None);
    let ship_material = add_material(
        materials,
        Color::srgba(0.7, 1.0, 1.0, 1.0),
        Some(art.flare.image.clone()),
    );
    // Territories: a soft disc per empire star in the empire's colour.
    let mut by_colour: Vec<Vec<(Vec2, f32, [f32; 4])>> =
        vec![Vec::new(); POLITICAL_PALETTE.len() + 1];
    for star in galaxy.stars.iter() {
        if let Some(e) = star.empire {
            let (slot, side) = if e == 0 {
                (POLITICAL_PALETTE.len(), 9.0)
            } else {
                (palette_index(e), 7.0)
            };
            let [r, g, b] = empire_color(e);
            by_colour[slot].push((star.position, side, [r, g, b, 0.30]));
        }
    }
    let territory_material = add_material(materials, Color::WHITE, Some(art.disc.image.clone()));
    let root = commands
        .spawn((
            StageOwned,
            GalaxyMapRoot,
            Name::new("galaxy map"),
            Transform::from_translation(GALAXY_ORIGIN),
            Visibility::Hidden,
        ))
        .with_children(|map| {
            if disc.is_some() {
                map.spawn((
                    NotShadowCaster,
                    GalaxyDustLayer { color: ARMS_COLOR },
                    Name::new("original galaxy arms"),
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(disc_material),
                    Transform::from_rotation(flat()).with_scale(Vec3::new(size, size, 1.0)),
                ));
            }
            if let Some(nebula_material) = nebula_material {
                map.spawn((
                    NotShadowCaster,
                    GalaxyDustLayer { color: NEBULA_COLOR },
                    Name::new("original galaxy nebulae"),
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(nebula_material),
                    Transform::from_xyz(0.0, 0.4, 0.0)
                        .with_rotation(flat())
                        .with_scale(Vec3::new(size * 0.74, size * 0.74, 1.0)),
                ));
            }
            for quads in by_colour {
                if quads.is_empty() {
                    continue;
                }
                map.spawn((
                    NotShadowCaster,
                    GalaxyTerritoryLayer,
                    Mesh3d(meshes.add(quad_mesh(quads.into_iter(), STAR_PLANE))),
                    MeshMaterial3d(territory_material.clone()),
                    Transform::IDENTITY,
                ));
            }
            map.spawn((
                NotShadowCaster,
                GalaxyHighlightLayer,
                Mesh3d(highlight_mesh),
                MeshMaterial3d(add_material(
                    materials,
                    Color::WHITE,
                    Some(art.star_sprite.image.clone()),
                )),
                Transform::IDENTITY,
            ));
            for (marker, mesh, material) in [
                (GalaxyMarker::Here, ring.clone(), ring_here),
                (GalaxyMarker::Cursor, ring.clone(), ring_cursor),
                (GalaxyMarker::Range, range, ring_range),
                (GalaxyMarker::Route, quad.clone(), route_ok),
                (GalaxyMarker::Ship, quad.clone(), ship_material),
            ] {
                map.spawn((
                    NotShadowCaster,
                    marker,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, STAR_PLANE, 0.0),
                    Visibility::Inherited,
                ));
            }
        })
        .id();
    // The star layer: the retail glow field (`stars`), or the sprite stand-in
    // when the glow material is not registered (headless runs). It needs the
    // world (materials, the installed rasters), so it is spawned by a command.
    let stars = galaxy.stars.clone();
    commands.queue(move |world: &mut World| {
        stars::spawn_layers(world, root, stars, stars_material);
    });
    // Caption: system name in the solar-system view, selection on the map.
    commands.spawn((
        StageOwned,
        SpaceCaption,
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.92, 1.0)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Percent(30.0),
            right: Val::Percent(30.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));
}

/// The galactic position of the ship: its star, or along the jump route.
fn ship_position(galaxy: &Galaxy, current: u32, view: &crate::space_view::SpaceView) -> Vec2 {
    match view.travel {
        Some(travel) => {
            let a = galaxy
                .star(travel.from)
                .map(|s| s.position)
                .unwrap_or_default();
            let b = galaxy
                .star(travel.to)
                .map(|s| s.position)
                .unwrap_or_default();
            let t = travel.t * travel.t * (3.0 - 2.0 * travel.t);
            a.lerp(b, t)
        }
        None => galaxy.star(current).map(|s| s.position).unwrap_or_default(),
    }
}

/// The pointer ray in the map layer's space (the layer sits at `GALAXY_ORIGIN`): origin and unit direction.
fn pointer_ray(
    cameras: &Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    cursor: Vec2,
) -> Option<(Vec3, Vec3)> {
    let (camera, at) = cameras.single().ok()?;
    let ray = camera.viewport_to_world(at, cursor).ok()?;
    Some((ray.origin - GALAXY_ORIGIN, *ray.direction))
}

/// The star objects the UFO keeps hit proxies for (0x1001200, `proxies.rs`): the saved-game stars and every star
/// within the scan distance of the ship's star. ADAPTED: the saved stars are the home star, the ship's star and the
/// visited ones.
fn pick_pool(galaxy: &Galaxy, campaign: &crate::space::SpaceCampaign) -> Vec<proxies::Candidate> {
    let current = campaign.current_system;
    let saved = |index: u32| index == HOME_STAR || index == current || campaign.visited.contains(&index);
    let mut indices: Vec<u32> = Vec::with_capacity(256);
    indices.push(current);
    indices.extend(campaign.visited.iter().copied());
    indices.push(HOME_STAR);
    let scan = hover::scan_distance(&campaign.tools);
    indices.extend(galaxy.stars_within(current, scan).into_iter().map(|(i, _)| i));
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .filter_map(|index| {
            let star = galaxy.star(index)?;
            Some(proxies::Candidate {
                index,
                position: Vec3::new(star.position.x, 0.0, star.position.y),
                core: star.kind == StarKind::GalacticCore,
                saved: saved(index),
            })
        })
        .take(proxies::POOL_SIZE)
        .collect()
}

/// The star the pointer ray picks (0xFFF780): the ray against the hit proxies of the pool, the first hit the
/// acceptance rule lets through (a saved star at any distance, any other with the camera closer than 100 units).
fn pick_star(galaxy: &Galaxy, campaign: &crate::space::SpaceCampaign, ray: (Vec3, Vec3)) -> Option<u32> {
    proxies::pick(ray.0, ray.1, &pick_pool(galaxy, campaign))
}

/// The Bevy-side handles of the star hover (`hover`): the time the last sound
/// was requested and the running effect.
#[derive(Default)]
pub struct HoverRun {
    state: hover::HoverState,
    asked: f64,
    effect: Option<Entity>,
}

/// What the star hover touches: sound requests, the voices playing, the effect.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HoverIo<'w, 's> {
    commands: Commands<'w, 's>,
    cues: MessageWriter<'w, crate::game_audio::PlayCue>,
    pause: Option<Res<'w, crate::pause_menu::PauseMenu>>,
    voices: Query<'w, 's, (Entity, &'static crate::game_audio::SfxVoice)>,
    effects: Query<'w, 's, &'static mut crate::swarm_fx::SwarmEffect>,
}

/// Development override (`OPENSPORE_HOVER_STAR=<index>` or `empire`, the
/// nearest empire star 35+ units from the ship): the pointer rests on that star, the SETI
/// tool counts as owned and the scan distance does not apply (captures have no
/// pointer).
fn hover_dev_star(galaxy: &Galaxy, current: u32) -> Option<u32> {
    static ASK: OnceLock<Option<String>> = OnceLock::new();
    let ask = ASK.get_or_init(|| std::env::var("OPENSPORE_HOVER_STAR").ok()).as_deref()?;
    if ask == "empire" {
        // The nearest one at least 35 units away (the capture's dialog covers the middle).
        return galaxy
            .stars_within(current, 90.0)
            .into_iter()
            .find(|(i, d)| *d >= 35.0 && galaxy.star(*i).is_some_and(|s| s.empire.is_some() && s.tech == TechLevel::Empire))
            .map(|(i, _)| i);
    }
    ask.parse().ok().filter(|s| galaxy.star(*s).is_some())
}

/// One frame of 0x106B340 (see `hover`): `picked` = the star the pointer ray hits (0xFFF780, `pick_star`).
fn update_star_hover(
    run: &mut HoverRun,
    io: &mut HoverIo,
    time: &Time,
    galaxy: &Galaxy,
    view: &crate::space_view::SpaceView,
    campaign: &crate::space::SpaceCampaign,
    picked: Option<u32>,
) {
    let dev = hover_dev_star(galaxy, campaign.current_system);
    let in_galaxy = view.galaxy && view.travel.is_none();
    let paused = io.pause.as_deref().is_some_and(|p| p.open);
    let gate_open = in_galaxy && !paused && (dev.is_some() || campaign.has_tool(hover::SETI_TOOL));
    let current = campaign.current_system;
    let target = if gate_open {
        (|| {
            let star = match dev {
                Some(star) => star,
                None => picked?,
            };
            let record = galaxy.star(star)?;
            let player_star = star == HOME_STAR || campaign.colonies.iter().any(|w| system_of_world(*w) == star);
            let facts = hover::StarFacts {
                kind: record.kind,
                tech: record.tech,
                player_star,
                roll: hover::star_roll(star),
            };
            Some(hover::HoverTarget {
                star,
                signal: hover::seti_signal(&facts),
                distance: if dev.is_some() { 0.0 } else { galaxy.distance(current, star) },
            })
        })()
    } else {
        None
    };
    let now = time.elapsed_secs_f64();
    let playing = run.state.sound.is_some_and(|cue| {
        io.voices.iter().any(|(_, v)| v.0 == cue) || now - run.asked < hover::SOUND_GRACE
    });
    let scan = hover::scan_distance(&campaign.tools);
    for action in hover::step(&mut run.state, gate_open, target, scan, playing) {
        debug!("space galaxy hover: {action:?} (target {target:?}, scan distance {scan})");
        match action {
            hover::HoverAction::PlaySound(cue) => {
                io.cues.write(crate::game_audio::PlayCue(cue));
                run.asked = now;
            }
            hover::HoverAction::StopSound(cue) => {
                for (entity, voice) in &io.voices {
                    if voice.0 == cue {
                        io.commands.entity(entity).despawn();
                    }
                }
                run.asked = f64::NEG_INFINITY;
            }
            hover::HoverAction::Mouseoff => {
                io.cues.write(crate::game_audio::PlayCue(crate::game_audio::Cue::StarmapFilterMouseoff));
            }
            hover::HoverAction::SpawnEffect(star) => {
                let at = galaxy.star(star).map_or(Vec2::ZERO, |s| s.position);
                let mut effect = crate::swarm_fx::SwarmEffect::new(hover::HOVER_EFFECT);
                effect.position = GALAXY_ORIGIN + Vec3::new(at.x, 0.0, at.y);
                info!(
                    "space galaxy hover: effect {:08X} on star {star} at {at:?} (focus {:?}, camera distance {})",
                    hover::HOVER_EFFECT,
                    view.galaxy_focus,
                    view.galaxy_distance
                );
                run.effect = Some(io.commands.spawn((effect, StageOwned)).id());
            }
            hover::HoverAction::StopEffect => {
                if let Some(mut effect) = run.effect.take().and_then(|e| io.effects.get_mut(e).ok()) {
                    effect.stop = true;
                }
            }
        }
    }
}

/// Galaxy-map controls: a
/// click on a star jumps to it, Ctrl + WASD/arrows step the selection, Delete
/// returns it to the ship, Enter jumps. Also advances a jump in progress
/// and, on arrival, swaps the solar system for the destination's.
pub fn galaxy_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    galaxy: Option<Res<Galaxy>>,
    view: Option<ResMut<crate::space_view::SpaceView>>,
    mut run: ResMut<crate::game::RunProgress>,
    controls: Option<Res<crate::space_controls::SpaceControls>>,
    mut hover_run: Local<HoverRun>,
    mut hover_io: HoverIo,
) {
    let (Some(galaxy), Some(mut view)) = (galaxy, view) else {
        return;
    };
    let current = run.space_campaign.current_system;
    let dt = time.delta_secs();
    // The star hover (0x106B340): the sounds and the effect of the SETI tool.
    {
        let picked = windows
            .iter()
            .find_map(|w| w.cursor_position())
            .and_then(|c| pointer_ray(&cameras, c))
            .and_then(|ray| pick_star(&galaxy, &run.space_campaign, ray));
        update_star_hover(&mut hover_run, &mut hover_io, &time, &galaxy, &view, &run.space_campaign, picked);
    }
    if let Some(mut travel) = view.travel {
        travel.t = (travel.t + dt / crate::space_view::TRAVEL_SECONDS).min(1.0);
        view.travel = Some(travel);
        view.galaxy_focus = ship_position(&galaxy, current, &view);
        if travel.t >= 1.0 {
            view.travel = None;
            view.arrival = Some(travel.to);
        }
        return;
    }
    if !view.galaxy {
        // Outside the map the selection rests on the ship's star.
        view.cursor = current;
        view.galaxy_focus = galaxy.star(current).map(|s| s.position).unwrap_or_default();
        return;
    }
    // Keyboard stepping with a short auto-repeat. ADAPTED: the original has
    // no keyboard star selection (WASD rotate the camera there), so the old
    // stepping moved to Ctrl + the movement keys.
    let held = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let mut dir = Vec2::ZERO;
    let mut fresh = false;
    if held {
        for (key, step) in [
            (KeyCode::KeyW, Vec2::NEG_Y),
            (KeyCode::ArrowUp, Vec2::NEG_Y),
            (KeyCode::KeyS, Vec2::Y),
            (KeyCode::ArrowDown, Vec2::Y),
            (KeyCode::KeyA, Vec2::NEG_X),
            (KeyCode::ArrowLeft, Vec2::NEG_X),
            (KeyCode::KeyD, Vec2::X),
            (KeyCode::ArrowRight, Vec2::X),
        ] {
            if keys.pressed(key) {
                dir += step;
            }
            fresh |= keys.just_pressed(key);
        }
    }
    view.cursor_repeat -= dt;
    if dir != Vec2::ZERO && (fresh || view.cursor_repeat <= 0.0) {
        if let Some(next) = galaxy.step_cursor(view.cursor, dir) {
            view.cursor = next;
        }
        view.cursor_repeat = if fresh { 0.35 } else { 0.12 };
    }
    // Delete returns the selection to the ship (Home is the discrete zoom).
    if keys.just_pressed(KeyCode::Delete) {
        view.cursor = current;
    }
    let mut jump = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    // A click on a star travels there as in the original (the controls tell
    // clicks on the HUD apart).
    let clicked = controls
        .as_ref()
        .map_or(mouse.just_pressed(MouseButton::Left), |c| c.click);
    if clicked {
        // 0xFDC5CD: the click takes the star 0xFFF780 picks (the hit proxies of `proxies.rs`).
        let picked = windows
            .iter()
            .find_map(|w| w.cursor_position())
            .and_then(|cursor| pointer_ray(&cameras, cursor))
            .and_then(|ray| pick_star(&galaxy, &run.space_campaign, ray));
        if let Some(star) = picked {
            view.cursor = star;
            jump = true;
        }
    }
    // Follow the selection so stepping across the galaxy keeps it in view.
    let target = galaxy
        .star(view.cursor)
        .map(|s| s.position)
        .unwrap_or_default();
    let focus = view.galaxy_focus;
    view.galaxy_focus = focus.lerp(target, 1.0 - (-3.0 * dt).exp());
    if jump {
        let campaign = &mut run.space_campaign;
        // The core limits the reach until Steve was met (0xFFC6A0).
        let reach = crate::space_endgame::jump_reach(campaign, &galaxy, current);
        match check_jump_reach(&galaxy, current, view.cursor, campaign.energy, reach) {
            Ok(_) => {
                let (cost, damage) = campaign.jump();
                let name = galaxy.star_name(view.cursor);
                if galaxy.is_wormhole_pair(current, view.cursor) {
                    campaign.message = format!("WARPING THROUGH WORMHOLE to {name}! ({cost:.0} energy)");
                } else {
                    campaign.message = format!("Jumping to {name} ({cost:.0} energy)");
                }
                if damage > 0.0 {
                    campaign.message = format!(
                        "Jumping to {name} on an empty reactor: the hull takes {damage:.0} damage"
                    );
                }
                view.travel = Some(crate::space_view::Travel {
                    from: current,
                    to: view.cursor,
                    t: 0.0,
                });
            }
            Err(reason) => campaign.message = reason,
        }
    }
}

/// Swap the loaded solar system for the destination's: despawn every body
/// of the old system (and its pirates), spawn the new one and drop the ship
/// at its edge, then zoom back in from the galaxy map.
pub fn arrive_in_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    (art, tints, galaxy, originals): (
        Option<Res<crate::space_scene::SpaceArt>>,
        Option<Res<crate::space::WorldTints>>,
        Option<Res<Galaxy>>,
        Res<crate::Originals>,
    ),
    view: Option<ResMut<crate::space_view::SpaceView>>,
    mut run: ResMut<crate::game::RunProgress>,
    old: Query<Entity, (With<crate::space::SystemBody>, Without<ChildOf>)>,
    mut player: Query<
        (&mut Transform, Option<&mut crate::space::ShipMotion>),
        With<crate::game::Player>,
    >,
) {
    let Some(mut view) = view else {
        return;
    };
    let Some(target) = view.arrival.take() else {
        return;
    };
    let (Some(galaxy), Some(tints)) = (galaxy, tints) else {
        return;
    };
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let names = crate::space::campaign_world_names(&originals);
    let plan = system_plan(&galaxy, target, &names);
    let mut campaign = std::mem::take(&mut run.space_campaign);
    let is_wormhole_transit = galaxy.is_wormhole_pair(campaign.current_system, target);
    campaign.arrive(target);
    let arrival = crate::space::spawn_system(
        &mut commands,
        &mut meshes,
        &mut materials,
        art.as_deref(),
        &tints,
        &campaign,
        &plan,
    );
    let star = galaxy.star(target);
    let star_name = galaxy.star_name(target);
    campaign.current_system_empire = star.and_then(|s| s.empire);
    let is_core = galaxy.is_galactic_core(target);
    let first_core = is_core && !campaign.reached_core;
    let empire_here = campaign.current_system_empire;
    let arrival_news = campaign.on_arrival(empire_here, is_core);
    campaign.on_arrival_near(&galaxy.empires_noticing(target));
    if first_core {
        campaign.message = arrival_news.last().cloned().unwrap_or_default();
    } else if let Some(news) = arrival_news.last() {
        campaign.message = news.clone();
    } else if is_wormhole_transit {
        campaign.message = format!(
            "WARPED THROUGH WORMHOLE! Emerged at {} ({}, {} world{}) [Wormhole Exit]",
            star_name.as_str(),
            plan.kind.label(),
            plan.bodies.iter().filter(|b| b.world.is_some()).count(),
            if plan.bodies.iter().filter(|b| b.world.is_some()).count() == 1 {
                ""
            } else {
                "s"
            }
        );
    } else if galaxy.is_sol_system(target) {
        campaign.message = format!(
            "Arrived at {} ({}, {} worlds) • [SOL SYSTEM - Cradle of Humanity]",
            star_name.as_str(),
            plan.kind.label(),
            plan.bodies.iter().filter(|b| b.world.is_some()).count(),
        );
    } else {
        campaign.message = format!(
            "Arrived at {} ({}, {} world{}){}",
            star_name.as_str(),
            plan.kind.label(),
            plan.bodies.iter().filter(|b| b.world.is_some()).count(),
            if plan.bodies.iter().filter(|b| b.world.is_some()).count() == 1 {
                ""
            } else {
                "s"
            },
            if is_core { " • [Galactic Core]" } else { "" }
        );
    }
    run.space_campaign = campaign;
    run.request_save();
    if let Ok((mut transform, motion)) = player.single_mut() {
        transform.translation = arrival;
        if let Some(mut motion) = motion {
            motion.velocity = Vec3::ZERO;
            motion.autopilot_target = None;
        }
    }
    view.cursor = target;
    view.leave_galaxy();
}

/// Galaxy-map presentation: map visibility, star sprite size against zoom,
/// markers, the player's territory, and the caption text.
pub fn present_galaxy(
    time: Res<Time>,
    view: Option<Res<crate::space_view::SpaceView>>,
    galaxy: Option<Res<Galaxy>>,
    system: Option<Res<crate::space::CurrentSystem>>,
    map_materials: Option<Res<GalaxyMapMaterials>>,
    run: Res<crate::game::RunProgress>,
    mut commands: Commands,
    mut roots: Query<(Entity, &mut Visibility), With<GalaxyMapRoot>>,
    mut markers: Query<
        (
            &GalaxyMarker,
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<GalaxyMapRoot>,
    >,
    territory: Query<Entity, With<PlayerTerritory>>,
    mut captions: Query<&mut Text, With<SpaceCaption>>,
    mut last_territory: Local<Vec<u32>>,
    mut last_filter: Local<Option<(crate::space_view::GalaxyFilter, usize, usize)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    dust: Query<(&GalaxyDustLayer, &MeshMaterial3d<StandardMaterial>), Without<GalaxyMarker>>,
) {
    let (Some(view), Some(galaxy)) = (view, galaxy) else {
        return;
    };
    let current = run.space_campaign.current_system;
    let shown = view.galaxy;
    let Ok((root, mut root_visibility)) = roots.single_mut() else {
        return;
    };
    let want = if shown {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if *root_visibility != want {
        *root_visibility = want;
    }
    // Player territory: a green glow around every system holding a colony.
    let mut systems: Vec<u32> = run
        .space_campaign
        .colonies
        .iter()
        .map(|id| system_of_world(*id))
        .collect();
    systems.sort();
    systems.dedup();
    if *last_territory != systems || territory.is_empty() {
        if let Some(materials) = map_materials.as_ref() {
            for entity in &territory {
                commands.entity(entity).despawn();
            }
            for system in &systems {
                let Some(star) = galaxy.star(*system) else {
                    continue;
                };
                let child = commands
                    .spawn((
                        PlayerTerritory,
                        NotShadowCaster,
                        Mesh3d(materials.disc_mesh.clone()),
                        MeshMaterial3d(materials.territory.clone()),
                        Transform::from_xyz(star.position.x, STAR_PLANE, star.position.y)
                            .with_rotation(flat())
                            .with_scale(Vec3::splat(44.0)),
                    ))
                    .id();
                commands.entity(root).add_child(child);
            }
            *last_territory = systems;
        }
    }
    if !shown {
        if let Ok(mut caption) = captions.single_mut() {
            let name = galaxy.star_name(current);
            let kind = system.as_ref().map(|s| s.0.kind.label()).unwrap_or("Star");
            let text = format!(
                "{name} system • {kind} • {}\nWheel / - / Home zoom out to the galaxy",
                view.zoom().label()
            );
            if caption.0 != text {
                caption.0 = text;
            }
        }
        return;
    }
    // The colour map and the nebulae fade out as the camera closes in
    // (`galaxyOpacityGraph`, ADAPTED: retail drives only the lonely-stars effect with it). `galaxyMaxZoomDustColor`
    // is the solar sky's dust (`stars::MAX_ZOOM_DUST_COLOR`): no tint here.
    let opacity = galaxy_opacity(view.galaxy_distance);
    for (layer, handle) in &dust {
        let alpha = layer.color[3] * opacity;
        let want = [layer.color[0], layer.color[1], layer.color[2], alpha];
        let current = materials.get(&handle.0).map(|m| m.base_color.to_srgba());
        if current.is_some_and(|c| {
            (c.alpha - want[3]).abs() > 1.0e-3
                || (c.red - want[0]).abs() > 1.0e-3
                || (c.green - want[1]).abs() > 1.0e-3
                || (c.blue - want[2]).abs() > 1.0e-3
        }) {
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.base_color = Color::srgba(want[0], want[1], want[2], want[3]);
            }
        }
    }
    // The sprite layer (only when the particle field is not drawn): a map filter dims it. Retail has no star objects
    // to draw, so with the particle field there is no such layer (`stars::spawn_layers`).
    if let Some(map_materials) = map_materials.as_ref().filter(|m| !m.glows) {
        let alpha = if view.filter == crate::space_view::GalaxyFilter::All {
            1.0
        } else {
            0.3
        };
        let current = materials.get(&map_materials.stars).map(|m| m.base_color.to_srgba().alpha);
        if current.is_some_and(|a| (a - alpha).abs() > 1.0e-3) {
            if let Some(mut material) = materials.get_mut(&map_materials.stars) {
                material.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
            }
        }
    }
    // Marker size follows the zoom; the star sprites keep their galaxy size.
    let scale = (view.galaxy_distance / 240.0).clamp(0.06, 4.5);
    // A map filter dims the all-stars layer and picks out its stars.
    let filter_key = (
        view.filter,
        run.space_campaign.colonies.len(),
        run.space_campaign.visited.len(),
    );
    if *last_filter != Some(filter_key) {
        *last_filter = Some(filter_key);
        if let Some(map_materials) = map_materials.as_ref() {
            let picked: Vec<u32> = match view.filter {
                crate::space_view::GalaxyFilter::All => Vec::new(),
                crate::space_view::GalaxyFilter::Colonies => last_territory.clone(),
                crate::space_view::GalaxyFilter::Empires => (0..galaxy.stars.len() as u32)
                    .filter(|i| galaxy.stars[*i as usize].empire.is_some())
                    .collect(),
                crate::space_view::GalaxyFilter::Visited => run.space_campaign.visited.clone(),
                crate::space_view::GalaxyFilter::Trade => run
                    .space_campaign
                    .trade_routes
                    .keys()
                    .map(|id| system_of_world(*id))
                    .collect(),
            };
            if let Some(mut mesh) = meshes.get_mut(&map_materials.highlight_mesh) {
                *mesh = quad_mesh(
                    picked
                        .iter()
                        .filter_map(|i| galaxy.star(*i))
                        .map(|s| {
                            let [r, g, b] = s.empire.map_or(s.kind.color(), empire_color);
                            (s.position, 3.2, [r, g, b, 1.0])
                        })
                        .collect::<Vec<_>>()
                        .into_iter(),
                    STAR_PLANE,
                );
            }
        }
    }
    let here = ship_position(&galaxy, current, &view);
    let cursor = galaxy.star(view.cursor).map(|s| s.position).unwrap_or(here);
    let pulse = 1.0 + 0.12 * (time.elapsed_secs() * 4.0).sin();
    let reachable = view.travel.is_some()
        || check_jump_reach(
            &galaxy,
            current,
            view.cursor,
            run.space_campaign.energy,
            crate::space_endgame::jump_reach(&run.space_campaign, &galaxy, current),
        )
        .is_ok();
    for (marker, mut transform, mut visibility, mut material) in &mut markers {
        let mut visible = true;
        match marker {
            GalaxyMarker::Here => {
                transform.translation = Vec3::new(here.x, STAR_PLANE, here.y);
                transform.scale = Vec3::splat(7.0 * scale);
                visible = view.travel.is_none();
            }
            GalaxyMarker::Cursor => {
                transform.translation = Vec3::new(cursor.x, STAR_PLANE, cursor.y);
                transform.scale = Vec3::splat(5.5 * scale * pulse);
                visible = view.cursor != current || view.travel.is_some();
            }
            GalaxyMarker::Range => {
                let reach_scale = crate::space_endgame::jump_reach(&run.space_campaign, &galaxy, current) / MAX_JUMP;
                transform.translation = Vec3::new(here.x, STAR_PLANE, here.y);
                transform.scale = Vec3::splat(reach_scale);
                visible = view.travel.is_none();
            }
            GalaxyMarker::Route => {
                let from = galaxy.star(current).map(|s| s.position).unwrap_or(here);
                let span = cursor - from;
                let mid = from + span * 0.5;
                transform.translation = Vec3::new(mid.x, STAR_PLANE, mid.y);
                transform.rotation = Quat::from_rotation_y(-span.y.atan2(span.x)) * flat();
                transform.scale = Vec3::new(span.length().max(0.01), 2.2 * scale, 1.0);
                visible = view.cursor != current || view.travel.is_some();
                if let Some(materials) = map_materials.as_ref() {
                    let want = if reachable {
                        &materials.route_ok
                    } else {
                        &materials.route_blocked
                    };
                    if material.0 != *want {
                        material.0 = want.clone();
                    }
                }
            }
            GalaxyMarker::Ship => {
                transform.translation = Vec3::new(here.x, STAR_PLANE, here.y);
                transform.rotation = flat();
                transform.scale = Vec3::splat(9.0 * scale * pulse);
                visible = view.travel.is_some();
            }
        }
        let want = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    if let Ok(mut caption) = captions.single_mut() {
        let text = galaxy_caption(&galaxy, &run.space_campaign, &view);
        if caption.0 != text {
            caption.0 = text;
        }
    }
}

/// What the galaxy map says about the selected star.
pub fn galaxy_caption(
    galaxy: &Galaxy,
    campaign: &crate::space::SpaceCampaign,
    view: &crate::space_view::SpaceView,
) -> String {
    let current = campaign.current_system;
    if let Some(travel) = view.travel {
        let name = galaxy.star_name(travel.to);
        return format!("Jumping to {name}…");
    }
    let Some(star) = galaxy.star(view.cursor) else {
        return "Galaxy".into();
    };
    let plan = system_plan(galaxy, view.cursor, &[]);
    let worlds = plan.bodies.iter().filter(|b| b.world.is_some()).count();
    let owner = match star.empire {
        Some(e) => {
            let rel = campaign.relation_label(e);
            let ally_tag = if campaign.allies.contains(&e) {
                " [ALLIED EMPIRE]"
            } else {
                ""
            };
            format!(" • {} empire [{rel}]{ally_tag}", empire_name(e))
        }
        None if campaign
            .colonies
            .iter()
            .any(|id| system_of_world(*id) == view.cursor) =>
        {
            " • your empire".into()
        }
        None => String::new(),
    };
    let visited = if campaign.visited.contains(&view.cursor) {
        " • visited"
    } else {
        ""
    };
    let filter_str = if view.filter != crate::space_view::GalaxyFilter::All {
        format!(" • filter: {}", view.filter.label())
    } else {
        String::new()
    };
    let core_str = if galaxy.is_galactic_core(view.cursor) {
        " • [GALACTIC CORE]"
    } else {
        ""
    };
    let sol_str = if galaxy.is_sol_system(view.cursor) {
        " • [SOL SYSTEM - HOME OF HUMANITY]"
    } else {
        ""
    };
    let wormhole_str = if let Some(exit) = galaxy.wormhole_partner(view.cursor) {
        let exit_name = galaxy.star_name(exit);
        format!(" • [BLACK HOLE WORMHOLE -> {exit_name}]")
    } else {
        String::new()
    };
    let sos_str = if let Some(ref d) = campaign.active_distress {
        if d.system_id == view.cursor {
            match d.kind {
                crate::space::DistressKind::PirateRaid => {
                    format!(
                        " • [! SOS: PIRATE RAID ({:.0}s) !]",
                        d.time_remaining.max(0.0)
                    )
                }
                crate::space::DistressKind::BiohazardOutbreak => {
                    format!(
                        " • [! SOS: BIOHAZARD ({:.0}s) !]",
                        d.time_remaining.max(0.0)
                    )
                }
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let mission_str = if let Some(ref m) = campaign.active_alien_mission {
        if m.target_world.map_or(false, |wid| system_of_world(wid) == view.cursor) {
            format!(" • [MISSION TARGET: {}]", m.kind.title())
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let line = format!(
        "{} • {} • {worlds} world{}{owner}{visited}{filter_str}{core_str}{sol_str}{wormhole_str}{sos_str}{mission_str}",
        galaxy.star_name(view.cursor),
        star.kind.label(),
        if worlds == 1 { "" } else { "s" }
    );
    let action = if view.cursor == current {
        "You are here • Click a star to jump • Ctrl+WASD pick • A D rotate, W S tilt • Tab filter • Backspace home • Enter jump • Wheel / End zoom in".to_string()
    } else if galaxy.is_wormhole_pair(current, view.cursor) {
        format!(
            "Wormhole Conduit • Enter to transit ({:.0} energy) • Tab filter",
            JUMP_BASE_COST
        )
    } else {
        let distance = galaxy.distance(current, view.cursor);
        match check_jump_reach(
            galaxy,
            current,
            view.cursor,
            campaign.energy,
            crate::space_endgame::jump_reach(campaign, galaxy, current),
        ) {
            Ok(cost) => format!(
                "{distance:.0} pc • Enter to jump ({cost:.0} of {:.0} energy) • Tab filter",
                campaign.energy
            ),
            Err(reason) => reason,
        }
    };
    format!("{line}\n{action}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_catalog::ResourceIndex;
    use crate::space_worldgen::EmpireArchetype;

    /// Test rig of the star hover: the system with a fixed pointer ray (map space: origin, direction), and the
    /// cues it asked for.
    #[derive(Resource)]
    struct HoverProbe {
        pointer: Option<(Vec3, Vec3)>,
    }

    #[derive(Resource, Default)]
    struct CueLog(Vec<crate::game_audio::Cue>);

    fn hover_probe(
        mut run: Local<HoverRun>,
        mut io: HoverIo,
        time: Res<Time>,
        galaxy: Res<Galaxy>,
        view: Res<crate::space_view::SpaceView>,
        campaign: Res<crate::space::SpaceCampaign>,
        probe: Res<HoverProbe>,
    ) {
        let picked = probe.pointer.and_then(|ray| pick_star(&galaxy, &campaign, ray));
        update_star_hover(&mut run, &mut io, &time, &galaxy, &view, &campaign, picked);
    }

    fn log_cues(mut reader: MessageReader<crate::game_audio::PlayCue>, mut log: ResMut<CueLog>) {
        log.0.extend(reader.read().map(|c| c.0));
    }

    /// A world with the galaxy map shown, the ship at `ship` and the SETI tool when asked.
    fn hover_app(galaxy: &Galaxy, ship: u32, seti: bool, pointer_star: u32) -> App {
        use crate::game_audio::PlayCue;
        let mut app = App::new();
        app.add_message::<PlayCue>()
            .init_resource::<Time>()
            .init_resource::<CueLog>()
            .insert_resource(galaxy.clone());
        let mut view = crate::space_view::SpaceView::default();
        view.galaxy = true;
        app.insert_resource(view);
        let mut campaign = crate::space::SpaceCampaign::default();
        campaign.current_system = ship;
        if seti {
            campaign.tools.push(hover::SETI_TOOL.to_string());
        }
        app.insert_resource(campaign);
        let at = galaxy.star(pointer_star).unwrap().position;
        let eye = Vec3::new(at.x, 50.0, at.y);
        app.insert_resource(HoverProbe { pointer: Some((eye, Vec3::NEG_Y)) });
        app.add_systems(Update, (hover_probe, log_cues).chain());
        app
    }

    fn effects(app: &mut App) -> Vec<(u32, Vec3, bool)> {
        let mut q = app.world_mut().query::<&crate::swarm_fx::SwarmEffect>();
        q.iter(app.world()).map(|e| (e.effect, e.position, e.stop)).collect()
    }

    #[test]
    fn the_star_hover_plays_the_seti_sounds_and_effect_and_the_mouseoff() {
        use crate::game_audio::Cue;
        let galaxy = Galaxy::generate(None);
        let empire = (0..galaxy.stars.len() as u32)
            .find(|i| {
                *i != HOME_STAR
                    && galaxy.star(*i).is_some_and(|s| {
                        s.empire.is_some() && s.tech == TechLevel::Empire && !matches!(s.kind, StarKind::BlackHole)
                    })
            })
            .expect("an empire star");
        // Without the SETI tool nothing happens at all.
        let mut app = hover_app(&galaxy, empire, false, empire);
        app.update();
        assert!(app.world().resource::<CueLog>().0.is_empty());
        assert!(effects(&mut app).is_empty());
        // With it: the `space` sound and the effect on the empire star.
        let mut app = hover_app(&galaxy, empire, true, empire);
        app.update();
        assert_eq!(app.world().resource::<CueLog>().0, [Cue::StarmapFilterSpace]);
        let at = galaxy.star(empire).unwrap().position;
        let fx = effects(&mut app);
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0].0, hover::HOVER_EFFECT);
        assert!((fx[0].1 - (GALAXY_ORIGIN + Vec3::new(at.x, 0.0, at.y))).length() < 1e-3);
        // The sound still counts as playing: nothing new.
        app.update();
        assert_eq!(app.world().resource::<CueLog>().0, [Cue::StarmapFilterSpace]);
        assert_eq!(effects(&mut app).len(), 1);
        // The pointer leaves every star: mouse-off, and the effect is stopped.
        app.world_mut().resource_mut::<HoverProbe>().pointer = None;
        app.update();
        assert_eq!(
            app.world().resource::<CueLog>().0,
            [Cue::StarmapFilterSpace, Cue::StarmapFilterMouseoff]
        );
        assert!(effects(&mut app).iter().all(|e| e.2), "the effect is stopped");
    }

    #[test]
    fn the_star_hover_of_an_ordinary_star_plays_the_standard_sound_only() {
        use crate::game_audio::Cue;
        let galaxy = Galaxy::generate(None);
        // A red / yellow star without an empire.
        let plain = (1..galaxy.stars.len() as u32)
            .find(|i| {
                galaxy.star(*i).is_some_and(|s| s.empire.is_none() && s.tech != TechLevel::Empire && matches!(s.kind, StarKind::Red))
            })
            .expect("a plain star");
        let mut app = hover_app(&galaxy, plain, true, plain);
        app.update();
        assert_eq!(app.world().resource::<CueLog>().0, [Cue::StarmapFilterStandard]);
        assert!(effects(&mut app).is_empty());
    }

    #[test]
    fn the_star_hover_ignores_stars_beyond_the_camera_range_and_the_scan_distance() {
        let galaxy = Galaxy::generate(None);
        let empire = (0..galaxy.stars.len() as u32)
            .find(|i| *i != HOME_STAR && galaxy.star(*i).is_some_and(|s| s.empire.is_some() && s.tech == TechLevel::Empire))
            .unwrap();
        // The camera 150 units above the star and the star not a saved one: no pick.
        let mut app = hover_app(&galaxy, empire, true, empire);
        let at = galaxy.star(empire).unwrap().position;
        app.world_mut().resource_mut::<HoverProbe>().pointer = Some((Vec3::new(at.x, 150.0, at.y), Vec3::NEG_Y));
        app.world_mut().resource_mut::<crate::space::SpaceCampaign>().current_system = HOME_STAR + 1;
        app.update();
        // Ship far from the star: also beyond the scan distance of 5 (no drive).
        let far = galaxy.distance(HOME_STAR + 1, empire);
        assert!(app.world().resource::<CueLog>().0.is_empty(), "camera {far:.0} away from the ship's star");
        assert!(effects(&mut app).is_empty());
    }

    #[test]
    fn galaxy_colour_map_fades_with_the_authored_opacity_graph() {
        // SpaceGalaxyEffect galaxyOpacityGraph: 0 at 50, 0.2 at 80, 0.5 at
        // 150, 1 from 200 on.
        assert_eq!(galaxy_opacity(5.0), 0.0);
        assert_eq!(galaxy_opacity(50.0), 0.0);
        assert!((galaxy_opacity(65.0) - 0.1).abs() < 1e-6);
        assert!((galaxy_opacity(80.0) - 0.2).abs() < 1e-6);
        assert!((galaxy_opacity(150.0) - 0.5).abs() < 1e-6);
        assert!((galaxy_opacity(175.0) - 0.75).abs() < 1e-6);
        assert_eq!(galaxy_opacity(200.0), 1.0);
        assert_eq!(galaxy_opacity(1200.0), 1.0);
    }

    #[test]
    fn the_pointer_picks_through_the_hit_proxies_of_the_pool() {
        // 0x1001200 / 0xFFF780 (`proxies.rs`): the pool is the saved stars plus the stars within the scan
        // distance of the ship's star; a ray picks a star it passes through, a saved one at any distance and
        // any other only with the camera closer than 100 units.
        let galaxy = Galaxy::generate(None);
        let mut campaign = crate::space::SpaceCampaign::default();
        campaign.current_system = HOME_STAR;
        campaign.visited.clear();
        campaign.tools = vec!["interstellardrive".to_string()];
        let pool = pick_pool(&galaxy, &campaign);
        let expected = galaxy.stars_within(HOME_STAR, 20.0).len() + 1;
        assert_eq!(pool.len(), expected);
        assert!(pool.iter().any(|c| c.index == HOME_STAR && c.saved));
        // A neighbour with no other star within a unit and out of the home star's proxy at 150 units (2.25), so
        // the rays below hit nothing else.
        let neighbour = galaxy
            .stars_within(HOME_STAR, 20.0)
            .into_iter()
            .map(|(i, _)| i)
            .find(|i| galaxy.distance(HOME_STAR, *i) > 4.0 && galaxy.stars_within(*i, 1.0).is_empty())
            .expect("an isolated neighbour in reach");
        assert!(pool.iter().any(|c| c.index == neighbour && !c.saved));
        // Without an interstellar drive the scan distance is 5.
        campaign.tools.clear();
        let small = pick_pool(&galaxy, &campaign);
        assert_eq!(small.len(), galaxy.stars_within(HOME_STAR, 5.0).len() + 1);
        // A ray straight down onto the neighbour, from 30 units up: picked.
        let at = galaxy.star(neighbour).unwrap().position;
        let target = Vec3::new(at.x, 0.0, at.y);
        let eye = target + Vec3::new(0.0, 30.0, 0.0);
        campaign.tools = vec!["interstellardrive".to_string()];
        assert_eq!(pick_star(&galaxy, &campaign, (eye, Vec3::NEG_Y)), Some(neighbour));
        // From 150 units up the same star is out of the acceptance range (not saved) ...
        let far = target + Vec3::new(0.0, 150.0, 0.0);
        assert_eq!(pick_star(&galaxy, &campaign, (far, Vec3::NEG_Y)), None);
        // ... while the home star (saved) is still picked from there.
        let home = galaxy.star(HOME_STAR).unwrap().position;
        let home_eye = Vec3::new(home.x, 150.0, home.y);
        assert_eq!(pick_star(&galaxy, &campaign, (home_eye, Vec3::NEG_Y)), Some(HOME_STAR));
        // A visited neighbour counts as saved.
        campaign.visited.push(neighbour);
        assert_eq!(pick_star(&galaxy, &campaign, (far, Vec3::NEG_Y)), Some(neighbour));
    }

    #[test]
    fn the_star_layers_sit_on_the_plane_the_camera_looks_at() {
        // The galaxy camera's target is at y = 0 (`space_view`): the stars and the markers are on that plane.
        assert_eq!(STAR_PLANE, 0.0);
    }

    #[test]
    fn galaxy_is_the_retail_galaxy_and_home_has_neighbours_in_reach() {
        let a = Galaxy::generate(None);
        let b = Galaxy::generate(None);
        assert_eq!(a.stars.len(), GALAXY_STARS);
        assert_eq!(GALAXY_STARS, 306_301);
        assert_eq!(a.stars.len(), b.stars.len());
        for i in (0..a.stars.len()).step_by(499) {
            assert_eq!(a.stars[i].position, b.stars[i].position);
            assert_eq!(a.stars[i].kind, b.stars[i].kind);
            assert_eq!(a.star_name(i as u32), b.star_name(i as u32));
        }
        let home = a.star(HOME_STAR).unwrap();
        assert_eq!(home.kind, StarKind::Yellow);
        assert_eq!(home.tech, TechLevel::Creature);
        assert!(home.empire.is_none(), "home is never rival territory");
        let reachable = a
            .stars_within(HOME_STAR, MAX_JUMP)
            .into_iter()
            .count();
        assert!(reachable >= 6, "only {reachable} stars within the last drive's reach");
        assert!(a.stars.iter().any(|s| s.empire.is_some()));
        for star in a.stars.iter() {
            assert!(star.position.length() <= GALAXY_RADIUS * 1.5);
        }
        // The core, the Grox and the empires.
        assert_eq!(a.star(a.core).unwrap().kind, StarKind::GalacticCore);
        assert!(a.is_galactic_core(a.core));
        assert!(a.grox_stars > 1000);
        assert!(a.empires.len() > 100);
        assert_eq!(a.star_name(a.core), "Galactic Core");
    }

    #[test]
    fn jumps_cost_five_energy_need_a_drive_and_respect_its_range() {
        let galaxy = Galaxy::generate(None);
        let near = galaxy.stars_within(HOME_STAR, MAX_JUMP)[0].0;
        let cost = check_jump(&galaxy, HOME_STAR, near, 120.0).expect("neighbour in reach");
        assert_eq!(cost, crate::space_rules::JUMP_ENERGY, "5 energy, any distance");
        // An empty reactor still jumps (the hull pays 200: SpaceCampaign::jump).
        assert_eq!(check_jump(&galaxy, HOME_STAR, near, 0.0), Ok(0.0));
        assert!(
            check_jump_reach(&galaxy, HOME_STAR, near, 120.0, 0.0).is_err(),
            "no interstellar drive, no jump"
        );
        assert!(check_jump(&galaxy, HOME_STAR, HOME_STAR, 120.0).is_err());
        let far = (1..galaxy.stars.len() as u32)
            .find(|i| galaxy.distance(HOME_STAR, *i) > MAX_JUMP * 2.0)
            .unwrap();
        assert!(
            check_jump(&galaxy, HOME_STAR, far, 120.0).is_err(),
            "range gates jumps"
        );
        // Retail has no free trip home: the range applies everywhere.
        assert!(check_jump(&galaxy, far, HOME_STAR, 120.0).is_err());
        // The first drive reaches 3 galaxy units, in the map's own units.
        assert_eq!(crate::space_rules::MAP_UNITS_PER_GALAXY_UNIT, 1.0);
    }

    #[test]
    fn home_system_has_the_homeworld_first_and_worlds_to_visit() {
        let galaxy = Galaxy::generate(None);
        let plan = system_plan(&galaxy, HOME_STAR, &["Gaia".to_string()]);
        let mut ids: Vec<u32> = plan.bodies.iter().filter_map(|b| b.world).collect();
        ids.sort();
        assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>(), "ids 0.. in order");
        assert!(ids.len() >= 4, "the tutorial needs worlds to visit");
        let home = plan.bodies.iter().find(|b| b.world == Some(0)).unwrap();
        assert_eq!(home.name, "Gaia");
        assert_eq!(home.kind, PlanetKind::T3);
        assert!(plan.bodies.iter().all(|b| b.orbit + b.radius < plan.extent));
        assert!(plan
            .bodies
            .iter()
            .all(|b| b.orbit - b.radius > plan.star_radius));
        // Without a hint the homeworld is still named.
        let unnamed = system_plan(&galaxy, HOME_STAR, &[]);
        assert_eq!(unnamed.bodies.iter().find(|b| b.world == Some(0)).unwrap().name, "Homeworld");
        assert_eq!(system_of_world(5), HOME_STAR);
    }

    #[test]
    fn foreign_systems_follow_the_retail_planet_rules() {
        let galaxy = Galaxy::generate(None);
        let mut worlds = 0;
        for index in (1..galaxy.stars.len() as u32).step_by(997) {
            let kind = galaxy.star(index).unwrap().kind;
            let plan = system_plan(&galaxy, index, &[]);
            let again = system_plan(&galaxy, index, &[]);
            assert_eq!(plan.bodies.len(), again.bodies.len());
            if !kind.has_planets() {
                assert!(plan.bodies.is_empty(), "{kind:?} has no planets");
                continue;
            }
            assert!(plan.belts.len() <= 2);
            assert!(plan.bodies.iter().filter(|b| b.gas_giant).count() <= 3);
            if index == SOL_STAR {
                continue;
            }
            for body in &plan.bodies {
                if let Some(id) = body.world {
                    assert!(id >= FOREIGN_WORLD_BASE);
                    assert_eq!(system_of_world(id), index);
                    assert!(body.spice.is_some());
                    worlds += 1;
                }
            }
            // The first body of every system is a rocky world.
            assert!(plan.bodies[0].world.is_some(), "star {index}");
            assert_eq!(plan.kind, kind);
        }
        assert!(worlds > 100, "{worlds} worlds");
    }

    #[test]
    fn world_data_comes_from_the_generated_planets() {
        let galaxy = Galaxy::generate(None);
        let index = 1234u32;
        let plan = system_plan(&galaxy, index, &[]);
        for body in plan.bodies.iter().filter(|b| b.world.is_some()) {
            let info = galaxy.world_info(body.world.unwrap()).expect("world data");
            assert_eq!(info.kind, body.kind);
            assert_eq!(info.temperature, body.temperature);
            assert_eq!(info.atmosphere, body.atmosphere);
            assert_eq!(info.t_score, crate::space_worldgen::t_score(info.temperature, info.atmosphere));
            assert_eq!(Some(info.spice), body.spice);
        }
        assert!(galaxy.world_info(FOREIGN_WORLD_BASE + 15 * WORLDS_PER_SYSTEM + 15).is_none());
    }

    #[test]
    fn keyboard_cursor_steps_toward_the_requested_direction() {
        let galaxy = Galaxy::generate(None);
        let origin = galaxy.star(HOME_STAR).unwrap().position;
        for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let next = galaxy.step_cursor(HOME_STAR, dir).expect("a star that way");
            let d = galaxy.star(next).unwrap().position - origin;
            assert!(d.normalize().dot(dir) > 0.7);
        }
        // The nearest star to a star's own position is that star.
        let p = galaxy.star(777).unwrap().position;
        assert_eq!(galaxy.nearest_to(p).map(|i| galaxy.star(i).unwrap().position), Some(p));
    }

    #[test]
    fn stars_are_named_by_the_lexicon_generators() {
        let galaxy = Galaxy::generate(None);
        let names: std::collections::HashSet<String> =
            (0..300u32).map(|i| galaxy.star_name(i)).collect();
        assert!(names.len() > 150, "{} distinct names", names.len());
        assert!(names.contains("Sol"));
        for name in &names {
            assert!(!name.is_empty() && name != "Unknown");
        }
        let hole = galaxy.wormholes[0].0;
        assert!(!galaxy.star_name(hole).is_empty());
    }

    #[test]
    fn empire_identities_cover_every_empire_id() {
        let galaxy = Galaxy::generate(None);
        assert_eq!(empire_name(0), "Grox");
        assert_eq!(EMPIRES[0].1, empire_color(0));
        for empire in galaxy.empires.iter().take(50) {
            // Every empire has a record, a stable colour from the palette and
            // the archetype its id encodes.
            assert_eq!(galaxy.empire_by_id(empire.id).map(|e| e.home), Some(empire.home));
            assert_eq!(empire_color(empire.id), empire_color(empire.id));
            assert_eq!(
                EmpireArchetype::from_residue(empire.id as u32 % 10),
                Some(empire.archetype)
            );
        }
        assert_eq!(galaxy.empire_stars(0), galaxy.grox_stars);
        assert!(pick_empire(7) != 0);
    }

    fn write_png(path: &std::path::Path, pixels: &crate::texture::Pixels) {
        let file = std::fs::File::create(path).expect("create raster preview");
        let mut encoder = png::Encoder::new(file, pixels.width, pixels.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("PNG header")
            .write_image_data(&pixels.rgba)
            .expect("PNG pixels");
    }

    #[test]
    #[ignore = "reads named installed Space rasters and writes ignored previews"]
    fn export_named_space_rasters_for_inspection() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let output =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/space-asset-previews");
        std::fs::create_dir_all(&output).unwrap();
        let names = std::env::var("SPACE_RASTERS").unwrap_or_default();
        for name in names.split(',').filter(|n| !n.is_empty()) {
            let id = crate::world_objects::name_id(name);
            let keys: Vec<_> = index
                .records
                .keys()
                .filter(|k| k.instance_id == id)
                .copied()
                .collect();
            if keys.is_empty() {
                println!("{name} {id:08X}: no record");
            }
            for key in keys {
                let bytes = index.read(key).unwrap_or_default();
                let decoded = crate::texture::decode_rw4(&bytes)
                    .or_else(|| crate::texture::decode_rw4_mips(&bytes)?.into_iter().next());
                match decoded {
                    Some(p) => {
                        write_png(&output.join(format!("{name}.png")), &p);
                        println!(
                            "{name} {:08X}/{:08X}/{:08X}: {}x{} raster ({} bytes)",
                            key.group_id,
                            key.instance_id,
                            key.type_id,
                            p.width,
                            p.height,
                            bytes.len()
                        );
                    }
                    None => println!(
                        "{name} {:08X}/{:08X}/{:08X}: {} bytes, not a raster",
                        key.group_id,
                        key.instance_id,
                        key.type_id,
                        bytes.len()
                    ),
                }
            }
        }
    }

    /// Statistics of the galaxy built from the installed emit maps and
    /// lexicons (`--ignored --nocapture`).
    #[test]
    #[ignore = "reads the installed Spore resources"]
    fn installed_galaxy_statistics() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let read = |group: u32, instance: u32, type_id: u32| {
            index
                .read(ResourceKey {
                    group_id: group,
                    instance_id: instance,
                    type_id,
                })
                .ok()
        };
        let maps = StarMaps::from_install(read).expect("installed star maps");
        let names = SpaceNames::from_install(read).expect("installed lexicons");
        let started = std::time::Instant::now();
        let galaxy = Galaxy::generate_with(&maps, names);
        println!("generated {} stars in {:?}", galaxy.stars.len(), started.elapsed());
        assert_eq!(galaxy.stars.len(), GALAXY_STARS);
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        for s in galaxy.stars.iter() {
            *counts.entry(s.kind.label()).or_default() += 1;
        }
        println!("kinds {counts:?}");
        let mut tech: std::collections::BTreeMap<&str, usize> = Default::default();
        for s in galaxy.stars.iter().filter(|s| s.kind.has_planets()) {
            *tech.entry(s.tech.label()).or_default() += 1;
        }
        println!("tech {tech:?}");
        println!(
            "empires {} (Grox stars {}), possible starts {}, wormholes {}",
            galaxy.empires.len(),
            galaxy.grox_stars,
            galaxy.possible_starts.len(),
            galaxy.wormholes.len()
        );
        let sizes: Vec<u32> = galaxy.empires.iter().map(|e| e.stars).collect();
        println!(
            "empire stars total {} mean {:.2}",
            sizes.iter().sum::<u32>(),
            sizes.iter().sum::<u32>() as f32 / sizes.len().max(1) as f32
        );
        let home = galaxy.star(HOME_STAR).unwrap();
        println!(
            "home {} at {:?} radius {:.0} kind {:?}",
            galaxy.star_name(HOME_STAR),
            home.position,
            home.position.length(),
            home.kind
        );
        for reach in [3.0f32, 5.0, 8.0, 12.0, 20.0] {
            println!(
                "stars within {reach}: {}",
                galaxy.stars_within(HOME_STAR, reach).len()
            );
        }
        println!("empires noticing home: {:?}", galaxy.empires_noticing(HOME_STAR));
        let sol = galaxy.star(SOL_STAR).unwrap();
        println!("sol at {:?}", sol.position);
        let plan = system_plan(&galaxy, HOME_STAR, &[]);
        for b in &plan.bodies {
            println!(
                "  {:>10} {:?} world {:?} orbit {:.1} moons {} T{} temp {:.2} atm {:.2} spice {:?}",
                b.name, b.kind, b.world, b.orbit, b.moons, b.t_score, b.temperature, b.atmosphere, b.spice
            );
        }
        for i in [1u32, 2, 3, 100, 1000, 20000, 150000, 250000] {
            let p = system_plan(&galaxy, i, &[]);
            println!(
                "star {i} {} ({}): {} bodies, {} belts",
                galaxy.star_name(i),
                p.kind.label(),
                p.bodies.len(),
                p.belts.len()
            );
        }
    }

    #[test]
    fn sol_system_generation_and_bodies() {
        let galaxy = Galaxy::generate(None);
        assert_eq!(galaxy.sol_system, SOL_STAR);
        assert!(galaxy.is_sol_system(SOL_STAR));
        let sol = galaxy.star(SOL_STAR).expect("sol star exists");
        assert_eq!(galaxy.star_name(SOL_STAR), "Sol");
        assert_eq!(sol.kind, StarKind::Yellow);
        assert_eq!(sol.empire, None);

        let plan = system_plan(&galaxy, SOL_STAR, &[]);
        assert_eq!(plan.bodies.len(), 9);
        let names: Vec<&str> = plan.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Mercury", "Venus", "Earth", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune",
                "Pluto"
            ]
        );

        let earth = plan.bodies.iter().find(|b| b.name == "Earth").unwrap();
        assert_eq!(earth.world, Some(earth_world_id(SOL_STAR)));
        assert_eq!(earth.moons, 1);
        assert!(!earth.gas_giant);
        assert_eq!(earth.t_score, 3, "Earth is a full T3 world");

        let mars = plan.bodies.iter().find(|b| b.name == "Mars").unwrap();
        assert_eq!(mars.moons, 2);

        let jupiter = plan.bodies.iter().find(|b| b.name == "Jupiter").unwrap();
        assert!(jupiter.gas_giant);
        assert_eq!(jupiter.world, None);
        assert_eq!(jupiter.moons, 4);

        let saturn = plan.bodies.iter().find(|b| b.name == "Saturn").unwrap();
        assert!(saturn.gas_giant);
        assert!(saturn.ring);

        let pluto = plan.bodies.iter().find(|b| b.name == "Pluto").unwrap();
        assert!(!pluto.gas_giant);
        assert_eq!(pluto.moons, 1);

        assert_eq!(plan.belt, Some(62.0));
        assert_eq!(plan.belts, vec![62.0]);
    }

    #[test]
    fn wormhole_pairs_and_instantaneous_transit() {
        let galaxy = Galaxy::generate(None);
        assert_eq!(galaxy.wormholes.len(), 1_200);

        for &(a, b) in galaxy.wormholes.iter().step_by(37) {
            assert_ne!(a, b);
            assert_ne!(a, HOME_STAR);
            assert_ne!(b, HOME_STAR);
            assert_ne!(a, SOL_STAR);
            assert_ne!(b, SOL_STAR);
            assert_eq!(galaxy.star(a).unwrap().kind, StarKind::BlackHole);

            assert!(galaxy.is_wormhole(a));
            assert!(galaxy.is_wormhole(b));
            assert_eq!(galaxy.wormhole_partner(a), Some(b));
            assert_eq!(galaxy.wormhole_partner(b), Some(a));
            assert!(galaxy.is_wormhole_pair(a, b));
            assert!(galaxy.is_wormhole_pair(b, a));

            // Drive reach is only 10 pc, but wormhole transit succeeds with the flat 5
            let cost = check_jump_reach(&galaxy, a, b, 120.0, 10.0).expect("wormhole jump ok");
            assert_eq!(cost, JUMP_BASE_COST);

            // An empty reactor transits too (the jump damages the hull instead).
            assert_eq!(check_jump_reach(&galaxy, a, b, 0.0, 10.0), Ok(0.0));
        }
        // A black hole has no planets.
        let hole = galaxy.wormholes[0].0;
        assert!(system_plan(&galaxy, hole, &[]).bodies.is_empty());
    }
}
