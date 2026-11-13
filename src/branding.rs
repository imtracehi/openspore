//! OpenSpore's own logo (`src/branding/openspore-logo.png`, also shown at the
//! top of the README). OpenSpore shows it wherever the retail game shows the
//! SPORE wordmark:
//!
//! * the galaxy screen's top-left panel (`GGEZoomPanel`): its atlas
//!   0x31A44893!0x9E6CB7F1 carries the wordmark (top-left cell) and a "TM"
//!   (its own cell, drawn by a second window at the wordmark's top right), so
//!   [`patch_texture`] rewrites both while the atlas is decoded. The cell's
//!   glow and ring arcs stay (they come from the player's own atlas); only
//!   the letters are filled in from their surroundings and the OpenSpore logo
//!   is drawn over them. Only the logo ships with OpenSpore;
//! * the legacy bevy-UI main menu (`player_hud`), which drew the
//!   `title-logo` texture: it draws [`Logo`] instead.
//!
//! ADAPTED (branding, not parity): the retail wordmark is Electronic Arts'.
//! The panel's other art (the strip, the buttons) is untouched.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::sync::OnceLock;

use crate::dbpf::ResourceKey;

/// The logo, transparent background (1959 x 366).
pub const LOGO_PNG: &[u8] = include_bytes!("branding/openspore-logo.png");

/// `GGEZoomPanel`'s atlas (512 x 256). The wordmark's letters (with the swirl
/// in the "O") cover x 10..=136, y 11..=38 of the top-left cell, inside the
/// cell's glow; the "TM" cell is x 0..=40, y 234..=255 (the button above it
/// ends at row 233).
pub const GGE_ZOOM_ATLAS: ResourceKey = ResourceKey {
    type_id: crate::ui_art::UI_PNG_TYPE,
    group_id: 0x31A4_4893,
    instance_id: 0x9E6C_B7F1,
};
const ATLAS_SIZE: [u32; 2] = [512, 256];
/// The rectangle refilled from its border (the letters plus 2 px), inclusive.
const LETTERS: [usize; 4] = [8, 9, 138, 40];
/// The "TM" cell, cleared: x range, y range.
const TM_CELL: (std::ops::Range<usize>, std::ops::Range<usize>) = (0..44, 234..256);
/// The OpenSpore logo in the cell: width, left edge, vertical centre (the
/// letters' centre); it comes out as tall as the old letters.
const LOGO_WIDTH: usize = 150;
const LOGO_LEFT: usize = 8;
const LOGO_CENTRE_Y: f32 = 26.0;
/// A light glow from the logo's shape (on top of the cell's own), so its
/// right end, past the retail glow, stays legible: blur margin, colour, gain.
const GLOW_PAD: usize = 8;
const GLOW_RGB: [u8; 3] = [220, 242, 255];
const GLOW_GAIN: f32 = 0.8;
/// Width of the legacy menu's copy (drawn 520 px wide).
const MENU_LOGO_WIDTH: usize = 640;

/// The logo decoded once: (width, height, straight RGBA8).
pub fn logo() -> Option<&'static (u32, u32, Vec<u8>)> {
    static LOGO: OnceLock<Option<(u32, u32, Vec<u8>)>> = OnceLock::new();
    LOGO.get_or_init(|| match crate::ui_art::decode_png(LOGO_PNG) {
        Ok(decoded) => Some(decoded),
        Err(e) => {
            warn!("OpenSpore logo: {e}");
            None
        }
    })
    .as_ref()
}

/// Replace the SPORE wordmark of a native texture being decoded. Only the
/// `GGEZoomPanel` atlas has one; every other texture is left as it is.
pub fn patch_texture(key: ResourceKey, width: u32, height: u32, rgba: &mut [u8]) {
    if key != GGE_ZOOM_ATLAS
        || [width, height] != ATLAS_SIZE
        || rgba.len() != (width * height * 4) as usize
    {
        return;
    }
    let Some((lw, lh, logo)) = logo() else { return };
    replace_wordmark(rgba, width as usize, (*lw as usize, *lh as usize), logo);
}

