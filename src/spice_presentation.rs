//! Native raster and terrain-stamp presentation for Civilization spice nodes.
//!
//! `syrup_resource1.prop` is the installed “Geyser Resource Object”. It defines
//! a terrain stamp and a painted decal rather than a static LOD mesh. This
//! presentation adapts those exact installed inputs into a low rocky mound
//! with the authored crater decal; the gameplay `SpiceNode` remains the owner
//! of harvesting and clickability.
//!
//! A free geyser vents spice with the retail Swarm effect
//! `object_resource_geyser_lvl5` (the display strategy 0xE8FF80 picks the
//! level effect `object_resource_geyser_lvl0..5` by the geyser level, 5 by
//! default; only level 5 exists in the installed effect directories); a
//! claimed one carries its nation's derrick, the installed
//! `ci_spice_pumpstation` world object ringed in the owner's colour (a
//! primitive rig without the install), and plays `object_resource_geyser_capped`
//! (the same function chooses it when the geyser is capped). The effect's
//! colour parameter (`kParamColor`, 5) is the planet's spice colour. The old
//! adapted pink spheres are gone.

use crate::{
    dbpf::ResourceKey,
    game::{RunProgress, SpiceNode},
    persistence::WorldId,
    stages::{GameStage, StageOwned},
    OriginalModel, Originals,
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

/// Installed PROP source with localized description “Geyser Resource Object”.
pub const GEYSER_SOURCE: ResourceKey = ResourceKey {
    group_id: 0x4184_2000,
    instance_id: 0xA5AA_B70A,
    type_id: crate::asset_catalog::PROP,
};
/// `modelPaintDecalTexture` on `syrup_resource1.prop`: native cracked crater.
pub const GEYSER_DECAL: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x3813_E774,
    type_id: crate::asset_catalog::RW4,
};
/// `modelStampTexture` on `syrup_resource1.prop`: native radial height stamp.
pub const GEYSER_STAMP: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x745B_F41F,
    type_id: crate::asset_catalog::RW4,
};

/// The logical node remains available to harvesting, saves, and navigation.
/// Only its old prototype render components are replaced.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSpiceGeyser(pub ResourceKey);

#[derive(Component, Clone, Copy)]
pub(crate) struct SpiceGeyserAttempted;

/// Which exact authored raster drives this generated surface.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSpiceGeyserSurface(pub ResourceKey);

/// Native crater-decal binding exposed for runtime GPU/resource diagnostics.
#[derive(Component, Clone, Debug)]
pub struct NativeSpiceGeyserDecal {
    pub source: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: [u32; 2],
}

const RINGS: usize = 12;
const SEGMENTS: usize = 64;
const MOUND_RADIUS: f32 = 2.25;
const MOUND_HEIGHT: f32 = 0.48;

fn native_pixels(
    originals: &Originals,
    key: ResourceKey,
) -> Result<Vec<crate::texture::Pixels>, String> {
    let bytes = originals.resource_index.read(key)?;
    crate::texture::decode_rw4_mips(&bytes)
        .filter(|mips| !mips.is_empty())
        .ok_or_else(|| format!("native raster {key:?} did not decode as an RW4 raster"))
}

fn image_from_mips(mips: Vec<crate::texture::Pixels>) -> Image {
    let base = &mips[0];
    let mut image = Image::new(
        Extent3d {
            width: base.width,
            height: base.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        base.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|mip| mip.rgba).collect());
    image
}

fn stamp_height(pixels: &crate::texture::Pixels, uv: Vec2) -> f32 {
    let x = (uv.x.clamp(0., 1.) * (pixels.width - 1) as f32).round() as usize;
    let y = ((1. - uv.y.clamp(0., 1.)) * (pixels.height - 1) as f32).round() as usize;
    let offset = (y * pixels.width as usize + x) * 4;
    let rgb = &pixels.rgba[offset..offset + 3];
    let level = (0.2126 * rgb[0] as f32 + 0.7152 * rgb[1] as f32 + 0.0722 * rgb[2] as f32) / 255.;
    // The source is a grayscale radial stamp. Its edge falls to black; the
    // scaled height is an adaptation for this prototype's world units.
    level * MOUND_HEIGHT
}

