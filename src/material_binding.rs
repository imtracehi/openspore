//! Bridge for the evidenced StaticModel shader family (80000002/80000004).
//! The original compiled state remains authoritative for resource bindings.
//! Bevy PBR is a preview of that family, not a translated native shader.
use crate::{
    material_state::{self, MaterialState, TextureSlot},
    rw4, texture,
};
use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat},
};

#[derive(Component)]
pub struct OriginalSurfaceChild {
    pub source_section: usize,
    pub state_sections: Vec<usize>,
}
impl OriginalSurfaceChild {
    /// The marker of a surface its spawner draws on its own entity: such an
    /// entity is "a drawn surface, not a root", so [`attach_remaining`] does not
    /// take its mesh for a whole model's first surface and spawn the model's
    /// other surfaces again under it. Those copies carry no `RenderLayers`, so
    /// under a layered scene (HUD portrait, Creator) they were drawn by the
    /// main camera at the scene's position (the Cell HUD portrait's pedestal
    /// left its backdrop square over the water at the world origin).
    pub fn of(surface: &crate::OriginalSurface) -> Self {
        Self {
            source_section: surface.source_section,
            state_sections: surface.state_sections.clone(),
        }
    }
}
#[derive(Component)]
pub struct OriginalSurfaceRoot {
    pub source_section: usize,
    pub state_sections: Vec<usize>,
    first_mesh: AssetId<Mesh>,
    native_color: Color,
    children: Vec<(Handle<StandardMaterial>, Color)>,
}

fn relative_tint(native: Color, reference: Color, actual: Color) -> Color {
    let native = native.to_linear().to_f32_array();
    let reference = reference.to_linear().to_f32_array();
    let actual = actual.to_linear().to_f32_array();
    let value: [f32; 4] = std::array::from_fn(|i| {
        if (actual[i] - reference[i]).abs() < 1e-6 {
            native[i]
        } else if reference[i].abs() > 1e-6 {
            native[i] * actual[i] / reference[i]
        } else {
            actual[i]
        }
    });
    Color::linear_rgba(value[0], value[1], value[2], value[3])
}

/// Imported actors draw their first native surface on the actor entity. The
/// remaining surfaces inherit its complete transform as children, preserving
/// separate material identities, part mirroring and XML handle geometry.
pub fn attach_remaining(world: &mut World) {
    // Appearance changes replace native surface entities directly. Finish
    // attachment and its deferred writes together, before another system can
    // replace those entities and invalidate a queued OriginalSurfaceRoot.
    world
        .run_system_cached(attach_remaining_surfaces)
        .expect("attach original material surfaces");
}

fn attach_remaining_surfaces(
    mut commands: Commands,
    originals: Res<crate::Originals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&OriginalSurfaceRoot>,
            Option<&Children>,
        ),
        (Changed<Mesh3d>, Without<OriginalSurfaceChild>),
    >,
    surface_children: Query<(), With<OriginalSurfaceChild>>,
) {
    for (entity, mesh, material, previous, children) in &roots {
        if previous.is_some_and(|p| p.first_mesh == mesh.0.id()) {
            continue;
        }
        if let Some(children) = children {
            for child in children.iter() {
                if surface_children.contains(child) {
                    commands.entity(child).despawn();
                }
            }
        }
        let model = originals
            .models
            .iter()
            .chain(originals.mechanical_models.values())
            .chain(originals.world_models.values())
            .chain(originals.baked_models.values())
            .chain(originals.baked_body.iter().map(|b| &b.model))
            .chain(
                originals
                    .creation_models
                    .values()
                    .flat_map(|c| c.blocks.iter().map(|b| &b.mechanical.model)),
            )
            .find(|m| m.surfaces.first().is_some_and(|s| s.handle == mesh.0));
        let Some(model) = model else {
            if previous.is_some() {
                commands.entity(entity).remove::<OriginalSurfaceRoot>();
            }
            continue;
        };
        let first = &model.surfaces[0];
        let native_color = materials
            .get(&first.material)
            .map(|m| m.base_color)
            .unwrap_or(Color::WHITE);
        let actual = materials
            .get(&material.0)
            .map(|m| m.base_color)
            .unwrap_or(native_color);
        let mut child_materials = vec![];
        for surface in model.surfaces.iter().skip(1) {
            let Some(mut mat) = materials.get(&surface.material).cloned() else {
                continue;
            };
            let color = mat.base_color;
            mat.base_color = relative_tint(color, native_color, actual);
            let handle = materials.add(mat);
            let child = commands
                .spawn((
                    crate::stages::StageOwned,
                    OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(handle.clone()),
                    Transform::default(),
                ))
                .id();
            commands.entity(entity).add_child(child);
            child_materials.push((handle, color));
        }
        commands.entity(entity).insert(OriginalSurfaceRoot {
            source_section: first.source_section,
            state_sections: first.state_sections.clone(),
            first_mesh: mesh.0.id(),
            native_color,
            children: child_materials,
        });
    }
}

