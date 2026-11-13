//! Window rectangle solver: turns the stored window areas into screen
//! rectangles for a given screen size, applying the layout styles
//! (`SimpleLayout`, `ProportionalLayout`) the way the retail UI does.
//!
//! Semantics (ported from SporeModder-FX `WindowBase.handleEvent`,
//! `SimpleLayout`, `ProportionalLayout`; confirmed against the retail frames):
//!
//! 1. A window's rectangle starts as its stored `area`, translated by the
//!    parent's real top-left corner.
//! 2. Its window procedures are applied in stored order. Each layout style adds
//!    to the *current* rectangle using the parent's real size:
//!    * `SimpleLayout` anchors: bit 8 (right) adds the parent width to `x2`
//!      (and to `x1` unless bit 4, left, is also set); bit 2 (bottom) does the
//!      same vertically with bit 1 (top). Stored coordinates of an anchored edge
//!      are therefore offsets from that parent edge (usually negative).
//!    * `ProportionalLayout` adds `round(parent size * proportion)` to each
//!      edge (Java rounding: `floor(x + 0.5)`).

use super::model::{Layout, WinId, WinProc};
use super::ids::layout::{ANCHOR_BOTTOM, ANCHOR_LEFT, ANCHOR_RIGHT, ANCHOR_TOP};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Rect {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }
    pub const fn from_array(a: [f32; 4]) -> Self {
        Self::new(a[0], a[1], a[2], a[3])
    }
    pub const fn to_array(self) -> [f32; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
    pub fn width(&self) -> f32 {
        self.x2 - self.x1
    }
    pub fn height(&self) -> f32 {
        self.y2 - self.y1
    }
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x1 + dx, self.y1 + dy, self.x2 + dx, self.y2 + dy)
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x1 && x <= self.x2 && y >= self.y1 && y <= self.y2
    }
    pub fn center(&self) -> (f32, f32) {
        ((self.x1 + self.x2) * 0.5, (self.y1 + self.y2) * 0.5)
    }
}

/// How the root windows of a layout are placed in the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootPolicy {
    /// The root window fills the screen (HUD layouts: the authored root is an
    /// 800x600-ish design rectangle, but its anchored children hug the real
    /// screen edges in the retail frames).
    Fit,
    /// The authored area is used as is, relative to the screen origin.
    Authored,
    /// The authored size, centred in the screen (dialogs).
    Centered,
}

/// Java `Math.round`.
fn java_round(x: f32) -> f32 {
    (x + 0.5).floor()
}

/// Apply one layout style to `dest` (already translated by the parent origin).
pub fn apply_layout(proc: &WinProc, dest: &mut Rect, parent: &Rect) {
    match proc {
        WinProc::SimpleLayout { anchor } => {
            if anchor & ANCHOR_RIGHT != 0 {
                dest.x2 += parent.width();
                if anchor & ANCHOR_LEFT == 0 {
                    dest.x1 += parent.width();
                }
            }
            if anchor & ANCHOR_BOTTOM != 0 {
                dest.y2 += parent.height();
                if anchor & ANCHOR_TOP == 0 {
                    dest.y1 += parent.height();
                }
            }
        }
        WinProc::ProportionalLayout { proportions } => {
            let (w, h) = (parent.width(), parent.height());
            dest.x1 += java_round(w * proportions[0]);
            dest.y1 += java_round(h * proportions[1]);
            dest.x2 += java_round(w * proportions[2]);
            dest.y2 += java_round(h * proportions[3]);
        }
        _ => {}
    }
}

/// Real (screen space) rectangle of one window given its parent's real
/// rectangle.
pub fn window_rect(layout: &Layout, areas: &[[f32; 4]], id: WinId, parent: &Rect) -> Rect {
    let mut r = Rect::from_array(areas[id]).translated(parent.x1, parent.y1);
    for p in &layout.windows[id].procs {
        apply_layout(p, &mut r, parent);
    }
    r
}

