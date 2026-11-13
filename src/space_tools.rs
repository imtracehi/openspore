//! The installed Space tools as data, and what the generic tool classes do with
//! it (helper `space-tools2`, 2026-10-26).
//!
//! Every tool of the game is a property file (`spacetools~`, 363 records) that
//! names a strategy class (`spaceToolStrategy`) and numbers for it; the retail
//! code has five generic classes plus a handful of one-off ones
//! (`notes\space-weapons-re.md` T4). [`INSTALLED_TOOLS`]
//! (`space_tools_table.rs`, generated from the install) holds every record;
//! this module classifies them ([`ToolClass`]) and ports what the classes do
//! with a hit and before a use:
//!
//! * `cToolStrategy::OnHit` 0x10593B0 hands the record's
//!   `toolHit<Ground|Water|Combatant|Air>` atmosphere / temperature velocities
//!   to the terraform marble (`space_terraform`) and its relationship delta to
//!   the relationship manager: [`ToolRecord::hit_velocity`].
//! * `cToolStrategy::Update` 0x1056130 is the availability gate that runs
//!   before every use: [`tool_gate`].
//! * `DefaultProjectile` / `DefaultAOE` / `DeepSpaceProjectile` tools explode
//!   with `DamageArea` ([`ToolRecord::damage_area`]) around the impact.

// The record is the whole install row; not every field has a reader yet.
#![allow(dead_code)]

pub use crate::space_tools_table::INSTALLED_TOOLS;

/// One `spacetools~` record (inheritance resolved: the properties the record
/// lacks come from `ToolTemplate` / `military_Template` / `cultural_Template`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolRecord {
    pub name: &'static str,
    /// Instance id of the `spaceToolStrategy` key (0: the tool has no behaviour).
    pub strategy: u32,
    /// Instance id of the `spaceToolType` key (the UI bucket, [`kind`]).
    pub kind: u32,
    /// Instance id of `spaceToolContext` (planet / space / always).
    pub context: u32,
    /// `cSpaceToolData::SpaceToolFlags` built from the boolean properties ([`flag`]).
    pub flags: u32,
    pub target_flags: u32,
    /// Instance of `spaceToolPanelKey`: the palette panel the tool sits in (0: none).
    pub panel: u32,
    /// `spaceToolHide`: the palette shows the tool only once it is unlocked.
    pub hidden: bool,
    /// `toolDeselectsAfterUse`: the palette selection falls back after a use.
    pub deselects_after_use: bool,
    pub min_damage: f32,
    pub max_damage: f32,
    pub damage_area: f32,
    pub energy_cost: f32,
    pub recharge_rate: f32,
    pub auto_fire_rate: f32,
    pub charge_rate: f32,
    pub min_range: f32,
    pub max_range: f32,
    pub detectable_range: f32,
    pub relationship_delta: f32,
    pub galactic_delta: f32,
    pub galactic_radius: f32,
    pub expansion_rate: f32,
    pub projectile_speed: f32,
    pub projectile_time_out: f32,
    pub ammo_per_shot: i32,
    pub max_ammo: i32,
    pub initial_ammo: i32,
    pub use_cost: i32,
    pub buy_cost: i32,
    /// `toolHit{Ground,Water,Combatant,Air}{Atmosphere,Water,Temperature}Velocity`.
    pub hit: [f32; 12],
}

