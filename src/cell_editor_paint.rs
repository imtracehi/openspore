//! Paint mode chrome of the Cell Creator: the retail paint palette layouts and
//! the dynamic pieces the retail `PaletteUI` creates at run time (the colour
//! row, the page of complete styles, the page counter).
//!
//! Layouts (all installed): `EditorPaintPalette` (the bar on the top right),
//! the Cell paint category `0x3FCA51EA` (panel, swatch row `3304221B`, page area
//! `05D1754B`, pager host `92DF6FD8`) and `EditorPageControls` (the "1/2"
//! counter and its arrows). The swatches use the `ColorSwatch_*` images named by
//! `DefaultColorPickerConfig`; the styles are the `cellpainticons~` icons. Item
//! positions follow the page template (`TemplatePaletteCellSkinPaint`: three
//! columns, four rows, item width 30% of the page, height at most 20%; placed
//! by the retail relative layout, `cell_editor_ui::item_area`).

use bevy::prelude::*;

use crate::{
    cell_editor::CellEditor,
    cell_editor_data::PalettePage,
    cell_editor_ui::{image, plain},
    cell_paint::{rgb8, CellPaintData, SwatchArt},
    spui::{
        ids::{class, flag},
        model::{ButtonData, Drawable, EffectKind, Extra, ImageDrawable, Layout, WinId, WinProc, Window, WindowKind},
    },
};

/// Controls of the paint layouts.
pub mod id {
    /// Swatch row container of the paint category.
    pub const SWATCH_AREA: u32 = 0x3304_221B;
    /// Host of the page counter in the category.
    pub const PAGE_HOST: u32 = 0x92DF_6FD8;
    /// `EditorPageControls`: counter text, previous and next arrows.
    pub const PAGE_TEXT: u32 = 0x734A_FA63;
    pub const PAGE_PREV: u32 = 0x92DF_6FD7;
    pub const PAGE_NEXT: u32 = 0x92DF_6FD6;
}

/// Windows of a swatch (button, selection glow) and of a style tile (button,
/// selection glow, theme instance).
#[derive(Clone, Debug, Default)]
pub struct PaintWindows {
    pub swatches: Vec<SwatchWindows>,
    pub tiles: Vec<(WinId, WinId, u32)>,
}

fn glow_hidden(mut window: Window) -> Window {
    window.flags &= !flag::VISIBLE;
    window
}

fn button(id: WinId, parent: WinId, area: [f32; 4], control: u32) -> Window {
    Window {
        class: class::BUTTON,
        kind: WindowKind::Button,
        flags: flag::VISIBLE | flag::ENABLED,
        control_id: control,
        extra: Extra::Button(ButtonData::default()),
        ..plain(id, Some(parent), area, None, 0xFFFF_FFFF)
    }
}

fn picture(key: Option<crate::dbpf::ResourceKey>) -> Option<Drawable> {
    key.map(|file| {
        Drawable::Image(ImageDrawable {
            image: Some(image(file)),
            ..Default::default()
        })
    })
}

/// The visibility effects (`Fade`, `Glide`, ...) a layout declares on a window.
pub fn effect_procs(layout: &Layout, window: WinId) -> Vec<WinProc> {
    layout
        .window(window)
        .procs
        .iter()
        .filter(|p| matches!(p, WinProc::Effect { kind: EffectKind::Fade | EffectKind::Glide { .. } | EffectKind::Inflate { .. }, .. }))
        .cloned()
        .collect()
}

/// Windows of one colour swatch (`ColorSwatchUI`): the button (main window)
/// and its four image windows, which all fill the button (`SetArea`
/// 0x5A44F0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwatchWindows {
    pub button: WinId,
    pub color: WinId,
    pub frame: WinId,
    pub glow: WinId,
    pub shine: WinId,
}

/// The four image windows of a swatch inside `parent` (a `w` x `h` button).
/// `ColorSwatchUI::Load` (0x5A51B0) adds colour, frame, glow and shine in
/// that order (later windows draw on top); the colour window is shaded with
/// the colour, the glow starts fully transparent (its alpha follows the
/// press / selection timers). `children` lists them front-most first.
fn swatch_images(
    windows: &mut Vec<Window>,
    parent: WinId,
    w: f32,
    h: f32,
    images: [Option<crate::dbpf::ResourceKey>; 4],
    shade: u32,
    glow_shade: u32,
) -> SwatchWindows {
    let [color_img, frame_img, glow_img, shine_img] = images;
    let full = [0.0, 0.0, w, h];
    let color = windows.len();
    windows.push(plain(color, Some(parent), full, picture(color_img), shade));
    let frame = windows.len();
    windows.push(plain(frame, Some(parent), full, picture(frame_img), 0xFFFF_FFFF));
    let glow = windows.len();
    windows.push(plain(glow, Some(parent), full, picture(glow_img), glow_shade));
    let shine = windows.len();
    windows.push(plain(shine, Some(parent), full, picture(shine_img), 0xFFFF_FFFF));
    windows[parent].children = vec![shine, glow, frame, color];
    SwatchWindows {
        button: parent,
        color,
        frame,
        glow,
        shine,
    }
}

