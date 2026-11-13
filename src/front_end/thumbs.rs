//! Offscreen thumbnails of creations for the Sporepedia cards: the player's
//! creatures (skin, paint and parts), their buildings, vehicles and UFOs, and
//! the installed Maxis creations.
//!
//! The approach of `part_thumbnails`: one small scene on its own render layer
//! with a camera rendering into an image, far from the stage. One creation is
//! rendered at a time; after a short warm-up the image is read back
//! (`Screenshot::image`), written as a PNG to `<save stem>-thumbs\` beside the
//! saves (next to the planet impostors) and registered with the SPUI painter.
//! A cached PNG is used without rendering. The installed Maxis creations show
//! their own PNG (`maxis`), as retail Sporepedia shows each creation file's
//! PNG. ADAPTED: the player's creations are rendered (OpenSpore writes no
//! creation PNGs; retail renders them when a creation is saved) and the studio
//! light is OpenSpore's. The camera is the editors' thumbnail camera: heading,
//! pitch and FOV of `EditorCameraCreatureThumbnail` / `...BuildingThumbnail` /
//! `...VehicleThumbnail` (`camera_properties~` 0x40410100, the setups'
//! `editorCameraThumbnail`; UFOs inherit the vehicle one from `VehicleTemplate`),
//! the model fitted into the view as the editor framing rule does
//! (`FUN_005A33B0`: boxes inside the FOV frustum; ADAPTED: the model's own
//! vertices instead of its blocks' boxes) and scaled by the setups'
//! `ThumbnailImageScale` (0.85, `GlobalTemplate`).
//!
//! The thumbnail effect (VERIFIED, `0x580700` reads `ThumbnailShadowSkew`,
//! `ThumbnailShadowWidening`, `ThumbnailLowering` and `ThumbnailImageScale` of
//! the setup into the request it hands to the thumbnail manager, vtable +0x20
//! 0x7BEAD0): the manager renders the model, measures the bounding box of the
//! pixels the render covers and composes the final image from it (0x7B6250 ..
//! 0x7B6426): the box's larger side is scaled to `ThumbnailImageScale` of the
//! frame and the box is centred; when the box is wider than tall the image is
//! moved down by `ThumbnailLowering` of the free vertical space on top of the
//! centring. `compose` does that on the CPU from a render of twice the size.
//! Values (setups `40600100`; `ThumbnailEffect` 2 with the skew, widening and
//! image scale is the template `578C04FA` the setups inherit from): lowering
//! creature 0.5, building 0.4, land vehicle 0.4 (the template says 0.65, the
//! cell setup 0.3 with `ThumbnailEffect` 1 and no shadow), image scale 0.85,
//! skew (-0.4, -0.5), widening 0.1.
//!
//! MEASURED against the retail output (the installed Maxis creations' PNGs,
//! 128 x 128, group `4062 / 4063 / 4064 / 4065 6200`; 40 creature, building and
//! vehicle PNGs): the model's box measures 0.85 of the frame on its larger side and
//! wide models sit lower by `lowering x free space / 2` (creature 6.5 px of 7.3
//! predicted, building 7 / 9 and 6 / 6.8), and every model has a soft
//! translucent grey ground shadow to its lower left: an upright copy of the
//! silhouette squashed to 0.7 of its height about the model's base, leaning left
//! by 0.2 px per px of height and 4 / 128 of the frame higher, with an alpha of
//! 0.15 (a grid fit of 40 PNGs: intersection over union 0.67). The pass that
//! draws it (0x7BA420 gets the skew and widening) was not traced, so
//! `shadow_alpha` draws the fitted shape: ADAPTED, from the PNGs, not from the
//! skew values (they give a different shape in a 2D reading and the widening
//! does not improve the fit).

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};

use crate::{dbpf::ResourceKey, game::RunProgress, stages::GameStage, Originals};

/// Thumbnail size (pixels).
pub const SIZE: u32 = 192;
/// Render layer of the thumbnail scenes (31 part thumbnails, 30 previews,
/// 29/28 the cell editor, 27..25 portraits).
const LAYER: usize = 24;
/// Frames a scene renders before it is read back (pipelines, textures).
const WARMUP: u32 = 24;
/// A capture that does not arrive is given up after this many frames.
const TIMEOUT: u32 = 400;
const IMAGE_GROUP: u32 = 0x0F2E_1B00;
pub const PNG_TYPE: u32 = 0x2F7D_0004;
/// Where the scenes stand (far below every stage).
const SCENE_AT: Vec3 = Vec3::new(0.0, -20_000.0, 0.0);

/// An editor thumbnail camera: heading and pitch in degrees (the editor
/// camera's `R = RotZ(-heading) RotX(pitch)`, pitch negative = looking down)
/// and the FOV in radians (`cameraInitialFOV`; the thumbnails are square, so
/// the editor viewer's 0.75 factor does not apply, `FUN_007C5400`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbCamera {
    pub heading: f32,
    pub pitch: f32,
    pub fov: f32,
}
/// `EditorCameraCreatureThumbnail` (0x40410100!B0D43C20; zoom 0.05, near 0.02, far 200).
pub const CREATURE_CAMERA: ThumbCamera = ThumbCamera { heading: -50.0, pitch: -15.0, fov: 0.2 };
/// `EditorCameraBuildingThumbnail` (4AA0E609; zoom 1, near 0.2, far 5000).
pub const BUILDING_CAMERA: ThumbCamera = ThumbCamera { heading: -57.0, pitch: -15.0, fov: 0.5726 };
/// `EditorCameraVehicleThumbnail` (B0905693; zoom 1, near 0.02, far 200).
pub const VEHICLE_CAMERA: ThumbCamera = ThumbCamera { heading: -66.0, pitch: -15.0, fov: 0.5726 };
/// `ThumbnailImageScale` of `GlobalTemplate` (0x05E30434): the model fills this
/// fraction of the frame.
pub const IMAGE_SCALE: f32 = 0.85;

