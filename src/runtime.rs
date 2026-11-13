//! Deterministic gameplay smoke checks, using the desktop game's schedule.
use crate::{
    game::*,
    stages::{GameStage, StageOwned},
};
use bevy::{prelude::*, state::app::StatesPlugin, time::TimeUpdateStrategy};
use std::path::Path;
use std::time::Duration;

#[path = "gameplay_capture.rs"]
mod gameplay_capture;

pub fn headless_app(run: RunProgress) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
    crate::configure_game(&mut app, run);
    app.insert_resource(crate::StartupComplete);
    app.world_mut()
        .resource_mut::<crate::persistence::SaveState>()
        .enabled = false;
    step(&mut app);
    app
}

/// Village `index` of this app's Tribal layout (tests run apps in parallel;
/// each reads its own `tribal_planet::StageLayout`).
fn village_at(app: &App, index: usize) -> Vec3 {
    let _layout = crate::tribal_planet::scope_world(app.world());
    crate::tribal::village_position(index)
}

fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    step(app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
}

fn stage(app: &App) -> GameStage {
    app.world().resource::<State<GameStage>>().get().clone()
}

fn teleport(app: &mut App, position: Vec3) {
    let world = app.world_mut();
    let mut players = world.query_filtered::<&mut Transform, With<Player>>();
    players.single_mut(world).expect("one player").translation = position;
}

/// Hold or release Ctrl+Alt (the Space debug chord that enables the ship
/// loadout keys sharing keys with the camera).
fn set_debug_chord(app: &mut App, down: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    for key in [KeyCode::ControlLeft, KeyCode::AltLeft] {
        if down {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
}

/// Civ test diagnostics: every rival's kind, owner, relationship value, war flag and
/// trade flag ("C4:Economic:rival/-3.0/W/T"), for the failure messages of the arc.
fn civ_nations_line(run: &RunProgress) -> String {
    (1..crate::civ::CITY_COUNT)
        .map(|n| {
            let city = &run.civ_campaign.cities[n];
            let nation = &run.civ_campaign.nations[n];
            format!(
                "C{}:{:?}:{}/{:.1}/{}{}",
                n + 1,
                city.kind,
                if city.owned() { "mine" } else { "rival" },
                nation.relation.value(),
                if nation.war { "W" } else { "-" },
                if city.trade { "T" } else { "-" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn check_civilization(app: &mut App) {
    assert_eq!(stage(app), GameStage::Civilization);
    // The opening fleet rolls out on the first simulated frame.
    step(app);
    // Spore's economy: vehicles claim spice geysers with derricks while the
    // capital and its derricks pay in.
    let derricks = |app: &App| {
        app.world()
            .resource::<RunProgress>()
            .civ_campaign
            .derricks(crate::civ::HOME_INDEX)
    };
    let before = derricks(app);
    press(app, KeyCode::KeyQ);
    press(app, KeyCode::KeyX);
    for _ in 0..1500 {
        if derricks(app) > before {
            break;
        }
        step(app);
    }
    assert!(
        derricks(app) > before,
        "vehicles must claim spice geysers with derricks"
    );
    assert_eq!(
        stage(app),
        GameStage::Civilization,
        "spice collection alone must not launch to Space"
    );
    // The planet layout rolls the rival types (retail 0xCF8D00, `civ_planet::lay_out`);
    // the arc takes each city with the doctrine of the earlier cycle of types, so
    // fix them to it here.
    {
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        for city in 1..crate::civ::CITY_COUNT {
            run.civ_campaign.cities[city].retype(crate::civ::RIVAL_KINDS[(city - 1) % crate::civ::RIVAL_COUNT]);
        }
    }
    // Each rival city falls to vehicles of the doctrine chosen for it: built
    // with G, selected with Q and sent with F against the city nearest the
    // camera. G builds at the owned city nearest the camera and the vehicle takes
    // that city's type (the capital's type follows the doctrine key); right after a
    // capture the camera is still at the captured city, whose type is the earlier
    // doctrine's, and Q selects every vehicle when none has the doctrine's type. So
    // the fleet is built standing at the capital and only the doctrine's vehicles
    // are selected: a leftover vehicle of another doctrine must not be sent along
    // (a military shell on a trade partner's building makes it hostile, and a
    // hostile nation refuses the trade route).
    let build_fleet = |app: &mut App, count: usize| {
        let capital = app.world().resource::<RunProgress>().civ_campaign.cities[crate::civ::HOME_INDEX].center();
        teleport(app, capital);
        for _ in 0..count {
            press(app, KeyCode::KeyG);
        }
    };
    let doctrine_vehicles = |app: &mut App| {
        let world = app.world_mut();
        let kind = world.resource::<RunProgress>().civ_campaign.approach;
        world
            .query::<&crate::civ_rts::CivUnit>()
            .iter(world)
            .filter(|u| u.nation == crate::civ::HOME_INDEX && u.kind == kind && u.hp > 0.0)
            .count()
    };
    for (index, key) in [
        (1, KeyCode::Digit1),
        (2, KeyCode::Digit2),
        (3, KeyCode::Digit3),
        (4, KeyCode::Digit1),
    ] {
        press(app, key);
        // On the planet the cities are 180-600 apart: a fleet built together
        // arrives together (vehicles trickling in one by one are picked off by
        // the rival's garrison). Test pacing: a purse for four vehicles.
        app.world_mut().resource_mut::<RunProgress>().spice += 4.0 * crate::civ::LAND_VEHICLE_COST;
        build_fleet(app, 4);
        assert!(doctrine_vehicles(app) > 0, "doctrine {key:?} builds vehicles at the capital");
        press(app, KeyCode::KeyQ);
        let mut next_wave = 1800;
        // Frame budget: capture points decay between waves when frames run
        // slow (full-suite load), so allow a long siege; it ends at capture.
        for frame in 0..150000 {
            if stage(app) != GameStage::Civilization
                || app.world().resource::<RunProgress>().civ_campaign.cities[index].owned()
            {
                break;
            }
            // The retail defence (600-health turrets, 800 per building, the
            // hall last) beats a trickle: once the vehicles at the city are
            // spent, a new wave is built to the vehicle cap and sent together
            // (test purse; vehicles stuck on the way do not count).
            if frame % 120 == 0 {
                let kind = app.world().resource::<RunProgress>().civ_campaign.approach;
                if kind == crate::civ::Approach::Economic {
                    // The retail economic conquest: the route opens, then the city is
                    // bought in one payment from the communication screen (`civ_comm`;
                    // the route's vehicles no longer pay a second at a time).
                    let mut run = app.world_mut().resource_mut::<RunProgress>();
                    let run = &mut *run;
                    if run.civ_campaign.cities[index].trade && !run.civ_campaign.cities[index].owned() {
                        run.spice += 50_000.0;
                        let _ = crate::civ_hud::comm::apply_buy(&mut run.civ_campaign, &mut run.spice, index);
                    }
                }
                let world = app.world_mut();
                let center = world.resource::<RunProgress>().civ_campaign.cities[index].center();
                let engaged = world
                    .query::<(&crate::civ_rts::CivUnit, &Transform)>()
                    .iter(world)
                    .filter(|(u, t)| {
                        u.nation == crate::civ::HOME_INDEX
                            && u.kind == kind
                            && u.hp > 0.0
                            && t.translation.distance(center) < 260.0
                    })
                    .count();
                if engaged < 2 && frame >= next_wave {
                    next_wave = frame + 1800;
                    let cap = world.resource::<RunProgress>().civ_campaign.vehicle_cap(crate::civ::HOME_INDEX);
                    world.resource_mut::<RunProgress>().spice += cap as f32 * crate::civ::LAND_VEHICLE_COST;
                    build_fleet(app, cap);
                }
                if doctrine_vehicles(app) > 0 {
                    press(app, KeyCode::KeyQ);
                }
            }
            let at = app.world().resource::<RunProgress>().civ_campaign.cities[index].center();
            teleport(app, at);
            press(app, KeyCode::KeyF);
        }
        let run = app.world().resource::<RunProgress>();
        assert!(
            run.civ_campaign.cities[index].owned(),
            "city {index} must be reachable through doctrine {key:?}: {} | {} | nations {}",
            run.civ_campaign.message,
            crate::civ::city_status(&run.civ_campaign.cities[index]),
            civ_nations_line(run)
        );
        step(app);
    }
    // Test pacing: retail's planet has ten city slots (nine rivals). The four
    // sieges above cover every doctrine and tech gate; the other rivals are
    // handed over through the capture API (a siege takes minutes of game time
    // each, and the ability / superweapon counts are unit tested).
    //
    // Every city that is not yours is handed over, the sieges' cities included:
    // the rivals fight back while a later siege goes on (0xBFA7A0 picks their
    // targets with dice, and a rival's capture points that reach 100 take a city
    // of yours, `civ::lose_city`), so a city won in an earlier siege can be a
    // rival's again here. Whether it is depends on the dice and on how many
    // frames the machine ran, and on a planet that is not unified the Advance
    // button never shows (the flake of Landing 230). The stage rules are not
    // touched: only a unified planet may launch, which the checks below still
    // demand.
    let unified = |app: &App| app.world().resource::<RunProgress>().civ_campaign.is_unified();
    let retaken: Vec<usize> = {
        let run = app.world().resource::<RunProgress>();
        (1..5).filter(|i| !run.civ_campaign.cities[*i].owned()).collect()
    };
    if !retaken.is_empty() {
        println!("civ arc: cities {retaken:?} of the sieges were taken back by their rivals");
    }
    // A rival's hit may still land in the frame of the hand-over; the next
    // pass hands that city over again. After that frame no rival nation is
    // alive (their vehicles stand down), so nothing can change hands again.
    for _ in 0..5 {
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let run = &mut *run;
            for index in 1..crate::civ::CITY_COUNT {
                if !run.civ_campaign.cities[index].owned() {
                    run.civ_campaign
                        .take_city(index, crate::civ::Conquest::Military, &mut run.spice);
                }
            }
        }
        step(app);
        if unified(app) {
            break;
        }
    }
    let state_line = |app: &App| {
        let run = app.world().resource::<RunProgress>();
        let campaign = &run.civ_campaign;
        format!(
            "stage {:?}, {} of {} cities yours, unified {}, advanced {}, world held by the guide {:?}, message {:?}, nations {}",
            stage(app),
            campaign.owned_count(),
            campaign.cities.len(),
            campaign.unified,
            campaign.advanced,
            app.world()
                .get_resource::<crate::civ_hud::guide::GuideState>()
                .map(|g| g.holds_world()),
            campaign.message,
            civ_nations_line(run)
        )
    };
    assert!(
        unified(app),
        "every city must answer to C1 after the hand-over: {}",
        state_line(app)
    );
    // The unified planet shows the Advance button; the stage waits for it.
    assert_eq!(
        stage(app),
        GameStage::Civilization,
        "a unified planet waits for the Advance button"
    );
    press(app, crate::civ_hud::ADVANCE_KEY);
    step(app);
    assert_eq!(
        stage(app),
        GameStage::Space,
        "city unification must launch the lineage: {}",
        state_line(app)
    );
    println!("PASS Civilization -> Space through military, religion and trade");
}
/// Cell flow input hooks: dismiss callouts (Enter) and skip the intro
/// (Space) until the avatar is free.
fn cell_settle(app: &mut App, frames: usize) {
    use crate::cell_flow::{CellFlow, FlowPhase};
    for _ in 0..frames {
        let (phase, callouts) = {
            let flow = app.world().resource::<CellFlow>();
            (flow.phase, flow.callouts.len())
        };
        if callouts > 0 {
            press(app, crate::cell_flow::DISMISS_KEY);
        } else if phase == FlowPhase::Intro {
            press(app, crate::cell_flow::SKIP_KEY);
        } else if phase == FlowPhase::Play {
            break;
        } else {
            step(app);
        }
    }
}

fn check_full_arc(app: &mut App) {
    press(app, KeyCode::Enter);
    // Spore's new-game diet step (before the stage): start as a herbivore.
    press(app, KeyCode::Digit1);
    step(app);
    assert_eq!(stage(app), GameStage::Cell);
    // The new-stage intro (skipped) and its callouts.
    cell_settle(app, 900);
    // Retail progression: eat the installed populate's plant food until the
    // food progression reaches 1000 (the whole stage, ten player sizes).
    for _ in 0..12000 {
        if app.world().resource::<RunProgress>().cell.food_progression
            >= crate::cell_engine::STAGE_FOOD
        {
            break;
        }
        if !app.world().resource::<crate::cell_flow::CellFlow>().callouts.is_empty() {
            press(app, crate::cell_flow::DISMISS_KEY);
            continue;
        }
        let world = app.world_mut();
        let herbivore = {
            let originals = world.resource::<crate::Originals>();
            originals.part_catalog.parts.is_empty()
                || crate::editor::cell_abilities(world.resource::<RunProgress>(), originals)
                    .herbivore
                    > 0
        };
        let player_at = world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .map(|t| t.translation)
            .unwrap_or_default();
        let player_scale = crate::cell_engine::player_scale(
            world.resource::<RunProgress>().cell.food_progression,
        );
        // Retail contact table: a plant mouth swallows plants exactly one
        // level smaller.
        let target = world
            .query::<(&Transform, &crate::cell_engine::CellObject)>()
            .iter(world)
            .filter(|(_, o)| {
                !o.background
                    && !o.dead
                    && o.scale.index() == player_scale.index() - 1
                    && (o.kind == crate::cell_engine::CellKind::PlantFood && herbivore)
            })
            .min_by(|a, b| {
                a.0.translation
                    .distance_squared(player_at)
                    .total_cmp(&b.0.translation.distance_squared(player_at))
            })
            .map(|(t, _)| t.translation);
        // A mouth swallows by touching: with the installed part catalog the
        // avatar has part spheres (retail 11.6), so it is placed with its first
        // plant mouth (filter / both) on the plant instead of its centre.
        let mouth = {
            let spheres = crate::cell_motion::player_part_spheres(
                world.resource::<RunProgress>(),
                world.resource::<crate::Originals>(),
            );
            let (heading, radius) = world
                .query_filtered::<&crate::cell_engine::CellObject, With<Player>>()
                .single(world)
                .map(|o| (o.heading, o.radius))
                .unwrap_or_default();
            spheres
                .iter()
                .find(|s| matches!(s.kind, 3 | 4))
                .map(|s| Quat::from_rotation_y(heading) * Vec3::new(s.center.x, 0.0, s.center.y) * radius)
                .unwrap_or(Vec3::ZERO)
        };
        // Swim on to stream new tiles when nothing edible is in range.
        teleport(app, target.map(|t| t - mouth).unwrap_or(player_at + Vec3::X * 30.0));
        step(app);
    }
    let (food_now, objects_now, plants_now) = {
        let world = app.world_mut();
        let food = world.resource::<RunProgress>().cell.food_progression;
        let mut q = world.query::<&crate::cell_engine::CellObject>();
        let all: Vec<_> = q.iter(world).map(|o| o.kind).collect();
        let plants = all
            .iter()
            .filter(|k| **k == crate::cell_engine::CellKind::PlantFood)
            .count();
        (food, all.len(), plants)
    };
    assert_eq!(
        food_now,
        crate::cell_engine::STAGE_FOOD,
        "the installed Cell levels must feed the cell through all ten sizes \
         ({objects_now} cell objects, {plants_now} plant food, tiles {})",
        app.world().resource::<crate::cell_population::CellTiles>().generated
    );
    assert_eq!(
        stage(app),
        GameStage::Cell,
        "a full progress bar waits for the Advance button"
    );
    // The mate round trip: Mating Call, mating, egg, Cell Creator, hatch.
    // Food 1000: the ending cinematic, then the Ending callout (dismissed).
    for _ in 0..1200 {
        if app.world().resource::<crate::cell_flow::CellFlow>().ending_done {
            break;
        }
        step(app);
    }
    cell_settle(app, 600);
    assert!(
        app.world().resource::<crate::cell_flow::CellFlow>().ending_done,
        "food 1000 plays the ending and shows Advance"
    );
    // The button shows once its countdown (6 s after the first meal past
    // 149 food) has run out.
    for _ in 0..1200 {
        if app.world().resource::<crate::cell_contract::CellHudState>().show_mate_button {
            break;
        }
        cell_settle(app, 1);
        step(app);
    }
    // The Mating Call: the key is pressed whenever the button is available and no mate is
    // out, as a player does. The avatar is parked on the mate every frame below, among the other
    // cells, so it can die on the way (the eaters are random); the respawn (`FlowPhase::Dying`)
    // removes the mate and the call, and the button is back. A single press then left the
    // test waiting for a mate that never came.
    let deaths_before = app.world().resource::<RunProgress>().cell.death_count;
    let mut calls = 0;
    let mut opened = false;
    for _ in 0..4000 {
        let mate = {
            let world = app.world_mut();
            world
                .query_filtered::<&Transform, With<crate::cell::CellMate>>()
                .iter(world)
                .next()
                .map(|t| t.translation)
        };
        if let Some(mate) = mate {
            teleport(app, mate);
        } else {
            let callable = {
                let flow = app.world().resource::<crate::cell_flow::CellFlow>();
                let run = app.world().resource::<RunProgress>();
                crate::cell_flow::mate_available(run, flow) && flow.mate_called <= 0.0
            };
            if callable {
                calls += 1;
                press(app, crate::cell_flow::MATE_KEY);
                continue;
            }
        }
        step(app);
        if app.world().resource::<crate::editor::EditorState>().open {
            opened = true;
            break;
        }
    }
    let deaths = app.world().resource::<RunProgress>().cell.death_count - deaths_before;
    if calls > 1 || deaths > 0 {
        println!("cell mate check: the Mating Call was pressed {calls} times, the avatar died {deaths} times on the way");
    }
    if !opened {
        let flow = app.world().resource::<crate::cell_flow::CellFlow>();
        let run = app.world().resource::<RunProgress>();
        eprintln!(
            "cell mate check: phase {:?} mate {:?} called {:.1} countdown {:.1} show {} food {} calls {calls} deaths {deaths}",
            flow.phase, flow.mate, flow.mate_called, flow.mate_countdown, run.cell.show_mate_button, run.cell.food_progression
        );
    }
    assert!(opened, "calling a mate and mating opens the Cell Creator");
    press(app, KeyCode::F2);
    for _ in 0..600 {
        step(app);
        if app.world().resource::<crate::cell_flow::CellFlow>().phase == crate::cell_flow::FlowPhase::Play {
            break;
        }
    }
    assert_eq!(
        app.world().resource::<crate::cell_flow::CellFlow>().phase,
        crate::cell_flow::FlowPhase::Play,
        "the next generation hatches after the Cell Creator"
    );
    // Retail end of stage: Advance (0x06455C5E) -> summary window (closed with
    // Enter) -> MovToBeach -> Creature.
    cell_settle(app, 600);
    press(app, crate::cell_flow::ADVANCE_KEY);
    assert!(
        app.world().resource::<crate::cell_flow::CellFlow>().summary.is_some(),
        "Advance opens the summary window"
    );
    press(app, crate::cell_flow::DISMISS_KEY);
    for _ in 0..600 {
        if stage(app) != GameStage::Cell {
            break;
        }
        step(app);
    }
    assert_eq!(
        stage(app),
        GameStage::Creature,
        "cell food and the Advance button must evolve the player"
    );
    assert!(
        app.world().resource::<crate::editor::EditorState>().open,
        "landfall opens the Creature creator"
    );
    press(app, KeyCode::F2);
    step(app);
    assert_eq!(app.world().resource::<RunProgress>().dna_this_stage, 0.0);
    println!("PASS Cell -> Creature through eating, a mate round trip and Advance");
    let world = app.world_mut();
    let bushes: Vec<_> = world
        .query_filtered::<Entity, With<Bush>>()
        .iter(world)
        .collect();
    for bush in bushes {
        world.despawn(bush);
    }
    // Retail rewards shrink with the player's brain level (2 * brain - strength
    // indexes DNAPointsMultipliers) and the last brain level needs the stage
    // total 1000, so this headless world (every creature has strength 2) cannot
    // pay 1000 DNA by itself: start one DNA short of the last level and let the
    // combat below earn the rest (a kill always pays at least 1 DNA).
    app.world_mut().resource_mut::<RunProgress>().dna_this_stage =
        crate::creature_rules::STAGE_TOTAL_DNA - 1.0;
    // A carnivore can finish Creature through actual cooldown-gated combat.
    // Use world waypoints, but deliver all actions through the keyboard input.
    // The last brain level only shows Advance (Evolve); the stage ends when the
    // button is pressed.
    let creature_done = |app: &App| {
        stage(app) != GameStage::Creature
            || app.world().resource::<RunProgress>().creature_progression.brain_level
                >= crate::creature_progression::MAX_BRAIN_LEVEL
    };
    for _ in 0..crate::species::MAX_TOTAL_MEMBERS {
        if creature_done(app) {
            break;
        }
        let world = app.world_mut();
        let position = world
            .query::<(&Transform, &Npc, &crate::creature_actions::CreatureMind)>()
            .iter(world)
            .find(|(_, n, m)| n.hp > 0. && !m.defeated && !m.allied)
            .map(|(t, _, _)| t.translation)
            .expect("living encounter target");
        teleport(app, position);
        // The attack stance (the HUD's stance key), then F targets the nearest.
        press(app, crate::creature_controls::KEY_STANCE_COMBAT);
        press(app, KeyCode::KeyF);
        let id = app
            .world()
            .resource::<crate::creature_actions::Encounter>()
            .target
            .expect("combat target in range");
        for _ in 0..1800 {
            if creature_done(app) {
                break;
            }
            let world = app.world_mut();
            let position = world
                .query::<(&crate::persistence::WorldId, &Transform, &Npc)>()
                .iter(world)
                .find(|(target, _, n)| target.0 == id && n.hp > 0.)
                .map(|(_, t, _)| t.translation);
            let Some(position) = position else {
                break;
            };
            teleport(app, position);
            if app
                .world()
                .resource::<crate::creature_actions::Encounter>()
                .target
                != Some(id)
            {
                press(app, KeyCode::KeyF);
            } else if app
                .world()
                .resource::<crate::creature_actions::Encounter>()
                .cooldowns[0]
                <= 0.
            {
                press(app, KeyCode::Digit1);
            } else {
                step(app);
            }
        }
        step(app);
    }
    assert_eq!(
        stage(app),
        GameStage::Creature,
        "the last brain level waits for the Advance (Evolve) button"
    );
    assert!(
        app.world().resource::<RunProgress>().creature_progression.brain_level
            >= crate::creature_progression::MAX_BRAIN_LEVEL,
        "Creature combat must reach the last brain level"
    );
    // Advance: the CRG2TRG moment and the stage timeline need the HUD windows;
    // headless runs go straight through.
    press(app, crate::creature_flow::ADVANCE_KEY);
    for _ in 0..10 {
        if stage(app) != GameStage::Creature {
            break;
        }
        step(app);
    }
    assert_eq!(
        stage(app),
        GameStage::Tribal,
        "Creature combat and Evolve must evolve the lineage"
    );
    println!("PASS Creature -> Tribal through ranked combat and cooldowns");
    assert_eq!(stage(app), GameStage::Tribal);
    // Food funds the tribe; it cannot bypass the territorial objective. The
    // tribe is selected (the HUD's tribe header key; Q is a camera key) and Z
    // sends it gathering: members strip the bushes and carry each bundle to
    // the food mat on their own.
    press(app, crate::tribal_controls::KEY_SELECT_TRIBE);
    press(app, KeyCode::KeyZ);
    // Retail yields: a bundle is five fruit (one food each) and a bush holds
    // ten, so the stock grows slower than with the old 80-food bushes.
    let mut gather_frames = 0;
    for _ in 0..48000 {
        if app.world().resource::<RunProgress>().dna_this_stage >= 250. {
            break;
        }
        step(app);
        gather_frames += 1;
    }
    assert_eq!(
        stage(app),
        GameStage::Tribal,
        "food alone cannot settle rival villages"
    );
    let run_now = app.world().resource::<RunProgress>();
    println!(
        "Tribal gathering: {:.0} food after {:.0} s, {} members",
        run_now.dna_this_stage,
        gather_frames as f32 / 60.0,
        run_now.tribe.len()
    );
    assert!(run_now.dna_this_stage >= 250.);
    let campaign = |app: &App| {
        app.world()
            .resource::<RunProgress>()
            .tribal_campaign
            .clone()
    };
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
    ];
    // Start songs with E and answer their notes through the normal inputs.
    // Keep the player at the village across cooldown frames: combat and
    // respawn can otherwise move the chieftain back home between songs.
    let perform = |app: &mut App, index: usize| {
        let at_village = village_at(app, index);
        let mut frames_left = 3600usize;
        while frames_left > 0 && !campaign(app).villages[index].status.is_resolved() {
            while campaign(app).cooldown > 0.0 && frames_left > 0 {
                teleport(app, at_village);
                step(app);
                frames_left -= 1;
            }
            if frames_left < 2 {
                break;
            }

            // Re-select the full troupe so the perform key orders the actual
            // RTS units to the village even after gathering or another unit
            // task.
            teleport(app, at_village);
            press(app, crate::tribal_controls::KEY_SELECT_TRIBE);
            frames_left -= 1;
            teleport(app, at_village);
            press(app, crate::tribal_controls::KEY_PERFORM);
            frames_left -= 1;

            if campaign(app).performance.is_none() {
                // E still issues the travel order when a performance cannot
                // start yet (for example, a required instrument is en route).
                for _ in 0..60.min(frames_left) {
                    teleport(app, at_village);
                    step(app);
                    frames_left -= 1;
                }
                continue;
            }

            while let Some(p) = campaign(app).performance {
                if frames_left == 0 {
                    break;
                }
                teleport(app, at_village);
                // The instrument the village asked for, whenever it is ready (a
                // social ability recharges for 5 s).
                let ready = crate::tribal::instrument_kind(p.prompt)
                    .filter(|_| p.required > 0)
                    .is_some_and(|kind| p.refresh[kind] <= 0.0);
                if ready {
                    press(app, digits[(p.prompt - 1) as usize]);
                } else {
                    step(app);
                }
                frames_left -= 1;
            }
        }
    };
    // NPC tribes come into view over time (the retail controller 0xCC6A90: the
    // first 25 s after 40 food, the second once a tribe is settled or with
    // more than 100 food, the next two at once, the fifth after three settled
    // tribes); wait for each one.
    let wait_for_tribe = |app: &mut App, index: usize| {
        for _ in 0..(6 * 3600) {
            if campaign(app).village(index).is_some() {
                break;
            }
            step(app);
        }
        assert!(
            campaign(app).village(index).is_some(),
            "tribe V{} appears in time",
            index + 1
        );
    };
    // The horns first: T walks the palette to Wooden Horns, B builds the shack,
    // B again hands horns to the tribe, who fetch them from the shack.
    while app
        .world()
        .resource::<crate::tribal_rts::TribalRts>()
        .palette
        != crate::tribal_tools::ToolKind::WoodenHorns
    {
        press(app, KeyCode::KeyT);
    }
    press(app, KeyCode::KeyB);
    step(app);
    press(app, KeyCode::KeyB);
    for _ in 0..3000 {
        if !campaign(app).tools.is_empty() {
            break;
        }
        step(app);
    }
    assert!(
        !campaign(app).tools.is_empty(),
        "members fetch horns from the shack"
    );
    // V1: the social rhythm game (the village's watching members answer at once
    // and lift the tribe to Pleased; the bubbles ask for horns, which are the
    // only open instrument; the bar fills with whole beats). A small troupe
    // cannot fill the bar of a second song, so the village is not expected to
    // ally by song here: the siege below settles it when it does not.
    let v = village_at(app, 0);
    teleport(app, v);
    perform(app, 0);
    let v1 = campaign(app);
    assert!(
        v1.villages[0].status.is_resolved() || v1.villages[0].relations >= 30.0,
        "V1's audience answers the song; relations={}, round={}, cooldown={:.2}, performance={:?}, message={:?}, living_mates={}",
        v1.villages[0].relations,
        v1.round,
        v1.cooldown,
        v1.performance,
        v1.message,
        app.world().resource::<RunProgress>().tribe.len()
    );
    // V2: a food gift (10 food) calms an angry tribe to Cautious (0xDBC1F1: only
    // a Furious or Annoyed tribe reacts).
    wait_for_tribe(app, 1);
    let v = village_at(app, 1);
    teleport(app, v);
    for _ in 0..20 {
        if campaign(app).villages[1].relations >= 0.0 {
            break;
        }
        while campaign(app).cooldown > 0. {
            let v = village_at(app, 1);
            teleport(app, v);
            step(app);
        }
        let v = village_at(app, 1);
        teleport(app, v);
        press(app, KeyCode::KeyG);
    }
    assert!(
        campaign(app).villages[1].relations >= 0.0,
        "V2 receives a food gift: {}",
        campaign(app).message
    );
    let v = village_at(app, 1);
    teleport(app, v);
    perform(app, 1);
    // Retail members only fight with a weapon: before each siege the tribe
    // gathers, fills the hut (R at home, eggs grow 60 s) and arms the members
    // with torches (open after two settled tribes; a rack holds six).
    let prepare_war_party = |app: &mut App| {
        press(app, crate::tribal_controls::KEY_SELECT_TRIBE);
        press(app, KeyCode::KeyZ);
        for _ in 0..48000 {
            if app.world().resource::<RunProgress>().dna_this_stage >= 120. {
                break;
            }
            step(app);
        }
        let home = crate::tribal::home_position() + Vec3::new(0.0, 0.0, 3.0);
        // R patches a wounded chieftain up first, so allow extra presses.
        for _ in 0..3 * crate::tribal::MAX_MATES {
            let run_now = app.world().resource::<RunProgress>();
            let used = run_now
                .tribal_campaign
                .member_capacity_used(run_now.tribe.len());
            if used >= run_now.tribal_campaign.population_cap()
                || run_now.dna_this_stage < crate::tribal::RECRUIT_FOOD + 25.0
            {
                break;
            }
            while campaign(app).recruit_cooldown > 0.0 {
                teleport(app, home);
                step(app);
            }
            teleport(app, home);
            press(app, KeyCode::KeyR);
        }
        for _ in 0..((crate::tribal::BIRTH_GROWTH_SECONDS + 2.0) * 60.0) as usize {
            teleport(app, home);
            step(app);
        }
        while app
            .world()
            .resource::<crate::tribal_rts::TribalRts>()
            .palette
            != crate::tribal_tools::ToolKind::SpikedClubs
        {
            press(app, KeyCode::KeyT);
        }
        if !campaign(app).has_shack(crate::tribal_tools::ToolKind::SpikedClubs) {
            press(app, KeyCode::KeyB);
            step(app);
        }
        press(app, crate::tribal_controls::KEY_SELECT_TRIBE);
        press(app, KeyCode::KeyB);
        for _ in 0..1800 {
            teleport(app, home);
            step(app);
        }
        // tribal-rules3: retail members and NPC citizens have 100+ HP, so the
        // members the torch rack cannot arm take axes (tier 1, open by now).
        // Test shortcut: the harness cannot select part of the tribe, so the
        // axes are handed out directly once their rack is built.
        for _ in 0..16 {
            if app.world().resource::<crate::tribal_rts::TribalRts>().palette
                == crate::tribal_tools::ToolKind::StoneAxes
            {
                break;
            }
            press(app, KeyCode::KeyT);
        }
        if !campaign(app).has_shack(crate::tribal_tools::ToolKind::StoneAxes) {
            press(app, KeyCode::KeyB);
            step(app);
        }
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for slot in run.tribal_campaign.slots.clone() {
                let id = crate::tribal::MATE_ID_BASE + slot;
                if run.tribal_campaign.tool_of(id).is_none() {
                    let _ = run
                        .tribal_campaign
                        .equip(id, crate::tribal_tools::ToolKind::StoneAxes);
                }
            }
        }
        // Before the clubs and axes are open the tribe fights with spears (tier
        // 0): build their rack and arm every member that holds no weapon.
        for _ in 0..16 {
            if app.world().resource::<crate::tribal_rts::TribalRts>().palette
                == crate::tribal_tools::ToolKind::ThrowingSpears
            {
                break;
            }
            press(app, KeyCode::KeyT);
        }
        if !campaign(app).has_shack(crate::tribal_tools::ToolKind::ThrowingSpears) {
            press(app, KeyCode::KeyB);
            step(app);
        }
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for slot in run.tribal_campaign.slots.clone() {
                let id = crate::tribal::MATE_ID_BASE + slot;
                let armed = run.tribal_campaign.tool_of(id).is_some_and(|k| {
                    matches!(
                        k,
                        crate::tribal_tools::ToolKind::SpikedClubs
                            | crate::tribal_tools::ToolKind::StoneAxes
                            | crate::tribal_tools::ToolKind::ThrowingSpears
                    )
                });
                if !armed {
                    let _ = run
                        .tribal_campaign
                        .equip(id, crate::tribal_tools::ToolKind::ThrowingSpears);
                }
            }
        }
        println!(
            "Tribal war party: {} members, {} carry torches, {:.0} food",
            app.world().resource::<RunProgress>().tribe.len(),
            campaign(app)
                .tools
                .values()
                .filter(|k| **k == crate::tribal_tools::ToolKind::SpikedClubs)
                .count(),
            app.world().resource::<RunProgress>().dna_this_stage
        );
    };
    // Every village the songs did not settle: siege - F sends the tribe at the
    // walls. Racks (200) and huts (400 / 750 / 1000) take the weapons' raze
    // damage (a torch 9 every 1.5 s).
    for index in 0..5 {
        wait_for_tribe(app, index);
        if campaign(app).villages[index].status.is_resolved() {
            continue;
        }
        prepare_war_party(app);
        let v = village_at(app, index);
        teleport(app, v);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
        for _ in 0..60000 {
            if campaign(app).villages[index].status.is_resolved() {
                break;
            }
            let v = village_at(app, index);
            teleport(app, v);
            step(app);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyF);
        let v = campaign(app).villages[index].clone();
        assert!(
            v.status.is_resolved(),
            "rival village {index} must resolve through actual inputs: {} pieces, {:.0} damage, {} members",
            v.huts,
            v.hurt,
            app.world().resource::<RunProgress>().tribe.len()
        );
        step(app);
    }
    // The last tribe makes the Evolve button appear; the stage waits for it.
    assert_eq!(
        stage(app),
        GameStage::Tribal,
        "the united tribes wait for the Evolve button"
    );
    assert!(campaign(app).can_advance());
    press(app, crate::tribal_hud::ADVANCE_KEY);
    step(app);
    assert_eq!(stage(app), GameStage::Civilization);
    println!(
        "PASS Tribal -> Civilization through gathering, songs, gifts, tool shacks and conquest"
    );
    check_civilization(app);
    assert_eq!(stage(app), GameStage::Space);
    use crate::space_rules::{tool, TutorialStep};
    let tutorial = |app: &App| app.world().resource::<RunProgress>().space_campaign.tutorial;
    // Worlds orbit their star (the star sits at the origin), so every visit
    // looks up where the world is now.
    let world_at = |app: &mut App, id: u32| {
        let world = app.world_mut();
        world
            .query::<(&crate::persistence::WorldId, &Transform, &Planet)>()
            .iter(world)
            .find(|(w, _, _)| w.0 == id)
            .map(|(_, t, _)| t.translation)
            .expect("campaign world")
    };
    // Retail arrival (S1): a 100,000 Sporebuck wallet whatever Civ left, the
    // start tools, no drives yet, Flight 101 first.
    {
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.spice, crate::space_rules::STARTING_MONEY);
        assert!(run.space_campaign.has_tool(tool::SCAN));
        assert!(!run.space_campaign.has_tool(tool::INTERPLANETARY_DRIVE));
        assert_eq!(run.space_campaign.tutorial.step, TutorialStep::Flight101VisitCities);
        let world = app.world_mut();
        let worlds = world
            .query::<(&crate::persistence::WorldId, &Planet)>()
            .iter(world)
            .filter(|(id, _)| id.0 > 0)
            .count();
        // The campaign's own galaxy (generated from the save's seed), not the
        // default one.
        let galaxy = world.resource::<crate::space_galaxy::Galaxy>().clone();
        let expected = crate::space_galaxy::system_plan(
            &galaxy,
            crate::space_galaxy::HOME_STAR,
            &[],
        )
        .bodies
        .iter()
        .filter(|b| b.world.is_some())
        .count()
            - 1;
        assert_eq!(worlds, expected, "the home system keeps all its worlds");
    }
    // Flight 101 at the homeworld through real inputs: visit (M), scan (V),
    // abduct (U), then destroy five hostile ships with the laser (F).
    let home = world_at(app, 0);
    teleport(app, home);
    press(app, KeyCode::KeyM);
    assert_eq!(tutorial(app).step, TutorialStep::Flight101Scan);
    let home = world_at(app, 0);
    teleport(app, home);
    press(app, KeyCode::KeyV);
    assert_eq!(tutorial(app).step, TutorialStep::Flight101FetchAnimal);
    let home = world_at(app, 0);
    teleport(app, home);
    press(app, KeyCode::KeyU);
    assert_eq!(tutorial(app).step, TutorialStep::Flight101EradicateAnimals);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyF);
    for _ in 0..20000 {
        if tutorial(app).step != TutorialStep::Flight101EradicateAnimals {
            break;
        }
        // Focus fire on the most damaged ship (a broken one flees).
        let world = app.world_mut();
        let target = world
            .query::<(&Transform, &crate::space::Pirate)>()
            .iter(world)
            .filter(|(_, p)| p.hp > 0.)
            .min_by(|a, b| a.1.hp.total_cmp(&b.1.hp))
            .map(|(t, _)| t.translation + Vec3::new(0., 0., 3.));
        if let Some(position) = target {
            teleport(app, position);
        }
        // The laser spends energy; the comm screen's Recharge tab at the
        // homeworld (free, cost index 0) refills it: there is no hover top-up.
        if app.world().resource::<RunProgress>().space_campaign.energy < 1.0 {
            let home = world_at(app, 0);
            teleport(app, home);
            app.world_mut().resource_mut::<RunProgress>().space_campaign.repair_request =
                Some(crate::space_rules::PlanetOwner::Homeworld);
        }
        step(app);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyF);
    {
        let c = &app.world().resource::<RunProgress>().space_campaign;
        assert_eq!(
            tutorial(app).step,
            TutorialStep::Story201,
            "laser combat must finish Flight 101 (kills {}, progress {}, energy {}, lost {}, msg {})",
            c.kills,
            c.tutorial.progress,
            c.energy,
            c.ships_lost,
            c.message
        );
    }
    {
        let campaign = &app.world().resource::<RunProgress>().space_campaign;
        assert!(campaign.has_tool(tool::INTERPLANETARY_DRIVE));
        assert!(campaign.has_tool(tool::INTERSTELLAR_DRIVE));
    }

    // Story 201: jump to the closest star on the galaxy map and scan a world.
    step(app);
    let star = tutorial(app).story_star.expect("Story 201 picks the closest star");
    {
        let galaxy = app.world().resource::<crate::space_galaxy::Galaxy>();
        let reach = app.world().resource::<RunProgress>().space_campaign.max_jump_distance();
        assert!(
            galaxy.distance(crate::space_galaxy::HOME_STAR, star) <= reach,
            "the first drive reaches the closest star"
        );
    }
    {
        let mut view = app.world_mut().resource_mut::<crate::space_view::SpaceView>();
        view.galaxy = true;
        view.cursor = star;
    }
    press(app, KeyCode::Enter);
    for _ in 0..1200 {
        if app.world().resource::<RunProgress>().space_campaign.current_system == star
            && app.world().resource::<crate::space_view::SpaceView>().travel.is_none()
        {
            break;
        }
        step(app);
    }
    for _ in 0..5 {
        step(app);
    }
    assert_eq!(app.world().resource::<RunProgress>().space_campaign.current_system, star);
    let world = app.world_mut();
    let mut local: Vec<u32> = world
        .query::<(&crate::persistence::WorldId, &Planet)>()
        .iter(world)
        .map(|(id, _)| id.0)
        .filter(|id| *id != crate::space_galaxy::world_id(star, 0))
        .collect();
    local.sort_unstable();
    let colony = *local.first().expect("a world beside the star's first planet");
    let position = world_at(app, colony);
    teleport(app, position);
    press(app, KeyCode::KeyV);
    // Story 201 scans the wreck, then the ruin on another world (0xC602B0).
    assert_eq!(tutorial(app).step, TutorialStep::Story201);
    let ruin = local.get(1).copied().unwrap_or(crate::space_galaxy::world_id(star, 0));
    let position = world_at(app, ruin);
    teleport(app, position);
    press(app, KeyCode::KeyV);
    assert_eq!(tutorial(app).step, TutorialStep::TeachColonize);
    assert!(app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .has_tool(tool::SETI));

    // TutorialColonize hands over a Colony Incredi-Pak: colonize with it (+10,000).
    let wallet = app.world().resource::<RunProgress>().spice;
    let position = world_at(app, colony);
    teleport(app, position);
    press(app, KeyCode::KeyC);
    assert!(app
        .world()
        .resource::<RunProgress>()
        .space_campaign
        .colonies
        .contains(&colony));
    assert_eq!(app.world().resource::<RunProgress>().spice, wallet + 10_000.0);
    assert_eq!(tutorial(app).step, TutorialStep::TeachTerraform);

    // TutorialTerraform: raise the colony to T1 with the paid planetary tools.
    develop_ecology_through_controls(app, colony, 1);
    assert_eq!(tutorial(app).step, TutorialStep::TeachTradeSpice);
    assert_eq!(stage(app), GameStage::Space);
    let run = app.world().resource::<RunProgress>();
    assert!(!run.finished);
    assert_eq!(run.colonies, 1);
    assert_eq!(run.stage_order_index, 4);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<Player>>()
            .iter(app.world())
            .count(),
        1
    );
    println!("PASS Space: retail purse and tools, Flight 101 by laser combat, Story 201 jump, colony pack, terraform; empire remains playable");
}

