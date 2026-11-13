//! The creation in the Sporepedia detail dialog (`AssetViewLarge`): the model
//! drawn into the dialog's render window.
//!
//! Retail's `cSPUILargeAssetView` (0x66EFB0 / 0x66DAC0) puts the opened asset
//! into a viewer (`cSwatchManager::AddViewer`, 0x5F0D20) whose render window
//! is the dialog's window `065F79E0` (40,24 .. 479,583: 439 x 559), with a
//! background environment picked per asset type (0x66C9B0). The model does not
//! turn by itself: no time-driven turn exists in the dialog's code (0x66B000 ..
//! 0x66F100 holds only the button gains below) or in the camera controller it
//! drives (0x5A21E0 / 0x5A2220). It turns by input:
//! * the rotate buttons `04CAB56F` / `04CAB56E` (held): yaw += pi x 0.02 per
//!   update (0x66BF40 -> 0x5A2220; 0.02 is `2034014D` of the Sporepedia config
//!   `851D4139!B55619C2`, read at 0x66BD50 / 0x66E5AC);
//! * the zoom buttons `04CAB570` (in) / `04CAB571` (out) (held): the view's
//!   magnification is multiplied by 1 +- 0.01 per update (0x5A21E0; 0.01 is
//!   `C97F8EE9` of the same file);
//! * a drag in the render window: yaw += 0.0015 rad per pixel (0x5F2D16, the
//!   viewer's own mouse handler).
//! OpenSpore renders the creation into an image on its own render layer (like
//! the card thumbnails, `thumbs`): the player's creations from their records,
//! the installed Maxis creations from their baked models (the GMDL beside each
//! `pollen_metadata` record: group `0x406?6200` -> `0x406?7100`, same
//! instance; VERIFIED for creatures `baked_model::CREATURE_GROUP`, buildings
//! and flora). An entry without a model keeps its card image.
//!
//! The viewer camera (VERIFIED): the viewer's camera is an `Editors::EditorCamera`
//! (type `kEditorCameraControllerType~` 0xFCC521, registered by the swatch
//! manager at 0x5F0345 with the factory 0x5A4140), read by
//! `EditorCamera::Initialize` (0x5A25E0) from a `camera_properties~` file
//! (group 0x40410100). The file of each asset type is the editor setup's
//! `editorSwatchCameraUI` (`40600100` setups): creature `CF2DC8D0` ->
//! `EditorCameraCreatureUI`, building `D817CD63` -> `EditorCameraBuildingUI`,
//! land vehicle `29DEABC2` -> `EditorCameraVehicleSporepedia`, flora
//! `1C7ECA95` -> `EditorCameraFloraUI`, cell `3615A30B` ->
//! `EditorCameraCellSporePedia`. Their `cameraInitialHeading` / `Pitch` give
//! the first view (creature -50 / -8.6, building -57 / -15, vehicle -66 / -10,
//! flora -50 / -5.6, cell -90 / -90), `cameraInitialFOV` the lens (absent: the
//! controller's 1 rad default, 0x5A3210; the projection keeps the vertical
//! angle `0.75 tan(fov / 2)` in a non-square window, `creature_editor::
//! retail_vertical_fov`), and the zoom buttons move the controller's distance
//! `[+0x38]` from `cameraInitialZoom` inside `cameraMinZoomDistance` +0x58 /
//! `cameraMaxZoomDistance` +0x5C (creature 5 in 2.5 .. 7.5, building 55 in
//! 25 .. 132, vehicle 9.8 in 3 .. 40, flora 25 in 8 .. 40, cell 2 in 3 .. 4:
//! the first press snaps it into the range), which OpenSpore keeps as a
//! magnification `initial / distance` (range `initial / max .. initial / min`).
//! That the large asset view uses `editorSwatchCameraUI` (and not another file)
//! is INFERRED from the property's name, the installed `...SporePedia` camera
//! files and the swatch manager's use of the editor camera type; the view
//! creates the viewer through `cSwatchManager::AddViewer` (0x5F0D20, caller
//! 0x66EC4D) and no other camera key is read on that path.
//!
//! ADAPTED: the first view's distance is fitted to the model (the retail
//! distances are in the swatch's own units, not traced), the light, the
//! background colour (retail picks one of nine environments per type, 0x66C9B0:
//! the browser's asset subtype `9EA3031A` creature / `372E2C04` tribal creature /
//! `4178B8E8` or `65672ADE` / `CCC35C46` draws one of nine (ten) Swarm effect ids
//! with `Math::sMainRNG` from the tables at 0x1400720 .. 0x1400790
//! (`0x4112345, 0x44D7F20, 0x411238E, 0x41123A5, ...`), a cell `DFAD9F51` one of two
//! (`0x5DA8E7D`, `0x5DFDEDD`), the plants `37148141` / `B8669EC9` / `BCD73E89` the
//! fixed `0x5DA8E82`, anything else `0x4112345`, created by 0x5F2D80 in the
//! viewer's effect world; VERIFIED that none of those ids is exported by the 36
//! installed effect directories), the flat stand under the creation and the
//! per-frame scaling of the button steps (retail applies them per update).

