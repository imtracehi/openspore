//! Palette part icons: the original pre-rendered icon when the editor's
//! thumbnail group has one, otherwise a small render of the exact original
//! part model.
//!
//! The editor UI adds [`ThumbnailRequest`] to its image node. Register
//! [`update_requests`] in `Update` and initialize [`PartThumbnailCache`].
//! Definitions with an installed palette icon (see
//! [`crate::editor_art::part_icon_key`]) bind that PNG directly. Others get
//! one offscreen camera scene each, bounded by [`MAX_LIVE_SCENES`] (one full
//! palette page). A scene renders every frame while it warms up and is then
//! refreshed round-robin, so a static page costs about one tiny render a
//! frame. Images are cached by the full PROP key and inactive scenes are
//! removed as soon as their UI request disappears.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::{Asset, AssetId},
    camera::{visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::render_resource::TextureFormat,
};

use crate::{asset_catalog::PartDefinition, dbpf::ResourceKey, editor, Originals};

const THUMBNAIL_LAYER: usize = 31;
const THUMBNAIL_SIZE: u32 = 128;
/// Retained icons and render targets, including inactive pages.
pub const MAX_CACHED_IMAGES: usize = 32;
/// Live model previews; one full creator palette page.
pub const MAX_LIVE_SCENES: usize = 18;
const SCENE_SPACING: f32 = 12.;
/// Frames a new preview renders continuously, covering asynchronous
/// pipeline and texture preparation, before it is refreshed round-robin.
const WARMUP_FRAMES: u32 = 45;
/// After warm-up each preview re-renders once per this many frames.
const REFRESH_FRAMES: u64 = 24;

/// Marks the image node that receives the rendered model for one exact part
/// definition. The full PROP key distinguishes definitions that share RW4s.
#[derive(Component, Debug, Clone, Copy)]
pub struct ThumbnailRequest {
    pub definition: ResourceKey,
}

/// Provenance marker on each original mesh surface rendered into a thumbnail.
#[derive(Component, Debug, Clone, Copy)]
pub struct ThumbnailGeometry {
    pub definition: ResourceKey,
    pub model: ResourceKey,
    pub surface: usize,
}

/// Identifies a dedicated, layer-isolated render camera.
#[derive(Component, Debug, Clone, Copy)]
pub struct ThumbnailCamera {
    pub definition: ResourceKey,
}

/// Identifies the shadowless studio light used only by thumbnail cameras.
#[derive(Component, Debug, Clone, Copy)]
pub struct ThumbnailStudioLight;

#[derive(Component, Debug, Clone, Copy)]
pub struct ThumbnailScene {
    pub definition: ResourceKey,
}

#[derive(Default)]
struct CachedThumbnail {
    image: Handle<Image>,
    scene: Option<Entity>,
    camera: Option<Entity>,
    scene_age: u32,
    model: Option<crate::OriginalModel>,
    owned_assets: OwnedAssets,
    /// Installed palette icon bound instead of a live model render.
    native_icon: Option<ResourceKey>,
    last_used: u64,
    failed: bool,
}

/// Provenance marker on a palette image node showing an original icon PNG.
#[derive(Component, Debug, Clone, Copy)]
pub struct NativePartIcon {
    pub definition: ResourceKey,
    pub icon: ResourceKey,
}

#[derive(Default)]
struct OwnedAssets {
    images: Vec<AssetId<Image>>,
    meshes: Vec<AssetId<Mesh>>,
    materials: Vec<AssetId<StandardMaterial>>,
}

struct AssetSnapshot {
    images: HashSet<AssetId<Image>>,
    meshes: HashSet<AssetId<Mesh>>,
    materials: HashSet<AssetId<StandardMaterial>>,
}

impl AssetSnapshot {
    fn capture(
        images: &Assets<Image>,
        meshes: &Assets<Mesh>,
        materials: &Assets<StandardMaterial>,
    ) -> Self {
        Self {
            images: images.ids().collect(),
            meshes: meshes.ids().collect(),
            materials: materials.ids().collect(),
        }
    }

