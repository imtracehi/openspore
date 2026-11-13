//! The retail callout dialogs and first-minute hints of the Cell stage.
//!
//! * **Callouts** (`Callout_CLG_Opening`, `..._ProgressBar`, `..._Ending`,
//!   `..._TransitionButton`): property files of group `calloutDialogs`
//!   (`0x3629F036`) with the title and text (strings of the `callouts` locale
//!   table) and the HUD windows the callout points at. The dialog is the
//!   installed layout `0x02A6B9AE`: a gold frame with a title band, text, a
//!   green check button and a big arrow that pops in (Fade + Inflate of its
//!   root window).
//! * **Hints** (`clg-move`, `clg-carnivore`, `clg-herbivore`): property files
//!   of group `hints` (`0x0522DE06`) whose text comes from the `HintsCLG`
//!   table (`<hinticon>` marks an inline icon, a PNG named by the property)
//!   shown on the installed `hint` layout, a gold bar with white text.
//!
//! The layouts carry all art. The game code positions them (RE of the
//! march2017 image, the UI hint / callout manager `0x67AAB0` / `0x67A170` /
//! `0x6795F0` and the hint bar `0x67BBF0`):
//!
//! * a callout's text window is moved to the fractions `0x0678B195` (x) and
//!   `0x064BD8EF` (y) of the property file, default 0.5, measured from the
//!   right / bottom of the screen (`pos = min + (max - min) * (1 - f)`,
//!   `0x67A2FD`); the arrow's target is the centre of the target window
//!   (`0x6797B8`) and the arrow is turned towards it and slid along that
//!   line to the target's near edge (`0x678380`);
//! * the hint bar is as wide as its text pieces plus 10 px, centred on the
//!   screen at its authored height; the inline icon sits 3 px after the first
//!   text; the gold frame and black text are shown only for hints whose
//!   property `0x060472A8` is set, the others are plain white text (`0x67BCF1`).
//!
//! * the hint fades with [`HintFade`] (retail 1 s in, 2 s out, linear alpha on
//!   the text and icon windows, `0x67ACC0`).
//!
//! Still ADAPTED: the other stages' callout placement ([`place_callout`]).
//! Text, strings, art and effects are the original data.

use std::collections::HashMap;

use bevy::prelude::*;

use super::CellHudRoot;
use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellHudState,
    dbpf::ResourceKey,
    spui::{
        model::{ImageRef, WinId},
        solve::Rect,
        view::{SpuiAssets, SpuiState},
    },
};

/// Callout dialog layout (its name is not in the registry).
pub const CALLOUT_LAYOUT: &str = "0x02A6B9AE";
pub const HINT_LAYOUT: &str = "hint";

/// Control ids of the callout layout.
pub mod callout {
    pub const ROOT: u32 = 0x0631_8E78;
    pub const TITLE: u32 = 0x0151_0D07;
    pub const TEXT: u32 = 0x0151_0D08;
    /// The green check button that closes the dialog.
    pub const OK: u32 = 0xFFFF_FFF1;
    /// Soft glow behind the check button (the layout shows it on hover).
    pub const OK_GLOW: u32 = 0x0594_3B78;
    pub const ARROW: u32 = 0x064B_E838;
    /// Elements (no control id): the gold frame, the group of title/button.
    pub const FRAME_ELEMENT: usize = 42;
}

/// Control ids of the hint layout.
pub mod hint {
    pub const BAR: u32 = 0x0524_6048;
    /// The gold 9-slice frame (shown only for gold hints).
    pub const FRAME: u32 = 0x0652_5790;
    /// Inline icon, the text before / after it, and their black copies.
    pub const ICON: u32 = 0x0524_4C08;
    pub const BEFORE: u32 = 0x0524_32E0;
    pub const AFTER: u32 = 0x0524_4C68;
    pub const SHADOW_BEFORE: u32 = 0x0524_32E1;
    pub const SHADOW_AFTER: u32 = 0x0524_32E2;
}

/// Callout property file group (`calloutDialogs`) and property ids.
const CALLOUT_GROUP: u32 = 0x3629_F036;
const PROP_TEXT: u32 = 0x4629_EDB4; // dialogText
const PROP_TITLE: u32 = 0x6B82_41C9; // dialogTitle
const PROP_TARGET: u32 = 0x0615_A45E;
const PROP_TARGETS: u32 = 0x064B_D8E8;
/// Placement fractions of the dialog (x, y; default 0.5 each).
const PROP_POS_X: u32 = 0x0678_B195;
const PROP_POS_Y: u32 = 0x064B_D8EF;
/// Hint flag `def+0x76`: gold frame and black text (bool).
const PROP_HINT_GOLD: u32 = 0x0604_72A8;
/// Hint property file group (`hints`) and property ids.
const HINT_GROUP: u32 = 0x0522_DE06;
const PROP_HINT_TEXT: u32 = 0x0522_DE0E;
const PROP_HINT_ICON: u32 = 0x0522_DE10;
/// Inline icon marker of hint strings.
const HINT_ICON_TAG: &str = "<hinticon>";

/// Arrow geometry of the callout layout (the arrow window is 90x60 and points
/// right; 55 px of it stick out of the frame in the authored layout).
const ARROW_LENGTH: f32 = 90.0;
const ARROW_WIDTH: f32 = 60.0;
const ARROW_REACH: f32 = 55.0;
/// Space between the arrow tip and its target, and to the screen edge.
const TARGET_GAP: f32 = 6.0;
const SCREEN_PAD: f32 = 8.0;

