//! The three dynamic textures of the Cell water (`notes\cell-fluid3-notes.md`).
//!
//! Retail creates them at level start (0xE81E20 -> 0xE834D0: three
//! `ITextureManager` textures of the grid without its border ring, 18 x 13
//! texels, into the three `TexturePtr`s at `cCellGFX+0x40..+0x48`) and uploads
//! them every frame from the water's grids (0xE831A0, the end of the GFX post
//! step 0xE73F60: velocity grid `f+0x1C210`, density grid `f+0x1BD60`):
//!
//! | id | format | texels (0xE82E00 / 0xE83040) |
//! |----|--------|------------------------------|
//! | 0xAE10ABF2 | 0x32 = D3DFMT_L8 | the density grid: `clamp((rho - 4) / (3 + 2^-16), 0, 1) * 255` (0x1041CE0 with 4, 7) |
//! | 0x0D1B9EA8 | 0x15 = A8R8G8B8 | a normal map made from the L8 texture (0x7C2610, strength 5, Roberts-cross corner differences, wrapping at the edges) |
//! | 0x6149AF6B | 0x15 = A8R8G8B8 | the velocity grid: each of x, y, z mapped from -1..1 to 0..255 (0xE82520), alpha 255 |
//!
//! Only the interior of the 20 x 15 grid (cells 1..=18 x 1..=13) is used, the
//! rows are written bottom-up (memory row 0 = the grid's row 13, so the image
//! is the water seen with +y up), values are rounded to nearest even (`cvtss2si`).
//! An installed effect names the normal map as the texture of a particle
//! (effect resource 0xC1195706, texture slot `00000000!0d1b9ea8`): the Swarm
//! renderer asks `image_for` before it looks for an installed raster.
//!
//! ADAPTED: the sampler (filter and address modes of the effect's material are
//! not decoded here: linear, clamp), and until the first upload the textures
//! hold neutral data (black density, flat normals, zero velocity).

use std::sync::Mutex;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::cell_fluid::{FluidGrid, GRID_H, GRID_W};

/// 0xAE10ABF2: the L8 density map.
pub const DENSITY_ID: u32 = 0xAE10_ABF2;
/// 0x0D1B9EA8: the normal map of the density map.
pub const NORMAL_ID: u32 = 0x0D1B_9EA8;
/// 0x6149AF6B: the velocity map.
pub const VELOCITY_ID: u32 = 0x6149_AF6B;
/// Texture size: the grid without its border ring (0xE834D0: width - 2, height - 2).
pub const TEX_W: usize = GRID_W - 2;
pub const TEX_H: usize = GRID_H - 2;
/// The density range mapped to 0..255 (the immediates of 0xE82EE3 / 0xE82ED9).
pub const DENSITY_LOW: f32 = 4.0;
pub const DENSITY_HIGH: f32 = 7.0;
/// The normal map's strength argument (0xE82F8C, `.rdata` 0x14852F8 = 5): the
/// z component is `1020 / 5`.
pub const NORMAL_STRENGTH: f32 = 5.0;
const NORMAL_Z: f32 = 1020.0 / NORMAL_STRENGTH;
/// `2^-16` (.rdata 0x1499CF0).
const EPSILON: f32 = 1.0 / 65536.0;

/// 0x1041CE0 `(a, b, x)`: `(x - a) / (b - a + 2^-16)`.
fn inverse_lerp(a: f32, b: f32, x: f32) -> f32 {
    (x - a) / (b - a + EPSILON)
}

