//! Shared run progression, save data, and common components.
//!
//! Pure progression logic is kept free of Bevy so it can be unit-tested
//! headlessly (`cargo test`). Systems only map it onto entities.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::stages::GameStage;

pub const SAVE_FILE: &str = "openspore_save.json";

fn default_editor_dna() -> f32 {
    25.0
}

fn default_acc_scale() -> f32 {
    1.0
}

/// DNA / points earned per stage; thresholds gate evolution to the next stage.
#[derive(Debug, Clone, Copy)]
pub struct StageTuning {
    pub dna_to_advance: f32,
    pub drain_per_sec: f32,
}

pub fn tuning_for(stage: &GameStage) -> StageTuning {
    match stage {
        GameStage::MainMenu => StageTuning {
            dna_to_advance: 0.0,
            drain_per_sec: 0.0,
        },
        GameStage::Cell => StageTuning {
            dna_to_advance: 100.0,
            drain_per_sec: 0.4,
        },
        GameStage::Creature => StageTuning {
            // Retail (creature_rules): the last brain level is reached at the
            // stage total 1000 (cumulative 100/300/600/1000); the medium hunger
            // drain is 1.111 per second.
            dna_to_advance: crate::creature_rules::STAGE_TOTAL_DNA,
            drain_per_sec: 1.111,
        },
        GameStage::Tribal => StageTuning {
            dna_to_advance: 200.0,
            drain_per_sec: 0.5,
        },
        GameStage::Civilization => StageTuning {
            dna_to_advance: 300.0,
            drain_per_sec: 0.0,
        },
        GameStage::Space => StageTuning {
            dna_to_advance: 5.0, // colonies
            drain_per_sec: 0.0,
        },
    }
}

/// Gameplay stats derived from the assembled creature (the original couples
/// part choices to stats; we couple count + part size). Pure over the saved
/// part list so it is unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CreatureStats {
    /// Walk speed (world units/sec).
    pub speed: f32,
    /// Max health.
    pub max_hp: f32,
    /// Social multiplier on befriending.
    pub social: f32,
    /// Bulk multiplier (visual + hitbox scaling).
    pub size: f32,
}

impl Default for CreatureStats {
    fn default() -> Self {
        Self {
            speed: 8.0,
            max_hp: 120.0,
            social: 1.0,
            size: 1.0,
        }
    }
}

/// Pure: stats from the assembled creature. `scale_of` maps a part's RW4
/// instance to its catalog scale (None = unknown part, counted at scale 1).
/// More/bigger parts ⇒ tougher and more social, but slower and bulkier.
pub fn creature_stats(
    parts: &[PartInstance],
    scale_of: impl Fn(u32) -> Option<f32>,
) -> CreatureStats {
    let mut s = CreatureStats::default();
    for p in parts {
        let sc = scale_of(p.instance).unwrap_or(1.0).max(0.05);
        s.size += 0.06 * sc;
        s.speed -= 0.10 * sc;
        s.max_hp += 10.0 * sc;
        s.social += 0.04;
    }
    // Bounded so no creature is unusable.
    s.speed = s.speed.clamp(4.5, 11.0);
    s.max_hp = s.max_hp.clamp(120.0, 400.0);
    s.social = s.social.clamp(1.0, 2.5);
    s.size = s.size.clamp(0.8, 1.6);
    s
}

/// A mounted original part in OpenSpore's saved assembly. This is an internal
/// save representation, not the original game's creature-file format.
/// `instance` is the source RW4 record ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartInstance {
    pub instance: u32,
    /// Optional PROP definition instance ID when the equipped part is a
    /// distinct catalog definition that shares `instance` (its RW4 model ID)
    /// with other definitions. `None` falls back to canonical model lookup.
    #[serde(default)]
    pub definition_id: Option<u32>,
    /// Slot bone index on the creature rig.
    pub bone: u32,
    /// Editor yaw around the slot (radians).
    #[serde(default)]
    pub yaw: f32,
    /// Editor scale multiplier.
    #[serde(default = "default_part_scale")]
    pub scale: f32,
    /// Editor slide on the slot, in bone space (metres).
    #[serde(default = "default_part_offset")]
    pub offset: [f32; 3],
    /// Nonzero IDs join a purchased symmetric pair into one editor item.
    /// Old saves retain their independent parts and refund behavior.
    #[serde(default)]
    pub pair_id: u32,
    /// Reflect the source mesh across the body's sagittal plane.
    #[serde(default)]
    pub mirrored: bool,
    /// Free transform of a rigid block while a vehicle / building / UFO
    /// editor session runs (`rigid_editor`); creature parts have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigid: Option<crate::rigid_editor::RigidBlock>,
    /// Creature Creator edits: rotation ring, deform handle weights, limb
    /// chain (`creature_handles`, `creature_limbs`). Missing in older saves.
    #[serde(default, skip_serializing_if = "crate::creature_handles::PartEdits::is_default")]
    pub edits: crate::creature_handles::PartEdits,
}

fn default_part_scale() -> f32 {
    1.0
}

fn default_part_offset() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

fn default_tribe() -> Vec<MateAssignment> {
    // `tribePopulationStart` (tribal_rules).
    vec![MateAssignment::default(); crate::tribal_rules::TRIBE_POPULATION_START]
}

impl PartInstance {
    pub fn new(instance: u32, bone: u32) -> Self {
        Self {
            instance,
            definition_id: None,
            bone,
            yaw: 0.0,
            scale: 1.0,
            offset: [0.0, 0.0, 0.0],
            pair_id: 0,
            mirrored: false,
            rigid: None,
            edits: Default::default(),
        }
    }

    /// Orientation on the body: the editor yaw, then the rotation ring.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * self.edits.ring()
    }

    pub fn with_definition(mut self, definition_id: u32) -> Self {
        self.definition_id = Some(definition_id);
        self
    }

    /// Shift the offset along one axis (0=x, 1=y, 2=z) by `delta`.
    pub fn shift_offset(&mut self, axis: usize, delta: f32) {
        if axis < 3 {
            self.offset[axis] += delta;
        }
    }

    pub fn mesh_scale(&self, scale: f32) -> Vec3 {
        Vec3::new(if self.mirrored { -scale } else { scale }, scale, scale)
    }
}

/// Pure: clamp an offset to a sane editor range (±`limit` per axis).
pub fn clamp_offset(o: [f32; 3], limit: f32) -> [f32; 3] {
    [
        o[0].clamp(-limit, limit),
        o[1].clamp(-limit, limit),
        o[2].clamp(-limit, limit),
    ]
}

/// Pure: mirror a part onto the paired slot from `map` (bone-space x and the
/// editor yaw negate; the instance and scale carry over). With no pair, the
/// part stays on its own bone (centreline mirror of the placement).
pub fn mirror_part(p: &PartInstance, map: &[Option<usize>]) -> PartInstance {
    let bone = map
        .get(p.bone as usize)
        .copied()
        .flatten()
        .unwrap_or(p.bone as usize);
    PartInstance {
        instance: p.instance,
        definition_id: p.definition_id,
        bone: bone as u32,
        yaw: -p.yaw,
        scale: p.scale,
        offset: [-p.offset[0], p.offset[1], p.offset[2]],
        pair_id: p.pair_id,
        mirrored: !p.mirrored,
        rigid: None,
        edits: p.edits.mirrored(),
    }
}

/// Pure: add or update a mirrored copy of the focused part.
/// With a paired slot (`map` resolves to a *different* bone) the copy mounts
/// there; otherwise (asymmetric rig, or a midline slot) the copy stays on the
/// same bone with the bone-space X offset and yaw negated — a centreline
/// mirror, which is how Spore places paired details.
/// Returns true when the list changed (caller persists).
pub fn part_add_mirrored(parts: &mut Vec<PartInstance>, focus: u32, map: &[Option<usize>]) -> bool {
    let Some(p) = parts.get(focus as usize) else {
        return false;
    };
    let m = mirror_part(p, map);
    if let Some(existing) = parts
        .iter_mut()
        .find(|q| q.bone == m.bone && q.offset == m.offset)
    {
        *existing = m;
    } else {
        parts.push(m);
    }
    true
}

/// Editor slide step (metres per keypress).
pub const OFFSET_STEP: f32 = 0.1;
/// Editor slide clamp (±metres).
pub const OFFSET_LIMIT: f32 = 2.0;

/// Shared slide controls used by the creature assembler, the tribal stage and
/// the civ vehicle editor: `I`/`K` slide X, `J`/`L` slide Z, `U`/`O` slide Y
/// on a 3-component offset array.
pub fn apply_slide_keys(keys: &ButtonInput<KeyCode>, part: &mut PartInstance) -> bool {
    apply_slide_offset(keys, &mut part.offset)
}

