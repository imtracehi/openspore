//! Civilization vehicles from their editor parts: the three stats, the combat
//! numbers they give, the weapon range of the mounted tools, and the cargo an
//! economic vehicle carries (`notes\civ-rules3-re.md` W1, W3).
//!
//! VERIFIED in the executable (march2017 addresses):
//! * creation save 0x40D2D0: `modelVehicleStatSpeed` (0x720BE500) = 0.1 x the
//!   parts' `modelCapabilitySpeed` total, `modelVehicleStatDefense`
//!   (0x720BE502) = 0.1 x `modelCapabilityDefense`, `modelVehicleStatPower`
//!   (0x720BE501) = 0.1 x (`modelCapabilityPower` + `...MilitaryPower` +
//!   `...ReligiousPower` + `...EconomicPower`);
//! * vehicle setup 0xCAAB20 reads those into `cVehicle` +0xC58 / +0xC50 /
//!   +0xC54 (default 0.33 each) and calls 0xCEED70 -> 0x4E74D0(locomotion,
//!   power, defense, speed) for the damage multiplier (+0xC5C, also every
//!   weapon tool's `mDamageMultiplier`), the maximum health and the standard
//!   speed (+0x218);
//! * the weapons are the creation's `modelTool` keys (0xF1FAE962); the
//!   vehicle tools (`spacetools~` `mil_*` / `cult_*`) do 1 damage per hit, so
//!   a shot hits for the multiplier; `VehicleWeaponCycleTime` 2 s;
//! * trade cargo 0xCEEC30 = max(1, 30 x (5 x power + 0.5 x sqrt(n))) where n
//!   is a value of the loading city (`cCity` +0x64C); a delivery (0xBE19B0)
//!   pays cargo / 6 (/3 more inside one civilization) x 8 to the city's owner
//!   and x 4 to the trader and adds it to the city's trade value with the
//!   trader. Economic vehicles wait 2 s at a city (0xDCC900).
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::civ::{Approach, VehicleDomain};
use crate::civ_rules;

/// Capability properties of `vehicle_parts~` PROPs (reg_property ids).
pub mod cap {
    pub const SPEED: u32 = 0x11B7_8A70;
    pub const DEFENSE: u32 = 0x11B7_8A71;
    pub const POWER: u32 = 0x11B7_8A72;
    pub const MILITARY_POWER: u32 = 0x0632_9468;
    pub const RELIGIOUS_POWER: u32 = 0x0632_9469;
    pub const ECONOMIC_POWER: u32 = 0x0632_946A;
    /// `modelTool`: the part's weapon (`spacetools~` key).
    pub const MODEL_TOOL: u32 = 0xF1FA_E962;
}

/// `modelVehicleStatSpeed / Power / Defense` (0x720BE500..502).
pub const STAT_SPEED: u32 = 0x720B_E500;
pub const STAT_POWER: u32 = 0x720B_E501;
pub const STAT_DEFENSE: u32 = 0x720B_E502;

/// 0x40D2D0: a stat is a tenth of its capability total.
pub const STAT_PER_CAPABILITY: f32 = 0.1;
/// INFERRED: the editor caps each capability total at `MaxVehicleStatLimit`
/// 20 (`CityGameGeneralTuning`; its reader was not found), so a stat never
/// exceeds 2.0.
pub const CAPABILITY_LIMIT: f32 = civ_rules::MAX_VEHICLE_STAT_LIMIT;
/// Damage per hit of the vehicle weapon tools (`military_Template`,
/// `cultural_Template`, every `mil_*` / `cult_*`: min = max = 1).
pub const VEHICLE_TOOL_DAMAGE: f32 = 1.0;

/// `spaceToolMaxRange` of the military vehicle tools that change the
/// template's 100 (the others, and every `cult_*`, keep 100).
pub const TOOL_RANGES: [(&str, f32); 8] = [
    ("mil_01", 125.0),
    ("mil_02", 125.0),
    ("mil_03", 125.0),
    ("mil_05", 130.0),
    ("mil_06", 130.0),
    ("mil_07", 130.0),
    ("mil_08", 130.0),
    ("mil_09", 125.0),
];

/// Range of a vehicle tool by its `spacetools~` instance, `None` for a tool
/// that is not a vehicle weapon of the table (template range).
pub fn tool_range(instance: u32) -> Option<f32> {
    TOOL_RANGES
        .iter()
        .find(|(name, _)| crate::world_objects::name_id(name) == instance)
        .map(|(_, r)| *r)
}

/// A vehicle's normalized stats (`cVehicle` +0xC50 power, +0xC54 defense,
/// +0xC58 speed) and the longest range of its weapons.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VehicleStats {
    pub power: f32,
    pub defense: f32,
    pub speed: f32,
    pub range: f32,
}

