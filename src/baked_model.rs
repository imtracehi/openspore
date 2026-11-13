//! Complete installed GMDL geometry and baked skin atlases. Layout references:
//! SporeModder-FX GameModelResource and SporeModder-Blender-Addons gmdl_importer.
//! Referenced files are dependencies; geometry is inline, not a RW4 mesh table.
use crate::{
    asset_catalog::{ResourceIndex, PROP},
    dbpf::ResourceKey,
    OriginalModel, OriginalSurface, Originals,
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::BTreeMap;
pub const CREATURE_GROUP: u32 = 0x40627100;
pub const FLORA_GROUP: u32 = 0x40667100;
/// floraModels_LOD1~ / LOD2~ / LOD3~ (the planet's flora meshes, `planet_flora`).
pub const FLORA_LOD_GROUPS: [u32; 3] = [0x40666201, 0x40666202, 0x40666203];
pub const BUILDING_GROUP: u32 = 0x40637100;
/// Installed authored low-capability creature used as the starter visual.
/// Its full GMDL is rendered for an untouched baseline; the editor's saved
/// anatomy is still the authority once the player changes the build.
pub const PLAYER_BASELINE_INSTANCE: u32 = 0x067B0FBC;

#[derive(Clone, Debug)]
pub struct TextureBinding {
    pub sampler: u32,
    pub extra: [u8; 12],
    pub key: ResourceKey,
}
#[derive(Clone, Debug, Default)]
pub struct Material {
    pub textures: Vec<TextureBinding>,
    pub parameters: Option<[u32; 5]>,
    /// Every fixed-size shader datum is retained byte-for-byte by native ID.
    /// The preview currently interprets only `0x210` as five words.
    pub shader_data: BTreeMap<u32, Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Piece {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub bones: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub material_id: u32,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub version: u32,
    pub references: Vec<ResourceKey>,
    pub pieces: Vec<Piece>,
    pub materials: Vec<Material>,
    pub bone_ranges: Vec<[u32; 2]>,
    pub root_bones: u32,
    /// Original GMDL rig bindings. These are retained as data only; their
    /// transform and baked-deform record layouts are not interpreted here.
    pub anim_data: Vec<GameModelAnimData>,
    /// Native trailing ResourceKey retained from the end of the GMDL.
    pub unknown_key: ResourceKey,
    /// Original native coordinates, packed tangents/weights, declaration bytes,
    /// and index widths. The Bevy preview conversion must not overwrite these.
    pub native_geometry: Vec<NativeGeometry>,
}

/// Native GameModelAnimData payload. Transform and baked deform records remain
/// byte exact until their semantics are independently verified.
#[derive(Clone, Debug)]
pub struct GameModelAnimData {
    pub transform: [u8; 56],
    pub field_38: [u8; 56],
    /// The native animation builder uses the signed low byte as a reserved
    /// bone count only when the binding has no external rig. For real rigs the
    /// count comes from SkinsInK; this word is usually all ones and is ignored.
    pub field_70: u32,
    /// SporeModder-FX ResourceKey LE order: instance, type, group.
    pub key: ResourceKey,
    pub baked_deforms: Vec<Vec<[u8; 160]>>,
}

impl Model {
    pub fn animation_bindings_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "records": self.anim_data.iter().map(|data| serde_json::json!({
                "key": crate::resource_audit::key_json(data.key),
                "field_70": data.field_70,
                "transform_sha256": crate::parity::sha256(&data.transform),
                "secondary_transform_sha256": crate::parity::sha256(&data.field_38),
                "deform_groups": data.baked_deforms.iter().map(|group| {
                    let bytes = group.iter().flat_map(|record| record.iter().copied()).collect::<Vec<_>>();
                    serde_json::json!({"records":group.len(), "bytes":bytes.len(), "sha256":crate::parity::sha256(&bytes)})
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "trailing_key": crate::resource_audit::key_json(self.unknown_key),
            "limitation": "Raw transforms and baked deform records retained byte-for-byte; assembled skeleton semantics and motion binding are unresolved."
        })
    }
}

const MAX_ANIM_DEFORM_BYTES: usize = 64 * 1024 * 1024;
const MAX_ANIM_DEFORM_GROUPS: usize = 262_144;
#[derive(Clone, Debug, serde::Serialize)]
pub struct NativeGeometry {
    pub vertex_bytes: Vec<u8>,
    pub stride: u32,
    pub vertex_count: u32,
    pub index_bytes: Vec<u8>,
    pub index_bits: u32,
    pub primitive_type: u32,
    /// Full 12-byte elements, including the native semantic code after D3D9's
    /// eight-byte declaration element. No guessed tangent/sign conversion.
    pub declaration: Vec<[u8; 12]>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("GMDL range overflow")?;
        let b = self.bytes.get(self.at..end).ok_or("truncated GMDL")?;
        self.at = end;
        Ok(b)
    }
    fn u(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn be(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn count(&mut self, limit: u32) -> Result<usize, String> {
        let n = self.u()?;
        if n > limit {
            Err("GMDL count exceeds limit".into())
        } else {
            Ok(n as usize)
        }
    }
}

fn read_resource_key_le(r: &mut Reader<'_>) -> Result<ResourceKey, String> {
    // SporeModder-FX ResourceKey.readLE(): instance, type, group.
    let instance_id = r.u()?;
    let type_id = r.u()?;
    let group_id = r.u()?;
    Ok(ResourceKey {
        instance_id,
        type_id,
        group_id,
    })
}

fn read_anim_data(
    r: &mut Reader<'_>,
    deform_bytes: &mut usize,
    deform_groups: &mut usize,
) -> Result<GameModelAnimData, String> {
    let transform = r.take(56)?.try_into().unwrap();
    let field_38 = r.take(56)?.try_into().unwrap();
    let field_70 = r.u()?;
    let key = read_resource_key_le(r)?;
    let group_count = r.count(64)?;
    *deform_groups = deform_groups
        .checked_add(group_count)
        .ok_or("GMDL deform group count overflow")?;
    if *deform_groups > MAX_ANIM_DEFORM_GROUPS {
        return Err("GMDL deform group allocation exceeds limit".into());
    }
    let mut baked_deforms = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let bone_count = r.count(4096)?;
        let byte_count = bone_count
            .checked_mul(160)
            .ok_or("GMDL deform byte count overflow")?;
        *deform_bytes = deform_bytes
            .checked_add(byte_count)
            .ok_or("GMDL deform allocation overflow")?;
        if *deform_bytes > MAX_ANIM_DEFORM_BYTES {
            return Err("GMDL deform allocation exceeds limit".into());
        }
        let bytes = r.take(byte_count)?;
        let mut records = Vec::with_capacity(bone_count);
        for record in bytes.chunks_exact(160) {
            records.push(record.try_into().unwrap());
        }
        baked_deforms.push(records);
    }
    Ok(GameModelAnimData {
        transform,
        field_38,
        field_70,
        key,
        baked_deforms,
    })
}

/// Shader datum widths from SporeModder-FX `ShaderData.getDataSize()`.
/// These ranges cover its complete registered 0x000..=0x33F table; IDs
/// outside it stay hard errors. `0x20D` is parsed separately as a variable-
/// length texture set, despite its shader registry's 0x64 data size.
fn shader_data_size(id: u32) -> Option<usize> {
    let size = match id {
        0x000..=0x200 => 0,
        0x201 => 8,
        0x202 => 20,
        0x203 => 8,
        0x204 => 16,
        0x205 => 20,
        0x206 => 256,
        0x207 => 0,
        0x208 => 8,
        0x209 => 4,
        0x20a..=0x20b => 64,
        0x20c => 16,
        0x20d => 100,
        0x20e..=0x20f => 8,
        0x210..=0x211 => 20,
        0x212 => 32,
        0x213 => 48,
        0x214..=0x215 => 0,
        0x216..=0x217 => 1,
        0x218..=0x21b => 0,
        0x21c => 32,
        0x21d => 0,
        0x21e => 16,
        0x21f => 0,
        0x220 => 16,
        0x221..=0x222 => 0,
        0x223 => 164,
        0x224 => 152,
        0x225 => 0,
        0x226 => 16,
        0x227..=0x22c => 0,
        0x22d..=0x22e => 16,
        0x22f => 0x3fcc,
        0x230 => 4,
        0x231 => 12,
        0x232 => 1,
        0x233 => 4,
        0x234 => 0,
        0x235..=0x236 => 4,
        0x237..=0x238 => 1,
        0x239..=0x23a => 0,
        0x23b => 96,
        0x23c => 48,
        0x23d..=0x240 => 0,
        0x241..=0x242 => 64,
        0x243 => 144,
        0x244 => 1,
        0x245 => 12,
        0x246 => 20,
        0x247..=0x248 => 16,
        0x249 => 0,
        0x24a => 48,
        0x24b..=0x254 => 0,
        0x255 => 32,
        0x256 => 16,
        0x257..=0x33f => 0,
        _ => return None,
    };
    Some(size)
}

fn read_material_shader_data(
    id: u32,
    r: &mut Reader<'_>,
    material: &mut Material,
) -> Result<(), String> {
    if id == 0x20d {
        for _ in 0..r.count(32)? {
            let sampler = r.u()?;
            let extra = r.take(12)?.try_into().unwrap();
            let instance_id = r.u()?;
            let group_id = r.u()?;
            material.textures.push(TextureBinding {
                sampler,
                extra,
                key: ResourceKey {
                    instance_id,
                    group_id,
                    type_id: crate::decode::TYPE_RASTER,
                },
            });
        }
        return Ok(());
    }

    let size =
        shader_data_size(id).ok_or_else(|| format!("unsupported GMDL shader data {id:04X}"))?;
    let bytes = r.take(size)?.to_vec();
    if id == 0x210 {
        let mut words = [0; 5];
        for (word, raw) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes(raw.try_into().unwrap());
        }
        material.parameters = Some(words);
    }
    material.shader_data.insert(id, bytes);
    Ok(())
}
fn floats<const N: usize>(b: &[u8], offset: usize) -> Result<[f32; N], String> {
    let mut out = [0.; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = f32::from_le_bytes(
            b.get(offset + i * 4..offset + i * 4 + 4)
                .ok_or("truncated GMDL float")?
                .try_into()
                .unwrap(),
        );
        if !v.is_finite() {
            return Err("nonfinite GMDL vertex".into());
        }
    }
    Ok(out)
}
pub fn parse(bytes: &[u8]) -> Result<Model, String> {
    let mut r = Reader { bytes, at: 0 };
    let version = r.u()?;
    if !(7..=9).contains(&version) {
        return Err(format!("unsupported GMDL version {version}"));
    }
    let n = r.be()?;
    if n > 4096 {
        return Err("GMDL dependency count exceeds limit".into());
    }
    let mut references = vec![];
    for _ in 0..n {
        let instance_id = r.be()?;
        let group_id = r.be()?;
        let type_id = r.be()?;
        references.push(ResourceKey {
            instance_id,
            group_id,
            type_id,
        });
    }
    let mesh_count = r.count(512)?;
    let _bounds = floats::<7>(r.take(28)?, 0)?;
    let mut triangles = vec![];
    let mut native_indices = vec![];
    for _ in 0..r.count(512)? {
        let primitive = r.u()?;
        let count = r.count(6_000_000)?;
        let bits = r.u()?;
        let size = r.u()? as usize;
        let width = match bits {
            16 => 2,
            32 => 4,
            _ => return Err("unsupported GMDL index width".into()),
        };
        if primitive != 4 || count % 3 != 0 || size != count * width {
            return Err("unsupported GMDL triangle buffer".into());
        }
        let data = r.take(size)?;
        native_indices.push((primitive, bits, data.to_vec()));
        triangles.push(
            data.chunks_exact(width)
                .map(|b| {
                    if width == 2 {
                        u16::from_le_bytes(b.try_into().unwrap()) as u32
                    } else {
                        u32::from_le_bytes(b.try_into().unwrap())
                    }
                })
                .collect::<Vec<_>>(),
        );
    }
    let mut formats = vec![];
    let mut native_formats = vec![];
    for _ in 0..r.count(512)? {
        let mut elements = vec![];
        let mut native_elements = vec![];
        for _ in 0..r.count(32)? {
            let b = r.take(12)?;
            native_elements.push(b.try_into().unwrap());
            if u16::from_le_bytes(b[..2].try_into().unwrap()) != 0 {
                return Err("GMDL multi-stream vertex format".into());
            }
            elements.push((
                u16::from_le_bytes(b[2..4].try_into().unwrap()) as usize,
                b[4],
                u32::from_le_bytes(b[8..12].try_into().unwrap()),
            ))
        }
        formats.push(elements);
        native_formats.push(native_elements);
    }
    let mut buffers = vec![];
    let mut native_buffers = vec![];
    for _ in 0..r.count(512)? {
        let format = r.u()? as usize;
        let count = r.count(1_000_000)?;
        let size = r.u()? as usize;
        let data = r.take(size)?;
        if count == 0 || size % count != 0 {
            return Err("invalid GMDL vertex buffer size".into());
        }
        let stride = size / count;
        if stride > 512 {
            return Err("GMDL vertex stride exceeds limit".into());
        }
        native_buffers.push((
            stride as u32,
            count as u32,
            data.to_vec(),
            native_formats
                .get(format)
                .ok_or("missing native GMDL vertex format")?
                .clone(),
        ));
        let format = formats.get(format).ok_or("missing GMDL vertex format")?;
        let mut p = Piece {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            bones: vec![],
            weights: vec![],
            material_id: 0,
        };
        for i in 0..count {
            let vertex = &data[i * stride..(i + 1) * stride];
            for &(offset, kind, code) in format {
                match (code, kind) {
                    (0, 2) => p.positions.push(floats(vertex, offset)?),
                    (2, 2) => p.normals.push(floats(vertex, offset)?),
                    (2, 5) => {
                        let n = vertex
                            .get(offset..offset + 3)
                            .ok_or("truncated GMDL packed normal")?;
                        p.normals
                            .push(std::array::from_fn(|j| n[j] as f32 * (2. / 255.) - 1.));
                    }
                    (6, 1) => p.uvs.push(floats(vertex, offset)?),
                    (14, 5) => p.bones.push(
                        vertex
                            .get(offset..offset + 4)
                            .ok_or("truncated GMDL bone indices")?
                            .try_into()
                            .unwrap(),
                    ),
                    (15, 8) => {
                        let w = vertex
                            .get(offset..offset + 4)
                            .ok_or("truncated GMDL bone weights")?;
                        p.weights.push(std::array::from_fn(|j| w[j] as f32 / 255.));
                    }
                    _ => {}
                }
            }
        }
        if p.positions.len() != count || p.normals.len() != count || p.uvs.len() != count {
            return Err("GMDL lacks supported position, normal or UV stream".into());
        }
        buffers.push(p);
    }
    let mut pieces = vec![];
    let mut native_geometry = vec![];
    for _ in 0..mesh_count {
        let ib = r.u()? as usize;
        let vb = r.u()? as usize;
        let (primitive_type, index_bits, index_bytes) = native_indices
            .get(ib)
            .ok_or("missing native GMDL index buffer")?;
        let (stride, vertex_count, vertex_bytes, declaration) = native_buffers
            .get(vb)
            .ok_or("missing native GMDL vertex buffer")?;
        native_geometry.push(NativeGeometry {
            primitive_type: *primitive_type,
            index_bits: *index_bits,
            index_bytes: index_bytes.clone(),
            stride: *stride,
            vertex_count: *vertex_count,
            vertex_bytes: vertex_bytes.clone(),
            declaration: declaration.clone(),
        });
        let mut p = buffers.get(vb).ok_or("missing GMDL vertex buffer")?.clone();
        p.indices = triangles
            .get(ib)
            .ok_or("missing GMDL triangle buffer")?
            .clone();
        if p.indices.iter().any(|&i| i as usize >= p.positions.len()) {
            return Err("GMDL triangle index exceeds vertex buffer".into());
        }
        pieces.push(p)
    }
    for p in &mut pieces {
        p.material_id = r.u()?;
    }
    if r.u()? != 0 {
        return Err("unsupported GMDL material table extension".into());
    }
    let mut materials = vec![];
    for _ in 0..r.count(512)? {
        let mut material = Material::default();
        if version != 9 || r.be()? != 0 {
            for _ in 0..r.count(64)? {
                let id = r.u()?;
                read_material_shader_data(id, &mut r, &mut material)?;
            }
        }
        materials.push(material)
    }
    if materials.len() != pieces.len() {
        return Err("GMDL material/mesh count mismatch".into());
    }
    let mut bone_ranges = vec![];
    for _ in 0..r.count(512)? {
        bone_ranges.push([r.u()?, r.u()?])
    }
    let parts = r.count(4096)?;
    let mut root_bones = 0;
    let mut anim_data = Vec::with_capacity(parts);
    let mut deform_bytes = 0;
    let mut deform_groups = 0;
    for i in 0..parts {
        let data = read_anim_data(&mut r, &mut deform_bytes, &mut deform_groups)?;
        if i == 0 {
            root_bones = data.field_70
        }
        anim_data.push(data);
    }
    let unknown_key = read_resource_key_le(&mut r)?;
    if r.at != bytes.len() {
        return Err("unparsed trailing GMDL data".into());
    }
    Ok(Model {
        version,
        references,
        pieces,
        materials,
        bone_ranges,
        root_bones,
        anim_data,
        unknown_key,
        native_geometry,
    })
}
/// TextureEntry encodes group and instance only. GMDLModelViewer resolves
/// .raster first, then .rw4 at exactly those IDs. Malformed indexed data fails.
pub fn texture_key(index: &ResourceIndex, key: ResourceKey) -> Result<ResourceKey, String> {
    if index.records.contains_key(&key) {
        return Ok(key);
    }
    let rw4 = ResourceKey {
        type_id: crate::asset_catalog::RW4,
        ..key
    };
    if key.type_id == crate::decode::TYPE_RASTER && index.records.contains_key(&rw4) {
        return Ok(rw4);
    }
    Err(format!("missing original GMDL texture {key:?}"))
}
pub fn texture(index: &ResourceIndex, key: ResourceKey) -> Result<crate::texture::Pixels, String> {
    let key = texture_key(index, key)?;
    let bytes = index.read(key)?;
    if let Some(p) = crate::texture::decode_rw4(&bytes) {
        return Ok(p);
    }
    let r = crate::decode::decode_raster_base(&bytes).ok_or("unsupported original GMDL raster")?;
    let format = match r.format {
        crate::decode::RasterFormat::Bc1 => u32::from_le_bytes(*b"DXT1"),
        crate::decode::RasterFormat::Bc2 => u32::from_le_bytes(*b"DXT3"),
        crate::decode::RasterFormat::Bc3 => u32::from_le_bytes(*b"DXT5"),
        crate::decode::RasterFormat::Rgba8 => 21,
    };
    crate::texture::decode_blocks(r.width, r.height, format, &r.data)
        .ok_or_else(|| "undecodable original GMDL raster".into())
}

/// Decode a GMDL-bound native raster's authored mip chain. Spore's standalone
/// raster record stores `mipmapCount` followed by a byte length and payload
/// for each level; RW4 texture containers have their own equivalent reader.
/// Unsupported chains fall back to the already-supported base-level decoder.
fn native_raster_mips(bytes: &[u8]) -> Option<Vec<crate::texture::Pixels>> {
    let word = |at: usize| {
        Some(u32::from_le_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if word(0)? != 1 {
        return None;
    }
    let mut width = word(4)?;
    let mut height = word(8)?;
    let levels = word(12)? as usize;
    let pixel_width = word(16)?;
    let format = word(20)?;
    if pixel_width != 8 || width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let max_levels = width.max(height).ilog2() as usize + 1;
    if levels == 0 || levels > max_levels {
        return None;
    }
    let mut at = 24usize;
    let mut mips = Vec::with_capacity(levels);
    for _ in 0..levels {
        let size = word(at)? as usize;
        at = at.checked_add(4)?;
        let expected = match format {
            20 => width as usize * height as usize * 3,
            21 | 22 => width as usize * height as usize * 4,
            0x3154_5844 => width.div_ceil(4) as usize * height.div_ceil(4) as usize * 8,
            0x3354_5844 | 0x3554_5844 => {
                width.div_ceil(4) as usize * height.div_ceil(4) as usize * 16
            }
            _ => return None,
        };
        if size != expected {
            return None;
        }
        let end = at.checked_add(size)?;
        mips.push(crate::texture::decode_blocks(
            width,
            height,
            format,
            bytes.get(at..end)?,
        )?);
        at = end;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Some(mips)
}

pub fn texture_mips(
    index: &ResourceIndex,
    key: ResourceKey,
) -> Result<Vec<crate::texture::Pixels>, String> {
    let key = texture_key(index, key)?;
    let bytes = index.read(key)?;
    if let Some(mips) =
        crate::texture::decode_rw4_mips(&bytes).or_else(|| native_raster_mips(&bytes))
    {
        return Ok(mips);
    }
    Ok(vec![texture(index, key)?])
}

fn gmdl_image(mips: Vec<crate::texture::Pixels>, srgb: bool) -> Option<Image> {
    let base = mips.first()?;
    let mut image = Image::new(
        Extent3d {
            width: base.width,
            height: base.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        base.rgba.clone(),
        if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|mip| mip.rgba).collect());
    Some(image)
}
pub fn mesh(piece: &Piece) -> Mesh {
    let basis = |p: [f32; 3]| [-p[0], p[2], p[1]];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        piece
            .positions
            .iter()
            .copied()
            .map(basis)
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        piece.normals.iter().copied().map(basis).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, piece.uvs.clone());
    mesh.insert_indices(Indices::U32(piece.indices.clone()));
    let _ = mesh.generate_tangents();
    mesh
}

/// Source bakedPaint is two float4 constants (ShaderData 021C). The preview
/// uses the first RGB for untextured building paint; native two-color shading
/// and its lighting remain unimplemented. Alpha here is a paint parameter.
pub fn preview_baked_paint_color(material: &Material) -> Option<[f32; 3]> {
    let bytes = material.shader_data.get(&0x21c)?;
    if bytes.len() != 32 {
        return None;
    }
    let words: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    words
        .iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        .then(|| [words[0], words[1], words[2]])
}
pub fn import(
    originals: &Originals,
    key: ResourceKey,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<OriginalModel, String> {
    let model = parse(&originals.resource_index.read(key)?)?;
    let mut surfaces = vec![];
    let mut combined = Piece {
        positions: vec![],
        normals: vec![],
        uvs: vec![],
        indices: vec![],
        bones: vec![],
        weights: vec![],
        material_id: 0,
    };
    for (i, (piece, info)) in model.pieces.iter().zip(&model.materials).enumerate() {
        if key.group_id == BUILDING_GROUP && info.textures.is_empty() && info.shader_data.is_empty()
        {
            // INFERRED (civ-polish2, `notes\civ-polish2-re.md` A7): a baked
            // building's material-less pieces are its window panes (flat
            // quads in a wall plane, 14 triangles per window), which retail
            // draws with windowInteriorBuilder: the window parts' shared
            // interior raster, unlit (lit windows by night).
            if let Some(material) = crate::material_binding::window_interior_material(&originals.resource_index, images) {
                surfaces.push(OriginalSurface {
                    handle: meshes.add(mesh(piece)),
                    material: materials.add(material),
                    source_section: i,
                    state_sections: vec![],
                });
                continue;
            }
            warn!("original building {:08X} surface {i}: material state absent; source surface omitted from preview",key.instance_id);
            continue;
        }
        let mut material = StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.7,
            cull_mode: None,
            double_sided: true,
            ..default()
        };
        for binding in &info.textures {
            if binding.sampler > 1 {
                continue;
            }
            let mips = texture_mips(&originals.resource_index, binding.key)?;
            let p = mips.first().ok_or("original GMDL raster has no base mip")?;
            if (key.group_id == FLORA_GROUP || FLORA_LOD_GROUPS.contains(&key.group_id))
                && binding.sampler == 0
                && p.rgba.chunks_exact(4).any(|pixel| pixel[3] < 250)
            {
                // Flora atlases contain cutout fronds/leaves. Opaque draws
                // turned their transparent borders into solid dark polygons.
                // The cutoff is a renderer adaptation; creature atlas alpha
                // can encode other data and must not use this flora policy.
                material.alpha_mode = AlphaMode::Mask(0.4);
            }
            let image = gmdl_image(mips, binding.sampler == 0)
                .ok_or("original GMDL raster has no base mip")?;
            let h = images.add(image);
            if binding.sampler == 0 {
                material.base_color_texture = Some(h)
            } else {
                material.normal_map_texture = Some(h)
            }
        }
        if material.base_color_texture.is_none() {
            let color = (key.group_id == BUILDING_GROUP)
                .then(|| preview_baked_paint_color(info))
                .flatten()
                .ok_or("original GMDL has neither diffuse atlas nor supported baked paint")?;
            material.base_color = Color::linear_rgb(color[0], color[1], color[2]);
        }
        let handle = meshes.add(mesh(piece));
        let material = materials.add(material);
        // `materialParams` (datum 0x210) for the Cell SH shader (`cell_sh`).
        crate::cell_sh::register_params(material.id(), info.parameters);
        surfaces.push(OriginalSurface {
            handle,
            material,
            source_section: i,
            state_sections: vec![],
        });
        let offset = combined.positions.len() as u32;
        combined.positions.extend_from_slice(&piece.positions);
        combined.normals.extend_from_slice(&piece.normals);
        combined.uvs.extend_from_slice(&piece.uvs);
        combined
            .indices
            .extend(piece.indices.iter().map(|&n| n + offset));
    }
    let material = surfaces.first().map(|s| s.material.clone());
    Ok(OriginalModel {
        instance: key.instance_id,
        source_definition: Some(key),
        handle: meshes.add(mesh(&combined)),
        material,
        surfaces,
        scale: 1.,
        root: None,
        ..Default::default()
    })
}
pub fn candidates(index: &ResourceIndex, group: u32) -> Vec<ResourceKey> {
    let mut keys = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == crate::decode::TYPE_GMDL && k.group_id == group)
        .collect::<Vec<_>>();
    keys.sort_by_key(|k| k.instance_id);
    keys
}
pub fn definition_key(key: ResourceKey) -> ResourceKey {
    ResourceKey {
        type_id: PROP,
        group_id: if key.group_id == CREATURE_GROUP {
            0x40626200
        } else {
            0x40666200
        },
        instance_id: key.instance_id,
    }
}

pub fn ensure(
    originals: &mut Originals,
    key: ResourceKey,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<OriginalModel, String> {
    if let Some(m) = originals.baked_models.get(&key) {
        return Ok(m.clone());
    }
    let model = import(originals, key, meshes, images, materials)?;
    originals.baked_models.insert(key, model.clone());
    Ok(model)
}
pub fn species_key(originals: &Originals, species: u32) -> Option<ResourceKey> {
    let keys = candidates(&originals.resource_index, CREATURE_GROUP);
    if keys.is_empty() {
        None
    } else {
        Some(keys[(species as usize * 37) % keys.len()])
    }
}

/// Exact source key for the authored starter creature visual. Keep this
/// separate from `species_key`: the latter intentionally varies NPC visuals.
pub fn player_source_key(originals: &Originals) -> Option<ResourceKey> {
    let key = ResourceKey {
        group_id: CREATURE_GROUP,
        instance_id: PLAYER_BASELINE_INSTANCE,
        type_id: crate::decode::TYPE_GMDL,
    };
    originals
        .resource_index
        .records
        .contains_key(&key)
        .then_some(key)
}

/// Strictly resolve the starter's native CreatureData anatomy and part
/// capabilities. Callers can use this for exact source metadata; rendering
/// the full GMDL itself does not claim that the native rig/animation runs.
pub fn player_source_anatomy(
    originals: &Originals,
) -> Result<crate::original_species::OriginalSpecies, String> {
    let key = player_source_key(originals).ok_or("installed starter creature GMDL is absent")?;
    crate::original_species::resolve(&originals.resource_index, &originals.part_catalog, key)
}

/// Player-side identity for an authored baseline model. Kept distinct from
/// `NativeCreature`, which marks NPC population visuals and is queried by
/// gameplay systems.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePlayerSource(pub ResourceKey);

/// Fit the source bounds to the creature actor's local footprint, centering
/// X/Z and placing its actual lowest vertex at local Y=-1.3. Source
/// basis/orientation is left intact so the authored front direction is kept.
pub fn player_fitted_transform(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
) -> Result<Transform, String> {
    let mesh = meshes
        .get(&model.handle)
        .ok_or("missing original player creature mesh")?;
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("missing original player creature positions".into());
    };
    let (min, max) = vertices.iter().map(|v| Vec3::from_array(*v)).fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), v| (min.min(v), max.max(v)),
    );
    let span = (max.x - min.x).max(max.z - min.z);
    if !span.is_finite() || span <= 0. {
        return Err("invalid original player creature bounds".into());
    }
    let scale = 1.9 / span;
    let center = (min + max) * 0.5;
    Ok(Transform::from_translation(Vec3::new(
        -center.x * scale,
        -1.3 - min.y * scale,
        -center.z * scale,
    ))
    .with_scale(Vec3::splat(scale)))
}

/// Add the full original starter creature below an actor. Every original
/// material surface is retained. `player_fitted_transform` centers the model
/// horizontally, fits its footprint to 1.9, and places its feet at local -1.3.
/// The imported basis already maps the native creature's authored up axis to
/// world Y; no speculative yaw or animation is applied here.
pub fn spawn_player_visual(
    commands: &mut Commands,
    originals: &Originals,
    meshes: &Assets<Mesh>,
    actor: Entity,
) -> bool {
    let Some(key) = player_source_key(originals) else {
        return false;
    };
    let Some(model) = originals.baked_models.get(&key) else {
        return false;
    };
    let Ok(transform) = player_fitted_transform(model, meshes) else {
        return false;
    };
    commands
        .entity(actor)
        .insert(NativePlayerSource(key))
        .with_children(|p| {
            for surface in &model.surfaces {
                p.spawn((
                    crate::stages::StageOwned,
                    crate::player_appearance::NativePlayerSurface(key),
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    source(key),
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                ));
            }
        });
    true
}
pub fn source(key: ResourceKey) -> crate::world_objects::OriginalWorldObject {
    crate::world_objects::OriginalWorldObject {
        definition: definition_key(key),
        model: key,
        name: format!("original baked {:08X}", key.instance_id),
    }
}
/// Render the installed whole creature on a child; actor scale, IDs, movement,
/// health and encounter state keep their existing simulation coordinate basis.
pub fn spawn_species_visual(
    commands: &mut Commands,
    originals: &Originals,
    meshes: &Assets<Mesh>,
    actor: Entity,
    species: u32,
) -> bool {
    let Some(key) = species_key(originals, species) else {
        return false;
    };
    let Some(model) = originals.baked_models.get(&key) else {
        return false;
    };
    let Ok(transform) =
        crate::world_objects::fitted_transform(model, meshes, Vec3::new(0., -1.0, 0.), 2.5)
    else {
        return false;
    };
    commands
        .entity(actor)
        .insert(NativeCreature)
        .with_children(|p| {
            for surface in &model.surfaces {
                p.spawn((
                    crate::stages::StageOwned,
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    source(key),
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                ));
            }
        });
    true
}
pub fn scenery(
    commands: &mut Commands,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    seed: u64,
) {
    scenery_where(commands, originals, meshes, images, materials, seed, 1.0, &|_| true);
}

/// `scenery` spread over `spread` times the area's width, keeping only the
/// spots `keep` accepts (the Creature stage spreads it over the planet's land
/// at retail distances). The random sequence is the same either way.
pub fn scenery_where(
    commands: &mut Commands,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    seed: u64,
    spread: f32,
    keep: &dyn Fn(Vec3) -> bool,
) {
    use rand::{Rng, SeedableRng};
    let keys = candidates(&originals.resource_index, FLORA_GROUP);
    if keys.is_empty() {
        return;
    }
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed ^ 0xF10A_05E4);
    // The stage planet's theme flora (planet_sky::theme_flora: terrainThemeFlora
    // slots, LargePlant1..3 / MediumPlant1..3 / SmallPlant1..3) at their
    // authored size; ADAPTED positions (the terrainDistribute grid and the
    // Flora / LiveFlora / Grass maps are not decoded yet).
    let theme = crate::planet_sky::current_theme_flora();
    let themed = theme.iter().any(Option::is_some);
    // On the stage planet the theme's flora is placed by `planet_flora`
    // (terrainDistribute groves); only the rocks below stay here.
    let plants = if crate::planet_flora::replaces_scatter(spread) { 0 } else { 64 };
    // Stable installed species pool; retain all material boundaries and atlas UVs.
    for i in 0..plants {
        let key = if themed {
            // One large, two medium, one small per four points.
            let slot = match i % 4 {
                0 => (i / 4) % 3,
                1 | 2 => 3 + (i / 2) % 3,
                _ => 6 + (i / 4) % 3,
            };
            match theme.get(slot).copied().flatten().or_else(|| theme.iter().flatten().next().copied()) {
                Some(k) => k,
                None => continue,
            }
        } else {
            keys[(i % 8 * 31) % keys.len()]
        };
        let result = (|| {
            let model = ensure(originals, key, meshes, images, materials)?;
            let position = Vec3::new(
                rng.random_range(-31.0..31.0) * spread,
                -0.6,
                rng.random_range(-31.0..31.0) * spread,
            );
            if position.x.abs() < 4. && position.z.abs() < 12. || !keep(position) {
                return Ok(());
            }
            let mut transform = crate::world_objects::fitted_transform(
                &model,
                meshes,
                position,
                rng.random_range(1.6..3.2),
            )?;
            // A thin authored trunk can have a tiny footprint and tall crown.
            // Fit to both dimensions so width-only fitting cannot tower over
            // the whole local world. Preserve native proportions and materials.
            if let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
                .get(&model.handle)
                .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION))
            {
                let min_y = vertices.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min);
                let max_y = vertices
                    .iter()
                    .map(|v| v[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                let height = max_y - min_y;
                if themed && height.is_finite() {
                    // Authored size (modelBoundingBox / obstacleCollisionHeight).
                    transform.scale = Vec3::ONE;
                    transform.translation.y = position.y - min_y;
                } else if height.is_finite() && height > 0. {
                    let max_height = if i % 4 == 0 {
                        rng.random_range(5.0..7.5)
                    } else {
                        rng.random_range(2.4..4.8)
                    };
                    let scale = transform.scale.x.min(max_height / height);
                    transform.scale = Vec3::splat(scale);
                    transform.translation.y = position.y - min_y * scale;
                }
            }
            transform.rotation =
                Quat::from_rotation_y(rng.random_range(0.0..std::f32::consts::TAU));
            for (ordinal, surface) in model.surfaces.iter().enumerate() {
                commands.spawn((
                    crate::stages::StageOwned,
                    Name::new(format!("original flora {i} surface {ordinal}")),
                    source(key),
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                ));
            }
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            warn!("original flora {key:?}: {e}")
        }
    }
    for i in 0..14 {
        let position = Vec3::new(
            rng.random_range(-23.0..23.0) * spread,
            -0.6,
            rng.random_range(-23.0..23.0) * spread,
        );
        if position.x.abs() < 4. && position.z.abs() < 12. || !keep(position) {
            continue;
        }
        let (instance_id, name) = if i % 2 == 0 {
            (0x07B2F264, "organic_smallrock1")
        } else {
            (0x07B2F267, "organic_smallrock2")
        };
        crate::world_objects::spawn_at(
            commands,
            originals,
            ResourceKey {
                type_id: PROP,
                group_id: 0x40842000,
                instance_id,
            },
            name,
            meshes,
            images,
            materials,
            position,
            rng.random_range(0.7..1.8),
        );
    }
}

#[derive(Clone)]
pub struct BodyTemplate {
    pub model: OriginalModel,
    pieces: Vec<Piece>,
    parameters: Vec<Vec<f32>>,
    rest_spine: crate::body::BodyProfile,
    radii: [f32; 32],
    anchors: Vec<(Vec3, f32)>,
    /// The retail metaball skin's render handles once it replaced this
    /// torso (`creature_skin_view`).
    pub skin: Option<crate::creature_skin_view::SkinTarget>,
}

/// The displayed body is the metaball skin (`creature_skin_view`).
fn skinned(originals: &Originals) -> bool {
    originals.baked_body.as_ref().is_some_and(|b| b.skin.is_some())
}

impl BodyTemplate {
    /// Preserve the native rest shape. Saved editor points describe changes
    /// from the default control cage, not a replacement horizontal body.
    fn deform_point(&self, point: Vec3, t: f32, profile: &crate::body::BodyProfile) -> Vec3 {
        let baseline = crate::body::BodyProfile::default();
        if profile == &baseline {
            return point;
        }
        let old = body_frame_at(&baseline, t);
        let new = body_frame_at(profile, t);
        let native = body_frame_at(&self.rest_spine, t);
        let delta = point - native.center;
        let lateral = delta - native.tangent * delta.dot(native.tangent);
        let rotation = Quat::from_rotation_arc(old.tangent, new.tangent);
        native.center
            + (new.center - old.center)
            + rotation * (delta + lateral * (new.radius / old.radius - 1.))
    }

    pub fn attachment(&self, profile: &crate::body::BodyProfile, bone: u32) -> Vec3 {
        if let Some(&(point, t)) = self.anchors.get((bone % 8) as usize) {
            self.deform_point(point, t, profile)
        } else {
            let baseline = crate::body::BodyProfile::default();
            let t = if bone % 8 == 7 { 0. } else { 0.5 };
            let native = body_frame_at(&self.rest_spine, t);
            let old = body_frame_at(&baseline, t);
            let rotation = Quat::from_rotation_arc(old.tangent, native.tangent);
            let point = native.center
                + rotation
                    * (baseline.slot(bone) - old.center)
                    * (source_radius_at(&self.radii, t) / old.radius);
            self.deform_point(point, t, profile)
        }
    }

    /// The fitted native vertebra positions (tail first) the editor's spine
    /// deforms; the metaball skin builds its vertebra balls on them.
    pub fn rest_spine(&self) -> &crate::body::BodyProfile {
        &self.rest_spine
    }

    /// A native rest-shape point moved with the edited spine (`t` = position
    /// along the native torso, 0 tail .. 1 head).
    pub fn deform(&self, point: Vec3, t: f32, profile: &crate::body::BodyProfile) -> Vec3 {
        self.deform_point(point, t, profile)
    }

    pub fn control_handle(&self, profile: &crate::body::BodyProfile, index: usize) -> Vec3 {
        let t = index as f32 / (profile.spine.len() - 1) as f32;
        let native = body_frame_at(&self.rest_spine, t);
        let old_radius = profile_at(&crate::body::BodyProfile::default(), t).1;
        let radius = source_radius_at(&self.radii, t) * profile.spine[index].radius / old_radius;
        self.deform_point(native.center, t, profile) + Vec3::Y * (radius + 0.13)
    }
}

/// Centre and radius of spine point `index` on the displayed body (the
/// deformed native body when installed), in the player's frame.
pub fn spine_center(originals: &Originals, profile: &crate::body::BodyProfile, index: usize) -> (Vec3, f32) {
    let p = &profile.spine[index.min(profile.spine.len() - 1)];
    let Some(b) = &originals.baked_body else {
        return (Vec3::from_array(p.position), p.radius);
    };
    let t = index as f32 / (profile.spine.len() - 1).max(1) as f32;
    let native = body_frame_at(&b.rest_spine, t);
    let old_radius = profile_at(&crate::body::BodyProfile::default(), t).1;
    (b.deform_point(native.center, t, profile), source_radius_at(&b.radii, t) * p.radius / old_radius)
}

pub fn attachment_point(
    originals: &Originals,
    profile: &crate::body::BodyProfile,
    bone: u32,
) -> Vec3 {
    if skinned(originals) {
        return crate::creature_skin_view::attachment_point(originals, profile, bone);
    }
    originals
        .baked_body
        .as_ref()
        .map_or_else(|| profile.slot(bone), |b| b.attachment(profile, bone))
}

/// Snap to the deformed native triangle surface, including its legs, instead
/// of the unrelated fallback capsule used by installations without GMDL data.
pub fn nearest_body_surface(
    originals: &Originals,
    profile: &crate::body::BodyProfile,
    point: Vec3,
) -> Vec3 {
    if skinned(originals) {
        return crate::creature_skin_view::nearest_surface(originals, profile, point);
    }
    let Some(template) = &originals.baked_body else {
        return crate::body::nearest_surface(profile, point);
    };
    let mut best = None::<(Vec3, f32)>;
    for (piece, parameters) in template.pieces.iter().zip(&template.parameters) {
        let vertices: Vec<_> = piece
            .positions
            .iter()
            .zip(parameters)
            .map(|(p, &t)| template.deform_point(Vec3::new(-p[0], p[2], p[1]), t, profile))
            .collect();
        for triangle in piece.indices.chunks_exact(3) {
            let p = closest_triangle(
                point,
                vertices[triangle[0] as usize],
                vertices[triangle[1] as usize],
                vertices[triangle[2] as usize],
            );
            let distance = p.distance_squared(point);
            if distance.is_finite() && best.is_none_or(|(_, d)| distance < d) {
                best = Some((p, distance));
            }
        }
    }
    best.map_or(point, |(p, _)| p)
}

/// The deformed body's triangles in the player's frame (the same surface as
/// [`nearest_body_surface`]), for picking a point under the cursor.
pub fn body_triangles(originals: &Originals, profile: &crate::body::BodyProfile) -> Vec<[Vec3; 3]> {
    if skinned(originals) {
        return crate::creature_skin_view::body_triangles(originals, profile);
    }
    let Some(template) = &originals.baked_body else {
        let s = crate::body::surface(profile);
        let v = |i: u32| Vec3::from_array(s.positions[i as usize]);
        return s.indices.chunks_exact(3).map(|t| [v(t[0]), v(t[1]), v(t[2])]).collect();
    };
    let mut out = Vec::new();
    for (piece, parameters) in template.pieces.iter().zip(&template.parameters) {
        let vertices: Vec<_> = piece
            .positions
            .iter()
            .zip(parameters)
            .map(|(p, &t)| template.deform_point(Vec3::new(-p[0], p[2], p[1]), t, profile))
            .collect();
        for t in piece.indices.chunks_exact(3) {
            out.push([vertices[t[0] as usize], vertices[t[1] as usize], vertices[t[2] as usize]]);
        }
    }
    out
}

fn closest_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    if ab.cross(ac).length_squared() < 1e-20 {
        let edge = |a: Vec3, b: Vec3| {
            let d = b - a;
            let length = d.length_squared();
            if length < 1e-20 {
                a
            } else {
                a + d * ((p - a).dot(d) / length).clamp(0., 1.)
            }
        };
        return [edge(a, b), edge(b, c), edge(c, a)]
            .into_iter()
            .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
            .unwrap();
    }
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0. && d2 <= 0. {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0. && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0. && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-20 {
        return [a, b, c]
            .into_iter()
            .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
            .unwrap();
    }
    a + ab * (vb / sum) + ac * (vc / sum)
}

pub fn control_handle(
    originals: &Originals,
    profile: &crate::body::BodyProfile,
    index: usize,
) -> Vec3 {
    originals.baked_body.as_ref().map_or_else(
        || {
            let p = &profile.spine[index];
            Vec3::from_array(p.position) + Vec3::Y * (p.radius + 0.18)
        },
        |b| b.control_handle(profile, index),
    )
}
/// Spine radius of `profile` over the default's at `t` (cross-section scale).
pub fn radius_ratio(profile: &crate::body::BodyProfile, t: f32) -> f32 {
    profile_at(profile, t).1 / profile_at(&crate::body::BodyProfile::default(), t).1
}

fn profile_at(body: &crate::body::BodyProfile, t: f32) -> (Vec3, f32) {
    let f = t * (body.spine.len() - 1) as f32;
    let i = (f.floor() as usize).min(body.spine.len() - 2);
    let u = (f - i as f32).clamp(0., 1.);
    (
        Vec3::from_array(body.spine[i].position)
            .lerp(Vec3::from_array(body.spine[i + 1].position), u),
        body.spine[i].radius * (1. - u) + body.spine[i + 1].radius * u,
    )
}

/// A body-relative section frame. The source body is laid across X/Y with its
/// long axis along Z after `mesh()`'s basis conversion, so the default profile
/// starts with matching section axes. Rebuilding the frame from the edited
/// spine lets the original cross section turn with a bent body.
#[derive(Clone, Copy, Debug)]
struct BodyFrame {
    center: Vec3,
    tangent: Vec3,
    side: Vec3,
    up: Vec3,
    radius: f32,
}

fn body_frame_at(body: &crate::body::BodyProfile, t: f32) -> BodyFrame {
    let t = t.clamp(0., 1.);
    let (center, radius) = profile_at(body, t);
    // Use a central difference across spine joints so the section direction is
    // stable there too. The one-sided endpoint case retains a finite tangent.
    let dt = 0.001;
    let before = profile_at(body, (t - dt).max(0.)).0;
    let after = profile_at(body, (t + dt).min(1.)).0;
    let tangent = (after - before).normalize_or(Vec3::NEG_Z);
    // Transport the default section axes with the tangent. Switching a world
    // reference axis near vertical produced an abrupt quarter-turn in skin.
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, tangent);
    let side = (rotation * Vec3::X).normalize_or(Vec3::X);
    let up = side.cross(tangent).normalize_or(Vec3::Y);
    BodyFrame {
        center,
        tangent,
        side,
        up,
        radius,
    }
}

