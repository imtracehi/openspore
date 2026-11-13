//! Small world map shared by the playable stages.
use crate::{game::*, persistence::WorldId, stages::GameStage};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

#[derive(Component)]
pub struct MapRoot;

/// A fixed north-up map: W moves towards its top, D towards its right.
fn map_position(position: Vec3, bounds: f32) -> Vec2 {
    Vec2::new(
        ((position.x / bounds + 1.) * 0.5).clamp(0., 1.),
        ((position.z / bounds + 1.) * 0.5).clamp(0., 1.),
    ) * 180.
}

fn map_bounds(stage: &GameStage, current_system: Option<&crate::space::CurrentSystem>) -> f32 {
    match stage {
        GameStage::Cell => 18.,
        GameStage::Creature => 42.,
        GameStage::Tribal => 45.,
        GameStage::Civilization => 48.,
        GameStage::Space => current_system.map_or(42., |system| system.extent().max(1.)),
        GameStage::MainMenu => 42.,
    }
}

const MAP_TEXTURE_SIZE: u32 = 128;

fn terrain_map_pixels(
    stage: &GameStage,
    bounds: f32,
    seed: u64,
    surface: Option<&crate::terrain::LandSurface>,
    palette: Option<&crate::world_catalog::PlanetPalette>,
) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((MAP_TEXTURE_SIZE * MAP_TEXTURE_SIZE * 4) as usize);
    let phase = ((seed ^ (seed >> 32)) as u32) as f32 / u32::MAX as f32 * 6.28;
    for y in 0..MAP_TEXTURE_SIZE {
        for x in 0..MAP_TEXTURE_SIZE {
            let u = x as f32 / (MAP_TEXTURE_SIZE - 1) as f32;
            let v = y as f32 / (MAP_TEXTURE_SIZE - 1) as f32;
            let color = if let Some(surface) = surface {
                // The texture and markers use the same north-up world bounds.
                let world_x = (u * 2. - 1.) * bounds;
                let world_z = (v * 2. - 1.) * bounds;
                let height = surface.height(world_x, world_z);
                let radial_noise = (world_x * 0.31 + phase).sin() * 1.05
                    + (world_z * 0.27 - phase * 0.7).cos() * 0.8
                    + ((world_x + world_z) * 0.16 + phase * 1.4).sin() * 0.65;
                let coastline = bounds * 0.86 + radial_noise;
                let coast_depth = coastline - Vec2::new(world_x, world_z).length();
                if coast_depth < -0.25 {
                    palette.map_or([0.025, 0.12, 0.20], |p| p.water)
                } else {
                    let rise = ((height + 0.6) / 2.).clamp(0., 1.);
                    let land = palette.map_or_else(
                        || {
                            let amount = rise.clamp(0.10, 0.92);
                            [
                                0.12 + amount * 0.15,
                                0.24 + amount * 0.25,
                                0.10 + amount * 0.12,
                            ]
                        },
                        |p| p.sample(rise),
                    );
                    if let Some(palette) = palette {
                        let t = (coast_depth / 1.6).clamp(0., 1.);
                        let beach_weight = 1. - t * t * (3. - 2. * t);
                        std::array::from_fn(|channel| {
                            land[channel] * (1. - beach_weight)
                                + palette.beach[channel] * beach_weight
                        })
                    } else {
                        land
                    }
                }
            } else if *stage == GameStage::Space {
                // Space has no LandSurface; show a restrained starfield instead.
                let speck = ((x * 37 + y * 61 + x * y * 13) % 173) == 0;
                if speck {
                    [0.34, 0.47, 0.64]
                } else {
                    [0.018, 0.035, 0.075]
                }
            } else {
                [0.025, 0.12, 0.20]
            };
            pixels.extend(color.map(|channel| (channel.clamp(0., 1.) * 255.) as u8));
            pixels.push(255);
        }
    }
    pixels
}