/// `cSpaceToolData::SpaceToolFlags` bits.
pub mod flag {
    pub const HIT_ORIENTS_TO_TERRAIN: u32 = 0x1;
    pub const BEAM_PASS_THROUGH: u32 = 0x2;
    pub const IS_HOMING: u32 = 0x4;
    pub const DESTROYS_FLORA_AND_FAUNA: u32 = 0x8;
    pub const PICKS_FLORA: u32 = 0x10;
    pub const USES_AMMO: u32 = 0x20;
    pub const DISABLE_ON_HOMEWORLD: u32 = 0x40;
    pub const DISABLE_ON_SAVE_GAMES: u32 = 0x80;
    pub const PASS_THROUGH_ALL: u32 = 0x100;
    pub const DAMAGES_FRIENDLIES: u32 = 0x200;
    pub const PREVENTS_PLANET_EXIT: u32 = 0x400;
    pub const ONLY_ON_PLAYER_COLONIES: u32 = 0x2000;
    pub const ONLY_ON_NPC_COLONIES: u32 = 0x4000;
    pub const USE_ON_ACTIVATE: u32 = 0x8000;
    pub const DISABLE_ON_201_RUINS: u32 = 0x20000;
    pub const PROJECTILE_PASS_THROUGH: u32 = 0x40000;
}

/// `spaceToolType` instances (the UI bucket of a tool).
pub mod kind {
    pub const TERRAFORMING: u32 = 0x293E_3F3D;
    pub const WEAPON: u32 = 0xE810_D505;
    pub const OTHER: u32 = 0x8DA5_FFB9;
    pub const SCULPTING: u32 = 0x997E_E240;
}

/// `spaceToolStrategy` instances (`notes\space-weapons-re.md` T4).
pub mod strategy {
    pub const DEFAULT_PROJECTILE: u32 = 0x2FD3_EB55;
    pub const DEFAULT_BEAM: u32 = 0xFC20_C32D;
    pub const DEFAULT_AOE: u32 = 0x6CB0_B689;
    pub const DRAG_INPUT_PROJECTILE: u32 = 0x0EB5_4070;
    pub const DEEP_SPACE_PROJECTILE: u32 = 0x68EC_CB0A;
    pub const DEFAULT_TOOL: u32 = 0xB0DA_49D9;
    pub const TURRET_FLAK: u32 = 0x4753_1ACD;
    pub const TURRET_DEFENCE_MISSILE: u32 = 0x00BE_727B;
    pub const PLACE_OBJECT: u32 = 0xEC43_9B3F;
    pub const ARTILLERY: u32 = 0x8A8B_6F35;
}

/// The behaviour class of a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolClass {
    /// No strategy: an inventory item (upgrades, cargo slots).
    Inventory,
    /// `DefaultBeam` (`cDefaultBeamTool`): lasers and the rays, held.
    DefaultBeam,
    /// `DefaultProjectile`: a shot that explodes with `DamageArea`.
    DefaultProjectile,
    /// `DefaultAOE`: the consequence-card tools, an area at the aim point.
    DefaultAoe,
    /// `DragInputProjectile`: the sculpting rivers / canyons / walls.
    DragInputProjectile,
    /// `DeepSpaceProjectile`: the projectile comes from the sky.
    DeepSpaceProjectile,
    /// `DefaultToolStrategy`: scan, portal, cloak, potions.
    DefaultTool,
    /// Turret and colony strategies (`TurretFlak`, defence missile, place object).
    Colony,
    /// Artillery and the other one-off strategy classes.
    OneOff,
}

impl ToolClass {
    /// Classes whose use ends in a projectile hit (`OnHit`).
    pub fn is_projectile(self) -> bool {
        matches!(self, Self::DefaultProjectile | Self::DefaultAoe | Self::DragInputProjectile | Self::DeepSpaceProjectile)
    }
}

/// `SpaceToolHit`: what a shot lands on (the switch of `OnHit`, 0..3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceToolHit {
    Ground = 0,
    Water = 1,
    Combatant = 2,
    Air = 3,
}

/// What a hit adds to the planet (`OnHit` 0x10593B0 -> 0xBBC4B0 / 0xBBC4E0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitVelocity {
    pub atmosphere: f32,
    /// Added to the water score directly (0xBBC4E0).
    pub water: f32,
    pub temperature: f32,
}

impl HitVelocity {
    pub fn is_zero(&self) -> bool {
        self.atmosphere == 0.0 && self.water == 0.0 && self.temperature == 0.0
    }
}

