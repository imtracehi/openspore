//! Original-data part palette, lazy mesh/texture import and persistent assembly.
use crate::{
    asset_catalog::{self, Capabilities, PartDefinition},
    game::{self, Health, Hunger, Mover, PartInstance, Player, RunProgress},
    stages::{GameStage, StageOwned},
    texture, OriginalModel, Originals,
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};

use bevy::{ui::Pressed, ui_widgets::Button};

/// Palette tabs as (label, catalog category). Spore's Cell creator only has
/// the four cell part families; the Creature creator follows the native
/// `creature_editor_palette_categories` order.
const CELL_CATEGORIES: &[(&str, &str)] = &[
    ("Mouths", "Mouths"),
    ("Movement", "Movement"),
    ("Weapons", "Weapons"),
    ("Perception", "Eyes"),
];
const CREATURE_CATEGORIES: &[(&str, &str)] = &[
    ("Mouths", "Mouths"),
    ("Eyes & Senses", "Eyes"),
    ("Graspers", "Hands"),
    ("Feet", "Feet"),
    ("Arms & Legs", "Movement"),
    ("Weapons", "Weapons"),
    ("Details", "Details"),
];
fn categories(stage: &GameStage) -> &'static [(&'static str, &'static str)] {
    if *stage == GameStage::Cell {
        CELL_CATEGORIES
    } else {
        CREATURE_CATEGORIES
    }
}
#[derive(Resource, Default)]
pub struct EditorState {
    pub open: bool,
    category: usize,
    page: usize,
    previous: Option<GameStage>,
    pub message: String,
    dirty: bool,
    pub visuals_dirty: bool,
    last_parts: Vec<PartInstance>,
    last_body: Option<crate::body::BodyProfile>,
    body_mode: bool,
    focused_spine: usize,
    undo: Vec<RunProgress>,
    redo: Vec<RunProgress>,
    drag: Option<PartDrag>,
    /// A part bought from the palette that follows the cursor until release.
    carry: Option<PartCarry>,
    scroll_offset: f32,
    scene_open: bool,
    orbit_yaw: f32,
    orbit_height: f32,
    orbit_zoom: f32,
    asymmetric: bool,
    free_placement: bool,
    /// The Cell creator looks down onto the cell like the Cell game does.
    cell_view: bool,
    /// Session-only name shown in the creator's name field.
    name: Option<String>,
    name_seed: u32,
    /// Actions queued by another creator UI (`creature_editor_ui`), applied
    /// like pressed palette buttons.
    pub queued: Vec<Action>,
    /// Screen regions of that UI (palette, top, bottom, abilities) for
    /// `drag`'s palette drop and UI tests.
    pub(crate) retail_layout: Option<CreatorLayout>,
    /// The first `locked_parts` entries of the part list belong to the
    /// creature itself and cannot be picked, scaled or sold (the outfitters,
    /// `editorOnlyEditFromPalette`; `outfitter`).
    pub locked_parts: usize,
    /// A rigid-block session (vehicle / building / UFO editor,
    /// `rigid_editor`) owns the part list: the creature editor's input,
    /// drag and part drawing leave it alone.
    pub rigid: bool,
}
struct PartDrag {
    part: usize,
    spine: Option<usize>,
    point: Vec3,
    normal: Vec3,
    grab: Vec3,
    before: RunProgress,
}
/// Spore-style palette drag: the purchase happens on press and the new part
/// rides the cursor onto the body. `before` is the pre-purchase state, so a
/// drop back onto the palette restores it exactly, as one undo step would.
struct PartCarry {
    part: usize,
    before: RunProgress,
    left_palette: bool,
}
impl EditorState {
    fn enter_stage(&mut self, stage: &GameStage) {
        if self.previous.as_ref() == Some(stage) {
            return;
        }
        self.previous = Some(stage.clone());
        self.open = false;
        self.dirty = true;
        self.visuals_dirty = true;
        self.last_parts.clear();
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.carry = None;
    }
    pub fn body_editing(&self) -> bool {
        self.open && self.body_mode
    }
    pub fn camera_offset(&self) -> Vec3 {
        if self.cell_view {
            // A steep three-quarter view onto the cell's swimming plane, so
            // mouth, eyes and flagella read as they do in Spore's Cell creator.
            let distance = (5.6 + self.orbit_zoom).clamp(2.5, 14.);
            let pitch = (1.3 + self.orbit_height * 0.12).clamp(0.45, 1.5);
            // `camera_follow` aims 0.8 units to the camera's right; seen from
            // this high that rolls the picture. Turning the orbit by the same
            // angle keeps the cell's heading pointing straight up the screen.
            let yaw = self.orbit_yaw + (0.8 / (pitch.cos() * distance)).atan();
            return Vec3::new(
                yaw.sin() * pitch.cos() * distance,
                pitch.sin() * distance,
                yaw.cos() * pitch.cos() * distance,
            );
        }
        let yaw = 0.67 + self.orbit_yaw;
        let distance = (6.4 + self.orbit_zoom).clamp(3., 15.);
        Vec3::new(
            yaw.sin() * distance,
            (2.8 + self.orbit_height).clamp(0.3, 8.),
            -yaw.cos() * distance,
        )
    }
    pub fn show(&mut self) {
        self.open = true;
        self.dirty = true;
    }
    /// A part or spine handle is being dragged, or a bought part rides the
    /// cursor.
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.carry.is_some()
    }
    /// The part a palette carry (`true`) or a part drag (`false`) is moving
    /// (`creature_editor_edit` places it on the skin).
    pub fn moving_part(&self) -> Option<(usize, bool)> {
        self.carry
            .as_ref()
            .map(|c| (c.part, true))
            .or_else(|| self.drag.as_ref().filter(|d| d.spine.is_none()).map(|d| (d.part, false)))
    }
    /// Carry part `part` with the held press after a purchase made outside
    /// `input` (`creature_limbs`); `before` is the pre-purchase state.
    pub fn start_carry(&mut self, part: usize, before: RunProgress) {
        self.carry = Some(PartCarry { part, before, left_palette: false });
    }
    /// Follow the moved part after its symmetric twin was added or removed.
    pub fn set_moving_part(&mut self, part: usize) {
        if let Some(c) = self.carry.as_mut() {
            c.part = part;
        } else if let Some(d) = self.drag.as_mut().filter(|d| d.spine.is_none()) {
            d.part = part;
        }
    }
    /// Record an edit made outside `input` as one undo step.
    pub fn record_external(&mut self, before: RunProgress) {
        self.record(before);
    }
    /// One Undo for an editor that owns its part list (`rigid_editor`): the
    /// run goes back to before the last recorded edit. False when there is
    /// nothing to undo.
    pub fn undo_step(&mut self, run: &mut RunProgress) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(run.clone());
        *run = previous;
        run.request_save();
        self.visuals_dirty = true;
        true
    }
    pub fn redo_step(&mut self, run: &mut RunProgress) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(run.clone());
        *run = next;
        run.request_save();
        self.visuals_dirty = true;
        true
    }
    /// Forget the undo history (a session begins or ends).
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.carry = None;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    fn record(&mut self, before: RunProgress) {
        if self.undo.len() >= UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }
}
/// Entries the undo history keeps. Retail has no limit: `cEditor::CommitEditHistory`
/// (0x586410) drops the redo tail and appends, with no depth check (VERIFIED, `editors-audit`).
/// ADAPTED: a bound of 512 full snapshots so a very long session cannot grow without limit.
pub(crate) const UNDO_DEPTH: usize = 512;
#[derive(Component)]
pub(crate) struct PaletteRoot;
#[derive(Component)]
pub(crate) struct PaletteShell;
#[derive(Component)]
pub(crate) struct EditorHidden(pub(crate) Visibility);
#[derive(Component)]
pub(crate) struct MenuPreview;
#[derive(Component)]
pub(crate) struct SpineHandle;
#[derive(Component, Clone)]
pub(crate) enum Action {
    Category(usize),
    Page(i32),
    Buy(u32),
    Remove,
    Done,
    Focus,
    Body,
    Spine,
    Thickness(f32),
    Lift(f32),
    Length(f32),
    AddSpine,
    RemoveSpine,
    Undo,
    Redo,
    Symmetry,
    SurfaceSnap,
    RandomName,
}

pub fn simulation_active(editor: Res<EditorState>) -> bool {
    !editor.open
}

