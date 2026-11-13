//! Swarm volume components (`volume`, component type 0x28): the camp fire,
//! torch, totem and fire-dance flames of the Tribal stage and the other
//! volumetric fires of the installed effects.
//!
//! NATIVE (code image, ModAPI march2017 addresses; notes
//! `notes\swarm-volume-notes.md`):
//! * Description 0x7EE600 (0xF0 bytes, read 0x7EE6F0): `offset` (vec3 at
//!   +0x08, the slicing anchor), `sliceSpacing` (+0x14, default 0.25),
//!   colour / alpha (+0x18..+0x24, never read from the file: always 1),
//!   `material` (+0x28, default 0x554771E5) and 8 corners (+0x34, 24 bytes
//!   each: position, then a 3D texture coordinate `uvw`; corner bit 0 = x,
//!   bit 1 = y, bit 2 = z).
//! * Instance (vtable 0x1414994): `SetTransforms` 0x7EF7F0 moves the corner
//!   positions into world space (rotation, uniform scale, offset); the uvw
//!   stay as authored. `IsRunning` 0xFEB920 is the visible flag: a volume
//!   runs from `Start` until `Stop` hides it.
//! * Drawing 0x7F12E0 -> slicing 0x7EFDD0: with the camera view direction
//!   `dir`, every corner gets `d = corner . dir`; planes `d = offset . dir +
//!   n * sliceSpacing` (world units, `n` integer) between the nearest and the
//!   farthest corner, from far to near, cut the box into convex polygons
//!   (edge intersections, position and uvw interpolated along the 12 box
//!   edges), drawn as triangle fans.
//! * Material: effdir material whose shader 0x4170FC29 compiles (links
//!   0x40212000!3, compiled states 0x40212001!3) to builder 0x7000009F
//!   `volumeFire` (VS `toClipSpace` + `volumeFire`, PS `volumeFire`, HLSL in
//!   the fragment pack 0x40212002!3) with `SRCBLEND = SRCALPHA, DESTBLEND =
//!   ONE` (additive), sampler 0 = the fire profile (border addressing, no
//!   mips), sampler 1 = `mNoise` (wrap, linear, no mips). Shader data 0x205
//!   `mNoiseScale` = the effdir material's 5-float property 9: frequency
//!   (xyz), magnitude (w) and a time scale that the 0x205 callback 0x7796E0
//!   multiplies by the graphics clock. The pixel shader (ported in
//!   `shaders/swarm_volume.wesl`): `u = |uv * 2 - 1|`, `noise = mag *
//!   mNoise(pos.xy * freq.xy, mTime * 0.6 - pos.z * freq.z)`, `v = 1 - w (1 +
//!   noise)`, colour = fire(u, v).
//!
//! ADAPTED: the noise position `pos` (retail: shader data 0x23E
//! `worldToPatch` of the world position, a planet patch frame) is the world
//! position in the effect's own axes (Spore Z = up); the graphics clock is the
//! app's elapsed time; the border colour is emulated by returning nothing
//! outside the fire texture; the fire texture is filtered linearly (its
//! compiled state leaves the filter to the renderer default); the additive
//! blend happens in Bevy's linear space like the additive particles.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension,
        TextureFormat,
    },
    shader::ShaderRef,
};

use super::format::{MaterialDef, MaterialValue, ResourceId, VolumeDef};
use super::sim::{Frame, VolumeState};
use super::{SwarmEffect, SwarmLibrary, SwarmSet};
use crate::{asset_catalog::RW4, dbpf::ResourceKey};

/// `volumeFire` material shader id (effdir material `shader`).
pub const SHADER_VOLUME_FIRE: u32 = 0x4170_FC29;
/// Effdir material property holding shader data 0x205 `mNoiseScale`.
const PROP_NOISE_SCALE: u32 = 9;
/// Retail caps nothing; this keeps a broken spacing from flooding the mesh.
const MAX_SLICES: usize = 512;

