//! ECS integration checks for the longer Tribal and Space progression loops.
//! These live under `runtime::tests`, so they can use its headless schedule,
//! input, and stage-transition helpers without duplicating game logic.

use super::tests::enter;
use super::*;
use crate::persistence::WorldId;

fn tribe_mate_count(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut members = world.query::<&WorldId>();
    members
        .iter(world)
        .filter(|id| {
            (crate::tribal::MATE_ID_BASE
                ..crate::tribal::MATE_ID_BASE + crate::tribal::MAX_MATES as u32)
                .contains(&id.0)
        })
        .count()
}

#[test]
fn tribal_paid_birth_grows_once_across_save_reload() {
    let mut app = headless_app(RunProgress::default());
    enter(&mut app, GameStage::Tribal);
    teleport(&mut app, crate::tribal::home_position());
    // Stage entry resets the incoming stage stockpile, so fund the real
    // recruitment action after the transition has completed.
    app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 100.0;
    // A neutral first tribe sends no raiders: the stock only pays the birth.
    app.world_mut()
        .resource_mut::<RunProgress>()
        .tribal_campaign
        .villages[0]
        .relations = crate::tribal_rules::RAID_RELATION_LIMIT;

    let initial_food = app.world().resource::<RunProgress>().dna_this_stage;
    assert_eq!(app.world().resource::<RunProgress>().tribe.len(), 3);
    assert_eq!(tribe_mate_count(&mut app), 3);
    press(&mut app, KeyCode::KeyR);

    {
        let run = app.world().resource::<RunProgress>();
        assert_eq!(
            run.dna_this_stage,
            initial_food - crate::tribal::RECRUIT_FOOD
        );
        assert_eq!(run.tribe.len(), 3, "a paid birth is not an instant adult");
        assert_eq!(run.tribal_campaign.births.len(), 1);
        assert!(
            (run.tribal_campaign.births[0].remaining - crate::tribal::BIRTH_GROWTH_SECONDS) < 0.02
        );
    }
    assert_eq!(tribe_mate_count(&mut app), 3);

    // Save with an in-flight birth. Reload the same persisted campaign and
    // continue the real Tribal schedule rather than restoring a test fixture.
    for _ in 0..600 {
        step(&mut app);
    }
    let encoded = serde_json::to_vec(app.world().resource::<RunProgress>()).unwrap();
    let saved: RunProgress = serde_json::from_slice(&encoded).unwrap();
    let remaining = saved.tribal_campaign.births[0].remaining;
    assert!(remaining > 14.0 && remaining < crate::tribal::BIRTH_GROWTH_SECONDS);

    let mut resumed = headless_app(saved);
    enter(&mut resumed, GameStage::Tribal);
    assert_eq!(resumed.world().resource::<RunProgress>().tribe.len(), 3);
    assert_eq!(tribe_mate_count(&mut resumed), 3);
    assert_eq!(
        resumed
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .births
            .len(),
        1
    );

    // The egg grows for `timeBeforeBabyGrowsUp` (60 s); 10 s passed before the
    // save, so a little over 50 s more hatches it.
    for _ in 0..((crate::tribal::BIRTH_GROWTH_SECONDS - 10.0 + 2.0) * 60.0) as usize {
        step(&mut resumed);
    }
    {
        let run = resumed.world().resource::<RunProgress>();
        assert_eq!(run.tribe.len(), 4);
        assert!(run.tribal_campaign.births.is_empty());
        assert_eq!(
            run.dna_this_stage,
            initial_food - crate::tribal::RECRUIT_FOOD
        );
    }
    assert_eq!(tribe_mate_count(&mut resumed), 4);
    for _ in 0..120 {
        step(&mut resumed);
    }
    assert_eq!(resumed.world().resource::<RunProgress>().tribe.len(), 4);
    assert_eq!(
        tribe_mate_count(&mut resumed),
        4,
        "the hatch is committed once"
    );
}

fn world_position(app: &mut App, id: u32) -> Vec3 {
    let world = app.world_mut();
    let mut planets = world.query::<(&WorldId, &Transform, &crate::game::Planet)>();
    planets
        .iter(world)
        .find(|(world_id, _, _)| world_id.0 == id)
        .map(|(_, pose, _)| pose.translation)
        .expect("campaign world is spawned")
}

fn press_near_world(app: &mut App, id: u32, key: KeyCode) {
    let position = world_position(app, id);
    teleport(app, position);
    press(app, key);
}

fn select_tool(app: &mut App, wanted: crate::space::PlanetaryTool) {
    for _ in 0..crate::space::PlanetaryTool::ALL.len() {
        if app
            .world()
            .resource::<RunProgress>()
            .space_campaign
            .planetary_tool
            == wanted
        {
            return;
        }
        press(app, KeyCode::KeyT);
    }
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .planetary_tool,
        wanted
    );
}

