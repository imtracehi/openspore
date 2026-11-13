//! The generic tool classes at work for the installed tools (helper
//! `space-tools3`, 2026-10-26): what a palette tool does when it is not one of
//! the hand-written actions of `space.rs`.
//!
//! The retail game gives each `spacetools~` record a strategy class
//! (`notes\space-weapons-re.md` T4). The table of `space_tools` holds the
//! numbers; this module ports what the classes do with them:
//!
//! * `DefaultProjectile` weapons (the antimatter bomb and missile, and every
//!   pulse / proton / bomb row): [`weapon`] builds the firing row (damage,
//!   energy, recharge, auto-fire, ranges, `DamageArea`) and [`projectile_speed`]
//!   the flight from the record, so `space::space_fire` fires any such tool the
//!   palette selects; ammo (`UsesAmmo`) is one per use (0x1050080).
//! * `cToolStrategy::OnHit` 0x10593B0 ([`hit_deltas`]): the relationship delta
//!   of the target owner (GoodToolUse above 0, BadToolUse below, VERIFIED at
//!   0x105987C), the galactic delta and radius, and the hit velocities.
//! * `cRepairToolStrategy` 0x7D4F4D72 / `cEnergyRepairToolStrategy` 0x28AE31DE
//!   (the repair and energy potions, [`SelfUse`]): `Update` 0x1056C80 /
//!   0x1056BC0 accepts the use only while the ship is below its maximum, `OnHit`
//!   0x105B140 / 0x105B0B0 restores `maximum x MinDamage x 0.01` (the value
//!   0x13EB954 is 0.01; `MinDamage` is read at `cSpaceToolData +0x1B4` by
//!   0xCE6AC0), `ProcessCost` 0x10535F0 scales the Sporebucks by the missing
//!   fraction ([`repair_cost`]).
//! * `toolUseOnActivate` (flag 0x8000: shield, cloak, potions): the palette
//!   button uses the tool at once. [`space_self_tools`] is that use: ownership,
//!   the availability gate ([`crate::space_tools::tool_gate`]), the recharge
//!   clock, `EnergyCost` energy (0x10504A1), one ammo, then the effect.
//!
//! * `cToolStrategy::WhileFiring` 0x1056880 (the base of every tool's use: all
//!   eleven derived `WhileFiring`s call it first) clears the owner UFO's
//!   cloak flag (`cGameDataUFO::SetFlag(1, false)` 0xC38B20) and stops its cloak
//!   timer (+0x6F0): **any tool use ends the cloak** ([`break_cloak`]). Then it
//!   switches on the tool id: `cloak` (0x44808DE7) sets flag 1 and starts the
//!   timer, `shield` (0x4542380A) sets `cCombatant::mbInvincible` (the
//!   combatant at UFO +0x508, byte +0x80, 0xBFC5B0) and starts the timer at
//!   +0x6D0. The per-frame sweep 0x1009F20 (`cSimulatorSpaceGame`, every UFO)
//!   ends them: cloak after 0x15F90 ms = **90 s**, shield after 0xEA60 ms =
//!   **60 s** ([`CLOAK_SECONDS`], [`SHIELD_SECONDS`]). The consumers: the UFO's
//!   `TakeDamage` (0xC3CF29) returns at once for an invincible combatant (and
//!   the projectile hit 0xCC0FDE bounces off the player's UFO); every
//!   nearest-UFO search (0xCC2820, used by the NPC behaviours 0xD56EC0 /
//!   0xD57400 and the defence missile 0xCBB3FC) skips a UFO with flag 1.
//! * `cRepairAOEToolStrategy::WhileFiring` 0x1057FF0 (`aoe_repair`, [`SelfUse::RepairAoe`]):
//!   after the base, a random amount between `MinDamage` and `MaxDamage`
//!   (the random roll of `sMainRNG`, clamped to the two) is dealt as a
//!   `TakeDamage(amount, playerEmpire, DamageType 10 = Healing)` to the tool
//!   owner's UFO and to every UFO / combatant of the player's empire or an ally
//!   inside `MaxRange` (0x104BE40: `MaxRange x RangeMultiplier`) around the ship;
//!   a repaired UFO gets flag 2 (the repair visual) and a sound, a repaired
//!   building gets the effect 0x58451561.
//! * The Rally Call (`rallycall`, `cRallyCallToolStrategy::WhileFiring` 0x105BF80,
//!   [`SelfUse::RallyCall`]) and the posse's life in combat (notes T9): the use
//!   sets UFO flag 3 on the player's ship and on every posse UFO (the
//!   effect `SG_ufo_rally_call` 0x2C860712 on the player, `SG_ufo_rally_response`
//!   0x20316487 on a posse ship; the effect function 0xE9A0A0 clears the flag
//!   as it starts the effect), plays each posse ship's `*_posse_rally` voice (by
//!   its empire's archetype) and puts the effect on the player's / posse-held
//!   turrets; the gameplay part is in the base combatant `TakeDamage` 0xBFCF10:
//!   while the tool's recharge timer runs (`RechargeRate` s = 60 s) damage dealt
//!   by the player's empire or by a posse ship is multiplied by `MaxDamage`
//!   (2.0, [`rally_multiplier`]). The posse ship (UFO type 3) takes damage like
//!   any UFO ([`hurt_escort`], `cGameDataUFO::TakeDamage` 0xC3CEC0): the player's
//!   side and other posse ships cannot hurt it, its first damage at full health
//!   plays the `*_posse_underattack` voice, its death the `*_posse_dead` voice,
//!   `DestroyAllyUFO` (0x526E542, scale 1) against the owner when it is another
//!   empire's, and `RemovePosse` 0x10015A0. `callforbackup` (OnHit 0x1059D3A)
//!   joins a ship of the player's own empire (`AddPosse` 0x1002650); the
//!   player-UFO update 0x1000C60 destroys it once the tool is ready again, so it
//!   stays for the tool's `RechargeRate` (120 s, [`expire_backup`]).
//! * The soothing song (`soothingsong`, Bard superpower): the base `WhileFiring`
//!   starts a timer on the game state (+0x1258, 0xC757D0); `IsActive` 0xC757F0
//!   is true for 0x1D4C0 ms = **120 s** ([`SOOTHING_SONG_SECONDS`]) and its
//!   consumer 0xD05A60 (`CalculateRelationshipAbsolute`) clamps a negative
//!   relationship to 0 while it plays (not for the Grox), see
//!   `SpaceCampaign::relationship`. The use of any `weapon`-type tool (lasers,
//!   missiles, bombs, shield, area repair) stops it (0x1050FC9 -> 0xC757E0,
//!   [`ends_soothing_song`]).
// `hit_deltas` and `repair_cost` document ports that only the tests read until the
// hit pipeline of the Space game calls them.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::game::{Health, Player, RunProgress};
use crate::space::{SpaceArchetype, SpaceCampaign};
use crate::space_rules::{self as rules, RelationshipEvent};
use crate::space_tools::{self, flag, kind, HitVelocity, SpaceToolHit, ToolClass, ToolRecord};
use crate::space_worldgen::EmpireId;

