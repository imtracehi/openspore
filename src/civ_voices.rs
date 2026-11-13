//! Civilization vehicle voices and event sounds (helper `sound-voices`, `notes\audio-audit.md`
//! gap G4; extends `audio_audit`).
//!
//! The retail vehicle voices are static id tables in `.rdata`; each row holds one id per
//! vehicle purpose (`cVehicle::mPurpose` at +0xB20: 0 Military, 1 Cultural = Religious, 2
//! Economic). A voice is not played directly: `0xDCC290` adds the id to a per-frame set of the
//! vehicle manager (`0xDC7D00`, singleton at 0x1699AB8, set at +0x24) and `0xDCED40` (the
//! manager's update) plays every id of the set once at the end of the frame (an
//! `AudioSystem` event 0x3475365 with the id as parameter 0x3475381), then clears it. So
//! twenty vehicles of one class that get the same order say it once; `Frame` below is that set.
//!
//! VERIFIED (march2017 addresses, `swarm-volume\target\re\dis.py`):
//! * Select: `cVehicle::SetSelected` 0xC9F8E0 / 0xCA34E0 -> 0xC9F850 queues
//!   `ui_veh_select_land|water|air` (table 0x14749D0 by locomotion, +0xB1C) and the class
//!   `*_Gtg` (table 0x14749C4) when a vehicle becomes selected in Civ mode (0x1654C04).
//!   `Gtg` is the SELECT voice ("good to go"); `audio_audit` played it on move orders.
//! * Orders: 0xDCC5A0 (called by the seven command setters 0xDCC6C7, 0xDCCDF3, 0xDCCF2F,
//!   0xDCCFB8, 0xDCD0F7, 0xDCDA36, 0xDD02A4) picks a row by the order: attack on a
//!   `cCreatureAnimal` that is an epic (0xC0C240) -> `Attack_Epic` (table 0x147C670), a capture
//!   order (type 8) on a commodity node (noun 0x403DF5C, the spice geyser) -> `Capture_Spice`
//!   (0x147C67C), on anything else -> `Capture_GoodieHut` (0x147C688), every other order ->
//!   `Ok` (0x147C664). The `Attack_Epic` row is stored Military, Economic, Religious (a retail
//!   quirk kept: a religious vehicle says the economic line).
//! * Target accepted: `sfx_civ_target_geyser|building|vehicle|hut` (the target-click routines
//!   0xCF3FF0 / 0xCF4309 / 0xCF44F9 / 0xCF40BF) once the orders on that target type are given.
//! * Ground order accepted (Round 3, `audio-round3`): `ui_trg_target_select` (id 0xC45C1574). The
//!   Civ go-to command 0xCF2680 (the formation move of the selection to a point, called by 0xCF6E63)
//!   ends with: at least one vehicle took the order -> a ping effect at the point and
//!   `PlayAudio(ui_trg_target_select)` (0xCF29E6 / 0xCF29F0); none -> `ui_civ_error` (0xCF29F6, id
//!   0x594303B7). The twin command 0xCD0450 (called by 0xCD0920 / 0xCD0CFD) plays the same sound
//!   through the audio event 0x3475365 (parameter 0x3475381 = the id, 0xCD0490). The `ui_trg_` prefix is
//!   a leftover name: both sites are in the Civ command code, none in the Tribal range.
//! * Trade drop-off: `Econ_DropOff_Trade` (0xDCCBA6, in the trade command after the unload at the
//!   foreign city): the player's economic vehicle, one chance in three.
//! * Cannot perform: 0xDCC550 (`Cant_Perform`, table 0x147C658), one call site, the trade
//!   command's exit "(23) trade: bad foreign city" 0xDCCD3C.
//! * Stance (0xC9FC00): Attack (stance 0, table 0x14749DC) / Defense (stance 1, 0x14749F0),
//!   not when the silent flag is set.
//! * Hit: the damage routine (~0xCA7203, Civ branch 0xCA72A7): only for the player's vehicles
//!   (`[this+0x34]->vtable[0x58]`), only when the vehicle's combat voice timer
//!   (`mCombatVoxTimer`, +0xCA8, started by `cVehicle::Load` 0xCA67F9) shows more than 60 s;
//!   the timer restarts; the voice is `*_AttackedBy_Religious` (0x1474A88) when the attacker is
//!   a vehicle of the religious purpose, else `*_Takefire` (0x1474A7C). Round 3: OpenSpore's
//!   religious vehicles shoot cultural projectiles (`CivShot::cultural`), which reset
//!   `CivUnit::cultural_hit` on the victim, so a hit that also resets it is a religious attack.
//! * Super-weapon voices (the status-effect routine 0xCAB6F0, voice of the AFFECTED vehicle's
//!   class): Invulnerability (type 15, table 0x1474E84, only when the caster is the owner),
//!   EMP (type 14, 0x1474E78), Bribed (type 4, 0x1474E6C, the caster is not the owner),
//!   FaithHeal (0x1474E60).
//! * City takeover (0xD5B3A0): the cinematic variant `sfx_cin_city_takeover_<specialty>`
//!   (table 0x147B950) or, outside the cinematic, the positional `city_takeover_<specialty>`
//!   (0x147B944); `sfx_civ_npc_takes_city` (0xBE8E30) when a city changes hands to a
//!   civilization that is not the player's (the same routine as `sfx_civ_losecity`).
//! * Trade route sounds (`ui_civ_traderoute_propose|create|reject`, 0xDCA1D9 / 0xDCA3E1 /
//!   0xDCA5B2): the proposal, the accepted route and the refusal of the route state machine.
//! * Relationship event-log templates (installed props `Relationship0AtWar` ..
//!   `Relationship4Allied`, group 0xAA9A8ED7): sound `civ_ui_at_war` (levels 0 and 1),
//!   `civ_ui_alliance_levelup` (level 2), `civ_ui_alliance_leveldown` (level 3),
//!   `civ_ui_form_alliance` (level 4) - the data is as installed.
//!
//! Goodie huts (helper `civ-goodies`): `Capture_GoodieHut` is the order voice of a capture order on a
//! hut (`UnitOrder::Hut`, `civ_orders::hut_command`), `sfx_civ_target_hut` plays once at the hut (0xCF40BF),
//! `New_Goodie` plays when the hut gave the player a free vehicle (`GoodieCue`, 0xDCB202).
//!
//! ADAPTED (the sound is retail, the OpenSpore trigger is mine):
//! * `Attack_Epic`: military vehicles now take the retail attack order on the epic (`UnitOrder::Epic`,
//!   type 1 of 0xDCC5A0); religious and economic ones still drive to its spot and say the same voice
//!   (marked `EpicOrder` by `civ_rts::rts_input`). `AttackedBy_Religious` plays when a cultural projectile
//!   hits (see Hit above).
//! * `Cant_Perform` also plays when a religious / economic vehicle is told to attack a vehicle
//!   (the order is refused); the retail site is the trade exit above, mapped to a rival
//!   refusing a trade route (the `Econ` voice).
//! * The vehicle voices of the super weapons key on the OpenSpore effects: your whole fleet
//!   for Invulnerability / FaithHeal, the vehicles that become stunned (EMP) or turned (Bribed).
//! * Relationship sounds play when a nation's attitude index (`civ_comm::stance`) changes (the
//!   level entered) or its war flag rises; the retail event is the relationship manager's.
//! * The armed / targeting sounds of the abilities (helper `voices-wire`): retail's cursor-tool
//!   update (0xCEA320) plays the three ids of the tool's row of table 0x1477DF0 (12 bytes per
//!   tool: a one-shot, a loop, a start sound) when a tool is armed and keeps the loops until the
//!   tool is dropped or fired. OpenSpore armed nothing (the keys fired at once): a power key now
//!   ARMS the tool (the armed sounds start, `civ_rts::RtsState::armed`), the second press of the
//!   key launches it at the camera focus (ADAPTED: retail launches with the next click on the
//!   planet), a right-click order drops it; `OPENSPORE_CIV_ARMING=0` restores the old instant fire
//!   (the one-shots then play at the launch). The row of a tool is INFERRED from the names
//!   (`faithheal`, `blackcloud`, bribe = `espionage`, `adblitz`, `tacnuke` = `sfx_tact_icbm_*`,
//!   napalm = the walla row); the table's tool order was not decoded. The loops are not 3D
//!   (`LoopAt::Everywhere`).
//! * Wall turrets (0xE8B780, per frame per turret): while the camera is within 200 units of a turret
//!   and the turret's orientation changed since the last frame, `sfx_civ_turret_rotateb_lp` plays at
//!   the turret (`PlayProceduralAudio` of its id at the turret's position); it stops when the turret
//!   rests or the camera is far. The idle turrets of retail look around too (0xBD1E70,
//!   `civ_fx::TurretScan`, ported by `civ-harvest`), so every turn plays the loop, a target or not.
//!   ADAPTED: the orientation test has a small tolerance.
//! * Positional one-shots (the target-accepted sounds at their target, the trade drop-off at the
//!   foreign city) play through `Hearing::play`: the soundProp's own falloff applies.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    city_presentation::CityTurretVisual,
    civ::{Approach, CivCampaign, PowerSlot, UnitOrder, VehicleDomain, CITY_COUNT, HOME_INDEX},
    civ_controls::CivStance,
    civ_rts::{CivUnit, RtsState, Selected, UnitAction},
    game::RunProgress,
    game_audio::Bus,
    loop_audio::{Hearing, LoopSounds, LoopSpec},
    stages::{GameStage, StageOwned},
};

