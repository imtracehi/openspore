//! Window painting as plain draw commands.
//!
//! [`window_commands`] decides what to draw for one window in its current
//! state: fills, images (with their source region, destination, slicing or
//! tiling and tint). The Bevy view turns the commands into UI nodes; keeping
//! this step free of Bevy lets the geometry be unit tested. Ported from the
//! SporeModder-FX drawables (`StdDrawable`, `cSPUIStdDrawable`,
//! `ImageDrawable`, `ButtonDrawableStandard`, `ISporeImage`).
//!
//! Destination rectangles are relative to the top-left corner of the window.

use super::ids::state;
use super::model::*;
use super::outline::{self, Outline};
use super::solve::Rect;
use crate::dbpf::ResourceKey;

/// How an image fills its destination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Stretch,
    /// Nine-slice with the border widths `[left, top, right, bottom]` in
    /// source pixels. `tile` repeats edges and centre instead of stretching.
    Sliced { border: [f32; 4], tile: bool },
    /// Repeat the source at its natural size.
    Tiled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Fill {
        rect: Rect,
        color: u32,
    },
    Image {
        file: ResourceKey,
        /// Source region in texture pixels.
        src: Rect,
        dest: Rect,
        /// Tint, `0xAARRGGBB`.
        color: u32,
        mode: Mode,
    },
    /// The copies of one image in a stroke / halo / outline kernel (see [`outline`]), drawn one
    /// over the other in order, each shifted by whole pixels and tinted with `rgb` (`0x00RRGGBB`)
    /// and its own alpha: what the retail renderer does with one draw per copy. The image is
    /// stretched into `dest` like a [`Cmd::Image`] of [`Mode::Stretch`]. The view draws all copies
    /// with one UI material; [`expand_copies`] turns them back into one [`Cmd::Image`] each.
    Copies {
        file: ResourceKey,
        /// Source region in texture pixels.
        src: Rect,
        dest: Rect,
        rgb: u32,
        /// `(dx, dy, alpha)` of the copies.
        taps: Vec<(i32, i32, u8)>,
    },
}

/// Most copies one [`Cmd::Copies`] carries (the uniform array of the outline material); a
/// larger kernel (none in the installed data: the biggest has 120) is drawn copy by copy.
pub const MAX_COPIES: usize = 120;

