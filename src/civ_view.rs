//! Civilization visuals: framed quick-capture scenarios for the visuals audit
//! (`notes\civ-visuals-notes.md`). `--quick-capture <dir> civilization
//! --scenario <name>` calls [`capture_scenario`] first; the older scenarios
//! stay in `civ_rts::quick_capture_scenario`.
//!
//! * `geyser`  - camera on spice geyser 0 (`--scenario geyser-claimed` gives it
//!   to the player first, so the derrick shows),
//! * `rival`   - camera on rival city 1,
//! * `city`    - camera on the capital,
//! * `fleet`   - the player's fleet ordered toward rival city 1 (at war), so
//!   the trails and selection rings show.
//!
//! civ-visuals2 added: `trade` (economic vehicles shuttling to rival city 2 over an open
//! route), `propose` (economic vehicles proposing a route at city 2, which is at war and
//! refuses; `open` is the same at peace: the route opens on arrival), `rivalsel`
//! (rival vehicles near the capital, one clicked: owner coloured circle), `arrows` (the fleet
//! sent ahead with the destination arrows at its goal; use `--frames 22`), `trails` (a land
//! and an air fleet on the move).
//!
//! All of them leave the camera zoom to `--wheel N@FRAME`.
//!
//! The module also holds the buildings' zoom-out display scale.
//! VERIFIED (`notes\civ-camera-notes.md` section 3, the display strategy
//! 0xE88DF0 that also scales the city hall): above `PlumpDistance` 250 the ring
//! buildings (nouns 0xECADE42 / 0xFF10521, `BuildingType1/2ZoomOutScale`) are
//! drawn 1.25 times larger, the live scale easing toward its target with the
//! vehicle's rate (`civ_controls::zoom_out_step`). The vehicles and the hall
//! already do the same in `civ_controls`; the building constant was loaded but
//! never applied.

use bevy::prelude::*;

use crate::civ::{Approach, CivCommander, UnitOrder, HOME_INDEX};
use crate::civ_controls::{tuning, zoom_out_target, CivCamera, ZoomOutState};
use crate::city_presentation::{CityPlotVisual, NativeCityDistrictSurface, CITY_ROOT_CLEARANCE};
use crate::civ_rts::CivUnit;
use crate::game::RunProgress;
use crate::stages::GameStage;

/// Move the camera focus (the invisible commander) onto `at`.
fn focus_on(world: &mut World, at: Vec3) {
    let on_sphere = world.resource::<RunProgress>().civ_campaign.shape().on_sphere();
    let mut query = world.query_filtered::<&mut Transform, With<CivCommander>>();
    if let Ok(mut t) = query.single_mut(world) {
        t.translation = if on_sphere { at } else { Vec3::new(at.x, t.translation.y, at.z) };
    }
}

