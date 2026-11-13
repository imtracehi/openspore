//! Space posse voices and event sounds (helper `sound-voices`, `notes\audio-audit.md` gap G5).
//!
//! The retail posse voices are one id per ARCHETYPE of the posse member's empire: a switch on
//! `cEmpire::mArchetype` (+0x58, `Archetypes` 0 .. 8 = warrior, trader, scientist, shaman, bard,
//! zealot, diplomat, ecologist, grob) with a jump table; any other archetype (the player's own
//! kinds, 9 .. 18) gets the `squi_` voice when the empire is the player's and no sound
//! otherwise. The six events are separate routines (march2017 addresses):
//! * `*_posse_underattack` (0xC3D36D .. in `cGameDataUFO::TakeDamage` 0xC3CEC0: a posse ship's
//!   first damage at full health, table 0xC3DBB0, the player's own `squi_` voice by default),
//! * `*_posse_joins` 0x1002990 (the ally joins the posse),
//! * `*_posse_dismissed` (0x10016D6 ..), `*_posse_healed` (0x10581EC ..), `*_posse_rally`
//!   (0x105C0A1 ..), `*_posse_attacks` 0xDBFE10 (the posse ship gets an attack target; the voice
//!   is its empire's), `*_posse_dead` 0xC3D6F8 .. (the ship-destroyed routine of a posse ship,
//!   positional).
//! * `ui_spg_reinforcements_call` 0xC00FB9, `_arrive` 0xFF8C3B; `ui_space_tscore_up|down`
//!   0xC83607 / 0xC8362F (a planet's T-score changes, down when it falls, else up);
//!   the system-captured sound 0xC8D406 (the player owns every planet of a system, in the
//!   `cEmpire::CaptureSystem` check 0xC8D190); `sfx_ufo_abduct_success` 0x103375C and
//!   `ufo_abduct_collect_hit` 0x1032688 (the abduct beam); `sfx_genesis_energy_blast` (the Staff
//!   of Life's tool table, 0x15E4F31 ..).
//!
//! ADAPTED (the sound is retail, the OpenSpore trigger is mine; the retail posse is a set of
//! live ships, OpenSpore keeps `SpaceCampaign::fleet`):
//! * joins: an escort enters the fleet; dismissed: one leaves with health left (an alliance
//!   broken; a destroyed escort leaves the same way, retail's `RemovePosse` 0x10015A0 is called
//!   with its voice flag 0 at the death too); healed: an escort's health rises (docking repairs
//!   them; at most one voice per archetype every 20 s); attacks: an escort fires after being
//!   quiet (one voice per archetype every 8 s). The player's own Mini-U wingman has no empire:
//!   the `squi_` voice.
//! * `rally`, `underattack` and `dead` are the rules' own events (`space_tool_use::PosseEvent`,
//!   helper `space-rally`): the Rally Call tool (0x105BF80, one voice per posse ship), the first
//!   damage a hostile ship does to a full-health escort (0xC3D36D), and an escort destroyed
//!   (0xC3D6F8, `kill_escort`). The voice choice and the triggers are the retail ones.
//! * A Knight / unmatched NPC archetype (OpenSpore has Knight NPC empires, retail does not)
//!   uses the player's `squi_` voice.
//! * reinforcements: the `callforbackup` summon (a Mini-U joins) plays `_call`; `_arrive` when
//!   its escort ship appears.
//! * T-score: any planet's ecology T-score changes; abduct: the wild-specimen cargo rises
//!   (the collect hit, then the success sound 0.3 s later); genesis: a Staff of Life charge is used.
//! * The system-captured sound, the hologram scout loop and the UFO fireworks launch: OpenSpore
//!   has no matching event (the loop also needs an owner).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    game::RunProgress,
    game_audio::Bus,
    space::{empire_archetype, EscortWingman, SpaceArchetype},
    space_worldgen::EmpireId,
    stages::GameStage,
};

