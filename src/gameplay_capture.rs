//! Bounded diagnostic snapshots for real gameplay systems.
//!
//! These scenarios stage visible state for render comparison. Their injected
//! budgets, part choices, and actor conditions are diagnostic setups, not
//! naturally earned campaign progress.

use super::*;
use crate::{
    game::{Player, RunProgress},
    stages::GameStage,
};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

/// Arrange one diagnostic quick-capture scenario. Returns `true` only when
/// this module handled the requested stage/scenario pair.
pub(super) fn stage(app: &mut App, window: Entity, name: &str, scenario: &str, target: &Handle<Image>, output: &std::path::Path) -> bool {
    if let Some(mut save) = app
        .world_mut()
        .get_resource_mut::<crate::persistence::SaveState>()
    {
        save.enabled = false;
    }
    let stage = app.world().resource::<State<GameStage>>().get().clone();
    match (stage, name, scenario) {
        (GameStage::Creature, "creature", "mobility") => stage_mobility(app, window),
        (GameStage::Tribal, "tribal", "tribe-needs") => stage_tribe_needs(app, window),
        (GameStage::Tribal, "tribal", "domestication") => stage_domestication(app, window),
        (GameStage::Tribal, "tribal", "pets-defend") => stage_pet_guard(app, window),
        (GameStage::Tribal, "tribal", "song") => stage_song(app, window, target, output),
        (GameStage::Space, "space", "ecology") => stage_ecology(app, window),
        _ => false,
    }
}

fn player_entity(world: &mut World) -> Entity {
    world
        .query_filtered::<Entity, With<Player>>()
        .single(world)
        .expect("quick capture has a player")
}

fn player_position(world: &mut World) -> Vec3 {
    let player = player_entity(world);
    world
        .get::<Transform>(player)
        .expect("player transform")
        .translation
}

fn set_player_position(world: &mut World, position: Vec3) {
    let player = player_entity(world);
    if let Some(mut pose) = world.get_mut::<Transform>(player) {
        pose.translation = position;
    }
}

fn capture_camera(at: Vec3, offset: Vec3) -> Transform {
    Transform::from_translation(at + offset).looking_at(at, Vec3::Y)
}

fn diagnostic_message(run: &mut RunProgress, text: &str) {
    match run.current_stage() {
        GameStage::Creature => {}
        GameStage::Tribal => run.tribal_campaign.say(format!("DIAGNOSTIC — {text}")),
        GameStage::Space => run.space_campaign.message = format!("DIAGNOSTIC — {text}"),
        _ => {}
    }
}