/// The thumbnail camera of a source.
pub(super) fn camera_for(source: &Source) -> ThumbCamera {
    use crate::creation_records::{BLD, UFO, VCL};
    let setup = |name: &str| crate::creature_editor_data::SetupKind::ALL.into_iter().find(|k| k.setup_name() == name);
    let rigid = |name: &str| match setup(name) {
        Some(kind) if kind.is_building() => BUILDING_CAMERA,
        _ => VEHICLE_CAMERA,
    };
    match source {
        Source::Design(_, name) => rigid(name),
        Source::Installed(key) => match key.type_id {
            BLD => BUILDING_CAMERA,
            VCL | UFO => VEHICLE_CAMERA,
            _ => CREATURE_CAMERA,
        },
        Source::Stored(c) => match &c.kind {
            super::store::StoredKind::Design { setup, .. } => rigid(setup),
            _ => CREATURE_CAMERA,
        },
        Source::Creature(_) | Source::Png(_) => CREATURE_CAMERA,
    }
}

/// Where the editor camera looks from, as a unit vector from the model's
/// centre: the creature's front turned by the heading about the vertical
/// (toward `-(front x up)`: heading -50 swings the eye 50 degrees from the
/// front, VERIFIED with `creature_editor::EditorRig::view`), raised by the
/// pitch.
pub fn view_direction(front: Vec3, camera: ThumbCamera) -> Vec3 {
    let (h, p) = ((-camera.heading).to_radians(), (-camera.pitch).to_radians());
    let side = front.cross(Vec3::Y).normalize_or(Vec3::X);
    ((front * h.cos() - side * h.sin()) * p.cos() + Vec3::Y * p.sin()).normalize()
}

/// `ThumbnailLowering` of the creature setups (`CF2DC8D0`).
pub const LOWERING_CREATURE: f32 = 0.5;
/// `ThumbnailLowering` of the building (`D817CD63`) and vehicle (`99F87089`) setups.
pub const LOWERING_RIGID: f32 = 0.4;
/// Size of the render the composition starts from (twice the thumbnail).
const RENDER: u32 = SIZE * 2;
/// The shadow fitted to the retail PNGs (see the header): vertical scale about
/// the base, lean per pixel of height, lift (fraction of the frame), alpha,
/// blur radius (fraction of the frame).
const SHADOW_SQUASH: f32 = 0.7;
const SHADOW_LEAN: f32 = -0.2;
const SHADOW_LIFT: f32 = -4.0 / 128.0;
const SHADOW_ALPHA: f32 = 0.16;
const SHADOW_BLUR: f32 = 2.0 / 128.0;

/// The setup's `ThumbnailLowering` for a camera (creatures use the long lens).
pub fn lowering_for(camera: ThumbCamera) -> f32 {
    if camera == CREATURE_CAMERA {
        LOWERING_CREATURE
    } else {
        LOWERING_RIGID
    }
}

/// The cache file of a thumbnail (version 4: the retail composition from a
/// double-size render, the lowering and the fitted shadow).
fn cache_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.v4.png"))
}

/// What a thumbnail shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The creature of a saved game (its save file).
    Creature(PathBuf),
    /// A rigid design of a saved game (save file, setup name).
    Design(PathBuf, String),
    /// An installed creation record.
    Installed(ResourceKey),
    /// An installed creation's own PNG (decoded, not rendered).
    Png(ResourceKey),
    /// A creation of the Sporepedia store.
    Stored(Box<super::store::StoredCreation>),
}

/// The SPUI image key of a thumbnail (by its cache name).
pub fn image_key(name: &str) -> ResourceKey {
    ResourceKey { group_id: IMAGE_GROUP, instance_id: crate::world_objects::name_id(name), type_id: PNG_TYPE }
}

/// Where the thumbnails of the saves beside `save` are cached.
pub fn cache_dir(save: &Path) -> PathBuf {
    let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
    save.with_file_name(format!("{stem}-thumbs"))
}

/// 64-bit FNV-1a of some bytes (cache names follow the content).
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

enum State {
    Idle,
    Rendering { name: String, frames: u32, root: Entity, target: Handle<Image>, asked: bool },
}

enum Entry {
    Queued,
    Ready(Handle<Image>),
    Failed,
}

/// The renderer: queue, the scene being rendered, finished thumbnails.
#[derive(Resource)]
pub struct Thumbnails {
    pub cache: Option<PathBuf>,
    queue: VecDeque<(String, Source)>,
    entries: HashMap<String, Entry>,
    state: State,
    inbox: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    count: u32,
    /// `ThumbnailLowering` of the thumbnail being rendered.
    lowering: f32,
}

impl Default for Thumbnails {
    fn default() -> Self {
        Self {
            cache: None,
            queue: VecDeque::new(),
            entries: HashMap::new(),
            state: State::Idle,
            inbox: Arc::new(Mutex::new(Vec::new())),
            count: 0,
            lowering: LOWERING_CREATURE,
        }
    }
}

impl Thumbnails {
    /// Ask for a thumbnail; `true` once it is ready.
    pub fn request(&mut self, name: &str, source: &Source) -> bool {
        match self.entries.get(name) {
            Some(Entry::Ready(_)) => true,
            Some(_) => false,
            None => {
                self.entries.insert(name.to_string(), Entry::Queued);
                self.queue.push_back((name.to_string(), source.clone()));
                false
            }
        }
    }

    /// The finished image (Rgba8Unorm, for the painter and UI image nodes).
    pub fn image(&self, name: &str) -> Option<Handle<Image>> {
        match self.entries.get(name) {
            Some(Entry::Ready(h)) => Some(h.clone()),
            _ => None,
        }
    }

    pub fn failed(&self, name: &str) -> bool {
        matches!(self.entries.get(name), Some(Entry::Failed))
    }

