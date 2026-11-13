//! Tooltips of SPUI windows.
//!
//! A window with a `Tooltip` window procedure shows a small bubble when the
//! pointer rests on it. The procedure names the layout of the bubble (the
//! installed `tooltips` layout: one text window on a rounded dark plate that
//! pops in with an `Inflate` effect), the text window inside it, the string
//! (usually a localized one from the `tooltips` table), a detailed string with
//! its own control, an offset and a behaviour. The game code needs to do
//! nothing: every spawned layout with such windows gets its tooltips.
//!
//! VERIFIED (`SporeTooltipWinProc::HandleUIMessage` 0x835B30, the manager it
//! reaches through `[0x164F328] - 8`, march2017 image):
//! * Timing. The proc's hover state follows the mouse refresh message (case
//!   0x835B97): entering the window stores the time in the manager (+0x24),
//!   leaving hides the bubble and stores the hide time (+0x28). The update
//!   message (case 0x835C1C) asks `ShouldShow` 0x8354D0: the bubble is not up
//!   (+0x2C), the proc's window is not the one it was last shown for / hidden
//!   by a click or the timeout (+0x10), and the pointer has been in the window
//!   longer than `TooltipHoverTime` (0x164F44C, registered by the static
//!   initializer 0x12FF780 with the code default 0.5 s; the installed UI tuning
//!   property file `0!5C770DB7` sets `TooltipHoverTime` 0.2, the other three
//!   unchanged) or a bubble was shown (+0x2D, "recent") and hidden
//!   less than `TooltipFastSwitchTime` (0.25 s, 0x164F45C, 0x12FF720) ago: then
//!   the next window's bubble comes up at once. A detailed string (+0x34) takes
//!   over after `TooltipShowDetailTime` (3 s, 0x164F47C, 0x12FF7E0; 0x835550);
//!   the bubble hides after `TooltipTimeoutTime` (20 s, 0x164F46C, 0x12FF6C0;
//!   0x835520) and stays hidden until the pointer leaves and re-enters. A mouse
//!   press (message 6, case 0x835B68) hides it the same way and clears "recent",
//!   so the next window waits the full delay.
//! * Placement (manager `Show` 0x8357C0, behaviour = the proc's field +0x5C):
//!   Default (0) and Offset (2) put the bubble above the owner window with a
//!   5 pixel gap (`0x808BD0`, constant 0x14852F8), below it when there is no
//!   room above, left edges aligned (right edges when it would leave the
//!   screen); Offset adds the proc's offset to that. UnderWindow (1) centres
//!   the bubble on the window horizontally (0x835A0B). The result is then kept
//!   on the screen by 0x835600 (right overflow shifts left, bottom overflow
//!   moves it up by its height). The cursor is not an anchor: the (0, 30)
//!   offset of the constructor 0x835DA0 only matters to behaviour 2.
//!
//! ADAPTED: the bubble is sized to its text (text bounds plus the window's text
//! border); retail sizes the layout window itself (0x835580, a 800 x 600
//! maximum).

use bevy::prelude::*;

use super::{
    ids::flag,
    model::{WinId, WinProc},
    solve::RootPolicy,
    view::{self, SpuiAssets, SpuiPointer, SpuiState},
};

/// `TooltipHoverTime`: seconds the pointer must be in a window before its
/// tooltip appears (VERIFIED: 0x164F44C, the installed tuning file
/// `0!5C770DB7` says 0.2; the code default is 0.5).
pub const DELAY: f32 = 0.2;
/// `TooltipFastSwitchTime`: a bubble hidden this recently makes the next
/// window's come up at once (0x164F45C).
pub const FAST_SWITCH: f32 = 0.25;
/// `TooltipShowDetailTime`: the detailed string replaces the text (0x164F47C).
pub const SHOW_DETAIL: f32 = 3.0;
/// `TooltipTimeoutTime`: the bubble hides by itself (0x164F46C).
pub const TIMEOUT: f32 = 20.0;
/// Gap between the owner window and the bubble (0x14852F8).
const GAP: f32 = 5.0;
/// Layout and text window of the standard bubble (used when the procedure
/// names none).
pub const DEFAULT_LAYOUT: &str = "tooltips";
pub const DEFAULT_CONTROL: u32 = 0x0375_4E6C;
/// Width given to the text window while its text is measured.
const MEASURE_WIDTH: f32 = 640.0;

