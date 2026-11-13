//! Civilization: the game's own Swarm effects on cities and vehicles
//! (`notes\civ-final-re.md` E1).
//!
//! VERIFIED (the effect ids appear in the code, ModAPI march2017 addresses):
//! * `city_building_plop_dust` (0x98FF936A): the City Planner's placement
//!   (0xD09B10 stores it at +0x288) and the city effect table 0xB3AD20: a
//!   building raised (by you or a rival city, `civ::npc_build_second`);
//! * `civ_building_partially_damaged` (0xE8A958) and `civ_building_damaged`
//!   (0xE8A699): the damage-state loops on a hurt building;
//! * `civ_basic_explosion`, `civ_turret_destroyed`, `civ_repair_building`,
//!   `civ_conquer_newRegime` (the table 0xB3AD20): a building or turret
//!   falling, the repairs (R3: every other second while healing), a city
//!   changing hands;
//! * `civ_turret_destroyed_continuous` (0xE8BA70, played with the sound
//!   0x04C9BD57 by 0xC8B3B0): a fallen turret smoulders until it is removed;
//! * `civ_create_vehicle` (0xE9643F) and `civ_explosion_vehicle` (cVehicle
//!   0xCAA592): a vehicle rolling out and one destroyed;
//! * the vehicle's own effects (its GFX update 0xE956D0): the trail
//!   `civ_vehicle_ground_moving_*` / `civ_vehicle_boat_wake_*` /
//!   `civ_vehicle_air_contrails_*` (names built from strings in the code and
//!   hashed at run time, which is why no constant held them) while it moves
//!   faster than 0.1, picked once by its locomotion (+0xB1C) and its design's
//!   speed stat (`mSpeedStat` +0xC58: slow below 0.33, medium below 0.66, else
//!   fast) and attached to it; `civ_vehicle_damage_minor` below 2/3 of its
//!   health and `civ_vehicle_damage_major` below 1/3 (0xBFC5D0 = health /
//!   max); `SG_ufo_static_cling_stunned_vehicle` while it is stunned
//!   (0xBFC740).
//!
//! * `city_zoning_ring` (0x5F688FA9, the display strategy of `cCityTerritory`
//!   0xE87D90 at 0xE87F5D; Space plays other effects there): a STATIC ribbon
//!   whose polyline is the effect parameter 0xD, which that function builds
//!   every frame: a ring of N segments (the ring detail setting, clamped to
//!   3..=500, else 500) around the city centre at the territory radius, each
//!   point projected onto the surface (`cPlanetModel::ToSurface`); the radius
//!   eases toward its target by a tenth per frame. Played without the
//!   parameter the ribbon is the one-pixel white bar of `--swarm-fx
//!   city_zoning_ring` (see `swarm_fx::sim::update_ribbon_points`). OPT-IN
//!   (`OPENSPORE_CIV_ZONING_RING=1`): the territory radius (the
//!   `cSpatialObject` virtual at +0x70, `cCityTerritory` field 0x10C = 100.0 at
//!   construction), its colour, when it is shown and what the three states
//!   (+0x34 = 1..3) mean in Civ were not traced, so the radius is ADAPTED.
//!
//! * `civ_selection_vehicle` (0xF6F34487): the selection circle of a vehicle
//!   (two ring particles with the sound `ui_citymember_select`, effect flags
//!   0x82050: NO_AUTO_STOP + the extended LOD distances). VERIFIED data;
//!   retail picks its LOD from the camera (`swarm_fx::sim::lod_value`: facing
//!   weight 1, so the ring fades out when the camera is level with it) and the
//!   ramp of its alpha scale table is what made it invisible at the old
//!   always-LOD-1 player (`notes\swarm-civ-notes.md`). Played here for every
//!   selected vehicle, attached to it, stopped when it is deselected.
//!   ADAPTED: who plays it and at which scale was not traced (the ids are
//!   built at run time); the effect is scaled so its ring (1.5 units across at
//!   scale 1) matches the diameter of OpenSpore's former torus ring, which
//!   stays as the fallback (`OPENSPORE_CIV_SELECTION_FX=0`).
//!
//! * `civ_selection_destination` (0x5BDA2617, one second, flags 0x40): the four arrows
//!   around the ordered point (the `UI_goHere_arrow` raster, size 7, 3.1 from the
//!   centre, plus one above). VERIFIED data; ADAPTED: played at world scale 1 on the
//!   ground point of every right-click move / attack order (who plays it was not
//!   traced; OpenSpore drew a shrinking torus marker there,
//!   `OPENSPORE_CIV_VFX=0` brings it back).
//! * Rival vehicles: clicking one gives it the same selection circle for
//!   [`INSPECT_SECONDS`] (ADAPTED), tinted with the owner's colour
//!   ([`owner_tint`]; effect parameter 5 / `SwarmEffect::tint`). The retail circle
//!   data is one yellow ring (1.0 / 0.95 / 0.6); the owner colour is ADAPTED.
//!
//! VERIFIED (civ-audit-b, the building display strategy 0xE8A560 .. 0xE8A9C0): the
//! building damage states are `civ_building_partially_damaged` (id 0x65C15577, slot
//! 0x49B925C) while the building's health fraction (0xBFC5D0) is from 0.5 up to 0.8,
//! `civ_building_damaged` (0x97C0B9BF, slot 0x49B925D) below 0.5, none from 0.8 up, and
//! NONE on a ruin (combatant state 2 stops both slots). Effects play at the plot /
//! tower / vehicle, upright on the planet. A stopped vehicle's trail fades over 1 s
//! (`trail_step`).

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::civ::{CityOwner, CityState, VehicleDomain};
use crate::civ_planet::{Globe, Shape};
use crate::civ_rts::CivUnit;
use crate::game::RunProgress;
use crate::stages::{GameStage, StageOwned};
use crate::swarm_fx::{SwarmEffect, SwarmPlay, SwarmStop};
use crate::world_objects::name_id;

