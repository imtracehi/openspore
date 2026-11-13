//! Swarm screen filter chains (`ScreenEffect` mode 10, SporeModder-FX
//! `filterChain`): main-world side. The GPU passes live in `filters_gpu.rs`.
//!
//! A chain is a list of filters (copy, add, colorize, blur, distort, multiply,
//! custom material...) that read and write textures named by small numbers:
//! 0 `source` (the frame so far), 1 `dest`, 2.. the particle buffers (instance
//! `n` is buffer `n - 1`: particles that name `buffer b` in their texture slot
//! draw there instead of into the frame; the Cell chains read buffer 2 =
//! distortion normals and buffer 3 = the hero blur mask), 0x20|i the chain's
//! own temporary textures (`filter_buffers`, 1/ratio of the screen), anything
//! else an installed texture.
//!
//! NATIVE: the filter list, its parameters and the shader maths of every
//! filter kind the chains use (the game's own `Filter*Shader` pixel shaders,
//! disassembled; see the shader), the buffer numbering (verified on the Cell
//! chains and the explosion chain: distorter instance 4 = buffer 3 for the
//! nuke shockwave), the temporary texture ratios.
//! ADAPTED: buffer 1 (instance 2: no installed effect draws into it) starts
//! each frame as a copy of `source`, so the Cell chain acts on the camera pass
//! it runs on (the background pass for `cell_dof_distort`, see cell_fx.rs);
//! the temporary texture the first filter blends a missing texture into starts
//! as the neutral normal (0.5 grey: no distortion); filter kinds without a
//! shader here (edge, dilate, contrast, strength fader, unknown custom
//! materials) pass the source through.
//!
//! The Cell History's backdrop is the second Cell user: its screen effect `modalUIBackgroundBlur` (the Cell
//! screen, 7 filters, custom material `CellMinspecCompositeMaterial`) runs on the GameCamera (`layer` 0) over the
//! paused world and reads the same scratch buffer 1 (so the same ADAPTED start as above).
//!
//! Particle buffers are drawn by one extra camera per buffer (render layer
//! `BUFFER_LAYER + b`, half resolution, transparent clear), created while a
//! running chain reads the buffer.

use std::collections::{HashMap, HashSet};

use bevy::{
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        extract_component::ExtractComponent,
        extract_resource::ExtractResource,
        render_resource::{Extent3d, TextureFormat},
        RenderApp,
    },
};

use super::format::{ResourceId, ScreenDef};
use super::render::SwarmRenderCache;
use super::{SwarmEffect, SwarmLibrary};

/// Chains with `SwarmEffect::layer == 0` run on the GameCamera (the whole
/// frame), chains on [`VIEW_BACKGROUND`] on the Cell background pass.
pub const VIEW_GAME: u8 = 0;
pub const VIEW_BACKGROUND: u8 = 1;
/// Render layer of particle buffer `b` is `BUFFER_LAYER + b`.
pub const BUFFER_LAYER: usize = 8;

/// Marks a camera that chains run on; the value is the chain `view`.
#[derive(Component, Clone, Copy, Debug, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct SwarmFilterView(pub u8);

/// Marks the camera that draws particle buffer `b`.
#[derive(Component)]
pub struct SwarmBufferCamera(#[allow(dead_code)] pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    None,
    Source,
    Dest,
    Buffer(u8),
    Temp(u8),
    Image(u8),
}

/// One filter pass. `kind` is the shader branch (the effect format's filter
/// numbers for the kinds that exist: 0 copy, 1 compress, 2 add, 3 colorize,
/// 6 blur1D, 10 distort, 13 multiply, 16 DOF leak composite, 255 pass).
#[derive(Clone, Debug)]
pub struct FilterOp {
    pub kind: u32,
    /// 0 replace, 1 alpha blend, 2 add, 3 multiply onto the destination.
    pub blend: u8,
    /// Shader FLAG_ALPHA 16, FLAG_MASK 32, FLAG_REPLACE 64, FLAG_INVERT 128.
    pub flags: u32,
    pub a: Slot,
    pub b: Slot,
    pub c: Slot,
    pub dst: Slot,
    pub p: [[f32; 4]; 3],
}

/// A compiled chain instance, extracted to the render world each frame.
#[derive(Clone, Debug)]
pub struct ChainRun {
    /// Persistent id (temporary textures live as long as the chain does).
    pub uid: u64,
    pub view: u8,
    /// Screen ratio of each temporary texture.
    pub temp_ratio: Vec<u32>,
    /// Temporary textures that start as the neutral normal.
    pub neutral: Vec<u8>,
    pub images: Vec<Handle<Image>>,
    pub ops: Vec<FilterOp>,
    pub buffers: Vec<u8>,
}