/// What a callout shows and points at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalloutDef {
    pub title: String,
    pub text: String,
    /// Control ids of the HUD windows it points at (the first is the arrow's
    /// target, the others are highlighted with it).
    pub targets: Vec<u32>,
}

/// What a hint shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HintDef {
    pub before: String,
    /// Text after the inline icon (only with an icon).
    pub after: String,
    pub icon: Option<ResourceKey>,
    /// Property `0x060472A8`: gold bar with black text (the others show
    /// plain white text without a frame).
    pub gold: bool,
}

/// Loaded callout/hint definitions and the hint's fade state.
#[derive(Resource, Default)]
pub struct DialogCache {
    callouts: HashMap<String, Option<CalloutDef>>,
    /// Placement fractions of the callouts read so far.
    fractions: HashMap<String, (f32, f32)>,
    hints: HashMap<String, Option<HintDef>>,
    hint_shown: Option<String>,
    hint_fade: HintFade,
}

/// Locale strings mark paragraphs with `~p~`.
pub fn paragraphs(text: &str) -> String {
    text.replace("~p~", "\n\n").trim_end().to_string()
}

pub(super) fn string_of(
    props: &crate::prop::PropertyList,
    id: u32,
    index: &ResourceIndex,
    assets: &mut SpuiAssets,
) -> String {
    props
        .0
        .get(&id)
        .and_then(|p| p.text())
        .map(|(table, text_id, literal)| {
            if table != 0 {
                assets.localized(index, table, text_id).unwrap_or(literal)
            } else {
                literal
            }
        })
        .unwrap_or_default()
}

/// Window ids (big endian `u32` values) of a property, zero meaning none.
fn ids_of(props: &crate::prop::PropertyList, id: u32) -> Vec<u32> {
    props
        .0
        .get(&id)
        .map(|p| {
            p.values
                .iter()
                .filter_map(|v| v.get(..4))
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                .filter(|v| *v != 0)
                .collect()
        })
        .unwrap_or_default()
}

/// Read a callout definition by its registered name (`Callout_CLG_Opening`).
pub fn read_callout(index: &ResourceIndex, assets: &mut SpuiAssets, name: &str) -> Option<CalloutDef> {
    let key = ResourceKey {
        group_id: CALLOUT_GROUP,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let props = index.properties(key).ok()?;
    let mut targets = ids_of(&props, PROP_TARGET);
    targets.extend(ids_of(&props, PROP_TARGETS));
    Some(CalloutDef {
        title: paragraphs(&string_of(&props, PROP_TITLE, index, assets)),
        text: paragraphs(&string_of(&props, PROP_TEXT, index, assets)),
        targets,
    })
}

/// Read a hint definition by its id (`clg-move`).
pub fn read_hint(index: &ResourceIndex, assets: &mut SpuiAssets, id: &str) -> Option<HintDef> {
    let key = ResourceKey {
        group_id: HINT_GROUP,
        instance_id: crate::world_objects::name_id(id),
        type_id: crate::asset_catalog::PROP,
    };
    let props = index.properties(key).ok()?;
    let text = string_of(&props, PROP_HINT_TEXT, index, assets);
    let gold = props.0.get(&PROP_HINT_GOLD).and_then(|p| p.number()).is_some_and(|v| v != 0.0);
    let icon = props.0.get(&PROP_HINT_ICON).and_then(|p| p.key()).map(|mut k| {
        // The icon is named without a type: an installed UI PNG.
        if k.type_id == 0 {
            k.type_id = crate::ui_art::UI_PNG_TYPE;
        }
        k
    });
    Some(HintDef { gold, ..split_hint(&text, icon) })
}

/// Placement fractions of a callout (`0x0678B195`, `0x064BD8EF`; the parent
/// property file of the targeted callouts holds y = 0.75).
pub fn read_callout_fractions(index: &ResourceIndex, name: &str) -> (f32, f32) {
    let key = ResourceKey {
        group_id: CALLOUT_GROUP,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let Ok(props) = index.properties(key) else {
        return (0.5, 0.5);
    };
    let read = |id: u32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(0.5);
    (read(PROP_POS_X), read(PROP_POS_Y))
}

/// Split a hint string at its inline icon.
pub fn split_hint(text: &str, icon: Option<ResourceKey>) -> HintDef {
    match (icon, text.split_once(HINT_ICON_TAG)) {
        (Some(icon), Some((before, after))) => HintDef {
            before: before.trim().to_string(),
            after: after.trim().to_string(),
            icon: Some(icon),
            gold: false,
        },
        _ => HintDef {
            before: text.replace(HINT_ICON_TAG, "").trim().to_string(),
            after: String::new(),
            icon: None,
            gold: false,
        },
    }
}

/// Which way the callout arrow points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointing {
    Right,
    Left,
    Down,
    Up,
}

impl Pointing {
    /// Rotation of the (right pointing) arrow art, clockwise on screen.
    pub fn radians(self) -> f32 {
        match self {
            Pointing::Right => 0.0,
            Pointing::Down => std::f32::consts::FRAC_PI_2,
            Pointing::Left => std::f32::consts::PI,
            Pointing::Up => -std::f32::consts::FRAC_PI_2,
        }
    }
}

/// How far the dialog frame reaches beyond the text area on each side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Where the callout goes: the text area (the layout's root window) and, with
/// a target, the arrow window and the angle (radians, clockwise on screen,
/// 0 = pointing right like the arrow art) it is turned by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub text_area: Rect,
    pub arrow: Option<(Rect, f32)>,
}

/// How a callout is placed: next to its target (the other stages' HUDs) or at
/// the retail fractions of the screen (the Cell stage).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalloutAt {
    /// ADAPTED: beside the target (`centre_y` without one).
    Near { centre_y: f32 },
    /// The retail rule: the properties `0x0678B195` / `0x064BD8EF` (x, y).
    Fractions(f32, f32),
}

