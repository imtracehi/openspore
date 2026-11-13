//! Model components and model particles (`particles ... model`): installed RW4
//! models drawn with their imported materials, one entity per surface, with a
//! per-instance material copy for the authored colour and alpha.
//! NATIVE: model ids, sizes, colours, alpha, particle curves. ADAPTED: model
//! particles spin about the effect Z axis by the `rotate` curve; model
//! animations (`ModelAnimationDef`) are not played. `gameModel` components
//! (game_model.rs) draw through the same cache and entities, lit, with the
//! model key's group honoured; their `animate` entries play the model's own
//! skeletal clips (game_anim.rs) into a per-instance mesh copy.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    light::NotShadowCaster,
    prelude::*,
};

use super::format::pflag;
use super::sim::{curve, curve3, loop_box_fade, AXES};
use super::{SwarmEffect, SwarmLibrary};
use crate::{asset_catalog::RW4, dbpf::ResourceKey};

/// Model property list resource type and its `modelMeshLOD0` property.
const PROP: u32 = 0x00B1_B104;
const MODEL_MESH_LOD0: u32 = 0x00F9_EFBB;

/// Marker for model entities drawn by the Swarm runtime.
#[derive(Component)]
pub struct SwarmModelDraw;

/// Entities whose attached effects drew at least one model last frame (stage
/// code hides its stand-in meshes for them).
#[derive(Resource, Default)]
pub struct SwarmAttachedDraws(pub HashSet<Entity>);

struct ModelDraw {
    entity: Entity,
    material: Handle<StandardMaterial>,
    layer: usize,
    /// Animated draws own a mesh copy whose vertices are skinned every frame.
    mesh: Option<Handle<Mesh>>,
}

/// One drawn surface of a cached model.
#[derive(Clone)]
struct CachedSurface {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// RW4 mesh section (the key into `game_anim::AnimatedModel::skins`).
    section: usize,
}

#[derive(Clone)]
struct CachedModel {
    key: ResourceKey,
    surfaces: Vec<CachedSurface>,
}

#[derive(Resource, Default)]
pub struct SwarmModelCache {
    /// Imported RW4 models by (group, instance).
    models: HashMap<(u32, u32), Option<CachedModel>>,
    /// Skeletons and clips of the models that have them (game_anim.rs).
    animated: super::game_anim::AnimatedModels,
    /// Entities per owner (model component uid or particle system uid).
    draws: HashMap<u64, Vec<ModelDraw>>,
}

impl SwarmModelCache {
    /// Import an installed RW4 model (cached). The importer already maps
    /// Spore Z-up to Bevy Y-up, i.e. it applies [`AXES`]. The model is looked
    /// up in `group` first, then in group 0.
    fn model(
        &mut self,
        originals: &crate::Originals,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        group: u32,
        instance: u32,
    ) -> Option<CachedModel> {
        if let Some(found) = self.models.get(&(group, instance)) {
            return found.clone();
        }
        let key = resolve_model_key(originals, group, instance);
        if key.is_none() {
            warn!("Swarm model {group:08x}!{instance:08x} is not installed");
        }
        let found = key.and_then(|key| {
            let definition = crate::asset_catalog::PartDefinition::raw_model(key);
            let surfaces = match crate::editor::import_definition(originals, &definition, meshes, images, materials) {
                Ok(model) if !model.surfaces.is_empty() => {
                    if group == EDITOR_MODEL_GROUP {
                        bind_editor_background(originals, key, &model, images, materials);
                    }
                    Some(
                        model
                            .surfaces
                            .iter()
                            .map(|s| CachedSurface {
                                mesh: s.handle.clone(),
                                material: s.material.clone(),
                                section: s.source_section,
                            })
                            .collect(),
                    )
                }
                // One merged mesh: not animatable (no per-surface section).
                Ok(model) => model.material.clone().map(|m| {
                    vec![CachedSurface {
                        mesh: model.handle.clone(),
                        material: m,
                        section: usize::MAX,
                    }]
                }),
                Err(error) => {
                    warn!("Swarm model {group:08x}!{instance:08x}: {error}");
                    None
                }
            };
            surfaces.map(|surfaces| CachedModel { key, surfaces })
        });
        self.models.insert((group, instance), found.clone());
        found
    }
}