/// ARGB of a colour (opaque).
pub fn argb(c: [f32; 3]) -> u32 {
    let [r, g, b] = rgb8(c);
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// `ColorPickerUI::GetSwatchArea` (0x5A4740): `n` swatches over a `w` wide
/// row with a gap of `gap` x (w / n) between them.
pub fn swatch_area(index: usize, n: usize, w: f32, h: f32, gap: f32) -> [f32; 4] {
    let nf = n.max(1) as f32;
    let g = w / nf * gap;
    let size = (w - (nf - 1.0) * g) / nf;
    let x = index as f32 * (size + g);
    [x, 0.0, x + size, h]
}

/// The Cell colour row: one swatch per colour over the swatch container (the
/// custom and default swatches last; the default one uses its own images).
/// Control ids are the swatch index + 1. The glows start transparent.
pub fn swatch_row_layout(
    art: &SwatchArt,
    colors: &[[f32; 3]],
    area: [f32; 4],
    procs: Vec<WinProc>,
    gap: f32,
    default_index: Option<usize>,
) -> (Layout, Vec<SwatchWindows>) {
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![plain(0, None, area, None, 0xFFFF_FFFF)];
    windows[0].procs = procs;
    let mut ids = Vec::new();
    for (i, c) in colors.iter().enumerate() {
        let r = swatch_area(i, colors.len(), w, h, gap);
        let b = windows.len();
        windows.push(button(b, 0, r, i as u32 + 1));
        windows[0].children.push(b);
        let images = if Some(i) == default_index {
            [art.default_color, art.default_frame, art.default_glow, art.default_shine]
        } else {
            [art.color, art.frame, art.glow, art.shine]
        };
        ids.push(swatch_images(&mut windows, b, r[2] - r[0], h, images, argb(*c), 0x00FF_FFFF));
    }
    (layout_of(windows), ids)
}

/// The colour row as other editors use it: every swatch with the common
/// images, the glow hidden (shown on hover / selection by the caller).
pub fn swatch_layout(art: &SwatchArt, colors: &[[f32; 3]], area: [f32; 4], procs: Vec<WinProc>) -> (Layout, Vec<(WinId, WinId)>) {
    let (mut layout, ids) = swatch_row_layout(art, colors, area, procs, 0.0, None);
    for s in &ids {
        layout.windows[s.glow].shade_color = 0xFFFF_FFFF;
        layout.windows[s.glow].flags &= !flag::VISIBLE;
    }
    (layout, ids.iter().map(|s| (s.button, s.glow)).collect())
}

/// The shade panel of a held swatch (`ColorSwatchUI` expansion window): the
/// `colorpickerImageExpansionBackground` square with `n` x `n` shade swatches
/// (`ColorSwatchUI::Update` 0x5A63C0: cell = side / n, gap = cell x gap,
/// column `i` at x = i cell + gap / 2). It pops in with an Inflate (from
/// 0.0001, centred) and a Fade of 0.3 s each (0x5A51B0). Control ids are the
/// shade index (`i n + j`) + 1. The root's area is set when it opens.
pub fn expansion_layout(art: &SwatchArt, n: usize, side: f32, gap: f32) -> (Layout, Vec<SwatchWindows>) {
    let mut root = plain(0, None, [0.0, 0.0, side, side], picture(art.expansion_background), 0xFFFF_FFFF);
    root.flags = flag::ENABLED;
    root.procs = vec![effect(EffectKind::Inflate { scale: 0.0001 }, 0.3), effect(EffectKind::Fade, 0.3)];
    let mut windows = vec![root];
    let cell = side / n.max(1) as f32;
    let g = cell * gap;
    let mut ids = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f32 * cell + g * 0.5, j as f32 * cell + g * 0.5);
            let size = (1.0 - gap) * cell;
            let b = windows.len();
            windows.push(button(b, 0, [x, y, x + size, y + size], (i * n + j) as u32 + 1));
            windows[0].children.push(b);
            ids.push(swatch_images(&mut windows, b, size, size, [art.color, art.frame, art.glow, art.shine], 0xFFFF_FFFF, 0x00FF_FFFF));
        }
    }
    (layout_of(windows), ids)
}