/// Solve every window. `areas` holds the (possibly runtime modified) stored
/// areas, one per window; use [`authored_areas`] for the file values.
pub fn solve(layout: &Layout, areas: &[[f32; 4]], screen: [f32; 2], policy: RootPolicy) -> Vec<Rect> {
    let container = Rect::new(0.0, 0.0, screen[0], screen[1]);
    let mut out = vec![Rect::default(); layout.windows.len()];
    // (window, parent rectangle)
    let mut stack: Vec<(WinId, Rect)> = Vec::new();
    for &root in layout.roots.iter().rev() {
        let r = match policy {
            RootPolicy::Fit => container,
            RootPolicy::Authored => window_rect(layout, areas, root, &container),
            RootPolicy::Centered => {
                let r = window_rect(layout, areas, root, &container);
                let (w, h) = (r.width(), r.height());
                let x = ((screen[0] - w) * 0.5).floor();
                let y = ((screen[1] - h) * 0.5).floor();
                Rect::new(x, y, x + w, y + h)
            }
        };
        out[root] = r;
        for &c in layout.windows[root].children.iter().rev() {
            stack.push((c, r));
        }
        while let Some((id, parent)) = stack.pop() {
            let r = window_rect(layout, areas, id, &parent);
            out[id] = r;
            for &c in layout.windows[id].children.iter().rev() {
                stack.push((c, r));
            }
        }
    }
    out
}

