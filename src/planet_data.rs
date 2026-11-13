//! Installed planet data (read-only): the PlanetScripts list, the per-planet
//! terrain script and its theme chain, the terrain brush library of the
//! `planet` effect directory and the brush rasters.
//!
//! Data chain (`notes/planet-re-notes.md` 2A-2C, VERIFIED keys):
//! `PlanetScripts~` 0xE38475D9 (terrainscript, FirstGroup, excludeFromGGE)
//! -> `planetTerrainScripts~` 0x4184A100 (fallback 0x4184AC00; parent -> theme
//! -> AllPlanets, resolved by `ResourceIndex::properties`) and the effect
//! directory `gameEffects_1~!planet` 0x40450200!0x5F848353 (spBrush
//! components). The EFFDIR layout follows SporeModder-FX (EffectDirectory,
//! VisualEffect, VisualEffectBlock, BrushEffect), as ported from
//! `planet-re/tools/effdir.py`.
use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey, prop::PropertyList};
use bevy::math::Vec3;
use std::collections::HashMap;

pub const PLANET_SCRIPTS_GROUP: u32 = 0xE384_75D9;
pub const TERRAIN_SCRIPTS_GROUP: u32 = 0x4184_A100;
pub const TERRAIN_SCRIPTS_FALLBACK_GROUP: u32 = 0x4184_AC00;
pub const EFFDIR_TYPE: u32 = 0xEA51_18B0;
pub const PLANET_EFFDIR: ResourceKey = ResourceKey {
    group_id: 0x4045_0200,
    instance_id: 0x5F84_8353,
    type_id: EFFDIR_TYPE,
};

// PlanetScripts properties.
const P_DESCRIPTION: u32 = 0x121A_6DCA;
const P_TERRAINSCRIPT: u32 = 0x6660_3A49;
const P_FIRST_GROUP: u32 = 0xB579_08C6;
const P_EXCLUDE_FROM_GGE: u32 = 0xEA28_8A88;
// Terrain script properties (2B).
const P_MODEL_EFFECT: u32 = 0x02A9_07B5;
const P_MODEL_EFFECT_TRANSFORMS: u32 = 0x02A9_07B6;
const P_MODEL_EFFECT_SEED: u32 = 0x02A9_07B7;
const P_EFFECT_SEEDS: u32 = 0x03DD_8E59;
const P_EFFECT_INTENSITIES: u32 = 0x03DD_8E5A;
const P_EFFECT_TIMES: u32 = 0x03A2_3F97;
/// Per-entry water level read by the `terrainScript` player (table 0x15B1028,
/// planet-blend notes 1); absent from the installed scripts.
const P_EFFECT_WATER_LEVELS: u32 = 0x7A6A_A70B;
const P_PLANET_INFO: u32 = 0x03A2_3F99;
const P_WTA_LEVELS: u32 = 0x03A2_3F98;

/// One `PlanetScripts~` entry (one planet type).
#[derive(Clone, Debug, PartialEq)]
pub struct PlanetScript {
    pub key: ResourceKey,
    pub description: String,
    pub terrain_script: u32,
    pub first_group: bool,
    pub exclude_from_gge: bool,
}

/// Every installed PlanetScript, sorted by instance id (deterministic order).
pub fn planet_scripts(index: &ResourceIndex) -> Vec<PlanetScript> {
    let mut keys: Vec<ResourceKey> = index
        .records
        .keys()
        .filter(|k| k.group_id == PLANET_SCRIPTS_GROUP && k.type_id == crate::asset_catalog::PROP)
        .copied()
        .collect();
    keys.sort_by_key(|k| k.instance_id);
    keys.into_iter()
        .filter_map(|key| {
            let props = index.properties(key).ok()?;
            let flag = |id| props.0.get(&id).and_then(|p| p.number()).unwrap_or(0.0) != 0.0;
            Some(PlanetScript {
                key,
                description: props
                    .0
                    .get(&P_DESCRIPTION)
                    .and_then(|p| p.text())
                    .map(|t| t.2)
                    .unwrap_or_default(),
                terrain_script: props.0.get(&P_TERRAINSCRIPT).and_then(u32_value)?,
                first_group: flag(P_FIRST_GROUP),
                exclude_from_gge: flag(P_EXCLUDE_FROM_GGE),
            })
        })
        .collect()
}

