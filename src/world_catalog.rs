//! Original world descriptors. Source properties and full resource identities
//! are retained; this decoder does not reproduce the native terrain generator.
use crate::{
    asset_catalog::{ResourceIndex, PROP},
    dbpf::ResourceKey,
    prop::{Property, PropertyList},
};
use std::collections::BTreeMap;

pub const PLANET_INFO: u32 = 0x03A23F99;
pub const BEACH: u32 = 0x03B4F7CC;
// Observed in Steam GA cTerrainStateMgr::ParseProp, runtime 0xBDC100.
// Field names follow the verified manager/texture struct offsets in the SDK.
pub const CLIFF_TEXTURE: u32 = 0x03B4F7C6;
/// terrainThemeAboveDetail1 (the intertiling near-detail raster).
pub const ABOVE_DETAIL_NEAR_TEXTURE: u32 = 0x03B4F7C8;
pub const ABOVE_DETAIL_TEXTURE: u32 = 0x03B4F7C9;
/// terrainThemeAboveDetailNoise (biome_noise_modifier 8-bit image).
pub const ABOVE_DETAIL_NOISE: u32 = 0x03B4F7CA;
pub const BELOW_TEXTURE: u32 = 0x03B4F7CB;
pub const LIGHTING_STATE: u32 = 0x03B5EC4A;
pub const STAMP_TEXTURE: u32 = 0x03B28C1B;
pub const SH_COEFFICIENTS: u32 = 0x0100EAC5;
pub const ELEVATION_STOPS: u32 = 0xB6929C92;
pub const ELEVATION_COLORS: u32 = 0xB6929C93;
pub const WATER_COLOR: u32 = 0x968C496A;
pub const ATMOSPHERE_COLOR: u32 = 0x968C496B;
pub const CLIFF_COLOR: u32 = 0xB6929C94;
pub const BEACH_COLOR: u32 = 0xB6929C95;

