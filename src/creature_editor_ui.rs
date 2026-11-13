//! The Creature Creator chrome: the retail editor layouts drawn through the
//! SPUI engine and bound to the editor state (the Cell Creator's
//! `cell_editor_ui` pattern, `notes/creator-re-notes.md` section 1).
//!
//! * `GlobalEditorUI` (every editor's frame): Build / Paint / Test Drive radio
//!   buttons, undo/redo, Cancel / Save and Exit, Sporepedia, the Esc options
//!   panel (`047BC920`), the host windows of the sub layouts;
//! * `EditorPartsPalette` (top bar with the category tab strip `72DF4CEE` and
//!   the "More Categories" arrows) + `EditorPartsPaletteCategory` (panel,
//!   page area `05D1754B`, pager host `92DF6FD8`);
//! * the category tabs and the page items are built from the palette data
//!   (the retail `PaletteUI` creates them at run time): tab images from
//!   `paletteCategoryIconList` (norm/off/hl/act, `on-` when selected), item
//!   icons from `CreaturePartIcons`, the `unavailable-item` shield for parts
//!   not found yet; the page grid is the page's columns x rows (4 columns =
//!   the upgrade levels);
//! * `EditorPageControls` (pages of a category), `EditorBudget` (DNA),
//!   `EditorComplexityMeter`, `EditorNameDescribe-Tag` (name field),
//!   `EditorHeaderTray` (diet) + `EditorVerbTray` / `editorVerbIcon`
//!   (abilities), `EditorPaletteItemRollover` +
//!   `EditorPaletteRolloverAbilityLine` (part tooltip), `RolloverEditorMessage`;
//! * Paint mode: `EditorPaintPalette` + `EditorPaintPaletteCategory` (the
//!   paint styles themselves are not bound yet: open gap);
//! * Test Drive: the `PlayMode` layout (0x7518573E): animation panel
//!   `03E831E4`, backgrounds panel `044594D8`, their toggles.
//!
//! Where the retail sizing is not documented the binding is marked ADAPTED.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    },
    prelude::*,
};