/// The same slide controls over a raw offset array (vehicle parts use it).
pub fn apply_slide_offset(keys: &ButtonInput<KeyCode>, offset: &mut [f32; 3]) -> bool {
    let mut moved = false;
    let s = OFFSET_STEP;
    if keys.just_pressed(KeyCode::KeyI) {
        offset[0] += s;
        moved = true;
    }
    if keys.just_pressed(KeyCode::KeyK) {
        offset[0] -= s;
        moved = true;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        offset[2] -= s;
        moved = true;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        offset[2] += s;
        moved = true;
    }
    if keys.just_pressed(KeyCode::KeyU) {
        offset[1] += s;
        moved = true;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        offset[1] -= s;
        moved = true;
    }
    if moved {
        *offset = clamp_offset(*offset, OFFSET_LIMIT);
    }
    moved
}

/// Semantic kind of a creature slot bone, derived from its bind position.
/// The retail rig's extent is ~1.2 units, so the thresholds are relative to
/// that scale and pure (no rig needed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// Root / midline spine.
    Root,
    /// Off-midline bones: limbs, graspers, wings.
    Limb,
    /// High on the midline: head assembly.
    Head,
    /// Behind the body: tail.
    Tail,
}

/// Pure: classify a bind-space position into a slot kind.
pub fn slot_kind(p: Vec3) -> SlotKind {
    if p.x.abs() > 0.15 {
        SlotKind::Limb
    } else if p.y > 0.45 {
        SlotKind::Head
    } else if p.z < -0.45 {
        SlotKind::Tail
    } else {
        SlotKind::Root
    }
}

/// Pure: classify a part by its rig size — limb parts rig a full limb chain
/// (>= 12 joints), details only a few.
pub fn part_family(rig_bones: usize) -> &'static str {
    if rig_bones >= 12 {
        "limb"
    } else if rig_bones >= 6 {
        "detail"
    } else {
        "tiny"
    }
}

/// A mounted vehicle part (mirrors the creature part record: part + slot +
/// transform). `instance` is the source RW4 record ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VehiclePart {
    pub instance: u32,
    /// Optional PROP definition instance ID when the equipped part is a
    /// distinct catalog definition that shares `instance` (its RW4 model ID)
    /// with other definitions. `None` falls back to canonical model lookup.
    #[serde(default)]
    pub definition_id: Option<u32>,
    /// Index into `VEHICLE_SLOTS`.
    pub slot: u32,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "default_part_scale")]
    pub scale: f32,
    #[serde(default = "default_part_offset")]
    pub offset: [f32; 3],
    /// Free transform from the rigid-block editors (`rigid_editor`); when
    /// present it replaces the slot placement (`slot`, `yaw`, `offset`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<crate::rigid_editor::RigidBlock>,
}

impl VehiclePart {
    pub fn new(instance: u32, slot: u32) -> Self {
        Self {
            instance,
            definition_id: None,
            slot,
            yaw: 0.0,
            scale: 1.0,
            offset: [0.0, 0.0, 0.0],
            block: None,
        }
    }

    pub fn with_definition(mut self, definition_id: u32) -> Self {
        self.definition_id = Some(definition_id);
        self
    }
}

/// Tribe member roles (Spore's tribal jobs). Pure multipliers so the whole
/// tribe's effect is computable from the assignments.
pub const TRIBE_ROLES: &[(&str, f32, f32)] = &[
    // name, gather multiplier, defense multiplier
    ("gatherer", 0.15, 0.0),
    ("warrior", 0.0, 0.35),
    ("shaman", 0.05, 0.05),
    ("idler", 0.0, 0.0),
];

/// A mounted ship module (mirrors the creature/vehicle part record).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShipModule {
    pub instance: u32,
    /// Optional PROP definition instance ID when the equipped part is a
    /// distinct catalog definition that shares `instance` (its RW4 model ID)
    /// with other definitions. `None` falls back to canonical model lookup.
    #[serde(default)]
    pub definition_id: Option<u32>,
    /// Index into `SHIP_SLOTS`.
    pub slot: u32,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "default_part_scale")]
    pub scale: f32,
    #[serde(default = "default_part_offset")]
    pub offset: [f32; 3],
    /// Free transform from the UFO editor (`rigid_editor`); when present it
    /// replaces the slot placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<crate::rigid_editor::RigidBlock>,
}

impl ShipModule {
    pub fn new(instance: u32, slot: u32) -> Self {
        Self {
            instance,
            definition_id: None,
            slot,
            yaw: 0.0,
            scale: 1.0,
            offset: [0.0, 0.0, 0.0],
            block: None,
        }
    }

    pub fn with_definition(mut self, definition_id: u32) -> Self {
        self.definition_id = Some(definition_id);
        self
    }
}

/// The space ship's loadout plus focus cursor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShipBuild {
    pub modules: Vec<ShipModule>,
    #[serde(default)]
    pub focused: u32,
}

impl Default for ShipBuild {
    fn default() -> Self {
        Self {
            modules: Vec::new(),
            focused: 0,
        }
    }
}

/// A ship module slot: a named mount point in ship-local units (the UFO is a
/// ~1.8 x 1.8 box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShipSlot {
    pub name: &'static str,
    pub offset: Vec3,
    pub base_yaw: f32,
    /// What the slot's modules affect: 0 = weapon, 1 = hull, 2 = engine.
    pub kind: u8,
}

/// The UFO's mount points: two weapon pylons, a hull plate, an engine nacelle
/// and a cockpit dome.
pub const SHIP_SLOTS: &[ShipSlot] = &[
    ShipSlot {
        name: "weapon_R",
        offset: Vec3::new(1.0, 0.0, 0.3),
        base_yaw: 0.0,
        kind: 0,
    },
    ShipSlot {
        name: "weapon_L",
        offset: Vec3::new(-1.0, 0.0, 0.3),
        base_yaw: 0.0,
        kind: 0,
    },
    ShipSlot {
        name: "hull",
        offset: Vec3::new(0.0, 0.0, -0.6),
        base_yaw: std::f32::consts::PI,
        kind: 1,
    },
    ShipSlot {
        name: "engine",
        offset: Vec3::new(0.0, 0.1, -1.4),
        base_yaw: std::f32::consts::PI,
        kind: 2,
    },
    ShipSlot {
        name: "cockpit",
        offset: Vec3::new(0.0, 0.55, 0.4),
        base_yaw: 0.0,
        kind: 1,
    },
];

/// Pure: world placement of a mounted ship module (ship-local space).
pub fn ship_module_transform(slot_index: u32, m: &ShipModule) -> (Vec3, f32) {
    let slot = SHIP_SLOTS
        .get(slot_index as usize)
        .or_else(|| SHIP_SLOTS.first())
        .copied()
        .unwrap_or(ShipSlot {
            name: "?",
            offset: Vec3::ZERO,
            base_yaw: 0.0,
            kind: 1,
        });
    let off = Vec3::new(m.offset[0], m.offset[1], m.offset[2]);
    (slot.offset + off, slot.base_yaw + m.yaw)
}

/// Pure: the loadout's mechanical bonus — (weapon multiplier, extra hull,
/// extra speed). Weapon slots stack damage, hull slots armour, engine slots
/// thrust; each module contributes its scale.
pub fn ship_bonus(build: &ShipBuild) -> (f32, f32, f32) {
    let mut weapon = 1.0f32;
    let mut hull = 0.0f32;
    let mut speed = 0.0f32;
    for m in &build.modules {
        let Some(slot) = SHIP_SLOTS.get(m.slot as usize) else {
            continue;
        };
        let w = m.scale.clamp(0.1, 4.0);
        match slot.kind {
            0 => weapon += 0.25 * w,
            1 => hull += 15.0 * w,
            2 => speed += 1.5 * w,
            _ => {}
        }
    }
    (weapon, hull, speed.min(12.0))
}

/// One tribe member's assignment (role + a mounted original outfit part).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MateAssignment {
    pub role: u32,
    /// Optional outfit model instance mounted on this mate (0 = none).
    /// Legacy arbitrary part IDs remain saved when an outfit cannot resolve.
    #[serde(default)]
    pub part: u32,
    /// Exact TOOL_PARTS PROP definition; old saves retain canonical model lookup.
    #[serde(default)]
    pub outfit_definition: Option<u32>,
}

impl Default for MateAssignment {
    fn default() -> Self {
        Self {
            role: 0,
            part: 0,
            outfit_definition: None,
        }
    }
}

/// Pure: role name by index (safe).
pub fn role_name(i: u32) -> &'static str {
    TRIBE_ROLES
        .get(i as usize)
        .map(|(n, _, _)| *n)
        .unwrap_or("idler")
}

/// Pure: tribe-wide gather multiplier from role counts.
pub fn gather_multiplier(roles: &[u32]) -> f32 {
    1.0 + roles
        .iter()
        .filter_map(|r| TRIBE_ROLES.get(*r as usize))
        .map(|(_, g, _)| g)
        .sum::<f32>()
}

/// Pure: tribe-wide defense multiplier (raid deterrence).
pub fn defense_multiplier(roles: &[u32]) -> f32 {
    1.0 + roles
        .iter()
        .filter_map(|r| TRIBE_ROLES.get(*r as usize))
        .map(|(_, _, d)| d)
        .sum::<f32>()
}

/// The civ vehicle's build: engine level plus its mounted parts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VehicleBuild {
    pub parts: Vec<VehiclePart>,
    #[serde(default)]
    pub focused: u32,
}

