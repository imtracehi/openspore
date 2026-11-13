//! The vehicle, building and UFO editors on the shared editor layer.
//!
//! Retail runs every creator through one `cEditor`; an editor differs only by
//! its setup PROP (`creature_editor_data`). The vehicle editors
//! (`VehicleTemplate` and its nine Civ classes: military / economic /
//! cultural x land / sea / air), the building editors (`BuildingTemplate`:
//! city hall, house, factory, entertainment) and the UFO editor
//! (`UFOEditorSetup`) are rigid-block editors: no creature, skin or spine, the
//! manipulators are Pinning, Stacking / PlanarInterpenetration, DeformHandle
//! and RotationHandle (`notes/rigid-editors-notes.md`).
//!
//! The session reuses the Creature Creator's chrome, camera, lights and undo
//! (`creature_editor*`), exactly as the outfitters do (`outfitter`):
//!
//! * `run.parts` holds the blocks being edited while a session runs, each a
//!   `PartInstance` with a free transform (`PartInstance::rigid`); the real
//!   parts and the creature name are kept in `RigidRecords::session` so a save
//!   written during the session never loses them (`repair`);
//! * a preview root with the `Player` marker (the camera target, as the
//!   outfitter's preview creature) carries the block meshes; the stage is put
//!   away (`editor::scene`, `hold_scene`);
//! * the budget is the setup's (`editorDefaultInitialBudget` minus the cost of
//!   the loaded design, `editorSubtractPartsCostOnLoad`), not the stage's
//!   money.
//!
//! Accepting stores the design per class in [`RigidRecords`] and in the
//! stage's own record (`RunProgress::{vehicle, building, ship}` where the
//! stages read them); the stage mounts blocks with their free transform and
//! old slot-based parts keep working ([`block_transform`], `civ`, `space`).
//! Old slot records are converted to blocks when a session opens
//! ([`legacy_block`]).
//!
//! Editing (`rigid_editor_edit`): palette drag, pinning to the surface of the
//! part under the cursor, the symmetry twin, wheel scale, rotation and deform
//! handles. F3 opens the editor in the Civ and Space stages (Shift+F3 the
//! older assembly modal).

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    creature_editor_data::SetupKind,
    game::{PartInstance, RunProgress, ShipModule, VehiclePart},
    stages::GameStage,
};

// ---------------------------------------------------------------------------
// The free-transform record
// ---------------------------------------------------------------------------

fn ones() -> [f32; 3] {
    [1.0; 3]
}

fn identity_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

/// A rigid block's free transform in editor space (Y up, +Z the front of the
/// model, one unit = one editor metre). The part's mesh keeps its own
/// proportions (`RigidBlock::transform`): `stretch` is the deform handles'
/// per-axis scale, the uniform size is the part's `scale`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidBlock {
    pub position: [f32; 3],
    /// Quaternion x, y, z, w.
    #[serde(default = "identity_rotation")]
    pub rotation: [f32; 4],
    /// Deform stretch along the block's own axes.
    #[serde(default = "ones")]
    pub stretch: [f32; 3],
    /// Symmetric pair (0 = single): one price, one refund, one undo entry.
    #[serde(default)]
    pub pair: u32,
    /// The twin of a symmetric pair: the mesh is reflected across x = 0.
    #[serde(default)]
    pub mirrored: bool,
    /// Paint per region (`rigid_paint`; empty = the setup's default theme).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paints: Vec<crate::rigid_paint::RegionPaint>,
    /// Morph (deform handle) weights of the part's mesh, as the creation file
    /// stores them (`<handles><weight channel=..>`; `maxis_creation`). Empty
    /// = the mesh as authored; the editor's own deform handles are `stretch`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<(u32, f32)>,
    /// The block's identity inside its design (a creation file's block index + 1; the editor numbers the blocks it
    /// buys). 0 = not assigned: a design saved before the parent links were stored (the session numbers it and
    /// derives its links once, `rigid_editor_stack::legacy_links`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u32,
    /// `EditorRigblock::mpParent` (+0x33C): the `id` of the block this one stands on, 0 = none (parentless, on the
    /// studio base). The creation files carry it as the parent's `childlist` entry (`maxis_creation`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parent: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl Default for RigidBlock {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: identity_rotation(), stretch: ones(), pair: 0, mirrored: false, paints: Vec::new(), handles: Vec::new(), id: 0, parent: 0 }
    }
}

impl RigidBlock {
    pub fn at(position: Vec3) -> Self {
        Self { position: position.to_array(), ..Self::default() }
    }

    pub fn position(&self) -> Vec3 {
        let p = Vec3::from_array(self.position);
        if p.is_finite() {
            p
        } else {
            Vec3::ZERO
        }
    }

    pub fn rotation(&self) -> Quat {
        let q = Quat::from_array(self.rotation);
        if q.is_finite() && q.length_squared() > 1e-6 {
            q.normalize()
        } else {
            Quat::IDENTITY
        }
    }

    pub fn stretch(&self) -> Vec3 {
        let s = Vec3::from_array(self.stretch);
        if s.is_finite() {
            s.clamp(Vec3::splat(STRETCH_MIN), Vec3::splat(STRETCH_MAX))
        } else {
            Vec3::ONE
        }
    }

    pub fn set_rotation(&mut self, q: Quat) {
        self.rotation = q.normalize().to_array();
    }

    /// The block reflected across the x = 0 plane of symmetry: x flips, the
    /// rotation about the y and z axes reverses (M R M for M = diag(-1, 1, 1)).
    pub fn reflected(&self) -> RigidBlock {
        let p = self.position();
        let q = self.rotation();
        RigidBlock {
            position: [-p.x, p.y, p.z],
            rotation: [q.x, -q.y, -q.z, q.w],
            stretch: self.stretch,
            pair: self.pair,
            mirrored: !self.mirrored,
            paints: self.paints.clone(),
            handles: self.handles.clone(),
            // The twin is its own block: `rigid_editor_stack::link_twin` gives it its id and mirrored parent.
            id: 0,
            parent: 0,
        }
    }

    /// Transform of the part mesh: `unit` is the uniform world size of the
    /// part's own units (the importer's model scale times the part's `scale`).
    pub fn transform(&self, unit: f32) -> Transform {
        let s = self.stretch() * unit;
        Transform {
            translation: self.position(),
            rotation: self.rotation(),
            scale: Vec3::new(if self.mirrored { -s.x } else { s.x }, s.y, s.z),
        }
    }
}

/// Deform handle limits (ADAPTED: the retail morph handle ranges are per part).
pub const STRETCH_MIN: f32 = 0.35;
pub const STRETCH_MAX: f32 = 3.0;

/// A saved design of one class (a vehicle class, a building type or the UFO).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigidDesign {
    /// The name typed into the name field (empty = not named).
    pub name: String,
    pub parts: Vec<VehiclePart>,
}

/// What a session must give back when it did not end properly (`repair`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidBackup {
    pub parts: Vec<PartInstance>,
    pub dna: f32,
    pub name: String,
    pub focused_part: u32,
}

/// What the vehicle, building and UFO editors saved (`RunProgress::rigid`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigidRecords {
    /// Designs by class (the setup's name, e.g. `VehicleMilitaryLand`).
    pub designs: BTreeMap<String, RigidDesign>,
    pub session: Option<RigidBackup>,
}

impl RigidRecords {
    pub fn design(&self, kind: SetupKind) -> Option<&RigidDesign> {
        self.designs.get(kind.setup_name())
    }

    /// The design a vehicle of this type and domain mounts, if its class was
    /// designed.
    pub fn vehicle_design(&self, approach: crate::civ::Approach, domain: crate::civ::VehicleDomain) -> Option<&RigidDesign> {
        self.design(vehicle_class(approach, domain)).filter(|d| !d.parts.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------

/// The retail vehicle class of a Civ vehicle (`Religious` vehicles are the
/// "Cultural" setups).
pub fn vehicle_class(approach: crate::civ::Approach, domain: crate::civ::VehicleDomain) -> SetupKind {
    use crate::civ::{Approach as A, VehicleDomain as D};
    match (approach, domain) {
        (A::Military, D::Land) => SetupKind::VehicleMilitaryLand,
        (A::Military, D::Sea) => SetupKind::VehicleMilitaryWater,
        (A::Military, D::Air) => SetupKind::VehicleMilitaryAir,
        (A::Economic, D::Land) => SetupKind::VehicleEconomicLand,
        (A::Economic, D::Sea) => SetupKind::VehicleEconomicWater,
        (A::Economic, D::Air) => SetupKind::VehicleEconomicAir,
        (A::Religious, D::Land) => SetupKind::VehicleCulturalLand,
        (A::Religious, D::Sea) => SetupKind::VehicleCulturalWater,
        (A::Religious, D::Air) => SetupKind::VehicleCulturalAir,
    }
}

/// The classes the stage's editor cycles through (`[` / `]`, `V`): in Civ the
/// capital's vehicle types by land / sea / air, then the four buildings; in
/// Space the UFO.
pub fn classes(stage: &GameStage, run: &RunProgress) -> Vec<SetupKind> {
    match stage {
        GameStage::Civilization => {
            use crate::civ::VehicleDomain as D;
            let approach = run.civ_campaign.approach;
            vec![
                vehicle_class(approach, D::Land),
                vehicle_class(approach, D::Sea),
                vehicle_class(approach, D::Air),
                SetupKind::BuildingCityHall,
                SetupKind::BuildingHouse,
                SetupKind::BuildingFactory,
                SetupKind::BuildingEntertainment,
            ]
        }
        GameStage::Space => vec![SetupKind::Ufo],
        _ => vec![],
    }
}

/// The class a stage's editor opens with: the last vehicle class in Civ (land
/// vehicles first), the UFO in Space.
pub fn first_class(stage: &GameStage, run: &RunProgress) -> Option<SetupKind> {
    let list = classes(stage, run);
    if *stage == GameStage::Civilization && run.civ_campaign.editor_mode == crate::civ::CivEditorMode::Building {
        return list.into_iter().find(|k| k.is_building());
    }
    list.into_iter().next()
}

// ---------------------------------------------------------------------------
// Stage mounts (pure)
// ---------------------------------------------------------------------------

/// Editor units to stage units of a mounted design (a Civ vehicle is about
/// 2.6 units long, a city hall 3.5 wide, the UFO a ~1.8 unit box). ADAPTED:
/// the retail vehicles and buildings have no stage size in this port; the
/// whole design is scaled uniformly, so its proportions are the editor's.
pub const VEHICLE_STAGE_SCALE: f32 = 0.4;
pub const BUILDING_STAGE_SCALE: f32 = 0.2;
pub const SHIP_STAGE_SCALE: f32 = 0.3;

/// Local transform of a block mounted in a stage: the free transform scaled
/// by `stage_scale`, lifted by `lift`; `unit` is the part's uniform size
/// (`model scale * part scale`).
pub fn block_transform(block: &RigidBlock, unit: f32, stage_scale: f32, lift: Vec3) -> Transform {
    let mut t = block.transform(unit * stage_scale);
    t.translation = t.translation * stage_scale + lift;
    t
}

/// Pose of a slot-based part in editor units, so an old record opens in the
/// editor where the stage showed it: `stage_position` is where the stage
/// mounted it (relative to the design's origin), `yaw` its yaw.
pub fn legacy_block(stage_position: Vec3, yaw: f32, stage_scale: f32) -> RigidBlock {
    let mut b = RigidBlock::at(stage_position / stage_scale);
    b.set_rotation(Quat::from_rotation_y(yaw));
    b
}

/// A hash of what a design looks like (a city skyline is rebuilt when it
/// changes, `civ::build_signature`).
pub fn block_signature(block: &Option<RigidBlock>) -> u32 {
    let Some(b) = block else { return 0 };
    let mut hash: u32 = 0x811c9dc5;
    let mut mix = |bits: u32| {
        hash ^= bits;
        hash = hash.wrapping_mul(16777619);
    };
    for v in b.position.iter().chain(b.rotation.iter()).chain(b.stretch.iter()) {
        mix(v.to_bits());
    }
    mix(b.mirrored as u32);
    mix(crate::rigid_paint::paints_signature(&b.paints));
    if !b.handles.is_empty() {
        for (id, w) in &b.handles {
            mix(*id);
            mix(w.to_bits());
        }
        // Rebuilt once its morphed mesh is ready (`prepare_morphs`).
        mix(morph_generation());
    }
    hash
}

// ---------------------------------------------------------------------------
// Morphed blocks (deform handle weights of a creation file)
// ---------------------------------------------------------------------------

/// Morphed block models by (definition, weights), shared by the editor, the
/// stages and the thumbnails (mesh handles are strong: they stay loaded).
static MORPHS: std::sync::Mutex<Option<HashMap<crate::creation_assets::MorphMeshKey, OriginalModel>>> = std::sync::Mutex::new(None);
static MORPH_GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Bumped whenever a morphed model is added (redraw what uses one).
pub fn morph_generation() -> u32 {
    MORPH_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// The cache key of a definition's mesh morphed by `handles` (weights
/// clamped to 0..1 as `creation_assets` does); `None` without handles.
pub fn morph_key(definition: ResourceKey, handles: &[(u32, f32)]) -> Option<crate::creation_assets::MorphMeshKey> {
    if handles.is_empty() || handles.iter().any(|(_, w)| !w.is_finite()) {
        return None;
    }
    let mut weights: Vec<(u32, u32)> = handles.iter().map(|(id, w)| (*id, w.clamp(0.0, 1.0).to_bits())).collect();
    weights.sort_by_key(|w| w.0);
    weights.dedup_by_key(|w| w.0);
    Some(crate::creation_assets::MorphMeshKey { definition, weights })
}

/// The model a block is drawn with: its definition's model, morphed by the
/// block's handle weights once [`ensure_morph`] made that mesh.
pub fn block_model(originals: &Originals, group: u32, definition_id: Option<u32>, instance: u32, handles: &[(u32, f32)]) -> Option<OriginalModel> {
    let base = model_of(originals, group, definition_id, instance)?;
    let morphed = base
        .source_definition
        .and_then(|d| morph_key(d, handles))
        .and_then(|key| MORPHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key).cloned()));
    Some(morphed.unwrap_or_else(|| base.clone()))
}

/// Make the morphed mesh of a block (its definition's model must be
/// imported). Returns whether a new model was added.
pub fn ensure_morph(originals: &Originals, meshes: &mut Assets<Mesh>, group: u32, definition_id: Option<u32>, instance: u32, handles: &[(u32, f32)]) -> bool {
    let Some(base) = model_of(originals, group, definition_id, instance) else { return false };
    let Some(key) = base.source_definition.and_then(|d| morph_key(d, handles)) else { return false };
    if MORPHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains_key(&key)) {
        return false;
    }
    let mut model = base.clone();
    let definition = originals.part_catalog.parts.iter().find(|d| d.key == key.definition);
    match definition.map(|d| crate::editor::creation_mesh(originals, d, handles)) {
        Some(Ok((mesh, surfaces, _))) => {
            model.handle = meshes.add(mesh);
            let added: Vec<(usize, Handle<Mesh>)> = surfaces.into_iter().map(|(section, m)| (section, meshes.add(m))).collect();
            for surface in &mut model.surfaces {
                if let Some((_, h)) = added.iter().find(|(section, _)| *section == surface.source_section) {
                    surface.handle = h.clone();
                }
            }
        }
        Some(Err(e)) => debug!("rigid block morph {:08X}: {e} (drawn as authored)", key.definition.instance_id),
        None => {}
    }
    MORPHS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, model);
    MORPH_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    true
}

