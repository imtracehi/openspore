//! Show the untouched starter's complete authored model in play and the creator.
//!
//! This is a visual-only presentation choice. It does not change the run,
//! player entity, movement, health, or the editable anatomy used after a build
//! change.
use crate::{
    creature::Accessory,
    editor::{self, EditorState},
    game::{Player, RunProgress},
    stages::{GameStage, StageOwned},
    OriginalModel, Originals,
};
use bevy::prelude::*;

/// Marker on every original source draw child. Distinct from the NPC-only
/// `baked_model::NativeCreature` marker used by population visuals.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePlayerSurface(pub crate::dbpf::ResourceKey);

/// The player's own primitive draw components, saved while authored surfaces
/// replace them. We restore these exact handles when the build becomes custom.
#[derive(Component, Clone)]
struct SavedRootVisual {
    mesh: Mesh3d,
    material: MeshMaterial3d<StandardMaterial>,
}

/// Part meshes are the editable build's separate draw children. While an
/// untouched source model supplies those same parts, retain the children and
/// their exact visibility so a later build change can restore them.
#[derive(Component, Clone, Copy)]
struct SavedMountedPartVisibility(Option<Visibility>);

fn authored_baseline(world: &World) -> Option<(crate::dbpf::ResourceKey, OriginalModel)> {
    if !matches!(
        *world.resource::<State<GameStage>>().get(),
        GameStage::MainMenu | GameStage::Creature | GameStage::Tribal
    ) && !world.get_resource::<RunProgress>().is_some_and(crate::outfitter::in_session)
    {
        return None;
    }
    let run = world.get_resource::<RunProgress>()?;
    let editor = world.get_resource::<EditorState>()?;
    let originals = world.get_resource::<Originals>()?;
    // A painted creature (its own paint or its outfit's) shows its skin: the
    // authored model cannot wear the paint.
    if editor.body_editing()
        || run.body != crate::body::BodyProfile::default()
        || !editor::is_starter_creature_parts(&run.parts, &originals.part_catalog)
        || crate::outfitter::shown_paint(run).painted
    {
        return None;
    }
    let key = crate::baked_model::player_source_key(originals)?;
    let model = originals.baked_models.get(&key)?.clone();
    (!model.surfaces.is_empty()).then_some((key, model))
}

