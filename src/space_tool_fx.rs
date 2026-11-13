//! Space tool effects from the retail tool records (helper `space-tools-fx`).
//!
//! Every retail tool (`spacetools~` group 0x30608F0B, 363 property files) names
//! the Swarm effects it plays in its property file (VERIFIED data, read with
//! `tools\toolfx.py` and here at run time): `toolMuzzleEffectID` 0x03025B14 (at the
//! ship), `toolProjectileEffectID` 0x03025B0B (the flying shot),
//! `toolBeamEffectID` 0x1138C0B9 (ribbons from the ship to the target),
//! `toolTargetEffectID` 0x0317B2CC (a mark on the target), `toolHitGroundEffectID`
//! 0x3138C0BF / `toolHitWaterEffectID` 0x3138C0C2 / `toolHitCombatantEffectID`
//! 0x3138C0C6 / `toolHitPlayerEffectID` 0x047F7359 (where the shot lands, by what
//! it lands on), `toolGenericEffectID` 0x306C86EF (self effects: scan the planet,
//! the escape portal, potions) and `toolHitOrientsToTerrain` 0x3138C9B1. The
//! projectile speed and time-out are `toolProjectileSpeed` 0x023FBE01 (retail
//! units per second) and `toolProjectileTimeOut` 0x030D28C1.
//!
//! This module reads those keys for any tool by name ([`ToolEffects`]), and
//! plays them when the Space rules use the tool: the action systems of
//! `space.rs` spawn a [`ToolFxUse`] request (scan V, tractor beam U, colony pack
//! C, the archetype superpower R, the planetary tool Y, the wingmen's and colony
//! turrets' shots), [`play_uses`] resolves the aim point on the world's surface
//! (the planet view's ground for decals and hits, the world sphere in the
//! solar system view) and starts the effects: the muzzle at the ship, the beam
//! along its path, the projectile flying to the aim point and the hit effect
//! where it lands. `--space-tool-fx NAME[,NAME..]` replays tools without any
//! game state for captures.
//!
//! Held tools (VERIFIED, `space-weapons`): the retail beam and ray tools run
//! while the button is held (the hold handler 0x1050500 calls
//! `cSpaceToolData::Use` 0x1050080 again every `AutoFireRate`, 0.1 s for the
//! rays and lasers, and each use re-aims the beam, `cDefaultBeamTool::
//! WhileFiring` 0x1059EF0). [`request_held`] asks for one such use: the first
//! starts the muzzle, beam, target mark and hit effect; the next ones, while
//! the effects are up ([`HOLD_TIMEOUT`]), only refresh them and move the beam's
//! end and the hit to the new aim point; when the uses stop the effects end
//! softly. Single-use tools ([`request`]) play once as before.
//!
//! ADAPTED (the rest of the effect plumbing of the retail tool code was not traced):
//! * a single use keeps its beam up for [`BEAM_SECONDS`] and plays the hit
//!   effect once where the beam lands;
//! * the aim point is the cursor ray's hit on the world when the cursor is over
//!   it, else the ground a short way ahead of the ship (retail tools fire at the
//!   cursor target; OpenSpore applies a tool to the world under the ship);
//! * projectiles fly straight at the retail `toolProjectileSpeed` (clamped to
//!   [`MIN_FLIGHT`]..[`MAX_FLIGHT`] seconds so the shot reads), the sky tools
//!   (asteroid, comet, meteors: a beam and a projectile) drop their projectile
//!   from [`SKY_HEIGHT`] retail units above the aim point;
//! * effects are upright on the surface normal (`Vec3::Y` -> normal), the muzzle
//!   and the hit effects use no other orientation;
//! * effects the retail code plays by name (no tool key): `SG_ufo_place_colony_
//!   projectile_explosion` where the colony pack lands (its tool names no hit),
//!   `SG_ufo_drop_cargo_HitGround` for the drop cargo tool.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::asset_catalog::ResourceIndex;
use crate::dbpf::ResourceKey;
use crate::game::Player;
use crate::persistence::WorldId;
use crate::prop::PropertyList;
use crate::space::PlanetaryTool;
use crate::space_planets::PlanetViewGround;
use crate::space_rules::RETAIL_UNITS_PER_WORLD_UNIT;
use crate::space_view::SpaceView;
use crate::stages::{GameStage, StageOwned};
use crate::swarm_fx::{SwarmEffect, SwarmLibrary};

/// `spacetools~`.
pub const TOOL_GROUP: u32 = 0x3060_8F0B;

/// Property ids of the tool records.
pub mod prop_id {
    pub const MUZZLE: u32 = 0x0302_5B14;
    pub const PROJECTILE: u32 = 0x0302_5B0B;
    pub const BEAM: u32 = 0x1138_C0B9;
    pub const TARGET: u32 = 0x0317_B2CC;
    pub const HIT_GROUND: u32 = 0x3138_C0BF;
    pub const HIT_WATER: u32 = 0x3138_C0C2;
    pub const HIT_COMBATANT: u32 = 0x3138_C0C6;
    pub const HIT_PLAYER: u32 = 0x047F_7359;
    pub const GENERIC: u32 = 0x306C_86EF;
    pub const HIT_ORIENTS_TO_TERRAIN: u32 = 0x3138_C9B1;
    pub const PROJECTILE_SPEED: u32 = 0x023F_BE01;
    pub const PROJECTILE_TIME_OUT: u32 = 0x030D_28C1;
}