/// The 12 edges of a volume box (corners differing in exactly one bit).
pub const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// A slice vertex: world position and the interpolated 3D texture coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceVertex {
    pub position: Vec3,
    pub uvw: Vec3,
}

/// Pure: cut a volume box (8 world-space corners with their uvw) into
/// view-aligned slice polygons, far to near (retail 0x7EFDD0). `dir` is the
/// camera view direction (unit), `anchor` = `offset . dir`.
pub fn slice_volume(corners: &[SliceVertex; 8], dir: Vec3, anchor: f32, spacing: f32) -> Vec<Vec<SliceVertex>> {
    let mut out = Vec::new();
    if !(spacing > 1e-5) || dir.length_squared() < 1e-12 {
        return out;
    }
    let d: [f32; 8] = std::array::from_fn(|i| corners[i].position.dot(dir));
    let dmin = d.iter().copied().fold(f32::INFINITY, f32::min);
    let dmax = d.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !dmin.is_finite() || !dmax.is_finite() {
        return out;
    }
    // The farthest plane on the anchor's lattice at or before the far corner.
    let mut plane = anchor + ((dmax - anchor) / spacing).floor() * spacing;
    let (u_axis, v_axis) = plane_basis(dir);
    while plane > dmin && out.len() < MAX_SLICES {
        let mut points: Vec<SliceVertex> = Vec::with_capacity(6);
        for &(a, b) in &EDGES {
            let (da, db) = (d[a], d[b]);
            if (da - db).abs() < 1e-9 {
                continue;
            }
            // Half-open on the low end so a plane through a corner is
            // counted once per edge pair.
            let (lo, hi) = if da < db { (da, db) } else { (db, da) };
            if !(plane > lo && plane <= hi) {
                continue;
            }
            let t = (plane - da) / (db - da);
            let p = SliceVertex {
                position: corners[a].position.lerp(corners[b].position, t),
                uvw: corners[a].uvw.lerp(corners[b].uvw, t),
            };
            if !points.iter().any(|q| q.position.distance_squared(p.position) < 1e-10) {
                points.push(p);
            }
        }
        if points.len() >= 3 {
            let centre = points.iter().map(|p| p.position).sum::<Vec3>() / points.len() as f32;
            points.sort_by(|a, b| {
                let angle = |p: &SliceVertex| {
                    let r = p.position - centre;
                    r.dot(v_axis).atan2(r.dot(u_axis))
                };
                angle(a).partial_cmp(&angle(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
            out.push(points);
        }
        plane -= spacing;
    }
    out
}

fn plane_basis(dir: Vec3) -> (Vec3, Vec3) {
    let helper = if dir.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = dir.cross(helper).normalize();
    (u, dir.cross(u))
}

/// World-space corners of a volume under its block frame (retail
/// `SetTransforms` 0x7EF7F0: rotation, uniform scale, offset).
pub fn world_corners(def: &VolumeDef, frame: &Frame) -> [SliceVertex; 8] {
    std::array::from_fn(|i| {
        let c = def.corners[i];
        SliceVertex {
            position: frame.point(Vec3::new(c[0], c[1], c[2])),
            uvw: Vec3::new(c[3], c[4], c[5]),
        }
    })
}

/// `mNoiseScale` (shader data 0x205) of a volume material: frequency xyz,
/// magnitude, time scale (property 9, five floats).
pub fn noise_scale(material: &MaterialDef) -> Option<[f32; 5]> {
    material.properties.iter().find_map(|p| match &p.value {
        MaterialValue::Floats(v) if p.name.instance == PROP_NOISE_SCALE && v.len() >= 5 => {
            Some([v[0], v[1], v[2], v[3], v[4]])
        }
        _ => None,
    })
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SwarmVolumeMaterial {
    /// Sampler 0: the fire profile (u = distance from the axis, v = 1 - height).
    #[texture(0)]
    #[sampler(1)]
    pub fire: Handle<Image>,
    /// Sampler 1: `mNoise`.
    #[texture(2)]
    #[sampler(3)]
    pub noise: Handle<Image>,
    /// `mNoiseScale.mFrequencyMag`.
    #[uniform(4)]
    pub frequency_mag: Vec4,
}

impl Material for SwarmVolumeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/swarm_volume.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Slices face the camera either way; additive, depth-tested, no
        // depth writes (like every effect pass).
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// How one volume material draws.
#[derive(Clone)]
struct VolumeLook {
    material: Handle<SwarmVolumeMaterial>,
    time_scale: f32,
}

#[derive(Resource, Default)]
pub struct SwarmVolumeCache {
    textures: HashMap<(u32, bool), Option<Handle<Image>>>,
    looks: HashMap<(u16, u32), Option<VolumeLook>>,
    /// Draw entity and its render layer per volume uid.
    draws: HashMap<u64, (Entity, usize)>,
    reported: HashSet<(u16, u32)>,
}

impl SwarmVolumeCache {
    /// Level 0 of an installed raster (the volume samplers use no mips).
    fn texture(
        &mut self,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        id: ResourceId,
        noise: bool,
    ) -> Option<Handle<Image>> {
        if let Some(found) = self.textures.get(&(id.instance, noise)) {
            return found.clone();
        }
        let key = ResourceKey {
            type_id: RW4,
            group_id: if id.group == u32::MAX { 0 } else { id.group },
            instance_id: id.instance,
        };
        let found = originals
            .resource_index
            .read(key)
            .ok()
            .and_then(|bytes| crate::texture::decode_rw4_mips(&bytes))
            .and_then(|mips| mips.into_iter().next())
            .map(|level| {
                let mut image = Image::new(
                    Extent3d {
                        width: level.width,
                        height: level.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    level.rgba,
                    // The noise is data (raw channels); the fire is colour.
                    if noise {
                        TextureFormat::Rgba8Unorm
                    } else {
                        TextureFormat::Rgba8UnormSrgb
                    },
                    RenderAssetUsages::default(),
                );
                let address = if noise {
                    ImageAddressMode::Repeat
                } else {
                    ImageAddressMode::ClampToEdge
                };
                image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: address,
                    address_mode_v: address,
                    mag_filter: ImageFilterMode::Linear,
                    min_filter: ImageFilterMode::Linear,
                    mipmap_filter: ImageFilterMode::Nearest,
                    ..default()
                });
                images.add(image)
            });
        if found.is_none() {
            warn!("Swarm volume texture {:08x}!{:08x} is not an installed raster", id.group, id.instance);
        }
        self.textures.insert((id.instance, noise), found.clone());
        found
    }

    fn look(
        &mut self,
        lib: &SwarmLibrary,
        originals: &crate::Originals,
        images: &mut Assets<Image>,
        materials: &mut Assets<SwarmVolumeMaterial>,
        dir: u16,
        def: &VolumeDef,
    ) -> Option<VolumeLook> {
        let key = (dir, def.material.instance);
        if let Some(found) = self.looks.get(&key) {
            return found.clone();
        }
        let look = (|| {
            let material = lib.dir(dir).materials.get(&def.material.instance)?;
            if material.shader.instance != SHADER_VOLUME_FIRE {
                return None;
            }
            let fire = material.sampler(2).or_else(|| material.first_texture())?;
            let noise = material.sampler(3)?;
            let scale = noise_scale(material)?;
            let fire = self.texture(originals, images, fire, false)?;
            let noise = self.texture(originals, images, noise, true)?;
            Some(VolumeLook {
                material: materials.add(SwarmVolumeMaterial {
                    fire,
                    noise,
                    frequency_mag: Vec4::new(scale[0], scale[1], scale[2], scale[3]),
                }),
                time_scale: scale[4],
            })
        })();
        if look.is_none() && self.reported.insert(key) {
            warn!(
                "Swarm volume material {:08x} (dir {dir}) is not a decodable volumeFire material",
                def.material.instance
            );
        }
        self.looks.insert(key, look.clone());
        look
    }
}

/// Registers the volume material and draw system (called by
/// `SwarmFxRenderPlugin`; the shader is embedded there).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<SwarmVolumeMaterial>::default())
        .init_resource::<SwarmVolumeCache>()
        .add_systems(
            PostUpdate,
            draw_volumes
                .in_set(SwarmSet::Draw)
                .after(SwarmSet::Simulate)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
        );
}

/// Marker for volume draw entities.
#[derive(Component)]
pub struct SwarmVolumeDraw;

#[derive(Default)]
struct VolumeMesh {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    uv_b: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl VolumeMesh {
    fn write(self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv_b);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
    }
    fn placeholder() -> Self {
        VolumeMesh {
            positions: vec![[0.0; 3]; 3],
            uv: vec![[0.0; 2]; 3],
            uv_b: vec![[0.0; 2]; 3],
            colors: vec![[0.0; 4]; 3],
            indices: vec![0, 1, 2],
        }
    }
}

/// Mesh of one volume: slice polygons as fans, vertices relative to the
/// frame origin. Per vertex: uv = uvw.xy, uv_b.x = uvw.z, colour = (noise
/// position, mTime).
fn volume_mesh(def: &VolumeDef, frame: &Frame, dir: Vec3, m_time: f32) -> VolumeMesh {
    let corners = world_corners(def, frame);
    let anchor = Vec3::from_array(def.offset).dot(dir);
    let mut out = VolumeMesh::default();
    // ADAPTED: OpenSpore fits props (and so their model effects) to its own
    // sizes, often far below the retail model scale (the totem's flames ride
    // a 0.19 frame); below scale 1 the slice spacing and the noise space
    // shrink with the frame so the flame keeps its authored slice count and
    // flicker. At scale >= 1 both stay in world units as retail.
    let shrink = frame.scale.clamp(1e-3, 1.0);
    // ADAPTED worldToPatch: world position in the effect's axes (Z up).
    let to_patch = frame.rot.transpose() / shrink;
    for polygon in slice_volume(&corners, dir, anchor, def.slice_spacing * shrink) {
        let base = out.positions.len() as u32;
        for v in &polygon {
            let patch = to_patch * v.position;
            out.positions.push((v.position - frame.origin).to_array());
            out.uv.push([v.uvw.x, v.uvw.y]);
            out.uv_b.push([v.uvw.z, 0.0]);
            out.colors.push([patch.x, patch.y, patch.z, m_time]);
        }
        for k in 2..polygon.len() as u32 {
            out.indices.extend([base, base + k - 1, base + k]);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn draw_volumes(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    time: Res<Time>,
    mut cache: ResMut<SwarmVolumeCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SwarmVolumeMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<
        (&mut Transform, &Mesh3d, &mut MeshMaterial3d<SwarmVolumeMaterial>, &mut Visibility),
        With<SwarmVolumeDraw>,
    >,
) {
    let Some(originals) = originals else { return };
    let Some(camera) = camera.iter().next() else { return };
    let dir = camera.forward().as_vec3();
    let hidden = editor.is_some_and(|e| e.open);
    let clock = time.elapsed_secs();
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_volume(&lib, &mut |state: &VolumeState, def: &VolumeDef| {
            let Some(frame) = state.frame else { return };
            let Some(look) = cache.look(&lib, &originals, &mut images, &mut materials, state.dir, def) else {
                return;
            };
            seen.insert(state.uid);
            let mesh = if hidden || state.stopped {
                VolumeMesh::default()
            } else {
                volume_mesh(def, &frame, dir, clock * look.time_scale)
            };
            let visible = !mesh.indices.is_empty();
            if std::env::var_os("OPENSPORE_FX_DEBUG").is_some() {
                let lo = mesh.positions.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(Vec3::from_array(*p)));
                let hi = mesh.positions.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(Vec3::from_array(*p)));
                info!(
                    "volume {} mesh local box {lo:?}..{hi:?} first uvw {:?} {:?} colour {:?} rot {:?}",
                    state.uid,
                    mesh.uv.first(),
                    mesh.uv_b.first(),
                    mesh.colors.first(),
                    frame.rot
                );
                info!(
                    "volume {} of fx {:08x}: origin {:?} scale {:.3} vertices {} triangles {} stopped {} (camera {:?} looking {:?})",
                    state.uid,
                    effect.effect,
                    frame.origin,
                    frame.scale,
                    mesh.positions.len(),
                    mesh.indices.len() / 3,
                    state.stopped,
                    camera.translation(),
                    dir
                );
            }
            let layer = effect.layer;
            match cache.draws.get(&state.uid).copied().and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d))) {
                Some((entity, drawn_layer, (mut transform, handle, mut material, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        cache.draws.insert(state.uid, (entity, layer));
                    }
                    transform.translation = frame.origin;
                    if material.0 != look.material {
                        material.0 = look.material.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if visible {
                        if let Some(mut m) = meshes.get_mut(&handle.0) {
                            mesh.write(&mut m);
                        }
                    }
                }
                None => {
                    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                    if visible { mesh } else { VolumeMesh::placeholder() }.write(&mut m);
                    let entity = commands
                        .spawn((
                            SwarmVolumeDraw,
                            // Also a Swarm draw: `terrain::ground` leaves those
                            // alone (the vertices are already in world space;
                            // grounding moved model-attached flames off their
                            // props by the ground height again).
                            super::render::SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(m)),
                            MeshMaterial3d(look.material.clone()),
                            Transform::from_translation(frame.origin),
                            GlobalTransform::from_translation(frame.origin),
                            if visible { Visibility::Visible } else { Visibility::Hidden },
                            NoFrustumCulling,
                            NotShadowCaster,
                            NotShadowReceiver,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    cache.draws.insert(state.uid, (entity, layer));
                }
            }
        });
    }
    cache.draws.retain(|uid, (entity, _)| {
        if seen.contains(uid) {
            return true;
        }
        if let Ok(mut e) = commands.get_entity(*entity) {
            e.despawn();
        }
        false
    });
}

/// Quick-capture support for any stage but Cell (`cell_fx.rs` has its own):
/// `--swarm-fx NAME|0xID` plays an installed effect at frame `--swarm-fx-at
/// N` (default `frames - 60`) on the player (or `--swarm-fx-dist D` world
/// units in front of the camera on the ground plane when there is no player;
/// `--swarm-fx-pos X,Y,Z` names the world position), scaled by
/// `--swarm-fx-scale S`.
pub fn capture_fx_step(app: &mut App, args: &[String], frame: usize, frames: usize) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    if let Some(mode) = flag("--swarm-fx-tribal") {
        tribal_capture_stage(app, &mode, frame);
    }
    let Some(name) = flag("--swarm-fx") else { return };
    let at = flag("--swarm-fx-at")
        .and_then(|v| v.parse().ok())
        .unwrap_or(frames.saturating_sub(60));
    if frame != at {
        return;
    }
    let id = name
        .strip_prefix("0x")
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .unwrap_or_else(|| crate::world_objects::name_id(&name));
    let scale = flag("--swarm-fx-scale").and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let world = app.world_mut();
    let player = world
        .query_filtered::<&GlobalTransform, With<crate::game::Player>>()
        .iter(world)
        .next()
        .map(|g| (g.translation(), g.rotation()));
    // Upright on the player's ground (the planet stages are spheres).
    let rotation = player.map_or(Quat::IDENTITY, |(_, r)| r);
    let explicit = flag("--swarm-fx-pos").and_then(|v| {
        let c: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (c.len() == 3).then(|| Vec3::new(c[0], c[1], c[2]))
    });
    let position = match (player, flag("--swarm-fx-dist").and_then(|v| v.parse::<f32>().ok())) {
        _ if explicit.is_some() => explicit.unwrap(),
        (Some((p, _)), None) => p,
        (_, distance) => {
            let camera = world
                .query_filtered::<&GlobalTransform, With<crate::GameCamera>>()
                .iter(world)
                .next()
                .copied()
                .unwrap_or_default();
            let up = rotation * Vec3::Y;
            let forward = camera.forward().as_vec3();
            let ahead = (forward - up * forward.dot(up)).normalize_or_zero();
            let p = camera.translation() + ahead * distance.unwrap_or(10.0);
            // Back down onto the player's ground plane.
            player.map_or(p, |(pl, _)| p - up * (p - pl).dot(up))
        }
    };
    let mut play = super::SwarmPlay::at(id, position);
    play.rotation = rotation;
    play.scale = scale;
    world.write_message(play);
    info!("swarm-fx capture: played {name} ({id:08x}) at {position:?} on frame {frame}");
}

/// Tribal quick captures, `--swarm-fx-tribal campfire|totem|village`: build
/// the player's camp fire (its PROP and model effects) and frame it, frame
/// the totem, or frame rival village 1 (with its camp fire) so their flames
/// can be looked at.
fn tribal_capture_stage(app: &mut App, mode: &str, frame: usize) {
    use crate::tribal_tools::ToolKind;
    if app.world().get_resource::<crate::tribal_controls::TribalControls>().is_none() {
        return;
    }
    let world = app.world_mut();
    let _layout = crate::tribal_planet::scope_world(world);
    let mut target = None;
    {
        let mut run = world.resource_mut::<crate::game::RunProgress>();
        let campaign = &mut run.tribal_campaign;
        match mode {
            "campfire" => {
                if frame == 20 && !campaign.has_shack(ToolKind::Campfire) {
                    campaign.shacks.push(ToolKind::Campfire);
                }
                if frame >= 300 && frame % 60 == 0 {
                    target = campaign
                        .shacks
                        .iter()
                        .position(|k| *k == ToolKind::Campfire)
                        .map(crate::tribal_tools::shack_position);
                }
            }
            "totem" if frame >= 300 && frame % 60 == 0 => target = Some(crate::tribal::totem_position()),
            "village" => {
                if frame == 20 {
                    if let Some(v) = campaign.villages.get_mut(0) {
                        v.appeared = true;
                    }
                }
                if frame >= 30 && frame % 60 == 30 {
                    target = Some(crate::tribal::village_position(0));
                }
            }
            _ => {}
        }
    }
    if let Some(point) = target {
        world.resource_mut::<crate::tribal_controls::TribalControls>().goto_request = Some(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box(spacing_axis: f32) -> [SliceVertex; 8] {
        std::array::from_fn(|i| {
            let bit = |b: usize| if i & b != 0 { 1.0 } else { 0.0 };
            SliceVertex {
                position: Vec3::new(bit(1), bit(2), bit(4) * spacing_axis),
                uvw: Vec3::new(bit(1), bit(2), bit(4)),
            }
        })
    }

    /// Planes along the box axis give rectangles at every lattice step
    /// strictly inside the box, far to near, with uvw interpolated.
    #[test]
    fn axis_aligned_slices_are_rectangles_on_the_lattice() {
        let corners = unit_box(1.0);
        let slices = slice_volume(&corners, Vec3::Z, 0.0, 0.25);
        // Planes at 1.0 (the far face: its own edges are parallel to the
        // plane and skipped, the four upright edges end on it), 0.75, 0.5,
        // 0.25; 0.0 is not beyond the near corner.
        let heights: Vec<f32> = slices.iter().map(|s| s[0].position.z).collect();
        assert_eq!(heights, vec![1.0, 0.75, 0.5, 0.25]);
        for s in &slices {
            assert_eq!(s.len(), 4);
            for v in s {
                assert!((v.uvw.z - v.position.z).abs() < 1e-6);
            }
        }
    }

    /// A diagonal view cuts triangles near the corners and hexagons through
    /// the middle; every vertex lies on its plane and inside the box.
    #[test]
    fn diagonal_slices_are_convex_polygons_on_their_planes() {
        let corners = unit_box(1.0);
        let dir = Vec3::ONE.normalize();
        let slices = slice_volume(&corners, dir, 0.0, 0.1);
        assert!(slices.iter().any(|s| s.len() == 6));
        assert!(slices.iter().any(|s| s.len() == 3));
        let mut last = f32::INFINITY;
        for s in &slices {
            let d = s[0].position.dot(dir);
            assert!(d < last + 1e-5, "far to near");
            last = d;
            for v in s {
                assert!((v.position.dot(dir) - d).abs() < 1e-4);
                assert!(v.position.min_element() > -1e-5 && v.position.max_element() < 1.0 + 1e-5);
                assert!((v.uvw - v.position).length() < 1e-5);
            }
            // Ordered around the centre: consecutive cross products agree.
            let c = s.iter().map(|v| v.position).sum::<Vec3>() / s.len() as f32;
            for k in 0..s.len() {
                let a = s[k].position - c;
                let b = s[(k + 1) % s.len()].position - c;
                assert!(a.cross(b).dot(dir) >= -1e-6);
            }
        }
    }

    /// The anchor shifts the lattice; a broken spacing draws nothing.
    #[test]
    fn anchor_and_spacing_rules() {
        let corners = unit_box(1.0);
        let slices = slice_volume(&corners, Vec3::Z, 0.1, 0.25);
        let heights: Vec<f32> = slices.iter().map(|s| (s[0].position.z * 100.0).round() / 100.0).collect();
        assert_eq!(heights, vec![0.85, 0.6, 0.35, 0.1]);
        assert!(slice_volume(&corners, Vec3::Z, 0.0, 0.0).is_empty());
        assert!(slice_volume(&corners, Vec3::ZERO, 0.0, 0.25).is_empty());
    }

    /// Corners follow the block frame; uvw stay as authored.
    #[test]
    fn world_corners_apply_the_frame() {
        let mut def = VolumeDef::default();
        for i in 0..8 {
            let s = |b: usize| if i & b != 0 { 1.0 } else { -1.0 };
            def.corners[i] = [s(1), s(2), s(4) + 1.0, 0.5, 0.5, i as f32];
        }
        let frame = Frame::new(Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY, 2.0);
        let w = world_corners(&def, &frame);
        // Spore (x, y, z) -> Bevy (x, z, -y), scaled 2, offset 10 on x.
        assert!((w[7].position - Vec3::new(12.0, 4.0, -2.0)).length() < 1e-5);
        assert_eq!(w[3].uvw, Vec3::new(0.5, 0.5, 3.0));
    }

    #[test]
    fn noise_scale_reads_property_nine() {
        let m = MaterialDef {
            id: ResourceId { group: 0, instance: 1 },
            shader: ResourceId { group: 0, instance: SHADER_VOLUME_FIRE },
            properties: vec![super::super::format::MaterialProperty {
                name: ResourceId { group: 0, instance: 9 },
                value: MaterialValue::Floats(vec![6.0, 6.0, 3.0, 0.6, 4.0]),
            }],
        };
        assert_eq!(noise_scale(&m), Some([6.0, 6.0, 3.0, 0.6, 4.0]));
    }

    /// `cargo test survey_volumes -- --ignored --nocapture`: every installed
    /// volume component, its material and what reaches it.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_volumes() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        for (d, dir) in lib.dirs.iter().enumerate() {
            for (i, v) in dir.volumes.iter().enumerate() {
                let m = dir.materials.get(&v.material.instance);
                println!(
                    "dir {d} volume {i}: spacing {} material {:08x} shader {:08x?} noise {:?}",
                    v.slice_spacing,
                    v.material.instance,
                    m.map(|m| m.shader.instance),
                    m.and_then(noise_scale)
                );
            }
        }
    }
}
