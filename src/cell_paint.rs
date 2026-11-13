//! The paint of the player's cell (Cell Creator, Paint mode).
//!
//! Retail paints the cell with "complete styles" (`cl_skinpaint_cell_*` theme
//! PROPs in `paints~` 0x406A0200, two palette pages of twelve) and a colour
//! row (`ColorPickerConfig` `DefaultColorPickerConfig`). A style carries its
//! brush scripts (`paintThemePaints`, effects of the `editors` effect
//! directory) and a default colour per layer (`paintThemeColor1s`); a cell style
//! has one layer. Painting is free (manual p. 12).
//!
//! What is original here: the palette pages, the styles, their icons
//! (`cellpainticons~` PNGs), default colours, the colour row, the default style
//! (`editorDefaultSkinPaintScripts` = blankMatte), and since cell-paint2 the
//! brush scripts themselves: the body's diffuse atlas, its spec / bump map
//! (normal map with the painted specular level in the alpha) and its
//! `materialParams` come from the style's script decoded from the `editors`
//! effect directory and run by the skin painter (`cell_skinpaint`).
//!
//! What is ADAPTED: the body is the installed baked cell body with its own atlas
//! (not the metaball skin of the player's vertebrae), so the scripts run on that
//! mesh; when the `editors` directory or the body mesh is missing the old
//! stand-in remains as a fallback (the body atlas keeps its shading and is
//! re-coloured to the paint colour, the pattern of the style icon is projected
//! onto the body from above: `paint_body`, `ICON_PILL`, `PATTERN_STRENGTH`,
//! `BODY_DETAIL`). The mounted parts are repainted by retail's `PaintParts`
//! (cell-paint3, `cell_skinpaint::paint_part`: the installed
//! `skpSplatTintShader` with the three layer colours through the part's tint
//! mask, splatted by the diffuse alpha over the style's own brush stamps on the
//! part's chart, cell-paint5: `cell_skinpaint::paint_chart`); layer 1
//! is the paint colour and layers 2 / 3 are the editor model's own white
//! (`cell_skinpaint::MODEL_COLOR`, read from `EditorModel::EditorModel`
//! 0x4ABBB0: the Creator only ever sets layer 1; the installed cell tint masks
//! use only channel R, and channel B on the four filter mouths). The body's normal map is retail's `skpBumpToNormal`
//! executed from the installed shader pack. Without the installed shaders the
//! hand-written tint (`texture::skinpaint`) and the painter's own normals
//! remain as fallbacks.
//!
//! A never-painted cell keeps the installed baked body (INFERRED from what the
//! install ships: a pre-baked player body with its own atlas, i.e. retail's own
//! Skinner output for the starter, which is therefore not re-run through
//! blankMatte; the Creator's first paint click starts from the default style).
//!
//! The paint lives in `RunProgress.cell_creation.paint` and is applied in
//! place to the materials the Cell stage, the Creator and the HUD portrait
//! share (`CellPaintAssets::sync`), so every view shows the same cell.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::{ResourceIndex, CELL_PARTS},
    cell_skinpaint,
    creature_skin_paint::{Brush, Script},
    cell_editor_data::{
        ints, key_of, keys, palette_pages, prop_id, prop_key, PalettePage, EDITOR_SETUP_GROUP, PALETTE_GROUP, PNG,
    },
    dbpf::ResourceKey,
    game::RunProgress,
    prop::Property,
    world_objects::name_id,
    Originals,
};

/// `paints~`: the paint theme PROPs (`cl_skinpaint_cell_*`).
pub const PAINT_THEME_GROUP: u32 = 0x406A_0200;
/// `cellpainticons~`: the style icons (instance = theme PROP instance); the
/// palette names it in `palettePaletteThumbnailGroupID`.
pub const PAINT_ICON_GROUP: u32 = 0xB224_E099;
/// `ColorPickerConfig`: colour row definitions.
pub const COLOR_PICKER_GROUP: u32 = 0x9709_7E36;

pub mod paint_prop {
    /// `paintThemePaints`: the brush script ids of the layers.
    pub const THEME_PAINTS: u32 = 0xB0E0_66A7;
    /// `paintThemeColor1s`: the default colour of each layer.
    pub const THEME_COLOR1S: u32 = 0xB0E0_66A8;
    /// `palettePaletteThumbnailGroupID`.
    pub const THUMBNAIL_GROUP: u32 = 0x0223_3661;
    /// `editorDefaultSkinPaintScripts`.
    pub const DEFAULT_SCRIPTS: u32 = 0x05E6_8C2E;
    /// `colorpickerColors`.
    pub const PICKER_COLORS: u32 = 0xD296_75EC;
    /// `colorpickerImageGroupID` and the swatch images of the picker config.
    pub const PICKER_IMAGE_GROUP: u32 = 0xD296_75E0;
    pub const PICKER_FRAME: u32 = 0xD296_75E1;
    pub const PICKER_GLOW: u32 = 0xD296_75E2;
    pub const PICKER_SHINE: u32 = 0xD296_75E3;
    pub const PICKER_COLOR: u32 = 0xD296_75E5;
}

/// The two swatches `ColorPickerUI::Load` (0x5A5EB0) appends to the configured
/// colours: the custom colour (`colorpickerAddCustomColor`, starts mid grey)
/// and the default colour (`colorpickerAddDefaultColor`, drawn with the
/// `colorPickerImageDefault*` images). Both properties default to true and
/// `DefaultColorPickerConfig` sets neither, so the row has 15 + 2 = 17
/// swatches (the retail frame `creator-paint-mode.jpg` shows 17). VERIFIED
/// constants (0.5, 0.5, 0.5) and (1.0, 0.94, 0.83).
pub const CUSTOM_COLOR: [f32; 3] = [0.5, 0.5, 0.5];
pub const DEFAULT_SWATCH_COLOR: [f32; 3] = [1.0, 0.94, 0.83];
pub const EXTRA_SWATCHES: [[f32; 3]; 2] = [CUSTOM_COLOR, DEFAULT_SWATCH_COLOR];

/// Timing and sizes of the colour picker (`ColorPickerConfig`, read by
/// `ColorSwatchUI::Update` 0x5A63C0 / `GenerateExpansionArea` 0x5A4980 /
/// the shade function 0x5A4240). `Default` holds the code's fallbacks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerConfig {
    /// `colorpickerRolloverMaxTime`: seconds of the hover inflation ramp.
    pub rollover_max: f32,
    /// `colorpickerMouseDownMaxTime`: seconds a press is held before the
    /// shades open.
    pub mouse_down_max: f32,
    /// `colorpickerSelectMaxTime`: seconds of the selection glow ramp.
    pub select_max: f32,
    /// `colorpickerMaxInflation`: pixels a hovered swatch grows on every side.
    pub max_inflation: f32,
    /// `colorpickerExpansionCatchUpSpeed`: rate (1/s) of the chosen shade's
    /// flight back to its swatch.
    pub catch_up: f32,
    /// `colorpickerGapPercent`: gap between swatches as a fraction of a cell.
    pub gap: f32,
    /// `colorpickerDoubleClickInterval` (ms).
    pub double_click_ms: f32,
    /// `colorpickerNumExpansionSwatches`: the shades form an N x N grid.
    pub expansion_count: usize,
    /// `colorpickerExpansionSize`: side of the shade panel at an 800 pixel
    /// wide window (scaled by the window width / 800).
    pub expansion_size: f32,
    pub sat_max: f32,
    pub sat_exponent: f32,
    pub val_max: f32,
    pub val_exponent: f32,
    /// `colorPickerAllowExpansion`.
    pub allow_expansion: bool,
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            rollover_max: 0.25,
            mouse_down_max: 0.25,
            select_max: 0.25,
            max_inflation: 3.0,
            catch_up: 12.0,
            gap: 0.1,
            double_click_ms: 400.0,
            expansion_count: 5,
            expansion_size: 0.0,
            sat_max: 1.0,
            sat_exponent: 1.0,
            val_max: 1.0,
            val_exponent: 1.0,
            allow_expansion: true,
        }
    }
}

impl PickerConfig {
    pub fn from_prop(p: &crate::prop::PropertyList) -> Self {
        let mut c = Self::default();
        let num = |id: u32| p.0.get(&id).and_then(|x| x.number());
        for (id, v) in [
            (0xD296_75E6, &mut c.rollover_max),
            (0xD296_75E7, &mut c.mouse_down_max),
            (0xD297_BFC6, &mut c.select_max),
            (0xD296_75E8, &mut c.max_inflation),
            (0xD296_75EF, &mut c.catch_up),
            (0x0412_0847, &mut c.gap),
            (0xD296_75EA, &mut c.expansion_size),
            (0xB2FA_6D7B, &mut c.sat_max),
            (0xD296_75ED, &mut c.sat_exponent),
            (0x92FA_6B4B, &mut c.val_max),
            (0xD296_75EE, &mut c.val_exponent),
            (0x041C_3428, &mut c.double_click_ms),
        ] {
            if let Some(x) = num(id) {
                *v = x;
            }
        }
        if let Some(n) = num(0xD296_75EB) {
            c.expansion_count = (n as usize).max(1);
        }
        if let Some(b) = num(0x055A_A173) {
            c.allow_expansion = b != 0.0;
        }
        c
    }
}

/// The shades a held swatch opens (0x5A4240), index `i * n + j` for column
/// `i` (saturation) and row `j` (value). A grey swatch opens a ramp from
/// black to white `(n i + j) / (n^2 - 1)`; any other keeps only its hue:
/// saturation `(sat_max (i + 1) / n)^sat_exponent`, value
/// `(val_max (1 - j / n))^val_exponent`.
pub fn expansion_shades(color: [f32; 3], c: &PickerConfig) -> Vec<[f32; 3]> {
    let n = c.expansion_count.max(1);
    let nf = n as f32;
    let grey = color[0] == color[1] && color[0] == color[2];
    let hue = rgb_to_hue(color);
    let mut out = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            if grey {
                let v = (n * i + j) as f32 / (n * n - 1).max(1) as f32;
                out.push([v, v, v]);
            } else {
                // 1 - (n - i - 1) / n and 1 - j / n, as the code computes them.
                let s = (c.sat_max * (1.0 - (nf - i as f32 - 1.0) / nf)).max(0.0).powf(c.sat_exponent);
                let v = (c.val_max * (1.0 - j as f32 / nf)).max(0.0).powf(c.val_exponent);
                out.push(hsv_to_rgb(hue, s, v));
            }
        }
    }
    out
}