/// The creator is an isolated preview. Preserve each world's visibility so
/// closing it restores defeated creatures and other hidden entities exactly.
pub fn scene(
    mut commands: Commands,
    mut editor: ResMut<EditorState>,
    state: Res<State<GameStage>>,
    run: Res<RunProgress>,
    mut clear: ResMut<ClearColor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut originals: ResMut<Originals>,
    objects: Query<
        (
            Entity,
            Option<&Visibility>,
            Option<&EditorHidden>,
            Option<&Player>,
            Option<&CellPart>,
            Option<&crate::creature::Accessory>,
            Option<&crate::cell_parallax::CellBackdrop>,
        ),
        (With<StageOwned>, Without<ChildOf>),
    >,
    previews: Query<Entity, Or<(With<MenuPreview>, With<SpineHandle>)>>,
    players: Query<&Transform, With<Player>>,
) {
    // Routing hook: the Cell stage's creator is `cell_editor`.
    if crate::cell_editor::routes(state.get()) {
        return;
    }
    let cell_view = *state.get() == GameStage::Cell;
    if editor.cell_view != cell_view {
        editor.cell_view = cell_view;
    }
    if editor.open == editor.scene_open {
        return;
    }
    editor.scene_open = editor.open;
    if editor.open && cell_view {
        // Spore's Cell creator shows the cell mouth-up; start the orbit
        // behind the cell's current heading.
        if let Some(transform) = players.iter().next() {
            editor.orbit_yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
            editor.orbit_height = 0.;
        }
    }
    for (entity, visibility, hidden, player, cell_part, accessory, backdrop) in &objects {
        if editor.open {
            // The Cell stage's original deep-water floor stays as the Cell
            // creator's backdrop; everything swimming in the pool is hidden.
            if player.is_none()
                && cell_part.is_none()
                && accessory.is_none()
                && backdrop.is_none()
                && hidden.is_none()
            {
                commands.entity(entity).insert((
                    EditorHidden(visibility.copied().unwrap_or(Visibility::Inherited)),
                    Visibility::Hidden,
                ));
            }
        } else if let Some(hidden) = hidden {
            commands
                .entity(entity)
                .insert(hidden.0)
                .remove::<EditorHidden>();
        }
    }
    if editor.open {
        // The Cell creator floats over deep microscope water; the backdrop
        // itself comes from `editor_environment`, this only fills its edges.
        clear.0 = if cell_view {
            Color::srgb(0.012, 0.07, 0.16)
        } else {
            Color::srgb(0.06, 0.09, 0.12)
        };
        if *state.get() == GameStage::MainMenu && previews.is_empty() {
            if let Some(key) = crate::baked_model::player_source_key(&originals) {
                if let Err(error) = crate::baked_model::ensure(
                    &mut originals,
                    key,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                ) {
                    warn!("authored creator preview: {error}");
                }
            }
            let body = crate::baked_model::prepare_body(
                &mut originals,
                &run.body,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .ok();
            commands.spawn((
                StageOwned,
                MenuPreview,
                Player,
                Mover { speed: 0. },
                Health::new(120.),
                Hunger::new(120.),
                Visibility::Inherited,
                Mesh3d(
                    body.as_ref()
                        .map(|m| m.draw_mesh())
                        .unwrap_or_else(|| meshes.add(crate::body::mesh(&run.body))),
                ),
                MeshMaterial3d(body.and_then(|m| m.material).unwrap_or_else(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.43, 0.68, 0.33),
                        perceptual_roughness: 0.6,
                        ..default()
                    })
                })),
                Transform::default(),
            ));
            editor.visuals_dirty = true;
            editor.last_body = None;
        }
    } else {
        clear.0 = state.get().accent();
        if *state.get() == GameStage::MainMenu {
            for preview in &previews {
                commands.entity(preview).despawn();
            }
            for (entity, _, _, _, _, accessory, _) in &objects {
                if accessory.is_some() {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}
pub fn load_part_catalog(mut originals: ResMut<Originals>) {
    let Ok(install) = crate::spore_install::discover() else {
        return;
    };
    let index = asset_catalog::ResourceIndex::load(&install.packages);
    let catalog = asset_catalog::PartCatalog::from_index(&index);
    info!(
        "authored part catalog: {} parts, {} unresolved definitions",
        catalog.parts.len(),
        catalog.errors.len()
    );
    originals.part_catalog = catalog;
    originals.creation_catalog = crate::creation_records::CreationCatalog::from_index(&index);
    originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
    originals.action_lists =
        crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
    originals.resource_index = index;
}

pub fn ensure_model(
    originals: &mut Originals,
    definition: &PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(), String> {
    let instance = definition.model.instance_id;
    let existing = originals
        .models
        .iter()
        .position(|m| m.source_definition == Some(definition.key))
        .or_else(|| {
            originals
                .models
                .iter()
                .position(|m| m.source_definition.is_none() && m.instance == instance)
        });
    if let Some(model) =
        crate::starter_parts::ensure(originals, definition, meshes, images, materials)?
    {
        if let Some(i) = existing {
            originals.models[i] = model;
        } else {
            originals.models.push(model);
        }
        return Ok(());
    }
    if existing.is_some_and(|i| {
        originals.models[i].material.is_some()
            && originals.models[i].source_definition == Some(definition.key)
    }) {
        return Ok(());
    }
    let model = import_definition(originals, definition, meshes, images, materials)?;
    if let Some(i) = existing {
        originals.models[i] = model;
    } else {
        originals.models.push(model);
    }
    Ok(())
}

pub(crate) fn model_for_part<'a>(
    originals: &'a Originals,
    part: &PartInstance,
) -> Option<&'a OriginalModel> {
    part.definition_id
        .and_then(|def_id| {
            originals.models.iter().find(|m| {
                m.source_definition
                    .is_some_and(|k| k.instance_id == def_id)
            })
        })
        .or_else(|| originals.models.iter().find(|m| m.instance == part.instance))
}

/// Decode one exact authored definition without choosing a cache identity.
/// Callers can cache by the full PROP key to preserve distinct materials and
/// transforms when several definitions share a model instance ID.
pub fn import_definition(
    originals: &Originals,
    definition: &PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<OriginalModel, String> {
    let instance = definition.model.instance_id;
    // Decode geometry before allocating GPU assets. Failed imports can be
    // retried without accumulating unused textures and materials.
    let bytes = originals.resource_index.read(definition.model)?;
    let mut stats = crate::rw4::MeshStats::default();
    let imported = crate::rw4::extract_meshes(&bytes, &mut stats);
    let mesh = assembled_mesh(imported.clone(), definition)?;
    let links = crate::rw4::material_links(&bytes)?;
    let surface_meshes = imported
        .into_iter()
        .map(|piece| {
            let section = piece.source_section;
            let states = links
                .iter()
                .filter(|l| l.mesh_section == section)
                .flat_map(|l| l.state_sections.iter().copied())
                .collect::<Vec<_>>();
            let state = crate::material_binding::common_state(&bytes, &[section])?;
            Ok((
                section,
                states,
                assembled_mesh(vec![piece], definition)?,
                state,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let pixels = definition
        .diffuse
        .and_then(|k| originals.resource_index.read(k).ok())
        .and_then(|b| texture::decode_rw4(&b));
    let tint = definition
        .tint
        .and_then(|k| originals.resource_index.read(k).ok())
        .and_then(|b| texture::decode_rw4(&b));
    let normal_pixels = if let (Some(source), Some(painter)) =
        (definition.spec_bump, originals.native_painter.as_ref())
    {
        if crate::native_paint::enabled() {
            Some(painter.native_part_normal(&originals.resource_index, source)?)
        } else {
            let bytes = originals.resource_index.read(source)?;
            let packed = texture::decode_rw4(&bytes)
                .ok_or_else(|| format!("undecodable authored spec/bump texture {source:?}"))?;
            Some(painter.part_normal(&packed)?)
        }
    } else {
        None
    };
    // Native starter paint palette from decoded CreatureData properties
    // (mSkinColor1/2/3 at 0x20/0x2c/0x38). Native floats are linear shader
    // space; quantized to u8 for the existing [[u8;3];3] skinpaint API
    // (quantization adaptation only, not color-space conversion). Falls back
    // to the legacy green defaults when the anatomy source is unavailable.
    let native_colors = if definition.key.group_id == asset_catalog::CREATURE_PARTS {
        originals
            .resource_index
            .read(crate::dbpf::ResourceKey {
                type_id: crate::creature_data::TYPE,
                group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
                instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
            })
            .ok()
            .and_then(|bytes| crate::creature_data::parse(&bytes).ok())
            .and_then(|data| data.skin_colors().ok())
            .map(|c| crate::creature_data::CreatureData::quantize_skin_colors(c))
            .unwrap_or([[109, 173, 84], [76, 119, 58], [223, 188, 82]])
    } else {
        [[109, 173, 84], [76, 119, 58], [223, 188, 82]]
    };
    // Imported cutouts and translucent pixels must retain their transparency.
    let alpha_mode = pixels.as_ref().map_or(AlphaMode::Opaque, |p| {
        if p.rgba.chunks_exact(4).any(|c| c[3] > 0 && c[3] < 255) {
            AlphaMode::Blend
        } else if p.rgba.chunks_exact(4).any(|c| c[3] == 0) {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        }
    });
    let image = pixels
        .map(|p| {
            let colors = native_colors;
            let p = if crate::native_paint::enabled() {
                if let (Some(painter), Some(diffuse), Some(mask)) = (
                    &originals.native_painter,
                    definition.diffuse,
                    definition.tint,
                ) {
                    painter.native_part_tint(&originals.resource_index, diffuse, mask, colors)?
                } else {
                    // An authored surface without a tint binding keeps its
                    // original diffuse pixels. Do not invent a paint mask or
                    // substitute the CPU preview's palette blend.
                    p
                }
            } else if let (Some(painter), Some(mask)) = (&originals.native_painter, tint.as_ref()) {
                painter.preview_part(&p, mask, colors)?
            } else {
                texture::skinpaint(&p, tint.as_ref(), colors)
            };
            Ok::<_, String>(images.add(Image::new(
                Extent3d {
                    width: p.width,
                    height: p.height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                p.rgba,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            )))
        })
        .transpose()?;
    let material = StandardMaterial {
        base_color: if image.is_some() {
            Color::WHITE
        } else {
            Color::srgb(0.6, 0.8, 0.4)
        },
        base_color_texture: image,
        alpha_mode,
        perceptual_roughness: 0.65,
        cull_mode: None,
        ..default()
    };
    let mut material = material;
    if let Some(pixels) = normal_pixels {
        material.normal_map_texture = Some(images.add(Image::new(
            Extent3d {
                width: pixels.width,
                height: pixels.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.rgba,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        )));
        material.flip_normal_map_y = false;
    }
    let mut surfaces = Vec::with_capacity(surface_meshes.len());
    for (source_section, state_sections, mut surface_mesh, state) in surface_meshes {
        let mut native_material = material.clone();
        if native_material.normal_map_texture.is_some() && surface_mesh.generate_tangents().is_err()
        {
            native_material.normal_map_texture = None;
            warn!(
                "part {} surface {source_section}: normal map needs valid UV tangents",
                definition.name
            );
        }
        if let Some(state) = &state {
            let bound = crate::material_binding::apply_static(
                state,
                &bytes,
                &mut surface_mesh,
                images,
                &mut native_material,
                definition.diffuse.is_some(),
            )?;
            // A window part's interior mesh (civ-polish2).
            if !bound {
                crate::material_binding::apply_window_interior(state, &bytes, images, &mut native_material)?;
            }
        }
        surfaces.push(crate::OriginalSurface {
            handle: meshes.add(surface_mesh),
            material: materials.add(native_material),
            source_section,
            state_sections,
        });
    }
    let material = surfaces
        .first()
        .map(|s| s.material.clone())
        .unwrap_or_else(|| materials.add(material));
    let (root, rig_bones) = crate::part_root_and_bones(&bytes).unwrap_or((None, 0));
    let mut preview_scale = definition.model_scale * 5.0;
    if definition.key.group_id == asset_catalog::CREATURE_PARTS {
        // The creature body's source is fitted to a 1.9-unit footprint. Keep
        // its editable features in that scale instead of applying the former
        // five-unit conversion to every eye and mouth. Saved scale stays on top.
        let cap = match definition.category() {
            "Eyes" => 0.45,
            "Mouths" => 0.7,
            "Feet" => 0.7,
            "Hands" => 0.65,
            "Movement" => 1.0,
            _ => 0.75,
        };
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            let (min, max) = positions.iter().fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(min, max), p| (min.min(Vec3::from_array(*p)), max.max(Vec3::from_array(*p))),
            );
            let extent = (max - min).max_element();
            if extent.is_finite() && extent > 0. {
                preview_scale = preview_scale.min(cap * 2. / extent);
            }
        }
    }
    let model = OriginalModel {
        source_definition: Some(definition.key),
        handle: meshes.add(mesh),
        // One common conversion preserves authored relative dimensions.
        // Retail part coordinates are roughly a fifth of our world units.
        scale: preview_scale,
        instance,
        root,
        rig_bones,
        material: Some(material),
        surfaces,
    };
    Ok(model)
}

/// Build an individual XML block's handle pose before basis conversion.
/// Materials stay shared with its exact-definition base import; geometry
/// is cached separately by its full handle assignment in creation_assets.
pub fn creation_mesh(
    originals: &Originals,
    definition: &PartDefinition,
    handles: &[(u32, f32)],
) -> Result<(Mesh, Vec<(usize, Mesh)>, crate::morph::MorphApplication), String> {
    let bytes = originals.resource_index.read(definition.model)?;
    let data = crate::rw4::extract_morph_data(&bytes)?;
    let mut imported = crate::rw4::extract_meshes(&bytes, &mut crate::rw4::MeshStats::default());
    let applied = crate::morph::apply_creation_handles(&mut imported, &data, handles)?;
    let combined = assembled_mesh(imported.clone(), definition)?;
    let surfaces = imported
        .into_iter()
        .map(|m| Ok((m.source_section, assembled_mesh(vec![m], definition)?)))
        .collect::<Result<Vec<_>, String>>()?;
    Ok((combined, surfaces, applied))
}

fn assembled_mesh(
    imported: Vec<crate::rw4::ExtractedMesh>,
    definition: &PartDefinition,
) -> Result<Mesh, String> {
    let count: usize = imported.iter().map(|m| m.positions.len()).sum();
    if count == 0 {
        return Err(format!("{} has no supported geometry", definition.name));
    }
    if count > 100_000 {
        return Err("part mesh exceeds vertex limit".into());
    }
    // Spore's part coordinates are Z-up; the game world uses Y-up.
    // Preserve the authored origin and uniform scale so different parts keep
    // their relative size rather than normalizing every mesh's bounding box.
    let rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Quat::from_euler(
            EulerRot::XYZ,
            definition.model_rotation[0].to_radians(),
            definition.model_rotation[1].to_radians(),
            definition.model_rotation[2].to_radians(),
        );
    let offset = Vec3::from_array(definition.model_offset);
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    let mut indices = Vec::new();
    for part in imported {
        let base = positions.len() as u32;
        if part.positions.len() != part.normals.len()
            || part.positions.len() != part.uvs.len()
            || part
                .indices
                .iter()
                .any(|&i| i as usize >= part.positions.len())
        {
            return Err("inconsistent part vertex buffers".into());
        }
        positions.extend(
            part.positions
                .iter()
                .map(|&v| (rotation * (Vec3::from_array(v) + offset)).to_array()),
        );
        normals.extend(
            part.normals
                .iter()
                .map(|&v| (rotation * Vec3::from_array(v)).to_array()),
        );
        uvs.extend(part.uvs);
        indices.extend(part.indices.into_iter().map(|i| base + i as u32));
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    // Morph assemblies rebuild these vertex buffers, so they must also keep
    // tangents whenever their imported native material supplies a normal map.
    let _ = mesh.generate_tangents();
    Ok(mesh)
}

fn group(stage: &GameStage) -> u32 {
    if *stage == GameStage::Cell {
        asset_catalog::CELL_PARTS
    } else {
        asset_catalog::CREATURE_PARTS
    }
}
fn active_parts<'a>(run: &'a RunProgress, stage: &GameStage) -> &'a [PartInstance] {
    if *stage == GameStage::Cell {
        &run.cell_parts
    } else {
        &run.parts
    }
}
fn active_parts_mut<'a>(run: &'a mut RunProgress, stage: &GameStage) -> &'a mut Vec<PartInstance> {
    if *stage == GameStage::Cell {
        &mut run.cell_parts
    } else {
        &mut run.parts
    }
}
/// Spore's Cell editor starts with mouths, eyes and flagella; every other
/// part kind must be found in the pool first. Later editors keep the
/// DNA-milestone unlock levels.
fn part_locked(run: &RunProgress, stage: &GameStage, definition: &PartDefinition) -> bool {
    // The outfitters sell every palette item (`itemUnlockFindPercentage` 1).
    if crate::outfitter::in_session(run) {
        return false;
    }
    if *stage == GameStage::Cell {
        return crate::cell::discoverable_kind(&definition.abilities)
            .is_some_and(|kind| !run.cell_found_parts.iter().any(|k| k == kind));
    }
    if *stage == GameStage::Creature {
        return definition.unlock_level >= crate::creature_parts::FIRST_FOUND_LEVEL
            && !run
                .creature_found_parts
                .contains(&definition.key.instance_id);
    }
    definition.unlock_level > unlocked(run, stage)
}
fn unlocked(run: &RunProgress, stage: &GameStage) -> u32 {
    if *stage == GameStage::MainMenu {
        5
    } else {
        1 + (run.dna_this_stage / game::tuning_for(stage).dna_to_advance.max(1.) * 4.).clamp(0., 4.)
            as u32
    }
}

pub fn purchase(
    run: &mut RunProgress,
    stage: &GameStage,
    definition: &PartDefinition,
    bone: u32,
) -> Result<(), String> {
    purchase_assembly(run, stage, definition, bone, false)
}

fn paired_slot(stage: &GameStage, bone: u32) -> u32 {
    if *stage == GameStage::Cell {
        match bone {
            1 => 2,
            2 => 1,
            4 => 5,
            5 => 4,
            _ => bone,
        }
    } else {
        match bone {
            1 => 2,
            2 => 1,
            3 => 4,
            4 => 3,
            5 => 6,
            6 => 5,
            _ => bone,
        }
    }
}
fn attachment_slot(body: &crate::body::BodyProfile, stage: &GameStage, bone: u32) -> Vec3 {
    if *stage == GameStage::Cell {
        cell_slot(bone)
    } else {
        body.slot(bone)
    }
}
fn reflected_part(
    body: &crate::body::BodyProfile,
    stage: &GameStage,
    p: &PartInstance,
) -> PartInstance {
    let mut other = p.clone();
    other.bone = paired_slot(stage, p.bone);
    let position = attachment_slot(body, stage, p.bone) + Vec3::from_array(p.offset);
    let reflected = Vec3::new(-position.x, position.y, position.z);
    other.offset = game::clamp_offset(
        (reflected - attachment_slot(body, stage, other.bone)).to_array(),
        game::OFFSET_LIMIT,
    );
    other.yaw = -p.yaw;
    other.mirrored = !p.mirrored;
    other.edits = p.edits.mirrored();
    other
}
/// A symmetric pair is one purchase, one refund and one undo operation.
/// The pair ID survives saving and does not depend on vector indices.
fn purchase_assembly(
    run: &mut RunProgress,
    stage: &GameStage,
    definition: &PartDefinition,
    bone: u32,
    symmetry: bool,
) -> Result<(), String> {
    if definition.is_null {
        return Err("Internal assembly helper is not an editor part".into());
    }
    if definition.key.group_id != crate::outfitter::edit_group(run, group(stage)) {
        return Err("This part belongs to a different editor".into());
    }
    if part_locked(run, stage, definition) {
        return Err(if *stage == GameStage::Cell {
            "Find this part in the tide pool first".into()
        } else if *stage == GameStage::Creature {
            "Find this part in a bone pile first".into()
        } else {
            format!("Unlock level {} required", definition.unlock_level)
        });
    }
    if run.editor_dna < definition.price as f32 {
        return Err(format!("Need {} DNA", definition.price));
    }
    let pair = symmetry && paired_slot(stage, bone) != bone;
    if active_parts(run, stage).len() + if pair { 2 } else { 1 } > 48 {
        return Err("Part limit reached".into());
    }
    let mut part =
        PartInstance::new(definition.model.instance_id, bone).with_definition(definition.key.instance_id);
    part.scale = definition.default_scale.clamp(
        definition.min_scale,
        definition.max_scale.max(definition.min_scale),
    );
    if pair {
        part.pair_id = active_parts(run, stage)
            .iter()
            .map(|p| p.pair_id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Symmetry ID limit reached")?;
    }
    let focus = active_parts(run, stage).len() as u32;
    let other = pair.then(|| reflected_part(&run.body, stage, &part));
    active_parts_mut(run, stage).push(part);
    if let Some(other) = other {
        active_parts_mut(run, stage).push(other);
    }
    run.focused_part = focus;
    run.editor_dna -= definition.price as f32;
    run.sync_legacy_accessory();
    run.request_save();
    Ok(())
}

fn sync_pair(run: &mut RunProgress, stage: &GameStage, focus: usize) {
    let Some(part) = active_parts(run, stage)
        .get(focus)
        .filter(|p| p.pair_id != 0)
        .cloned()
    else {
        return;
    };
    let mirror = reflected_part(&run.body, stage, &part);
    for (i, p) in active_parts_mut(run, stage).iter_mut().enumerate() {
        if i != focus && p.pair_id == part.pair_id {
            *p = mirror.clone();
        }
    }
}
fn sell_selected(
    run: &mut RunProgress,
    stage: &GameStage,
    catalog: &asset_catalog::PartCatalog,
) -> bool {
    let focus = run.focused_part as usize;
    let Some(part) = active_parts(run, stage).get(focus).cloned() else {
        return false;
    };
    let refund = catalog
        .by_part_in(crate::outfitter::edit_group(run, group(stage)), &part)
        .map(|p| p.price)
        .unwrap_or(0);
    if part.pair_id == 0 {
        active_parts_mut(run, stage).remove(focus);
    } else {
        active_parts_mut(run, stage).retain(|p| p.pair_id != part.pair_id);
    }
    run.editor_dna += refund as f32;
    run.focused_part = run
        .focused_part
        .min(active_parts(run, stage).len().saturating_sub(1) as u32);
    run.sync_legacy_accessory();
    run.request_save();
    true
}

fn finish_drag(editor: &mut EditorState, run: &mut RunProgress, stage: &GameStage) {
    if let Some(drag) = editor.drag.take() {
        if active_parts(&drag.before, stage) != active_parts(run, stage)
            || drag.before.body != run.body
        {
            editor.record(drag.before);
            run.request_save();
        }
    }
}

pub fn prepare_stage(
    mut originals: ResMut<Originals>,
    mut run: ResMut<RunProgress>,
    state: Res<State<GameStage>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut editor: ResMut<EditorState>,
) {
    let stage = state.get();
    editor.enter_stage(stage);
    editor.open = false;
    editor.dirty = true;
    editor.visuals_dirty = true;
    seed_parts(&mut run, &originals, stage);
    load_saved_models(
        &run,
        &mut originals,
        stage,
        &mut meshes,
        &mut images,
        &mut materials,
    );
    crate::outfitter::load_worn(&run, &mut originals, stage, &mut meshes, &mut images, &mut materials);
    if matches!(stage, GameStage::Creature | GameStage::Tribal) {
        if let Some(key) = crate::baked_model::player_source_key(&originals) {
            if let Err(error) = crate::baked_model::ensure(
                &mut originals,
                key,
                &mut meshes,
                &mut images,
                &mut materials,
            ) {
                warn!("authored player model: {error}");
            }
        }
    }
}

/// Exact unchanged starter recipe used by the editor and authored-player
/// display guard. A sold, moved, morphed, scaled or replaced part differs.
pub fn starter_creature_parts(
    run: &RunProgress,
    catalog: &asset_catalog::PartCatalog,
) -> Vec<PartInstance> {
    starter_creature_parts_for_diet(run.diet_carnivore > run.diet_herbivore, catalog)
}

/// Whether `parts` still exactly matches either untouched starter recipe.
/// Diet counters can change during play, so this check is stable even when
/// consumed food would cause the seeding helper to choose the other mouth.
pub fn is_starter_creature_parts(
    parts: &[PartInstance],
    catalog: &asset_catalog::PartCatalog,
) -> bool {
    let herbivore = starter_creature_parts_for_diet(false, catalog);
    let carnivore = starter_creature_parts_for_diet(true, catalog);
    parts == herbivore.as_slice()
        || parts == carnivore.as_slice()
        || parts == legacy_starter_parts(false, catalog).as_slice()
}

fn starter_creature_parts_for_diet(
    carnivore: bool,
    catalog: &asset_catalog::PartCatalog,
) -> Vec<PartInstance> {
    if !carnivore {
        if let Some(parts) = crate::starter_parts::recipe(catalog) {
            return parts;
        }
    }
    legacy_starter_parts(carnivore, catalog)
}
fn legacy_starter_parts(
    carnivore: bool,
    catalog: &asset_catalog::PartCatalog,
) -> Vec<PartInstance> {
    [
        ("Mouths", 0),
        ("Eyes", 1),
        ("Eyes", 2),
        ("Feet", 3),
        ("Feet", 4),
    ]
    .into_iter()
    .filter_map(|(category, bone)| {
        catalog
            .parts
            .iter()
            .find(|p| {
                p.key.group_id == asset_catalog::CREATURE_PARTS
                    && p.category() == category
                    && !p.is_null
                    && p.unlock_level <= 1
                    && (category != "Mouths"
                        || if carnivore {
                            p.abilities.carnivore > 0
                        } else {
                            p.abilities.herbivore > 0
                        })
            })
            .map(|p| PartInstance::new(p.model.instance_id, bone))
    })
    .collect()
}

fn seed_parts(run: &mut RunProgress, originals: &Originals, stage: &GameStage) {
    // A new cell begins with a plant mouth, eye and flagellum. These free
    // starting components can subsequently be sold/replaced in the editor.
    if *stage == GameStage::Cell
        && !run.cell_editor_initialized
        && !originals.part_catalog.parts.is_empty()
    {
        run.cell_editor_initialized = true;
        run.request_save();
        if run.cell_parts.is_empty() {
            for (category, bone) in [("Mouths", 0), ("Eyes", 1), ("Movement", 3)] {
                if let Some(p) = originals.part_catalog.parts.iter().find(|p| {
                    p.key.group_id == asset_catalog::CELL_PARTS
                        && p.category() == category
                        && !p.is_null
                        && (category != "Mouths" || p.abilities.herbivore > 0)
                        && (category != "Movement" || p.abilities.cell_flagella > 0)
                }) {
                    run.cell_parts
                        .push(PartInstance::new(p.model.instance_id, bone));
                }
            }
        }
    }
    run.body = run.body.validated();
    if matches!(stage, GameStage::Creature | GameStage::MainMenu)
        && !run.creature_editor_initialized
        && !originals.part_catalog.parts.is_empty()
    {
        run.creature_editor_initialized = true;
        run.request_save();
        if run.parts.is_empty() {
            run.parts = starter_creature_parts(run, &originals.part_catalog);
            run.sync_legacy_accessory();
            run.request_save();
        }
    }
}

pub(crate) fn load_saved_models(
    run: &RunProgress,
    originals: &mut Originals,
    stage: &GameStage,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let definitions: Vec<_> = active_parts(&run, stage)
        .iter()
        .filter_map(|p| {
            originals
                .part_catalog
                .by_part_in(group(stage), p)
                .cloned()
        })
        .collect();
    for definition in definitions {
        if let Err(err) = ensure_model(originals, &definition, meshes, images, materials) {
            warn!("part import: {err}");
        }
    }
}

pub fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<&Action, Added<Pressed>>,
    state: Res<State<GameStage>>,
    mut editor: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
) {
    let stage = state.get();
    editor.enter_stage(stage);
    // A rigid-block session (`rigid_editor`) owns its keys and actions.
    if editor.rigid {
        return;
    }
    if !(matches!(
        stage,
        GameStage::Cell | GameStage::Creature | GameStage::MainMenu
    ) || crate::outfitter::routes(stage))
        || crate::cell_editor::routes(stage)
    {
        return;
    }
    // In Cell the editor is only reached through a mating call (cell.rs);
    // F2 there summons a mate instead of opening the creator directly.
    let cell_mating_only = *stage == GameStage::Cell && !editor.open;
    // The retail Creature Creator (`creature_editor`) has its own keyboard
    // table: Esc opens its options panel there.
    let retail = crate::creature_editor::routes(stage);
    // The outfitters share F2 with the other creators; the Civ / Space
    // assembly modal (F3) is a different editor and is not open under them.
    let modal_open = crate::outfitter::routes(stage) && mechanical.is_some_and(|m| m.open);
    if (keys.just_pressed(KeyCode::F2) && !cell_mating_only && !modal_open)
        || (keys.just_pressed(KeyCode::Escape) && editor.open && !retail)
    {
        finish_drag(&mut editor, &mut run, stage);
        editor.open = !editor.open;
        editor.dirty = true;
        editor.visuals_dirty = true;
    }
    if !editor.open {
        return;
    }
    if *stage == GameStage::MainMenu {
        seed_parts(&mut run, &originals, stage);
        load_saved_models(
            &run,
            &mut originals,
            stage,
            &mut meshes,
            &mut images,
            &mut materials,
        );
    }
    let mut actions: Vec<Action> = buttons.iter().cloned().collect();
    actions.extend(std::mem::take(&mut editor.queued));
    if retail {
        // Keys are mapped by `creature_editor::keyboard`.
    } else if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        if keys.just_pressed(KeyCode::KeyZ) {
            actions.push(Action::Undo);
        }
        if keys.just_pressed(KeyCode::KeyY) {
            actions.push(Action::Redo);
        }
    }
    if keys.just_pressed(KeyCode::Backspace) && !retail {
        actions.push(Action::Remove);
    }
    if keys.just_pressed(KeyCode::Tab) && !retail {
        actions.push(Action::Focus);
    }
    if keys.just_pressed(KeyCode::KeyM) && !retail {
        actions.push(Action::Symmetry);
    }
    if !actions.is_empty() {
        finish_drag(&mut editor, &mut run, stage);
        // Any further palette action drops a carried part where it is.
        editor.carry = None;
    }
    for action in actions {
        let history_action = matches!(&action, Action::Undo | Action::Redo);
        let before = run.clone();
        match action {
            Action::Undo => {
                if let Some(previous) = editor.undo.pop() {
                    editor.redo.push(run.clone());
                    *run = previous;
                    run.request_save();
                    editor.visuals_dirty = true;
                }
            }
            Action::Redo => {
                if let Some(next) = editor.redo.pop() {
                    editor.undo.push(run.clone());
                    *run = next;
                    run.request_save();
                    editor.visuals_dirty = true;
                }
            }
            Action::Category(i) => {
                editor.body_mode = false;
                editor.visuals_dirty = true;
                editor.scroll_offset = 0.;
                editor.category = i;
                editor.page = 0;
            }
            Action::Page(dir) => {
                editor.scroll_offset = 0.;
                editor.page = editor.page.saturating_add_signed(dir as isize);
            }
            Action::Done => {
                editor.open = false;
                editor.visuals_dirty = true;
                run.request_save();
            }
            Action::Focus => {
                let n = active_parts(&run, stage).len();
                if n > 0 {
                    run.focused_part = (run.focused_part + 1) % n as u32;
                }
            }
            Action::Symmetry => {
                editor.asymmetric = !editor.asymmetric;
            }
            Action::RandomName => {
                editor.name_seed = editor.name_seed.wrapping_add(1);
                editor.name = Some(random_name(
                    editor.name_seed,
                    *stage == GameStage::Cell,
                    run.dna_banked_total as u32,
                ));
            }
            Action::SurfaceSnap => {
                editor.free_placement = !editor.free_placement;
            }
            Action::Remove => {
                if run.focused_part as usize >= editor.locked_parts
                    && sell_selected(&mut run, stage, &originals.part_catalog)
                {
                    editor.visuals_dirty = true;
                }
            }
            Action::Body => {
                editor.body_mode = !editor.body_mode;
                editor.visuals_dirty = true;
            }
            Action::Spine => {
                editor.focused_spine = (editor.focused_spine + 1) % run.body.spine.len().max(1);
                editor.visuals_dirty = true;
            }
            Action::Thickness(delta) => {
                let i = editor.focused_spine;
                // Reject rather than reset: a point dragged onto its neighbour
                // refuses the edit and the rest of the body survives untouched.
                if run.body.edit_spine(|spine| {
                    let Some(p) = spine.get_mut(i) else {
                        return false;
                    };
                    p.radius = (p.radius + delta).clamp(0.08, 2.);
                    true
                }) {
                    editor.visuals_dirty = true;
                    run.request_save();
                }
            }
            Action::Lift(delta) => {
                let i = editor.focused_spine;
                if run.body.edit_spine(|spine| {
                    let Some(p) = spine.get_mut(i) else {
                        return false;
                    };
                    p.position[1] = (p.position[1] + delta).clamp(-1., 2.);
                    true
                }) {
                    editor.visuals_dirty = true;
                    run.request_save();
                }
            }
            Action::Length(delta) => {
                let i = editor.focused_spine;
                if run.body.edit_spine(|spine| {
                    let Some(p) = spine.get_mut(i) else {
                        return false;
                    };
                    p.position[2] = (p.position[2] + delta).clamp(-3., 3.);
                    true
                }) {
                    editor.visuals_dirty = true;
                    run.request_save();
                }
            }
            Action::AddSpine => {
                let i = editor.focused_spine;
                if run.body.edit_spine(|spine| {
                    if spine.len() >= 32 {
                        return false;
                    }
                    let i = i.min(spine.len().saturating_sub(2));
                    let midpoint = {
                        let a = &spine[i];
                        let b = &spine[i + 1];
                        [
                            (a.position[0] + b.position[0]) * 0.5,
                            (a.position[1] + b.position[1]) * 0.5,
                            (a.position[2] + b.position[2]) * 0.5,
                        ]
                    };
                    let radius = (spine[i].radius + spine[i + 1].radius) * 0.5;
                    spine.insert(
                        i + 1,
                        crate::body::SpinePoint {
                            position: midpoint,
                            radius,
                        },
                    );
                    true
                }) {
                    editor.focused_spine = i + 1;
                    editor.visuals_dirty = true;
                    run.request_save();
                }
            }
            Action::RemoveSpine => {
                let i = editor.focused_spine;
                if run.body.edit_spine(|spine| {
                    if spine.len() <= 2 {
                        return false;
                    }
                    spine.remove(i.min(spine.len() - 1));
                    true
                }) {
                    editor.focused_spine = i.min(run.body.spine.len() - 1);
                    editor.visuals_dirty = true;
                    run.request_save();
                }
            }
            Action::Buy(instance) => {
                let sold_group = crate::outfitter::edit_group(&run, group(stage));
                if let Some(definition) = originals
                    .part_catalog
                    .by_definition_in(sold_group, instance)
                    .or_else(|| originals.part_catalog.by_model_in(sold_group, instance))
                    .cloned()
                {
                    let bone = if *stage == GameStage::Cell {
                        match definition.category() {
                            "Mouths" => 0,
                            "Eyes" => 1,
                            "Movement" => 3,
                            _ => 4,
                        }
                    } else {
                        match definition.category() {
                            "Mouths" => 0,
                            "Eyes" => 1,
                            "Feet" => 3,
                            "Hands" => 5,
                            _ => 7,
                        }
                    };
                    // Import first: unsupported records never charge the player.
                    editor.message = match ensure_model(
                        &mut originals,
                        &definition,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                    )
                    .and_then(|()| {
                        purchase_assembly(&mut run, stage, &definition, bone, !editor.asymmetric)
                    }) {
                        Ok(()) => {
                            editor.visuals_dirty = true;
                            // A press that is still held picks the new part up:
                            // it follows the cursor onto the body until release.
                            if mouse.pressed(MouseButton::Left) {
                                editor.carry = Some(PartCarry {
                                    part: run.focused_part as usize,
                                    before: before.clone(),
                                    left_palette: false,
                                });
                            }
                            format!("Mounted {}", definition.name)
                        }
                        Err(err) => err,
                    };
                }
            }
        }
        if !history_action
            && (before.parts != run.parts
                || before.cell_parts != run.cell_parts
                || before.body != run.body
                || before.editor_dna != run.editor_dna)
        {
            editor.record(before);
        }
        editor.dirty = true;
    }
    // A close action in this same frame ends editing: without this guard a key
    // held across the Done press would still mutate the focused part and push
    // onto the history of a creator the player already left.
    if !editor.open || retail {
        return;
    }
    let before_edit = run.clone();
    let fi = run.focused_part as usize;
    if let Some(part) = active_parts_mut(&mut run, stage).get_mut(fi) {
        let mut changed = false;
        if keys.just_pressed(KeyCode::KeyR) {
            part.yaw = game::wrap_yaw(part.yaw + std::f32::consts::FRAC_PI_4);
            changed = true;
        }
        if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::Equal) {
            let (min, max) = originals
                .part_catalog
                .by_part_in(group(stage), part)
                .map(|p| (p.min_scale, p.max_scale))
                .unwrap_or((0.5, 2.));
            part.scale = (part.scale
                * if keys.just_pressed(KeyCode::Minus) {
                    0.9
                } else {
                    1.1
                })
            .clamp(min, max.max(min));
            changed = true;
        }
        if keys.just_pressed(KeyCode::Period) {
            part.bone = (part.bone + 1) % if *stage == GameStage::Cell { 6 } else { 8 };
            changed = true;
        }
        changed |= game::apply_slide_keys(&keys, part);
        if changed {
            sync_pair(&mut run, stage, fi);
            run.sync_legacy_accessory();
            run.request_save();
            editor.visuals_dirty = true;
            editor.dirty = true;
            editor.record(before_edit);
        }
    }
}

#[derive(Component)]
pub(crate) struct CellPart;
pub fn cell_slot(bone: u32) -> Vec3 {
    match bone {
        0 => Vec3::new(0., 0., -0.9),
        1 => Vec3::new(-0.65, 0.6, -0.5),
        2 => Vec3::new(0.65, 0.6, -0.5),
        3 => Vec3::new(0., 0., 0.9),
        4 => Vec3::new(-0.75, 0., 0.),
        _ => Vec3::new(0.75, 0., 0.),
    }
}

fn ray_plane(origin: Vec3, direction: Vec3, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = direction.dot(normal);
    if denom.abs() < 1e-5 {
        return None;
    }
    let distance = (point - origin).dot(normal) / denom;
    if distance < 0. || !distance.is_finite() {
        None
    } else {
        Some(origin + direction * distance)
    }
}
/// Drag a mounted part in the plane facing the editor camera. Its saved
/// offset remains body-local, so rotating/moving the creature preserves it.
pub fn drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    player: Query<&Transform, With<Player>>,
    state: Res<State<GameStage>>,
    mut editor: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
) {
    if editor.rigid {
        return;
    }
    if !editor.open {
        finish_drag(&mut editor, &mut run, state.get());
        editor.carry = None;
        return;
    }
    let stage = state.get();
    if !(matches!(
        stage,
        GameStage::Cell | GameStage::Creature | GameStage::MainMenu
    ) || crate::outfitter::routes(stage))
        || crate::cell_editor::routes(stage)
    {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, cam)) = cameras.single() else {
        return;
    };
    let Ok(pt) = player.single() else {
        return;
    };
    let layout = editor
        .retail_layout
        .filter(|_| crate::creature_editor::routes(stage))
        .unwrap_or_else(|| CreatorLayout::new(window.width(), window.height(), stage));
    if editor.carry.is_some() {
        carry_part(
            &mouse,
            window.cursor_position(),
            &layout,
            camera,
            cam,
            pt,
            stage,
            &mut editor,
            &mut run,
            &originals,
        );
        return;
    }
    if mouse.just_released(MouseButton::Left) {
        finish_drag(&mut editor, &mut run, stage);
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam, cursor) else {
        return;
    };
    let direction = *ray.direction;
    if mouse.just_pressed(MouseButton::Left) && !layout.over_ui(cursor) {
        let editing_spine = editor.body_mode && *stage != GameStage::Cell;
        let centers: Vec<Vec3> = if editing_spine {
            run.body
                .spine
                .iter()
                .enumerate()
                .map(|(i, _)| crate::baked_model::control_handle(&originals, &run.body, i))
                .collect()
        } else {
            active_parts(&run, stage)
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    // The creature's own parts cannot be picked in an
                    // outfitter (they are put out of reach).
                    if i < editor.locked_parts {
                        return Vec3::splat(1e6);
                    }
                    let slot = if *stage == GameStage::Cell {
                        cell_slot(p.bone)
                    } else {
                        crate::baked_model::attachment_point(&originals, &run.body, p.bone)
                    };
                    slot + Vec3::from_array(p.offset)
                })
                .collect()
        };
        let closest = centers
            .iter()
            .enumerate()
            .filter_map(|(i, local)| {
                let center = pt.translation + pt.rotation * (*local * pt.scale);
                let along = (center - ray.origin).dot(direction);
                if along < 0. {
                    return None;
                }
                let d = (center - (ray.origin + direction * along)).length_squared();
                if d < 0.4 * 0.4 {
                    Some((i, center, d))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((part, center, _)) = closest {
            if let Some(point) = ray_plane(ray.origin, direction, center, direction) {
                if editing_spine {
                    editor.focused_spine = part;
                    editor.visuals_dirty = true;
                } else {
                    run.focused_part = part as u32;
                }
                editor.drag = Some(PartDrag {
                    part,
                    spine: editing_spine.then_some(part),
                    point: center,
                    normal: direction,
                    grab: point - center,
                    before: run.clone(),
                });
                editor.dirty = true;
            }
        }
    }
    if mouse.pressed(MouseButton::Left) {
        if let Some(drag) = editor.drag.as_ref() {
            let part_index = drag.part;
            let spine_index = drag.spine;
            if let Some(center) =
                ray_plane(ray.origin, direction, drag.point, drag.normal).map(|p| p - drag.grab)
            {
                let body = run.body.clone();
                if let Some(index) = spine_index {
                    let mut local =
                        (pt.rotation.conjugate() * (center - pt.translation)) / pt.scale;
                    local = Vec3::from_array(body.spine[index].position) + local
                        - crate::baked_model::control_handle(&originals, &body, index);
                    if !editor.asymmetric {
                        local.x = 0.;
                    }
                    local = local.clamp(Vec3::splat(-3.), Vec3::splat(3.));
                    if !local.is_finite() {
                        return;
                    }
                    // Dropping onto any other point is refused, so the drag can
                    // never collapse the body silently.
                    if run.body.spine.iter().enumerate().any(|(i, p)| {
                        i != index && Vec3::from_array(p.position).distance_squared(local) < 0.0001
                    }) {
                        return;
                    }
                    if run.body.edit_spine(|spine| {
                        let Some(p) = spine.get_mut(index) else {
                            return false;
                        };
                        p.position = local.to_array();
                        true
                    }) {
                        editor.visuals_dirty = true;
                        editor.dirty = true;
                        run.request_save();
                    }
                    return;
                }
                if let Some(p) = active_parts_mut(&mut run, stage).get_mut(part_index) {
                    let slot = if *stage == GameStage::Cell {
                        cell_slot(p.bone)
                    } else {
                        crate::baked_model::attachment_point(&originals, &body, p.bone)
                    };
                    let mut local =
                        (pt.rotation.conjugate() * (center - pt.translation)) / pt.scale;
                    if !editor.free_placement {
                        local = if *stage == GameStage::Cell {
                            // The cell body has radius 0.9; its squash is in
                            // the player's transform, already removed above.
                            local.normalize_or(Vec3::NEG_Z) * 0.9
                        } else {
                            crate::baked_model::nearest_body_surface(&originals, &body, local)
                        };
                    }
                    p.offset = game::clamp_offset((local - slot).to_array(), game::OFFSET_LIMIT);
                    sync_pair(&mut run, stage, part_index);
                    editor.visuals_dirty = true;
                    editor.dirty = true;
                    run.request_save();
                }
            }
        }
    }
}

/// Body-local point under the cursor, snapped like a mounted-part drag: onto
/// the cell membrane or the nearest creature body surface.
fn body_point_under(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
    pt: &Transform,
    stage: &GameStage,
    snap: bool,
    originals: &Originals,
    body: &crate::body::BodyProfile,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let direction = *ray.direction;
    let hit = ray_plane(ray.origin, direction, pt.translation, direction)?;
    let local = (pt.rotation.conjugate() * (hit - pt.translation)) / pt.scale;
    if !local.is_finite() {
        return None;
    }
    Some(if !snap {
        local
    } else if *stage == GameStage::Cell {
        local.normalize_or(Vec3::NEG_Z) * 0.9
    } else {
        crate::baked_model::nearest_body_surface(originals, body, local)
    })
}

/// End a palette carry. A part that was carried out and dropped back onto
/// the palette is returned: the pre-purchase state comes back exactly and
/// the purchase's undo entry is dropped with it. Anywhere else it stays.
fn release_carry(
    editor: &mut EditorState,
    run: &mut RunProgress,
    stage: &GameStage,
    over_palette: bool,
) {
    let Some(carry) = editor.carry.take() else {
        return;
    };
    if carry.left_palette && over_palette {
        if editor.undo.last().is_some_and(|last| {
            active_parts(last, stage) == active_parts(&carry.before, stage)
                && last.editor_dna == carry.before.editor_dna
        }) {
            editor.undo.pop();
        }
        *run = carry.before;
        run.request_save();
        editor.message = "Part returned to the palette".into();
    }
    editor.visuals_dirty = true;
    editor.dirty = true;
}

/// Move a part just bought from the palette with the cursor. Releasing over
/// the 3D view keeps it there; carrying it out and back onto the palette
/// returns it, restoring the exact pre-purchase state and its undo entry.
fn carry_part(
    mouse: &ButtonInput<MouseButton>,
    cursor: Option<Vec2>,
    layout: &CreatorLayout,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    pt: &Transform,
    stage: &GameStage,
    editor: &mut EditorState,
    run: &mut RunProgress,
    originals: &Originals,
) {
    if !mouse.pressed(MouseButton::Left) {
        release_carry(
            editor,
            run,
            stage,
            cursor.is_some_and(|c| layout.over_palette(c)),
        );
        return;
    }
    let Some(cursor) = cursor.filter(|c| !layout.over_ui(*c)) else {
        return;
    };
    let snap = !editor.free_placement;
    let body = run.body.clone();
    let Some(local) = body_point_under(
        camera,
        camera_transform,
        cursor,
        pt,
        stage,
        snap,
        originals,
        &body,
    ) else {
        return;
    };
    let Some(carry) = editor.carry.as_mut() else {
        return;
    };
    carry.left_palette = true;
    let index = carry.part;
    if let Some(p) = active_parts_mut(run, stage).get_mut(index) {
        let slot = if *stage == GameStage::Cell {
            cell_slot(p.bone)
        } else {
            crate::baked_model::attachment_point(originals, &body, p.bone)
        };
        p.offset = game::clamp_offset((local - slot).to_array(), game::OFFSET_LIMIT);
        sync_pair(run, stage, index);
        run.sync_legacy_accessory();
        run.request_save();
        editor.visuals_dirty = true;
    }
}

pub fn refresh_parts(
    mut commands: Commands,
    state: Res<State<GameStage>>,
    mut editor: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<
        Entity,
        Or<(
            With<CellPart>,
            With<crate::creature::Accessory>,
            With<SpineHandle>,
        )>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut player: Query<(Entity, &Transform, &mut Health, &mut Mover, &mut Mesh3d), With<Player>>,
    mut stats: ResMut<crate::creature::CreatureStatsRes>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut paint: Option<ResMut<crate::cell_paint::CellPaintAssets>>,
) {
    let stage = state.get();
    // The rigid editors draw their own blocks (`rigid_editor`).
    if editor.rigid {
        return;
    }
    let outfitting = crate::outfitter::routes(stage) && crate::outfitter::in_session(&run);
    if !(matches!(
        stage,
        GameStage::Cell | GameStage::Creature | GameStage::MainMenu
    ) || outfitting)
        || (*stage == GameStage::MainMenu && !editor.open)
    {
        return;
    }
    let parts = active_parts(&run, stage);
    let body_changed = editor.last_body.as_ref() != Some(&run.body);
    if !editor.visuals_dirty
        && parts == editor.last_parts
        && editor.last_body.as_ref() == Some(&run.body)
    {
        return;
    }
    let Ok((entity, transform, mut hp, mut mover, mut body_mesh)) = player.single_mut() else {
        return;
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    if editor.open && editor.body_mode && *stage != GameStage::Cell {
        let handle_mesh = meshes.add(Sphere::new(0.13));
        let normal = materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 1.),
            emissive: Color::srgb(0.08, 0.3, 0.4).into(),
            ..default()
        });
        let selected = materials.add(StandardMaterial {
            base_color: Color::srgb(1., 0.6, 0.15),
            emissive: Color::srgb(0.4, 0.15, 0.).into(),
            ..default()
        });
        commands.entity(entity).with_children(|parent| {
            for (i, _p) in run.body.spine.iter().enumerate() {
                parent.spawn((
                    StageOwned,
                    SpineHandle,
                    Mesh3d(handle_mesh.clone()),
                    MeshMaterial3d(if i == editor.focused_spine {
                        selected.clone()
                    } else {
                        normal.clone()
                    }),
                    Transform::from_translation(crate::baked_model::control_handle(
                        &originals, &run.body, i,
                    )),
                ));
            }
        });
    }
    for (i, p) in parts.iter().enumerate() {
        let Some(model) = model_for_part(&originals, p) else {
            continue;
        };
        let material = model.material.clone().unwrap_or_else(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.65, 0.8, 0.45),
                ..default()
            })
        });
        if *stage == GameStage::Cell {
            // Each mounted cell part draws its own texture (per rigblock instance, `cell_paint`).
            let material = match (paint.as_deref_mut(), images.as_deref_mut()) {
                (Some(paint), Some(images)) => paint
                    .instance_material(i, model, &originals, &mut materials, images)
                    .unwrap_or(material),
                _ => material,
            };
            let local = cell_slot(p.bone) + Vec3::from_array(p.offset);
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    StageOwned,
                    CellPart,
                    Mesh3d(model.draw_mesh()),
                    MeshMaterial3d(material),
                    Transform::from_translation(local)
                        .with_rotation(Quat::from_rotation_y(p.yaw))
                        .with_scale(p.mesh_scale(model.scale * 0.35 * p.scale)),
                ));
            });
        } else {
            let slot = crate::baked_model::attachment_point(&originals, &run.body, p.bone);
            commands.spawn((
                StageOwned,
                crate::creature::Accessory {
                    part: i,
                    slot_local: slot,
                },
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(material),
                Transform::from_translation(
                    transform.translation
                        + transform.rotation
                            * ((slot + Vec3::from_array(p.offset)) * transform.scale),
                )
                .with_rotation(transform.rotation * p.rotation())
                .with_scale(p.mesh_scale(model.scale * 0.5 * p.scale) * transform.scale),
            ));
        }
    }
    editor.last_parts = parts.to_vec();
    editor.last_body = Some(run.body.clone());
    editor.visuals_dirty = false;
    if *stage != GameStage::Cell {
        if body_changed {
            if let Some(template) = &originals.baked_body {
                crate::baked_model::update_body(template, &run.body, &mut meshes);
            } else {
                let replacement = crate::body::mesh(&run.body);
                if meshes.get(&body_mesh.0).is_some() {
                    *meshes.get_mut(&body_mesh.0).unwrap() = replacement;
                } else {
                    body_mesh.0 = meshes.add(replacement);
                }
            }
        }
        // The creature's own stats do not change in an outfitter.
        if outfitting {
            return;
        }
        let s = crate::creature::stats_for(&run, &originals);
        // The Creature avatar follows the retail rules (health table + speed
        // gear); `s` stays the lineage summary later stages inherit.
        let avatar = if *stage == GameStage::Creature {
            crate::creature::avatar_stats(&run, &originals)
        } else {
            s
        };
        let fraction = hp.hp / hp.max;
        hp.max = avatar.max_hp;
        hp.hp = hp.max * fraction;
        mover.speed = avatar.speed;
        stats.0 = s;
        run.stats = Some(s);
    } else {
        let a = originals
            .part_catalog
            .abilities_for_parts_in(asset_catalog::CELL_PARTS, &run.cell_parts);
        mover.speed =
            9. + a.cell_flagella as f32 + a.cell_cilia as f32 * 2. + a.cell_jet as f32 * 3.;
    }
}

