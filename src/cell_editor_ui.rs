//! The Cell Creator chrome: the retail editor layouts drawn through the SPUI
//! engine and bound to the editor state.
//!
//! * `GlobalEditorUI`: the frame (bottom bar, mode buttons, undo/redo,
//!   Cancel/Save, host windows of the sub layouts: `908891A7` budget,
//!   `272EB68E` name, `F383C97D` complexity meter, `722F9A6B` verb trays);
//! * `EditorPartsPalette` + `EditorPartsPaletteCategory`: the palette panel
//!   (page area `05D1754B`);
//! * the page items themselves: the retail `PaletteUI` creates one window per
//!   part at run time; here they are a layout built from the palette data
//!   (part icon, the `unavailable-item` shield for undiscovered parts, the
//!   `cgm_icon_sunburst_01` burst for new ones);
//! * `EditorBudget` (DNA), `EditorNameDescribe-NoTag` (name field),
//!   `EditorComplexityMeter`, `EditorHeaderTray` (diet), `EditorVerbTray` +
//!   `editorVerbIcon` (abilities), `EditorPaletteItemRollover` +
//!   `EditorPaletteRolloverAbilityLine` (part tooltip), `RolloverEditorMessage`;
//! * Paint mode (`cell_editor_paint`): `EditorPaintPalette` (top bar), the Cell
//!   paint category layout `0x3FCA51EA` (panel), a colour row and a page of
//!   complete styles built from the palette data, `EditorPageControls` (the
//!   "1/2" pager); the Build-only roots are hidden meanwhile;
//! * the camera hint: the `cHintManager` hint bar (`hint` layout) bound by
//!   `cell_hud::dialogs::bind_hint` and faded by `HintFade`.
//!
//! * The tray icons (cell-icons): the round icon of a tray and the header's diet disc are the
//!   images the game looks up by id in the `VerbIcons` layout (`0x4592B0`: the window with that
//!   control id; the ringed discs of the atlas `31A44893!E0233C2C / 2D`), and each verb loads
//!   its own icon layout (`verbIconLayout`: the ability tile `EditorAbilityVerbIconUI`, the
//!   combat tile, the speed and game speed icons) instead of one shared `editorVerbIcon`; see
//!   [`verb_picture`], [`VERB_ICON_LAYOUT`] and the Header / Tray / Verb arms of [`bind`].
//!
//! Positions of the sub layouts come from their host windows in the frame,
//! like the retail `EditorUI` inserts them. Where the retail code's sizing is
//! not documented the binding is marked ADAPTED.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use crate::{
    cell_editor::{self, CellEditor, Diet, Mode, Refusal},
    cell_editor_data::{CellEditorData, PalettePage, GAME_PALETTE, PNG},
    cell_editor_paint::{self, id as paint_id},
    dbpf::ResourceKey,
    editor::EditorState,
    game::RunProgress,
    spui::{
        ids::{class, flag, state},
        model::{ButtonData, Drawable, Extra, ImageDrawable, ImageRef, Layout, WinId, WinProc, Window, WindowKind},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    Originals,
};

pub mod id {
    // GlobalEditorUI
    pub const HOST_BUDGET: u32 = 0x9088_91A7;
    pub const HOST_NAME: u32 = 0x272E_B68E;
    pub const HOST_COMPLEXITY: u32 = 0xF383_C97D;
    pub const HOST_VERBS: u32 = 0x722F_9A6B;
    pub const MODE_BUILD: u32 = 0xF019_C2E7;
    pub const MODE_PAINT: u32 = 0xF019_C2F3;
    pub const MODE_PLAY: u32 = 0x7021_8642;
    pub const UNDO: u32 = 0xB006_EF6E;
    pub const REDO: u32 = 0xF006_EFA5;
    pub const CANCEL: u32 = 0x9043_9D7C;
    pub const ACCEPT: u32 = 0x058A_3350;
    /// Other buttons of the right cluster (Galaxy-screen editor only: save,
    /// new, publish...).
    pub const RIGHT_CLUSTER_OTHERS: [u32; 4] = [0x0476_6FF0, 0x05B6_E484, 0x0612_EFEA, 0x0612_EFEB];
    // EditorPartsPalette
    pub const TAB_LEFT: u32 = 0x05AE_C4B8;
    pub const TAB_RIGHT: u32 = 0x05AE_C4B9;
    pub const TAB_HIGHLIGHT: u32 = 0x49AF_E6A1;
    pub const TABS: u32 = 0x72DF_4CEE;
    // EditorPartsPaletteCategory
    pub const CATEGORY_PANEL: u32 = 0x52DF_67AF;
    pub const CATEGORY_GLOW: u32 = 0x065B_ADEF;
    // EditorPartsPalette top bar: pieces around the tab strip and the full
    // width bar shown when there are no tabs.
    pub const BAR_LEFT: u32 = 0xBA83_C461;
    pub const BAR_TABS: u32 = 0x90D4_AADC;
    pub const BAR_FULL: u32 = 0x05AE_FF7F;
    pub const PAGE_AREA: u32 = 0x05D1_754B;
    // EditorBudget
    pub const BUDGET_TEXT: u32 = 0x041E_98C3;
    // EditorNameDescribe-NoTag
    pub const NAME_BUTTON: u32 = 0x272E_B68E;
    pub const NAME_TEXT: u32 = 0xD0E6_D04B;
    pub const NAME_DESCRIBE_PANEL: u32 = 0x6A53_636C;
    // EditorComplexityMeter
    pub const COMPLEXITY_ICON: u32 = 0x05BA_D010;
    pub const COMPLEXITY_FILL: u32 = 0x530C_12D0;
    // EditorHeaderTray / EditorVerbTray / editorVerbIcon
    pub const TRAY_ICON: u32 = 0x04AB_090B;
    pub const HEADER_TEXT: u32 = 0x062D_6F99;
    pub const TRAY_VALUE: u32 = 0x04A6_1762;
    pub const VERB_BUTTON: u32 = 0x0497_6E19;
    /// The button of the Cell verbs' own icon layouts (`BED39F24`, `05722A76`, `12F13857`, `7ED81048`, `CDA510D7`).
    pub const VERB_TILE_BUTTON: u32 = 0x0628_3C92;
    pub const VERB_VALUE: u32 = 0x04A4_FF55;
    // EditorPaletteItemRollover
    pub const ROLL_NAME: u32 = 0x7052_4F6B;
    pub const ROLL_PRICE: u32 = 0x0232_88A3;
    pub const ROLL_ERROR_ROW: u32 = 0x064A_8A83;
    pub const ROLL_ERROR: u32 = 0x0440_B6B8;
    // EditorPaletteRolloverAbilityLine
    pub const LINE_NAME: u32 = 0x04C0_1BC4;
    pub const LINE_VALUE: u32 = 0x04C0_1BCA;
    pub const LINE_ICON: u32 = 0x04C0_1BBE;
    // RolloverEditorMessage
    pub const MESSAGE_TEXT: u32 = 0x7052_4F6B;
    pub const MESSAGE_SHADOW: u32 = 0xB25F_2DF9;
}

/// DNA glyph of `SporeCurrencyEA` (`editorCurrencyChar` 0x268B).
const DNA_GLYPH: char = '\u{268B}';
/// The shield of a part that is not unlocked yet: the background of the item's
/// `itemUnlockableLayout` (property 0x065F9E79 of `RigblockTemplate`,
/// 0x40606000!BA1942B6 -> layout `UnlockableBackground`, 0x40464200!8070FE55).
/// The layout's only drawing is the PNG `NoGroup_UnlockableBackground-00`
/// (0x31A44893!C385FB5D): uv (0, 0, 0.84375, 0.546875) = a 27 x 35 px dark
/// shield, kept at that size and centred in the item window (`P[0.5 ..]`
/// anchors). (The earlier `common~!unavailable-item` "?" square belongs to
/// other layouts.)
const SHIELD: ResourceKey = ResourceKey {
    type_id: PNG,
    group_id: 0x31A4_4893,
    instance_id: 0xC385_FB5D,
};
const SHIELD_UV: [f32; 4] = [0.0, 0.0, 0.84375, 0.546875];
const SHIELD_SIZE: [f32; 2] = [27.0, 35.0];
/// `global_ui_creature~!cgm_icon_sunburst_01`: the "new part" burst.
const BURST: ResourceKey = ResourceKey {
    type_id: PNG,
    group_id: 0x106C_7116,
    instance_id: 0x0F34_769F,
};
/// ADAPTED: the burst's size relative to the item window, measured on the
/// retail frame (burst 0.18 of the panel width, item 0.22).
const BURST_SCALE: f32 = 0.8;

/// `palettePageItemAspectRatio` (0x05ADC18D) of both Cell palette templates.
pub(crate) const ITEM_ASPECT: f32 = 1.0;
/// `palettePageItemMaxPercentageHeight` (0x05B9A649): the parts template does
/// not set it, so the `PalettePage` constructor's default (0x5C809D) applies;
/// the paint template sets 0.2.
pub(crate) const PARTS_MAX_HEIGHT: f32 = 1.0;
pub(crate) const PAINT_MAX_HEIGHT: f32 = 0.2;