/// `maxss(0, v)` then `minss(1)`: NaN reads as 1 (the second operand wins).
fn clamp01(v: f32) -> f32 {
    if v.is_nan() {
        1.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// `cvtss2si`: round to nearest, ties to even.
fn round_even(v: f32) -> i32 {
    v.round_ties_even() as i32
}

fn byte(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// A8R8G8B8.
fn pack(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// 0xE82E00, first half: the L8 density map, `TEX_W x TEX_H` bytes in memory
/// order (row 0 on top).
pub fn density_texture(grid: &FluidGrid) -> Vec<u8> {
    let mut out = vec![0u8; TEX_W * TEX_H];
    for row in 0..TEX_H {
        // Written bottom-up: the first source row (grid row 1) lands in the last row.
        let iy = TEX_H - row;
        for col in 0..TEX_W {
            let rho = grid.density[iy * GRID_W + col + 1];
            let v = clamp01(inverse_lerp(DENSITY_LOW, DENSITY_HIGH, rho));
            out[row * TEX_W + col] = byte(round_even(v * 255.0));
        }
    }
    out
}

/// One entry of the table 0x7C2570 builds: `127.5 / sqrt(i * 64 + z^2)` by the
/// bit-trick inverse square root (`0x5F400000 - (bits >> 1)`) and one Newton
/// step, exactly as the code computes it.
fn normal_scale(i: u32) -> f32 {
    let x = i as f32 * 64.0 + NORMAL_Z * NORMAL_Z;
    let y = f32::from_bits(0x5F40_0000u32.wrapping_sub(x.to_bits() >> 1));
    let t = y * y * (x * 0.5);
    (1.5 - t) * y * 127.5
}

/// 0x7C2610: the A8R8G8B8 normal map of an L8 map (`TEX_W x TEX_H`). Per texel
/// the four diagonal neighbours (wrapping at the edges) give
/// `m = (tl + bl) - (tr + br)` and `p = (tl + tr) - (bl + br)`; the texel is
/// `(m, p, 1020 / strength) * 127.5 / |(m, p, 1020 / strength)| + 128`, clamped
/// (the length is looked up by `(m^2 + p^2) >> 6`, see `normal_scale`).
pub fn normal_texture(density: &[u8]) -> Vec<u32> {
    let at = |x: isize, y: isize| -> i32 {
        let (w, h) = (TEX_W as isize, TEX_H as isize);
        density[(y.rem_euclid(h) * w + x.rem_euclid(w)) as usize] as i32
    };
    let mut out = vec![0u32; TEX_W * TEX_H];
    for y in 0..TEX_H as isize {
        for x in 0..TEX_W as isize {
            let (tl, tr) = (at(x - 1, y - 1), at(x + 1, y - 1));
            let (bl, br) = (at(x - 1, y + 1), at(x + 1, y + 1));
            let a = tl - br;
            let b = tr - bl;
            let (m, p) = (a - b, a + b);
            let t = normal_scale(((p * p + m * m) >> 6) as u32);
            let r = byte(round_even(m as f32 * t) + 0x80);
            let g = byte(round_even(p as f32 * t) + 0x80);
            let z = byte(round_even(t * NORMAL_Z) + 0x80);
            out[y as usize * TEX_W + x as usize] = pack(r, g, z);
        }
    }
    out
}

/// 0xE82520: one velocity component from -1..1 to a byte.
fn velocity_byte(v: f32) -> u8 {
    byte(round_even(clamp01((v + 1.0) / 2.0) * 255.0))
}

/// 0xE83040: the A8R8G8B8 velocity map (x, y from the grid, z = 0), same
/// interior and row order as the density map.
pub fn velocity_texture(grid: &FluidGrid) -> Vec<u32> {
    let mut out = vec![0u32; TEX_W * TEX_H];
    for row in 0..TEX_H {
        let iy = TEX_H - row;
        for col in 0..TEX_W {
            let v = grid.velocity[iy * GRID_W + col + 1];
            out[row * TEX_W + col] = pack(velocity_byte(v.x), velocity_byte(v.y), velocity_byte(0.0));
        }
    }
    out
}

/// RGBA bytes of an L8 map (D3D9 reads L8 as (l, l, l, 1)).
fn l8_rgba(l8: &[u8]) -> Vec<u8> {
    l8.iter().flat_map(|&l| [l, l, l, 255]).collect()
}

/// RGBA bytes of A8R8G8B8 texels.
fn argb_rgba(argb: &[u32]) -> Vec<u8> {
    argb.iter()
        .flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8])
        .collect()
}

/// The three maps as RGBA bytes, in the order density, normal, velocity.
pub fn maps(grid: &FluidGrid) -> [Vec<u8>; 3] {
    let density = density_texture(grid);
    let normal = normal_texture(&density);
    [l8_rgba(&density), argb_rgba(&normal), argb_rgba(&velocity_texture(grid))]
}

/// What the stage uploaded last and the images that show it.
struct Shared {
    texels: [Vec<u8>; 3],
    handles: Vec<(u32, Handle<Image>)>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    texels: [Vec::new(), Vec::new(), Vec::new()],
    handles: Vec::new(),
});

fn slot(id: u32) -> Option<usize> {
    match id {
        DENSITY_ID => Some(0),
        NORMAL_ID => Some(1),
        VELOCITY_ID => Some(2),
        _ => None,
    }
}

/// Neutral contents before the first upload.
fn neutral(slot: usize) -> Vec<u8> {
    let texel: [u8; 4] = match slot {
        0 => [0, 0, 0, 255],
        1 => [128, 128, 255, 255],
        _ => [128, 128, 128, 255],
    };
    texel.iter().copied().cycle().take(TEX_W * TEX_H * 4).collect()
}

/// Store this frame's maps (0xE831A0). Returns whether anything changed.
pub fn publish(grid: &FluidGrid) -> bool {
    let maps = maps(grid);
    let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    let changed = shared.texels.iter().zip(&maps).any(|(a, b)| a != b);
    shared.texels = maps;
    changed
}

/// The image of a dynamic texture id for the Swarm renderer (`None` for every
/// other id). `raw` selects linear rather than sRGB sampling.
pub fn image_for(id: u32, raw: bool, repeat: bool) -> Option<Image> {
    let slot = slot(id)?;
    // `OPENSPORE_NO_FLUID_TEXTURES=1` (diagnostics, A/B captures): the textures
    // stay uninstalled, as before they were ported.
    if std::env::var_os("OPENSPORE_NO_FLUID_TEXTURES").is_some() {
        return None;
    }
    let data = {
        let shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if shared.texels[slot].is_empty() {
            neutral(slot)
        } else {
            shared.texels[slot].clone()
        }
    };
    let mut image = Image::new(
        Extent3d { width: TEX_W as u32, height: TEX_H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        if raw { TextureFormat::Rgba8Unorm } else { TextureFormat::Rgba8UnormSrgb },
        RenderAssetUsages::default(),
    );
    let address = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    Some(image)
}

/// The renderer made an image for `id`: keep it up to date.
pub fn register(id: u32, handle: Handle<Image>) {
    if slot(id).is_some() {
        SHARED.lock().unwrap_or_else(|e| e.into_inner()).handles.push((id, handle));
    }
}

/// Write the latest maps into every registered image (retail uploads them
/// every frame).
pub fn sync(images: &mut Assets<Image>) {
    let shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    for (id, handle) in &shared.handles {
        let Some(slot) = slot(*id) else {
            continue;
        };
        let data = &shared.texels[slot];
        if data.is_empty() {
            continue;
        }
        if let Some(mut image) = images.get_mut(handle) {
            if image.data.as_deref() != Some(data.as_slice()) {
                image.data = Some(data.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with(density: f32, velocity: Vec2) -> FluidGrid {
        let mut g = FluidGrid::default();
        g.density.fill(density);
        g.velocity.fill(velocity);
        g
    }

    #[test]
    fn the_density_map_is_an_l8_ramp_from_4_to_7() {
        let at = |rho: f32| density_texture(&grid_with(rho, Vec2::ZERO))[0];
        assert_eq!(at(4.0), 0);
        assert_eq!(at(3.0), 0, "below 4 is black");
        assert_eq!(at(7.0), 255);
        assert_eq!(at(9.0), 255);
        assert_eq!(at(f32::INFINITY), 255, "the first step reads infinity");
        // (5.5 - 4) / (3 + 2^-16) * 255 = 127.499... -> 127.
        assert_eq!(at(5.5), 127);
        // Uniform water (5 particles per unit square, rho ~ 6.15) is light grey.
        assert!((150..=190).contains(&at(6.15)), "{}", at(6.15));
    }

    #[test]
    fn the_maps_show_the_interior_with_the_water_seen_from_above() {
        let mut g = FluidGrid::default();
        // A dense cell in the interior's top-left corner (grid cell (1, 13))
        // and one in its bottom-right corner (grid cell (18, 1)).
        g.density[13 * GRID_W + 1] = 7.0;
        g.density[GRID_W + 18] = 7.0;
        // The border ring (row 14, column 0) never shows.
        g.density[14 * GRID_W + 5] = 7.0;
        g.density[3 * GRID_W] = 7.0;
        let d = density_texture(&g);
        assert_eq!((TEX_W, TEX_H), (18, 13));
        assert_eq!(d.len(), 18 * 13);
        assert_eq!(d[0], 255, "top-left texel = grid cell (1, 13)");
        assert_eq!(d[12 * TEX_W + 17], 255, "bottom-right texel = grid cell (18, 1)");
        assert_eq!(d.iter().filter(|&&v| v == 255).count(), 2, "the ring is not drawn");
    }

    #[test]
    fn the_velocity_map_encodes_minus_one_to_one() {
        let mut g = grid_with(0.0, Vec2::new(1.0, -1.0));
        g.velocity[7 * GRID_W + 5] = Vec2::new(0.0, 0.5);
        let v = velocity_texture(&g);
        // x = 1 -> 255, y = -1 -> 0, z = 0 -> 127.5 rounds to even = 128, alpha 255.
        assert_eq!(v[0], 0xFF_FF_00_80);
        // Grid cell (5, 7) is texel (4, row 13 - 7 = 6): x = 0 -> 128, y = 0.5 -> 191.25 -> 191.
        assert_eq!(v[6 * TEX_W + 4], 0xFF_80_BF_80);
        // Beyond +-1 the bytes clamp.
        assert_eq!(velocity_byte(3.0), 255);
        assert_eq!(velocity_byte(-3.0), 0);
    }

    #[test]
    fn a_flat_density_map_has_flat_normals_and_a_ramp_tilts_them() {
        // The bit-trick root of entry 0 is 1.2 % short (0.61757 for 127.5 / 204),
        // so the z byte of a flat texel is round(204 * 0.61757) + 128 = 254.
        let flat = normal_texture(&vec![90u8; TEX_W * TEX_H]);
        assert!(flat.iter().all(|&c| c == 0xFF_80_80_FE), "{:08x}", flat[0]);
        // Density rising to the right: the normal leans left (red below 128),
        // the same for every row; green stays at 128 away from the wrap seam.
        let mut ramp = vec![0u8; TEX_W * TEX_H];
        for y in 0..TEX_H {
            for x in 0..TEX_W {
                ramp[y * TEX_W + x] = (x * 10) as u8;
            }
        }
        let n = normal_texture(&ramp);
        let c = n[5 * TEX_W + 8];
        let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
        assert!(r < 128 && g == 128 && b > 128, "{c:08x}");
        // m = (tl + bl) - (tr + br) = 140 - 180 = -40 and p = 0: the length is
        // looked up by (m^2 + p^2) >> 6 = 25.
        let t = normal_scale(25);
        assert_eq!(r as i32, (-40.0 * t).round_ties_even() as i32 + 128);
        // Rising downwards (memory rows): p = (tl + tr) - (bl + br) < 0, the
        // normal leans up in memory order (green below 128).
        let mut down = vec![0u8; TEX_W * TEX_H];
        for y in 0..TEX_H {
            for x in 0..TEX_W {
                down[y * TEX_W + x] = (y * 10) as u8;
            }
        }
        let c = normal_texture(&down)[5 * TEX_W + 8];
        assert!(((c >> 8) as u8) < 128 && (c >> 16) as u8 == 128, "{c:08x}");
    }

    #[test]
    fn the_normal_length_table_matches_the_code() {
        // Entry 0 is 127.5 / 204 = 0.625 up to the bit-trick root's error after
        // one Newton step (the constant 0x5F400000 leaves it 1.2 % short).
        let t = normal_scale(0);
        assert!((t - 0.617_574).abs() < 1e-4 && (t - 127.5 / 204.0).abs() < 0.01, "{t}");
        assert_eq!(round_even(t * NORMAL_Z), 126);
        // Larger gradients shorten the scale.
        assert!(normal_scale(1000) < normal_scale(10));
    }

    #[test]
    fn the_three_maps_are_rgba_images_of_18_by_13() {
        let [d, n, v] = maps(&grid_with(7.0, Vec2::ZERO));
        assert_eq!((d.len(), n.len(), v.len()), (18 * 13 * 4, 18 * 13 * 4, 18 * 13 * 4));
        assert_eq!(&d[..4], &[255, 255, 255, 255]);
        assert_eq!(&v[..4], &[128, 128, 128, 255]);
        assert!(image_for(NORMAL_ID, true, false).is_some());
        assert!(image_for(0x1234_5678, true, false).is_none());
    }
}