/// The spawned bubble layout.
#[derive(Component)]
pub struct TooltipRoot {
    pub layout: String,
}

/// The manager's state (`[0x164F328] - 8`: hover time +0x24, hide time +0x28,
/// shown +0x2C, recent +0x2D, detail +0x2E, the window it was last shown for
/// +0x10) and the proc's hover state (+0x58), over any window key.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine<K: Copy + PartialEq> {
    /// The window under the pointer (the proc's hover state).
    target: Option<K>,
    /// The window the bubble was shown for, or was hidden for by a press or the
    /// timeout; forgotten when the pointer leaves a window.
    current: Option<K>,
    hover_start: f32,
    hidden_at: f32,
    shown: bool,
    recent: bool,
    detail_shown: bool,
    was_pressed: bool,
}

impl<K: Copy + PartialEq> Default for Machine<K> {
    fn default() -> Self {
        Self {
            target: None,
            current: None,
            hover_start: 0.0,
            hidden_at: 0.0,
            shown: false,
            recent: false,
            detail_shown: false,
            was_pressed: false,
        }
    }
}

impl<K: Copy + PartialEq> Machine<K> {
    /// `Show(show = false, window)` 0x8357C0: a hide while nothing is up clears
    /// "recent"; the window becomes the current one.
    fn hide(&mut self, window: Option<K>) {
        if !self.shown {
            self.recent = false;
        }
        self.shown = false;
        self.detail_shown = false;
        self.current = window;
    }

    /// One frame: the window under the pointer (if it has a tooltip), whether
    /// it has a detailed string, the pointer button. `now` is the window
    /// manager's clock in seconds.
    pub fn step(&mut self, now: f32, target: Option<K>, has_detail: bool, pressed: bool) {
        if target != self.target {
            // Refresh message, the pointer left the old window: hide, stamp the time.
            if self.target.is_some() {
                self.hide(None);
                self.hidden_at = now;
            }
            // ... and entered the new one.
            if target.is_some() {
                self.hover_start = now;
            }
            self.target = target;
        }
        // Mouse down on the window (0x835B68): hide until the pointer leaves it.
        if pressed && !self.was_pressed && self.target.is_some() {
            self.hide(self.target);
            self.recent = false;
        }
        self.was_pressed = pressed;
        let Some(window) = self.target else { return };
        // Update message (0x835C1C).
        let hovered = now - self.hover_start;
        let fast = self.recent && now - self.hidden_at < FAST_SWITCH;
        if !self.shown && self.current != Some(window) && (hovered > DELAY || fast) {
            self.recent = true;
            self.shown = true;
            self.detail_shown = false;
            self.current = Some(window);
        } else if has_detail && self.shown && !self.detail_shown && hovered > SHOW_DETAIL {
            self.detail_shown = true;
        } else if self.shown && hovered > TIMEOUT {
            self.hide(Some(window));
            self.recent = false;
        }
    }

    /// A bubble is up for the hovered window.
    pub fn up(&self) -> bool {
        self.shown && self.target.is_some()
    }

    /// The detailed string is showing.
    pub fn detail(&self) -> bool {
        self.detail_shown
    }
}

/// What the pointer rests on and what the bubble currently shows.
#[derive(Resource, Default)]
pub struct TooltipState {
    /// The retail manager: the window under the pointer is (layout root, window).
    machine: Machine<(Entity, WinId)>,
    /// Text the bubble is sized and placed for.
    measured: Option<String>,
    /// Text being measured and the frames it has been laid out for (the text
    /// bounds of a frame belong to the text set a few frames earlier).
    measuring: Option<String>,
    measure_frames: u32,
    /// The bubble layout and control window last shown.
    shown_control: Option<(String, u32)>,
}