impl Default for VehicleStats {
    /// The `cVehicle` constructor's 0.33 (a vehicle without a design) and the
    /// tool template's range 100.
    fn default() -> Self {
        Self {
            power: civ_rules::DEFAULT_VEHICLE_STAT,
            defense: civ_rules::DEFAULT_VEHICLE_STAT,
            speed: civ_rules::DEFAULT_VEHICLE_STAT,
            range: civ_rules::VEHICLE_TOOL_RANGE,
        }
    }
}

impl VehicleStats {
    /// Stats from the parts' capability totals (0x40D2D0).
    pub fn from_totals(speed: f32, defense: f32, power: f32, range: Option<f32>) -> Self {
        let stat = |total: f32| total.clamp(0.0, CAPABILITY_LIMIT) * STAT_PER_CAPABILITY;
        Self {
            power: stat(power),
            defense: stat(defense),
            speed: stat(speed),
            range: range.unwrap_or(civ_rules::VEHICLE_TOOL_RANGE),
        }
    }
}

/// What the stats give in a fight (0x4E74D0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleCombat {
    /// Damage of one weapon hit: the tool's 1 x the damage multiplier.
    pub damage: f32,
    pub max_health: f32,
    /// Standard speed, world units per second (the planet is the retail
    /// R = 500 one, `CIV_WORLD_SCALE` 1).
    pub speed: f32,
}

/// `VehicleLocomotion`: land 0, water 1, air 2.
pub fn locomotion(domain: VehicleDomain) -> u32 {
    match domain {
        VehicleDomain::Land => 0,
        VehicleDomain::Sea => 1,
        VehicleDomain::Air => 2,
    }
}

/// 0x4E74D0: land damage 10 + 20 p, health 150 + 300 d, speed 5 + 20 s;
/// water 15 + 30 p, 200 + 400 d, 5 + 20 s; air 10 + 20 p, 150 + 300 d,
/// 10 + 40 s.
pub fn combat(domain: VehicleDomain, s: VehicleStats) -> VehicleCombat {
    let (damage, health, speed) = match locomotion(domain) {
        0 => (10.0 + 20.0 * s.power, 150.0 + 300.0 * s.defense, 5.0 + 20.0 * s.speed),
        1 => (15.0 + 30.0 * s.power, 200.0 + 400.0 * s.defense, 5.0 + 20.0 * s.speed),
        _ => (10.0 + 20.0 * s.power, 150.0 + 300.0 * s.defense, 10.0 + 40.0 * s.speed),
    };
    VehicleCombat { damage: VEHICLE_TOOL_DAMAGE * damage, max_health: health, speed }
}

/// Capability totals of a design's parts and the longest tool range among
/// them (`None` without a ranged tool of the table). Parts are read from
/// `vehicle_parts~` by their definition (else model) instance, with the
/// template inheritance resolved.
pub fn design_stats(index: &crate::asset_catalog::ResourceIndex, parts: &[crate::game::VehiclePart]) -> Option<VehicleStats> {
    if parts.is_empty() || index.records.is_empty() {
        return None;
    }
    let (mut speed, mut defense, mut power) = (0.0f32, 0.0f32, 0.0f32);
    let mut range: Option<f32> = None;
    let mut read = 0;
    for part in parts {
        let key = crate::dbpf::ResourceKey {
            group_id: crate::asset_catalog::VEHICLE_PARTS,
            instance_id: part.definition_id.unwrap_or(part.instance),
            type_id: crate::asset_catalog::PROP,
        };
        let Ok(p) = index.properties(key) else {
            continue;
        };
        read += 1;
        let n = |id: u32| p.0.get(&id).and_then(|v| v.number()).unwrap_or(0.0).max(0.0);
        speed += n(cap::SPEED);
        defense += n(cap::DEFENSE);
        power += n(cap::POWER) + n(cap::MILITARY_POWER) + n(cap::RELIGIOUS_POWER) + n(cap::ECONOMIC_POWER);
        if let Some(tool) = p.0.get(&cap::MODEL_TOOL).and_then(|v| v.key()) {
            let r = tool_range(tool.instance_id).unwrap_or(civ_rules::VEHICLE_TOOL_RANGE);
            range = Some(range.map_or(r, |old: f32| old.max(r)));
        }
    }
    (read > 0).then(|| VehicleStats::from_totals(speed, defense, power, range))
}

/// The stats of every vehicle class the player has designed (the rigid
/// editor's class designs), keyed by type and locomotion.
pub fn class_stats(index: &crate::asset_catalog::ResourceIndex, run: &crate::game::RunProgress) -> Vec<(Approach, VehicleDomain, VehicleStats)> {
    let mut out = Vec::new();
    for approach in [Approach::Military, Approach::Religious, Approach::Economic] {
        for domain in [VehicleDomain::Land, VehicleDomain::Sea, VehicleDomain::Air] {
            if let Some(stats) = run.rigid.vehicle_design(approach, domain).and_then(|d| design_stats(index, &d.parts)) {
                out.push((approach, domain, stats));
            }
        }
    }
    out
}