fn stamp_mesh(pixels: &crate::texture::Pixels) -> Mesh {
    // One shared center avoids a ring of degenerate fan triangles and zero
    // normals at the summit.
    let mut positions = vec![[0., stamp_height(pixels, Vec2::splat(0.5)), 0.]];
    let mut uvs = vec![[0.5, 0.5]];
    for ring in 1..=RINGS {
        let radial = ring as f32 / RINGS as f32;
        for segment in 0..=SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let x = angle.cos() * radial;
            let z = angle.sin() * radial;
            let uv = Vec2::new(x * 0.5 + 0.5, z * 0.5 + 0.5);
            positions.push([x * MOUND_RADIUS, stamp_height(pixels, uv), z * MOUND_RADIUS]);
            uvs.push(uv.to_array());
        }
    }
    let mut indices = Vec::with_capacity(RINGS * SEGMENTS * 6);
    for segment in 0..SEGMENTS {
        indices.extend_from_slice(&[0, (segment + 2) as u32, (segment + 1) as u32]);
    }
    for ring in 0..RINGS - 1 {
        for segment in 0..SEGMENTS {
            let a = (1 + ring * (SEGMENTS + 1) + segment) as u32;
            let b = a + (SEGMENTS + 1) as u32;
            // Counter-clockwise when viewed from above.
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

/// Replace the Civilization prototype cylinder with native source-driven art.
/// Register in `GameSystems::Presentation` after Civilization setup and terrain
/// grounding. `SpiceNode` and its entity remain intact for the existing logic.
pub fn sync(
    mut commands: Commands,
    state: Res<State<GameStage>>,
    originals: Res<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    land: Option<Res<crate::terrain::LandSurface>>,
    nodes: Query<(Entity, &Transform), (With<SpiceNode>, Without<SpiceGeyserAttempted>)>,
) {
    if *state.get() != GameStage::Civilization || originals.resource_index.records.is_empty() {
        return;
    }
    let pending: Vec<_> = nodes.iter().collect();
    if pending.is_empty() {
        return;
    }

    let imported = (|| {
        let decal_mips = native_pixels(&originals, GEYSER_DECAL)?;
        let decal_dimensions = [decal_mips[0].width, decal_mips[0].height];
        let decal_image = images.add(image_from_mips(decal_mips));
        let stamp_pixels = native_pixels(&originals, GEYSER_STAMP)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("native stamp {GEYSER_STAMP:?} has no base mip"))?;
        let mesh = meshes.add(stamp_mesh(&stamp_pixels));
        let mound_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.35, 0.25),
            perceptual_roughness: 0.98,
            ..default()
        });
        let decal_material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(decal_image.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        Ok::<_, String>((
            mesh,
            mound_material,
            decal_material,
            decal_image,
            decal_dimensions,
        ))
    })();

    let (mound_mesh, mound_material, decal_material, decal_image, decal_dimensions) = match imported
    {
        Ok(assets) => assets,
        Err(error) => {
            warn!("native geyser presentation unavailable: {error}");
            for (entity, _) in pending {
                commands.entity(entity).insert(SpiceGeyserAttempted);
            }
            return;
        }
    };

    let land = land
        .as_deref()
        .filter(|s| s.stage == GameStage::Civilization);
    let globe = crate::civ_planet::Globe::from_surface(land);
    for (entity, transform) in pending {
        // Roots are the gameplay/click targets. Remove only their old render
        // primitives, then mount the adapted native surfaces beneath them.
        commands.entity(entity).remove::<Mesh3d>();
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((SpiceGeyserAttempted, NativeSpiceGeyser(GEYSER_SOURCE)));
        // The ground in the root's own frame (scaled, standing on the
        // planet's local up).
        let floor_y = if land.is_some() {
            globe.local_ground(transform, Vec2::ZERO)
        } else {
            -transform.translation.y / transform.scale.y.max(1e-6)
        };
        // Adapt the stamp to the actual local slope, rather than floating a
        // flat circular rim above hills. Each site keeps its own mesh copy.
        let mut fitted_mesh = meshes.get(&mound_mesh).unwrap().clone();
        if let (Some(_), Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices))) = (
            land,
            fitted_mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION),
        ) {
            for p in vertices {
                p[1] += globe.local_ground(transform, Vec2::new(p[0], p[2])) - floor_y;
            }
            fitted_mesh.compute_smooth_normals();
        }
        let site_mesh = meshes.add(fitted_mesh);
        let mound_transform = Transform::from_xyz(0., floor_y + 0.015, 0.);
        let decal_transform = Transform::from_xyz(0., floor_y + 0.025, 0.);
        commands.spawn((
            StageOwned,
            NativeSpiceGeyserSurface(GEYSER_STAMP),
            Name::new("native geyser stamp mound"),
            Mesh3d(site_mesh.clone()),
            MeshMaterial3d(mound_material.clone()),
            mound_transform,
            ChildOf(entity),
        ));
        commands.spawn((
            StageOwned,
            NativeSpiceGeyserSurface(GEYSER_DECAL),
            NativeSpiceGeyserDecal {
                source: GEYSER_DECAL,
                image: decal_image.clone(),
                dimensions: decal_dimensions,
            },
            Name::new("native geyser crater decal"),
            Mesh3d(site_mesh),
            MeshMaterial3d(decal_material.clone()),
            decal_transform,
            ChildOf(entity),
        ));
    }
}