use crate::{
    cell_editor_paint::id as page_id,
    cell_editor_ui::{image, plain},
    creature_editor::{self as ce, CreatureEditor, Diet, Mode, Refusal},
    creature_editor_data::{CreatureEditorData, PalettePage, ABILITY_CAPABILITIES, BACKGROUND_BUTTONS, PLAY_ANIM_BUTTONS},
    dbpf::ResourceKey,
    editor::{Action, EditorState},
    game::RunProgress,
    spui::{
        ids::{class, flag, state},
        model::{ButtonData, Drawable, Extra, ImageDrawable, Layout, WinId, Window, WindowKind},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    world_objects::name_id,
    Originals,
};

pub mod id {
    pub use crate::cell_editor_ui::id::*;
    /// Esc options panel and its buttons.
    pub const OPTIONS_TOGGLE: u32 = 0x047B_C8D8;
    pub const OPTIONS_PANEL: u32 = 0x047B_C920;
    pub const OPTIONS_RESUME: u32 = 0x0595_E0A8;
    pub const OPTIONS_GUIDE: u32 = 0x05FC_FEE8;
    pub const OPTIONS_SETTINGS: u32 = 0x047B_C978;
    pub const OPTIONS_EXIT: u32 = 0x055F_DCA8;
    pub const OPTIONS_QUIT: u32 = 0x047B_C958;
    pub const SPOREPEDIA: u32 = 0x05B6_E49C;
    pub const UNDO_REDO: u32 = 0x0578_EC50;
    pub const DEV_UI: u32 = 0x1140_129D;
    // PlayMode
    /// Container of the animation and background panels (hidden in the file).
    pub const PLAY_PANELS: u32 = 0x05AC_71D8;
    pub const PLAY_ANIMS_PANEL: u32 = 0x03E8_31E4;
    pub const PLAY_BACKGROUNDS_PANEL: u32 = 0x0445_94D8;
    pub const PLAY_ANIMATE: u32 = 0x03F4_3834;
    pub const PLAY_BACKDROPS: u32 = 0x0445_93D0;
    pub const PLAY_LOOK_AT_ME: u32 = 0x044E_EC00;
    /// PlayMode buttons with no function here (babies, pictures, movie,
    /// its own save/cancel/help): hidden.
    pub const PLAY_HIDDEN: [u32; 10] = [
        0x0445_A418, 0x0445_EA50, 0x03A8_EDE4, 0x03FC_1740, 0x0446_3E78, 0x0447_82A8, 0x044D_CBC8, 0x0461_5580,
        0x058B_74E8, 0x05B0_2188,
    ];
}

/// DNA glyph of `SporeCurrencyEA` (`editorCurrencyChar` 0x268B).
const DNA_GLYPH: char = '\u{268B}';
/// `common~!unavailable-item`: shield of parts not found yet.
const SHIELD: ResourceKey = ResourceKey {
    type_id: crate::creature_editor_data::PNG,
    group_id: 0x011C_0BDE,
    instance_id: 0x43C6_E503,
};
/// `UnlockableBackground` (layout 0x40464200!8070FE55, `itemUnlockableLayout` of the shared `RigblockTemplate`, so the
/// Creature palette uses the same art as the Cell palette, `cell_editor_ui`): the 27 x 35 shield of a part that is not
/// found yet, centred in the item window (PNG 0x31A44893!C385FB5D, uv 0..0.84375 x 0..0.546875).
const UNLOCKABLE_SHIELD: ResourceKey = ResourceKey {
    type_id: crate::creature_editor_data::PNG,
    group_id: 0x31A4_4893,
    instance_id: 0xC385_FB5D,
};
const UNLOCKABLE_SHIELD_UV: [f32; 4] = [0.0, 0.0, 0.84375, 0.546875];
const UNLOCKABLE_SHIELD_SIZE: [f32; 2] = [27.0, 35.0];
/// `global_ui_creature~!cgm_icon_sunburst_01`: the burst behind a newly unlocked part (status bit 2), the art the Cell
/// palette shows.
const NEW_ITEM_BURST: ResourceKey = ResourceKey {
    type_id: crate::creature_editor_data::PNG,
    group_id: 0x106C_7116,
    instance_id: 0x0F34_769F,
};
/// ADAPTED: the burst's size relative to the item window (measured on the retail Cell frame, burst 0.18 of the panel
/// width against an item of 0.22; no Creature frame was measured).
const NEW_ITEM_BURST_SCALE: f32 = 0.8;
/// `palettePageItemAspectRatio` (0x05ADC18D): 1 in `TemplatePalettePartsPage` and in every installed Creature page.
const ITEM_ASPECT: f32 = 1.0;
/// Category tab art size (`button-<x>-norm.png` is 50 x 52).
const TAB_WIDTH: f32 = 50.0;
/// `creature action icons` (`atk_bite`...) and the editor verb icons group.
const ACTION_ICON_GROUP: u32 = crate::ui_art::CREATURE_ACTION_ICON_GROUP;
const EDITOR_VERB_GROUP: u32 = 0x9CC3_EB58;
const DIET_ICON_GROUP: u32 = 0x7FFE_D3DF;

/// Ability icon (the `verbIconImageID` of the category's ability template /
/// `CreatureAbilities!health`, VERIFIED) and (ADAPTED, English) caption per verb category.
const VERB_ART: &[(&str, &str, u32, &str)] = &[
    ("bite", "atk_bite", ACTION_ICON_GROUP, "Bite"),
    ("charge", "atk_charge", ACTION_ICON_GROUP, "Charge"),
    ("strike", "atk_strike", ACTION_ICON_GROUP, "Strike"),
    ("spit", "atk_spit", ACTION_ICON_GROUP, "Spit"),
    ("sing", "social_vocalize", ACTION_ICON_GROUP, "Sing"),
    ("dance", "social_dance", ACTION_ICON_GROUP, "Dance"),
    ("charm", "social_gesture", ACTION_ICON_GROUP, "Charm"),
    ("pose", "social_poses", ACTION_ICON_GROUP, "Pose"),
    ("call", "mv_call", ACTION_ICON_GROUP, "Call"),
    ("jump", "mv_jump", ACTION_ICON_GROUP, "Jump"),
    ("grasp", "mv_grasper", ACTION_ICON_GROUP, "Grasp"),
    ("glide", "mv_glide", ACTION_ICON_GROUP, "Glide"),
    ("stealth", "mv_sneak", ACTION_ICON_GROUP, "Sneak"),
    ("sprint", "mv_energy", ACTION_ICON_GROUP, "Sprint"),
    ("sense", "mv_sight", ACTION_ICON_GROUP, "Sense"),
    ("speed", "stats_speedPartInfo_icon", EDITOR_VERB_GROUP, "Speed"),
    ("health", "stats_healthPartInfo_icon", EDITOR_VERB_GROUP, "Health"),
    // Tribal accessories: the `verbIconImageID` of the ability templates (`attackTemplate~` TRG_Atk,
    // `socialTemplate~` trg_social, `armorTemplate~` trg_defense, `gatherTemplate~`, `fishingTemplate~`),
    // all in the editor verb icon group; the `stats_*_icon` images are the trays' round icons.
    ("gather", "trg_gather", EDITOR_VERB_GROUP, "Gather"),
    ("fishing", "trg_fish", EDITOR_VERB_GROUP, "Fishing"),
    ("attack", "TRG_Atk", EDITOR_VERB_GROUP, "Attack"),
    ("social", "trg_social", EDITOR_VERB_GROUP, "Social"),
    ("armor", "trg_defense", EDITOR_VERB_GROUP, "Armor"),
    // Vehicles (`VehicleEditorVerbIcons~`; ADAPTED speed / military icons:
    // the trays' icon ids CEEFB113 / 533F9E1D are not in that group).
    ("vehicle_speed", "stats_motion_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Speed"),
    ("vehicle_defense", "stats_health_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Health"),
    ("vehicle_power", "stats_attack_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Power"),
    ("vehicle_militarypower", "stats_military_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Military Power"),
    ("vehicle_religiouspower", "stats_religiouspower_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Religious Power"),
    ("vehicle_economicpower", "stats_econpower_icon", crate::rigid_stats::VEHICLE_ICON_GROUP, "Economic Power"),
];

fn verb_art(verb: &str) -> Option<(ResourceKey, &'static str)> {
    VERB_ART.iter().find(|v| v.0 == verb).map(|v| {
        (
            ResourceKey {
                type_id: crate::creature_editor_data::PNG,
                group_id: v.2,
                instance_id: name_id(v.1),
            },
            v.3,
        )
    })
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorRoot {
    Frame,
    Palette,
    Category,
    Tabs,
    Page,
    Pager,
    Budget,
    Name,
    Complexity,
    Header,
    Tray(usize),
    Verb(usize, usize),
    Rollover,
    RolloverLine(usize),
    Message,
    PaintPalette,
    PaintCategory,
    Play,
}

/// Screen facts and the windows of the dynamic layouts.
#[derive(Resource, Default, Clone, Debug)]
pub struct CreatureEditorUiLayout {
    /// Item buttons of the page layout -> part PROP instance.
    pub items: Vec<(WinId, u32)>,
    /// Tab buttons -> (icon window, category index).
    pub tabs: Vec<(WinId, WinId, usize)>,
    /// Tabs that fit the strip.
    pub visible_tabs: usize,
    built: Option<BuiltKey>,
}

#[derive(Clone, Debug, PartialEq)]
struct BuiltKey {
    kind: crate::creature_editor_data::SetupKind,
    category: usize,
    page: usize,
    tab_scroll: usize,
    items: Vec<(u32, bool)>,
    verbs: Vec<Vec<(&'static str, i32)>>,
    rollover: Option<(u32, usize)>,
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

fn button(id: WinId, parent: WinId, rect: [f32; 4], control: u32) -> Window {
    Window {
        class: class::BUTTON,
        kind: WindowKind::Button,
        flags: flag::VISIBLE | flag::ENABLED,
        control_id: control,
        extra: Extra::Button(ButtonData::default()),
        ..plain(id, Some(parent), rect, None, 0xFFFF_FFFF)
    }
}

fn picture(id: WinId, parent: WinId, rect: [f32; 4], file: ResourceKey, shade: u32) -> Window {
    plain(
        id,
        Some(parent),
        rect,
        Some(Drawable::Image(ImageDrawable {
            image: Some(image(file)),
            ..Default::default()
        })),
        shade,
    )
}

/// The page of palette items at their row/column of the page area.
/// `PalettePageUI::GetItemArea` 0x5C8E60 (`palettePageUseRelativeLayout`, set by `TemplatePalettePartsPage`; ported as
/// `cell_editor_ui::item_area`, notes `cell-final2-audit.md` rows 24 / 34): the page area is divided into
/// columns x rows, an item is centred in its cell, `palettePageItemPercentageWidth` of the page width wide, as tall as
/// wide / aspect, its height limited to `palettePageItemMaxPercentageHeight` of the page height. A part not found yet
/// shows the 27 x 35 shield of `UnlockableBackground`, a newly found one a burst behind its icon.
fn page_layout(
    data: &CreatureEditorData,
    run: &RunProgress,
    palette: &crate::creature_editor_data::Palette,
    page: &PalettePage,
    area: [f32; 4],
    new_parts: &[u32],
) -> (Layout, Vec<(WinId, u32)>) {
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![plain(0, None, area, None, 0xFFFF_FFFF)];
    windows[0].flags = flag::VISIBLE | flag::ENABLED;
    let rows = page.rows.max(page.used_rows()).max(1);
    let mut items = Vec::new();
    for item in &page.items {
        let Some(info) = data.parts.get(&item.part) else {
            continue;
        };
        let r = crate::cell_editor_ui::item_area(
            w,
            h,
            page.columns.max(1),
            rows,
            item.row,
            item.column,
            page.item_width_fraction,
            ITEM_ASPECT,
            page.item_max_height_fraction,
        );
        // ADAPTED: corners snapped to whole pixels (retail draws at the float positions).
        let rect = [r[0].round(), r[1].round(), r[2].round(), r[3].round()];
        let (iw, ih) = (rect[2] - rect[0], rect[3] - rect[1]);
        let b = windows.len();
        windows.push(button(b, 0, rect, item.part));
        windows[0].children.push(b);
        let unlocked = ce::part_unlocked(run, info);
        let icon = windows.len();
        if unlocked {
            windows.push(picture(icon, b, [0.0, 0.0, iw, ih], palette.icon(item.part), 0xFFFF_FFFF));
        } else {
            // The shield, centred in the item window at its authored size.
            let (sx, sy) = (((iw - UNLOCKABLE_SHIELD_SIZE[0]) * 0.5).round(), ((ih - UNLOCKABLE_SHIELD_SIZE[1]) * 0.5).round());
            windows.push(plain(
                icon,
                Some(b),
                [sx, sy, sx + UNLOCKABLE_SHIELD_SIZE[0], sy + UNLOCKABLE_SHIELD_SIZE[1]],
                Some(Drawable::Image(ImageDrawable {
                    image: Some(crate::spui::model::ImageRef {
                        file: UNLOCKABLE_SHIELD,
                        uv: UNLOCKABLE_SHIELD_UV,
                        dims: Some([UNLOCKABLE_SHIELD_SIZE[0] as u32, UNLOCKABLE_SHIELD_SIZE[1] as u32]),
                    }),
                    ..Default::default()
                })),
                0xFFFF_FFFF,
            ));
        }
        // Children draw front to back: the icon above the burst.
        windows[b].children.push(icon);
        if unlocked && new_parts.contains(&item.part) {
            let burst = windows.len();
            let (bx, by) = (iw * (1.0 - NEW_ITEM_BURST_SCALE) * 0.5, ih * (1.0 - NEW_ITEM_BURST_SCALE) * 0.5);
            windows.push(picture(burst, b, [bx, by, iw - bx, ih - by], NEW_ITEM_BURST, 0xFFFF_FFFF));
            windows[b].children.push(burst);
        }
        items.push((b, item.part));
    }
    (layout_of(windows), items)
}

/// The category tabs in the strip (`72DF4CEE`): one 50 px button per category
/// from the first visible one.
pub(crate) fn tab_layout(palette: &crate::creature_editor_data::Palette, strip: [f32; 4], first: usize) -> (Layout, Vec<(WinId, WinId, usize)>, usize) {
    let (w, h) = (strip[2] - strip[0], strip[3] - strip[1]);
    let fit = ((w / TAB_WIDTH).floor() as usize).max(1);
    let mut windows = vec![plain(0, None, strip, None, 0xFFFF_FFFF)];
    windows[0].flags = flag::VISIBLE | flag::ENABLED;
    let mut tabs = Vec::new();
    for (slot, (ci, c)) in palette.categories.iter().enumerate().skip(first).take(fit).enumerate() {
        let x = slot as f32 * TAB_WIDTH;
        // The strip window 72DF4CEE is 51 px high and a tab button 52, so the buttons sit at the
        // strip's top whichever way they are aligned.
        let rect = [x, (h - 52.0).max(0.0), x + TAB_WIDTH, h.max(52.0)];
        let b = windows.len();
        windows.push(button(b, 0, rect, ci as u32 + 1));
        windows[0].children.push(b);
        let icon = windows.len();
        let file = c.icon(false, false, false, false).unwrap_or(SHIELD);
        windows.push(picture(icon, b, [0.0, 0.0, TAB_WIDTH, 52.0], file, 0xFFFF_FFFF));
        windows[b].children.push(icon);
        tabs.push((b, icon, ci));
    }
    (layout_of(windows), tabs, fit)
}

fn rect_of(ui: &SpuiState, control: u32) -> Option<[f32; 4]> {
    let w = ui.find(control)?;
    let r = ui.rect(w);
    Some([r.x1, r.y1, r.x2, r.y2])
}

fn place_root(ui: &mut SpuiState, x: f32, y: f32) {
    let Some(&root) = ui.layout.roots.first() else {
        return;
    };
    let a = ui.layout.window(root).area;
    ui.set_area(root, [x, y, x + a[2] - a[0], y + a[3] - a[1]]);
}

fn place_root_grow(ui: &mut SpuiState, x: f32, y: f32, extra: f32) {
    let Some(&root) = ui.layout.roots.first() else {
        return;
    };
    let a = ui.layout.window(root).area;
    ui.set_area(root, [x, y, x + a[2] - a[0], y + a[3] - a[1] + extra]);
}

/// Control id of the speech-bubble frame of `RolloverEditorMessage` (window `125F2C4F`).
const MESSAGE_BUBBLE: u32 = 0x125F_2C4F;
/// Gap between the advisor icon and the message text, and the padding right of the text.
const MESSAGE_GAP: f32 = 6.0;
const MESSAGE_PAD: f32 = 10.0;

/// Place the one-line editor message (`RolloverEditorMessage`, layout 0x40464100) with its top left
/// corner at `x, y`, with the text clear of the advisor icon and the bubble wrapped round the text.
///
/// The layout is authored as a 36 x 46 bubble holding the 23 x 31 icon with both text windows
/// (`70524F6B` and its shadow `B25F2DF9`) starting at x 419 / 421, i.e. over the icon: the retail
/// code that shows a message (`cEditor` side of 0x5BEE40, which keeps the bubble, text and shadow
/// windows at +0x84 / +0x88 / +0x8C) is not located. ADAPTED: the text starts after the icon
/// ([`MESSAGE_GAP`]) and the bubble and root grow to the text extent measured last frame.
pub fn place_message(ui: &mut SpuiState, x: f32, y: f32) {
    use crate::cell_editor_ui::id::{MESSAGE_SHADOW, MESSAGE_TEXT};
    let Some(&root) = ui.layout.roots.first() else {
        return;
    };
    let a = ui.layout.window(root).area;
    let (Some(text), Some(shadow), Some(bubble)) =
        (ui.find(MESSAGE_TEXT), ui.find(MESSAGE_SHADOW), ui.find(MESSAGE_BUBBLE))
    else {
        place_root(ui, x, y);
        return;
    };
    let (t0, s0, b0) = (ui.layout.window(text).area, ui.layout.window(shadow).area, ui.layout.window(bubble).area);
    // The icon: the narrow window among the root's children that is neither the bubble nor the
    // texts (authored 23 px wide; areas are relative to the root).
    let icon_right = ui
        .layout
        .window(root)
        .children
        .iter()
        .copied()
        .filter(|&c| c != text && c != shadow && c != bubble)
        .map(|c| ui.layout.window(c).area)
        .filter(|r| r[2] - r[0] < 30.0)
        .map(|r| r[2])
        .fold(f32::NEG_INFINITY, f32::max);
    let icon_right = if icon_right.is_finite() { icon_right } else { t0[0] };
    let tx = icon_right + MESSAGE_GAP;
    let extent = ui.text_extent(text).map_or(0.0, |e| e.x.ceil());
    // A wide window so the text never wraps (the measured width is the whole line).
    let wide = 900.0;
    ui.set_area(text, [tx, t0[1], tx + wide, t0[3]]);
    ui.set_area(shadow, [tx + (s0[0] - t0[0]), s0[1], tx + (s0[0] - t0[0]) + wide, s0[3]]);
    // (Child areas are relative to the root; its own area is in screen pixels.)
    let right = (tx + extent + MESSAGE_PAD).max(a[2] - a[0]);
    ui.set_area(bubble, [b0[0], b0[1], right, b0[3]]);
    ui.set_area(root, [x, y, x + right, y + a[3] - a[1]]);
}

fn fit_root(ui: &mut SpuiState, rect: [f32; 4]) {
    if let Some(&root) = ui.layout.roots.first() {
        ui.set_area(root, rect);
    }
}

fn show_root(ui: &mut SpuiState, on: bool) {
    if let Some(&root) = ui.layout.roots.first() {
        if ui.visible(root) != on {
            ui.set_visible(root, on);
        }
    }
}

fn root_size(ui: &SpuiState) -> [f32; 2] {
    ui.layout
        .roots
        .first()
        .map(|&r| {
            let a = ui.layout.window(r).area;
            [a[2] - a[0], a[3] - a[1]]
        })
        .unwrap_or([0.0, 0.0])
}

fn set_visible_all(ui: &mut SpuiState, control: u32, on: bool) {
    for w in ui.find_all(control) {
        if ui.visible(w) != on {
            ui.set_visible(w, on);
        }
    }
}

const LINE_HEIGHT: f32 = 29.0;
const ERROR_HEIGHT: f32 = 22.0;

/// Spawn the editor layouts when a session starts, despawn them after.
pub fn manage(
    mut commands: Commands,
    editor: Res<CreatureEditor>,
    originals: Res<Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &EditorRoot)>,
    mut layout: ResMut<CreatureEditorUiLayout>,
    mut editor_state: ResMut<EditorState>,
) {
    let want = editor.active && !originals.resource_index.records.is_empty();
    if !want {
        if !roots.is_empty() {
            for (e, _) in &roots {
                commands.entity(e).despawn();
            }
            *layout = CreatureEditorUiLayout::default();
            editor_state.retail_layout = None;
        }
        return;
    }
    if roots.iter().any(|(_, r)| *r == EditorRoot::Frame) {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let mut spawn = |name: &str, kind: EditorRoot, policy: RootPolicy, z: i32, hidden: bool| {
        if let Some(l) = assets.layout(index, name) {
            let l = if hidden {
                let mut copy = (*l).clone();
                for &r in &copy.roots.clone() {
                    copy.windows[r].flags &= !flag::VISIBLE;
                }
                Arc::new(copy)
            } else {
                l
            };
            let root = view::spawn(&mut commands, l, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        } else {
            warn!("creature editor: layout {name} not installed");
        }
    };
    spawn("GlobalEditorUI", EditorRoot::Frame, RootPolicy::Fit, 100, false);
    spawn("EditorPartsPaletteCategory", EditorRoot::Category, RootPolicy::Fit, 97, false);
    spawn("EditorPartsPalette", EditorRoot::Palette, RootPolicy::Fit, 98, false);
    spawn("EditorPaintPaletteCategory", EditorRoot::PaintCategory, RootPolicy::Fit, 97, true);
    spawn("EditorPaintPalette", EditorRoot::PaintPalette, RootPolicy::Fit, 98, true);
    spawn("EditorBudget", EditorRoot::Budget, RootPolicy::Authored, 104, false);
    spawn("EditorNameDescribe-Tag", EditorRoot::Name, RootPolicy::Authored, 104, false);
    spawn("EditorComplexityMeter", EditorRoot::Complexity, RootPolicy::Authored, 104, false);
    spawn("EditorHeaderTray", EditorRoot::Header, RootPolicy::Authored, 104, false);
    spawn("EditorPageControls", EditorRoot::Pager, RootPolicy::Authored, 99, true);
    spawn("RolloverEditorMessage", EditorRoot::Message, RootPolicy::Authored, 120, false);
    // `PlayMode` (0x7518573E), parented to the frame's play-mode container.
    spawn("0x7518573E", EditorRoot::Play, RootPolicy::Fit, 101, true);
    *layout = CreatureEditorUiLayout::default();
}

/// Push the editor state into the layouts and (re)build the dynamic ones.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut commands: Commands,
    mut editor: ResMut<CreatureEditor>,
    mut editor_state: ResMut<EditorState>,
    data: Res<CreatureEditorData>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut layout: ResMut<CreatureEditorUiLayout>,
    camera: Query<Entity, With<crate::GameCamera>>,
    windows: Query<&bevy::window::Window>,
    mut roots: Query<(Entity, &mut SpuiState, &EditorRoot)>,
    (time, mut blink): (Res<Time>, Local<Option<u32>>),
) {
    let index = &originals.resource_index;
    let mode = editor.mode;
    let setup_kind = editor.kind;
    let build = mode == Mode::Build && !editor.ui_hidden;
    let paint = mode == Mode::Paint && !editor.ui_hidden;
    let play = mode == Mode::Play && !editor.ui_hidden;
    let kind = editor.kind;
    let Some(palette) = data.parts_palette(kind).cloned() else {
        return;
    };
    if editor.category >= palette.categories.len() {
        editor.category = 0;
    }
    let pages = palette.page_count(editor.category);
    if editor.page >= pages.max(1) {
        editor.page = pages.saturating_sub(1);
    }
    // Facts from the frame and the palette layouts.
    let mut frame_rects: HashMap<u32, [f32; 4]> = HashMap::new();
    let mut page_area = None;
    let mut pager_host = None;
    let mut panel = None;
    let mut strip = None;
    let mut screen = [0.0f32; 2];
    for (_, ui, kind) in &roots {
        if !ui.ready {
            continue;
        }
        match kind {
            EditorRoot::Frame => {
                screen = ui.screen;
                for c in [id::HOST_BUDGET, id::HOST_NAME, id::HOST_COMPLEXITY, id::HOST_VERBS] {
                    if let Some(r) = rect_of(&ui, c) {
                        frame_rects.insert(c, r);
                    }
                }
            }
            EditorRoot::Category => {
                page_area = rect_of(&ui, id::PAGE_AREA);
                pager_host = rect_of(&ui, page_id::PAGE_HOST);
                panel = rect_of(&ui, id::CATEGORY_PANEL);
            }
            EditorRoot::Palette => strip = rect_of(&ui, id::TABS),
            _ => {}
        }
    }
    // Screen regions for `editor::drag` (logical pixels).
    if let (Some(panel), Ok(window)) = (panel, windows.single()) {
        let k = 1.0 / window.scale_factor().max(0.1);
        let r = |a: [f32; 4]| Rect::new(a[0] * k, a[1] * k, a[2] * k, a[3] * k);
        let (w, h) = (screen[0], screen[1]);
        let top = strip.map_or(80.0, |s| s[3] + 8.0);
        let palette_rect = if build { r([0.0, 0.0, panel[2], panel[3]]) } else { Rect::default() };
        editor_state.retail_layout = Some(crate::editor::CreatorLayout::retail(
            palette_rect,
            r([w * 0.35, 0.0, w * 0.65, top.min(90.0)]),
            r([0.0, h - 100.0 * h / 600.0, w, h]),
            frame_rects.get(&id::HOST_VERBS).map(|v| r(*v)).unwrap_or_default(),
        ));
    }
    // Dynamic roots: tabs, page items, verb trays, rollover.
    let page = palette.page(editor.category, editor.page).cloned();
    let verbs = active_verbs(&data, kind, &run);
    let hover = editor.hover_item.filter(|_| !editor_state.dragging() && build);
    let rollover = hover.map(|p| (p, data.parts.get(&p).map_or(0, |i| i.abilities.len())));
    let key = BuiltKey {
        kind,
        category: editor.category,
        page: editor.page,
        tab_scroll: editor.tab_scroll,
        items: page
            .as_ref()
            .map(|p| {
                p.items
                    .iter()
                    .filter_map(|i| Some((i.part, ce::part_unlocked(&run, data.parts.get(&i.part)?))))
                    .collect()
            })
            .unwrap_or_default(),
        verbs: verbs.clone(),
        rollover,
    };
    let Ok(camera) = camera.single() else {
        return;
    };
    if page_area.is_some() && strip.is_some() && layout.built.as_ref() != Some(&key) {
        let old = layout.built.clone();
        let changed_page = old.as_ref().is_none_or(|b| (b.kind, b.category, b.page, &b.items) != (key.kind, key.category, key.page, &key.items));
        let changed_tabs = old.as_ref().is_none_or(|b| (b.kind, b.tab_scroll) != (key.kind, key.tab_scroll));
        let changed_verbs = old.as_ref().is_none_or(|b| b.verbs != key.verbs);
        let changed_roll = old.as_ref().is_none_or(|b| b.rollover != key.rollover);
        for (e, _, kind) in &roots {
            let stale = match kind {
                EditorRoot::Page => changed_page,
                EditorRoot::Tabs => changed_tabs,
                EditorRoot::Tray(_) | EditorRoot::Verb(..) => changed_verbs,
                EditorRoot::Rollover | EditorRoot::RolloverLine(_) => changed_roll,
                _ => false,
            };
            if stale {
                commands.entity(e).despawn();
            }
        }
        if changed_page {
            layout.items.clear();
            if let (Some(page), Some(area)) = (page.as_ref(), page_area) {
                let (l, items) = page_layout(&data, &run, &palette, page, area, &editor.new_parts);
                let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(EditorRoot::Page);
                layout.items = items;
            }
        }
        if changed_tabs {
            if let Some(strip) = strip {
                let (l, tabs, fit) = tab_layout(&palette, strip, editor.tab_scroll);
                let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(EditorRoot::Tabs);
                layout.tabs = tabs;
                layout.visible_tabs = fit;
            }
        }
        if changed_verbs {
            for (t, tray) in verbs.iter().enumerate() {
                if tray.is_empty() {
                    continue;
                }
                if let Some(l) = assets.layout(index, "EditorVerbTray") {
                    let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 104);
                    commands.entity(root).insert(EditorRoot::Tray(t));
                }
                for v in 0..tray.len() {
                    if let Some(l) = assets.layout(index, "editorVerbIcon") {
                        let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 105);
                        commands.entity(root).insert(EditorRoot::Verb(t, v));
                    }
                }
            }
        }
        if changed_roll {
            if let Some((_, lines)) = rollover {
                if let Some(l) = assets.layout(index, "EditorPaletteItemRollover") {
                    let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 110);
                    commands.entity(root).insert(EditorRoot::Rollover);
                }
                for i in 0..lines {
                    if let Some(l) = assets.layout(index, "EditorPaletteRolloverAbilityLine") {
                        let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 111);
                        commands.entity(root).insert(EditorRoot::RolloverLine(i));
                    }
                }
            }
        }
        layout.built = Some(key);
    }
    // Hovered palette item.
    let mut hovered = None;
    let mut item_rect = None;
    for (_, ui, kind) in &roots {
        if *kind == EditorRoot::Page && ui.ready && build {
            for (w, part) in &layout.items {
                if ui.states[*w] & state::HOVER != 0 {
                    hovered = Some(*part);
                    let r = ui.rect(*w);
                    item_rect = Some([r.x1, r.y1, r.x2, r.y2]);
                }
            }
        }
    }
    editor.hover_item = hovered;
    let dna = run.editor_dna.max(0.0).round() as i64;
    let complexity = data.complexity(kind, &run.parts, run.body.spine.len());
    let diet = ce::diet(&data, &run);
    let setup = data.setup(kind).cloned().unwrap_or_default();
    let currency = setup.currency_char.unwrap_or(DNA_GLYPH);
    let verbs_host = frame_rects.get(&id::HOST_VERBS).copied();
    let mut tray_y: HashMap<usize, f32> = HashMap::new();
    if let Some(host) = verbs_host {
        // ADAPTED: rows of 36 px under the diet header (retail tray size).
        let mut y = host[1] + 44.0;
        for (t, tray) in verbs.iter().enumerate() {
            if !tray.is_empty() {
                tray_y.insert(t, y);
                y += 36.0;
            }
        }
    }
    let prompt = setup.name_prompt.clone().unwrap_or_else(|| "Name Your Creature".into());
    // The name panel shows the edited model's name (`EditorNamePanel`): the
    // creature's, else the cell's (INFERRED: the cell resource converted by
    // FUN_00E83910 keeps its name; an outfitter edits that same creature),
    // the setup's prompt only for an unnamed creation.
    let shown_name = crate::front_end::slots::creature_name(&run);
    let name_text = if editor.naming {
        format!("{}_", run.creature_creation.name)
    } else if setup_kind.is_rigid() {
        if run.creature_creation.name.is_empty() { prompt } else { run.creature_creation.name.clone() }
    } else if shown_name.is_empty() {
        prompt
    } else {
        shown_name
    };
    let message = editor.message.as_ref().map(|(t, _)| t.clone());
    let ready_textures: Vec<ResourceKey> = layout
        .tabs
        .iter()
        .filter_map(|(_, _, ci)| palette.categories.get(*ci))
        .flat_map(|c| c.icons.iter().take(8).copied())
        .collect();
    for key in ready_textures {
        assets.texture(index, &mut images, key);
    }
    for (_, mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match *kind {
            EditorRoot::Frame => bind_frame(&mut ui, &editor, &editor_state),
            EditorRoot::Palette | EditorRoot::PaintPalette => {
                let on = if *kind == EditorRoot::Palette { build } else { paint };
                show_root(&mut ui, on);
                let more = palette.categories.len() > layout.visible_tabs.max(1);
                for (c, vis) in [
                    (id::TAB_LEFT, more && editor.tab_scroll > 0),
                    (id::TAB_RIGHT, more && editor.tab_scroll + layout.visible_tabs < palette.categories.len()),
                    (id::TAB_HIGHLIGHT, false),
                    (id::BAR_LEFT, false),
                    (id::BAR_TABS, false),
                    (id::BAR_FULL, true),
                ] {
                    if let Some(w) = ui.find(c) {
                        if *kind == EditorRoot::PaintPalette && matches!(c, id::TAB_LEFT | id::TAB_RIGHT) {
                            ui.set_visible(w, false);
                        } else {
                            ui.set_visible(w, vis);
                        }
                    }
                }
            }
            EditorRoot::Category | EditorRoot::PaintCategory => {
                let on = if *kind == EditorRoot::Category { build } else { paint };
                if let Some(w) = ui.find(id::CATEGORY_PANEL) {
                    if ui.visible(w) != on {
                        ui.set_visible(w, on);
                    }
                }
                if *kind == EditorRoot::PaintCategory {
                    show_root(&mut ui, paint);
                    // Designer fill of the colour-picker row (the pickers are
                    // not bound yet).
                    if let Some(w) = ui.find(page_id::SWATCH_AREA) {
                        ui.set_visible(w, false);
                    }
                }
                if let Some(w) = ui.find(id::CATEGORY_GLOW) {
                    ui.set_visible(w, false);
                }
            }
            EditorRoot::Tabs => {
                show_root(&mut ui, build);
                for (b, icon, ci) in &layout.tabs {
                    let st = ui.states[*b];
                    let Some(c) = palette.categories.get(*ci) else { continue };
                    let file = c.icon(
                        *ci == editor.category,
                        st & state::HOVER != 0,
                        st & state::CLICKED != 0,
                        false,
                    );
                    if let Some(file) = file {
                        ui.set_image(*icon, Some(image(file)));
                    }
                    ui.set_tooltip(*b, Some(c.name.clone()));
                }
            }
            EditorRoot::Page => show_root(&mut ui, build),
            EditorRoot::Pager => {
                show_root(&mut ui, build && pages > 1);
                if let Some(host) = pager_host {
                    fit_root(&mut ui, host);
                }
                if let Some(w) = ui.find(page_id::PAGE_TEXT) {
                    ui.set_text(w, format!("{}/{}", editor.page + 1, pages.max(1)));
                }
                if let Some(w) = ui.find(page_id::PAGE_PREV) {
                    ui.set_enabled(w, editor.page > 0);
                }
                if let Some(w) = ui.find(page_id::PAGE_NEXT) {
                    ui.set_enabled(w, editor.page + 1 < pages);
                }
            }
            EditorRoot::Budget => {
                show_root(&mut ui, build);
                if let Some(r) = frame_rects.get(&id::HOST_BUDGET) {
                    place_root(&mut ui, r[0], r[1]);
                }
                if let Some(w) = ui.find(id::BUDGET_TEXT) {
                    ui.set_text(w, format!("{currency}{dna}"));
                }
                if let (Some(w), Some(tip)) = (ui.layout.roots.first().copied(), setup.budget_tooltip.clone()) {
                    ui.set_tooltip(w, Some(tip));
                }
            }
            EditorRoot::Name => {
                show_root(&mut ui, !editor.ui_hidden);
                if let Some(r) = frame_rects.get(&id::HOST_NAME) {
                    place_root(&mut ui, r[0], r[1]);
                }
                if let Some(w) = ui.find(id::NAME_DESCRIBE_PANEL) {
                    ui.set_visible(w, false);
                }
                if let Some(w) = ui.find(id::NAME_TEXT) {
                    ui.set_text(w, name_text.clone());
                }
                if let Some(w) = ui.find(id::NAME_BUTTON) {
                    ui.set_selected(w, editor.naming);
                }
            }
            EditorRoot::Complexity => {
                show_root(&mut ui, build);
                if let Some(r) = frame_rects.get(&id::HOST_COMPLEXITY) {
                    place_root(&mut ui, r[0], r[1]);
                }
                // FUN_005A74D0: the fill keeps its authored left edge and
                // its width is the authored width x the ratio (0..1); the
                // animated icon takes the ratio as a percentage (FUN_005A72E0).
                let ratio = complexity.ratio().clamp(0.0, 1.0);
                if let Some(w) = ui.find(id::COMPLEXITY_FILL) {
                    let a = ui.layout.window(w).area;
                    ui.set_area(w, [a[0], a[1], a[0] + (a[2] - a[0]) * ratio, a[3]]);
                    ui.set_visible(w, ratio > 0.0);
                }
                if let Some(w) = ui.find(id::COMPLEXITY_ICON) {
                    ui.set_frame(w, Some((ratio * 11.0).round() as u32));
                    // FUN_005A7440 / FUN_005A72E0: while the palette item under
                    // the cursor is too complex the meter pulses: white, its
                    // alpha |t - 500| / 500 over a 1000 ms cycle; a cycle in
                    // progress ends before the plain look returns.
                    let too_complex = hovered
                        .is_some_and(|part| ce::refusal(&data, setup_kind, &run, &originals, part) == Some(Refusal::TooComplex));
                    let dt = (time.delta_secs() * 1000.0) as u32;
                    *blink = match *blink {
                        Some(t) if t + dt <= BLINK_MS => Some(t + dt),
                        Some(t) if too_complex => Some((t + dt) % BLINK_MS),
                        Some(_) => None,
                        None if too_complex => Some(0),
                        None => None,
                    };
                    let alpha = blink.map_or(1.0, |t| (t as f32 - BLINK_MS as f32 * 0.5).abs() / (BLINK_MS as f32 * 0.5));
                    ui.set_shade(w, ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24 | 0x00FF_FFFF);
                }
            }
            EditorRoot::Header => {
                // The verb trays sit in the parts palette root (`F006F308`):
                // Build mode only; the diet header is the creatures' (a
                // vehicle has no mouth).
                show_root(&mut ui, build && !setup_kind.is_rigid());
                if let Some(host) = verbs_host {
                    let s = root_size(&ui);
                    place_root(&mut ui, host[2] - s[0], host[1]);
                }
                let (text, icon) = match diet {
                    Diet::Herbivore => ("Herbivore", "stats_veg_icon"),
                    Diet::Carnivore => ("Carnivore", "stats_carni_icon"),
                    Diet::Omnivore => ("Omnivore", "stats_omni_icon"),
                    Diet::None => ("", "stats_no_mouth_icon"),
                };
                if let Some(w) = ui.find(id::HEADER_TEXT) {
                    ui.set_text(w, text);
                }
                if let Some(w) = ui.find(id::TRAY_ICON) {
                    let png = ResourceKey {
                        type_id: crate::creature_editor_data::PNG,
                        group_id: DIET_ICON_GROUP,
                        instance_id: name_id(icon),
                    };
                    let ok = index.records.contains_key(&png);
                    if ok {
                        assets.texture(index, &mut images, png);
                    }
                    ui.set_image(w, ok.then(|| image(png)));
                    ui.set_visible(w, ok);
                }
            }
            EditorRoot::Tray(t) => {
                show_root(&mut ui, build);
                if let (Some(host), Some(y)) = (verbs_host, tray_y.get(&t)) {
                    let s = root_size(&ui);
                    place_root(&mut ui, host[2] - s[0], *y);
                }
                if let Some(w) = ui.find(id::TRAY_VALUE) {
                    ui.set_text(w, "");
                }
                if let Some(w) = ui.find(id::TRAY_ICON) {
                    // The tray's round icon (`verbTrayIconID`, 39 x 35 in the editor verb icon group; the
                    // vehicle trays' ids are in the vehicle group or not installed: hidden then).
                    let png = data.tray_icons_of(setup_kind).get(t).copied().flatten().and_then(|instance_id| {
                        [EDITOR_VERB_GROUP, crate::rigid_stats::VEHICLE_ICON_GROUP]
                            .into_iter()
                            .map(|group_id| ResourceKey { type_id: crate::creature_editor_data::PNG, group_id, instance_id })
                            .find(|k| index.records.contains_key(k))
                    });
                    if let Some(png) = png {
                        assets.texture(index, &mut images, png);
                    }
                    ui.set_image(w, png.map(image));
                    ui.set_visible(w, png.is_some());
                }
            }
            EditorRoot::Verb(t, v) => {
                show_root(&mut ui, build);
                let Some((verb, level)) = verbs.get(t).and_then(|tr| tr.get(v)).copied() else {
                    continue;
                };
                if let (Some(host), Some(y)) = (verbs_host, tray_y.get(&t)) {
                    // ADAPTED: icons right to left from the tray's right end.
                    let count = verbs[t].len() as f32;
                    let x = host[2] - 40.0 - (count - v as f32) * 28.0;
                    place_root(&mut ui, x, *y - 2.0);
                }
                if let (Some(w), Some((png, caption))) = (ui.find(id::VERB_BUTTON), verb_art(verb)) {
                    if index.records.contains_key(&png) {
                        assets.texture(index, &mut images, png);
                        ui.set_image(w, Some(image(png)));
                    }
                    ui.set_tooltip(w, Some(caption.to_string()));
                }
                if let Some(w) = ui.find(id::VERB_VALUE) {
                    ui.set_text(w, level.to_string());
                }
            }
            EditorRoot::Rollover => {
                let Some(part) = hover else { continue };
                let Some(info) = data.parts.get(&part) else { continue };
                let reason = ce::refusal(&data, setup_kind, &run, &originals, part).map(|r| refusal_text(&data, r));
                if let Some(r) = item_rect {
                    let extra = info.abilities.len() as f32 * LINE_HEIGHT + if reason.is_some() { ERROR_HEIGHT } else { 0.0 };
                    place_root_grow(&mut ui, r[2] + 4.0, r[1], extra);
                }
                if let Some(w) = ui.find(id::ROLL_NAME) {
                    let name = if ce::part_unlocked(&run, info) { info.name.clone() } else { "?".to_string() };
                    ui.set_text(w, name);
                }
                if let Some(w) = ui.find(id::ROLL_PRICE) {
                    ui.set_text(w, format!("{currency}{}", info.price));
                }
                if let Some(w) = ui.find(id::ROLL_ERROR_ROW) {
                    ui.set_visible(w, reason.is_some());
                }
                if let Some(w) = ui.find(id::ROLL_ERROR) {
                    ui.set_text(w, reason.unwrap_or_default());
                }
            }
            EditorRoot::RolloverLine(i) => {
                let Some(part) = hover else { continue };
                let Some(info) = data.parts.get(&part) else { continue };
                let Some((verb, level)) = info.abilities.get(i).copied() else { continue };
                if let Some(r) = item_rect {
                    let refused = ce::refusal(&data, setup_kind, &run, &originals, part).is_some();
                    let top = 40.0 + if refused { ERROR_HEIGHT } else { 0.0 };
                    place_root(&mut ui, r[2] + 8.0, r[1] + top + i as f32 * LINE_HEIGHT);
                }
                let art = verb_art(verb);
                if let Some(w) = ui.find(id::LINE_NAME) {
                    ui.set_text(w, art.map(|a| a.1).unwrap_or(verb).to_string());
                }
                if let Some(w) = ui.find(id::LINE_VALUE) {
                    ui.set_text(w, format!("+{level}"));
                }
                if let (Some(w), Some((png, _))) = (ui.find(id::LINE_ICON), art) {
                    if index.records.contains_key(&png) {
                        assets.texture(index, &mut images, png);
                        ui.set_image(w, Some(image(png)));
                    }
                }
            }
            EditorRoot::Message => {
                let Some(&root) = ui.layout.roots.first() else { continue };
                ui.set_visible(root, message.is_some());
                if let Some(r) = frame_rects.get(&id::HOST_NAME) {
                    place_message(&mut ui, r[0], r[1] - 48.0);
                }
                for c in [id::MESSAGE_TEXT, id::MESSAGE_SHADOW] {
                    for w in ui.find_all(c) {
                        ui.set_visible(w, message.is_some());
                        ui.set_text(w, message.clone().unwrap_or_default());
                    }
                }
            }
            EditorRoot::Play => {
                // The vehicles' Test Drive has no animation / backdrop panels.
                show_root(&mut ui, play && !editor.kind.is_rigid());
                set_visible_all(&mut ui, id::PLAY_PANELS, play);
                set_visible_all(&mut ui, id::PLAY_ANIMS_PANEL, play && editor.play.anims_panel);
                set_visible_all(&mut ui, id::PLAY_BACKGROUNDS_PANEL, play && !editor.play.anims_panel);
                for c in id::PLAY_HIDDEN {
                    set_visible_all(&mut ui, c, false);
                }
                for c in [id::PLAY_ANIMATE, id::PLAY_BACKDROPS] {
                    if let Some(w) = ui.find(c) {
                        let on = (c == id::PLAY_ANIMATE) == editor.play.anims_panel;
                        ui.set_selected(w, on);
                    }
                }
                for (slot, c) in BACKGROUND_BUTTONS.iter().enumerate() {
                    let n = data.backgrounds_of(setup_kind).len();
                    set_visible_all(&mut ui, *c, slot < n);
                    if let Some(w) = ui.find(*c) {
                        ui.set_selected(w, slot == editor.play.background);
                    }
                }
            }
        }
    }
}

