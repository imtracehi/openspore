//! Cell-stage water presentation using installed native background rasters.
//! The water response and compositing roles are Bevy preview adaptations, not
//! a claim of native Spore shader or stage-compositor parity.

use crate::{asset_catalog::RW4, dbpf::ResourceKey, Originals};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// The Cell world's established pool half-extent, shared with `cell.rs`.
pub const POOL_HALF_EXTENT: f32 = 22.0;
const WATER_Y: f32 = -1.1;
const GRID_CELLS: usize = 80;
const CELL_GLOBALS: u32 = 0x2A3CE5B7;
const CELL_WORLD: u32 = 0x9B8E862F;
const CELL_BACKGROUND_MAP: u32 = 0x612B3191;

/// Full resource identity and import status for the renderer's source report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellEnvironmentSourceKind {
    /// A decoded raster image in an RW4 container.
    Texture,
    /// Static geometry (no current Cell background source uses this kind).
    Mesh,
    /// A decoded Cell world, globals, or palette data record.
    #[default]
    SerializedData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellEnvironmentLayerRole {
    /// Full-pool diffuse modulation, inferred from the installed resource name.
    SurfaceDiffuse,
    /// Soft additive light breakup, inferred from the installed resource name.
    BlobLight,
    /// Low-strength central glow, inferred from the installed resource name.
    GlowPatch,
}

/// Provenance for an imported image actually bound to a visible Cell surface.
/// The handle is kept here (and attached to the rendered entity) so render
/// reports can distinguish loaded source pixels from a model/mesh import.
#[derive(Component, Clone, Debug)]
pub struct CellEnvironmentLayerSource {
    pub name: &'static str,
    pub key: ResourceKey,
    pub kind: CellEnvironmentSourceKind,
    pub role: CellEnvironmentLayerRole,
    pub image: Handle<Image>,
    pub dimensions: [u32; 2],
}

#[derive(Clone, Debug)]
pub struct CellEnvironmentSource {
    pub name: &'static str,
    pub key: ResourceKey,
    pub kind: CellEnvironmentSourceKind,
    pub imported: bool,
    pub diagnostic: Option<String>,
    /// Original decoded image handle when this source is an imported raster.
    pub image: Option<Handle<Image>>,
    /// Bounds of decoded model vertices in the importer basis before placement.
    pub bounds: Option<[[f32; 3]; 2]>,
    /// Adapted X/Z footprint; textures record the target full-pool footprint.
    pub footprint: Option<[f32; 2]>,
    /// Uniform source-model transform scale used to fit the water tile.
    pub fitted_scale: Option<f32>,
    pub image_dimensions: Option<[u32; 2]>,
    pub render_role: Option<CellEnvironmentLayerRole>,
}

