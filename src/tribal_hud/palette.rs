//! The tool planner: the retail `TribeToolsCategory` drawer with its three
//! category boxes ("Food and Healing", "Instruments", "Weapons"), one tile per
//! tool inside the box of its category.
//!
//! The layout is only the drawer: the retail code fills each category box with
//! a palette viewer over the tribe's tools (`ItemPlannerTribalTool`, window
//! `0x04DA6498` of the original, whose items are not described by any layout).
//! ADAPTED: the tiles are windows added to a copy of the drawer layout at run
//! time, cloned from the slot buttons of the stance trays of
//! `GlobalUITribe3-6` (the button face and icon slot) and from the price text of
//! `PlannerSwatch`; a tile shows the tool's installed palette picture (the
//! `PUI-icon-tool-*` PNGs), its price below, and takes the tool's state from
//! the campaign (locked, buildable, built, highlighted).

use std::sync::Arc;

use bevy::prelude::*;

use super::{TribalHudActive, TribalHudRoot, TribalHudState, ToolState};
use crate::{
    asset_catalog::ResourceIndex,
    spui::{
        model::{Drawable, ImageRef, Layout, WinProc},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    tribal_tools::{self as tools, ToolKind},
};

pub const PLANNER_LAYOUT: &str = "TribeToolsCategory";
/// The layout the tile windows are cloned from, and the price text source.
const PRICE_LAYOUT: &str = "PlannerSwatch";

/// Control ids of the drawer layout.
#[allow(dead_code)]
pub mod id {
    /// The drawer's fading window (the whole left column).
    pub const PANEL: u32 = 0x52DF_67AF;
    /// Window at the bottom of the drawer (the retail code's close area).
    pub const FOOTER: u32 = 0x92DF_6FD8;
    /// Synthetic control ids of the tiles and their price texts: the tool's
    /// palette index is added.
    pub const TILE_BASE: u32 = 0x7C10_0000;
    pub const PRICE_BASE: u32 = 0x7C11_0000;
    /// The slot button of the tray the tile is cloned from, and the price text
    /// of `PlannerSwatch`.
    pub const TILE_SOURCE: u32 = 0x0101_2060;
    pub const PRICE_SOURCE: u32 = 0x0642_BD0B;
}

/// File elements of the three category boxes (the grey windows under the
/// headers) in `TribeToolsCategory`.
pub const CATEGORY_ELEMENTS: [usize; 3] = [10, 15, 20];
/// Size of a tile (the slot button) and the pitch of a row; the grey area of a
/// box is 196 px wide and 87 px high at x 9, y 30 of the box.
const TILE: f32 = 38.0;
const PITCH: f32 = 48.0;
const GREY: (f32, f32, f32, f32) = (9.0, 30.0, 196.0, 87.0);
/// Height of the price text under a tile.
const PRICE_H: f32 = 14.0;
/// Colour of a tile whose tool cannot be built yet.
const LOCKED_TINT: u32 = 0x8040_4048;

/// Where tile `slot` of a category with `count` tiles starts (top-left, relative
/// to the category box): a centred row with room for the price below.
pub fn tile_position(slot: usize, count: usize) -> (f32, f32) {
    let row = count.max(1) as f32 * PITCH - (PITCH - TILE);
    let x = GREY.0 + (GREY.2 - row) * 0.5 + slot as f32 * PITCH;
    let y = GREY.1 + (GREY.3 - TILE - PRICE_H - 2.0) * 0.5;
    (x.round(), y.round())
}

/// The price text window's area under a tile.
pub fn price_area(slot: usize, count: usize) -> [f32; 4] {
    let (x, y) = tile_position(slot, count);
    [x - 5.0, y + TILE + 1.0, x + TILE + 5.0, y + TILE + 1.0 + PRICE_H]
}

/// The installed picture of a tool tile.
pub fn tile_icon(kind: ToolKind) -> Option<crate::dbpf::ResourceKey> {
    // The camp fire has no palette picture: the planner does not list it.
    let (group, name) = tools::info(kind).icon?;
    Some(crate::dbpf::ResourceKey {
        type_id: crate::ui_art::UI_PNG_TYPE,
        group_id: group,
        instance_id: crate::world_objects::name_id(name),
    })
}

/// The text under a tile: the price of a shack that can be built, a dash for
/// one that cannot yet, nothing for one that stands.
pub fn price_text(state: ToolState, price: f32) -> String {
    match state {
        ToolState::Available => format!("{price:.0}"),
        ToolState::Locked => "-".to_string(),
        ToolState::Built => String::new(),
    }
}

/// The derived drawer layout (built once).
#[derive(Resource, Default)]
pub struct PlannerLayout {
    layout: Option<Arc<Layout>>,
    tried: bool,
}

/// The drawer with a tile and a price text per tool added: `sections` is the
/// number of tiles in each of the three boxes. `None` while one of the source
/// layouts is not available.
pub fn derive_layout(assets: &mut SpuiAssets, index: &ResourceIndex, sections: [usize; 3]) -> Option<Arc<Layout>> {
    let base = assets.layout(index, PLANNER_LAYOUT)?;
    let main = assets.layout(index, super::MAIN_LAYOUT)?;
    let swatch = assets.layout(index, PRICE_LAYOUT)?;
    let tile_src = main.window(main.find(id::TILE_SOURCE)?).clone();
    let price_src = swatch.window(swatch.find(id::PRICE_SOURCE)?).clone();
    let mut layout = (*base).clone();
    // The hit mask of the tile button moves into the new layout.
    let mut tile_src = tile_src;
    let remap = |drawable: &mut Option<Drawable>, layout: &mut Layout| {
        let mask = match drawable {
            Some(Drawable::SporeStd { base, .. }) => &mut base.hit_mask,
            Some(Drawable::Std(base)) => &mut base.hit_mask,
            _ => return,
        };
        if let Some(old) = mask.take() {
            if let Some(m) = main.hit_masks.get(old).cloned() {
                layout.hit_masks.push(m);
                *mask = Some(layout.hit_masks.len() - 1);
            }
        }
    };
    remap(&mut tile_src.drawable, &mut layout);
    let layoutless = |w: &mut crate::spui::model::Window| {
        w.procs.retain(|p| !matches!(p, WinProc::SimpleLayout { .. } | WinProc::ProportionalLayout { .. }));
    };
    layoutless(&mut tile_src);
    let mut price_src = price_src;
    layoutless(&mut price_src);
    let mut next_tool = 0usize;
    for (category, &count) in CATEGORY_ELEMENTS.iter().zip(&sections) {
        let parent = layout.windows.iter().find(|w| w.element == *category)?.id;
        for slot in 0..count {
            let index_in_palette = next_tool;
            next_tool += 1;
            let (x, y) = tile_position(slot, count);
            let mut tile = tile_src.clone();
            tile.id = layout.windows.len();
            tile.element = 20_000 + index_in_palette;
            tile.parent = Some(parent);
            tile.children = Vec::new();
            tile.control_id = id::TILE_BASE + index_in_palette as u32;
            tile.area = [x, y, x + TILE, y + TILE];
            tile.flags |= crate::spui::ids::flag::VISIBLE | crate::spui::ids::flag::ENABLED;
            let tile_id = tile.id;
            layout.windows.push(tile);
            let mut price = price_src.clone();
            price.id = layout.windows.len();
            price.element = 21_000 + index_in_palette;
            price.parent = Some(parent);
            price.children = Vec::new();
            price.control_id = id::PRICE_BASE + index_in_palette as u32;
            price.area = price_area(slot, count);
            price.flags |= crate::spui::ids::flag::VISIBLE | crate::spui::ids::flag::ENABLED;
            let price_id = price.id;
            layout.windows.push(price);
            layout.windows[parent].children.insert(0, price_id);
            layout.windows[parent].children.insert(0, tile_id);
        }
    }
    Some(Arc::new(layout))
}

/// Spawn the planner root once (hidden until the planner is opened) and keep
/// it while the HUD is shown.
#[allow(clippy::too_many_arguments)]
pub fn manage_swatches(
    mut commands: Commands,
    active: Res<TribalHudActive>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut planner: ResMut<PlannerLayout>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<&TribalHudRoot>,
) {
    if !active.0 {
        planner.tried = false;
        return;
    }
    if roots.iter().any(|r| *r == TribalHudRoot::Planner) {
        return;
    }
    let index = &originals.resource_index;
    let Ok(camera) = camera.single() else {
        return;
    };
    if index.records.is_empty() || planner.tried {
        return;
    }
    planner.tried = true;
    let sections = super::PLANNER_SECTIONS.map(|s| s.len());
    let Some(layout) = derive_layout(&mut assets, index, sections) else {
        return;
    };
    assets.preload(index, &mut images, &layout);
    planner.layout = Some(layout.clone());
    let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 97);
    commands.entity(root).insert(TribalHudRoot::Planner);
}

