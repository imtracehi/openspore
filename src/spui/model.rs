//! Typed SPUI layout: windows, drawables, window procedures.
//!
//! [`Layout::from_raw`] interprets the generic property bag of
//! [`super::format`] the way the retail UI classes (and SporeModder-FX) do.
//! The window tree keeps the file order of `Children`: the **first child is the
//! front-most** window, the last is painted first.
//!
//! Colours are `0xAARRGGBB`. Areas are `[x1, y1, x2, y2]` exactly as stored:
//! they are relative to the parent window and, for edges that a layout style
//! anchors, relative to the anchored parent edge (see [`super::solve`]).

use std::collections::HashMap;

use super::format::{parse_raw, PropList, RawFile, RawHitMask, SpuiError, Target, Text};
use super::ids::{self, class, drawable as dr, event as ev, win};
use super::outline::{self, Outline};
use super::schema;
use crate::dbpf::ResourceKey;

pub type WinId = usize;

/// Default text style id (`DefaultStyle` in the style sheet).
pub const DEFAULT_STYLE: u32 = 0x00AE_BB69;
pub const WHITE: u32 = 0xFFFF_FFFF;
pub const DEFAULT_FILL: u32 = 0xFF7F_7F7F;

/// What the window is, as far as painting and input are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// Plain window (also the base of progress bars and similar custom windows).
    Window,
    Button,
    Text,
    AnimatedIcon,
    ProgressBar,
    GradientProgressBar,
    Slider,
    Scrollbar,
    Spinner,
    ComboBox,
    TextEdit,
    Dialog,
    MessageBox,
    Grid,
    TreeView,
    /// Any other window class (kept so the tree stays intact).
    Other,
}

/// A reference to part of an image file: either a region of an atlas or a
/// whole image.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub file: ResourceKey,
    /// Normalised source rectangle `[x1, y1, x2, y2]` inside the file.
    pub uv: [f32; 4],
    /// Authored size in pixels (`Dimensions` of an atlas image). `None` for a
    /// whole-file image, whose size is the texture size.
    pub dims: Option<[u32; 2]>,
}

/// `SporeStdDrawableImageInfo`: background plus icon of one button state.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub background: Option<ImageRef>,
    pub background_color: u32,
    pub icon: Option<ImageRef>,
    pub icon_color: u32,
    /// 0 = image size, 1 = window size.
    pub icon_draw_mode: u32,
    pub icon_scale: [f32; 2],
    pub icon_offset: [f32; 2],
    pub background_scale: [f32; 2],
    pub background_offset: [f32; 2],
    /// `ShadowModes` (`outline::SHADOW_*`): which of background / icon get the stroke.
    pub stroke_mode: u32,
    /// `ShadowModes`: which of background / icon get the halo.
    pub halo_mode: u32,
    /// `mStrokeShadow` (+0x48): drawn right behind the layer, see [`Outline::taps`].
    pub stroke: Outline,
    /// `mHaloShadow` (+0x70): drawn behind the stroke.
    pub halo: Outline,
}

impl Default for ImageInfo {
    fn default() -> Self {
        Self {
            background: None,
            background_color: WHITE,
            icon: None,
            icon_color: WHITE,
            icon_draw_mode: 1,
            icon_scale: [1.0, 1.0],
            icon_offset: [0.0, 0.0],
            background_scale: [1.0, 1.0],
            background_offset: [0.0, 0.0],
            // Constructor 0x8308F0: both modes start at `Full`; the outlines start with a zero
            // size, so nothing is drawn until the file gives a size.
            stroke_mode: outline::SHADOW_FULL,
            halo_mode: outline::SHADOW_FULL,
            stroke: Outline::default(),
            halo: Outline::default(),
        }
    }
}

pub const SCALE_STRETCH_IMAGE: u32 = 1;
pub const SCALE_STRETCH_CENTER: u32 = 2;
pub const SCALE_TILE_IMAGE: u32 = 3;
pub const SCALE_TILE_CENTER: u32 = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct StdDrawable {
    /// One image per state: normal, disabled, highlighted, active, then the
    /// four checked variants.
    pub images: [Option<ImageRef>; 8],
    /// Index into [`Layout::hit_masks`].
    pub hit_mask: Option<usize>,
    pub scale_type: u32,
    pub scale_factor: [f32; 2],
    /// Left, top, right, bottom non-stretched border as a fraction of the
    /// image size.
    pub scale_area: [f32; 4],
    /// "Image Outline" (0x039A69E6), drawn around the state image by `StdDrawable::Paint`
    /// 0x987C70.
    pub outline: Option<Outline>,
}

impl Default for StdDrawable {
    fn default() -> Self {
        Self {
            images: Default::default(),
            hit_mask: None,
            scale_type: SCALE_STRETCH_IMAGE,
            scale_factor: [1.0, 1.0],
            scale_area: [0.333, 0.333, 0.333, 0.333],
            outline: None,
        }
    }
}

pub const TILING_NONE: u32 = 0;
pub const TILING_STANDARD: u32 = 1;
pub const TILING_EDGE: u32 = 2;
pub const IMAGE_FIXED_WIDTH: u32 = 1;
pub const IMAGE_FIXED_HEIGHT: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct ImageDrawable {
    pub image: Option<ImageRef>,
    /// 1 left, 2 right, 3 centre.
    pub align_h: u32,
    /// 1 top, 2 bottom, 3 middle.
    pub align_v: u32,
    pub flags: u32,
    pub tiling: u32,
    pub scale: f32,
    /// "Image Outline" (0x039A69E6), drawn around the image by `ImageDrawable::Paint` 0x97D1E0.
    pub outline: Option<Outline>,
}

