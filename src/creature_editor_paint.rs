//! Paint mode of the retail Creature Creator (creator-b), on the pattern of
//! the Cell Creator's paint (`cell_paint.rs`, `cell_editor_paint.rs`).
//!
//! Data (VERIFIED, `notes/creator-re-notes.md` section 3): palette
//! `palettes~!0D` with the tabs Complete Styles (`ce_skinpaint_theme_*`: base,
//! coat and detail brushes with their colours) and the Partial Styles layers
//! Base / Coat / Detail (`ce_skinpaint_{base,coat,detail}_*`: one brush and
//! its colour), 3 x 4 pages with the `CreaturePaintIcons~` icons; the colour
//! row of `DefaultColorPickerConfig`; the default paint
//! `editorDefaultSkinPaintScripts` (blank matte base, blank coat and detail).
//! A creature's paint is three layers (brush + colour), saved with the
//! creation (`CreatureCreation::paint`).
//!
//! Applying it: every mounted part is re-tinted through its own tint mask
//! with the three colours (what retail does per part, `skinpaint.rs`); the
//! body skin takes the brush stamps of the retail pipeline
//! (`creature_skin_paint`, drawn by `creature_skin_view`), and the body atlas
//! the starter parts are cut from is re-coloured texel by texel from the
//! installed skin colours to the painted ones (`repaint_atlas`, ADAPTED: those
//! parts have no tint mask of their own).
//! ADAPTED: the colour row sets the colour of the shown layer (the base on
//! the Complete Styles tab); R randomizes (no retail control found in the
//! installed layouts); 1..4 switch the paint tabs (retail paint-tab keys).

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::ResourceIndex,
    cell_editor_data::{ints, key_list, prop_key, EDITOR_SETUP_GROUP},
    cell_editor_paint::{id as page_id, swatch_layout, tile_layout},
    cell_editor_ui::{id, image},
    cell_paint::{colors as color_list, paint_prop, same_color, CellPaintData, PaintTheme, PAINT_THEME_GROUP},
    creature_editor::{CreatureEditor, CreatureEditorOwned, Mode},
    creature_editor_data::{
        palette, prop_id::CATEGORY_ICONS, Palette, PaletteCategory, CATEGORY_GROUP_BASE, CREATURE_PAINT_PALETTE, EDITORS_CREATURE_GROUP,
        PNG,
    },
    creature_editor_ui::EditorRoot,
    dbpf::ResourceKey,
    editor::EditorState,
    game::{Player, RunProgress},
    spui::{
        ids::state,
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    world_objects::name_id,
    Originals,
};

/// Base, coat, detail.
pub const LAYERS: usize = 3;

/// The paint of a creature as saved with the creation.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CreaturePaint {
    /// False until the player paints (the creature keeps its installed look).
    pub painted: bool,
    /// Brush script ids (`skinpaint_<layer>_<name>`) of base, coat, detail.
    pub brushes: [u32; LAYERS],
    /// Colour per layer, 0..1 (gamma space, as the shaders take them).
    pub colors: [[f32; 3]; LAYERS],
    /// Last style chosen.
    pub theme: u32,
    /// `skineffectseed1..3` (`EditorModel::mSkinEffectSeeds`); `None` = the
    /// startup creature's ([`crate::creature_skin_paint::DEFAULT_SEEDS`]).
    /// A palette style sets every layer it paints to [`STYLE_SEED`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seeds: Option<[u32; LAYERS]>,
}

/// The seed a palette paint item gives every layer it paints (0x4D2 = 1234,
/// `FUN_004B2FB0` at 0x4B301A / 0x4B2CCB, VERIFIED `notes/limb-re-notes.md`
/// 6): a style always paints the same.
pub const STYLE_SEED: u32 = 1234;

impl CreaturePaint {
    /// The seeds the skin painter uses.
    pub fn effective_seeds(&self) -> [u32; LAYERS] {
        match (self.painted, self.seeds) {
            (true, Some(s)) => s,
            _ => crate::creature_skin_paint::DEFAULT_SEEDS,
        }
    }
}

/// One paint style of the palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub instance: u32,
    /// `paintThemePaints`: brush per layer (0 = this style leaves the layer).
    pub paints: [u32; LAYERS],
    /// `paintThemeColor1s`: colour per layer.
    pub colors: [Option<[f32; 3]>; LAYERS],
    pub icon: ResourceKey,
}

impl Theme {
    /// The one layer a partial style paints (`None` for a complete style).
    pub fn layer(&self) -> Option<usize> {
        let used: Vec<usize> = (0..LAYERS).filter(|&i| self.paints[i] != 0).collect();
        (used.len() == 1).then(|| used[0])
    }
}