/// `cRepairToolStrategy` (the repair potions).
pub const STRATEGY_REPAIR: u32 = 0x7D4F_4D72;

/// `cRallyCallToolStrategy` (`rallycall`, 0x105BF80).
pub const STRATEGY_RALLY_CALL: u32 = 0x02C2_B837;
/// `cEnergyRepairToolStrategy` (the energy potions).
pub const STRATEGY_ENERGY_REPAIR: u32 = 0x28AE_31DE;

/// `cRepairAOEToolStrategy` (the area repair, `aoe_repair`).
pub const STRATEGY_REPAIR_AOE: u32 = 0x63E3_33CB;

/// Seconds the shield (`cCombatant::mbInvincible`) lasts: the sweep 0x1009F20
/// clears it once the timer at UFO +0x6D0 passes 0xEA60 ms (VERIFIED).
pub const SHIELD_SECONDS: f32 = 60.0;
/// Seconds the cloak (UFO flag 1) lasts: cleared once the timer at UFO +0x6F0
/// passes 0x15F90 ms (VERIFIED, 0x1009F8B).
pub const CLOAK_SECONDS: f32 = 90.0;
/// Seconds the soothing song plays: `IsActive` 0xC757F0 stops its timer
/// (+0x1258) after 0x1D4C0 ms (VERIFIED).
pub const SOOTHING_SONG_SECONDS: f32 = 120.0;

/// What the use of a `UseOnActivate` tool does to the player's ship.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelfUse {
    /// Heal `percent` of the maximum health (`cRepairToolStrategy::OnHit`).
    Repair { percent: f32 },
    /// Refill `percent` of the reactor (`cEnergyRepairToolStrategy::OnHit`).
    EnergyRepair { percent: f32 },
    /// Damage immunity for [`SHIELD_SECONDS`] (`cCombatant::mbInvincible`).
    Shield,
    /// Hostile ships cannot find the ship for [`CLOAK_SECONDS`] (UFO flag 1).
    Cloak,
    /// `cRepairAOEToolStrategy::WhileFiring` 0x1057FF0: heals the ship and the
    /// player's / allied ships around it by a random amount in `min..=max`.
    RepairAoe { min: f32, max: f32 },
    /// `cRallyCallToolStrategy::WhileFiring` 0x105BF80: the posse answers and
    /// for `seconds` (the tool's `RechargeRate`, the damage clause of the base
    /// `TakeDamage` 0xBFCF10) the damage of the player's side is multiplied by
    /// `multiplier` (`MaxDamage`).
    RallyCall { multiplier: f32, seconds: f32 },
}

/// The self use of a tool record, if it is one of the ship tools used at
/// activation (`toolUseOnActivate`).
pub fn self_use(record: &ToolRecord) -> Option<SelfUse> {
    match record.strategy {
        STRATEGY_REPAIR => Some(SelfUse::Repair { percent: record.min_damage }),
        STRATEGY_ENERGY_REPAIR => Some(SelfUse::EnergyRepair { percent: record.min_damage }),
        STRATEGY_REPAIR_AOE => Some(SelfUse::RepairAoe { min: record.min_damage, max: record.max_damage.max(record.min_damage) }),
        STRATEGY_RALLY_CALL => Some(SelfUse::RallyCall { multiplier: record.max_damage, seconds: record.recharge_rate }),
        _ if record.name == "shield" && record.has(flag::USE_ON_ACTIVATE) => Some(SelfUse::Shield),
        _ if record.name == "cloak" && record.has(flag::USE_ON_ACTIVATE) => Some(SelfUse::Cloak),
        _ => None,
    }
}

/// Points a repair / energy tool restores: `maximum x percent x 0.01`
/// (0x105B140 / 0x105B0B0, the multiplier is the constant 0.01 at 0x13EB954).
pub fn repair_amount(maximum: f32, percent: f32) -> f32 {
    maximum * percent * 0.01
}

/// `cRepairToolStrategy::Update` 0x1056C80 and `cEnergyRepairToolStrategy::Update`
/// 0x1056BC0: the use is accepted only while the maximum is above the current
/// value (else the tool manager's fail message 0x14 / 0x13 is shown).
pub fn can_restore(current: f32, maximum: f32) -> bool {
    maximum > current
}