/// Abilities per verb tray: (verb, level) for each verb category the
/// creature has.
fn active_verbs(data: &CreatureEditorData, kind: crate::creature_editor_data::SetupKind, run: &RunProgress) -> Vec<Vec<(&'static str, i32)>> {
    if kind.is_rigid() {
        // The vehicle stats trays (capability totals, `rigid_stats`).
        return crate::rigid_stats::tray_levels(data, kind, run);
    }
    let levels = ce::ability_levels(data, run);
    data.trays_of(kind)
        .iter()
        .map(|tray| {
            tray.iter()
                .filter_map(|category| {
                    let verb = ABILITY_CAPABILITIES.iter().find(|(v, _)| name_id(v) == *category)?.0;
                    levels.iter().find(|(v, _)| *v == verb).map(|(v, l)| (*v, *l))
                })
                .collect()
        })
        .collect()
}

/// Cycle of the complexity meter's pulse (`DAT_0150F9D4` = 1000 ms).
const BLINK_MS: u32 = 1000;

fn refusal_text(data: &CreatureEditorData, r: Refusal) -> String {
    // The budget and complexity reasons are the retail `editor` locale
    // strings FUN_0059E580 sets (0x201 / 0x202); the others ADAPTED.
    match r {
        Refusal::NoDna => data.reason_budget.clone().unwrap_or_else(|| "Not enough DNA".into()),
        Refusal::TooComplex => data.reason_complexity.clone().unwrap_or_else(|| "The creation is too complex".into()),
        Refusal::Locked => "Find this part first".into(),
        Refusal::Unavailable => "Not available yet".into(),
    }
}

