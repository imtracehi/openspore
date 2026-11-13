//! The Cell Creator's colour swatches and palette item hover, as the retail
//! `Palettes::ColorSwatchUI` / `ColorPickerUI` and `AdvancedItemViewer` run
//! them (decompiled: `notes\cell-creator-final-notes.md`).
//!
//! Swatches (`ColorSwatchUI::Update` 0x5A63C0, `HandleUIMessage` 0x5A5B60):
//! four clocks per swatch ramp up or down with the elapsed time and clamp:
//! rollover (hovered, to `colorpickerRolloverMaxTime`), mouse down (pressed,
//! to `colorpickerMouseDownMaxTime`), select (the selected swatch, to
//! `colorpickerSelectMaxTime`) and panel (the shade panel shown, to 0.5 s).
//! A hovered swatch grows by `colorpickerMaxInflation` pixels on every side
//! in proportion to its rollover clock; the frame glow's alpha is select /
//! select max while selected, else mouse down / mouse down max; the swatch
//! fades out (1 - 2 x panel) while its panel shows. Holding a press for the
//! mouse-down time, or a second press within `colorpickerDoubleClickInterval`,
//! opens the shade panel (`GenerateExpansionArea` 0x5A4980): a square of
//! `colorpickerExpansionSize` x (window width / 800) pixels centred on the
//! swatch (kept inside the window's right edge) holding N x N shades of the
//! swatch's colour (0x5A4240). Releasing over a shade chooses it: the swatch
//! takes that colour (0x5A45C0), the panel hides and the shade flies back to
//! the swatch, easing by `colorpickerExpansionCatchUpSpeed` x dt per frame
//! until both corners are within a pixel (0x5A4B70 + Update). Leaving the
//! panel hides it. The default colour swatch never opens a panel.
//!
//! Palette items (`AdvancedItemViewer` 0x5F2840 / 0x5F3230 / 0x5F6FD0 /
//! 0x5F2410): the item's preview is drawn into a viewport of its window's
//! size x zoom around the window centre; the zoom eases toward its target by
//! min(0.012 x ms, 1) per frame (snaps within 0.01); the target is 2 x the
//! item zoom while hovered (x 0.75 for an unavailable item) and 1 otherwise.
//! The flat icon window is scaled; the hovered unlocked item's part model turns
//! in that viewport at 1.5 rad/s (`viewer`: 0x5F3230 spin, 0x5F5670 turn about the
//! middle of the model's box, 0x5F3D80 camera). INFERRED: the item zoom is 1.0
//! (the item frames' fallback value 0x15ABEE4; no Cell caller of `SetZoom` was
//! found); locked items count as unavailable. The hover sound 0xBB58117E is the
//! UI sound `editor_part_rollover` (FNV of the name; `ui_sounds` plays it when the
//! pointer enters an item).

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::*;

pub mod viewer;

use crate::{
    cell_editor::{CellEditor, Mode},
    cell_editor_data::CellEditorData,
    cell_editor_paint::{self, argb, SwatchWindows},
    cell_editor_ui::{CellEditorUiLayout, EditorRoot},
    cell_paint::{same_color, CellPaint, PickerConfig},
    game::RunProgress,
    spui::{
        ids::state,
        model::WinId,
        solve::RootPolicy,
        view::{self, SpuiState},
    },
};

/// The four clocks of a swatch (seconds) and its press bookkeeping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clocks {
    pub rollover: f32,
    pub down: f32,
    pub select: f32,
    pub panel: f32,
    pub pressed: bool,
    /// Times of the last two presses (seconds of the editor clock).
    pub click: Option<f32>,
    pub prev_click: Option<f32>,
}

/// Ramp a clock up (`on`) or down by `dt`, clamped to `0..=max`.
pub fn ramp(t: f32, on: bool, dt: f32, max: f32) -> f32 {
    (t + if on { dt } else { -dt }).clamp(0.0, max.max(0.0))
}

/// Advance a swatch's clocks one frame (0x5A63C0, first half).
pub fn step(c: &mut Clocks, cfg: &PickerConfig, dt: f32, hovered: bool, selected: bool, showing: bool) {
    c.select = ramp(c.select, selected, dt, cfg.select_max);
    c.rollover = ramp(c.rollover, hovered, dt, cfg.rollover_max);
    c.down = ramp(c.down, c.pressed, dt, cfg.mouse_down_max);
    c.panel = ramp(c.panel, showing, dt, 0.5);
}

