//! RW4 texture sections and BC1/BC2/BC3 base mip decoding. Decoding to RGBA
//! also permits skinpaint's diffuse and tint channels to be composed on any GPU.
#[derive(Debug, Clone)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

pub fn decode_rw4(b: &[u8]) -> Option<Pixels> {
    if b.get(..4)? != [0x89, b'R', b'W', b'4'] || u32le(b, 28)? != 0x04000000 {
        return None;
    }
    let count = u32le(b, 36)? as usize;
    let table = u32le(b, 48)? as usize;
    let base = u32le(b, 68)? as usize;
    if count > 4096 || table.checked_add(count.checked_mul(24)?)? > b.len() {
        return None;
    }
    for i in 0..count {
        let section = table + i * 24;
        if u32le(b, section + 20)? != 0x20003 {
            continue;
        }
        let at = u32le(b, section)? as usize;
        if u32le(b, section + 8)? < 32 {
            return None;
        }
        let r = b.get(at..at.checked_add(32)?)?;
        let format = u32le(r, 0)?;
        if u16le(r, 4)? & 0x1000 != 0 || u16le(r, 6)? > 1 {
            return None;
        }
        let width = u16le(r, 12)? as u32;
        let height = u16le(r, 14)? as u32;
        let reference = u32le(r, 28)?;
        if reference >> 22 != 0 || reference as usize >= count {
            return None;
        }
        let raw = table + reference as usize * 24;
        if u32le(b, raw + 20)? != 0x10030 {
            return None;
        }
        let start = base.checked_add(u32le(b, raw)? as usize)?;
        let data = b.get(start..start.checked_add(u32le(b, raw + 8)? as usize)?)?;
        return decode_blocks(width, height, format, data);
    }
    None
}

/// Decode the exact raster referenced by a compiled material, including rasters
/// embedded in MODEL containers. No scan-based choice of the first texture.
pub fn decode_raster(blob: &[u8], raster_section: usize) -> Option<Pixels> {
    let sections = crate::rw4::sections(blob).ok()?;
    let (kind, raster) = *sections.get(raster_section)?;
    if kind != 0x20003 || raster.len() < 32 {
        return None;
    }
    if u16le(raster, 4)? & 0x1000 != 0 || u16le(raster, 6)? > 1 {
        return None;
    }
    let reference = u32le(raster, 28)?;
    if reference >> 22 != 0 {
        return None;
    }
    let (kind, data) = *sections.get(reference as usize)?;
    if kind != crate::rw4::T_RAW {
        return None;
    }
    decode_blocks(
        u16le(raster, 12)? as u32,
        u16le(raster, 14)? as u32,
        u32le(raster, 0)?,
        data,
    )
}

/// Decode every authored mip level of the exact raster section referenced by a
/// compiled material. Levels are returned from largest to smallest; the raw
/// section must contain every byte required by the authored chain.
pub fn decode_raster_mips(blob: &[u8], raster_section: usize) -> Option<Vec<Pixels>> {
    let sections = crate::rw4::sections(blob).ok()?;
    let (kind, raster) = *sections.get(raster_section)?;
    if kind != 0x20003 || raster.len() < 32 {
        return None;
    }
    let (format, width, height, levels, reference) = raster_descriptor(raster)?;
    let (kind, data) = *sections.get(reference)?;
    if kind != crate::rw4::T_RAW {
        return None;
    }
    decode_mip_chain(format, width, height, levels, data)
}

fn raster_descriptor(raster: &[u8]) -> Option<(u32, u32, u32, usize, usize)> {
    if raster.len() < 32 || u16le(raster, 4)? & 0x1000 != 0 || u16le(raster, 6)? > 1 {
        return None;
    }
    let reference = u32le(raster, 28)?;
    if reference >> 22 != 0 {
        return None;
    }
    Some((
        u32le(raster, 0)?,
        u16le(raster, 12)? as u32,
        u16le(raster, 14)? as u32,
        raster[17] as usize,
        reference as usize,
    ))
}

fn mip_level_size(format: u32, width: u32, height: u32) -> Option<usize> {
    let (w, h) = (width as usize, height as usize);
    match format {
        20 => w.checked_mul(h)?.checked_mul(3),
        21 | 22 => w.checked_mul(h)?.checked_mul(4),
        0x31545844 => w.div_ceil(4).checked_mul(h.div_ceil(4))?.checked_mul(8),
        0x33545844 | 0x35545844 => w.div_ceil(4).checked_mul(h.div_ceil(4))?.checked_mul(16),
        _ => None,
    }
}

