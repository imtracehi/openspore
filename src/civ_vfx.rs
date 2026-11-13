//! Civilization: the retail event effects of vehicles, trade routes and nodes
//! (helper `civ-visuals2`, `notes\civ-visuals2-notes.md`).
//!
//! VERIFIED in the code image (ModAPI march2017 addresses, `swarm-volume\target\re\dis.py`):
//! * The vehicle display update 0xE956D0 drains the vehicle's event queue
//!   (`cVehicle::mEvents`, a `deque<int>` at +0xB2C, popped by 0xCA7100, pushed
//!   by 0xCA8270 `PushEvent(code)`) and starts / stops one Swarm effect per
//!   event in a fixed model slot (jump table 0xE97218, codes 1..0x1C). The
//!   economic events: 0x12 starts `civ_economic_cargo_propose_trade_route`
//!   (slot 0x428890E, stopped by 0x13), 0x14 `civ_trade_transport_away`
//!   (slot 0x428890F), 0x15 `civ_trade_transport_home` (slot 0x4EBF115), 0x16
//!   stops the away transport and plays `civ_trade_unload_away` (slot
//!   0x4288914), 0x17 stops the home transport and plays `civ_trade_unload_home`
//!   (slot 0x4EBEEAC); 0x0C / 0x0D / 0x0F / 0x10 / 0x11 are the harvester's
//!   `civ_harvester_capped` + `civ_harvester_fill_balloon`,
//!   `civ_economic_cargo_harvesting` and `civ_harvester_empty_balloon`. Every
//!   one of them gets the vehicle's identity colour (0xC9F7E0) through effect
//!   parameter 5.
//! * Who pushes the trade events: the trade shuttle behaviour 0xDCC7C0 (and its
//!   enter / exit hooks 0xDCC6E0 / 0xDC4790). Leaving home with the cargo
//!   (cargo type 1) pushes 0x14; on arrival at the partner city 0x16 (unload)
//!   and the return load pushes 0x15; back home 0x17 and 0x14 again. The
//!   proposal (0xDCCD80) pushes 0x12 until the nation answers (0xDCA090 pushes
//!   0x13).
//! * The city effect table 0xB3AD20 (event -> effect): 0x4288915 plays
//!   `civ_economic_create_trade_route` when a route is created.
//! * The commodity node display strategy 0xE8FF80: state 1 (a pump under construction) plays
//!   `civ_spice_pump_unpack` (or `civ_ocean_pump_unpack` at sea) once in slot
//!   0x547EFC1; the pump capsule itself is `civ_spice_pump_projectile` and lands
//!   with `civ_spice_pump_landing` (0xCC3573 / 0xCB946C). Since `civ-harvest` those are
//!   played by `civ_pump` along the retail construction (capsule flight, 5 s timer).
//!
//! The effects were checked with `--swarm-fx NAME` (`captures\fxprev`): the
//! transport effects are cargo balloons (the `ci_trade_balloons` game model)
//! hanging over the vehicle, the proposal a trade-route arrow icon.
//!
//! ADAPTED (marked in the code): OpenSpore's trade shuttle (`civ_rts`) stands in
//! for the retail behaviour, so the events are derived from its state (leg,
//! route open, waiting at the rival city); the effect scale is world scale 1 like
//! the trails (`civ_fx`: the vehicle root is drawn `CITY_SCALE` times larger).
//! There are no harvester vehicles: the retail ones are cosmetic (the income is
//! paid by the nodes directly, 0xBF7164), see `notes\civ-harvest-notes.md`.
//!
//! `OPENSPORE_CIV_VFX=0` turns this module and the retail destination arrows
//! off (the pre-round look: crate convoys and glass beams, the torus marker).

use bevy::prelude::*;
use std::collections::HashMap;

use crate::civ::{Approach, UnitOrder, HOME_INDEX};
use crate::civ_planet::CITY_SCALE;
use crate::civ_rts::{CivUnit, UnitAction};
use crate::game::RunProgress;
use crate::stages::GameStage;
use crate::swarm_fx::{SwarmPlay, SwarmStop};
use crate::world_objects::name_id;