/// Installed spice derrick world object (`ci_spice_pumpstation`).
pub const DERRICK_SOURCE: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x0472_8435,
    type_id: crate::asset_catalog::PROP,
};
const DERRICK_NAME: &str = "ci_spice_pumpstation";
const DERRICK_WIDTH: f32 = 2.6;
/// Retail geyser effects (0xE8FF80; ids 0x16DD36C1 and 0x66DBFED1).
const GEYSER_FREE_EFFECT: &str = "object_resource_geyser_lvl5";
const GEYSER_CAPPED_EFFECT: &str = "object_resource_geyser_capped";
/// `SwarmPlay` tags of the geyser effects (low bits: the geyser index);
/// `civ_fx` uses 0xC1F0..0xC1F4.
const TAG_GEYSER: u64 = 0xC1F5_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// A drawn derrick on geyser `geyser` (WorldId 100 + index) for a nation.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerrickVisual {
    pub geyser: usize,
    pub owner: usize,
}

/// The spice plume over a geyser: the entity the retail Swarm effect is
/// attached to; `claimed` tells which of the two effects plays.
#[derive(Component, Clone, Copy, Debug)]
pub struct GeyserPlume {
    pub geyser: usize,
    pub claimed: bool,
}

/// Shared spice-plume and derrick materials.
#[derive(Default)]
pub struct DerrickArt {
    model: Option<Option<OriginalModel>>,
    rig: Option<(Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>)>,
    collar: Option<Handle<Mesh>>,
    nations: HashMap<usize, Handle<StandardMaterial>>,
}

fn geyser_index(id: &WorldId) -> Option<usize> {
    (id.0 >= 100)
        .then(|| (id.0 - 100) as usize)
        .filter(|g| *g < crate::civ::GEYSER_COUNT)
}

