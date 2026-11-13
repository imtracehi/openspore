//! Retail Cell skin paint: the brush scripts `skinpaint_cell_*` decoded from the
//! `editors` effect directory and run on the cell body (`cell_paint.rs`).
//!
//! VERIFIED data. A Cell paint style (`cl_skinpaint_cell_*` in `paints~`) names
//! one brush script (`paintThemePaints`, e.g. `skinpaint_cell_blankMatte`
//! 0xDCAD3E09). The 24 scripts are not `pfx` text resources like the creature
//! ones: they are exported effects of the effect directory `editors`
//! (`gameEffects_N~` 0x4045020N!BAFC5CA1) built from the binary Skinpaint
//! components of the Swarm effect format (layouts: SporeModder-FX
//! `effects\Skinpaint{Settings,Distribute,Particle,Flood}Effect.java`, all four
//! sections of that directory decode to exactly their recorded sizes):
//!
//! * type 0x22 (v3) `SPSkinPaintSettings` / `SPSkinPaintClear`: one record with
//!   flag bits (1 diffuse colour, 2 spec, 4 bump, 8 spec exponent, 0x200 gloss,
//!   0x400 phong): the clear of the three textures and the material settings;
//! * type 0x23 (v2) `SPSkinPaintDistribute`: spacing, region, `particleSelect`;
//! * type 0x26 (v1) `SPSkinPaintParticle`: brushes, life / delay, alignment,
//!   `chain`, channel variables and `modifier` blocks;
//! * type 0x27 (v1) `SPSkinPaintFlood`: the same variables, applied per texel;
//! * an exported effect is a `sequence` (type 0x04) of effects holding those
//!   blocks (`blankMatte`: clear, then two distributes of 0.05 spacing: the
//!   `skn_brush_blankTooth` stamps and the `noise_specks_skin` overlay).
//!
//! The binary components are converted into the model of the creature painter
//! (`creature_skin_paint::Script`) and run by the same CPU painter (stamps,
//! blends, colour dilate, bump -> normal); the binary form of the creature
//! `skinpaint_base_blankMatte` in the same directory family equals its `pfx`
//! text (checked field by field while decoding).
//!
//! Output feeds `CellSh` (`cell_sh.rs`): the diffuse atlas, a normal map whose
//! alpha is the painted specular level (retail `specNMap.a`), and the body's
//! `materialParams` from the Settings component ([`BodyTextures::material_params`]).
//!
//! Since cell-paint3 the body's normal map is retail's `skpBumpToNormal`
//! ([`BodyTextures::with_native_normals`], the installed shader) and the parts
//! get retail's `PaintParts` tint ([`paint_part`], the installed
//! `skpSplatTintShader`).
//!
//! Since cell-paint4 the placement of the particles follows retail's
//! `cSkinpaintDistributeEffect::ApplyEffect` (0x5314B0, a 6.7 KB routine earlier
//! passes mistook for the description's text writer): triangle order (index
//! order for `ordered`, else a shuffle of the first half), one particle per
//! unused triangle at its centroid, the `region` / `back` / `belly` / `range` /
//! `inverse` / `-centerOnly` / `cover` / `limit` / `particleSelect` rules and the
//! `spacing` exclusion flood over shared edges are the code's own
//! (`creature_skin_paint::Painter::samples_mesh`, which documents each address
//! and offset). The layer colours 2 / 3 are the editor model's white and the
//! layer seed its default 1234 ([`MODEL_COLOR`], [`SEED`]).
//!
//! Since cell-paint5 the placement uses retail's own random generators
//! (`Math::RandomNumberGenerator`: `RandomUInt` 0x936020, `RandomInt` 0xA68F70,
//! `RandomFloat` 0x936040, see `creature_skin_paint::RetailRng`), the spine of
//! the body is the baked body's own vertebra partition (the GMDL's body bones,
//! [`BodySurface::build`]; `range` compares the triangle's vertebra index) and
//! the parts carry the style's brush stamps: every distribute without a
//! `region` filter (39 of the 45 installed ones) also runs over the parts'
//! charts (triangle flag 4, [`PartChart`]) and what it paints lies under the
//! part's diffuse in [`paint_part`].
//!
//! The retail Cell body is a metaball skin of the creation's vertebrae
//! (`CellTemplate`: `editorUseSkin` 1, `editorSkinResolutionRealtime` 0.035,
//! `editorVertebraModel` `cl_vertebra`, manipulators `CellTorso` and
//! `SpineScaling`). The starter's baked GMDL is that same code's output for the
//! unmodified chain, so its mesh and atlas UVs are retail's own unwrap; a
//! creation that scaled its vertebrae would need the unwrap run again.
//!
//! ADAPTED (still not traced; see `creature_skin_paint`): the retail skin
//! classifier derives back / belly from the weighted vertebra up vectors, here
//! they use the angle around the polyline of the vertebra centres (the +Y
//! end is vertebra 0 here, retail's vertebra 0 is the mouth end at -Y); crawling particles (`life`, `attract`, `align`) move over
//! the nearest surface texels (retail steps `cSkinpaintInfo`s in 1/50 s ticks
//! over a surface walker inlined in 0x520300 with edge crossing); the global
//! random generator's state is fixed by the style and layer seed instead of
//! `rdtsc`, and the order of the per-particle draws is this painter's; the
//! identity flag is ignored (no installed Cell script uses it, census); the
//! unwrap of the baked body is the installed one; a part's chart is painted
//! once per mounted instance at that instance's mount size (retail has one
//! rectangle per rigblock instance, `FUN_004CB890`; the draws of each instance
//! are seeded apart, `paint_chart`) and only the diffuse layer (the part's
//! spec / bump keep their authored values). The body's skin frame is the
//! GMDL's own (front = vertebra 0 = -Y, `body_to_skin`).

use std::collections::HashMap;

use bevy::math::Vec3;

use crate::{
    asset_catalog::ResourceIndex,
    creature_skin::{Chain, ChartId, Primitives, Unwrapped, PAINT_SIZE},
    creature_skin_paint::{self as paint, Blend, Brush, BrushTex, ColorRef, Item, Motion, MotionKind, Op, Particle, RangeFlag, Script, Source, Variable},
    dbpf::ResourceKey,
    swarm_fx::format::{EffectDirectory, EFFECT_GROUPS, TYPE_EFFDIR},
    world_objects::name_id,
};

/// Seed of the single Cell layer. VERIFIED: a fresh editor model
/// (`EditorModel::EditorModel` 0x4ABBB0) holds 0x4D2 = 1234 as the seed of each
/// of its three layers (+0x98 + 4 * layer); the Cell Creator never sets one.
/// INFERRED: a new cell keeps it (a loaded `.cll` could carry another).
pub const SEED: u32 = 1234;

/// The colours of layers 2 and 3 of a cell. VERIFIED: the editor model's three
/// colours all start as (1, 1, 1) (0x4ABBB0 copies the static at 0x15D68E8,
/// initialised by 0x1238980), the paint colour message of the editor (0x593118
/// -> `EditorModel::SetColor` 0x4AE250) sets one layer by index and the Cell
/// Creator's picker sends layer 0 only, so layers 2 / 3 stay white. INFERRED
/// that a new cell is not loaded from a resource that stores others.
pub const MODEL_COLOR: [f32; 3] = [1.0, 1.0, 1.0];

/// The three layer colours of a cell painted with `color` (layer 1).
pub fn layer_colors(color: [f32; 3]) -> [[f32; 3]; 3] {
    [color, MODEL_COLOR, MODEL_COLOR]
}

/// Brush textures `<name>__diffuse` / `<name>__specBump` the 24 Cell styles
/// use, by name (the scripts hold the FNV-1 hash of the name only; names from
/// the SporeModder-FX registry, each hash re-checked by a test).
pub const BRUSH_NAMES: &[&str] = &[
    "frog_base_skin",
    "frog_skin_crease",
    "frog_warts",
    "leopard_skin_spots2",
    "noise_specks_skin",
    "skn_base_giraffe_over",
    "skn_base_giraffe_under",
    "skn_base_jellyfish_organs",
    "skn_base_lumpy",
    "skn_base_snakescales_border",
    "skn_brush_airbrush",
    "skn_brush_blankTooth",
    "skn_brush_circle",
    "skn_brush_cowSplotch",
    "skn_brush_dent",
    "skn_brush_hitsoft",
    "skn_brush_melt",
    "skn_brush_plates_highlight",
    "skn_brush_plates_innerLarge",
    "skn_brush_plates_innerSmall",
    "skn_brush_plates_outer",
    "skn_brush_polkadot",
    "skn_cell_bacteria",
    "skn_cell_circleWarped",
    "skn_cell_fishscales",
    "skn_cell_giraffe",
    "skn_cell_granite",
    "skn_cell_lumpy",
    "skn_cell_mantella",
    "skn_cell_marble",
    "skn_cell_melt",
    "skn_cell_snakescales_border",
    "skn_cell_snakescales_shape",
    "skn_cell_squiggle_over",
    "skn_cell_squiggle_under",
    "skn_cell_sucker",
    "skn_cell_walrus",
];

/// The brush's name for its hash (unknown hashes keep `#XXXXXXXX`, which has
/// no textures: the painter falls back to a round dot).
fn brush_name(hash: u32) -> String {
    BRUSH_NAMES
        .iter()
        .find(|n| name_id(n) == hash)
        .map_or_else(|| format!("#{hash:08X}"), |n| (*n).to_string())
}

// ---------------------------------------------------------------------------
// Binary components
// ---------------------------------------------------------------------------

/// Component type codes (`SkinpaintSettingsEffect.TYPE_CODE` ...).
const SETTINGS: u16 = 0x22;
const DISTRIBUTE: u16 = 0x23;
const PARTICLE: u16 = 0x26;
const FLOOD: u16 = 0x27;
/// `EffectBlock::kind` of a nested visual effect / a sequence.
const BLOCK_EFFECT: u8 = 0x00;
const BLOCK_SEQUENCE: u8 = 0x04;

/// Big-endian reader with bounds checks.
struct Rd<'a> {
    data: &'a [u8],
    pos: usize,
}

type R<T> = Result<T, String>;