fn galaxy_map_pixels(galaxy: &crate::space_galaxy::Galaxy) -> Vec<u8> {
    let mut pixels = vec![0; (MAP_TEXTURE_SIZE * MAP_TEXTURE_SIZE * 4) as usize];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[8, 13, 32, 255]);
    }
    for star in galaxy.stars.iter() {
        let position = map_position(
            Vec3::new(star.position.x, 0., star.position.y),
            crate::space_galaxy::GALAXY_RADIUS,
        );
        let x = ((position.x / 180.) * MAP_TEXTURE_SIZE as f32) as usize;
        let y = ((position.y / 180.) * MAP_TEXTURE_SIZE as f32) as usize;
        let x = x.min(MAP_TEXTURE_SIZE as usize - 1);
        let y = y.min(MAP_TEXTURE_SIZE as usize - 1);
        let color = star.empire.map_or_else(
            || star.kind.color(),
            crate::space_galaxy::empire_color,
        );
        let offset = (y * MAP_TEXTURE_SIZE as usize + x) * 4;
        for channel in 0..3 {
            pixels[offset + channel] = (color[channel].clamp(0., 1.) * 220.) as u8;
        }
        pixels[offset + 3] = 255;
    }
    pixels
}

fn map_image(
    stage: &GameStage,
    seed: u64,
    bounds: f32,
    surface: Option<&crate::terrain::LandSurface>,
    land_theme: Option<&crate::world_presentation::LandTheme>,
    galaxy: Option<&crate::space_galaxy::Galaxy>,
    galaxy_view: bool,
    originals: &crate::Originals,
    images: &mut Assets<Image>,
    cached: &mut Option<(GameStage, u64, bool, u32, Handle<Image>)>,
) -> Handle<Image> {
    if let Some((cached_stage, cached_seed, cached_galaxy_view, cached_bounds, image)) = cached {
        if *cached_stage == *stage
            && *cached_seed == seed
            && *cached_galaxy_view == galaxy_view
            && *cached_bounds == bounds.to_bits()
        {
            return image.clone();
        }
    }

    let loaded_theme = if land_theme.is_none() && surface.is_some() {
        crate::world_presentation::LandTheme::load(&originals.resource_index).ok()
    } else {
        None
    };
    let palette = land_theme
        .map(|theme| &theme.palette)
        .or_else(|| loaded_theme.as_ref().map(|theme| &theme.palette));
    let data = if galaxy_view {
        galaxy.map_or_else(
            || terrain_map_pixels(stage, bounds, seed, surface, palette),
            galaxy_map_pixels,
        )
    } else {
        terrain_map_pixels(stage, bounds, seed, surface, palette)
    };
    let image = Image::new(
        Extent3d {
            width: MAP_TEXTURE_SIZE,
            height: MAP_TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let image = images.add(image);
    *cached = Some((
        stage.clone(),
        seed,
        galaxy_view,
        bounds.to_bits(),
        image.clone(),
    ));
    image
}

pub fn update_map(
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed: Local<f32>,
    (
        state,
        run,
        editor,
        surface,
        land_theme,
        current_system,
        space_view,
        galaxy,
        originals,
        civ_hud,
        space_hud,
        creature_hud,
        tribal_hud,
    ): (
        Res<State<GameStage>>,
        Res<RunProgress>,
        Res<crate::editor::EditorState>,
        Option<Res<crate::terrain::LandSurface>>,
        Option<Res<crate::world_presentation::LandTheme>>,
        Option<Res<crate::space::CurrentSystem>>,
        Option<Res<crate::space_view::SpaceView>>,
        Option<Res<crate::space_galaxy::Galaxy>>,
        Res<crate::Originals>,
        Option<Res<crate::civ_hud::CivHudActive>>,
        Option<Res<crate::space_hud::SpaceHudActive>>,
        Option<Res<crate::creature_hud::CreatureHudActive>>,
        Option<Res<crate::tribal_hud::TribalHudActive>>,
    ),
    mut images: ResMut<Assets<Image>>,
    mut cached_map: Local<Option<(GameStage, u64, bool, u32, Handle<Image>)>>,
    materials: Res<Assets<StandardMaterial>>,
    roots: Query<Entity, With<MapRoot>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
    points: Query<(
        Entity,
        &Transform,
        Option<&Player>,
        Option<&Bush>,
        Option<&SpiceNode>,
        Option<&Planet>,
        Option<&TribeMate>,
        Option<&Npc>,
        Option<&crate::tribal::RivalVillage>,
        Option<&crate::civ::CityMarker>,
        Option<&crate::civ_rts::CivUnit>,
        Option<&crate::civ_rts::Selected>,
        Option<&WorldId>,
        Option<&Edible>,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    *elapsed += time.delta_secs();
    // The retail SPUI HUDs draw their own minimap: Creature and Tribal
    // (`land_minimap`) and Civ (`civ_hud`), and the Space HUD (`space_hud`) its
    // own planet panel and minimap.
    let hidden = matches!(state.get(), GameStage::MainMenu | GameStage::Cell)
        || editor.open
        || (*state.get() == GameStage::Creature && creature_hud.as_deref().is_some_and(|a| a.0))
        || (*state.get() == GameStage::Tribal && tribal_hud.as_deref().is_some_and(|a| a.0))
        || (*state.get() == GameStage::Civilization && civ_hud.as_deref().is_some_and(|a| a.0))
        || (*state.get() == GameStage::Space && space_hud.as_deref().is_some_and(|a| a.0));
    if hidden {
        for root in &roots {
            commands.entity(root).despawn();
        }
        *elapsed = 1.;
        return;
    }
    if *elapsed < 0.25 && !roots.is_empty() && !state.is_changed() {
        return;
    }
    *elapsed = 0.;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let galaxy_view =
        *state.get() == GameStage::Space && space_view.as_deref().is_some_and(|view| view.galaxy);
    let bounds = if galaxy_view {
        crate::space_galaxy::GALAXY_RADIUS
    } else {
        map_bounds(state.get(), current_system.as_deref())
    };
    let mut dots = Vec::new();
    let mut player_marker = None;
    for (
        entity,
        transform,
        player,
        bush,
        spice,
        planet,
        mate,
        npc,
        village,
        city,
        civ_unit,
        civ_selected,
        id,
        edible,
        material,
    ) in &points
    {
        if galaxy_view {
            continue;
        }
        let (label, color, size) = if player.is_some() {
            (String::new(), Color::WHITE, 7.)
        } else if let Some(village) = village {
            (format!("V{}", village.0 + 1), Color::srgb(1., 0.5, 0.2), 7.)
        } else if let Some(city) = city {
            (format!("C{}", city.0 + 1), Color::srgb(1., 0.7, 0.2), 7.)
        } else if let Some(unit) = civ_unit.filter(|unit| unit.hp > 0.) {
            (
                String::new(),
                crate::civ::nation_color(unit.nation),
                if civ_selected.is_some() { 5. } else { 3. },
            )
        } else if let Some(planet) = planet {
            let label = if id.is_some_and(|id| id.0 == 0) {
                "HOME".to_string()
            } else {
                id.map(|id| format!("P{}", id.0)).unwrap_or_default()
            };
            let color = if planet.colonized {
                Color::srgb(0.2, 1., 0.6)
            } else if planet.surveyed {
                Color::srgb(0.3, 0.7, 1.)
            } else {
                Color::srgb(0.7, 0.7, 0.9)
            };
            (label, color, 6.)
        } else if spice.is_some_and(|n| n.spice_left > 0.) {
            (String::new(), Color::srgb(1., 0.3, 0.7), 4.)
        } else if edible.is_some() || bush.is_some_and(|b| b.food_left > 0.) {
            (String::new(), Color::srgb(0.3, 0.9, 0.3), 3.)
        } else if mate.is_some() {
            (String::new(), Color::srgb(0.4, 0.8, 1.), 3.)
        } else if let Some(npc) = npc.filter(|n| n.hp > 0.) {
            (
                String::new(),
                if npc.hostile {
                    Color::srgb(1., 0.2, 0.2)
                } else {
                    Color::srgb(0.8, 0.8, 0.3)
                },
                3.,
            )
        } else {
            continue;
        };
        let color = if village.is_some() || city.is_some() {
            material
                .and_then(|m| materials.get(&m.0))
                .map(|m| m.base_color)
                .unwrap_or(color)
        } else {
            color
        };
        let position = map_position(transform.translation, bounds);
        if player.is_some() {
            let forward = transform.rotation * Vec3::NEG_Z;
            let heading = forward.x.atan2(-forward.z);
            player_marker = Some((position, heading));
        }
        dots.push((entity, position, label, color, size));
    }
    if galaxy_view {
        if let (Some(galaxy), Some(view)) = (galaxy.as_deref(), space_view.as_deref()) {
            let placeholder = points
                .iter()
                .next()
                .map(|point| point.0)
                .unwrap_or(Entity::PLACEHOLDER);
            if let Some(star) = galaxy.star(run.space_campaign.current_system) {
                dots.push((
                    placeholder,
                    map_position(Vec3::new(star.position.x, 0., star.position.y), bounds),
                    "HERE".into(),
                    Color::srgb(0.2, 1., 0.6),
                    8.,
                ));
            }
            if view.cursor != run.space_campaign.current_system {
                if let Some(star) = galaxy.star(view.cursor) {
                    dots.push((
                        placeholder,
                        map_position(Vec3::new(star.position.x, 0., star.position.y), bounds),
                        "TARGET".into(),
                        Color::srgb(1., 0.82, 0.24),
                        7.,
                    ));
                }
            }
        }
    }
    if matches!(state.get(), GameStage::Tribal | GameStage::Civilization) {
        if let Some((entity, ..)) = dots.first() {
            dots.push((
                *entity,
                map_position(Vec3::ZERO, bounds),
                "HOME".into(),
                Color::srgb(0.3, 1., 0.8),
                7.,
            ));
        }
    }
    // Fixed cap prevents an expanding world from filling the interface.
    if *state.get() == GameStage::Creature {
        if let Some((entity, ..)) = dots.first() {
            let entity = *entity;
            dots.push((
                entity,
                map_position(Vec3::from_array(run.creature_lifecycle.home), bounds),
                "HOME".into(),
                Color::srgb(0.3, 1., 0.8),
                7.,
            ));
            if let Some(migration) = run.creature_progression.pending_migration {
                dots.push((
                    entity,
                    map_position(Vec3::from_array(migration.destination), bounds),
                    "MOVE".into(),
                    Color::srgb(1., 0.85, 0.25),
                    9.,
                ));
            }
        }
    }
    dots.sort_by_key(|(e, _, label, _, _)| (label.is_empty(), e.index()));
    dots.truncate(64);
    let map = map_image(
        state.get(),
        run.species_world.seed,
        bounds,
        surface.as_deref(),
        land_theme.as_deref(),
        galaxy.as_deref(),
        galaxy_view,
        &originals,
        &mut images,
        &mut cached_map,
    );
    let root = commands
        .spawn((
            MapRoot,
            // Spore keeps its round planet map at the bottom left, just above
            // the DNA strip; the player's portrait owns the bottom right.
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.),
                bottom: Val::Px(if *state.get() == GameStage::Creature {
                    64.
                } else {
                    36.
                }),
                width: Val::Px(200.),
                height: Val::Px(240.),
                padding: UiRect::all(Val::Px(8.)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(7.),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(16.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.018, 0.055, 0.12, 0.55)),
            BorderColor::all(Color::srgba(0.47, 0.70, 0.95, 0.5)),
        ))
        .id();
    if let Ok(camera) = cameras.single() {
        commands.entity(root).insert(UiTargetCamera(camera));
    }
    let legend = if galaxy_view {
        [
            ("Here", Color::srgb(0.2, 1., 0.6)),
            ("Target", Color::srgb(1., 0.82, 0.24)),
            ("Stars", Color::srgb(0.58, 0.75, 1.)),
        ]
    } else {
        match state.get() {
            GameStage::Creature => [
                ("You", Color::srgb(0.25, 1., 0.92)),
                ("Food", Color::srgb(0.3, 0.95, 0.4)),
                ("Spice", Color::srgb(1., 0.38, 0.68)),
            ],
            GameStage::Tribal => [
                ("You", Color::srgb(0.25, 1., 0.92)),
                ("Food", Color::srgb(0.3, 0.95, 0.4)),
                ("Tribes", Color::srgb(1., 0.5, 0.2)),
            ],
            GameStage::Civilization => [
                ("You", Color::srgb(0.25, 1., 0.92)),
                ("Cities", Color::srgb(1., 0.7, 0.2)),
                ("Spice", Color::srgb(1., 0.38, 0.68)),
            ],
            GameStage::Space | GameStage::Cell | GameStage::MainMenu => [
                ("Home", Color::srgb(0.2, 1., 0.6)),
                ("Surveyed", Color::srgb(0.3, 0.7, 1.)),
                ("Planets", Color::srgb(0.7, 0.7, 0.9)),
            ],
        }
    };
    let map_title = if galaxy_view {
        "GALAXY MAP"
    } else if *state.get() == GameStage::Space {
        "SYSTEM MAP"
    } else {
        "WORLD MAP"
    };
    commands.entity(root).with_children(|panel| {
        panel
            .spawn((Node {
                width: Val::Percent(100.),
                height: Val::Px(18.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },))
            .with_children(|header| {
                header.spawn((
                    Text::new(map_title),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(12.),
                        ..default()
                    },
                    TextColor(Color::srgb(0.88, 0.95, 0.98)),
                ));
                header.spawn((
                    Text::new("N"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(10.),
                        ..default()
                    },
                    TextColor(Color::srgb(0.65, 0.85, 0.91)),
                ));
            });
        panel
            .spawn((
                Node {
                    width: Val::Px(180.),
                    height: Val::Px(180.),
                    border: UiRect::all(Val::Px(2.)),
                    border_radius: BorderRadius::all(Val::Px(14.)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.025, 0.06, 0.10)),
                BorderColor::all(Color::srgba(0.68, 0.85, 0.91, 0.72)),
            ))
            .with_children(|canvas| {
                canvas.spawn((
                    ImageNode::new(map.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.),
                        top: Val::Px(0.),
                        width: Val::Percent(100.),
                        height: Val::Percent(100.),
                        ..default()
                    },
                ));
                for (_, position, label, color, size) in dots {
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px((position.x - size * 0.5).clamp(0., 180. - size)),
                            top: Val::Px((position.y - size * 0.5).clamp(0., 180. - size)),
                            width: Val::Px(size),
                            height: Val::Px(size),
                            border: UiRect::all(Val::Px(1.)),
                            border_radius: BorderRadius::all(Val::Percent(50.)),
                            ..default()
                        },
                        BackgroundColor(color),
                        BorderColor::all(Color::srgba(0.015, 0.035, 0.055, 0.9)),
                    ));
                    if !label.is_empty() {
                        canvas.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(8.),
                                ..default()
                            },
                            TextColor(Color::srgb(0.94, 0.98, 1.0)),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px((position.x + 5.).clamp(0., 154.)),
                                top: Val::Px((position.y - 8.).clamp(0., 168.)),
                                padding: UiRect::axes(Val::Px(3.), Val::Px(1.)),
                                border_radius: BorderRadius::all(Val::Px(4.)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.015, 0.045, 0.07, 0.78)),
                        ));
                    }
                }

                if let Some((position, heading)) = player_marker {
                    canvas.spawn((
                        Text::new("^"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(14.),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px((position.x - 7.).clamp(0., 166.)),
                            top: Val::Px((position.y - 9.).clamp(0., 164.)),
                            width: Val::Px(14.),
                            height: Val::Px(18.),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::radians(heading)),
                    ));
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px((position.x - 3.).clamp(0., 174.)),
                            top: Val::Px((position.y - 3.).clamp(0., 174.)),
                            width: Val::Px(6.),
                            height: Val::Px(6.),
                            border: UiRect::all(Val::Px(1.)),
                            border_radius: BorderRadius::all(Val::Percent(50.)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.25, 1., 0.92)),
                        BorderColor::all(Color::WHITE),
                    ));
                }
            });
        panel
            .spawn(Node {
                width: Val::Percent(100.),
                height: Val::Px(14.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|legend_row| {
                for (label, color) in legend {
                    legend_row
                        .spawn(Node {
                            column_gap: Val::Px(4.),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|item| {
                            item.spawn((
                                Node {
                                    width: Val::Px(6.),
                                    height: Val::Px(6.),
                                    border_radius: BorderRadius::all(Val::Percent(50.)),
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                            item.spawn((
                                Text::new(label),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(9.),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.80, 0.87, 0.91)),
                            ));
                        });
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_directions_and_bounds_match_wasd() {
        assert_eq!(map_position(Vec3::ZERO, 24.), Vec2::splat(90.));
        assert_eq!(map_position(Vec3::new(-24., 5., -24.), 24.), Vec2::ZERO);
        assert_eq!(
            map_position(Vec3::new(30., -1., 30.), 24.),
            Vec2::splat(180.)
        );
    }
}