pub const PROPOSE_ROUTE: &str = "civ_economic_cargo_propose_trade_route";
pub const TRANSPORT_AWAY: &str = "civ_trade_transport_away";
pub const TRANSPORT_HOME: &str = "civ_trade_transport_home";
pub const UNLOAD_AWAY: &str = "civ_trade_unload_away";
pub const UNLOAD_HOME: &str = "civ_trade_unload_home";
pub const CREATE_ROUTE: &str = "civ_economic_create_trade_route";
pub const PUMP_LANDING: &str = crate::civ_pump::LANDING;
pub const PUMP_UNPACK: &str = crate::civ_pump::UNPACK;
pub const SELECTION_BUILDING: &str = "civ_selection_building";

/// Tags of the looping vehicle slots (the low byte is the slot) and of the
/// building circle. 0xC1F0..0xC1F7 belong to `civ_fx` / `spice_presentation`.
const TAG_TRADE: u64 = 0xC1F8_0000_0000_0000;
const TAG_CIRCLE: u64 = 0xC1F9_0000_0000_0000;
const SLOT_PROPOSE: u64 = 1;
const SLOT_AWAY: u64 = 2;
const SLOT_HOME: u64 = 3;

/// ADAPTED: seconds between two plays of the one-shot building circle while
/// its target stays selected (the retail ring has no loop: the building
/// selection is held by its caller, which was not traced).
const CIRCLE_PERIOD: f32 = 1.6;

/// `OPENSPORE_CIV_VFX=0` brings the pre-round look back.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_CIV_VFX").map_or(true, |v| v != "0"))
}

/// Which way a trade shuttle is heading (retail cargo type 1 = away, 2 = home).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leg {
    Away,
    Home,
}

/// What an economic vehicle shows (its effect slots).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeLook {
    /// 0x12 / 0x13: the proposal popup.
    pub propose: bool,
    /// 0x14 / 0x15: the cargo balloons of the leg.
    pub cargo: Option<Leg>,
}

/// Pure: the slots of one vehicle. `route_open` is `Some(open)` when its order is a city (the
/// ordered city's `trade` flag). Only the player's economic vehicles trade (`civ_rts`).
/// ADAPTED: the retail behaviour pushes the events itself; here they follow the shuttle's state.
pub fn trade_look(
    kind: Approach,
    nation: usize,
    order: UnitOrder,
    action: UnitAction,
    homeward: bool,
    route_open: Option<bool>,
) -> TradeLook {
    if kind != Approach::Economic || nation != HOME_INDEX {
        return TradeLook::default();
    }
    let UnitOrder::City(city) = order else {
        return TradeLook::default();
    };
    match route_open {
        // 0xDCCD80: proposing until the nation answers (a refusal keeps asking).
        Some(false) => TradeLook { propose: action == UnitAction::Trading(city), cargo: None },
        // 0xDCC7C0: cargo type 1 outbound, 2 homeward.
        Some(true) => TradeLook { propose: false, cargo: Some(if homeward { Leg::Home } else { Leg::Away }) },
        None => TradeLook::default(),
    }
}

