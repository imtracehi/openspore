//! Tool artifacts lying on planets (helper `space-final4`,
//! `notes/space-final4-notes.md` T1-T3): the universe simulator's artifact
//! routine, the discoverable tool pick and the pickup with the tractor beam.
//!
//! Original code (VERIFIED, SDK / march2017 addresses):
//! * The universe simulator (`cSimulatorUniverse`, ctor 0x1015390) builds an
//!   artifact routine (+0xD0, vtable 0x14955E8, id 0xEEC211A9) with its action
//!   (+0xD8, vtable 0x14956C0 -> 0x10105C0 -> 0x100F660). It is not one of the
//!   nine disaster routines of the vector map +0xF8; the update 0x1014CD0 runs
//!   it every frame through 0x10105D0: `p = GetProbability(dt)`, and when
//!   `rand < p` Execute.
//! * GetProbability 0x100E3F0: the routine's clock (+4) gains dt; past
//!   `universeSimulatorArtifactMinTime` (60 s) the chance this frame is
//!   `1 - exp(-0.30103 x dt / universeSimulatorArtifactFrequency)` (180 s; the
//!   constant is log10(2) as compiled), else 0.
//! * Execute 0x1010FC0: the clock restarts; FindStars within 4 units of the
//!   active star (star types 0x1FF1: no galactic core, black hole or
//!   proto-planetary disk; tech levels 0x3E: a star with life); one at random;
//!   its planets from a random start, cyclically, the first that is not an
//!   asteroid belt or gas giant gets 0x100F660.
//! * 0x100F660(planet): the planet's tool objects (cPlanetObjectData type 2
//!   with flag 8) below `universeSimulatorMaxArtifactsPerPlanet` (1): a tool
//!   from 0x10513B0 with the count 0x104E480 is added (0xC73340, an unset
//!   position = a random land point when the planet is visited).
//! * 0x10513B0: ToolConfig 0x59252EE9 `spaceDiscoverableTools` (70 tools),
//!   every tool the player's inventory does not have (cPlayerInventory slot
//!   0x74 HasTool) sorted by `toolDiscoverableRarity` 0x0370040A into common
//!   0x8ECB2282 / uncommon 0x1461B0D3 / rare 0x7ECBE6F5 / super rare
//!   0x54F29D46 buckets weighted by `toolDiscoverable*Chance` 100 / 0 / 0 / 0
//!   (an empty bucket weighs 0), then one tool of the bucket at random; none
//!   left: no artifact. All 70 installed tools are common.
//! * 0x104E480: `spaceToolMin/MaxDiscoveredAmmo` (0x02E06E31 / 0x02E06E36):
//!   min + RandomInt(max - min); 1 / 1 for every discoverable tool.
//! * Pickup: cAbductToolStrategy::OnHit 0x105A0D0, artifact item type 2:
//!   cToolManager::LoadTool and the item joins the inventory with its count.
//! * Planet entry 0xFDFBB0: a type-2 cPlanetaryArtifact on the planet ->
//!   queued `SPG_TerraformToolClose` (`cinematics::moments`).
//!
//! ADAPTED: the universe's own LCG stands for Math::sMainRNG; the universe
//! suppression 0xFE8820 (an app state and the new-game protection until the
//! player's badge level reaches 2) is not modelled; moons are not worlds in
//! OpenSpore, so the cyclic planet search skips them like belts; the
//! artifact lies on the world (no surface object) and the tractor beam picks
//! it up like the rares; the tool is owned and gets its count as charges
//! (the sculpt / colour tools have no OpenSpore action yet).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    space::SpaceCampaign,
    space_galaxy::Galaxy,
    space_worldgen::{PlanetKind, StarKind, TechLevel},
};