/// `PalettePageUI::GetItemArea` (0x5C8E60) for a relative layout with relative
/// item sizes (`palettePageUseRelativeLayout` 1, absolute item size 0; the page
/// borders are 0 in both installed templates; spacing and offset properties are
/// only used by the non-relative branch). The `w` x `h` page area is divided
/// into `cols` x `rows` equal cells; an item is centred in its cell, `percent`
/// x `w` wide and `width / aspect` tall, with its height limited to
/// `max_height` x `h` (the width then follows the aspect). Page-relative
/// `[x1, y1, x2, y2]`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn item_area(w: f32, h: f32, cols: u32, rows: u32, row: u32, col: u32, percent: f32, aspect: f32, max_height: f32) -> [f32; 4] {
    let (cw, ch) = (w / cols.max(1) as f32, h / rows.max(1) as f32);
    let (cx, cy) = (cw * (col as f32 + 0.5), ch * (row as f32 + 0.5));
    let mut width = percent * w;
    let mut height = width / aspect.max(1e-3);
    let limit = max_height * h;
    if height > limit {
        height = limit;
        width = limit * aspect;
    }
    [cx - width * 0.5, cy - height * 0.5, cx + width * 0.5, cy + height * 0.5]
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorRoot {
    Frame,
    Palette,
    Category,
    Page,
    Budget,
    Name,
    Complexity,
    Header,
    Tray(usize),
    Verb(usize, usize),
    Rollover,
    RolloverLine(usize),
    Message,
    /// Paint mode: top bar, category panel, colour row, page of styles, pager.
    PaintPalette,
    PaintCategory,
    PaintSwatches,
    PaintPage,
    Pager,
    /// The camera hint ("Click and drag the background to rotate the camera"):
    /// the `hint` layout.
    Hint,
    /// The shade panel of a held colour swatch and the chosen shade flying
    /// back (`cell_editor_swatch`).
    PaintShades,
    PaintFlyer,
}

/// Screen facts other systems need (physical pixels of the SPUI surface).
#[derive(Resource, Default, Clone, Debug)]
pub struct CellEditorUiLayout {
    /// Shift of the projection centre so the cell sits in the middle of the
    /// area right of the palette.
    pub view_shift: f32,
    pub palette_right: f32,
    /// Item windows of the page layout -> palette part.
    pub items: Vec<(WinId, u32)>,
    /// Windows of the Paint mode swatches and style tiles.
    pub paint: cell_editor_paint::PaintWindows,
    /// What the dynamic roots were built for.
    built: Option<BuiltKey>,
    /// Paint page the dynamic paint roots show (and the swatch count).
    paint_built: Option<(usize, usize)>,
    /// The camera hint (`edi-cameramoved`), read once.
    hint: Option<crate::cell_hud::dialogs::HintDef>,
}

#[derive(Clone, Debug, PartialEq)]
struct BuiltKey {
    page: Vec<(u32, bool, bool)>,
    verbs: Vec<Vec<(u32, String)>>,
    rollover: Option<(u32, usize)>,
}

pub(crate) fn image(file: ResourceKey) -> ImageRef {
    ImageRef {
        file,
        uv: [0.0, 0.0, 1.0, 1.0],
        dims: None,
    }
}

pub(crate) fn plain(id: WinId, parent: Option<WinId>, area: [f32; 4], drawable: Option<Drawable>, shade: u32) -> Window {
    Window {
        id,
        element: id,
        class: class::WINDOW,
        kind: WindowKind::Window,
        parent,
        children: vec![],
        flags: flag::VISIBLE | flag::ENABLED | flag::IGNORE_MOUSE,
        control_id: 0,
        command_id: 0,
        area,
        caption: None,
        fill_color: 0,
        shade_color: shade,
        text_style: 0,
        drawable,
        procs: vec![],
        extra: Extra::None,
    }
}

/// The page of palette items: one button per part at its row/column of the
/// page area, placed by the retail relative layout ([`item_area`],
/// `palettePageItemPercentageWidth` of the page width).
fn page_layout(
    data: &CellEditorData,
    run: &RunProgress,
    page: &PalettePage,
    area: [f32; 4],
    procs: Vec<WinProc>,
) -> (Layout, Vec<(WinId, u32)>) {
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![plain(0, None, area, None, 0xFFFF_FFFF)];
    windows[0].flags = flag::VISIBLE | flag::ENABLED;
    // The page glides and fades with the panel it sits on.
    windows[0].procs = procs;
    let mut items = Vec::new();
    for item in &page.items {
        let Some(info) = data.parts.get(&item.part) else {
            continue;
        };
        let r = item_area(
            w,
            h,
            page.columns,
            page.rows,
            item.row,
            item.column,
            page.item_width_fraction,
            ITEM_ASPECT,
            PARTS_MAX_HEIGHT,
        );
        // ADAPTED: corners snapped to whole pixels (retail draws at the float
        // positions).
        let rect = [r[0].round(), r[1].round(), r[2].round(), r[3].round()];
        let (iw, ih) = (rect[2] - rect[0], rect[3] - rect[1]);
        let button = windows.len();
        windows.push(Window {
            class: class::BUTTON,
            kind: WindowKind::Button,
            flags: flag::VISIBLE | flag::ENABLED,
            control_id: item.part,
            extra: Extra::Button(ButtonData::default()),
            ..plain(button, Some(0), rect, None, 0xFFFF_FFFF)
        });
        windows[0].children.push(button);
        let local = [0.0, 0.0, iw, ih];
        let unlocked = cell_editor::part_is_unlocked(run, info);
        if unlocked && cell_editor::part_is_new(run, info) {
            // The burst sits behind the icon.
            let b = windows.len();
            let (bx, by) = (iw * (1.0 - BURST_SCALE) * 0.5, ih * (1.0 - BURST_SCALE) * 0.5);
            windows.push(plain(
                b,
                Some(button),
                [bx, by, iw - bx, ih - by],
                Some(Drawable::Image(ImageDrawable {
                    image: Some(image(BURST)),
                    ..Default::default()
                })),
                0xFFFF_FFFF,
            ));
            windows[button].children.push(b);
        }
        let icon = windows.len();
        // A part that is not unlocked shows the 27 x 35 shield centred in the
        // item window; the others their icon at the item size.
        let (picture, icon_rect) = if unlocked {
            (ImageRef { file: info.icon, uv: [0.0, 0.0, 1.0, 1.0], dims: None }, local)
        } else {
            let (sx, sy) = ((iw - SHIELD_SIZE[0]) * 0.5, (ih - SHIELD_SIZE[1]) * 0.5);
            (
                ImageRef {
                    file: SHIELD,
                    uv: SHIELD_UV,
                    dims: Some([SHIELD_SIZE[0] as u32, SHIELD_SIZE[1] as u32]),
                },
                [sx.round(), sy.round(), sx.round() + SHIELD_SIZE[0], sy.round() + SHIELD_SIZE[1]],
            )
        };
        windows.push(plain(
            icon,
            Some(button),
            icon_rect,
            Some(Drawable::Image(ImageDrawable {
                image: Some(picture),
                ..Default::default()
            })),
            0xFFFF_FFFF,
        ));
        // Front-most child first: the icon above the burst.
        windows[button].children.insert(0, icon);
        items.push((button, item.part));
    }
    let layout = Layout {
        version: 0,
        windows,
        roots: vec![0],
        hit_masks: vec![],
        warnings: vec![],
        unknown_classes: vec![],
    };
    (layout, items)
}