/// Keep every geyser's plume and derrick in step with the campaign: a
/// derrick appears in its builder's colours the moment it is built and
/// vanishes when it is destroyed.
#[allow(clippy::too_many_arguments)]
pub fn sync_derricks(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    land: Option<Res<crate::terrain::LandSurface>>,
    nodes: Query<(Entity, &WorldId, &Transform), With<SpiceNode>>,
    derricks: Query<(Entity, &DerrickVisual)>,
    plumes: Query<(Entity, &GeyserPlume)>,
    mut plays: MessageWriter<crate::swarm_fx::SwarmPlay>,
    mut stops: MessageWriter<crate::swarm_fx::SwarmStop>,
    mut art: Local<DerrickArt>,
) {
    let model = art
        .model
        .get_or_insert_with(|| {
            if originals.resource_index.records.is_empty() {
                return None;
            }
            crate::world_objects::definition_at(&originals, DERRICK_SOURCE, DERRICK_NAME)
                .and_then(|d| {
                    crate::world_objects::import_at(
                        &mut originals,
                        d,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                    )
                })
                .map_err(|e| warn!("spice derrick {DERRICK_NAME}: {e}"))
                .ok()
                .map(|(model, _)| model)
                .filter(|m| !m.surfaces.is_empty())
        })
        .clone();
    let plume_of: HashMap<usize, (Entity, bool)> = plumes.iter().map(|(e, p)| (p.geyser, (e, p.claimed))).collect();
    let mut shown: HashMap<usize, usize> = HashMap::new();
    for (entity, derrick) in &derricks {
        let owner = run
            .civ_campaign
            .geysers
            .get(derrick.geyser)
            .and_then(|g| g.owner);
        if owner != Some(derrick.owner) {
            commands.entity(entity).despawn();
        } else {
            shown.insert(derrick.geyser, derrick.owner);
        }
    }
    for (node, id, transform) in &nodes {
        let Some(g) = geyser_index(id) else {
            continue;
        };
        let floor = crate::civ_planet::Globe::from_surface(
            land.as_deref()
                .filter(|s| s.stage == crate::stages::GameStage::Civilization),
        )
        .local_ground(transform, Vec2::ZERO);
        // The retail effect on the geyser (0xE8FF80): the free level effect,
        // or the capped one once a derrick stands on it; switching stops the
        // old one (its smoke finishes) and starts the new.
        let claimed = run.civ_campaign.geysers.get(g).is_some_and(|s| s.owner.is_some());
        let effect_of = |claimed: bool| {
            crate::world_objects::name_id(if claimed { GEYSER_CAPPED_EFFECT } else { GEYSER_FREE_EFFECT })
        };
        // A tag per state (and the effect id in the stop), so the stop of the
        // old effect cannot reach the one started in the same frame.
        let tag_of = |claimed: bool| TAG_GEYSER | (g as u64) << 8 | claimed as u64;
        let plume = match plume_of.get(&g) {
            Some((_, was)) if *was == claimed => None,
            Some((entity, was)) => {
                stops.write(crate::swarm_fx::SwarmStop {
                    effect: effect_of(*was),
                    attach_to: Some(*entity),
                    tag: tag_of(*was),
                    hard: false,
                });
                commands.entity(*entity).insert(GeyserPlume { geyser: g, claimed });
                Some(*entity)
            }
            None => Some(
                commands
                    .spawn((
                        StageOwned,
                        GeyserPlume { geyser: g, claimed },
                        Name::new(format!("spice plume {g}")),
                        Transform::from_xyz(0.0, floor + 0.2, 0.0),
                        Visibility::Inherited,
                        ChildOf(node),
                    ))
                    .id(),
            ),
        };
        if let Some(plume) = plume {
            let mut play = crate::swarm_fx::SwarmPlay::at(effect_of(claimed), Vec3::ZERO);
            play.attach_to = Some(plume);
            play.tag = tag_of(claimed);
            // Retail units: undo the node's own scale (the frame multiplies it).
            play.scale = 1.0 / transform.scale.max_element().max(1e-6);
            plays.write(play);
        }
        let Some(owner) = run.civ_campaign.geysers.get(g).and_then(|s| s.owner) else {
            continue;
        };
        if shown.get(&g) == Some(&owner) {
            continue;
        }
        let collar_material = art
            .nations
            .entry(owner)
            .or_insert_with(|| materials.add(crate::civ::nation_material(owner)))
            .clone();
        let collar = art
            .collar
            .get_or_insert_with(|| meshes.add(Torus::new(0.95, 1.3)))
            .clone();
        let root = commands
            .spawn((
                StageOwned,
                DerrickVisual { geyser: g, owner },
                Name::new(format!("spice derrick {g}")),
                Transform::from_xyz(0.0, floor + 0.2, 0.0),
                Visibility::Inherited,
                ChildOf(node),
            ))
            .id();
        commands.spawn((
            StageOwned,
            Mesh3d(collar),
            MeshMaterial3d(collar_material.clone()),
            Transform::from_xyz(0.0, 0.12, 0.0).with_scale(Vec3::new(1.0, 0.35, 1.0)),
            ChildOf(root),
        ));
        match model.as_ref().and_then(|m| {
            crate::world_objects::fitted_transform(m, &meshes, Vec3::ZERO, DERRICK_WIDTH).ok()
        }) {
            Some(fit) => {
                for surface in &model.as_ref().unwrap().surfaces {
                    commands.spawn((
                        StageOwned,
                        crate::material_binding::OriginalSurfaceChild {
                            source_section: surface.source_section,
                            state_sections: surface.state_sections.clone(),
                        },
                        Mesh3d(surface.handle.clone()),
                        MeshMaterial3d(surface.material.clone()),
                        fit,
                        ChildOf(root),
                    ));
                }
            }
            None => {
                // No install: a lattice rig with a pump head.
                let (leg, head, steel) = art
                    .rig
                    .get_or_insert_with(|| {
                        (
                            meshes.add(Cylinder::new(0.08, 2.6)),
                            meshes.add(Cuboid::new(0.9, 0.5, 0.9)),
                            materials.add(StandardMaterial {
                                base_color: Color::srgb(0.45, 0.45, 0.48),
                                perceptual_roughness: 0.5,
                                metallic: 0.4,
                                ..default()
                            }),
                        )
                    })
                    .clone();
                for corner in 0..4 {
                    let a =
                        corner as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
                    commands.spawn((
                        StageOwned,
                        Mesh3d(leg.clone()),
                        MeshMaterial3d(steel.clone()),
                        Transform::from_xyz(a.cos() * 0.55, 1.25, a.sin() * 0.55).with_rotation(
                            Quat::from_rotation_z(-a.cos() * 0.15)
                                * Quat::from_rotation_x(a.sin() * 0.15),
                        ),
                        ChildOf(root),
                    ));
                }
                commands.spawn((
                    StageOwned,
                    Mesh3d(head),
                    MeshMaterial3d(collar_material),
                    Transform::from_xyz(0.0, 2.6, 0.0),
                    ChildOf(root),
                ));
            }
        }
    }
}

