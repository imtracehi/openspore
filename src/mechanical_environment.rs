//! Original native studio surfaces for the modal mechanical creator.
//!
//! These models are the source-authored creator sets in group 40606000. Their
//! RW4 geometry and material bindings are imported through the same path as
//! original world objects, then uniformly fitted to the caller's already
//! normalized preview subject. The fitting adapts scene scale and placement;
//! it does not replace model geometry, texture bindings, or material flags.

use crate::{
    asset_catalog::{self, PartDefinition},
    dbpf::ResourceKey,
    material_binding::OriginalSurfaceChild,
    mechanical_creator::CreatorKind,
    world_objects::{self, OriginalWorldObject},
    Originals,
};
use bevy::{camera::visibility::RenderLayers, mesh::VertexAttributeValues, prelude::*};
use std::collections::HashMap;

pub const MECHANICAL_PREVIEW_LAYER: usize = 30;

const fn prop(instance_id: u32) -> ResourceKey {
    ResourceKey {
        group_id: 0x40606000,
        instance_id,
        type_id: asset_catalog::PROP,
    }
}

pub const BUILDING_BASE: ResourceKey = prop(0x3C3B3DB1); // BE_editor_base_low
pub const BUILDING_BACKGROUND: ResourceKey = prop(0x93447B52); // be_editor_background_night
pub const VEHICLE_BASE: ResourceKey = prop(0xC7A4014D); // ve_editor_landbase
pub const VEHICLE_BACKGROUND: ResourceKey = prop(0xDAB70DA4); // ve_editor_landbase_background
pub const SHIP_BASE: ResourceKey = prop(0x89779F80); // ve_editor_ufobase
pub const SHIP_BACKGROUND: ResourceKey = prop(0x1FF03EF7); // ve_editor_ufobase_background

#[derive(Component, Clone, Copy, Debug)]
pub struct MechanicalEnvironmentOwned;