    fn added_assets(
        &self,
        images: &Assets<Image>,
        meshes: &Assets<Mesh>,
        materials: &Assets<StandardMaterial>,
    ) -> OwnedAssets {
        OwnedAssets {
            images: added_ids(&self.images, images),
            meshes: added_ids(&self.meshes, meshes),
            materials: added_ids(&self.materials, materials),
        }
    }
}

fn added_ids<A: Asset>(before: &HashSet<AssetId<A>>, assets: &Assets<A>) -> Vec<AssetId<A>> {
    assets.ids().filter(|id| !before.contains(id)).collect()
}

/// Bounded image cache. Live camera scenes are tracked separately on each
/// entry and exist only while a visible palette page requests the definition.
#[derive(Resource, Default)]
pub struct PartThumbnailCache {
    images: HashMap<ResourceKey, CachedThumbnail>,
    studio_light: Option<Entity>,
    clock: u64,
}

impl PartThumbnailCache {
    /// Number of retained render targets, exposed for runtime auditing.
    pub fn cached_images(&self) -> usize {
        self.images.len()
    }

    /// Number of active offscreen scenes, exposed for runtime auditing.
    pub fn live_scenes(&self) -> usize {
        self.images
            .values()
            .filter(|entry| entry.scene.is_some())
            .count()
    }

    /// Installed palette icon bound for a definition, if any.
    pub fn native_icon(&self, definition: ResourceKey) -> Option<ResourceKey> {
        self.images.get(&definition)?.native_icon
    }
}