/// ADAPTED placement. Without a target the frame is centred horizontally and
/// vertically around `centre_y` (a fraction of the screen height, 0.5 =
/// centred). With one the dialog sits on the side of the target that faces
/// the screen centre, the arrow leaves the frame edge towards the target and
/// its tip stops [`TARGET_GAP`] short of it; the dialog stays on screen.
pub fn place_callout(
    screen: Vec2,
    text: Vec2,
    margins: Margins,
    target: Option<Rect>,
    centre_y: f32,
) -> Placement {
    let frame_w = text.x + margins.left + margins.right;
    let frame_h = text.y + margins.top + margins.bottom;
    let clamp_x = |x: f32| x.clamp(SCREEN_PAD, (screen.x - SCREEN_PAD - frame_w).max(SCREEN_PAD));
    let clamp_y = |y: f32| y.clamp(SCREEN_PAD, (screen.y - SCREEN_PAD - frame_h).max(SCREEN_PAD));
    let text_area = |frame_x: f32, frame_y: f32| {
        Rect::new(
            frame_x + margins.left,
            frame_y + margins.top,
            frame_x + margins.left + text.x,
            frame_y + margins.top + text.y,
        )
    };
    let Some(t) = target else {
        return Placement {
            text_area: text_area(
                clamp_x((screen.x - frame_w) * 0.5),
                clamp_y(screen.y * centre_y - frame_h * 0.5),
            ),
            arrow: None,
        };
    };
    let (tx, ty) = t.center();
    let (dx, dy) = ((tx - screen.x * 0.5) / screen.x, (ty - screen.y * 0.5) / screen.y);
    let pointing = if dy.abs() >= dx.abs() {
        if dy > 0.0 { Pointing::Down } else { Pointing::Up }
    } else if dx > 0.0 {
        Pointing::Right
    } else {
        Pointing::Left
    };
    let (frame_x, frame_y, centre) = match pointing {
        Pointing::Down => {
            let (x, y) = (clamp_x(tx - frame_w * 0.5), clamp_y(t.y1 - TARGET_GAP - ARROW_REACH - frame_h));
            let cx = tx.clamp(x + ARROW_WIDTH * 0.5, x + frame_w - ARROW_WIDTH * 0.5);
            (x, y, (cx, y + frame_h + ARROW_REACH - ARROW_LENGTH * 0.5))
        }
        Pointing::Up => {
            let (x, y) = (clamp_x(tx - frame_w * 0.5), clamp_y(t.y2 + TARGET_GAP + ARROW_REACH));
            let cx = tx.clamp(x + ARROW_WIDTH * 0.5, x + frame_w - ARROW_WIDTH * 0.5);
            (x, y, (cx, y - ARROW_REACH + ARROW_LENGTH * 0.5))
        }
        Pointing::Right => {
            let (x, y) = (clamp_x(t.x1 - TARGET_GAP - ARROW_REACH - frame_w), clamp_y(ty - frame_h * 0.5));
            let cy = ty.clamp(y + ARROW_WIDTH * 0.5, y + frame_h - ARROW_WIDTH * 0.5);
            (x, y, (x + frame_w + ARROW_REACH - ARROW_LENGTH * 0.5, cy))
        }
        Pointing::Left => {
            let (x, y) = (clamp_x(t.x2 + TARGET_GAP + ARROW_REACH), clamp_y(ty - frame_h * 0.5));
            let cy = ty.clamp(y + ARROW_WIDTH * 0.5, y + frame_h - ARROW_WIDTH * 0.5);
            (x, y, (x - ARROW_REACH + ARROW_LENGTH * 0.5, cy))
        }
    };
    Placement {
        text_area: text_area(frame_x, frame_y),
        arrow: Some((
            Rect::new(
                centre.0 - ARROW_LENGTH * 0.5,
                centre.1 - ARROW_WIDTH * 0.5,
                centre.0 + ARROW_LENGTH * 0.5,
                centre.1 + ARROW_WIDTH * 0.5,
            ),
            pointing.radians(),
        )),
    }
}

/// Retail placement. The text window (the layout's root) moves to
/// `min + (max - min) * (1 - f)` of the screen on each axis, `max` being the
/// position that keeps the window on screen (unchanged when it does not fit;
/// `0x67A2FD`). With a target the arrow window (authored `arrow_rel`, relative
/// to the text window) is turned by the angle of the unit vector `u` from the
/// text window's centre to the target window's centre and slid along it by
/// `t - half the arrow length`, `t` being the distance to the target
/// window's near edge (`0x678380`: a bisection to 1 px, here exact).
/// ADAPTED: the retail code slides the arrow from its authored spot (about
/// 190 px right of the text window's centre), which leaves its tip beside the
/// target and, for the Advance button in the corner, off the screen; here the
/// slide starts at the text window's centre, so the tip touches the target's
/// edge on the line `u`.
pub fn place_callout_at(
    screen: Vec2,
    text: Vec2,
    target: Option<Rect>,
    fractions: (f32, f32),
    arrow_rel: [f32; 4],
) -> Placement {
    let place = |size: f32, extent: f32, f: f32| {
        let max = extent - size;
        if max > 0.0 { max * (1.0 - f) } else { 0.0 }
    };
    let (x, y) = (place(text.x, screen.x, fractions.0), place(text.y, screen.y, fractions.1));
    let text_area = Rect::new(x, y, x + text.x, y + text.y);
    let Some(t) = target else {
        return Placement { text_area, arrow: None };
    };
    let (cx, cy) = text_area.center();
    let (tx, ty) = t.center();
    let (dx, dy) = (tx - cx, ty - cy);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return Placement { text_area, arrow: None };
    }
    let (ux, uy) = (dx / len, dy / len);
    // Distance from the target's centre back to its edge along the line.
    let reach = |half: f32, u: f32| if u.abs() < 1e-6 { f32::INFINITY } else { half / u.abs() };
    let inside = reach(t.width() * 0.5, ux).min(reach(t.height() * 0.5, uy));
    let to_edge = (len - inside).max(0.0);
    let (half_w, half_h) = ((arrow_rel[2] - arrow_rel[0]) * 0.5, (arrow_rel[3] - arrow_rel[1]) * 0.5);
    let slide = to_edge - half_w;
    let (ax, ay) = (cx + ux * slide, cy + uy * slide);
    Placement {
        text_area,
        arrow: Some((Rect::new(ax - half_w, ay - half_h, ax + half_w, ay + half_h), uy.atan2(ux))),
    }
}