    fn finish(&mut self, name: &str, rgba: Vec<u8>, images: &mut Assets<Image>, assets: &mut crate::spui::view::SpuiAssets) {
        self.finish_sized(name, rgba, [SIZE, SIZE], images, assets);
    }

    fn finish_sized(&mut self, name: &str, rgba: Vec<u8>, [w, h]: [u32; 2], images: &mut Assets<Image>, assets: &mut crate::spui::view::SpuiAssets) {
        let handle = images.add(Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            rgba,
            // The SPUI painter samples its textures unconverted.
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        ));
        assets.insert_texture(image_key(name), crate::spui::view::Tex { handle: handle.clone(), size: [w, h] });
        self.entries.insert(name.to_string(), Entry::Ready(handle));
    }
}

/// One drawable of a thumbnail scene (also the detail view's turntable).
pub(super) struct Item {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub transform: Transform,
}

/// Render the queued thumbnails one at a time.
#[allow(clippy::too_many_arguments)]
pub fn render(
    mut commands: Commands,
    mut thumbs: ResMut<Thumbnails>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: ResMut<crate::spui::view::SpuiAssets>,
    paint_data: Option<Res<crate::creature_editor_paint::CreaturePaintData>>,
    mut block_paint: Option<ResMut<crate::rigid_paint::RigidPaintCache>>,
) {
    let thumbs = &mut *thumbs;
    match &mut thumbs.state {
        State::Idle => {
            let Some((name, source)) = thumbs.queue.pop_front() else { return };
            // An installed PNG: decoded as it is.
            if let Source::Png(key) = &source {
                match originals.resource_index.read(*key).ok().and_then(|b| decode_png(&b)) {
                    Some((rgba, size)) => thumbs.finish_sized(&name, rgba, size, &mut images, &mut assets),
                    None => {
                        thumbs.entries.insert(name, Entry::Failed);
                    }
                }
                return;
            }
            let file = thumbs.cache.as_ref().map(|d| cache_file(d, &name));
            if let Some(rgba) = file.as_ref().and_then(|f| read_png(f)) {
                thumbs.finish(&name, rgba, &mut images, &mut assets);
                return;
            }
            if originals.resource_index.records.is_empty() {
                thumbs.entries.insert(name, Entry::Failed);
                return;
            }
            let defaults = paint_data.map_or([0; 3], |d| d.defaults);
            let (items, front) = match items_for(&source, &mut originals, &mut meshes, &mut images, &mut materials, defaults, block_paint.as_deref_mut()) {
                Ok(v) => v,
                Err(error) => {
                    warn!("front end: thumbnail {name}: {error}");
                    thumbs.entries.insert(name, Entry::Failed);
                    return;
                }
            };
            let creature = match &source {
                Source::Creature(_) => true,
                Source::Stored(c) => c.is_creature(),
                _ => false,
            };
            let Some((centre, radius, aabb)) = bounds(&items, &meshes, creature) else {
                thumbs.entries.insert(name, Entry::Failed);
                return;
            };
            let target = images.add(Image::new_target_texture(RENDER, RENDER, TextureFormat::Rgba8UnormSrgb, None));
            thumbs.count += 1;
            let camera = camera_for(&source);
            thumbs.lowering = lowering_for(camera);
            // The frame is fitted to the geometry itself (see `fit_distance_points`).
            let points = fit_points(&items, &meshes, aabb);
            let (centre, aabb) = points_box(&points).unwrap_or((centre, aabb));
            let root = spawn_scene(&mut commands, SCENE_AT, &items, centre, radius, aabb, &points, front, camera, target.clone());
            thumbs.state = State::Rendering { name, frames: 0, root, target, asked: false };
        }
        State::Rendering { name, frames, root, target, asked } => {
            *frames += 1;
            if *frames >= WARMUP && !*asked {
                *asked = true;
                let inbox = thumbs.inbox.clone();
                let key = name.clone();
                commands.spawn(Screenshot::image(target.clone())).observe(move |captured: On<ScreenshotCaptured>| {
                    let rgba = captured.image.clone().try_into_dynamic().ok().map(|d| d.to_rgba8().into_raw());
                    if let (Some(rgba), Ok(mut inbox)) = (rgba, inbox.lock()) {
                        inbox.push((key.clone(), rgba));
                    }
                });
            }
            let done = thumbs.inbox.lock().ok().and_then(|mut inbox| {
                let i = inbox.iter().position(|(n, _)| n == name)?;
                Some(inbox.remove(i).1)
            });
            let (name, root, timed_out) = (name.clone(), *root, *frames > TIMEOUT);
            if let Some(rgba) = done.filter(|v| v.len() == (RENDER * RENDER * 4) as usize) {
                let rgba = compose(&rgba, RENDER as usize, SIZE as usize, thumbs.lowering);
                if let Some(dir) = &thumbs.cache {
                    let _ = std::fs::create_dir_all(dir);
                    if let Err(error) = write_png(&cache_file(dir, &name), &rgba) {
                        warn!("front end: thumbnail cache: {error}");
                    }
                }
                info!("front end: Sporepedia thumbnail {name} rendered");
                thumbs.finish(&name, rgba, &mut images, &mut assets);
            } else if timed_out {
                warn!("front end: thumbnail {name}: no capture");
                thumbs.entries.insert(name.clone(), Entry::Failed);
            } else {
                return;
            }
            commands.entity(root).despawn();
            thumbs.state = State::Idle;
        }
    }
}