/// UniverseSimulatorTuning `universeSimulatorArtifactFrequency` (s).
pub const ARTIFACT_FREQUENCY: f32 = 180.0;
/// `universeSimulatorArtifactMinTime` (s).
pub const ARTIFACT_MIN_TIME: f32 = 60.0;
/// `universeSimulatorMaxArtifactsPerPlanet`.
pub const MAX_ARTIFACTS_PER_PLANET: usize = 1;
/// 0x1010FC0: FindStars maxDistance.
pub const ARTIFACT_STAR_RADIUS: f32 = 4.0;
/// The compiled constant of 0x100E3F0 (log10 2, used as an exp rate).
pub const LOG10_2: f32 = 0.30103;
/// `toolDiscoverableCommonChance` / Uncommon / Rare / SuperRare (ToolConfig).
pub const RARITY_CHANCES: [f32; 4] = [100.0, 0.0, 0.0, 0.0];
/// `spaceToolMinDiscoveredAmmo` / `spaceToolMaxDiscoveredAmmo` of every
/// discoverable tool.
pub const DISCOVERED_AMMO: (u32, u32) = (1, 1);

/// ToolConfig `spaceDiscoverableTools` in its order: the tool id and the
/// English `spaceToolDescription` (locale 0x0BA5C1BF). Every one has rarity
/// common 0x8ECB2282.
pub const DISCOVERABLE_TOOLS: [(&str, &str); 70] = [
    ("sculpt_cute_canyon", "Cute Canyon"),
    ("sculpt_cute_wall", "Cute Walls"),
    ("sculpt_cute_hills", "Cute Hills"),
    ("sculpt_cute_holes", "Cute Stamps"),
    ("sculpt_cute_mesas", "Cute Mesas"),
    ("sculpt_cute_mountains", "Cute Mountains"),
    ("sculpt_cute_plateau", "Cute Plateaus"),
    ("sculpt_cute_river", "Cute Rivers"),
    ("sculpt_cute_seas", "Cute Seas"),
    ("sculpt_gear_craters", "Gear Craters"),
    ("sculpt_gear_mesas", "Gear Mesas"),
    ("sculpt_gear_canyon", "Gear Canyons"),
    ("sculpt_gear_fissure", "Gear Fissure"),
    ("sculpt_gear_mountains", "Gear Mountains"),
    ("sculpt_gear_walls", "Gear Walls"),
    ("sculpt_gear_river", "Gear Rivers"),
    ("sculpt_gear_plateau", "Gear Plateaus"),
    ("sculpt_gear_seas", "Gear Seas"),
    ("sculpt_crystal_canyon", "Crystal Canyon"),
    ("sculpt_crystal_crack", "Crystal Chasm"),
    ("sculpt_crystal_hills", "Crystal Hills"),
    ("sculpt_crystal_holes", "Crystal Holes"),
    ("sculpt_crystal_mesas", "Crystal Mesas"),
    ("sculpt_crystal_mountains", "Crystal Mountains"),
    ("sculpt_crystal_plateau", "Crystal Plateau"),
    ("sculpt_crystal_river", "Crystal River"),
    ("sculpt_crystal_seas", "Crystal Seas"),
    ("sculpt_tentacle_arm", "Tentacle Arm"),
    ("sculpt_tentacle_canyon", "Tentacle Canyon"),
    ("sculpt_tentacle_hills", "Tentacle Hills"),
    ("sculpt_tentacle_mountains", "Tentacle Mountains"),
    ("sculpt_tentacle_mouth", "Tentacle Mouth Hole"),
    ("sculpt_tentacle_plateau", "Create Tentacle Plateau"),
    ("sculpt_tentacle_river", "Tentacle River"),
    ("sculpt_tentacle_seas", "Tentacle Seas"),
    ("sculpt_tentacle_suckers", "Tentacle Suckers"),
    ("sculpt_chocolate_beads", "Chocolate Strand"),
    ("sculpt_chocolate_hill", "Chocolate Hill"),
    ("sculpt_chocolate_pond", "Chocolate Swirl Pond"),
    ("sculpt_chocolate_river", "Chocolate Swirl River"),
    ("sculpt_chocolate_seas", "Chocolate Swirl Seas"),
    ("sculpt_chocolate_spirals", "Chocolate Swirl Spirals"),
    ("sculpt_chocolate_squares", "Chocolate Squares"),
    ("sculpt_chocolate_truffles", "Chocolate Truffle"),
    ("sculpt_chocolate_valley", "Swirly Valley"),
    ("ColorTerrainRed", "Red Terra-Coloring"),
    ("ColorAtmoRed", "Skies of Red"),
    ("ColorWaterRed", "Crimson Sea"),
    ("ColorTerrainOrange", "Orange Terra-Coloring"),
    ("ColorAtmoOrange", "Sky Orangizer"),
    ("ColorWaterOrange", "Orange Flavor Crystals"),
    ("ColorTerrainYellow", "Yellow Terra-Coloring"),
    ("ColorAtmoYellow", "Sky Lemonizer"),
    ("ColorWaterYellow", "Lemonade Sea"),
    ("ColorTerrainGreen", "Green Terra-Coloring"),
    ("ColorAtmoGreen", "Sky Greeninator"),
    ("ColorWaterGreen", "Sea of Green"),
    ("ColorTerrainCyan", "Cyan Terra-Coloring"),
    ("ColorAtmoCyan", "Sky Cyaninator"),
    ("ColorWaterCyan", "Cyan Sea"),
    ("ColorTerrainBlue", "Blue Terra-Coloring"),
    ("ColorAtmoBlue", "Sky Blueinator"),
    ("ColorWaterBlue", "Deep Blue Sea"),
    ("ColorTerrainPurple", "Purple Terra-Coloring"),
    ("ColorAtmoPurple", "Grape Flavored Sky"),
    ("ColorWaterPurple", "Grape Juice Ocean"),
    ("ColorTerrainPink", "Pink Terra-Coloring"),
    ("ColorAtmoPink", "Sky Pinkinator"),
    ("ColorWaterPink", "Sweetheart Sea"),
    ("ColorReset", "Planet Color Reset"),
];

