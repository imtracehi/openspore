//! Decoder for Swarm effect directories (`.effdir`, type 0xEA5118B0).
//!
//! Original data, native layout: the field order, endianness and section
//! structure follow SporeModder-FX's `EffectDirectory.read` and the component
//! readers (`VisualEffect`, `VisualEffectBlock`, `ParticleEffect`,
//! `MetaParticleEffect`, `ModelEffect`, `ScreenEffect`, `RibbonEffect`,
//! `SequenceEffect`, `ShakeEffect`, `MaterialResource`). The structures are
//! big-endian with the little-endian arrays SporeModder-FX annotates. Every
//! section is checked to end exactly at its recorded size, so a misread field
//! cannot silently shift the rest of a directory; the installed `games` and
//! `base` directories decode byte-exact.
//!
//! Component kinds this runtime does not simulate (light, game,
//! distribute, brush, ...) are skipped by their section size; blocks that
//! reference them are kept so the runtime can account for them.

use std::collections::HashMap;

pub const TYPE_EFFDIR: u32 = 0xEA51_18B0;
/// `gameEffects_3~`, `gameEffects_2~`, `gameEffects_1~` (reg_file.txt): the
/// three effect quality tiers. The highest tier is preferred.
pub const EFFECT_GROUPS: [u32; 3] = [0x4045_0202, 0x4045_0201, 0x4045_0200];
/// Block/export index bit marking an imported (by-name) effect.
pub const IMPORT_MASK: i32 = 0x4000_0000;
const SUPPORTED_VERSION: u16 = 4;

/// Component type codes (`EffectDirectory` factory table).
pub mod kind {
    pub const EFFECT: u8 = 0x00;
    pub const PARTICLES: u8 = 0x01;
    pub const META_PARTICLES: u8 = 0x02;
    pub const DECAL: u8 = 0x03;
    pub const SEQUENCE: u8 = 0x04;
    pub const SOUND: u8 = 0x05;
    pub const SHAKE: u8 = 0x06;
    pub const CAMERA: u8 = 0x07;
    pub const MODEL: u8 = 0x08;
    pub const SCREEN: u8 = 0x09;
    pub const LIGHT: u8 = 0x0A;
    pub const GAME: u8 = 0x0B;
    pub const FAST_PARTICLES: u8 = 0x0C;
    pub const DISTRIBUTE: u8 = 0x0D;
    pub const RIBBON: u8 = 0x0E;
    pub const GAME_MODEL: u8 = 0x25;
    pub const VOLUME: u8 = 0x28;
    pub const MIX_EVENT: u8 = 0x2D;
}

/// `ParticleEffect` flag bits (SporeModder-FX `FLAGBIT_*`).
pub mod pflag {
    pub const INJECT: u32 = 0x00;
    pub const MAINTAIN: u32 = 0x01;
    pub const RATE_SUSTAIN: u32 = 0x02;
    pub const EMIT_BASE: u32 = 0x03;
    pub const SOURCE_ROUND: u32 = 0x04;
    pub const MAP_EMIT_PIN_TO_SURFACE: u32 = 0x05;
    pub const MAP_EMIT_HEIGHT_RANGE: u32 = 0x06;
    pub const MAP_EMIT_DENSITY: u32 = 0x07;
    pub const RATE_SIZE_SCALE: u32 = 0x08;
    pub const RATE_AREA_SCALE: u32 = 0x09;
    pub const RATE_VOLUME_SCALE: u32 = 0x0A;
    pub const SOURCE_SCALE_PARTICLES: u32 = 0x0B;
    pub const SURFACES: u32 = 0x0C;
    pub const MAP_COLLIDE: u32 = 0x0D;
    pub const MAP_REPEL: u32 = 0x0E;
    pub const MAP_ADVECT: u32 = 0x0F;
    pub const MAP_FORCE: u32 = 0x10;
    pub const KILL_OUTSIDE_MAP: u32 = 0x11;
    pub const MAP_COLLIDE_PIN_TO_MAP: u32 = 0x12;
    pub const RANDOM_WALK: u32 = 0x13;
    pub const RANDOM_WALK_WAIT: u32 = 0x14;
    pub const MODEL: u32 = 0x15;
    pub const TEXTURE_ACCEPT_COMPOSITE: u32 = 0x16;
    pub const ATTRACTOR: u32 = 0x17;
    pub const NOT_PRESET_ATTRACTOR: u32 = 0x18;
    pub const EMIT_SCALE_EXISTING: u32 = 0x19;
    pub const SOURCE_RESET_INCOMING: u32 = 0x1A;
    pub const RATE_KILL: u32 = 0x1B;
    pub const RATE_HOLD: u32 = 0x1C;
    pub const WARP_SPIRAL: u32 = 0x1D;
    pub const LOOP_BOX: u32 = 0x1E;
    pub const PATH: u32 = 0x1F;
    pub const PROPAGATE_ALWAYS: u32 = 0x20;
    pub const PROPAGATE_IF_KILLED: u32 = 0x21;
    pub const FRAMES_RELATIVE_SPEED: u32 = 0x22;
    pub const COLOR_VARY_RGB: u32 = 0x23;
}

/// `MetaParticleEffect` flag bits (they differ from the particle bits above 0x13).
pub mod mflag {
    pub const INJECT: u32 = 0x00;
    pub const MAINTAIN: u32 = 0x01;
    pub const RATE_SUSTAIN: u32 = 0x02;
    pub const SOURCE_ROUND: u32 = 0x04;
    pub const RATE_SIZE_SCALE: u32 = 0x08;
    pub const RATE_AREA_SCALE: u32 = 0x09;
    pub const RATE_VOLUME_SCALE: u32 = 0x0A;
    pub const SOURCE_SCALE_PARTICLES: u32 = 0x0B;
    pub const MAP_ADVECT: u32 = 0x0F;
    pub const RATE_KILL: u32 = 0x1E;
    pub const RATE_HOLD: u32 = 0x1F;
    pub const LOOP_BOX: u32 = 0x21;
}

/// `VisualEffect` flags.
pub mod eflag {
    pub const VIEW_RELATIVE: u32 = 0x1;
    pub const CAMERA_FACING: u32 = 0x2;
    pub const NO_AUTO_STOP: u32 = 0x10;
    pub const HARD_STOP: u32 = 0x20;
    pub const RIGID: u32 = 0x40;
    pub const IGNORE_SCALE: u32 = 0x200;
    pub const IGNORE_ORIENTATION: u32 = 0x400;
    /// `lodDistances` are weights (focus / facing / distance), not metres
    /// (retail LOD evaluator, world update 0xA722DB).
    pub const EXTENDED_LOD_DISTANCES: u32 = 0x80000;
    /// An LOD change stops the leaving blocks at once / lets them finish.
    pub const HARD_LOD_TRANSITION: u32 = 0x100000;
    pub const SOFT_LOD_TRANSITION: u32 = 0x200000;
}

/// `VisualEffectBlock` flags.
pub mod bflag {
    pub const IGNORE_LENGTH: u32 = 0x1;
    pub const RIGID: u32 = 0x40;
}

