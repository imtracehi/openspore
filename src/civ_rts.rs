//! Civilization's real-time strategy layer: Spore's vehicles with selection
//! and orders, combat with shells and beams, preaching and trading, spice
//! derricks, wall turrets and the rival nations' AI.
//!
//! The persistent state lives in `civ::CivCampaign`; this module owns the
//! live vehicle entities and snapshots them back twice a second.
//!
//! Visuals: vehicles are the installed `gencar` / `genair` creations painted
//! in their nation's colour, crowned with an original weapon, cultural or
//! diplomatic vehicle part by type (primitive hulls without an install).
//! Shots, beams, explosions, selection rings, health bars, the drag box and
//! the city banners are OpenSpore adaptations, not original effects.
//!
//! Controls are listed in `civ`'s module documentation.

use bevy::{ecs::system::SystemParam, light::NotShadowCaster, prelude::*};
use std::collections::{HashMap, HashSet};

use crate::civ::{
    self, Approach, CivCampaign, CivCommander, UnitOrder, UnitState, CITY_COUNT, HOME_INDEX,
};
use crate::civ_planet::{Globe, Shape, CITY_SCALE, RETAIL_FROM_ISLAND};
use crate::civ_comm;
use crate::civ_rules;
use crate::creation_assets::{self, PresetKind};
use crate::game::{RunProgress, Vehicle};
use crate::stages::StageOwned;
use crate::Originals;

/// Military vehicles open fire inside this distance: the vehicle weapon
/// tools' range 100 (`military_template`, `mil_13..19`) x `CIV_WORLD_SCALE`.
pub const WEAPON_RANGE: f32 = civ_rules::VEHICLE_TOOL_RANGE * civ_rules::CIV_WORLD_SCALE;
/// Preaching, trading and assaulting a city work from this distance to its
/// centre (the walls are about 24 units out). ADAPTED: island 10.5 at
/// `CITY_SCALE`.
pub const CITY_REACH: f32 = 10.5 * CITY_SCALE;
/// Idle military vehicles engage enemies this close on their own (ADAPTED).
pub const AGGRO_RANGE: f32 = 9.0 * CITY_SCALE;
/// Damage of one hit of a default land vehicle: the tool's 1 x the damage
/// multiplier 10 + 20 x 0.33 (VERIFIED 0x4E74D0; each vehicle's own value is
/// `civ_combat::combat(domain, stats).damage`).
pub const SHOT_DAMAGE: f32 = 16.6;
/// `VehicleWeaponCycleTime` 2 s.
pub const SHOT_COOLDOWN: f32 = civ_rules::VEHICLE_WEAPON_CYCLE;
/// City turrets (`turret_at_*`) reach 100 x `CIV_WORLD_SCALE`.
pub const TURRET_RANGE: f32 = civ_rules::TURRET_TOOL_RANGE * civ_rules::CIV_WORLD_SCALE;
/// A military city's turret hit: `turret_at_mil` 25 (religious / economic
/// turrets `turret_at_rel / _ecn` 20 / 15).
pub const TURRET_DAMAGE: f32 = civ_rules::TURRET_TOOL_DAMAGE[0];
/// Seconds between turret hits. VERIFIED: the turret tools are beams; a beam
/// projectile deals one damage point every `1000 / (damage x
/// mDamageMultiplier)` ms while it holds its target (SetTool 0xCB5880 from
/// cDefaultBeamTool 0x1054F1A): 25 / 20 / 15 per second, the 0.1 s auto-fire
/// only re-aims it. ADAPTED: delivered as one-second hits of that damage.
pub const TURRET_COOLDOWN: f32 = 1.0;
/// A vehicle builds its derrick this close to the geyser.
pub const CLAIM_RANGE: f32 = 2.8 * CITY_SCALE;

const SHOT_SPEED: f32 = 22.0 * CITY_SCALE;
/// Land vehicles steer around city walls at this distance from the centre.
const CITY_CLEARANCE: f32 = 7.2 * CITY_SCALE;
/// Aircraft fly this high over the ground (world units; the vehicle's own
/// layout is in island units under a root drawn `CITY_SCALE` times larger).
const AIR_CLEARANCE: f32 = 5.0 * CITY_SCALE;
/// `terrain::BASE_Y`: the floor every grounded clearance is measured from.
#[cfg(test)]
const BASE_Y: f32 = -0.6;
const LAND_EXTENT: f32 = 2.4;
const AIR_EXTENT: f32 = 2.8;
const SEA_EXTENT: f32 = 2.6;
/// The largest a retail vehicle can be: the vehicle editor's
/// `editorBoundSize` 6 (VERIFIED data, `notes/rigid-editors-data.md`).
/// Vehicles are drawn at their own creation size (an editor unit is a world
/// unit, INFERRED as for creatures), capped by it; the old fit to
/// `*_EXTENT` x `CITY_SCALE` (9 / 10.5 / 9.75 units) drew them about 1.5x
/// too big. Distances, speeds and ranges are retail units already
/// (`civ_combat`, the tools' ranges) and do not change with the drawing.
pub const VEHICLE_BOUND_SIZE: f32 = 6.0;
/// Editor units to the vehicle's island-unit layout (its root is drawn
/// `CITY_SCALE` larger): one editor unit = one world unit.
pub const VEHICLE_MOUNT_SCALE: f32 = 1.0 / CITY_SCALE;
/// The drawn vehicle against the old land fit (6 / 9): muzzle and aim
/// heights, selection rings follow the smaller vehicles.
pub const VEHICLE_DRAW_RATIO: f32 = VEHICLE_BOUND_SIZE / (LAND_EXTENT * CITY_SCALE);
const SNAPSHOT_PERIOD: f32 = 0.5;
const AI_PERIOD: f32 = 1.0;
const DOUBLE_CLICK: f32 = 0.35;
const DRAG_PIXELS: f32 = crate::civ_controls::CLICK_DRAG_DISTANCE;
const PICK_PIXELS: f32 = 30.0;

// ---------------------------------------------------------------------------
// Components and resources
// ---------------------------------------------------------------------------

/// What a vehicle is doing this frame (drives the presentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnitAction {
    #[default]
    Idle,
    Moving,
    Firing,
    Preaching(usize),
    Trading(usize),
}

/// One live vehicle. Its id, nation, type and order are persisted through
/// `civ::UnitState`.
#[derive(Component, Debug, Clone)]
pub struct CivUnit {
    pub id: u32,
    pub nation: usize,
    pub kind: Approach,
    pub air: bool,
    pub domain: civ::VehicleDomain,
    pub hp: f32,
    pub max_hp: f32,
    pub order: UnitOrder,
    pub yaw: f32,
    pub velocity: Vec2,
    pub cooldown: f32,
    pub action: UnitAction,
    /// Seconds spent without an order (rival AI pacing).
    pub idle: f32,
    /// Seconds since this vehicle was last hit (presentation).
    pub hit: f32,
    /// Seconds since a cultural (religious) weapon last hit it: the retail vehicle event 0x18
    /// starts `civ_weapons_culture_vehicle_damage` on it (`civ_fx`).
    pub cultural_hit: f32,
    /// Seconds a Static Bomb still holds it (no moving, no shooting).
    pub stunned: f32,
    /// Seconds a Bribe Bomb still turns it on other enemy vehicles.
    pub bribed: f32,
    /// Power / defense / speed / weapon range (`civ_combat`).
    pub stats: crate::civ_combat::VehicleStats,
    /// Trade shuttle: driving back to the home city, the cargo carried, the
    /// seconds waited at the current end and the home city.
    pub homeward: bool,
    pub cargo: f32,
    pub trade_wait: f32,
    pub trade_home: Option<usize>,
    /// Waypoints around water / cliffs (`civ_path`), the goal they lead to
    /// and the seconds since they were planned.
    pub path: Vec<Vec3>,
    pub path_goal: Option<Vec3>,
    pub path_age: f32,
}

impl CivUnit {
    pub fn from_state(s: &UnitState) -> Self {
        let domain = s.domain();
        let stats = s.stats.unwrap_or_default();
        let max_hp = crate::civ_combat::combat(domain, stats).max_health;
        Self {
            id: s.id,
            nation: s.nation,
            kind: s.kind,
            air: domain == civ::VehicleDomain::Air,
            domain,
            hp: if s.hp > 0.0 { s.hp.min(max_hp) } else { max_hp },
            max_hp,
            order: s.order,
            yaw: s.yaw,
            velocity: Vec2::ZERO,
            cooldown: 0.0,
            action: UnitAction::Idle,
            idle: 0.0,
            hit: 10.0,
            cultural_hit: 10.0,
            stunned: 0.0,
            bribed: 0.0,
            stats,
            // A saved trade shuttle resumes its round trip.
            homeward: s.shuttle.is_some_and(|t| t.homeward),
            cargo: s.shuttle.map_or(0.0, |t| t.cargo),
            trade_wait: s.shuttle.map_or(0.0, |t| t.wait),
            trade_home: s.shuttle.and_then(|t| t.home),
            path: Vec::new(),
            path_goal: None,
            path_age: 10.0,
        }
    }

    /// Hit damage, health and standard speed from the stats (0x4E74D0).
    pub fn combat(&self) -> crate::civ_combat::VehicleCombat {
        crate::civ_combat::combat(self.domain, self.stats)
    }

    pub fn state(&self, at: Vec3) -> UnitState {
        UnitState {
            id: self.id,
            nation: self.nation,
            kind: self.kind,
            air: self.domain == civ::VehicleDomain::Air,
            position: [at.x, at.z],
            world: Some(at.to_array()),
            yaw: self.yaw,
            hp: self.hp,
            order: self.order,
            domain: self.domain,
            stats: Some(self.stats),
            shuttle: (self.kind == Approach::Economic && (self.homeward || self.cargo > 0.0 || self.trade_home.is_some())).then_some(
                civ::ShuttleState { homeward: self.homeward, cargo: self.cargo, wait: self.trade_wait, home: self.trade_home },
            ),
        }
    }
}

/// Marks your selected vehicles.
#[derive(Component)]
pub struct Selected;

/// The vehicle carrying the original `OriginalCreation` preset root (the
/// others draw copies of the same cached creation).
#[derive(Component)]
pub struct Flagship;

/// The child entities a vehicle's presentation drives.
#[derive(Component)]
pub struct UnitRig {
    pub art: Entity,
    pub ring: Entity,
    pub bar: Entity,
    pub fill: Entity,
    pub lift: f32,
}

/// Parent of every shot and effect, so the terrain grounding never moves them.
#[derive(Component)]
pub struct FxRoot;

/// What a shot hits when it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotTarget {
    Unit(u32),
    /// An epic predator (`civ_wildlife`).
    Epic(u32),
    Turret(usize, usize),
    /// A house / factory / entertainment building: city, plot.
    Building(usize, usize),
    /// The city hall (counts only once every other building is down).
    City(usize),
    Derrick(usize),
}

#[derive(Component, Clone, Debug)]
pub struct CivShot {
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
    pub duration: f32,
    pub arc: f32,
    pub damage: f32,
    pub target: ShotTarget,
    pub nation: usize,
    /// A cultural weapon's bolt (the religious vehicle's, not the military shell).
    pub cultural: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxKind {
    Boom,
    Spark,
    Marker,
    Puff,
}

/// A short-lived effect at a world position.
#[derive(Component, Clone, Debug)]
pub struct CivFx {
    pub pos: Vec3,
    pub age: f32,
    pub life: f32,
    pub size: f32,
    pub kind: FxKind,
}

/// A preaching / trading beam owned by a vehicle id.
#[derive(Component)]
pub struct ActionBeam(pub u32);

/// The golden halo over a city being preached to.
#[derive(Component)]
pub struct PreachHalo(pub usize);

/// A trade convoy crate travelling an open trade route.
#[derive(Component)]
pub struct TradeCargo {
    pub city: usize,
    pub phase: f32,
}

/// Screen-space city banner (name, armour and influence bars).
#[derive(Component)]
pub struct CityLabel(pub usize);
#[derive(Component)]
pub struct CityLabelText(pub usize);
/// Bar `1` is armour, bar `2` faith or purchase.
#[derive(Component)]
pub struct CityLabelBar(pub usize, pub u8);

#[derive(Component)]
pub struct DragBox;

/// Vehicles waiting for their entity (opening fleet, production, AI).
#[derive(Resource, Default)]
pub struct UnitSpawns(pub Vec<UnitState>);

/// Input and simulation bookkeeping that is not saved.
#[derive(Resource, Default)]
pub struct RtsState {
    pub left_press: Option<Vec2>,
    pub right_press: Option<Vec2>,
    pub drag: Option<(Vec2, Vec2)>,
    pub last_click: Option<(f32, u32)>,
    pub turret_cooldown: HashMap<(usize, usize), f32>,
    /// Where each turret is aiming (presentation).
    pub turret_aim: HashMap<(usize, usize), Vec3>,
    pub snapshot: f32,
    pub ai: f32,
    pub fx_root: Option<Entity>,
    pub camera: Option<Entity>,
    /// Cities being preached to this frame.
    pub preaching: HashSet<usize>,
    /// The groups performing a conversion this frame, `(city, nation)` -> (vehicles,
    /// group power): the retail groups of 0xBEB310 (`civ_rules` "the conversion
    /// model"), stepped once a second by `preach_clock`.
    pub preach_groups: HashMap<(usize, usize), (u32, f32)>,
    pub preach_clock: f32,
    /// Throttle for progress messages.
    pub chatter: f32,
    /// Each rival's spice geyser target and seconds since it was chosen
    /// (`cCivilization` +0x468, 0xBF9960).
    pub expansion: HashMap<usize, (Option<usize>, f32)>,
    /// Seconds since each rival's last vehicle build.
    pub build_wait: HashMap<usize, f32>,
    /// The ability tool the player has armed (`civ_voices` plays its armed sounds, `civ_target`
    /// shows the cursor): a left click on a valid target fires it, the same key / button, a right
    /// click or Escape drops it.
    pub armed: Option<civ::PowerSlot>,
}

/// Shared meshes and materials of the RTS presentation.
#[derive(Resource)]
pub struct RtsAssets {
    pub ring: Handle<Mesh>,
    pub ring_own: Handle<StandardMaterial>,
    pub quad: Handle<Mesh>,
    pub bar_back: Handle<StandardMaterial>,
    pub bar_own: Handle<StandardMaterial>,
    pub bar_rival: Handle<StandardMaterial>,
    pub shell: Handle<Mesh>,
    pub bolt: Handle<Mesh>,
    /// Military, religious and economic shot colours.
    pub shot: [Handle<StandardMaterial>; 3],
    pub boom: Handle<Mesh>,
    pub boom_mat: Handle<StandardMaterial>,
    pub spark_mat: Handle<StandardMaterial>,
    pub smoke_mat: Handle<StandardMaterial>,
    pub marker_mat: Handle<StandardMaterial>,
    pub beam: Handle<Mesh>,
    pub preach_mat: Handle<StandardMaterial>,
    pub trade_mat: Handle<StandardMaterial>,
    pub cargo: Handle<Mesh>,
    pub halo: Handle<Mesh>,
    pub hull: Handle<Mesh>,
    pub dome: Handle<Mesh>,
    pub nation: Vec<Handle<StandardMaterial>>,
    pub dark: Handle<StandardMaterial>,
    /// Painted copies of the creation blocks per (nation, block, domain_slot).
    pub paint: HashMap<(usize, usize, u8), Handle<StandardMaterial>>,
    /// Creation imports that failed once are not retried every spawn.
    pub creation_failed: [bool; 3],
    /// Type crowns per vehicle type, once imported.
    pub crowns: HashMap<Approach, Option<crate::OriginalModel>>,
}

/// Every asset store a vehicle spawn touches.
#[derive(SystemParam)]
pub struct ArtAssets<'w> {
    pub originals: ResMut<'w, Originals>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
}

/// The mouse, the camera and the ground under the cursor.
#[derive(SystemParam)]
pub struct Pointer<'w, 's> {
    pub mouse: Res<'w, ButtonInput<MouseButton>>,
    pub windows: Query<'w, 's, &'static Window>,
    pub camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<crate::GameCamera>>,
    pub hover: Option<Res<'w, bevy::picking::hover::HoverMap>>,
    /// The retail SPUI HUD (`civ_hud`) reports its own hit regions here.
    pub spui: Option<Res<'w, crate::spui::view::SpuiPointer>>,
    pub ui: Query<'w, 's, (), With<Node>>,
    pub surface: Option<Res<'w, crate::terrain::LandSurface>>,
    /// The city / derrick pie menu (`civ_orders`) owns the clicks on its icons.
    pub pie: Option<Res<'w, crate::civ_orders::CivPie>>,
}

impl Pointer<'_, '_> {
    fn cursor(&self) -> Option<Vec2> {
        self.windows.iter().find_map(|w| w.cursor_position())
    }

    /// The cursor is over a HUD element: clicks belong to the interface.
    fn over_ui(&self) -> bool {
        self.pie.as_ref().is_some_and(|pie| self.cursor().is_some_and(|c| pie.covers(c)))
            || self.spui.as_ref().is_some_and(|spui| spui.over_ui)
            || self.hover.as_ref().is_some_and(|hover| {
                hover
                    .0
                    .values()
                    .any(|hits| hits.keys().any(|e| self.ui.contains(*e)))
            })
    }

    fn project(&self, world: Vec3) -> Option<Vec2> {
        let (camera, transform) = self.camera.single().ok()?;
        camera.world_to_viewport(transform, world).ok()
    }

    /// The ground (or sea) under the cursor.
    fn ground(&self, cursor: Vec2) -> Option<Vec3> {
        let (camera, transform) = self.camera.single().ok()?;
        let ray = camera.viewport_to_world(transform, cursor).ok()?;
        ground_hit(ray.origin, *ray.direction, self.surface.as_deref())
    }
}

/// Pure: first point where a ray meets the terrain, the sea on the planet, or
/// the base plane (`civ_planet::Globe::ray`).
pub fn ground_hit(
    origin: Vec3,
    direction: Vec3,
    surface: Option<&crate::terrain::LandSurface>,
) -> Option<Vec3> {
    Globe::from_surface(surface).ray(origin, direction)
}

/// Pure: where a city's turret `i` stands in the world (its authored mount of
/// the walls model in the city's frame, plus 1.35 island units for the gun
/// over its mount: the earlier fixed 2.4 was a 1.05 tower top plus that).
pub fn turret_world(city: &civ::CityState, turret: usize, shape: Shape) -> Vec3 {
    let site = city.turret_site(turret);
    let height = city.layout().turret_height(turret) + 1.35;
    shape.local_point(city.center(), Vec3::new(site.x, height, site.y) * CITY_SCALE)
}

/// Pure: where a city's plot `plot` building stands in the world (its plot
/// in the city's frame, 1.2 island units over the floor: where shots land).
pub fn plot_world(city: &civ::CityState, plot: usize, shape: Shape) -> Vec3 {
    let site = city.plot_site(plot);
    shape.local_point(city.center(), Vec3::new(site.x, 1.2, site.y) * CITY_SCALE)
}

/// Pure: the nearest standing building of a city to `at`.
pub fn nearest_building(city: &civ::CityState, at: Vec3, shape: Shape) -> Option<(usize, Vec3)> {
    (0..city.plots.len())
        .filter(|p| city.standing(*p))
        .map(|p| (p, plot_world(city, p, shape)))
        .min_by(|a, b| shape.distance(at, a.1).total_cmp(&shape.distance(at, b.1)))
}

/// Pure (VERIFIED 0xCF4322, the Civ command handler 0xCF3D50): a click on a
/// vehicle orders every selected vehicle that is not economic (purpose 2) to
/// attack it, military and religious alike (the earlier OpenSpore mapping
/// refused the religious ones).
pub fn can_attack_vehicle(kind: Approach) -> bool {
    kind != Approach::Economic
}

/// Pure (VERIFIED 0xE35C42 for the cursor): military boats are not sent
/// against the buildings of a city that is not yours and has no boat spawn
/// point, unless the order also holds a vehicle that is not a boat (0xCF3D50
/// gives such an order to every selected vehicle).
pub fn boat_refused(
    domain: civ::VehicleDomain,
    kind: Approach,
    city_owned: bool,
    coastal: bool,
    has_non_boat: bool,
) -> bool {
    domain == civ::VehicleDomain::Sea && kind == Approach::Military && !city_owned && !coastal && !has_non_boat
}

/// Pure: whether two nations' vehicles fight on sight.
pub fn hostile(campaign: &CivCampaign, a: usize, b: usize) -> bool {
    if a == b {
        return false;
    }
    if a == HOME_INDEX {
        campaign.at_war(b)
    } else if b == HOME_INDEX {
        campaign.at_war(a)
    } else {
        false
    }
}