/// Spawn the editor layouts when a session starts, despawn them after.
#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    editor: Res<CellEditor>,
    originals: Res<Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &EditorRoot)>,
    mut layout: ResMut<CellEditorUiLayout>,
) {
    let want = editor.active && !originals.resource_index.records.is_empty();
    if !want {
        if !roots.is_empty() {
            for (e, _) in &roots {
                commands.entity(e).despawn();
            }
            *layout = CellEditorUiLayout::default();
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
    // `hidden`: the root window starts invisible (Paint mode roots wait for the
    // brush button; the first frame must not flash them).
    let mut spawn = |name: &str, kind: EditorRoot, policy: RootPolicy, z: i32, hidden: bool, prepare: Option<fn(&mut Layout)>| {
        if let Some(l) = assets.layout(index, name) {
            let l = if hidden || prepare.is_some() {
                let mut copy = (*l).clone();
                if hidden {
                    for &r in &copy.roots.clone() {
                        copy.windows[r].flags &= !flag::VISIBLE;
                    }
                }
                if let Some(prepare) = prepare {
                    prepare(&mut copy);
                }
                Arc::new(copy)
            } else {
                l
            };
            let root = view::spawn(&mut commands, l, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        } else {
            warn!("cell editor: layout {name} not installed");
        }
    };
    // The palettes sit under the frame: its mode buttons and the Cancel/Save
    // buttons draw over the palette bars and panels, as in the retail frames.
    spawn("GlobalEditorUI", EditorRoot::Frame, RootPolicy::Fit, 100, false, None);
    spawn("EditorPartsPaletteCategory", EditorRoot::Category, RootPolicy::Fit, 97, false, None);
    spawn("EditorPartsPalette", EditorRoot::Palette, RootPolicy::Fit, 98, false, None);
    // The Cell's paint category (`0x3FCA51EA`; `EditorPaintPaletteCategory` is the creature's).
    spawn("0x3FCA51EA", EditorRoot::PaintCategory, RootPolicy::Fit, 97, false, None);
    spawn("EditorPaintPalette", EditorRoot::PaintPalette, RootPolicy::Fit, 98, true, None);
    spawn("EditorBudget", EditorRoot::Budget, RootPolicy::Authored, 104, false, None);
    spawn("EditorNameDescribe-NoTag", EditorRoot::Name, RootPolicy::Authored, 104, false, None);
    spawn("EditorComplexityMeter", EditorRoot::Complexity, RootPolicy::Authored, 104, false, None);
    spawn("EditorHeaderTray", EditorRoot::Header, RootPolicy::Authored, 104, false, None);
    spawn("EditorPageControls", EditorRoot::Pager, RootPolicy::Authored, 99, true, None);
    spawn("RolloverEditorMessage", EditorRoot::Message, RootPolicy::Authored, 120, false, None);
    // The camera hint is a `cHintManager` hint: the `hint` layout, bound and
    // faded like the Cell HUD's hint bar.
    spawn(crate::cell_hud::dialogs::HINT_LAYOUT, EditorRoot::Hint, RootPolicy::Fit, 119, false, None);
    *layout = CellEditorUiLayout::default();
}

fn rect_of(ui: &SpuiState, control: u32) -> Option<[f32; 4]> {
    let w = ui.find(control)?;
    let r = ui.rect(w);
    Some([r.x1, r.y1, r.x2, r.y2])
}

/// Move a sub layout's root window so its authored size starts at `(x, y)`.
fn place_root(ui: &mut SpuiState, x: f32, y: f32) {
    let Some(&root) = ui.layout.roots.first() else {
        return;
    };
    let a = ui.layout.window(root).area;
    ui.set_area(root, [x, y, x + a[2] - a[0], y + a[3] - a[1]]);
}

/// Move a sub layout's root window to `(x, y)` and make it `extra` pixels taller
/// (the rollover's background and frame are anchored to the root, so they grow
/// around the ability lines the root now holds).
fn place_root_grow(ui: &mut SpuiState, x: f32, y: f32, extra: f32) {
    let Some(&root) = ui.layout.roots.first() else {
        return;
    };
    let a = ui.layout.window(root).area;
    ui.set_area(root, [x, y, x + a[2] - a[0], y + a[3] - a[1] + extra]);
}

/// The verb tray's icon host window `04978D2D` of `EditorVerbTray` (SPUI dump,
/// authored 1280 x 900): (93, 24, 161, 56) inside the root (27, 22, 202, 57), so
/// 68 x 32 at (66, 2) of the 175 x 35 root.
const TRAY_ROOT_WIDTH: f32 = 175.0;
/// The root's gutters around the icon host window (0x5E7EB4..0x5E7F7B: `+0xB8` = the host's left offset 66, `+0xC0` = the
/// root width minus the host's right offset, 175 - 134 = 41) and the host's vertical margins 2 + 1 (`+0xBC`, `+0xC4`).
const TRAY_GUTTERS: [f32; 2] = [66.0, 41.0];
const TRAY_ICON_MARGIN: f32 = 3.0;
/// The tray's icon aspect (`+0xCC`, set by the first icon added, 0x5E8387 -> 0x5E1FC0): the `editorVerbIcon` root is 26 x 39.
const VERB_ICON_ASPECT: f32 = 26.0 / 39.0;
/// Authored root heights of `EditorHeaderTray` (25, 22, 202, 66) and `EditorVerbTray` (27, 22, 202, 57).
const HEADER_TRAY_HEIGHT: f32 = 44.0;
const VERB_TRAY_HEIGHT: f32 = 35.0;
const TRAY_HOST_REL: [f32; 4] = [66.0, 2.0, 134.0, 34.0];

/// Tops of the collection's trays under the verb host's top (`LayoutCollection` 0x5E8A20, kJustified, justification
/// right): the header tray first, then every verb tray, each slot as high as that tray's authored root.
fn tray_slots(host_top: f32, verb_trays: usize) -> Vec<f32> {
    (0..verb_trays).map(|t| host_top + HEADER_TRAY_HEIGHT + VERB_TRAY_HEIGHT * t as f32).collect()
}

/// The root rectangle of a verb tray with `count` icons whose slot starts at `top` (0x5E6430 right justified): the
/// authored height, the right edge on the host's, as wide as the two gutters plus the packed icon cells (`aspect` is
/// the tray's icon aspect, the first icon's layout root: width over height).
fn tray_root_rect(host_right: f32, top: f32, count: usize, aspect: f32) -> [f32; 4] {
    let cell = aspect * (VERB_TRAY_HEIGHT - TRAY_ICON_MARGIN);
    let width = TRAY_GUTTERS[0] + TRAY_GUTTERS[1] + cell * count as f32;
    [host_right - width, top, host_right, top + VERB_TRAY_HEIGHT]
}

/// `verbIconLayout` (0x04A20FE7): the layout of one verb's icon. VERIFIED: `VerbIconData::ParseProp` (0x5E54A0) reads it
/// from the verb file into `+0x9C` (type 0x510A95B, group 0x40464100 are the defaults of the key), the tray's icon
/// creation (0x5E8160, branch 0x5E82A3) loads that layout for the item unless the tray sets its own
/// (`verbTrayOverrideIconLayout` 0xF1FA52AF, which no Cell tray does). A verb without one (the diet verbs, which only
/// the header tray's round icon shows) takes the collection's `editorVerbIcon`.
const VERB_ICON_LAYOUT: u32 = 0x04A2_0FE7;
const FALLBACK_VERB_LAYOUT: &str = "editorVerbIcon";
/// The global image library the verb ids are looked up in (`0x4592B0` -> 0x8062B0: the window with that control id in
/// the layout kept at 0x15D2DBC, loaded by 0x459150 and drawn with that window's image): the installed layout
/// `VerbIcons` (40464200!44B79D08), one window per verb image id, each showing a region of the atlas
/// `31A44893!E0233C2C / 2D` (the ringed discs, the 26 / 34 px glyphs).
const VERB_ICONS_LAYOUT: &str = "VerbIcons";

/// The picture of a verb image id exactly as the game resolves it (`0x4592B0`): the image of the window `id` in the
/// `VerbIcons` layout. Cached per id.
fn verb_picture(
    index: &crate::asset_catalog::ResourceIndex,
    assets: &mut SpuiAssets,
    cache: &mut HashMap<u32, Option<ImageRef>>,
    id: u32,
) -> Option<ImageRef> {
    if id == 0 {
        return None;
    }
    if let Some(hit) = cache.get(&id) {
        return hit.clone();
    }
    let found = assets.layout(index, VERB_ICONS_LAYOUT).and_then(|l| {
        l.windows.iter().find(|w| w.control_id == id).and_then(|w| match &w.drawable {
            Some(Drawable::Image(d)) => d.image.clone(),
            _ => None,
        })
    });
    cache.insert(id, found.clone());
    found
}

/// The layout name of the icon of the verb of `category` (see [`VERB_ICON_LAYOUT`]).
fn verb_layout_name(
    index: &crate::asset_catalog::ResourceIndex,
    assets: &mut SpuiAssets,
    data: &CellEditorData,
    category: u32,
    cache: &mut HashMap<u32, String>,
) -> String {
    if let Some(name) = cache.get(&category) {
        return name.clone();
    }
    let own = data
        .verbs
        .iter()
        .find(|v| v.category == category)
        .and_then(|v| index.properties(v.key).ok())
        .and_then(|p| crate::cell_editor_data::key_of(&p, VERB_ICON_LAYOUT))
        .map(|k| format!("0x{:08X}", k.instance_id))
        .filter(|name| assets.layout(index, name).is_some());
    let name = own.unwrap_or_else(|| FALLBACK_VERB_LAYOUT.to_string());
    cache.insert(category, name.clone());
    name
}

/// Width over height of a layout's root window (`+0x180` of the verb icon, 0x5E26FF; the first icon of a tray sets the
/// tray's aspect).
fn layout_aspect(layout: &Layout) -> Option<f32> {
    let root = *layout.roots.first()?;
    let a = layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    (w > 0.0 && h > 0.0).then_some(w / h)
}

/// The cells of a verb tray's icons, from the tray's root position. VERIFIED (the
/// tray class, vtable 0x13F9668, loader 0x5E7860; layout = vtable slot 15 =
/// 0x5E6430, cell size 0x5E6060 / 0x5E5F40 / 0x5E5FD0): the tray's `+0x78`
/// justification is `verbTrayJustification` (0x04A213B2) and defaults to
/// 0xE816F049 = a horizontal row packed against the host window's RIGHT edge
/// (0x5E6631: left = right - content width; 0x3EB432 = the same row packed
/// left). The first icon added sets the tray's aspect (`+0xCC` = the icon layout
/// root's width / height, 0x5E8387 -> 0x5E1FC0 reads item `+0x180` = right -
/// left over bottom - top, 0x5E26FF); a horizontal tray then sizes every cell
/// to the host's full height by `aspect` times that (0x5E6060: `+0xD0` = host
/// height, `+0xD4` = aspect x it), and the layout packs the cells one after
/// the other without a gap (cursor += cell width, 0x5E6552).
fn verb_cells(tray_x: f32, tray_y: f32, count: usize, aspect: f32, right_aligned: bool) -> Vec<[f32; 4]> {
    let host = [
        tray_x + TRAY_HOST_REL[0],
        tray_y + TRAY_HOST_REL[1],
        tray_x + TRAY_HOST_REL[2],
        tray_y + TRAY_HOST_REL[3],
    ];
    let height = host[3] - host[1];
    let width = aspect * height;
    let left = if right_aligned { host[2] - width * count as f32 } else { host[0] };
    (0..count)
        .map(|i| [left + width * i as f32, host[1], left + width * (i + 1) as f32, host[3]])
        .collect()
}

/// Height of one ability line (`EditorPaletteRolloverAbilityLine` is 29 px)
/// and of the refusal row under the header (ADAPTED).
const LINE_HEIGHT: f32 = 29.0;
const ERROR_HEIGHT: f32 = 22.0;
/// Gap between a palette item and its rollover box.
const ROLLOVER_GAP: f32 = 4.0;
/// Offset of the ability line container (`EditorPaletteItemRollover` window
/// 0x04C01A78 at (442, 338) against the root at (428, 298)).
const LINES_OFFSET: [f32; 2] = [14.0, 40.0];

/// Resize a sub layout's root window to `rect` (its anchored children follow).
fn fit_root(ui: &mut SpuiState, rect: [f32; 4]) {
    if let Some(&root) = ui.layout.roots.first() {
        ui.set_area(root, rect);
    }
}

/// Show or hide a whole layout (its visibility effects run).
fn show_root(ui: &mut SpuiState, on: bool) {
    if let Some(&root) = ui.layout.roots.first() {
        if ui.visible(root) != on {
            ui.set_visible(root, on);
        }
    }
}

/// What the Paint category layout offers the dynamic paint roots.
struct PaintFacts {
    page: [f32; 4],
    swatches: [f32; 4],
    pager: Option<[f32; 4]>,
    /// Visibility effects of the category panel (the pieces glide with it).
    procs: Vec<WinProc>,
}

fn panel_effects(ui: &SpuiState, control: u32) -> Vec<WinProc> {
    ui.find(control)
        .map(|w| cell_editor_paint::effect_procs(&ui.layout, w))
        .unwrap_or_default()
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

/// PNG of a verb image id (the icons live in several UI groups).
fn verb_png(index: &crate::asset_catalog::ResourceIndex, cache: &mut HashMap<u32, Option<ResourceKey>>, image: u32) -> Option<ResourceKey> {
    *cache.entry(image).or_insert_with(|| {
        index
            .records
            .keys()
            .filter(|k| k.type_id == PNG && k.instance_id == image)
            .min_by_key(|k| k.group_id)
            .copied()
    })
}

/// `verbTrayIconID` (0x06286814) of each non-header tray of `CLE_VerbTrayCollection`,
/// in the order of `CellEditorData::trays` (the round icon of the tray).
fn cell_tray_icons(index: &crate::asset_catalog::ResourceIndex) -> Vec<Option<u32>> {
    use crate::cell_editor_data::{key_list, key_of, prop_key, VERBTRAY_GROUP};
    const VERB_TRAYS: u32 = 0x04AA_3838;
    const VERB_TRAY_ICON_ID: u32 = 0x0628_6814;
    let Ok(collection) = index.properties(prop_key(VERBTRAY_GROUP, crate::world_objects::name_id("CLE_VerbTrayCollection"))) else {
        return vec![];
    };
    key_list(&collection, VERB_TRAYS)
        .into_iter()
        .filter(|k| k.group_id != crate::world_objects::name_id("EditorHeaderTray"))
        .filter_map(|k| index.properties(prop_key(VERBTRAY_GROUP, k.instance_id)).ok())
        .map(|tray| key_of(&tray, VERB_TRAY_ICON_ID).map(|k| k.instance_id).filter(|i| *i != 0 && *i != u32::MAX))
        .collect()
}

/// The ability trays: diet header plus the verb trays of
/// `CLE_VerbTrayCollection` with the abilities the cell has.
fn active_verbs(data: &CellEditorData, run: &RunProgress) -> Vec<Vec<(u32, String)>> {
    let mounted: Vec<u32> = run.cell_parts.iter().filter_map(|p| data.definition_of(p)).collect();
    let mut trays = Vec::new();
    for tray in &data.trays {
        let mut verbs = Vec::new();
        for category in tray {
            for v in data.verbs.iter().filter(|v| v.category == *category) {
                let granted = mounted.iter().any(|m| {
                    v.parts.contains(m)
                        || data.parts.get(m).is_some_and(|i| {
                            [i.left, i.right, i.center].into_iter().flatten().any(|f| v.parts.contains(&f))
                        })
                });
                if granted && !verbs.iter().any(|(c, _)| *c == v.category) {
                    let value = if v.category == crate::world_objects::name_id("cell_gamespeed") {
                        cell_editor::speed_level(data, &run.cell_parts).to_string()
                    } else {
                        String::new()
                    };
                    verbs.push((v.category, value));
                }
            }
        }
        trays.push(verbs);
    }
    trays
}

/// Push the editor state into the layouts and (re)build the dynamic ones.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut commands: Commands,
    mut editor: ResMut<CellEditor>,
    data: Res<CellEditorData>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut layout: ResMut<CellEditorUiLayout>,
    camera: Query<Entity, With<crate::GameCamera>>,
    mut roots: Query<(Entity, &mut SpuiState, &EditorRoot)>,
    mut png_cache: Local<HashMap<u32, Option<ResourceKey>>>,
    mut tray_icons: Local<Option<Vec<Option<u32>>>>,
    mut pictures: Local<HashMap<u32, Option<ImageRef>>>,
    mut verb_layouts: Local<HashMap<u32, String>>,
) {
    let index = &originals.resource_index;
    let paint_mode = editor.mode == Mode::Paint;
    if tray_icons.is_none() && !index.records.is_empty() {
        *tray_icons = Some(cell_tray_icons(index));
    }
    if layout.hint.is_none() {
        layout.hint = Some(
            crate::cell_hud::dialogs::read_hint(index, &mut assets, crate::cell_editor_data::CAMERA_HINT)
                .unwrap_or_default(),
        );
    }
    let mut frame_rects: HashMap<u32, [f32; 4]> = HashMap::new();
    let mut page_area = None;
    let mut build_procs = Vec::new();
    let mut paint_facts: Option<PaintFacts> = None;
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
                page_area = rect_of(&ui, id::PAGE_AREA).zip(rect_of(&ui, id::CATEGORY_PANEL));
                build_procs = panel_effects(&ui, id::CATEGORY_PANEL);
            }
            EditorRoot::PaintCategory => {
                paint_facts = rect_of(&ui, id::PAGE_AREA)
                    .zip(rect_of(&ui, paint_id::SWATCH_AREA))
                    .map(|(page, swatches)| PaintFacts {
                        page,
                        swatches,
                        pager: rect_of(&ui, paint_id::PAGE_HOST),
                        procs: panel_effects(&ui, id::CATEGORY_PANEL),
                    });
            }
            _ => {}
        }
    }
    // The cell sits in the middle of the area right of the palette.
    if let Some((_, panel)) = page_area {
        layout.palette_right = panel[2];
        if screen[0] > 0.0 {
            let center = (panel[2] + screen[0]) * 0.5;
            layout.view_shift = (center - screen[0] * 0.5).round();
        }
    }
    // Dynamic roots: the page items, the verb trays and the rollover.
    let page = data.palette(data.level(editor.level).parts_palette.unwrap_or(GAME_PALETTE)).first().cloned();
    let trays = active_verbs(&data, &run);
    let hover = editor.hover_item.filter(|_| editor.carry.is_none());
    let rollover = hover.map(|p| (p, data.verbs_of(p).len()));
    let key = BuiltKey {
        page: page
            .as_ref()
            .map(|p| {
                p.items
                    .iter()
                    .filter_map(|i| {
                        let info = data.parts.get(&i.part)?;
                        Some((i.part, cell_editor::part_is_unlocked(&run, info), cell_editor::part_is_new(&run, info)))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        verbs: trays.clone(),
        rollover,
    };
    let Ok(camera) = camera.single() else {
        return;
    };
    if page_area.is_some() && layout.built.as_ref() != Some(&key) {
        let changed_page = layout.built.as_ref().is_none_or(|b| b.page != key.page);
        let changed_verbs = layout.built.as_ref().is_none_or(|b| b.verbs != key.verbs);
        let changed_roll = layout.built.as_ref().is_none_or(|b| b.rollover != key.rollover);
        for (e, _, kind) in &roots {
            let stale = match kind {
                EditorRoot::Page => changed_page,
                EditorRoot::Tray(_) | EditorRoot::Verb(..) => changed_verbs,
                EditorRoot::Rollover | EditorRoot::RolloverLine(_) => changed_roll,
                _ => false,
            };
            if stale {
                commands.entity(e).despawn();
            }
        }
        if changed_page {
            if let (Some(page), Some((area, _))) = (page.as_ref(), page_area) {
                let (l, items) = page_layout(&data, &run, page, area, build_procs.clone());
                let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(EditorRoot::Page);
                layout.items = items;
            }
        }
        if changed_verbs {
            for (t, verbs) in trays.iter().enumerate() {
                // Every tray of the collection exists from the start (`InitTrays` 0x606000 builds one per
                // `verbTrays` entry): an empty tray keeps its slot and shows only its round icon.
                if let Some(l) = assets.layout(index, "EditorVerbTray") {
                    let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 104);
                    commands.entity(root).insert(EditorRoot::Tray(t));
                }
                for (v, (category, _)) in verbs.iter().enumerate() {
                    // Each verb brings its own icon layout (`verbIconLayout`, see `VERB_ICON_LAYOUT`).
                    let name = verb_layout_name(index, &mut assets, &data, *category, &mut verb_layouts);
                    if let Some(l) = assets.layout(index, &name) {
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
    // Paint mode's dynamic roots: the colour row and the shown page of styles.
    if let Some(facts) = &paint_facts {
        let pages = data.paint.pages.len();
        let page_no = editor.paint_page.min(pages.saturating_sub(1));
        let want = (page_no, data.paint.swatches.len());
        if data.paint.loaded && layout.paint_built != Some(want) {
            let first = layout.paint_built.is_none();
            let page_stale = layout.paint_built.map(|b| b.0) != Some(page_no);
            for (e, _, kind) in &roots {
                if (*kind == EditorRoot::PaintPage && page_stale) || (*kind == EditorRoot::PaintSwatches && first) {
                    commands.entity(e).despawn();
                }
            }
            // A page turn swaps the tiles in place; the first build waits,
            // hidden, for the brush button (the panel glides in with it).
            let start_visible = paint_mode && !first;
            if first {
                let (mut l, ids) = cell_editor_paint::swatch_row_layout(
                    &data.paint.art,
                    &data.paint.swatches,
                    facts.swatches,
                    facts.procs.clone(),
                    data.paint.picker.gap,
                    data.paint.default_index(),
                );
                if !start_visible {
                    l.windows[0].flags &= !flag::VISIBLE;
                }
                let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(EditorRoot::PaintSwatches);
                layout.paint.swatches = ids;
            }
            if page_stale {
                if let Some(page) = data.paint.pages.get(page_no) {
                    let (mut l, ids) =
                        cell_editor_paint::cell_tile_layout(&data.paint, page, facts.page, &data.paint.art, facts.procs.clone());
                    if !start_visible {
                        l.windows[0].flags &= !flag::VISIBLE;
                    }
                    let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
                    commands.entity(root).insert(EditorRoot::PaintPage);
                    layout.paint.tiles = ids;
                }
            }
            layout.paint_built = Some(want);
        }
    }
    // Hovered palette item (for the rollover and the press in `actions`).
    let mut hovered = None;
    let mut item_rect = None;
    for (_, ui, kind) in &roots {
        if *kind == EditorRoot::Page && ui.ready && !paint_mode {
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
    let level = editor.level;
    let complexity = cell_editor::complexity(&data, &run.cell_parts) as f32 / data.complexity_limit(level).max(1) as f32;
    let diet = cell_editor::diet(&data, &run.cell_parts);
    // Header/tray stacking in the verb host (right aligned, top down).
    let verbs_host = frame_rects.get(&id::HOST_VERBS).copied();
    let mut tray_y: HashMap<usize, f32> = HashMap::new();
    if let Some(host) = verbs_host {
        // VERIFIED (cell-final5): `cSPVerbTrayCollection::LayoutCollection` (0x5E8A20, run once from `Init` 0x6059F0 after
        // `InitTrays` 0x606000) stacks the collection's trays in the host window by their ORIGINAL root heights: the
        // `CLE_VerbTrayCollection` record (AF028F41!64BC540B, parent the `VerbTrayCollectionTemplate`) has
        // `verbTrayJustification` right (0xE816F049), `verbCollectionLayoutStyle` (0x04B43AA3) kJustified (0xFEA10E68) and four
        // `verbTrays` (the header tray 177 x 44 and three 175 x 35). The kJustified branch (0x5E8CB2, vertical stacking
        // 0x5E8D4E) puts tray i at (right - width, cursor) with `cursor += +0xA4` (the authored root height) and nothing
        // else: no pitch of the host's height / n (that is the kFill branch), and no compaction (the loop runs over the
        // whole vector; nothing relays it when a tray's icons come or go, so an empty tray keeps its slot).
        for (t, y) in tray_slots(host[1], trays.len()).into_iter().enumerate() {
            tray_y.insert(t, y);
        }
    }
    // The tray's icon aspect: the first icon added sets it (`+0xCC`, 0x5E8387 -> 0x5E1FC0 = the icon layout root's
    // width over height) and sizes every cell of the tray by it (0x5E6060).
    let tray_aspect: Vec<f32> = trays
        .iter()
        .map(|verbs| {
            verbs
                .first()
                .and_then(|(category, _)| {
                    let name = verb_layout_name(index, &mut assets, &data, *category, &mut verb_layouts);
                    assets.layout(index, &name).and_then(|l| layout_aspect(&l))
                })
                .unwrap_or(VERB_ICON_ASPECT)
        })
        .collect();
    let prompt = data.name_prompt.clone().unwrap_or_else(|| "Name Your Species".into());
    let name_text = if editor.naming {
        format!("{}_", run.cell_creation.name)
    } else if run.cell_creation.name.is_empty() {
        prompt
    } else {
        run.cell_creation.name.clone()
    };
    let message = editor.message.as_ref().map(|(t, _)| t.clone());
    for (_, mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match *kind {
            EditorRoot::Frame => bind_frame(&mut ui, &editor),
            EditorRoot::Palette | EditorRoot::PaintPalette => {
                let paint_bar = *kind == EditorRoot::PaintPalette;
                show_root(&mut ui, paint_bar == paint_mode);
                // One category: no tabs, no tab scroll arrows and no selected
                // tab highlight animation.
                for c in [id::TAB_LEFT, id::TAB_RIGHT, id::TAB_HIGHLIGHT, id::TABS, id::BAR_LEFT, id::BAR_TABS] {
                    if let Some(w) = ui.find(c) {
                        ui.set_visible(w, false);
                    }
                }
                // One category: `PaletteUI::UpdateTabs` (0x5CAAE0, taken when
                // there are no tab buttons, branch 0x5CAB8D) shows only the
                // full-width bar and hides the strip pieces.
                if let Some(w) = ui.find(id::BAR_FULL) {
                    ui.set_visible(w, true);
                }
            }
            EditorRoot::PaintCategory => {
                // The panel glides in from the right when the brush is chosen.
                if let Some(w) = ui.find(id::CATEGORY_PANEL) {
                    ui.set_visible(w, paint_mode);
                }
                if let Some(w) = ui.find(id::CATEGORY_GLOW) {
                    ui.set_visible(w, false);
                }
                // Designer fill of the swatch container (the swatches are
                // their own layout here); retail draws no strip behind them.
                if let Some(w) = ui.find(paint_id::SWATCH_AREA) {
                    ui.set_visible(w, false);
                }
            }
            EditorRoot::PaintSwatches => {
                // Clocks, inflation, glows and the shade panel:
                // `cell_editor_swatch::swatches`.
                show_root(&mut ui, paint_mode);
            }
            EditorRoot::PaintPage => {
                show_root(&mut ui, paint_mode);
                for (button, glow, theme) in &layout.paint.tiles {
                    let hover = ui.states[*button] & state::HOVER != 0;
                    ui.set_visible(*glow, run.cell_creation.paint.theme == *theme || hover);
                }
            }
            EditorRoot::Pager => {
                show_root(&mut ui, paint_mode);
                if let Some(host) = paint_facts.as_ref().and_then(|f| f.pager) {
                    fit_root(&mut ui, host);
                }
                let pages = data.paint.pages.len();
                let page = editor.paint_page.min(pages.saturating_sub(1));
                if let Some(w) = ui.find(paint_id::PAGE_TEXT) {
                    ui.set_text(w, cell_editor_paint::page_text(page, pages));
                }
                if let Some(w) = ui.find(paint_id::PAGE_PREV) {
                    ui.set_enabled(w, page > 0);
                }
                if let Some(w) = ui.find(paint_id::PAGE_NEXT) {
                    ui.set_enabled(w, page + 1 < pages);
                }
            }
            EditorRoot::Category => {
                if let Some(w) = ui.find(id::CATEGORY_PANEL) {
                    ui.set_visible(w, !paint_mode);
                }
                // The selected-category glow is not lit for the one Cell page.
                if let Some(w) = ui.find(id::CATEGORY_GLOW) {
                    ui.set_visible(w, false);
                }
                if std::env::var_os("CELL_EDITOR_DEBUG").is_some() {
                    for (i, w) in ui.layout.windows.iter().enumerate() {
                        info!("category window E{} ctrl {:08X} rect {:?}", w.element, w.control_id, ui.rect(i));
                    }
                }
            }
            EditorRoot::Page => show_root(&mut ui, !paint_mode),
            EditorRoot::Budget => {
                // Painting costs nothing: the DNA budget is not shown.
                show_root(&mut ui, !paint_mode);
                if let Some(r) = frame_rects.get(&id::HOST_BUDGET) {
                    place_root(&mut ui, r[0], r[1]);
                }
                if let Some(w) = ui.find(id::BUDGET_TEXT) {
                    ui.set_text(w, format!("{DNA_GLYPH}{dna}"));
                }
            }
            EditorRoot::Name => {
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
                show_root(&mut ui, !paint_mode);
                if let Some(r) = frame_rects.get(&id::HOST_COMPLEXITY) {
                    place_root(&mut ui, r[0], r[1]);
                }
                if let Some(w) = ui.find(id::COMPLEXITY_FILL) {
                    // ADAPTED: the fill window's width follows the complexity
                    // fraction (72 px authored bar).
                    let a = ui.layout.window(w).area;
                    let full = 72.0f32.max(a[2] - a[0]);
                    ui.set_area(w, [a[0], a[1], a[0] + full * complexity.clamp(0.0, 1.0), a[3]]);
                    ui.set_visible(w, complexity > 0.0);
                }
                if let Some(w) = ui.find(id::COMPLEXITY_ICON) {
                    let frames = 12.0;
                    ui.set_frame(w, Some(((complexity.clamp(0.0, 1.0) * (frames - 1.0)).round()) as u32));
                }
            }
            EditorRoot::Header => {
                show_root(&mut ui, !paint_mode);
                if let Some(host) = verbs_host {
                    let s = root_size(&ui);
                    place_root(&mut ui, host[2] - s[0], host[1]);
                }
                let (text, icon) = match diet {
                    Diet::Herbivore => ("Herbivore", "stats_veg_icon"),
                    Diet::Carnivore => ("Carnivore", "stats_meat_icon"),
                    Diet::Omnivore => ("Omnivore", "stats_omni_icon"),
                    Diet::None => ("", ""),
                };
                // The diet captions are the verbs' own names.
                let verb = data.verbs.iter().find(|v| v.name == text);
                let caption = verb.map(|v| v.name.clone()).unwrap_or_else(|| text.to_string());
                if let Some(w) = ui.find(id::HEADER_TEXT) {
                    ui.set_text(w, caption);
                }
                if let Some(w) = ui.find(id::TRAY_ICON) {
                    // VERIFIED: the diet tray shows its verb when collapsed (`verbTrayShowVerbIconWhenCollapsed`
                    // 0x04D1A4EE of `CLE_DietVerbTray`) and then sets the round icon window's background to the
                    // active verb's `verbIconTrayOverrideImageID` (0x062CC8D9, item `+0x94`; 0x5E87E3 ->
                    // `SetBackground`), looked up like every verb image (`verb_picture`): the 44 x 44 ringed disc
                    // (green herbivore, red carnivore, blue omnivore).
                    let picture = verb
                        .and_then(|v| v.tray_image)
                        .and_then(|id| verb_picture(index, &mut assets, &mut pictures, id));
                    // Fallback: the plain PNG by name.
                    let png = picture
                        .is_none()
                        .then(|| {
                            (!icon.is_empty())
                                .then(|| verb_png(index, &mut png_cache, crate::world_objects::name_id(icon)))
                                .flatten()
                        })
                        .flatten();
                    let shown = picture.or_else(|| png.map(image));
                    if let Some(shown) = &shown {
                        assets.texture(index, &mut images, shown.file);
                    }
                    ui.set_visible(w, shown.is_some());
                    ui.set_image(w, shown);
                }
            }
            EditorRoot::Tray(t) => {
                show_root(&mut ui, !paint_mode);
                if let (Some(host), Some(y)) = (verbs_host, tray_y.get(&t)) {
                    // 0x5E6430 (the tray's own layout, after the collection's SetArea 0x5E63E0): the root keeps the
                    // authored height, ends on the host's right edge and is as wide as its gutters plus the packed
                    // icon cells (`0xC0 + 0xB8 + content width`); an empty tray is 107 wide.
                    fit_root(&mut ui, tray_root_rect(host[2], *y, trays[t].len(), tray_aspect[t]));
                }
                if let Some(w) = ui.find(id::TRAY_VALUE) {
                    ui.set_text(w, "");
                }
                if let Some(w) = ui.find(id::TRAY_ICON) {
                    // The tray's round icon at the right end of its row
                    // (`verbTrayIconID` 0x06286814 of the `CLE_*VerbTray` file,
                    // 39 x 35, the retail frames show it on every row).
                    // VERIFIED: the tray loader sets the window's background to the image of `verbTrayIconID`
                    // (`+0x6C`) looked up like every verb image (0x5E7E88..0x5E7EA7: `0x4592B0` then
                    // `SetBackground`): the ringed disc of the `VerbIcons` layout, not the bare glyph.
                    let id = tray_icons.as_ref().and_then(|icons| icons.get(t).copied().flatten());
                    let picture = id.and_then(|id| verb_picture(index, &mut assets, &mut pictures, id));
                    let png = picture
                        .is_none()
                        .then(|| id.and_then(|image| verb_png(index, &mut png_cache, image)))
                        .flatten();
                    let shown = picture.or_else(|| png.map(image));
                    if let Some(shown) = &shown {
                        assets.texture(index, &mut images, shown.file);
                    }
                    ui.set_visible(w, shown.is_some());
                    ui.set_image(w, shown);
                }
            }
            EditorRoot::Verb(t, v) => {
                show_root(&mut ui, !paint_mode);
                let Some((category, value)) = trays.get(t).and_then(|tr| tr.get(v)).cloned() else {
                    continue;
                };
                if let (Some(host), Some(y)) = (verbs_host, tray_y.get(&t)) {
                    // 0x5E6430: the icons are packed cells of the host window (see
                    // `verb_cells`); the icon root is resized to its cell, its
                    // windows follow their scale flags.
                    let cells = verb_cells(host[2] - TRAY_ROOT_WIDTH, *y, trays[t].len(), tray_aspect[t], true);
                    if let (Some(cell), Some(&root)) = (cells.get(v), ui.layout.roots.first()) {
                        ui.set_area(root, *cell);
                    }
                }
                let verb = data.verbs.iter().find(|x| x.category == category);
                // The picture is the verb's `verbIconImageID` looked up in the `VerbIcons` layout (see
                // `verb_picture`). The layouts of the Cell verbs (`verbIconLayout`) hold a `SporeStd` button
                // (`06283C92`: the tile `31A44893!DE7FC560` / `B8761340` tinted 0xF03A4FAC behind the icon, or
                // `04976E19`: the icon alone) that draws the picture in its icon slot; `editorVerbIcon` holds a
                // plain `Std` button that shows it whole. The states' `stroke_mode` / `halo_mode` (the dark
                // outline and glow around the icon glyph of the retail frames) are drawn by the SPUI engine
                // (`spui::outline`, `SporeStdDrawable::Paint` 0x8318F0) around the icon set here.
                let target = ui.find(id::VERB_BUTTON).or_else(|| ui.find(id::VERB_TILE_BUTTON));
                if let (Some(w), Some(verb)) = (target, verb) {
                    let picture = verb_picture(index, &mut assets, &mut pictures, verb.image);
                    let png = picture
                        .is_none()
                        .then(|| verb_png(index, &mut png_cache, verb.image))
                        .flatten();
                    let shown = picture.or_else(|| png.map(image));
                    if let Some(shown) = &shown {
                        assets.texture(index, &mut images, shown.file);
                    }
                    if matches!(ui.layout.window(w).drawable, Some(Drawable::SporeStd { .. })) {
                        ui.set_icon(w, shown);
                    } else {
                        ui.set_image(w, shown);
                    }
                }
                if let Some(w) = ui.find(id::VERB_VALUE) {
                    // The value text is authored hidden in the Cell verbs' layouts; the game shows it when there is
                    // a value (`verbIconShowLevel`).
                    ui.set_visible(w, !value.is_empty());
                    ui.set_text(w, value);
                }
            }
            EditorRoot::Rollover => {
                let Some(part) = hover else {
                    continue;
                };
                let Some(info) = data.parts.get(&part) else {
                    continue;
                };
                let reason = cell_editor::refusal(&data, &run, level, part).and_then(|r| refusal_text(&data, r));
                if let Some(r) = item_rect {
                    // The box holds the header, the refusal row and every
                    // ability line.
                    let extra = data.verbs_of(part).len() as f32 * LINE_HEIGHT
                        + if reason.is_some() { ERROR_HEIGHT } else { 0.0 };
                    place_root_grow(&mut ui, r[2] + ROLLOVER_GAP, r[1], extra);
                }
                if let Some(w) = ui.find(id::ROLL_NAME) {
                    let name = if cell_editor::part_is_unlocked(&run, info) {
                        info.name.clone()
                    } else {
                        // ADAPTED: undiscovered parts keep their name hidden.
                        "?".to_string()
                    };
                    ui.set_text(w, name);
                }
                if let Some(w) = ui.find(id::ROLL_PRICE) {
                    ui.set_text(w, format!("{DNA_GLYPH}{}", info.price));
                }
                if let Some(w) = ui.find(id::ROLL_ERROR_ROW) {
                    ui.set_visible(w, reason.is_some());
                }
                if let Some(w) = ui.find(id::ROLL_ERROR) {
                    ui.set_text(w, reason.unwrap_or_default());
                }
            }
            EditorRoot::RolloverLine(i) => {
                let Some(part) = hover else {
                    continue;
                };
                let verbs = data.verbs_of(part);
                let Some(verb) = verbs.get(i) else {
                    continue;
                };
                if let Some(r) = item_rect {
                    // The lines start at the rollover layout's (zero-height)
                    // container 0x04C01A78, (14, 40) from the rollover's
                    // corner, and stack by their authored height; ADAPTED: the
                    // refusal row pushes them down by its height.
                    let refused = cell_editor::refusal(&data, &run, level, part).and_then(|r| refusal_text(&data, r)).is_some();
                    let top = LINES_OFFSET[1] + if refused { ERROR_HEIGHT } else { 0.0 };
                    place_root(&mut ui, r[2] + ROLLOVER_GAP + LINES_OFFSET[0], r[1] + top + i as f32 * LINE_HEIGHT);
                }
                if let Some(w) = ui.find(id::LINE_NAME) {
                    ui.set_text(w, verb.name.clone());
                }
                if let Some(w) = ui.find(id::LINE_VALUE) {
                    ui.set_text(w, "");
                }
                if let Some(w) = ui.find(id::LINE_ICON) {
                    let png = verb_png(index, &mut png_cache, verb.image);
                    if let Some(png) = png {
                        assets.texture(index, &mut images, png);
                    }
                    ui.set_image(w, png.map(image));
                }
            }
            EditorRoot::Hint => {
                // The retail hint bar (`0x67BBF0`): centred on the screen at
                // its authored height, faded by `HintFade`.
                crate::cell_hud::dialogs::bind_hint(&mut ui, layout.hint.as_ref(), editor.hint_alpha, None);
            }
            EditorRoot::Message => {
                let Some(&root) = ui.layout.roots.first() else {
                    continue;
                };
                ui.set_visible(root, message.is_some());
                if let Some(r) = frame_rects.get(&id::HOST_NAME) {
                    // The same layout as the Creature Creator's message: text clear of the icon.
                    crate::creature_editor_ui::place_message(&mut ui, r[0], r[1] - 48.0);
                }
                for c in [id::MESSAGE_TEXT, id::MESSAGE_SHADOW] {
                    for w in ui.find_all(c) {
                        ui.set_visible(w, message.is_some());
                        ui.set_text(w, message.clone().unwrap_or_default());
                    }
                }
            }
            // Driven by `cell_editor_swatch::swatches`.
            EditorRoot::PaintShades | EditorRoot::PaintFlyer => {}
        }
    }
}

/// Why a palette part cannot be taken. The retail check (`0x59E580`, result
/// 2 / 3) stores LocalizedStrings of the editor constants table 0x496BFB26:
/// 0x101 "Too expensive" when the price is over the DNA, 0x102 "Too many
/// parts" when a complexity / part budget slot is exhausted (the 0x201 / 0x202
/// sentences are labelled "(NOT IN USE)" in the table). ADAPTED: the Cell
/// Creator's undiscovered parts (their rollover shows the "?" name) have no
/// decoded retail string, and the part count limit shares the "Too many parts"
/// text with the complexity limit like the retail slots do.
fn refusal_text(data: &CellEditorData, r: Refusal) -> Option<String> {
    Some(match r {
        Refusal::Undiscovered => "Find this part first".to_string(),
        Refusal::NoDna => data.reason_expensive.clone().unwrap_or_else(|| "Too expensive".to_string()),
        Refusal::TooComplex | Refusal::TooManyParts => {
            data.reason_parts.clone().unwrap_or_else(|| "Too many parts".to_string())
        }
        Refusal::Unknown => return None,
    })
}

fn bind_frame(ui: &mut SpuiState, editor: &CellEditor) {
    // ADAPTED: the root's fill (a translucent purple) is a designer backdrop;
    // the retail frames show the scene untinted.
    if let Some(&root) = ui.layout.roots.first() {
        ui.set_tint(root, 0x00FF_FFFF);
    }
    for c in id::RIGHT_CLUSTER_OTHERS {
        for w in ui.find_all(c) {
            ui.set_visible(w, false);
        }
    }
    for (c, on) in [(id::UNDO, !editor.undo.is_empty()), (id::REDO, !editor.redo.is_empty())] {
        if let Some(w) = ui.find(c) {
            ui.set_visible(w, true);
            ui.set_enabled(w, on);
        }
    }
    if let Some(w) = ui.find(id::MODE_PLAY) {
        // No test drive in the Cell Creator: the retail frames show only the
        // empty dark socket (window 0x066FF240) where the play button sits.
        ui.set_visible(w, false);
    }
    if let Some(w) = ui.find(id::MODE_BUILD) {
        ui.set_selected(w, editor.mode == cell_editor::Mode::Build);
    }
    if let Some(w) = ui.find(id::MODE_PAINT) {
        ui.set_selected(w, editor.mode == cell_editor::Mode::Paint);
    }
    for c in [id::CANCEL, id::ACCEPT] {
        if let Some(w) = ui.find(c) {
            ui.set_visible(w, true);
            ui.set_enabled(w, true);
        }
    }
}

/// Buttons of the chrome and presses on palette items.
#[allow(clippy::too_many_arguments)]
pub fn actions(
    mut editor: ResMut<CellEditor>,
    mut editor_state: ResMut<EditorState>,
    data: Res<CellEditorData>,
    mut run: ResMut<RunProgress>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut roots: Query<(&mut SpuiState, &EditorRoot)>,
) {
    let mut clicked = Vec::new();
    for (mut ui, kind) in &mut roots {
        for w in ui.take_clicks() {
            clicked.push((*kind, ui.layout.window(w).control_id));
        }
    }
    for (kind, control) in clicked {
        match (kind, control) {
            (EditorRoot::Frame, id::UNDO) => cell_editor::undo(&mut editor, &mut run),
            (EditorRoot::Frame, id::REDO) => cell_editor::redo(&mut editor, &mut run),
            (EditorRoot::Frame, id::CANCEL) => {
                cell_editor::finish(&mut editor, &mut editor_state, &mut run, false);
                return;
            }
            (EditorRoot::Frame, id::ACCEPT) => {
                if cell_editor::try_save(&mut editor, &mut editor_state, &data, &mut run) {
                    return;
                }
            }
            (EditorRoot::Frame, id::MODE_BUILD) => cell_editor::set_mode(&mut editor, &data, &run, Mode::Build),
            (EditorRoot::Frame, id::MODE_PAINT) => cell_editor::set_mode(&mut editor, &data, &run, Mode::Paint),
            (EditorRoot::PaintPage, theme) => {
                cell_editor::choose_style(&mut editor, &data, &mut run, theme);
            }
            (EditorRoot::Pager, paint_id::PAGE_PREV) => {
                cell_editor_paint::step_page(&mut editor, data.paint.pages.len(), -1)
            }
            (EditorRoot::Pager, paint_id::PAGE_NEXT) => {
                cell_editor_paint::step_page(&mut editor, data.paint.pages.len(), 1)
            }
            (EditorRoot::Name, id::NAME_BUTTON) => editor.naming = true,
            _ => {}
        }
    }
    // A press on a palette item picks the part up (the price is escrowed).
    if mouse.just_pressed(MouseButton::Left) && editor.carry.is_none() && editor.mode == Mode::Build {
        if let Some(part) = editor.hover_item {
            if let Err(r) = cell_editor::take_from_palette(&mut editor, &data, &mut run, part) {
                if let Some(text) = refusal_text(&data, r) {
                    editor.say(text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_collection_stacks_header_and_trays_by_their_authored_heights() {
        // 0x5E8D4E (kJustified): the cursor advances by each tray's original height, empty trays included.
        let slots = tray_slots(47.0, 3);
        assert_eq!(slots, vec![47.0 + 44.0, 47.0 + 44.0 + 35.0, 47.0 + 44.0 + 70.0]);
        // The tray's root is as wide as its gutters (66 + 41) plus the packed cells (21.33 each), right edge on the host's.
        let empty = tray_root_rect(797.0, slots[1], 0, VERB_ICON_ASPECT);
        assert_eq!(empty, [797.0 - 107.0, slots[1], 797.0, slots[1] + 35.0]);
        let three = tray_root_rect(797.0, slots[1], 3, VERB_ICON_ASPECT);
        assert!((three[2] - three[0] - (107.0 + 64.0)).abs() < 1e-3 && three[2] == 797.0);
        // The icon cells then end on the host window's right edge: root right - 41.
        let cells = verb_cells(three[2] - TRAY_ROOT_WIDTH, slots[1], 3, VERB_ICON_ASPECT, true);
        assert!((cells[2][2] - (three[2] - TRAY_GUTTERS[1])).abs() < 1e-3);
        assert!((cells[0][0] - (three[0] + TRAY_GUTTERS[0])).abs() < 1e-3, "and the first starts at the left gutter");
    }

    #[test]
    fn verb_icons_are_packed_cells_against_the_host_edge() {
        // 26 x 39 icons (aspect 2/3) in the 68 x 32 host: cells 21.33 wide, 32 tall.
        let cells = verb_cells(100.0, 50.0, 3, 26.0 / 39.0, true);
        assert_eq!(cells.len(), 3);
        let host_right = 100.0 + 134.0;
        assert!((cells[2][2] - host_right).abs() < 1e-3, "the group ends at the host's right edge");
        for (i, c) in cells.iter().enumerate() {
            assert!((c[2] - c[0] - 64.0 / 3.0).abs() < 1e-3 && (c[3] - c[1] - 32.0).abs() < 1e-3);
            assert_eq!((c[1], c[3]), (52.0, 84.0));
            if i > 0 {
                assert!((c[0] - cells[i - 1][2]).abs() < 1e-3, "no gap between cells");
            }
        }
        // Left packed (justification 0x3EB432) starts at the host's left edge.
        let left = verb_cells(100.0, 50.0, 2, 26.0 / 39.0, false);
        assert!((left[0][0] - 166.0).abs() < 1e-3);
    }

    /// `verbTrayJustification` / layout properties of the installed Cell verb trays
    /// (printed: the default justification is right).
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_verb_trays_justification() {
        use crate::cell_editor_data::{key_list, key_of, prop_key, VERBTRAY_GROUP};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let collection = index
            .properties(prop_key(VERBTRAY_GROUP, crate::world_objects::name_id("CLE_VerbTrayCollection")))
            .unwrap();
        println!(
            "collection: justification {:?} layout {:?}",
            key_of(&collection, 0x04A2_13B2).map(|k| format!("{:08X}", k.instance_id)),
            key_of(&collection, 0x04A2_0FE1).map(|k| format!("{:08X}", k.instance_id)),
        );
        // What `LayoutCollection` (0x5E8A20) reads: justification right (0xE816F049), `verbCollectionLayoutStyle`
        // (0x04B43AA3) kJustified (0xFEA10E68), four trays with the header (group `EditorHeaderTray`) first.
        assert_eq!(key_of(&collection, 0x04A2_13B2).map(|k| k.instance_id), Some(0xE816_F049));
        assert_eq!(key_of(&collection, 0x04B4_3AA3).map(|k| k.instance_id), Some(0xFEA1_0E68));
        let listed = key_list(&collection, 0x04AA_3838);
        assert_eq!(listed.len(), 4, "header tray + three verb trays");
        assert_eq!(listed[0].group_id, crate::world_objects::name_id("EditorHeaderTray"));
        for k in key_list(&collection, 0x04AA_3838) {
            let Ok(tray) = index.properties(prop_key(VERBTRAY_GROUP, k.instance_id)) else {
                continue;
            };
            println!(
                "tray {:08X}: justification {:?} layout {:?} iconLayout {:?}",
                k.instance_id,
                key_of(&tray, 0x04A2_13B2).map(|k| format!("{:08X}", k.instance_id)),
                key_of(&tray, 0x04A2_0FE1).map(|k| format!("{:08X}", k.instance_id)),
                key_of(&tray, 0x04A2_0FE7).map(|k| format!("{:08X}", k.instance_id)),
            );
        }
    }

    #[test]
    fn verb_pictures_are_the_images_of_the_windows_of_the_verbicons_layout() {
        // 0x4592B0 -> 0x8062B0: the picture of a verb image id is the window with that control id in `VerbIcons`.
        let atlas = ResourceKey { type_id: PNG, group_id: 0x31A4_4893, instance_id: 0xE023_3C2C };
        let picture = ImageRef { file: atlas, uv: [0.42, 0.34, 0.57, 0.41], dims: Some([39, 35]) };
        let mut window = plain(
            0,
            None,
            [0.0, 0.0, 39.0, 35.0],
            Some(Drawable::Image(ImageDrawable { image: Some(picture.clone()), ..Default::default() })),
            0xFFFF_FFFF,
        );
        window.control_id = 0xA6A2_963B;
        let layout = Layout {
            version: 0,
            windows: vec![window],
            roots: vec![0],
            hit_masks: vec![],
            warnings: vec![],
            unknown_classes: vec![],
        };
        let mut assets = SpuiAssets::default();
        assets.insert_layout(crate::world_objects::name_id(VERB_ICONS_LAYOUT), Arc::new(layout));
        let index = crate::asset_catalog::ResourceIndex::default();
        let mut cache = HashMap::new();
        assert_eq!(verb_picture(&index, &mut assets, &mut cache, 0xA6A2_963B), Some(picture));
        assert_eq!(verb_picture(&index, &mut assets, &mut cache, 0x1234_5678), None, "an id the layout lacks");
        assert_eq!(verb_picture(&index, &mut assets, &mut cache, 0), None, "no id, no picture");
    }

    #[test]
    fn the_icon_aspect_is_the_layout_roots_width_over_height() {
        let mut layout = Layout {
            version: 0,
            windows: vec![plain(0, None, [115.0, 134.0, 147.0, 166.0], None, 0xFFFF_FFFF)],
            roots: vec![0],
            hit_masks: vec![],
            warnings: vec![],
            unknown_classes: vec![],
        };
        assert_eq!(layout_aspect(&layout), Some(1.0), "the 32 x 32 root of the Cell verbs' icon layouts");
        layout.windows[0].area = [31.0, 20.0, 57.0, 59.0];
        assert!((layout_aspect(&layout).unwrap() - 26.0 / 39.0).abs() < 1e-6, "editorVerbIcon is 26 x 39");
        // A tray of three 32 x 32 icons is 107 + 96 wide.
        let rect = tray_root_rect(797.0, 100.0, 3, 1.0);
        assert!((rect[2] - rect[0] - (107.0 + 96.0)).abs() < 1e-3);
    }

    /// Every id the Cell trays look up resolves through the installed `VerbIcons` layout, the Cell verbs' icon layouts are
    /// the installed 32 x 32 ones, and the ringed discs are the atlas regions of the tray icons.
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_verb_images_resolve_through_the_verbicons_layout() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut assets = SpuiAssets::default();
        let mut cache = HashMap::new();
        let icons = cell_tray_icons(&index);
        assert_eq!(icons.len(), 3, "the three verb trays of the collection");
        for icon in &icons {
            let id = icon.expect("every Cell tray has a verbTrayIconID");
            let picture = verb_picture(&index, &mut assets, &mut cache, id).unwrap_or_else(|| panic!("{id:08X}"));
            // The ringed discs are 39 x 35 atlas regions (the 44 x 44 ones are the diet header's).
            assert_eq!(picture.dims, Some([39, 35]), "{id:08X}");
            assert_eq!(picture.file.instance_id & 0xFFFF_FFFE, 0xE023_3C2C, "the atlas E0233C2C / 2D");
        }
        let data = CellEditorData::load(&index);
        let mut seen = 0;
        for verb in &data.verbs {
            if let Some(tray) = verb.tray_image {
                let picture = verb_picture(&index, &mut assets, &mut cache, tray).unwrap_or_else(|| panic!("{}", verb.name));
                if matches!(verb.name.as_str(), "Herbivore" | "Carnivore" | "Omnivore") {
                    // The diet header's ringed discs (green, red, blue ring).
                    assert_eq!(picture.dims, Some([44, 44]), "{}", verb.name);
                    seen += 1;
                }
            }
        }
        assert!(seen >= 3, "herbivore, carnivore and omnivore verbs carry a tray image");
        let sight = data.verbs.iter().find(|v| v.name == "Sight").expect("the sight verb");
        assert!(verb_picture(&index, &mut assets, &mut cache, sight.image).is_some());
        let mut layouts = HashMap::new();
        for verb in &data.verbs {
            let name = verb_layout_name(&index, &mut assets, &data, verb.category, &mut layouts);
            let layout = assets.layout(&index, &name).unwrap_or_else(|| panic!("{name}"));
            if name != FALLBACK_VERB_LAYOUT {
                assert_eq!(layout_aspect(&layout), Some(1.0), "{} {name}: the Cell verbs' icon layouts are square", verb.name);
            }
        }
        let sight_layout = verb_layout_name(&index, &mut assets, &data, sight.category, &mut layouts);
        assert_eq!(sight_layout, "0xBED39F24", "sight: EditorAbilityVerbIconUI");
    }

    #[test]
    fn items_sit_in_the_cell_centres_at_the_template_width() {
        // The parts page of a 1280 x 900 frame: 447 x 696, three columns by four rows.
        let r = item_area(447.0, 696.0, 3, 4, 1, 1, 0.22, ITEM_ASPECT, PARTS_MAX_HEIGHT);
        let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        assert!((cx - 223.5).abs() < 1e-3 && (cy - 261.0).abs() < 1e-3, "{r:?}");
        assert!((r[2] - r[0] - 0.22 * 447.0).abs() < 1e-3, "width = 22% of the page");
        assert!(((r[3] - r[1]) - (r[2] - r[0])).abs() < 1e-3, "aspect 1");
        // A page whose height limit bites: the item shrinks to the limit.
        let r = item_area(447.0, 696.0, 3, 4, 0, 0, 0.3, 1.0, 0.1);
        assert!((r[3] - r[1] - 69.6).abs() < 1e-3 && (r[2] - r[0] - 69.6).abs() < 1e-3, "{r:?}");
        // The paint template's 20% does not bite at its 30% width.
        let r = item_area(447.0, 696.0, 3, 4, 0, 0, 0.3, ITEM_ASPECT, PAINT_MAX_HEIGHT);
        assert!((r[2] - r[0] - 134.1).abs() < 0.01);
    }
}
