//! Caption outlines from one entity.
//!
//! The retail text path draws the glyph batch of an outlined caption once per kernel copy
//! (`UTFWin` 0x96E690, see [`super::outline`]). Doing the same with one Bevy text node per copy costs
//! every glyph of every copy again each frame (Bevy rebuilds the UI vertex buffer from all glyph
//! quads every frame), so a caption with a size 3 outline pays for 48 extra captions.
//!
//! This module draws the copies from ONE node per caption instead: [`update_masks`] rasterises the
//! glyph quads that Bevy's text layout produced for the caption (the same atlas texels, at the
//! same subpixel positions, bilinear like the GPU) into a small coverage mask, only when the
//! caption's layout or place changes; the node's [`SpuiCopiesMaterial`] (`shaders/copies.wesl`, the
//! one that draws the copies of image layers) then sums the kernel's shifted, tinted samples of the
//! mask in one draw. The caption itself stays an ordinary text node on top.
//!
//! Everything here is presentation: the kernel (taps, colour, alpha) is [`super::outline`], the
//! node is spawned and kept in step by [`super::view`]'s `sync`, this system runs after the UI
//! layout and text systems of the same frame, so a changed caption and its outline always show
//! together.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::surface::SpuiCopiesMaterial;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    text::TextLayoutInfo,
    ui::{ComputedNode, UiGlobalTransform},
};

/// Attached to the text node (the root `Text` of a caption's box) of a caption that draws its
/// outline copies with a mask: where the mask is, how big it is and which box it belongs to.
#[derive(Component)]
pub struct CaptionMask {
    /// The caption's flex box (origin of the mask's coordinate system).
    pub outer: Entity,
    /// The mask image (what `update_masks` writes).
    pub image: Handle<Image>,
    /// The material that samples the mask. Bevy does not rebuild a material's bind group when an
    /// image it uses is replaced, so every redraw touches the material too.
    pub material: Handle<SpuiCopiesMaterial>,
    /// The caption box in pixels (the part of the mask that is the box itself).
    pub box_size: [u32; 2],
    /// Extra pixels on every side of the box that the mask covers (text wider or taller than its
    /// box). The copies node is sized with the same value.
    pub pad: u32,
    /// Overshoot of the glyphs over the box at the last redraw (written here, read by `sync`).
    pub needed_pad: Arc<AtomicU32>,
    /// What the image shows now.
    drawn: Option<Drawn>,
}

#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    origin: Vec2,
    box_size: [u32; 2],
    pad: u32,
}

impl CaptionMask {
    pub fn new(
        outer: Entity,
        image: Handle<Image>,
        material: Handle<SpuiCopiesMaterial>,
        box_size: [u32; 2],
        pad: u32,
        needed_pad: Arc<AtomicU32>,
    ) -> Self {
        Self { outer, image, material, box_size, pad, needed_pad, drawn: None }
    }

    /// Mask size in pixels: the box plus the pad on every side.
    pub fn layer_size(box_size: [u32; 2], pad: u32) -> [u32; 2] {
        [box_size[0] + 2 * pad, box_size[1] + 2 * pad]
    }
}

