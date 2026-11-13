//! Drawing side of the galactic sprite programs (`galactic.rs`): the additive material whose pixel stage is
//! the programs' (two strip taps blended, times the vertex colour; `shaders/swarm_galactic.wesl`), the mesh
//! writer shared with the galaxy view's disk layer (`space_galaxy/disks.rs`), and the Swarm particle path:
//! particles whose texture slot names a galactic material (`Program::from_material`) are drawn here from
//! the vertex stage's outputs instead of by `render.rs::draw_particles` (which skips them).
//!
//! The vertex data carries the stage's outputs: position = centre + the corner's world offset, uv = the
//! first tile coordinate, uv_b = the second, colour = oT3 (a signed alpha: the retail blend is
//! SRCALPHA / ONE, so a negative tilt alpha subtracts) and the tile blend in the tangent's w (the engine's
//! vertex stage passes it through unchanged).
//!
//! Nothing in the installed Swarm effects that the stages play uses these materials today (the Space
//! galaxy directory 0xC1195706 is built by `space_galaxy`, which draws its disks with this module; the
//! editor effect directory 0xBAFC5CA1's `editor_skybox` stars are not played): the particle path is
//! exercised by tests only.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
};

use super::format::{draw, pflag, MaterialDef, ParticleDef, ResourceId};
use super::galactic::{self, Output, Program, Registers};
use super::sim::{curve, curve3, loop_box_fade, ParticleSystem};
use super::{SwarmEffect, SwarmLibrary, SwarmSet};
use crate::{asset_catalog::RW4, dbpf::ResourceKey};

/// The galactic sprite material: the strip raster, sampled as stored (the retail blend runs in gamma space and
/// the shader converts its result, like `space_stars.wesl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct GalacticMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

impl Material for GalacticMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/swarm_galactic.wesl".into()
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
        // ZWRITE 0, ZENABLE 1 (the compiled state of the three materials), no culling (the planes are seen from both sides).
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Write the sprites (four corners each, in the order of [`galactic::sprite`]) into `mesh`.
pub fn write_sprites(mesh: &mut Mesh, sprites: &[(Vec3, [Output; 4])]) {
    let n = sprites.len() * 4;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut uv: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut uv_b: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut tangents: Vec<[f32; 4]> = Vec::with_capacity(n);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(n);
    let mut indices: Vec<u32> = Vec::with_capacity(sprites.len() * 6);
    for (center, corners) in sprites {
        let base = positions.len() as u32;
        for out in corners {
            positions.push((*center + out.world_offset).to_array());
            uv.push(out.uv_a.to_array());
            uv_b.push(out.uv_b.to_array());
            tangents.push([1.0, 0.0, 0.0, out.blend]);
            colors.push(out.color.to_array());
        }
        indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
    if sprites.is_empty() {
        // An empty mesh draws nothing but must keep a valid layout.
        positions = vec![[0.0; 3]; 3];
        uv = vec![[0.0; 2]; 3];
        uv_b = vec![[0.0; 2]; 3];
        tangents = vec![[1.0, 0.0, 0.0, 0.0]; 3];
        colors = vec![[0.0; 4]; 3];
        indices = vec![0, 1, 2];
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv_b);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

/// A dynamic sprite mesh (positions are rewritten every frame).
pub fn new_mesh() -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    write_sprites(&mut mesh, &[]);
    mesh
}

/// The strip raster of a galactic material as an image sampled as stored, repeating (the compiled states leave
/// the address mode at the D3D default, wrap).
pub fn load_raster(originals: &crate::Originals, id: ResourceId) -> Option<Image> {
    let key = ResourceKey { type_id: RW4, group_id: if id.group == u32::MAX { 0 } else { id.group }, instance_id: id.instance };
    crate::planet_sky::rw4_image(&originals.resource_index, key, true, false)
}

/// The vertex randoms of a simulated particle. ADAPTED: the retail vertex carries two randoms of its own
/// (v3.zw) that the simulation does not reproduce; two of the particle's draws stand in.
fn vertex_rand(p: &super::sim::Particle) -> Vec2 {
    Vec2::new(p.r[2] * 0.5 + 0.5, p.r[5] * 0.5 + 0.5)
}

/// The galactic program and material of a particle component, when its texture slot names one.
pub fn galactic_material<'a>(lib: &'a SwarmLibrary, dir: u16, def: &ParticleDef) -> Option<(Program, &'a MaterialDef)> {
    if def.texture.draw_mode != draw::MATERIAL {
        return None;
    }
    let material = lib.dir(dir).materials.get(&def.texture.resource.instance)?;
    Some((Program::from_material(material.shader.instance)?, material))
}

/// `true` for particle components `render.rs::draw_particles` leaves to this module.
pub fn is_galactic(lib: &SwarmLibrary, dir: u16, def: &ParticleDef) -> bool {
    galactic_material(lib, dir, def).is_some()
}