/// The amount of one `aoe_repair` use (0x1057FF0): `MinDamage + roll x (MaxDamage -
/// MinDamage)`, `roll` the 0..1 `RandomFloat`, clamped between the two.
pub fn repair_aoe_amount(min: f32, max: f32, roll: f32) -> f32 {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    (min + roll * (max - min)).clamp(lo, hi)
}

/// The base `cToolStrategy::WhileFiring` 0x1056880 runs first for every tool
/// use: the owner's cloak flag and timer are cleared (VERIFIED). The cloak
/// tool itself sets them again after this.
pub fn break_cloak(campaign: &mut crate::space::SpaceCampaign) {
    campaign.cloak_left = 0.0;
}

/// After a successful use (0x1050E30, run for every tool), a tool of type `weapon`
/// (`[tool+0x17C] == 0xE810D505`, 0x1050FC9) stops the soothing song's timer
/// (0xC757E0): fighting ends the song (VERIFIED).
pub fn ends_soothing_song(record: &ToolRecord) -> bool {
    record.kind == kind::WEAPON
}

/// `CalculateRelationshipAbsolute` 0xD05A60 while the soothing song plays: a
/// negative relationship reads as 0 unless the Grox are one side (the code
/// compares both ids with the star manager's Grox id, 0xD05D50..0xD05D91).
pub fn soothed_relationship(value: f32, song_active: bool, grox: bool) -> f32 {
    if song_active && !grox && value < 0.0 {
        0.0
    } else {
        value
    }
}

// ============================================================== the posse in combat

/// A posse voice the engine raises for `space_voices` to play (the retail code
/// plays each from the routine named at the variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosseEvent {
    /// 0x105C0A1 ..: the Rally Call, one voice per posse ship, by its empire's
    /// archetype (`None`: a ship of the player's own empire, the `squi_` voice).
    Rally(Option<EmpireId>),
    /// 0xC3D36D ..: a posse ship's first damage while at full health (VERIFIED:
    /// `mHealthPoints == mMaxHealthPoints` before the damage is applied).
    UnderAttack(Option<EmpireId>),
    /// 0xC3D6F8 ..: a posse ship is destroyed.
    Dead(Option<EmpireId>),
}

/// The damage multiplier the Rally Call gives the player's side
/// (`cCombatant::TakeDamage` 0xBFCF10, VERIFIED): while the tool's recharge
/// timer runs, a hit by the player's empire or by a posse ship does `MaxDamage`
/// (2.0) times its damage. Applied wherever the player's side deals damage.
pub fn rally_multiplier(campaign: &SpaceCampaign) -> f32 {
    if campaign.rally_left > 0.0 {
        space_tools::record("rallycall").map_or(2.0, |r| r.max_damage)
    } else {
        1.0
    }
}

/// Who a hit on a posse ship comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attacker {
    /// The player's empire (its UFO, colonies, turrets).
    Player,
    /// Another posse ship (UFO type 3).
    Posse,
    /// Anyone else: pirates, the Grox, hostile empires.
    Hostile,
}

/// `cGameDataUFO::TakeDamage` 0xC3CEC0, the clauses for a posse ship (UFO type
/// 3), VERIFIED at 0xC3CFFF..0xC3D01C and 0xC3D04F..0xC3D064: a hit whose
/// attacker is the player's empire, or any posse ship, returns without damage.
pub fn escort_takes_damage_from(attacker: Attacker) -> bool {
    attacker == Attacker::Hostile
}

/// The damage a posse ship takes from hostile fire: its first damage at full
/// health raises the `*_posse_underattack` voice (0xC3D36D), the health drops,
/// and at 0 or below the ship is destroyed ([`kill_escort`]). Returns the news
/// line when the ship died.
pub fn hurt_escort(campaign: &mut SpaceCampaign, slot: usize, damage: f32) -> Option<String> {
    if damage <= 0.0 {
        return None;
    }
    let ship = campaign.fleet.get_mut(slot)?;
    if ship.hp >= ship.max_hp {
        campaign.posse_events.push(PosseEvent::UnderAttack(ship.empire));
    }
    ship.hp -= damage;
    if ship.hp > 0.0 {
        return None;
    }
    kill_escort(campaign, slot)
}

/// A posse ship is destroyed (0xC3D6B1 .. 0xC3D8D4): the dead voice by its
/// empire's archetype, `DestroyAllyUFO` (event 0x526E542, scale 1) against the
/// owner when the ship is not the player's own empire's, then `RemovePosse`
/// 0x10015A0 (the ship leaves the posse; its dismissed voice follows, the
/// routine's flag argument is 0 here). Returns the news line.
pub fn kill_escort(campaign: &mut SpaceCampaign, slot: usize) -> Option<String> {
    if slot >= campaign.fleet.len() {
        return None;
    }
    let ship = campaign.fleet.remove(slot);
    campaign.posse_events.push(PosseEvent::Dead(ship.empire));
    Some(match ship.empire {
        Some(empire) => {
            let stance = campaign.apply_relationship(empire, RelationshipEvent::DestroyAllyUfo, 1.0).unwrap_or_default();
            format!("{} of the {} empire was destroyed in your posse. {stance}", ship.name, SpaceCampaign::empire_name(empire))
        }
        None => format!("{} was destroyed", ship.name),
    })
}