use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};

use super::{pedia, thumbs, FrontEnd, Overlay};
use crate::{dbpf::ResourceKey, Originals};

/// Render layer of the turntable scene (24 thumbnails, 23 Space portraits).
pub const LAYER: usize = 22;
/// Image size (pixels): the render window of `AssetViewLarge` (439 x 559).
pub const SIZE: [u32; 2] = [440, 560];
/// Yaw per update of a held rotate button (0x5A2220: `pi * 0.02`, `2034014D`).
pub const ROTATE_STEP: f32 = std::f32::consts::PI * 0.02;
/// Magnification change per update of a held zoom button (0x5A21E0: `0.01`,
/// `C97F8EE9`).
pub const ZOOM_STEP: f32 = 0.01;
/// Yaw per pixel of a drag in the render window (0x5F2D16).
pub const DRAG_GAIN: f32 = 0.0015;
/// The retail rate of the button steps (per update). ADAPTED: the steps are
/// scaled by `dt * UPDATE_RATE` so they do not depend on the frame rate.
pub const UPDATE_RATE: f32 = 60.0;
/// Magnification range of a viewer without a camera file: the controller's
/// own defaults (0x5A3210: distance 10 in 5 .. 25).
const DEFAULT_ZOOM_RANGE: [f32; 2] = [10.0 / 25.0, 10.0 / 5.0];
/// Where the scene stands (far below every stage, beside the thumbnails).
const SCENE_AT: Vec3 = Vec3::new(6_000.0, -20_000.0, 0.0);
/// Frames before the first read-back (pipelines, textures); the image is read
/// again at multiples of it while the textures stream in.
const READBACK_WARMUP: u32 = 24;
/// The viewer camera's FOV without a camera file or `cameraInitialFOV`: 1 rad
/// (0x5A3210 stores 57.29578 degrees).
const DEFAULT_FOV: f32 = 1.0;
/// The background of the render window. ADAPTED (retail: one of nine
/// environments per asset type).
const BACKGROUND: Color = Color::srgb(0.06, 0.1, 0.16);

/// The SPUI image key of the creation's render target (by its thumbnail
/// name): the dialog's render window draws it through `SpuiState::set_image`.
pub fn image_key(name: &str) -> ResourceKey {
    ResourceKey { group_id: 0x0F2E_1B01, instance_id: crate::world_objects::name_id(name), type_id: thumbs::PNG_TYPE }
}

/// The turning node (its children are the creation's drawables).
#[derive(Component)]
pub struct TurntableSpin;

/// The viewer camera: it looks at the stand from `dir` at `distance`
/// divided by the magnification.
#[derive(Component)]
pub struct TurntableCamera {
    pub dir: Vec3,
    pub distance: f32,
}

/// What the player has done to the view of the shown creation (the dialog's
/// buttons and drag, `pedia::actions`); reset when another creation opens.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Turn about the vertical, radians.
    pub yaw: f32,
    /// Magnification (1 = the first view).
    pub zoom: f32,
    /// The magnification range of the shown asset's camera file.
    pub range: [f32; 2],
}