/// Seconds a single-use tool's beam stays up per use (ADAPTED, see the module docs).
pub const BEAM_SECONDS: f32 = 1.2;
/// Seconds the effects of a held tool outlive its last use: longer than the
/// 0.1 s `AutoFireRate` of the rays, so a held ray never flickers, short
/// enough that the beam ends with the button.
pub const HOLD_TIMEOUT: f32 = 0.35;
/// Seconds a laser-class beam (wingmen, turrets) stays up per shot (the shots
/// repeat every 0.85 / 1 s).
pub const SHOT_BEAM_SECONDS: f32 = 0.3;
/// Shortest and longest projectile flight shown, seconds (ADAPTED).
pub const MIN_FLIGHT: f32 = 0.25;
pub const MAX_FLIGHT: f32 = 1.6;
/// Height the sky tools drop their projectile from, retail units (ADAPTED).
pub const SKY_HEIGHT: f32 = 260.0;
/// How far ahead of the ship, as a fraction of its altitude, the aim point is
/// when no cursor ray hits the world in the planet view (ADAPTED).
pub const AIM_AHEAD: f32 = 0.55;
/// Seconds a one-shot effect (muzzle, self effect) is kept running before a
/// soft stop, for the looping ones.
const ONE_SHOT_SECONDS: f32 = 2.0;
/// Seconds a hit effect is kept before a soft stop.
const HIT_SECONDS: f32 = 3.0;

/// The effects one tool names (`0` ids are absent).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolEffects {
    pub muzzle: Option<u32>,
    pub projectile: Option<u32>,
    pub beam: Option<u32>,
    pub target: Option<u32>,
    pub hit_ground: Option<u32>,
    pub hit_water: Option<u32>,
    pub hit_combatant: Option<u32>,
    pub hit_player: Option<u32>,
    pub generic: Option<u32>,
    pub orients_to_terrain: bool,
    /// Retail units per second (`toolProjectileSpeed`), 0 when unnamed.
    pub projectile_speed: f32,
    /// Seconds (`toolProjectileTimeOut`), 0 when unnamed.
    pub projectile_time_out: f32,
}

impl ToolEffects {
    /// The effect keys of a tool's property list.
    pub fn from_props(props: &PropertyList) -> Self {
        let effect = |id: u32| props.0.get(&id).and_then(|p| p.key()).map(|k| k.instance_id).filter(|i| *i != 0);
        let number = |id: u32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(0.0);
        Self {
            muzzle: effect(prop_id::MUZZLE),
            projectile: effect(prop_id::PROJECTILE),
            beam: effect(prop_id::BEAM),
            target: effect(prop_id::TARGET),
            hit_ground: effect(prop_id::HIT_GROUND),
            hit_water: effect(prop_id::HIT_WATER),
            hit_combatant: effect(prop_id::HIT_COMBATANT),
            hit_player: effect(prop_id::HIT_PLAYER),
            generic: effect(prop_id::GENERIC),
            orients_to_terrain: number(prop_id::HIT_ORIENTS_TO_TERRAIN) > 0.5,
            projectile_speed: number(prop_id::PROJECTILE_SPEED),
            projectile_time_out: number(prop_id::PROJECTILE_TIME_OUT),
        }
    }

    /// Every effect id the tool names, once each.
    pub fn all(&self) -> Vec<u32> {
        let mut out: Vec<u32> = [
            self.muzzle,
            self.projectile,
            self.beam,
            self.target,
            self.hit_ground,
            self.hit_water,
            self.hit_combatant,
            self.hit_player,
            self.generic,
        ]
        .into_iter()
        .flatten()
        .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The hit effect for what the shot lands on, falling back along the
    /// retail pairs (a tool that names only a ground hit uses it everywhere).
    pub fn hit(&self, kind: HitKind) -> Option<u32> {
        match kind {
            HitKind::Ground => self.hit_ground,
            HitKind::Water => self.hit_water.or(self.hit_ground),
            HitKind::Combatant => self.hit_combatant.or(self.hit_ground),
            HitKind::Player => self.hit_player.or(self.hit_combatant).or(self.hit_ground),
        }
    }

    /// The tool shows something when used.
    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }
}

/// What a shot lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Ground,
    Water,
    Combatant,
    Player,
}