/// Window of a layout element (windows without a control id).
pub(super) fn by_element(ui: &SpuiState, element: usize) -> Option<WinId> {
    ui.layout.windows.iter().find(|w| w.element == element).map(|w| w.id)
}

/// Rough text height before the first measurement: lines of ~55 characters
/// at 17 px.
fn estimated_height(text: &str) -> f32 {
    let lines: usize = text
        .split('\n')
        .map(|l| (l.chars().count() as f32 / 55.0).ceil().max(1.0) as usize)
        .sum();
    lines as f32 * 17.0
}

/// Fill and place a callout dialog layout beside its target (`centre_y` is
/// the vertical centre of a dialog without a target, as a fraction of the
/// screen height).
pub fn bind_callout(ui: &mut SpuiState, def: Option<&CalloutDef>, target: Option<Rect>, centre_y: f32) {
    bind_callout_at(ui, def, target, CalloutAt::Near { centre_y });
}

/// Fill and place a callout dialog layout.
pub fn bind_callout_at(ui: &mut SpuiState, def: Option<&CalloutDef>, target: Option<Rect>, at: CalloutAt) {
    let Some(root) = ui.find(callout::ROOT) else {
        return;
    };
    let (Some(def), true) = (def, ui.ready) else {
        ui.set_visible(root, false);
        return;
    };
    let (Some(title), Some(text), Some(arrow), Some(frame)) = (
        ui.find(callout::TITLE),
        ui.find(callout::TEXT),
        ui.find(callout::ARROW),
        by_element(ui, callout::FRAME_ELEMENT),
    ) else {
        return;
    };
    ui.set_text(title, def.title.clone());
    ui.set_text(text, def.text.clone());
    // The root window is the text area; the frame hugs it by anchored
    // margins, the text window is positioned by the code (authored offsets).
    let authored_root = ui.layout.window(root).area;
    let authored_text = ui.layout.window(text).area;
    // A measurement of a few pixels is a layout that has not seen the text
    // yet: keep the estimate until a real one arrives.
    let text_h = ui
        .text_extent(text)
        .filter(|e| e.y >= 4.0)
        .map_or_else(|| estimated_height(&def.text), |e| e.y.ceil())
        .max(17.0);
    let text_w = authored_text[2] - authored_text[0];
    let below = (authored_root[3] - authored_root[1]) - authored_text[3];
    let (root_rect, frame_rect) = (ui.rect(root), ui.rect(frame));
    let margins = Margins {
        left: root_rect.x1 - frame_rect.x1,
        top: root_rect.y1 - frame_rect.y1,
        right: frame_rect.x2 - root_rect.x2,
        bottom: frame_rect.y2 - root_rect.y2,
    };
    let root_w = authored_root[2] - authored_root[0];
    let root_h = authored_text[1] + text_h + below;
    // The frame reaches `margins` beyond the root; place the root window.
    let screen = Vec2::new(ui.screen[0], ui.screen[1]);
    let size = Vec2::new(root_w, root_h);
    let placement = match at {
        CalloutAt::Near { centre_y } => place_callout(screen, size, margins, target, centre_y),
        CalloutAt::Fractions(fx, fy) => {
            place_callout_at(screen, size, target, (fx, fy), ui.layout.window(arrow).area)
        }
    };
    let at = placement.text_area;
    ui.set_area(root, [at.x1, at.y1, at.x2, at.y2]);
    ui.set_area(
        text,
        [authored_text[0], authored_text[1], authored_text[0] + text_w, authored_text[1] + text_h],
    );
    match placement.arrow {
        Some((rect, angle)) => {
            ui.set_area(arrow, [rect.x1 - at.x1, rect.y1 - at.y1, rect.x2 - at.x1, rect.y2 - at.y1]);
            ui.set_rotation(arrow, angle);
            ui.set_visible(arrow, true);
        }
        None => ui.set_visible(arrow, false),
    }
    // The glow behind the check button belongs to its hover state.
    if let (Some(ok), Some(glow)) = (ui.find(callout::OK), ui.find(callout::OK_GLOW)) {
        let hover = ui.states[ok] & crate::spui::ids::state::HOVER != 0;
        ui.set_visible(glow, hover);
    }
    ui.set_visible(root, true);
}

/// Padding of the hint bar around its content, the gap between a text and the
/// inline icon (`0x67C16D` adds 10, `0x67C04D` adds 3) and the room the
/// two-line form needs.
const HINT_PAD: f32 = 10.0;
const HINT_ICON_GAP: f32 = 3.0;
/// A hint wider than the screen minus this wraps onto two lines (`0x67BF11`).
const HINT_WRAP_MARGIN: f32 = 20.0;