const PALETTE_PAD: f32 = 12.;
const GRID_GAP: f32 = 8.;
const GRID_COLUMNS: usize = 3;
/// Padding, mode row, category tabs and divider above the part grid.
const PALETTE_HEADER: f32 = 146.;
/// Pager, toggles, selection box and padding below the part grid.
const PALETTE_FOOTER: f32 = 148.;
/// `palette-bckgrnd`'s single native colour (32, 58, 74 at 80%), used only
/// when that PNG is missing.
const PALETTE_COLOR: Color = Color::srgba(0.125, 0.227, 0.29, 0.8);
const INK: Color = Color::srgb(0.95, 0.96, 0.91);
const SOFT_INK: Color = Color::srgba(0.78, 0.87, 0.9, 0.86);
const WARN_INK: Color = Color::srgb(1., 0.58, 0.45);
const GOLD: Color = Color::srgb(1., 0.85, 0.32);
const ATTACK: Color = Color::srgb(0.74, 0.15, 0.13);
const SOCIAL: Color = Color::srgb(0.24, 0.6, 0.15);
const MOTION: Color = Color::srgb(0.11, 0.44, 0.76);

/// Screen regions of the creator UI. Layout, picking and scrolling share it,
/// so presses on a panel never fall through to 3D part picking.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CreatorLayout {
    pub palette: Rect,
    pub header: Rect,
    pub actions: Rect,
    pub abilities: Rect,
    rows: usize,
    cell: f32,
    icon: f32,
}