/// Hue in degrees of the 8-bit colour (the code truncates the channels to
/// bytes before its RGB -> HSV 0x918310).
fn rgb_to_hue(c: [f32; 3]) -> f32 {
    let [r, g, b] = c.map(|v| ((v * 255.0) as i32 & 0xFF) as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if d <= 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h / 60.0).rem_euclid(6.0);
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// ADAPTED: seconds after the last change of an edited body before its brush
/// script is run again (about 0.35 s of stamps and normals); meanwhile the body shows
/// a flat paint colour (retail meshes at `editorSkinResolutionRealtime` while an edit
/// is held and refines when idle; the creature skin uses the same 0.25 s,
/// `creature_skin_view::IDLE_SECONDS`).
pub const EDIT_IDLE: f32 = 0.25;

/// ADAPTED: surface of the body material (see `sync`; only Bevy-lit uses of the
/// material still see it).
pub const BODY_ROUGHNESS: f32 = 1.0;
pub const BODY_REFLECTANCE: f32 = 0.1;

/// How much of the body atlas's own shading survives the re-colouring
/// (ADAPTED fallback of the brush scripts; 0 = flat colour, 1 = the atlas's
/// full contrast).
pub const BODY_DETAIL: f32 = 0.55;

/// The paint of a cell as saved with the creation.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CellPaint {
    /// Instance of the chosen `cl_skinpaint_cell_*` style (0 = never painted:
    /// the cell keeps its installed look).
    pub theme: u32,
    /// Colour of the base layer, 0..1 per channel. Retail shades in gamma
    /// space, so the value is shown unconverted.
    pub color: [f32; 3],
}

impl CellPaint {
    pub fn is_painted(&self) -> bool {
        self.theme != 0
    }
}

/// One complete style of the paint palette.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PaintTheme {
    pub instance: u32,
    /// `description` of the PROP (`skinpaint_cell_amphibian`).
    pub name: String,
    /// `paintThemePaints`: brush script ids (0 entries dropped).
    pub paints: Vec<u32>,
    /// `paintThemeColor1s`: default colour per layer.
    pub colors: Vec<[f32; 3]>,
    pub icon: ResourceKey,
}

impl PaintTheme {
    /// Default colour of the base layer.
    pub fn color(&self) -> [f32; 3] {
        self.colors.first().copied().unwrap_or([1.0, 1.0, 1.0])
    }
}

/// Images of a colour swatch (`colorpickerImage*` of the picker config).
#[derive(Clone, Debug, Default)]
pub struct SwatchArt {
    pub frame: Option<ResourceKey>,
    pub glow: Option<ResourceKey>,
    pub shine: Option<ResourceKey>,
    pub color: Option<ResourceKey>,
    /// `colorPickerImageDefault*` / `colorpickerImageDefaultFrame*`: the
    /// default colour swatch's own images (0x5A51B0).
    pub default_color: Option<ResourceKey>,
    pub default_frame: Option<ResourceKey>,
    pub default_glow: Option<ResourceKey>,
    pub default_shine: Option<ResourceKey>,
    /// `colorpickerImageExpansionBackground`: behind the shades.
    pub expansion_background: Option<ResourceKey>,
}

fn swatch_art(p: &crate::prop::PropertyList) -> SwatchArt {
    let Some(group) = key_of(p, paint_prop::PICKER_IMAGE_GROUP).map(|k| k.instance_id) else {
        return SwatchArt::default();
    };
    let image = |id| {
        key_of(p, id).map(|k| ResourceKey {
            type_id: PNG,
            group_id: group,
            instance_id: k.instance_id,
        })
    };
    SwatchArt {
        frame: image(paint_prop::PICKER_FRAME),
        glow: image(paint_prop::PICKER_GLOW),
        shine: image(paint_prop::PICKER_SHINE),
        color: image(paint_prop::PICKER_COLOR),
        default_color: image(0x05AD_CD6F),
        default_frame: image(0x05AD_CD71),
        default_glow: image(0x05AD_CD72),
        default_shine: image(0x05AD_CD73),
        expansion_background: image(0xD296_75E4),
    }
}

/// Everything the Paint mode shows, read from the install.
#[derive(Clone, Debug, Default)]
pub struct CellPaintData {
    pub loaded: bool,
    pub palette: u32,
    pub pages: Vec<PalettePage>,
    pub themes: HashMap<u32, PaintTheme>,
    pub art: SwatchArt,
    /// The colour row: the picker config's colours plus [`EXTRA_SWATCHES`]
    /// (the custom and the default colour swatch, in that order).
    pub swatches: Vec<[f32; 3]>,
    /// Timing, sizes and shade rules of the picker.
    pub picker: PickerConfig,
    /// The row ends with the custom and default swatches ([`EXTRA_SWATCHES`]).
    pub extras: bool,
    /// Style a new cell starts with (`editorDefaultSkinPaintScripts`).
    pub default_theme: u32,
    pub errors: Vec<String>,
}

/// Every colour of a colour array property (12-byte little endian items).
pub fn colors(p: &Property) -> Vec<[f32; 3]> {
    p.values
        .iter()
        .filter(|v| p.kind == 0x32 && v.len() >= 12)
        .map(|v| [0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
        .collect()
}

fn load_theme(index: &ResourceIndex, icon_group: u32, instance: u32) -> Result<PaintTheme, String> {
    let p = index.properties(prop_key(PAINT_THEME_GROUP, instance))?;
    let paints = p
        .0
        .get(&paint_prop::THEME_PAINTS)
        .map(keys)
        .unwrap_or_default()
        .into_iter()
        .map(|k| k.instance_id)
        .collect::<Vec<_>>();
    Ok(PaintTheme {
        instance,
        name: p
            .0
            .get(&prop_id::DESCRIPTION)
            .and_then(|d| d.text())
            .map(|(_, _, t)| t)
            .unwrap_or_default(),
        // A cell style uses its first layer; the others are unused ids.
        paints: paints.into_iter().filter(|i| *i != 0 && *i != 0xFFFF_FFFF).collect(),
        colors: p.0.get(&paint_prop::THEME_COLOR1S).map(colors).unwrap_or_default(),
        icon: ResourceKey {
            type_id: PNG,
            group_id: icon_group,
            instance_id: instance,
        },
    })
}

impl CellPaintData {
    /// Read the Cell paint palette, its styles and the colour row.
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self {
            loaded: true,
            ..Self::default()
        };
        if index.records.is_empty() {
            return out;
        }
        let template = index.properties(prop_key(EDITOR_SETUP_GROUP, name_id("CellTemplate")));
        let (palette, scripts) = match &template {
            Ok(p) => (
                key_of(p, prop_id::EDITOR_PAINT_PALETTE).map(|k| k.instance_id).unwrap_or(6),
                p.0.get(&paint_prop::DEFAULT_SCRIPTS).map(ints).unwrap_or_default(),
            ),
            Err(e) => {
                out.errors.push(format!("CellTemplate: {e}"));
                (6, vec![])
            }
        };
        out.palette = palette;
        out.pages = palette_pages(index, palette);
        if out.pages.is_empty() {
            out.errors.push(format!("paint palette {palette}: no pages"));
        }
        let icon_group = index
            .properties(prop_key(PALETTE_GROUP, palette))
            .ok()
            .and_then(|p| key_of(&p, paint_prop::THUMBNAIL_GROUP))
            .map(|k| k.group_id)
            .filter(|g| *g != 0 && *g != 0xFFFF_FFFF)
            .unwrap_or(PAINT_ICON_GROUP);
        for item in out.pages.iter().flat_map(|p| p.items.iter()) {
            if out.themes.contains_key(&item.part) {
                continue;
            }
            match load_theme(index, icon_group, item.part) {
                Ok(t) => {
                    out.themes.insert(item.part, t);
                }
                Err(e) => out.errors.push(format!("paint theme {:08X}: {e}", item.part)),
            }
        }
        // The default style is the one whose first brush is the default script.
        out.default_theme = scripts
            .first()
            .and_then(|s| {
                out.pages
                    .iter()
                    .flat_map(|p| p.items.iter())
                    .find(|i| out.themes.get(&i.part).is_some_and(|t| t.paints.first() == Some(&(*s as u32))))
                    .map(|i| i.part)
            })
            .or_else(|| out.pages.first().and_then(|p| p.items.first()).map(|i| i.part))
            .unwrap_or(0);
        // The colour row: `DefaultColorPickerConfig.colorpickerColors`.
        let picker = index
            .properties(prop_key(COLOR_PICKER_GROUP, name_id("DefaultColorPickerConfig")))
            .ok();
        out.art = picker.as_ref().map(swatch_art).unwrap_or_default();
        out.picker = picker.as_ref().map(PickerConfig::from_prop).unwrap_or_default();
        out.swatches = picker
            .and_then(|p| p.0.get(&paint_prop::PICKER_COLORS).map(colors))
            .unwrap_or_default();
        if out.swatches.is_empty() {
            out.errors.push("DefaultColorPickerConfig: no colours".into());
        } else {
            out.swatches.extend(EXTRA_SWATCHES);
            out.extras = true;
        }
        out
    }

    pub fn theme(&self, instance: u32) -> Option<&PaintTheme> {
        self.themes.get(&instance)
    }

    /// Colour of a freshly painted cell (the default style's colour).
    pub fn default_color(&self) -> [f32; 3] {
        self.theme(self.default_theme)
            .map(PaintTheme::color)
            .unwrap_or([1.0, 0.988, 0.863])
    }

    /// The paint a click on a style makes: the style with its own colour.
    pub fn choose_style(&self, instance: u32) -> Option<CellPaint> {
        self.theme(instance).map(|t| CellPaint {
            theme: instance,
            color: t.color(),
        })
    }

    /// The paint a click on swatch `index` makes: the current style (the
    /// default one when the cell was never painted) in that colour.
    pub fn choose_color(&self, current: &CellPaint, index: usize) -> Option<CellPaint> {
        let color = *self.swatches.get(index)?;
        if Some(index) == self.default_index() {
            return Some(self.default_color_of(current));
        }
        Some(self.with_color(current, color))
    }

    /// The current style (the default one when never painted) in `color`.
    pub fn with_color(&self, current: &CellPaint, color: [f32; 3]) -> CellPaint {
        CellPaint {
            theme: if current.is_painted() { current.theme } else { self.default_theme },
            color,
        }
    }

    /// The default colour swatch: the style's own colour. INFERRED from
    /// 0x5A5950 (the picker's colour message carries "is default" instead of
    /// the swatch's colour when the default swatch is chosen).
    pub fn default_color_of(&self, current: &CellPaint) -> CellPaint {
        let theme = if current.is_painted() { current.theme } else { self.default_theme };
        CellPaint {
            theme,
            color: self.theme(theme).map(PaintTheme::color).unwrap_or_else(|| self.default_color()),
        }
    }

    /// Index of the custom colour swatch (one before the default one).
    pub fn custom_index(&self) -> Option<usize> {
        self.swatches.len().checked_sub(EXTRA_SWATCHES.len()).filter(|_| self.extras)
    }

    /// Index of the default colour swatch (the last one).
    pub fn default_index(&self) -> Option<usize> {
        (self.extras && self.swatches.len() >= EXTRA_SWATCHES.len()).then(|| self.swatches.len() - 1)
    }

    /// The swatch the paint colour is, if it is one of them (the Creator's
    /// selection, with shades and the custom swatch: `cell_editor_swatch`).
    #[cfg(test)]
    pub fn swatch_of(&self, paint: &CellPaint) -> Option<usize> {
        paint
            .is_painted()
            .then(|| self.swatches.iter().position(|c| same_color(*c, paint.color)))
            .flatten()
    }

    /// Page (index into `pages`) a style is on.
    pub fn page_of(&self, instance: u32) -> Option<usize> {
        self.pages.iter().position(|p| p.items.iter().any(|i| i.part == instance))
    }
}

pub fn same_color(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0 / 512.0)
}