impl Default for VehicleBuild {
    fn default() -> Self {
        Self {
            parts: Vec::new(),
            focused: 0,
        }
    }
}

/// A vehicle slot: a named mount point on the land vehicle, in vehicle-local
/// units (the civ flagship is a ~1.6 x 2.6 box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleSlot {
    pub name: &'static str,
    pub offset: Vec3,
    pub base_yaw: f32,
}

/// The civ vehicle's mount points (front engine bay, rear cargo, weapon
/// pylons, cockpit, and the underside belly).
pub const VEHICLE_SLOTS: &[VehicleSlot] = &[
    VehicleSlot {
        name: "nose",
        offset: Vec3::new(0.0, 0.0, 1.5),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "rear",
        offset: Vec3::new(0.0, 0.0, -1.5),
        base_yaw: std::f32::consts::PI,
    },
    VehicleSlot {
        name: "pylon_R",
        offset: Vec3::new(1.0, 0.1, 0.2),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "pylon_L",
        offset: Vec3::new(-1.0, 0.1, 0.2),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "cockpit",
        offset: Vec3::new(0.0, 0.6, 0.5),
        base_yaw: 0.0,
    },
    VehicleSlot {
        name: "belly",
        offset: Vec3::new(0.0, -0.45, 0.0),
        base_yaw: 0.0,
    },
];

/// Pure: world placement of a mounted vehicle part (vehicle-local space).
pub fn vehicle_part_transform(slot_index: u32, p: &VehiclePart) -> (Vec3, f32) {
    let slot = VEHICLE_SLOTS
        .get(slot_index as usize)
        .or_else(|| VEHICLE_SLOTS.first())
        .copied()
        .unwrap_or(VehicleSlot {
            name: "?",
            offset: Vec3::ZERO,
            base_yaw: 0.0,
        });
    let off = Vec3::new(p.offset[0], p.offset[1], p.offset[2]);
    (slot.offset + off, slot.base_yaw + p.yaw)
}

/// Persistent run state: what the original Spore calls the saved creature lineage.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct RunProgress {
    #[serde(default)]
    pub body: crate::body::BodyProfile,
    /// Spendable editor DNA is separate from evolution progress.
    #[serde(default = "default_editor_dna")]
    pub editor_dna: f32,
    #[serde(default)]
    pub cell_parts: Vec<PartInstance>,
    #[serde(default)]
    pub cell_editor_initialized: bool,
    /// Cell part kinds discovered from meteor shards or defeated cells, as in
    /// Spore, where only the starting mouths, eyes and flagella are free.
    #[serde(default)]
    pub cell_found_parts: Vec<String>,
    /// The opening herbivore/carnivore choice has been made for this lineage.
    #[serde(default)]
    pub cell_diet_chosen: bool,
    /// Retail Cell-stage save data (`cCellSerializableData` + world seed and
    /// placement). Missing in older saves; migrated on entry.
    #[serde(default)]
    pub cell: crate::cell_rules::CellSave,
    /// The cell as a Cell Creator creation (species name); parts stay in
    /// `cell_parts`. Missing in older saves.
    #[serde(default)]
    pub cell_creation: crate::cell_editor::CellCreation,
    /// The creature as a Creature Creator creation (species name). Missing
    /// in older saves.
    #[serde(default)]
    pub creature_creation: crate::creature_editor::CreatureCreation,
    /// Creature part definitions (instance IDs) found in bone piles or taken
    /// from other species. Spore's advanced parts are never bought blind.
    #[serde(default)]
    pub creature_found_parts: Vec<u32>,
    /// Bone piles already picked over on this planet.
    #[serde(default)]
    pub creature_bone_piles_taken: Vec<u32>,
    #[serde(default)]
    pub creature_editor_initialized: bool,
    #[serde(default)]
    pub species_world: crate::species::SpeciesWorld,
    #[serde(default)]
    pub creature_pack: crate::creature_pack::CreaturePack,
    #[serde(default)]
    pub creature_lifecycle: crate::creature_lifecycle::CreatureLifecycle,
    #[serde(default)]
    pub creature_progression: crate::creature_progression::CreatureProgression,
    /// One-shot flags of the Creature moments (first-time hints) and the
    /// stage clock.
    #[serde(default)]
    pub creature_flow: crate::creature_flow::CreatureFlowSave,
    /// One-shot cinematics already played in this run (the retail profile
    /// flags of the first-time moments, `cinematics::moments`).
    #[serde(default)]
    pub cinematics_played: Vec<String>,
    #[serde(default)]
    pub tribal_campaign: crate::tribal::TribalCampaign,
    #[serde(default)]
    pub civ_campaign: crate::civ::CivCampaign,
    #[serde(default)]
    pub space_campaign: crate::space::SpaceCampaign,
    /// Last playable-world checkpoint. Missing in saves before 0.41.
    #[serde(default)]
    pub checkpoint: Option<crate::persistence::Checkpoint>,
    /// New-game choices (planet name, difficulty, specialty; `front_end`).
    #[serde(default)]
    pub game_setup: crate::front_end::GameSetup,
    /// Systems request a save; the world is captured after gameplay finishes.
    #[serde(skip)]
    pub save_requested: bool,
    pub stage_order_index: usize,
    pub dna_banked_total: f32,
    pub dna_this_stage: f32,
    pub colonies: u32,
    pub victories: u32,
    pub diet_herbivore: f32,
    pub diet_carnivore: f32,
    pub finished: bool,
    /// Editor accessory: RW4 instance ID of an original part (0 = none).
    /// Legacy single-slot field; superseded by `parts` (kept to migrate old
    /// saves and to keep the creature-stage accessory slot working).
    #[serde(default)]
    pub accessory: u32,
    /// Multi-part creature: every mounted part with its slot bone + tweaks.
    /// Old saves (no `parts`) migrate from the legacy fields on load.
    #[serde(default)]
    pub parts: Vec<PartInstance>,
    /// Stats derived from the assembled creature, carried into later stages.
    /// Old saves (no `stats`) fall back to the base creature.
    #[serde(default)]
    pub stats: Option<CreatureStats>,
    /// Index into `parts` that editor keys currently act on.
    #[serde(default)]
    pub focused_part: u32,
    /// Editor accessory tweaks (persisted).
    #[serde(default)]
    pub accessory_yaw: f32,
    #[serde(default = "default_acc_scale")]
    pub accessory_scale: f32,
    /// Creature slot bone the accessory mounts on (index into the rig).
    #[serde(default)]
    pub accessory_bone: u32,
    /// True once the slot default has been applied (old saves: false).
    #[serde(default)]
    pub accessory_picked: bool,
    /// Spendable spice stockpile (civ economy; old saves start at 0).
    #[serde(default)]
    pub spice: f32,
    /// Engine upgrade level (civ economy).
    #[serde(default)]
    pub engine: u8,
    /// The civ vehicle build (parts + focus); old saves default to empty.
    #[serde(default)]
    pub vehicle: VehicleBuild,
    /// Saved original-part building assembly shared by owned cities.
    #[serde(default)]
    pub building: VehicleBuild,
    /// Tribe member assignments (role + optional part); old saves default to
    /// three gatherers.
    #[serde(default = "default_tribe")]
    pub tribe: Vec<MateAssignment>,
    /// Space ship loadout; old saves default to empty.
    #[serde(default)]
    pub ship: ShipBuild,
    /// Hull upgrade level (space ship; old saves start at 0).
    #[serde(default)]
    pub hull: u8,
    /// Weapon upgrade level (space ship; old saves start at 0).
    #[serde(default)]
    pub weapon: u8,
    /// What the Tribal / Civ / Space outfitters saved (accessories on the
    /// creature); old saves start without.
    #[serde(default)]
    pub outfits: crate::outfitter::OutfitRecords,
    /// What the vehicle, building and UFO editors saved per class (free
    /// transform designs); old saves start without.
    #[serde(default)]
    pub rigid: crate::rigid_editor::RigidRecords,
}

impl Default for RunProgress {
    fn default() -> Self {
        Self {
            body: crate::body::BodyProfile::default(),
            editor_dna: default_editor_dna(),
            cell_parts: Vec::new(),
            cell_editor_initialized: false,
            cell_found_parts: Vec::new(),
            cell_diet_chosen: false,
            cell: crate::cell_rules::CellSave::default(),
            cell_creation: Default::default(),
            creature_creation: Default::default(),
            creature_found_parts: Vec::new(),
            creature_bone_piles_taken: Vec::new(),
            creature_editor_initialized: false,
            species_world: crate::species::SpeciesWorld::default(),
            creature_pack: crate::creature_pack::CreaturePack::default(),
            creature_lifecycle: crate::creature_lifecycle::CreatureLifecycle::default(),
            creature_progression: crate::creature_progression::CreatureProgression::default(),
            creature_flow: crate::creature_flow::CreatureFlowSave::default(),
            cinematics_played: Vec::new(),
            tribal_campaign: crate::tribal::TribalCampaign::default(),
            civ_campaign: crate::civ::CivCampaign::default(),
            space_campaign: crate::space::SpaceCampaign::default(),
            checkpoint: None,
            game_setup: Default::default(),
            save_requested: false,
            stage_order_index: 0,
            dna_banked_total: 0.0,
            dna_this_stage: 0.0,
            colonies: 0,
            victories: 0,
            diet_herbivore: 0.0,
            diet_carnivore: 0.0,
            finished: false,
            accessory: 0,
            parts: Vec::new(),
            focused_part: 0,
            stats: None,
            accessory_yaw: 0.0,
            accessory_scale: 1.0,
            accessory_bone: 0,
            accessory_picked: false,
            spice: 0.0,
            engine: 0,
            vehicle: VehicleBuild::default(),
            building: VehicleBuild::default(),
            tribe: default_tribe(),
            ship: ShipBuild::default(),
            hull: 0,
            weapon: 0,
            outfits: Default::default(),
            rigid: Default::default(),
        }
    }
}

