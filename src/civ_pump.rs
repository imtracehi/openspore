//! Civilization: building a spice derrick the retail way (helper `civ-harvest`,
//! `notes\civ-harvest-notes.md`).
//!
//! VERIFIED in the code image (ModAPI march2017 addresses, `swarm-volume\target\re\dis.py`):
//! * Command 8 (claim an unclaimed node, any vehicle) starts the vehicle behaviour 0x200 (command
//!   table 0xC9F950). The behaviour drives to the node; once it has arrived and the node is free
//!   (`cCommodityNode::mMineState` 0, 0xDCB2E5) it calls `cCommodityNode::StartConstruction`
//!   0xBFE590: `mMineStateTimeout` = 5000 ms, `mConstructingPoliticalID` = the vehicle's nation,
//!   `mConstructingVehicle` = the vehicle, `mMineState` = 1; the node's display state (the object at
//!   +0x288, +4) becomes 1, which plays `civ_spice_pump_unpack` (display strategy 0xE8FF80).
//! * The node update 0xBFF8E0, state 1: on the first update a `cResourceProjectile` is created
//!   (0xBFE8E0), placed at the vehicle's position and launched (`LaunchProjectile` 0xCBE7C0) at the
//!   node (a point one unit above it) with the speed parameter 20; `+0x1E7` records the launch.
//!   When the node's timer passes the timeout, the owner becomes the constructing political id
//!   (0xBFF410) and the node moves to state 2: the derrick stands. The capsule is only the
//!   picture: the timer, not the landing, decides.
//! * 0xCBE7C0 builds the flight as a path of 32 points: the straight line from the vehicle to the
//!   target one unit above the node in 33 steps, each point lifted along the planet's up by a
//!   height that grows by 0.05 x (16 - i) for i < 16 (0.8, 1.55 ... 6.8) and then falls by
//!   0.05 x (i - 16) (6.8 ... 0.8), and the projectile follows it at the speed parameter 20 units a
//!   second (SetPath 0xC42310). The display strategy of the projectile (0xCC3500) plays
//!   `civ_spice_pump_projectile` (id 0x4AAD8BAF; ribbon, game model, sound) with the planet's
//!   spice colour as effect parameter 5; the landing plays `civ_spice_pump_landing`.
//! * Income does not depend on the pump or on harvester vehicles: the money update sums the owned
//!   nodes' income directly (0xBF7164 -> 0xBFDF90 -> 0xCEF130).
//!
//! ADAPTED: the capsule is a plain entity moving along that path (retail moves a physics-less
//! projectile along a path object); the landing effect is played where the path ends; the
//! effect colour uses OpenSpore's own RGB for the planet's spice colour (`SpiceColor::for_world(0)`).

use bevy::prelude::*;

use crate::civ::GEYSER_COUNT;
use crate::game::RunProgress;
use crate::stages::GameStage;
use crate::swarm_fx::{SwarmEffect, SwarmPlay, SwarmStop};
use crate::world_objects::name_id;

/// `mMineStateTimeout` set by `StartConstruction` (0xBFE59D: 0x1388 ms).
pub const BUILD_SECONDS: f32 = 5.0;
/// The speed parameter `LaunchProjectile` gets from the node update (0xBFF9A3: 20.0).
pub const CAPSULE_SPEED: f32 = 20.0;
/// Points of the flight path (0xCBE9B0 loop, ebp 0 .. 31).
pub const CAPSULE_POINTS: usize = 32;
/// The path's height step (0xCBE9B9: 0.05 per unit of the loop counter).
const LIFT_STEP: f32 = 0.05;
/// The path runs 33 steps from the vehicle to the target (0x1476334 = 1 / 33).
const PATH_STEPS: f32 = 33.0;

pub const PROJECTILE: &str = "civ_spice_pump_projectile";
pub const LANDING: &str = "civ_spice_pump_landing";
pub const UNPACK: &str = "civ_spice_pump_unpack";

/// Tags of the pump effects (the low bits: the geyser). 0xC1F0..0xC1F9 belong to the other Civ
/// effect modules.
pub const TAG_PUMP: u64 = 0xC1FA_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Seconds the capsule's entity stays after it lands, so the effect's ribbon and sound finish.
const LINGER: f32 = 2.0;

/// The 32 heights of the path (0xCBE9B0 .. 0xCBEAF8): a rise of 0.05 x (16 - i), then a fall of
/// 0.05 x (i - 16).
pub fn capsule_heights() -> [f32; CAPSULE_POINTS] {
    let mut heights = [0.0; CAPSULE_POINTS];
    let mut h = 0.0;
    for (i, slot) in heights.iter_mut().enumerate() {
        if i < 16 {
            h += (16 - i) as f32 * LIFT_STEP;
        } else {
            h -= (i - 16) as f32 * LIFT_STEP;
        }
        *slot = h;
    }
    heights
}

