//! Swarm screen filter chains: render-world side. Prepares the temporary
//! textures and pipelines per view and runs the chain's passes between the
//! main pass and tone mapping (see `filters.rs`, `shaders/swarm_filter.wesl`).

use std::collections::{HashMap, HashSet};

use bevy::{
    core_pipeline::{
        tonemapping::tonemapping, Core3d, Core3dSystems, FullscreenShader,
    },
    prelude::*,
    render::{
        extract_component::ExtractComponentPlugin,
        extract_resource::ExtractResourcePlugin,
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer_sized},
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
            BlendFactor, BlendOperation, BlendState, BufferInitDescriptor, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, MipmapFilterMode,
            Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            SpecializedRenderPipeline, SpecializedRenderPipelines, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::ViewTarget,
        GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems,
    },
    shader::Shader,
};

use super::filters::{ChainRun, Slot, SwarmFilterFrame, SwarmFilterView};

const FLAG_A_SRGB: u32 = 1;
const FLAG_B_SRGB: u32 = 2;
const FLAG_C_SRGB: u32 = 4;
const FLAG_DST_SRGB: u32 = 8;
const PARAM_BYTES: usize = 80;

pub fn build(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<SwarmFilterView>::default(),
        ExtractResourcePlugin::<SwarmFilterFrame>::default(),
    ));
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_gpu_resource::<SpecializedRenderPipelines<FilterPipeline>>()
        .init_resource::<FilterTextures>()
        .add_systems(RenderStartup, init_filter_pipeline)
        .add_systems(Render, prepare_filters.in_set(RenderSystems::Prepare))
        .add_systems(
            Core3d,
            swarm_filters.in_set(Core3dSystems::PostProcess).before(tonemapping),
        );
}

#[derive(Resource)]
pub struct FilterPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
    /// Stands in for an input that is not available (transparent black).
    blank: TextureView,
    _blank_texture: Texture,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilterKey {
    format: TextureFormat,
    blend: u8,
}