/// Fill and place the hint bar (`0x67BBF0`). `alpha` is the fade of the bar
/// (0 hides it).
pub fn bind_hint(
    ui: &mut SpuiState,
    def: Option<&HintDef>,
    alpha: f32,
    icon: Option<ImageRef>,
) {
    let Some(bar) = ui.find(hint::BAR) else {
        return;
    };
    let (Some(def), true) = (def.filter(|_| alpha > 0.0), ui.ready) else {
        ui.set_visible(bar, false);
        return;
    };
    let (Some(before), Some(after), Some(inline), Some(shadow_before), Some(shadow_after), Some(frame)) = (
        ui.find(hint::BEFORE),
        ui.find(hint::AFTER),
        ui.find(hint::ICON),
        ui.find(hint::SHADOW_BEFORE),
        ui.find(hint::SHADOW_AFTER),
        ui.find(hint::FRAME),
    ) else {
        return;
    };
    let has_icon = icon.is_some();
    // 0x67BCF1: gold hints show the black text pair over the gold frame, the
    // others the white pair and no frame.
    let (first, second, unused_first, unused_second) =
        if def.gold { (shadow_before, shadow_after, before, after) } else { (before, after, shadow_before, shadow_after) };
    ui.set_text(first, def.before.clone());
    ui.set_text(second, def.after.clone());
    ui.set_visible(first, true);
    ui.set_visible(second, has_icon);
    ui.set_visible(unused_first, false);
    ui.set_visible(unused_second, false);
    ui.set_visible(frame, def.gold);
    ui.set_image(inline, icon.clone());
    ui.set_visible(inline, has_icon);
    // Authored (parent relative) geometry: the first text 10 px inside the
    // bar, 20 px high; the icon window 33 x 30.
    let authored_bar = ui.layout.window(bar).area;
    let authored_text = ui.layout.window(before).area;
    let authored_icon = ui.layout.window(inline).area;
    let bar_h = authored_bar[3] - authored_bar[1];
    let (text_x, text_y) = (authored_text[0], authored_text[1]);
    let line_h = authored_text[3] - authored_text[1];
    // Text bounds; before the first measurement the windows are wide so that
    // the layout can measure them. ADAPTED: 2 px of slack against clipped
    // last glyphs of our text drawing.
    let width_of = |ui: &SpuiState, w: WinId| ui.text_extent(w).filter(|e| e.x > 2.0).map(|e| e.x.ceil() + 2.0);
    let (m1, m2) = (width_of(ui, first), if has_icon { width_of(ui, second) } else { Some(0.0) });
    let (w1, w2) = (m1.unwrap_or(640.0), m2.unwrap_or(640.0));
    let screen_w = ui.screen[0];
    let wrapped = w1 + HINT_WRAP_MARGIN > screen_w;
    let (text_w, extra_h) = if wrapped { (screen_w - HINT_WRAP_MARGIN, line_h) } else { (w1, 0.0) };
    let text_area = |x: f32, w: f32| [x, text_y, x + w, text_y + line_h + extra_h];
    for w in [before, shadow_before] {
        ui.set_area(w, text_area(text_x, text_w));
    }
    let mut x = text_x + text_w;
    if has_icon {
        // The icon is scaled to its authored width, keeping the picture's
        // aspect, about the centre line of the authored icon window.
        let icon_w = authored_icon[2] - authored_icon[0];
        let icon_h = match icon.as_ref().and_then(|i| i.dims) {
            Some([w, h]) if w > 0 => icon_w * h as f32 / w as f32,
            _ => authored_icon[3] - authored_icon[1],
        };
        let centre = (authored_icon[1] + authored_icon[3]) * 0.5;
        let ix = x + HINT_ICON_GAP;
        ui.set_area(inline, [ix, centre - icon_h * 0.5, ix + icon_w, centre + icon_h * 0.5]);
        x = ix + icon_w + HINT_ICON_GAP;
        for w in [after, shadow_after] {
            ui.set_area(w, text_area(x, w2));
        }
        x += w2;
    }
    // The bar is as wide as its content plus 10 px, centred on the screen at
    // its authored height (0x67C24D).
    let bar_w = x + HINT_PAD;
    let x0 = ((screen_w - bar_w) * 0.5).floor();
    ui.set_area(bar, [x0, authored_bar[1], x0 + bar_w, authored_bar[1] + bar_h + extra_h]);
    // `0x67ACC0` writes the fade into the shade colour (alpha byte, RGB kept)
    // of exactly three windows: the first text (0x052432E0), the second text
    // (0x05244C68) and the inline icon (0x05244C08). The black copies of a
    // gold hint and the gold frame are not touched, so they appear and vanish
    // at full strength with the layout's visibility.
    let a = ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24;
    for w in [before, after, inline] {
        let authored = ui.layout.window(w).shade_color;
        ui.set_shade(w, a | (authored & 0x00FF_FFFF));
    }
    ui.set_visible(bar, true);
}

/// Retail hint fade: `cHintManager::Update` (0x67C410) runs each hint through
/// states and `0x67ACC0` turns them into an alpha. State 4 fades in over the
/// hint's `+0x54` seconds (alpha = timer / fade-in), state 5 shows it at full
/// alpha, states 6 and 7 fade it out over `+0x58` seconds (alpha = 1 - timer /
/// fade-out). The hint structure's constructor (0x67B110) defaults are 1.0 s
/// and 2.0 s; the loader (0x67B480) overrides them from the hint property file
/// (`0x07E8708C` fade-in, `0x07E87096` fade-out), but none of the 133 installed
/// hint property files sets either, so every installed hint uses the defaults.
/// The timer restarts at 0 on each state change, so a fade-in always runs to the
/// end (state 4 has no early exit) and the fade-out starts at full alpha.
pub const HINT_FADE_IN: f32 = 1.0;
pub const HINT_FADE_OUT: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HintPhase {
    #[default]
    Hidden,
    FadeIn,
    Shown,
    FadeOut,
}