fn bind_frame(ui: &mut SpuiState, editor: &CreatureEditor, state: &EditorState) {
    // The root's fill (a translucent purple) is a designer backdrop: the retail frames show the
    // scene untinted (as in the Cell Creator).
    if let Some(&root) = ui.layout.roots.first() {
        ui.set_tint(root, 0x00FF_FFFF);
        let on = !editor.ui_hidden;
        if ui.visible(root) != on {
            ui.set_visible(root, on);
        }
    }
    // In-game editor: no New / Save / Share (Galaxy-screen editor only).
    for c in id::RIGHT_CLUSTER_OTHERS {
        set_visible_all(ui, c, false);
    }
    let play = editor.mode == Mode::Play;
    set_visible_all(ui, id::UNDO_REDO, !play);
    for (c, on) in [(id::UNDO, state.can_undo()), (id::REDO, state.can_redo())] {
        if let Some(w) = ui.find(c) {
            ui.set_visible(w, !play);
            ui.set_enabled(w, on);
        }
    }
    for (c, m) in [(id::MODE_BUILD, Mode::Build), (id::MODE_PAINT, Mode::Paint), (id::MODE_PLAY, Mode::Play)] {
        if let Some(w) = ui.find(c) {
            // The rigid-block editors keep the retail buttons; Test Drive
            // is off but for vehicles (`creature_editor::set_mode`). The
            // outfitters paint the creature and its accessories (`outfitter`).
            ui.set_visible(w, true);
            if editor.kind.is_rigid() && m == Mode::Play && !editor.kind.is_vehicle() {
                ui.set_enabled(w, false);
            }
            ui.set_selected(w, editor.mode == m);
        }
    }
    for c in [id::CANCEL, id::ACCEPT] {
        if let Some(w) = ui.find(c) {
            ui.set_visible(w, true);
            ui.set_enabled(w, true);
        }
    }
    set_visible_all(ui, id::DEV_UI, false);
    set_visible_all(ui, id::OPTIONS_PANEL, editor.options_open);
    // In game the options panel cannot leave Spore from the editor.
    for c in [id::OPTIONS_EXIT, id::OPTIONS_QUIT] {
        if let Some(w) = ui.find(c) {
            ui.set_enabled(w, false);
        }
    }
}