pub const PLOP_DUST: &str = "city_building_plop_dust";
pub const BUILDING_PARTIALLY_DAMAGED: &str = "civ_building_partially_damaged";
pub const BUILDING_DAMAGED: &str = "civ_building_damaged";
pub const BASIC_EXPLOSION: &str = "civ_basic_explosion";
pub const TURRET_DESTROYED: &str = "civ_turret_destroyed";
pub const TURRET_DESTROYED_LOOP: &str = "civ_turret_destroyed_continuous";
pub const REPAIR_BUILDING: &str = "civ_repair_building";
pub const CONQUER: &str = "civ_conquer_newRegime";
pub const CREATE_VEHICLE: &str = "civ_create_vehicle";
pub const VEHICLE_EXPLOSION: &str = "civ_explosion_vehicle";
pub const VEHICLE_DAMAGE_MINOR: &str = "civ_vehicle_damage_minor";
pub const VEHICLE_DAMAGE_MAJOR: &str = "civ_vehicle_damage_major";
pub const VEHICLE_STUNNED: &str = "SG_ufo_static_cling_stunned_vehicle";
/// Vehicle event 0x18 (jump table 0xE97218): a cultural weapon hurts the vehicle; event 0x19 stops it.
pub const VEHICLE_CULTURE: &str = "civ_weapons_culture_vehicle_damage";
/// ADAPTED: seconds the effect keeps playing after a cultural hit. Retail stops it with event 0x19
/// pushed by the weapon reset (0xCAA546), whose timing was not traced; a little over the
/// 2 s weapon cycle keeps it on while the attack goes on.
pub const CULTURE_HOLD: f32 = 2.5;
pub const SELECTION_VEHICLE: &str = "civ_selection_vehicle";
pub const SELECTION_DESTINATION: &str = "civ_selection_destination";
/// ADAPTED: seconds a clicked rival vehicle keeps its (owner coloured) circle.
pub const INSPECT_SECONDS: f32 = 6.0;
/// ADAPTED: the ring particle of `civ_selection_vehicle` is 1.5 units across.
const SELECTION_RING_UNITS: f32 = 1.5;
/// 0xE956D0: a vehicle trails while it moves faster than this.
pub const TRAIL_MIN_SPEED: f32 = 0.1;
/// 0xE956D0: a stopped vehicle's trail fades over 1000 ms.
pub const TRAIL_FADE_SECONDS: f32 = 1.0;

/// Trails by domain and pace (slow, medium, fast).
pub const TRAILS: [(VehicleDomain, [&str; 3]); 3] = [
    (
        VehicleDomain::Land,
        ["civ_vehicle_ground_moving_slow", "civ_vehicle_ground_moving_medium", "civ_vehicle_ground_moving_fast"],
    ),
    (VehicleDomain::Sea, ["civ_vehicle_boat_wake_slow", "civ_vehicle_boat_wake_medium", "civ_vehicle_boat_wake_fast"]),
    (
        VehicleDomain::Air,
        ["civ_vehicle_air_contrails_slow", "civ_vehicle_air_contrails_medium", "civ_vehicle_air_contrails_fast"],
    ),
];

/// Tags of the looping effects (`SwarmStop` by tag).
const TAG_BUILDING: u64 = 0xC1F0_0000_0000_0000;
const TAG_TURRET: u64 = 0xC1F1_0000_0000_0000;
const TAG_TRAIL: u64 = 0xC1F2_0000_0000_0000;
const TAG_VEHICLE_DAMAGE: u64 = 0xC1F3_0000_0000_0000;
const TAG_VEHICLE_STUN: u64 = 0xC1F4_0000_0000_0000;
/// `civ_weapons_culture_vehicle_damage` on a vehicle (0xC1FA is `civ_pump`).
const TAG_VEHICLE_CULTURE: u64 = 0xC1FB_0000_0000_0000;
/// `city_zoning_ring` effects (the low byte: the city index); the geyser
/// effects of `spice_presentation` use 0xC1F5.
const TAG_ZONING: u64 = 0xC1F6_0000_0000_0000;
/// `civ_selection_vehicle` circles.
const TAG_SELECTION: u64 = 0xC1F7_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
pub const ZONING_RING: &str = "city_zoning_ring";
/// ADAPTED: the territory radius (`cCityTerritory` field 0x10C, 100.0).
pub const ZONING_RADIUS: f32 = 100.0;
/// ADAPTED: the ring floats this far above the surface (the retail ribbon is
/// on it; the margin keeps it out of the terrain's depth).
const ZONING_LIFT: f32 = 0.25;
/// ADAPTED: the ring detail (retail reads a setting, 0x1029880).
const ZONING_SEGMENTS: i32 = 160;

/// What a plot shows, for the effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotLook {
    Empty,
    /// Standing, with its damage state (0 sound, 1 partially damaged, 2
    /// damaged).
    Standing(u8),
    Ruined,
}

/// VERIFIED (0xE8A89C / 0xE8A8A5): a building at or above this health fraction shows no damage
/// effect, at or above `BUILDING_DAMAGED_BELOW` it shows the partial one.
pub const BUILDING_PARTIAL_BELOW: f32 = 0.8;
/// VERIFIED (0xE8A8A5): below this health fraction a building shows the heavy damage effect.
pub const BUILDING_DAMAGED_BELOW: f32 = 0.5;

/// Pure: the plot's look (damage states at 0.8 and 0.5 of the health, VERIFIED 0xE8A89C).
pub fn plot_look(city: &CityState, plot: usize) -> PlotLook {
    if city.standing(plot) {
        let health = city.building_health(plot) / crate::civ::BUILDING_HP;
        PlotLook::Standing(if health >= BUILDING_PARTIAL_BELOW {
            0
        } else if health >= BUILDING_DAMAGED_BELOW {
            1
        } else {
            2
        })
    } else if city.ruined(plot) {
        PlotLook::Ruined
    } else {
        PlotLook::Empty
    }
}

/// The loop a plot look carries (`None`: none). VERIFIED (0xE8A8CB): a ruin (state 2) carries
/// none, both damage slots are stopped.
pub fn damage_loop(look: PlotLook) -> Option<&'static str> {
    match look {
        PlotLook::Standing(1) => Some(BUILDING_PARTIALLY_DAMAGED),
        PlotLook::Standing(2) => Some(BUILDING_DAMAGED),
        _ => None,
    }
}

/// What a change of a plot starts: one-shots, and whether its loop changes.
pub fn plot_change(before: PlotLook, now: PlotLook) -> Vec<&'static str> {
    let mut out = Vec::new();
    match (before, now) {
        (PlotLook::Empty | PlotLook::Ruined, PlotLook::Standing(_)) => out.push(PLOP_DUST),
        (PlotLook::Standing(_), PlotLook::Ruined) => out.push(BASIC_EXPLOSION),
        _ => {}
    }
    out
}

/// VERIFIED 0xE956D0: the trail of a vehicle moving at `speed` (world units
/// / s) with the design speed stat `speed_stat` (0..1): none at 0.1 or less,
/// else by locomotion and the stat (below 0.33 slow, below 0.66 medium, else
/// fast).
pub fn trail(domain: VehicleDomain, speed: f32, speed_stat: f32) -> Option<&'static str> {
    if speed <= TRAIL_MIN_SPEED {
        return None;
    }
    let names = TRAILS.iter().find(|(d, _)| *d == domain)?.1;
    Some(if speed_stat < 0.33 {
        names[0]
    } else if speed_stat < 0.66 {
        names[1]
    } else {
        names[2]
    })
}

/// VERIFIED 0xE956D0: the damage effect of a vehicle at `health` (health /
/// max): none from 0.66, minor from 0.33, else major.
pub fn vehicle_damage(health: f32) -> Option<&'static str> {
    if health >= 0.66 {
        None
    } else if health >= 0.33 {
        Some(VEHICLE_DAMAGE_MINOR)
    } else if health >= 0.0 {
        Some(VEHICLE_DAMAGE_MAJOR)
    } else {
        None
    }
}