/// Bind original palette icons and create, reuse, throttle and retire the
/// fallback model render scenes.
pub fn update_requests(
    mut commands: Commands,
    mut cache: ResMut<PartThumbnailCache>,
    mut requests: Query<(
        Entity,
        &ThumbnailRequest,
        &mut ImageNode,
        &mut Visibility,
        Option<&NativePartIcon>,
    )>,
    mut cameras: Query<&mut Camera, With<ThumbnailCamera>>,
    originals: Res<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    cache.clock = cache.clock.wrapping_add(1);

    let mut ordered_keys = Vec::new();
    let mut active = HashSet::new();
    for (_, request, _, _, _) in &requests {
        if active.len() < MAX_CACHED_IMAGES && active.insert(request.definition) {
            ordered_keys.push(request.definition);
        }
    }

    for (key, entry) in &mut cache.images {
        if !active.contains(key) {
            if let Some(scene) = entry.scene.take() {
                commands.entity(scene).despawn();
            }
            entry.camera = None;
        }
    }

    for (entity, request, mut image_node, mut visibility, marker) in &mut requests {
        if !active.contains(&request.definition) {
            if image_node.image != Handle::default() {
                image_node.image = Handle::default();
            }
            *visibility = Visibility::Hidden;
            continue;
        }
        if !cache.images.contains_key(&request.definition) {
            if !make_room(
                &mut cache,
                &mut images,
                &mut meshes,
                &mut materials,
                &active,
            ) {
                image_node.image = Handle::default();
                *visibility = Visibility::Hidden;
                continue;
            }
            // The installed palette icon is the authored thumbnail; a live
            // render is only the fallback for definitions without one.
            let native = crate::editor_art::part_icon_key(request.definition)
                .filter(|icon| originals.resource_index.records.contains_key(icon))
                .and_then(|icon| {
                    match crate::ui_art::import_png(&originals.resource_index, icon, &mut images) {
                        Ok((image, _, _)) => Some((icon, image)),
                        Err(error) => {
                            warn!("palette icon {icon:?}: {error}");
                            None
                        }
                    }
                });
            let (native_icon, image) = match native {
                Some((icon, image)) => (Some(icon), image),
                None => (
                    None,
                    images.add(Image::new_target_texture(
                        THUMBNAIL_SIZE,
                        THUMBNAIL_SIZE,
                        TextureFormat::Rgba8UnormSrgb,
                        None,
                    )),
                ),
            };
            let last_used = cache.clock;
            cache.images.insert(
                request.definition,
                CachedThumbnail {
                    image,
                    native_icon,
                    model: None,
                    owned_assets: OwnedAssets::default(),
                    last_used,
                    ..default()
                },
            );
        }
        let last_used = cache.clock;
        let entry = cache.images.get_mut(&request.definition).unwrap();
        entry.last_used = last_used;
        if entry.failed {
            image_node.image = Handle::default();
            *visibility = Visibility::Hidden;
        } else {
            if image_node.image != entry.image {
                image_node.image = entry.image.clone();
            }
            *visibility = Visibility::Inherited;
        }
        match (entry.native_icon, marker) {
            (Some(icon), None) => {
                commands.entity(entity).insert(NativePartIcon {
                    definition: request.definition,
                    icon,
                });
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<NativePartIcon>();
            }
            _ => {}
        }
    }

    // Live fallback scenes, bounded to one palette page.
    let live_keys: Vec<ResourceKey> = ordered_keys
        .into_iter()
        .filter(|key| {
            cache
                .images
                .get(key)
                .is_some_and(|entry| entry.native_icon.is_none() && !entry.failed)
        })
        .take(MAX_LIVE_SCENES)
        .collect();
    let live: HashSet<ResourceKey> = live_keys.iter().copied().collect();
    for (key, entry) in &mut cache.images {
        if !live.contains(key) {
            if let Some(scene) = entry.scene.take() {
                commands.entity(scene).despawn();
            }
            entry.camera = None;
        }
    }
    if live.is_empty() {
        if let Some(light) = cache.studio_light.take() {
            commands.entity(light).despawn();
        }
    } else if cache.studio_light.is_none() {
        cache.studio_light = Some(spawn_studio_light(&mut commands));
    }

    // Static previews only need a few frames; afterwards one camera renders
    // per frame in turn, which also repairs any frame drawn before its
    // pipeline was ready.
    let clock = cache.clock;
    for (slot, key) in live_keys.iter().enumerate() {
        let Some(entry) = cache.images.get_mut(key) else {
            continue;
        };
        let Some(camera) = entry.camera else {
            continue;
        };
        entry.scene_age = entry.scene_age.saturating_add(1);
        let render = renders_this_frame(entry.scene_age, clock, slot, live_keys.len());
        if let Ok(mut camera) = cameras.get_mut(camera) {
            if camera.is_active != render {
                camera.is_active = render;
            }
        }
    }

    for (slot, key) in live_keys.into_iter().enumerate() {
        let needs_scene = cache
            .images
            .get(&key)
            .is_some_and(|entry| entry.scene.is_none() && !entry.failed);
        if !needs_scene {
            continue;
        }
        let Some(definition) = originals
            .part_catalog
            .parts
            .iter()
            .find(|part| part.key == key)
        else {
            if let Some(entry) = cache.images.get_mut(&key) {
                entry.failed = true;
            }
            warn!("thumbnail definition {key:?} is absent from the part catalog");
            continue;
        };
        if cache.images.get(&key).unwrap().model.is_none() {
            let snapshot = AssetSnapshot::capture(&images, &meshes, &materials);
            match editor::import_definition(
                &originals,
                definition,
                &mut meshes,
                &mut images,
                &mut materials,
            ) {
                Ok(model) => {
                    let owned_assets = snapshot.added_assets(&images, &meshes, &materials);
                    let entry = cache.images.get_mut(&key).unwrap();
                    entry.model = Some(model);
                    entry.owned_assets = owned_assets;
                }
                Err(error) => {
                    let partial = snapshot.added_assets(&images, &meshes, &materials);
                    release_assets(partial, &mut images, &mut meshes, &mut materials);
                    cache.images.get_mut(&key).unwrap().failed = true;
                    warn!(
                        "could not render thumbnail for {}: {error}",
                        definition.name
                    );
                    continue;
                }
            }
        }

        let model = cache.images.get(&key).unwrap().model.as_ref().unwrap();
        let Some((center, extent)) = mesh_bounds(&meshes, &model.handle) else {
            cache.images.get_mut(&key).unwrap().failed = true;
            warn!(
                "thumbnail model {} has no finite vertex bounds",
                definition.name
            );
            continue;
        };
        if model.surfaces.is_empty() {
            cache.images.get_mut(&key).unwrap().failed = true;
            warn!(
                "thumbnail model {} has no renderable surfaces",
                definition.name
            );
            continue;
        }
        let image = cache.images.get(&key).unwrap().image.clone();
        let (scene, camera) = spawn_scene(
            &mut commands,
            key,
            definition,
            center,
            extent,
            slot,
            image,
            &model.surfaces,
        );
        let entry = cache.images.get_mut(&key).unwrap();
        entry.scene = Some(scene);
        entry.camera = Some(camera);
        entry.scene_age = 0;
    }

    // Import failures are discovered after the first UI pass above. Hide them
    // immediately rather than leaving a plausible-looking blank tile.
    for (_, request, mut image_node, mut visibility, _) in &mut requests {
        if cache
            .images
            .get(&request.definition)
            .is_some_and(|entry| entry.failed)
        {
            image_node.image = Handle::default();
            *visibility = Visibility::Hidden;
        }
    }
}