/// Interpolate a softly widened source envelope instead of selecting one of
/// 32 hard vertical bins. The maximum at each sample retains the creature's
/// real outer contour; linear interpolation removes bin-edge radius jumps.
fn source_radius_at(radii: &[f32; 32], t: f32) -> f32 {
    let at = t.clamp(0., 1.) * 31.;
    let lo = at.floor() as usize;
    let hi = (lo + 1).min(31);
    let u = at - lo as f32;
    let widened = |i: usize| {
        radii[i.saturating_sub(1)..=(i + 1).min(31)]
            .iter()
            .copied()
            .fold(0f32, f32::max)
    };
    (widened(lo) * (1. - u) + widened(hi) * u).max(0.01)
}

fn remap_body_position(
    position: Vec3,
    t: f32,
    from: &crate::body::BodyProfile,
    to: &crate::body::BodyProfile,
) -> Vec3 {
    let old = body_frame_at(from, t);
    let new = body_frame_at(to, t);
    let delta = position - old.center;
    let lateral_x = delta.dot(old.side) / old.radius;
    let lateral_y = delta.dot(old.up) / old.radius;
    let axial = delta.dot(old.tangent);
    new.center
        + new.side * (lateral_x * new.radius)
        + new.up * (lateral_y * new.radius)
        + new.tangent * axial
}