// ---------------------------------------------------------------------------
// Original data: the tables (one id per purpose Military, Religious, Economic)
// ---------------------------------------------------------------------------

/// 0x14749C4: the select voice.
pub const GTG: [&str; 3] = ["Mili_Gtg", "Reli_Gtg", "Econ_Gtg"];
/// 0x14749DC / 0x14749F0.
pub const STANCE_ATTACK: [&str; 3] = ["Mili_Stance_Attack", "Reli_Stance_Attack", "Econ_Stance_Attack"];
pub const STANCE_DEFENSE: [&str; 3] = ["Mili_Stance_Defense", "Reli_Stance_Defense", "Econ_Stance_Defense"];
/// 0x1474A7C / 0x1474A88.
pub const TAKEFIRE: [&str; 3] = ["Mili_Takefire", "Reli_Takefire", "Econ_Takefire"];
pub const ATTACKED_BY_RELIGIOUS: [&str; 3] =
    ["Mili_AttackedBy_Religious", "Reli_Attackedby_Religious", "Econ_AttackedBy_Religious"];
/// 0x1474E60 .. 0x1474E8C.
pub const FAITH_HEAL: [&str; 3] = ["Mili_Faithheal_SW", "Reli_FaithHeal_SW", "Econ_FaithHeal_SW"];
pub const BRIBED: [&str; 3] = ["Mili_Bribed_SW", "Reli_Bribed_SW", "Econ_Bribed_SW"];
pub const EMP: [&str; 3] = ["Mili_EMP_SW", "Reli_EMP_SW", "Econ_EMP_SW"];
pub const INVULNERABILITY: [&str; 3] =
    ["Mili_Invulnerability_SW", "Reli_Invulnerability_SW", "Econ_Invulnerability_SW"];
/// 0x147C5DC.
pub const NEW_GOODIE: [&str; 3] = ["Mili_New_Goodie", "Reli_New_Goodie", "Econ_New_Goodie"];
/// 0x147C658.
pub const CANT_PERFORM: [&str; 3] = ["Mili_Cant_Perform", "Reli_Cant_Perform", "Econ_Cant_Perform"];
/// 0x147C664 (the generic order voice).
pub const OK: [&str; 3] = ["Mili_Ok", "Reli_Ok", "Econ_Ok"];
/// 0x147C670: stored Military, Economic, Religious in the retail table.
pub const ATTACK_EPIC: [&str; 3] = ["Mili_Attack_Epic", "Econ_Attack_Epic", "Reli_Attack_Epic"];
/// 0x147C67C / 0x147C688.
pub const CAPTURE_SPICE: [&str; 3] = ["Mili_Capture_Spice", "Reli_Capture_Spice", "Econ_Capture_Spice"];
pub const CAPTURE_GOODIE_HUT: [&str; 3] =
    ["Mili_Capture_GoodieHut", "Reli_Capture_GoodieHut", "Econ_Capture_GoodieHut"];
/// 0x147B944 / 0x147B950: the takeover sounds by the city's specialty.
pub const TAKEOVER: [&str; 3] = ["city_takeover_mili", "city_takeover_relig", "city_takeover_econ"];
pub const TAKEOVER_CINEMATIC: [&str; 3] =
    ["sfx_cin_city_takeover_mili", "sfx_cin_city_takeover_relig", "sfx_cin_city_takeover_econ"];

/// The order-accepted sounds by target (the target-click routines 0xCF3E25 geyser 0xCF3FF0 /
/// hut 0xCF40BF, 0xCF4259 building 0xCF4309, 0xCF43B7 vehicle 0xCF44F9: after the orders are
/// given and at least one vehicle accepted, positional at the target with the soundProp's own
/// falloff, `Hearing::play`).
pub const TARGET_GEYSER: &str = "sfx_civ_target_geyser";
pub const TARGET_HUT: &str = "sfx_civ_target_hut";
pub const TARGET_BUILDING: &str = "sfx_civ_target_building";
pub const TARGET_VEHICLE: &str = "sfx_civ_target_vehicle";
/// A go-to order on the ground was accepted (0xCF29E6, 0xCD0490).
pub const TARGET_SELECT: &str = "ui_trg_target_select";

/// 0xDCCBA6: an economic vehicle of yours unloads at a foreign city (one chance in three,
/// `RandomInt(3) == 0`; positional procedural audio there, with the soundProp's own falloff).
pub const DROP_OFF_TRADE: &str = "Econ_DropOff_Trade";

pub const NPC_TAKES_CITY: &str = "sfx_civ_npc_takes_city";
pub const TRADE_PROPOSE: &str = "ui_civ_traderoute_propose";
pub const TRADE_CREATE: &str = "ui_civ_traderoute_create";
pub const TRADE_REJECT: &str = "ui_civ_traderoute_reject";
/// The event-log relationship templates, level 0 .. 4 (installed props, group 0xAA9A8ED7).
pub const RELATIONSHIP: [&str; 5] = [
    "civ_ui_at_war",
    "civ_ui_at_war",
    "civ_ui_alliance_levelup",
    "civ_ui_alliance_leveldown",
    "civ_ui_form_alliance",
];
/// The launch / start sounds of the abilities (tables 0x1468864 and 0x1477E40; the
/// tool -> sound pairing is by name, INFERRED).
pub const NAPALM_LAUNCH: &str = "sfx_civ_napalm_launch";
pub const NUKE_LAUNCH: &str = "sfx_civ_tactical_nuke_launch";
pub const GLOBAL_NUCLEAR_WAR: &str = "sfx_civ_global_nuclear_war";
pub const ECON_SUPER: &str = "sfx_civ_econ_super";
pub const BRIBE_START: &str = "sfx_civ_bribe_start";
pub const ADBLITZ_START: &str = "sfx_civ_adblitz_start";

/// The purpose index of a vehicle class (`cVehicle::mPurpose`).
pub fn slot(kind: Approach) -> usize {
    match kind {
        Approach::Military => 0,
        Approach::Religious => 1,
        Approach::Economic => 2,
    }
}

/// `ui_veh_select_land|water|air` by the vehicle's domain (table 0x14749D0).
pub fn select_sound(domain: VehicleDomain) -> &'static str {
    match domain {
        VehicleDomain::Land => "ui_veh_select_land",
        VehicleDomain::Sea => "ui_veh_select_water",
        VehicleDomain::Air => "ui_veh_select_air",
    }
}

/// The voice of a new order of the player (0xDCC5A0). `epic`: the order attacks an epic
/// creature (`UnitOrder::Epic` for military vehicles, the retail attack order type 1; the other
/// classes still drive to the epic's spot); a capture order (type 8) on a spice geyser says
/// `Capture_Spice`, on a goodie hut `Capture_GoodieHut`.
pub fn order_voice(kind: Approach, order: UnitOrder, epic: bool) -> Option<&'static str> {
    let k = slot(kind);
    match order {
        UnitOrder::Idle => None,
        _ if epic => Some(ATTACK_EPIC[k]),
        UnitOrder::Epic(_) => Some(ATTACK_EPIC[k]),
        UnitOrder::Geyser(_) => Some(CAPTURE_SPICE[k]),
        UnitOrder::Hut(_) => Some(CAPTURE_GOODIE_HUT[k]),
        _ => Some(OK[k]),
    }
}