/// Read the effect keys of the retail tool `name` from the install.
pub fn read_tool_effects(index: &ResourceIndex, name: &str) -> Option<ToolEffects> {
    let key = ResourceKey {
        group_id: TOOL_GROUP,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    index.properties(key).ok().map(|p| ToolEffects::from_props(&p))
}

/// The retail tool of each OpenSpore planetary tool (the palette stand-ins of
/// `space_hud::palette::TOOLS`; the seeding tools drop cargo).
pub fn planetary_tool_id(tool: PlanetaryTool) -> &'static str {
    match tool {
        PlanetaryTool::Warm => "heatray",
        PlanetaryTool::Cool => "coldray",
        PlanetaryTool::ThickenAtmosphere => "cloudray",
        PlanetaryTool::ThinAtmosphere => "cloudvacuum",
        PlanetaryTool::SeedFlora | PlanetaryTool::SeedHerbivore | PlanetaryTool::SeedCarnivore => "dropcargo",
        PlanetaryTool::HappyRay => "happyraylevel1",
        PlanetaryTool::StaffOfLife => "genesisdevice",
        PlanetaryTool::Monolith => "monolith",
        PlanetaryTool::PlanetBuster => "planetbusterbomb",
        PlanetaryTool::AtmosphereFreeze => "coldcloudvacuum",
        PlanetaryTool::HeatRay => "hotcloudray",
        PlanetaryTool::AsteroidCaller => "asteroid",
        PlanetaryTool::CreatureTweaker => "creatureedit",
        PlanetaryTool::ColdCloudRay => "coldcloudray",
        PlanetaryTool::HotCloudVacuum => "hotcloudvacuum",
        PlanetaryTool::Volcano => "volcano",
        PlanetaryTool::IceComet => "icecomet",
        PlanetaryTool::IceCapSeeder => "icecapseeder",
        PlanetaryTool::RainOfFire => "rainoffire",
        PlanetaryTool::DryIce => "dryice",
        PlanetaryTool::AtmosphereGenerator => "atmospheregenerator",
        PlanetaryTool::DroughtGenerator => "droughtgenerator",
    }
}

/// Effects the retail code plays by name next to a tool's own keys (ADAPTED
/// association by name): (tool, effect name, where: the landing point).
pub const CODE_NAMED: &[(&str, &str)] = &[
    ("placecolony", "SG_ufo_place_colony_projectile_explosion"),
    ("dropcargo", "SG_ufo_drop_cargo_HitGround"),
    // 0xE9A0A0 (the UFO effect function, flag 3): the player's ship plays this on a Rally Call
    // (a posse ship's is `SG_ufo_rally_response` 0x20316487, not played here).
    ("rallycall", "SG_ufo_rally_call"),
];

/// Tools the Space rules use today, with the effects they show (the wired
/// set, counted by the tests): the player's tools of the palette, the
/// planetary tools, the archetype superpowers and the allied / colony shots.
pub fn wired_tools() -> Vec<&'static str> {
    let mut tools: Vec<&'static str> = vec![
        "scan",
        "Planetscan",
        "abduct",
        "placecolony",
        "laserlevel1",
        "laserlevel2",
        "laserlevel3",
        "allylaser1",
        "turretlaser1",
        "dropcargo",
    ];
    for tool in PlanetaryTool::ALL {
        tools.push(planetary_tool_id(tool));
    }
    for archetype in crate::space::SpaceArchetype::ALL {
        if let Some(power) = crate::space_rules::archetype_superpower(archetype) {
            tools.push(power.id);
        }
    }
    tools.sort_unstable();
    tools.dedup();
    tools
}

/// The tool effect keys by tool name, read once from the install.
#[derive(Resource, Default)]
pub struct ToolFxTable {
    tools: HashMap<String, Option<ToolEffects>>,
}

impl ToolFxTable {
    pub fn get(&mut self, index: &ResourceIndex, tool: &str) -> Option<ToolEffects> {
        *self
            .tools
            .entry(tool.to_string())
            .or_insert_with(|| read_tool_effects(index, tool))
    }

    #[cfg(test)]
    fn insert(&mut self, tool: &str, effects: ToolEffects) {
        self.tools.insert(tool.to_string(), Some(effects));
    }
}

/// A request to show a tool use. Spawned (with `StageOwned`) by the action
/// code; [`play_uses`] consumes it the same frame.
#[derive(Component, Clone, Debug)]
pub struct ToolFxUse {
    /// The retail tool (`spacetools~` name).
    pub tool: String,
    /// The world the tool is used on (aim point on its surface).
    pub world: Option<u32>,
    /// Start of the muzzle / beam; the ship when `None`.
    pub from: Option<Vec3>,
    /// Aim point; resolved on the world when `None`.
    pub to: Option<Vec3>,
    /// The shot lands on a ship (the combatant hit) instead of the ground.
    pub combatant: bool,
    /// The beam stays up this long; [`BEAM_SECONDS`] when `None`.
    pub beam_seconds: Option<f32>,
    /// One use of a held tool: effects already up are refreshed and re-aimed,
    /// new ones last [`HOLD_TIMEOUT`].
    pub hold: bool,
}

impl ToolFxUse {
    pub fn new(tool: &str, world: Option<u32>) -> Self {
        Self { tool: tool.to_string(), world, from: None, to: None, combatant: false, beam_seconds: None, hold: false }
    }

    /// A ship-to-ship shot (wingmen, colony turrets): both ends given, a
    /// short beam and the combatant hit.
    pub fn shot(tool: &str, from: Vec3, to: Vec3) -> Self {
        Self {
            tool: tool.to_string(),
            world: None,
            from: Some(from),
            to: Some(to),
            combatant: true,
            beam_seconds: Some(SHOT_BEAM_SECONDS),
            hold: false,
        }
    }
}