impl RunProgress {
    /// Earned Creature points are permanent brain progress; editor spending
    /// and a regressed legacy counter cannot remove recruited pack capacity.
    pub fn earned_creature_dna(&self) -> f32 {
        self.creature_progression
            .observed_dna
            .max(self.dna_this_stage)
            .max(0.0)
    }
    pub fn stage_order() -> &'static [GameStage] {
        use GameStage::*;
        &[Cell, Creature, Tribal, Civilization, Space]
    }

    pub fn current_stage(&self) -> GameStage {
        Self::stage_order()[self.stage_order_index.min(4)].clone()
    }

    /// Pure function: award DNA, returns true if the stage goal is met.
    pub fn award_dna(&mut self, amount: f32, stage: &GameStage) -> bool {
        if !amount.is_finite() || amount < 0.0 {
            return false;
        }
        self.dna_this_stage += amount;
        self.dna_banked_total += amount;
        if *stage == GameStage::Tribal {
            // The tribe's food store holds `tribeMaxFood` (tribal_rules).
            self.dna_this_stage = self.dna_this_stage.min(crate::tribal_rules::TRIBE_MAX_FOOD);
        }
        if *stage == GameStage::Creature {
            // Two counters (retail 0xD2E8A0): spendable editor DNA is capped at
            // 9999, the stage total (`dna_this_stage`) only ever grows.
            self.editor_dna =
                crate::creature_rules::add_evolution_points(self.editor_dna, 0.0, amount).0;
        } else if *stage == GameStage::Cell {
            self.editor_dna += amount.max(0.0);
        }
        if *stage == GameStage::Creature {
            self.creature_progression.observe_dna(self.dna_this_stage);
        }
        self.dna_this_stage >= tuning_for(stage).dna_to_advance
    }

    /// Advance to the next stage. Space is open-ended: no further stage is
    /// selected and no progress/checkpoint is cleared when already in Space.
    pub fn advance(&mut self) -> Option<GameStage> {
        if self.finished || self.current_stage() == GameStage::Space {
            return None;
        }
        if self.current_stage() == GameStage::Creature {
            crate::tribal_rts::livestock::inherit_pack(self);
        }
        self.dna_this_stage = 0.0;
        self.checkpoint = None;
        self.request_save();
        self.stage_order_index += 1;
        // The Tribal stage starts with `newTribeFood` (tribal_rules).
        self.dna_this_stage = crate::tribal_rules::start_food(&self.current_stage());
        Some(self.current_stage())
    }

    /// Reopen an OpenSpore save stopped by the old five-mission ending.
    /// Historical victory counts and all empire/checkpoint data are retained.
    pub fn resume_legacy_space(&mut self) {
        if self.finished && self.stage_order_index == Self::stage_order().len() - 1 {
            self.finished = false;
            self.request_save();
        }
    }

    pub fn reset_to(&mut self, stage: GameStage) {
        if self.current_stage() == GameStage::Creature && stage == GameStage::Tribal {
            crate::tribal_rts::livestock::inherit_pack(self);
        }
        if stage == GameStage::Creature {
            self.creature_progression = Default::default();
        }
        self.stage_order_index = Self::stage_order()
            .iter()
            .position(|s| *s == stage)
            .unwrap_or(0);
        self.dna_this_stage = crate::tribal_rules::start_food(&stage);
        self.finished = false;
        self.checkpoint = None;
        self.request_save();
    }

    /// Scene construction is also used by Continue; only a different stage
    /// starts a fresh objective. Upgrades and the lineage always carry over.
    pub fn enter_stage(&mut self, stage: GameStage) {
        if self.current_stage() != stage {
            self.reset_to(stage);
        }
    }

    pub fn request_save(&mut self) {
        self.save_requested = true;
    }

    /// Use the menu's edited creature as the template for a fresh lineage.
    pub fn new_lineage(&self) -> Self {
        let mut fresh = Self {
            body: self.body.clone(),
            parts: self.parts.clone(),
            creature_editor_initialized: self.creature_editor_initialized,
            focused_part: self.focused_part,
            accessory_picked: self.accessory_picked,
            ..Self::default()
        };
        fresh.sync_legacy_accessory();
        fresh.request_save();
        fresh
    }

    pub fn save_path() -> PathBuf {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            if arg == "--save-path" {
                if let Some(path) = args.next() {
                    return PathBuf::from(path);
                }
            }
        }
        // %APPDATA%\OpenSpore\saves\, or an existing save next to the exe, or
        // portable mode (see user_paths).
        crate::user_paths::UserPaths::from_process().default_save_path()
    }

    pub fn load() -> Self {
        let explicit = std::env::args().any(|a| a == "--save-path");
        let mut paths = vec![Self::save_path()];
        if !explicit && paths[0] != PathBuf::from(SAVE_FILE) {
            paths.push(PathBuf::from(SAVE_FILE));
        }
        for p in paths {
            match crate::persistence::load_from(&p) {
                Ok(run) => return run,
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    warn!("Cannot load save {} or its backup: {error}", p.display());
                }
                Err(_) => {}
            }
        }
        Self::default()
    }

    /// Backward compatibility: old saves carried one accessory in `accessory`
    /// (+ `accessory_bone`/`accessory_yaw`/`accessory_scale`). Fold it into
    /// the part list so multi-part editing is a pure superset. Idempotent.
    pub fn migrate_parts(&mut self) {
        if self.parts.is_empty() && self.accessory != 0 {
            self.parts.push(PartInstance {
                instance: self.accessory,
                definition_id: None,
                bone: self.accessory_bone,
                yaw: self.accessory_yaw,
                scale: self.accessory_scale,
                offset: default_part_offset(),
                pair_id: 0,
                mirrored: false,
                rigid: None,
                edits: Default::default(),
            });
            self.focused_part = 0;
        }
        if self.focused_part as usize >= self.parts.len() {
            self.focused_part = 0;
        }
    }

    /// Add a part (dedupe: re-mounting the same instance refocuses it).
    /// Returns the index of the focused part.
    pub fn part_add(&mut self, instance: u32, bone: u32) -> usize {
        if let Some(i) = self.parts.iter().position(|p| p.instance == instance) {
            self.focused_part = i as u32;
            return i;
        }
        self.parts.push(PartInstance::new(instance, bone));
        self.focused_part = (self.parts.len() - 1) as u32;
        self.parts.len() - 1
    }

    /// Remove the focused part; returns its index (list length after removal).
    pub fn part_remove_focused(&mut self) -> usize {
        let i = self.focused_part as usize;
        if i < self.parts.len() {
            self.parts.remove(i);
            if self.focused_part as usize >= self.parts.len() {
                self.focused_part = self.parts.len().saturating_sub(1) as u32;
            }
        }
        self.parts.len()
    }

    /// Mirror the part list back into the legacy single-slot field (the
    /// creature-stage accessory and older readers use it for the first part).
    pub fn sync_legacy_accessory(&mut self) {
        self.accessory = self.parts.first().map(|p| p.instance).unwrap_or(0);
        if let Some(p) = self.parts.first() {
            self.accessory_bone = p.bone;
            self.accessory_yaw = p.yaw;
            self.accessory_scale = p.scale;
        }
    }

    /// The carried creature stats (base creature for old saves).
    pub fn creature_stats(&self) -> CreatureStats {
        self.stats.unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Shared components (all stages move on the XZ plane, Y up)
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Mover {
    pub speed: f32,
}

#[derive(Component)]
pub struct Health {
    pub hp: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { hp: max, max }
    }
    pub fn hurt(&mut self, dmg: f32) {
        self.hp = (self.hp - dmg).max(0.0);
    }
    pub fn heal(&mut self, amount: f32) {
        self.hp = (self.hp + amount).min(self.max);
    }
    pub fn dead(&self) -> bool {
        self.hp <= 0.0
    }
    pub fn frac(&self) -> f32 {
        (self.hp / self.max).clamp(0.0, 1.0)
    }
}

#[derive(Component)]
pub struct Hunger {
    pub food: f32,
    pub max: f32,
    /// Seconds the Creature stage has held hunger at its maximum
    /// (`hungerDecayDelay`); see `creature_rules::hunger_tick`.
    pub hold: f32,
}