/// The swatch's area grown by its rollover clock.
pub fn inflated(area: [f32; 4], c: &Clocks, cfg: &PickerConfig) -> [f32; 4] {
    let d = if cfg.rollover_max > 0.0 { cfg.max_inflation / cfg.rollover_max * c.rollover } else { 0.0 };
    [area[0] - d, area[1] - d, area[2] + d, area[3] + d]
}

/// Alpha of the frame glow.
pub fn glow_alpha(c: &Clocks, cfg: &PickerConfig) -> f32 {
    if c.select > 0.0 {
        c.select / cfg.select_max.max(1e-6)
    } else {
        c.down / cfg.mouse_down_max.max(1e-6)
    }
}

/// Whether the press opens the shade panel this frame: held for the
/// mouse-down time, or two presses within the double-click interval.
pub fn opens(c: &Clocks, cfg: &PickerConfig, showing: bool) -> bool {
    let double = match (c.click, c.prev_click) {
        (Some(a), Some(b)) => (a - b) * 1000.0 < cfg.double_click_ms,
        _ => false,
    };
    c.down >= cfg.mouse_down_max || (double && !showing)
}

/// The shade panel's rectangle: a square of `expansion_size` x (width /
/// 800) centred on `centre`, moved left to stay inside the window.
pub fn panel_rect(centre: Vec2, screen_w: f32, cfg: &PickerConfig) -> [f32; 4] {
    let half = cfg.expansion_size * screen_w * 0.00125 * 0.5;
    let mut x = centre.x;
    if x + half > screen_w {
        x = screen_w - half - 1.0;
    }
    [x - half, centre.y - half, x + half, centre.y + half]
}

/// One frame of the flight back: ease `rect` toward `target`; true once both
/// corners are within a pixel.
pub fn fly(rect: &mut [f32; 4], target: [f32; 4], dt: f32, catch_up: f32) -> bool {
    let t = (dt * catch_up).min(1.0);
    for k in 0..4 {
        rect[k] += (target[k] - rect[k]) * t;
    }
    let d1 = Vec2::new(target[0] - rect[0], target[1] - rect[1]).length();
    let d2 = Vec2::new(target[2] - rect[2], target[3] - rect[3]).length();
    d1 < 1.0 && d2 < 1.0
}

/// One frame of an item viewer's zoom toward `target`.
pub fn zoom_step(zoom: f32, target: f32, dt: f32) -> f32 {
    let z = zoom + (target - zoom) * (dt * 1000.0 * 0.012).min(1.0);
    if (z - target).abs() < 0.01 {
        target
    } else {
        z
    }
}

/// The swatch the paint shows as selected (`ColorPickerUI::SetColor`
/// 0x5A4E10: the swatch whose colour matches; ADAPTED reading of its custom
/// branch: a colour no swatch shows goes to the custom swatch). The default
/// swatch is selected when the paint is the style's own colour.
pub fn selected(paint: &CellPaint, colors: &[[f32; 3]], custom: Option<usize>, default: Option<usize>, style_color: Option<[f32; 3]>) -> Option<usize> {
    if !paint.is_painted() {
        return None;
    }
    let shown = colors.iter().enumerate().filter(|(i, _)| Some(*i) != default);
    if let Some((i, _)) = shown.clone().find(|(_, c)| same_color(**c, paint.color)) {
        return Some(i);
    }
    if default.is_some() && style_color.is_some_and(|c| same_color(c, paint.color)) {
        return default;
    }
    custom
}

/// The open shade panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub swatch: usize,
    pub rect: [f32; 4],
    pub shades: Vec<[f32; 3]>,
    pub frames: u32,
    /// Opened by a capture flag: stays open without the pointer.
    pub pinned: bool,
    /// The rollover clock of each shade (seconds): the expansion swatches run the same
    /// update as any swatch with their own clocks.
    pub rollover: Vec<f32>,
}

/// The chosen shade on its way back to its swatch.
#[derive(Clone, Debug, PartialEq)]
pub struct Flyer {
    pub swatch: usize,
    pub color: [f32; 3],
    pub rect: [f32; 4],
}