/// Smoke/capture waypoints use ordinary planetary tool inputs. This never
/// grants ecology state, currency, or ownership as a substitute for actions.
fn develop_ecology_through_controls(app: &mut App, id: u32, tier: u8) {
    use crate::space::PlanetaryTool;
    let select = |app: &mut App, wanted: PlanetaryTool| {
        for _ in 0..PlanetaryTool::ALL.len() {
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
    };
    let apply = |app: &mut App| {
        let world = app.world_mut();
        let at = world
            .query::<(&crate::persistence::WorldId, &Transform, &Planet)>()
            .iter(world)
            .find(|(world_id, _, _)| world_id.0 == id)
            .expect("ecology world")
            .1
            .translation;
        teleport(app, at);
        press(app, KeyCode::KeyY);
    };
    // The climate moves as the planet's marble rolls (0xC7FC50): hold the ray the
    // planner picks (ten uses a second, `AutoFireRate` 0.1 s) for a second, then
    // let the marble roll for the minute a player would wait (`coast_marble` is
    // that physics without the frames). Energy comes back through the free
    // Recharge request of the homeworld's comm screen (the ship's place does not
    // matter to the request; the driver only skips the trip home).
    let position_of = |app: &mut App, wanted: u32| -> Vec3 {
        let world = app.world_mut();
        world
            .query::<(&crate::persistence::WorldId, &Transform, &Planet)>()
            .iter(world)
            .find(|(world_id, _, _)| world_id.0 == wanted)
            .expect("campaign world")
            .1
            .translation
    };
    for _ in 0..300 {
        if app.world().resource::<RunProgress>().space_campaign.terraform_goal_reached(id, tier) {
            break;
        }
        let plan = app
            .world_mut()
            .resource_mut::<RunProgress>()
            .space_campaign
            .plan_terraform_ray(id);
        if let Some((tool, seconds)) = plan {
            if app.world().resource::<RunProgress>().space_campaign.energy < 5.0 * seconds as f32 + 5.0 {
                app.world_mut().resource_mut::<RunProgress>().space_campaign.repair_request =
                    Some(crate::space_rules::PlanetOwner::Homeworld);
                step(app);
                step(app);
            }
            select(app, tool);
            let at = position_of(app, id);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
            for frame in 0..seconds * 60 {
                if frame % 10 == 0 {
                    teleport(app, at);
                }
                step(app);
                if frame % 60 == 59
                    && app.world().resource::<RunProgress>().space_campaign.terraform_goal_reached(id, tier)
                {
                    break;
                }
            }
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyY);
        }
        app.world_mut().resource_scope(|_, mut run: Mut<RunProgress>| {
            let mut campaign = std::mem::take(&mut run.space_campaign);
            campaign.coast_marble(id, 5.0, &mut run);
            run.space_campaign = campaign;
        });
    }
    assert!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(id)
            .t_score()
            >= tier,
        "the planner's rays must roll the marble into the requested habitat tier"
    );
    for (tool, count) in [
        (PlanetaryTool::SeedFlora, 3 * tier),
        (PlanetaryTool::SeedHerbivore, 2 * tier),
        (PlanetaryTool::SeedCarnivore, tier),
    ] {
        select(app, tool);
        for _ in 0..count {
            let state = app
                .world()
                .resource::<RunProgress>()
                .space_campaign
                .ecology_for(id);
            let occupied = match tool {
                PlanetaryTool::SeedFlora => state.flora,
                PlanetaryTool::SeedHerbivore => state.herbivores,
                _ => state.carnivores,
            };
            if occupied >= count {
                break;
            }
            apply(app);
        }
    }
    assert!(
        app.world()
            .resource::<RunProgress>()
            .space_campaign
            .ecology_for(id)
            .stable_foodweb_tier()
            >= tier,
        "flora, herbivore and carnivore inputs must establish a complete food web"
    );
}

fn installed_campaign_app(run: RunProgress) -> App {
    let install = crate::spore_install::discover().expect("original installation");
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
    assert!(
        catalog.parts.len() > 1000,
        "load the original part definitions"
    );
    let mut app = headless_app(run);
    let ui_art = crate::ui_art::load(&index, &mut app.world_mut().resource_mut::<Assets<Image>>());
    {
        let mut originals = app.world_mut().resource_mut::<crate::Originals>();
        originals.part_catalog = catalog;
        originals.ui_art = ui_art;
        originals.creature_abilities =
            crate::creature_actions::CreatureAbilityCatalog::from_index(&index);
        assert_eq!(
            originals.creature_abilities.abilities.len(),
            8,
            "the installed campaign must load all original ranked ability templates"
        );
        assert!(
            originals.creature_abilities.errors.is_empty(),
            "original ability catalog errors: {:?}",
            originals.creature_abilities.errors
        );
        originals.creation_catalog = crate::creation_records::CreationCatalog::from_index(&index);
        originals.resource_index = index;
    }
    app
}

pub fn run_smoke_test() {
    let original_assets = std::env::args().any(|a| a == "--original-assets");
    let mut app = if original_assets {
        installed_campaign_app(RunProgress::default())
    } else {
        headless_app(RunProgress::default())
    };
    check_full_arc(&mut app);
    if original_assets {
        assert!(app.world().resource::<crate::Originals>().models.len() >= 10);
        println!("PASS original part diets, abilities and imports through the full campaign");
    }
    println!("OpenSpore gameplay smoke test passed (no window, no user saves modified).");
}

/// Fast offscreen preview of selected stages for visual iteration. Uses the
/// same startup and asset provenance as `--render-smoke`, without its campaign
/// assertions. Usage: `--quick-capture <dir> [stage...] [--frames N]
/// [--hold KEY]`, where stages are menu/cell/creature/tribal/civilization/space
/// and KEY is one of w/a/s/d (held for the whole warm-up). Never touches saves.
pub fn run_quick_capture(output: &Path) {
    use bevy::{
        camera::RenderTarget, render::RenderPlugin, window::ExitCondition, winit::WinitPlugin,
    };
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let frames: usize = flag("--frames").and_then(|f| f.parse().ok()).unwrap_or(120);
    let hold = flag("--hold").map(|k| match k.as_str() {
        "w" => KeyCode::KeyW,
        "a" => KeyCode::KeyA,
        "s" => KeyCode::KeyS,
        _ => KeyCode::KeyD,
    });
    // `--press KEY@FRAME` (repeatable; enter, space, escape, f2, f4, w/a/s/d,
    // and the Creature camera and control keys: f, tab, r, q, e, z, k, l, p,
    // comma, period, slash, equal, minus, pageup, pagedown, shift, ctrl, 1-4)
    // taps a key (a keyboard message pressed on that frame, released on the
    // next) during the warm-up of the stage. `KEY@FIRST-LAST` holds it from
    // frame FIRST until frame LAST.
    let presses: Vec<(KeyCode, usize, usize)> = args
        .windows(2)
        .filter(|w| w[0] == "--press")
        .filter_map(|w| {
            let (key, at) = w[1].split_once('@')?;
            let code = match key {
                "enter" => KeyCode::Enter,
                "space" => KeyCode::Space,
                "escape" => KeyCode::Escape,
                "f2" => KeyCode::F2,
                "f3" => KeyCode::F3,
                "f4" => KeyCode::F4,
                "bracketleft" => KeyCode::BracketLeft,
                "bracketright" => KeyCode::BracketRight,
                "v" => KeyCode::KeyV,
                "w" => KeyCode::KeyW,
                "a" => KeyCode::KeyA,
                "s" => KeyCode::KeyS,
                "d" => KeyCode::KeyD,
                "f" => KeyCode::KeyF,
                "tab" => KeyCode::Tab,
                "r" => KeyCode::KeyR,
                "q" => KeyCode::KeyQ,
                "e" => KeyCode::KeyE,
                "up" => KeyCode::ArrowUp,
                "down" => KeyCode::ArrowDown,
                "left" => KeyCode::ArrowLeft,
                "right" => KeyCode::ArrowRight,
                // The Tribal HUD's virtual keys (select tribe, stances, perform).
                "f15" => crate::tribal_controls::KEY_SELECT_TRIBE,
                "f16" => crate::tribal_controls::KEY_STANCE_SOCIAL,
                "f17" => crate::tribal_controls::KEY_STANCE_ATTACK,
                "f18" => crate::tribal_controls::KEY_PERFORM,
                "z" => KeyCode::KeyZ,
                "k" => KeyCode::KeyK,
                "l" => KeyCode::KeyL,
                "p" => KeyCode::KeyP,
                "comma" => KeyCode::Comma,
                "period" => KeyCode::Period,
                "slash" => KeyCode::Slash,
                "equal" => KeyCode::Equal,
                "minus" => KeyCode::Minus,
                "pageup" => KeyCode::PageUp,
                "pagedown" => KeyCode::PageDown,
                "shift" => KeyCode::ShiftLeft,
                "ctrl" => KeyCode::ControlLeft,
                "backspace" => KeyCode::Backspace,
                "delete" => KeyCode::Delete,
                "1" => KeyCode::Digit1,
                "2" => KeyCode::Digit2,
                "3" => KeyCode::Digit3,
                "4" => KeyCode::Digit4,
                "5" => KeyCode::Digit5,
                "6" => KeyCode::Digit6,
                "7" => KeyCode::Digit7,
                "8" => KeyCode::Digit8,
                "9" => KeyCode::Digit9,
                "0" => KeyCode::Digit0,
                "g" => KeyCode::KeyG,
                "x" => KeyCode::KeyX,
                // The Space camera keys (End / Home discrete zoom, F6 the control scheme).
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                "f6" => KeyCode::F6,
                _ => return None,
            };
            let (first, last) = match at.split_once('-') {
                Some((first, last)) => (first.parse().ok()?, last.parse().ok()?),
                None => {
                    let first: usize = at.parse().ok()?;
                    (first, first + 1)
                }
            };
            Some((code, first, last))
        })
        .collect();
    // `--wheel N@FRAME` (repeatable) scrolls the mouse wheel by N notches
    // (positive zooms in) on that frame of the warm-up.
    let wheels: Vec<(f32, usize)> = args
        .windows(2)
        .filter(|w| w[0] == "--wheel")
        .filter_map(|w| {
            let (notches, at) = w[1].split_once('@')?;
            Some((notches.parse().ok()?, at.parse().ok()?))
        })
        .collect();
    let known = [
        "menu",
        "cell",
        "creature",
        "tribal",
        "civilization",
        "space",
    ];
    let mut stages: Vec<&str> = args
        .iter()
        .filter_map(|a| known.iter().find(|k| **k == a.as_str()).copied())
        .collect();
    if stages.is_empty() {
        stages = known.to_vec();
    }
    std::fs::create_dir_all(output).expect("create capture directory");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                render_creation: crate::renderer_settings().into(),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    crate::configure_game(&mut app, RunProgress::default());
    // `--space-view planet|system|galaxy` picks the Space zoom level shown.
    if let Some(view) = flag("--space-view")
        .as_deref()
        .and_then(crate::space_view::SpaceViewRequest::parse)
    {
        app.insert_resource(view);
    }
    app.add_plugins((
        crate::water::WaterPlugin,
        crate::terrain::TerrainPlugin,
        crate::world_presentation::PreviewSkyPlugin,
        crate::spui::surface::SpuiSurfacePlugin,
    ));
    app.world_mut()
        .resource_mut::<crate::persistence::SaveState>()
        .enabled = false;
    crate::add_startup(&mut app);
    app.finish();
    app.cleanup();
    app.update();
    gpu_flush(&app);
    // `--size WxH` renders at another resolution (default 1280x900).
    let (capture_w, capture_h) = flag("--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1280, 900));
    let image = Image::new_target_texture(
        capture_w,
        capture_h,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let world = app.world_mut();
    let camera = world
        .query_filtered::<Entity, With<crate::GameCamera>>()
        .single(world)
        .expect("camera");
    world
        .entity_mut(camera)
        .insert(RenderTarget::from(target.clone()));
    let input_window = world
        .spawn(Window {
            resolution: (capture_w, capture_h).into(),
            ..default()
        })
        .id();
    for name in stages {
        let stage = match name {
            "cell" => GameStage::Cell,
            "creature" => GameStage::Creature,
            "tribal" => GameStage::Tribal,
            "civilization" => GameStage::Civilization,
            "space" => GameStage::Space,
            _ => GameStage::MainMenu,
        };
        if stage != GameStage::MainMenu {
            let intro = args.iter().any(|a| a == "--intro");
            app.world_mut()
                .resource_mut::<RunProgress>()
                .cell_diet_chosen = !intro;
            app.world_mut()
                .resource_mut::<RunProgress>()
                .reset_to(stage.clone());
            // `--system N` starts the Space stage at star N of the galaxy.
            if let Some(system) = flag("--system").and_then(|f| f.parse::<u32>().ok()) {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                run.space_campaign.current_system = system;
                run.space_campaign.galaxy_version = crate::space::GALAXY_VERSION;
            }
            // `--cell-food N` starts the Cell stage at that food progression.
            if let Some(food) = flag("--cell-food").and_then(|f| f.parse::<i32>().ok()) {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                run.cell.food_progression = food.clamp(0, crate::cell_engine::STAGE_FOOD);
                run.cell.migrated = true;
            }
            // `--cell-mate` shows the mating-call button (F2) from the start.
            if args.iter().any(|a| a == "--cell-mate") {
                app.world_mut().resource_mut::<RunProgress>().cell.show_mate_button = true;
            }
            // `--cell-seed N` starts the Cell stage with that game id (the
            // seed of the population and the effects), to compare layouts.
            if let Some(seed) = flag("--cell-seed").and_then(|f| f.parse::<u32>().ok()) {
                app.world_mut().resource_mut::<RunProgress>().cell.game_id = seed;
            }
            app.world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(stage);
        }
        // `--anim-code N` / `--anim-row N` hold one Cell request code or one
        // definition-table row on every cell (cell_anim capture strips).
        if name == "cell" {
            app.world_mut()
                .insert_resource(crate::cell_anim::CellAnimOverride {
                    code: flag("--anim-code").and_then(|v| v.parse().ok()),
                    row: flag("--anim-row").and_then(|v| v.parse().ok()),
                });
        }
        let scenario = flag("--scenario");
        for i in 0..frames {
            // `--fx NAME [--fx-at N] [--fx-snaps a,b] [--fx-world-scale W]`
            // fires an installed effect on the player (cell_fx.rs).
            if name == "cell" {
                crate::cell_fx::capture_fx_step(&mut app, &args, i, frames, output, &target);
            } else {
                // `--swarm-fx NAME [--swarm-fx-at N]` (swarm_fx/volume.rs).
                crate::swarm_fx::volume::capture_fx_step(&mut app, &args, i, frames);
            }
            // Stage a Civilization scene once the stage has spawned.
            if i == 4 && name == "civilization" {
                if let Some(scenario) = scenario.as_deref() {
                    crate::civ_rts::quick_capture_scenario(app.world_mut(), scenario);
                }
            }
            // `--cell-food-jump N` sets the food progression half way through
            // (a mid-run level change instead of a level start).
            if name == "cell" && i == frames / 2 {
                if let Some(food) = flag("--cell-food-jump").and_then(|f| f.parse::<i32>().ok()) {
                    app.world_mut().resource_mut::<RunProgress>().cell.food_progression =
                        food.clamp(0, crate::cell_engine::STAGE_FOOD);
                }
            }
            // `--cell-part-guide N` marks the part guarantee (the Pokey scene, RE 10A) as due at
            // frame N, as `add_food` does at a meal between 120 and 150 food.
            if name == "cell" && flag("--cell-part-guide").and_then(|f| f.parse::<usize>().ok()) == Some(i) {
                app.world_mut().resource_mut::<RunProgress>().cell.part_guide_pending = true;
            }
            // UI roots spawned on stage entry need the offscreen camera.
            if i < 3 || i % 30 == 0 {
                let world = app.world_mut();
                let roots: Vec<_> = world
                    .query_filtered::<Entity, (With<Node>, Without<UiTargetCamera>, Without<ChildOf>)>()
                    .iter(world)
                    .collect();
                for entity in roots {
                    world.entity_mut(entity).insert(UiTargetCamera(camera));
                }
            }
            if let Some(key) = hold {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key);
            }
            for (key, at, until) in &presses {
                // A real keyboard message, so that the key is "just pressed"
                // for the game systems of the next update.
                if i == *at || i == *until {
                    app.world_mut()
                        .resource_mut::<Messages<bevy::input::keyboard::KeyboardInput>>()
                        .write(bevy::input::keyboard::KeyboardInput {
                            key_code: *key,
                            logical_key: bevy::input::keyboard::Key::Unidentified(
                                bevy::input::keyboard::NativeKey::Unidentified,
                            ),
                            state: if i == *at {
                                bevy::input::ButtonState::Pressed
                            } else {
                                bevy::input::ButtonState::Released
                            },
                            text: None,
                            repeat: false,
                            window: input_window,
                        });
                }
            }
            for (notches, at) in &wheels {
                if i == *at {
                    app.world_mut()
                        .resource_mut::<Messages<bevy::input::mouse::MouseWheel>>()
                        .write(bevy::input::mouse::MouseWheel {
                            unit: bevy::input::mouse::MouseScrollUnit::Line,
                            x: 0.0,
                            y: *notches,
                            phase: bevy::input::touch::TouchPhase::Moved,
                            window: input_window,
                        });
                }
            }
            let frame_started = std::time::Instant::now();
            app.update();
            gpu_flush(&app);
            if std::env::var_os("OPENSPORE_FRAME_TRACE").is_some() {
                eprintln!("capture frame {i} took {:?}", frame_started.elapsed());
            }
        }
        if let Some(key) = hold {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key);
        }
        // `--cinematic NAME`: play a cinematic and capture its frames.
        crate::cinematics::quick_capture(&mut app, &args, output, &target);
        // `--look-demo player|npc`: close-up look-at / morph frames (creature_look).
        crate::creature_look::quick_capture_demo(&mut app, &args, output, &target, name);
        // `--morph-demo CLIP`: close-ups of the own creature's part morphs (creature_morph).
        crate::creature_morph::quick_capture_demo(&mut app, &args, output, &target, name);
        // `--encounter social|combat|epic` stages a Creature encounter so the
        // world-anchored cues (action bubble, friendship/health bars, giant
        // scale) can be captured. It edits world state, not input.
        if let Some(kind) = flag("--encounter").filter(|_| name == "creature") {
            stage_capture_encounter(&mut app, &kind);
            for _ in 0..90 {
                app.update();
                gpu_flush(&app);
            }
        }
        if let Some(scenario) = flag("--scenario") {
            let handled = gameplay_capture::stage(&mut app, input_window, name, &scenario, &target, output);
            if !handled && name == "space" {
                stage_capture_space_art(&mut app, &scenario);
            }
            for _ in 0..12 {
                app.update();
                gpu_flush(&app);
            }
        }
        let editor = args.iter().any(|a| a == "--editor");
        // `--rigid [--rigid-class N] [--rigid-demo] [--rigid-select]`: the
        // vehicle / building / UFO editor of the stage (F3), with `N` class
        // changes (`]`), the scripted demo design and the last block selected.
        let rigid_capture = args.iter().any(|a| a == "--rigid") && matches!(name, "civilization" | "space");
        if rigid_capture {
            render_key(&mut app, input_window, KeyCode::F3, "F3");
            for _ in 0..30 {
                app.update();
                gpu_flush(&app);
            }
            let classes: usize = flag("--rigid-class").and_then(|c| c.parse().ok()).unwrap_or(0);
            for _ in 0..classes {
                render_key(&mut app, input_window, KeyCode::BracketRight, "]");
                for _ in 0..30 {
                    app.update();
                    gpu_flush(&app);
                }
            }
            if args.iter().any(|a| a == "--rigid-demo") {
                crate::rigid_editor::capture_demo(app.world_mut());
                for _ in 0..90 {
                    app.update();
                    gpu_flush(&app);
                }
                if args.iter().any(|a| a == "--rigid-paint") {
                    // `--rigid-paint`: Paint mode with the second complete style.
                    crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Paint);
                    crate::rigid_paint::demo_theme(app.world_mut(), 1);
                    for _ in 0..30 {
                        app.update();
                        gpu_flush(&app);
                    }
                    let (painted, total) = crate::rigid_paint::painted_surfaces(app.world_mut());
                    info!("rigid paint capture: {painted} of {total} block surfaces painted");
                }
                if args.iter().any(|a| a == "--rigid-drive") {
                    // `--rigid-drive`: Test Drive, driving on a circle.
                    app.world_mut().resource_mut::<crate::rigid_stats::TestDrive>().auto = true;
                    crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Play);
                }
                // `--rigid-select [N]`: block N (default the last) is selected, with its handles.
                if let Some(i) = args.iter().position(|a| a == "--rigid-select") {
                    let last = app.world().resource::<RunProgress>().parts.len().saturating_sub(1);
                    let pick = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()).map_or(last, |n| n.min(last));
                    app.world_mut()
                        .resource_mut::<crate::rigid_editor::RigidEditor>()
                        .script
                        .push_back(crate::rigid_editor_edit::Step::Select(Some(pick)));
                }
            }
            for _ in 0..60 {
                app.update();
                gpu_flush(&app);
            }
            // `--rigid-accept`: Enter accepts the demo design (the record the City
            // Planner's swatches show; see `--planner`).
            if args.iter().any(|a| a == "--rigid-accept") {
                render_key(&mut app, input_window, KeyCode::Enter, "Enter");
                for _ in 0..40 {
                    app.update();
                    gpu_flush(&app);
                }
            }
        }
        // `--planner [buildings|vehicles]`: the City Planner over the capital, on
        // that page (after `--rigid --rigid-demo --rigid-accept` the swatch of
        // the designed class shows the design's thumbnail).
        if name == "civilization" {
            if let Some(i) = args.iter().position(|a| a == "--planner") {
                let page = match args.get(i + 1).map(String::as_str) {
                    Some("vehicles") => crate::civ_planner::PlannerPage::Vehicles,
                    _ => crate::civ_planner::PlannerPage::Buildings,
                };
                {
                    let mut planner = app.world_mut().resource_mut::<crate::civ_planner::CivPlanner>();
                    planner.request = Some(crate::civ::HOME_INDEX);
                    planner.page = page;
                }
                // The palette is Bevy UI: its roots take the capture camera.
                for _ in 0..150 {
                    let world = app.world_mut();
                    let roots: Vec<_> = world
                        .query_filtered::<Entity, (With<Node>, Without<UiTargetCamera>, Without<ChildOf>)>()
                        .iter(world)
                        .collect();
                    for entity in roots {
                        world.entity_mut(entity).insert(UiTargetCamera(camera));
                    }
                    app.update();
                    gpu_flush(&app);
                }
                let run = app.world().resource::<RunProgress>();
                info!(
                    "planner capture: designs {:?}",
                    run.rigid.designs.iter().map(|(k, d)| (k.clone(), d.parts.len())).collect::<Vec<_>>()
                );
            }
        }
        let mechanical_editor = editor && !rigid_capture && matches!(name, "civilization" | "space");
        if mechanical_editor {
            let before = {
                let run = app.world().resource::<RunProgress>();
                (
                    run.vehicle.clone(),
                    run.building.clone(),
                    run.ship.clone(),
                    run.spice,
                )
            };
            // The older assembly modal opens with Shift+F3 (F3 is the rigid editor).
            render_shifted_key(&mut app, input_window, KeyCode::F3, "F3");
            if name == "civilization" && args.iter().any(|a| a == "--editor-building") {
                render_key(&mut app, input_window, KeyCode::KeyV, "v");
            }
            for _ in 0..90 {
                app.update();
                gpu_flush(&app);
            }
            assert_mechanical_preview(&mut app);
            let run = app.world().resource::<RunProgress>();
            assert_eq!(
                (&run.vehicle, &run.building, &run.ship, run.spice),
                (&before.0, &before.1, &before.2, before.3),
                "opening/browsing a creator must preserve recipes and economy"
            );
        } else if editor {
            app.world_mut()
                .resource_mut::<crate::editor::EditorState>()
                .show();
            // Optional creator page for captures: `--editor-category N` and
            // `--editor-body`, pressed through the palette's own actions.
            app.update();
            let press = |app: &mut App, action: crate::editor::Action| {
                let entity = app.world_mut().spawn((action, bevy::ui::Pressed)).id();
                app.update();
                app.world_mut().despawn(entity);
            };
            if let Some(category) = flag("--editor-category").and_then(|c| c.parse().ok()) {
                press(&mut app, crate::editor::Action::Category(category));
            }
            if args.iter().any(|a| a == "--editor-body") {
                press(&mut app, crate::editor::Action::Body);
            }
            for _ in 0..90 {
                let world = app.world_mut();
                let roots: Vec<_> = world
                    .query_filtered::<Entity, (With<Node>, Without<UiTargetCamera>, Without<ChildOf>)>()
                    .iter(world)
                    .collect();
                for entity in roots {
                    world.entity_mut(entity).insert(UiTargetCamera(camera));
                }
                app.update();
                gpu_flush(&app);
            }
        }
        let path = output.join(crate::spui::view::capture_file_name(format!(
            "{name}{}.png",
            if editor { "-editor" } else { "" }
        )));
        capture_render(&mut app, &target, &path);
        // `--anim-strip N [--anim-step K]`: N more Cell frames, K updates apart.
        if let Some(count) = flag("--anim-strip").and_then(|v| v.parse::<usize>().ok()) {
            let step: usize = flag("--anim-step").and_then(|v| v.parse().ok()).unwrap_or(6);
            for i in 0..count.min(64) {
                for _ in 0..step {
                    app.update();
                    gpu_flush(&app);
                }
                capture_render(&mut app, &target, &output.join(format!("{name}-anim-{i:02}.png")));
            }
        }
        if name == "creature" && flag("--scenario").as_deref() == Some("mobility") {
            gameplay_capture::release_mobility_inputs(&mut app, input_window);
        }
        if mechanical_editor {
            exercise_mechanical_edits(&mut app, input_window, &target, output, name);
            // Exercise real controls while paused, then ensure the world
            // camera and gameplay return without a second pause panel.
            let world = app.world_mut();
            let player_before = world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap()
                .translation;
            let spice_before = world.resource::<RunProgress>().spice;
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            for _ in 0..150 {
                app.update();
                gpu_flush(&app);
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::KeyW);
            let world = app.world_mut();
            assert_eq!(
                world
                    .query_filtered::<&Transform, With<Player>>()
                    .single(world)
                    .unwrap()
                    .translation,
                player_before
            );
            assert_eq!(world.resource::<RunProgress>().spice, spice_before);
            render_key(&mut app, input_window, KeyCode::Escape, "Escape");
            let world = app.world_mut();
            assert!(
                !world
                    .resource::<crate::mechanical_creator::MechanicalCreator>()
                    .open
            );
            assert!(!world.resource::<crate::pause_menu::PauseMenu>().open);
            assert!(
                world
                    .query_filtered::<&Camera, With<crate::GameCamera>>()
                    .single(world)
                    .unwrap()
                    .is_active
            );
            assert_eq!(
                world
                    .query_filtered::<Entity, With<crate::mechanical_preview::PreviewDraw>>()
                    .iter(world)
                    .count(),
                0
            );
            assert_eq!(world.query_filtered::<Entity, With<crate::mechanical_environment::MechanicalEnvironmentOwned>>()
                .iter(world).count(), 0, "closing must remove the original studio as well as the preview subject");
            println!("PASS mechanical creator source identity, recipe conservation, pause and camera restoration");
        }
        println!("captured {}", path.display());
        app.world_mut().remove_resource::<RenderSmokeCamera>();
    }
}