/// The drawables of a creation (creation space) and the direction its front
/// faces.
pub(super) fn items_for(
    source: &Source,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    paint_defaults: [u32; 3],
    block_paint: Option<&mut crate::rigid_paint::RigidPaintCache>,
) -> Result<(Vec<Item>, Vec3), String> {
    match source {
        Source::Creature(path) => {
            let run = crate::persistence::load_from(path).map_err(|e| e.to_string())?;
            Ok((creature_items(&run, originals, meshes, images, materials, paint_defaults), Vec3::NEG_Z))
        }
        Source::Design(path, setup) => {
            let run = crate::persistence::load_from(path).map_err(|e| e.to_string())?;
            let design = run.rigid.designs.get(setup).ok_or("no such design")?.clone();
            let kind = crate::creature_editor_data::SetupKind::ALL
                .into_iter()
                .find(|k| k.setup_name() == setup)
                .ok_or("unknown setup")?;
            Ok((design_items(kind, &design, originals, meshes, images, materials, block_paint), Vec3::Z))
        }
        Source::Installed(key) => Ok((installed_items(*key, originals, meshes, images, materials)?, Vec3::Z)),
        Source::Png(_) => Err("a PNG is not rendered".into()),
        Source::Stored(creation) => match &creation.kind {
            super::store::StoredKind::Creature(_) => {
                Ok((creature_items(&creation.to_run(), originals, meshes, images, materials, paint_defaults), Vec3::NEG_Z))
            }
            super::store::StoredKind::Design { setup, design } => {
                let kind = crate::creature_editor_data::SetupKind::ALL
                    .into_iter()
                    .find(|k| k.setup_name() == setup)
                    .ok_or("unknown setup")?;
                Ok((design_items(kind, design, originals, meshes, images, materials, block_paint), Vec3::Z))
            }
        },
    }
}

/// A creature: its skin (built from the body, painted with its paint) and its
/// parts where the creator attaches them (`editor::refresh_parts`). A game
/// without creature parts yet shows the parts the creator seeds for it.
fn creature_items(
    run: &RunProgress,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    paint_defaults: [u32; 3],
) -> Vec<Item> {
    let mut run = run.clone();
    if run.parts.is_empty() && !originals.part_catalog.parts.is_empty() {
        run.parts = crate::editor::starter_creature_parts(&run, &originals.part_catalog);
    }
    crate::editor::load_saved_models(&run, originals, &GameStage::Creature, meshes, images, materials);
    let mut items = Vec::new();
    let body = run.body.validated();
    let skin = crate::creature_skin_view::enabled()
        .then(|| crate::creature_skin_view::build(originals.baked_body.as_ref(), &body, crate::creature_skin::RES_HIGH_QUALITY))
        .flatten();
    match skin {
        Some(built) => {
            let (diffuse, normal) = crate::creature_skin_view::paint_built(originals, &built, &run.creature_creation.paint, paint_defaults);
            let size = crate::creature_skin::PAINT_SIZE;
            let image = |data: Vec<u8>, format| {
                Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, format, RenderAssetUsages::default())
            };
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image(diffuse, TextureFormat::Rgba8UnormSrgb))),
                normal_map_texture: Some(images.add(image(normal, TextureFormat::Rgba8Unorm))),
                perceptual_roughness: 0.7,
                reflectance: 0.3,
                ..default()
            });
            items.push(Item { mesh: meshes.add(crate::creature_skin_view::render_mesh(&built)), material, transform: Transform::IDENTITY });
        }
        None => {
            let material = materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.7, 0.45), ..default() });
            items.push(Item { mesh: meshes.add(crate::body::mesh(&body)), material, transform: Transform::IDENTITY });
        }
    }
    for p in &run.parts {
        let Some(model) = crate::editor::model_for_part(originals, p) else { continue };
        let slot = crate::baked_model::attachment_point(originals, &body, p.bone);
        let material = model.material.clone().unwrap_or_else(|| materials.add(StandardMaterial::default()));
        items.push(Item {
            mesh: model.draw_mesh(),
            material,
            transform: Transform::from_translation(slot + Vec3::from_array(p.offset))
                .with_rotation(p.rotation())
                .with_scale(p.mesh_scale(model.scale * 0.5 * p.scale)),
        });
    }
    items
}

/// A rigid design: its blocks as the editor and the stages place them
/// (`rigid_editor::block_transform` at stage scale 1).
fn design_items(
    kind: crate::creature_editor_data::SetupKind,
    design: &crate::rigid_editor::RigidDesign,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    mut block_paint: Option<&mut crate::rigid_paint::RigidPaintCache>,
) -> Vec<Item> {
    use crate::rigid_editor as re;
    let group = kind.part_group();
    let mut items = Vec::new();
    for p in &design.parts {
        let Some(block) = &p.block else { continue };
        let _ = re::import_block(originals, group, p.definition_id, p.instance, meshes, images, materials);
        // A block with handle weights is drawn with its morphed mesh.
        re::ensure_morph(originals, meshes, group, p.definition_id, p.instance, &block.handles);
        let Some(model) = re::block_model(originals, group, p.definition_id, p.instance, &block.handles) else { continue };
        let model = &model;
        let unit = re::editor_unit(model) * p.scale.clamp(0.1, 4.0);
        let t = re::block_transform(block, unit, 1.0, Vec3::ZERO);
        if model.surfaces.is_empty() {
            let material = model.material.clone().unwrap_or_else(|| materials.add(StandardMaterial::default()));
            items.push(Item { mesh: model.draw_mesh(), material, transform: t });
        } else {
            for (i, s) in model.surfaces.iter().enumerate() {
                // The block's paint (`rigid_paint`), as the editor and the stages draw it.
                let material = block_paint
                    .as_deref_mut()
                    .and_then(|c| c.surface_material(originals, images, materials, model, i, &block.paints, false))
                    .unwrap_or_else(|| s.material.clone());
                items.push(Item { mesh: s.handle.clone(), material, transform: t });
            }
        }
    }
    items
}