/// Everything the render world needs this frame.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub struct SwarmFilterFrame {
    pub chains: Vec<ChainRun>,
    /// Image each camera-drawn particle buffer renders into.
    pub buffers: Vec<(u8, Handle<Image>)>,
}

/// The effects renderer's global colour: `cCellGFX` sends the background
/// colour (`CellBackgrounds` at the world scale) to the skybox effect
/// `cell_background_color` (parameter 5) and, in the same routine (0xE55080),
/// to a method of the global effects renderer (vtable +0xC4, taking the three
/// floats). The filter chains' `colorize ... overrideColor` takes its colour
/// from there (the `cell_dof_distort` colorize carries a dummy red colour and
/// the `overrideColor` flag). ADAPTED: the receiving renderer method itself is
/// not decoded (the colour and where it ends up are inferred from the retail
/// frames: the blurred background is tinted toward the water colour). `None`
/// outside the Cell stage: the override colorize then does nothing.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SwarmOverrideColor(pub Option<Vec3>);

/// The colour alpha-blended sprites are fitted against (the Cell stage sets
/// the water colour). Spore blends in gamma space (a UNORM back buffer), Bevy
/// in linear space over an sRGB target, so the same alpha gives a different
/// result; with a backdrop the sprite alpha is replaced by the linear alpha
/// that reproduces the gamma-space blend over that colour (least squares over
/// the channels, `render.rs::gamma_fit_alpha`). `None` keeps the generic
/// `alpha^2.2` approximation (exact over black).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SwarmBlendBackdrop(pub Option<Vec3>);

/// `FilterOp::flags` bit: the op's colour is [`SwarmOverrideColor`] (the main
/// world patches `p[0].rgb` every frame; the shader never sees this bit).
pub const FLAG_OVERRIDE_COLOR: u32 = 256;

/// Compiled chains by effect component (directory, screen index).
#[derive(Resource, Default)]
pub struct SwarmChainCache {
    compiled: HashMap<(u16, u32), Option<ChainRun>>,
    reported: HashSet<String>,
}

/// Particle buffer cameras and images, and which buffers a chain reads.
#[derive(Resource, Default)]
pub struct SwarmFilterBuffers {
    images: HashMap<u8, Handle<Image>>,
    cameras: HashMap<u8, Entity>,
    pub needed: HashSet<u8>,
    size: UVec2,
}

impl SwarmFilterBuffers {
    pub fn wants(&self, buffer: u8) -> bool {
        self.needed.contains(&buffer)
    }
}

/// `OS_FILTERS=0` turns every chain off.
pub fn filters_enabled() -> bool {
    crate::world_presentation::tune("FILTERS", 1.0) > 0.5
}

/// Decoded `parameters` bytes of one filter: indices into the screen's
/// parameter lists, 255 = unset.
struct Args<'a> {
    def: &'a ScreenDef,
    bytes: &'a [u8],
}

impl Args<'_> {
    fn index(&self, i: usize) -> Option<usize> {
        self.bytes.get(i).copied().filter(|b| *b != 255).map(|b| b as usize)
    }
    fn float(&self, i: usize) -> Option<f32> {
        self.def.params_float.get(self.index(i)?).copied()
    }
    fn vec3(&self, i: usize) -> Option<[f32; 3]> {
        self.def.params_vec3.get(self.index(i)?).copied()
    }
    fn vec2(&self, i: usize) -> Option<[f32; 2]> {
        self.def.params_vec2.get(self.index(i)?).copied()
    }
    fn resource(&self, i: usize) -> Option<ResourceId> {
        self.def.params_resource.get(self.index(i)?).copied()
    }
    fn flag(&self, i: usize) -> bool {
        self.bytes.get(i).is_some_and(|b| *b != 0 && *b != 255)
    }
}