impl CreatorLayout {
    pub(crate) fn new(width: f32, height: f32, stage: &GameStage) -> Self {
        let width = width.max(640.);
        let height = height.max(480.);
        // Spore's palette is a tall left column of a little over a quarter
        // of the screen; the creature stands in the space to its right.
        let palette_w = (width * 0.28).clamp(300., 460.).round();
        let palette = Rect::new(0., 0., palette_w, height);
        // Name and DNA sit centred between the palette and the top-right
        // undo/redo/Done cluster.
        let actions = Rect::new(width - 270., 0., width, 96.);
        let free = (palette_w + 24., actions.min.x - 16.);
        let header_w = (free.1 - free.0).clamp(200., 430.);
        let center = (free.0 + free.1) * 0.5;
        let header = Rect::new(center - header_w * 0.5, 0., center + header_w * 0.5, 66.);
        let abilities_h = if *stage == GameStage::Cell {
            134.
        } else {
            266.
        };
        let abilities = Rect::new(
            width - 360.,
            height - abilities_h - 12.,
            width - 12.,
            height - 12.,
        );
        let inner = palette_w - PALETTE_PAD * 2.;
        let cell = ((inner - GRID_GAP * (GRID_COLUMNS - 1) as f32) / GRID_COLUMNS as f32).floor();
        let icon = (cell - 18.).clamp(48., 104.).floor();
        let available = height - PALETTE_HEADER - PALETTE_FOOTER;
        let rows = ((available + GRID_GAP) / (icon + 26. + GRID_GAP))
            .floor()
            .clamp(2., 6.) as usize;
        Self {
            palette,
            header,
            actions,
            abilities,
            rows,
            cell,
            icon,
        }
    }
    /// Regions of another creator UI (`creature_editor_ui`).
    pub(crate) fn retail(palette: Rect, header: Rect, actions: Rect, abilities: Rect) -> Self {
        Self {
            palette,
            header,
            actions,
            abilities,
            rows: 0,
            cell: 0.,
            icon: 0.,
        }
    }
    pub(crate) fn page_size(&self) -> usize {
        GRID_COLUMNS * self.rows
    }
    pub(crate) fn over_palette(&self, cursor: Vec2) -> bool {
        self.palette.contains(cursor)
    }
    pub(crate) fn over_ui(&self, cursor: Vec2) -> bool {
        [self.palette, self.header, self.actions, self.abilities]
            .iter()
            .any(|rect| rect.contains(cursor))
    }
    /// Logical pixels from the window centre to the centre of the 3D area
    /// right of the palette, where the creator's subject belongs.
    pub(crate) fn view_center_shift(&self) -> f32 {
        self.palette.max.x * 0.5
    }
}

fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Native art as a UI image with its verified source recorded.
fn art_image(
    image: &crate::editor_art::EditorChromeImage,
    role: &'static str,
    node: Node,
) -> impl Bundle {
    (
        ImageNode {
            image: image.image.clone(),
            image_mode: bevy::ui::widget::NodeImageMode::Stretch,
            ..default()
        },
        node,
        crate::editor_art::EditorArtSource {
            key: image.key,
            role,
        },
    )
}

fn pill_button(
    parent: &mut ChildSpawnerCommands,
    text: String,
    action: Action,
    color: Color,
    active: bool,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                flex_grow: 1.,
                flex_basis: Val::Px(0.),
                min_height: Val::Px(28.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(Val::Px(6.), Val::Px(3.)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(14.)),
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(if active {
                GOLD.with_alpha(0.8)
            } else {
                Color::srgba(0.7, 0.85, 0.9, 0.25)
            }),
        ))
        .with_children(|p| {
            p.spawn(label(text, 12., if active { GOLD } else { INK }));
        });
}

fn spine_button(parent: &mut ChildSpawnerCommands, text: &str, action: Action) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Percent(100.),
                flex_shrink: 0.,
                padding: UiRect::axes(Val::Px(10.), Val::Px(7.)),
                margin: UiRect::bottom(Val::Px(4.)),
                border_radius: BorderRadius::all(Val::Px(8.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.16, 0.22, 0.85)),
        ))
        .with_children(|p| {
            p.spawn(label(text, 13., INK));
        });
}

/// Mode switch in the palette's top row. The selected mode is display only;
/// the other one carries the toggle action.
fn mode_button(
    parent: &mut ChildSpawnerCommands,
    art: Option<&crate::editor_art::EditorChromeImage>,
    selected: bool,
    fallback: &str,
) {
    let size = 46.;
    let mut button = parent.spawn(Node {
        width: Val::Px(size),
        height: Val::Px(size),
        flex_shrink: 0.,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border_radius: BorderRadius::all(Val::Percent(50.)),
        ..default()
    });
    if !selected {
        button.insert((Button, Action::Body));
    }
    button.with_children(|p| match art {
        Some(image) => {
            p.spawn(art_image(
                image,
                "mode",
                Node {
                    width: Val::Px(size),
                    height: Val::Px(size),
                    ..default()
                },
            ));
        }
        None => {
            p.spawn(label(fallback, 11., if selected { GOLD } else { INK }));
        }
    });
}

fn category_tab(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    name: &str,
    catalog_category: &str,
    selected: bool,
    empty: bool,
    stage: &GameStage,
    originals: &Originals,
    representative: Option<crate::dbpf::ResourceKey>,
) {
    let art = &originals.editor_art;
    parent
        .spawn((
            Button,
            Action::Category(index),
            Node {
                width: Val::Px(46.),
                height: Val::Px(50.),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(if selected { 2. } else { 0. })),
                border_radius: BorderRadius::all(Val::Px(12.)),
                ..default()
            },
            BackgroundColor(if selected {
                Color::srgba(1., 0.85, 0.32, 0.12)
            } else {
                Color::NONE
            }),
            BorderColor::all(GOLD.with_alpha(0.7)),
        ))
        .with_children(|p| {
            if *stage == GameStage::Cell {
                // Cell tabs show the family's starter part from the original
                // `cellparticons` group, like the Cell palette's own rows.
                if let Some(definition) = representative {
                    p.spawn((
                        ImageNode {
                            color: if selected {
                                Color::WHITE
                            } else {
                                Color::srgba(0.85, 0.9, 0.92, 0.8)
                            },
                            ..default()
                        },
                        crate::part_thumbnails::ThumbnailRequest { definition },
                        Visibility::Inherited,
                        Node {
                            width: Val::Px(42.),
                            height: Val::Px(42.),
                            ..default()
                        },
                    ));
                    return;
                }
            } else if let Some(category) = art.category_by_application(catalog_category) {
                let state = if selected {
                    crate::editor_art::SELECTED_STATE
                } else if empty {
                    crate::editor_art::OFF_STATE
                } else {
                    crate::editor_art::NORMAL_STATE
                };
                if let Some(image) = art
                    .state_image(category, state)
                    .or_else(|| art.neutral_image(category))
                {
                    let scale = (44. / image.width.max(image.height).max(1) as f32).min(1.);
                    p.spawn((
                        ImageNode {
                            image: image.image.clone(),
                            ..default()
                        },
                        Node {
                            width: Val::Px(image.width as f32 * scale),
                            height: Val::Px(image.height as f32 * scale),
                            flex_shrink: 0.,
                            ..default()
                        },
                        crate::editor_art::EditorArtSource {
                            key: image.key,
                            role: if selected { "selected" } else { "neutral" },
                        },
                    ));
                    return;
                }
            }
            p.spawn(label(name, 10., if selected { GOLD } else { INK }));
        });
}