pub fn authored_areas(layout: &Layout) -> Vec<[f32; 4]> {
    layout.windows.iter().map(|w| w.area).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::format::testing::*;
    use crate::spui::format::ty;
    use crate::spui::ids::{class, layout as lid, win};

    fn child(area: [f32; 4], procs: &[i16]) -> El {
        el(class::WINDOW, false, |w| {
            w.prop_rect(win::AREA, area).prop_refs(win::PROCS, procs);
            2
        })
    }
    fn simple(anchor: u8) -> El {
        el(class::SIMPLE_LAYOUT, false, |w| {
            w.head(lid::PARAM, ty::U8, 1).u8(anchor);
            1
        })
    }
    fn proportional(p: [f32; 4]) -> El {
        el(class::PROPORTIONAL_LAYOUT, false, |w| {
            w.head(lid::PARAM, ty::F32, 4);
            for v in p {
                w.f32(v);
            }
            1
        })
    }

    fn layout_of(children: Vec<El>, procs: Vec<El>) -> Layout {
        // elements: 0 root, 1..=n children, then procs (indices from n+1).
        let n = children.len() as i16;
        let root = el(class::WINDOW, true, |w| {
            w.prop_rect(win::AREA, [25.0, 25.0, 871.0, 625.0])
                .prop_refs(win::CHILDREN, &(1..=n).collect::<Vec<_>>());
            2
        });
        let mut all = vec![root];
        all.extend(children);
        all.extend(procs);
        Layout::parse(&container(&[], &[], &[], &all)).unwrap()
    }

    #[test]
    fn anchors_offset_from_parent_edges() {
        // Bottom bar of the Cell HUD: left|right|bottom, area (223,-29,-88,-1).
        // proc element index = 1 (one child) + 1 = 2.
        let layout = layout_of(
            vec![child([223.0, -29.0, -88.0, -1.0], &[2])],
            vec![simple(14)],
        );
        let areas = authored_areas(&layout);
        let r = solve(&layout, &areas, [1280.0, 900.0], RootPolicy::Fit);
        assert_eq!(r[0], Rect::new(0.0, 0.0, 1280.0, 900.0));
        assert_eq!(r[1], Rect::new(223.0, 871.0, 1192.0, 899.0));
        // At the 4:3 design size the same bar sits at the design bottom.
        let r = solve(&layout, &areas, [1024.0, 768.0], RootPolicy::Fit);
        assert_eq!(r[1], Rect::new(223.0, 739.0, 936.0, 767.0));
    }

    #[test]
    fn layout_styles_apply_in_stored_order() {
        // Bottom anchor then proportional 0.5 (callout centred above the bar):
        // area (-141,-178,143,-95) -> x in [W/2-141, W/2+143], y in [H-178, H-95].
        let layout = layout_of(
            vec![child([-141.0, -178.0, 143.0, -95.0], &[2, 3])],
            vec![simple(2), proportional([0.5, 0.0, 0.5, 0.0])],
        );
        let areas = authored_areas(&layout);
        let r = solve(&layout, &areas, [1280.0, 900.0], RootPolicy::Fit);
        assert_eq!(r[1], Rect::new(499.0, 722.0, 783.0, 805.0));
    }

    #[test]
    fn proportional_edges_round_like_java() {
        let mut dest = Rect::new(0.0, 0.0, 0.0, 10.0);
        let parent = Rect::new(0.0, 0.0, 5.0, 5.0);
        // 5 * 0.5 = 2.5 rounds up to 3 (Java), negative halves round up too.
        apply_layout(
            &WinProc::ProportionalLayout {
                proportions: [0.5, 0.0, -0.5, 0.0],
            },
            &mut dest,
            &parent,
        );
        assert_eq!((dest.x1, dest.x2), (3.0, -2.0));
    }

    #[test]
    fn nested_children_follow_their_parent_origin() {
        // parent (anchored bottom) with a left-top anchored child at (10, 5).
        let mut layout = layout_of(
            vec![
                child([0.0, -100.0, 200.0, 0.0], &[3]),
                child([10.0, 5.0, 30.0, 25.0], &[]),
            ],
            vec![simple(2)],
        );
        // Make element 2 a child of element 1: rebuild via the model directly.
        layout.windows[1].children = vec![2];
        layout.windows[2].parent = Some(1);
        layout.windows[0].children = vec![1];
        let areas = authored_areas(&layout);
        let r = solve(&layout, &areas, [800.0, 600.0], RootPolicy::Fit);
        assert_eq!(r[1], Rect::new(0.0, 500.0, 200.0, 600.0));
        assert_eq!(r[2], Rect::new(10.0, 505.0, 30.0, 525.0));
    }

    /// Retail layout check: `GlobalUICell-3` at 1280x900 (ignored: needs the
    /// install). Prints the solved rectangles of the progress bar pieces.
    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn installed_cell_hud_geometry() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let id = crate::world_objects::name_id("GlobalUICell-3");
        let key = index
            .records
            .keys()
            .find(|k| k.type_id == crate::spui::format::SPUI_TYPE && k.instance_id == id)
            .copied()
            .expect("GlobalUICell-3");
        let layout = Layout::parse(&index.read(key).unwrap()).unwrap();
        let areas = authored_areas(&layout);
        let r = solve(&layout, &areas, [1280.0, 900.0], RootPolicy::Fit);
        let rect = |control: u32| r[layout.find(control).unwrap()];
        assert_eq!(rect(0x051B_5478), Rect::new(223.0, 871.0, 1192.0, 899.0), "progress bar strip");
        for control in [0x051B_54C0u32, 0x051B_54C1, 0x051B_54C2, 0x051B_54C3, 0x051B_54C4, 0x051B_5168] {
            let w = rect(control);
            println!("{control:08X}: {:?} ({}x{})", w, w.width(), w.height());
        }
        // Everything visible that covers a point in the bar's light band.
        let covers = |x: f32, y: f32| {
            let mut out = Vec::new();
            for id in layout.preorder() {
                let w = layout.window(id);
                let mut visible = true;
                let mut p = Some(id);
                while let Some(i) = p {
                    visible &= layout.window(i).visible();
                    p = layout.window(i).parent;
                }
                if visible && r[id].contains(x, y) {
                    out.push(format!(
                        "E{} ctrl={:08X} {:?} fill={:08X} drawable={} rect={:?}",
                        w.element,
                        w.control_id,
                        w.kind,
                        w.fill_color,
                        w.drawable.as_ref().map_or("-".to_string(), |d| format!("{d:?}").chars().take(40).collect()),
                        r[id]
                    ));
                }
            }
            out
        };
        for line in covers(600.0, 876.0) {
            println!("covers(600,876): {line}");
        }
        let mate = rect(0x0448_37C0);
        // The Mating Call button sits at the screen centre (+0.5 proportional)
        // of its container, which starts 2 px right of the origin.
        assert_eq!((mate.x1, mate.x2), (615.0, 670.0));
        assert_eq!((mate.y1, mate.y2), (810.0, 865.0));
    }

    /// Developer aid: prints every window of an installed layout with its solved
    /// rectangle. `SPUI_DUMP=<LayoutName> [SPUI_SIZE=1280x900] [SPUI_ROOT=fit|authored|centered]
    /// [SPUI_AT=x,y]` (only windows covering the point). Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "developer aid; needs the Spore install and SPUI_DUMP"]
    fn dump_installed_layout() {
        let Ok(name) = std::env::var("SPUI_DUMP") else {
            return;
        };
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        // A layout is named by its file name, or by a raw instance id (`0x02A6B9AE`)
        // for the many layouts the name registry does not know.
        let id = match name.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(raw) => raw,
            None => crate::world_objects::name_id(&name),
        };
        let key = index
            .records
            .keys()
            .find(|k| k.type_id == crate::spui::format::SPUI_TYPE && k.instance_id == id)
            .copied()
            .expect("layout not found");
        let layout = Layout::parse(&index.read(key).unwrap()).unwrap();
        let size = std::env::var("SPUI_SIZE").unwrap_or_else(|_| "1280x900".into());
        let (w, h) = size.split_once('x').expect("SPUI_SIZE=WxH");
        let screen = [w.parse::<f32>().unwrap(), h.parse::<f32>().unwrap()];
        let policy = match std::env::var("SPUI_ROOT").as_deref() {
            Ok("authored") => RootPolicy::Authored,
            Ok("centered") => RootPolicy::Centered,
            _ => RootPolicy::Fit,
        };
        let at = std::env::var("SPUI_AT").ok().and_then(|s| {
            let (x, y) = s.split_once(',')?;
            Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
        });
        let areas = authored_areas(&layout);
        let r = solve(&layout, &areas, screen, policy);
        let sheet = index
            .records
            .keys()
            .find(|k| {
                k.type_id == crate::spui::style::STYLE_SHEET_TYPE
                    && k.group_id == crate::spui::style::STYLE_SHEET_GROUP
                    && k.instance_id == crate::spui::style::STYLE_SHEET_INSTANCE
            })
            .copied()
            .and_then(|k| index.read(k).ok())
            .map(|b| crate::spui::style::StyleSheet::parse(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        for id in layout.preorder() {
            let win = layout.window(id);
            if let Some((x, y)) = at {
                if !r[id].contains(x, y) {
                    continue;
                }
            }
            let mut depth = 0;
            let mut p = win.parent;
            while let Some(i) = p {
                depth += 1;
                p = layout.window(i).parent;
            }
            let procs: Vec<String> = win
                .procs
                .iter()
                .map(|p| match p {
                    crate::spui::model::WinProc::SimpleLayout { anchor } => format!("S{anchor}"),
                    crate::spui::model::WinProc::ProportionalLayout { proportions } => format!("P{proportions:?}"),
                    crate::spui::model::WinProc::Effect { kind, bi } => {
                        format!("Fx{{{:?} trig={:X} t={}}}", kind, bi.trigger, bi.time)
                    }
                    crate::spui::model::WinProc::Event(e) => format!("Ev{{msg={:X} trig={}}}", e.message, e.trigger),
                    crate::spui::model::WinProc::Tooltip {
                        layout,
                        control,
                        detail_control,
                        text,
                        detail,
                        offset,
                        behavior,
                    } => format!(
                        "Tip{{layout={layout:?} ctrl={control:X}/{detail_control:X} text={text:?} detail={detail:?} off={offset:?} beh={behavior}}}"
                    ),
                    crate::spui::model::WinProc::Other(c) => format!("O{c:X}"),
                })
                .collect();
            println!(
                "{}E{} {:?} ctrl={:08X} cmd={:08X} fl={:05X} rect=({:.0},{:.0},{:.0},{:.0}) fill={:08X} shade={:08X} extra={} drw={} procs={:?}",
                "  ".repeat(depth),
                win.element,
                win.kind,
                win.control_id,
                win.command_id,
                win.flags,
                r[id].x1,
                r[id].y1,
                r[id].x2,
                r[id].y2,
                win.fill_color,
                win.shade_color,
                {
                    let mut s = match &win.extra {
                        crate::spui::model::Extra::None => String::new(),
                        e => format!("{e:?}").chars().take(300).collect(),
                    };
                    if matches!(win.kind, crate::spui::model::WindowKind::Text | crate::spui::model::WindowKind::Button)
                        || win.caption.is_some()
                    {
                        let st = sheet.style(win.text_style);
                        s.push_str(&format!(
                            " ts={:08X}({} {} {}px bold={} {:?}/{:?}) caption={:?}",
                            win.text_style, st.name, st.family, st.size_px, st.bold, st.align, st.valign, win.caption
                        ));
                    }
                    s
                },
                win.drawable.as_ref().map_or("-".to_string(), |d| {
                    let n = std::env::var("SPUI_DRW").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
                    format!("{d:?}").chars().take(n).collect()
                }),
                procs
            );
        }
    }

    #[test]
    fn root_policies() {
        let layout = layout_of(vec![], vec![]);
        let areas = authored_areas(&layout);
        let screen = [1280.0, 900.0];
        assert_eq!(
            solve(&layout, &areas, screen, RootPolicy::Fit)[0],
            Rect::new(0.0, 0.0, 1280.0, 900.0)
        );
        assert_eq!(
            solve(&layout, &areas, screen, RootPolicy::Authored)[0],
            Rect::new(25.0, 25.0, 871.0, 625.0)
        );
        let c = solve(&layout, &areas, screen, RootPolicy::Centered)[0];
        assert_eq!((c.width(), c.height()), (846.0, 600.0));
        assert_eq!((c.x1, c.y1), (217.0, 150.0));
    }
}