/// The 15 FirstGroup families a new galaxy draws the homeworld from
/// (0xBB8C90 -> 0xBAC7B0(1)).
pub fn first_group(index: &ResourceIndex) -> Vec<PlanetScript> {
    planet_scripts(index)
        .into_iter()
        .filter(|s| s.first_group)
        .collect()
}

/// ADAPTED: retail draws the homeworld from the FirstGroup list with
/// `Math::sMainRNG.RandomInt(n)` (0xBB61C0) at galaxy creation; OpenSpore has
/// no galaxy RNG state yet, so the save's world seed picks the entry. An
/// `OPENSPORE_PLANET=<description or terrain script hex>` override exists for
/// development captures.
pub fn choose_homeworld(scripts: &[PlanetScript], seed: u64) -> Option<&PlanetScript> {
    if let Ok(want) = std::env::var("OPENSPORE_PLANET") {
        let want = want.trim().to_lowercase();
        if let Some(s) = scripts.iter().find(|s| {
            s.description.to_lowercase() == want
                || format!("{:08x}", s.terrain_script) == want.trim_start_matches("0x")
        }) {
            return Some(s);
        }
    }
    if scripts.is_empty() {
        return None;
    }
    let mixed = seed ^ (seed >> 29) ^ (seed >> 41);
    scripts.get((mixed % scripts.len() as u64) as usize)
}

/// A modelEffectTransforms entry (PROP type 0x38): rows of the local frame
/// (row 2 is the outward normal at the stamp), offset at radius ~500.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntryTransform {
    pub offset: Vec3,
    pub scale: f32,
    pub rows: [Vec3; 3],
}

impl EntryTransform {
    pub const IDENTITY: Self = Self {
        offset: Vec3::ZERO,
        scale: 1.0,
        rows: [Vec3::X, Vec3::Y, Vec3::Z],
    };
    fn parse(bytes: &[u8]) -> Option<Self> {
        let f = |i: usize| {
            bytes
                .get(4 + i * 4..8 + i * 4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        };
        let v = |i: usize| Some(Vec3::new(f(i)?, f(i + 1)?, f(i + 2)?));
        let scale = f(3)?;
        Some(Self {
            offset: v(0)?,
            scale: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 },
            rows: [v(4)?, v(7)?, v(10)?],
        })
    }
    /// Local vector -> planet space (rows are the local axes).
    pub fn apply_dir(&self, local: Vec3) -> Vec3 {
        self.rows[0] * local.x + self.rows[1] * local.y + self.rows[2] * local.z
    }
}

/// One terrain-script entry: an effect stamped at a transform.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainEntry {
    /// Effect name hash (`modelEffect` key instance, e.g. `BR_continent_small~`).
    pub effect: u32,
    pub transform: EntryTransform,
    pub seed: u32,
    pub intensity: f32,
    pub time: f32,
}

/// The decoded terrain script of one planet.
#[derive(Clone, Debug)]
pub struct PlanetDescriptor {
    pub script: ResourceKey,
    pub seed: u32,
    pub entries: Vec<TerrainEntry>,
    /// terrainScriptWTALevels (Water, Temperature, Atmosphere), default 0.5.
    pub wta: [f32; 3],
    /// terrainScriptPlanetInfo (ramp x, ramp y, radius 500).
    pub planet_info: [f32; 3],
    pub properties: PropertyList,
    /// `terrainScriptEffectSeeds` has one seed per entry: the player reseeds
    /// the main RNG with it before each entry (0xF95590). Otherwise the RNG
    /// runs on from the planet seed.
    pub reseed_entries: bool,
    /// Per-entry water levels (0x7A6AA70B), empty when absent.
    pub entry_water_levels: Vec<f32>,
}

impl PlanetDescriptor {
    pub fn radius(&self) -> f32 {
        if self.planet_info[2] > 0.0 {
            self.planet_info[2]
        } else {
            crate::planet_query::PLANET_RADIUS
        }
    }
    /// waterLevelIsZero (0xF4B830 / 0xF67880): brush levels are relative to
    /// h_w = 2W - 1, so mapset mWaterLevel = 2 * WTA.x - 1 (INFERRED).
    pub fn water_level(&self) -> f32 {
        2.0 * self.wta[0] - 1.0
    }
}

fn u32_value(p: &crate::prop::Property) -> Option<u32> {
    if p.kind != 10 && p.kind != 9 {
        return None;
    }
    let v = p.values.first()?;
    Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?))
}

