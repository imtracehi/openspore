use super::*;
use bevy::asset::RenderAssetUsages;

fn mesh_asset() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-2.0, -1.0, 0.0],
            [2.0, -1.0, 0.0],
            [2.0, 1.0, 0.0],
            [-2.0, 1.0, 0.0],
        ],
    );
    mesh
}

fn insert_assets(world: &mut World) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    let mut meshes = Assets::<Mesh>::default();
    let mesh = meshes.add(mesh_asset());
    let mut materials = Assets::<StandardMaterial>::default();
    let material = materials.add(StandardMaterial::default());
    world.insert_resource(meshes);
    world.insert_resource(materials);
    (mesh, material)
}

fn attach_drawable(
    world: &mut World,
    parent: Entity,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    local: Transform,
    marker: Option<OriginalSurfaceChild>,
    visibility: Visibility,
) -> Entity {
    let parent_global = *world.get::<GlobalTransform>(parent).unwrap();
    let global = parent_global.mul_transform(local);
    let mut entity = world.spawn((
        ChildOf(parent),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        local,
        global,
        visibility,
    ));
    if let Some(marker) = marker {
        entity.insert(marker);
    }
    entity.id()
}

#[test]
fn source_draws_keep_native_handles_and_root_space_while_rejecting_hidden_and_ring_meshes() {
    let mut world = World::new();
    let (mesh, material) = insert_assets(&mut world);
    let root_local =
        Transform::from_xyz(100.0, -30.0, 4.0).with_rotation(Quat::from_rotation_y(0.7));
    let root = world
        .spawn((
            crate::game::Player,
            root_local,
            GlobalTransform::from(root_local),
        ))
        .id();
    let source_local = Transform::from_xyz(5.0, 1.0, -2.0);
    let source = attach_drawable(
        &mut world,
        root,
        &mesh,
        &material,
        source_local,
        Some(OriginalSurfaceChild {
            source_section: 17,
            state_sections: vec![3, 8],
        }),
        Visibility::Inherited,
    );
    let ring = attach_drawable(
        &mut world,
        root,
        &mesh,
        &material,
        Transform::from_xyz(0.0, 2.0, 0.0),
        None,
        Visibility::Inherited,
    );
    let hidden = attach_drawable(
        &mut world,
        root,
        &mesh,
        &material,
        Transform::from_xyz(-4.0, 0.0, 0.0),
        Some(OriginalSurfaceChild {
            source_section: 22,
            state_sections: vec![],
        }),
        Visibility::Hidden,
    );

    let draws = source_draws(&mut world, CreatorKind::Ship);
    assert_eq!(
        draws.len(),
        1,
        "selection rings and hidden source meshes must not draw"
    );
    let draw = &draws[0];
    assert_eq!(draw.entity, source);
    assert_ne!(draw.entity, ring);
    assert_ne!(draw.entity, hidden);
    assert_eq!(draw.mesh, mesh, "preview must reuse the source mesh asset");
    assert_eq!(
        draw.material, material,
        "preview must reuse the source material"
    );
    assert_eq!(draw.section, 17);
    assert_eq!(draw.states, vec![3, 8]);
    assert!(draw
        .transform
        .translation
        .abs_diff_eq(source_local.translation, 1e-4));
    assert!(draw.transform.rotation.dot(source_local.rotation).abs() > 0.9999);

    let (center, fit, floor_y) =
        bounds(&world, &draws).expect("native mesh positions define preview bounds");
    assert!(center.abs_diff_eq(source_local.translation, 1e-4));
    assert!((fit - 1.0).abs() < 1e-4);
    assert!((floor_y + 1.0).abs() < 1e-4);
}