/// The geyser effects carry the planet's spice colour (retail: effect
/// parameter 5 set from `cPlanet +0x1C4`, 0xE8FF80). ADAPTED: the Civ
/// homeworld's spice is red (`SpiceColor::for_world(0)`, whose RGB is
/// OpenSpore's own choice).
pub fn animate_plumes(mut effects: Query<&mut crate::swarm_fx::SwarmEffect>) {
    let tint = Vec3::from_array(crate::space::SpiceColor::for_world(0).rgb());
    for mut effect in &mut effects {
        if effect.tag & TAG_MASK == TAG_GEYSER && effect.tint != tint {
            effect.tint = tint;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// The retail geyser effects are installed and carry Civ blocks (app flag
    /// 0x800); the other level effects are not installed (see
    /// `notes\swarm-civ-notes.md`).
    #[test]
    #[ignore = "requires installed Spore resource packages"]
    fn retail_geyser_effects_are_installed() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = crate::swarm_fx::SwarmLibrary::load(&index);
        for name in [GEYSER_FREE_EFFECT, GEYSER_CAPPED_EFFECT] {
            assert!(lib.find(crate::world_objects::name_id(name)).is_some(), "{name}");
        }
        for level in 0..5 {
            let name = format!("object_resource_geyser_lvl{level}");
            assert!(lib.find(crate::world_objects::name_id(&name)).is_none(), "{name} is not installed");
        }
    }

    #[test]
    fn geyser_tags_are_distinct_from_the_other_civ_effects() {
        assert_eq!(TAG_GEYSER & TAG_MASK, TAG_GEYSER);
        for tag in [0xC1F0u64, 0xC1F1, 0xC1F2, 0xC1F3, 0xC1F4, 0xC1F6] {
            assert_ne!(TAG_GEYSER >> 48, tag);
        }
    }

    #[test]
    fn geyser_world_ids_map_to_campaign_slots() {
        assert_eq!(geyser_index(&WorldId(100)), Some(0));
        assert_eq!(
            geyser_index(&WorldId(100 + crate::civ::GEYSER_COUNT as u32 - 1)),
            Some(crate::civ::GEYSER_COUNT - 1)
        );
        assert_eq!(geyser_index(&WorldId(99)), None);
        assert_eq!(
            geyser_index(&WorldId(100 + crate::civ::GEYSER_COUNT as u32)),
            None
        );
        assert_eq!(
            crate::world_objects::name_id(DERRICK_NAME),
            DERRICK_SOURCE.instance_id
        );
    }

    #[test]
    fn stamp_mesh_has_finite_normals_and_bounded_uvs() {
        let pixels = crate::texture::Pixels {
            width: 2,
            height: 2,
            rgba: vec![
                0, 0, 0, 255, 128, 128, 128, 255, 128, 128, 128, 255, 255, 255, 255, 255,
            ],
        };
        let mesh = stamp_mesh(&pixels);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("stamp mesh has no positions");
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("stamp mesh has no computed normals");
        };
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("stamp mesh has no UVs");
        };
        assert!(positions.iter().flatten().all(|v| v.is_finite()));
        assert!(normals.iter().flatten().all(|v| v.is_finite()));
        assert!(normals.iter().all(|n| Vec3::from_array(*n).length() > 0.9));
        assert!(uvs
            .iter()
            .flatten()
            .all(|uv| uv.is_finite() && (0.0..=1.0).contains(uv)));
    }

    /// This assertion intentionally requires the user's installed Spore files.
    #[test]
    #[ignore = "requires installed Spore resource packages"]
    fn installed_geyser_prop_resolves_native_decal_and_stamp() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        // The child's parent key names absent group 00000000. Local decal and
        // stamp fields still decode; do not silently substitute a same-ID
        // parent from a different group.
        let properties = crate::prop::parse(&index.read(GEYSER_SOURCE).expect("geyser PROP bytes"))
            .expect("geyser local PROP");
        let decal = properties
            .0
            .get(&0x03AB_C381)
            .and_then(crate::prop::Property::key)
            .expect("modelPaintDecalTexture");
        let stamp = properties
            .0
            .get(&0x03B2_8C1B)
            .and_then(crate::prop::Property::key)
            .expect("modelStampTexture");
        assert_eq!(decal.type_id, 0, "native PROP omits the extension");
        assert_eq!(stamp.type_id, 0, "native PROP omits the extension");
        assert_eq!(
            ResourceKey {
                type_id: crate::asset_catalog::RW4,
                ..decal
            },
            GEYSER_DECAL
        );
        assert_eq!(
            ResourceKey {
                type_id: crate::asset_catalog::RW4,
                ..stamp
            },
            GEYSER_STAMP
        );
        let decal_bytes = index.read(GEYSER_DECAL).expect("geyser decal bytes");
        let decal_mips =
            crate::texture::decode_rw4_mips(&decal_bytes).expect("geyser decal raster");
        assert!(!decal_mips.is_empty());
        let stamp_bytes = index.read(GEYSER_STAMP).expect("geyser stamp bytes");
        let stamp_pixels = crate::texture::decode_rw4(&stamp_bytes).expect("geyser stamp raster");
        let mesh = stamp_mesh(&stamp_pixels);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
    }
}
