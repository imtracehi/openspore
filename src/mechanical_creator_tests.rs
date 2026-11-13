use super::*;

use bevy::{prelude::*, ui::Pressed};
use std::path::PathBuf;

use crate::{
    asset_catalog::{self, PartDefinition},
    dbpf::{DbpfEntry, ResourceKey},
    game::{RunProgress, ShipBuild, ShipModule, VehiclePart},
    mechanical_creator::{self, CreatorAction, CreatorKind, MechanicalCreator},
    stages::GameStage,
    Originals,
};

const MODEL_A: u32 = 0xa001_0001;
const MODEL_B: u32 = 0xa001_0002;

fn definition(group: u32, definition: u32, model: u32, name: &str) -> PartDefinition {
    PartDefinition {
        key: ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: group,
            instance_id: definition,
        },
        model: ResourceKey {
            type_id: asset_catalog::RW4,
            group_id: group,
            instance_id: model,
        },
        name: name.into(),
        is_null: false,
        price: 40,
        unlock_level: 1,
        min_scale: 0.5,
        max_scale: 2.0,
        default_scale: 1.0,
        model_scale: 1.0,
        model_offset: [0.0; 3],
        model_rotation: [0.0; 3],
        diffuse: None,
        tint: None,
        spec_bump: None,
        abilities: Default::default(),
    }
}

fn originals_with(definitions: &[PartDefinition]) -> Originals {
    let mut originals = Originals::default();
    originals.part_catalog.parts = definitions.to_vec();
    for definition in definitions {
        originals.resource_index.records.insert(
            definition.model,
            (
                PathBuf::from("mechanical_creator_test_missing.package"),
                DbpfEntry {
                    key: definition.model,
                    offset: 0,
                    disk_size: 8,
                    mem_size: 8,
                    is_compressed: false,
                },
            ),
        );
    }
    originals
}

fn input_app(stage: GameStage, run: RunProgress, originals: Originals) -> App {
    let mut app = App::new();
    app.insert_resource(State::new(stage));
    app.insert_resource(run);
    app.insert_resource(originals);
    app.init_resource::<MechanicalCreator>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.insert_resource(Assets::<Mesh>::default());
    app.insert_resource(Assets::<Image>::default());
    app.insert_resource(Assets::<StandardMaterial>::default());
    app.add_systems(Update, mechanical_creator::input);
    app
}

fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}

fn seed_cached_part(originals: &mut Originals, definition: &PartDefinition) {
    originals.mechanical_models.insert(
        definition.key,
        crate::OriginalModel {
            instance: definition.model.instance_id,
            ..Default::default()
        },
    );
}

#[test]
fn modal_open_browse_close_preserves_recipes_and_economy() {
    let a = definition(asset_catalog::VEHICLE_PARTS, 1, MODEL_A, "Original A");
    let b = definition(asset_catalog::VEHICLE_PARTS, 2, MODEL_B, "Original B");
    let mut run = RunProgress::default();
    run.spice = 321.0;
    run.vehicle.parts.push(VehiclePart::new(0xdead_1001, 1));
    run.building.parts.push(VehiclePart::new(0xdead_2001, 2));
    run.ship.modules.push(ShipModule::new(0xdead_3001, 3));
    let vehicle_before = run.vehicle.clone();
    let building_before = run.building.clone();
    let ship_before = run.ship.clone();
    let spice_before = run.spice;
    let dna_before = run.editor_dna;
    let mut app = input_app(GameStage::Civilization, run, originals_with(&[a, b]));

    // The modal opens with Shift+F3 (F3 is the rigid-block editor).
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    press(&mut app, KeyCode::F3);
    assert!(app.world().resource::<MechanicalCreator>().open);
    assert_eq!(
        app.world().resource::<MechanicalCreator>().selected_model,
        Some(MODEL_A)
    );
    press(&mut app, KeyCode::BracketRight);
    assert_eq!(
        app.world().resource::<MechanicalCreator>().selected_model,
        Some(MODEL_B)
    );
    assert!(!app.world().resource::<RunProgress>().save_requested);
    assert_eq!(
        app.world().resource::<RunProgress>().vehicle,
        vehicle_before
    );
    assert_eq!(
        app.world().resource::<RunProgress>().building,
        building_before
    );
    assert_eq!(app.world().resource::<RunProgress>().ship, ship_before);
    assert_eq!(app.world().resource::<RunProgress>().spice, spice_before);
    assert_eq!(app.world().resource::<RunProgress>().editor_dna, dna_before);

    press(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<MechanicalCreator>().open);
    assert_eq!(
        app.world().resource::<RunProgress>().vehicle,
        vehicle_before
    );
    assert_eq!(
        app.world().resource::<RunProgress>().building,
        building_before
    );
    assert_eq!(app.world().resource::<RunProgress>().ship, ship_before);
    assert_eq!(app.world().resource::<RunProgress>().spice, spice_before);
}