impl Rd<'_> {
    fn take<const N: usize>(&mut self) -> R<[u8; N]> {
        let bytes = self
            .data
            .get(self.pos..self.pos + N)
            .ok_or("skinpaint component runs past its section")?;
        self.pos += N;
        Ok(bytes.try_into().unwrap())
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn i8(&mut self) -> R<i8> {
        Ok(self.u8()? as i8)
    }
    fn i16(&mut self) -> R<i16> {
        Ok(i16::from_be_bytes(self.take()?))
    }
    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_be_bytes(self.take()?))
    }
    fn f32_le(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    /// A list length (32 bit), bounded by what could still be read.
    fn count(&mut self) -> R<usize> {
        let n = self.u32()? as usize;
        if n > self.data.len() {
            return Err("implausible skinpaint list length".into());
        }
        Ok(n)
    }
    fn cstr(&mut self) -> R<()> {
        let end = self.data[self.pos..].iter().position(|b| *b == 0).ok_or("unterminated string")?;
        self.pos += end + 1;
        Ok(())
    }
}

/// `VarModifier`: three floats, type, value count, value index.
#[derive(Clone, Copy, Debug)]
struct RawModifier {
    v: [f32; 3],
    kind: i8,
    count: i8,
    index: i16,
}

/// `Variable`: range, values, channel, modifier index, operation, range flag.
#[derive(Clone, Copy, Debug)]
struct RawVariable {
    range: [f32; 2],
    values: [f32; 2],
    channel: u8,
    modifier: i8,
    op: u8,
    flag: u8,
}

#[derive(Clone, Debug, Default)]
struct RawVars {
    modifiers: Vec<RawModifier>,
    variables: Vec<RawVariable>,
    values: Vec<f32>,
}