/// The tooltip data of a window: layout, control, text and offset.
pub struct TooltipSpec {
    pub layout: String,
    pub control: u32,
    pub text: String,
    pub offset: [f32; 2],
    /// `TooltipBehaviour`: 0 Default, 1 UnderWindow, 2 Offset.
    pub behavior: u32,
    /// The detailed string and the control that shows it (after
    /// [`SHOW_DETAIL`] seconds).
    pub detail: Option<String>,
    pub detail_control: u32,
}

/// Topmost window under `p` that has a tooltip (children before their
/// parent, front-most sibling first, like the paint order reversed).
pub fn tooltip_window_at(state: &SpuiState, live: &[bool], p: Vec2) -> Option<WinId> {
    fn walk(state: &SpuiState, live: &[bool], id: WinId, p: Vec2) -> Option<WinId> {
        if !live[id] {
            return None;
        }
        let w = &state.layout.windows[id];
        if state.flags[id] & flag::IGNORE_MOUSE_CHILDREN == 0 {
            for &c in &w.children {
                if let Some(hit) = walk(state, live, c, p) {
                    return Some(hit);
                }
            }
        }
        let has_tooltip =
            w.procs.iter().any(|p| matches!(p, WinProc::Tooltip { .. })) || state.tooltip_overrides[id].is_some();
        (has_tooltip && state.rects[id].contains(p.x, p.y)).then_some(id)
    }
    state.layout.roots.iter().find_map(|&root| walk(state, live, root, p))
}

/// Resolve the tooltip of a window (`None` when it has no usable text).
pub fn spec_of(
    state: &SpuiState,
    id: WinId,
    assets: &mut SpuiAssets,
    index: &crate::asset_catalog::ResourceIndex,
) -> Option<TooltipSpec> {
    if let Some(text) = state.tooltip_overrides[id].as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        // `CreateTooltip` of the ModAPI: the constructor's defaults (offset (0, 30), Default behaviour).
        return Some(TooltipSpec {
            layout: DEFAULT_LAYOUT.to_string(),
            control: DEFAULT_CONTROL,
            text: text.to_string(),
            offset: [0.0, 30.0],
            behavior: 0,
            detail: None,
            detail_control: DEFAULT_CONTROL,
        });
    }
    state.layout.windows[id].procs.iter().find_map(|p| match p {
        WinProc::Tooltip {
            layout,
            control,
            detail_control,
            text,
            detail,
            offset,
            behavior,
        } => {
            let text = assets.text(index, text.as_ref()?);
            let text = text.trim().to_string();
            let detail = detail
                .as_ref()
                .map(|d| assets.text(index, d).trim().to_string())
                .filter(|d| !d.is_empty());
            (!text.is_empty()).then(|| TooltipSpec {
                // Layout names hash case-insensitively ("Tooltips" is `tooltips`).
                layout: if layout.is_empty() { DEFAULT_LAYOUT.to_string() } else { layout.to_ascii_lowercase() },
                control: if *control == 0 { DEFAULT_CONTROL } else { *control },
                text,
                offset: *offset,
                behavior: *behavior,
                detail,
                detail_control: if *detail_control == 0 { DEFAULT_CONTROL } else { *detail_control },
            })
        }
        _ => None,
    })
}