/// `DOFBlurLeakMaterial` (a `customMaterial` of the Cell chains): the
/// `DOFBlurLeakShader` composite.
const MATERIAL_DOF_BLUR_LEAK: u32 = 0x3F0F_7959;
/// `CellMinspecCompositeMaterial` (a `customMaterial` of `modalUIBackgroundBlur`'s
/// Cell chain, the screen effect behind the History window): the compiled shader pack's
/// `CellMinspecCompositeShader` (`Spore_Graphics` pack 3, id 0x7000007F), whose pixel
/// shader is `sampler0.rgb * sampler0.a + sampler1.rgb * (1 - sampler0.a)` (both read
/// at the same uv, alpha = the vertex colour's alpha): the same colour maths as
/// `DOFBlurLeakShader`, so it runs as the same filter kind. The material table
/// (`Spore_Graphics` 0x40212000) lists both materials with the same first sampler
/// parameter; the material -> shader link itself is by name (ADAPTED: not read from the
/// table, whose layout is not decoded) and the vertex colour alpha is taken as 1.
const MATERIAL_CELL_MINSPEC_COMPOSITE: u32 = 0x2C81_5C80;

struct Compiler<'a, 'b> {
    def: &'a ScreenDef,
    originals: Option<&'b crate::Originals>,
    cache: &'b mut SwarmRenderCache,
    textures: &'b mut Assets<Image>,
    images: Vec<Handle<Image>>,
    image_index: HashMap<ResourceId, Option<u8>>,
    neutral: Vec<u8>,
    ops: Vec<FilterOp>,
    buffers: HashSet<u8>,
    notes: Vec<String>,
}

impl Compiler<'_, '_> {
    /// Slot named by a resource instance (sources and parameters).
    fn slot(&mut self, id: ResourceId) -> Slot {
        match id.instance {
            0 => Slot::Source,
            1 => Slot::Dest,
            n @ 2..=0x1F => {
                self.buffers.insert((n - 1) as u8);
                Slot::Buffer((n - 1) as u8)
            }
            n @ 0x20..=0x3F => Slot::Temp((n & 0x1F) as u8),
            _ => {
                if id.is_none() {
                    return Slot::None;
                }
                let found = match self.image_index.get(&id) {
                    Some(found) => *found,
                    None => {
                        let found = self
                            .originals
                            .and_then(|originals| self.cache.texture(originals, self.textures, id))
                            .map(|handle| {
                                self.images.push(handle);
                                (self.images.len() - 1) as u8
                            });
                        self.image_index.insert(id, found);
                        found
                    }
                };
                found.map_or(Slot::None, Slot::Image)
            }
        }
    }

    fn destination(&mut self, byte: u8) -> Slot {
        self.slot(ResourceId {
            group: 0,
            instance: byte as u32,
        })
    }

    fn push(&mut self, op: FilterOp) {
        self.ops.push(op);
    }