fn raw_vars(r: &mut Rd) -> R<RawVars> {
    let mut out = RawVars::default();
    for _ in 0..r.count()? {
        out.modifiers.push(RawModifier { v: [r.f32()?, r.f32()?, r.f32()?], kind: r.i8()?, count: r.i8()?, index: r.i16()? });
    }
    for _ in 0..r.count()? {
        out.variables.push(RawVariable {
            range: [r.f32()?, r.f32()?],
            values: [r.f32()?, r.f32()?],
            channel: r.u8()?,
            modifier: r.i8()?,
            op: r.u8()?,
            flag: r.u8()?,
        });
    }
    for _ in 0..r.count()? {
        out.values.push(r.f32()?);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct RawSettings {
    color_index: i32,
    color: [f32; 3],
    spec_bump: [f32; 3],
    gloss: f32,
    phong: f32,
    flags: i32,
}

fn raw_settings(r: &mut Rd, version: u16) -> R<RawSettings> {
    let color_index = r.i32()?;
    let color = [r.f32()?, r.f32()?, r.f32()?];
    let spec_bump = [r.f32_le()?, r.f32_le()?, r.f32_le()?];
    let gloss = if version >= 2 { r.f32()? } else { 0.0 };
    let phong = if version >= 3 { r.f32()? } else { 0.0 };
    // part bump / spec scale, eight hair floats, a flag and two resource ids.
    for _ in 0..(2 + 8) {
        r.f32()?;
    }
    r.u8()?;
    for _ in 0..4 {
        r.u32()?;
    }
    Ok(RawSettings { color_index, color, spec_bump, gloss, phong, flags: r.i32()? })
}

#[derive(Clone, Debug)]
struct RawDistribute {
    particle: i32,
    spacing: f32,
    limit: i32,
    region: i32,
    back: f32,
    belly: f32,
    range: [f32; 2],
    inverse: bool,
    center_only: bool,
    cover: bool,
    ordered: bool,
    select: Vec<(i32, f32)>,
    select_all: bool,
}

fn raw_distribute(r: &mut Rd) -> R<RawDistribute> {
    r.cstr()?;
    let particle = r.i32()?;
    let spacing = r.f32()?;
    let limit = r.i32()?;
    let region = r.i32()?;
    let back = r.f32()?;
    let belly = r.f32()?;
    let range = [r.f32_le()?, r.f32_le()?];
    let inverse = r.u8()? != 0;
    let center_only = r.u8()? != 0;
    let cover = r.u8()? != 0;
    let ordered = r.u8()? != 0;
    let mut select = Vec::new();
    for _ in 0..r.count()? {
        select.push((r.i32()?, r.f32()?));
    }
    Ok(RawDistribute { particle, spacing, limit, region, back, belly, range, inverse, center_only, cover, ordered, select, select_all: r.u8()? != 0 })
}

#[derive(Clone, Debug)]
struct RawParticle {
    brushes: Vec<u32>,
    chain: i32,
    flags: i32,
    color: [f32; 3],
    color_index: i8,
    align_flag: u8,
    init_dir_flag: u8,
    attract_flag: u8,
    align: [f32; 3],
    init_dir: [f32; 3],
    attract: [f32; 3],
    attract_force: f32,
    delay: [f32; 2],
    life: [f32; 2],
    inherit: i32,
    vars: RawVars,
    blends: [u8; 3],
}

fn raw_particle(r: &mut Rd) -> R<RawParticle> {
    let mut brushes = Vec::new();
    for _ in 0..r.count()? {
        brushes.push(r.u32()?);
    }
    let chain = r.i32()?;
    let flags = r.i32()?;
    let color = [r.f32_le()?, r.f32_le()?, r.f32_le()?];
    let (color_index, align_flag, init_dir_flag, attract_flag) = (r.i8()?, r.u8()?, r.u8()?, r.u8()?);
    let vec3 = |r: &mut Rd| -> R<[f32; 3]> { Ok([r.f32_le()?, r.f32_le()?, r.f32_le()?]) };
    let (align, init_dir, attract) = (vec3(r)?, vec3(r)?, vec3(r)?);
    let attract_force = r.f32()?;
    let delay = [r.f32()?, r.f32()?];
    let life = [r.f32()?, r.f32()?];
    let inherit = r.i32()?;
    let vars = raw_vars(r)?;
    let blends = [r.u8()?, r.u8()?, r.u8()?];
    Ok(RawParticle { brushes, chain, flags, color, color_index, align_flag, init_dir_flag, attract_flag, align, init_dir, attract, attract_force, delay, life, inherit, vars, blends })
}

#[derive(Clone, Debug)]
struct RawFlood {
    vars: RawVars,
    color: [f32; 3],
    color_index: i32,
    blends: [u8; 3],
}

fn raw_flood(r: &mut Rd) -> R<RawFlood> {
    let vars = raw_vars(r)?;
    let _hair_face_camera = r.u8()?;
    let color = [r.f32()?, r.f32()?, r.f32()?];
    let color_index = r.i32()?;
    Ok(RawFlood { vars, color, color_index, blends: [r.u8()?, r.u8()?, r.u8()?] })
}

/// The four component lists of the directory, as recorded.
#[derive(Default)]
struct Components {
    settings: Vec<RawSettings>,
    distributes: Vec<RawDistribute>,
    particles: Vec<RawParticle>,
    floods: Vec<RawFlood>,
}

/// Walk the component sections of an `.effdir` (the Swarm decoder skips the
/// Skinpaint kinds by size) and decode the four Skinpaint ones.
fn components(data: &[u8]) -> R<Components> {
    let mut out = Components::default();
    let mut r = Rd { data, pos: 2 };
    loop {
        let kind = u16::from_be_bytes(r.take()?);
        if kind == 0xFFFF {
            return Ok(out);
        }
        let version = u16::from_be_bytes(r.take()?);
        let size = r.u32()? as usize;
        let end = r.pos.checked_add(size).filter(|e| *e <= data.len()).ok_or("effdir section overruns the resource")?;
        let count = r.count()?;
        let mut section = Rd { data: &data[..end], pos: r.pos };
        match kind {
            SETTINGS => {
                for _ in 0..count {
                    out.settings.push(raw_settings(&mut section, version)?);
                }
            }
            DISTRIBUTE => {
                for _ in 0..count {
                    out.distributes.push(raw_distribute(&mut section)?);
                }
            }
            PARTICLE => {
                for _ in 0..count {
                    out.particles.push(raw_particle(&mut section)?);
                }
            }
            FLOOD => {
                for _ in 0..count {
                    out.floods.push(raw_flood(&mut section)?);
                }
            }
            _ => {}
        }
        if matches!(kind, SETTINGS | DISTRIBUTE | PARTICLE | FLOOD) && section.pos != end {
            return Err(format!("skinpaint section {kind:#x} decoded {} bytes of {}", section.pos - r.pos, end - r.pos));
        }
        r.pos = end;
    }
}

// ---------------------------------------------------------------------------
// Conversion to the painter's script model
// ---------------------------------------------------------------------------

fn blend_of(b: u8) -> Blend {
    match b {
        2 => Blend::Add,
        3 => Blend::Subtract,
        4 => Blend::Multiply,
        5 => Blend::Screen,
        _ => Blend::Alpha,
    }
}

/// `diffuseColor`: 0..2 = colour 1..3, -2 = identity. -1 (a literal colour in
/// the script) stores the colour as (hue, saturation, value): the 24 Cell
/// scripts use it only for greys and white (the highlights of the `add` /
/// `screen` stamps, `rubberband` 0.9), so a script's `diffuseColor 1 1 1` reads
/// back as (0, 0, 1). INFERRED from those values; the hue scale (degrees) is
/// never exercised.
fn color_of(index: i32, hsv: [f32; 3]) -> ColorRef {
    match index {
        0..=2 => ColorRef::Layer(index as usize),
        -2 => ColorRef::Identity,
        _ => {
            let (h, s, v) = (hsv[0].rem_euclid(360.0) / 60.0, hsv[1].clamp(0.0, 1.0), hsv[2].clamp(0.0, 1.0));
            let (i, f) = (h.floor(), h - h.floor());
            let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
            ColorRef::Rgb(match i as u32 {
                0 => [v, t, p],
                1 => [q, v, p],
                2 => [p, v, t],
                3 => [p, q, v],
                4 => [t, p, v],
                _ => [v, p, q],
            })
        }
    }
}

/// `alignFlag` & co: 0 inherit, 1 move direction, 2 attract, 3 a vector,
/// 4 along the bone (or a vector in bone space), 5 around the bone, 6 along the
/// spine, 7 around the spine (`SkinpaintParticleEffect.ALIGN_*`).
fn motion_of(flag: u8, vector: [f32; 3], reverse: bool) -> Option<Motion> {
    let v = Vec3::from_array(vector);
    let kind = match flag {
        0 => return None,
        1 => MotionKind::Dir,
        2 => MotionKind::Attract,
        3 => MotionKind::Attract,
        4 if v == Vec3::Y => MotionKind::AlongBone,
        4 => MotionKind::Attract,
        5 => MotionKind::AroundBone,
        6 => MotionKind::AlongSpine,
        _ => MotionKind::AroundSpine,
    };
    Some(Motion { kind, vector: if matches!(flag, 3 | 4) && v != Vec3::Y { v } else { Vec3::ZERO }, reverse })
}

fn source_of(m: &RawModifier) -> Source {
    let axis = Vec3::from_array(m.v);
    match m.kind {
        0 => Source::Age,
        1 => Source::Random,
        2 => Source::WorldAngle(axis.normalize_or(Vec3::Z)),
        // The binary keeps the axis already divided by the sum / the square length.
        3 => Source::WorldPos(axis),
        4 => Source::WorldDist(axis),
        5 => Source::BoneAngle,
        6 => Source::BonePos,
        7 => Source::BoneOffset,
        8 => Source::TorsoPos,
        9 => Source::LimbPos,
        10 => Source::LimbType,
        11 => Source::Region,
        12 => Source::PaintMask,
        13 => Source::Channel((m.count.max(0) as usize).min(18)),
        // Constant curves (-1) and the compare slots (-2, -3) are never read
        // through `Variable::modifier`.
        _ => Source::PaintMask,
    }
}

fn op_of(op: u8) -> Op {
    match op {
        1 => Op::Add,
        2 => Op::Mult,
        3 => Op::Div,
        _ => Op::Set,
    }
}

fn flag_of(flag: u8) -> RangeFlag {
    match flag {
        1 => RangeFlag::Open,
        2 | 5 => RangeFlag::Clamp,
        3 | 6 => RangeFlag::Wrap,
        4 | 7 => RangeFlag::Mirror,
        _ => RangeFlag::Inside,
    }
}

/// The channel variables and modifier sources of a particle / flood.
fn convert_vars(raw: &RawVars) -> (Vec<Source>, Vec<Variable>) {
    let modifiers: Vec<Source> = raw.modifiers.iter().map(source_of).collect();
    let mut vars = Vec::new();
    for v in &raw.variables {
        let channel = (v.channel as usize).min(18);
        let modifier = raw.modifiers.get(v.modifier.max(0) as usize).filter(|_| v.modifier >= 0);
        match modifier {
            // No modifier: one value, applied with the stored operation (set).
            None => vars.push(Variable {
                channel,
                op: op_of(v.op),
                values: vec![v.values[0]],
                vary: 0.0,
                modifier: None,
                range: (0.0, 1.0),
                flag: RangeFlag::Clamp,
                curve: false,
            }),
            // A constant curve (values over the age, `-vary`; the stored
            // operation defaults to multiply).
            Some(m) if m.kind == -1 => {
                let at = m.index.max(0) as usize;
                let values: Vec<f32> = raw.values.get(at..at + m.count.max(0) as usize).map(<[f32]>::to_vec).unwrap_or_default();
                if values.is_empty() {
                    continue;
                }
                vars.push(Variable { channel, op: op_of(v.op), values, vary: m.v[0], modifier: None, range: (0.0, 1.0), flag: RangeFlag::Clamp, curve: true });
            }
            // compareNormal / comparePosition (never used by the Cell scripts).
            Some(m) if m.kind < 0 => {}
            Some(_) => vars.push(Variable {
                channel,
                op: op_of(v.op),
                values: v.values.to_vec(),
                vary: 0.0,
                modifier: Some(v.modifier as usize),
                range: (v.range[0], v.range[1]),
                flag: flag_of(v.flag),
                curve: false,
            }),
        }
    }
    (modifiers, vars)
}

/// The decoded `editors` directory: components and exported effects.
pub struct Library {
    components: Components,
    dir: EffectDirectory,
}

impl Library {
    /// Decode the `editors` effect directory of the highest installed quality
    /// tier that has it.
    pub fn load(index: &ResourceIndex) -> R<Library> {
        let editors = name_id("editors");
        for group in EFFECT_GROUPS {
            let key = ResourceKey { type_id: TYPE_EFFDIR, group_id: group, instance_id: editors };
            let Ok(bytes) = index.read(key) else { continue };
            return Ok(Library { components: components(&bytes)?, dir: EffectDirectory::parse(&bytes)? });
        }
        Err("no `editors` effect directory installed".into())
    }

    /// The brush script exported under `id` (`skinpaint_cell_blankMatte` =
    /// 0xDCAD3E09), as the painter's script model. Effects are named `e<index>`,
    /// particles `p<index>`, floods `f<index>`, distributes `d<index>`; the
    /// entry effect is the last one.
    pub fn script(&self, id: u32) -> Option<Script> {
        let entry = *self.dir.exports.get(&id)?;
        if entry < 0 || entry & crate::swarm_fx::format::IMPORT_MASK != 0 {
            return None;
        }
        let mut script = Script::default();
        self.add_effect(&mut script, entry as usize, 0);
        Some(script)
    }

    fn add_effect(&self, script: &mut Script, effect: usize, depth: usize) -> String {
        let name = format!("e{effect}");
        if script.effects.contains_key(&name) || depth > 8 {
            return name;
        }
        script.effects.insert(name.clone(), Vec::new());
        let mut items = Vec::new();
        let Some(def) = self.dir.effects.get(effect) else { return name };
        for block in &def.blocks {
            let index = block.index.max(0) as usize;
            match block.kind {
                k if k as u16 == SETTINGS => {
                    if let Some(s) = self.components.settings.get(index) {
                        items.extend(settings_items(s));
                    }
                }
                k if k as u16 == DISTRIBUTE => {
                    if let Some(d) = self.components.distributes.get(index) {
                        let dname = format!("d{index}");
                        let dist = self.convert_distribute(script, d);
                        script.distributes.insert(dname.clone(), dist);
                        items.push(Item::Distribute(dname));
                    }
                }
                k if k as u16 == FLOOD => {
                    if let Some(f) = self.components.floods.get(index) {
                        let fname = format!("f{index}");
                        let (modifiers, vars) = convert_vars(&f.vars);
                        script.floods.insert(
                            fname.clone(),
                            Particle {
                                color: Some(color_of(f.color_index, f.color)),
                                diffuse_blend: blend_of(f.blends[0]),
                                specular_blend: blend_of(f.blends[1]),
                                bump_blend: blend_of(f.blends[2]),
                                modifiers,
                                vars,
                                ..Default::default()
                            },
                        );
                        items.push(Item::Flood(fname));
                    }
                }
                BLOCK_SEQUENCE => {
                    if let Some(seq) = self.dir.sequences.get(index) {
                        for (_, e) in &seq.instances {
                            if *e >= 0 {
                                items.push(Item::Effect(self.add_effect(script, *e as usize, depth + 1)));
                            }
                        }
                    }
                }
                BLOCK_EFFECT => items.push(Item::Effect(self.add_effect(script, index, depth + 1))),
                k => script.unknown.push(format!("block kind {k:#x}")),
            }
        }
        script.effects.insert(name.clone(), items);
        // Entry last: `Script::entry` falls back to the last effect.
        script.effect_order.retain(|n| *n != name);
        script.effect_order.push(name.clone());
        name
    }

    fn convert_distribute(&self, script: &mut Script, d: &RawDistribute) -> paint::Distribute {
        let particle = |script: &mut Script, index: i32| -> Option<String> {
            (index >= 0).then(|| self.add_particle(script, index as usize)).flatten()
        };
        let mut out = paint::Distribute {
            spacing: d.spacing,
            ordered: d.ordered,
            limit: (d.limit >= 0).then_some(d.limit as usize),
            region: (d.region != -1).then_some(d.region as u32),
            body_range: (d.range[0], d.range[1]),
            inverse: d.inverse,
            select_all: d.select_all,
            center_only: d.center_only,
            cover: d.cover,
            ..Default::default()
        };
        if d.region != -1 {
            if d.region as u32 & paint::region::BACK != 0 {
                out.back = Some(d.back);
            }
            if d.region as u32 & paint::region::BELLY != 0 {
                out.belly = Some(d.belly);
            }
        }
        out.particle = particle(script, d.particle).unwrap_or_default();
        for (index, prob) in &d.select {
            if let Some(name) = particle(script, *index) {
                out.select.push((name, *prob));
            }
        }
        out
    }

    /// Add particle `index` (and its chain) to `script`; returns its name.
    fn add_particle(&self, script: &mut Script, index: usize) -> Option<String> {
        let name = format!("p{index}");
        if script.particles.contains_key(&name) {
            return Some(name);
        }
        let raw = self.components.particles.get(index)?;
        // Insert a placeholder first: a chain cycle terminates.
        script.particles.insert(name.clone(), Particle::default());
        let chain = (raw.chain >= 0).then(|| self.add_particle(script, raw.chain as usize)).flatten();
        let (modifiers, vars) = convert_vars(&raw.vars);
        let reverse = |bit: i32| raw.flags & bit != 0;
        let particle = Particle {
            brushes: raw.brushes.iter().map(|b| brush_name(*b)).collect(),
            life: (raw.life[0], raw.life[1]),
            delay: (raw.delay[0], raw.delay[1]),
            color: Some(color_of(raw.color_index as i32, raw.color)),
            diffuse_blend: blend_of(raw.blends[0]),
            specular_blend: blend_of(raw.blends[1]),
            bump_blend: blend_of(raw.blends[2]),
            align: motion_of(raw.align_flag, raw.align, reverse(0x40)),
            attract: motion_of(raw.attract_flag, raw.attract, reverse(0x10)),
            attract_force: raw.attract_force,
            init_dir: motion_of(raw.init_dir_flag, raw.init_dir, reverse(0x20)),
            chain,
            inherit: (0..19).filter(|c| raw.inherit & (1 << c) != 0).collect(),
            modifiers,
            vars,
        };
        script.particles.insert(name.clone(), particle);
        Some(name)
    }
}

/// The clear and the material settings of one 0x22 component.
fn settings_items(s: &RawSettings) -> Vec<Item> {
    let mut items = Vec::new();
    if s.flags & 0x7 != 0 {
        items.push(Item::Clear(paint::Clear {
            diffuse: (s.flags & 1 != 0).then(|| color_of(s.color_index, s.color)),
            spec: (s.flags & 2 != 0).then_some(s.spec_bump[0]),
            bump: (s.flags & 4 != 0).then_some(s.spec_bump[2]),
        }));
    }
    if s.flags & (0x8 | 0x200 | 0x400) != 0 {
        items.push(Item::Settings {
            exponent: (s.flags & 0x8 != 0).then_some(s.spec_bump[1] * 60.0),
            phong: (s.flags & 0x400 != 0).then_some(s.phong),
            gloss: (s.flags & 0x200 != 0).then_some(s.gloss),
        });
    }
    items
}

// ---------------------------------------------------------------------------
// The body surface
// ---------------------------------------------------------------------------

/// The cell body as a paintable skin: texels -> surface points of the baked
/// mesh in skin units (the GMDL's own space, Z up; the mouth end at -Y), with the
/// spine of the body's own vertebrae.
pub struct BodySurface {
    prims: Primitives,
    map: paint::TexelMap,
    /// Body size in skin units (x, y, z), for logs.
    pub extent: Vec3,
    /// Number of vertebrae of the spine (0 = the synthetic slab spine).
    pub vertebrae: usize,
    bounds: (Vec3, Vec3),
}

/// A mesh in Bevy space -> skin units (Z up), `map` taking a Bevy vector to the skin's.
fn skin_units(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    scale: f32,
    map: fn([f32; 3]) -> Vec3,
) -> (Vec<Vec3>, Vec<Vec3>, Vec3, Vec3) {
    let inv = 1.0 / scale;
    let positions: Vec<Vec3> = positions.iter().map(|p| map(*p) * inv).collect();
    let normals: Vec<Vec3> = normals.iter().map(|n| map(*n).normalize_or(Vec3::Z)).collect();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &positions {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (positions, normals, lo, hi)
}

/// The body mesh -> skin units: the skin is the GMDL's own space (Z up; the cell's
/// front, vertebra 0 and the mouth, at -Y). GMDL space -> Bevy is (x, y, z) ->
/// (-x, z, y) and the player's body is fitted head first (no turn,
/// `cell_visuals::player_body`), so skin (x, y, z) = (-bx, bz, by) / scale. Until
/// cell-parts2 the fit also turned the body half a turn about Y (mapping skin
/// (bx, -bz, by)), which left the narrow, mouth end of the body at +Z while the mouth
/// is mounted at -Z; the skin's own coordinates are unchanged, so the baked atlas and
/// every paint of the body stay as they were.
fn body_to_skin(positions: &[[f32; 3]], normals: &[[f32; 3]], scale: f32) -> (Vec<Vec3>, Vec<Vec3>, Vec3, Vec3) {
    skin_units(positions, normals, scale, |p| Vec3::new(-p[0], p[2], p[1]))
}

/// A part model's mesh -> skin units, the mapping the part charts always used
/// ((x, y, z) -> (x, -z, y)): a part chart only needs some consistent skin frame,
/// and this one keeps every part's paint as painted before.
fn part_to_skin(positions: &[[f32; 3]], normals: &[[f32; 3]], scale: f32) -> (Vec<Vec3>, Vec<Vec3>, Vec3, Vec3) {
    skin_units(positions, normals, scale, |p| Vec3::new(p[0], -p[2], p[1]))
}

/// The spine of the body from its bone partition. VERIFIED data
/// (`notes\cell-animation-notes.md` section 4): the GMDL's body bone `i` is the
/// anatomy's rigblock `i` and every vertebra owns the vertices skinned to it;
/// `bones` is the dominant body bone of each vertex (`u8::MAX` = none). Each
/// vertebra's point is the middle of the bounds of its vertices (the mean of
/// the vertices is pulled sideways by the doubled vertices along the UV seams,
/// which tilts the spine's local frame) and its radius their mean distance from
/// it; the chain runs over the vertebrae from the +Y end (the flagellum end of the
/// anatomy). Returns the chain and the vertebra index of every vertex (the rank of
/// its bone; vertices without a bone take the nearest vertebra), `None` when
/// fewer than two vertebrae own vertices.
///
/// ADAPTED: the retail classifier (0x5241F0) derives the back / belly verdict
/// from the weighted vertebra up vectors; here `back` / `belly` use the angle
/// around this polyline, and the order of the vertebrae (which end is index 0) starts at
/// the +Y end. Retail's index is the rigblock order (`range` compares it with
/// `numVertebrae * range`, 0x5323B8): vertebra 0 is the anatomy's first vertebra at the
/// mouth end, -Y (cell-parts2: that end is the front), i.e. the last one here; no
/// installed Cell style depends on it (VERIFIED, `installed_cell_scripts_region_census`:
/// all 45 distributes carry the default `range` (0, 1), which the code skips).
fn vertebra_chain(positions: &[Vec3], bones: &[u8]) -> Option<(Chain, Vec<u8>)> {
    if bones.len() != positions.len() {
        return None;
    }
    let mut owned: std::collections::BTreeMap<u8, (Vec3, Vec3, usize)> = std::collections::BTreeMap::new();
    for (p, b) in positions.iter().zip(bones) {
        if *b != u8::MAX {
            let e = owned.entry(*b).or_insert((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0));
            e.0 = e.0.min(*p);
            e.1 = e.1.max(*p);
            e.2 += 1;
        }
    }
    let mut vertebrae: Vec<(u8, Vec3, usize)> = owned.into_iter().filter(|(_, (_, _, n))| *n >= 3).map(|(b, (lo, hi, n))| (b, (lo + hi) * 0.5, n)).collect();
    if vertebrae.len() < 2 {
        return None;
    }
    // Front (+Y) end first.
    if vertebrae.first().map(|v| v.1.y).unwrap_or(0.0) < vertebrae.last().map(|v| v.1.y).unwrap_or(0.0) {
        vertebrae.reverse();
    }
    let radii: Vec<f32> = vertebrae
        .iter()
        .map(|(b, c, n)| positions.iter().zip(bones).filter(|(_, k)| **k == *b).map(|(p, _)| (*p - *c).length()).sum::<f32>() / *n as f32)
        .collect();
    let rank = |p: Vec3, b: u8| -> u8 {
        vertebrae
            .iter()
            .position(|v| v.0 == b)
            .or_else(|| {
                vertebrae
                    .iter()
                    .enumerate()
                    .min_by(|x, y| (x.1 .1 - p).length_squared().total_cmp(&(y.1 .1 - p).length_squared()))
                    .map(|(i, _)| i)
            })
            .unwrap_or(0) as u8
    };
    let ranks = positions.iter().zip(bones).map(|(p, b)| rank(*p, *b)).collect();
    let points = vertebrae.iter().map(|v| v.1).collect();
    Some((Chain { points, radii, limb: false, up: Vec3::Z }, ranks))
}

impl BodySurface {
    /// `positions` / `normals` in the Bevy space of the fitted body mesh,
    /// `scale` Bevy units per skin unit (`cell_visuals::player_body`).
    /// `bones`: the dominant body bone of every vertex (`u8::MAX` = none), the
    /// body's partition into vertebrae ([`vertebra_chain`]); `None` or fewer
    /// than two vertebrae keep the synthetic 8-slab spine.
    pub fn build(
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        uvs: &[[f32; 2]],
        indices: &[u32],
        scale: f32,
        bones: Option<&[u8]>,
    ) -> Option<BodySurface> {
        if positions.len() < 3 || normals.len() != positions.len() || uvs.len() != positions.len() || scale <= 0.0 {
            return None;
        }
        let (positions, normals, lo, hi) = body_to_skin(positions, normals, scale);
        let real = bones.and_then(|b| vertebra_chain(&positions, b));
        let (chain, vertex_vertebra, vertebrae) = match real {
            Some((chain, ranks)) => {
                let n = chain.points.len();
                (chain, Some(ranks), n)
            }
            None => {
                // ADAPTED fallback: slab means along the longer horizontal axis,
                // front (+Y) first.
                let along_y = (hi.y - lo.y) >= (hi.x - lo.x);
                let axis = |p: Vec3| if along_y { p.y } else { p.x };
                let (a0, a1) = if along_y { (lo.y, hi.y) } else { (lo.x, hi.x) };
                const SLABS: usize = 8;
                let mut sums = vec![(Vec3::ZERO, 0.0f32); SLABS];
                for p in &positions {
                    let t = ((axis(*p) - a0) / (a1 - a0).max(1e-6) * SLABS as f32).floor().clamp(0.0, SLABS as f32 - 1.0) as usize;
                    sums[t].0 += *p;
                    sums[t].1 += 1.0;
                }
                let mut points: Vec<Vec3> = sums.iter().filter(|s| s.1 > 0.0).map(|s| s.0 / s.1).collect();
                if along_y {
                    points.reverse();
                } else if points.first().is_some_and(|p| p.x < points.last().map_or(0.0, |q| q.x)) {
                    points.reverse();
                }
                let radii: Vec<f32> = points
                    .iter()
                    .map(|c| {
                        let near: Vec<f32> = positions.iter().filter(|p| (axis(**p) - axis(*c)).abs() < (a1 - a0) / SLABS as f32).map(|p| (*p - *c).length()).collect();
                        near.iter().sum::<f32>() / near.len().max(1) as f32
                    })
                    .collect();
                (Chain { points, radii, limb: false, up: Vec3::Z }, None, 0)
            }
        };
        let prims = Primitives { chains: vec![chain], ..Default::default() };
        let triangles: Vec<[u32; 3]> = indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
        // Retail's triangle flags take the vertebra of the first corner.
        let vertebra: Vec<u8> = vertex_vertebra.as_ref().map_or_else(Vec::new, |v| triangles.iter().map(|t| v[t[0] as usize]).collect());
        let un = Unwrapped {
            normals,
            uvs: uvs.to_vec(),
            source: (0..positions.len() as u32).collect(),
            triangles,
            chart: vec![ChartId { chain: 0, side: 0 }; positions.len()],
            charts: vec![ChartId { chain: 0, side: 0 }],
            positions,
            texel_density: 0.0,
        };
        let mut map = paint::texel_map(&prims, &un);
        map.vertebra = vertebra;
        map.vertebrae = vertebrae as u32;
        (!map.samples.is_empty()).then_some(BodySurface { prims, map, extent: hi - lo, vertebrae, bounds: (lo, hi) })
    }

    /// The spine's points and radii (skin units), for logs.
    pub fn spine(&self) -> (&[Vec3], &[f32]) {
        let chain = &self.prims.chains[0];
        (&chain.points, &chain.radii)
    }

    /// The body's bounds in skin units, for logs.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.bounds
    }
}

/// One part's chart of the atlas: the part's own mesh as a paintable skin in
/// the part's UV space (triangles carry the parts flag 4 of the retail
/// classifier, so the distributes' `region` masks treat them as parts).
pub struct PartChart {
    prims: Primitives,
    map: paint::TexelMap,
}

impl PartChart {
    /// `positions` / `normals` in the Bevy space of the part model, `scale`
    /// Bevy units per skin unit (the part's size on the cell over the body's
    /// fit), `uvs` the part's texture coordinates.
    pub fn build(positions: &[[f32; 3]], normals: &[[f32; 3]], uvs: &[[f32; 2]], indices: &[u32], scale: f32) -> Option<PartChart> {
        if positions.len() < 3 || normals.len() != positions.len() || uvs.len() != positions.len() || scale <= 0.0 {
            return None;
        }
        let (positions, normals, lo, hi) = part_to_skin(positions, normals, scale);
        // The installed part meshes keep their V in -1 .. 0 (the importer samples the
        // texture with repeat addressing): a coordinate range that fits one unit is
        // shifted into 0 .. 1 so the chart lies where the texture does.
        let mut uvs = uvs.to_vec();
        for k in 0..2 {
            let (min, max) = uvs.iter().fold((f32::MAX, f32::MIN), |(a, b), u| (a.min(u[k]), b.max(u[k])));
            let shift = -min.floor();
            if shift != 0.0 && max + shift <= 1.0 + 1e-4 {
                for u in &mut uvs {
                    u[k] += shift;
                }
            }
        }
        // ADAPTED: the part's own bone frame is not mapped; a chain through the
        // middle of the mesh along its longest axis gives the brush alignments
        // a direction.
        let size = hi - lo;
        let axis = if size.x >= size.y && size.x >= size.z { Vec3::X } else if size.y >= size.z { Vec3::Y } else { Vec3::Z };
        let mid = (lo + hi) * 0.5;
        let half = axis * (size.dot(axis) * 0.5);
        let radius = (size - axis * size.dot(axis)).length() * 0.25;
        let chain = Chain { points: vec![mid - half, mid + half], radii: vec![radius; 2], limb: false, up: if axis == Vec3::Z { Vec3::Y } else { Vec3::Z } };
        let prims = Primitives { chains: vec![chain], ..Default::default() };
        let un = Unwrapped {
            normals,
            uvs,
            source: (0..positions.len() as u32).collect(),
            triangles: indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
            chart: vec![ChartId { chain: 0, side: 0 }; positions.len()],
            charts: vec![ChartId { chain: 0, side: 0 }],
            positions,
            texel_density: 0.0,
        };
        let mut map = paint::texel_map(&prims, &un);
        map.parts = true;
        (!map.samples.is_empty()).then_some(PartChart { prims, map })
    }
}

/// The painted body textures at [`PAINT_SIZE`] squared.
#[derive(Clone)]
pub struct BodyTextures {
    /// Diffuse, alpha 255 (display colours, as the shaders write them).
    pub diffuse: Vec<u8>,
    /// Tangent normal map from the bump layer; alpha = the specular level.
    pub normal_spec: Vec<u8>,
    /// The painter's `tex1` (R specular level, B bump height, A 255): the
    /// input of retail's `skpBumpToNormal` pass.
    pub bump_spec: Vec<u8>,
    pub stamps: usize,
    pub settings: paint::Settings,
}

impl BodyTextures {
    /// The `materialParams` of the painted body: (specE, specStrength, bump,
    /// gloss) = (exponent, phong x `skinpaintPhongMultiplier` 0.95, 1, gloss x
    /// `skinpaintGlossMultiplier` 0.7). VERIFIED by fit: every installed Cell
    /// GMDL material has specStrength in 0.95 x {0.5 .. 1.0}, gloss in
    /// 0.7 x {0 .. 0.9}, specE in 4 .. 60 (`census_cell_material_params`).
    pub fn material_params(&self) -> crate::cell_sh::MaterialParams {
        crate::cell_sh::MaterialParams {
            spec_e: self.settings.exponent.clamp(1.0, 60.0),
            spec_strength: self.settings.phong * 0.95,
            bump: 1.0,
            gloss: self.settings.gloss * 0.7,
        }
    }
}

/// Brush textures by name (`<name>__diffuse` / `<name>__specBump`).
pub fn load_brush(index: &ResourceIndex, name: &str) -> Brush {
    let tex = |suffix: &str| {
        let key = ResourceKey {
            type_id: paint::RASTER_TYPE,
            group_id: paint::BRUSH_GROUP,
            instance_id: name_id(&format!("{name}{suffix}")),
        };
        index
            .read(key)
            .ok()
            .and_then(|b| crate::texture::decode_rw4(&b))
            .map(|p| BrushTex { width: p.width, height: p.height, rgba: p.rgba })
    };
    Brush { diffuse: tex("__diffuse"), spec_bump: tex("__specBump") }
}

/// Run `script` (the style's brush effect, resource id `id`) in `color` on
/// the body.
pub fn paint_body(surface: &BodySurface, script: &Script, id: u32, color: [f32; 3], brushes: &HashMap<String, Brush>) -> BodyTextures {
    let job = paint::PaintJob { scripts: [Some((script, id)), None, None], colors: layer_colors(color), seeds: [SEED; 3], brushes };
    let map = &surface.map;
    let project = |p: Vec3| map.nearest(p);
    let painted = paint::paint_on(&surface.prims, map, &job, Some(&project));
    let mut normal_spec = painted.normal;
    for (px, spec) in normal_spec.chunks_exact_mut(4).zip(&painted.spec_map) {
        px[3] = *spec;
    }
    let bump_spec = painted.spec_map.iter().zip(&painted.bump_map).flat_map(|(s, b)| [*s, 0, *b, 255]).collect();
    BodyTextures { diffuse: painted.diffuse, normal_spec, bump_spec, stamps: painted.stamps, settings: painted.settings }
}

impl BodyTextures {
    /// Replace the painter's own bump -> normal derivation by retail's
    /// `skpBumpToNormal` (stage 12 of the pipeline, 0x9E74D163 = shader
    /// 0x700000B2), executed from the installed shader pack on the painter's
    /// `tex1` with the app's `skinpaintBumpHeight` (constants 1/w, 1/h,
    /// height / 3): the output carries the specular level (input R) in its
    /// alpha, as the retail normal map does. VERIFIED: the shader and its
    /// bindings are the installed program (`skinpaint.rs`, checked against
    /// Direct3D 9 pixels by `spatial_normal_conversion_matches_original_gpu_stencils`).
    pub fn with_native_normals(mut self, native: &crate::skinpaint::NativePainter) -> Self {
        let packed = crate::texture::Pixels { width: SIZE, height: SIZE, rgba: self.bump_spec.clone() };
        match native.normal_map(&packed, native.parameters.bump_height) {
            Ok(normal) if normal.rgba.len() == self.normal_spec.len() => self.normal_spec = normal.rgba,
            Ok(_) => bevy::log::warn!("Cell skin paint: skpBumpToNormal returned an unexpected size"),
            Err(e) => bevy::log::warn!("Cell skin paint: skpBumpToNormal failed: {e}"),
        }
        self
    }
}

// ---------------------------------------------------------------------------
// PaintParts
// ---------------------------------------------------------------------------

/// What lies under a part in the atlas before `PaintParts` splats it: the
/// script's first clear of the diffuse layer (`SPSkinPaintClear -diffuse`,
/// every Cell script starts with one: colour 1 or a literal colour), resolved
/// with the layer colours. A script without a clear leaves the cell's colour 1.
pub fn script_base_color(script: &Script, colors: [[f32; 3]; 3]) -> [f32; 3] {
    let clear = script.effect_order.iter().find_map(|name| {
        script.effects.get(name).and_then(|items| {
            items.iter().find_map(|item| match item {
                Item::Clear(c) => c.diffuse,
                _ => None,
            })
        })
    });
    match clear {
        Some(ColorRef::Layer(i)) => colors[i.min(2)],
        Some(ColorRef::Rgb(c)) => c,
        Some(ColorRef::Identity) | None => colors[0],
    }
}

/// Retail's `PaintParts` stage 0 for one part (`cSkinPainterJobPaintParts`,
/// pipeline 0x51AFE0 stage 4): the part's authored diffuse is tinted with the
/// three layer colours through its tint mask by the installed
/// `skpSplatTintShader` (0x700000AD: the shader output keeps the diffuse alpha)
/// and splatted over what the atlas holds at the part (`base`) with that alpha.
/// The shader runs from the installed pack (`skinpaint::NativePainter::tint`,
/// checked against Direct3D 9 pixels). `tint` = None: a mask of pure layer 1.
/// The result is opaque.
///
/// `under` is what lies below the part in retail's atlas: the brush stamps of
/// the script's distributes on the part's chart ([`paint_chart`], resampled
/// to the part's size); `None` (or a size mismatch) uses the flat clear colour
/// `base` of the script. One chart per mounted part instance
/// (`cell_paint::CellPaintAssets::instance_material`: retail has one rectangle
/// per rigblock instance, with that instance's scale; ADAPTED: the draws of an
/// instance are seeded apart, `paint_chart`, instead of running once over the
/// whole atlas); the layers 2 / 3 are the editor model's white (`MODEL_COLOR`).
pub fn paint_part(
    native: &crate::skinpaint::NativePainter,
    diffuse: &crate::texture::Pixels,
    tint: Option<&crate::texture::Pixels>,
    base: [f32; 3],
    under: Option<&crate::texture::Pixels>,
    colors: [[u8; 3]; 3],
) -> Result<crate::texture::Pixels, String> {
    let solid = crate::texture::Pixels { width: 1, height: 1, rgba: vec![255, 0, 0, 255] };
    let mut out = native.tint(diffuse, tint.unwrap_or(&solid), colors)?;
    let base = base.map(|c| c.clamp(0.0, 1.0) * 255.0);
    let under = under.filter(|u| u.width == out.width && u.height == out.height && u.rgba.len() == out.rgba.len());
    for (i, p) in out.rgba.chunks_exact_mut(4).enumerate() {
        let a = p[3] as f32 / 255.0;
        let below = under.map_or(base, |u| [u.rgba[i * 4] as f32, u.rgba[i * 4 + 1] as f32, u.rgba[i * 4 + 2] as f32]);
        for c in 0..3 {
            p[c] = (p[c] as f32 * a + below[c] * (1.0 - a)).round() as u8;
        }
        p[3] = 255;
    }
    Ok(out)
}

/// Run `script` (the style's brush effect, resource id `id`) in `color` on a
/// part's chart: the diffuse layer of the chart at [`PAINT_SIZE`] squared in
/// the part's own UV space, with every triangle flagged as a part (retail
/// `-parts` stamps, notes `cell-paint-notes.md`). The result lies under the
/// part's own diffuse in [`paint_part`].
///
/// `salt` is the instance's draw seed: retail runs the distributes once over the
/// whole atlas, so every rigblock instance's rectangle takes its own draws
/// (`FUN_004CB890` gives each instance a rectangle; the placement shuffle and the
/// `particleSelect` draws run through the shared generators). ADAPTED: each
/// chart here is painted on its own, and the instance's ordinal among the
/// instances of its definition (0 for the first) is mixed into the layer seed so
/// the draws differ per instance; 0 reproduces the one-chart-per-definition paint.
pub fn paint_chart(chart: &PartChart, script: &Script, id: u32, color: [f32; 3], brushes: &HashMap<String, Brush>, salt: u32) -> crate::texture::Pixels {
    let seed = SEED ^ salt.wrapping_mul(0x9E37_79B1);
    let job = paint::PaintJob { scripts: [Some((script, id)), None, None], colors: layer_colors(color), seeds: [seed; 3], brushes };
    let map = &chart.map;
    let project = |p: Vec3| map.nearest(p);
    let started = std::time::Instant::now();
    let painted = paint::paint_on(&chart.prims, map, &job, Some(&project));
    bevy::log::info!("Cell skin paint: part chart, {} stamps in {} ms", painted.stamps, started.elapsed().as_millis());
    crate::texture::Pixels { width: SIZE, height: SIZE, rgba: painted.diffuse }
}

/// Bilinear resample of an RGBA8 image (the atlas of the body may not be
/// [`PAINT_SIZE`] squared).
pub fn resample(rgba: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    if from == to {
        return rgba.to_vec();
    }
    let mut out = vec![0u8; (to.0 * to.1 * 4) as usize];
    for y in 0..to.1 {
        for x in 0..to.0 {
            let fx = ((x as f32 + 0.5) * from.0 as f32 / to.0 as f32 - 0.5).clamp(0.0, from.0 as f32 - 1.0);
            let fy = ((y as f32 + 0.5) * from.1 as f32 / to.1 as f32 - 0.5).clamp(0.0, from.1 as f32 - 1.0);
            let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(from.0 - 1), (y0 + 1).min(from.1 - 1));
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            for c in 0..4 {
                let at = |px: u32, py: u32| rgba[((py * from.0 + px) * 4) as usize + c] as f32;
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                out[((y * to.0 + x) * 4) as usize + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}

/// Texture size of the painter (re-exported for the caller's size checks).
pub const SIZE: u32 = PAINT_SIZE;

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use paint::ch;

    #[test]
    fn brush_names_hash_to_their_ids() {
        // The table is used through the hash; every name must be unique.
        let mut ids: Vec<u32> = BRUSH_NAMES.iter().map(|n| name_id(n)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BRUSH_NAMES.len());
        assert_eq!(brush_name(name_id("skn_brush_blankTooth")), "skn_brush_blankTooth");
        assert_eq!(brush_name(0x1234_5678), "#12345678");
    }

    #[test]
    fn motion_flags_follow_the_component_enum() {
        assert!(motion_of(0, [0.0; 3], false).is_none());
        assert_eq!(motion_of(7, [0.0; 3], true).map(|m| (m.kind, m.reverse)), Some((MotionKind::AroundSpine, true)));
        assert_eq!(motion_of(6, [0.0, 1.0, 0.0], false).map(|m| m.kind), Some(MotionKind::AlongSpine));
        assert_eq!(motion_of(4, [0.0, 1.0, 0.0], false).map(|m| m.kind), Some(MotionKind::AlongBone));
        // A vector in bone space is an attract direction.
        let m = motion_of(4, [1.0, 0.0, 0.0], false).unwrap();
        assert_eq!((m.kind, m.vector), (MotionKind::Attract, Vec3::X));
    }

    #[test]
    fn variables_convert_by_modifier_kind() {
        let raw = RawVars {
            modifiers: vec![
                RawModifier { v: [0.0; 3], kind: -1, count: 0, index: 0 },
                RawModifier { v: [0.0; 3], kind: -1, count: 0, index: 0 },
                // constant curve: values [0.2], vary 0.2
                RawModifier { v: [0.2, 0.0, 0.0], kind: -1, count: 1, index: 0 },
                // random source
                RawModifier { v: [0.0; 3], kind: 1, count: 19, index: 0 },
            ],
            variables: vec![
                RawVariable { range: [0.0, 1.0], values: [0.25, 1.0], channel: 6, modifier: -1, op: 0, flag: 0 },
                RawVariable { range: [0.0, 1.0], values: [0.0, 0.0], channel: 0, modifier: 2, op: 2, flag: 0 },
                RawVariable { range: [0.0, 0.5], values: [0.0, 360.0], channel: 2, modifier: 3, op: 1, flag: 3 },
            ],
            values: vec![0.2],
        };
        let (mods, vars) = convert_vars(&raw);
        assert_eq!(mods.len(), 4);
        assert!(matches!(mods[3], Source::Random));
        assert_eq!(vars.len(), 3);
        // alpha 0.25 (set, no curve)
        assert_eq!((vars[0].channel, vars[0].op, vars[0].curve, vars[0].values.clone()), (ch::ALPHA, Op::Set, false, vec![0.25]));
        // size: curve of one value with vary, multiplied
        assert_eq!((vars[1].channel, vars[1].op, vars[1].curve, vars[1].vary), (ch::SIZE, Op::Mult, true, 0.2));
        // rotation: random modifier, add 0..360 over range 0..0.5, wrapped
        assert_eq!((vars[2].channel, vars[2].op, vars[2].modifier, vars[2].range, vars[2].flag), (ch::ROTATION, Op::Add, Some(3), (0.0, 0.5), RangeFlag::Wrap));
    }

    #[test]
    fn literal_colours_read_as_hsv_and_the_params_follow_the_installed_fit() {
        assert_eq!(color_of(-1, [0.0, 0.0, 1.0]), ColorRef::Rgb([1.0, 1.0, 1.0]));
        assert_eq!(color_of(-1, [0.0, 0.0, 0.9]), ColorRef::Rgb([0.9, 0.9, 0.9]));
        assert_eq!(color_of(1, [0.0; 3]), ColorRef::Layer(1));
        assert_eq!(color_of(-2, [0.0; 3]), ColorRef::Identity);
        // blankMatte (exponent 60, default gloss 0.5 / phong 1) is the installed
        // (60, 0.95, 1, 0.35) material.
        let t = BodyTextures { diffuse: vec![], normal_spec: vec![], bump_spec: vec![], stamps: 0, settings: paint::Settings { exponent: 60.0, phong: 1.0, gloss: 0.5 } };
        let p = t.material_params();
        assert_eq!((p.spec_e, p.spec_strength, p.bump, p.gloss), (60.0, 0.95, 1.0, 0.35));
        let clamp = BodyTextures { settings: paint::Settings { exponent: 500.0, phong: 0.5, gloss: 0.0 }, ..t };
        assert_eq!(clamp.material_params().spec_e, 60.0);
    }

    #[test]
    fn settings_flags_become_clear_and_material_items() {
        // blankMatte's component: flags 0xF (diffuse, spec, bump, exponent).
        let s = RawSettings { color_index: 0, color: [0.0; 3], spec_bump: [0.02, 1.0, 0.5], gloss: 0.5, phong: 1.0, flags: 0xF };
        let items = settings_items(&s);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Item::Clear(c) if c.diffuse == Some(ColorRef::Layer(0)) && c.spec == Some(0.02) && c.bump == Some(0.5)));
        assert!(matches!(&items[1], Item::Settings { exponent: Some(e), phong: None, gloss: None } if (*e - 60.0).abs() < 1e-4));
        // blankShiny's second component: only gloss + phong + exponent.
        let m = RawSettings { color_index: -1, color: [0.0; 3], spec_bump: [0.0, 0.5, 0.5], gloss: 0.1, phong: 1.0, flags: 0x608 };
        let items = settings_items(&m);
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], Item::Settings { exponent: Some(e), phong: Some(_), gloss: Some(g) } if (*e - 30.0).abs() < 1e-3 && (*g - 0.1).abs() < 1e-6));
    }

    /// A UV-mapped ellipsoid in Bevy space (one chart: u = azimuth, v = polar).
    fn ellipsoid(scale: f32) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
        let (rings, segs) = (24usize, 48usize);
        let (mut p, mut n, mut uv, mut ix) = (vec![], vec![], vec![], vec![]);
        for i in 0..=rings {
            let phi = std::f32::consts::PI * i as f32 / rings as f32;
            for j in 0..=segs {
                let th = std::f32::consts::TAU * j as f32 / segs as f32;
                let d = Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                // Long along Bevy Z (the cell's front is -Z), 0.4 x 0.3 skin units across.
                let q = Vec3::new(d.x * 0.2, d.y * 0.12, d.z * 0.5) * scale;
                p.push(q.to_array());
                n.push(d.normalize().to_array());
                uv.push([j as f32 / segs as f32 * 0.98 + 0.01, i as f32 / rings as f32 * 0.98 + 0.01]);
            }
        }
        for i in 0..rings {
            for j in 0..segs {
                let a = (i * (segs + 1) + j) as u32;
                let b = a + segs as u32 + 1;
                ix.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        (p, n, uv, ix)
    }

    #[test]
    fn a_synthetic_script_paints_the_body_and_the_spec_alpha() {
        let (p, n, uv, ix) = ellipsoid(2.0);
        let surface = BodySurface::build(&p, &n, &uv, &ix, 2.0, None).expect("surface");
        assert!(surface.extent.y > 0.9 && surface.extent.z < 0.3, "{:?}", surface.extent);
        // A clear to colour 1 with spec 0.4, then one stamp layer without a
        // brush texture (a round dot) that raises the spec on its dots.
        let text = "SPSkinPaintParticle dots
  alpha 1
  size 0.1
  diffuseColor color1
  diffuseAlpha 1
  specularAlpha 1
  specularScale 0.9
  bumpAlpha 0
  delay 0 1
end
SPSkinPaintDistribute dist
  particle dots
  spacing 0.06
end
effect clear
  SPSkinPaintClear -diffuse color1 -spec 0.1 -bump 0.5
end
effect entry
  effect clear
  SPSkinPaintDistribute dist
end";
        let script = paint::parse(text);
        let out = paint_body(&surface, &script, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new());
        assert!(out.stamps > 100, "{}", out.stamps);
        assert_eq!(out.diffuse.len(), (SIZE * SIZE * 4) as usize);
        // Texels carry the paint colour (dots and the cleared body, same colour).
        let texel = out.diffuse.chunks_exact(4).find(|p| p[0] > 100).expect("painted texel");
        assert_eq!((texel[0], texel[1], texel[2], texel[3]), (204, 102, 51, 255));
        // The spec alpha holds both the cleared level and the dots.
        let alphas: Vec<u8> = out.normal_spec.chunks_exact(4).map(|p| p[3]).collect();
        assert!(alphas.iter().any(|a| (*a as i32 - 26).abs() <= 2), "cleared level 0.1");
        assert!(alphas.iter().any(|a| *a > 200), "dots 0.9");
        // Flat bump: the normal is straight up.
        assert!(out.normal_spec.chunks_exact(4).any(|p| p[2] > 250));
    }

    #[test]
    fn the_spine_is_the_vertebra_partition_of_the_body_front_first() {
        // Three vertebrae of four vertices each, bone 0 at the back (-Y).
        let mut positions = Vec::new();
        let mut bones = Vec::new();
        for (bone, y) in [(0u8, -0.5f32), (1, 0.0), (2, 0.5)] {
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2;
                positions.push(Vec3::new(0.2 * a.cos(), y, 0.2 * a.sin()));
                bones.push(bone);
            }
        }
        let (chain, ranks) = vertebra_chain(&positions, &bones).expect("three vertebrae");
        assert_eq!(chain.points.len(), 3);
        // Front (+Y) first: vertebra 0 is the one at y = 0.5 (bone 2).
        assert!((chain.points[0].y - 0.5).abs() < 1e-5 && (chain.points[2].y + 0.5).abs() < 1e-5);
        assert_eq!(ranks[0], 2, "bone 0 (back) is the last vertebra");
        assert_eq!(ranks[8], 0, "bone 2 (front) is vertebra 0");
        assert!(chain.radii.iter().all(|r| (r - 0.2).abs() < 1e-4));
        // One vertebra, or vertices without a bone, give no spine.
        assert!(vertebra_chain(&positions[..4], &bones[..4]).is_none());
        assert!(vertebra_chain(&positions, &vec![u8::MAX; 12]).is_none());
    }

    #[test]
    fn a_part_chart_takes_the_style_stamps_in_its_own_uv_space() {
        // A flat 0.4 x 0.4 part (Bevy x, z; y up) with its texture over the square, UV 0.1 .. 0.9.
        let n = 10usize;
        let (mut p, mut nr, mut uv, mut ix) = (vec![], vec![], vec![], vec![]);
        for j in 0..=n {
            for i in 0..=n {
                let (fx, fz) = (i as f32 / n as f32, j as f32 / n as f32);
                p.push([fx * 0.4 - 0.2, 0.0, fz * 0.4 - 0.2]);
                nr.push([0.0, 1.0, 0.0]);
                uv.push([0.1 + 0.8 * fx, 0.1 + 0.8 * fz]);
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                let b = a + n as u32 + 1;
                ix.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        let chart = PartChart::build(&p, &nr, &uv, &ix, 1.0).expect("chart");
        assert!(chart.map.parts);
        // Layer-2 (white) dots over a layer-1 clear.
        let text = "SPSkinPaintParticle dots
  alpha 1
  size 0.08
  diffuseColor color2
  diffuseAlpha 1
  specularAlpha 0
  bumpAlpha 0
  delay 0 1
end
SPSkinPaintDistribute dist
  particle dots
  spacing 0.06
end
effect clear
  SPSkinPaintClear -diffuse color1
end
effect entry
  effect clear
  SPSkinPaintDistribute dist
end";
        let script = paint::parse(text);
        let out = paint_chart(&chart, &script, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new(), 0);
        assert_eq!((out.width, out.height), (SIZE, SIZE));
        let white = out.rgba.chunks_exact(4).filter(|px| px[0] > 240 && px[1] > 240 && px[2] > 240).count();
        let base = out.rgba.chunks_exact(4).filter(|px| px[..3] == [204, 102, 51]).count();
        assert!(white > 5_000, "stamps cover the chart: {white}");
        assert!(base > 5_000, "the border outside the chart keeps the clear colour: {base}");
        // Part meshes keep their V in -1 .. 0: the chart is shifted into the texture.
        let negative: Vec<[f32; 2]> = uv.iter().map(|u| [u[0], u[1] - 1.0]).collect();
        let shifted = PartChart::build(&p, &nr, &negative, &ix, 1.0).expect("shifted chart");
        let again = paint_chart(&shifted, &script, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new(), 0);
        assert!(again.rgba.chunks_exact(4).filter(|px| px[0] > 240 && px[1] > 240 && px[2] > 240).count() > 5_000);
        // The retail `region` mask of a torso-only distribute leaves a part chart bare.
        let torso = paint::parse(&text.replace("spacing 0.06", "spacing 0.06
  region -torso"));
        let bare = paint_chart(&chart, &torso, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new(), 0);
        assert_eq!(bare.rgba.chunks_exact(4).filter(|px| px[0] > 240 && px[1] > 240).count(), 0);
        // Another instance of the same part draws its own stamps (its salt seeds the draws), the
        // same instance reproduces its paint.
        let second = paint_chart(&chart, &script, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new(), 1);
        assert_ne!(second.rgba, out.rgba, "the second instance takes other draws");
        assert_eq!(paint_chart(&chart, &script, name_id("entry"), [0.8, 0.4, 0.2], &HashMap::new(), 1).rgba, second.rgba);
    }

    #[test]
    fn the_body_skin_frame_is_the_gmdls_own_under_the_head_first_fit() {
        // GMDL (x, y, z) -> Bevy (-x, z, y) -> head-first fit (scale s, translation t, no turn):
        // the body's skin units are the GMDL coordinates again.
        let (s, t) = (2.5f32, Vec3::new(0.1, -0.3, 0.2));
        let g = [Vec3::new(0.3, -0.4, 0.1), Vec3::new(-0.2, 0.25, 0.35)];
        let bevy: Vec<[f32; 3]> = g.iter().map(|p| (t + Vec3::new(-p.x, p.z, p.y) * s).to_array()).collect();
        let normals: Vec<[f32; 3]> = g.iter().map(|p| Vec3::new(-p.x, p.z, p.y).normalize().to_array()).collect();
        // The skin frame is anchored at the fit's translation: the caller maps the mesh with its
        // own translation, `body_to_skin` takes the centred mesh.
        let centred: Vec<[f32; 3]> = bevy.iter().map(|p| (Vec3::from_array(*p) - t).to_array()).collect();
        let (skin, n, _, _) = body_to_skin(&centred, &normals, s);
        for (a, b) in skin.iter().zip(&g) {
            assert!((*a - *b).length() < 1e-5, "{a:?} {b:?}");
        }
        assert!((n[0] - g[0].normalize()).length() < 1e-5);
        // A part chart keeps its own mapping.
        let (part, _, _, _) = part_to_skin(&[[1.0, 2.0, 3.0]], &[[0.0, 1.0, 0.0]], 1.0);
        assert_eq!(part[0], Vec3::new(1.0, -3.0, 2.0));
    }

    #[test]
    fn resample_keeps_equal_sizes_and_averages_down() {
        let img: Vec<u8> = (0..16).flat_map(|i| [i * 16, 0, 0, 255]).collect();
        assert_eq!(resample(&img, (4, 4), (4, 4)), img);
        let small = resample(&img, (4, 4), (2, 2));
        assert_eq!(small.len(), 16);
        assert!(small[0] > 0 && small[0] < small[12], "{small:?}");
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_cell_brush_scripts_decode_into_runnable_scripts() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = Library::load(&index).expect("editors directory");
        let data = crate::cell_paint::CellPaintData::load(&index);
        assert_eq!(data.themes.len(), 24);
        let mut total_brushes = std::collections::BTreeSet::new();
        for theme in data.themes.values() {
            let id = *theme.paints.first().unwrap_or_else(|| panic!("{}: no script", theme.name));
            let script = lib.script(id).unwrap_or_else(|| panic!("{}: {id:08X} not exported", theme.name));
            assert!(script.unknown.is_empty(), "{}: {:?}", theme.name, script.unknown);
            assert!(script.entry(id).is_some(), "{}", theme.name);
            assert!(!script.distributes.is_empty(), "{}", theme.name);
            for name in script.brushes() {
                assert!(!name.starts_with('#'), "{}: unnamed brush {name}", theme.name);
                let brush = load_brush(&index, &name);
                assert!(brush.diffuse.is_some(), "{}: brush {name} has no diffuse raster", theme.name);
                total_brushes.insert(name);
            }
        }
        // blankMatte: clear (spec 0.02, bump 0.5), two distributes of 0.05 spacing.
        let blank = data.theme(data.default_theme).unwrap();
        assert_eq!(blank.name, "skinpaint_cell_blankMatte");
        let id = blank.paints[0];
        assert_eq!(id, name_id("skinpaint_cell_blankMatte"));
        let script = lib.script(id).unwrap();
        assert_eq!(script.distributes.len(), 2);
        assert!(script.distributes.values().all(|d| (d.spacing - 0.05).abs() < 1e-6));
        let items = &script.effects[script.entry(id).unwrap()];
        assert!(items.iter().any(|i| matches!(i, Item::Effect(_))), "{items:?}");
        let first = script.particles.values().find(|p| p.brushes == ["skn_brush_blankTooth"]).expect("blankTooth stamp");
        assert_eq!((first.delay.0, first.delay.1), (0.0, 1.0));
        // The table names exactly the brushes the 24 styles use.
        let table: std::collections::BTreeSet<String> = BRUSH_NAMES.iter().map(|n| n.to_string()).collect();
        assert_eq!(table, total_brushes);
    }

    /// Which retail distribute options the 24 Cell styles depend on (the
    /// options whose runtime semantics are still ADAPTED in the painter).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_cell_scripts_option_census() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = Library::load(&index).expect("editors directory");
        let data = crate::cell_paint::CellPaintData::load(&index);
        let (mut cover, mut center, mut ordered, mut limited, mut selected, mut total) = (0, 0, 0, 0, 0, 0);
        let mut rows = Vec::new();
        for theme in data.themes.values() {
            let script = lib.script(theme.paints[0]).expect("script");
            let (mut c, mut m, mut o) = (0, 0, 0);
            for (name, d) in &script.distributes {
                let raw = &lib.components.distributes[name[1..].parse::<usize>().expect("d<index>")];
                total += 1;
                c += raw.cover as usize;
                m += raw.center_only as usize;
                o += raw.ordered as usize;
                limited += (raw.limit >= 0) as usize;
                selected += (!d.select.is_empty()) as usize;
            }
            cover += c;
            center += m;
            ordered += o;
            let layers = [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9]];
            let base = script_base_color(&script, layers);
            let under = layers.iter().position(|l| *l == base).map_or_else(|| format!("literal {base:?}"), |i| format!("colour {}", i + 1));
            rows.push(format!(
                "{:<34} distributes {} cover {c} centerOnly {m} ordered {o}, parts rest on {under}",
                theme.name,
                script.distributes.len()
            ));
        }
        println!("{}", rows.join("
"));
        println!("TOTAL distributes {total}: cover {cover}, centerOnly {center}, ordered {ordered}, limit {limited}, particleSelect {selected}");
    }

    /// Which `region` masks the distributes of the 24 Cell styles carry (the
    /// bit 4 `-parts` ones stamp the parts' charts in retail).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_cell_scripts_region_census() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = Library::load(&index).expect("editors directory");
        let data = crate::cell_paint::CellPaintData::load(&index);
        let mut by_mask: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for theme in data.themes.values() {
            let script = lib.script(theme.paints[0]).expect("script");
            let mut row = Vec::new();
            for (name, d) in &script.distributes {
                let raw = &lib.components.distributes[name[1..].parse::<usize>().expect("d<index>")];
                let mask = if raw.region == -1 { "all".to_string() } else { format!("{:#x}", raw.region) };
                *by_mask.entry(mask.clone()).or_default() += 1;
                row.push(format!("{mask}/sp{:.2}/lim{}/range{:?}/{}", d.spacing, raw.limit, d.body_range, d.particle));
            }
            println!("{:<34} {}", theme.name, row.join("  "));
        }
        println!("REGION MASKS: {by_mask:?}");
    }

    /// Which layer colours and particle selections the 24 Cell styles use
    /// (layers 2 / 3 are the editor model's white in retail).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_cell_scripts_colour_and_select_census() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = Library::load(&index).expect("editors directory");
        let data = crate::cell_paint::CellPaintData::load(&index);
        let mut rows = Vec::new();
        let mut layer_use = [0usize; 3];
        let mut identity_use = 0usize;
        for theme in data.themes.values() {
            let script = lib.script(theme.paints[0]).expect("script");
            let mut used = [0usize; 3];
            let mut note = |c: Option<ColorRef>| match c {
                Some(ColorRef::Layer(i)) => used[i.min(2)] += 1,
                Some(ColorRef::Identity) => identity_use += 1,
                _ => {}
            };
            for items in script.effects.values() {
                for item in items {
                    if let Item::Clear(c) = item {
                        note(c.diffuse);
                    }
                }
            }
            for p in script.particles.values().chain(script.floods.values()) {
                note(p.color);
            }
            for i in 0..3 {
                layer_use[i] += used[i];
            }
            let selects: Vec<String> = script
                .distributes
                .values()
                .filter(|d| !d.select.is_empty())
                .map(|d| format!("{}{:?}", if d.select_all { "all" } else { "one" }, d.select.iter().map(|s| s.1).collect::<Vec<_>>()))
                .collect();
            rows.push(format!("{:<34} colour uses {used:?} select {selects:?}", theme.name));
        }
        println!("{}", rows.join("
"));
        println!("TOTAL colour uses (layer 1, 2, 3): {layer_use:?}, identity (-2) uses: {identity_use}");
        let mut forces: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for p in &lib.components.particles {
            let key = format!(
                "life {:.2}..{:.2} initDir {} attract {} force {:.3} align {}",
                p.life[0], p.life[1], p.init_dir_flag, p.attract_flag, p.attract_force, p.align_flag
            );
            *forces.entry(key).or_default() += 1;
        }
        for (k, n) in &forces {
            println!("PARTICLE x{n}: {k}");
        }
    }

    #[test]
    fn the_script_base_is_its_first_diffuse_clear() {
        let text = "effect clear
  SPSkinPaintClear -diffuse color2 -spec 0.1
end
effect later
  SPSkinPaintClear -diffuse color3
end
effect entry
  effect clear
  effect later
end";
        let script = paint::parse(text);
        let colors = [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9]];
        assert_eq!(script_base_color(&script, colors), colors[1]);
        let none = paint::parse("effect entry
end");
        assert_eq!(script_base_color(&none, colors), colors[0]);
    }

    #[test]
    #[ignore = "requires the installed original Spore data (pixel shader packs)"]
    fn installed_shaders_paint_parts_and_derive_normals() {
        use crate::texture::Pixels;
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let native = crate::skinpaint::load(&index).expect("installed pixel shaders");
        // PaintParts: a zero tint mask keeps the authored diffuse; diffuse
        // alpha 0 shows what lies under the part; a full layer-1 mask on an
        // opaque texel takes the layer colour.
        let diffuse = Pixels { width: 3, height: 1, rgba: vec![128, 128, 128, 255, 10, 200, 30, 0, 128, 128, 128, 255] };
        let mask = Pixels { width: 3, height: 1, rgba: vec![0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255] };
        let out = paint_part(&native, &diffuse, Some(&mask), [1.0, 0.5, 0.0], None, [[200, 100, 50]; 3]).expect("paint_part");
        assert!(out.rgba[..3].iter().all(|v| v.abs_diff(128) <= 1), "{:?}", &out.rgba[..4]);
        assert_eq!(&out.rgba[4..8], &[255, 128, 0, 255], "alpha 0 shows the base colour");
        assert!(out.rgba[8].abs_diff(200) < 40 && out.rgba[9].abs_diff(100) < 40, "layer colour: {:?}", &out.rgba[8..12]);
        assert!(out.rgba.chunks_exact(4).all(|p| p[3] == 255));
        // skpBumpToNormal: a flat bump is straight up, a height step tilts the normal
        // away from flat, and the alpha is the specular level (input R).
        let n = (SIZE * SIZE) as usize;
        let mut bump_spec = vec![0u8; n * 4];
        for y in 0..SIZE as usize {
            for x in 0..SIZE as usize {
                let i = (y * SIZE as usize + x) * 4;
                // Flat in the upper half; a step in height (64 -> 192) across x = 256 below.
                let height = if y < SIZE as usize / 2 { 128 } else if x < SIZE as usize / 2 { 64 } else { 192 };
                bump_spec[i..i + 4].copy_from_slice(&[77, 0, height, 255]);
            }
        }
        let started = std::time::Instant::now();
        let textures = BodyTextures {
            diffuse: vec![],
            normal_spec: vec![0; n * 4],
            bump_spec,
            stamps: 0,
            settings: paint::Settings::default(),
        }
        .with_native_normals(&native);
        println!("skpBumpToNormal over {SIZE}x{SIZE}: {} ms", started.elapsed().as_millis());
        let px = |x: usize, y: usize| {
            let i = (y * SIZE as usize + x) * 4;
            [textures.normal_spec[i], textures.normal_spec[i + 1], textures.normal_spec[i + 2], textures.normal_spec[i + 3]]
        };
        let flat = px(100, 50);
        assert!(flat[0].abs_diff(128) <= 2 && flat[1].abs_diff(128) <= 2 && flat[2] >= 253, "flat: {flat:?}");
        // Height rises towards +x at the step: the normal leans to -x (R below 128).
        let edge = px(255, 400);
        assert!(edge[0] < 100 && edge[1].abs_diff(128) <= 4, "step edge: {edge:?}");
        assert!([flat[3], edge[3]].iter().all(|a| *a == 77), "alpha is the specular level");
        println!("flat {flat:?} step(+x) {edge:?}");
    }
}