/// A cheap fingerprint of the class designs (refresh trigger).
fn design_signature(run: &crate::game::RunProgress) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for approach in [Approach::Military, Approach::Religious, Approach::Economic] {
        for domain in [VehicleDomain::Land, VehicleDomain::Sea, VehicleDomain::Air] {
            let parts = run.rigid.vehicle_design(approach, domain).map(|d| d.parts.as_slice()).unwrap_or(&[]);
            h = (h ^ parts.len() as u64).wrapping_mul(0x100_0000_01b3);
            for p in parts {
                h = (h ^ p.definition_id.unwrap_or(p.instance) as u64).wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    h
}

/// Keep `CivCampaign::vehicle_stats` in step with the player's designs (new
/// vehicles take the stats of their class when built, as a retail vehicle
/// takes its creation's).
pub fn refresh_vehicle_stats(
    stage: Res<State<crate::stages::GameStage>>,
    originals: Option<Res<crate::Originals>>,
    mut run: ResMut<crate::game::RunProgress>,
    mut last: Local<Option<u64>>,
) {
    if *stage.get() != crate::stages::GameStage::Civilization {
        return;
    }
    let Some(originals) = originals else {
        return;
    };
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    let signature = design_signature(&run);
    if *last == Some(signature) {
        return;
    }
    *last = Some(signature);
    let stats = class_stats(index, &run);
    if run.civ_campaign.vehicle_stats != stats {
        run.civ_campaign.vehicle_stats = stats;
    }
}

/// 0xCEEC30: cargo a vehicle of `power` loads at a city whose loading value
/// is `n` (`cCity` +0x64C, the citizens: `CityState::citizens`).
pub fn cargo(n: f32, power: f32) -> f32 {
    (30.0 * (5.0 * power + 0.5 * n.max(0.0).sqrt())).max(1.0)
}

/// 0xDCC900: an economic vehicle waits this long at a city before it
/// unloads and loads.
pub const TRADE_WAIT: f32 = 2.0;

/// ADAPTED: OpenSpore keeps the economic vehicle at its partner city instead
/// of driving it home and back; a delivery is made every round trip the
/// vehicle would take at its standard speed (two legs and two 2 s waits).
pub fn delivery_period(distance: f32, speed: f32) -> f32 {
    2.0 * distance.max(0.0) / speed.max(0.1) + 2.0 * TRADE_WAIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_are_a_tenth_of_the_capability_totals() {
        let s = VehicleStats::from_totals(4.0, 2.0, 7.0, None);
        assert!((s.speed - 0.4).abs() < 1e-6 && (s.defense - 0.2).abs() < 1e-6 && (s.power - 0.7).abs() < 1e-6);
        assert_eq!(s.range, 100.0, "the tool template's range");
        // The editor limit: 20 per total.
        assert!((VehicleStats::from_totals(99.0, 0.0, 0.0, None).speed - 2.0).abs() < 1e-6);
        let d = VehicleStats::default();
        assert_eq!((d.power, d.defense, d.speed), (0.33, 0.33, 0.33));
    }

    #[test]
    fn combat_follows_the_locomotion_table() {
        let s = VehicleStats { power: 0.5, defense: 0.5, speed: 0.5, range: 100.0 };
        let land = combat(VehicleDomain::Land, s);
        assert_eq!((land.damage, land.max_health, land.speed), (20.0, 300.0, 15.0));
        let sea = combat(VehicleDomain::Sea, s);
        assert_eq!((sea.damage, sea.max_health, sea.speed), (30.0, 400.0, 15.0));
        let air = combat(VehicleDomain::Air, s);
        assert_eq!((air.damage, air.max_health, air.speed), (20.0, 300.0, 30.0));
        // Defaults: a land vehicle hits for 16.6 and has 249 health.
        let d = combat(VehicleDomain::Land, VehicleStats::default());
        assert!((d.damage - 16.6).abs() < 1e-4 && (d.max_health - 249.0).abs() < 1e-3);
    }

    #[test]
    fn tool_ranges_and_cargo() {
        assert_eq!(tool_range(crate::world_objects::name_id("mil_05")), Some(130.0));
        assert_eq!(tool_range(crate::world_objects::name_id("mil_13")), None);
        // 30 x (5 x 0.33 + 0.5 x 4) = 109.5 for n = 16.
        assert!((cargo(16.0, 0.33) - 109.5).abs() < 1e-3);
        assert_eq!(cargo(0.0, 0.0), 1.0, "at least one unit");
        let (owner, trader) = civ_rules::trade_delivery(120.0, false);
        assert_eq!((owner, trader), (160.0, 80.0));
        assert!((delivery_period(100.0, 10.0) - 24.0).abs() < 1e-4);
    }

    #[test]
    fn installed_vehicle_parts_carry_capabilities() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        // ve_aircraft_01: modelCapabilitySpeed 2.
        let part = crate::game::VehiclePart::new(crate::world_objects::name_id("ve_aircraft_01"), 0);
        let s = design_stats(&index, &[part]).expect("the part PROP resolves");
        assert!((s.speed - 0.2).abs() < 1e-6, "{s:?}");
    }
}