/// Reconcile the authored full-creature display with the current editor build.
/// Register this exclusive system in `Presentation` after `editor::refresh_parts`.
pub fn sync(world: &mut World) {
    if !world.contains_resource::<State<GameStage>>()
        || (!matches!(
            *world.resource::<State<GameStage>>().get(),
            GameStage::MainMenu | GameStage::Creature | GameStage::Tribal
        ) && !world.get_resource::<RunProgress>().is_some_and(crate::outfitter::in_session))
    {
        return;
    }
    let authored = authored_baseline(world);
    let stage = world.resource::<State<GameStage>>().get().clone();
    let actors: Vec<Entity> = if stage == GameStage::Tribal {
        let mut query =
            world.query_filtered::<Entity, Or<(With<Player>, With<crate::game::TribeMate>)>>();
        query.iter(world).collect()
    } else {
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(world).collect()
    };

    for actor in actors {
        if let Some((key, model)) = authored.as_ref() {
            if world
                .entity(actor)
                .contains::<crate::baked_model::NativePlayerSource>()
            {
                continue;
            }
            let (Some(mesh), Some(material)) = (
                world.get::<Mesh3d>(actor).cloned(),
                world
                    .get::<MeshMaterial3d<StandardMaterial>>(actor)
                    .cloned(),
            ) else {
                // A partially initialized actor keeps its current view.
                continue;
            };
            let Ok(mut transform) = crate::baked_model::player_fitted_transform(
                model,
                world.resource::<Assets<Mesh>>(),
            ) else {
                continue;
            };

            // The outfitter's preview creature stands alone like the free
            // creator's (no land under it).
            let preview = world.get::<crate::outfitter::OutfitterPreview>(actor).is_some();
            if let Some(actor_transform) = world.get::<Transform>(actor) {
                if actor_transform.scale.y.is_finite() && actor_transform.scale.y > 0. {
                    let ground = if stage == GameStage::MainMenu || preview {
                        actor_transform.translation.y - 1.3 * actor_transform.scale.y
                    } else {
                        world.get_resource::<crate::terrain::LandSurface>().map_or(
                            -0.6,
                            |surface| {
                                surface.height(
                                    actor_transform.translation.x,
                                    actor_transform.translation.z,
                                )
                            },
                        )
                    };
                    transform.translation.y +=
                        1.3 + (ground - actor_transform.translation.y) / actor_transform.scale.y;
                }
            }

            world.entity_mut(actor).insert((
                SavedRootVisual { mesh, material },
                crate::baked_model::NativePlayerSource(*key),
            ));
            world.entity_mut(actor).remove::<Mesh3d>();
            world
                .entity_mut(actor)
                .remove::<MeshMaterial3d<StandardMaterial>>();

            // Creature-stage legacy editor parts can be detached Accessory
            // entities. Tribal outfits are parented and carry TribalOutfit, so
            // never remove either outfit or other equipment here.
            if matches!(stage, GameStage::MainMenu | GameStage::Creature) || preview {
                let old_accessories: Vec<Entity> = {
                    let mut query =
                        world.query_filtered::<Entity, (With<Accessory>, Without<ChildOf>)>();
                    query.iter(world).collect()
                };
                for entity in old_accessories {
                    world.entity_mut(entity).despawn();
                }
            }
            let old_body_surfaces: Vec<_> = world
                .query_filtered::<(Entity, &ChildOf), With<crate::material_binding::OriginalSurfaceChild>>()
                .iter(world)
                .filter_map(|(entity, parent)| (parent.parent() == actor).then_some(entity))
                .collect();
            for entity in old_body_surfaces {
                world.despawn(entity);
            }

            // This authored model supplies a coherent baseline appearance.
            // Hide the editable mounted pieces in place; retain them so a change
            // to saved anatomy can restore the original editable presentation.
            let mounted_parts: Vec<Entity> = {
                let mut query = world
                    .query_filtered::<(Entity, &ChildOf), With<crate::creature::MountedPart>>();
                query
                    .iter(world)
                    .filter_map(|(entity, parent)| (parent.parent() == actor).then_some(entity))
                    .collect()
            };
            for part in mounted_parts {
                let visibility = world.get::<Visibility>(part).copied();
                world
                    .entity_mut(part)
                    .insert((SavedMountedPartVisibility(visibility), Visibility::Hidden));
            }

            for surface in model.surfaces.iter() {
                world.spawn((
                    StageOwned,
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    NativePlayerSurface(*key),
                    crate::baked_model::source(*key),
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                    ChildOf(actor),
                ));
            }
        } else if world
            .entity(actor)
            .contains::<crate::baked_model::NativePlayerSource>()
        {
            let authored_children: Vec<Entity> = {
                let mut query =
                    world.query_filtered::<(Entity, &ChildOf), With<NativePlayerSurface>>();
                query
                    .iter(world)
                    .filter_map(|(entity, child_of)| (child_of.parent() == actor).then_some(entity))
                    .collect()
            };
            for entity in authored_children {
                world.entity_mut(entity).despawn();
            }
            world
                .entity_mut(actor)
                .remove::<crate::baked_model::NativePlayerSource>();
            world
                .entity_mut(actor)
                .remove::<crate::locomotion_preview::AdaptedWalk>();
            if let Some(saved) = world.entity_mut(actor).take::<SavedRootVisual>() {
                world.entity_mut(actor).insert((saved.mesh, saved.material));
            }
            let mounted_parts: Vec<Entity> = {
                let mut query = world.query_filtered::<Entity, With<SavedMountedPartVisibility>>();
                query
                    .iter(world)
                    .filter(|entity| {
                        world
                            .get::<ChildOf>(*entity)
                            .is_some_and(|p| p.parent() == actor)
                    })
                    .collect()
            };
            for part in mounted_parts {
                if let Some(saved) = world.entity_mut(part).take::<SavedMountedPartVisibility>() {
                    if let Some(visibility) = saved.0 {
                        world.entity_mut(part).insert(visibility);
                    } else {
                        world.entity_mut(part).remove::<Visibility>();
                    }
                }
            }
            if let Some(mut editor) = world.get_resource_mut::<EditorState>() {
                editor.visuals_dirty = true;
            }
        }
    }
}