fn init_filter_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "swarm filter layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer_sized(false, None),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        min_filter: FilterMode::Linear,
        mag_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Nearest,
        ..default()
    });
    let blank_texture = render_device.create_texture(&TextureDescriptor {
        label: Some("swarm filter blank"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let blank = blank_texture.create_view(&TextureViewDescriptor::default());
    commands.insert_resource(FilterPipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        shader: asset_server.load("embedded://openspore/shaders/swarm_filter.wesl"),
        blank,
        _blank_texture: blank_texture,
    });
}

impl SpecializedRenderPipeline for FilterPipeline {
    type Key = FilterKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let blend = match key.blend {
            1 => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::SrcAlpha,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            }),
            2 => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
            3 => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Zero,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
            _ => None,
        };
        RenderPipelineDescriptor {
            label: Some("swarm filter".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// Persistent temporary textures, by chain and slot.
struct Stored {
    size: UVec2,
    _texture: Texture,
    view: TextureView,
    cleared: bool,
}

#[derive(Resource, Default)]
pub struct FilterTextures {
    map: HashMap<(u64, u8, u8), Stored>,
}

#[derive(Clone)]
enum SlotView {
    Source,
    Dest,
    Tex { view: TextureView, srgb: bool, size: UVec2 },
}

enum Step {
    Clear {
        dst: usize,
        color: [f32; 4],
    },
    Draw {
        pipeline: CachedRenderPipelineId,
        inputs: [Option<usize>; 3],
        dst: usize,
        blend: u8,
        params: [u8; PARAM_BYTES],
    },
}

struct PreparedChain {
    views: Vec<SlotView>,
    steps: Vec<Step>,
}

#[derive(Component, Default)]
struct FilterPrepared {
    chains: Vec<PreparedChain>,
}

fn write_params(kind: u32, flags: u32, p: &[[f32; 4]; 3], texel: [f32; 4]) -> [u8; PARAM_BYTES] {
    let mut out = [0u8; PARAM_BYTES];
    let mut at = 0;
    let mut put = |bytes: [u8; 4]| {
        out[at..at + 4].copy_from_slice(&bytes);
        at += 4;
    };
    put(kind.to_le_bytes());
    put(flags.to_le_bytes());
    put(0u32.to_le_bytes());
    put(0u32.to_le_bytes());
    for row in p {
        for v in row {
            put(v.to_le_bytes());
        }
    }
    for v in texel {
        put(v.to_le_bytes());
    }
    out
}

/// Builds the pass list of one chain for one view.
struct ChainBuilder<'a> {
    chain: &'a ChainRun,
    frame: &'a SwarmFilterFrame,
    images: &'a RenderAssets<GpuImage>,
    device: &'a RenderDevice,
    store: &'a mut FilterTextures,
    used: &'a mut HashSet<(u64, u8, u8)>,
    size: UVec2,
    main_format: TextureFormat,
    views: Vec<SlotView>,
    steps: Vec<Step>,
    index: HashMap<Slot, Option<usize>>,
}

impl ChainBuilder<'_> {
    fn gpu_view(gpu: &GpuImage) -> SlotView {
        SlotView::Tex {
            view: gpu.texture_view.clone(),
            srgb: gpu.texture_descriptor.format.is_srgb(),
            size: UVec2::new(gpu.texture_descriptor.size.width, gpu.texture_descriptor.size.height),
        }
    }

    fn resolve(&mut self, slot: Slot) -> Option<usize> {
        if let Some(found) = self.index.get(&slot) {
            return *found;
        }
        let mut clear: Option<[f32; 4]> = None;
        let made: Option<SlotView> = match slot {
            Slot::None => None,
            Slot::Source => Some(SlotView::Source),
            Slot::Dest => Some(SlotView::Dest),
            Slot::Image(i) => self
                .chain
                .images
                .get(i as usize)
                .and_then(|h| self.images.get(h))
                .map(Self::gpu_view),
            Slot::Buffer(b) => {
                let produced = self
                    .frame
                    .buffers
                    .iter()
                    .find(|(n, _)| *n == b)
                    .and_then(|(_, h)| self.images.get(h));
                match produced {
                    Some(gpu) => Some(Self::gpu_view(gpu)),
                    None => {
                        let (view, init) = self.scratch(100 + b, self.size, false);
                        clear = init;
                        Some(view)
                    }
                }
            }
            Slot::Temp(t) => {
                let ratio = self.chain.temp_ratio.get(t as usize).copied().unwrap_or(1).max(1);
                let size = (self.size / ratio).max(UVec2::ONE);
                let (view, init) = self.scratch(t, size, self.chain.neutral.contains(&t));
                clear = init;
                Some(view)
            }
        };
        let found = made.map(|v| {
            self.views.push(v);
            self.views.len() - 1
        });
        if let (Some(i), Some(color)) = (found, clear) {
            self.steps.push(Step::Clear { dst: i, color });
        }
        self.index.insert(slot, found);
        found
    }

    /// A persistent texture of this chain (recreated when the size changes);
    /// the colour of its first clear when it is new.
    fn scratch(&mut self, slot: u8, size: UVec2, neutral: bool) -> (SlotView, Option<[f32; 4]>) {
        let key = (self.chain.uid, slot, 0);
        self.used.insert(key);
        let recreate = self.store.map.get(&key).is_none_or(|s| s.size != size);
        if recreate {
            let texture = self.device.create_texture(&TextureDescriptor {
                label: Some("swarm filter texture"),
                size: Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&TextureViewDescriptor::default());
            self.store.map.insert(
                key,
                Stored {
                    size,
                    _texture: texture,
                    view,
                    cleared: false,
                },
            );
        }
        let stored = self.store.map.get_mut(&key).unwrap();
        let view = SlotView::Tex {
            view: stored.view.clone(),
            srgb: false,
            size,
        };
        let init = if stored.cleared {
            None
        } else {
            stored.cleared = true;
            Some(if neutral { [0.5, 0.5, 0.5, 1.0] } else { [0.0; 4] })
        };
        (view, init)
    }

    fn srgb(&self, v: usize) -> bool {
        match &self.views[v] {
            SlotView::Source | SlotView::Dest => self.main_format.is_srgb(),
            SlotView::Tex { srgb, .. } => *srgb,
        }
    }

    fn texel(&self, v: usize) -> [f32; 2] {
        let s = match &self.views[v] {
            SlotView::Source | SlotView::Dest => self.size,
            SlotView::Tex { size, .. } => *size,
        };
        [1.0 / s.x.max(1) as f32, 1.0 / s.y.max(1) as f32]
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        cache: &PipelineCache,
        pipeline: &FilterPipeline,
        specialized: &mut SpecializedRenderPipelines<FilterPipeline>,
        kind: u32,
        blend: u8,
        mut flags: u32,
        inputs: [Option<usize>; 3],
        dst: usize,
        p: &[[f32; 4]; 3],
    ) {
        for (bit, input) in [(FLAG_A_SRGB, inputs[0]), (FLAG_B_SRGB, inputs[1]), (FLAG_C_SRGB, inputs[2])] {
            if input.is_some_and(|i| self.srgb(i)) {
                flags |= bit;
            }
        }
        if self.srgb(dst) {
            flags |= FLAG_DST_SRGB;
        }
        let texel_a = inputs[0].map_or([0.0; 2], |i| self.texel(i));
        let texel_d = self.texel(dst);
        let format = match &self.views[dst] {
            SlotView::Source | SlotView::Dest => self.main_format,
            SlotView::Tex { srgb: true, .. } => TextureFormat::Rgba8UnormSrgb,
            SlotView::Tex { .. } => TextureFormat::Rgba8Unorm,
        };
        self.steps.push(Step::Draw {
            pipeline: specialized.specialize(cache, pipeline, FilterKey { format, blend }),
            inputs,
            dst,
            blend,
            params: write_params(kind, flags, p, [texel_a[0], texel_a[1], texel_d[0], texel_d[1]]),
        });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn prepare_filters(
    mut commands: Commands,
    frame: Option<Res<SwarmFilterFrame>>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<FilterPipeline>>,
    mut specialized: ResMut<SpecializedRenderPipelines<FilterPipeline>>,
    mut store: ResMut<FilterTextures>,
    images: Res<RenderAssets<GpuImage>>,
    views: Query<(Entity, &ViewTarget, &SwarmFilterView)>,
) {
    let Some(pipeline) = pipeline else { return };
    let mut used: HashSet<(u64, u8, u8)> = HashSet::new();
    for (entity, view_target, filter_view) in &views {
        let chains: Vec<&ChainRun> = frame
            .as_ref()
            .map(|f| f.chains.iter().filter(|c| c.view == filter_view.0).collect())
            .unwrap_or_default();
        let Some(frame) = frame.as_deref().filter(|_| !chains.is_empty()) else {
            commands.entity(entity).remove::<FilterPrepared>();
            continue;
        };
        let main = view_target.main_texture();
        let size = UVec2::new(main.width(), main.height());
        let mut prepared = FilterPrepared::default();
        for chain in chains {
            let mut b = ChainBuilder {
                chain,
                frame,
                images: &images,
                device: &render_device,
                store: &mut store,
                used: &mut used,
                size,
                main_format: view_target.main_texture_format(),
                views: vec![],
                steps: vec![],
                index: HashMap::new(),
            };
            let (Some(src), Some(dest)) = (b.resolve(Slot::Source), b.resolve(Slot::Dest)) else {
                continue;
            };
            // The destination starts as a copy of the source when something
            // blends into it or reads it.
            let needs_dest_copy = chain
                .ops
                .iter()
                .any(|op| (op.dst == Slot::Dest && op.blend != 0) || [op.a, op.b, op.c].contains(&Slot::Dest));
            let none = [[0.0; 4]; 3];
            if needs_dest_copy {
                b.draw(&pipeline_cache, &pipeline, &mut specialized, 255, 0, 0, [Some(src), None, None], dest, &none);
            }
            let mut wrote_dest = false;
            let mut seeded = false;
            for op in &chain.ops {
                let Some(dst) = b.resolve(op.dst) else { continue };
                let (a, bb, c) = (b.resolve(op.a), b.resolve(op.b), b.resolve(op.c));
                if a.is_none() || (op.b != Slot::None && bb.is_none()) || (op.c != Slot::None && c.is_none()) {
                    continue;
                }
                // Scratch buffer 1 (instance 2) starts each frame as the
                // source (ADAPTED).
                if !seeded && [op.a, op.b, op.c, op.dst].contains(&Slot::Buffer(1)) {
                    seeded = true;
                    if !frame.buffers.iter().any(|(n, _)| *n == 1) {
                        if let Some(buffer) = b.resolve(Slot::Buffer(1)) {
                            b.draw(&pipeline_cache, &pipeline, &mut specialized, 255, 0, 0, [Some(src), None, None], buffer, &none);
                        }
                    }
                }
                if op.dst == Slot::Dest {
                    wrote_dest = true;
                }
                b.draw(&pipeline_cache, &pipeline, &mut specialized, op.kind, op.blend, op.flags, [a, bb, c], dst, &op.p);
            }
            if !wrote_dest {
                b.draw(&pipeline_cache, &pipeline, &mut specialized, 255, 0, 0, [Some(src), None, None], dest, &none);
            }
            // `OS_FILTER_SHOW=source|buf<n>|t<n>` shows one texture instead.
            if let Ok(show) = std::env::var("OS_FILTER_SHOW") {
                let slot = match show.as_str() {
                    "source" => Some(Slot::Source),
                    s if s.starts_with("buf") => s[3..].parse().ok().map(Slot::Buffer),
                    s if s.starts_with('t') => s[1..].parse().ok().map(Slot::Temp),
                    _ => None,
                };
                if let Some(shown) = slot.and_then(|slot| b.resolve(slot)) {
                    b.draw(&pipeline_cache, &pipeline, &mut specialized, 255, 0, 0, [Some(shown), None, None], dest, &none);
                }
            }
            prepared.chains.push(PreparedChain {
                views: b.views,
                steps: b.steps,
            });
        }
        commands.entity(entity).insert(prepared);
    }
    store.map.retain(|key, _| used.contains(key));
}

#[allow(clippy::too_many_arguments)]
fn swarm_filters(
    view: ViewQuery<(&ViewTarget, &FilterPrepared)>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<FilterPipeline>>,
    mut ctx: RenderContext,
) {
    let Some(pipeline) = pipeline else { return };
    let (view_target, prepared) = view.into_inner();
    for chain in &prepared.chains {
        let post = view_target.post_process_write();
        let pick = |v: &SlotView| -> TextureView {
            match v {
                SlotView::Source => post.source.clone(),
                SlotView::Dest => post.destination.clone(),
                SlotView::Tex { view, .. } => view.clone(),
            }
        };
        for step in &chain.steps {
            match step {
                Step::Clear { dst, color } => {
                    let target = pick(&chain.views[*dst]);
                    let _pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
                        label: Some("swarm filter clear"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(LinearRgba::new(color[0], color[1], color[2], color[3]).into()),
                                store: bevy::render::render_resource::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                Step::Draw {
                    pipeline: id,
                    inputs,
                    dst,
                    blend,
                    params,
                } => {
                    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(*id) else {
                        continue;
                    };
                    let uniform = ctx.render_device().create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("swarm filter params"),
                        contents: params,
                        usage: BufferUsages::UNIFORM,
                    });
                    let views: Vec<TextureView> = inputs
                        .iter()
                        .map(|i| i.map_or_else(|| pipeline.blank.clone(), |i| pick(&chain.views[i])))
                        .collect();
                    let bind_group = ctx.render_device().create_bind_group(
                        Some("swarm filter bind group"),
                        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
                        &BindGroupEntries::sequential((
                            &pipeline.sampler,
                            &views[0],
                            &views[1],
                            &views[2],
                            uniform.as_entire_binding(),
                        )),
                    );
                    let target = pick(&chain.views[*dst]);
                    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
                        label: Some("swarm filter"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: if *blend != 0 {
                                    LoadOp::Load
                                } else {
                                    LoadOp::Clear(Default::default())
                                },
                                store: bevy::render::render_resource::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_render_pipeline(render_pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
    }
}