/// Spawn a request from an action system (the one-line hook of `space.rs`).
pub fn request(commands: &mut Commands, tool: &str, world: Option<u32>) {
    commands.spawn((StageOwned, ToolFxUse::new(tool, world)));
}

/// Spawn a request for one use of a held tool (`Y` held: the rays).
pub fn request_held(commands: &mut Commands, tool: &str, world: Option<u32>) {
    commands.spawn((StageOwned, ToolFxUse { hold: true, ..ToolFxUse::new(tool, world) }));
}

/// Spawn a ship-to-ship shot request.
pub fn request_shot(commands: &mut Commands, tool: &str, from: Vec3, to: Vec3) {
    commands.spawn((StageOwned, ToolFxUse::shot(tool, from, to)));
}

/// Where a tool lands.
#[derive(Clone, Copy, Debug)]
pub struct Aim {
    pub point: Vec3,
    /// The outward normal at the point.
    pub up: Vec3,
    pub kind: HitKind,
}

/// Scene units per retail unit: the planet view has its own, the solar system
/// view one world unit per `RETAIL_UNITS_PER_WORLD_UNIT`.
fn scene_scale(view: Option<&SpaceView>) -> f32 {
    view.and_then(|v| v.planet.as_ref()).map_or(1.0 / RETAIL_UNITS_PER_WORLD_UNIT, |p| p.unit())
}

/// Where the ray (the cursor) meets the sphere, if it does.
pub fn ray_hits_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<Vec3> {
    let to = center - origin;
    let along = to.dot(dir);
    if along <= 0.0 {
        return None;
    }
    let off2 = (origin + dir * along - center).length_squared();
    if off2 > radius * radius {
        return None;
    }
    Some(origin + dir * (along - (radius * radius - off2).sqrt()).max(0.0))
}

/// The aim point over the planet view's world: the cursor ray's hit on the
/// world when the cursor is over it, else the ground ahead of the ship.
pub fn planet_aim(
    view: &crate::space_view::PlanetView,
    ground: Option<&PlanetViewGround>,
    ray: Option<Ray3d>,
) -> Aim {
    let unit = view.unit();
    let hit = ray
        .and_then(|r| ray_hits_sphere(r.origin, *r.direction, view.center, view.radius))
        // A cursor far from the ship asks for another place: keep to the area
        // the ship's camera shows.
        .filter(|p| p.distance(view.ship_position()) < (view.altitude * 3.0 + 60.0) * unit);
    let wanted = hit.unwrap_or_else(|| {
        let ahead = view.heading * (view.altitude * AIM_AHEAD + 20.0) * unit;
        view.center + view.up * view.radius + ahead
    });
    let dir = (wanted - view.center).normalize_or(view.up);
    match ground {
        Some(g) => {
            let probe = view.center + dir * view.radius;
            let land = g.surface_under(probe, false);
            let sea = g.surface_under(probe, true);
            match (land, sea) {
                (Some((land, normal)), Some((sea, _))) => {
                    let wet = (sea - view.center).length() > (land - view.center).length() + 1.0e-4 * unit;
                    Aim { point: if wet { sea } else { land }, up: normal, kind: if wet { HitKind::Water } else { HitKind::Ground } }
                }
                (Some((land, normal)), None) => Aim { point: land, up: normal, kind: HitKind::Ground },
                _ => Aim { point: view.center + dir * view.radius, up: dir, kind: HitKind::Ground },
            }
        }
        None => Aim {
            point: view.center + dir * (view.radius + view.ground(dir) * unit),
            up: dir,
            kind: HitKind::Ground,
        },
    }
}

/// The aim point on a world seen from the solar system view: the surface
/// point nearest the ship.
pub fn system_aim(ship: Vec3, center: Vec3, radius: f32) -> Aim {
    let up = (ship - center).normalize_or(Vec3::Y);
    Aim { point: center + up * radius, up, kind: HitKind::Ground }
}

/// A beam following the ship.
#[derive(Component)]
pub(crate) struct FxBeam {
    to: Vec3,
    /// The beam starts at the ship (follows it) instead of a fixed point.
    follow_ship: bool,
    from: Vec3,
}

/// A running effect that stops softly after `life` seconds.
#[derive(Component)]
pub(crate) struct FxLife {
    age: f32,
    life: f32,
}

/// Which part of a held tool's effects an entity is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeldPart {
    Muzzle,
    Beam,
    /// The mark on the target and the hit effect: they follow the aim point.
    Aim,
}

/// An effect of a held tool; the next use of the same tool refreshes it.
#[derive(Component)]
pub(crate) struct FxHeld {
    tool: String,
    part: HeldPart,
}

/// A projectile flying from `from` to `to`; the hit effect plays on arrival.
#[derive(Component)]
pub(crate) struct FxFlight {
    from: Vec3,
    to: Vec3,
    age: f32,
    duration: f32,
    hit: Option<u32>,
    up: Vec3,
    scale: f32,
}

fn upright(up: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, up.normalize_or(Vec3::Y))
}