/// The target-accepted sound of an order: positional at a geyser / building / vehicle, and the
/// plain `ui_trg_target_select` for a drive-to-point order (0xCF29E6).
pub fn target_sound(order: UnitOrder) -> Option<&'static str> {
    match order {
        UnitOrder::Geyser(_) => Some(TARGET_GEYSER),
        UnitOrder::City(_) => Some(TARGET_BUILDING),
        UnitOrder::Unit(_) => Some(TARGET_VEHICLE),
        UnitOrder::Hut(_) => Some(TARGET_HUT),
        UnitOrder::Move(_) | UnitOrder::Goto(_) => Some(TARGET_SELECT),
        // The attack on an epic is not one of the four target-click routines (0xCF3FF0 / 0xCF4309 /
        // 0xCF44F9 / 0xCF40BF).
        UnitOrder::Epic(_) | UnitOrder::Idle => None,
    }
}

/// The retail one-in-three roll of the trade drop-off voice.
pub fn drop_off_plays(roll: u32) -> bool {
    roll % 3 == 0
}

/// The capture voice for a goodie hut (the order voice of a `UnitOrder::Hut`).
pub fn goodie_hut_voice(kind: Approach) -> &'static str {
    CAPTURE_GOODIE_HUT[slot(kind)]
}

/// The voice of a hit vehicle (damage routine 0xCA72A7): religious attackers get their own.
pub fn hit_voice(kind: Approach, attacker_religious_vehicle: bool) -> &'static str {
    if attacker_religious_vehicle {
        ATTACKED_BY_RELIGIOUS[slot(kind)]
    } else {
        TAKEFIRE[slot(kind)]
    }
}

/// The voice of a stance change (0xC9FC00).
pub fn stance_voice(kind: Approach, stance: CivStance) -> &'static str {
    match stance {
        CivStance::Aggressive => STANCE_ATTACK[slot(kind)],
        CivStance::StandGround => STANCE_DEFENSE[slot(kind)],
    }
}

/// Seconds between two combat voices of one vehicle (`mCombatVoxTimer` > 0xEA60 ms).
pub const COMBAT_VOX_SECONDS: f64 = 60.0;

/// What an ability plays when it fires: the launch sound and the class voices of the vehicles
/// it affects on your side (`None` when the affected vehicles are found by their state).
pub struct PowerSounds {
    pub launch: Option<&'static str>,
    pub own_fleet_voice: Option<[&'static str; 3]>,
}

pub fn power_sounds(tool: &str) -> PowerSounds {
    let (launch, own_fleet_voice) = match tool {
        "i_field" => (None, Some(INVULNERABILITY)),
        "faithheal" => (None, Some(FAITH_HEAL)),
        "napalm" => (Some(NAPALM_LAUNCH), None),
        "tacnuke" => (Some(NUKE_LAUNCH), None),
        // `*_start` are the arm-time sounds of table 0x1477DF0 (`armed_sounds`).
        "espionage" | "adblitz" => (None, None),
        "icbm" => (Some(GLOBAL_NUCLEAR_WAR), None),
        "globaladblitz" => (Some(ECON_SUPER), None),
        _ => (None, None),
    };
    PowerSounds { launch, own_fleet_voice }
}

/// The relationship event sound of a level (0 .. 4) entered.
pub fn relationship_sound(level: usize) -> &'static str {
    RELATIONSHIP[level.min(4)]
}

/// The takeover sound of a captured city by its specialty (0 military, 1 religious, 2
/// economic); `cinematic`: the capture cinematic plays (OpenSpore always plays it).
pub fn takeover_sound(specialty: usize, cinematic: bool) -> &'static str {
    let s = specialty.min(2);
    if cinematic {
        TAKEOVER_CINEMATIC[s]
    } else {
        TAKEOVER[s]
    }
}

/// Table 0x1477DF0: the one-shot every targeted tool plays when armed, and the loops / start
/// sounds of its row.
pub const TARGET_ARMED: &str = "sfx_napalm_target_armed";
pub const TARGET_WALLA_LP: &str = "sfx_civ_target_walla_lp";
pub const TACNUKE_ARMED_LP: &str = "sfx_tact_icbm_target_armed_lp";
pub const FAITHHEAL_ARMED_LP: &str = "sfx_faithheal_armed_lp";
pub const BLACKCLOUD_ARMED_LP: &str = "sfx_blackcloud_armed_lp";
pub const BRIBE_ARMED_LP: &str = "sfx_civ_bribe_armed_lp";
pub const ADBLITZ_ARMED_LP: &str = "sfx_civ_adblitz_armed_lp";

/// What arming a tool plays: one-shots (once, on arming) and loops (until it is dropped or fired).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmedSounds {
    pub once: &'static [&'static str],
    pub looped: &'static [&'static str],
}

/// The row of table 0x1477DF0 of an ability tool. The row of a tool is its retail tool index (table
/// 0x14693E8, `civ_target::TOOLS`, VERIFIED by `civ-target`); the rows of the superweapons `icbm`,
/// `rising` and `globaladblitz` are empty (they launch without arming).
pub fn armed_sounds(tool: &str) -> ArmedSounds {
    let (once, looped): (&'static [&'static str], &'static [&'static str]) = match tool {
        "napalm" => (&[TARGET_ARMED], &[TARGET_WALLA_LP]),
        "tacnuke" => (&[TARGET_ARMED], &[TACNUKE_ARMED_LP]),
        "faithheal" => (&[TARGET_ARMED], &[FAITHHEAL_ARMED_LP]),
        "blackcloud" => (&[], &[BLACKCLOUD_ARMED_LP]),
        // Rows 6 and 11 are the same: espionage and diplodervish.
        "espionage" | "diplodervish" => (&[TARGET_ARMED, BRIBE_START], &[BRIBE_ARMED_LP]),
        "adblitz" => (&[ADBLITZ_START], &[ADBLITZ_ARMED_LP]),
        "emp" | "i_field" => (&[TARGET_ARMED], &[]),
        _ => (&[], &[]),
    };
    ArmedSounds { once, looped }
}

/// The ability data of a power slot (`None`: the slot has no ability yet).
pub fn slot_tool(campaign: &CivCampaign, slot: PowerSlot) -> Option<&'static str> {
    slot_data(campaign, slot).map(|d| d.tool)
}

pub fn slot_data(campaign: &CivCampaign, slot: PowerSlot) -> Option<&'static crate::civ_rules::PowerData> {
    match slot {
        PowerSlot::Cell => campaign.cell_power.as_ref().map(|p| p.data()),
        PowerSlot::Creature => campaign.creature_power.as_ref().map(|p| p.data()),
        PowerSlot::Tribal => campaign.tribal_power.as_ref().map(|p| p.data()),
        PowerSlot::Superweapon => Some(crate::civ::resolve_superweapon(campaign.approach).data()),
    }
}

/// Whether a power key arms its tool first (`OPENSPORE_CIV_ARMING=0` fires at once as before).
pub fn arming_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_CIV_ARMING").map_or(true, |v| v != "0"))
}

/// Retail's hearing range of a turning turret (0xE8B780: the camera within 200 units).
pub const TURRET_HEARING: f32 = 200.0;
pub const TURRET_LOOP: &str = "sfx_civ_turret_rotateb_lp";
/// ADAPTED: the turn that counts as "the orientation changed" (retail compares the quaternions
/// exactly; OpenSpore's eased turn never settles to bit equality).
pub const TURRET_TURN_EPSILON: f32 = 2.0e-4;