/// An installed creation: its blocks as `creation_assets::mount_preset` places
/// them (paint colour 1 on each block).
fn installed_items(
    key: ResourceKey,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Vec<Item>, String> {
    if !originals.creation_models.contains_key(&key) {
        let record = originals.creation_catalog.creations.iter().find(|c| c.key == key).cloned().ok_or("not installed")?;
        let imported = crate::creation_assets::import_creation(originals, &record, meshes, images, materials)?;
        originals.creation_models.insert(key, imported);
    }
    let imported = originals.creation_models.get(&key).ok_or("not imported")?;
    let mut items = Vec::new();
    for block in &imported.blocks {
        let model = &block.mechanical.model;
        let mut material = model.material.as_ref().and_then(|h| materials.get(h)).cloned().unwrap_or_default();
        if let Some(paint) = block.paints.first() {
            material.base_color = Color::srgb(paint.color1[0], paint.color1[1], paint.color1[2]);
        }
        items.push(Item {
            mesh: model.draw_mesh(),
            material: materials.add(material),
            transform: Transform::from_translation(block.position)
                .with_rotation(Quat::from_mat3(&block.orientation).normalize())
                .with_scale(Vec3::splat(block.scale * model.scale)),
        });
    }
    Ok(items)
}

/// Centre, radius and box of the items' bounds. With `body_first` (a
/// creature) the parts count only near the body: a part far from it (or much
/// larger) would shrink the creature in the frame.
pub(super) fn bounds(items: &[Item], meshes: &Assets<Mesh>, body_first: bool) -> Option<(Vec3, f32, [Vec3; 2])> {
    let boxes: Vec<Option<[Vec3; 2]>> = items
        .iter()
        .map(|item| {
            let mesh = meshes.get(&item.mesh)?;
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
                return None;
            };
            // Only vertices the triangles use (a buffer may hold unused ones).
            let (mut a, mut b) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            let mut add = |p: &[f32; 3]| {
                a = a.min(Vec3::from_array(*p));
                b = b.max(Vec3::from_array(*p));
            };
            match mesh.indices() {
                Some(indices) => indices.iter().filter_map(|i| points.get(i)).for_each(&mut add),
                None => points.iter().for_each(&mut add),
            }
            if !(a.is_finite() && b.is_finite()) {
                return None;
            }
            let (c, h) = ((a + b) * 0.5, (b - a) * 0.5);
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for i in 0..8 {
                let corner = c + h * Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
                let p = item.transform.transform_point(corner);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            Some([lo, hi])
        })
        .collect();
    let body = if body_first { boxes.first().copied().flatten() } else { None };
    debug!("thumbnail bounds: body {body:?}, items {:?}", boxes.iter().flatten().map(|b| b[1] - b[0]).collect::<Vec<_>>());
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for b in boxes.iter().flatten() {
        if let Some(body) = body {
            let (bc, br) = ((body[0] + body[1]) * 0.5, ((body[1] - body[0]) * 0.5).length());
            let (c, r) = ((b[0] + b[1]) * 0.5, ((b[1] - b[0]) * 0.5).length());
            if c.distance(bc) > 1.6 * br || r > 1.5 * br {
                continue;
            }
        }
        lo = lo.min(b[0]);
        hi = hi.max(b[1]);
    }
    (lo.is_finite() && hi.is_finite()).then(|| ((lo + hi) * 0.5, ((hi - lo) * 0.5).length().max(0.05), [lo, hi]))
}

/// The scene: the items, a three-quarter front camera framing them, a studio
/// light, all on [`LAYER`].
#[allow(clippy::too_many_arguments)]
fn spawn_scene(
    commands: &mut Commands,
    at: Vec3,
    items: &[Item],
    centre: Vec3,
    radius: f32,
    aabb: [Vec3; 2],
    points: &[Vec3],
    front: Vec3,
    camera: ThumbCamera,
    target: Handle<Image>,
) -> Entity {
    let layer = RenderLayers::layer(LAYER);
    let fov = camera.fov;
    let side = front.cross(Vec3::Y).normalize_or(Vec3::X);
    let dir = view_direction(front, camera);
    // The model just inside the frustum, the frame filled to `IMAGE_SCALE`.
    let fitted = if points.is_empty() {
        (fit_distance(aabb, centre, dir, fov) / FIT_MARGIN).min(radius / (fov * 0.5).sin() * 1.02)
    } else {
        fit_distance_points(points, centre, dir, fov)
    };
    // The first pass of the retail effect: the model just inside the frame
    // (`compose` then scales its covered box to `IMAGE_SCALE`).
    let distance = fitted * FIT_MARGIN;
    let root = commands
        .spawn((Transform::from_translation(at - centre), Visibility::Inherited, Name::new("sporepedia thumbnail")))
        .id();
    commands.entity(root).with_children(|parent| {
        for item in items {
            parent.spawn((Mesh3d(item.mesh.clone()), MeshMaterial3d(item.material.clone()), item.transform, layer.clone()));
        }
        parent.spawn((
            Camera3d::default(),
            Camera { order: -3, clear_color: Color::NONE.into(), ..default() },
            RenderTarget::from(target),
            Projection::Perspective(PerspectiveProjection {
                fov,
                near: (distance - radius * 1.5).max(0.01),
                far: distance + radius * 1.5,
                ..default()
            }),
            Transform::from_translation(centre + dir * distance).looking_at(centre, Vec3::Y),
            layer.clone(),
            AmbientLight { color: Color::srgb(0.85, 0.88, 1.0), brightness: 320.0, ..default() },
        ));
        parent.spawn((
            DirectionalLight { color: Color::srgb(1.0, 0.95, 0.86), illuminance: 6000.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(centre + (dir + Vec3::Y * 0.8 - side * 0.6).normalize()).looking_at(centre, Vec3::Y),
            layer.clone(),
        ));
    });
    root
}

/// The retail composition (see the header) of a render (`RGBA8`, `render`
/// pixels square, transparent where nothing was drawn) into a thumbnail of
/// `size` pixels: the covered box's larger side becomes `IMAGE_SCALE` of the
/// frame, the box is centred, a box wider than tall is moved down by `lowering`
/// of the free vertical space / 2, and the fitted shadow goes behind the model.
pub(super) fn compose(rgba: &[u8], render: usize, size: usize, lowering: f32) -> Vec<u8> {
    let alpha = |x: usize, y: usize| rgba[(y * render + x) * 4 + 3];
    let (mut x0, mut x1, mut y0, mut y1) = (render, 0usize, render, 0usize);
    for y in 0..render {
        for x in 0..render {
            if alpha(x, y) > 4 {
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                y0 = y0.min(y);
                y1 = y1.max(y + 1);
            }
        }
    }
    let mut out = vec![0u8; size * size * 4];
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let scale = IMAGE_SCALE * size as f32 / w.max(h);
    let (sw, sh) = (w * scale, h * scale);
    let free_y = size as f32 - sh;
    let drop = if w > h { lowering * free_y * 0.5 } else { 0.0 };
    let (ox, oy) = ((size as f32 - sw) * 0.5, free_y * 0.5 + drop);
    // Premultiplied bilinear samples of the render, four per output pixel.
    let sample = |fx: f32, fy: f32| -> [f32; 4] {
        let (fx, fy) = ((fx - 0.5).clamp(0.0, render as f32 - 1.001), (fy - 0.5).clamp(0.0, render as f32 - 1.001));
        let (ix, iy, tx, ty) = (fx as usize, fy as usize, fx.fract(), fy.fract());
        let px = |x: usize, y: usize| {
            let i = (y * render + x) * 4;
            let a = rgba[i + 3] as f32 / 255.0;
            [rgba[i] as f32 / 255.0 * a, rgba[i + 1] as f32 / 255.0 * a, rgba[i + 2] as f32 / 255.0 * a, a]
        };
        let (a, b, c, d) = (px(ix, iy), px(ix + 1, iy), px(ix, iy + 1), px(ix + 1, iy + 1));
        std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty)
    };
    let mut model = vec![[0.0f32; 4]; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let fx = (x as f32 + dx - ox) / scale + x0 as f32;
                let fy = (y as f32 + dy - oy) / scale + y0 as f32;
                if fx < x0 as f32 - 1.0 || fx > x1 as f32 + 1.0 || fy < y0 as f32 - 1.0 || fy > y1 as f32 + 1.0 {
                    continue;
                }
                let s = sample(fx, fy);
                for k in 0..4 {
                    acc[k] += s[k] * 0.25;
                }
            }
            model[y * size + x] = acc;
        }
    }
    let shadow = shadow_alpha(&model.iter().map(|p| p[3]).collect::<Vec<_>>(), size);
    for i in 0..size * size {
        let (m, sh) = (model[i], shadow[i]);
        // The model over the black shadow (premultiplied).
        let a = m[3] + sh * (1.0 - m[3]);
        if a <= 0.0 {
            continue;
        }
        for k in 0..3 {
            out[i * 4 + k] = (m[k] / a * 255.0).clamp(0.0, 255.0).round() as u8;
        }
        out[i * 4 + 3] = (a * 255.0).clamp(0.0, 255.0).round() as u8;
    }
    out
}