/// Where the bubble goes (manager `Show` 0x8357C0): placed against the owner
/// window's screen rect `anchor` (x1, y1, x2, y2) by the proc's behaviour, then
/// kept on the screen (0x835600).
pub fn place(anchor: [f32; 4], behavior: u32, offset: [f32; 2], size: Vec2, screen: Vec2) -> Vec2 {
    let [ax1, ay1, ax2, ay2] = anchor;
    let mut at = if behavior == 1 {
        // UnderWindow (0x835A0B): centred on the window horizontally; the
        // vertical term is the code's, centre of the window + half the bubble.
        Vec2::new((ax1 + ax2) * 0.5 - size.x * 0.5, (ay1 + ay2) * 0.5 + size.y * 0.5)
    } else {
        // 0x808BD0 with the flag clear.
        let x = if size.x + ax1 > screen.x { ax2.min(screen.x) - size.x } else { ax1.max(0.0) };
        let above = ay1 - size.y - GAP;
        let y = if above >= 0.0 { above } else { ay2 + GAP };
        Vec2::new(x, y)
    };
    if behavior == 2 {
        at += Vec2::from(offset);
    }
    // 0x835600.
    let (left, top) = (at.x, at.y);
    let mut x = left.max(0.0);
    let mut y = top.max(0.0);
    if left + size.x > screen.x {
        x = left - (left + size.x - screen.x);
    }
    if top + size.y > screen.y {
        y = top - size.y;
    }
    Vec2::new(x, y)
}

/// Track the pointer, spawn the bubble layout on demand and fill it.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time<Real>>,
    pointer: Res<SpuiPointer>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut tip: ResMut<TooltipState>,
    camera: Query<Entity, With<crate::GameCamera>>,
    owners: Query<(Entity, &SpuiState, Option<&GlobalZIndex>, Option<&InheritedVisibility>), Without<TooltipRoot>>,
    mut bubbles: Query<(Entity, &mut SpuiState, &TooltipRoot)>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    let index = &originals.resource_index;
    // The pointer rests on the front-most layout window with a tooltip.
    let mut found: Option<(Entity, WinId, TooltipSpec, [f32; 4])> = None;
    if let Some(p) = pointer.position {
        let mut layouts: Vec<_> = owners
            .iter()
            .filter(|(_, s, _, vis)| s.ready && vis.is_none_or(|v| v.get()))
            .collect();
        layouts.sort_by_key(|(_, _, z, _)| std::cmp::Reverse(z.map_or(0, |z| z.0)));
        for (entity, state, _, _) in layouts {
            let live = state.interactive();
            if let Some(id) = tooltip_window_at(state, &live, p) {
                if let Some(spec) = spec_of(state, id, &mut assets, index) {
                    found = Some((entity, id, spec, state.rects[id].to_array()));
                    break;
                }
            }
        }
    }
    let target = found.as_ref().map(|(e, id, _, _)| (*e, *id));
    let now = time.elapsed_secs();
    let has_detail = found.as_ref().is_some_and(|(_, _, s, _)| s.detail.is_some());
    let was_up = tip.machine.up();
    tip.machine.step(now, target, has_detail, pointer.pressed);
    if was_up != tip.machine.up() {
        // Shown or hidden: the bubble is measured and placed again.
        tip.measured = None;
        tip.measuring = None;
    }
    let want = found.as_ref().filter(|_| tip.machine.up() && pointer.position.is_some());

    let Some((_, _, spec, anchor)) = want else {
        for (_, mut ui, root) in &mut bubbles {
            if let Some(w) = bubble_window(&ui, &root.layout) {
                ui.set_visible(w, false);
            }
        }
        tip.measured = None;
        tip.measuring = None;
        return;
    };
    // The detailed string takes over the bubble (its own control).
    let detail = tip.machine.detail().then(|| spec.detail.clone()).flatten();
    let (control, text) = match detail {
        Some(d) => (spec.detail_control, d),
        None => (spec.control, spec.text.clone()),
    };
    // The bubble layout of this tooltip (spawned the first time it is needed).
    if !bubbles.iter().any(|(_, _, r)| r.layout == spec.layout) {
        let Ok(camera) = camera.single() else {
            return;
        };
        if let Some(layout) = assets.layout(index, &spec.layout) {
            let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 300);
            // Not `StageOwned`: the root is the one bubble layer of the whole game and lives on across
            // stages (a stage change would otherwise leave `TooltipState`'s measured text for a layout
            // that is gone), and the menu stage's `menu_presentation` hides every `StageOwned` root that is
            // not the galaxy scene, which kept the bubble from ever painting there.
            commands.entity(root).insert(TooltipRoot {
                layout: spec.layout.clone(),
            });
        }
        return;
    }
    // A control other than the last one shown (the detail window) hides that one.
    let shown_now = Some((spec.layout.clone(), control));
    if tip.shown_control != shown_now {
        if let Some((layout, old)) = tip.shown_control.take() {
            for (_, mut ui, root) in &mut bubbles {
                if root.layout == layout {
                    if let Some(w) = ui.find(old) {
                        ui.set_visible(w, false);
                    }
                }
            }
        }
        tip.shown_control = shown_now;
        tip.measured = None;
        tip.measuring = None;
    }
    for (_, mut ui, root) in &mut bubbles {
        if root.layout != spec.layout {
            if let Some(w) = bubble_window(&ui, &root.layout) {
                ui.set_visible(w, false);
            }
            continue;
        }
        let Some(window) = ui.find(control) else {
            continue;
        };
        if !ui.ready {
            continue;
        }
        if tip.measured.as_deref() == Some(text.as_str()) {
            ui.set_visible(window, true);
            continue;
        }
        if tip.measuring.as_deref() != Some(text.as_str()) {
            // Start measuring: a wide, transparent window holding the text.
            tip.measuring = Some(text.clone());
            tip.measure_frames = 0;
            ui.set_text(window, text.clone());
            ui.text_extents[window] = None;
            ui.set_area(window, [0.0, 0.0, MEASURE_WIDTH, 64.0]);
            ui.set_shade(window, 0x00FF_FFFF);
            ui.set_visible(window, true);
            continue;
        }
        tip.measure_frames += 1;
        if tip.measure_frames < 3 {
            continue;
        }
        if let Some(extent) = ui.text_extent(window).filter(|e| e.x > 0.0) {
            let border = ui.layout.window(window).text().map_or([0.0; 4], |t| t.border);
            let size = Vec2::new(
                extent.x.ceil() + 1.0 + border[0] + border[2],
                extent.y.ceil() + border[1] + border[3],
            );
            let screen = Vec2::new(ui.screen[0], ui.screen[1]);
            let at = place(*anchor, spec.behavior, spec.offset, size, screen);
            ui.set_area(window, [at.x, at.y, at.x + size.x, at.y + size.y]);
            ui.set_shade(window, 0xFFFF_FFFF);
            tip.measured = Some(text.clone());
        }
    }
}