fn f32_list(props: &PropertyList, id: u32) -> Vec<f32> {
    props.0.get(&id).map_or_else(Vec::new, |p| {
        p.values
            .iter()
            .filter_map(|v| Some(f32::from_be_bytes(v.get(..4)?.try_into().ok()?)))
            .collect()
    })
}
fn u32_list(props: &PropertyList, id: u32) -> Vec<u32> {
    props.0.get(&id).map_or_else(Vec::new, |p| {
        p.values
            .iter()
            .filter_map(|v| Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?)))
            .collect()
    })
}
fn vec3_of(props: &PropertyList, id: u32) -> Option<[f32; 3]> {
    let v = props.0.get(&id)?.values.first()?;
    v.get(..12)?;
    Some([0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
}

/// Load the terrain script `instance` (group 0x4184A100, else 0x4184AC00),
/// with its theme chain resolved through property inheritance.
pub fn load_descriptor(index: &ResourceIndex, instance: u32) -> Result<PlanetDescriptor, String> {
    let key = [TERRAIN_SCRIPTS_GROUP, TERRAIN_SCRIPTS_FALLBACK_GROUP]
        .into_iter()
        .map(|group_id| ResourceKey {
            group_id,
            instance_id: instance,
            type_id: crate::asset_catalog::PROP,
        })
        .find(|k| index.records.contains_key(k))
        .ok_or_else(|| format!("terrain script {instance:08x} not installed"))?;
    load_descriptor_key(index, key)
}

/// Load the terrain script at `key` (any group: the Space T0-T3 pools live in
/// 0x4184A1xx..0x4184A4xx, planet-re 2A), theme chain resolved as above.
pub fn load_descriptor_key(index: &ResourceIndex, key: ResourceKey) -> Result<PlanetDescriptor, String> {
    if !index.records.contains_key(&key) {
        return Err(format!("terrain script {:08x}!{:08x} not installed", key.group_id, key.instance_id));
    }
    let instance = key.instance_id;
    let props = index.properties(key)?;
    let effects: Vec<u32> = props.0.get(&P_MODEL_EFFECT).map_or_else(Vec::new, |p| {
        p.values
            .iter()
            .filter_map(|v| Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?)))
            .collect()
    });
    let transforms: Vec<EntryTransform> = props
        .0
        .get(&P_MODEL_EFFECT_TRANSFORMS)
        .map_or_else(Vec::new, |p| {
            p.values
                .iter()
                .map(|v| EntryTransform::parse(v).unwrap_or(EntryTransform::IDENTITY))
                .collect()
        });
    let seed = props
        .0
        .get(&P_MODEL_EFFECT_SEED)
        .and_then(u32_value)
        .unwrap_or(instance);
    // The terrainScript player (loader 0xF94F40, VERIFIED) drops an optional
    // per-entry array whose length differs from the modelEffect count.
    let n = effects.len();
    let full_u32 = |v: Vec<u32>| if v.len() == n { v } else { Vec::new() };
    let full_f32 = |v: Vec<f32>| if v.len() == n { v } else { Vec::new() };
    let seeds = full_u32(u32_list(&props, P_EFFECT_SEEDS));
    let intensities = full_f32(f32_list(&props, P_EFFECT_INTENSITIES));
    let times = full_f32(f32_list(&props, P_EFFECT_TIMES));
    let entry_water_levels = full_f32(f32_list(&props, P_EFFECT_WATER_LEVELS));
    let transforms = if transforms.len() == n { transforms } else { Vec::new() };
    let entries = effects
        .iter()
        .enumerate()
        .map(|(i, &effect)| TerrainEntry {
            effect,
            transform: transforms.get(i).copied().unwrap_or(EntryTransform::IDENTITY),
            seed: seeds.get(i).copied().unwrap_or(seed),
            intensity: intensities.get(i).copied().unwrap_or(1.0),
            time: times.get(i).copied().unwrap_or(0.0),
        })
        .collect();
    Ok(PlanetDescriptor {
        script: key,
        seed,
        entries,
        wta: vec3_of(&props, P_WTA_LEVELS).unwrap_or([0.5; 3]),
        planet_info: vec3_of(&props, P_PLANET_INFO).unwrap_or([32.0, 32.0, 500.0]),
        reseed_entries: !seeds.is_empty(),
        entry_water_levels,
        properties: props,
    })
}

// ---------------------------------------------------------------------------
// Effect directory: VisualEffect + spBrush.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawMode {
    Add,
    Biased,
    Subtract,
    Level,
    LevelRange,
    AbsoluteMax,
    AbsoluteMin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondKind {
    None,
    Less,
    Greater,
    Between,
}

