//! Drawing Swarm effects with Bevy: one dynamic mesh per particle system
//! (camera-facing or source-aligned quads rebuilt on the CPU each frame), and
//! full-screen quads for screen effects.
//!
//! NATIVE: textures (installed RW4 rasters with their mips), atlas tiles and
//! frame animation, effdir materials (`effectAnimMaterial` strips with their
//! `animBlendRate`/`stripLength`), draw modes (decal = alpha blend, additive,
//! modulate, ignore/invert depth), sizes, colours and alpha from the curves.
//! ADAPTED: blending happens in Bevy's linear colour space; colours are taken
//! as sRGB; the strip frame rate is frames per second of particle age; the
//! velocity stretch adds `stretch * speed / 30` to the length; particles that
//! name a buffer draw (raw colour) through the buffer cameras of filters.rs
//! instead of into the frame; ribbons are camera-facing strips through their
//! points with the authored width / offset / colour curves; model particles
//! and model components are drawn with the installed RW4 model.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, CompareFunction, Extent3d, RenderPipelineDescriptor,
        SpecializedMeshPipelineError, TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
};

use super::filters::{SwarmBlendBackdrop, SwarmFilterBuffers, BUFFER_LAYER};
use super::format::{draw, pflag, ParticleDef, ResourceId, RibbonDef, ScreenDef};
use super::sim::{curve, curve3, loop_box_fade, ParticleSystem, RibbonState};
use super::{SwarmEffect, SwarmLibrary, SwarmSet};
use crate::{asset_catalog::RW4, dbpf::ResourceKey};

/// `effectAnimMaterial` and `effectAnimAdditiveMaterial` shader ids.
const SHADER_ANIM: u32 = 0x1754_507E;
const SHADER_ANIM_ADDITIVE: u32 = 0x628D_6012;
/// `genericAdditiveParticle_material` (`reg_file.txt`): the civic lamps'
/// glow (`civ_civicobjects_lamp1`, `center_glow_textureMap`, an opaque white
/// glow on black that only reads right added to the frame).
const SHADER_GENERIC_ADDITIVE: u32 = 0x92AB_CE76;
/// `galacticZpoleTiltAdditive_material` (the protoplanetary disks): its particles are drawn by `galactic_draw.rs` with the
/// vertex stage of the retail program; only the blend fallback of `material_blend` still names it.
const SHADER_GALACTIC_ADDITIVE: u32 = 0xBF4F_930A;

/// The blend of an effect material without the install's compiled states
/// (`material_blends`): the additive ones by name, the rest by alpha.
pub fn material_blend(shader: u32) -> Blend {
    match shader {
        SHADER_ANIM_ADDITIVE | SHADER_GENERIC_ADDITIVE | SHADER_GALACTIC_ADDITIVE => Blend::Additive,
        _ => Blend::Alpha,
    }
}

/// The compiled-state tables of the effect materials: material links
/// `0x40212000!N` (SporeModder-FX `MaterialStateLinks`: material id, state
/// count, textures, until -1) and their compiled states `0x40212001!N` (RW4
/// section 0x2000B, in link order); N = the shader quality tier.
const MATERIAL_LINKS_GROUP: u32 = 0x4021_2000;
const MATERIAL_STATES_GROUP: u32 = 0x4021_2001;
const MATERIAL_LINKS_TYPE: u32 = 0x0469_A3F7;
/// D3DRS_ALPHABLENDENABLE, D3DRS_SRCBLEND, D3DRS_DESTBLEND; D3DBLEND_ZERO,
/// D3DBLEND_ONE, D3DBLEND_SRCCOLOR, D3DBLEND_DESTCOLOR.
const RS_BLEND_ENABLE: u32 = 27;
const RS_SRC_BLEND: u32 = 19;
const RS_DEST_BLEND: u32 = 20;
const BLEND_ZERO: u32 = 1;
const BLEND_ONE: u32 = 2;
const BLEND_SRC_COLOR: u32 = 3;
const BLEND_DEST_COLOR: u32 = 9;