impl Default for View {
    fn default() -> Self {
        Self { yaw: 0.0, zoom: 1.0, range: DEFAULT_ZOOM_RANGE }
    }
}

impl View {
    pub fn rotate(&mut self, radians: f32) {
        self.yaw = (self.yaw + radians).rem_euclid(std::f32::consts::TAU);
    }

    /// Multiply the magnification by `1 + step` (0x5A21E0), kept in range.
    pub fn zoom_by(&mut self, step: f32) {
        self.zoom = ((1.0 + step) * self.zoom).clamp(self.range[0], self.range[1]);
    }
}

/// The swatch camera file of a source (`editorSwatchCameraUI` of its editor
/// setup, see the header).
pub fn swatch_camera_name(source: &thumbs::Source) -> &'static str {
    use crate::creation_records::{BLD, UFO, VCL};
    let rigid = |setup: &str| match crate::creature_editor_data::SetupKind::ALL.into_iter().find(|k| k.setup_name() == setup) {
        Some(kind) if kind.is_building() => "EditorCameraBuildingUI",
        _ => "EditorCameraVehicleSporepedia",
    };
    match source {
        thumbs::Source::Design(_, setup) => rigid(setup),
        thumbs::Source::Installed(key) => match key.type_id {
            BLD => "EditorCameraBuildingUI",
            VCL | UFO => "EditorCameraVehicleSporepedia",
            _ => "EditorCameraCreatureUI",
        },
        thumbs::Source::Stored(c) => match &c.kind {
            super::store::StoredKind::Design { setup, .. } => rigid(setup),
            _ => "EditorCameraCreatureUI",
        },
        // The creation PNGs of the installed Maxis creations: the group names the type
        // (0x4061 cell, 0x4062 creature, 0x4063 building, 0x4064 / 0x4065 vehicle / UFO, 0x4066 flora).
        thumbs::Source::Png(key) => match key.group_id >> 16 {
            0x4061 => "EditorCameraCellSporePedia",
            0x4063 => "EditorCameraBuildingUI",
            0x4064 | 0x4065 => "EditorCameraVehicleSporepedia",
            0x4066 => "EditorCameraFloraUI",
            _ => "EditorCameraCreatureUI",
        },
        thumbs::Source::Creature(_) => "EditorCameraCreatureUI",
    }
}

/// The magnification range of a camera file: the controller's distance
/// starts at `cameraInitialZoom` and lives in `min .. max` (0x5A21E0 clamps
/// `[+0x38]` between +0x58 and +0x5C), so the magnification `initial / distance`
/// lies in `initial / max .. initial / min`.
pub fn zoom_range(props: &crate::creature_editor_data::EditorCameraProps) -> [f32; 2] {
    if props.min_zoom > 0.0 && props.max_zoom >= props.min_zoom && props.zoom > 0.0 {
        [props.zoom / props.max_zoom, props.zoom / props.min_zoom]
    } else {
        DEFAULT_ZOOM_RANGE
    }
}

/// The view the last read-back showed, with the generation of the scene.
type Frame = (u64, Vec<u8>);

#[derive(Resource, Default)]
pub struct Turntable {
    /// Thumbnail name of the creation on the stand.
    pub shown: Option<String>,
    root: Option<Entity>,
    target: Option<Handle<Image>>,
    frames: u32,
    /// The creation has no model (its card image stays).
    pub failed: bool,
    /// The picture the dialog's render window shows: the render target read
    /// back into an `Rgba8Unorm` image (the SPUI painter samples gamma-space
    /// bytes unconverted; the sRGB render target would read back dark).
    cpu: Option<Handle<Image>>,
    /// The view of the last request, a request in flight, the scene's number
    /// and the finished read-backs.
    requested: Option<View>,
    pending: bool,
    pending_at: u32,
    generation: u64,
    inbox: std::sync::Arc<std::sync::Mutex<Vec<Frame>>>,
}