/// Whether a live preview camera draws this frame: always while warming up,
/// then once per refresh period, staggered by its slot.
fn renders_this_frame(age: u32, clock: u64, slot: usize, live: usize) -> bool {
    age <= WARMUP_FRAMES || (clock + slot as u64) % REFRESH_FRAMES.max(live as u64) == 0
}

fn make_room(
    cache: &mut PartThumbnailCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    active: &HashSet<ResourceKey>,
) -> bool {
    while cache.images.len() >= MAX_CACHED_IMAGES {
        let oldest = cache
            .images
            .iter()
            .filter(|(key, entry)| !active.contains(key) && entry.scene.is_none())
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| *key);
        let Some(key) = oldest else {
            return false;
        };
        if let Some(entry) = cache.images.remove(&key) {
            release_entry(entry, images, meshes, materials);
        }
    }
    true
}

fn release_entry(
    entry: CachedThumbnail,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    debug_assert!(
        entry.scene.is_none(),
        "live thumbnail scenes must be retired first"
    );
    release_assets(entry.owned_assets, images, meshes, materials);
    images.remove(entry.image.id());
}

fn release_assets(
    owned: OwnedAssets,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for id in owned.materials {
        materials.remove(id);
    }
    for id in owned.meshes {
        meshes.remove(id);
    }
    for id in owned.images {
        images.remove(id);
    }
}

fn mesh_bounds(meshes: &Assets<Mesh>, handle: &Handle<Mesh>) -> Option<(Vec3, f32)> {
    let mesh = meshes.get(handle)?;
    let bevy::mesh::VertexAttributeValues::Float32x3(positions) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let (min, max) = positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), position| {
            let position = Vec3::from_array(*position);
            (min.min(position), max.max(position))
        },
    );
    let extent = (max - min).max_element();
    (extent.is_finite() && extent > 0.).then_some(((min + max) * 0.5, extent))
}

fn spawn_studio_light(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            DirectionalLight {
                color: Color::srgb(1., 0.91, 0.78),
                illuminance: 5200.,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(-1.8, 2.3, 3.).looking_at(Vec3::ZERO, Vec3::Y),
            Visibility::Inherited,
            RenderLayers::layer(THUMBNAIL_LAYER),
            ThumbnailStudioLight,
        ))
        .id()
}

