//! Original creator background geometry and materials, with adapted scene fit.
use crate::{
    asset_catalog::{self, PROP, RW4},
    dbpf::ResourceKey,
    editor::EditorState,
    material_binding::OriginalSurfaceChild,
    stages::GameStage,
    world_objects, OriginalModel, Originals,
};
use bevy::{mesh::VertexAttributeValues, prelude::*};

pub const CE_EDITOR_PROP: ResourceKey = ResourceKey {
    group_id: 0x40606000,
    instance_id: 0x01DCAF31,
    type_id: PROP,
};
pub const CL_EDITOR_MODEL: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x9EDFE49C,
    type_id: RW4,
};

#[derive(Component, Clone, Copy, Debug)]
pub struct EditorEnvironmentSurface(pub ResourceKey);

/// Adapted Cell creator water: light pool, drifting bubbles and motes over
/// the Cell stage's own original deep-water floor (kept visible by
/// `editor::scene`). These are Bevy presentation pieces, not native assets.
#[derive(Component, Clone, Copy, Debug)]
pub struct CellCreatorWater;

#[derive(Component, Clone, Copy, Debug)]
pub struct CellCreatorBubble {
    origin: Vec3,
    rise: f32,
    sway: f32,
    phase: f32,
}

#[derive(Resource, Default)]
pub struct EditorEnvironmentAssets {
    pub definition: Option<ResourceKey>,
    pub model: Option<ResourceKey>,
    pub diagnostic: Option<String>,
    attempted: bool,
    imported: Option<(OriginalModel, world_objects::OriginalWorldObject)>,
    cell_water: Option<CellWaterAssets>,
}

#[derive(Clone)]
struct CellWaterAssets {
    glow: Handle<Mesh>,
    glow_material: Handle<StandardMaterial>,
    bubble: Handle<Mesh>,
    bubble_material: Handle<StandardMaterial>,
    mote_material: Handle<StandardMaterial>,
}

/// Soft radial falloff used for the light pool and motes (white, alpha in
/// the falloff), so tints come from the material colour.
fn soft_disc(size: u32, hardness: f32) -> Image {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / size as f32 * 2. - Vec2::ONE;
            let r = p.length().min(1.);
            let a = (1. - r).powf(hardness).clamp(0., 1.);
            rgba.extend_from_slice(&[255, 255, 255, (a * 255.) as u8]);
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        rgba,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    )
}

fn cell_water_assets(
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> CellWaterAssets {
    let disc = images.add(soft_disc(128, 1.6));
    CellWaterAssets {
        glow: meshes.add(crate::cell_parallax::sprite_quad()),
        glow_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.42, 0.78, 0.95, 0.42),
            base_color_texture: Some(disc.clone()),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        }),
        bubble: meshes.add(Sphere::new(1.).mesh().ico(2).unwrap()),
        bubble_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.72, 0.9, 1., 0.22),
            emissive: LinearRgba::rgb(0.05, 0.12, 0.16),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.08,
            reflectance: 0.9,
            fog_enabled: false,
            ..default()
        }),
        mote_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.55, 0.85, 1., 0.3),
            base_color_texture: Some(disc),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        }),
    }
}