/// ADAPTED (see the header): the alpha of the fitted ground shadow of a
/// silhouette (`alpha` of the composed model, `size` pixels square): the
/// silhouette squashed about its base, leaning left, lifted, blurred.
fn shadow_alpha(alpha: &[f32], size: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; size * size];
    let (mut x0, mut x1, mut y0, mut y1) = (size, 0usize, size, 0usize);
    for y in 0..size {
        for x in 0..size {
            if alpha[y * size + x] > 0.5 {
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                y0 = y0.min(y);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    let (centre_x, base) = ((x0 + x1) as f32 * 0.5, y1 as f32);
    let lift = SHADOW_LIFT * size as f32;
    for y in 0..size {
        // The source row: the shadow stands on the base line at 0.7 of the height.
        let ys = base - (base - (y as f32 + 0.5 - lift)) / SHADOW_SQUASH;
        if ys < y0 as f32 || ys >= y1 as f32 {
            continue;
        }
        let height = base - ys;
        for x in 0..size {
            let xs = centre_x + (x as f32 + 0.5 - SHADOW_LEAN * height - centre_x);
            if xs < 0.0 || xs >= size as f32 {
                continue;
            }
            if alpha[ys as usize * size + xs as usize] > 0.5 {
                out[y * size + x] = SHADOW_ALPHA;
            }
        }
    }
    // Two box blurs of the shadow's radius.
    let radius = ((SHADOW_BLUR * size as f32).round() as usize).max(1);
    for _ in 0..2 {
        out = box_blur(&out, size, radius);
    }
    out
}

fn box_blur(src: &[f32], size: usize, radius: usize) -> Vec<f32> {
    let n = (2 * radius + 1) as f32;
    let mut tmp = vec![0.0f32; src.len()];
    for y in 0..size {
        for x in 0..size {
            let mut acc = 0.0;
            for dx in 0..=2 * radius {
                let sx = x as i64 + dx as i64 - radius as i64;
                if sx >= 0 && (sx as usize) < size {
                    acc += src[y * size + sx as usize];
                }
            }
            tmp[y * size + x] = acc / n;
        }
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..size {
        for x in 0..size {
            let mut acc = 0.0;
            for dy in 0..=2 * radius {
                let sy = y as i64 + dy as i64 - radius as i64;
                if sy >= 0 && (sy as usize) < size {
                    acc += tmp[sy as usize * size + x];
                }
            }
            out[y * size + x] = acc / n;
        }
    }
    out
}

/// The vertices of the items in the scene's frame, the ones inside `aabb` (the
/// box `bounds` kept: a creature's far parts stay out), thinned to about 1500
/// per mesh. Only vertices a triangle uses count.
pub(super) fn fit_points(items: &[Item], meshes: &Assets<Mesh>, aabb: [Vec3; 2]) -> Vec<Vec3> {
    let eps = Vec3::splat(1e-3);
    let mut out = Vec::new();
    for item in items {
        let Some(mesh) = meshes.get(&item.mesh) else { continue };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let mut used = vec![mesh.indices().is_none(); points.len()];
        if let Some(indices) = mesh.indices() {
            for i in indices.iter() {
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
            }
        }
        let step = (points.len() / 1500).max(1);
        for (p, used) in points.iter().zip(&used).step_by(step) {
            let w = item.transform.transform_point(Vec3::from_array(*p));
            if *used && w.cmpge(aabb[0] - eps).all() && w.cmple(aabb[1] + eps).all() {
                out.push(w);
            }
        }
    }
    out
}

/// Centre and box of some points.
pub(super) fn points_box(points: &[Vec3]) -> Option<(Vec3, [Vec3; 2])> {
    let first = *points.first()?;
    let (lo, hi) = points.iter().fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    Some(((lo + hi) * 0.5, [lo, hi]))
}

/// Camera distance along `dir` from `centre` at which every point is just
/// inside the frustum of a camera with the vertical FOV `fov` looking at
/// `centre` (the exit condition of the editors' framing rule `FUN_005A33B0`:
/// the model's boxes inside the FOV; ADAPTED: the geometry's own points, not
/// per-block boxes).
pub(super) fn fit_distance_points(points: &[Vec3], centre: Vec3, dir: Vec3, fov: f32) -> f32 {
    let right = Vec3::Y.cross(dir).normalize_or(Vec3::X);
    let up = dir.cross(right);
    let tan = (fov * 0.5).tan();
    points
        .iter()
        .map(|p| {
            let p = *p - centre;
            p.dot(right).abs().max(p.dot(up).abs()) / tan + p.dot(dir)
        })
        .fold(0.0, f32::max)
}

/// The margin `fit_distance` leaves around the box.
const FIT_MARGIN: f32 = 1.08;

/// Camera distance along `dir` from `centre` at which the box fills the
/// view (with a small margin).
fn fit_distance(aabb: [Vec3; 2], centre: Vec3, dir: Vec3, fov: f32) -> f32 {
    let right = Vec3::Y.cross(dir).normalize_or(Vec3::X);
    let up = dir.cross(right);
    let tan = (fov * 0.5).tan();
    let mut d: f32 = 0.0;
    for i in 0..8 {
        let p = Vec3::new(
            if i & 1 == 0 { aabb[0].x } else { aabb[1].x },
            if i & 2 == 0 { aabb[0].y } else { aabb[1].y },
            if i & 4 == 0 { aabb[0].z } else { aabb[1].z },
        ) - centre;
        let lateral = p.dot(right).abs().max(p.dot(up).abs());
        d = d.max(lateral / tan + p.dot(dir));
    }
    d * FIT_MARGIN
}

fn write_png(path: &Path, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), SIZE, SIZE);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}

/// An installed PNG as RGBA8 and its size (palette / RGB / grey expanded).
pub fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, [u32; 2])> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let pixels = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        _ => return None,
    };
    (rgba.len() == pixels * 4).then_some((rgba, [info.width, info.height]))
}