/// Buttons of the chrome, palette presses and the name field.
#[allow(clippy::too_many_arguments)]
pub fn actions(
    mut editor: ResMut<CreatureEditor>,
    mut editor_state: ResMut<EditorState>,
    data: Res<CreatureEditorData>,
    originals: Res<Originals>,
    mut run: ResMut<RunProgress>,
    mouse: Res<ButtonInput<MouseButton>>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    mut typed: MessageReader<KeyboardInput>,
    layout: Res<CreatureEditorUiLayout>,
    mut roots: Query<(&mut SpuiState, &EditorRoot)>,
) {
    let mut clicked = Vec::new();
    for (mut ui, kind) in &mut roots {
        for w in ui.take_clicks() {
            clicked.push((*kind, ui.layout.window(w).control_id));
        }
    }
    let kind = editor.kind;
    let categories = data.parts_palette(kind).map_or(0, |p| p.categories.len());
    for (root, control) in clicked {
        match (root, control) {
            (EditorRoot::Frame, id::UNDO) => editor_state.queued.push(Action::Undo),
            (EditorRoot::Frame, id::REDO) => editor_state.queued.push(Action::Redo),
            (EditorRoot::Frame, id::CANCEL) => {
                ce::finish(&mut editor, &mut editor_state, &data, &mut run, false);
                return;
            }
            (EditorRoot::Frame, id::ACCEPT) => {
                if ce::finish(&mut editor, &mut editor_state, &data, &mut run, true) {
                    return;
                }
            }
            (EditorRoot::Frame, id::MODE_BUILD) => ce::set_mode(&mut editor, &editor_state, &data, &originals, Mode::Build),
            (EditorRoot::Frame, id::MODE_PAINT) => ce::set_mode(&mut editor, &editor_state, &data, &originals, Mode::Paint),
            (EditorRoot::Frame, id::MODE_PLAY) => ce::set_mode(&mut editor, &editor_state, &data, &originals, Mode::Play),
            (EditorRoot::Frame, id::OPTIONS_TOGGLE) => editor.options_open = !editor.options_open,
            (EditorRoot::Frame, id::OPTIONS_RESUME) => editor.options_open = false,
            (EditorRoot::Frame, id::OPTIONS_GUIDE | id::OPTIONS_SETTINGS | id::SPOREPEDIA) => {
                // ADAPTED: Spore Guide, settings and the Sporepedia are not
                // reachable from the in-game editor here yet.
                editor.say("Not available in the creator yet");
            }
            (EditorRoot::Tabs, c) if c >= 1 && (c as usize) <= categories => {
                editor.category = c as usize - 1;
                editor.page = 0;
            }
            (EditorRoot::Palette, id::TAB_LEFT) => editor.tab_scroll = editor.tab_scroll.saturating_sub(1),
            (EditorRoot::Palette, id::TAB_RIGHT) => {
                if editor.tab_scroll + layout.visible_tabs < categories {
                    editor.tab_scroll += 1;
                }
            }
            (EditorRoot::Pager, page_id::PAGE_PREV) => editor.page = editor.page.saturating_sub(1),
            (EditorRoot::Pager, page_id::PAGE_NEXT) => {
                let pages = data.parts_palette(kind).map_or(1, |p| p.page_count(editor.category));
                editor.page = (editor.page + 1).min(pages.saturating_sub(1));
            }
            (EditorRoot::Name, id::NAME_BUTTON) => editor.naming = true,
            (EditorRoot::Play, id::PLAY_ANIMATE) => editor.play.anims_panel = true,
            (EditorRoot::Play, id::PLAY_BACKDROPS) => editor.play.anims_panel = false,
            (EditorRoot::Play, c) => {
                if let Some(slot) = BACKGROUND_BUTTONS.iter().position(|b| *b == c) {
                    ce::choose_background(&mut editor, &data, &originals, slot);
                } else if PLAY_ANIM_BUTTONS.iter().any(|b| b.control == c) {
                    ce::press_play_button(&mut editor, &originals, c);
                }
            }
            _ => {}
        }
    }
    // Naming: typed characters go to the name field.
    if editor.naming {
        for k in typed.read() {
            if k.state != ButtonState::Pressed {
                continue;
            }
            match &k.logical_key {
                Key::Enter | Key::Escape => editor.naming = false,
                Key::Backspace => {
                    run.creature_creation.name.pop();
                }
                Key::Character(c) => {
                    if run.creature_creation.name.chars().count() < 32 {
                        run.creature_creation.name.extend(c.chars().filter(|c| !c.is_control()));
                    }
                }
                Key::Space => run.creature_creation.name.push(' '),
                _ => {}
            }
        }
        if mouse.just_pressed(MouseButton::Left) && !pointer.over_ui {
            editor.naming = false;
        }
        return;
    }
    typed.clear();
    // A press on a palette item buys the part; the held press carries it onto
    // the body (`editor::drag`).
    if mouse.just_pressed(MouseButton::Left) && editor.mode == Mode::Build && !editor_state.dragging() {
        if let Some(part) = editor.hover_item {
            match ce::refusal(&data, kind, &run, &originals, part) {
                None => editor_state.queued.push(Action::Buy(part)),
                Some(r) => editor.say(refusal_text(&data, r)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Palette items follow `GetItemArea` (item = `palettePageItemPercentageWidth` x page width, capped at
    /// `palettePageItemMaxPercentageHeight` x page height) and a part not found yet shows the 27 x 35 shield
    /// centred in its window, a newly found one the burst behind its icon.
    #[test]
    fn palette_items_follow_the_retail_item_area_and_locked_parts_show_the_shield() {
        use crate::creature_editor_data::{CreaturePartInfo, Palette, PaletteItem};
        let mut data = CreatureEditorData::default();
        for (id, found) in [(5u32, true), (6, false)] {
            let mut info = CreaturePartInfo::test(1);
            info.key.instance_id = id;
            info.starts_unlocked = found;
            data.parts.insert(id, info);
        }
        let item = |part, row, column| PaletteItem { part, group: 0, row, column };
        let page = |fraction: f32| PalettePage {
            rows: 10,
            columns: 4,
            item_width_fraction: 0.25,
            item_max_height_fraction: fraction,
            items: vec![item(5, 0, 0), item(6, 0, 1)],
            ..Default::default()
        };
        let mut run = RunProgress::default();
        run.parts.clear();
        let area = [0.0, 0.0, 448.0, 710.0];
        let size = |l: &Layout, b: usize| {
            let a = l.windows[b].area;
            (a[2] - a[0], a[3] - a[1])
        };
        // `TemplatePalettePartsPage`: 0.25 of 448 = 112 wide, capped at 0.1 x 710 = 71 high (aspect 1).
        let (layout, items) = page_layout(&data, &run, &Palette::default(), &page(0.1), area, &[5]);
        assert_eq!(items.len(), 2);
        let (found, locked) = (items[0].0, items[1].0);
        for b in [found, locked] {
            let (w, h) = size(&layout, b);
            // (the corners are snapped to whole pixels)
            assert!((w - 71.0).abs() <= 1.0 && (h - 71.0).abs() <= 1.0, "{w} x {h}");
        }
        // The locked part's only child is the shield at its authored size; the found part has its icon and the burst.
        let shield = layout.windows[locked].children[0];
        let a = layout.windows[shield].area;
        assert_eq!((a[2] - a[0], a[3] - a[1]), (27.0, 35.0));
        assert_eq!(layout.windows[locked].children.len(), 1);
        assert_eq!(layout.windows[found].children.len(), 2, "icon in front of the burst");
        // A page authoring 0.12 (`ce_page_weapon`) grows to 0.12 x 710 = 85 px (the old layout clamped to the cell).
        let (layout, items) = page_layout(&data, &run, &Palette::default(), &page(0.12), area, &[]);
        let (w, h) = size(&layout, items[0].0);
        assert!((w - 85.2).abs() <= 1.0 && (h - 85.2).abs() <= 1.0, "{w} x {h}");
        assert_eq!(layout.windows[items[0].0].children.len(), 1, "no burst for an item that is not new");
    }

    /// The one-line message keeps its text clear of the advisor icon and the bubble covers the
    /// icon (the installed layout authors the text over the icon, see [`place_message`]).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_editor_message_text_clears_its_icon() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let layout = SpuiAssets::default().layout(&index, "RolloverEditorMessage").expect("RolloverEditorMessage layout");
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        let text = ui.find(id::MESSAGE_TEXT).expect("text window");
        let shadow = ui.find(id::MESSAGE_SHADOW).expect("shadow window");
        let bubble = ui.find(MESSAGE_BUBBLE).expect("bubble window");
        let root = ui.layout.roots[0];
        let authored_text = ui.layout.window(text).area;
        let icon = ui
            .layout
            .window(root)
            .children
            .iter()
            .map(|&c| ui.layout.window(c).area)
            .find(|a| a[2] - a[0] < 30.0)
            .expect("the advisor icon");
        // The authored text starts over the icon.
        assert!(authored_text[0] < icon[2], "{authored_text:?} vs {icon:?}");
        place_message(&mut ui, 100.0, 200.0);
        assert!(ui.area(text)[0] >= icon[2] + MESSAGE_GAP - 0.01, "{:?}", ui.area(text));
        assert!(ui.area(shadow)[0] > ui.area(text)[0], "the shadow stays offset");
        // The bubble and the root still cover the icon and sit where they were placed.
        assert!(ui.area(bubble)[2] >= icon[2], "{:?}", ui.area(bubble));
        assert_eq!(&ui.area(root)[..2], &[100.0, 200.0]);
    }
}
