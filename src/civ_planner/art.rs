//! The City Planner palette's installed art and the civic objects'
//! thumbnails.
//!
//! RETAIL (installed data: the planner palette PROPs of
//! `buttons_city_buildings~` 0xDCC387F5, `buttons_city_vehicle~` 0x96BEFF33
//! and their templates; the PNGs of `editors_planner~` 0x0355BA26):
//! * category tabs: `CIP_butn-section-{buldg,vhcl,civc,music,acc}` with the
//!   states `-norm` / `-hl` and, for the page shown, `-on-norm` / `-on-hl`
//!   (53 x 57; music 49 x 49);
//! * asset swatches: `paletteAssetSwatchBackgroundImage` (TemplateBuilding /
//!   TemplateVehicle 0x908F1077, the City Hall 0xED7BE52C, the turret and the
//!   Spaceship `pln_palette-asset-tile`), `paletteAssetSwatchHeaderIcon`
//!   (`header-icon-bldg-*`, `header-icon-vehicle-*`, `header-icon-defense`,
//!   `header-icon-ufo`), `paletteAssetSwatchNewIcon` (the four button states
//!   norm / off / hl / act of `CIP_button-new-{housing,entertain,industry,
//!   land,sea,air}`), and when locked `paletteAssetSwatchLockedIcon`
//!   (`header-icon-*-off`) on `paletteAssetSwatchLockedBackgroundImage`
//!   (`pln_palette-asset-tile`);
//! * civic objects carry no icon: the planner renders their models
//!   (`planner_page_thumbnails`, 49 civic objects), as done here.
//!
//! * a retail asset swatch shows the thumbnail of the design in its slot: the
//!   saved blocks of the class (`rigid_editor`) are drawn by a camera of their
//!   own (`design_thumbnails`) over the swatch's picture once the class was
//!   designed; before that the swatch shows its New icon.
//!
//! ADAPTED: the City Hall and the turret, which have no New icon of their own,
//! show their header icon and the turret's locked icon; the thumbnails (civic
//! objects and designs) are lit by a studio light of OpenSpore's choosing.

use std::collections::HashMap;

use bevy::{
    camera::{visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::render_resource::TextureFormat,
};

use crate::civ::{Building, VehicleDomain};
use crate::dbpf::ResourceKey;
use crate::game::RunProgress;
use crate::world_objects::name_id;
use crate::Originals;

/// `editors_planner~`.
pub const PLANNER_GROUP: u32 = 0x0355_BA26;
/// TemplateBuilding / TemplateVehicle `paletteAssetSwatchBackgroundImage`
/// (an unnamed PNG).
pub const SWATCH_BACKGROUND: u32 = 0x908F_1077;
/// The City Hall's `paletteAssetSwatchBackgroundImage` (unnamed).
pub const HALL_BACKGROUND: u32 = 0xED7B_E52C;
pub const ASSET_TILE: &str = "pln_palette-asset-tile";

/// One planner PNG: a registered name or a bare instance id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Art {
    Named(&'static str),
    Id(u32),
}

impl Art {
    pub fn instance(self) -> u32 {
        match self {
            Art::Named(name) => name_id(name),
            Art::Id(id) => id,
        }
    }

    pub fn key(self) -> ResourceKey {
        ResourceKey {
            type_id: crate::ui_art::UI_PNG_TYPE,
            group_id: PLANNER_GROUP,
            instance_id: self.instance(),
        }
    }
}

/// A button's states in `paletteAssetSwatchNewIcon` order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct States {
    pub norm: Art,
    pub off: Art,
    pub hl: Art,
    pub act: Art,
}