impl Turntable {
    /// The turning image once its first read-back is in.
    pub fn image(&self, name: &str) -> Option<Handle<Image>> {
        (self.shown.as_deref() == Some(name) && !self.failed).then(|| self.cpu.clone()).flatten()
    }
}

/// The baked model of an installed creation, by its card PNG key.
pub fn baked_key(png: ResourceKey) -> ResourceKey {
    ResourceKey {
        type_id: crate::decode::TYPE_GMDL,
        group_id: (png.group_id & 0xFFFF_0000) | 0x7100,
        instance_id: png.instance_id,
    }
}

/// Build / replace / turn the scene of the open detail view.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    fe: Res<FrontEnd>,
    pedia: Res<pedia::PediaState>,
    mut table: ResMut<Turntable>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    paint_data: Option<Res<crate::creature_editor_paint::CreaturePaintData>>,
    mut block_paint: Option<ResMut<crate::rigid_paint::RigidPaintCache>>,
    mut view: ResMut<View>,
    mut assets: ResMut<crate::spui::view::SpuiAssets>,
    mut spins: Query<&mut Transform, (With<TurntableSpin>, Without<TurntableCamera>)>,
    mut cameras: Query<(&TurntableCamera, &mut Transform), Without<TurntableSpin>>,
) {
    let open = fe.overlay == Some(Overlay::Sporepedia) && fe.shopper.is_none() && pedia.confirm.is_none();
    let entry = pedia.detail.and_then(|i| pedia.entries.get(i)).filter(|_| open);
    let want = entry.map(|e| e.thumb.clone());
    if want != table.shown {
        if let Some(root) = table.root.take() {
            commands.entity(root).despawn();
        }
        table.shown = want;
        table.frames = 0;
        table.failed = false;
        table.target = None;
        table.cpu = None;
        table.requested = None;
        table.pending = false;
        table.generation += 1;
        *view = View::default();
        let Some(entry) = entry else { return };
        if originals.resource_index.records.is_empty() {
            table.failed = true;
            return;
        }
        let (items, front) = match &entry.source {
            // ADAPTED: a baked model's front is not read; its card PNG's view
            // shows creatures facing -Z like the saved ones.
            thumbs::Source::Png(png) => (baked_items(*png, &mut originals, &mut meshes, &mut images, &mut materials), Vec3::NEG_Z),
            source => {
                let defaults = paint_data.map_or([0; 3], |d| d.defaults);
                match thumbs::items_for(source, &mut originals, &mut meshes, &mut images, &mut materials, defaults, block_paint.as_deref_mut()) {
                    Ok((items, front)) => (Ok(items), front),
                    Err(error) => (Err(error), Vec3::Z),
                }
            }
        };
        // The viewer's camera file (`editorSwatchCameraUI`), see the header.
        let camera_props =
            crate::creature_editor_data::camera(&originals.resource_index, crate::world_objects::name_id(swatch_camera_name(&entry.source)));
        let (heading, pitch, fov) = camera_props.map_or((-50.0, -8.6, DEFAULT_FOV), |p| (p.heading, p.pitch, p.fov));
        let camera = thumbs::ThumbCamera { heading, pitch, fov };
        let range = camera_props.as_ref().map_or(DEFAULT_ZOOM_RANGE, zoom_range);
        let creature = matches!(&entry.source, thumbs::Source::Creature(_)) || matches!(&entry.source, thumbs::Source::Stored(c) if c.is_creature());
        let Some((items, (centre, radius, aabb))) = items.ok().and_then(|items| {
            let b = thumbs::bounds(&items, &meshes, creature)?;
            Some((items, b))
        }) else {
            table.failed = true;
            return;
        };
        let target = images.add(Image::new_target_texture(SIZE[0], SIZE[1], TextureFormat::Rgba8UnormSrgb, None));
        let dir = thumbs::view_direction(front, camera);
        view.range = range;
        let fov = crate::creature_editor::retail_vertical_fov(camera.fov, SIZE[0] as f32 / SIZE[1] as f32);
        table.root = Some(spawn(&mut commands, &mut meshes, &mut materials, &items, centre, radius, aabb, dir, fov, range, target.clone()));
        table.target = Some(target);
        return;
    }
    if table.root.is_none() {
        return;
    }
    table.frames = table.frames.saturating_add(1);
    for mut t in &mut spins {
        t.rotation = Quat::from_rotation_y(view.yaw);
    }
    for (camera, mut t) in &mut cameras {
        t.translation = camera.dir * camera.distance / view.zoom;
    }
    // Read the render target back: first after the pipelines are up and a
    // few times while textures stream in, then whenever the view changed.
    let table = &mut *table;
    let incoming = table.inbox.lock().ok().map(|mut inbox| std::mem::take(&mut *inbox)).unwrap_or_default();
    for (generation, rgba) in incoming {
        table.pending = false;
        if generation != table.generation || rgba.len() != (SIZE[0] * SIZE[1] * 4) as usize {
            continue;
        }
        match &table.cpu {
            Some(handle) => {
                if let Some(mut image) = images.get_mut(handle) {
                    image.data = Some(rgba);
                }
            }
            None => {
                let handle = images.add(Image::new(
                    Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    rgba,
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                ));
                if let Some(name) = table.shown.as_deref() {
                    assets.insert_texture(image_key(name), crate::spui::view::Tex { handle: handle.clone(), size: SIZE });
                }
                table.cpu = Some(handle);
            }
        }
    }
    // A read-back that never arrives is given up after a while.
    if table.pending && table.frames > table.pending_at + 120 {
        table.pending = false;
    }
    let streaming = table.frames < READBACK_WARMUP * 4 && table.frames % READBACK_WARMUP == 0;
    if table.frames >= READBACK_WARMUP && !table.pending && (streaming || table.requested != Some(*view)) {
        if let Some(target) = table.target.clone() {
            table.pending = true;
            table.pending_at = table.frames;
            table.requested = Some(*view);
            let (inbox, generation) = (table.inbox.clone(), table.generation);
            commands.spawn(Screenshot::image(target)).observe(move |captured: On<ScreenshotCaptured>| {
                let rgba = captured.image.clone().try_into_dynamic().ok().map(|d| d.to_rgba8().into_raw());
                if let Ok(mut inbox) = inbox.lock() {
                    inbox.push((generation, rgba.unwrap_or_default()));
                }
            });
        }
    }
}