/// Per-instance recoloring must affect every surface without mutating the
/// exact-definition material cache shared by other actors.
pub fn sync_surface_tints(
    roots: Query<(&MeshMaterial3d<StandardMaterial>, &OriginalSurfaceRoot)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (material, root) in &roots {
        let Some(actual) = materials.get(&material.0).map(|m| m.base_color) else {
            continue;
        };
        for (handle, native) in &root.children {
            let color = relative_tint(*native, root.native_color, actual);
            if let Some(mut mat) = materials.get_mut(handle) {
                if mat.base_color != color {
                    mat.base_color = color;
                }
            }
        }
    }
}

/// Flattened geometry can use a native material only when every source mesh
/// has exactly the same single state. Distinct materials and native multiple
/// passes must be split before rendering; never choose the first state's data.
pub fn common_state(blob: &[u8], mesh_sections: &[usize]) -> Result<Option<MaterialState>, String> {
    let links = rw4::material_links(blob)?;
    let sections = rw4::sections(blob)?;
    let mut common = None;
    for &mesh in mesh_sections {
        let matches: Vec<_> = links.iter().filter(|l| l.mesh_section == mesh).collect();
        if matches.len() != 1 || matches[0].state_sections.len() != 1 {
            return Ok(None);
        }
        let state = matches[0].state_sections[0];
        if common.is_some_and(|s| s != state) {
            return Ok(None);
        }
        common = Some(state);
    }
    common
        .map(|i| material_state::parse(sections[i].1))
        .transpose()
}

fn state_value(state: &MaterialState, id: u32) -> Option<u32> {
    state
        .render_states
        .iter()
        .filter(|(group, _)| *group == 0)
        .flat_map(|(_, pairs)| pairs)
        .rev()
        .find(|(key, _)| *key == id)
        .map(|(_, value)| *value)
}
fn slot_value(slot: &TextureSlot, id: u32) -> Option<u32> {
    slot.sampler_states
        .iter()
        .rev()
        .find(|(key, _)| *key == id)
        .map(|(_, value)| *value)
}
fn sampler(slot: &TextureSlot) -> ImageSamplerDescriptor {
    let address = |id| match slot_value(slot, id).unwrap_or(1) {
        1 => ImageAddressMode::Repeat,
        2 => ImageAddressMode::MirrorRepeat,
        // Native border/mirror-once require further GPU capabilities. Keep
        // their original values in the state; this bridge clamps their edges.
        _ => ImageAddressMode::ClampToEdge,
    };
    let filter = |id| {
        if slot_value(slot, id).unwrap_or(1) == 1 {
            ImageFilterMode::Nearest
        } else {
            ImageFilterMode::Linear
        }
    };
    ImageSamplerDescriptor {
        address_mode_u: address(1),
        address_mode_v: address(2),
        address_mode_w: address(3),
        mag_filter: filter(5),
        min_filter: filter(6),
        mipmap_filter: filter(7),
        ..default()
    }
}
fn image(pixels: texture::Pixels, slot: &TextureSlot, linear: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.rgba,
        if linear || slot_value(slot, 11).unwrap_or(0) == 0 {
            TextureFormat::Rgba8Unorm
        } else {
            TextureFormat::Rgba8UnormSrgb
        },
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(sampler(slot));
    image
}
fn mip_pixels(blob: &[u8], slot: &TextureSlot) -> Option<Vec<texture::Pixels>> {
    if slot.raster_reference >> 22 != 0 {
        return None;
    }
    texture::decode_raster_mips(blob, slot.raster_reference as usize)
}