/// A glyph atlas texture as the rasteriser reads it: RGBA8, only the alpha byte is used.
#[derive(Clone, Copy)]
pub struct AlphaAtlas<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl AlphaAtlas<'_> {
    fn alpha(&self, x: i32, y: i32) -> f32 {
        let x = x.clamp(0, self.width as i32 - 1) as usize;
        let y = y.clamp(0, self.height as i32 - 1) as usize;
        self.rgba.get((y * self.width as usize + x) * 4 + 3).map_or(0.0, |a| *a as f32 / 255.0)
    }

    /// Bilinear sample at the continuous texel position `s` (texel `k` covers `[k, k + 1)`),
    /// clamped to the edge like the GPU sampler of the glyph atlas.
    fn sample(&self, s: Vec2) -> f32 {
        let u = s - Vec2::splat(0.5);
        let (x0, y0) = (u.x.floor(), u.y.floor());
        let (fx, fy) = (u.x - x0, u.y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let top = self.alpha(x0, y0) * (1.0 - fx) + self.alpha(x0 + 1, y0) * fx;
        let bottom = self.alpha(x0, y0 + 1) * (1.0 - fx) + self.alpha(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// One glyph quad of a text layout: its centre (in the layout's pixels), the texel rectangle of
/// the glyph in its atlas.
#[derive(Clone, Copy)]
pub struct Glyph<'a> {
    pub center: Vec2,
    pub rect_min: Vec2,
    pub size: Vec2,
    pub atlas: AlphaAtlas<'a>,
}

/// The coverage mask of a caption.
pub struct Mask {
    pub width: u32,
    pub height: u32,
    /// RGBA8: white with the accumulated glyph coverage as alpha.
    pub rgba: Vec<u8>,
    /// Bounds of all glyph quads in mask pixels, when there are any.
    pub bounds: Option<(Vec2, Vec2)>,
}

/// Rasterises `glyphs` into a `width` x `height` mask. `origin` is where the layout's top-left
/// corner lies in mask pixels. Each quad is sampled exactly like the UI shader does it: pixel
/// centres inside the quad, the texel position linear across the quad, bilinear filtering;
/// quads blend over each other ("over") like consecutive glyph draws.
pub fn rasterize(width: u32, height: u32, origin: Vec2, glyphs: &[Glyph]) -> Mask {
    let mut coverage = vec![0.0f32; width as usize * height as usize];
    let mut bounds: Option<(Vec2, Vec2)> = None;
    for g in glyphs {
        if g.size.x <= 0.0 || g.size.y <= 0.0 {
            continue;
        }
        let q0 = origin + g.center - g.size * 0.5;
        let q1 = q0 + g.size;
        bounds = Some(match bounds {
            Some((lo, hi)) => (lo.min(q0), hi.max(q1)),
            None => (q0, q1),
        });
        // Pixel `i` is covered when its centre `i + 0.5` lies in `[q0, q1)`.
        let first = |v: f32| ((v - 0.5).ceil() as i32).max(0);
        let last = |v: f32, limit: u32| (((v - 0.5).ceil() as i32) - 1).min(limit as i32 - 1);
        let (x_from, x_to) = (first(q0.x), last(q1.x, width));
        let (y_from, y_to) = (first(q0.y), last(q1.y, height));
        for y in y_from..=y_to {
            for x in x_from..=x_to {
                let s = g.rect_min + (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - q0);
                let a = g.atlas.sample(s);
                if a > 0.0 {
                    let c = &mut coverage[y as usize * width as usize + x as usize];
                    *c = a + *c * (1.0 - a);
                }
            }
        }
    }
    let mut rgba = Vec::with_capacity(coverage.len() * 4);
    for c in coverage {
        rgba.extend_from_slice(&[255, 255, 255, (c.clamp(0.0, 1.0) * 255.0).round() as u8]);
    }
    Mask { width, height, rgba, bounds }
}

/// `OS_SPUI_TRACE_MASKS=1` prints one line per mask redraw (diagnostics).
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OS_SPUI_TRACE_MASKS").is_some())
}

/// The mask image for a mask of the given size: `Rgba8Unorm`, raw values like every SPUI texture
/// of the gamma surface.
pub fn mask_image(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    Image::new(
        Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// A blank (fully transparent) mask image of the given size.
pub fn blank_image(width: u32, height: u32) -> Image {
    mask_image(width, height, vec![0; width.max(1) as usize * height.max(1) as usize * 4])
}

/// Redraws the masks of the captions whose layout or place changed. Runs after the UI layout and
/// the text systems of the frame (`UiSystems::PostLayout`), so the mask matches the caption node
/// that is drawn in the same frame.
pub fn update_masks(
    mut masks: Query<
        (&mut CaptionMask, Ref<TextLayoutInfo>, &ComputedNode, &UiGlobalTransform),
        Or<(Changed<CaptionMask>, Changed<TextLayoutInfo>, Changed<UiGlobalTransform>)>,
    >,
    outers: Query<(&ComputedNode, &UiGlobalTransform)>,
    images: Option<ResMut<Assets<Image>>>,
    mut materials: Option<ResMut<Assets<SpuiCopiesMaterial>>>,
) {
    let Some(mut images) = images else {
        return;
    };
    for (mut mask, layout, node, transform) in &mut masks {
        let Ok((outer_node, outer_transform)) = outers.get(mask.outer) else {
            continue;
        };
        // Where the text's content box starts, in the caption box's own pixels (its top-left
        // corner is the origin): the offset of the two node centres in the outer node's frame
        // (undoing the window effects both share), plus the content box inside its node.
        let Some(inverse) = outer_transform.try_inverse() else {
            continue;
        };
        // All in physical pixels, which are the mask's pixels (the SPUI surface has scale factor 1).
        let delta = inverse.matrix2 * (transform.translation - outer_transform.translation);
        let content = node.content_box();
        let origin_in_box = outer_node.size() * 0.5 + delta + content.min;
        // On a 1/32 pixel grid: the transforms of animated windows jitter in the last bits, which
        // must not count as a change.
        let origin = ((origin_in_box + Vec2::splat(mask.pad as f32)) * 32.0).round() / 32.0;
        let wanted = Drawn { origin, box_size: mask.box_size, pad: mask.pad };
        if mask.drawn == Some(wanted) && !layout.is_changed() {
            continue;
        }
        let [width, height] = CaptionMask::layer_size(mask.box_size, mask.pad);
        let mut glyphs = Vec::with_capacity(layout.glyphs.len());
        for g in &layout.glyphs {
            let Some(atlas) = images.get(g.atlas_info.texture) else {
                continue;
            };
            let Some(rgba) = atlas.data.as_deref() else {
                continue;
            };
            if atlas.texture_descriptor.format.block_copy_size(None) != Some(4) {
                continue;
            }
            glyphs.push(Glyph {
                center: g.position,
                rect_min: g.atlas_info.rect.min,
                size: g.atlas_info.rect.size(),
                atlas: AlphaAtlas { width: atlas.width(), height: atlas.height(), rgba },
            });
        }
        let drawn = rasterize(width, height, origin, &glyphs);
        // How far the glyphs reach outside the box itself, measured in the box's own pixels (so it
        // does not depend on the pad): `sync` sizes the mask with it one frame later (a larger
        // text is cropped until then).
        let pad = mask.pad as f32;
        let over = drawn.bounds.map_or(0.0, |(lo, hi)| {
            let (lo, hi) = (lo - Vec2::splat(pad), hi - Vec2::splat(pad));
            (-lo.x).max(-lo.y).max(hi.x - mask.box_size[0] as f32).max(hi.y - mask.box_size[1] as f32).max(0.0)
        });
        mask.needed_pad.store(over.ceil() as u32, Ordering::Relaxed);
        if trace() {
            println!(
                "SPUI_MASK {}x{} pad {} origin {:?} glyphs {} of {} bounds {:?} lit {}",
                width,
                height,
                mask.pad,
                origin,
                glyphs.len(),
                layout.glyphs.len(),
                drawn.bounds,
                drawn.rgba.chunks(4).filter(|p| p[3] > 0).count()
            );
        }
        let image = mask_image(drawn.width, drawn.height, drawn.rgba);
        if let Some(mut slot) = images.get_mut(&mask.image) {
            *slot = image;
        }
        // Touch the material (a changed uniform) so that it binds the new texture.
        if let Some(mut m) = materials.as_mut().and_then(|m| m.get_mut(&mask.material)) {
            m.params.info.z = (m.params.info.z + 1.0) % 1024.0;
        }
        mask.bypass_change_detection().drawn = Some(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 x 4 atlas whose texels carry their index as alpha.
    fn atlas(data: &mut Vec<u8>) -> AlphaAtlas<'_> {
        data.clear();
        for i in 0..16u32 {
            data.extend_from_slice(&[255, 255, 255, (i * 16) as u8]);
        }
        AlphaAtlas { width: 4, height: 4, rgba: data }
    }

    fn glyph(atlas: AlphaAtlas<'_>, center: Vec2, rect_min: Vec2, size: Vec2) -> Glyph<'_> {
        Glyph { center, rect_min, size, atlas }
    }

    fn alpha_at(mask: &Mask, x: u32, y: u32) -> u8 {
        mask.rgba[(y as usize * mask.width as usize + x as usize) * 4 + 3]
    }

    #[test]
    fn a_glyph_on_whole_pixels_is_copied_texel_for_texel() {
        let mut data = Vec::new();
        let a = atlas(&mut data);
        // The 2 x 2 texels at (1, 1) placed with the quad's top-left corner at (3, 2).
        let g = glyph(a, Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0));
        let mask = rasterize(8, 8, Vec2::new(3.0, 2.0), &[g]);
        assert_eq!(alpha_at(&mask, 3, 2), 5 * 16);
        assert_eq!(alpha_at(&mask, 4, 2), 6 * 16);
        assert_eq!(alpha_at(&mask, 3, 3), 9 * 16);
        assert_eq!(alpha_at(&mask, 4, 3), 10 * 16);
        // Nothing outside the quad.
        assert_eq!(alpha_at(&mask, 2, 2), 0);
        assert_eq!(alpha_at(&mask, 5, 2), 0);
        assert_eq!(alpha_at(&mask, 3, 4), 0);
        assert_eq!(mask.bounds, Some((Vec2::new(3.0, 2.0), Vec2::new(5.0, 4.0))));
        assert!(mask.rgba.chunks(4).all(|p| p[..3] == [255, 255, 255]));
    }

    #[test]
    fn a_half_pixel_shift_is_the_bilinear_average_like_the_gpu() {
        let mut data = Vec::new();
        let a = atlas(&mut data);
        // The quad's corner at (3.5, 2). Pixel x = 3 has its centre on the quad's left edge: the
        // texel position is the boundary between texel 0 (alpha 64 in row 1) and texel 1 (80).
        let g = glyph(a, Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0));
        let mask = rasterize(8, 8, Vec2::new(3.5, 2.0), &[g]);
        assert_eq!(alpha_at(&mask, 3, 2), 72);
        // x = 4: halfway between texel 1 (80) and texel 2 (96).
        assert_eq!(alpha_at(&mask, 4, 2), 88);
        // The quad ends at 5.5: pixel 5 (centre 5.5) is outside.
        assert_eq!(alpha_at(&mask, 5, 2), 0);
    }

    #[test]
    fn overlapping_quads_blend_over_each_other_and_stay_inside_the_mask() {
        let mut data = Vec::new();
        let a = atlas(&mut data);
        // Texel 15 (alpha 240) twice on the same pixel: 240 + 240 * (1 - 240/255) = 254 or 255.
        let g = glyph(a, Vec2::new(3.5, 3.5), Vec2::new(3.0, 3.0), Vec2::new(1.0, 1.0));
        let origin = Vec2::new(-3.0, -3.0);
        let mask = rasterize(2, 2, origin, &[g, g]);
        assert_eq!(alpha_at(&mask, 0, 0), 254);
        // A quad partly outside the mask is clipped, its bounds still tell how far it reaches.
        let far = glyph(a, Vec2::new(0.5, 0.5), Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        let mask = rasterize(2, 2, Vec2::new(-5.0, 0.0), &[far]);
        assert!(mask.rgba.chunks(4).all(|p| p[3] == 0));
        assert_eq!(mask.bounds, Some((Vec2::new(-5.0, 0.0), Vec2::new(-4.0, 1.0))));
    }

    #[test]
    fn sampling_clamps_at_the_atlas_edge_and_empty_input_gives_a_blank_mask() {
        let mut data = Vec::new();
        let a = atlas(&mut data);
        assert_eq!(a.sample(Vec2::new(-3.0, 0.5)), 0.0, "texel 0 has alpha 0");
        assert_eq!(a.sample(Vec2::new(10.0, 3.5)), 15.0 * 16.0 / 255.0);
        let mask = rasterize(3, 2, Vec2::ZERO, &[]);
        assert_eq!(mask.rgba.len(), 3 * 2 * 4);
        assert!(mask.bounds.is_none());
        assert!(mask.rgba.chunks(4).all(|p| p[3] == 0));
    }

    fn app_with(glyph_center: Vec2) -> (App, Entity, Handle<Image>, Arc<AtomicU32>) {
        use bevy::text::{GlyphAtlasInfo, PositionedGlyph};
        let mut images = Assets::<Image>::default();
        let mut data = Vec::new();
        let atlas_alpha = atlas(&mut data);
        let atlas_image = images.add(Image::new(
            Extent3d { width: atlas_alpha.width, height: atlas_alpha.height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            atlas_alpha.rgba.to_vec(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        ));
        let mask_image = images.add(blank_image(8, 4));
        let mut materials = Assets::<SpuiCopiesMaterial>::default();
        let material = materials.add(SpuiCopiesMaterial::default());
        let mut app = App::new();
        app.insert_resource(images).insert_resource(materials).add_systems(Update, update_masks);
        // A box of 8 x 4 pixels centred at (100, 50); its text node (4 x 2) one pixel right of the centre.
        let node = |w: f32, h: f32| ComputedNode { size: Vec2::new(w, h), inverse_scale_factor: 1.0, ..Default::default() };
        let outer = app.world_mut().spawn((node(8.0, 4.0), UiGlobalTransform::from_xy(100.0, 50.0))).id();
        let needed = Arc::new(AtomicU32::new(0));
        let layout = TextLayoutInfo {
            glyphs: vec![PositionedGlyph {
                position: glyph_center,
                atlas_info: GlyphAtlasInfo {
                    texture: atlas_image.id(),
                    rect: Rect::new(0.0, 0.0, 2.0, 2.0),
                    offset: Vec2::ZERO,
                    is_alpha_mask: true,
                },
                section_index: 0,
                line_index: 0,
            }],
            ..Default::default()
        };
        let inner = app
            .world_mut()
            .spawn((
                CaptionMask::new(outer, mask_image.clone(), material, [8, 4], 0, needed.clone()),
                layout,
                node(4.0, 2.0),
                UiGlobalTransform::from_xy(101.0, 50.0),
            ))
            .id();
        (app, inner, mask_image, needed)
    }

    #[test]
    fn the_system_draws_the_glyphs_where_the_text_node_sits_in_the_box() {
        // The text node's top-left corner is at (3, 1) in the box; a 2 x 2 glyph centred at (1, 1) of
        // its layout covers the box pixels (3..5, 1..3), reading the atlas texels (0..2, 0..2).
        let (mut app, _, mask_image, needed) = app_with(Vec2::new(1.0, 1.0));
        app.update();
        let images = app.world().resource::<Assets<Image>>();
        let mask = images.get(&mask_image).unwrap();
        assert_eq!((mask.width(), mask.height()), (8, 4));
        let data = mask.data.as_ref().unwrap();
        let alpha = |x: usize, y: usize| data[(y * 8 + x) * 4 + 3];
        assert_eq!((alpha(3, 1), alpha(4, 1), alpha(3, 2), alpha(4, 2)), (0, 16, 64, 80));
        assert_eq!(alpha(2, 1), 0);
        assert_eq!(alpha(5, 2), 0);
        assert_eq!(needed.load(Ordering::Relaxed), 0, "inside the box: no pad needed");
    }

    #[test]
    fn a_glyph_outside_the_box_asks_for_a_pad() {
        // The same glyph centred at x = 10 of the layout: box pixels 12..14 of a box 8 wide.
        let (mut app, _, mask_image, needed) = app_with(Vec2::new(10.0, 1.0));
        app.update();
        assert_eq!(needed.load(Ordering::Relaxed), 6);
        let images = app.world().resource::<Assets<Image>>();
        let mask = images.get(&mask_image).unwrap();
        assert!(mask.data.as_ref().unwrap().chunks(4).all(|p| p[3] == 0), "cropped until the node grows");
    }

    #[test]
    fn layer_size_adds_the_pad_on_both_sides() {
        assert_eq!(CaptionMask::layer_size([10, 4], 0), [10, 4]);
        assert_eq!(CaptionMask::layer_size([10, 4], 3), [16, 10]);
    }
}