/// Keep only triangles whose non-zero skin influences belong to the GMDL's
/// root-body range. This retains the authored triangle order, UVs, and material
/// identity while leaving separately attached limbs and details to editor parts.
fn root_body_piece(
    piece: &Piece,
    bone_base: u32,
    root_bones: u32,
) -> Result<Option<Piece>, String> {
    if piece.bones.len() != piece.positions.len() || piece.weights.len() != piece.positions.len() {
        return Err("original body has no bone weights".into());
    }
    let mut indices = Vec::new();
    for triangle in piece.indices.chunks_exact(3) {
        if triangle.iter().all(|&vertex| {
            (0..4).all(|j| {
                piece.weights[vertex as usize][j] == 0.
                    || bone_base + piece.bones[vertex as usize][j] as u32 / 3 < root_bones
            })
        }) {
            indices.extend_from_slice(triangle);
        }
    }
    if indices.is_empty() {
        return Ok(None);
    }

    // Compact vertices after partitioning so discarded limb bounds and unused
    // vertices do not affect the editable body envelope.
    let mut remap = std::collections::HashMap::new();
    let mut compact = Piece {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::with_capacity(indices.len()),
        bones: Vec::new(),
        weights: Vec::new(),
        material_id: piece.material_id,
    };
    for old in indices {
        let new = *remap.entry(old).or_insert_with(|| {
            let new = compact.positions.len() as u32;
            compact.positions.push(piece.positions[old as usize]);
            compact.normals.push(piece.normals[old as usize]);
            compact.uvs.push(piece.uvs[old as usize]);
            compact.bones.push(piece.bones[old as usize]);
            compact.weights.push(piece.weights[old as usize]);
            new
        });
        compact.indices.push(new);
    }
    Ok(Some(compact))
}

