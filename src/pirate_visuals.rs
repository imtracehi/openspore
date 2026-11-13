//! Original creation-geometry preview for Space pirates.
//!
//! No pirate-specific creation has been verified in the installed catalog.
//! This explicitly adapts the authored `genufo` spaceship as a pirate visual;
//! the pirate actor and all gameplay state remain on their original entity.

use bevy::{light::NotShadowCaster, prelude::*};

use crate::{
    creation_assets::{self, PresetKind},
    dbpf::ResourceKey,
    space::Pirate,
    Originals,
};

/// Identifies the authored spaceship creation adapted for this pirate actor.
#[derive(Component, Debug, Clone, Copy)]
pub struct OriginalPirateVisual(pub ResourceKey);

#[derive(Component)]
pub(crate) struct PirateVisualAttempted;

/// Replace the red cuboid only after the complete native creation assembly
/// has imported and mounted successfully. Missing catalogs or failed imports
/// leave both the fallback meshes and pirate gameplay components untouched.
pub(crate) fn mount_original_pirate_visuals(
    mut commands: Commands,
    originals: Option<ResMut<Originals>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut marker: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    pirates: Query<
        (Entity, &Pirate),
        (
            With<Mesh3d>,
            Without<OriginalPirateVisual>,
            Without<PirateVisualAttempted>,
        ),
    >,
) {
    let (Some(mut originals), Some(mut meshes), Some(mut images), Some(mut materials)) =
        (originals, meshes, images, materials)
    else {
        return;
    };

    // The stage creates four pirates at a time. Keep the visual importer
    // bounded if a future gameplay change raises that population.
    for (entity, _pirate) in pirates.iter().take(6) {
        let Some(preset) = creation_assets::select_creation(
            &originals.creation_catalog.creations,
            PresetKind::Spaceship,
        ) else {
            commands.entity(entity).insert(PirateVisualAttempted);
            continue;
        };
        let key = preset.key;
        match creation_assets::mount_preset(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            entity,
            PresetKind::Spaceship,
            1.8,
        ) {
            Ok(true) => {
                commands.entity(entity).remove::<Mesh3d>();
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>();
                commands.entity(entity).insert(OriginalPirateVisual(key));
                let (mesh, material) = marker.get_or_insert_with(|| {
                    (
                        meshes.add(Torus::new(0.92, 0.98)),
                        materials.add(StandardMaterial {
                            base_color: Color::srgb(0.95, 0.15, 0.1),
                            emissive: Color::srgb(0.85, 0.08, 0.02).into(),
                            unlit: true,
                            ..default()
                        }),
                    )
                });
                commands.entity(entity).with_children(|parent| {
                    parent.spawn((
                        crate::stages::StageOwned,
                        NotShadowCaster,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::IDENTITY,
                    ));
                });
            }
            Ok(false) => {
                commands.entity(entity).insert(PirateVisualAttempted);
            }
            Err(error) => {
                warn!("could not mount adapted genufo pirate visual: {error}");
                commands.entity(entity).insert(PirateVisualAttempted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::Pirate;

    fn pirate_entity(world: &mut World) -> (Entity, Handle<Mesh>, Handle<StandardMaterial>) {
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.2, 0.4, 1.2));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.1, 0.1),
                ..default()
            });
        let entity = world
            .spawn((
                Pirate {
                    hp: 42.0,
                    max: 60.0,
                    speed: 9.0,
                    rewarded: false,
                    is_grox: false,
                },
                crate::space::SystemBody,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(7.0, 4.0, -3.0),
            ))
            .id();
        (entity, mesh, material)
    }

    #[test]
    fn absent_originals_preserves_pirate_fallback_and_state() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, mount_original_pirate_visuals);
        let (entity, mesh, material) = pirate_entity(app.world_mut());
        app.update();

        let world = app.world();
        assert_eq!(world.get::<Mesh3d>(entity).unwrap().0, mesh);
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0,
            material
        );
        assert!(world.get::<OriginalPirateVisual>(entity).is_none());
        assert_eq!(world.get::<Pirate>(entity).unwrap().hp, 42.0);
        assert!(world.get::<crate::space::SystemBody>(entity).is_some());
        assert_eq!(
            world.get::<Transform>(entity).unwrap().translation,
            Vec3::new(7.0, 4.0, -3.0)
        );
    }

    #[test]
    fn missing_spaceship_preset_keeps_primitive_and_gameplay_identity() {
        let mut app = App::new();
        app.init_resource::<Originals>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, mount_original_pirate_visuals);
        let (entity, mesh, material) = pirate_entity(app.world_mut());
        app.update();

        let world = app.world();
        assert_eq!(world.get::<Mesh3d>(entity).unwrap().0, mesh);
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0,
            material
        );
        assert!(world.get::<OriginalPirateVisual>(entity).is_none());
        let pirate = world.get::<Pirate>(entity).unwrap();
        assert_eq!((pirate.hp, pirate.max, pirate.speed), (42.0, 60.0, 9.0));
        assert!(!pirate.rewarded);
        assert!(world.get::<crate::space::SystemBody>(entity).is_some());
    }

    #[test]
    fn failed_preset_import_preserves_fallback_and_gameplay_identity() {
        let mut app = App::new();
        let mut originals = Originals::default();
        originals
            .creation_catalog
            .creations
            .push(crate::creation_records::CreationRecord {
                key: ResourceKey {
                    type_id: crate::creation_records::UFO,
                    group_id: 0x4065_6200,
                    instance_id: 0x6ABF_210B,
                },
                format_version: 15,
                name: "genufo".into(),
                model_type: 0x98E0_3C0D,
                blocks: vec![crate::creation_records::CreationBlock {
                    definition: ResourceKey {
                        type_id: crate::asset_catalog::PROP,
                        group_id: 0xDEAD_BEEF,
                        instance_id: 0xABCD_1234,
                    },
                    scale: 1.0,
                    position: [0.0; 3],
                    orientation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    paints: vec![],
                    handles: vec![],
                }],
            });
        app.insert_resource(originals)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, mount_original_pirate_visuals);
        let (entity, mesh, material) = pirate_entity(app.world_mut());
        app.update();

        let world = app.world();
        assert_eq!(world.get::<Mesh3d>(entity).unwrap().0, mesh);
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0,
            material
        );
        assert!(world.get::<OriginalPirateVisual>(entity).is_none());
        let pirate = world.get::<Pirate>(entity).unwrap();
        assert_eq!((pirate.hp, pirate.max, pirate.speed), (42.0, 60.0, 9.0));
        assert!(!pirate.rewarded);
        assert!(world.get::<crate::space::SystemBody>(entity).is_some());
        assert!(world.get::<Transform>(entity).is_some());
        assert!(world.get::<PirateVisualAttempted>(entity).is_some());
    }
}