impl Default for ImageDrawable {
    fn default() -> Self {
        Self {
            image: None,
            align_h: 3,
            align_v: 3,
            flags: 0,
            tiling: TILING_NONE,
            scale: 1.0,
            outline: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Drawable {
    Image(ImageDrawable),
    Std(StdDrawable),
    SporeStd {
        base: StdDrawable,
        infos: Box<[Option<ImageInfo>; 8]>,
    },
    /// Four-state strip (disabled | normal | highlighted | depressed order of
    /// the original; see the renderer for the tile mapping).
    ButtonStandard {
        image: Option<ImageRef>,
        tileable: bool,
    },
    ButtonRadio {
        image: Option<ImageRef>,
        tileable: bool,
    },
    VariableWidth {
        image: Option<ImageRef>,
        fill_color: u32,
    },
    /// Controls drawn from a fixed list of images (slider, scrollbar, spinner,
    /// combo box, dialog).
    ImageSet {
        class: u32,
        images: Vec<Option<ImageRef>>,
    },
    Frame,
    Other(u32),
}

/// Window procedure shared fields of the bistate effects.
#[derive(Debug, Clone, PartialEq)]
pub struct BiState {
    pub time: f32,
    pub trigger: u32,
    pub interpolation: u32,
    pub ease_in: f32,
    pub ease_out: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectKind {
    Fade,
    Glide { offset: [f32; 2] },
    Inflate { scale: f32 },
    Modulate { color1: u32, color2: u32 },
    Rotate { axis: [f32; 3], angle: f32 },
    Perspective { near: f32 },
    Cascade { delay: f32, interval: f32 },
    Material { from: String, to: String, recursive: bool },
    Swarm { name: String },
    Sound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeBase {
    pub negated: bool,
    pub duration: f32,
    pub pre_delay: f32,
    pub post_delay: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimeFunction {
    Ramp(TimeBase),
    SmoothRamp {
        base: TimeBase,
        smooth_in: f32,
        smooth_out: f32,
    },
    Oscillator {
        base: TimeBase,
        kind: u32,
        frequency: f32,
        damping: f32,
        ceiling: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Interpolator {
    Scale {
        from_window: bool,
        from: f32,
        to: f32,
    },
    Color {
        from_window: bool,
        from: u32,
        to: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub negate: bool,
    pub combine: u32,
    pub query: u32,
    pub parameter: u32,
    pub window_id: u32,
    pub window_is: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Animate {
        predicates: Vec<Predicate>,
        time: Option<TimeFunction>,
        value: Option<Interpolator>,
    },
    SetState {
        predicates: Vec<Predicate>,
        state: u32,
        parameter: u32,
        window_id: u32,
        window_is: u32,
    },
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateEvent {
    pub message: u32,
    pub broadcast: bool,
    pub window_id: u32,
    /// 0 off, 1 on, 2 off->on, 3 on->off.
    pub trigger: u32,
    pub actions: Vec<Action>,
    pub predicates: Vec<Predicate>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WinProc {
    /// Anchors: top 1, bottom 2, left 4, right 8.
    SimpleLayout { anchor: u32 },
    /// Left, top, right, bottom fractions of the parent size.
    ProportionalLayout { proportions: [f32; 4] },
    Effect { bi: BiState, kind: EffectKind },
    Event(StateEvent),
    Tooltip {
        layout: String,
        control: u32,
        detail_control: u32,
        text: Option<Text>,
        detail: Option<Text>,
        offset: [f32; 2],
        behavior: u32,
    },
    /// Anything the engine does not model (navigators, layer ids, ...).
    Other(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonData {
    pub flags: u32,
    pub state_flags: u32,
    /// 1 standard, 2 toggle, 3 radio.
    pub button_type: u32,
    pub group_id: u32,
    pub alignment: u32,
    pub caption_border: [f32; 4],
    pub caption_offset: [f32; 2],
    /// Caption colours per state (enabled, disabled, depressed, highlighted,
    /// selected, selected highlighted, selected depressed, active).
    pub caption_colors: [u32; 8],
    /// "Text Outline" (0x039A69E6): the dark outline / glow around the caption.
    pub outline: Option<Outline>,
}

impl Default for ButtonData {
    fn default() -> Self {
        Self {
            flags: 0,
            state_flags: 0,
            button_type: 1,
            group_id: 0,
            alignment: 3,
            caption_border: [0.0; 4],
            caption_offset: [0.0; 2],
            caption_colors: [0xFF00_0000; 8],
            outline: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextData {
    pub color: u32,
    pub border: [f32; 4],
    /// "Text Outline" (0x039A69E6): the dark outline / glow around the text.
    pub outline: Option<Outline>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimData {
    pub image: Option<ImageRef>,
    pub duration: f32,
    /// 0 forward, 1 backward.
    pub direction: u32,
    /// 0 loop, 1 ping-pong, 2 stop, 3 loop count.
    pub end_mode: u32,
    pub loop_count: u32,
    pub auto_play: bool,
    pub frame_count: u32,
    pub rows: u32,
    pub columns: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RangeData {
    pub value: i32,
    pub min: i32,
    pub max: i32,
    /// 1 horizontal, 2 vertical.
    pub orientation: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Extra {
    #[default]
    None,
    Button(ButtonData),
    Text(TextData),
    Anim(AnimData),
    Range(RangeData),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: WinId,
    pub element: usize,
    pub class: u32,
    pub kind: WindowKind,
    pub parent: Option<WinId>,
    /// Front-most child first.
    pub children: Vec<WinId>,
    pub flags: u32,
    pub control_id: u32,
    pub command_id: u32,
    pub area: [f32; 4],
    pub caption: Option<Text>,
    pub fill_color: u32,
    pub shade_color: u32,
    pub text_style: u32,
    pub drawable: Option<Drawable>,
    pub procs: Vec<WinProc>,
    pub extra: Extra,
}

impl Window {
    pub fn visible(&self) -> bool {
        self.flags & ids::flag::VISIBLE != 0
    }
    pub fn enabled(&self) -> bool {
        self.flags & ids::flag::ENABLED != 0
    }
    /// Only generic windows paint their fill colour.
    pub fn paints_fill_color(&self) -> bool {
        self.class == class::WINDOW
    }
    pub fn button(&self) -> Option<&ButtonData> {
        match &self.extra {
            Extra::Button(b) => Some(b),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<&TextData> {
        match &self.extra {
            Extra::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn anim(&self) -> Option<&AnimData> {
        match &self.extra {
            Extra::Anim(a) => Some(a),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitMask {
    Rle {
        width: u32,
        height: u32,
        /// `(first, last)` pairs of linear pixel indices (inclusive).
        runs: Vec<u16>,
    },
    Image(ResourceKey),
}

impl HitMask {
    /// Whether pixel `(x, y)` of the mask is covered. Out of range is a miss.
    pub fn covers(&self, x: i32, y: i32) -> bool {
        match self {
            HitMask::Rle { width, height, runs } => {
                if x < 0 || y < 0 || x as u32 >= *width || y as u32 >= *height {
                    return false;
                }
                let index = y as u32 * *width + x as u32;
                // Runs are ascending; find the last run starting at or before it.
                let pairs = runs.len() / 2;
                let (mut lo, mut hi) = (0usize, pairs);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if (runs[mid * 2] as u32) <= index {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                lo > 0 && index <= runs[(lo - 1) * 2 + 1] as u32
            }
            // Image masks need the decoded pixels; callers treat them as a
            // plain rectangle.
            HitMask::Image(_) => true,
        }
    }

    pub fn size(&self) -> Option<[u32; 2]> {
        match self {
            HitMask::Rle { width, height, .. } => Some([*width, *height]),
            HitMask::Image(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub version: u16,
    pub windows: Vec<Window>,
    /// Root windows in file order.
    pub roots: Vec<WinId>,
    pub hit_masks: Vec<HitMask>,
    /// Non-fatal problems met while interpreting the file.
    pub warnings: Vec<String>,
    /// Class ids the model did not recognise (anywhere in the file).
    pub unknown_classes: Vec<u32>,
}

impl Layout {
    pub fn parse(bytes: &[u8]) -> Result<Layout, SpuiError> {
        Ok(Layout::from_raw(&parse_raw(bytes)?))
    }

    pub fn window(&self, id: WinId) -> &Window {
        &self.windows[id]
    }

    /// Every window with this control id (ids are not guaranteed unique).
    pub fn find_all(&self, control_id: u32) -> Vec<WinId> {
        self.windows
            .iter()
            .filter(|w| w.control_id == control_id)
            .map(|w| w.id)
            .collect()
    }

    pub fn find(&self, control_id: u32) -> Option<WinId> {
        self.windows
            .iter()
            .find(|w| w.control_id == control_id)
            .map(|w| w.id)
    }

    /// Window ids in paint-independent pre-order (parents before children).
    pub fn preorder(&self) -> Vec<WinId> {
        let mut out = Vec::with_capacity(self.windows.len());
        let mut stack: Vec<WinId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.windows[id].children.iter().rev().copied());
        }
        out
    }

    pub fn from_raw(raw: &RawFile) -> Layout {
        let mut b = Builder {
            raw,
            out: Layout {
                version: raw.version,
                ..Default::default()
            },
            built: HashMap::new(),
            visiting: Vec::new(),
        };
        b.out.hit_masks = raw
            .masks
            .iter()
            .map(|m| match m {
                RawHitMask::Rle {
                    width,
                    height,
                    runs,
                } => HitMask::Rle {
                    width: *width,
                    height: *height,
                    runs: runs.clone(),
                },
                RawHitMask::Image(k) => HitMask::Image(*k),
            })
            .collect();
        for root in raw.roots().collect::<Vec<_>>() {
            if let Some(id) = b.window(root, None) {
                b.out.roots.push(id);
            }
        }
        for e in &raw.elements {
            if schema::class_by_proxy(e.class).is_none() && !b.out.unknown_classes.contains(&e.class)
            {
                b.out.unknown_classes.push(e.class);
            }
        }
        b.out
    }
}

/// Classify a window class through the schema's inheritance chain.
pub fn kind_of(class_id: u32) -> Option<WindowKind> {
    let kind = match class_id {
        class::WINDOW => WindowKind::Window,
        class::BUTTON | class::POPUP_MENU | class::POPUP_MENU_ITEM => WindowKind::Button,
        class::TEXT | class::OUTLINED_TEXT => WindowKind::Text,
        class::ANIMATED_ICON => WindowKind::AnimatedIcon,
        class::PROGRESS_BAR => WindowKind::ProgressBar,
        class::GRADIENT_PROGRESS_BAR => WindowKind::GradientProgressBar,
        class::SLIDER => WindowKind::Slider,
        class::SCROLLBAR => WindowKind::Scrollbar,
        class::SPINNER => WindowKind::Spinner,
        class::COMBO_BOX => WindowKind::ComboBox,
        class::TEXT_EDIT => WindowKind::TextEdit,
        class::DIALOG => WindowKind::Dialog,
        class::MESSAGE_BOX => WindowKind::MessageBox,
        class::GRID => WindowKind::Grid,
        class::TREE_VIEW => WindowKind::TreeView,
        _ => {
            let def = schema::class_by_proxy(class_id)?;
            if !def.is_a("WindowBase") {
                return None;
            }
            if def.is_a("Button") {
                WindowKind::Button
            } else if def.is_a("Text") {
                WindowKind::Text
            } else if def.is_a("Window") {
                WindowKind::Window
            } else {
                WindowKind::Other
            }
        }
    };
    Some(kind)
}

struct Builder<'a> {
    raw: &'a RawFile,
    out: Layout,
    built: HashMap<usize, WinId>,
    visiting: Vec<usize>,
}

fn point_f(p: [i32; 2]) -> [f32; 2] {
    [p[0] as f32, p[1] as f32]
}

impl Builder<'_> {
    fn warn(&mut self, message: String) {
        if self.out.warnings.len() < 200 {
            self.out.warnings.push(message);
        }
    }

    fn borders(&self, props: &[crate::spui::format::Prop], id: u32) -> Option<[f32; 4]> {
        let s = props.structs(id).first()?;
        Some([
            s.f32_at(ids::borders::LEFT, 0).unwrap_or(0.0),
            s.f32_at(ids::borders::TOP, 0).unwrap_or(0.0),
            s.f32_at(ids::borders::RIGHT, 0).unwrap_or(0.0),
            s.f32_at(ids::borders::BOTTOM, 0).unwrap_or(0.0),
        ])
    }

    /// `image` typed reference: whole image file or an atlas region.
    fn image(&mut self, index: i16) -> Option<ImageRef> {
        match self.raw.resolve(index) {
            Target::Null | Target::HitMask(_) => None,
            Target::Image(i) => Some(whole(self.raw.images[i])),
            Target::AtlasFile(i) => Some(whole(self.raw.atlases[i])),
            Target::Element(e) => {
                let el = &self.raw.elements[e];
                if el.class != class::ATLAS_IMAGE {
                    self.warn(format!("image reference to non-image element {e}"));
                    return None;
                }
                let file = match self.raw.resolve(el.reference(dr::ATLAS_FILE, 0)?) {
                    Target::Image(i) => self.raw.images[i],
                    Target::AtlasFile(i) => self.raw.atlases[i],
                    _ => return None,
                };
                let uv = el.rect(dr::ATLAS_UV).unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let dims = el
                    .point(dr::ATLAS_DIMENSIONS)
                    .map(|d| [d[0].max(0) as u32, d[1].max(0) as u32]);
                Some(ImageRef { file, uv, dims })
            }
        }
    }

    fn images<const N: usize>(&mut self, refs: &[i16]) -> [Option<ImageRef>; N] {
        let mut out: [Option<ImageRef>; N] = std::array::from_fn(|_| None);
        for (slot, r) in out.iter_mut().zip(refs) {
            *slot = self.image(*r);
        }
        out
    }

    fn hit_mask(&self, index: Option<i16>) -> Option<usize> {
        match self.raw.resolve(index?) {
            Target::HitMask(i) => Some(i),
            _ => None,
        }
    }

    fn std_base(&mut self, e: usize) -> StdDrawable {
        let el = &self.raw.elements[e];
        let refs: Vec<i16> = el.refs(dr::IMAGES).to_vec();
        let scale_area = self
            .borders(&el.props, dr::SCALE_AREA)
            .unwrap_or([0.333, 0.333, 0.333, 0.333]);
        let scale_type = el.u32_at(dr::SCALE_TYPE, 0).unwrap_or(SCALE_STRETCH_IMAGE);
        let scale_factor = el.vec2(dr::SCALE_FACTOR).unwrap_or([1.0, 1.0]);
        let hit_mask = self.hit_mask(el.reference(dr::HIT_MASK, 0));
        StdDrawable {
            images: self.images::<8>(&refs),
            hit_mask,
            scale_type,
            scale_factor,
            scale_area,
            outline: Outline::find(&el.props, outline::PROP_OUTLINE),
        }
    }

    fn image_info(&mut self, e: usize) -> ImageInfo {
        let el = &self.raw.elements[e];
        let background = el.reference(dr::INFO_BACKGROUND, 0);
        let icon = el.reference(dr::INFO_ICON, 0);
        let d = ImageInfo::default();
        let info = ImageInfo {
            background_color: el.u32_at(dr::INFO_BACKGROUND_COLOR, 0).unwrap_or(d.background_color),
            icon_color: el.u32_at(dr::INFO_ICON_COLOR, 0).unwrap_or(d.icon_color),
            icon_draw_mode: el.u32_at(dr::INFO_ICON_MODE, 0).unwrap_or(d.icon_draw_mode),
            icon_scale: el.vec2(dr::INFO_ICON_SCALE).unwrap_or(d.icon_scale),
            icon_offset: el.vec2(dr::INFO_ICON_OFFSET).unwrap_or(d.icon_offset),
            background_scale: el
                .vec2(dr::INFO_BACKGROUND_SCALE)
                .unwrap_or(d.background_scale),
            background_offset: el
                .vec2(dr::INFO_BACKGROUND_OFFSET)
                .unwrap_or(d.background_offset),
            stroke_mode: el.u32_at(dr::INFO_STROKE_MODE, 0).unwrap_or(d.stroke_mode),
            halo_mode: el.u32_at(dr::INFO_HALO_MODE, 0).unwrap_or(d.halo_mode),
            stroke: Outline::find(&el.props, dr::INFO_STROKE_SHADOW).unwrap_or(d.stroke),
            halo: Outline::find(&el.props, dr::INFO_HALO_SHADOW).unwrap_or(d.halo),
            ..d
        };
        ImageInfo {
            background: background.and_then(|r| self.image(r)),
            icon: icon.and_then(|r| self.image(r)),
            ..info
        }
    }

    fn drawable(&mut self, index: i16) -> Option<Drawable> {
        let Target::Element(e) = self.raw.resolve(index) else {
            return None;
        };
        let el = &self.raw.elements[e];
        let class_id = el.class;
        Some(match class_id {
            class::IMAGE_DRAWABLE => {
                let image = el.reference(dr::IMAGE, 0);
                let d = ImageDrawable::default();
                let mut out = ImageDrawable {
                    align_h: el.u32_at(dr::ALIGN_H, 0).unwrap_or(d.align_h),
                    align_v: el.u32_at(dr::ALIGN_V, 0).unwrap_or(d.align_v),
                    flags: el.u32_at(dr::IMAGE_FLAGS, 0).unwrap_or(0),
                    tiling: el.u32_at(dr::TILING, 0).unwrap_or(0),
                    scale: el.f32_at(dr::SCALE, 0).unwrap_or(1.0),
                    image: None,
                    outline: Outline::find(&el.props, outline::PROP_OUTLINE),
                };
                out.image = image.and_then(|r| self.image(r));
                Drawable::Image(out)
            }
            class::STD_DRAWABLE => Drawable::Std(self.std_base(e)),
            class::SPORE_STD_DRAWABLE => {
                let base = self.std_base(e);
                let el = &self.raw.elements[e];
                let refs: Vec<Option<i16>> = dr::SPORE_STATES
                    .iter()
                    .map(|id| el.reference(*id, 0))
                    .collect();
                let mut infos: [Option<ImageInfo>; 8] = Default::default();
                for (slot, r) in infos.iter_mut().zip(refs) {
                    if let Some(Target::Element(i)) = r.map(|r| self.raw.resolve(r)) {
                        if self.raw.elements[i].class == class::SPORE_STD_IMAGE_INFO {
                            *slot = Some(self.image_info(i));
                        }
                    }
                }
                Drawable::SporeStd {
                    base,
                    infos: Box::new(infos),
                }
            }
            class::BUTTON_DRAWABLE_STANDARD | class::BUTTON_DRAWABLE_RADIO => {
                let image = el.reference(dr::BUTTON_IMAGE, 0);
                let tileable = el.bool_at(dr::BUTTON_TILEABLE, 0).unwrap_or(false);
                let image = image.and_then(|r| self.image(r));
                if class_id == class::BUTTON_DRAWABLE_STANDARD {
                    Drawable::ButtonStandard { image, tileable }
                } else {
                    Drawable::ButtonRadio { image, tileable }
                }
            }
            class::VARIABLE_WIDTH_DRAWABLE => {
                let image = el.reference(dr::VARWIDTH_IMAGE, 0);
                let fill_color = el.u32_at(dr::VARWIDTH_FILL, 0).unwrap_or(DEFAULT_FILL);
                Drawable::VariableWidth {
                    image: image.and_then(|r| self.image(r)),
                    fill_color,
                }
            }
            class::COMBO_BOX_DRAWABLE
            | class::SCROLLBAR_DRAWABLE
            | class::SLIDER_DRAWABLE
            | class::SPINNER_DRAWABLE
            | class::DIALOG_DRAWABLE => {
                let id = match class_id {
                    class::COMBO_BOX_DRAWABLE => dr::COMBO_IMAGES,
                    class::SCROLLBAR_DRAWABLE => dr::SCROLLBAR_IMAGES,
                    class::SLIDER_DRAWABLE => dr::SLIDER_IMAGES,
                    class::SPINNER_DRAWABLE => dr::SPINNER_IMAGES,
                    _ => dr::DIALOG_IMAGES,
                };
                let refs: Vec<i16> = el.refs(id).to_vec();
                let images = refs.iter().map(|r| self.image(*r)).collect();
                Drawable::ImageSet {
                    class: class_id,
                    images,
                }
            }
            class::FRAME_DRAWABLE => Drawable::Frame,
            other => Drawable::Other(other),
        })
    }

    fn time_base(&self, props: &[crate::spui::format::Prop]) -> TimeBase {
        TimeBase {
            negated: props.bool_at(ev::TIME_NEGATED, 0).unwrap_or(false),
            duration: props.f32_at(ev::TIME_DURATION, 0).unwrap_or(1.0),
            pre_delay: props.f32_at(ev::TIME_PRE_DELAY, 0).unwrap_or(0.0),
            post_delay: props.f32_at(ev::TIME_POST_DELAY, 0).unwrap_or(0.0),
        }
    }

    fn time_function(&self, index: Option<i16>) -> Option<TimeFunction> {
        let Target::Element(e) = self.raw.resolve(index?) else {
            return None;
        };
        let el = &self.raw.elements[e];
        let base = self.time_base(&el.props);
        Some(match el.class {
            class::TIME_RAMP => TimeFunction::Ramp(base),
            class::TIME_SMOOTH_RAMP => TimeFunction::SmoothRamp {
                base,
                smooth_in: el.f32_at(ev::SMOOTH_IN, 0).unwrap_or(0.5),
                smooth_out: el.f32_at(ev::SMOOTH_OUT, 0).unwrap_or(0.5),
            },
            class::TIME_OSCILLATOR => TimeFunction::Oscillator {
                base,
                kind: el.u32_at(ev::OSC_TYPE, 0).unwrap_or(1),
                frequency: el.f32_at(ev::OSC_FREQUENCY, 0).unwrap_or(2.0),
                damping: el.f32_at(ev::OSC_DAMPING, 0).unwrap_or(0.0),
                ceiling: el.bool_at(ev::OSC_CEILING, 0).unwrap_or(false),
            },
            _ => return None,
        })
    }

    fn interpolator(&self, index: Option<i16>) -> Option<Interpolator> {
        let Target::Element(e) = self.raw.resolve(index?) else {
            return None;
        };
        let el = &self.raw.elements[e];
        let from_window = el.bool_at(ev::FROM_WINDOW, 0).unwrap_or(true);
        Some(match el.class {
            class::WINDOW_SCALE => Interpolator::Scale {
                from_window,
                from: el.f32_at(ev::FROM_VALUE, 0).unwrap_or(1.0),
                to: el.f32_at(ev::TO_VALUE, 0).unwrap_or(1.0),
            },
            class::WINDOW_COLOR => Interpolator::Color {
                from_window,
                from: el.u32_at(ev::FROM_VALUE, 0).unwrap_or(WHITE),
                to: el.u32_at(ev::TO_VALUE, 0).unwrap_or(0x00FF_FFFF),
            },
            _ => return None,
        })
    }

    fn predicates(&self, props: &[crate::spui::format::Prop]) -> Vec<Predicate> {
        ev::PREDICATES
            .iter()
            .filter_map(|id| props.reference(*id, 0))
            .filter_map(|r| match self.raw.resolve(r) {
                Target::Element(e) => Some(&self.raw.elements[e]),
                _ => None,
            })
            .filter(|el| el.class == class::WINDOW_STATE_PREDICATE)
            .map(|el| Predicate {
                negate: el.bool_at(ev::NEGATE, 0).unwrap_or(false),
                combine: el.u32_at(ev::COMBINE, 0).unwrap_or(1),
                query: el.u32_at(ev::QUERY, 0).unwrap_or(1),
                parameter: el.u32_at(ev::QUERY_PARAMETER, 0).unwrap_or(0x7FFF_FFFF),
                window_id: el.u32_at(ev::QUERY_WINDOW_ID, 0).unwrap_or(0x7FFF_FFFF),
                window_is: el.u32_at(ev::QUERY_WINDOW_IS, 0).unwrap_or(0),
            })
            .collect()
    }

    fn action(&self, e: usize) -> Action {
        let el = &self.raw.elements[e];
        let predicates = self.predicates(&el.props);
        match el.class {
            class::WINDOW_ANIMATION_ACTION => Action::Animate {
                predicates,
                time: self.time_function(el.reference(ev::TIME_FUNCTION, 0)),
                value: self.interpolator(el.reference(ev::ANIMATED_VALUE, 0)),
            },
            class::WINDOW_STATE_ACTION => Action::SetState {
                predicates,
                state: el.u32_at(ev::STATE_ACTION, 0).unwrap_or(0),
                parameter: el.u32_at(ev::STATE_PARAMETER, 0).unwrap_or(0x7FFF_FFFF),
                window_id: el.u32_at(ev::STATE_WINDOW_ID, 0).unwrap_or(0x7FFF_FFFF),
                window_is: el.u32_at(ev::STATE_WINDOW_IS, 0).unwrap_or(0),
            },
            other => Action::Unknown(other),
        }
    }

    fn bistate(&self, el: &crate::spui::format::RawElement) -> BiState {
        BiState {
            time: el.f32_at(ids::effect::TIME, 0).unwrap_or(0.0),
            trigger: el.u32_at(ids::effect::TRIGGER, 0).unwrap_or(0),
            interpolation: el.u32_at(ids::effect::INTERPOLATION, 0).unwrap_or(0),
            ease_in: el.f32_at(ids::effect::EASE_IN, 0).unwrap_or(0.0),
            ease_out: el.f32_at(ids::effect::EASE_OUT, 0).unwrap_or(0.0),
            damping: el.f32_at(ids::effect::DAMPING, 0).unwrap_or(0.1),
        }
    }

    fn text_string(&self, el: &crate::spui::format::RawElement, id: u32) -> String {
        match el.text(id) {
            Some(Text::Literal(s)) => s.clone(),
            _ => String::new(),
        }
    }

    fn win_proc(&mut self, e: usize) -> WinProc {
        use ids::effect as fx;
        let el = &self.raw.elements[e];
        match el.class {
            class::SIMPLE_LAYOUT => WinProc::SimpleLayout {
                anchor: el.u32_at(ids::layout::PARAM, 0).unwrap_or(ids::layout::ANCHOR_LEFT | ids::layout::ANCHOR_TOP),
            },
            class::PROPORTIONAL_LAYOUT => {
                let mut p = [0.0f32; 4];
                for (i, slot) in p.iter_mut().enumerate() {
                    *slot = el.f32_at(ids::layout::PARAM, i).unwrap_or(0.0);
                }
                WinProc::ProportionalLayout { proportions: p }
            }
            class::FADE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Fade,
            },
            class::GLIDE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Glide {
                    offset: el.vec2(fx::PARAM0).unwrap_or([0.0, 0.0]),
                },
            },
            class::INFLATE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Inflate {
                    scale: el.f32_at(fx::PARAM0, 0).unwrap_or(1.0),
                },
            },
            class::MODULATE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Modulate {
                    color1: el.u32_at(fx::PARAM0, 0).unwrap_or(WHITE),
                    color2: el.u32_at(fx::PARAM1, 0).unwrap_or(WHITE),
                },
            },
            class::ROTATE | class::SPORE_ROTATE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Rotate {
                    axis: [
                        el.f32_at(fx::ROT_X, 0).unwrap_or(0.0),
                        el.f32_at(fx::ROT_Y, 0).unwrap_or(0.0),
                        el.f32_at(fx::ROT_Z, 0).unwrap_or(1.0),
                    ],
                    angle: el.f32_at(fx::ROT_ANGLE, 0).unwrap_or(0.0),
                },
            },
            class::PERSPECTIVE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Perspective {
                    near: el.f32_at(fx::NEAR_PLANE, 0).unwrap_or(0.0),
                },
            },
            class::CASCADE => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Cascade {
                    delay: el.f32_at(fx::CASCADE_DELAY, 0).unwrap_or(0.0),
                    interval: el.f32_at(fx::CASCADE_INTERVAL, 0).unwrap_or(0.0),
                },
            },
            class::MATERIAL_EFFECT => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Material {
                    from: self.text_string(el, fx::FROM_MATERIAL),
                    to: self.text_string(el, fx::TO_MATERIAL),
                    recursive: el.bool_at(fx::MATERIAL_RECURSIVE, 0).unwrap_or(false),
                },
            },
            class::SWARM_EFFECT => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Swarm {
                    name: self.text_string(el, fx::SWARM_NAME),
                },
            },
            class::SOUND_EFFECT => WinProc::Effect {
                bi: self.bistate(el),
                kind: EffectKind::Sound,
            },
            class::TOOLTIP => WinProc::Tooltip {
                layout: self.text_string(el, ids::tooltip::LAYOUT),
                control: el.u32_at(ids::tooltip::CONTROL, 0).unwrap_or(0),
                detail_control: el.u32_at(ids::tooltip::DETAIL_CONTROL, 0).unwrap_or(0),
                text: el.text(ids::tooltip::STRING).cloned(),
                detail: el.text(ids::tooltip::DETAIL_STRING).cloned(),
                offset: el.vec2(ids::tooltip::OFFSET).unwrap_or([0.0, 30.0]),
                behavior: el.u32_at(ids::tooltip::BEHAVIOR, 0).unwrap_or(0),
            },
            class::WINDOW_STATE_EVENT => {
                let actions = ev::ACTIONS
                    .iter()
                    .filter_map(|id| el.reference(*id, 0))
                    .filter_map(|r| match self.raw.resolve(r) {
                        Target::Element(i) => Some(i),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                WinProc::Event(StateEvent {
                    message: el.u32_at(ev::MESSAGE, 0).unwrap_or(ev::MSG_VISIBLE),
                    broadcast: el.bool_at(ev::BROADCAST, 0).unwrap_or(true),
                    window_id: el.u32_at(ev::WINDOW_ID, 0).unwrap_or(0x7FFF_FFFF),
                    trigger: el.u32_at(ev::TRIGGER, 0).unwrap_or(2),
                    actions: actions.into_iter().map(|i| self.action(i)).collect(),
                    predicates: self.predicates(&el.props),
                })
            }
            other => WinProc::Other(other),
        }
    }

    /// Build (or fetch) the window for an element.
    fn window(&mut self, e: usize, parent: Option<WinId>) -> Option<WinId> {
        // A window still being built is its own ancestor: reject the cycle.
        if self.visiting.contains(&e) {
            self.warn(format!("window element {e} is its own ancestor"));
            return None;
        }
        if let Some(id) = self.built.get(&e) {
            return Some(*id);
        }
        let class_id = self.raw.elements[e].class;
        let Some(kind) = kind_of(class_id) else {
            self.warn(format!(
                "element {e} ({}) is not a window",
                schema::class_name(class_id)
            ));
            return None;
        };
        self.visiting.push(e);
        let id = self.out.windows.len();
        self.built.insert(e, id);

        let el = &self.raw.elements[e];
        let default_flags = match kind {
            WindowKind::Button
            | WindowKind::Text
            | WindowKind::ComboBox
            | WindowKind::TextEdit
            | WindowKind::Grid
            | WindowKind::TreeView => 0x0B,
            _ => 0x03,
        };
        let flags = el.u32_at(win::FLAGS, 0).unwrap_or(default_flags);
        let area = el.rect(win::AREA).unwrap_or([10.0, 10.0, 50.0, 50.0]);
        let extra = match kind {
            WindowKind::Button => {
                let d = ButtonData::default();
                let mut colors = d.caption_colors;
                if let Some(v) = el.get(ids::button::CAPTION_COLORS) {
                    for (i, c) in colors.iter_mut().enumerate() {
                        if let Some(x) = v.int_at(i) {
                            *c = x as u32;
                        }
                    }
                }
                Extra::Button(ButtonData {
                    flags: el.u32_at(ids::button::FLAGS, 0).unwrap_or(0),
                    state_flags: el.u32_at(ids::button::STATE_FLAGS, 0).unwrap_or(0),
                    button_type: el.u32_at(ids::button::TYPE, 0).unwrap_or(1),
                    group_id: el.u32_at(ids::button::GROUP_ID, 0).unwrap_or(0),
                    alignment: el.u32_at(ids::button::ALIGNMENT, 0).unwrap_or(3),
                    caption_border: self
                        .borders(&el.props, ids::button::CAPTION_BORDER)
                        .unwrap_or([0.0; 4]),
                    caption_offset: el.vec2(ids::button::CAPTION_OFFSET).unwrap_or([0.0; 2]),
                    caption_colors: colors,
                    outline: Outline::find(&el.props, outline::PROP_OUTLINE),
                })
            }
            WindowKind::Text => Extra::Text(TextData {
                color: el.u32_at(ids::text::COLOR, 0).unwrap_or(WHITE),
                border: self
                    .borders(&el.props, ids::text::BORDER)
                    .unwrap_or([0.0; 4]),
                outline: Outline::find(&el.props, outline::PROP_OUTLINE),
            }),
            // TextEdit "Text Colors" (0x0FF11731, the first is the normal
            // text) and "BorderWidth" (0x0FF11732, default 4).
            WindowKind::TextEdit => Extra::Text(TextData {
                color: el.u32_at(0x0FF1_1731, 0).unwrap_or(WHITE),
                border: self.borders(&el.props, 0x0FF1_1732).unwrap_or([4.0; 4]),
                outline: Outline::find(&el.props, outline::PROP_OUTLINE),
            }),
            WindowKind::AnimatedIcon => {
                let image = el.reference(ids::anim_icon::IMAGE, 0);
                let a = AnimData {
                    image: None,
                    duration: el.f32_at(ids::anim_icon::DURATION, 0).unwrap_or(1.0),
                    direction: el.u32_at(ids::anim_icon::DIRECTION, 0).unwrap_or(0),
                    end_mode: el.u32_at(ids::anim_icon::END_MODE, 0).unwrap_or(0),
                    loop_count: el.u32_at(ids::anim_icon::LOOP_COUNT, 0).unwrap_or(1),
                    auto_play: el.bool_at(ids::anim_icon::AUTO_PLAY, 0).unwrap_or(false),
                    frame_count: el.u32_at(ids::anim_icon::FRAME_COUNT, 0).unwrap_or(0),
                    rows: el.u32_at(ids::anim_icon::ROWS, 0).unwrap_or(1),
                    columns: el.u32_at(ids::anim_icon::COLUMNS, 0).unwrap_or(1),
                };
                Extra::Anim(AnimData {
                    image: image.and_then(|r| self.image(r)),
                    ..a
                })
            }
            WindowKind::Slider | WindowKind::Scrollbar => {
                let (v, lo, hi, o) = if kind == WindowKind::Slider {
                    (
                        ids::slider::VALUE,
                        ids::slider::MIN,
                        ids::slider::MAX,
                        ids::slider::ORIENTATION,
                    )
                } else {
                    (
                        ids::scrollbar::VALUE,
                        ids::scrollbar::MIN,
                        ids::scrollbar::MAX,
                        ids::scrollbar::ORIENTATION,
                    )
                };
                Extra::Range(RangeData {
                    value: el.i32_at(v, 0).unwrap_or(0),
                    min: el.i32_at(lo, 0).unwrap_or(0),
                    max: el.i32_at(hi, 0).unwrap_or(0),
                    orientation: el.u32_at(o, 0).unwrap_or(1),
                })
            }
            _ => Extra::None,
        };
        let drawable_ref = el.reference(win::DRAWABLE, 0);
        let proc_refs: Vec<i16> = el.refs(win::PROCS).to_vec();
        let child_refs: Vec<i16> = el.refs(win::CHILDREN).to_vec();
        let window = Window {
            id,
            element: e,
            class: class_id,
            kind,
            parent,
            children: Vec::new(),
            flags,
            control_id: el.u32_at(win::CONTROL_ID, 0).unwrap_or(0),
            command_id: el.u32_at(win::COMMAND_ID, 0).unwrap_or(0),
            area,
            caption: el.text(win::CAPTION).cloned().filter(|t| !t.is_empty()),
            fill_color: el.u32_at(win::FILL_COLOR, 0).unwrap_or(DEFAULT_FILL),
            shade_color: el.u32_at(win::SHADE_COLOR, 0).unwrap_or(WHITE),
            text_style: el.u32_at(win::TEXT_FONT, 0).unwrap_or(DEFAULT_STYLE),
            drawable: None,
            procs: Vec::new(),
            extra,
        };
        self.out.windows.push(window);

        let drawable = drawable_ref.and_then(|r| self.drawable(r));
        let procs: Vec<WinProc> = proc_refs
            .iter()
            .filter_map(|r| match self.raw.resolve(*r) {
                Target::Element(i) => Some(i),
                _ => None,
            })
            .map(|i| self.win_proc(i))
            .collect();
        let mut children = Vec::new();
        for r in child_refs {
            if let Target::Element(child) = self.raw.resolve(r) {
                if let Some(cid) = self.window(child, Some(id)) {
                    children.push(cid);
                }
            }
        }
        let w = &mut self.out.windows[id];
        w.drawable = drawable;
        w.procs = procs;
        w.children = children;
        self.visiting.pop();
        Some(id)
    }
}

fn whole(file: ResourceKey) -> ImageRef {
    ImageRef {
        file,
        uv: [0.0, 0.0, 1.0, 1.0],
        dims: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::format::testing::*;
    use crate::spui::format::ty;

    fn atlas_key() -> ResourceKey {
        ResourceKey {
            type_id: 0x2F7D_0004,
            group_id: 0x31A4_4893,
            instance_id: 0xB8B5_0133,
        }
    }

    /// root window with a button child and a text child, one shared atlas.
    fn sample() -> Vec<u8> {
        // Resource table: [0] atlas file, [1] hit mask, elements start at 2.
        let atlas = atlas_key();
        // Element indices: 0 root, 1 button, 2 text, 3 atlas image, 4 image
        // drawable, 5 simple layout, 6 std drawable (button).
        let e = |i: i16| 2 + i;
        let root = el(class::WINDOW, true, |w| {
            w.prop_u32(win::FLAGS, 3)
                .prop_rect(win::AREA, [25.0, 25.0, 871.0, 625.0])
                .prop_refs(win::CHILDREN, &[e(1), e(2)]);
            3
        });
        let button = el(class::BUTTON, false, |w| {
            w.prop_u32(win::FLAGS, 0x0A)
                .prop_u32(win::CONTROL_ID, 0x1234)
                .prop_rect(win::AREA, [-27.0, -90.0, 28.0, -35.0])
                .prop_refs(win::DRAWABLE, &[e(6)])
                .prop_refs(win::PROCS, &[e(5)]);
            5
        });
        let text = el(class::TEXT, false, |w| {
            w.prop_u32(win::CONTROL_ID, 0x4321)
                .prop_rect(win::AREA, [1.0, 2.0, 3.0, 4.0])
                .head(win::CAPTION, ty::TEXT, 1)
                .i16(1)
                .u16(b'0' as u16)
                .prop_u32(ids::text::COLOR, 0xFF79_D45B);
            4
        });
        let atlas_image = el(class::ATLAS_IMAGE, false, |w| {
            w.prop_refs(dr::ATLAS_FILE, &[0])
                .prop_rect(dr::ATLAS_UV, [0.25, 0.5, 0.5, 0.75])
                .head(dr::ATLAS_DIMENSIONS, ty::POINT, 1)
                .i32(128)
                .i32(128);
            3
        });
        let image_drawable = el(class::IMAGE_DRAWABLE, false, |w| {
            w.prop_refs(dr::IMAGE, &[e(3)]).prop_u32(dr::TILING, 1);
            2
        });
        let layout = el(class::SIMPLE_LAYOUT, false, |w| {
            w.head(ids::layout::PARAM, ty::U8, 1).u8(2);
            1
        });
        let std = el(class::STD_DRAWABLE, false, |w| {
            // images[0] = atlas image, hit mask = resource 1
            w.head(dr::IMAGES, ty::REF, 8);
            for i in 0..8 {
                w.i16(if i == 0 { e(3) } else { -1 });
            }
            w.prop_refs(dr::HIT_MASK, &[1]).prop_u32(dr::SCALE_TYPE, 2);
            3
        });
        // root window's drawable is only reachable through the image drawable
        // when referenced; reference it from the text window below instead.
        let _ = image_drawable;
        container(
            &[],
            &[atlas],
            &[(4, 4, vec![5, 6, 9, 10])],
            &[root, button, text, atlas_image, el(class::IMAGE_DRAWABLE, false, |w| {
                w.prop_refs(dr::IMAGE, &[e(3)]).prop_u32(dr::TILING, 1);
                2
            }), layout, std],
        )
    }

    #[test]
    fn builds_the_window_tree_with_drawables_and_procs() {
        let layout = Layout::parse(&sample()).unwrap();
        assert!(layout.warnings.is_empty(), "{:?}", layout.warnings);
        assert_eq!(layout.roots, vec![0]);
        assert_eq!(layout.windows.len(), 3);
        let root = layout.window(0);
        assert_eq!(root.kind, WindowKind::Window);
        assert_eq!(root.area, [25.0, 25.0, 871.0, 625.0]);
        assert_eq!(root.children, vec![1, 2]);
        let button = layout.window(1);
        assert_eq!(button.kind, WindowKind::Button);
        assert!(!button.visible() && button.enabled());
        assert_eq!(button.control_id, 0x1234);
        assert_eq!(button.parent, Some(0));
        assert_eq!(button.procs, vec![WinProc::SimpleLayout { anchor: 2 }]);
        match button.drawable.as_ref().unwrap() {
            Drawable::Std(std) => {
                assert_eq!(std.scale_type, SCALE_STRETCH_CENTER);
                assert_eq!(std.hit_mask, Some(0));
                let img = std.images[0].as_ref().unwrap();
                assert_eq!(img.file, atlas_key());
                assert_eq!(img.uv, [0.25, 0.5, 0.5, 0.75]);
                assert_eq!(img.dims, Some([128, 128]));
                assert!(std.images[1].is_none());
            }
            other => panic!("{other:?}"),
        }
        let text = layout.window(2);
        assert_eq!(text.caption, Some(Text::Literal("0".into())));
        assert_eq!(text.text().unwrap().color, 0xFF79_D45B);
        assert_eq!(layout.find(0x4321), Some(2));
        assert_eq!(layout.find_all(0x9999), Vec::<WinId>::new());
        assert_eq!(layout.preorder(), vec![0, 1, 2]);
    }

    #[test]
    fn hit_mask_runs_cover_exactly_their_pixels() {
        let layout = Layout::parse(&sample()).unwrap();
        let mask = &layout.hit_masks[0];
        // 4x4: runs 5..=6 (row 1, x 1..2) and 9..=10 (row 2, x 1..2).
        let covered: Vec<(i32, i32)> = (0..4)
            .flat_map(|y| (0..4).map(move |x| (x, y)))
            .filter(|(x, y)| mask.covers(*x, *y))
            .collect();
        assert_eq!(covered, vec![(1, 1), (2, 1), (1, 2), (2, 2)]);
        assert!(!mask.covers(-1, 0) && !mask.covers(4, 0) && !mask.covers(0, 4));
        assert_eq!(mask.size(), Some([4, 4]));
    }

    #[test]
    fn classification_follows_the_schema_chain() {
        assert_eq!(kind_of(class::BUTTON), Some(WindowKind::Button));
        assert_eq!(kind_of(class::POPUP_MENU), Some(WindowKind::Button));
        assert_eq!(kind_of(class::OUTLINED_TEXT), Some(WindowKind::Text));
        assert_eq!(kind_of(class::PROGRESS_BAR), Some(WindowKind::ProgressBar));
        assert_eq!(kind_of(class::STD_DRAWABLE), None);
        assert_eq!(kind_of(0xDEAD_BEEF), None);
    }

    #[test]
    fn id_constants_match_the_designer_schema() {
        let check = |class_id: u32, id: u32, name: &str| {
            assert_eq!(schema::prop_name(class_id, id), name, "{class_id:08X}/{id:08X}");
        };
        check(class::WINDOW, win::FLAGS, "WindowFlags");
        check(class::WINDOW, win::CONTROL_ID, "ControlID");
        check(class::WINDOW, win::AREA, "Area");
        check(class::WINDOW, win::CHILDREN, "Children");
        check(class::WINDOW, win::PROCS, "WinProcs");
        check(class::BUTTON, ids::button::FLAGS, "ButtonFlags");
        check(class::BUTTON, ids::button::CAPTION_COLORS, "CaptionColors");
        check(class::TEXT, ids::text::COLOR, "TextColor");
        check(class::ANIMATED_ICON, ids::anim_icon::FRAME_COUNT, "Frame Count");
        check(class::ANIMATED_ICON, ids::anim_icon::IMAGE, "Animation");
        check(class::STD_DRAWABLE, dr::SCALE_TYPE, "ScaleType");
        check(class::STD_DRAWABLE, dr::IMAGES, "Image");
        check(class::IMAGE_DRAWABLE, dr::TILING, "Tiling");
        check(class::ATLAS_IMAGE, dr::ATLAS_UV, "UV Coordinates");
        check(class::SPORE_STD_IMAGE_INFO, dr::INFO_ICON_COLOR, "Icon Color");
        check(class::SIMPLE_LAYOUT, ids::layout::PARAM, "Anchor");
        check(class::PROPORTIONAL_LAYOUT, ids::layout::PARAM, "Proportions");
        check(class::FADE, ids::effect::TIME, "Time");
        check(class::INFLATE, ids::effect::PARAM0, "Scale");
        check(class::TOOLTIP, ids::tooltip::OFFSET, "Tooltip offset position");
        check(class::WINDOW_STATE_EVENT, ev::TRIGGER, "Trigger");
        check(class::TIME_OSCILLATOR, ev::OSC_FREQUENCY, "Frequency");
        check(class::WINDOW_SCALE, ev::TO_VALUE, "To Scale");
        // Class names resolve too.
        assert_eq!(schema::class_name(class::SPORE_STD_DRAWABLE), "SporeStdDrawable");
        assert_eq!(schema::class_name(class::ATLAS_IMAGE), "Image Atlas");
    }

    #[test]
    fn unknown_classes_are_reported_not_fatal() {
        let bytes = container(
            &[],
            &[],
            &[],
            &[
                el(class::WINDOW, true, |w| {
                    w.prop_refs(win::CHILDREN, &[1]);
                    1
                }),
                el(0xDEAD_BEEF, false, |_| 0),
            ],
        );
        let layout = Layout::parse(&bytes).unwrap();
        assert_eq!(layout.windows.len(), 1);
        assert_eq!(layout.unknown_classes, vec![0xDEAD_BEEF]);
        assert_eq!(layout.warnings.len(), 1);
    }

    #[test]
    fn cyclic_children_do_not_loop() {
        let bytes = container(
            &[],
            &[],
            &[],
            &[el(class::WINDOW, true, |w| {
                w.prop_refs(win::CHILDREN, &[0]);
                1
            })],
        );
        let layout = Layout::parse(&bytes).unwrap();
        assert_eq!(layout.windows.len(), 1);
        assert!(layout.window(0).children.is_empty());
    }

    #[test]
    fn missing_properties_take_designer_defaults() {
        let bytes = container(
            &[],
            &[],
            &[],
            &[
                el(class::WINDOW, true, |w| {
                    w.prop_refs(win::CHILDREN, &[1, 2]);
                    1
                }),
                el(class::BUTTON, false, |_| 0),
                el(class::WINDOW, false, |_| 0),
            ],
        );
        let layout = Layout::parse(&bytes).unwrap();
        let button = layout.window(1);
        assert_eq!(button.flags, 0x0B);
        assert_eq!(button.area, [10.0, 10.0, 50.0, 50.0]);
        assert_eq!(button.text_style, DEFAULT_STYLE);
        assert_eq!(button.shade_color, WHITE);
        assert_eq!(button.button().unwrap().caption_colors, [0xFF00_0000; 8]);
        assert_eq!(layout.window(2).flags, 0x03);
        assert_eq!(layout.window(2).fill_color, DEFAULT_FILL);
    }

    /// Parses every layout of the installed game. Run explicitly:
    /// `claude-cargo.ps1 test --bin openspore spui::model::tests::installed -- --ignored --nocapture`
    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn installed_layouts_parse_without_errors() {
        use std::collections::BTreeMap;
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == crate::spui::format::SPUI_TYPE)
            .collect();
        keys.sort_by_key(|k| (k.group_id, k.instance_id));
        assert!(keys.len() > 100, "found only {} layouts", keys.len());
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut procs: BTreeMap<String, usize> = BTreeMap::new();
        let mut drawables: BTreeMap<String, usize> = BTreeMap::new();
        let mut failures = Vec::new();
        let mut unknown: BTreeMap<u32, usize> = BTreeMap::new();
        let mut warnings = 0usize;
        let mut windows = 0usize;
        for key in &keys {
            let bytes = match index.read(*key) {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("{key:?}: read: {e}"));
                    continue;
                }
            };
            match Layout::parse(&bytes) {
                Err(e) => failures.push(format!("{:08X}!{:08X}: {e}", key.group_id, key.instance_id)),
                Ok(layout) => {
                    windows += layout.windows.len();
                    warnings += layout.warnings.len();
                    for c in &layout.unknown_classes {
                        *unknown.entry(*c).or_default() += 1;
                    }
                    for w in &layout.windows {
                        *kinds.entry(format!("{:?}", w.kind)).or_default() += 1;
                        for p in &w.procs {
                            let name = match p {
                                WinProc::SimpleLayout { .. } => "SimpleLayout".to_string(),
                                WinProc::ProportionalLayout { .. } => "ProportionalLayout".into(),
                                WinProc::Effect { kind, .. } => {
                                    format!("Effect::{}", format!("{kind:?}").split(|c: char| !c.is_alphanumeric()).next().unwrap())
                                }
                                WinProc::Event(_) => "Event".into(),
                                WinProc::Tooltip { .. } => "Tooltip".into(),
                                WinProc::Other(c) => format!("Other({})", schema::class_name(*c)),
                            };
                            *procs.entry(name).or_default() += 1;
                        }
                        if let Some(d) = &w.drawable {
                            let name = match d {
                                Drawable::Image(_) => "Image".to_string(),
                                Drawable::Std(_) => "Std".into(),
                                Drawable::SporeStd { .. } => "SporeStd".into(),
                                Drawable::ButtonStandard { .. } => "ButtonStandard".into(),
                                Drawable::ButtonRadio { .. } => "ButtonRadio".into(),
                                Drawable::VariableWidth { .. } => "VariableWidth".into(),
                                Drawable::ImageSet { class, .. } => format!("ImageSet({})", schema::class_name(*class)),
                                Drawable::Frame => "Frame".into(),
                                Drawable::Other(c) => format!("Other({})", schema::class_name(*c)),
                            };
                            *drawables.entry(name).or_default() += 1;
                        }
                    }
                }
            }
        }
        println!("layouts: {}  windows: {windows}  warnings: {warnings}", keys.len());
        println!("window kinds: {kinds:?}");
        println!("window procedures: {procs:?}");
        println!("drawables: {drawables:?}");
        println!("unknown classes: {unknown:?}");
        assert!(failures.is_empty(), "failed layouts:\n{}", failures.join("\n"));
    }
}
