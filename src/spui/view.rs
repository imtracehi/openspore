//! Bevy UI presentation of SPUI layouts.
//!
//! [`spawn`] builds one absolutely positioned UI node per window plus a pool of
//! paint children (images, text) per window. A [`SpuiState`] component on the
//! root holds the runtime window state (visibility, enabled, hover/pressed,
//! areas, captions, tints); game code changes that state through its methods
//! and [`sync`] turns it into node geometry, images and text every frame. All
//! retail art, fonts, styles and strings are read from the install through
//! [`SpuiAssets`].

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    picking::Pickable,
    prelude::*,
    sprite::{BorderRect, SliceScaleMode, TextureSlicer},
    text::{Justify, LineBreak, LineHeight},
    ui::{widget::NodeImageMode, UiTransform, Val2},
};

use super::effects::{self, Pose, WindowEffects};
use super::fonts::{self, FontInfo};
use super::caption::{self, CaptionMask};
use super::format::{Text as SpuiText, SPUI_TYPE};
use super::ids::flag;
use super::model::*;
// Explicit import: Bevy's prelude also exports a `Window`.
use super::model::Window;
use super::paint::{self, Cmd, Mode, WinState};
use super::solve::{self, Rect as SRect, RootPolicy};
use super::surface::SpuiCopiesMaterial;
use super::style::{
    HAlign, StyleSheet, VAlign, STYLE_SHEET_GROUP, STYLE_SHEET_INSTANCE, STYLE_SHEET_TYPE,
};
use crate::asset_catalog::ResourceIndex;
use crate::dbpf::ResourceKey;

pub const LOCALE_TABLE_TYPE: u32 = 0x02FA_C0B6;
pub const FONT_TYPE: u32 = 0x027C_5CEF;
pub const FONT_GROUP: u32 = 0x0248_E873;

/// Style sheet font sizes are points: the retail text engine draws a
/// `font-size:8px` style at 8 x 96/72 pixels (verified against the retail goal
/// card, where 8/9/10 px styles measure 11/12/13 px on screen).
pub const FONT_SCALE: f32 = 96.0 / 72.0;

#[derive(Clone)]
pub struct Tex {
    pub handle: Handle<Image>,
    pub size: [u32; 2],
}

/// Everything read from the install that layouts need, loaded on demand.
#[derive(Resource, Default)]
pub struct SpuiAssets {
    textures: HashMap<ResourceKey, Option<Tex>>,
    layouts: HashMap<u32, Option<Arc<Layout>>>,
    styles: Option<StyleSheet>,
    fonts_loaded: bool,
    /// Faces registered with Bevy, in the style sheet's `@font` order.
    font_infos: Vec<FontInfo>,
    /// Asset handle per family (runs select their face by handle, so the
    /// choice never depends on how the text engine reads the name table).
    font_handles: HashMap<String, Handle<Font>>,
    locale: HashMap<u32, Arc<HashMap<u32, String>>>,
}

impl SpuiAssets {
    pub fn texture(
        &mut self,
        index: &ResourceIndex,
        images: &mut Assets<Image>,
        key: ResourceKey,
    ) -> Option<Tex> {
        self.textures
            .entry(key)
            .or_insert_with(|| {
                match import_texture(index, key, images) {
                    Ok((handle, w, h)) => Some(Tex {
                        handle,
                        size: [w, h],
                    }),
                    Err(e) => {
                        warn!("SPUI image {key:?}: {e}");
                        None
                    }
                }
            })
            .clone()
    }

    /// Register an image made at run time (e.g. a rendered planet impostor)
    /// under `key`, so windows can draw it through `SpuiState::set_image`.
    pub fn insert_texture(&mut self, key: ResourceKey, tex: Tex) {
        self.textures.insert(key, Some(tex));
    }

    pub fn texture_size(&self, key: &ResourceKey) -> Option<[u32; 2]> {
        self.textures.get(key)?.as_ref().map(|t| t.size)
    }

    /// Parsed layout by file name (for example `GlobalUICell-3`), or by raw
    /// instance id written as `0x02A6B9AE` for the layouts the name registry
    /// does not know.
    pub fn layout(&mut self, index: &ResourceIndex, name: &str) -> Option<Arc<Layout>> {
        let id = match name.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(raw) => raw,
            None => crate::world_objects::name_id(name),
        };
        self.layouts
            .entry(id)
            .or_insert_with(|| {
                let key = index
                    .records
                    .keys()
                    .filter(|k| k.type_id == SPUI_TYPE && k.instance_id == id)
                    .min_by_key(|k| k.group_id)
                    .copied()?;
                let bytes = index.read(key).ok()?;
                match Layout::parse(&bytes) {
                    Ok(l) => Some(Arc::new(l)),
                    Err(e) => {
                        warn!("SPUI layout {name}: {e}");
                        None
                    }
                }
            })
            .clone()
    }

    /// Instance id of the cached layout `layout` (`ui_sounds` finds the screen a spawned
    /// window tree shows by it).
    pub fn layout_instance(&self, layout: &Arc<Layout>) -> Option<u32> {
        self.layouts
            .iter()
            .find(|(_, l)| l.as_ref().is_some_and(|l| Arc::ptr_eq(l, layout)))
            .map(|(id, _)| *id)
    }

    /// Tests: register a hand-made layout under the instance id of a layout name.
    #[cfg(test)]
    pub fn insert_layout(&mut self, id: u32, layout: Arc<Layout>) {
        self.layouts.insert(id, Some(layout));
    }

    pub fn styles(&mut self, index: &ResourceIndex) -> &StyleSheet {
        self.styles.get_or_insert_with(|| {
            index
                .read(ResourceKey {
                    type_id: STYLE_SHEET_TYPE,
                    group_id: STYLE_SHEET_GROUP,
                    instance_id: STYLE_SHEET_INSTANCE,
                })
                .map(|b| StyleSheet::parse(&String::from_utf8_lossy(&b)))
                .unwrap_or_default()
        })
    }

    /// Register the UI fonts named by the style sheet with Bevy's text system.
    pub fn ensure_fonts(&mut self, index: &ResourceIndex, fonts: &mut Assets<Font>) {
        if self.fonts_loaded {
            return;
        }
        self.fonts_loaded = true;
        let names = self.styles(index).fonts.clone();
        for file in names {
            let stem = file.trim_end_matches(".ttf").trim_end_matches(".TTF");
            let key = ResourceKey {
                type_id: FONT_TYPE,
                group_id: FONT_GROUP,
                instance_id: crate::world_objects::name_id(stem),
            };
            // The fallback face is 23 MB; the HUD does not need it.
            if index
                .records
                .get(&key)
                .is_some_and(|(_, e)| e.mem_size < 4 * 1024 * 1024)
            {
                if let Ok(bytes) = index.read(key) {
                    let handle = fonts.add(Font::from_bytes(bytes.clone()));
                    if let Some(info) = FontInfo::parse(&bytes) {
                        self.font_handles.entry(info.family.clone()).or_insert(handle);
                        self.font_infos.push(info);
                    }
                }
            }
        }
    }

    /// Faces known to the text renderer (family, metrics, coverage).
    pub fn font_infos(&self) -> &[FontInfo] {
        &self.font_infos
    }

    /// Font source for a family: the registered asset when we have it.
    pub fn font_source(&self, family: &str) -> bevy::text::FontSource {
        match self.font_handles.get(family) {
            Some(handle) => bevy::text::FontSource::Handle(handle.clone()),
            None => bevy::text::FontSource::Family(family.to_string().into()),
        }
    }

    /// Localized string by table name hash and id.
    pub fn localized(&mut self, index: &ResourceIndex, table: u32, id: u32) -> Option<String> {
        let entries = self
            .locale
            .entry(table)
            .or_insert_with(|| {
                let mut keys: Vec<_> = index
                    .records
                    .keys()
                    .filter(|k| k.type_id == LOCALE_TABLE_TYPE && k.instance_id == table)
                    .copied()
                    .collect();
                keys.sort_by_key(|k| k.group_id);
                let mut map = HashMap::new();
                for k in keys {
                    if let Ok(bytes) = index.read(k) {
                        for (id, text) in crate::decode::parse_locale_table(&bytes) {
                            map.entry(id).or_insert(text);
                        }
                    }
                }
                Arc::new(map)
            })
            .clone();
        entries.get(&id).cloned()
    }

    pub fn text(&mut self, index: &ResourceIndex, text: &SpuiText) -> String {
        match text {
            SpuiText::Literal(s) => s.clone(),
            SpuiText::Localized { table, id } => self.localized(index, *table, *id).unwrap_or_default(),
        }
    }

    /// Load every texture a layout draws from.
    pub fn preload(&mut self, index: &ResourceIndex, images: &mut Assets<Image>, layout: &Layout) {
        let mut files: Vec<ResourceKey> = Vec::new();
        let mut add = |r: &Option<ImageRef>| {
            if let Some(r) = r {
                if !files.contains(&r.file) {
                    files.push(r.file);
                }
            }
        };
        for w in &layout.windows {
            match &w.drawable {
                Some(Drawable::Image(d)) => add(&d.image),
                Some(Drawable::Std(d)) => d.images.iter().for_each(&mut add),
                Some(Drawable::SporeStd { base, infos }) => {
                    base.images.iter().for_each(&mut add);
                    for i in infos.iter().flatten() {
                        add(&i.background);
                        add(&i.icon);
                    }
                }
                Some(Drawable::ButtonStandard { image, .. })
                | Some(Drawable::ButtonRadio { image, .. })
                | Some(Drawable::VariableWidth { image, .. }) => add(image),
                Some(Drawable::ImageSet { images, .. }) => images.iter().for_each(&mut add),
                _ => {}
            }
            if let Extra::Anim(a) = &w.extra {
                add(&a.image);
            }
        }
        for f in files {
            self.texture(index, images, f);
        }
    }
}

/// Runtime state of one spawned layout.
#[derive(Component)]
pub struct SpuiState {
    pub layout: Arc<Layout>,
    pub policy: RootPolicy,
    /// Logical size of the surface the layout is solved for.
    pub screen: [f32; 2],
    pub areas: Vec<[f32; 4]>,
    pub flags: Vec<u32>,
    pub states: Vec<u32>,
    pub shades: Vec<u32>,
    /// Colour multiplied into the window's own drawable only (not inherited
    /// by children), for example the progress bar segment colours.
    pub tints: Vec<u32>,
    /// Draw the window and its children again over the window's left
    /// `fraction` with the colour taking the place of the window's own shade
    /// (the filled part of a progress bar segment).
    pub overlays: Vec<Option<(f32, u32)>>,
    /// Draw the overlay of the window over its child windows instead of under
    /// them (progress segments whose art is made of child windows, as the Space
    /// bar's; the overlay redraws the children, so nothing is lost).
    pub overlays_front: Vec<bool>,
    pub captions: Vec<Option<String>>,
    /// Em size in pixels that replaces the style's size (after the point
    /// scaling) for a text window.
    pub text_sizes: Vec<Option<f32>>,
    /// Size of the laid-out text of the previous frame (`IText::GetTextBounds`).
    pub text_extents: Vec<Option<Vec2>>,
    pub rects: Vec<SRect>,
    pub entities: Vec<Entity>,
    pub time: f32,
    /// Windows whose button was clicked since the last [`SpuiState::take_clicks`].
    pub clicks: Vec<WinId>,
    /// Windows clicked during the last [`input`] pass. Unlike `clicks` nobody takes them:
    /// they are read-only for observers (interface sounds, `ui_sounds`) and the
    /// next pass clears them.
    pub activations: Vec<WinId>,
    pub ready: bool,
    /// Image shown instead of the window's own drawable (game code setting a
    /// window's image at runtime, e.g. palette and ability icons).
    pub image_overrides: Vec<Option<ImageRef>>,
    /// Icon drawn inside a `SporeStd` button over its own background (the
    /// `icon` slot of the button's state images), for ability buttons whose
    /// round slot art stays.
    pub icon_overrides: Vec<Option<ImageRef>>,
    /// Tooltip text the game code gives a window that has no `Tooltip`
    /// procedure in its layout (ability buttons the retail code labels at
    /// runtime); it takes the standard bubble.
    pub tooltip_overrides: Vec<Option<String>>,
    /// Flipbook frame shown by an animated icon instead of its playback.
    pub frame_overrides: Vec<Option<u32>>,
    /// Columns and rows of the flipbook of an animated icon whose layout
    /// leaves the grid to the game code (a one-frame grid in the file).
    pub frame_grids: Vec<Option<(u32, u32)>>,
    /// Rotation of a window about its centre in radians, clockwise on screen
    /// (the callout arrow); children turn with it.
    pub rotations: Vec<f32>,
    /// Value (0..1 of the range) of a slider or scrollbar set by game code
    /// instead of the authored value.
    pub ranges: Vec<Option<f32>>,
    /// Visibility effects (Fade/Inflate/Glide) per window, the seconds each
    /// has been on and the resulting pose (see [`super::effects`]).
    effects: Vec<Option<WindowEffects>>,
    effect_clock: Vec<f32>,
    poses: Vec<Pose>,
    effects_started: bool,
    layout_dirty: bool,
    preorder: Vec<WinId>,
}