#[derive(Component)]
pub struct NativeCreature;

/// The reserved GMDL range also contains limb controllers. Resolve the actual
/// torso chain from native CreatureData rather than treating every root bone
/// as a vertebra. This mapping is restricted to the matching authored starter.
fn source_torso(
    index: &ResourceIndex,
    key: ResourceKey,
) -> Result<crate::body::BodyProfile, String> {
    let anatomy = crate::creature_data::parse(&index.read(ResourceKey {
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: key.instance_id,
        type_id: crate::creature_data::TYPE,
    })?)
    .map_err(|e| e.to_string())?;
    let chain: Vec<_> = anatomy
        .rigblocks
        .iter()
        .filter(|b| b.block_type() == 5)
        .collect();
    if chain.len() < 2
        || chain
            .iter()
            .enumerate()
            .any(|(i, b)| b.index() != i as i16 || b.parent_index() != i as i16 - 1)
    {
        return Err("native torso is not a contiguous vertebral chain".into());
    }
    let spine = chain
        .iter()
        .rev()
        .map(|b| {
            let p = b.position_bits().map(f32::from_bits);
            crate::body::SpinePoint {
                position: [-p[0], p[2], p[1]],
                radius: 1.,
            }
        })
        .collect();
    let profile = crate::body::BodyProfile { spine };
    if profile.problem().is_some() {
        return Err("native torso positions are invalid".into());
    }
    Ok(profile)
}