fn part_tile(
    parent: &mut ChildSpawnerCommands,
    layout: &CreatorLayout,
    part: &PartDefinition,
    locked: bool,
    affordable: bool,
    not_found: bool,
    art: &crate::editor_art::NativeEditorArt,
) {
    parent
        .spawn((
            Button,
            Action::Buy(part.key.instance_id),
            Node {
                width: Val::Px(layout.cell),
                height: Val::Px(layout.icon + 26.),
                flex_shrink: 0.,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::top(Val::Px(2.)),
                border_radius: BorderRadius::all(Val::Px(10.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.07, 0.1, 0.14)),
        ))
        .with_children(|tile| {
            // The installed palette icon (or a live render of the model).
            tile.spawn((
                ImageNode {
                    color: if locked {
                        Color::srgba(0.25, 0.28, 0.3, 0.75)
                    } else if !affordable {
                        Color::srgba(0.62, 0.66, 0.68, 0.7)
                    } else {
                        Color::WHITE
                    },
                    ..default()
                },
                crate::part_thumbnails::ThumbnailRequest {
                    definition: part.key,
                },
                Visibility::Inherited,
                Node {
                    width: Val::Px(layout.icon),
                    height: Val::Px(layout.icon),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
            if locked {
                if let Some(lock) = art.chrome("locked-item") {
                    let size = (layout.icon * 0.42).round();
                    tile.spawn(art_image(
                        lock,
                        "locked",
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(layout.icon * 0.5 - size * 0.5),
                            left: Val::Px((layout.cell - size) * 0.5),
                            width: Val::Px(size),
                            height: Val::Px(size),
                            ..default()
                        },
                    ));
                }
            }
            tile.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(3.),
                height: Val::Px(20.),
                ..default()
            })
            .with_children(|row| {
                if locked {
                    row.spawn(label(
                        if not_found {
                            "Not found yet".to_string()
                        } else {
                            format!("Level {}", part.unlock_level)
                        },
                        11.,
                        WARN_INK,
                    ));
                    return;
                }
                if let Some(dna) = art.chrome("edi_dna_icon") {
                    row.spawn(art_image(
                        dna,
                        "price",
                        Node {
                            width: Val::Px(10.),
                            height: Val::Px(13.),
                            ..default()
                        },
                    ));
                }
                row.spawn(label(
                    part.price.to_string(),
                    13.,
                    if affordable { INK } else { WARN_INK },
                ));
            });
        });
}

/// One ability: coloured disc with the native verb glyph, its name and
/// five level pips cropped from the native `CE-property-dots` strips.
fn ability_cell(
    parent: &mut ChildSpawnerCommands,
    art: &crate::editor_art::NativeEditorArt,
    name: &str,
    glyph: AbilityGlyph,
    tint: Color,
    level: u8,
) {
    let present = level > 0;
    parent
        .spawn(Node {
            width: Val::Px(54.),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(2.),
            ..default()
        })
        .with_children(|cell| {
            cell.spawn((
                Node {
                    width: Val::Px(34.),
                    height: Val::Px(34.),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(2.)),
                    border_radius: BorderRadius::all(Val::Percent(50.)),
                    ..default()
                },
                BackgroundColor(if present {
                    tint
                } else {
                    Color::srgba(0.12, 0.16, 0.19, 0.85)
                }),
                BorderColor::all(if present {
                    Color::srgba(1., 1., 1., 0.55)
                } else {
                    Color::srgba(0.6, 0.7, 0.75, 0.25)
                }),
            ))
            .with_children(|disc| match glyph {
                AbilityGlyph::Native(file) => {
                    if let Some(image) = art.chrome(file) {
                        disc.spawn((
                            ImageNode {
                                image: image.image.clone(),
                                color: Color::WHITE.with_alpha(if present { 1. } else { 0.35 }),
                                ..default()
                            },
                            Node {
                                width: Val::Px(24.),
                                height: Val::Px(24.),
                                ..default()
                            },
                            crate::editor_art::EditorArtSource {
                                key: image.key,
                                role: "ability",
                            },
                        ));
                    }
                }
                AbilityGlyph::Part(definition) => {
                    disc.spawn((
                        ImageNode {
                            color: Color::WHITE.with_alpha(if present { 1. } else { 0.4 }),
                            ..default()
                        },
                        crate::part_thumbnails::ThumbnailRequest { definition },
                        Visibility::Inherited,
                        Node {
                            width: Val::Px(30.),
                            height: Val::Px(30.),
                            ..default()
                        },
                    ));
                }
            });
            cell.spawn(label(
                name,
                10.,
                if present {
                    INK
                } else {
                    SOFT_INK.with_alpha(0.45)
                },
            ));
            cell.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(1.),
                ..default()
            })
            .with_children(|pips| {
                let full = art.chrome("CE-property-dots-full");
                let empty = art.chrome("CE-property-dots-empty");
                for pip in 0..5u8 {
                    let lit = pip < level.min(5);
                    // Both native strips hold ten dots at a 14-pixel pitch.
                    let source = if lit { full } else { empty };
                    match source {
                        Some(image) => {
                            let pitch = image.width as f32 / 10.;
                            pips.spawn((
                                ImageNode {
                                    image: image.image.clone(),
                                    rect: Some(Rect::new(0., 0., pitch, image.height as f32)),
                                    ..default()
                                },
                                Node {
                                    width: Val::Px(8.),
                                    height: Val::Px(8.),
                                    ..default()
                                },
                            ));
                        }
                        None => {
                            pips.spawn((
                                Node {
                                    width: Val::Px(6.),
                                    height: Val::Px(6.),
                                    border_radius: BorderRadius::all(Val::Percent(50.)),
                                    ..default()
                                },
                                BackgroundColor(if lit {
                                    GOLD
                                } else {
                                    Color::srgba(1., 1., 1., 0.15)
                                }),
                            ));
                        }
                    }
                }
            });
        });
}

#[derive(Clone, Copy)]
enum AbilityGlyph {
    Native(&'static str),
    Part(crate::dbpf::ResourceKey),
}

/// Pronounceable session names for the name field's dice button.
fn random_name(seed: u32, cell: bool, salt: u32) -> String {
    const START: &[&str] = &[
        "Bo", "Ze", "Gri", "Ka", "Plu", "Mo", "Fli", "Wu", "Zor", "Bla", "Ti", "Quo", "Ny", "Dro",
        "Sku", "Hap",
    ];
    const MIDDLE: &[&str] = &[
        "la", "ri", "bo", "gu", "ma", "zi", "to", "ne", "pa", "lo", "ki", "ru",
    ];
    const END: &[&str] = &[
        "ps", "x", "b", "gon", "tor", "lie", "ma", "zor", "th", "nk", "rp", "o", "ly", "us",
    ];
    let mut hash = seed.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B) ^ 0x5F3C;
    let mut pick = |list: &[&'static str]| {
        hash ^= hash >> 15;
        hash = hash.wrapping_mul(0x2C1B_3C6D);
        hash ^= hash >> 12;
        list[hash as usize % list.len()]
    };
    let mut name = pick(START).to_string();
    for _ in 0..if cell { 1 } else { 1 + seed % 2 } {
        name.push_str(pick(MIDDLE));
    }
    name.push_str(pick(END));
    name
}