/// The sprites of one particle system (centre and the vertex stage's four corners). Size, alpha and colour
/// follow the same curves as `render.rs::system_quads`; colour stays in display values (the shader works in
/// gamma space).
pub fn particle_sprites(
    sys: &ParticleSystem,
    def: &ParticleDef,
    program: Program,
    regs: &Registers,
    cam: &galactic::Camera,
    view_position: Vec3,
) -> Vec<(Vec3, [Output; 4])> {
    let Some(frame) = sys.frame else { return Vec::new() };
    let mut order: Vec<usize> = (0..sys.particles.len()).collect();
    // Additive: the order does not matter, but a stable order keeps the mesh steady.
    order.sort_by(|&a, &b| {
        let da = (sys.particles[a].pos - view_position).length_squared();
        let db = (sys.particles[b].pos - view_position).length_squared();
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    let color_rgb = def.has(pflag::COLOR_VARY_RGB);
    let mut out = Vec::with_capacity(order.len());
    for i in order {
        let p = &sys.particles[i];
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let size = curve(&def.size, t, 1.0) * (1.0 + def.size_vary * p.r[0]) * sys.mods.size * frame.scale;
        if !(size > 0.0) {
            continue;
        }
        let mut alpha = (curve(&def.alpha, t, 1.0) * (1.0 + def.alpha_vary * p.r[3])).clamp(0.0, 1.0) * sys.mods.alpha;
        if def.has(pflag::LOOP_BOX) {
            alpha *= loop_box_fade(&frame, &def.emit_volume, &def.loop_box_alpha, p.pos);
        }
        if alpha <= 0.002 {
            continue;
        }
        let vary = Vec3::from_array(def.color_vary);
        let draw = if color_rgb { Vec3::from_array(p.rc) } else { Vec3::splat(p.r[4]) };
        let color = ((curve3(&def.color, t) + vary * draw) * sys.mods.color).max(Vec3::ZERO);
        let sprite = galactic::sprite(program, regs, cam, p.pos, size * 0.5, color.extend(alpha), vertex_rand(p));
        out.push((p.pos, sprite));
    }
    out
}

/// Marks the draw entities of this module.
#[derive(Component)]
pub struct GalacticDraw;

#[derive(Resource, Default)]
pub struct GalacticDrawCache {
    images: HashMap<ResourceId, Option<Handle<Image>>>,
    materials: HashMap<AssetId<Image>, Handle<GalacticMaterial>>,
    /// Draw entity and render layer per particle system uid.
    systems: HashMap<u64, (Entity, usize)>,
}

/// The engine camera as the programs see it.
pub fn camera_for(
    transform: &GlobalTransform,
    projection: &Projection,
    viewport: Vec2,
    time: f32,
) -> galactic::Camera {
    let (fov, aspect, near, far) = match projection {
        Projection::Perspective(p) => (p.fov, p.aspect_ratio, p.near, p.far),
        _ => (std::f32::consts::FRAC_PI_4, 1.4, 0.1, 1000.0),
    };
    galactic::Camera::from_frame(
        transform.translation(),
        transform.right().as_vec3(),
        transform.up().as_vec3(),
        transform.forward().as_vec3(),
        fov,
        aspect,
        near,
        far,
        viewport,
        time,
    )
}

/// Registration (called by `SwarmFxRenderPlugin`).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<GalacticMaterial>::default())
        .init_resource::<GalacticDrawCache>()
        .add_systems(
            PostUpdate,
            draw_particles
                .in_set(SwarmSet::Draw)
                .after(SwarmSet::Simulate)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
        );
}

#[allow(clippy::too_many_arguments)]
fn draw_particles(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut cache: ResMut<GalacticDrawCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GalacticMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    time: Res<Time>,
    camera: Query<(&GlobalTransform, &Projection, &bevy::camera::Camera), With<crate::GameCamera>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&Mesh3d, &mut MeshMaterial3d<GalacticMaterial>, &mut Visibility), With<GalacticDraw>>,
) {
    let Some(originals) = originals else { return };
    let Some((transform, projection, cam)) = camera.iter().next() else { return };
    let viewport = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 900.0));
    let galactic_camera = camera_for(transform, projection, viewport, time.elapsed_secs());
    let hidden = editor.is_some_and(|e| e.open);
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_particles(&lib, &mut |sys, def| {
            let Some((program, material)) = galactic_material(&lib, sys.dir, def) else { return };
            let Some(texture) = material.sampler(2).or_else(|| material.first_texture()) else { return };
            let handle = cache
                .images
                .entry(texture)
                .or_insert_with(|| load_raster(&originals, texture).map(|image| images.add(image)))
                .clone();
            let Some(handle) = handle else { return };
            let material_handle = cache
                .materials
                .entry(handle.id())
                .or_insert_with(|| materials.add(GalacticMaterial { texture: handle.clone() }))
                .clone();
            seen.insert(sys.uid);
            let regs = Registers::from_material(material);
            let sprites = if hidden {
                Vec::new()
            } else {
                particle_sprites(sys, def, program, &regs, &galactic_camera, transform.translation())
            };
            let visible = !sprites.is_empty();
            let layer = effect.layer;
            match cache.systems.get(&sys.uid).copied().and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d))) {
                Some((entity, drawn_layer, (mesh, mut mat, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        cache.systems.insert(sys.uid, (entity, layer));
                    }
                    if mat.0 != material_handle {
                        mat.0 = material_handle.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if visible {
                        if let Some(mut m) = meshes.get_mut(&mesh.0) {
                            write_sprites(&mut m, &sprites);
                        }
                    }
                }
                None => {
                    let mut mesh = new_mesh();
                    if visible {
                        write_sprites(&mut mesh, &sprites);
                    }
                    let entity = commands
                        .spawn((
                            GalacticDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(material_handle),
                            Transform::IDENTITY,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm_fx::format::ParticleDef;

    fn regs() -> Registers {
        let mut r = Registers::default();
        for (i, v) in [(4, 0.0), (5, 25.0), (6, 40.0), (7, 100.0), (8, 1.0), (9, 1.0), (10, 1.0), (11, 0.0)] {
            r.0[i] = v;
        }
        r.0[29] = 3.0;
        r.0[30] = -0.1;
        r.0[31] = 3.0;
        r.0[32] = 3.0;
        r.0[33] = 3.0;
        r.0[34] = 0.7;
        r.0[62] = 1.0;
        r
    }

    #[test]
    fn the_mesh_carries_the_stage_outputs_per_corner() {
        let cam = galactic::Camera::from_frame(
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            1.0,
            1.4,
            0.05,
            10_000.0,
            Vec2::new(1280.0, 900.0),
            3.0,
        );
        let center = Vec3::new(0.5, 0.25, 4.0);
        let sprite = galactic::sprite(Program::ZpoleTilt, &regs(), &cam, center, 0.2, Vec4::new(1.0, 0.5, 0.25, 0.8), Vec2::new(0.3, 0.6));
        let mut mesh = new_mesh();
        write_sprites(&mut mesh, &[(center, sprite)]);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().to_vec();
        assert_eq!(positions.len(), 4);
        for (corner, out) in positions.iter().zip(sprite.iter()) {
            assert!((Vec3::from_array(*corner) - (center + out.world_offset)).length() < 1e-6);
        }
        let tangents = match mesh.attribute(Mesh::ATTRIBUTE_TANGENT) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(v)) => v.clone(),
            other => panic!("tangent attribute: {other:?}"),
        };
        assert!(tangents.iter().zip(sprite.iter()).all(|(t, out)| t[3] == out.blend));
        assert_eq!(mesh.indices().map(|i| i.len()), Some(6));
        // The empty mesh keeps a valid single-triangle layout.
        assert_eq!(new_mesh().indices().map(|i| i.len()), Some(3));
    }

    #[test]
    fn a_simulated_particle_becomes_a_sprite_of_its_authored_size() {
        use crate::swarm_fx::sim::{Emitter, Frame, Mods, Particle, ParticleSystem};
        let mut def = ParticleDef::default();
        def.size = vec![0.4];
        def.alpha = vec![0.8];
        def.color = vec![[1.0, 1.0, 1.0]];
        let mut sys = ParticleSystem {
            dir: 0,
            index: 0,
            uid: 1,
            particles: vec![],
            emitter: Emitter::default(),
            frame: Some(Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0)),
            mods: Mods::default(),
            rigid: false,
            stopped: false,
        };
        sys.particles.push(Particle {
            pos: Vec3::new(0.0, 0.0, 6.0),
            vel: Vec3::ZERO,
            age: 1.0,
            life: 10.0,
            r: [0.0, 0.0, 0.3, 0.0, 0.0, -0.4],
            rc: [0.0; 3],
            frame: 0,
        });
        let cam = galactic::Camera::from_frame(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z, 1.0, 1.4, 0.05, 10_000.0, Vec2::new(1280.0, 900.0), 2.0);
        let sprites = particle_sprites(&sys, &def, Program::SpinTilt, &regs(), &cam, Vec3::ZERO);
        assert_eq!(sprites.len(), 1);
        let (_, corners) = &sprites[0];
        let width = (corners[1].world_offset - corners[0].world_offset).length();
        assert!((width - 0.4).abs() < 1e-3, "{width}");
        assert!(corners.iter().all(|c| c.color.w.abs() <= 0.8 + 1e-6));
    }
}