/// A height condition (`cond` / `gradientCond`) and its falloff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cond {
    pub kind: CondKind,
    pub a: f32,
    pub b: f32,
    pub falloff: bool,
    pub falloff_lo: f32,
    pub falloff_hi: f32,
}

/// One `spBrush` component (BrushEffect.java field order, section v2).
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub texture: Option<ResourceKey>,
    pub draw_mode: DrawMode,
    pub tex_gaussian: bool,
    pub tex_gaussian_value: f32,
    pub life: f32,
    pub size: Vec<f32>,
    pub intensity: Vec<f32>,
    pub water_level_is_zero: bool,
    pub intensity_vary: f32,
    pub size_vary: f32,
    pub tex_offset: [f32; 2],
    pub draw_level: f32,
    pub tex_vary: f32,
    pub draw_range: [f32; 2],
    pub rate: f32,
    pub use_filter: bool,
    pub size_global: bool,
    pub filter: u8,
    pub filter_mode: u8,
    pub filter_value: f32,
    pub spacing: Vec<f32>,
    pub spacing_vary: f32,
    pub cond: Cond,
    pub gradient_cond: Cond,
    pub tex_cube: bool,
    pub rotate: [f32; 3],
    pub rotate_vary: [f32; 3],
    pub use_ribbon: bool,
    /// Ribbon (+0xF0..+0xFC, read in order): tiles, steps, texture strips,
    /// point skip (the retail ribbon draw 0xF4D980 uses them in this order).
    pub ribbon: [i32; 4],
}

/// VisualEffectBlock transform: flags bit 0 scale, bit 1 rotation, bit 2 offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockTransform {
    pub flags: u16,
    pub scale: f32,
    /// Row-major 3x3 (rows are local axes).
    pub rows: [Vec3; 3],
    pub offset: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectBlock {
    pub kind: u8,
    pub transform: BlockTransform,
    pub selection_group: u16,
    pub selection_chance: u16,
    pub index: i32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct EffectDef {
    pub blocks: Vec<EffectBlock>,
}

pub const KIND_EFFECT: u8 = 0;
/// Swarm `metaParticles` component (blocks index [`BrushLibrary::metas`]).
pub const KIND_META: u8 = 0x02;
pub const KIND_BRUSH: u8 = 0x20;
pub const IMPORT_MASK: i32 = 0x4000_0000;

/// The terrain brushes and visual effects of one effect directory.
#[derive(Clone, Debug, Default)]
pub struct BrushLibrary {
    pub brushes: Vec<Brush>,
    pub effects: Vec<EffectDef>,
    /// Effect name hash -> effect index.
    pub exports: HashMap<u32, i32>,
    pub imports: Vec<u32>,
    pub section_counts: HashMap<u16, u32>,
    /// The `metaParticles` components (section 2), decoded with the shared
    /// Swarm reader (`swarm_fx::format`, SporeModder-FX MetaParticleEffect
    /// layout = the retail reader 0xA9A880). Empty when the section fails to
    /// decode.
    pub metas: Vec<crate::swarm_fx::format::MetaParticleDef>,
}

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}
type R<T> = Result<T, String>;
impl Rd<'_> {
    fn take<const N: usize>(&mut self) -> R<[u8; N]> {
        let b = self
            .d
            .get(self.p..self.p + N)
            .ok_or_else(|| format!("effdir truncated at {:#x}", self.p))?;
        self.p += N;
        Ok(b.try_into().unwrap())
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> R<u16> {
        Ok(u16::from_be_bytes(self.take()?))
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
    fn f(&mut self) -> R<f32> {
        Ok(f32::from_be_bytes(self.take()?))
    }
    fn fle(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    fn flist(&mut self) -> R<Vec<f32>> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return Err("implausible curve length".into());
        }
        (0..n).map(|_| self.f()).collect()
    }
    fn cond(&mut self) -> R<(CondKind, f32, f32)> {
        let kind = match self.u8()? {
            1 => CondKind::Less,
            2 => CondKind::Greater,
            3 => CondKind::Between,
            _ => CondKind::None,
        };
        Ok((kind, self.f()?, self.f()?))
    }
}

