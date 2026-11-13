//! Grounded selection cues for the playable Creature encounter.
//! Geometry and colors are adapted; these are not original Swarm effects.
use crate::{
    game::{Health, Npc, Player},
    persistence::WorldId,
    stages::{GameStage, StageOwned},
};
use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

#[derive(Component)]
pub struct SelectionRing {
    actor: Entity,
    target: bool,
}

#[derive(Resource)]
struct RingAssets {
    mesh: Handle<Mesh>,
    player: Handle<StandardMaterial>,
    social: Handle<StandardMaterial>,
    combat: Handle<StandardMaterial>,
}

pub fn update(world: &mut World) {
    let visible = world.resource::<State<GameStage>>().get() == &GameStage::Creature
        && !world.resource::<crate::editor::EditorState>().open;
    let existing: Vec<_> = world
        .query::<(Entity, &SelectionRing)>()
        .iter(world)
        .map(|(e, r)| (e, r.actor, r.target))
        .collect();
    if !visible {
        for (entity, _, _) in existing {
            world.despawn(entity);
        }
        return;
    }
    let Some(surface) = world.get_resource::<crate::terrain::LandSurface>().cloned() else {
        return;
    };
    let target = world
        .get_resource::<crate::creature_actions::Encounter>()
        .and_then(|e| e.target);
    let combat = world
        .get_resource::<crate::creature_actions::Encounter>()
        .is_some_and(|e| e.mode == crate::creature_actions::EncounterMode::Combat);
    let mut desired = Vec::new();
    for (actor, transform, player, npc, health, id) in world
        .query::<(
            Entity,
            &Transform,
            Option<&Player>,
            Option<&Npc>,
            Option<&Health>,
            Option<&WorldId>,
        )>()
        .iter(world)
    {
        let selected_target =
            npc.is_some_and(|n| n.hp > 0.) && id.is_some_and(|id| Some(id.0) == target);
        if (player.is_some() && health.is_none_or(|h| h.hp > 0.)) || selected_target {
            desired.push((
                actor,
                transform.translation,
                transform.scale.x,
                selected_target,
            ));
        }
    }
    if world.get_resource::<RingAssets>().is_none() {
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Torus::new(0.95, 1.0));
        let make = |color: Color| StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.35,
            unlit: true,
            cull_mode: None,
            ..default()
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let player = materials.add(make(Color::srgb(0.3, 0.95, 0.25)));
        let social = materials.add(make(Color::srgb(0.15, 0.7, 1.)));
        let combat = materials.add(make(Color::srgb(1., 0.22, 0.08)));
        world.insert_resource(RingAssets {
            mesh,
            player,
            social,
            combat,
        });
    }
    for (entity, actor, selected_target) in &existing {
        if !desired
            .iter()
            .any(|d| d.0 == *actor && d.3 == *selected_target)
        {
            world.despawn(*entity);
        }
    }
    for (actor, position, size, selected_target) in desired {
        let assets = world.resource::<RingAssets>();
        let mesh = assets.mesh.clone();
        let material = if !selected_target {
            assets.player.clone()
        } else if combat {
            assets.combat.clone()
        } else {
            assets.social.clone()
        };
        let transform = Transform::from_xyz(
            position.x,
            surface.height(position.x, position.z) + 0.035,
            position.z,
        )
        .with_scale(Vec3::new(size, 0.08, size));
        if let Some((entity, _, _)) = existing
            .iter()
            .find(|(_, e, t)| *e == actor && *t == selected_target)
        {
            world
                .entity_mut(*entity)
                .insert((transform, MeshMaterial3d(material)));
        } else {
            world.spawn((
                StageOwned,
                SelectionRing {
                    actor,
                    target: selected_target,
                },
                Name::new("Creature selection cue"),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
}
