//! Native static perimeter art, authored city buildings and wall turrets for
//! Civilization cities.
//!
//! Walls follow the city's type like Spore's city styles: military cities
//! raise `ci_castle_citywalls_<size>`, religious ones `ci_shrine_citywalls_<size>`
//! and economic ones `ci_diplo_citywalls_<size>` (installed PROPs with decoded
//! static RW4 meshes), the player's city behind the large walls and every
//! rival city behind the small ones (`civ::CityState::walls_large`, RETAIL
//! 0xBE7D40 / 0xBED5B0). The buildings, wall guns and civic objects stand on
//! the authored slots of those PROPs (`city_layout`: `City_Hall`, `Buildings`,
//! `Turrets`, `Decorations`), in the model's own frame at the one scale
//! `city_layout::LAYOUT_SCALE`. Their wall guns are `ci_castle_gun` /
//! `ci_old_gun` / `ci_diplo_gun`, swapped for the matching `_destroyed` model
//! when a turret falls.
//!
//! Buildings are verified authored models selected by their `.bem`
//! `ModelType` (SDK `CommonIDs.h`): city hall `0x99E92F05`, house
//! `0x4E3F7777`, industry `0x47C10953`, and entertainment `0x72C49181`. The
//! city hall source is used for the integrated central hall only; the authored
//! building slots around it show exactly the houses, factories and
//! entertainment buildings saved in `civ::CityState::plots`. Meshes and baked
//! atlases are genuine original geometry; the slots' places are the walls
//! model's own data, the buildings' fitted width and facing (outward), the
//! hall's place on the root axis and the grounding are adapted.

use crate::{
    asset_catalog::PROP,
    baked_model,
    civ::{Approach, Building, CityMarker},
    dbpf::ResourceKey,
    decode,
    game::RunProgress,
    material_binding,
    stages::{GameStage, StageOwned},
    OriginalModel, Originals,
};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use std::collections::HashMap;

/// VERIFIED (`CityGameGeneralTuning` `CityWallSinkDepth` 0.2, a world-unit
/// depth): the walls stand this far below the ground, which hides the seam on
/// uneven terrain. The city root is drawn `CITY_SCALE` times larger, so the
/// depth is divided by it in the city's own frame.
pub const CITY_WALL_SINK_DEPTH: f32 = 0.2;
/// Setup places city roots at `city_position + Y * CITY_Y`; this is the
/// Civilization ground baseline when no generated terrain is available.
pub const CITY_ROOT_CLEARANCE: f32 = 2.0;
const TURRET_WIDTH: f32 = 1.7;

/// Complete baked building GMDL group.
pub const CITY_DISTRICT_GROUP: u32 = 0x4063_7100;
/// Category-verified authored building instances from the building group above.
pub const CITY_DISTRICT_CITY_HALL_INSTANCE: u32 = 0x072C_092C;
pub const CITY_DISTRICT_HOUSE_INSTANCE: u32 = 0x072F_A9BA;
pub const CITY_DISTRICT_INDUSTRY_INSTANCE: u32 = 0x072F_A9B5;
pub const CITY_DISTRICT_ENTERTAINMENT_INSTANCE: u32 = 0x072F_A9BB;

/// Exact native source keys for every verified category.
pub const CITY_DISTRICT_CITY_HALL_SOURCE: ResourceKey = ResourceKey {
    group_id: CITY_DISTRICT_GROUP,
    instance_id: CITY_DISTRICT_CITY_HALL_INSTANCE,
    type_id: decode::TYPE_GMDL,
};
pub const CITY_DISTRICT_HOUSE_SOURCE: ResourceKey = ResourceKey {
    group_id: CITY_DISTRICT_GROUP,
    instance_id: CITY_DISTRICT_HOUSE_INSTANCE,
    type_id: decode::TYPE_GMDL,
};
pub const CITY_DISTRICT_INDUSTRY_SOURCE: ResourceKey = ResourceKey {
    group_id: CITY_DISTRICT_GROUP,
    instance_id: CITY_DISTRICT_INDUSTRY_INSTANCE,
    type_id: decode::TYPE_GMDL,
};
pub const CITY_DISTRICT_ENTERTAINMENT_SOURCE: ResourceKey = ResourceKey {
    group_id: CITY_DISTRICT_GROUP,
    instance_id: CITY_DISTRICT_ENTERTAINMENT_INSTANCE,
    type_id: decode::TYPE_GMDL,
};