/// Upload the authored levels in native order. Minification must sample the
/// source mip chain rather than repeat the full-resolution raster at distance.
fn mip_image(mips: Vec<texture::Pixels>, slot: &TextureSlot, linear: bool) -> Option<Image> {
    let mut result = image(mips.first()?.clone(), slot, linear);
    result.texture_descriptor.mip_level_count = mips.len() as u32;
    result.data = Some(mips.into_iter().flat_map(|mip| mip.rgba).collect());
    Some(result)
}

/// Apply authored Z scaling to decoded tangent normals. Alpha is retained as
/// original specular metadata; no speculative roughness conversion is applied.
fn normal_pixels(
    mut pixels: texture::Pixels,
    inverse_bump: f32,
) -> Result<texture::Pixels, String> {
    if !inverse_bump.is_finite() || inverse_bump < 0. {
        return Err("invalid inverse bump parameter".into());
    }
    for p in pixels.rgba.chunks_exact_mut(4) {
        let n = Vec3::new(
            p[0] as f32 / 127.5 - 1.,
            p[1] as f32 / 127.5 - 1.,
            (p[2] as f32 / 127.5 - 1.) * inverse_bump,
        )
        .normalize_or_zero();
        if n == Vec3::ZERO {
            p[..3].copy_from_slice(&[128, 128, 255]);
        } else {
            for i in 0..3 {
                p[i] = ((n[i] * 0.5 + 0.5) * 255.).round().clamp(0., 255.) as u8;
            }
        }
    }
    Ok(pixels)
}

/// The creator backdrop uses 80000003 with an embedded sampler-0 raster and
/// no shader-data selectors. Preserve that exact binding in an explicitly
/// unlit preview; this does not execute the original background shader.
pub fn apply_editor_background(
    state: &MaterialState,
    blob: &[u8],
    images: &mut Assets<Image>,
    material: &mut StandardMaterial,
) -> Result<(), String> {
    if state.renderer_id != 0x80000003 || !state.shader_data.is_empty() {
        return Err("unsupported creator background material".into());
    }
    apply_background_diffuse(state, blob, images, material)
}

/// Bounded diffuse preview for the installed UFO creator panorama. Its
/// original 80000033 shader remains untranslated; this preserves the actual
/// embedded sampler-0 image and render states instead of a generic tint.
pub fn apply_editor_panorama(
    state: &MaterialState,
    blob: &[u8],
    images: &mut Assets<Image>,
    material: &mut StandardMaterial,
) -> Result<(), String> {
    if state.renderer_id != 0x80000033
        || state.shader_data.len() != 1
        || state.shader_data[0].index != 528
        || state.shader_data[0].offset != 0
        || state.shader_data[0].object_reference.is_some()
        || state.shader_data[0].words != [0x26445c02, 0, 0, 1_f32.to_bits(), 0]
    {
        return Err("unsupported creator panorama selector".into());
    }
    apply_background_diffuse(state, blob, images, material)
}

fn apply_background_diffuse(
    state: &MaterialState,
    blob: &[u8],
    images: &mut Assets<Image>,
    material: &mut StandardMaterial,
) -> Result<(), String> {
    let slot = state
        .texture_slots
        .iter()
        .find(|s| s.sampler == 0)
        .ok_or("creator background has no diffuse sampler")?;
    let mips =
        mip_pixels(blob, slot).ok_or("creator background diffuse raster mip chain missing")?;
    material.base_color_texture = Some(
        images
            .add(mip_image(mips, slot, false).ok_or("empty creator background diffuse mip chain")?),
    );
    material.base_color = state
        .material_color
        .map_or(Color::WHITE, |c| Color::linear_rgba(c[0], c[1], c[2], c[3]));
    material.unlit = true;
    material.cull_mode = match state_value(state, 22).unwrap_or(2) {
        1 => None,
        2 => Some(Face::Back),
        3 => Some(Face::Front),
        _ => return Err("invalid creator background culling mode".into()),
    };
    material.double_sided = material.cull_mode.is_none();
    material.alpha_mode = if state_value(state, 27) == Some(1) {
        AlphaMode::Blend
    } else if state_value(state, 15) == Some(1) && matches!(state_value(state, 25), Some(5 | 7)) {
        AlphaMode::Mask(state_value(state, 24).unwrap_or(0).min(255) as f32 / 255.)
    } else {
        AlphaMode::Opaque
    };
    Ok(())
}