/// Pure: whether city `c`'s turrets shoot at this vehicle. Rival turrets fire
/// on anyone at war with them and on any military or religious vehicle sent
/// against the city; economic vehicles may trade in peace.
pub fn turret_hostile(campaign: &CivCampaign, c: usize, unit: &CivUnit) -> bool {
    let Some(city) = campaign.cities.get(c) else {
        return false;
    };
    if unit.nation == city.nation() {
        return false;
    }
    if city.owned() {
        campaign.at_war(unit.nation)
    } else if unit.nation == HOME_INDEX {
        campaign.at_war(c) || (unit.order == UnitOrder::City(c) && unit.kind != Approach::Economic)
    } else {
        false
    }
}

/// Pure: build a vehicle at an owned city by explicit domain. `live` counts
/// your vehicles of that city's type (alive or queued): the cap is per type
/// (VERIFIED 0xBF2110) and the price per locomotion (0xBF22B0).
pub fn try_produce_domain(
    campaign: &mut CivCampaign,
    spice: &mut f32,
    live: usize,
    city: usize,
    domain: civ::VehicleDomain,
) -> Result<UnitState, String> {
    campaign.ensure_cities();
    let Some(home) = campaign.cities.get(city).cloned() else {
        return Err("no such city".into());
    };
    if !home.owned() {
        return Err(format!("{} is not yours", home.label()));
    }
    if domain == civ::VehicleDomain::Air && !campaign.air_unlocked() {
        return Err(format!(
            "Aircraft need vehicle tech level {} ({} cities; you hold {})",
            civ_rules::AIR_TECH_LEVEL,
            civ::AIR_UNLOCK_CITIES,
            campaign.owned_count()
        ));
    }
    if domain == civ::VehicleDomain::Sea && !campaign.sea_unlocked() {
        return Err(format!(
            "Sea vehicles need {} cities (you hold {})",
            civ::SEA_UNLOCK_CITIES,
            campaign.owned_count()
        ));
    }
    let shape = campaign.shape();
    if domain == civ::VehicleDomain::Sea && shape.on_sphere() && home.harbor.is_none() {
        return Err(format!("{} has no harbour: sea vehicles roll out of seaports", home.label()));
    }
    let cap = campaign.vehicle_cap_for(HOME_INDEX, home.kind);
    if live >= cap {
        return Err(format!(
            "Your {} cities support no more {} vehicles ({live}/{cap}): build houses or take cities",
            home.kind.title().to_lowercase(),
            home.kind.title().to_lowercase()
        ));
    }
    let cost = match domain {
        civ::VehicleDomain::Air => civ::AIR_VEHICLE_COST,
        civ::VehicleDomain::Land => civ::LAND_VEHICLE_COST,
        civ::VehicleDomain::Sea => civ::SEA_VEHICLE_COST,
    };
    if *spice < cost {
        return Err(format!(
            "A {} {} vehicle costs {cost:.0} spice",
            home.kind.title().to_lowercase(),
            domain.title()
        ));
    }
    *spice -= cost;
    let pad = match (domain, home.harbor) {
        (civ::VehicleDomain::Sea, Some(harbor)) => Vec3::from_array(harbor),
        _ => campaign.vehicle_pad(city),
    };
    let out = shape
        .from_tangent(pad, shape.to_tangent(pad, pad - home.center()))
        .normalize_or(shape.bearing(pad, std::f32::consts::FRAC_PI_2));
    let side = shape.up(pad).cross(out);
    let k = live as f32;
    let step = 1.9 * CITY_SCALE;
    let at = shape.toward(pad, side, ((k % 5.0) - 2.0) * step);
    let at = shape.toward(at, out, (k / 5.0).floor() * step);
    Ok(campaign.add_unit_domain(HOME_INDEX, home.kind, domain, at))
}

pub fn try_produce(
    campaign: &mut CivCampaign,
    spice: &mut f32,
    live: usize,
    city: usize,
    air: bool,
) -> Result<UnitState, String> {
    try_produce_domain(
        campaign,
        spice,
        live,
        city,
        if air {
            civ::VehicleDomain::Air
        } else {
            civ::VehicleDomain::Land
        },
    )
}

fn shot_style(kind: Approach) -> usize {
    match kind {
        Approach::Military => 0,
        Approach::Religious => 1,
        Approach::Economic => 2,
    }
}

// ---------------------------------------------------------------------------
// Setup and spawning
// ---------------------------------------------------------------------------