/// The English name of a discoverable tool.
pub fn tool_title(id: &str) -> Option<&'static str> {
    DISCOVERABLE_TOOLS.iter().find(|(t, _)| *t == id).map(|(_, n)| *n)
}

/// A tool lying on a world (cPlanetObjectData type 2, flag 8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArtifact {
    pub tool: String,
    /// The item count (mAmmo): the charges the pickup gives.
    pub count: u32,
}

/// The routine's saved state (`UniverseState::tool_artifacts`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolArtifacts {
    /// The routine clock (+4 of 0x14955E8, ms in retail): seconds since the
    /// last Execute.
    pub clock: f32,
    /// Tools lying on worlds, by WorldId.
    pub lying: BTreeMap<u32, Vec<ToolArtifact>>,
    /// The routine's LCG (ADAPTED for Math::sMainRNG; 0 = not seeded yet).
    pub seed: u32,
}

impl ToolArtifacts {
    fn roll(&mut self) -> f32 {
        if self.seed == 0 {
            self.seed = 0x7A11_0001;
        }
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// RandomInt(n): 0..n-1 (0 when n is 0).
    fn random_int(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        ((self.roll() * n as f32) as usize).min(n - 1)
    }

    /// The tools lying on `world`.
    pub fn on(&self, world: u32) -> &[ToolArtifact] {
        self.lying.get(&world).map_or(&[], |v| v.as_slice())
    }
}

/// 0x100E3F0: the clock gains `dt`; the chance that the routine runs this
/// frame.
pub fn probability(clock: &mut f32, dt: f32) -> f32 {
    *clock += dt;
    if *clock <= ARTIFACT_MIN_TIME {
        return 0.0;
    }
    if ARTIFACT_FREQUENCY <= 0.0 {
        return 0.0;
    }
    1.0 - (-LOG10_2 * dt / ARTIFACT_FREQUENCY).exp()
}

/// 0x10513B0: a discoverable tool the player does not have (weighted rarity
/// buckets; every installed tool is common). `roll_bucket` / `roll_item` are
/// uniform draws in 0..1.
pub fn pick_tool(owned: impl Fn(&str) -> bool, roll_bucket: f32, roll_item: f32) -> Option<&'static str> {
    // Buckets: common, uncommon, rare, super rare (all entries common).
    let common: Vec<&'static str> = DISCOVERABLE_TOOLS.iter().map(|(t, _)| *t).filter(|t| !owned(t)).collect();
    let buckets: [&[&'static str]; 4] = [&common, &[], &[], &[]];
    let weights: [f32; 4] = std::array::from_fn(|i| if buckets[i].is_empty() { 0.0 } else { RARITY_CHANCES[i] });
    let total: f32 = weights.iter().sum();
    let pick = (roll_bucket * total).clamp(0.0, total);
    let mut sum = 0.0;
    for (i, w) in weights.iter().enumerate() {
        sum += w;
        if *w > 0.0 && pick <= sum {
            let list = buckets[i];
            let n = ((roll_item * list.len() as f32) as usize).min(list.len() - 1);
            return Some(list[n]);
        }
    }
    None
}

/// 0x104E480: the count of a discovered tool (min + RandomInt(max - min);
/// 1 when either is 0).
pub fn discovered_count(roll: f32) -> u32 {
    let (min, max) = DISCOVERED_AMMO;
    if min == 0 || max == 0 {
        return 1;
    }
    let span = max.saturating_sub(min);
    min + ((roll * span as f32) as u32).min(span.saturating_sub(1))
}

/// Whether the player has `tool` (HasTool: owned, or charges of it).
pub fn player_has(campaign: &SpaceCampaign, tool: &str) -> bool {
    campaign.has_tool(tool) || campaign.charges(tool) > 0
}

/// 0x100F660 on `world`: a tool artifact when it has fewer than the maximum.
/// Returns the tool placed.
pub fn place_on(campaign: &mut SpaceCampaign, world: u32) -> Option<&'static str> {
    if campaign.universe.tool_artifacts.on(world).len() >= MAX_ARTIFACTS_PER_PLANET {
        return None;
    }
    let (rb, ri, rc) = {
        let t = &mut campaign.universe.tool_artifacts;
        (t.roll(), t.roll(), t.roll())
    };
    let tool = pick_tool(|t| player_has(campaign, t), rb, ri)?;
    let count = discovered_count(rc);
    campaign.universe.tool_artifacts.lying.entry(world).or_default().push(ToolArtifact { tool: tool.to_string(), count });
    Some(tool)
}

/// 0x1010FC0: a star within 4 units of the active star (life, no core /
/// black hole / disk) and the first settleable planet from a random start.
/// Returns the world that got a tool.
pub fn execute(campaign: &mut SpaceCampaign, galaxy: &Galaxy) -> Option<(u32, &'static str)> {
    campaign.universe.tool_artifacts.clock = 0.0;
    let here = campaign.current_system;
    let wanted = |star: u32| {
        galaxy.star(star).is_some_and(|s| {
            !matches!(s.kind, StarKind::GalacticCore | StarKind::BlackHole | StarKind::ProtoPlanetary) && s.tech != TechLevel::None
        })
    };
    let stars: Vec<u32> = std::iter::once(here)
        .chain(galaxy.stars_within(here, ARTIFACT_STAR_RADIUS).into_iter().map(|(i, _)| i))
        .filter(|s| wanted(*s))
        .collect();
    if stars.is_empty() {
        return None;
    }
    let pick = campaign.universe.tool_artifacts.random_int(stars.len());
    let star = stars[pick];
    let record = galaxy.system(star);
    let planets = &record.planets;
    if planets.is_empty() {
        return None;
    }
    let start = campaign.universe.tool_artifacts.random_int(planets.len());
    for k in 0..planets.len() {
        let p = &planets[(start + k) % planets.len()];
        // ADAPTED: moons are not worlds (no WorldId) and are skipped too.
        if matches!(p.kind, PlanetKind::AsteroidBelt | PlanetKind::GasGiant) || p.moon_of.is_some() {
            continue;
        }
        let Some(slot) = p.world_slot else {
            continue;
        };
        let world = crate::space_galaxy::world_id(star, slot as u32);
        // 0x100F660 runs on the first such planet, whether or not it adds.
        return place_on(campaign, world).map(|t| (world, t));
    }
    None
}

/// The routine of one frame (0x10105D0).
pub fn tick(campaign: &mut SpaceCampaign, dt: f32, galaxy: &Galaxy) -> Option<(u32, &'static str)> {
    let p = probability(&mut campaign.universe.tool_artifacts.clock, dt);
    if p <= 0.0 {
        return None;
    }
    if campaign.universe.tool_artifacts.roll() < p {
        let placed = execute(campaign, galaxy);
        if let Some((world, tool)) = placed {
            bevy::log::debug!("universe: tool artifact {tool} on world {world}");
        }
        return placed;
    }
    None
}

/// Whether a tool artifact lies on `world`.
pub fn lying_on(campaign: &SpaceCampaign, world: u32) -> bool {
    !campaign.universe.tool_artifacts.on(world).is_empty()
}

/// The tractor beam on `world` takes its first tool artifact (0x105A0D0 type
/// 2: LoadTool, the item joins the inventory with its count). Returns the
/// message.
pub fn pick_up(campaign: &mut SpaceCampaign, world: u32) -> Option<String> {
    let lying = campaign.universe.tool_artifacts.lying.get_mut(&world)?;
    if lying.is_empty() {
        return None;
    }
    let artifact = lying.remove(0);
    if lying.is_empty() {
        campaign.universe.tool_artifacts.lying.remove(&world);
    }
    campaign.grant_tool(&artifact.tool);
    campaign.add_charges(&artifact.tool, artifact.count);
    let name = tool_title(&artifact.tool).unwrap_or(&artifact.tool);
    Some(format!("Tractor Beam recovered a tool: {name} (x{})", artifact.count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routine_waits_a_minute_then_runs_at_the_frequency() {
        let mut clock = 0.0;
        // The first 60 s give no chance.
        for _ in 0..60 {
            assert_eq!(probability(&mut clock, 1.0), 0.0);
        }
        // Then 1 - exp(-0.30103 dt / 180) per frame.
        let p = probability(&mut clock, 1.0);
        assert!((p - (1.0 - (-LOG10_2 / 180.0f32).exp())).abs() < 1e-7);
        // Mean wait after the first minute: 180 / 0.30103 s.
        assert!((ARTIFACT_FREQUENCY / LOG10_2 - 597.9).abs() < 0.1);
    }

    #[test]
    fn the_pick_skips_owned_tools_and_gives_up_when_all_are_owned() {
        assert_eq!(DISCOVERABLE_TOOLS.len(), 70);
        assert_eq!(pick_tool(|_| false, 0.3, 0.0), Some("sculpt_cute_canyon"));
        assert_eq!(pick_tool(|_| false, 0.3, 0.999), Some("ColorReset"));
        assert_eq!(pick_tool(|t| t != "ColorAtmoBlue", 0.9, 0.5), Some("ColorAtmoBlue"));
        assert_eq!(pick_tool(|_| true, 0.5, 0.5), None);
        assert_eq!(discovered_count(0.7), 1);
        assert_eq!(tool_title("sculpt_crystal_crack"), Some("Crystal Chasm"));
    }

    #[test]
    fn one_tool_per_planet_and_the_tractor_beam_takes_it() {
        let mut c = SpaceCampaign::default();
        let world = 1_234_567;
        let tool = place_on(&mut c, world).expect("a tool");
        assert!(lying_on(&c, world));
        // `universeSimulatorMaxArtifactsPerPlanet` 1.
        assert_eq!(place_on(&mut c, world), None);
        let msg = pick_up(&mut c, world).unwrap();
        assert!(msg.contains(tool_title(tool).unwrap()), "{msg}");
        assert!(c.has_tool(tool) && c.charges(tool) == 1 && !lying_on(&c, world));
        // An owned tool is not discovered again.
        for _ in 0..200 {
            if let Some(t) = place_on(&mut c, world) {
                assert_ne!(t, tool);
                pick_up(&mut c, world);
            }
        }
        // The state is saved with the universe.
        let json = serde_json::to_string(&c.universe).unwrap();
        let back: crate::space_universe::UniverseState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_artifacts, c.universe.tool_artifacts);
    }
}