/// `windowInteriorBuilder` (renderer 0x40000007): the interior mesh of the
/// building editor's window parts (28 rigblocks, e.g. `be_classic_13`).
pub const WINDOW_INTERIOR_RENDERER: u32 = 0x4000_0007;

/// VERIFIED (the builder's fragment chain in the compiled shader pack
/// 0x40212004 and the fragment source 0x40212002): `windowInteriorBuilder`
/// draws `copyVertexColor`, `applyInteriorVS`, `toClipSpace` + skinning / PS
/// `setupUV0`, `applyInteriorPS`: the embedded sampler-0 interior texture,
/// with no lighting fragment, so a building's windows keep their full colour
/// by night (the city's lit windows). ADAPTED: the view parallax of
/// `applyInteriorVS` (uv + 0.2 x the camera-space normal's right / up
/// components) is not applied. Returns false for other renderers or without
/// an embedded interior raster (the material is then left as it was).
pub fn apply_window_interior(
    state: &MaterialState,
    blob: &[u8],
    images: &mut Assets<Image>,
    material: &mut StandardMaterial,
) -> Result<bool, String> {
    if state.renderer_id != WINDOW_INTERIOR_RENDERER {
        return Ok(false);
    }
    let Some(slot) = state.texture_slots.iter().find(|s| s.sampler == 0) else {
        return Ok(false);
    };
    let Some(image) = mip_pixels(blob, slot).and_then(|mips| mip_image(mips, slot, false)) else {
        return Ok(false);
    };
    material.base_color_texture = Some(images.add(image));
    material.base_color = state
        .material_color
        .map_or(Color::WHITE, |c| Color::linear_rgba(c[0], c[1], c[2], c[3]));
    material.unlit = true;
    material.normal_map_texture = None;
    material.cull_mode = match state_value(state, 22).unwrap_or(2) {
        1 => None,
        3 => Some(Face::Front),
        _ => Some(Face::Back),
    };
    material.double_sided = material.cull_mode.is_none();
    material.alpha_mode = if state_value(state, 27) == Some(1) { AlphaMode::Blend } else { AlphaMode::Opaque };
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| info!("window interior bound: unlit sampler-0 interior raster (windowInteriorBuilder)"));
    Ok(true)
}

/// A window part embedding the shared interior raster (26 of the 28 window
/// parts embed the same 256 x 256 image; `be_classic_13`).
pub const WINDOW_INTERIOR_SOURCE: crate::dbpf::ResourceKey = crate::dbpf::ResourceKey {
    type_id: crate::asset_catalog::RW4,
    group_id: 0x4060_6000,
    instance_id: 0xCDE7_E5C6,
};

/// The window interior material (`apply_window_interior`) of the shared
/// interior raster, for models that carry window panes without their own
/// material (the installed baked buildings, `baked_model::import`).
pub fn window_interior_material(
    index: &crate::asset_catalog::ResourceIndex,
    images: &mut Assets<Image>,
) -> Option<StandardMaterial> {
    let blob = index.read(WINDOW_INTERIOR_SOURCE).ok()?;
    let sections = crate::rw4::sections(&blob).ok()?;
    for (kind, payload) in &sections {
        if *kind != crate::rw4::T_COMPILED_STATE {
            continue;
        }
        let Ok(state) = material_state::parse(payload) else { continue };
        let mut material = StandardMaterial::default();
        if apply_window_interior(&state, &blob, images, &mut material).ok()? {
            return Some(material);
        }
    }
    None
}