/// VERIFIED installed data: each material's blend from its first compiled
/// state's render states (destination ONE = additive, a colour product =
/// multiply, else alpha), from the highest installed quality tier. Of the 59
/// materials the installed effects use, 19 add (e.g. `effectAnimAdditive`,
/// `genericAdditiveParticle`, `effectStarMaterial`, `effectCallMaterial`,
/// `civLightColumn_material`, the planet rings, `SG_shields_*`, lightning).
pub fn material_blends(index: &crate::asset_catalog::ResourceIndex) -> HashMap<u32, Blend> {
    let mut out = HashMap::new();
    for tier in (0..4u32).rev() {
        let key = |group, type_id| ResourceKey { type_id, group_id: group, instance_id: tier };
        let (Ok(links), Ok(rw)) =
            (index.read(key(MATERIAL_LINKS_GROUP, MATERIAL_LINKS_TYPE)), index.read(key(MATERIAL_STATES_GROUP, RW4)))
        else {
            continue;
        };
        let Ok(sections) = crate::rw4::sections(&rw) else {
            continue;
        };
        let states: Vec<&[u8]> =
            sections.iter().filter(|s| s.0 == crate::rw4::T_COMPILED_STATE).map(|s| s.1).collect();
        for (id, first_state) in material_links(&links) {
            let Some(state) = states.get(first_state).and_then(|b| crate::material_state::parse(b).ok()) else {
                continue;
            };
            let value = |rs: u32| {
                state
                    .render_states
                    .iter()
                    .filter(|(group, _)| *group == 0)
                    .flat_map(|(_, pairs)| pairs)
                    .rev()
                    .find(|(key, _)| *key == rs)
                    .map(|(_, v)| *v)
            };
            if value(RS_BLEND_ENABLE) != Some(1) {
                continue;
            }
            let blend = match (value(RS_SRC_BLEND), value(RS_DEST_BLEND)) {
                (_, Some(BLEND_ONE)) => Blend::Additive,
                (Some(BLEND_DEST_COLOR), Some(BLEND_ZERO)) | (Some(BLEND_ZERO), Some(BLEND_SRC_COLOR)) => Blend::Multiply,
                _ => Blend::Alpha,
            };
            out.insert(id, blend);
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Pure: (material id, index of its first compiled state) of a material link
/// table (versions 0 and 1, big-endian).
pub fn material_links(links: &[u8]) -> Vec<(u32, usize)> {
    let word = |at: usize| links.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()));
    let mut out = Vec::new();
    let Some(version) = word(0) else {
        return out;
    };
    let mut at = 4;
    let mut state = 0usize;
    while let Some(id) = word(at) {
        at += 4;
        if id == u32::MAX {
            break;
        }
        let count = match version {
            0 => {
                let (Some(n), Some(textures)) = (links.get(at), links.get(at + 1)) else {
                    break;
                };
                at += 2 + *textures as usize * 8;
                *n as usize
            }
            1 => {
                let Some(t) = links.get(at..at + 2) else {
                    break;
                };
                at += 2 + u16::from_be_bytes([t[0], t[1]]) as usize * 12 + 32;
                let Some(flags) = links.get(at..at + 16) else {
                    break;
                };
                at += 16;
                flags.iter().filter(|b| **b != 0).count()
            }
            _ => break,
        };
        if count > 0 {
            out.push((id, state));
        }
        state += count;
    }
    out
}

/// Alpha exponent approximating Spore's gamma-space blending (see the shader).
const GAMMA: f32 = 2.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    Alpha,
    Additive,
    Multiply,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Depth {
    Test,
    Ignore,
    Invert,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(SwarmMaterialKey)]
pub struct SwarmMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// x: 0 alpha, 1 additive, 2 multiply; y: alpha cut-off.
    #[uniform(2)]
    pub params: Vec4,
    pub blend: Blend,
    pub depth: Depth,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SwarmMaterialKey {
    depth: u8,
}

impl From<&SwarmMaterial> for SwarmMaterialKey {
    fn from(m: &SwarmMaterial) -> Self {
        SwarmMaterialKey {
            depth: match m.depth {
                Depth::Test => 0,
                Depth::Ignore => 1,
                Depth::Invert => 2,
            },
        }
    }
}

impl Material for SwarmMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/swarm_particle.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        match self.blend {
            Blend::Alpha => AlphaMode::Blend,
            Blend::Additive => AlphaMode::Add,
            Blend::Multiply => AlphaMode::Multiply,
        }
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
            match key.bind_group_data.depth {
                1 => depth.depth_compare = Some(CompareFunction::Always),
                // Bevy uses reverse Z: "behind what is drawn" is Less.
                2 => depth.depth_compare = Some(CompareFunction::Less),
                _ => {}
            }
        }
        Ok(())
    }
}

/// How one texture slot draws.
#[derive(Clone, Debug)]
pub struct SlotLook {
    pub image: Handle<Image>,
    pub blend: Blend,
    pub depth: Depth,
    /// (frames per second, strip length) of an animated material strip.
    pub strip: Option<(f32, u32)>,
    /// The strip's frames are stacked along V (the installed strips are
    /// taller than wide), else along U.
    pub vertical: bool,
    pub material: Handle<SwarmMaterial>,
    /// Particle buffer the texture slot names (0 = the frame).
    pub buffer: u8,
    /// Buffer particles keep raw (non-sRGB) colour: they store normals and
    /// masks, not light.
    pub raw: bool,
}

#[derive(Resource, Default)]
pub struct SwarmRenderCache {
    textures: HashMap<(ResourceId, bool, bool), Option<(Handle<Image>, UVec2)>>,
    materials: HashMap<(AssetId<Image>, Blend, Depth, bool), Handle<SwarmMaterial>>,
    looks: HashMap<(u16, u32), Option<SlotLook>>,
    ribbon_looks: HashMap<(u16, u32), Option<SlotLook>>,
    /// Draw entity and its render layer per particle system / screen / ribbon uid.
    systems: HashMap<u64, (Entity, usize)>,
    screens: HashMap<u64, (Entity, usize)>,
    ribbons: HashMap<u64, (Entity, usize)>,
    white: Option<Handle<Image>>,
    pub missing_textures: HashSet<ResourceId>,
}

/// Registers the material and draw systems; the shader itself is embedded by
/// `SwarmFxRenderPlugin` (embedded paths resolve relative to the caller).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<SwarmMaterial>::default())
        .init_resource::<SwarmRenderCache>()
        .init_resource::<super::models::SwarmModelCache>()
        .init_resource::<super::models::SwarmAttachedDraws>()
        .init_resource::<super::filters::SwarmFilterFrame>()
        .init_resource::<super::filters::SwarmChainCache>()
        .init_resource::<super::filters::SwarmFilterBuffers>()
        .add_systems(
            PostUpdate,
            (
                super::filters::mark_game_cameras,
                super::filters::collect_filter_chains,
                draw_particles,
                draw_ribbons,
                draw_screens,
                super::models::draw_models,
            )
                .chain()
                .in_set(SwarmSet::Draw)
                .after(SwarmSet::Simulate)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
        );
    super::filters_gpu::build(app);
}

fn raster(originals: &crate::Originals, id: ResourceId, raw: bool, repeat: bool) -> Option<Image> {
    let key = ResourceKey {
        type_id: RW4,
        group_id: if id.group == u32::MAX { 0 } else { id.group },
        instance_id: id.instance,
    };
    let bytes = originals.resource_index.read(key).ok()?;
    let mips = crate::texture::decode_rw4_mips(&bytes)?;
    let first = mips.first()?;
    let mut image = Image::new(
        Extent3d {
            width: first.width,
            height: first.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        first.rgba.clone(),
        if raw {
            TextureFormat::Rgba8Unorm
        } else {
            TextureFormat::Rgba8UnormSrgb
        },
        RenderAssetUsages::default(),
    );
    let address = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|p| p.rgba).collect());
    Some(image)
}