// ---------------------------------------------------------------------------
// Pixels (pure)
// ---------------------------------------------------------------------------

/// Paint colour as the bytes the shaders write (gamma space, unconverted).
pub fn rgb8(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn luminance(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// A skin texel that holds colour. The atlas's alpha is a paint mask, not
/// coverage (it is 0 over much of the body), so unlit black counts as empty.
fn covered(p: &[u8]) -> bool {
    p[3] > 0 || p[0] as u32 + p[1] as u32 + p[2] as u32 > 24
}

/// Mean brightness of the covered texels of an RGBA8 image.
pub fn mean_luminance(rgba: &[u8]) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0u64);
    for p in rgba.chunks_exact(4).filter(|p| covered(p)) {
        sum += luminance(p) as f64;
        n += 1;
    }
    if n == 0 {
        128.0
    } else {
        (sum / n as f64) as f32
    }
}

/// Re-colour an RGBA8 skin atlas: every covered texel takes `color`, keeping
/// its brightness relative to `mean` (scaled by `detail`) so the atlas's own
/// shading and grain survive. Alpha is kept.
pub fn recolor(rgba: &[u8], mean: f32, color: [f32; 3], detail: f32) -> Vec<u8> {
    let target = rgb8(color).map(|v| v as f32);
    let mean = mean.max(1.0);
    let mut out = rgba.to_vec();
    for p in out.chunks_exact_mut(4).filter(|p| covered(p)) {
        let k = 1.0 + detail * (luminance(p) / mean - 1.0);
        for c in 0..3 {
            p[c] = (target[c] * k).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Mip chain (2x2 box filter) of an RGBA8 base level, `levels` levels long,
/// laid out level after level like Bevy's `Image::data`.
pub fn with_mips(base: &[u8], width: u32, height: u32, levels: u32) -> Vec<u8> {
    let mut out = base.to_vec();
    let (mut w, mut h) = (width as usize, height as usize);
    let mut prev = base.to_vec();
    for _ in 1..levels.max(1) {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |sx: usize, sy: usize| prev[(sy.min(h - 1) * w + sx.min(w - 1)) * 4 + c] as u32;
                    let (sx, sy) = (x * 2, y * 2);
                    next[(y * nw + x) * 4 + c] =
                        ((at(sx, sy) + at(sx + 1, sy) + at(sx, sy + 1) + at(sx + 1, sy + 1) + 2) / 4) as u8;
                }
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        w = nw;
        h = nh;
    }
    out
}

/// ADAPTED: the pill of the style icons (pixels `[x1, y1, x2, y2]` of the
/// 128 x 128 `cellpainticons~` pictures): the swatch cell seen from above with
/// its front to the left. The eyes cover its left end (no pattern there).
pub const ICON_PILL: [f32; 4] = [30.0, 20.0, 114.0, 88.0];
/// First icon column after the eyes (coverage left of it is dropped).
const ICON_EYES_END: f32 = 46.0;
/// How strongly the icon pattern shows on the body (ADAPTED).
pub const PATTERN_STRENGTH: f32 = 0.7;

/// The pattern of a style icon: where the picture is opaque, and whether the
/// pattern is lighter (true) or darker than the translucent body.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<f32>,
    pub lighten: bool,
}

/// Pattern of an RGBA8 style icon: the pill is translucent (alpha ~0.15) and
/// the marks of the style are opaque.
pub fn pattern_from_icon(rgba: &[u8], width: u32, height: u32) -> Pattern {
    let coverage: Vec<f32> = rgba
        .chunks_exact(4)
        .map(|p| ((p[3] as f32 - 48.0) / 160.0).clamp(0.0, 1.0))
        .collect();
    // Polarity: marks brighter than the body they sit on lighten it.
    let (mut marks, mut mark_n, mut body, mut body_n) = (0.0f64, 0u64, 0.0f64, 0u64);
    for y in ICON_PILL[1] as u32..(ICON_PILL[3] as u32).min(height) {
        for x in ICON_EYES_END as u32..(ICON_PILL[2] as u32).min(width) {
            let i = (y * width + x) as usize;
            let l = luminance(&rgba[i * 4..]) as f64;
            if coverage[i] > 0.5 {
                marks += l;
                mark_n += 1;
            } else if coverage[i] < 0.1 {
                body += l;
                body_n += 1;
            }
        }
    }
    let lighten = mark_n == 0 || body_n == 0 || marks / mark_n as f64 >= body / body_n as f64;
    Pattern {
        width,
        height,
        coverage,
        lighten,
    }
}

impl Pattern {
    /// Coverage at `(u, v)` over the icon pill (0..1 each, front left, top of
    /// the cell up), bilinear; 0 under the eyes and outside the picture.
    pub fn at(&self, u: f32, v: f32) -> f32 {
        let x = ICON_PILL[0] + u.clamp(0.0, 1.0) * (ICON_PILL[2] - ICON_PILL[0]);
        let y = ICON_PILL[1] + v.clamp(0.0, 1.0) * (ICON_PILL[3] - ICON_PILL[1]);
        let sample = |sx: i32, sy: i32| -> f32 {
            if sx < 0 || sy < 0 || sx >= self.width as i32 || sy >= self.height as i32 || (sx as f32) < ICON_EYES_END {
                0.0
            } else {
                self.coverage[sy as usize * self.width as usize + sx as usize]
            }
        };
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let top = sample(x0, y0) * (1.0 - tx) + sample(x0 + 1, y0) * tx;
        let bottom = sample(x0, y0 + 1) * (1.0 - tx) + sample(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// Grow the covered texels of a map by `passes` texels (each empty texel next
/// to covered ones takes the first neighbour's value): the atlas pads its
/// islands the same way, and bilinear sampling across an island border would
/// otherwise show a seam where the pattern stops.
pub fn dilate(map: &mut [Option<[f32; 2]>], width: u32, height: u32, passes: u32) {
    let (w, h) = (width as i64, height as i64);
    for _ in 0..passes {
        let before = map.to_vec();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if before[i].is_some() {
                    continue;
                }
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        if let Some(v) = before[(ny * w + nx) as usize] {
                            map[i] = Some(v);
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Top-view position `[x, z]` (mesh space) of every atlas texel the mesh
/// covers: its triangles rasterised in UV space. `None` where no triangle is.
pub fn texel_positions(
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
    width: u32,
    height: u32,
) -> Vec<Option<[f32; 2]>> {
    let mut out = vec![None; (width * height) as usize];
    let (w, h) = (width as f32, height as f32);
    for tri in indices.chunks_exact(3) {
        let idx = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if idx.iter().any(|i| *i >= positions.len() || *i >= uvs.len()) {
            continue;
        }
        let p: [[f32; 2]; 3] = idx.map(|i| [uvs[i][0] * w, uvs[i][1] * h]);
        let top = idx.map(|i| [positions[i][0], positions[i][2]]);
        let lo = |axis: usize| p.iter().map(|q| q[axis]).fold(f32::MAX, f32::min);
        let hi = |axis: usize| p.iter().map(|q| q[axis]).fold(f32::MIN, f32::max);
        let denom = (p[1][1] - p[2][1]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][1] - p[2][1]);
        if denom.abs() < 1e-9 {
            continue;
        }
        let (x0, x1) = (lo(0).floor().max(0.0) as u32, (hi(0).ceil() as u32).min(width.saturating_sub(1)));
        let (y0, y1) = (lo(1).floor().max(0.0) as u32, (hi(1).ceil() as u32).min(height.saturating_sub(1)));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let a = ((p[1][1] - p[2][1]) * (cx - p[2][0]) + (p[2][0] - p[1][0]) * (cy - p[2][1])) / denom;
                let b = ((p[2][1] - p[0][1]) * (cx - p[2][0]) + (p[0][0] - p[2][0]) * (cy - p[2][1])) / denom;
                let c = 1.0 - a - b;
                if a >= -1e-3 && b >= -1e-3 && c >= -1e-3 {
                    out[(y * width + x) as usize] = Some([
                        a * top[0][0] + b * top[1][0] + c * top[2][0],
                        a * top[0][1] + b * top[1][1] + c * top[2][1],
                    ]);
                }
            }
        }
    }
    out
}

/// A re-coloured body atlas with the pattern of a style projected from above.
/// `bounds` is the body `[x_min, x_max, z_min, z_max]`; `map` the top-view
/// positions of its texels ([`texel_positions`]).
pub fn paint_body(
    base: &[u8],
    mean: f32,
    color: [f32; 3],
    map: &[Option<[f32; 2]>],
    bounds: [f32; 4],
    pattern: Option<&Pattern>,
) -> Vec<u8> {
    let mut out = recolor(base, mean, color, BODY_DETAIL);
    let Some(pattern) = pattern else {
        return out;
    };
    let (dx, dz) = ((bounds[1] - bounds[0]).max(1e-6), (bounds[3] - bounds[2]).max(1e-6));
    for (p, at) in out.chunks_exact_mut(4).zip(map) {
        let Some([x, z]) = *at else {
            continue;
        };
        let c = pattern.at((z - bounds[2]) / dz, (bounds[1] - x) / dx) * PATTERN_STRENGTH;
        if c <= 0.0 {
            continue;
        }
        for v in &mut p[..3] {
            let value = *v as f32;
            *v = if pattern.lighten {
                value + (255.0 - value) * c * 0.6
            } else {
                value * (1.0 - 0.6 * c)
            }
            .round() as u8;
        }
    }
    out
}

/// Colours the parts' paint layers (base, coat, detail) get: the paint colour
/// on layer 1, the editor model's own default (white) on layers 2 / 3
/// (`cell_skinpaint::MODEL_COLOR`: the Cell Creator only ever sets layer 1).
/// Only the filter mouths' tint masks reach layer 3; none reaches layer 2.
pub fn part_colors(color: [f32; 3]) -> [[u8; 3]; 3] {
    cell_skinpaint::layer_colors(color).map(rgb8)
}

// ---------------------------------------------------------------------------
// Applying the paint to the shared materials
// ---------------------------------------------------------------------------

/// The body's baked atlas (original pixels kept to re-colour from).
struct BodyTarget {
    /// The body material and the atlas image it shows now.
    material: Handle<StandardMaterial>,
    image: Handle<Image>,
    base: Vec<u8>,
    width: u32,
    height: u32,
    levels: u32,
    mean: f32,
    /// Top-view position of each texel and the body `[x_min, x_max, z_min,
    /// z_max]` (for the pattern projection).
    map: Vec<Option<[f32; 2]>>,
    bounds: [f32; 4],
    /// The body as a skin for the retail brush scripts (`cell_skinpaint`);
    /// `None` keeps the icon-pattern stand-in.
    surface: Option<cell_skinpaint::BodySurface>,
    /// The installed spec / bump map of the body, the one now in the
    /// material, and its size / mip levels.
    normal: Option<NormalTarget>,
    /// The installed `materialParams` of the body (restored when unpainted).
    params: Option<crate::cell_sh::MaterialParams>,
}

/// The body material's spec / bump map (its alpha is the specular mask the SH
/// fragment multiplies in, `specNMap.a`).
struct NormalTarget {
    original: Handle<Image>,
    image: Handle<Image>,
    width: u32,
    height: u32,
    levels: u32,
}

/// `CELL_GLOBALS` (the Cell stage's globals resource; `cell_visuals` keeps the
/// player's body under this type id in `Originals::world_models`).
const CELL_GLOBALS: u32 = 0x2a3c_e5b7;

/// The dominant body bone of every vertex of the player's body mesh
/// (`u8::MAX` = none): the baked GMDL's own partition of the body into
/// vertebrae (body bone `i` = anatomy rigblock `i`, `notes\cell-animation-notes.md`
/// section 4). The fitted mesh keeps the GMDL's texture coordinates, so a
/// vertex takes the bone of the GMDL vertex with the same coordinates.
fn body_bones(originals: &Originals, uvs: &[[f32; 2]]) -> Option<Vec<u8>> {
    let creature = originals.world_models.keys().find(|k| k.type_id == CELL_GLOBALS)?.instance_id;
    let key = ResourceKey {
        type_id: crate::decode::TYPE_GMDL,
        group_id: crate::cell_visuals::CELL_MODEL_GROUP,
        instance_id: creature,
    };
    let raw = crate::baked_model::parse(&originals.resource_index.read(key).ok()?).ok()?;
    if raw.root_bones == 0 || raw.root_bones == u32::MAX || raw.bone_ranges.len() != raw.pieces.len() {
        return None;
    }
    let mut by_uv: HashMap<(u32, u32), u8> = HashMap::new();
    for (i, piece) in raw.pieces.iter().enumerate() {
        let base = raw.bone_ranges[i][0];
        for v in 0..piece.positions.len().min(piece.bones.len()).min(piece.weights.len()).min(piece.uvs.len()) {
            let w = piece.weights[v];
            let j = (0..4).filter(|j| w[*j] > 0.0).max_by(|a, b| w[*a].total_cmp(&w[*b]));
            let bone = j.map(|j| base + piece.bones[v][j] as u32 / 3).filter(|b| *b < raw.root_bones && *b < 255);
            if let Some(bone) = bone {
                by_uv.entry((piece.uvs[v][0].to_bits(), piece.uvs[v][1].to_bits())).or_insert(bone as u8);
            }
        }
    }
    let bones: Vec<u8> = uvs.iter().map(|uv| by_uv.get(&(uv[0].to_bits(), uv[1].to_bits())).copied().unwrap_or(u8::MAX)).collect();
    bones.iter().any(|b| *b != u8::MAX).then_some(bones)
}

/// Mesh arrays of a body surface -> the paintable skin (`scale` Bevy units per
/// skin unit); the vertebra partition is the baked body's own ([`body_bones`]),
/// or, for a body edited in the Creator (`cell_body`), the dominant vertebra of
/// every vertex of the skinned mesh (`edited`).
fn body_surface(
    meshes: &Assets<Mesh>,
    handle: &Handle<Mesh>,
    scale: f32,
    originals: &Originals,
    edited: Option<&[u8]>,
) -> Option<cell_skinpaint::BodySurface> {
    use bevy::mesh::{Indices, VertexAttributeValues as Values};
    let mesh = meshes.get(handle)?;
    let (Some(Values::Float32x3(positions)), Some(Values::Float32x3(normals)), Some(Values::Float32x2(uvs))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
    ) else {
        return None;
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|v| *v as u32).collect(),
    };
    let bones = match edited {
        Some(b) if b.len() == uvs.len() => Some(b.to_vec()),
        _ => body_bones(originals, uvs),
    };
    cell_skinpaint::BodySurface::build(positions, normals, uvs, &indices, scale, bones.as_deref())
}

/// A part model's mesh -> its chart (`scale` Bevy units per skin unit).
fn part_chart(meshes: &Assets<Mesh>, handle: &Handle<Mesh>, scale: f32) -> Option<cell_skinpaint::PartChart> {
    use bevy::mesh::{Indices, VertexAttributeValues as Values};
    let Some(mesh) = meshes.get(handle) else {
        debug!("Cell skin paint: part mesh asset not available");
        return None;
    };
    let (Some(Values::Float32x3(positions)), Some(Values::Float32x3(normals)), Some(Values::Float32x2(uvs))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
    ) else {
        debug!("Cell skin paint: part mesh lacks position / normal / uv attributes");
        return None;
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|v| *v as u32).collect(),
    };
    let chart = cell_skinpaint::PartChart::build(positions, normals, uvs, &indices, scale);
    if chart.is_none() {
        let uv = |k: usize, f: fn(f32, f32) -> f32, init: f32| uvs.iter().map(|u| u[k]).fold(init, f);
        info!(
            "Cell skin paint: part chart empty ({} vertices, {} triangles, scale {scale}, u {}..{}, v {}..{})",
            positions.len(),
            indices.len() / 3,
            uv(0, f32::min, f32::MAX),
            uv(0, f32::max, f32::MIN),
            uv(1, f32::min, f32::MAX),
            uv(1, f32::max, f32::MIN)
        );
    }
    chart
}

/// Top-view map of a body mesh's atlas texels and its bounds.
fn body_topview(
    meshes: &Assets<Mesh>,
    handle: &Handle<Mesh>,
    width: u32,
    height: u32,
) -> Option<(Vec<Option<[f32; 2]>>, [f32; 4])> {
    use bevy::mesh::{Indices, VertexAttributeValues as Values};
    let mesh = meshes.get(handle)?;
    let Some(Values::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let Some(Values::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        return None;
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|v| *v as u32).collect(),
    };
    let fold = |f: fn(f32, f32) -> f32, init: f32, axis: usize| positions.iter().map(|p| p[axis]).fold(init, f);
    let bounds = [
        fold(f32::min, f32::MAX, 0),
        fold(f32::max, f32::MIN, 0),
        fold(f32::min, f32::MAX, 2),
        fold(f32::max, f32::MIN, 2),
    ];
    let mut map = texel_positions(positions, uvs, &indices, width, height);
    dilate(&mut map, width, height, 3);
    Some((map, bounds))
}

/// A mounted part's painted texture: the authored diffuse and tint mask.
struct PartTarget {
    image: Handle<Image>,
    diffuse: crate::texture::Pixels,
    tint: Option<crate::texture::Pixels>,
    /// The part's model (RW4 record id, `PartInstance::instance`), its mesh and
    /// the scale it is mounted with (`OriginalModel::scale`).
    instance: u32,
    mesh: Handle<Mesh>,
    model_scale: f32,
    /// The part's own mesh as a chart of the atlas (the brush stamps of the
    /// style lie under the part's diffuse), built when the part is first
    /// mounted; `None` keeps the flat base.
    chart: Option<cell_skinpaint::PartChart>,
    chart_tried: bool,
    /// The layer colours, base colour, style, whether the part is mounted and
    /// whether the cell's visuals existed when this texture was last painted.
    painted: Option<([[u8; 3]; 3], [u8; 3], u32, bool, bool)>,
}

/// A part model's paintable target: its definition's authored diffuse and tint
/// mask (`None`: the model has no paintable texture).
fn capture_part(
    originals: &Originals,
    materials: &Assets<StandardMaterial>,
    model: &crate::OriginalModel,
) -> Option<(AssetId<Image>, PartTarget)> {
    let key = model.source_definition.filter(|k| k.group_id == CELL_PARTS)?;
    let handle = model
        .material
        .as_ref()
        .and_then(|m| materials.get(m))
        .and_then(|m| m.base_color_texture.clone())?;
    let definition = originals.part_catalog.by_definition_in(CELL_PARTS, key.instance_id)?;
    let read = |key: Option<ResourceKey>| {
        key.and_then(|k| originals.resource_index.read(k).ok())
            .and_then(|b| crate::texture::decode_rw4(&b))
    };
    let diffuse = read(definition.diffuse)?;
    Some((
        handle.id(),
        PartTarget {
            image: handle,
            diffuse,
            tint: read(definition.tint),
            instance: definition.model.instance_id,
            mesh: model.handle.clone(),
            model_scale: model.scale,
            chart: None,
            chart_tried: false,
            painted: None,
        },
    ))
}

/// The texture and material of one mounted part instance (`run.cell_parts[index]`).
/// Retail keeps one atlas rectangle per rigblock instance (`FUN_004CB890`, sized
/// by the instance's scale), so a pair of eyes carries two charts with their own
/// stamps; the shared definition material would show one texture on both.
struct InstanceTarget {
    /// The definition target (`CellPaintAssets::parts` key) whose paint it follows.
    definition: AssetId<Image>,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    /// Colours, base colour, style, mount scale (bits), draw salt and whether the
    /// cell's visuals existed when this texture was last painted.
    painted: Option<([[u8; 3]; 3], [u8; 3], u32, u32, u32, bool)>,
}

/// How long a part's scale must rest before its instance chart is repainted (the
/// wheel scales a part by a notch every few frames; the paint costs ~0.1 s per chart).
pub const INSTANCE_IDLE: f32 = 0.25;

/// Targets of the paint and what they show now.
#[derive(Resource, Default)]
pub struct CellPaintAssets {
    body: Option<BodyTarget>,
    parts: HashMap<AssetId<Image>, PartTarget>,
    /// Per-instance part textures by index into `run.cell_parts`
    /// ([`CellPaintAssets::instance_material`]).
    instances: HashMap<usize, InstanceTarget>,
    /// Part charts by definition texture and mount scale (bits).
    charts: HashMap<(AssetId<Image>, u32), Option<cell_skinpaint::PartChart>>,
    /// (index, mount scale bits, draw salt) of every instance the last time the
    /// scales changed, and when.
    instance_sig: Vec<(usize, u32, u32)>,
    instance_clock: Option<std::time::Instant>,
    /// Part textures already examined (captured or found unpaintable).
    parts_seen: std::collections::HashSet<AssetId<Image>>,
    /// The part models mounted on the cell when the parts were last painted
    /// (sorted): a part mounted later gets its chart painted.
    mounted: Vec<u32>,
    applied: Option<CellPaint>,
    /// `CellVisualAssets::body_revision` the body target follows (`cell_body`
    /// rewrote the shared body mesh when it changes).
    body_rev: u32,
    /// When the body mesh was last rewritten, and the revision the flat stand-in
    /// of the edit in progress was drawn for (see [`EDIT_IDLE`]).
    edit_clock: Option<std::time::Instant>,
    quick_rev: u32,
    data: Option<CellPaintData>,
    /// Style icon patterns by style (None: the icon could not be read).
    patterns: HashMap<u32, Option<Pattern>>,
    /// The retail brush scripts and what they painted last.
    skin: SkinState,
    /// Bumped after every repaint (views that cache pictures of the cell).
    pub revision: u32,
}

/// The decoded Cell brush scripts (`cell_skinpaint`), their brush textures and
/// the last painted result.
#[derive(Default)]
struct SkinState {
    library: Option<Result<cell_skinpaint::Library, String>>,
    /// Script of each style by theme instance (None: not exported / failed).
    scripts: HashMap<u32, Option<(Script, u32)>>,
    brushes: HashMap<String, Brush>,
    last: Option<((u32, [u32; 3]), cell_skinpaint::BodyTextures)>,
}

impl SkinState {
    /// Make sure the brush script of `theme` is decoded (cached, also when it
    /// is missing). `false`: the `editors` directory or the script is absent.
    fn ensure_script(&mut self, index: &ResourceIndex, theme: &PaintTheme) -> bool {
        let library = self.library.get_or_insert_with(|| cell_skinpaint::Library::load(index));
        let library = match library {
            Ok(l) => l,
            Err(e) => {
                if self.scripts.is_empty() {
                    warn!("Cell skin paint: {e}");
                    self.scripts.insert(0, None);
                }
                return false;
            }
        };
        let entry = self.scripts.entry(theme.instance).or_insert_with(|| {
            let id = *theme.paints.first()?;
            let script = library.script(id);
            if script.is_none() {
                warn!("Cell skin paint: script {id:08X} of {} is not in the editors directory", theme.name);
            }
            script.map(|s| (s, id))
        });
        entry.is_some()
    }

    /// The body textures of `theme` in `color` on `surface` (cached for the
    /// same style and colour). `native`: the installed pixel shaders, which
    /// derive the normal map (`skpBumpToNormal`).
    fn textures(
        &mut self,
        index: &ResourceIndex,
        theme: &PaintTheme,
        color: [f32; 3],
        surface: &cell_skinpaint::BodySurface,
        native: Option<&crate::skinpaint::NativePainter>,
    ) -> Option<cell_skinpaint::BodyTextures> {
        let key = (theme.instance, color.map(f32::to_bits));
        if let Some((k, t)) = &self.last {
            if *k == key {
                return Some(t.clone());
            }
        }
        if !self.ensure_script(index, theme) {
            return None;
        }
        let (script, id) = self.scripts.get(&theme.instance)?.as_ref()?;
        for name in script.brushes() {
            if !self.brushes.contains_key(&name) {
                let brush = cell_skinpaint::load_brush(index, &name);
                self.brushes.insert(name, brush);
            }
        }
        let started = std::time::Instant::now();
        let mut out = cell_skinpaint::paint_body(surface, script, *id, color, &self.brushes);
        let painted = started.elapsed().as_millis();
        if let Some(native) = native {
            out = out.with_native_normals(native);
        }
        info!(
            "Cell skin paint: {} ({} stamps) in {painted} ms, normals {} in {} ms",
            theme.name,
            out.stamps,
            if native.is_some() { "skpBumpToNormal" } else { "painter" },
            started.elapsed().as_millis() - painted
        );
        self.last = Some((key, out.clone()));
        Some(out)
    }

    /// The brush stamps of `theme` in `color` on a part's chart (the layer
    /// under the part's own diffuse), at [`cell_skinpaint::SIZE`] squared;
    /// `None` when the script is not available.
    fn chart_under(
        &mut self,
        index: &ResourceIndex,
        theme: &PaintTheme,
        color: [f32; 3],
        chart: &cell_skinpaint::PartChart,
        salt: u32,
    ) -> Option<crate::texture::Pixels> {
        if !self.ensure_script(index, theme) {
            return None;
        }
        let (script, id) = self.scripts.get(&theme.instance)?.as_ref()?;
        for name in script.brushes() {
            if !self.brushes.contains_key(&name) {
                let brush = cell_skinpaint::load_brush(index, &name);
                self.brushes.insert(name, brush);
            }
        }
        Some(cell_skinpaint::paint_chart(chart, script, *id, color, &self.brushes, salt))
    }

    /// What lies under the parts for `theme` (its script's clear colour);
    /// `None` when the script is not available.
    fn base_color(&mut self, index: &ResourceIndex, theme: &PaintTheme, colors: [[f32; 3]; 3]) -> Option<[f32; 3]> {
        if !self.ensure_script(index, theme) {
            return None;
        }
        let (script, _) = self.scripts.get(&theme.instance)?.as_ref()?;
        Some(cell_skinpaint::script_base_color(script, colors))
    }
}

impl CellPaintAssets {
    /// The default colour of a freshly painted cell (loaded on first use).
    pub fn default_color(&mut self, index: &ResourceIndex) -> [f32; 3] {
        self.data
            .get_or_insert_with(|| CellPaintData::load(index))
            .default_color()
    }

    /// The material of the part mounted as `run.cell_parts[index]`: a copy of its
    /// model's material with a texture of its own, painted by [`sync`] with that
    /// instance's chart (scale and draws). `None` keeps the shared definition
    /// material (the part has no paintable texture). The spawn sites of the cell's
    /// parts (the stage, the Creator) call this.
    pub fn instance_material(
        &mut self,
        index: usize,
        model: &crate::OriginalModel,
        originals: &Originals,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<StandardMaterial>> {
        let source = model.material.as_ref()?;
        let base = materials.get(source)?.clone();
        let texture = base.base_color_texture.clone()?;
        let id = texture.id();
        if !self.parts.contains_key(&id) {
            if !self.parts_seen.insert(id) {
                return None;
            }
            let (_, target) = capture_part(originals, materials, model)?;
            self.parts.insert(id, target);
        }
        if let Some(t) = self.instances.get(&index) {
            if t.definition == id {
                return Some(t.material.clone());
            }
        }
        let image = images.add(images.get(&texture)?.clone());
        let mut material = base;
        material.base_color_texture = Some(image.clone());
        let material = materials.add(material);
        // The copy keeps the source's `materialParams` (`cell_sh`).
        crate::cell_sh::copy_params(source.id(), material.id());
        self.instances.insert(index, InstanceTarget { definition: id, image, material: material.clone(), painted: None });
        Some(material)
    }
}

pub struct CellPaintPlugin;

impl Plugin for CellPaintPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CellPaintAssets>()
            .add_systems(Update, (demo, sync).chain());
    }
}

/// `--cell-paint-demo STYLE COLOUR` (captures): paint the cell with the
/// STYLE-th style (palette order) in the COLOUR-th swatch once the stage runs,
/// without opening the Creator (`-` keeps the style's own colour).
fn demo(mut run: ResMut<RunProgress>, originals: Res<Originals>, mut assets: ResMut<CellPaintAssets>, mut done: Local<bool>) {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a == "--cell-paint-demo") else {
        return;
    };
    if *done || originals.resource_index.records.is_empty() {
        return;
    }
    *done = true;
    let data = assets.data.get_or_insert_with(|| CellPaintData::load(&originals.resource_index));
    let style = args.get(at + 1).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let item = data.pages.iter().flat_map(|p| p.items.iter()).nth(style).map(|i| i.part);
    let Some(mut paint) = item.and_then(|i| data.choose_style(i)) else {
        return;
    };
    if let Some(color) = args.get(at + 2).and_then(|v| v.parse::<usize>().ok()).and_then(|c| data.swatches.get(c)) {
        paint.color = *color;
    }
    run.cell_creation.paint = paint;
}