/// What a live vehicle shows (effects attached to it).
#[derive(Clone, Copy, Default)]
struct UnitFx {
    at: Vec3,
    trail: Option<&'static str>,
    /// Seconds the trail has been fading since the vehicle stopped.
    fade: Option<f32>,
    damage: Option<&'static str>,
    stunned: bool,
    /// Hit by a cultural weapon a moment ago (vehicle event 0x18 until 0x19).
    cultural: bool,
}

/// VERIFIED 0xE956D0: one frame of a vehicle's trail. A trail starts (tier
/// fixed) when the vehicle moves and none runs; while it moves the trail's
/// intensity (effect parameter 4) is 1; stopped, the intensity falls from 1
/// to 0 over 1 s (`(1000 - t) / 1000` of the stop timer), then the trail
/// stops. Returns the new (trail, fade) and the intensity to set (`None`:
/// leave it).
pub fn trail_step(
    shown: Option<&'static str>,
    fade: Option<f32>,
    moving: Option<&'static str>,
    dt: f32,
) -> (Option<&'static str>, Option<f32>, Option<f32>) {
    match (shown, moving) {
        (None, now) => (now, None, None),
        (Some(name), Some(_)) => (Some(name), None, fade.map(|_| 1.0)),
        (Some(name), None) => {
            let t = fade.unwrap_or(0.0) + dt;
            if t > TRAIL_FADE_SECONDS {
                (None, None, None)
            } else {
                (Some(name), Some(t), Some(1.0 - t / TRAIL_FADE_SECONDS))
            }
        }
    }
}

#[derive(Default)]
struct CitySnap {
    owner: Option<CityOwner>,
    plots: Vec<PlotLook>,
    damage: Vec<f32>,
    turrets: Vec<bool>,
    repair_wait: f32,
}

#[derive(Resource, Default)]
pub struct FxMemory {
    cities: HashMap<usize, CitySnap>,
    /// Live vehicles: last position and the effects attached.
    units: HashMap<Entity, UnitFx>,
    ready: bool,
}

pub struct CivFxPlugin;

impl Plugin for CivFxPlugin {
    fn build(&self, app: &mut App) {
        // civ-visuals2: the retail trade / node / building-circle effects.
        app.add_plugins(crate::civ_vfx::CivVfxPlugin);
        app.init_resource::<FxMemory>().add_systems(
            Update,
            city_effects.run_if(in_state(GameStage::Civilization)).run_if(resource_exists::<Messages<SwarmPlay>>),
        )
        .add_systems(
            Update,
            zoning_rings.run_if(in_state(GameStage::Civilization)).run_if(zoning_enabled),
        )
        .add_systems(
            Update,
            selection_effects
                .run_if(in_state(GameStage::Civilization))
                .run_if(selection_effect_on),
        )
        .add_systems(OnExit(GameStage::Civilization), forget);
    }
}

/// `OPENSPORE_CIV_SELECTION_FX=0` brings the torus selection rings back.
pub fn selection_effect_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_CIV_SELECTION_FX").map_or(true, |v| v != "0"))
}

/// The selection effect of one vehicle (the owner is the vehicle root).
#[derive(Component)]
struct SelectionFx(Entity);

/// A rival vehicle the player clicked: it shows the selection circle until `until`
/// (`Time::elapsed_secs`). ADAPTED (the player only selects own vehicles).
#[derive(Component, Clone, Copy, Debug)]
pub struct Inspected {
    pub until: f32,
}

impl Inspected {
    pub fn new(now: f32) -> Self {
        Self { until: now + INSPECT_SECONDS }
    }
}

/// Pure: the colour multiplier of a selection circle by owner. Your own vehicles keep the
/// authored yellow (no tint); a rival's ring takes its nation colour at full brightness
/// (the largest channel normalised to 1). ADAPTED.
pub fn owner_tint(nation: usize) -> Vec3 {
    if nation == crate::civ::HOME_INDEX {
        return Vec3::ONE;
    }
    let c = crate::civ::nation_color(nation).to_linear();
    let v = Vec3::new(c.red, c.green, c.blue);
    v / v.max_element().max(1e-3)
}

/// Play `civ_selection_vehicle` on every selected vehicle (and on the rival vehicle the player
/// clicked last) and stop it when the vehicle is deselected or gone.
fn selection_effects(
    mut commands: Commands,
    time: Res<Time>,
    units: Query<(Entity, &CivUnit, &crate::civ_rts::UnitRig, Has<crate::civ_rts::Selected>, Option<&Inspected>)>,
    transforms: Query<&Transform>,
    mut fx: Query<(&SelectionFx, &mut SwarmEffect)>,
) {
    let now = time.elapsed_secs();
    // Only the newest clicked rival keeps its circle.
    // (`OPENSPORE_CIV_VFX=0`: the old behaviour, rival vehicles show no circle.)
    let newest = units
        .iter()
        .filter(|_| crate::civ_vfx::on())
        .filter_map(|(e, _, _, _, i)| i.filter(|i| i.until > now).map(|i| (e, i.until)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e);
    let wanted = |e: Entity, selected: bool| selected || newest == Some(e);
    let mut live: HashSet<Entity> = HashSet::new();
    for (marker, mut effect) in &mut fx {
        let on = units.get(marker.0).is_ok_and(|(e, _, _, selected, _)| wanted(e, selected));
        if !on {
            effect.stop = true;
            continue;
        }
        live.insert(marker.0);
        // Follow the former torus ring: it hugs the drawn vehicle and rides
        // low under aircraft.
        if let Ok((_, _, rig, _, _)) = units.get(marker.0) {
            if let Ok(ring) = transforms.get(rig.ring) {
                effect.position = ring.translation;
                effect.scale = ring.scale.x * 2.0 / SELECTION_RING_UNITS;
            }
        }
    }
    for (entity, unit, rig, selected, _) in &units {
        if !wanted(entity, selected) || live.contains(&entity) {
            continue;
        }
        let mut effect = SwarmEffect::new(name_id(SELECTION_VEHICLE));
        effect.attach_to = Some(entity);
        effect.tag = TAG_SELECTION;
        effect.tint = owner_tint(unit.nation);
        if let Ok(ring) = transforms.get(rig.ring) {
            effect.position = ring.translation;
            effect.scale = ring.scale.x * 2.0 / SELECTION_RING_UNITS;
        }
        commands.spawn((effect, SelectionFx(entity), StageOwned));
    }
}

/// The retail destination arrows at a ground point (`civ_selection_destination`, one second),
/// upright on the planet. The caller checks [`destination_effect_on`].
pub fn destination_effect(commands: &mut Commands, at: Vec3, shape: Shape) {
    let mut effect = SwarmEffect::new(name_id(SELECTION_DESTINATION));
    effect.position = at;
    effect.rotation = upright(shape, at);
    commands.spawn((effect, StageOwned));
}

/// The destination arrows replace the torus marker unless `OPENSPORE_CIV_VFX=0`.
pub fn destination_effect_on() -> bool {
    crate::civ_vfx::on()
}

fn forget(mut memory: ResMut<FxMemory>) {
    *memory = FxMemory::default();
}

/// `OPENSPORE_CIV_ZONING_RING=1` turns the territory rings on (see the module
/// notes: the radius is ADAPTED).
fn zoning_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_CIV_ZONING_RING").is_ok_and(|v| v != "0"))
}