fn torso_parameter(
    bones: [u8; 4],
    weights: [f32; 4],
    base: u32,
    count: usize,
) -> Result<f32, String> {
    let mut sum = 0.;
    let mut weight = 0.;
    for j in 0..4 {
        if weights[j] == 0. {
            continue;
        }
        let bone = base as usize + bones[j] as usize / 3;
        if bone >= count || !weights[j].is_finite() || weights[j] < 0. {
            return Err("non-torso influence in compact body".into());
        }
        sum += (1. - bone as f32 / (count - 1) as f32) * weights[j];
        weight += weights[j];
    }
    if weight <= 0. {
        return Err("body vertex has no skin weights".into());
    }
    Ok((sum / weight).clamp(0., 1.))
}

fn inherited_body_parameters(
    anatomy: &crate::creature_data::CreatureData,
    count: usize,
) -> Result<Vec<f32>, String> {
    let mut parameters = Vec::new();
    for (index, block) in anatomy.rigblocks.iter().enumerate() {
        if block.index() != index as i16 {
            return Err("body control indices are not contiguous".into());
        }
        let mut ancestor = index;
        let mut found = None;
        for _ in 0..=anatomy.rigblocks.len() {
            let block = anatomy
                .rigblocks
                .get(ancestor)
                .ok_or("invalid body control parent")?;
            if block.block_type() == 5 && ancestor < count {
                found = Some(1. - ancestor as f32 / (count - 1) as f32);
                break;
            }
            ancestor = usize::try_from(block.parent_index())
                .map_err(|_| "body control has no vertebral ancestor")?;
        }
        parameters.push(found.ok_or("body control ancestry cycle")?);
    }
    Ok(parameters)
}