fn decode_mip_chain(
    format: u32,
    mut width: u32,
    mut height: u32,
    levels: usize,
    data: &[u8],
) -> Option<Vec<Pixels>> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    if levels == 0 || levels > (width.max(height).ilog2() + 1) as usize {
        return None;
    }
    let mut offset = 0usize;
    let mut result = Vec::with_capacity(levels);
    for _ in 0..levels {
        let size = mip_level_size(format, width, height)?;
        let end = offset.checked_add(size)?;
        result.push(decode_blocks(
            width,
            height,
            format,
            data.get(offset..end)?,
        )?);
        offset = end;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Some(result)
}
fn rgb565(v: u16) -> [u8; 3] {
    let r = (v >> 11) & 31;
    let g = (v >> 5) & 63;
    let b = v & 31;
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}

/// All authored mip levels of a standalone RW4 raster, in native order.
pub fn decode_rw4_mips(blob: &[u8]) -> Option<Vec<Pixels>> {
    let sections = crate::rw4::sections(blob).ok()?;
    let (_, r) = sections.iter().find(|(kind, _)| *kind == 0x20003)?;
    let (format, width, height, levels, reference) = raster_descriptor(r)?;
    let (kind, data) = sections.get(reference)?;
    if *kind != crate::rw4::T_RAW {
        return None;
    }
    decode_mip_chain(format, width, height, levels, data)
}
pub fn decode_blocks(width: u32, height: u32, format: u32, data: &[u8]) -> Option<Pixels> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let mut rgba = vec![0; width as usize * height as usize * 4];
    if matches!(format, 20 | 21 | 22) {
        // D3DFMT_R8G8B8 is 24-bit; X8R8G8B8 still has a fourth byte.
        let stride = if format == 20 { 3 } else { 4 };
        let src = data.get(..width as usize * height as usize * stride)?;
        for (out, p) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(stride)) {
            out.copy_from_slice(&[p[2], p[1], p[0], if format == 21 { p[3] } else { 255 }]);
        }
        return Some(Pixels {
            width,
            height,
            rgba,
        });
    }
    let bc1 = format == u32::from_le_bytes(*b"DXT1");
    let bc2 = format == u32::from_le_bytes(*b"DXT3");
    let bc3 = format == u32::from_le_bytes(*b"DXT5");
    if !bc1 && !bc2 && !bc3 {
        return None;
    }
    let stride = if bc1 { 8 } else { 16 };
    let nx = width.div_ceil(4) as usize;
    let ny = height.div_ceil(4) as usize;
    let blocks = data.get(..nx * ny * stride)?;
    for by in 0..ny {
        for bx in 0..nx {
            let b = &blocks[(by * nx + bx) * stride..][..stride];
            let color = &b[if bc1 { 0 } else { 8 }..];
            let c0 = u16le(color, 0)?;
            let c1 = u16le(color, 2)?;
            let a = rgb565(c0);
            let z = rgb565(c1);
            let mut colors = [[0u8; 4]; 4];
            colors[0] = [a[0], a[1], a[2], 255];
            colors[1] = [z[0], z[1], z[2], 255];
            if c0 > c1 || !bc1 {
                for c in 0..3 {
                    colors[2][c] = ((2 * a[c] as u16 + z[c] as u16) / 3) as u8;
                    colors[3][c] = ((a[c] as u16 + 2 * z[c] as u16) / 3) as u8;
                }
                colors[2][3] = 255;
                colors[3][3] = 255;
            } else {
                for c in 0..3 {
                    colors[2][c] = ((a[c] as u16 + z[c] as u16) / 2) as u8;
                }
                colors[2][3] = 255;
            }
            let mut alphas = [0u8; 8];
            let mut alpha_bits = 0u64;
            if bc3 {
                alphas[0] = b[0];
                alphas[1] = b[1];
                if b[0] > b[1] {
                    for i in 1..7 {
                        alphas[i + 1] = (((7 - i) * b[0] as usize + i * b[1] as usize) / 7) as u8;
                    }
                } else {
                    for i in 1..5 {
                        alphas[i + 1] = (((5 - i) * b[0] as usize + i * b[1] as usize) / 5) as u8;
                    }
                    alphas[6] = 0;
                    alphas[7] = 255;
                }
                for i in 0..6 {
                    alpha_bits |= (b[2 + i] as u64) << (i * 8);
                }
            }
            let bits = u32le(color, 4)?;
            for y in 0..4 {
                for x in 0..4 {
                    let px = bx * 4 + x;
                    let py = by * 4 + y;
                    if px >= width as usize || py >= height as usize {
                        continue;
                    }
                    let n = y * 4 + x;
                    let mut pixel = colors[((bits >> (n * 2)) & 3) as usize];
                    if bc2 {
                        pixel[3] = ((b[n / 2] >> (n % 2 * 4)) & 15) * 17;
                    }
                    if bc3 {
                        pixel[3] = alphas[((alpha_bits >> (n * 3)) & 7) as usize];
                    }
                    rgba[(py * width as usize + px) * 4..][..4].copy_from_slice(&pixel);
                }
            }
        }
    }
    Some(Pixels {
        width,
        height,
        rgba,
    })
}