// ---------------------------------------------------------------------------
// Original data: the voices per archetype slot
// (warrior, trader, scientist, shaman, bard, zealot, diplomat, ecologist, grob, player)
// ---------------------------------------------------------------------------

pub const JOINS: [&str; 10] = [
    "warr_posse_joins",
    "trad_posse_joins",
    "scie_posse_joins",
    "sham_posse_joins",
    "bard_posse_joins",
    "zeal_posse_joins",
    "dipl_posse_joins",
    "ecol_posse_joins",
    "grob_posse_joins",
    "squi_posse_joins",
];
pub const DISMISSED: [&str; 10] = [
    "warr_posse_dismissed",
    "trad_posse_dismissed",
    "scie_posse_dismissed",
    "sham_posse_dismissed",
    "bard_posse_dismissed",
    "zeal_posse_dismissed",
    "dipl_posse_dismissed",
    "ecol_posse_dismissed",
    "grob_posse_dismissed",
    "squi_posse_dismissed",
];
pub const HEALED: [&str; 10] = [
    "warr_posse_healed",
    "trad_posse_healed",
    "scie_posse_healed",
    "sham_posse_healed",
    "bard_posse_healed",
    "zeal_posse_healed",
    "dipl_posse_healed",
    "ecol_posse_healed",
    "grob_posse_healed",
    "squi_posse_healed",
];
pub const RALLY: [&str; 10] = [
    "warr_posse_rally",
    "trad_posse_rally",
    "scie_posse_rally",
    "sham_posse_rally",
    "bard_posse_rally",
    "zeal_posse_rally",
    "dipl_posse_rally",
    "ecol_posse_rally",
    "grob_posse_rally",
    "squi_posse_rally",
];
pub const ATTACKS: [&str; 10] = [
    "warr_posse_attacks",
    "trad_posse_attacks",
    "scie_posse_attacks",
    "sham_posse_attacks",
    "bard_posse_attacks",
    "zeal_posse_attacks",
    "dipl_posse_attacks",
    "ecol_posse_attacks",
    "grob_posse_attacks",
    "squi_posse_attacks",
];
/// The ten ids of the retail jump table 0xC3DBB0 (`*_posse_underattack`, all matching the
/// registry's names); the install holds the sound of the diplomat only, so the game plays
/// just that one ([`UNDERATTACK_INSTALLED`]) and the others are silent, as retail's
/// `PlayAudio` of an id without a sound is.
pub const UNDERATTACK: [&str; 10] = [
    "warr_posse_underattack",
    "trad_posse_underattack",
    "scie_posse_underattack",
    "sham_posse_underattack",
    "bard_posse_underattack",
    "zeal_posse_underattack",
    "dipl_posse_underattack",
    "ecol_posse_underattack",
    "grob_posse_underattack",
    "squi_posse_underattack",
];
pub const UNDERATTACK_INSTALLED: [&str; 1] = ["dipl_posse_underattack"];
pub const DEAD: [&str; 10] = [
    "warr_posse_dead",
    "trad_posse_dead",
    "scie_posse_dead",
    "sham_posse_dead",
    "bard_posse_dead",
    "zeal_posse_dead",
    "dipl_posse_dead",
    "ecol_posse_dead",
    "grob_posse_dead",
    "squi_posse_dead",
];

pub const REINFORCEMENTS_CALL: &str = "ui_spg_reinforcements_call";
pub const REINFORCEMENTS_ARRIVE: &str = "ui_spg_reinforcements_arrive";
pub const TSCORE_UP: &str = "ui_space_tscore_up";
pub const TSCORE_DOWN: &str = "ui_space_tscore_down";
pub const ABDUCT_HIT: &str = "ufo_abduct_collect_hit";
pub const ABDUCT_SUCCESS: &str = "sfx_ufo_abduct_success";
pub const GENESIS_BLAST: &str = "sfx_genesis_energy_blast";