#[derive(Resource, Clone, Debug, Default)]
pub struct CreaturePaintData {
    pub loaded: bool,
    pub palette: Palette,
    pub themes: HashMap<u32, Theme>,
    /// The same styles in the Cell Creator's shape, plus the colour row and
    /// the swatch images (for `cell_editor_paint`'s page and row layouts).
    pub cell: CellPaintData,
    /// The two top tabs: Complete Styles and Partial Styles
    /// (`ce_category_partial_themes`, whose layers Base / Coat / Detail are
    /// sub-tabs with the `EDT-icon-partialTheme-*` icons).
    pub top_tabs: Palette,
    pub layer_icons: Vec<ResourceKey>,
    /// `edt-button-partialTheme-{norm,hl,on-norm}` frames of the sub-tabs.
    pub layer_frames: Vec<ResourceKey>,
    /// `editorDefaultSkinPaintScripts`.
    pub defaults: [u32; LAYERS],
    pub errors: Vec<String>,
}

fn layer_key(i: u32) -> u32 {
    if i == 0xFFFF_FFFF {
        0
    } else {
        i
    }
}

impl CreaturePaintData {
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self { loaded: true, ..Self::default() };
        if index.records.is_empty() {
            return out;
        }
        out.palette = palette(index, &mut crate::cell_editor_data::Locale::default(), CREATURE_PAINT_PALETTE);
        let art = |name: &str| ResourceKey { type_id: PNG, group_id: EDITORS_CREATURE_GROUP, instance_id: name_id(name) };
        let mut partial = PaletteCategory { name: "Partial Styles".into(), ..Default::default() };
        if let Ok(p) = index.properties(prop_key(CATEGORY_GROUP_BASE | CREATURE_PAINT_PALETTE, name_id("ce_category_partial_themes"))) {
            partial.icons = key_list(&p, CATEGORY_ICONS).into_iter().map(|k| ResourceKey { type_id: PNG, ..k }).collect();
        }
        if partial.icons.is_empty() {
            partial.icons = ["norm", "off", "hl", "act", "on-norm", "off", "on-hl", "on-act"]
                .iter()
                .map(|s| art(&format!("button-layer-{s}")))
                .collect();
        }
        out.top_tabs = Palette { categories: out.palette.categories.iter().take(1).cloned().chain([partial]).collect(), ..Default::default() };
        // The sub-tab art lives in `editors~` (0x100D976E).
        let editors = |name: &str| ResourceKey { type_id: PNG, group_id: EDITORS_GROUP, instance_id: name_id(name) };
        out.layer_icons = ["base", "coat", "detail"].map(|l| editors(&format!("EDT-icon-partialTheme-{l}"))).to_vec();
        out.layer_frames = ["norm", "hl", "on-norm"].map(|s| editors(&format!("edt-button-partialTheme-{s}"))).to_vec();
        let icon_group = out.palette.thumbnail_group.unwrap_or(0);
        // Colour row and swatch art: the Cell Creator's picker config.
        let cell = CellPaintData::load(index);
        out.cell = CellPaintData { loaded: true, art: cell.art, swatches: cell.swatches, ..Default::default() };
        for item in out.palette.pages.iter().flat_map(|p| p.items.iter()) {
            if out.themes.contains_key(&item.part) {
                continue;
            }
            match index.properties(prop_key(PAINT_THEME_GROUP, item.part)) {
                Ok(p) => {
                    let paints = key_list(&p, paint_prop::THEME_PAINTS);
                    let colors = p.0.get(&paint_prop::THEME_COLOR1S).map(color_list).unwrap_or_default();
                    let theme = Theme {
                        instance: item.part,
                        paints: std::array::from_fn(|i| paints.get(i).map_or(0, |k| layer_key(k.instance_id))),
                        colors: std::array::from_fn(|i| colors.get(i).copied()),
                        icon: ResourceKey { type_id: PNG, group_id: icon_group, instance_id: item.part },
                    };
                    out.cell.themes.insert(
                        item.part,
                        PaintTheme {
                            instance: item.part,
                            name: String::new(),
                            paints: theme.paints.iter().copied().filter(|p| *p != 0).collect(),
                            colors: theme.colors.iter().flatten().copied().collect(),
                            icon: theme.icon,
                        },
                    );
                    out.themes.insert(item.part, theme);
                }
                Err(e) => out.errors.push(format!("paint style {:08X}: {e}", item.part)),
            }
        }
        if let Ok(p) = index.properties(prop_key(EDITOR_SETUP_GROUP, name_id("CreatureEditorExtraLarge"))) {
            let d = p.0.get(&paint_prop::DEFAULT_SCRIPTS).map(ints).unwrap_or_default();
            out.defaults = std::array::from_fn(|i| d.get(i).map_or(0, |v| *v as u32));
        }
        out
    }

    /// The layer a paint tab edits: `None` for Complete Styles, else the
    /// partial layer (categories in retail order: Complete, Base, Coat,
    /// Detail = `paletteCategorySkinPaintIndex` 0/1/2).
    pub fn tab_layer(&self, tab: usize) -> Option<usize> {
        (tab > 0 && tab <= LAYERS).then(|| tab - 1)
    }

    /// A click on a style: a complete style sets all three layers with its
    /// colours; a partial one its layer.
    pub fn choose_theme(&self, current: &CreaturePaint, native: [[f32; 3]; LAYERS], instance: u32) -> Option<CreaturePaint> {
        let t = self.themes.get(&instance)?;
        let mut p = self.started(current, native);
        let mut seeds = p.effective_seeds();
        match t.layer() {
            Some(l) => {
                p.brushes[l] = t.paints[l];
                seeds[l] = STYLE_SEED;
                if let Some(c) = t.colors[l] {
                    p.colors[l] = c;
                }
            }
            None => {
                for l in 0..LAYERS {
                    if t.paints[l] != 0 {
                        p.brushes[l] = t.paints[l];
                        seeds[l] = STYLE_SEED;
                    }
                    if let Some(c) = t.colors[l] {
                        p.colors[l] = c;
                    }
                }
            }
        }
        p.theme = instance;
        p.seeds = Some(seeds);
        Some(p)
    }

    /// A click on swatch `index` of the colour row while `tab` is shown.
    pub fn choose_color(&self, current: &CreaturePaint, native: [[f32; 3]; LAYERS], tab: usize, index: usize) -> Option<CreaturePaint> {
        let color = *self.cell.swatches.get(index)?;
        let mut p = self.started(current, native);
        p.colors[self.tab_layer(tab).unwrap_or(0)] = color;
        Some(p)
    }

    /// ADAPTED randomize: a random complete style with random colours from
    /// the colour row.
    pub fn randomize(&self, current: &CreaturePaint, native: [[f32; 3]; LAYERS], seed: u64) -> Option<CreaturePaint> {
        let mut complete: Vec<u32> = self.themes.values().filter(|t| t.layer().is_none()).map(|t| t.instance).collect();
        complete.sort();
        if complete.is_empty() || self.cell.swatches.is_empty() {
            return None;
        }
        let mut s = seed | 1;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let theme = complete[(next() % complete.len() as u64) as usize];
        let mut p = self.choose_theme(current, native, theme)?;
        for l in 0..LAYERS {
            p.colors[l] = self.cell.swatches[(next() % self.cell.swatches.len() as u64) as usize];
        }
        Some(p)
    }

    /// The paint to change: the current one, or the default brushes with the
    /// installed colours when never painted.
    fn started(&self, current: &CreaturePaint, native: [[f32; 3]; LAYERS]) -> CreaturePaint {
        if current.painted {
            current.clone()
        } else {
            CreaturePaint { painted: true, brushes: self.defaults, colors: native, theme: 0, seeds: current.seeds }
        }
    }

    /// Pages of a tab in `cell_editor_data` form.
    fn page(&self, tab: usize, page: usize) -> Option<crate::cell_editor_data::PalettePage> {
        let p = self.palette.page(tab, page)?;
        Some(crate::cell_editor_data::PalettePage {
            key: prop_key(0, p.instance),
            sequence: p.sequence,
            items: p
                .items
                .iter()
                .map(|i| crate::cell_editor_data::PaletteItem { part: i.part, row: i.row, column: i.column })
                .collect(),
            rows: p.rows,
            columns: p.columns,
            item_width_fraction: p.item_width_fraction,
        })
    }
}