/// The commands with every [`Cmd::Copies`] replaced by the [`Cmd::Image`] of each copy.
pub fn expand_copies(cmds: Vec<Cmd>) -> Vec<Cmd> {
    if !cmds.iter().any(|c| matches!(c, Cmd::Copies { .. })) {
        return cmds;
    }
    let mut out = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        match cmd {
            Cmd::Copies { file, src, dest, rgb, taps } => {
                for (dx, dy, alpha) in taps {
                    out.push(Cmd::Image {
                        file,
                        src,
                        dest: Rect::new(dest.x1 + dx as f32, dest.y1 + dy as f32, dest.x2 + dx as f32, dest.y2 + dy as f32),
                        color: (alpha as u32) << 24 | rgb & 0x00FF_FFFF,
                        mode: Mode::Stretch,
                    });
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Texture sizes by file, so normalised atlas rectangles become pixels.
pub trait TextureSizes {
    fn size(&self, key: &ResourceKey) -> Option<[u32; 2]>;
}

impl<F: Fn(&ResourceKey) -> Option<[u32; 2]>> TextureSizes for F {
    fn size(&self, key: &ResourceKey) -> Option<[u32; 2]> {
        self(key)
    }
}

/// Runtime state of a window that selects its images.
#[derive(Debug, Clone, Copy, Default)]
pub struct WinState {
    pub flags: u32,
    pub state: u32,
}

impl WinState {
    pub fn enabled(&self) -> bool {
        self.flags & super::ids::flag::ENABLED != 0
    }
    pub fn hover(&self) -> bool {
        self.state & state::HOVER != 0
    }
    pub fn clicked(&self) -> bool {
        self.state & state::CLICKED != 0
    }
    pub fn selected(&self) -> bool {
        self.state & state::SELECTED != 0
    }
}

/// Image slot of the eight drawable states: disabled/highlighted/active and
/// their checked variants (`getImageIndex`). A pressed button shows the
/// active image even while hovered.
pub fn image_index(s: WinState) -> usize {
    let base = if !s.enabled() {
        1
    } else if s.clicked() {
        3
    } else if s.hover() {
        2
    } else {
        0
    };
    if s.selected() {
        4 + base
    } else {
        base
    }
}

/// Tile of a four-state button strip (`getTileIndex`): disabled, idle,
/// hover, pressed/selected.
pub fn tile_index(s: WinState) -> usize {
    if !s.enabled() {
        0
    } else if s.clicked() || s.selected() {
        3
    } else if s.hover() {
        2
    } else {
        1
    }
}

/// Pixel source rectangle of an image reference.
pub fn source_rect(image: &ImageRef, sizes: &dyn TextureSizes) -> Option<Rect> {
    let [tw, th] = sizes.size(&image.file)?;
    let (tw, th) = (tw as f32, th as f32);
    let r = Rect::new(
        image.uv[0] * tw,
        image.uv[1] * th,
        image.uv[2] * tw,
        image.uv[3] * th,
    );
    // Atlas regions are whole pixels; trim float noise from the stored uvs.
    let snap = |v: f32| {
        let n = v.round();
        if (v - n).abs() < 0.02 {
            n
        } else {
            v
        }
    };
    Some(Rect::new(snap(r.x1), snap(r.y1), snap(r.x2), snap(r.y2)))
}

/// Authored size of an image in pixels (`Dimensions`, else the source size).
pub fn natural_size(image: &ImageRef, sizes: &dyn TextureSizes) -> Option<[f32; 2]> {
    if let Some(d) = image.dims {
        return Some([d[0] as f32, d[1] as f32]);
    }
    let r = source_rect(image, sizes)?;
    Some([r.width(), r.height()])
}

fn slice_border(natural: [f32; 2], area: [f32; 4]) -> [f32; 4] {
    [
        area[0] * natural[0],
        area[1] * natural[1],
        area[2] * natural[0],
        area[3] * natural[1],
    ]
}

/// Draw an image into `dest` using a drawable scale type.
fn scaled(
    image: &ImageRef,
    dest: Rect,
    scale_type: u32,
    scale_area: [f32; 4],
    color: u32,
    sizes: &dyn TextureSizes,
    out: &mut Vec<Cmd>,
) {
    let Some(src) = source_rect(image, sizes) else {
        return;
    };
    let natural = natural_size(image, sizes).unwrap_or([src.width(), src.height()]);
    let mode = match scale_type {
        SCALE_STRETCH_CENTER => Mode::Sliced {
            border: slice_border(natural, scale_area),
            tile: false,
        },
        SCALE_TILE_IMAGE => Mode::Tiled,
        SCALE_TILE_CENTER => Mode::Sliced {
            border: slice_border(natural, scale_area),
            tile: true,
        },
        _ => Mode::Stretch,
    };
    out.push(Cmd::Image {
        file: image.file,
        src,
        dest,
        color,
        mode,
    });
}

/// `ImageDrawable`: alignment, fixed size flags, tiling.
fn image_drawable(
    d: &ImageDrawable,
    win: Rect,
    color: u32,
    sizes: &dyn TextureSizes,
    out: &mut Vec<Cmd>,
) {
    let Some(image) = &d.image else {
        return;
    };
    let Some(src) = source_rect(image, sizes) else {
        return;
    };
    let natural = natural_size(image, sizes).unwrap_or([src.width(), src.height()]);
    let (ww, wh) = (win.width(), win.height());
    let (nw, nh) = (natural[0] * d.scale, natural[1] * d.scale);
    let w = if d.flags & IMAGE_FIXED_WIDTH != 0 { nw } else { ww };
    let h = if d.flags & IMAGE_FIXED_HEIGHT != 0 { nh } else { wh };
    let x = match d.align_h {
        1 => 0.0,
        2 => ww - w,
        _ => (ww - w) * 0.5,
    };
    let y = match d.align_v {
        1 => 0.0,
        2 => wh - h,
        _ => (wh - h) * 0.5,
    };
    let mut dest = Rect::new(x, y, x + w, y + h);
    let mut src = src;
    let mode = match d.tiling {
        TILING_STANDARD => Mode::Tiled,
        TILING_EDGE => Mode::Sliced {
            border: slice_border(natural, [1.0 / 3.0; 4]),
            tile: true,
        },
        _ => Mode::Stretch,
    };
    // A fixed-size image that overhangs its window is cut off at the window
    // edge (ADAPTED: this is how progress style fills that shrink the window
    // of a fixed-width bar image must behave; the retail renderer is assumed
    // to scissor a window's own drawing).
    if mode == Mode::Stretch
        && d.flags & (IMAGE_FIXED_WIDTH | IMAGE_FIXED_HEIGHT) != 0
        && dest.width() > 0.0
        && dest.height() > 0.0
    {
        let cut = Rect::new(dest.x1.max(0.0), dest.y1.max(0.0), dest.x2.min(ww), dest.y2.min(wh));
        if cut.width() <= 0.0 || cut.height() <= 0.0 {
            return;
        }
        let (fx0, fx1) = ((cut.x1 - dest.x1) / dest.width(), (cut.x2 - dest.x1) / dest.width());
        let (fy0, fy1) = ((cut.y1 - dest.y1) / dest.height(), (cut.y2 - dest.y1) / dest.height());
        src = Rect::new(
            src.x1 + fx0 * src.width(),
            src.y1 + fy0 * src.height(),
            src.x1 + fx1 * src.width(),
            src.y1 + fy1 * src.height(),
        );
        dest = cut;
    }
    let at = out.len();
    out.push(Cmd::Image {
        file: image.file,
        src,
        dest,
        color,
        mode,
    });
    if let Some(o) = &d.outline {
        insert_outlines(out, at, &[o], color);
    }
}

/// Image copies made by [`insert_outlines`] since the view last reset it (diagnostics,
/// `OS_SPUI_STATS`).
pub static OUTLINE_IMAGE_COPIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Puts the copies of the `outlines` (drawn in the given order, each one over the previous) in
/// front of the image command at `out[at]`: the retail kernel re-draws the layer's image once per
/// tap, shifted by whole pixels and tinted with the outline colour and the tap's alpha (see
/// [`outline`]). ADAPTED: the window's `shade` modulates the copies like the layer itself (the
/// retail renderer's shade / alpha state is not decoded; every other draw of the window is
/// modulated by it, so the copies are too).
fn insert_outlines(out: &mut Vec<Cmd>, at: usize, outlines: &[&Outline], shade: u32) {
    let Some(Cmd::Image { file, src, dest, mode, .. }) = out.get(at).cloned() else {
        return;
    };
    let mut pre = Vec::new();
    for o in outlines {
        let taps = o.taps();
        if taps.is_empty() {
            continue;
        }
        OUTLINE_IMAGE_COPIES.fetch_add(taps.len(), std::sync::atomic::Ordering::Relaxed);
        if mode == Mode::Stretch && taps.len() <= MAX_COPIES {
            // The tint of every copy is the outline colour times the shade, quantised like a
            // single draw would (the rgb part is the same for all copies).
            let rgb = mul_color(o.tap_color(&taps[0]), shade) & 0x00FF_FFFF;
            pre.push(Cmd::Copies {
                file,
                src,
                dest,
                rgb,
                taps: taps
                    .iter()
                    .map(|t| (t.dx, t.dy, (mul_color(o.tap_color(t), shade) >> 24) as u8))
                    .collect(),
            });
            continue;
        }
        for tap in taps {
            pre.push(Cmd::Image {
                file,
                src,
                dest: Rect::new(
                    dest.x1 + tap.dx as f32,
                    dest.y1 + tap.dy as f32,
                    dest.x2 + tap.dx as f32,
                    dest.y2 + tap.dy as f32,
                ),
                color: mul_color(o.tap_color(&tap), shade),
                mode,
            });
        }
    }
    if !pre.is_empty() {
        out.splice(at..at, pre);
    }
}

fn mul_color(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s) & 0xFF) as f32 / 255.0 * ((b >> s) & 0xFF) as f32 / 255.0;
    let q = |v: f32| (v * 255.0 + 0.5) as u32;
    q(ch(24)) << 24 | q(ch(16)) << 16 | q(ch(8)) << 8 | q(ch(0))
}

/// What to draw for a window. `rect` is the window's real area; `shade` is the
/// effective colour modulation (own shade times ancestors').
pub fn window_commands(
    w: &Window,
    rect: Rect,
    st: WinState,
    shade: u32,
    sizes: &dyn TextureSizes,
) -> Vec<Cmd> {
    let mut out = Vec::new();
    let local = Rect::new(0.0, 0.0, rect.width(), rect.height());
    if w.paints_fill_color() && (w.fill_color >> 24) != 0 {
        out.push(Cmd::Fill {
            rect: local,
            color: mul_color(w.fill_color, shade),
        });
    }
    let Some(drawable) = &w.drawable else {
        return out;
    };
    match drawable {
        Drawable::Image(d) => image_drawable(d, local, shade, sizes, &mut out),
        Drawable::Std(d) => {
            let index = image_index(st);
            if let Some(image) = d.images[index].as_ref().or(d.images[0].as_ref()) {
                let at = out.len();
                scaled(image, local, d.scale_type, d.scale_area, shade, sizes, &mut out);
                if let Some(o) = &d.outline {
                    insert_outlines(&mut out, at, &[o], shade);
                }
            }
        }
        Drawable::SporeStd { base, infos } => {
            let index = image_index(st);
            let Some(info) = infos[index].as_ref().or(infos[0].as_ref()) else {
                return out;
            };
            let bg = Rect::new(
                info.background_offset[0],
                info.background_offset[1],
                info.background_offset[0] + info.background_scale[0] * local.width(),
                info.background_offset[1] + info.background_scale[1] * local.height(),
            );
            let tint = mul_color(info.background_color, shade);
            if let Some(image) = &info.background {
                // `SporeStdDrawable::Paint` 0x8318F0: background halo, background stroke,
                // background (modes 1 = background and icon, 2 = background only).
                let at = out.len();
                scaled(image, bg, base.scale_type, base.scale_area, tint, sizes, &mut out);
                let mut around: Vec<&Outline> = Vec::new();
                if matches!(info.halo_mode, outline::SHADOW_FULL | outline::SHADOW_BACKGROUND) {
                    around.push(&info.halo);
                }
                if matches!(info.stroke_mode, outline::SHADOW_FULL | outline::SHADOW_BACKGROUND) {
                    around.push(&info.stroke);
                }
                insert_outlines(&mut out, at, &around, shade);
            }
            // "Sometimes only the first state specifies the icon."
            let icon = info
                .icon
                .as_ref()
                .or_else(|| infos[0].as_ref().and_then(|i| i.icon.as_ref()));
            if let Some(icon) = icon {
                if let Some(src) = source_rect(icon, sizes) {
                    let natural = natural_size(icon, sizes).unwrap_or([src.width(), src.height()]);
                    let (iw, ih) = if info.icon_draw_mode == 0 {
                        (natural[0] * info.icon_scale[0], natural[1] * info.icon_scale[1])
                    } else {
                        (bg.width() * info.icon_scale[0], bg.height() * info.icon_scale[1])
                    };
                    let x = bg.x1 + (bg.width() - iw) * 0.5 + info.icon_offset[0];
                    let y = bg.y1 + (bg.height() - ih) * 0.5 + info.icon_offset[1];
                    // Icon halo, icon stroke, icon (modes 1 = background and icon, 3 = icon only).
                    let at = out.len();
                    out.push(Cmd::Image {
                        file: icon.file,
                        src,
                        dest: Rect::new(x, y, x + iw, y + ih),
                        color: mul_color(info.icon_color, shade),
                        mode: Mode::Stretch,
                    });
                    let mut around: Vec<&Outline> = Vec::new();
                    if matches!(info.halo_mode, outline::SHADOW_FULL | outline::SHADOW_ICON) {
                        around.push(&info.halo);
                    }
                    if matches!(info.stroke_mode, outline::SHADOW_FULL | outline::SHADOW_ICON) {
                        around.push(&info.stroke);
                    }
                    insert_outlines(&mut out, at, &around, shade);
                }
            }
        }
        Drawable::ButtonStandard { image, tileable } | Drawable::ButtonRadio { image, tileable } => {
            if let Some(image) = image {
                if let Some(src) = source_rect(image, sizes) {
                    let tiles = if matches!(drawable, Drawable::ButtonRadio { .. }) { 6.0 } else { 4.0 };
                    let tw = src.width() / tiles;
                    let t = tile_index(st).min(tiles as usize - 1) as f32;
                    let tile = Rect::new(src.x1 + t * tw, src.y1, src.x1 + (t + 1.0) * tw, src.y2);
                    let mode = if *tileable {
                        Mode::Sliced {
                            border: slice_border([tw, src.height()], [1.0 / 3.0; 4]),
                            tile: false,
                        }
                    } else {
                        Mode::Stretch
                    };
                    out.push(Cmd::Image {
                        file: image.file,
                        src: tile,
                        dest: local,
                        color: shade,
                        mode,
                    });
                }
            }
        }
        Drawable::VariableWidth { image, fill_color } => match image {
            Some(image) => {
                if let Some(src) = source_rect(image, sizes) {
                    out.push(Cmd::Image {
                        file: image.file,
                        src,
                        dest: local,
                        color: shade,
                        mode: Mode::Stretch,
                    });
                }
            }
            None => out.push(Cmd::Fill {
                rect: local,
                color: mul_color(*fill_color, shade),
            }),
        },
        Drawable::ImageSet { class, images } if *class == super::ids::class::SLIDER_DRAWABLE => {
            if let super::model::Extra::Range(r) = &w.extra {
                slider_commands(images, r, local, st, shade, sizes, &mut out);
            }
        }
        Drawable::ImageSet { .. } | Drawable::Frame | Drawable::Other(_) => {}
    }
    out
}

/// Border of the slider groove's slicing: `SliderDrawable::Paint`
/// (march2017 0x9849A0, called once per image index) draws image 2, the thumb
/// container ("groove"), with `Graphics2D::DrawSlicedStretchedImage` and the
/// slice proportions `0.5 -/+ 0.5 / size` (0x984A04 .. 0x984A5D): a nine-slice
/// whose centre is a single pixel, so the groove keeps its end caps and tick
/// marks at their size and only the middle pixel row / column is stretched over
/// the window (VERIFIED, code).
fn groove_border(src: Rect) -> [f32; 4] {
    let (hx, hy) = (((src.width() - 1.0) / 2.0).max(0.0), ((src.height() - 1.0) / 2.0).max(0.0));
    [hx, hy, hx, hy]
}

/// A slider (`SliderDrawable`, three images): image 0 (background) tiled over
/// the window, image 2 (the groove, [`groove_border`]) over the window, image 1
/// a four-tile thumb strip (state tiles as [`tile_index`]) placed at the value.
/// Tall windows are vertical sliders with the maximum at the top.
fn slider_commands(
    images: &[Option<ImageRef>],
    r: &super::model::RangeData,
    local: Rect,
    st: WinState,
    shade: u32,
    sizes: &dyn TextureSizes,
    out: &mut Vec<Cmd>,
) {
    if let Some(bg) = images.first().and_then(|i| i.as_ref()) {
        if let Some(src) = source_rect(bg, sizes) {
            out.push(Cmd::Image { file: bg.file, src, dest: local, color: shade, mode: Mode::Tiled });
        }
    }
    if let Some(groove) = images.get(2).and_then(|i| i.as_ref()) {
        if let Some(src) = source_rect(groove, sizes) {
            out.push(Cmd::Image {
                file: groove.file,
                src,
                dest: local,
                color: shade,
                mode: Mode::Sliced { border: groove_border(src), tile: false },
            });
        }
    }
    let Some(thumb) = images.get(1).and_then(|i| i.as_ref()) else { return };
    let Some(src) = source_rect(thumb, sizes) else { return };
    let natural = natural_size(thumb, sizes).unwrap_or([src.width(), src.height()]);
    let (tw, th) = (natural[0] / 4.0, natural[1]);
    let span = (r.max - r.min) as f32;
    let t = if span.abs() > 0.0 { ((r.value - r.min) as f32 / span).clamp(0.0, 1.0) } else { 0.0 };
    let dest = if r.orientation == 2 || local.height() > local.width() {
        let y = local.y2 - th - t * (local.height() - th).max(0.0);
        let x = local.x1 + (local.width() - tw) * 0.5;
        Rect::new(x, y, x + tw, y + th)
    } else {
        let x = local.x1 + t * (local.width() - tw).max(0.0);
        let y = local.y1 + (local.height() - th) * 0.5;
        Rect::new(x, y, x + tw, y + th)
    };
    let sw = src.width() / 4.0;
    let i = tile_index(st) as f32;
    out.push(Cmd::Image {
        file: thumb.file,
        src: Rect::new(src.x1 + i * sw, src.y1, src.x1 + (i + 1.0) * sw, src.y2),
        dest,
        color: shade,
        mode: Mode::Stretch,
    });
}

/// Source region of flipbook frame `frame` (clamped to the frame count), for
/// icons whose frame the game code chooses (meters).
pub fn flipbook_frame_index(a: &AnimData, image: &ImageRef, frame: u32, sizes: &dyn TextureSizes) -> Option<Rect> {
    let src = source_rect(image, sizes)?;
    let (rows, cols) = (a.rows.max(1), a.columns.max(1));
    let count = if a.frame_count == 0 { rows * cols } else { a.frame_count.min(rows * cols) };
    let frame = frame.min(count.max(1) - 1);
    let (fw, fh) = (src.width() / cols as f32, src.height() / rows as f32);
    let (col, row) = (frame % cols, frame / cols);
    Some(Rect::new(
        src.x1 + col as f32 * fw,
        src.y1 + row as f32 * fh,
        src.x1 + (col + 1) as f32 * fw,
        src.y1 + (row + 1) as f32 * fh,
    ))
}

/// Frame of a flipbook animated icon: source region and frame index.
pub fn flipbook_frame(a: &AnimData, image: &ImageRef, time: f32, sizes: &dyn TextureSizes) -> Option<Rect> {
    let src = source_rect(image, sizes)?;
    let (rows, cols) = (a.rows.max(1), a.columns.max(1));
    let count = if a.frame_count == 0 { rows * cols } else { a.frame_count.min(rows * cols) };
    let duration = a.duration.max(1e-3);
    let phase = (time / duration).max(0.0);
    let mut frame = match a.end_mode {
        // Ping-pong runs forward then backward.
        1 => {
            let cycle = phase % 2.0;
            let p = if cycle <= 1.0 { cycle } else { 2.0 - cycle };
            ((p * count as f32) as u32).min(count - 1)
        }
        // Stop at the last frame, loop count behaves like loop for the first
        // pass only.
        2 => ((phase.min(0.999_9) * count as f32) as u32).min(count - 1),
        3 => {
            let loops = a.loop_count.max(1) as f32;
            ((phase.min(loops - 0.000_1).fract() * count as f32) as u32).min(count - 1)
        }
        _ => ((phase.fract() * count as f32) as u32).min(count - 1),
    };
    if a.direction == 1 {
        frame = count - 1 - frame;
    }
    let (fw, fh) = (src.width() / cols as f32, src.height() / rows as f32);
    let (col, row) = (frame % cols, frame / cols);
    Some(Rect::new(
        src.x1 + col as f32 * fw,
        src.y1 + row as f32 * fh,
        src.x1 + (col + 1) as f32 * fw,
        src.y1 + (row + 1) as f32 * fh,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ResourceKey {
        ResourceKey {
            type_id: 0x2F7D_0004,
            group_id: 1,
            instance_id: 2,
        }
    }
    fn image(uv: [f32; 4], dims: Option<[u32; 2]>) -> ImageRef {
        ImageRef {
            file: key(),
            uv,
            dims,
        }
    }
    fn sizes() -> impl TextureSizes {
        |_: &ResourceKey| Some([512u32, 512])
    }
    fn window(drawable: Option<Drawable>) -> Window {
        Window {
            id: 0,
            element: 0,
            class: super::super::ids::class::WINDOW,
            kind: WindowKind::Window,
            parent: None,
            children: vec![],
            flags: 3,
            control_id: 0,
            command_id: 0,
            area: [0.0; 4],
            caption: None,
            fill_color: 0x007F_7F7F,
            shade_color: WHITE,
            text_style: DEFAULT_STYLE,
            drawable,
            procs: vec![],
            extra: Extra::None,
        }
    }

    #[test]
    fn state_indices() {
        let s = |flags, state| WinState { flags, state };
        assert_eq!(image_index(s(3, 0)), 0);
        assert_eq!(image_index(s(1, 0)), 1, "disabled");
        assert_eq!(image_index(s(3, state::HOVER)), 2);
        assert_eq!(image_index(s(3, state::CLICKED | state::HOVER)), 3, "pressed wins over hover");
        assert_eq!(image_index(s(3, state::SELECTED)), 4);
        assert_eq!(image_index(s(3, state::SELECTED | state::HOVER)), 6);
        assert_eq!(image_index(s(1, state::SELECTED)), 5);
        assert_eq!(tile_index(s(3, 0)), 1);
        assert_eq!(tile_index(s(3, state::HOVER)), 2);
        assert_eq!(tile_index(s(3, state::SELECTED)), 3);
        assert_eq!(tile_index(s(1, 0)), 0);
    }

    #[test]
    fn atlas_regions_become_whole_pixels() {
        let img = image([0.115234375, 0.904296875, 0.28125, 0.9453125], Some([85, 21]));
        let r = source_rect(&img, &sizes()).unwrap();
        assert_eq!(r, Rect::new(59.0, 463.0, 144.0, 484.0));
        assert_eq!(natural_size(&img, &sizes()), Some([85.0, 21.0]));
        let direct = ImageRef {
            file: key(),
            uv: [0.0, 0.0, 1.0, 1.0],
            dims: None,
        };
        assert_eq!(natural_size(&direct, &sizes()), Some([512.0, 512.0]));
    }

    #[test]
    fn stretch_center_borders_are_fractions_of_the_image() {
        let img = image([0.0, 0.0, 0.25, 0.25], Some([128, 128]));
        let d = Drawable::Std(StdDrawable {
            images: [Some(img), None, None, None, None, None, None, None],
            scale_type: SCALE_STRETCH_CENTER,
            scale_area: [0.25, 0.5, 0.25, 0.5],
            ..Default::default()
        });
        let w = window(Some(d));
        let cmds = window_commands(&w, Rect::new(10.0, 20.0, 110.0, 70.0), WinState { flags: 3, state: 0 }, WHITE, &sizes());
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Cmd::Image { dest, mode, src, .. } => {
                assert_eq!(*dest, Rect::new(0.0, 0.0, 100.0, 50.0));
                assert_eq!(*src, Rect::new(0.0, 0.0, 128.0, 128.0));
                assert_eq!(*mode, Mode::Sliced { border: [32.0, 64.0, 32.0, 64.0], tile: false });
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn image_drawable_alignment_and_fixed_size() {
        let img = image([0.0, 0.0, 0.1, 0.1], Some([55, 33]));
        let d = Drawable::Image(ImageDrawable {
            image: Some(img),
            flags: IMAGE_FIXED_WIDTH | IMAGE_FIXED_HEIGHT,
            tiling: TILING_STANDARD,
            ..Default::default()
        });
        let w = window(Some(d));
        let cmds = window_commands(&w, Rect::new(0.0, 0.0, 55.0, 25.0), WinState { flags: 3, state: 0 }, WHITE, &sizes());
        match &cmds[0] {
            Cmd::Image { dest, mode, .. } => {
                // Natural 55x33 centred vertically in a 25 high window.
                assert_eq!(*dest, Rect::new(0.0, -4.0, 55.0, 29.0));
                assert_eq!(*mode, Mode::Tiled);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fixed_width_images_are_cut_off_by_a_narrow_window() {
        // An 83x5 bar image left aligned in a window narrowed to 40 px.
        let img = image([0.0, 0.0, 83.0 / 512.0, 5.0 / 512.0], Some([83, 5]));
        let d = Drawable::Image(ImageDrawable {
            image: Some(img),
            align_h: 1,
            flags: IMAGE_FIXED_WIDTH | IMAGE_FIXED_HEIGHT,
            ..Default::default()
        });
        let w = window(Some(d));
        let cmds = window_commands(&w, Rect::new(0.0, 0.0, 40.0, 14.0), WinState { flags: 3, state: 0 }, WHITE, &sizes());
        match &cmds[0] {
            Cmd::Image { dest, src, .. } => {
                assert_eq!(*dest, Rect::new(0.0, 4.5, 40.0, 9.5));
                assert!((src.x2 - 40.0).abs() < 1e-3 && (src.y2 - 5.0).abs() < 1e-3, "{src:?}");
                assert_eq!((src.x1, src.y1), (0.0, 0.0));
            }
            other => panic!("{other:?}"),
        }
        // A window narrower than nothing draws nothing.
        assert!(window_commands(&w, Rect::new(0.0, 0.0, 0.0, 14.0), WinState { flags: 3, state: 0 }, WHITE, &sizes()).is_empty());
    }

    #[test]
    fn spore_std_draws_background_then_icon_by_state() {
        let bg = image([0.0, 0.0, 0.1, 0.1], Some([58, 58]));
        let icon = image([0.5, 0.5, 0.6, 0.6], Some([62, 62]));
        let normal = ImageInfo {
            background: Some(bg.clone()),
            icon: Some(icon.clone()),
            icon_draw_mode: 1,
            icon_scale: [0.9, 0.9],
            ..Default::default()
        };
        let hover = ImageInfo {
            background: Some(bg),
            icon_scale: [1.0, 1.0],
            icon_color: 0x80FF_FFFF,
            ..Default::default()
        };
        let mut infos: [Option<ImageInfo>; 8] = Default::default();
        infos[0] = Some(normal);
        infos[2] = Some(hover);
        let w = window(Some(Drawable::SporeStd {
            base: StdDrawable::default(),
            infos: Box::new(infos),
        }));
        let rect = Rect::new(0.0, 0.0, 55.0, 55.0);
        let idle = window_commands(&w, rect, WinState { flags: 3, state: 0 }, WHITE, &sizes());
        assert_eq!(idle.len(), 2);
        let Cmd::Image { dest, .. } = &idle[1] else { panic!() };
        // window-size icon at 0.9 centred in the window
        assert!((dest.width() - 49.5).abs() < 1e-4 && (dest.x1 - 2.75).abs() < 1e-4);
        // The hover state has no icon of its own: falls back to the idle icon,
        // drawn with the hover colour and scale.
        let hov = window_commands(&w, rect, WinState { flags: 3, state: state::HOVER }, WHITE, &sizes());
        let Cmd::Image { dest, color, .. } = &hov[1] else { panic!() };
        assert_eq!(dest.width(), 55.0);
        assert_eq!(*color, 0x80FF_FFFF);
    }

    fn outline_of(size: u32, strength: u32, color: u32) -> Outline {
        let mut o = Outline { size, strength, color, ..Default::default() };
        o.apply_presets();
        o
    }

    /// `SporeStdDrawable::Paint` 0x8318F0: background halo, background stroke, background, icon
    /// halo, icon stroke, icon; the copies are tinted `(alpha << 24) + colour` and shifted.
    #[test]
    fn spore_std_stroke_and_halo_are_drawn_around_the_layers() {
        let bg = image([0.0, 0.0, 0.1, 0.1], Some([58, 58]));
        let icon = image([0.5, 0.5, 0.6, 0.6], Some([62, 62]));
        let info = ImageInfo {
            background: Some(bg),
            icon: Some(icon),
            stroke_mode: outline::SHADOW_ICON,
            halo_mode: outline::SHADOW_BACKGROUND,
            stroke: outline_of(1, 2, 0xFF12_3456),
            halo: outline_of(2, 8, 0xFF00_0000),
            ..Default::default()
        };
        let mut infos: [Option<ImageInfo>; 8] = Default::default();
        infos[0] = Some(info);
        let w = window(Some(Drawable::SporeStd { base: StdDrawable::default(), infos: Box::new(infos) }));
        let cmds = expand_copies(window_commands(&w, Rect::new(0.0, 0.0, 40.0, 40.0), WinState { flags: 3, state: 0 }, WHITE, &sizes()));
        // Background: halo of size 2 (24 copies), no stroke (mode 3 = icon only), the background;
        // icon: no halo (mode 2 = background only), stroke of size 1 (8 copies), the icon.
        assert_eq!(cmds.len(), 24 + 1 + 8 + 1);
        let Cmd::Image { color, dest, .. } = &cmds[24] else { panic!() };
        assert_eq!((*color, *dest), (WHITE, Rect::new(0.0, 0.0, 40.0, 40.0)), "the background itself");
        // Halo copies carry the halo colour; strength 8 at the nearest taps is fully opaque.
        for c in &cmds[..24] {
            let Cmd::Image { color, .. } = c else { panic!() };
            assert_eq!(color & 0x00FF_FFFF, 0, "{color:08X}");
        }
        let Cmd::Image { color, dest, .. } = &cmds[0] else { panic!() };
        // x = -2, y = -2 is the first copy of the x-outer loop.
        assert_eq!(*dest, Rect::new(-2.0, -2.0, 38.0, 38.0));
        assert!(*color >> 24 > 0);
        let Cmd::Image { color, dest, .. } = &cmds[25] else { panic!() };
        assert_eq!(*dest, Rect::new(-1.0, -1.0, 39.0, 39.0));
        assert_eq!(color & 0x00FF_FFFF, 0x12_3456);
        assert_eq!(color >> 24, 18, "corner of a size 1, strength 2 stroke");
        let Cmd::Image { color, .. } = &cmds[33] else { panic!() };
        assert_eq!(*color, WHITE, "the icon itself is last");
    }

    #[test]
    fn stroke_copies_follow_the_window_shade_and_a_mode_of_zero_draws_none() {
        let icon = image([0.5, 0.5, 0.6, 0.6], Some([62, 62]));
        let mut info = ImageInfo {
            icon: Some(icon),
            stroke_mode: outline::SHADOW_ICON,
            stroke: outline_of(1, 9, 0xFFFF_FFFF),
            ..Default::default()
        };
        let mk = |info: ImageInfo| {
            let mut infos: [Option<ImageInfo>; 8] = Default::default();
            infos[0] = Some(info);
            window(Some(Drawable::SporeStd { base: StdDrawable::default(), infos: Box::new(infos) }))
        };
        let rect = Rect::new(0.0, 0.0, 30.0, 30.0);
        let cmds = expand_copies(window_commands(&mk(info.clone()), rect, WinState { flags: 3, state: 0 }, 0x8080_8080, &sizes()));
        assert_eq!(cmds.len(), 9);
        let Cmd::Image { color, .. } = &cmds[0] else { panic!() };
        // The first copy is the corner (-1, -1): strength 9 gives it alpha 111 (0x6F); the shade
        // multiplies the copy like the layer.
        assert_eq!(*color, mul_color(0x6FFF_FFFF, 0x8080_8080));
        info.stroke_mode = outline::SHADOW_NONE;
        assert_eq!(expand_copies(window_commands(&mk(info), rect, WinState { flags: 3, state: 0 }, WHITE, &sizes())).len(), 1);
    }

    #[test]
    fn image_and_std_drawables_draw_their_image_outline_first() {
        let img = image([0.0, 0.0, 0.1, 0.1], Some([55, 33]));
        let o = outline_of(1, 5, 0x0000_0000);
        let d = Drawable::Image(ImageDrawable { image: Some(img.clone()), outline: Some(o), ..Default::default() });
        let cmds = expand_copies(window_commands(&window(Some(d)), Rect::new(0.0, 0.0, 55.0, 33.0), WinState { flags: 3, state: 0 }, WHITE, &sizes()));
        assert_eq!(cmds.len(), 9);
        let Cmd::Image { color, .. } = &cmds[8] else { panic!() };
        assert_eq!(*color, WHITE);
        let d = Drawable::Std(StdDrawable { images: [Some(img), None, None, None, None, None, None, None], outline: Some(o), ..Default::default() });
        let cmds = window_commands(&window(Some(d)), Rect::new(0.0, 0.0, 55.0, 33.0), WinState { flags: 3, state: 0 }, WHITE, &sizes());
        // One `Copies` command with the eight copies, then the image.
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], Cmd::Copies { taps, .. } if taps.len() == 8));
        assert_eq!(expand_copies(cmds).len(), 9);
    }

    #[test]
    fn slider_draws_its_groove_sliced_around_one_centre_pixel() {
        // The audio page's slider: image 0 empty, image 1 the four-tile thumb,
        // image 2 the groove with its tick marks (40 x 120 px).
        let thumb = image([0.0, 0.5, 0.3125, 0.55], Some([160, 27]));
        let groove = image([0.0, 0.0, 0.078125, 0.234375], Some([40, 120]));
        let mut w = window(Some(Drawable::ImageSet {
            class: super::super::ids::class::SLIDER_DRAWABLE,
            images: vec![None, Some(thumb), Some(groove)],
        }));
        w.kind = WindowKind::Slider;
        w.extra = Extra::Range(RangeData { value: 500, min: 0, max: 1000, orientation: 2 });
        let rect = Rect::new(0.0, 0.0, 48.0, 114.0);
        let cmds = window_commands(&w, rect, WinState { flags: 3, state: 0 }, WHITE, &sizes());
        // The groove is drawn first (window-sized, one-pixel centre), then the thumb.
        let Cmd::Image { dest, mode, src, .. } = &cmds[0] else { panic!("{cmds:?}") };
        assert_eq!(*dest, rect);
        assert_eq!((src.width(), src.height()), (40.0, 120.0));
        assert_eq!(*mode, Mode::Sliced { border: [19.5, 59.5, 19.5, 59.5], tile: false });
        let Cmd::Image { dest, .. } = &cmds[1] else { panic!() };
        assert_eq!((dest.width(), dest.height()), (40.0, 27.0), "one tile of the thumb strip");
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn generic_windows_fill_with_their_colour_only() {
        let mut w = window(None);
        w.fill_color = 0xFF00_8000;
        let cmds = window_commands(&w, Rect::new(0.0, 0.0, 4.0, 4.0), WinState { flags: 3, state: 0 }, 0x80FF_FFFF, &sizes());
        assert_eq!(cmds, vec![Cmd::Fill { rect: Rect::new(0.0, 0.0, 4.0, 4.0), color: 0x8000_8000 }]);
        w.class = super::super::ids::class::TEXT;
        assert!(window_commands(&w, Rect::new(0.0, 0.0, 4.0, 4.0), WinState::default(), WHITE, &sizes()).is_empty());
    }

    #[test]
    fn flipbook_frames_walk_the_grid() {
        let a = AnimData {
            image: None,
            duration: 1.0,
            direction: 0,
            end_mode: 0,
            loop_count: 1,
            auto_play: true,
            frame_count: 20,
            rows: 5,
            columns: 4,
        };
        let img = image([0.0, 0.0, 0.46875, 0.224609375], Some([240, 115]));
        let at = |t| flipbook_frame(&a, &img, t, &sizes()).unwrap();
        // 240x115 atlas rect, 4 columns x 5 rows -> 60x23 frames.
        assert_eq!(at(0.0), Rect::new(0.0, 0.0, 60.0, 23.0));
        assert_eq!(at(0.26), Rect::new(60.0, 23.0, 120.0, 46.0));
        assert_eq!(at(1.0), Rect::new(0.0, 0.0, 60.0, 23.0), "loops");
        let mut back = a.clone();
        back.direction = 1;
        assert_eq!(flipbook_frame(&back, &img, 0.0, &sizes()).unwrap(), Rect::new(180.0, 92.0, 240.0, 115.0));
    }
}