/// One of the Cell water's dynamic textures (`cell_fluid_tex`).
fn is_fluid_texture(id: ResourceId) -> bool {
    (id.group == 0 || id.group == u32::MAX)
        && matches!(
            id.instance,
            crate::cell_fluid_tex::DENSITY_ID | crate::cell_fluid_tex::NORMAL_ID | crate::cell_fluid_tex::VELOCITY_ID
        )
}

impl SwarmRenderCache {
    fn texture_sized(
        &mut self,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        id: ResourceId,
        raw: bool,
        repeat: bool,
    ) -> Option<(Handle<Image>, UVec2)> {
        if let Some(found) = self.textures.get(&(id, raw, repeat)) {
            return found.clone();
        }
        // The Cell water's dynamic textures (`cell_fluid_tex`: the game makes
        // them through ITextureManager at level start, 0xE834D0) are not
        // installed rasters: the stage keeps their images up to date.
        let dynamic = (id.group == 0 || id.group == u32::MAX)
            .then(|| crate::cell_fluid_tex::image_for(id.instance, raw, repeat))
            .flatten()
            .map(|image| {
                let size = image.size();
                let handle = images.add(image);
                crate::cell_fluid_tex::register(id.instance, handle.clone());
                (handle, size)
            });
        let found = dynamic.or_else(|| {
            raster(originals, id, raw, repeat).map(|image| {
                let size = image.size();
                (images.add(image), size)
            })
        });
        if found.is_none() && self.missing_textures.insert(id) {
            warn!("Swarm texture {:08x}!{:08x} is not an installed raster", id.group, id.instance);
        }
        self.textures.insert((id, raw, repeat), found.clone());
        found
    }

    pub(super) fn texture(
        &mut self,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        id: ResourceId,
    ) -> Option<Handle<Image>> {
        if is_fluid_texture(id) {
            info!("A Swarm screen / filter chain samples the Cell water texture {:08x}", id.instance);
        }
        self.texture_sized(originals, images, id, false, false).map(|(h, _)| h)
    }

    fn white(&mut self, images: &mut Assets<Image>) -> Handle<Image> {
        self.white
            .get_or_insert_with(|| {
                images.add(Image::new(
                    Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    vec![255; 4],
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                ))
            })
            .clone()
    }

    fn material(
        &mut self,
        materials: &mut Assets<SwarmMaterial>,
        image: &Handle<Image>,
        blend: Blend,
        depth: Depth,
        vertical: bool,
        sprite: bool,
    ) -> Handle<SwarmMaterial> {
        self.materials
            .entry((image.id(), blend, depth, sprite))
            .or_insert_with(|| {
                materials.add(SwarmMaterial {
                    texture: image.clone(),
                    params: Vec4::new(
                        match blend {
                            Blend::Alpha => 0.0,
                            Blend::Additive => 1.0,
                            Blend::Multiply => 2.0,
                        },
                        0.0,
                        if vertical { 1.0 } else { 0.0 },
                        // ADAPTED: gamma-space look for alpha-blended sprites
                        // (light over dark). Additive sprites keep linear
                        // alpha: over mid-tone water that is closer to a
                        // gamma-space add. Screen overlays keep their alpha.
                        if sprite && blend == Blend::Alpha { GAMMA } else { 1.0 },
                    ),
                    blend,
                    depth,
                })
            })
            .clone()
    }

    /// Resolve what a particle component's texture slot draws.
    fn look(
        &mut self,
        lib: &SwarmLibrary,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        materials: &mut Assets<SwarmMaterial>,
        dir: u16,
        index: u32,
        def: &ParticleDef,
    ) -> Option<SlotLook> {
        if let Some(found) = self.looks.get(&(dir, index)) {
            return found.clone();
        }
        // Model particles are drawn by models.rs.
        let look = if def.has(pflag::MODEL) {
            None
        } else {
            self.slot_look(lib, originals, images, materials, dir, &def.texture, false, true)
        };
        self.looks.insert((dir, index), look.clone());
        look
    }