/// Table 0x1583E70 (1-based: 0xD5B55B reads `[state * 4 + 0x1583E6C]`): the voice loops of a city's
/// crowd, by crowd state (the sound is the event + the creature's voice bank, `voice_banks`).
/// VERIFIED names; states 8..10 are the takeover loops (`8 + city type`, 0xD5B375).
pub const CROWD_LOOPS: [&str; 12] = [
    "vcode_crowd_Neutral_loop",
    "vcode_crowd_Protest_loop",
    "vcode_crowd_Happy_loop",
    "vcode_crowd_Panic_loop",
    "vcode_crowd_Cheer_loop",
    "vcode_crowd_Angry_loop",
    "vcode_crowd_Interested_loop",
    "vcode_crowd_Surrender_loop",
    "vcode_crowd_Convert_loop",
    "vcode_crowd_Buy_Out_loop",
    "vcode_crowd_cheer_relig",
    "vcode_crowd_hate_lp",
];
/// ADAPTED: the crowd is heard with the camera this close to the city (retail: the crowd actors'
/// own 3D falloff).
pub const CROWD_HEARING: f32 = 150.0;
/// VERIFIED: the mood above which the crowd is `Happy`: the crowd's constructor 0xD5E9A0 reads
/// `RolloverHappyCap` (0x01D37A23) from the installed `ui` property list (group 0, instance
/// 0x5C770DB7, also `RolloverNeutralHappyCap` 60, `RolloverUnhappyCap` 40, `RolloverVUnhappyCap`
/// 25) into `[crowd + 0xB18]`; 0xD5B0CC compares the city's mood with it (strictly greater).
pub const CROWD_HAPPY: f32 = 75.0;

/// The base crowd state of a city (0xD5B070, 1-based index into `CROWD_LOOPS`): neutral; protest
/// when the mood is under 50 (0xBD7F40: city +0x304 < 50); happy over `CROWD_HAPPY`; panic when
/// the city was hit by a military attack in the last 20 s (0xC00600(0)). Later rules win.
pub fn crowd_state(happiness: f32, hit_recently: bool) -> usize {
    let mut state = 1;
    if happiness < 50.0 {
        state = 2;
    }
    if happiness > CROWD_HAPPY {
        state = 3;
    }
    if hit_recently {
        state = 4;
    }
    state
}