/// Swatch and item hover state of the open Creator.
#[derive(Resource, Default)]
pub struct SwatchUi {
    /// Colour each swatch shows (`mColor`) and opens shades of
    /// (`mOriginalColor`).
    pub colors: Vec<[f32; 3]>,
    pub originals: Vec<[f32; 3]>,
    pub clocks: Vec<Clocks>,
    pub panel: Option<Panel>,
    pub flyer: Option<Flyer>,
    /// The current press opened a panel: its release chooses nothing.
    pub swallow: bool,
    /// Captures (`--cell-editor-shades N`): open swatch N's shades as if held.
    pub demo_open: Option<usize>,
    pub clock: f32,
    shades: Vec<SwatchWindows>,
    flyer_ids: Option<SwatchWindows>,
    spawned: bool,
    /// Item viewer zoom per (root, button).
    zoom: HashMap<(u8, WinId), f32>,
}

/// Run the colour row, the shade panel, the flight back and the palette
/// item zoom (between `cell_editor_ui::bind` and `actions`).
#[allow(clippy::too_many_arguments)]
pub fn swatches(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut ui_state: ResMut<SwatchUi>,
    mut editor: ResMut<CellEditor>,
    data: Res<CellEditorData>,
    mut run: ResMut<RunProgress>,
    layout: Res<CellEditorUiLayout>,
    camera: Query<Entity, With<crate::GameCamera>>,
    mut roots: Query<(&mut SpuiState, &EditorRoot)>,
    mut item_viewer: ResMut<viewer::ItemViewer>,
) {
    let dt = time.delta_secs().min(0.1);
    let s = &mut *ui_state;
    s.clock += dt;
    let paint = &data.paint;
    let cfg = paint.picker;
    let have_row = roots.iter().any(|(_, k)| *k == EditorRoot::PaintSwatches);
    let have_shades = roots.iter().any(|(_, k)| *k == EditorRoot::PaintShades);
    // The Creator closed (its roots are gone): start over next time.
    if (s.spawned && !have_shades) || (!have_row && !s.colors.is_empty()) {
        let clock = s.clock;
        *s = SwatchUi { clock, ..default() };
    }
    if !have_row {
        item_zoom(s, dt, &data, &run, &layout, &mut roots, &mut item_viewer);
        return;
    }
    if s.colors.len() != paint.swatches.len() {
        s.colors = paint.swatches.clone();
        s.originals = paint.swatches.clone();
        s.clocks = vec![Clocks::default(); paint.swatches.len()];
    }
    // The shade panel (laid out at its size for this window width) and the
    // flyer have their own roots.
    let screen = roots
        .iter()
        .find(|(ui, k)| **k == EditorRoot::PaintSwatches && ui.ready)
        .map(|(ui, _)| ui.screen[0])
        .unwrap_or(0.0);
    if !s.spawned && screen > 0.0 {
        if let Ok(camera) = camera.single() {
            let n = cfg.expansion_count;
            let side = (cfg.expansion_size * screen * 0.00125).max(1.0);
            let (mut l, ids) = cell_editor_paint::expansion_layout(&paint.art, n, side, cfg.gap);
            l.windows[0].flags &= !crate::spui::ids::flag::VISIBLE;
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 110);
            commands.entity(root).insert(EditorRoot::PaintShades);
            s.shades = ids;
            let (mut l, ids) = cell_editor_paint::flyer_layout(&paint.art);
            l.windows[0].flags &= !crate::spui::ids::flag::VISIBLE;
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 111);
            commands.entity(root).insert(EditorRoot::PaintFlyer);
            s.flyer_ids = Some(ids);
            s.spawned = true;
        }
    }
    let paint_mode = editor.mode == Mode::Paint;
    let custom = paint.custom_index();
    let default_ix = paint.default_index();
    let current = run.cell_creation.paint.clone();
    let style_color = paint.default_color_of(&current).color;
    // The custom swatch takes a colour no other swatch shows.
    let sel = selected(&current, &s.colors, custom, default_ix, Some(style_color));
    if let Some(c) = custom.filter(|c| sel == Some(*c)) {
        if !same_color(s.colors[c], current.color) {
            s.colors[c] = current.color;
            s.originals[c] = current.color;
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        s.swallow = false;
    }
    let mut choose: Option<CellPaint> = None;
    let mut screen_w = 0.0;
    let mut swatch_rects = vec![[0.0f32; 4]; s.colors.len()];
    let mut open_panel: Option<(usize, Vec2, bool)> = None;
    // The colour row.
    for (mut ui, kind) in &mut roots {
        if *kind != EditorRoot::PaintSwatches || !ui.ready {
            continue;
        }
        screen_w = ui.screen[0];
        let clicks = ui.take_clicks();
        if !paint_mode {
            continue;
        }
        let showing = s.panel.as_ref().map(|p| p.swatch);
        for (i, w) in layout.paint.swatches.iter().enumerate() {
            let Some(c) = s.clocks.get_mut(i) else {
                continue;
            };
            let hovered = ui.states[w.button] & state::HOVER != 0 && showing.is_none();
            if hovered && mouse.just_pressed(MouseButton::Left) {
                c.pressed = true;
                c.prev_click = c.click;
                c.click = Some(s.clock);
            }
            if !hovered || mouse.just_released(MouseButton::Left) {
                c.pressed = false;
            }
            if !hovered {
                c.click = None;
                c.prev_click = None;
            }
            step(c, &cfg, dt, hovered, sel == Some(i), showing == Some(i));
            let base = ui.layout.window(w.button).area;
            let area = inflated(base, c, &cfg);
            ui.set_area(w.button, area);
            let size = [0.0, 0.0, area[2] - area[0], area[3] - area[1]];
            for child in [w.color, w.frame, w.glow, w.shine] {
                ui.set_area(child, size);
            }
            ui.set_shade(w.color, argb(s.colors[i]));
            let a = |v: f32| ((v.clamp(0.0, 1.0) * 255.0).round() as u32) << 24 | 0x00FF_FFFF;
            ui.set_shade(w.glow, a(glow_alpha(c, &cfg)));
            ui.set_shade(w.button, a(1.0 - c.panel * 2.0));
            let r = ui.rect(w.button);
            swatch_rects[i] = [r.x1, r.y1, r.x2, r.y2];
            let demo = s.demo_open == Some(i);
            if Some(i) != default_ix && cfg.allow_expansion && showing.is_none() && (opens(c, &cfg, false) || demo) {
                s.demo_open = None;
                *c = Clocks { select: c.select, panel: c.panel, ..default() };
                open_panel = Some((i, Vec2::new((r.x1 + r.x2) * 0.5, (r.y1 + r.y2) * 0.5), demo));
            }
        }
        // A click chooses the swatch's colour (unless its press opened shades).
        for w in clicks {
            let control = ui.layout.window(w).control_id;
            let Some(i) = (control as usize).checked_sub(1).filter(|i| *i < s.colors.len()) else {
                continue;
            };
            if s.swallow || s.panel.is_some() {
                continue;
            }
            choose = Some(if Some(i) == default_ix {
                paint.default_color_of(&current)
            } else {
                paint.with_color(&current, s.colors[i])
            });
        }
    }
    if let Some((i, centre, pinned)) = open_panel {
        s.swallow = true;
        s.panel = Some(Panel {
            swatch: i,
            rect: panel_rect(centre, screen_w, &cfg),
            shades: crate::cell_paint::expansion_shades(s.originals[i], &cfg),
            frames: 0,
            pinned,
            rollover: Vec::new(),
        });
    }
    // The shade panel.
    let mut fly_from: Option<(usize, [f32; 3], [f32; 4])> = None;
    for (mut ui, kind) in &mut roots {
        if *kind != EditorRoot::PaintShades || !ui.ready {
            continue;
        }
        ui.take_clicks();
        let root = ui.layout.roots[0];
        let Some(panel) = s.panel.as_mut().filter(|_| paint_mode) else {
            if ui.visible(root) {
                ui.set_visible(root, false);
            }
            continue;
        };
        if panel.frames == 0 {
            ui.set_area(root, panel.rect);
            for (k, w) in s.shades.iter().enumerate() {
                ui.set_shade(w.color, argb(panel.shades.get(k).copied().unwrap_or([1.0; 3])));
                ui.set_shade(w.glow, 0x00FF_FFFF);
            }
            ui.set_visible(root, true);
        }
        panel.frames += 1;
        let hovered_shade = s.shades.iter().position(|w| ui.states[w.button] & state::HOVER != 0);
        let inside = hovered_shade.is_some() || ui.states[root] & state::HOVER != 0;
        // Hovered shades inflate like any swatch: the expansion swatches run the same
        // update with their own rollover clocks.
        panel.rollover.resize(s.shades.len(), 0.0);
        for (k, w) in s.shades.iter().enumerate() {
            let on = Some(k) == hovered_shade;
            let base = ui.layout.window(w.button).area;
            let clock = ramp(panel.rollover[k], on, dt, cfg.rollover_max);
            panel.rollover[k] = clock;
            let d = if cfg.rollover_max > 0.0 { cfg.max_inflation / cfg.rollover_max * clock } else { 0.0 };
            let area = [base[0] - d, base[1] - d, base[2] + d, base[3] + d];
            ui.set_area(w.button, area);
            for child in [w.color, w.frame, w.glow, w.shine] {
                ui.set_area(child, [0.0, 0.0, area[2] - area[0], area[3] - area[1]]);
            }
        }
        if let Some(k) = hovered_shade.filter(|_| mouse.just_released(MouseButton::Left)) {
            let color = panel.shades[k];
            let r = ui.rect(s.shades[k].button);
            fly_from = Some((panel.swatch, color, [r.x1, r.y1, r.x2, r.y2]));
        } else if !inside && panel.frames > 2 && !panel.pinned {
            s.panel = None;
            ui.set_visible(root, false);
        }
    }
    if let Some((i, color, rect)) = fly_from {
        s.colors[i] = color;
        choose = Some(paint.with_color(&current, color));
        s.flyer = Some(Flyer { swatch: i, color, rect });
        s.panel = None;
        s.swallow = true;
    }
    // The flight back.
    for (mut ui, kind) in &mut roots {
        if *kind != EditorRoot::PaintFlyer || !ui.ready {
            continue;
        }
        let root = ui.layout.roots[0];
        let Some(ids) = s.flyer_ids else {
            continue;
        };
        let done = match s.flyer.as_mut() {
            Some(f) if paint_mode => {
                let target = swatch_rects.get(f.swatch).copied().unwrap_or(f.rect);
                let done = fly(&mut f.rect, target, dt, cfg.catch_up);
                ui.set_area(root, f.rect);
                let size = [0.0, 0.0, f.rect[2] - f.rect[0], f.rect[3] - f.rect[1]];
                for w in [ids.button, ids.color, ids.frame, ids.glow, ids.shine] {
                    ui.set_area(w, size);
                }
                ui.set_shade(ids.color, argb(f.color));
                ui.set_shade(ids.glow, 0x00FF_FFFF);
                if !ui.visible(root) {
                    ui.set_visible(root, true);
                }
                done
            }
            _ => true,
        };
        if done {
            s.flyer = None;
            if ui.visible(root) {
                ui.set_visible(root, false);
            }
        }
    }
    if let Some(p) = choose {
        crate::cell_editor::set_paint(&mut editor, &mut run, p);
    }
    item_zoom(s, dt, &data, &run, &layout, &mut roots, &mut item_viewer);
}