/// One hint's fade (retail states 4 to 7).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HintFade {
    pub phase: HintPhase,
    pub timer: f32,
}

impl HintFade {
    /// Alpha of the hint's windows (`0x67ACC0`).
    pub fn alpha(&self) -> f32 {
        match self.phase {
            HintPhase::Hidden => 0.0,
            HintPhase::FadeIn => (self.timer / HINT_FADE_IN).clamp(0.0, 1.0),
            HintPhase::Shown => 1.0,
            HintPhase::FadeOut => (1.0 - self.timer / HINT_FADE_OUT).clamp(0.0, 1.0),
        }
    }

    /// Advance by `dt` seconds; `want` is whether the hint is asked for (the
    /// retail state 3 selection, or the end of its life / its condition being
    /// met when false). Returns the alpha.
    pub fn step(&mut self, want: bool, dt: f32) -> f32 {
        match self.phase {
            HintPhase::Hidden => {
                if want {
                    *self = HintFade { phase: HintPhase::FadeIn, timer: 0.0 };
                }
            }
            HintPhase::FadeIn => {
                self.timer += dt;
                if self.timer > HINT_FADE_IN {
                    *self = HintFade { phase: HintPhase::Shown, timer: 0.0 };
                }
            }
            HintPhase::Shown => {
                if !want {
                    *self = HintFade { phase: HintPhase::FadeOut, timer: 0.0 };
                }
            }
            HintPhase::FadeOut => {
                self.timer += dt;
                if self.timer > HINT_FADE_OUT {
                    *self = HintFade::default();
                } else if want {
                    // ADAPTED: the hint is asked for again while it fades out
                    // (retail restarts it from state 1): fade back in from
                    // the current alpha.
                    let a = self.alpha();
                    *self = HintFade { phase: HintPhase::FadeIn, timer: a * HINT_FADE_IN };
                }
            }
        }
        self.alpha()
    }
}