impl Hunger {
    pub fn new(max: f32) -> Self {
        Self {
            food: max,
            max,
            hold: 0.0,
        }
    }
    /// The retail hunger update (0xC0D5A0): held at the maximum for `delay`
    /// seconds, then drained by `drain` per second.
    pub fn tick_retail(&mut self, dt: f32, drain: f32, delay: f32) {
        let (food, hold) =
            crate::creature_rules::hunger_tick(self.food, self.max, self.hold, dt, drain, delay);
        self.food = food;
        self.hold = hold;
    }
    pub fn drain(&mut self, dt: f32, per_sec: f32) {
        self.food = (self.food - per_sec * dt).max(0.0);
    }
    pub fn eat(&mut self, amount: f32) {
        self.food = (self.food + amount).min(self.max);
    }
    pub fn starving(&self) -> bool {
        self.food <= 0.01
    }
    pub fn frac(&self) -> f32 {
        (self.food / self.max).clamp(0.0, 1.0)
    }
}

#[derive(Component)]
pub struct Edible {
    pub nutrition: f32,
    pub dna: f32,
    pub meat: bool,
}

#[derive(Component)]
pub struct Drifter {
    pub vel: Vec3,
}

#[derive(Component)]
pub struct Predator {
    pub damage: f32,
    pub speed: f32,
}

#[derive(Component)]
pub struct TribeMate;

#[derive(Component)]
pub struct Bush {
    pub food_left: f32,
}

#[derive(Component)]
pub struct SpiceNode {
    pub spice_left: f32,
}

#[derive(Component)]
pub struct Vehicle;

#[derive(Component)]
pub struct Planet {
    pub radius: f32,
    pub colonized: bool,
    pub name: String,
    /// Surveyed worlds have been scanned for spice (space stage).
    pub surveyed: bool,
}

#[derive(Component)]
pub struct Npc {
    pub speed: f32,
    pub hostile: bool,
    pub hp: f32,
}

/// Top-down WASD movement helper shared by all stages.
pub fn move_player_on_plane(
    keys: &ButtonInput<KeyCode>,
    transform: &mut Transform,
    speed: f32,
    dt: f32,
    bounds: f32,
) {
    move_player_relative(keys, transform, speed, dt, bounds, Vec3::NEG_Z, Vec3::X);
}

/// Plane movement in an orthonormal camera basis. Facing remains aligned with
/// the actual world-space travel direction, including after an orbit.
pub fn move_player_relative(
    keys: &ButtonInput<KeyCode>,
    transform: &mut Transform,
    speed: f32,
    dt: f32,
    bounds: f32,
    forward: Vec3,
    right: Vec3,
) {
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir -= forward;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir += right;
    }
    if dir.length_squared() > 0.0 {
        dir = dir.normalize();
        transform.translation += dir * speed * dt;
        transform.translation.x = transform.translation.x.clamp(-bounds, bounds);
        transform.translation.z = transform.translation.z.clamp(-bounds, bounds);
        // Face travel direction.
        let yaw = (-dir.x).atan2(-dir.z);
        transform.rotation = Quat::from_rotation_y(yaw);
    }
}