fn spawn_effect(
    commands: &mut Commands,
    library: &SwarmLibrary,
    id: u32,
    position: Vec3,
    rotation: Quat,
    scale: f32,
    life: f32,
) -> Option<Entity> {
    if library.loaded && library.find(id).is_none() {
        return None;
    }
    let mut effect = SwarmEffect::new(id);
    effect.position = position;
    effect.rotation = rotation;
    effect.scale = scale;
    Some(commands.spawn((effect, FxLife { age: 0.0, life }, StageOwned)).id())
}

/// `OPENSPORE_TOOL_FX_SKIP=muzzle,beam,...` leaves out parts of a tool's
/// effects (muzzle, generic, beam, projectile, target, hit) to see which one
/// draws what.
fn skipped(part: &str) -> bool {
    std::env::var("OPENSPORE_TOOL_FX_SKIP").is_ok_and(|v| v.split(',').any(|p| p.trim() == part))
}

/// Resolve and start the effects of every pending [`ToolFxUse`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn play_uses(
    mut commands: Commands,
    mut table: ResMut<ToolFxTable>,
    originals: Option<Res<crate::Originals>>,
    library: Res<SwarmLibrary>,
    view: Option<Res<SpaceView>>,
    controls: Option<Res<crate::space_controls::SpaceControls>>,
    ground: Option<Res<PlanetViewGround>>,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&Transform, &crate::game::Planet, &WorldId), Without<Player>>,
    uses: Query<(Entity, &ToolFxUse)>,
    mut held_fx: Query<(&mut FxLife, &mut SwarmEffect, &FxHeld, Option<&mut FxBeam>)>,
) {
    if uses.is_empty() {
        return;
    }
    let Some(originals) = originals else {
        for (entity, _) in &uses {
            commands.entity(entity).despawn();
        }
        return;
    };
    let ship = player.single().ok().map(|t| t.translation);
    let ray = controls.as_ref().and_then(|c| c.ray);
    let scale = scene_scale(view.as_deref());
    for (entity, request) in &uses {
        commands.entity(entity).despawn();
        let planet_view = view.as_deref().and_then(|v| v.planet.as_ref());
        // ADAPTED: V over a world seen from the solar system view is the retail
        // planet scan tool (`Planetscan`, a self effect), over its ground the
        // handheld `scan` beam.
        let tool = if request.tool == "scan" && planet_view.is_none() { "Planetscan" } else { request.tool.as_str() };
        let Some(effects) = table.get(&originals.resource_index, tool) else { continue };
        let from = request.from.or(ship).unwrap_or(Vec3::ZERO);
        // The aim point: given, in the planet view, or on the world.
        let aim = match (request.to, planet_view) {
            (Some(to), _) => {
                let up = planet_view.map_or(Vec3::Y, |p| (to - p.center).normalize_or(p.up));
                Aim { point: to, up, kind: HitKind::Ground }
            }
            (None, Some(p)) => planet_aim(p, ground.as_deref(), ray),
            (None, None) => {
                let world = request
                    .world
                    .and_then(|id| planets.iter().find(|(_, _, w)| w.0 == id))
                    .or_else(|| {
                        let at = ship?;
                        planets.iter().min_by(|a, b| {
                            a.0.translation.distance(at).total_cmp(&b.0.translation.distance(at))
                        })
                    });
                match (world, ship) {
                    (Some((t, planet, _)), Some(at)) => system_aim(at, t.translation, planet.radius),
                    // No world known (the capture demo): a point ahead of the ship.
                    _ => {
                        let forward = player.single().map_or(Vec3::NEG_Z, |t| t.rotation * Vec3::NEG_Z);
                        Aim { point: from + forward * 8.0, up: Vec3::Y, kind: HitKind::Ground }
                    }
                }
            }
        };
        let kind = if request.combatant { HitKind::Combatant } else { aim.kind };
        info!("Space tool fx: {tool} on {kind:?} ({} effects named)", effects.all().len());
        let upright_rotation = upright(aim.up);
        let ship_up = planet_view.map_or(Vec3::Y, |p| p.up);
        // A held tool whose effects are still up: keep them alive, re-aim the
        // beam and the hit, start nothing new (0x1059EF0 re-aims every use).
        if request.hold {
            let mut refreshed = false;
            for (mut life, mut fx, held, beam) in &mut held_fx {
                if held.tool != tool {
                    continue;
                }
                refreshed = true;
                life.age = 0.0;
                life.life = HOLD_TIMEOUT;
                match held.part {
                    HeldPart::Beam => {
                        if let Some(mut beam) = beam {
                            beam.to = aim.point;
                        }
                    }
                    HeldPart::Aim => {
                        fx.position = aim.point;
                        fx.rotation = upright_rotation;
                    }
                    HeldPart::Muzzle => {}
                }
            }
            if refreshed {
                continue;
            }
        }
        let beam_life = if request.hold { HOLD_TIMEOUT } else { request.beam_seconds.unwrap_or(BEAM_SECONDS) };
        let one_shot = if request.hold { HOLD_TIMEOUT } else { ONE_SHOT_SECONDS };
        let hit_life = if request.hold { HOLD_TIMEOUT } else { HIT_SECONDS };
        let hold = request.hold;
        let hold_part = |commands: &mut Commands, entity: Option<Entity>, part: HeldPart| {
            if let Some(entity) = entity.filter(|_| hold) {
                commands.entity(entity).insert(FxHeld { tool: tool.to_string(), part });
            }
        };

        if let Some(id) = effects.muzzle.filter(|_| !skipped("muzzle")) {
            let e = spawn_effect(&mut commands, &library, id, from, upright(ship_up), scale, one_shot);
            hold_part(&mut commands, e, HeldPart::Muzzle);
        }
        if let Some(id) = effects.generic.filter(|_| !skipped("generic")) {
            let e = spawn_effect(&mut commands, &library, id, from, upright(ship_up), scale, one_shot);
            hold_part(&mut commands, e, HeldPart::Muzzle);
        }
        // A named hit (or the by-name association of the tool).
        let named_hit = CODE_NAMED
            .iter()
            .find(|(named, _)| *named == tool)
            .map(|(_, name)| crate::world_objects::name_id(name));
        let hit = effects.hit(kind).or(named_hit).filter(|_| !skipped("hit"));
        let mut flight = None;
        if let Some(id) = effects.beam.filter(|_| !skipped("beam")) {
            if let Some(e) = spawn_effect(&mut commands, &library, id, from, Quat::IDENTITY, scale, beam_life) {
                commands.entity(e).insert(FxBeam { to: aim.point, follow_ship: request.from.is_none(), from });
                hold_part(&mut commands, Some(e), HeldPart::Beam);
            }
        }
        if let Some(id) = effects.projectile.filter(|_| !skipped("projectile")) {
            // Sky tools (a beam and a projectile) drop the projectile from above.
            let sky = effects.beam.is_some();
            let start = if sky { aim.point + aim.up * SKY_HEIGHT * scale } else { from };
            let speed = if effects.projectile_speed > 0.0 { effects.projectile_speed } else { 150.0 };
            let time_out = if effects.projectile_time_out > 0.0 { effects.projectile_time_out } else { MAX_FLIGHT };
            let duration = (start.distance(aim.point) / (speed * scale).max(1.0e-6))
                .clamp(MIN_FLIGHT, MAX_FLIGHT.min(time_out.max(MIN_FLIGHT)));
            if let Some(e) = spawn_effect(&mut commands, &library, id, start, upright_rotation, scale, duration + 0.8) {
                commands.entity(e).insert(FxFlight {
                    from: start,
                    to: aim.point,
                    age: 0.0,
                    duration,
                    hit,
                    up: aim.up,
                    scale,
                });
                flight = Some(duration);
            }
        }
        if let Some(id) = effects.target.filter(|_| !skipped("target")) {
            let e = spawn_effect(&mut commands, &library, id, aim.point, upright_rotation, scale, beam_life);
            hold_part(&mut commands, e, HeldPart::Aim);
        }
        // Beam and instant tools land at once; projectiles land on arrival.
        if flight.is_none() {
            if let Some(id) = hit {
                let e = spawn_effect(&mut commands, &library, id, aim.point, upright_rotation, scale, hit_life);
                hold_part(&mut commands, e, HeldPart::Aim);
            }
        }
    }
}