#[test]
fn space_input_builds_climate_foodweb_and_colony_capacity_across_reload() {
    use crate::space::PlanetaryTool;

    let mut run = RunProgress::default();
    // Fund the real paid actions; all ecology, survey and settlement changes
    // below are produced by the stage's key-input systems.
    run.spice = 2_000.0;
    let mut app = headless_app(run);
    enter(&mut app, GameStage::Space);
    {
        // Retail rules: a new Space game starts with 100,000 and must finish
        // Flight 101 before it may colonize; skip ahead with a big purse.
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        run.space_campaign
            .grant_tool(crate::space_rules::tool::INTERPLANETARY_DRIVE);
        run.spice = 2_000_000.0;
    }

    // The homeworld is already established and cannot be climate-tuned or
    // colonized again. Exercise those same protections through live inputs.
    let home_ecology = app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .ecology_for(0);
    let initial_spice = app.world().resource::<RunProgress>().spice;
    press_near_world(&mut app, 0, KeyCode::KeyY);
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(0),
        home_ecology
    );
    press_near_world(&mut app, 0, KeyCode::KeyC);
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(0),
        home_ecology
    );
    assert_eq!(app.world().resource::<RunProgress>().spice, initial_spice);

    let world_id = 1;
    press_near_world(&mut app, world_id, KeyCode::KeyV);
    assert!(app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .surveyed
        .contains(&world_id));
    press_near_world(&mut app, world_id, KeyCode::KeyC);
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(world_id)
            .colony_count,
        1
    );
    let spice_after_outpost = app.world().resource::<RunProgress>().spice;

    // Repeating C at the same world is refused at its starting capacity.
    press_near_world(&mut app, world_id, KeyCode::KeyC);
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(world_id)
            .colony_count,
        1
    );
    assert_eq!(
        app.world().resource::<RunProgress>().spice,
        spice_after_outpost
    );

    // Use T/Y to move the climate into T2: a ray's use adds its velocity to the
    // planet's marble (no Sporebucks), and the climate follows as the marble
    // rolls. Hold the ray the planner picks for a second, then let the marble
    // roll for the minute a player would wait (`coast_marble`); the energy comes
    // back through the free Recharge at the homeworld.
    for _ in 0..300 {
        if app.world().resource::<RunProgress>().space_campaign.terraform_goal_reached(world_id, 2) {
            break;
        }
        let plan = app
            .world_mut()
            .resource_mut::<RunProgress>()
            .space_campaign
            .plan_terraform_ray(world_id);
        if let Some((tool, seconds)) = plan {
            if app.world().resource::<RunProgress>().space_campaign.energy < 5.0 * seconds as f32 + 5.0 {
                app.world_mut().resource_mut::<RunProgress>().space_campaign.repair_request =
                    Some(crate::space_rules::PlanetOwner::Homeworld);
                step(&mut app);
                step(&mut app);
            }
            select_tool(&mut app, tool);
            let at = world_position(&mut app, world_id);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
            for frame in 0..seconds * 60 {
                if frame % 10 == 0 {
                    teleport(&mut app, at);
                }
                step(&mut app);
                if frame % 60 == 59
                    && app.world().resource::<RunProgress>().space_campaign.terraform_goal_reached(world_id, 2)
                {
                    break;
                }
            }
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyY);
        }
        app.world_mut().resource_scope(|_, mut run: Mut<RunProgress>| {
            let mut campaign = std::mem::take(&mut run.space_campaign);
            campaign.coast_marble(world_id, 5.0, &mut run);
            run.space_campaign = campaign;
        });
    }
    assert!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(world_id)
            .t_score()
            >= 2
    );

    // The adapted food web requires flora before herbivores and herbivores
    // before carnivores. Exercise each tool through T/Y, not direct mutation.
    for (tool, goal) in [
        (PlanetaryTool::SeedFlora, 6),
        (PlanetaryTool::SeedHerbivore, 4),
        (PlanetaryTool::SeedCarnivore, 2),
    ] {
        select_tool(&mut app, tool);
        for _ in 0..goal {
            press_near_world(&mut app, world_id, KeyCode::KeyY);
        }
    }
    let ecology = app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .ecology_for(world_id);
    assert_eq!(
        (ecology.flora, ecology.herbivores, ecology.carnivores),
        (6, 4, 2)
    );
    assert!(ecology.stable_foodweb_tier() >= 2);
    assert!(ecology.colony_capacity() >= 2);

    press_near_world(&mut app, world_id, KeyCode::KeyC);
    let run = app.world().resource::<RunProgress>();
    let expanded = run.space_campaign.ecology_for(world_id);
    assert_eq!(expanded.colony_count, 2);
    assert_eq!(
        run.space_campaign
            .colonies
            .iter()
            .filter(|&&id| id == world_id)
            .count(),
        1
    );

    let encoded = serde_json::to_vec(run).unwrap();
    let saved: RunProgress = serde_json::from_slice(&encoded).unwrap();
    let mut resumed = headless_app(saved);
    enter(&mut resumed, GameStage::Space);
    let restored = resumed.world().resource::<RunProgress>();
    // The marble keeps rolling while the resumed ship hovers over the world: the
    // food web and colonies match exactly, the climate to within a few frames of drift.
    let was = restored.space_campaign.ecology_for(world_id);
    assert_eq!(
        (was.flora, was.herbivores, was.carnivores, was.colony_count),
        (expanded.flora, expanded.herbivores, expanded.carnivores, expanded.colony_count)
    );
    assert!((was.temperature - expanded.temperature).abs() < 1.0e-3, "{was:?} vs {expanded:?}");
    assert!((was.atmosphere - expanded.atmosphere).abs() < 1.0e-3, "{was:?} vs {expanded:?}");
    assert!(restored.space_campaign.surveyed.contains(&world_id));
    assert!(restored.space_campaign.colonies.contains(&world_id));
}