fn read_brush(r: &mut Rd) -> R<Brush> {
    for _ in 0..3 {
        r.i32()?; // field_8
    }
    let group = r.u32()?;
    let instance = r.u32()?;
    let texture = (instance != 0 && instance != u32::MAX).then_some(ResourceKey {
        group_id: if group == u32::MAX { 0 } else { group },
        instance_id: instance,
        type_id: crate::asset_catalog::RW4,
    });
    let draw_mode = match r.u8()? {
        1 => DrawMode::Biased,
        2 => DrawMode::Subtract,
        3 => DrawMode::Level,
        4 => DrawMode::LevelRange,
        5 => DrawMode::AbsoluteMax,
        6 => DrawMode::AbsoluteMin,
        _ => DrawMode::Add,
    };
    let tex_gaussian = r.u8()? != 0;
    let tex_gaussian_value = r.f()?;
    let life = r.f()?;
    let size = r.flist()?;
    let intensity = r.flist()?;
    let water_level_is_zero = r.u8()? != 0;
    let intensity_vary = r.f()?;
    let size_vary = r.f()?;
    r.i32()?; // field_60
    let tex_offset = [r.f()?, r.f()?];
    let draw_level = r.f()?;
    let tex_vary = r.f()?;
    let draw_range = [r.f()?, r.f()?];
    let rate = r.f()?;
    let use_filter = r.u8()? != 0;
    let size_global = r.u8()? != 0;
    let filter = r.u8()?;
    let filter_mode = r.u8()?;
    let filter_value = r.f()?;
    let spacing = r.flist()?;
    let spacing_vary = r.f()?;
    let (ck, ca, cb) = r.cond()?;
    let (cf, cflo, cfhi) = (r.u8()? != 0, r.f()?, r.f()?);
    let (gk, ga, gb) = r.cond()?;
    let (gf, gflo, gfhi) = (r.u8()? != 0, r.f()?, r.f()?);
    let tex_cube = r.u8()? != 0;
    let rotate = [r.fle()?, r.fle()?, r.fle()?];
    let rotate_vary = [r.fle()?, r.fle()?, r.fle()?];
    let use_ribbon = r.u8()? != 0;
    let ribbon = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
    r.u8()?; // cap
    Ok(Brush {
        texture,
        draw_mode,
        tex_gaussian,
        tex_gaussian_value,
        life,
        size,
        intensity,
        water_level_is_zero,
        intensity_vary,
        size_vary,
        tex_offset,
        draw_level,
        tex_vary,
        draw_range,
        rate,
        use_filter,
        size_global,
        filter,
        filter_mode,
        filter_value,
        spacing,
        spacing_vary,
        cond: Cond {
            kind: ck,
            a: ca,
            b: cb,
            falloff: cf,
            falloff_lo: cflo,
            falloff_hi: cfhi,
        },
        gradient_cond: Cond {
            kind: gk,
            a: ga,
            b: gb,
            falloff: gf,
            falloff_lo: gflo,
            falloff_hi: gfhi,
        },
        tex_cube,
        rotate,
        rotate_vary,
        use_ribbon,
        ribbon,
    })
}

fn read_block(r: &mut Rd) -> R<EffectBlock> {
    let kind = r.u8()?;
    r.i32()?; // flags
    let flags = r.u16()?;
    let scale = r.f()?;
    let mut m = [0.0f32; 9];
    for v in &mut m {
        *v = r.fle()?;
    }
    let offset = Vec3::new(r.fle()?, r.fle()?, r.fle()?);
    r.u8()?;
    r.u8()?; // lod range
    let n = r.u32()? as usize;
    if n > 64 {
        return Err("implausible lod scale count".into());
    }
    for _ in 0..n * 3 {
        r.f()?;
    }
    for _ in 0..6 {
        r.f()?; // emit/size/alpha scale
    }
    r.i32()?;
    r.i32()?; // app flags
    let selection_group = r.u16()?;
    let selection_chance = r.u16()?;
    r.f()?; // time scale
    let index = r.i32()?;
    Ok(EffectBlock {
        kind,
        transform: BlockTransform {
            flags,
            scale,
            rows: [
                Vec3::new(m[0], m[1], m[2]),
                Vec3::new(m[3], m[4], m[5]),
                Vec3::new(m[6], m[7], m[8]),
            ],
            offset,
        },
        selection_group,
        selection_chance,
        index,
    })
}