fn read_png(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let ok = info.width == SIZE && info.height == SIZE && info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight;
    ok.then(|| {
        buf.truncate(info.buffer_size());
        buf
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_are_cached_by_content_beside_the_saves() {
        assert_eq!(cache_dir(Path::new("C:/x/qc_save.json")), Path::new("C:/x/qc_save-thumbs"));
        assert_ne!(content_hash(b"a"), content_hash(b"b"));
        assert_eq!(image_key("c-1").group_id, IMAGE_GROUP);
        let mut t = Thumbnails::default();
        let source = Source::Installed(ResourceKey { group_id: 1, instance_id: 2, type_id: 3 });
        assert!(!t.request("m-1", &source));
        assert!(!t.request("m-1", &source), "queued once");
        assert_eq!(t.queue.len(), 1);
        let dir = std::env::temp_dir().join(format!("openspore-fe-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rgba = vec![200u8; (SIZE * SIZE * 4) as usize];
        write_png(&dir.join("x.png"), &rgba).unwrap();
        assert_eq!(read_png(&dir.join("x.png")).as_deref(), Some(rgba.as_slice()));
        let bytes = std::fs::read(dir.join("x.png")).unwrap();
        assert_eq!(decode_png(&bytes).map(|(_, size)| size), Some([SIZE, SIZE]));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn thumbnail_cameras_look_from_the_editors_angles() {
        // The creature faces -Z (Bevy basis of the skin), the rigid designs +Z.
        let c = view_direction(Vec3::NEG_Z, CREATURE_CAMERA);
        let expected = Vec3::new(-(50f32.to_radians().sin()) * 15f32.to_radians().cos(), 15f32.to_radians().sin(), -(50f32.to_radians().cos()) * 15f32.to_radians().cos());
        assert!((c - expected).length() < 1e-5, "{c:?} vs {expected:?}");
        let v = view_direction(Vec3::Z, VEHICLE_CAMERA);
        assert!(v.x > 0.0 && v.y > 0.0 && v.z > 0.0 && v.x > v.z, "66 degrees from the front: {v:?}");
        assert!((v.length() - 1.0).abs() < 1e-5);
        // Creatures use the long lens, buildings and vehicles 0.5726 rad.
        assert!(CREATURE_CAMERA.fov < BUILDING_CAMERA.fov && BUILDING_CAMERA.fov == VEHICLE_CAMERA.fov);
        assert_eq!(camera_for(&Source::Design(PathBuf::new(), "BuildingHouse".into())), BUILDING_CAMERA);
        assert_eq!(camera_for(&Source::Design(PathBuf::new(), "VehicleMilitaryLand".into())), VEHICLE_CAMERA);
        assert_eq!(camera_for(&Source::Design(PathBuf::new(), "UFOEditorSetup".into())), VEHICLE_CAMERA);
        assert_eq!(camera_for(&Source::Creature(PathBuf::new())), CREATURE_CAMERA);
        assert_eq!(cache_file(Path::new("d"), "c-1"), Path::new("d").join("c-1.v4.png"));
        assert_eq!((lowering_for(CREATURE_CAMERA), lowering_for(BUILDING_CAMERA), lowering_for(VEHICLE_CAMERA)), (0.5, 0.4, 0.4));
    }

    /// A render with an opaque white rectangle (x0 .. x1, y0 .. y1) of `render` px.
    fn rectangle(render: usize, x0: usize, x1: usize, y0: usize, y1: usize) -> Vec<u8> {
        let mut rgba = vec![0u8; render * render * 4];
        for y in y0..y1 {
            for x in x0..x1 {
                rgba[(y * render + x) * 4..(y * render + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        rgba
    }

    /// The box of the pixels of at least `min` alpha.
    fn covered(rgba: &[u8], size: usize, min: u8) -> (usize, usize, usize, usize) {
        let (mut x0, mut x1, mut y0, mut y1) = (size, 0, size, 0);
        for y in 0..size {
            for x in 0..size {
                if rgba[(y * size + x) * 4 + 3] >= min {
                    x0 = x0.min(x);
                    x1 = x1.max(x + 1);
                    y0 = y0.min(y);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x0, x1, y0, y1)
    }

    #[test]
    fn the_thumbnail_is_scaled_centred_and_wide_models_are_lowered() {
        let size = SIZE as usize;
        // A tall model off to one side of its render: scaled to 0.85, centred, not lowered.
        let tall = compose(&rectangle(384, 40, 100, 60, 360), 384, size, 0.5);
        let (x0, x1, y0, y1) = covered(&tall, size, 250);
        assert!(((y1 - y0) as f32 - 0.85 * size as f32).abs() < 3.0, "major side 0.85: {}", y1 - y0);
        assert!(((x0 + x1) as f32 * 0.5 - size as f32 * 0.5).abs() < 2.0, "centred in x: {x0} {x1}");
        assert!(((y0 + y1) as f32 * 0.5 - size as f32 * 0.5).abs() < 2.0, "centred in y: {y0} {y1}");
        // A wide model: 0.85 wide, dropped by lowering x free space / 2.
        let wide = compose(&rectangle(384, 20, 360, 150, 250), 384, size, 0.5);
        let (x0, x1, y0, y1) = covered(&wide, size, 250);
        assert!(((x1 - x0) as f32 - 0.85 * size as f32).abs() < 3.0, "major side 0.85: {}", x1 - x0);
        let h = (y1 - y0) as f32;
        let free = size as f32 - h;
        let drop = (y0 + y1) as f32 * 0.5 - size as f32 * 0.5;
        assert!((drop - 0.5 * free * 0.5).abs() < 2.0, "lowered by 0.5 x free / 2: {drop} of {}", free * 0.5);
        // No lowering: centred.
        let flat = compose(&rectangle(384, 20, 360, 150, 250), 384, size, 0.0);
        let (_, _, y0, y1) = covered(&flat, size, 250);
        assert!(((y0 + y1) as f32 * 0.5 - size as f32 * 0.5).abs() < 2.0);
        // A render with nothing in it stays empty.
        assert!(compose(&vec![0u8; 384 * 384 * 4], 384, size, 0.5).iter().all(|b| *b == 0));
    }

    #[test]
    fn the_shadow_is_light_to_the_lower_left_and_leaves_the_model_alone() {
        let size = SIZE as usize;
        let rgba = compose(&rectangle(384, 100, 300, 40, 340), 384, size, 0.5);
        let (x0, x1, y0, y1) = covered(&rgba, size, 250);
        let px = |x: usize, y: usize| &rgba[(y * size + x) * 4..(y * size + x) * 4 + 4];
        // The model keeps its colour and alpha.
        assert_eq!(px((x0 + x1) / 2, (y0 + y1) / 2), &[255, 255, 255, 255]);
        // Left of the model, low down: a translucent black, no more than the shadow's alpha.
        let (sx, sy) = (x0 - 3, y1 - (y1 - y0) / 5);
        let s = px(sx, sy);
        assert!(s[3] > 8 && (s[3] as f32) < SHADOW_ALPHA * 255.0 + 3.0 && s[0] == 0, "{s:?}");
        // The shadow stays below the lifted top of the squashed copy: nothing above it.
        assert_eq!(px(x0 - 3, y0 + 2)[3], 0);
        // Right of the model: nothing.
        assert_eq!(px((x1 + 6).min(size - 1), sy)[3], 0);
    }    #[test]
    fn the_frame_is_fitted_to_the_points() {
        // A unit cube seen along +Z from a 1 rad lens: the near face corners
        // at depth -0.5 need the most distance, the far ones the least.
        let mut points = Vec::new();
        for i in 0..8 {
            points.push(Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { -0.5 } else { 0.5 }, if i & 4 == 0 { -0.5 } else { 0.5 }));
        }
        let d = fit_distance_points(&points, Vec3::ZERO, Vec3::Z, 1.0);
        let tan = 0.5f32.tan();
        assert!((d - (0.5 / tan + 0.5)).abs() < 1e-5, "{d}");
        // Every point is inside the frustum at that distance.
        for p in &points {
            let depth = d - p.z;
            assert!(p.x.abs() <= tan * depth + 1e-5 && p.y.abs() <= tan * depth + 1e-5);
        }
        assert_eq!(points_box(&points).map(|(c, _)| c), Some(Vec3::ZERO));
        assert!(points_box(&[]).is_none());
    }
}