/// VERIFIED 0xE87D90: the ring's segment count is the detail setting when it
/// lies in 3..=500, else 500.
pub fn ring_segments(requested: i32) -> usize {
    if (3..=500).contains(&requested) {
        requested as usize
    } else {
        500
    }
}

/// VERIFIED 0xE87D90: the ring radius moves a tenth of the way to its target
/// every frame.
pub fn ring_radius_step(current: f32, target: f32) -> f32 {
    current + (target - current) * 0.1
}

/// VERIFIED 0xE87D90 (shape; the points are the effect's path parameter 0xD):
/// `segments + 1` points (the last repeats the first) on the circle of
/// `radius` around `center`, each lifted onto the surface under it.
pub fn ring_points(globe: &Globe, center: Vec3, radius: f32, segments: usize) -> Vec<Vec3> {
    let shape = globe.shape();
    (0..=segments)
        .map(|k| {
            let angle = k as f32 / segments as f32 * std::f32::consts::TAU;
            let on_circle = shape.toward(center, shape.bearing(center, angle), radius);
            globe.lift(on_circle, ZONING_LIFT)
        })
        .collect()
}

/// One `city_zoning_ring` effect per city that is present, its path rebuilt
/// every frame while the radius eases out to [`ZONING_RADIUS`].
fn zoning_rings(
    mut commands: Commands,
    run: Res<RunProgress>,
    land: Option<Res<crate::terrain::LandSurface>>,
    mut radii: Local<HashMap<usize, f32>>,
    mut rings: Query<&mut SwarmEffect>,
) {
    let campaign = &run.civ_campaign;
    let globe = Globe::from_surface(land.as_deref().filter(|s| s.stage == GameStage::Civilization));
    let segments = ring_segments(ZONING_SEGMENTS);
    let mut have: HashSet<usize> = HashSet::new();
    for mut effect in &mut rings {
        if effect.tag & TAG_MASK != TAG_ZONING {
            continue;
        }
        let index = (effect.tag & 0xFF) as usize;
        have.insert(index);
        let Some(city) = campaign.cities.get(index) else { continue };
        let radius = radii.entry(index).or_insert(0.0);
        *radius = ring_radius_step(*radius, ZONING_RADIUS);
        effect.path = ring_points(&globe, city.center(), *radius, segments);
    }
    for city in &campaign.cities {
        if !campaign.present(city.index) || have.contains(&city.index) {
            continue;
        }
        radii.insert(city.index, 0.0);
        let mut effect = SwarmEffect::new(name_id(ZONING_RING));
        effect.tag = TAG_ZONING | city.index as u64;
        effect.position = city.center();
        effect.rotation = upright(campaign.shape(), city.center());
        effect.path = ring_points(&globe, city.center(), 0.0, segments);
        commands.spawn((effect, StageOwned));
    }
}

fn upright(shape: Shape, at: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, shape.up(at))
}

fn play(plays: &mut MessageWriter<SwarmPlay>, name: &str, at: Vec3, rotation: Quat, tag: u64, attach: Option<Entity>) {
    let mut p = SwarmPlay::at(name_id(name), at);
    p.rotation = rotation;
    p.tag = tag;
    p.attach_to = attach;
    plays.write(p);
}