    fn compile(&mut self) {
        let def = self.def;
        for (n, f) in def.filters.iter().enumerate() {
            let args = Args {
                def,
                bytes: &f.parameters,
            };
            let src = self.slot(f.source);
            let dst = self.destination(f.destination);
            let mut op = FilterOp {
                kind: 255,
                blend: 0,
                flags: 0,
                a: src,
                b: Slot::None,
                c: Slot::None,
                dst,
                p: [[0.0; 4]; 3],
            };
            match f.kind {
                // copy
                0 => {
                    if src == Slot::None {
                        // A missing texture: nothing to copy. The temporary it
                        // would have faded in starts neutral (ADAPTED).
                        if let Slot::Temp(t) = dst {
                            self.neutral.push(t);
                        }
                        continue;
                    }
                    op.kind = 0;
                    op.blend = if args.flag(0) {
                        1
                    } else if args.flag(1) {
                        2
                    } else if args.flag(2) {
                        3
                    } else {
                        0
                    };
                    let color = args.vec3(5).unwrap_or([1.0; 3]);
                    let alpha = args.float(4);
                    if alpha.is_some() {
                        op.flags |= 16;
                    }
                    op.p[0] = [color[0], color[1], color[2], alpha.unwrap_or(1.0)];
                    if let Some(mask) = args.resource(6) {
                        op.b = self.slot(mask);
                        if op.b != Slot::None {
                            op.flags |= 32;
                        }
                    }
                    if args.flag(9) {
                        op.flags |= 128;
                    }
                    op.p[1][0] = args.float(7).unwrap_or(0.0);
                }
                // compress
                1 => {
                    op.kind = 1;
                    op.p[0] = [
                        args.float(0).unwrap_or(0.0),
                        args.float(1).unwrap_or(1.0),
                        if args.flag(2) { 1.0 } else { 0.0 },
                        0.0,
                    ];
                }
                // add
                2 => {
                    op.kind = 2;
                    if let Some(texture) = args.resource(0) {
                        op.b = self.slot(texture);
                    }
                    if op.b == Slot::None {
                        op.kind = 255;
                    }
                    if args.flag(8) {
                        op.flags |= 64;
                    }
                    if let Some(mask) = args.resource(6) {
                        op.c = self.slot(mask);
                        if op.c != Slot::None {
                            op.flags |= 32;
                        }
                    }
                    op.p[0] = [
                        args.float(3).unwrap_or(1.0),
                        args.float(4).unwrap_or(1.0),
                        args.float(7).unwrap_or(0.0),
                        0.0,
                    ];
                    let tile = args.vec2(5).unwrap_or([1.0, 1.0]);
                    op.p[1] = [tile[0], tile[1], 0.0, 0.0];
                }
                // colorize
                3 => {
                    op.kind = 3;
                    // `overrideColor`: the colour comes from the effects
                    // renderer's global colour ([`SwarmOverrideColor`], patched
                    // in per frame); the authored `color` is a dummy then.
                    let color = args.vec3(0).unwrap_or([1.0; 3]);
                    let strength = args.float(1).unwrap_or(1.0);
                    op.p[0] = [color[0], color[1], color[2], strength];
                    if args.flag(3) {
                        op.flags |= FLAG_OVERRIDE_COLOR;
                    }
                }
                // blurX / blurY / blur1D
                4..=6 => {
                    op.kind = 6;
                    let scale = args.float(0);
                    let (x, y) = match f.kind {
                        4 => (scale.unwrap_or(1.0), 0.0),
                        5 => (0.0, scale.unwrap_or(1.0)),
                        _ => (
                            args.float(1).or(scale).unwrap_or(1.0),
                            args.float(2).or(scale).unwrap_or(1.0),
                        ),
                    };
                    op.p[0] = [x, y, 0.0, 0.0];
                }
                // distort
                10 => {
                    op.kind = 10;
                    if let Some(distorter) = args.resource(0) {
                        op.b = self.slot(distorter);
                    }
                    if op.b == Slot::None {
                        op.kind = 255;
                    }
                    let tile = args.vec2(5).unwrap_or([1.0, 1.0]);
                    op.p[0][0] = args.float(3).unwrap_or(1.0);
                    op.p[1] = [tile[0], tile[1], args.float(1).unwrap_or(0.0), args.float(2).unwrap_or(0.0)];
                }
                // multiply
                13 => {
                    op.kind = 13;
                    if let Some(texture) = args.resource(0) {
                        op.b = self.slot(texture);
                    }
                    if op.b == Slot::None {
                        op.kind = 255;
                    }
                    op.p[0] = [
                        args.float(1).unwrap_or(1.0),
                        args.float(2).unwrap_or(1.0),
                        args.float(7).unwrap_or(0.0),
                        args.float(8).unwrap_or(0.0),
                    ];
                    let tile = args.vec2(3).unwrap_or([1.0, 1.0]);
                    op.p[1] = [tile[0], tile[1], 0.0, 0.0];
                }
                // custom material
                16 => {
                    let material = args.resource(0).map(|r| r.instance);
                    if material == Some(MATERIAL_DOF_BLUR_LEAK) || material == Some(MATERIAL_CELL_MINSPEC_COMPOSITE) {
                        op.kind = 16;
                        let sampler0 = args.resource(1).map_or(Slot::None, |r| self.slot(r));
                        op.a = sampler0;
                        op.b = src;
                        if sampler0 == Slot::None {
                            op.kind = 255;
                            op.a = src;
                        }
                    } else {
                        self.notes
                            .push(format!("custom material {material:08x?} (filter {n}) passes the source through"));
                    }
                }
                other => {
                    self.notes.push(format!("filter kind {other} (filter {n}) passes the source through"));
                }
            }
            if op.a == Slot::None {
                continue;
            }
            self.push(op);
        }
    }
}

fn compile_chain(
    def: &ScreenDef,
    originals: Option<&crate::Originals>,
    cache: &mut SwarmRenderCache,
    textures: &mut Assets<Image>,
    notes: &mut Vec<String>,
) -> ChainRun {
    let mut compiler = Compiler {
        def,
        originals,
        cache,
        textures,
        images: vec![],
        image_index: HashMap::new(),
        neutral: vec![],
        ops: vec![],
        buffers: HashSet::new(),
        notes: vec![],
    };
    compiler.compile();
    notes.append(&mut compiler.notes);
    let mut buffers: Vec<u8> = compiler.buffers.into_iter().collect();
    buffers.sort_unstable();
    ChainRun {
        uid: 0,
        view: VIEW_GAME,
        temp_ratio: def.filter_buffers.iter().map(|(ratio, _)| (*ratio).max(1) as u32).collect(),
        neutral: compiler.neutral,
        images: compiler.images,
        ops: compiler.ops,
        buffers,
    }
}