/// Spawn the light pool under the cell plus a ring of bubbles and motes.
fn spawn_cell_water(commands: &mut Commands, assets: &CellWaterAssets, center: Vec3) {
    // A broad teal light column under the cell, as in Spore's creator.
    for (scale, depth, alpha) in [(13., 2.4, 1.), (6.5, 1.6, 0.6)] {
        let mut material = assets.glow_material.clone();
        if alpha < 1. {
            material = assets.mote_material.clone();
        }
        commands.spawn((
            CellCreatorWater,
            Name::new("adapted Cell creator light pool"),
            Mesh3d(assets.glow.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(center - Vec3::Y * depth)
                .with_scale(Vec3::new(scale, 1., scale)),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
        ));
    }
    let mut seed = 0x5EED_C311u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.
    };
    for i in 0..26 {
        let angle = next() * std::f32::consts::TAU;
        let radius = 1.8 + next() * 4.2;
        let origin = center
            + Vec3::new(angle.cos() * radius, -2.2 + next() * 2.6, angle.sin() * radius);
        let size = if i % 3 == 0 {
            0.05 + next() * 0.05
        } else {
            0.025 + next() * 0.03
        };
        let bubble = CellCreatorBubble {
            origin,
            rise: 0.12 + next() * 0.2,
            sway: 0.04 + next() * 0.08,
            phase: next() * 10.,
        };
        if i % 4 == 3 {
            // Larger soft motes drifting in the water column.
            commands.spawn((
                CellCreatorWater,
                bubble,
                Mesh3d(assets.glow.clone()),
                MeshMaterial3d(assets.mote_material.clone()),
                Transform::from_translation(origin).with_scale(Vec3::splat(0.35 + size * 4.)),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        } else {
            commands.spawn((
                CellCreatorWater,
                bubble,
                Mesh3d(assets.bubble.clone()),
                MeshMaterial3d(assets.bubble_material.clone()),
                Transform::from_translation(origin).with_scale(Vec3::splat(size)),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        }
    }
}

/// Bubbles rise through a short column and wrap, swaying gently.
fn animate_cell_water(
    time: f32,
    bubbles: &mut Query<(&CellCreatorBubble, &mut Transform), Without<crate::game::Player>>,
) {
    for (bubble, mut transform) in bubbles.iter_mut() {
        let t = time + bubble.phase;
        let height = (t * bubble.rise).rem_euclid(3.2);
        transform.translation = bubble.origin
            + Vec3::new(
                (t * 0.7).sin() * bubble.sway,
                height - 1.2,
                (t * 0.5).cos() * bubble.sway,
            );
    }
}

/// Resolve a PROP's exact LOD0 key. The fallback is a separately named native
/// model, never a same-instance raster or a guessed group substitution.
fn room_definition(originals: &Originals) -> Result<asset_catalog::PartDefinition, String> {
    let definition =
        world_objects::definition_at(originals, CE_EDITOR_PROP, "ce_editor_background_sporepedia")
            .or_else(|_| {
                originals
                    .resource_index
                    .records
                    .contains_key(&CL_EDITOR_MODEL)
                    .then(|| asset_catalog::PartDefinition::raw_model(CL_EDITOR_MODEL))
                    .ok_or("no indexed original creator background".to_string())
            })?;
    let bytes = originals.resource_index.read(definition.model)?;
    if bytes.get(..crate::rw4::RW_MAGIC.len()) != Some(crate::rw4::RW_MAGIC)
        || bytes
            .get(0x1c..0x20)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            != Some(crate::rw4::RW_TYPE_MODEL)
    {
        return Err("creator LOD0 resource is not an RW4 model".into());
    }
    Ok(definition)
}

fn room_transform(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
    floor: Vec3,
) -> Result<Transform, String> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for surface in &model.surfaces {
        let mesh = meshes.get(&surface.handle).ok_or("creator mesh missing")?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("creator positions missing".into());
        };
        for p in positions {
            let p = Vec3::from_array(*p);
            if !p.is_finite() {
                return Err("creator bounds are non-finite".into());
            }
            min = min.min(p);
            max = max.max(p);
        }
    }
    let width = (max.x - min.x).max(max.z - min.z);
    if !width.is_finite() || width <= 0. {
        return Err("creator bounds are empty".into());
    }
    let scale = 24. / width;
    Ok(Transform::from_translation(
        floor - Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5) * scale,
    )
    .with_scale(Vec3::splat(scale)))
}

fn import_room(
    originals: &mut Originals,
    definition: asset_catalog::PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(OriginalModel, world_objects::OriginalWorldObject), String> {
    let (model, source) =
        world_objects::import_at(originals, definition, meshes, images, materials)?;
    let bytes = originals.resource_index.read(source.model)?;
    for surface in &model.surfaces {
        if let Some(state) =
            crate::material_binding::common_state(&bytes, &[surface.source_section])?
        {
            if state.renderer_id == 0x80000003 {
                let mut material = materials
                    .get_mut(&surface.material)
                    .ok_or("creator material missing")?;
                // Reopening a creator reuses this same imported material.
                if material.base_color_texture.is_none() {
                    crate::material_binding::apply_editor_background(
                        &state,
                        &bytes,
                        images,
                        &mut material,
                    )?;
                }
            }
        }
    }
    Ok((model, source))
}

pub fn sync(
    mut commands: Commands,
    state: Res<State<GameStage>>,
    editor: Res<EditorState>,
    mut originals: ResMut<Originals>,
    mut assets: ResMut<EditorEnvironmentAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<EditorEnvironmentSurface>>,
    players: Query<&Transform, (With<crate::game::Player>, Without<CellCreatorBubble>)>,
    land: Option<Res<crate::terrain::LandSurface>>,
    time: Res<Time>,
    water: Query<Entity, With<CellCreatorWater>>,
    mut bubbles: Query<(&CellCreatorBubble, &mut Transform), Without<crate::game::Player>>,
) {
    let cell_creator = editor.open && *state.get() == GameStage::Cell;
    if !cell_creator {
        for entity in &water {
            commands.entity(entity).despawn();
        }
    } else if water.is_empty() {
        let center = players.iter().next().map_or(Vec3::ZERO, |t| t.translation);
        let water_assets = assets
            .cell_water
            .get_or_insert_with(|| cell_water_assets(&mut meshes, &mut images, &mut materials))
            .clone();
        spawn_cell_water(&mut commands, &water_assets, center);
    } else {
        animate_cell_water(time.elapsed_secs(), &mut bubbles);
    }
    let allowed = editor.open
        && matches!(
            *state.get(),
            GameStage::MainMenu | GameStage::Creature | GameStage::Tribal
        );
    if !allowed {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }
    if !assets.attempted {
        assets.attempted = true;
        let result = room_definition(&originals).and_then(|definition| {
            assets.definition = Some(definition.key);
            assets.model = Some(definition.model);
            import_room(
                &mut originals,
                definition,
                &mut meshes,
                &mut images,
                &mut materials,
            )
        });
        match result {
            Ok((model, source)) if !model.surfaces.is_empty() => {
                assets.imported = Some((model, source))
            }
            Ok(_) => assets.diagnostic = Some("creator model has no draw surfaces".into()),
            Err(error) => {
                warn!("original creator background: {error}");
                assets.diagnostic = Some(error);
            }
        }
    }
    let Some((model, source)) = &assets.imported else {
        return;
    };
    let actor = players.iter().next().copied().unwrap_or_default();
    let floor_y = if *state.get() == GameStage::MainMenu {
        actor.translation.y - 1.3 * actor.scale.y
    } else {
        land.as_deref()
            .map_or(actor.translation.y - 1.3 * actor.scale.y, |l| {
                l.height(actor.translation.x, actor.translation.z)
            })
    };
    let floor = Vec3::new(actor.translation.x, floor_y, actor.translation.z);
    let Ok(transform) = room_transform(model, &meshes, floor) else {
        return;
    };
    for surface in &model.surfaces {
        commands.spawn((
            EditorEnvironmentSurface(source.model),
            source.clone(),
            OriginalSurfaceChild {
                source_section: surface.source_section,
                state_sections: surface.state_sections.clone(),
            },
            Name::new(format!(
                "original creator background surface {}",
                surface.source_section
            )),
            Mesh3d(surface.handle.clone()),
            MeshMaterial3d(surface.material.clone()),
            transform,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires installed original Spore assets"]
    fn installed_editor_environment_source_imports() {
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = asset_catalog::ResourceIndex::load(&install.packages);
        let definition = room_definition(&originals).unwrap();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let (model, source) = import_room(
            &mut originals,
            definition,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap();
        assert_eq!(source.definition, CE_EDITOR_PROP);
        assert_eq!(
            source.model,
            ResourceKey {
                type_id: RW4,
                ..CE_EDITOR_PROP
            }
        );
        assert!(!model.surfaces.is_empty());
        for surface in &model.surfaces {
            let mesh = meshes.get(&surface.handle).unwrap();
            assert!(mesh.count_vertices() > 0);
            let material = materials.get(&surface.material).unwrap();
            let bytes = originals.resource_index.read(source.model).unwrap();
            let state =
                crate::material_binding::common_state(&bytes, &[surface.source_section]).unwrap();
            assert!(material
                .base_color_texture
                .as_ref()
                .is_some_and(|h| images.contains(h)));
            println!(
                "creator surface {} renderer {:08X}, exact diffuse bound, unlit preview {}",
                surface.source_section,
                state.unwrap().renderer_id,
                material.unlit
            );
        }
        let fit = room_transform(&model, &meshes, Vec3::new(3., -0.6, 4.)).unwrap();
        assert!(fit.translation.is_finite() && fit.scale.min_element() > 0.);
        println!(
            "creator background: {:?}, {} surfaces, adapted fit {:?}",
            source.model,
            model.surfaces.len(),
            fit
        );
    }
}