/// All category-verified source keys, retained for evidence tests and helpers.
pub const CITY_DISTRICT_CATEGORY_SOURCES: &[ResourceKey] = &[
    CITY_DISTRICT_CITY_HALL_SOURCE,
    CITY_DISTRICT_HOUSE_SOURCE,
    CITY_DISTRICT_INDUSTRY_SOURCE,
    CITY_DISTRICT_ENTERTAINMENT_SOURCE,
];

/// Plot sources. The city hall is intentionally excluded because the root
/// integration mounts that model at the central city hall plot.
pub const CITY_DISTRICT_PLACEMENT_SOURCES: &[ResourceKey] = &[
    CITY_DISTRICT_HOUSE_SOURCE,
    CITY_DISTRICT_INDUSTRY_SOURCE,
    CITY_DISTRICT_ENTERTAINMENT_SOURCE,
];

/// Adapted plot footprint. The default city hall footprint is 3.5 units.
pub const CITY_DISTRICT_WIDTH: f32 = 2.3;
pub const CITY_CENTRAL_HALL_FOOTPRINT: f32 = 3.5;

/// Marker on the imported native draw surfaces, separate from city gameplay
/// and creation-assembly markers.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeCityWallSurface(pub ResourceKey);

/// Marker on imported native authored building surfaces. The carried key is
/// the native source definition for that surface.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeCityDistrictSurface(pub ResourceKey);

/// Cosmetic wall model currently mounted on a city.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityWallPresentation(pub ResourceKey);

/// The wall style (city type, large walls) a city currently shows; walls are
/// rebuilt when the capital's doctrine changes. Present even when the install
/// is missing, so the import is attempted once per style.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityWallStyle(pub Approach, pub bool);

/// First building source mounted on a city.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityDistrictPresentation(pub ResourceKey);

/// One drawn piece of a ring plot's building (a direct child of the city).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CityPlotVisual {
    pub city: usize,
    pub plot: usize,
    pub kind: Building,
    /// The player's design of this building kind that is drawn
    /// (`rigid_editor::plot_design_signature`; 0 = the native building).
    pub design: u32,
    /// The plot's authored site in the city's XZ plane (`city_layout`).
    pub site: Vec2,
}

/// A wall turret's pivot (child of the city): it turns toward its target.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CityTurretVisual {
    pub city: usize,
    pub turret: usize,
    pub alive: bool,
    pub kind: Approach,
}

/// The installed walls of a city type and size (`large`: the player's city;
/// RETAIL `InlandModelSmall` / `InlandModelLarge`, 0x44449BA / 0x44449BB).
pub fn wall_source(kind: Approach, large: bool) -> (ResourceKey, &'static str) {
    let layout = crate::city_layout::wall_layout(kind, large);
    (
        ResourceKey {
            group_id: 0x4082_8100,
            instance_id: layout.instance,
            type_id: PROP,
        },
        layout.name,
    )
}

/// The installed wall gun for a city type, standing or destroyed.
pub fn gun_source(kind: Approach, destroyed: bool) -> (ResourceKey, &'static str) {
    let (instance, name) = match (kind, destroyed) {
        (Approach::Military, false) => (0xA783_9343, "ci_castle_gun"),
        (Approach::Military, true) => (0x9BFC_D95F, "ci_castle_gun_destroyed"),
        (Approach::Religious, false) => (0x0207_441C, "ci_old_gun"),
        (Approach::Religious, true) => (0x68EF_C2D0, "ci_old_gun_destroyed"),
        (Approach::Economic, false) => (0xE117_D223, "ci_diplo_gun"),
        (Approach::Economic, true) => (0x9C7A_C9FF, "ci_diplo_gun_destroyed"),
    };
    (
        ResourceKey {
            group_id: 0x4082_8100,
            instance_id: instance,
            type_id: PROP,
        },
        name,
    )
}

fn source_for(kind: Building) -> ResourceKey {
    match kind {
        Building::House => CITY_DISTRICT_HOUSE_SOURCE,
        Building::Factory => CITY_DISTRICT_INDUSTRY_SOURCE,
        Building::Entertainment => CITY_DISTRICT_ENTERTAINMENT_SOURCE,
    }
}