/// `cSimulatorPlayerUFO` update tail 0x1000C60 (called every frame from
/// 0x100202B, VERIFIED): when the player owns the `callforbackup` tool and it
/// is not busy (0x104FF20: its recharge timer ran out), a posse ship of the
/// player's own empire (UFO type 3, political id the player's) is destroyed
/// (`TakeDamage(1000000)`). The ship `callforbackup` joined therefore stays for
/// the tool's `RechargeRate`, 120 s. In the rewrite the tool's clock is the
/// Knight's superpower cooldown. Returns the news line.
pub fn expire_backup(campaign: &mut SpaceCampaign) -> Option<String> {
    if !matches!(campaign.archetype, SpaceArchetype::Knight) || campaign.superweapon_cooldown > 0.0 {
        return None;
    }
    let mut note = None;
    while let Some(slot) = campaign.fleet.iter().position(|s| s.empire.is_none()) {
        note = kill_escort(campaign, slot);
    }
    note
}

/// Hostile fire on the posse: `damage[slot]` of this frame's fire at each posse
/// ship, applied from the last slot down so a death does not shift the rest.
/// Returns the news lines.
pub fn apply_escort_fire(campaign: &mut SpaceCampaign, damage: &[f32]) -> Vec<String> {
    let mut news = Vec::new();
    for slot in (0..damage.len().min(campaign.fleet.len())).rev() {
        news.extend(hurt_escort(campaign, slot, damage[slot]));
    }
    news
}

/// Where a posse ship fires. Retail (the behaviour node 0xDBFE10, VERIFIED): the
/// ship takes the PLAYER's current target (`cSimulatorPlayerUFO` 0xBDDE60) as its
/// own. The rewrite's player has no lock-on: the player's target is the hostile
/// ship nearest to him inside `reach` (the ship F fires at, ADAPTED). The escort
/// fires at it when it is inside its own `reach`, else at the hostile ship
/// nearest to the escort (ADAPTED: the escorts keep their formation, they do not
/// fly to the target).
pub fn posse_target(player: Vec3, escort: Vec3, hostiles: &[Vec3], reach: f32) -> Option<Vec3> {
    let nearest = |from: Vec3| {
        hostiles
            .iter()
            .copied()
            .filter(|h| h.distance(from) < reach)
            .min_by(|a, b| a.distance_squared(from).total_cmp(&b.distance_squared(from)))
    };
    nearest(player).filter(|t| t.distance(escort) < reach).or_else(|| nearest(escort))
}

/// `cRepairToolStrategy::ProcessCost` 0x10535F0: the use cost scaled by the
/// missing fraction of the health, `((max - health) / max) x useCost`
/// truncated to an integer.
pub fn repair_cost(use_cost: i32, health: f32, maximum: f32) -> i32 {
    if maximum <= 0.0 {
        return 0;
    }
    (((maximum - health) / maximum) * use_cost as f32) as i32
}

/// What `cToolStrategy::OnHit` 0x10593B0 does with one hit of a tool.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitDeltas {
    /// Added to the planet's marble (atmosphere / temperature, 0xBBC4B0) and
    /// water score (0xBBC4E0).
    pub velocity: HitVelocity,
    /// `toolRelationshipDelta` (+0x1F8): the owner of a hit city's relationship.
    pub relationship: f32,
    /// `toolGalacticRelationshipDelta` (+0x1FC) and its radius (+0x200).
    pub galactic: f32,
    pub galactic_radius: f32,
}

/// The deltas of a tool's hit on `hit` (the type picks the velocity triple).
pub fn hit_deltas(record: &ToolRecord, hit: SpaceToolHit) -> HitDeltas {
    HitDeltas {
        velocity: record.hit_velocity(hit),
        relationship: record.relationship_delta,
        galactic: record.galactic_delta,
        galactic_radius: record.galactic_radius,
    }
}

/// The relationship event of a delta (VERIFIED 0x105987C): above 0 a
/// GoodToolUse of that size, below 0 a BadToolUse of its magnitude.
pub fn relationship_event(delta: f32) -> Option<(RelationshipEvent, f32)> {
    if delta > 0.0 {
        Some((RelationshipEvent::GoodToolUse, delta))
    } else if delta < 0.0 {
        Some((RelationshipEvent::BadToolUse, -delta))
    } else {
        None
    }
}

/// The firing row of a `DefaultProjectile` weapon of the table that the
/// curated `rules::PLAYER_WEAPONS` rows do not cover (the antimatter bomb and
/// missile): the record's numbers in retail units. `MaxRange` -1 is "no limit"
/// in the record; the flight reach (`toolProjectileSpeed` x `toolProjectileTimeOut`)
/// stands in for it (ADAPTED).
fn record_weapon(record: &'static ToolRecord) -> Option<rules::WeaponStats> {
    if record.class() != ToolClass::DefaultProjectile
        || record.kind != kind::WEAPON
        || record.panel == 0
        || record.is_terraformer()
        || (record.min_damage <= 0.0 && record.max_damage <= 0.0)
    {
        return None;
    }
    let reach = record.projectile_speed * record.projectile_time_out;
    Some(rules::WeaponStats {
        id: record.name,
        damage: record.min_damage,
        energy: record.energy_cost,
        recharge: record.recharge_rate,
        autofire: record.auto_fire_rate,
        min_range: record.min_range,
        max_range: if record.max_range > 0.0 { record.max_range } else { reach },
        area: record.damage_area,
    })
}

/// The weapon row F can fire for a retail tool name: the curated player
/// weapons, else a table weapon ([`record_weapon`]).
pub fn weapon(id: &str) -> Option<rules::WeaponStats> {
    rules::player_weapon(id).or_else(|| space_tools::record(id).and_then(record_weapon))
}

