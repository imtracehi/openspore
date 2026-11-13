//! The parts of the Civ HUD that belong to the world: the card of the city,
//! vehicle, building or spice geyser under the pointer (`RolloverCivCity`,
//! `Rollover_CivVehicle`, `Rollover_Building`, `CivSpiceMeter`), the buy
//! buttons of a city the player clicked on, and the planet minimap.
//!
//! [`publish_world`] picks what the pointer rests on from the campaign and
//! the live vehicles and fills [`WorldState`]; [`bind_world`] pushes it into
//! the layouts. The minimap picture is painted on the CPU into an image that
//! a plain UI node shows over the holder window of `GlobalUICiv3-3`.
//!
//! ADAPTED: the retail code that decides what a hover shows is not decoded.
//! The cards show the data the rewrite keeps (population, mood, defence and
//! income of a city; armour of a building; hit points of a vehicle).

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{id as hud_id, CivHudActive, CivHudLocal, CivHudRoot, CivHudState};
use crate::{
    civ::{self, Approach, CivCampaign, VehicleDomain, HOME_INDEX},
    civ_rts::{CivUnit, Selected, UnitSpawns},
    game::RunProgress,
    spui::view::{SpuiAssets, SpuiPointer, SpuiState},
    stages::GameStage,
};

pub const CITY_LAYOUT: &str = "RolloverCivCity";
pub const VEHICLE_LAYOUT: &str = "Rollover_CivVehicle";
pub const BUILDING_LAYOUT: &str = "Rollover_Building";
pub const SPICE_LAYOUT: &str = "CivSpiceMeter";

/// Control ids of the four card layouts.
#[allow(dead_code)]
pub mod id {
    // RolloverCivCity
    pub const CITY_ROOT: u32 = 0x0199_A7DE;
    /// The arrow under the card; its tip points at the city.
    pub const CITY_POINTER: u32 = 0x04A0_E5D0;
    pub const CITY_NAME: u32 = 0x02CF_2F44;
    pub const CITY_FACE: u32 = 0x046A_DF86;
    /// Kind icons: economic, religious, military.
    pub const CITY_KINDS: [u32; 3] = [0x02CF_2F30, 0x02CF_2F34, 0x02CF_2F38];
    pub const CITY_POPULATION: u32 = 0x0199_A7D2;
    /// Five-orb meters: mood, defence.
    pub const CITY_MOOD: [u32; 5] = [0x04FE_1AA0, 0x04FE_1AA1, 0x04FE_1AA2, 0x04FE_1AA3, 0x04FE_1AA4];
    pub const CITY_DEFENCE: [u32; 5] = [0x04FE_38F0, 0x04FE_38F1, 0x04FE_38F2, 0x04FE_38F3, 0x04FE_38F4];
    pub const CITY_INCOME: u32 = 0x02CF_2F74;
    /// City Planner, buy land / sea / air vehicle.
    pub const CITY_PLANNER: u32 = 0x062D_5618;
    pub const CITY_BUY: [u32; 3] = [0x062D_6902, 0x062D_6908, 0x062D_690F];
    // Rollover_CivVehicle / Rollover_Building (the shared card ids)
    pub const ROLL_ROOT: u32 = 0x9331_0E4A;
    pub const ROLL_PLATE: u32 = 0x9331_03EA;
    pub const ROLL_NAME: u32 = 0x031E_94D0;
    pub const ROLL_BAR: u32 = 0x026E_0DF3;
    pub const ROLL_HEALTH: u32 = 0x026E_0DF0;
    pub const ROLL_HEAL: u32 = 0x026E_0DF1;
    pub const ROLL_HURT: u32 = 0x026E_0DF2;
    pub const ROLL_NUMBER: u32 = 0x04D0_39A8;
    pub const ROLL_FACE: u32 = 0x046A_DF86;
    /// The dark body of a card and the pointer under it.
    pub const ROLL_BACKDROP: u32 = 0x9331_03EB;
    pub const ROLL_POINTER: u32 = 0x0492_1F0C;
    /// The plate behind the city's name (two windows carry the id of the
    /// header group) and its frame.
    pub const CITY_PLATES: [u32; 2] = [0x02CF_324C, 0x04B1_F8AE];
    /// Vehicle card: type icon on the plate; stat rows the rewrite hides.
    pub const VEHICLE_TYPE: u32 = 0x026E_0001;
    pub const VEHICLE_STATS: [u32; 4] = [0x0553_EA66, 0x0553_E937, 0x0553_EA6E, 0x0553_E92B];
    /// Building card: type icon.
    pub const BUILDING_TYPE: u32 = 0x0630_3998;
    // CivSpiceMeter
    pub const SPICE_ROOT: u32 = 0x026F_1F20;
    pub const SPICE_NAME: u32 = 0x0624_5A18;
    pub const SPICE_FACE: u32 = 0x046A_DF86;
    pub const SPICE_RATE: u32 = 0x0551_55F9;
    pub const SPICE_METER: u32 = 0x026E_0DF0;
}

/// Frames of the vehicle card's type strip: eye, pot, swords, spice bag
/// (religious, economic, military; the bag is the retail harvester).
pub fn vehicle_type_frame(kind: Approach) -> u32 {
    match kind {
        Approach::Religious => 0,
        Approach::Economic => 1,
        Approach::Military => 2,
    }
}

/// Frames of the five-frame kind icon of a nation's communication button:
/// eye, spice swirl, swords.
pub fn kind_frame(kind: Approach) -> u32 {
    match kind {
        Approach::Religious => 0,
        Approach::Economic => 1,
        Approach::Military => 2,
    }
}

/// Frame of the building type strip: turret shield, house, entertainment
/// masks, factory swirl.
pub fn building_frame(kind: Option<civ::Building>) -> u32 {
    match kind {
        None => 0,
        Some(civ::Building::House) => 1,
        Some(civ::Building::Entertainment) => 2,
        Some(civ::Building::Factory) => 3,
    }
}

/// `RoomsPerHouse` of `CityGameBuildingTuning` (the global at 0x169C088).
pub const ROOMS_PER_HOUSE: u32 = 15;

/// Population of a city. VERIFIED (0xBD8AB0, read by the rollover update 0xBDE100 into
/// the population window `0199A7D2`): the number of houses and city halls whose connected
/// flag (`cBuilding` +0x289, set by `ProcessBuildingUpdate` through 0xBCC850) is on, times
/// `RoomsPerHouse`, so the capital with its hall and two linked houses reads 45.
/// (`InitialCreatures` 20 is how many citizens spawn, not the number shown.)
pub fn population(city: &civ::CityState) -> u32 {
    ROOMS_PER_HOUSE * city.housing()
}

/// VERIFIED (0xBDE1BE .. 0xBDE238): the colour of the income text, `0xAARRGGBB`: white
/// when the (party / riot adjusted) income is not above zero, else green while the city
/// parties above neutral mood, red while it riots below it, cyan otherwise.
pub fn income_color(city: &civ::CityState) -> u32 {
    const WHITE: u32 = 0xFFFF_FFFF;
    const GREEN: u32 = 0xFF00_FF00;
    const RED: u32 = 0xFFFF_0000;
    const CYAN: u32 = 0xFF00_FFFF;
    let mood = city.happiness();
    if city.income() <= 0.0 {
        WHITE
    } else if city.partying() && mood > crate::civ_rules::HAPPINESS_NEUTRAL {
        GREEN
    } else if city.rioting() && mood < crate::civ_rules::HAPPINESS_NEUTRAL {
        RED
    } else {
        CYAN
    }
}

/// Happy minus unhappy count of a city (`cCity` +0x668 - +0x66C, VERIFIED 0xBDE241; the
/// link model's `CityModel`), the number of lit mood orbs with its sign.
pub fn mood_delta(city: &civ::CityState) -> i32 {
    let model = city.model();
    model.happy - model.unhappy
}

/// Lit orbs (of five) for a mood fraction (`sim::mood_fraction`). Legacy: only the City
/// Planner's stats panel (`civ_planner`) still uses it; the rollover card uses
/// [`mood_delta`].
pub fn mood_orbs(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 5.0).round() as u32
}

/// Lit orbs (of five) for a city's defence. VERIFIED (0xBDE2CF): one orb per turret
/// whose combatant state is 0 (standing), at most five.
pub fn defence_orbs(city: &civ::CityState) -> u32 {
    (city.live_turrets() as u32).min(5)
}

/// Frame of mood icon `i` (of five) for a happy-minus-unhappy `delta` (VERIFIED 0xBDE241):
/// the first |delta| icons are lit, with the value 100 (the green happy face, frame 2) for a
/// positive delta and 50 (the red unhappy face, frame 1) for a negative one; the rest
/// stay dark (frame 0), so an even city shows five dark icons.
pub fn mood_frame_delta(i: u32, delta: i32) -> u32 {
    if (i as i32) >= delta.abs() {
        0
    } else if delta > 0 {
        2
    } else {
        1
    }
}

/// Frame of mood icon `i` (of five) when `lit` of them are on (legacy: only the City
/// Planner's stats panel uses it). ADAPTED: red below three lit icons, green from three.
pub fn mood_frame(i: u32, lit: u32) -> u32 {
    if i >= lit {
        0
    } else if lit >= 3 {
        2
    } else {
        1
    }
}