/// Diffuse alpha controls original texture versus paint. RGB tint weights
/// select base/coat/detail. Spec-bump is retained separately: the original
/// material's shader and slot usage must be resolved before applying it.
pub fn skinpaint(diffuse: &Pixels, tint: Option<&Pixels>, colors: [[u8; 3]; 3]) -> Pixels {
    let mut out = diffuse.clone();
    for y in 0..diffuse.height as usize {
        for x in 0..diffuse.width as usize {
            let o = (y * diffuse.width as usize + x) * 4;
            let alpha = diffuse.rgba[o + 3] as f32 / 255.;
            let mask = tint
                .map(|m| {
                    let mx = x * m.width as usize / diffuse.width as usize;
                    let my = y * m.height as usize / diffuse.height as usize;
                    let i = (my * m.width as usize + mx) * 4;
                    [m.rgba[i] as f32, m.rgba[i + 1] as f32, m.rgba[i + 2] as f32]
                })
                .unwrap_or([255., 0., 0.]);
            let sum = mask.iter().sum::<f32>();
            let weights = if sum > 0. {
                mask.map(|v| v / sum)
            } else {
                [1., 0., 0.]
            };
            for c in 0..3 {
                let paint = (0..3)
                    .map(|i| colors[i][c] as f32 * weights[i])
                    .sum::<f32>();
                out.rgba[o + c] =
                    (diffuse.rgba[o + c] as f32 * alpha + paint * (1. - alpha)).round() as u8;
            }
            out.rgba[o + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxt1_block(color: u16) -> [u8; 8] {
        let mut block = [0; 8];
        block[..2].copy_from_slice(&color.to_le_bytes());
        block[2..4].copy_from_slice(&0u16.to_le_bytes());
        block
    }

    fn raster_section(reference: u32) -> Vec<u8> {
        let mut raster = vec![0; 32];
        raster[..4].copy_from_slice(&u32::from_le_bytes(*b"DXT1").to_le_bytes());
        raster[12..14].copy_from_slice(&4u16.to_le_bytes());
        raster[14..16].copy_from_slice(&4u16.to_le_bytes());
        raster[17] = 3;
        raster[28..32].copy_from_slice(&reference.to_le_bytes());
        raster
    }

    fn raster_blob(first_raw: &[u8], second_raw: &[u8]) -> Vec<u8> {
        let parts = [
            (0x20003u32, raster_section(2)),
            (0x20003u32, raster_section(3)),
            (crate::rw4::T_RAW, first_raw.to_vec()),
            (crate::rw4::T_RAW, second_raw.to_vec()),
        ];
        let mut blob = vec![0; 96];
        blob[..28].copy_from_slice(crate::rw4::RW_MAGIC);
        blob[0x1c..0x20].copy_from_slice(&1u32.to_le_bytes());
        blob[0x24..0x28].copy_from_slice(&(parts.len() as u32).to_le_bytes());
        let table_at = 96 + 64;
        let buffer_at = table_at + parts.len() * 24;
        blob[0x30..0x34].copy_from_slice(&(table_at as u32).to_le_bytes());
        blob[0x44..0x48].copy_from_slice(&(buffer_at as u32).to_le_bytes());
        blob.extend_from_slice(&parts[0].1);
        blob.extend_from_slice(&parts[1].1);
        let mut raw_offset = 0usize;
        for (i, (kind, data)) in parts.iter().enumerate() {
            let (offset, size) = if *kind == crate::rw4::T_RAW {
                let offset = raw_offset;
                raw_offset += data.len();
                (offset, data.len())
            } else {
                (96 + i * 32, data.len())
            };
            blob.extend_from_slice(&(offset as u32).to_le_bytes());
            blob.extend_from_slice(&0u32.to_le_bytes());
            blob.extend_from_slice(&(size as u32).to_le_bytes());
            blob.extend_from_slice(&4u32.to_le_bytes());
            blob.extend_from_slice(&0u32.to_le_bytes());
            blob.extend_from_slice(&kind.to_le_bytes());
        }
        blob.extend_from_slice(first_raw);
        blob.extend_from_slice(second_raw);
        blob
    }

    #[test]
    fn embedded_raster_mips_select_the_referenced_section_and_reject_short_chains() {
        let mut red_chain = vec![];
        red_chain.extend_from_slice(&dxt1_block(0xf800));
        red_chain.extend_from_slice(&dxt1_block(0x07e0));
        red_chain.extend_from_slice(&dxt1_block(0x001f));
        let mut blue_chain = vec![];
        blue_chain.extend_from_slice(&dxt1_block(0x001f));
        blue_chain.extend_from_slice(&dxt1_block(0xf800));
        blue_chain.extend_from_slice(&dxt1_block(0x07e0));
        let blob = raster_blob(&red_chain, &blue_chain);

        let selected = decode_raster_mips(&blob, 1).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|mip| (mip.width, mip.height))
                .collect::<Vec<_>>(),
            [(4, 4), (2, 2), (1, 1)]
        );
        assert_eq!(&selected[0].rgba[..4], &[0, 0, 255, 255]);
        assert_eq!(&selected[1].rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&selected[2].rgba[..4], &[0, 255, 0, 255]);
        assert_eq!(
            &decode_raster_mips(&blob, 0).unwrap()[0].rgba[..4],
            &[255, 0, 0, 255]
        );
        assert!(decode_raster_mips(&blob, 2).is_none());

        // The legacy base-level API remains useful for partially present mip
        // chains, while the new API refuses to report an incomplete authored
        // chain as a complete set of source levels.
        let short_blob = raster_blob(&red_chain[..16], &blue_chain);
        assert!(decode_raster(&short_blob, 0).is_some());
        assert!(decode_raster_mips(&short_blob, 0).is_none());
    }

    #[test]
    fn d3d_rgb_formats_keep_their_native_pixel_stride() {
        let bytes = [1, 2, 3, 17, 4, 5, 6, 29];
        assert_eq!(
            decode_blocks(2, 1, 21, &bytes).unwrap().rgba,
            [3, 2, 1, 17, 6, 5, 4, 29]
        );
        assert_eq!(
            decode_blocks(2, 1, 22, &bytes).unwrap().rgba,
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
        assert_eq!(
            decode_blocks(2, 1, 20, &[1, 2, 3, 4, 5, 6]).unwrap().rgba,
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
        assert!(decode_blocks(2, 1, 22, &bytes[..6]).is_none());
    }
    #[test]
    fn bc1_transparency_and_odd_extent() {
        let p = decode_blocks(
            3,
            2,
            u32::from_le_bytes(*b"DXT1"),
            &[0, 0, 255, 255, 255, 255, 255, 255],
        )
        .unwrap();
        assert_eq!(p.rgba.len(), 24);
        assert!(p.rgba.chunks_exact(4).all(|p| p == [0, 0, 0, 0]));
        assert!(decode_blocks(4, 4, u32::from_le_bytes(*b"DXT5"), &[0; 15]).is_none());
    }
    #[test]
    fn bc3_alpha_endpoints() {
        let mut b = [0; 16];
        b[0] = 0;
        b[1] = 100;
        b[2] = 6 | (7 << 3);
        let p = decode_blocks(4, 4, u32::from_le_bytes(*b"DXT5"), &b).unwrap();
        assert_eq!((p.rgba[3], p.rgba[7]), (0, 255));
    }
    #[test]
    fn paint_preserves_diffuse_and_uses_tint() {
        let d = Pixels {
            width: 2,
            height: 1,
            rgba: vec![11, 22, 33, 255, 0, 0, 0, 0],
        };
        let t = Pixels {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 255, 255, 0, 255, 0, 255],
        };
        let p = skinpaint(&d, Some(&t), [[10, 20, 30], [40, 50, 60], [70, 80, 90]]);
        assert_eq!(p.rgba, vec![11, 22, 33, 255, 40, 50, 60, 255]);
    }
}