/// The walls model at the layout scale with its authored origin on the city
/// root's axis and its ground plane (model z = 0, where the `Buildings` stand
/// and the `Turrets` mount above) on the city floor, `CityWallSinkDepth` under
/// it. The wall's foundations reach 4 to 15 model units below that plane (the
/// meshes span y -15..6): they hide the terrain's unevenness, as the earlier
/// "lowest vertex on the floor" fit could not (it raised the walls clear of
/// the buildings and the turret mounts).
fn fit_wall(model: &OriginalModel, meshes: &Assets<Mesh>) -> Result<Transform, String> {
    let (min, max) = mesh_bounds(model, meshes)?;
    if !(max - min).is_finite() || (max.x - min.x).max(max.z - min.z) <= 0.0 {
        return Err("invalid wall bounds".into());
    }
    let floor = -CITY_ROOT_CLEARANCE - CITY_WALL_SINK_DEPTH / crate::civ_planet::CITY_SCALE;
    Ok(Transform::from_translation(Vec3::new(0.0, floor, 0.0))
        .with_scale(Vec3::splat(crate::city_layout::LAYOUT_SCALE)))
}

/// Fit geometry from its actual source bounds, centred horizontally on the
/// requested point and turned by `yaw` about that point. Invalid or missing
/// bounds are an error, and there is no arbitrary scale fallback.
fn centered_fit_transform(
    min: Vec3,
    max: Vec3,
    position: Vec3,
    width: f32,
    yaw: f32,
) -> Result<Transform, String> {
    let span_x = max.x - min.x;
    let span_y = max.y - min.y;
    let span_z = max.z - min.z;
    // Keep tall source houses within the plot's overall presentation size.
    let span = span_x.max(span_y).max(span_z);
    if !span.is_finite()
        || span <= 0.0
        || !width.is_finite()
        || width <= 0.0
        || !min.is_finite()
        || !max.is_finite()
        || span_x <= 0.0
        || span_y <= 0.0
        || span_z <= 0.0
    {
        return Err("invalid source bounds".into());
    }
    let scale = width / span;
    let rotation = Quat::from_rotation_y(yaw);
    let anchor = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5) * scale;
    Ok(Transform::from_translation(position - rotation * anchor)
        .with_rotation(rotation)
        .with_scale(Vec3::splat(scale)))
}

fn mesh_bounds(model: &OriginalModel, meshes: &Assets<Mesh>) -> Result<(Vec3, Vec3), String> {
    let mesh = meshes
        .get(&model.handle)
        .ok_or("missing original object mesh")?;
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("missing original object positions".into());
    };
    if vertices.is_empty() {
        return Err("source has no vertices".into());
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for v in vertices {
        let v = Vec3::from_array(*v);
        min = min.min(v);
        max = max.max(v);
    }
    Ok((min, max))
}

fn district_transform(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
    position: Vec3,
    width: f32,
    yaw: f32,
) -> Result<Transform, String> {
    let (min, max) = mesh_bounds(model, meshes)?;
    centered_fit_transform(min, max, position, width, yaw)
}

/// Fit the category-verified city hall source at the city center.
pub fn fit_central_city_hall(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
    city_transform: Option<&Transform>,
    land: Option<&crate::terrain::LandSurface>,
) -> Result<Transform, String> {
    // The floor under the city's centre in the root's own frame (the root is
    // scaled by `civ_planet::CITY_SCALE` and stands on the planet's local up).
    let floor_local = match (city_transform, land) {
        (Some(transform), Some(land)) if land.stage == GameStage::Civilization => {
            crate::civ_planet::Globe::from_surface(Some(land)).local_ground(transform, Vec2::ZERO)
        }
        _ => -CITY_ROOT_CLEARANCE,
    };
    district_transform(
        model,
        meshes,
        Vec3::new(0.0, floor_local, 0.0),
        CITY_CENTRAL_HALL_FOOTPRINT,
        0.0,
    )
}

pub fn district_source_keys() -> &'static [ResourceKey] {
    CITY_DISTRICT_PLACEMENT_SOURCES
}

pub fn category_source_keys() -> &'static [ResourceKey] {
    CITY_DISTRICT_CATEGORY_SOURCES
}

/// Align a newly mounted hall to the same sampled city floor as its perimeter.
pub fn ground_halls(
    land: Option<Res<crate::terrain::LandSurface>>,
    originals: Res<Originals>,
    meshes: Res<Assets<Mesh>>,
    cities: Query<&Transform, (With<CityMarker>, Without<crate::civ::NativeCityHallSurface>)>,
    mut halls: Query<
        (&crate::civ::NativeCityHallSurface, &ChildOf, &mut Transform),
        (
            Added<crate::civ::NativeCityHallSurface>,
            Without<CityMarker>,
        ),
    >,
) {
    for (source, parent, mut transform) in &mut halls {
        let Some(model) = originals.baked_models.get(&source.0) else {
            continue;
        };
        let Ok(city) = cities.get(parent.parent()) else {
            continue;
        };
        if let Ok(fit) = fit_central_city_hall(model, &meshes, Some(city), land.as_deref()) {
            *transform = fit;
        }
    }
}