/// Move the beams with the ship, fly the projectiles, stop effects that
/// outlive their time.
#[allow(clippy::type_complexity)]
pub fn update_fx(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<SwarmLibrary>,
    player: Query<&Transform, With<Player>>,
    mut effects: Query<
        (Entity, &mut SwarmEffect, Option<&mut FxLife>, Option<&FxBeam>, Option<&mut FxFlight>),
        Or<(With<FxLife>, With<FxBeam>, With<FxFlight>)>,
    >,
) {
    let dt = time.delta_secs();
    let ship = player.single().ok().map(|t| t.translation);
    for (entity, mut effect, life, beam, flight) in &mut effects {
        if let Some(beam) = beam {
            let from = if beam.follow_ship { ship.unwrap_or(beam.from) } else { beam.from };
            effect.position = from;
            effect.path = vec![from, beam.to];
        }
        if let Some(mut flight) = flight {
            flight.age += dt;
            let t = (flight.age / flight.duration).clamp(0.0, 1.0);
            effect.position = flight.from.lerp(flight.to, t);
            if flight.age >= flight.duration {
                effect.stop = true;
                if let Some(id) = flight.hit {
                    spawn_effect(&mut commands, &library, id, flight.to, upright(flight.up), flight.scale, HIT_SECONDS);
                }
                commands.entity(entity).remove::<FxFlight>();
            }
        }
        if let Some(mut life) = life {
            life.age += dt;
            if life.age >= life.life {
                effect.stop = true;
                commands.entity(entity).remove::<FxLife>().remove::<FxBeam>();
            }
        }
    }
}