/// Readable close-ups of real scene entities. Only this capture harness uses
/// the framing override; normal play keeps its own zoom/follow camera.
fn stage_capture_space_art(app: &mut App, scenario: &str) {
    let world = app.world_mut();
    let camera = match scenario {
        "gas-giant" => {
            let Some((at, radius)) = world
                .query::<(&Transform, &crate::space::GasGiant)>()
                .iter(world)
                .next()
                .map(|(t, g)| (t.translation, g.radius))
            else {
                panic!("gas giant source");
            };
            Transform::from_translation(at + Vec3::new(2.2, 1.05, -3.2) * radius)
                .looking_at(at, Vec3::Y)
        }
        "asteroid-belt" => {
            let (mesh, global) = world
                .query::<(
                    &crate::asteroid_visuals::NativeAsteroidBelt,
                    &Mesh3d,
                    &GlobalTransform,
                )>()
                .iter(world)
                .next()
                .map(|(_, m, t)| (m.0.clone(), *t))
                .expect("native asteroid batch");
            let mesh = world.resource::<Assets<Mesh>>().get(&mesh).unwrap();
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("native asteroid positions");
            };
            let center = vertices[..4]
                .iter()
                .map(|p| Vec3::from_array(*p))
                .sum::<Vec3>()
                / 4.;
            let at = global.transform_point(center);
            Transform::from_translation(at + Vec3::new(0.5, 1.7, -4.0)).looking_at(at, Vec3::Y)
        }
        "pirates" => {
            let at = world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap()
                .translation;
            let mut pirates = world.query_filtered::<&mut Transform, With<crate::space::Pirate>>();
            let Some(mut pirate) = pirates.iter_mut(world).next() else {
                panic!("pirate source");
            };
            pirate.translation = at + Vec3::new(3., 0., -0.5);
            Transform::from_translation(at + Vec3::new(7., 5., -9.))
                .looking_at(at + Vec3::X, Vec3::Y)
        }
        _ => return,
    };
    world.insert_resource(RenderSmokeCamera(camera));
}

/// Drive the same controls as a player, including mounting and refreshing the
/// original assembly while its simulation is suspended. Capture the actual
/// edited design before removing the test part; disk saves remain disabled.
fn exercise_mechanical_edits(
    app: &mut App,
    window: Entity,
    target: &Handle<Image>,
    output: &Path,
    name: &str,
) {
    use crate::mechanical_creator::CreatorKind;
    type Part = (u32, u32, f32, f32, [f32; 3]);
    let kind = app
        .world()
        .resource::<crate::mechanical_creator::MechanicalCreator>()
        .kind;
    let parts = |app: &App| -> Vec<Part> {
        let run = app.world().resource::<RunProgress>();
        match kind {
            CreatorKind::Ship => run
                .ship
                .modules
                .iter()
                .map(|p| (p.instance, p.slot, p.yaw, p.scale, p.offset))
                .collect(),
            CreatorKind::Vehicle | CreatorKind::Building => {
                let build = if kind == CreatorKind::Vehicle {
                    &run.vehicle
                } else {
                    &run.building
                };
                build
                    .parts
                    .iter()
                    .map(|p| (p.instance, p.slot, p.yaw, p.scale, p.offset))
                    .collect()
            }
        }
    };
    let before = parts(app);
    for attempt in 0..8 {
        render_key(app, window, KeyCode::KeyN, "n");
        if parts(app).len() == before.len() + 1 {
            break;
        }
        assert_eq!(
            parts(app),
            before,
            "unavailable or duplicate models must preserve the design"
        );
        assert!(
            attempt < 7,
            "no usable original part in the first eight creator candidates"
        );
        render_key(app, window, KeyCode::BracketRight, "]");
    }
    assert_mechanical_preview(app);
    let added = parts(app);
    assert_eq!(added.len(), before.len() + 1);
    let preview_pose = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(&Mesh3d, &Transform), With<crate::mechanical_preview::PreviewDraw>>()
            .iter(world)
            .map(|(mesh, transform)| (mesh.0.clone(), *transform))
            .collect::<Vec<_>>()
    };
    let before_pose = preview_pose(app);
    for (key, text) in [
        (KeyCode::KeyR, "r"),
        (KeyCode::Equal, "="),
        (KeyCode::KeyL, "l"),
    ] {
        render_key(app, window, key, text);
    }
    let edited = parts(app);
    assert_eq!(
        &edited[..before.len()],
        before.as_slice(),
        "editing focus must preserve existing parts"
    );
    let old = added.last().unwrap();
    let new = edited.last().unwrap();
    assert_eq!((new.0, new.1), (old.0, old.1));
    assert_ne!(new.2, old.2, "rotate must update the saved part");
    assert!(new.3 > old.3, "scale must update the saved part");
    assert_ne!(new.4, old.4, "slide must update the saved part");
    assert_ne!(
        preview_pose(app),
        before_pose,
        "committed transforms must refresh the actual preview"
    );
    assert_mechanical_preview(app);
    for _ in 0..30 {
        app.update();
        gpu_flush(app);
    }
    capture_render(
        app,
        target,
        &output.join(format!("{name}-editor-edited.png")),
    );
    render_key(app, window, KeyCode::Backspace, "Backspace");
    assert_eq!(
        parts(app),
        before,
        "removing the added part must restore the original design"
    );
    assert_mechanical_preview(app);
    println!("PASS mechanical creator original-part add, rotate, scale, slide, preview refresh and removal");
}

fn assert_mechanical_preview(app: &mut App) {
    let world = app.world_mut();
    assert!(
        world
            .resource::<crate::mechanical_creator::MechanicalCreator>()
            .open
    );
    let draws = world
        .query::<(
            &crate::mechanical_preview::PreviewDraw,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(draw, mesh, mat)| (draw.source, mesh.0.clone(), mat.0.clone()))
        .collect::<Vec<_>>();
    assert!(
        !draws.is_empty(),
        "creator must show the actual native assembly"
    );
    for (source, mesh, material) in draws {
        assert_eq!(
            world.get::<Mesh3d>(source).unwrap().0,
            mesh,
            "preview shares source geometry"
        );
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(source)
                .unwrap()
                .0,
            material,
            "preview shares source paint/material"
        );
    }
    let main = world
        .query_filtered::<&Camera, With<crate::GameCamera>>()
        .single(world)
        .unwrap();
    assert!(
        !main.is_active,
        "live world camera must be suspended in creator"
    );
    let camera = world
        .query_filtered::<&Camera, With<crate::mechanical_preview::PreviewCamera>>()
        .single(world)
        .unwrap();
    assert!(camera.is_active);
    assert!(world.query_filtered::<Entity, With<crate::mechanical_environment::MechanicalEnvironmentSurface>>()
        .iter(world).next().is_some(), "original studio must be mounted in the mechanical creator");
    assert!(!world.resource::<crate::pause_menu::PauseMenu>().open);
}

/// Quick-capture helper: put the player in front of a creature with an
/// encounter already under way (no saves are touched).
fn stage_capture_encounter(app: &mut App, kind: &str) {
    use crate::creature_actions::{CreatureMind, Encounter, EncounterMode};
    let world = app.world_mut();
    let player = world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    let target = if kind == "epic" {
        world
            .query::<(
                &crate::persistence::WorldId,
                &Transform,
                &crate::epic_creatures::RoamerActor,
            )>()
            .iter(world)
            .min_by(|a, b| {
                dist_xz(a.1.translation, player).total_cmp(&dist_xz(b.1.translation, player))
            })
            .map(|(id, t, _)| (id.0, t.translation, 10.0))
    } else {
        world
            .query::<(
                &crate::persistence::WorldId,
                &Transform,
                &Npc,
                &CreatureMind,
            )>()
            .iter(world)
            .filter(|(_, _, npc, mind)| {
                !mind.allied && !mind.defeated && npc.hp > 0. && (kind != "social" || !npc.hostile)
            })
            .min_by(|a, b| {
                dist_xz(a.1.translation, player).total_cmp(&dist_xz(b.1.translation, player))
            })
            .map(|(id, t, _, _)| (id.0, t.translation, 3.2))
    };
    let Some((id, at, gap)) = target else {
        println!("no creature to stage a {kind} encounter with");
        return;
    };
    // Stand south of the creature, facing it, so the follow camera frames it.
    let stand = at + Vec3::new(0.0, 0.0, gap);
    {
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        if let Ok(mut t) = players.single_mut(world) {
            t.translation = Vec3::new(stand.x, t.translation.y, stand.z);
            t.rotation = Quat::from_rotation_y(0.0);
        }
    }
    let ranks = {
        let run = world.resource::<RunProgress>();
        let originals = world.resource::<crate::Originals>();
        crate::creature_actions::levels(
            crate::creature_actions::member_abilities(run, originals, id),
            EncounterMode::Social,
        )
    };
    let mut minds = world.query::<(&crate::persistence::WorldId, &mut CreatureMind)>();
    if let Some((_, mut mind)) = minds.iter_mut(world).find(|(w, _)| w.0 == id) {
        if kind == "social" {
            mind.impression = 45.;
            mind.beats = 1;
            crate::creature_actions::request(&mut mind, ranks);
        }
    }
    if kind != "social" {
        let mut npcs = world.query::<(&crate::persistence::WorldId, &mut Npc)>();
        if let Some((_, mut npc)) = npcs.iter_mut(world).find(|(w, _)| w.0 == id) {
            npc.hp *= 0.6;
            npc.hostile = true;
        }
        let mut roamers = world.query::<(&crate::persistence::WorldId, &mut Health)>();
        if let Some((_, mut health)) = roamers.iter_mut(world).find(|(w, _)| w.0 == id) {
            health.hp *= 0.7;
        }
    }
    let mut encounter = world.resource_mut::<Encounter>();
    encounter.target = Some(id);
    encounter.mode = if kind == "social" {
        EncounterMode::Social
    } else {
        EncounterMode::Combat
    };
    encounter.message = format!("Staged {kind} encounter for capture");
    println!("staged {kind} encounter with creature {id}");
}

/// Offscreen GPU verification with the same original-asset startup as the
/// desktop app. It writes local screenshots and never touches player saves.
#[derive(Resource)]
pub(crate) struct RenderSmokeCamera(pub Transform);