fn spawn_scene(
    commands: &mut Commands,
    key: ResourceKey,
    definition: &PartDefinition,
    center: Vec3,
    extent: f32,
    slot: usize,
    target: Handle<Image>,
    surfaces: &[crate::OriginalSurface],
) -> (Entity, Entity) {
    let layer = RenderLayers::layer(THUMBNAIL_LAYER);
    let root = commands
        .spawn((
            ThumbnailScene { definition: key },
            Transform::from_xyz(slot as f32 * SCENE_SPACING, 0., 0.),
            Visibility::Inherited,
        ))
        .id();
    let fit = (1.55 / extent.max(0.001)).clamp(0.04, 20.);
    let rotation = Quat::from_euler(EulerRot::YXZ, -0.28, 0.12, 0.);
    let model_transform = Transform {
        translation: -(rotation * center) * fit,
        rotation,
        scale: Vec3::splat(fit),
    };
    let mut camera = Entity::PLACEHOLDER;
    commands.entity(root).with_children(|parent| {
        // A transparent clear lets the fallback render sit on the palette
        // like the installed icons, whose backgrounds are transparent.
        camera = parent
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -1,
                    clear_color: Color::NONE.into(),
                    ..default()
                },
                RenderTarget::from(target),
                Projection::Perspective(PerspectiveProjection {
                    fov: 38_f32.to_radians(),
                    near: 0.05,
                    far: 5.5,
                    ..default()
                }),
                Transform::from_xyz(0., 0., 3.4).looking_at(Vec3::ZERO, Vec3::Y),
                layer.clone(),
                AmbientLight {
                    color: Color::srgb(0.82, 0.86, 1.),
                    brightness: 180.,
                    ..default()
                },
                ThumbnailCamera { definition: key },
            ))
            .id();
        for (surface_index, surface) in surfaces.iter().enumerate() {
            parent.spawn((
                // A drawn surface, not a model root: `material_binding::attach_remaining`
                // would otherwise spawn the model's other surfaces again under the
                // first one with no render layer, drawn in the world by the main camera.
                crate::material_binding::OriginalSurfaceChild::of(surface),
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(surface.material.clone()),
                model_transform,
                layer.clone(),
                ThumbnailGeometry {
                    definition: key,
                    model: definition.model,
                    surface: surface_index,
                },
            ));
        }
    });
    (root, camera)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;

    #[test]
    fn thumbnail_layer_isolated_from_main_camera_layer() {
        let thumbnail = RenderLayers::layer(THUMBNAIL_LAYER);
        let main = RenderLayers::layer(0);
        assert!(!thumbnail.intersects(&main));
        assert!(thumbnail.intersects(&RenderLayers::layer(THUMBNAIL_LAYER)));
    }

    #[test]
    fn model_bounds_center_and_fit_actual_vertex_positions() {
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-1., -2., 0.], [3., 2., 0.], [1., 0., 1.]],
        );
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(mesh);
        let (center, extent) = mesh_bounds(&meshes, &handle).unwrap();
        assert_eq!(center, Vec3::new(1., 0., 0.5));
        assert_eq!(extent, 4.);
    }

    /// A model's surfaces on the thumbnail layer must not be copied into the
    /// world by `material_binding::attach_remaining` (it took the first surface
    /// for a whole model and spawned the others again with no render layer, so
    /// the main camera drew them where the thumbnail scene stands).
    #[test]
    fn a_thumbnail_scene_adds_no_surface_copies_outside_its_layer() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Originals>()
            .add_systems(Update, crate::material_binding::attach_remaining);
        let mut surfaces = vec![];
        for (section, size) in [(4usize, 1.0f32), (8, 2.0)] {
            let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::new(size, 1.0, 1.0));
            let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
            surfaces.push(crate::OriginalSurface { handle, material, source_section: section, state_sections: vec![section + 1] });
        }
        let model = crate::OriginalModel { handle: surfaces[0].handle.clone(), surfaces: surfaces.clone(), ..default() };
        let key = ResourceKey { type_id: 1, group_id: 2, instance_id: 3 };
        app.world_mut().resource_mut::<Originals>().world_models.insert(key, model);
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                let definition = PartDefinition::raw_model(key);
                spawn_scene(&mut commands, key, &definition, Vec3::ZERO, 1., 0, Handle::default(), &surfaces);
            })
            .unwrap();
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        assert_eq!(world.query::<&Mesh3d>().iter(world).count(), 2, "one entity per surface, no copies");
        let unlayered = world.query_filtered::<(), (With<Mesh3d>, Without<RenderLayers>)>().iter(world).count();
        assert_eq!(unlayered, 0, "every thumbnail mesh is on the thumbnail layer");
    }

    #[test]
    fn evicting_a_definition_releases_its_target_model_and_texture_assets() {
        let mut images = Assets::<Image>::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let target = images.add(Image::default());
        let snapshot = AssetSnapshot::capture(&images, &meshes, &materials);
        let texture = images.add(Image::default());
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        );
        let mesh = meshes.add(mesh);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(texture.clone()),
            ..default()
        });
        let definition = ResourceKey {
            type_id: 1,
            group_id: 2,
            instance_id: 3,
        };
        let model = crate::OriginalModel {
            source_definition: Some(definition),
            material: Some(material.clone()),
            handle: mesh.clone(),
            surfaces: vec![crate::OriginalSurface {
                handle: mesh.clone(),
                material: material.clone(),
                source_section: 0,
                state_sections: Vec::new(),
            }],
            scale: 1.,
            instance: 3,
            root: None,
            rig_bones: 0,
        };
        let entry = CachedThumbnail {
            image: target.clone(),
            scene: None,
            model: Some(model),
            owned_assets: snapshot.added_assets(&images, &meshes, &materials),
            last_used: 0,
            failed: false,
            ..default()
        };

        release_entry(entry, &mut images, &mut meshes, &mut materials);

        assert!(images.get(target.id()).is_none());
        assert!(images.get(texture.id()).is_none());
        assert!(meshes.get(mesh.id()).is_none());
        assert!(materials.get(material.id()).is_none());
    }

    #[test]
    fn settled_previews_refresh_one_at_a_time() {
        let live = MAX_LIVE_SCENES;
        assert!((0..live).all(|slot| renders_this_frame(1, 7, slot, live)));
        assert!((0..live).all(|slot| renders_this_frame(WARMUP_FRAMES, 7, slot, live)));
        let period = REFRESH_FRAMES.max(live as u64);
        for clock in 1000..1000 + period {
            let drawn = (0..live)
                .filter(|&slot| renders_this_frame(WARMUP_FRAMES + 1, clock, slot, live))
                .count();
            assert!(drawn <= 1, "settled previews share the frame budget");
        }
        for slot in 0..live {
            assert!(
                (1000..1000 + period).any(|clock| renders_this_frame(u32::MAX, clock, slot, live)),
                "every settled preview is still refreshed"
            );
        }
    }

    #[test]
    fn page_sized_preview_budget_stays_inside_the_cache() {
        assert!(MAX_LIVE_SCENES <= MAX_CACHED_IMAGES);
        assert!(MAX_LIVE_SCENES >= 15, "one full palette page renders live");
    }

    #[test]
    fn cache_room_evicts_the_least_recent_inactive_entry() {
        let mut cache = PartThumbnailCache::default();
        let mut images = Assets::<Image>::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut keys = Vec::new();
        for instance_id in 0..MAX_CACHED_IMAGES as u64 {
            let key = ResourceKey {
                type_id: 1,
                group_id: 2,
                instance_id: instance_id as u32,
            };
            keys.push(key);
            cache.images.insert(
                key,
                CachedThumbnail {
                    image: images.add(Image::default()),
                    last_used: instance_id,
                    ..default()
                },
            );
        }
        let oldest_target = cache.images[&keys[0]].image.id();
        let newest_target = cache.images[&keys[keys.len() - 1]].image.id();

        assert!(make_room(
            &mut cache,
            &mut images,
            &mut meshes,
            &mut materials,
            &HashSet::new(),
        ));

        assert_eq!(cache.cached_images(), MAX_CACHED_IMAGES - 1);
        assert!(!cache.images.contains_key(&keys[0]));
        assert!(cache.images.contains_key(keys.last().unwrap()));
        assert!(images.get(oldest_target).is_none());
        assert!(images.get(newest_target).is_some());
    }
}