/// `--space-tool-fx NAME[,NAME..]` (captures): use the named retail tools one
/// after the other, one every 90 frames from frame 90, on the ground the
/// planet view shows (or ahead of the ship in the solar system view);
/// `--space-tool-hold` holds the first one instead.
fn tool_demo(mut commands: Commands, mut frame: Local<u32>, mut names: Local<Option<Vec<String>>>) {
    let names = names.get_or_insert_with(|| {
        let args: Vec<String> = std::env::args().collect();
        args.windows(2)
            .find(|w| w[0] == "--space-tool-fx")
            .map(|w| w[1].split(',').map(str::to_string).collect())
            .unwrap_or_default()
    });
    *frame += 1;
    // `--space-tool-hold`: the first named tool is held from frame 90, one use
    // every 6 frames (0.1 s, the `AutoFireRate` of the rays).
    if std::env::args().any(|a| a == "--space-tool-hold") {
        if !names.is_empty() && *frame >= 90 && (*frame - 90) % 6 == 0 {
            commands.spawn((StageOwned, ToolFxUse { hold: true, ..ToolFxUse::new(&names[0], None) }));
        }
        return;
    }
    if names.is_empty() || *frame < 90 || (*frame - 90) % 90 != 0 {
        return;
    }
    let n = ((*frame - 90) / 90) as usize % names.len();
    commands.spawn((StageOwned, ToolFxUse::new(&names[n], None)));
}

pub struct SpaceToolFxPlugin;