    /// Resolve what a ribbon component's texture slot draws (repeating, for
    /// scrolling strips).
    fn ribbon_look(
        &mut self,
        lib: &SwarmLibrary,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        materials: &mut Assets<SwarmMaterial>,
        dir: u16,
        index: u32,
        def: &RibbonDef,
    ) -> Option<SlotLook> {
        if let Some(found) = self.ribbon_looks.get(&(dir, index)) {
            return found.clone();
        }
        let look = self.slot_look(lib, originals, images, materials, dir, &def.texture, true, true);
        self.ribbon_looks.insert((dir, index), look.clone());
        look
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn slot_look(
        &mut self,
        lib: &SwarmLibrary,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        materials: &mut Assets<SwarmMaterial>,
        dir: u16,
        slot: &super::format::TextureSlot,
        repeat: bool,
        gamma: bool,
    ) -> Option<SlotLook> {
        // Textures that name a buffer draw into it (filters.rs) with raw
        // colour; the frame's own particles use buffer 0.
        let raw = slot.buffer != 0;
        let (texture, blend, depth, strip) = if slot.draw_mode == draw::MATERIAL {
            let material = lib.dir(dir).materials.get(&slot.resource.instance)?;
            let texture = material.sampler(2).or_else(|| material.first_texture())?;
            let shader = material.shader.instance;
            let strip = (shader == SHADER_ANIM || shader == SHADER_ANIM_ADDITIVE).then(|| {
                (
                    material.float(0, 0).unwrap_or(1.0),
                    material.float(0, 1).unwrap_or(1.0).max(1.0) as u32,
                )
            });
            (texture, lib.material_blends.get(&shader).copied().unwrap_or_else(|| material_blend(shader)), Depth::Test, strip)
        } else {
            let (blend, depth) = match slot.draw_mode {
                draw::DECAL | draw::DECAL_DEPTH | draw::ALPHA_TEST_DISSOLVE => (Blend::Alpha, Depth::Test),
                draw::DECAL_INVERT_DEPTH => (Blend::Alpha, Depth::Invert),
                draw::DECAL_IGNORE_DEPTH => (Blend::Alpha, Depth::Ignore),
                draw::ADDITIVE => (Blend::Additive, Depth::Test),
                draw::ADDITIVE_INVERT_DEPTH => (Blend::Additive, Depth::Invert),
                draw::ADDITIVE_IGNORE_DEPTH => (Blend::Additive, Depth::Ignore),
                draw::MODULATE => (Blend::Multiply, Depth::Test),
                _ => return None,
            };
            (slot.resource, blend, depth, None)
        };
        if is_fluid_texture(texture) {
            info!(
                "Swarm effect directory {dir:04x} samples the Cell water texture {:08x} (draw mode {:#x}, blend {blend:?})",
                texture.instance, slot.draw_mode
            );
        }
        let (image, size) = self.texture_sized(originals, images, texture, raw, repeat)?;
        let vertical = size.y > size.x;
        let material = self.material(materials, &image, blend, depth, vertical, gamma && !raw);
        Some(SlotLook {
            image,
            blend,
            depth,
            strip,
            vertical,
            material,
            buffer: slot.buffer,
            raw,
        })
    }
}

pub(super) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Vertex alpha for an alpha-blended sprite of (gamma-space) colour `src` and
/// alpha `alpha` over a backdrop of colour `dst`: the shader raises it to
/// [`GAMMA`], and the linear-space blend then matches Spore's gamma-space
/// blend `dst + alpha * (src - dst)` on average (least squares over the
/// channels, luminance weights). ADAPTED (see `SwarmBlendBackdrop`).
fn gamma_fit_alpha(src: Vec3, alpha: f32, dst: Vec3) -> f32 {
    const W: [f32; 3] = [0.23, 0.66, 0.11];
    let src = src.clamp(Vec3::ZERO, Vec3::ONE);
    let mut num = 0.0;
    let mut den = 0.0;
    for c in 0..3 {
        let (s, d) = (src[c], dst[c]);
        let target = srgb_to_linear((d + alpha * (s - d)).clamp(0.0, 1.0));
        let (s_lin, d_lin) = (srgb_to_linear(s), srgb_to_linear(d));
        num += W[c] * (target - d_lin) * (s_lin - d_lin);
        den += W[c] * (s_lin - d_lin) * (s_lin - d_lin);
    }
    if den < 1e-6 {
        return alpha;
    }
    (num / den).clamp(0.0, 1.0).powf(1.0 / GAMMA)
}

#[derive(Default)]
struct QuadBuffers {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    uv_b: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// Camera axes for billboards.
#[derive(Clone, Copy)]
struct View {
    position: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
}

/// Build the quads of one particle system. Vertices are relative to `origin`.
fn system_quads(
    sys: &ParticleSystem,
    def: &ParticleDef,
    look: &SlotLook,
    view: View,
    origin: Vec3,
    backdrop: Option<Vec3>,
    out: &mut QuadBuffers,
) {
    let Some(frame) = sys.frame else { return };
    let cols = def.tile_count[0].max(1) as u32;
    let rows = def.tile_count[1].max(1) as u32;
    let tiles = cols * rows;
    let mut order: Vec<usize> = (0..sys.particles.len()).collect();
    if look.blend == Blend::Alpha {
        // Back to front.
        order.sort_by(|&a, &b| {
            let da = (sys.particles[a].pos - view.position).length_squared();
            let db = (sys.particles[b].pos - view.position).length_squared();
            db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    let color_rgb = def.has(pflag::COLOR_VARY_RGB);
    for i in order {
        let p = &sys.particles[i];
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let size = curve(&def.size, t, 1.0) * (1.0 + def.size_vary * p.r[0]) * sys.mods.size * frame.scale;
        if !(size > 0.0) {
            continue;
        }
        let aspect = curve(&def.aspect, t, 1.0) * (1.0 + def.aspect_vary * p.r[1]);
        let mut alpha = (curve(&def.alpha, t, 1.0) * (1.0 + def.alpha_vary * p.r[3])).clamp(0.0, 1.0) * sys.mods.alpha;
        if def.has(pflag::LOOP_BOX) {
            alpha *= loop_box_fade(&frame, &def.emit_volume, &def.loop_box_alpha, p.pos);
        }
        if alpha <= 0.002 {
            continue;
        }
        let vary = Vec3::from_array(def.color_vary);
        let draw = if color_rgb {
            Vec3::from_array(p.rc)
        } else {
            Vec3::splat(p.r[4])
        };
        let color = ((curve3(&def.color, t) + vary * draw) * sys.mods.color).max(Vec3::ZERO);
        // Atlas tile and frame animation.
        let mut tile = def.frame_start as u32;
        if def.frame_random > 0 {
            tile += p.frame as u32 % def.frame_random as u32;
        }
        if def.frame_count > 0 {
            let step = if def.has(pflag::FRAMES_RELATIVE_SPEED) {
                (t * def.frame_speed * def.frame_count as f32) as u32
            } else {
                (p.age * def.frame_speed) as u32
            };
            tile = def.frame_start as u32 + (tile - def.frame_start as u32 + step) % def.frame_count as u32;
        }
        let tile = tile % tiles;
        let tw = 1.0 / cols as f32;
        let th = 1.0 / rows as f32;
        let mut u0 = (tile % cols) as f32 * tw;
        let mut v0 = (tile / cols) as f32 * th;
        let mut uw = tw;
        let mut vh = th;
        let (mut blend_t, mut du) = (0.0, 0.0);
        if let Some((rate, frames)) = look.strip {
            let n = frames.max(1);
            let position = p.age * rate + (p.frame as f32 % n as f32);
            let a = position.floor() as u32 % n;
            blend_t = position.fract();
            let cell = if look.vertical { &mut vh } else { &mut uw };
            *cell /= n as f32;
            let step = *cell;
            if look.vertical {
                v0 += a as f32 * step;
            } else {
                u0 += a as f32 * step;
            }
            du = if a + 1 < n { step } else { -(n as f32 - 1.0) * step };
        }
        // Orientation.
        let (mut right, mut up) = match def.align_mode {
            5 => (frame.rot * Vec3::X, frame.rot * Vec3::Y),
            1 => (Vec3::X, Vec3::NEG_Z),
            _ => (view.right, view.up),
        };
        let width = size;
        let mut height = size * aspect;
        if def.velocity_stretch > 0.0 {
            let v = p.vel - view.forward * p.vel.dot(view.forward);
            let speed = v.length();
            if speed > 1e-5 {
                up = v / speed;
                right = up.cross(view.forward).normalize_or_zero();
                height += def.velocity_stretch * speed / 30.0;
            }
        } else {
            let angle = (curve(&def.rotate, t, 0.0) + def.rotation_offset + def.rotation_vary * p.r[2]) * TAU;
            if angle != 0.0 {
                let (s, c) = angle.sin_cos();
                let (r0, u0v) = (right, up);
                right = r0 * c + u0v * s;
                up = u0v * c - r0 * s;
            }
        }
        let mut center = p.pos;
        for (k, w) in def.wiggles.iter().enumerate() {
            let phase = p.r[5] * TAU + k as f32;
            center += frame.vector(Vec3::from_array(w.wiggle_direction)) * (TAU * w.time_rate * p.age + phase).sin();
        }
        let hx = right * (width * 0.5);
        let hy = up * (height * 0.5);
        let base = out.positions.len() as u32;
        let c = center - origin;
        let alpha = match backdrop {
            Some(dst) if look.blend == Blend::Alpha && !look.raw => gamma_fit_alpha(color, alpha, dst),
            _ => alpha,
        };
        let rgba = if look.raw {
            [color.x, color.y, color.z, alpha]
        } else {
            [srgb_to_linear(color.x), srgb_to_linear(color.y), srgb_to_linear(color.z), alpha]
        };
        for (corner, uv) in [
            (c - hx + hy, [u0, v0]),
            (c + hx + hy, [u0 + uw, v0]),
            (c - hx - hy, [u0, v0 + vh]),
            (c + hx - hy, [u0 + uw, v0 + vh]),
        ] {
            out.positions.push(corner.to_array());
            out.uv.push(uv);
            out.uv_b.push([blend_t, du]);
            out.colors.push(rgba);
        }
        out.indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
}

fn write_mesh(mesh: &mut Mesh, q: QuadBuffers) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, q.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, q.uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, q.uv_b);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, q.colors);
    mesh.insert_indices(Indices::U32(q.indices));
}

fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let q = QuadBuffers {
        positions: vec![[0.0; 3]; 3],
        uv: vec![[0.0; 2]; 3],
        uv_b: vec![[0.0; 2]; 3],
        colors: vec![[0.0; 4]; 3],
        indices: vec![0, 1, 2],
    };
    write_mesh(&mut mesh, q);
    mesh
}

/// Marker for render-side entities owned by the Swarm runtime.
#[derive(Component)]
pub struct SwarmDraw;

#[allow(clippy::too_many_arguments)]
fn draw_particles(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut cache: ResMut<SwarmRenderCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SwarmMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    buffers: Res<SwarmFilterBuffers>,
    backdrop: Res<SwarmBlendBackdrop>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&mut Transform, &Mesh3d, &mut MeshMaterial3d<SwarmMaterial>, &mut Visibility), With<SwarmDraw>>,
) {
    let Some(originals) = originals else { return };
    let Some(camera) = camera.iter().next() else { return };
    let view = View {
        position: camera.translation(),
        right: camera.right().as_vec3(),
        up: camera.up().as_vec3(),
        forward: camera.forward().as_vec3(),
    };
    let hidden = editor.is_some_and(|e| e.open);
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_particles(&lib, &mut |sys, def| {
            // The galactic sprite programs (galacticSpinTilt / galacticStar / galacticZpoleTilt) are drawn by galactic_draw.rs.
            if super::galactic_draw::is_galactic(&lib, sys.dir, def) {
                return;
            }
            let Some(look) = cache.look(&lib, &originals, &mut images, &mut materials, sys.dir, sys.index, def) else {
                return;
            };
            let Some(frame) = sys.frame else { return };
            seen.insert(sys.uid);
            let mut quads = QuadBuffers::default();
            // A buffer nothing reads is not drawn (retail draws it for no one).
            let drawn = look.buffer == 0 || buffers.wants(look.buffer);
            if !hidden && drawn {
                system_quads(sys, def, &look, view, frame.origin, backdrop.0, &mut quads);
            }
            let visible = !quads.indices.is_empty();
            if std::env::var_os("OPENSPORE_FX_DEBUG").is_some() && !sys.particles.is_empty() {
                info!(
                    "particles {} of fx {:08x}: {} alive, {} verts, blend {:?} depth {:?} align {} origin {:?} first {:?} layer {}",
                    sys.index,
                    effect.effect,
                    sys.particles.len(),
                    quads.positions.len(),
                    look.blend,
                    look.depth,
                    def.align_mode,
                    frame.origin,
                    sys.particles.first().map(|p| p.pos),
                    effect.layer
                );
                if quads.positions.len() >= 4 {
                    info!(
                        "  quad {:?} colour {:?} mods {:?} scale {}",
                        &quads.positions[..4],
                        quads.colors[0],
                        sys.mods,
                        frame.scale
                    );
                }
            }
            let layer = if look.buffer == 0 {
                effect.layer
            } else {
                BUFFER_LAYER + look.buffer as usize
            };
            match cache.systems.get(&sys.uid).copied().and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d))) {
                Some((entity, drawn_layer, (mut transform, mesh, mut material, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        cache.systems.insert(sys.uid, (entity, layer));
                    }
                    transform.translation = frame.origin;
                    if material.0 != look.material {
                        material.0 = look.material.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if visible {
                        if let Some(mut m) = meshes.get_mut(&mesh.0) {
                            write_mesh(&mut m, quads);
                        }
                    }
                }
                None => {
                    let mut mesh = empty_mesh();
                    if visible {
                        write_mesh(&mut mesh, quads);
                    }
                    let entity = commands
                        .spawn((
                            SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(look.material.clone()),
                            Transform::from_translation(frame.origin),
                            if visible { Visibility::Visible } else { Visibility::Hidden },
                            NoFrustumCulling,
                            NotShadowCaster,
                            NotShadowReceiver,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    cache.systems.insert(sys.uid, (entity, layer));
                }
            }
        });
    }
    cache.systems.retain(|uid, (entity, _)| {
        if seen.contains(uid) {
            return true;
        }
        if let Ok(mut e) = commands.get_entity(*entity) {
            e.despawn();
        }
        false
    });
}

/// Strip of one ribbon: `m` samples along its polyline (resampled by arc
/// length), camera-facing, width / offset / colour curves over the length,
/// alpha and colour curves over the ribbon's life.
fn ribbon_quads(
    r: &RibbonState,
    def: &RibbonDef,
    look: &SlotLook,
    view: View,
    origin: Vec3,
    backdrop: Option<Vec3>,
    out: &mut QuadBuffers,
) {
    let Some(frame) = r.frame else { return };
    if r.points.len() < 2 {
        return;
    }
    // Arc-length table.
    let mut cumulative = vec![0.0f32];
    for pair in r.points.windows(2) {
        cumulative.push(cumulative.last().unwrap() + pair[0].distance(pair[1]));
    }
    let total = *cumulative.last().unwrap();
    if total < 1e-5 {
        return;
    }
    let m = (def.segment_count.clamp(2, 128) + 1) as usize;
    let sample = |s: f32| -> Vec3 {
        let d = s * total;
        let i = cumulative.partition_point(|c| *c < d).clamp(1, cumulative.len() - 1);
        let (a, b) = (cumulative[i - 1], cumulative[i]);
        let t = if b > a { (d - a) / (b - a) } else { 0.0 };
        r.points[i - 1].lerp(r.points[i], t)
    };
    let time = r.time(def);
    let alpha_time = (curve(&def.alpha, time, 1.0) * r.mods.alpha).clamp(0.0, 1.0);
    let color_time = curve3(&def.color, time);
    let tiles = if def.tile_uv > 0 { def.tile_uv as f32 } else { 1.0 };
    let scroll = def.slip_uv_speed * r.age;
    let base = out.positions.len() as u32;
    for j in 0..m {
        let s = j as f32 / (m - 1) as f32;
        let p = sample(s);
        let ahead = sample((s + 0.5 / (m - 1) as f32).min(1.0));
        let behind = sample((s - 0.5 / (m - 1) as f32).max(0.0));
        let tangent = (ahead - behind).normalize_or_zero();
        let mut side = tangent.cross(view.forward).normalize_or_zero();
        if side == Vec3::ZERO {
            side = view.right;
        }
        let width = curve(&def.width, s, 1.0) * frame.scale * r.mods.size;
        let offset = curve(&def.offset, s, 0.0) * frame.scale;
        let centre = p + side * offset - origin;
        let color = (color_time * curve3(&def.length_color, s)).max(Vec3::ZERO) * r.mods.color;
        let alpha = (alpha_time * curve(&def.length_alpha, s, 1.0)).clamp(0.0, 1.0);
        let alpha = match backdrop {
            Some(dst) if look.blend == Blend::Alpha && !look.raw => gamma_fit_alpha(color, alpha, dst),
            _ => alpha,
        };
        let rgba = if look.raw {
            [color.x, color.y, color.z, alpha]
        } else {
            [srgb_to_linear(color.x), srgb_to_linear(color.y), srgb_to_linear(color.z), alpha]
        };
        // The installed ribbon textures run along U (the lightning bolt is a
        // horizontal zigzag) and across the width along V.
        let along = s * tiles + scroll;
        for (k, across) in [0.0f32, 1.0].into_iter().enumerate() {
            let sign = if k == 0 { -0.5 } else { 0.5 };
            out.positions.push((centre + side * width * sign).to_array());
            out.uv.push([along, across]);
            out.uv_b.push([0.0, 0.0]);
            out.colors.push(rgba);
        }
    }
    for j in 0..m as u32 - 1 {
        let a = base + j * 2;
        out.indices.extend([a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_ribbons(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut cache: ResMut<SwarmRenderCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SwarmMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    backdrop: Res<SwarmBlendBackdrop>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&mut Transform, &Mesh3d, &mut MeshMaterial3d<SwarmMaterial>, &mut Visibility), With<SwarmDraw>>,
) {
    let Some(originals) = originals else { return };
    let Some(camera) = camera.iter().next() else { return };
    let view = View {
        position: camera.translation(),
        right: camera.right().as_vec3(),
        up: camera.up().as_vec3(),
        forward: camera.forward().as_vec3(),
    };
    let hidden = editor.is_some_and(|e| e.open);
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_ribbon(&lib, &mut |ribbon, def| {
            let Some(look) = cache.ribbon_look(&lib, &originals, &mut images, &mut materials, ribbon.dir, ribbon.index, def)
            else {
                return;
            };
            let Some(frame) = ribbon.frame else { return };
            seen.insert(ribbon.uid);
            let origin = frame.origin;
            let mut quads = QuadBuffers::default();
            if !hidden {
                ribbon_quads(ribbon, def, &look, view, origin, backdrop.0, &mut quads);
            }
            let visible = !quads.indices.is_empty();
            if std::env::var_os("OPENSPORE_FX_DEBUG").is_some() {
                info!(
                    "ribbon {} points {} first {:?} last {:?} scale {:.3} age {:.2}/{:.2} verts {} blend {:?} visible {visible}",
                    ribbon.uid,
                    ribbon.points.len(),
                    ribbon.points.first(),
                    ribbon.points.last(),
                    frame.scale,
                    ribbon.age,
                    ribbon.life,
                    quads.positions.len(),
                    look.blend
                );
            }
            let layer = effect.layer;
            match cache.ribbons.get(&ribbon.uid).copied().and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d))) {
                Some((entity, drawn_layer, (mut transform, mesh, mut material, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        cache.ribbons.insert(ribbon.uid, (entity, layer));
                    }
                    transform.translation = origin;
                    if material.0 != look.material {
                        material.0 = look.material.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if visible {
                        if let Some(mut m) = meshes.get_mut(&mesh.0) {
                            write_mesh(&mut m, quads);
                        }
                    }
                }
                None => {
                    let mut mesh = empty_mesh();
                    if visible {
                        write_mesh(&mut mesh, quads);
                    }
                    let entity = commands
                        .spawn((
                            SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(look.material.clone()),
                            Transform::from_translation(origin),
                            if visible { Visibility::Visible } else { Visibility::Hidden },
                            NoFrustumCulling,
                            NotShadowCaster,
                            NotShadowReceiver,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    cache.ribbons.insert(ribbon.uid, (entity, layer));
                }
            }
        });
    }
    cache.ribbons.retain(|uid, (entity, _)| {
        if seen.contains(uid) {
            return true;
        }
        if let Ok(mut e) = commands.get_entity(*entity) {
            e.despawn();
        }
        false
    });
}

/// Full-screen quads for screen effects: mode 5 (background) is drawn just in
/// front of the far plane, the colour modes over everything.
#[allow(clippy::too_many_arguments)]
fn draw_screens(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut cache: ResMut<SwarmRenderCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SwarmMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    camera: Query<(&GlobalTransform, &Projection), (With<crate::GameCamera>, Without<SwarmDraw>)>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&mut Transform, &mut GlobalTransform, &Mesh3d, &mut MeshMaterial3d<SwarmMaterial>, &mut Visibility), With<SwarmDraw>>,
) {
    let Some(originals) = originals else { return };
    let Some((camera, projection)) = camera.iter().next() else { return };
    let (fov, aspect, near, far) = match projection {
        Projection::Perspective(p) => (p.fov, p.aspect_ratio, p.near, p.far),
        _ => (std::f32::consts::FRAC_PI_4, 1.4, 0.1, 1000.0),
    };
    let hidden = editor.is_some_and(|e| e.open);
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_screen(&lib, &mut |screen, def: &ScreenDef| {
            let background = def.mode == 5;
            if !(background || matches!(def.mode, 0 | 1 | 2 | 3)) {
                return; // filter chains, skybox, user modes: not drawn (yet)
            }
            seen.insert(screen.uid);
            let strength = screen.strength(def).clamp(0.0, 4.0) * screen.mods.alpha;
            let mut color = screen.color(def) * screen.mods.color;
            if let Some(param) = screen.color {
                color *= param;
            }
            let image = if def.texture.is_none() {
                cache.white(&mut images)
            } else {
                cache
                    .texture(&originals, &mut images, def.texture)
                    .unwrap_or_else(|| cache.white(&mut images))
            };
            let blend = if def.mode == 0 || def.mode == 3 {
                Blend::Additive
            } else {
                Blend::Alpha
            };
            let material = cache.material(
                &mut materials,
                &image,
                blend,
                if background { Depth::Test } else { Depth::Ignore },
                false,
                false,
            );
            // Quad in the camera's view plane at `distance`.
            let distance = if background { far * 0.95 } else { near * 4.0 };
            let half_h = (fov * 0.5).tan() * distance * 1.02;
            let half_w = half_h * aspect;
            let center = camera.translation() + camera.forward().as_vec3() * distance;
            let right = camera.right().as_vec3() * half_w;
            let up = camera.up().as_vec3() * half_h;
            let alpha = if def.mode == 2 { strength * 0.5 } else { strength };
            let rgba = [srgb_to_linear(color.x), srgb_to_linear(color.y), srgb_to_linear(color.z), alpha.min(1.0)];
            let quads = QuadBuffers {
                positions: vec![(-right + up).to_array(), (right + up).to_array(), (-right - up).to_array(), (right - up).to_array()],
                uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
                uv_b: vec![[0.0, 0.0]; 4],
                colors: vec![rgba; 4],
                indices: vec![0, 2, 1, 1, 2, 3],
            };
            let visible = !hidden && alpha > 0.001;
            let layer = effect.layer;
            match cache.screens.get(&screen.uid).copied().and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d))) {
                Some((entity, drawn_layer, (mut transform, mut global, mesh, mut mat, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        cache.screens.insert(screen.uid, (entity, layer));
                    }
                    transform.translation = center;
                    // This runs after transform propagation: write the global
                    // transform too, or the quad trails the camera by a frame
                    // (camera motion / shake uncovered a strip of the screen
                    // under full-screen overlays such as the hatch fade).
                    *global = GlobalTransform::from_translation(center);
                    if mat.0 != material {
                        mat.0 = material.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if let Some(mut m) = meshes.get_mut(&mesh.0) {
                        write_mesh(&mut m, quads);
                    }
                }
                None => {
                    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                    write_mesh(&mut mesh, quads);
                    let entity = commands
                        .spawn((
                            SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation(center),
                            GlobalTransform::from_translation(center),
                            if visible { Visibility::Visible } else { Visibility::Hidden },
                            NoFrustumCulling,
                            NotShadowCaster,
                            NotShadowReceiver,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    cache.screens.insert(screen.uid, (entity, layer));
                }
            }
        });
    }
    cache.screens.retain(|uid, (entity, _)| {
        if seen.contains(uid) {
            return true;
        }
        if let Ok(mut e) = commands.get_entity(*entity) {
            e.despawn();
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Over black the fitted vertex alpha equals the plain one: the shader's
    /// `alpha^2.2` already reproduces a gamma-space blend exactly there.
    #[test]
    fn gamma_fit_matches_the_plain_exponent_over_black() {
        for a in [0.1f32, 0.3, 0.5, 0.8, 1.0] {
            let fitted = gamma_fit_alpha(Vec3::ONE, a, Vec3::ZERO);
            assert!((fitted - a).abs() < 0.03, "alpha {a}: {fitted}");
        }
    }

    /// A sprite the colour of the backdrop is invisible either way, and the
    /// fit never exceeds the vertex alpha range.
    #[test]
    fn gamma_fit_is_neutral_without_contrast_and_bounded() {
        let water = Vec3::new(0.2, 0.3, 0.6);
        assert_eq!(gamma_fit_alpha(water, 0.4, water), 0.4);
        for a in [0.0f32, 0.25, 0.5, 1.0] {
            let v = gamma_fit_alpha(Vec3::new(0.0, 0.5, 0.5), a, water);
            assert!((0.0..=1.0).contains(&v));
        }
        assert!(gamma_fit_alpha(Vec3::ONE, 0.5, water) < gamma_fit_alpha(Vec3::ONE, 0.9, water));
    }

    /// Light sprites over mid-tone water need more linear alpha than over
    /// black (the same gamma-space alpha lifts a dark pixel less in linear
    /// space), which is what the plain exponent under-delivers.
    #[test]
    fn gamma_fit_lifts_light_sprites_over_water() {
        let water = Vec3::new(0.2, 0.3, 0.6);
        let plain = 0.5f32.powf(GAMMA);
        let fitted = gamma_fit_alpha(Vec3::ONE, 0.5, water).powf(GAMMA);
        assert!(fitted > plain, "{fitted} vs {plain}");
    }

    #[test]
    fn material_links_map_ids_to_their_first_state() {
        // Version 0: id, state count, texture count, textures (8 bytes each).
        let mut t = vec![0, 0, 0, 0];
        t.extend_from_slice(&0x1754_507Eu32.to_be_bytes());
        t.extend_from_slice(&[1, 1]);
        t.extend_from_slice(&[0; 8]);
        t.extend_from_slice(&0x628D_6012u32.to_be_bytes());
        t.extend_from_slice(&[2, 0]);
        t.extend_from_slice(&0x0000_0007u32.to_be_bytes());
        t.extend_from_slice(&[1, 0]);
        t.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(material_links(&t), vec![(0x1754_507E, 0), (0x628D_6012, 1), (7, 3)]);
        assert!(material_links(&[0, 0]).is_empty());
    }

    #[test]
    fn additive_materials_add() {
        assert_eq!(material_blend(crate::world_objects::name_id("genericAdditiveParticle_material")), Blend::Additive);
        assert_eq!(material_blend(crate::world_objects::name_id("effectAnimAdditiveMaterial")), Blend::Additive);
        assert_eq!(material_blend(crate::world_objects::name_id("effectAnimMaterial")), Blend::Alpha);
    }

    /// `cargo test survey_material_shaders -- --ignored --nocapture`: every
    /// effect material shader of the installed library with its use count
    /// (resolve the names with `reg_file.txt`).
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_material_shaders() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        let mut counts: std::collections::BTreeMap<u32, (usize, Option<ResourceId>)> = Default::default();
        for dir in &lib.dirs {
            for m in dir.materials.values() {
                let e = counts.entry(m.shader.instance).or_insert((0, None));
                e.0 += 1;
                e.1 = e.1.or(m.first_texture());
            }
        }
        for (shader, (n, tex)) in counts {
            println!("material shader {shader:08x} x{n} blend {:?} e.g. texture {:?}", material_blend(shader), tex);
        }
    }

    /// `cargo test survey_material_blend_states -- --ignored --nocapture`:
    /// the compiled render states (D3DRS_ALPHABLENDENABLE 27, SRCBLEND 19,
    /// DESTBLEND 20) of every effect material shader, from the material link
    /// tables 0x40212000!N and the compiled states 0x40212001!N (RW4).
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_material_blend_states() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        let used: std::collections::BTreeSet<u32> =
            lib.dirs.iter().flat_map(|d| d.materials.values().map(|m| m.shader.instance)).collect();
        for tier in 0..4u32 {
            let key = |group, type_id| crate::dbpf::ResourceKey { type_id, group_id: group, instance_id: tier };
            let (Ok(links), Ok(rw)) = (index.read(key(0x4021_2000, 0x0469_A3F7)), index.read(key(0x4021_2001, crate::asset_catalog::RW4)))
            else {
                continue;
            };
            let sections = crate::rw4::sections(&rw).unwrap();
            let states: Vec<&[u8]> =
                sections.iter().filter(|s| s.0 == crate::rw4::T_COMPILED_STATE).map(|s| s.1).collect();
            // MaterialStateLinks (SporeModder-FX): version, then id, count / textures ... until -1.
            let be = |at: usize| u32::from_be_bytes(links[at..at + 4].try_into().unwrap());
            let le = |at: usize| u32::from_le_bytes(links[at..at + 4].try_into().unwrap());
            let big = be(0) <= 1;
            let word = |at: usize| if big { be(at) } else { le(at) };
            let version = word(0);
            let mut at = 4;
            let mut state_index = 0usize;
            println!("tier {tier}: links version {version} ({} bytes, big {big}), {} states", links.len(), states.len());
            while at + 4 <= links.len() {
                let id = word(at);
                at += 4;
                if id == u32::MAX {
                    break;
                }
                let n_states: usize;
                if version == 0 {
                    n_states = links[at] as usize;
                    let textures = links[at + 1] as usize;
                    at += 2 + textures * 8;
                } else {
                    let textures = if big {
                        u16::from_be_bytes([links[at], links[at + 1]])
                    } else {
                        u16::from_le_bytes([links[at], links[at + 1]])
                    } as usize;
                    at += 2 + textures * 12 + 32;
                    n_states = links[at..at + 16].iter().filter(|b| **b != 0).count();
                    at += 16;
                }
                if used.contains(&id) {
                    for k in 0..n_states {
                        let Some(bytes) = states.get(state_index + k) else { continue };
                        match crate::material_state::parse(bytes) {
                            Ok(st) => {
                                let v = |rs: u32| {
                                    st.render_states
                                        .iter()
                                        .filter(|(g, _)| *g == 0)
                                        .flat_map(|(_, p)| p)
                                        .rev()
                                        .find(|(key, _)| *key == rs)
                                        .map(|(_, v)| *v)
                                };
                                println!(
                                    "  material {id:08x} state {k}: renderer {:08x} blend {:?} src {:?} dst {:?} zwrite {:?}",
                                    st.renderer_id,
                                    v(27),
                                    v(19),
                                    v(20),
                                    v(14)
                                );
                            }
                            Err(e) => println!("  material {id:08x} state {k}: {e}"),
                        }
                    }
                }
                state_index += n_states;
            }
        }
    }
}