fn replace_wordmark(rgba: &mut [u8], stride: usize, (lw, lh): (usize, usize), logo: &[u8]) {
    let at = |x: usize, y: usize| (y * stride + x) * 4;
    // 1. Fill the letters' rectangle from its border (a Coons patch over the
    //    lightly smoothed border lines, premultiplied), so the cell's glow
    //    and arcs run on through it.
    let [x0, y0, x1, y1] = LETTERS;
    let line = |rgba: &[u8], pts: Vec<(usize, usize)>| -> Vec<[f32; 4]> {
        let raw: Vec<[f32; 4]> = pts.iter().map(|&(x, y)| premultiply(&rgba[at(x, y)..at(x, y) + 4])).collect();
        smooth(&raw, 2)
    };
    let top = line(rgba, (x0 - 1..=x1 + 1).map(|x| (x, y0 - 1)).collect());
    let bottom = line(rgba, (x0 - 1..=x1 + 1).map(|x| (x, y1 + 1)).collect());
    let left = line(rgba, (y0 - 1..=y1 + 1).map(|y| (x0 - 1, y)).collect());
    let right = line(rgba, (y0 - 1..=y1 + 1).map(|y| (x1 + 1, y)).collect());
    let (w, h) = ((x1 - x0 + 2) as f32, (y1 - y0 + 2) as f32);
    let (tl, tr, bl, br) = (top[0], top[top.len() - 1], bottom[0], bottom[bottom.len() - 1]);
    for y in y0..=y1 {
        let (j, v) = (y + 1 - y0, (y + 1 - y0) as f32 / h);
        for x in x0..=x1 {
            let (i, u) = (x + 1 - x0, (x + 1 - x0) as f32 / w);
            let mut c = [0f32; 4];
            for k in 0..4 {
                c[k] = (1.0 - v) * top[i][k] + v * bottom[i][k] + (1.0 - u) * left[j][k] + u * right[j][k]
                    - ((1.0 - u) * (1.0 - v) * tl[k] + u * (1.0 - v) * tr[k] + (1.0 - u) * v * bl[k] + u * v * br[k]);
            }
            rgba[at(x, y)..at(x, y) + 4].copy_from_slice(&unpremultiply(c));
        }
    }
    // 2. The "TM" goes.
    for y in TM_CELL.1.clone() {
        for x in TM_CELL.0.clone() {
            rgba[at(x, y)..at(x, y) + 4].fill(0);
        }
    }
    // 3. The logo at the wordmark's size, over a light glow of its own shape.
    let tw = LOGO_WIDTH;
    let th = ((tw * lh) as f32 / lw as f32).round().max(1.0) as usize;
    let small = box_resize(logo, (lw, lh), (tw, th));
    let top = (LOGO_CENTRE_Y - th as f32 / 2.0).round().max(0.0) as usize;
    let (gw, gh) = (tw + 2 * GLOW_PAD, th + 2 * GLOW_PAD);
    let mut mask = vec![0f32; gw * gh];
    for y in 0..th {
        for x in 0..tw {
            mask[(y + GLOW_PAD) * gw + x + GLOW_PAD] = small[(y * tw + x) * 4 + 3] as f32 / 255.0;
        }
    }
    for _ in 0..3 {
        box_blur(&mut mask, gw, gh, 3);
    }
    for gy in 0..gh {
        for gx in 0..gw {
            let (Some(x), Some(y)) = ((LOGO_LEFT + gx).checked_sub(GLOW_PAD), (top + gy).checked_sub(GLOW_PAD))
            else {
                continue;
            };
            if x < stride {
                over(&mut rgba[at(x, y)..at(x, y) + 4], GLOW_RGB, (mask[gy * gw + gx] * GLOW_GAIN).min(1.0));
            }
        }
    }
    for y in 0..th {
        for x in 0..tw {
            let (ax, ay) = (LOGO_LEFT + x, top + y);
            if ax < stride {
                let s = &small[(y * tw + x) * 4..(y * tw + x) * 4 + 4];
                over(&mut rgba[at(ax, ay)..at(ax, ay) + 4], [s[0], s[1], s[2]], s[3] as f32 / 255.0);
            }
        }
    }
}

fn premultiply(p: &[u8]) -> [f32; 4] {
    let a = p[3] as f32 / 255.0;
    [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
}

fn unpremultiply(c: [f32; 4]) -> [u8; 4] {
    let a = c[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0; 4];
    }
    let ch = |v: f32| (v / a).round().clamp(0.0, 255.0) as u8;
    [ch(c[0]), ch(c[1]), ch(c[2]), (a * 255.0).round() as u8]
}