/// Stage `name` if it is one of ours; `false` leaves it to the older
/// scenarios.
pub fn capture_scenario(world: &mut World, name: &str) -> bool {
    match name {
        "geyser" | "geyser-claimed" => {
            let at = {
                let mut run = world.resource_mut::<RunProgress>();
                if name == "geyser-claimed" {
                    run.civ_campaign.geysers[0] = crate::civ::GeyserState::derrick(HOME_INDEX, 100.0);
                }
                run.civ_campaign.geyser_at(0)
            };
            focus_on(world, at);
        }
        "rival" => {
            let at = world.resource::<RunProgress>().civ_campaign.cities[1].center();
            focus_on(world, at);
        }
        "city" => {
            let at = world.resource::<RunProgress>().civ_campaign.cities[HOME_INDEX].center();
            focus_on(world, at);
        }
        "fleet" => {
            let at = {
                let mut run = world.resource_mut::<RunProgress>();
                run.civ_campaign.nations[1].war = true;
                run.civ_campaign.vehicle_pad(HOME_INDEX)
            };
            focus_on(world, at);
            let mut units = world.query::<&mut CivUnit>();
            for mut unit in units.iter_mut(world) {
                if unit.nation == HOME_INDEX {
                    unit.order = crate::civ::UnitOrder::City(1);
                }
            }
        }
        "trade" | "propose" | "open" => {
            // civ-visuals2: economic vehicles of the player at rival city 2, on an open route
            // (`trade`: 45 units from it, outbound, balloons) or proposing one (`propose`: at
            // the city with no spice to pay for it).
            let at = {
                let mut run = world.resource_mut::<RunProgress>();
                let shape = run.civ_campaign.shape();
                let capital = run.civ_campaign.cities[HOME_INDEX].center();
                let partner = run.civ_campaign.cities[2].center();
                run.civ_campaign.hidden.retain(|h| *h != 2);
                run.spice = if name == "trade" { 400.0 } else { 100.0 };
                run.civ_campaign.cities[2].trade = name == "trade";
                // `propose`: a nation at war refuses, so the vehicles keep asking (a route
                // costs nothing, `civ::TRADE_COST`, and opens at once otherwise).
                run.civ_campaign.nations[2].war = name == "propose";
                if name == "propose" {
                    // A hostile relationship keeps the war (`civ.rs`: a peaceful one ends it).
                    run.civ_campaign.nations[2].relation.apply(crate::civ_rules::RelationKind::DeclaredWar);
                }
                let near = if name == "trade" { 45.0 } else { 4.0 };
                let at = shape.toward(partner, capital - partner, near);
                let mut extra = Vec::new();
                for k in 0..2 {
                    let spot = shape.toward(at, shape.bearing(at, std::f32::consts::FRAC_PI_2), 7.0 * k as f32);
                    let mut unit = run.civ_campaign.add_unit(HOME_INDEX, Approach::Economic, false, spot);
                    unit.order = UnitOrder::City(2);
                    extra.push(unit);
                }
                world_spawns(world, extra);
                at
            };
            crate::civ_rts::teleport_fleet(world, HOME_INDEX, at);
            focus_on(world, at);
        }
        "rivalsel" => {
            // civ-visuals2: rival vehicles beside the capital; the first of nation 1 is the
            // clicked one (its circle takes the nation colour), your fleet stays selected.
            let at = {
                let mut run = world.resource_mut::<RunProgress>();
                let shape = run.civ_campaign.shape();
                let pad = run.civ_campaign.vehicle_pad(HOME_INDEX);
                let at = shape.toward(pad, shape.bearing(pad, std::f32::consts::FRAC_PI_2), 28.0);
                let mut extra = Vec::new();
                for (k, nation) in [1usize, 2].into_iter().enumerate() {
                    let spot = shape.toward(at, shape.bearing(at, 0.0), 9.0 * k as f32);
                    extra.push(run.civ_campaign.add_unit(nation, Approach::Military, false, spot));
                }
                extra.push(run.civ_campaign.add_unit(1, Approach::Economic, false, shape.toward(at, shape.bearing(at, 1.0), 14.0)));
                world_spawns(world, extra);
                at
            };
            world.insert_resource(CaptureInspect(1));
            focus_on(world, at);
        }
        "arrows" | "trails" => {
            // civ-visuals2: the fleet sent 45 units ahead. `arrows` plays the retail destination
            // arrows at its goal; `trails` adds two aircraft and shows the vehicle trails.
            let (pad, shape) = {
                let run = world.resource::<RunProgress>();
                (run.civ_campaign.vehicle_pad(HOME_INDEX), run.civ_campaign.shape())
            };
            let goal = shape.toward(pad, shape.bearing(pad, std::f32::consts::FRAC_PI_2), 45.0);
            if name == "trails" {
                let extra: Vec<_> = {
                    let mut run = world.resource_mut::<RunProgress>();
                    (0..2)
                        .map(|k| {
                            let spot = shape.toward(pad, shape.bearing(pad, 0.0), 8.0 * (k + 1) as f32);
                            run.civ_campaign.add_unit(HOME_INDEX, Approach::Military, true, spot)
                        })
                        .collect()
                };
                world_spawns(world, extra);
            }
            let mut units = world.query::<&mut CivUnit>();
            for mut unit in units.iter_mut(world) {
                if unit.nation == HOME_INDEX {
                    unit.order = UnitOrder::Goto(goal.to_array());
                }
            }
            if name == "arrows" {
                let mut commands = world.commands();
                crate::civ_fx::destination_effect(&mut commands, goal, shape);
                world.flush();
            }
            focus_on(world, shape.toward(pad, shape.bearing(pad, std::f32::consts::FRAC_PI_2), 22.0));
        }
        _ => return false,
    }
    true
}

/// Queue vehicles for the Civ stage to spawn.
fn world_spawns(world: &mut World, units: Vec<crate::civ::UnitState>) {
    world.resource_mut::<crate::civ_rts::UnitSpawns>().0.extend(units);
}

/// Capture helper: the first live vehicle of this nation is "clicked" (`Inspected`) once it exists.
#[derive(Resource)]
struct CaptureInspect(usize);

fn capture_inspect(
    mut commands: Commands,
    pending: Res<CaptureInspect>,
    units: Query<(Entity, &CivUnit)>,
) {
    if let Some((entity, _)) = units.iter().find(|(_, u)| u.nation == pending.0) {
        commands.entity(entity).insert(crate::civ_fx::Inspected { until: f32::MAX });
        commands.remove_resource::<CaptureInspect>();
    }
}

/// The ring buildings drawn larger while the camera is zoomed far out. A
/// native building is a set of surfaces sharing one fitted transform; they
/// scale about the plot's floor point (city-local, as `sync_plots` placed the
/// building). The player's own design hangs under one root placed at that
/// point, so it scales in place.
fn building_zoom_out(
    time: Res<Time>,
    camera: Res<CivCamera>,
    mut commands: Commands,
    mut plots: Query<(
        Entity,
        &CityPlotVisual,
        Has<NativeCityDistrictSurface>,
        &mut Transform,
        Option<&mut ZoomOutState>,
    )>,
) {
    let target = zoom_out_target(camera.distance, tuning::BUILDING_ZOOM_OUT_SCALE);
    let dt = time.delta_secs();
    for (entity, plot, native, mut transform, state) in &mut plots {
        let Some(mut state) = state else {
            commands.entity(entity).insert(ZoomOutState::new(&transform));
            continue;
        };
        let pivot = native.then(|| Vec3::new(plot.site.x, -CITY_ROOT_CLEARANCE, plot.site.y));
        state.step(&mut transform, target, dt, pivot);
    }
}