/// Frame (0..=15) of the 16-frame bar of the spice card for a fraction.
pub fn spice_frame(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 15.0).round() as u32
}

/// One buy button of the city card.
#[derive(Clone, Debug, PartialEq)]
pub struct BuyOption {
    pub domain: VehicleDomain,
    pub cost: f32,
    /// Why the vehicle cannot be bought now, if it cannot.
    pub refused: Option<String>,
}

/// The card of the city under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct CityCard {
    pub index: usize,
    pub name: String,
    pub kind: Approach,
    pub owned: bool,
    /// Identity colour of the city's nation (`0xAARGB`).
    pub color: u32,
    /// Relationship with the nation (faces; the player's own citizens show
    /// their mood).
    pub relation: f32,
    pub population: u32,
    /// Happy minus unhappy count ([`mood_delta`]).
    pub mood: i32,
    pub defence: u32,
    pub per_minute: f32,
    /// Colour of the income text ([`income_color`]).
    pub income_color: u32,
    /// The buy buttons are shown (an owned city the player clicked).
    pub pinned: bool,
    pub buy: Vec<BuyOption>,
    /// Anchor in physical screen pixels (the hall).
    pub screen: Vec2,
}

/// The card of a vehicle.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleCard {
    pub nation: usize,
    pub kind: Approach,
    pub domain: VehicleDomain,
    pub health: f32,
    pub hp: i32,
    pub relation: f32,
    pub screen: Vec2,
}

/// The card of a building (or a wall turret).
#[derive(Clone, Debug, PartialEq)]
pub struct BuildingCard {
    pub nation: usize,
    pub name: String,
    pub frame: u32,
    pub health: f32,
    pub hp: i32,
    pub relation: f32,
    pub screen: Vec2,
}

/// The card of a spice geyser.
#[derive(Clone, Debug, PartialEq)]
pub struct SpiceCard {
    pub owner: Option<usize>,
    pub name: String,
    /// Spice per minute of the derrick on it.
    pub per_minute: f32,
    pub relation: f32,
    /// Health of the derrick (the meter), `0.0..=1.0`.
    pub meter: f32,
    pub screen: Vec2,
}

/// Cards and the minimap's view of the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldState {
    pub city: Option<CityCard>,
    pub vehicle: Option<VehicleCard>,
    pub building: Option<BuildingCard>,
    pub spice: Option<SpiceCard>,
    /// The pointer is over the minimap picture (clicks move the camera).
    pub over_minimap: bool,
}

/// What the pointer can rest on in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Vehicle(u32),
    Plot(usize, usize),
    Turret(usize, usize),
    Geyser(usize),
    City(usize),
}

impl Target {
    /// Smaller wins when targets overlap on screen.
    fn priority(self) -> u8 {
        match self {
            Target::Vehicle(_) => 0,
            Target::Plot(..) | Target::Turret(..) => 1,
            Target::Geyser(_) => 2,
            Target::City(_) => 3,
        }
    }
}

/// A pickable target at a screen position with its pick radius (pixels).
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub target: Target,
    pub screen: Vec2,
    pub radius: f32,
}

/// The target under `cursor`: the highest priority one whose circle holds the
/// cursor, the nearest among equals.
pub fn pick(candidates: &[Candidate], cursor: Vec2) -> Option<Candidate> {
    candidates
        .iter()
        .filter(|c| c.screen.distance(cursor) <= c.radius)
        .min_by(|a, b| {
            a.target
                .priority()
                .cmp(&b.target.priority())
                .then(a.screen.distance(cursor).total_cmp(&b.screen.distance(cursor)))
        })
        .copied()
}

/// How far a card reaches from its anchor: left, right and up (the vehicle
/// and building cards hang above their anchor like the creature card).
const CARD_LEFT: f32 = 76.0;
const CARD_RIGHT: f32 = 88.0;
const CARD_UP: f32 = 124.0;

/// The card anchor moved so that the card stays on screen.
pub fn clamp_anchor(anchor: Vec2, screen: Vec2) -> Vec2 {
    Vec2::new(
        anchor.x.clamp(CARD_LEFT, (screen.x - CARD_RIGHT).max(CARD_LEFT)),
        anchor.y.clamp(CARD_UP, (screen.y - 8.0).max(CARD_UP)),
    )
}

/// The buy options of a city card from a dry run of the purchase on a copy of
/// the campaign: the reason it is refused is the one the key command would
/// give.
pub fn buy_options(campaign: &CivCampaign, spice: f32, live: usize, city: usize) -> Vec<BuyOption> {
    [VehicleDomain::Land, VehicleDomain::Sea, VehicleDomain::Air]
        .into_iter()
        .map(|domain| {
            let mut trial = campaign.clone();
            let mut purse = spice;
            let cost = match domain {
                VehicleDomain::Land => civ::LAND_VEHICLE_COST,
                VehicleDomain::Sea => civ::SEA_VEHICLE_COST,
                VehicleDomain::Air => civ::AIR_VEHICLE_COST,
            };
            let refused = crate::civ_rts::try_produce_domain(&mut trial, &mut purse, live, city, domain).err();
            BuyOption { domain, cost, refused }
        })
        .collect()
}

/// Retail wording of a buy tooltip with the rewrite's price (the retail
/// strings carry $1000 / $1500 / $3000).
pub fn buy_tooltip(retail: &str, retail_price: u32, option: &BuyOption) -> String {
    let mut text = retail.replacen(&retail_price.to_string(), &format!("{:.0}", option.cost), 1);
    if let Some(why) = &option.refused {
        text.push('\n');
        text.push_str(why);
    }
    text
}

/// Pixels of one world unit near a point: the distance between the screen
/// positions of `p` and `p + X`.
fn pixels_per_unit(project: &dyn Fn(Vec3) -> Option<Vec2>, p: Vec3) -> f32 {
    match (project(p), project(p + Vec3::X)) {
        (Some(a), Some(b)) => a.distance(b),
        _ => 0.0,
    }
}