/// Box-smooth a line of premultiplied colours (window 2r+1, ends clamped).
fn smooth(line: &[[f32; 4]], r: usize) -> Vec<[f32; 4]> {
    (0..line.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(r), (i + r).min(line.len() - 1));
            let n = (b - a + 1) as f32;
            let mut c = [0f32; 4];
            for p in &line[a..=b] {
                for k in 0..4 {
                    c[k] += p[k] / n;
                }
            }
            c
        })
        .collect()
}

/// Area-average resize of straight RGBA8 (colour weighted by alpha).
fn box_resize(src: &[u8], (sw, sh): (usize, usize), (tw, th): (usize, usize)) -> Vec<u8> {
    let (sx, sy) = (sw as f32 / tw as f32, sh as f32 / th as f32);
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        let (y0, y1) = (ty as f32 * sy, (ty + 1) as f32 * sy);
        for tx in 0..tw {
            let (x0, x1) = (tx as f32 * sx, (tx + 1) as f32 * sx);
            let mut acc = [0f32; 4];
            let mut area = 0f32;
            for y in (y0.floor() as usize)..(y1.ceil() as usize).min(sh) {
                let wy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).max(0.0);
                for x in (x0.floor() as usize)..(x1.ceil() as usize).min(sw) {
                    let w = wy * (x1.min(x as f32 + 1.0) - x0.max(x as f32)).max(0.0);
                    let i = (y * sw + x) * 4;
                    let a = src[i + 3] as f32 / 255.0 * w;
                    for c in 0..3 {
                        acc[c] += src[i + c] as f32 * a;
                    }
                    acc[3] += a;
                    area += w;
                }
            }
            if area > 0.0 && acc[3] > 0.0 {
                let o = (ty * tw + tx) * 4;
                for c in 0..3 {
                    out[o + c] = (acc[c] / acc[3]).round().clamp(0.0, 255.0) as u8;
                }
                out[o + 3] = (acc[3] / area * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Separable box blur (window 2r+1), outside = 0.
fn box_blur(v: &mut [f32], w: usize, h: usize, r: usize) {
    let n = (2 * r + 1) as f32;
    let mut tmp = vec![0f32; v.len()];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = v[y * w + a..=y * w + b].iter().sum::<f32>() / n;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            v[y * w + x] = (a..=b).map(|yy| tmp[yy * w + x]).sum::<f32>() / n;
        }
    }
}

/// Straight-alpha "over" of a colour with coverage `sa` onto `dst` (RGBA8).
fn over(dst: &mut [u8], src: [u8; 3], sa: f32) {
    if sa <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    for c in 0..3 {
        let v = (src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa)) / oa;
        dst[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// The logo as a UI image (the legacy main menu's title).
#[derive(Resource, Clone)]
pub struct Logo {
    pub image: Handle<Image>,
    pub size: [u32; 2],
}

pub struct BrandingPlugin;

impl Plugin for BrandingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_logo);
    }
}