/// Records the exact native model supplying each rendered studio surface.
#[derive(Component, Clone, Debug)]
pub struct MechanicalEnvironmentSurface {
    pub definition: ResourceKey,
    pub model: ResourceKey,
    pub source_section: usize,
    pub role: MechanicalEnvironmentRole,
    pub material_mode: EnvironmentMaterialMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MechanicalEnvironmentRole {
    Base,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentMaterialMode {
    NativeStaticPreview,
    AdaptedEditorBackground,
    AdaptedEditorPanorama,
    GenericFallback,
}

/// The caller supplies the normalized subject's frame. `floor_y` is the
/// desired height of the fitted base's highest native vertex. The returned
/// frame lets the caller place the subject consistently with the fitted set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubjectFrame {
    pub center: Vec3,
    pub extent: f32,
    pub floor_y: f32,
}

/// Optional root-provided preview frame. A normalized four-unit subject with
/// its base at -1.8 is used when the root does not provide one.
#[derive(Resource, Clone, Copy, Debug)]
pub struct MechanicalEnvironmentSubject(pub SubjectFrame);

impl Default for MechanicalEnvironmentSubject {
    fn default() -> Self {
        Self(SubjectFrame {
            center: Vec3::ZERO,
            extent: 4.0,
            floor_y: -1.8,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnvironmentFit {
    pub center: Vec3,
    pub base_floor_y: f32,
    pub base_top_y: f32,
    pub base_scale: f32,
    pub base_width: f32,
    pub background_scale: f32,
}

/// Retains the small, fixed set of adapted native materials across modal
/// reopenings. Keys are exact native model keys and surface sections, so this
/// cache is bounded by the six indexed source models.
#[derive(Resource, Default)]
pub struct MechanicalEnvironmentCache {
    materials: HashMap<(ResourceKey, usize), (Handle<StandardMaterial>, EnvironmentMaterialMode)>,
}

#[derive(Clone)]
struct ImportedSet {
    definition: PartDefinition,
    source: OriginalWorldObject,
    model: crate::OriginalModel,
    role: MechanicalEnvironmentRole,
    material_modes: Vec<EnvironmentMaterialMode>,
}

fn isolate_materials(
    originals: &Originals,
    source: &OriginalWorldObject,
    model: &mut crate::OriginalModel,
    cache: &mut MechanicalEnvironmentCache,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Vec<EnvironmentMaterialMode>, String> {
    let bytes = originals.resource_index.read(source.model)?;
    let mut modes = Vec::with_capacity(model.surfaces.len());
    for surface in &mut model.surfaces {
        let cache_key = (source.model, surface.source_section);
        if let Some((material, mode)) = cache.materials.get(&cache_key) {
            surface.material = material.clone();
            modes.push(*mode);
            continue;
        }
        let mut material = materials
            .get(&surface.material)
            .cloned()
            .ok_or("mechanical environment material missing")?;
        let state = crate::material_binding::common_state(&bytes, &[surface.source_section])?;
        let mode = if let Some(state) = state {
            if state.renderer_id == 0x80000003 {
                // This is the project's explicitly supported bridge for the
                // creator background shader. It binds its authored diffuse
                // raster, alpha and cull state in an unlit Bevy preview; the
                // native shader itself and any shader-specific effects do not
                // execute in this renderer.
                crate::material_binding::apply_editor_background(
                    &state,
                    &bytes,
                    images,
                    &mut material,
                )?;
                EnvironmentMaterialMode::AdaptedEditorBackground
            } else if state.renderer_id == 0x80000033 {
                crate::material_binding::apply_editor_panorama(
                    &state,
                    &bytes,
                    images,
                    &mut material,
                )?;
                EnvironmentMaterialMode::AdaptedEditorPanorama
            } else if matches!(state.renderer_id, 0x80000002 | 0x80000004 | 0x80000005) {
                EnvironmentMaterialMode::NativeStaticPreview
            } else {
                EnvironmentMaterialMode::GenericFallback
            }
        } else {
            EnvironmentMaterialMode::GenericFallback
        };
        // Never mutate the shared OriginalModel material cache. Each preview
        // surface gets its own instance so unlit/background adaptation cannot
        // leak into a live object that uses the same imported source.
        let material = materials.add(material);
        surface.material = material.clone();
        cache.materials.insert(cache_key, (material, mode));
        modes.push(mode);
    }
    Ok(modes)
}

fn keys(kind: CreatorKind) -> [(ResourceKey, MechanicalEnvironmentRole); 2] {
    match kind {
        CreatorKind::Vehicle => [
            (VEHICLE_BASE, MechanicalEnvironmentRole::Base),
            (VEHICLE_BACKGROUND, MechanicalEnvironmentRole::Background),
        ],
        CreatorKind::Building => [
            (BUILDING_BASE, MechanicalEnvironmentRole::Base),
            (BUILDING_BACKGROUND, MechanicalEnvironmentRole::Background),
        ],
        CreatorKind::Ship => [
            (SHIP_BASE, MechanicalEnvironmentRole::Base),
            (SHIP_BACKGROUND, MechanicalEnvironmentRole::Background),
        ],
    }
}

fn imported_bounds(
    model: &crate::OriginalModel,
    meshes: &Assets<Mesh>,
) -> Result<(Vec3, Vec3), String> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;
    for surface in &model.surfaces {
        let mesh = meshes
            .get(&surface.handle)
            .ok_or("mechanical environment mesh missing")?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("mechanical environment positions missing".into());
        };
        for position in positions {
            let point = Vec3::from_array(*position);
            if !point.is_finite() {
                return Err("mechanical environment bounds are non-finite".into());
            }
            min = min.min(point);
            max = max.max(point);
            found = true;
        }
    }
    if !found || !(max - min).is_finite() {
        return Err("mechanical environment has no measurable geometry".into());
    }
    Ok((min, max))
}

fn fit_transform(
    min: Vec3,
    max: Vec3,
    subject: SubjectFrame,
) -> Result<(Transform, f32, f32), String> {
    let span = max - min;
    let width = span.x.max(span.z);
    if !width.is_finite()
        || width <= 0.
        || !subject.center.is_finite()
        || !subject.extent.is_finite()
        || subject.extent <= 0.
        || !subject.floor_y.is_finite()
    {
        return Err("invalid mechanical environment or subject frame".into());
    }
    let target_width = subject.extent * 2.5;
    let scale = target_width / width;
    let center = (min + max) * 0.5;
    let translation = Vec3::new(
        subject.center.x - center.x * scale,
        subject.floor_y - max.y * scale,
        subject.center.z - center.z * scale,
    );
    let transform = Transform::from_translation(translation).with_scale(Vec3::splat(scale));
    Ok((transform, scale, target_width))
}

/// Import and spawn the original base and backdrop for the selected creator.
/// All native records are validated before entities are created, so an
/// incomplete set reports an error rather than leaving a half-studio visible.
/// Imported source geometry/material handles are cached by `Originals` and
/// shared with their provenance preserved on every surface entity.
pub fn spawn(
    commands: &mut Commands,
    originals: &mut Originals,
    cache: &mut MechanicalEnvironmentCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    kind: CreatorKind,
    subject: SubjectFrame,
) -> Result<EnvironmentFit, String> {
    let mut set = Vec::new();
    for (key, role) in keys(kind) {
        let name = match role {
            MechanicalEnvironmentRole::Base => "native mechanical creator base",
            MechanicalEnvironmentRole::Background => "native mechanical creator background",
        };
        let definition = world_objects::definition_at(originals, key, name)?;
        let (mut model, source) =
            world_objects::import_at(originals, definition.clone(), meshes, images, materials)?;
        if model.surfaces.is_empty() {
            return Err(format!("{name} has no native draw surfaces"));
        }
        let material_modes =
            isolate_materials(originals, &source, &mut model, cache, images, materials)?;
        set.push(ImportedSet {
            definition,
            source,
            model,
            role,
            material_modes,
        });
    }

    // Measure both native models (reject missing/non-finite geometry), then
    // fit once from the base and reuse that exact authored-space transform for
    // the backdrop. Independent centering or depth offsets break the source
    // composition between the floor and its scenery.
    let mut base_bounds = None;
    for item in &set {
        let bounds = imported_bounds(&item.model, meshes)?;
        if item.role == MechanicalEnvironmentRole::Base {
            base_bounds = Some(bounds);
        }
    }
    let (base_min, base_max) = base_bounds.ok_or("native mechanical creator base missing")?;
    let (transform, scale, base_width) = fit_transform(base_min, base_max, subject)?;
    let base_floor_y = transform.translation.y + base_min.y * scale;
    let base_top_y = transform.translation.y + base_max.y * scale;

    for item in &set {
        for (surface, material_mode) in item.model.surfaces.iter().zip(&item.material_modes) {
            commands.spawn((
                MechanicalEnvironmentOwned,
                MechanicalEnvironmentSurface {
                    definition: item.definition.key,
                    model: item.source.model,
                    source_section: surface.source_section,
                    role: item.role,
                    material_mode: *material_mode,
                },
                item.source.clone(),
                OriginalSurfaceChild {
                    source_section: surface.source_section,
                    state_sections: surface.state_sections.clone(),
                },
                Name::new(format!(
                    "native mechanical creator {:?} surface {}",
                    item.role, surface.source_section
                )),
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(surface.material.clone()),
                transform,
                RenderLayers::layer(MECHANICAL_PREVIEW_LAYER),
                Visibility::Inherited,
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        }
    }

    Ok(EnvironmentFit {
        center: subject.center,
        base_floor_y,
        base_top_y,
        base_scale: scale,
        base_width,
        background_scale: scale,
    })
}

/// Despawn only entities owned by this module. Root-owned preview subject and
/// camera entities are deliberately unaffected.
pub fn cleanup(
    commands: &mut Commands,
    existing: &Query<Entity, With<MechanicalEnvironmentOwned>>,
) {
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }
}

/// Lifecycle system for roots that prefer a single registration. Optional
/// resources make this a safe no-op in headless/unit Apps without installed
/// originals; in a game it retries a failed import when resources become
/// available, while warning only once per unchanged failure message.
pub fn sync(
    mut commands: Commands,
    creator: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    mut originals: Option<ResMut<Originals>>,
    mut cache: Option<ResMut<MechanicalEnvironmentCache>>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    existing: Query<Entity, With<MechanicalEnvironmentOwned>>,
    subject: Option<Res<MechanicalEnvironmentSubject>>,
    mut active: Local<Option<(CreatorKind, SubjectFrame)>>,
    mut last_error: Local<Option<(CreatorKind, String)>>,
) {
    let Some(creator) = creator else {
        cleanup(&mut commands, &existing);
        *active = None;
        return;
    };
    if !creator.open {
        cleanup(&mut commands, &existing);
        *active = None;
        // An import failure is latched only for this opening. Reopening may
        // recover from a transient read error without a per-frame retry loop.
        *last_error = None;
        return;
    }
    let kind = creator.kind;
    let subject = subject.map_or_else(|| MechanicalEnvironmentSubject::default().0, |s| s.0);
    if last_error
        .as_ref()
        .is_some_and(|(failed_kind, _)| *failed_kind != kind)
    {
        *last_error = None;
    }
    if last_error
        .as_ref()
        .is_some_and(|(failed_kind, _)| *failed_kind == kind)
    {
        return;
    }
    if *active == Some((kind, subject)) && !existing.is_empty() {
        return;
    }
    if !existing.is_empty() {
        cleanup(&mut commands, &existing);
    }
    let (
        Some(mut originals),
        Some(mut cache),
        Some(mut meshes),
        Some(mut images),
        Some(mut materials),
    ) = (originals, cache, meshes, images, materials)
    else {
        return;
    };
    match spawn(
        &mut commands,
        &mut originals,
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        kind,
        subject,
    ) {
        Ok(_) => {
            *active = Some((kind, subject));
            *last_error = None;
        }
        Err(error) => {
            if last_error.as_ref() != Some(&(kind, error.clone())) {
                warn!("original mechanical creator environment: {error}");
                *last_error = Some((kind, error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;

    fn frame() -> SubjectFrame {
        SubjectFrame {
            center: Vec3::new(10.0, 0.0, 20.0),
            extent: 4.0,
            floor_y: -2.0,
        }
    }

    #[test]
    fn base_fit_centers_offset_bounds_scales_uniformly_and_aligns_native_top_to_subject_floor() {
        let min = Vec3::new(-2.0, -4.0, -1.0);
        let max = Vec3::new(6.0, 8.0, 3.0);
        let (transform, scale, width) = fit_transform(min, max, frame()).unwrap();

        assert!((scale - 1.25).abs() < 1e-6);
        assert!((width - 10.0).abs() < 1e-6);
        assert_eq!(transform.scale, Vec3::splat(scale));
        let fitted_min = transform.transform_point(min);
        let fitted_max = transform.transform_point(max);
        assert!((fitted_max.y - frame().floor_y).abs() < 1e-6);
        assert!(((fitted_min.x + fitted_max.x) * 0.5 - frame().center.x).abs() < 1e-6);
        assert!(((fitted_min.z + fitted_max.z) * 0.5 - frame().center.z).abs() < 1e-6);
    }

    #[test]
    fn backdrop_reuses_base_transform_and_preserves_authored_relative_composition() {
        let base_min = Vec3::new(-2.0, -4.0, -1.0);
        let base_max = Vec3::new(6.0, 8.0, 3.0);
        let backdrop_min = Vec3::new(-8.0, -2.0, 5.0);
        let backdrop_max = Vec3::new(4.0, 10.0, 13.0);
        let (transform, scale, width) = fit_transform(base_min, base_max, frame()).unwrap();

        assert!((scale - 1.25).abs() < 1e-6);
        assert!((width - 10.0).abs() < 1e-6);
        assert_eq!(transform.scale, Vec3::splat(scale));
        let base_top = transform.transform_point(base_max).y;
        assert!((base_top - frame().floor_y).abs() < 1e-6);

        // spawn applies this one transform to both models. Their independent
        // source-space offset and scale remain unchanged by environment fit.
        let authored_delta = (backdrop_min + backdrop_max) * 0.5 - (base_min + base_max) * 0.5;
        let rendered_delta = transform.transform_point((backdrop_min + backdrop_max) * 0.5)
            - transform.transform_point((base_min + base_max) * 0.5);
        assert!((rendered_delta - authored_delta * scale).length() < 1e-5);
    }

    #[test]
    fn fit_rejects_empty_footprints_and_nonfinite_subject_frames() {
        let valid = frame();
        assert!(fit_transform(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), valid).is_err());

        let invalid = SubjectFrame {
            extent: f32::NAN,
            ..valid
        };
        assert!(fit_transform(Vec3::ZERO, Vec3::ONE, invalid).is_err());
    }

    #[test]
    fn imported_bounds_reject_nonfinite_native_vertices() {
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [1.0, f32::NAN, 0.0], [0.0, 0.0, 1.0]],
        );
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(mesh);
        let model = crate::OriginalModel {
            source_definition: None,
            material: None,
            handle: handle.clone(),
            surfaces: vec![crate::OriginalSurface {
                handle,
                material: Handle::default(),
                source_section: 0,
                state_sections: vec![],
            }],
            scale: 1.0,
            instance: 0,
            root: None,
            rig_bones: 0,
        };

        assert!(imported_bounds(&model, &meshes).is_err());
    }

    #[test]
    fn lifecycle_is_a_noop_in_a_headless_app_without_creator_or_originals() {
        let mut app = App::new();
        app.add_systems(Update, sync);
        app.update();
        let world = app.world_mut();
        let mut entities = world.query_filtered::<Entity, With<MechanicalEnvironmentOwned>>();
        assert!(entities.iter(world).next().is_none());
    }
}