/// Pick the city, building, vehicle or geyser under the pointer and fill the
/// cards of [`WorldState`]. A left click (not a drag) on one of the player's
/// cities keeps its card (with the buy buttons) open; a click elsewhere
/// closes it.
#[allow(clippy::too_many_arguments)]
pub fn publish_world(
    stage: Res<State<GameStage>>,
    active: Res<CivHudActive>,
    run: Res<RunProgress>,
    pointer: Res<SpuiPointer>,
    mouse: Res<ButtonInput<MouseButton>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    units: Query<(&CivUnit, &Transform)>,
    spawns: Option<Res<UnitSpawns>>,
    mut local: ResMut<CivHudLocal>,
    mut hud: ResMut<CivHudState>,
    mut press: Local<Option<Vec2>>,
) {
    if *stage.get() != GameStage::Civilization || !active.0 {
        hud.world = WorldState::default();
        return;
    }
    let campaign = &run.civ_campaign;
    let Ok((camera, camera_t)) = cameras.single() else {
        return;
    };
    let scale = camera.target_scaling_factor().unwrap_or(1.0);
    let shape = campaign.shape();
    let eye = camera_t.translation();
    // Nothing behind the planet's limb is picked.
    let project = |position: Vec3| {
        shape
            .visible_from(eye, position)
            .then(|| camera.world_to_viewport(camera_t, position).ok().map(|p| p * scale))
            .flatten()
    };
    let unit_scale = crate::civ_planet::CITY_SCALE;
    // Candidates, front to back by priority.
    let mut candidates: Vec<Candidate> = Vec::new();
    for (unit, t) in &units {
        if unit.hp <= 0.0 {
            continue;
        }
        if let Some(screen) = project(t.translation + shape.up(t.translation) * 0.6 * unit_scale) {
            candidates.push(Candidate { target: Target::Vehicle(unit.id), screen, radius: 26.0 });
        }
    }
    for city in &campaign.cities {
        let center = city.center();
        let hall = shape.local_point(center, Vec3::Y * 1.4 * unit_scale);
        // Pixels per island unit of the city's layout.
        let unit_px = pixels_per_unit(&project, hall) * unit_scale;
        for (plot, building) in city.plots.iter().enumerate() {
            if building.is_none() {
                continue;
            }
            let site = city.plot_site(plot);
            let at = shape.local_point(center, Vec3::new(site.x, 1.0, site.y) * unit_scale);
            if let Some(screen) = project(at) {
                candidates.push(Candidate {
                    target: Target::Plot(city.index, plot),
                    screen,
                    radius: (unit_px * 1.3).clamp(14.0, 40.0),
                });
            }
        }
        for turret in 0..city.turret_hp.len() {
            if city.turret_hp[turret] <= 0.0 {
                continue;
            }
            if let Some(screen) = project(crate::civ_rts::turret_world(city, turret, shape)) {
                candidates.push(Candidate {
                    target: Target::Turret(city.index, turret),
                    screen,
                    radius: (unit_px * 1.2).clamp(12.0, 34.0),
                });
            }
        }
        if let Some(screen) = project(hall) {
            candidates.push(Candidate {
                target: Target::City(city.index),
                screen,
                radius: (unit_px * civ::CITY_RANGE / unit_scale * 0.45).clamp(36.0, 120.0),
            });
        }
    }
    for g in 0..civ::GEYSER_COUNT {
        let p = campaign.geyser_at(g);
        if let Some(screen) = project(p + shape.up(p) * 0.4 * unit_scale) {
            candidates.push(Candidate { target: Target::Geyser(g), screen, radius: 30.0 });
        }
    }
    // The pointer: not over a window of the HUD (the cards' own buttons) and
    // not over the minimap picture.
    let over_hud = pointer.over_ui || hud.world.over_minimap;
    let cursor = pointer.position.filter(|_| !over_hud);
    let hovered = cursor.and_then(|c| pick(&candidates, c));
    // Click tracking for the pinned city card.
    if mouse.just_pressed(MouseButton::Left) {
        *press = pointer.position.filter(|_| !over_hud);
    }
    if mouse.just_released(MouseButton::Left) {
        if let (Some(start), Some(now)) = (press.take(), pointer.position) {
            if start.distance(now) <= 6.0 {
                local.pinned_city = match hovered.map(|h| h.target) {
                    Some(Target::City(i) | Target::Plot(i, _) | Target::Turret(i, _))
                        if campaign.cities.get(i).is_some_and(|c| c.owned()) =>
                    {
                        Some(i)
                    }
                    _ => None,
                };
            }
        }
    }
    if local.pinned_city.is_some_and(|i| !campaign.cities.get(i).is_some_and(|c| c.owned())) {
        local.pinned_city = None;
    }
    // The vehicle cap is per type (`civ_rules`): count the player's vehicles of
    // the city's own type.
    let live_of = |kind: civ::Approach| {
        units.iter().filter(|(u, _)| u.nation == HOME_INDEX && u.kind == kind).count()
            + spawns
                .as_deref()
                .map_or(0, |s| s.0.iter().filter(|u| u.nation == HOME_INDEX && u.kind == kind).count())
    };
    let city_of = |index: usize, screen: Vec2| -> Option<CityCard> {
        let city = campaign.cities.get(index)?;
        let owned = city.owned();
        // VERIFIED (0xBDE2CF .. 0xBDE3E7): the face is the player's own 50 (neutral) on his
        // cities and the attitude level of the nation's civilization on the others.
        let relation = super::sim::nation_relation(campaign, city.nation());
        let pinned = owned && local.pinned_city == Some(index);
        Some(CityCard {
            index,
            name: civ::city_label(index),
            kind: city.kind,
            owned,
            color: super::sim::nation_argb(city.nation()),
            relation,
            population: population(city),
            mood: mood_delta(city),
            defence: defence_orbs(city),
            per_minute: super::sim::income_per_minute(city),
            income_color: income_color(city),
            pinned,
            buy: if pinned {
                buy_options(campaign, super::sim::money(&run), live_of(city.kind), index)
            } else {
                Vec::new()
            },
            screen,
        })
    };
    let relation_of_nation = |nation: usize| -> f32 {
        super::sim::nation_relation(campaign, nation)
    };
    let mut state = WorldState { over_minimap: hud.world.over_minimap, ..default() };
    match hovered.map(|h| (h.target, h.screen)) {
        Some((Target::Vehicle(id), screen)) => {
            if let Some((unit, _)) = units.iter().find(|(u, _)| u.id == id) {
                state.vehicle = Some(VehicleCard {
                    nation: unit.nation,
                    kind: unit.kind,
                    domain: unit.domain,
                    health: (unit.hp / unit.max_hp.max(1.0)).clamp(0.0, 1.0),
                    hp: unit.hp.max(0.0).ceil() as i32,
                    relation: relation_of_nation(unit.nation),
                    screen,
                });
            }
        }
        Some((Target::Plot(i, plot), screen)) => {
            let city = &campaign.cities[i];
            let building = city.plots.get(plot).copied().flatten();
            state.building = Some(BuildingCard {
                nation: city.nation(),
                name: building.map_or("Building", |b| b.title()).to_string(),
                frame: building_frame(building),
                // The building's own health (`CivilianBuildingHealth` 800).
                health: (city.building_health(plot) / civ::BUILDING_HP).clamp(0.0, 1.0),
                hp: city.building_health(plot).ceil() as i32,
                relation: relation_of_nation(city.nation()),
                screen,
            });
        }
        Some((Target::Turret(i, turret), screen)) => {
            let city = &campaign.cities[i];
            let hp = city.turret_hp.get(turret).copied().unwrap_or(0.0);
            state.building = Some(BuildingCard {
                nation: city.nation(),
                name: "Turret".into(),
                frame: building_frame(None),
                health: (hp / civ::TURRET_HP).clamp(0.0, 1.0),
                hp: hp.max(0.0).ceil() as i32,
                relation: relation_of_nation(city.nation()),
                screen,
            });
        }
        Some((Target::Geyser(g), screen)) => {
            let geyser = &campaign.geysers[g];
            let owner = geyser.owner;
            state.spice = Some(SpiceCard {
                owner,
                name: "Spice Geyser".into(),
                per_minute: if owner.is_some() { super::sim::geyser_per_minute(geyser) } else { 0.0 },
                relation: owner.map_or(0.0, relation_of_nation),
                meter: if owner.is_some() { super::sim::geyser_meter(geyser) } else { 1.0 },
                screen,
            });
        }
        Some((Target::City(i), screen)) => state.city = city_of(i, screen),
        None => {}
    }
    // A clicked city keeps its card while nothing else is hovered (its
    // buttons are below the pointer's way there).
    if state.city.is_none() && state.building.is_none() && state.vehicle.is_none() {
        if let Some(i) = local.pinned_city {
            if let Some(c) = candidates.iter().find(|c| c.target == Target::City(i)) {
                state.city = city_of(i, c.screen);
            }
        }
    }
    // A hovered city that is not the pinned one still shows the pinned card's
    // buttons only on the pinned city (see `city_of`).
    if hud.world != state {
        hud.world = state;
    }
}

/// Window of `control` inside the window `ancestor` (some controls exist in
/// several panels of a layout).
pub fn find_in(ui: &SpuiState, ancestor: u32, control: u32) -> Option<usize> {
    let ancestor = ui.find(ancestor)?;
    ui.find_all(control).into_iter().find(|w| {
        let mut at = ui.layout.window(*w).parent;
        while let Some(p) = at {
            if p == ancestor {
                return true;
            }
            at = ui.layout.window(p).parent;
        }
        false
    })
}

/// Move the root window of an authored layout so that `anchor` (a window of
/// the layout) lands with its top-left corner on `(x, y)`.
fn place_by(ui: &mut SpuiState, root: usize, anchor: usize, x: f32, y: f32) {
    let mut offset = Vec2::ZERO;
    let mut at = Some(anchor);
    while let Some(w) = at {
        if w == root {
            break;
        }
        let a = ui.layout.window(w).area;
        offset += Vec2::new(a[0], a[1]);
        at = ui.layout.window(w).parent;
    }
    let authored = ui.layout.window(root).area;
    let (w, h) = (authored[2] - authored[0], authored[3] - authored[1]);
    ui.set_area(root, [x - offset.x, y - offset.y, x - offset.x + w, y - offset.y + h]);
}

/// Size a bar window to `fraction` of its authored width.
fn set_bar(ui: &mut SpuiState, control: u32, fraction: f32) {
    if let Some(w) = ui.find(control) {
        let mut area = ui.layout.window(w).area;
        let full = area[2] - area[0];
        area[2] = area[0] + full * fraction.clamp(0.0, 1.0);
        ui.set_area(w, area);
    }
}

/// Bind a vehicle / building card (the two layouts share their window ids).
/// What a vehicle / building card shows: anchor, health fraction and number,
/// relationship, type frame, name and the nation's colour for the plate.
pub type UnitCardData<'a> = (Vec2, f32, i32, f32, u32, &'a str, u32);

/// Height of the vehicle card's body that stays empty (its two stat rows are
/// not shown), cut away.
const VEHICLE_TRIM: f32 = 51.0;

fn bind_unit_card(ui: &mut SpuiState, shown: Option<UnitCardData>, type_control: u32, hide: &[u32], trim: f32) {
    let Some(root) = ui.find(id::ROLL_ROOT) else {
        return;
    };
    ui.set_visible(root, shown.is_some());
    let Some((screen, health, hp, relation, frame, name, color)) = shown else {
        return;
    };
    let at = clamp_anchor(screen, Vec2::new(ui.screen[0], ui.screen[1]));
    // The body loses `trim` pixels at the bottom; the anchor moves down by as
    // much so that the pointer's tip stays on the target.
    ui.set_area(root, [at.x, at.y + trim, at.x, at.y + trim]);
    if trim > 0.0 {
        if let Some(w) = ui.find(id::ROLL_BACKDROP) {
            let a = ui.layout.window(w).area;
            ui.set_area(w, [a[0], a[1], a[2], a[3] - trim]);
        }
        if let Some(w) = ui.find(id::ROLL_POINTER) {
            let a = ui.layout.window(w).area;
            ui.set_area(w, [a[0], a[1] - trim, a[2], a[3] - trim]);
        }
    }
    if let Some(w) = ui.find(id::ROLL_NAME) {
        ui.set_text(w, name.to_string());
    }
    if let Some(w) = ui.find(id::ROLL_PLATE) {
        ui.set_tint(w, color);
    }
    if let Some(w) = ui.find(id::ROLL_NUMBER) {
        ui.set_text(w, hp.to_string());
    }
    for control in [id::ROLL_HEAL, id::ROLL_HURT] {
        super::set_visible(ui, control, false);
    }
    set_bar(ui, id::ROLL_HEALTH, health);
    if let Some(w) = ui.find(id::ROLL_FACE) {
        ui.set_frame_grid(w, 5, 1);
        ui.set_frame(w, Some(super::relation_frame(relation)));
    }
    if let Some(w) = ui.find(type_control) {
        // Both strips hold four icons.
        ui.set_frame_grid(w, 4, 1);
        ui.set_frame(w, Some(frame));
    }
    for control in hide {
        super::set_visible(ui, *control, false);
    }
}