/// The voice slot of a posse member's empire (`None` = the player's own ship).
pub fn voice_slot(empire: Option<EmpireId>) -> usize {
    match empire.map(empire_archetype) {
        None => 9,
        Some(SpaceArchetype::Warrior) => 0,
        Some(SpaceArchetype::Trader) => 1,
        Some(SpaceArchetype::Scientist) => 2,
        Some(SpaceArchetype::Shaman) => 3,
        Some(SpaceArchetype::Bard) => 4,
        Some(SpaceArchetype::Zealot) => 5,
        Some(SpaceArchetype::Diplomat) => 6,
        Some(SpaceArchetype::Ecologist) => 7,
        // The Grox are the retail `grob` archetype.
        Some(SpaceArchetype::Wanderer) => 8,
        // ADAPTED: retail NPC empires are never Knights; the player's voice.
        Some(SpaceArchetype::Knight) => 9,
    }
}

/// Seconds between two healed / attack voices of one archetype (ADAPTED).
pub const HEALED_EVERY: f64 = 20.0;
pub const ATTACKS_EVERY: f64 = 8.0;

/// Every sound name this module can play (the prefetch list and the install test).
pub fn all_names() -> Vec<&'static str> {
    let mut out = vec![
        REINFORCEMENTS_CALL,
        REINFORCEMENTS_ARRIVE,
        TSCORE_UP,
        TSCORE_DOWN,
        ABDUCT_HIT,
        ABDUCT_SUCCESS,
        GENESIS_BLAST,
    ];
    for table in [JOINS, DISMISSED, HEALED, RALLY, ATTACKS, DEAD] {
        out.extend(table);
    }
    out.extend(UNDERATTACK_INSTALLED);
    out
}

/// What changed between two looks at the posse: the voices to play.
#[derive(Debug, Default, PartialEq)]
pub struct PosseEvents {
    pub joined: Vec<Option<EmpireId>>,
    pub dismissed: Vec<Option<EmpireId>>,
    pub dead: Vec<Option<EmpireId>>,
    /// Escorts (slot voices) whose health rose.
    pub healed: Vec<Option<EmpireId>>,
}

/// One look at the posse: (empire, health, max health) per escort.
pub type Look = Vec<(Option<EmpireId>, f32, f32)>;

/// Pure: what happened between `before` and `now`.
pub fn compare(before: &Look, now: &Look) -> PosseEvents {
    let mut ev = PosseEvents::default();
    for entry in now.iter().skip(before.len()) {
        ev.joined.push(entry.0);
    }
    if now.len() < before.len() {
        // The escorts that left: by empire, the surplus of the earlier look.
        let mut left: Vec<(Option<EmpireId>, f32)> = Vec::new();
        let mut counts: HashMap<Option<EmpireId>, usize> = HashMap::new();
        for e in now {
            *counts.entry(e.0).or_default() += 1;
        }
        for e in before {
            match counts.get_mut(&e.0) {
                Some(n) if *n > 0 => *n -= 1,
                _ => left.push((e.0, e.1)),
            }
        }
        for (empire, hp) in left {
            if hp <= 0.0 {
                ev.dead.push(empire);
            } else {
                ev.dismissed.push(empire);
            }
        }
    }
    for (b, n) in before.iter().zip(now.iter()) {
        if b.0 == n.0 && n.1 > b.1 + 0.01 {
            ev.healed.push(n.0);
        }
    }
    ev
}

fn play(sounds: &mut NamedSounds, now: f64, name: &'static str, delay: f32) {
    let mut p = Play::named(name, "space_voices");
    p.bus = Bus::Sfx;
    sounds.play(p, now, delay);
}