impl States {
    const fn new(names: [&'static str; 4]) -> Self {
        States {
            norm: Art::Named(names[0]),
            off: Art::Named(names[1]),
            hl: Art::Named(names[2]),
            act: Art::Named(names[3]),
        }
    }

    /// One picture for every state.
    pub const fn single(art: Art) -> Self {
        States { norm: art, off: art, hl: art, act: art }
    }

    pub fn all(&self) -> [Art; 4] {
        [self.norm, self.off, self.hl, self.act]
    }
}

pub const NEW_HOUSING: States = States::new([
    "CIP_button-new-housing-norm",
    "CIP_button-new-housing-off",
    "CIP_button-new-housing-hl",
    "CIP_button-new-housing-act",
]);
pub const NEW_ENTERTAIN: States = States::new([
    "CIP_button-new-entertain-norm",
    "CIP_button-new-entertain-off",
    "CIP_button-new-entertain-hl",
    "CIP_button-new-entertain-act",
]);
pub const NEW_INDUSTRY: States = States::new([
    "CIP_button-new-industry-norm",
    "CIP_button-new-industry-off",
    "CIP_button-new-industry-hl",
    "CIP_button-new-industry-act",
]);
pub const NEW_LAND: States = States::new([
    "CIP_button-new-land-norm",
    "CIP_button-new-land-off",
    "CIP_button-new-land-hl",
    "CIP_button-new-land-act",
]);
pub const NEW_SEA: States = States::new([
    "CIP_button-new-sea-norm",
    "CIP_button-new-sea-off",
    "CIP_button-new-sea-hl",
    "CIP_button-new-sea-act",
]);
pub const NEW_AIR: States = States::new([
    "CIP_button-new-air-norm",
    "CIP_button-new-air-off",
    "CIP_button-new-air-hl",
    "CIP_button-new-air-act",
]);

/// An asset swatch's art.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Swatch {
    pub background: Art,
    pub header: Art,
    /// The swatch's picture (its New icon; see the module notes).
    pub picture: States,
    /// Shown when the swatch is locked, on `locked_background`.
    pub locked: Option<Art>,
    pub locked_background: Art,
}

/// The Buildings page's swatches (`None`: the City Hall).
pub fn building_swatch(building: Option<Building>) -> Swatch {
    let tile = Art::Named(ASSET_TILE);
    match building {
        None => Swatch {
            background: Art::Id(HALL_BACKGROUND),
            header: Art::Named("header-icon-bldg-cityhall"),
            picture: States::single(Art::Named("header-icon-bldg-cityhall")),
            locked: None,
            locked_background: tile,
        },
        Some(Building::House) => Swatch {
            background: Art::Id(SWATCH_BACKGROUND),
            header: Art::Named("header-icon-bldg-housing"),
            picture: NEW_HOUSING,
            locked: Some(Art::Named("header-icon-bldg-housing-off")),
            locked_background: tile,
        },
        Some(Building::Entertainment) => Swatch {
            background: Art::Id(SWATCH_BACKGROUND),
            header: Art::Named("header-icon-bldg-entrtnmt"),
            picture: NEW_ENTERTAIN,
            locked: Some(Art::Named("header-icon-bldg-entrtnmt-off")),
            locked_background: tile,
        },
        Some(Building::Factory) => Swatch {
            background: Art::Id(SWATCH_BACKGROUND),
            header: Art::Named("header-icon-bldg-factory"),
            picture: NEW_INDUSTRY,
            locked: Some(Art::Named("header-icon-bldg-factory-off")),
            locked_background: tile,
        },
    }
}

/// `Military_turret` (the city type's turret; not editable).
pub fn turret_swatch() -> Swatch {
    Swatch {
        background: Art::Named(ASSET_TILE),
        header: Art::Named("header-icon-defense"),
        picture: States::single(Art::Named("header-icon-bldg-defense")),
        locked: Some(Art::Named("header-icon-bldg-defense")),
        locked_background: Art::Named(ASSET_TILE),
    }
}

/// The Vehicles page's swatches (`None`: the Spaceship).
pub fn vehicle_swatch(domain: Option<VehicleDomain>) -> Swatch {
    let tile = Art::Named(ASSET_TILE);
    let (header, picture, locked) = match domain {
        Some(VehicleDomain::Land) => ("header-icon-vehicle-land", NEW_LAND, "header-icon-vehicle-land-off"),
        Some(VehicleDomain::Sea) => ("header-icon-vehicle-sea", NEW_SEA, "header-icon-vehicle-sea-off"),
        Some(VehicleDomain::Air) => ("header-icon-vehicle-air", NEW_AIR, "header-icon-vehicle-air-off"),
        // The Spaceship's own PROP lists the sea New icon; it is locked
        // throughout Civ.
        None => ("header-icon-ufo", NEW_SEA, "header-icon-vehicle-ufo-off"),
    };
    Swatch {
        background: if domain.is_some() { Art::Id(SWATCH_BACKGROUND) } else { tile },
        header: Art::Named(header),
        picture,
        locked: Some(Art::Named(locked)),
        locked_background: tile,
    }
}