pub fn run_render_smoke(output: &Path) {
    use bevy::{
        camera::RenderTarget, render::RenderPlugin, window::ExitCondition, winit::WinitPlugin,
    };
    std::fs::create_dir_all(output).expect("create render output directory");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                render_creation: crate::renderer_settings().into(),
                ..default()
            })
            .disable::<WinitPlugin>()
            // Manual updates and GPU waits must finish rendering on this
            // thread, including tasks that otherwise need its executor.
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    crate::configure_game(&mut app, RunProgress::default());
    app.add_plugins((
        crate::water::WaterPlugin,
        crate::terrain::TerrainPlugin,
        crate::world_presentation::PreviewSkyPlugin,
        crate::spui::surface::SpuiSurfacePlugin,
    ));
    app.world_mut()
        .resource_mut::<crate::persistence::SaveState>()
        .enabled = false;
    crate::add_startup(&mut app);
    app.finish();
    app.cleanup();
    app.update();
    gpu_flush(&app);
    assert!(
        app.world().resource::<crate::InstallStatus>().ok,
        "render smoke requires original Spore installation"
    );
    let originals = app.world().resource::<crate::Originals>();
    assert_eq!(
        originals.creature_abilities.abilities.len(),
        8,
        "GPU scenes must load all original ranked ability templates"
    );
    assert!(
        originals.creature_abilities.errors.is_empty(),
        "original ability catalog errors: {:?}",
        originals.creature_abilities.errors
    );
    assert!(
        originals.native_painter.is_some(),
        "original SkinPaint tint shader must be active in GPU captures"
    );
    assert!(!originals.models.is_empty(), "original meshes must load");
    assert!(
        !originals.textures.is_empty(),
        "original textures must load"
    );
    assert!(
        originals.rigged_demo.is_some(),
        "original animated preview model must load"
    );
    let body = originals.rigged_demo.as_ref().unwrap();
    println!(
        "Original sea-monster preview: {:#X}, {} bones, scale {:.3}",
        body.instance, body.bones, body.scale
    );
    assert_eq!(
        body.instance, 0xC4F9_6704,
        "use the verified original sea-monster model"
    );
    let part = originals
        .part_catalog
        .parts
        .iter()
        .find(|p| {
            p.key.group_id == crate::asset_catalog::CREATURE_PARTS
                && p.abilities.sing > 0
                && p.price == 0
        })
        .expect("original starting mouth")
        .model
        .instance_id;
    // Creature entry constructs the starter species, including original
    // eyes/feet/mouth, rather than mounting onto the imported preview rig.
    let _ = part;
    let image = Image::new_target_texture(
        1280,
        900,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let world = app.world_mut();
    let camera = world
        .query_filtered::<Entity, With<crate::GameCamera>>()
        .single(world)
        .expect("camera");
    world
        .entity_mut(camera)
        .insert(RenderTarget::from(target.clone()));
    // A logical window supplies cursor/input coordinates without creating an
    // OS window: Winit is disabled and rendering targets the image above.
    let input_window = world
        .spawn(Window {
            resolution: (1280, 900).into(),
            ..default()
        })
        .id();
    // Offscreen cameras have no primary window for implicit UI targeting.
    let ui_roots: Vec<_> = world
        .query_filtered::<Entity, With<Node>>()
        .iter(world)
        .collect();
    for entity in ui_roots {
        world.entity_mut(entity).insert(UiTargetCamera(camera));
    }
    for (name, stage) in [
        ("menu", GameStage::MainMenu),
        ("cell", GameStage::Cell),
        ("creature", GameStage::Creature),
        ("tribal", GameStage::Tribal),
        ("civilization", GameStage::Civilization),
        ("space", GameStage::Space),
    ] {
        println!("Rendering {name}...");
        if stage != GameStage::MainMenu {
            app.world_mut()
                .resource_mut::<RunProgress>()
                .reset_to(stage.clone());
            app.world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(stage);
        }
        for _ in 0..90 {
            app.update();
            gpu_flush(&app);
        }
        if name == "cell" {
            // Complete the actual opening choice before exercising gameplay
            // and creator interactions in this renderer check.
            render_key(&mut app, input_window, KeyCode::Digit1, "1");
        }
        if name != "menu" {
            let world = app.world_mut();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<crate::RiggedMount>>()
                    .iter(world)
                    .count(),
                0,
                "menu animation attachments must not leak into {name}"
            );
        }
        if name == "cell" {
            let world = app.world_mut();
            // The adapted background layers stand in until the original effect
            // map environment (`cell_fx`) runs; then they are retired.
            let environment_loaded = {
                let maps = world.resource::<crate::cell_fx::CellFxMaps>();
                maps.loaded && !maps.effect_map.is_empty()
            };
            assert!(
                environment_loaded,
                "the original Cell effect map environment must be active"
            );
            assert_eq!(
                world
                    .query::<&crate::cell_environment::CellEnvironmentLayerSource>()
                    .iter(world)
                    .count(),
                0,
                "adapted Cell background layers must be retired once the original environment runs"
            );
            let assets = world.resource::<crate::cell_visuals::CellVisualAssets>();
            assert!(
                assets.species.len() >= 20,
                "native Cell species must actually import, not silently use fallback spheres"
            );
            assert!(
                assets.body.is_some(),
                "native starting Cell body must be active"
            );
            let fauna = world
                .query_filtered::<Entity, With<crate::cell_ai::CellAiState>>()
                .iter(world)
                .count();
            let native = world
                .query_filtered::<Entity, (
                    With<crate::cell_ai::CellAiState>,
                    With<crate::cell_visuals::OriginalCellVisual>,
                )>()
                .iter(world)
                .count();
            assert_eq!(
                native, fauna,
                "all live Cell NPCs require original model identities"
            );
        }
        if name == "civilization" || name == "space" {
            assert_creation_scene(&mut app);
        }
        if name == "civilization" {
            let world = app.world_mut();
            let nodes = world
                .query_filtered::<Entity, With<crate::game::SpiceNode>>()
                .iter(world)
                .collect::<Vec<_>>();
            assert!(!nodes.is_empty());
            for node in &nodes {
                assert_eq!(
                    world
                        .get::<crate::spice_presentation::NativeSpiceGeyser>(*node)
                        .unwrap()
                        .0,
                    crate::spice_presentation::GEYSER_SOURCE
                );
                assert!(
                    !world.entity(*node).contains::<Mesh3d>(),
                    "original stamp replaces the cylinder draw"
                );
            }
            let decals: Vec<_> = world
                .query::<(
                    &crate::spice_presentation::NativeSpiceGeyserDecal,
                    &ChildOf,
                    &MeshMaterial3d<StandardMaterial>,
                )>()
                .iter(world)
                .map(|(s, p, m)| (s.clone(), p.parent(), m.0.clone()))
                .collect();
            assert_eq!(decals.len(), nodes.len());
            for (source, parent, handle) in decals {
                assert!(nodes.contains(&parent));
                assert_eq!(source.source, crate::spice_presentation::GEYSER_DECAL);
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&handle)
                    .unwrap();
                assert_eq!(material.base_color_texture.as_ref(), Some(&source.image));
                let image = world
                    .resource::<Assets<Image>>()
                    .get(&source.image)
                    .unwrap();
                assert_eq!(source.dimensions, [image.width(), image.height()]);
            }
            let cities = world
                .query_filtered::<Entity, With<crate::civ::CityMarker>>()
                .iter(world)
                .collect::<Vec<_>>();
            assert!(!cities.is_empty());
            let native_buildings = world
                .query::<(
                    Option<&crate::city_presentation::NativeCityDistrictSurface>,
                    Option<&crate::civ::NativeCityHallSurface>,
                    &ChildOf,
                    &Mesh3d,
                    &MeshMaterial3d<StandardMaterial>,
                )>()
                .iter(world)
                .filter_map(|(district, hall, parent, mesh, mat)| {
                    let key = district.map(|d| d.0).or_else(|| hall.map(|h| h.0))?;
                    Some((key, parent.parent(), mesh.0.clone(), mat.0.clone()))
                })
                .collect::<Vec<_>>();
            for city in &cities {
                for key in crate::city_presentation::category_source_keys() {
                    let source_model = &world.resource::<crate::Originals>().baked_models[key];
                    let expected = if *key == crate::city_presentation::CITY_DISTRICT_HOUSE_SOURCE {
                        2
                    } else {
                        1
                    };
                    let draws: Vec<_> = native_buildings
                        .iter()
                        .filter(|(k, parent, ..)| k == key && parent == city)
                        .collect();
                    assert_eq!(draws.len(), expected * source_model.surfaces.len());
                    for (_, _, mesh, mat) in draws {
                        assert!(source_model
                            .surfaces
                            .iter()
                            .any(|s| s.handle == *mesh && s.material == *mat));
                        let material = world
                            .resource::<Assets<StandardMaterial>>()
                            .get(mat)
                            .unwrap();
                        let surface = source_model
                            .surfaces
                            .iter()
                            .find(|s| s.handle == *mesh && s.material == *mat)
                            .unwrap();
                        let raw = crate::baked_model::parse(
                            &world
                                .resource::<crate::Originals>()
                                .resource_index
                                .read(*key)
                                .unwrap(),
                        )
                        .unwrap();
                        let info = &raw.materials[surface.source_section];
                        if info.textures.iter().any(|t| t.sampler == 0) {
                            assert!(
                                material.base_color_texture.is_some(),
                                "native building diffuse binding missing"
                            );
                        } else if info.textures.is_empty() && info.shader_data.is_empty() {
                            // A window pane: the unlit window interior
                            // (material_binding::window_interior_material).
                            assert!(
                                material.unlit && material.base_color_texture.is_some(),
                                "window interior binding missing"
                            );
                        } else {
                            let c = crate::baked_model::preview_baked_paint_color(info)
                                .expect("native baked paint RGB");
                            assert_eq!(material.base_color, Color::linear_rgb(c[0], c[1], c[2]));
                        }
                    }
                }
            }
            for city in cities {
                let marker = *world.get::<crate::civ::CityMarker>(city).unwrap();
                let (kind, large) = {
                    let city = &world.resource::<RunProgress>().civ_campaign.cities[marker.0];
                    (city.kind, city.walls_large())
                };
                let (definition, _) = crate::city_presentation::wall_source(kind, large);
                let model = world
                    .resource::<crate::Originals>()
                    .world_models
                    .get(&definition)
                    .expect("installed original wall for each city style")
                    .clone();
                assert_eq!(model.source_definition, Some(definition));
                let presentation = *world
                    .get::<crate::city_presentation::CityWallPresentation>(city)
                    .expect("city has its authored wall style");
                assert_eq!(presentation.0.instance_id, model.instance);
                let draws = world
                    .query::<(
                        &crate::city_presentation::NativeCityWallSurface,
                        &ChildOf,
                        &Mesh3d,
                        &MeshMaterial3d<StandardMaterial>,
                        &crate::material_binding::OriginalSurfaceChild,
                    )>()
                    .iter(world)
                    .filter(|(_, parent, ..)| parent.parent() == city)
                    .map(|(source, _, mesh, mat, binding)| {
                        (
                            source.0,
                            mesh.0.clone(),
                            mat.0.clone(),
                            binding.source_section,
                            binding.state_sections.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    draws.len(),
                    model.surfaces.len(),
                    "one source-faithful perimeter on every city"
                );
                for (source_key, mesh, mat, section, state_sections) in draws {
                    assert_eq!(source_key, presentation.0);
                    let source_surface = model
                        .surfaces
                        .iter()
                        .find(|s| s.handle == mesh && s.material == mat)
                        .expect("wall retains original mesh and complete material binding");
                    assert_eq!(section, source_surface.source_section);
                    assert_eq!(state_sections, source_surface.state_sections);
                    assert!(world
                        .resource::<Assets<StandardMaterial>>()
                        .get(&mat)
                        .is_some());
                }
            }
        }
        if name == "space" {
            let world = app.world_mut();
            let planets = world
                .query::<(&crate::game::Planet, &crate::space_visuals::PlanetSurface)>()
                .iter(world)
                .count();
            let clouds: Vec<_> = world
                .query::<(
                    &crate::space_visuals::OriginalPlanetCloud,
                    &ChildOf,
                    &MeshMaterial3d<StandardMaterial>,
                )>()
                .iter(world)
                .map(|(s, p, m)| (s.clone(), p.parent(), m.0.clone()))
                .collect();
            assert!(planets > 0);
            assert_eq!(
                clouds.len(),
                planets,
                "all orbital planets have installed cloud shells"
            );
            for (source, parent, handle) in clouds {
                let planet = world
                    .get::<crate::space_visuals::PlanetSurface>(parent)
                    .unwrap();
                assert!(planet.terrain_texture.is_some());
                assert_eq!(planet.cloud_texture, Some(source.key));
                assert_eq!(source.key, crate::world_presentation::CLOUD_RASTER);
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&handle)
                    .unwrap();
                assert_eq!(material.base_color_texture.as_ref(), Some(&source.image));
                assert_eq!(material.alpha_mode, AlphaMode::Blend);
            }
            let sources = world
                .query::<(
                    &crate::space_visuals::OriginalSpaceBackdrop,
                    &MeshMaterial3d<StandardMaterial>,
                )>()
                .iter(world)
                .map(|(s, m)| (s.clone(), m.0.clone()))
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 3, "original nebula plates must be bound");
            for (source, material) in sources {
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&material)
                    .unwrap();
                assert_eq!(material.base_color_texture.as_ref(), Some(&source.image));
                assert_eq!(material.alpha_mode, AlphaMode::Add);
                assert!(world
                    .resource::<Assets<Image>>()
                    .get(&source.image)
                    .is_some());
            }
        }
        if matches!(name, "creature" | "tribal" | "civilization") {
            assert_original_cloud_binding(&mut app);
        }
        if name == "creature" {
            assert_native_creature_surfaces(&mut app);
            assert_authored_player_surfaces(&mut app);
            let world = app.world_mut();
            if world.resource::<crate::creature_hud::CreatureHudActive>().0 {
                // The retail SPUI Creature HUD replaced the legacy native art:
                // its layouts must be spawned and solved.
                let ready = world
                    .query::<(&crate::creature_hud::CreatureHudRoot, &crate::spui::view::SpuiState)>()
                    .iter(world)
                    .filter(|(_, ui)| ui.ready)
                    .count();
                assert!(ready >= 4, "the Creature HUD layouts must be spawned and laid out, got {ready}");
            } else {
                assert!(world.query::<(&crate::player_hud::NativeHudArt,&ComputedNode)>().iter(world)
                    .filter(|(_,node)|node.size().min_element()>0.).count()>=14,
                    "native health/hunger, brain, action and tray images must have visible layout sizes");
            }
        }
        if name == "civilization" {
            let world = app.world_mut();
            if world.resource::<crate::civ_hud::CivHudActive>().0 {
                // The retail SPUI Civ HUD replaced the legacy HUD: its layouts
                // must be spawned and laid out.
                let ready = world
                    .query::<(&crate::civ_hud::CivHudRoot, &crate::spui::view::SpuiState)>()
                    .iter(world)
                    .filter(|(_, ui)| ui.ready)
                    .count();
                assert!(ready >= 6, "the Civ HUD layouts must be spawned and laid out, got {ready}");
            }
        }
        if name == "space" {
            let world = app.world_mut();
            if world.resource::<crate::space_hud::SpaceHudActive>().0 {
                // The retail SPUI Space HUD replaced the legacy one: its
                // layouts must be spawned and solved.
                let ready = world
                    .query::<(&crate::space_hud::SpaceHudRoot, &crate::spui::view::SpuiState)>()
                    .iter(world)
                    .filter(|(_, ui)| ui.ready)
                    .count();
                assert!(ready >= 4, "the Space HUD layouts must be spawned and laid out, got {ready}");
            }
        }
        let path = output.join(format!("{name}.png"));
        capture_render(&mut app, &target, &path);
        println!("PASS original-asset GPU render: {}", path.display());
        if name == "menu" {
            app.world_mut()
                .resource_mut::<crate::HudOptions>()
                .diagnostics = true;
            let count = app
                .world()
                .resource::<crate::Originals>()
                .animation_catalog
                .clips
                .len();
            assert!(
                count > 24,
                "the full installed animation catalog must be available"
            );
            let mesh_count = app.world().resource::<Assets<Mesh>>().len();
            for _ in 0..20 {
                render_key(&mut app, input_window, KeyCode::KeyP, "p");
            }
            assert_eq!(app.world().resource::<crate::ClipPick>().0, 20);
            assert_eq!(
                app.world().resource::<Assets<Mesh>>().len(),
                mesh_count,
                "changing clips must reuse the preview marker assets"
            );
            let originals = app.world().resource::<crate::Originals>();
            assert_eq!(
                originals.clips.len(),
                1,
                "only the current preview is active"
            );
            assert_eq!(
                originals.animation_cache.len(),
                16,
                "decoding clips must stay bounded"
            );
            assert!(originals.clips[0].timed_clip.is_some());
            capture_render(&mut app, &target, &output.join("menu-animation.png"));
            app.world_mut()
                .resource_mut::<crate::HudOptions>()
                .diagnostics = false;
            println!("PASS full animation catalog input and bounded preview cache");
        }
        if name == "creature" {
            let previous = *app.world().get::<Transform>(camera).unwrap();
            let world = app.world_mut();
            let actor = world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap()
                .translation;
            world.insert_resource(RenderSmokeCamera(
                Transform::from_translation(actor + Vec3::new(0., 1.5, -7.))
                    .looking_at(actor + Vec3::Y * 0.2, Vec3::Y),
            ));
            for _ in 0..12 {
                app.update();
                gpu_flush(&app);
            }
            capture_render(
                &mut app,
                &target,
                &output.join("creature-original-player.png"),
            );
            app.world_mut().remove_resource::<RenderSmokeCamera>();
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = previous;
            let world = app.world_mut();
            let surface = world.resource::<crate::terrain::LandSurface>();
            assert_eq!(
                surface.seed,
                world.resource::<RunProgress>().species_world.seed
            );
            assert_eq!(
                world
                    .query_filtered::<Entity, With<crate::terrain::WorldGround>>()
                    .iter(world)
                    .count(),
                2,
                "terrain mesh plus sea"
            );
            let previous = *world.get::<Transform>(camera).unwrap();
            world.insert_resource(RenderSmokeCamera(
                Transform::from_xyz(0., 42., 53.).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            for _ in 0..12 {
                app.update();
                gpu_flush(&app);
            }
            capture_render(&mut app, &target, &output.join("creature-coast.png"));
            app.world_mut().remove_resource::<RenderSmokeCamera>();
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = previous;
            println!("PASS deterministic terrain and animated ocean GPU capture");
        }
        if name == "tribal" {
            // Confirm that installed objects replaced village cones, then
            // inspect a close-up without the prototype HUD covering the hut.
            let world = app.world_mut();
            let count = world
                .query_filtered::<Entity, (
                    With<crate::tribal::RivalHut>,
                    Without<crate::world_objects::OriginalWorldObject>,
                )>()
                .iter(world)
                .count();
            assert_eq!(count, 0, "installed Tribal huts must draw native objects");
            let previous = *world.get::<Transform>(camera).unwrap();
            let ui: Vec<_> = world
                .query_filtered::<(Entity, Option<&Visibility>), (With<Node>, Without<ChildOf>)>()
                .iter(world)
                .map(|(e, v)| (e, v.copied()))
                .collect();
            for (e, _) in &ui {
                world.entity_mut(*e).insert(Visibility::Hidden);
            }
            world.insert_resource(RenderSmokeCamera(
                Transform::from_xyz(7., 4.8, 8.).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
            ));
            for _ in 0..12 {
                app.update();
                gpu_flush(&app);
            }
            capture_render(&mut app, &target, &output.join("tribal-original-hut.png"));
            let world = app.world_mut();
            world.remove_resource::<RenderSmokeCamera>();
            *world.get_mut::<Transform>(camera).unwrap() = previous;
            for (e, v) in ui {
                let Ok(mut ui_entity) = world.get_entity_mut(e) else {
                    continue;
                };
                match v {
                    Some(v) => {
                        ui_entity.insert(v);
                    }
                    None => {
                        ui_entity.remove::<Visibility>();
                    }
                }
            }
            let world = app.world_mut();
            let mates_before = world
                .query_filtered::<Entity, With<TribeMate>>()
                .iter(world)
                .count();
            render_key(&mut app, input_window, KeyCode::KeyN, "n");
            let run = app.world().resource::<RunProgress>();
            let focused = run.focused_part as usize;
            let instance = run.tribe[focused].part;
            let definition = crate::dbpf::ResourceKey {
                group_id: crate::asset_catalog::TOOL_PARTS,
                type_id: crate::asset_catalog::PROP,
                instance_id: run.tribe[focused]
                    .outfit_definition
                    .expect("new outfits save their exact definition"),
            };
            assert_ne!(instance, 0, "N equips an original outfit part");
            assert_definition_mount(&mut app, definition);
            capture_render(&mut app, &target, &output.join("tribal-outfit-editor.png"));
            render_key(&mut app, input_window, KeyCode::Backspace, "\u{8}");
            let world = app.world_mut();
            assert_eq!(world.resource::<RunProgress>().tribe[focused].part, 0);
            assert_eq!(
                world.resource::<RunProgress>().tribe[focused].outfit_definition,
                None
            );
            let mesh = world
                .resource::<crate::Originals>()
                .mechanical_models
                .get(&definition)
                .unwrap()
                .handle
                .clone();
            assert!(
                world.query::<&Mesh3d>().iter(world).all(|m| m.0 != mesh),
                "removing the outfit must remove its mesh"
            );
            assert_eq!(
                world
                    .query_filtered::<Entity, With<TribeMate>>()
                    .iter(world)
                    .count(),
                mates_before
            );
            println!("PASS original Tribal outfit keyboard assignment and removal on GPU");
            let before = app.world().resource::<RunProgress>().tribe.len();
            {
                let world = app.world_mut();
                world.resource_mut::<RunProgress>().dna_this_stage =
                    crate::tribal::RECRUIT_FOOD * 2.;
                let mut player = world.query_filtered::<&mut Health, With<Player>>();
                let mut health = player.single_mut(world).unwrap();
                health.hp = health.max;
            }
            render_key(&mut app, input_window, KeyCode::KeyR, "r");
            assert_eq!(
                app.world().resource::<RunProgress>().tribe.len(),
                before,
                "a paid egg must grow before joining the controllable tribe"
            );
            capture_render(&mut app, &target, &output.join("tribal-growing.png"));
            for _ in 0..((crate::tribal::BIRTH_GROWTH_SECONDS * 60.0).ceil() as usize + 5) {
                app.update();
                gpu_flush(&app);
                if app.world().resource::<RunProgress>().tribe.len() > before {
                    break;
                }
            }
            assert_eq!(
                app.world().resource::<RunProgress>().tribe.len(),
                before + 1,
                "R must recruit through the actual gameplay controls"
            );
            assert_tribal_body_appearance(&mut app, before + 1);
            capture_render(&mut app, &target, &output.join("tribal-recruited.png"));
            println!("PASS recruited tribe member uses the same original body, paint, scale and attachments");
            // Also verify the untouched lineage: the earlier Creature creator
            // checks deliberately customized this run, so that view must keep
            // its editable model rather than silently substitute a source.
            let starter = crate::editor::starter_creature_parts(
                app.world().resource::<RunProgress>(),
                &app.world().resource::<crate::Originals>().part_catalog,
            );
            let saved = {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                let saved = (run.body.clone(), run.parts.clone());
                run.body = crate::body::BodyProfile::default();
                run.parts = starter;
                saved
            };
            for _ in 0..6 {
                app.update();
                gpu_flush(&app);
            }
            assert_authored_player_surfaces(&mut app);
            let world = app.world_mut();
            let expected = world.resource::<RunProgress>().tribe.len() + 1;
            assert_eq!(
                world
                    .query::<&crate::baked_model::NativePlayerSource>()
                    .iter(world)
                    .count(),
                expected,
                "the chief and every tribe member must use the untouched authored source"
            );
            capture_render(
                &mut app,
                &target,
                &output.join("tribal-authored-lineage.png"),
            );
            {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                run.body = saved.0;
                run.parts = saved.1;
            }
            for _ in 0..6 {
                app.update();
                gpu_flush(&app);
            }
            assert_tribal_body_appearance(&mut app, before + 1);
            println!("PASS authored Tribal lineage and restoration of the customized build");
        } else if name == "civilization" {
            render_key(&mut app, input_window, KeyCode::KeyN, "n");
            let instance = app
                .world()
                .resource::<RunProgress>()
                .vehicle
                .parts
                .last()
                .expect("N equips an original vehicle part")
                .instance;
            assert_mechanical_mount(&mut app, crate::asset_catalog::VEHICLE_PARTS, instance);
            render_key(&mut app, input_window, KeyCode::KeyR, "r");
            render_ctrl_key(&mut app, input_window, KeyCode::Equal, "=");
            render_key(&mut app, input_window, KeyCode::KeyI, "i");
            let fitted = &app.world().resource::<RunProgress>().vehicle.parts[0];
            assert!(fitted.yaw > 0. && fitted.scale > 1. && fitted.offset[0] > 0.);
            render_key(&mut app, input_window, KeyCode::BracketRight, "]");
            let changed = app.world().resource::<RunProgress>().vehicle.parts[0].instance;
            assert_ne!(
                changed, instance,
                "vehicle cycling reaches a different original model"
            );
            assert_mechanical_mount(&mut app, crate::asset_catalog::VEHICLE_PARTS, changed);
            capture_render(&mut app, &target, &output.join("civ-vehicle-editor.png"));
            render_key(&mut app, input_window, KeyCode::KeyV, "v");
            render_key(&mut app, input_window, KeyCode::KeyN, "n");
            let instance = app
                .world()
                .resource::<RunProgress>()
                .building
                .parts
                .last()
                .expect("V then N equips an original building part")
                .instance;
            assert_mechanical_mount(&mut app, crate::asset_catalog::BUILDING_PARTS, instance);
            capture_render(&mut app, &target, &output.join("civ-building-editor.png"));
            render_ctrl_key(&mut app, input_window, KeyCode::Backspace, "\u{8}");
            let world = app.world_mut();
            assert!(world.resource::<RunProgress>().building.parts.is_empty());
            assert_eq!(
                world
                    .query::<&crate::civ::CityPartRef>()
                    .iter(world)
                    .count(),
                0
            );
            assert_eq!(
                world.query::<&crate::civ::CityMarker>().iter(world).count(),
                crate::civ::CITY_COUNT
            );
            println!("PASS original vehicle and building keyboard assembly on GPU");
        } else if name == "space" {
            render_key(&mut app, input_window, KeyCode::KeyN, "n");
            let instance = app
                .world()
                .resource::<RunProgress>()
                .ship
                .modules
                .last()
                .expect("N equips an original mechanical ship module")
                .instance;
            assert_mechanical_mount(&mut app, crate::asset_catalog::VEHICLE_PARTS, instance);
            render_key(&mut app, input_window, KeyCode::KeyR, "r");
            // The retail camera owns `=`: the loadout key needs Ctrl+Alt held.
            set_debug_chord(&mut app, true);
            render_key(&mut app, input_window, KeyCode::Equal, "=");
            set_debug_chord(&mut app, false);
            render_key(&mut app, input_window, KeyCode::KeyI, "i");
            let fitted = &app.world().resource::<RunProgress>().ship.modules[0];
            assert!(fitted.yaw > 0. && fitted.scale > 1. && fitted.offset[0] > 0.);
            capture_render(&mut app, &target, &output.join("space-ship-editor.png"));
            set_debug_chord(&mut app, true);
            render_key(&mut app, input_window, KeyCode::Backspace, "\u{8}");
            set_debug_chord(&mut app, false);
            let world = app.world_mut();
            assert!(world.resource::<RunProgress>().ship.modules.is_empty());
            assert_eq!(
                world
                    .query::<&crate::space::ShipModuleRef>()
                    .iter(world)
                    .count(),
                0
            );
            assert_eq!(
                world
                    .query_filtered::<Entity, With<Player>>()
                    .iter(world)
                    .count(),
                1
            );
            println!("PASS original ship module keyboard assembly on GPU");
        }
        if matches!(name, "tribal" | "civilization" | "space") {
            // The later stages' outfitters run on the shared editor layer.
            crate::outfitter::render_smoke_checks(
                &mut app,
                input_window,
                &|app| {
                    app.update();
                    gpu_flush(app);
                },
                &|app, file| capture_render(app, &target, &output.join(file)),
            );
        }
        if matches!(name, "civilization" | "space") {
            // The vehicle, building and UFO editors (F3) on the same layer.
            crate::rigid_editor::render_smoke_checks(
                &mut app,
                input_window,
                &|app| {
                    app.update();
                    gpu_flush(app);
                },
                &|app, file| capture_render(app, &target, &output.join(file)),
            );
        }
        if name == "menu" || name == "cell" || name == "creature" {
            app.world_mut()
                .resource_mut::<crate::editor::EditorState>()
                .show();
            app.update();
            gpu_flush(&app);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            for _ in 0..20 {
                app.update();
                gpu_flush(&app);
            }
            if name == "creature" {
                assert_authored_player_surfaces(&mut app);
                assert_original_editor_environment(&mut app);
                capture_render(
                    &mut app,
                    &target,
                    &output.join("creature-editor-original.png"),
                );
            }
            if name == "cell" {
                let world = app.world_mut();
                let run = world.resource::<RunProgress>();
                println!("Cell build {:?}", run.cell_parts);
                assert!(run.cell_parts.len() >= 3);
                for p in &run.cell_parts {
                    let m = world
                        .resource::<crate::Originals>()
                        .models
                        .iter()
                        .find(|m| m.instance == p.instance)
                        .expect("cell part loaded");
                    assert!(m.material.is_some());
                }
                assert!(
                    world
                        .query_filtered::<Entity, With<crate::editor::CellPart>>()
                        .iter(world)
                        .count()
                        >= 3
                );
                // The Cell stage has its own Creator (cell_editor), driven through
                // its real palette, pointer and keyboard paths.
                crate::cell_editor::render_smoke_checks(&mut app, input_window, &|app| {
                    app.update();
                    gpu_flush(app);
                });
            }
            for _ in 0..20 {
                app.update();
                gpu_flush(&app);
            }
            let path = output.join(format!("{name}-editor.png"));
            capture_render(&mut app, &target, &path);
            println!(
                "PASS authored part palette render and interaction: {}",
                path.display()
            );
            if name == "creature" {
                // The retail Creature Creator (creature_editor) is driven
                // through its own chrome, keyboard table and modes.
                crate::creature_editor::render_smoke_checks(&mut app, input_window, &|app| {
                    app.update();
                    gpu_flush(app);
                });
                for (mode, file) in [
                    (crate::creature_editor::Mode::Paint, "creature-editor-paint.png"),
                    (crate::creature_editor::Mode::Play, "creature-editor-testdrive.png"),
                ] {
                    crate::creature_editor::force_mode(&mut app, mode);
                    for _ in 0..30 {
                        app.update();
                        gpu_flush(&app);
                    }
                    capture_render(&mut app, &target, &output.join(file));
                }
                crate::creature_editor::force_mode(&mut app, crate::creature_editor::Mode::Build);
                for _ in 0..4 {
                    app.update();
                    gpu_flush(&app);
                }
                println!("PASS Creature Creator Paint and Test Drive modes render");
                // The older body handles stay reachable through queued actions
                // (the retail spine manipulators are creator-b's).
                let queue_action = |app: &mut App, action: crate::editor::Action| {
                    app.world_mut().resource_mut::<crate::editor::EditorState>().queued.push(action);
                    app.update();
                    gpu_flush(app);
                };
                let before = app.world().resource::<RunProgress>().body.clone();
                queue_action(&mut app, crate::editor::Action::Body);
                queue_action(&mut app, crate::editor::Action::Thickness(0.1));
                assert_ne!(app.world().resource::<RunProgress>().body, before);
                let before_drag = app.world().resource::<RunProgress>().body.clone();
                let world = app.world_mut();
                let pt = *world
                    .query_filtered::<&Transform, With<Player>>()
                    .single(world)
                    .unwrap();
                let handle_point = crate::baked_model::control_handle(
                    world.resource::<crate::Originals>(),
                    &before_drag,
                    2,
                );
                let center = pt.translation + pt.rotation * (handle_point * pt.scale);
                let (cam, global) = world
                    .entity(camera)
                    .get::<Camera>()
                    .zip(world.entity(camera).get::<GlobalTransform>())
                    .unwrap();
                let cursor = cam
                    .world_to_viewport(global, center)
                    .expect("spine handle projects");
                world
                    .entity_mut(input_window)
                    .get_mut::<Window>()
                    .unwrap()
                    .set_cursor_position(Some(cursor));
                send_mouse(&mut app, input_window, bevy::input::ButtonState::Pressed);
                app.update();
                gpu_flush(&app);
                app.world_mut()
                    .entity_mut(input_window)
                    .get_mut::<Window>()
                    .unwrap()
                    .set_cursor_position(Some(cursor + Vec2::new(30., -45.)));
                app.update();
                gpu_flush(&app);
                send_mouse(&mut app, input_window, bevy::input::ButtonState::Released);
                app.update();
                gpu_flush(&app);
                let after_drag = app.world().resource::<RunProgress>().body.clone();
                assert_ne!(after_drag, before_drag, "actual mouse drag shapes spine");
                queue_action(&mut app, crate::editor::Action::Undo);
                assert_eq!(app.world().resource::<RunProgress>().body, before_drag);
                queue_action(&mut app, crate::editor::Action::Redo);
                assert_eq!(app.world().resource::<RunProgress>().body, after_drag);
                assert!(after_drag.spine.iter().all(|p| p.position[0] == 0.));
                println!("PASS actual spine dragging, symmetry plane, undo and redo");
                capture_render(&mut app, &target, &output.join("creature-body-editor.png"));
                queue_action(&mut app, crate::editor::Action::Body);
            }
            if name == "creature" {
                // The retail Creature Creator closes with Save and Exit.
                render_key(&mut app, input_window, KeyCode::Enter, "");
                assert!(!app.world().resource::<crate::editor::EditorState>().open);
            } else if name != "cell" {
                press_palette(&mut app, |a| matches!(a, crate::editor::Action::Done));
            } else {
                // The Cell Creator (cell_editor) closes with Save.
                render_key(&mut app, input_window, KeyCode::F2, "");
            }
        }
        if name == "cell" {
            // Retail growth: food progression raises the world scale; the
            // player keeps its world size while the world shrinks around it.
            app.world_mut().resource_mut::<RunProgress>().cell.food_progression = 800;
            // The world scale follows its retail bang-bang filter (RE 11.2):
            // leaving 0.5 takes about 1.6 s.
            for _ in 0..240 {
                app.update();
                gpu_flush(&app);
                if app.world().resource::<crate::cell_engine::CellGame>().world_scale > 1.0 {
                    break;
                }
            }
            let world = app.world_mut();
            let grown = world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap();
            assert!(grown.scale.x.is_finite() && grown.scale.x > 0.0);
            assert!(world.resource::<crate::cell_engine::CellGame>().world_scale > 1.0);
            capture_render(&mut app, &target, &output.join("cell-grown.png"));
            println!("PASS grown Cell rendering with original mounted parts");
        }
        if name == "creature" {
            // Retail brain level 3 starts at the stage total 600.
            app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 600.;
            app.world_mut()
                .resource_mut::<crate::play_camera::PlayCamera>()
                .input_delta(Vec2::new(60., -12.), 1., true, false);
            render_key(&mut app, input_window, KeyCode::KeyF, "f");
            for _ in 0..60 {
                app.update();
                gpu_flush(&app);
            }
            assert_eq!(
                app.world()
                    .resource::<RunProgress>()
                    .creature_progression
                    .brain_level,
                3
            );
            capture_render(&mut app, &target, &output.join("creature-evolved.png"));
            println!("PASS orbit camera and evolved Creature native combat HUD");
        }
    }
    // Independent default runs prevent mutation stress tests above from
    // determining the appearance of a later stage's user-facing screenshot.
    for (name, stage) in [
        ("main-menu", GameStage::MainMenu),
        ("cell", GameStage::Cell),
        ("creature", GameStage::Creature),
        ("tribal", GameStage::Tribal),
        ("civilization", GameStage::Civilization),
        ("space", GameStage::Space),
    ] {
        let mut run = RunProgress::default();
        if !matches!(stage, GameStage::MainMenu | GameStage::Cell) {
            run.parts = crate::editor::starter_creature_parts(
                &run,
                &app.world().resource::<crate::Originals>().part_catalog,
            );
            run.creature_editor_initialized = true;
        }
        run.reset_to(stage.clone());
        // This test looks at the game, not at the Creature introduction text
        // window that holds a new lineage.
        run.creature_flow.intro_shown = true;
        *app.world_mut().resource_mut::<RunProgress>() = run;
        *app.world_mut().resource_mut::<crate::editor::EditorState>() = default();
        *app.world_mut()
            .resource_mut::<crate::play_camera::PlayCamera>() = default();
        app.world_mut()
            .resource_mut::<crate::pause_menu::PauseMenu>()
            .open = false;
        app.world_mut()
            .resource_mut::<crate::HudOptions>()
            .diagnostics = false;
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(stage.clone());
        for _ in 0..90 {
            app.update();
            gpu_flush(&app);
        }
        if stage == GameStage::Cell {
            render_key(&mut app, input_window, KeyCode::Digit1, "1");
        }
        capture_render(
            &mut app,
            &target,
            &output.join(format!("showcase-{name}.png")),
        );
        if stage == GameStage::Creature {
            let key =
                crate::baked_model::player_source_key(app.world().resource::<crate::Originals>())
                    .unwrap();
            let original_handles = app.world().resource::<crate::Originals>().baked_models[&key]
                .surfaces
                .iter()
                .map(|s| s.handle.clone())
                .collect::<Vec<_>>();
            let original_positions = original_handles
                .iter()
                .map(|h| {
                    app.world()
                        .resource::<Assets<Mesh>>()
                        .get(h)
                        .unwrap()
                        .attribute(Mesh::ATTRIBUTE_POSITION)
                        .unwrap()
                        .clone()
                })
                .collect::<Vec<_>>();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            for _ in 0..40 {
                app.update();
                gpu_flush(&app);
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::KeyW);
            let world = app.world_mut();
            let owned = world
                .query::<(&crate::locomotion_preview::OwnedWalkSurface, &Mesh3d)>()
                .iter(world)
                .map(|(s, m)| (s.original.clone(), m.0.clone()))
                .collect::<Vec<_>>();
            assert!(
                !owned.is_empty(),
                "walking actor must own deformable meshes"
            );
            let meshes = world.resource::<Assets<Mesh>>();
            for (handle, positions) in original_handles.iter().zip(&original_positions) {
                assert_eq!(
                    meshes
                        .get(handle)
                        .unwrap()
                        .attribute(Mesh::ATTRIBUTE_POSITION)
                        .unwrap(),
                    positions,
                    "walking must not modify cached original meshes"
                );
            }
            for (original, draw) in &owned {
                assert_ne!(original, draw, "GPU skinning uses a derived draw mesh");
                assert_eq!(
                    meshes
                        .get(original)
                        .unwrap()
                        .attribute(Mesh::ATTRIBUTE_POSITION),
                    meshes
                        .get(draw)
                        .unwrap()
                        .attribute(Mesh::ATTRIBUTE_POSITION),
                    "GPU skinning preserves source rest vertices"
                );
            }
            let palettes=world.query_filtered::<&bevy::mesh::skinning::SkinnedMesh,With<crate::player_appearance::NativePlayerSurface>>().iter(world).map(|skin|skin.joints.clone()).collect::<Vec<_>>();
            assert!(
                !palettes.is_empty(),
                "the walking player's original surfaces bind GPU joint palettes"
            );
            assert!(
                palettes.iter().flatten().any(|joint| world
                    .get::<Transform>(*joint)
                    .is_some_and(|t| !t.to_matrix().abs_diff_eq(Mat4::IDENTITY, 1e-5))),
                "walking poses an actual player's GPU joint palette"
            );
            capture_render(
                &mut app,
                &target,
                &output.join("showcase-creature-walking.png"),
            );
            render_key(&mut app, input_window, KeyCode::F2, "");
            for _ in 0..30 {
                app.update();
                gpu_flush(&app);
            }
            // The Creature stage's creator is the retail one (creature_editor).
            assert_original_editor_environment(&mut app);
            assert!(app.world().resource::<crate::creature_editor::CreatureEditor>().active);
            capture_render(&mut app, &target, &output.join("showcase-creator.png"));
            {
                let mut editor = app.world_mut().resource_mut::<crate::creature_editor::CreatureEditor>();
                // Arms & Legs, second page.
                editor.category = 2;
                editor.page = 1;
            }
            for _ in 0..12 {
                app.update();
                gpu_flush(&app);
            }
            assert_eq!(app.world().resource::<crate::creature_editor::CreatureEditor>().page, 1);
            capture_render(
                &mut app,
                &target,
                &output.join("showcase-creator-parts-page-2.png"),
            );
            let world = app.world_mut();
            assert!(
                world
                    .query::<&crate::player_appearance::NativePlayerSurface>()
                    .iter(world)
                    .count()
                    > 0,
                "fresh creator uses full authored starter surfaces"
            );
            render_key(&mut app, input_window, KeyCode::F2, "");
            assert!(!app.world().resource::<crate::editor::EditorState>().open);
            render_key(&mut app, input_window, KeyCode::Escape, "");
            assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
            for _ in 0..30 {
                app.update();
                gpu_flush(&app);
            }
            capture_render(&mut app, &target, &output.join("showcase-pause-menu.png"));
            render_key(&mut app, input_window, KeyCode::Escape, "");
            assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        }
    }
    let originals = app.world().resource::<crate::Originals>();
    println!(
        "Rendered all stages, creator and pause menu from independent default runs, plus mutation stress scenarios with {} creature/preview meshes, {} catalog material slots, {} assembly definitions and {} gallery textures; player saves untouched.",
        originals.models.len(),
        originals
            .models
            .iter()
            .filter(|m| m.material.is_some())
            .count(),
        originals.mechanical_models.len(),
        originals.textures.len()
    );
}

fn assert_mechanical_mount(app: &mut App, group: u32, instance: u32) {
    let world = app.world_mut();
    let model =
        crate::mechanical_assets::cached(world.resource::<crate::Originals>(), group, instance)
            .expect("equipped mechanical model has an exact-definition cache entry")
            .clone();
    assert_model_mount(app, model);
}

fn assert_definition_mount(app: &mut App, key: crate::dbpf::ResourceKey) {
    let model = app
        .world()
        .resource::<crate::Originals>()
        .mechanical_models
        .get(&key)
        .expect("equipped exact definition is imported")
        .clone();
    assert_model_mount(app, model);
}

fn assert_original_cloud_binding(app: &mut App) {
    let world = app.world_mut();
    let seas: Vec<_> = world
        .query::<(
            &crate::water::OriginalWaterSources,
            &MeshMaterial3d<crate::water::WaterMaterial>,
        )>()
        .iter(world)
        .map(|(sources, material)| (sources.clone(), material.0.clone()))
        .collect();
    assert_eq!(seas.len(), 1, "one installed ocean material per land stage");
    let (sources, handle) = &seas[0];
    let water = world
        .resource::<Assets<crate::water::WaterMaterial>>()
        .get(handle)
        .unwrap();
    assert_eq!(sources.0.len(), 3, "normal, foam and seabed must decode");
    for source in &sources.0 {
        let bound = if source.key == crate::water::WATER_NORMAL {
            &water.extension.normal_texture
        } else if source.key == crate::water::FOAM_CUT {
            &water.extension.foam_texture
        } else {
            assert_eq!(source.key, crate::water::SEABED);
            &water.extension.seabed_texture
        };
        assert_eq!(bound, &source.image);
        let image = world.resource::<Assets<Image>>().get(bound).unwrap();
        assert_eq!(source.dimensions, UVec2::new(image.width(), image.height()));
        assert!(source.dimensions.min_element() > 1);
        assert_eq!(
            image.texture_descriptor.format,
            if source.linear {
                bevy::render::render_resource::TextureFormat::Rgba8Unorm
            } else {
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
            }
        );
    }
    for entity in world
        .query_filtered::<Entity, (
            With<crate::terrain::WorldGround>,
            Without<MeshMaterial3d<crate::water::WaterMaterial>>,
        )>()
        .iter(world)
    {
        assert!(world
            .entity(entity)
            .contains::<MeshMaterial3d<crate::terrain::TerrainMaterial>>());
        assert!(
            !world
                .entity(entity)
                .contains::<MeshMaterial3d<StandardMaterial>>(),
            "a second opaque material must not hide the layered coast/detail shader"
        );
    }
    let draws: Vec<_> = world
        .query::<(
            &crate::world_presentation::OriginalCloudSource,
            &MeshMaterial3d<crate::world_presentation::PreviewSkyMaterial>,
        )>()
        .iter(world)
        .map(|(source, material)| (source.clone(), material.0.clone()))
        .collect();
    assert_eq!(
        draws.len(),
        1,
        "land sky must bind the original cloud raster"
    );
    let (source, handle) = &draws[0];
    assert_eq!(source.key, crate::world_presentation::CLOUD_RASTER);
    let image = world
        .resource::<Assets<Image>>()
        .get(&source.image)
        .expect("cloud image present");
    assert_eq!(source.dimensions, UVec2::new(image.width(), image.height()));
    assert!(
        source.dimensions.min_element() > 1,
        "original image, not transparent fallback"
    );
    let material = world
        .resource::<Assets<crate::world_presentation::PreviewSkyMaterial>>()
        .get(handle)
        .expect("sky material present");
    assert_eq!(material.extension.cloud_texture, source.image);
    assert!(material.extension.atmosphere.w > 0.);
}

fn assert_original_editor_environment(app: &mut App) {
    let world = app.world_mut();
    let source_key = world
        .resource::<crate::editor_environment::EditorEnvironmentAssets>()
        .definition
        .expect("creator background definition imported");
    let model = world.resource::<crate::Originals>().world_models[&source_key].clone();
    let surfaces = world
        .query::<(
            &crate::editor_environment::EditorEnvironmentSurface,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(source, mesh, mat)| (source.0, mesh.0.clone(), mat.0.clone()))
        .collect::<Vec<_>>();
    assert_eq!(surfaces.len(), model.surfaces.len());
    for (_, mesh, mat) in surfaces {
        assert!(model
            .surfaces
            .iter()
            .any(|s| s.handle == mesh && s.material == mat));
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(&mat)
            .unwrap();
        assert!(material
            .base_color_texture
            .as_ref()
            .is_some_and(|h| world.resource::<Assets<Image>>().contains(h)));
    }
}