#[derive(Default)]
struct SpaceMemory {
    ready: bool,
    posse: Look,
    healed: HashMap<usize, f64>,
    attacks: HashMap<usize, f64>,
    /// Per escort slot: the previous fire cooldown.
    cooldown: HashMap<usize, f32>,
    tscore: HashMap<u32, u8>,
    staff: u8,
    cargo: u8,
}

#[allow(clippy::too_many_arguments)]
fn space_voices(
    time: Res<Time<Real>>,
    mut run: ResMut<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    wingmen: Query<&EscortWingman>,
    arrived: Query<&EscortWingman, Added<EscortWingman>>,
    mut memory: Local<SpaceMemory>,
) {
    let now = time.elapsed_secs_f64();
    let memory = &mut *memory;
    // The rules' posse events (rally, under attack, dead), taken without marking the run changed.
    let posse_events = if run.space_campaign.posse_events.is_empty() {
        Vec::new()
    } else {
        std::mem::take(&mut run.bypass_change_detection().space_campaign.posse_events)
    };
    let campaign = &run.space_campaign;
    let look: Look = campaign.fleet.iter().map(|s| (s.empire, s.hp, s.max_hp)).collect();
    let staff = campaign.staff_of_life_charges;
    let cargo = campaign.cargo_wildlife.total();
    let mut out: Vec<(&'static str, f32)> = Vec::new();
    if memory.ready {
        // The rules' events first: a destroyed ship's dead voice (0xC3D6F8) comes before the
        // dismissed voice of its `RemovePosse` (0x10015A0).
        for event in &posse_events {
            use crate::space_tool_use::PosseEvent;
            match event {
                PosseEvent::Rally(e) => out.push((RALLY[voice_slot(*e)], 0.0)),
                PosseEvent::UnderAttack(e) => {
                    let name = UNDERATTACK[voice_slot(*e)];
                    if UNDERATTACK_INSTALLED.contains(&name) {
                        out.push((name, 0.0));
                    }
                }
                PosseEvent::Dead(e) => out.push((DEAD[voice_slot(*e)], 0.0)),
            }
        }
        let ev = compare(&memory.posse, &look);
        for e in &ev.joined {
            out.push((JOINS[voice_slot(*e)], 0.0));
            if e.is_none() {
                out.push((REINFORCEMENTS_CALL, 0.0));
            }
        }
        for e in &ev.dismissed {
            out.push((DISMISSED[voice_slot(*e)], 0.0));
        }
        for e in &ev.dead {
            out.push((DEAD[voice_slot(*e)], 0.0));
        }
        for e in &ev.healed {
            let slot = voice_slot(*e);
            if memory.healed.get(&slot).is_none_or(|at| now - at >= HEALED_EVERY) {
                memory.healed.insert(slot, now);
                out.push((HEALED[slot], 0.0));
            }
        }
        // An escort that fires after being quiet starts an attack.
        for w in &wingmen {
            let before = memory.cooldown.insert(w.slot, w.fire_cooldown).unwrap_or(w.fire_cooldown);
            if w.fire_cooldown > before + 0.3 {
                if let Some(ship) = campaign.fleet.get(w.slot) {
                    let slot = voice_slot(ship.empire);
                    if memory.attacks.get(&slot).is_none_or(|at| now - at >= ATTACKS_EVERY) {
                        memory.attacks.insert(slot, now);
                        out.push((ATTACKS[slot], 0.0));
                    }
                }
            }
        }
        // The summoned wingman appears.
        for w in &arrived {
            if campaign.fleet.get(w.slot).is_some_and(|s| s.empire.is_none()) {
                out.push((REINFORCEMENTS_ARRIVE, 0.0));
            }
        }
        // A planet's T-score moves.
        for eco in &campaign.ecology {
            let t = eco.t_score();
            match memory.tscore.insert(eco.world_id, t) {
                Some(before) if t < before => out.push((TSCORE_DOWN, 0.0)),
                Some(before) if t > before => out.push((TSCORE_UP, 0.0)),
                _ => {}
            }
        }
        if staff < memory.staff {
            out.push((GENESIS_BLAST, 0.0));
        }
        if cargo > memory.cargo {
            out.push((ABDUCT_HIT, 0.0));
            out.push((ABDUCT_SUCCESS, 0.3));
        }
    } else {
        for eco in &campaign.ecology {
            memory.tscore.insert(eco.world_id, eco.t_score());
        }
    }
    memory.ready = true;
    memory.posse = look;
    memory.staff = staff;
    memory.cargo = cargo;
    if sounds.is_running() {
        for (name, delay) in out {
            play(&mut sounds, now, name, delay);
        }
    }
}

/// Decode the module's sounds once the decoder runs.
fn prefetch(mut sounds: ResMut<NamedSounds>, mut done: Local<bool>) {
    if *done || !sounds.is_running() {
        return;
    }
    *done = true;
    for name in all_names() {
        sounds.request(crate::world_objects::name_id(name), Some(name), false);
    }
}

pub struct SpaceVoicesPlugin;

impl Plugin for SpaceVoicesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (prefetch, space_voices.run_if(in_state(GameStage::Space)))
                .run_if(resource_exists::<RunProgress>)
                .run_if(resource_exists::<NamedSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archetype_slots_follow_the_retail_jump_table() {
        // Retail 0 .. 8: warr, trad, scie, sham, bard, zeal, dipl, ecol, grob.
        assert_eq!(JOINS[voice_slot(Some(2))], "warr_posse_joins"); // Kroxid
        assert_eq!(JOINS[voice_slot(Some(1))], "zeal_posse_joins"); // Zelem
        assert_eq!(JOINS[voice_slot(Some(3))], "scie_posse_joins"); // Avorni
        assert_eq!(JOINS[voice_slot(Some(4))], "dipl_posse_joins"); // Thelos
        assert_eq!(JOINS[voice_slot(Some(5))], "trad_posse_joins"); // Ubrik
        assert_eq!(JOINS[voice_slot(Some(0))], "grob_posse_joins"); // Grox
        assert_eq!(DEAD[voice_slot(None)], "squi_posse_dead");
        let names = all_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no duplicates");
        assert_eq!(names.len(), 68);
    }

    #[test]
    fn the_posse_changes_are_told_apart() {
        let a: Look = vec![(Some(2), 50.0, 100.0), (None, 100.0, 100.0)];
        // A third escort joins, the first is repaired.
        let b: Look = vec![(Some(2), 60.0, 100.0), (None, 100.0, 100.0), (Some(5), 100.0, 100.0)];
        let ev = compare(&a, &b);
        assert_eq!(ev.joined, vec![Some(5)]);
        assert_eq!(ev.healed, vec![Some(2)]);
        assert!(ev.dismissed.is_empty() && ev.dead.is_empty());
        // The empire-5 escort leaves with health: dismissed; the Mini-U with none: dead.
        let c: Look = vec![(Some(2), 60.0, 100.0), (None, 100.0, 100.0)];
        let ev = compare(&b, &c);
        assert_eq!(ev.dismissed, vec![Some(5)]);
        assert_eq!(ev.dead, Vec::<Option<EmpireId>>::new());
        let d: Look = vec![(Some(2), 60.0, 100.0), (None, 0.0, 100.0)];
        let ev = compare(&d, &vec![(Some(2), 60.0, 100.0)]);
        assert_eq!(ev.dead, vec![None]);
    }

    /// The posse, a T-score change, an abduction and a Staff of Life charge queue their sounds.
    #[test]
    fn space_events_queue_their_sounds() {
        use crate::{space::EscortShip, world_objects::name_id};
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, space_voices.run_if(in_state(GameStage::Space)));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Space);
        for _ in 0..4 {
            app.update();
        }
        assert!(queued(&app).is_empty(), "{:?}", queued(&app));
        // An allied Kroxid escort joins, then the Mini-U.
        app.world_mut().resource_mut::<RunProgress>().space_campaign.fleet.push(EscortShip {
            name: "Escort".into(),
            empire: Some(2),
            hp: 100.0,
            max_hp: 100.0,
        });
        app.update();
        assert_eq!(queued(&app), vec![name_id("warr_posse_joins")]);
        app.world_mut().resource_mut::<RunProgress>().space_campaign.fleet.push(EscortShip {
            name: "Mini-U".into(),
            empire: None,
            hp: 100.0,
            max_hp: 100.0,
        });
        app.update();
        let q = queued(&app);
        assert_eq!(&q[1..], &[name_id("squi_posse_joins"), name_id("ui_spg_reinforcements_call")], "{q:?}");
        // The alliance is broken: the Kroxid escort is dismissed.
        app.world_mut().resource_mut::<RunProgress>().space_campaign.fleet.remove(0);
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("warr_posse_dismissed")));
        // A Staff of Life charge is spent; a specimen is abducted.
        app.world_mut().resource_mut::<RunProgress>().space_campaign.staff_of_life_charges = 5;
        app.update();
        app.world_mut().resource_mut::<RunProgress>().space_campaign.staff_of_life_charges = 4;
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("sfx_genesis_energy_blast")));
        app.world_mut().resource_mut::<RunProgress>().space_campaign.cargo_wildlife.flora += 1;
        app.update();
        let q = queued(&app);
        assert_eq!(&q[q.len() - 2..], &[name_id("ufo_abduct_collect_hit"), name_id("sfx_ufo_abduct_success")]);
    }

    /// The rules' posse events play the retail voices of the ship's empire: a rally, the first
    /// damage at full health and a destruction.
    #[test]
    fn posse_events_queue_the_rally_underattack_and_dead_voices() {
        use crate::{space::EscortShip, space_tool_use::{hurt_escort, kill_escort, PosseEvent}, world_objects::name_id};
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, space_voices.run_if(in_state(GameStage::Space)));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Space);
        for _ in 0..4 {
            app.update();
        }
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let campaign = &mut run.space_campaign;
            // Thelos (empire 4, the diplomat voices), a Kroxid (2) and the player's own Mini-U.
            campaign.fleet.push(EscortShip { name: "Thelos".into(), empire: Some(4), hp: 100.0, max_hp: 100.0 });
            campaign.fleet.push(EscortShip { name: "Kroxid".into(), empire: Some(2), hp: 100.0, max_hp: 100.0 });
            campaign.fleet.push(EscortShip { name: "Mini-U".into(), empire: None, hp: 100.0, max_hp: 100.0 });
        }
        for _ in 0..3 {
            app.update();
        }
        let before = queued(&app).len();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let campaign = &mut run.space_campaign;
            campaign.posse_events.push(PosseEvent::Rally(Some(4)));
            campaign.posse_events.push(PosseEvent::Rally(None));
            // The first hit on the full-health Thelos ship: the under-attack voice, once.
            assert!(hurt_escort(campaign, 0, 10.0).is_none());
            assert!(hurt_escort(campaign, 0, 10.0).is_none());
            // The Kroxid's voice is not in the install: retail plays an id without a sound.
            assert!(hurt_escort(campaign, 1, 10.0).is_none());
        }
        app.update();
        let q = queued(&app);
        assert_eq!(&q[before..], &[name_id("dipl_posse_rally"), name_id("squi_posse_rally"), name_id("dipl_posse_underattack")], "{q:?}");
        // Destroyed: the dead voice, then the dismissed voice of the leaving escort.
        let before = queued(&app).len();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            assert!(kill_escort(&mut run.space_campaign, 0).is_some());
        }
        app.update();
        let q = queued(&app);
        assert_eq!(&q[before..], &[name_id("dipl_posse_dead"), name_id("dipl_posse_dismissed")], "{q:?}");
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
}