impl SpuiState {
    pub fn new(layout: Arc<Layout>, policy: RootPolicy) -> Self {
        let n = layout.windows.len();
        Self {
            policy,
            screen: [0.0, 0.0],
            areas: solve::authored_areas(&layout),
            flags: layout.windows.iter().map(|w| w.flags).collect(),
            states: layout
                .windows
                .iter()
                .map(|w| w.button().map_or(0, |b| b.state_flags & super::ids::state::SELECTED))
                .collect(),
            shades: layout.windows.iter().map(|w| w.shade_color).collect(),
            tints: vec![0xFFFF_FFFF; n],
            overlays: vec![None; n],
            overlays_front: vec![false; n],
            captions: vec![None; n],
            text_sizes: vec![None; n],
            text_extents: vec![None; n],
            rects: vec![SRect::default(); n],
            entities: Vec::new(),
            time: 0.0,
            clicks: Vec::new(),
            activations: Vec::new(),
            ready: false,
            image_overrides: vec![None; n],
            icon_overrides: vec![None; n],
            tooltip_overrides: vec![None; n],
            frame_overrides: vec![None; n],
            frame_grids: vec![None; n],
            rotations: vec![0.0; n],
            ranges: vec![None; n],
            effects: effects::collect(&layout),
            effect_clock: vec![0.0; n],
            poses: vec![Pose::REST; n],
            effects_started: false,
            layout_dirty: true,
            preorder: layout.preorder(),
            layout,
        }
    }

    /// Set a slider's value as a fraction of its range (`None`: authored).
    pub fn set_range(&mut self, id: WinId, fraction: Option<f32>) {
        self.ranges[id] = fraction.map(|f| f.clamp(0.0, 1.0));
    }

    /// Rotate a window (and its children) about its centre; radians, clockwise.
    pub fn set_rotation(&mut self, id: WinId, radians: f32) {
        self.rotations[id] = radians;
    }

    /// Show `image` (a whole PNG or an atlas region) stretched over the window
    /// instead of its drawable; `None` restores the drawable. The texture must
    /// be loaded through [`SpuiAssets::texture`].
    pub fn set_image(&mut self, id: WinId, image: Option<ImageRef>) {
        if self.image_overrides[id] != image {
            self.image_overrides[id] = image;
        }
    }

    /// Draw `icon` in the icon slot of a `SporeStd` button (a whole PNG or an
    /// atlas region) while keeping the button's own background; `None`
    /// restores the authored icon. The texture must be loaded through
    /// [`SpuiAssets::texture`].
    pub fn set_icon(&mut self, id: WinId, icon: Option<ImageRef>) {
        if self.icon_overrides[id] != icon {
            self.icon_overrides[id] = icon;
        }
    }

    /// Give the window a tooltip with `text` (`None` removes it).
    pub fn set_tooltip(&mut self, id: WinId, text: Option<String>) {
        if self.tooltip_overrides[id] != text {
            self.tooltip_overrides[id] = text;
        }
    }

    /// Show one frame of an animated icon (`None` = normal playback).
    pub fn set_frame(&mut self, id: WinId, frame: Option<u32>) {
        self.frame_overrides[id] = frame;
    }

    /// Give an animated icon a flipbook of `columns` x `rows` frames.
    pub fn set_frame_grid(&mut self, id: WinId, columns: u32, rows: u32) {
        self.frame_grids[id] = Some((columns.max(1), rows.max(1)));
    }

    pub fn find(&self, control_id: u32) -> Option<WinId> {
        self.layout.find(control_id)
    }

    pub fn find_all(&self, control_id: u32) -> Vec<WinId> {
        self.layout.find_all(control_id)
    }

    pub fn visible(&self, id: WinId) -> bool {
        self.flags[id] & flag::VISIBLE != 0
    }

    pub fn set_visible(&mut self, id: WinId, on: bool) {
        self.set_flag(id, flag::VISIBLE, on);
    }

    pub fn set_enabled(&mut self, id: WinId, on: bool) {
        self.set_flag(id, flag::ENABLED, on);
    }

    fn set_flag(&mut self, id: WinId, bit: u32, on: bool) {
        let new = if on { self.flags[id] | bit } else { self.flags[id] & !bit };
        self.flags[id] = new;
    }

    pub fn set_selected(&mut self, id: WinId, on: bool) {
        if on {
            self.states[id] |= super::ids::state::SELECTED;
        } else {
            self.states[id] &= !super::ids::state::SELECTED;
        }
    }

    pub fn set_text(&mut self, id: WinId, text: impl Into<String>) {
        let text = text.into();
        if self.captions[id].as_deref() != Some(text.as_str()) {
            self.captions[id] = Some(text);
        }
    }

    /// Override the em size (pixels) of a text window; `None` follows the style.
    pub fn set_text_size(&mut self, id: WinId, px: Option<f32>) {
        self.text_sizes[id] = px;
    }

    /// Width and height of the text laid out last frame, the equivalent of
    /// `IText::GetTextBounds` (code that sizes a window to its text uses it).
    pub fn text_extent(&self, id: WinId) -> Option<Vec2> {
        self.text_extents[id]
    }

    pub fn set_area(&mut self, id: WinId, area: [f32; 4]) {
        if self.areas[id] != area {
            self.areas[id] = area;
            self.layout_dirty = true;
        }
    }

    pub fn area(&self, id: WinId) -> [f32; 4] {
        self.areas[id]
    }

    pub fn set_shade(&mut self, id: WinId, argb: u32) {
        self.shades[id] = argb;
    }

    pub fn set_tint(&mut self, id: WinId, argb: u32) {
        self.tints[id] = argb;
    }

    pub fn set_overlay(&mut self, id: WinId, overlay: Option<(f32, u32)>) {
        self.overlays[id] = overlay;
    }

    /// Show the window's overlay above its child windows (set before the
    /// overlay first appears).
    pub fn set_overlay_front(&mut self, id: WinId, front: bool) {
        self.overlays_front[id] = front;
    }

    /// Real (solved) rectangle of a window, valid once [`SpuiState::ready`].
    pub fn rect(&self, id: WinId) -> SRect {
        self.rects[id]
    }

    pub fn take_clicks(&mut self) -> Vec<WinId> {
        std::mem::take(&mut self.clicks)
    }

    /// Windows that are shown according to their flags and their ancestors'
    /// (what input sees; a window fading out is no longer interactive).
    pub fn interactive(&self) -> Vec<bool> {
        let mut live = vec![false; self.layout.windows.len()];
        for &id in &self.preorder {
            let parent = self.layout.windows[id].parent;
            live[id] = parent.map_or(true, |p| live[p]) && self.visible(id);
        }
        live
    }

    /// Advance the visibility effects by `dt` seconds: a window's effects run
    /// forward while it is shown and backward while it is hidden. Windows
    /// shown when the layout first runs start fully shown (no entrance).
    fn step_effects(&mut self, dt: f32) {
        if self.effects.iter().all(Option::is_none) {
            return;
        }
        let live = self.interactive();
        for id in 0..self.effects.len() {
            let Some(fx) = &self.effects[id] else {
                continue;
            };
            let clock = &mut self.effect_clock[id];
            if !self.effects_started {
                *clock = if live[id] { fx.duration } else { 0.0 };
            } else if live[id] {
                *clock = (*clock + dt).min(fx.duration);
            } else {
                *clock = (*clock - dt).max(0.0);
            }
            self.poses[id] = fx.pose(*clock);
        }
        self.effects_started = true;
    }