// ---------------------------------------------------------------------------
// Pixels (pure)
// ---------------------------------------------------------------------------

fn luminance(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// Hue (degrees) and saturation of a colour (0..255 channels).
fn hue_sat(c: [f32; 3]) -> (f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    if max <= 0.0 || d <= 0.0 {
        return (0.0, 0.0);
    }
    let h = if max == c[0] {
        60.0 * ((c[1] - c[2]) / d)
    } else if max == c[1] {
        60.0 * ((c[2] - c[0]) / d) + 120.0
    } else {
        60.0 * ((c[0] - c[1]) / d) + 240.0
    };
    (h.rem_euclid(360.0), d / max)
}

/// ADAPTED body paint: each texel of the installed atlas is matched (by hue,
/// and by brightness: the installed base and coat may share a hue) to the
/// installed skin colours (base, coat, detail) and takes the painted colour
/// of the matching layers, scaled by its own brightness relative to the
/// installed colour; unsaturated texels (eyes, teeth, claws) stay.
pub fn repaint_atlas(rgba: &[u8], native: [[u8; 3]; LAYERS], painted: [[f32; 3]; LAYERS]) -> Vec<u8> {
    /// Hue tolerance of the skin (degrees); hue and brightness (ln of the
    /// ratio) tolerances of the split over the layers.
    const SKIN_HUE: f32 = 35.0;
    const HUE: f32 = 25.0;
    const BRIGHT: f32 = 0.25;
    let native_f: [[f32; 3]; LAYERS] = native.map(|c| c.map(|v| v as f32));
    let native_hue = native_f.map(|c| hue_sat(c).0);
    let native_lum = native_f.map(|c| luminance(c).max(1.0));
    let target = painted.map(|c| c.map(|v| v.clamp(0.0, 1.0) * 255.0));
    // A layer whose installed colour repeats an earlier one, or is a neutral
    // grey/white (indistinguishable from teeth, claws and eyes), is not
    // matched: its texels belong to the earlier layer or stay.
    let active: [bool; LAYERS] = std::array::from_fn(|l| {
        let c = native[l];
        let neutral = c.iter().max().unwrap() - c.iter().min().unwrap() < 24;
        let repeat = (0..l).any(|e| native[e].iter().zip(c.iter()).all(|(a, b)| a.abs_diff(*b) < 12));
        !neutral && !repeat
    });
    let mut out = rgba.to_vec();
    for p in out.chunks_exact_mut(4) {
        let c = [p[0] as f32, p[1] as f32, p[2] as f32];
        let (h, s) = hue_sat(c);
        let gate = ((s - 0.06) / 0.12).clamp(0.0, 1.0);
        if gate <= 0.0 {
            continue;
        }
        let lum = luminance(c);
        // How much the texel is skin (saturated, of a skin hue), then how it
        // splits over the layers (hue and brightness).
        let mut skin = 0.0f32;
        let mut split = [0.0f32; LAYERS];
        for l in (0..LAYERS).filter(|&l| active[l]) {
            let k = lum / native_lum[l];
            let dh = (h - native_hue[l]).rem_euclid(360.0);
            let dh = dh.min(360.0 - dh);
            skin = skin.max(gate * (-(dh / SKIN_HUE).powi(2)).exp());
            split[l] = (-(dh / HUE).powi(2) - (k.max(1e-3).ln() / BRIGHT).powi(2)).exp();
        }
        let total: f32 = split.iter().sum();
        if skin <= 1e-4 || total <= 1e-12 {
            continue;
        }
        for i in 0..3 {
            let painted: f32 = (0..LAYERS).map(|l| split[l] / total * target[l][i] * lum / native_lum[l]).sum();
            p[i] = (skin * painted + (1.0 - skin) * c[i]).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn rgb8(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Installed skin colours of the player baseline creature (`mSkinColor1/2/3`,
/// as `editor::import_definition` tints the parts).
pub fn native_skin_colors(originals: &Originals) -> [[u8; 3]; LAYERS] {
    originals
        .resource_index
        .read(ResourceKey {
            type_id: crate::creature_data::TYPE,
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
        })
        .ok()
        .and_then(|bytes| crate::creature_data::parse(&bytes).ok())
        .and_then(|data| data.skin_colors().ok())
        .map(crate::creature_data::CreatureData::quantize_skin_colors)
        .unwrap_or([[109, 173, 84], [76, 119, 58], [223, 188, 82]])
}

// ---------------------------------------------------------------------------
// Applying the paint to the creature
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct PaintCache {
    applied: Option<CreaturePaint>,
    native: Option<[[u8; 3]; LAYERS]>,
    /// Atlas images and their installed pixels (base level).
    atlas: HashMap<AssetId<Image>, Vec<u8>>,
    /// Images painted with `applied`.
    done: Vec<AssetId<Image>>,
}

fn write_base(image: &mut Image, base: Vec<u8>) {
    let (w, h) = (image.width(), image.height());
    let levels = image.texture_descriptor.mip_level_count;
    image.data = Some(if levels > 1 { crate::cell_paint::with_mips(&base, w, h, levels) } else { base });
}

/// Keep the creature's materials painted (any stage with the player; new
/// parts get painted when imported).
#[allow(clippy::too_many_arguments)]
pub fn apply(
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut cache: Local<PaintCache>,
    mut images: ResMut<Assets<Image>>,
    materials: Res<Assets<StandardMaterial>>,
    players: Query<&MeshMaterial3d<StandardMaterial>, With<Player>>,
) {
    // The outfitted creature shows its outfit's paint (`outfitter`).
    let paint = crate::outfitter::shown_paint(&run);
    if !paint.painted && cache.applied.is_none() {
        return;
    }
    if cache.applied.as_ref() != Some(paint) {
        cache.done.clear();
    }
    let native = *cache.native.get_or_insert_with(|| native_skin_colors(&originals));
    let colors = if paint.painted { paint.colors } else { native.map(|c| c.map(|v| v as f32 / 255.0)) };
    let texture = |m: &Handle<StandardMaterial>| materials.get(m).and_then(|m| m.base_color_texture.clone());
    // The body atlas and the starter parts cut from it.
    // The metaball skin is painted by its brushes (`creature_skin_view`).
    let mut atlas: Vec<Handle<Image>> =
        players.iter().filter(|m| !crate::creature_skin_view::is_skin_material(&originals, &m.0)).filter_map(|m| texture(&m.0)).collect();
    for model in originals.starter_parts.values() {
        atlas.extend(model.material.iter().chain(model.surfaces.iter().map(|s| &s.material)).filter_map(texture));
    }
    // Creature part models without their own diffuse + tint mask (cut from
    // a baked creature like the starter parts) are re-coloured the same way.
    for model in &originals.models {
        let Some(key) = model.source_definition.filter(|k| k.group_id == crate::asset_catalog::CREATURE_PARTS) else {
            continue;
        };
        let own = originals.part_catalog.parts.iter().find(|d| d.key == key).is_some_and(|d| d.diffuse.is_some());
        if !own || originals.starter_parts.contains_key(&key) {
            atlas.extend(model.material.iter().chain(model.surfaces.iter().map(|s| &s.material)).filter_map(texture));
        }
    }
    atlas.dedup_by_key(|h| h.id());
    for handle in atlas {
        let id = handle.id();
        if cache.done.contains(&id) {
            continue;
        }
        cache.done.push(id);
        let Some(mut image) = images.get_mut(&handle) else { continue };
        let (w, h) = (image.width(), image.height());
        let n = (w * h * 4) as usize;
        let Some(base) = cache
            .atlas
            .get(&id)
            .cloned()
            .or_else(|| image.data.as_ref().filter(|d| d.len() >= n).map(|d| d[..n].to_vec()))
        else {
            continue;
        };
        if !cache.atlas.contains_key(&id) && std::env::args().any(|a| a == "--creator-paint-demo") {
            // Capture diagnostics: the atlas's most common colours.
            let mut hist: HashMap<[u8; 3], u32> = HashMap::new();
            for p in base.chunks_exact(4).filter(|p| p[3] > 0) {
                *hist.entry([p[0] & 0xF0, p[1] & 0xF0, p[2] & 0xF0]).or_default() += 1;
            }
            let mut top: Vec<_> = hist.into_iter().collect();
            top.sort_by_key(|e| std::cmp::Reverse(e.1));
            top.truncate(8);
            info!("creator-paint atlas {w}x{h}: native {native:?} painted {colors:?} top {top:?}");
        }
        cache.atlas.entry(id).or_insert_with(|| base.clone());
        let pixels = if paint.painted { repaint_atlas(&base, native, colors) } else { base };
        write_base(&mut image, pixels);
    }
    // Every other mounted part model: its definition's diffuse + tint mask.
    let tint = colors.map(rgb8);
    for model in &originals.models {
        let Some(key) = model.source_definition.filter(|k| k.group_id == crate::asset_catalog::CREATURE_PARTS) else {
            continue;
        };
        if originals.starter_parts.contains_key(&key) {
            continue;
        }
        let Some(handle) = model.material.as_ref().and_then(texture) else { continue };
        if cache.done.contains(&handle.id()) {
            continue;
        }
        cache.done.push(handle.id());
        let Some(def) = originals.part_catalog.parts.iter().find(|d| d.key == key) else { continue };
        let read = |k: Option<ResourceKey>| {
            k.and_then(|k| originals.resource_index.read(k).ok()).and_then(|b| crate::texture::decode_rw4(&b))
        };
        let Some(diffuse) = read(def.diffuse) else { continue };
        let mask = read(def.tint);
        let painted = match (&originals.native_painter, mask.as_ref()) {
            (Some(painter), Some(m)) => painter.preview_part(&diffuse, m, tint).unwrap_or_else(|_| crate::texture::skinpaint(&diffuse, Some(m), tint)),
            _ => crate::texture::skinpaint(&diffuse, mask.as_ref(), tint),
        };
        if let Some(mut image) = images.get_mut(&handle) {
            if image.width() == painted.width && image.height() == painted.height {
                write_base(&mut image, painted.rgba);
            }
        }
    }
    cache.applied = Some(paint.clone());
}

// ---------------------------------------------------------------------------
// Paint palette UI
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintRoot {
    Tabs,
    Page,
    Swatches,
    Pager,
    /// Base / Coat / Detail sub-tabs of Partial Styles.
    Layers,
}

/// Control id of the sub-tab row host in the paint category layout.
const LAYER_ROW: u32 = 0x05D1_22C0;
/// `editors~` image group (sub-tab frames 52 x 27, layer icons 29 x 28).
const EDITORS_GROUP: u32 = 0x100D_976E;

/// The three layer sub-tabs: frame + icon buttons (control = layer + 1).
fn layers_layout(data: &CreaturePaintData, area: [f32; 4]) -> (crate::spui::model::Layout, Vec<(usize, usize)>) {
    use crate::spui::{
        ids::{class, flag},
        model::{ButtonData, Drawable, Extra, ImageDrawable, Layout, Window, WindowKind},
    };
    let pic = |key: ResourceKey| Some(Drawable::Image(ImageDrawable { image: Some(image(key)), ..Default::default() }));
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![crate::cell_editor_ui::plain(0, None, area, None, 0xFFFF_FFFF)];
    let mut ids = Vec::new();
    let (bw, bh) = (52.0f32.min(w / LAYERS as f32), 27.0f32.min(h));
    let y = ((h - bh) * 0.5).round();
    for l in 0..LAYERS {
        let x = ((l as f32 + 0.5) * w / LAYERS as f32 - bw * 0.5).round();
        let b = windows.len();
        windows.push(Window {
            class: class::BUTTON,
            kind: WindowKind::Button,
            flags: flag::VISIBLE | flag::ENABLED,
            control_id: l as u32 + 1,
            extra: Extra::Button(ButtonData::default()),
            ..crate::cell_editor_ui::plain(b, Some(0), [x, y, x + bw, y + bh], None, 0xFFFF_FFFF)
        });
        windows[0].children.push(b);
        let icon = windows.len();
        let ix = ((bw - 29.0) * 0.5).round();
        windows.push(crate::cell_editor_ui::plain(icon, Some(b), [ix, 0.0, ix + 29.0, bh], pic(data.layer_icons[l]), 0xFFFF_FFFF));
        let frame = windows.len();
        windows.push(crate::cell_editor_ui::plain(frame, Some(b), [0.0, 0.0, bw, bh], pic(data.layer_frames[0]), 0xFFFF_FFFF));
        windows[b].children = vec![icon, frame];
        ids.push((b, frame));
    }
    (Layout { version: 0, windows, roots: vec![0], hit_masks: vec![], warnings: vec![], unknown_classes: vec![] }, ids)
}

#[derive(Default)]
pub struct PaintUi {
    tab: usize,
    page: usize,
    built: Option<(usize, usize, [f32; 4])>,
    tabs: Vec<(usize, usize, usize)>,
    layers: Vec<(usize, usize)>,
    /// Last partial layer shown (the Partial Styles tab opens it).
    layer: usize,
    started: bool,
    tiles: Vec<(usize, usize, u32)>,
    swatches: Vec<(usize, usize)>,
    seed: u64,
}

fn rect_of(ui: &SpuiState, control: u32) -> Option<[f32; 4]> {
    let w = ui.find(control)?;
    let r = ui.rect(w);
    Some([r.x1, r.y1, r.x2, r.y2])
}

/// The paint palette: tabs, page of styles, colour row and page counter;
/// clicks paint the creature (one undo step each).
#[allow(clippy::too_many_arguments)]
pub fn paint_ui(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: Local<PaintUi>,
    mut data: ResMut<CreaturePaintData>,
    editor: Res<CreatureEditor>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    hosts: Query<(&SpuiState, &EditorRoot), Without<PaintRoot>>,
    mut roots: Query<(Entity, &mut SpuiState, &PaintRoot), Without<EditorRoot>>,
) {
    let index = &originals.resource_index;
    // The rigid-block editors have their own palettes (`rigid_paint`).
    let on = editor.mode == Mode::Paint && !editor.ui_hidden && !editor.kind.is_rigid();
    if !on {
        for (e, _, _) in &roots {
            commands.entity(e).despawn();
        }
        ui_state.built = None;
        return;
    }
    if !data.loaded && !index.records.is_empty() {
        *data = CreaturePaintData::load(index);
        for e in &data.errors {
            warn!("creature paint data: {e}");
        }
    }
    let tabs = data.palette.categories.len();
    if tabs == 0 {
        return;
    }
    let native = native_skin_colors(&originals).map(|c| c.map(|v| v as f32 / 255.0));
    if !ui_state.started {
        // Capture option `--creator-paint-tab N` (0 Complete, 1..3 layers).
        ui_state.started = true;
        let args: Vec<String> = std::env::args().collect();
        if let Some(t) = args.iter().position(|a| a == "--creator-paint-tab").and_then(|i| args.get(i + 1)?.parse::<usize>().ok()) {
            ui_state.tab = t.min(tabs - 1);
            ui_state.layer = ui_state.tab.saturating_sub(1);
        }
    }
    // Clicks and keys.
    let mut chosen: Option<CreaturePaint> = None;
    let mut clicks = Vec::new();
    for (_, mut ui, kind) in &mut roots {
        for w in ui.take_clicks() {
            clicks.push((*kind, ui.layout.window(w).control_id));
        }
    }
    let current = run.creature_creation.paint.clone();
    for (kind, control) in clicks {
        match (kind, control) {
            (PaintRoot::Tabs, 1) => {
                ui_state.tab = 0;
                ui_state.page = 0;
            }
            (PaintRoot::Tabs, 2) => {
                ui_state.tab = 1 + ui_state.layer;
                ui_state.page = 0;
            }
            (PaintRoot::Layers, c) if c >= 1 && (c as usize) <= LAYERS => {
                ui_state.layer = c as usize - 1;
                ui_state.tab = c as usize;
                ui_state.page = 0;
            }
            (PaintRoot::Page, theme) => chosen = data.choose_theme(&current, native, theme),
            (PaintRoot::Swatches, c) if c >= 1 => chosen = data.choose_color(&current, native, ui_state.tab, c as usize - 1),
            (PaintRoot::Pager, page_id::PAGE_PREV) => ui_state.page = ui_state.page.saturating_sub(1),
            (PaintRoot::Pager, page_id::PAGE_NEXT) => ui_state.page += 1,
            _ => {}
        }
    }
    if !editor.naming && !editor.options_open {
        for (i, k) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].iter().enumerate() {
            if keys.just_pressed(*k) && i < tabs {
                ui_state.tab = i;
                ui_state.layer = i.saturating_sub(1);
                ui_state.page = 0;
            }
        }
        if keys.just_pressed(KeyCode::KeyR) {
            ui_state.seed = ui_state.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            chosen = data.randomize(&current, native, ui_state.seed ^ editor.clock.to_bits() as u64);
        }
    }
    // Capture option `--creator-paint-demo`: one randomized paint.
    if !current.painted && std::env::args().any(|a| a == "--creator-paint-demo") {
        chosen = data.randomize(&current, native, 0x00C0_FFEE);
    }
    if let Some(p) = chosen.filter(|p| *p != current) {
        let before = run.clone();
        run.creature_creation.paint = p;
        state.record_external(before);
        run.request_save();
    }
    ui_state.tab = ui_state.tab.min(tabs - 1);
    let pages = data.palette.page_count(ui_state.tab).max(1);
    ui_state.page = ui_state.page.min(pages - 1);
    // Host rectangles from the creator's paint layouts.
    let (mut strip, mut area, mut swatch, mut pager, mut row) = (None, None, None, None, None);
    for (ui, kind) in &hosts {
        if !ui.ready {
            continue;
        }
        match kind {
            EditorRoot::PaintPalette => strip = rect_of(ui, id::TABS),
            EditorRoot::PaintCategory => {
                area = rect_of(ui, id::PAGE_AREA);
                swatch = rect_of(ui, page_id::SWATCH_AREA);
                pager = rect_of(ui, page_id::PAGE_HOST);
                row = rect_of(ui, LAYER_ROW);
            }
            _ => {}
        }
    }
    let (Some(strip), Some(area), Some(swatch), Ok(camera)) = (strip, area, swatch, camera.single()) else {
        return;
    };
    let key = (ui_state.tab, ui_state.page, area);
    if ui_state.built != Some(key) {
        for (e, _, _) in &roots {
            commands.entity(e).despawn();
        }
        let (l, tabs_ids, _) = crate::creature_editor_ui::tab_layout(&data.top_tabs, strip, 0);
        let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
        commands.entity(root).insert((PaintRoot::Tabs, CreatureEditorOwned));
        ui_state.tabs = tabs_ids;
        ui_state.layers.clear();
        if let (true, Some(row)) = (ui_state.tab > 0 && data.layer_icons.len() == LAYERS && data.layer_frames.len() == 3, row) {
            let (l, ids) = layers_layout(&data, row);
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 100);
            commands.entity(root).insert((PaintRoot::Layers, CreatureEditorOwned));
            ui_state.layers = ids;
        }
        if let Some(page) = data.page(ui_state.tab, ui_state.page) {
            let (l, tiles) = tile_layout(&data.cell, &page, area, &data.cell.art, vec![]);
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
            commands.entity(root).insert((PaintRoot::Page, CreatureEditorOwned));
            ui_state.tiles = tiles;
        }
        let (l, sw) = swatch_layout(&data.cell.art, &data.cell.swatches, swatch, vec![]);
        let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 100);
        commands.entity(root).insert((PaintRoot::Swatches, CreatureEditorOwned));
        ui_state.swatches = sw;
        if pages > 1 {
            if let Some(l) = assets.layout(index, "EditorPageControls") {
                let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert((PaintRoot::Pager, CreatureEditorOwned));
            }
        }
        ui_state.built = Some(key);
    }
    for c in &data.top_tabs.categories {
        for icon in c.icons.iter().take(8) {
            assets.texture(index, &mut images, *icon);
        }
    }
    for key in data.layer_icons.iter().chain(data.layer_frames.iter()) {
        assets.texture(index, &mut images, *key);
    }
    // States: tab icons, the chosen style and colour glows, the counter.
    let paint = &run.creature_creation.paint;
    let layer = data.tab_layer(ui_state.tab);
    let color = paint.colors[layer.unwrap_or(0)];
    for (_, mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            PaintRoot::Tabs => {
                let top = (ui_state.tab > 0) as usize;
                for (b, icon, ci) in &ui_state.tabs {
                    let st = ui.states[*b];
                    let Some(c) = data.top_tabs.categories.get(*ci) else { continue };
                    if let Some(file) = c.icon(*ci == top, st & state::HOVER != 0, st & state::CLICKED != 0, false) {
                        ui.set_image(*icon, Some(image(file)));
                    }
                    ui.set_tooltip(*b, Some(c.name.clone()));
                }
            }
            PaintRoot::Page => {
                for (_, glow, theme) in &ui_state.tiles {
                    let on = paint.painted
                        && match (layer, data.themes.get(theme)) {
                            (None, _) => paint.theme == *theme,
                            (Some(l), Some(t)) => t.layer() == Some(l) && paint.brushes[l] == t.paints[l],
                            _ => false,
                        };
                    if ui.visible(*glow) != on {
                        ui.set_visible(*glow, on);
                    }
                }
            }
            PaintRoot::Swatches => {
                for (i, (_, glow)) in ui_state.swatches.iter().enumerate() {
                    let on = paint.painted && data.cell.swatches.get(i).is_some_and(|s| same_color(*s, color));
                    if ui.visible(*glow) != on {
                        ui.set_visible(*glow, on);
                    }
                }
            }
            PaintRoot::Layers => {
                for (l, (b, frame)) in ui_state.layers.iter().enumerate() {
                    let hover = ui.states[*b] & state::HOVER != 0;
                    let f = if Some(l) == layer { 2 } else if hover { 1 } else { 0 };
                    ui.set_image(*frame, Some(image(data.layer_frames[f])));
                    ui.set_tooltip(*b, data.palette.categories.get(l + 1).map(|c| c.name.clone()));
                }
            }
            PaintRoot::Pager => {
                if let (Some(r), Some(host)) = (ui.layout.roots.first().copied(), pager) {
                    ui.set_area(r, host);
                }
                if let Some(w) = ui.find(page_id::PAGE_TEXT) {
                    ui.set_text(w, crate::cell_editor_paint::page_text(ui_state.page, pages));
                }
                if let Some(w) = ui.find(page_id::PAGE_PREV) {
                    ui.set_enabled(w, ui_state.page > 0);
                }
                if let Some(w) = ui.find(page_id::PAGE_NEXT) {
                    ui.set_enabled(w, ui_state.page + 1 < pages);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(instance: u32, paints: [u32; 3], colors: [Option<[f32; 3]>; 3]) -> Theme {
        Theme { instance, paints, colors, icon: ResourceKey { type_id: PNG, group_id: 0, instance_id: instance } }
    }

    fn data() -> CreaturePaintData {
        let mut d = CreaturePaintData { defaults: [11, 12, 13], ..Default::default() };
        d.themes.insert(1, theme(1, [21, 22, 23], [Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0]), Some([0.0, 0.0, 1.0])]));
        d.themes.insert(2, theme(2, [0, 32, 0], [None, Some([0.5, 0.5, 0.5]), None]));
        d.cell.swatches = vec![[0.2, 0.3, 0.4], [0.9, 0.9, 0.1]];
        d
    }

    #[test]
    fn complete_styles_set_every_layer_and_partial_styles_one() {
        let d = data();
        let native = [[0.4, 0.7, 0.3], [0.3, 0.5, 0.2], [0.9, 0.7, 0.3]];
        assert_eq!(d.themes[&1].layer(), None);
        assert_eq!(d.themes[&2].layer(), Some(1));
        let blank = CreaturePaint::default();
        let p = d.choose_theme(&blank, native, 2).unwrap();
        // Never painted: the default brushes and the installed colours, then
        // the coat layer of the partial style.
        assert!(p.painted);
        assert_eq!(p.brushes, [11, 32, 13]);
        assert_eq!(p.colors, [native[0], [0.5, 0.5, 0.5], native[2]]);
        // A palette item seeds the layers it paints with 1234; the others keep
        // the startup creature's seeds.
        let defaults = crate::creature_skin_paint::DEFAULT_SEEDS;
        assert_eq!(p.effective_seeds(), [defaults[0], STYLE_SEED, defaults[2]]);
        assert_eq!(blank.effective_seeds(), defaults);
        let q = d.choose_theme(&p, native, 1).unwrap();
        assert_eq!(q.effective_seeds(), [STYLE_SEED; 3]);
        assert_eq!(q.brushes, [21, 22, 23]);
        assert_eq!(q.colors[2], [0.0, 0.0, 1.0]);
        // The colour row paints the shown layer (base on Complete Styles).
        let r = d.choose_color(&q, native, 0, 1).unwrap();
        assert_eq!(r.colors[0], [0.9, 0.9, 0.1]);
        let s = d.choose_color(&q, native, 3, 0).unwrap();
        assert_eq!(s.colors[2], [0.2, 0.3, 0.4]);
        assert!(d.choose_color(&q, native, 0, 9).is_none());
        let a = d.randomize(&blank, native, 7).unwrap();
        assert!(a.painted && a.theme == 1);
        assert!(a.colors.iter().all(|c| d.cell.swatches.contains(c)));
    }

    #[test]
    fn atlas_repaint_maps_installed_skin_colours_and_keeps_the_rest() {
        let native = [[109, 173, 84], [76, 119, 58], [223, 188, 82]];
        let painted = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 1.0]];
        let px = |c: [u8; 3]| [c[0], c[1], c[2], 255];
        let mut rgba = Vec::new();
        rgba.extend(px([109, 173, 84])); // base
        rgba.extend(px([76, 119, 58])); // coat: same hue, darker
        rgba.extend(px([230, 230, 230])); // teeth
        let out = repaint_atlas(&rgba, native, painted);
        assert!(out[0] > 180 && out[1] < 60 && out[2] < 60, "{:?}", &out[0..4]);
        assert!(out[6] > 180 && out[4] < 60 && out[5] < 60, "{:?}", &out[4..8]);
        assert_eq!(&out[8..12], &[230, 230, 230, 255]);
        assert_eq!(out[3], 255);
        // The installed player: coat repeats the base, detail is white. The
        // skin takes the base colour; white texels stay.
        let installed = [[147, 87, 110], [147, 87, 110], [255, 255, 255]];
        let yellow = [[1.0, 0.9, 0.3], [0.5, 0.3, 0.9], [0.4, 0.8, 0.4]];
        let mut rgba = Vec::new();
        rgba.extend(px([128, 64, 80]));
        rgba.extend(px([250, 250, 250]));
        let out = repaint_atlas(&rgba, installed, yellow);
        assert!(out[0] > 150 && out[1] > 130 && out[2] < 90, "{:?}", &out[0..4]);
        assert_eq!(&out[4..8], &[250, 250, 250, 255]);
    }
}