fn stage_mobility(app: &mut App, window: Entity) -> bool {
    use crate::asset_catalog::{Capabilities, CREATURE_PARTS};

    let chosen = {
        let catalog = &app.world().resource::<crate::Originals>().part_catalog;
        let mut definitions = Vec::new();
        for capability in 0..4 {
            let has_rank = |abilities: Capabilities| match capability {
                0 => abilities.jump,
                1 => abilities.glide,
                2 => abilities.sprint,
                _ => abilities.sneak,
            };
            let already_covered =
                definitions
                    .iter()
                    .any(|definition: &crate::asset_catalog::PartDefinition| {
                        has_rank(definition.abilities) > 0
                    });
            if already_covered {
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
                        && has_rank(canonical.abilities) > 0
                        && !definitions.iter().any(
                            |selected: &crate::asset_catalog::PartDefinition| {
                                selected.model.instance_id == canonical.model.instance_id
                            },
                        )
                });
            let Some(definition) = definition else {
                warn!(
                    "mobility quick capture: no canonical original part grants capability {capability}"
                );
                return false;
            };
            definitions.push(definition.clone());
        }
        definitions
    };

    let mut instances = Vec::new();
    for definition in &chosen {
        let (mut originals, mut meshes, mut images, mut materials) = {
            let world = app.world_mut();
            (
                world
                    .remove_resource::<crate::Originals>()
                    .expect("original catalog"),
                world
                    .remove_resource::<Assets<Mesh>>()
                    .expect("mesh assets"),
                world
                    .remove_resource::<Assets<Image>>()
                    .expect("image assets"),
                world
                    .remove_resource::<Assets<StandardMaterial>>()
                    .expect("material assets"),
            )
        };
        let imported = crate::editor::ensure_model(
            &mut originals,
            definition,
            &mut meshes,
            &mut images,
            &mut materials,
        );
        {
            let world = app.world_mut();
            world.insert_resource(originals);
            world.insert_resource(meshes);
            world.insert_resource(images);
            world.insert_resource(materials);
        }
        match imported {
            Ok(()) => instances.push(definition.model.instance_id),
            Err(error) => {
                warn!(
                    "mobility quick capture could not import {}: {error}",
                    definition.name
                );
                return false;
            }
        }
    }

    {
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        for &instance in &instances {
            if !run.parts.iter().any(|part| part.instance == instance) {
                run.parts.push(crate::game::PartInstance::new(instance, 0));
            }
        }
        run.focused_part = 0;
    }
    let abilities = crate::creature_actions::player_abilities(
        app.world().resource::<RunProgress>(),
        app.world().resource::<crate::Originals>(),
    );
    if abilities.jump == 0 || abilities.glide == 0 || abilities.sprint == 0 || abilities.sneak == 0
    {
        warn!("mobility quick capture: canonical native loadout is missing a movement rank");
        return false;
    }
    app.world_mut()
        .resource_mut::<crate::creature::mobility::CreatureMobility>()
        .reset_for_respawn(abilities);
    let _ = app
        .world_mut()
        .run_system_once(crate::creature::spawn_creature_parts);
    crate::player_appearance::sync(app.world_mut());

    let start = player_position(app.world_mut());
    app.world_mut()
        .insert_resource(RenderSmokeCamera(capture_camera(
            start + Vec3::Y * 0.4,
            Vec3::new(3.8, 3.0, -7.5),
        )));
    render_key(app, window, KeyCode::ShiftLeft, "Shift");
    render_key_down(app, window, KeyCode::KeyW, "w");
    for _ in 0..8 {
        app.update();
        gpu_flush(app);
    }
    render_key_down(app, window, KeyCode::Space, " ");
    // Leave both controls held through the caller's final render frames so
    // the real jump/glide system is still airborne and moving at capture.
    for _ in 0..24 {
        app.update();
        gpu_flush(app);
    }
    diagnostic_message(
        &mut app.world_mut().resource_mut::<RunProgress>(),
        "original mobility parts: Jump / Glide / Sprint / Sneak",
    );
    app.world_mut()
        .resource_mut::<crate::creature_actions::Encounter>()
        .message = "DIAGNOSTIC — original Jump / Glide / Sprint / Sneak parts".into();
    true
}