    /// The colour a `SporeStd` drawable (every retail button) is modulated by: what the ancestors and
    /// the window's visibility effects hand down, but not the window's own authored shade.
    /// VERIFIED (march2017 image): `Window::OnPaint` 0x960270 passes only the window's own shade in
    /// `RenderParams::shadeColor`, and `SporeStdDrawable::Paint` 0x8318F0 starts with
    /// `Graphics2D::SetColor(0xFFFFFFFF)`, stores the parameter (+0x144) and never reads it again
    /// (its helpers 0x8314F0, 0x8308F0, 0x830E70 draw with the image infos' own colours). So the
    /// five step dots of the new-game flow, authored `shade = 00FFFFFF` and shown by 0xDF3170 with
    /// a plain visibility flag, are drawn in retail; with the shade multiplied in they were invisible.
    fn sporestd_shade(&self, id: WinId, shade: &[u32]) -> u32 {
        let parent = self.layout.windows[id].parent.map_or(0xFFFF_FFFF, |p| shade[p]);
        let alpha = self.poses[id].alpha;
        if alpha < 1.0 {
            mul_argb(parent, ((alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 24 | 0x00FF_FFFF)
        } else {
            parent
        }
    }

    /// Inherited colour and drawn-visibility per window. A window fading out
    /// stays drawn (with its subtree) until its effects have run back.
    fn effective(&self) -> (Vec<u32>, Vec<bool>) {
        let n = self.layout.windows.len();
        let mut shade = vec![0xFFFF_FFFFu32; n];
        let mut visible = vec![false; n];
        for &id in &self.preorder {
            let parent = self.layout.windows[id].parent;
            let (ps, pv) = parent.map_or((0xFFFF_FFFF, true), |p| (shade[p], visible[p]));
            shade[id] = mul_argb(ps, self.shades[id]);
            let fading = self.effects[id].is_some() && self.effect_clock[id] > 0.0;
            visible[id] = pv && (self.visible(id) || fading);
            let alpha = self.poses[id].alpha;
            if alpha < 1.0 {
                shade[id] = mul_argb(shade[id], ((alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 24 | 0x00FF_FFFF);
            }
        }
        (shade, visible)
    }
}

pub fn mul_argb(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s) & 0xFF) as f32 / 255.0 * ((b >> s) & 0xFF) as f32 / 255.0;
    let q = |v: f32| (v * 255.0 + 0.5) as u32;
    q(ch(24)) << 24 | q(ch(16)) << 16 | q(ch(8)) << 8 | q(ch(0))
}

/// Colour of an `0xAARRGGBB` value. With the gamma surface the raw values are
/// handed to the (Unorm) surface unconverted, like the retail renderer; without
/// it they are ordinary sRGB colours.
pub fn argb_color(argb: u32) -> Color {
    let (r, g, b, a) = (
        ((argb >> 16) & 0xFF) as f32 / 255.0,
        ((argb >> 8) & 0xFF) as f32 / 255.0,
        (argb & 0xFF) as f32 / 255.0,
        ((argb >> 24) & 0xFF) as f32 / 255.0,
    );
    if super::surface::gamma_surface() {
        Color::LinearRgba(LinearRgba::new(r, g, b, a))
    } else {
        Color::srgba(r, g, b, a)
    }
}

/// Decode a retail PNG into an image of the colour encoding the active
/// presentation expects (raw values for the gamma surface, sRGB otherwise).
fn import_texture(
    index: &ResourceIndex,
    key: ResourceKey,
    images: &mut Assets<Image>,
) -> Result<(Handle<Image>, u32, u32), String> {
    if !super::surface::gamma_surface() {
        return crate::ui_art::import_png(index, key, images);
    }
    if key.type_id != crate::ui_art::UI_PNG_TYPE {
        return Err(format!("resource type {:08X} is not a native PNG", key.type_id));
    }
    let bytes = index.read(key)?;
    let (width, height, mut rgba) = crate::ui_art::decode_png(&bytes)?;
    crate::branding::patch_texture(key, width, height, &mut rgba);
    let image = Image::new(
        bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        rgba,
        bevy::render::render_resource::TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    Ok((images.add(image), width, height))
}

#[derive(Component)]
pub struct SpuiWindow {
    pub root: Entity,
    pub id: WinId,
}

/// Pooled paint children of a window: the base image pool, an optional clipped
/// overlay (progress fills) and the text box.
#[derive(Component, Default)]
pub struct WinPaint {
    pool: Vec<Entity>,
    /// Copy nodes ([`SpuiMat`]) of the commands that are `Cmd::Copies`.
    mat_pool: Vec<Entity>,
    overlay: Option<(Entity, Vec<Entity>)>,
    text: Option<TextNodes>,
    /// One extra text box per copy of the caption's outline kernel ([`Outline::taps`]),
    /// behind the caption.
    outline_text: Vec<GhostText>,
    /// The kernel copies of the window's "Text Outline" (computed once; the format is static).
    outline_taps: Option<Arc<Vec<super::outline::Tap>>>,
    /// Frames the window (or its caption) has been hidden while it owns outline copies.
    hidden_frames: u32,
    /// Frames the caption is held back after its outline copies were spawned, so that it never
    /// shows without them.
    ghost_wait: u32,
    /// The one node that draws all outline copies of the caption from its glyph mask
    /// ([`super::caption`]); replaces `outline_text` where the copies material exists.
    mask: Option<CaptionCopies>,
}

/// The copies node of a caption (see [`super::caption`]) and what it was last set up with.
struct CaptionCopies {
    node: Entity,
    material: Handle<SpuiCopiesMaterial>,
    image: Handle<Image>,
    needed_pad: Arc<std::sync::atomic::AtomicU32>,
    applied: CopiesKey,
}

/// What the copies node, its material and the mask component depend on besides the static kernel.
#[derive(Clone, Copy, PartialEq)]
struct CopiesKey {
    box_size: [u32; 2],
    pad: u32,
    /// The window's effective shade (tints the copies).
    shade: u32,
}

/// One copy of a caption in an outline: a single absolutely positioned text node inside the
/// caption's flex box (its static position follows the box's alignment like the caption's own),
/// shifted with a `UiTransform`, plus its further font runs.
struct GhostText {
    text: Entity,
    spans: Vec<Entity>,
}

/// Frames a hidden window keeps its outline copies before they are despawned (the copies of a
/// window that is shown again within this time are reused; a window shown after a long time spawns
/// them again and holds its caption back for [`GHOST_WAIT_FRAMES`]).
const GHOST_GRACE_FRAMES: u32 = 30;
const GHOST_WAIT_FRAMES: u32 = 2;

/// The text box of a window: the flex box, the root `Text` (first font run)
/// and one `TextSpan` per further font run.
#[derive(Clone)]
struct TextNodes {
    outer: Entity,
    inner: Entity,
    spans: Vec<Entity>,
}

#[derive(Component)]
pub struct SpuiPaint;

/// A UI node that draws the copies of an image layer with [`SpuiCopiesMaterial`].
#[derive(Component)]
pub struct SpuiMat;

/// Container nodes (text boxes, clipped overlays).
#[derive(Component)]
pub struct SpuiBox;

// Stacking inside a window node: its own fill (the node background), then the
// paint children, the clipped overlay and the text box, then child windows.
// Negative z-indices would put the paint children *behind* the node's own
// background in Bevy's UI stack, so everything is non-negative.
// The paint children of a window take the z of their command index on top of `Z_PAINT` (images
// and copy nodes live in two pools, so the order of the commands is the z order).
const Z_PAINT: i32 = 0;
const Z_OVERLAY: i32 = 1000;
const Z_TEXT: i32 = 1001;
const Z_WINDOW: i32 = 1002;

fn abs_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Px(0.0),
        height: Val::Px(0.0),
        ..default()
    }
}

/// Spawn a layout as a UI tree. Returns the root entity carrying [`SpuiState`].
pub fn spawn(
    commands: &mut Commands,
    layout: Arc<Layout>,
    policy: RootPolicy,
    camera: Option<Entity>,
    z: i32,
) -> Entity {
    let mut state = SpuiState::new(layout.clone(), policy);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(z),
            Pickable::IGNORE,
            Name::new("spui-root"),
        ))
        .id();
    if let Some(cam) = camera {
        commands.entity(root).insert(UiTargetCamera(cam));
    }
    let entities: Vec<Entity> = layout.windows.iter().map(|_| commands.spawn_empty().id()).collect();
    for w in &layout.windows {
        let e = entities[w.id];
        let mut bundle = commands.entity(e);
        bundle.insert((
            abs_node(),
            Visibility::Hidden,
            Pickable::IGNORE,
            ZIndex(Z_WINDOW),
            SpuiWindow { root, id: w.id },
            WinPaint::default(),
        ));
        if w.paints_fill_color() {
            bundle.insert(BackgroundColor(Color::NONE));
        }
    }
    // Front-most child first in the file: insert children in reverse so the
    // first child is spawned last and therefore painted on top.
    for &r in layout.roots.iter().rev() {
        commands.entity(entities[r]).insert(ChildOf(root));
    }
    for w in &layout.windows {
        for &c in w.children.iter().rev() {
            commands.entity(entities[c]).insert(ChildOf(entities[w.id]));
        }
    }
    state.entities = entities;
    commands.entity(root).insert(state);
    root
}

fn image_node(tex: &Tex, src: SRect, color: u32, mode: Mode) -> ImageNode {
    let rect = bevy::math::Rect::new(src.x1, src.y1, src.x2, src.y2);
    let image_mode = match mode {
        Mode::Stretch => NodeImageMode::Stretch,
        Mode::Tiled => NodeImageMode::Tiled {
            tile_x: true,
            tile_y: true,
            stretch_value: 1.0,
        },
        Mode::Sliced { border, tile } => {
            let scale = if tile {
                SliceScaleMode::Tile { stretch_value: 1.0 }
            } else {
                SliceScaleMode::Stretch
            };
            NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect {
                    min_inset: Vec2::new(border[0], border[1]),
                    max_inset: Vec2::new(border[2], border[3]),
                },
                center_scale_mode: scale,
                sides_scale_mode: scale,
                max_corner_scale: 1.0,
            })
        }
    };
    ImageNode {
        image: tex.handle.clone(),
        rect: Some(rect),
        color: argb_color(color),
        image_mode,
        ..default()
    }
}

fn set_px(node: &mut Node, x: f32, y: f32, w: f32, h: f32) {
    let (l, t, wd, ht) = (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h));
    if node.left != l {
        node.left = l;
    }
    if node.top != t {
        node.top = t;
    }
    if node.width != wd {
        node.width = wd;
    }
    if node.height != ht {
        node.height = ht;
    }
}

fn text_color_for(w: &Window, st: WinState) -> u32 {
    match &w.extra {
        Extra::Button(b) => {
            // CaptionColors order: enabled, disabled, depressed, highlighted,
            // selected, selected highlighted, selected depressed, active.
            let i = if !st.enabled() {
                1
            } else if st.selected() {
                if st.clicked() {
                    6
                } else if st.hover() {
                    5
                } else {
                    4
                }
            } else if st.clicked() {
                2
            } else if st.hover() {
                3
            } else {
                0
            };
            b.caption_colors[i]
        }
        Extra::Text(t) => t.color,
        _ => 0xFFFF_FFFF,
    }
}

type PaintQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut ImageNode, &'static mut Visibility, &'static mut ZIndex),
    (With<SpuiPaint>, Without<SpuiWindow>, Without<SpuiBox>),
>;

