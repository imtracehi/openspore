use super::*;

fn vertex_positions(mesh: &Mesh) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("asteroid mesh positions are missing or have the wrong format");
    };
    positions.iter().copied().map(Vec3::from_array).collect()
}

fn assert_camera_plane(
    mesh: &Mesh,
    sprites: &[Sprite],
    belt_transform: GlobalTransform,
    camera_transform: GlobalTransform,
) {
    let vertices = vertex_positions(mesh);
    let (_, camera_rotation, _) = camera_transform.to_scale_rotation_translation();
    let camera_forward = camera_rotation * Vec3::NEG_Z;
    assert_eq!(vertices.len(), sprites.len() * 4);

    for (sprite, quad) in sprites.iter().zip(vertices.chunks_exact(4)) {
        let affine = belt_transform.affine();
        let world_quad = [
            affine.transform_point3(quad[0]),
            affine.transform_point3(quad[1]),
            affine.transform_point3(quad[2]),
            affine.transform_point3(quad[3]),
        ];
        let world_center = affine.transform_point3(sprite.center);
        let quad_center = quad.iter().copied().sum::<Vec3>() / 4.0;
        assert!(affine
            .transform_point3(quad_center)
            .abs_diff_eq(world_center, 1e-4));

        for vertex in world_quad {
            let offset = vertex - world_center;
            assert!(
                camera_forward.dot(offset).abs() < 1e-4,
                "sprite vertex is outside the camera-facing plane"
            );
        }

        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
            let edge = world_quad[a].distance(world_quad[b]);
            assert!((edge - 2.0 * sprite.half_size).abs() < 1e-4);
        }
    }
}

fn uv_values(mesh: &Mesh) -> Vec<[f32; 2]> {
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        panic!("asteroid mesh UVs are missing or have the wrong format");
    };
    uvs.clone()
}

fn crop_uvs(quad_count: usize) -> Vec<[f32; 2]> {
    let quad = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    quad.into_iter().cycle().take(quad_count * 4).collect()
}

#[test]
fn camera_facing_system_tracks_rotated_camera_and_belt_without_changing_source_data() {
    let mut app = App::new();
    let mut meshes = Assets::<Mesh>::default();
    let sprites = vec![
        Sprite {
            center: Vec3::new(2.0, 0.4, -3.0),
            half_size: 0.35,
            angle: 0.2,
            frame: 2,
        },
        Sprite {
            center: Vec3::new(-4.0, -0.3, 1.5),
            half_size: 0.7,
            angle: 1.1,
            frame: 2,
        },
    ];
    let mesh_handle = meshes.add(mesh(&sprites));
    let expected_uvs = crop_uvs(sprites.len());
    assert_eq!(uv_values(meshes.get(&mesh_handle).unwrap()), expected_uvs);
    app.insert_resource(meshes)
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, face_camera);

    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let source = ASTEROID_ATLAS;
    let instances = sprites.len();
    let belt_entity = app
        .world_mut()
        .spawn((
            NativeAsteroidBelt {
                source,
                instances,
                frame: 2,
                sprites: sprites.clone(),
            },
            Mesh3d(mesh_handle.clone()),
            MeshMaterial3d(material.clone()),
            GlobalTransform::from(
                Transform::from_xyz(10.0, -2.0, 7.0).with_rotation(Quat::from_euler(
                    EulerRot::YXZ,
                    0.7,
                    -0.3,
                    0.25,
                )),
            ),
        ))
        .id();
    let camera_entity = app
        .world_mut()
        .spawn((
            crate::GameCamera,
            GlobalTransform::from(Transform::from_rotation(Quat::from_euler(
                EulerRot::YXZ,
                -0.4,
                0.55,
                0.1,
            ))),
        ))
        .id();

    app.update();
    let first_global = *app.world().get::<GlobalTransform>(belt_entity).unwrap();
    let first_camera = *app.world().get::<GlobalTransform>(camera_entity).unwrap();
    let first_positions = {
        let meshes = app.world().resource::<Assets<Mesh>>();
        let mesh = meshes.get(&mesh_handle).unwrap();
        assert_camera_plane(mesh, &sprites, first_global, first_camera);
        vertex_positions(mesh)
    };
    assert_eq!(
        uv_values(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&mesh_handle)
                .unwrap()
        ),
        expected_uvs
    );

    *app.world_mut()
        .get_mut::<GlobalTransform>(belt_entity)
        .unwrap() = GlobalTransform::from(
        Transform::from_xyz(-3.0, 5.0, 1.0).with_rotation(Quat::from_euler(
            EulerRot::XYZ,
            -0.6,
            0.9,
            -0.2,
        )),
    );
    *app.world_mut()
        .get_mut::<GlobalTransform>(camera_entity)
        .unwrap() = GlobalTransform::from(Transform::from_rotation(Quat::from_euler(
        EulerRot::ZYX,
        0.25,
        -0.8,
        1.2,
    )));
    app.update();

    let second_global = *app.world().get::<GlobalTransform>(belt_entity).unwrap();
    let second_camera = *app.world().get::<GlobalTransform>(camera_entity).unwrap();
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mesh = meshes.get(&mesh_handle).unwrap();
    assert_camera_plane(mesh, &sprites, second_global, second_camera);
    let second_positions = vertex_positions(mesh);
    assert_ne!(
        first_positions, second_positions,
        "camera/belt rotation changes should rebuild the facing basis"
    );
    assert_eq!(
        uv_values(mesh),
        expected_uvs,
        "camera-facing updates must retain source atlas frames"
    );

    let belt = app.world().get::<NativeAsteroidBelt>(belt_entity).unwrap();
    assert_eq!(belt.source, source);
    assert_eq!(belt.instances, instances);
    assert_eq!(belt.frame, 2);
    assert_eq!(belt.sprites.len(), sprites.len());
    for (actual, expected) in belt.sprites.iter().zip(&sprites) {
        assert_eq!(actual.center, expected.center);
        assert_eq!(actual.half_size, expected.half_size);
        assert_eq!(actual.angle, expected.angle);
        assert_eq!(actual.frame, expected.frame);
    }
    assert_eq!(
        app.world().get::<Mesh3d>(belt_entity).unwrap().0,
        mesh_handle
    );
    assert_eq!(
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(belt_entity)
            .unwrap()
            .0,
        material
    );
}

#[test]
fn camera_facing_system_is_safe_when_camera_or_mesh_assets_are_unavailable() {
    let mut app = App::new();
    app.add_systems(Update, face_camera);
    app.update();

    app.init_resource::<Assets<Mesh>>();
    app.world_mut().spawn((
        NativeAsteroidBelt {
            source: ASTEROID_ATLAS,
            instances: 0,
            frame: 0,
            sprites: Vec::new(),
        },
        Mesh3d(Handle::<Mesh>::default()),
        GlobalTransform::IDENTITY,
    ));
    app.update();
}