/// Frame size of the GameCamera in pixels.
fn camera_size(camera: &Camera) -> UVec2 {
    camera.physical_target_size().unwrap_or(UVec2::new(1280, 720)).max(UVec2::ONE)
}

fn buffer_image(size: UVec2) -> Image {
    let half = (size / 2).max(UVec2::ONE);
    // Raw (non-sRGB) storage: normal maps and masks keep their stored bytes.
    Image::new_target_texture(half.x, half.y, TextureFormat::Rgba8Unorm, None)
}

/// Keeps `SwarmFilterView` on the GameCamera (the background camera gets its
/// own marker from the Cell code).
pub fn mark_game_cameras(
    mut commands: Commands,
    cameras: Query<Entity, (With<crate::GameCamera>, Without<SwarmFilterView>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(SwarmFilterView(VIEW_GAME));
    }
}

/// Collect the running chains of this frame, and create/update the particle
/// buffer cameras they need.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn collect_filter_chains(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut render_cache: ResMut<SwarmRenderCache>,
    mut textures: ResMut<Assets<Image>>,
    mut chains: ResMut<SwarmChainCache>,
    mut frame: ResMut<SwarmFilterFrame>,
    mut buffers: ResMut<SwarmFilterBuffers>,
    override_color: Res<SwarmOverrideColor>,
    editor: Option<Res<crate::editor::EditorState>>,
    effects: Query<&SwarmEffect>,
    main: Query<(&Camera, &GlobalTransform, &Projection), (With<crate::GameCamera>, Without<SwarmBufferCamera>)>,
    mut buffer_cameras: Query<
        (&mut Camera, &mut Transform, &mut GlobalTransform, &mut Projection),
        (With<SwarmBufferCamera>, Without<crate::GameCamera>),
    >,
) {
    frame.chains.clear();
    frame.buffers.clear();
    let mut needed: HashSet<u8> = HashSet::new();
    let hidden = editor.is_some_and(|e| e.open);
    if let (Some(originals), true, false) = (originals.as_deref(), filters_enabled(), hidden) {
        for effect in &effects {
            let Some(instance) = &effect.instance else { continue };
            let view = if effect.layer == 0 { VIEW_GAME } else { VIEW_BACKGROUND };
            instance.for_each_screen(&lib, &mut |screen, def: &ScreenDef| {
                if def.mode != 10 || screen.stopped {
                    return;
                }
                if screen.strength(def).clamp(0.0, 4.0) * screen.mods.alpha <= 0.001 {
                    return;
                }
                let key = (screen.dir, screen.index);
                if !chains.compiled.contains_key(&key) {
                    let mut notes = vec![];
                    let chain = compile_chain(def, Some(originals), &mut render_cache, &mut textures, &mut notes);
                    // ADAPTED: a chain with a custom material this renderer cannot run (the
                    // bloom bright-pass 0x32AB8E92 of the UFO beam glow, `space-tools-fx`) is
                    // not drawn: passing its source through blurs the whole frame and adds
                    // it back, which whites the screen out.
                    let unsupported = notes.iter().any(|n| n.starts_with("custom material"));
                    for note in notes {
                        if chains.reported.insert(note.clone()) {
                            warn!("Swarm filter chain: {note}");
                        }
                    }
                    chains.compiled.insert(key, if unsupported { None } else { Some(chain) });
                }
                let Some(Some(template)) = chains.compiled.get(&key) else { return };
                let mut run = template.clone();
                run.uid = screen.uid;
                run.view = view;
                for op in &mut run.ops {
                    if op.kind == 3 && op.flags & FLAG_OVERRIDE_COLOR != 0 {
                        match override_color.0 {
                            Some(c) => op.p[0] = [c.x, c.y, c.z, op.p[0][3]],
                            None => op.p[0][3] = 0.0,
                        }
                    }
                }
                needed.extend(run.buffers.iter().copied().filter(|b| *b >= 2));
                frame.chains.push(run);
            });
        }
    }
    // Buffer cameras follow the GameCamera's view.
    let Ok((camera, global, projection)) = main.single() else {
        buffers.needed = needed;
        return;
    };
    let size = camera_size(camera);
    for &b in &needed {
        let handle = match buffers.images.get(&b) {
            Some(handle) => handle.clone(),
            None => {
                let handle = textures.add(buffer_image(size));
                buffers.images.insert(b, handle.clone());
                handle
            }
        };
        frame.buffers.push((b, handle.clone()));
        match buffers.cameras.get(&b).copied().and_then(|e| buffer_cameras.get_mut(e).ok()) {
            Some((mut cam, mut transform, mut buffer_global, mut buffer_projection)) => {
                if !cam.is_active {
                    cam.is_active = true;
                }
                if *buffer_global != *global {
                    *buffer_global = *global;
                    *transform = global.compute_transform();
                }
                let same = match (&*buffer_projection, projection) {
                    (Projection::Perspective(a), Projection::Perspective(c)) => {
                        a.fov == c.fov && a.near == c.near && a.far == c.far && a.aspect_ratio == c.aspect_ratio
                    }
                    _ => false,
                };
                if !same {
                    *buffer_projection = projection.clone();
                }
            }
            None => {
                let entity = commands
                    .spawn((
                        SwarmBufferCamera(b),
                        Name::new(format!("swarm-buffer-camera-{b}")),
                        Camera3d::default(),
                        Camera {
                            order: -20 + b as isize,
                            clear_color: ClearColorConfig::Custom(Color::NONE),
                            ..default()
                        },
                        RenderTarget::from(handle.clone()),
                        projection.clone(),
                        global.compute_transform(),
                        *global,
                        RenderLayers::layer(BUFFER_LAYER + b as usize),
                        Tonemapping::None,
                        DebandDither::Disabled,
                        Msaa::Off,
                    ))
                    .id();
                buffers.cameras.insert(b, entity);
            }
        }
    }
    // Resize every buffer image with the frame; park cameras nobody reads.
    if buffers.size != size {
        buffers.size = size;
        for handle in buffers.images.values() {
            if let Some(mut image) = textures.get_mut(handle) {
                let half = (size / 2).max(UVec2::ONE);
                image.resize(Extent3d {
                    width: half.x,
                    height: half.y,
                    depth_or_array_layers: 1,
                });
            }
        }
    }
    let unused: Vec<u8> = buffers.cameras.keys().copied().filter(|b| !needed.contains(b)).collect();
    for b in unused {
        if let Some(entity) = buffers.cameras.get(&b).copied() {
            if let Ok((mut cam, ..)) = buffer_cameras.get_mut(entity) {
                if cam.is_active {
                    cam.is_active = false;
                }
            }
        }
    }
    buffers.needed = needed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm_fx::format::{kind, ScreenFilterDef};

    fn chain(def: &ScreenDef) -> ChainRun {
        let mut notes = vec![];
        compile_chain(
            def,
            None,
            &mut SwarmRenderCache::default(),
            &mut Assets::<Image>::default(),
            &mut notes,
        )
    }

    #[test]
    fn parameter_bytes_index_the_effect_lists() {
        let def = ScreenDef {
            params_float: vec![0.25, 0.75],
            params_vec3: vec![[1.0, 0.0, 0.5]],
            params_vec2: vec![[2.0, 3.0]],
            params_resource: vec![ResourceId { group: 0, instance: 4 }],
            ..Default::default()
        };
        let bytes = [1u8, 255, 0, 0, 1, 7];
        let args = Args { def: &def, bytes: &bytes };
        assert_eq!(args.float(0), Some(0.75));
        assert_eq!(args.float(1), None);
        assert_eq!(args.vec3(2), Some([1.0, 0.0, 0.5]));
        assert_eq!(args.vec2(3), Some([2.0, 3.0]));
        assert_eq!(args.resource(3).map(|r| r.instance), Some(4));
        assert!(args.flag(4) && args.flag(5) && !args.flag(1) && !args.flag(9));
    }

    /// The DoF chain's numbering: buffer n is instance n + 1, instance 2 is
    /// the scratch buffer 1, distortion is buffer 2, the hero blur mask 3.
    #[test]
    fn resource_numbers_map_to_slots() {
        let def = ScreenDef {
            mode: 10,
            filters: vec![
                // blur1D temp0 -> temp1 along (1, -1)
                ScreenFilterDef { kind: 6, destination: 33, source: ResourceId { group: 0, instance: 32 }, parameters: vec![255, 0, 1] },
                // multiply scratch buffer 1 by the blur mask (instance 4) -> dest
                ScreenFilterDef { kind: 13, destination: 1, source: ResourceId { group: 0, instance: 2 }, parameters: vec![0, 255, 255, 255, 255, 0, 255, 255, 255] },
            ],
            filter_buffers: vec![(2, 256), (4, 256)],
            params_float: vec![1.0, -1.0],
            params_resource: vec![ResourceId { group: 0, instance: 4 }],
            ..Default::default()
        };
        let run = chain(&def);
        assert_eq!(run.temp_ratio, vec![2, 4]);
        assert_eq!(run.ops.len(), 2);
        let blur = &run.ops[0];
        assert_eq!((blur.kind, blur.a, blur.dst), (6, Slot::Temp(0), Slot::Temp(1)));
        assert_eq!(blur.p[0][..2], [1.0, -1.0]);
        let mul = &run.ops[1];
        assert_eq!((mul.kind, mul.a, mul.b, mul.dst), (13, Slot::Buffer(1), Slot::Buffer(3), Slot::Dest));
        assert_eq!(run.buffers, vec![1, 3]);
    }

    #[test]
    fn missing_textures_skip_the_filter_and_start_the_temporary_neutral() {
        let def = ScreenDef {
            mode: 10,
            filters: vec![ScreenFilterDef {
                kind: 0,
                destination: 35,
                source: ResourceId { group: 0, instance: 219_913_896 },
                parameters: vec![1, 0, 0, 0, 0, 255, 255, 255, 255, 0, 255],
            }],
            filter_buffers: vec![(4, 256); 4],
            params_float: vec![0.05],
            ..Default::default()
        };
        let run = chain(&def);
        assert!(run.ops.is_empty());
        assert_eq!(run.neutral, vec![3]);
    }

    /// `colorize ... overrideColor` keeps its strength, is marked so the main
    /// world can patch the renderer's colour in, and a plain colorize is not.
    #[test]
    fn override_colorize_is_flagged() {
        let def = ScreenDef {
            mode: 10,
            filters: vec![
                ScreenFilterDef { kind: 3, destination: 34, source: ResourceId { group: 0, instance: 2 }, parameters: vec![0, 1, 255, 1] },
                ScreenFilterDef { kind: 3, destination: 33, source: ResourceId { group: 0, instance: 34 }, parameters: vec![0, 1, 255, 0] },
            ],
            filter_buffers: vec![(2, 256), (2, 256), (2, 256)],
            params_float: vec![0.05, 0.4],
            params_vec3: vec![[1.0, 0.0, 0.0]],
            ..Default::default()
        };
        let run = chain(&def);
        assert_eq!(run.ops.len(), 2);
        assert!(run.ops[0].flags & FLAG_OVERRIDE_COLOR != 0);
        assert_eq!(run.ops[0].p[0][3], 0.4);
        assert_eq!(run.ops[1].flags & FLAG_OVERRIDE_COLOR, 0);
        assert_eq!(run.ops[1].p[0], [1.0, 0.0, 0.0, 0.4]);
    }

    /// The Cell screen of `modalUIBackgroundBlur` (the History backdrop, decoded from the
    /// installed effects directory 0x4CF9B596, component 27): tint the frame toward the
    /// effects colour, composite it with `CellMinspecCompositeMaterial`, four diagonal
    /// blur passes between the two quarter-resolution temporaries, desaturate into dest.
    #[test]
    fn modal_ui_background_blur_cell_screen_compiles() {
        let id = |instance| ResourceId { group: 0, instance };
        let def = ScreenDef {
            mode: 10,
            filters: vec![
                ScreenFilterDef { kind: 3, destination: 32, source: id(2), parameters: vec![0, 0, 255, 1] },
                ScreenFilterDef {
                    kind: 16,
                    destination: 33,
                    source: id(0),
                    parameters: vec![0, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 255, 0, 0, 0],
                },
                ScreenFilterDef { kind: 6, destination: 32, source: id(33), parameters: vec![255, 1, 2] },
                ScreenFilterDef { kind: 6, destination: 33, source: id(32), parameters: vec![255, 3, 4] },
                ScreenFilterDef { kind: 6, destination: 32, source: id(33), parameters: vec![255, 5, 6] },
                ScreenFilterDef { kind: 6, destination: 33, source: id(32), parameters: vec![255, 7, 8] },
                ScreenFilterDef { kind: 3, destination: 1, source: id(33), parameters: vec![1, 9, 255, 0] },
            ],
            filter_buffers: vec![(4, 256), (4, 256)],
            params_float: vec![0.4, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, -1.0, 1.0, 0.4],
            params_vec3: vec![[1.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
            params_resource: vec![id(MATERIAL_CELL_MINSPEC_COMPOSITE), id(32)],
            ..Default::default()
        };
        let mut notes = vec![];
        let run = compile_chain(&def, None, &mut SwarmRenderCache::default(), &mut Assets::<Image>::default(), &mut notes);
        assert!(notes.is_empty(), "the custom material is known now: {notes:?}");
        assert_eq!(run.temp_ratio, vec![4, 4]);
        assert_eq!(run.ops.len(), 7);
        // 1: colorize the frame (scratch buffer 1) with the effects colour, strength 0.4.
        let tint = &run.ops[0];
        assert_eq!((tint.kind, tint.a, tint.dst), (3, Slot::Buffer(1), Slot::Temp(0)));
        assert!(tint.flags & FLAG_OVERRIDE_COLOR != 0);
        assert_eq!(tint.p[0][3], 0.4);
        // 2: the composite: sampler0 = temp 0 over the source.
        let composite = &run.ops[1];
        assert_eq!((composite.kind, composite.a, composite.b, composite.dst), (16, Slot::Temp(0), Slot::Source, Slot::Temp(1)));
        // 3 .. 6: the four diagonals, alternating between the temporaries.
        let diagonals = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)];
        for (i, (x, y)) in diagonals.into_iter().enumerate() {
            let blur = &run.ops[2 + i];
            let (from, to) = if i % 2 == 0 { (Slot::Temp(1), Slot::Temp(0)) } else { (Slot::Temp(0), Slot::Temp(1)) };
            assert_eq!((blur.kind, blur.a, blur.dst), (6, from, to), "pass {i}");
            assert_eq!(blur.p[0][..2], [x, y], "pass {i}");
        }
        // 7: white colorize at 0.4 (a 40 % desaturation) into the frame.
        let last = &run.ops[6];
        assert_eq!((last.kind, last.a, last.dst), (3, Slot::Temp(1), Slot::Dest));
        assert_eq!(last.flags & FLAG_OVERRIDE_COLOR, 0);
        assert_eq!(last.p[0], [1.0, 1.0, 1.0, 0.4]);
        assert_eq!(run.buffers, vec![1]);
    }

    /// The installed Cell chains compile to the shape the notes describe.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_dof_chains_compile() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        for (name, ops, buffers) in [("cell_dof_distort", 14, vec![1, 2, 3]), ("cell_dof_distort_blind", 10, vec![1, 2, 3])] {
            let effect = lib.find(crate::world_objects::name_id(name)).expect("installed effect");
            let dir = lib.dir(effect.dir);
            let block = &dir.effects[effect.index as usize].blocks[0];
            assert_eq!(block.kind, kind::SCREEN);
            let def = &dir.screens[block.index as usize];
            let run = chain(def);
            assert_eq!(run.buffers, buffers, "{name}");
            // The first filter copies a texture that is not installed: skipped.
            assert_eq!(run.ops.len(), ops - 1, "{name}");
            assert_eq!(run.neutral, vec![3], "{name}");
            let last = run.ops.last().unwrap();
            assert_eq!(last.dst, Slot::Dest, "{name}");
            assert!(run.ops.iter().any(|op| op.kind == 16 && op.a == Slot::Temp(1) && op.b == Slot::Source));
        }
    }

    /// The installed `modalUIBackgroundBlur` has one chain per app mode; the Cell's (app flag
    /// 0x80) is the 7-filter chain whose custom material is `CellMinspecCompositeMaterial`.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_modal_blur_cell_chain_compiles() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let effect = lib.find(crate::world_objects::name_id("modalUIBackgroundBlur")).expect("installed effect");
        let dir = lib.dir(effect.dir);
        let blocks = &dir.effects[effect.index as usize].blocks;
        let cell: Vec<_> = blocks.iter().filter(|b| b.kind == kind::SCREEN && b.app_flags_mask & 0x80 != 0).collect();
        assert_eq!(cell.len(), 1, "one screen of the Cell mode");
        let def = &dir.screens[cell[0].index as usize];
        assert_eq!((def.mode, def.filters.len()), (10, 7));
        let mut notes = vec![];
        let run = compile_chain(def, None, &mut SwarmRenderCache::default(), &mut Assets::<Image>::default(), &mut notes);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(run.ops.len(), 7);
        assert_eq!(run.ops[1].kind, 16);
        assert_eq!(run.ops[6].dst, Slot::Dest);
    }
}