/// Make the morphed meshes of every block the run holds (designs, the
/// session, the stage records), a few per frame.
pub fn prepare_morphs(
    run: Res<RunProgress>,
    originals: Res<Originals>,
    rigid: Res<RigidEditor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut frame: Local<u32>,
) {
    *frame = frame.wrapping_add(1);
    // While a morph handle is dragged the new weight's mesh is made at once.
    let dragging = matches!(rigid.grab, Some(crate::rigid_editor_edit::Grab::Morph { .. }));
    if (*frame % 10 != 1 && !dragging) || originals.part_catalog.parts.is_empty() {
        return;
    }
    let mut blocks: Vec<(Option<u32>, u32, &RigidBlock)> = Vec::new();
    for v in run.rigid.designs.values().flat_map(|d| d.parts.iter()).chain(&run.vehicle.parts).chain(&run.building.parts) {
        if let Some(b) = v.block.as_ref().filter(|b| !b.handles.is_empty()) {
            blocks.push((v.definition_id, v.instance, b));
        }
    }
    for m in &run.ship.modules {
        if let Some(b) = m.block.as_ref().filter(|b| !b.handles.is_empty()) {
            blocks.push((m.definition_id, m.instance, b));
        }
    }
    for p in &run.parts {
        if let Some(b) = p.rigid.as_ref().filter(|b| !b.handles.is_empty()) {
            blocks.push((p.definition_id, p.instance, b));
        }
    }
    let mut made = 0;
    for (definition_id, instance, b) in blocks {
        for group in [asset_catalog::VEHICLE_PARTS, asset_catalog::BUILDING_PARTS] {
            if model_of(&originals, group, definition_id, instance).is_some() {
                if ensure_morph(&originals, &mut meshes, group, definition_id, instance, &b.handles) {
                    made += 1;
                }
                break;
            }
        }
        if made >= 12 {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Records <-> session parts
// ---------------------------------------------------------------------------

/// The record of a session part (`PartInstance` with a free transform).
pub fn to_record(p: &PartInstance) -> VehiclePart {
    let mut block = p.rigid.clone().unwrap_or_default();
    block.pair = p.pair_id;
    block.mirrored = p.mirrored;
    VehiclePart {
        instance: p.instance,
        definition_id: p.definition_id,
        slot: 0,
        yaw: 0.0,
        scale: p.scale,
        offset: [0.0; 3],
        block: Some(block),
    }
}

/// The session part of a record that has a free transform.
pub fn from_record(v: &VehiclePart, block: &RigidBlock) -> PartInstance {
    PartInstance {
        instance: v.instance,
        definition_id: v.definition_id,
        bone: 0,
        yaw: 0.0,
        scale: v.scale,
        offset: [0.0; 3],
        pair_id: block.pair,
        mirrored: block.mirrored,
        rigid: Some(block.clone()),
        edits: Default::default(),
    }
}

/// UFO modules of a design (`ship_bonus` reads the module's slot kind: weapon,
/// hull or engine). ADAPTED: the module's slot follows the palette tab of its
/// part (`weapon` 0, `hull` 2 and `engine` 3 of `SHIP_SLOTS`).
pub fn to_ship_module(p: &PartInstance, slot: u32) -> ShipModule {
    let v = to_record(p);
    ShipModule {
        instance: v.instance,
        definition_id: v.definition_id,
        slot,
        yaw: 0.0,
        scale: v.scale,
        offset: [0.0; 3],
        block: v.block,
    }
}

pub fn ship_module_to_vehicle_part(m: &ShipModule) -> VehiclePart {
    VehiclePart {
        instance: m.instance,
        definition_id: m.definition_id,
        slot: m.slot,
        yaw: m.yaw,
        scale: m.scale,
        offset: m.offset,
        block: m.block.clone(),
    }
}

// ---------------------------------------------------------------------------
// Models of blocks
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    asset_catalog::{self, PartDefinition},
    creature_editor::CreatureEditor,
    creature_editor_data::CreatureEditorData,
    dbpf::ResourceKey,
    editor::EditorState,
    game::{Health, Hunger, Mover, Player},
    mechanical_assets,
    stages::StageOwned,
    OriginalModel, Originals,
};

/// The imported model of a saved block (`definition_id` is the exact PROP, a
/// record without one resolves its model instance canonically).
pub fn model_of<'a>(originals: &'a Originals, group: u32, definition_id: Option<u32>, instance: u32) -> Option<&'a OriginalModel> {
    if let Some(d) = definition_id {
        let key = ResourceKey { type_id: asset_catalog::PROP, group_id: group, instance_id: d };
        if let Some(m) = originals.mechanical_models.get(&key) {
            return Some(m);
        }
    }
    mechanical_assets::cached(originals, group, instance)
}

/// Import a saved block's definition so it can be shown (the stages and the
/// editor share `Originals::mechanical_models`). `Err` names what is missing;
/// the record is never touched.
pub fn import_block(
    originals: &mut Originals,
    group: u32,
    definition_id: Option<u32>,
    instance: u32,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(), String> {
    let definition: PartDefinition = definition_id
        .and_then(|d| originals.part_catalog.by_definition_in(group, d))
        .or_else(|| originals.part_catalog.by_model_in(group, instance))
        .cloned()
        .ok_or_else(|| format!("part {instance:08X} is not an original part of group {group:08X}"))?;
    mechanical_assets::import(originals, &definition, meshes, images, materials).map(|_| ())
}

/// The importer scales every model by `5 * modelScale` (the creature
/// editor's adaptation); the rigid editors work in the part's own units
/// (`modelScale`: the retail editor metres, a vehicle body is ~3 long, the
/// vehicle bound size is 6).
pub fn editor_unit(model: &OriginalModel) -> f32 {
    model.scale / 5.0
}

/// Spawn a block's draw surfaces under the transform-only entity `block`: one
/// child per authored surface with its own material, or the whole mesh with
/// the model's material. Authored surfaces get the block's `paints`
/// (`rigid_paint::apply_surface_paint` swaps in the painted material).
pub fn spawn_surfaces(
    commands: &mut Commands,
    block: Entity,
    model: &OriginalModel,
    materials: &mut Assets<StandardMaterial>,
    light: Option<&mut HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>>,
    paints: &[crate::rigid_paint::RegionPaint],
) {
    let lit = light.is_some();
    let shared: std::sync::Arc<[crate::rigid_paint::RegionPaint]> = paints.into();
    let authored = !model.surfaces.is_empty();
    let mut children: Vec<(Handle<Mesh>, Option<Handle<StandardMaterial>>)> =
        model.surfaces.iter().map(|s| (s.handle.clone(), Some(s.material.clone()))).collect();
    if children.is_empty() {
        children.push((model.handle.clone(), model.material.clone()));
    }
    let mut light = light;
    for (surface, (mesh, material)) in children.into_iter().enumerate() {
        let mut material = material.unwrap_or_else(|| {
            materials.add(StandardMaterial { base_color: Color::srgb(0.75, 0.75, 0.8), perceptual_roughness: 0.5, ..default() })
        });
        // The selected block: every surface's own material, lit.
        if let Some(cache) = light.as_deref_mut() {
            material = cache
                .entry(material.id())
                .or_insert_with(|| {
                    let mut copy = materials.get(&material).cloned().unwrap_or_default();
                    copy.emissive = LinearRgba::new(0.16, 0.2, 0.32, 1.0);
                    materials.add(copy)
                })
                .clone();
        }
        let mut child = commands.spawn((StageOwned, Mesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY, ChildOf(block)));
        if authored {
            child.insert(crate::rigid_paint::RigidSurfacePaint { definition: model.source_definition, surface, paints: shared.clone(), lit });
            // A drawn surface, not a root: `material_binding::attach_remaining`
            // otherwise took the first surface for a whole model and spawned
            // the other surfaces again under it with their imported materials
            // (the debug-coloured region textures over the painted blocks).
            if let Some(s) = model.surfaces.get(surface) {
                child.insert(crate::material_binding::OriginalSurfaceChild {
                    source_section: s.source_section,
                    state_sections: s.state_sections.clone(),
                });
            }
        }
    }
}

/// Triangles and bounds of a part's mesh in its own space (picking).
#[derive(Debug)]
pub struct Hull {
    pub tris: Vec<[Vec3; 3]>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Hull {
    pub fn of(meshes: &Assets<Mesh>, handle: &Handle<Mesh>) -> Option<Hull> {
        let mesh = meshes.get(handle)?;
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
            return None;
        };
        let p: Vec<Vec3> = positions.iter().map(|v| Vec3::from_array(*v)).collect();
        let index: Vec<usize> = match mesh.indices()? {
            bevy::mesh::Indices::U16(v) => v.iter().map(|i| *i as usize).collect(),
            bevy::mesh::Indices::U32(v) => v.iter().map(|i| *i as usize).collect(),
        };
        let tris: Vec<[Vec3; 3]> = index
            .chunks_exact(3)
            .filter(|t| t.iter().all(|i| *i < p.len()))
            .map(|t| [p[t[0]], p[t[1]], p[t[2]]])
            .collect();
        let (min, max) = p
            .iter()
            .filter(|v| v.is_finite())
            .fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        (min.is_finite() && max.is_finite()).then_some(Hull { tris, min, max })
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// The root of the blocks being edited: the camera's subject (it carries the
/// `Player` marker like the outfitter's preview creature).
#[derive(Component)]
pub struct RigidPreview;

/// The blocks' parent, a child of the preview turned about the vertical axis:
/// the blocks and the studio are in the importer's basis (Spore (x, y, z) ->
/// (x, z, -y), a part's front is +Z), the editor camera and light are in the
/// creature editor's (`creature_editor::spore_to_bevy`), a half turn apart.
#[derive(Component)]
pub struct RigidHolder;

/// A real player entity that lost its `Player` marker for the session.
#[derive(Component)]
pub struct RigidShelved;

/// A drawn block of the session (child of the preview).
#[derive(Component)]
pub struct RigidBlockVisual {
    pub index: usize,
}

/// A surface of the studio base or backdrop.
#[derive(Component)]
pub struct RigidStudio;

/// What the block entities show now.
#[derive(Default)]
pub struct Shown {
    pub entities: Vec<Entity>,
    pub key: Vec<(u32, Option<u32>)>,
    pub selected: Option<usize>,
    pub root: Option<Entity>,
    /// Blocks whose model was imported when the entities were built.
    pub ready: usize,
    /// The blocks' paints when the entities were built.
    pub paint: u32,
}

#[derive(Resource, Default)]
pub struct RigidEditor {
    pub active: bool,
    pub kind: SetupKind,
    /// The class the next session opens with.
    pub requested: Option<SetupKind>,
    /// Open this class as soon as the running session has ended (`[` `]` `V`).
    pub reopen: Option<SetupKind>,
    pub stage: Option<GameStage>,
    pub classes: Vec<SetupKind>,
    /// Studio models of the class: (name, PROP key).
    pub studio: Vec<(String, ResourceKey)>,
    studio_done: bool,
    /// The record changed (an accepted session): the stages remount.
    pub pending_sync: bool,
    hidden: Vec<Entity>,
    pub shown: Shown,
    /// Selected block (an index of `run.parts`).
    pub selected: Option<usize>,
    pub hulls: HashMap<ResourceKey, Arc<Hull>>,
    pub highlight: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    /// What the left button holds (`rigid_editor_edit`).
    pub grab: Option<crate::rigid_editor_edit::Grab>,
    pub wheel: Option<crate::rigid_editor_edit::WheelOp>,
    /// The handle under the pointer.
    pub hover: Option<crate::rigid_editor_edit::HandleKind>,
    pub handle_assets: Option<crate::rigid_editor_edit::HandleAssets>,
    pub handle_entities: Vec<(Entity, crate::rigid_editor_edit::HandleKind)>,
    /// Scripted edits (captures, the GPU test), one per frame.
    pub script: std::collections::VecDeque<crate::rigid_editor_edit::Step>,
    /// The retail morph handles of each part definition (`modelHandleKeys`
    /// against the model's morph data), read once.
    pub morph_handles: HashMap<ResourceKey, Arc<crate::creature_handles::PartHandles>>,
    /// The stacking rules of each part definition (`rigid_editor_stack`), read once.
    pub stack_rules: HashMap<ResourceKey, Arc<crate::rigid_editor_stack::StackRules>>,
}

/// A handle of the selected block (a ring or a deform sphere).
#[derive(Component)]
pub struct RigidHandle;

impl RigidEditor {
    /// The morph handles of a block's part (`None` when the part is not an
    /// original definition or has none).
    pub fn part_handles(&mut self, originals: &Originals, group: u32, p: &PartInstance) -> Option<Arc<crate::creature_handles::PartHandles>> {
        let def = originals.part_catalog.by_part_in(group, p)?;
        let loaded = self
            .morph_handles
            .entry(def.key)
            .or_insert_with(|| Arc::new(crate::creature_handles::load_part_handles(&originals.resource_index, def)));
        (!loaded.handles.is_empty()).then(|| loaded.clone())
    }

    /// The stacking rules of a block's part (`None` when the part is not an
    /// original definition).
    pub fn stack_rules(&mut self, originals: &Originals, group: u32, p: &PartInstance) -> Option<Arc<crate::rigid_editor_stack::StackRules>> {
        let def = originals.part_catalog.by_part_in(group, p)?;
        Some(
            self.stack_rules
                .entry(def.key)
                .or_insert_with(|| Arc::new(crate::rigid_editor_stack::StackRules::load(&originals.resource_index, def.key)))
                .clone(),
        )
    }

    /// The stacking rules of every block of the session, in block order.
    pub fn all_stack_rules(&mut self, originals: &Originals, group: u32, parts: &[PartInstance]) -> Vec<Option<Arc<crate::rigid_editor_stack::StackRules>>> {
        parts.iter().map(|p| self.stack_rules(originals, group, p)).collect()
    }

    /// The hull of a block's mesh (cached by the definition).
    pub fn hull(&mut self, meshes: &Assets<Mesh>, originals: &Originals, group: u32, p: &PartInstance) -> Option<Arc<Hull>> {
        let model = model_of(originals, group, p.definition_id, p.instance)?;
        self.hull_of(meshes, model)
    }

    pub fn hull_of(&mut self, meshes: &Assets<Mesh>, model: &OriginalModel) -> Option<Arc<Hull>> {
        let key = model.source_definition.unwrap_or(ResourceKey { type_id: 0, group_id: 0, instance_id: model.instance });
        if let Some(h) = self.hulls.get(&key) {
            return Some(h.clone());
        }
        let hull = Arc::new(Hull::of(meshes, &model.handle)?);
        self.hulls.insert(key, hull.clone());
        Some(hull)
    }
}

/// Where the preview stands (the outfitter's: far above the stage).
pub const PREVIEW_AT: Vec3 = crate::outfitter::PREVIEW_AT;

/// Orientation of the holder in the preview: the half turn between the
/// importer's basis and the editor camera's (`RigidHolder`).
pub fn preview_rotation() -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI)
}

/// The holder's transform for a preview root: the space the blocks, the
/// studio and the pointer's rays are in.
pub fn holder_of(root: &Transform) -> Transform {
    Transform { translation: root.translation, rotation: root.rotation * preview_rotation(), scale: root.scale }
}

/// Run condition: the editor wants the stages to remount a design.
pub fn sync_pending(rigid: Res<RigidEditor>) -> bool {
    rigid.pending_sync
}

fn clear_pending(mut rigid: ResMut<RigidEditor>) {
    rigid.pending_sync = false;
}

/// Put the real parts back after a session that did not end (a save written
/// during the session, a crash).
pub fn repair(run: &mut RunProgress) -> bool {
    let Some(backup) = run.rigid.session.take() else {
        return false;
    };
    run.parts = backup.parts;
    run.editor_dna = backup.dna;
    run.creature_creation.name = backup.name;
    run.focused_part = backup.focused_part;
    run.sync_legacy_accessory();
    run.request_save();
    true
}

fn repair_system(rigid: Res<RigidEditor>, mut run: ResMut<RunProgress>) {
    if !rigid.active && run.rigid.session.is_some() {
        repair(&mut run);
    }
}

// ---------------------------------------------------------------------------
// Budget and designs (pure)
// ---------------------------------------------------------------------------

/// Price of a design (a symmetric pair is one purchase).
pub fn design_cost(data: &CreatureEditorData, parts: &[PartInstance]) -> f32 {
    let mut seen: Vec<u32> = Vec::new();
    let mut total = 0.0;
    for p in parts {
        if p.pair_id != 0 {
            if seen.contains(&p.pair_id) {
                continue;
            }
            seen.push(p.pair_id);
        }
        total += data.info_of(p).map_or(0, |i| i.price) as f32;
    }
    total
}

/// The session's budget: `editorDefaultInitialBudget` minus the cost of the
/// loaded design (`editorSubtractPartsCostOnLoad`).
pub fn session_budget(data: &CreatureEditorData, kind: SetupKind, parts: &[PartInstance]) -> f32 {
    let Some(s) = data.setup(kind) else {
        return 0.0;
    };
    let initial = s.initial_budget.unwrap_or(5000) as f32;
    let cost = if s.subtract_cost_on_load { design_cost(data, parts) } else { 0.0 };
    (initial - cost).max(0.0)
}

/// Slot of a UFO module by the palette tab of its part (`SHIP_SLOTS`: 0 a
/// weapon, 2 hull, 3 engine, 4 cockpit); `ship_bonus` reads the slot's kind.
/// The UFO palette's tabs: Bodies, Cockpit, Space, Land, Water, Air, Weapons,
/// Effect, Detail.
pub fn ship_slot_of_tab(tab: usize) -> u32 {
    match tab {
        6 => 0,
        1 => 4,
        2..=5 => 3,
        _ => 2,
    }
}

/// Studio models of a class: the `editorBackgroundModel` and the base (the
/// setup's pedestal or swatch pedestal, else the retail base of the domain).
pub fn studio_models(data: &CreatureEditorData, kind: SetupKind) -> Vec<(String, ResourceKey)> {
    let Some(s) = data.setup(kind) else {
        return vec![];
    };
    let key = |id: u32| ResourceKey { type_id: asset_catalog::PROP, group_id: 0x4060_6000, instance_id: id };
    let name = |id: u32| format!("{id:08X}");
    let base = s.pedestal_model.or(s.swatch_pedestal_model).or(match kind {
        // ADAPTED: the air and UFO templates do not name their base; the
        // retail models are `ve_editor_airbase` and `ve_editor_ufobase`.
        SetupKind::VehicleMilitaryAir | SetupKind::VehicleEconomicAir | SetupKind::VehicleCulturalAir => Some(0x8E29_8BFC),
        SetupKind::Ufo => Some(0x8977_9F80),
        _ => None,
    });
    [s.background_model, base].into_iter().flatten().map(|id| (name(id), key(id))).collect()
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct RigidEditorPlugin;

impl Plugin for RigidEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RigidEditor>()
            .init_resource::<crate::rigid_paint::RigidPaintCache>()
            .init_resource::<crate::rigid_paint::RigidPaintData>()
            .init_resource::<crate::rigid_paint::RigidPaintSession>()
            .init_resource::<crate::rigid_stats::TestDrive>()
            .add_systems(
                Update,
                (crate::rigid_stats::check_playable, crate::rigid_stats::test_drive).chain().after(visuals).before(crate::spui::view::sync),
            )
            .add_systems(Update, (open_key, class_keys).chain().before(crate::editor::input))
            .add_systems(Update, prepare_morphs.before(visuals))
            .add_systems(
                Update,
                (repair_system, sync).chain().after(crate::editor::input).before(crate::editor::scene),
            )
            .add_systems(Update, hold_scene.after(crate::editor::scene).before(crate::editor::drag))
            .add_systems(
                Update,
                (
                    crate::rigid_editor_edit::scripted,
                    crate::rigid_editor_edit::actions,
                    crate::rigid_editor_edit::manipulate.before(crate::creature_editor::mouse_camera),
                    crate::rigid_editor_edit::handles,
                    crate::rigid_paint::fill_new_blocks,
                    crate::rigid_paint::paint_ui,
                    visuals,
                    studio,
                )
                    .chain()
                    .after(crate::editor::drag)
                    .after(crate::spui::view::input)
                    .before(crate::spui::view::sync),
            )
            .add_systems(
                Update,
                (hide_replaced_halls, hide_replaced_art).run_if(in_state(GameStage::Civilization)),
            )
            .add_systems(Update, hide_replaced_hull.run_if(in_state(GameStage::Space)))
            .add_systems(
                Update,
                import_designs
                    .before(crate::civ::sync_creator_assemblies)
                    .before(crate::space::sync_creator_loadout),
            )
            .add_systems(PostUpdate, crate::rigid_paint::apply_surface_paint)
            .add_systems(PostUpdate, crate::rigid_stats::follow_camera.after(crate::creature_editor::place_camera).before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, clear_pending);
    }
}

fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

/// F3 opens the stage's rigid editor (Shift+F3 is the older assembly modal);
/// a class change reopens it.
#[allow(clippy::too_many_arguments)]
fn open_key(
    keys: Res<ButtonInput<KeyCode>>,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    mut state: ResMut<EditorState>,
    mut rigid: ResMut<RigidEditor>,
    mut editor: ResMut<CreatureEditor>,
    mechanical: Res<crate::mechanical_creator::MechanicalCreator>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    outfitter: Res<crate::outfitter::Outfitter>,
) {
    let here = stage.get();
    // F3 again closes the editor and keeps the design (the older modal's F3).
    if rigid.active && state.open && keys.just_pressed(KeyCode::F3) && !shift_held(&keys) {
        state.open = false;
        return;
    }
    if rigid.active || state.open {
        return;
    }
    let class = if let Some(k) = rigid.reopen.take() {
        Some(k)
    } else if keys.just_pressed(KeyCode::F3)
        && !shift_held(&keys)
        && !mechanical.open
        && !outfitter.active
        && !pause.is_some_and(|p| p.open)
    {
        first_class(here, &run)
    } else {
        None
    };
    let Some(class) = class else {
        return;
    };
    rigid.requested = Some(class);
    rigid.classes = classes(here, &run);
    editor.requested = Some(class);
    state.show();
    state.rigid = true;
    state.visuals_dirty = true;
}

/// `[` / `]` change the class (reopening the editor with its own palette and
/// studio, the design is kept), `V` jumps between the vehicles and the
/// buildings (ADAPTED: retail has the model type chooser, `EditorSharedUI`).
fn class_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<EditorState>,
    mut rigid: ResMut<RigidEditor>,
    editor: Res<CreatureEditor>,
) {
    if !rigid.active || !state.open || !editor.active || editor.options_open || editor.naming || state.dragging() {
        return;
    }
    let list = rigid.classes.clone();
    let Some(at) = list.iter().position(|k| *k == rigid.kind) else {
        return;
    };
    let n = list.len();
    let next = if keys.just_pressed(KeyCode::BracketRight) {
        Some(list[(at + 1) % n])
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        Some(list[(at + n - 1) % n])
    } else if keys.just_pressed(KeyCode::KeyV) {
        let building = rigid.kind.is_building();
        list.iter().copied().find(|k| k.is_building() != building)
    } else {
        None
    };
    if let Some(k) = next.filter(|k| *k != rigid.kind) {
        rigid.reopen = Some(k);
        // The session ends (the design is stored) and opens again.
        state.open = false;
    }
}

/// `editor::scene` hides what exists when the editor opens; the stages keep
/// spawning (the Space stage's ships), so what appears later is hidden too.
fn hold_scene(
    mut commands: Commands,
    rigid: Res<RigidEditor>,
    roots: Query<
        (Entity, Option<&Visibility>),
        (
            With<StageOwned>,
            With<Transform>,
            Without<ChildOf>,
            Without<Node>,
            Without<Player>,
            Without<crate::creature::Accessory>,
            Without<RigidPreview>,
            Without<RigidStudio>,
            Without<RigidHandle>,
            Without<crate::editor::EditorHidden>,
        ),
    >,
) {
    if !rigid.active {
        return;
    }
    for (e, visibility) in &roots {
        commands
            .entity(e)
            .insert((crate::editor::EditorHidden(visibility.copied().unwrap_or(Visibility::Inherited)), Visibility::Hidden));
    }
}

/// Begin and end the session with the editor's `open` flag.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    mut state: ResMut<EditorState>,
    mut rigid: ResMut<RigidEditor>,
    mut data: ResMut<CreatureEditorData>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    editor: Res<CreatureEditor>,
    players: Query<Entity, With<Player>>,
    shelved: Query<Entity, With<RigidShelved>>,
    children: Query<&Children>,
    mut visibility: Query<&mut Visibility>,
    leftovers: Query<Entity, Or<(With<RigidPreview>, With<RigidStudio>, With<RigidHandle>)>>,
) {
    let here = stage.get().clone();
    if !rigid.active {
        if !(state.open && state.rigid) {
            return;
        }
        if !data.loaded && !originals.resource_index.records.is_empty() {
            *data = CreatureEditorData::load(&originals.resource_index);
            for e in &data.errors {
                warn!("creature editor data: {e}");
            }
        }
        let Some(kind) = rigid.requested.take().or_else(|| first_class(&here, &run)) else {
            state.open = false;
            state.rigid = false;
            return;
        };
        let group = kind.part_group();
        // F3 reopens the last type of the Civ editor (vehicles or buildings).
        if here == GameStage::Civilization {
            run.civ_campaign.editor_mode = if kind.is_building() { crate::civ::CivEditorMode::Building } else { crate::civ::CivEditorMode::Vehicle };
        }
        // The design to edit: the class's record, else the stage's old
        // slot-based record converted to blocks.
        let (parts, name) = load_design(&run, kind, &mut originals, &mut meshes, &mut images, &mut materials);
        run.rigid.session = Some(RigidBackup {
            parts: run.parts.clone(),
            dna: run.editor_dna,
            name: run.creature_creation.name.clone(),
            focused_part: run.focused_part,
        });
        run.editor_dna = session_budget(&data, kind, &parts);
        run.creature_creation.name = name;
        run.focused_part = parts.len().saturating_sub(1) as u32;
        run.parts = parts;
        run.sync_legacy_accessory();
        state.locked_parts = 0;
        state.clear_history();
        // Import the blocks (a part that does not import is reported and the
        // record keeps it).
        for p in run.parts.clone() {
            if let Err(e) = import_block(&mut originals, group, p.definition_id, p.instance, &mut meshes, &mut images, &mut materials) {
                warn!("rigid editor: {e}");
            }
        }
        // The isolated scene: the real players lose the marker (hidden by
        // `editor::scene`), the preview root is the camera's subject.
        for e in &players {
            commands.entity(e).remove::<Player>().insert(RigidShelved);
            let mut stack = vec![e];
            while let Some(parent) = stack.pop() {
                if let Ok(kids) = children.get(parent) {
                    for kid in kids.iter() {
                        stack.push(kid);
                        if let Ok(mut v) = visibility.get_mut(kid) {
                            if *v == Visibility::Visible {
                                *v = Visibility::Hidden;
                                rigid.hidden.push(kid);
                            }
                        }
                    }
                }
            }
        }
        let root = commands
            .spawn((
                StageOwned,
                RigidPreview,
                crate::outfitter::OutfitterPreview,
                Player,
                Mover { speed: 0. },
                Health::new(120.),
                Hunger::new(120.),
                Visibility::Inherited,
                Transform { translation: PREVIEW_AT, rotation: Quat::IDENTITY, scale: Vec3::ONE },
                Name::new("rigid editor preview"),
            ))
            .id();
        commands.spawn((
            StageOwned,
            RigidHolder,
            Visibility::Inherited,
            Transform::from_rotation(preview_rotation()),
            ChildOf(root),
            Name::new("rigid editor blocks"),
        ));
        rigid.shown = Shown::default();
        rigid.selected = None;
        rigid.grab = None;
        rigid.wheel = None;
        rigid.studio = studio_models(&data, kind);
        rigid.studio_done = false;
        rigid.kind = kind;
        rigid.stage = Some(here.clone());
        rigid.classes = classes(&here, &run);
        rigid.active = true;
        state.visuals_dirty = true;
        info!("rigid editor opened ({kind:?}): {} blocks, budget {}", run.parts.len(), run.editor_dna);
        return;
    }
    if state.open && state.rigid && rigid.stage.as_ref() == Some(&here) {
        return;
    }
    // End of the session: Cancel keeps the record, anything else stores it.
    let kind = rigid.kind;
    if let Some(backup) = run.rigid.session.take() {
        if !editor.cancelled {
            commit(&mut run, kind, &data);
            rigid.pending_sync = true;
        }
        run.parts = backup.parts;
        run.editor_dna = backup.dna;
        run.creature_creation.name = backup.name;
        run.focused_part = backup.focused_part;
        run.sync_legacy_accessory();
        run.request_save();
    }
    state.locked_parts = 0;
    state.rigid = false;
    state.clear_history();
    rigid.active = false;
    rigid.stage = None;
    rigid.shown = Shown::default();
    rigid.selected = None;
    rigid.grab = None;
    rigid.wheel = None;
    rigid.hover = None;
    rigid.handle_entities.clear();
    for e in &leftovers {
        commands.entity(e).despawn();
    }
    for e in &shelved {
        commands.entity(e).remove::<RigidShelved>().insert(Player);
    }
    for e in std::mem::take(&mut rigid.hidden) {
        if let Ok(mut v) = visibility.get_mut(e) {
            *v = Visibility::Visible;
        }
    }
    state.visuals_dirty = true;
}