/// The flight path from `from` (the vehicle) to `to` (the node): 0xCBE7C0. `up` is the planet's up
/// at a point (retail: the point's direction from the planet's centre).
pub fn capsule_path(from: Vec3, to: Vec3, up: impl Fn(Vec3) -> Vec3) -> Vec<Vec3> {
    // The target is a point one unit above the node (0xCBE88B: T + T / |T|).
    let target = to + up(to);
    let step = (target - from) / PATH_STEPS;
    let heights = capsule_heights();
    (0..CAPSULE_POINTS)
        .map(|i| {
            let base = from + step * i as f32;
            base + up(base) * heights[i]
        })
        .collect()
}

/// The length of a polyline.
pub fn path_length(path: &[Vec3]) -> f32 {
    path.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The point `distance` along the polyline (the end once past it).
pub fn point_on_path(path: &[Vec3], distance: f32) -> Vec3 {
    let mut left = distance.max(0.0);
    for w in path.windows(2) {
        let len = w[0].distance(w[1]);
        if left <= len {
            return if len > 0.0 { w[0].lerp(w[1], left / len) } else { w[0] };
        }
        left -= len;
    }
    path.last().copied().unwrap_or(Vec3::ZERO)
}

/// A pump capsule in flight (`cResourceProjectile`).
#[derive(Component, Debug, Clone)]
pub struct PumpCapsule {
    pub geyser: usize,
    pub path: Vec<Vec3>,
    pub length: f32,
    pub travelled: f32,
    /// Seconds since it landed (it lingers for the effect to finish).
    pub landed: Option<f32>,
}

/// The planet's spice colour as the effects take it (retail effect parameter 5).
fn spice_tint() -> Vec3 {
    Vec3::from_array(crate::space::SpiceColor::for_world(0).rgb())
}

/// The node update's state 1 (0xBFF944 .. 0xBFF9E8): the first update of a pump under
/// construction fires its capsule; the node's display plays the unpack effect.
fn launch_capsules(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut plays: MessageWriter<SwarmPlay>,
) {
    if run.civ_campaign.geysers.is_empty() {
        return;
    }
    let shape = run.civ_campaign.shape();
    for g in 0..run.civ_campaign.geysers.len().min(GEYSER_COUNT) {
        let Some(claim) = run.civ_campaign.geysers[g].claim.filter(|c| !c.launched) else {
            continue;
        };
        let node = run.civ_campaign.geyser_at(g);
        let path = capsule_path(Vec3::from_array(claim.from), node, |p| shape.up(p));
        let length = path_length(&path);
        let start = path[0];
        let capsule = commands
            .spawn((
                crate::stages::StageOwned,
                Name::new(format!("spice pump capsule {g}")),
                PumpCapsule { geyser: g, path, length, travelled: 0.0, landed: None },
                Transform::from_translation(start),
                Visibility::Inherited,
            ))
            .id();
        let mut play = SwarmPlay::at(name_id(PROJECTILE), Vec3::ZERO);
        play.attach_to = Some(capsule);
        play.tag = TAG_PUMP | g as u64;
        plays.write(play);
        // The display state 1 of the node (0xE8FF80): the pump unpacks.
        let mut unpack = SwarmPlay::at(name_id(UNPACK), node);
        unpack.rotation = Quat::from_rotation_arc(Vec3::Y, shape.up(node));
        unpack.tag = TAG_PUMP | 0x100 | g as u64;
        plays.write(unpack);
        run.civ_campaign.geysers[g].claim.as_mut().expect("claim").launched = true;
    }
}

/// The projectile follows its path at `CAPSULE_SPEED`; where it ends it lands.
fn fly_capsules(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    mut capsules: Query<(Entity, &mut PumpCapsule, &mut Transform)>,
    mut plays: MessageWriter<SwarmPlay>,
    mut stops: MessageWriter<SwarmStop>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let shape = run.civ_campaign.shape();
    for (entity, mut capsule, mut transform) in &mut capsules {
        if let Some(since) = capsule.landed.as_mut() {
            *since += dt;
            if *since >= LINGER {
                commands.entity(entity).despawn();
            }
            continue;
        }
        capsule.travelled += CAPSULE_SPEED * dt;
        transform.translation = point_on_path(&capsule.path, capsule.travelled);
        if capsule.travelled >= capsule.length {
            capsule.landed = Some(0.0);
            let at = capsule.path.last().copied().unwrap_or(transform.translation);
            let mut landing = SwarmPlay::at(name_id(LANDING), at);
            landing.rotation = Quat::from_rotation_arc(Vec3::Y, shape.up(at));
            landing.tag = TAG_PUMP | 0x200 | capsule.geyser as u64;
            plays.write(landing);
            stops.write(SwarmStop {
                effect: name_id(PROJECTILE),
                attach_to: Some(entity),
                tag: TAG_PUMP | capsule.geyser as u64,
                hard: false,
            });
        }
    }
}

/// The pump effects carry the planet's spice colour (effect parameter 5).
fn tint_effects(mut effects: Query<&mut SwarmEffect>) {
    let tint = spice_tint();
    for mut effect in &mut effects {
        if effect.tag & TAG_MASK == TAG_PUMP && effect.tint != tint {
            effect.tint = tint;
        }
    }
}

/// The pump capsules of `civ_vfx` (registered with it; the entities and effects are `StageOwned`).
pub struct CivPumpPlugin;

impl Plugin for CivPumpPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (launch_capsules, fly_capsules, tint_effects)
                .chain()
                .run_if(in_state(GameStage::Civilization))
                .run_if(resource_exists::<Messages<SwarmPlay>>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::{GeyserState, HOME_INDEX};

    fn flat_up(_: Vec3) -> Vec3 {
        Vec3::Y
    }

    #[test]
    fn the_capsule_rises_to_6_8_and_falls_back_to_0_8() {
        let h = capsule_heights();
        assert!((h[0] - 0.8).abs() < 1e-5, "{h:?}");
        assert!((h[1] - 1.55).abs() < 1e-5);
        assert!((h[15] - 6.8).abs() < 1e-4, "{}", h[15]);
        assert!((h[16] - 6.8).abs() < 1e-4, "the fall starts with 0");
        assert!((h[17] - 6.75).abs() < 1e-4);
        assert!((h[31] - 0.8).abs() < 1e-4, "{}", h[31]);
        assert!(h.iter().all(|v| *v > 0.0));
    }

    #[test]
    fn the_path_runs_from_the_vehicle_to_a_point_just_short_of_the_node() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let node = Vec3::new(33.0, 0.0, 0.0);
        let path = capsule_path(from, node, flat_up);
        assert_eq!(path.len(), CAPSULE_POINTS);
        assert!((path[0] - Vec3::new(0.0, 0.8, 0.0)).length() < 1e-4);
        // 31 of 33 steps toward (node + 1 up), lifted 0.8.
        let last = path[CAPSULE_POINTS - 1];
        assert!((last.x - 31.0).abs() < 1e-3, "{last}");
        assert!((last.y - (31.0 / 33.0 + 0.8)).abs() < 1e-3, "{last}");
        // The peak is over the middle.
        let top = path.iter().map(|p| p.y).fold(0.0, f32::max);
        assert!(top > 6.5, "{top}");
    }

    #[test]
    fn the_capsule_flies_the_path_at_twenty_units_a_second() {
        let path = capsule_path(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), flat_up);
        let len = path_length(&path);
        assert!(len > 10.0 && len < 30.0, "{len}");
        assert_eq!(point_on_path(&path, 0.0), path[0]);
        let end = point_on_path(&path, len + 5.0);
        assert!((end - path[CAPSULE_POINTS - 1]).length() < 1e-5);
        let mid = point_on_path(&path, len * 0.5);
        assert!(mid.y > 3.0, "{mid}");
        // Flight time at 20 u/s.
        assert!(len / CAPSULE_SPEED < 1.5);
    }

    #[test]
    fn a_claim_stands_after_five_seconds_for_the_nation_that_started_it() {
        let mut g = GeyserState::default();
        assert!(g.free());
        assert!(g.start_claim(3, Vec3::new(1.0, 2.0, 3.0)));
        assert!(!g.free(), "a node in construction is out of state 0");
        assert!(!g.start_claim(2, Vec3::ZERO), "only a free node starts");
        assert_eq!(g.owner, None);
        assert_eq!(g.tick_claim(4.9), None);
        assert_eq!(g.claim.map(|c| c.nation), Some(3));
        assert_eq!(g.tick_claim(0.2), Some(3));
        assert_eq!(g.owner, Some(3));
        assert!(g.claim.is_none());
        assert!(!g.start_claim(4, Vec3::ZERO), "an owned node cannot be claimed");
        assert!(g.hp > 0.0, "the derrick stands with its health");
    }

    #[test]
    fn the_campaign_tick_finishes_claims_and_tells_the_player() {
        let mut campaign = crate::civ::CivCampaign::default();
        campaign.ensure_cities();
        campaign.geysers[0].start_claim(HOME_INDEX, Vec3::ZERO);
        campaign.tick(2.0);
        assert_eq!(campaign.geysers[0].owner, None);
        campaign.tick(3.1);
        assert_eq!(campaign.geysers[0].owner, Some(HOME_INDEX));
        assert!(campaign.message.contains("Spice derrick built"), "{}", campaign.message);
    }

    #[test]
    fn tags_do_not_collide_with_the_other_civ_effects() {
        assert_eq!(TAG_PUMP & 0x0000_FFFF_FFFF_FFFF, 0);
        assert!((TAG_PUMP >> 48) > 0xC1F9);
    }

    // ---- the Civ scene (headless): the claim fires the capsule, the effects play ----

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        app
    }

    #[test]
    fn a_started_claim_launches_one_capsule_with_the_retail_effects() {
        if !crate::civ_vfx::on() {
            return;
        }
        let mut app = civ_app();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.civ_campaign.ensure_cities();
            let node = run.civ_campaign.geyser_at(0);
            // The opening may have given the nearest geyser to a rival: take it free.
            run.civ_campaign.geysers[0] = GeyserState::default();
            assert!(run.civ_campaign.geysers[0].start_claim(HOME_INDEX, node + Vec3::new(8.0, 0.0, 0.0)));
        }
        for _ in 0..3 {
            app.update();
        }
        let capsules = app.world_mut().query::<&PumpCapsule>().iter(app.world()).count();
        assert_eq!(capsules, 1, "one capsule per claim");
        assert!(app.world().resource::<RunProgress>().civ_campaign.geysers[0].claim.is_some_and(|c| c.launched));
        let names: Vec<u32> = app
            .world_mut()
            .query::<&SwarmEffect>()
            .iter(app.world())
            .filter(|e| e.tag & TAG_MASK == TAG_PUMP)
            .map(|e| e.effect)
            .collect();
        assert!(names.contains(&name_id(PROJECTILE)), "the projectile effect plays: {names:?}");
        assert!(names.contains(&name_id(UNPACK)), "the node unpacks: {names:?}");
        // A second frame does not launch another.
        for _ in 0..3 {
            app.update();
        }
        let capsules = app.world_mut().query::<&PumpCapsule>().iter(app.world()).count();
        assert_eq!(capsules, 1);
    }

    #[test]
    fn a_vehicle_that_reaches_a_free_geyser_starts_the_construction_and_goes_idle() {
        use crate::civ::{Approach, UnitOrder, UnitState};
        use crate::civ_rts::CivUnit;
        let mut app = civ_app();
        let node = {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.civ_campaign.ensure_cities();
            run.civ_campaign.geysers[0] = GeyserState::default();
            run.civ_campaign.geyser_at(0)
        };
        let state = UnitState { id: 901, nation: HOME_INDEX, kind: Approach::Military, ..Default::default() };
        let mut unit = CivUnit::from_state(&state);
        unit.order = UnitOrder::Geyser(0);
        let vehicle = app.world_mut().spawn((unit, Transform::from_translation(node))).id();
        for _ in 0..4 {
            app.update();
        }
        let geyser = app.world().resource::<RunProgress>().civ_campaign.geysers[0].clone();
        assert_eq!(geyser.owner, None, "the derrick is not there yet: {geyser:?}");
        let claim = geyser.claim.expect("the construction started");
        assert_eq!(claim.nation, HOME_INDEX);
        assert!(claim.left > 4.0 && claim.left <= BUILD_SECONDS, "{}", claim.left);
        assert_eq!(app.world().get::<CivUnit>(vehicle).unwrap().order, UnitOrder::Idle);
        // The pump stands when the timer runs out.
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        run.civ_campaign.tick(BUILD_SECONDS);
        assert_eq!(run.civ_campaign.geysers[0].owner, Some(HOME_INDEX));
        assert!(run.civ_campaign.geysers[0].claim.is_none());
    }

    #[test]
    #[ignore = "requires installed Spore resource packages"]
    fn the_retail_pump_effects_are_installed() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        for name in [PROJECTILE, LANDING, UNPACK] {
            assert!(lib.find(name_id(name)).is_some(), "{name} is not installed");
        }
        assert_eq!(name_id(PROJECTILE), 0x4AAD_8BAF, "the id the projectile display strategy 0xCC3500 uses");
    }
}