#[test]
fn building_preview_contains_home_hall_and_custom_parts_but_not_walls_or_other_cities() {
    let mut world = World::new();
    let (mesh, material) = insert_assets(&mut world);
    let home = world
        .spawn((
            crate::civ::CityMarker(crate::civ::HOME_INDEX),
            Transform::default(),
            GlobalTransform::default(),
        ))
        .id();
    let hall = attach_drawable(
        &mut world,
        home,
        &mesh,
        &material,
        Transform::from_xyz(0.0, 1.0, 0.0),
        None,
        Visibility::Inherited,
    );
    world
        .entity_mut(hall)
        .insert(crate::civ::NativeCityHallSurface(
            crate::dbpf::ResourceKey {
                type_id: 1,
                group_id: 2,
                instance_id: 3,
            },
        ));

    let custom_part = attach_drawable(
        &mut world,
        home,
        &mesh,
        &material,
        Transform::from_xyz(2.0, 0.0, 0.0),
        None,
        Visibility::Inherited,
    );
    world.entity_mut(custom_part).insert((
        crate::civ::CityPartRef {
            city: crate::civ::HOME_INDEX,
            part: 4,
        },
        crate::creation_assets::OriginalCreationPart {
            definition: crate::dbpf::ResourceKey {
                type_id: 4,
                group_id: 5,
                instance_id: 6,
            },
        },
    ));

    let wall = attach_drawable(
        &mut world,
        home,
        &mesh,
        &material,
        Transform::from_xyz(0.0, 0.0, 3.0),
        None,
        Visibility::Inherited,
    );
    let other_city = world
        .spawn((
            crate::civ::CityMarker(crate::civ::HOME_INDEX + 1),
            Transform::from_xyz(50.0, 0.0, 0.0),
            GlobalTransform::from(Transform::from_xyz(50.0, 0.0, 0.0)),
        ))
        .id();
    let other_hall = attach_drawable(
        &mut world,
        other_city,
        &mesh,
        &material,
        Transform::default(),
        None,
        Visibility::Inherited,
    );
    world
        .entity_mut(other_hall)
        .insert(crate::civ::NativeCityHallSurface(
            crate::dbpf::ResourceKey {
                type_id: 7,
                group_id: 8,
                instance_id: 9,
            },
        ));

    let draws = source_draws(&mut world, CreatorKind::Building);
    let selected: Vec<Entity> = draws.iter().map(|draw| draw.entity).collect();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&hall));
    assert!(selected.contains(&custom_part));
    assert!(
        !selected.contains(&wall),
        "unmarked wall dressing is not a building part"
    );
    assert!(
        !selected.contains(&other_hall),
        "a different city must not enter the home preview"
    );
}

#[test]
fn closing_preview_restores_camera_states_and_despawns_only_owned_preview_entities() {
    let mut world = World::new();
    let (mesh, material) = insert_assets(&mut world);
    let initially_active = world.spawn((crate::GameCamera, Camera::default())).id();
    let initially_inactive = world
        .spawn((
            crate::GameCamera,
            Camera {
                is_active: false,
                ..default()
            },
        ))
        .id();
    let player_transform = Transform::from_xyz(3.0, 0.0, 0.0);
    let player = world
        .spawn((
            crate::game::Player,
            player_transform,
            GlobalTransform::from(player_transform),
        ))
        .id();
    let source = attach_drawable(
        &mut world,
        player,
        &mesh,
        &material,
        Transform::default(),
        Some(OriginalSurfaceChild {
            source_section: 4,
            state_sections: vec![1],
        }),
        Visibility::Inherited,
    );
    world.insert_resource(MechanicalCreator {
        open: true,
        kind: CreatorKind::Ship,
        ..default()
    });

    sync(&mut world);
    assert_eq!(
        world.get::<Camera>(initially_active).unwrap().is_active,
        false
    );
    assert_eq!(
        world.get::<Camera>(initially_inactive).unwrap().is_active,
        false
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<PreviewDraw>>()
            .iter(&world)
            .count(),
        1,
        "opening should create a clone for the native source mesh"
    );
    assert!(world.get_entity(source).is_ok());
    assert!(world.get::<Mesh3d>(source).is_some());

    world.resource_mut::<MechanicalCreator>().open = false;
    sync(&mut world);

    assert!(world.get::<Camera>(initially_active).unwrap().is_active);
    assert!(!world.get::<Camera>(initially_inactive).unwrap().is_active);
    assert_eq!(
        world
            .query_filtered::<Entity, With<PreviewOwned>>()
            .iter(&world)
            .count(),
        0,
        "closing should remove the preview camera, lights, and draw clones"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<PreviewDraw>>()
            .iter(&world)
            .count(),
        0
    );
    assert!(world.get_entity(initially_active).is_ok());
    assert!(world.get_entity(initially_inactive).is_ok());
    assert!(world.get_entity(player).is_ok());
    assert!(world.get_entity(source).is_ok());
    assert!(world.get::<Mesh3d>(source).is_some());
    assert_eq!(world.resource::<PreviewScene>().draw_count, 0);
}