/// Fill the card layouts from [`WorldState`].
#[allow(clippy::too_many_arguments)]
pub fn bind_world(
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        let ui = &mut *ui;
        match kind {
            CivHudRoot::CityCard => bind_city_card(ui, hud.world.city.as_ref(), &mut assets, index),
            CivHudRoot::VehicleCard => {
                let card = hud.world.vehicle.as_ref();
                let name = card.map(vehicle_name).unwrap_or_default();
                let shown = card.map(|v| {
                    (v.screen, v.health, v.hp, v.relation, vehicle_type_frame(v.kind), name.as_str(), super::sim::nation_argb(v.nation))
                });
                bind_unit_card(ui, shown, id::VEHICLE_TYPE, &id::VEHICLE_STATS, VEHICLE_TRIM);
            }
            CivHudRoot::BuildingCard => {
                let card = hud.world.building.as_ref();
                let shown = card
                    .map(|b| (b.screen, b.health, b.hp, b.relation, b.frame, b.name.as_str(), super::sim::nation_argb(b.nation)));
                bind_unit_card(ui, shown, id::BUILDING_TYPE, &[], 0.0);
            }
            CivHudRoot::SpiceCard => bind_spice_card(ui, hud.world.spice.as_ref()),
            _ => {}
        }
    }
}

/// The vehicle card's name line: nation and type (the retail card shows the
/// creation's name; the rewrite's vehicles have none).
pub fn vehicle_name(v: &VehicleCard) -> String {
    let nation = if v.nation == HOME_INDEX { "Your".to_string() } else { civ::city_label(v.nation) };
    format!("{nation} {} {}", v.kind.title(), v.domain.title())
}

fn bind_spice_card(ui: &mut SpuiState, card: Option<&SpiceCard>) {
    let Some(root) = ui.find(id::SPICE_ROOT) else {
        return;
    };
    ui.set_visible(root, card.is_some());
    let Some(card) = card else {
        return;
    };
    let authored = ui.layout.window(root).area;
    let (w, h) = (authored[2] - authored[0], authored[3] - authored[1]);
    let screen = Vec2::new(ui.screen[0], ui.screen[1]);
    let x = (card.screen.x + 26.0).clamp(4.0, (screen.x - w - 4.0).max(4.0));
    let y = (card.screen.y - h * 0.5).clamp(4.0, (screen.y - h - 4.0).max(4.0));
    ui.set_area(root, [x, y, x + w, y + h]);
    if let Some(w) = ui.find(id::SPICE_NAME) {
        ui.set_text(w, card.name.clone());
    }
    if let Some(w) = ui.find(id::SPICE_RATE) {
        ui.set_text(w, format!("{:.0}", card.per_minute));
    }
    if let Some(w) = ui.find(id::SPICE_FACE) {
        ui.set_frame_grid(w, 5, 1);
        ui.set_frame(w, Some(super::relation_frame(card.relation)));
        ui.set_visible(w, card.owner.is_some());
    }
    if let Some(w) = ui.find(id::SPICE_METER) {
        ui.set_frame_grid(w, 16, 1);
        ui.set_frame(w, Some(spice_frame(card.meter)));
    }
}

fn bind_city_card(ui: &mut SpuiState, card: Option<&CityCard>, assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex) {
    let Some(root) = ui.find(id::CITY_ROOT) else {
        return;
    };
    ui.set_visible(root, card.is_some());
    let Some(card) = card else {
        return;
    };
    if let Some(pointer) = ui.find(id::CITY_POINTER) {
        let at = clamp_card(card.screen, Vec2::new(ui.screen[0], ui.screen[1]));
        place_by(ui, root, pointer, at.x - 8.0, at.y - 12.0);
    }
    if let Some(w) = ui.find(id::CITY_NAME) {
        ui.set_text(w, card.name.clone());
    }
    // The name sits on a plate in the nation's colour.
    for control in id::CITY_PLATES {
        for w in ui.find_all(control) {
            ui.set_tint(w, card.color);
        }
    }
    if let Some(w) = ui.find(id::CITY_FACE) {
        ui.set_frame_grid(w, 5, 1);
        ui.set_frame(w, Some(super::relation_frame(card.relation)));
    }
    // Kind icons: economic, religious, military; one is shown.
    let shown = match card.kind {
        Approach::Economic => 0,
        Approach::Religious => 1,
        Approach::Military => 2,
    };
    for (i, control) in id::CITY_KINDS.iter().enumerate() {
        super::set_visible(ui, *control, i == shown);
    }
    if let Some(w) = ui.find(id::CITY_POPULATION) {
        ui.set_text(w, card.population.to_string());
    }
    if let Some(w) = ui.find(id::CITY_INCOME) {
        ui.set_text(w, format!("{:.0}/min", card.per_minute));
        // Text takes its colour from the window's shade (a tint only reaches drawables).
        ui.set_shade(w, card.income_color);
    }
    // Mood: three-frame icons (dark, red unhappy face, green happy face);
    // defence: two-frame shields (dark, gold).
    for (i, control) in id::CITY_MOOD.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_frame_grid(w, 3, 1);
            ui.set_frame(w, Some(mood_frame_delta(i as u32, card.mood)));
        }
    }
    for (i, control) in id::CITY_DEFENCE.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_frame_grid(w, 2, 1);
            ui.set_frame(w, Some(u32::from((i as u32) < card.defence)));
        }
    }
    // Buttons: City Planner and the three purchases, only on a clicked city of
    // the player's.
    let show = card.pinned;
    super::set_visible(ui, id::CITY_PLANNER, show);
    let retail: [(u32, u32); 3] = [(0x064A_7064, 1000), (0x064A_705D, 1500), (0x064A_7057, 3000)];
    for (i, control) in id::CITY_BUY.iter().enumerate() {
        let Some(w) = ui.find(*control) else {
            continue;
        };
        ui.set_visible(w, show);
        if let (true, Some(option)) = (show, card.buy.get(i)) {
            ui.set_enabled(w, option.refused.is_none());
            let (text_id, price) = retail[i];
            let text = super::ui_text(assets, index, "tooltips", text_id);
            ui.set_tooltip(w, Some(buy_tooltip(&text, price, option)));
        }
    }
}

/// Keep the city card (which hangs above the hall and has buttons below) on
/// screen.
fn clamp_card(anchor: Vec2, screen: Vec2) -> Vec2 {
    Vec2::new(anchor.x.clamp(70.0, (screen.x - 80.0).max(70.0)), anchor.y.clamp(150.0, (screen.y - 130.0).max(150.0)))
}