/// Compare the cities and vehicles with the last frame and start / stop the
/// effects of what changed (the first frame only remembers, except loops).
#[allow(clippy::too_many_arguments)]
fn city_effects(
    time: Res<Time>,
    run: Res<RunProgress>,
    units: Query<(Entity, &CivUnit, &GlobalTransform)>,
    mut plays: MessageWriter<SwarmPlay>,
    mut stops: MessageWriter<SwarmStop>,
    mut memory: ResMut<FxMemory>,
    mut effects: Query<&mut crate::swarm_fx::SwarmEffect>,
) {
    let dt = time.delta_secs();
    let campaign = &run.civ_campaign;
    if campaign.cities.is_empty() {
        return;
    }
    let shape = campaign.shape();
    let first = !memory.ready;
    memory.ready = true;
    let memory = &mut *memory;
    for city in &campaign.cities {
        if !campaign.present(city.index) {
            continue;
        }
        let snap = memory.cities.entry(city.index).or_default();
        let center = city.center();
        let rotation = upright(shape, center);
        if snap.owner.is_some_and(|o| o != city.owner) && city.owner == CityOwner::Player {
            play(&mut plays, CONQUER, center, rotation, 0, None);
        }
        snap.owner = Some(city.owner);
        snap.repair_wait = (snap.repair_wait - dt).max(0.0);
        let mut repaired = false;
        for plot in 0..city.plots.len() {
            let look = plot_look(city, plot);
            let before = snap.plots.get(plot).copied().unwrap_or(if first { look } else { PlotLook::Empty });
            let at = crate::civ_rts::plot_world(city, plot, shape);
            let tag = TAG_BUILDING | ((city.index as u64) << 8) | plot as u64;
            if before != look || (first && damage_loop(look).is_some()) {
                if !first {
                    for name in plot_change(before, look) {
                        play(&mut plays, name, at, rotation, 0, None);
                    }
                }
                if damage_loop(before) != damage_loop(look) || first {
                    stops.write(SwarmStop { effect: 0, attach_to: None, tag, hard: false });
                    if let Some(name) = damage_loop(look) {
                        play(&mut plays, name, at, rotation, tag, None);
                    }
                }
            }
            // Repairs: a standing building healing (0xBE3C20, every other
            // second).
            let damage = city.building_damage.get(plot).copied().unwrap_or(0.0);
            let was = snap.damage.get(plot).copied().unwrap_or(damage);
            if matches!(look, PlotLook::Standing(_)) && damage < was - 1e-3 && snap.repair_wait <= 0.0 {
                play(&mut plays, crate::civ_fx::REPAIR_BUILDING, at, rotation, 0, None);
                repaired = true;
            }
            if snap.plots.len() <= plot {
                snap.plots.resize(plot + 1, PlotLook::Empty);
                snap.damage.resize(plot + 1, 0.0);
            }
            snap.plots[plot] = look;
            snap.damage[plot] = damage;
        }
        snap.plots.truncate(city.plots.len());
        snap.damage.truncate(city.plots.len());
        if repaired {
            snap.repair_wait = 2.0;
        }
        for (i, hp) in city.turret_hp.iter().enumerate() {
            let alive = *hp > 0.0;
            let before = snap.turrets.get(i).copied().unwrap_or(alive || first);
            let tag = TAG_TURRET | ((city.index as u64) << 8) | i as u64;
            let at = crate::civ_rts::turret_world(city, i, shape);
            if before && !alive {
                play(&mut plays, TURRET_DESTROYED, at, rotation, 0, None);
                play(&mut plays, TURRET_DESTROYED_LOOP, at, rotation, tag, None);
            } else if !before && alive {
                stops.write(SwarmStop { effect: 0, attach_to: None, tag, hard: false });
            } else if first && !alive {
                play(&mut plays, TURRET_DESTROYED_LOOP, at, rotation, tag, None);
            }
        }
        // Removed turrets stop smouldering.
        for i in city.turret_hp.len()..snap.turrets.len() {
            let tag = TAG_TURRET | ((city.index as u64) << 8) | i as u64;
            stops.write(SwarmStop { effect: 0, attach_to: None, tag, hard: false });
        }
        snap.turrets = city.turret_hp.iter().map(|hp| *hp > 0.0).collect();
    }
    // Vehicles: rolled out, destroyed, and their own effects (0xE956D0).
    let mut seen: HashSet<Entity> = HashSet::new();
    let mut intensities: HashMap<Entity, f32> = HashMap::new();
    for (entity, unit, transform) in &units {
        seen.insert(entity);
        let at = transform.translation();
        let rotation = upright(shape, at);
        let health = if unit.max_hp > 0.0 { unit.hp / unit.max_hp } else { 1.0 };
        let shown = match memory.units.get(&entity) {
            None => {
                if !first {
                    play(&mut plays, CREATE_VEHICLE, at, rotation, 0, None);
                }
                UnitFx { at, ..default() }
            }
            Some(shown) => *shown,
        };
        let moving = trail(unit.domain, unit.velocity.length(), unit.stats.speed);
        let (trail_now, fade, intensity) = trail_step(shown.trail, shown.fade, moving, dt);
        if let Some(v) = intensity {
            intensities.insert(entity, v);
        }
        let want = UnitFx {
            at,
            trail: trail_now,
            fade,
            damage: vehicle_damage(health),
            stunned: unit.stunned > 0.0,
            cultural: unit.cultural_hit < CULTURE_HOLD,
        };
        for (tag, was, now) in [
            (TAG_TRAIL, shown.trail, want.trail),
            (TAG_VEHICLE_DAMAGE, shown.damage, want.damage),
            (TAG_VEHICLE_STUN, shown.stunned.then_some(VEHICLE_STUNNED), want.stunned.then_some(VEHICLE_STUNNED)),
            (TAG_VEHICLE_CULTURE, shown.cultural.then_some(VEHICLE_CULTURE), want.cultural.then_some(VEHICLE_CULTURE)),
        ] {
            if was == now {
                continue;
            }
            if was.is_some() {
                stops.write(SwarmStop { effect: 0, attach_to: Some(entity), tag, hard: false });
            }
            if let Some(name) = now {
                // The vehicle root is drawn CITY_SCALE times larger than its
                // world size (its layout is in island units): the trail is
                // authored in world units (ribbon width 6, 40 long at
                // scale 1), so the root's scale is taken back out.
                // ADAPTED: the retail effect scale of a vehicle was not
                // traced; world scale 1 is what the authored data fits (a
                // 6-unit ribbon behind a vehicle of at most 6 units).
                let mut p = SwarmPlay::at(name_id(name), Vec3::ZERO);
                p.tag = tag;
                p.attach_to = Some(entity);
                // The damage / stun effects take the same world scale (round 2: they
                // were drawn CITY_SCALE times too large at the root's scale).
                // ADAPTED like the trail.
                if tag == TAG_TRAIL || crate::civ_vfx::on() {
                    p.scale = 1.0 / crate::civ_planet::CITY_SCALE;
                }
                plays.write(p);
            }
        }
        memory.units.insert(entity, want);
    }
    // The trails' intensity (effect parameter 4) while they fade or resume.
    if !intensities.is_empty() {
        for mut effect in &mut effects {
            if effect.tag != TAG_TRAIL {
                continue;
            }
            if let Some(v) = effect.attach_to.and_then(|e| intensities.get(&e)) {
                effect.params[crate::swarm_fx::sim::PARAM_INTENSITY] = Some(Vec4::new(*v, 0.0, 0.0, 0.0));
            }
        }
    }
    let gone: Vec<(Entity, Vec3)> =
        memory.units.iter().filter(|(e, _)| !seen.contains(*e)).map(|(e, fx)| (*e, fx.at)).collect();
    for (entity, at) in gone {
        memory.units.remove(&entity);
        play(&mut plays, VEHICLE_EXPLOSION, at, upright(shape, at), 0, None);
    }
}

// ---------------------------------------------------------------------------
// Wall turret facing
// ---------------------------------------------------------------------------

/// A turret turns toward its target at a quarter turn a second (`cTurret` update 0xBD1E70, 0xBD239B: the
/// float 1.5708 at 0x156D70C when it has a target) ...
pub const TURRET_TRACK_RATE: f32 = std::f32::consts::FRAC_PI_2;
/// ... and scans at 0.785398 x 0.666 = 0.5231 rad/s while it has none (0xBD23A9).
pub const TURRET_SCAN_RATE: f32 = std::f32::consts::FRAC_PI_4 * 0.666;
/// A scan step is reached when the facing is within `acos(0.96)` (16.3 degrees) of it (0xBD1FC2: `[0x1467A40]`).
pub const TURRET_SCAN_ARRIVE: f32 = 0.96;
/// The scan's side step: 45 degrees (0xBD20EF: `[0x156D708]`).
pub const TURRET_SCAN_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// The idle look-around of a wall turret. VERIFIED 0xBD1E70 (the turret facing update): without a
/// target (`[aim + 0x10]` 0) the turret picks its next look direction each time it is within 0.96
/// (dot product) of the last, or the moment it loses its target: a counter (`[aim + 0x14]`) counts
/// the steps; an odd step looks straight out (the direction from the city to the turret), step 4k
/// 45 degrees to one side, step 4k + 2 to the other (0xBD20D0 .. 0xBD2105); the turret turns toward the
/// step at `TURRET_SCAN_RATE`. The retail steps also tilt the barrel (up on the odd steps, down on the
/// even ones, 2/3 of the up vector, 0xBD21F1 / 0xBD2251): ADAPTED, OpenSpore's gun only turns.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurretScan {
    /// `[aim + 0x14]`: the scan steps taken.
    pub counter: u32,
    /// `[aim + 0x10]`: it had a target last update.
    pub tracking: bool,
    /// The current step's yaw relative to the way out.
    pub offset: f32,
}

/// The side step of scan step `counter` (0xBD20D0 .. 0xBD2105).
pub fn scan_offset(counter: u32) -> f32 {
    if counter % 2 == 1 {
        0.0
    } else if counter % 4 == 0 {
        TURRET_SCAN_ANGLE
    } else {
        -TURRET_SCAN_ANGLE
    }
}