pub fn ui(
    mut commands: Commands,
    mut editor: ResMut<EditorState>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    state: Res<State<GameStage>>,
    shells: Query<Entity, With<PaletteShell>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
    windows: Query<&Window>,
) {
    if !editor.dirty {
        return;
    }
    editor.dirty = false;
    for e in &shells {
        commands.entity(e).despawn();
    }
    if !editor.open
        || crate::cell_editor::routes(state.get())
        || crate::creature_editor::routes(state.get())
    {
        return;
    }
    let stage = state.get();
    let (window_width, window_height) = windows
        .single()
        .map(|window| (window.width(), window.height()))
        .unwrap_or((1280., 768.));
    let layout = CreatorLayout::new(window_width, window_height, stage);
    let tabs = categories(stage);
    editor.category = editor.category.min(tabs.len() - 1);
    let parts_of = |category: &str| {
        let mut variants = std::collections::HashSet::new();
        originals
            .part_catalog
            .parts
            .iter()
            .filter(|p| p.key.group_id == group(stage) && p.category() == category && !p.is_null)
            .filter(|p| variants.insert((&p.name, p.price, p.unlock_level, p.abilities)))
            .collect::<Vec<_>>()
    };
    let list = parts_of(tabs[editor.category].1);
    let page_size = layout.page_size();
    let pages = list.len().div_ceil(page_size).max(1);
    editor.page = editor.page.min(pages - 1);
    let camera = cameras.single().ok();
    let art = &originals.editor_art;
    let creature = matches!(stage, GameStage::Creature | GameStage::MainMenu);
    let body_mode = editor.body_mode && creature;

    // Left palette: modes, category tabs, part grid and footer.
    let mut palette = commands.spawn((
        PaletteShell,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            top: Val::Px(0.),
            width: Val::Px(layout.palette.width()),
            height: Val::Px(layout.palette.height()),
            padding: UiRect::all(Val::Px(PALETTE_PAD)),
            row_gap: Val::Px(6.),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        GlobalZIndex(10),
    ));
    if art.chrome("palette-bckgrnd").is_none() {
        palette.insert(BackgroundColor(PALETTE_COLOR));
    }
    if let Some(camera) = camera {
        palette.insert(bevy::ui::UiTargetCamera(camera));
    }
    palette.with_children(|p| {
        // Spore's palette sits over its dark editor room; a light shade under
        // the native translucent colour keeps it that dark over the brighter
        // adapted backdrops.
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                top: Val::Px(0.),
                width: Val::Percent(100.),
                height: Val::Percent(100.),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.03, 0.05, 0.38)),
        ));
        if let Some(background) = art.chrome("palette-bckgrnd") {
            p.spawn(art_image(
                background,
                "palette",
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.),
                    top: Val::Px(0.),
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..default()
                },
            ));
        }
        // Native curved flares on the palette's right edge.
        let flare = (layout.palette.height() / 900.).clamp(0.5, 1.) * 0.62;
        for (name, top) in [("palette-curve-top", true), ("palette-curve-bottom", false)] {
            if let Some(image) = art.chrome(name) {
                let node = Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(100.),
                    top: if top { Val::Px(0.) } else { Val::Auto },
                    bottom: if top { Val::Auto } else { Val::Px(0.) },
                    width: Val::Px((image.width as f32 * flare).round()),
                    height: Val::Px((image.height as f32 * flare).round()),
                    ..default()
                };
                // The same shade as the panel body, in the flare's own shape.
                p.spawn((
                    ImageNode {
                        image: image.image.clone(),
                        color: Color::srgba(0., 0., 0., 0.38),
                        image_mode: bevy::ui::widget::NodeImageMode::Stretch,
                        ..default()
                    },
                    node.clone(),
                ));
                p.spawn(art_image(image, "palette edge", node));
            }
        }
        // Mode row with the current section title.
        p.spawn(Node {
            width: Val::Percent(100.),
            height: Val::Px(50.),
            flex_shrink: 0.,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.),
            ..default()
        })
        .with_children(|row| {
            mode_button(
                row,
                art.chrome(if body_mode {
                    "button_mode-build-norm"
                } else {
                    "button_mode-build-on"
                }),
                !body_mode,
                "Build",
            );
            if creature {
                mode_button(
                    row,
                    art.chrome(if body_mode {
                        "button-layer-on-norm"
                    } else {
                        "button-layer-norm"
                    }),
                    body_mode,
                    "Body",
                );
            }
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::left(Val::Px(4.)),
                ..default()
            })
            .with_children(|title| {
                title.spawn(label(
                    if *stage == GameStage::Cell {
                        "CELL CREATOR"
                    } else {
                        "CREATURE CREATOR"
                    },
                    15.,
                    INK,
                ));
                title.spawn(label(
                    if body_mode {
                        "Body shape".to_string()
                    } else {
                        format!("Build: {}", tabs[editor.category].0)
                    },
                    12.,
                    SOFT_INK,
                ));
            });
        });
        // Category tabs.
        p.spawn((
            Node {
                width: Val::Percent(100.),
                height: Val::Px(58.),
                flex_shrink: 0.,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: if tabs.len() > 4 {
                    JustifyContent::SpaceBetween
                } else {
                    JustifyContent::SpaceEvenly
                },
                padding: UiRect::axes(Val::Px(2.), Val::Px(4.)),
                border_radius: BorderRadius::all(Val::Px(14.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.04, 0.06, 0.32)),
        ))
        .with_children(|row| {
            for (i, (name, category)) in tabs.iter().enumerate() {
                let parts = parts_of(category);
                category_tab(
                    row,
                    i,
                    name,
                    category,
                    i == editor.category && !body_mode,
                    parts.is_empty(),
                    stage,
                    &originals,
                    parts.first().map(|p| p.key),
                );
            }
        });
        p.spawn((
            Node {
                width: Val::Percent(100.),
                height: Val::Px(1.),
                flex_shrink: 0.,
                margin: UiRect::vertical(Val::Px(2.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.7, 0.85, 0.9, 0.18)),
        ));
        // Part grid, or the spine controls while shaping the body.
        p.spawn((
            PaletteRoot,
            Node {
                width: Val::Percent(100.),
                flex_grow: 1.,
                min_height: Val::Px(0.),
                flex_direction: if body_mode {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                },
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                column_gap: Val::Px(GRID_GAP),
                row_gap: Val::Px(GRID_GAP),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition(Vec2::new(0., if body_mode { editor.scroll_offset } else { 0. })),
        ))
        .with_children(|p| {
            if body_mode {
                let spine = run.body.spine.get(editor.focused_spine);
                p.spawn((
                    label(
                        format!(
                            "Spine point {} of {}\nRadius {:.2}   Height {:.2}\nDrag a spine handle to shape the body",
                            editor.focused_spine + 1,
                            run.body.spine.len(),
                            spine.map(|p| p.radius).unwrap_or(0.),
                            spine.map(|p| p.position[1]).unwrap_or(0.)
                        ),
                        13.,
                        INK,
                    ),
                    Node {
                        margin: UiRect::bottom(Val::Px(6.)),
                        ..default()
                    },
                ));
                for (text, action) in [
                    ("Next spine point", Action::Spine),
                    ("Thicker", Action::Thickness(0.1)),
                    ("Thinner", Action::Thickness(-0.1)),
                    ("Raise spine", Action::Lift(0.1)),
                    ("Lower spine", Action::Lift(-0.1)),
                    ("Move spine forward", Action::Length(-0.1)),
                    ("Move spine backward", Action::Length(0.1)),
                    ("Add spine point", Action::AddSpine),
                    ("Remove spine point", Action::RemoveSpine),
                ] {
                    spine_button(p, text, action);
                }
            } else if list.is_empty() {
                p.spawn(label(
                    "No original parts in this category",
                    13.,
                    SOFT_INK,
                ));
            } else {
                for part in list.iter().skip(editor.page * page_size).take(page_size) {
                    let locked = part_locked(&run, stage, part);
                    part_tile(
                        p,
                        &layout,
                        part,
                        locked,
                        part.price as f32 <= run.editor_dna,
                        matches!(stage, GameStage::Cell | GameStage::Creature),
                        art,
                    );
                }
            }
        });
        // Pager.
        p.spawn(Node {
            width: Val::Percent(100.),
            height: Val::Px(34.),
            flex_shrink: 0.,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(14.),
            display: if body_mode { Display::None } else { Display::Flex },
            ..default()
        })
        .with_children(|row| {
            for (name, step) in [("CE_button-back-norm", -1), ("CE_button-forward-norm", 1)] {
                let enabled = if step < 0 {
                    editor.page > 0
                } else {
                    editor.page + 1 < pages
                };
                let mut button = row.spawn((
                    Button,
                    Action::Page(step),
                    Node {
                        width: Val::Px(30.),
                        height: Val::Px(32.),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ));
                button.with_children(|b| match art.chrome(name) {
                    Some(image) => {
                        b.spawn((
                            ImageNode {
                                image: image.image.clone(),
                                color: Color::WHITE.with_alpha(if enabled { 1. } else { 0.3 }),
                                ..default()
                            },
                            Node {
                                width: Val::Px(21.),
                                height: Val::Px(29.),
                                ..default()
                            },
                            crate::editor_art::EditorArtSource {
                                key: image.key,
                                role: "page",
                            },
                        ));
                    }
                    None => {
                        b.spawn(label(if step < 0 { "<" } else { ">" }, 18., INK));
                    }
                });
                if step < 0 {
                    row.spawn(label(
                        format!("Page {} of {pages}", editor.page + 1),
                        13.,
                        SOFT_INK,
                    ));
                }
            }
        });
        // Placement toggles and selling.
        p.spawn(Node {
            width: Val::Percent(100.),
            flex_shrink: 0.,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(6.),
            ..default()
        })
        .with_children(|row| {
            pill_button(
                row,
                format!("Symmetry {}", if editor.asymmetric { "off" } else { "on" }),
                Action::Symmetry,
                Color::srgba(0.04, 0.12, 0.16, 0.85),
                !editor.asymmetric,
            );
            pill_button(
                row,
                format!("Snap {}", if editor.free_placement { "off" } else { "on" }),
                Action::SurfaceSnap,
                Color::srgba(0.04, 0.12, 0.16, 0.85),
                !editor.free_placement,
            );
            pill_button(
                row,
                "Sell part".into(),
                Action::Remove,
                Color::srgba(0.36, 0.1, 0.08, 0.85),
                false,
            );
        });
        // Selected part and the latest editor message.
        let selected = active_parts(&run, stage)
            .get(run.focused_part as usize)
            .and_then(|p| {
                originals
                    .part_catalog
                    .by_part_in(group(stage), p)
                    .map(|d| (d.name.clone(), d.abilities.summary()))
            });
        p.spawn((
            Node {
                width: Val::Percent(100.),
                height: Val::Px(58.),
                flex_shrink: 0.,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(10.), Val::Px(6.)),
                row_gap: Val::Px(2.),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(Val::Px(10.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.04, 0.06, 0.45)),
        ))
        .with_children(|info| {
            match selected {
                Some((name, abilities)) => {
                    info.spawn(label(name, 13., GOLD));
                    if !abilities.is_empty() {
                        info.spawn(label(abilities, 11., SOFT_INK));
                    }
                }
                None => {
                    info.spawn(label("Drag a part onto your creation", 13., SOFT_INK));
                }
            }
            info.spawn((
                Button,
                Action::Focus,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(8.),
                    top: Val::Px(6.),
                    padding: UiRect::axes(Val::Px(6.), Val::Px(2.)),
                    border_radius: BorderRadius::all(Val::Px(8.)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1., 1., 1., 0.08)),
            ))
            .with_children(|b| {
                b.spawn(label("Next (Tab)", 10., SOFT_INK));
            });
        });
    });

    // Top bar: name field with the dice and the DNA budget.
    let (default_name, prompt) = if *stage == GameStage::Cell {
        (art.cell_default_name.clone(), art.cell_name_prompt.clone())
    } else {
        (
            art.creature_default_name.clone(),
            art.creature_name_prompt.clone(),
        )
    };
    // An unnamed creation shows the native prompt, dimmed, like an empty
    // text field; the dice fills in a name.
    let (name, named) = match editor.name.clone().or(default_name) {
        Some(name) => (name, true),
        None => (
            prompt.unwrap_or_else(|| {
                if *stage == GameStage::Cell {
                    "Name your cell".into()
                } else {
                    "Name your creature".into()
                }
            }),
            false,
        ),
    };
    let mut header = commands.spawn((
        PaletteShell,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(layout.header.min.x),
            top: Val::Px(10.),
            width: Val::Px(layout.header.width()),
            height: Val::Px(48.),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.),
            ..default()
        },
        GlobalZIndex(10),
    ));
    if let Some(camera) = camera {
        header.insert(bevy::ui::UiTargetCamera(camera));
    }
    header.with_children(|bar| {
        bar.spawn((
            Node {
                flex_grow: 1.,
                height: Val::Px(36.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(Val::Px(14.), Val::Px(2.)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(18.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.05, 0.08, 0.78)),
            BorderColor::all(Color::srgba(0.62, 0.78, 0.86, 0.45)),
        ))
        .with_children(|field| {
            field.spawn(label(
                name,
                if named { 17. } else { 15. },
                if named { INK } else { SOFT_INK.with_alpha(0.6) },
            ));
        });
        bar.spawn((
            Button,
            Action::RandomName,
            Node {
                width: Val::Px(34.),
                height: Val::Px(26.),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|b| match art.chrome("dice") {
            Some(image) => {
                // The native strip holds four 32-pixel button states.
                b.spawn((
                    ImageNode {
                        image: image.image.clone(),
                        rect: Some(Rect::new(
                            0.,
                            0.,
                            image.width as f32 / 4.,
                            image.height as f32,
                        )),
                        ..default()
                    },
                    Node {
                        width: Val::Px(32.),
                        height: Val::Px(23.),
                        ..default()
                    },
                    crate::editor_art::EditorArtSource {
                        key: image.key,
                        role: "random name",
                    },
                ));
            }
            None => {
                b.spawn(label("?", 16., INK));
            }
        });
        bar.spawn(Node {
            width: Val::Px(110.),
            height: Val::Px(46.),
            flex_shrink: 0.,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|dna| {
            match art.chrome("CE_evo_points_bg") {
                Some(image) => {
                    dna.spawn(art_image(
                        image,
                        "dna",
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.),
                            top: Val::Px(0.),
                            width: Val::Px(110.),
                            height: Val::Px(46.),
                            ..default()
                        },
                    ));
                }
                None => {
                    dna.spawn(label("DNA", 12., SOFT_INK));
                }
            }
            dna.spawn((
                label(format!("{:.0}", run.editor_dna), 18., INK),
                Node {
                    margin: UiRect::left(Val::Px(42.)),
                    ..default()
                },
            ));
        });
    });

    // Top right: undo, redo and Done.
    let mut actions = commands.spawn((
        PaletteShell,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(14.),
            top: Val::Px(10.),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.),
            ..default()
        },
        GlobalZIndex(10),
    ));
    if let Some(camera) = camera {
        actions.insert(bevy::ui::UiTargetCamera(camera));
    }
    actions.with_children(|bar| {
        for (action, normal, off, fallback, enabled) in [
            (
                Action::Undo,
                "CE_button-undo-norm",
                "CE_button-undo-off",
                "Undo",
                !editor.undo.is_empty(),
            ),
            (
                Action::Redo,
                "CE_button-redo-norm",
                "CE_button-redo-off",
                "Redo",
                !editor.redo.is_empty(),
            ),
        ] {
            bar.spawn((
                Button,
                action,
                Node {
                    width: Val::Px(38.),
                    height: Val::Px(38.),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_children(|b| match art.chrome(if enabled { normal } else { off }) {
                Some(image) => {
                    b.spawn(art_image(
                        image,
                        "history",
                        Node {
                            width: Val::Px(36.),
                            height: Val::Px(36.),
                            ..default()
                        },
                    ));
                }
                None => {
                    b.spawn(label(fallback, 11., INK));
                }
            });
        }
        bar.spawn((
            Button,
            Action::Done,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.),
                padding: UiRect {
                    left: Val::Px(4.),
                    right: Val::Px(16.),
                    top: Val::Px(4.),
                    bottom: Val::Px(4.),
                },
                border: UiRect::all(Val::Px(2.)),
                border_radius: BorderRadius::all(Val::Px(32.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.16, 0.3, 0.92)),
            BorderColor::all(Color::srgba(0.55, 0.85, 1., 0.65)),
        ))
        .with_children(|b| {
            if let Some(image) = art.chrome("button_mode-play-norm") {
                b.spawn(art_image(
                    image,
                    "done",
                    Node {
                        width: Val::Px(56.),
                        height: Val::Px(56.),
                        ..default()
                    },
                ));
            }
            b.spawn(label("DONE", 20., INK));
        });
    });

    // Bottom right: ability summary with native glyphs and level pips.
    let abilities = originals
        .part_catalog
        .abilities_for_parts_in(group(stage), active_parts(&run, stage));
    let (diet_icon, diet) = match (abilities.herbivore > 0, abilities.carnivore > 0) {
        (true, true) => ("stats_omni_icon", "Omnivore"),
        (true, false) => ("stats_veg_icon", "Herbivore"),
        (false, true) => ("stats_meat_icon", "Carnivore"),
        (false, false) => ("", "No mouth"),
    };
    let rows: Vec<Vec<(&str, AbilityGlyph, Color, u8)>> = if *stage == GameStage::Cell {
        let part_of = |pick: fn(&Capabilities) -> u8| {
            originals
                .part_catalog
                .parts
                .iter()
                .find(|p| {
                    p.key.group_id == asset_catalog::CELL_PARTS
                        && !p.is_null
                        && pick(&p.abilities) > 0
                })
                .map(|p| AbilityGlyph::Part(p.key))
                .unwrap_or(AbilityGlyph::Native("atk_strike"))
        };
        vec![vec![
            (
                "Speed",
                AbilityGlyph::Native("stats_speedPartInfo_icon"),
                MOTION,
                abilities
                    .cell_flagella
                    .max(abilities.cell_cilia)
                    .max(abilities.cell_jet),
            ),
            (
                "Spike",
                part_of(|a| a.cell_spike),
                ATTACK,
                abilities.cell_spike,
            ),
            (
                "Poison",
                part_of(|a| a.cell_poison),
                ATTACK,
                abilities.cell_poison,
            ),
            (
                "Electric",
                part_of(|a| a.cell_electric),
                ATTACK,
                abilities.cell_electric,
            ),
            (
                "Sight",
                AbilityGlyph::Native("mv_sight"),
                SOCIAL,
                abilities.eye,
            ),
            ("Jet", part_of(|a| a.cell_jet), MOTION, abilities.cell_jet),
        ]]
    } else {
        vec![
            vec![
                (
                    "Bite",
                    AbilityGlyph::Native("atk_bite"),
                    ATTACK,
                    abilities.bite,
                ),
                (
                    "Charge",
                    AbilityGlyph::Native("atk_charge"),
                    ATTACK,
                    abilities.charge,
                ),
                (
                    "Strike",
                    AbilityGlyph::Native("atk_strike"),
                    ATTACK,
                    abilities.strike,
                ),
                (
                    "Spit",
                    AbilityGlyph::Native("atk_spit"),
                    ATTACK,
                    abilities.spit,
                ),
            ],
            vec![
                (
                    "Sing",
                    AbilityGlyph::Native("social_vocalize"),
                    SOCIAL,
                    abilities.sing,
                ),
                (
                    "Dance",
                    AbilityGlyph::Native("social_dance"),
                    SOCIAL,
                    abilities.dance,
                ),
                (
                    "Charm",
                    AbilityGlyph::Native("social_gesture"),
                    SOCIAL,
                    abilities.charm,
                ),
                (
                    "Pose",
                    AbilityGlyph::Native("social_poses"),
                    SOCIAL,
                    abilities.pose,
                ),
            ],
            vec![
                (
                    "Speed",
                    AbilityGlyph::Native("stats_speedPartInfo_icon"),
                    MOTION,
                    abilities.speed,
                ),
                (
                    "Health",
                    AbilityGlyph::Native("stats_healthPartInfo_icon"),
                    MOTION,
                    abilities.health,
                ),
                (
                    "Sprint",
                    AbilityGlyph::Native("mv_energy"),
                    MOTION,
                    abilities.sprint,
                ),
                (
                    "Sneak",
                    AbilityGlyph::Native("mv_sneak"),
                    MOTION,
                    abilities.sneak,
                ),
                (
                    "Jump",
                    AbilityGlyph::Native("mv_jump"),
                    MOTION,
                    abilities.jump,
                ),
                (
                    "Glide",
                    AbilityGlyph::Native("mv_glide"),
                    MOTION,
                    abilities.glide,
                ),
            ],
        ]
    };
    let mut panel = commands.spawn((
        PaletteShell,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(layout.abilities.min.x),
            top: Val::Px(layout.abilities.min.y),
            width: Val::Px(layout.abilities.width()),
            height: Val::Px(layout.abilities.height()),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(14.), Val::Px(10.)),
            row_gap: Val::Px(6.),
            ..default()
        },
        GlobalZIndex(10),
    ));
    if art.chrome("panel-text").is_none() {
        panel.insert(BackgroundColor(Color::srgba(0.02, 0.07, 0.09, 0.8)));
    }
    if let Some(camera) = camera {
        panel.insert(bevy::ui::UiTargetCamera(camera));
    }
    panel.with_children(|p| {
        if let Some(image) = art.chrome("panel-text") {
            p.spawn((
                ImageNode {
                    image: image.image.clone(),
                    image_mode: bevy::ui::widget::NodeImageMode::Sliced(
                        bevy::sprite::TextureSlicer {
                            border: bevy::sprite::BorderRect::all(24.),
                            ..default()
                        },
                    ),
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.),
                    top: Val::Px(0.),
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..default()
                },
                crate::editor_art::EditorArtSource {
                    key: image.key,
                    role: "abilities panel",
                },
            ));
        }
        p.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.),
            height: Val::Px(30.),
            ..default()
        })
        .with_children(|title| {
            if let Some(image) = art.chrome(diet_icon) {
                title.spawn(art_image(
                    image,
                    "diet",
                    Node {
                        width: Val::Px(image.width as f32 * 0.8),
                        height: Val::Px(image.height as f32 * 0.8),
                        ..default()
                    },
                ));
            }
            title.spawn(label("ABILITIES", 14., INK));
            title.spawn(label(diet, 12., SOFT_INK));
        });
        for row in rows {
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(0.),
                ..default()
            })
            .with_children(|row_node| {
                for (name, glyph, tint, level) in row {
                    ability_cell(row_node, art, name, glyph, tint, level);
                }
            });
        }
    });

    // Short controls reminder along the bottom of the 3D area.
    let mut hint = commands.spawn((
        PaletteShell,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(layout.palette.max.x + 18.),
            bottom: Val::Px(12.),
            max_width: Val::Px((layout.abilities.min.x - layout.palette.max.x - 36.).max(120.)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        GlobalZIndex(9),
    ));
    if let Some(camera) = camera {
        hint.insert(bevy::ui::UiTargetCamera(camera));
    }
    hint.with_children(|p| {
        if !editor.message.is_empty() {
            p.spawn((
                label(editor.message.clone(), 14., GOLD),
                TextShadow {
                    offset: Vec2::splat(1.),
                    color: Color::srgba(0., 0., 0., 0.7),
                },
                Node {
                    margin: UiRect::bottom(Val::Px(4.)),
                    ..default()
                },
            ));
        }
        p.spawn((
            label(
                if body_mode {
                    "Drag the glowing handles to shape the spine   |   Right-drag to orbit, wheel to zoom"
                } else {
                    "Drag parts from the palette onto the body   |   Click a part to select, drag to move\nR rotate   +/- scale   . slot   Backspace sell   Right-drag orbit   Wheel zoom"
                },
                11.,
                SOFT_INK.with_alpha(0.7),
            ),
            TextShadow {
                offset: Vec2::splat(1.),
                color: Color::srgba(0., 0., 0., 0.6),
            },
        ));
    });
}

/// Mouse wheel: over the palette it pages through parts (or scrolls the
/// spine controls); over the 3D view it zooms the creator camera.
pub fn scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window>,
    state: Res<State<GameStage>>,
    mut editor: ResMut<EditorState>,
    mut roots: Query<(&mut ScrollPosition, &ComputedNode), With<PaletteRoot>>,
) {
    let Ok(window) = windows.single() else {
        wheel.clear();
        return;
    };
    if crate::cell_editor::routes(state.get()) || crate::creature_editor::routes(state.get()) {
        wheel.clear();
        return;
    }
    let layout = CreatorLayout::new(window.width(), window.height(), state.get());
    let cursor = window.cursor_position();
    let over_palette = cursor.is_some_and(|p| layout.over_palette(p));
    let over_ui = cursor.is_some_and(|p| layout.over_ui(p));
    for event in wheel.read() {
        if !editor.open {
            continue;
        }
        if !over_palette {
            if !over_ui {
                editor.orbit_zoom = (editor.orbit_zoom - event.y * 0.3).clamp(-3.4, 8.6);
            }
            continue;
        }
        let delta = event.y
            * if event.unit == bevy::input::mouse::MouseScrollUnit::Line {
                28.
            } else {
                1.
            };
        if !editor.body_mode {
            // Pixel-precise wheels report many small steps; flip one page
            // per notch-sized amount.
            editor.scroll_offset += delta;
            if editor.scroll_offset.abs() >= 28. {
                let step = -editor.scroll_offset.signum() as isize;
                editor.scroll_offset = 0.;
                editor.page = editor.page.saturating_add_signed(step);
                editor.dirty = true;
            }
            continue;
        }
        for (mut scroll, node) in &mut roots {
            let max = (node.content_size().y - node.size().y) * node.inverse_scale_factor();
            scroll.y = (scroll.y - delta).clamp(0., max.max(0.));
            editor.scroll_offset = scroll.y;
        }
    }
}