/// Texture slot draw modes (`TextureSlot.ENUM_DRAWMODE`).
pub mod draw {
    pub const DECAL: u8 = 0x00;
    pub const DECAL_INVERT_DEPTH: u8 = 0x01;
    pub const DECAL_IGNORE_DEPTH: u8 = 0x02;
    pub const DECAL_DEPTH: u8 = 0x03;
    pub const ADDITIVE: u8 = 0x04;
    pub const ADDITIVE_INVERT_DEPTH: u8 = 0x05;
    pub const ADDITIVE_IGNORE_DEPTH: u8 = 0x06;
    pub const MODULATE: u8 = 0x07;
    pub const NORMAL_MAP: u8 = 0x08;
    pub const NORMAL_MAP_DEPTH: u8 = 0x09;
    pub const ALPHA_TEST_DISSOLVE: u8 = 0x0A;
    /// The slot names a material resource instead of a raster.
    pub const MATERIAL: u8 = 0x0F;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ResourceId {
    pub group: u32,
    pub instance: u32,
}

impl ResourceId {
    pub const NONE: ResourceId = ResourceId {
        group: u32::MAX,
        instance: u32::MAX,
    };
    pub fn is_none(self) -> bool {
        self.instance == u32::MAX || self.instance == 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextureSlot {
    pub resource: ResourceId,
    pub format: u8,
    pub draw_mode: u8,
    pub draw_flags: u8,
    /// 0 main, 2 distortion (normal maps), 3 blur mask (observed in the Cell data).
    pub buffer: u8,
    pub layer: i16,
    pub sort_offset: f32,
    pub resource2: ResourceId,
}

/// Block transform: `flags` bit 0 scale, bit 1 rotation, bit 2 offset.
#[derive(Clone, Debug)]
pub struct BlockTransform {
    pub flags: u16,
    pub scale: f32,
    /// Row-major 3x3 (`m[row][col]`), as SporeModder-FX's `Matrix.readLE`.
    pub rotation: [f32; 9],
    pub offset: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct EffectBlock {
    pub kind: u8,
    pub flags: u32,
    pub transform: BlockTransform,
    pub lod_begin: u8,
    pub lod_end: u8,
    /// Per-LOD (emit, size, alpha) scales.
    pub lod_scales: Vec<[f32; 3]>,
    pub emit_scale: [f32; 2],
    pub size_scale: [f32; 2],
    pub alpha_scale: [f32; 2],
    pub app_flags: u32,
    pub app_flags_mask: u32,
    pub selection_group: u16,
    pub selection_chance: u16,
    pub time_scale: f32,
    pub index: i32,
}

#[derive(Clone, Debug)]
pub struct VisualEffectDef {
    pub flags: u32,
    pub app_flags_mask: u32,
    pub notify: u32,
    pub screen_size_range: [f32; 2],
    pub cursor_active_distance: f32,
    pub cursor_button: u8,
    pub lod_distances: Vec<f32>,
    pub extended_lod_weights: [f32; 3],
    pub seed: i32,
    pub blocks: Vec<EffectBlock>,
}

#[derive(Clone, Debug, Default)]
pub struct Wiggle {
    pub time_rate: f32,
    pub rate_direction: [f32; 3],
    pub wiggle_direction: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct RandomWalk {
    pub time: [f32; 2],
    pub strength: [f32; 2],
    pub turn_range: f32,
    pub turn_offset: f32,
    pub mix: f32,
    pub turn_offset_curve: Vec<f32>,
    pub loop_type: i8,
}

#[derive(Clone, Debug, Default)]
pub struct Attractor {
    pub strength: Vec<f32>,
    pub range: f32,
    pub kill_range: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub flags: u32,
    pub map: ResourceId,
    pub bounce: f32,
    pub slide: f32,
    pub collision_radius: f32,
    pub death_probability: f32,
    pub pin_offset: f32,
    pub collide_effect: i32,
    pub death_effect: i32,
    pub points: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct PathPoint {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub time: f32,
}

/// Shared particle-map fields of particle and meta-particle components.
#[derive(Clone, Debug, Default)]
pub struct MapFields {
    pub bounce: f32,
    pub repulse_height: f32,
    pub repulse_strength: f32,
    pub repulse_scout_distance: f32,
    pub repulse_vertical: f32,
    pub repulse_kill_height: f32,
    pub probability_death: f32,
    pub altitude_range: [f32; 2],
    pub force: ResourceId,
    pub emit: ResourceId,
    pub emit_color: ResourceId,
}

#[derive(Clone, Debug, Default)]
pub struct ParticleDef {
    /// Bit `i` is SporeModder-FX `FLAGBIT` i (see [`pflag`]).
    pub flags: u64,
    pub life: [f32; 2],
    pub preroll: f32,
    pub emit_delay: [f32; 2],
    pub emit_retrigger: [f32; 2],
    pub emit_direction: [[f32; 3]; 2],
    pub emit_speed: [f32; 2],
    pub emit_volume: [[f32; 3]; 2],
    pub torus_width: f32,
    pub rate: Vec<f32>,
    pub rate_loop: f32,
    pub rate_curve_cycles: i32,
    pub rate_speed_scale: f32,
    pub size: Vec<f32>,
    pub size_vary: f32,
    pub aspect: Vec<f32>,
    pub aspect_vary: f32,
    pub rotation_vary: f32,
    pub rotation_offset: f32,
    pub rotate: Vec<f32>,
    pub alpha: Vec<f32>,
    pub alpha_vary: f32,
    pub color: Vec<[f32; 3]>,
    pub color_vary: [f32; 3],
    pub texture: TextureSlot,
    pub physics_type: u8,
    pub override_set: u8,
    pub tile_count: [u8; 2],
    pub align_mode: u8,
    pub frame_start: u8,
    pub frame_count: u8,
    pub frame_random: u8,
    pub frame_speed: f32,
    pub direction_forces: [f32; 3],
    pub wind_strength: f32,
    pub gravity_strength: f32,
    pub radial_force: f32,
    pub radial_force_location: [f32; 3],
    pub drag: f32,
    pub velocity_stretch: f32,
    pub screw_rate: f32,
    pub wiggles: Vec<Wiggle>,
    pub bloom: [u8; 4],
    pub loop_box_color: Vec<[f32; 3]>,
    pub loop_box_alpha: Vec<f32>,
    pub surfaces: Vec<Surface>,
    pub map: MapFields,
    pub random_walk: RandomWalk,
    pub attractor_origin: [f32; 3],
    pub attractor: Attractor,
    pub path: Vec<PathPoint>,
}

impl ParticleDef {
    pub fn has(&self, bit: u32) -> bool {
        self.flags >> bit & 1 != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct MetaParticleDef {
    pub flags: u64,
    pub life: [f32; 2],
    pub preroll: f32,
    pub emit_delay: [f32; 2],
    pub emit_retrigger: [f32; 2],
    pub emit_direction: [[f32; 3]; 2],
    pub emit_speed: [f32; 2],
    pub emit_volume: [[f32; 3]; 2],
    pub torus_width: f32,
    pub rate: Vec<f32>,
    pub rate_loop: f32,
    pub rate_curve_cycles: i32,
    pub size: Vec<f32>,
    pub size_vary: f32,
    pub pitch: Vec<f32>,
    pub roll: Vec<f32>,
    pub yaw: Vec<f32>,
    pub pitch_vary: f32,
    pub roll_vary: f32,
    pub yaw_vary: f32,
    pub pitch_offset: f32,
    pub roll_offset: f32,
    pub yaw_offset: f32,
    pub color: Vec<[f32; 3]>,
    pub color_vary: [f32; 3],
    pub alpha: Vec<f32>,
    pub alpha_vary: f32,
    /// (component index, type code) of the effect each meta particle carries.
    pub component: (i32, i32),
    pub death_effect: i32,
    pub align_mode: u8,
    pub direction_forces: [f32; 3],
    pub global_forces: [f32; 3],
    pub wind_strength: f32,
    pub gravity_strength: f32,
    pub radial_force: f32,
    pub radial_force_location: [f32; 3],
    pub drag: f32,
    pub screw_rate: f32,
    pub wiggles: Vec<Wiggle>,
    pub bloom: [u8; 4],
    pub loop_box_color: Vec<[f32; 3]>,
    pub loop_box_alpha: Vec<f32>,
    pub surfaces: Vec<Surface>,
    pub map: MapFields,
    pub random_walk: RandomWalk,
    pub random_walk2: RandomWalk,
    pub random_walk_preferred_dir: [f32; 3],
    pub align_damping: f32,
    pub bank_amount: f32,
    pub bank_damping: f32,
    pub attractor_origin: [f32; 3],
    pub attractor: Attractor,
    pub path: Vec<PathPoint>,
    pub tractor_reset_speed: f32,
}

impl MetaParticleDef {
    pub fn has(&self, bit: u32) -> bool {
        self.flags >> bit & 1 != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModelAnimationDef {
    pub length_range: [f32; 2],
    pub curve: Vec<f32>,
    pub curve_vary: f32,
    pub speed_scale: f32,
    pub channel: u8,
    pub mode: u8,
}

#[derive(Clone, Debug, Default)]
pub struct ModelDef {
    pub flags: u32,
    pub resource: ResourceId,
    pub size: f32,
    pub color: [f32; 3],
    pub alpha: f32,
    pub animations: Vec<ModelAnimationDef>,
    pub material: ResourceId,
    pub override_set: u8,
}

#[derive(Clone, Debug, Default)]
pub struct ScreenFilterDef {
    pub kind: u8,
    pub destination: u8,
    pub source: ResourceId,
    pub parameters: Vec<u8>,
}

/// `ScreenEffect.mode`: 0 additive, 1 blend, 2 tint, 3 brighten, 4 skybox,
/// 5 background, 6..9 user, 10 filter chain.
#[derive(Clone, Debug, Default)]
pub struct ScreenDef {
    pub mode: u8,
    /// bit 0 loop, bit 1 sustain, bit 2 3D falloff, bit 3 hold, bit 4 cycle hold.
    pub flags: u32,
    pub color: Vec<[f32; 3]>,
    pub strength: Vec<f32>,
    pub distance: Vec<f32>,
    pub life: f32,
    pub delay: f32,
    pub falloff: f32,
    pub distance_base: f32,
    pub texture: ResourceId,
    pub layer: i16,
    pub filters: Vec<ScreenFilterDef>,
    pub filter_buffers: Vec<(i32, i32)>,
    pub params_float: Vec<f32>,
    pub params_vec3: Vec<[f32; 3]>,
    pub params_vec2: Vec<[f32; 2]>,
    pub params_resource: Vec<ResourceId>,
}

#[derive(Clone, Debug, Default)]
pub struct RibbonDef {
    pub flags: u32,
    pub life: [f32; 2],
    pub offset: Vec<f32>,
    pub width: Vec<f32>,
    pub taper: f32,
    pub fade: f32,
    pub alpha_decay: f32,
    pub color: Vec<[f32; 3]>,
    pub alpha: Vec<f32>,
    pub length_color: Vec<[f32; 3]>,
    pub length_alpha: Vec<f32>,
    pub edge_color: Vec<[f32; 3]>,
    pub edge_alpha: Vec<f32>,
    pub start_edge_alpha: Vec<f32>,
    pub end_edge_alpha: Vec<f32>,
    pub segment_count: i32,
    pub segment_length: f32,
    pub texture: TextureSlot,
    pub tile_uv: i32,
    pub slip_curve_speed: f32,
    pub slip_uv_speed: f32,
    pub direction_forces: [f32; 3],
    pub wind_strength: f32,
    pub gravity_strength: f32,
    pub map_emit_color: ResourceId,
    pub map_force: ResourceId,
    pub map_repulse_strength: f32,
}

/// A sequence is a script run top to bottom (SporeModder `SequenceEffect`: the
/// ArgScript lines `play EFFECT [t [t2]]` and `wait t [t2]`; an entry without an
/// effect is a `wait`). The installed data confirms the order is the play order:
/// `g2c_comet` lists its stages as 15.3 / 8 / 7.75 / 7 / 7 / 0.2 s with the stage
/// game messages in the same order, and 227 of the 372 sequences are `play A`
/// `play B` chains with no times at all. See `sim.rs::update_sequence`.
#[derive(Clone, Debug, Default)]
pub struct SequenceDef {
    /// (duration range in seconds, visual effect index). Index -1 = `wait`.
    /// A `play` range of -1 (unset) means "until the effect ends by itself".
    pub instances: Vec<([f32; 2], i32)>,
    pub flags: u32,
}

/// `SequenceDef::flags` (read from the retail ArgScript parser 0xF94D90: `loop`
/// 1, `hardStart` 2, `noOverlap` / `hardStop` 4, `overlap` / `noStop` 8).
pub mod seqflag {
    pub const LOOP: u32 = 1;
    pub const HARD_START: u32 = 2;
    pub const HARD_STOP: u32 = 4;
    pub const NO_STOP: u32 = 8;
}

/// `VolumeEffect` component (type 0x28, SporeModder-FX `VolumeEffect`; the
/// native description 0x7EE600 / reader 0x7EE6F0): a box of 8 corners cut
/// into view-aligned slices (see `volume.rs`).
#[derive(Clone, Debug)]
pub struct VolumeDef {
    /// Description +0x08 (SporeModder `field_8`): the point the slice planes
    /// are anchored to (0 in every installed volume).
    pub offset: [f32; 3],
    /// Distance between slices in world units (default 0.25).
    pub slice_spacing: f32,
    /// Effdir material (default 0x554771E5).
    pub material: ResourceId,
    /// Corner `i` (bit 0 = +x, bit 1 = +y, bit 2 = +z): position xyz, then
    /// the 3D texture coordinate uvw.
    pub corners: [[f32; 6]; 8],
}

impl Default for VolumeDef {
    fn default() -> Self {
        VolumeDef {
            offset: [0.0; 3],
            slice_spacing: 0.25,
            material: ResourceId { group: 0, instance: 0x5547_71E5 },
            corners: std::array::from_fn(|i| {
                let s = |b: usize| if i & b != 0 { 1.0 } else { -1.0 };
                [s(1), s(2), s(4), s(1), s(2), s(4)]
            }),
        }
    }
}

/// `gameModel` option bits (SporeModder-FX `GameModelEffect.ENUM_OPTIONS`).
pub mod game_model_option {
    pub const VISIBLE: u32 = 1 << 0;
    pub const APPLY_COLOR: u32 = 1 << 1;
    pub const APPLY_COLOR_AS_IDENTITY: u32 = 1 << 2;
    pub const HIGHLIGHT: u32 = 1 << 3;
    pub const ABOVE_GROUND: u32 = 1 << 4;
    pub const LOCAL_Z_UP: u32 = 1 << 5;
    pub const SHOW_DESCRIPTION: u32 = 1 << 6;
    pub const VISUAL_SCALE: u32 = 1 << 7;
    pub const CUSTOM_PICK_LEVEL: u32 = 1 << 8;
    pub const OVERRIDE_BOUNDS: u32 = 1 << 9;
    pub const APPLY_NIGHT_LIGHTS: u32 = 1 << 10;
    pub const APPLY_MAX_IDENTITY: u32 = 1 << 11;
    pub const ALPHA_SORT: u32 = 1 << 12;
    pub const SUPER_HIGH_LOD: u32 = 1 << 13;
}

/// `GameModelAnimation` (animation channel curve of a `gameModel`).
#[derive(Clone, Debug, Default)]
pub struct GameModelAnimationDef {
    pub length_range: [f32; 2],
    pub curve: Vec<f32>,
    pub curve_vary: f32,
    pub speed_scale: f32,
    /// Channel name hash (0 = default channel).
    pub channel: u32,
    /// 0 loop, 1 sustain, 2 single (SporeModder `MODE_FLAG_*`).
    pub mode: u8,
}

/// One `split` of a `gameModel` (decoded for the census, not applied).
#[derive(Clone, Debug)]
pub struct GameModelSplit {
    pub kind: u8,
    pub index: i32,
    pub transform: BlockTransform,
    pub indices: Vec<i32>,
    pub origin: [f32; 3],
}

/// `gameModel` component (type 0x25, SporeModder-FX `GameModelEffect`, versions
/// 7 and 8): a model of the game's model world (`worldID`) placed at the
/// effect's frame. The model key is stored instance first, then group.
#[derive(Clone, Debug)]
pub struct GameModelDef {
    /// `FLAGS_MESSAGE` 4, `FLAGS_PERSIST` 8, `FLAGS_FIXED_SIZE` 0x10,
    /// `FLAGS_NO_ATTACHMENTS` 0x20.
    pub flags: u32,
    pub size: f32,
    pub color: [f32; 3],
    pub alpha: f32,
    /// The model resource (instance first in the file).
    pub model: ResourceId,
    /// Model world name hash (0 = the default world).
    pub world: u32,
    pub animations: Vec<GameModelAnimationDef>,
    /// (splitter kernel index, split type, preference).
    pub splitters: Vec<(i32, u8, u8)>,
    pub splits: Vec<GameModelSplit>,
    /// Model group name hashes (`ENUM_GROUPS`).
    pub groups: Vec<u32>,
    /// [`game_model_option`] bits.
    pub options: u32,
    pub pick_level: u8,
    pub override_set: u8,
    pub message_id: u32,
}

impl GameModelDef {
    pub fn fixed_size(&self) -> bool {
        self.flags & 0x10 != 0
    }
    pub fn has_option(&self, bit: u32) -> bool {
        self.options & bit != 0
    }
}

/// Decal component flags (SporeModder-FX `DecalEffect.FLAGS_*`).
pub mod decal_flag {
    /// `texture ... repeat R`: `textureRepeat` applies.
    pub const TEXTURE_REPEAT: u32 = 1;
    /// `size ... terrainScale`: the size is relative to the terrain scale.
    pub const TERRAIN_SCALE: u32 = 2;
    /// `life ... static`: a static decal (baked into the terrain face).
    pub const STATIC: u32 = 4;
}

/// `DecalEffect.type`: which surfaces take the decal (SporeModder-FX
/// `ENUM_TYPE`; ModAPI `Swarm::DecalType`).
pub mod decal_type {
    pub const TERRAIN: u8 = 0;
    pub const WATER: u8 = 1;
    pub const TERRAIN_AND_WATER: u8 = 2;
    pub const PAINT: u8 = 3;
}

/// `decal` component (type 0x03, SporeModder-FX `DecalEffect`, version 1): a
/// textured mark projected onto the terrain (or the water, or both) at the
/// effect's position. Big-endian, with LE colours and texture offset.
#[derive(Clone, Debug)]
pub struct DecalDef {
    /// [`decal_flag`] bits.
    pub flags: u32,
    /// [`decal_type`].
    pub kind: u8,
    /// `ENUM_LIFE_TYPE`: 0 loop, 1 single, 2 sustain.
    pub life_type: u8,
    /// The texture slot (draw mode 0x0F names a material instead).
    pub texture: TextureSlot,
    pub life: f32,
    /// Turns (rotation) over normalised life.
    pub rotation: Vec<f32>,
    /// Edge length over normalised life (world units, or terrain-relative).
    pub size: Vec<f32>,
    pub alpha: Vec<f32>,
    pub color: Vec<[f32; 3]>,
    /// Width / height over normalised life.
    pub aspect: Vec<f32>,
    pub rotation_vary: f32,
    pub size_vary: f32,
    pub alpha_vary: f32,
    pub texture_repeat: f32,
    pub texture_offset: [f32; 2],
    pub map_emit_color: ResourceId,
}

impl Default for DecalDef {
    /// Constructor defaults of `DecalEffect`.
    fn default() -> Self {
        DecalDef {
            flags: 0,
            kind: 0,
            life_type: 0,
            texture: TextureSlot::default(),
            life: 0.0,
            rotation: vec![0.0],
            size: vec![1.0],
            alpha: vec![1.0],
            color: vec![[1.0; 3]],
            aspect: vec![1.0],
            rotation_vary: 0.0,
            size_vary: 0.0,
            alpha_vary: 0.0,
            texture_repeat: 1.0,
            texture_offset: [0.0; 2],
            map_emit_color: ResourceId::NONE,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShakeDef {
    pub flags: u32,
    pub life: f32,
    pub fade_time: f32,
    pub strength: Vec<f32>,
    pub frequency: Vec<f32>,
    pub aspect_ratio: f32,
    pub table: u8,
    pub falloff: f32,
}

/// `SoundEffect` component (type 0x05, SporeModder-FX `SoundEffect`): the
/// audio resource an effect plays (`soundID.instance` is the lowercase FNV-1
/// of the sound's name) and its flags. Decoded for the audio systems; the
/// Swarm simulation itself does not play sound.
#[derive(Clone, Debug, Default)]
pub struct SoundDef {
    /// `FLAGS_3D` 1, `FLAGS_LOOP` 2, `FLAGS_VOLUME` 4, `FLAGS_STOP` 8 (stops
    /// with the effect), `FLAGS_TRACK` 0x10.
    pub flags: u32,
    pub sound: ResourceId,
    pub track: u32,
    pub inv_sample: f32,
    pub length: f32,
    pub volume: f32,
}

pub mod sound_flag {
    pub const THREE_D: u32 = 1;
    pub const LOOP: u32 = 2;
    pub const VOLUME: u32 = 4;
    pub const STOP_WITH_EFFECT: u32 = 8;
    pub const TRACK: u32 = 0x10;
}

#[derive(Clone, Debug)]
pub enum MaterialValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Floats(Vec<f32>),
    Ints(Vec<i32>),
    Bools(Vec<bool>),
    /// A texture sampler binding; the property instance is the sampler index.
    Texture(ResourceId),
}

#[derive(Clone, Debug)]
pub struct MaterialProperty {
    /// Shader register parameter id: `0x10000 + (register * 4 + lane) << 4`,
    /// or the sampler index for textures.
    pub name: ResourceId,
    pub value: MaterialValue,
}

#[derive(Clone, Debug)]
pub struct MaterialDef {
    pub id: ResourceId,
    pub shader: ResourceId,
    pub properties: Vec<MaterialProperty>,
}

impl MaterialDef {
    /// Float value of shader constant register `c{register}.{lane}`.
    pub fn float(&self, register: u32, lane: u32) -> Option<f32> {
        let id = 0x10000 + ((register * 4 + lane) << 4);
        self.properties.iter().find_map(|p| match p.value {
            MaterialValue::Float(v) if p.name.instance == id => Some(v),
            _ => None,
        })
    }
    /// Texture bound to sampler `index`.
    pub fn sampler(&self, index: u32) -> Option<ResourceId> {
        self.properties.iter().find_map(|p| match p.value {
            MaterialValue::Texture(t) if p.name.instance == index => Some(t),
            _ => None,
        })
    }
    /// First bound texture, whatever its sampler.
    pub fn first_texture(&self) -> Option<ResourceId> {
        self.properties.iter().find_map(|p| match p.value {
            MaterialValue::Texture(t) => Some(t),
            _ => None,
        })
    }
}

/// One decoded `.effdir`.
#[derive(Clone, Debug, Default)]
pub struct EffectDirectory {
    pub key_group: u32,
    pub key_instance: u32,
    pub effects: Vec<VisualEffectDef>,
    pub particles: Vec<ParticleDef>,
    pub metas: Vec<MetaParticleDef>,
    pub models: Vec<ModelDef>,
    pub screens: Vec<ScreenDef>,
    pub ribbons: Vec<RibbonDef>,
    pub sequences: Vec<SequenceDef>,
    pub shakes: Vec<ShakeDef>,
    /// Sound components (blocks of kind [`kind::SOUND`] index this list).
    pub sounds: Vec<SoundDef>,
    /// Volume components (blocks of kind [`kind::VOLUME`]).
    pub volumes: Vec<VolumeDef>,
    /// Decal components (blocks of kind [`kind::DECAL`]).
    pub decals: Vec<DecalDef>,
    /// Game model components (blocks of kind [`kind::GAME_MODEL`]).
    pub game_models: Vec<GameModelDef>,
    /// Material resources by instance id.
    pub materials: HashMap<u32, MaterialDef>,
    /// Exported effect name hash -> effect index (may carry [`IMPORT_MASK`]).
    pub exports: HashMap<u32, i32>,
    /// Imported effect name hashes, addressed by `index & !IMPORT_MASK`.
    pub imports: Vec<u32>,
    /// Section counts of every component kind present, decoded or not.
    pub section_counts: HashMap<u16, u32>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    fn take<const N: usize>(&mut self) -> R<[u8; N]> {
        let bytes = self
            .data
            .get(self.pos..self.pos + N)
            .ok_or_else(|| format!("effdir truncated at {:#x}", self.pos))?;
        self.pos += N;
        Ok(bytes.try_into().unwrap())
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn i8(&mut self) -> R<i8> {
        Ok(self.u8()? as i8)
    }
    fn u16(&mut self) -> R<u16> {
        Ok(u16::from_be_bytes(self.take()?))
    }
    fn i16(&mut self) -> R<i16> {
        Ok(i16::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_be_bytes(self.take()?))
    }
    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_be_bytes(self.take()?))
    }
    fn f32_le(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    fn le2(&mut self) -> R<[f32; 2]> {
        Ok([self.f32_le()?, self.f32_le()?])
    }
    fn le3(&mut self) -> R<[f32; 3]> {
        Ok([self.f32_le()?, self.f32_le()?, self.f32_le()?])
    }
    fn count(&mut self) -> R<usize> {
        let n = self.i32()?;
        if !(0..=1 << 20).contains(&n) {
            return Err(format!("implausible effdir list length {n} at {:#x}", self.pos));
        }
        Ok(n as usize)
    }
    fn floats(&mut self) -> R<Vec<f32>> {
        let n = self.count()?;
        (0..n).map(|_| self.f32()).collect()
    }
    /// `List<ColorRGB>`: BE count, LE components.
    fn colors(&mut self) -> R<Vec<[f32; 3]>> {
        let n = self.count()?;
        (0..n).map(|_| self.le3()).collect()
    }
    fn rid(&mut self) -> R<ResourceId> {
        Ok(ResourceId {
            group: self.u32()?,
            instance: self.u32()?,
        })
    }
    fn flags64(&mut self) -> R<u64> {
        // readFlags: the first word holds bits 32..63, the second bits 0..31.
        let high = self.u32()? as u64;
        let low = self.u32()? as u64;
        Ok(high << 32 | low)
    }
}

fn texture_slot(r: &mut Reader) -> R<TextureSlot> {
    Ok(TextureSlot {
        resource: r.rid()?,
        format: r.u8()?,
        draw_mode: r.u8()?,
        draw_flags: r.u8()?,
        buffer: r.u8()?,
        layer: r.i16()?,
        sort_offset: r.f32()?,
        resource2: r.rid()?,
    })
}

fn wiggles(r: &mut Reader) -> R<Vec<Wiggle>> {
    let n = r.count()?;
    (0..n)
        .map(|_| {
            Ok(Wiggle {
                time_rate: r.f32()?,
                rate_direction: r.le3()?,
                wiggle_direction: r.le3()?,
            })
        })
        .collect()
}

fn surfaces(r: &mut Reader) -> R<Vec<Surface>> {
    let n = r.count()?;
    (0..n)
        .map(|_| {
            let mut s = Surface {
                flags: r.u32()?,
                map: r.rid()?,
                bounce: r.f32()?,
                slide: r.f32()?,
                collision_radius: r.f32()?,
                death_probability: r.f32()?,
                pin_offset: r.f32()?,
                collide_effect: r.i32()?,
                death_effect: r.i32()?,
                points: vec![],
            };
            let m = r.count()?;
            s.points = (0..m).map(|_| r.le3()).collect::<R<_>>()?;
            Ok(s)
        })
        .collect()
}

fn map_fields(r: &mut Reader) -> R<MapFields> {
    Ok(MapFields {
        bounce: r.f32()?,
        repulse_height: r.f32()?,
        repulse_strength: r.f32()?,
        repulse_scout_distance: r.f32()?,
        repulse_vertical: r.f32()?,
        repulse_kill_height: r.f32()?,
        probability_death: r.f32()?,
        altitude_range: r.le2()?,
        force: r.rid()?,
        emit: r.rid()?,
        emit_color: r.rid()?,
    })
}

fn random_walk(r: &mut Reader) -> R<RandomWalk> {
    Ok(RandomWalk {
        time: r.le2()?,
        strength: r.le2()?,
        turn_range: r.f32()?,
        turn_offset: r.f32()?,
        mix: r.f32()?,
        turn_offset_curve: r.floats()?,
        loop_type: r.i8()?,
    })
}

fn attractor(r: &mut Reader) -> R<Attractor> {
    Ok(Attractor {
        strength: r.floats()?,
        range: r.f32()?,
        kill_range: r.f32()?,
    })
}

fn path(r: &mut Reader) -> R<Vec<PathPoint>> {
    let n = r.count()?;
    (0..n)
        .map(|_| {
            Ok(PathPoint {
                position: r.le3()?,
                velocity: r.le3()?,
                time: r.f32()?,
            })
        })
        .collect()
}

fn particle(r: &mut Reader) -> R<ParticleDef> {
    let mut p = ParticleDef {
        flags: r.flags64()?,
        life: r.le2()?,
        preroll: r.f32()?,
        emit_delay: r.le2()?,
        emit_retrigger: r.le2()?,
        emit_direction: [r.le3()?, r.le3()?],
        emit_speed: r.le2()?,
        emit_volume: [r.le3()?, r.le3()?],
        torus_width: r.f32()?,
        rate: r.floats()?,
        rate_loop: r.f32()?,
        ..Default::default()
    };
    p.rate_curve_cycles = r.i16()? as i32;
    p.rate_speed_scale = r.f32()?;
    p.size = r.floats()?;
    p.size_vary = r.f32()?;
    p.aspect = r.floats()?;
    p.aspect_vary = r.f32()?;
    p.rotation_vary = r.f32()?;
    p.rotation_offset = r.f32()?;
    p.rotate = r.floats()?;
    p.alpha = r.floats()?;
    p.alpha_vary = r.f32()?;
    p.color = r.colors()?;
    p.color_vary = r.le3()?;
    p.texture = texture_slot(r)?;
    p.physics_type = r.u8()?;
    p.override_set = r.u8()?;
    p.tile_count = [r.u8()?, r.u8()?];
    p.align_mode = r.u8()?;
    p.frame_start = r.u8()?;
    p.frame_count = r.u8()?;
    p.frame_random = r.u8()?;
    p.frame_speed = r.f32()?;
    p.direction_forces = r.le3()?;
    p.wind_strength = r.f32()?;
    p.gravity_strength = r.f32()?;
    p.radial_force = r.f32()?;
    p.radial_force_location = r.le3()?;
    p.drag = r.f32()?;
    p.velocity_stretch = r.f32()?;
    p.screw_rate = r.f32()?;
    p.wiggles = wiggles(r)?;
    p.bloom = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
    p.loop_box_color = r.colors()?;
    p.loop_box_alpha = r.floats()?;
    p.surfaces = surfaces(r)?;
    p.map = map_fields(r)?;
    p.random_walk = random_walk(r)?;
    p.attractor_origin = r.le3()?;
    p.attractor = attractor(r)?;
    p.path = path(r)?;
    Ok(p)
}

fn meta_particle(r: &mut Reader) -> R<MetaParticleDef> {
    let mut p = MetaParticleDef {
        flags: r.flags64()?,
        life: r.le2()?,
        preroll: r.f32()?,
        emit_delay: r.le2()?,
        emit_retrigger: r.le2()?,
        emit_direction: [r.le3()?, r.le3()?],
        emit_speed: r.le2()?,
        emit_volume: [r.le3()?, r.le3()?],
        torus_width: r.f32()?,
        rate: r.floats()?,
        rate_loop: r.f32()?,
        ..Default::default()
    };
    // MetaParticleEffect.rateCurveCycles is an int, unlike ParticleEffect's short.
    p.rate_curve_cycles = r.i32()?;
    p.size = r.floats()?;
    p.size_vary = r.f32()?;
    p.pitch = r.floats()?;
    p.roll = r.floats()?;
    p.yaw = r.floats()?;
    p.pitch_vary = r.f32()?;
    p.roll_vary = r.f32()?;
    p.yaw_vary = r.f32()?;
    p.pitch_offset = r.f32()?;
    p.roll_offset = r.f32()?;
    p.yaw_offset = r.f32()?;
    p.color = r.colors()?;
    p.color_vary = r.le3()?;
    p.alpha = r.floats()?;
    p.alpha_vary = r.f32()?;
    p.component = (r.i32()?, r.i32()?);
    p.death_effect = r.i32()?;
    p.align_mode = r.u8()?;
    p.direction_forces = r.le3()?;
    p.global_forces = r.le3()?;
    p.wind_strength = r.f32()?;
    p.gravity_strength = r.f32()?;
    p.radial_force = r.f32()?;
    p.radial_force_location = r.le3()?;
    p.drag = r.f32()?;
    p.screw_rate = r.f32()?;
    p.wiggles = wiggles(r)?;
    p.bloom = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
    p.loop_box_color = r.colors()?;
    p.loop_box_alpha = r.floats()?;
    p.surfaces = surfaces(r)?;
    p.map = map_fields(r)?;
    p.random_walk = random_walk(r)?;
    p.random_walk2 = random_walk(r)?;
    p.random_walk_preferred_dir = r.le3()?;
    p.align_damping = r.f32()?;
    p.bank_amount = r.f32()?;
    p.bank_damping = r.f32()?;
    p.attractor_origin = r.le3()?;
    p.attractor = attractor(r)?;
    p.path = path(r)?;
    p.tractor_reset_speed = r.f32()?;
    Ok(p)
}

fn model(r: &mut Reader) -> R<ModelDef> {
    let mut m = ModelDef {
        flags: r.u32()?,
        resource: r.rid()?,
        size: r.f32()?,
        color: r.le3()?,
        alpha: r.f32()?,
        ..Default::default()
    };
    let n = r.count()?;
    for _ in 0..n {
        m.animations.push(ModelAnimationDef {
            length_range: r.le2()?,
            curve: r.floats()?,
            curve_vary: r.f32()?,
            speed_scale: r.f32()?,
            channel: r.u8()?,
            mode: r.u8()?,
        });
    }
    m.material = r.rid()?;
    m.override_set = r.u8()?;
    Ok(m)
}

fn screen(r: &mut Reader) -> R<ScreenDef> {
    let mut s = ScreenDef {
        mode: r.u8()?,
        flags: r.u32()?,
        color: r.colors()?,
        strength: r.floats()?,
        distance: r.floats()?,
        life: r.f32()?,
        delay: r.f32()?,
        falloff: r.f32()?,
        distance_base: r.f32()?,
        texture: r.rid()?,
        layer: r.i16()?,
        ..Default::default()
    };
    let n = r.count()?;
    for _ in 0..n {
        let mut f = ScreenFilterDef {
            kind: r.u8()?,
            destination: r.u8()?,
            source: r.rid()?,
            parameters: vec![],
        };
        let m = r.count()?;
        f.parameters = (0..m).map(|_| r.u8()).collect::<R<_>>()?;
        s.filters.push(f);
    }
    let n = r.count()?;
    s.filter_buffers = (0..n)
        .map(|_| Ok((r.i32()?, r.i32()?)))
        .collect::<R<_>>()?;
    s.params_float = r.floats()?;
    let n = r.count()?;
    s.params_vec3 = (0..n).map(|_| r.le3()).collect::<R<_>>()?;
    let n = r.count()?;
    s.params_vec2 = (0..n).map(|_| r.le2()).collect::<R<_>>()?;
    let n = r.count()?;
    s.params_resource = (0..n).map(|_| r.rid()).collect::<R<_>>()?;
    Ok(s)
}

fn ribbon(r: &mut Reader) -> R<RibbonDef> {
    Ok(RibbonDef {
        flags: r.u32()?,
        life: r.le2()?,
        offset: r.floats()?,
        width: r.floats()?,
        taper: r.f32()?,
        fade: r.f32()?,
        alpha_decay: r.f32()?,
        color: r.colors()?,
        alpha: r.floats()?,
        length_color: r.colors()?,
        length_alpha: r.floats()?,
        edge_color: r.colors()?,
        edge_alpha: r.floats()?,
        start_edge_alpha: r.floats()?,
        end_edge_alpha: r.floats()?,
        segment_count: r.i32()?,
        segment_length: r.f32()?,
        texture: texture_slot(r)?,
        tile_uv: r.i32()?,
        slip_curve_speed: r.f32()?,
        slip_uv_speed: r.f32()?,
        direction_forces: r.le3()?,
        wind_strength: r.f32()?,
        gravity_strength: r.f32()?,
        map_emit_color: r.rid()?,
        map_force: r.rid()?,
        map_repulse_strength: r.f32()?,
    })
}

fn sequence(r: &mut Reader) -> R<SequenceDef> {
    let n = r.count()?;
    let instances = (0..n)
        .map(|_| Ok((r.le2()?, r.i32()?)))
        .collect::<R<_>>()?;
    Ok(SequenceDef {
        instances,
        flags: r.u32()?,
    })
}

fn shake(r: &mut Reader) -> R<ShakeDef> {
    Ok(ShakeDef {
        flags: r.u32()?,
        life: r.f32()?,
        fade_time: r.f32()?,
        strength: r.floats()?,
        frequency: r.floats()?,
        aspect_ratio: r.f32()?,
        table: r.u8()?,
        falloff: r.f32()?,
    })
}

fn sound(r: &mut Reader) -> R<SoundDef> {
    Ok(SoundDef {
        flags: r.u32()?,
        sound: r.rid()?,
        track: r.u32()?,
        inv_sample: r.f32()?,
        length: r.f32()?,
        volume: r.f32()?,
    })
}

fn decal(r: &mut Reader) -> R<DecalDef> {
    Ok(DecalDef {
        flags: r.u32()?,
        kind: r.u8()?,
        life_type: r.u8()?,
        texture: texture_slot(r)?,
        life: r.f32()?,
        rotation: r.floats()?,
        size: r.floats()?,
        alpha: r.floats()?,
        color: r.colors()?,
        aspect: r.floats()?,
        rotation_vary: r.f32()?,
        size_vary: r.f32()?,
        alpha_vary: r.f32()?,
        texture_repeat: r.f32()?,
        texture_offset: r.le2()?,
        map_emit_color: r.rid()?,
    })
}

fn volume(r: &mut Reader) -> R<VolumeDef> {
    let offset = r.le3()?;
    let slice_spacing = r.f32()?;
    let material = r.rid()?;
    let mut corners = [[0f32; 6]; 8];
    for c in &mut corners {
        let [a, b, c3] = r.le3()?;
        let [d, e, f] = r.le3()?;
        *c = [a, b, c3, d, e, f];
    }
    Ok(VolumeDef {
        offset,
        slice_spacing,
        material,
        corners,
    })
}

fn game_model(r: &mut Reader, version: u16) -> R<GameModelDef> {
    let flags = r.u32()?;
    let size = r.f32()?;
    let color = r.le3()?;
    let alpha = r.f32()?;
    // Not a ResourceID: the instance comes first.
    let instance = r.u32()?;
    let group = r.u32()?;
    let world = r.u32()?;
    let mut m = GameModelDef {
        flags,
        size,
        color,
        alpha,
        model: ResourceId { group, instance },
        world,
        animations: vec![],
        splitters: vec![],
        splits: vec![],
        groups: vec![],
        options: 0,
        pick_level: 0,
        override_set: 0,
        message_id: 0,
    };
    let n = r.count()?;
    for _ in 0..n {
        m.animations.push(GameModelAnimationDef {
            length_range: r.le2()?,
            curve: r.floats()?,
            curve_vary: r.f32()?,
            speed_scale: r.f32()?,
            channel: r.u32()?,
            mode: r.u8()?,
        });
    }
    let n = r.count()?;
    for _ in 0..n {
        m.splitters.push((r.i32()?, r.u8()?, r.u8()?));
    }
    let n = r.count()?;
    for _ in 0..n {
        let kind = r.u8()?;
        let index = r.i32()?;
        let transform = block_transform(r)?;
        let k = r.count()?;
        let indices = (0..k).map(|_| r.i32()).collect::<R<_>>()?;
        let origin = r.le3()?;
        m.splits.push(GameModelSplit {
            kind,
            index,
            transform,
            indices,
            origin,
        });
    }
    let n = r.count()?;
    for _ in 0..n {
        m.groups.push(r.u32()?);
    }
    m.options = r.u32()?;
    m.pick_level = r.u8()?;
    m.override_set = r.u8()?;
    if version < 8 {
        m.flags &= !4;
    } else {
        m.message_id = r.u32()?;
    }
    Ok(m)
}

fn material(r: &mut Reader) -> R<MaterialDef> {
    let id = r.rid()?;
    let shader = r.rid()?;
    let n = r.count()?;
    let mut properties = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.rid()?;
        let kind = r.u8()?;
        let short_count = |r: &mut Reader| -> R<usize> {
            let c = r.i16()?;
            if c < 0 {
                return Err("negative material array length".into());
            }
            Ok(c as usize)
        };
        let value = match kind {
            0 => MaterialValue::Float(r.f32()?),
            1 => MaterialValue::Int(r.i32()?),
            2 => MaterialValue::Bool(r.u8()? != 0),
            3 => {
                let c = short_count(r)?;
                MaterialValue::Floats((0..c).map(|_| r.f32()).collect::<R<_>>()?)
            }
            4 => {
                let c = short_count(r)?;
                MaterialValue::Ints((0..c).map(|_| r.i32()).collect::<R<_>>()?)
            }
            5 => {
                let c = short_count(r)?;
                MaterialValue::Bools((0..c).map(|_| Ok(r.u8()? != 0)).collect::<R<_>>()?)
            }
            6 => MaterialValue::Texture(r.rid()?),
            other => return Err(format!("unknown material property type {other}")),
        };
        properties.push(MaterialProperty { name, value });
    }
    Ok(MaterialDef {
        id,
        shader,
        properties,
    })
}

fn block_transform(r: &mut Reader) -> R<BlockTransform> {
    Ok(BlockTransform {
        flags: r.u16()?,
        scale: r.f32()?,
        rotation: {
            let mut m = [0f32; 9];
            for v in &mut m {
                *v = r.f32_le()?;
            }
            m
        },
        offset: r.le3()?,
    })
}

fn visual_effect(r: &mut Reader) -> R<VisualEffectDef> {
    let mut e = VisualEffectDef {
        flags: r.u32()?,
        app_flags_mask: r.u32()?,
        notify: r.u32()?,
        screen_size_range: r.le2()?,
        cursor_active_distance: r.f32()?,
        cursor_button: r.u8()?,
        lod_distances: r.floats()?,
        extended_lod_weights: r.le3()?,
        seed: r.i32()?,
        blocks: vec![],
    };
    let n = r.count()?;
    for _ in 0..n {
        let kind = r.u8()?;
        let flags = r.u32()?;
        let transform = block_transform(&mut *r)?;
        let lod_begin = r.u8()?;
        let lod_end = r.u8()?;
        let m = r.count()?;
        let lod_scales = (0..m)
            .map(|_| Ok([r.f32()?, r.f32()?, r.f32()?]))
            .collect::<R<_>>()?;
        e.blocks.push(EffectBlock {
            kind,
            flags,
            transform,
            lod_begin,
            lod_end,
            lod_scales,
            emit_scale: [r.f32()?, r.f32()?],
            size_scale: [r.f32()?, r.f32()?],
            alpha_scale: [r.f32()?, r.f32()?],
            app_flags: r.u32()?,
            app_flags_mask: r.u32()?,
            selection_group: r.u16()?,
            selection_chance: r.u16()?,
            time_scale: r.f32()?,
            index: r.i32()?,
        });
    }
    Ok(e)
}

/// Decode `count` items of a section that must end at `end`.
fn section<T>(
    data: &[u8],
    start: usize,
    count: usize,
    end: usize,
    what: &str,
    mut item: impl FnMut(&mut Reader) -> R<T>,
) -> R<Vec<T>> {
    let mut r = Reader::new(data, start);
    let mut out = Vec::with_capacity(count.min(1 << 16));
    for i in 0..count {
        out.push(item(&mut r).map_err(|e| format!("{what} #{i}: {e}"))?);
    }
    if r.pos != end {
        return Err(format!(
            "{what} section decoded {} bytes but records {}",
            r.pos - start + 4,
            end - start + 4
        ));
    }
    Ok(out)
}

impl EffectDirectory {
    /// Decode a complete directory. Unknown component kinds are skipped by size.
    pub fn parse(data: &[u8]) -> R<EffectDirectory> {
        let mut out = EffectDirectory::default();
        let mut r = Reader::new(data, 0);
        let version = r.u16()?;
        if version != SUPPORTED_VERSION {
            return Err(format!("unsupported effdir version {version}"));
        }
        // Components (every kind except visual effects).
        loop {
            let kind = r.u16()?;
            if kind == 0xFFFF {
                break;
            }
            let version = r.u16()?;
            let size = r.u32()? as usize;
            let offset = r.pos;
            let end = offset
                .checked_add(size)
                .filter(|&e| e <= data.len())
                .ok_or("effdir component section overruns the resource")?;
            let count = Reader::new(data, offset).u32()? as usize;
            out.section_counts.insert(kind, count as u32);
            let start = offset + 4;
            match (kind as u8, version) {
                (kind::PARTICLES, 1) => {
                    out.particles = section(data, start, count, end, "particles", particle)?
                }
                (kind::META_PARTICLES, 1 | 2) => {
                    out.metas = section(data, start, count, end, "metaParticles", meta_particle)?
                }
                (kind::MODEL, 1) => out.models = section(data, start, count, end, "model", model)?,
                (kind::SCREEN, 1) => {
                    out.screens = section(data, start, count, end, "screen", screen)?
                }
                (kind::RIBBON, 1) => {
                    out.ribbons = section(data, start, count, end, "ribbon", ribbon)?
                }
                (kind::SEQUENCE, 1) => {
                    out.sequences = section(data, start, count, end, "sequence", sequence)?
                }
                (kind::SHAKE, 1 | 2) => {
                    out.shakes = section(data, start, count, end, "shake", shake)?
                }
                (kind::SOUND, 1) => out.sounds = section(data, start, count, end, "sound", sound)?,
                (kind::VOLUME, 1) => {
                    out.volumes = section(data, start, count, end, "volume", volume)?
                }
                (kind::DECAL, 1) => out.decals = section(data, start, count, end, "decal", decal)?,
                (kind::GAME_MODEL, 7 | 8) => {
                    out.game_models = section(data, start, count, end, "gameModel", |r| game_model(r, version))?
                }
                _ => {}
            }
            r.pos = end;
        }
        // Resources (maps, materials, splitters).
        loop {
            let kind = r.u16()?;
            if kind == 0xFFFF {
                break;
            }
            let _version = r.u16()?;
            let size = r.u32()? as usize;
            let offset = r.pos;
            let end = offset
                .checked_add(size)
                .filter(|&e| e <= data.len())
                .ok_or("effdir resource section overruns the resource")?;
            let count = Reader::new(data, offset).u32()? as usize;
            if kind == 1 {
                for m in section(data, offset + 4, count, end, "material", material)? {
                    out.materials.insert(m.id.instance, m);
                }
            }
            r.pos = end;
        }
        // Visual effects, then exports, string assignments and imports.
        let _version = r.u16()?;
        let count = r.u32()? as usize;
        if count > 1 << 20 {
            return Err("implausible visual effect count".into());
        }
        out.effects.reserve(count);
        for i in 0..count {
            out.effects
                .push(visual_effect(&mut r).map_err(|e| format!("effect #{i}: {e}"))?);
        }
        loop {
            let index = r.i32()?;
            if index == -1 {
                break;
            }
            let _group = r.u32()?;
            let name = r.u32()?;
            out.exports.entry(name).or_insert(index);
        }
        let _string_assignments = r.i32()?;
        let imports = r.u32()? as usize;
        if imports > 1 << 16 {
            return Err("implausible import count".into());
        }
        for _ in 0..imports {
            let _group = r.u32()?;
            out.imports.push(r.u32()?);
        }
        // Every non-import block index must address a decoded list entry.
        for (i, e) in out.effects.iter().enumerate() {
            for b in &e.blocks {
                if b.index < 0 || b.index & IMPORT_MASK != 0 {
                    continue;
                }
                let len = match b.kind {
                    kind::EFFECT => out.effects.len(),
                    kind::PARTICLES => out.particles.len(),
                    kind::META_PARTICLES => out.metas.len(),
                    kind::MODEL => out.models.len(),
                    kind::SCREEN => out.screens.len(),
                    kind::RIBBON => out.ribbons.len(),
                    kind::SEQUENCE => out.sequences.len(),
                    kind::SHAKE => out.shakes.len(),
                    kind::SOUND => out.sounds.len(),
                    kind::VOLUME => out.volumes.len(),
                    kind::DECAL => out.decals.len(),
                    kind::GAME_MODEL => out.game_models.len(),
                    _ => usize::MAX,
                };
                if b.index as usize >= len {
                    return Err(format!(
                        "effect #{i} block of kind {:#x} references #{} beyond {len}",
                        b.kind, b.index
                    ));
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Big-endian writer used to build synthetic directories.
    #[derive(Default)]
    pub struct W(pub Vec<u8>);
    impl W {
        pub fn u8(&mut self, v: u8) -> &mut Self {
            self.0.push(v);
            self
        }
        pub fn u16(&mut self, v: u16) -> &mut Self {
            self.0.extend(v.to_be_bytes());
            self
        }
        pub fn i16(&mut self, v: i16) -> &mut Self {
            self.0.extend(v.to_be_bytes());
            self
        }
        pub fn u32(&mut self, v: u32) -> &mut Self {
            self.0.extend(v.to_be_bytes());
            self
        }
        pub fn i32(&mut self, v: i32) -> &mut Self {
            self.0.extend(v.to_be_bytes());
            self
        }
        pub fn f32(&mut self, v: f32) -> &mut Self {
            self.0.extend(v.to_be_bytes());
            self
        }
        pub fn le(&mut self, v: &[f32]) -> &mut Self {
            for x in v {
                self.0.extend(x.to_le_bytes());
            }
            self
        }
        pub fn floats(&mut self, v: &[f32]) -> &mut Self {
            self.i32(v.len() as i32);
            for x in v {
                self.f32(*x);
            }
            self
        }
        pub fn colors(&mut self, v: &[[f32; 3]]) -> &mut Self {
            self.i32(v.len() as i32);
            for c in v {
                self.le(c);
            }
            self
        }
        pub fn rid(&mut self, group: u32, instance: u32) -> &mut Self {
            self.u32(group).u32(instance)
        }
    }

    /// A minimal particle component: inject `count`, life 1, size `size`, a
    /// raster texture and default everything else.
    pub fn particle_bytes(w: &mut W, count: f32, size: f32, texture: u32) {
        w.u32(0).u32(1); // flags: inject (bit 0 in the low word)
        w.le(&[1.0, 1.0]).f32(0.5); // life, preroll
        w.le(&[-1.0, -1.0]).le(&[-1.0, -1.0]); // delay, retrigger
        w.le(&[0.0, 0.0, 1.0]).le(&[0.0, 0.0, 1.0]); // direction box
        w.le(&[0.0, 0.0]); // speed
        w.le(&[0.0; 3]).le(&[0.0; 3]).f32(-1.0); // volume, torus
        w.floats(&[count]).f32(0.01).i16(1).f32(0.0); // rate, loop, cycles, speed scale
        w.floats(&[size]).f32(0.0); // size, vary
        w.floats(&[1.0]).f32(0.0); // aspect, vary
        w.f32(0.0).f32(0.0).floats(&[0.0]); // rotation vary, offset, rotate
        w.floats(&[1.0, 0.0]).f32(0.0); // alpha, vary
        w.colors(&[[1.0, 0.5, 0.25]]).le(&[0.0; 3]); // color, vary
        w.rid(0, texture).u8(0).u8(draw::ADDITIVE).u8(0).u8(0).i16(0).f32(0.0).rid(u32::MAX, u32::MAX);
        w.u8(0).u8(0).u8(1).u8(1).u8(0).u8(0).u8(0).u8(0).f32(0.0); // physics..frame speed
        w.le(&[0.0; 3]).f32(0.0).f32(0.0).f32(0.0).le(&[0.0; 3]); // forces
        w.f32(0.0).f32(0.0).f32(0.0); // drag, stretch, screw
        w.i32(0); // wiggles
        w.u8(0).u8(255).u8(0).u8(255); // bloom
        w.colors(&[]).floats(&[]); // loop box
        w.i32(0); // surfaces
        for _ in 0..6 {
            w.f32(0.0);
        }
        w.f32(0.0).le(&[-10000.0, 10000.0]); // death probability, altitude
        w.rid(u32::MAX, u32::MAX).rid(u32::MAX, u32::MAX).rid(u32::MAX, u32::MAX);
        w.le(&[5.0, 5.0]).le(&[0.0, 0.0]).f32(0.25).f32(0.0).f32(0.0).floats(&[]).u8(2); // walk
        w.le(&[0.0; 3]).floats(&[]).f32(1.0).f32(0.0); // attractor
        w.i32(0); // path
    }

    pub fn block_bytes(w: &mut W, kind: u8, index: i32, offset: [f32; 3]) {
        w.u8(kind).u32(0);
        w.u16(if offset == [0.0; 3] { 0 } else { 4 }).f32(1.0);
        w.le(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).le(&offset);
        w.u8(1).u8(255).i32(0);
        for _ in 0..6 {
            w.f32(1.0);
        }
        w.u32(0).u32(0).u16(0).u16(0).f32(1.0).i32(index);
    }

    /// A directory with one particle component, one exported effect using
    /// it at `offset`, and one effect importing `import_name`.
    pub fn directory_bytes(export_name: u32, import_name: u32, offset: [f32; 3]) -> Vec<u8> {
        let mut body = W::default();
        particle_bytes(&mut body, 3.0, 2.0, 0xABCD);
        let mut w = W::default();
        w.u16(4);
        w.u16(kind::PARTICLES as u16).u16(1).u32(body.0.len() as u32 + 4).u32(1);
        w.0.extend(&body.0);
        w.u16(0xFFFF); // end of components
        w.u16(0xFFFF); // end of resources
        w.u16(1).u32(2); // two visual effects
        for (i, (kind, index)) in [(kind::PARTICLES, 0), (kind::EFFECT, IMPORT_MASK)].into_iter().enumerate() {
            w.u32(0).u32(0).u32(0).le(&[0.0, 0.0]).f32(0.0).u8(0);
            w.floats(&[]).le(&[0.0; 3]).i32(0);
            w.i32(1);
            block_bytes(&mut w, kind, index, if i == 0 { offset } else { [0.0; 3] });
        }
        w.i32(0).u32(0).u32(export_name);
        w.i32(1).u32(0).u32(export_name ^ 1);
        w.i32(-1);
        w.i32(-1);
        w.u32(1).u32(0).u32(import_name);
        w.0
    }

    #[test]
    fn synthetic_directory_round_trips_particles_exports_and_imports() {
        let bytes = directory_bytes(0x1234_5678, 0x0BAD_F00D, [0.0, 0.0, 2.0]);
        let dir = EffectDirectory::parse(&bytes).unwrap();
        assert_eq!(dir.particles.len(), 1);
        let p = &dir.particles[0];
        assert!(p.has(pflag::INJECT));
        assert_eq!(p.rate, vec![3.0]);
        assert_eq!(p.rate_curve_cycles, 1);
        assert_eq!(p.size, vec![2.0]);
        assert_eq!(p.alpha, vec![1.0, 0.0]);
        assert_eq!(p.color, vec![[1.0, 0.5, 0.25]]);
        assert_eq!(p.texture.resource.instance, 0xABCD);
        assert_eq!(p.texture.draw_mode, draw::ADDITIVE);
        assert_eq!(p.map.altitude_range, [-10000.0, 10000.0]);
        assert_eq!(dir.effects.len(), 2);
        assert_eq!(dir.effects[0].blocks[0].transform.offset, [0.0, 0.0, 2.0]);
        assert_eq!(dir.effects[0].blocks[0].transform.flags, 4);
        assert_eq!(dir.exports.get(&0x1234_5678), Some(&0));
        assert_eq!(dir.exports.get(&(0x1234_5678 ^ 1)), Some(&1));
        assert_eq!(dir.imports, vec![0x0BAD_F00D]);
        assert_eq!(dir.effects[1].blocks[0].index, IMPORT_MASK);
    }

    #[test]
    fn sound_components_decode_and_their_blocks_validate() {
        // One sound component (loop + volume, sound id, no track, 0.05 s
        // location rate, 1.5 s, volume 0.5) and one effect whose sound block
        // points at `index`.
        let build = |index: i32| {
            let mut w = W::default();
            w.u16(SUPPORTED_VERSION);
            w.u16(kind::SOUND as u16).u16(1).u32(4 + 28).u32(1);
            w.u32(sound_flag::LOOP | sound_flag::VOLUME)
                .rid(0, 0xABCD_1234)
                .u32(0xCDCD_CDCD)
                .f32(0.05)
                .f32(1.5)
                .f32(0.5);
            w.u16(0xFFFF); // end of components
            w.u16(0xFFFF); // end of resources
            w.u16(1).u32(1); // one visual effect
            w.u32(0).u32(0).u32(0).le(&[0.0, 0.0]).f32(0.0).u8(0);
            w.floats(&[]).le(&[0.0; 3]).i32(0);
            w.i32(1);
            block_bytes(&mut w, kind::SOUND, index, [0.0; 3]);
            w.i32(0).u32(0).u32(0x77);
            w.i32(-1);
            w.i32(-1);
            w.u32(0);
            w.0
        };
        let dir = EffectDirectory::parse(&build(0)).unwrap();
        assert_eq!(dir.sounds.len(), 1);
        let s = &dir.sounds[0];
        assert_eq!(s.sound.instance, 0xABCD_1234);
        assert_eq!(s.flags, sound_flag::LOOP | sound_flag::VOLUME);
        assert_eq!((s.inv_sample, s.length, s.volume), (0.05, 1.5, 0.5));
        assert_eq!(dir.effects[0].blocks[0].kind, kind::SOUND);
        assert_eq!(dir.effects[0].blocks[0].index, 0);
        // A sound block past the list is rejected.
        assert!(EffectDirectory::parse(&build(5)).is_err());
    }

    #[test]
    fn decal_components_decode_and_their_blocks_validate() {
        // One decal (116 bytes) and one effect whose decal block points at
        // `index`.
        let build = |index: i32| {
            let mut w = W::default();
            w.u16(SUPPORTED_VERSION);
            w.u16(kind::DECAL as u16).u16(1).u32(4 + 116).u32(1);
            w.u32(decal_flag::STATIC | decal_flag::TERRAIN_SCALE).u8(decal_type::PAINT).u8(1);
            // Texture slot: resource, format, draw mode, draw flags, buffer,
            // layer, sort offset, second resource.
            w.rid(0, 0x0D14_1A32).u8(0).u8(draw::DECAL).u8(4).u8(0).i16(2).f32(0.0).rid(u32::MAX, u32::MAX);
            w.f32(5.0);
            w.floats(&[0.25]).floats(&[6.0]).floats(&[0.7]).colors(&[[0.5, 0.25, 0.125]]).floats(&[0.6]);
            w.f32(0.1).f32(0.2).f32(0.3).f32(2.0).le(&[0.5, 0.25]).rid(0, 0x02E8_31AF);
            w.u16(0xFFFF); // end of components
            w.u16(0xFFFF); // end of resources
            w.u16(1).u32(1); // one visual effect
            w.u32(0).u32(0).u32(0).le(&[0.0, 0.0]).f32(0.0).u8(0);
            w.floats(&[]).le(&[0.0; 3]).i32(0);
            w.i32(1);
            block_bytes(&mut w, kind::DECAL, index, [0.0; 3]);
            w.i32(0).u32(0).u32(0x77);
            w.i32(-1);
            w.i32(-1);
            w.u32(0);
            w.0
        };
        let dir = EffectDirectory::parse(&build(0)).unwrap();
        assert_eq!(dir.decals.len(), 1);
        let d = &dir.decals[0];
        assert_eq!(d.flags, decal_flag::STATIC | decal_flag::TERRAIN_SCALE);
        assert_eq!((d.kind, d.life_type), (decal_type::PAINT, 1));
        assert_eq!((d.texture.resource.instance, d.texture.draw_flags, d.texture.layer), (0x0D14_1A32, 4, 2));
        assert_eq!(d.life, 5.0);
        assert_eq!((d.rotation.clone(), d.size.clone(), d.alpha.clone(), d.aspect.clone()), (vec![0.25], vec![6.0], vec![0.7], vec![0.6]));
        assert_eq!(d.color, vec![[0.5, 0.25, 0.125]]);
        assert_eq!((d.rotation_vary, d.size_vary, d.alpha_vary, d.texture_repeat), (0.1, 0.2, 0.3, 2.0));
        assert_eq!((d.texture_offset, d.map_emit_color.instance), ([0.5, 0.25], 0x02E8_31AF));
        assert_eq!(dir.effects[0].blocks[0].kind, kind::DECAL);
        // A decal block past the list is rejected.
        assert!(EffectDirectory::parse(&build(3)).is_err());
    }

    #[test]
    fn volume_components_decode_and_their_blocks_validate() {
        // One volume (offset, spacing 0.125, material, 8 corners of 6 LE
        // floats: 216 bytes) and one effect whose volume block points at
        // `index`.
        let build = |index: i32| {
            let mut w = W::default();
            w.u16(SUPPORTED_VERSION);
            w.u16(kind::VOLUME as u16).u16(1).u32(4 + 216).u32(1);
            w.le(&[0.5, 0.0, -0.5]).f32(0.125).rid(0, 0x1C8F_3FE4);
            for i in 0..8u32 {
                let s = |b: u32| if i & b != 0 { 0.9 } else { -0.9 };
                let z = if i & 4 != 0 { 1.2 } else { 0.0 };
                let w3 = if i & 4 != 0 { 1.5 } else { 0.1 };
                w.le(&[s(1), s(2), z, (i & 1) as f32, (i >> 1 & 1) as f32, w3]);
            }
            w.u16(0xFFFF); // end of components
            w.u16(0xFFFF); // end of resources
            w.u16(1).u32(1); // one visual effect
            w.u32(0).u32(0).u32(0).le(&[0.0, 0.0]).f32(0.0).u8(0);
            w.floats(&[]).le(&[0.0; 3]).i32(0);
            w.i32(1);
            block_bytes(&mut w, kind::VOLUME, index, [0.0, 0.0, -0.3]);
            w.i32(0).u32(0).u32(0x77);
            w.i32(-1);
            w.i32(-1);
            w.u32(0);
            w.0
        };
        let dir = EffectDirectory::parse(&build(0)).unwrap();
        assert_eq!(dir.volumes.len(), 1);
        let v = &dir.volumes[0];
        assert_eq!(v.offset, [0.5, 0.0, -0.5]);
        assert_eq!(v.slice_spacing, 0.125);
        assert_eq!(v.material.instance, 0x1C8F_3FE4);
        assert_eq!(v.corners[0], [-0.9, -0.9, 0.0, 0.0, 0.0, 0.1]);
        assert_eq!(v.corners[7], [0.9, 0.9, 1.2, 1.0, 1.0, 1.5]);
        assert_eq!(dir.effects[0].blocks[0].kind, kind::VOLUME);
        assert!(EffectDirectory::parse(&build(1)).is_err());
        // The SporeModder-FX defaults.
        let d = VolumeDef::default();
        assert_eq!((d.slice_spacing, d.material.instance), (0.25, 0x5547_71E5));
        assert_eq!(d.corners[5], [1.0, -1.0, 1.0, 1.0, -1.0, 1.0]);
    }

    #[test]
    fn a_section_that_does_not_end_at_its_size_is_rejected() {
        let mut bytes = directory_bytes(1, 2, [0.0; 3]);
        // Grow the recorded particle section size by four bytes.
        let size = u32::from_be_bytes(bytes[6..10].try_into().unwrap()) + 4;
        bytes[6..10].copy_from_slice(&size.to_be_bytes());
        bytes.splice(10 + size as usize - 4..10 + size as usize - 4, [0u8; 4]);
        assert!(EffectDirectory::parse(&bytes).is_err());
        assert!(EffectDirectory::parse(&bytes[..20]).is_err());
    }

    #[test]
    fn material_registers_and_samplers_resolve() {
        let m = MaterialDef {
            id: ResourceId { group: 0, instance: 7 },
            shader: ResourceId { group: 0, instance: 0x628D_6012 },
            properties: vec![
                MaterialProperty {
                    name: ResourceId { group: 0, instance: 0x10000 },
                    value: MaterialValue::Float(0.5),
                },
                MaterialProperty {
                    name: ResourceId { group: 0, instance: 0x10010 },
                    value: MaterialValue::Float(8.0),
                },
                MaterialProperty {
                    name: ResourceId { group: 0, instance: 2 },
                    value: MaterialValue::Texture(ResourceId { group: 0, instance: 99 }),
                },
            ],
        };
        assert_eq!(m.float(0, 0), Some(0.5));
        assert_eq!(m.float(0, 1), Some(8.0));
        assert_eq!(m.sampler(2).map(|t| t.instance), Some(99));
        assert_eq!(m.first_texture().map(|t| t.instance), Some(99));
    }
}