/// The class's design as session parts and its name: the saved design, else
/// the stage's old slot-based record converted (`legacy_block`), else empty.
fn load_design(
    run: &RunProgress,
    kind: SetupKind,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (Vec<PartInstance>, String) {
    use crate::civ::Approach;
    let group = kind.part_group();
    let own_land = matches!(
        (kind, run.civ_campaign.approach),
        (SetupKind::VehicleMilitaryLand, Approach::Military)
            | (SetupKind::VehicleEconomicLand, Approach::Economic)
            | (SetupKind::VehicleCulturalLand, Approach::Religious)
    );
    let (records, name): (Vec<VehiclePart>, String) = if let Some(d) = run.rigid.design(kind) {
        (d.parts.clone(), d.name.clone())
    } else if kind == SetupKind::Ufo {
        (run.ship.modules.iter().map(ship_module_to_vehicle_part).collect(), String::new())
    } else if kind == SetupKind::BuildingCityHall {
        (run.building.parts.clone(), String::new())
    } else if own_land {
        // The old single vehicle build is the capital type's land vehicle.
        (run.vehicle.parts.clone(), String::new())
    } else {
        (vec![], String::new())
    };
    let stage_scale = stage_scale_of(kind);
    let parts = records
        .iter()
        .map(|v| match &v.block {
            Some(block) => from_record(v, block),
            None => {
                let (position, yaw) = legacy_stage_pose(kind, v);
                let mut part = from_record(v, &legacy_block(position, yaw, stage_scale));
                // Keep the part's look: the stage showed it at
                // `capped scale * base * scale`, the editor shows
                // `model scale * scale`.
                if import_block(originals, group, v.definition_id, v.instance, meshes, images, materials).is_ok() {
                    if let Some(model) = model_of(originals, group, v.definition_id, v.instance) {
                        let shown = legacy_unit(kind, meshes, model) * v.scale.clamp(0.25, 4.0);
                        part.scale = (shown / (editor_unit(model) * stage_scale).max(1e-6)).clamp(0.1, 10.0);
                    }
                }
                part
            }
        })
        .collect();
    (parts, name)
}

pub fn stage_scale_of(kind: SetupKind) -> f32 {
    if kind.is_building() {
        BUILDING_STAGE_SCALE
    } else if kind == SetupKind::Ufo {
        SHIP_STAGE_SCALE
    } else {
        VEHICLE_STAGE_SCALE
    }
}

/// Where the old slot record put a part in the stage (relative to the
/// design's origin) and its yaw.
fn legacy_stage_pose(kind: SetupKind, v: &VehiclePart) -> (Vec3, f32) {
    if kind.is_building() {
        crate::civ::city_part_transform(v.slot, v)
    } else if kind == SetupKind::Ufo {
        let m = ShipModule { instance: v.instance, definition_id: v.definition_id, slot: v.slot, yaw: v.yaw, scale: v.scale, offset: v.offset, block: None };
        crate::game::ship_module_transform(v.slot, &m)
    } else {
        let (offset, yaw) = crate::game::vehicle_part_transform(v.slot, v);
        (offset * 0.6, yaw)
    }
}

/// The uniform size the stage gave an old slot-based part (before its own
/// multiplier).
fn legacy_unit(kind: SetupKind, meshes: &Assets<Mesh>, model: &OriginalModel) -> f32 {
    if kind.is_building() {
        mechanical_assets::capped_scale(meshes, model, crate::civ::BUILDING_PART_CAP) * crate::civ::CITY_PART_BASE
    } else if kind == SetupKind::Ufo {
        mechanical_assets::capped_scale(meshes, model, crate::space::SHIP_MODULE_WORLD_CAP / crate::space::MODULE_BASE_FRACTION)
            * crate::space::MODULE_BASE_FRACTION
    } else {
        mechanical_assets::capped_scale(meshes, model, crate::civ::VEHICLE_PART_CAP) * crate::civ::VEHICLE_PART_BASE
    }
}

/// Store the session's blocks: the class record, and the stage's own record
/// where the stage reads it (the UFO's modules, the city hall's skyline).
fn commit(run: &mut RunProgress, kind: SetupKind, data: &CreatureEditorData) {
    if kind == SetupKind::Ufo {
        // `editorMoveModelToCenterOfMass` (only the UFO setup).
        crate::rigid_stats::center_of_mass(&mut run.parts);
    }
    let parts: Vec<VehiclePart> = run.parts.iter().map(to_record).collect();
    run.rigid.designs.insert(
        kind.setup_name().to_string(),
        RigidDesign { name: run.creature_creation.name.clone(), parts: parts.clone() },
    );
    if kind == SetupKind::Ufo {
        let palette = data.parts_palette(kind);
        run.ship.modules = run
            .parts
            .iter()
            .map(|p| {
                let tab = data
                    .definition_of(p)
                    .and_then(|d| palette.and_then(|pal| pal.locate(d)))
                    .map_or(usize::MAX, |(c, _)| c);
                to_ship_module(p, ship_slot_of_tab(tab))
            })
            .collect();
        run.ship.focused = run.ship.focused.min(run.ship.modules.len().saturating_sub(1) as u32);
    } else if kind == SetupKind::BuildingCityHall {
        run.building.parts = parts;
        run.building.focused = run.building.focused.min(run.building.parts.len().saturating_sub(1) as u32);
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Keep one entity per block, posed by its free transform; the selected block
/// is lit.
#[allow(clippy::too_many_arguments)]
fn visuals(
    mut commands: Commands,
    mut rigid: ResMut<RigidEditor>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<Entity, With<RigidHolder>>,
    mut blocks: Query<&mut Transform, With<RigidBlockVisual>>,
) {
    let _ = &meshes;
    if !rigid.active {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };
    let group = rigid.kind.part_group();
    let key: Vec<(u32, Option<u32>)> = run.parts.iter().map(|p| (p.instance, p.definition_id)).collect();
    let selected = rigid.selected.filter(|i| *i < run.parts.len());
    let models: Vec<Option<OriginalModel>> = run
        .parts
        .iter()
        .map(|p| block_model(&originals, group, p.definition_id, p.instance, p.rigid.as_ref().map_or(&[][..], |b| &b.handles[..])))
        .collect();
    let ready = models.iter().filter(|m| m.is_some()).count();
    // Paint and morphed meshes (handle weights, `prepare_morphs`) rebuild too.
    let paint = run.parts.iter().fold(0u32, |h, p| {
        h.rotate_left(5)
            ^ p.rigid.as_ref().map_or(0, |b| {
                crate::rigid_paint::paints_signature(&b.paints) ^ if b.handles.is_empty() { 0 } else { morph_generation().wrapping_mul(0x9E37_79B9) }
            })
    });
    let rebuild = rigid.shown.key != key
        || rigid.shown.selected != selected
        || rigid.shown.root != Some(root)
        || rigid.shown.ready != ready
        || rigid.shown.paint != paint;
    let pose = |p: &PartInstance, model: &OriginalModel| -> Transform {
        let mut block = p.rigid.clone().unwrap_or_default();
        block.mirrored = p.mirrored;
        block.transform(editor_unit(model) * p.scale)
    };
    if rebuild {
        for e in std::mem::take(&mut rigid.shown.entities).into_iter().filter(|e| *e != Entity::PLACEHOLDER) {
            commands.entity(e).despawn();
        }
        let mut entities = Vec::with_capacity(run.parts.len());
        for (i, p) in run.parts.iter().enumerate() {
            let Some(model) = models[i].as_ref() else {
                entities.push(Entity::PLACEHOLDER);
                continue;
            };
            let entity = commands
                .spawn((
                    StageOwned,
                    RigidBlockVisual { index: i },
                    Visibility::Inherited,
                    pose(p, model),
                    ChildOf(root),
                    Name::new(format!("rigid block {i}")),
                ))
                .id();
            let light = (selected == Some(i)).then_some(&mut rigid.highlight);
            let paints = p.rigid.as_ref().map_or(&[][..], |b| &b.paints[..]);
            spawn_surfaces(&mut commands, entity, model, &mut materials, light, paints);
            entities.push(entity);
        }
        rigid.shown = Shown { entities, key, selected, root: Some(root), ready, paint };
        return;
    }
    for (i, p) in run.parts.iter().enumerate() {
        let (Some(model), Some(&entity)) = (models[i].as_ref(), rigid.shown.entities.get(i)) else {
            continue;
        };
        if let Ok(mut t) = blocks.get_mut(entity) {
            let next = pose(p, model);
            if *t != next {
                *t = next;
            }
        }
    }
}

/// The class's studio: the retail base and backdrop of its setup.
#[allow(clippy::too_many_arguments)]
fn studio(
    mut commands: Commands,
    mut rigid: ResMut<RigidEditor>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    roots: Query<&Transform, With<RigidPreview>>,
) {
    if !rigid.active || rigid.studio_done {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };
    rigid.studio_done = true;
    let root = holder_of(root);
    for (name, key) in rigid.studio.clone() {
        let result = crate::world_objects::definition_at(&originals, key, &name).and_then(|d| {
            crate::editor_environment::import_room(&mut originals, d, &mut meshes, &mut images, &mut materials)
        });
        match result {
            Ok((model, source)) => {
                for surface in &model.surfaces {
                    commands.spawn((
                        StageOwned,
                        RigidStudio,
                        source.clone(),
                        crate::material_binding::OriginalSurfaceChild {
                            source_section: surface.source_section,
                            state_sections: surface.state_sections.clone(),
                        },
                        Name::new(format!("rigid editor studio {name} surface {}", surface.source_section)),
                        Mesh3d(surface.handle.clone()),
                        MeshMaterial3d(surface.material.clone()),
                        Transform { translation: root.translation, rotation: root.rotation, scale: Vec3::ONE },
                    ));
                }
            }
            Err(e) => warn!("rigid editor studio model {name}: {e}"),
        }
    }
}


// ---------------------------------------------------------------------------
// The stages' side of the designs
// ---------------------------------------------------------------------------

/// ADAPTED: the largest extent of the UFO design drawn as the whole ship in
/// Space, the size the stage fits the original `genufo` preset hull to
/// (`creation_assets::mount_preset(.., Spaceship, 2.4)`), so a designed ship
/// is as big as the ship it replaces.
pub const SHIP_DESIGN_EXTENT: f32 = 2.4;

/// Stage scale and lift of the UFO design drawn as the whole ship: the
/// blocks' bounds (editor metres) fitted to [`SHIP_DESIGN_EXTENT`] and
/// centred on the ship's origin. `None` without drawable blocks.
pub fn ship_design_fit(meshes: &Assets<Mesh>, originals: &Originals, modules: &[ShipModule]) -> Option<(f32, Vec3)> {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for m in modules {
        let Some(block) = &m.block else { continue };
        let Some(model) = model_of(originals, asset_catalog::VEHICLE_PARTS, m.definition_id, m.instance) else { continue };
        let Some(hull) = Hull::of(meshes, &model.handle) else { continue };
        let t = block.transform(editor_unit(model) * m.scale.clamp(0.1, 4.0));
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { hull.min.x } else { hull.max.x },
                if i & 2 == 0 { hull.min.y } else { hull.max.y },
                if i & 4 == 0 { hull.min.z } else { hull.max.z },
            );
            let w = t.transform_point(c);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    let extent = (hi - lo).max_element();
    if !extent.is_finite() || extent <= 1e-4 {
        return None;
    }
    let scale = SHIP_DESIGN_EXTENT / extent;
    Some((scale, -(lo + hi) * 0.5 * scale))
}

/// Whether the player's ship is drawn from the UFO design (its modules carry
/// free-transform blocks).
pub fn ship_is_designed(run: &RunProgress) -> bool {
    run.ship.modules.iter().any(|m| m.block.is_some())
}

/// The UFO design replaces the ship's old hull (the `genufo` preset root
/// under the player's ship is hidden while the design is drawn).
fn hide_replaced_hull(
    mut commands: Commands,
    run: Res<RunProgress>,
    ships: Query<(), (With<Player>, With<crate::space::ShipMotion>)>,
    hulls: Query<(Entity, &ChildOf, Option<&Visibility>), With<crate::creation_assets::OriginalCreation>>,
) {
    let want = if ship_is_designed(&run) { Visibility::Hidden } else { Visibility::Inherited };
    for (entity, parent, visibility) in &hulls {
        if ships.contains(parent.parent()) && visibility != Some(&want) {
            commands.entity(entity).insert(want);
        }
    }
}

/// Floor of a city's hall in the city root's frame (`city_presentation`'s
/// `CITY_ROOT_CLEARANCE`: the sampled floor on the planet differs by the
/// curvature only). ADAPTED.
pub const HALL_FLOOR: f32 = -2.0;
/// Editor units to city units of a house, factory or entertainment building on
/// its ring plot (the plot's footprint is 2.3, the city hall's 3.5).
pub const PLOT_STAGE_SCALE: f32 = 0.13;

/// The design a ring plot of this building kind shows instead of the native
/// building, if its class was designed.
pub fn plot_design(run: &RunProgress, kind: crate::civ::Building) -> Option<&RigidDesign> {
    let class = match kind {
        crate::civ::Building::House => SetupKind::BuildingHouse,
        crate::civ::Building::Factory => SetupKind::BuildingFactory,
        crate::civ::Building::Entertainment => SetupKind::BuildingEntertainment,
    };
    run.rigid.design(class).filter(|d| d.parts.iter().any(|p| p.block.is_some()))
}

/// What a plot's building looks like (0 = the native building): a plot is
/// redrawn when it changes.
pub fn plot_design_signature(run: &RunProgress, kind: crate::civ::Building) -> u32 {
    let Some(design) = plot_design(run, kind) else { return 0 };
    let mut hash: u32 = 0x811c9dc5;
    for p in &design.parts {
        for bits in [p.instance, p.scale.to_bits(), block_signature(&p.block)] {
            hash ^= bits;
            hash = hash.wrapping_mul(16777619);
        }
    }
    hash | 1
}

/// Mount a designed building on a ring plot: a root at the plot, facing
/// outward, with the design's blocks scaled to the plot (`tag` is the root's
/// plot tag, the plot is redrawn by despawning it).
#[allow(clippy::too_many_arguments)]
pub fn mount_plot_design<T: Component>(
    commands: &mut Commands,
    originals: &Originals,
    materials: &mut Assets<StandardMaterial>,
    city: Entity,
    design: &RigidDesign,
    at: Vec3,
    yaw: f32,
    tag: T,
) {
    let root = commands
        .spawn((
            StageOwned,
            tag,
            Visibility::Inherited,
            Transform { translation: at, rotation: Quat::from_rotation_y(yaw), scale: Vec3::ONE },
            ChildOf(city),
        ))
        .id();
    for p in &design.parts {
        let Some(block) = &p.block else { continue };
        let Some(model) = block_model(originals, asset_catalog::BUILDING_PARTS, p.definition_id, p.instance, &block.handles) else {
            continue;
        };
        let model = &model;
        let unit = editor_unit(model) * p.scale.clamp(0.1, 4.0);
        let entity = commands
            .spawn((StageOwned, Visibility::Inherited, block_transform(block, unit, PLOT_STAGE_SCALE, Vec3::ZERO), ChildOf(root)))
            .id();
        spawn_surfaces(commands, entity, model, materials, None, &block.paints);
    }
}

/// The player's city hall design replaces the native hall of the cities it
/// stands in (`civ::mount_city_assembly` mounts the blocks).
fn hide_replaced_halls(
    run: Res<RunProgress>,
    mut halls: Query<(&ChildOf, &mut Visibility), With<crate::civ::NativeCityHallSurface>>,
    cities: Query<&crate::civ::CityMarker>,
    parents: Query<&ChildOf, Without<crate::civ::NativeCityHallSurface>>,
) {
    let designed = run.building.parts.iter().any(|p| p.block.is_some());
    for (parent, mut v) in &mut halls {
        // The hall hangs under the city root or one of its tower entities.
        let mut at = parent.parent();
        let mut city = None;
        for _ in 0..4 {
            if let Ok(m) = cities.get(at) {
                city = Some(m.0);
                break;
            }
            match parents.get(at) {
                Ok(up) => at = up.parent(),
                Err(_) => break,
            }
        }
        let owned = city.and_then(|i| run.civ_campaign.cities.get(i)).is_some_and(|c| c.owned());
        let want = if designed && owned { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
}

/// A vehicle of a designed class shows the design, not the preset creation of
/// its domain (`civ_rts::spawn_unit` draws the preset under the unit's art
/// anchor).
fn hide_replaced_art(
    run: Res<RunProgress>,
    units: Query<(&crate::civ_rts::CivUnit, &crate::civ_rts::UnitRig), With<crate::game::Vehicle>>,
    mut visibility: Query<&mut Visibility>,
) {
    for (unit, rig) in &units {
        if unit.nation != crate::civ::HOME_INDEX {
            continue;
        }
        let want = if run.rigid.vehicle_design(unit.kind, unit.domain).is_some() { Visibility::Hidden } else { Visibility::Inherited };
        if let Ok(mut v) = visibility.get_mut(rig.art) {
            if *v != want {
                *v = want;
            }
        }
    }
}

/// Import the models of every saved design (a fresh process has none) and
/// ask the stages to remount them.
#[allow(clippy::too_many_arguments)]
fn import_designs(
    mut rigid: ResMut<RigidEditor>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    stage: Res<State<GameStage>>,
    mut seen: Local<Option<(GameStage, u64)>>,
) {
    if originals.resource_index.records.is_empty() || !matches!(stage.get(), GameStage::Civilization | GameStage::Space) {
        return;
    }
    // Cheap identity of what the records hold.
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut mix = |v: u64| {
        hash ^= v;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    let mut wanted: Vec<(u32, Option<u32>, u32)> = Vec::new();
    for (name, design) in &run.rigid.designs {
        let group = if name.starts_with("Building") { asset_catalog::BUILDING_PARTS } else { asset_catalog::VEHICLE_PARTS };
        for p in design.parts.iter().filter(|p| p.block.is_some()) {
            mix(p.instance as u64 ^ ((p.definition_id.unwrap_or(0) as u64) << 32));
            wanted.push((group, p.definition_id, p.instance));
        }
    }
    for m in run.ship.modules.iter().filter(|m| m.block.is_some()) {
        mix(m.instance as u64);
        wanted.push((asset_catalog::VEHICLE_PARTS, m.definition_id, m.instance));
    }
    for p in run.building.parts.iter().filter(|p| p.block.is_some()) {
        mix(p.instance as u64);
        wanted.push((asset_catalog::BUILDING_PARTS, p.definition_id, p.instance));
    }
    let now = (stage.get().clone(), hash);
    if seen.as_ref() == Some(&now) {
        return;
    }
    *seen = Some(now);
    let mut imported = false;
    for (group, definition_id, instance) in wanted {
        if model_of(&originals, group, definition_id, instance).is_some() {
            continue;
        }
        match import_block(&mut originals, group, definition_id, instance, &mut meshes, &mut images, &mut materials) {
            Ok(()) => imported = true,
            Err(e) => warn!("rigid design part: {e}"),
        }
    }
    if imported {
        rigid.pending_sync = true;
    }
}

// ---------------------------------------------------------------------------
// Captures and render checks
// ---------------------------------------------------------------------------

/// The first available item of palette tab `tab`.
fn first_item(data: &CreatureEditorData, kind: SetupKind, tab: usize) -> Option<u32> {
    let palette = data.parts_palette(kind)?;
    let category = palette.categories.get(tab)?;
    category
        .pages
        .iter()
        .flat_map(|&p| palette.pages[p].items.iter())
        .find(|i| data.parts.get(&i.part).is_some_and(|x| x.starts_unlocked))
        .map(|i| i.part)
}

/// A scripted design for the captures and the render check: a body on the
/// studio base, a cockpit (a roof for a building) on top, the class's own
/// parts on the sides (wheels, doors, windows) and a detail. Every part is
/// dropped with a ray the way the pointer drops it, so the pinning, the
/// orientation and the symmetry twin are the editor's own.
pub fn demo_script(data: &CreatureEditorData, kind: SetupKind) -> Vec<crate::rigid_editor_edit::Step> {
    use crate::rigid_editor_edit::Step;
    let down = Vec3::NEG_Y;
    let mut steps = vec![];
    let mut add = |tab: usize, origin: Vec3, dir: Vec3| {
        if let Some(part) = first_item(data, kind, tab) {
            steps.push(Step::Buy(part));
            steps.push(Step::Place { origin, dir });
        }
    };
    if kind.is_building() {
        // Bodies, Roofs, Connectors, Doors, Windows, <class>, Details.
        add(0, Vec3::new(0.0, 40.0, 0.0), down);
        add(1, Vec3::new(0.0, 40.0, 0.0), down);
        // A connector dropped a little off the roof's centre line: the centre
        // snap (`rigid_editor_stack`) pulls it onto it and it stands upright.
        add(2, Vec3::new(0.35, 40.0, 0.25), down);
        add(3, Vec3::new(0.2, 2.0, 40.0), Vec3::NEG_Z);
        add(4, Vec3::new(40.0, 2.6, 0.4), Vec3::NEG_X);
        add(5, Vec3::new(-40.0, 2.0, -0.4), Vec3::X);
    } else if kind == SetupKind::Ufo {
        // Bodies, Cockpit, Space, Land, Water, Air, Weapons, Effect, Detail.
        add(0, Vec3::new(0.0, 20.0, 0.0), down);
        add(1, Vec3::new(0.0, 20.0, 0.1), down);
        add(5, Vec3::new(20.0, 0.5, 0.2), Vec3::NEG_X);
        add(6, Vec3::new(20.0, 0.5, 1.0), Vec3::NEG_X);
        add(8, Vec3::new(0.0, 20.0, -1.0), down);
    } else {
        // Bodies, Cockpit, <domain>, <class>, Effect, Detail.
        add(0, Vec3::new(0.0, 20.0, 0.0), down);
        add(1, Vec3::new(0.0, 20.0, 0.1), down);
        add(2, Vec3::new(20.0, 0.4, 0.6), Vec3::NEG_X);
        add(3, Vec3::new(0.0, 20.0, -0.9), down);
        add(5, Vec3::new(20.0, 0.5, -0.6), Vec3::NEG_X);
    }
    steps
}

/// The `nth` unlocked palette item whose part is of one of the rigblock types `wanted` (the instance names of
/// `modelRigBlockType`: `wheel`, `door`, `stair` ...), in palette order.
fn item_of_type(data: &CreatureEditorData, originals: &crate::Originals, kind: SetupKind, wanted: &[&str], nth: usize) -> Option<u32> {
    let palette = data.parts_palette(kind)?;
    let group = kind.part_group();
    let wanted: Vec<u32> = wanted.iter().map(|n| crate::world_objects::name_id(n)).collect();
    palette
        .categories
        .iter()
        .flat_map(|c| c.pages.iter())
        .flat_map(|&p| palette.pages[p].items.iter())
        .filter(|i| data.parts.get(&i.part).is_some_and(|x| x.starts_unlocked))
        .filter(|i| {
            originals
                .part_catalog
                .by_definition_in(group, i.part)
                .is_some_and(|d| wanted.contains(&crate::rigid_editor_stack::StackRules::load(&originals.resource_index, d.key).block_type.instance))
        })
        .map(|i| i.part)
        .nth(nth)
}

/// A second scripted design for the captures that show the parts the first one hides (`--rigid-detail`): a
/// building with a door, the stair below it and two windows a little off each other's height on the front wall
/// (the lateral / height align rules pull the stair onto the door's centre line and the windows to one height);
/// a vehicle with its wheels, wings or paddles on the sides. Every part is dropped by ray like the pointer does.
pub fn detail_script(data: &CreatureEditorData, originals: &crate::Originals, kind: SetupKind) -> Vec<crate::rigid_editor_edit::Step> {
    use crate::rigid_editor_edit::Step;
    let down = Vec3::NEG_Y;
    let mut steps = vec![];
    let mut add = |part: Option<u32>, origin: Vec3, dir: Vec3| {
        if let Some(part) = part {
            steps.push(Step::Buy(part));
            steps.push(Step::Place { origin, dir });
        }
    };
    let by_type = |names: &[&str], nth: usize| item_of_type(data, originals, kind, names, nth);
    if kind.is_building() {
        add(first_item(data, kind, 0), Vec3::new(0.0, 40.0, 0.0), down);
        add(first_item(data, kind, 1), Vec3::new(0.0, 40.0, 0.0), down);
        // The door on the +Z wall near the ground, the stair a little off its centre line below it.
        add(by_type(&["door"], 0), Vec3::new(0.0, 1.2, 40.0), Vec3::NEG_Z);
        add(by_type(&["stair"], 0), Vec3::new(0.18, 0.3, 40.0), Vec3::NEG_Z);
        // Two windows at slightly different heights either side of the door.
        add(by_type(&["window"], 0), Vec3::new(-1.8, 2.6, 40.0), Vec3::NEG_Z);
        add(by_type(&["window"], 0), Vec3::new(1.8, 2.72, 40.0), Vec3::NEG_Z);
    } else {
        add(first_item(data, kind, 0), Vec3::new(0.0, 20.0, 0.0), down);
        add(first_item(data, kind, 1), Vec3::new(0.0, 20.0, 0.1), down);
        // Wheels (or, for the other classes, wings / paddles / skis) low on the +X side, front and back.
        let mounted = by_type(&["wheel", "wing", "paddle", "ski", "propeller"], 0);
        add(mounted, Vec3::new(20.0, 0.5, 0.9), Vec3::NEG_X);
        add(mounted, Vec3::new(20.0, 0.5, -0.9), Vec3::NEG_X);
        add(by_type(&["weapon"], 0), Vec3::new(0.0, 20.0, -0.9), down);
    }
    // No selection: the captures show the parts, not the handles of the last one.
    steps.push(Step::Select(None));
    steps
}

/// Queue the demo design in the open session (`--rigid-demo`). `--rigid-detail` queues the second design
/// ([`detail_script`]); `--rigid-view <heading> [pitch [zoom factor]]` turns the editor camera by that many degrees (sets
/// the pitch, scales the zoom) so a capture shows the other side of the design.
pub(crate) fn capture_demo(world: &mut World) {
    let kind = world.resource::<RigidEditor>().kind;
    let args: Vec<String> = std::env::args().collect();
    let steps = if args.iter().any(|a| a == "--rigid-detail") {
        detail_script(world.resource::<CreatureEditorData>(), world.resource::<crate::Originals>(), kind)
    } else {
        demo_script(world.resource::<CreatureEditorData>(), kind)
    };
    world.resource_mut::<RigidEditor>().script.extend(steps);
    if let Some(i) = args.iter().position(|a| a == "--rigid-view") {
        let number = |k: usize| args.get(i + k).and_then(|v| v.parse::<f32>().ok());
        let mut editor = world.resource_mut::<CreatureEditor>();
        if let Some(heading) = number(1) {
            editor.rig.heading += heading.to_radians();
        }
        if let Some(pitch) = number(2) {
            editor.rig.pitch = pitch.to_radians().clamp(editor.rig.props.min_pitch.to_radians(), editor.rig.props.max_pitch.to_radians());
        }
        if let Some(factor) = number(3) {
            let props = editor.rig.props;
            editor.rig.zoom = (editor.rig.zoom * factor).clamp(props.min_zoom, props.max_zoom.max(props.min_zoom));
        }
        editor.rig.moved = true;
    }
}

fn kind_of_session(app: &App) -> SetupKind {
    app.world().resource::<RigidEditor>().kind
}

/// Where block `index` is on the screen (the centre of its drawn surfaces).
fn block_screen_centre(app: &mut App, index: usize) -> Option<Vec2> {
    let world = app.world_mut();
    let kids: Vec<Entity> = {
        let mut q = world.query::<(&RigidBlockVisual, &Children)>();
        q.iter(world).find(|(b, _)| b.index == index)?.1.iter().collect()
    };
    let centres: Vec<Vec3> = kids
        .iter()
        .filter_map(|k| {
            let gt = world.get::<GlobalTransform>(*k)?;
            let aabb = world.get::<bevy::camera::primitives::Aabb>(*k)?;
            Some(gt.transform_point(Vec3::from(aabb.center)))
        })
        .collect();
    if centres.is_empty() {
        return None;
    }
    let centre = centres.iter().sum::<Vec3>() / centres.len() as f32;
    let mut cameras = world.query_filtered::<(&Camera, &GlobalTransform), With<crate::GameCamera>>();
    let (camera, transform) = cameras.single(world).ok()?;
    camera.world_to_viewport(transform, centre).ok()
}

/// Where a handle of the selected block is on the screen.
fn handle_screen(app: &mut App, kind: crate::rigid_editor_edit::HandleKind) -> Option<Vec2> {
    let entity = app.world().resource::<RigidEditor>().handle_entities.iter().find(|(_, k)| *k == kind)?.0;
    let world = app.world_mut();
    let at = world.get::<GlobalTransform>(entity)?.translation();
    let mut cameras = world.query_filtered::<(&Camera, &GlobalTransform), With<crate::GameCamera>>();
    let (camera, transform) = cameras.single(world).ok()?;
    camera.world_to_viewport(transform, at).ok()
}

/// Drive the stage's rigid editor through its real chrome in the GPU render
/// test (`runtime::render_original_install`): F3 opens it, a scripted design
/// is built, captured with the handles of a selected block, undone, redone,
/// sold, rebuilt and accepted: the record is stored and the stage shows it.
pub(crate) fn render_smoke_checks(app: &mut App, window: Entity, step: &dyn Fn(&mut App), capture: &dyn Fn(&mut App, &str)) {
    let stage = app.world().resource::<State<GameStage>>().get().clone();
    // Civ: the vehicle editor and the building editors; Space: the UFO.
    let modes: &[crate::civ::CivEditorMode] = if stage == GameStage::Civilization {
        &[crate::civ::CivEditorMode::Vehicle, crate::civ::CivEditorMode::Building]
    } else {
        &[crate::civ::CivEditorMode::Vehicle]
    };
    for mode in modes {
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.editor_mode = *mode;
        check_one(app, window, step, capture);
    }
}

/// A camera close to what the stage shows of the design: the capital's hall,
/// one of your land vehicles, the ship.
fn stage_close_up(app: &mut App, kind: SetupKind) -> Option<Transform> {
    let world = app.world_mut();
    let (anchor, look, up) = if kind.is_building() {
        let mut q = world.query::<(&crate::civ::CityMarker, &GlobalTransform)>();
        let (_, gt) = q.iter(world).find(|(m, _)| m.0 == crate::civ::HOME_INDEX)?;
        (gt.transform_point(Vec3::new(3.2, 1.6, 6.5)), gt.transform_point(Vec3::new(0.0, -0.4, 0.0)), gt.up().as_vec3())
    } else if kind == SetupKind::Ufo {
        let mut q = world.query_filtered::<&GlobalTransform, With<Player>>();
        let gt = q.single(world).ok()?;
        // The design is the whole 2.4-wide ship: a wider view, the ship off
        // the centre (the stage's tutorial card covers the middle).
        (gt.transform_point(Vec3::new(3.6, 2.4, 4.8)), gt.transform_point(Vec3::new(-2.2, -1.8, 0.0)), gt.up().as_vec3())
    } else {
        let mut q = world.query_filtered::<(&crate::civ_rts::CivUnit, &GlobalTransform), With<crate::game::Vehicle>>();
        let (_, gt) = q.iter(world).find(|(u, _)| u.nation == crate::civ::HOME_INDEX && u.domain == crate::civ::VehicleDomain::Land)?;
        (gt.transform_point(Vec3::new(1.8, 1.0, 2.6)), gt.transform_point(Vec3::new(0.0, 0.25, 0.0)), gt.up().as_vec3())
    };
    Some(Transform::from_translation(anchor).looking_at(look, up))
}

fn check_one(app: &mut App, window: Entity, step: &dyn Fn(&mut App), capture: &dyn Fn(&mut App, &str)) {
    use bevy::input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    };
    let press = |app: &mut App, keys: &[KeyCode]| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            for k in keys {
                app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
                    key_code: *k,
                    logical_key: Key::Character("".into()),
                    state,
                    text: None,
                    repeat: false,
                    window,
                });
            }
            step(app);
        }
        step(app);
    };
    let stage = app.world().resource::<State<GameStage>>().get().clone();
    let base = app.world().resource::<RunProgress>().parts.clone();
    let dna = app.world().resource::<RunProgress>().editor_dna;
    press(app, &[KeyCode::F3]);
    for _ in 0..40 {
        step(app);
    }
    assert!(app.world().resource::<RigidEditor>().active, "{stage:?}: F3 opens the rigid editor");
    let kind = app.world().resource::<RigidEditor>().kind;
    assert!(kind.is_rigid(), "{stage:?}: {kind:?}");
    assert!(app.world().resource::<CreatureEditor>().active, "the editor session runs");
    assert_eq!(app.world().resource::<CreatureEditor>().kind, kind);
    assert!(!app.world().resource::<crate::outfitter::Outfitter>().active);
    assert!(!app.world().resource::<crate::mechanical_creator::MechanicalCreator>().open);
    let data = app.world().resource::<CreatureEditorData>().clone();
    assert!(data.loaded, "creature editor data");
    let palette = data.parts_palette(kind).cloned().expect("the class palette");
    assert!(palette.categories.len() >= 6, "{kind:?}: {} tabs", palette.categories.len());
    {
        let world = app.world_mut();
        let roots = world.query::<&crate::creature_editor_ui::EditorRoot>().iter(world).count();
        assert!(roots >= 6, "the retail editor layouts are spawned ({roots})");
        let mut previews = world.query_filtered::<Entity, With<RigidPreview>>();
        assert_eq!(previews.iter(world).count(), 1, "one preview root");
        let mut players = world.query_filtered::<Entity, With<Player>>();
        assert_eq!(players.iter(world).count(), 1, "the preview is the only player");
    }
    let initial = app.world().resource::<RunProgress>().editor_dna;
    let setup_budget = data.setup(kind).and_then(|s| s.initial_budget).unwrap_or(0) as f32;
    assert!(app.world().resource::<RunProgress>().parts.is_empty() || initial < setup_budget);
    // The scripted design.
    let script = demo_script(&data, kind);
    assert!(script.len() >= 6, "{kind:?}: the demo finds parts ({} steps)", script.len());
    let steps = script.len();
    app.world_mut().resource_mut::<RigidEditor>().script.extend(script);
    for _ in 0..(steps * 3 + 30) {
        step(app);
    }
    let parts = app.world().resource::<RunProgress>().parts.clone();
    assert!(parts.len() >= steps / 2, "{kind:?}: {} blocks of {} steps", parts.len(), steps / 2);
    assert!(parts.iter().all(|p| p.rigid.is_some()), "every block has a free transform");
    let spent = initial - app.world().resource::<RunProgress>().editor_dna;
    assert!(spent > 0.0, "the budget pays for the parts");
    if data.setup(kind).is_some_and(|s| s.use_symmetry) {
        assert!(parts.iter().any(|p| p.pair_id != 0 && p.mirrored), "{kind:?}: an off-centre block has its mirrored twin");
    }
    let complexity = data.complexity(kind, &parts, 0);
    assert!(!complexity.over(), "{kind:?}: complexity {complexity:?}");
    // Select a block (its handles show) and look at it.
    app.world_mut().resource_mut::<RigidEditor>().script.push_back(crate::rigid_editor_edit::Step::Select(Some(parts.len() - 1)));
    for _ in 0..30 {
        step(app);
    }
    assert!(!app.world_mut().query::<&RigidHandle>().iter(app.world()).collect::<Vec<_>>().is_empty(), "the selected block shows its handles");
    capture(app, &format!("{}-{}-rigid-editor.png", format!("{stage:?}").to_lowercase(), format!("{kind:?}").to_lowercase()));
    // The pointer: drag a block along the model, scale it with the wheel,
    // stretch it with its handle, carry a palette purchase onto the model.
    {
        use bevy::input::{
            mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
            ButtonState,
        };
        let click = |app: &mut App, state: ButtonState| {
            app.world_mut().resource_mut::<Messages<MouseButtonInput>>().write(MouseButtonInput {
                button: MouseButton::Left,
                state,
                window,
            });
            step(app);
        };
        let aim = |app: &mut App, at: Vec2| {
            app.world_mut().resource_mut::<crate::spui::view::SpuiPointer>().position_override = Some(at);
            app.world_mut().entity_mut(window).get_mut::<Window>().unwrap().set_cursor_position(Some(at));
            step(app);
            step(app);
        };
        let parts_now = |app: &App| app.world().resource::<RunProgress>().parts.clone();
        // Drag block 1 (above the body) sideways on the screen.
        let target = 1.min(parts.len() - 1);
        app.world_mut().resource_mut::<RigidEditor>().script.push_back(crate::rigid_editor_edit::Step::Select(None));
        for _ in 0..6 {
            step(app);
        }
        let from = block_screen_centre(app, target).expect("the block is on the screen");
        let undo_before = parts_now(app);
        aim(app, from);
        click(app, ButtonState::Pressed);
        assert!(app.world().resource::<RigidEditor>().grab.is_some(), "a press on a block grabs it ({from:?})");
        assert_eq!(app.world().resource::<RigidEditor>().selected, Some(target));
        aim(app, from + Vec2::new(-90.0, 6.0));
        aim(app, from + Vec2::new(-140.0, 10.0));
        click(app, ButtonState::Released);
        step(app);
        let moved = parts_now(app);
        assert_ne!(moved, undo_before, "dragging moves the block over the surfaces");
        assert!(app.world().resource::<RigidEditor>().grab.is_none());
        press(app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
        assert_eq!(parts_now(app), undo_before, "one drag is one undo entry");
        press(app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
        assert_eq!(parts_now(app), moved);
        // The wheel over the block scales it.
        let at = block_screen_centre(app, target).expect("the block is still on the screen");
        aim(app, at);
        let scale_before = parts_now(app)[target].scale;
        app.world_mut().resource_mut::<Messages<MouseWheel>>().write(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: 3.0,
            phase: bevy::input::touch::TouchPhase::Moved,
            window,
        });
        for _ in 0..4 {
            step(app);
        }
        assert!(parts_now(app)[target].scale > scale_before, "the wheel over a block scales it");
        for _ in 0..60 {
            step(app);
        }
        // Stretch handle: press on the +x deform sphere and pull.
        app.world_mut().resource_mut::<RigidEditor>().script.push_back(crate::rigid_editor_edit::Step::Select(Some(target)));
        for _ in 0..8 {
            step(app);
        }
        let kind = crate::rigid_editor_edit::HandleKind::Stretch(1, 1);
        if let Some(sphere) = handle_screen(app, kind) {
            let before = parts_now(app)[target].rigid.clone().unwrap();
            aim(app, sphere);
            click(app, ButtonState::Pressed);
            assert!(
                matches!(app.world().resource::<RigidEditor>().grab, Some(crate::rigid_editor_edit::Grab::Stretch { .. })),
                "a press on a deform handle grabs it"
            );
            aim(app, sphere + Vec2::new(0.0, -50.0));
            click(app, ButtonState::Released);
            step(app);
            let after = parts_now(app)[target].rigid.clone().unwrap();
            assert_ne!(after.stretch, before.stretch, "pulling the handle stretches the block");
        } else {
            println!("TODO rigid editor: the deform handle is off the screen; the handle drag was skipped");
        }
        // A palette purchase carried by the held press rests on the model.
        let count = parts_now(app).len();
        let first = demo_script(&data, kind_of_session(app)).iter().find_map(|s| match s {
            crate::rigid_editor_edit::Step::Buy(p) => Some(*p),
            _ => None,
        });
        if let Some(part) = first {
            let body = block_screen_centre(app, 0).expect("the body is on the screen");
            // The press starts on the palette (the left panel), not on the model.
            aim(app, Vec2::new(80.0, 300.0));
            click(app, ButtonState::Pressed);
            app.world_mut().resource_mut::<EditorState>().queued.push(crate::editor::Action::Buy(part));
            step(app);
            step(app);
            assert!(matches!(app.world().resource::<RigidEditor>().grab, Some(crate::rigid_editor_edit::Grab::Move { carried: true, .. })), "the held press carries the purchase");
            aim(app, body + Vec2::new(10.0, -20.0));
            aim(app, body + Vec2::new(14.0, -26.0));
            click(app, ButtonState::Released);
            step(app);
            assert!(parts_now(app).len() > count, "the carried purchase stays where it was dropped");
        }
        app.world_mut().resource_mut::<crate::spui::view::SpuiPointer>().position_override = None;
    }
    // Undo and redo.
    let built = app.world().resource::<RunProgress>().parts.clone();
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert_ne!(app.world().resource::<RunProgress>().parts, built, "undo");
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
    assert_eq!(app.world().resource::<RunProgress>().parts, built, "redo");
    // Turn, scale and sell the selected block.
    {
        use crate::rigid_editor_edit::Step;
        let mut rigid = app.world_mut().resource_mut::<RigidEditor>();
        rigid.script.extend([Step::Select(Some(0)), Step::Scale(2.0), Step::Turn(0.5), Step::Stretch(0, 1.2)]);
    }
    for _ in 0..12 {
        step(app);
    }
    let edited = app.world().resource::<RunProgress>().parts[0].clone();
    assert!(edited.scale > built[0].scale, "the wheel scales the block");
    assert_ne!(edited.rigid, built[0].rigid, "turn and stretch change the free transform");
    let before_sale = app.world().resource::<RunProgress>().editor_dna;
    app.world_mut().resource_mut::<RunProgress>().focused_part = (built.len() - 1) as u32;
    press(app, &[KeyCode::Delete]);
    assert!(app.world().resource::<RunProgress>().parts.len() < built.len(), "Delete sells the selected block");
    assert!(app.world().resource::<RunProgress>().editor_dna > before_sale, "the refund");
    // Paint mode (`rigid_paint`): a complete style on every block, the
    // paintbrush on one region of the first block.
    crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Paint);
    for _ in 0..10 {
        step(app);
    }
    assert_eq!(app.world().resource::<crate::creature_editor::CreatureEditor>().mode, crate::creature_editor::Mode::Paint, "{kind:?}: Paint mode opens");
    let theme = crate::rigid_paint::demo_theme(app.world_mut(), 1).expect("a complete style");
    let brushed = crate::rigid_paint::demo_brush(app.world_mut(), [0.85, 0.12, 0.1]).expect("a paintbrush paint");
    for _ in 0..60 {
        step(app);
    }
    {
        let run = app.world().resource::<RunProgress>();
        assert!(run.parts.iter().all(|p| p.rigid.as_ref().is_some_and(|b| !b.paints.is_empty())), "{kind:?}: every block takes the style");
        let first = run.parts[0].rigid.as_ref().unwrap();
        assert!(first.paints.contains(&brushed), "{kind:?}: the brushed region");
    }
    let (painted, total) = crate::rigid_paint::painted_surfaces(app.world_mut());
    assert!(total > 0 && painted * 4 >= total * 3, "{kind:?}: painted surfaces {painted} of {total}");
    assert!(app.world_mut().query::<&crate::rigid_paint::RigidPaintRoot>().iter(app.world()).count() >= 3, "{kind:?}: the paint palette shows");
    capture(app, &format!("{}-{}-rigid-paint.png", format!("{stage:?}").to_lowercase(), format!("{kind:?}").to_lowercase()));
    crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Build);
    for _ in 0..4 {
        step(app);
    }
    println!("PASS {kind:?} rigid paint: style {theme:08X} on every block, brush {brushed:?}, {painted} painted surfaces");
    // The stats trays (vehicles: health, power, speed from the parts).
    let trays = {
        let data = app.world().resource::<CreatureEditorData>();
        crate::rigid_stats::tray_levels(data, kind, app.world().resource::<RunProgress>())
    };
    let tray_roots = app
        .world_mut()
        .query::<&crate::creature_editor_ui::EditorRoot>()
        .iter(app.world())
        .filter(|r| matches!(r, crate::creature_editor_ui::EditorRoot::Tray(_)))
        .count();
    if kind.is_vehicle() {
        assert_eq!(trays.iter().filter(|t| !t.is_empty()).count(), 3, "{kind:?}: three stats trays {trays:?}");
        assert!(tray_roots >= 3, "{kind:?}: the trays are drawn ({tray_roots})");
        assert!(trays.iter().flatten().any(|(_, l)| *l > 0), "{kind:?}: the parts give stats {trays:?}");
    } else {
        assert!(trays.is_empty(), "{kind:?}: no trays");
    }
    // Test Drive (vehicles): the vehicle drives at its speed, Build puts it back.
    if kind.is_vehicle() {
        let start = {
            let world = app.world_mut();
            world.query_filtered::<&Transform, With<RigidHolder>>().single(world).map(|t| *t).expect("the blocks' holder")
        };
        app.world_mut().resource_mut::<crate::rigid_stats::TestDrive>().auto = true;
        crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Play);
        for _ in 0..60 {
            step(app);
        }
        let (driven, top) = {
            let d = app.world().resource::<crate::rigid_stats::TestDrive>();
            (d.distance, d.top_speed)
        };
        let moved = {
            let world = app.world_mut();
            world.query_filtered::<&Transform, With<RigidHolder>>().single(world).map(|t| t.translation.distance(start.translation)).unwrap_or(0.0)
        };
        assert!(top > 0.0 && driven > 1.0 && moved > 1.0, "{kind:?}: Test Drive moves the vehicle (driven {driven}, moved {moved}, top {top})");
        capture(app, &format!("{}-{}-rigid-drive.png", format!("{stage:?}").to_lowercase(), format!("{kind:?}").to_lowercase()));
        crate::rigid_paint::enter_mode(app.world_mut(), crate::creature_editor::Mode::Build);
        app.world_mut().resource_mut::<crate::rigid_stats::TestDrive>().auto = false;
        for _ in 0..4 {
            step(app);
        }
        let back = {
            let world = app.world_mut();
            world.query_filtered::<&Transform, With<RigidHolder>>().single(world).map(|t| *t).unwrap()
        };
        assert!(back.translation.distance(start.translation) < 1e-3, "{kind:?}: Build puts the vehicle back");
        println!("PASS {kind:?} rigid stats and Test Drive: trays {trays:?}, drove {driven:.1} m at top speed {top:.1} m/s");
    }
    // Accept.
    press(app, &[KeyCode::Enter]);
    for _ in 0..20 {
        step(app);
    }
    assert!(!app.world().resource::<RigidEditor>().active, "Enter closes the editor");
    let run = app.world().resource::<RunProgress>();
    assert_eq!(run.parts, base, "the creature's parts are restored");
    assert_eq!(run.editor_dna, dna);
    assert!(run.rigid.session.is_none());
    let design = run.rigid.design(kind).expect("the design is stored");
    assert!(design.parts.len() >= 3 && design.parts.iter().all(|p| p.block.is_some()));
    assert!(design.parts.iter().all(|p| p.block.as_ref().is_some_and(|b| !b.paints.is_empty())), "{kind:?}: the design keeps its paint");
    let stored = design.parts.len();
    // The stage shows the design.
    for _ in 0..30 {
        step(app);
    }
    let world = app.world_mut();
    if stage == GameStage::Space {
        let modules = world.query::<&crate::space::ShipModuleRef>().iter(world).count();
        assert_eq!(modules, stored, "the ship wears the design's blocks");
        assert_eq!(world.resource::<RunProgress>().ship.modules.len(), stored);
        // The design is the whole ship: the old hull (`genufo` preset) is hidden.
        let mut ships = world.query_filtered::<Entity, (With<Player>, With<crate::space::ShipMotion>)>();
        let ship = ships.single(world).expect("the player's ship");
        let mut hulls = world.query_filtered::<(&ChildOf, Option<&Visibility>), With<crate::creation_assets::OriginalCreation>>();
        let hull: Vec<_> = hulls.iter(world).filter(|(p, _)| p.parent() == ship).map(|(_, v)| v.copied()).collect();
        assert!(hull.iter().all(|v| *v == Some(Visibility::Hidden)), "the old hull gives way to the design ({hull:?})");
        let fit = crate::rigid_editor::ship_design_fit(
            world.resource::<Assets<Mesh>>(),
            world.resource::<Originals>(),
            &world.resource::<RunProgress>().ship.modules,
        )
        .expect("the design's fit");
        println!("PASS Ufo design drawn as the whole ship: {modules} blocks, scale {:.3}, old hull hidden ({} roots)", fit.0, hull.len());
    }
    if kind.is_vehicle() && stage == GameStage::Civilization {
        let world = app.world_mut();
        let mounted = world.query::<&crate::civ::VehiclePartRef>().iter(world).count();
        assert!(mounted >= stored, "{kind:?}: your land vehicles wear the design ({mounted} blocks of {stored})");
        let mut art_hidden = 0;
        let mut units = world.query_filtered::<(&crate::civ_rts::CivUnit, &crate::civ_rts::UnitRig), With<crate::game::Vehicle>>();
        let rigs: Vec<(bool, Entity)> = units
            .iter(world)
            .filter(|(u, _)| u.nation == crate::civ::HOME_INDEX)
            .map(|(u, r)| (u.domain == crate::civ::VehicleDomain::Land, r.art))
            .collect();
        for (land, art) in rigs {
            if land && world.get::<Visibility>(art) == Some(&Visibility::Hidden) {
                art_hidden += 1;
            }
        }
        assert!(art_hidden >= 1, "{kind:?}: the preset art of the designed class is hidden");
    }
    if kind.is_building() && kind == SetupKind::BuildingCityHall {
        let world = app.world_mut();
        let mounted = world.query::<&crate::civ::CityPartRef>().iter(world).count();
        assert!(mounted >= stored, "the city hall design stands in your city ({mounted} of {stored})");
        // The native hall of your city gives way to it.
        let world = app.world_mut();
        let mut halls = world.query_filtered::<&Visibility, With<crate::civ::NativeCityHallSurface>>();
        let total = halls.iter(world).count();
        let hidden = halls.iter(world).filter(|v| **v == Visibility::Hidden).count();
        assert!(total == 0 || (hidden >= 1 && hidden < total), "your city's native hall is hidden, the rivals' are not ({hidden} of {total})");
    }
    // A designed house stands on a ring plot instead of the native one.
    if kind == SetupKind::BuildingCityHall {
        let design = app.world().resource::<RunProgress>().rigid.design(kind).cloned().expect("the stored design");
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.rigid.designs.insert(SetupKind::BuildingHouse.setup_name().into(), design);
            let city = &mut run.civ_campaign.cities[crate::civ::HOME_INDEX];
            city.ensure_layout();
            city.plots[0] = Some(crate::civ::Building::House);
        }
        for _ in 0..30 {
            step(app);
        }
        let world = app.world_mut();
        let mut plots = world.query::<(&crate::city_presentation::CityPlotVisual, &Children)>();
        let drawn: Vec<usize> = plots
            .iter(world)
            .filter(|(p, _)| p.kind == crate::civ::Building::House && p.design != 0)
            .map(|(_, kids)| kids.len())
            .collect();
        assert!(!drawn.is_empty() && drawn[0] >= 1, "a designed house is drawn on its plot ({drawn:?})");
    }
    // A close-up of what the stage shows.
    if let Some(close) = stage_close_up(app, kind) {
        let previous = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<(Entity, &Transform), With<crate::GameCamera>>();
            q.single(world).ok().map(|(e, t)| (e, *t))
        };
        app.world_mut().insert_resource(crate::runtime::RenderSmokeCamera(close));
        for _ in 0..12 {
            step(app);
        }
        capture(app, &format!("{}-{}-rigid-stage.png", format!("{stage:?}").to_lowercase(), format!("{kind:?}").to_lowercase()));
        app.world_mut().remove_resource::<crate::runtime::RenderSmokeCamera>();
        if let Some((camera, t)) = previous {
            if let Some(mut tr) = app.world_mut().get_mut::<Transform>(camera) {
                *tr = t;
            }
        }
    }
    println!("PASS {kind:?} rigid editor: retail chrome, scripted pinning/symmetry/handles, undo/redo, sell, accept stores {stored} blocks and the stage shows them");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_mirrors_position_and_turns_the_other_way() {
        let mut b = RigidBlock::at(Vec3::new(0.7, 0.4, -1.2));
        b.set_rotation(Quat::from_rotation_y(0.5) * Quat::from_rotation_x(0.2));
        let r = b.reflected();
        assert_eq!(r.position(), Vec3::new(-0.7, 0.4, -1.2));
        assert!(r.mirrored && !b.mirrored);
        // Reflecting twice is the identity.
        let rr = r.reflected();
        assert!(rr.position().distance(b.position()) < 1e-6);
        assert!(rr.rotation().angle_between(b.rotation()) < 1e-5);
        assert!(!rr.mirrored);
        // The twin's mesh (x flipped, then posed) is the mirror image of the
        // block's mesh (posed): check a point.
        let t = b.transform(1.0);
        let tr = r.transform(1.0);
        let v = Vec3::new(0.3, 0.2, 0.9);
        let a = t.transform_point(v);
        let c = tr.transform_point(v);
        assert!(c.distance(Vec3::new(-a.x, a.y, a.z)) < 1e-5, "{a} {c}");
    }

    #[test]
    fn block_transform_scales_the_design_about_its_origin() {
        let b = RigidBlock { position: [1.0, 2.0, 3.0], stretch: [2.0, 1.0, 1.0], ..default() };
        let t = block_transform(&b, 0.5, 0.4, Vec3::new(0.0, 0.7, 0.0));
        assert!((t.translation - Vec3::new(0.4, 1.5, 1.2)).length() < 1e-6);
        assert!((t.scale - Vec3::new(0.4, 0.2, 0.2)).length() < 1e-6);
    }

    #[test]
    fn records_round_trip_and_old_saves_load() {
        let mut p = PartInstance::new(7, 0).with_definition(9);
        p.scale = 1.5;
        p.pair_id = 3;
        p.mirrored = true;
        p.rigid = Some(RigidBlock::at(Vec3::new(1.0, 2.0, 3.0)));
        let v = to_record(&p);
        assert_eq!(v.block.as_ref().unwrap().pair, 3);
        let b = v.block.clone().unwrap();
        let back = from_record(&v, &b);
        assert_eq!(back.pair_id, 3);
        assert!(back.mirrored);
        assert_eq!(back.scale, 1.5);
        assert_eq!(back.definition_id, Some(9));
        assert_eq!(back.rigid.as_ref().unwrap().position, [1.0, 2.0, 3.0]);
        // The stored parent link round trips; a block saved before it (or a parentless one) writes neither field.
        let linked = RigidBlock { id: 4, parent: 2, ..default() };
        let text = serde_json::to_string(&linked).unwrap();
        assert!(text.contains("\"id\":4") && text.contains("\"parent\":2"), "{text}");
        assert_eq!(serde_json::from_str::<RigidBlock>(&text).unwrap(), linked);
        let old_block: RigidBlock = serde_json::from_str(r#"{"position":[1.0,2.0,3.0]}"#).unwrap();
        assert_eq!((old_block.id, old_block.parent), (0, 0), "an old design has no ids: the session numbers it");
        assert!(!serde_json::to_string(&old_block).unwrap().contains("parent"));
        // A slot-based record (no block) deserializes as before.
        let old = r#"{"instance":5,"slot":2,"yaw":0.5,"scale":1.0,"offset":[0.0,0.0,0.0]}"#;
        let part: VehiclePart = serde_json::from_str(old).unwrap();
        assert!(part.block.is_none());
        let text = serde_json::to_string(&part).unwrap();
        assert!(!text.contains("block"), "no new field is written for a slot record: {text}");
        // A run without the new record loads.
        let json = serde_json::to_string(&RunProgress::default()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("rigid");
        let run: RunProgress = serde_json::from_value(value).unwrap();
        assert_eq!(run.rigid, RigidRecords::default());
    }

    #[test]
    fn legacy_slot_poses_convert_to_editor_units() {
        let b = legacy_block(Vec3::new(0.0, 0.0, 1.5), std::f32::consts::FRAC_PI_2, 0.5);
        assert!((b.position() - Vec3::new(0.0, 0.0, 3.0)).length() < 1e-6);
        let front = b.rotation() * Vec3::Z;
        assert!(front.distance(Vec3::X) < 1e-5);
    }

    use std::sync::Mutex;

    /// The app tests share the process's GPU-less app setup: one at a time.
    static APP: Mutex<()> = Mutex::new(());

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }

    fn settle(app: &mut App) {
        for _ in 0..4 {
            app.update();
        }
    }

    fn open_in(stage: GameStage) -> App {
        let mut run = RunProgress::default();
        run.parts = vec![PartInstance::new(1, 0)];
        run.editor_dna = 77.0;
        run.creature_creation.name = "Zork".into();
        let mut app = crate::runtime::headless_app(run);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(stage);
        app.update();
        app.update();
        app
    }

    fn block_part(instance: u32, at: Vec3) -> PartInstance {
        let mut p = PartInstance::new(instance, 0).with_definition(instance);
        p.rigid = Some(RigidBlock::at(at));
        p
    }

    fn players(app: &mut App) -> (usize, usize) {
        let world = app.world_mut();
        let all = world.query_filtered::<Entity, With<Player>>().iter(world).count();
        let previews = world.query_filtered::<Entity, With<RigidPreview>>().iter(world).count();
        (all, previews)
    }

    fn close(app: &mut App, cancel: bool) {
        if cancel {
            app.world_mut().resource_mut::<CreatureEditor>().cancelled = true;
        }
        app.world_mut().resource_mut::<EditorState>().open = false;
        settle(app);
    }

    #[test]
    fn f3_opens_the_rigid_editor_and_accept_stores_the_design() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        for (stage, class) in [(GameStage::Civilization, SetupKind::VehicleMilitaryLand), (GameStage::Space, SetupKind::Ufo)] {
            let mut app = open_in(stage.clone());
            let before = players(&mut app);
            tap(&mut app, KeyCode::F3);
            settle(&mut app);
            assert!(app.world().resource::<RigidEditor>().active, "{stage:?}");
            assert_eq!(app.world().resource::<RigidEditor>().kind, class);
            {
                let state = app.world().resource::<EditorState>();
                assert!(state.open && state.rigid);
                let editor = app.world().resource::<CreatureEditor>();
                assert!(editor.active, "the shared editor session runs");
                assert_eq!(editor.kind, class, "{stage:?}: the class's setup");
                let run = app.world().resource::<RunProgress>();
                assert!(run.parts.is_empty(), "the creature's parts are put away");
                assert_eq!(run.creature_creation.name, "", "the design has its own name");
                assert!(run.rigid.session.is_some());
            }
            assert_eq!(players(&mut app), (1, 1), "the preview is the only player");
            // The F3 key does not open the outfitter or the modal as well.
            assert!(!app.world().resource::<crate::outfitter::Outfitter>().active);
            assert!(!app.world().resource::<crate::mechanical_creator::MechanicalCreator>().open);
            // A bought block (what the palette does) and a name.
            {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                run.parts.push(block_part(7, Vec3::new(1.0, 0.5, 2.0)));
                run.creature_creation.name = "Rover".into();
            }
            close(&mut app, false);
            assert!(!app.world().resource::<RigidEditor>().active);
            assert_eq!(players(&mut app), (before.0, 0), "{stage:?}: the real player is back, no preview");
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.parts, vec![PartInstance::new(1, 0)], "the creature is restored");
            assert_eq!(run.editor_dna, 77.0);
            assert_eq!(run.creature_creation.name, "Zork");
            assert!(run.rigid.session.is_none());
            let design = run.rigid.design(class).expect("the design is stored");
            assert_eq!(design.name, "Rover");
            assert_eq!(design.parts.len(), 1);
            // The UFO's design is moved to its centre of mass (`editorMoveModelToCenterOfMass`).
            let at = if class == SetupKind::Ufo { [0.0, 0.5, 0.0] } else { [1.0, 0.5, 2.0] };
            assert_eq!(design.parts[0].block.as_ref().unwrap().position, at);
            if class == SetupKind::Ufo {
                assert_eq!(run.ship.modules.len(), 1, "the UFO's modules are the stage's record");
                assert!(run.ship.modules[0].block.is_some());
            }
        }
    }

    #[test]
    fn f3_again_closes_the_editor_and_keeps_the_design() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Space);
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        assert!(app.world().resource::<RigidEditor>().active);
        app.world_mut().resource_mut::<RunProgress>().parts.push(block_part(7, Vec3::Y));
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        assert!(!app.world().resource::<RigidEditor>().active, "F3 closes it again");
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.rigid.design(SetupKind::Ufo).unwrap().parts.len(), 1, "and keeps the design");
        assert_eq!(run.ship.modules.len(), 1);
        assert_eq!(run.parts, vec![PartInstance::new(1, 0)]);
    }

    #[test]
    fn cancel_keeps_the_record() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Civilization);
        app.world_mut().resource_mut::<RunProgress>().rigid.designs.insert(
            SetupKind::VehicleMilitaryLand.setup_name().into(),
            RigidDesign { name: "Old".into(), parts: vec![to_record(&block_part(3, Vec3::ZERO))] },
        );
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        {
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.parts.len(), 1, "the saved design opens");
            assert_eq!(run.creature_creation.name, "Old");
        }
        app.world_mut().resource_mut::<RunProgress>().parts.push(block_part(4, Vec3::X));
        close(&mut app, true);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.rigid.design(SetupKind::VehicleMilitaryLand).unwrap().parts.len(), 1);
        assert_eq!(run.rigid.design(SetupKind::VehicleMilitaryLand).unwrap().name, "Old");
        assert_eq!(run.parts, vec![PartInstance::new(1, 0)]);
        assert_eq!(run.creature_creation.name, "Zork");
    }

    #[test]
    fn brackets_switch_the_class_and_keep_the_design() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Civilization);
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.approach = crate::civ::Approach::Economic;
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        assert_eq!(app.world().resource::<RigidEditor>().kind, SetupKind::VehicleEconomicLand);
        app.world_mut().resource_mut::<RunProgress>().parts.push(block_part(9, Vec3::Y));
        tap(&mut app, KeyCode::BracketRight);
        settle(&mut app);
        assert!(app.world().resource::<RigidEditor>().active, "the editor reopens");
        assert_eq!(app.world().resource::<RigidEditor>().kind, SetupKind::VehicleEconomicWater);
        assert_eq!(app.world().resource::<CreatureEditor>().kind, SetupKind::VehicleEconomicWater);
        assert!(app.world().resource::<RunProgress>().parts.is_empty(), "the sea vehicle starts empty");
        assert_eq!(
            app.world().resource::<RunProgress>().rigid.design(SetupKind::VehicleEconomicLand).unwrap().parts.len(),
            1,
            "the land vehicle's design was kept"
        );
        // V jumps to the buildings.
        tap(&mut app, KeyCode::KeyV);
        settle(&mut app);
        assert_eq!(app.world().resource::<RigidEditor>().kind, SetupKind::BuildingCityHall);
        assert!(app.world().resource::<RigidEditor>().kind.is_building());
        close(&mut app, false);
        assert!(app.world().resource::<RunProgress>().rigid.design(SetupKind::VehicleEconomicLand).is_some());
    }

    #[test]
    fn old_slot_records_open_as_blocks_where_the_stage_showed_them() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Civilization);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.civ_campaign.approach = crate::civ::Approach::Military;
            run.vehicle.parts.push(VehiclePart::new(5, 2)); // pylon_R
            run.building.parts.push(VehiclePart::new(6, 0));
        }
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        {
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.parts.len(), 1);
            let b = run.parts[0].rigid.as_ref().expect("converted to a block");
            // `vehicle_part_transform` pylon_R (1.0, 0.1, 0.2), x 0.6 in the stage, over the stage scale.
            let want = Vec3::new(1.0, 0.1, 0.2) * 0.6 / VEHICLE_STAGE_SCALE;
            assert!(b.position().distance(want) < 1e-4, "{:?} vs {want}", b.position());
            assert_eq!(run.parts[0].instance, 5);
        }
        close(&mut app, true);
        // The old record itself is untouched.
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.vehicle.parts, vec![VehiclePart::new(5, 2)]);
        // The city hall opens the old skyline.
        let mut app = open_in(GameStage::Civilization);
        app.world_mut().resource_mut::<RunProgress>().building.parts.push(VehiclePart::new(6, 0));
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.editor_mode = crate::civ::CivEditorMode::Building;
        tap(&mut app, KeyCode::F3);
        settle(&mut app);
        assert_eq!(app.world().resource::<RigidEditor>().kind, SetupKind::BuildingCityHall);
        assert_eq!(app.world().resource::<RunProgress>().parts.len(), 1);
    }

    #[test]
    fn an_interrupted_session_is_repaired() {
        let mut run = RunProgress::default();
        run.parts = vec![block_part(1, Vec3::ZERO)];
        run.editor_dna = 12.0;
        run.creature_creation.name = "Design".into();
        run.rigid.session = Some(RigidBackup {
            parts: vec![PartInstance::new(9, 1)],
            dna: 55.0,
            name: "Creature".into(),
            focused_part: 0,
        });
        assert!(repair(&mut run));
        assert_eq!(run.parts, vec![PartInstance::new(9, 1)]);
        assert_eq!(run.editor_dna, 55.0);
        assert_eq!(run.creature_creation.name, "Creature");
        assert!(!repair(&mut run));
    }

    #[test]
    fn the_budget_is_the_setups_minus_the_loaded_design() {
        use crate::creature_editor_data::{CreaturePartInfo, CreatureSetup};
        let mut data = CreatureEditorData::default();
        data.setups.insert(
            SetupKind::VehicleMilitaryAir,
            CreatureSetup { initial_budget: Some(5000), subtract_cost_on_load: true, ..Default::default() },
        );
        let info = CreaturePartInfo {
            key: crate::dbpf::ResourceKey { type_id: 0, group_id: 0, instance_id: 7 },
            name: "wing".into(),
            price: 300,
            complexity: 2,
            bones: 0,
            bake_level: 0,
            geom: 0.0,
            unlock_level: 1,
            find_percentage: 1.0,
            min_scale: 0.5,
            max_scale: 2.0,
            sell_back: 1.0,
            icon: crate::dbpf::ResourceKey { type_id: 0, group_id: 0, instance_id: 0 },
            model: 70,
            abilities: vec![],
            carnivore: 0,
            herbivore: 0,
            mouth: false,
            orient_to_surface: true,
            prefer_symmetry_plane: false,
            symmetry_snap_delta: None,
            acts_like_foot: false,
            acts_like_grasper: false,
            starts_unlocked: true,
        };
        data.parts.insert(7, info);
        assert_eq!(session_budget(&data, SetupKind::VehicleMilitaryAir, &[]), 5000.0);
        let one = PartInstance::new(70, 0).with_definition(7);
        let pair = [PartInstance { pair_id: 1, ..one.clone() }, PartInstance { pair_id: 1, mirrored: true, ..one.clone() }];
        assert_eq!(session_budget(&data, SetupKind::VehicleMilitaryAir, &pair), 4700.0, "a pair is one purchase");
        assert_eq!(session_budget(&data, SetupKind::VehicleMilitaryAir, &[one.clone(), one]), 4400.0);
        assert_eq!(session_budget(&data, SetupKind::Ufo, &[]), 0.0, "no setup, no budget");
        // The UFO module slot follows the palette tab.
        assert_eq!(ship_slot_of_tab(6), 0);
        assert_eq!(ship_slot_of_tab(3), 3);
        assert_eq!(ship_slot_of_tab(0), 2);
        assert_eq!(ship_slot_of_tab(1), 4);
    }

    fn bounds_of(meshes: &Assets<Mesh>, handle: &Handle<Mesh>) -> (Vec3, Vec3) {
        let mesh = meshes.get(handle).unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions");
        };
        positions.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
            let p = Vec3::from_array(*p);
            (lo.min(p), hi.max(p))
        })
    }

    /// Prints the sizes the editor works with (the measurement behind
    /// [`VEHICLE_STAGE_SCALE`] and friends): a few parts of every tab and
    /// the studio models.
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_part_and_studio_sizes() {
        use crate::{
            asset_catalog::{PartCatalog, ResourceIndex},
            creature_editor_data::CreatureEditorData,
            Originals,
        };
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = ResourceIndex::load(&install.packages);
        originals.part_catalog = PartCatalog::from_index(&originals.resource_index);
        let data = CreatureEditorData::load(&originals.resource_index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        for kind in SetupKind::ALL.into_iter().filter(|k| k.is_rigid()) {
            let palette = data.parts_palette(kind).unwrap();
            let items: usize = palette.pages.iter().map(|p| p.items.len()).sum();
            let tabs: Vec<&str> = palette.categories.iter().map(|c| c.name.as_str()).collect();
            let s = data.setup(kind).unwrap();
            println!(
                "{kind:?}: {items} items, tabs {tabs:?}, budget {:?}/{:?}, complexity {:?}, bound {:?}, heights {:?}..{:?}, bg {:08X?} pedestal {:08X?}/{:08X?}",
                s.initial_budget, s.max_budget, s.complexity_limit, s.bound_size, s.min_height, s.max_height,
                s.background_model, s.pedestal_model, s.swatch_pedestal_model
            );
        }
        for kind in [SetupKind::VehicleMilitaryLand, SetupKind::VehicleCulturalAir, SetupKind::BuildingCityHall, SetupKind::BuildingHouse, SetupKind::Ufo] {
            let palette = data.parts_palette(kind).unwrap();
            for c in &palette.categories {
                let mut shown = 0;
                for &pi in &c.pages {
                    for item in &palette.pages[pi].items {
                        if shown >= 2 {
                            continue;
                        }
                        let Some(def) = originals.part_catalog.by_definition_in(item.group, item.part).cloned() else {
                            println!("  {kind:?} {}: {:08X} not in the catalog", c.name, item.part);
                            continue;
                        };
                        match crate::editor::import_definition(&originals, &def, &mut meshes, &mut images, &mut materials) {
                            Ok(model) => {
                                shown += 1;
                                let (lo, hi) = bounds_of(&meshes, &model.handle);
                                let info = &data.parts[&item.part];
                                println!(
                                    "  {kind:?} {} '{}': mesh {lo:.2?}..{hi:.2?} size {:.2?} model.scale {:.3} (modelScale {:.3}) price {} cx {} scale {:.2}..{:.2} orient {} -> world size {:.2?}",
                                    c.name, def.name, hi - lo, model.scale, def.model_scale, info.price, info.complexity,
                                    info.min_scale, info.max_scale, info.orient_to_surface, (hi - lo) * model.scale
                                );
                            }
                            Err(e) => println!("  {kind:?} {} '{}': import fails: {e}", c.name, def.name),
                        }
                    }
                }
            }
        }
        for (name, id) in [
            ("ve_editor_landbase", 0xC7A4014Du32),
            ("ve_editor_landbase_background", 0xDAB70DA4),
            ("ve_editor_waterbase", 0x874D5C49),
            ("ve_editor_airbase", 0x8E298BFC),
            ("ve_editor_airbase_background", 0xA5E13B8B),
            ("ve_editor_ufobase", 0x89779F80),
            ("ve_editor_ufobase_background", 0x1FF03EF7),
            ("BE_editor_base_low", 0x3C3B3DB1),
            ("be_editor_background_night", 0x93447B52),
            ("be_editor_base_ui_style", 0xBF4CB375),
            ("be_editor_base_ui_style_background", 0x4DD2457C),
            ("968D20D9", 0x968D20D9),
        ] {
            let key = crate::dbpf::ResourceKey { group_id: 0x4060_6000, instance_id: id, type_id: crate::asset_catalog::PROP };
            let result = crate::world_objects::definition_at(&originals, key, name)
                .and_then(|d| crate::world_objects::import_at(&mut originals, d, &mut meshes, &mut images, &mut materials));
            match result {
                Ok((model, _)) => {
                    let (lo, hi) = bounds_of(&meshes, &model.handle);
                    println!("studio {name}: {lo:.2?}..{hi:.2?} size {:.2?} scale {:.3}", hi - lo, model.scale);
                }
                Err(e) => println!("studio {name}: {e}"),
            }
        }
    }

    #[test]
    fn classes_follow_the_stage_and_the_city_type() {
        let mut run = RunProgress::default();
        run.civ_campaign.approach = crate::civ::Approach::Religious;
        let civ = classes(&GameStage::Civilization, &run);
        assert_eq!(civ[0], SetupKind::VehicleCulturalLand);
        assert_eq!(civ[2], SetupKind::VehicleCulturalAir);
        assert!(civ[3..].iter().all(|k| k.is_building()));
        assert_eq!(classes(&GameStage::Space, &run), vec![SetupKind::Ufo]);
        assert!(classes(&GameStage::Cell, &run).is_empty());
        assert_eq!(first_class(&GameStage::Space, &run), Some(SetupKind::Ufo));
        assert!(SetupKind::BuildingHouse.is_building() && SetupKind::BuildingHouse.is_rigid());
        assert!(SetupKind::VehicleMilitaryAir.is_vehicle() && !SetupKind::Ufo.is_vehicle());
        assert_eq!(SetupKind::Ufo.part_group(), crate::asset_catalog::VEHICLE_PARTS);
        assert_eq!(SetupKind::BuildingCityHall.part_group(), crate::asset_catalog::BUILDING_PARTS);
    }
}