/// The chosen shade flying back to its swatch (0x5A4B70: the shade's window
/// is moved to the main window and eased toward the swatch). One swatch that
/// ignores the mouse; the root's area is its rectangle.
pub fn flyer_layout(art: &SwatchArt) -> (Layout, SwatchWindows) {
    let mut root = plain(0, None, [0.0, 0.0, 20.0, 20.0], None, 0xFFFF_FFFF);
    root.flags = flag::IGNORE_MOUSE;
    let mut windows = vec![root];
    let b = windows.len();
    windows.push(plain(b, Some(0), [0.0, 0.0, 20.0, 20.0], None, 0xFFFF_FFFF));
    windows[0].children.push(b);
    let ids = swatch_images(&mut windows, b, 20.0, 20.0, [art.color, art.frame, art.glow, art.shine], 0xFFFF_FFFF, 0x00FF_FFFF);
    (layout_of(windows), ids)
}

fn effect(kind: EffectKind, time: f32) -> WinProc {
    WinProc::Effect {
        bi: crate::spui::model::BiState {
            time,
            trigger: 1,
            interpolation: 0,
            ease_in: 0.0,
            ease_out: 0.0,
            damping: 0.0,
        },
        kind,
    }
}

fn layout_of(windows: Vec<Window>) -> Layout {
    Layout {
        version: 0,
        windows,
        roots: vec![0],
        hit_masks: vec![],
        warnings: vec![],
        unknown_classes: vec![],
    }
}

/// ADAPTED: shade of a tile's selection outline (the retail selected glow
/// colour `editorSelectedGlowColor` is not decoded for the Cell palettes).
const TILE_GLOW_SHADE: u32 = 0xB0FF_F0B0;

/// One page of complete styles: an icon button per style at its row and
/// column of the page area. Control ids are the style PROP instances. Used by
/// the Creature and Rigid editors too: items are squares of the page fraction,
/// kept inside their cell.
pub fn tile_layout(data: &CellPaintData, page: &PalettePage, area: [f32; 4], art: &SwatchArt, procs: Vec<WinProc>) -> (Layout, Vec<(WinId, WinId, u32)>) {
    tile_layout_with(data, page, area, art, procs, None)
}

/// The Cell Creator's page of styles, placed by the retail relative layout
/// (`PalettePageUI::GetItemArea` 0x5C8E60, [`crate::cell_editor_ui::item_area`])
/// with the paint template's maximum item height of 20% of the page.
pub fn cell_tile_layout(data: &CellPaintData, page: &PalettePage, area: [f32; 4], art: &SwatchArt, procs: Vec<WinProc>) -> (Layout, Vec<(WinId, WinId, u32)>) {
    tile_layout_with(data, page, area, art, procs, Some(crate::cell_editor_ui::PAINT_MAX_HEIGHT))
}

fn tile_layout_with(
    data: &CellPaintData,
    page: &PalettePage,
    area: [f32; 4],
    art: &SwatchArt,
    procs: Vec<WinProc>,
    retail_max_height: Option<f32>,
) -> (Layout, Vec<(WinId, WinId, u32)>) {
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![plain(0, None, area, None, 0xFFFF_FFFF)];
    windows[0].procs = procs;
    let (cw, ch) = (w / page.columns.max(1) as f32, h / page.rows.max(1) as f32);
    let mut ids = Vec::new();
    for item in &page.items {
        let Some(theme) = data.theme(item.part) else {
            continue;
        };
        let rect = match retail_max_height {
            Some(max_height) => {
                let r = crate::cell_editor_ui::item_area(
                    w,
                    h,
                    page.columns,
                    page.rows,
                    item.row,
                    item.column,
                    page.item_width_fraction,
                    crate::cell_editor_ui::ITEM_ASPECT,
                    max_height,
                );
                // ADAPTED: corners snapped to whole pixels.
                [r[0].round(), r[1].round(), r[2].round(), r[3].round()]
            }
            None => {
                let size = (page.item_width_fraction * w).min(cw).min(ch).round();
                let cx = (item.column as f32 + 0.5) * cw;
                let cy = (item.row as f32 + 0.5) * ch;
                [
                    (cx - size * 0.5).round(),
                    (cy - size * 0.5).round(),
                    (cx + size * 0.5).round(),
                    (cy + size * 0.5).round(),
                ]
            }
        };
        let size = rect[2] - rect[0];
        let b = windows.len();
        windows.push(button(b, 0, rect, item.part));
        windows[0].children.push(b);
        let glow = windows.len();
        windows.push(glow_hidden(plain(glow, Some(b), [-2.0, -2.0, size + 2.0, size + 2.0], picture(art.glow), TILE_GLOW_SHADE)));
        let icon = windows.len();
        windows.push(plain(icon, Some(b), [0.0, 0.0, size, size], picture(Some(theme.icon)), 0xFFFF_FFFF));
        windows[b].children = vec![icon, glow];
        ids.push((b, glow, item.part));
    }
    let layout = Layout {
        version: 0,
        windows,
        roots: vec![0],
        hit_masks: vec![],
        warnings: vec![],
        unknown_classes: vec![],
    };
    (layout, ids)
}