/// Clicks on the city card: City Planner opens the retail planner screen
/// (`civ_planner`) on that city and the buy buttons build the vehicle in that
/// city (the same call the G / H / J keys make for the city nearest the
/// camera). The Civ creator stays on its key (F3).
#[allow(clippy::too_many_arguments)]
pub fn card_actions(
    hud: &CivHudState,
    _local: &mut CivHudLocal,
    roots: &mut Query<(&mut SpuiState, &CivHudRoot)>,
    planner: &mut crate::civ_planner::CivPlanner,
    run: &mut RunProgress,
    spawns: &mut UnitSpawns,
    units: &Query<&CivUnit>,
) {
    let Some(card) = hud.world.city.as_ref() else {
        // Drain stray clicks so that they do not fire later.
        for (mut ui, kind) in roots.iter_mut() {
            if *kind == CivHudRoot::CityCard {
                ui.take_clicks();
            }
        }
        return;
    };
    for (mut ui, kind) in roots.iter_mut() {
        if *kind != CivHudRoot::CityCard {
            continue;
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            if control == id::CITY_PLANNER {
                if card.pinned {
                    planner.request = Some(card.index);
                }
            } else if let Some(slot) = id::CITY_BUY.iter().position(|c| *c == control) {
                if !card.pinned {
                    continue;
                }
                let domain = [VehicleDomain::Land, VehicleDomain::Sea, VehicleDomain::Air][slot];
                let kind = run.civ_campaign.cities[card.index].kind;
                let live = units.iter().filter(|u| u.nation == HOME_INDEX && u.kind == kind).count()
                    + spawns.0.iter().filter(|u| u.nation == HOME_INDEX && u.kind == kind).count();
                let mut spice = super::sim::money(run);
                match crate::civ_rts::try_produce_domain(&mut run.civ_campaign, &mut spice, live, card.index, domain) {
                    Ok(unit) => {
                        run.civ_campaign.message = format!(
                            "{} built a {} {} vehicle ({}/{})",
                            civ::city_label(card.index),
                            unit.kind.title().to_lowercase(),
                            unit.domain().title(),
                            live + 1,
                            run.civ_campaign.vehicle_cap(HOME_INDEX)
                        );
                        spawns.0.push(unit);
                        run.request_save();
                    }
                    Err(message) => run.civ_campaign.message = message,
                }
                super::sim::set_money(run, spice);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Minimap
// ---------------------------------------------------------------------------

/// Size of the minimap picture in pixels (the holder window is 231 x 102 at
/// the authored size).
pub const MAP_W: u32 = 231;
pub const MAP_H: u32 = 102;
/// Seconds between repaints.
const MAP_PERIOD: f32 = 0.2;
/// Frames before the picture is shown (the texture is prepared by the render
/// world asynchronously).
const SETTLE_FRAMES: u32 = 4;

/// Minimap picture, its UI node and when it was last painted.
#[derive(Resource, Default)]
pub struct MinimapState {
    image: Option<Handle<Image>>,
    node: Option<Entity>,
    /// Frames the picture has existed.
    seen: u32,
    since_paint: f32,
    terrain: Option<Vec<[u8; 3]>>,
    terrain_key: Option<(u64, u32)>,
    /// The map's projection (the planet map, or the plane).
    frame: Option<crate::civ_planet::MapFrame>,
}

/// Plain UI node showing the minimap picture.
#[derive(Component)]
pub struct MinimapNode;

/// World units across the picture's height without a planet (the plane; the
/// planet map shows the whole planet, `civ_planet::MapFrame`).
pub const MAP_SPAN: f32 = crate::civ_planet::FLAT_MAP_SPAN;

/// Picture size as a vector.
fn map_size() -> Vec2 {
    Vec2::new(MAP_W as f32, MAP_H as f32)
}

/// Pixel of a world point on the flat minimap (x east, z south; north up).
#[cfg_attr(not(test), allow(dead_code))]
pub fn map_pixel(world: Vec2) -> Vec2 {
    let scale = MAP_H as f32 / MAP_SPAN;
    Vec2::new(MAP_W as f32 * 0.5 + world.x * scale, MAP_H as f32 * 0.5 + world.y * scale)
}

/// World point (x, z) under a minimap pixel.
pub fn map_world(pixel: Vec2) -> Vec2 {
    let scale = MAP_H as f32 / MAP_SPAN;
    Vec2::new((pixel.x - MAP_W as f32 * 0.5) / scale, (pixel.y - MAP_H as f32 * 0.5) / scale)
}

fn color_u8(c: Color) -> [u8; 3] {
    let s = c.to_srgba();
    [(s.red * 255.0) as u8, (s.green * 255.0) as u8, (s.blue * 255.0) as u8]
}

/// Terrain colours of the picture: the planet map (`civ_planet`), else the
/// island's sea, shore and land shaded by height.
fn paint_map_terrain(
    frame: &crate::civ_planet::MapFrame,
    surface: Option<&crate::terrain::LandSurface>,
    palette: Option<&crate::world_catalog::PlanetPalette>,
    seed: u64,
) -> Vec<[u8; 3]> {
    match surface.and_then(|s| s.planet()) {
        Some(planet) => crate::civ_planet::paint_globe_map(frame, planet, palette, MAP_W, MAP_H),
        None => paint_terrain(surface, seed),
    }
}

fn paint_terrain(surface: Option<&crate::terrain::LandSurface>, seed: u64) -> Vec<[u8; 3]> {
    let extent = surface.map_or(48.0, |s| s.extent());
    let phase = (seed ^ (seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
    let mut out = Vec::with_capacity((MAP_W * MAP_H) as usize);
    for y in 0..MAP_H {
        for x in 0..MAP_W {
            let p = map_world(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            let depth = crate::terrain::coast_depth(p, extent, phase);
            out.push(if depth < -0.25 {
                // Deeper water further out.
                let t = (-depth / 12.0).clamp(0.0, 1.0);
                [(10.0 - 4.0 * t) as u8, (52.0 - 20.0 * t) as u8, (88.0 - 28.0 * t) as u8]
            } else if depth < 1.2 {
                [176, 164, 112]
            } else {
                let h = surface.map_or(0.0, |s| s.height(p.x, p.y));
                let rise = ((h + 0.6) / 3.0).clamp(0.0, 1.0);
                [(46.0 + 70.0 * rise) as u8, (96.0 + 50.0 * rise) as u8, (44.0 + 26.0 * rise) as u8]
            });
        }
    }
    out
}

fn put(data: &mut [u8], x: i32, y: i32, rgb: [u8; 3]) {
    if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
        return;
    }
    let at = ((y as u32 * MAP_W + x as u32) * 4) as usize;
    data[at..at + 3].copy_from_slice(&rgb);
    data[at + 3] = 255;
}

fn square(data: &mut [u8], c: Vec2, half: i32, rgb: [u8; 3]) {
    for dy in -half..=half {
        for dx in -half..=half {
            put(data, c.x as i32 + dx, c.y as i32 + dy, rgb);
        }
    }
}

/// Repaint the minimap picture: terrain, spice geysers, vehicles, cities and
/// the camera's view.
#[allow(clippy::too_many_arguments)]
pub fn paint_minimap(
    mut commands: Commands,
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    active: Res<CivHudActive>,
    local: Res<CivHudLocal>,
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    theme: Option<Res<crate::world_presentation::LandTheme>>,
    camera: Option<Res<crate::civ_controls::CivCamera>>,
    mut state: ResMut<MinimapState>,
    mut images: ResMut<Assets<Image>>,
    units: Query<(&CivUnit, &Transform, Has<Selected>)>,
    focus: Query<&Transform, With<civ::CivCommander>>,
) {
    if *stage.get() != GameStage::Civilization || !active.0 {
        for e in [state.node.take()].into_iter().flatten() {
            if let Ok(mut entity) = commands.get_entity(e) {
                entity.despawn();
            }
        }
        state.seen = 0;
        return;
    }
    let image = state
        .image
        .get_or_insert_with(|| {
            images.add(Image::new(
                Extent3d { width: MAP_W, height: MAP_H, depth_or_array_layers: 1 },
                TextureDimension::D2,
                vec![0; (MAP_W * MAP_H * 4) as usize],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ))
        })
        .clone();
    if state.node.is_none() {
        state.node = Some(
            commands
                .spawn((
                    MinimapNode,
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode::new(image.clone()),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    GlobalZIndex(101),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    state.since_paint += time.delta_secs();
    if state.since_paint < MAP_PERIOD && state.seen > 0 {
        state.seen = state.seen.saturating_add(1);
        return;
    }
    state.since_paint = 0.0;
    state.seen = state.seen.saturating_add(1);
    let surface = surface
        .as_deref()
        .filter(|s| s.stage == GameStage::Civilization);
    let on_planet = surface.is_some_and(|s| s.planet().is_some());
    let key = (
        run.species_world.seed,
        surface.map_or(0, |s| if on_planet { 1 } else { s.extent().to_bits() }),
    );
    if state.terrain_key != Some(key) || state.terrain.is_none() {
        let frame = crate::civ_planet::MapFrame::new(surface.and_then(|s| s.planet()));
        state.terrain = Some(paint_map_terrain(
            &frame,
            surface,
            theme.as_deref().map(|t| &t.palette),
            run.species_world.seed,
        ));
        state.terrain_key = Some(key);
        state.frame = Some(frame);
    }
    let frame = state.frame.unwrap_or_else(|| crate::civ_planet::MapFrame::new(None));
    let size = map_size();
    let pixel = |p: Vec3| frame.pixel(p, size);
    let Some(terrain) = state.terrain.as_ref() else {
        return;
    };
    let Some(mut picture) = images.get_mut(&image) else {
        return;
    };
    let mut data = vec![255u8; (MAP_W * MAP_H * 4) as usize];
    for (i, rgb) in terrain.iter().enumerate() {
        data[i * 4..i * 4 + 3].copy_from_slice(rgb);
    }
    let campaign = &run.civ_campaign;
    // Spice geysers: a pink dot, the owner's colour once claimed.
    for g in 0..civ::GEYSER_COUNT {
        let p = pixel(campaign.geyser_at(g));
        let rgb = match campaign.geysers.get(g).and_then(|s| s.owner) {
            Some(owner) => color_u8(civ::nation_color(owner)),
            None => [255, 96, 176],
        };
        square(&mut data, p, 1, rgb);
    }
    // Vehicles: small squares in their nation's colour; selected ones bigger.
    for (unit, t, selected) in &units {
        if unit.hp <= 0.0 {
            continue;
        }
        let p = pixel(t.translation);
        let rgb = color_u8(civ::nation_color(unit.nation));
        if selected {
            square(&mut data, p, 2, [255, 255, 255]);
        }
        square(&mut data, p, if selected { 1 } else { 0 }, rgb);
    }
    // Cities: bigger squares with a dark border; the capital has a white one.
    for city in &campaign.cities {
        let p = pixel(city.center());
        let border = if city.index == HOME_INDEX { [255, 255, 255] } else { [10, 14, 20] };
        square(&mut data, p, 3, border);
        square(&mut data, p, 2, color_u8(civ::nation_color(city.nation())));
    }
    // The camera's view: a thin white frame around the focus, sized by the
    // camera's distance (ADAPTED: about the ground the view spans).
    if let Ok(t) = focus.single() {
        let c = pixel(t.translation);
        let reach = camera.as_deref().map_or(221.8, |c| c.world_distance()) * 1.1;
        let px = (reach * frame.pixels_per_unit(size)).clamp(3.0, 40.0);
        let half = Vec2::new(px * 1.5, px);
        for k in 0..=(half.x * 2.0) as i32 {
            for y in [c.y - half.y, c.y + half.y] {
                put(&mut data, (c.x - half.x) as i32 + k, y as i32, [235, 235, 235]);
            }
        }
        for k in 0..=(half.y * 2.0) as i32 {
            for x in [c.x - half.x, c.x + half.x] {
                put(&mut data, x as i32, (c.y - half.y) as i32 + k, [235, 235, 235]);
            }
        }
    }
    let _ = local;
    picture.data = Some(data);
}

/// Place the minimap node over the holder window of the HUD and show it while
/// the minimap is open.
pub fn place_minimap(
    state: Res<MinimapState>,
    local: Res<CivHudLocal>,
    hud: Res<CivHudState>,
    roots: Query<(&SpuiState, &CivHudRoot, Option<&Visibility>), Without<MinimapNode>>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<MinimapNode>>,
) {
    let Some(node) = state.node else {
        return;
    };
    let Ok((mut n, mut visibility)) = nodes.get_mut(node) else {
        return;
    };
    let window = roots
        .iter()
        .find(|(_, kind, _)| **kind == CivHudRoot::Main)
        // The picture goes with its layout while the options menu hides the HUD layers
        // (`front_end::pause::layers`).
        .filter(|(ui, _, shown)| ui.ready && shown.is_none_or(|v| *v != Visibility::Hidden))
        .and_then(|(ui, _, _)| ui.find(hud_id::MAP_HOLDER).map(|w| (ui.rect(w), ui.screen)));
    let want = match window {
        Some((rect, screen)) if state.seen >= SETTLE_FRAMES && local.minimap_open && hud.comm.is_none() => {
            // Percentages of the full-screen root keep the node on the window
            // at any resolution or UI scale.
            let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
            n.left = pct(rect.x1, screen[0]);
            n.top = pct(rect.y1, screen[1]);
            n.width = pct(rect.width(), screen[0]);
            n.height = pct(rect.height(), screen[1]);
            Visibility::Inherited
        }
        _ => Visibility::Hidden,
    };
    if *visibility != want {
        *visibility = want;
    }
}

/// Layout element number of the group that holds the minimap panel.
const MAP_GROUP_ELEMENT: usize = 722;

/// Planet name of the minimap header when the install's name generator is not
/// available (`CivHudState::planet_name` is the home planet's generated name, the one
/// the Creature, Tribal and Space HUDs show).
pub const PLANET_NAME: &str = "Homeworld";

/// The planet minimap's frame, header and the nations' communication
/// buttons; the minimap picture itself is the node of [`paint_minimap`].
pub fn bind_minimap_frame(ui: &mut SpuiState, hud: &CivHudState, local: &CivHudLocal) {
    let comm_open = hud.comm.is_some();
    // The whole minimap group (it has no control id) goes while the
    // communication screen is open.
    if let Some(group) = ui.layout.windows.iter().find(|w| w.element == MAP_GROUP_ELEMENT).map(|w| w.id) {
        ui.set_visible(group, !comm_open);
    }
    let open = local.minimap_open && !comm_open;
    if let Some(w) = ui.find(hud_id::MAP_NAME) {
        ui.set_text(w, if hud.planet_name.is_empty() { PLANET_NAME } else { hud.planet_name.as_str() });
    }
    if let Some(w) = ui.find(hud_id::MAP_HIDE) {
        ui.set_selected(w, !local.minimap_open);
    }
    // The planet map's frame stays while the map is hidden; the picture and
    // the buttons beside it go (ADAPTED: the retail layout keeps the hide
    // button inside the frame, so the frame stays to let it be shown again).
    for control in [hud_id::MAP_FRAME, hud_id::MAP_NAME_GROUP] {
        super::set_visible(ui, control, !comm_open);
    }
    for control in [hud_id::MAP_HOLDER] {
        super::set_visible(ui, control, open);
    }
    let rivals = hud.rivals.len().min(hud_id::COMM_BUTTONS.len());
    let panel_shown = open && rivals > 0;
    let strip_shown = !local.minimap_open && !comm_open && rivals > 0;
    super::set_visible(ui, hud_id::COMM_PANEL, panel_shown);
    super::set_visible(ui, hud_id::COMM_STRIP, strip_shown);
    for (panel, shown) in [(hud_id::COMM_PANEL, panel_shown), (hud_id::COMM_STRIP, strip_shown)] {
        for (slot, control) in hud_id::COMM_BUTTONS.iter().enumerate() {
            let Some(button) = find_in(ui, panel, *control) else {
                continue;
            };
            let rival = hud.rivals.get(slot);
            ui.set_visible(button, shown && rival.is_some());
            // The slot's background (0497974B..E: one per button) is the
            // empty slot (a muted speaker), shown while no nation fills it.
            if let Some(back) = find_in(ui, panel, 0x0497_974B + slot as u32) {
                ui.set_visible(back, shown && rival.is_none());
            }
            let Some(rival) = rival else {
                continue;
            };
            let child = |ui: &SpuiState, control: u32| {
                ui.layout
                    .window(button)
                    .children
                    .iter()
                    .copied()
                    .find(|c| ui.layout.window(*c).control_id == control)
            };
            if let Some(face) = child(ui, hud_id::COMM_FACE) {
                ui.set_frame(face, Some(super::relation_frame(rival.relation)));
            }
            if let Some(kind) = child(ui, hud_id::COMM_EMOTION) {
                ui.set_frame(kind, Some(kind_frame(rival.kind)));
            }
            ui.set_tooltip(button, Some(comm_tooltip(rival)));
        }
    }
}

/// Tooltip of a communication button: the nation and how it feels.
pub fn comm_tooltip(rival: &super::Rival) -> String {
    let mood = match super::relation_frame(rival.relation) {
        0 => "At war",
        1 => "Unfriendly",
        2 => "Neutral",
        3 => "Friendly",
        _ => "Allied",
    };
    format!("{} {}\n{mood}", rival.name, rival.kind.title())
}

/// Pointer over the minimap picture and its clicks: a click moves the
/// camera's focus there (the retail minimap does the same). Runs before the
/// cards are picked so that the click is not also a selection.
pub fn minimap_pointer(
    mut pointer: ResMut<SpuiPointer>,
    mouse: Res<ButtonInput<MouseButton>>,
    local: Res<CivHudLocal>,
    roots: Query<(&SpuiState, &CivHudRoot)>,
    mut hud: ResMut<CivHudState>,
    state: Res<MinimapState>,
    run: Res<RunProgress>,
    mut focus: Query<&mut Transform, With<civ::CivCommander>>,
) {
    let holder = roots
        .iter()
        .find(|(_, kind)| **kind == CivHudRoot::Main)
        .filter(|(ui, _)| ui.ready)
        .and_then(|(ui, _)| ui.find(hud_id::MAP_HOLDER).filter(|w| ui.visible(*w)).map(|w| ui.rect(w)));
    let over = local.minimap_open
        && hud.comm.is_none()
        && holder.is_some_and(|r| pointer.position.is_some_and(|p| r.contains(p.x, p.y)));
    if hud.world.over_minimap != over {
        hud.world.over_minimap = over;
    }
    // The click belongs to the minimap: the vehicle selection ignores it.
    pointer.over_ui |= over;
    if over && (mouse.just_pressed(MouseButton::Left) || mouse.pressed(MouseButton::Left)) {
        if let (Some(rect), Some(p)) = (holder, pointer.position) {
            let pixel = Vec2::new(
                (p.x - rect.x1) / rect.width().max(1.0) * MAP_W as f32,
                (p.y - rect.y1) / rect.height().max(1.0) * MAP_H as f32,
            );
            let frame = state.frame.unwrap_or_else(|| crate::civ_planet::MapFrame::new(None));
            let world = frame.world(pixel, map_size());
            if let Ok(mut t) = focus.single_mut() {
                crate::civ_controls::move_focus(&mut t, world, run.civ_campaign.shape());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::solve::RootPolicy;

    fn cand(target: Target, x: f32, y: f32, r: f32) -> Candidate {
        Candidate { target, screen: Vec2::new(x, y), radius: r }
    }

    #[test]
    fn the_pointer_picks_the_front_most_target_it_touches() {
        let all = [
            cand(Target::City(1), 100.0, 100.0, 60.0),
            cand(Target::Plot(1, 2), 110.0, 100.0, 20.0),
            cand(Target::Vehicle(7), 140.0, 100.0, 26.0),
            cand(Target::Geyser(3), 400.0, 300.0, 30.0),
        ];
        // On the city only: the city. Over a plot: the building. Over a vehicle: the vehicle.
        assert_eq!(pick(&all, Vec2::new(60.0, 100.0)).map(|c| c.target), Some(Target::City(1)));
        assert_eq!(pick(&all, Vec2::new(112.0, 100.0)).map(|c| c.target), Some(Target::Plot(1, 2)));
        assert_eq!(pick(&all, Vec2::new(138.0, 100.0)).map(|c| c.target), Some(Target::Vehicle(7)));
        assert_eq!(pick(&all, Vec2::new(405.0, 305.0)).map(|c| c.target), Some(Target::Geyser(3)));
        assert_eq!(pick(&all, Vec2::new(300.0, 10.0)).map(|c| c.target), None);
        // The nearest of two equals wins.
        let two = [cand(Target::Vehicle(1), 0.0, 0.0, 30.0), cand(Target::Vehicle(2), 20.0, 0.0, 30.0)];
        assert_eq!(pick(&two, Vec2::new(15.0, 0.0)).map(|c| c.target), Some(Target::Vehicle(2)));
    }

    #[test]
    fn cards_stay_on_screen() {
        let screen = Vec2::new(1280.0, 900.0);
        let c = clamp_anchor(Vec2::new(-50.0, 10.0), screen);
        assert!(c.x >= CARD_LEFT && c.y >= CARD_UP);
        let c = clamp_anchor(Vec2::new(5000.0, 5000.0), screen);
        assert!(c.x <= screen.x - CARD_RIGHT && c.y <= screen.y);
        assert_eq!(clamp_anchor(Vec2::new(640.0, 450.0), screen), Vec2::new(640.0, 450.0));
        let p = clamp_card(Vec2::new(0.0, 0.0), screen);
        assert!(p.x >= 70.0 && p.y >= 150.0);
    }

    #[test]
    fn the_minimap_maps_the_island_into_its_picture_and_back() {
        let origin = map_pixel(Vec2::ZERO);
        assert_eq!(origin, Vec2::new(MAP_W as f32 * 0.5, MAP_H as f32 * 0.5));
        for p in [Vec2::new(10.0, -20.0), Vec2::new(-48.0, 48.0), Vec2::new(0.5, 0.25)] {
            let back = map_world(map_pixel(p));
            assert!((back - p).length() < 1e-3, "{p:?} -> {back:?}");
        }
        // North is up: more negative z is higher in the picture.
        assert!(map_pixel(Vec2::new(0.0, -10.0)).y < map_pixel(Vec2::new(0.0, 10.0)).y);
        // The cities' rings fit the picture's height.
        assert!(map_pixel(Vec2::new(0.0, 400.0)).y < MAP_H as f32);
        assert!(map_pixel(Vec2::new(0.0, -400.0)).y > 0.0);
        // Every city is inside the picture.
        for i in 0..civ::CITY_COUNT {
            let p = map_pixel(civ::city_position(i).xz());
            assert!(p.x > 0.0 && p.x < MAP_W as f32 && p.y > 0.0 && p.y < MAP_H as f32, "city {i}: {p:?}");
        }
    }

    #[test]
    fn the_terrain_picture_has_sea_and_land() {
        let pixels = paint_terrain(None, 7);
        assert_eq!(pixels.len(), (MAP_W * MAP_H) as usize);
        let at = |p: Vec2| {
            let px = map_pixel(p);
            pixels[(px.y as u32 * MAP_W + px.x as u32) as usize]
        };
        let land = at(Vec2::ZERO);
        let sea = pixels[0];
        assert!(land[1] > land[2], "land is green: {land:?}");
        assert!(sea[2] > sea[1] && sea[2] > sea[0], "sea is blue: {sea:?}");
    }

    #[test]
    fn dots_are_clipped_to_the_picture() {
        let mut data = vec![0u8; (MAP_W * MAP_H * 4) as usize];
        square(&mut data, Vec2::new(-5.0, -5.0), 3, [1, 2, 3]);
        square(&mut data, Vec2::new(MAP_W as f32 + 5.0, 50.0), 3, [1, 2, 3]);
        assert!(data.iter().all(|b| *b == 0), "nothing painted outside");
        square(&mut data, Vec2::new(10.0, 10.0), 1, [9, 8, 7]);
        let at = ((10 * MAP_W + 10) * 4) as usize;
        assert_eq!(&data[at..at + 4], &[9, 8, 7, 255]);
    }

    #[test]
    fn a_city_card_counts_what_the_campaign_has() {
        let mut city = civ::CityState::new(0);
        // The hall and its linked houses, 15 rooms each.
        assert_eq!(population(&city), ROOMS_PER_HOUSE * city.housing());
        assert!(city.housing() >= 1);
        let bare = {
            let mut c = city.clone();
            c.plots = vec![None; c.plots.len()];
            c
        };
        assert_eq!(population(&bare), ROOMS_PER_HOUSE, "the hall alone");
        // The mood orbs follow happy minus unhappy of the link model.
        let model = city.model();
        assert_eq!(mood_delta(&city), model.happy - model.unhappy);
        city.plots = vec![None; city.plots.len()];
        let fresh = civ::CityState::new(1);
        let full = defence_orbs(&fresh);
        assert!(full >= 3, "a fresh fortified city is well defended: {full}");
        let mut broken = fresh.clone();
        broken.hp = 0.0;
        broken.turret_hp = vec![0.0; broken.turret_hp.len()];
        assert_eq!(defence_orbs(&broken), 0);
        assert!(defence_orbs(&broken) < full);
    }

    #[test]
    fn frames_follow_the_art_strips() {
        // Vehicle type strip: eye, pot, swords; kind strip: eye, swirl, swords.
        assert_eq!([Approach::Religious, Approach::Economic, Approach::Military].map(vehicle_type_frame), [0, 1, 2]);
        assert_eq!([Approach::Religious, Approach::Economic, Approach::Military].map(kind_frame), [0, 1, 2]);
        assert_eq!(building_frame(None), 0);
        assert_eq!(building_frame(Some(civ::Building::House)), 1);
        assert_eq!(building_frame(Some(civ::Building::Entertainment)), 2);
        assert_eq!(building_frame(Some(civ::Building::Factory)), 3);
        assert_eq!(spice_frame(0.0), 0);
        assert_eq!(spice_frame(1.0), 15);
        assert_eq!(spice_frame(2.0), 15);
        assert_eq!(spice_frame(0.5), 8);
    }

    #[test]
    fn buy_options_report_why_a_vehicle_is_refused() {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        // The retail start money (1000) buys a land vehicle (1000) but not a sea
        // one (1500); aircraft need the tech level.
        let options = buy_options(&campaign, 1000.0, 0, 0);
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].domain, VehicleDomain::Land);
        assert_eq!(options[0].cost, civ::LAND_VEHICLE_COST);
        assert!(options[0].refused.is_none(), "{:?}", options[0]);
        assert!(options[1].refused.as_deref().is_some_and(|m| m.contains("costs")), "{:?}", options[1]);
        assert!(options[2].refused.as_deref().is_some_and(|m| m.contains("Aircraft")), "{:?}", options[2]);
        // With 2000 the sea vehicle is allowed (sea vehicles need one city).
        let richer = buy_options(&campaign, 2000.0, 0, 0);
        assert!(richer[1].refused.is_none(), "{:?}", richer[1]);
        assert_eq!(richer[1].cost, civ::SEA_VEHICLE_COST);
        // Too poor.
        let poor = buy_options(&campaign, 1.0, 0, 0);
        assert!(poor[0].refused.as_deref().is_some_and(|m| m.contains("costs")), "{:?}", poor[0]);
        // The dry run did not spend anything.
        assert_eq!(campaign.units.len(), 0);
        // A foreign city cannot be bought in.
        let foreign = buy_options(&campaign, 1000.0, 0, 2);
        assert!(foreign[0].refused.is_some());
    }

    #[test]
    fn buy_tooltips_keep_the_retail_words_with_the_rewrites_price() {
        let option = BuyOption { domain: VehicleDomain::Land, cost: 60.0, refused: None };
        assert_eq!(buy_tooltip("Buy Land Vehicle ($1000)", 1000, &option), "Buy Land Vehicle ($60)");
        let refused = BuyOption { refused: Some("no room".into()), ..option };
        assert_eq!(buy_tooltip("Buy Land Vehicle ($1000)", 1000, &refused), "Buy Land Vehicle ($60)\nno room");
    }

    #[test]
    fn comm_tooltips_name_the_nation_and_its_mood() {
        let mut rival = super::super::Rival {
            nation: 2,
            name: "C3".into(),
            kind: Approach::Religious,
            at_war: false,
            trade: false,
            relation: 0.0,
        };
        assert_eq!(comm_tooltip(&rival), "C3 Religious\nNeutral");
        rival.relation = -1.0;
        assert!(comm_tooltip(&rival).ends_with("At war"));
        rival.relation = 1.0;
        assert!(comm_tooltip(&rival).ends_with("Allied"));
    }

    #[test]
    fn vehicle_cards_name_the_nation_and_the_type() {
        let mut v = VehicleCard {
            nation: 0,
            kind: Approach::Military,
            domain: VehicleDomain::Land,
            health: 1.0,
            hp: 100,
            relation: 1.0,
            screen: Vec2::ZERO,
        };
        assert_eq!(vehicle_name(&v), "Your Military land");
        v.nation = 3;
        v.kind = Approach::Economic;
        v.domain = VehicleDomain::Air;
        assert_eq!(vehicle_name(&v), "C4 Economic air");
    }

    // ---- installed layouts ---------------------------------------------

    fn installed() -> Option<(crate::asset_catalog::ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_card_layouts_have_every_window_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let city = assets.layout(&index, CITY_LAYOUT).expect("RolloverCivCity");
        for control in [id::CITY_ROOT, id::CITY_POINTER, id::CITY_NAME, id::CITY_FACE, id::CITY_POPULATION, id::CITY_INCOME, id::CITY_PLANNER]
            .iter()
            .chain(&id::CITY_KINDS)
            .chain(&id::CITY_MOOD)
            .chain(&id::CITY_DEFENCE)
            .chain(&id::CITY_BUY)
        {
            assert!(city.find(*control).is_some(), "city card window {control:08X}");
        }
        for (name, extra) in [(VEHICLE_LAYOUT, id::VEHICLE_TYPE), (BUILDING_LAYOUT, id::BUILDING_TYPE)] {
            let layout = assets.layout(&index, name).unwrap_or_else(|| panic!("{name}"));
            for control in [
                id::ROLL_ROOT, id::ROLL_PLATE, id::ROLL_BAR, id::ROLL_HEALTH, id::ROLL_HEAL, id::ROLL_HURT, id::ROLL_NUMBER,
                id::ROLL_FACE, extra,
            ] {
                assert!(layout.find(control).is_some(), "{name} window {control:08X}");
            }
        }
        // Only the building card has a name line (the vehicle's plate shows its type icon).
        let building = assets.layout(&index, BUILDING_LAYOUT).unwrap();
        assert!(building.find(id::ROLL_NAME).is_some());
        assert!(assets.layout(&index, VEHICLE_LAYOUT).unwrap().find(id::ROLL_NAME).is_none());
        let vehicle = assets.layout(&index, VEHICLE_LAYOUT).unwrap();
        for control in id::VEHICLE_STATS {
            assert!(vehicle.find(control).is_some(), "vehicle stat window {control:08X}");
        }
        let spice = assets.layout(&index, SPICE_LAYOUT).expect("CivSpiceMeter");
        for control in [id::SPICE_ROOT, id::SPICE_NAME, id::SPICE_FACE, id::SPICE_RATE, id::SPICE_METER] {
            assert!(spice.find(control).is_some(), "spice card window {control:08X}");
        }
    }

    #[test]
    fn the_city_card_shows_the_published_city() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, CITY_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        let mut card = CityCard {
            index: 0,
            name: "C1".into(),
            kind: Approach::Military,
            owned: true,
            color: 0xFF2A_C8AA,
            relation: 0.5,
            population: 50,
            mood: 3,
            defence: 4,
            per_minute: 120.0,
            income_color: 0xFF00_FFFF,
            pinned: false,
            buy: Vec::new(),
            screen: Vec2::new(640.0, 450.0),
        };
        bind_city_card(&mut ui, Some(&card), &mut assets, &index);
        let w = |ui: &SpuiState, c: u32| ui.find(c).unwrap();
        assert_eq!(ui.captions[w(&ui, id::CITY_NAME)].as_deref(), Some("C1"));
        assert_eq!(ui.captions[w(&ui, id::CITY_POPULATION)].as_deref(), Some("50"));
        assert_eq!(ui.captions[w(&ui, id::CITY_INCOME)].as_deref(), Some("120/min"));
        assert_eq!(ui.shades[w(&ui, id::CITY_INCOME)], 0xFF00_FFFF, "the income text is cyan");
        assert_eq!(ui.frame_overrides[w(&ui, id::CITY_FACE)], Some(3));
        // Military: only the swords icon.
        assert!(ui.visible(w(&ui, id::CITY_KINDS[2])) && !ui.visible(w(&ui, id::CITY_KINDS[0])));
        // Three of five mood orbs and four of five defence orbs are lit.
        let lit = |ui: &SpuiState, controls: &[u32; 5]| controls.iter().filter(|c| ui.frame_overrides[w(ui, **c)] != Some(0)).count();
        assert_eq!(lit(&ui, &id::CITY_MOOD), 3);
        assert_eq!(lit(&ui, &id::CITY_DEFENCE), 4);
        assert_eq!(ui.frame_overrides[w(&ui, id::CITY_MOOD[0])], Some(2), "three lit: green faces");
        assert_eq!(mood_frame_delta(0, 2), 2, "a positive delta: green faces");
        assert_eq!(mood_frame_delta(1, 2), 2);
        assert_eq!(mood_frame_delta(2, 2), 0);
        assert_eq!(mood_frame_delta(0, -2), 1, "a negative delta: red faces");
        assert_eq!(mood_frame_delta(2, -2), 0);
        assert_eq!(mood_frame_delta(0, 0), 0, "an even city shows no lit face");
        assert_eq!(mood_frame_delta(4, 9), 2, "at most five");
        assert_eq!(mood_frame(0, 2), 1, "legacy planner rule: fewer than three lit are red");
        // Not clicked: no buttons.
        assert!(!ui.visible(w(&ui, id::CITY_PLANNER)) && !ui.visible(w(&ui, id::CITY_BUY[0])));
        // The card hangs above its anchor: the pointer's tip is near the anchor.
        let pointer = ui.rect(w(&ui, id::CITY_POINTER));
        assert!(ui.visible(w(&ui, id::CITY_ROOT)));
        let _ = pointer;
        card.pinned = true;
        card.buy = vec![
            BuyOption { domain: VehicleDomain::Land, cost: 60.0, refused: None },
            BuyOption { domain: VehicleDomain::Sea, cost: 80.0, refused: Some("needs 2 cities".into()) },
            BuyOption { domain: VehicleDomain::Air, cost: 100.0, refused: Some("needs 3 cities".into()) },
        ];
        bind_city_card(&mut ui, Some(&card), &mut assets, &index);
        assert!(ui.visible(w(&ui, id::CITY_PLANNER)) && ui.visible(w(&ui, id::CITY_BUY[2])));
        assert!(ui.flags[w(&ui, id::CITY_BUY[0])] & crate::spui::ids::flag::ENABLED != 0);
        assert!(ui.flags[w(&ui, id::CITY_BUY[1])] & crate::spui::ids::flag::ENABLED == 0);
        assert_eq!(ui.tooltip_overrides[w(&ui, id::CITY_BUY[0])].as_deref(), Some("Buy Land Vehicle ($60)"));
        assert_eq!(ui.tooltip_overrides[w(&ui, id::CITY_BUY[1])].as_deref(), Some("Buy Sea Vehicle ($80)\nneeds 2 cities"));
        bind_city_card(&mut ui, None, &mut assets, &index);
        assert!(!ui.visible(w(&ui, id::CITY_ROOT)));
    }

    #[test]
    fn the_unit_cards_and_the_spice_card_fill_in() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, VEHICLE_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        ui.screen = [1280.0, 900.0];
        bind_unit_card(
            &mut ui,
            Some((Vec2::new(640.0, 450.0), 0.5, 50, 1.0, 2, "Your Military land", 0xFF2A_C8AA)),
            id::VEHICLE_TYPE,
            &id::VEHICLE_STATS,
            VEHICLE_TRIM,
        );
        let w = |ui: &SpuiState, c: u32| ui.find(c).unwrap();
        assert_eq!(ui.captions[w(&ui, id::ROLL_NUMBER)].as_deref(), Some("50"));
        assert_eq!(ui.frame_overrides[w(&ui, id::VEHICLE_TYPE)], Some(2));
        assert_eq!(ui.frame_overrides[w(&ui, id::ROLL_FACE)], Some(4));
        let bar = ui.area(w(&ui, id::ROLL_HEALTH));
        let full = ui.layout.window(w(&ui, id::ROLL_HEALTH)).area;
        assert!(((bar[2] - bar[0]) - (full[2] - full[0]) * 0.5).abs() < 0.01);
        assert!(!ui.visible(w(&ui, id::VEHICLE_STATS[0])));
        bind_unit_card(&mut ui, None, id::VEHICLE_TYPE, &id::VEHICLE_STATS, VEHICLE_TRIM);
        assert!(!ui.visible(w(&ui, id::ROLL_ROOT)));
        let layout = assets.layout(&index, SPICE_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        ui.screen = [1280.0, 900.0];
        let card = SpiceCard { owner: Some(0), name: "Spice Geyser".into(), per_minute: 120.0, relation: 1.0, meter: 1.0, screen: Vec2::new(500.0, 400.0) };
        bind_spice_card(&mut ui, Some(&card));
        assert_eq!(ui.captions[w(&ui, id::SPICE_RATE)].as_deref(), Some("120"));
        assert_eq!(ui.frame_overrides[w(&ui, id::SPICE_METER)], Some(15));
        assert!(ui.visible(w(&ui, id::SPICE_ROOT)));
    }
}