/// An installed creation's baked model (every surface with its atlas).
fn baked_items(
    png: ResourceKey,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Vec<thumbs::Item>, String> {
    let key = baked_key(png);
    if !originals.resource_index.records.contains_key(&key) {
        return Err("no baked model".into());
    }
    let model = crate::baked_model::ensure(originals, key, meshes, images, materials)?;
    Ok(model
        .surfaces
        .iter()
        .map(|s| thumbs::Item { mesh: s.handle.clone(), material: s.material.clone(), transform: Transform::IDENTITY })
        .collect())
}

/// The camera's near and far planes for a first-view distance `fit`, a turning
/// reach and the magnification range: the closest and farthest the zoom buttons
/// bring the camera (a range that lies on one side of the first view, the cell's,
/// still keeps the first view inside).
fn clip_planes(fit: f32, reach: f32, range: [f32; 2]) -> (f32, f32) {
    let (near, far) = (fit / range[1].max(1.0), fit / range[0].min(1.0));
    ((near - reach * 3.0).max(0.01), far + reach * 3.0)
}

/// The distance at which a creation turning about its axis stays inside a
/// frustum of vertical FOV `fov` and `aspect` (width / height): the cylinder
/// of its horizontal `reach` and `half_height`, so every turn fits.
pub fn fit_distance(reach: f32, half_height: f32, fov: f32, aspect: f32) -> f32 {
    let half_v = fov * 0.5;
    let half_h = (half_v.tan() * aspect).atan();
    let sphere = (reach * reach + half_height * half_height).sqrt() * 1.08;
    sphere / half_v.min(half_h).sin()
}

/// The scene: the stand, the creation centred on the turning node, the viewer
/// camera looking along `-dir` (the first view of the asset type), a key light.
#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    items: &[thumbs::Item],
    centre: Vec3,
    radius: f32,
    aabb: [Vec3; 2],
    dir: Vec3,
    fov: f32,
    range: [f32; 2],
    target: Handle<Image>,
) -> Entity {
    let layer = RenderLayers::layer(LAYER);
    // The turning box: its widest horizontal reach around the axis.
    let reach = (aabb[0] - centre).xz().length().max((aabb[1] - centre).xz().length()).max(radius * 0.5);
    let half_height = (aabb[1].y - aabb[0].y) * 0.5;
    let fit = fit_distance(reach, half_height, fov, SIZE[0] as f32 / SIZE[1] as f32);
    let stand_top = aabb[0].y - centre.y;
    let stand = meshes.add(Cylinder::new(reach * 1.05, reach * 0.06));
    let stand_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.3, 0.38),
        perceptual_roughness: 0.6,
        reflectance: 0.3,
        ..default()
    });
    let side = Vec3::Y.cross(dir).normalize_or(Vec3::X);
    let root = commands
        .spawn((Transform::from_translation(SCENE_AT), Visibility::Inherited, Name::new("sporepedia turntable")))
        .id();
    commands.entity(root).with_children(|parent| {
        parent
            .spawn((Transform::IDENTITY, Visibility::Inherited, TurntableSpin))
            .with_children(|spin| {
                for item in items {
                    let mut t = item.transform;
                    t.translation -= centre;
                    spin.spawn((Mesh3d(item.mesh.clone()), MeshMaterial3d(item.material.clone()), Transform::IDENTITY.mul_transform(t), layer.clone()));
                }
                spin.spawn((
                    Mesh3d(stand),
                    MeshMaterial3d(stand_material),
                    Transform::from_xyz(0.0, stand_top - reach * 0.03, 0.0),
                    layer.clone(),
                ));
            });
        parent.spawn((
            Camera3d::default(),
            Camera { order: -4, clear_color: ClearColorConfig::Custom(BACKGROUND), ..default() },
            RenderTarget::from(target),
            Projection::Perspective(PerspectiveProjection {
                fov,
                near: clip_planes(fit, reach, range).0,
                far: clip_planes(fit, reach, range).1,
                ..default()
            }),
            // A camera looking straight down (the cell view, pitch -90) takes the
            // creature's front as screen up.
            Transform::from_translation(dir * fit).looking_at(Vec3::ZERO, if dir.y.abs() > 0.999 { Vec3::NEG_Z } else { Vec3::Y }),
            TurntableCamera { dir, distance: fit },
            layer.clone(),
            AmbientLight { color: Color::srgb(0.85, 0.88, 1.0), brightness: 320.0, ..default() },
        ));
        parent.spawn((
            DirectionalLight { color: Color::srgb(1.0, 0.95, 0.86), illuminance: 6000.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation((dir + Vec3::Y * 0.8 - side * 0.6).normalize()).looking_at(Vec3::ZERO, Vec3::Y),
            layer.clone(),
        ));
    });
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_creations_find_their_baked_models() {
        let png = ResourceKey { group_id: 0x4062_6200, instance_id: 0x1234, type_id: thumbs::PNG_TYPE };
        let k = baked_key(png);
        assert_eq!((k.group_id, k.instance_id, k.type_id), (crate::baked_model::CREATURE_GROUP, 0x1234, crate::decode::TYPE_GMDL));
        assert_eq!(baked_key(ResourceKey { group_id: 0x4063_6200, ..png }).group_id, crate::baked_model::BUILDING_GROUP);
        assert_eq!(baked_key(ResourceKey { group_id: 0x4066_6200, ..png }).group_id, crate::baked_model::FLORA_GROUP);
        let mut t = Turntable::default();
        assert!(t.image("x").is_none());
        t.shown = Some("x".into());
        assert!(t.image("x").is_none(), "no picture before the first read-back");
        t.cpu = Some(Handle::default());
        assert!(t.image("x").is_some() && t.image("y").is_none());
        t.failed = true;
        assert!(t.image("x").is_none());
    }

    #[test]
    fn buttons_and_drag_turn_and_zoom_the_view() {
        let mut v = View::default();
        // Retail steps: 0.02 pi per update turn, 1 % per update zoom.
        v.rotate(ROTATE_STEP);
        assert!((v.yaw - 0.02 * std::f32::consts::PI).abs() < 1e-6);
        v.rotate(-2.0 * ROTATE_STEP);
        assert!(v.yaw > 6.0, "wraps below zero");
        v.zoom_by(ZOOM_STEP);
        assert!((v.zoom - 1.01).abs() < 1e-6);
        for _ in 0..2000 {
            v.zoom_by(ZOOM_STEP);
        }
        assert_eq!(v.zoom, v.range[1]);
        for _ in 0..2000 {
            v.zoom_by(-ZOOM_STEP);
        }
        assert_eq!(v.zoom, v.range[0]);
        assert!((DRAG_GAIN * 100.0 - 0.15).abs() < 1e-6);
    }

    #[test]
    fn the_fit_keeps_a_turning_creation_in_a_tall_window() {
        let aspect = SIZE[0] as f32 / SIZE[1] as f32;
        let fov = crate::creature_editor::retail_vertical_fov(DEFAULT_FOV, aspect);
        let d = fit_distance(1.0, 1.5, fov, aspect);
        let half_h = ((fov * 0.5).tan() * aspect).atan();
        // The widest turn (reach 1) is inside the horizontal half-FOV.
        assert!((1.0 / (d * d - 1.0).sqrt()).atan() < half_h);
        // A wider window needs no more distance.
        assert!(fit_distance(1.0, 1.5, fov, 2.0) <= d);
    }

    #[test]
    fn the_clip_planes_keep_the_first_view_inside_even_for_the_cells_range() {
        // Creature range 0.67 .. 2: the nearest zoom is half the fit.
        let (near, far) = clip_planes(10.0, 1.0, [0.67, 2.0]);
        assert!(near < 10.0 / 2.0 && far > 10.0 / 0.67);
        // Cell range 0.5 .. 0.667 (all below 1), a narrow lens: the first view (distance 30 x reach) is still in front of the near plane.
        let (near, far) = clip_planes(30.0, 1.0, [0.5, 0.667]);
        assert!(near < 30.0 - 1.0 && far > 30.0 / 0.5);
    }

    #[test]
    fn the_zoom_range_follows_the_camera_file() {
        use crate::creature_editor_data::EditorCameraProps;
        // EditorCameraCreatureUI: 5 in 2.5 .. 7.5.
        let creature = zoom_range(&EditorCameraProps::default());
        assert!((creature[0] - 5.0 / 7.5).abs() < 1e-6 && (creature[1] - 2.0).abs() < 1e-6);
        // EditorCameraCellSporePedia: 2 in 3 .. 4, so the range lies below the first view.
        let cell = zoom_range(&EditorCameraProps { zoom: 2.0, min_zoom: 3.0, max_zoom: 4.0, ..default() });
        assert!(cell[1] < 1.0 && (cell[0] - 0.5).abs() < 1e-6);
        let mut v = View { range: cell, ..default() };
        v.zoom_by(ZOOM_STEP);
        assert!((v.zoom - cell[1]).abs() < 1e-6, "the first press snaps into the range");
        // No camera data: the controller's defaults.
        assert_eq!(zoom_range(&EditorCameraProps { min_zoom: 0.0, ..default() }), DEFAULT_ZOOM_RANGE);
        let png = |group| thumbs::Source::Png(ResourceKey { group_id: group, instance_id: 1, type_id: thumbs::PNG_TYPE });
        assert_eq!(swatch_camera_name(&png(0x4063_6200)), "EditorCameraBuildingUI");
        assert_eq!(swatch_camera_name(&png(0x4061_6200)), "EditorCameraCellSporePedia");
        assert_eq!(swatch_camera_name(&png(0x4066_6200)), "EditorCameraFloraUI");
        assert_eq!(swatch_camera_name(&thumbs::Source::Creature(Default::default())), "EditorCameraCreatureUI");
    }
}