/// "1/2": the page counter text.
pub fn page_text(page: usize, pages: usize) -> String {
    format!("{}/{}", page.min(pages.saturating_sub(1)) + 1, pages.max(1))
}

/// Move the shown paint page by `delta`, staying inside the pages.
pub fn step_page(editor: &mut CellEditor, pages: usize, delta: i32) {
    let last = pages.saturating_sub(1) as i32;
    editor.paint_page = (editor.paint_page as i32 + delta).clamp(0, last) as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_editor_data::{prop_key, PaletteItem};
    use crate::cell_paint::PaintTheme;

    fn data() -> CellPaintData {
        let mut d = CellPaintData::default();
        for i in 1..=12u32 {
            d.themes.insert(
                i,
                PaintTheme {
                    instance: i,
                    name: format!("t{i}"),
                    paints: vec![],
                    colors: vec![[1.0, 1.0, 1.0]],
                    icon: prop_key(0, i),
                },
            );
        }
        d
    }

    #[test]
    fn swatches_fill_the_row_and_tiles_follow_their_rows_and_columns() {
        let art = SwatchArt::default();
        let colors = vec![[1.0, 0.0, 0.0]; 17];
        let (layout, ids) = swatch_layout(&art, &colors, [100.0, 50.0, 440.0, 84.0], vec![]);
        assert_eq!(ids.len(), 17);
        let first = layout.window(ids[0].0).area;
        let last = layout.window(ids[16].0).area;
        assert_eq!((first[0], last[2]), (0.0, 340.0), "the row covers the container");
        assert_eq!(layout.window(ids[5].0).control_id, 6);
        // The swatch's colour window carries the colour as its shade.
        let colour_window = *layout.window(ids[0].0).children.last().unwrap();
        assert_eq!(layout.window(colour_window).shade_color, 0xFFFF_0000);
        // The glow starts hidden.
        assert_eq!(layout.window(ids[0].1).flags & flag::VISIBLE, 0);

        let page = PalettePage {
            key: prop_key(0, 0),
            sequence: 0,
            items: (0..12)
                .map(|i| PaletteItem {
                    part: i + 1,
                    row: i / 3,
                    column: i % 3,
                })
                .collect(),
            rows: 4,
            columns: 3,
            item_width_fraction: 0.3,
        };
        let (layout, tiles) = tile_layout(&data(), &page, [0.0, 0.0, 300.0, 400.0], &art, vec![]);
        assert_eq!(tiles.len(), 12);
        let a = layout.window(tiles[0].0).area;
        let b = layout.window(tiles[4].0).area; // row 1, column 1
        assert!(b[0] > a[0] && b[1] > a[1]);
        assert_eq!(layout.window(tiles[4].0).control_id, 5);
        // Tile centres sit in the middle of their cells.
        assert_eq!((b[0] + b[2]) / 2.0, 150.0);
        assert_eq!((b[1] + b[3]) / 2.0, 150.0);
    }

    #[test]
    fn page_counter_and_stepping_stay_inside_the_pages() {
        assert_eq!(page_text(0, 2), "1/2");
        assert_eq!(page_text(1, 2), "2/2");
        assert_eq!(page_text(5, 2), "2/2");
        assert_eq!(page_text(0, 0), "1/1");
        let mut e = CellEditor::default();
        step_page(&mut e, 2, -1);
        assert_eq!(e.paint_page, 0);
        step_page(&mut e, 2, 1);
        step_page(&mut e, 2, 1);
        assert_eq!(e.paint_page, 1);
        step_page(&mut e, 0, 1);
        assert_eq!(e.paint_page, 0);
    }
}