fn wrap_angle(a: f32) -> f32 {
    let a = a % std::f32::consts::TAU;
    if a > std::f32::consts::PI {
        a - std::f32::consts::TAU
    } else if a < -std::f32::consts::PI {
        a + std::f32::consts::TAU
    } else {
        a
    }
}

impl TurretScan {
    /// One update: the new yaw of a turret facing `current`. `outward` is the yaw of the way out of
    /// the city at this turret, `target` the yaw to the target it is firing at, if any.
    pub fn step(&mut self, current: f32, outward: f32, target: Option<f32>, dt: f32) -> f32 {
        let (desired, rate) = match target {
            Some(yaw) => {
                self.tracking = true;
                (yaw, TURRET_TRACK_RATE)
            }
            None => {
                let arrived = wrap_angle(outward + self.offset - current).cos() > TURRET_SCAN_ARRIVE;
                if self.tracking || arrived {
                    self.counter += 1;
                    self.tracking = false;
                    self.offset = scan_offset(self.counter);
                }
                (outward + self.offset, TURRET_SCAN_RATE)
            }
        };
        let delta = wrap_angle(desired - current);
        // A constant angular speed (0xBD23E4: the fraction is rate x dt over the angle, at most 1).
        current + delta.clamp(-rate * dt, rate * dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::{Building, HOME_INDEX};

    #[test]
    fn plots_raise_dust_explode_and_smoulder_by_their_damage() {
        let mut city = CityState::new(HOME_INDEX);
        let free = (0..city.plots.len()).find(|p| city.plots[*p].is_none()).unwrap();
        assert_eq!(plot_look(&city, free), PlotLook::Empty);
        city.plots[free] = Some(Building::House);
        assert_eq!(plot_look(&city, free), PlotLook::Standing(0));
        assert_eq!(plot_change(PlotLook::Empty, PlotLook::Standing(0)), vec![PLOP_DUST]);
        city.building_damage[free] = 400.0;
        assert_eq!(plot_look(&city, free), PlotLook::Standing(1));
        assert_eq!(damage_loop(plot_look(&city, free)), Some(BUILDING_PARTIALLY_DAMAGED));
        city.building_damage[free] = 700.0;
        assert_eq!(damage_loop(plot_look(&city, free)), Some(BUILDING_DAMAGED));
        // The thresholds are 0.8 and 0.5 of the health (0xE8A89C), not thirds.
        city.building_damage[free] = 0.15 * crate::civ::BUILDING_HP;
        assert_eq!(plot_look(&city, free), PlotLook::Standing(0), "85 % health: no smoke");
        city.building_damage[free] = 0.25 * crate::civ::BUILDING_HP;
        assert_eq!(plot_look(&city, free), PlotLook::Standing(1), "75 % health: partially damaged");
        city.building_damage[free] = 0.45 * crate::civ::BUILDING_HP;
        assert_eq!(plot_look(&city, free), PlotLook::Standing(1), "55 % health: still partial");
        city.building_damage[free] = 0.55 * crate::civ::BUILDING_HP;
        assert_eq!(plot_look(&city, free), PlotLook::Standing(2), "45 % health: damaged");
        city.damage_building(free, 800.0, 2);
        assert_eq!(plot_look(&city, free), PlotLook::Ruined);
        assert_eq!(damage_loop(PlotLook::Ruined), None, "a ruin smoulders in no damage slot");
        assert_eq!(plot_change(PlotLook::Standing(2), PlotLook::Ruined), vec![BASIC_EXPLOSION]);
        // A rival rebuilding on the ruin raises dust again.
        assert_eq!(plot_change(PlotLook::Ruined, PlotLook::Standing(0)), vec![PLOP_DUST]);
    }

    #[test]
    fn the_zoning_ring_follows_the_territory_display_strategy() {
        assert_eq!(ring_segments(160), 160);
        assert_eq!(ring_segments(3), 3);
        assert_eq!(ring_segments(500), 500);
        assert_eq!(ring_segments(2), 500, "out of range: 500");
        assert_eq!(ring_segments(501), 500);
        // A tenth of the way per frame.
        assert!((ring_radius_step(0.0, 100.0) - 10.0).abs() < 1e-5);
        assert!((ring_radius_step(10.0, 100.0) - 19.0).abs() < 1e-5);
        // The ring closes on itself and keeps its radius around the centre.
        let globe = Globe::flat();
        let center = Vec3::new(20.0, 0.0, -30.0);
        let points = ring_points(&globe, center, 50.0, 64);
        assert_eq!(points.len(), 65);
        assert!(points[0].distance(points[64]) < 1e-2);
        for p in &points {
            let flat = Vec2::new(p.x - center.x, p.z - center.z).length();
            assert!((flat - 50.0).abs() < 1e-2, "{flat}");
        }
    }

    #[test]
    fn rival_circles_take_the_owner_colour_and_yours_stays_authored() {
        assert_eq!(owner_tint(crate::civ::HOME_INDEX), Vec3::ONE);
        // Nation 1 is red: the largest channel is normalised to 1, red stays dominant.
        let red = owner_tint(1);
        assert!((red.max_element() - 1.0).abs() < 1e-5, "{red:?}");
        assert!(red.x > red.y && red.x > red.z, "{red:?}");
        for nation in 1..10 {
            let t = owner_tint(nation);
            assert!(t.min_element() >= 0.0 && (t.max_element() - 1.0).abs() < 1e-5, "{nation}: {t:?}");
        }
        // A clicked rival keeps its circle for a while only.
        assert!(Inspected::new(10.0).until > 10.0 && Inspected::new(10.0).until <= 10.0 + INSPECT_SECONDS + 1e-6);
    }

    #[test]
    fn trails_follow_the_locomotion_and_the_speed_stat() {
        assert_eq!(trail(VehicleDomain::Land, 0.1, 0.9), None, "standing still");
        assert_eq!(trail(VehicleDomain::Land, 3.0, 0.2), Some("civ_vehicle_ground_moving_slow"));
        assert_eq!(trail(VehicleDomain::Land, 0.5, 0.33), Some("civ_vehicle_ground_moving_medium"));
        assert_eq!(trail(VehicleDomain::Sea, 20.0, 0.66), Some("civ_vehicle_boat_wake_fast"));
        assert_eq!(trail(VehicleDomain::Air, 23.2, 0.5), Some("civ_vehicle_air_contrails_medium"));
        // The cVehicle constructor's 0.33: medium.
        let stat = crate::civ_combat::VehicleStats::default().speed;
        assert_eq!(trail(VehicleDomain::Air, 1.0, stat), Some("civ_vehicle_air_contrails_medium"));
        assert_eq!(vehicle_damage(1.0), None);
        assert_eq!(vehicle_damage(0.66), None);
        assert_eq!(vehicle_damage(0.5), Some(VEHICLE_DAMAGE_MINOR));
        assert_eq!(vehicle_damage(0.1), Some(VEHICLE_DAMAGE_MAJOR));
        // The trail fades over 1 s after stopping, then stops; moving again
        // inside the fade keeps the same trail at full intensity.
        let slow = Some("civ_vehicle_ground_moving_slow");
        let fast = Some("civ_vehicle_ground_moving_fast");
        assert_eq!(trail_step(None, None, slow, 0.1), (slow, None, None));
        assert_eq!(trail_step(slow, None, fast, 0.1), (slow, None, None), "the tier stays");
        let (t, fade, v) = trail_step(slow, None, None, 0.25);
        assert_eq!((t, fade), (slow, Some(0.25)));
        assert!((v.unwrap() - 0.75).abs() < 1e-6);
        assert_eq!(trail_step(slow, Some(0.25), fast, 0.1), (slow, None, Some(1.0)));
        assert_eq!(trail_step(slow, Some(0.95), None, 0.1), (None, None, None));
    }

    /// `cargo test survey_night_light_models -- --ignored --nocapture`: the
    /// GameModel components (section 0x25, SporeModder-FX `GameModelEffect`)
    /// with the `applyNightLights` option (bit 10) and the effects that use
    /// them.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_night_light_models() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        let names: std::collections::HashMap<u32, String> = std::fs::read_to_string(
            r"D:\SteamLibrary\steamapps\common\Spore\OpenSpore\target\format-reference\reg_file.txt",
        )
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split('\t').next())
        .filter(|n| !n.is_empty())
        .map(|n| (crate::world_objects::name_id(n), n.to_string()))
        .collect();
        let name = |id: u32| names.get(&id).cloned().unwrap_or_else(|| format!("{id:08x}"));
        for (d, dir) in lib.dirs.iter().enumerate() {
            let key = crate::dbpf::ResourceKey {
                type_id: crate::swarm_fx::format::TYPE_EFFDIR,
                group_id: dir.key_group,
                instance_id: dir.key_instance,
            };
            let Ok(data) = index.read(key) else { continue };
            let be32 = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap());
            let be16 = |at: usize| u16::from_be_bytes(data[at..at + 2].try_into().unwrap());
            let mut at = 2;
            let mut models: Vec<(u32, u32, u32, u32)> = Vec::new();
            loop {
                let kind = be16(at);
                if kind == 0xFFFF {
                    break;
                }
                let version = be16(at + 2);
                let size = be32(at + 4) as usize;
                let offset = at + 8;
                if kind == 0x25 {
                    let count = be32(offset) as usize;
                    let mut p = offset + 4;
                    for _ in 0..count {
                        let flags = be32(p);
                        let instance = be32(p + 20);
                        let group = be32(p + 24);
                        p += 4 + 4 + 12 + 4 + 12;
                        let anims = be32(p) as usize;
                        p += 4;
                        for _ in 0..anims {
                            p += 8;
                            let n = be32(p) as usize;
                            p += 4 + n * 4 + 4 + 4 + 4 + 1;
                        }
                        let splitters = be32(p) as usize;
                        p += 4 + splitters * 6;
                        let splits = be32(p) as usize;
                        p += 4;
                        for _ in 0..splits {
                            p += 1 + 4 + 54;
                            let n = be32(p) as usize;
                            p += 4 + n * 4 + 12;
                        }
                        let groups = be32(p) as usize;
                        p += 4 + groups * 4;
                        let options = be32(p);
                        p += 4 + 2;
                        if version >= 8 {
                            p += 4;
                        }
                        models.push((flags, instance, group, options));
                    }
                    println!("dir {d}: {count} game models, parsed to {p} of section end {}", offset + size);
                }
                at = offset + size;
            }
            let mut bits = [0usize; 16];
            for m in &models {
                for (b, n) in bits.iter_mut().enumerate() {
                    if m.3 & (1 << b) != 0 {
                        *n += 1;
                    }
                }
            }
            if !models.is_empty() {
                println!("  option bits {bits:?}");
            }
            let mut exports: std::collections::HashMap<i32, Vec<u32>> = Default::default();
            for (n, i) in &dir.exports {
                exports.entry(*i).or_default().push(*n);
            }
            // Blocks gated on kAppFlagIsNight (bit 0x15).
            for (e, effect) in dir.effects.iter().enumerate() {
                for b in &effect.blocks {
                    if b.app_flags_mask & (1 << 0x15) != 0 {
                        let ex: Vec<String> = exports.get(&(e as i32)).map(|v| v.iter().map(|n| name(*n)).collect()).unwrap_or_default();
                        println!(
                            "  night block: effect #{e} {ex:?} kind {:#x} index {} flags {:x} mask {:x}",
                            b.kind, b.index, b.app_flags, b.app_flags_mask
                        );
                    }
                }
            }
            for (e, effect) in dir.effects.iter().enumerate() {
                for b in &effect.blocks {
                    if b.kind == 0x25 {
                        if let Some(m) = models.get(b.index as usize) {
                            if m.3 & (1 << 10) != 0 {
                                let ex: Vec<String> = exports.get(&(e as i32)).map(|v| v.iter().map(|n| name(*n)).collect()).unwrap_or_default();
                                println!(
                                    "  effect #{e} {ex:?}: game model {}!{} flags {:x} options {:x}",
                                    name(m.2),
                                    name(m.1),
                                    m.0,
                                    m.3
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// `cargo test survey_building_pieces -- --ignored --nocapture`: the
    /// pieces of the installed building models (GMDL, group 0x40637100) with
    /// their material ids, textures and shader data.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_building_pieces() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let keys = crate::baked_model::candidates(&index, crate::baked_model::BUILDING_GROUP);
        let mut ids: std::collections::BTreeMap<u32, (usize, usize)> = Default::default();
        for (n, key) in keys.iter().enumerate() {
            let Ok(model) = index.read(*key).and_then(|b| crate::baked_model::parse(&b)) else { continue };
            for (piece, info) in model.pieces.iter().zip(&model.materials) {
                let e = ids.entry(piece.material_id).or_default();
                e.0 += 1;
                if info.textures.is_empty() && info.shader_data.is_empty() {
                    e.1 += 1;
                    if e.1 <= 12 {
                        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for i in &piece.indices {
                            let v = Vec3::from_array(piece.positions[*i as usize]);
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                        let (mut alo, mut ahi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for v in &piece.positions {
                            alo = alo.min(Vec3::from_array(*v));
                            ahi = ahi.max(Vec3::from_array(*v));
                        }
                        println!(
                            "  bare piece of {:08x}: {} indices, box {lo:?}..{hi:?} (model {alo:?}..{ahi:?}), references {:?}",
                            key.instance_id,
                            piece.indices.len(),
                            model.references
                        );
                    }
                }
                if n < 3 {
                    println!(
                        "{:08x} piece material {:08x} verts {} textures {:?} data {:?}",
                        key.instance_id,
                        piece.material_id,
                        piece.positions.len(),
                        info.textures.iter().map(|t| (t.sampler, t.key)).collect::<Vec<_>>(),
                        info.shader_data.iter().map(|(k, v)| (*k, v.len())).collect::<Vec<_>>()
                    );
                }
            }
        }
        println!("{} building models", keys.len());
        for (id, (n, bare)) in ids {
            println!("material id {id:08x}: {n} pieces, {bare} without textures / data");
        }
    }

    /// `cargo test survey_window_interiors -- --ignored --nocapture`: the
    /// embedded interior rasters of the window parts (renderer 0x40000007).
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_window_interiors() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut hashes: std::collections::BTreeMap<u64, Vec<u32>> = Default::default();
        for key in index.records.keys().filter(|k| k.group_id == 0x4060_6000 && k.type_id == crate::asset_catalog::RW4) {
            let Ok(blob) = index.read(*key) else { continue };
            let Ok(sections) = crate::rw4::sections(&blob) else { continue };
            for (kind, payload) in &sections {
                if *kind != crate::rw4::T_COMPILED_STATE {
                    continue;
                }
                let Ok(state) = crate::material_state::parse(payload) else { continue };
                if state.renderer_id != crate::material_binding::WINDOW_INTERIOR_RENDERER {
                    continue;
                }
                let Some(slot) = state.texture_slots.iter().find(|s| s.sampler == 0) else {
                    println!("{:08x}: no sampler 0", key.instance_id);
                    continue;
                };
                let Some(mips) = crate::texture::decode_raster_mips(&blob, slot.raster_reference as usize) else {
                    println!("{:08x}: raster {:x} not embedded", key.instance_id, slot.raster_reference);
                    continue;
                };
                let p = &mips[0];
                let h = p.rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
                let avg: [u64; 4] = std::array::from_fn(|c| p.rgba.iter().skip(c).step_by(4).map(|v| *v as u64).sum::<u64>() / (p.rgba.len() as u64 / 4).max(1));
                println!("{:08x}: interior {}x{} avg {avg:?} hash {h:016x}", key.instance_id, p.width, p.height);
                hashes.entry(h).or_default().push(key.instance_id);
            }
        }
        println!("{} distinct interior rasters: {:?}", hashes.len(), hashes.values().map(|v| v.len()).collect::<Vec<_>>());
    }

    #[test]
    fn an_idle_turret_looks_straight_out_then_to_each_side() {
        // The scan steps: odd = straight out, 4k = +45, 4k + 2 = -45 degrees (0xBD20D0).
        assert_eq!(scan_offset(1), 0.0);
        assert_eq!(scan_offset(3), 0.0);
        assert!((scan_offset(4) - TURRET_SCAN_ANGLE).abs() < 1e-6);
        assert!((scan_offset(2) + TURRET_SCAN_ANGLE).abs() < 1e-6);
        assert!((scan_offset(6) + TURRET_SCAN_ANGLE).abs() < 1e-6);
        assert!((scan_offset(8) - TURRET_SCAN_ANGLE).abs() < 1e-6);
        // 0.5231 rad/s while scanning, a quarter turn a second on a target.
        assert!((TURRET_SCAN_RATE - 0.5231).abs() < 1e-3);
        assert!((TURRET_TRACK_RATE - 1.5708).abs() < 1e-3);
    }

    #[test]
    fn a_scanning_turret_swings_to_both_sides_at_a_constant_speed() {
        let mut scan = TurretScan::default();
        let (mut yaw, mut lo, mut hi) = (0.0f32, 0.0f32, 0.0f32);
        let dt = 1.0 / 30.0;
        let mut last = yaw;
        let mut biggest_step = 0.0f32;
        for _ in 0..30 * 40 {
            yaw = scan.step(yaw, 0.0, None, dt);
            biggest_step = biggest_step.max((yaw - last).abs());
            last = yaw;
            lo = lo.min(yaw);
            hi = hi.max(yaw);
        }
        // It turns toward 45 degrees until it is within acos(0.96) of it: about 29 degrees.
        let reach = TURRET_SCAN_ANGLE - TURRET_SCAN_ARRIVE.acos();
        assert!((hi - reach).abs() < 0.03, "right edge {hi} vs {reach}");
        assert!((lo + reach).abs() < 0.03, "left edge {lo}");
        assert!(biggest_step <= TURRET_SCAN_RATE * dt + 1e-5, "constant speed: {biggest_step}");
        assert!(scan.counter > 8, "the steps keep coming: {}", scan.counter);
    }

    #[test]
    fn a_turret_with_a_target_turns_to_it_and_resumes_the_scan_after() {
        let mut scan = TurretScan::default();
        let dt = 1.0 / 30.0;
        let mut yaw = 0.0;
        for _ in 0..60 {
            yaw = scan.step(yaw, 0.0, Some(1.2), dt);
        }
        assert!(scan.tracking);
        assert!((yaw - 1.2).abs() < 1e-3, "{yaw}");
        // The target is gone: the next update starts a new step (counter + 1) and turns back.
        let before = scan.counter;
        let next = scan.step(yaw, 0.0, None, dt);
        assert!(!scan.tracking);
        assert_eq!(scan.counter, before + 1);
        assert!(next < yaw, "turning back toward the way out");
        // A step through the wrap-around turns the short way.
        let mut wrap = TurretScan::default();
        let turned = wrap.step(3.0, 0.0, Some(-3.0), 0.1);
        assert!(turned > 3.0, "{turned}: the short way crosses pi");
    }

    #[test]
    fn a_cultural_hit_plays_the_culture_effect_on_the_vehicle_and_stops_it_later() {
        use crate::civ::{Approach, UnitState};
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        let state = UnitState { id: 902, nation: HOME_INDEX, kind: Approach::Religious, ..Default::default() };
        let mut unit = CivUnit::from_state(&state);
        unit.cultural_hit = 0.0;
        let vehicle = app.world_mut().spawn((unit, Transform::from_translation(Vec3::new(500.0, 0.0, 500.0)))).id();
        let culture = |app: &mut App| {
            let mut query = app.world_mut().query::<&SwarmEffect>();
            query
                .iter(app.world())
                .filter(|e| e.attach_to == Some(vehicle) && e.effect == name_id(VEHICLE_CULTURE))
                .map(|e| e.stop)
                .collect::<Vec<_>>()
        };
        for _ in 0..4 {
            app.update();
        }
        let live = culture(&mut app);
        assert!(live.contains(&false), "the effect plays while the hit is fresh: {live:?}");
        // Long after the last hit: it is stopped (event 0x19).
        app.world_mut().get_mut::<CivUnit>(vehicle).unwrap().cultural_hit = 10.0;
        for _ in 0..4 {
            app.update();
        }
        let after = culture(&mut app);
        assert!(after.iter().all(|stop| *stop), "stopped: {after:?}");
    }
}