/// Every sound name this module can play (the prefetch list and the install test).
pub fn all_names() -> Vec<&'static str> {
    let mut out = vec![
        "ui_veh_select_land",
        "ui_veh_select_water",
        "ui_veh_select_air",
        DROP_OFF_TRADE,
        TARGET_GEYSER,
        TARGET_HUT,
        TARGET_BUILDING,
        TARGET_VEHICLE,
        TARGET_SELECT,
        NPC_TAKES_CITY,
        TRADE_PROPOSE,
        TRADE_CREATE,
        TRADE_REJECT,
        NAPALM_LAUNCH,
        NUKE_LAUNCH,
        GLOBAL_NUCLEAR_WAR,
        ECON_SUPER,
        BRIBE_START,
        ADBLITZ_START,
        TARGET_ARMED,
        TARGET_WALLA_LP,
        TACNUKE_ARMED_LP,
        FAITHHEAL_ARMED_LP,
        BLACKCLOUD_ARMED_LP,
        BRIBE_ARMED_LP,
        ADBLITZ_ARMED_LP,
        TURRET_LOOP,
    ];
    for table in [
        GTG,
        STANCE_ATTACK,
        STANCE_DEFENSE,
        TAKEFIRE,
        ATTACKED_BY_RELIGIOUS,
        FAITH_HEAL,
        BRIBED,
        EMP,
        INVULNERABILITY,
        NEW_GOODIE,
        CANT_PERFORM,
        OK,
        ATTACK_EPIC,
        CAPTURE_SPICE,
        CAPTURE_GOODIE_HUT,
        TAKEOVER,
        TAKEOVER_CINEMATIC,
    ] {
        out.extend(table);
    }
    for name in RELATIONSHIP {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The per-frame voice set (0xDCC290 / 0xDCED40)
// ---------------------------------------------------------------------------

/// The ids queued this frame: a name already in the set is not added again.
#[derive(Default)]
pub struct Frame(Vec<&'static str>);

impl Frame {
    pub fn add(&mut self, name: &'static str) {
        if !self.0.contains(&name) {
            self.0.push(name);
        }
    }

    pub fn names(&self) -> &[&'static str] {
        &self.0
    }

    fn flush(self, sounds: &mut NamedSounds, now: f64, bus: Bus) {
        for name in self.0 {
            play(sounds, now, name, bus);
        }
    }
}

fn play(sounds: &mut NamedSounds, now: f64, name: &'static str, bus: Bus) {
    let mut p = Play::named(name, "civ_voices");
    p.bus = bus;
    sounds.play(p, now, 0.0);
}

/// A one-shot heard from `at` (Civ world units are retail units: 1.0).
fn play_at(sounds: &mut NamedSounds, hearing: &Hearing, now: f64, name: &'static str, at: Vec3, bus: Bus) {
    let mut p = hearing.play(name, "civ_voices", at, 1.0);
    p.bus = bus;
    sounds.play(p, now, 0.0);
}

// ---------------------------------------------------------------------------
// Markers the player's order code (`civ_rts::rts_input`) puts on entities
// ---------------------------------------------------------------------------

/// A vehicle that just got a new order from the player (right-click, F, X).
#[derive(Component)]
pub struct VoiceOrder;

/// A vehicle whose new order from the player attacks an epic predator (the right-click landed on
/// one; `civ_rts::rts_input` adds it together with `VoiceOrder`).
#[derive(Component)]
pub struct EpicOrder;

/// A vehicle whose order from the player cannot be performed (ADAPTED, see the notes).
#[derive(Component)]
pub struct CantPerform;

/// A goodie hut gave the player a free vehicle of this class (`civ_orders::hut_arrival`): the player
/// hears `New_Goodie` of the class (table 0x147C5DC, `PlayAudio` at 0xDCB202).
#[derive(Component)]
pub struct GoodieCue {
    pub kind: Approach,
    pub at: Vec3,
}

fn goodie_voices(
    time: Res<Time<Real>>,
    mut sounds: ResMut<NamedSounds>,
    mut commands: Commands,
    cues: Query<(Entity, &GoodieCue)>,
) {
    let now = time.elapsed_secs_f64();
    let mut voices = Frame::default();
    for (e, cue) in &cues {
        commands.entity(e).despawn();
        voices.add(NEW_GOODIE[slot(cue.kind)]);
    }
    if sounds.is_running() {
        voices.flush(&mut sounds, now, Bus::Sfx);
    }
}

/// An ability the player just used (`CivCampaign::use_power`).
#[derive(Component)]
pub struct PowerCue {
    pub tool: &'static str,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

#[derive(Default)]
struct UnitMemory {
    /// Per vehicle: the previous `hit` and `cultural_hit` values and the time of its last combat voice.
    hits: HashMap<u32, (f32, f32, f64)>,
    /// Per economic vehicle: heading home (it just unloaded at the foreign city when this rises).
    homeward: HashMap<u32, bool>,
}

/// Where a vehicle's order lands in the world (the target of the target-accepted sound).
fn order_target_at(
    campaign: &CivCampaign,
    positions: &Query<(&CivUnit, &GlobalTransform)>,
    order: UnitOrder,
) -> Option<Vec3> {
    match order {
        UnitOrder::Geyser(g) => Some(campaign.geyser_at(g)),
        UnitOrder::City(c) => campaign.cities.get(c).map(|city| city.center()),
        UnitOrder::Unit(id) => positions.iter().find(|(u, _)| u.id == id).map(|(_, t)| t.translation()),
        UnitOrder::Hut(h) => campaign.hut_at(h),
        UnitOrder::Idle | UnitOrder::Move(_) | UnitOrder::Goto(_) | UnitOrder::Epic(_) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn unit_voices(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    hearing: Hearing,
    mut sounds: ResMut<NamedSounds>,
    mut commands: Commands,
    selected: Query<&CivUnit, Added<Selected>>,
    ordered: Query<(Entity, &CivUnit, Has<EpicOrder>), Added<VoiceOrder>>,
    refused: Query<(Entity, &CivUnit), Added<CantPerform>>,
    stances: Query<(&CivUnit, Ref<CivStance>)>,
    units: Query<(&CivUnit, Option<&GlobalTransform>)>,
    positions: Query<(&CivUnit, &GlobalTransform)>,
    mut memory: Local<UnitMemory>,
) {
    let now = time.elapsed_secs_f64();
    // The markers are consumed whatever the decoder does.
    let mut orders = Frame::default();
    // The target-accepted sounds are positional at their target (soundProp falloff).
    let mut at_target: Vec<(&'static str, Vec3)> = Vec::new();
    for (e, unit, epic) in &ordered {
        commands.entity(e).remove::<VoiceOrder>();
        if epic {
            commands.entity(e).remove::<EpicOrder>();
        }
        if unit.nation == HOME_INDEX {
            if let Some(name) = order_voice(unit.kind, unit.order, epic) {
                orders.add(name);
            }
            // An attack on an epic is not the ground order the click sound belongs to.
            if let Some(name) = target_sound(unit.order).filter(|_| !epic) {
                match order_target_at(&run.civ_campaign, &positions, unit.order) {
                    Some(at) if !at_target.iter().any(|(n, _)| *n == name) => at_target.push((name, at)),
                    Some(_) => {}
                    None => orders.add(name),
                }
            }
        }
    }
    let mut cant = Frame::default();
    for (e, unit) in &refused {
        commands.entity(e).remove::<CantPerform>();
        if unit.nation == HOME_INDEX {
            cant.add(CANT_PERFORM[slot(unit.kind)]);
        }
    }
    let mut hurt = Frame::default();
    let mut seen: HashMap<u32, (f32, f32, f64)> = HashMap::with_capacity(memory.hits.len());
    let mut homeward: HashMap<u32, bool> = HashMap::with_capacity(memory.homeward.len());
    let mut dropped: Option<Option<Vec3>> = None;
    for (unit, at) in &units {
        if unit.nation != HOME_INDEX {
            continue;
        }
        if unit.kind == Approach::Economic {
            homeward.insert(unit.id, unit.homeward);
            if unit.homeward && memory.homeward.get(&unit.id) == Some(&false) && drop_off_plays(rand::random::<u32>()) {
                dropped = Some(at.map(|t| t.translation()));
            }
        }
        let (before, cultural_before, last) =
            memory.hits.get(&unit.id).copied().unwrap_or((unit.hit, unit.cultural_hit, now));
        let mut last = last;
        // `hit` counts the seconds since the last hit: a drop is a new hit. A cultural projectile
        // (a religious vehicle's) also restarts `cultural_hit`: the religious attacker's own voice.
        if unit.hit < before - 0.01 && now - last > COMBAT_VOX_SECONDS {
            hurt.add(hit_voice(unit.kind, unit.cultural_hit < cultural_before - 0.01));
            last = now;
        }
        seen.insert(unit.id, (unit.hit, unit.cultural_hit, last));
    }
    memory.hits = seen;
    memory.homeward = homeward;
    if !sounds.is_running() {
        return;
    }
    // Selection: the domain sound and the class voice of every newly selected vehicle.
    let mut select = Frame::default();
    for unit in selected.iter().filter(|u| u.nation == HOME_INDEX) {
        select.add(select_sound(unit.domain));
        select.add(GTG[slot(unit.kind)]);
    }
    select.flush(&mut sounds, now, Bus::Ui);
    // Stance toggle (Tab): the voices of the vehicles whose stance changed.
    let mut stance = Frame::default();
    for (unit, s) in &stances {
        if unit.nation == HOME_INDEX && s.is_changed() && !s.is_added() {
            stance.add(stance_voice(unit.kind, *s));
        }
    }
    stance.flush(&mut sounds, now, Bus::Sfx);
    orders.flush(&mut sounds, now, Bus::Sfx);
    for (name, at) in at_target {
        play_at(&mut sounds, &hearing, now, name, at, Bus::Sfx);
    }
    cant.flush(&mut sounds, now, Bus::Sfx);
    hurt.flush(&mut sounds, now, Bus::Sfx);
    match dropped {
        // Positional at the foreign city where the vehicle unloads (0xDCCBA6).
        Some(Some(at)) => play_at(&mut sounds, &hearing, now, DROP_OFF_TRADE, at, Bus::Sfx),
        Some(None) => play(&mut sounds, now, DROP_OFF_TRADE, Bus::Sfx),
        None => {}
    }
}

#[derive(Default)]
struct PowerMemory {
    /// Vehicles that were stunned / turned last frame.
    stunned: HashSet<u32>,
    bribed: HashSet<u32>,
}

fn power_voices(
    time: Res<Time<Real>>,
    mut sounds: ResMut<NamedSounds>,
    mut commands: Commands,
    cues: Query<(Entity, &PowerCue)>,
    units: Query<&CivUnit>,
    mut memory: Local<PowerMemory>,
) {
    let now = time.elapsed_secs_f64();
    let mut voices = Frame::default();
    let mut effects = Frame::default();
    for (e, cue) in &cues {
        commands.entity(e).despawn();
        let sounds = power_sounds(cue.tool);
        if let Some(launch) = sounds.launch {
            effects.add(launch);
        }
        // Without arming the tool's one-shots (`armed_sounds`) play at the launch.
        if !arming_enabled() {
            for name in armed_sounds(cue.tool).once {
                effects.add(name);
            }
        }
        if let Some(table) = sounds.own_fleet_voice {
            for unit in units.iter().filter(|u| u.nation == HOME_INDEX) {
                voices.add(table[slot(unit.kind)]);
            }
        }
    }
    // The vehicles that became stunned (EMP) or turned (Bribe Bomb) answer in their class voice.
    let mut stunned = HashSet::new();
    let mut bribed = HashSet::new();
    for unit in &units {
        if unit.stunned > 0.0 {
            stunned.insert(unit.id);
            if !memory.stunned.contains(&unit.id) {
                voices.add(EMP[slot(unit.kind)]);
            }
        }
        if unit.bribed > 0.0 {
            bribed.insert(unit.id);
            if !memory.bribed.contains(&unit.id) {
                voices.add(BRIBED[slot(unit.kind)]);
            }
        }
    }
    memory.stunned = stunned;
    memory.bribed = bribed;
    if !sounds.is_running() {
        return;
    }
    effects.flush(&mut sounds, now, Bus::Sfx);
    voices.flush(&mut sounds, now, Bus::Sfx);
}

/// The owner entity of the armed-tool loops (stage owned: it goes with the stage).
#[derive(Component)]
struct ArmedOwner;

#[derive(Default)]
struct ArmedMemory {
    armed: Option<(PowerSlot, &'static str)>,
}

/// The armed sounds of the tool the player holds (`RtsState::armed`): its one-shots once, its
/// loops until it is fired or dropped.
#[allow(clippy::too_many_arguments)]
fn armed_loops(
    mut commands: Commands,
    time: Res<Time<Real>>,
    rts: Option<Res<RtsState>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut loops: ResMut<LoopSounds>,
    owner: Query<Entity, With<ArmedOwner>>,
    mut memory: Local<ArmedMemory>,
) {
    let Ok(owner) = owner.single() else {
        // The loops need a live owner: it exists from the next frame on.
        commands.spawn((ArmedOwner, StageOwned, Name::new("armed tool loops")));
        return;
    };
    let want = rts
        .as_ref()
        .and_then(|r| r.armed)
        .and_then(|slot| slot_tool(&run.civ_campaign, slot).map(|tool| (slot, tool)));
    if want == memory.armed {
        return;
    }
    loops.stop_owner(owner);
    if let Some((_, tool)) = want {
        let armed = armed_sounds(tool);
        for name in armed.looped {
            loops.start(owner, name, LoopSpec::everywhere());
        }
        if sounds.is_running() {
            let now = time.elapsed_secs_f64();
            for name in armed.once {
                play(&mut sounds, now, name, Bus::Sfx);
            }
        }
    }
    memory.armed = want;
}

/// Wall turrets turn with a loop (0xE8B780): the loop plays at a turret that turned since the last
/// frame while the camera is within `TURRET_HEARING` of it. Kept 0.25 s after the last turn.
fn turret_loops(
    time: Res<Time<Real>>,
    hearing: Hearing,
    mut loops: ResMut<LoopSounds>,
    turrets: Query<(Entity, &CityTurretVisual, &Transform, &GlobalTransform)>,
    mut memory: Local<HashMap<Entity, (Quat, f64)>>,
) {
    let now = time.elapsed_secs_f64();
    let mut seen = HashMap::with_capacity(memory.len());
    for (entity, turret, transform, global) in &turrets {
        let rotation = transform.rotation;
        let (previous, mut last_turn) = memory.get(&entity).copied().unwrap_or((rotation, f64::NEG_INFINITY));
        // VERIFIED 0xE8B780: any change of orientation plays the loop (a target, or the idle look-around).
        let turning = turret.alive;
        if turning && previous.angle_between(rotation) > TURRET_TURN_EPSILON {
            last_turn = now;
        }
        seen.insert(entity, (rotation, last_turn));
        let near = hearing.distance(global.translation(), 1.0).is_some_and(|d| d < TURRET_HEARING);
        if near && now - last_turn < 0.25 {
            loops.start(entity, TURRET_LOOP, LoopSpec::at_owner());
        } else {
            loops.stop(entity, TURRET_LOOP);
        }
    }
    *memory = seen;
}

/// The crowd voice loop of every city the camera is near (0xD5B070 -> `PlayVoice`, the retail crowd
/// actors are not modelled: the loop plays at the city). ADAPTED: hearing range, happy threshold,
/// the voice bank of the player's creature for every city; the transient states 5..7 and the
/// relationship states 11 / 12 are not played.
#[allow(clippy::too_many_arguments)]
fn crowd_loops(
    run: Res<RunProgress>,
    audio: Res<crate::game_audio::GameAudio>,
    originals: Option<Res<crate::Originals>>,
    hearing: Hearing,
    mut loops: ResMut<LoopSounds>,
    cities: Query<(Entity, &crate::civ::CityMarker, &GlobalTransform)>,
    mut memory: Local<HashMap<Entity, Option<(usize, &'static str)>>>,
    mut mouth: Local<Option<&'static str>>,
) {
    let library = audio.library();
    if library.is_none() {
        return;
    }
    let Some(originals) = originals else { return };
    let mouth = *mouth.get_or_insert_with(|| crate::anim_audio::player_mouth(&originals, &run));
    let campaign = &run.civ_campaign;
    for (entity, marker, global) in &cities {
        let Some(city) = campaign.cities.get(marker.0).filter(|c| campaign.present(c.index)) else {
            continue;
        };
        let near = hearing.distance(global.translation(), 1.0).is_some_and(|d| d < CROWD_HEARING);
        let state = near.then(|| {
            crowd_state(city.happiness() as f32, city.calm < crate::civ_rules::ATTACK_MEMORY_SECONDS)
        });
        let want = state.and_then(|s| {
            let name = crate::voice_banks::pick(&library, CROWD_LOOPS[s - 1], mouth, crate::anim_audio::intern)?;
            Some((s, name))
        });
        if memory.get(&entity).copied().flatten() == want {
            memory.entry(entity).or_insert(want);
            continue;
        }
        loops.stop_owner(entity);
        if let Some((_, name)) = want {
            loops.start(entity, name, LoopSpec::at_owner());
        }
        memory.insert(entity, want);
    }
}

#[derive(Default)]
struct EventMemory {
    ready: bool,
    /// The nation of each city, its attitude index and war flag.
    owners: Vec<usize>,
    stance: Vec<usize>,
    war: Vec<bool>,
    trade: Vec<bool>,
    refusal: f32,
    /// Cities an economic vehicle of yours is proposing a route to.
    proposing: HashSet<usize>,
}

fn event_sounds(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    units: Query<&CivUnit>,
    mut memory: Local<EventMemory>,
) {
    let now = time.elapsed_secs_f64();
    let campaign = &run.civ_campaign;
    let memory = &mut *memory;
    let n = campaign.cities.len().min(CITY_COUNT);
    let owners: Vec<usize> = campaign.cities.iter().take(n).map(|c| c.nation()).collect();
    let stance: Vec<usize> = (0..n)
        .map(|c| crate::civ_comm::stance(campaign.nations.get(c).map_or(0.0, |s| s.relation.value())))
        .collect();
    let war: Vec<bool> = (0..n).map(|c| campaign.nations.get(c).is_some_and(|s| s.war)).collect();
    let trade: Vec<bool> = campaign.cities.iter().take(n).map(|c| c.trade).collect();
    let refusal = campaign.comm.refusal_wait;
    let mut proposing = HashSet::new();
    for unit in units.iter().filter(|u| u.nation == HOME_INDEX && u.kind == Approach::Economic) {
        if let UnitAction::Trading(c) = unit.action {
            if !trade.get(c).copied().unwrap_or(true) {
                proposing.insert(c);
            }
        }
    }
    let mut frame = Frame::default();
    if memory.ready && memory.owners.len() == n {
        for c in 0..n {
            // A city changes hands to a civilization that is not the player's (0xBE8E30).
            if owners[c] != memory.owners[c] && owners[c] != HOME_INDEX {
                frame.add(NPC_TAKES_CITY);
            }
            // The city of a rival is taken by the player: the takeover sound (0xD5B3A0),
            // the specialty of the city, the capture cinematic's variant.
            if owners[c] == HOME_INDEX && memory.owners[c] != HOME_INDEX && c != HOME_INDEX {
                let specialty = campaign.cities.get(c).map_or(0, |city| slot(city.kind));
                frame.add(takeover_sound(specialty, true));
            }
            // The relationship event-log templates: the level entered, or a war declared.
            if c != HOME_INDEX && campaign.nation_alive(c) {
                if stance[c] != memory.stance[c] {
                    frame.add(relationship_sound(stance[c]));
                } else if war[c] && !memory.war[c] {
                    frame.add(relationship_sound(0));
                }
            }
            // A trade route opens.
            if trade[c] && !memory.trade[c] {
                frame.add(TRADE_CREATE);
            }
        }
        // A rival refuses the route (the refusal timer restarts).
        if refusal > memory.refusal + 1.0 {
            frame.add(TRADE_REJECT);
            frame.add(CANT_PERFORM[2]);
        }
        // An economic vehicle of yours starts a proposal.
        if proposing.iter().any(|c| !memory.proposing.contains(c)) {
            frame.add(TRADE_PROPOSE);
        }
    }
    memory.ready = true;
    memory.owners = owners;
    memory.stance = stance;
    memory.war = war;
    memory.trade = trade;
    memory.refusal = refusal;
    memory.proposing = proposing;
    if sounds.is_running() {
        frame.flush(&mut sounds, now, Bus::Ui);
    }
}

/// Decode the module's sounds once the decoder runs.
fn prefetch(mut sounds: ResMut<NamedSounds>, mut done: Local<bool>) {
    if *done || !sounds.is_running() {
        return;
    }
    *done = true;
    // The loops are decoded by `LoopSounds` (one variant) when they start.
    for name in all_names().into_iter().filter(|n| !n.ends_with("_lp")) {
        sounds.request(crate::world_objects::name_id(name), Some(name), false);
    }
}

pub struct CivVoicesPlugin;

impl Plugin for CivVoicesPlugin {
    fn build(&self, app: &mut App) {
        // `NamedSounds` is decoded and played by the stage's own audio chain: the observers
        // only queue sounds.
        app.add_systems(
            Update,
            (
                prefetch,
                (unit_voices, power_voices, goodie_voices, event_sounds, armed_loops, turret_loops, crowd_loops)
                    .run_if(in_state(GameStage::Civilization)),
            )
                .run_if(resource_exists::<RunProgress>)
                .run_if(resource_exists::<NamedSounds>)
                .run_if(resource_exists::<LoopSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_objects::name_id;

    #[test]
    fn tables_follow_the_retail_rows() {
        assert_eq!(GTG[0], "Mili_Gtg");
        assert_eq!(OK[2], "Econ_Ok");
        assert_eq!(slot(Approach::Religious), 1);
        // The Attack_Epic row is stored Military, Economic, Religious (0x147C670).
        assert_eq!(ATTACK_EPIC[1], "Econ_Attack_Epic");
        assert_eq!(order_voice(Approach::Religious, UnitOrder::Unit(3), true), Some("Econ_Attack_Epic"));
        assert_eq!(order_voice(Approach::Military, UnitOrder::Unit(3), false), Some("Mili_Ok"));
        assert_eq!(order_voice(Approach::Economic, UnitOrder::Geyser(1), false), Some("Econ_Capture_Spice"));
        assert_eq!(order_voice(Approach::Military, UnitOrder::Goto([0.0; 3]), false), Some("Mili_Ok"));
        assert_eq!(order_voice(Approach::Military, UnitOrder::Idle, false), None);
        assert_eq!(target_sound(UnitOrder::Geyser(0)), Some("sfx_civ_target_geyser"));
        assert_eq!(target_sound(UnitOrder::City(2)), Some("sfx_civ_target_building"));
        assert_eq!(target_sound(UnitOrder::Unit(2)), Some("sfx_civ_target_vehicle"));
        // A ground order plays the plain select sound of 0xCF29E6; nothing for no order.
        assert_eq!(target_sound(UnitOrder::Goto([0.0; 3])), Some("ui_trg_target_select"));
        assert_eq!(target_sound(UnitOrder::Idle), None);
        // An attack on an epic overrides the order's own voice (0xDCC5A0), for every vehicle class.
        assert_eq!(order_voice(Approach::Military, UnitOrder::Goto([0.0; 3]), true), Some("Mili_Attack_Epic"));
        assert_eq!(order_voice(Approach::Military, UnitOrder::Idle, true), None);
        assert_eq!(goodie_hut_voice(Approach::Religious), "Reli_Capture_GoodieHut");
        assert!(drop_off_plays(0) && drop_off_plays(3) && !drop_off_plays(1) && !drop_off_plays(2));
        assert_eq!(hit_voice(Approach::Military, false), "Mili_Takefire");
        assert_eq!(hit_voice(Approach::Economic, true), "Econ_AttackedBy_Religious");
        assert_eq!(stance_voice(Approach::Religious, CivStance::Aggressive), "Reli_Stance_Attack");
        assert_eq!(stance_voice(Approach::Religious, CivStance::StandGround), "Reli_Stance_Defense");
        assert_eq!(select_sound(VehicleDomain::Sea), "ui_veh_select_water");
    }

    #[test]
    fn abilities_play_their_voices_and_launches() {
        assert_eq!(power_sounds("i_field").own_fleet_voice, Some(INVULNERABILITY));
        assert_eq!(power_sounds("faithheal").own_fleet_voice, Some(FAITH_HEAL));
        assert_eq!(power_sounds("napalm").launch, Some("sfx_civ_napalm_launch"));
        assert_eq!(power_sounds("icbm").launch, Some("sfx_civ_global_nuclear_war"));
        assert_eq!(power_sounds("blackcloud").launch, None);
        assert_eq!(takeover_sound(1, true), "sfx_cin_city_takeover_relig");
        assert_eq!(takeover_sound(2, false), "city_takeover_econ");
        assert_eq!(relationship_sound(0), "civ_ui_at_war");
        assert_eq!(relationship_sound(4), "civ_ui_form_alliance");
    }

    #[test]
    fn a_frame_plays_each_voice_once() {
        let mut f = Frame::default();
        f.add("Mili_Ok");
        f.add("Mili_Ok");
        f.add("Reli_Ok");
        assert_eq!(f.names(), &["Mili_Ok", "Reli_Ok"]);
        let names = all_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no duplicates");
    }

    /// The observers queue the retail voices (headless; a decoder-less `NamedSounds` only
    /// queues).
    #[test]
    fn player_orders_and_selection_queue_their_voices() {
        use crate::civ_rts::UnitAction;
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, unit_voices.run_if(in_state(GameStage::Civilization)));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..4 {
            app.update();
        }
        assert!(queued(&app).is_empty(), "{:?}", queued(&app));
        let mut unit = CivUnit::from_state(&crate::civ::UnitState {
            id: 7,
            nation: HOME_INDEX,
            kind: Approach::Economic,
            ..Default::default()
        });
        unit.action = UnitAction::Idle;
        let e = app.world_mut().spawn((unit, CivStance::default())).id();
        app.update();
        assert!(queued(&app).is_empty(), "a new vehicle is silent: {:?}", queued(&app));
        // Selected: the domain sound and the class voice.
        app.world_mut().entity_mut(e).insert(Selected);
        app.update();
        let q = queued(&app);
        assert_eq!(q, vec![name_id("ui_veh_select_land"), name_id("Econ_Gtg")], "{q:?}");
        // A new order from the player: the generic order voice; a geyser order: the capture one.
        app.world_mut().entity_mut(e).get_mut::<CivUnit>().unwrap().order = UnitOrder::Geyser(2);
        app.world_mut().entity_mut(e).insert(VoiceOrder);
        app.update();
        let q = queued(&app);
        assert_eq!(&q[q.len() - 2..], &[name_id("Econ_Capture_Spice"), name_id("sfx_civ_target_geyser")], "{q:?}");
        // The class cannot attack: Cant_Perform.
        app.world_mut().entity_mut(e).insert(CantPerform);
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("Econ_Cant_Perform")));
        // Stance toggle.
        *app.world_mut().entity_mut(e).get_mut::<CivStance>().unwrap() = CivStance::Aggressive;
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("Econ_Stance_Attack")));
    }

    /// Round 3: a go-to order says the class voice and the ground select sound; an order on an epic
    /// says only the epic voice (the order is `EpicOrder`-marked by the right-click).
    #[test]
    fn ground_and_epic_orders_queue_their_sounds() {
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, unit_voices.run_if(in_state(GameStage::Civilization)));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..4 {
            app.update();
        }
        let unit = CivUnit::from_state(&crate::civ::UnitState {
            id: 9,
            nation: HOME_INDEX,
            kind: Approach::Military,
            ..Default::default()
        });
        let e = app.world_mut().spawn((unit, CivStance::default())).id();
        app.update();
        assert!(queued(&app).is_empty(), "{:?}", queued(&app));
        // A drive-to-point order.
        app.world_mut().entity_mut(e).get_mut::<CivUnit>().unwrap().order = UnitOrder::Goto([1.0, 0.0, 2.0]);
        app.world_mut().entity_mut(e).insert(VoiceOrder);
        app.update();
        assert_eq!(queued(&app), vec![name_id("Mili_Ok"), name_id("ui_trg_target_select")]);
        // The same click on an epic predator: the epic voice, no ground sound, the marker is consumed.
        app.world_mut().entity_mut(e).insert((VoiceOrder, EpicOrder));
        app.update();
        let q = queued(&app);
        assert_eq!(&q[2..], &[name_id("Mili_Attack_Epic")], "{q:?}");
        assert!(app.world().entity(e).get::<EpicOrder>().is_none() && app.world().entity(e).get::<VoiceOrder>().is_none());
    }

    /// Round 3: the hit voice is the religious attacker's when the hit also restarted `cultural_hit`.
    #[test]
    fn a_cultural_hit_calls_out_the_religious_attacker_line() {
        use std::time::Duration;
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, unit_voices.run_if(in_state(GameStage::Civilization)));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..4 {
            app.update();
        }
        let mut unit = CivUnit::from_state(&crate::civ::UnitState {
            id: 11,
            nation: HOME_INDEX,
            kind: Approach::Military,
            ..Default::default()
        });
        unit.hit = 5.0;
        unit.cultural_hit = 10.0;
        let e = app.world_mut().spawn((unit, CivStance::default())).id();
        app.update();
        // More than a minute into the vehicle's life a plain hit says `Takefire`.
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs(61));
        app.world_mut().entity_mut(e).get_mut::<CivUnit>().unwrap().hit = 0.0;
        app.update();
        assert_eq!(queued(&app), vec![name_id("Mili_Takefire")]);
        // A minute later a cultural projectile lands: the religious attacker's line.
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs(61));
        app.world_mut().entity_mut(e).get_mut::<CivUnit>().unwrap().hit = 3.0;
        app.update();
        {
            let mut entity = app.world_mut().entity_mut(e);
            let mut unit = entity.get_mut::<CivUnit>().unwrap();
            unit.hit = 0.0;
            unit.cultural_hit = 0.0;
        }
        app.update();
        let q = queued(&app);
        assert_eq!(q.last(), Some(&name_id("Mili_AttackedBy_Religious")), "{q:?}");
    }

    #[test]
    fn armed_tools_play_their_table_rows() {
        let a = armed_sounds("faithheal");
        assert_eq!(a.looped, &["sfx_faithheal_armed_lp"]);
        assert_eq!(a.once, &["sfx_napalm_target_armed"]);
        assert!(armed_sounds("blackcloud").once.is_empty());
        assert_eq!(armed_sounds("blackcloud").looped, &["sfx_blackcloud_armed_lp"]);
        assert_eq!(armed_sounds("espionage").looped, &["sfx_civ_bribe_armed_lp"]);
        assert!(armed_sounds("espionage").once.contains(&"sfx_civ_bribe_start"));
        assert_eq!(armed_sounds("adblitz").once, &["sfx_civ_adblitz_start"]);
        assert_eq!(armed_sounds("tacnuke").looped, &["sfx_tact_icbm_target_armed_lp"]);
        assert_eq!(armed_sounds("napalm").looped, &["sfx_civ_target_walla_lp"]);
        // Rows 9, 10 and 11 (emp, i_field, diplodervish) and the empty superweapon rows.
        assert_eq!(armed_sounds("i_field").once, &["sfx_napalm_target_armed"]);
        assert!(armed_sounds("i_field").looped.is_empty());
        assert_eq!(armed_sounds("emp").once, &["sfx_napalm_target_armed"]);
        assert_eq!(armed_sounds("diplodervish"), armed_sounds("espionage"));
        for tool in ["icbm", "rising", "globaladblitz"] {
            let a = armed_sounds(tool);
            assert!(a.once.is_empty() && a.looped.is_empty(), "{tool}");
        }
        // The start sounds moved from the launch to the arming.
        assert_eq!(power_sounds("espionage").launch, None);
        assert_eq!(power_sounds("adblitz").launch, None);
        assert_eq!(power_sounds("napalm").launch, Some("sfx_civ_napalm_launch"));
        // Every looped name is a `_lp` sound and prefetch leaves the loops to `LoopSounds`.
        for tool in ["napalm", "tacnuke", "faithheal", "blackcloud", "espionage", "adblitz"] {
            assert!(armed_sounds(tool).looped.iter().all(|n| n.ends_with("_lp")), "{tool}");
        }
    }

    /// An armed tool starts its loop and its one-shot, and drops them when it is fired or dropped.
    #[test]
    fn an_armed_tool_loops_until_it_is_fired_or_dropped() {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        run.civ_campaign.cell_power = Some(crate::civ::CivCellPower::HealingAura);
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>().init_resource::<LoopSounds>().init_resource::<RtsState>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        let owner = app.world_mut().spawn(ArmedOwner).id();
        app.add_systems(Update, armed_loops);
        app.update();
        let names = |app: &App| app.world().resource::<LoopSounds>().names(owner);
        assert!(names(&app).is_empty(), "nothing armed, nothing loops");
        app.world_mut().resource_mut::<RtsState>().armed = Some(PowerSlot::Cell);
        app.update();
        assert_eq!(names(&app), vec!["sfx_faithheal_armed_lp"]);
        assert_eq!(app.world().resource::<NamedSounds>().queued(), vec![crate::world_objects::name_id("sfx_napalm_target_armed")]);
        // Still armed: no second one-shot.
        app.update();
        assert_eq!(app.world().resource::<NamedSounds>().queued().len(), 1);
        // Fired (the key pressed again clears `armed`): the loop ends.
        app.world_mut().resource_mut::<RtsState>().armed = None;
        app.update();
        assert!(names(&app).is_empty());
    }

    /// A turret that turns near the camera loops; a resting or distant one does not.
    #[test]
    fn turning_turrets_near_the_camera_loop() {
        use crate::city_presentation::CityTurretVisual;
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<LoopSounds>().init_resource::<RtsState>();
        app.add_systems(Update, turret_loops);
        app.world_mut().spawn((crate::GameCamera, GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0))));
        let turret = app
            .world_mut()
            .spawn((
                CityTurretVisual { city: 0, turret: 1, alive: true, kind: Approach::Military },
                Transform::from_xyz(30.0, 0.0, 0.0),
                GlobalTransform::from(Transform::from_xyz(30.0, 0.0, 0.0)),
            ))
            .id();
        let looping = |app: &App| app.world().resource::<LoopSounds>().is_active(turret, TURRET_LOOP);
        app.update();
        assert!(!looping(&app), "a turret that has not turned is silent");
        // It turns with no target (the idle look-around): the loop plays (0xE8B780 tests the orientation only).
        app.world_mut().entity_mut(turret).insert(Transform::from_xyz(30.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.2)));
        app.update();
        assert!(looping(&app));
        // With a target it turns: loop.
        app.world_mut().resource_mut::<RtsState>().turret_aim.insert((0, 1), Vec3::new(100.0, 0.0, 0.0));
        app.world_mut().entity_mut(turret).insert(Transform::from_xyz(30.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.4)));
        app.update();
        assert!(looping(&app));
        // Out of the 200 unit hearing range: the loop ends.
        app.world_mut().entity_mut(turret).insert((
            Transform::from_xyz(900.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.6)),
            GlobalTransform::from(Transform::from_xyz(900.0, 0.0, 0.0)),
        ));
        app.update();
        assert!(!looping(&app));
    }

    /// A hit plays the class voice once every 60 s of the vehicle's life.
    #[test]
    fn a_hit_vehicle_calls_out_once_a_minute() {
        // Pure model of the observer's timer.
        let mut last = 0.0_f64;
        let mut calls = 0;
        for t in [10.0_f64, 59.0, 61.0, 90.0, 122.0] {
            if t - last > COMBAT_VOX_SECONDS {
                calls += 1;
                last = t;
            }
        }
        assert_eq!(calls, 2);
    }

    #[test]
    fn the_crowd_state_follows_the_retail_rules() {
        assert_eq!(CROWD_LOOPS[0], "vcode_crowd_Neutral_loop");
        assert_eq!(CROWD_LOOPS[3], "vcode_crowd_Panic_loop");
        assert_eq!(crowd_state(55.0, false), 1);
        assert_eq!(crowd_state(49.0, false), 2);
        assert_eq!(crowd_state(80.0, false), 3);
        assert_eq!(crowd_state(75.0, false), 1, "RolloverHappyCap is exclusive");
        assert_eq!(crowd_state(76.0, false), 3);
        // An attack wins over the mood (0xC00600 is read last).
        assert_eq!(crowd_state(80.0, true), 4);
        assert_eq!(crowd_state(10.0, true), 4);
    }

    /// Which crowd loop banks are installed (the names carry the voice class of the creature).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_crowd_loops() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let library = Some(std::sync::Arc::new(library));
        let found: Vec<_> = CROWD_LOOPS
            .iter()
            .map(|event| (*event, crate::voice_banks::pick(&library, event, "", crate::anim_audio::intern)))
            .collect();
        for (event, name) in &found {
            println!("{event} -> {name:?}");
        }
        // The four base states must have a bank for the loop to play at all.
        for (event, name) in found.iter().take(4) {
            assert!(name.is_some(), "{event} has no installed bank");
        }
    }

    /// Every name the module plays is installed.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_names_resolve() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let missing: Vec<_> = all_names().into_iter().filter(|n| library.resolve(n).is_err()).collect();
        assert!(missing.is_empty(), "not installed: {missing:?}");
    }

    #[test]
    fn hut_and_epic_orders_have_their_retail_voices() {
        // 0xDCC5A0: a capture order (type 8) on a node that is not the geyser noun says Capture_GoodieHut,
        // an attack (type 1) on an epic Attack_Epic (stored Military, Economic, Religious).
        assert_eq!(order_voice(Approach::Military, UnitOrder::Hut(2), false), Some("Mili_Capture_GoodieHut"));
        assert_eq!(order_voice(Approach::Economic, UnitOrder::Hut(0), false), Some("Econ_Capture_GoodieHut"));
        assert_eq!(order_voice(Approach::Military, UnitOrder::Epic(3), false), Some("Mili_Attack_Epic"));
        // The hut click plays sfx_civ_target_hut (0xCF40BF); the attack on an epic plays no target sound.
        assert_eq!(target_sound(UnitOrder::Hut(1)), Some("sfx_civ_target_hut"));
        assert_eq!(target_sound(UnitOrder::Epic(1)), None);
        assert_eq!(NEW_GOODIE[slot(Approach::Religious)], "Reli_New_Goodie");
    }
}