pub fn dist_xz(a: Vec3, b: Vec3) -> f32 {
    ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Editor part placement: world offset for an avatar at `pos` facing `yaw`,
/// with the part's bone-space slide and user yaw. Pure for testability.
/// The slide is applied *before* the yaw rotation (bone-space slide).
pub fn accessory_offset(yaw: f32, part_yaw: f32, base: Vec3, offset: Vec3) -> (Vec3, f32) {
    let (s, c) = yaw.sin_cos();
    let local = base + offset;
    let rotated = Vec3::new(
        local.x * c - local.z * s,
        local.y,
        local.x * s + local.z * c,
    );
    (rotated, yaw + part_yaw)
}

/// Cycle index over `[0..n]` options with wrap (0 = none). Pure.
pub fn cycle_index(cur: i32, dir: i32, n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    (cur + dir).rem_euclid(n + 1)
}

/// Civ engine economy: upgrade cost grows per level (max level 4).
pub fn engine_cost(level: u8) -> f32 {
    50.0 * (level as f32 + 1.0)
}

pub const ENGINE_MAX: u8 = 4;
pub const ENGINE_BASE_SPEED: f32 = 11.0;
pub const ENGINE_SPEED_PER_LEVEL: f32 = 2.5;

/// Pure purchase: (spice, level) -> new (spice, level) or None.
pub fn try_buy_engine(spice: f32, level: u8) -> Option<(f32, u8)> {
    if level >= ENGINE_MAX {
        return None;
    }
    let cost = engine_cost(level);
    if spice < cost {
        return None;
    }
    Some((spice - cost, level + 1))
}

pub fn engine_speed(level: u8) -> f32 {
    ENGINE_BASE_SPEED + ENGINE_SPEED_PER_LEVEL * level as f32
}

/// Flee target directly away from a threat (pure; None when far).
pub fn flee_target(npc: Vec3, threat: Vec3, bounds: f32) -> Option<(f32, f32)> {
    let dx = npc.x - threat.x;
    let dz = npc.z - threat.z;
    let d = (dx * dx + dz * dz).sqrt();
    if d > 6.0 || d < 0.01 {
        return None;
    }
    Some((
        (npc.x + dx / d * 8.0).clamp(-bounds, bounds),
        (npc.z + dz / d * 8.0).clamp(-bounds, bounds),
    ))
}

// ---------------------------------------------------------------------------
// Space-stage economy: surveying, hull and weapon upgrades
// ---------------------------------------------------------------------------

pub const HULL_MAX: u8 = 4;
pub const WEAPON_MAX: u8 = 4;
pub const HULL_BASE: f32 = 200.0;
pub const HULL_PER_LEVEL: f32 = 75.0;
pub const WEAPON_BASE: f32 = 10.0;
pub const WEAPON_PER_LEVEL: f32 = 5.0;

/// Maximum hull HP at hull level.
pub fn hull_max_hp(level: u8) -> f32 {
    HULL_BASE + HULL_PER_LEVEL * level as f32
}

/// Hull upgrade price (grows per level).
pub fn hull_cost(level: u8) -> f32 {
    60.0 * (level as f32 + 1.0)
}

/// Pure purchase: (spice, level) -> new (spice, level) or None.
pub fn try_buy_hull(spice: f32, level: u8) -> Option<(f32, u8)> {
    if level >= HULL_MAX {
        return None;
    }
    let cost = hull_cost(level);
    if spice < cost {
        return None;
    }
    Some((spice - cost, level + 1))
}

/// Beam damage per shot at weapon level.
pub fn weapon_damage(level: u8) -> f32 {
    WEAPON_BASE + WEAPON_PER_LEVEL * level as f32
}

/// Weapon upgrade price (grows per level).
pub fn weapon_cost(level: u8) -> f32 {
    70.0 * (level as f32 + 1.0)
}

/// Pure purchase: (spice, level) -> new (spice, level) or None.
pub fn try_buy_weapon(spice: f32, level: u8) -> Option<(f32, u8)> {
    if level >= WEAPON_MAX {
        return None;
    }
    let cost = weapon_cost(level);
    if spice < cost {
        return None;
    }
    Some((spice - cost, level + 1))
}

/// Survey payout: bigger worlds hold more spice (pure).
pub fn survey_reward(radius: f32) -> f32 {
    (8.0 + radius * 2.0).clamp(8.0, 40.0)
}

/// Pirates break off the attack when badly damaged (pure).
pub fn pirate_flees(hp: f32, max: f32) -> bool {
    max > 0.0 && hp / max < 0.3
}

/// Aim direction from `from` to `to` (None when coincident). Pure.
pub fn aim_dir(from: Vec3, to: Vec3) -> Option<Vec3> {
    let d = to - from;
    (d.length_squared() > 1e-6).then(|| d.normalize())
}

/// Pure: clamp a focus index into `[0, len)` (0 when empty).
pub fn focus_index(cur: u32, len: usize) -> u32 {
    if len == 0 {
        0
    } else {
        cur.min(len as u32 - 1)
    }
}

/// Pure: rotate (radians) wrapping to `[0, 2π)`.
pub fn wrap_yaw(yaw: f32) -> f32 {
    yaw.rem_euclid(std::f32::consts::TAU)
}

/// Pure: multiply-then-clamp scale into `[0.1, 4.0]`.
pub fn scaled(scale: f32, factor: f32) -> f32 {
    (scale * factor).clamp(0.1, 4.0)
}

/// Pure: walk a bone index over `[0, n)` with wrap (0 when no rig).
pub fn cycle_bone(cur: u32, dir: i32, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    (cur as i32 + dir).rem_euclid(n as i32) as u32
}

/// Spore's 32-bit hash (FNV-1, lowercase-first), transcribed from the
/// documented modding-tool implementation. Used pervasively for bone
/// names, channel IDs, and resource keys.
pub fn spore_hash(name: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for c in name.to_lowercase().chars() {
        h = h.wrapping_mul(0x0100_0193);
        h ^= c as u32;
    }
    h
}

/// Format a hash the way the game tools do.
pub fn hash_label(h: u32) -> String {
    format!("{h:#010X}")
}

/// Bone-name hashes resolved by wordlist attack against the retail 57-bone
/// creature rig (Spore_Graphics 0xC4F96704). Names follow `word`, `word_N`
/// and `word_word_N` conventions; part rigs use `joint0..19`.
pub fn known_bone_name(h: u32) -> Option<&'static str> {
    Some(match h {
        0x3286_0993 => "global_NODE",
        0x7BD0_5F71 => "root",
        0x2E86_47BD => "jaw",
        0x4194_5228 => "neck",
        0x8B67_25F9 => "shoulder",
        0x9571_DD87 => "upper_spine_1",
        0x9571_DD84 => "upper_spine_2",
        0x9571_DD85 => "upper_spine_3",
        0x5384_5ED6 => "lower_spine_1",
        0x5384_5ED7 => "lower_spine_2",
        0x5384_5ED4 => "lower_spine_3",
        0x8933_2E8F => "tail_1",
        0x8933_2E8C => "tail_2",
        0x8933_2E8D => "tail_3",
        0xBB89_24E1 => "head_base",
        0x944E_C723 => "head_top",
        0xA110_8203 => "head_front",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outfits_keep_legacy_model_ids_and_save_exact_definition_identity() {
        let old: MateAssignment = serde_json::from_str(r#"{"role":2,"part":123}"#).unwrap();
        assert_eq!(old.part, 123);
        assert_eq!(old.outfit_definition, None);
        let exact = MateAssignment {
            outfit_definition: Some(456),
            ..old
        };
        let restored: MateAssignment =
            serde_json::from_str(&serde_json::to_string(&exact).unwrap()).unwrap();
        assert_eq!(restored.part, 123);
        assert_eq!(restored.outfit_definition, Some(456));
        assert_eq!(restored.role, 2);
    }

    #[test]
    fn stats_couple_to_parts() {
        let scale_of = |_: u32| Some(1.0);
        // No parts: the base creature.
        assert_eq!(creature_stats(&[], scale_of), CreatureStats::default());
        // Each part costs speed and buys health/social/size.
        let one = creature_stats(&[PartInstance::new(1, 0)], scale_of);
        assert!(one.max_hp > 120.0 && one.social > 1.0 && one.size > 1.0);
        assert!(one.speed < 8.0);
        let many: Vec<PartInstance> = (0..40).map(|i| PartInstance::new(i, 0)).collect();
        let m = creature_stats(&many, scale_of);
        // Bounded: 40 parts can't break the creature.
        assert_eq!(m.speed, 4.5);
        assert_eq!(m.max_hp, 400.0);
        assert_eq!(m.social, 2.5);
        assert_eq!(m.size, 1.6);
        // Bigger parts move the numbers more.
        let big = creature_stats(&[PartInstance::new(1, 0)], |_: u32| Some(10.0));
        assert!(big.max_hp - 120.0 > one.max_hp - 120.0);
        assert!(big.speed < one.speed);
        // Unknown parts still count (fallback scale 1).
        let unknown = creature_stats(&[PartInstance::new(999, 0)], |_: u32| None);
        assert_eq!(
            unknown,
            creature_stats(&[PartInstance::new(1, 0)], scale_of)
        );
    }

    #[test]
    fn slot_and_part_semantics() {
        // Off-midline bones are limbs regardless of height.
        assert_eq!(slot_kind(Vec3::new(0.9, 0.8, 0.2)), SlotKind::Limb);
        // Midline classification by height / front-back.
        assert_eq!(slot_kind(Vec3::new(0.0, 1.0, 0.0)), SlotKind::Head);
        assert_eq!(slot_kind(Vec3::new(0.0, 0.0, -1.0)), SlotKind::Tail);
        assert_eq!(slot_kind(Vec3::new(0.0, 0.0, 0.0)), SlotKind::Root);
        // Just inside the midline band stays spine/root.
        assert_eq!(slot_kind(Vec3::new(0.1, 0.2, 0.1)), SlotKind::Root);
        // Part families come from rig size.
        assert_eq!(part_family(15), "limb");
        assert_eq!(part_family(8), "detail");
        assert_eq!(part_family(2), "tiny");
    }

    #[test]
    fn part_list_and_migration() {
        // Old-save migration folds the legacy slot into the part list once.
        let mut run = RunProgress::default();
        run.accessory = 0x1234_5678;
        run.accessory_bone = 5;
        run.accessory_yaw = 1.0;
        run.accessory_scale = 2.0;
        run.migrate_parts();
        assert_eq!(run.parts.len(), 1);
        assert_eq!(run.parts[0].instance, 0x1234_5678);
        assert_eq!(run.parts[0].bone, 5);
        assert_eq!(run.parts[0].yaw, 1.0);
        assert_eq!(run.parts[0].scale, 2.0);
        // Idempotent.
        run.migrate_parts();
        assert_eq!(run.parts.len(), 1);
        // Add is deduped by instance and refocuses.
        run.part_add(0xAAAA, 1);
        run.part_add(0xBBBB, 2);
        assert_eq!(run.parts.len(), 3);
        run.focused_part = 0;
        assert_eq!(run.part_add(0xBBBB, 9), 2); // refocus existing
                                                // Remove the focused part; focus stays in range.
        run.part_remove_focused();
        assert_eq!(run.parts.len(), 2);
        run.focused_part = 99;
        run.migrate_parts();
        assert!(run.focused_part as usize <= run.parts.len().saturating_sub(1));
        // Legacy mirror for the creature-stage accessory.
        run.sync_legacy_accessory();
        assert_eq!(run.accessory, run.parts[0].instance);
    }

    #[test]
    fn editor_math() {
        assert_eq!(focus_index(7, 0), 0);
        assert_eq!(focus_index(7, 3), 2);
        assert!((wrap_yaw(-1.0) - (std::f32::consts::TAU - 1.0)).abs() < 1e-5);
        assert!((wrap_yaw(std::f32::consts::TAU + 0.5) - 0.5).abs() < 1e-5);
        assert_eq!(scaled(1.0, 0.5), 0.5);
        assert_eq!(scaled(0.1, 0.5), 0.1); // clamped low
        assert_eq!(scaled(4.0, 2.0), 4.0); // clamped high
        assert_eq!(cycle_bone(0, 1, 57), 1);
        assert_eq!(cycle_bone(56, 1, 57), 0);
        assert_eq!(cycle_bone(0, -1, 57), 56);
        assert_eq!(cycle_bone(9, 1, 0), 0); // no rig
    }

    #[test]
    fn old_save_json_migrates() {
        // A v0.16-era save: no `parts` field at all, legacy accessory set.
        let old = r#"{
            "stage_order_index": 2,
            "dna_banked_total": 10.0,
            "dna_this_stage": 5.0,
            "colonies": 0,
            "victories": 0,
            "diet_herbivore": 0.0,
            "diet_carnivore": 0.0,
            "finished": false,
            "accessory": 305419896,
            "accessory_yaw": 0.7853982,
            "accessory_scale": 1.2,
            "spice": 0.0,
            "engine": 0,
            "hull": 0,
            "weapon": 0,
            "accessory_bone": 39,
            "accessory_picked": true
        }"#;
        let mut loaded: RunProgress = serde_json::from_str(old).expect("parse old save");
        loaded.migrate_parts();
        assert_eq!(loaded.parts.len(), 1);
        assert_eq!(loaded.parts[0].instance, 305419896);
        assert_eq!(loaded.parts[0].bone, 39);
        // Offset field didn't exist back then: defaults to zero.
        assert_eq!(loaded.parts[0].offset, [0.0, 0.0, 0.0]);
        assert!((loaded.parts[0].yaw - 0.7853982).abs() < 1e-6);
        assert!((loaded.parts[0].scale - 1.2).abs() < 1e-6);
        // A rounded save keeps the part list and the legacy mirror agrees.
        loaded.sync_legacy_accessory();
        let round = serde_json::to_string(&loaded).expect("write");
        let again: RunProgress = serde_json::from_str(&round).expect("reparse");
        assert_eq!(again.parts, loaded.parts);
        assert_eq!(again.accessory, loaded.accessory);
    }

    #[test]
    fn stats_carry_through_save() {
        // Old saves have no stats field: the base creature is used.
        let old = r#"{"stage_order_index":3,"dna_banked_total":0.0,"dna_this_stage":0.0,
            "colonies":0,"victories":0,"diet_herbivore":0.0,"diet_carnivore":0.0,
            "finished":false,"accessory":0}"#;
        let loaded: RunProgress = serde_json::from_str(old).expect("parse");
        assert_eq!(loaded.stats, None);
        assert_eq!(loaded.creature_stats(), CreatureStats::default());

        // Stats set in the creature stage survive a save round-trip and are
        // what later stages read.
        let mut run = RunProgress::default();
        run.parts = vec![PartInstance::new(1, 0), PartInstance::new(2, 3)];
        let stats = creature_stats(&run.parts, |_: u32| Some(1.0));
        run.stats = Some(stats);
        let text = serde_json::to_string(&run).expect("write");
        let again: RunProgress = serde_json::from_str(&text).expect("reparse");
        assert_eq!(again.creature_stats(), stats);
        assert!(again.creature_stats().max_hp > 120.0);
    }

    #[test]
    fn slide_keys_move_one_axis() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut p = PartInstance::new(1, 0);
        assert!(!apply_slide_keys(&keys, &mut p));
        keys.press(KeyCode::KeyI);
        keys.press(KeyCode::KeyU);
        assert!(apply_slide_keys(&keys, &mut p));
        assert!((p.offset[0] - OFFSET_STEP).abs() < 1e-6);
        assert!((p.offset[1] - OFFSET_STEP).abs() < 1e-6);
        assert_eq!(p.offset[2], 0.0);
        // Clamped at the editor limit (fresh input each tick: `just_pressed`
        // only fires once per press).
        for _ in 0..60 {
            let mut k = ButtonInput::<KeyCode>::default();
            k.press(KeyCode::KeyI);
            apply_slide_keys(&k, &mut p);
        }
        assert_eq!(p.offset[0], OFFSET_LIMIT);
    }

    #[test]
    fn mirror_math() {
        // Sagittal pairs + a midline bone.
        let m = crate::rw4::mirror_map(
            &[
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ], // midline x=0
                [
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 0.5],
                    [0.0, 0.0, 1.0, 0.0],
                ], // right x=1
                [
                    [1.0, 0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0, 0.5],
                    [0.0, 0.0, 1.0, 0.0],
                ], // left x=-1
                [
                    [1.0, 0.0, 0.0, 3.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ], // unpaired
            ],
            0.25,
        );
        assert_eq!(m[0], Some(0)); // midline maps to itself
        assert_eq!(m[1], Some(2)); // right <-> left
        assert_eq!(m[2], Some(1));
        assert_eq!(m[3], None); // no pair
                                // Part mirroring negates bone-space x and yaw; instance/scale carry.
        let p = PartInstance {
            instance: 7,
            bone: 1,
            yaw: 0.5,
            scale: 1.5,
            offset: [0.3, 0.2, -0.1],
            ..PartInstance::new(7, 1)
        };
        let m2 = mirror_part(&p, &m);
        assert_eq!(m2.bone, 2);
        assert_eq!(m2.instance, 7);
        assert_eq!(m2.scale, 1.5);
        assert!((m2.yaw + 0.5).abs() < 1e-6);
        assert_eq!(m2.offset, [-0.3, 0.2, -0.1]);
        // Add-mirrored updates an existing part on the paired bone, else adds.
        let mut parts = vec![PartInstance::new(7, 1)];
        assert!(part_add_mirrored(&mut parts, 0, &m));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].bone, 2);
        part_add_mirrored(&mut parts, 0, &m);
        assert_eq!(parts.len(), 2); // updated in place
                                    // Asymmetric rig / midline slot: centreline mirror on the SAME bone,
                                    // negating the bone-space X offset and the yaw.
        let flat: Vec<Option<usize>> = vec![None; 4]; // no pairs at all
        let mut q = vec![PartInstance {
            instance: 9,
            bone: 3,
            yaw: 0.25,
            scale: 1.0,
            offset: [0.4, 0.1, 0.0],
            ..PartInstance::new(9, 3)
        }];
        assert!(part_add_mirrored(&mut q, 0, &flat));
        assert_eq!(q.len(), 2);
        assert_eq!(q[1].bone, 3);
        assert_eq!(q[1].offset, [-0.4, 0.1, 0.0]);
        assert!((q[1].yaw + 0.25).abs() < 1e-6);
        part_add_mirrored(&mut q, 0, &flat);
        assert_eq!(q.len(), 2); // second press updates the mirror in place
    }

    #[test]
    fn ship_loadout_slots_and_bonus() {
        assert_eq!(SHIP_SLOTS.len(), 5);
        let m = ShipModule::new(0xBBBB, 0);
        let (off, yaw) = ship_module_transform(0, &m);
        assert_eq!(off, Vec3::new(1.0, 0.0, 0.3));
        assert_eq!(yaw, 0.0);
        let m2 = ShipModule {
            instance: 1,
            definition_id: None,
            slot: 3,
            yaw: 0.0,
            scale: 2.0,
            offset: [0.0, 0.5, 0.0],
            block: None,
        };
        let (off2, _) = ship_module_transform(3, &m2);
        assert_eq!(off2, Vec3::new(0.0, 0.6, -1.4));
        // Out-of-range slot falls back to the first.
        let (off3, _) = ship_module_transform(42, &m);
        assert_eq!(off3, Vec3::new(1.0, 0.0, 0.3));

        // Bonuses stack by slot kind; the engine bonus is capped.
        let mut build = ShipBuild::default();
        build.modules.push(ShipModule::new(1, 0)); // weapon_R, scale 1
        build.modules.push(ShipModule::new(2, 2)); // hull, scale 1
        build.modules.push(ShipModule::new(3, 3)); // engine, scale 1
        let (weapon, hull, speed) = ship_bonus(&build);
        assert!((weapon - 1.25).abs() < 1e-6);
        assert!((hull - 15.0).abs() < 1e-6);
        assert!((speed - 1.5).abs() < 1e-6);
        // Many engines cap the speed bonus.
        let mut big = ShipBuild::default();
        for i in 0..20 {
            big.modules.push(ShipModule::new(100 + i, 3));
        }
        let (_, _, capped) = ship_bonus(&big);
        assert_eq!(capped, 12.0);

        // Old saves without a ship field default to empty; round-trips keep it.
        let old = r#"{"stage_order_index":4,"dna_banked_total":0.0,"dna_this_stage":0.0,
            "colonies":0,"victories":0,"diet_herbivore":0.0,"diet_carnivore":0.0,
            "finished":false,"accessory":0}"#;
        let loaded: RunProgress = serde_json::from_str(old).expect("parse");
        assert_eq!(loaded.ship.modules.len(), 0);
        assert_eq!(ship_bonus(&loaded.ship), (1.0, 0.0, 0.0));
        let mut run = RunProgress::default();
        run.ship.modules.push(ShipModule::new(7, 1));
        let text = serde_json::to_string(&run).expect("write");
        let again: RunProgress = serde_json::from_str(&text).expect("reparse");
        assert_eq!(again.ship.modules[0].slot, 1);
    }

    #[test]
    fn tribe_roles_and_multipliers() {
        assert_eq!(role_name(0), "gatherer");
        assert_eq!(role_name(3), "idler");
        assert_eq!(role_name(99), "idler");
        // Multipliers are additive per role.
        let roles = [0, 0, 1, 2];
        assert!((gather_multiplier(&roles) - 1.35).abs() < 1e-6);
        assert!((defense_multiplier(&roles) - 1.4).abs() < 1e-6);
        // Empty tribe: neutral.
        assert_eq!(gather_multiplier(&[]), 1.0);
        assert_eq!(defense_multiplier(&[]), 1.0);
        // Default tribe is three gatherers.
        let d = default_tribe();
        assert_eq!(d.len(), 3);
        assert!(d.iter().all(|m| m.role == 0));
    }

    #[test]
    fn vehicle_build_and_slots() {
        // Six named slots; transforms compose slot + user offset, yaw adds.
        assert_eq!(VEHICLE_SLOTS.len(), 6);
        let p = VehiclePart::new(0xAAAA, 0);
        let (off, yaw) = vehicle_part_transform(0, &p);
        assert_eq!(off, Vec3::new(0.0, 0.0, 1.5));
        assert_eq!(yaw, 0.0);
        let p2 = VehiclePart {
            instance: 1,
            definition_id: None,
            slot: 3,
            yaw: 0.5,
            scale: 1.0,
            offset: [0.25, 0.0, 0.0],
            block: None,
        };
        let (off2, yaw2) = vehicle_part_transform(3, &p2);
        assert_eq!(off2, Vec3::new(-0.75, 0.1, 0.2));
        assert!((yaw2 - 0.5).abs() < 1e-6);
        // Out-of-range slots fall back to the first slot.
        let (off3, _) = vehicle_part_transform(99, &p);
        assert_eq!(off3, Vec3::new(0.0, 0.0, 1.5));

        // Save round-trip with an old save that has no vehicle field.
        let old = r#"{"stage_order_index":3,"dna_banked_total":0.0,"dna_this_stage":0.0,
            "colonies":0,"victories":0,"diet_herbivore":0.0,"diet_carnivore":0.0,
            "finished":false,"accessory":0}"#;
        let loaded: RunProgress = serde_json::from_str(old).expect("parse");
        assert_eq!(loaded.vehicle.parts.len(), 0);
        assert!(loaded.building.parts.is_empty());
        let mut run = RunProgress::default();
        run.vehicle.parts.push(VehiclePart::new(7, 2));
        let mut wall = VehiclePart::new(19, 1);
        wall.offset = [1.5, 2., -0.5];
        wall.yaw = 0.5;
        wall.scale = 0.75;
        run.building.parts.push(wall);
        let text = serde_json::to_string(&run).expect("write");
        let again: RunProgress = serde_json::from_str(&text).expect("reparse");
        assert_eq!(again.vehicle.parts[0].instance, 7);
        assert_eq!(again.vehicle.parts[0].slot, 2);
        assert_eq!(again.building, run.building);
    }

    #[test]
    fn creature_dna_keeps_two_counters_the_spendable_one_capped() {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        run.editor_dna = 9950.0;
        assert!(!run.award_dna(100.0, &GameStage::Creature));
        assert_eq!(run.editor_dna, 9999.0, "spendable DNA caps at 9999");
        assert_eq!(run.dna_this_stage, 100.0, "the stage total is not capped");
        // Spending lowers only the spendable pool; the total never drops.
        run.editor_dna = crate::creature_rules::spend_dna(run.editor_dna, 500.0);
        assert_eq!(run.editor_dna, 9499.0);
        assert_eq!(run.dna_this_stage, 100.0);
        // The stage ends at the last brain level's total (1000), not 400.
        assert!(!run.award_dna(899.0, &GameStage::Creature));
        assert!(run.award_dna(1.0, &GameStage::Creature));
        assert_eq!(tuning_for(&GameStage::Creature).dna_to_advance, 1000.0);
        assert_eq!(run.creature_progression.brain_level, 4);
        // Negative amounts are rejected outright.
        assert!(!run.award_dna(-5.0, &GameStage::Creature));
        assert_eq!(run.dna_this_stage, 1000.0);
    }

    #[test]
    fn cell_progression_requires_100_dna() {
        let mut run = RunProgress::default();
        assert!(!run.award_dna(99.0, &GameStage::Cell));
        assert!(run.award_dna(1.0, &GameStage::Cell));
        assert_eq!(run.advance(), Some(GameStage::Creature));
    }

    #[test]
    fn full_arc_reaches_open_ended_space() {
        let mut run = RunProgress::default();
        let order = RunProgress::stage_order().to_vec();
        for stage in &order[..4] {
            let need = tuning_for(stage).dna_to_advance;
            assert!(run.award_dna(need, stage));
            run.advance();
        }
        // Extra advancement attempts cannot finish or erase the empire.
        run.colonies = 5;
        run.stage_order_index = 4;
        assert_eq!(run.advance(), None);
        assert!(!run.finished);
        assert_eq!(run.victories, 0);
        assert_eq!(run.current_stage(), GameStage::Space);
        assert_eq!(run.colonies, 5);
    }

    #[test]
    fn save_roundtrip() {
        let mut run = RunProgress::default();
        run.dna_banked_total = 42.5;
        run.colonies = 3;
        run.accessory = 0x12345678;
        let text = serde_json::to_string(&run).unwrap();
        let back: RunProgress = serde_json::from_str(&text).unwrap();
        assert_eq!(back.dna_banked_total, 42.5);
        assert_eq!(back.colonies, 3);
        assert_eq!(back.accessory, 0x12345678);
        // Old saves without the field still load.
        let old = r#"{"stage_order_index":0,"dna_banked_total":0.0,"dna_this_stage":0.0,"colonies":0,"victories":0,"diet_herbivore":0.0,"diet_carnivore":0.0,"finished":false}"#;
        let back2: RunProgress = serde_json::from_str(old).unwrap();
        assert_eq!(back2.accessory, 0);
    }

    #[test]
    fn health_math() {
        let mut h = Health::new(100.0);
        h.hurt(30.0);
        assert!((h.frac() - 0.7).abs() < 1e-5);
        h.heal(50.0);
        assert_eq!(h.hp, 100.0);
        h.hurt(100.0);
        assert!(h.dead());
    }

    #[test]
    fn accessory_helpers() {
        assert_eq!(cycle_index(0, 1, 3), 1);
        assert_eq!(cycle_index(3, 1, 3), 0);
        assert_eq!(cycle_index(0, -1, 3), 3);
        assert_eq!(cycle_index(5, 1, 0), 0);
        let (off, yaw) = accessory_offset(0.0, 0.0, Vec3::new(0.0, 1.9, -0.6), Vec3::ZERO);
        assert_eq!(off, Vec3::new(0.0, 1.9, -0.6));
        assert_eq!(yaw, 0.0);
        // Facing +90° yaw rotates the offset rigidly.
        let (off2, _) = accessory_offset(
            std::f32::consts::FRAC_PI_2,
            0.0,
            Vec3::new(0.0, 1.9, -0.6),
            Vec3::ZERO,
        );
        assert!((off2.x - 0.6).abs() < 1e-5 && off2.z.abs() < 1e-5);
        // A bone-space slide moves the part before the facing rotation.
        let (off3, _) = accessory_offset(
            0.0,
            0.0,
            Vec3::new(0.0, 1.9, -0.6),
            Vec3::new(0.25, 0.0, 0.0),
        );
        assert!((off3.x - 0.25).abs() < 1e-5);
        // Offset clamps to a sane editor range.
        assert_eq!(clamp_offset([99.0, -99.0, 0.5], 2.0), [2.0, -2.0, 0.5]);
        // Shifting moves one axis only.
        let mut p = PartInstance::new(1, 2);
        p.shift_offset(0, 0.5);
        assert_eq!(p.offset, [0.5, 0.0, 0.0]);
        p.shift_offset(9, 5.0); // out of range: ignored
        assert_eq!(p.offset, [0.5, 0.0, 0.0]);
    }

    #[test]
    fn engine_economy() {
        assert_eq!(engine_cost(0), 50.0);
        assert_eq!(engine_cost(3), 200.0);
        assert_eq!(engine_speed(0), 11.0);
        assert_eq!(engine_speed(2), 16.0);
        assert_eq!(try_buy_engine(49.0, 0), None);
        assert_eq!(try_buy_engine(50.0, 0), Some((0.0, 1)));
        assert_eq!(try_buy_engine(1000.0, ENGINE_MAX), None);
    }

    #[test]
    fn flee_math() {
        // Close threat -> flee target 8 units away, clamped.
        let t = flee_target(Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 4.0), 22.0);
        assert!(t.is_some());
        // Far threat / same spot -> no flee.
        assert!(flee_target(Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), 22.0).is_none());
        assert!(flee_target(Vec3::ZERO, Vec3::ZERO, 22.0).is_none());
    }

    #[test]
    fn space_economy() {
        assert_eq!(hull_max_hp(0), 200.0);
        assert_eq!(hull_max_hp(2), 350.0);
        assert_eq!(hull_cost(0), 60.0);
        assert_eq!(hull_cost(3), 240.0);
        assert_eq!(try_buy_hull(59.0, 0), None);
        assert_eq!(try_buy_hull(60.0, 0), Some((0.0, 1)));
        assert_eq!(try_buy_hull(900.0, HULL_MAX), None);
        assert_eq!(weapon_damage(0), 10.0);
        assert_eq!(weapon_damage(3), 25.0);
        assert_eq!(weapon_cost(0), 70.0);
        assert_eq!(try_buy_weapon(70.0, 0), Some((0.0, 1)));
        assert_eq!(try_buy_weapon(900.0, WEAPON_MAX), None);
        assert_eq!(survey_reward(2.2), 12.4);
        assert_eq!(survey_reward(100.0), 40.0); // clamped
                                                // Low hp flees; healthy pirates press the attack.
        assert!(pirate_flees(10.0, 60.0));
        assert!(!pirate_flees(40.0, 60.0));
        let d = aim_dir(Vec3::ZERO, Vec3::new(3.0, 0.0, 4.0)).unwrap();
        assert!((d.length() - 1.0).abs() < 1e-5);
        assert!(aim_dir(Vec3::ZERO, Vec3::ZERO).is_none());
    }

    #[test]
    fn hash_basics() {
        // FNV-1, multiply-then-XOR, lowercased (matches spore::Math::hash).
        assert_eq!(spore_hash(""), 0x811C_9DC5);
        assert_eq!(
            spore_hash("a"),
            0x811C_9DC5u32.wrapping_mul(0x0100_0193) ^ b'a' as u32
        );
        assert_eq!(spore_hash("A"), spore_hash("a"));
        assert_ne!(spore_hash("ab"), spore_hash("ba"));
        // Verified against retail PROP name hashes from the game data.
        assert_eq!(spore_hash("Group_mkmidlegSkinnyE_0"), 0x5EAFF3ED);
        assert_eq!(spore_hash("joint0"), 0xd0be09e1);
        assert_eq!(spore_hash("joint19"), 0x7b298b99);
    }

    #[test]
    fn bone_names_resolve() {
        // Wordlist-attacked names of the retail creature rig.
        assert_eq!(
            known_bone_name(spore_hash("global_NODE")),
            Some("global_NODE")
        );
        assert_eq!(
            known_bone_name(spore_hash("upper_spine_1")),
            Some("upper_spine_1")
        );
        assert_eq!(known_bone_name(spore_hash("tail_3")), Some("tail_3"));
        assert_eq!(known_bone_name(spore_hash("root")), Some("root"));
        assert!(known_bone_name(0xdead_beef).is_none());
        // Every table entry must hash back to its own name.
        for h in [
            0x7BD0_5F71u32,
            0x2E86_47BD,
            0x4194_5228,
            0x8B67_25F9,
            0xBB89_24E1,
        ] {
            let n = known_bone_name(h).expect("name");
            assert_eq!(spore_hash(n), h);
        }
    }
}