fn render_key_down(app: &mut App, window: Entity, key_code: KeyCode, character: &str) {
    use bevy::input::keyboard::{Key, KeyboardInput};
    app.world_mut()
        .resource_mut::<Messages<KeyboardInput>>()
        .write(KeyboardInput {
            key_code,
            logical_key: Key::Character(character.into()),
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
    app.update();
    gpu_flush(app);
}

/// Release the diagnostic locomotion inputs after the caller has captured its
/// airborne frame, before the same app advances into another staged scene.
pub(super) fn release_mobility_inputs(app: &mut App, window: Entity) {
    render_key_up(app, window, KeyCode::Space, " ");
    render_key_up(app, window, KeyCode::KeyW, "w");
}

fn render_key_up(app: &mut App, window: Entity, key_code: KeyCode, character: &str) {
    use bevy::input::keyboard::{Key, KeyboardInput};
    app.world_mut()
        .resource_mut::<Messages<KeyboardInput>>()
        .write(KeyboardInput {
            key_code,
            logical_key: Key::Character(character.into()),
            state: bevy::input::ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
    app.update();
    gpu_flush(app);
}

fn stage_tribe_needs(app: &mut App, window: Entity) -> bool {
    let (mate, food_mat) = {
        let world = app.world_mut();
        let mut units = world.query::<(
            Entity,
            &crate::persistence::WorldId,
            &crate::tribal_rts::TribeUnit,
        )>();
        let mate = units
            .iter(world)
            .find(|(_, _, unit)| !unit.chieftain)
            .map(|(entity, id, _)| (entity, id.0))
            .expect("tribal mate spawned");
        (mate, crate::tribal::food_pile_position())
    };
    {
        let (entity, stable_id) = mate;
        let world = app.world_mut();
        let slot = stable_id.saturating_sub(crate::tribal::MATE_ID_BASE);
        world
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .member_energy
            .insert(slot, 5.0);
        if let Some(mut unit) = world.get_mut::<crate::tribal_rts::TribeUnit>(entity) {
            unit.energy = 5.0;
            unit.recovery_work = 0.0;
        }
        if let Some(mut task) = world.get_mut::<crate::tribal_rts::UnitTask>(entity) {
            *task = crate::tribal_rts::UnitTask::Recover;
        }
        if let Some(mut pose) = world.get_mut::<Transform>(entity) {
            pose.translation = food_mat;
        }
        let mut run = world.resource_mut::<RunProgress>();
        run.dna_this_stage = 120.0;
    }

    render_key(app, window, crate::tribal_controls::KEY_SELECT_TRIBE, "f15");
    render_key(app, window, KeyCode::KeyZ, "z");
    set_player_position(app.world_mut(), crate::tribal::home_position());
    render_key(app, window, KeyCode::KeyR, "r");
    if app
        .world()
        .resource::<RunProgress>()
        .tribal_campaign
        .births
        .is_empty()
    {
        warn!("Tribal needs diagnostic could not queue the paid egg");
        return false;
    }

    // Make the visible low-energy mate take the real stockpile recovery path
    // while the paid egg grows in parallel.
    {
        let (entity, stable_id) = mate;
        let world = app.world_mut();
        let slot = stable_id.saturating_sub(crate::tribal::MATE_ID_BASE);
        world
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .member_energy
            .insert(slot, 5.0);
        if let Some(mut unit) = world.get_mut::<crate::tribal_rts::TribeUnit>(entity) {
            unit.energy = 5.0;
            unit.recovery_work = 0.0;
        }
        if let Some(mut task) = world.get_mut::<crate::tribal_rts::UnitTask>(entity) {
            *task = crate::tribal_rts::UnitTask::Recover;
        }
        if let Some(mut pose) = world.get_mut::<Transform>(entity) {
            pose.translation = food_mat;
        }
        let chief = player_entity(world);
        if let Some(mut player) = world.get_mut::<Transform>(chief) {
            player.translation = crate::tribal::home_position();
        }
    }
    for _ in 0..90 {
        app.update();
        gpu_flush(app);
    }
    let hungry_mate_remains = app
        .world_mut()
        .get::<crate::tribal_rts::TribeUnit>(mate.0)
        .is_some_and(|unit| unit.energy <= crate::tribal_rts::ENERGY_RETURN_THRESHOLD);
    if !hungry_mate_remains
        || app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .births
            .is_empty()
    {
        warn!("Tribal needs diagnostic lost its hungry member or pending egg");
        return false;
    }
    let at = player_position(app.world_mut());
    app.world_mut()
        .insert_resource(RenderSmokeCamera(capture_camera(
            at + Vec3::Y * 0.7,
            Vec3::new(8.0, 5.6, -11.0),
        )));
    diagnostic_message(
        &mut app.world_mut().resource_mut::<RunProgress>(),
        "hungry member recovering; paid egg growing (diagnostic state)",
    );
    true
}

/// The social game at the first village: the chieftain and the tribe stand at it,
/// the song starts (the perform key), the instruments are ordered at the frames
/// of `OPENSPORE_SONG_PLAN` (`frame:key,...`, keys 3 / 4 / 5, frames counted from
/// the start of the song) and the scene is captured after
/// `OPENSPORE_SONG_FRAMES` frames (default 200). A diagnostic setup: the song is
/// the real game, only the walk to the village is skipped.
fn stage_song(app: &mut App, window: Entity, target: &Handle<Image>, output: &std::path::Path) -> bool {
    let village = 0;
    // The tribes come into view over the stage; the song needs the first one.
    app.world_mut().resource_mut::<RunProgress>().tribal_campaign.reveal_all();
    for _ in 0..4 {
        app.update();
        gpu_flush(app);
    }
    // The troupe: the three social tools built and handed round the members (what
    // is still locked fails and stays without), as the tests' songs do.
    {
        use crate::tribal_tools::ToolKind;
        let kinds = [ToolKind::WoodenHorns, ToolKind::Maracas, ToolKind::Aerophones];
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        let run = &mut *run;
        run.dna_this_stage = 400.0;
        for kind in kinds {
            let _ = run.tribal_campaign.build_shack(kind, 999.0);
        }
        for (i, slot) in run.tribal_campaign.slots.clone().into_iter().enumerate() {
            let _ = run.tribal_campaign.equip(crate::tribal::MATE_ID_BASE + slot, kinds[i % 3]);
        }
    }
    for _ in 0..4 {
        app.update();
        gpu_flush(app);
    }
    let at = {
        let _layout = crate::tribal_planet::scope_world(app.world());
        crate::tribal::village_position(village)
    };
    let dir = (crate::tribal::home_position() - at).normalize_or(Vec3::X);
    let spot = at + Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::X) * 9.0;
    set_player_position(app.world_mut(), spot);
    {
        // The chieftain stays where it was put (an idle order at its own spot).
        let world = app.world_mut();
        let chief = player_entity(world);
        if let Some(mut task) = world.get_mut::<crate::tribal_rts::UnitTask>(chief) {
            *task = crate::tribal_rts::UnitTask::Idle(spot);
        }
    }
    {
        let world = app.world_mut();
        let members: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::game::TribeMate>>()
            .iter(world)
            .collect();
        for (i, member) in members.into_iter().enumerate() {
            if let Some(mut pose) = world.get_mut::<Transform>(member) {
                pose.translation = spot + Vec3::new(-1.5 + i as f32 * 1.1, 0.0, 1.5);
            }
        }
    }
    render_key(app, window, crate::tribal_controls::KEY_SELECT_TRIBE, "f15");
    // The chieftain stands where it was put when the perform key arrives.
    set_player_position(app.world_mut(), spot);
    for _ in 0..3 {
        app.update();
        gpu_flush(app);
        set_player_position(app.world_mut(), spot);
    }
    {
        let _layout = crate::tribal_planet::scope_world(app.world());
        let here = player_position(app.world_mut());
        info!(
            "song scenario: chieftain {here:?}, village {at:?}, distance {:.1}, range {}",
            crate::game::dist_xz(here, at),
            crate::tribal::INTERACT_RANGE
        );
    }
    {
        let _layout = crate::tribal_planet::scope_world(app.world());
        let here = player_position(app.world_mut());
        let campaign = &app.world().resource::<RunProgress>().tribal_campaign;
        info!(
            "song scenario: nearest hostile {:?}, statuses {:?}",
            crate::tribal::nearest_hostile(here, campaign),
            (0..crate::tribal::RIVAL_VILLAGES).map(|v| campaign.village(v).map(|s| s.status)).collect::<Vec<_>>()
        );
    }
    // The camera follows a tribe member (Space cycles, the chieftain first).
    render_key(app, window, KeyCode::Space, " ");
    set_player_position(app.world_mut(), spot);
    render_key(app, window, crate::tribal_controls::KEY_PERFORM, "f18");
    {
        let _layout = crate::tribal_planet::scope_world(app.world());
        let here = player_position(app.world_mut());
        info!("song scenario after the key: chieftain {here:?}, distance {:.1}", crate::game::dist_xz(here, at));
    }
    let plan: Vec<(usize, KeyCode, &str)> = std::env::var("OPENSPORE_SONG_PLAN")
        .unwrap_or_default()
        .split(',')
        .filter_map(|item| {
            let (frame, key) = item.split_once(':')?;
            let (code, text) = match key {
                "3" => (KeyCode::Digit3, "3"),
                "4" => (KeyCode::Digit4, "4"),
                "5" => (KeyCode::Digit5, "5"),
                _ => return None,
            };
            Some((frame.parse().ok()?, code, text))
        })
        .collect();
    let frames: usize = std::env::var("OPENSPORE_SONG_FRAMES").ok().and_then(|f| f.parse().ok()).unwrap_or(200);
    // `OPENSPORE_SONG_SNAPS=frame,frame,...`: extra captures `song-<frame>.png`.
    let snaps: Vec<usize> = std::env::var("OPENSPORE_SONG_SNAPS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|f| f.trim().parse().ok())
        .collect();
    // `OPENSPORE_SONG_THRESHOLD=score`: the bar's threshold (a short song for the
    // captures of the full bar and the success burst).
    if let Some(threshold) = std::env::var("OPENSPORE_SONG_THRESHOLD").ok().and_then(|t| t.parse::<f32>().ok()) {
        if let Some(p) = app.world_mut().resource_mut::<RunProgress>().tribal_campaign.performance.as_mut() {
            p.threshold = threshold;
        }
    }
    // `OPENSPORE_SONG_ANSWER=1`: every bubble is answered with its instrument.
    let answer = std::env::var_os("OPENSPORE_SONG_ANSWER").is_some();
    for frame in 0..frames {
        if answer {
            let p = app.world().resource::<RunProgress>().tribal_campaign.performance;
            if let Some(p) = p.filter(|p| p.bubble && !p.evaluated) {
                if let Some(kind) = crate::tribal::instrument_kind(p.prompt).filter(|k| p.refresh[*k] <= 0.0) {
                    let (code, text) = [(KeyCode::Digit3, "3"), (KeyCode::Digit4, "4"), (KeyCode::Digit5, "5")][kind];
                    render_key(app, window, code, text);
                }
            }
        }
        if snaps.contains(&frame) {
            crate::runtime::capture_render(app, target, &output.join(format!("song-{frame}.png")));
        }
        set_player_position(app.world_mut(), spot);
        if let Some((_, code, text)) = plan.iter().find(|(f, ..)| *f == frame) {
            render_key(app, window, *code, text);
        }
        app.update();
        gpu_flush(app);
    }
    let started = app
        .world()
        .resource::<RunProgress>()
        .tribal_campaign
        .performance
        .is_some()
        || app.world().resource::<RunProgress>().tribal_campaign.social_end.is_some();
    if !started {
        let campaign = &app.world().resource::<RunProgress>().tribal_campaign;
        warn!("Tribal song diagnostic: the song did not start ({:?}, cooldown {})", campaign.message, campaign.cooldown);
    }
    started
}

fn stage_ecology(app: &mut App, window: Entity) -> bool {
    use crate::space::PlanetaryTool;

    let world_id = 1;
    app.world_mut().resource_mut::<RunProgress>().spice = 1_000.0;
    place_ship_at_world(app, world_id);
    render_key(app, window, KeyCode::KeyV, "v");
    if !app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .surveyed
        .contains(&world_id)
    {
        warn!("Space ecology diagnostic could not survey target world {world_id}");
        return false;
    }
    place_ship_at_world(app, world_id);
    render_key(app, window, KeyCode::KeyC, "c");
    if app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .ecology_for(world_id)
        .colony_count
        < 1
    {
        warn!("Space ecology diagnostic could not establish target outpost {world_id}");
        return false;
    }

    // The climate moves as the planet's marble rolls: hold the planned ray for a
    // second (ten uses at `AutoFireRate` 0.1 s), then let the marble roll for the
    // minute a player would wait (`coast_marble`); energy comes back through the
    // free Recharge at the homeworld.
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
                app.update();
                app.update();
            }
            select_planetary_tool(app, window, tool);
            place_ship_at_world(app, world_id);
            render_key_down(app, window, KeyCode::KeyY, "y");
            for frame in 0..seconds * 60 {
                if frame % 10 == 0 {
                    place_ship_at_world(app, world_id);
                }
                app.update();
                if frame % 60 == 59
                    && app.world().resource::<RunProgress>().space_campaign.terraform_goal_reached(world_id, 2)
                {
                    break;
                }
            }
            render_key_up(app, window, KeyCode::KeyY, "y");
        }
        app.world_mut().resource_scope(|_, mut run: Mut<RunProgress>| {
            let mut campaign = std::mem::take(&mut run.space_campaign);
            campaign.coast_marble(world_id, 5.0, &mut run);
            run.space_campaign = campaign;
        });
    }
    if app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .ecology_for(world_id)
        .t_score()
        < 2
    {
        warn!("Space ecology diagnostic could not reach T2 within its bounded actions");
        return false;
    }

    for (tool, count) in [
        (PlanetaryTool::SeedFlora, 6),
        (PlanetaryTool::SeedHerbivore, 4),
        (PlanetaryTool::SeedCarnivore, 2),
    ] {
        select_planetary_tool(app, window, tool);
        for _ in 0..count {
            place_ship_at_world(app, world_id);
            render_key(app, window, KeyCode::KeyY, "y");
        }
    }
    place_ship_at_world(app, world_id);
    render_key(app, window, KeyCode::KeyC, "c");
    if app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .ecology_for(world_id)
        .colony_count
        < 2
    {
        warn!("Space ecology diagnostic could not expand target settlement {world_id}");
        return false;
    }

    let focus = {
        let world = app.world_mut();
        let mut planets = world.query::<(
            &crate::persistence::WorldId,
            &Transform,
            &crate::game::Planet,
        )>();
        planets
            .iter(world)
            .find(|(id, _, _)| id.0 == world_id)
            .map(|(_, pose, planet)| (pose.translation, planet.radius))
            .expect("world 1 remains spawned")
    };
    app.world_mut()
        .insert_resource(RenderSmokeCamera(capture_camera(
            focus.0,
            Vec3::new(focus.1 * 2.2, focus.1 * 1.3, -focus.1 * 3.5),
        )));
    diagnostic_message(
        &mut app.world_mut().resource_mut::<RunProgress>(),
        "T2 climate, 6/4/2 food web, second settlement (diagnostic state)",
    );
    true
}

/// Keep the ship inside the selected world's interaction radius after each
/// rendered key tap; normal Space updates continue to advance while capturing.
fn place_ship_at_world(app: &mut App, world_id: u32) {
    let (position, radius) = {
        let world = app.world_mut();
        let mut planets = world.query::<(
            &crate::persistence::WorldId,
            &Transform,
            &crate::game::Planet,
        )>();
        planets
            .iter(world)
            .find(|(id, _, _)| id.0 == world_id)
            .map(|(_, pose, planet)| (pose.translation, planet.radius))
            .expect("diagnostic target planet remains spawned")
    };
    let player = player_entity(app.world_mut());
    if let Some(mut pose) = app.world_mut().get_mut::<Transform>(player) {
        pose.translation = position + Vec3::Y * (radius + 1.0);
    }
    if let Some(mut motion) = app.world_mut().get_mut::<crate::space::ShipMotion>(player) {
        motion.velocity = Vec3::ZERO;
        motion.thrusting = false;
    }
}

fn select_planetary_tool(app: &mut App, window: Entity, wanted: crate::space::PlanetaryTool) {
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
        render_key(app, window, KeyCode::KeyT, "t");
    }
    assert_eq!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .planetary_tool,
        wanted
    );
}