fn read_effect(r: &mut Rd) -> R<EffectDef> {
    r.i32()?; // flags
    r.i32()?; // component app flags mask
    r.u32()?; // notify
    r.fle()?;
    r.fle()?; // screen size range
    r.f()?; // cursor active distance
    r.u8()?; // cursor button
    r.flist()?; // lod distances
    for _ in 0..3 {
        r.fle()?;
    }
    r.u32()?; // seed
    let n = r.u32()? as usize;
    if n > 4096 {
        return Err("implausible block count".into());
    }
    Ok(EffectDef {
        blocks: (0..n).map(|_| read_block(r)).collect::<R<_>>()?,
    })
}

impl BrushLibrary {
    pub fn parse(data: &[u8]) -> R<Self> {
        let mut out = Self::default();
        let mut r = Rd { d: data, p: 0 };
        let version = r.i16()?;
        if version != 4 {
            return Err(format!("unsupported effdir version {version}"));
        }
        let mut brush_section = None;
        for pass in 0..2 {
            loop {
                let kind = r.i16()?;
                if kind == -1 {
                    break;
                }
                let _v = r.i16()?;
                let size = r.i32()? as usize;
                let off = r.p;
                let end = off
                    .checked_add(size)
                    .filter(|&e| e <= data.len())
                    .ok_or("effdir section overruns the resource")?;
                let count = Rd { d: data, p: off }.u32()?;
                if pass == 0 {
                    out.section_counts.insert(kind as u16, count);
                    if kind == KIND_BRUSH as i16 {
                        brush_section = Some((off + 4, count as usize));
                    }
                }
                r.p = end;
            }
        }
        if let Some((start, count)) = brush_section {
            let mut br = Rd { d: data, p: start };
            out.brushes = (0..count)
                .map(|i| read_brush(&mut br).map_err(|e| format!("brush #{i}: {e}")))
                .collect::<R<_>>()?;
        }
        let _ev = r.i16()?;
        let count = r.i32()? as usize;
        if count > 1 << 20 {
            return Err("implausible effect count".into());
        }
        out.effects = (0..count)
            .map(|i| read_effect(&mut r).map_err(|e| format!("effect #{i}: {e}")))
            .collect::<R<_>>()?;
        loop {
            let idx = r.i32()?;
            if idx == -1 {
                break;
            }
            r.u32()?;
            let name = r.u32()?;
            out.exports.entry(name).or_insert(idx);
        }
        r.i32()?;
        let imports = r.i32()?.max(0) as usize;
        for _ in 0..imports.min(1 << 16) {
            r.u32()?;
            out.imports.push(r.u32()?);
        }
        if out.section_counts.contains_key(&(KIND_META as u16)) {
            out.metas = crate::swarm_fx::format::EffectDirectory::parse(data)
                .map(|d| d.metas)
                .unwrap_or_default();
        }
        Ok(out)
    }

    pub fn load(index: &ResourceIndex) -> R<Self> {
        Self::parse(&index.read(PLANET_EFFDIR)?)
    }

    /// Effect by exported name hash.
    pub fn effect(&self, name: u32) -> Option<&EffectDef> {
        let i = *self.exports.get(&name)?;
        (i >= 0 && i & IMPORT_MASK == 0)
            .then(|| self.effects.get(i as usize))
            .flatten()
    }
}

// ---------------------------------------------------------------------------
// Brush rasters.

/// A decoded single-channel brush raster (values 0..1) or a 6-face cube.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushRaster {
    pub width: usize,
    pub height: usize,
    /// One plane for 2D rasters, six (D3D cube face order +X,-X,+Y,-Y,+Z,-Z):
    /// the red channel (= luminance of the L8 rasters).
    pub faces: Vec<Vec<f32>>,
    /// Alpha planes (same layout); empty for formats without alpha (D3DFMT_L8
    /// samples alpha 1). The level programs weight by alpha (planet-blend
    /// notes 3).
    pub alpha: Vec<Vec<f32>>,
}

