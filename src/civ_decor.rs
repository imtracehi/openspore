//! Civilization: city decorations (the City Planner's Decorations page).
//!
//! RETAIL (installed data, `notes\civ-final-re.md` P2): the page
//! `Palette_Citydecoration_Parent~` (category `cityplanner_category_civicobjects`
//! "Decorations", 3 columns x 5 rows) lists the 15 civic objects of the
//! `CivicObjects` group (0x9430ADD7): flora containers large / medium /
//! small, fountains and lamps, three styles each; each costs `modelPrice` 25
//! (`CivicObjectTemplate`; tuning `CivicObjectCost`, read by the planner code
//! 0xD0AC10) and carries its `modelMeshLOD0..2` and `modelEffect` list (the
//! lamps' `civ_civicobjects_lamp1..3` lights, the fountains'
//! `civ_civicobjects_fountain1..3` water). `cCity` keeps them in
//! `mCivicObjects` (+0x308) placed by `mDecorationsLayout` (+0x450).
//!
//! RETAIL (`city_layout`): the spots are the walls model's authored
//! `Decorations` property (0x446FF4E, read by `cCityWalls::Load` 0xBEE5A0 into
//! `mDecorations`, then `cCommunityLayout::InitializeSlots` 4.0 at 0xBE5470):
//! 20 spots behind the small walls, 30 behind the large ones. ADAPTED:
//! decorations are cosmetic (no happiness rule found in the city model
//! 0xBE2590) and their fitted width.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::civ::CityMarker;
use crate::dbpf::ResourceKey;
use crate::game::RunProgress;
use crate::stages::{GameStage, StageOwned};
use crate::world_objects::name_id;
use crate::{material_binding, OriginalModel, Originals};

/// `CivicObjects` group (name hash of the group).
pub const CIVIC_GROUP: u32 = 0x9430_ADD7;
/// `CivicObjectTemplate` `modelPrice`.
pub const DECORATION_COST: f32 = 25.0;
/// The spot count of a decoration list that was never sized to a city (the
/// small walls' 20); a city's list is sized by `CityState::ensure_layout`.
pub const DEFAULT_SPOTS: usize = 20;
/// The city floor in the city root's frame (`city_presentation`'s root
/// clearance).
const DECOR_FLOOR: f32 = -2.0;
/// Fitted width of a decoration (island units, ADAPTED).
const DECOR_WIDTH: f32 = 0.9;

/// The page's items in page order (column-major as the item lists give:
/// column 0 rows 0..4, then column 1, then 2): (PROP instance name, the
/// installed `description`).
pub const ITEMS: [(&str, &str); 15] = [
    ("ci_objects_floracontainers_large1", "Giggles n Greens"),
    ("ci_objects_floracontainers_medium1", "Green Tentacle"),
    ("ci_objects_floracontainers_small1", "Pleather Shrub"),
    ("ci_objects_fountain1", "Festivity Fountain"),
    ("ci_objects_lamp1", "Irrepressible Illumination"),
    ("ci_objects_floracontainers_large2", "Bouncy Botany"),
    ("ci_objects_floracontainers_medium2", "Foliage Folly"),
    ("ci_objects_floracontainers_small2", "Back-Slapping Bush"),
    ("ci_objects_fountain2", "Slapstick Spring"),
    ("ci_objects_lamp2", "Light-Hearted"),
    ("ci_objects_floracontainers_large3", "Sim Flora"),
    ("ci_objects_floracontainers_medium3", "Blooms of Euphoria"),
    ("ci_objects_floracontainers_small3", "Snappy Sprout"),
    ("ci_objects_fountain3", "Gushing Smiles"),
    ("ci_objects_lamp3", "Lamp of Delight"),
];

/// The PROP of item `i`.
pub fn item_key(i: usize) -> Option<ResourceKey> {
    ITEMS.get(i).map(|(name, _)| ResourceKey {
        type_id: crate::asset_catalog::PROP,
        group_id: CIVIC_GROUP,
        instance_id: name_id(name),
    })
}

/// Pure: place item `item` on `spot` of a city's decorations (the list's
/// length is the city's spot count; a full city takes the first free spot
/// when `spot` is `None`). Returns the spot used.
pub fn place(decorations: &mut Vec<Option<u8>>, item: usize, spot: Option<usize>) -> Option<usize> {
    if item >= ITEMS.len() {
        return None;
    }
    if decorations.is_empty() {
        decorations.resize(DEFAULT_SPOTS, None);
    }
    let spots = decorations.len();
    let spot = match spot {
        Some(s) if s < spots => s,
        Some(_) => return None,
        None => decorations.iter().position(Option::is_none)?,
    };
    decorations[spot] = Some(item as u8);
    Some(spot)
}