/// Right-drag orbit, plus the off-centre projection that keeps the subject
/// in the middle of the 3D area right of the palette. The sub view shifts
/// the image, so picking rays and rendering stay consistent.
pub fn orbit(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut editor: ResMut<EditorState>,
    state: Res<State<GameStage>>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Camera, &Projection), With<crate::GameCamera>>,
) {
    if crate::cell_editor::routes(state.get()) || crate::creature_editor::routes(state.get()) {
        motion.clear();
        return;
    }
    for event in motion.read() {
        if editor.open && mouse.pressed(MouseButton::Right) {
            editor.orbit_yaw = game::wrap_yaw(editor.orbit_yaw + event.delta.x * 0.008);
            editor.orbit_height = (editor.orbit_height + event.delta.y * 0.02).clamp(-2.5, 5.2);
        }
    }
    let Ok((mut camera, projection)) = cameras.single_mut() else {
        return;
    };
    let wanted = if editor.open {
        windows.single().ok().and_then(|window| {
            let size = camera.physical_viewport_size()?;
            let Projection::Perspective(perspective) = projection else {
                return None;
            };
            let layout = CreatorLayout::new(window.width(), window.height(), state.get());
            // `camera_follow` aims 0.8 units to the camera's right of the
            // subject; add that back so the subject itself is centred.
            let distance = editor.camera_offset().length().max(0.1);
            let aim = 0.8 * window.height() * 0.5 / (distance * (perspective.fov * 0.5).tan());
            let scale = size.x as f32 / window.width().max(1.);
            let shift = ((layout.view_center_shift() + aim) * scale).round();
            Some(sub_view_shift(size, shift))
        })
    } else {
        None
    };
    if camera.sub_camera_view != wanted {
        camera.sub_camera_view = wanted;
    }
}

/// Sub view of a wider virtual image that moves the projection centre
/// `shift` physical pixels to the right (left when negative).
fn sub_view_shift(size: UVec2, shift: f32) -> bevy::camera::SubCameraView {
    let pad = (shift.abs() * 2.).round() as u32;
    bevy::camera::SubCameraView {
        full_size: UVec2::new(size.x + pad, size.y),
        offset: Vec2::new(if shift < 0. { pad as f32 } else { 0. }, 0.),
        size,
    }
}