/// One change of a vehicle's trade slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeFx {
    Start(&'static str, u64),
    Stop(u64),
    /// A one-shot at the vehicle (the unload).
    Once(&'static str),
}

fn transport(leg: Leg) -> (&'static str, u64) {
    match leg {
        Leg::Away => (TRANSPORT_AWAY, TAG_TRADE | SLOT_AWAY),
        Leg::Home => (TRANSPORT_HOME, TAG_TRADE | SLOT_HOME),
    }
}

/// Pure (0xE97218 events 0x12..0x17): what a change of look starts and stops. A leg that ends
/// stops its balloons and unloads (0x16 away, 0x17 home) before the next leg's balloons start.
pub fn trade_step(before: TradeLook, now: TradeLook) -> Vec<TradeFx> {
    let mut out = Vec::new();
    if before.propose != now.propose {
        if now.propose {
            out.push(TradeFx::Start(PROPOSE_ROUTE, TAG_TRADE | SLOT_PROPOSE));
        } else {
            out.push(TradeFx::Stop(TAG_TRADE | SLOT_PROPOSE));
        }
    }
    if before.cargo != now.cargo {
        if let Some(leg) = before.cargo {
            out.push(TradeFx::Stop(transport(leg).1));
            // The unload plays only when the next leg follows (a delivery), not when the
            // vehicle drops the route.
            if now.cargo.is_some_and(|next| next != leg) {
                out.push(TradeFx::Once(match leg {
                    Leg::Away => UNLOAD_AWAY,
                    Leg::Home => UNLOAD_HOME,
                }));
            }
        }
        if let Some(leg) = now.cargo {
            let (name, tag) = transport(leg);
            out.push(TradeFx::Start(name, tag));
        }
    }
    out
}

/// Play `name` attached to `vehicle` at world scale 1 (the root is drawn `CITY_SCALE` larger).
fn play_on(plays: &mut MessageWriter<SwarmPlay>, name: &str, vehicle: Entity, tag: u64) {
    let mut play = SwarmPlay::at(name_id(name), Vec3::ZERO);
    play.attach_to = Some(vehicle);
    play.scale = 1.0 / CITY_SCALE;
    play.tag = tag;
    plays.write(play);
}

/// What the systems below showed last frame (cleared when the stage ends).
#[derive(Resource, Default)]
struct VfxMemory {
    shown: HashMap<Entity, TradeLook>,
    routes: HashMap<usize, bool>,
    circle: Option<(crate::civ_orders::PieTarget, f32)>,
}

/// The economic vehicles' balloons and popups (0xE956D0 events 0x12..0x17).
fn trade_vehicle_effects(
    run: Res<RunProgress>,
    units: Query<(Entity, &CivUnit)>,
    mut memory: ResMut<VfxMemory>,
    mut plays: MessageWriter<SwarmPlay>,
    mut stops: MessageWriter<SwarmStop>,
) {
    let shown = &mut memory.shown;
    let campaign = &run.civ_campaign;
    let mut live = Vec::new();
    for (entity, unit) in &units {
        live.push(entity);
        let route_open = match unit.order {
            UnitOrder::City(c) => campaign.cities.get(c).map(|city| city.trade && !city.owned()),
            _ => None,
        };
        let now = trade_look(unit.kind, unit.nation, unit.order, unit.action, unit.homeward, route_open);
        let before = shown.get(&entity).copied().unwrap_or_default();
        if before == now {
            continue;
        }
        for change in trade_step(before, now) {
            match change {
                TradeFx::Start(name, tag) => play_on(&mut plays, name, entity, tag),
                TradeFx::Stop(tag) => {
                    stops.write(SwarmStop { effect: 0, attach_to: Some(entity), tag, hard: false });
                }
                TradeFx::Once(name) => play_on(&mut plays, name, entity, 0),
            }
        }
        shown.insert(entity, now);
    }
    shown.retain(|e, _| live.contains(e));
}

/// A route being created (city effect table 0xB3AD20, event 0x4288915). The derrick's pump
/// effects belong to `civ_pump` (the retail construction: capsule flight, unpack, 5 s timer).
fn node_and_route_effects(
    run: Res<RunProgress>,
    mut memory: ResMut<VfxMemory>,
    mut plays: MessageWriter<SwarmPlay>,
) {
    let routes = &mut memory.routes;
    let campaign = &run.civ_campaign;
    if campaign.cities.is_empty() {
        return;
    }
    let shape = campaign.shape();
    for city in &campaign.cities {
        let was = routes.insert(city.index, city.trade);
        if was == Some(false) && city.trade {
            let center = city.center();
            let mut play = SwarmPlay::at(name_id(CREATE_ROUTE), center);
            play.rotation = Quat::from_rotation_arc(Vec3::Y, shape.up(center));
            plays.write(play);
        }
    }
}

/// The building circle (`civ_selection_building`, 0x3051F2C9: one camera-facing additive ring,
/// 5 -> 15 -> 12 units, offset 5 up) around the target of an open pie menu. ADAPTED: retail
/// plays it for a selected building; OpenSpore's selected buildings are the pie's targets (a
/// rival hall or derrick), and the ring is replayed while the pie stays open.
fn building_circle(
    time: Res<Time>,
    run: Res<RunProgress>,
    pie: Option<Res<crate::civ_orders::CivPie>>,
    mut memory: ResMut<VfxMemory>,
    mut plays: MessageWriter<SwarmPlay>,
    mut stops: MessageWriter<SwarmStop>,
) {
    let last = &mut memory.circle;
    let target = pie.as_ref().and_then(|p| p.open.as_ref()).map(|o| o.target);
    let now = time.elapsed_secs();
    match (target, *last) {
        (None, Some(_)) => {
            stops.write(SwarmStop { effect: 0, attach_to: None, tag: TAG_CIRCLE, hard: true });
            *last = None;
        }
        (Some(target), previous) => {
            let due = previous.is_none_or(|(t, at)| t != target || now - at >= CIRCLE_PERIOD);
            if !due {
                return;
            }
            let campaign = &run.civ_campaign;
            let at = match target {
                crate::civ_orders::PieTarget::City(c) => campaign.cities.get(c).map(|city| city.center()),
                crate::civ_orders::PieTarget::Geyser(g) => Some(campaign.geyser_at(g)),
            };
            let Some(at) = at else { return };
            if previous.is_some_and(|(t, _)| t != target) {
                stops.write(SwarmStop { effect: 0, attach_to: None, tag: TAG_CIRCLE, hard: true });
            }
            let mut play = SwarmPlay::at(name_id(SELECTION_BUILDING), at);
            play.rotation = Quat::from_rotation_arc(Vec3::Y, campaign.shape().up(at));
            play.tag = TAG_CIRCLE;
            plays.write(play);
            *last = Some((target, now));
        }
        (None, None) => {}
    }
}

/// Forget everything when the stage ends (the effects are `StageOwned`).
fn forget(mut memory: ResMut<VfxMemory>) {
    *memory = VfxMemory::default();
}

pub struct CivVfxPlugin;

impl Plugin for CivVfxPlugin {
    fn build(&self, app: &mut App) {
        if !on() {
            return;
        }
        app.add_plugins(crate::civ_pump::CivPumpPlugin).init_resource::<VfxMemory>().add_systems(
            Update,
            (trade_vehicle_effects, node_and_route_effects, building_circle)
                .run_if(in_state(GameStage::Civilization))
                .run_if(resource_exists::<Messages<SwarmPlay>>),
        )
        .add_systems(OnExit(GameStage::Civilization), forget);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm_fx::SwarmEffect;

    const AWAY: TradeLook = TradeLook { propose: false, cargo: Some(Leg::Away) };
    const HOME: TradeLook = TradeLook { propose: false, cargo: Some(Leg::Home) };

    #[test]
    fn only_the_players_economic_vehicles_show_trade_events() {
        let city = UnitOrder::City(2);
        let at = UnitAction::Idle;
        assert_eq!(trade_look(Approach::Economic, HOME_INDEX, city, at, false, Some(true)), AWAY);
        assert_eq!(trade_look(Approach::Economic, HOME_INDEX, city, at, true, Some(true)), HOME);
        assert_eq!(trade_look(Approach::Military, HOME_INDEX, city, at, false, Some(true)), TradeLook::default());
        assert_eq!(trade_look(Approach::Economic, 2, city, at, false, Some(true)), TradeLook::default());
        assert_eq!(trade_look(Approach::Economic, HOME_INDEX, UnitOrder::Idle, at, false, None), TradeLook::default());
    }

    #[test]
    fn a_proposal_shows_its_popup_only_while_the_vehicle_waits_at_the_city() {
        let city = UnitOrder::City(2);
        let asking = trade_look(Approach::Economic, HOME_INDEX, city, UnitAction::Trading(2), false, Some(false));
        assert_eq!(asking, TradeLook { propose: true, cargo: None });
        let driving = trade_look(Approach::Economic, HOME_INDEX, city, UnitAction::Moving, false, Some(false));
        assert_eq!(driving, TradeLook::default(), "still on its way: nothing yet");
        // The route opens: the popup stops and the cargo leg starts.
        let steps = trade_step(asking, AWAY);
        assert_eq!(
            steps,
            vec![TradeFx::Stop(TAG_TRADE | SLOT_PROPOSE), TradeFx::Start(TRANSPORT_AWAY, TAG_TRADE | SLOT_AWAY)]
        );
    }

    #[test]
    fn deliveries_unload_and_swap_the_balloons() {
        // Partner city reached: away balloons stop, unload_away plays, home balloons start.
        assert_eq!(
            trade_step(AWAY, HOME),
            vec![
                TradeFx::Stop(TAG_TRADE | SLOT_AWAY),
                TradeFx::Once(UNLOAD_AWAY),
                TradeFx::Start(TRANSPORT_HOME, TAG_TRADE | SLOT_HOME)
            ]
        );
        // Back home: 0x17 then the away leg again.
        assert_eq!(
            trade_step(HOME, AWAY),
            vec![
                TradeFx::Stop(TAG_TRADE | SLOT_HOME),
                TradeFx::Once(UNLOAD_HOME),
                TradeFx::Start(TRANSPORT_AWAY, TAG_TRADE | SLOT_AWAY)
            ]
        );
        // A route that is dropped only stops its balloons.
        assert_eq!(trade_step(AWAY, TradeLook::default()), vec![TradeFx::Stop(TAG_TRADE | SLOT_AWAY)]);
        assert!(trade_step(AWAY, AWAY).is_empty());
    }

    #[test]
    #[ignore = "requires installed Spore resource packages"]
    fn the_retail_effects_are_installed() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        for name in [
            PROPOSE_ROUTE,
            TRANSPORT_AWAY,
            TRANSPORT_HOME,
            UNLOAD_AWAY,
            UNLOAD_HOME,
            CREATE_ROUTE,
            PUMP_LANDING,
            PUMP_UNPACK,
            SELECTION_BUILDING,
            crate::civ_fx::SELECTION_DESTINATION,
            crate::civ_fx::SELECTION_VEHICLE,
        ] {
            assert!(lib.find(name_id(name)).is_some(), "{name} is not installed");
        }
    }

    // ---- the Civ scene (headless): the systems start and stop the effects ----

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        app
    }

    /// The live trade effects of `unit` as (effect name, tag, stopping).
    fn effects_on(app: &mut App, unit: Entity) -> Vec<(u32, u64, bool)> {
        let mut query = app.world_mut().query::<&SwarmEffect>();
        query
            .iter(app.world())
            .filter(|e| e.attach_to == Some(unit))
            .map(|e| (e.effect, e.tag, e.stop))
            .collect()
    }

    #[test]
    fn a_trade_shuttle_carries_balloons_and_swaps_them_at_each_delivery() {
        if !on() {
            return;
        }
        let mut app = civ_app();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.civ_campaign.ensure_cities();
            run.civ_campaign.cities[2].trade = true;
        }
        let state = crate::civ::UnitState { id: 900, nation: HOME_INDEX, kind: Approach::Economic, ..Default::default() };
        let mut unit = CivUnit::from_state(&state);
        unit.order = UnitOrder::City(2);
        let vehicle = app.world_mut().spawn((unit, Transform::from_translation(Vec3::new(500.0, 0.0, 500.0)))).id();
        for _ in 0..4 {
            app.update();
        }
        let away = (name_id(TRANSPORT_AWAY), TAG_TRADE | SLOT_AWAY);
        let live = effects_on(&mut app, vehicle);
        assert!(live.iter().any(|(e, t, stop)| (*e, *t) == away && !stop), "outbound balloons: {live:?}");
        // The delivery at the partner city: the shuttle turns homeward.
        app.world_mut().get_mut::<CivUnit>(vehicle).unwrap().homeward = true;
        for _ in 0..4 {
            app.update();
        }
        let live = effects_on(&mut app, vehicle);
        assert!(live.iter().any(|(e, t, stop)| (*e, *t) == away && *stop), "the away balloons stop: {live:?}");
        assert!(live.iter().any(|(e, _, _)| *e == name_id(UNLOAD_AWAY)), "unload_away plays: {live:?}");
        assert!(
            live.iter().any(|(e, t, stop)| (*e, *t) == (name_id(TRANSPORT_HOME), TAG_TRADE | SLOT_HOME) && !stop),
            "homeward balloons: {live:?}"
        );
    }

    #[test]
    fn tags_do_not_collide_with_the_other_civ_effects() {
        for tag in [TAG_TRADE, TAG_CIRCLE] {
            assert_eq!(tag & 0x0000_FFFF_FFFF_FFFF, 0);
            let family = tag >> 48;
            assert!(family > 0xC1F7, "{family:x}");
        }
        assert_ne!(TAG_TRADE, TAG_CIRCLE);
    }
}