impl ToolRecord {
    pub fn has(&self, bit: u32) -> bool {
        self.flags & bit != 0
    }

    pub fn uses_ammo(&self) -> bool {
        self.has(flag::USES_AMMO)
    }

    pub fn class(&self) -> ToolClass {
        match self.strategy {
            0 => ToolClass::Inventory,
            strategy::DEFAULT_BEAM => ToolClass::DefaultBeam,
            strategy::DEFAULT_PROJECTILE => ToolClass::DefaultProjectile,
            strategy::DEFAULT_AOE => ToolClass::DefaultAoe,
            strategy::DRAG_INPUT_PROJECTILE => ToolClass::DragInputProjectile,
            strategy::DEEP_SPACE_PROJECTILE => ToolClass::DeepSpaceProjectile,
            strategy::DEFAULT_TOOL => ToolClass::DefaultTool,
            strategy::TURRET_FLAK | strategy::TURRET_DEFENCE_MISSILE | strategy::PLACE_OBJECT => ToolClass::Colony,
            _ => ToolClass::OneOff,
        }
    }

    /// The three velocities the tool adds for what it hit.
    pub fn hit_velocity(&self, hit: SpaceToolHit) -> HitVelocity {
        let i = hit as usize * 3;
        HitVelocity { atmosphere: self.hit[i], water: self.hit[i + 1], temperature: self.hit[i + 2] }
    }

    /// A tool whose hits move the marble (the eight rays and the throwables).
    pub fn is_terraformer(&self) -> bool {
        (0..4).any(|i| {
            let v = self.hit_velocity(match i {
                0 => SpaceToolHit::Ground,
                1 => SpaceToolHit::Water,
                2 => SpaceToolHit::Combatant,
                _ => SpaceToolHit::Air,
            });
            v.atmosphere != 0.0 || v.temperature != 0.0
        })
    }

    /// Damage of one hit: `MinDamage` .. `MaxDamage` by `roll` (0..1).
    pub fn damage(&self, roll: f32) -> f32 {
        self.min_damage + (self.max_damage - self.min_damage) * roll.clamp(0.0, 1.0)
    }
}

/// The record of a tool by retail name.
pub fn record(name: &str) -> Option<&'static ToolRecord> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static INDEX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| INSTALLED_TOOLS.iter().enumerate().map(|(i, t)| (t.name, i)).collect());
    index.get(name).map(|i| &INSTALLED_TOOLS[*i])
}

/// `cPlanetRecord::mTechLevel` (`TechLevel`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tech {
    #[default]
    None = 0,
    Creature = 1,
    Tribe = 2,
    City = 3,
    Civilization = 4,
    Empire = 5,
}

/// The facts `cToolStrategy::Update` reads. The caller fills what the rewrite
/// models; the rest keeps its default (the gate then passes).
#[derive(Clone, Copy, Debug, Default)]
pub struct GateContext {
    /// The UFO is dead or dying, a cinematic plays, or the game is paused:
    /// the tool is silently unavailable (0x1056131..0x105619C).
    pub busy: bool,
    /// The active planet is the player's homeworld (0x1021220).
    pub homeworld: bool,
    /// The active star is a saved-game world (star flag 2).
    pub save_game: bool,
    /// The active star is the 201 ruins (star flag 0x40000).
    pub ruins_201: bool,
    /// The planet is a wildlife sanctuary (`mFlags` 0x800).
    pub sanctuary: bool,
    /// Tech level of the active planet.
    pub tech: Tech,
    /// The star that owns the planet belongs to the player's empire.
    pub player_star: bool,
    /// Planet T-score (`GetTScore`).
    pub t_score: u8,
    /// The planet record lists animal species (`+0xD0`).
    pub has_animals: bool,
    /// Ships of the player's posse (the list at `cSimulatorPlayerUFO` 0x108DB90).
    pub posse: usize,
    /// The planet's empire has a trade route with the player (0x1037D40).
    pub trade_route: bool,
    /// A zoo is already built on the planet (0x1005170).
    pub has_zoo: bool,
    /// The active star's tech level (0xC8B730) for the monolith's uplift test.
    pub star_tech: Tech,
}