/// The draw salt of `run.cell_parts[index]`: how many earlier instances of the same part
/// model (a pair's left and right files share the model and its texture) are mounted. 0 for the first, so a single part keeps the paint
/// the definition has ([`cell_skinpaint::paint_chart`]).
fn instance_salt(parts: &[crate::game::PartInstance], index: usize) -> u32 {
    let Some(p) = parts.get(index) else {
        return 0;
    };
    parts[..index].iter().filter(|q| q.instance == p.instance).count() as u32
}

/// Drop the instance textures that no longer belong to a mounted part and start the rest
/// timer of [`INSTANCE_IDLE`] when a mounted instance's scale or draw salt changed.
fn instances_follow_parts(assets: &mut CellPaintAssets, run: &RunProgress) {
    let CellPaintAssets { parts, instances, instance_sig, instance_clock, .. } = assets;
    instances.retain(|i, t| run.cell_parts.get(*i).is_some_and(|p| parts.get(&t.definition).is_some_and(|d| d.instance == p.instance)));
    let mut sig: Vec<(usize, u32, u32)> = instances
        .iter()
        .filter_map(|(i, t)| {
            let (placed, def) = (run.cell_parts.get(*i)?, parts.get(&t.definition)?);
            let mount = crate::cell_editor_scene::part_transform(placed, def.model_scale).scale.y.abs();
            Some((*i, mount.to_bits(), instance_salt(&run.cell_parts, *i)))
        })
        .collect();
    sig.sort_unstable();
    if *instance_sig != sig {
        // A new instance appears with `painted == None` and is painted at once; only an
        // instance that was already painted waits for its scale to rest.
        if instance_sig.iter().any(|old| sig.iter().any(|new| new.0 == old.0 && new != old)) {
            *instance_clock = Some(std::time::Instant::now());
        }
        *instance_sig = sig;
    }
}