fn assert_authored_player_surfaces(app: &mut App) {
    let world = app.world_mut();
    let actor = world
        .query_filtered::<Entity, With<Player>>()
        .single(world)
        .unwrap();
    let key = world
        .get::<crate::baked_model::NativePlayerSource>(actor)
        .expect("untouched player must display complete authored creature")
        .0;
    assert!(
        !world.entity(actor).contains::<Mesh3d>(),
        "retire the reshaped proxy draw"
    );
    assert!(
        world.resource::<State<GameStage>>().get() != &GameStage::Creature
            || !world
                .entity(actor)
                .contains::<crate::baked_model::NativeCreature>(),
        "player presentation is not an NPC population marker"
    );
    let model = world.resource::<crate::Originals>().baked_models[&key].clone();
    let draws: Vec<_> = world
        .query::<(
            &crate::player_appearance::NativePlayerSurface,
            &ChildOf,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            &Transform,
            Option<&crate::locomotion_preview::OwnedWalkSurface>,
        )>()
        .iter(world)
        .filter(|(_, parent, ..)| parent.parent() == actor)
        // A clip-posed body draws the shared skinned copy of its source.
        .map(|(source, _, mesh, material, t, owned)| (source.0, owned.map_or(mesh.0.clone(), |o| o.original.clone()), material.0.clone(), *t))
        .collect();
    assert_eq!(
        draws.len(),
        model.surfaces.len(),
        "one draw per exact original material partition"
    );
    for draw in &draws {
        assert_eq!(draw.0, key);
        assert!(model
            .surfaces
            .iter()
            .any(|surface| draw.1 == surface.handle && draw.2 == surface.material));
    }
    let actor_transform = *world.get::<Transform>(actor).unwrap();
    let meshes = world.resource::<Assets<Mesh>>();
    let lowest = draws
        .iter()
        .flat_map(|draw| {
            let mesh = meshes.get(&draw.1).unwrap();
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("original player positions")
            };
            vertices.iter().map(move |p| {
                actor_transform
                    .transform_point(draw.3.transform_point(Vec3::from_array(*p)))
                    .y
            })
        })
        .fold(f32::INFINITY, f32::min);
    let ground = world
        .resource::<crate::terrain::LandSurface>()
        .height(actor_transform.translation.x, actor_transform.translation.z);
    assert!(
        (lowest - ground).abs() < 0.09,
        "original feet must meet terrain: {lowest} vs {ground}"
    );
}

fn assert_native_creature_surfaces(app: &mut App) {
    let world = app.world_mut();
    let actors: Vec<Vec<Entity>> = world
        .query_filtered::<&Children, (With<Npc>, With<crate::baked_model::NativeCreature>)>()
        .iter(world)
        .map(|children| children.iter().collect())
        .collect();
    assert!(
        !actors.is_empty(),
        "installed Creature NPCs require original models"
    );
    for children in actors {
        let source = children
            .iter()
            .find_map(|&child| world.get::<crate::world_objects::OriginalWorldObject>(child))
            .expect("native NPC has its GMDL source");
        let model = world
            .resource::<crate::Originals>()
            .baked_models
            .get(&source.model)
            .unwrap();
        assert_eq!(
            children.len(),
            model.surfaces.len(),
            "each material partition needs its own draw"
        );
        for surface in &model.surfaces {
            assert!(
                children.iter().any(|&child| {
                    // A walking NPC draws the shared skinned copy of its source.
                    (world
                        .get::<Mesh3d>(child)
                        .is_some_and(|mesh| mesh.0 == surface.handle)
                        || world
                            .get::<crate::locomotion_preview::OwnedWalkSurface>(child)
                            .is_some_and(|owned| owned.original == surface.handle))
                        && world
                            .get::<MeshMaterial3d<StandardMaterial>>(child)
                            .is_some_and(|material| {
                                material.0 == surface.material
                                    // An alpha wears an identity-painted copy (creature_alpha).
                                    || crate::creature_alpha::painted_from(world, &material.0) == Some(surface.material.id())
                            })
                }),
                "native mesh must use its own source material"
            );
        }
    }
}

fn assert_tribal_body_appearance(app: &mut App, expected: usize) {
    let world = app.world_mut();
    let model = world
        .resource::<crate::Originals>()
        .baked_body
        .as_ref()
        .expect("original tribal body")
        .model
        .clone();
    let size = world.resource::<RunProgress>().creature_stats().size;
    let parts = world.resource::<RunProgress>().parts.len();
    let actors: Vec<Entity> = world
        .query_filtered::<Entity, With<TribeMate>>()
        .iter(world)
        .collect();
    assert_eq!(actors.len(), expected);
    for actor in actors {
        assert!(world
            .entity(actor)
            .contains::<crate::baked_model::NativeCreature>());
        // A clip-posed member draws the crowd's skinned copy of that mesh.
        let mesh = world.get::<Mesh3d>(actor).unwrap().0.clone();
        let drawn = world.get::<crate::outfit_crowd::CrowdActor>(actor).map_or(mesh, |a| a.original_mesh().clone());
        assert_eq!(drawn, model.draw_mesh());
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(actor)
                .unwrap()
                .0,
            model.material.clone().unwrap()
        );
        assert_eq!(
            world.get::<Transform>(actor).unwrap().scale,
            Vec3::splat(size)
        );
        let attached = world
            .get::<Children>(actor)
            .map(|children| {
                children
                    .iter()
                    .filter(|&child| {
                        world
                            .entity(child)
                            .contains::<crate::creature::MountedPart>()
                    })
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            attached, parts,
            "every member uses the saved lineage's attachments"
        );
    }
}

fn assert_model_mount(app: &mut App, model: crate::OriginalModel) {
    let world = app.world_mut();
    let material = model.material.clone().expect("catalog part material");
    assert!(world.resource::<Assets<Mesh>>().contains(&model.handle));
    assert!(world
        .resource::<Assets<StandardMaterial>>()
        .contains(&material));
    let imported_material = world
        .resource::<Assets<StandardMaterial>>()
        .get(&material)
        .unwrap();
    if let Some(texture) = imported_material.base_color_texture.as_ref() {
        let image = world
            .resource::<Assets<Image>>()
            .get(texture)
            .expect("bound original texture is live");
        if image
            .data
            .as_deref()
            .is_some_and(|rgba| rgba.chunks_exact(4).any(|c| c[3] < 255))
        {
            assert_ne!(
                imported_material.alpha_mode,
                AlphaMode::Opaque,
                "original texture transparency must reach the material"
            );
        }
    }
    assert!(
        world
            .query::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>)>()
            .iter(world)
            .any(|(mesh, mat)| mesh.0 == model.draw_mesh() && mat.0 == material),
        "equipped part must be mounted with its imported mesh and cached material"
    );
}

/// A key with Ctrl held: the Civilization assembly editor's keys that clash with
/// the camera (`, . - =`, Tab, Backspace) need it (`civ_controls::assembly_keys`).
fn render_ctrl_key(app: &mut App, window: Entity, key_code: KeyCode, character: &str) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    render_key(app, window, key_code, character);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
}

fn render_key(app: &mut App, window: Entity, key_code: KeyCode, character: &str) {
    use bevy::input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    };
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut()
            .resource_mut::<Messages<KeyboardInput>>()
            .write(KeyboardInput {
                key_code,
                logical_key: Key::Character(character.into()),
                state,
                text: None,
                repeat: false,
                window,
            });
        app.update();
        gpu_flush(app);
    }
    for _ in 0..4 {
        app.update();
        gpu_flush(app);
    }
}

/// A key tapped with Shift held.
fn render_shifted_key(app: &mut App, window: Entity, key_code: KeyCode, character: &str) {
    use bevy::input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    };
    let shift = |app: &mut App, state: ButtonState| {
        app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
            key_code: KeyCode::ShiftLeft,
            logical_key: Key::Shift,
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        gpu_flush(app);
    };
    shift(app, ButtonState::Pressed);
    render_key(app, window, key_code, character);
    shift(app, ButtonState::Released);
}

fn send_mouse(app: &mut App, window: Entity, state: bevy::input::ButtonState) {
    app.world_mut()
        .resource_mut::<Messages<bevy::input::mouse::MouseButtonInput>>()
        .write(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
}
pub(crate) fn gpu_flush(app: &App) {
    // Manual App::update has no window runner to pace submissions. Waiting
    // for each offscreen frame avoids building a large GPU queue and makes
    // asynchronous screenshot readbacks complete before changing scenes.
    app.world()
        .resource::<bevy::render::renderer::RenderDevice>()
        .poll(bevy::render::render_resource::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })
        .expect("offscreen GPU frame completes");
    // A screenshot can still complete while a broken material is invisible.
    // Treat shader/pipeline failures as failures of the render check itself.
    use bevy::render::{
        render_resource::{CachedPipelineState, PipelineCache},
        RenderApp,
    };
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    for pipeline in cache.pipelines() {
        if let CachedPipelineState::Err(error) = &pipeline.state {
            if matches!(
                error,
                bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                    | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable
            ) {
                continue; // The asynchronous asset loader retries these states.
            }
            panic!("GPU pipeline failed: {error:?}");
        }
    }
}
fn press_palette(app: &mut App, predicate: impl Fn(&crate::editor::Action) -> bool) {
    let world = app.world_mut();
    let button = world
        .query::<(Entity, &crate::editor::Action)>()
        .iter(world)
        .find(|(_, a)| predicate(a))
        .map(|(e, _)| e)
        .expect("creator action button");
    world.entity_mut(button).insert(bevy::ui::Pressed);
    app.update();
    gpu_flush(&app);
}

fn thumbnail_target_handles(
    app: &mut App,
) -> std::collections::HashMap<crate::dbpf::ResourceKey, bevy::asset::AssetId<Image>> {
    let world = app.world_mut();
    world
        .query::<(&crate::part_thumbnails::ThumbnailRequest, &ImageNode)>()
        .iter(world)
        .map(|(source, image)| (source.definition, image.image.id()))
        .collect()
}