impl BrushRaster {
    /// Bilinear (red, alpha) of plane `face` at uv in [0, 1] (clamped).
    pub fn sample_ra(&self, face: usize, u: f32, v: f32) -> (f32, f32) {
        let x = (u * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (v * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let bilinear = |plane: &[f32]| {
            let t = |x: usize, y: usize| plane[y * self.width + x];
            let top = t(x0, y0) + (t(x1, y0) - t(x0, y0)) * fx;
            let bottom = t(x0, y1) + (t(x1, y1) - t(x0, y1)) * fx;
            top + (bottom - top) * fy
        };
        let f = face.min(self.faces.len() - 1);
        let r = bilinear(&self.faces[f]);
        let a = self.alpha.get(f).map_or(1.0, |p| bilinear(p));
        (r, a)
    }

    /// D3D cube lookup by direction: (red, alpha).
    pub fn sample_cube_ra(&self, d: Vec3) -> (f32, f32) {
        if self.faces.len() < 6 {
            return self.sample_ra(0, 0.5, 0.5);
        }
        let (face, u, v) = Self::cube_face_uv(d);
        self.sample_ra(face, u, v)
    }

    fn cube_face_uv(d: Vec3) -> (usize, f32, f32) {
        let (ax, ay, az) = (d.x.abs(), d.y.abs(), d.z.abs());
        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if d.x > 0.0 {
                (0, -d.z, -d.y, ax)
            } else {
                (1, d.z, -d.y, ax)
            }
        } else if ay >= az {
            if d.y > 0.0 {
                (2, d.x, d.z, ay)
            } else {
                (3, d.x, -d.z, ay)
            }
        } else if d.z > 0.0 {
            (4, d.x, -d.y, az)
        } else {
            (5, -d.x, -d.y, az)
        };
        (face, (sc / ma + 1.0) * 0.5, (tc / ma + 1.0) * 0.5)
    }

    /// Decode level 0 of an installed RW4 brush (D3DFMT_L8, A8R8G8B8, DXT).
    /// Brush values are the grey (red) channel: the installed ARGB brushes
    /// carry r = g = b with a near-equal alpha.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let tex = crate::native_texture::decode(bytes, None).ok()?;
        let (w, h) = (tex.width as usize, tex.height as usize);
        let mut faces = Vec::with_capacity(tex.faces.len());
        let mut alpha = Vec::new();
        for face in &tex.faces {
            let mip = face.first()?;
            let plane = if tex.format == 50 {
                let mut plane = Vec::with_capacity(w * h);
                for row in 0..h {
                    let start = row * mip.row_pitch;
                    plane.extend(mip.bytes.get(start..start + w)?.iter().map(|&l| l as f32 / 255.0));
                }
                plane
            } else {
                let px = crate::texture::decode_blocks(tex.width, tex.height, tex.format as u32, &mip.bytes)?;
                alpha.push(px.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect());
                px.rgba.chunks_exact(4).map(|p| p[0] as f32 / 255.0).collect()
            };
            faces.push(plane);
        }
        (!faces.is_empty() && w > 0 && h > 0).then_some(Self {
            width: w,
            height: h,
            faces,
            alpha,
        })
    }
}

/// Everything the generator needs from the install for one planet.
#[derive(Clone, Debug)]
pub struct PlanetSource {
    pub script: PlanetScript,
    pub descriptor: PlanetDescriptor,
    pub library: BrushLibrary,
    pub rasters: HashMap<ResourceKey, BrushRaster>,
}

impl PlanetSource {
    /// Load a planet type and every raster its effects reference.
    pub fn load(index: &ResourceIndex, script: &PlanetScript) -> R<Self> {
        let descriptor = load_descriptor(index, script.terrain_script)?;
        let library = BrushLibrary::load(index)?;
        Ok(Self::from_parts(index, script.clone(), descriptor, library))
    }