pub fn apply_static(
    state: &MaterialState,
    blob: &[u8],
    mesh: &mut Mesh,
    images: &mut Assets<Image>,
    material: &mut StandardMaterial,
    authored_skinpaint: bool,
) -> Result<bool, String> {
    let skinpaint = authored_skinpaint
        || state
            .shader_data
            .iter()
            .any(|d| d.index == 0x216 && d.words.len() == 1);
    if !matches!(state.renderer_id, 0x80000002 | 0x80000004)
        && !(state.renderer_id == 0x80000005 && skinpaint)
    {
        return Ok(false);
    }
    if let Some(color) = state.material_color {
        material.base_color = Color::linear_rgba(color[0], color[1], color[2], color[3]);
    }
    material.cull_mode = match state_value(state, 22).unwrap_or(2) {
        1 => None,
        2 => Some(Face::Back),
        3 => Some(Face::Front),
        _ => return Err("invalid original culling mode".into()),
    };
    material.double_sided = material.cull_mode.is_none();
    material.alpha_mode = if state_value(state, 27) == Some(1) {
        AlphaMode::Blend
    } else if state_value(state, 15) == Some(1) && matches!(state_value(state, 25), Some(5 | 7)) {
        AlphaMode::Mask(state_value(state, 24).unwrap_or(0).min(255) as f32 / 255.)
    } else {
        AlphaMode::Opaque
    };
    if let Some(slot) = state.texture_slots.iter().find(|s| s.sampler == 0) {
        if skinpaint && material.base_color_texture.is_some() {
            // The PROP's painted texture is authoritative for a part. Retail
            // geometry can also carry a neutral editor preview texture. Its
            // native UV sampler still applies to the composed texture.
            if let Some(mut painted) = material
                .base_color_texture
                .as_ref()
                .and_then(|h| images.get(h))
                .cloned()
            {
                painted.sampler = ImageSampler::Descriptor(sampler(slot));
                material.base_color_texture = Some(images.add(painted));
            }
        } else if let Some(mut mips) = mip_pixels(blob, slot) {
            // StaticModel's identity selector (0x218) uses diffuse alpha as
            // a paint mask and restores the output alpha. Feeding that mask
            // into PBR opacity made original hut walls disappear. Identity
            // tint itself remains a preview until shader execution exists.
            if state.shader_data.iter().any(|d| d.index == 0x218) {
                for mip in &mut mips {
                    for p in mip.rgba.chunks_exact_mut(4) {
                        p[3] = 255;
                    }
                }
            }
            material.base_color_texture =
                mip_image(mips, slot, false).map(|image| images.add(image));
        }
    }
    if let Some(slot) = state.texture_slots.iter().find(|s| s.sampler == 1) {
        if skinpaint {
            // The part's authored height map has already run through the
            // installed normal-conversion program. Keep the native draw sampler.
            if let Some(mut normal) = material
                .normal_map_texture
                .as_ref()
                .and_then(|h| images.get(h))
                .cloned()
            {
                normal.sampler = ImageSampler::Descriptor(sampler(slot));
                material.normal_map_texture = Some(images.add(normal));
            }
        } else if let Some(mips) = mip_pixels(blob, slot) {
            let inverse_bump = state
                .shader_data
                .iter()
                .find(|d| d.index == 0x210)
                .and_then(|d| d.words.get(2))
                .map(|w| f32::from_bits(*w))
                .unwrap_or(1.);
            let mips = mips
                .into_iter()
                .map(|pixels| normal_pixels(pixels, inverse_bump))
                .collect::<Result<Vec<_>, _>>()?;
            // UV/normal data must support tangents before allocating the map.
            if mesh.generate_tangents().is_ok() {
                material.normal_map_texture =
                    mip_image(mips, slot, true).map(|image| images.add(image));
                material.flip_normal_map_y = false;
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_surfaces_keep_materials_transforms_and_instance_paint_isolated() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<crate::Originals>()
            .add_systems(Update, (attach_remaining, sync_surface_tints).chain());
        let mesh0 = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1., 1., 1.));
        let mesh1 = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(2., 1., 1.));
        let base0 = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::WHITE,
                ..default()
            });
        let base1 = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::linear_rgba(0.5, 1., 0.25, 1.),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
        let instance = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::linear_rgba(1., 0.2, 0.4, 1.),
                ..default()
            });
        app.world_mut()
            .resource_mut::<crate::Originals>()
            .models
            .push(crate::OriginalModel {
                handle: mesh0.clone(),
                surfaces: vec![
                    crate::OriginalSurface {
                        handle: mesh0.clone(),
                        material: base0.clone(),
                        source_section: 4,
                        state_sections: vec![5],
                    },
                    crate::OriginalSurface {
                        handle: mesh1.clone(),
                        material: base1.clone(),
                        source_section: 8,
                        state_sections: vec![9],
                    },
                ],
                ..default()
            });
        let transform = Transform::from_xyz(3., 4., 5.).with_scale(Vec3::new(-2., 3., 4.));
        let actor = app
            .world_mut()
            .spawn((
                Mesh3d(mesh0.clone()),
                MeshMaterial3d(instance.clone()),
                transform,
            ))
            .id();
        app.update();
        let child = app.world().get::<Children>(actor).unwrap()[0];
        assert_eq!(app.world().get::<ChildOf>(child).unwrap().parent(), actor);
        assert_eq!(
            *app.world().get::<Transform>(child).unwrap(),
            Transform::default()
        );
        assert_eq!(*app.world().get::<Transform>(actor).unwrap(), transform);
        assert_eq!(app.world().get::<Mesh3d>(child).unwrap().0, mesh1);
        assert_eq!(
            app.world()
                .get::<OriginalSurfaceChild>(child)
                .unwrap()
                .source_section,
            8
        );
        let handle = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(child)
            .unwrap()
            .0
            .clone();
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(materials.get(&handle).unwrap().alpha_mode, AlphaMode::Blend);
        assert!(materials
            .get(&handle)
            .unwrap()
            .base_color
            .to_linear()
            .to_f32_array()
            .iter()
            .zip([0.5, 0.2, 0.1, 1.])
            .all(|(a, b)| (*a - b).abs() < 1e-6));
        assert_eq!(
            materials.get(&base1).unwrap().base_color,
            Color::linear_rgba(0.5, 1., 0.25, 1.)
        );
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .get_mut(&instance)
            .unwrap()
            .base_color = Color::WHITE;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .unwrap()
                .base_color,
            Color::linear_rgba(0.5, 1., 0.25, 1.)
        );
        assert_eq!(
            app.world().get::<Children>(actor).unwrap().len(),
            1,
            "no duplicate surfaces on subsequent frames"
        );
        app.world_mut().entity_mut(actor).despawn();
        assert!(
            app.world().get_entity(child).is_err(),
            "surface children must retire with their actor"
        );
    }
    #[test]
    fn tangent_normals_retain_alpha_and_authored_z_scale() {
        let p = texture::Pixels {
            width: 1,
            height: 1,
            rgba: vec![255, 128, 255, 43],
        };
        let n = normal_pixels(p.clone(), 2.).unwrap();
        assert_eq!(n.rgba[3], 43);
        assert!(n.rgba[2] > normal_pixels(p.clone(), 1.).unwrap().rgba[2]);
        assert!(normal_pixels(p.clone(), f32::NAN).is_err());
        assert!(normal_pixels(p, -1.).is_err());
    }
    #[test]
    fn native_sampler_addressing_filters_and_color_space() {
        let slot = TextureSlot {
            sampler: 0,
            raster_reference: 0,
            stage_mask: 0,
            stage_states: vec![],
            sampler_mask: 0,
            sampler_states: vec![(1, 1), (2, 2), (5, 2), (6, 1), (11, 1)],
        };
        let s = sampler(&slot);
        assert_eq!(s.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(s.address_mode_v, ImageAddressMode::MirrorRepeat);
        assert_eq!(s.mag_filter, ImageFilterMode::Linear);
        assert_eq!(s.min_filter, ImageFilterMode::Nearest);
        let p = texture::Pixels {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        };
        assert_eq!(
            image(p.clone(), &slot, false).texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(
            image(p, &slot, true).texture_descriptor.format,
            TextureFormat::Rgba8Unorm
        );
    }
}