fn stage_domestication(app: &mut App, window: Entity) -> bool {
    use crate::tribal_rts::{self, livestock};
    let pen = livestock::pen_position();
    let world = app.world_mut();
    let animal = world
        .query::<(Entity, &tribal_rts::WildAnimal)>()
        .iter(world)
        .find(|(_, animal)| !animal.hostile)
        .map(|(entity, _)| entity)
        .expect("Tribal stage has a wild grazer");
    // Keep unrelated hunts and carcass deliveries out of this payment/egg
    // diagnostic. The separate defense scenario brings a real predator back.
    for (entity, mut pose, mut wild) in world
        .query::<(Entity, &mut Transform, &mut tribal_rts::WildAnimal)>()
        .iter_mut(world)
    {
        if entity != animal {
            pose.translation.x = 29.0;
            pose.translation.z = 29.0;
            wild.home = pose.translation;
            wild.target = pose.translation;
            wild.wait = 1_000.0;
        }
    }
    world.get_mut::<Transform>(animal).unwrap().translation = pen;
    {
        let mut wild = world.get_mut::<tribal_rts::WildAnimal>(animal).unwrap();
        wild.home = pen;
        wild.target = pen;
        wild.wait = 1_000.0;
    }
    let selected: Vec<_> = world
        .query_filtered::<Entity, With<tribal_rts::Selected>>()
        .iter(world)
        .collect();
    for entity in selected {
        world.entity_mut(entity).remove::<tribal_rts::Selected>();
    }
    let member = world
        .query_filtered::<Entity, With<crate::game::TribeMate>>()
        .iter(world)
        .next()
        .unwrap();
    world.entity_mut(member).insert(tribal_rts::Selected);
    world.get_mut::<Transform>(member).unwrap().translation = pen + Vec3::X * 0.5;
    world.resource_mut::<RunProgress>().dna_this_stage = 60.0;
    render_key(app, window, KeyCode::KeyK, "k");
    for _ in 0..1_420 {
        app.update();
        gpu_flush(app);
    }
    let run = app.world().resource::<RunProgress>();
    assert_eq!(
        run.tribal_campaign.livestock.len(),
        1,
        "the real tame order must complete once"
    );
    assert!(
        (run.dna_this_stage - (60.0 - livestock::TAME_FOOD)).abs() < 0.01,
        "taming pays actual stock; observed {} food, expected {}",
        run.dna_this_stage,
        60.0 - livestock::TAME_FOOD,
    );
    assert!(
        run.tribal_campaign.livestock_eggs >= livestock::EGG_FOOD,
        "a settled animal must produce collectable eggs"
    );
    app.world_mut()
        .insert_resource(RenderSmokeCamera(capture_camera(
            pen + Vec3::Y,
            Vec3::new(-8.0, 5.6, 10.0),
        )));
    diagnostic_message(
        &mut app.world_mut().resource_mut::<RunProgress>(),
        "paid domestication and eggs at the pen; J gathers (diagnostic animal position / budget)",
    );
    true
}