/// The creator's model group (`40606000`: `ce_*` models and their property lists).
const EDITOR_MODEL_GROUP: u32 = 0x4060_6000;

/// The creator backdrops' own materials: the `ce_*` models use the editor
/// background renderers (`0x80000003`, the UFO editor's panorama `0x80000033`)
/// whose diffuse the importer does not bind (the surfaces stayed the flat
/// debug colour); the same binding the creator room gets
/// (`editor_environment::import_room`).
fn bind_editor_background(
    originals: &crate::Originals,
    key: ResourceKey,
    model: &crate::OriginalModel,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Ok(bytes) = originals.resource_index.read(key) else { return };
    for surface in &model.surfaces {
        let Ok(Some(state)) = crate::material_binding::common_state(&bytes, &[surface.source_section]) else { continue };
        let Some(mut material) = materials.get_mut(&surface.material) else { continue };
        if material.base_color_texture.is_some() {
            continue;
        }
        let bound = match state.renderer_id {
            0x8000_0003 => crate::material_binding::apply_editor_background(&state, &bytes, images, &mut material),
            0x8000_0033 => crate::material_binding::apply_editor_panorama(&state, &bytes, images, &mut material),
            _ => Ok(()),
        };
        debug!(
            "creator backdrop model {:08x} surface {}: renderer {:08x}, {}",
            key.instance_id,
            surface.source_section,
            state.renderer_id,
            bound.map_or_else(|e| e, |_| "bound".to_string())
        );
    }
}

/// The RW4 mesh an effect's model key names: the RW4 itself (group, then
/// group 0), or the `modelMeshLOD0` of the model property list of that name.
/// Some effects name a model property list (`.prop`, 0x00B1B104) instead of
/// the mesh; its modelMeshLOD0 (0x00F9EFBB) is the mesh.
pub fn resolve_model_key(originals: &crate::Originals, group: u32, instance: u32) -> Option<ResourceKey> {
    resolve_model_key_in(&originals.resource_index, group, instance)
}

/// [`resolve_model_key`] on a bare resource index (tests, surveys).
pub fn resolve_model_key_in(
    index: &crate::asset_catalog::ResourceIndex,
    group: u32,
    instance: u32,
) -> Option<ResourceKey> {
    let groups: &[u32] = if group == 0 { &[0] } else { &[group, 0] };
    for &group_id in groups {
        let key = ResourceKey {
            type_id: RW4,
            group_id,
            instance_id: instance,
        };
        if index.records.contains_key(&key) {
            return Some(key);
        }
        let prop = ResourceKey {
            type_id: PROP,
            ..key
        };
        if let Some(mut mesh) = index
            .read(prop)
            .ok()
            .and_then(|bytes| crate::prop::parse(&bytes).ok())
            .and_then(|list| list.0.get(&MODEL_MESH_LOD0).and_then(|p| p.key()))
        {
            // Some lists leave the key's type unset (the mesh is an RW4).
            if mesh.type_id == 0 {
                mesh.type_id = RW4;
            }
            if index.records.contains_key(&mesh) {
                return Some(mesh);
            }
        }
    }
    None
}

struct ModelInstance {
    transform: Transform,
    alpha: f32,
    color: Vec3,
}