pub struct CivViewPlugin;

impl Plugin for CivViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            building_zoom_out
                .after(crate::city_presentation::sync_plots)
                .run_if(in_state(GameStage::Civilization)),
        )
        .add_systems(
            Update,
            capture_inspect.run_if(resource_exists::<CaptureInspect>).run_if(in_state(GameStage::Civilization)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buildings_grow_by_the_retail_scale_about_their_plot_floor() {
        assert_eq!(zoom_out_target(300.0, tuning::BUILDING_ZOOM_OUT_SCALE), 1.25);
        assert_eq!(zoom_out_target(200.0, tuning::BUILDING_ZOOM_OUT_SCALE), 1.0);
        let site = crate::civ::CityState::new(0).plot_site(2);
        let pivot = Vec3::new(site.x, -CITY_ROOT_CLEARANCE, site.y);
        // A fitted native surface: the model's base point maps to the plot floor.
        let base = Transform::from_translation(pivot + Vec3::new(0.3, 0.0, -0.2)).with_scale(Vec3::splat(0.4));
        let floor_point = base.compute_affine().inverse().transform_point3(pivot);
        let mut state = ZoomOutState::new(&base);
        let mut live = base;
        // One second is long past the approach (0.008 per millisecond): the
        // factor arrives at the target.
        state.step(&mut live, 1.25, 1.0, Some(pivot));
        assert!((live.scale.x - 0.5).abs() < 1e-5, "{:?}", live.scale);
        assert!(live.transform_point(floor_point).distance(pivot) < 1e-4, "the base stays on the plot floor");
        // Zooming back in restores the fitted transform.
        state.step(&mut live, 1.0, 1.0, Some(pivot));
        assert!(live.translation.distance(base.translation) < 1e-5 && (live.scale.x - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_system_scales_plot_buildings_only_while_the_camera_is_far() {
        use crate::civ::Building;
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
        app.insert_resource(CivCamera::default());
        app.add_systems(Update, building_zoom_out);
        let site = crate::civ::CityState::new(0).plot_site(1);
        let at = Vec3::new(site.x, -CITY_ROOT_CLEARANCE, site.y);
        let base = Transform::from_translation(at + Vec3::new(0.2, 0.0, 0.1)).with_scale(Vec3::splat(0.4));
        let native = app
            .world_mut()
            .spawn((
                CityPlotVisual { city: 0, plot: 1, kind: Building::House, design: 0, site },
                NativeCityDistrictSurface(crate::dbpf::ResourceKey { group_id: 0, instance_id: 0, type_id: 0 }),
                base,
            ))
            .id();
        // A designed building: one root at the plot floor.
        let designed_base = Transform::from_translation(at).with_scale(Vec3::ONE);
        let designed = app
            .world_mut()
            .spawn((CityPlotVisual { city: 0, plot: 1, kind: Building::House, design: 7, site }, designed_base))
            .id();
        let scale = |app: &App, e: Entity| app.world().get::<Transform>(e).unwrap().scale.x;
        app.world_mut().resource_mut::<CivCamera>().distance = tuning::PLUMP_DISTANCE - 1.0;
        for _ in 0..5 {
            app.update();
        }
        assert!((scale(&app, native) - 0.4).abs() < 1e-6 && (scale(&app, designed) - 1.0).abs() < 1e-6);
        app.world_mut().resource_mut::<CivCamera>().distance = tuning::PLUMP_DISTANCE + 50.0;
        for _ in 0..30 {
            app.update();
        }
        assert!((scale(&app, native) - 0.5).abs() < 1e-4, "{}", scale(&app, native));
        assert!((scale(&app, designed) - 1.25).abs() < 1e-4, "{}", scale(&app, designed));
        // The designed root grows in place; the native surface about the floor point.
        assert!(app.world().get::<Transform>(designed).unwrap().translation.distance(at) < 1e-5);
        app.world_mut().resource_mut::<CivCamera>().distance = 100.0;
        for _ in 0..30 {
            app.update();
        }
        let back = app.world().get::<Transform>(native).unwrap();
        assert!((back.scale.x - 0.4).abs() < 1e-4 && back.translation.distance(base.translation) < 1e-4, "{back:?}");
    }

    #[test]
    fn the_walls_sink_by_the_tuning_depth_in_the_city_frame() {
        let sunk = crate::city_presentation::CITY_WALL_SINK_DEPTH / crate::civ_planet::CITY_SCALE;
        assert!((sunk * crate::civ_planet::CITY_SCALE - 0.2).abs() < 1e-6);
        assert!(sunk > 0.0 && sunk < 0.1);
    }
}