/// Import an installed world object (walls, guns) once; failures are cached.
fn world_object(
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    key: ResourceKey,
    name: &str,
) -> Result<(OriginalModel, crate::world_objects::OriginalWorldObject), String> {
    if originals.resource_index.records.is_empty() {
        return Err("no install".into());
    }
    let definition = crate::world_objects::definition_at(originals, key, name)?;
    let (model, source) =
        crate::world_objects::import_at(originals, definition, meshes, images, materials)?;
    if model.surfaces.is_empty() {
        return Err("source has no draw surfaces".into());
    }
    Ok((model, source))
}

/// Mount each city's wall set by its type, and rebuild it when the type
/// changes (the capital follows your doctrine). Register in Civilization
/// `Presentation` after city setup. Roots, ownership, skyline assemblies and
/// campaign data remain owned by `civ`.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    state: Res<State<GameStage>>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cities: Query<(Entity, &CityMarker, Option<&CityWallStyle>)>,
    walls: Query<(Entity, &ChildOf), With<NativeCityWallSurface>>,
    mut failed: Local<HashMap<ResourceKey, String>>,
) {
    if *state.get() != GameStage::Civilization {
        return;
    }
    for (city, marker, style) in &cities {
        let Some(campaign_city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        let kind = campaign_city.kind;
        let large = campaign_city.walls_large();
        if style.map(|s| (s.0, s.1)) == Some((kind, large)) {
            continue;
        }
        for (wall, parent) in &walls {
            if parent.parent() == city {
                commands.entity(wall).despawn();
            }
        }
        commands.entity(city).insert(CityWallStyle(kind, large));
        let (key, name) = wall_source(kind, large);
        if failed.contains_key(&key) {
            continue;
        }
        let imported = world_object(
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            key,
            name,
        )
        .and_then(|(model, source)| Ok((fit_wall(&model, &meshes)?, model, source)));
        let (transform, model, source) = match imported {
            Ok(result) => result,
            Err(error) => {
                if !originals.resource_index.records.is_empty() {
                    warn!("city walls unavailable ({name}): {error}");
                }
                failed.insert(key, error);
                continue;
            }
        };
        commands
            .entity(city)
            .insert(CityWallPresentation(source.model));
        for surface in &model.surfaces {
            commands.spawn((
                StageOwned,
                NativeCityWallSurface(source.model),
                source.clone(),
                material_binding::OriginalSurfaceChild {
                    source_section: surface.source_section,
                    state_sections: surface.state_sections.clone(),
                },
                Name::new(format!(
                    "city {} native {name} surface {}",
                    marker.0 + 1,
                    surface.source_section
                )),
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(surface.material.clone()),
                transform,
                ChildOf(city),
            ));
        }
    }
}

/// Primitive stand-ins for a plot building when the install is missing.
fn primitive_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    city: Entity,
    tag: CityPlotVisual,
    at: Vec3,
    nation: usize,
) {
    let wall = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.82, 0.74),
        perceptual_roughness: 0.8,
        ..default()
    });
    let roof = materials.add(crate::civ::nation_material(nation));
    let mut part = |mesh: Mesh, material: Handle<StandardMaterial>, offset: Vec3| {
        commands.spawn((
            StageOwned,
            tag,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(at + offset),
            ChildOf(city),
        ));
    };
    match tag.kind {
        Building::House => {
            part(
                Cuboid::new(1.6, 1.2, 1.6).into(),
                wall.clone(),
                Vec3::Y * 0.6,
            );
            part(Cone::new(1.2, 0.9).into(), roof, Vec3::Y * 1.65);
        }
        Building::Factory => {
            part(
                Cuboid::new(2.0, 1.4, 1.5).into(),
                wall.clone(),
                Vec3::Y * 0.7,
            );
            part(
                Cylinder::new(0.22, 1.6).into(),
                roof,
                Vec3::new(0.6, 2.0, 0.3),
            );
        }
        Building::Entertainment => {
            part(
                Cylinder::new(0.95, 0.9).into(),
                wall.clone(),
                Vec3::Y * 0.45,
            );
            part(Sphere::new(0.9).into(), roof, Vec3::Y * 0.95);
        }
    }
}