#[test]
fn failed_add_and_replace_keep_an_unsupported_saved_ship_module_unchanged() {
    let candidate = definition(
        asset_catalog::VEHICLE_PARTS,
        1,
        MODEL_A,
        "Unreadable Original",
    );
    let mut run = RunProgress::default();
    let mut unsupported = ShipModule::new(0xdead_beef, 3);
    unsupported.yaw = 0.75;
    unsupported.scale = 1.4;
    unsupported.offset = [0.2, -0.3, 0.4];
    run.ship = ShipBuild {
        modules: vec![unsupported],
        focused: 0,
    };
    run.spice = 88.0;
    let build_before = run.ship.clone();
    let mut app = input_app(GameStage::Space, run, originals_with(&[candidate]));
    {
        let mut creator = app.world_mut().resource_mut::<MechanicalCreator>();
        creator.open = true;
        creator.kind = CreatorKind::Ship;
        creator.selected_model = Some(MODEL_A);
    }

    press(&mut app, KeyCode::KeyN);
    assert_eq!(app.world().resource::<RunProgress>().ship, build_before);
    assert_eq!(app.world().resource::<RunProgress>().spice, 88.0);
    assert!(!app.world().resource::<RunProgress>().save_requested);
    assert!(app
        .world()
        .resource::<MechanicalCreator>()
        .note
        .contains("unavailable in this install"));

    app.world_mut().spawn((CreatorAction::Replace, Pressed));
    app.update();
    assert_eq!(app.world().resource::<RunProgress>().ship, build_before);
    assert_eq!(app.world().resource::<RunProgress>().spice, 88.0);
    assert!(!app.world().resource::<RunProgress>().save_requested);
    assert!(app
        .world()
        .resource::<MechanicalCreator>()
        .note
        .contains("unavailable in this install"));
}

#[test]
fn ship_add_refuses_duplicate_modules_and_the_part_limit() {
    let candidate = definition(asset_catalog::VEHICLE_PARTS, 1, MODEL_A, "Cached Original");

    let mut duplicate_run = RunProgress::default();
    duplicate_run.ship.modules.push(ShipModule::new(MODEL_A, 0));
    let duplicate_before = duplicate_run.ship.clone();
    let mut duplicate_originals = originals_with(std::slice::from_ref(&candidate));
    seed_cached_part(&mut duplicate_originals, &candidate);
    let mut duplicate_app = input_app(GameStage::Space, duplicate_run, duplicate_originals);
    {
        let mut creator = duplicate_app
            .world_mut()
            .resource_mut::<MechanicalCreator>();
        creator.open = true;
        creator.kind = CreatorKind::Ship;
        creator.selected_model = Some(MODEL_A);
    }
    press(&mut duplicate_app, KeyCode::KeyN);
    assert_eq!(
        duplicate_app.world().resource::<RunProgress>().ship,
        duplicate_before
    );
    assert!(
        !duplicate_app
            .world()
            .resource::<RunProgress>()
            .save_requested
    );
    assert!(duplicate_app
        .world()
        .resource::<MechanicalCreator>()
        .note
        .contains("already fitted"));

    let mut full_run = RunProgress::default();
    full_run.ship.modules = (0..crate::space::MAX_SHIP_MODULES)
        .map(|slot| ShipModule::new(0xb000_0000 + slot as u32, slot as u32))
        .collect();
    let full_before = full_run.ship.clone();
    let mut full_originals = originals_with(std::slice::from_ref(&candidate));
    seed_cached_part(&mut full_originals, &candidate);
    let mut full_app = input_app(GameStage::Space, full_run, full_originals);
    {
        let mut creator = full_app.world_mut().resource_mut::<MechanicalCreator>();
        creator.open = true;
        creator.kind = CreatorKind::Ship;
        creator.selected_model = Some(MODEL_A);
    }
    press(&mut full_app, KeyCode::KeyN);
    assert_eq!(full_app.world().resource::<RunProgress>().ship, full_before);
    assert!(!full_app.world().resource::<RunProgress>().save_requested);
    assert!(full_app
        .world()
        .resource::<MechanicalCreator>()
        .note
        .contains("part limit"));
}

#[derive(Resource, Default)]
struct SimulationTicks(u32);

fn count_simulation(mut ticks: ResMut<SimulationTicks>) {
    ticks.0 += 1;
}

#[test]
fn creator_run_condition_freezes_simulation_until_the_modal_closes() {
    let mut app = App::new();
    app.init_resource::<MechanicalCreator>()
        .init_resource::<SimulationTicks>()
        .add_systems(
            Update,
            count_simulation.run_if(mechanical_creator::simulation_active),
        );

    app.update();
    assert_eq!(app.world().resource::<SimulationTicks>().0, 1);
    app.world_mut().resource_mut::<MechanicalCreator>().open = true;
    app.update();
    assert_eq!(app.world().resource::<SimulationTicks>().0, 1);
    app.world_mut().resource_mut::<MechanicalCreator>().open = false;
    app.update();
    assert_eq!(app.world().resource::<SimulationTicks>().0, 2);
}