/// Authored planet-theme colors, including the original ordered gradient.
/// These properties contain Vector3 values, despite their registered names.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanetPalette {
    pub key: ResourceKey,
    pub elevations: Vec<f32>,
    pub colors: Vec<[f32; 3]>,
    pub water: [f32; 3],
    pub atmosphere: [f32; 3],
    pub cliff: [f32; 3],
    pub beach: [f32; 3],
}
impl PlanetPalette {
    pub fn from_properties(key: ResourceKey, list: &PropertyList) -> Result<Self, String> {
        let array = |id, kind| {
            let p = list
                .0
                .get(&id)
                .ok_or_else(|| format!("missing planet palette {id:08X}"))?;
            if !p.array || p.kind != kind || p.values.is_empty() || p.values.len() > 256 {
                return Err(format!("invalid planet palette array {id:08X}"));
            }
            Ok(p)
        };
        let elevations = array(ELEVATION_STOPS, 13)?
            .values
            .iter()
            .map(|b| {
                let p = Property {
                    kind: 13,
                    array: false,
                    values: vec![b.clone()],
                };
                p.number()
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .ok_or_else(|| "invalid elevation stop".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let colors = array(ELEVATION_COLORS, 0x31)?
            .values
            .iter()
            .map(|b| {
                palette_rgb(&Property {
                    kind: 0x31,
                    array: false,
                    values: vec![b.clone()],
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if elevations.len() != colors.len() || elevations.windows(2).any(|p| p[0] >= p[1]) {
            return Err("planet palette needs matched, strictly increasing color stops".into());
        }
        let color = |id| {
            palette_rgb(
                list.0
                    .get(&id)
                    .ok_or_else(|| format!("missing palette color {id:08X}"))?,
            )
        };
        Ok(Self {
            key,
            elevations,
            colors,
            water: color(WATER_COLOR)?,
            atmosphere: color(ATMOSPHERE_COLOR)?,
            cliff: color(CLIFF_COLOR)?,
            beach: color(BEACH_COLOR)?,
        })
    }
    /// Preview interpolation. Native color factoring and ramp generation are
    /// separate operations and have not been compared to this approximation.
    pub fn sample(&self, elevation: f32) -> [f32; 3] {
        let elevation = if elevation.is_finite() {
            elevation
        } else {
            self.elevations[0]
        };
        let end = self.elevations.partition_point(|v| *v < elevation);
        if end == 0 {
            return self.colors[0];
        }
        if end == self.elevations.len() {
            return *self.colors.last().unwrap();
        }
        let t = (elevation - self.elevations[end - 1])
            / (self.elevations[end] - self.elevations[end - 1]);
        std::array::from_fn(|i| self.colors[end - 1][i] * (1. - t) + self.colors[end][i] * t)
    }
}
fn palette_rgb(p: &Property) -> Result<[f32; 3], String> {
    if p.array || p.values.len() != 1 {
        return Err("palette color must be scalar".into());
    }
    let color = p.vector3().ok_or("palette color needs a Vector3")?;
    if color
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err("invalid palette color range".into());
    }
    Ok(color)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f32),
    Color([f32; 3]),
    Vector3([f32; 3]),
    Vector4([f32; 4]),
    Key(ResourceKey),
    Raw { kind: u16, bytes: Vec<u8> },
}
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub array: bool,
    pub values: Vec<Value>,
}
#[derive(Debug, Clone)]
pub struct WorldDescriptor {
    pub key: ResourceKey,
    pub full: PropertyList,
    pub fields: BTreeMap<u32, Field>,
}
/// Optional overrides accepted by native ParseProp. Absence leaves the native
/// defaults untouched; this reader does not invent or render those defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainOverrides {
    pub cliff: Option<ResourceKey>,
    pub above_detail: Option<ResourceKey>,
    pub above_detail_near: Option<ResourceKey>,
    pub above_detail_noise: Option<ResourceKey>,
    pub below: Option<ResourceKey>,
    pub beach: Option<ResourceKey>,
    pub lighting_state: Option<ResourceKey>,
}

/// Global terrain material/config tuning read by native cTerrainStateMgr
/// ("planet materials" and "planet config" in group 0243AD2B). Missing or
/// malformed fields keep the values observed in the installed game.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanetMaterials {
    /// Detail repeats per planet cube face (kAboveDetailLow/HighTiling).
    pub above_detail_low_tiling: f32,
    pub above_detail_high_tiling: f32,
    pub cliff_tiling: f32,
    pub below_tiling: f32,
    pub terrain_brightness: f32,
    pub ambient_level: f32,
    pub day_light: [f32; 3],
    pub day_shadow: [f32; 3],
    pub fog_min_distance: f32,
    pub fog_max_density: f32,
    /// Cliff blend range in degrees (kMin/MaxCliffGradient).
    pub min_cliff_gradient: f32,
    pub max_cliff_gradient: f32,
}
pub const PLANET_MATERIALS: ResourceKey = ResourceKey {
    group_id: 0x0243AD2B,
    instance_id: 0xD985994C,
    type_id: PROP,
};
pub const PLANET_CONFIG: ResourceKey = ResourceKey {
    group_id: 0x0243AD2B,
    instance_id: 0x5F848353,
    type_id: PROP,
};
impl Default for PlanetMaterials {
    fn default() -> Self {
        Self {
            above_detail_low_tiling: 6.,
            above_detail_high_tiling: 45.,
            cliff_tiling: 40.,
            below_tiling: 3.,
            terrain_brightness: 1.1,
            ambient_level: 0.45,
            day_light: [1.75; 3],
            day_shadow: [0.85; 3],
            fog_min_distance: 70.,
            fog_max_density: 0.009,
            min_cliff_gradient: 32.,
            max_cliff_gradient: 45.,
        }
    }
}
impl PlanetMaterials {
    pub fn from_properties(
        materials: Option<&PropertyList>,
        config: Option<&PropertyList>,
    ) -> Self {
        let mut out = Self::default();
        let number = |list: Option<&PropertyList>, id: u32, slot: &mut f32| {
            if let Some(v) = list
                .and_then(|l| l.0.get(&id))
                .filter(|p| !p.array)
                .and_then(Property::number)
                .filter(|v| v.is_finite() && *v >= 0.)
            {
                *slot = v;
            }
        };
        let vector = |list: Option<&PropertyList>, id: u32, slot: &mut [f32; 3]| {
            if let Some(v) = list
                .and_then(|l| l.0.get(&id))
                .filter(|p| !p.array)
                .and_then(Property::vector3)
                .filter(|v| v.iter().all(|c| c.is_finite() && *c >= 0.))
            {
                *slot = v;
            }
        };
        number(materials, 0x017EDA6B, &mut out.above_detail_low_tiling);
        number(materials, 0x017EDA5F, &mut out.above_detail_high_tiling);
        number(materials, 0x010EBDBE, &mut out.cliff_tiling);
        number(materials, 0x010EBDAD, &mut out.below_tiling);
        number(materials, 0x02570386, &mut out.terrain_brightness);
        number(materials, 0xDE37AF7E, &mut out.ambient_level);
        vector(materials, 0x87C2473F, &mut out.day_light);
        vector(materials, 0x1331B443, &mut out.day_shadow);
        number(materials, 0x58D751BC, &mut out.fog_min_distance);
        number(materials, 0x2C66E2C6, &mut out.fog_max_density);
        number(config, 0x0228D235, &mut out.min_cliff_gradient);
        number(config, 0x0228D23F, &mut out.max_cliff_gradient);
        if out.max_cliff_gradient <= out.min_cliff_gradient || out.max_cliff_gradient > 90. {
            out.min_cliff_gradient = Self::default().min_cliff_gradient;
            out.max_cliff_gradient = Self::default().max_cliff_gradient;
        }
        out
    }
    pub fn load(index: &ResourceIndex) -> Self {
        let materials = index
            .read(PLANET_MATERIALS)
            .and_then(|b| crate::prop::parse(&b))
            .ok();
        let config = index
            .read(PLANET_CONFIG)
            .and_then(|b| crate::prop::parse(&b))
            .ok();
        Self::from_properties(materials.as_ref(), config.as_ref())
    }
}
#[derive(Debug, Clone, Default)]
pub struct WorldCatalog {
    pub descriptors: Vec<WorldDescriptor>,
    pub errors: Vec<String>,
}

fn selected(id: u32) -> bool {
    matches!(id,0x0100EAB6..=0x0100EACF | 0x02478ED7 | 0x02478EDA..=0x02478EE4)
        || matches!(
            id,
            PLANET_INFO
                | BEACH
                | STAMP_TEXTURE
                | CLIFF_TEXTURE
                | ABOVE_DETAIL_NEAR_TEXTURE
                | ABOVE_DETAIL_TEXTURE
                | ABOVE_DETAIL_NOISE
                | BELOW_TEXTURE
                | LIGHTING_STATE
                | ELEVATION_STOPS
                | ELEVATION_COLORS
                | WATER_COLOR
                | ATMOSPHERE_COLOR
                | CLIFF_COLOR
                | BEACH_COLOR
        )
}
fn expected_kind(id: u32) -> Option<u16> {
    match id {
        ELEVATION_STOPS => Some(13),
        ELEVATION_COLORS | WATER_COLOR | ATMOSPHERE_COLOR | CLIFF_COLOR | BEACH_COLOR => Some(0x31),
        0x0100EAB6 | 0x0100EAB9 | 0x0100EABC | 0x0100EABF | 0x0100EAC2 | SH_COEFFICIENTS => {
            Some(0x32)
        }
        0x0100EAB8 | 0x0100EABB | 0x0100EABE | 0x0100EAC1 | PLANET_INFO => Some(0x31),
        0x02478EDC..=0x02478EDE => Some(0x33),
        STAMP_TEXTURE
        | CLIFF_TEXTURE
        | ABOVE_DETAIL_NEAR_TEXTURE
        | ABOVE_DETAIL_TEXTURE
        | ABOVE_DETAIL_NOISE
        | BELOW_TEXTURE
        | BEACH
        | LIGHTING_STATE => Some(0x20),
        _ => None,
    }
}
fn value(scalar: &Property) -> Result<Value, String> {
    let bytes = scalar.values.first().ok_or("missing world field value")?;
    let finite = |values: &[f32]| {
        if values.iter().all(|v| v.is_finite()) {
            Ok(())
        } else {
            Err("nonfinite world value".to_string())
        }
    };
    Ok(match scalar.kind {
        1 => match bytes.first() {
            Some(0) => Value::Bool(false),
            Some(1) => Value::Bool(true),
            _ => return Err("invalid world boolean".into()),
        },
        5..=10 | 13 => {
            let v = scalar.number().ok_or("truncated world number")?;
            finite(&[v])?;
            Value::Number(v)
        }
        0x20 => Value::Key(scalar.key().ok_or("truncated world resource key")?),
        0x31 => {
            let v = scalar.vector3().ok_or("truncated world vector3")?;
            finite(&v)?;
            Value::Vector3(v)
        }
        0x32 => {
            let v = scalar.color_rgb().ok_or("truncated world color")?;
            finite(&v)?;
            Value::Color(v)
        }
        0x33 | 0x34 => {
            let v = scalar.vector4().ok_or("truncated world vector4")?;
            finite(&v)?;
            Value::Vector4(v)
        }
        kind => Value::Raw {
            kind,
            bytes: bytes.clone(),
        },
    })
}
impl WorldDescriptor {
    pub fn terrain_overrides(&self) -> Result<TerrainOverrides, String> {
        let key = |id: u32| -> Result<Option<ResourceKey>, String> {
            let Some(field) = self.fields.get(&id) else {
                return Ok(None);
            };
            match (&field.values[..], field.array) {
                ([Value::Key(key)], false) => Ok(Some(*key)),
                _ => Err(format!(
                    "terrain override {id:08X} needs one scalar resource key"
                )),
            }
        };
        Ok(TerrainOverrides {
            cliff: key(CLIFF_TEXTURE)?,
            above_detail: key(ABOVE_DETAIL_TEXTURE)?,
            above_detail_near: key(ABOVE_DETAIL_NEAR_TEXTURE)?,
            above_detail_noise: key(ABOVE_DETAIL_NOISE)?,
            below: key(BELOW_TEXTURE)?,
            beach: key(BEACH)?,
            lighting_state: key(LIGHTING_STATE)?,
        })
    }
    pub fn parse(key: ResourceKey, full: PropertyList) -> Result<Self, String> {
        let mut fields = BTreeMap::new();
        for (&id, p) in full.0.iter().filter(|(id, _)| selected(**id)) {
            if expected_kind(id).is_some_and(|kind| kind != p.kind) {
                return Err(format!(
                    "world field {id:08X} has wrong kind {:04X}",
                    p.kind
                ));
            }
            if !p.array && p.values.len() != 1 {
                return Err(format!("world field {id:08X} needs one scalar"));
            }
            if p.values.len() > 4096 {
                return Err("world array exceeds limit".into());
            }
            let values = p
                .values
                .iter()
                .map(|bytes| {
                    value(&Property {
                        kind: p.kind,
                        array: false,
                        values: vec![bytes.clone()],
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("world field {id:08X}: {e}"))?;
            fields.insert(
                id,
                Field {
                    array: p.array,
                    values,
                },
            );
        }
        Ok(Self { key, full, fields })
    }
}
impl WorldCatalog {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let mut out = Self::default();
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == PROP)
            .collect();
        keys.sort_by_key(|k| (k.group_id, k.instance_id));
        for key in keys {
            let result = index
                .read(key)
                .and_then(|b| crate::prop::parse(&b))
                .and_then(|p| WorldDescriptor::parse(key, p));
            match result {
                Ok(d) if !d.fields.is_empty() => out.descriptors.push(d),
                Ok(_) => {}
                Err(e) => out
                    .errors
                    .push(format!("{:08X}!{:08X}: {e}", key.group_id, key.instance_id)),
            }
        }
        out
    }
}
pub fn audit(index: &ResourceIndex) -> serde_json::Value {
    let catalog = WorldCatalog::from_index(index);
    let mut field_counts = BTreeMap::<String, usize>::new();
    let mut rows = vec![];
    let mut terrain_override_records = 0;
    for d in &catalog.descriptors {
        let fields: serde_json::Map<_, _> = d
            .fields
            .iter()
            .map(|(&id, f)| {
                *field_counts.entry(format!("{id:08X}")).or_default() += 1;
                let values: Vec<_> = f
                    .values
                    .iter()
                    .map(|v| match v {
                        Value::Bool(v) => serde_json::json!(v),
                        Value::Number(v) => serde_json::json!(v),
                        Value::Color(v) | Value::Vector3(v) => serde_json::json!(v),
                        Value::Vector4(v) => serde_json::json!(v),
                        Value::Key(k) => crate::resource_audit::key_json(*k),
                        Value::Raw { kind, bytes } => {
                            serde_json::json!({"kind":kind,"bytes":bytes.len()})
                        }
                    })
                    .collect();
                (
                    format!("{id:08X}"),
                    serde_json::json!({"array":f.array,"values":values}),
                )
            })
            .collect();
        let overrides = d.terrain_overrides();
        let terrain = match overrides {
            Ok(refs) => {
                let optional = |key: Option<ResourceKey>| key.map(crate::resource_audit::key_json);
                if refs != TerrainOverrides::default() {
                    terrain_override_records += 1;
                }
                serde_json::json!({"cliff":optional(refs.cliff),"above_detail":optional(refs.above_detail),"above_detail_near":optional(refs.above_detail_near),"above_detail_noise":optional(refs.above_detail_noise),"below":optional(refs.below),"beach":optional(refs.beach),"lighting_state":optional(refs.lighting_state),"native_lighting_instance":refs.lighting_state.map(|k|k.instance_id)})
            }
            Err(error) => serde_json::json!({"error":error}),
        };
        rows.push(serde_json::json!({"key":crate::resource_audit::key_json(d.key),"source_property_count":d.full.0.len(),"fields":fields,"terrain_overrides":terrain}));
    }
    serde_json::json!({"descriptors":rows,"field_counts":field_counts,"terrain_override_records":terrain_override_records,"errors":catalog.errors,"index_errors":index.errors})
}
pub fn run_audit() {
    match crate::spore_install::discover() {
        Ok(i) => println!(
            "{}",
            serde_json::to_string_pretty(&audit(&ResourceIndex::load(&i.packages))).unwrap()
        ),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> ResourceKey {
        ResourceKey {
            type_id: PROP,
            group_id: 0x40200100,
            instance_id: 7,
        }
    }
    fn key_property(reference: ResourceKey) -> Property {
        Property {
            kind: 0x20,
            array: false,
            values: vec![[
                reference.instance_id.to_le_bytes(),
                reference.type_id.to_le_bytes(),
                reference.group_id.to_le_bytes(),
            ]
            .concat()],
        }
    }
    #[test]
    fn native_terrain_overrides_preserve_groups_types_and_missing_defaults() {
        let mut list = PropertyList::default();
        let mut expected = vec![];
        for (i, id) in [
            CLIFF_TEXTURE,
            ABOVE_DETAIL_TEXTURE,
            BELOW_TEXTURE,
            BEACH,
            LIGHTING_STATE,
        ]
        .iter()
        .enumerate()
        {
            let reference = ResourceKey {
                instance_id: 9,
                group_id: 0x100 + i as u32,
                type_id: if i == 4 {
                    PROP
                } else {
                    crate::asset_catalog::RW4
                },
            };
            list.0.insert(*id, key_property(reference));
            expected.push(reference);
        }
        let d = WorldDescriptor::parse(key(), list).unwrap();
        let overrides = d.terrain_overrides().unwrap();
        assert_eq!(
            [
                overrides.cliff,
                overrides.above_detail,
                overrides.below,
                overrides.beach,
                overrides.lighting_state
            ],
            expected.iter().copied().map(Some).collect::<Vec<_>>()[..]
        );
        let empty = WorldDescriptor::parse(key(), PropertyList::default()).unwrap();
        assert_eq!(
            empty.terrain_overrides().unwrap(),
            TerrainOverrides::default()
        );
    }
    #[test]
    fn terrain_reference_arrays_and_wrong_types_are_not_guessed() {
        let mut list = PropertyList::default();
        list.0
            .insert(CLIFF_TEXTURE, floats(0x32, false, &[[0.1, 0.2, 0.3]]));
        assert!(WorldDescriptor::parse(key(), list.clone()).is_err());
        let mut array = key_property(key());
        array.array = true;
        list.0.insert(CLIFF_TEXTURE, array);
        assert!(WorldDescriptor::parse(key(), list)
            .unwrap()
            .terrain_overrides()
            .is_err());
    }
    fn floats(kind: u16, array: bool, values: &[[f32; 3]]) -> Property {
        Property {
            kind,
            array,
            values: values
                .iter()
                .map(|v| v.iter().flat_map(|f| f.to_le_bytes()).collect())
                .collect(),
        }
    }
    #[test]
    fn authored_coefficients_vectors_and_exact_references_survive() {
        let reference = ResourceKey {
            type_id: 0,
            group_id: 123,
            instance_id: 456,
        };
        let mut list = PropertyList::default();
        list.0
            .insert(SH_COEFFICIENTS, floats(0x32, true, &[[0.1, 0.2, 0.3]; 25]));
        list.0
            .insert(PLANET_INFO, floats(0x31, false, &[[13., 27., 500.]]));
        list.0.insert(
            STAMP_TEXTURE,
            Property {
                kind: 0x20,
                array: false,
                values: vec![[
                    reference.instance_id.to_le_bytes(),
                    reference.type_id.to_le_bytes(),
                    reference.group_id.to_le_bytes(),
                ]
                .concat()],
            },
        );
        list.0.insert(
            0xFFFFFFFF,
            Property {
                kind: 0xFFFF,
                array: false,
                values: vec![vec![9, 8, 7]],
            },
        );
        let d = WorldDescriptor::parse(key(), list).unwrap();
        assert_eq!(d.key, key());
        assert_eq!(d.fields[&SH_COEFFICIENTS].values.len(), 25);
        assert_eq!(
            d.fields[&PLANET_INFO].values,
            vec![Value::Vector3([13., 27., 500.])]
        );
        assert_eq!(d.fields[&STAMP_TEXTURE].values, vec![Value::Key(reference)]);
        assert_eq!(d.full.0[&0xFFFFFFFF].values[0], vec![9, 8, 7]);
    }
    #[test]
    fn present_invalid_types_truncations_and_nonfinite_array_items_fail() {
        let mut list = PropertyList::default();
        list.0
            .insert(PLANET_INFO, floats(0x32, false, &[[1., 2., 3.]]));
        assert!(WorldDescriptor::parse(key(), list.clone()).is_err());
        list.0.insert(
            PLANET_INFO,
            Property {
                kind: 0x31,
                array: false,
                values: vec![vec![0; 11]],
            },
        );
        assert!(WorldDescriptor::parse(key(), list.clone()).is_err());
        list.0.remove(&PLANET_INFO);
        list.0.insert(
            SH_COEFFICIENTS,
            floats(0x32, true, &[[0., 0., 0.], [0., f32::NAN, 0.]]),
        );
        assert!(WorldDescriptor::parse(key(), list).is_err());
    }
}