fn load_logo(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let Some((lw, lh, logo)) = logo() else { return };
    let (lw, lh) = (*lw as usize, *lh as usize);
    let (w, h) = (MENU_LOGO_WIDTH, ((MENU_LOGO_WIDTH * lh) as f32 / lw as f32).round() as usize);
    let rgba = box_resize(logo, (lw, lh), (w, h));
    let image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    commands.insert_resource(Logo {
        image: images.add(image),
        size: [w as u32, h as u32],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: [u8; 4] = [78, 97, 184, 131];
    const OLD_LETTERS: [u8; 4] = [20, 40, 120, 255];
    const TM: [u8; 4] = [255, 255, 255, 255];
    const BUTTON: [u8; 4] = [60, 60, 200, 200];

    /// A stand-in for the atlas: the strip with the old letters, the TM
    /// glyph, a button just above it, and other art.
    fn atlas() -> Vec<u8> {
        let (w, h) = (ATLAS_SIZE[0] as usize, ATLAS_SIZE[1] as usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let px = if (10..=136).contains(&x) && (11..=38).contains(&y) {
                    OLD_LETTERS
                } else if y < 52 && x < 275 - 2 * y {
                    STRIP
                } else if (0..=40).contains(&x) && (234..=255).contains(&y) {
                    TM
                } else if x < 60 && (222..=233).contains(&y) {
                    BUTTON
                } else if y >= 52 {
                    [200, 10, 10, 255]
                } else {
                    [0; 4]
                };
                let i = (y * w + x) * 4;
                rgba[i..i + 4].copy_from_slice(&px);
            }
        }
        rgba
    }

    fn px(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * ATLAS_SIZE[0] as usize + x) * 4;
        rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn logo_decodes_with_its_wordmark_aspect() {
        let (w, h, rgba) = logo().expect("embedded logo decodes");
        assert_eq!((*w, *h), (1959, 366));
        assert_eq!(rgba.len(), (*w * *h * 4) as usize);
        // Transparent corners, opaque body.
        assert_eq!(rgba[3], 0);
        assert!(rgba.chunks(4).filter(|p| p[3] == 255).count() > 100_000);
    }

    #[test]
    fn the_zoom_panel_wordmark_becomes_the_openspore_logo() {
        let mut rgba = atlas();
        let before = rgba.clone();
        patch_texture(GGE_ZOOM_ATLAS, ATLAS_SIZE[0], ATLAS_SIZE[1], &mut rgba);
        // No old letter pixel is left: each one is now fill, glow or logo.
        for y in 11..=38 {
            for x in 10..=136 {
                assert_ne!(px(&rgba, x, y), OLD_LETTERS, "old wordmark pixel at {x},{y}");
            }
        }
        // The fill continues the strip: above the logo, inside the old
        // letters' rectangle, the colour is the strip's, lit by the glow.
        let fill = px(&rgba, 70, 9);
        assert!(fill[3] >= STRIP[3] && fill[2] >= STRIP[2], "fill {fill:?}");
        // The logo is drawn: opaque blue at mid-height (the left side of the "O").
        let mid = px(&rgba, LOGO_LEFT + 5, 26);
        assert!(mid[3] > 240 && mid[2] > mid[0] + 60, "logo pixel {mid:?}");
        // The TM is gone; the button above it and the cell's corner stay.
        for y in 234..=255 {
            for x in 0..=40 {
                assert_eq!(px(&rgba, x, y)[3], 0, "TM pixel at {x},{y}");
            }
        }
        for y in 222..=233 {
            for x in 0..60 {
                assert_eq!(px(&rgba, x, y), BUTTON);
            }
        }
        assert_eq!(px(&rgba, 0, 0), STRIP);
        // Art away from the wordmark and the TM is untouched.
        for (x, y) in [(400, 5), (300, 100), (100, 200), (511, 255), (60, 240)] {
            assert_eq!(px(&rgba, x, y), px(&before, x, y), "pixel at {x},{y}");
        }
    }

    #[test]
    fn other_textures_are_untouched() {
        let mut rgba = atlas();
        let before = rgba.clone();
        let other = ResourceKey { instance_id: 0x9E6C_B7F2, ..GGE_ZOOM_ATLAS };
        patch_texture(other, ATLAS_SIZE[0], ATLAS_SIZE[1], &mut rgba);
        assert!(rgba == before);
        // A different size under the same key (a mod's atlas) is left alone too.
        let mut small = vec![7u8; 64 * 64 * 4];
        patch_texture(GGE_ZOOM_ATLAS, 64, 64, &mut small);
        assert!(small.iter().all(|&b| b == 7));
    }

    #[test]
    fn box_resize_keeps_colour_under_partial_alpha() {
        // Half-transparent white next to opaque blue: the colours mix by
        // coverage (white weighs half as much as the blue), the alphas average.
        let src = [255, 255, 255, 128, 0, 0, 255, 255];
        let out = box_resize(&src, (2, 1), (1, 1));
        assert!((191..=192).contains(&out[3]), "{out:?}");
        assert!((80..=90).contains(&out[0]) && out[2] == 255, "{out:?}");
    }

    #[test]
    fn coons_fill_of_a_flat_border_is_flat() {
        let line = vec![premultiply(&STRIP); 9];
        let s = smooth(&line, 2);
        assert!(s.iter().all(|c| (c[3] - STRIP[3] as f32 / 255.0).abs() < 1e-6));
        assert_eq!(unpremultiply(s[4]), STRIP);
    }
}