#[test]
fn cached_add_rotate_scale_slide_and_remove_mutate_only_the_selected_build_part() {
    let candidate = definition(asset_catalog::VEHICLE_PARTS, 1, MODEL_A, "Cached Original");
    let mut run = RunProgress::default();
    let mut existing = VehiclePart::new(0xdead_0001, 2);
    existing.yaw = 0.3;
    existing.scale = 1.25;
    existing.offset = [0.2, -0.1, 0.4];
    run.vehicle.parts.push(existing.clone());
    let mut originals = originals_with(std::slice::from_ref(&candidate));
    seed_cached_part(&mut originals, &candidate);
    let mut app = input_app(GameStage::Civilization, run, originals);
    {
        let mut creator = app.world_mut().resource_mut::<MechanicalCreator>();
        creator.open = true;
        creator.kind = CreatorKind::Vehicle;
        creator.selected_model = Some(MODEL_A);
    }

    press(&mut app, KeyCode::KeyN);
    let after_add = app.world().resource::<RunProgress>();
    assert_eq!(after_add.vehicle.parts.len(), 2);
    assert_eq!(after_add.vehicle.focused, 1);
    assert_eq!(after_add.vehicle.parts[0], existing);
    assert_eq!(after_add.vehicle.parts[1], VehiclePart::new(MODEL_A, 1));
    assert!(after_add.save_requested);

    // Exercise the same persisted transform fields used by the editor's
    // keyboard controls, then verify removal leaves the prior fitted part
    // byte-for-byte intact and clamps focus to the remaining entry.
    press(&mut app, KeyCode::KeyR);
    press(&mut app, KeyCode::Equal);
    let action = app
        .world_mut()
        .spawn((CreatorAction::Offset(0, 0.1), Pressed))
        .id();
    app.update();
    app.world_mut().despawn(action);
    let edited = app.world().resource::<RunProgress>().vehicle.parts[1].clone();
    assert!((edited.yaw - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    assert!((edited.scale - 1.1).abs() < 1e-5);
    assert!((edited.offset[0] - 0.1).abs() < 1e-5);

    let remove = app.world_mut().spawn((CreatorAction::Remove, Pressed)).id();
    app.update();
    app.world_mut().despawn(remove);
    let after_remove = app.world().resource::<RunProgress>();
    assert_eq!(after_remove.vehicle.parts, vec![existing]);
    assert_eq!(after_remove.vehicle.focused, 0);
    assert!(after_remove.save_requested);
}

#[test]
fn replace_refuses_model_already_fitted_in_another_vehicle_or_ship_slot() {
    let candidate = definition(asset_catalog::VEHICLE_PARTS, 1, MODEL_A, "Cached A");
    for (kind, stage) in [
        (CreatorKind::Vehicle, GameStage::Civilization),
        (CreatorKind::Ship, GameStage::Space),
    ] {
        let mut run = RunProgress::default();
        match kind {
            CreatorKind::Vehicle => {
                run.vehicle.parts =
                    vec![VehiclePart::new(MODEL_A, 0), VehiclePart::new(MODEL_B, 3)];
                run.vehicle.focused = 1;
            }
            CreatorKind::Ship => {
                run.ship.modules = vec![ShipModule::new(MODEL_A, 0), ShipModule::new(MODEL_B, 3)];
                run.ship.focused = 1;
            }
            CreatorKind::Building => unreachable!(),
        }
        let before = run.clone();
        let mut originals = originals_with(std::slice::from_ref(&candidate));
        seed_cached_part(&mut originals, &candidate);
        let mut app = input_app(stage, run, originals);
        {
            let mut creator = app.world_mut().resource_mut::<MechanicalCreator>();
            creator.open = true;
            creator.kind = kind;
            creator.selected_model = Some(MODEL_A);
        }

        let replace = app
            .world_mut()
            .spawn((CreatorAction::Replace, Pressed))
            .id();
        app.update();
        app.world_mut().despawn(replace);

        let after = app.world().resource::<RunProgress>();
        match kind {
            CreatorKind::Vehicle => assert_eq!(after.vehicle, before.vehicle),
            CreatorKind::Ship => assert_eq!(after.ship, before.ship),
            CreatorKind::Building => unreachable!(),
        }
        assert!(!after.save_requested);
        assert!(app
            .world()
            .resource::<MechanicalCreator>()
            .note
            .contains("already fitted"));
    }
}

#[test]
fn f3_does_not_open_mechanical_creator_over_an_already_open_pause_menu() {
    let mut app = App::new();
    app.insert_resource(State::new(GameStage::Civilization));
    app.insert_resource(RunProgress::default());
    app.insert_resource(Originals::default());
    app.init_resource::<MechanicalCreator>()
        .init_resource::<crate::pause_menu::PauseMenu>()
        .init_resource::<crate::editor::EditorState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<NextState<GameStage>>()
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<Image>::default())
        .insert_resource(Assets::<StandardMaterial>::default())
        .add_systems(
            Update,
            (crate::pause_menu::input, mechanical_creator::input).chain(),
        );
    app.world_mut()
        .resource_mut::<crate::pause_menu::PauseMenu>()
        .open = true;

    press(&mut app, KeyCode::F3);

    assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
    assert!(
        !app.world().resource::<MechanicalCreator>().open,
        "the pause overlay must own F3 until the player resumes"
    );
}