impl Plugin for SpaceToolFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolFxTable>().add_systems(
            Update,
            (tool_demo, play_uses, update_fx)
                .chain()
                .run_if(in_state(GameStage::Space)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install() -> Option<ResourceIndex> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then_some(index)
    }

    #[test]
    fn the_hit_effect_follows_what_the_shot_lands_on() {
        let fx = ToolEffects { hit_ground: Some(1), hit_water: Some(2), hit_combatant: Some(3), ..Default::default() };
        assert_eq!(fx.hit(HitKind::Ground), Some(1));
        assert_eq!(fx.hit(HitKind::Water), Some(2));
        assert_eq!(fx.hit(HitKind::Combatant), Some(3));
        // Missing kinds fall back to the ground hit; the player hit to the combatant hit.
        let ground_only = ToolEffects { hit_ground: Some(7), ..Default::default() };
        assert_eq!(ground_only.hit(HitKind::Water), Some(7));
        assert_eq!(ground_only.hit(HitKind::Player), Some(7));
        let none = ToolEffects::default();
        assert!(none.is_empty());
        assert_eq!(none.hit(HitKind::Ground), None);
    }

    #[test]
    fn every_effect_is_listed_once() {
        let fx = ToolEffects { muzzle: Some(5), projectile: Some(6), hit_ground: Some(7), hit_water: Some(7), ..Default::default() };
        assert_eq!(fx.all(), vec![5, 6, 7]);
    }

    #[test]
    fn planetary_tools_map_to_installed_tool_names() {
        for tool in PlanetaryTool::ALL {
            assert!(!planetary_tool_id(tool).is_empty());
        }
        assert_eq!(planetary_tool_id(PlanetaryTool::Warm), "heatray");
        assert_eq!(planetary_tool_id(PlanetaryTool::StaffOfLife), "genesisdevice");
        let wired = wired_tools();
        assert!(wired.contains(&"abduct") && wired.contains(&"scan") && wired.contains(&"planetbusterbomb"));
        let mut sorted = wired.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), wired.len());
    }

    #[test]
    fn the_cursor_ray_meets_the_sphere_in_front_only() {
        let hit = ray_hits_sphere(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z, Vec3::ZERO, 2.0).unwrap();
        assert!((hit - Vec3::new(0.0, 0.0, 2.0)).length() < 1.0e-5);
        assert!(ray_hits_sphere(Vec3::new(5.0, 0.0, 10.0), Vec3::NEG_Z, Vec3::ZERO, 2.0).is_none());
        // Pointing away from the world.
        assert!(ray_hits_sphere(Vec3::new(0.0, 0.0, 10.0), Vec3::Z, Vec3::ZERO, 2.0).is_none());
    }

    #[test]
    fn the_solar_aim_is_the_surface_point_nearest_the_ship() {
        let aim = system_aim(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, 3.0);
        assert!((aim.point - Vec3::new(0.0, 3.0, 0.0)).length() < 1.0e-5);
        assert_eq!(aim.up, Vec3::Y);
        assert_eq!(aim.kind, HitKind::Ground);
    }

    #[test]
    fn uses_are_consumed_even_without_the_install() {
        // No `Originals`: the request is dropped, nothing panics.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SwarmLibrary>()
            .init_resource::<ToolFxTable>()
            .add_systems(Update, (play_uses, update_fx).chain());
        app.world_mut().spawn(ToolFxUse::new("abduct", Some(0)));
        app.update();
        assert!(app.world_mut().query::<&ToolFxUse>().iter(app.world()).next().is_none());
    }

    #[test]
    fn a_table_entry_is_reused_by_the_systems() {
        let mut table = ToolFxTable::default();
        table.insert("t", ToolEffects { muzzle: Some(1), ..Default::default() });
        assert_eq!(table.tools.get("t").copied().flatten().and_then(|e| e.muzzle), Some(1));
    }

    /// Every wired tool's effects exist in the installed effect directories
    /// (the keys are read from the tool records) and the count of effects the
    /// tools play is reported (`--nocapture`).
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_tools_name_effects_that_exist() {
        let Some(index) = install() else { return };
        let library = SwarmLibrary::load(&index);
        let mut played = std::collections::BTreeSet::new();
        let mut missing = vec![];
        for tool in wired_tools() {
            let fx = read_tool_effects(&index, tool).unwrap_or_else(|| panic!("no tool record {tool}"));
            for id in fx.all() {
                played.insert(id);
                if library.find(id).is_none() {
                    missing.push(format!("{tool}: {id:08x}"));
                }
            }
        }
        for (_, name) in CODE_NAMED {
            let id = crate::world_objects::name_id(name);
            played.insert(id);
            if library.find(id).is_none() {
                missing.push(format!("{name}: {id:08x}"));
            }
        }
        println!(
            "wired tools: {}, distinct effects named: {}, installed: {}",
            wired_tools().len(),
            played.len(),
            played.len() - missing.len()
        );
        // Two names the tool records give have no effect in the install (the
        // registry knows `SG_ufo_abduct_Muzzle` and `SG_ufo_cash_infusion_hit`, the
        // effect directories do not export them): the retail game plays nothing
        // there either, and `play_uses` skips ids the library lacks.
        let not_installed = [crate::world_objects::name_id("SG_ufo_abduct_Muzzle"), crate::world_objects::name_id("SG_ufo_cash_infusion_hit")];
        missing.retain(|m| !not_installed.iter().any(|id| m.ends_with(&format!("{id:08x}"))));
        assert!(missing.is_empty(), "named effects missing from the effect directories: {missing:?}");
        // The laser keys name what space_weapon_fx plays by name.
        let laser = read_tool_effects(&index, "laserlevel1").unwrap();
        assert_eq!(laser.beam, Some(crate::world_objects::name_id("SG_laser_lvl_1_beam")));
        assert_eq!(laser.hit_combatant, Some(crate::world_objects::name_id("SG_laser_lvl_1_beam_impact")));
        // The abduct tool: muzzle, beam, ground and water hits.
        let abduct = read_tool_effects(&index, "abduct").unwrap();
        assert!(abduct.muzzle.is_some() && abduct.beam.is_some() && abduct.hit_ground.is_some() && abduct.hit_water.is_some());
        // The pulse weapon: muzzle, projectile and explosion.
        let pulse = read_tool_effects(&index, "pulselevel1").unwrap();
        assert!(pulse.muzzle.is_some() && pulse.projectile.is_some() && pulse.hit_ground.is_some());
    }

    /// The use table of `space_rules::TOOL_USE` and the ray velocities
    /// (`RAY_VELOCITY`) equal the installed tool records: `spaceToolEnergyCost`
    /// 0x057B4EEC, `spaceToolAutoFireRate` 0x51191D96, `spaceToolRechargeRate`
    /// 0x51191D8F, `toolRelationshipDelta` 0x039A4CAF and the hit velocities
    /// (ground / water / combatant / air agree for every ray).
    #[test]
    #[ignore = "requires the installed game"]
    fn the_use_table_matches_the_installed_tool_records() {
        let Some(index) = install() else { return };
        let props_of = |name: &str| {
            let key = ResourceKey {
                group_id: TOOL_GROUP,
                instance_id: crate::world_objects::name_id(name),
                type_id: crate::asset_catalog::PROP,
            };
            index.properties(key).unwrap_or_else(|_| panic!("no tool record {name}"))
        };
        let number = |props: &PropertyList, id: u32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(0.0);
        for row in crate::space_rules::TOOL_USE {
            let props = props_of(row.id);
            assert_eq!(number(&props, 0x057B_4EEC), row.energy, "{} energy", row.id);
            assert_eq!(number(&props, 0x5119_1D96), row.autofire, "{} auto-fire", row.id);
            assert_eq!(number(&props, 0x5119_1D8F), row.recharge, "{} recharge", row.id);
            assert!((number(&props, 0x039A_4CAF) - row.relationship).abs() < 1e-6, "{} relationship", row.id);
        }
        for (name, atmosphere, temperature) in crate::space_rules::RAY_VELOCITY {
            let props = props_of(name);
            for id in [0x030F_D84D, 0x030F_D861, 0x030F_D86B, 0x030F_E481] {
                assert!((number(&props, id) - atmosphere).abs() < 1e-6, "{name} atmosphere velocity {id:08x}");
            }
            for id in [0x030F_D85D, 0x030F_D868, 0x030F_D872, 0x030F_E488] {
                assert!((number(&props, id) - temperature).abs() < 1e-6, "{name} temperature velocity {id:08x}");
            }
        }
    }

    /// Census: the share of the Space effects (`SG_*`, `ufo_*`, `space_*`
    /// names of the registry) that a tool record names, over all 363 tools.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_tool_census() {
        let Some(index) = install() else { return };
        let mut all = std::collections::BTreeSet::new();
        let mut tools = 0;
        for (key, _) in index.records.iter() {
            if key.group_id == TOOL_GROUP && key.type_id == crate::asset_catalog::PROP {
                if let Ok(props) = index.properties(*key) {
                    let fx = ToolEffects::from_props(&props);
                    tools += 1;
                    all.extend(fx.all());
                }
            }
        }
        println!("tool records: {tools}, distinct effects named by tool keys: {}", all.len());
        assert!(tools > 300);
        assert!(all.len() > 400);
    }
}