    /// A planet type from an already loaded descriptor and brush library
    /// (callers that generate many planets keep one library), with every
    /// raster its effects reference.
    pub fn from_parts(
        index: &ResourceIndex,
        script: PlanetScript,
        descriptor: PlanetDescriptor,
        library: BrushLibrary,
    ) -> Self {
        let mut rasters = HashMap::new();
        let mut stack: Vec<u32> = Vec::new();
        let mut seen_effects = std::collections::HashSet::new();
        for entry in &descriptor.entries {
            if let Some(&i) = library.exports.get(&entry.effect) {
                stack.push(i as u32);
            }
        }
        while let Some(i) = stack.pop() {
            if i as i32 & IMPORT_MASK != 0 || !seen_effects.insert(i) {
                continue;
            }
            let Some(effect) = library.effects.get(i as usize) else {
                continue;
            };
            for block in &effect.blocks {
                match block.kind {
                    KIND_EFFECT if block.index >= 0 => stack.push(block.index as u32),
                    // A metaParticles component carries an effect per particle.
                    KIND_META => {
                        if let Some(m) = library.metas.get(block.index.max(0) as usize) {
                            if m.component.0 >= 0 && m.component.1 == KIND_EFFECT as i32 {
                                stack.push(m.component.0 as u32);
                            }
                        }
                    }
                    KIND_BRUSH => {
                        if let Some(key) = library
                            .brushes
                            .get(block.index.max(0) as usize)
                            .and_then(|b| b.texture)
                        {
                            if let std::collections::hash_map::Entry::Vacant(e) = rasters.entry(key) {
                                if let Some(r) = index.read(key).ok().and_then(|b| BrushRaster::decode(&b)) {
                                    e.insert(r);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Self {
            script,
            descriptor,
            library,
            rasters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_index() -> Option<ResourceIndex> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then_some(index)
    }

    #[test]
    fn homeworld_choice_is_deterministic_and_in_range() {
        let scripts: Vec<PlanetScript> = (0..15)
            .map(|i| PlanetScript {
                key: ResourceKey {
                    group_id: PLANET_SCRIPTS_GROUP,
                    instance_id: i,
                    type_id: crate::asset_catalog::PROP,
                },
                description: format!("p{i}"),
                terrain_script: i,
                first_group: true,
                exclude_from_gge: false,
            })
            .collect();
        if std::env::var("OPENSPORE_PLANET").is_ok() {
            return;
        }
        let a = choose_homeworld(&scripts, 12345).unwrap();
        let b = choose_homeworld(&scripts, 12345).unwrap();
        assert_eq!(a, b);
        let picks: std::collections::HashSet<u32> = (0..200u64)
            .map(|s| choose_homeworld(&scripts, s * 7919).unwrap().terrain_script)
            .collect();
        assert!(picks.len() > 8, "seeds spread over the families");
        assert!(choose_homeworld(&[], 1).is_none());
    }

    #[test]
    #[ignore = "reads the installed game"]
    fn installed_first_group_has_fifteen_base_families() {
        let Some(index) = install_index() else { return };
        let first = first_group(&index);
        let names: Vec<_> = first.iter().map(|s| s.description.to_lowercase()).collect();
        assert_eq!(first.len(), 15, "{names:?}");
        for want in ["bumpy", "mountains", "volcanoes", "rivers", "mesas"] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        let all = planet_scripts(&index);
        assert!(all.len() >= 150, "{} planet scripts", all.len());
    }

    #[test]
    #[ignore = "reads the installed game"]
    fn installed_brush_library_and_scripts_decode() {
        let Some(index) = install_index() else { return };
        let library = BrushLibrary::load(&index).unwrap();
        assert_eq!(library.brushes.len(), 1319);
        let continent = library
            // `BR_continent_small~` (the registry gives the "~" names explicit ids).
            .effect(0x03CD_9D76)
            .unwrap();
        let brushes: Vec<&Brush> = continent
            .blocks
            .iter()
            .filter(|b| b.kind == KIND_BRUSH)
            .map(|b| &library.brushes[b.index as usize])
            .collect();
        assert_eq!(brushes.len(), 2);
        assert_eq!(brushes[0].draw_mode, DrawMode::Add);
        assert_eq!(brushes[0].size, vec![50.0]);
        assert_eq!(brushes[0].cond.kind, CondKind::Between);
        assert!((brushes[0].cond.b - 0.025).abs() < 1e-6);
        assert!((continent.blocks[0].transform.scale - 0.9).abs() < 1e-6);
        let flatten = library
            .effect(0x0534_71DE) // BR_flatten~
            .unwrap();
        let b = &library.brushes[flatten.blocks[0].index as usize];
        assert_eq!(b.draw_mode, DrawMode::Level);
        assert_eq!(b.draw_level, -2.0);
        // Every FirstGroup script decodes and every entry's effect resolves.
        for script in first_group(&index) {
            let source = PlanetSource::load(&index, &script).unwrap();
            let d = &source.descriptor;
            assert!(d.entries.len() > 50, "{}: {} entries", script.description, d.entries.len());
            assert!((d.radius() - 500.0).abs() < 1e-3);
            let mut on_sphere = 0;
            for e in &d.entries {
                assert!(source.library.effect(e.effect).is_some(), "{} effect {:08x}", script.description, e.effect);
                // Stamps sit on the R = 500 sphere with row 2 = outward normal
                // (global passes may carry an arbitrary transform).
                if (e.transform.offset.length() - 500.0).abs() < 80.0
                    && e.transform.rows[2].dot(e.transform.offset.normalize()) > 0.9
                {
                    on_sphere += 1;
                }
            }
            assert!(on_sphere * 10 >= d.entries.len() * 9, "{}: {on_sphere}/{}", script.description, d.entries.len());
            assert!(!source.rasters.is_empty());
        }
    }
}