/// A category tab's art: (normal, shown page) x (idle, hovered).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tab {
    pub norm: Art,
    pub hl: Art,
    pub on: Art,
    pub on_hl: Art,
}

impl Tab {
    /// In `tab_index` order.
    pub fn all(&self) -> [Art; 4] {
        [self.norm, self.hl, self.on, self.on_hl]
    }
}

/// The state shown of a button (`States::all` order): disabled, pressed /
/// armed, hovered, normal.
pub fn button_index(off: bool, active: bool, hovered: bool) -> usize {
    if off {
        1
    } else if active {
        3
    } else if hovered {
        2
    } else {
        0
    }
}

/// The state shown of a tab (`Tab::all` order).
pub fn tab_index(on: bool, hovered: bool) -> usize {
    match (on, hovered) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    }
}

/// `editors_planner~!CIP_butn-section-*` by page.
pub fn tab_art(page: super::PlannerPage) -> Tab {
    use super::PlannerPage::*;
    let names: [&'static str; 4] = match page {
        Buildings => [
            "CIP_butn-section-buldg-norm",
            "CIP_butn-section-buldg-hl",
            "CIP_butn-section-buldg-on-norm",
            "CIP_butn-section-buldg-on-hl",
        ],
        Vehicles => [
            "CIP_butn-section-vhcl-norm",
            "CIP_butn-section-vhcl-hl",
            "CIP_butn-section-vhcl-on-norm",
            "CIP_butn-section-vhcl-on-hl",
        ],
        Decorations => [
            "CIP_butn-section-civc-norm",
            "CIP_butn-section-civc-hl",
            "CIP_butn-section-civc-on-norm",
            "CIP_butn-section-civc-on-hl",
        ],
        Music => [
            "CIP_butn-section-music-norm",
            "CIP_butn-section-music-hl",
            "CIP_butn-section-music-on-norm",
            "CIP_butn-section-music-on-hl",
        ],
        Outfits => [
            "CIP_butn-section-acc-norm",
            "CIP_butn-section-acc-hl",
            "CIP_butn-section-acc-on-norm",
            "CIP_butn-section-acc-on-hl",
        ],
    };
    Tab {
        norm: Art::Named(names[0]),
        hl: Art::Named(names[1]),
        on: Art::Named(names[2]),
        on_hl: Art::Named(names[3]),
    }
}

/// The planner PNGs imported so far (by instance; `None`: not installed).
#[derive(Resource, Default)]
pub struct PlannerArt {
    images: HashMap<u32, Option<Handle<Image>>>,
}

impl PlannerArt {
    /// The image of `art`, imported on first use (`None` without the install).
    pub fn image(&mut self, originals: &Originals, images: &mut Assets<Image>, art: Art) -> Option<Handle<Image>> {
        if originals.resource_index.records.is_empty() {
            return None;
        }
        self.images
            .entry(art.instance())
            .or_insert_with(|| match crate::ui_art::import_png(&originals.resource_index, art.key(), images) {
                Ok((image, _, _)) => Some(image),
                Err(e) => {
                    warn!("planner art {art:?}: {e}");
                    None
                }
            })
            .clone()
    }
}

/// A palette image with states, swapped by `animate`.
#[derive(Component, Clone)]
pub enum ArtState {
    /// A swatch picture: off when unaffordable / locked, active when armed.
    Button { states: [Option<Handle<Image>>; 4], off: bool, active: bool },
    /// A category tab.
    Tab { states: [Option<Handle<Image>>; 4], on: bool },
}

