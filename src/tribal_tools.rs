//! Tribal tools: Spore's tool shacks and the tools tribe members carry.
//!
//! The catalog mirrors the original `tribe_tools` PROPs (group `0x04292F52`):
//! the authored display name (`0x8F6FC401`), the food price (`modelPrice`,
//! `0x02166464`), the rack model the shack is built from
//! (`toolData_RackModelKey`, `0x04294752`) and the carried field model listed
//! in `TribeTuning.modelPreloads`. Tool *effects* are OpenSpore adaptations:
//! the original abilities (`tribe_hunt1`, `tribe_socialhorn`, ...) are
//! referenced by the PROPs but their combat formulas are not decoded here, so
//! the numbers below are honest prototype values grouped in one place.
//!
//! Every visual has a primitive fallback, so headless tests never need the
//! installation.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dbpf::ResourceKey;
use crate::game::*;
use crate::stages::StageOwned;

/// Original `tribe_tools` definitions.
pub const TOOL_GROUP: u32 = 0x0429_2F52;
/// Original tribal art PROPs (`tt_*_home`, `tt_*_field`, totem, food mats).
pub const TRIBAL_ART_GROUP: u32 = 0xF37C_24E6;
/// Original tool palette PNGs (`PUI-icon-tool-*`).
pub const TOOL_ICON_GROUP: u32 = 0x03D8_64E7;

/// One of the ten tools a tribe can build a shack for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ToolKind {
    /// `attack1`: Spiked Clubs.
    SpikedClubs,
    /// `attack2`: Stone Axes.
    StoneAxes,
    /// `attack3`: Throwing Spears (ranged).
    ThrowingSpears,
    /// `social1`: Wooden Horns.
    WoodenHorns,
    /// `social2`: Maracas.
    Maracas,
    /// `social3`: Aerophones (the didgeridoo shack).
    Aerophones,
    /// `fish`: Fishing Spears.
    FishingSpears,
    /// `gather`: Fruit Sticks.
    FruitSticks,
    /// `heal`: Healing Rods.
    HealingRods,
    /// `firepit`: Campfire (a building, not a carried tool).
    Campfire,
}

/// `toolData_ToolClass`: attack 1, social 2, utility 3, camp 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    Attack,
    Social,
    Utility,
    Camp,
}