pub fn cell_abilities(run: &RunProgress, originals: &Originals) -> Capabilities {
    originals
        .part_catalog
        .abilities_for_parts_in(asset_catalog::CELL_PARTS, &run.cell_parts)
}
pub fn can_eat(a: Capabilities, meat: bool) -> bool {
    if meat {
        a.carnivore > 0
    } else {
        a.herbivore > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_model_variant_preserves_exact_definition_price_capabilities_and_refund() {
        use crate::asset_catalog::PartCatalog;
        use crate::dbpf::ResourceKey;
        let mut catalog = PartCatalog::default();
        let shared_model = ResourceKey {
            instance_id: 0x0528_b240,
            type_id: asset_catalog::RW4,
            group_id: asset_catalog::CREATURE_PARTS,
        };
        let canonical = PartDefinition {
            key: ResourceKey {
                instance_id: 0xaaaa_0001,
                type_id: asset_catalog::PROP,
                group_id: asset_catalog::CREATURE_PARTS,
            },
            name: "Canonical Variant".into(),
            model: shared_model,
            diffuse: None,
            tint: None,
            spec_bump: None,
            price: 50,
            unlock_level: 1,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            min_scale: 0.5,
            max_scale: 1.5,
            is_null: false,
            abilities: Capabilities {
                strike: 1,
                ..Capabilities::default()
            },
        };
        let expensive_variant = PartDefinition {
            key: ResourceKey {
                instance_id: 0xbbbb_0002,
                type_id: asset_catalog::PROP,
                group_id: asset_catalog::CREATURE_PARTS,
            },
            name: "Elite Variant".into(),
            price: 175,
            unlock_level: 2,
            min_scale: 0.8,
            max_scale: 2.2,
            abilities: Capabilities {
                strike: 4,
                charge: 3,
                ..Capabilities::default()
            },
            ..canonical.clone()
        };
        catalog.parts.push(canonical.clone());
        catalog.parts.push(expensive_variant.clone());

        let mut run = RunProgress {
            editor_dna: 300.0,
            creature_found_parts: vec![expensive_variant.key.instance_id],
            ..RunProgress::default()
        };
        assert!(purchase_assembly(
            &mut run,
            &GameStage::Creature,
            &expensive_variant,
            0,
            false,
        )
        .is_ok());
        assert_eq!(run.editor_dna, 125.0);
        assert_eq!(run.parts.len(), 1);
        assert_eq!(run.parts[0].instance, shared_model.instance_id);
        assert_eq!(
            run.parts[0].definition_id,
            Some(expensive_variant.key.instance_id)
        );
        assert_eq!(
            catalog
                .by_part_in(asset_catalog::CREATURE_PARTS, &run.parts[0])
                .map(|d| d.name.as_str()),
            Some("Elite Variant")
        );
        let abilities = catalog.abilities_for_parts_in(asset_catalog::CREATURE_PARTS, &run.parts);
        assert_eq!((abilities.strike, abilities.charge), (4, 3));

        // Legacy parts without definition_id still fall back to canonical model lookup.
        let legacy = PartInstance::new(shared_model.instance_id, 0);
        assert_eq!(
            catalog
                .by_part_in(asset_catalog::CREATURE_PARTS, &legacy)
                .map(|d| d.name.as_str()),
            Some("Canonical Variant")
        );

        assert!(sell_selected(
            &mut run,
            &GameStage::Creature,
            &catalog,
        ));
        assert_eq!(run.editor_dna, 300.0);
        assert!(run.parts.is_empty());
    }

    #[test]
    #[ignore = "requires installed original part geometry and spec/bump textures"]
    fn authored_bump_normal_reaches_each_imported_surface_as_linear_data() {
        let install = crate::spore_install::discover().unwrap();
        let index = asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = asset_catalog::PartCatalog::from_index(&index);
        let painter = crate::skinpaint::load(&index).unwrap();
        assert_eq!(
            painter.parameters.source,
            Some(crate::skinpaint::APP_PAINT_DEFAULTS)
        );
        assert_eq!(painter.parameters.bump_height, 5.);
        assert_eq!(painter.parameters.part_bump_scale, 0.8);
        let definition = catalog
            .parts
            .iter()
            .find(|p| !p.is_null && p.abilities.eye > 0 && p.spec_bump.is_some())
            .unwrap()
            .clone();
        let original =
            texture::decode_rw4(&index.read(definition.spec_bump.unwrap()).unwrap()).unwrap();
        let expected = painter.part_normal(&original).unwrap();
        let originals = Originals {
            resource_index: index,
            part_catalog: catalog,
            native_painter: Some(painter),
            ..default()
        };
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let model = import_definition(
            &originals,
            &definition,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap();
        assert!(!model.surfaces.is_empty());
        for surface in model.surfaces {
            let material = materials.get(&surface.material).unwrap();
            let normal = images
                .get(
                    material
                        .normal_map_texture
                        .as_ref()
                        .expect("original height map must be mounted"),
                )
                .unwrap();
            assert_eq!(normal.texture_descriptor.format, TextureFormat::Rgba8Unorm);
            assert_eq!(normal.data.as_ref().unwrap(), &expected.rgba);
            assert!(!material.flip_normal_map_y);
            assert!(meshes
                .get(&surface.handle)
                .unwrap()
                .contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        }
    }
    fn definition() -> PartDefinition {
        let key = crate::dbpf::ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: asset_catalog::CELL_PARTS,
            instance_id: 123,
        };
        PartDefinition {
            key,
            model: crate::dbpf::ResourceKey {
                type_id: asset_catalog::RW4,
                ..key
            },
            name: "Filter".into(),
            is_null: false,
            price: 15,
            unlock_level: 1,
            min_scale: 0.5,
            max_scale: 1.5,
            default_scale: 2.,
            model_scale: 1.,
            model_offset: [0.; 3],
            model_rotation: [0.; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities {
                herbivore: 1,
                ..default()
            },
        }
    }
    #[test]
    fn editor_purchase_preserves_evolution_and_roundtrips() {
        let mut run = RunProgress {
            editor_dna: 25.,
            dna_this_stage: 82.,
            ..default()
        };
        purchase(&mut run, &GameStage::Cell, &definition(), 0).unwrap();
        assert_eq!((run.editor_dna, run.dna_this_stage), (10., 82.));
        assert!(run.parts.is_empty());
        assert_eq!(run.cell_parts[0].scale, 1.5);
        let saved: RunProgress =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        assert_eq!(saved.cell_parts, run.cell_parts);
        assert_eq!(saved.editor_dna, 10.);
    }
    #[test]
    fn rejected_purchase_changes_nothing() {
        let mut run = RunProgress {
            editor_dna: 2.,
            ..default()
        };
        let before = serde_json::to_string(&run).unwrap();
        assert!(purchase(&mut run, &GameStage::Cell, &definition(), 0).is_err());
        assert!(purchase(&mut run, &GameStage::Creature, &definition(), 0).is_err());
        assert_eq!(serde_json::to_string(&run).unwrap(), before);
    }
    #[test]
    fn symmetric_purchase_edit_reload_and_sell_are_one_item() {
        let stage = GameStage::Creature;
        let mut d = definition();
        d.key.group_id = asset_catalog::CREATURE_PARTS;
        let mut catalog = asset_catalog::PartCatalog::default();
        catalog.parts.push(d.clone());
        let mut run = RunProgress::default();
        purchase_assembly(&mut run, &stage, &d, 1, true).unwrap();
        assert_eq!(run.editor_dna, 10.);
        assert_eq!(run.parts.len(), 2);
        assert_eq!((run.parts[0].bone, run.parts[1].bone), (1, 2));
        assert!(run.parts[0].pair_id != 0 && run.parts[0].pair_id == run.parts[1].pair_id);
        assert!(run.parts[1].mirrored && !run.parts[0].mirrored);
        run.parts[0].offset = [-0.2, 0.1, 0.3];
        run.parts[0].yaw = 0.8;
        run.parts[0].scale = 1.2;
        sync_pair(&mut run, &stage, 0);
        let a = run.body.slot(1) + Vec3::from_array(run.parts[0].offset);
        let b = run.body.slot(2) + Vec3::from_array(run.parts[1].offset);
        assert!(b.distance(Vec3::new(-a.x, a.y, a.z)) < 1e-6);
        assert_eq!(run.parts[1].yaw, -0.8);
        assert_eq!(run.parts[1].scale, 1.2);
        assert!(run.parts[1].mesh_scale(2.).x < 0.);
        let mut loaded: RunProgress =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        loaded.focused_part = 1;
        assert!(sell_selected(&mut loaded, &stage, &catalog));
        assert!(loaded.parts.is_empty());
        assert_eq!(loaded.editor_dna, 25.);
        assert!(!sell_selected(&mut loaded, &stage, &catalog));
        assert_eq!(loaded.editor_dna, 25.);
    }
    #[test]
    fn symmetric_part_limit_rejects_atomically_and_legacy_parts_stay_independent() {
        let mut run = RunProgress::default();
        run.cell_parts = (0..47).map(|i| PartInstance::new(i, 0)).collect();
        let before = serde_json::to_string(&run).unwrap();
        assert!(purchase_assembly(&mut run, &GameStage::Cell, &definition(), 1, true).is_err());
        assert_eq!(before, serde_json::to_string(&run).unwrap());
        let old: PartInstance = serde_json::from_str(r#"{"instance":123,"bone":1}"#).unwrap();
        assert_eq!(old.pair_id, 0);
        assert!(!old.mirrored);
        run.cell_parts = vec![old.clone(), old];
        let mut catalog = asset_catalog::PartCatalog::default();
        catalog.parts.push(definition());
        assert!(sell_selected(&mut run, &GameStage::Cell, &catalog));
        assert_eq!(run.cell_parts.len(), 1);
    }
    #[test]
    fn closing_during_drag_keeps_the_final_undo_operation() {
        let stage = GameStage::Cell;
        let mut run = RunProgress::default();
        run.cell_parts.push(PartInstance::new(123, 1));
        let before = run.clone();
        let mut editor = EditorState::default();
        editor.drag = Some(PartDrag {
            part: 0,
            spine: None,
            point: Vec3::ZERO,
            normal: Vec3::Z,
            grab: Vec3::ZERO,
            before,
        });
        run.cell_parts[0].offset[0] = 0.3;
        finish_drag(&mut editor, &mut run, &stage);
        assert_eq!(editor.undo.len(), 1);
        assert_eq!(editor.undo[0].cell_parts[0].offset[0], 0.);
        assert!(editor.drag.is_none());
        finish_drag(&mut editor, &mut run, &stage);
        assert_eq!(editor.undo.len(), 1);
    }
    #[test]
    fn authored_assembly_keeps_all_mesh_sections() {
        let triangle = crate::rw4::ExtractedMesh {
            source_section: 0,
            bone_indices: vec![],
            bone_weights: vec![],
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.]; 3],
            indices: vec![0, 1, 2],
            morphs: vec![],
            skin_idx: vec![],
            skin_w: vec![],
        };
        let mesh = assembled_mesh(vec![triangle.clone(), triangle], &definition()).unwrap();
        assert_eq!(mesh.count_vertices(), 6);
        assert!(matches!(mesh.indices(), Some(Indices::U32(v)) if v == &[0,1,2,3,4,5]));
    }
    #[test]
    fn mounted_mouth_controls_food() {
        let plant = Capabilities {
            herbivore: 1,
            ..default()
        };
        let meat = Capabilities {
            carnivore: 1,
            ..default()
        };
        assert!(can_eat(plant, false));
        assert!(!can_eat(plant, true));
        assert!(!can_eat(meat, false));
        assert!(can_eat(meat, true));
        let mut both = plant;
        both.merge(meat);
        assert!(can_eat(both, true) && can_eat(both, false));
        assert!(!can_eat(Capabilities::default(), false));
    }
    #[test]
    fn starter_parts_are_granted_once_even_after_selling_and_reload() {
        let mut originals = Originals::default();
        originals.part_catalog.parts.push(definition());
        let mut run = RunProgress::default();
        seed_parts(&mut run, &originals, &GameStage::Cell);
        assert_eq!(run.cell_parts.len(), 1);
        run.cell_parts.clear();
        let mut run: RunProgress =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        seed_parts(&mut run, &originals, &GameStage::Cell);
        assert!(run.cell_editor_initialized && run.cell_parts.is_empty());
    }
    #[test]
    fn palette_pauses_and_resumes_simulation() {
        // Creature's creator opens directly on F2; Cell's opens via a mate.
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
        let world = app.world_mut();
        let entity = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        let start = world.entity(entity).get::<Transform>().unwrap().translation;
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<Transform>()
                .unwrap()
                .translation,
            start
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        assert_ne!(
            app.world()
                .entity(entity)
                .get::<Transform>()
                .unwrap()
                .translation,
            start
        );
    }
    #[test]
    fn editor_history_restores_parts_dna_and_spine_through_actual_controls() {
        let mut run = RunProgress::default();
        run.parts.push(PartInstance::new(123, 0));
        let mut app = crate::runtime::headless_app(run);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
        let mut part = definition();
        part.key.group_id = asset_catalog::CREATURE_PARTS;
        app.world_mut()
            .resource_mut::<Originals>()
            .part_catalog
            .parts
            .push(part);
        app.world_mut().resource_mut::<EditorState>().show();
        let original = app.world().resource::<RunProgress>().clone();
        let action = |app: &mut App, action: Action| {
            app.world_mut().spawn((action, Pressed));
            app.update();
        };
        action(&mut app, Action::Remove);
        assert!(app.world().resource::<RunProgress>().parts.is_empty());
        assert_eq!(
            app.world().resource::<RunProgress>().editor_dna,
            original.editor_dna + 15.
        );
        action(&mut app, Action::Undo);
        assert_eq!(app.world().resource::<RunProgress>().parts, original.parts);
        assert_eq!(
            app.world().resource::<RunProgress>().editor_dna,
            original.editor_dna
        );
        action(&mut app, Action::Redo);
        assert!(app.world().resource::<RunProgress>().parts.is_empty());
        action(&mut app, Action::Thickness(0.1));
        assert_ne!(app.world().resource::<RunProgress>().body, original.body);
        action(&mut app, Action::Undo);
        assert_eq!(app.world().resource::<RunProgress>().body, original.body);
        assert!(app.world().resource::<RunProgress>().parts.is_empty());
        // A new edit after undo discards the abandoned redo branch.
        action(&mut app, Action::Lift(0.1));
        assert!(app.world().resource::<EditorState>().redo.is_empty());
        let n = app.world().resource::<RunProgress>().body.spine.len();
        action(&mut app, Action::AddSpine);
        assert_eq!(
            app.world().resource::<RunProgress>().body.spine.len(),
            n + 1
        );
        action(&mut app, Action::Undo);
        assert_eq!(app.world().resource::<RunProgress>().body.spine.len(), n);
    }
    /// The creature stage with a mounted part and the creator open. Also loads
    /// the creature catalog definition so purchases/refunds resolve.
    fn creature_app() -> App {
        let mut run = RunProgress::default();
        run.parts.push(PartInstance::new(123, 0));
        let mut app = crate::runtime::headless_app(run);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
        let mut part = definition();
        part.key.group_id = asset_catalog::CREATURE_PARTS;
        app.world_mut()
            .resource_mut::<Originals>()
            .part_catalog
            .parts
            .push(part);
        // Stand in for a part already imported successfully: the catalog
        // definition resolves and `ensure_model` finds a cached model carrying
        // a material, exactly as it does after the first real import.
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Sphere::new(0.4));
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let imported_definition = app
            .world()
            .resource::<Originals>()
            .part_catalog
            .parts
            .last()
            .unwrap()
            .key;
        app.world_mut()
            .resource_mut::<Originals>()
            .models
            .push(crate::OriginalModel {
                source_definition: Some(imported_definition),
                surfaces: vec![],
                material: Some(material),
                handle: mesh,
                scale: 1.,
                instance: 123,
                root: None,
                rig_bones: 0,
            });
        app.world_mut().resource_mut::<EditorState>().show();
        app
    }
    /// Build a pointer press the way `PointerEventsPlugin` does, so the real
    /// `bevy_ui_widgets` button observers run. Nothing in those observers reads
    /// the hit or the target, so placeholder values are enough.
    fn pointer_press(entity: Entity) -> bevy::picking::events::PointerPress {
        bevy::picking::events::PointerPress {
            entity,
            pointer: bevy::picking::prelude::Pointer::new(
                bevy::picking::pointer::PointerId::Mouse,
                bevy::picking::pointer::Location {
                    target: bevy::camera::NormalizedRenderTarget::None {
                        width: 1280,
                        height: 720,
                    },
                    position: Vec2::new(120., 300.),
                },
            ),
            button: bevy::picking::prelude::PointerButton::Primary,
            hit: bevy::picking::backend::HitData {
                camera: Entity::PLACEHOLDER,
                depth: 0.,
                position: None,
                normal: None,
                extra: None,
            },
            count: 1,
        }
    }
    /// Spawn a palette-style button and press it with a real pointer event,
    /// returning the entity after the widget has marked it `Pressed`.
    fn press_button(app: &mut App, action: Action) -> Entity {
        let entity = app.world_mut().spawn((Button, action)).id();
        app.world_mut().trigger(pointer_press(entity));
        // The observer queues a command, so the deferred buffers have to be
        // applied before `editor::input` can observe `Added<Pressed>`.
        app.world_mut().flush();
        app.world_mut().flush();
        app.update();
        assert!(
            app.world().entity(entity).contains::<Pressed>(),
            "the real button widget must insert Pressed on a pointer press"
        );
        entity
    }
    /// Regression: the creator used to be driven by hand-inserted `Pressed`
    /// components only, so nothing proved the shipped `Button` → `Pressed` →
    /// `Action` chain works. `bevy_internal` 0.20 puts `UiWidgetsPlugins` in
    /// `DefaultPlugins`, which is what registers these observers.
    #[test]
    fn a_real_pointer_press_runs_a_creator_action() {
        let stage = GameStage::Creature;
        let mut app = creature_app();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin);
        assert!(!app.world().resource::<EditorState>().asymmetric);

        press_button(&mut app, Action::Symmetry);
        assert!(
            app.world().resource::<EditorState>().asymmetric,
            "a pointer press on the palette must reach editor::input"
        );
        assert!(app.world().resource::<EditorState>().open);

        // A part purchase through the same real path, proving the button chain
        // is not a decoration: it charges DNA and mounts the part.
        let dna = app.world().resource::<RunProgress>().editor_dna;
        let parts = active_parts(&app.world().resource::<RunProgress>(), &stage).len();
        press_button(&mut app, Action::Buy(123));
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.editor_dna, dna - 15., "one click spends one price");
        assert_eq!(active_parts(run, &stage).len(), parts + 1);

        // Selling it back through the same real path refunds exactly once.
        press_button(&mut app, Action::Remove);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(active_parts(run, &stage).len(), parts);
        assert_eq!(run.editor_dna, dna);
    }
    /// Closing with `Action::Done` while a part drag is in flight must keep the
    /// drag as one undo operation, exactly like the F2 path.
    #[test]
    fn done_close_keeps_an_in_flight_drag_in_the_history() {
        let stage = GameStage::Creature;
        let mut app = creature_app();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin);
        let original = active_parts(&app.world().resource::<RunProgress>(), &stage).to_vec();

        let before = app.world().resource::<RunProgress>().clone();
        {
            let world = app.world_mut();
            world.resource_mut::<RunProgress>().parts[0].offset[0] = 0.3;
            world.resource_mut::<EditorState>().drag = Some(PartDrag {
                part: 0,
                spine: None,
                point: Vec3::ZERO,
                normal: Vec3::Z,
                grab: Vec3::ZERO,
                before,
            });
        }
        press_button(&mut app, Action::Done);
        assert!(
            !app.world().resource::<EditorState>().open,
            "Done closes the creator"
        );
        assert!(
            app.world().resource::<EditorState>().drag.is_none(),
            "the in-flight drag is finished on close"
        );
        // The drag's final placement stays applied, and is one undo entry deep.
        let edited = active_parts(&app.world().resource::<RunProgress>(), &stage).to_vec();
        assert_ne!(edited, original);
        assert_eq!(app.world().resource::<EditorState>().undo.len(), 1);
        assert_eq!(
            app.world().resource::<EditorState>().undo[0].parts,
            original
        );

        // Reopening and undoing brings the pre-drag placement back.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert!(app.world().resource::<EditorState>().open);
        press_button(&mut app, Action::Undo);
        assert_eq!(
            active_parts(&app.world().resource::<RunProgress>(), &stage),
            &original[..]
        );
    }
    /// The F2 key (not a button) must finish the drag too. This also documents
    /// the stage-change path: it clears the history on purpose, because undo
    /// entries hold per-stage `RunProgress` snapshots, so discarding an
    /// in-flight drag there is correct rather than state loss — the final
    /// placement is already saved.
    #[test]
    fn f2_close_keeps_an_in_flight_drag_and_stage_change_discards_history() {
        let stage = GameStage::Creature;
        let mut app = creature_app();
        let original = active_parts(&app.world().resource::<RunProgress>(), &stage).to_vec();

        let before = app.world().resource::<RunProgress>().clone();
        {
            let world = app.world_mut();
            world.resource_mut::<RunProgress>().parts[0].offset[0] = 0.3;
            world.resource_mut::<EditorState>().drag = Some(PartDrag {
                part: 0,
                spine: None,
                point: Vec3::ZERO,
                normal: Vec3::Z,
                grab: Vec3::ZERO,
                before,
            });
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert!(!app.world().resource::<EditorState>().open);
        assert!(app.world().resource::<EditorState>().drag.is_none());
        assert_eq!(app.world().resource::<EditorState>().undo.len(), 1);
        let edited = active_parts(&app.world().resource::<RunProgress>(), &stage).to_vec();
        assert_ne!(edited, original, "the drag placement is kept, not reverted");
        assert_eq!(
            app.world().resource::<EditorState>().undo[0].parts,
            original
        );

        // A debug stage change with a drag in flight keeps placement and drops
        // the (now meaningless) per-stage history, but nothing is lost.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let leaving = app.world().resource::<RunProgress>().clone();
        {
            let world = app.world_mut();
            world.resource_mut::<RunProgress>().parts[0].offset[0] = 0.6;
            world.resource_mut::<EditorState>().drag = Some(PartDrag {
                part: 0,
                spine: None,
                point: Vec3::ZERO,
                normal: Vec3::Z,
                grab: Vec3::ZERO,
                before: leaving,
            });
        }
        // The debug jump to the menu is a Ctrl+Alt chord (plain Ctrl + digit is
        // a Civilization vehicle group).
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::Digit0);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        for _ in 0..2 {
            app.update();
        }
        assert_eq!(
            *app.world().resource::<State<GameStage>>().get(),
            GameStage::MainMenu
        );
        let run = app.world().resource::<RunProgress>();
        assert!(
            (run.parts[0].offset[0] - 0.6).abs() < 1e-6,
            "the final drag placement survives leaving the stage"
        );
        assert!(app.world().resource::<EditorState>().drag.is_none());
    }
    /// A spine edit that would collapse two neighbouring points is refused
    /// instead of throwing the hand-shaped body away.
    #[test]
    fn spine_controls_reject_a_collapse_and_keep_the_rest_of_the_body() {
        let mut app = creature_app();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.body.spine[1].radius = 1.3;
            run.body.spine[2].position[1] = 1.2;
        }
        let shaped = app.world().resource::<RunProgress>().body.clone();
        let action = |app: &mut App, action: Action| {
            app.world_mut().spawn((action, Pressed));
            app.update();
        };
        // Walk point 2 onto point 3 through the real length control.
        for _ in 0..200 {
            action(&mut app, Action::Length(0.1));
        }
        let body = app.world().resource::<RunProgress>().body.clone();
        assert_ne!(body, crate::body::BodyProfile::default());
        assert_eq!(body.spine.len(), shaped.spine.len(), "no point is dropped");
        assert!(
            body.spine
                .windows(2)
                .all(
                    |w| Vec3::from_array(w[0].position).distance(Vec3::from_array(w[1].position))
                        > 0.01
                ),
            "the collapse is refused, not silently repaired"
        );
        assert_eq!(
            body.spine[1].radius, shaped.spine[1].radius,
            "unrelated edits survive"
        );
        assert_ne!(body, shaped, "the legal part of the movement still applies");
    }
    #[test]
    fn creator_isolates_preview_and_restores_defeated_visibility() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<EditorState>().show();
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<MenuPreview>>()
                .iter(world)
                .count(),
            1
        );
        world.resource_mut::<EditorState>().open = false;
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<Player>>()
                .iter(world)
                .count(),
            0
        );
        world
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
        let world = app.world_mut();
        let npc = world
            .query_filtered::<Entity, With<crate::game::Npc>>()
            .iter(world)
            .next()
            .unwrap();
        world.entity_mut(npc).insert(Visibility::Hidden);
        world.resource_mut::<EditorState>().show();
        app.update();
        app.world_mut().resource_mut::<EditorState>().open = false;
        app.update();
        assert_eq!(
            *app.world().entity(npc).get::<Visibility>().unwrap(),
            Visibility::Hidden
        );
        assert!(app.world().entity(npc).get::<EditorHidden>().is_none());
    }
    #[test]
    fn creator_layout_puts_the_palette_on_the_left_and_blocks_its_panels() {
        let layout = CreatorLayout::new(1280., 900., &GameStage::Creature);
        // A tall left column of a little over a quarter of the screen.
        assert_eq!(layout.palette.min, Vec2::ZERO);
        assert!((layout.palette.width() / 1280. - 0.28).abs() < 0.01);
        assert_eq!(layout.palette.height(), 900.);
        // Three columns of icons, at least five rows on a 900-pixel screen.
        assert_eq!(layout.page_size() % GRID_COLUMNS, 0);
        assert!(layout.page_size() >= 15);
        assert!(layout.icon >= 60., "icons stay readable");
        // Palette, name/DNA bar, undo/done cluster and ability panel all
        // swallow presses; the open view right of the palette does not.
        assert!(layout.over_palette(Vec2::new(40., 500.)));
        assert!(layout.over_ui(Vec2::new(layout.header.center().x, 30.)));
        assert!(layout.over_ui(Vec2::new(1270., 30.)));
        assert!(layout.over_ui(Vec2::new(1200., 860.)));
        assert!(!layout.over_ui(Vec2::new(760., 450.)));
        // The subject is centred in the space right of the palette.
        assert_eq!(
            640. + layout.view_center_shift(),
            layout.palette.max.x + (1280. - layout.palette.max.x) * 0.5
        );
        // Small windows keep a usable grid.
        let small = CreatorLayout::new(800., 600., &GameStage::Cell);
        assert!(small.page_size() >= 6 && small.palette.width() >= 300.);
    }

    #[test]
    fn cell_creator_tabs_are_only_cell_part_families() {
        let cell: Vec<_> = categories(&GameStage::Cell).iter().map(|c| c.1).collect();
        assert_eq!(cell, ["Mouths", "Movement", "Weapons", "Eyes"]);
        assert!(!cell.contains(&"Feet") && !cell.contains(&"Hands"));
        let creature: Vec<_> = categories(&GameStage::Creature)
            .iter()
            .map(|c| c.1)
            .collect();
        assert_eq!(creature[1], "Eyes", "the second creature tab is Eyes");
        for category in ["Mouths", "Eyes", "Hands", "Feet", "Weapons", "Details"] {
            assert!(creature.contains(&category));
        }
        assert_eq!(
            categories(&GameStage::MainMenu),
            categories(&GameStage::Creature)
        );
    }

    #[test]
    fn sub_view_moves_the_projection_centre_and_keeps_the_frame() {
        let size = UVec2::new(1280, 900);
        let view = sub_view_shift(size, 200.);
        assert_eq!(view.size, size);
        assert_eq!(view.full_size, UVec2::new(1680, 900));
        // The wider image's centre lies 200 pixels right of the sub view's.
        let centre = view.full_size.x as f32 * 0.5 - view.offset.x;
        assert_eq!(centre - size.x as f32 * 0.5, 200.);
        let left = sub_view_shift(size, -120.);
        let centre = left.full_size.x as f32 * 0.5 - left.offset.x;
        assert_eq!(centre - size.x as f32 * 0.5, -120.);
    }

    #[test]
    fn cell_creator_camera_looks_down_onto_the_swimming_plane() {
        let mut editor = EditorState {
            cell_view: true,
            ..default()
        };
        let offset = editor.camera_offset();
        assert!(offset.y > offset.xz().length(), "steep top-down view");
        editor.cell_view = false;
        assert!(editor.camera_offset().y < editor.camera_offset().xz().length());
    }

    #[test]
    fn a_part_dropped_back_on_the_palette_is_returned_exactly() {
        let stage = GameStage::Cell;
        let mut run = RunProgress {
            editor_dna: 40.,
            ..default()
        };
        let before = run.clone();
        let mut editor = EditorState::default();
        purchase(&mut run, &stage, &definition(), 0).unwrap();
        editor.record(before.clone());
        editor.carry = Some(PartCarry {
            part: 0,
            before: before.clone(),
            left_palette: true,
        });
        run.cell_parts[0].offset = [0.2, 0., 0.1];
        release_carry(&mut editor, &mut run, &stage, true);
        assert!(run.cell_parts.is_empty());
        assert_eq!(run.editor_dna, 40.);
        assert!(
            editor.undo.is_empty(),
            "the purchase leaves no history behind"
        );
        assert!(editor.carry.is_none());

        // Dropped on the body instead, the purchase and placement stay as one
        // undo step back to the pre-purchase state.
        purchase(&mut run, &stage, &definition(), 0).unwrap();
        editor.record(before.clone());
        editor.carry = Some(PartCarry {
            part: 0,
            before: before.clone(),
            left_palette: true,
        });
        run.cell_parts[0].offset = [0.2, 0., 0.1];
        release_carry(&mut editor, &mut run, &stage, false);
        assert_eq!(run.cell_parts.len(), 1);
        assert_eq!(run.cell_parts[0].offset, [0.2, 0., 0.1]);
        assert_eq!(editor.undo.len(), 1);
        assert_eq!(editor.undo[0].cell_parts, before.cell_parts);

        // A click that never left the palette keeps the default mount.
        editor.carry = Some(PartCarry {
            part: 0,
            before,
            left_palette: false,
        });
        release_carry(&mut editor, &mut run, &stage, true);
        assert_eq!(run.cell_parts.len(), 1);
    }

    #[test]
    fn random_names_are_pronounceable_and_vary() {
        let names: std::collections::HashSet<_> =
            (0..20).map(|seed| random_name(seed, false, 7)).collect();
        assert!(names.len() > 10);
        for name in &names {
            assert!(name.len() >= 3 && name.chars().all(|c| c.is_ascii_alphabetic()));
            assert!(name.chars().next().unwrap().is_ascii_uppercase());
        }
        assert_eq!(random_name(3, true, 1), random_name(3, true, 1));
    }

    #[test]
    fn dragging_plane_rejects_parallel_and_behind_camera_rays() {
        let point = Vec3::new(0., 0., -5.);
        assert_eq!(
            ray_plane(Vec3::ZERO, Vec3::NEG_Z, point, Vec3::Z),
            Some(point)
        );
        assert!(ray_plane(Vec3::ZERO, Vec3::X, point, Vec3::Z).is_none());
        assert!(ray_plane(Vec3::ZERO, Vec3::Z, point, Vec3::Z).is_none());
    }
    #[test]
    #[ignore = "requires original Spore packages"]
    fn retail_definitions_and_authored_texture_links() {
        let install = crate::spore_install::discover().unwrap();
        let index = asset_catalog::ResourceIndex::load(&install.packages);
        let cat = asset_catalog::PartCatalog::from_index(&index);
        assert!(cat.parts.len() > 1400);
        let p = cat
            .parts
            .iter()
            .find(|p| {
                p.key.instance_id == 0x56e77082 && p.key.group_id == asset_catalog::CREATURE_PARTS
            })
            .unwrap();
        assert_eq!(p.price, 250);
        assert_eq!((p.abilities.bite, p.abilities.sing), (3, 5));
        assert_eq!(p.abilities.herbivore, 1);
        assert!(
            !p.name.contains("PLACEHOLDER"),
            "locale must override development text"
        );
        let b = index.read(p.diffuse.unwrap()).unwrap();
        let pixels = texture::decode_rw4(&b).unwrap();
        assert_eq!(pixels.width, 128);
        assert_eq!(pixels.rgba.len(), 128 * 128 * 4);
        let eye = cat
            .parts
            .iter()
            .find(|p| p.key.group_id == asset_catalog::CELL_PARTS && p.name == "Beady Eye")
            .unwrap();
        assert_eq!(eye.price, 5);
        assert_eq!(eye.model_scale, 1.8);
    }
}