/// One owner's models to draw this frame.
struct Wanted {
    owner: u64,
    group: u32,
    model: u32,
    instances: Vec<ModelInstance>,
    attach: Option<Entity>,
    layer: usize,
    /// `gameModel` draws: lit like scene objects (the others are unlit).
    lit: bool,
    /// Marks the attached entity as drawn (hides stage stand-ins).
    marks_attached: bool,
    /// `gameModel` `animate` entries to play on the model's own skeleton.
    anim: Option<super::game_anim::AnimRequest>,
    /// Drawn this frame (a creator session hides the stage's effect models;
    /// the Test Drive backdrop's own models stay).
    shown: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw_models(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut cache: ResMut<SwarmModelCache>,
    mut attached: ResMut<SwarmAttachedDraws>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    stage: Option<Res<State<crate::stages::GameStage>>>,
    mut reported: Local<HashSet<u64>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&mut Transform, &mut GlobalTransform, &mut Visibility), With<SwarmModelDraw>>,
) {
    let Some(originals) = originals else { return };
    let hidden = editor.is_some_and(|e| e.open);
    // The creator session is `EditorState::open` too: its own effects' models
    // (the Test Drive backdrops, `game_model::set_editor_backdrop`) draw.
    let backdrop = super::game_model::editor_backdrop();
    let unbasis = AXES.transpose();
    let mut wanted: Vec<Wanted> = vec![];
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        let first = wanted.len();
        let shown = !hidden || (backdrop && effect.tag == crate::creature_editor::EFFECT_TAG);
        instance.for_each_model(&lib, &mut |m, def| {
            let Some(frame) = m.frame else { return };
            wanted.push(Wanted {
                owner: m.uid,
                group: def.resource.group,
                model: def.resource.instance,
                instances: vec![ModelInstance {
                    transform: Transform {
                        translation: frame.origin,
                        rotation: Quat::from_mat3(&(frame.rot * unbasis)),
                        scale: Vec3::splat(frame.scale * def.size.max(0.0) * m.mods.size),
                    },
                    alpha: (def.alpha * m.mods.alpha).clamp(0.0, 1.0),
                    color: Vec3::from_array(def.color) * m.mods.color,
                }],
                attach: None,
                layer: 0,
                lit: false,
                marks_attached: true,
                anim: None,
                shown,
            });
        });
        instance.for_each_game_model(&lib, &mut |g, def| {
            if !super::game_model::drawn(def, stage.as_ref().map(|s| s.get())) {
                if def.world != 0 && reported.insert(g.uid) {
                    debug!(
                        "Swarm gameModel {:08x} of world {:08x} (groups {:?}) is not drawn in stage {:?}",
                        def.model.instance,
                        def.world,
                        def.groups,
                        stage.as_ref().map(|s| s.get())
                    );
                }
                return;
            }
            let Some(frame) = g.frame else { return };
            wanted.push(Wanted {
                owner: g.uid,
                group: def.model.group,
                model: def.model.instance,
                instances: vec![ModelInstance {
                    transform: super::game_model::transform(def, &frame, g.mods.size, unbasis),
                    alpha: (def.alpha * g.mods.alpha).clamp(0.0, 1.0),
                    color: Vec3::from_array(def.color) * g.mods.color,
                }],
                attach: None,
                layer: 0,
                lit: true,
                marks_attached: false,
                anim: (!def.animations.is_empty()).then(|| super::game_anim::AnimRequest {
                    defs: def.animations.clone(),
                    age: g.age,
                    uid: g.uid,
                }),
                shown,
            });
        });
        instance.for_each_particles(&lib, &mut |sys, def| {
            if !def.has(pflag::MODEL) {
                return;
            }
            let Some(frame) = sys.frame else { return };
            let instances = sys
                .particles
                .iter()
                .map(|p| {
                    let t = (p.age / p.life).clamp(0.0, 1.0);
                    let size = curve(&def.size, t, 1.0) * (1.0 + def.size_vary * p.r[0]) * sys.mods.size * frame.scale;
                    let spin = (curve(&def.rotate, t, 0.0) + def.rotation_offset + def.rotation_vary * p.r[2]) * TAU;
                    let mut alpha = (curve(&def.alpha, t, 1.0) * (1.0 + def.alpha_vary * p.r[3])).clamp(0.0, 1.0)
                        * sys.mods.alpha;
                    if def.has(pflag::LOOP_BOX) {
                        alpha *= loop_box_fade(&frame, &def.emit_volume, &def.loop_box_alpha, p.pos);
                    }
                    ModelInstance {
                        transform: Transform {
                            translation: p.pos,
                            rotation: Quat::from_mat3(&(frame.rot * Mat3::from_rotation_z(spin) * unbasis)),
                            scale: Vec3::splat(size.max(0.0)),
                        },
                        alpha,
                        color: curve3(&def.color, t) * sys.mods.color,
                    }
                })
                .collect();
            wanted.push(Wanted {
                owner: sys.uid,
                group: def.texture.resource.group,
                model: def.texture.resource.instance,
                instances,
                attach: None,
                layer: 0,
                lit: false,
                marks_attached: true,
                anim: None,
                shown,
            });
        });
        for entry in &mut wanted[first..] {
            entry.attach = effect.attach_to;
            entry.layer = effect.layer;
        }
    }
    let cache = &mut *cache;
    let mut seen = HashSet::new();
    attached.0.clear();
    for Wanted {
        owner,
        group,
        model,
        instances,
        attach,
        layer,
        lit,
        marks_attached,
        anim,
        shown,
    } in wanted
    {
        seen.insert(owner);
        let Some(cached) = cache.model(&originals, &mut meshes, &mut images, &mut materials, group, model) else {
            if reported.insert(owner ^ 0x6D6F_6465_6C00_0000) {
                debug!("Swarm model {group:08x}!{model:08x} (owner {owner}) could not be loaded");
            }
            continue;
        };
        let surfaces = &cached.surfaces;
        // gameModel animation: the model's skeleton posed by the `animate`
        // entries (game_anim.rs); skinned into per-draw mesh copies below.
        let pose = anim.as_ref().and_then(|request| {
            let animated = cache.animated.get(&originals.resource_index, cached.key)?;
            let tracks = super::game_anim::tracks(&animated, request);
            Some((animated.matrices(&tracks), animated))
        });
        if let Some(entity) = attach.filter(|_| marks_attached && !instances.is_empty()) {
            attached.0.insert(entity);
        }
        if !cache.draws.contains_key(&owner) {
            // `RUST_LOG=openspore::swarm_fx::models=debug` shows where models appear.
            debug!(
                "Swarm model {group:08x}!{model:08x} (owner {owner}, {} surface(s), animated {}) first drawn at {:?} scale {:?} alpha {:?} color {:?} lit {lit}",
                surfaces.len(),
                pose.is_some(),
                instances.first().map(|i| i.transform.translation),
                instances.first().map(|i| i.transform.scale.x),
                instances.first().map(|i| i.alpha),
                instances.first().map(|i| i.color),
            );
            for surface in surfaces.iter() {
                if let Some(m) = materials.get(&surface.material) {
                    let bounds = meshes.get(&surface.mesh).and_then(|mesh| match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                        Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => Some(v.iter().fold(
                            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                            |(lo, hi), p| (lo.min(Vec3::from_array(*p)), hi.max(Vec3::from_array(*p))),
                        )),
                        _ => None,
                    });
                    debug!(
                        "  surface {}: color {:?} alpha_mode {:?} cull {:?} unlit {} texture {} bounds {:?}",
                        surface.section,
                        m.base_color.to_linear(),
                        m.alpha_mode,
                        m.cull_mode,
                        m.unlit,
                        m.base_color_texture.is_some(),
                        bounds
                    );
                }
            }
        }
        let pool = cache.draws.entry(owner).or_default();
        let needed = instances.len() * surfaces.len();
        while pool.len() > needed {
            let draw = pool.pop().unwrap();
            if let Ok(mut e) = commands.get_entity(draw.entity) {
                e.despawn();
            }
            if let Some(mesh) = draw.mesh {
                meshes.remove(&mesh);
            }
        }
        for (i, instance) in instances.iter().enumerate() {
            for (s, surface) in surfaces.iter().enumerate() {
                let (mesh, base) = (&surface.mesh, &surface.material);
                let slot = i * surfaces.len() + s;
                let visible = shown && instance.alpha > 0.002 && instance.transform.scale.x > 0.0;
                let visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                if slot >= pool.len() {
                    // Animated surfaces own a copy of the mesh to skin.
                    let own_mesh = pose
                        .as_ref()
                        .filter(|(_, a)| a.skins.contains_key(&surface.section))
                        .and_then(|_| meshes.get(mesh).cloned())
                        .map(|m| meshes.add(m));
                    let mut copy = materials.get(base).cloned().unwrap_or_default();
                    if !lit {
                        copy.alpha_mode = AlphaMode::Blend;
                        // ADAPTED: effect models show their authored texture
                        // colours (Swarm model lighting is not ported; scene
                        // lights saturate the pale Cell rock textures).
                        copy.unlit = true;
                    }
                    // gameModel (`lit`): the imported material and its alpha
                    // mode stay; fades switch to blending below.
                    let material = materials.add(copy);
                    let entity = commands
                        .spawn((
                            SwarmModelDraw,
                            // Placed by the effect renderer: `terrain::ground`
                            // must not lift it a second time.
                            super::render::SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(own_mesh.clone().unwrap_or_else(|| mesh.clone())),
                            MeshMaterial3d(material.clone()),
                            instance.transform,
                            GlobalTransform::from(instance.transform),
                            visibility,
                            NotShadowCaster,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    if own_mesh.is_some() {
                        // The skinned vertices leave the bind-pose bounds.
                        commands.entity(entity).insert(NoFrustumCulling);
                    }
                    pool.push(ModelDraw {
                        entity,
                        material,
                        layer,
                        mesh: own_mesh,
                    });
                }
                if pool[slot].layer != layer {
                    commands.entity(pool[slot].entity).try_insert(RenderLayers::layer(layer));
                    pool[slot].layer = layer;
                }
                let draw = &pool[slot];
                if let (Some((matrices, animated)), Some(handle)) = (&pose, &draw.mesh) {
                    if let (Some(skin), Some(mut target)) = (animated.skins.get(&surface.section), meshes.get_mut(handle)) {
                        super::game_anim::write_skinned(&mut target, skin, matrices);
                    }
                }
                if let Ok((mut t, mut g, mut v)) = draws.get_mut(draw.entity) {
                    *t = instance.transform;
                    *g = GlobalTransform::from(instance.transform);
                    *v = visibility;
                }
                let source = materials.get(base).map(|m| m.base_color.to_linear());
                let base_mode = materials.get(base).map(|m| m.alpha_mode);
                if let (Some(source), Some(mut m)) = (source, materials.get_mut(&draw.material)) {
                    let c = instance.color;
                    let tint = LinearRgba::new(
                        source.red * c.x,
                        source.green * c.y,
                        source.blue * c.z,
                        source.alpha * instance.alpha,
                    );
                    if m.base_color.to_linear() != tint {
                        m.base_color = tint.into();
                    }
                    if lit {
                        // Blend only while faded; the imported mode otherwise.
                        let mode = if instance.alpha < 0.995 {
                            AlphaMode::Blend
                        } else {
                            base_mode.unwrap_or(AlphaMode::Opaque)
                        };
                        if m.alpha_mode != mode {
                            m.alpha_mode = mode;
                        }
                    }
                }
            }
        }
    }
    cache.draws.retain(|owner, pool| {
        if seen.contains(owner) {
            return true;
        }
        for draw in pool.drain(..) {
            if let Ok(mut e) = commands.get_entity(draw.entity) {
                e.despawn();
            }
            if let Some(mesh) = draw.mesh {
                meshes.remove(&mesh);
            }
        }
        false
    });
}