/// Pick each stateful image by the pointer (hover) and its flags.
pub fn animate(
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    mut nodes: Query<(Entity, &ArtState, &mut ImageNode)>,
) {
    let hovered = |e: Entity| hover.as_ref().is_some_and(|h| h.0.values().any(|hits| hits.contains_key(&e)));
    for (entity, state, mut node) in &mut nodes {
        let over = hovered(entity);
        let want = match state {
            // norm, off, hl, act
            ArtState::Button { states, off, active } => {
                states[button_index(*off, *active, over)].clone().or_else(|| states[0].clone())
            }
            ArtState::Tab { states, on } => states[tab_index(*on, over)].clone().or_else(|| states[0].clone()),
        };
        if let Some(image) = want {
            if node.image != image {
                node.image = image;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Civic object thumbnails
// ---------------------------------------------------------------------------

/// A free render layer for the thumbnail scenes (see the layer list in
/// `part_thumbnails` / `mechanical_preview`).
const THUMB_LAYER: usize = 21;
const THUMB_SIZE: u32 = 96;
/// Frames a thumbnail camera renders before it stops (pipelines and
/// textures prepare asynchronously).
const THUMB_FRAMES: u32 = 45;
const THUMB_SPACING: f32 = 12.0;
/// Away from the other thumbnail scenes.
const THUMB_ORIGIN: Vec3 = Vec3::new(-2000.0, -2000.0, 0.0);

/// A civic object's thumbnail: the render target and its scene while it
/// renders.
struct Thumb {
    image: Handle<Image>,
    scene: Option<Entity>,
    camera: Option<Entity>,
    frames: u32,
    failed: bool,
}

#[derive(Resource, Default)]
pub struct DecorThumbs {
    thumbs: HashMap<u8, Thumb>,
    light: Option<Entity>,
}

impl DecorThumbs {
    /// The thumbnail of civic object `item`, once its scene exists.
    pub fn image(&self, item: u8) -> Option<Handle<Image>> {
        self.thumbs.get(&item).filter(|t| !t.failed).map(|t| t.image.clone())
    }
}

/// Marks a palette image node that shows civic object `0`'s thumbnail.
#[derive(Component, Clone, Copy)]
pub struct DecorThumb(pub u8);

/// Render the civic objects' thumbnails while the Decorations page is open
/// (each once: the camera stops after `THUMB_FRAMES`; the scenes go when the
/// page closes, the images stay).
#[allow(clippy::too_many_arguments)]
pub fn decor_thumbnails(
    mut commands: Commands,
    planner: Res<super::CivPlanner>,
    mut thumbs: ResMut<DecorThumbs>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera>,
    mut nodes: Query<(&DecorThumb, &mut ImageNode, &mut Visibility)>,
) {
    let want = planner.is_open() && planner.page == super::PlannerPage::Decorations;
    let thumbs = &mut *thumbs;
    if !want || originals.resource_index.records.is_empty() {
        for thumb in thumbs.thumbs.values_mut() {
            if let Some(scene) = thumb.scene.take() {
                commands.entity(scene).despawn();
            }
            thumb.camera = None;
        }
        if let Some(light) = thumbs.light.take() {
            commands.entity(light).despawn();
        }
        return;
    }
    let layer = RenderLayers::layer(THUMB_LAYER);
    for item in 0..crate::civ_decor::ITEMS.len() as u8 {
        if thumbs.thumbs.contains_key(&item) {
            continue;
        }
        let image = images.add(Image::new_target_texture(THUMB_SIZE, THUMB_SIZE, TextureFormat::Rgba8UnormSrgb, None));
        let mut thumb = Thumb { image: image.clone(), scene: None, camera: None, frames: 0, failed: false };
        let model = crate::civ_decor::item_key(item as usize).and_then(|key| {
            let name = crate::civ_decor::ITEMS[item as usize].0;
            let definition = crate::world_objects::definition_at(&originals, key, name).ok()?;
            crate::world_objects::import_at(&mut originals, definition, &mut meshes, &mut images, &mut materials)
                .map_err(|e| warn!("civic object thumbnail {name}: {e}"))
                .ok()
                .map(|(model, _)| model)
        });
        let bounds = model.as_ref().and_then(|m| bounds(&meshes, &m.handle));
        match (model, bounds) {
            (Some(model), Some((center, extent))) => {
                let fit = 1.6 / extent.max(1e-3);
                let rotation = Quat::from_euler(EulerRot::YXZ, -0.5, 0.0, 0.0);
                let root = commands
                    .spawn((
                        Transform::from_translation(THUMB_ORIGIN + Vec3::X * THUMB_SPACING * item as f32),
                        Visibility::Inherited,
                        Name::new(format!("civic object thumbnail {item}")),
                    ))
                    .id();
                let mut camera = Entity::PLACEHOLDER;
                commands.entity(root).with_children(|parent| {
                    camera = parent
                        .spawn((
                            Camera3d::default(),
                            Camera { order: -2, clear_color: Color::NONE.into(), ..default() },
                            RenderTarget::from(image.clone()),
                            Projection::Perspective(PerspectiveProjection {
                                fov: 34_f32.to_radians(),
                                near: 0.05,
                                far: 8.0,
                                ..default()
                            }),
                            Transform::from_xyz(0.0, 1.1, 3.2).looking_at(Vec3::ZERO, Vec3::Y),
                            layer.clone(),
                            AmbientLight { color: Color::srgb(0.85, 0.88, 1.0), brightness: 220.0, ..default() },
                        ))
                        .id();
                    let model_transform = Transform {
                        translation: -(rotation * center) * fit,
                        rotation,
                        scale: Vec3::splat(fit),
                    };
                    for surface in &model.surfaces {
                        parent.spawn((
                            // A drawn surface, not a model root (see `part_thumbnails`): keeps
                            // `material_binding::attach_remaining` from copying the model's
                            // other surfaces into the world with no render layer.
                            crate::material_binding::OriginalSurfaceChild::of(surface),
                            Mesh3d(surface.handle.clone()),
                            MeshMaterial3d(surface.material.clone()),
                            model_transform,
                            layer.clone(),
                        ));
                    }
                });
                thumb.scene = Some(root);
                thumb.camera = Some(camera);
            }
            _ => thumb.failed = true,
        }
        thumbs.thumbs.insert(item, thumb);
    }
    if thumbs.light.is_none() && thumbs.thumbs.values().any(|t| t.scene.is_some()) {
        thumbs.light = Some(
            commands
                .spawn((
                    DirectionalLight {
                        color: Color::srgb(1.0, 0.94, 0.84),
                        illuminance: 5200.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(-1.8, 2.6, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
                    layer.clone(),
                ))
                .id(),
        );
    }
    // Each camera renders its warm-up frames, then rests.
    for thumb in thumbs.thumbs.values_mut() {
        let Some(camera) = thumb.camera else { continue };
        thumb.frames = thumb.frames.saturating_add(1);
        if let Ok(mut camera) = cameras.get_mut(camera) {
            let active = thumb.frames <= THUMB_FRAMES;
            if camera.is_active != active {
                camera.is_active = active;
            }
        }
    }
    for (thumb, mut node, mut visibility) in &mut nodes {
        match thumbs.image(thumb.0) {
            Some(image) => {
                if node.image != image {
                    node.image = image;
                }
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

/// Centre and largest extent of a mesh's vertices.
fn bounds(meshes: &Assets<Mesh>, handle: &Handle<Mesh>) -> Option<(Vec3, f32)> {
    let mesh = meshes.get(handle)?;
    let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
        return None;
    };
    let (min, max) = positions.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
        let p = Vec3::from_array(*p);
        (lo.min(p), hi.max(p))
    });
    let extent = (max - min).max_element();
    (extent.is_finite() && extent > 0.0).then_some(((min + max) * 0.5, extent))
}

// ---------------------------------------------------------------------------
// Design thumbnails (the swatches of the Buildings and Vehicles pages)
// ---------------------------------------------------------------------------

/// The design slots of the swatches: 0 the City Hall, 1 house, 2 entertainment,
/// 3 factory, 4 / 5 / 6 the land / sea / air vehicle of the planned city's type.
pub const DESIGN_SLOTS: u8 = 7;

pub fn building_slot(building: Option<Building>) -> u8 {
    match building {
        None => 0,
        Some(Building::House) => 1,
        Some(Building::Entertainment) => 2,
        Some(Building::Factory) => 3,
    }
}

pub fn vehicle_slot(domain: VehicleDomain) -> u8 {
    match domain {
        VehicleDomain::Land => 4,
        VehicleDomain::Sea => 5,
        VehicleDomain::Air => 6,
    }
}

/// The saved design a slot shows and the part group of its blocks (`None`
/// until the class has been designed: the swatch then shows its New icon).
fn slot_design(run: &RunProgress, slot: u8, approach: crate::civ::Approach) -> Option<(u32, &[crate::game::VehiclePart])> {
    use crate::creature_editor_data::SetupKind;
    let has_blocks = |parts: &[crate::game::VehiclePart]| parts.iter().any(|p| p.block.is_some());
    let class = |kind: SetupKind| run.rigid.design(kind).map(|d| d.parts.as_slice()).filter(|p| has_blocks(p));
    let building = crate::asset_catalog::BUILDING_PARTS;
    match slot {
        // The hall's design is also the stage's own record (`RunProgress::building`).
        0 => class(SetupKind::BuildingCityHall)
            .or_else(|| Some(run.building.parts.as_slice()).filter(|p| has_blocks(p)))
            .map(|p| (building, p)),
        1 => class(SetupKind::BuildingHouse).map(|p| (building, p)),
        2 => class(SetupKind::BuildingEntertainment).map(|p| (building, p)),
        3 => class(SetupKind::BuildingFactory).map(|p| (building, p)),
        4..=6 => {
            let domain = match slot {
                4 => VehicleDomain::Land,
                5 => VehicleDomain::Sea,
                _ => VehicleDomain::Air,
            };
            run.rigid
                .vehicle_design(approach, domain)
                .map(|d| (crate::asset_catalog::VEHICLE_PARTS, d.parts.as_slice()))
                .filter(|(_, p)| has_blocks(p))
        }
        _ => None,
    }
}

/// A hash of what a slot's design looks like (the thumbnail is drawn again
/// when it changes).
fn design_signature(parts: &[crate::game::VehiclePart]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for p in parts {
        for bits in [p.instance, p.scale.to_bits(), crate::rigid_editor::block_signature(&p.block)] {
            hash ^= bits;
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash | 1
}

/// Marks a palette image node that shows the design thumbnail of slot `0`.
#[derive(Component, Clone, Copy)]
pub struct DesignThumb(pub u8);

struct DesignScene {
    image: Handle<Image>,
    signature: u32,
    scene: Option<Entity>,
    camera: Option<Entity>,
    frames: u32,
}

#[derive(Resource, Default)]
pub struct DesignThumbs {
    thumbs: HashMap<u8, DesignScene>,
    light: Option<Entity>,
}

/// Between the slots' scenes (each far from the others and from the civic
/// object scenes, so no thumbnail camera sees another's models).
const DESIGN_ORIGIN: Vec3 = Vec3::new(-2000.0, -2600.0, 0.0);
const DESIGN_SPACING: f32 = 500.0;
/// The warm-up frames before a finished thumbnail is shown.
const DESIGN_SHOW_FRAME: u32 = 6;

/// Axis-aligned box of a mesh's vertices.
fn mesh_box(meshes: &Assets<Mesh>, handle: &Handle<Mesh>) -> Option<(Vec3, Vec3)> {
    let mesh = meshes.get(handle)?;
    let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
        return None;
    };
    let (lo, hi) = positions.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
        (lo.min(Vec3::from_array(*p)), hi.max(Vec3::from_array(*p)))
    });
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// Render the planner's design swatches: the retail swatch shows the
/// thumbnail of the design in its slot (`paletteAssetSwatch`), here the saved
/// blocks drawn by a camera of their own (the civic objects' method). A slot
/// whose class was designed is drawn while the planner is open and again when
/// the design changes. ADAPTED: the studio light, the three-quarter view and
/// the 96 pixel size are OpenSpore's.
#[allow(clippy::too_many_arguments)]
pub fn design_thumbnails(
    mut commands: Commands,
    planner: Res<super::CivPlanner>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut thumbs: ResMut<DesignThumbs>,
    meshes: Res<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera>,
    mut nodes: Query<(&DesignThumb, &ChildOf, &mut ImageNode, &mut Visibility), Without<ArtState>>,
    mut pictures: Query<&mut ImageNode, With<ArtState>>,
) {
    let thumbs = &mut *thumbs;
    let city = planner.city.filter(|_| planner.is_open()).and_then(|c| run.civ_campaign.cities.get(c));
    let (Some(city), false) = (city, originals.resource_index.records.is_empty()) else {
        for thumb in thumbs.thumbs.values_mut() {
            if let Some(scene) = thumb.scene.take() {
                commands.entity(scene).despawn();
            }
            thumb.camera = None;
            thumb.signature = 0;
        }
        if let Some(light) = thumbs.light.take() {
            commands.entity(light).despawn();
        }
        return;
    };
    let layer = RenderLayers::layer(THUMB_LAYER);
    for slot in 0..DESIGN_SLOTS {
        let design = slot_design(&run, slot, city.kind);
        let signature = design.map_or(0, |(_, parts)| design_signature(parts));
        let known = thumbs.thumbs.get(&slot).map_or(0, |t| t.signature);
        if signature == known {
            continue;
        }
        // The design changed (or went): the old scene goes.
        if let Some(thumb) = thumbs.thumbs.get_mut(&slot) {
            if let Some(scene) = thumb.scene.take() {
                commands.entity(scene).despawn();
            }
            thumb.camera = None;
            thumb.signature = 0;
            thumb.frames = 0;
        }
        let Some((group, parts)) = design else { continue };
        // Every block with its model (a model that is not imported yet: try
        // again next frame).
        let mut blocks = Vec::new();
        let mut complete = true;
        for p in parts {
            let Some(block) = &p.block else { continue };
            match crate::rigid_editor::block_model(&originals, group, p.definition_id, p.instance, &block.handles) {
                Some(model) => {
                    let unit = crate::rigid_editor::editor_unit(&model) * p.scale.clamp(0.1, 4.0);
                    blocks.push((model, block.transform(unit)));
                }
                None => complete = false,
            }
        }
        if !complete || blocks.is_empty() {
            continue;
        }
        // The design's box in editor space.
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for (model, transform) in &blocks {
            let Some((a, b)) = mesh_box(&meshes, &model.handle) else { continue };
            for i in 0..8 {
                let corner = Vec3::new(if i & 1 == 0 { a.x } else { b.x }, if i & 2 == 0 { a.y } else { b.y }, if i & 4 == 0 { a.z } else { b.z });
                let at = transform.transform_point(corner);
                lo = lo.min(at);
                hi = hi.max(at);
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            continue;
        }
        let centre = (lo + hi) * 0.5;
        // The design fills the picture: its largest side, not the box's diagonal.
        let radius = ((hi - lo).max_element() * 0.62).max(0.05);
        let fov = 34_f32.to_radians();
        let distance = radius / (fov * 0.5).sin() * 1.08;
        let direction = Vec3::new(0.62, 0.5, 1.0).normalize();
        let image = match thumbs.thumbs.get(&slot) {
            Some(t) => t.image.clone(),
            None => images.add(Image::new_target_texture(THUMB_SIZE, THUMB_SIZE, TextureFormat::Rgba8UnormSrgb, None)),
        };
        let root = commands
            .spawn((
                Transform::from_translation(DESIGN_ORIGIN + Vec3::X * DESIGN_SPACING * slot as f32),
                Visibility::Inherited,
                Name::new(format!("planner design thumbnail {slot}")),
            ))
            .id();
        let mut camera = Entity::PLACEHOLDER;
        commands.entity(root).with_children(|parent| {
            camera = parent
                .spawn((
                    Camera3d::default(),
                    Camera { order: -2, clear_color: Color::NONE.into(), ..default() },
                    RenderTarget::from(image.clone()),
                    Projection::Perspective(PerspectiveProjection { fov, near: distance * 0.05, far: distance * 4.0, ..default() }),
                    Transform::from_translation(centre + direction * distance).looking_at(centre, Vec3::Y),
                    layer.clone(),
                    AmbientLight { color: Color::srgb(0.85, 0.88, 1.0), brightness: 260.0, ..default() },
                ))
                .id();
            for (model, transform) in &blocks {
                for surface in &model.surfaces {
                    parent.spawn((
                        // A drawn surface, not a model root (as the civic objects').
                        crate::material_binding::OriginalSurfaceChild::of(surface),
                        Mesh3d(surface.handle.clone()),
                        MeshMaterial3d(surface.material.clone()),
                        *transform,
                        layer.clone(),
                    ));
                }
            }
        });
        thumbs.thumbs.insert(slot, DesignScene { image, signature, scene: Some(root), camera: Some(camera), frames: 0 });
    }
    if thumbs.light.is_none() && thumbs.thumbs.values().any(|t| t.scene.is_some()) {
        thumbs.light = Some(
            commands
                .spawn((
                    DirectionalLight {
                        color: Color::srgb(1.0, 0.94, 0.84),
                        illuminance: 5200.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(-1.8, 2.6, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
                    layer.clone(),
                ))
                .id(),
        );
    }
    // Each camera renders its warm-up frames, then rests.
    for thumb in thumbs.thumbs.values_mut() {
        let Some(camera) = thumb.camera else { continue };
        thumb.frames = thumb.frames.saturating_add(1);
        if let Ok(mut camera) = cameras.get_mut(camera) {
            let active = thumb.frames <= THUMB_FRAMES;
            if camera.is_active != active {
                camera.is_active = active;
            }
        }
    }
    for (thumb, parent, mut node, mut visibility) in &mut nodes {
        let shown = thumbs.thumbs.get(&thumb.0).filter(|t| t.signature != 0 && t.frames >= DESIGN_SHOW_FRAME);
        // The thumbnail replaces the New icon under it.
        if let Ok(mut picture) = pictures.get_mut(parent.parent()) {
            let want = if shown.is_some() { Color::NONE } else { Color::WHITE };
            if picture.color != want {
                picture.color = want;
            }
        }
        match shown {
            Some(t) => {
                if node.image != t.image {
                    node.image = t.image.clone();
                }
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ_planner::PlannerPage;

    #[test]
    fn a_slot_shows_its_class_design_once_the_class_was_designed() {
        use crate::creature_editor_data::SetupKind;
        use crate::game::VehiclePart;
        use crate::rigid_editor::{RigidBlock, RigidDesign};
        let mut run = RunProgress::default();
        let approach = crate::civ::Approach::Military;
        assert!(slot_design(&run, 1, approach).is_none());
        let mut part = VehiclePart::new(7, 0);
        part.block = Some(RigidBlock::at(Vec3::new(0.0, 1.0, 0.0)));
        run.rigid.designs.insert(SetupKind::BuildingHouse.setup_name().into(), RigidDesign { name: String::new(), parts: vec![part.clone()] });
        let (group, parts) = slot_design(&run, 1, approach).expect("the house design");
        assert_eq!((group, parts.len()), (crate::asset_catalog::BUILDING_PARTS, 1));
        assert!(slot_design(&run, 2, approach).is_none() && slot_design(&run, 4, approach).is_none());
        // A design without free blocks (an old slot record) shows no thumbnail.
        let mut old = part.clone();
        old.block = None;
        run.rigid.designs.insert(SetupKind::BuildingFactory.setup_name().into(), RigidDesign { name: String::new(), parts: vec![old] });
        assert!(slot_design(&run, 3, approach).is_none());
        // The hall also reads the stage's own record.
        run.building.parts = vec![part.clone()];
        assert!(slot_design(&run, 0, approach).is_some());
        // A vehicle slot follows the planned city's type.
        run.rigid.designs.insert(
            crate::rigid_editor::vehicle_class(approach, VehicleDomain::Sea).setup_name().into(),
            RigidDesign { name: String::new(), parts: vec![part.clone()] },
        );
        assert_eq!(slot_design(&run, vehicle_slot(VehicleDomain::Sea), approach).map(|d| d.0), Some(crate::asset_catalog::VEHICLE_PARTS));
        assert!(slot_design(&run, vehicle_slot(VehicleDomain::Sea), crate::civ::Approach::Economic).is_none());
        // The signature follows the blocks.
        let mut moved = part.clone();
        moved.block = Some(RigidBlock::at(Vec3::new(0.0, 2.0, 0.0)));
        assert_ne!(design_signature(&[part]), design_signature(&[moved]));
        assert_eq!([building_slot(None), building_slot(Some(Building::Factory)), vehicle_slot(VehicleDomain::Air)], [0, 3, 6]);
    }

    #[test]
    fn swatches_name_the_installed_planner_art() {
        assert_eq!(Art::Named("header-icon-bldg-housing").instance(), 0x129A_1E2A);
        assert_eq!(Art::Named(ASSET_TILE).instance(), 0xD20F_D1D3);
        assert_eq!(NEW_LAND.norm.instance(), 0x15CE_FCDA);
        assert_eq!(NEW_HOUSING.act.instance(), 0x9E92_69E6);
        assert_eq!(tab_art(PlannerPage::Buildings).norm.instance(), 0x41B5_733F);
        assert_eq!(tab_art(PlannerPage::Vehicles).on.instance(), 0x1F40_C264);
        assert_eq!(building_swatch(None).background, Art::Id(HALL_BACKGROUND));
        assert_eq!(building_swatch(Some(Building::Factory)).picture, NEW_INDUSTRY);
        assert_eq!(vehicle_swatch(Some(VehicleDomain::Air)).locked, Some(Art::Named("header-icon-vehicle-air-off")));
        assert_eq!(vehicle_swatch(None).background, Art::Named(ASSET_TILE));
        assert_eq!(turret_swatch().header.instance(), 0x6D32_49E3);
        assert_eq!(NEW_SEA.all()[button_index(false, false, true)], NEW_SEA.hl);
        assert_eq!(NEW_SEA.all()[button_index(true, true, true)], NEW_SEA.off);
        assert_eq!(tab_art(PlannerPage::Music).all()[tab_index(true, true)].instance(), 0x7029_BADC);
        assert_eq!(Art::Id(SWATCH_BACKGROUND).key().group_id, PLANNER_GROUP);
    }
}