/// Static catalog row.
#[derive(Debug, Clone, Copy)]
pub struct ToolInfo {
    pub kind: ToolKind,
    /// Authored display name.
    pub name: &'static str,
    /// Definition instance inside [`TOOL_GROUP`].
    pub definition: u32,
    /// Authored `modelPrice` in food.
    pub price: f32,
    pub class: ToolClass,
    /// Rival villages that must be settled before the shack can be built.
    /// Spore unlocks new tools as the tribe allies with or conquers others.
    pub tier: usize,
    /// Shack (rack) PROP: group and registered name.
    pub shack: (u32, &'static str),
    /// Carried field PROP, if members hold this tool.
    pub field: Option<(u32, &'static str)>,
    /// Palette PNG: group and registered name.
    pub icon: Option<(u32, &'static str)>,
    /// One-line effect shown in the palette.
    pub effect: &'static str,
}

pub const TOOLS: [ToolInfo; 10] = [
    ToolInfo {
        kind: ToolKind::SpikedClubs,
        name: "Spiked Clubs",
        definition: 0xE09F_5F26,
        price: 25.0,
        class: ToolClass::Attack,
        // Swapped with Throwing Spears: `tribeStartingTools` [3, 4] (spear and
        // horn) are read as the tools open at the start (ADAPTED: whether they
        // start owned or only unlocked is not traced).
        tier: 2,
        shack: (TRIBAL_ART_GROUP, "tt_hunt_home_01"),
        field: Some((0xC82A_6010, "tt_hunt_field_01")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-torch")),
        effect: "torch: 6 damage every 1.5 s",
    },
    ToolInfo {
        kind: ToolKind::StoneAxes,
        name: "Stone Axes",
        definition: 0xE09F_5F25,
        price: 25.0,
        class: ToolClass::Attack,
        tier: 1,
        shack: (TRIBAL_ART_GROUP, "tt_hunt_home_02"),
        field: Some((0xC82A_6013, "tt_hunt_field_02")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-axe")),
        effect: "axe: 9 damage every 2 s",
    },
    ToolInfo {
        kind: ToolKind::ThrowingSpears,
        name: "Throwing Spears",
        definition: 0xE09F_5F24,
        price: 25.0,
        class: ToolClass::Attack,
        // `tribeStartingTools` lists the spear (type 3): open from the start.
        tier: 0,
        shack: (TRIBAL_ART_GROUP, "tt_hunt_home_03"),
        field: Some((0xC82A_6012, "tt_hunt_field_03")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-spears")),
        effect: "spear: 5 close, 10 thrown, every 3 s",
    },
    ToolInfo {
        kind: ToolKind::WoodenHorns,
        name: "Wooden Horns",
        definition: 0xF07B_79E9,
        price: 25.0,
        class: ToolClass::Social,
        tier: 0,
        shack: (TRIBAL_ART_GROUP, "tt_Horn_home"),
        field: Some((TRIBAL_ART_GROUP, "tt_horn_field")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-horn2")),
        effect: "plays the Horn [3] in performances",
    },
    ToolInfo {
        kind: ToolKind::Maracas,
        name: "Maracas",
        definition: 0xF07B_79EA,
        price: 25.0,
        class: ToolClass::Social,
        tier: 1,
        shack: (TRIBAL_ART_GROUP, "tt_Maraca_home"),
        field: Some((TRIBAL_ART_GROUP, "tt_Maraca_field")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-maracas")),
        effect: "plays the Maracas [4] in performances",
    },
    ToolInfo {
        kind: ToolKind::Aerophones,
        name: "Aerophones",
        definition: 0xF07B_79EB,
        price: 25.0,
        class: ToolClass::Social,
        tier: 2,
        shack: (TRIBAL_ART_GROUP, "tt_Didgeridoo_home"),
        field: Some((TRIBAL_ART_GROUP, "tt_Didgeridoo_field")),
        icon: Some((TOOL_ICON_GROUP, "PUI-icon-tool-didgeridoo")),
        effect: "plays the Didgeridoo [5] in performances",
    },
    ToolInfo {
        kind: ToolKind::FishingSpears,
        name: "Fishing Spears",
        definition: 0xA0AC_7B57,
        price: 20.0,
        class: ToolClass::Utility,
        tier: 0,
        shack: (TRIBAL_ART_GROUP, "tt_fish_home_01"),
        field: Some((0x5152_F1AE, "tt_fish_field_01")),
        icon: Some((TRIBAL_ART_GROUP, "trg_icon_fish1")),
        effect: "lets members fish at the shore",
    },
    ToolInfo {
        kind: ToolKind::FruitSticks,
        name: "Fruit Sticks",
        definition: 0xC1E9_F31C,
        price: 20.0,
        class: ToolClass::Utility,
        tier: 1,
        shack: (TRIBAL_ART_GROUP, "tt_gather_home_03"),
        field: Some((0xC279_C704, "tt_gather_treebeater")),
        icon: Some((TRIBAL_ART_GROUP, "trg_icon_gather3")),
        effect: "smacks 5 fruit down at once (3 by hand)",
    },
    ToolInfo {
        kind: ToolKind::HealingRods,
        name: "Healing Rods",
        definition: 0xCD88_7E0F,
        price: 20.0,
        class: ToolClass::Utility,
        tier: 2,
        shack: (TRIBAL_ART_GROUP, "tt_heal_home_02"),
        field: Some((0x38EB_ED6E, "tt_heal_field_staff")),
        icon: Some((TOOL_ICON_GROUP, "PUI_TRG_medic")),
        effect: "heals 5% health a second nearby",
    },
    ToolInfo {
        kind: ToolKind::Campfire,
        name: "Campfire",
        definition: 0xFF7F_9A4C,
        price: 15.0,
        class: ToolClass::Camp,
        tier: 0,
        shack: (TRIBAL_ART_GROUP, "tt_firepit_home_01"),
        field: None,
        icon: None,
        effect: "members resting at home heal",
    },
];

/// The chieftain's staff (`tt_chieftain_staff`), always carried.
pub const CHIEFTAIN_STAFF: (u32, &str) = (TRIBAL_ART_GROUP, "tt_chieftain_staff");

pub fn info(kind: ToolKind) -> &'static ToolInfo {
    TOOLS
        .iter()
        .find(|t| t.kind == kind)
        .expect("every tool kind has a catalog row")
}

/// Pure: whether a shack can be built once `resolved` villages are settled.
pub fn is_unlocked(kind: ToolKind, resolved: usize) -> bool {
    info(kind).tier <= resolved
}

/// Pure: the next tool in palette order (wrapping).
pub fn next_tool(kind: ToolKind) -> ToolKind {
    let i = TOOLS.iter().position(|t| t.kind == kind).unwrap_or(0);
    TOOLS[(i + 1) % TOOLS.len()].kind
}

// ---------------------------------------------------------------------------
// Effects (the weapon numbers are the original's, see `tribal_rules`)
// ---------------------------------------------------------------------------

/// Melee damage per second against creatures and rival members: the weapon's
/// damage over its refresh (torch 6 per 1.5 s, axe 9 per 2 s, spear 5 per 3 s,
/// Healing Rods' staff 3 per 2.5 s; bare hands are ADAPTED).
pub fn creature_dps(tool: Option<ToolKind>) -> f32 {
    crate::tribal_rules::weapon_of(tool).melee_dps()
}

/// Melee reach of an attack (ADAPTED: about a creature's length; the
/// weapons' `range` 1 is in the original's own units, not traced).
pub const MELEE_REACH: f32 = 1.9;

/// Reach of an attack: melee, or a thrown spear (the spear's retail `range`,
/// 25 units, now that the stage plays at retail distances).
pub fn attack_range(tool: Option<ToolKind>) -> f32 {
    match tool {
        Some(ToolKind::ThrowingSpears) => {
            crate::tribal_rules::SPEAR.range * crate::tribal_rules::RANGE_SCALE
        }
        _ => MELEE_REACH,
    }
}

/// Only members holding fishing spears can fish.
pub fn can_fish(tool: Option<ToolKind>) -> bool {
    tool == Some(ToolKind::FishingSpears)
}

/// The performance note (`3`/`4`/`5`) an instrument plays.
pub fn instrument_note(tool: Option<ToolKind>) -> Option<u32> {
    match tool {
        Some(ToolKind::WoodenHorns) => Some(3),
        Some(ToolKind::Maracas) => Some(4),
        Some(ToolKind::Aerophones) => Some(5),
        _ => None,
    }
}

/// The instrument that plays performance note `note`.
pub fn instrument_for_note(note: u32) -> Option<ToolKind> {
    match note {
        3 => Some(ToolKind::WoodenHorns),
        4 => Some(ToolKind::Maracas),
        5 => Some(ToolKind::Aerophones),
        _ => None,
    }
}

/// Percent of a member's maximum health per second a healing-rod carrier
/// restores around itself (`healPercentPerSecondWithTool`, 5).
pub fn heal_rate(tool: Option<ToolKind>) -> f32 {
    if tool == Some(ToolKind::HealingRods) {
        crate::tribal_rules::HEAL_PERCENT_PER_SECOND_WITH_TOOL
    } else {
        0.0
    }
}

/// Radius a healing rod reaches (`shamanHealingDistances`, up to 5).
pub const HEAL_RADIUS: f32 = crate::tribal_rules::SHAMAN_HEALING_DISTANCES.1;
/// Health per second near a built campfire at home.
pub const CAMPFIRE_REGEN: f32 = 3.0;
/// Radius of the campfire's warmth around the home hut.
pub const CAMPFIRE_RADIUS: f32 = 9.0;

// ---------------------------------------------------------------------------
// Home layout
// ---------------------------------------------------------------------------

/// Where shack `slot` stands: an arc behind and beside the home hut, leaving
/// the front (toward the default camera) for the food mat and totem.
pub fn shack_position(slot: usize) -> Vec3 {
    const ANGLES: [f32; 10] = [
        200.0, 340.0, 235.0, 305.0, 270.0, 165.0, 15.0, 130.0, 50.0, 270.0,
    ];
    let angle = ANGLES[slot % ANGLES.len()].to_radians();
    let radius = if slot >= 9 { 11.5 } else { 8.0 };
    crate::tribal::home_position() + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
}

// ---------------------------------------------------------------------------
// Original prop spawning shared by the tribal modules
// ---------------------------------------------------------------------------

/// Spawn an original world PROP with every native surface (`world_objects`
/// draws only the first surface; several tribal props are multi-material).
/// The root is fitted to `width` and its lowest vertex sits at `position.y`.
/// Returns `None` without an installation or when the PROP cannot import.
#[allow(clippy::too_many_arguments)]
pub fn spawn_prop(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    group: u32,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
    width: f32,
) -> Option<Entity> {
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let key = ResourceKey {
        group_id: group,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let result = (|| {
        let d = crate::world_objects::definition_at(originals, key, name)?;
        let (model, source) =
            crate::world_objects::import_at(originals, d, meshes, images, materials)?;
        let transform = crate::world_objects::fitted_transform(&model, meshes, position, width)?;
        Ok::<_, String>((model, source, transform))
    })();
    let (model, source, transform) = match result {
        Ok(found) => found,
        Err(e) => {
            warn!("original tribal prop {name}: {e}");
            return None;
        }
    };
    let root = spawn_model(commands, materials, &model, source, name, transform);
    // The PROP's own effects (camp fire flames, rack torches, tool pads, hut
    // smoke, totem glow; `modelEffect`), riding the fitted root.
    crate::tribal_village::attach_model_effects(commands, originals, key, root);
    Some(root)
}

/// Root + one child per surface (or the root alone for single-surface models).
fn spawn_model(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    model: &crate::OriginalModel,
    source: crate::world_objects::OriginalWorldObject,
    name: &str,
    transform: Transform,
) -> Entity {
    if model.surfaces.len() <= 1 {
        let material = model
            .material
            .as_ref()
            .and_then(|h| materials.get(h))
            .cloned()
            .unwrap_or_default();
        return commands
            .spawn((
                StageOwned,
                Name::new(name.to_string()),
                source,
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(materials.add(material)),
                transform,
            ))
            .id();
    }
    let root = commands
        .spawn((
            StageOwned,
            Name::new(name.to_string()),
            source,
            transform,
            Visibility::Inherited,
        ))
        .id();
    commands.entity(root).with_children(|p| {
        for surface in &model.surfaces {
            p.spawn((
                StageOwned,
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(surface.material.clone()),
                Transform::IDENTITY,
            ));
        }
    });
    root
}

/// Import an original PROP's model for use as a child (held tool, carried
/// food). Cached by `world_objects::import_at`.
pub fn import_prop(
    originals: &mut crate::Originals,
    group: u32,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<crate::OriginalModel> {
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let key = ResourceKey {
        group_id: group,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let d = crate::world_objects::definition_at(originals, key, name).ok()?;
    crate::world_objects::import_at(originals, d, meshes, images, materials)
        .map(|(m, _)| m)
        .map_err(|e| warn!("original tribal prop {name}: {e}"))
        .ok()
}

/// Local transform that centres `model` on `anchor` with its largest axis
/// `length` long (field tools are long and thin, so the footprint fit used for
/// buildings would blow them up).
pub fn fit_largest_axis(
    model: &crate::OriginalModel,
    meshes: &Assets<Mesh>,
    anchor: Vec3,
    length: f32,
) -> Option<Transform> {
    let mesh = meshes.get(&model.handle)?;
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for v in vertices {
        let v = Vec3::from_array(*v);
        min = min.min(v);
        max = max.max(v);
    }
    let span = (max - min).max_element();
    if !span.is_finite() || span <= 0.0 {
        return None;
    }
    let scale = length / span;
    let centre = (min + max) * 0.5;
    Some(Transform::from_translation(anchor - centre * scale).with_scale(Vec3::splat(scale)))
}

/// Spawn `model` as a child of `parent` (one entity per surface under a
/// pivot), returning the pivot.
pub fn spawn_model_child(
    commands: &mut Commands,
    parent: Entity,
    model: &crate::OriginalModel,
    pivot: Transform,
    fitted: Transform,
    marker: impl Bundle,
) -> Entity {
    let pivot_entity = commands
        .spawn((
            StageOwned,
            marker,
            pivot,
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    let surfaces: Vec<(Handle<Mesh>, Option<Handle<StandardMaterial>>)> =
        if model.surfaces.is_empty() {
            vec![(model.handle.clone(), model.material.clone())]
        } else {
            model
                .surfaces
                .iter()
                .map(|s| (s.handle.clone(), Some(s.material.clone())))
                .collect()
        };
    for (mesh, material) in surfaces {
        let mut child = commands.spawn((StageOwned, Mesh3d(mesh), fitted, ChildOf(pivot_entity)));
        if let Some(material) = material {
            child.insert(MeshMaterial3d(material));
        }
    }
    pivot_entity
}

// ---------------------------------------------------------------------------
// Palette icons
// ---------------------------------------------------------------------------

/// Imported palette icons, keyed by tool.
#[derive(Resource, Default, Clone)]
pub struct ToolIcons {
    pub loaded: bool,
    pub icons: Vec<(ToolKind, Handle<Image>)>,
    pub chieftain: Option<Handle<Image>>,
}

impl ToolIcons {
    pub fn get(&self, kind: ToolKind) -> Option<Handle<Image>> {
        self.icons
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, h)| h.clone())
    }
}

/// Load the original palette PNGs once (no-op without an installation).
pub fn load_icons(icons: &mut ToolIcons, originals: &crate::Originals, images: &mut Assets<Image>) {
    if icons.loaded {
        return;
    }
    icons.loaded = true;
    if originals.resource_index.records.is_empty() {
        return;
    }
    let png = |group: u32, name: &str| ResourceKey {
        group_id: group,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::ui_art::UI_PNG_TYPE,
    };
    for tool in &TOOLS {
        let Some((group, name)) = tool.icon else {
            continue;
        };
        match crate::ui_art::import_png(&originals.resource_index, png(group, name), images) {
            Ok((image, _, _)) => icons.icons.push((tool.kind, image)),
            Err(e) => warn!("tool icon {name}: {e}"),
        }
    }
    icons.chieftain = crate::ui_art::import_png(
        &originals.resource_index,
        png(TOOL_ICON_GROUP, "PUI_TRG_chieftain"),
        images,
    )
    .ok()
    .map(|(h, _, _)| h);
}

// ---------------------------------------------------------------------------
// Shacks and held tools (presentation kept in step with the campaign)
// ---------------------------------------------------------------------------

/// A built tool shack next to the home hut.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolShack(pub ToolKind);

/// Marker on the pivot child holding a member's tool model.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldTool(pub Option<ToolKind>);

/// What a unit currently shows in its hand (`None` = empty-handed).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeldToolState {
    pub shown: Option<Option<ToolKind>>,
}

/// Fallback colour of a primitive shack / tool.
pub fn tool_color(kind: ToolKind) -> Color {
    match info(kind).class {
        ToolClass::Attack => Color::srgb(0.70, 0.30, 0.22),
        ToolClass::Social => Color::srgb(0.85, 0.72, 0.30),
        ToolClass::Utility => Color::srgb(0.30, 0.55, 0.70),
        ToolClass::Camp => Color::srgb(0.95, 0.45, 0.10),
    }
}

/// The rack PROP a shack shows: the camp fire follows the tribe level
/// (`tt_firepit_home_01/02/03`, VERIFIED 0xC91BD0); the other tools have one
/// rack (`toolData_RackModelKey`).
pub fn shack_model(kind: ToolKind, level: u8) -> (u32, &'static str) {
    match kind {
        ToolKind::Campfire => (TRIBAL_ART_GROUP, crate::tribal_village::firepit_name(level)),
        _ => info(kind).shack,
    }
}

/// Which rack model a shack shows (the camp fire changes with the level).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShackModel(pub &'static str);

/// Spawn the shack for `kind` in `slot` (original model, else a primitive);
/// `level` is the tribe level 0..=2 (the camp fire's look).
#[allow(clippy::too_many_arguments)]
pub fn spawn_shack(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    kind: ToolKind,
    slot: usize,
    level: u8,
) -> Entity {
    let pos = shack_position(slot);
    let (group, name) = shack_model(kind, level);
    let width = if kind == ToolKind::Campfire { 2.6 } else { 3.6 };
    if let Some(e) = spawn_prop(
        commands,
        originals,
        group,
        name,
        meshes,
        images,
        materials,
        Vec3::new(pos.x, -0.6, pos.z),
        width,
    ) {
        // Face the shack toward the hut, as in Spore's village ring.
        let yaw = (-(pos.x)).atan2(-(pos.z)) + std::f32::consts::PI;
        commands
            .entity(e)
            .insert((ToolShack(kind), ShackModel(name), Name::new(info(kind).name)));
        commands.queue(move |world: &mut World| {
            if let Some(mut t) = world.get_mut::<Transform>(e) {
                t.rotation = Quat::from_rotation_y(yaw);
            }
        });
        return e;
    }
    commands
        .spawn((
            StageOwned,
            ToolShack(kind),
            ShackModel(name),
            Name::new(info(kind).name),
            Mesh3d(meshes.add(Cuboid::new(2.0, 1.4, 1.6))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: tool_color(kind),
                perceptual_roughness: 1.0,
                ..default()
            })),
            Transform::from_xyz(pos.x, 0.1, pos.z),
        ))
        .id()
}

/// Mount the model a unit holds (`None` = the chieftain's staff when
/// `chieftain`, otherwise nothing) as a child of `unit`.
#[allow(clippy::too_many_arguments)]
pub fn mount_held_tool(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unit: Entity,
    tool: Option<ToolKind>,
    chieftain: bool,
) {
    let source = match tool {
        Some(kind) => info(kind).field,
        None if chieftain => Some(CHIEFTAIN_STAFF),
        None => None,
    };
    let Some((group, name)) = source else {
        return;
    };
    // Held at the creature's right side, tilted forward like a carried tool.
    let pivot = Transform::from_xyz(0.62, -0.05, -0.15)
        .with_rotation(Quat::from_rotation_x(-0.35) * Quat::from_rotation_z(-0.15));
    let length = match tool {
        Some(ToolKind::ThrowingSpears) | Some(ToolKind::FishingSpears) => 1.7,
        Some(ToolKind::Maracas) => 0.8,
        Some(ToolKind::WoodenHorns) => 1.0,
        Some(ToolKind::Aerophones) => 1.5,
        None => 1.8,
        _ => 1.3,
    };
    if let Some(model) = import_prop(originals, group, name, meshes, images, materials) {
        if let Some(fitted) = fit_largest_axis(&model, meshes, Vec3::ZERO, length) {
            spawn_model_child(commands, unit, &model, pivot, fitted, HeldTool(tool));
            return;
        }
    }
    let color = tool
        .map(tool_color)
        .unwrap_or(Color::srgb(0.45, 0.30, 0.15));
    let pivot_entity = commands
        .spawn((
            StageOwned,
            HeldTool(tool),
            pivot,
            Visibility::Inherited,
            ChildOf(unit),
        ))
        .id();
    commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(Cylinder::new(0.06, length))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            ..default()
        })),
        Transform::IDENTITY,
        ChildOf(pivot_entity),
    ));
}

/// Keep built shacks and the tools members hold in step with the campaign.
#[allow(clippy::too_many_arguments)]
pub fn tribal_tools_sync(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shacks: Query<(Entity, &ToolShack, Option<&ShackModel>)>,
    mut units: Query<(
        Entity,
        &crate::tribal_rts::TribeUnit,
        Option<&crate::persistence::WorldId>,
        &mut HeldToolState,
    )>,
    held: Query<(Entity, &HeldTool, &ChildOf)>,
) {
    let campaign = &run.tribal_campaign;
    let level = crate::tribal_rules::tribe_level(campaign.resolved_count());
    for (slot, kind) in campaign.shacks.iter().enumerate() {
        let shown = shacks.iter().find(|s| s.1 .0 == *kind);
        // The camp fire is rebuilt in the look of a new tribe level.
        if let Some((entity, _, Some(model))) = shown {
            if model.0 != shack_model(*kind, level).1 {
                commands.entity(entity).despawn();
            } else {
                continue;
            }
        } else if shown.is_some() {
            continue;
        }
        spawn_shack(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            *kind,
            slot,
            level,
        );
    }
    for (entity, unit, id, mut state) in &mut units {
        let wanted = if unit.chieftain {
            None
        } else {
            id.and_then(|id| campaign.tool_of(id.0))
        };
        if state.shown == Some(wanted) {
            continue;
        }
        for (child, _, parent) in &held {
            if parent.parent() == entity {
                commands.entity(child).despawn();
            }
        }
        mount_held_tool(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            entity,
            wanted,
            unit.chieftain,
        );
        state.shown = Some(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_matches_the_original_tool_definitions() {
        assert_eq!(TOOLS.len(), 10);
        // Authored modelPrice values from the installed tribe_tools PROPs.
        assert_eq!(info(ToolKind::SpikedClubs).price, 25.0);
        assert_eq!(info(ToolKind::FishingSpears).price, 20.0);
        assert_eq!(info(ToolKind::Campfire).price, 15.0);
        // Every kind appears exactly once and every carried tool has a model.
        for (i, tool) in TOOLS.iter().enumerate() {
            assert!(TOOLS[i + 1..].iter().all(|t| t.kind != tool.kind));
            assert_eq!(tool.field.is_none(), tool.kind == ToolKind::Campfire);
        }
        assert_eq!(
            crate::world_objects::name_id("tt_hunt_home_01"),
            0x43C7_59C7,
            "rack model names hash to the authored instance"
        );
    }

    #[test]
    fn tools_unlock_in_tiers_as_villages_settle() {
        let start: Vec<_> = TOOLS
            .iter()
            .filter(|t| is_unlocked(t.kind, 0))
            .map(|t| t.kind)
            .collect();
        // `tribeStartingTools` [3, 4]: the spear and the horn are open at once.
        assert!(start.contains(&ToolKind::ThrowingSpears));
        assert!(!start.contains(&ToolKind::SpikedClubs));
        assert!(start.contains(&ToolKind::WoodenHorns));
        assert!(start.contains(&ToolKind::FishingSpears));
        assert!(!start.contains(&ToolKind::Maracas));
        assert!(is_unlocked(ToolKind::Maracas, 1));
        assert!(is_unlocked(ToolKind::Aerophones, 2));
        assert!(TOOLS.iter().all(|t| is_unlocked(t.kind, 2)));
        // The palette cursor visits every tool.
        let mut k = ToolKind::SpikedClubs;
        for _ in 0..TOOLS.len() {
            k = next_tool(k);
        }
        assert_eq!(k, ToolKind::SpikedClubs);
    }

    #[test]
    fn tools_have_real_effects() {
        // Weapon damage per second is the retail damage over the refresh.
        assert!((creature_dps(Some(ToolKind::SpikedClubs)) - 4.0).abs() < 1e-4);
        assert!((creature_dps(Some(ToolKind::StoneAxes)) - 4.5).abs() < 1e-4);
        assert!((creature_dps(Some(ToolKind::ThrowingSpears)) - 5.0 / 3.0).abs() < 1e-4);
        assert!(creature_dps(Some(ToolKind::StoneAxes)) > creature_dps(None));
        assert!(attack_range(Some(ToolKind::ThrowingSpears)) > attack_range(None));
        assert_eq!(attack_range(None), MELEE_REACH);
        assert!(can_fish(Some(ToolKind::FishingSpears)) && !can_fish(None));
        for note in 3..=5 {
            assert_eq!(instrument_note(instrument_for_note(note)), Some(note));
        }
        // Healing Rods restore 5% of the maximum health a second within 5 units.
        assert_eq!(heal_rate(Some(ToolKind::HealingRods)), 5.0);
        assert_eq!(heal_rate(None), 0.0);
        assert_eq!(HEAL_RADIUS, 5.0);
    }

    #[test]
    fn shack_slots_ring_the_hut_without_overlapping() {
        for a in 0..TOOLS.len() {
            let pa = shack_position(a);
            assert!(dist_xz(pa, crate::tribal::home_position()) > 5.0);
            for b in a + 1..TOOLS.len() {
                assert!(
                    dist_xz(pa, shack_position(b)) > 3.4,
                    "slots {a} and {b} overlap"
                );
            }
        }
    }
}