/// Projectile speed in retail units per second: `toolProjectileSpeed` of the
/// record (pulse and proton missile 150, dumb bombs 35, antimatter bomb 40, missile 250),
/// else the curated value.
pub fn projectile_speed(id: &str) -> f32 {
    space_tools::record(id).map(|r| r.projectile_speed).filter(|s| *s > 0.0).unwrap_or_else(|| rules::projectile_speed(id))
}

/// Table tools of the combat panels the palette can show next to the hand-written
/// ones: weapons F fires ([`record_weapon`]) and ship tools used at activation
/// ([`self_use`]), the panel placement read from the record.
pub fn generic_palette_tools() -> Vec<&'static ToolRecord> {
    space_tools::INSTALLED_TOOLS
        .iter()
        .filter(|r| r.panel != 0 && (record_weapon(r).is_some() || self_use(r).is_some()))
        .filter(|r| rules::player_weapon(r.name).is_none())
        .collect()
}

/// A use of a ship tool requested by the palette (its button is the
/// activation). One request is served per frame.
pub fn request_self_use(run: &mut RunProgress, tool: &str) {
    run.space_campaign.pending_self_tool = Some(tool.to_string());
}

/// Serve the activation of a ship tool and run the timers of the timed
/// effects. Ownership (a kept tool or a carried charge), the tool gate, the
/// recharge clock (`RechargeRate` s after a use, 0x1050A70), the energy gate
/// (0x1050C4F) and the spending (0x10504A1, one ammo per use) are the generic
/// `Use`; the effect is the strategy's `OnHit`.
pub fn space_self_tools(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut health: Query<&mut Health, With<Player>>,
    ship: Query<&Transform, With<Player>>,
    mut commands: Commands,
    mut demo: Local<(u32, Option<bool>)>,
) {
    let dt = time.delta_secs();
    // `--space-posse-demo` (captures): from frame 60 the ship owns the Rally Call and has an
    // escort of the Kroxid, the Thelos and its own empire; at frame 100 the call is used.
    let demo_on = *demo.1.get_or_insert_with(|| std::env::args().any(|a| a == "--space-posse-demo"));
    demo.0 += 1;
    if demo_on && demo.0 == 60 {
        let campaign = &mut run.space_campaign;
        campaign.grant_tool("rallycall");
        campaign.energy = campaign.energy.max(100.0);
        for (name, empire) in [("Kroxid escort", Some(2)), ("Thelos escort", Some(4)), ("Mini-U", None)] {
            campaign.fleet.push(crate::space::EscortShip { name: name.into(), empire, hp: 300.0, max_hp: 300.0 });
        }
    }
    if demo_on && demo.0 == 100 {
        run.space_campaign.pending_self_tool = Some("rallycall".into());
    }
    let campaign = &mut run.space_campaign;
    campaign.shield_left = (campaign.shield_left - dt).max(0.0);
    campaign.cloak_left = (campaign.cloak_left - dt).max(0.0);
    campaign.rally_left = (campaign.rally_left - dt).max(0.0);
    for clock in campaign.generic_clocks.values_mut() {
        clock.step(-1.0, false, false, dt);
    }
    let Some(name) = campaign.pending_self_tool.take() else { return };
    let Some(record) = space_tools::record(&name) else { return };
    let Some(effect) = self_use(record) else { return };
    let owned = campaign.has_tool(&name) || campaign.charges(&name) > 0;
    if !owned {
        campaign.message = format!("You do not own the {name}");
        return;
    }
    if let Err(fail) = space_tools::tool_gate(record, &space_tools::GateContext::default()) {
        campaign.message = fail.text.to_string();
        return;
    }
    let clock = campaign.generic_clocks.entry(name.clone()).or_default();
    if clock.recharge_left() > 0.0 {
        let left = clock.recharge_left();
        campaign.message = format!("The {name} is recharging ({left:.0} s)");
        return;
    }
    let cost = rules::tool_use_energy(record.energy_cost, record.auto_fire_rate, dt);
    if !rules::tool_can_afford_energy(campaign.energy, cost) {
        campaign.message = format!("Not enough energy for the {name} (recharge at a planet)");
        return;
    }
    let max_energy = campaign.max_reactor_energy();
    match effect {
        SelfUse::Repair { percent } => {
            let Ok(mut health) = health.single_mut() else { return };
            if !can_restore(health.hp, health.max) {
                campaign.message = "Nothing to repair: the ship is undamaged".into();
                return;
            }
            health.hp = (health.hp + repair_amount(health.max, percent)).min(health.max);
            campaign.message = format!("{name}: hull at {:.0} of {:.0}", health.hp, health.max);
        }
        SelfUse::EnergyRepair { percent } => {
            if !can_restore(campaign.energy, max_energy) {
                campaign.message = "Nothing to recharge: the reactor is full".into();
                return;
            }
            campaign.energy = (campaign.energy + repair_amount(max_energy, percent)).min(max_energy);
            campaign.message = format!("{name}: reactor at {:.0} of {:.0}", campaign.energy, max_energy);
        }
        SelfUse::RepairAoe { min, max } => {
            // 0x1057FF0: the base WhileFiring, then the heal of the owner's UFO and of
            // every ship of the player's empire or an ally in `MaxRange` (the escort
            // formation flies inside it). There is no `Update` override, so the use
            // is accepted at full health too (the ammo is spent).
            let amount = repair_aoe_amount(min, max, time.elapsed_secs().fract());
            let Ok(mut health) = health.single_mut() else { return };
            health.hp = (health.hp + amount).min(health.max);
            let mut escorts = 0;
            for escort in &mut campaign.fleet {
                if escort.hp < escort.max_hp {
                    escorts += 1;
                }
                escort.hp = (escort.hp + amount).min(escort.max_hp);
            }
            campaign.message = format!("{name}: hull at {:.0} of {:.0}, {escorts} escort(s) repaired", health.hp, health.max);
        }
        SelfUse::RallyCall { multiplier, seconds } => {
            // 0x105BF80: the base `WhileFiring`, flag 3 on the player's ship and every
            // posse UFO, each posse ship's rally voice; the doubled damage runs for the
            // tool's recharge (0xBFCF10).
            campaign.rally_left = seconds;
            for escort in &campaign.fleet {
                campaign.posse_events.push(PosseEvent::Rally(escort.empire));
            }
            campaign.message = format!("{name}: your posse and you deal x{multiplier:.0} damage for {seconds:.0} s");
        }
        SelfUse::Shield => {
            campaign.shield_left = SHIELD_SECONDS;
            campaign.message = format!("Shield up for {SHIELD_SECONDS:.0} s");
        }
        SelfUse::Cloak => {
            campaign.cloak_left = CLOAK_SECONDS;
            campaign.message = format!("Cloaked for {CLOAK_SECONDS:.0} s");
        }
    }
    // The base `WhileFiring` (0x1056880) ran first for this use: any tool but the cloak
    // itself ends a cloak that is still on.
    if effect != SelfUse::Cloak {
        break_cloak(campaign);
    }
    if ends_soothing_song(record) {
        campaign.siren_song_active = 0.0;
    }
    campaign.energy = (campaign.energy - cost).max(0.0);
    if record.uses_ammo() {
        campaign.take_charge(&name);
    }
    if let Some(clock) = campaign.generic_clocks.get_mut(&name) {
        clock.commit(record.recharge_rate);
    }
    // The rally effect (`SG_ufo_rally_call`) plays on the ship, the other tools' on the world below.
    match (effect, ship.single()) {
        (SelfUse::RallyCall { .. }, Ok(at)) => crate::space_tool_fx::request_shot(&mut commands, &name, at.translation, at.translation),
        _ => crate::space_tool_fx::request(&mut commands, &name, None),
    }
    run.request_save();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> &'static ToolRecord {
        space_tools::record(name).unwrap_or_else(|| panic!("no tool {name}"))
    }

    #[test]
    fn potions_restore_a_percentage_of_the_maximum() {
        assert_eq!(self_use(tool("repairpotion")), Some(SelfUse::Repair { percent: 25.0 }));
        assert_eq!(self_use(tool("repairmegapotion")), Some(SelfUse::Repair { percent: 100.0 }));
        assert_eq!(self_use(tool("energypotion")), Some(SelfUse::EnergyRepair { percent: 25.0 }));
        assert_eq!(self_use(tool("energymegapotion")), Some(SelfUse::EnergyRepair { percent: 100.0 }));
        assert_eq!(repair_amount(1000.0, 25.0), 250.0);
        assert_eq!(repair_amount(150.0, 100.0), 150.0);
        // Update 0x1056C80: only while below the maximum.
        assert!(can_restore(900.0, 1000.0));
        assert!(!can_restore(1000.0, 1000.0));
        // ProcessCost 0x10535F0: the missing fraction of the use cost.
        assert_eq!(repair_cost(1000, 500.0, 1000.0), 500);
        assert_eq!(repair_cost(1000, 1000.0, 1000.0), 0);
        assert_eq!(repair_cost(1000, 0.0, 0.0), 0);
    }

    #[test]
    fn shield_and_cloak_are_use_on_activate_tools_with_the_installed_energy_and_recharge() {
        let shield = tool("shield");
        let cloak = tool("cloak");
        assert_eq!(self_use(shield), Some(SelfUse::Shield));
        assert_eq!(self_use(cloak), Some(SelfUse::Cloak));
        assert_eq!((shield.energy_cost, shield.recharge_rate), (20.0, 180.0));
        assert_eq!((cloak.energy_cost, cloak.recharge_rate), (20.0, 270.0));
        assert!(self_use(tool("laserlevel1")).is_none());
        assert!(self_use(tool("heatray")).is_none());
    }

    #[test]
    fn shield_and_cloak_durations_and_effects_are_the_retail_ones() {
        // The sweep 0x1009F20: cloak 0x15F90 ms, shield 0xEA60 ms; the song 0x1D4C0 ms.
        assert_eq!(CLOAK_SECONDS, 90.0);
        assert_eq!(SHIELD_SECONDS, 60.0);
        assert_eq!(SOOTHING_SONG_SECONDS, 120.0);
        // Any other tool use ends a cloak (0x1056880); the cloak tool does not end itself.
        let mut campaign = crate::space::SpaceCampaign::default();
        campaign.cloak_left = CLOAK_SECONDS;
        break_cloak(&mut campaign);
        assert_eq!(campaign.cloak_left, 0.0);
        // The soothing song: a negative relationship reads as 0 (not against the Grox).
        assert_eq!(soothed_relationship(-8.0, true, false), 0.0);
        assert_eq!(soothed_relationship(-8.0, true, true), -8.0);
        assert_eq!(soothed_relationship(-8.0, false, false), -8.0);
        assert_eq!(soothed_relationship(4.0, true, false), 4.0);
    }

    #[test]
    fn the_soothing_song_holds_every_relationship_at_neutral_but_the_groxs() {
        use crate::space::SpaceCampaign;
        let mut campaign = SpaceCampaign::default();
        campaign.apply_relationship(3, RelationshipEvent::BadToolUse, 20.0);
        campaign.apply_relationship(0, RelationshipEvent::BadToolUse, 20.0);
        assert!(campaign.relationship(3) < 0.0 && campaign.relationship(0) < 0.0);
        campaign.siren_song_active = SOOTHING_SONG_SECONDS;
        assert_eq!(campaign.relationship(3), 0.0);
        assert!(campaign.relationship(0) < 0.0, "the Grox are not soothed");
        campaign.siren_song_active = 0.0;
        assert!(campaign.relationship(3) < 0.0);
        // Fighting ends the song: weapon-type tools do, the potions and the song itself do not.
        for name in ["laserlevel1", "pulselevel1", "shield", "aoe_repair"] {
            assert!(ends_soothing_song(tool(name)), "{name}");
        }
        for name in ["repairpotion", "soothingsong", "cloak", "heatray"] {
            assert!(!ends_soothing_song(tool(name)), "{name}");
        }
    }

    #[test]
    fn the_area_repair_heals_between_min_and_max_damage() {
        let record = tool("aoe_repair");
        assert_eq!(self_use(record), Some(SelfUse::RepairAoe { min: 50_000.0, max: 50_000.0 }));
        assert!(record.has(flag::USE_ON_ACTIVATE) && record.uses_ammo());
        assert_eq!((record.recharge_rate, record.energy_cost, record.max_range), (30.0, 0.0, 500.0));
        assert_eq!(repair_aoe_amount(50_000.0, 50_000.0, 0.37), 50_000.0);
        assert_eq!(repair_aoe_amount(10.0, 30.0, 0.5), 20.0);
        assert_eq!(repair_aoe_amount(10.0, 30.0, 2.0), 30.0);
        assert!(generic_palette_tools().iter().any(|r| r.name == "aoe_repair"));
    }

    #[test]
    fn the_rally_call_doubles_the_damage_of_the_players_side_for_its_recharge() {
        let record = tool("rallycall");
        assert_eq!(self_use(record), Some(SelfUse::RallyCall { multiplier: 2.0, seconds: 60.0 }));
        assert!(record.has(flag::USE_ON_ACTIVATE) && !record.uses_ammo());
        assert_eq!((record.energy_cost, record.recharge_rate, record.max_damage), (30.0, 60.0, 2.0));
        assert!(generic_palette_tools().iter().any(|r| r.name == "rallycall"));
        // Sold as an upgrade (`toolrallycall`, 750000, tier 5, warmonger-3 or diplomat-3).
        let item = rules::store_item_for_tool("rallycall").expect("store row");
        assert_eq!((item.cost, item.tier, item.badge, item.badge_or), (750_000.0, Some(5), Some("warmonger-3"), Some("diplomat-3")));
        let mut campaign = SpaceCampaign::default();
        assert_eq!(rally_multiplier(&campaign), 1.0);
        campaign.rally_left = 30.0;
        assert_eq!(rally_multiplier(&campaign), 2.0);
        campaign.rally_left = 0.0;
        assert_eq!(rally_multiplier(&campaign), 1.0);
    }

    #[test]
    fn a_posse_ship_takes_only_hostile_fire_and_its_death_is_told() {
        use crate::space::EscortShip;
        assert!(escort_takes_damage_from(Attacker::Hostile));
        assert!(!escort_takes_damage_from(Attacker::Player) && !escort_takes_damage_from(Attacker::Posse));
        let mut campaign = SpaceCampaign::default();
        campaign.fleet.push(EscortShip { name: "Kroxid".into(), empire: Some(2), hp: 100.0, max_hp: 100.0 });
        campaign.fleet.push(EscortShip { name: "Mini-U".into(), empire: None, hp: 50.0, max_hp: 50.0 });
        let before = campaign.relationship(2);
        // The first damage at full health calls the under-attack voice, the later ones do not.
        assert!(hurt_escort(&mut campaign, 0, 40.0).is_none());
        assert!(hurt_escort(&mut campaign, 0, 10.0).is_none());
        assert_eq!(campaign.fleet[0].hp, 50.0);
        assert_eq!(campaign.posse_events, vec![PosseEvent::UnderAttack(Some(2))]);
        // No damage, no event; an unknown slot is ignored.
        assert!(hurt_escort(&mut campaign, 0, 0.0).is_none() && hurt_escort(&mut campaign, 9, 5.0).is_none());
        // Destroyed: dead voice, the owner's `DestroyAllyUFO` relationship, the ship leaves the posse.
        let news = hurt_escort(&mut campaign, 0, 60.0).expect("death news");
        assert!(news.contains("destroyed"), "{news}");
        assert_eq!(campaign.fleet.len(), 1);
        assert_eq!(campaign.posse_events.last(), Some(&PosseEvent::Dead(Some(2))));
        assert!(campaign.relationship(2) < before, "{} < {before}", campaign.relationship(2));
        // The player's own empire's ship: no relationship event.
        let rel2 = campaign.relationship(2);
        assert!(kill_escort(&mut campaign, 0).is_some());
        assert!(campaign.fleet.is_empty());
        assert_eq!(campaign.posse_events.last(), Some(&PosseEvent::Dead(None)));
        assert_eq!(campaign.relationship(2), rel2);
        // This frame's fire on two ships applies from the last slot down.
        campaign.fleet.push(EscortShip { name: "A".into(), empire: Some(3), hp: 10.0, max_hp: 10.0 });
        campaign.fleet.push(EscortShip { name: "B".into(), empire: Some(4), hp: 10.0, max_hp: 10.0 });
        let news = apply_escort_fire(&mut campaign, &[20.0, 4.0]);
        assert_eq!(news.len(), 1);
        assert_eq!(campaign.fleet.len(), 1);
        assert_eq!(campaign.fleet[0].name, "B");
        assert_eq!(campaign.fleet[0].hp, 6.0);
    }

    #[test]
    fn the_posse_fires_at_the_players_target_when_it_can_reach_it() {
        let player = Vec3::ZERO;
        let escort = Vec3::new(3.0, 0.0, 0.0);
        // A is the hostile nearest to the player (his target), B the one nearest to the escort.
        let a = Vec3::new(-10.0, 0.0, 0.0);
        let b = Vec3::new(12.0, 0.0, 0.0);
        assert_eq!(posse_target(player, escort, &[b, a], 32.0), Some(a));
        // Out of the escort's reach the escort picks the hostile nearest to it.
        assert_eq!(posse_target(player, Vec3::new(40.0, 0.0, 0.0), &[a, b], 32.0), Some(b));
        assert_eq!(posse_target(player, escort, &[], 32.0), None);
        assert_eq!(posse_target(player, escort, &[Vec3::new(100.0, 0.0, 0.0)], 32.0), None);
    }

    #[test]
    fn the_backup_ship_stays_for_the_tool_recharge_then_is_destroyed() {
        let mut campaign = SpaceCampaign::default();
        let mut run = RunProgress::default();
        campaign.set_archetype(SpaceArchetype::Knight);
        assert!(matches!(campaign.try_activate_superweapon(&mut run, None), crate::space::WorldAction::Done(_)));
        assert_eq!(campaign.fleet.len(), 1);
        assert_eq!(campaign.fleet[0].empire, None, "a ship of the player's own empire (AddPosse 0x1002650)");
        assert_eq!(campaign.superweapon_cooldown, 120.0);
        let mut save = false;
        let mut spice = 0.0;
        campaign.tick(119.0, false, &mut spice, &mut save);
        assert_eq!(campaign.fleet.len(), 1);
        campaign.tick(2.0, false, &mut spice, &mut save);
        assert!(campaign.fleet.is_empty(), "0x1000C60: destroyed once the tool is ready again");
        assert_eq!(campaign.posse_events.last(), Some(&PosseEvent::Dead(None)));
        // A full posse refuses the use (gate 0x1056130, log 0x72CB7F2C).
        campaign.superweapon_cooldown = 0.0;
        for n in 0..5 {
            campaign.fleet.push(crate::space::EscortShip { name: format!("ally {n}"), empire: Some(2), hp: 10.0, max_hp: 10.0 });
        }
        assert!(matches!(campaign.try_activate_superweapon(&mut run, None), crate::space::WorldAction::Refused(msg) if msg.contains("posse is full")));
    }

    #[test]
    fn on_hit_turns_the_delta_sign_into_good_or_bad_tool_use() {
        assert_eq!(relationship_event(0.5), Some((RelationshipEvent::GoodToolUse, 0.5)));
        assert_eq!(relationship_event(-5.0), Some((RelationshipEvent::BadToolUse, 5.0)));
        assert_eq!(relationship_event(0.0), None);
        let wave = hit_deltas(tool("gravitationwave"), SpaceToolHit::Ground);
        assert_eq!((wave.relationship, wave.galactic, wave.galactic_radius), (-20.0, -4.0, 10.0));
        let buster = hit_deltas(tool("planetbusterbomb"), SpaceToolHit::Ground);
        assert_eq!(buster.velocity.atmosphere, -0.2);
        assert_eq!(buster.relationship, -20.0);
        let bomb = hit_deltas(tool("dumbbomblevel1"), SpaceToolHit::Combatant);
        assert_eq!(bomb.relationship, -0.5);
        assert!(bomb.velocity.is_zero());
    }

    #[test]
    fn the_antimatter_weapons_fire_from_their_records() {
        let bomb = weapon("antimatterbomb").expect("antimatter bomb row");
        assert_eq!((bomb.damage, bomb.area, bomb.recharge), (1_000_000.0, 140.0, 20.0));
        assert_eq!(bomb.max_range, 300.0);
        assert_eq!(projectile_speed("antimatterbomb"), 40.0);
        let missile = weapon("antimattermissile").expect("antimatter missile row");
        assert_eq!((missile.autofire, missile.min_range, missile.max_range), (5.0, 10.0, 500.0));
        assert_eq!(projectile_speed("antimattermissile"), 250.0);
        // The curated rows win and keep their old numbers; projectile speeds come from the record.
        assert_eq!(weapon("pulselevel1"), rules::player_weapon("pulselevel1"));
        for id in ["pulselevel1", "protonmissilelevel1"] {
            assert_eq!(projectile_speed(id), rules::projectile_speed(id), "{id}");
        }
        // The record corrects the old hand-written constant: the dumb bomb flies at 35, not 40.
        assert_eq!(projectile_speed("dumbbomblevel1"), 35.0);
        assert_eq!(rules::projectile_speed("dumbbomblevel1"), 40.0);
        // Not weapons F fires: the terraforming bomb, potions, beams, the shield.
        for id in ["planetbusterbomb", "repairpotion", "heatray", "shield", "monolith"] {
            assert!(record_weapon(tool(id)).is_none(), "{id}");
        }
    }

    #[test]
    fn the_generic_palette_tools_are_the_combat_panel_extras() {
        let names: Vec<&str> = generic_palette_tools().iter().map(|r| r.name).collect();
        for name in ["antimatterbomb", "antimattermissile", "shield", "cloak", "repairpotion", "repairmegapotion", "energypotion", "energymegapotion", "aoe_repair"] {
            assert!(names.contains(&name), "{name}");
        }
        assert!(!names.contains(&"pulselevel1"), "the curated weapons keep their own rows");
        assert!(!names.contains(&"planetbusterbomb"));
    }
}