fn inherited_vertex_parameter(
    bones: [u8; 4],
    weights: [f32; 4],
    base: u32,
    parameters: &[f32],
) -> Result<f32, String> {
    let mut sum = 0.;
    let mut total = 0.;
    for j in 0..4 {
        if weights[j] == 0. {
            continue;
        }
        let t = parameters
            .get(base as usize + bones[j] as usize / 3)
            .ok_or("unresolved body influence")?;
        sum += t * weights[j];
        total += weights[j];
    }
    if !total.is_finite() || total <= 0. {
        return Err("body vertex has invalid weights".into());
    }
    Ok((sum / total).clamp(0., 1.))
}
/// Adapt the starter's authored body partition to the saved editor spine. Its
/// UVs and source atlas remain intact; the body shape is a preview adaptation,
/// and native body-region painting is still unresolved.
pub fn prepare_body(
    originals: &mut Originals,
    profile: &crate::body::BodyProfile,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<OriginalModel, String> {
    if originals.baked_body.is_none() {
        let key = ResourceKey {
            group_id: CREATURE_GROUP,
            // Use the exact starter whose full model is shown before editing.
            // Root-bone filtering below keeps its editable body while mounted
            // anatomy remains governed by the saved part list.
            instance_id: PLAYER_BASELINE_INSTANCE,
            type_id: crate::decode::TYPE_GMDL,
        };
        let raw = parse(&originals.resource_index.read(key)?)?;
        if raw.root_bones == 0
            || raw.root_bones == u32::MAX
            || raw.bone_ranges.len() != raw.pieces.len()
        {
            return Err("original creature body partition absent".into());
        }
        let mut pieces = vec![];
        let mut source_spine = source_torso(&originals.resource_index, key)?;
        let anatomy = crate::creature_data::parse(&originals.resource_index.read(ResourceKey {
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: key.instance_id,
            type_id: crate::creature_data::TYPE,
        })?)
        .map_err(|e| e.to_string())?;
        if anatomy.rigblocks.len() != raw.root_bones as usize {
            return Err("body control count mismatch".into());
        }
        let inherited = inherited_body_parameters(&anatomy, source_spine.spine.len())?;
        let mut imported = import(originals, key, meshes, images, materials)?;
        let fit = player_fitted_transform(&imported, meshes)?;
        for point in &mut source_spine.spine {
            point.position = fit
                .transform_point(Vec3::from_array(point.position))
                .to_array();
        }
        let mut anchors = Vec::new();
        for block in anatomy.rigblocks.iter().filter(|b| b.block_type() == 2) {
            let p = block.position_bits().map(f32::from_bits);
            anchors.push((
                fit.transform_point(Vec3::new(-p[0], p[2], p[1])),
                inherited[block.index() as usize],
            ));
        }
        if anchors.len() != 5 {
            return Err("starter has an unsupported attachment layout".into());
        }
        let mut surfaces = vec![];
        let mut parameters = vec![];
        for (i, p) in raw.pieces.iter().enumerate() {
            let base = raw.bone_ranges[i][0];
            let Some(mut compact) = root_body_piece(p, base, raw.root_bones)? else {
                continue;
            };
            parameters.push(
                compact
                    .bones
                    .iter()
                    .zip(&compact.weights)
                    .map(|(b, w)| inherited_vertex_parameter(*b, *w, base, &inherited))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            for v in &mut compact.positions {
                let q = fit.transform_point(Vec3::new(-v[0], v[2], v[1]));
                *v = [-q.x, q.z, q.y];
            }
            surfaces.push(imported.surfaces[i].clone());
            pieces.push(compact);
        }
        if pieces.is_empty() {
            return Err("original body partition has no geometry".into());
        }
        let mut radii = [0f32; 32];
        for (p, ts) in pieces.iter().zip(&parameters) {
            for (v, &t) in p.positions.iter().zip(ts) {
                let frame = body_frame_at(&source_spine, t);
                let delta = Vec3::new(-v[0], v[2], v[1]) - frame.center;
                let bin = (t * 31.) as usize;
                let r = Vec2::new(delta.dot(frame.side), delta.dot(frame.up)).length();
                radii[bin] = radii[bin].max(r)
            }
        }
        imported.surfaces = surfaces;
        imported.material = imported.surfaces.first().map(|s| s.material.clone());
        originals.baked_body = Some(BodyTemplate {
            model: imported,
            pieces,
            parameters,
            rest_spine: source_spine,
            radii,
            anchors,
            skin: None,
        });
    }
    if crate::creature_skin_view::enabled() {
        crate::creature_skin_view::adopt_body(originals.baked_body.as_mut().unwrap(), &originals.resource_index, meshes, images, materials);
    }
    let template = originals.baked_body.as_ref().unwrap();
    update_body(template, profile, meshes);
    Ok(template.model.clone())
}
pub fn update_body(
    template: &BodyTemplate,
    profile: &crate::body::BodyProfile,
    meshes: &mut Assets<Mesh>,
) {
    if template.skin.is_some() {
        crate::creature_skin_view::update_body(template, profile, meshes);
        return;
    }
    let profile = profile.validated();
    let edited = profile != crate::body::BodyProfile::default();
    let mut combined = Piece {
        positions: vec![],
        normals: vec![],
        uvs: vec![],
        indices: vec![],
        bones: vec![],
        weights: vec![],
        material_id: 0,
    };
    for ((piece, surface), parameters) in template
        .pieces
        .iter()
        .zip(&template.model.surfaces)
        .zip(&template.parameters)
    {
        let mut p = piece.clone();
        for (v, &t) in p.positions.iter_mut().zip(parameters) {
            let position = Vec3::new(-v[0], v[2], v[1]);
            let q = template.deform_point(position, t, &profile);
            *v = [-q.x, q.z, q.y]
        }
        let mut draw = mesh(&p);
        if edited {
            draw.compute_smooth_normals();
        }
        let _ = draw.generate_tangents();
        if let Some(mut m) = meshes.get_mut(&surface.handle) {
            *m = draw
        }
        let offset = combined.positions.len() as u32;
        combined.positions.extend_from_slice(&p.positions);
        combined.normals.extend_from_slice(&p.normals);
        combined.uvs.extend_from_slice(&p.uvs);
        combined
            .indices
            .extend(p.indices.iter().map(|&n| n + offset));
    }
    let mut draw = mesh(&combined);
    if edited {
        draw.compute_smooth_normals();
    }
    let _ = draw.generate_tangents();
    if let Some(mut m) = meshes.get_mut(&template.model.handle) {
        *m = draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_torso_parameter_uses_weights_and_chain_order() {
        assert_eq!(
            torso_parameter([0, 0, 0, 0], [1., 0., 0., 0.], 0, 9).unwrap(),
            1.
        );
        assert_eq!(
            torso_parameter([24, 0, 0, 0], [1., 0., 0., 0.], 0, 9).unwrap(),
            0.
        );
        assert_eq!(
            torso_parameter([0, 24, 0, 0], [0.25, 0.75, 0., 0.], 0, 9).unwrap(),
            0.25
        );
        assert!(torso_parameter([27, 0, 0, 0], [1., 0., 0., 0.], 0, 9).is_err());
    }

    #[test]
    fn body_partition_keeps_source_triangle_order_uvs_and_material() {
        let piece = Piece {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            normals: vec![[0., 0., 1.]; 4],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
            indices: vec![0, 1, 2, 1, 3, 2],
            bones: [[0, 0, 0, 0], [3, 0, 0, 0], [0, 0, 0, 0], [6, 0, 0, 0]].to_vec(),
            weights: [[1., 0., 0., 0.]; 4].to_vec(),
            material_id: 0x1234,
        };
        let body = root_body_piece(&piece, 0, 2).unwrap().unwrap();
        assert_eq!(body.material_id, piece.material_id);
        assert_eq!(body.indices, vec![0, 1, 2]);
        assert_eq!(body.positions.as_slice(), &piece.positions[..3]);
        assert_eq!(body.uvs.as_slice(), &piece.uvs[..3]);
    }

    #[test]
    fn animation_binding_retains_native_bytes_and_little_endian_keys() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x11; 56]);
        bytes.extend_from_slice(&[0x22; 56]);
        bytes.extend_from_slice(&0x89ab_cdefu32.to_le_bytes());
        bytes.extend_from_slice(&0x0102_0304u32.to_le_bytes()); // instance
        bytes.extend_from_slice(&0x1122_3344u32.to_le_bytes()); // type
        bytes.extend_from_slice(&0xaabb_ccddu32.to_le_bytes()); // group
        bytes.extend_from_slice(&2u32.to_le_bytes()); // deform groups
        bytes.extend_from_slice(&2u32.to_le_bytes()); // first group records
        bytes.extend_from_slice(&[0x31; 160]);
        bytes.extend_from_slice(&[0x32; 160]);
        bytes.extend_from_slice(&1u32.to_le_bytes()); // second group records
        bytes.extend_from_slice(&[0x33; 160]);
        bytes.extend_from_slice(&0x5566_7788u32.to_le_bytes()); // trailing unknownKey instance
        bytes.extend_from_slice(&0x99aa_bbccu32.to_le_bytes()); // trailing unknownKey type
        bytes.extend_from_slice(&0xddee_ff00u32.to_le_bytes()); // trailing unknownKey group

        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let mut deform_bytes = 0;
        let mut deform_groups = 0;
        let data = read_anim_data(&mut reader, &mut deform_bytes, &mut deform_groups).unwrap();
        assert_eq!(data.transform, [0x11; 56]);
        assert_eq!(data.field_38, [0x22; 56]);
        assert_eq!(data.field_70, 0x89ab_cdef);
        assert_eq!(
            data.key,
            ResourceKey {
                instance_id: 0x0102_0304,
                type_id: 0x1122_3344,
                group_id: 0xaabb_ccdd,
            }
        );
        assert_eq!(data.baked_deforms.len(), 2);
        assert_eq!(data.baked_deforms[0], vec![[0x31; 160], [0x32; 160]]);
        assert_eq!(data.baked_deforms[1], vec![[0x33; 160]]);
        assert_eq!(
            read_resource_key_le(&mut reader).unwrap(),
            ResourceKey {
                instance_id: 0x5566_7788,
                type_id: 0x99aa_bbcc,
                group_id: 0xddee_ff00,
            }
        );
        assert_eq!(reader.at, bytes.len());
        assert_eq!(deform_bytes, 3 * 160);
        assert_eq!(deform_groups, 2);
    }

    #[test]
    fn animation_binding_rejects_truncated_transform_or_deform_payload() {
        let empty = [0u8; 112];
        let mut reader = Reader {
            bytes: &empty[..111],
            at: 0,
        };
        let mut deform_bytes = 0;
        let mut deform_groups = 0;
        assert!(read_anim_data(&mut reader, &mut deform_bytes, &mut deform_groups).is_err());

        let mut bytes = vec![0; 56 * 2 + 4 + 12];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0x44; 159]);
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let mut deform_bytes = 0;
        let mut deform_groups = 0;
        assert!(read_anim_data(&mut reader, &mut deform_bytes, &mut deform_groups).is_err());
    }

    #[test]
    fn body_frames_follow_the_spine_and_remap_without_losing_cross_sections() {
        let baseline = crate::body::BodyProfile::default();
        let bent = crate::body::BodyProfile {
            spine: vec![
                crate::body::SpinePoint {
                    position: [-0.35, 0.05, 0.85],
                    radius: 0.25,
                },
                crate::body::SpinePoint {
                    position: [-0.25, 0.20, 0.45],
                    radius: 0.6,
                },
                crate::body::SpinePoint {
                    position: [0.20, 0.30, 0.],
                    radius: 0.72,
                },
                crate::body::SpinePoint {
                    position: [0.55, 0.50, -0.55],
                    radius: 0.5,
                },
                crate::body::SpinePoint {
                    position: [0.65, 0.65, -1.0],
                    radius: 0.3,
                },
            ],
        };
        let t = 0.43;
        let old = body_frame_at(&baseline, t);
        let new = body_frame_at(&bent, t);
        for frame in [old, new] {
            assert!((frame.side.length() - 1.).abs() < 1e-5);
            assert!((frame.up.length() - 1.).abs() < 1e-5);
            assert!((frame.tangent.length() - 1.).abs() < 1e-5);
            assert!(frame.side.dot(frame.up).abs() < 1e-5);
            assert!(frame.side.dot(frame.tangent).abs() < 1e-5);
            assert!(frame.up.dot(frame.tangent).abs() < 1e-5);
        }

        let source = old.center
            + old.side * (old.radius * 0.4)
            + old.up * (old.radius * -0.65)
            + old.tangent * 0.12;
        let unchanged = remap_body_position(source, t, &baseline, &baseline);
        assert!(unchanged.distance(source) < 1e-5);

        let deformed = remap_body_position(source, t, &baseline, &bent);
        let offset = deformed - new.center;
        assert!((offset.dot(new.side) - new.radius * 0.4).abs() < 1e-5);
        assert!((offset.dot(new.up) - new.radius * -0.65).abs() < 1e-5);
        assert!((offset.dot(new.tangent) - 0.12).abs() < 1e-5);
        assert!(deformed.is_finite());
    }

    #[test]
    fn section_frame_remains_continuous_near_vertical() {
        let frame = |y: f32| {
            let tangent = Vec3::new(0., y, -(1. - y * y).sqrt());
            let profile = crate::body::BodyProfile {
                spine: vec![
                    crate::body::SpinePoint {
                        position: [0.; 3],
                        radius: 0.5,
                    },
                    crate::body::SpinePoint {
                        position: tangent.to_array(),
                        radius: 0.5,
                    },
                ],
            };
            body_frame_at(&profile, 0.5)
        };
        let before = frame(0.949);
        let after = frame(0.951);
        assert!(before.side.dot(after.side) > 0.999);
        assert!(before.up.dot(after.up) > 0.999);
    }

    #[test]
    fn source_envelope_interpolates_and_stays_finite_at_ends() {
        let radii = std::array::from_fn(|i| i as f32);
        assert_eq!(source_radius_at(&radii, 0.), 1.);
        assert_eq!(source_radius_at(&radii, 1.), 31.);
        let midpoint = source_radius_at(&radii, 10.5 / 31.);
        assert!((midpoint - 11.5).abs() < 1e-5);
        assert!(source_radius_at(&[0.; 32], 0.5).is_finite());
    }

    #[test]
    fn fixed_shader_data_uses_registered_widths_and_preserves_bytes() {
        assert_eq!(shader_data_size(0x238), Some(1));
        assert_eq!(shader_data_size(0x210), Some(20));
        assert_eq!(shader_data_size(0x218), Some(0));
        assert_eq!(shader_data_size(0x340), None);

        let mut reader = Reader {
            bytes: &[0xa5, 0x18, 0x27],
            at: 0,
        };
        let mut material = Material::default();
        read_material_shader_data(0x238, &mut reader, &mut material).unwrap();
        read_material_shader_data(0x218, &mut reader, &mut material).unwrap();
        assert_eq!(material.shader_data[&0x238], [0xa5]);
        assert_eq!(material.shader_data[&0x218], Vec::<u8>::new());
        assert_eq!(reader.at, 1);
    }

    #[test]
    fn unsupported_shader_data_is_not_silently_skipped() {
        let mut reader = Reader { bytes: &[], at: 0 };
        assert!(read_material_shader_data(0x340, &mut reader, &mut Material::default()).is_err());
        assert_eq!(reader.at, 0);
    }

    #[test]
    fn native_raster_mips_decode_all_levels_and_reject_unsupported_layouts() {
        let mut raster = Vec::new();
        for word in [1u32, 4, 4, 2, 8, 21, 64] {
            raster.extend_from_slice(&word.to_le_bytes());
        }
        raster.extend((0..64).map(|n| n as u8));
        raster.extend_from_slice(&16u32.to_le_bytes());
        raster.extend((64..80).map(|n| n as u8));

        let mips = native_raster_mips(&raster).unwrap();
        assert_eq!(
            mips.iter()
                .map(|mip| (mip.width, mip.height))
                .collect::<Vec<_>>(),
            vec![(4, 4), (2, 2)]
        );
        assert_eq!(&mips[0].rgba[..4], &[2, 1, 0, 3]);
        assert_eq!(&mips[1].rgba[..4], &[66, 65, 64, 67]);

        let mut unsupported = raster.clone();
        unsupported[16..20].copy_from_slice(&4u32.to_le_bytes());
        assert!(native_raster_mips(&unsupported).is_none());
        let mut truncated = raster;
        truncated.pop();
        assert!(native_raster_mips(&truncated).is_none());
    }

    #[test]
    fn gmdl_images_upload_authored_mips_in_linear_or_color_space() {
        let mips = vec![
            crate::texture::Pixels {
                width: 2,
                height: 2,
                rgba: vec![1; 16],
            },
            crate::texture::Pixels {
                width: 1,
                height: 1,
                rgba: vec![2; 4],
            },
        ];
        let image = gmdl_image(mips.clone(), true).unwrap();
        assert_eq!(image.texture_descriptor.mip_level_count, 2);
        assert_eq!(
            image.data.as_deref().unwrap(),
            &[vec![1; 16], vec![2; 4]].concat()
        );
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        let normal = gmdl_image(mips, false).unwrap();
        assert_eq!(normal.texture_descriptor.format, TextureFormat::Rgba8Unorm);
    }

    #[test]
    #[ignore = "requires installed original Spore assets"]
    fn installed_gmdl_sources_upload_authored_raster_mip_chains() {
        fn inspect_model(index: &ResourceIndex, key: ResourceKey, label: &str) -> bool {
            let model = parse(&index.read(key).unwrap()).unwrap();
            let mut sampler_counts = [0usize; 2];
            let mut has_multiple_levels = false;
            for material in &model.materials {
                for binding in material.textures.iter().filter(|t| t.sampler <= 1) {
                    sampler_counts[binding.sampler as usize] += 1;
                    let mips = texture_mips(index, binding.key).unwrap();
                    assert!(!mips.is_empty(), "{label}: empty source mip chain");
                    let mut width = mips[0].width;
                    let mut height = mips[0].height;
                    let mut byte_total = 0usize;
                    for mip in &mips {
                        assert_eq!((mip.width, mip.height), (width, height), "{label}");
                        assert_eq!(
                            mip.rgba.len(),
                            width as usize * height as usize * 4,
                            "{label}: decoded mip byte length"
                        );
                        byte_total += mip.rgba.len();
                        width = (width / 2).max(1);
                        height = (height / 2).max(1);
                    }
                    let level_count = mips.len();
                    let image = gmdl_image(mips, binding.sampler == 0).unwrap();
                    let expected_format = if binding.sampler == 0 {
                        TextureFormat::Rgba8UnormSrgb
                    } else {
                        TextureFormat::Rgba8Unorm
                    };
                    assert_eq!(image.texture_descriptor.mip_level_count, level_count as u32);
                    assert_eq!(image.texture_descriptor.format, expected_format);
                    assert_eq!(image.data.as_ref().unwrap().len(), byte_total);
                    has_multiple_levels |= level_count > 1;
                    println!(
                        "{label} {key:?} sampler {}: {} levels, {}x{} base, {} uploaded bytes, {:?}",
                        binding.sampler,
                        level_count,
                        image.width(),
                        image.height(),
                        byte_total,
                        expected_format
                    );
                }
            }
            assert!(sampler_counts[0] > 0, "{label}: no diffuse sampler binding");
            assert!(sampler_counts[1] > 0, "{label}: no sampler-1 binding");
            has_multiple_levels
        }

        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let key = |group_id, instance_id| ResourceKey {
            group_id,
            instance_id,
            type_id: crate::decode::TYPE_GMDL,
        };

        let mut multiple_levels = inspect_model(
            &index,
            key(CREATURE_GROUP, PLAYER_BASELINE_INSTANCE),
            "starter creature",
        );
        let flora_key = candidates(&index, FLORA_GROUP)
            .into_iter()
            .find(|key| {
                index
                    .read(*key)
                    .ok()
                    .and_then(|bytes| parse(&bytes).ok())
                    .is_some_and(|model| {
                        model.materials.iter().any(|material| {
                            material.textures.iter().any(|t| t.sampler == 0)
                                && material.textures.iter().any(|t| t.sampler == 1)
                        })
                    })
            })
            .expect("installed original flora with diffuse and sampler-1 bindings");
        multiple_levels |= inspect_model(&index, flora_key, "original flora");
        multiple_levels |= inspect_model(&index, key(BUILDING_GROUP, 0x072C092C), "city hall");
        multiple_levels |= inspect_model(&index, key(BUILDING_GROUP, 0x072FA9BA), "house");
        assert!(
            multiple_levels,
            "none of the installed source textures had authored mips"
        );
    }

    #[test]
    #[ignore = "requires installed original Spore assets"]
    fn installed_creatures_and_flora_have_complete_geometry_and_skin() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut checked = 0;
        let mut errors = vec![];
        for group in [CREATURE_GROUP, FLORA_GROUP] {
            for key in candidates(&index, group) {
                let result = (|| {
                    let model = parse(&index.read(key)?)?;
                    assert_eq!(model.native_geometry.len(), model.pieces.len());
                    for (raw, piece) in model.native_geometry.iter().zip(&model.pieces) {
                        assert_eq!(
                            raw.vertex_bytes.len(),
                            raw.stride as usize * raw.vertex_count as usize
                        );
                        assert_eq!(raw.vertex_count as usize, piece.positions.len());
                        let position = raw
                            .declaration
                            .iter()
                            .find(|e| u32::from_le_bytes(e[8..12].try_into().unwrap()) == 0)
                            .unwrap();
                        let offset =
                            u16::from_le_bytes(position[2..4].try_into().unwrap()) as usize;
                        assert_eq!(
                            floats::<3>(&raw.vertex_bytes, offset).unwrap(),
                            piece.positions[0]
                        );
                        let decoded: Vec<_> = raw
                            .index_bytes
                            .chunks_exact(raw.index_bits as usize / 8)
                            .map(|v| {
                                if raw.index_bits == 16 {
                                    u16::from_le_bytes(v.try_into().unwrap()) as u32
                                } else {
                                    u32::from_le_bytes(v.try_into().unwrap())
                                }
                            })
                            .collect();
                        assert_eq!(decoded, piece.indices);
                    }
                    for (p, m) in model.pieces.iter().zip(&model.materials) {
                        if p.indices.is_empty() {
                            return Err("empty original geometry".to_string());
                        }
                        if !m.textures.iter().any(|t| t.sampler == 0) {
                            return Err("missing diffuse binding".to_string());
                        }
                        for t in &m.textures {
                            let original_texture = index.read(t.key)?;
                            let native = crate::native_texture::decode(&original_texture, None)?;
                            assert_eq!(native.faces.len(), 1);
                            assert!(!native.faces[0].is_empty());
                            let pixels = texture(&index, t.key)?;
                            if pixels.rgba.len()
                                != pixels.width as usize * pixels.height as usize * 4
                            {
                                return Err("invalid atlas length".into());
                            }
                        }
                    }
                    Ok::<_, String>(())
                })();
                match result {
                    Ok(()) => checked += 1,
                    Err(e) => errors.push(format!("{key:?}: {e}")),
                }
            }
        }
        println!(
            "{checked} complete original models; {} failures: {errors:?}",
            errors.len()
        );
        assert!(checked > 500);
        assert!(errors.is_empty());
    }
    #[test]
    #[ignore = "requires installed original Spore assets"]
    fn original_body_keeps_atlas_and_editor_changes() {
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = ResourceIndex::load(&install.packages);
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut profile = crate::body::BodyProfile::default();
        let body = crate::creature_skin_view::without_skin(|| {
            prepare_body(&mut originals, &profile, &mut meshes, &mut images, &mut materials)
        })
        .unwrap();
        let source = ResourceKey {
            group_id: CREATURE_GROUP,
            instance_id: PLAYER_BASELINE_INSTANCE,
            type_id: crate::decode::TYPE_GMDL,
        };
        assert_eq!(body.source_definition, Some(source));
        let raw = parse(&originals.resource_index.read(source).unwrap()).unwrap();
        assert_eq!(raw.root_bones, 18);
        assert_eq!(raw.bone_ranges, vec![[0, 34]]);
        assert_eq!(
            source_torso(&originals.resource_index, source)
                .unwrap()
                .spine
                .len(),
            9
        );
        assert!(originals
            .baked_body
            .as_ref()
            .unwrap()
            .pieces
            .iter()
            .all(|piece| piece
                .bones
                .iter()
                .zip(&piece.weights)
                .all(|(bones, weights)| (0..4).all(|j| weights[j] == 0. || bones[j] / 3 < 18))));
        assert!(raw.materials.iter().flat_map(|m| &m.textures).any(|t| {
            t.sampler == 0
                && t.key.instance_id == PLAYER_BASELINE_INSTANCE
                && t.key.group_id == 0x40622900
        }));
        assert_eq!(
            body.surfaces.len(),
            originals.baked_body.as_ref().unwrap().pieces.len()
        );
        assert!(originals
            .baked_body
            .as_ref()
            .unwrap()
            .pieces
            .iter()
            .zip(&raw.pieces)
            .all(|(body, source)| body.indices.len() < source.indices.len()));
        let before = meshes
            .get(&body.draw_mesh())
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .clone();
        let template = originals.baked_body.as_ref().unwrap();
        assert!(
            template
                .pieces
                .iter()
                .any(|p| p.bones.iter().zip(&p.weights).any(|(b, w)| {
                    b.iter()
                        .zip(w)
                        .any(|(&b, &w)| w > 0. && (9..=12).contains(&(b / 3)))
                })),
            "native leg influences remain in the editable body"
        );
        for (piece, surface) in template.pieces.iter().zip(&template.model.surfaces) {
            let VertexAttributeValues::Float32x3(positions) = meshes
                .get(&surface.handle)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
            else {
                panic!("body positions")
            };
            assert_eq!(
                positions,
                &piece
                    .positions
                    .iter()
                    .map(|p| [-p[0], p[2], p[1]])
                    .collect::<Vec<_>>(),
                "default controls preserve fitted native rest geometry"
            );
        }
        let before_uv = meshes
            .get(&body.draw_mesh())
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .unwrap()
            .clone();
        assert!(body.surfaces.iter().all(|s| {
            materials
                .get(&s.material)
                .unwrap()
                .base_color_texture
                .is_some()
        }));
        assert!(body.surfaces.iter().all(|s| {
            materials
                .get(&s.material)
                .unwrap()
                .normal_map_texture
                .is_some()
        }));
        profile.spine[2].radius *= 1.5;
        update_body(
            originals.baked_body.as_ref().unwrap(),
            &profile,
            &mut meshes,
        );
        assert_ne!(
            format!("{before:?}"),
            format!(
                "{:?}",
                meshes
                    .get(&body.draw_mesh())
                    .unwrap()
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
            )
        );
        assert_eq!(
            meshes
                .get(&body.draw_mesh())
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_UV_0)
                .unwrap(),
            &before_uv
        );
        for key in originals
            .resource_index
            .records
            .keys()
            .filter(|k| k.instance_id == 0x2538D7D9)
        {
            println!("terrain detail resource {key:?}")
        }
        println!(
            "body triangles {}",
            originals
                .baked_body
                .as_ref()
                .unwrap()
                .pieces
                .iter()
                .map(|p| p.indices.len() / 3)
                .sum::<usize>()
        );
    }
    #[test]
    #[ignore = "requires installed original Spore packages; reads actual starter anatomy"]
    fn installed_native_starter_palette_matches_source_bytes() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let anatomy_key = crate::dbpf::ResourceKey {
            type_id: crate::creature_data::TYPE,
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
        };
        let bytes = index.read(anatomy_key).unwrap();
        let data = crate::creature_data::parse(&bytes).unwrap();
        let colors = data.skin_colors().unwrap();
        // Independently captured native properties in the lossless 0.56 cache
        // inventory: pink base/detail and white third layer, not a guessed tint.
        let expected = [
            [0.57646996, 0.34117597, 0.43137199],
            [0.57646996, 0.34117597, 0.43137199],
            [1., 1., 1.],
        ];
        for (actual, expected) in colors
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-7);
        }
        let u8 = crate::creature_data::CreatureData::quantize_skin_colors(colors);
        println!("starter native palette u8: {:?}", u8);
    }

    #[test]
    fn body_surface_snap_handles_faces_edges_and_degenerate_triangles() {
        assert_eq!(
            closest_triangle(Vec3::new(0.2, 0.3, 3.), Vec3::ZERO, Vec3::X, Vec3::Y),
            Vec3::new(0.2, 0.3, 0.)
        );
        assert_eq!(
            closest_triangle(Vec3::new(3., 0., 1.), Vec3::ZERO, Vec3::X, Vec3::Y),
            Vec3::X
        );
        assert_eq!(
            closest_triangle(Vec3::new(0.5, 2., 0.), Vec3::ZERO, Vec3::ZERO, Vec3::X),
            Vec3::X * 0.5
        );
        assert_eq!(
            closest_triangle(Vec3::ONE, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
            Vec3::ZERO
        );
    }
}