/// Why a tool cannot be used: the event log id `Update` shows (0x1052800) and
/// the reason code it passes to 0x1052890. The wording is ADAPTED (the
/// locale strings of the logs were not looked up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateFail {
    pub log: u32,
    pub reason: u8,
    pub text: &'static str,
}

const fn fail(log: u32, reason: u8, text: &'static str) -> Result<(), GateFail> {
    Err(GateFail { log, reason, text })
}

/// `cToolStrategy::Update` 0x1056130, the availability rules of a tool in the
/// Space game (VERIFIED order and constants; the tests of the target flags
/// 0x104FF20 / 0x104E330 / 0x104CB80 are the cursor's target type and are not
/// modelled: the rewrite always aims at a valid ground point).
///
/// 1. Busy UFO, cinematic or pause: unavailable without a message.
/// 2. By tool name (the vtable +0x18 id against the constants 0x16E1CDC..):
///    `PlanetConversion` and `cashinfusion` need an Empire-level planet, and
///    not the player's own; `raiderrally` needs an Empire-level planet;
///    `callforbackup` is refused with five ships in the posse; `monolith`
///    needs animals, an uplift-able star and planet; `zoo` needs a Creature
///    planet with a T-score and no zoo yet.
/// 3. `DisableOnHomeWorld` on the player's homeworld (0xC5E8C5D1), the
///    save-game / 201-ruins flags, and terraforming on a wildlife sanctuary
///    (0x4EA7991C).
pub fn tool_gate(tool: &ToolRecord, ctx: &GateContext) -> Result<(), GateFail> {
    if ctx.busy {
        return fail(0, 0xFF, "The ship cannot use tools right now");
    }
    match tool.name {
        "PlanetConversion" | "cashinfusion" => {
            if ctx.tech != Tech::Empire {
                return fail(0xBF80_8149, 4, "This tool needs an Empire's planet");
            }
            if ctx.player_star {
                return fail(0x1558_9A01, 5, "This tool cannot be used on your own empire");
            }
            if tool.name == "cashinfusion" && !ctx.trade_route {
                return fail(0xCCBE_06F7, 0xA, "Open a trade route with this empire first");
            }
        }
        "raiderrally" => {
            if ctx.tech != Tech::Empire {
                return fail(0xBF80_8149, 4, "This tool needs an Empire's planet");
            }
        }
        "callforbackup" => {
            if ctx.posse >= 5 {
                return fail(0x72CB_7F2C, 8, "Your posse is full");
            }
        }
        "monolith" => {
            if !ctx.has_animals {
                return fail(0x53CA_BFB4, 0xC, "There are no animals here to uplift");
            }
            if ctx.star_tech >= Tech::Empire {
                return fail(0xA32A_CA4C, 0xE, "An empire already holds this star");
            }
            if ctx.tech != ctx.star_tech {
                return fail(0x3E8F_ADF5, 0xB, "Only the star's own planet can be uplifted");
            }
            if ctx.sanctuary {
                return fail(0x4EA7_991C, 0xF, "This world is a wildlife sanctuary");
            }
        }
        "zoo" => {
            if ctx.tech != Tech::Creature {
                return fail(0x194E_7038, 0x10, "A zoo needs a world of wild creatures");
            }
            if ctx.t_score == 0 {
                return fail(0x7B98_0BA9, 0x11, "Terraform this world before building a zoo");
            }
            if ctx.has_zoo {
                return fail(0x5F18_DD43, 0x18, "This world already has a zoo");
            }
        }
        _ => {}
    }
    if tool.has(flag::DISABLE_ON_HOMEWORLD) && ctx.homeworld {
        return fail(0xC5E8_C5D1, 0x12, "This tool cannot be used on your homeworld");
    }
    if (tool.has(flag::DISABLE_ON_SAVE_GAMES) && ctx.save_game) || (tool.has(flag::DISABLE_ON_201_RUINS) && ctx.ruins_201) {
        return fail(0x5F18_DD43, 0, "This tool cannot be used here");
    }
    if ctx.sanctuary && tool.kind == kind::TERRAFORMING {
        return fail(0x4EA7_991C, 0xF, "Terraforming tools cannot be used on a wildlife sanctuary");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> &'static ToolRecord {
        record(name).unwrap_or_else(|| panic!("no tool {name}"))
    }

    #[test]
    fn the_table_holds_the_363_installed_tools_once() {
        assert_eq!(INSTALLED_TOOLS.len(), 363);
        let mut names: Vec<&str> = INSTALLED_TOOLS.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 363);
    }

    #[test]
    fn the_strategies_count_as_the_census_says() {
        // notes\space-weapons-re.md T4: 100 + 66 + 13 + 11 + 6 + 5 DefaultProjectile ...
        let count = |class: ToolClass| INSTALLED_TOOLS.iter().filter(|t| t.class() == class).count();
        assert_eq!(count(ToolClass::DefaultBeam), 34 + 8 + 3);
        assert_eq!(count(ToolClass::DragInputProjectile), 24);
        assert_eq!(count(ToolClass::DeepSpaceProjectile), 3);
        assert_eq!(count(ToolClass::DefaultAoe), 16);
        assert!(count(ToolClass::DefaultProjectile) >= 200);
        // The 'none' strategies of the census inherit `DefaultToolStrategy` from `ToolTemplate`.
        assert!(count(ToolClass::DefaultTool) >= 18 + 3 + 10);
        println!(
            "beam {} projectile {} aoe {} drag {} deepspace {} default {} colony {} oneoff {} inventory {}",
            count(ToolClass::DefaultBeam),
            count(ToolClass::DefaultProjectile),
            count(ToolClass::DefaultAoe),
            count(ToolClass::DragInputProjectile),
            count(ToolClass::DeepSpaceProjectile),
            count(ToolClass::DefaultTool),
            count(ToolClass::Colony),
            count(ToolClass::OneOff),
            count(ToolClass::Inventory)
        );
    }

    #[test]
    fn the_terraform_tools_carry_their_hit_velocities() {
        let heat = tool("heatray").hit_velocity(SpaceToolHit::Ground);
        assert_eq!((heat.atmosphere, heat.temperature), (0.0, 0.01));
        let asteroid = tool("asteroid");
        assert_eq!(asteroid.class(), ToolClass::DeepSpaceProjectile);
        assert_eq!(asteroid.hit_velocity(SpaceToolHit::Ground), HitVelocity { atmosphere: -0.75, water: 0.0, temperature: 0.75 });
        let buster = tool("planetbusterbomb");
        // The combatant hit pushes the atmosphere the other way (VERIFIED data).
        assert_eq!(buster.hit_velocity(SpaceToolHit::Ground).atmosphere, -0.2);
        assert_eq!(buster.hit_velocity(SpaceToolHit::Combatant).atmosphere, 0.2);
        assert_eq!(buster.hit_velocity(SpaceToolHit::Ground).water, -0.02);
        let terraformers: Vec<&str> = INSTALLED_TOOLS.iter().filter(|t| t.is_terraformer()).map(|t| t.name).collect();
        for name in ["heatray", "coldray", "cloudray", "cloudvacuum", "hotcloudray", "coldcloudray", "hotcloudvacuum", "coldcloudvacuum", "asteroid", "volcano", "icecomet", "icecapseeder", "rainoffire", "dryice", "atmospheregenerator", "droughtgenerator", "planetbusterbomb"] {
            assert!(terraformers.contains(&name), "{name} moves the marble");
        }
        assert!(!terraformers.contains(&"laserlevel1"));
    }

    #[test]
    fn projectile_tools_have_a_damage_area_and_ammo() {
        let pulse = tool("pulselevel1");
        assert_eq!(pulse.class(), ToolClass::DefaultProjectile);
        assert!(pulse.damage_area > 0.0);
        let monolith = tool("monolith");
        assert!(monolith.uses_ammo() && monolith.max_ammo == 99);
        assert!(!tool("laserlevel1").uses_ammo());
        assert!((tool("asteroid").damage(0.5) - 1000.0).abs() < 1.0e-3);
    }

    #[test]
    fn the_gate_keeps_homeworld_only_tools_off_the_homeworld() {
        let home = GateContext { homeworld: true, ..Default::default() };
        // `ToolTemplate` disables every tool on the homeworld unless its record
        // says otherwise.
        let heat = tool("heatray");
        assert!(heat.has(flag::DISABLE_ON_HOMEWORLD));
        assert_eq!(tool_gate(heat, &home).unwrap_err().log, 0xC5E8_C5D1);
        assert!(tool_gate(heat, &GateContext::default()).is_ok());
        // A busy ship uses nothing.
        assert!(tool_gate(heat, &GateContext { busy: true, ..Default::default() }).is_err());
    }

    #[test]
    fn the_gate_reads_the_special_tools_rules() {
        let ctx = GateContext::default();
        assert_eq!(tool_gate(tool("callforbackup"), &GateContext { posse: 5, ..ctx }).unwrap_err().log, 0x72CB_7F2C);
        assert!(tool_gate(tool("callforbackup"), &GateContext { posse: 4, ..ctx }).is_ok());
        // monolith: animals, a pre-empire star, the star's own tech level
        let creature = GateContext { has_animals: true, tech: Tech::Creature, star_tech: Tech::Creature, ..ctx };
        assert!(tool_gate(tool("monolith"), &creature).is_ok());
        assert_eq!(tool_gate(tool("monolith"), &GateContext { has_animals: false, ..creature }).unwrap_err().log, 0x53CA_BFB4);
        assert_eq!(tool_gate(tool("monolith"), &GateContext { star_tech: Tech::Empire, ..creature }).unwrap_err().log, 0xA32A_CA4C);
        assert_eq!(tool_gate(tool("monolith"), &GateContext { tech: Tech::Tribe, ..creature }).unwrap_err().log, 0x3E8F_ADF5);
        // cash infusion: an empire planet, not the player's, with a trade route
        let empire = GateContext { tech: Tech::Empire, trade_route: true, ..ctx };
        assert!(tool_gate(tool("cashinfusion"), &empire).is_ok());
        assert_eq!(tool_gate(tool("cashinfusion"), &GateContext { tech: Tech::Tribe, ..empire }).unwrap_err().log, 0xBF80_8149);
        assert_eq!(tool_gate(tool("cashinfusion"), &GateContext { player_star: true, ..empire }).unwrap_err().log, 0x1558_9A01);
        assert_eq!(tool_gate(tool("cashinfusion"), &GateContext { trade_route: false, ..empire }).unwrap_err().log, 0xCCBE_06F7);
        // zoo: creature world, terraformed, no zoo yet
        let zoo = GateContext { tech: Tech::Creature, t_score: 1, ..ctx };
        assert!(tool_gate(tool("zoo"), &zoo).is_ok());
        assert_eq!(tool_gate(tool("zoo"), &GateContext { t_score: 0, ..zoo }).unwrap_err().log, 0x7B98_0BA9);
        assert_eq!(tool_gate(tool("zoo"), &GateContext { has_zoo: true, ..zoo }).unwrap_err().log, 0x5F18_DD43);
    }

    #[test]
    fn terraforming_is_refused_on_a_sanctuary() {
        let sanctuary = GateContext { sanctuary: true, ..Default::default() };
        assert_eq!(tool_gate(tool("asteroid"), &sanctuary).unwrap_err().log, 0x4EA7_991C);
        assert!(tool_gate(tool("laserlevel1"), &sanctuary).is_ok());
    }

    /// Every row equals the installed record (`spaceTool*` numbers, flags built
    /// from the booleans, the strategy / type / context instances, the twelve
    /// hit velocities).
    #[test]
    #[ignore = "requires the installed game"]
    fn every_row_matches_the_installed_record() {
        let Some(install) = crate::spore_install::discover().ok() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        for row in INSTALLED_TOOLS.iter() {
            let key = crate::dbpf::ResourceKey {
                group_id: crate::space_tool_fx::TOOL_GROUP,
                instance_id: crate::world_objects::name_id(row.name),
                type_id: crate::asset_catalog::PROP,
            };
            let props = index.properties(key).unwrap_or_else(|_| panic!("no record {}", row.name));
            let number = |id: u32, default: f32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(default);
            let instance = |id: u32| props.0.get(&id).and_then(|p| p.key()).map_or(0, |k| k.instance_id);
            let checks: [(&str, u32, f32, f32); 16] = [
                ("min_damage", 0x5119_1D87, 0.0, row.min_damage),
                ("max_damage", 0x5119_1D8B, 0.0, row.max_damage),
                ("damage_area", 0x909A_C99E, 0.0, row.damage_area),
                ("energy_cost", 0x057B_4EEC, 0.0, row.energy_cost),
                ("recharge_rate", 0x5119_1D8F, 0.0, row.recharge_rate),
                ("auto_fire_rate", 0x5119_1D96, -1.0, row.auto_fire_rate),
                ("charge_rate", 0x5137_5B2D, 0.0, row.charge_rate),
                ("min_range", 0x3119_2468, -1.0, row.min_range),
                ("max_range", 0x3119_246D, -1.0, row.max_range),
                ("detectable_range", 0x0371_2DC0, 0.0, row.detectable_range),
                ("relationship_delta", 0x039A_4CAF, 0.0, row.relationship_delta),
                ("galactic_delta", 0x0686_EF4E, 0.0, row.galactic_delta),
                ("galactic_radius", 0x0686_F4B2, 0.0, row.galactic_radius),
                ("expansion_rate", 0x030C_073A, 0.0, row.expansion_rate),
                ("projectile_speed", 0x023F_BE01, 0.0, row.projectile_speed),
                ("projectile_time_out", 0x030D_28C1, 0.0, row.projectile_time_out),
            ];
            for (what, id, default, want) in checks {
                assert_eq!(number(id, default), want, "{} {what}", row.name);
            }
            assert_eq!(instance(0xA70D_87AC), row.strategy, "{} strategy", row.name);
            assert_eq!(instance(0x5119_1D92), row.kind, "{} type", row.name);
            assert_eq!(instance(0x1060_9EE4), row.context, "{} context", row.name);
            assert_eq!(instance(0xA6A3_7FC4), row.panel, "{} palette panel", row.name);
            assert_eq!(number(0xF060_9ECE, 0.0) as i32, row.buy_cost, "{} buy cost", row.name);
            assert_eq!(number(0x1060_9EDF, 0.0) as i32, row.use_cost, "{} use cost", row.name);
            let hit_ids = [
                0x030F_D84D, 0x030F_D85A, 0x030F_D85D, 0x030F_D861, 0x030F_D864, 0x030F_D868, 0x030F_D86B, 0x030F_D86E, 0x030F_D872,
                0x030F_E481, 0x030F_E485, 0x030F_E488,
            ];
            for (i, id) in hit_ids.into_iter().enumerate() {
                assert_eq!(number(id, 0.0), row.hit[i], "{} hit velocity {i}", row.name);
            }
        }
    }
}