fn glow(color: Color, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        emissive: (color.to_linear() * strength).into(),
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

fn glass(color: Color, alpha: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        emissive: (color.to_linear() * 0.8).into(),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

/// Install the RTS resources, the effect root and the screen-space HUD bits
/// (city banners and the drag box). Called from `civ::setup_civ`.
pub fn setup_rts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    run: &RunProgress,
    camera: Option<Entity>,
) {
    let assets = RtsAssets {
        ring: meshes.add(Torus::new(0.86, 1.0)),
        ring_own: materials.add(glow(Color::srgb(0.35, 1.0, 0.35), 0.6)),
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        bar_back: materials.add(glow(Color::srgb(0.05, 0.05, 0.05), 0.0)),
        bar_own: materials.add(glow(Color::srgb(0.3, 0.95, 0.35), 0.4)),
        bar_rival: materials.add(glow(Color::srgb(1.0, 0.25, 0.2), 0.4)),
        shell: meshes.add(Sphere::new(0.2)),
        bolt: meshes.add(Capsule3d::new(0.09, 0.9)),
        shot: [
            materials.add(glow(Color::srgb(1.0, 0.62, 0.18), 3.0)),
            materials.add(glow(Color::srgb(1.0, 0.92, 0.45), 3.0)),
            materials.add(glow(Color::srgb(0.35, 0.95, 1.0), 3.0)),
        ],
        boom: meshes.add(Sphere::new(1.0)),
        boom_mat: materials.add(glass(Color::srgb(1.0, 0.55, 0.15), 0.8)),
        spark_mat: materials.add(glass(Color::srgb(1.0, 0.9, 0.5), 0.9)),
        smoke_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.25, 0.23, 0.22, 0.55),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        }),
        marker_mat: materials.add(glow(Color::srgb(0.4, 1.0, 0.45), 1.0)),
        beam: meshes.add(Cylinder::new(0.09, 1.0)),
        preach_mat: materials.add(glass(Color::srgb(1.0, 0.85, 0.35), 0.55)),
        trade_mat: materials.add(glass(Color::srgb(0.35, 1.0, 0.55), 0.55)),
        cargo: meshes.add(Cuboid::new(0.55, 0.4, 0.75)),
        halo: meshes.add(Torus::new(2.6, 3.0)),
        hull: meshes.add(Cuboid::new(1.3, 0.55, 2.1)),
        dome: meshes.add(Sphere::new(0.45)),
        nation: (0..CITY_COUNT)
            .map(|n| materials.add(civ::nation_material(n)))
            .collect(),
        dark: materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.18, 0.2),
            perceptual_roughness: 0.6,
            ..default()
        }),
        paint: HashMap::new(),
        creation_failed: [false; 3],
        crowns: HashMap::new(),
    };
    commands.insert_resource(assets);
    commands.insert_resource(UnitSpawns(run.civ_campaign.units.clone()));
    let fx_root = commands
        .spawn((
            StageOwned,
            FxRoot,
            Name::new("civilization effects"),
            Transform::IDENTITY,
            Visibility::Inherited,
        ))
        .id();
    commands.insert_resource(RtsState {
        fx_root: Some(fx_root),
        camera,
        ..default()
    });
    // The drag-select rectangle.
    let drag = commands
        .spawn((
            StageOwned,
            DragBox,
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.3, 1.0, 0.4, 0.08)),
            BorderColor::all(Color::srgba(0.45, 1.0, 0.5, 0.85)),
        ))
        .id();
    if let Some(camera) = camera {
        commands.entity(drag).insert(UiTargetCamera(camera));
    }
    // A banner over every city: name, type, armour and influence, as Spore
    // labels its cities with the owner's colours.
    for index in 0..CITY_COUNT {
        let bar = |parent: &mut ChildSpawnerCommands, which: u8, color: Color| {
            parent
                .spawn((
                    Node {
                        width: Val::Px(112.0),
                        height: Val::Px(5.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                ))
                .with_children(|track| {
                    track.spawn((
                        CityLabelBar(index, which),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                });
        };
        let label = commands
            .spawn((
                StageOwned,
                CityLabel(index),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(124.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.08, 0.12, 0.62)),
                BorderColor::all(civ::nation_color(index)),
                Visibility::Hidden,
            ))
            .with_children(|label| {
                label.spawn((
                    CityLabelText(index),
                    Text::new(civ::city_label(index)),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
                bar(label, 1, Color::srgb(0.35, 0.95, 0.4));
                bar(label, 2, Color::srgb(1.0, 0.85, 0.3));
            })
            .id();
        if let Some(camera) = camera {
            commands.entity(label).insert(UiTargetCamera(camera));
        }
    }
}

/// Import (once) the installed creation for `kind`; returns the lift that
/// puts its lowest point on the ground, its drawn height and the extent
/// (island units) it is drawn at: its own size, at most
/// [`VEHICLE_BOUND_SIZE`] (`extent` only when the size is unusable).
fn ensure_creation(
    assets: &mut RtsAssets,
    art: &mut ArtAssets,
    kind: PresetKind,
    extent: f32,
) -> Option<(f32, f32, f32)> {
    let slot = match kind {
        PresetKind::AirVehicle => 1,
        PresetKind::SeaVehicle => 2,
        _ => 0,
    };
    if assets.creation_failed[slot] {
        return None;
    }
    let record =
        creation_assets::select_creation(&art.originals.creation_catalog.creations, kind)?.clone();
    if !art.originals.creation_models.contains_key(&record.key) {
        match creation_assets::import_creation(
            &mut art.originals,
            &record,
            &mut art.meshes,
            &mut art.images,
            &mut art.materials,
        ) {
            Ok(imported) => {
                art.originals.creation_models.insert(record.key, imported);
            }
            Err(err) => {
                warn!("civ vehicle creation {} unavailable: {err}", record.name);
                assets.creation_failed[slot] = true;
                return None;
            }
        }
    }
    let imported = &art.originals.creation_models[&record.key];
    let size = imported.local_max - imported.local_min;
    let largest = size.max_element();
    if !largest.is_finite() || largest <= 0.0 {
        return None;
    }
    let extent = retail_extent(largest).unwrap_or(extent);
    let scale = extent / largest;
    Some((size.y * 0.5 * scale, size.y * scale, extent))
}

/// Pure: the island-unit extent a creation of `largest` world units is drawn
/// at (its own size, capped by the editor bound).
pub fn retail_extent(largest: f32) -> Option<f32> {
    (largest.is_finite() && largest > 0.0).then(|| largest.min(VEHICLE_BOUND_SIZE) / CITY_SCALE)
}

/// Paint a vehicle block in its nation's colour: bare painted panels take
/// the colour outright, textured parts a tint.
fn paint_material(material: &mut StandardMaterial, nation: usize) {
    let color = civ::nation_color(nation).to_linear();
    if material.base_color_texture.is_none() {
        let original = material.base_color.to_linear();
        let lum = 0.2126 * original.red + 0.7152 * original.green + 0.0722 * original.blue;
        let shade = 0.65 + 0.5 * lum.clamp(0.0, 1.0);
        material.base_color =
            Color::linear_rgb(color.red * shade, color.green * shade, color.blue * shade);
    } else {
        let original = material.base_color.to_linear();
        let mix = |a: f32, b: f32| a * (0.55 + 0.45 * b);
        material.base_color = Color::linear_rgb(
            mix(original.red, color.red),
            mix(original.green, color.green),
            mix(original.blue, color.blue),
        );
    }
}

/// Spawn a painted copy of the cached creation under `parent` (the same
/// geometry `creation_assets::mount_preset` lays out, without the
/// `OriginalCreation` marker that belongs to the flagship alone).
fn spawn_creation_copy(
    commands: &mut Commands,
    assets: &mut RtsAssets,
    art: &mut ArtAssets,
    parent: Entity,
    kind: PresetKind,
    extent: f32,
    nation: usize,
) -> bool {
    let Some(record) =
        creation_assets::select_creation(&art.originals.creation_catalog.creations, kind)
    else {
        return false;
    };
    let Some(imported) = art.originals.creation_models.get(&record.key).cloned() else {
        return false;
    };
    let largest = (imported.local_max - imported.local_min).max_element();
    if !largest.is_finite() || largest <= 0.0 {
        return false;
    }
    let scale = extent / largest;
    let center = (imported.local_min + imported.local_max) * 0.5;
    let root = commands
        .spawn((
            StageOwned,
            Transform::default(),
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    let kind_id: u8 = match kind {
        PresetKind::AirVehicle => 1,
        PresetKind::SeaVehicle => 2,
        _ => 0,
    };
    for (i, block) in imported.blocks.iter().enumerate() {
        let material = match assets.paint.get(&(nation, i, kind_id)) {
            Some(handle) => handle.clone(),
            None => {
                let mut material = block
                    .mechanical
                    .model
                    .material
                    .as_ref()
                    .and_then(|h| art.materials.get(h))
                    .cloned()
                    .unwrap_or_else(|| StandardMaterial {
                        base_color: Color::srgb(0.6, 0.7, 0.85),
                        perceptual_roughness: 0.4,
                        ..default()
                    });
                if let Some(paint) = block.paints.first() {
                    material.base_color =
                        Color::srgb(paint.color1[0], paint.color1[1], paint.color1[2]);
                }
                paint_material(&mut material, nation);
                let handle = art.materials.add(material);
                assets.paint.insert((nation, i, kind_id), handle.clone());
                handle
            }
        };
        let transform = Transform::from_translation((block.position - center) * scale)
            .with_rotation(Quat::from_mat3(&block.orientation).normalize())
            .with_scale(Vec3::splat(
                block.scale * block.mechanical.model.scale * scale,
            ));
        commands.spawn((
            StageOwned,
            Mesh3d(block.mechanical.model.draw_mesh()),
            MeshMaterial3d(material),
            transform,
            ChildOf(root),
        ));
    }
    true
}

/// The original vehicle part that marks a vehicle's type: a weapon for the
/// military, a cultural loudspeaker for the religious, a diplomatic dish for
/// the economic. Imported once per type.
fn crown(
    assets: &mut RtsAssets,
    art: &mut ArtAssets,
    kind: Approach,
) -> Option<crate::OriginalModel> {
    if let Some(cached) = assets.crowns.get(&kind) {
        return cached.clone();
    }
    let name = match kind {
        Approach::Military => "ve_weapons_05",
        Approach::Religious => "ve_cultural_03",
        Approach::Economic => "ve_diplomatic_03",
    };
    let id = crate::world_objects::name_id(name);
    let model = art
        .originals
        .part_catalog
        .parts
        .iter()
        .find(|d| d.key.group_id == crate::asset_catalog::VEHICLE_PARTS && d.key.instance_id == id)
        .cloned()
        .and_then(|definition| {
            crate::mechanical_assets::import(
                &mut art.originals,
                &definition,
                &mut art.meshes,
                &mut art.images,
                &mut art.materials,
            )
            .map_err(|e| warn!("civ type part {name}: {e}"))
            .ok()
        })
        .map(|imported| imported.model);
    assets.crowns.insert(kind, model.clone());
    model
}

/// Spawn one vehicle with its art, selection ring and health bar.
/// Returns the entity and whether it carries the original preset root.
fn spawn_unit(
    commands: &mut Commands,
    assets: &mut RtsAssets,
    art: &mut ArtAssets,
    s: &UnitState,
    flagship: bool,
    shape: Shape,
    custom: bool,
) -> (Entity, bool) {
    let clearance = if s.domain() == civ::VehicleDomain::Air {
        AIR_CLEARANCE
    } else {
        0.0
    };
    // The vehicle's own layout (art, ring, bar) is in island units under a
    // root drawn `CITY_SCALE` times larger, on the local up of its spot.
    let at = s.at();
    let root = commands
        .spawn((
            StageOwned,
            Name::new(format!(
                "{} {} {} vehicle {}",
                civ::city_label(s.nation),
                s.kind.title(),
                s.domain().title(),
                s.id
            )),
            CivUnit::from_state(s),
            Transform::from_translation(shape.place_on(at, clearance))
                .with_rotation(shape.heading(at, s.yaw))
                .with_scale(Vec3::splat(CITY_SCALE)),
            crate::terrain::Radial,
            crate::terrain::Grounded::new(clearance),
            Visibility::Inherited,
        ))
        .id();
    if s.nation == HOME_INDEX {
        commands.entity(root).insert(Vehicle);
    }
    let (kind, mut extent) = match s.domain() {
        civ::VehicleDomain::Air => (PresetKind::AirVehicle, AIR_EXTENT),
        civ::VehicleDomain::Sea => (PresetKind::SeaVehicle, SEA_EXTENT),
        civ::VehicleDomain::Land => (PresetKind::LandVehicle, LAND_EXTENT),
    };
    let anchor = commands
        .spawn((
            StageOwned,
            Transform::default(),
            Visibility::Inherited,
            ChildOf(root),
        ))
        .id();
    let mut drawn = false;
    let mut original = false;
    let mut lift = 0.45;
    let mut height = 0.9;
    if custom {
        // The player's own design (`rigid_editor`) is mounted by
        // `rts_spawn` instead of the preset art.
        drawn = true;
        lift = 0.05;
        height = 1.1;
        if flagship {
            original = true;
            commands.entity(root).insert(Flagship);
        }
    } else if let Some((l, h, e)) = ensure_creation(assets, art, kind, extent) {
        lift = l;
        height = h;
        extent = e;
        if flagship {
            if let Ok(true) = creation_assets::mount_preset(
                commands,
                &mut art.originals,
                &mut art.meshes,
                &mut art.images,
                &mut art.materials,
                anchor,
                kind,
                extent,
            ) {
                drawn = true;
                original = true;
                commands.entity(root).insert(Flagship);
            }
        }
        if !drawn {
            drawn = spawn_creation_copy(commands, assets, art, anchor, kind, extent, s.nation);
        }
    }
    commands
        .entity(anchor)
        .insert(Transform::from_xyz(0.0, lift, 0.0));
    if !drawn {
        // No install: a painted hull with a canopy.
        let paint = assets.nation[s.nation.min(assets.nation.len() - 1)].clone();
        commands.spawn((
            StageOwned,
            Mesh3d(assets.hull.clone()),
            MeshMaterial3d(paint),
            Transform::default(),
            ChildOf(anchor),
        ));
        commands.spawn((
            StageOwned,
            Mesh3d(assets.dome.clone()),
            MeshMaterial3d(assets.dark.clone()),
            Transform::from_xyz(0.0, 0.35, -0.2),
            ChildOf(anchor),
        ));
    }
    // The type crown: an original weapon / cultural / diplomatic part.
    if let Some(model) = crown(assets, art, s.kind).filter(|_| !custom) {
        let scale = crate::mechanical_assets::capped_scale(&art.meshes, &model, 1.1);
        let material = model
            .material
            .clone()
            .unwrap_or_else(|| assets.dark.clone());
        commands.spawn((
            StageOwned,
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, height * 0.5 + 0.15, 0.1).with_scale(Vec3::splat(scale)),
            ChildOf(anchor),
        ));
    }
    // The selection ring hugs the drawn vehicle.
    let ring_size = (extent / LAND_EXTENT).clamp(0.4, 1.2);
    let ring_material = if s.nation == HOME_INDEX {
        assets.ring_own.clone()
    } else {
        assets.bar_rival.clone()
    };
    let ring = commands
        .spawn((
            StageOwned,
            Mesh3d(assets.ring.clone()),
            MeshMaterial3d(ring_material),
            Transform::from_xyz(0.0, 0.06, 0.0).with_scale(Vec3::new(1.55, 0.06, 1.55) * Vec3::new(ring_size, 1.0, ring_size)),
            Visibility::Hidden,
            NotShadowCaster,
            ChildOf(root),
        ))
        .id();
    let bar = commands
        .spawn((
            StageOwned,
            Transform::from_xyz(0.0, height + 0.9, 0.0),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        StageOwned,
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(assets.bar_back.clone()),
        Transform::from_scale(Vec3::new(1.7, 0.26, 1.0)),
        NotShadowCaster,
        ChildOf(bar),
    ));
    let fill = commands
        .spawn((
            StageOwned,
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(if s.nation == HOME_INDEX {
                assets.bar_own.clone()
            } else {
                assets.bar_rival.clone()
            }),
            Transform::from_xyz(0.0, 0.0, 0.01).with_scale(Vec3::new(1.6, 0.16, 1.0)),
            NotShadowCaster,
            ChildOf(bar),
        ))
        .id();
    commands.entity(root).insert(UnitRig {
        art: anchor,
        ring,
        bar,
        fill,
        lift,
    });
    (root, original)
}

/// Turn queued vehicles into entities: the opening fleet, production and
/// the rival AI all go through here. Your vehicles carry the vehicle build.
pub fn rts_spawn(
    mut commands: Commands,
    mut spawns: ResMut<UnitSpawns>,
    mut assets: ResMut<RtsAssets>,
    mut art: ArtAssets,
    flagship: Query<(), With<Flagship>>,
    run: Res<RunProgress>,
) {
    if spawns.0.is_empty() {
        return;
    }
    let mut has_flagship = !flagship.is_empty();
    let shape = run.civ_campaign.shape();
    for state in std::mem::take(&mut spawns.0) {
        let wants_flagship = !has_flagship && state.nation == HOME_INDEX && !state.air;
        let class = (state.nation == HOME_INDEX).then(|| (state.kind, state.domain()));
        let custom = class.is_some_and(|(a, d)| run.rigid.vehicle_design(a, d).is_some());
        let (entity, original) =
            spawn_unit(&mut commands, &mut assets, &mut art, &state, wants_flagship, shape, custom);
        has_flagship |= original;
        if state.nation == HOME_INDEX && (custom || !run.vehicle.parts.is_empty()) {
            civ::mount_vehicle_class_parts(
                &mut commands,
                &art.meshes,
                &mut art.materials,
                &art.originals,
                entity,
                &run,
                class,
            );
        }
    }
}

/// The flagship's preset parts arrive with their authored paint; give them
/// the nation colour like every other vehicle.
pub fn paint_flagship(
    parts: Query<
        (&MeshMaterial3d<StandardMaterial>, &ChildOf),
        Added<creation_assets::OriginalCreationPart>,
    >,
    parents: Query<&ChildOf>,
    units: Query<&CivUnit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (material, parent) in &parts {
        let mut ancestor = parent.parent();
        for _ in 0..6 {
            if let Ok(unit) = units.get(ancestor) {
                if let Some(mut m) = materials.get_mut(&material.0) {
                    paint_material(&mut m, unit.nation);
                }
                break;
            }
            let Ok(next) = parents.get(ancestor) else {
                break;
            };
            ancestor = next.parent();
        }
    }
}

fn spawn_fx(
    commands: &mut Commands,
    assets: &RtsAssets,
    root: Entity,
    pos: Vec3,
    kind: FxKind,
    size: f32,
) {
    let (mesh, material, life) = match kind {
        FxKind::Boom => (assets.boom.clone(), assets.boom_mat.clone(), 0.55),
        FxKind::Spark => (assets.boom.clone(), assets.spark_mat.clone(), 0.18),
        FxKind::Marker => (assets.ring.clone(), assets.marker_mat.clone(), 0.6),
        FxKind::Puff => (assets.boom.clone(), assets.smoke_mat.clone(), 1.4),
    };
    commands.spawn((
        StageOwned,
        CivFx {
            pos,
            age: 0.0,
            life,
            // Effect sizes are island units, drawn at the city scale.
            size: size * CITY_SCALE,
            kind,
        },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(pos).with_scale(Vec3::splat(0.01)),
        NotShadowCaster,
        ChildOf(root),
    ));
}

#[allow(clippy::too_many_arguments)]
fn fire(
    commands: &mut Commands,
    assets: &RtsAssets,
    root: Entity,
    from: Vec3,
    to: Vec3,
    target: ShotTarget,
    damage: f32,
    nation: usize,
    style: usize,
) {
    let shell = style == 0;
    let duration = (from.distance(to) / SHOT_SPEED).max(0.12);
    commands.spawn((
        StageOwned,
        CivShot {
            from,
            to,
            t: 0.0,
            duration,
            arc: if shell {
                CITY_SCALE + from.distance(to) * 0.12
            } else {
                0.0
            },
            damage,
            target,
            nation,
            cultural: !shell,
        },
        Mesh3d(if shell {
            assets.shell.clone()
        } else {
            assets.bolt.clone()
        }),
        MeshMaterial3d(assets.shot[style.min(2)].clone()),
        Transform::from_translation(from).with_scale(Vec3::splat(CITY_SCALE)),
        NotShadowCaster,
        ChildOf(root),
    ));
    spawn_fx(commands, assets, root, from, FxKind::Spark, 0.45);
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Where a right-click lands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Unit(u32, usize),
    City(usize),
    Geyser(usize),
    /// A goodie hut (`civ_orders::hut_command` gives the capture orders by reach).
    Hut(usize),
    /// An epic predator by its id, and the ground point under the click.
    Epic(u32, Vec3),
    Ground(Vec3),
}

/// Spread `k`-th vehicle of a group around a move point (tangent offset).
fn formation(k: usize) -> Vec2 {
    if k == 0 {
        return Vec2::ZERO;
    }
    let angle = k as f32 * 2.39996;
    Vec2::new(angle.cos(), angle.sin()) * 1.7 * CITY_SCALE * (k as f32).sqrt()
}

/// `p` moved by a tangent-frame offset along the surface.
fn offset_on(shape: Shape, p: Vec3, offset: Vec2) -> Vec3 {
    if offset == Vec2::ZERO {
        return p;
    }
    shape.toward(p, shape.from_tangent(p, offset), offset.length())
}

/// Does this right click open a pie menu (`civ_orders`)? VERIFIED 0xCF56A0: a geyser held by a
/// civilisation with an economic vehicle selected goes to the economic handler 0xCE8C30, whose
/// popup (0xCE8940) is the bribe offer for another civilisation's derrick; the hall of a city that is
/// not the player's is the pie menu (its "more" cursor, `civ_cursor`; the command handler 0xCF3D50
/// gives a hall no order of its own).
fn pie_wanted(
    target: Target,
    hover: Option<crate::civ_cursor::Target>,
    campaign: &CivCampaign,
    has_economic: bool,
) -> Option<crate::civ_orders::PieTarget> {
    use crate::civ_orders::{route_right_click, ClickRoute, PieTarget};
    match target {
        Target::City(c)
            if matches!(hover, Some(crate::civ_cursor::Target::CityHall { owned: false }))
                && campaign.cities.get(c).is_some_and(|city| !city.owned()) =>
        {
            Some(PieTarget::City(c))
        }
        Target::Geyser(g) => {
            let owner = campaign.geysers.get(g).and_then(|s| s.owner);
            (route_right_click(owner.is_some(), has_economic) == ClickRoute::Economic
                && owner.is_some_and(|o| o != HOME_INDEX))
            .then_some(PieTarget::Geyser(g))
        }
        _ => None,
    }
}

/// Selection, orders and production. Mouse: left-click / Shift+click /
/// double-click / drag-box select, right-click orders. Keys: `Q` select the
/// doctrine fleet, `F` attack/preach/trade the rival city nearest the
/// camera, `X` claim geysers, `G`/`H` build land/air vehicles, Space centre.
#[allow(clippy::too_many_arguments)]
pub fn rts_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    pointer: Pointer,
    mut state: ResMut<RtsState>,
    mut spawns: ResMut<UnitSpawns>,
    assets: Res<RtsAssets>,
    mut run: ResMut<RunProgress>,
    mut units: Query<(Entity, &mut CivUnit, &Transform, Has<Selected>), Without<CivCommander>>,
    mut focus: Query<&mut Transform, (With<CivCommander>, Without<CivUnit>)>,
    planner: Option<Res<crate::civ_planner::CivPlanner>>,
    hover: Option<Res<crate::civ_cursor::CivHover>>,
    mut pie: Option<ResMut<crate::civ_orders::PieRequests>>,
    mut huts: Option<ResMut<crate::civ_orders::HutRequests>>,
    epics: Query<(&crate::civ_wildlife::CivEpic, &Transform), (Without<CivUnit>, Without<CivCommander>)>,
) {
    // The City Planner screen owns the mouse and the keys while open.
    if planner.is_some_and(|p| p.is_open()) {
        return;
    }
    let run = &mut *run;
    run.civ_campaign.ensure_cities();
    let now = time.elapsed_secs();
    // Ctrl adds to the selection (the manual); Shift still does.
    let shift = keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight)
        || keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight);
    let focus_at = focus.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let shape = run.civ_campaign.shape();
    let dist = |a: Vec3, b: Vec3| shape.distance(a, b);
    // The HUD reports on the city under the camera.
    run.civ_campaign.last_target = run
        .civ_campaign
        .cities
        .iter()
        .filter(|c| run.civ_campaign.present(c.index))
        .map(|c| (c.index, dist(focus_at, c.center())))
        .filter(|(_, d)| *d <= 20.0 * RETAIL_FROM_ISLAND)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);

    let mut selected: HashSet<Entity> = units
        .iter()
        .filter(|(_, u, _, s)| *s && u.nation == HOME_INDEX)
        .map(|(e, ..)| e)
        .collect();
    let before = selected.clone();
    let doctrine = run.civ_campaign.approach;

    if keys.just_pressed(KeyCode::KeyQ) {
        selected.clear();
        let mut matching: Vec<Entity> = units
            .iter()
            .filter(|(_, u, ..)| u.nation == HOME_INDEX && u.kind == doctrine)
            .map(|(e, ..)| e)
            .collect();
        if matching.is_empty() {
            matching = units
                .iter()
                .filter(|(_, u, ..)| u.nation == HOME_INDEX)
                .map(|(e, ..)| e)
                .collect();
        }
        selected.extend(matching);
        run.civ_campaign.message = format!("{} vehicles selected", selected.len());
    }

    // ---- mouse
    let cursor = pointer.cursor();
    let over_ui = pointer.over_ui();
    let mut order_at: Option<Vec2> = None;
    // An armed ability tool takes the left click (retail message 0x576439B): it fires at the
    // ground under the cursor instead of selecting (`civ_target`).
    let mut tool_click: Option<Vec2> = None;
    if let (Some(c), Some(_)) = (cursor, state.armed) {
        if pointer.mouse.just_pressed(MouseButton::Left) && !over_ui {
            tool_click = Some(c);
        }
    }
    if let Some(c) = cursor {
        if pointer.mouse.just_pressed(MouseButton::Left) && !over_ui && tool_click.is_none() {
            state.left_press = Some(c);
            state.drag = None;
        }
        if let Some(start) = state.left_press {
            if pointer.mouse.pressed(MouseButton::Left) && start.distance(c) > DRAG_PIXELS {
                state.drag = Some((start, c));
            }
            if pointer.mouse.just_released(MouseButton::Left) {
                if let Some((a, b)) = state.drag.take() {
                    let (lo, hi) = (a.min(b), a.max(b));
                    if !shift {
                        selected.clear();
                    }
                    for (e, unit, t, _) in &units {
                        if unit.nation != HOME_INDEX {
                            continue;
                        }
                        if let Some(p) = pointer.project(t.translation + Vec3::Y * 0.6) {
                            if p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y {
                                selected.insert(e);
                            }
                        }
                    }
                } else {
                    // A click: the vehicle under the cursor, if any.
                    let picked = units
                        .iter()
                        .filter_map(|(e, unit, t, _)| {
                            let p = pointer.project(t.translation + Vec3::Y * 0.6)?;
                            let d = p.distance(c);
                            (d <= PICK_PIXELS).then_some((
                                e,
                                unit.id,
                                unit.nation,
                                unit.kind,
                                unit.air,
                                d,
                            ))
                        })
                        .min_by(|a, b| a.5.total_cmp(&b.5));
                    match picked {
                        Some((e, id, HOME_INDEX, kind, air, _)) => {
                            let double = state
                                .last_click
                                .is_some_and(|(at, last)| last == id && now - at <= DOUBLE_CLICK);
                            if double {
                                selected.clear();
                                selected.extend(units.iter().filter_map(|(e, u, ..)| {
                                    (u.nation == HOME_INDEX && u.kind == kind && u.air == air)
                                        .then_some(e)
                                }));
                            } else if shift {
                                if !selected.remove(&e) {
                                    selected.insert(e);
                                }
                            } else {
                                selected.clear();
                                selected.insert(e);
                            }
                            state.last_click = Some((now, id));
                        }
                        Some((e, _, nation, kind, _, _)) => {
                            // civ-visuals2: the clicked rival vehicle shows the selection circle
                            // in its owner's colour for a while (ADAPTED, `civ_fx::Inspected`).
                            commands.entity(e).insert(crate::civ_fx::Inspected::new(now));
                            run.civ_campaign.message = format!(
                                "{} {} vehicle{}",
                                civ::city_label(nation),
                                kind.title().to_lowercase(),
                                if run.civ_campaign.at_war(nation) {
                                    " - at war with you"
                                } else {
                                    ""
                                }
                            );
                        }
                        None => {
                            if !shift {
                                selected.clear();
                            }
                        }
                    }
                }
                state.left_press = None;
            }
        }
        if pointer.mouse.just_pressed(MouseButton::Right) && !over_ui {
            state.right_press = Some(c);
        }
        if pointer.mouse.just_released(MouseButton::Right) {
            // A click, not an orbit drag.
            if let Some(start) = state.right_press.take() {
                if start.distance(c) <= DRAG_PIXELS {
                    order_at = Some(c);
                }
            }
        }
    }
    if !pointer.mouse.pressed(MouseButton::Left) && !pointer.mouse.just_released(MouseButton::Left)
    {
        state.left_press = None;
        state.drag = None;
    }

    // The vehicles an order applies to: the selection, else the whole fleet.
    let fleet: Vec<Entity> = if selected.is_empty() {
        units
            .iter()
            .filter(|(_, u, ..)| u.nation == HOME_INDEX)
            .map(|(e, ..)| e)
            .collect()
    } else {
        let mut v: Vec<Entity> = selected.iter().copied().collect();
        v.sort_by_key(|e| units.get(*e).map(|(_, u, ..)| u.id).unwrap_or(0));
        v
    };

    // ---- right-click order; with an ability tool armed the click only drops the tool
    if order_at.is_some() && state.armed.take().is_some() {
        run.civ_campaign.message = "Ability dropped".into();
        order_at = None;
    }
    if let Some(c) = order_at {
        let target = (|| {
            let unit = units
                .iter()
                .filter(|(_, u, ..)| u.nation != HOME_INDEX)
                .filter_map(|(_, u, t, _)| {
                    let p = pointer.project(t.translation + Vec3::Y * 0.6)?;
                    let d = p.distance(c);
                    (d <= PICK_PIXELS).then_some((u.id, u.nation, d))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((id, nation, _)) = unit {
                return Some(Target::Unit(id, nation));
            }
            // 0xCF3D50 animal branch: the epic predator under the pointer (picked on screen like the cursor,
            // `civ_cursor`), the ground point kept for the vehicles that cannot fight it.
            if matches!(hover.as_deref().map(|h| h.target), Some(crate::civ_cursor::Target::Wild { alive: true })) {
                let epic = epics
                    .iter()
                    .filter(|(e, _)| e.hp > 0.0)
                    .filter_map(|(e, t)| {
                        let d = pointer.project(t.translation + Vec3::Y * 1.5)?.distance(c);
                        (d <= crate::civ_cursor::EPIC_PIXELS).then_some((e.id, t.translation, d))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                if let Some((id, at, _)) = epic {
                    return Some(Target::Epic(id, pointer.ground(c).unwrap_or(at)));
                }
            }
            let ground = pointer.ground(c)?;
            for g in 0..civ::GEYSER_COUNT {
                if dist(ground, run.civ_campaign.geyser_at(g)) <= 3.2 * CITY_SCALE {
                    return Some(Target::Geyser(g));
                }
            }
            // 0xCF4001 hut branch: a hut whose tribe is not yet captured.
            for h in 0..run.civ_campaign.hut_sites.len() {
                if run.civ_campaign.hut_standing(h)
                    && run.civ_campaign.hut_at(h).is_some_and(|at| dist(ground, at) <= crate::civ_cursor::HUT_RADIUS)
                {
                    return Some(Target::Hut(h));
                }
            }
            for city in &run.civ_campaign.cities {
                if dist(ground, city.center()) <= 7.0 * CITY_SCALE {
                    return Some(Target::City(city.index));
                }
            }
            Some(Target::Ground(ground))
        })();
        if let Some(target) = target {
            if selected.is_empty() {
                run.civ_campaign.message = "Select vehicles first (click, drag a box, or Q)".into();
            } else if let Some(refusal) = hover
                .as_deref()
                .and_then(|h| crate::civ_cursor::refusal(h.action, h.target))
            {
                // The cursor shows the no-entry sign (`civ_cursor`, 0xE35320): the order is not given.
                run.civ_campaign.message = refusal.into();
            } else if let Some(want) = pie_wanted(
                target,
                hover.as_deref().map(|h| h.target),
                &run.civ_campaign,
                fleet.iter().any(|e| units.get(*e).is_ok_and(|(_, u, ..)| u.kind == Approach::Economic)),
            ) {
                // 0xCF56A0 / 0xCE8C30 (civ_orders): the hall of a rival city opens the pie menu, a
                // rival's derrick the bribe popup when an economic vehicle is selected; the entries
                // give the orders.
                if let Some(requests) = pie.as_deref_mut() {
                    requests.0 = Some(crate::civ_orders::PieRequest { target: want, at: c });
                }
                run.civ_campaign.message = "Choose an order".into();
            } else if let Target::Hut(h) = target {
                // 0xCF4001 (VERIFIED): every selected vehicle that can reach the hut (0xC9EAF0) gets the
                // capture order (type 8, 0xCAC1A0), whatever its purpose; `civ_orders::hut_command` does it
                // with the landmass labels, and plays sfx_civ_target_hut once (0xCF40BF).
                if let Some(requests) = huts.as_deref_mut() {
                    requests.0 = Some(crate::civ_orders::HutRequest { hut: h, fleet: fleet.clone() });
                }
            } else {
                let mut ordered = 0;
                // 0xCF3D50 (the Civ command handler): a boat is refused a rival city's buildings
                // when the order has no non-boat vehicle and the city has no boat spawn point.
                let has_non_boat = fleet
                    .iter()
                    .any(|e| units.get(*e).is_ok_and(|(_, u, ..)| u.domain != civ::VehicleDomain::Sea));
                // A rival's building or turret (not its hall): the command handler orders only
                // military and religious vehicles (0xCF40D0: purposes 0 and 1); economic ones get
                // no order (they trade through the pie).
                let structure_click = matches!(
                    hover.as_deref().map(|h| h.target),
                    Some(crate::civ_cursor::Target::Structure { owned: false, .. })
                );
                // `civ_voices` (Attack_Epic, 0xDCC5A0): the click landed on a live epic predator.
                let epic_click =
                    matches!(hover.as_deref().map(|h| h.target), Some(crate::civ_cursor::Target::Wild { alive: true }));
                for (k, e) in fleet.iter().enumerate() {
                    let Ok((_, mut unit, ..)) = units.get_mut(*e) else {
                        continue;
                    };
                    unit.order = match target {
                        Target::City(c)
                            if structure_click
                                && unit.kind == Approach::Economic
                                && !run.civ_campaign.cities[c].owned() =>
                        {
                            commands.entity(*e).insert(crate::civ_voices::CantPerform);
                            continue;
                        }
                        // 0xCF4322 (VERIFIED): every vehicle that is not economic (purpose 2) is
                        // ordered against a vehicle, military and religious alike.
                        Target::Unit(id, _) if can_attack_vehicle(unit.kind) => UnitOrder::Unit(id),
                        Target::Unit(..) => {
                            // `civ_voices` (ADAPTED): the order cannot be performed.
                            commands.entity(*e).insert(crate::civ_voices::CantPerform);
                            continue;
                        }
                        Target::City(c)
                            if boat_refused(
                                unit.domain,
                                unit.kind,
                                run.civ_campaign.cities[c].owned(),
                                run.civ_campaign.is_coastal(c),
                                has_non_boat,
                            ) =>
                        {
                            // 0xE35C42: the cursor offers boats no attack on an inland city.
                            commands.entity(*e).insert(crate::civ_voices::CantPerform);
                            continue;
                        }
                        Target::City(c) => UnitOrder::City(c),
                        Target::Geyser(g) => {
                            // 0xCF3D50 geyser branch (civ_orders::geyser_command): unclaimed nodes
                            // are claimed by every vehicle, a rival's is attacked by military and
                            // religious ones, your own and (without the popup) a rival's for an
                            // economic vehicle give no order.
                            let holder = match run.civ_campaign.geysers.get(g).and_then(|s| s.owner) {
                                None => crate::civ_orders::Holder::Nobody,
                                Some(HOME_INDEX) => crate::civ_orders::Holder::Player,
                                Some(_) => crate::civ_orders::Holder::Rival,
                            };
                            if crate::civ_orders::geyser_command(holder, unit.kind, false).is_none() {
                                commands.entity(*e).insert(crate::civ_voices::CantPerform);
                                continue;
                            }
                            UnitOrder::Geyser(g)
                        }
                        // The retail attack order (type 1, 0xDCC5A0) on an epic predator: military vehicles
                        // close in and fire. Religious and economic vehicles keep the drive-to-point order
                        // (what a cultural weapon does to an animal was not decoded: ADAPTED).
                        Target::Epic(id, _) if unit.kind == Approach::Military => UnitOrder::Epic(id),
                        Target::Epic(_, p) | Target::Ground(p) => {
                            UnitOrder::Goto(offset_on(shape, p, formation(k)).to_array())
                        }
                        Target::Hut(_) => continue,
                    };
                    unit.idle = 0.0;
                    commands.entity(*e).insert(crate::civ_voices::VoiceOrder);
                    if epic_click && unit.kind != Approach::Economic {
                        commands.entity(*e).insert(crate::civ_voices::EpicOrder);
                    }
                    ordered += 1;
                }
                run.civ_campaign.message = match target {
                    Target::Unit(_, nation) => format!(
                        "{ordered} vehicles attack a {} vehicle",
                        civ::city_label(nation)
                    ),
                    Target::City(c) if run.civ_campaign.cities[c].owned() => {
                        format!("{ordered} vehicles garrison {}", civ::city_label(c))
                    }
                    Target::City(c) => {
                        format!("{ordered} vehicles sent against {}", civ::city_label(c))
                    }
                    Target::Geyser(_) => format!(
                        "{ordered} vehicles head for the spice geyser ({:.0} spice per derrick)",
                        civ::DERRICK_COST
                    ),
                    Target::Epic(..) => format!("{ordered} vehicles move on the epic creature"),
                    Target::Hut(_) => String::new(),
                    Target::Ground(_) => format!("{ordered} vehicles on the move"),
                };
                if let (Some(root), Some(ground)) = (state.fx_root, pointer.ground(c)) {
                    if crate::civ_fx::destination_effect_on() {
                        // civ-visuals2: the retail destination arrows (`civ_selection_destination`).
                        crate::civ_fx::destination_effect(&mut commands, ground, shape);
                    } else {
                        spawn_fx(
                            &mut commands,
                            &assets,
                            root,
                            ground + shape.up(ground) * 0.1,
                            FxKind::Marker,
                            1.6,
                        );
                    }
                }
            }
        }
    }

    // ---- F: press the rival city nearest the camera with the fleet.
    if keys.pressed(KeyCode::KeyF) {
        let target = run
            .civ_campaign
            .cities
            .iter()
            .filter(|c| !c.owned())
            .min_by(|a, b| dist(focus_at, a.center()).total_cmp(&dist(focus_at, b.center())))
            .map(|c| c.index);
        match target {
            None => run.civ_campaign.message = "No rival city is left".into(),
            Some(c) => {
                let mut counts = [0usize; 3];
                for e in &fleet {
                    if let Ok((_, mut unit, ..)) = units.get_mut(*e) {
                        if unit.order != UnitOrder::City(c) {
                            unit.order = UnitOrder::City(c);
                            unit.idle = 0.0;
                            commands.entity(*e).insert(crate::civ_voices::VoiceOrder);
                        }
                        counts[shot_style(unit.kind)] += 1;
                    }
                }
                if keys.just_pressed(KeyCode::KeyF) {
                    run.civ_campaign.message = if fleet.is_empty() {
                        "You have no vehicles: build some with G".into()
                    } else {
                        format!(
                            "Fleet sent against {}: {} military, {} religious, {} economic",
                            civ::city_label(c),
                            counts[0],
                            counts[1],
                            counts[2]
                        )
                    };
                }
            }
        }
    }

    // ---- X: claim the free geysers nearest each vehicle.
    if keys.just_pressed(KeyCode::KeyX) {
        let mut taken: HashSet<usize> = units
            .iter()
            .filter(|(_, u, ..)| u.nation == HOME_INDEX)
            .filter_map(|(_, u, ..)| match u.order {
                UnitOrder::Geyser(g) => Some(g),
                _ => None,
            })
            .collect();
        let mut sent = 0;
        for e in &fleet {
            let Ok((_, mut unit, t, _)) = units.get_mut(*e) else {
                continue;
            };
            let at = t.translation;
            let campaign = &run.civ_campaign;
            let free = (0..civ::GEYSER_COUNT)
                .filter(|g| campaign.geysers[*g].free() && !taken.contains(g))
                .min_by(|a, b| {
                    dist(at, campaign.geyser_at(*a)).total_cmp(&dist(at, campaign.geyser_at(*b)))
                });
            let Some(g) = free else {
                break;
            };
            taken.insert(g);
            unit.order = UnitOrder::Geyser(g);
            unit.idle = 0.0;
            commands.entity(*e).insert(crate::civ_voices::VoiceOrder);
            sent += 1;
        }
        run.civ_campaign.message = if sent == 0 {
            "No free spice geyser is left to claim".into()
        } else {
            format!(
                "{sent} vehicles sent to claim spice ({:.0} spice per derrick)",
                civ::DERRICK_COST
            )
        };
    }

    // ---- G / H / J: build a vehicle at the city nearest the camera.
    let air = keys.just_pressed(KeyCode::KeyH);
    let sea = keys.just_pressed(KeyCode::KeyJ);
    if keys.just_pressed(KeyCode::KeyG) || air || sea {
        let city = run
            .civ_campaign
            .nearest_owned(focus_at, civ::BUILD_REACH)
            .unwrap_or(HOME_INDEX);
        let kind = run.civ_campaign.cities[city].kind;
        let live = units
            .iter()
            .filter(|(_, u, ..)| u.nation == HOME_INDEX && u.kind == kind)
            .count()
            + spawns
                .0
                .iter()
                .filter(|u| u.nation == HOME_INDEX && u.kind == kind)
                .count();
        let domain = if air {
            civ::VehicleDomain::Air
        } else if sea {
            civ::VehicleDomain::Sea
        } else {
            civ::VehicleDomain::Land
        };
        let mut spice = run.spice;
        match try_produce_domain(&mut run.civ_campaign, &mut spice, live, city, domain) {
            Ok(unit) => {
                run.civ_campaign.message = format!(
                    "{} built a {} {} vehicle ({}/{})",
                    civ::city_label(city),
                    unit.kind.title().to_lowercase(),
                    unit.domain().title(),
                    live + 1,
                    run.civ_campaign.vehicle_cap_for(HOME_INDEX, kind)
                );
                spawns.0.push(unit);
                run.request_save();
            }
            Err(message) => run.civ_campaign.message = message,
        }
        run.spice = spice;
    }

    // Consequence abilities and the superweapon (plain digits 7, 8, 9, 0, or
    // U, I, O, P when not ctrl-held): `CivCampaign::use_power` pays and
    // applies the campaign side, the vehicles are handled here.
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let slot = if ctrl {
        None
    } else if keys.just_pressed(KeyCode::Digit7) || keys.just_pressed(KeyCode::KeyU) {
        Some(civ::PowerSlot::Cell)
    } else if keys.just_pressed(KeyCode::Digit8) || keys.just_pressed(KeyCode::KeyI) {
        Some(civ::PowerSlot::Creature)
    } else if keys.just_pressed(KeyCode::Digit9) || keys.just_pressed(KeyCode::KeyO) {
        Some(civ::PowerSlot::Tribal)
    } else if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::KeyP) {
        Some(civ::PowerSlot::Superweapon)
    } else {
        None
    };
    // Retail arms an ability tool first (0xCF1D40; 0xCEA320 plays the tool's armed sounds) and
    // launches it with the next click on the planet (0xCF688E, `civ_target`): the key (or the HUD
    // button) arms the tool when it could be used, the same key again drops it (the retail
    // button sends the cancel message 0x1B), the superweapons are launched at once, a left click
    // on a valid target fires, right-click / Escape drop it (`OPENSPORE_CIV_ARMING=0` fires at
    // the camera focus at once as before).
    let mut aim = civ::PowerAim::Camera;
    let mut aim_at = focus_at;
    let slot = slot.filter(|slot| {
        if !crate::civ_voices::arming_enabled() {
            return true;
        }
        let data = crate::civ_voices::slot_data(&run.civ_campaign, *slot);
        let name = data.map_or("Ability", |d| d.name);
        if state.armed == Some(*slot) {
            state.armed = None;
            run.civ_campaign.message = format!("{name} dropped");
            return false;
        }
        match crate::civ_hud::sim::power_refusal(&run.civ_campaign, *slot, run.spice) {
            Some(refusal) => {
                state.armed = None;
                run.civ_campaign.message = refusal;
            }
            None if data.is_some_and(|d| crate::civ_target::rule(d.tool) == crate::civ_target::TargetRule::Immediate) => {
                // 0xCF1D40: the superweapons need no target.
                state.armed = None;
                return true;
            }
            None => {
                state.armed = Some(*slot);
                run.civ_campaign.message = format!("{name} armed: click a target, right-click or Esc drops it");
            }
        }
        false
    });
    let mut slot = slot;
    if let (Some(c), Some(armed)) = (tool_click, state.armed) {
        use crate::civ_target::Click;
        match crate::civ_target::resolve_click(&run.civ_campaign, armed, pointer.ground(c)) {
            Click::Fire { at, city } => {
                state.armed = None;
                slot = Some(armed);
                aim = civ::PowerAim::Click { city };
                aim_at = at;
            }
            Click::Invalid(message) => run.civ_campaign.message = message,
            Click::Ignore => {}
        }
    }
    if let Some(slot) = slot {
        let mut money = run.spice;
        match run.civ_campaign.use_power_at(slot, &mut money, aim_at, aim) {
            Ok(used) => {
                run.spice = money;
                run.civ_campaign.message = used.message.clone();
                commands.spawn(crate::civ_voices::PowerCue { tool: used.tool });
                if let Some(effect) = used.effect {
                    apply_unit_effect(&mut units, effect, &mut run.civ_campaign);
                }
                if let Some(root) = state.fx_root {
                    // Retail radii are world units; `spawn_fx` sizes are island units.
                    let (kind, size) = match (slot, used.effect) {
                        (_, Some(civ::UnitEffect::Damage { radius, .. })) => {
                            (FxKind::Boom, (radius / CITY_SCALE).max(3.0))
                        }
                        (civ::PowerSlot::Superweapon, _) => (FxKind::Boom, 5.5),
                        (_, Some(civ::UnitEffect::Stun { radius, .. })) => {
                            (FxKind::Spark, radius / CITY_SCALE)
                        }
                        _ => (FxKind::Marker, 3.5),
                    };
                    let up = shape.up(used.at);
                    spawn_fx(&mut commands, &assets, root, used.at + up * 0.5 * CITY_SCALE, kind, size);
                    if kind == FxKind::Boom {
                        spawn_fx(
                            &mut commands,
                            &assets,
                            root,
                            used.at + up * 1.5 * CITY_SCALE,
                            FxKind::Puff,
                            size * 0.8,
                        );
                    }
                }
                run.request_save();
            }
            Err(err) => run.civ_campaign.message = err,
        }
    }

    // ---- Space: centre the camera on the selection (or the capital).
    if keys.just_pressed(KeyCode::Space) {
        let points: Vec<Vec3> = selected
            .iter()
            .filter_map(|e| units.get(*e).ok().map(|(_, _, t, _)| t.translation))
            .collect();
        let centre = if points.is_empty() {
            run.civ_campaign.vehicle_pad(HOME_INDEX)
        } else {
            points.iter().copied().sum::<Vec3>() / points.len() as f32
        };
        if let Ok(mut t) = focus.single_mut() {
            // `terrain::ground` puts the focus back on the surface.
            t.translation = if shape.on_sphere() {
                centre
            } else {
                Vec3::new(centre.x, t.translation.y, centre.z)
            };
        }
    }

    if selected != before {
        for e in before.difference(&selected) {
            commands.entity(*e).remove::<Selected>();
        }
        for e in selected.difference(&before) {
            commands.entity(*e).insert(Selected);
        }
    }
}

/// Apply an ability's effect to the live vehicles: repairs for yours,
/// damage, stuns or bribes for the rivals inside its radius (a hit nation
/// counts as attacked).
fn apply_unit_effect(
    units: &mut Query<(Entity, &mut CivUnit, &Transform, Has<Selected>), Without<CivCommander>>,
    effect: civ::UnitEffect,
    campaign: &mut CivCampaign,
) {
    let mut hit: HashSet<usize> = HashSet::new();
    let shape = campaign.shape();
    let dist_xz = |a: Vec3, b: Vec3| shape.distance(a, b);
    for (_, mut unit, t, _) in units.iter_mut() {
        let at = t.translation;
        match effect {
            civ::UnitEffect::Heal(amount) if unit.nation == HOME_INDEX => {
                unit.hp = (unit.hp + amount).min(unit.max_hp);
            }
            civ::UnitEffect::Damage { at: c, radius, amount }
                if unit.nation != HOME_INDEX && dist_xz(at, c) <= radius =>
            {
                unit.hp -= amount;
                unit.hit = 0.0;
                hit.insert(unit.nation);
            }
            civ::UnitEffect::Stun { at: c, radius, seconds }
                if unit.nation != HOME_INDEX && dist_xz(at, c) <= radius =>
            {
                unit.stunned = seconds;
                unit.velocity = Vec2::ZERO;
                hit.insert(unit.nation);
            }
            civ::UnitEffect::Bribe { at: c, radius, seconds }
                if unit.nation != HOME_INDEX && dist_xz(at, c) <= radius =>
            {
                unit.bribed = seconds;
                unit.order = UnitOrder::Idle;
            }
            _ => {}
        }
    }
    for nation in hit {
        campaign.bombed(nation);
    }
}

// ---------------------------------------------------------------------------
// Rival nations
// ---------------------------------------------------------------------------

/// Deterministic jitter in [0, 1) from a vehicle id and a tick.
fn jitter(id: u32, tick: u32) -> f32 {
    let mut h = id.wrapping_mul(0x9E37_79B9) ^ tick.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 10_000.0
}

/// The rival nations think once a second: build vehicles, claim spice near
/// home, raid the player when at war (military nations) and patrol.
pub fn rts_ai(
    time: Res<Time>,
    mut state: ResMut<RtsState>,
    mut run: ResMut<RunProgress>,
    mut spawns: ResMut<UnitSpawns>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut units: Query<(&mut CivUnit, &Transform)>,
) {
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == crate::stages::GameStage::Civilization),
    );
    state.ai += time.delta_secs();
    if state.ai < AI_PERIOD {
        return;
    }
    state.ai -= AI_PERIOD;
    let difficulty = civ_rules::Difficulty::from_index(run.cell.difficulty);
    let campaign = &mut run.civ_campaign;
    campaign.ensure_cities();
    let shape = campaign.shape();
    let dist_xz = |a: Vec3, b: Vec3| shape.distance(a, b);
    // ADAPTED island radii at retail scale.
    let near_home = |island: f32| island * RETAIL_FROM_ISLAND;
    let tick = (campaign.clock * 10.0) as u32;
    let snapshot: Vec<(u32, usize, Vec3, UnitOrder)> = units
        .iter()
        .map(|(u, t)| (u.id, u.nation, t.translation, u.order))
        .collect();
    for nation in 1..CITY_COUNT {
        if !campaign.nation_alive(nation) {
            continue;
        }
        let live = snapshot.iter().filter(|u| u.1 == nation).count()
            + spawns.0.iter().filter(|u| u.nation == nation).count();
        // Unit builds follow the rival's decisions (0xBFA7A0 military 40 s,
        // cultural 50 s; 0xBFB160 economic 60 s): one vehicle per interval of
        // the nation's type (INFERRED mapping of type to builder).
        let kind = campaign.cities[nation].kind;
        let wait = state.build_wait.entry(nation).or_insert(f32::MAX);
        *wait += AI_PERIOD;
        if *wait >= civ_comm::build_interval(kind)
            && live < campaign.vehicle_cap(nation)
            && campaign.nations[nation].spice >= civ::LAND_VEHICLE_COST
        {
            *wait = 0.0;
            campaign.nations[nation].spice -= civ::LAND_VEHICLE_COST;
            let kind = campaign.cities[nation].kind;
            let pad = campaign.vehicle_pad(nation);
            let side = shape.bearing(pad, 0.0);
            let at = shape.toward(pad, side, (live as f32 - 1.0) * 2.0 * CITY_SCALE);
            let unit = campaign.add_unit(nation, kind, false, at);
            spawns.0.push(unit);
        }
        // Attack decision (0xBFA7A0): once the player has developed, a
        // nation whose attitude is Annoyed or worse targets the player.
        let stance = civ_comm::stance(campaign.nations[nation].relation.value());
        if campaign.player_developed && stance < 2 && !campaign.nations[nation].war {
            campaign.nations[nation].war = true;
            campaign.message = format!("{} turns hostile: its vehicles will attack your cities", civ::city_label(nation));
        }
        // The stopwatch of 0xBFA7A0 (+0x148 / +0x168) runs from the nation's creation, war or not; the
        // decision itself is the retail one (`civ_comm::attack_decision`): the highest-scoring city
        // among those the nation is Annoyed or worse with, re-chosen 40 s / 50 s after the last.
        campaign.nations[nation].raid -= AI_PERIOD;
        if let Some(purpose) = civ_comm::AttackPurpose::of(campaign.cities[nation].kind) {
            let orders: Vec<(usize, UnitOrder)> = snapshot.iter().map(|u| (u.1, u.3)).collect();
            civ_comm::attack_decision(campaign, nation, purpose, difficulty, &orders, &mut |n| {
                rand::random::<u32>() % n.max(1)
            });
        }
        // Expansion (0xBF9960): every 10 s, or when the target is taken, the
        // rival picks the spice geyser with the best score.
        let targets: HashMap<usize, Option<usize>> = state.expansion.iter().map(|(n, (t, _))| (*n, *t)).collect();
        let radius = campaign.sphere.map_or(500.0, |s| s[3]);
        let capital = campaign.cities[nation].center();
        let (player_derricks, own_derricks) = (campaign.derricks(HOME_INDEX), campaign.derricks(nation));
        let entry = state.expansion.entry(nation).or_insert((None, f32::MAX));
        entry.1 += AI_PERIOD;
        let taken = entry.0.is_some_and(|g| campaign.geysers.get(g).is_some_and(|s| !s.free()));
        if taken || entry.1 > crate::civ_comm::EXPANSION_INTERVAL {
            let current = if taken { None } else { entry.0 };
            entry.1 = 0.0;
            entry.0 = if difficulty == civ_rules::Difficulty::Easy
                && !crate::civ_comm::expands_on_easy(player_derricks, own_derricks)
            {
                None
            } else {
                let list: Vec<(Option<usize>, f32, u32)> = (0..civ::GEYSER_COUNT.min(campaign.geysers.len()))
                    .map(|g| {
                        let weight = targets.iter().filter(|(n, t)| **n != nation && **t == Some(g)).count() as u32 * 2;
                        // A pump under construction already belongs to its builder (the node is out of state 0).
                        (campaign.geysers[g].owner.or(campaign.geysers[g].claim.map(|c| c.nation)), dist_xz(campaign.geyser_at(g), capital), weight)
                    })
                    .collect();
                crate::civ_comm::expansion_pick(&list, current, radius)
            };
        }
    }
    for (mut unit, t) in &mut units {
        let nation = unit.nation;
        if nation == HOME_INDEX || !campaign.nation_alive(nation) {
            continue;
        }
        if unit.order != UnitOrder::Idle
            || unit.idle < 1.5
            || unit.stunned > 0.0
            || unit.bribed > 0.0
        {
            continue;
        }
        let home = campaign.cities[nation].center();
        let at = t.translation;
        // 1. Claim free spice near home.
        let claimed: HashSet<usize> = snapshot
            .iter()
            .filter(|u| u.1 == nation)
            .filter_map(|u| match u.3 {
                UnitOrder::Geyser(g) => Some(g),
                _ => None,
            })
            .collect();
        // The nation's expansion target (0xBF9960), one vehicle at a time.
        if campaign.nations[nation].spice >= civ::DERRICK_COST {
            let target = state.expansion.get(&nation).and_then(|(t, _)| *t);
            if let Some(g) = target.filter(|g| campaign.geysers[*g].free() && !claimed.contains(g)) {
                unit.order = UnitOrder::Geyser(g);
                continue;
            }
        }
        let _ = at;
        // 2. Military nations at war raid the player.
        if unit.kind == Approach::Military && campaign.at_war(nation) {
            let derrick = (0..civ::GEYSER_COUNT)
                .filter(|g| campaign.geysers[*g].owner == Some(HOME_INDEX))
                .filter(|g| dist_xz(campaign.geyser_at(*g), home) <= near_home(40.0))
                .min_by(|a, b| {
                    dist_xz(home, campaign.geyser_at(*a))
                        .total_cmp(&dist_xz(home, campaign.geyser_at(*b)))
                });
            let intruder = snapshot
                .iter()
                .filter(|u| u.1 == HOME_INDEX && dist_xz(u.2, home) <= near_home(22.0))
                .min_by(|a, b| dist_xz(a.2, home).total_cmp(&dist_xz(b.2, home)));
            if let Some(intruder) = intruder {
                unit.order = UnitOrder::Unit(intruder.0);
                continue;
            }
            if let Some(g) = derrick {
                unit.order = UnitOrder::Geyser(g);
                continue;
            }
            // City attacks: the target 0xBFA7A0 chose for the military purpose
            // (`NationState::attack_target`, re-chosen 40 s after the last; on Easy / Medium a rival with
            // more cities than the player holds back there, `civ_comm::attack_decision`). Idle vehicles
            // go at it as the order loop 0xBF65F8 sends every vehicle without an order.
            if let Some(target) = campaign.nations[nation].attack_target[0].filter(|t| campaign.cities.get(*t).is_some_and(|c| c.owned())) {
                unit.order = UnitOrder::City(target);
                continue;
            }
        }
        // 2b. Religious nations hostile to the player preach at the city 0xBFA7A0 chose for the
        // cultural purpose (`attack_target[1]`, re-chosen every 50 s: the score of
        // `civ_comm::attack_score`, mood x 4 term and the random terms included).
        if unit.kind == Approach::Religious && campaign.at_war(nation) && campaign.player_developed {
            if let Some(target) = campaign.nations[nation].attack_target[1].filter(|t| campaign.cities.get(*t).is_some_and(|c| c.owned())) {
                unit.order = UnitOrder::City(target);
                continue;
            }
        }
        // 3. Patrol outside the walls.
        let angle = jitter(unit.id, tick) * std::f32::consts::TAU;
        let radius = (8.5 + 4.0 * jitter(unit.id ^ 0x55, tick)) * CITY_SCALE;
        let p = shape.toward(home, shape.bearing(home, angle), radius);
        // Land vehicles patrol on land (the next roll tries another spot).
        if unit.domain == civ::VehicleDomain::Land && !globe.is_land(globe.surface(p)) {
            unit.idle = 0.0;
            continue;
        }
        unit.order = UnitOrder::Goto(p.to_array());
    }
}

// ---------------------------------------------------------------------------
// Movement and combat
// ---------------------------------------------------------------------------

fn angle_lerp(from: f32, to: f32, t: f32) -> f32 {
    let mut delta = (to - from) % std::f32::consts::TAU;
    if delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    from + delta * t.clamp(0.0, 1.0)
}

/// Epic predators share the vehicles' position map (`goal_of`) under this key bit (vehicle ids are small).
const EPIC_KEY: u32 = 0x8000_0000;

/// The point a vehicle's order drives it to, and how close it stops.
fn goal_of(
    unit: &CivUnit,
    at: Vec3,
    campaign: &CivCampaign,
    positions: &HashMap<u32, Vec3>,
    shape: Shape,
) -> Option<(Vec3, f32)> {
    // A trade shuttle on its way home.
    if let (UnitOrder::City(_), true, Some(h)) = (unit.order, unit.homeward, unit.trade_home) {
        if let Some(home) = campaign.cities.get(h) {
            return Some((home.center(), 6.0 * CITY_SCALE));
        }
    }
    match unit.order {
        UnitOrder::Idle => None,
        // An island-era order: its x, z on the plane through the stage pole.
        UnitOrder::Move(p) => Some((Vec3::new(p[0], 0.0, p[1]), 0.4 * CITY_SCALE)),
        UnitOrder::Goto(p) => Some((Vec3::from_array(p), 0.4 * CITY_SCALE)),
        UnitOrder::City(c) => {
            let city = campaign.cities.get(c)?;
            let center = city.center();
            if city.nation() == unit.nation {
                return Some((center, 9.0 * CITY_SCALE));
            }
            // 0xBD7F70 / CivGame 0x06776CA9 (civ_orders): above mood 50 the player's preachers
            // auto-target the city's entertainment to lower its happiness.
            if unit.kind == Approach::Religious && unit.nation == HOME_INDEX && crate::civ_orders::convert_locked(city) {
                if let Some((_, building)) = crate::civ_orders::nearest_entertainment(city, at, shape) {
                    return Some((building, WEAPON_RANGE * 0.75));
                }
            }
            if unit.kind == Approach::Military {
                // Close on the nearest standing turret, then the walls.
                let turret = (0..city.turret_hp.len())
                    .filter(|i| city.turret_hp[*i] > 0.0)
                    .map(|i| turret_world(city, i, shape))
                    .min_by(|a, b| shape.distance(at, *a).total_cmp(&shape.distance(at, *b)));
                if let Some(turret) = turret {
                    return Some((turret, WEAPON_RANGE * 0.75));
                }
                // Then the buildings, then the hall (the retail capture rule:
                // the hall counts once every other building is down).
                if let Some((_, building)) = nearest_building(city, at, shape) {
                    return Some((building, WEAPON_RANGE * 0.75));
                }
            }
            Some((center, CITY_REACH - 1.5 * CITY_SCALE))
        }
        UnitOrder::Unit(id) => positions.get(&id).map(|p| (*p, WEAPON_RANGE * 0.75)),
        // The epics' positions ride in the same map under `EPIC_KEY | id`.
        UnitOrder::Epic(id) => positions.get(&(EPIC_KEY | id)).map(|p| (*p, WEAPON_RANGE * 0.75)),
        UnitOrder::Hut(h) => campaign
            .hut_at(h)
            .filter(|_| campaign.hut_standing(h))
            .map(|p| (p, crate::civ_orders::HUT_STOP)),
        UnitOrder::Geyser(g) => {
            let p = campaign.geyser_at(g);
            let owner = campaign.geysers.get(g).and_then(|s| s.owner);
            let stop = match owner {
                Some(o) if o != unit.nation && unit.kind == Approach::Military => {
                    WEAPON_RANGE * 0.7
                }
                _ => 0.6 * CITY_SCALE,
            };
            Some((p, stop))
        }
    }
}

/// Drive every vehicle toward its order: arrive, keep apart, flow around
/// city walls (aircraft fly over them). 3D tangent-space integration on the
/// planet (planet-re 4C): the velocity lives in the tangent frame at the
/// vehicle (`civ_planet::Shape`), a step moves along the surface and
/// `terrain::ground` snaps it back on (`terrain::Radial`); land vehicles keep
/// to the land, sea vehicles to the sea, aircraft fly anywhere.
pub fn rts_move(
    time: Res<Time>,
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut units: Query<(&mut CivUnit, &mut Transform)>,
    epics: Query<(&crate::civ_wildlife::CivEpic, &Transform), Without<CivUnit>>,
    mut nav: Local<Option<(u64, crate::civ_path::NavGrid)>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let campaign = &run.civ_campaign;
    let shape = campaign.shape();
    let surface = surface
        .as_deref()
        .filter(|s| s.stage == crate::stages::GameStage::Civilization);
    let globe = Globe::from_surface(surface);
    // The planet's navigation grid, sampled once per layout.
    if globe.on_planet() && nav.as_ref().map(|(k, _)| *k) != Some(campaign.layout_key) {
        *nav = crate::civ_path::NavGrid::build(&globe).map(|g| (campaign.layout_key, g));
    }
    let grid = nav.as_ref().filter(|_| globe.on_planet()).map(|(_, g)| g);
    let snapshot: Vec<(u32, bool, Vec3)> = units
        .iter()
        .map(|(u, t)| (u.id, u.air, t.translation))
        .collect();
    let mut positions: HashMap<u32, Vec3> = units.iter().map(|(u, t)| (u.id, t.translation)).collect();
    positions.extend(epics.iter().filter(|(e, _)| e.hp > 0.0).map(|(e, t)| (EPIC_KEY | e.id, t.translation)));
    for (mut unit, mut t) in &mut units {
        let pos = t.translation;
        unit.stunned = (unit.stunned - dt).max(0.0);
        unit.bribed = (unit.bribed - dt).max(0.0);
        if unit.stunned > 0.0 {
            // Static Bomb: immobilized.
            unit.velocity = Vec2::ZERO;
            unit.action = UnitAction::Idle;
            continue;
        }
        let speed = unit.combat().speed * civ::engine_factor(run.engine);
        if let UnitOrder::Unit(id) = unit.order {
            if !positions.contains_key(&id) {
                unit.order = UnitOrder::Idle;
            }
        }
        // The epic fell or the hut was taken: the order is done.
        match unit.order {
            UnitOrder::Epic(id) if !positions.contains_key(&(EPIC_KEY | id)) => unit.order = UnitOrder::Idle,
            UnitOrder::Hut(h) if !campaign.hut_standing(h) => unit.order = UnitOrder::Idle,
            _ => {}
        }
        let mut desired = Vec2::ZERO;
        if let Some((target, stop)) = goal_of(&unit, pos, campaign, &positions, shape) {
            let d = shape.distance(pos, target);
            // Land and sea vehicles follow waypoints when the straight way
            // crosses the wrong medium (ADAPTED planner, `civ_path`).
            let mut steer = target;
            if let (Some(grid), true) = (grid, d > stop && unit.domain != civ::VehicleDomain::Air) {
                let land = unit.domain == civ::VehicleDomain::Land;
                unit.path_age += dt;
                let moved = unit.path_goal.map_or(true, |g| shape.distance(g, target) > 20.0);
                if (moved && unit.path_age > 1.0) || (unit.path.is_empty() && unit.path_age > 3.0) {
                    unit.path = if crate::civ_path::clear(&globe, pos, target, land, 8.0) {
                        Vec::new()
                    } else {
                        grid.find(pos, target, land).unwrap_or_default()
                    };
                    unit.path_goal = Some(target);
                    unit.path_age = 0.0;
                }
                while unit.path.len() > 1 && shape.distance(pos, unit.path[0]) < 12.0 {
                    unit.path.remove(0);
                }
                if unit.path.len() > 1 {
                    steer = unit.path[0];
                }
            }
            if d > stop {
                let to = shape.to_tangent(pos, steer - pos).normalize_or_zero();
                desired = to * speed * ((d - stop) / (2.0 * CITY_SCALE)).clamp(0.3, 1.0);
            } else if matches!(unit.order, UnitOrder::Move(_) | UnitOrder::Goto(_)) {
                unit.order = UnitOrder::Idle;
            }
        }
        // Keep apart from vehicles at the same altitude.
        let apart = 2.0 * CITY_SCALE;
        for (id, air, other) in &snapshot {
            if *id == unit.id || *air != unit.air {
                continue;
            }
            let off = shape.to_tangent(pos, pos - *other);
            let d = off.length();
            if d < apart && d > 1e-3 {
                desired += off / d * (apart - d) * 3.0;
            }
        }
        // Land vehicles flow around city walls.
        if unit.domain == civ::VehicleDomain::Land {
            for city in &campaign.cities {
                let off = shape.to_tangent(pos, pos - city.center());
                let d = off.length().max(1e-3);
                let reach = CITY_CLEARANCE + 2.5 * CITY_SCALE;
                if d < reach {
                    let away = off / d;
                    let push = ((reach - d) / (2.5 * CITY_SCALE)).clamp(0.0, 1.6);
                    let tangent = Vec2::new(-away.y, away.x);
                    let side = if desired.dot(tangent) >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    };
                    if desired.dot(away) < 0.0 {
                        desired += tangent * side * push * speed;
                        desired -= away * desired.dot(away) * push.min(1.0);
                    }
                    desired += away * push * speed * 0.6;
                }
            }
        }
        let desired = desired.clamp_length_max(speed);
        let blend = (6.0 * dt).min(1.0);
        unit.velocity = unit.velocity.lerp(desired, blend);
        let mut next = shape.advance(pos, shape.from_tangent(pos, unit.velocity * dt));
        // Stay on land (land vehicles) or at sea (sea vehicles). ADAPTED
        // steering (retail pathing not traced): a blocked step turns up to 90
        // degrees either way, else the vehicle stops at the shore.
        if globe.on_planet() && unit.domain != civ::VehicleDomain::Air && unit.velocity != Vec2::ZERO {
            let wants_land = unit.domain == civ::VehicleDomain::Land;
            let fits = |p: Vec3| globe.is_land(p) == wants_land;
            if !fits(next) && fits(pos) {
                let turned = [0.5f32, -0.5, 1.0, -1.0, 1.57, -1.57].into_iter().find_map(|turn| {
                    let v = Vec2::from_angle(turn).rotate(unit.velocity);
                    let p = shape.advance(pos, shape.from_tangent(pos, v * dt));
                    fits(p).then_some((v, p))
                });
                match turned {
                    Some((v, p)) => {
                        unit.velocity = v;
                        next = p;
                    }
                    None => {
                        unit.velocity = Vec2::ZERO;
                        next = pos;
                        // A move the shore blocks is given up.
                        if matches!(unit.order, UnitOrder::Move(_) | UnitOrder::Goto(_)) {
                            unit.order = UnitOrder::Idle;
                        }
                    }
                }
            }
        }
        t.translation = next;
        let moving = unit.velocity.length() > 0.35;
        if moving {
            let yaw = (-unit.velocity.x).atan2(-unit.velocity.y);
            unit.yaw = angle_lerp(unit.yaw, yaw, 8.0 * dt);
        }
        unit.action = if moving {
            UnitAction::Moving
        } else {
            UnitAction::Idle
        };
        if unit.order == UnitOrder::Idle {
            unit.idle += dt;
        } else {
            unit.idle = 0.0;
        }
        unit.hit += dt;
        unit.cultural_hit += dt;
    }
}

/// The shooting, preaching, trading and claiming of every vehicle, and the
/// city turrets' fire.
#[allow(clippy::too_many_arguments)]
pub fn rts_combat(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut state: ResMut<RtsState>,
    assets: Res<RtsAssets>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut units: Query<(&mut CivUnit, &Transform)>,
    epics: Query<(&crate::civ_wildlife::CivEpic, &Transform), Without<CivUnit>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let Some(root) = state.fx_root else {
        return;
    };
    let _ = &surface;
    let shape = run.civ_campaign.shape();
    let up = |p: Vec3| shape.up(p);
    let dist_xz = |a: Vec3, b: Vec3| shape.distance(a, b);
    let epic_spots: Vec<(u32, Vec3, f32)> = epics
        .iter()
        .filter(|(e, _)| e.hp > 0.0)
        .map(|(e, t)| (e.id, t.translation, crate::civ_wildlife::EPIC_BODY_RADIUS))
        .collect();
    let run = &mut *run;
    run.civ_campaign.ensure_cities();
    state.preaching.clear();
    state.preach_groups.clear();
    state.chatter -= dt;
    let snapshot: Vec<(u32, usize, Vec3)> = units
        .iter()
        .map(|(u, t)| (u.id, u.nation, t.translation))
        .collect();
    let mut chatter: Option<String> = None;
    for (mut unit, t) in &mut units {
        unit.cooldown -= dt;
        let at = t.translation;
        let muzzle = at + up(at) * 0.9 * CITY_SCALE * VEHICLE_DRAW_RATIO;
        let campaign = &mut run.civ_campaign;
        // Static Bomb: a stunned vehicle can neither act nor fire.
        if unit.stunned > 0.0 {
            unit.action = UnitAction::Idle;
            continue;
        }
        // The longest range of the vehicle's tools (100 / 125 / 130); ADAPTED:
        // ships reach a little further to hit coastal cities from the water.
        let weapon_range = if unit.domain == civ::VehicleDomain::Sea {
            unit.stats.range + 3.5 * CITY_SCALE
        } else {
            unit.stats.range
        };
        // --- military fire (and religious fire at an ordered vehicle: the retail command
        // handler 0xCF4322 sends every non-economic vehicle against a vehicle; buildings,
        // geysers and the auto-aggro stay military, the religious vehicles convert)
        let strikes_buildings = unit.kind == Approach::Military;
        if matches!(unit.kind, Approach::Military | Approach::Religious) {
            let explicit: Option<(ShotTarget, Vec3)> = match unit.order {
                UnitOrder::Unit(id) => snapshot
                    .iter()
                    .find(|u| u.0 == id && u.1 != unit.nation)
                    .filter(|u| at.distance(u.2) <= weapon_range)
                    .map(|u| (ShotTarget::Unit(id), u.2 + up(u.2) * 0.5 * CITY_SCALE * VEHICLE_DRAW_RATIO)),
                // The attack order on an epic predator (0xDCC5A0 type 1): fire once it is in range.
                UnitOrder::Epic(id) if strikes_buildings => epic_spots
                    .iter()
                    .find(|e| e.0 == id)
                    .filter(|e| at.distance(e.1) <= weapon_range + e.2)
                    .map(|e| (ShotTarget::Epic(e.0), e.1 + up(e.1) * 1.2 * CITY_SCALE)),
                UnitOrder::City(c)
                    if strikes_buildings
                        && campaign
                            .cities
                            .get(c)
                            .is_some_and(|city| city.nation() != unit.nation) =>
                {
                    let city = &campaign.cities[c];
                    let turret = (0..city.turret_hp.len())
                        .filter(|i| city.turret_hp[*i] > 0.0)
                        .map(|i| (i, turret_world(city, i, shape)))
                        .filter(|(_, p)| at.distance(*p) <= weapon_range + 0.5 * CITY_SCALE)
                        .min_by(|a, b| at.distance(a.1).total_cmp(&at.distance(b.1)));
                    // ADAPTED order: the turrets in range, then the nearest
                    // building in range, then the hall once it is exposed
                    // (VERIFIED 0xBD9E70: every other building down).
                    let building = nearest_building(city, at, shape)
                        .filter(|(_, p)| at.distance(*p) <= weapon_range + 0.5 * CITY_SCALE);
                    if let Some((i, p)) = turret {
                        Some((ShotTarget::Turret(c, i), p))
                    } else if let Some((plot, p)) = building {
                        Some((ShotTarget::Building(c, plot), p))
                    } else if city.hall_exposed() && dist_xz(at, city.center()) <= CITY_REACH {
                        Some((
                            ShotTarget::City(c),
                            city.center() + up(city.center()) * 2.0 * CITY_SCALE,
                        ))
                    } else {
                        None
                    }
                }
                // 0xBD7F70 / 0x06776CA9: the player's preachers ordered against a city above mood 50
                // shoot its entertainment buildings first (civ_orders::convert_locked).
                UnitOrder::City(c)
                    if unit.kind == Approach::Religious
                        && unit.nation == HOME_INDEX
                        && campaign
                            .cities
                            .get(c)
                            .is_some_and(|city| city.nation() != unit.nation && crate::civ_orders::convert_locked(city)) =>
                {
                    crate::civ_orders::nearest_entertainment(&campaign.cities[c], at, shape)
                        .filter(|(_, p)| at.distance(*p) <= weapon_range + 0.5 * CITY_SCALE)
                        .map(|(plot, p)| (ShotTarget::Building(c, plot), p))
                }
                UnitOrder::Geyser(g) if strikes_buildings => {
                    let owner = campaign.geysers.get(g).and_then(|s| s.owner);
                    let p = campaign.geyser_at(g);
                    match owner {
                        Some(o)
                            if o != unit.nation
                                && hostile_or_aggressor(campaign, unit.nation, o)
                                && dist_xz(at, p) <= weapon_range =>
                        {
                            Some((ShotTarget::Derrick(g), p + up(p) * 1.4 * CITY_SCALE))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            let reach = AGGRO_RANGE.min(weapon_range + CITY_SCALE);
            let target = if unit.bribed > 0.0 && strikes_buildings {
                // Bribe Bomb: shoot the nearest other enemy vehicle instead.
                snapshot
                    .iter()
                    .filter(|u| u.0 != unit.id && u.1 != HOME_INDEX)
                    .filter(|u| at.distance(u.2) <= weapon_range)
                    .min_by(|a, b| at.distance(a.2).total_cmp(&at.distance(b.2)))
                    .map(|u| (ShotTarget::Unit(u.0), u.2 + up(u.2) * 0.5 * CITY_SCALE * VEHICLE_DRAW_RATIO))
            } else {
                explicit.or_else(|| {
                    if !strikes_buildings
                        || !matches!(
                            unit.order,
                            UnitOrder::Idle | UnitOrder::City(_) | UnitOrder::Geyser(_)
                        )
                    {
                        return None;
                    }
                    snapshot
                        .iter()
                        .filter(|u| hostile(campaign, unit.nation, u.1))
                        .filter(|u| at.distance(u.2) <= reach)
                        .min_by(|a, b| at.distance(a.2).total_cmp(&at.distance(b.2)))
                        .map(|u| (ShotTarget::Unit(u.0), u.2 + up(u.2) * 0.5 * CITY_SCALE * VEHICLE_DRAW_RATIO))
                        .or_else(|| {
                            // Epic predators are fair game for everyone.
                            epic_spots
                                .iter()
                                .filter(|e| at.distance(e.1) <= reach + e.2)
                                .min_by(|a, b| at.distance(a.1).total_cmp(&at.distance(b.1)))
                                .map(|e| (ShotTarget::Epic(e.0), e.1 + up(e.1) * 1.2 * CITY_SCALE))
                        })
                })
            };
            if let Some((target, aim)) = target {
                unit.yaw = angle_lerp(unit.yaw, shape.yaw_of(at, aim - at), 10.0 * dt);
                unit.action = UnitAction::Firing;
                if unit.cooldown <= 0.0 {
                    // `VehicleWeaponCycleTime` 2 s; one hit = the damage
                    // multiplier of the vehicle's stats (0x4E74D0).
                    unit.cooldown = SHOT_COOLDOWN;
                    let damage = unit.combat().damage;
                    fire(
                        &mut commands,
                        &assets,
                        root,
                        muzzle,
                        aim,
                        target,
                        damage,
                        unit.nation,
                        if strikes_buildings { 0 } else { shot_style(unit.kind) },
                    );
                }
            }
        }
        // --- preaching and trading at a rival city
        if let UnitOrder::City(c) = unit.order {
            let reach = campaign.cities.get(c).is_some_and(|city| {
                city.nation() != unit.nation && dist_xz(at, city.center()) <= CITY_REACH
            });
            if reach && unit.nation == HOME_INDEX {
                match unit.kind {
                    // Above mood 50 the preachers shoot entertainment instead (civ_orders).
                    Approach::Religious if crate::civ_orders::convert_locked(&campaign.cities[c]) => {}
                    Approach::Religious => {
                        let city = &mut campaign.cities[c];
                        // The vehicle joins the group of its nation performing at
                        // the city (0xBD8710: size and `(power stat + 0.5) x 2/3`);
                        // the group's step runs once a second after the loop.
                        let group = state.preach_groups.entry((c, unit.nation)).or_insert((0, 0.0));
                        group.0 += 1;
                        group.1 += civ_rules::vehicle_group_power(unit.stats.power);
                        // 0xBEB310 -> 0xC00C40(1): the city is culturally
                        // attacked (its rival AI only raises turrets).
                        city.mark_preached();
                        unit.action = UnitAction::Preaching(c);
                        state.preaching.insert(c);
                        chatter = Some(format!(
                            "{}: converting {:.0}%",
                            city.label(),
                            city.conversion * 100.0
                        ));
                    }
                    Approach::Economic => {
                        unit.action = UnitAction::Trading(c);
                        let city = &mut campaign.cities[c];
                        if !city.trade {
                            // 0xDCA090 / 0xCEEF90: the nation accepts when it is Pleased,
                            // or Cautious and fonder of you than any other nation, and
                            // says so on the comm screen.
                            let label = city.label();
                            if campaign.at_war(c) || !civ_comm::trade_route_accepted(campaign, c) {
                                if campaign.comm.refusal_wait <= 0.0 {
                                    campaign.comm.refusal_wait = civ_comm::TRADE_REFUSAL_INTERVAL;
                                    campaign.comm.queue(civ_comm::Incoming {
                                        nation: c,
                                        dialog: civ_comm::dialog::TRADE_ROUTE_REJECTED,
                                        offer: 0.0,
                                    });
                                }
                                chatter = Some(format!("{label} will not trade with you until your relationship improves"));
                            } else if run.spice >= civ::TRADE_COST {
                                run.spice -= civ::TRADE_COST;
                                campaign.cities[c].trade = true;
                                campaign.nations[c].relation.apply(civ_rules::RelationKind::Trade);
                                campaign.comm.queue(civ_comm::Incoming {
                                    nation: c,
                                    dialog: civ_comm::dialog::TRADE_ROUTE_ACCEPTED,
                                    offer: 0.0,
                                });
                                campaign.message = format!(
                                    "Trade route to {label} opened: your economic vehicles shuttle cargo, the city is now for sale"
                                );
                            } else {
                                chatter = Some(format!("{label}: trade rights cost {:.0} spice", civ::TRADE_COST));
                            }
                        }
                    }
                    Approach::Military => {}
                }
            } else if reach && unit.kind == Approach::Religious && campaign.cities[c].owned() {
                // A rival's preachers at one of your cities: the same step
                // (0xBEB310 runs for every political id), toward the city leaving
                // your nation.
                let nation = unit.nation;
                let line = format!("{} is preaching to {}: defend it with turrets and happiness", civ::city_label(nation), campaign.cities[c].label());
                if campaign.cities[c].converter != Some(nation) && campaign.message != line {
                    campaign.message = line;
                }
                let group = state.preach_groups.entry((c, nation)).or_insert((0, 0.0));
                group.0 += 1;
                group.1 += civ_rules::vehicle_group_power(unit.stats.power);
                campaign.cities[c].mark_preached();
                unit.action = UnitAction::Preaching(c);
                state.preaching.insert(c);
            }
        }
        // --- trade shuttles (0xDCC900 / 0xBE19B0 / 0xCEEC30): an economic
        // vehicle on an open route drives between the partner city and your
        // nearest city; at each end it waits 2 s, unloads (partner: owner
        // 8/6, you 4/6 of the cargo, trade value += cargo / 6; at home a third
        // of that, all yours) and loads 30 x (5 power + 0.5 sqrt(housing)).
        if let UnitOrder::City(c) = unit.order {
            let campaign = &mut run.civ_campaign;
            let route = unit.nation == HOME_INDEX
                && unit.kind == Approach::Economic
                && campaign.cities.get(c).is_some_and(|city| city.trade && !city.owned());
            if !route {
                unit.homeward = false;
            } else {
                let partner = campaign.cities[c].center();
                let home = unit.trade_home.filter(|h| campaign.cities.get(*h).is_some_and(|x| x.owned())).or_else(|| {
                    campaign
                        .cities
                        .iter()
                        .filter(|h| h.owned())
                        .min_by(|a, b| dist_xz(a.center(), partner).total_cmp(&dist_xz(b.center(), partner)))
                        .map(|h| h.index)
                });
                unit.trade_home = home;
                let end = if unit.homeward { home } else { Some(c) };
                let at_end = end.is_some_and(|e| dist_xz(at, campaign.cities[e].center()) <= CITY_REACH);
                if at_end {
                    unit.trade_wait += dt;
                    unit.action = UnitAction::Trading(end.unwrap_or(c));
                } else {
                    unit.trade_wait = 0.0;
                }
                if at_end && unit.trade_wait >= crate::civ_combat::TRADE_WAIT {
                    unit.trade_wait = 0.0;
                    let power = unit.stats.power;
                    let label = campaign.cities[c].label();
                    let (gain, here) = if unit.homeward {
                        let (owner, trader) = civ_rules::trade_delivery(unit.cargo, true);
                        let h = home.unwrap_or(HOME_INDEX);
                        unit.cargo = crate::civ_combat::cargo(campaign.cities[h].citizens() as f32, power);
                        (owner + trader, campaign.cities[h].center())
                    } else {
                        let (owner, trader) = civ_rules::trade_delivery(unit.cargo, false);
                        campaign.cities[c].trade_value += unit.cargo / 6.0;
                        let nation = &mut campaign.nations[c];
                        nation.spice = (nation.spice + owner).min(civ_rules::WEALTH_CAP);
                        nation.relation.apply_scaled(civ_rules::RelationKind::Trade, civ_rules::VEHICLE_TRADE_EFFECT);
                        unit.cargo = crate::civ_combat::cargo(campaign.cities[c].citizens() as f32, power);
                        (trader, partner)
                    };
                    unit.homeward = !unit.homeward;
                    if gain > 0.0 {
                        run.spice = (run.spice + gain).min(civ_rules::WEALTH_CAP);
                        chatter = Some(format!("{label} route: cargo delivered, +{gain:.0} spice"));
                        spawn_fx(&mut commands, &assets, root, here + up(here) * 2.0 * CITY_SCALE, FxKind::Puff, 1.2);
                    }
                }
                // The 0.82 per-second "offers" toward the buy price are gone
                // (civ-audit-a): retail pays for a city once, through the buy
                // offer of the communication screen (0xCFF010 -> 0xBF0600(1),
                // `civ_comm`); the route only earns by deliveries (0xBE19B0) and
                // lowers the price by the trade value (0xCEECE0).
            }
        }
        // --- claiming spice
        if let UnitOrder::Geyser(g) = unit.order {
            let p = run.civ_campaign.geyser_at(g);
            if dist_xz(at, p) <= CLAIM_RANGE {
                let campaign = &mut run.civ_campaign;
                match campaign.geysers[g].owner {
                    // A pump is on its way (`mMineState` 1): the builder's other vehicles have nothing to
                    // do here (0xDCB2D6), another nation's wait until the node is theirs or not.
                    None if campaign.geysers[g].claim.is_some() => {
                        if campaign.geysers[g].claim.is_some_and(|c| c.nation == unit.nation) {
                            unit.order = UnitOrder::Idle;
                        }
                    }
                    None => {
                        let purse = if unit.nation == HOME_INDEX {
                            &mut run.spice
                        } else {
                            &mut campaign.nations[unit.nation].spice
                        };
                        if *purse >= civ::DERRICK_COST {
                            *purse -= civ::DERRICK_COST;
                            // 0xDCB2F4 -> 0xBFE590 `StartConstruction`: the node builds for
                            // `BUILD_SECONDS`, fires the pump capsule (`civ_pump`) and is the
                            // vehicle's nation's when the timer runs out (`GeyserState::tick_claim`).
                            campaign.geysers[g].start_claim(unit.nation, at);
                            unit.order = UnitOrder::Idle;
                            if !crate::civ_vfx::on() {
                                spawn_fx(&mut commands, &assets, root, p + up(p) * CITY_SCALE, FxKind::Puff, 1.6);
                            }
                        } else if unit.nation == HOME_INDEX {
                            chatter =
                                Some(format!("A derrick costs {:.0} spice", civ::DERRICK_COST));
                        }
                    }
                    Some(o) if o == unit.nation => unit.order = UnitOrder::Idle,
                    // Command 0xB (civ_orders, 0xCE8C30): an economic vehicle buys another
                    // civilisation's derrick (the "Bribe the Workers" popup).
                    Some(_) if unit.kind == Approach::Economic => {
                        let nation = unit.nation;
                        match crate::civ_orders::bribe(campaign, &mut run.spice, nation, g) {
                            crate::civ_orders::Bribe::Done { price, seller } => {
                                unit.order = UnitOrder::Idle;
                                if nation == HOME_INDEX {
                                    campaign.message = format!(
                                        "You bribed the workers of {}'s derrick for ${price:.0} ({} derricks)",
                                        civ::city_label(seller),
                                        campaign.derricks(HOME_INDEX)
                                    );
                                }
                                spawn_fx(&mut commands, &assets, root, p + up(p) * CITY_SCALE, FxKind::Puff, 1.6);
                            }
                            crate::civ_orders::Bribe::Poor { price } => {
                                if nation == HOME_INDEX {
                                    chatter = Some(format!("Bribing the workers costs ${price:.0}"));
                                }
                            }
                            crate::civ_orders::Bribe::Refused => unit.order = UnitOrder::Idle,
                        }
                    }
                    Some(_) if unit.kind != Approach::Military => unit.order = UnitOrder::Idle,
                    Some(_) => {}
                }
            }
        }
    }
    // --- city turrets
    let campaign = &run.civ_campaign;
    let targets: Vec<(u32, Vec3, CivUnit)> = units
        .iter()
        .map(|(u, t)| (u.id, t.translation, u.clone()))
        .collect();
    let mut volleys = Vec::new();
    for city in &campaign.cities {
        // A city that has not appeared yet (`civ_reveal`) has no turrets.
        if !campaign.present(city.index) {
            continue;
        }
        // Turrets held by a Static Bomb or shut down by a Black Cloud.
        if city.stunned > 0.0 {
            continue;
        }
        if campaign.black_cloud_city.is_some_and(|(c, t)| c == city.index && t > 0.0) {
            continue;
        }
        for i in 0..city.turret_hp.len() {
            if city.turret_hp[i] <= 0.0 {
                state.turret_aim.remove(&(city.index, i));
                continue;
            }
            let cooldown = state.turret_cooldown.entry((city.index, i)).or_insert(0.0);
            *cooldown -= dt;
            let pos = turret_world(city, i, shape);
            let target = targets
                .iter()
                .filter(|(_, p, u)| {
                    pos.distance(*p) <= TURRET_RANGE && turret_hostile(campaign, city.index, u)
                })
                .min_by(|a, b| pos.distance(a.1).total_cmp(&pos.distance(b.1)));
            match target {
                Some((id, p, _)) => {
                    state.turret_aim.insert((city.index, i), *p);
                    let cooldown = state.turret_cooldown.get_mut(&(city.index, i)).unwrap();
                    if *cooldown <= 0.0 {
                        *cooldown = TURRET_COOLDOWN;
                        volleys.push((
                            pos + up(pos) * 0.6 * CITY_SCALE,
                            *p + up(*p) * 0.5 * CITY_SCALE,
                            *id,
                            city.nation(),
                            shot_style(city.kind),
                        ));
                    }
                }
                None => {
                    state.turret_aim.remove(&(city.index, i));
                }
            }
        }
    }
    for (from, to, id, nation, style) in volleys {
        fire(
            &mut commands,
            &assets,
            root,
            from,
            to,
            ShotTarget::Unit(id),
            TURRET_DAMAGE * civ_rules::TURRET_TOOL_DAMAGE[style.min(2)]
                / civ_rules::TURRET_TOOL_DAMAGE[0],
            nation,
            style,
        );
    }
    // The conversion step of 0xBEB310, once a second per performing group.
    if state.preach_groups.is_empty() {
        state.preach_clock = 0.0;
    } else {
        state.preach_clock += dt;
        if state.preach_clock >= civ_rules::CIV_UPDATE_SECONDS {
            state.preach_clock -= civ_rules::CIV_UPDATE_SECONDS;
            let groups: Vec<civ::PreachGroup> = state
                .preach_groups
                .iter()
                .map(|(&(city, nation), &(size, power))| civ::PreachGroup { city, nation, size, power })
                .collect();
            run.civ_campaign.preach_second(&groups);
        }
    }
    if let Some(line) = chatter {
        if state.chatter <= 0.0 {
            state.chatter = 1.0;
            run.civ_campaign.message = line;
        }
    }
}

/// A rival may shoot your derricks only at war; you may shoot anyone's.
fn hostile_or_aggressor(campaign: &CivCampaign, shooter: usize, owner: usize) -> bool {
    shooter == HOME_INDEX || hostile(campaign, shooter, owner)
}

/// A hit by the player on a nation: the Hostility event of every hit (VERIFIED
/// 0xBCF116 / 0xBCFC5A / 0xBD0807 / 0xCABA7A), the Religion event instead for a
/// hit of a cultural weapon (damage type 3: 0xBFFD2C / 0xCABA73 / 0xCBAB06 /
/// 0xCBACD9 / 0xCBAD15), and war once that makes the nation Annoyed or worse.
fn provoke(campaign: &mut CivCampaign, attacker: usize, victim: usize, cultural: bool) {
    if attacker == HOME_INDEX && victim != HOME_INDEX && campaign.nation_alive(victim) {
        if let Some(nation) = campaign.nations.get_mut(victim) {
            if cultural {
                nation.relation.cultural_attacked();
            } else {
                nation.relation.attacked();
            }
            if !nation.war && nation.relation.hostile() {
                nation.war = true;
                campaign.message = format!(
                    "{} declares war on you: its turrets and vehicles will fire on sight",
                    civ::city_label(victim)
                );
            }
        }
    }
}

/// Fly every shot; on arrival it damages its target (vehicles, turrets,
/// city armour, derricks) and bursts.
pub fn rts_shots(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    state: Res<RtsState>,
    assets: Res<RtsAssets>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut shots: Query<(Entity, &mut CivShot)>,
    mut units: Query<(&mut CivUnit, &Transform)>,
    mut epics: Query<&mut crate::civ_wildlife::CivEpic>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let Some(root) = state.fx_root else {
        return;
    };
    let positions: HashMap<u32, Vec3> = units.iter().map(|(u, t)| (u.id, t.translation)).collect();
    let campaign = &mut run.civ_campaign;
    let shape = campaign.shape();
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == crate::stages::GameStage::Civilization),
    );
    for (entity, mut shot) in &mut shots {
        if let ShotTarget::Unit(id) = shot.target {
            if let Some(p) = positions.get(&id) {
                shot.to = *p + shape.up(*p) * 0.5 * CITY_SCALE;
            }
        }
        shot.t += dt / shot.duration;
        if shot.t < 1.0 {
            continue;
        }
        commands.entity(entity).despawn();
        let mut burst = 1.0;
        match shot.target {
            ShotTarget::Unit(id) => {
                for (mut unit, _) in &mut units {
                    if unit.id == id {
                        let damage = if unit.nation == HOME_INDEX && campaign.invulnerability_timer > 0.0 {
                            0.0
                        } else {
                            shot.damage
                        };
                        unit.hp -= damage;
                        unit.hit = 0.0;
                        if shot.cultural && damage > 0.0 {
                            unit.cultural_hit = 0.0;
                        }
                        let victim = unit.nation;
                        provoke(campaign, shot.nation, victim, shot.cultural);
                        break;
                    }
                }
            }
            ShotTarget::Epic(id) => {
                if let Some(mut epic) = epics.iter_mut().find(|e| e.id == id) {
                    epic.hp -= shot.damage;
                    epic.hit = 0.0;
                }
            }
            ShotTarget::Turret(c, i) => {
                provoke(campaign, shot.nation, c, shot.cultural);
                if let Some(city) = campaign.cities.get_mut(c) {
                    if city.damage_turret(i, shot.damage) {
                        burst = 2.6;
                        campaign.message = format!("{} lost a wall turret", civ::city_label(c));
                    }
                }
            }
            ShotTarget::Building(c, plot) => {
                if campaign.cities.get(c).is_some_and(|city| !city.owned()) {
                    provoke(campaign, shot.nation, c, shot.cultural);
                }
                if let Some(city) = campaign.cities.get_mut(c) {
                    if city.damage_building(plot, shot.damage, shot.nation) {
                        burst = 2.4;
                        let name = city.plots[plot].map_or("building", |b| b.title());
                        campaign.message = format!(
                            "{} lost its {name} (captured {:.0}%)",
                            civ::city_label(c),
                            city.capture
                        );
                    }
                }
            }
            ShotTarget::City(c) => {
                if campaign.cities.get(c).is_some_and(|city| !city.owned()) {
                    provoke(campaign, shot.nation, c, shot.cultural);
                }
                if let Some(city) = campaign.cities.get_mut(c) {
                    city.damage_hall(shot.damage, shot.nation);
                }
            }
            ShotTarget::Derrick(g) => {
                if let Some(owner) = campaign.geysers.get(g).and_then(|s| s.owner) {
                    provoke(campaign, shot.nation, owner, shot.cultural);
                    let geyser = &mut campaign.geysers[g];
                    geyser.hp -= shot.damage;
                    if geyser.hp <= 0.0 {
                        geyser.owner = None;
                        geyser.hp = 0.0;
                        burst = 2.4;
                        if owner == HOME_INDEX {
                            campaign.message = "One of your spice derricks was destroyed".into();
                        }
                    }
                }
            }
        }
        // Never under the ground.
        let up = shape.up(shot.to);
        let floor = globe.lift(shot.to, 0.3 * CITY_SCALE);
        let at = if (shot.to - floor).dot(up) < 0.0 {
            floor
        } else {
            shot.to
        };
        spawn_fx(&mut commands, &assets, root, at, FxKind::Boom, 0.7 * burst);
        if burst > 1.0 {
            spawn_fx(
                &mut commands,
                &assets,
                root,
                at + up * 0.5 * CITY_SCALE,
                FxKind::Puff,
                1.4 * burst,
            );
        }
    }
}

/// Wrecks, vehicles of vanished nations and the twice-a-second snapshot of
/// every vehicle into the campaign save.
pub fn rts_cleanup(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut state: ResMut<RtsState>,
    assets: Res<RtsAssets>,
    mut units: Query<(Entity, &mut CivUnit, &Transform)>,
) {
    let Some(root) = state.fx_root else {
        return;
    };
    let mut gone: HashSet<Entity> = HashSet::new();
    for (entity, mut unit, t) in &mut units {
        let campaign = &mut run.civ_campaign;
        if unit.hp <= 0.0 {
            spawn_fx(
                &mut commands,
                &assets,
                root,
                t.translation + Vec3::Y * 0.6,
                FxKind::Boom,
                2.2,
            );
            spawn_fx(
                &mut commands,
                &assets,
                root,
                t.translation + Vec3::Y * 1.2,
                FxKind::Puff,
                2.0,
            );
            commands.entity(entity).despawn();
            gone.insert(entity);
            if unit.nation == HOME_INDEX {
                campaign.vehicles_lost += 1;
                campaign.message = format!(
                    "A {} vehicle was destroyed ({} lost)",
                    unit.kind.title().to_lowercase(),
                    campaign.vehicles_lost
                );
            }
            continue;
        }
        if !campaign.nation_alive(unit.nation) {
            // A conquered nation's vehicles stand down.
            spawn_fx(
                &mut commands,
                &assets,
                root,
                t.translation + Vec3::Y * 0.8,
                FxKind::Puff,
                1.6,
            );
            commands.entity(entity).despawn();
            gone.insert(entity);
            continue;
        }
        // Orders against a city that is now yours end there.
        if let UnitOrder::City(c) = unit.order {
            if unit.nation == HOME_INDEX
                && campaign.cities.get(c).is_some_and(|city| city.owned())
                && unit.kind != Approach::Military
            {
                unit.order = UnitOrder::Idle;
            }
        }
    }
    state.snapshot += time.delta_secs();
    if state.snapshot >= SNAPSHOT_PERIOD {
        state.snapshot = 0.0;
        let mut snapshot: Vec<UnitState> = units
            .iter()
            .filter(|(e, ..)| !gone.contains(e))
            .map(|(_, u, t)| u.state(t.translation))
            .collect();
        snapshot.sort_by_key(|u| u.id);
        run.civ_campaign.units = snapshot;
    }
}

// ---------------------------------------------------------------------------
// Presentation
// ---------------------------------------------------------------------------

fn terrain_normal(surface: Option<&crate::terrain::LandSurface>, x: f32, z: f32) -> Vec3 {
    let Some(s) = surface else {
        return Vec3::Y;
    };
    if let Some(planet) = s.planet() {
        let p = Vec3::new(x, planet.surface_y(x, z), z);
        return planet.ground_normal_world(p);
    }
    let e = 0.6;
    Vec3::new(
        s.height(x - e, z) - s.height(x + e, z),
        2.0 * e,
        s.height(x, z - e) - s.height(x, z + e),
    )
    .normalize_or(Vec3::Y)
}

/// Vehicles lean with the ground (aircraft bank and bob), show their ring
/// when selected and a camera-facing health bar when hurt or selected.
pub fn present_units(
    time: Res<Time>,
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut units: Query<(&CivUnit, &mut Transform, &UnitRig, Has<Selected>)>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<CivUnit>>,
) {
    let now = time.elapsed_secs();
    let camera_rotation = camera
        .iter()
        .next()
        .map(|g| g.compute_transform().rotation)
        .unwrap_or(Quat::IDENTITY);
    let shape = run.civ_campaign.shape();
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == crate::stages::GameStage::Civilization),
    );
    for (unit, mut t, rig, selected) in &mut units {
        // Facing its yaw on the local up (the tangent frame of `civ_planet`).
        let heading = shape.heading(t.translation, unit.yaw);
        let rotation = if unit.air {
            let bank = (unit.velocity.length() / CITY_SCALE * 0.04).min(0.25)
                * (now * 1.3 + unit.id as f32).sin();
            heading * Quat::from_rotation_z(bank)
        } else if globe.on_planet() {
            let normal = globe.ground_normal(t.translation);
            let normal = if unit.domain == civ::VehicleDomain::Sea {
                shape.up(t.translation)
            } else {
                normal
            };
            Quat::from_rotation_arc(shape.up(t.translation), normal) * heading
        } else {
            let normal = terrain_normal(surface.as_deref(), t.translation.x, t.translation.z);
            Quat::from_rotation_arc(Vec3::Y, normal) * heading
        };
        t.rotation = rotation;
        if let Ok((mut art, _)) = parts.get_mut(rig.art) {
            let bob = if unit.air {
                (now * 2.0 + unit.id as f32 * 0.7).sin() * 0.18
            } else {
                0.0
            };
            // A hit jolts the vehicle.
            let jolt = if unit.hit < 0.15 { 0.12 } else { 0.0 };
            art.translation.y = rig.lift + bob + jolt;
        }
        if let Ok((mut ring, mut visibility)) = parts.get_mut(rig.ring) {
            // The retail circle effect (civ_fx::selection_effects) replaces the
            // torus; the torus entity stays as the effect's size / height guide.
            let want = if selected && !crate::civ_fx::selection_effect_on() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != want {
                *visibility = want;
            }
            ring.translation.y = if unit.air {
                -AIR_CLEARANCE / CITY_SCALE + 0.08
            } else {
                0.06
            };
        }
        let frac = (unit.hp / unit.max_hp).clamp(0.0, 1.0);
        let show_bar = selected || frac < 0.999;
        if let Ok((mut bar, mut visibility)) = parts.get_mut(rig.bar) {
            let want = if show_bar {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != want {
                *visibility = want;
            }
            bar.rotation = rotation.inverse() * camera_rotation;
        }
        if let Ok((mut fill, _)) = parts.get_mut(rig.fill) {
            fill.scale.x = 1.6 * frac.max(0.02);
            fill.translation.x = -0.8 * (1.0 - frac);
        }
    }
}

/// Shots fly on arcs, bursts swell and fade, order markers shrink.
pub fn present_fx(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    roots: Query<&Transform, (With<FxRoot>, Without<CivShot>, Without<CivFx>)>,
    mut shots: Query<(&CivShot, &mut Transform), (Without<CivFx>, Without<FxRoot>)>,
    mut effects: Query<(Entity, &mut CivFx, &mut Transform), (Without<CivShot>, Without<FxRoot>)>,
) {
    let dt = time.delta_secs();
    let origin = roots
        .iter()
        .next()
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    let shape = run.civ_campaign.shape();
    for (shot, mut t) in &mut shots {
        let s = shot.t.clamp(0.0, 1.0);
        let up = shape.up(shot.from.lerp(shot.to, 0.5));
        let pos = shot.from.lerp(shot.to, s) + up * shot.arc * 4.0 * s * (1.0 - s);
        let ahead = (shot.t + 0.05).clamp(0.0, 1.0);
        let next = shot.from.lerp(shot.to, ahead) + up * shot.arc * 4.0 * ahead * (1.0 - ahead);
        let dir = (next - pos).normalize_or(Vec3::Y);
        t.translation = pos - origin;
        t.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
    }
    for (entity, mut fx, mut t) in &mut effects {
        fx.age += dt;
        if fx.age >= fx.life {
            commands.entity(entity).despawn();
            continue;
        }
        let k = fx.age / fx.life;
        let (scale, rise) = match fx.kind {
            FxKind::Boom => (
                fx.size * (0.35 + 0.65 * (k * 2.0).min(1.0)) * (1.0 - k * k),
                0.0,
            ),
            FxKind::Spark => (fx.size * (1.0 - k), 0.0),
            FxKind::Marker => (fx.size * (1.0 - 0.7 * k), 0.0),
            FxKind::Puff => (fx.size * (0.4 + 0.6 * k), 1.6 * CITY_SCALE * k),
        };
        let up = shape.up(fx.pos);
        t.translation = fx.pos - origin + up * rise;
        if fx.kind == FxKind::Marker {
            t.rotation = shape.frame(fx.pos);
        }
        t.scale = if fx.kind == FxKind::Marker {
            Vec3::new(scale, 0.05 * CITY_SCALE, scale)
        } else {
            Vec3::splat(scale.max(0.01))
        };
    }
}

/// Preaching and trading beams, the halo over a city being converted, the
/// trade convoys on open routes and the drag-select box.
#[allow(clippy::too_many_arguments)]
pub fn present_actions(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    state: Res<RtsState>,
    assets: Res<RtsAssets>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    roots: Query<
        &Transform,
        (
            With<FxRoot>,
            Without<ActionBeam>,
            Without<PreachHalo>,
            Without<TradeCargo>,
        ),
    >,
    units: Query<
        (&CivUnit, &Transform),
        (
            Without<ActionBeam>,
            Without<PreachHalo>,
            Without<TradeCargo>,
            Without<FxRoot>,
        ),
    >,
    mut beams: Query<
        (
            Entity,
            &ActionBeam,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (Without<PreachHalo>, Without<TradeCargo>),
    >,
    mut halos: Query<
        (&PreachHalo, &mut Transform, &mut Visibility),
        (Without<ActionBeam>, Without<TradeCargo>),
    >,
    mut cargo: Query<
        (Entity, &TradeCargo, &mut Transform),
        (Without<ActionBeam>, Without<PreachHalo>),
    >,
    mut drag: Query<&mut Node, With<DragBox>>,
) {
    let Some(root) = state.fx_root else {
        return;
    };
    let now = time.elapsed_secs();
    let origin = roots.get(root).map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == crate::stages::GameStage::Civilization),
    );
    let campaign = &run.civ_campaign;
    let shape = campaign.shape();
    let up = |p: Vec3| shape.up(p);
    let city_top = |c: usize| {
        let center = campaign.cities[c].center();
        center + up(center) * 6.5 * CITY_SCALE
    };
    // Beams from every preaching or trading vehicle to its city.
    let mut wanted: HashMap<u32, (Vec3, Vec3, bool)> = HashMap::new();
    for (unit, t) in &units {
        let at = t.translation;
        match unit.action {
            UnitAction::Preaching(c) => {
                wanted.insert(unit.id, (at + up(at) * 1.3 * CITY_SCALE, city_top(c), true));
            }
            // civ-visuals2: the retail trade look is the cargo balloons of `civ_vfx`; the glass beam
            // only stays with `OPENSPORE_CIV_VFX=0`.
            UnitAction::Trading(c) if !crate::civ_vfx::on() => {
                let top = city_top(c);
                wanted.insert(
                    unit.id,
                    (
                        at + up(at) * CITY_SCALE,
                        top - up(top) * 3.0 * CITY_SCALE,
                        false,
                    ),
                );
            }
            _ => {}
        }
    }
    for (entity, beam, mut t, mut material) in &mut beams {
        match wanted.remove(&beam.0) {
            Some((from, to, preach)) => {
                let flicker = 1.0 + 0.35 * (now * 18.0 + beam.0 as f32).sin();
                let length = from.distance(to).max(0.01);
                t.translation = (from + to) * 0.5 - origin;
                t.rotation = Quat::from_rotation_arc(Vec3::Y, (to - from) / length);
                t.scale = Vec3::new(flicker * CITY_SCALE, length, flicker * CITY_SCALE);
                let want = if preach {
                    &assets.preach_mat
                } else {
                    &assets.trade_mat
                };
                if material.0 != *want {
                    material.0 = want.clone();
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, (from, to, preach)) in wanted {
        let length = from.distance(to).max(0.01);
        commands.spawn((
            StageOwned,
            ActionBeam(id),
            Mesh3d(assets.beam.clone()),
            MeshMaterial3d(if preach {
                assets.preach_mat.clone()
            } else {
                assets.trade_mat.clone()
            }),
            Transform::from_translation((from + to) * 0.5 - origin)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, (to - from) / length))
                .with_scale(Vec3::new(CITY_SCALE, length, CITY_SCALE)),
            NotShadowCaster,
            ChildOf(root),
        ));
    }
    // A golden halo turns over every city being preached to.
    let mut have_halo = HashSet::new();
    for (halo, mut t, mut visibility) in &mut halos {
        have_halo.insert(halo.0);
        let active = state.preaching.contains(&halo.0);
        let want = if active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
        if active {
            let city = &campaign.cities[halo.0];
            let conversion = if city.owned() { city.rival_conversion } else { city.conversion };
            let top = city_top(halo.0);
            t.translation = top + up(top) * (1.5 + 0.3 * (now * 2.0).sin()) * CITY_SCALE - origin;
            t.rotation =
                shape.frame(top) * Quat::from_rotation_y(now * 1.5) * Quat::from_rotation_x(0.25);
            t.scale = Vec3::new(0.6 + conversion, 0.2, 0.6 + conversion) * CITY_SCALE;
        }
    }
    for c in state.preaching.iter().copied() {
        if !have_halo.contains(&c) {
            commands.spawn((
                StageOwned,
                PreachHalo(c),
                Mesh3d(assets.halo.clone()),
                MeshMaterial3d(assets.preach_mat.clone()),
                Transform::from_translation(city_top(c) - origin)
                    .with_scale(Vec3::splat(0.6 * CITY_SCALE)),
                NotShadowCaster,
                ChildOf(root),
            ));
        }
    }
    // Trade convoys shuttle along every open route from the capital.
    // civ-visuals2: the retail trade vehicles carry balloons (`civ_vfx`); the ADAPTED crate
    // convoys only stay with `OPENSPORE_CIV_VFX=0`.
    let routes: Vec<usize> = campaign
        .cities
        .iter()
        .filter(|c| c.trade && !c.owned() && !crate::civ_vfx::on())
        .map(|c| c.index)
        .collect();
    let mut have_cargo: HashMap<usize, usize> = HashMap::new();
    let home = campaign.vehicle_pad(HOME_INDEX);
    for (entity, crate_, mut t) in &mut cargo {
        if !routes.contains(&crate_.city) {
            commands.entity(entity).despawn();
            continue;
        }
        *have_cargo.entry(crate_.city).or_default() += 1;
        let target = campaign.vehicle_pad(crate_.city);
        let s = (now * 0.12 + crate_.phase).fract();
        // Out and back again, over the curve of the planet.
        let k = if s < 0.5 { s * 2.0 } else { 2.0 - s * 2.0 };
        let length = shape.distance(home, target);
        let along = shape.toward(home, target - home, length * k);
        let pos = globe.lift(along, (0.9 + 5.0 * 4.0 * k * (1.0 - k)) * CITY_SCALE);
        let ahead = shape.toward(home, target - home, length * (k + 0.01).min(1.0));
        let dir = (ahead - along) * if s < 0.5 { 1.0 } else { -1.0 };
        t.translation = pos - origin;
        t.rotation = shape.heading(pos, shape.yaw_of(pos, dir));
        t.scale = Vec3::splat(CITY_SCALE);
    }
    for city in routes {
        for k in have_cargo.get(&city).copied().unwrap_or(0)..3 {
            commands.spawn((
                StageOwned,
                TradeCargo {
                    city,
                    phase: k as f32 / 3.0,
                },
                Mesh3d(assets.cargo.clone()),
                MeshMaterial3d(assets.nation[HOME_INDEX].clone()),
                Transform::from_translation(home - origin),
                ChildOf(root),
            ));
        }
    }
    // The drag-select rectangle.
    for mut node in &mut drag {
        match state.drag {
            Some((a, b)) => {
                let (lo, hi) = (a.min(b), a.max(b));
                node.display = Display::Flex;
                node.left = Val::Px(lo.x);
                node.top = Val::Px(lo.y);
                node.width = Val::Px(hi.x - lo.x);
                node.height = Val::Px(hi.y - lo.y);
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
}

/// City banners follow their cities on screen: name, type, income, armour
/// and the faith / purchase progress of a rival.
#[allow(clippy::too_many_arguments)]
pub fn present_labels(
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    mut labels: Query<
        (&CityLabel, &mut Node, &mut Visibility, &mut BorderColor),
        Without<CityLabelBar>,
    >,
    mut texts: Query<(&CityLabelText, &mut Text)>,
    mut bars: Query<(&CityLabelBar, &mut Node, &mut BackgroundColor), Without<CityLabel>>,
) {
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    let _ = &surface;
    let campaign = &run.civ_campaign;
    let shape = campaign.shape();
    let eye = transform.translation();
    for (label, mut node, mut visibility, mut border) in &mut labels {
        let Some(city) = campaign.cities.get(label.0) else {
            continue;
        };
        let center = city.center();
        let anchor = center + shape.up(center) * 8.0 * CITY_SCALE;
        // Cities behind the planet's limb keep their banners hidden.
        let screen = (campaign.present(label.0) && shape.visible_from(eye, center))
            .then(|| camera.world_to_viewport(transform, anchor).ok())
            .flatten();
        let want = if screen.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
        if let Some(p) = screen {
            node.left = Val::Px(p.x - 62.0);
            node.top = Val::Px(p.y - 24.0);
        }
        *border = BorderColor::all(civ::nation_color(city.nation()));
    }
    for (text, mut value) in &mut texts {
        let Some(city) = campaign.cities.get(text.0) else {
            continue;
        };
        let line = if city.owned() {
            let state = if city.rioting() {
                " RIOT"
            } else if !city.producing() {
                " SIEGE"
            } else {
                ""
            };
            format!(
                "{} {} +{:.0}{state}",
                city.label(),
                city.kind.title(),
                city.income()
            )
        } else if campaign.at_war(city.index) {
            format!("{} {} - WAR", city.label(), city.kind.title())
        } else if city.trade {
            format!("{} {} - trading", city.label(), city.kind.title())
        } else {
            format!("{} {}", city.label(), city.kind.title())
        };
        if value.0 != line {
            value.0 = line;
        }
    }
    for (bar, mut node, mut color) in &mut bars {
        let Some(city) = campaign.cities.get(bar.0) else {
            continue;
        };
        let (frac, tint) = match bar.1 {
            1 => (
                city.hp / city.max_hp(),
                Color::srgb(0.35, 0.95, 0.4).mix(
                    &Color::srgb(1.0, 0.25, 0.2),
                    1.0 - (city.hp / city.max_hp()).clamp(0.0, 1.0),
                ),
            ),
            // A rival converting one of your cities fills the bar in red.
            _ if city.owned() => (city.rival_conversion, Color::srgb(0.95, 0.3, 0.25)),
            _ if city.trade => (city.spent / city.buy_price(), Color::srgb(0.35, 1.0, 0.6)),
            _ => (city.conversion, Color::srgb(1.0, 0.85, 0.3)),
        };
        node.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
        if color.0 != tint {
            color.0 = tint;
        }
    }
}

// ---------------------------------------------------------------------------
// Test and capture helpers
// ---------------------------------------------------------------------------

/// Move a nation's whole fleet next to a point (tests and captures).
pub fn teleport_fleet(world: &mut World, nation: usize, at: Vec3) {
    let shape = world.resource::<RunProgress>().civ_campaign.shape();
    let mut query = world.query::<(&CivUnit, &mut Transform)>();
    let mut k = 0;
    for (unit, mut t) in query.iter_mut(world) {
        if unit.nation != nation {
            continue;
        }
        let p = offset_on(shape, at, formation(k));
        t.translation = if shape.on_sphere() {
            p
        } else {
            Vec3::new(p.x, t.translation.y, p.z)
        };
        k += 1;
    }
}

/// Stage a scene for `--quick-capture civilization --scenario NAME`:
/// `battle` (your army assaulting C2 while it fights back), `faith`
/// (preachers at C3 and a trade route to C4), `conversion` (C3's preachers
/// at your capital, your fleet beside it, seen from 100), `globe` (the
/// widest view over the level origin) or `select` (your fleet selected at
/// home).
pub fn quick_capture_scenario(world: &mut World, name: &str) {
    let selection: Vec<Entity> = {
        let mut query = world.query::<(Entity, &CivUnit)>();
        query
            .iter(world)
            .filter(|(_, u)| u.nation == HOME_INDEX)
            .map(|(e, _)| e)
            .collect()
    };
    for e in &selection {
        world.entity_mut(*e).insert(Selected);
    }
    // The visuals audit's framed scenarios (`civ_view`).
    if crate::civ_view::capture_scenario(world, name) {
        return;
    }
    let (shape, capital, cities) = {
        let campaign = &world.resource::<RunProgress>().civ_campaign;
        (
            campaign.shape(),
            campaign.cities[HOME_INDEX].center(),
            campaign.cities.iter().map(|c| c.center()).collect::<Vec<_>>(),
        )
    };
    let focus_on = |world: &mut World, at: Vec3| {
        let mut query = world.query_filtered::<&mut Transform, With<CivCommander>>();
        if let Ok(mut t) = query.single_mut(world) {
            t.translation = if shape.on_sphere() {
                at
            } else {
                Vec3::new(at.x, t.translation.y, at.z)
            };
        }
    };
    let before_point = |shape: Shape, from: Vec3, toward: Vec3, d: f32| shape.toward(from, toward, d);
    // A point `d` from city `c` on the way to the capital.
    let before = |c: usize, d: f32| {
        let city = cities[c];
        let toward = if c == HOME_INDEX { shape.bearing(city, 0.0) } else { capital - city };
        shape.toward(city, toward, d)
    };
    match name {
        // swarm-civ: frame the spots a `--swarm-fx NAME` capture needs (the
        // effect plays at the camera focus): geyser 0 (`fx-claimed` gives it
        // the player's derrick first), the capital, rival city 1.
        "fx-geyser" | "fx-claimed" => {
            let at = {
                let mut run = world.resource_mut::<RunProgress>();
                if name == "fx-claimed" {
                    run.civ_campaign.geysers[0] = civ::GeyserState::derrick(HOME_INDEX, 100.0);
                }
                run.civ_campaign.geyser_at(0)
            };
            focus_on(world, at);
        }
        // civ-harvest: one of your economic vehicles 8 units from geyser 0 ordered to claim it
        // (`civ_pump`): it starts the construction at the first frame, the pump capsule leaves it
        // and flies to the node (use `--frames 3 .. 40` for the flight, the landing and the
        // unpacking pump; the derrick stands after 5 s).
        "pump" => {
            let (at, unit) = {
                let mut run = world.resource_mut::<RunProgress>();
                // The geyser nearest the capital (in view at the start), taken free first: the
                // opening may have given it to a nation.
                let g = (0..civ::GEYSER_COUNT)
                    .min_by(|a, b| {
                        shape
                            .distance(run.civ_campaign.geyser_at(*a), capital)
                            .total_cmp(&shape.distance(run.civ_campaign.geyser_at(*b), capital))
                    })
                    .unwrap_or(0);
                let node = run.civ_campaign.geyser_at(g);
                let from = shape.toward(node, shape.bearing(node, 0.8), 8.0);
                run.civ_campaign.geysers[g] = civ::GeyserState::default();
                let mut unit = run.civ_campaign.add_unit(HOME_INDEX, Approach::Economic, false, from);
                unit.order = UnitOrder::Geyser(g);
                (node, unit)
            };
            world.resource_mut::<UnitSpawns>().0.push(unit);
            focus_on(world, at);
        }
        "fx-city" => focus_on(world, capital),
        "fx-rival" => focus_on(world, cities[1]),
        // civ-orders: the pie menu of rival city 2 (or, `pie-bribe`, the bribe popup of a rival's
        // derrick) with one military, one religious and one economic vehicle selected (the three
        // selected opening vehicles take the three purposes for the picture).
        "pie" | "pie-bribe" => {
            let mut query = world.query::<&mut CivUnit>();
            for (k, mut unit) in query.iter_mut(world).filter(|u| u.nation == HOME_INDEX).enumerate() {
                unit.kind = [Approach::Military, Approach::Religious, Approach::Economic][k % 3];
            }
            let (target, at) = if name == "pie" {
                (crate::civ_orders::PieTarget::City(2), cities[2])
            } else {
                let mut run = world.resource_mut::<RunProgress>();
                run.spice = 10_000.0;
                run.civ_campaign.geysers[4] = civ::GeyserState::derrick(2, 80.0);
                (crate::civ_orders::PieTarget::Geyser(4), run.civ_campaign.geyser_at(4))
            };
            teleport_fleet(world, HOME_INDEX, before_point(shape, at, capital - at, 40.0));
            focus_on(world, at);
            world.resource_mut::<crate::civ_orders::PieRequests>().0 =
                Some(crate::civ_orders::PieRequest { target, at: Vec2::new(640.0, 430.0) });
        }
        "battle" => {
            let target = cities[1];
            let approach = before(1, 70.0);
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.spice += 400.0;
                run.civ_campaign.nations[1].war = true;
                let extra: Vec<UnitState> = (0..3)
                    .map(|k| {
                        run.civ_campaign.add_unit(
                            HOME_INDEX,
                            Approach::Military,
                            false,
                            offset_on(shape, approach, Vec2::new(k as f32 * 2.0, 2.0) * CITY_SCALE),
                        )
                    })
                    .collect();
                world.resource_mut::<UnitSpawns>().0.extend(extra);
            }
            teleport_fleet(world, HOME_INDEX, approach);
            teleport_fleet(world, 1, before(1, 35.0));
            let mut query = world.query::<&mut CivUnit>();
            for mut unit in query.iter_mut(world) {
                if unit.nation == HOME_INDEX {
                    unit.order = UnitOrder::City(1);
                }
            }
            let _ = target;
            focus_on(world, before(1, 45.0));
        }
        "faith" => {
            let preach = before(2, 70.0);
            let trade = before(3, 70.0);
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.spice += 400.0;
                run.civ_campaign.cities[3].trade = true;
                run.civ_campaign.cities[2].conversion = 0.35;
                let mut extra = Vec::new();
                for k in 0..3 {
                    extra.push(run.civ_campaign.add_unit(
                        HOME_INDEX,
                        Approach::Religious,
                        false,
                        offset_on(shape, preach, Vec2::new(k as f32 * 2.0, 0.0) * CITY_SCALE),
                    ));
                }
                extra.push(run.civ_campaign.add_unit(
                    HOME_INDEX,
                    Approach::Economic,
                    false,
                    trade,
                ));
                for unit in &mut extra {
                    unit.order = if unit.kind == Approach::Religious {
                        UnitOrder::City(2)
                    } else {
                        UnitOrder::City(3)
                    };
                }
                world.resource_mut::<UnitSpawns>().0.extend(extra);
            }
            focus_on(world, before(2, 50.0));
        }
        "conversion" => {
            // C3's preachers converting your capital (its meter part full),
            // your fleet beside the city at the retail vehicle size, seen
            // from close (100).
            let at = before(HOME_INDEX, 22.0);
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.civ_campaign.player_developed = true;
                run.civ_campaign.nations[2].war = true;
                run.civ_campaign.nations[2].raid = 999.0;
                run.civ_campaign.cities[HOME_INDEX].rival_conversion = 0.4;
                run.civ_campaign.cities[HOME_INDEX].converter = Some(2);
                let mut extra = Vec::new();
                for k in 0..3 {
                    let mut unit = run.civ_campaign.add_unit(2, Approach::Religious, false, offset_on(shape, at, Vec2::new(k as f32 * 2.5, 0.0) * CITY_SCALE));
                    unit.order = UnitOrder::City(HOME_INDEX);
                    extra.push(unit);
                }
                world.resource_mut::<UnitSpawns>().0.extend(extra);
            }
            teleport_fleet(world, HOME_INDEX, before(HOME_INDEX, 30.0));
            focus_on(world, before(HOME_INDEX, 12.0));
            let mut camera = world.resource_mut::<crate::civ_controls::CivCamera>();
            camera.goal_distance = crate::civ_controls::tuning::ZOOM_STEPS[2];
        }
        "siege" | "capture" => {
            // C2 under the retail capture rule: "siege" = mid-assault (a
            // turret and a building down, another damaged, 18 points),
            // "capture" = every building down and 97 points: the fleet's
            // fire on the hall takes it within seconds.
            let capture = name == "capture";
            let approach = before(1, 55.0);
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.civ_campaign.nations[1].war = true;
                let city = &mut run.civ_campaign.cities[1];
                if capture {
                    for p in 0..city.plots.len() {
                        city.damage_building(p, civ::BUILDING_HP, HOME_INDEX);
                    }
                    for t in 0..city.turret_hp.len() {
                        city.damage_turret(t, civ::TURRET_HP);
                    }
                    let left = civ_rules::CAPTURE_FULL - 3.0 - city.capture;
                    city.damage_hall(left / civ_rules::CAPTURE_PER_DAMAGE, HOME_INDEX);
                } else {
                    let plots: Vec<usize> = (0..city.plots.len()).filter(|p| city.standing(*p)).collect();
                    city.damage_building(plots[0], civ::BUILDING_HP, HOME_INDEX);
                    city.damage_building(plots[1], 500.0, HOME_INDEX);
                    city.damage_turret(0, civ::TURRET_HP);
                    city.damage_turret(1, 350.0);
                    let left = 18.0 - city.capture;
                    city.add_capture(left / civ_rules::CAPTURE_PER_DAMAGE, HOME_INDEX);
                }
                let extra: Vec<UnitState> = (0..4)
                    .map(|k| {
                        run.civ_campaign.add_unit(
                            HOME_INDEX,
                            Approach::Military,
                            false,
                            offset_on(shape, approach, Vec2::new(k as f32 * 2.0 - 3.0, 1.5) * CITY_SCALE),
                        )
                    })
                    .collect();
                world.resource_mut::<UnitSpawns>().0.extend(extra);
            }
            teleport_fleet(world, HOME_INDEX, approach);
            teleport_fleet(world, 1, before(1, 160.0));
            let mut query = world.query::<&mut CivUnit>();
            for mut unit in query.iter_mut(world) {
                if unit.nation == HOME_INDEX {
                    unit.order = UnitOrder::City(1);
                }
            }
            focus_on(world, before(1, 18.0));
            let mut camera = world.resource_mut::<crate::civ_controls::CivCamera>();
            camera.goal_distance = crate::civ_controls::tuning::ZOOM_STEPS[2];
        }
        "vehicles" | "decorations" | "outfits" | "music" => {
            // The City Planner's other pages (civ-final): the capital's
            // vehicle swatches; the decorations page with a lamp and a
            // fountain already placed and a bush armed over a free spot.
            world.resource_mut::<RunProgress>().spice += 5000.0;
            if name == "decorations" {
                let mut run = world.resource_mut::<RunProgress>();
                let city = &mut run.civ_campaign.cities[HOME_INDEX];
                crate::civ_decor::place(&mut city.decorations, 4, Some(0));
                crate::civ_decor::place(&mut city.decorations, 3, Some(2));
                crate::civ_decor::place(&mut city.decorations, 0, Some(4));
            }
            let mut planner = world.resource_mut::<crate::civ_planner::CivPlanner>();
            planner.request = Some(HOME_INDEX);
            planner.page = match name {
                "vehicles" => crate::civ_planner::PlannerPage::Vehicles,
                "decorations" => crate::civ_planner::PlannerPage::Decorations,
                "music" => crate::civ_planner::PlannerPage::Music,
                _ => crate::civ_planner::PlannerPage::Outfits,
            };
            if name == "decorations" {
                planner.armed = Some(crate::civ_planner::PlannerItem::Decoration(2));
                planner.hover_spot = Some(6);
            }
        }
        "rebuild" => {
            // A rival city after a raid (civ-final): its buildings fell and
            // the ruins were cleared, one turret smoulders; with its domestic
            // budget it rebuilds (0xBE5890: a city earning nothing builds
            // every second), each new building raising plop dust.
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.civ_campaign.hidden.retain(|h| *h != 1);
                run.civ_campaign.nations[1].domestic = 9000.0;
                let city = &mut run.civ_campaign.cities[1];
                for p in 0..city.plots.len() {
                    if p != 4 {
                        city.plots[p] = None;
                        city.building_damage[p] = 0.0;
                    }
                }
                city.damage_building(4, 520.0, HOME_INDEX);
                city.damage_turret(0, civ::TURRET_HP);
                city.calm = civ_rules::ATTACK_MEMORY_SECONDS + 1.0;
                city.age = 500.0;
            }
            // The tutorial callouts would hide the city.
            if let Some(mut guide) = world.get_resource_mut::<crate::civ_hud::guide::GuideState>() {
                for _ in 0..8 {
                    guide.dismiss();
                }
            }
            focus_on(world, before(1, 10.0));
            let mut camera = world.resource_mut::<crate::civ_controls::CivCamera>();
            camera.goal_distance = crate::civ_controls::tuning::ZOOM_STEPS[2];
        }
        "planner" | "anthem" => {
            // The City Planner on the capital with a factory armed over a
            // free slot (the preview and its links); "anthem" also opens the
            // anthem editor.
            world.resource_mut::<crate::civ_anthem::AnthemEditor>().open = name == "anthem";
            let free = {
                let campaign = &world.resource::<RunProgress>().civ_campaign;
                let city = &campaign.cities[HOME_INDEX];
                (0..city.plots.len()).find(|p| city.plots[*p].is_none())
            };
            world.resource_mut::<RunProgress>().spice += 5000.0;
            let mut planner = world.resource_mut::<crate::civ_planner::CivPlanner>();
            planner.request = Some(HOME_INDEX);
            planner.armed = Some(crate::civ_planner::PlannerItem::Building(civ::Building::Factory));
            planner.hover = free;
        }
        "tool" | "tool-invalid" => crate::civ_target::capture_scenario(world, name),
        "globe" => {
            // The level origin under the widest retail view (450, 65 deg):
            // the cities around it on the curved planet.
            focus_on(world, Vec3::ZERO);
            let mut camera = world.resource_mut::<crate::civ_controls::CivCamera>();
            camera.goal_distance = crate::civ_controls::tuning::ZOOM_STEPS[8];
            camera.goal_pitch = crate::civ_controls::tuning::MAX_PITCH_DEG.to_radians();
        }
        _ => {
            let pad = world.resource::<RunProgress>().civ_campaign.vehicle_pad(HOME_INDEX);
            focus_on(world, pad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_handler_sends_non_economic_vehicles_against_vehicles() {
        // 0xCF4322: military and religious (and harvester) vehicles attack a vehicle, economic never.
        assert!(can_attack_vehicle(Approach::Military));
        assert!(can_attack_vehicle(Approach::Religious));
        assert!(!can_attack_vehicle(Approach::Economic));
    }

    #[test]
    fn boats_attack_only_coastal_cities_unless_the_order_holds_a_land_vehicle() {
        use civ::VehicleDomain::{Land, Sea};
        // 0xE35C42: a rival's inland city is refused to military boats ...
        assert!(boat_refused(Sea, Approach::Military, false, false, false));
        // ... but not a coastal one, an own one, a mixed order, or other vehicles.
        assert!(!boat_refused(Sea, Approach::Military, false, true, false));
        assert!(!boat_refused(Sea, Approach::Military, true, false, false));
        assert!(!boat_refused(Sea, Approach::Military, false, false, true));
        assert!(!boat_refused(Land, Approach::Military, false, false, false));
        assert!(!boat_refused(Sea, Approach::Religious, false, false, false));
    }

    #[test]
    fn vehicles_are_drawn_at_their_creation_size() {
        // Own size in world units, capped by the editor bound 6.
        assert!((retail_extent(4.5).unwrap() * CITY_SCALE - 4.5).abs() < 1e-4);
        assert!((retail_extent(14.0).unwrap() * CITY_SCALE - VEHICLE_BOUND_SIZE).abs() < 1e-4);
        assert_eq!(retail_extent(0.0), None);
        assert!((VEHICLE_DRAW_RATIO - 6.0 / 9.0).abs() < 1e-4);
        assert!((VEHICLE_MOUNT_SCALE * CITY_SCALE - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_trade_shuttle_survives_a_save() {
        let mut unit = CivUnit::from_state(&UnitState { id: 5, nation: HOME_INDEX, kind: Approach::Economic, ..Default::default() });
        unit.homeward = true;
        unit.cargo = 123.0;
        unit.trade_wait = 1.5;
        unit.trade_home = Some(0);
        let saved = unit.state(Vec3::ZERO);
        let json = serde_json::to_string(&saved).unwrap();
        let back = CivUnit::from_state(&serde_json::from_str::<UnitState>(&json).unwrap());
        assert!(back.homeward && back.cargo == 123.0 && back.trade_wait == 1.5 && back.trade_home == Some(0));
        // A military vehicle saves no shuttle; old saves load without one.
        let mil = CivUnit::from_state(&UnitState { id: 6, kind: Approach::Military, ..Default::default() });
        assert!(mil.state(Vec3::ZERO).shuttle.is_none());
    }

    #[test]
    fn rays_land_on_the_ground_plane_and_variable_terrain() {
        let hit = ground_hit(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y, None).expect("hits");
        assert!((hit.y - BASE_Y).abs() < 0.01, "{hit:?}");
        let slanted = ground_hit(
            Vec3::new(0.0, 10.6, 10.6),
            Vec3::new(0.0, -1.0, -1.0).normalize(),
            None,
        )
        .expect("slanted hit");
        assert!(
            slanted.distance(Vec3::new(0.0, BASE_Y, BASE_Y)) < 0.01,
            "{slanted:?}"
        );
        assert!(ground_hit(Vec3::ZERO, Vec3::Y, None).is_none());

        let pads: Vec<_> = (0..civ::CITY_COUNT)
            .map(|i| {
                let p = civ::city_position(i);
                (Vec2::new(p.x, p.z), 5.0)
            })
            .collect();
        let surface = crate::terrain::LandSurface::generate(
            crate::stages::GameStage::Civilization,
            0xC17_5EED,
            &pads,
        );
        let origin = Vec3::new(-7.0, 30.0, 11.0);
        let direction = Vec3::new(0.16, -1.0, -0.09).normalize();
        let terrain_hit = ground_hit(origin, direction, Some(&surface)).expect("terrain hit");
        let ray_t = (terrain_hit - origin).dot(direction);
        assert!(ray_t > 0.0, "hit must be in front of the ray origin");
        assert!(
            (terrain_hit.y - surface.height(terrain_hit.x, terrain_hit.z)).abs() < 0.01,
            "hit must meet the sampled terrain: {terrain_hit:?}"
        );
        assert!(
            (origin + direction * ray_t).distance(terrain_hit) < 0.01,
            "hit must lie on the input ray: {terrain_hit:?}"
        );
    }

    #[test]
    fn turrets_spare_peaceful_traders_but_not_aggressors() {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        let mut spice = 0.0;
        campaign.begin(&mut spice);
        let mut unit = CivUnit::from_state(&campaign.units[0]);
        unit.kind = Approach::Economic;
        unit.order = UnitOrder::City(2);
        assert!(!turret_hostile(&campaign, 2, &unit), "traders are welcome");
        unit.kind = Approach::Religious;
        assert!(turret_hostile(&campaign, 2, &unit), "preachers are shot at");
        unit.order = UnitOrder::Idle;
        assert!(!turret_hostile(&campaign, 2, &unit), "passing by is fine");
        campaign.nations[2].war = true;
        assert!(
            turret_hostile(&campaign, 2, &unit),
            "at war, everything is a target"
        );
        assert!(hostile(&campaign, HOME_INDEX, 2));
        assert!(!hostile(&campaign, 1, 2), "rivals leave each other alone");
        // Your own turrets fire only on nations at war with you.
        let at = campaign.vehicle_pad(1);
        let rival = campaign.add_unit(1, Approach::Military, false, at);
        let mut raider = CivUnit::from_state(&rival);
        raider.order = UnitOrder::City(HOME_INDEX);
        assert!(!turret_hostile(&campaign, HOME_INDEX, &raider));
        campaign.nations[raider.nation].war = true;
        assert!(turret_hostile(&campaign, HOME_INDEX, &raider));
    }

    #[test]
    fn production_respects_ownership_the_cap_and_the_purse() {
        let mut campaign = CivCampaign::default();
        let mut spice = 0.0;
        campaign.begin(&mut spice);
        let live = campaign.fleet(HOME_INDEX);
        let unit =
            try_produce(&mut campaign, &mut spice, live, HOME_INDEX, false).expect("affordable");
        assert_eq!(unit.nation, HOME_INDEX);
        assert_eq!(unit.kind, campaign.cities[HOME_INDEX].kind);
        assert_eq!(spice, civ::STARTING_SPICE - civ::LAND_VEHICLE_COST);
        assert!(
            try_produce(&mut campaign, &mut spice, live, 2, false).is_err(),
            "not your city"
        );
        let cap = campaign.vehicle_cap(HOME_INDEX);
        assert!(
            try_produce(&mut campaign, &mut spice, cap, HOME_INDEX, false)
                .unwrap_err()
                .contains("support")
        );
        assert!(try_produce(&mut campaign, &mut spice, 0, HOME_INDEX, true)
            .unwrap_err()
            .contains("cities"));
        let mut broke = 1.0;
        assert!(try_produce(&mut campaign, &mut broke, 0, HOME_INDEX, false).is_err());
        assert_eq!(broke, 1.0);
    }
}