/// One drawn decoration.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecorVisual {
    pub city: usize,
    pub spot: usize,
    pub item: u8,
}

pub struct CivDecorPlugin;

impl Plugin for CivDecorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_decorations
                .run_if(in_state(GameStage::Civilization))
                .run_if(resource_exists::<Messages<crate::swarm_fx::SwarmPlay>>),
        );
    }
}

/// The models and effects of an item, imported once.
#[derive(Default)]
pub struct DecorArt {
    models: HashMap<u8, Option<(OriginalModel, Vec<u32>)>>,
}

/// Draw each city's decorations (children of the city root) with their
/// installed models and effects; rebuild a spot when its item changes.
#[allow(clippy::too_many_arguments)]
fn sync_decorations(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cities: Query<(Entity, &CityMarker)>,
    shown: Query<(Entity, &DecorVisual)>,
    mut art: Local<DecorArt>,
    mut plays: MessageWriter<crate::swarm_fx::SwarmPlay>,
) {
    let mut have: HashMap<(usize, usize), u8> = HashMap::new();
    for (entity, visual) in &shown {
        let want = run
            .civ_campaign
            .cities
            .get(visual.city)
            .and_then(|c| c.decorations.get(visual.spot).copied().flatten());
        if want != Some(visual.item) {
            commands.entity(entity).despawn();
        } else {
            have.insert((visual.city, visual.spot), visual.item);
        }
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    for (city_entity, marker) in &cities {
        let Some(city) = run.civ_campaign.cities.get(marker.0) else {
            continue;
        };
        for (spot, item) in city.decorations.iter().enumerate() {
            let Some(item) = *item else { continue };
            if have.get(&(marker.0, spot)) == Some(&item) {
                continue;
            }
            let Some(key) = item_key(item as usize) else { continue };
            let entry = art
                .models
                .entry(item)
                .or_insert_with(|| {
                    let effects: Vec<u32> = originals
                        .resource_index
                        .properties(key)
                        .ok()
                        .map(|p| crate::cell_editor_data::key_list(&p, 0x02A9_07B5))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|k| k.instance_id)
                        .collect();
                    let name = ITEMS[item as usize].0;
                    let definition = crate::world_objects::definition_at(&originals, key, name).ok()?;
                    let (model, _) = crate::world_objects::import_at(
                        &mut originals,
                        definition,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                    )
                    .map_err(|e| warn!("civic object {name}: {e}"))
                    .ok()?;
                    Some((model, effects))
                })
                .clone();
            let site = city.decoration_site(spot);
            let local = Vec3::new(site.x, DECOR_FLOOR, site.y);
            let root = commands
                .spawn((
                    StageOwned,
                    DecorVisual { city: marker.0, spot, item },
                    Name::new(format!("city {} decoration {}", marker.0 + 1, spot + 1)),
                    Transform::from_translation(local),
                    Visibility::Inherited,
                    ChildOf(city_entity),
                ))
                .id();
            let Some((model, effects)) = entry else { continue };
            if let Ok(transform) = crate::world_objects::fitted_transform(&model, &meshes, Vec3::ZERO, DECOR_WIDTH) {
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
                        ChildOf(root),
                    ));
                }
            }
            // The object's own effects (the lamps' lights, the fountains'
            // water), at the decoration in the world.
            for effect in &effects {
                let mut p = crate::swarm_fx::SwarmPlay::at(*effect, Vec3::ZERO);
                p.attach_to = Some(root);
                p.tag = 0xDEC0_0000_0000_0000 | ((marker.0 as u64) << 8) | spot as u64;
                plays.write(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_fill_the_spots_and_name_the_installed_objects() {
        let mut d = Vec::new();
        assert_eq!(place(&mut d, 3, None), Some(0));
        assert_eq!(place(&mut d, 4, Some(5)), Some(5));
        assert_eq!(place(&mut d, 99, None), None);
        assert_eq!(d.len(), DEFAULT_SPOTS);
        assert_eq!(d[0], Some(3));
        for _ in 0..DEFAULT_SPOTS - 2 {
            place(&mut d, 0, None).unwrap();
        }
        assert_eq!(place(&mut d, 0, None), None, "every spot taken");
        assert_eq!(item_key(4).unwrap().instance_id, name_id("ci_objects_lamp1"));
        assert_eq!(item_key(0).unwrap().group_id, name_id("CivicObjects"));
    }
}