fn hold_for(app: &mut App, key: KeyCode, frames: usize) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    for _ in 0..frames {
        step(app);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
    step(app);
}

fn player_pose(app: &mut App) -> Transform {
    let world = app.world_mut();
    let mut player = world.query_filtered::<&Transform, With<Player>>();
    *player.single(world).expect("one player")
}

fn clear_wild_creatures(app: &mut App) {
    use std::collections::HashSet;

    let world = app.world_mut();
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, With<Npc>>()
        .iter(world)
        .collect();
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = roots;
    while let Some(entity) = pending.pop() {
        if !seen.insert(entity) {
            continue;
        }
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
        order.push(entity);
    }
    for entity in order.into_iter().rev() {
        let _ = world.despawn(entity);
    }
}

#[test]
#[ignore = "requires the installed original creature-part catalog and imported meshes"]
fn original_mobility_parts_drive_jump_glide_sprint_sneak_and_pause() {
    use crate::{
        asset_catalog::{Capabilities, CREATURE_PARTS},
        creature::mobility::CreatureMobility,
        creature_actions::{Encounter, EncounterMode},
        game::PartInstance,
        terrain::LandSurface,
    };

    let mut app = installed_campaign_app(RunProgress::default());
    let chosen_parts = {
        let catalog = &app.world().resource::<crate::Originals>().part_catalog;
        let mut chosen = Vec::new();
        for capability in 0..4 {
            let rank = |a: Capabilities| match capability {
                0 => a.jump,
                1 => a.glide,
                2 => a.sprint,
                _ => a.sneak,
            };
            if chosen.iter().any(|part: &u32| {
                catalog
                    .by_model_in(CREATURE_PARTS, *part)
                    .is_some_and(|definition| rank(definition.abilities) > 0)
            }) {
                continue;
            }
            let definition = catalog
                .parts
                .iter()
                .filter(|candidate| candidate.key.group_id == CREATURE_PARTS)
                .filter_map(|candidate| {
                    catalog.by_model_in(CREATURE_PARTS, candidate.model.instance_id)
                })
                .find(|canonical| {
                    !canonical.is_null
                        && rank(canonical.abilities) > 0
                        && !chosen.contains(&canonical.model.instance_id)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "installed creature catalog has no canonical part granting capability {capability}"
                    )
                });
            if !chosen.contains(&definition.model.instance_id) {
                chosen.push(definition.model.instance_id);
            }
        }
        chosen
    };
    assert!(!chosen_parts.is_empty());
    {
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        for &instance in &chosen_parts {
            if !run.parts.iter().any(|part| part.instance == instance) {
                run.parts.push(PartInstance::new(instance, 0));
            }
        }
    }
    enter(&mut app, GameStage::Creature);
    clear_wild_creatures(&mut app);

    let abilities = {
        let world = app.world();
        let catalog = &world.resource::<crate::Originals>().part_catalog;
        catalog.abilities_in(CREATURE_PARTS, chosen_parts.iter().copied())
    };
    assert!(
        abilities.jump > 0 && abilities.glide > 0 && abilities.sprint > 0 && abilities.sneak > 0
    );

    // Compare actual WASD displacement with the installed Sprint rank.
    for _ in 0..3 {
        step(&mut app);
    }
    let start = player_pose(&mut app).translation;
    hold_for(&mut app, KeyCode::KeyW, 24);
    let walk_distance = player_pose(&mut app).translation.distance(start);
    teleport(&mut app, start);
    step(&mut app);
    press(&mut app, KeyCode::ShiftLeft);
    assert!(app.world().resource::<CreatureMobility>().sprinting);
    let sprint_start = player_pose(&mut app).translation;
    hold_for(&mut app, KeyCode::KeyW, 24);
    let sprint_distance = player_pose(&mut app).translation.distance(sprint_start);
    assert!(
        sprint_distance > walk_distance * 1.05,
        "walk={walk_distance} sprint={sprint_distance}"
    );

    press(&mut app, KeyCode::ShiftLeft);
    press(&mut app, KeyCode::KeyZ);
    assert!(app.world().resource::<CreatureMobility>().sneaking);
    {
        let mut encounter = app.world_mut().resource_mut::<Encounter>();
        encounter.mode = EncounterMode::Combat;
        encounter.target = Some(u32::MAX);
    }
    press(&mut app, KeyCode::Digit1);
    assert!(
        !app.world().resource::<CreatureMobility>().sneaking,
        "an attack breaks Sneak"
    );
    {
        // Esc untargets before it pauses: drop the staged target so the
        // Escape below reaches the pause menu.
        let mut encounter = app.world_mut().resource_mut::<Encounter>();
        encounter.mode = EncounterMode::Social;
        encounter.target = None;
    }

    // Jump and glide are driven by Space through creature_walk; the grounding
    // pass must keep the body at terrain height plus its standing clearance.
    let grounded = player_pose(&mut app);
    let base_surface = app
        .world()
        .resource::<LandSurface>()
        .height(grounded.translation.x, grounded.translation.z);
    let grounded_offset = grounded.translation.y - base_surface;
    press(&mut app, KeyCode::Space);
    // Observe the jump after several normal frames rather than expecting
    // 30cm of travel in the first frame consumed by a key tap.
    hold_for(&mut app, KeyCode::Space, 6);
    let airborne = player_pose(&mut app);
    let mobility = app.world().resource::<CreatureMobility>();
    assert!(mobility.height > 0.0);
    assert!(airborne.translation.y - base_surface > grounded_offset + 0.3);

    let glide_before = mobility.glide_seconds;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    for _ in 0..24 {
        step(&mut app);
    }
    let gliding = app.world().resource::<CreatureMobility>();
    assert!(gliding.gliding);
    assert!(gliding.glide_seconds < glide_before);
    let airborne = player_pose(&mut app);
    let surface_y = app
        .world()
        .resource::<LandSurface>()
        .height(airborne.translation.x, airborne.translation.z);
    assert!(airborne.translation.y > surface_y + grounded_offset + 0.3);

    // Checkpoint capture removes transient jump height and stores the stable
    // standing basis; reload must re-ground once against the regenerated land.
    crate::persistence::capture(app.world_mut());
    let expected_standing_y = 0.7 * airborne.scale.x;
    let checkpoint_y = app
        .world()
        .resource::<RunProgress>()
        .checkpoint
        .as_ref()
        .expect("captured creature checkpoint")
        .player
        .position[1];
    assert!((checkpoint_y - expected_standing_y).abs() < 1e-3);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    step(&mut app);
    assert!(
        !app.world().resource::<CreatureMobility>().gliding,
        "releasing Space ends glide"
    );

    press(&mut app, KeyCode::Escape);
    assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
    let frozen_pose = player_pose(&mut app);
    let frozen_mobility = app.world().resource::<CreatureMobility>().clone();
    hold_for(&mut app, KeyCode::KeyW, 18);
    assert_eq!(player_pose(&mut app), frozen_pose);
    assert_eq!(*app.world().resource::<CreatureMobility>(), frozen_mobility);
    press(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    for _ in 0..180 {
        if app.world().resource::<CreatureMobility>().height == 0.0 {
            break;
        }
        step(&mut app);
    }
    assert_eq!(app.world().resource::<CreatureMobility>().height, 0.0);
    let landed = player_pose(&mut app);
    let land_y = app
        .world()
        .resource::<LandSurface>()
        .height(landed.translation.x, landed.translation.z);
    assert!((landed.translation.y - land_y - grounded_offset).abs() < 0.1);

    let saved: RunProgress = serde_json::from_str(
        &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
    )
    .unwrap();
    let mut resumed = installed_campaign_app(saved);
    enter(&mut resumed, GameStage::Creature);
    for _ in 0..3 {
        step(&mut resumed);
    }
    let restored = player_pose(&mut resumed);
    let restored_surface = resumed
        .world()
        .resource::<LandSurface>()
        .height(restored.translation.x, restored.translation.z);
    assert_eq!(resumed.world().resource::<CreatureMobility>().height, 0.0);
    assert!(restored.translation.y - restored_surface > 0.0);
    assert!((restored.translation.y - restored_surface - grounded_offset).abs() < 0.1);
}