fn assert_part_thumbnails(app: &mut App, populated: bool) {
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<crate::GameCamera>>()
            .iter(world)
            .count(),
        1,
        "UI and navigation retain one explicit main camera"
    );
    let requests = world
        .query::<(
            &crate::part_thumbnails::ThumbnailRequest,
            &ImageNode,
            &ComputedNode,
            &Visibility,
        )>()
        .iter(world)
        .map(|(s, i, n, v)| (s.definition, i.image.clone(), n.size(), *v))
        .collect::<Vec<_>>();
    let cameras = world
        .query::<&crate::part_thumbnails::ThumbnailCamera>()
        .iter(world)
        .map(|c| c.definition)
        .collect::<Vec<_>>();
    let scenes = world
        .query::<&crate::part_thumbnails::ThumbnailScene>()
        .iter(world)
        .map(|c| c.definition)
        .collect::<Vec<_>>();
    let lights = world
        .query::<&crate::part_thumbnails::ThumbnailStudioLight>()
        .iter(world)
        .count();
    let cache = world.resource::<crate::part_thumbnails::PartThumbnailCache>();
    assert!(
        cache.cached_images() <= crate::part_thumbnails::MAX_CACHED_IMAGES
            && cache.live_scenes() <= crate::part_thumbnails::MAX_LIVE_SCENES,
        "part previews remain bounded"
    );
    assert_eq!(cache.live_scenes(), scenes.len());
    assert_eq!(cameras.len(), scenes.len());
    if !populated {
        assert!(
            requests.is_empty() && cameras.is_empty() && scenes.is_empty() && lights == 0,
            "closing catalog retires all thumbnail rendering work"
        );
        return;
    }
    // Installed palette icons need no camera; live model previews (for
    // parts without one) are bounded to a page and share one studio light.
    assert!(
        cameras.len() <= crate::part_thumbnails::MAX_LIVE_SCENES
            && lights == usize::from(!cameras.is_empty()),
        "page-bounded isolated model previews share one studio light"
    );
    let images = world.resource::<Assets<Image>>();
    let mut visible = 0;
    for (key, image, size, visibility) in &requests {
        if *visibility == Visibility::Hidden {
            continue;
        }
        visible += 1;
        assert!(
            size.x >= 59. && size.y >= 59.,
            "part preview is visible in the actual UI layout"
        );
        let target = images.get(image).expect("part preview target exists");
        assert_eq!([target.width(), target.height()], [128, 128]);
        assert!(
            cache.native_icon(*key).is_some() || cameras.contains(key),
            "each palette icon is an installed icon or a live original-model render"
        );
    }
    assert!(visible > 0, "the open palette shows part icons");
    let rows = world
        .query::<(
            &crate::part_thumbnails::ThumbnailGeometry,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(s, m, a)| (*s, m.0.clone(), a.0.clone()))
        .collect::<Vec<_>>();
    assert!(
        cameras.is_empty() || !rows.is_empty(),
        "live part previews render actual imported geometry"
    );
    for (source, mesh, material) in rows {
        let definition = world
            .resource::<crate::Originals>()
            .part_catalog
            .parts
            .iter()
            .find(|p| p.key == source.definition)
            .expect("thumbnail definition");
        assert_eq!(definition.model, source.model);
        assert!(world.resource::<Assets<Mesh>>().contains(mesh.id()));
        assert!(world
            .resource::<Assets<StandardMaterial>>()
            .contains(material.id()));
    }
    println!("PASS isolated native part thumbnails, proportional layout, source bindings and bounded scenes");
}
pub(crate) fn capture_render(app: &mut App, target: &Handle<Image>, path: &Path) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    // Remove only this generated capture. An old PNG must never satisfy a
    // new renderer check while an asynchronous readback is still pending.
    if path.exists() {
        std::fs::remove_file(path).expect("replace generated capture");
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(save_to_disk(path.to_path_buf()));
    for _ in 0..600 {
        app.update();
        gpu_flush(&app);
        if std::fs::metadata(path).is_ok_and(|m| m.len() > 1000) {
            let cache = app
                .sub_app(bevy::render::RenderApp)
                .world()
                .resource::<bevy::render::render_resource::PipelineCache>();
            for pipeline in cache.pipelines() {
                if let bevy::render::render_resource::CachedPipelineState::Err(error) =
                    &pipeline.state
                {
                    panic!("GPU capture has an unresolved pipeline: {error:?}");
                }
            }
            write_scene_provenance(app, path);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("fresh GPU capture did not complete: {}", path.display());
}

/// Record the resources of drawn actors, rather than inferring authenticity
/// from an index census. Unmatched meshes are explicitly counted as generated.
fn write_scene_provenance(app: &mut App, path: &Path) {
    let world = app.world_mut();
    let tribal_pets: Vec<_> = world
        .query::<(&crate::tribal_rts::livestock::DomesticAnimal, &Health, &Transform, Option<&crate::tribal_rts::livestock::PetGuard>)>()
        .iter(world)
        .map(|(pet, health, pose, guard)| serde_json::json!({
            "slot":pet.0,"health":health.hp,"max_health":health.max,
            "position":pose.translation.to_array(),"defending":guard.is_some_and(|g|g.target.is_some())
        }))
        .collect();
    let gameplay = {
        let run = world.resource::<RunProgress>();
        let state = world.resource::<State<GameStage>>().get();
        let originals = world.resource::<crate::Originals>();
        let abilities = crate::creature_actions::player_abilities(run, originals);
        let tribe = run.tribal_campaign.status_summary();
        serde_json::json!({
            "stage":format!("{state:?}"),
            "creature_mobility":world.get_resource::<crate::creature::mobility::CreatureMobility>().map(|mobility|serde_json::json!({
                "height_above_ground":mobility.height,"vertical_velocity":mobility.vertical_velocity,
                "sprinting":mobility.sprinting,"sneaking":mobility.sneaking,"gliding":mobility.gliding,
                "sprint_seconds":mobility.sprint_seconds,"sprint_cooldown":mobility.sprint_cooldown,"glide_seconds":mobility.glide_seconds,
                "ranks":{"jump":abilities.jump,"glide":abilities.glide,"sprint":abilities.sprint,"sneak":abilities.sneak}
            })),
            "tribal":{"food":run.dna_this_stage,"members":run.tribe.len(),
                "energy":run.tribal_campaign.member_energy,"average_energy":tribe.average_energy_percent,
                "hungry_members":tribe.hungry_members,"births":run.tribal_campaign.births,
                "livestock":run.tribal_campaign.livestock,"livestock_eggs":run.tribal_campaign.livestock_eggs,
                "pack_inherited":run.tribal_campaign.pack_inherited,"live_pets":tribal_pets,
                "friendly_stance":run.tribal_campaign.friendly_stance},
            "space":{"tool":format!("{:?}",run.space_campaign.planetary_tool),"worlds":run.space_campaign.ecology,
                "owned_worlds":run.space_campaign.colonies,"currency":run.spice},
            "limitation":"Documented controls and original part ranks drive adapted physics, growth, needs and ecology. Capture scenarios may stage a diagnostic loadout, energy or budget; this is not an original-game scenario comparison."
        })
    };
    let asteroid_sources = world
        .query::<(
            &crate::asteroid_visuals::NativeAsteroidBelt,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(source, material)| {
            (
                source.source,
                source.instances,
                source.frame,
                material.0.clone(),
            )
        })
        .collect::<Vec<_>>();
    let giant_sources = world
        .query::<(
            &crate::space_scene::OriginalGasGiantArt,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(source, material)| (source.source, source.image.clone(), material.0.clone()))
        .collect::<Vec<_>>();
    let original_pirates = world.query::<(Entity, &crate::pirate_visuals::OriginalPirateVisual)>()
        .iter(world).map(|(entity, source)| serde_json::json!({"entity":format!("{entity:?}"),
            "creation":crate::resource_audit::key_json(source.0),
            "primitive_removed":world.get::<Mesh3d>(entity).is_none(),
            "limitation":"Original genufo assembly adapted as pirate art; this is not a verified original pirate design."})).collect::<Vec<_>>();
    let preview_draws = world.query::<(Entity, &crate::mechanical_preview::PreviewDraw, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>()
        .iter(world).map(|(entity, draw, mesh, material)| serde_json::json!({
            "entity":format!("{entity:?}"),"source_entity":format!("{:?}",draw.source),
            "source_geometry_matches":world.get::<Mesh3d>(draw.source).is_some_and(|source|source.0==mesh.0),
            "source_material_matches":world.get::<MeshMaterial3d<StandardMaterial>>(draw.source).is_some_and(|source|source.0==material.0),
            "render_layer":30,
        })).collect::<Vec<_>>();
    let mechanical_creator = world.get_resource::<crate::mechanical_creator::MechanicalCreator>().map(|creator| serde_json::json!({
        "open":creator.open,"kind":format!("{:?}",creator.kind),"preview_draws":preview_draws,
        "limitation":"Adapted modal controls, framing and studio lighting over original preset geometry and saved added parts. Preset base blocks, drag/morph/paint tools and native creator/SPUI parity remain unfinished."
    }));
    let mechanical_environment = world
        .query::<(
            &crate::mechanical_environment::MechanicalEnvironmentSurface,
            &Transform,
        )>()
        .iter(world)
        .map(|(surface, transform)| {
            serde_json::json!({
                "definition":crate::resource_audit::key_json(surface.definition),
                "native_model":crate::resource_audit::key_json(surface.model),
                "source_section":surface.source_section,"role":format!("{:?}", surface.role),
                "material_mode":format!("{:?}",surface.material_mode),
                "translation":transform.translation.to_array(),"scale":transform.scale.to_array(),
            })
        })
        .collect::<Vec<_>>();
    let actors: Vec<_> = world
        .query_filtered::<(
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&Name>,
            Option<&GlobalTransform>,
            &InheritedVisibility,
            &ViewVisibility,
        ), Without<crate::part_thumbnails::ThumbnailGeometry>>()
        .iter(world)
        .filter(|(_, _, _, _, _, inherited, view)| inherited.get() && view.get())
        .map(|(e, m, mat, n, t, _, _)| {
            (
                e,
                m.0.clone(),
                mat.0.clone(),
                n.map(|n| n.as_str().to_owned()),
                t.copied(),
            )
        })
        .collect();
    let animation_joints = world.query::<&crate::ClipJoint>().iter(world).count();
    let thumbnail_images = world.query::<(&crate::part_thumbnails::ThumbnailRequest, &ImageNode, &ComputedNode, &Visibility, Option<&crate::part_thumbnails::NativePartIcon>)>().iter(world).map(|(request,image,node,visibility,native)| serde_json::json!({
        "definition":crate::resource_audit::key_json(request.definition),
        "native_icon":native.filter(|icon| icon.definition == request.definition).map(|icon| crate::resource_audit::key_json(icon.icon)),
        "image_handle":format!("{:?}",image.image.id()),"layout_size":node.size().to_array(),
        "visible":*visibility!=Visibility::Hidden,
        "target_dimensions":world.resource::<Assets<Image>>().get(&image.image).map(|i|[i.width(),i.height()]),
    })).collect::<Vec<_>>();
    let thumbnail_geometry = world.query::<(&crate::part_thumbnails::ThumbnailGeometry,&Mesh3d,&MeshMaterial3d<StandardMaterial>)>().iter(world).map(|(source,mesh,material)|serde_json::json!({
        "definition":crate::resource_audit::key_json(source.definition),"native_model":crate::resource_audit::key_json(source.model),"surface":source.surface,
        "mesh_present":world.resource::<Assets<Mesh>>().contains(mesh.0.id()),"material_present":world.resource::<Assets<StandardMaterial>>().contains(material.0.id()),
        "render_layer":31,"role":"offscreen part thumbnail; excluded from main scene mesh census",
    })).collect::<Vec<_>>();
    let thumbnail_cache = world.resource::<crate::part_thumbnails::PartThumbnailCache>();
    let thumbnail_report = serde_json::json!({"images":thumbnail_images,"geometry":thumbnail_geometry,"cached_images":thumbnail_cache.cached_images(),"live_scenes":thumbnail_cache.live_scenes()});
    let foliage_cutaway=world.query::<(&crate::foliage_visibility::FoliageFade,&MeshMaterial3d<StandardMaterial>)>().iter(world).map(|(fade,draw)| {
        let materials=world.resource::<Assets<StandardMaterial>>();
        serde_json::json!({"source":crate::resource_audit::key_json(fade.source),"factor":fade.factor,"actor_owns_material":draw.0==fade.owned && fade.original!=fade.owned,
            "source_alpha":materials.get(&fade.original).map(|m|m.base_color.alpha()),"draw_alpha":materials.get(&draw.0).map(|m|m.base_color.alpha()),
            "policy":"adapted camera cutaway; original textures and shared source material preserved"})
    }).collect::<Vec<_>>();
    let displayed_editor_art=world.query::<(&crate::editor_art::EditorArtSource,&ImageNode,&ComputedNode)>().iter(world).map(|(source,image,node)|serde_json::json!({"source":crate::resource_audit::key_json(source.key),"role":source.role,"image_handle":format!("{:?}",image.image.id()),"layout_size":node.size().to_array()})).collect::<Vec<_>>();
    let displayed_menu_galaxy=world.query::<(&crate::menu_visuals::NativeMenuGalaxy,&MeshMaterial3d<StandardMaterial>)>().iter(world).map(|(source,mat)|serde_json::json!({"source":crate::resource_audit::key_json(source.key),"dimensions":source.dimensions.to_array(),"bound":world.resource::<Assets<StandardMaterial>>().get(&mat.0).is_some_and(|m|m.base_color_texture.as_ref()==Some(&source.image)),"projection":"adapted billboard; native raster"})).collect::<Vec<_>>();
    let adapted_walk_surfaces=world.query::<(&crate::locomotion_preview::OwnedWalkSurface,&crate::player_appearance::NativePlayerSurface,&Mesh3d)>().iter(world).map(|(rest,source,draw)|serde_json::json!({"source":crate::resource_audit::key_json(source.0),"original_mesh":format!("{:?}",rest.original.id()),"actor_mesh":format!("{:?}",draw.0.id()),"separate_mesh":rest.original!=draw.0,"motion":"adapted cycle on native skin weights; original ANIM evaluation incomplete"})).collect::<Vec<_>>();

    let displayed_ui_art = world
        .query::<(&crate::player_hud::NativeHudArt, &ImageNode, &ComputedNode)>()
        .iter(world)
        .map(|(source, image, node)| {
            serde_json::json!({
                "source": crate::resource_audit::key_json(source.0),
                "image_handle":format!("{:?}",image.image.id()),
                "layout_size":node.size().to_array(),
                "crop":image.rect.map(|r|[r.min.x,r.min.y,r.max.x,r.max.y]),
                "color":image.color.to_srgba().to_f32_array(),
            })
        })
        .collect::<Vec<_>>();
    let displayed_cell_backgrounds = world
        .query::<(
            &crate::cell_environment::CellEnvironmentLayerSource,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(source, material)| {
            serde_json::json!({
                "source":crate::resource_audit::key_json(source.key),
                "kind":format!("{:?}",source.kind),"role":format!("{:?}",source.role),
                "image_handle":format!("{:?}",source.image.id()),
                "dimensions":source.dimensions,"material_handle":format!("{:?}",material.0.id())
            })
        })
        .collect::<Vec<_>>();
    let cloud_sources = world
        .query::<(
            &crate::world_presentation::OriginalCloudSource,
            &MeshMaterial3d<crate::world_presentation::PreviewSkyMaterial>,
        )>()
        .iter(world)
        .map(|(source, material)| (source.clone(), material.0.clone()))
        .collect::<Vec<_>>();
    let water_sources = world
        .query::<(
            &crate::water::OriginalWaterSources,
            &MeshMaterial3d<crate::water::WaterMaterial>,
        )>()
        .iter(world)
        .map(|(s, m)| (s.clone(), m.0.clone()))
        .collect::<Vec<_>>();
    let planet_sources = world
        .query::<(
            &crate::space_visuals::PlanetSurface,
            &MeshMaterial3d<StandardMaterial>,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
        .filter(|(_, _, inherited, view)| inherited.get() && view.get())
        .map(|(s, m, _, _)| (s.clone(), m.0.clone()))
        .collect::<Vec<_>>();
    let planet_cloud_sources = world
        .query::<(
            &crate::space_visuals::OriginalPlanetCloud,
            &MeshMaterial3d<StandardMaterial>,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
        .filter(|(_, _, inherited, view)| inherited.get() && view.get())
        .map(|(s, m, _, _)| (s.clone(), m.0.clone()))
        .collect::<Vec<_>>();
    let geyser_sources = world
        .query::<(
            &crate::spice_presentation::NativeSpiceGeyserDecal,
            &MeshMaterial3d<StandardMaterial>,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
        .filter(|(_, _, inherited, view)| inherited.get() && view.get())
        .map(|(s, m, _, _)| (s.clone(), m.0.clone()))
        .collect::<Vec<_>>();
    let terrain_sources = world
        .query::<(
            &crate::terrain::OriginalTerrainTexture,
            &MeshMaterial3d<crate::terrain::TerrainMaterial>,
        )>()
        .iter(world)
        .map(|(t, m)| (t.definition, t.texture, m.0.clone()))
        .collect::<Vec<_>>();
    let space_sources = world
        .query::<(
            &crate::space_visuals::OriginalSpaceBackdrop,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(s, m)| (s.clone(), m.0.clone()))
        .collect::<Vec<_>>();
    let distant_landforms = world
        .query::<(
            &crate::terrain::AdaptedDistantLandform,
            &MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(s, m)| (s.seed, s.palette, s.detail_texture, m.0.clone()))
        .collect::<Vec<_>>();
    let originals = world.resource::<crate::Originals>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let images = world.resource::<Assets<Image>>();
    let raster_audit = |source: crate::dbpf::ResourceKey,
                        handle: Option<&Handle<Image>>,
                        frame: Option<usize>| {
        let payload = originals.resource_index.read(source).ok();
        let native = payload.as_deref().and_then(crate::texture::decode_rw4_mips);
        let sampled = native.as_ref().and_then(|mips| match frame {
            Some(frame) => crate::asteroid_visuals::tile_mips(mips, frame).ok(),
            None => Some(mips.clone()),
        });
        let actual = handle.and_then(|handle| images.get(handle));
        serde_json::json!({
            "source":crate::resource_audit::key_json(source),
            "source_sha256":payload.as_ref().map(|bytes|crate::parity::sha256(bytes)),
            "source_pixels_match":sampled.as_ref().and_then(|mips|mips.first()).is_some_and(|pixels|
                actual.and_then(|image|image.data.as_ref()).is_some_and(|data|data.get(..pixels.rgba.len())==Some(pixels.rgba.as_slice()))),
            "source_mip_levels":native.as_ref().map(Vec::len),
            "selected_frame":frame,"sampled_mip_levels":sampled.as_ref().map(Vec::len),
            "uploaded_mip_levels":actual.map(|image|image.texture_descriptor.mip_level_count),
            "dimensions":actual.map(|image|[image.width(),image.height()]),
        })
    };
    let displayed_original_asteroids = asteroid_sources.iter().map(|(source,count,frame,material)| {
        let image = materials.get(material).and_then(|material| material.base_color_texture.as_ref());
        serde_json::json!({"raster":raster_audit(*source,image,Some(*frame)),"instances":count,"atlas_grid":[4,4],
            "geometry":"Batched camera-facing quads; belt positions, sizes, rotation and alpha composition are adapted."})
    }).collect::<Vec<_>>();
    let displayed_original_gas_giants = giant_sources.iter().map(|(source,image,material)| {
        serde_json::json!({"raster":raster_audit(*source,Some(image),None),
            "exact_image_bound":materials.get(material).is_some_and(|material|material.base_color_texture.as_ref()==Some(image)),
            "projection":"Original base raster pixels and mip chain; seeded selection and UV sphere are adapted. Native cloud/scattering/UV-distortion shaders remain unfinished."})
    }).collect::<Vec<_>>();
    let displayed_planets = planet_sources.iter().map(|(source,handle)| {
        let image = materials.get(handle).and_then(|m|m.base_color_texture.as_ref()).and_then(|h|images.get(h));
        serde_json::json!({"world_id":source.planet_id,"palette":crate::resource_audit::key_json(source.theme),
            "terrain_definition":crate::resource_audit::key_json(source.terrain_definition),
            "terrain_detail":source.terrain_texture.map(crate::resource_audit::key_json),
            "map_dimensions":image.map(|i|[i.width(),i.height()]),
            "surface":"Generated orbital geography and color map incorporating original palette/detail inputs; not an original baked planet or native generator."})
    }).collect::<Vec<_>>();
    let displayed_planet_clouds = planet_cloud_sources.iter().map(|(source,handle)| {
        serde_json::json!({"source":crate::resource_audit::key_json(source.key),"dimensions":source.dimensions.to_array(),
            "exact_image_bound":materials.get(handle).is_some_and(|m|m.base_color_texture.as_ref()==Some(&source.image)),
            "projection":"Adapted rotating sphere using original cloud_lit raster; native orbital cloud/weather generation unresolved."})
    }).collect::<Vec<_>>();
    let displayed_geysers = geyser_sources.iter().map(|(source,handle)| {
        serde_json::json!({"definition":crate::resource_audit::key_json(crate::spice_presentation::GEYSER_SOURCE),
            "decal":crate::resource_audit::key_json(source.source),"dimensions":source.dimensions,
            "stamp":crate::resource_audit::key_json(crate::spice_presentation::GEYSER_STAMP),
            "exact_image_bound":materials.get(handle).is_some_and(|m|m.base_color_texture.as_ref()==Some(&source.image)),
            "unresolved_inheritance":originals.resource_index.properties(crate::spice_presentation::GEYSER_SOURCE).err(),
            "geometry":"Adapted terrain-conforming mound from original height stamp; original crater decal. Native geyser particles unresolved."})
    }).collect::<Vec<_>>();
    let displayed_water = water_sources.iter().flat_map(|(sources, handle)| {
        let material = world.resource::<Assets<crate::water::WaterMaterial>>().get(handle);
        sources.0.iter().map(move |source| {
            let bound = material.map(|m| if source.key == crate::water::WATER_NORMAL { &m.extension.normal_texture }
                else if source.key == crate::water::FOAM_CUT { &m.extension.foam_texture } else { &m.extension.seabed_texture });
            let image = images.get(&source.image);
            serde_json::json!({"name":source.name,"source":crate::resource_audit::key_json(source.key),
                "dimensions":source.dimensions.to_array(),"linear":source.linear,
                "mip_levels":image.map(|i|i.texture_descriptor.mip_level_count),
                "exact_image_bound":bound==Some(&source.image),
                "projection":"Original raster pixels; adapted normal flow, seabed and shoreline foam. Native PCA/shader execution unresolved."})
        })
    }).collect::<Vec<_>>();
    let displayed_space_sources = space_sources.iter().map(|(source, material)| {
        let material = materials.get(material);
        serde_json::json!({"source":crate::resource_audit::key_json(source.key),
            "dimensions":source.dimensions,"image_present":images.get(&source.image).is_some(),
            "exact_image_bound":material.is_some_and(|m|m.base_color_texture.as_ref()==Some(&source.image)),
            "blend":material.map(|m|format!("{:?}",m.alpha_mode)),
            "projection":"Original raster pixels; adapted additive billboard placement and tint."})
    }).collect::<Vec<_>>();
    let distant_landform_sources = distant_landforms.iter().map(|(seed,palette,detail,handle)| {
        let image = materials.get(handle).and_then(|m|m.base_color_texture.as_ref()).and_then(|h|images.get(h));
        serde_json::json!({"seed":seed,"palette":crate::resource_audit::key_json(*palette),
            "detail_source":detail.map(crate::resource_audit::key_json),"image_present":image.is_some(),
            "geometry":"Generated cosmetic island; excluded from collision and native geometry counts."})
    }).collect::<Vec<_>>();
    let displayed_clouds = cloud_sources.iter().map(|(source, handle)| {
        let bound = world.resource::<Assets<crate::world_presentation::PreviewSkyMaterial>>().get(handle);
        serde_json::json!({
            "source":crate::resource_audit::key_json(source.key),
            "image_dimensions":source.dimensions.to_array(),
            "image_asset_present":images.get(&source.image).is_some(),
            "exact_image_bound":bound.is_some_and(|m|m.extension.cloud_texture == source.image),
            "controls":bound.map(|m|m.extension.cloud_controls.to_array()),
            "projection":"Adapted equirectangular sky; native cloud projection and motion are unverified."
        })
    }).collect::<Vec<_>>();
    let cell_assets = world.get_resource::<crate::cell_visuals::CellVisualAssets>().map(|a| serde_json::json!({
        "decoded_rules":a.decoded_rules,"resolved_species":a.species.len(),"original_body":a.body.as_ref().and_then(|b|b.source_definition).map(crate::resource_audit::key_json),
        "species":a.species.iter().map(|s|serde_json::json!({"rule":crate::resource_audit::key_json(s.rule.key),"model":crate::resource_audit::key_json(s.model_key),"awareness":s.rule.ai.awareness,"speed":s.rule.ai.speed,"turn":s.rule.ai.turn,"food_value":s.rule.food_value})).collect::<Vec<_>>()
    }));
    let cell_environment = world
        .get_resource::<crate::cell_environment::CellEnvironmentAssets>()
        .map(|assets| {
            serde_json::json!({
                "source_record_count": assets.sources.len(),
                "imported_backplate_count": assets.sources.iter().filter(|source| {
                    source.imported && matches!(source.name,
                        "cell_background_l0_screen" | "cell_background_l0_blob" | "cell_background_glowpatch")
                }).count(),
                "sources": assets.sources.iter().map(|source| serde_json::json!({
                    "name": source.name,
                    "key": crate::resource_audit::key_json(source.key),
                    "imported": source.imported,
                    "kind": format!("{:?}",source.kind),
                    "render_role": source.render_role.map(|r|format!("{r:?}")),
                    "image_dimensions": source.image_dimensions,
                    "image_asset_present": source.image.as_ref().is_some_and(|h|images.get(h).is_some()),
                    "diagnostic": source.diagnostic,
                    "bounds": source.bounds,
                    "footprint": source.footprint,
                    "fitted_scale": source.fitted_scale,
                })).collect::<Vec<_>>(),
                "native_background_colors": assets.native_background_colors,
                "native_background_distance": assets.native_background_distance,
                "palette_use": "Native colors are exposed as evidence; water interpolation and placement remain preview adaptations."
            })
        });
    let ui_art_assets = {
        let hint_refs = crate::ui_art::action_hint_icon_refs(&originals.resource_index);
        let hint_by_action = hint_refs
            .iter()
            .map(|hint| (hint.action, hint))
            .collect::<std::collections::HashMap<_, _>>();
        serde_json::json!({
            "texture_import_count": originals.ui_art.textures.len(),
            "textures": originals.ui_art.textures.iter().map(|texture| {
                let image = images.get(&texture.image);
                serde_json::json!({
                    "name": texture.name,
                    "source": crate::resource_audit::key_json(texture.source),
                    "width": texture.width,
                    "height": texture.height,
                    "asset_store_present": image.is_some(),
                })
            }).collect::<Vec<_>>(),
            "action_icon_import_count": originals.ui_art.action_icons.len(),
            "action_icons": originals.ui_art.action_icons.iter().map(|icon| {
                let image = images.get(&icon.image);
                let hint = hint_by_action.get(icon.action);
                serde_json::json!({
                    "action": icon.action,
                    "source": crate::resource_audit::key_json(icon.source),
                    "hint_prop_source": icon.hint_source.map(crate::resource_audit::key_json),
                    "native_hint_icon_reference": hint.and_then(|item| item.native_reference).map(crate::resource_audit::key_json),
                    "resolved_hint_texture_source": hint.and_then(|item| item.texture_source).map(crate::resource_audit::key_json),
                    "hint_resolution_error": hint.and_then(|item| item.error.as_deref()),
                    "width": icon.width,
                    "height": icon.height,
                    "asset_store_present": image.is_some(),
                })
            }).collect::<Vec<_>>(),
            "errors": originals.ui_art.errors,
            "evidence_scope": "Imported source-keyed images present in Bevy Assets<Image>; this manifest does not claim each image was displayed in the capture."
        })
    };
    let models: Vec<_> = originals
        .models
        .iter()
        .chain(originals.mechanical_models.values())
        .chain(originals.world_models.values())
        .chain(originals.baked_models.values())
        .chain(originals.baked_body.iter().map(|b| &b.model))
        .chain(
            originals
                .creation_models
                .values()
                .flat_map(|c| c.blocks.iter().map(|b| &b.mechanical.model)),
        )
        .collect();
    let mut rows = vec![];
    let mut generated = 0;
    let mut texture_rows = vec![];
    let mut texture_ids = std::collections::HashMap::new();
    let mut normal_rows = vec![];
    let mut normal_ids = std::collections::HashMap::new();
    let mut baked_keys = models
        .iter()
        .filter(|m| {
            actors.iter().any(|(_, mesh, _, _, _)| {
                m.handle == *mesh || m.surfaces.iter().any(|s| s.handle == *mesh)
            })
        })
        .filter_map(|m| m.source_definition)
        .filter(|k| k.type_id == crate::decode::TYPE_GMDL)
        .collect::<Vec<_>>();
    baked_keys.sort_by_key(|k| (k.group_id, k.instance_id));
    baked_keys.dedup();
    let original_species_sources: Vec<_> = baked_keys.iter().filter_map(|key| {
        let result = originals.original_species.get(key)?;
        Some(match result {
            Ok(native) => serde_json::json!({
                "display_model": crate::resource_audit::key_json(*key),
                "anatomy": crate::resource_audit::key_json(native.anatomy_key),
                "model_sha256": native.display_model_sha256,
                "anatomy_sha256": native.anatomy_sha256,
                "resolved": true,
                "part_definitions": native.parts.iter().map(|part| crate::resource_audit::key_json(part.definition)).collect::<Vec<_>>(),
                "social_ranks": crate::creature_actions::levels(native.capabilities, crate::creature_actions::EncounterMode::Social),
                "combat_ranks": crate::creature_actions::levels(native.capabilities, crate::creature_actions::EncounterMode::Combat),
                "health_rank": native.capabilities.health,
                "speed_rank": native.capabilities.speed,
                "native_capability_ids": native.native.capability_ids,
                "native_capability_levels": native.native.capability_levels,
                "limitation": "Exact original part capability properties; health/speed formulas, AI and encounters remain prototype adaptations."
            }),
            Err(error) => serde_json::json!({
                "display_model": crate::resource_audit::key_json(*key),
                "resolved": false, "error": error,
                "fallback": "generated species recipe capabilities"
            }),
        })
    }).collect();
    let baked_sources=baked_keys.into_iter().filter_map(|key|crate::baked_model::parse(&originals.resource_index.read(key).ok()?).ok().map(|m| {
        let layout = crate::animation_layout::resolve(&originals.resource_index, &m);
        serde_json::json!({"model":crate::resource_audit::key_json(key),"version":m.version,"body_bones":m.root_bones,
            "animation_bindings":m.animation_bindings_audit(),
            "animation_group_layout": match layout { Ok(layout) => serde_json::json!({"resolved":true,"layout":layout,"motion_active":false,"limitation":"Group slots and constructor transforms resolved; baked per-bone poses and procedural controllers remain unresolved."}), Err(error) => serde_json::json!({"resolved":false,"error":error,"motion_active":false}) },
            "meshes":m.pieces.iter().zip(&m.materials).map(|(p,mat)|serde_json::json!({"material_id":format!("{:08X}",p.material_id),"vertices":p.positions.len(),"triangles":p.indices.len()/3,"shader_parameters":mat.parameters,"samplers":mat.textures.iter().map(|t|serde_json::json!({"slot":t.sampler,"texture":crate::baked_model::texture_key(&originals.resource_index,t.key).ok().map(crate::resource_audit::key_json),"native_entry_group":format!("{:08X}",t.key.group_id),"native_entry_instance":format!("{:08X}",t.key.instance_id),"native_sampler_bytes":t.extra})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    })).collect::<Vec<_>>();
    let terrain_materials = world.resource::<Assets<crate::terrain::TerrainMaterial>>();
    let terrain_rows=terrain_sources.iter().enumerate().map(|(i,(definition,key,mat))| {
        let image=terrain_materials.get(mat).and_then(|m|images.get(&m.extension.detail_texture));
        let filename=format!("{}-terrain-{i:03}.png",path.file_stem().unwrap().to_string_lossy());
        let saved=image.and_then(|im|{let mut base=im.clone();base.texture_descriptor.mip_level_count=1;if let Some(data)=&mut base.data{data.truncate(im.width()as usize*im.height()as usize*4)}base.try_into_dynamic().ok()?.save(path.with_file_name(&filename)).ok()}).is_some();
        serde_json::json!({"definition":crate::resource_audit::key_json(*definition),"detail_texture":crate::resource_audit::key_json(*key),"property":"03B4F7C9","authored_mip_levels":image.map(|i|i.texture_descriptor.mip_level_count),"file":if saved{Some(filename)}else{None},"geometry":"generated terrain"})
    }).collect::<Vec<_>>();
    for (entity, mesh, material, name, transform) in actors {
        let imported_mesh = world
            .get::<crate::locomotion_preview::OwnedWalkSurface>(entity)
            .map(|owned| &owned.original)
            .unwrap_or(&mesh);
        let model = models.iter().find(|m| {
            &m.handle == imported_mesh || m.surfaces.iter().any(|s| &s.handle == imported_mesh)
        });
        let Some(model) = model else {
            generated += 1;
            continue;
        };
        let surface = model.surfaces.iter().find(|s| &s.handle == imported_mesh);
        let mat = materials.get(&material);
        let texture=mat.and_then(|m|m.base_color_texture.as_ref()).map(|h| {
            let next=texture_rows.len();
            *texture_ids.entry(h.id()).or_insert_with(|| {
                if let Some(image)=images.get(h) {
                    let filename=format!("{}-texture-{next:03}.png",path.file_stem().unwrap().to_string_lossy());
                    let mut png_image=image.clone();
                    png_image.texture_descriptor.mip_level_count=1;
                    if let Some(data)=&mut png_image.data {data.truncate(image.width() as usize*image.height() as usize*4);}
                    // Preserve RGBA bytes for the PNG encoder. This changes
                    // only an export clone's format, not the GPU texture.
                    if png_image.texture_descriptor.format==bevy::render::render_resource::TextureFormat::Rgba8Unorm {
                        png_image.texture_descriptor.format=bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
                    }
                    let saved=png_image.try_into_dynamic().ok().and_then(|i|i.save(path.with_file_name(&filename)).ok()).is_some();
                    let mean=image.data.as_ref().map(|data|{
                        let count=image.width()as usize*image.height()as usize;
                        let mut sum=[0u64;4];
                        for p in data.chunks_exact(4).take(count) {for c in 0..4 {sum[c]+=p[c] as u64;}}
                        sum.map(|v|v as f64/count.max(1) as f64)
                    });
                    texture_rows.push(serde_json::json!({"id":format!("{:?}",h.id()),"width":image.width(),"height":image.height(),"authored_mip_levels":image.texture_descriptor.mip_level_count,"format":format!("{:?}",image.texture_descriptor.format),"mean_rgba8":mean,"file":if saved {Some(filename)}else{None}}));
                } else {texture_rows.push(serde_json::json!({"missing":format!("{:?}",h.id())}));}
                next
            })
        });
        let normal_texture=mat.and_then(|m|m.normal_map_texture.as_ref()).map(|h| {
            let next=normal_rows.len();
            *normal_ids.entry(h.id()).or_insert_with(|| {
                if let Some(image)=images.get(h) {
                    let filename=format!("{}-normal-{next:03}.png",path.file_stem().unwrap().to_string_lossy());
                    let mut png_image=image.clone();
                    png_image.texture_descriptor.mip_level_count=1;
                    if let Some(data)=&mut png_image.data {data.truncate(image.width() as usize*image.height() as usize*4);}
                    if png_image.texture_descriptor.format==bevy::render::render_resource::TextureFormat::Rgba8Unorm {
                        png_image.texture_descriptor.format=bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
                    }
                    let saved=png_image.try_into_dynamic().ok().and_then(|i|i.save(path.with_file_name(&filename)).ok()).is_some();
                    normal_rows.push(serde_json::json!({"id":format!("{:?}",h.id()),"width":image.width(),"height":image.height(),"authored_mip_levels":image.texture_descriptor.mip_level_count,"format":format!("{:?}",image.texture_descriptor.format),"file":if saved {Some(filename)}else{None}}));
                } else {normal_rows.push(serde_json::json!({"missing":format!("{:?}",h.id())}));}
                next
            })
        });
        let definition = model.source_definition;
        let spec_bump_source = definition
            .and_then(|key| originals.part_catalog.parts.iter().find(|p| p.key == key))
            .and_then(|p| p.spec_bump);
        let world_source = world.get::<crate::world_objects::OriginalWorldObject>(entity);
        let native_model = world_source.map(|s| s.model).or_else(|| {
            definition
                .and_then(|k| {
                    if [crate::asset_catalog::RW4, crate::decode::TYPE_GMDL].contains(&k.type_id) {
                        Some(k)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    definition
                        .and_then(|k| {
                            originals
                                .resource_index
                                .properties(k)
                                .or_else(|_| crate::prop::parse(&originals.resource_index.read(k)?))
                                .ok()
                        })
                        .and_then(|p| p.0.get(&0x00f9efbb).and_then(|p| p.key()))
                })
        });
        let package = native_model
            .and_then(|k| originals.resource_index.records.get(&k))
            .map(|(p, _)| p.to_string_lossy().into_owned());
        rows.push(serde_json::json!({"entity":format!("{entity:?}"),"adapted_native_deformation":imported_mesh!=&mesh,"name":name,"world_object":world_source.map(|s|s.name.as_str()),"unresolved_inheritance":world_source.and_then(|s|originals.resource_index.properties(s.definition).err()),"definition":definition.map(crate::resource_audit::key_json),"native_model":native_model.map(crate::resource_audit::key_json),"package":package,"mesh_section":surface.map(|s|s.source_section),"state_sections":surface.map(|s|&s.state_sections),"combined_mesh":surface.is_none(),"alpha_mode":mat.map(|m|format!("{:?}",m.alpha_mode)),"base_color":mat.map(|m|m.base_color.to_linear().to_f32_array()),"diffuse_texture":texture,"normal_map":mat.is_some_and(|m|m.normal_map_texture.is_some()),"normal_texture":normal_texture,"authored_spec_bump_source":spec_bump_source.map(crate::resource_audit::key_json),"transform":transform.map(|t|t.to_matrix().to_cols_array())}));
    }
    let paint=originals.native_painter.as_ref().map(|p|serde_json::json!({"shader":"700000AD","normal_shader":"700000B2","copy_shader":"700000AF","pack":p.pack,"source":crate::resource_audit::key_json(p.source),"settings_source":p.parameters.source.map(crate::resource_audit::key_json),"bump_height":p.parameters.bump_height,"part_bump_scale":p.parameters.part_bump_scale,"version":format!("{:08X}",p.kernel.version),"execution":if crate::native_paint::enabled(){"Rust Direct3D9 HAL original bytecode"}else{"CPU pixel programs"},"gpu_counters":crate::native_paint::counters(),"partial_precision_advisory":!crate::native_paint::enabled() && p.kernel.partial_precision,"limitation":"Isolated part tint/normal passes; body paint and whole-creature atlas remain a preview. Scene rendering still uses Bevy PBR."}));
    let animation = originals.clips.first().and_then(|preview| preview.timed_clip.as_ref()).map(|clip|serde_json::json!({"clip_id":format!("{:08X}",clip.id),"source_path":clip.source_path,"flags":format!("{:08X}",clip.flags),"cyclic_neighbors":clip.flags&8!=0,"rotation_branch":"spherical cubic","evaluator":"native 005BF880 Hermite and quaternion curves","preview_joints":animation_joints,"limitation":"Generalized goal preview; native body binding, IK, locomotion and full timing policy remain incomplete. Runtime rotation-branch preference was not captured."}));
    let report = serde_json::json!({"gameplay":gameplay,"original_pirates":original_pirates,"displayed_original_asteroids":displayed_original_asteroids,"displayed_original_gas_giants":displayed_original_gas_giants,"mechanical_environment":mechanical_environment,"mechanical_creator":mechanical_creator,"part_thumbnails":thumbnail_report,"foliage_cutaway":foliage_cutaway,"adapted_walk_surfaces":adapted_walk_surfaces,"displayed_editor_art":displayed_editor_art,"displayed_menu_galaxy":displayed_menu_galaxy,"displayed_original_geyser_inputs":displayed_geysers,"displayed_orbital_planet_inputs":displayed_planets,"displayed_original_planet_clouds":displayed_planet_clouds,"displayed_original_water_rasters":displayed_water,"displayed_original_space_backdrops":displayed_space_sources,"adapted_distant_landforms":distant_landform_sources,"displayed_original_clouds":displayed_clouds,"original_species_sources":original_species_sources,"cell_assets":cell_assets,"cell_environment":cell_environment,"displayed_original_cell_backgrounds":displayed_cell_backgrounds,"original_ui_art":ui_art_assets,"displayed_original_ui_art":displayed_ui_art,"creature_abilities":originals.creature_abilities.audit(),"capture":path.file_name().unwrap().to_string_lossy(),"native_part_painter":paint,"native_animation_sampler":animation,"original_ui_font":world.resource::<crate::native_ui::OriginalFont>().source.map(crate::resource_audit::key_json),"baked_model_sources":baked_sources,"terrain_sources":terrain_rows,"original_mesh_entities":rows.len(),"generated_mesh_entities":generated,"originals":rows,"used_diffuse_textures":texture_rows,"used_normal_textures":normal_rows,"limitation":"Original complete GMDL geometry/atlases, body bone partition adapted to the editable spine, and native terrain detail/mips are rendered through Bevy material approximations. Planet geometry, full body animation/IK, shader and interface parity remain incomplete."});
    std::fs::write(
        path.with_extension("assets.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .expect("write capture asset provenance");
}

/// Verify actual mounted meshes and exact XML block identities before a capture.
fn assert_creation_scene(app: &mut App) {
    let current = stage(app);
    let world = app.world_mut();
    let roots: Vec<_> = world
        .query::<(Entity, &crate::creation_assets::OriginalCreation)>()
        .iter(world)
        .map(|(entity, source)| (entity, source.key))
        .collect();
    let native_halls: std::collections::HashSet<_> = world
        .query::<(&crate::civ::NativeCityHallSurface, &ChildOf)>()
        .iter(world)
        .map(|(_, parent)| parent.parent())
        .collect();
    let expected_roots = if current == GameStage::Civilization {
        assert_eq!(native_halls.len(), crate::civ::CITY_COUNT);
        // The player's own creation plus one hall per city slot (retail ten),
        // the halls mounted natively not counted.
        1 + crate::civ::CITY_COUNT - native_halls.len()
    } else {
        let pirate_count = world.query::<&crate::space::Pirate>().iter(world).count();
        let native_count = world
            .query::<&crate::pirate_visuals::OriginalPirateVisual>()
            .iter(world)
            .count();
        assert_eq!(
            pirate_count, native_count,
            "installed pirates must retire all red primitive bodies"
        );
        1 + native_count
    };
    assert_eq!(
        roots.len(),
        expected_roots,
        "default original assemblies must mount once"
    );
    for (root, key) in roots {
        assert!(world.entity(root).contains::<StageOwned>());
        let parent = world
            .get::<ChildOf>(root)
            .expect("creation belongs to a stage actor")
            .parent();
        assert!(world.entity(parent).contains::<StageOwned>());
        assert!(
            !world.entity(parent).contains::<Mesh3d>(),
            "successful preset must retire the primitive body"
        );
        let source = world
            .resource::<crate::Originals>()
            .creation_catalog
            .creations
            .iter()
            .find(|record| record.key == key)
            .expect("mounted source record");
        let mut expected = std::collections::HashMap::new();
        for block in &source.blocks {
            *expected.entry(block.definition).or_insert(0usize) += 1;
        }
        let mut actual = std::collections::HashMap::new();
        for (entity, part, mesh, material, parent) in world
            .query::<(
                Entity,
                &crate::creation_assets::OriginalCreationPart,
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
                &ChildOf,
            )>()
            .iter(world)
        {
            if parent.parent() != root {
                continue;
            }
            *actual.entry(part.definition).or_insert(0usize) += 1;
            assert!(world.entity(entity).contains::<StageOwned>());
            assert!(world.resource::<Assets<Mesh>>().contains(&mesh.0));
            assert!(world
                .resource::<Assets<StandardMaterial>>()
                .contains(&material.0));
        }
        assert_eq!(
            actual, expected,
            "every XML block must render with its exact definition"
        );
    }
    println!("PASS original construction XML block assemblies in {current:?}");
}

#[cfg(test)]
#[path = "gameplay_runtime_tests.rs"]
mod gameplay_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{capture, WorldId};

    #[test]
    #[ignore = "requires the original authored player GMDL and part catalog"]
    fn authored_player_display_restores_creator_and_preserves_custom_edits() {
        let mut app = installed_campaign_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<RunProgress>()
            .reset_to(GameStage::Creature);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        for _ in 0..5 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.diet_carnivore = run.diet_herbivore + 2.;
        }
        for _ in 0..3 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .reset_to(GameStage::Tribal);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Tribal);
        for _ in 0..5 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        {
            let world = app.world_mut();
            let expected = world.resource::<RunProgress>().tribe.len() + 1;
            assert_eq!(
                world
                    .query::<&crate::baked_model::NativePlayerSource>()
                    .iter(world)
                    .count(),
                expected
            );
            for (_, visibility) in world
                .query::<(&crate::creature::MountedPart, &Visibility)>()
                .iter(world)
            {
                assert_eq!(
                    *visibility,
                    Visibility::Hidden,
                    "do not double-draw starter attachments"
                );
            }
        }
        app.world_mut()
            .resource_mut::<RunProgress>()
            .reset_to(GameStage::Creature);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        for _ in 0..5 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        let actor = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .resource_mut::<crate::editor::EditorState>()
            .open = true;
        for _ in 0..3 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        app.world_mut()
            .resource_mut::<crate::editor::EditorState>()
            .open = false;
        for _ in 0..3 {
            step(&mut app);
        }
        assert_authored_player_surfaces(&mut app);
        app.world_mut().resource_mut::<RunProgress>().parts[0].scale *= 1.1;
        for _ in 0..3 {
            step(&mut app);
        }
        assert!(app.world().entity(actor).contains::<Mesh3d>());
        assert!(!app
            .world()
            .entity(actor)
            .contains::<crate::baked_model::NativePlayerSource>());
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let mut resumed = installed_campaign_app(serde_json::from_str(&saved).unwrap());
        resumed
            .world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        for _ in 0..4 {
            step(&mut resumed);
        }
        let world = resumed.world_mut();
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        assert!(
            world.entity(player).contains::<Mesh3d>(),
            "custom saved body must survive reload"
        );
        assert!(!world
            .entity(player)
            .contains::<crate::baked_model::NativePlayerSource>());
    }

    #[test]
    #[ignore = "requires original Spore data and a GPU"]
    fn render_original_install() {
        run_render_smoke(Path::new("target/render-smoke"));
    }

    #[test]
    #[ignore = "requires the original installed Spore data"]
    fn installed_ui_icons_and_cell_backplates_keep_native_source_evidence() {
        let mut app = installed_campaign_app(RunProgress::default());
        {
            let world = app.world();
            let originals = world.resource::<crate::Originals>();
            let images = world.resource::<Assets<Image>>();
            let imported = &originals.ui_art.action_icons;
            assert_eq!(
                imported.len(),
                crate::ui_art::ACTION_ICON_TEXTURES.len(),
                "expected every registered native action icon; importer errors: {:?}",
                originals.ui_art.errors
            );
            for (icon, expected) in imported.iter().zip(crate::ui_art::ACTION_ICON_TEXTURES) {
                assert_eq!(icon.action, expected.action);
                assert_eq!(
                    icon.source, expected.key,
                    "{} source key changed",
                    icon.action
                );
                let image = images
                    .get(&icon.image)
                    .unwrap_or_else(|| panic!("{} image handle was not imported", icon.action));
                let data = image
                    .data
                    .as_ref()
                    .unwrap_or_else(|| panic!("{} decoded image has no CPU pixels", icon.action));
                assert_eq!(image.width(), icon.width, "{} width", icon.action);
                assert_eq!(image.height(), icon.height, "{} height", icon.action);
                assert_eq!(
                    data.len(),
                    icon.width as usize * icon.height as usize * 4,
                    "{} RGBA byte count",
                    icon.action
                );
            }
        }

        let mut world = app.world_mut();
        let mut originals = world
            .remove_resource::<crate::Originals>()
            .expect("installed campaign originals");
        let mut meshes = world
            .remove_resource::<Assets<Mesh>>()
            .expect("headless mesh asset store");
        let mut images = world
            .remove_resource::<Assets<Image>>()
            .expect("headless image asset store");
        let mut materials = world
            .remove_resource::<Assets<StandardMaterial>>()
            .expect("headless material asset store");
        let environment = crate::cell_environment::prepare(
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        );
        world.insert_resource(originals);
        world.insert_resource(meshes);
        world.insert_resource(images);
        world.insert_resource(materials);
        world.insert_resource(environment);

        let environment = world.resource::<crate::cell_environment::CellEnvironmentAssets>();
        let backplates = environment
            .sources
            .iter()
            .filter(|source| {
                matches!(
                    source.name,
                    "cell_background_l0_screen"
                        | "cell_background_l0_blob"
                        | "cell_background_glowpatch"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            backplates.len(),
            3,
            "expected one attempt record per native backplate: {backplates:#?}"
        );
        let imported = backplates
            .iter()
            .copied()
            .filter(|source| source.imported)
            .collect::<Vec<_>>();
        assert!(
            imported.len() == 3,
            "no native Cell background texture imported; source diagnostics: {backplates:#?}"
        );
        let originals = world.resource::<crate::Originals>();
        for source in imported {
            assert_eq!(source.key.type_id, crate::asset_catalog::RW4);
            assert!(
                originals.resource_index.records.contains_key(&source.key),
                "imported backplate key is absent from ResourceIndex: {source:#?}"
            );
            assert!(
                source.kind == crate::cell_environment::CellEnvironmentSourceKind::Texture,
                "background raster incorrectly described as mesh: {source:#?}"
            );
            let image = source
                .image
                .as_ref()
                .expect("imported background image handle");
            let pixels = world
                .resource::<Assets<Image>>()
                .get(image)
                .expect("native background image retained");
            assert_eq!(
                Some([pixels.width(), pixels.height()]),
                source.image_dimensions
            );
        }
        if !environment.native_background_colors.is_empty() {
            assert!(environment
                .native_background_colors
                .iter()
                .flatten()
                .all(|value| { value.is_finite() && (0.0..=1.0).contains(value) }));
        }
    }

    pub(super) fn enter(app: &mut App, stage: GameStage) {
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(stage.clone());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(stage);
        step(app);
    }

    #[test]
    fn gameplay_full_arc() {
        // The Cell levels come from the installed game (skipped on a machine without Spore).
        if crate::spore_install::discover().is_err() {
            return;
        }
        check_full_arc(&mut headless_app(RunProgress::default()));
    }

    #[test]
    fn home_rest_mating_and_death_survive_actual_schedule_and_reload() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let home = Vec3::from_array(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .home,
        );
        teleport(&mut app, home);
        let world = app.world_mut();
        for mut npc in world.query::<&mut Npc>().iter_mut(world) {
            npc.hostile = false;
        }
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        // Resting at the nest heals 7.5% of the maximum HP per second (retail
        // creatureHealingHealthRecovery), so the retail 10 HP avatar gains 0.75.
        let max = world.get::<Health>(player).unwrap().max;
        world.get_mut::<Health>(player).unwrap().hp = max * 0.3;
        press(&mut app, KeyCode::KeyH);
        for _ in 0..60 {
            step(&mut app);
        }
        assert!(app.world().get::<Health>(player).unwrap().hp > max * 0.3 + max * 0.06);
        app.world_mut().get_mut::<Health>(player).unwrap().hp = max;
        press(&mut app, KeyCode::KeyM);
        assert!(app.world().resource::<crate::editor::EditorState>().open);
        assert!(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .mating
        );
        let before = app
            .world()
            .resource::<RunProgress>()
            .creature_lifecycle
            .mating_cooldown;
        for _ in 0..20 {
            step(&mut app);
        }
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .mating_cooldown,
            before
        );
        press(&mut app, KeyCode::F2);
        step(&mut app);
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .generation,
            2
        );
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let restored: RunProgress = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.creature_lifecycle.generation, 2);
        app.world_mut().get_mut::<Health>(player).unwrap().hp = 0.;
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_lifecycle
            .resting = true;
        step(&mut app);
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .deaths,
            1
        );
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .generation,
            2
        );
        assert!(app.world().get::<Health>(player).unwrap().hp > 0.);
    }

    #[test]
    fn creature_migration_persists_destination_and_keeps_cleared_outcome() {
        let mut run = RunProgress::default();
        run.reset_to(GameStage::Creature);
        // The first migration unlocks at the stage total 150 (retail).
        run.dna_this_stage = 150.0;
        run.dna_banked_total = 150.0;
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Creature);
        {
            let world = app.world_mut();
            let nest = world
                .query_filtered::<&Transform, With<crate::creature_home::HomeNest>>()
                .single(world)
                .unwrap();
            let expected = world
                .resource::<crate::terrain::LandSurface>()
                .height(nest.translation.x, nest.translation.z)
                + 0.08;
            assert!(
                (nest.translation.y - expected).abs() < 1e-5,
                "home nest must be grounded exactly once"
            );
        }
        assert!(
            app.world()
                .resource::<RunProgress>()
                .creature_progression
                .initialized
        );
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .creature_progression
                .brain_level,
            1
        );

        let origin = app
            .world()
            .resource::<RunProgress>()
            .creature_lifecycle
            .home;
        let destination =
            {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                let index = run
                    .species_world
                    .nests
                    .iter()
                    .position(|nest| {
                        crate::creature_lifecycle::CreatureLifecycle {
                            home: origin,
                            ..Default::default()
                        }
                        .at_home(nest.position)
                            == false
                    })
                    .expect("generated Creature world has an alternate nest");
                let nest = &mut run.species_world.nests[index];
                nest.members.clear();
                assert_eq!(nest.surviving(), 0);
                // The avatar claims a nest from 50 m (0xD41510): keep the cleared
                // one farther than that from home so the migration is a journey.
                let (dx, dz) = (nest.position[0] - origin[0], nest.position[2] - origin[2]);
                if (dx * dx + dz * dz).sqrt() < 70.0 {
                    nest.position = [origin[0] + 70.0, nest.position[1], origin[2]];
                }
                let destination = nest.position;
                assert!(run.species_world.nests.iter().enumerate().all(|(i, nest)| {
                i == index || nest.surviving() > 0
            }), "the selected cleared nest must be the only zero-population migration destination");
                destination
            };
        teleport(&mut app, Vec3::from_array(origin));
        // Retail has no migrate key: the avatar's birth at its nest (0xD40FA0, here
        // after a death) reads `MigrationDNALevels` and starts the migration.
        press(&mut app, KeyCode::KeyV);
        assert!(
            app.world()
                .resource::<RunProgress>()
                .creature_progression
                .pending_migration
                .is_none(),
            "V is no migrate key"
        );
        let hero = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .expect("one player")
        };
        app.world_mut().get_mut::<Health>(hero).unwrap().hp = 0.;
        step(&mut app);
        step(&mut app);
        assert_eq!(app.world().resource::<RunProgress>().creature_lifecycle.deaths, 1);
        let pending = app
            .world()
            .resource::<RunProgress>()
            .creature_progression
            .pending_migration
            .expect("the birth after a death at DNA 150 starts a migration to the cleared nest");
        assert_eq!(
            app.world().resource::<RunProgress>().creature_progression.last_migration_dna,
            150.0,
            "the migration records the DNA it started at"
        );
        assert_eq!(pending.origin, origin);
        assert_eq!(pending.destination, destination);

        // The pending destination is part of RunProgress, independent of the
        // world checkpoint. Reconstruct a cold app from its serialized save.
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let restored: RunProgress = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored
                .creature_progression
                .pending_migration
                .unwrap()
                .destination,
            destination
        );
        let mut resumed = headless_app(restored);
        enter(&mut resumed, GameStage::Creature);
        assert_eq!(
            resumed
                .world()
                .resource::<RunProgress>()
                .creature_progression
                .pending_migration
                .unwrap()
                .destination,
            destination,
            "Creature setup must not replace the saved migration"
        );
        let cleared = resumed
            .world()
            .resource::<RunProgress>()
            .species_world
            .nests
            .iter()
            .find(|nest| nest.position == destination)
            .expect("cleared nest survives reload");
        assert!(
            cleared.members.is_empty(),
            "cleared outcome was resurrected"
        );

        // Arrival is checked in XZ: the player's height differs greatly from
        // the destination record, as it can while traversing uneven terrain.
        teleport(
            &mut resumed,
            Vec3::new(destination[0], destination[1] + 40.0, destination[2]),
        );
        step(&mut resumed);
        {
            let run = resumed.world().resource::<RunProgress>();
            assert_eq!(run.creature_lifecycle.home, destination);
            assert_eq!(run.creature_progression.migrations_completed, 1);
            assert_eq!(run.creature_progression.pending_migration, None);
        }
        step(&mut resumed);
        assert_eq!(
            resumed
                .world()
                .resource::<RunProgress>()
                .creature_progression
                .migrations_completed,
            1,
            "arrival only completes the migration once"
        );

        // Reload the completed lineage too, proving the destination and cleared
        // nest outcome survive a second stage reconstruction.
        let saved = serde_json::to_string(resumed.world().resource::<RunProgress>()).unwrap();
        let restored: RunProgress = serde_json::from_str(&saved).unwrap();
        let mut resumed = headless_app(restored);
        enter(&mut resumed, GameStage::Creature);
        {
            let run = resumed.world().resource::<RunProgress>();
            assert_eq!(run.creature_lifecycle.home, destination);
            assert_eq!(run.creature_progression.migrations_completed, 1);
            assert!(run.creature_progression.pending_migration.is_none());
            assert!(run
                .species_world
                .nests
                .iter()
                .find(|nest| nest.position == destination)
                .expect("migrated nest remains in the saved world")
                .members
                .is_empty());
        }

        let player = {
            let world = resumed.world_mut();
            world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .expect("one migrated player")
        };
        resumed.world_mut().get_mut::<Health>(player).unwrap().hp = 30.0;
        press(&mut resumed, KeyCode::KeyH);
        assert!(
            resumed
                .world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .resting
        );
        resumed.world_mut().get_mut::<Health>(player).unwrap().hp = 120.0;
        press(&mut resumed, KeyCode::KeyM);
        assert!(
            resumed
                .world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .mating
        );
        assert!(
            resumed
                .world()
                .resource::<crate::editor::EditorState>()
                .open
        );
        press(&mut resumed, KeyCode::F2);
        step(&mut resumed);
        let run = resumed.world().resource::<RunProgress>();
        assert_eq!(run.creature_lifecycle.generation, 2);
        assert_eq!(run.creature_progression.migrations_completed, 1);
        assert_eq!(run.creature_lifecycle.home, destination);
        assert!(run.creature_progression.pending_migration.is_none());
        assert!(run
            .species_world
            .nests
            .iter()
            .find(|nest| nest.position == destination)
            .expect("cleared nest remains after mating")
            .members
            .is_empty());
    }

    /// Retail WASD (C6B): W walks along the creature's own facing whatever the
    /// camera does, and A/D turn the creature (replaces the earlier
    /// camera-relative scheme this test used to pin).
    #[test]
    fn creature_w_walks_along_the_creature_facing_and_ignores_the_orbit_camera() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let home = Vec3::from_array(
            app.world()
                .resource::<RunProgress>()
                .creature_lifecycle
                .home,
        );
        teleport(&mut app, home);
        {
            let mut camera = app
                .world_mut()
                .resource_mut::<crate::play_camera::PlayCamera>();
            camera.input_delta(Vec2::new(300.0, 0.0), 0.0, true, false);
        }
        let camera_forward = app
            .world()
            .resource::<crate::play_camera::PlayCamera>()
            .rig()
            .goal_azimuth();
        assert!(camera_forward.abs() > 0.5, "the camera was orbited");
        let player = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .expect("one Creature player")
        };
        let before = app.world().get::<Transform>(player).unwrap().clone();
        let facing = before.rotation * Vec3::NEG_Z;
        let facing = Vec3::new(facing.x, 0.0, facing.z).normalize();

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..20 {
            step(&mut app);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);

        let transform = app.world().get::<Transform>(player).unwrap();
        let delta = transform.translation - before.translation;
        let horizontal_delta = Vec3::new(delta.x, 0.0, delta.z).normalize();
        assert!(
            horizontal_delta.dot(facing) > 0.999,
            "W moved {horizontal_delta:?}, expected the facing {facing:?}"
        );
        let after = transform.rotation * Vec3::NEG_Z;
        let after = Vec3::new(after.x, 0.0, after.z).normalize();
        assert!(after.dot(facing) > 0.999, "W does not turn the creature");
    }

    #[test]
    fn missing_mechanical_catalog_keeps_saved_builds_and_explains_refused_edits() {
        let mut run = RunProgress::default();
        run.vehicle.parts.push(VehiclePart::new(0x1111, 2));
        run.building.parts.push(VehiclePart::new(0x2222, 1));
        run.ship.modules.push(ShipModule::new(0x3333, 3));
        let vehicle = run.vehicle.clone();
        let building = run.building.clone();
        let ship = run.ship.clone();
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Civilization);
        press(&mut app, KeyCode::KeyN);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.vehicle, vehicle);
        assert!(run
            .civ_campaign
            .message
            .contains("no usable original parts"));
        press(&mut app, KeyCode::KeyV);
        press(&mut app, KeyCode::KeyN);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.building, building);
        assert!(run
            .civ_campaign
            .message
            .contains("no usable original parts"));
        enter(&mut app, GameStage::Space);
        press(&mut app, KeyCode::KeyN);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.ship, ship);
        assert!(run
            .space_campaign
            .ship_message
            .contains("no unmounted original vehicle part"));
    }

    #[test]
    fn missing_outfit_catalog_preserves_saved_identity_and_allows_removal() {
        let mut run = RunProgress::default();
        run.tribe[1] = MateAssignment {
            role: 2,
            part: 0x1234,
            outfit_definition: Some(0x5678),
        };
        run.focused_part = 1;
        let saved = run.tribe.clone();
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        press(&mut app, KeyCode::KeyN);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribe, saved);
        assert!(run.tribal_campaign.message.contains("No original outfits"));
        assert!(
            crate::tribal::tribal_editor_hud(run, app.world().resource::<crate::Originals>())
                .contains("unsupported outfit 00005678")
        );
        press(&mut app, KeyCode::Backspace);
        let world = app.world_mut();
        let run = world.resource::<RunProgress>();
        assert_eq!(run.tribe[0], saved[0]);
        assert_eq!(run.tribe[2], saved[2]);
        assert_eq!(run.tribe[1].role, 2);
        assert_eq!(run.tribe[1].part, 0);
        assert_eq!(run.tribe[1].outfit_definition, None);
        assert_eq!(
            world
                .query_filtered::<Entity, With<TribeMate>>()
                .iter(world)
                .count(),
            saved.len()
        );
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_npc_capabilities_follow_displayed_anatomy_through_reload() {
        let mut app = installed_campaign_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        assert_native_creature_surfaces(&mut app);
        let members: Vec<_> = {
            let world = app.world_mut();
            world
                .query_filtered::<(&WorldId, &Npc), With<crate::baked_model::NativeCreature>>()
                .iter(world)
                .map(|(id, npc)| (id.0, npc.speed, npc.hp))
                .collect()
        };
        assert!(!members.is_empty());
        let mut resolved = 0;
        for (id, speed, hp) in &members {
            let world = app.world();
            let run = world.resource::<RunProgress>();
            let originals = world.resource::<crate::Originals>();
            let species = run.species_world.species_for_member(*id).unwrap();
            let Some(native) = crate::creature::original_build(species, originals) else {
                continue;
            };
            resolved += 1;
            assert_eq!(
                native.anatomy_key.instance_id,
                native.display_model.instance_id
            );
            assert_eq!(
                crate::creature_actions::member_abilities(run, originals, *id),
                native.capabilities
            );
            assert_eq!(
                (*speed, *hp),
                (
                    crate::creature::displayed_member_stats(species, originals).0,
                    crate::creature::member_max_hp(&run.species_world, *id, originals).unwrap()
                )
            );
        }
        assert!(
            resolved > 0,
            "installed scene must use verified native NPC abilities"
        );
        // A conflicting fallback build must not change a displayed original's
        // skills, health or speed. The source anatomy is authoritative.
        for species in &mut app
            .world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .species
        {
            species.parts.clear();
        }
        let id = members[0].0;
        let entity = creature_entity(&mut app, id);
        let hp_before = app.world().get::<Npc>(entity).unwrap().hp * 0.5;
        app.world_mut().get_mut::<Npc>(entity).unwrap().hp = hp_before;
        capture(app.world_mut());
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let mut resumed = installed_campaign_app(serde_json::from_str(&saved).unwrap());
        enter(&mut resumed, GameStage::Creature);
        assert_native_creature_surfaces(&mut resumed);
        let entity = creature_entity(&mut resumed, id);
        let npc = resumed.world().get::<Npc>(entity).unwrap();
        assert!(
            (npc.hp - hp_before).abs() < 0.001,
            "partial health must survive native-stat reload"
        );
        let world = resumed.world();
        let run = world.resource::<RunProgress>();
        let originals = world.resource::<crate::Originals>();
        for (id, _, _) in members {
            let species = run.species_world.species_for_member(id).unwrap();
            if let Some(native) = crate::creature::original_build(species, originals) {
                assert_eq!(
                    crate::creature_actions::member_abilities(run, originals, id),
                    native.capabilities
                );
                println!(
                    "NPC {id}: original model {:08X}, anatomy {:08X}, {} exact part definitions",
                    native.display_model.instance_id,
                    native.anatomy_key.instance_id,
                    native.parts.len()
                );
            }
        }
        println!("PASS native displayed NPC capabilities, conflicting fallback recipe, and partial-health reload");
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_social_refresh_allows_complete_encounter_through_gameplay_schedule() {
        use crate::{
            creature::Wander,
            creature_actions::{CreatureMind, Encounter},
        };
        let mut app = installed_campaign_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let social_parts: Vec<_> = app
            .world()
            .resource::<crate::Originals>()
            .part_catalog
            .parts
            .iter()
            .filter(|p| {
                p.key.group_id == crate::asset_catalog::CREATURE_PARTS
                    && crate::creature_actions::levels(
                        p.abilities,
                        crate::creature_actions::EncounterMode::Social,
                    )
                    .iter()
                    .any(|&rank| rank > 0)
            })
            .map(|p| PartInstance::new(p.model.instance_id, 0))
            .collect();
        assert!(!social_parts.is_empty());
        let target_id = {
            let world = app.world();
            let run = world.resource::<RunProgress>();
            let originals = world.resource::<crate::Originals>();
            run.species_world
                .spawn_list()
                .iter()
                .find(|spawn| {
                    let ranks = crate::creature_actions::levels(
                        crate::creature_actions::member_abilities(run, originals, spawn.member),
                        crate::creature_actions::EncounterMode::Social,
                    );
                    ranks.iter().any(|&rank| rank > 0)
                })
                .expect("original NPC with a social ability")
                .member
        };
        {
            let world = app.world_mut();
            let mut run = world.resource_mut::<RunProgress>();
            run.parts = social_parts;
            for species in &mut run.species_world.species {
                species.hostile = false;
            }
            for (id, mut transform, mut npc, mut wander) in world
                .query::<(&WorldId, &mut Transform, &mut Npc, &mut Wander)>()
                .iter_mut(world)
            {
                transform.translation = if id.0 == target_id {
                    Vec3::new(0., 0.4, 1.5)
                } else {
                    Vec3::new(20., 0.4, 20.)
                };
                npc.hostile = false;
                wander.wait = 1000.;
            }
        }
        // Roamers stay out of it (a rogue's territory can reach the centre).
        {
            let world = app.world_mut();
            for mut t in world
                .query_filtered::<&mut Transform, With<crate::epic_creatures::RoamerActor>>()
                .iter_mut(world)
            {
                t.translation = Vec3::new(100., 0., 100.);
            }
        }
        teleport(&mut app, Vec3::new(0., 0.7, 0.));
        press(&mut app, KeyCode::KeyF);
        assert_eq!(app.world().resource::<Encounter>().target, Some(target_id));
        let target = creature_entity(&mut app, target_id);
        for _ in 0..2400 {
            let mind = app.world().get::<CreatureMind>(target).unwrap();
            if mind.allied {
                break;
            }
            let slot = mind.requested as usize;
            if app.world().resource::<Encounter>().cooldowns[slot] <= 0. {
                press(
                    &mut app,
                    [
                        KeyCode::Digit1,
                        KeyCode::Digit2,
                        KeyCode::Digit3,
                        KeyCode::Digit4,
                    ][slot],
                );
            } else {
                step(&mut app);
            }
        }
        assert!(
            app.world().get::<CreatureMind>(target).unwrap().allied,
            "native refresh values must still permit a complete social encounter"
        );
        // Retail reward at brain level 0: the herd archetype's
        // evoPointsRewardedSocial (+ the social bonus for the player's share of
        // the bar above one half), strength = the archetype's FoodChainLevel.
        let expected = {
            let world = app.world();
            let run = world.resource::<RunProgress>();
            let mind = world.get::<CreatureMind>(target).unwrap();
            let archetype = run.species_world.archetype_for(target_id).unwrap();
            crate::creature_ecology::befriend_dna(
                archetype,
                run.species_world.member_role(target_id),
                0,
                archetype.record().food_chain as i32,
                crate::creature_rules::Difficulty::Medium,
                mind.player_fill / mind.bar_max,
            )
        };
        assert_eq!(app.world().resource::<RunProgress>().dna_this_stage, expected);
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let restored: RunProgress = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.species_world.relationship(target_id),
            Some(crate::species::Relationship::Allied)
        );
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_part_catalog_full_campaign() {
        let mut app = installed_campaign_app(RunProgress::default());
        check_full_arc(&mut app);
        assert!(app.world().resource::<crate::Originals>().models.len() >= 10);
        println!("PASS original part diets, abilities and imports through the full campaign");
    }

    fn original_mechanical_instance(app: &mut App, group: u32) -> u32 {
        original_catalog_definition(app, group, false)
            .model
            .instance_id
    }

    fn original_catalog_definition(
        app: &mut App,
        group: u32,
        variant: bool,
    ) -> crate::asset_catalog::PartDefinition {
        let world = app.world_mut();
        let mut originals = world.remove_resource::<crate::Originals>().unwrap();
        let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
        let mut images = world.remove_resource::<Assets<Image>>().unwrap();
        let mut materials = world.remove_resource::<Assets<StandardMaterial>>().unwrap();
        let mut seen = std::collections::HashSet::new();
        let definitions: Vec<_> = crate::mechanical_assets::eligible(&originals, group)
            .into_iter()
            .filter(|d| {
                let first = seen.insert(d.model.instance_id);
                first != variant
            })
            .cloned()
            .collect();
        let found = definitions.iter().find_map(|d| {
            crate::mechanical_assets::import(
                &mut originals,
                d,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .ok()
            .map(|_| d.clone())
        });
        world.insert_resource(originals);
        world.insert_resource(meshes);
        world.insert_resource(images);
        world.insert_resource(materials);
        found.expect("the original catalog has a supported mechanical model")
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_mechanical_builds_reload_without_preview_cache() {
        let mut app = installed_campaign_app(RunProgress::default());
        let vehicle = original_mechanical_instance(&mut app, crate::asset_catalog::VEHICLE_PARTS);
        let building = original_mechanical_instance(&mut app, crate::asset_catalog::BUILDING_PARTS);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.vehicle.parts.push(VehiclePart::new(vehicle, 0));
            run.building.parts.push(VehiclePart::new(building, 0));
            run.ship.modules.push(ShipModule::new(vehicle, 0));
        }
        enter(&mut app, GameStage::Civilization);
        capture(app.world_mut());
        let loaded: RunProgress = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = installed_campaign_app(loaded);
        assert!(resumed
            .world()
            .resource::<crate::Originals>()
            .models
            .is_empty());
        assert!(resumed
            .world()
            .resource::<crate::Originals>()
            .mechanical_models
            .is_empty());
        enter(&mut resumed, GameStage::Civilization);
        assert_mechanical_mount(&mut resumed, crate::asset_catalog::VEHICLE_PARTS, vehicle);
        assert_mechanical_mount(&mut resumed, crate::asset_catalog::BUILDING_PARTS, building);
        assert_eq!(
            resumed
                .world()
                .resource::<RunProgress>()
                .building
                .parts
                .len(),
            1
        );
        enter(&mut resumed, GameStage::Space);
        capture(resumed.world_mut());
        let loaded = serde_json::from_str(
            &serde_json::to_string(resumed.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut ship_resumed = installed_campaign_app(loaded);
        assert!(ship_resumed
            .world()
            .resource::<crate::Originals>()
            .mechanical_models
            .is_empty());
        enter(&mut ship_resumed, GameStage::Space);
        assert_mechanical_mount(
            &mut ship_resumed,
            crate::asset_catalog::VEHICLE_PARTS,
            vehicle,
        );
        println!("PASS original vehicle, building and ship builds reload with a cold model cache");
    }

    fn assert_outfit_wearer(app: &mut App, key: crate::dbpf::ResourceKey, expected_id: u32) {
        assert_definition_mount(app, key);
        let world = app.world_mut();
        let mesh = world
            .resource::<crate::Originals>()
            .mechanical_models
            .get(&key)
            .unwrap()
            .draw_mesh();
        let parents: Vec<_> = world
            .query::<(&Mesh3d, &ChildOf)>()
            .iter(world)
            .filter(|(m, _)| m.0 == mesh)
            .map(|(_, parent)| parent.parent())
            .collect();
        assert_eq!(parents.len(), 1, "one assigned outfit must have one wearer");
        let mut ancestor = parents[0];
        let mut wearer = None;
        for _ in 0..64 {
            if let Some(id) = world.entity(ancestor).get::<WorldId>() {
                wearer = Some(id.0);
                break;
            }
            let Some(parent) = world.entity(ancestor).get::<ChildOf>() else {
                break;
            };
            ancestor = parent.parent();
        }
        assert_eq!(wearer, Some(expected_id));
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_tribal_outfit_reload_and_member_loss_preserve_the_wearer() {
        let mut app = installed_campaign_app(RunProgress::default());
        let definition =
            original_catalog_definition(&mut app, crate::asset_catalog::TOOL_PARTS, true);
        let outfit = definition.model.instance_id;
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.slots = vec![11, 0, 4];
            run.tribe[2].part = outfit;
            run.tribe[2].outfit_definition = Some(definition.key.instance_id);
            run.tribe[2].role = 2;
        }
        enter(&mut app, GameStage::Tribal);
        assert_outfit_wearer(&mut app, definition.key, 204);
        capture(app.world_mut());
        let loaded = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = installed_campaign_app(loaded);
        assert!(resumed
            .world()
            .resource::<crate::Originals>()
            .models
            .is_empty());
        assert!(resumed
            .world()
            .resource::<crate::Originals>()
            .mechanical_models
            .is_empty());
        enter(&mut resumed, GameStage::Tribal);
        assert_outfit_wearer(&mut resumed, definition.key, 204);
        enter(&mut resumed, GameStage::MainMenu);
        enter(&mut resumed, GameStage::Tribal);
        assert_outfit_wearer(&mut resumed, definition.key, 204);
        let world = resumed.world();
        let model = world
            .resource::<crate::Originals>()
            .mechanical_models
            .get(&definition.key)
            .unwrap();
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(model.material.as_ref().unwrap())
            .unwrap();
        assert!(world.resource::<Assets<Image>>().contains(
            material
                .base_color_texture
                .as_ref()
                .expect("original outfit diffuse texture is decoded")
        ));
        let world = resumed.world_mut();
        for (id, mut hp) in world.query::<(&WorldId, &mut Health)>().iter_mut(world) {
            if id.0 == 211 {
                hp.hp = 0.;
            }
        }
        step(&mut resumed);
        assert_eq!(
            resumed.world().resource::<RunProgress>().tribe[0].part,
            outfit
        );
        assert_eq!(
            resumed.world().resource::<RunProgress>().tribe[0].outfit_definition,
            Some(definition.key.instance_id)
        );
        assert_outfit_wearer(&mut resumed, definition.key, 204);
        println!("PASS original outfit reload and roster compaction preserve the actual wearer");
    }

    #[test]
    #[ignore = "requires original Spore data"]
    fn original_creation_defaults_reload_without_granting_gameplay_parts() {
        let mut app = installed_campaign_app(RunProgress::default());
        let originals = app.world().resource::<crate::Originals>();
        let records = &originals.creation_catalog.creations;
        assert_eq!(records.iter().map(|r| r.blocks.len()).sum::<usize>(), 112);
        assert_eq!(
            records
                .iter()
                .flat_map(|r| &r.blocks)
                .map(|b| b.handles.len())
                .sum::<usize>(),
            243
        );
        assert_eq!(
            records
                .iter()
                .flat_map(|r| &r.blocks)
                .map(|b| b.paints.len())
                .sum::<usize>(),
            195
        );
        for block in records.iter().flat_map(|record| &record.blocks) {
            assert!(originals
                .part_catalog
                .parts
                .iter()
                .any(|definition| definition.key == block.definition));
        }
        assert_eq!(
            app.world()
                .resource::<crate::Originals>()
                .creation_catalog
                .creations
                .len(),
            7
        );
        assert!(app
            .world()
            .resource::<crate::Originals>()
            .creation_catalog
            .errors
            .is_empty());
        // Audit all seven records, including the sea/air presets and paint
        // swatches that aren't current campaign defaults. No ECS or build grants.
        let world = app.world_mut();
        let mut originals = world.remove_resource::<crate::Originals>().unwrap();
        let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
        let mut images = world.remove_resource::<Assets<Image>>().unwrap();
        let mut materials = world.remove_resource::<Assets<StandardMaterial>>().unwrap();
        for record in originals.creation_catalog.creations.clone() {
            let imported = crate::creation_assets::import_creation(
                &mut originals,
                &record,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .unwrap_or_else(|error| panic!("original {:?} {}: {error}", record.key, record.name));
            assert_eq!(imported.blocks.len(), record.blocks.len());
            assert!(imported.local_min.is_finite() && imported.local_max.is_finite());
        }
        // Real construction handles must change actual vertices, and an
        // independently shaped block must not mutate the shared PROP mesh.
        let mut variant = originals
            .creation_catalog
            .creations
            .iter()
            .find(|r| {
                r.blocks.iter().any(|b| {
                    originals
                        .part_catalog
                        .parts
                        .iter()
                        .any(|d| d.key == b.definition && d.model.instance_id == 0xAF87_7F25)
                })
            })
            .unwrap()
            .clone();
        variant.blocks.retain(|b| {
            originals
                .part_catalog
                .parts
                .iter()
                .any(|d| d.key == b.definition && d.model.instance_id == 0xAF87_7F25)
        });
        variant.blocks.truncate(1);
        let definition = variant.blocks[0].definition;
        let base = originals.mechanical_models[&definition].clone();
        let base_positions = crate::creation_assets::mesh_positions(&meshes, &base).unwrap();
        variant.blocks[0].handles[0].1 = 0.;
        let low = crate::creation_assets::import_creation(
            &mut originals,
            &variant,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap();
        variant.blocks[0].handles[0].1 = 1.;
        let high = crate::creation_assets::import_creation(
            &mut originals,
            &variant,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap();
        let again = crate::creation_assets::import_creation(
            &mut originals,
            &variant,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap();
        let low_model = &low.blocks[0].mechanical.model;
        let high_model = &high.blocks[0].mechanical.model;
        assert_ne!(low_model.handle, high_model.handle);
        assert_ne!(
            crate::creation_assets::mesh_positions(&meshes, low_model).unwrap(),
            crate::creation_assets::mesh_positions(&meshes, high_model).unwrap()
        );
        assert_eq!(high_model.handle, again.blocks[0].mechanical.model.handle);
        assert_eq!(
            base_positions,
            crate::creation_assets::mesh_positions(&meshes, &base).unwrap()
        );
        println!("PASS original skeletal handle endpoints and isolated geometry cache");
        world.insert_resource(originals);
        world.insert_resource(meshes);
        world.insert_resource(images);
        world.insert_resource(materials);
        enter(&mut app, GameStage::Civilization);
        assert_creation_scene(&mut app);
        let world = app.world_mut();
        assert_eq!(
            world.query::<&crate::civ::CityMarker>().iter(world).count(),
            crate::civ::CITY_COUNT
        );
        assert_eq!(
            world
                .query::<&crate::civ::CityOwnershipBeacon>()
                .iter(world)
                .count(),
            crate::civ::CITY_COUNT
        );
        let city_paints: Vec<_> = world
            .query::<(
                &crate::creation_assets::OriginalCreationPart,
                &MeshMaterial3d<StandardMaterial>,
            )>()
            .iter(world)
            .filter(|(part, _)| part.definition.group_id == crate::asset_catalog::BUILDING_PARTS)
            .map(|(_, handle)| {
                (
                    handle.0.clone(),
                    world
                        .resource::<Assets<StandardMaterial>>()
                        .get(&handle.0)
                        .unwrap()
                        .base_color,
                )
            })
            .collect();
        app.world_mut()
            .resource_mut::<RunProgress>()
            .civ_campaign
            .cities[1]
            .owner = crate::civ::CityOwner::Player;
        step(&mut app);
        let world = app.world_mut();
        for (beacon, material) in world
            .query::<(
                &crate::civ::CityOwnershipBeacon,
                &MeshMaterial3d<StandardMaterial>,
            )>()
            .iter(world)
        {
            let city = &world.resource::<RunProgress>().civ_campaign.cities[beacon.0];
            assert_eq!(
                world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&material.0)
                    .unwrap()
                    .base_color,
                crate::civ::city_material(city.owner).base_color,
                "ownership beacon follows the current city owner"
            );
        }
        let world = app.world_mut();
        let flagship = world
            .query::<(Entity, &crate::creation_assets::OriginalCreation)>()
            .iter(world)
            .find(|(_, source)| source.key.type_id == 0x24682294)
            .unwrap()
            .0;
        let paints: Vec<_> = world
            .query::<(&MeshMaterial3d<StandardMaterial>, &ChildOf)>()
            .iter(world)
            .filter(|(_, parent)| parent.parent() == flagship)
            .map(|(paint, _)| {
                (
                    paint.0.clone(),
                    world
                        .resource::<Assets<StandardMaterial>>()
                        .get(&paint.0)
                        .unwrap()
                        .base_color,
                )
            })
            .collect();
        assert!(!paints.is_empty());
        press(&mut app, KeyCode::KeyC);
        assert!(
            paints.iter().all(|(handle, previous)| app
                .world()
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap()
                .base_color
                == *previous),
            "C is the Space colonize binding; it must not recolor a Civilization flagship"
        );
        assert!(
            city_paints.iter().all(|(handle, previous)| app
                .world()
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap()
                .base_color
                == *previous),
            "city ownership changes must preserve original hall materials"
        );
        let run = app.world().resource::<RunProgress>();
        assert!(run.vehicle.parts.is_empty() && run.building.parts.is_empty());
        capture(app.world_mut());
        let saved = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = installed_campaign_app(saved);
        assert!(resumed
            .world()
            .resource::<crate::Originals>()
            .creation_models
            .is_empty());
        enter(&mut resumed, GameStage::Civilization);
        assert_creation_scene(&mut resumed);
        enter(&mut resumed, GameStage::Space);
        assert_creation_scene(&mut resumed);
        assert!(resumed
            .world()
            .resource::<RunProgress>()
            .ship
            .modules
            .is_empty());
        enter(&mut resumed, GameStage::MainMenu);
        let world = resumed.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<crate::creation_assets::OriginalCreation>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<crate::creation_assets::OriginalCreationPart>>()
                .iter(world)
                .count(),
            0
        );
        enter(&mut resumed, GameStage::Civilization);
        assert_creation_scene(&mut resumed);
        println!("PASS original default creation reloads, cleanup and unchanged saved builds");
    }

    fn isolate_creature(app: &mut App, id: u32) {
        let world = app.world_mut();
        for (world_id, mut transform) in world.query::<(&WorldId, &mut Transform)>().iter_mut(world)
        {
            transform.translation = if world_id.0 == id {
                Vec3::new(0., 0.7, -2.4)
            } else {
                Vec3::new(100., 0., 100.)
            };
        }
        teleport(app, Vec3::new(0., 0.7, 0.));
    }
    #[test]
    fn species_health_from_part_ranks_survives_checkpoint_reload() {
        use crate::asset_catalog::{Capabilities, PartDefinition, CREATURE_PARTS};
        use crate::dbpf::ResourceKey;
        let definition = PartDefinition {
            key: ResourceKey {
                group_id: CREATURE_PARTS,
                instance_id: 123,
                type_id: crate::asset_catalog::PROP,
            },
            model: ResourceKey {
                group_id: 0,
                instance_id: 123,
                type_id: 0,
            },
            name: "Health-ranked test mouth".into(),
            is_null: false,
            price: 0,
            unlock_level: 0,
            min_scale: 0.5,
            max_scale: 1.5,
            default_scale: 1.,
            model_scale: 1.,
            model_offset: [0.; 3],
            model_rotation: [0.; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities {
                health: 5,
                herbivore: 1,
                sing: 1,
                mouth: 1,
                ..default()
            },
        };
        // A legacy species world: its HP is the build's (retail herds use their
        // archetype's HitpointOverride instead, creature_ecology).
        let mut run = RunProgress::default();
        run.species_world = crate::species::SpeciesWorld::generate_n(
            crate::species::lineage_seed(&run.parts, &run.body),
            &crate::asset_catalog::PartCatalog {
                parts: vec![definition.clone()],
                errors: Vec::new(),
            },
            crate::species::DEFAULT_NEST_RADIUS,
            crate::species::DEFAULT_SPECIES,
        );
        let mut app = headless_app(run);
        app.world_mut()
            .resource_mut::<crate::Originals>()
            .part_catalog
            .parts
            .push(definition.clone());
        enter(&mut app, GameStage::Creature);
        let world = app.world_mut();
        let mut actors = world.query::<(&WorldId, &mut Npc)>();
        let (_, mut npc) = actors.iter_mut(world).find(|(id, _)| id.0 == 300).unwrap();
        // Retail: the level-0 health entry (10) plus the parts bonus
        // (creatureHealthIncrement 2 x health level 5).
        assert_eq!(npc.hp, 20.);
        npc.hp -= 7.;
        capture(world);
        let loaded =
            serde_json::from_str(&serde_json::to_string(world.resource::<RunProgress>()).unwrap())
                .unwrap();
        let mut resumed = headless_app(loaded);
        resumed
            .world_mut()
            .resource_mut::<crate::Originals>()
            .part_catalog
            .parts
            .push(definition);
        enter(&mut resumed, GameStage::Creature);
        let world = resumed.world_mut();
        let (_, npc) = world
            .query::<(&WorldId, &Npc)>()
            .iter(world)
            .find(|(id, _)| id.0 == 300)
            .unwrap();
        assert_eq!(npc.hp, 13., "reload must preserve a wounded member's health");
    }

    fn pack_fixture(app: &mut App, allies: &[u32], dna: f32) {
        let world = app.world_mut();
        for (id, mut t, mut npc, mut mind) in world
            .query::<(
                &WorldId,
                &mut Transform,
                &mut Npc,
                &mut crate::creature_actions::CreatureMind,
            )>()
            .iter_mut(world)
        {
            t.translation = if allies.contains(&id.0) {
                Vec3::new(id.0 as f32 - 301., 0.4, 2.)
            } else {
                Vec3::new(100., 0.4, 100.)
            };
            mind.allied = allies.contains(&id.0);
            mind.response_time = 100.;
            npc.hostile = false;
        }
        for mut bush in world.query::<&mut Bush>().iter_mut(world) {
            bush.food_left = 0.;
        }
        let mut run = world.resource_mut::<RunProgress>();
        run.dna_this_stage = dna;
        for &id in allies {
            run.species_world.record_befriend(id);
        }
        teleport(app, Vec3::new(0., 0.7, 0.));
    }

    /// A run with a legacy (pre-archetype) species world: these encounter
    /// tests pin the legacy numbers (flat 10 HP, one social round, base 100);
    /// the retail herds are covered in creature.rs / creature_actions.rs.
    fn legacy_creature_run() -> RunProgress {
        let mut run = RunProgress::default();
        run.species_world = crate::species::SpeciesWorld::generate_n(
            crate::species::lineage_seed(&run.parts, &run.body),
            &crate::asset_catalog::PartCatalog::default(),
            crate::species::DEFAULT_NEST_RADIUS,
            crate::species::DEFAULT_SPECIES,
        );
        run
    }

    fn creature_entity(app: &mut App, member: u32) -> Entity {
        let world = app.world_mut();
        world
            .query::<(Entity, &WorldId)>()
            .iter(world)
            .find(|(_, id)| id.0 == member)
            .unwrap()
            .0
    }

    #[test]
    fn pack_recruitment_following_and_cold_reload_keep_explicit_roster() {
        let mut app = headless_app(legacy_creature_run());
        enter(&mut app, GameStage::Creature);
        pack_fixture(&mut app, &[300, 301, 302], 300.);
        step(&mut app);
        assert!(app
            .world()
            .resource::<RunProgress>()
            .creature_pack
            .members
            .is_empty());
        app.world_mut()
            .resource_mut::<crate::creature_actions::Encounter>()
            .target = Some(301);
        press(&mut app, crate::creature_controls::KEY_RECRUIT);
        assert_eq!(
            app.world().resource::<RunProgress>().creature_pack.members,
            vec![301]
        );
        press(&mut app, crate::creature_controls::KEY_RECRUIT);
        assert!(app
            .world()
            .resource::<RunProgress>()
            .creature_pack
            .members
            .is_empty());
        press(&mut app, crate::creature_controls::KEY_RECRUIT);
        app.world_mut()
            .resource_mut::<crate::creature_actions::Encounter>()
            .target = Some(300);
        press(&mut app, crate::creature_controls::KEY_RECRUIT);
        app.world_mut()
            .resource_mut::<crate::creature_actions::Encounter>()
            .target = Some(302);
        press(&mut app, crate::creature_controls::KEY_RECRUIT);
        assert_eq!(
            app.world().resource::<RunProgress>().creature_pack.members,
            vec![301, 300]
        );
        assert_eq!(app.world().resource::<RunProgress>().dna_this_stage, 300.);
        let unselected = creature_entity(&mut app, 302);
        let stationary = app
            .world()
            .get::<Transform>(unselected)
            .unwrap()
            .translation;
        teleport(&mut app, Vec3::new(13., 0.7, 15.));
        for _ in 0..600 {
            step(&mut app);
        }
        assert_eq!(
            app.world()
                .get::<Transform>(unselected)
                .unwrap()
                .translation,
            stationary
        );
        let e0 = creature_entity(&mut app, 301);
        let e1 = creature_entity(&mut app, 300);
        let p0 = app.world().get::<Transform>(e0).unwrap().translation;
        let p1 = app.world().get::<Transform>(e1).unwrap().translation;
        assert!(
            dist_xz(p0, p1) > 2.,
            "members occupy separate formation slots"
        );
        assert!(dist_xz(p0, Vec3::new(13., 0., 15.)) < 6.);
        let surface = app.world().resource::<crate::terrain::LandSurface>();
        assert!((p0.y - surface.height(p0.x, p0.z) - 1.).abs() < 1e-4);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_pack
            .members = vec![301, 301, 999, 300, 302];
        step(&mut app);
        assert_eq!(
            app.world().resource::<RunProgress>().creature_pack.members,
            vec![301, 300]
        );
        capture(app.world_mut());
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let mut cold = headless_app(serde_json::from_str(&saved).unwrap());
        enter(&mut cold, GameStage::Creature);
        assert_eq!(
            cold.world().resource::<RunProgress>().creature_pack.members,
            vec![301, 300]
        );
        cold.world_mut()
            .resource_mut::<crate::creature_actions::Encounter>()
            .target = Some(301);
        press(&mut cold, crate::creature_controls::KEY_RECRUIT);
        assert_eq!(
            cold.world().resource::<RunProgress>().creature_pack.members,
            vec![300]
        );
        assert_eq!(cold.world().resource::<RunProgress>().dna_this_stage, 300.);
    }

    #[test]
    fn recruited_pack_loyalty_survives_angry_species_cold_spawn_without_checkpoint() {
        use crate::creature_actions::CreatureMind;
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        pack_fixture(&mut app, &[300, 301, 302], 200.);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_pack
            .members = vec![300];
        app.world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .record_attack(302);
        // Stage entry had captured the pre-fixture actors. Exercise the
        // promised no-checkpoint cold spawn, not that unrelated old snapshot.
        app.world_mut().resource_mut::<RunProgress>().checkpoint = None;
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let mut cold = headless_app(serde_json::from_str(&saved).unwrap());
        enter(&mut cold, GameStage::Creature);
        assert_eq!(
            cold.world().resource::<RunProgress>().creature_pack.members,
            vec![300]
        );
        let selected = creature_entity(&mut cold, 300);
        let unselected = creature_entity(&mut cold, 301);
        assert!(cold.world().get::<CreatureMind>(selected).unwrap().allied);
        assert!(!cold.world().get::<Npc>(selected).unwrap().hostile);
        assert!(!cold.world().get::<CreatureMind>(unselected).unwrap().allied);
        assert!(cold.world().get::<Npc>(unselected).unwrap().hostile);
    }

    #[test]
    fn pack_assist_obeys_selected_target_saved_timer_and_single_reward() {
        use crate::creature_actions::{CreatureMind, Encounter, EncounterMode};
        let mut app = headless_app(legacy_creature_run());
        enter(&mut app, GameStage::Creature);
        pack_fixture(&mut app, &[300, 301], 100.);
        let ally = creature_entity(&mut app, 300);
        let other_ally = creature_entity(&mut app, 301);
        let target = creature_entity(&mut app, 302);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .creature_pack
            .members = vec![300];
        app.world_mut()
            .get_mut::<Transform>(target)
            .unwrap()
            .translation = Vec3::new(0., 0.4, 1.5);
        {
            // The retail level-0 health entry is 10 HP (a checkpoint restore
            // clamps a member to its species maximum).
            let mut npc = app.world_mut().get_mut::<Npc>(target).unwrap();
            npc.hp = 10.;
            npc.hostile = true;
        }
        app.world_mut().resource_mut::<Encounter>().mode = EncounterMode::Combat;
        // An allied target cannot provoke assistance.
        app.world_mut().resource_mut::<Encounter>().target = Some(301);
        let allied_hp = app.world().get::<Npc>(other_ally).unwrap().hp;
        step(&mut app);
        assert_eq!(app.world().get::<Npc>(other_ally).unwrap().hp, allied_hp);
        assert_eq!(
            app.world().get::<CreatureMind>(ally).unwrap().attack_timer,
            0.
        );
        app.world_mut().resource_mut::<Encounter>().target = Some(302);
        step(&mut app);
        // The ally's rank-1 bite deals the retail 1.0 damage.
        assert_eq!(app.world().get::<Npc>(target).unwrap().hp, 9.);
        assert!(app.world().get::<CreatureMind>(ally).unwrap().attack_timer > 1.4);
        assert_eq!(
            app.world()
                .get::<CreatureMind>(other_ally)
                .unwrap()
                .attack_timer,
            0.
        );
        step(&mut app);
        assert_eq!(app.world().get::<Npc>(target).unwrap().hp, 9.);
        capture(app.world_mut());
        let saved = serde_json::to_string(app.world().resource::<RunProgress>()).unwrap();
        let mut cold = headless_app(serde_json::from_str(&saved).unwrap());
        enter(&mut cold, GameStage::Creature);
        let ally = creature_entity(&mut cold, 300);
        let target = creature_entity(&mut cold, 302);
        assert_eq!(cold.world().get::<Npc>(target).unwrap().hp, 9.);
        assert!(cold.world().get::<CreatureMind>(ally).unwrap().attack_timer > 1.3);
        cold.world_mut()
            .get_mut::<CreatureMind>(ally)
            .unwrap()
            .attack_timer = 0.;
        cold.world_mut().get_mut::<Npc>(target).unwrap().hp = 1.;
        step(&mut cold);
        assert!(cold.world().get::<CreatureMind>(target).unwrap().defeated);
        // The fixture's 100 DNA plus the retail kill reward (100 at brain level 1
        // against a strength-2 creature: index 2 * 1 - 2 = 0).
        assert_eq!(cold.world().resource::<RunProgress>().dna_this_stage, 200.);
        assert_eq!(
            cold.world()
                .resource::<RunProgress>()
                .species_world
                .relationship(302),
            Some(crate::species::Relationship::Defeated)
        );
        for _ in 0..120 {
            step(&mut cold);
        }
        assert_eq!(cold.world().resource::<RunProgress>().dna_this_stage, 200.);
    }

    #[test]
    fn mid_combat_checkpoint_restores_target_cooldowns_and_wander() {
        use crate::creature_actions::{Encounter, EncounterMode};
        let mut app = headless_app(legacy_creature_run());
        enter(&mut app, GameStage::Creature);
        isolate_creature(&mut app, 300);
        press(&mut app, crate::creature_controls::KEY_STANCE_COMBAT);
        press(&mut app, KeyCode::KeyF);
        press(&mut app, KeyCode::Digit1);
        let world = app.world_mut();
        let npc = world
            .query::<(Entity, &WorldId)>()
            .iter(world)
            .find(|(_, id)| id.0 == 300)
            .map(|(e, _)| e)
            .unwrap();
        world
            .entity_mut(npc)
            .get_mut::<crate::creature::Wander>()
            .unwrap()
            .wait = 8.;
        world
            .entity_mut(npc)
            .get_mut::<Transform>()
            .unwrap()
            .rotation = Quat::from_rotation_y(1.1);
        let expected_hp = world.entity(npc).get::<Npc>().unwrap().hp;
        assert!(expected_hp > 0. && expected_hp < 50.);
        let expected_cd = world.resource::<Encounter>().cooldowns[0];
        assert!(expected_cd > 1.);
        capture(world);
        let loaded: RunProgress =
            serde_json::from_str(&serde_json::to_string(world.resource::<RunProgress>()).unwrap())
                .unwrap();
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Creature);
        crate::persistence::restore(resumed.world_mut());
        let world = resumed.world_mut();
        let encounter = world.resource::<Encounter>();
        assert_eq!(encounter.target, Some(300));
        assert_eq!(encounter.mode, EncounterMode::Combat);
        assert_eq!(encounter.cooldowns[0], expected_cd);
        let (t, npc, w) = world
            .query::<(&WorldId, &Transform, &Npc, &crate::creature::Wander)>()
            .iter(world)
            .find(|(id, _, _, _)| id.0 == 300)
            .map(|(_, t, n, w)| (t, n, w))
            .unwrap();
        assert_eq!(npc.hp, expected_hp);
        assert_eq!(w.wait, 8.);
        assert!(t.rotation.angle_between(Quat::from_rotation_y(1.1)) < 1e-4);
        press(&mut resumed, KeyCode::Digit1);
        let world = resumed.world_mut();
        let hp = world
            .query::<(&WorldId, &Npc)>()
            .iter(world)
            .find(|(id, _)| id.0 == 300)
            .unwrap()
            .1
            .hp;
        assert_eq!(
            hp, expected_hp,
            "reloading cannot bypass an attack cooldown"
        );
    }
    #[test]
    fn befriending_keeps_a_living_ally_and_survives_checkpoint() {
        use crate::creature_actions::CreatureMind;
        let mut app = headless_app(legacy_creature_run());
        enter(&mut app, GameStage::Creature);
        isolate_creature(&mut app, 300);
        press(&mut app, KeyCode::KeyF);
        let ally = creature_entity(&mut app, 300);
        for _ in 0..3 {
            // The creature demonstrates for 5 s first: the round opens when
            // 15 s of answer time are left (retail social round).
            for _ in 0..1200 {
                if app.world().get::<CreatureMind>(ally).unwrap().response_time
                    <= crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT
                {
                    break;
                }
                step(&mut app);
            }
            press(&mut app, KeyCode::Digit1);
            for _ in 0..400 {
                if app
                    .world()
                    .resource::<crate::creature_actions::Encounter>()
                    .cooldowns[0]
                    <= 0.
                {
                    break;
                }
                step(&mut app);
            }
        }
        assert_eq!(
            stage(&app),
            GameStage::Creature,
            "ability keys must not perform debug jumps"
        );
        let world = app.world_mut();
        let (npc, mind) = world
            .query::<(&WorldId, &Npc, &CreatureMind)>()
            .iter(world)
            .find(|(id, _, _)| id.0 == 300)
            .map(|(_, n, m)| (n.hp, m.clone()))
            .unwrap();
        assert!(mind.allied);
        assert_eq!(npc, 10.);
        assert_eq!(
            world
                .resource::<RunProgress>()
                .species_world
                .relationship(300),
            Some(crate::species::Relationship::Allied)
        );
        // Retail reward at brain level 0: the species base 100.
        assert_eq!(world.resource::<RunProgress>().dna_this_stage, 100.);
        press(&mut app, KeyCode::Digit1);
        assert_eq!(app.world().resource::<RunProgress>().dna_this_stage, 100.);
        capture(app.world_mut());
        let loaded = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Creature);
        let world = resumed.world_mut();
        let (npc, mind) = world
            .query::<(&WorldId, &Npc, &CreatureMind)>()
            .iter(world)
            .find(|(id, _, _)| id.0 == 300)
            .map(|(_, n, m)| (n.hp, m.clone()))
            .unwrap();
        assert!(mind.allied);
        assert_eq!(npc, 10.);
        assert_eq!(
            world
                .resource::<RunProgress>()
                .species_world
                .relationship(300),
            Some(crate::species::Relationship::Allied)
        );
    }
    #[test]
    fn combat_obeys_cooldown_and_keeps_defeat_on_reload() {
        use crate::creature_actions::CreatureMind;
        let mut app = headless_app(legacy_creature_run());
        enter(&mut app, GameStage::Creature);
        isolate_creature(&mut app, 304);
        // A retail avatar has 10 HP and the creature fights back with the same
        // bite: keep the player alive so the test is about cooldowns and the
        // recorded defeat.
        {
            let world = app.world_mut();
            let mut player = world.query_filtered::<&mut Health, With<Player>>();
            let mut health = player.single_mut(world).unwrap();
            health.max = 100.;
            health.hp = 100.;
        }
        press(&mut app, crate::creature_controls::KEY_STANCE_COMBAT);
        press(&mut app, KeyCode::KeyF);
        press(&mut app, KeyCode::Digit1);
        press(&mut app, KeyCode::Digit1);
        let world = app.world_mut();
        // One rank-1 bite (retail 1.0 damage) out of the level-0 10 HP.
        assert_eq!(
            world
                .query::<(&WorldId, &Npc)>()
                .iter(world)
                .find(|(id, _)| id.0 == 304)
                .unwrap()
                .1
                .hp,
            9.
        );
        for _ in 0..9 {
            for _ in 0..100 {
                step(&mut app);
            }
            press(&mut app, KeyCode::Digit1);
        }
        // The kill pays the retail 100 DNA (brain level 0, strength-2 target).
        assert_eq!(app.world().resource::<RunProgress>().dna_this_stage, 100.);
        capture(app.world_mut());
        let loaded = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Creature);
        let world = resumed.world_mut();
        let dead_actor = world
            .query::<(&WorldId, &Npc, &CreatureMind, &Visibility)>()
            .iter(world)
            .find(|(id, _, _, _)| id.0 == 304);
        if let Some((_, npc, mind, visibility)) = dead_actor {
            assert_eq!(npc.hp, 0.);
            assert!(mind.defeated);
            assert_eq!(*visibility, Visibility::Hidden);
        }
        assert_eq!(
            world
                .resource::<RunProgress>()
                .species_world
                .relationship(304),
            Some(crate::species::Relationship::Defeated)
        );
        for _ in 0..300 {
            step(&mut resumed);
        }
        let world = resumed.world_mut();
        assert!(
            world
                .query::<(&WorldId, &Npc)>()
                .iter(world)
                .all(|(id, npc)| id.0 != 304 || npc.hp == 0.),
            "recorded defeats must survive population refill ticks"
        );
    }

    #[test]
    fn tribal_checkpoint_preserves_large_roster_and_member_identity_after_death() {
        use crate::tribal::TribeRole;
        let mut run = RunProgress::default();
        run.tribe = (0..crate::tribal::MAX_MATES)
            .map(|i| MateAssignment {
                role: (i % 4) as u32,
                part: 0,
                outfit_definition: None,
            })
            .collect();
        run.tribal_campaign.slots = vec![11, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<TribeMate>>()
                .iter(world)
                .count(),
            crate::tribal::MAX_MATES,
            "setup must render every saved recruit"
        );
        for (id, mut hp) in world.query::<(&WorldId, &mut Health)>().iter_mut(world) {
            if id.0 == 200 {
                hp.hp = 0.;
            }
        }
        step(&mut app);
        let world = app.world_mut();
        assert_eq!(world.resource::<RunProgress>().tribe.len(), 11);
        assert!(!world
            .resource::<RunProgress>()
            .tribal_campaign
            .slots
            .contains(&0));
        let mut before: Vec<_> = world
            .query_filtered::<(&WorldId, &TribeRole), With<TribeMate>>()
            .iter(world)
            .map(|(id, role)| (id.0, role.0))
            .collect();
        before.sort_unstable();
        capture(world);
        let loaded =
            serde_json::from_str(&serde_json::to_string(world.resource::<RunProgress>()).unwrap())
                .unwrap();
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Tribal);
        let world = resumed.world_mut();
        let mut after: Vec<_> = world
            .query_filtered::<(&WorldId, &TribeRole), With<TribeMate>>()
            .iter(world)
            .map(|(id, role)| (id.0, role.0))
            .collect();
        after.sort_unstable();
        assert_eq!(
            after, before,
            "reload must preserve roles by WorldId after swap-removal"
        );
        assert!(!after.iter().any(|(id, _)| *id == 200));
    }

    #[test]
    fn civilization_checkpoint_preserves_owned_city_and_partial_conversion() {
        use crate::civ::{Approach, CityOwner};
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Civilization);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.spice = 417.;
            run.civ_campaign.approach = Approach::Religious;
            run.civ_campaign.cities[1].owner = CityOwner::Player;
            run.civ_campaign.cities[1].reward_paid = true;
            run.civ_campaign.cities[1].buildings = 2;
            run.civ_campaign.cities[1].defense = 1;
            run.civ_campaign.cities[2].conversion = 0.45;
            run.civ_campaign.cities[3].trade = true;
            run.civ_campaign.cities[3].spent = 23.;
        }
        teleport(&mut app, Vec3::new(-20., 0.35, -20.));
        capture(app.world_mut());
        let before = app.world().resource::<RunProgress>().civ_campaign.clone();
        let loaded = serde_json::from_str(
            &serde_json::to_string(app.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Civilization);
        let run = resumed.world().resource::<RunProgress>();
        assert_eq!(run.spice, 417.);
        assert_eq!(run.civ_campaign.approach, before.approach);
        for index in 0..5 {
            let city = &run.civ_campaign.cities[index];
            let saved = &before.cities[index];
            assert_eq!(city.owner, saved.owner);
            assert_eq!(city.reward_paid, saved.reward_paid);
            assert_eq!(city.buildings, saved.buildings);
            assert_eq!(city.defense, saved.defense);
            assert_eq!(city.conversion, saved.conversion);
            assert_eq!(city.spent, saved.spent);
            assert_eq!(city.trade, saved.trade);
        }
        assert_eq!(stage(&resumed), GameStage::Civilization);
    }

    #[test]
    fn space_checkpoint_restores_world_player_and_upgrades() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Space);
        let world = app.world_mut();
        let target = world
            .query::<(&Transform, &WorldId)>()
            .iter(world)
            .find(|(_, id)| id.0 == 2)
            .unwrap()
            .0
            .translation;
        {
            // Retail: colonizing needs Flight 101's drive and a 150,000 pack.
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.spice = 1_000_000.;
            run.space_campaign
                .grant_tool(crate::space_rules::tool::INTERPLANETARY_DRIVE);
        }
        teleport(&mut app, target);
        press(&mut app, KeyCode::KeyV);
        press(&mut app, KeyCode::KeyT);
        press(&mut app, KeyCode::KeyC);
        {
            // Health upgrade 2 (150,000) and laser 2 (500,000), bought from an
            // empire store (retail has no hotkeys for them); `hull` / `weapon`
            // mirror the levels for old callers.
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.spice = 1_000_000.0 - 150_000.0 - 500_000.0;
            run.space_campaign.grant_tool("healthupgrade2");
            run.space_campaign.grant_tool("laserlevel2");
            run.hull = 1;
            run.weapon = 1;
        }
        teleport(&mut app, Vec3::new(4.0, 20.0, 7.0));
        let world = app.world_mut();
        world
            .query_filtered::<&mut Health, With<Player>>()
            .single_mut(world)
            .unwrap()
            .hp = 73.0;
        capture(world);
        let text = serde_json::to_string(world.resource::<RunProgress>()).unwrap();
        let loaded: RunProgress = serde_json::from_str(&text).unwrap();
        let spice = loaded.spice;
        let mut resumed = headless_app(loaded);
        resumed
            .world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Space);
        step(&mut resumed);
        assert_eq!(resumed.world().resource::<RunProgress>().colonies, 1);
        assert_eq!(resumed.world().resource::<RunProgress>().hull, 1);
        assert_eq!(resumed.world().resource::<RunProgress>().weapon, 1);
        let world = resumed.world_mut();
        let (t, hp) = world
            .query_filtered::<(&Transform, &Health), With<Player>>()
            .single(world)
            .unwrap();
        assert_eq!(t.translation, Vec3::new(4.0, 20.0, 7.0));
        assert_eq!(hp.hp, 73.0);
        let (t, _, planet) = world
            .query::<(&Transform, &WorldId, &Planet)>()
            .iter(world)
            .find(|(_, id, _)| id.0 == 2)
            .unwrap();
        assert!(planet.colonized && planet.surveyed);
        let position = t.translation;
        teleport(&mut resumed, position);
        press(&mut resumed, KeyCode::KeyV);
        press(&mut resumed, KeyCode::KeyC);
        assert_eq!(resumed.world().resource::<RunProgress>().colonies, 1);
        assert_eq!(resumed.world().resource::<RunProgress>().spice, spice);
    }

    #[test]
    fn pause_freezes_every_stage_and_resume_restarts_simulation() {
        // The Cell levels come from the installed game: without Spore the cell has nothing
        // to move through, so that stage is skipped there.
        let installed = crate::spore_install::discover().is_ok();
        for target in [
            GameStage::Cell,
            GameStage::Creature,
            GameStage::Tribal,
            GameStage::Civilization,
            GameStage::Space,
        ] {
            if target == GameStage::Cell && !installed {
                continue;
            }
            let mut app = headless_app(RunProgress::default());
            enter(&mut app, target.clone());
            press(&mut app, KeyCode::Escape);
            assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
            let snapshot = |app: &mut App| {
                let world = app.world_mut();
                let (t, h, food) = world
                    .query_filtered::<(&Transform, &Health, Option<&Hunger>), With<Player>>()
                    .single(world)
                    .unwrap();
                (
                    t.translation,
                    h.hp,
                    food.map(|h| h.food),
                    world.resource::<RunProgress>().spice,
                )
            };
            let before = snapshot(&mut app);
            // tribal_controls: in the Tribal stage W pans the camera instead of
            // walking the chieftain.
            let focus = |app: &App| {
                app.world()
                    .resource::<crate::tribal_controls::TribalCamera>()
                    .rig
                    .goal_focus()
            };
            let focus_before = focus(&app);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            for _ in 0..120 {
                app.update();
            }
            assert_eq!(snapshot(&mut app), before, "pause freezes {target:?}");
            assert_eq!(focus(&app), focus_before, "pause freezes the camera");
            press(&mut app, KeyCode::Escape);
            assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
            for _ in 0..10 {
                app.update();
            }
            let moved = if target == GameStage::Tribal {
                focus(&app) != focus_before
            } else {
                snapshot(&mut app).0 != before.0
            };
            assert!(moved, "resume moves {target:?}");
        }
    }

    #[test]
    fn pause_preserves_stage_and_menu_checkpoints_without_restarting() {
        let mut app = headless_app(RunProgress::default());
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Sphere::new(0.5));
        app.world_mut()
            .resource_mut::<crate::Originals>()
            .models
            .push(crate::OriginalModel {
                handle,
                instance: 123,
                scale: 1.0,
                ..default()
            });
        enter(&mut app, GameStage::Creature);
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 52.0;
        press(&mut app, KeyCode::KeyN);
        step(&mut app);
        assert_eq!(stage(&app), GameStage::Creature);
        assert_eq!(app.world().resource::<RunProgress>().parts.len(), 1);
        press(&mut app, KeyCode::Escape);
        step(&mut app);
        assert_eq!(stage(&app), GameStage::Creature);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        app.world_mut()
            .spawn((crate::pause_menu::PauseAction::MainMenu, bevy::ui::Pressed));
        step(&mut app);
        step(&mut app);
        assert_eq!(stage(&app), GameStage::MainMenu);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.current_stage(), GameStage::Creature);
        assert_eq!(run.dna_this_stage, 52.0);
        assert!(run.checkpoint.is_some());
        // Reconstruct as a freshly loaded process would, without touching disk.
        let loaded = serde_json::from_str(&serde_json::to_string(run).unwrap()).unwrap();
        let mut resumed = headless_app(loaded);
        resumed
            .world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        step(&mut resumed);
        assert_eq!(
            resumed.world().resource::<RunProgress>().dna_this_stage,
            52.0
        );
        press(&mut app, KeyCode::Enter);
        step(&mut app);
        assert_eq!(stage(&app), GameStage::Cell);
        let fresh = app.world().resource::<RunProgress>();
        assert_eq!(fresh.parts[0].instance, 123);
        assert_eq!(fresh.dna_this_stage, 0.0);
        assert!(fresh.checkpoint.as_ref().is_none_or(|c| c.stage == 0));
    }

    #[test]
    fn checkpoints_keep_depleted_resources_and_grown_tribe() {
        for expected in [
            GameStage::Creature,
            GameStage::Tribal,
            GameStage::Civilization,
        ] {
            let mut run = RunProgress::default();
            run.tribe.resize(5, MateAssignment::default());
            let mut app = headless_app(run);
            enter(&mut app, expected.clone());
            teleport(&mut app, Vec3::new(0.0, 1.0, 0.0));
            let world = app.world_mut();
            for (id, bush, spice) in world
                .query::<(&WorldId, Option<&mut Bush>, Option<&mut SpiceNode>)>()
                .iter_mut(world)
            {
                if id.0 == 100 {
                    if let Some(mut bush) = bush {
                        bush.food_left = 17.0;
                    }
                    if let Some(mut spice) = spice {
                        spice.spice_left = 29.0;
                    }
                }
            }
            world.resource_mut::<RunProgress>().dna_this_stage = 43.0;
            capture(world);
            let loaded = serde_json::from_str(
                &serde_json::to_string(world.resource::<RunProgress>()).unwrap(),
            )
            .unwrap();
            let mut resumed = headless_app(loaded);
            // Inspect the restored snapshot without advancing collection on
            // this frame; grazing resumes once the clock advances again.
            resumed.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            resumed
                .world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(expected.clone());
            step(&mut resumed);
            let world = resumed.world_mut();
            let (_, bush, spice) = world
                .query::<(&WorldId, Option<&Bush>, Option<&SpiceNode>)>()
                .iter(world)
                .find(|(id, _, _)| id.0 == 100)
                .unwrap();
            if let Some(bush) = bush {
                assert_eq!(bush.food_left, 17.0);
            }
            if let Some(spice) = spice {
                assert_eq!(spice.spice_left, 29.0);
            }
            assert_eq!(world.resource::<RunProgress>().dna_this_stage, 43.0);
            if expected == GameStage::Tribal {
                assert_eq!(
                    world
                        .query_filtered::<Entity, With<TribeMate>>()
                        .iter(world)
                        .count(),
                    5
                );
            }
        }
    }

    #[test]
    fn emitted_wav_is_decodable_by_the_game_audio_backend() {
        use bevy::audio::{AudioSource, Decodable};
        let pcm = vec![0_i16; 480];
        let source = AudioSource {
            bytes: crate::audio::write_wav(&pcm, 1, 24000).into(),
        };
        let decoded: Vec<_> = source.decoder().collect();
        assert_eq!(decoded.len(), pcm.len());
        assert!(decoded.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn periodic_and_exit_saves_capture_the_finished_frame() {
        let dir =
            std::env::temp_dir().join(format!("openspore-exit-save-test-{}", std::process::id()));
        let path = dir.join("run.json");
        let mut app = headless_app(RunProgress::default());
        {
            let mut saves = app
                .world_mut()
                .resource_mut::<crate::persistence::SaveState>();
            saves.path = path.clone();
            saves.enabled = true;
        }
        enter(&mut app, GameStage::Cell);
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 17.0;
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(11));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(11)));
        step(&mut app);
        let saved = crate::persistence::load_from(&path).unwrap();
        assert_eq!(
            saved.dna_this_stage,
            app.world().resource::<RunProgress>().dna_this_stage
        );
        assert!(saved.checkpoint.is_some());
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 63.0;
        app.world_mut().write_message(bevy::app::AppExit::Success);
        step(&mut app);
        let saved = crate::persistence::load_from(&path).unwrap();
        assert_eq!(
            saved.dna_this_stage,
            app.world().resource::<RunProgress>().dna_this_stage
        );
        let world = app.world_mut();
        let position = world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .unwrap()
            .translation
            .to_array();
        assert_eq!(saved.checkpoint.unwrap().player.position, position);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(crate::persistence::backup_path(&path)).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