fn stage_pet_guard(app: &mut App, window: Entity) -> bool {
    use crate::tribal_rts::{livestock, TribeUnit, UnitTask, WildAnimal};
    if !stage_domestication(app, window) {
        return false;
    }
    let world = app.world_mut();
    let pet = world
        .query_filtered::<Entity, With<livestock::DomesticAnimal>>()
        .single(world)
        .expect("diagnostic acquired one real pet");
    let at = world.get::<Transform>(pet).unwrap().translation;
    // Move existing tribe members aside so this image exercises the pet's
    // own defense. The pet was acquired by the normal paid K order above.
    for (mut t, mut task) in world
        .query_filtered::<(&mut Transform, &mut UnitTask), With<TribeUnit>>()
        .iter_mut(world)
    {
        t.translation.x = 15.0;
        t.translation.z = -10.0;
        *task = UnitTask::Idle(t.translation);
    }
    let predator = world
        .query::<(Entity, &WildAnimal)>()
        .iter(world)
        .find(|(_, a)| a.hostile)
        .map(|(e, _)| e)
        .unwrap();
    let position = at + Vec3::X * 2.0;
    world.get_mut::<Transform>(predator).unwrap().translation = position;
    {
        let mut animal = world.get_mut::<WildAnimal>(predator).unwrap();
        animal.home = position;
        animal.target = position;
        animal.wait = 1_000.0;
        animal.chase = None;
    }
    for _ in 0..100 {
        app.update();
        gpu_flush(app);
    }
    assert!(
        app.world().get::<crate::game::Health>(pet).unwrap().hp < 40.0,
        "pet must receive real predator damage"
    );
    assert!(
        app.world().get::<crate::game::Health>(predator).unwrap().hp < 70.0,
        "pet must strike back through the normal combat schedule"
    );
    app.world_mut()
        .insert_resource(RenderSmokeCamera(capture_camera(
            at + Vec3::Y,
            Vec3::new(-8.0, 5.6, 10.0),
        )));
    diagnostic_message(
        &mut app.world_mut().resource_mut::<RunProgress>(),
        "pet defending the pen; both animals take damage (diagnostic predator position)",
    );
    true
}