#[derive(Clone)]
struct NativeLayer {
    source: CellEnvironmentLayerSource,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

/// Prepared original background art plus the source records needed by capture
/// manifests. Missing native backgrounds leave the procedural water layer in
/// place and are reported with their full resource key.
#[derive(Resource, Clone, Default)]
pub struct CellEnvironmentAssets {
    layers: Vec<NativeLayer>,
    screen_diffuse: Option<CellEnvironmentLayerSource>,
    pub sources: Vec<CellEnvironmentSource>,
    /// Colors copied from the referenced native CellBackgroundMap resource.
    /// The radial interpolation used by the Bevy surface is an adaptation.
    pub native_background_colors: Vec<[f32; 3]>,
    /// Authored cCellGlobalsResource backgroundDistance when finite/positive.
    pub native_background_distance: Option<f32>,
}

impl CellEnvironmentAssets {
    /// Successfully decoded native raster sources, including the screen image
    /// bound as water diffuse and both images rendered on additive quads.
    pub fn texture_sources(&self) -> impl Iterator<Item = &CellEnvironmentLayerSource> {
        self.screen_diffuse
            .iter()
            .chain(self.layers.iter().map(|layer| &layer.source))
    }
}

/// Decode the named original Cell RW4 rasters. The installed resources are
/// standalone RASTER containers, not source meshes; the two soft effects are
/// placed on adapted X/Z quads while the opaque screen raster modulates the
/// water's full-pool diffuse channel.
pub fn prepare(
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> CellEnvironmentAssets {
    let mut result = CellEnvironmentAssets::default();
    read_cell_world_rules(originals, &mut result);
    for (name, instance_id, role, y, tint) in [
        (
            "cell_background_l0_screen",
            0xBD08BD8A,
            CellEnvironmentLayerRole::SurfaceDiffuse,
            WATER_Y,
            Color::WHITE,
        ),
        (
            "cell_background_l0_blob",
            0x5F6819F3,
            CellEnvironmentLayerRole::BlobLight,
            WATER_Y + 0.03,
            Color::srgba(0.34, 0.72, 0.86, 0.68),
        ),
        (
            "cell_background_glowpatch",
            0x65B3C7E6,
            CellEnvironmentLayerRole::GlowPatch,
            WATER_Y + 0.04,
            Color::srgba(0.48, 0.78, 0.92, 0.42),
        ),
    ] {
        let key = ResourceKey {
            type_id: RW4,
            group_id: 0,
            instance_id,
        };
        let loaded = originals
            .resource_index
            .read(key)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                crate::texture::decode_rw4(&bytes)
                    .ok_or_else(|| "native RW4 has no supported standalone raster".to_owned())
            });
        match loaded {
            Ok(pixels) => {
                let dimensions = [pixels.width, pixels.height];
                let image = images.add(Image::new(
                    Extent3d {
                        width: pixels.width,
                        height: pixels.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    pixels.rgba,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                ));
                let source = CellEnvironmentLayerSource {
                    name,
                    key,
                    kind: CellEnvironmentSourceKind::Texture,
                    role,
                    image,
                    dimensions,
                };
                result.sources.push(CellEnvironmentSource {
                    name,
                    key,
                    kind: CellEnvironmentSourceKind::Texture,
                    imported: true,
                    diagnostic: None,
                    image: Some(source.image.clone()),
                    bounds: None,
                    footprint: Some([POOL_HALF_EXTENT * 2.0; 2]),
                    fitted_scale: None,
                    image_dimensions: Some(dimensions),
                    render_role: Some(role),
                });
                match role {
                    CellEnvironmentLayerRole::SurfaceDiffuse => {
                        result.screen_diffuse = Some(source);
                    }
                    CellEnvironmentLayerRole::BlobLight | CellEnvironmentLayerRole::GlowPatch => {
                        let material = materials.add(StandardMaterial {
                            base_color: tint,
                            base_color_texture: Some(source.image.clone()),
                            alpha_mode: AlphaMode::Add,
                            cull_mode: None,
                            unlit: true,
                            ..default()
                        });
                        result.layers.push(NativeLayer {
                            source,
                            mesh: meshes.add(texture_quad()),
                            material,
                            transform: Transform::from_xyz(0.0, y, 0.0),
                        });
                    }
                }
            }
            Err(error) => result.sources.push(source_error_kind(
                name,
                key,
                CellEnvironmentSourceKind::Texture,
                error,
            )),
        }
    }
    info!(
        "Cell environment: {} native rasters imported, {} source records",
        result
            .sources
            .iter()
            .filter(|source| source.imported && source.kind == CellEnvironmentSourceKind::Texture)
            .count(),
        result.sources.len()
    );
    result
}

/// Spawn the deep pool floor and the imported original raster overlays. The
/// opaque screen raster tiles across the camera-following floor; the blob and
/// glow rasters become large additive light shafts at depth. Their roles are
/// inferred from the installed names and remain preview adaptations.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    assets: &CellEnvironmentAssets,
) {
    let floor = crate::cell_parallax::spawn_backdrop(
        commands,
        meshes,
        images,
        materials,
        assets
            .screen_diffuse
            .as_ref()
            .map(|source| source.image.clone()),
        &assets.native_background_colors,
    );
    if let Some(source) = &assets.screen_diffuse {
        commands.entity(floor).insert(source.clone());
    }

    let quad = meshes.add(crate::cell_parallax::sprite_quad());
    let mut rng = <rand_chacha::ChaCha8Rng as rand::SeedableRng>::seed_from_u64(0xB10B);
    for layer in &assets.layers {
        let spec = match layer.source.role {
            CellEnvironmentLayerRole::GlowPatch => crate::cell_parallax::SpriteLayer {
                depth: [50.0, 65.0],
                size: [45.0, 80.0],
                tile: 240.0,
                count: 5,
                tint: [0.40, 0.70, 0.95],
                alpha: [0.25, 0.45],
                additive: true,
                spin: 0.02,
                drift: 0.3,
            },
            _ => crate::cell_parallax::SpriteLayer {
                depth: [36.0, 48.0],
                size: [26.0, 48.0],
                tile: 200.0,
                count: 7,
                tint: [0.30, 0.58, 0.90],
                alpha: [0.25, 0.5],
                additive: true,
                spin: 0.02,
                drift: 0.3,
            },
        };
        let atlas = crate::cell_parallax::CellAtlas {
            image: layer.source.image.clone(),
            cols: 1,
            rows: 1,
        };
        let sprites =
            crate::cell_parallax::spawn_layer(commands, &quad, materials, &mut rng, &atlas, &spec);
        // Provenance is recorded once per original raster, on its first sprite.
        if let Some(first) = sprites.first() {
            commands.entity(*first).insert(layer.source.clone());
        }
        debug!("spawned native Cell raster layer {}", layer.source.name);
    }
}