/// The palette index of a tile control id.
pub fn tile_of(control: u32) -> Option<usize> {
    control.checked_sub(id::TILE_BASE).filter(|i| *i < 32).map(|i| i as usize)
}

/// Push the palette into the planner: the drawer opens and closes, every tile
/// shows its tool.
pub fn bind_palette(
    hud: Res<TribalHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<(&mut SpuiState, &TribalHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    for (mut ui, kind) in &mut roots {
        if *kind != TribalHudRoot::Planner || !ui.ready {
            continue;
        }
        let open = hud.planner_open && !hud.paused;
        if let Some(panel) = ui.find(id::PANEL) {
            ui.set_visible(panel, open);
        }
        for (i, entry) in hud.palette.iter().enumerate() {
            let (Some(tile), Some(price)) = (ui.find(id::TILE_BASE + i as u32), ui.find(id::PRICE_BASE + i as u32)) else {
                continue;
            };
            let key = tile_icon(entry.kind);
            let icon = key.and_then(|key| {
                assets.texture(index, &mut images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
            });
            ui.set_icon(tile, icon);
            ui.set_selected(tile, entry.highlighted);
            ui.set_enabled(tile, entry.state != ToolState::Locked);
            ui.set_tint(tile, if entry.state == ToolState::Locked { LOCKED_TINT } else { 0xFFFF_FFFF });
            ui.set_tooltip(tile, Some(super::tool_tooltip(entry)));
            ui.set_text(price, price_text(entry.state, entry.price));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_form_a_centred_row_inside_the_grey_area() {
        for count in 1..=4 {
            let first = tile_position(0, count);
            let last = tile_position(count - 1, count);
            let left = first.0 - GREY.0;
            let right = GREY.0 + GREY.2 - (last.0 + TILE);
            assert!((left - right).abs() <= 1.0, "{count} tiles are centred: {left} {right}");
            assert!(left >= 0.0, "{count} tiles fit the box");
            // The price text stays inside the box too.
            assert!(price_area(count - 1, count)[3] <= GREY.1 + GREY.3);
        }
        // The tiles of a row never overlap.
        let a = tile_position(0, 4);
        let b = tile_position(1, 4);
        assert!(b.0 - a.0 >= TILE);
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn every_tool_has_a_tile_picture_and_a_price_text() {
        for info in &tools::TOOLS {
            assert_eq!(tile_icon(info.kind).is_some(), info.kind != ToolKind::Campfire, "{:?}", info.kind);
        }
        assert_eq!(price_text(ToolState::Available, 25.0), "25");
        assert_eq!(price_text(ToolState::Locked, 25.0), "-");
        assert_eq!(price_text(ToolState::Built, 25.0), "");
        assert_eq!(tile_of(id::TILE_BASE + 4), Some(4));
        assert_eq!(tile_of(0x0101_2060), None);
    }

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn the_derived_drawer_has_a_tile_and_a_price_per_tool_in_its_box() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let sections = super::super::PLANNER_SECTIONS.map(|s| s.len());
        let layout = derive_layout(&mut assets, &index, sections).expect("derived layout");
        let base = assets.layout(&index, PLANNER_LAYOUT).unwrap();
        assert_eq!(layout.windows.len(), base.windows.len() + 2 * sections.iter().sum::<usize>());
        let mut palette = 0u32;
        for (category, count) in CATEGORY_ELEMENTS.iter().zip(sections) {
            let parent = layout.windows.iter().find(|w| w.element == *category).unwrap().id;
            for _ in 0..count {
                let tile = layout.window(layout.find(id::TILE_BASE + palette).expect("tile"));
                let price = layout.window(layout.find(id::PRICE_BASE + palette).expect("price"));
                assert_eq!(tile.parent, Some(parent));
                assert_eq!(price.parent, Some(parent));
                assert!(layout.window(parent).children.contains(&tile.id));
                assert_eq!(layout.window(tile.id).id, tile.id, "window ids are positions");
                assert!(!tile.procs.iter().any(|p| matches!(p, WinProc::SimpleLayout { .. })));
                assert!(tile.button().is_some(), "a tile is a button");
                palette += 1;
            }
        }
        // The headers of the boxes are still there.
        for category in CATEGORY_ELEMENTS {
            assert!(layout.windows.iter().any(|w| w.element == category));
        }
        assert!(layout.find(id::PANEL).is_some());
        // Every hit mask the tiles refer to exists.
        for w in &layout.windows {
            if let Some(Drawable::SporeStd { base, .. }) = &w.drawable {
                if let Some(mask) = base.hit_mask {
                    assert!(mask < layout.hit_masks.len());
                }
            }
        }
    }

    #[test]
    fn the_planner_sections_hold_every_tool_once() {
        let total: usize = super::super::PLANNER_SECTIONS.iter().map(|s| s.len()).sum();
        assert_eq!(total, tools::TOOLS.len() - 1, "every tool but the camp fire (it is not bought)");
        assert!(super::super::PLANNER_SECTIONS.iter().all(|s| (1..=4).contains(&s.len())));
    }
}