/// Keep the shared body and part textures painted like `cell_creation.paint`.
/// New targets (a part model imported later) are captured as they appear.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    run: Res<RunProgress>,
    originals: Res<Originals>,
    visuals: Option<Res<crate::cell_visuals::CellVisualAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    meshes: Res<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: ResMut<CellPaintAssets>,
    cell_body: Res<crate::cell_body::CellBody>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    let mut changed = false;
    // The cell body: capture its atlas once (again if the stage built a new
    // body material).
    let current = visuals.as_deref().and_then(|v| v.body.as_ref()).and_then(|b| b.material.clone());
    if assets.body.as_ref().is_some_and(|b| Some(&b.material) != current.as_ref()) {
        assets.body = None;
    }
    if assets.body.is_none() {
        let body = visuals.as_deref().and_then(|v| v.body.as_ref());
        // ADAPTED: the body is a matt membrane (only the Bevy-lit uses of the
        // material still see this: the SH fragment replaces Bevy's PBR in the
        // Creator and, since cell-light2, on the stage). With the importer's default
        // roughness (0.7) Bevy's ambient specular adds a bright warm rim at
        // grazing angles that no paint colour hides; retail shows a rim of the
        // body's own colour.
        if let Some(mut material) = body.and_then(|b| b.material.as_ref()).and_then(|m| materials.get_mut(m)) {
            material.perceptual_roughness = BODY_ROUGHNESS;
            material.reflectance = BODY_REFLECTANCE;
        }
        if let Some(handle) = body
            .and_then(|b| b.material.as_ref())
            .and_then(|m| materials.get(m))
            .and_then(|m| m.base_color_texture.clone())
        {
            if let Some(image) = images.get(&handle) {
                let (w, h) = (image.width(), image.height());
                let base = image
                    .data
                    .as_ref()
                    .filter(|d| d.len() >= (w * h * 4) as usize)
                    .map(|d| d[..(w * h * 4) as usize].to_vec());
                if let Some(base) = base {
                    let mean = mean_luminance(&base);
                    let (map, bounds) = body
                        .and_then(|b| body_topview(&meshes, &b.draw_mesh(), w, h))
                        .unwrap_or_else(|| (vec![], [0.0; 4]));
                    let body_scale = visuals.as_deref().map_or(0.0, |v| v.body_scale);
                    let edited = visuals.as_deref().and_then(|v| v.body_edit.as_deref());
                    let surface = body.and_then(|b| body_surface(&meshes, &b.draw_mesh(), body_scale, &originals, edited));
                    if let Some(surface) = &surface {
                        info!("Cell skin paint: body {:?} skin units, {} vertebrae, atlas {w} x {h}", surface.extent, surface.vertebrae);
                        let (points, radii) = surface.spine();
                        info!("Cell skin paint: spine {points:?} radii {radii:?} bounds {:?}", surface.bounds());
                    }
                    let normal = body
                        .and_then(|b| b.material.as_ref())
                        .and_then(|m| materials.get(m))
                        .and_then(|m| m.normal_map_texture.clone())
                        .and_then(|handle| {
                            let image = images.get(&handle)?;
                            Some(NormalTarget {
                                width: image.width(),
                                height: image.height(),
                                levels: image.texture_descriptor.mip_level_count,
                                original: handle.clone(),
                                image: handle,
                            })
                        });
                    let params = body.and_then(|b| b.material.as_ref()).and_then(|m| crate::cell_sh::params_of(m.id()));
                    assets.body = Some(BodyTarget {
                        surface,
                        normal,
                        params,
                        material: body
                            .and_then(|b| b.material.clone())
                            .expect("the atlas came from the body material"),
                        levels: image.texture_descriptor.mip_level_count,
                        image: handle,
                        base,
                        width: w,
                        height: h,
                        mean,
                        map,
                        bounds,
                    });
                    assets.body_rev = visuals.as_deref().map_or(0, |v| v.body_revision);
                    changed = true;
                }
            }
        }
    }
    // The body mesh was rewritten by the Creator's spine scaling (`cell_body`):
    // the captured target keeps the baked pixels (an unedited body restores
    // them) but its surface, texel map and bounds follow the new mesh, and the
    // brush cache holds textures of the old surface.
    let revision = visuals.as_deref().map_or(0, |v| v.body_revision);
    if revision != assets.body_rev {
        assets.body_rev = revision;
        let body = visuals.as_deref().and_then(|v| v.body.as_ref());
        let edited = visuals.as_deref().and_then(|v| v.body_edit.as_deref());
        let body_scale = visuals.as_deref().map_or(0.0, |v| v.body_scale);
        let size = assets.body.as_ref().map(|b| (b.width, b.height));
        if let (Some(body), Some((w, h))) = (body, size) {
            let handle = body.draw_mesh();
            let (map, bounds) = body_topview(&meshes, &handle, w, h).unwrap_or_else(|| (vec![], [0.0; 4]));
            let surface = body_surface(&meshes, &handle, body_scale, &originals, edited);
            if let Some(target) = assets.body.as_mut() {
                target.map = map;
                target.bounds = bounds;
                target.surface = surface;
            }
        }
        assets.skin.last = None;
        assets.edit_clock = Some(std::time::Instant::now());
        changed = true;
    }
    // Mounted part models: authored diffuse + tint mask of their definition.
    for model in &originals.models {
        if !model.source_definition.is_some_and(|k| k.group_id == CELL_PARTS) {
            continue;
        }
        let Some(handle) = model
            .material
            .as_ref()
            .and_then(|m| materials.get(m))
            .and_then(|m| m.base_color_texture.clone())
        else {
            continue;
        };
        // Every part texture is looked at once (a part without paintable
        // textures is not retried every frame).
        if !assets.parts_seen.insert(handle.id()) {
            continue;
        }
        let Some((id, target)) = capture_part(&originals, &materials, model) else {
            continue;
        };
        assets.parts.insert(id, target);
        changed = true;
    }
    let mut mounted: Vec<u32> = run.cell_parts.iter().map(|p| p.instance).collect();
    mounted.sort_unstable();
    mounted.dedup();
    if assets.mounted != mounted {
        assets.mounted = mounted;
        changed = true;
    }
    // The mounted instances: a new one is painted at once, a changed scale or draw order
    // once its part rests ([`INSTANCE_IDLE`]; the wheel changes it every few frames).
    instances_follow_parts(&mut assets, &run);
    if assets.instances.values().any(|t| t.painted.is_none()) || assets.instance_clock.is_some() {
        changed = true;
    }
    let paint = run.cell_creation.paint.clone();
    if !changed && assets.applied.as_ref() == Some(&paint) {
        return;
    }
    let default = assets.default_color(&originals.resource_index);
    // The pattern of the style (read from its icon once).
    if paint.is_painted() && !assets.patterns.contains_key(&paint.theme) {
        let pattern = assets
            .data
            .as_ref()
            .and_then(|d| d.theme(paint.theme))
            .and_then(|t| originals.resource_index.read(t.icon).ok())
            .and_then(|bytes| crate::ui_art::decode_png(&bytes).ok())
            .map(|(w, h, rgba)| pattern_from_icon(&rgba, w, h));
        assets.patterns.insert(paint.theme, pattern);
    }
    // Unpainted cells keep the installed body; their parts use the default
    // style's colour (retail's creator always paints with it).
    let part_color = if paint.is_painted() { paint.color } else { default };
    let colors = part_colors(part_color);
    // The retail brush script of the style on the body (`cell_skinpaint`);
    // without it (no `editors` directory, no surface) the icon pattern below.
    // An edited body (`cell_body`) has no baked atlas: an unpainted one is painted with the
    // neutral cell's own paint, which the baked starter is the output of (VERIFIED: its
    // anatomy names `skinpaint_cell_amphibian` and a colour, `CellBody::neutral_paint`);
    // without that script, with the default style.
    let edited = visuals.as_deref().is_some_and(|v| v.body_edit.is_some());
    let body_paint = match assets.data.as_ref() {
        Some(data) if edited && !paint.is_painted() => cell_body
            .neutral_paint
            .and_then(|(script, color)| {
                data.themes
                    .values()
                    .find(|t| t.paints.first() == Some(&script))
                    .map(|t| CellPaint { theme: t.instance, color })
            })
            .or_else(|| data.choose_style(data.default_theme))
            .unwrap_or_else(|| paint.clone()),
        _ => paint.clone(),
    };
    // An edit in progress shows a flat body (drawn once per change) and waits for it to be idle.
    let pending = edited && assets.edit_clock.is_some_and(|t| t.elapsed().as_secs_f32() < EDIT_IDLE);
    let flat_drawn = pending && assets.quick_rev == revision;
    let skin = if pending {
        None
    } else {
        let CellPaintAssets { body, skin, data, .. } = &mut *assets;
        let theme = data.as_ref().and_then(|d| d.theme(body_paint.theme));
        match (body_paint.is_painted(), body.as_ref().and_then(|b| b.surface.as_ref()), theme) {
            (true, Some(surface), Some(theme)) => {
                skin.textures(&originals.resource_index, theme, body_paint.color, surface, originals.native_painter.as_ref())
            }
            _ => None,
        }
    };
    let body_job = assets.body.as_ref().filter(|_| !flat_drawn).map(|body| {
        let rgba = if edited && skin.is_none() {
            // No brush script for the edited surface: flat paint colour (the baked atlas has
            // another layout).
            let [r, g, b] = rgb8(body_paint.color);
            [r, g, b, 255].repeat((body.width * body.height) as usize)
        } else if !paint.is_painted() && !edited {
            body.base.clone()
        } else if let Some(skin) = &skin {
            cell_skinpaint::resample(&skin.diffuse, (cell_skinpaint::SIZE, cell_skinpaint::SIZE), (body.width, body.height))
        } else {
            let pattern = assets.patterns.get(&paint.theme).and_then(Option::as_ref);
            paint_body(&body.base, body.mean, paint.color, &body.map, body.bounds, pattern)
        };
        (body.image.clone(), body.material.clone(), body.width, body.height, body.levels, rgba)
    });
    if let Some((image, material, width, height, levels, rgba)) = body_job {
        // Developer aid: `OS_CELL_PAINT_DUMP=<dir>` writes the painted atlas
        // and the spec / bump map (alpha = specular level).
        if let Some(dir) = std::env::var_os("OS_CELL_PAINT_DUMP") {
            let write = |name: &str, w: u32, h: u32, data: &[u8]| {
                if let Ok(file) = std::fs::File::create(std::path::Path::new(&dir).join(name)) {
                    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    if let Ok(mut writer) = encoder.write_header() {
                        let _ = writer.write_image_data(data);
                    }
                }
            };
            write("cell-paint-body.png", width, height, &rgba);
            if let Some(skin) = &skin {
                write("cell-paint-normal.png", cell_skinpaint::SIZE, cell_skinpaint::SIZE, &skin.normal_spec);
            }
        }
        // The body's settings: the brush script's (or the installed ones back).
        {
            let params = match (&skin, assets.body.as_ref().and_then(|b| b.params)) {
                (Some(skin), _) => Some(skin.material_params()),
                (None, installed) => installed,
            };
            if let Some(params) = params {
                crate::cell_sh::set_params(material.id(), params);
            }
        }
        // The spec / bump map: the painted normal map with the specular level
        // in its alpha, or the installed one back for an unpainted cell.
        if let Some(target) = assets.body.as_mut().and_then(|b| b.normal.as_mut()) {
            let fresh = match &skin {
                Some(skin) => images.get(&target.original).cloned().map(|mut painted| {
                    let data = cell_skinpaint::resample(
                        &skin.normal_spec,
                        (cell_skinpaint::SIZE, cell_skinpaint::SIZE),
                        (target.width, target.height),
                    );
                    painted.data = Some(with_mips(&data, target.width, target.height, target.levels));
                    images.add(painted)
                }),
                // The edited surface has no baked bump map: a flat one without specular.
                None if edited => images.get(&target.original).cloned().map(|mut flat| {
                    let data = [128, 128, 255, 0].repeat((target.width * target.height) as usize);
                    flat.data = Some(with_mips(&data, target.width, target.height, target.levels));
                    images.add(flat)
                }),
                None => Some(target.original.clone()),
            };
            if let Some(fresh) = fresh {
                if fresh != target.image {
                    if let Some(mut m) = materials.get_mut(&material) {
                        m.normal_map_texture = Some(fresh.clone());
                    }
                    if target.image != target.original {
                        images.remove(&target.image);
                    }
                    target.image = fresh;
                }
            }
        }
        // A fresh image asset: rewriting the data of the live one only reaches
        // the GPU's first mip level, so grazing views of the body (its rim)
        // would keep sampling the old colours.
        if let Some(mut painted) = images.get(&image).cloned() {
            painted.data = Some(with_mips(&rgba, width, height, levels));
            let fresh = images.add(painted);
            if let Some(mut m) = materials.get_mut(&material) {
                m.base_color_texture = Some(fresh.clone());
            }
            if let Some(body) = assets.body.as_mut() {
                body.image = fresh;
            }
            images.remove(&image);
        }
    }
    // The parts: retail's PaintParts (`cell_skinpaint::paint_part`: the
    // installed tint shader with the three layer colours, splatted over what
    // the script's clear leaves under the part). Without the installed
    // shaders the old hand-written tint (`texture::skinpaint`) remains.
    let part_theme = {
        let data = assets.data.as_ref();
        if paint.is_painted() { paint.theme } else { data.map_or(0, |d| d.default_theme) }
    };
    let base = {
        let CellPaintAssets { skin, data, .. } = &mut *assets;
        let layers = cell_skinpaint::layer_colors(part_color);
        data.as_ref()
            .and_then(|d| d.theme(part_theme))
            .and_then(|t| skin.base_color(&originals.resource_index, t, layers))
            .unwrap_or(part_color)
    };
    let native = originals.native_painter.as_ref();
    let CellPaintAssets { parts, skin, data, instances, charts, instance_clock, .. } = &mut *assets;
    let body_scale = visuals.as_deref().map_or(0.0, |v| v.body_scale);
    for part in parts.values_mut() {
        // Only the parts mounted on the cell get their chart painted (a style change
        // would otherwise paint all ~60 installed parts).
        let mounted = run.cell_parts.iter().any(|p| p.instance == part.instance);
        let key = (colors, rgb8(base), part_theme, mounted, body_scale > 0.0);
        if part.painted == Some(key) {
            continue;
        }
        if mounted && !part.chart_tried && body_scale > 0.0 {
            // The part's size in skin units: its mesh scaled the way the Creator and the
            // stage mount it (`cell_editor_scene::part_transform`, instance scale 1) over
            // the body's fit. The definition's own texture (the carried preview, the HUD portrait)
            // takes this default-scale chart; every mounted instance has its own (below).
            part.chart_tried = true;
            let mount = crate::cell_editor_scene::part_transform(&crate::game::PartInstance::new(0, 0), part.model_scale).scale.y;
            if mount > 0.0 {
                part.chart = part_chart(&meshes, &part.mesh, body_scale / mount);
            }
            info!("Cell skin paint: part model {:08X} chart {}", part.instance, if part.chart.is_some() { "built" } else { "unavailable" });
        }
        // What lies under the part: the style's brush stamps on its chart (retail's
        // `-parts` distributes, flag 4), resampled to the part texture.
        let under = part
            .chart
            .as_ref()
            .filter(|_| mounted)
            .zip(data.as_ref().and_then(|d| d.theme(part_theme)))
            .and_then(|(chart, theme)| skin.chart_under(&originals.resource_index, theme, part_color, chart, 0))
            .map(|chart| crate::texture::Pixels {
                width: part.diffuse.width,
                height: part.diffuse.height,
                rgba: cell_skinpaint::resample(&chart.rgba, (cell_skinpaint::SIZE, cell_skinpaint::SIZE), (part.diffuse.width, part.diffuse.height)),
            });
        let painted = native
            .and_then(|n| {
                cell_skinpaint::paint_part(n, &part.diffuse, part.tint.as_ref(), base, under.as_ref(), colors)
                    .map_err(|e| warn!("Cell PaintParts: {e}"))
                    .ok()
            })
            .unwrap_or_else(|| crate::texture::skinpaint(&part.diffuse, part.tint.as_ref(), colors));
        if let Some(mut image) = images.get_mut(&part.image) {
            if image.width() == painted.width && image.height() == painted.height && image.texture_descriptor.mip_level_count == 1 {
                image.data = Some(painted.rgba);
            }
        }
        part.painted = Some(key);
    }
    // One texture per mounted part instance (retail: one atlas rectangle per rigblock
    // instance, `FUN_004CB890`): the instance's own chart (its mount scale), its own
    // draws. The first instance of a definition at the default scale is the definition's
    // own paint (above).
    let resting = instance_clock.is_none_or(|t| t.elapsed().as_secs_f32() >= INSTANCE_IDLE);
    let mut repainted = true;
    let order: Vec<usize> = instances.keys().copied().collect();
    for index in order {
        let (Some(inst), Some(placed)) = (instances.get(&index), run.cell_parts.get(index)) else {
            continue;
        };
        let Some(def) = parts.get(&inst.definition) else {
            continue;
        };
        let mount = crate::cell_editor_scene::part_transform(placed, def.model_scale).scale.y.abs();
        let default_mount = crate::cell_editor_scene::part_transform(&crate::game::PartInstance::new(0, 0), def.model_scale).scale.y;
        let salt = instance_salt(&run.cell_parts, index);
        let key = (colors, rgb8(base), part_theme, mount.to_bits(), salt, body_scale > 0.0);
        if inst.painted == Some(key) {
            continue;
        }
        if inst.painted.is_some() && !resting {
            repainted = false;
            continue;
        }
        let (definition, image) = (inst.definition, inst.image.clone());
        let shared = salt == 0 && (mount - default_mount).abs() < 1e-6;
        let pixels = if shared || body_scale <= 0.0 {
            // The definition's own paint.
            images.get(&def.image).and_then(|i| i.data.clone()).map(|rgba| (def.diffuse.width, def.diffuse.height, rgba))
        } else {
            let chart = charts
                .entry((definition, mount.to_bits()))
                .or_insert_with(|| part_chart(&meshes, &def.mesh, body_scale / mount));
            let under = chart
                .as_ref()
                .zip(data.as_ref().and_then(|d| d.theme(part_theme)))
                .and_then(|(chart, theme)| skin.chart_under(&originals.resource_index, theme, part_color, chart, salt))
                .map(|chart| crate::texture::Pixels {
                    width: def.diffuse.width,
                    height: def.diffuse.height,
                    rgba: cell_skinpaint::resample(&chart.rgba, (cell_skinpaint::SIZE, cell_skinpaint::SIZE), (def.diffuse.width, def.diffuse.height)),
                });
            let painted = native
                .and_then(|n| {
                    cell_skinpaint::paint_part(n, &def.diffuse, def.tint.as_ref(), base, under.as_ref(), colors)
                        .map_err(|e| warn!("Cell PaintParts: {e}"))
                        .ok()
                })
                .unwrap_or_else(|| crate::texture::skinpaint(&def.diffuse, def.tint.as_ref(), colors));
            Some((painted.width, painted.height, painted.rgba))
        };
        if let Some((w, h, rgba)) = pixels {
            let own = images.get(&def.image).and_then(|i| i.data.as_ref()).map_or(0, |d| d.iter().zip(&rgba).filter(|(a, b)| a != b).count());
            info!(
                "Cell skin paint: part instance {index} (model {:08X}) mount {mount:.3}, draws {salt}: {own} bytes differ from the definition's paint",
                def.instance
            );
            // Developer aid: `OS_CELL_PAINT_DUMP=<dir>` also writes every part instance's texture.
            if let Some(dir) = std::env::var_os("OS_CELL_PAINT_DUMP") {
                if let Ok(file) = std::fs::File::create(std::path::Path::new(&dir).join(format!("part-{index}-{:08X}.png", def.instance))) {
                    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    if let Ok(mut writer) = encoder.write_header() {
                        let _ = writer.write_image_data(&rgba);
                    }
                }
            }
            if let Some(mut target) = images.get_mut(&image) {
                if target.width() == w && target.height() == h && target.texture_descriptor.mip_level_count == 1 {
                    target.data = Some(rgba);
                }
            }
        }
        if let Some(inst) = instances.get_mut(&index) {
            inst.painted = Some(key);
        }
    }
    if repainted {
        *instance_clock = None;
    }
    if pending {
        // Come back when the edit is idle, then run the script.
        assets.quick_rev = revision;
        assets.applied = None;
    } else {
        assets.applied = Some(paint);
    }
    assets.revision = assets.revision.wrapping_add(1);
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_saves_with_defaults_for_old_creations() {
        let paint: CellPaint = serde_json::from_str("{}").unwrap();
        assert_eq!(paint, CellPaint::default());
        assert!(!paint.is_painted());
        let paint = CellPaint {
            theme: 7,
            color: [0.5, 0.25, 1.0],
        };
        let round: CellPaint = serde_json::from_str(&serde_json::to_string(&paint).unwrap()).unwrap();
        assert_eq!(round, paint);
        assert!(round.is_painted());
    }

    #[test]
    fn recolor_keeps_alpha_and_relative_brightness() {
        // Two covered texels (bright, dark; the second has alpha 0 like most of
        // the real atlas) and one empty.
        let atlas = [200, 160, 100, 255, 100, 80, 50, 0, 3, 3, 3, 0];
        let mean = mean_luminance(&atlas);
        assert!((mean - (luminance(&atlas[0..4]) + luminance(&atlas[4..8])) / 2.0).abs() < 1e-3);
        let out = recolor(&atlas, mean, [0.5, 0.25, 1.0], 1.0);
        assert_eq!([out[3], out[7], out[11]], [255, 0, 0], "alpha is kept");
        assert_eq!(&out[8..12], &[3, 3, 3, 0], "uncovered texels are untouched");
        // The brighter texel stays brighter and keeps the paint's hue order.
        assert!(out[0] > out[4] && out[2] > out[6], "bright stays bright");
        assert!(out[2] > out[0] && out[0] > out[1], "blue > red > green like (0.5, 0.25, 1.0)");
        // Without detail the covered texels are flat paint.
        let flat = recolor(&atlas, mean, [0.5, 0.25, 1.0], 0.0);
        assert_eq!(&flat[0..3], &[128, 64, 255]);
        assert_eq!(&flat[4..7], &[128, 64, 255]);
    }

    #[test]
    fn mip_chain_has_every_level_and_averages() {
        let base: Vec<u8> = (0..4 * 4).flat_map(|i| [(i * 16) as u8, 0, 0, 255]).collect();
        let chain = with_mips(&base, 4, 4, 3);
        assert_eq!(chain.len(), 4 * 4 * 4 + 2 * 2 * 4 + 4);
        // Level 1 texel (0,0) averages base texels 0, 1, 4, 5.
        let expected = ((0 + 16 + 64 + 80 + 2) / 4) as u8;
        assert_eq!(chain[64], expected);
        // Level 2 (1x1) is the mean of level 1.
        assert_eq!(chain[64 + 16 + 3], 255);
        // A single level is the base unchanged.
        assert_eq!(with_mips(&base, 4, 4, 1), base);
    }

    #[test]
    fn icon_patterns_are_opaque_marks_on_a_translucent_pill() {
        // A 128 x 128 icon: translucent dark pill (alpha 30) with an opaque
        // bright stripe at column 90.
        let (w, h) = (128u32, 128u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 20..88 {
            for x in 30..114 {
                let i = ((y * w + x) * 4) as usize;
                let stripe = x == 90;
                rgba[i..i + 4].copy_from_slice(&if stripe { [220, 220, 255, 255] } else { [20, 20, 30, 30] });
            }
        }
        let pattern = pattern_from_icon(&rgba, w, h);
        assert!(pattern.lighten, "bright marks lighten the body");
        // u = (90.5 - 30) / 84: the stripe; elsewhere nothing.
        let on = pattern.at((90.5 - 30.0) / 84.0, 0.5);
        assert!(on > 0.9, "{on}");
        assert_eq!(pattern.at((70.0 - 30.0) / 84.0, 0.5), 0.0);
        // The eye columns carry no pattern.
        assert_eq!(pattern.at(0.05, 0.5), 0.0);
        // Dark marks darken.
        for p in rgba.chunks_exact_mut(4).filter(|p| p[3] == 255) {
            p[..3].copy_from_slice(&[0, 0, 0]);
        }
        assert!(!pattern_from_icon(&rgba, w, h).lighten);
    }

    #[test]
    fn texels_map_to_the_top_view_of_the_mesh() {
        // One triangle covering the upper-left half of a 8 x 8 atlas.
        let positions = [[0.0, 0.3, -1.0], [1.0, 0.3, -1.0], [0.0, 0.3, 1.0]];
        let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let map = texel_positions(&positions, &uvs, &[0, 1, 2], 8, 8);
        assert_eq!(map.len(), 64);
        // Texel (0, 0) is near vertex 0, texel (7, 0) near vertex 1, (0, 7) near vertex 2.
        let near = |at: Option<[f32; 2]>, x: f32, z: f32| at.is_some_and(|p| (p[0] - x).abs() < 0.2 && (p[1] - z).abs() < 0.3);
        assert!(near(map[0], 0.06, -0.8), "{:?}", map[0]);
        assert!(near(map[7], 0.9, -0.9), "{:?}", map[7]);
        assert!(near(map[7 * 8], 0.06, 0.9), "{:?}", map[7 * 8]);
        assert_eq!(map[63], None, "outside the triangle");
    }

    #[test]
    fn dilation_pads_islands_by_the_given_texels() {
        let mut map = vec![None; 8 * 8];
        map[3 * 8 + 3] = Some([1.0, 2.0]);
        dilate(&mut map, 8, 8, 2);
        let covered = map.iter().filter(|m| m.is_some()).count();
        assert_eq!(covered, 25, "a 5 x 5 block around the texel");
        assert_eq!(map[1 * 8 + 1], Some([1.0, 2.0]));
        assert_eq!(map[0], None);
        // No passes change nothing.
        let mut same = vec![None; 4];
        same[1] = Some([0.0, 0.0]);
        dilate(&mut same, 2, 2, 0);
        assert_eq!(same.iter().filter(|m| m.is_some()).count(), 1);
    }

    #[test]
    fn instances_of_one_part_model_take_apart_draws() {
        use crate::game::PartInstance;
        let eye = |x: f32| {
            let mut p = PartInstance::new(7, 0);
            p.offset = [x, 0.0, 0.0];
            p
        };
        let parts = [eye(-0.3), PartInstance::new(9, 0), eye(0.3), eye(0.0)];
        assert_eq!(instance_salt(&parts, 0), 0, "the first eye keeps the definition's paint");
        assert_eq!(instance_salt(&parts, 1), 0, "another model starts again");
        assert_eq!(instance_salt(&parts, 2), 1);
        assert_eq!(instance_salt(&parts, 3), 2);
        assert_eq!(instance_salt(&parts, 9), 0, "no such part");
    }

    #[test]
    fn the_pattern_changes_covered_texels_only() {
        let (w, h) = (128u32, 128u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 20..88 {
            for x in 30..114 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&if x > 60 { [230, 230, 250, 255] } else { [20, 20, 30, 20] });
            }
        }
        let pattern = pattern_from_icon(&rgba, w, h);
        // Two texels: one over the marks (back of the cell), one over the
        // plain front half of the pill.
        let atlas = [100, 100, 100, 255, 100, 100, 100, 255];
        let map = [Some([0.0, 0.7]), Some([0.0, -0.5])];
        let bounds = [-0.5, 0.5, -0.9, 0.9];
        let plain = paint_body(&atlas, 100.0, [0.5, 0.5, 0.5], &map, bounds, None);
        let styled = paint_body(&atlas, 100.0, [0.5, 0.5, 0.5], &map, bounds, Some(&pattern));
        assert_eq!(&plain[0..4], &plain[4..8], "no pattern: a flat colour");
        assert!(styled[0] > plain[0] + 20, "marks lighten the body: {} vs {}", styled[0], plain[0]);
        assert_eq!(&styled[4..8], &plain[4..8], "plain texels stay");
    }

    #[test]
    fn parts_take_the_colour_on_layer_one_and_the_model_default_elsewhere() {
        let c = part_colors([1.0, 0.5, 0.0]);
        assert_eq!(c, [[255, 128, 0], [255, 255, 255], [255, 255, 255]]);
    }

    fn data() -> CellPaintData {
        let mut d = CellPaintData {
            loaded: true,
            default_theme: 10,
            swatches: vec![[0.1, 0.2, 0.3], [0.9, 0.8, 0.7]],
            ..default()
        };
        for (i, color) in [(10, [1.0, 0.99, 0.86]), (11, [0.7, 0.9, 0.4])] {
            d.themes.insert(
                i,
                PaintTheme {
                    instance: i,
                    name: format!("t{i}"),
                    paints: vec![i + 100],
                    colors: vec![color],
                    icon: prop_key(0, i),
                },
            );
        }
        d
    }

    #[test]
    fn a_style_click_takes_the_style_colour_and_a_colour_click_keeps_the_style() {
        let d = data();
        let styled = d.choose_style(11).unwrap();
        assert_eq!((styled.theme, styled.color), (11, [0.7, 0.9, 0.4]));
        let recolored = d.choose_color(&styled, 1).unwrap();
        assert_eq!((recolored.theme, recolored.color), (11, [0.9, 0.8, 0.7]));
        assert_eq!(d.swatch_of(&recolored), Some(1));
        // A never painted cell is painted with the default style.
        let first = d.choose_color(&CellPaint::default(), 0).unwrap();
        assert_eq!(first.theme, d.default_theme);
        assert!(d.choose_color(&styled, 9).is_none());
        assert!(d.choose_style(99).is_none());
        // An unpainted cell highlights no swatch even if the colour matches.
        assert_eq!(d.swatch_of(&CellPaint { theme: 0, color: [0.1, 0.2, 0.3] }), None);
    }

    #[test]
    fn colour_arrays_decode_little_endian_floats() {
        let mut item = Vec::new();
        for v in [0.25f32, 0.5, 0.75] {
            item.extend_from_slice(&v.to_le_bytes());
        }
        let p = Property {
            kind: 0x32,
            array: true,
            values: vec![item.clone(), item],
        };
        assert_eq!(colors(&p), vec![[0.25, 0.5, 0.75]; 2]);
        let not_colour = Property {
            kind: 0x31,
            array: true,
            values: vec![vec![0; 12]],
        };
        assert!(colors(&not_colour).is_empty());
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_paint_palette_has_the_retail_styles_and_colours() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CellPaintData::load(&index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        assert_eq!(data.palette, 6);
        assert_eq!(data.pages.len(), 2, "pager 1/2");
        for page in &data.pages {
            assert_eq!((page.items.len(), page.rows, page.columns), (12, 4, 3));
        }
        assert_eq!(data.themes.len(), 24);
        assert_eq!(data.swatches.len(), 15 + EXTRA_SWATCHES.len());
        let blank = data.theme(data.default_theme).unwrap();
        assert_eq!(blank.name, "skinpaint_cell_blankMatte");
        let c = blank.color();
        assert!((c[0] - 1.0).abs() < 1e-3 && (c[1] - 0.9882).abs() < 1e-3 && (c[2] - 0.8627).abs() < 1e-3, "{c:?}");
        // Page 1 starts with blankMatte, blankShiny, amphibian (retail frame).
        let first: Vec<_> = data.pages[0].items.iter().take(3).map(|i| data.themes[&i.part].name.clone()).collect();
        assert_eq!(first, ["skinpaint_cell_blankMatte", "skinpaint_cell_blankShiny", "skinpaint_cell_amphibian"]);
        let amphibian = data.pages[0].items[2].part;
        assert!((data.themes[&amphibian].color()[1] - 0.9059).abs() < 1e-3);
        for image in [data.art.frame, data.art.glow, data.art.color] {
            assert!(index.records.contains_key(&image.expect("swatch image named")), "{image:?}");
        }
        assert!(data.themes.values().all(|t| index.records.contains_key(&t.icon)), "every style has its icon");
    }
}