/// Make the children of `parent` show exactly the image commands: `pool` holds the `ImageNode`s of
/// the `Cmd::Image` commands, `mat_pool` the copy nodes of the `Cmd::Copies` commands; each node
/// takes the z of its command's index so that the commands stay in drawing order.
#[allow(clippy::too_many_arguments)]
fn update_pool(
    commands: &mut Commands,
    parent: Entity,
    z: i32,
    pool: &mut Vec<Entity>,
    mat_pool: &mut Vec<Entity>,
    cmds: &[&Cmd],
    paints: &mut PaintQuery,
    mats: &mut Query<
        (&mut Node, &mut MaterialNode<SpuiCopiesMaterial>, &mut Visibility, &mut ZIndex),
        (With<SpuiMat>, Without<SpuiWindow>, Without<SpuiPaint>, Without<SpuiBox>),
    >,
    materials: &mut Option<ResMut<Assets<SpuiCopiesMaterial>>>,
    textures: &HashMap<ResourceKey, Tex>,
) {
    let (mut image_n, mut mat_n) = (0usize, 0usize);
    for (index, cmd) in cmds.iter().enumerate() {
        let node_z = z + index as i32;
        match cmd {
            Cmd::Image { file, src, dest, color, mode } => {
                let Some(tex) = textures.get(file) else {
                    continue;
                };
                if pool.len() <= image_n {
                    let e = commands
                        .spawn((
                            SpuiPaint,
                            abs_node(),
                            ImageNode::default(),
                            Visibility::Hidden,
                            ZIndex(node_z),
                            Pickable::IGNORE,
                            ChildOf(parent),
                        ))
                        .id();
                    pool.push(e);
                }
                let pe = pool[image_n];
                image_n += 1;
                let Ok((mut pn, mut pi, mut pv, mut pz)) = paints.get_mut(pe) else {
                    continue;
                };
                set_px(&mut pn, dest.x1, dest.y1, dest.width(), dest.height());
                let node_image = image_node(tex, *src, *color, *mode);
                let resized = pn.is_changed();
                if pi.image != node_image.image
                    || pi.rect != node_image.rect
                    || pi.color != node_image.color
                    || pi.image_mode != node_image.image_mode
                {
                    *pi = node_image;
                } else if resized && matches!(mode, Mode::Sliced { .. } | Mode::Tiled) {
                    // ADAPTED (cell-editor): Bevy 0.20 extracts sliced
                    // images incrementally and kept the slices of an
                    // earlier, smaller node size; re-extract on resize.
                    pi.set_changed();
                }
                if pz.0 != node_z {
                    pz.0 = node_z;
                }
                if *pv != Visibility::Inherited {
                    *pv = Visibility::Inherited;
                }
            }
            Cmd::Copies { file, src, dest, rgb, taps } => {
                let (Some(tex), Some(materials)) = (textures.get(file), materials.as_mut()) else {
                    continue;
                };
                if mat_pool.len() <= mat_n {
                    let handle = materials.add(SpuiCopiesMaterial::default());
                    let e = commands
                        .spawn((
                            SpuiMat,
                            abs_node(),
                            MaterialNode(handle),
                            Visibility::Hidden,
                            ZIndex(node_z),
                            Pickable::IGNORE,
                            ChildOf(parent),
                        ))
                        .id();
                    mat_pool.push(e);
                }
                let me = mat_pool[mat_n];
                mat_n += 1;
                let Ok((mut node, handle, mut vis, mut zi)) = mats.get_mut(me) else {
                    continue;
                };
                // The node is the layer's rectangle grown by the largest shift of a copy.
                let margin = taps.iter().map(|(dx, dy, _)| dx.abs().max(dy.abs())).max().unwrap_or(0) as f32;
                set_px(
                    &mut node,
                    dest.x1 - margin,
                    dest.y1 - margin,
                    dest.width() + 2.0 * margin,
                    dest.height() + 2.0 * margin,
                );
                if DUMP_COPIES.load(std::sync::atomic::Ordering::Relaxed) {
                    println!(
                        "SPUI_COPIES dest {:.0}x{:.0} margin {} taps {}",
                        dest.width(),
                        dest.height(),
                        margin,
                        taps.len()
                    );
                }
                let [tw, th] = [tex.size[0] as f32, tex.size[1] as f32];
                let mut params = super::surface::CopiesParams::default();
                params.src = Vec4::new(src.x1 / tw, src.y1 / th, src.x2 / tw, src.y2 / th);
                params.info = Vec4::new(margin, taps.len().min(paint::MAX_COPIES) as f32, 0.0, 0.0);
                params.color = Vec4::new(
                    ((rgb >> 16) & 0xFF) as f32 / 255.0,
                    ((rgb >> 8) & 0xFF) as f32 / 255.0,
                    (rgb & 0xFF) as f32 / 255.0,
                    0.0,
                );
                for (slot, (dx, dy, alpha)) in params.taps.iter_mut().zip(taps.iter()) {
                    *slot = Vec4::new(*dx as f32, *dy as f32, *alpha as f32 / 255.0, 0.0);
                }
                let stale = materials
                    .get(&handle.0)
                    .is_none_or(|m| m.image != tex.handle || m.params != params);
                if stale {
                    if let Some(mut m) = materials.get_mut(&handle.0) {
                        m.image = tex.handle.clone();
                        m.params = params;
                    }
                }
                if zi.0 != node_z {
                    zi.0 = node_z;
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            Cmd::Fill { .. } => {}
        }
    }
    // Nodes of commands that are gone.
    for pe in pool.iter().skip(image_n) {
        if let Ok((_, _, mut pv, _)) = paints.get_mut(*pe) {
            if *pv != Visibility::Hidden {
                *pv = Visibility::Hidden;
            }
        }
    }
    for me in mat_pool.iter().skip(mat_n) {
        if let Ok((_, _, mut vis, _)) = mats.get_mut(*me) {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
    }
}

/// Image commands of a window and its visible descendants as if the window's
/// own shade were `color` (the parents' shade still applies), relative to the
/// window's rectangle. Back to front, so a subtree stays contiguous.
fn overlay_commands(
    layout: &Layout,
    state: &SpuiState,
    root: WinId,
    color: u32,
    effective_shade: &[u32],
    visible: &[bool],
    sizes: &dyn paint::TextureSizes,
) -> Vec<Cmd> {
    let origin = state.rects[root];
    let parent_shade = layout.windows[root].parent.map_or(0xFFFF_FFFF, |p| effective_shade[p]);
    let mut out = Vec::new();
    let mut stack = vec![(root, mul_argb(parent_shade, color))];
    while let Some((id, shade)) = stack.pop() {
        if !visible[id] {
            continue;
        }
        let w = &layout.windows[id];
        let rect = state.rects[id];
        let st = WinState {
            flags: state.flags[id],
            state: state.states[id],
        };
        let (dx, dy) = (rect.x1 - origin.x1, rect.y1 - origin.y1);
        for cmd in paint::expand_copies(paint::window_commands(w, rect, st, shade, sizes)) {
            if let Cmd::Image { file, src, dest, color, mode } = cmd {
                out.push(Cmd::Image {
                    file,
                    src,
                    dest: SRect::new(dest.x1 + dx, dest.y1 + dy, dest.x2 + dx, dest.y2 + dy),
                    color,
                    mode,
                });
            }
        }
        for &child in &w.children {
            stack.push((child, mul_argb(shade, state.shades[child])));
        }
    }
    out
}

/// The main per-frame system: solve, then update nodes, images and text.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    time: Res<Time>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    images: Option<ResMut<Assets<Image>>>,
    fonts: Option<ResMut<Assets<Font>>>,
    mut roots: Query<(Entity, &mut SpuiState, &ComputedNode)>,
    mut windows: Query<
        (
            &mut Node,
            &mut Visibility,
            Option<&mut BackgroundColor>,
            &mut WinPaint,
            Option<&mut UiTransform>,
        ),
        (With<SpuiWindow>, Without<SpuiPaint>, Without<SpuiBox>),
    >,
    mut paints: PaintQuery,
    mut mats: Query<
        (&mut Node, &mut MaterialNode<SpuiCopiesMaterial>, &mut Visibility, &mut ZIndex),
        (With<SpuiMat>, Without<SpuiWindow>, Without<SpuiPaint>, Without<SpuiBox>),
    >,
    mut materials: Option<ResMut<Assets<SpuiCopiesMaterial>>>,
    mut boxes: Query<
        (&mut Node, &mut Visibility),
        (With<SpuiBox>, Without<SpuiWindow>, Without<SpuiPaint>),
    >,
    mut texts: Query<(
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut TextLayout,
        &mut LineHeight,
        &ComputedNode,
    )>,
    mut spans: Query<(&mut TextSpan, &mut TextFont, &mut TextColor, &mut LineHeight), Without<Text>>,
    mut stats: Local<SyncStats>,
) {
    let (Some(mut images), Some(mut fonts)) = (images, fonts) else {
        return;
    };
    if originals.resource_index.records.is_empty() {
        return;
    }
    assets.ensure_fonts(&originals.resource_index, &mut fonts);
    let styles = assets.styles(&originals.resource_index).clone();
    let mut stat_text_copies = 0usize;
    let use_mask = materials.is_some() && super::surface::gamma_surface() && !text_copies_forced();
    super::paint::OUTLINE_IMAGE_COPIES.store(0, std::sync::atomic::Ordering::Relaxed);
    for (_root_entity, mut state, computed) in &mut roots {
        let size = computed.size() * computed.inverse_scale_factor();
        if size.x < 1.0 || size.y < 1.0 {
            continue;
        }
        let layout = state.layout.clone();
        if !state.ready {
            assets.preload(&originals.resource_index, &mut images, &layout);
        }
        let screen = [size.x, size.y];
        if state.screen != screen {
            state.screen = screen;
            state.layout_dirty = true;
        }
        state.time += time.delta_secs();
        state.step_effects(time.delta_secs());
        if state.layout_dirty {
            let rects = solve::solve(&layout, &state.areas, screen, state.policy);
            state.rects = rects;
            state.layout_dirty = false;
        }
        state.ready = true;
        let (shade, visible) = state.effective();
        let textures: HashMap<ResourceKey, Tex> = assets
            .textures
            .iter()
            .filter_map(|(k, t)| t.as_ref().map(|t| (*k, t.clone())))
            .collect();
        let size_map: HashMap<ResourceKey, [u32; 2]> =
            textures.iter().map(|(k, t)| (*k, t.size)).collect();
        let sizes = |k: &ResourceKey| size_map.get(k).copied();
        for &id in &state.preorder.clone() {
            let w = &layout.windows[id];
            let entity = state.entities[id];
            let Ok((mut node, mut vis, fill, mut paint, transform)) = windows.get_mut(entity) else {
                continue;
            };
            let rect = state.rects[id];
            let parent_rect = w.parent.map_or(SRect::new(0.0, 0.0, 0.0, 0.0), |p| state.rects[p]);
            set_px(
                &mut node,
                rect.x1 - parent_rect.x1,
                rect.y1 - parent_rect.y1,
                rect.width(),
                rect.height(),
            );
            let want = if visible[id] { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
            // Visibility effects (inflate, glide) and a runtime rotation.
            let pose = state.poses[id];
            // A static half-turn about the vertical axis (`Rotate` effect,
            // axis (0,1,0), 180 degrees, trigger 2) mirrors the window.
            let mirror = mirrored(&layout.windows[id]);
            let want_transform = if pose == Pose::REST && state.rotations[id] == 0.0 && !mirror {
                UiTransform::IDENTITY
            } else {
                UiTransform {
                    translation: Val2::px(pose.offset[0], pose.offset[1]),
                    scale: Vec2::new(if mirror { -pose.scale } else { pose.scale }, pose.scale),
                    rotation: Rot2::radians(state.rotations[id]),
                }
            };
            match transform {
                Some(mut t) => {
                    if *t != want_transform {
                        *t = want_transform;
                    }
                }
                None if want_transform != UiTransform::IDENTITY => {
                    commands.entity(entity).insert(want_transform);
                }
                None => {}
            }
            if !visible[id] {
                // Outline copies of a window that stays hidden are despawned after a while.
                if !paint.outline_text.is_empty() || paint.mask.is_some() {
                    paint.hidden_frames += 1;
                    if paint.hidden_frames > GHOST_GRACE_FRAMES {
                        paint.hidden_frames = 0;
                        for copy in std::mem::take(&mut paint.outline_text) {
                            commands.entity(copy.text).despawn();
                        }
                        drop_caption_copies(&mut commands, &mut paint);
                    }
                }
                continue;
            }
            if paint.hidden_frames != 0 {
                paint.hidden_frames = 0;
            }
            let st = WinState {
                flags: state.flags[id],
                state: state.states[id],
            };
            let clip = state.flags[id] & flag::CLIP != 0;
            let want_overflow = if clip { Overflow::clip() } else { Overflow::visible() };
            if node.overflow != want_overflow {
                node.overflow = want_overflow;
            }
            let own = mul_argb(shade[id], state.tints[id]);
            let overridden;
            let w = match &state.image_overrides[id] {
                Some(image) => {
                    overridden = Window {
                        drawable: Some(super::model::Drawable::Image(super::model::ImageDrawable {
                            image: Some(image.clone()),
                            ..Default::default()
                        })),
                        ..w.clone()
                    };
                    &overridden
                }
                None => match (&state.icon_overrides[id], &w.drawable) {
                    (Some(icon), Some(super::model::Drawable::SporeStd { base, infos })) => {
                        let mut infos = infos.clone();
                        for info in infos.iter_mut().flatten() {
                            info.icon = Some(icon.clone());
                        }
                        overridden = Window {
                            drawable: Some(super::model::Drawable::SporeStd {
                                base: base.clone(),
                                infos,
                            }),
                            ..w.clone()
                        };
                        &overridden
                    }
                    _ => w,
                },
            };
            // A slider value set by game code replaces the authored one.
            let ranged;
            let w = match (state.ranges[id], &w.extra) {
                (Some(f), Extra::Range(r)) => {
                    let (min, max) = if r.max != r.min { (r.min, r.max) } else { (0, 1000) };
                    let value = min + ((max - min) as f32 * f).round() as i32;
                    ranged = Window { extra: Extra::Range(super::model::RangeData { value, min, max, ..r.clone() }), ..w.clone() };
                    &ranged
                }
                _ => w,
            };
            // A `SporeStd` drawable ignores the window's own shade (see `SpuiState::sporestd_shade`).
            let own = if matches!(w.drawable, Some(super::model::Drawable::SporeStd { .. })) {
                mul_argb(state.sporestd_shade(id, &shade), state.tints[id])
            } else {
                own
            };
            let mut cmds = paint::window_commands(w, rect, st, own, &sizes);
            if materials.is_none() || !super::surface::gamma_surface() {
                cmds = paint::expand_copies(cmds);
            }
            let mut fill_color = Color::NONE;
            let mut image_cmds: Vec<&Cmd> = Vec::new();
            for c in &cmds {
                match c {
                    Cmd::Fill { color, .. } => fill_color = argb_color(*color),
                    Cmd::Image { .. } | Cmd::Copies { .. } => image_cmds.push(c),
                }
            }
            let mut anim_cmd: Option<Cmd> = None;
            if let Extra::Anim(a) = &w.extra {
                let grid_anim;
                let a = match state.frame_grids[id] {
                    Some((columns, rows)) => {
                        grid_anim = AnimData { columns, rows, frame_count: 0, ..a.clone() };
                        &grid_anim
                    }
                    None => a,
                };
                if let Some(image) = &a.image {
                    // Icons that do not auto play rest on their first frame.
                    let t = if a.auto_play { state.time } else { 0.0 };
                    let src = match state.frame_overrides[id] {
                        Some(frame) => paint::flipbook_frame_index(a, image, frame, &sizes),
                        None => paint::flipbook_frame(a, image, t, &sizes),
                    };
                    if let Some(src) = src {
                        anim_cmd = Some(Cmd::Image {
                            file: image.file,
                            src,
                            dest: SRect::new(0.0, 0.0, rect.width(), rect.height()),
                            color: shade[id],
                            mode: Mode::Stretch,
                        });
                    }
                }
            }
            if let Some(c) = anim_cmd.as_ref() {
                image_cmds.push(c);
            }
            if let Some(mut bg) = fill {
                if bg.0 != fill_color {
                    bg.0 = fill_color;
                }
            }
            let WinPaint { pool, mat_pool, overlay, text, outline_text, outline_taps, hidden_frames, ghost_wait, mask } = &mut *paint;
            update_pool(&mut commands, entity, Z_PAINT, pool, mat_pool, &image_cmds, &mut paints, &mut mats, &mut materials, &textures);

            // Clipped overlay: the window and its children again with another
            // colour over the left part (the filled part of a progress segment).
            let wanted_overlay = state.overlays[id].filter(|(f, _)| *f > 0.0);
            if wanted_overlay.is_some() && overlay.is_none() {
                let boxed = commands
                    .spawn((
                        SpuiBox,
                        Node {
                            position_type: PositionType::Absolute,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Visibility::Hidden,
                        ZIndex(if state.overlays_front[id] { Z_WINDOW + 1 } else { Z_OVERLAY }),
                        Pickable::IGNORE,
                        ChildOf(entity),
                    ))
                    .id();
                *overlay = Some((boxed, Vec::new()));
            }
            if let Some((boxed, opool)) = overlay.as_mut() {
                match wanted_overlay {
                    Some((fraction, color)) => {
                        let recolored = overlay_commands(&layout, &state, id, color, &shade, &visible, &sizes);
                        let cmds: Vec<&Cmd> = recolored.iter().collect();
                        if let Ok((mut bn, mut bv)) = boxes.get_mut(*boxed) {
                            set_px(&mut bn, 0.0, 0.0, rect.width() * fraction.clamp(0.0, 1.0), rect.height());
                            if *bv != Visibility::Inherited {
                                *bv = Visibility::Inherited;
                            }
                        }
                        update_pool(&mut commands, *boxed, Z_PAINT, opool, &mut Vec::new(), &cmds, &mut paints, &mut mats, &mut materials, &textures);
                    }
                    None => {
                        if let Ok((_, mut bv)) = boxes.get_mut(*boxed) {
                            if *bv != Visibility::Hidden {
                                *bv = Visibility::Hidden;
                            }
                        }
                    }
                }
            }

            // Text.
            if !matches!(w.kind, WindowKind::Text | WindowKind::Button | WindowKind::TextEdit) {
                continue;
            }
            let content = state.captions[id]
                .clone()
                .or_else(|| w.caption.as_ref().map(|c| assets.text(&originals.resource_index, c)))
                .unwrap_or_default();
            let style = styles.style(w.text_style);
            if text.is_none() && !content.is_empty() {
                *text = Some(spawn_text_nodes(&mut commands, entity, Z_TEXT));
            }
            let Some(nodes) = text.as_mut() else {
                continue;
            };
            // The "Text Outline" of the window: the retail text path (0x96E690) draws the caption
            // once per kernel copy, shifted and tinted with the outline colour, before the
            // caption itself. One more text node per copy inside the caption's box.
            let outline = match &w.extra {
                _ if std::env::var_os("OS_SPUI_NO_TEXT_OUTLINE").is_some() => None,
                Extra::Button(b) => b.outline.as_ref(),
                Extra::Text(t) => t.outline.as_ref(),
                _ => None,
            };
            if outline_taps.is_none() {
                *outline_taps = Some(Arc::new(outline.map(|o| o.taps()).unwrap_or_default()));
            }
            let taps = outline_taps.clone().unwrap_or_default();
            let wanted = match outline {
                Some(_) if !content.is_empty() => taps.len(),
                _ => 0,
            };
            stat_text_copies += wanted;
            // One node and a glyph mask per caption instead of one text node per copy, where the
            // copies material exists and the kernel fits its tap array.
            let masked = use_mask && !taps.is_empty() && taps.len() <= paint::MAX_COPIES;
            if masked {
                if let Some(o) = outline {
                    if wanted > 0 {
                        *hidden_frames = 0;
                        let box_size = [rect.width().ceil().max(1.0) as u32, rect.height().ceil().max(1.0) as u32];
                        sync_caption_copies(
                            &mut commands,
                            mask,
                            nodes,
                            o,
                            &taps,
                            CopiesKey { box_size, pad: 0, shade: shade[id] },
                            &mut images,
                            materials.as_deref_mut(),
                            &mut mats,
                        );
                    } else if mask.is_some() {
                        // An empty caption keeps its node for a while like a hidden window.
                        *hidden_frames += 1;
                        if *hidden_frames > GHOST_GRACE_FRAMES {
                            *hidden_frames = 0;
                            drop_caption_copies_of(&mut commands, mask, nodes.inner);
                        }
                    }
                }
            } else if outline.is_some() && content.is_empty() {
                // An empty caption keeps its copies for a while like a hidden window.
                if !outline_text.is_empty() {
                    *hidden_frames += 1;
                    if *hidden_frames > GHOST_GRACE_FRAMES {
                        *hidden_frames = 0;
                        for copy in outline_text.drain(..) {
                            commands.entity(copy.text).despawn();
                        }
                    }
                }
            } else if wanted > 0 {
                *hidden_frames = 0;
                while outline_text.len() > wanted {
                    if let Some(copy) = outline_text.pop() {
                        commands.entity(copy.text).despawn();
                    }
                }
                if outline_text.is_empty() {
                    *ghost_wait = GHOST_WAIT_FRAMES;
                }
                while outline_text.len() < wanted {
                    let tap = taps[outline_text.len()];
                    outline_text.push(spawn_ghost(&mut commands, nodes.outer, tap.dx as f32, tap.dy as f32));
                }
            }
            // The caption box: the caption offset only applies while pressed/selected.
            let (border, offset) = match &w.extra {
                Extra::Button(b) if st.clicked() || st.selected() => (b.caption_border, b.caption_offset),
                Extra::Button(b) => (b.caption_border, [0.0, 0.0]),
                Extra::Text(t) => (t.border, [0.0, 0.0]),
                _ => ([0.0; 4], [0.0; 2]),
            };
            // Font runs: the style's family first, then the sheet's font order
            // (the DNA symbol of SporeCurrencyEA inside a Trebuchet caption).
            let runs = fonts::split_runs(&content, &style.family, assets.font_infos());
            let em = state.text_sizes[id]
                .unwrap_or(style.size_px * FONT_SCALE)
                .round()
                .max(1.0);
            let line_height_of = |family: &str| -> f32 {
                assets
                    .font_infos()
                    .iter()
                    .find(|f| f.family == family)
                    .map_or(em * 1.2, |f| f.line_height(em))
                    + style.line_spacing
            };
            let spec = TextBoxSpec {
                offset,
                size: [rect.width(), rect.height()],
                inner_width: (rect.width() - border[0] - border[2]).max(0.0),
                padding: UiRect {
                    left: Val::Px(border[0]),
                    top: Val::Px(border[1]),
                    right: Val::Px(border[2]),
                    bottom: Val::Px(border[3]),
                },
                justify_content: match style.align {
                    HAlign::Center => JustifyContent::Center,
                    HAlign::Right => JustifyContent::FlexEnd,
                    _ => JustifyContent::FlexStart,
                },
                align_items: match style.valign {
                    VAlign::Top => AlignItems::FlexStart,
                    VAlign::Bottom => AlignItems::FlexEnd,
                    VAlign::Middle => AlignItems::Center,
                },
                visible: !content.is_empty() && *ghost_wait == 0,
                layout: TextLayout::new(
                    match style.align {
                        HAlign::Center => Justify::Center,
                        HAlign::Right => Justify::Right,
                        _ => Justify::Left,
                    },
                    if style.no_wrap { LineBreak::NoWrap } else { LineBreak::WordBoundary },
                ),
                em,
                runs: runs
                    .iter()
                    .map(|(family, run)| RunSpec {
                        font: assets.font_source(family),
                        line_height: line_height_of(family),
                        text: run.clone(),
                    })
                    .collect(),
                default_font: assets.font_source(&style.family),
                default_line_height: line_height_of(&style.family),
            };
            let color = if hide_caption() {
                Color::NONE
            } else {
                argb_color(mul_argb(text_color_for(w, st), shade[id]))
            };
            let extent = update_text_nodes(
                &mut commands,
                nodes,
                &spec,
                color,
                &mut boxes,
                &mut texts,
                &mut spans,
            );
            if let Some(extent) = extent {
                let extent = (!content.is_empty()).then_some(extent);
                if state.text_extents[id] != extent {
                    state.text_extents[id] = extent;
                }
            }
            if let Some(o) = outline.filter(|_| wanted > 0 && !masked) {
                for (copy, tap) in outline_text.iter_mut().zip(taps.iter()) {
                    let tint = argb_color(mul_argb(o.tap_color(tap), shade[id]));
                    update_ghost(&mut commands, copy, &spec, tint, &mut boxes, &mut texts, &mut spans);
                }
            }
            if *ghost_wait > 0 {
                *ghost_wait -= 1;
            }
        }
    }
    if stats_enabled() {
        let spawned: usize = windows.iter().map(|(_, _, _, p, _)| p.outline_text.len()).sum();
        let ghost_spans: usize = windows
            .iter()
            .map(|(_, _, _, p, _)| p.outline_text.iter().map(|g| g.spans.len()).sum::<usize>())
            .sum();
        TEXT_WRITES.store(ghost_spans, std::sync::atomic::Ordering::Relaxed);
        let masks = windows.iter().filter(|(_, _, _, p, _)| p.mask.is_some()).count();
        stats.record(
            stat_text_copies,
            spawned,
            super::paint::OUTLINE_IMAGE_COPIES.load(std::sync::atomic::Ordering::Relaxed),
            masks,
        );
    }
}

/// Diagnostics for `OS_SPUI_STATS=1`: the wall time between two `sync` runs (one frame, in a
/// capture the whole `App::update`) and the number of outline copies on screen, printed every
/// 100 frames as `SPUI_STATS ...`. `OS_SPUI_NO_OUTLINE=1` ([`super::outline::disabled`]) turns
/// the stroke / halo / outline copies off to compare the frame time.
#[derive(Default)]
pub struct SyncStats {
    last: Option<std::time::Instant>,
    frame: usize,
    intervals: Vec<f32>,
}

/// Text node content writes since the last stats line (diagnostics, `OS_SPUI_STATS`).
static DUMP_COPIES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static TEXT_WRITES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn stats_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OS_SPUI_STATS").is_some())
}

/// Diagnostics (`OS_SPUI_STATS`): at frame 300 prints how many text nodes the per-copy path made
/// and how many glyph quads they and the captions themselves carry (Bevy rebuilds the vertices of
/// every glyph quad each frame, so the cost of a text node follows its glyph count).
fn stats_glyphs(
    texts: Query<(&bevy::text::TextLayoutInfo, Has<SpuiBox>, &ChildOf), With<Text>>,
    boxes: Query<(), With<SpuiBox>>,
    masks: Query<(), With<CaptionMask>>,
    mut frame: Local<u32>,
) {
    *frame += 1;
    if *frame != 300 {
        return;
    }
    let (mut ghost, mut live) = ((0usize, 0usize), (0usize, 0usize));
    for (info, is_ghost, parent) in &texts {
        if is_ghost {
            ghost.0 += 1;
            ghost.1 += info.glyphs.len();
        } else if boxes.contains(parent.parent()) {
            live.0 += 1;
            live.1 += info.glyphs.len();
        }
    }
    println!(
        "SPUI_GLYPHS ghost_texts={} ghost_glyphs={} caption_texts={} caption_glyphs={} masks={}",
        ghost.0,
        ghost.1,
        live.0,
        live.1,
        masks.iter().count()
    );
}

impl SyncStats {
    fn record(&mut self, text_copies: usize, spawned: usize, image_copies: usize, masks: usize) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last.replace(now) {
            self.intervals.push((now - last).as_secs_f32() * 1000.0);
        }
        self.frame += 1;
        DUMP_COPIES.store(self.frame == 300, std::sync::atomic::Ordering::Relaxed);
        if self.frame % 100 == 0 && self.intervals.len() >= 50 {
            let mut v = self.intervals.clone();
            v.sort_by(|a, b| a.total_cmp(b));
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            println!(
                "SPUI_STATS frame={} mean_ms={:.2} median_ms={:.2} p90_ms={:.2} outline_text_active={} outline_text_spawned={} outline_image_copies={} ghost_spans={} caption_masks={}",
                self.frame,
                mean,
                v[v.len() / 2],
                v[v.len() * 9 / 10],
                text_copies,
                spawned,
                image_copies,
                TEXT_WRITES.swap(0, std::sync::atomic::Ordering::Relaxed),
                masks
            );
            self.intervals.clear();
        }
    }
}

/// Spawns the text box of a window: the flex box, its root `Text` and (later) one span per
/// further font run.
fn spawn_text_nodes(commands: &mut Commands, parent: Entity, z: i32) -> TextNodes {
    let inner = commands
        .spawn((
            Text::new(""),
            TextFont::default(),
            TextColor(Color::WHITE),
            TextLayout::default(),
            Node {
                max_width: Val::Percent(100.0),
                ..default()
            },
            // Above the outline copies (z 0) of the same box.
            ZIndex(1),
            Pickable::IGNORE,
        ))
        .id();
    let outer = commands
        .spawn((
            SpuiBox,
            Node {
                position_type: PositionType::Absolute,
                display: Display::Flex,
                ..default()
            },
            Visibility::Inherited,
            ZIndex(z),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.entity(inner).insert(ChildOf(outer));
    TextNodes {
        outer,
        inner,
        spans: Vec::new(),
    }
}

/// `OS_SPUI_HIDE_CAPTION=1` makes the caption texts transparent so that only their outline copies
/// show (diagnostics for comparing the two copy paths).
fn hide_caption() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OS_SPUI_HIDE_CAPTION").is_some())
}

/// `OS_SPUI_TEXT_COPIES=1` draws the copies of an outlined caption as one text node each (the
/// per-copy path that is also used without the copies material) instead of one node and a glyph
/// mask per caption; for before / after comparisons.
fn text_copies_forced() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OS_SPUI_TEXT_COPIES").is_some())
}

/// Uniform block of a caption's copies node: the kernel's taps (alpha times the window shade),
/// the tint colour and the margin the node has around the mask.
fn caption_params(
    outline: &super::outline::Outline,
    taps: &[super::outline::Tap],
    shade: u32,
    margin: u32,
) -> super::surface::CopiesParams {
    let mut params = super::surface::CopiesParams::default();
    params.info = Vec4::new(margin as f32, taps.len().min(paint::MAX_COPIES) as f32, 0.0, 0.0);
    // The tint of every copy is the outline colour times the shade, quantised like a single draw
    // would (the rgb part is the same for all copies).
    let rgb = taps.first().map_or(0, |t| mul_argb(outline.tap_color(t), shade));
    params.color = Vec4::new(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
        0.0,
    );
    for (slot, tap) in params.taps.iter_mut().zip(taps.iter()) {
        let alpha = (mul_argb(outline.tap_color(tap), shade) >> 24) & 0xFF;
        *slot = Vec4::new(tap.dx as f32, tap.dy as f32, alpha as f32 / 255.0, 0.0);
    }
    params
}

/// Rectangle of a caption's copies node inside the caption box: the mask (box plus pad) grown by
/// the largest shift of a copy, in box pixels.
fn copies_rect(key: &CopiesKey, margin: u32) -> (f32, f32, f32, f32) {
    let layer = CaptionMask::layer_size(key.box_size, key.pad);
    let inset = (key.pad + margin) as f32;
    (-inset, -inset, layer[0] as f32 + 2.0 * margin as f32, layer[1] as f32 + 2.0 * margin as f32)
}

/// Keeps the copies node of an outlined caption (see [`super::caption`]) in step: spawns it with
/// its mask image, resizes it when the caption box or the mask pad changes, retints it when the
/// window's shade changes.
#[allow(clippy::too_many_arguments)]
fn sync_caption_copies(
    commands: &mut Commands,
    copies: &mut Option<CaptionCopies>,
    nodes: &TextNodes,
    outline: &super::outline::Outline,
    taps: &[super::outline::Tap],
    key: CopiesKey,
    images: &mut Assets<Image>,
    materials: Option<&mut Assets<SpuiCopiesMaterial>>,
    mats: &mut Query<
        (&mut Node, &mut MaterialNode<SpuiCopiesMaterial>, &mut Visibility, &mut ZIndex),
        (With<SpuiMat>, Without<SpuiWindow>, Without<SpuiPaint>, Without<SpuiBox>),
    >,
) {
    let Some(materials) = materials else {
        return;
    };
    let margin = taps.iter().map(|t| t.dx.abs().max(t.dy.abs())).max().unwrap_or(0) as u32;
    match copies {
        None => {
            let needed_pad = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let layer = CaptionMask::layer_size(key.box_size, key.pad);
            let image = images.add(caption::blank_image(layer[0], layer[1]));
            let material = materials.add(SpuiCopiesMaterial {
                image: image.clone(),
                params: caption_params(outline, taps, key.shade, margin),
            });
            let mut node = abs_node();
            let (x, y, w, h) = copies_rect(&key, margin);
            set_px(&mut node, x, y, w, h);
            let entity = commands
                .spawn((
                    SpuiMat,
                    node,
                    MaterialNode(material.clone()),
                    Visibility::Inherited,
                    // Behind the caption text (z 1) of the same box.
                    ZIndex(0),
                    Pickable::IGNORE,
                    ChildOf(nodes.outer),
                ))
                .id();
            commands.entity(nodes.inner).insert(CaptionMask::new(
                nodes.outer,
                image.clone(),
                material.clone(),
                key.box_size,
                key.pad,
                needed_pad.clone(),
            ));
            *copies = Some(CaptionCopies { node: entity, material, image, needed_pad, applied: key });
        }
        Some(c) => {
            // Text that reaches outside its box makes the mask grow (and shrink again with it).
            let pad = c.needed_pad.load(std::sync::atomic::Ordering::Relaxed);
            let key = CopiesKey { pad, ..key };
            if key == c.applied {
                return;
            }
            if key.box_size != c.applied.box_size || key.pad != c.applied.pad {
                if let Ok((mut node, _, _, _)) = mats.get_mut(c.node) {
                    let (x, y, w, h) = copies_rect(&key, margin);
                    set_px(&mut node, x, y, w, h);
                }
                commands.entity(nodes.inner).insert(CaptionMask::new(
                    nodes.outer,
                    c.image.clone(),
                    c.material.clone(),
                    key.box_size,
                    key.pad,
                    c.needed_pad.clone(),
                ));
            }
            if key.shade != c.applied.shade {
                let params = caption_params(outline, taps, key.shade, margin);
                if let Some(mut m) = materials.get_mut(&c.material) {
                    m.params = params;
                }
            }
            c.applied = key;
        }
    }
}

/// Removes the copies node of a caption and its mask.
fn drop_caption_copies_of(commands: &mut Commands, copies: &mut Option<CaptionCopies>, inner: Entity) {
    if let Some(c) = copies.take() {
        commands.entity(c.node).despawn();
        commands.entity(inner).remove::<CaptionMask>();
    }
}

fn drop_caption_copies(commands: &mut Commands, paint: &mut WinPaint) {
    if let Some(inner) = paint.text.as_ref().map(|t| t.inner) {
        drop_caption_copies_of(commands, &mut paint.mask, inner);
    }
}

/// Spawns one copy of a caption's outline: a text node inside the caption box (`parent`), out of
/// the flex flow (its static position is the caption's), shifted by `(dx, dy)` pixels.
fn spawn_ghost(commands: &mut Commands, parent: Entity, dx: f32, dy: f32) -> GhostText {
    let text = commands
        .spawn((
            SpuiBox,
            Text::new(""),
            TextFont::default(),
            TextColor(Color::WHITE),
            TextLayout::default(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            UiTransform {
                translation: Val2::px(dx, dy),
                ..UiTransform::IDENTITY
            },
            ZIndex(0),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    GhostText { text, spans: Vec::new() }
}

/// One font run of a caption.
struct RunSpec {
    font: bevy::text::FontSource,
    line_height: f32,
    text: String,
}

/// Everything the text boxes of one window (the caption and its outline copies) share.
struct TextBoxSpec {
    offset: [f32; 2],
    size: [f32; 2],
    /// Width of the box inside its padding (the `max_width` of the caption text).
    inner_width: f32,
    padding: UiRect,
    justify_content: JustifyContent,
    align_items: AlignItems,
    visible: bool,
    layout: TextLayout,
    em: f32,
    runs: Vec<RunSpec>,
    default_font: bevy::text::FontSource,
    default_line_height: f32,
}

/// Brings the caption's text box in line with `spec` and `color`. Returns the laid-out size of the first run once the text node
/// exists.
#[allow(clippy::too_many_arguments)]
fn update_text_nodes(
    commands: &mut Commands,
    nodes: &mut TextNodes,
    spec: &TextBoxSpec,
    color: Color,
    boxes: &mut Query<
        (&mut Node, &mut Visibility),
        (With<SpuiBox>, Without<SpuiWindow>, Without<SpuiPaint>),
    >,
    texts: &mut Query<(
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut TextLayout,
        &mut LineHeight,
        &ComputedNode,
    )>,
    spans: &mut Query<(&mut TextSpan, &mut TextFont, &mut TextColor, &mut LineHeight), Without<Text>>,
) -> Option<Vec2> {
    let (outer, inner) = (nodes.outer, nodes.inner);
    if let Ok((mut bn, mut bv)) = boxes.get_mut(outer) {
        set_px(&mut bn, spec.offset[0], spec.offset[1], spec.size[0], spec.size[1]);
        if bn.padding != spec.padding {
            bn.padding = spec.padding;
        }
        if bn.justify_content != spec.justify_content {
            bn.justify_content = spec.justify_content;
        }
        if bn.align_items != spec.align_items {
            bn.align_items = spec.align_items;
        }
        let want_vis = if spec.visible { Visibility::Inherited } else { Visibility::Hidden };
        if *bv != want_vis {
            *bv = want_vis;
        }
    }
    update_text_runs(commands, inner, &mut nodes.spans, spec, color, texts, spans)
}

/// Brings the text node `inner` and its further font runs in line with `spec` and `color`.
/// Returns the laid-out size of the first run once the text node exists.
fn update_text_runs(
    commands: &mut Commands,
    inner: Entity,
    span_list: &mut Vec<Entity>,
    spec: &TextBoxSpec,
    color: Color,
    texts: &mut Query<(
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut TextLayout,
        &mut LineHeight,
        &ComputedNode,
    )>,
    spans: &mut Query<(&mut TextSpan, &mut TextFont, &mut TextColor, &mut LineHeight), Without<Text>>,
) -> Option<Vec2> {
    let mut extent = None;
    if let Ok((mut t, mut tf, mut tc, mut tl, mut lh, computed)) = texts.get_mut(inner) {
        let first = spec.runs.first();
        let first_text = first.map_or("", |r| r.text.as_str());
        if t.0 != first_text {
            t.0 = first_text.to_string();
            TEXT_WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let font = first.map_or(&spec.default_font, |r| &r.font);
        if tf.font != *font {
            tf.font = font.clone();
        }
        let px = bevy::text::FontSize::Px(spec.em);
        if tf.font_size != px {
            tf.font_size = px;
        }
        let height = LineHeight::Px(first.map_or(spec.default_line_height, |r| r.line_height));
        if *lh != height {
            *lh = height;
        }
        if tc.0 != color {
            tc.0 = color;
        }
        if tl.justify != spec.layout.justify || tl.linebreak != spec.layout.linebreak {
            *tl = spec.layout.clone();
        }
        extent = Some(computed.size() * computed.inverse_scale_factor());
    }
    // One span per further run.
    let extra = spec.runs.len().saturating_sub(1);
    while span_list.len() < extra {
        let span = commands
            .spawn((
                TextSpan::new(""),
                TextFont::default(),
                TextColor(Color::WHITE),
                LineHeight::default(),
                ChildOf(inner),
            ))
            .id();
        span_list.push(span);
    }
    while span_list.len() > extra {
        if let Some(span) = span_list.pop() {
            commands.entity(span).despawn();
        }
    }
    for (span, run) in span_list.iter().zip(spec.runs.iter().skip(1)) {
        let Ok((mut ts, mut tf, mut tc, mut lh)) = spans.get_mut(*span) else {
            continue;
        };
        if ts.0 != run.text {
            ts.0 = run.text.clone();
        }
        if tf.font != run.font {
            tf.font = run.font.clone();
        }
        let px = bevy::text::FontSize::Px(spec.em);
        if tf.font_size != px {
            tf.font_size = px;
        }
        let height = LineHeight::Px(run.line_height);
        if *lh != height {
            *lh = height;
        }
        if tc.0 != color {
            tc.0 = color;
        }
    }
    extent
}

/// Brings one outline copy of a caption in line with `spec`, tinted `color`.
fn update_ghost(
    commands: &mut Commands,
    copy: &mut GhostText,
    spec: &TextBoxSpec,
    color: Color,
    boxes: &mut Query<
        (&mut Node, &mut Visibility),
        (With<SpuiBox>, Without<SpuiWindow>, Without<SpuiPaint>),
    >,
    texts: &mut Query<(
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut TextLayout,
        &mut LineHeight,
        &ComputedNode,
    )>,
    spans: &mut Query<(&mut TextSpan, &mut TextFont, &mut TextColor, &mut LineHeight), Without<Text>>,
) {
    if let Ok((mut node, _)) = boxes.get_mut(copy.text) {
        // The caption's text is as wide as the box inside its padding at most.
        let width = Val::Px(spec.inner_width);
        if node.max_width != width {
            node.max_width = width;
        }
    }
    update_text_runs(commands, copy.text, &mut copy.spans, spec, color, texts, spans);
}

/// Pointer used for hover/press. Real play reads the primary window cursor and
/// the left mouse button; tests and captures can override both.
#[derive(Resource, Default)]
pub struct SpuiPointer {
    pub position_override: Option<Vec2>,
    pub pressed_override: Option<bool>,
    /// Pointer position of the last frame (logical pixels), if any.
    pub position: Option<Vec2>,
    pub pressed: bool,
    /// The pointer is over a visible interactive window of a layout.
    pub over_ui: bool,
}

/// Interactive windows can be hovered, pressed and clicked.
fn interactive(w: &Window) -> bool {
    matches!(w.kind, WindowKind::Button)
}

/// Topmost interactive window under `p` (front-most sibling first, children
/// before their parent; hit masks of the window's drawable are honoured).
pub fn hit_test(state: &SpuiState, visible: &[bool], p: Vec2) -> Option<WinId> {
    fn walk(state: &SpuiState, visible: &[bool], id: WinId, p: Vec2) -> Option<WinId> {
        if !visible[id] {
            return None;
        }
        let w = &state.layout.windows[id];
        let flags = state.flags[id];
        if flags & flag::IGNORE_MOUSE_CHILDREN == 0 {
            for &c in &w.children {
                if let Some(hit) = walk(state, visible, c, p) {
                    return Some(hit);
                }
            }
        }
        if flags & flag::IGNORE_MOUSE != 0 || !interactive(w) {
            return None;
        }
        let r = state.rects[id];
        if !r.contains(p.x, p.y) {
            return None;
        }
        let mask = match &w.drawable {
            Some(Drawable::Std(d)) => d.hit_mask,
            Some(Drawable::SporeStd { base, .. }) => base.hit_mask,
            _ => None,
        };
        if let Some(HitMask::Rle { width, height, .. }) = mask.and_then(|m| state.layout.hit_masks.get(m)) {
            let u = (p.x - r.x1) / r.width().max(1e-3) * *width as f32;
            let v = (p.y - r.y1) / r.height().max(1e-3) * *height as f32;
            let m = &state.layout.hit_masks[mask.unwrap()];
            if !m.covers(u.floor() as i32, v.floor() as i32) {
                return None;
            }
        }
        Some(id)
    }
    for &root in &state.layout.roots {
        if let Some(hit) = walk(state, visible, root, p) {
            return Some(hit);
        }
    }
    None
}

/// The window carries a static mirror: a `Rotate` effect of 180 degrees about
/// the vertical axis with trigger 2 (the halves of the new-game stage wheel,
/// card header bars).
pub fn mirrored(w: &Window) -> bool {
    w.procs.iter().any(|p| {
        matches!(p, super::model::WinProc::Effect { bi, kind: super::model::EffectKind::Rotate { axis, angle } }
            if bi.trigger == 2 && axis[1].abs() > 0.5 && axis[0].abs() < 0.5 && axis[2].abs() < 0.5
                && (angle.abs() - 180.0).abs() < 1.0)
    })
}

/// Hover, press and click handling of every spawned layout.
pub fn input(
    mut pointer: ResMut<SpuiPointer>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    mut roots: Query<(Entity, &mut SpuiState)>,
    mut held: Local<Option<(Entity, WinId)>>,
) {
    // The gamma surface is laid out in physical pixels (no DPI scaling, like
    // the original), the plain presentation in logical ones.
    let physical = super::surface::gamma_surface();
    let cursor = pointer.position_override.or_else(|| {
        windows.iter().next().and_then(|w| {
            if physical {
                w.physical_cursor_position()
            } else {
                w.cursor_position()
            }
        })
    });
    let pressed = pointer
        .pressed_override
        .unwrap_or_else(|| mouse.as_ref().is_some_and(|m| m.pressed(MouseButton::Left)));
    let was_pressed = pointer.pressed;
    pointer.position = cursor;
    pointer.pressed = pressed;
    pointer.over_ui = false;
    for (this_root, mut state) in &mut roots {
        state.activations.clear();
        if !state.ready {
            continue;
        }
        // Windows fading out are no longer interactive.
        let visible = state.interactive();
        let hover = cursor.and_then(|p| hit_test(&state, &visible, p));
        pointer.over_ui |= hover.is_some();
        let n = state.layout.windows.len();
        // Hover bit follows the pointer; the press bit follows the button.
        for id in 0..n {
            let want_hover = hover == Some(id);
            let has = state.states[id] & super::ids::state::HOVER != 0;
            if want_hover != has {
                if want_hover {
                    state.states[id] |= super::ids::state::HOVER;
                } else {
                    state.states[id] &= !super::ids::state::HOVER;
                }
            }
        }
        if pressed && !was_pressed {
            if let Some(id) = hover {
                if state.flags[id] & flag::ENABLED != 0 {
                    state.states[id] |= super::ids::state::CLICKED;
                    *held = Some((this_root, id));
                    let b = state.layout.windows[id].button().cloned();
                    if b.is_some_and(|b| b.flags & super::ids::button_flag::TRIGGER_ON_DOWN != 0) {
                        click(&mut state, id);
                    }
                }
            }
        }
        if let Some((root, id)) = *held {
            if root == this_root && !pressed {
                state.states[id] &= !super::ids::state::CLICKED;
                let trigger_on_down = state.layout.windows[id]
                    .button()
                    .is_some_and(|b| b.flags & super::ids::button_flag::TRIGGER_ON_DOWN != 0);
                if hover == Some(id) && !trigger_on_down {
                    click(&mut state, id);
                }
                *held = None;
            }
        }
    }
}

/// Apply a click: toggle/radio selection, then record it for the game.
fn click(state: &mut SpuiState, id: WinId) {
    let Some(b) = state.layout.windows[id].button().cloned() else {
        return;
    };
    use super::ids::state::SELECTED;
    match b.button_type {
        2 => state.states[id] ^= SELECTED,
        3 => {
            if b.group_id != 0 {
                for other in 0..state.layout.windows.len() {
                    if other != id
                        && state.layout.windows[other]
                            .button()
                            .is_some_and(|o| o.group_id == b.group_id && o.button_type == 3)
                    {
                        state.states[other] &= !SELECTED;
                    }
                }
            }
            state.states[id] |= SELECTED;
        }
        _ => {}
    }
    state.clicks.push(id);
    state.activations.push(id);
}

pub struct SpuiPlugin;

impl Plugin for SpuiPlugin {
    fn build(&self, app: &mut App) {
        // `--spui-pointer x,y` / `--spui-press` pin the pointer (captures).
        let args: Vec<String> = std::env::args().collect();
        let pinned = args
            .iter()
            .position(|a| a == "--spui-pointer")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
            });
        app.init_resource::<SpuiAssets>()
            .init_resource::<super::tooltip::TooltipState>()
            .insert_resource(SpuiPointer {
                position_override: pinned,
                pressed_override: args.iter().any(|a| a == "--spui-press").then_some(true),
                ..default()
            })
            .add_systems(
                Update,
                (demo, input, super::tooltip::update, sync)
                    .chain()
                    .after(crate::native_ui::apply),
            )
            // After the layout and text systems of the frame: redraws the glyph masks of the
            // outlined captions that changed.
            .add_systems(PostUpdate, caption::update_masks.after(bevy::ui::UiSystems::PostLayout));
        if stats_enabled() {
            app.add_systems(PostUpdate, stats_glyphs.after(bevy::ui::UiSystems::PostLayout));
        }
    }
}

/// `--spui <LayoutName>` (quick capture / debugging): spawn any layout over
/// the current scene, fitted to the screen, for inspection.
pub fn demo_layout_name() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--spui")
        .and_then(|i| args.get(i + 1).cloned())
}

/// Output file name for `--quick-capture` when `--spui` is given.
pub fn capture_file_name(default: String) -> String {
    match demo_layout_name() {
        Some(name) => format!("spui-{name}.png"),
        None => default,
    }
}

#[derive(Component)]
pub struct SpuiDemo;

fn demo(
    mut commands: Commands,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    existing: Query<(), With<SpuiDemo>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    mut done: Local<bool>,
) {
    let Some(name) = demo_layout_name() else {
        return;
    };
    if *done || !existing.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let Some(layout) = assets.layout(&originals.resource_index, &name) else {
        warn!("--spui: layout {name} not found");
        *done = true;
        return;
    };
    let policy = if std::env::args().any(|a| a == "--spui-authored") {
        RootPolicy::Authored
    } else if std::env::args().any(|a| a == "--spui-centered") {
        RootPolicy::Centered
    } else {
        RootPolicy::Fit
    };
    let root = spawn(&mut commands, layout, policy, Some(camera), 400);
    commands.entity(root).insert(SpuiDemo);
    *done = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::model::{BiState, EffectKind, WinProc, Window, WindowKind};

    fn window(id: WinId, parent: Option<WinId>, children: Vec<WinId>, procs: Vec<WinProc>) -> Window {
        Window {
            id,
            element: id,
            class: 0,
            kind: WindowKind::Window,
            parent,
            children,
            flags: flag::VISIBLE | flag::ENABLED,
            control_id: id as u32 + 1,
            command_id: 0,
            area: [0.0, 0.0, 100.0, 100.0],
            caption: None,
            fill_color: 0,
            shade_color: 0xFFFF_FFFF,
            text_style: 0,
            drawable: None,
            procs,
            extra: Extra::None,
        }
    }

    fn effect(kind: EffectKind, time: f32) -> WinProc {
        WinProc::Effect {
            bi: BiState {
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

    /// Root window with a child that fades (0.2 s) and inflates (0.3 s).
    fn state() -> SpuiState {
        let layout = Layout {
            windows: vec![
                window(0, None, vec![1, 2], vec![]),
                window(
                    1,
                    Some(0),
                    vec![],
                    vec![effect(EffectKind::Fade, 0.2), effect(EffectKind::Inflate { scale: 0.1 }, 0.3)],
                ),
                window(2, Some(0), vec![], vec![]),
            ],
            roots: vec![0],
            ..Default::default()
        };
        SpuiState::new(Arc::new(layout), RootPolicy::Fit)
    }

    #[test]
    fn a_window_shown_later_fades_and_inflates_in() {
        let mut ui = state();
        ui.set_visible(1, false);
        ui.step_effects(0.016);
        assert!(!ui.effective().1[1], "hidden at the start");
        ui.set_visible(1, true);
        ui.step_effects(0.1);
        let (shade, visible) = ui.effective();
        assert!(visible[1]);
        // Half way through the 0.2 s fade: half transparent.
        assert_eq!((shade[1] >> 24) & 0xFF, 128);
        assert!(ui.poses[1].scale > 0.1 && ui.poses[1].scale < 1.0);
        ui.step_effects(0.3);
        assert_eq!(ui.poses[1], Pose::REST);
        assert_eq!(ui.effective().0[1], 0xFFFF_FFFF);
        // A sibling without effects is untouched.
        assert_eq!(ui.poses[2], Pose::REST);
    }

    #[test]
    fn a_window_hidden_keeps_drawing_while_it_fades_out_but_stops_being_interactive() {
        let mut ui = state();
        ui.step_effects(0.016);
        // Shown at the start: no entrance animation.
        assert_eq!(ui.poses[1], Pose::REST);
        ui.set_visible(1, false);
        ui.step_effects(0.1);
        assert!(ui.effective().1[1], "still fading out");
        assert!(!ui.interactive()[1], "no clicks while fading out");
        ui.step_effects(0.5);
        assert!(!ui.effective().1[1]);
    }

    /// A fading root (alpha 128) over a `SporeStd` button and a plain image window, both authored with
    /// the shade `00FFFFFF` (the step dots of the new-game flow are such buttons).
    fn shaded_children() -> SpuiState {
        use crate::spui::model::{Drawable, StdDrawable};
        let mut root = window(0, None, vec![1, 2], vec![]);
        root.shade_color = 0x80FF_FFFF;
        let mut button = window(1, Some(0), vec![], vec![]);
        button.shade_color = 0x00FF_FFFF;
        button.drawable = Some(Drawable::SporeStd { base: StdDrawable::default(), infos: Box::new(Default::default()) });
        let mut image = window(2, Some(0), vec![], vec![]);
        image.shade_color = 0x00FF_FFFF;
        image.drawable = Some(Drawable::Std(StdDrawable::default()));
        let layout = Layout { windows: vec![root, button, image], roots: vec![0], ..Default::default() };
        SpuiState::new(Arc::new(layout), RootPolicy::Fit)
    }

    #[test]
    fn a_sporestd_drawable_ignores_the_authored_shade_of_its_window() {
        // VERIFIED (0x960270 / 0x8318F0, see `sporestd_shade`): the dots are drawn although authored
        // with alpha 0; what the parents hand down still applies, and a plain `Std` drawable is
        // modulated by the window's own shade as before.
        let ui = shaded_children();
        let (shade, _) = ui.effective();
        assert_eq!(shade[1] >> 24, 0, "the combined shade of the button is transparent");
        assert_eq!(ui.sporestd_shade(1, &shade), 0x80FF_FFFF);
        assert_eq!(shade[2] >> 24, 0, "a plain image window keeps its authored shade");
    }

    #[test]
    fn a_sporestd_drawable_follows_its_own_fade() {
        use crate::spui::model::{Drawable, StdDrawable};
        let mut root = window(0, None, vec![1], vec![]);
        root.shade_color = 0xFFFF_FFFF;
        let mut button = window(1, Some(0), vec![], vec![effect(EffectKind::Fade, 0.2)]);
        button.shade_color = 0x00FF_FFFF;
        button.drawable = Some(Drawable::SporeStd { base: StdDrawable::default(), infos: Box::new(Default::default()) });
        let layout = Layout { windows: vec![root, button], roots: vec![0], ..Default::default() };
        let mut ui = SpuiState::new(Arc::new(layout), RootPolicy::Fit);
        ui.set_visible(1, false);
        ui.step_effects(0.016);
        ui.set_visible(1, true);
        ui.step_effects(0.1);
        let (shade, _) = ui.effective();
        assert_eq!(ui.sporestd_shade(1, &shade) >> 24, 128, "half way through the fade");
        ui.step_effects(0.3);
        let (shade, _) = ui.effective();
        assert_eq!(ui.sporestd_shade(1, &shade), 0xFFFF_FFFF);
    }

    /// Census of the installed layouts: the `SporeStd` windows whose authored shade is not white (the
    /// ones `sporestd_shade` changes), and the step dots among them. Developer aid (prints the census).
    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn installed_sporestd_windows_with_authored_shades() {
        use crate::spui::model::Drawable;
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == crate::spui::format::SPUI_TYPE)
            .collect();
        keys.sort_by_key(|k| (k.group_id, k.instance_id));
        let (mut total, mut shaded, mut dots) = (0, 0, 0);
        for key in keys {
            let Ok(bytes) = index.read(key) else { continue };
            let Ok(layout) = Layout::parse(&bytes) else { continue };
            for w in &layout.windows {
                if !matches!(w.drawable, Some(Drawable::SporeStd { .. })) {
                    continue;
                }
                total += 1;
                if w.shade_color != 0xFFFF_FFFF {
                    shaded += 1;
                    let dot = [0x0538_5002, 0x0538_4EEC, 0x0538_4FBB, 0x053F_00C7, 0x0614_92B9].contains(&w.control_id);
                    dots += usize::from(dot);
                    eprintln!(
                        "{:08X}!{:08X} window {} control {:08X} shade {:08X} flags {:X}{}",
                        key.group_id,
                        key.instance_id,
                        w.id,
                        w.control_id,
                        w.shade_color,
                        w.flags,
                        if dot { " (step dot)" } else { "" }
                    );
                }
            }
        }
        eprintln!("SporeStd windows: {total}, with an authored shade other than white: {shaded}, step dots among them: {dots}");
        assert!(dots >= 5, "the five step dots of the new-game layout are authored with a transparent shade");
    }

    #[test]
    fn rotation_is_kept_per_window() {
        let mut ui = state();
        ui.set_rotation(2, std::f32::consts::FRAC_PI_2);
        assert_eq!(ui.rotations[2], std::f32::consts::FRAC_PI_2);
        assert_eq!(ui.rotations[1], 0.0);
    }

    #[test]
    fn caption_copies_fold_the_shade_into_the_taps_and_the_tint() {
        let mut o = crate::spui::outline::Outline { size: 1, strength: 2, color: 0x0011_2233, ..Default::default() };
        o.apply_presets();
        let taps = o.taps();
        let opaque = caption_params(&o, &taps, 0xFFFF_FFFF, 1);
        assert_eq!((opaque.info.x, opaque.info.y), (1.0, 8.0));
        assert_eq!(opaque.color, Vec4::new(0x11 as f32 / 255.0, 0x22 as f32 / 255.0, 0x33 as f32 / 255.0, 0.0));
        // The first copy is at (-1, -1) with alpha 18, the second at (-1, 0) with alpha 35.
        assert_eq!(opaque.taps[0], Vec4::new(-1.0, -1.0, 18.0 / 255.0, 0.0));
        assert_eq!(opaque.taps[1], Vec4::new(-1.0, 0.0, 35.0 / 255.0, 0.0));
        // A half transparent window (alpha 128) halves the alphas (quantised like `mul_argb`) and
        // leaves the colour alone.
        let faded = caption_params(&o, &taps, 0x80FF_FFFF, 1);
        assert_eq!(faded.color, opaque.color);
        let expected = (mul_argb(o.tap_color(&taps[1]), 0x80FF_FFFF) >> 24) as f32 / 255.0;
        assert_eq!(faded.taps[1].z, expected);
        assert!(faded.taps[1].z < opaque.taps[1].z);
    }

    #[test]
    fn the_copies_node_is_the_mask_grown_by_the_largest_shift() {
        let key = CopiesKey { box_size: [100, 20], pad: 0, shade: 0xFFFF_FFFF };
        assert_eq!(copies_rect(&key, 3), (-3.0, -3.0, 106.0, 26.0));
        let padded = CopiesKey { pad: 5, ..key };
        assert_eq!(copies_rect(&padded, 3), (-8.0, -8.0, 116.0, 36.0));
    }
}