/// Soft bathymetric color ramp with shallow caustic bands and a slight swell.
/// It deliberately uses geometry and authored vertex colors rather than a
/// temporary texture or randomized decor.
fn water_mesh(native_palette: &[[f32; 3]]) -> Mesh {
    let side = GRID_CELLS + 1;
    let mut positions = Vec::with_capacity(side * side);
    let mut colors = Vec::with_capacity(side * side);
    let mut uvs = Vec::with_capacity(side * side);
    for z in 0..side {
        for x in 0..side {
            let u = x as f32 / GRID_CELLS as f32;
            let v = z as f32 / GRID_CELLS as f32;
            let px = (u * 2.0 - 1.0) * POOL_HALF_EXTENT;
            let pz = (v * 2.0 - 1.0) * POOL_HALF_EXTENT;
            let radius = Vec2::new(px, pz).length() / (POOL_HALF_EXTENT * 1.42);
            let depth = smoothstep((radius - 0.08) / 0.92);
            let swell = ((px * 0.26 + pz * 0.11).sin() * (pz * 0.23 - px * 0.07).cos());
            let caustic = ((radius * 31.0 + swell * 1.2).sin() * 0.5 + 0.5).powf(10.0);
            // The endpoints include the surface glare and sandy bottom. Use
            // the authored blue water entries for the playable suspended view;
            // this selection is adapted until native depth sampling is ported.
            let shallow = native_palette
                .get(2)
                .or_else(|| native_palette.first())
                .copied()
                .unwrap_or([0.105, 0.49, 0.52]);
            let deep = native_palette
                .get(6)
                .or_else(|| native_palette.last())
                .copied()
                .unwrap_or([0.018, 0.095, 0.22]);
            let color: [f32; 3] = std::array::from_fn(|i| {
                let base = shallow[i] * (1.0 - depth) + deep[i] * depth;
                let glint = caustic * (1.0 - depth) * [0.055, 0.14, 0.16][i];
                (base + glint + swell * [0.006, 0.014, 0.016][i]).clamp(0.0, 1.0)
            });
            let wave_height = swell * 0.018 + caustic * 0.004;
            positions.push([px, wave_height, pz]);
            colors.push(
                Color::srgb(color[0], color[1], color[2])
                    .to_linear()
                    .to_f32_array(),
            );
            uvs.push([u, v]);
        }
    }
    let indices = water_indices(GRID_CELLS);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

/// Full-pool X/Z quad for a source raster with an alpha-controlled soft edge.
/// The winding faces toward +Y, matching the water top and gameplay camera.
fn texture_quad() -> Mesh {
    let extent = POOL_HALF_EXTENT;
    let positions = [
        [-extent, 0.0, -extent],
        [extent, 0.0, -extent],
        [-extent, 0.0, extent],
        [extent, 0.0, extent],
    ];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3]));
    mesh.compute_smooth_normals();
    mesh
}

fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn water_indices(cells: usize) -> Vec<u32> {
    let side = cells + 1;
    let mut indices = Vec::with_capacity(cells * cells * 6);
    for z in 0..cells {
        for x in 0..cells {
            let a = (z * side + x) as u32;
            let b = a + side as u32;
            // +X crossed with +Z points toward -Y. Reverse the grid winding
            // so the visible top face points toward the Cell camera (+Y).
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    indices
}

/// Read the original Cell globals' world/background references and the
/// referenced background color table. The resource identities and field
/// offsets come from Spore ModAPI's cCellGlobalsResource and
/// cCellBackgroundMapResource definitions in the local format reference.
fn read_cell_world_rules(originals: &Originals, result: &mut CellEnvironmentAssets) {
    let Some(global_key) = originals
        .resource_index
        .records
        .keys()
        .copied()
        .filter(|key| key.type_id == CELL_GLOBALS)
        .min_by_key(|key| (key.group_id, key.instance_id))
    else {
        return;
    };
    let bytes = match originals.resource_index.read(global_key) {
        Ok(bytes) => bytes,
        Err(error) => {
            result
                .sources
                .push(source_error("CellGlobals", global_key, error));
            return;
        }
    };
    let globals = match parse_cell_globals(&bytes) {
        Ok(globals) => globals,
        Err(error) => {
            result
                .sources
                .push(source_error("CellGlobals", global_key, error));
            return;
        }
    };
    result.sources.push(CellEnvironmentSource {
        name: "CellGlobals",
        key: global_key,
        kind: CellEnvironmentSourceKind::SerializedData,
        imported: true,
        diagnostic: None,
        image: None,
        bounds: None,
        footprint: None,
        fitted_scale: None,
        image_dimensions: None,
        render_role: None,
    });

    // CellGlobalsFile serializes all fields in this order and little-endian;
    // its reader consumes exactly 0x114 bytes.
    for instance_id in globals.world_ids {
        if instance_id == 0 {
            continue;
        }
        let Some(key) = resolve_cell_resource(originals, global_key, CELL_WORLD, instance_id)
        else {
            result.sources.push(source_error(
                "CellWorld",
                ResourceKey {
                    type_id: CELL_WORLD,
                    group_id: global_key.group_id,
                    instance_id,
                },
                "referenced world is absent from the loaded packages".into(),
            ));
            continue;
        };
        match originals.resource_index.read(key) {
            Ok(world_bytes) => match parse_cell_world(&world_bytes) {
                Ok(_) => result.sources.push(CellEnvironmentSource {
                    name: "CellWorld",
                    key,
                    kind: CellEnvironmentSourceKind::SerializedData,
                    imported: true,
                    diagnostic: None,
                    image: None,
                    bounds: None,
                    footprint: None,
                    fitted_scale: None,
                    image_dimensions: None,
                    render_role: None,
                }),
                Err(error) => result.sources.push(source_error("CellWorld", key, error)),
            },
            Err(error) => result.sources.push(source_error("CellWorld", key, error)),
        }
    }
    result.native_background_distance =
        (globals.background_distance > 0.0).then_some(globals.background_distance);

    let Some(background_id) = (globals.background_map_id != 0).then_some(globals.background_map_id)
    else {
        return;
    };
    let Some(background_key) =
        resolve_cell_resource(originals, global_key, CELL_BACKGROUND_MAP, background_id)
    else {
        let key = ResourceKey {
            type_id: CELL_BACKGROUND_MAP,
            group_id: global_key.group_id,
            instance_id: background_id,
        };
        result.sources.push(source_error(
            "CellBackgroundMap",
            key,
            "referenced background map is absent from the loaded packages".into(),
        ));
        return;
    };
    match originals.resource_index.read(background_key) {
        Ok(map_bytes) => match parse_background_map(&map_bytes) {
            Ok(colors) => {
                result.native_background_colors = colors;
                result.sources.push(CellEnvironmentSource {
                    name: "CellBackgroundMap",
                    key: background_key,
                    kind: CellEnvironmentSourceKind::SerializedData,
                    imported: true,
                    diagnostic: None,
                    image: None,
                    bounds: None,
                    footprint: None,
                    fitted_scale: None,
                    image_dimensions: None,
                    render_role: None,
                });
            }
            Err(error) => {
                result
                    .sources
                    .push(source_error("CellBackgroundMap", background_key, error))
            }
        },
        Err(error) => result
            .sources
            .push(source_error("CellBackgroundMap", background_key, error)),
    }
}

fn resolve_cell_resource(
    originals: &Originals,
    owner: ResourceKey,
    type_id: u32,
    instance_id: u32,
) -> Option<ResourceKey> {
    [owner.group_id, 0]
        .into_iter()
        .map(|group_id| ResourceKey {
            type_id,
            group_id,
            instance_id,
        })
        .find(|key| originals.resource_index.records.contains_key(key))
}

fn parse_background_map(bytes: &[u8]) -> Result<Vec<[f32; 3]>, String> {
    let count = read_u32(bytes, 0).ok_or("Cell background map has no entry count")? as usize;
    let entry_offset = read_u32(bytes, 4).ok_or("Cell background map has no entry offset")?;
    let expected_len = if count == 0 {
        8
    } else {
        entry_offset as usize + count * 16
    };
    if count > 256
        || (count == 0 && entry_offset != 0)
        || (count > 0 && entry_offset != 8)
        || bytes.len() != expected_len
    {
        return Err("unsupported Cell background map entry layout".into());
    }
    let mut colors = Vec::with_capacity(count);
    for index in 0..count {
        let offset = entry_offset as usize + index * 16;
        let red = read_f32(bytes, offset).ok_or("truncated Cell background map color")?;
        let green = read_f32(bytes, offset + 4).ok_or("truncated Cell background map color")?;
        let blue = read_f32(bytes, offset + 8).ok_or("truncated Cell background map color")?;
        let field_c = read_f32(bytes, offset + 12).ok_or("truncated Cell background map value")?;
        if [red, green, blue]
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || !field_c.is_finite()
        {
            return Err(format!(
                "invalid Cell background map color at entry {index}"
            ));
        }
        colors.push([red, green, blue]);
    }
    Ok(colors)
}

/// Validate the packed file layout used by SporeModder-FX CellWorldFile.read.
/// The contents are retained as source provenance; native population/advection
/// simulation is outside the environment renderer.
fn parse_cell_world(bytes: &[u8]) -> Result<(usize, usize), String> {
    let populate_count = read_u32(bytes, 0).ok_or("CellWorld has no population count")? as usize;
    let populate_offset = read_u32(bytes, 4).ok_or("CellWorld has no population offset")? as usize;
    let advect_count = read_u32(bytes, 8).ok_or("CellWorld has no advection count")? as usize;
    let advect_offset = read_u32(bytes, 12).ok_or("CellWorld has no advection offset")? as usize;
    if populate_count > 4096 || advect_count > 4096 {
        return Err("CellWorld entry count exceeds decode limit".into());
    }
    let expected_populate_offset = if populate_count == 0 { 0 } else { 16 };
    let expected_advect_offset = if advect_count == 0 {
        0
    } else {
        16 + populate_count * 12
    };
    if populate_offset != expected_populate_offset || advect_offset != expected_advect_offset {
        return Err("unsupported CellWorld entry offsets".into());
    }
    let expected_len = 16 + populate_count * 12 + advect_count * 24;
    if bytes.len() != expected_len {
        return Err("unsupported CellWorld byte count".into());
    }
    for index in 0..populate_count {
        let entry = populate_offset + index * 12;
        if bytes[entry + 4] > 1 {
            return Err(format!(
                "invalid CellWorld startTile value at entry {index}"
            ));
        }
    }
    for index in 0..advect_count {
        let entry = advect_offset + index * 24;
        for offset in [8, 12, 16] {
            let value = read_f32(bytes, entry + offset).ok_or("truncated CellWorld advection")?;
            if !value.is_finite() {
                return Err(format!("non-finite CellWorld advection at entry {index}"));
            }
        }
    }
    Ok((populate_count, advect_count))
}

struct CellGlobalRules {
    world_ids: [u32; 12],
    background_map_id: u32,
    background_distance: f32,
}

/// Exact serialized-file layout from SporeModder-FX's CellGlobalsFile.read,
/// checked through the end of the fixed-size 0x114-byte record.
fn parse_cell_globals(bytes: &[u8]) -> Result<CellGlobalRules, String> {
    if bytes.len() != 0x114 {
        return Err(format!(
            "unsupported CellGlobals byte count {} (expected 276)",
            bytes.len()
        ));
    }
    let mut world_ids = [0; 12];
    for (index, id) in world_ids.iter_mut().enumerate() {
        *id = read_u32(bytes, 4 + index * 4).ok_or("truncated CellGlobals world reference")?;
    }
    let background_map_id = read_u32(bytes, 0x40).ok_or("truncated CellGlobals background map")?;
    let background_distance =
        read_f32(bytes, 0x68).ok_or("truncated CellGlobals background distance")?;
    if !background_distance.is_finite() {
        return Err("non-finite CellGlobals background distance".into());
    }
    Ok(CellGlobalRules {
        world_ids,
        background_map_id,
        background_distance,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_f32(bytes: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn source_error(name: &'static str, key: ResourceKey, error: String) -> CellEnvironmentSource {
    source_error_kind(name, key, CellEnvironmentSourceKind::SerializedData, error)
}

fn source_error_kind(
    name: &'static str,
    key: ResourceKey,
    kind: CellEnvironmentSourceKind,
    error: String,
) -> CellEnvironmentSource {
    warn!("Cell environment source {name} {key:?}: {error}");
    CellEnvironmentSource {
        name,
        key,
        kind,
        imported: false,
        diagnostic: Some(error),
        image: None,
        bounds: None,
        footprint: None,
        fitted_scale: None,
        image_dimensions: None,
        render_role: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_globals_file_offsets_follow_serialized_field_order() {
        let mut bytes = vec![0; 0x114];
        for (index, id) in [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
            .into_iter()
            .enumerate()
        {
            bytes[4 + index * 4..8 + index * 4].copy_from_slice(&id.to_le_bytes());
        }
        bytes[0x40..0x44].copy_from_slice(&0x12345678u32.to_le_bytes());
        bytes[0x68..0x6c].copy_from_slice(&27.5f32.to_le_bytes());
        let parsed = parse_cell_globals(&bytes).unwrap();
        assert_eq!(parsed.world_ids, std::array::from_fn(|i| i as u32 + 1));
        assert_eq!(parsed.background_map_id, 0x12345678);
        assert_eq!(parsed.background_distance, 27.5);
        assert!(parse_cell_globals(&bytes[..0x113]).is_err());
    }

    #[test]
    fn background_map_parser_honors_sporemodder_header_and_rgb_entries() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for (color, depth) in [([0.1f32, 0.2, 0.3], 0.0f32), ([0.7, 0.8, 0.9], 1.0)] {
            for component in color {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
            bytes.extend_from_slice(&depth.to_le_bytes());
        }
        assert_eq!(
            parse_background_map(&bytes).unwrap(),
            vec![[0.1, 0.2, 0.3], [0.7, 0.8, 0.9]]
        );
        bytes[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert!(parse_background_map(&bytes).is_err());
    }

    #[test]
    fn cell_world_parser_checks_sporemodder_array_offsets() {
        let mut bytes = vec![0; 16 + 12 + 24];
        bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&16u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&28u32.to_le_bytes());
        bytes[20] = 1; // populated startTile bool + the three serialized pad bytes
        bytes[36..40].copy_from_slice(&3.5f32.to_le_bytes()); // advection strength
        bytes[40..44].copy_from_slice(&0.5f32.to_le_bytes()); // variance
        bytes[44..48].copy_from_slice(&2.0f32.to_le_bytes()); // period
        assert_eq!(parse_cell_world(&bytes), Ok((1, 1)));
        bytes[12..16].copy_from_slice(&16u32.to_le_bytes());
        assert!(parse_cell_world(&bytes).is_err());
    }

    #[test]
    fn every_water_grid_triangle_winds_toward_positive_y() {
        let cells = 4;
        let side = cells + 1;
        let indices = water_indices(cells);
        let vertices = (0..side)
            .flat_map(|z| (0..side).map(move |x| Vec3::new(x as f32, 0.0, z as f32)))
            .collect::<Vec<_>>();
        for triangle in indices.chunks_exact(3) {
            let a = vertices[triangle[0] as usize];
            let b = vertices[triangle[1] as usize];
            let c = vertices[triangle[2] as usize];
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }

    #[test]
    fn raster_quad_covers_one_pool_tile_and_winds_toward_positive_y() {
        let mesh = texture_quad();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("texture quad has positions");
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("texture quad has UVs");
        };
        assert_eq!(positions.len(), 4);
        assert_eq!(uvs, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);
        assert_eq!(
            positions,
            &[
                [-POOL_HALF_EXTENT, 0.0, -POOL_HALF_EXTENT],
                [POOL_HALF_EXTENT, 0.0, -POOL_HALF_EXTENT],
                [-POOL_HALF_EXTENT, 0.0, POOL_HALF_EXTENT],
                [POOL_HALF_EXTENT, 0.0, POOL_HALF_EXTENT],
            ]
        );
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("texture quad has u32 indices");
        };
        for triangle in indices.chunks_exact(3) {
            let a = Vec3::from_array(positions[triangle[0] as usize]);
            let b = Vec3::from_array(positions[triangle[1] as usize]);
            let c = Vec3::from_array(positions[triangle[2] as usize]);
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }
}