/// The palette items' hover zoom (parts page and style page) and the turning
/// model of the hovered unlocked part item (`viewer`).
fn item_zoom(
    s: &mut SwatchUi,
    dt: f32,
    data: &CellEditorData,
    run: &RunProgress,
    layout: &CellEditorUiLayout,
    roots: &mut Query<(&mut SpuiState, &EditorRoot)>,
    item_viewer: &mut viewer::ItemViewer,
) {
    // The item whose model is hovered this frame, and the zoom of the viewed one.
    let mut hovered_item: Option<viewer::Shown> = None;
    let mut viewed_zoom = None;
    for (mut ui, kind) in roots.iter_mut() {
        let (tag, buttons): (u8, Vec<(WinId, bool, u32)>) = match kind {
            EditorRoot::Page => (
                0,
                layout
                    .items
                    .iter()
                    .map(|(b, part)| {
                        let unlocked = data.parts.get(part).is_none_or(|info| crate::cell_editor::part_is_unlocked(run, info));
                        (*b, unlocked, *part)
                    })
                    .collect(),
            ),
            EditorRoot::PaintPage => (1, layout.paint.tiles.iter().map(|(b, _, _)| (*b, true, 0)).collect()),
            _ => continue,
        };
        if !ui.ready {
            continue;
        }
        for (button, available, part) in buttons {
            if button >= ui.layout.windows.len() {
                continue;
            }
            let Some(&icon) = ui.layout.window(button).children.first() else {
                continue;
            };
            let hovered = ui.states[button] & state::HOVER != 0;
            let target = if hovered { 2.0 * if available { 1.0 } else { 0.75 } } else { 1.0 };
            let z = s.zoom.entry((tag, button)).or_insert(1.0);
            *z = zoom_step(*z, target, dt);
            let z = *z;
            let a = ui.layout.window(icon).area;
            let (cx, cy) = ((a[0] + a[2]) * 0.5, (a[1] + a[3]) * 0.5);
            let (hw, hh) = ((a[2] - a[0]) * 0.5 * z, (a[3] - a[1]) * 0.5 * z);
            ui.set_area(icon, [cx - hw, cy - hh, cx + hw, cy + hh]);
            if tag != 0 || !available {
                continue;
            }
            // The unlocked part item's model (AdvancedItemViewer): it turns
            // while hovered and eases back to rest afterwards; the flat icon
            // is hidden while the model shows.
            let viewed = item_viewer.current.is_some_and(|c| c.button == button);
            if hovered {
                hovered_item = Some(viewer::Shown { part, button });
            }
            if hovered || viewed {
                let r = ui.rect(button);
                let screen = ui.screen;
                if hovered || item_viewer.current.is_some_and(|c| c.button == button) {
                    item_viewer.rect = viewer::viewport([r.x1, r.y1, r.x2, r.y2], z, screen);
                    item_viewer.screen = screen;
                    viewed_zoom = Some(z);
                }
            }
            let show_flat = !(item_viewer.live && viewed);
            if ui.visible(icon) != show_flat {
                ui.set_visible(icon, show_flat);
            }
        }
    }
    match hovered_item {
        Some(shown) => {
            if item_viewer.current != Some(shown) {
                // ADAPTED: one preview at a time; another item starts at rest.
                item_viewer.current = Some(shown);
                item_viewer.rotation = 0.0;
                item_viewer.live = false;
            }
            item_viewer.rotation = viewer::spin_step(item_viewer.rotation, true, dt);
        }
        None => {
            if item_viewer.current.is_some() {
                item_viewer.rotation = viewer::spin_step(item_viewer.rotation, false, dt);
                // Back at rest and at zoom 1: the flat icon takes over again.
                if item_viewer.rotation == 0.0 && viewed_zoom.is_none_or(|z| z <= 1.0 + 1e-3) {
                    item_viewer.current = None;
                    item_viewer.live = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_paint::expansion_shades;

    fn retail() -> PickerConfig {
        // `DefaultColorPickerConfig` as installed.
        PickerConfig {
            rollover_max: 0.08,
            mouse_down_max: 0.25,
            select_max: 0.15,
            max_inflation: 3.0,
            catch_up: 8.0,
            gap: 0.0,
            double_click_ms: 400.0,
            expansion_count: 7,
            expansion_size: 115.0,
            sat_max: 0.95,
            sat_exponent: 1.0,
            val_max: 1.0,
            val_exponent: 0.65,
            allow_expansion: true,
        }
    }

    #[test]
    fn hover_inflates_three_pixels_in_the_rollover_time_and_the_glow_follows_the_press() {
        let cfg = retail();
        let mut c = Clocks::default();
        step(&mut c, &cfg, 0.04, true, false, false);
        let a = inflated([10.0, 0.0, 30.0, 20.0], &c, &cfg);
        assert!((a[0] - 8.5).abs() < 1e-4 && (a[2] - 31.5).abs() < 1e-4, "half way: 1.5 px");
        step(&mut c, &cfg, 0.1, true, false, false);
        assert_eq!(inflated([10.0, 0.0, 30.0, 20.0], &c, &cfg), [7.0, -3.0, 33.0, 23.0]);
        step(&mut c, &cfg, 1.0, false, false, false);
        assert_eq!(c.rollover, 0.0);
        c.pressed = true;
        step(&mut c, &cfg, 0.125, true, false, false);
        assert!((glow_alpha(&c, &cfg) - 0.5).abs() < 1e-4);
        assert!(!opens(&c, &cfg, false));
        step(&mut c, &cfg, 0.2, true, false, false);
        assert!(opens(&c, &cfg, false), "held for the mouse-down time");
        // Selected: the glow follows the select clock.
        let mut s = Clocks::default();
        step(&mut s, &cfg, 0.075, false, true, false);
        assert!((glow_alpha(&s, &cfg) - 0.5).abs() < 1e-4);
        // Double click within 400 ms.
        let d = Clocks { click: Some(1.3), prev_click: Some(1.0), ..default() };
        assert!(opens(&d, &cfg, false));
        assert!(!opens(&d, &cfg, true));
        let slow = Clocks { click: Some(1.5), prev_click: Some(1.0), ..default() };
        assert!(!opens(&slow, &cfg, false));
    }

    #[test]
    fn shades_keep_the_hue_and_greys_ramp_from_black_to_white() {
        let cfg = retail();
        let red = expansion_shades([1.0, 0.2, 0.2], &cfg);
        assert_eq!(red.len(), 49);
        // Column 6 row 0: saturation 0.95, value 1.
        let c = red[6 * 7];
        assert!((c[0] - 1.0).abs() < 1e-4 && (c[1] - 0.05).abs() < 1e-3 && (c[2] - 0.05).abs() < 1e-3, "{c:?}");
        // Column 0: saturation 0.95 / 7.
        let c = red[0];
        assert!((c[1] - (1.0 - 0.95 / 7.0)).abs() < 1e-3, "{c:?}");
        // Row 6: value (1/7)^0.65.
        let c = red[6 * 7 + 6];
        assert!((c[0] - (1.0f32 / 7.0).powf(0.65)).abs() < 1e-3, "{c:?}");
        let grey = expansion_shades([0.5, 0.5, 0.5], &cfg);
        assert_eq!(grey[0], [0.0; 3]);
        assert_eq!(grey[48], [1.0; 3]);
        assert!((grey[8][0] - 8.0 / 48.0).abs() < 1e-6, "index i n + j");
    }

    #[test]
    fn the_panel_scales_with_the_window_and_stays_inside_it() {
        let cfg = retail();
        let r = panel_rect(Vec2::new(600.0, 100.0), 1280.0, &cfg);
        assert!((r[2] - r[0] - 184.0).abs() < 1e-3, "115 x 1280 / 800");
        assert_eq!((r[0] + r[2]) / 2.0, 600.0);
        let r = panel_rect(Vec2::new(1250.0, 100.0), 1280.0, &cfg);
        assert!((r[2] - 1279.0).abs() < 1e-3, "moved left of the edge");
    }

    #[test]
    fn the_flyer_catches_up_and_the_item_zoom_eases_and_snaps() {
        let mut r = [0.0, 0.0, 10.0, 10.0];
        let target = [100.0, 50.0, 120.0, 70.0];
        let mut frames = 0;
        while !fly(&mut r, target, 1.0 / 60.0, 8.0) {
            frames += 1;
            assert!(frames < 200);
        }
        assert!(frames > 10, "eases over several frames");
        let mut z = 1.0;
        z = zoom_step(z, 2.0, 1.0 / 60.0);
        assert!((z - 1.2).abs() < 1e-4, "0.012 x 16.7 ms of the gap");
        for _ in 0..60 {
            z = zoom_step(z, 2.0, 1.0 / 60.0);
        }
        assert_eq!(z, 2.0, "snaps within 0.01");
        assert_eq!(zoom_step(1.5, 1.0, 1.0), 1.0, "the step is clamped to the whole gap");
    }

    #[test]
    fn selection_prefers_shown_colours_then_the_default_then_the_custom_swatch() {
        let colors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.5, 0.5, 0.5], [1.0, 0.94, 0.83]];
        let p = |c| CellPaint { theme: 7, color: c };
        assert_eq!(selected(&p([0.0, 1.0, 0.0]), &colors, Some(2), Some(3), Some([0.9; 3])), Some(1));
        assert_eq!(selected(&p([0.9; 3]), &colors, Some(2), Some(3), Some([0.9; 3])), Some(3));
        assert_eq!(selected(&p([0.1, 0.2, 0.3]), &colors, Some(2), Some(3), Some([0.9; 3])), Some(2));
        assert_eq!(selected(&CellPaint::default(), &colors, Some(2), Some(3), None), None);
        // The default swatch's own colour never matches by itself.
        assert_eq!(selected(&p([1.0, 0.94, 0.83]), &colors, Some(2), Some(3), Some([0.9; 3])), Some(2));
    }
}