/// The text window of a spawned bubble layout.
fn bubble_window(ui: &SpuiState, layout: &str) -> Option<WinId> {
    let control = if layout == DEFAULT_LAYOUT { DEFAULT_CONTROL } else { 0 };
    ui.find(control).or_else(|| ui.layout.roots.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = Vec2::new(1280.0, 900.0);

    #[test]
    fn bubble_sits_above_its_window_with_a_five_pixel_gap() {
        // A button at (100, 400) - (140, 440), an 80 x 22 bubble: above, left edges aligned.
        let at = place([100.0, 400.0, 140.0, 440.0], 0, [0.0, 30.0], Vec2::new(80.0, 22.0), SCREEN);
        assert_eq!(at, Vec2::new(100.0, 400.0 - 22.0 - GAP));
    }

    #[test]
    fn bubble_goes_below_when_there_is_no_room_above() {
        let at = place([100.0, 10.0, 140.0, 50.0], 0, [0.0, 30.0], Vec2::new(80.0, 22.0), SCREEN);
        assert_eq!(at, Vec2::new(100.0, 50.0 + GAP));
    }

    #[test]
    fn bubble_is_kept_on_screen() {
        // Wider than what is left on the right: right edges line up with the window's.
        let at = place([1250.0, 400.0, 1270.0, 420.0], 0, [0.0, 30.0], Vec2::new(100.0, 22.0), SCREEN);
        assert_eq!(at.x, 1270.0 - 100.0);
        // A window past the right screen edge: the bubble ends at the screen edge.
        let at = place([1270.0, 400.0, 1400.0, 420.0], 0, [0.0, 30.0], Vec2::new(100.0, 22.0), SCREEN);
        assert_eq!(at.x, 1280.0 - 100.0);
        // Never left of the screen.
        let at = place([-30.0, 400.0, -10.0, 420.0], 0, [0.0, 30.0], Vec2::new(10.0, 10.0), SCREEN);
        assert_eq!(at.x, 0.0);
        // Below the window and past the bottom edge: the final clamp moves it up by its height.
        let at = place([100.0, 10.0, 140.0, 890.0], 0, [0.0, 30.0], Vec2::new(80.0, 22.0), SCREEN);
        assert_eq!(at.y, 895.0 - 22.0);
    }

    #[test]
    fn offset_and_under_window_behaviours() {
        let size = Vec2::new(80.0, 22.0);
        // Offset (2): the default place plus the offset.
        let at = place([100.0, 400.0, 140.0, 440.0], 2, [4.0, 30.0], size, SCREEN);
        assert_eq!(at, Vec2::new(104.0, 400.0 - 22.0 - GAP + 30.0));
        // UnderWindow (1): centred horizontally.
        let at = place([100.0, 400.0, 140.0, 440.0], 1, [0.0, 0.0], size, SCREEN);
        assert_eq!(at.x, 120.0 - 40.0);
    }

    #[test]
    fn the_bubble_follows_the_retail_delays() {
        let mut m = Machine::<u32>::default();
        // Enter window 1 at t = 1: nothing until the hover time has passed.
        m.step(1.0, Some(1), false, false);
        assert!(!m.up());
        m.step(1.0 + DELAY - 0.01, Some(1), false, false);
        assert!(!m.up());
        m.step(1.0 + DELAY + 0.01, Some(1), false, false);
        assert!(m.up());
        // Entering another window right away shows its bubble at once (a recent bubble, hidden under the fast switch time ago).
        m.step(2.0, Some(2), false, false);
        assert!(m.up(), "recent bubble: the next window needs no delay");
        // A pause longer than the fast switch time: the full delay again.
        m.step(3.0, None, false, false);
        assert!(!m.up());
        m.step(3.0 + FAST_SWITCH + 0.05, Some(3), false, false);
        assert!(!m.up());
        m.step(3.0 + FAST_SWITCH + 0.05 + DELAY + 0.01, Some(3), false, false);
        assert!(m.up());
    }

    #[test]
    fn a_press_hides_the_bubble_until_the_pointer_leaves() {
        let mut m = Machine::<u32>::default();
        m.step(0.0, Some(1), false, false);
        m.step(DELAY + 0.1, Some(1), false, false);
        assert!(m.up());
        m.step(DELAY + 0.2, Some(1), false, true);
        assert!(!m.up());
        m.step(5.0, Some(1), false, false);
        assert!(!m.up(), "it stays hidden over the same window");
        // A press clears "recent": the next window waits the whole delay.
        m.step(5.1, Some(2), false, false);
        assert!(!m.up());
        m.step(5.1 + DELAY + 0.01, Some(2), false, false);
        assert!(m.up());
        // Leaving the window the press hid and coming back shows it again.
        m.step(6.0, None, false, false);
        m.step(7.0, Some(1), false, false);
        m.step(7.0 + DELAY + 0.01, Some(1), false, false);
        assert!(m.up());
    }

    #[test]
    fn the_detail_takes_over_and_the_bubble_times_out() {
        let mut m = Machine::<u32>::default();
        m.step(0.0, Some(1), true, false);
        m.step(DELAY + 0.01, Some(1), true, false);
        assert!(m.up() && !m.detail());
        m.step(SHOW_DETAIL + 0.01, Some(1), true, false);
        assert!(m.up() && m.detail());
        m.step(TIMEOUT + 0.01, Some(1), true, false);
        assert!(!m.up(), "twenty seconds hide it");
        m.step(TIMEOUT + 5.0, Some(1), true, false);
        assert!(!m.up(), "and it stays hidden until the pointer leaves");
    }
}