/// Bind the callout and hint layouts to the published HUD state and turn a
/// click on the callout's check button into the dismiss key.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    time: Res<Time>,
    hud: Res<CellHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<DialogCache>,
    mut roots: Query<(&mut SpuiState, &CellHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // Callout: definition and the rectangle of its target window.
    let callout_name = hud.callouts.first().cloned();
    if let Some(name) = &callout_name {
        if !cache.callouts.contains_key(name) {
            let def = read_callout(index, &mut assets, name);
            cache.callouts.insert(name.clone(), def);
            cache.fractions.insert(name.clone(), read_callout_fractions(index, name));
        }
    }
    let fractions = callout_name.as_ref().and_then(|n| cache.fractions.get(n)).copied().unwrap_or((0.5, 0.5));
    let def = callout_name.as_ref().and_then(|n| cache.callouts.get(n)).cloned().flatten();
    let target = def.as_ref().and_then(|d| d.targets.first()).and_then(|control| {
        roots
            .iter()
            .find(|(_, kind)| **kind == CellHudRoot::Main)
            .and_then(|(ui, _)| ui.find(*control).filter(|w| ui.ready && ui.visible(*w)).map(|w| ui.rect(w)))
    });
    // Hint: the retail fade ([`HintFade`]) towards the published hint.
    let dt = time.delta_secs();
    match hud.hint.as_ref() {
        Some(id) => {
            if cache.hint_shown.as_ref() != Some(id) {
                cache.hint_shown = Some(id.clone());
                cache.hint_fade = HintFade::default();
            }
            cache.hint_fade.step(true, dt);
        }
        None => {
            if cache.hint_fade.step(false, dt) <= 0.0 {
                cache.hint_shown = None;
            }
        }
    }
    let hint_id = cache.hint_shown.clone();
    if let Some(id) = &hint_id {
        if !cache.hints.contains_key(id) {
            let def = read_hint(index, &mut assets, id);
            cache.hints.insert(id.clone(), def);
        }
    }
    let hint_def = hint_id.as_ref().and_then(|id| cache.hints.get(id)).cloned().flatten();
    let icon = hint_def.as_ref().and_then(|d| d.icon).and_then(|key| {
        // The picture's size sets the inline icon's aspect (0x67C0C2).
        assets.texture(index, &mut images, key).map(|tex| ImageRef {
            file: key,
            uv: [0.0, 0.0, 1.0, 1.0],
            dims: Some(tex.size),
        })
    });
    let alpha = cache.hint_fade.alpha();
    let mut dismiss = false;
    for (mut ui, kind) in &mut roots {
        match kind {
            CellHudRoot::Callout => {
                bind_callout_at(&mut ui, def.as_ref(), target, CalloutAt::Fractions(fractions.0, fractions.1));
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == callout::OK {
                        dismiss = true;
                    }
                }
            }
            CellHudRoot::Hint => bind_hint(&mut ui, hint_def.as_ref(), alpha, icon.clone()),
            _ => {}
        }
    }
    if dismiss && !keys.pressed(crate::cell_flow::DISMISS_KEY) {
        keys.press(crate::cell_flow::DISMISS_KEY);
        injected.push(crate::cell_flow::DISMISS_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = Vec2::new(1280.0, 900.0);
    /// The authored arrow window relative to the text window (layout
    /// `0x02A6B9AE`: the 90 x 60 arrow starts 15 px inside the right edge).
    const ARROW_REL: [f32; 4] = [308.0, 2.0, 398.0, 62.0];
    const MARGINS: Margins = Margins {
        left: 23.0,
        top: 50.0,
        right: 20.0,
        bottom: 45.0,
    };

    #[test]
    fn hints_fade_in_for_a_second_and_out_for_two() {
        let mut f = HintFade::default();
        assert_eq!(f.step(false, 0.1), 0.0, "nothing asked: stays hidden");
        assert_eq!(f.step(true, 0.0), 0.0);
        assert!((f.step(true, 0.5) - 0.5).abs() < 1e-5, "linear fade-in over 1 s");
        // Withdrawn mid fade-in: state 4 has no early exit, it runs to full alpha.
        assert!(f.step(false, 0.4) > 0.89);
        assert_eq!(f.step(false, 0.2), 1.0, "shown at full alpha");
        assert_eq!(f.step(false, 0.0), 1.0, "the fade-out starts at full alpha");
        assert!((f.step(false, 1.0) - 0.5).abs() < 1e-5, "linear fade-out over 2 s");
        assert!(f.step(false, 0.9) > 0.0 && f.step(false, 0.2) == 0.0);
        assert_eq!(f.phase, HintPhase::Hidden);
        // Asked for again while fading out: back in from the current alpha.
        let mut g = HintFade { phase: HintPhase::FadeOut, timer: 1.0 };
        assert!((g.step(true, 0.0) - 0.5).abs() < 1e-5);
        assert_eq!(g.phase, HintPhase::FadeIn);
    }

    #[test]
    fn paragraph_markers_become_blank_lines() {
        assert_eq!(paragraphs("a~p~b~p~"), "a\n\nb");
        assert_eq!(paragraphs("one line"), "one line");
    }

    #[test]
    fn hint_text_splits_at_the_inline_icon() {
        let icon = ResourceKey {
            group_id: 1,
            instance_id: 2,
            type_id: 3,
        };
        let d = split_hint("Eat <hinticon> to grow", Some(icon));
        assert_eq!((d.before.as_str(), d.after.as_str(), d.icon), ("Eat", "to grow", Some(icon)));
        // No icon: the marker is dropped and the text stays one piece.
        let d = split_hint("Eat <hinticon> to grow", None);
        assert_eq!((d.before.as_str(), d.icon), ("Eat  to grow", None));
        let d = split_hint("Use the mouse", None);
        assert_eq!(d.before, "Use the mouse");
    }

    #[test]
    fn dialog_without_a_target_is_centred() {
        let p = place_callout(SCREEN, Vec2::new(323.0, 100.0), MARGINS, None, 0.5);
        assert!(p.arrow.is_none());
        let frame_w = 323.0 + 43.0;
        let frame_x = p.text_area.x1 - MARGINS.left;
        assert_eq!(frame_x + frame_w / 2.0, 640.0);
        assert_eq!(p.text_area.height(), 100.0);
    }

    #[test]
    fn dialog_above_a_bottom_target_points_down_at_it() {
        // The progress bar's first segment at the bottom of the screen.
        let target = Rect::new(230.0, 871.0, 400.0, 897.0);
        let p = place_callout(SCREEN, Vec2::new(323.0, 120.0), MARGINS, Some(target), 0.5);
        let (arrow, pointing) = p.arrow.unwrap();
        assert_eq!(pointing, Pointing::Down.radians());
        // The arrow tip is just above the target, over the target's centre.
        // The arrow art points right and is turned: its tip is half a length from the centre.
        let tip_y = arrow.center().1 + ARROW_LENGTH * 0.5;
        assert!((tip_y - (target.y1 - TARGET_GAP)).abs() < 0.01, "tip {tip_y}");
        assert!((arrow.center().0 - 315.0).abs() < 0.01);
        // The frame ends where the visible arrow starts and stays on screen.
        let frame_bottom = p.text_area.y2 + MARGINS.bottom;
        assert!((frame_bottom + ARROW_REACH - tip_y).abs() < 0.01);
        assert!(p.text_area.x1 - MARGINS.left >= SCREEN_PAD);
    }

    #[test]
    fn dialog_near_the_right_edge_is_pushed_in_but_the_arrow_keeps_its_target() {
        // The Advance button in the bottom right corner.
        let target = Rect::new(1203.0, 870.0, 1270.0, 898.0);
        let p = place_callout(SCREEN, Vec2::new(323.0, 80.0), MARGINS, Some(target), 0.5);
        let (arrow, pointing) = p.arrow.unwrap();
        assert_eq!(pointing, Pointing::Down.radians());
        let frame_right = p.text_area.x2 + MARGINS.right;
        assert!(frame_right <= SCREEN.x - SCREEN_PAD + 0.01);
        // The arrow stays above the button and inside the frame's width.
        assert!(arrow.center().0 > 1180.0 && arrow.center().0 <= frame_right);
    }

    #[test]
    fn side_targets_get_horizontal_arrows() {
        let right = Rect::new(1200.0, 400.0, 1260.0, 460.0);
        let (_, pointing) = place_callout(SCREEN, Vec2::new(323.0, 80.0), MARGINS, Some(right), 0.5).arrow.unwrap();
        assert_eq!(pointing, Pointing::Right.radians());
        let left = Rect::new(10.0, 400.0, 60.0, 460.0);
        let (arrow, pointing) = place_callout(SCREEN, Vec2::new(323.0, 80.0), MARGINS, Some(left), 0.5).arrow.unwrap();
        assert_eq!(pointing, Pointing::Left.radians());
        assert!(arrow.center().0 > left.x2);
    }

    #[test]
    fn retail_fractions_are_measured_from_the_right_and_bottom() {
        // Default 0.5 / 0.5: the text window is centred on the screen.
        let p = place_callout_at(SCREEN, Vec2::new(323.0, 100.0), None, (0.5, 0.5), ARROW_REL);
        assert_eq!(p.text_area, Rect::new(478.5, 400.0, 801.5, 500.0));
        assert!(p.arrow.is_none());
        // The ProgressBar / TransitionButton callouts have y = 0.75: a quarter
        // of the free height from the top (`min + (max - min) * (1 - f)`).
        let p = place_callout_at(SCREEN, Vec2::new(323.0, 100.0), None, (0.5, 0.75), ARROW_REL);
        assert_eq!(p.text_area.y1, 200.0);
        // A window larger than the screen stays where the layout put it (0).
        let p = place_callout_at(Vec2::new(300.0, 100.0), Vec2::new(323.0, 80.0), None, (0.5, 0.5), ARROW_REL);
        assert_eq!(p.text_area.x1, 0.0);
    }

    #[test]
    fn the_retail_arrow_turns_towards_the_target_and_slides_to_its_edge() {
        // A target straight below the text window's centre: the arrow points
        // down and its tip (half a length past its centre) reaches the target's
        // top edge.
        let target = Rect::new(570.0, 800.0, 710.0, 840.0);
        let p = place_callout_at(SCREEN, Vec2::new(323.0, 100.0), Some(target), (0.5, 0.5), ARROW_REL);
        let (arrow, angle) = p.arrow.unwrap();
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "angle {angle}");
        let tip_y = arrow.center().1 + ARROW_LENGTH * 0.5;
        // text centre y = 450, target edge y = 800: t = 350, slide = 350 - 45.
        assert!((arrow.center().1 - 755.0).abs() < 1e-3);
        assert!((arrow.center().0 - 640.0).abs() < 1e-3);
        assert!((tip_y - 800.0).abs() < 1e-3);
        // The arrow keeps its authored size and shifts as a whole.
        assert!((arrow.width() - (ARROW_REL[2] - ARROW_REL[0])).abs() < 1e-3);
        // A target to the right: the arrow stays turned right.
        let right = Rect::new(1200.0, 440.0, 1260.0, 460.0);
        let (_, angle) = place_callout_at(SCREEN, Vec2::new(323.0, 100.0), Some(right), (0.5, 0.5), ARROW_REL).arrow.unwrap();
        assert!(angle.abs() < 0.05, "angle {angle}");
    }

    #[test]
    fn installed_callouts_and_hints_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let opening = read_callout(&index, &mut assets, "Callout_CLG_Opening").expect("opening callout");
        assert_eq!(opening.title, "Welcome To The Tidepool");
        assert!(opening.text.starts_with("Throughout Spore"));
        assert!(opening.text.contains("

"), "paragraph markers become blank lines");
        assert!(opening.targets.is_empty());
        let bar = read_callout(&index, &mut assets, "Callout_CLG_ProgressBar").expect("progress bar callout");
        assert_eq!(bar.title, "Making Progress");
        // The arrow points at the first progress segment; all five are highlighted.
        assert_eq!(bar.targets.len(), 5);
        assert_eq!(bar.targets[0], 0x051B_54C0);
        let advance = read_callout(&index, &mut assets, "Callout_CLG_TransitionButton").expect("advance callout");
        assert_eq!(advance.targets, vec![0x0645_5C5E]);
        let ending = read_callout(&index, &mut assets, "Callout_CLG_Ending").expect("ending callout");
        assert_eq!(ending.title, "Congratulations!");
        let carnivore = read_hint(&index, &mut assets, "clg-carnivore").expect("carnivore hint");
        assert_eq!((carnivore.before.as_str(), carnivore.after.as_str()), ("Eat", "to grow"));
        assert!(carnivore.icon.is_some());
        let moving = read_hint(&index, &mut assets, "clg-move").expect("move hint");
        assert!(moving.icon.is_none() && moving.before.contains("W,A,S,D"));
        // Only some hints are gold bars with black text (property 0x060472A8).
        assert!(!moving.gold && !carnivore.gold);
        assert!(read_hint(&index, &mut assets, "crg-movecreature").expect("creature hint").gold);
        // Callout placement fractions: the targeted callouts inherit y = 0.75.
        assert_eq!(read_callout_fractions(&index, "Callout_CLG_Opening"), (0.5, 0.5));
        assert_eq!(read_callout_fractions(&index, "Callout_CLG_ProgressBar"), (0.5, 0.75));
        assert_eq!(read_callout_fractions(&index, "Callout_CLG_TransitionButton"), (0.5, 0.75));
    }

    #[test]
    fn installed_dialog_layouts_have_the_windows_the_binding_uses() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let callout_layout = assets.layout(&index, CALLOUT_LAYOUT).expect("callout layout");
        for control in [callout::ROOT, callout::TITLE, callout::TEXT, callout::OK, callout::OK_GLOW, callout::ARROW] {
            assert!(callout_layout.find(control).is_some(), "callout window {control:08X}");
        }
        assert!(callout_layout.windows.iter().any(|w| w.element == callout::FRAME_ELEMENT));
        let hint_layout = assets.layout(&index, HINT_LAYOUT).expect("hint layout");
        for control in [hint::BAR, hint::ICON, hint::BEFORE, hint::AFTER, hint::SHADOW_BEFORE, hint::SHADOW_AFTER] {
            assert!(hint_layout.find(control).is_some(), "hint window {control:08X}");
        }
    }
}