/// Draw every city's building ring from the saved plots: a building bought
/// with `B`/`4`/`5`/`6` appears on its plot the same frame. Native surfaces
/// are direct children of the city root, each tagged with its plot.
#[allow(clippy::too_many_arguments)]
pub fn sync_plots(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cities: Query<(Entity, &CityMarker)>,
    visuals: Query<(Entity, &CityPlotVisual)>,
    mut art: Local<Option<HashMap<Building, Option<OriginalModel>>>>,
) {
    let art = art.get_or_insert_with(|| {
        let mut loaded = HashMap::new();
        for kind in Building::ALL {
            let key = source_for(kind);
            let model = if originals.resource_index.records.is_empty() {
                None
            } else {
                match baked_model::ensure(
                    &mut originals,
                    key,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                ) {
                    Ok(model) if !model.surfaces.is_empty() => Some(model),
                    Ok(_) => {
                        warn!("city {} building has no draw surfaces", kind.title());
                        None
                    }
                    Err(error) => {
                        warn!("city {} building unavailable: {error}", kind.title());
                        None
                    }
                }
            };
            loaded.insert(kind, model);
        }
        loaded
    });
    let mut shown: HashMap<(usize, usize), (Building, u32)> = HashMap::new();
    for (entity, visual) in &visuals {
        let want = run
            .civ_campaign
            .cities
            .get(visual.city)
            .filter(|c| c.standing(visual.plot))
            .and_then(|c| c.plots.get(visual.plot).copied().flatten());
        if want != Some(visual.kind) || visual.design != crate::rigid_editor::plot_design_signature(&run, visual.kind) {
            commands.entity(entity).despawn();
        } else {
            shown.insert((visual.city, visual.plot), (visual.kind, visual.design));
        }
    }
    for (city_entity, marker) in &cities {
        let Some(city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        let mut first = None;
        for (plot, kind) in city.plots.iter().enumerate() {
            // A destroyed building is not drawn (ADAPTED: no rubble model).
            let Some(kind) = kind.filter(|_| city.standing(plot)) else {
                continue;
            };
            let design = crate::rigid_editor::plot_design_signature(&run, kind);
            if shown.get(&(marker.0, plot)) == Some(&(kind, design)) {
                continue;
            }
            let site = city.plot_site(plot);
            let at = Vec3::new(site.x, -CITY_ROOT_CLEARANCE, site.y);
            // Fronts face outward, toward the walls and the gates.
            let yaw = (-site.x).atan2(-site.y) + std::f32::consts::PI;
            let tag = CityPlotVisual {
                city: marker.0,
                plot,
                kind,
                design,
                site,
            };
            // The player's own design of this kind (`rigid_editor`) stands
            // instead of the native building.
            if let Some(designed) = crate::rigid_editor::plot_design(&run, kind) {
                crate::rigid_editor::mount_plot_design(&mut commands, &originals, &mut materials, city_entity, designed, at, yaw, tag);
                continue;
            }
            match art.get(&kind).cloned().flatten() {
                Some(model) => {
                    let Ok(transform) =
                        district_transform(&model, &meshes, at, CITY_DISTRICT_WIDTH, yaw)
                    else {
                        continue;
                    };
                    let source = source_for(kind);
                    first.get_or_insert(source);
                    for surface in &model.surfaces {
                        commands.spawn((
                            StageOwned,
                            tag,
                            NativeCityDistrictSurface(source),
                            material_binding::OriginalSurfaceChild {
                                source_section: surface.source_section,
                                state_sections: surface.state_sections.clone(),
                            },
                            Name::new(format!(
                                "city {} plot {} {} surface {}",
                                marker.0 + 1,
                                plot + 1,
                                kind.title(),
                                surface.source_section
                            )),
                            Mesh3d(surface.handle.clone()),
                            MeshMaterial3d(surface.material.clone()),
                            transform,
                            ChildOf(city_entity),
                        ));
                    }
                }
                None => primitive_building(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    city_entity,
                    tag,
                    at,
                    city.nation(),
                ),
            }
        }
        if let Some(source) = first {
            commands
                .entity(city_entity)
                .insert(CityDistrictPresentation(source));
        }
    }
}

/// Wall turrets: one gun per standing (or ruined) turret on the authored
/// `Turrets` mounts of the city's walls, styled by the city type. Standing guns track the target
/// their turret is firing at; ruined ones show the authored wreck.
#[allow(clippy::too_many_arguments)]
pub fn sync_turrets(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    rts: Option<Res<crate::civ_rts::RtsState>>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cities: Query<(Entity, &CityMarker, &GlobalTransform)>,
    mut turrets: Query<(Entity, &CityTurretVisual, &mut Transform)>,
    mut models: Local<HashMap<ResourceKey, Option<OriginalModel>>>,
    mut scans: Local<HashMap<(usize, usize), crate::civ_fx::TurretScan>>,
) {
    let mut shown: HashMap<(usize, usize), (bool, Approach)> = HashMap::new();
    for (entity, turret, mut transform) in &mut turrets {
        let Some(city) = run.civ_campaign.cities.get(turret.city) else {
            commands.entity(entity).despawn();
            continue;
        };
        let alive = city.turret_hp.get(turret.turret).map(|hp| *hp > 0.0);
        if alive != Some(turret.alive) || city.kind != turret.kind {
            commands.entity(entity).despawn();
            continue;
        }
        shown.insert((turret.city, turret.turret), (turret.alive, turret.kind));
        if !turret.alive {
            continue;
        }
        // Track the target at a quarter turn a second, or look around while idle (the retail
        // turret facing update 0xBD1E70, `civ_fx::TurretScan`).
        let target = rts
            .as_ref()
            .and_then(|r| r.turret_aim.get(&(turret.city, turret.turret)))
            .and_then(|aim| {
                let (_, _, city_transform) = cities.iter().find(|(_, m, _)| m.0 == turret.city)?;
                // In the city's own (tilted, scaled) frame.
                let pivot = city_transform.transform_point(transform.translation);
                let to = city_transform.affine().inverse().transform_vector3(*aim - pivot);
                Some((-to.x).atan2(-to.z))
            });
        let site = city.turret_site(turret.turret);
        let outward = (-site.x).atan2(-site.y) + std::f32::consts::PI;
        let current = transform.rotation.to_euler(EulerRot::YXZ).0;
        let yaw = scans
            .entry((turret.city, turret.turret))
            .or_default()
            .step(current, outward, target, time.delta_secs().min(0.1));
        transform.rotation = Quat::from_rotation_y(yaw);
    }
    for (city_entity, marker, _) in &cities {
        let Some(city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        for (i, hp) in city.turret_hp.iter().enumerate() {
            let alive = *hp > 0.0;
            if shown.get(&(marker.0, i)) == Some(&(alive, city.kind)) {
                continue;
            }
            let (key, name) = gun_source(city.kind, !alive);
            let model = models
                .entry(key)
                .or_insert_with(|| {
                    world_object(
                        &mut originals,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                        key,
                        name,
                    )
                    .map_err(|e| {
                        if !originals.resource_index.records.is_empty() {
                            warn!("city turret {name}: {e}")
                        }
                    })
                    .ok()
                    .map(|(model, _)| model)
                })
                .clone();
            let site = city.turret_site(i);
            let mount = city.layout().turret_height(i);
            let pivot = commands
                .spawn((
                    StageOwned,
                    CityTurretVisual {
                        city: marker.0,
                        turret: i,
                        alive,
                        kind: city.kind,
                    },
                    Name::new(format!("city {} turret {}", marker.0 + 1, i + 1)),
                    Transform::from_xyz(site.x, -CITY_ROOT_CLEARANCE + mount, site.y)
                        .with_rotation(Quat::from_rotation_y(
                            (-site.x).atan2(-site.y) + std::f32::consts::PI,
                        )),
                    Visibility::Inherited,
                    ChildOf(city_entity),
                ))
                .id();
            match model.and_then(|m| {
                crate::world_objects::fitted_transform(&m, &meshes, Vec3::ZERO, TURRET_WIDTH)
                    .ok()
                    .map(|t| (m, t))
            }) {
                Some((model, transform)) => {
                    for surface in &model.surfaces {
                        commands.spawn((
                            StageOwned,
                            material_binding::OriginalSurfaceChild {
                                source_section: surface.source_section,
                                state_sections: surface.state_sections.clone(),
                            },
                            Mesh3d(surface.handle.clone()),
                            MeshMaterial3d(surface.material.clone()),
                            transform,
                            ChildOf(pivot),
                        ));
                    }
                }
                None => {
                    let metal = materials.add(StandardMaterial {
                        base_color: if alive {
                            Color::srgb(0.5, 0.52, 0.58)
                        } else {
                            Color::srgb(0.18, 0.16, 0.15)
                        },
                        perceptual_roughness: 0.5,
                        ..default()
                    });
                    commands.spawn((
                        StageOwned,
                        Mesh3d(meshes.add(Cylinder::new(0.5, 0.6))),
                        MeshMaterial3d(metal.clone()),
                        Transform::from_xyz(0.0, 0.3, 0.0),
                        ChildOf(pivot),
                    ));
                    if alive {
                        commands.spawn((
                            StageOwned,
                            Mesh3d(meshes.add(Cylinder::new(0.12, 1.1))),
                            MeshMaterial3d(metal),
                            Transform::from_xyz(0.0, 0.65, -0.5)
                                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                            ChildOf(pivot),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verified_category_key_is_a_building_gmdl() {
        for key in category_source_keys() {
            assert_eq!(key.group_id, CITY_DISTRICT_GROUP);
            assert_eq!(key.type_id, decode::TYPE_GMDL);
        }
        let unique: std::collections::BTreeSet<_> = category_source_keys()
            .iter()
            .map(|key| key.instance_id)
            .collect();
        assert_eq!(unique.len(), category_source_keys().len());
    }

    #[test]
    fn city_types_map_to_their_own_wall_and_gun_sets() {
        let walls: std::collections::BTreeSet<_> =
            [Approach::Military, Approach::Religious, Approach::Economic]
                .into_iter()
                .flat_map(|kind| [false, true].map(|large| wall_source(kind, large).0.instance_id))
                .collect();
        assert_eq!(walls.len(), 6, "three types x small / large");
        assert_eq!(wall_source(Approach::Military, false).0.instance_id, 0x25A9_2685);
        for kind in [Approach::Military, Approach::Religious, Approach::Economic] {
            let (live, live_name) = gun_source(kind, false);
            let (dead, dead_name) = gun_source(kind, true);
            assert_ne!(live, dead);
            assert_eq!(format!("{live_name}_destroyed"), dead_name);
            assert_eq!(
                crate::world_objects::name_id(live_name),
                live.instance_id,
                "{live_name}"
            );
            assert_eq!(crate::world_objects::name_id(dead_name), dead.instance_id);
            for large in [false, true] {
                assert_eq!(
                    crate::world_objects::name_id(wall_source(kind, large).1),
                    wall_source(kind, large).0.instance_id
                );
            }
        }
    }

    #[test]
    fn plot_buildings_use_only_placement_assets() {
        // 5 building slots behind the small walls, 11 behind the large ones.
        assert_eq!(crate::civ::CityState::new(0).layout().plots(), 11);
        assert_eq!(crate::civ::CityState::new(1).layout().plots(), 5);
        assert_eq!(district_source_keys().len(), 3);
        for kind in Building::ALL {
            assert!(district_source_keys().contains(&source_for(kind)));
        }
    }

    #[test]
    fn centered_fit_moves_a_displaced_source_into_its_plot() {
        let site = crate::civ::CityState::new(1).plot_site(0);
        let transform = centered_fit_transform(
            Vec3::new(-4.0, -1.0, -2.0),
            Vec3::new(2.0, 3.0, 4.0),
            Vec3::new(site.x, 0.0, site.y),
            CITY_DISTRICT_WIDTH,
            0.7,
        )
        .expect("valid bounds");
        let corners = [
            Vec3::new(-4.0, -1.0, -2.0),
            Vec3::new(2.0, -1.0, 4.0),
            Vec3::new(-4.0, 3.0, 4.0),
            Vec3::new(2.0, 3.0, -2.0),
        ];
        for corner in corners {
            let p = transform.transform_point(corner);
            let from_hall = Vec2::new(p.x, p.z).length();
            assert!(
                from_hall >= CITY_CENTRAL_HALL_FOOTPRINT * 0.5 - 0.001,
                "{p:?}"
            );
            assert!(p.x.abs() <= crate::city_layout::WALL_EXTENT * 0.5 + 0.001, "{p:?}");
            assert!(p.z.abs() <= crate::city_layout::WALL_EXTENT * 0.5 + 0.001, "{p:?}");
            assert!(p.y >= -0.001, "grounded: {p:?}");
        }
    }

    #[test]
    fn invalid_or_missing_bounds_are_rejected() {
        assert!(centered_fit_transform(
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 3.0),
            CITY_DISTRICT_WIDTH,
            0.0,
        )
        .is_err());
        assert!(centered_fit_transform(
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, f32::NAN, 1.0),
            Vec3::new(3.0, 0.0, 3.0),
            CITY_DISTRICT_WIDTH,
            0.0,
        )
        .is_err());
    }

    /// The layout properties and the walls mesh share one frame: the mesh is
    /// centred on the authored origin, as wide as `radiusOuter` says, and
    /// every authored turret mount (x, y) -> (x, -y) has wall geometry under
    /// it (this is what fixes the Spore-to-city axis mapping of `city_layout`).
    #[test]
    #[ignore = "requires a Spore install"]
    fn installed_walls_hold_the_authored_slots_on_their_mesh() {
        use crate::asset_catalog::ResourceIndex;
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = ResourceIndex::load(&install.packages);
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        for kind in [Approach::Military, Approach::Religious, Approach::Economic] {
            for large in [false, true] {
                let layout = crate::city_layout::wall_layout(kind, large);
                let (key, name) = wall_source(kind, large);
                let (model, _) =
                    world_object(&mut originals, &mut meshes, &mut images, &mut materials, key, name).unwrap();
                let (min, max) = mesh_bounds(&model, &meshes).unwrap();
                let reach = max.x.max(-min.x).max(max.z).max(-min.z);
                println!("{name}: mesh x {:.1}..{:.1} y {:.1}..{:.1} z {:.1}..{:.1}, outer {}", min.x, max.x, min.y, max.y, min.z, max.z, layout.outer_radius);
                assert!(
                    (reach / layout.outer_radius - 1.0).abs() < 0.2,
                    "{name}: the mesh reaches {reach}, radiusOuter {}",
                    layout.outer_radius
                );
                let Some(VertexAttributeValues::Float32x3(vertices)) =
                    meshes.get(&model.handle).unwrap().attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("positions");
                };
                for (i, mount) in layout.turrets.iter().enumerate() {
                    let want = Vec2::new(mount[0], -mount[1]);
                    let near = vertices
                        .iter()
                        .map(|v| Vec2::new(v[0], v[2]).distance(want))
                        .fold(f32::MAX, f32::min);
                    let mirrored = vertices
                        .iter()
                        .map(|v| Vec2::new(v[0], v[2]).distance(Vec2::new(mount[0], mount[1])))
                        .fold(f32::MAX, f32::min);
                    println!("{name}: turret {i} nearest vertex {near:.2} (mirrored {mirrored:.2})");
                    assert!(near < 6.0, "{name}: turret {i} has no wall under it ({near})");
                }
                // The gates stand in gaps of the ring: the main gate has less
                // wall around it than a turret mount has.
                let wall_near = |p: Vec2| {
                    vertices.iter().filter(|v| Vec2::new(v[0], v[2]).distance(p) < 8.0).count()
                };
                let gate = wall_near(Vec2::new(layout.main_gate[0], -layout.main_gate[1]));
                let mount = wall_near(Vec2::new(layout.turrets[0][0], -layout.turrets[0][1]));
                println!("{name}: wall vertices within 8 units: main gate {gate}, turret 0 {mount}");
            }
        }
    }

    #[test]
    #[ignore = "requires a Spore install"]
    fn installed_category_verified_sources_parse_and_have_surfaces() {
        use crate::asset_catalog::ResourceIndex;
        use crate::Originals;
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut originals = Originals::default();
        originals.resource_index = index.clone();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let expected = |key: ResourceKey| -> u32 {
            match key.instance_id {
                CITY_DISTRICT_CITY_HALL_INSTANCE => 0x99E92F05,
                CITY_DISTRICT_HOUSE_INSTANCE => 0x4E3F7777,
                CITY_DISTRICT_INDUSTRY_INSTANCE => 0x47C10953,
                CITY_DISTRICT_ENTERTAINMENT_INSTANCE => 0x72C49181,
                _ => 0,
            }
        };
        let mut checked = 0;
        for key in category_source_keys() {
            let bem_key = ResourceKey {
                group_id: 0x4063_6200,
                instance_id: key.instance_id,
                type_id: 0x1A99_B06B,
            };
            let bem = index.read(bem_key).unwrap();
            assert!(
                bem.len() >= 22,
                "BEM too short for ModelType field: {key:?}"
            );
            let model_type = u32::from_le_bytes(bem[18..22].try_into().unwrap());
            assert_eq!(
                model_type,
                expected(*key),
                "unexpected ModelType for {key:?}: 0x{model_type:08X}"
            );
            let model = baked_model::import(
                &mut originals,
                *key,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .unwrap();
            assert!(
                !model.surfaces.is_empty(),
                "source has no surfaces: {key:?}"
            );
            checked += 1;
        }
        assert_eq!(checked, category_source_keys().len());
    }
}
