//! Native raster/RW4 payloads, including all mip levels and cube faces.
//! Layout cross-checked with SporeModder-FX RasterTexture/RWRaster. This reader
//! does not decode DXT, change channel order, drop mips, or choose a first raster.
use crate::native_packet::{surface_layout, Mip, Texture};
type Result<T> = std::result::Result<T, String>;
fn u32le(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or("truncated native texture")?
            .try_into()
            .unwrap(),
    ))
}
fn u16le(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(at..at + 2)
            .ok_or("truncated native raster")?
            .try_into()
            .unwrap(),
    ))
}
fn levels(width: u32, height: u32, n: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || n == 0
        || n > 32 - width.max(height).leading_zeros()
    {
        return Err("invalid native mip dimensions/count".into());
    }
    Ok(())
}
pub fn raster(bytes: &[u8]) -> Result<Texture> {
    if u32le(bytes, 0)? != 1 {
        return Err("unsupported raster version".into());
    }
    let width = u32le(bytes, 4)?;
    let height = u32le(bytes, 8)?;
    let n = u32le(bytes, 12)?;
    levels(width, height, n)?;
    if u32le(bytes, 16)? != 8 {
        return Err("unsupported native raster pixel-width field".into());
    }
    let format = u32le(bytes, 20)? as i32;
    let mut at = 24;
    let mut mips = vec![];
    for level in 0..n {
        let size = u32le(bytes, at)? as usize;
        at += 4;
        let (row, rows) =
            surface_layout(format, (width >> level).max(1), (height >> level).max(1))?;
        if size != row * rows {
            return Err(format!(
                "native raster mip {level} byte size differs from format"
            ));
        }
        let end = at.checked_add(size).ok_or("native mip range overflow")?;
        mips.push(Mip {
            row_pitch: row,
            bytes: bytes.get(at..end).ok_or("truncated native mip")?.to_vec(),
        });
        at = end;
    }
    if at != bytes.len() {
        return Err("unparsed native raster trailing bytes".into());
    }
    Ok(Texture {
        width,
        height,
        format,
        faces: vec![mips],
    })
}
pub fn rw4(bytes: &[u8], raster_section: Option<usize>) -> Result<Texture> {
    if bytes.get(..4) != Some(&[0x89, b'R', b'W', b'4']) {
        return Err("not an RW4 container".into());
    }
    let n = u32le(bytes, 36)? as usize;
    let table = u32le(bytes, 48)? as usize;
    let base = u32le(bytes, 68)? as usize;
    if n > 4096 || table.checked_add(n * 24).is_none_or(|v| v > bytes.len()) {
        return Err("invalid native section table".into());
    }
    let candidates = (0..n)
        .filter(|i| u32le(bytes, table + i * 24 + 20).ok() == Some(0x20003))
        .collect::<Vec<_>>();
    let index = if let Some(index) = raster_section {
        if !candidates.contains(&index) {
            return Err("requested section is not a raster".into());
        }
        index
    } else {
        if candidates.len() != 1 {
            return Err("native raster selection is missing or ambiguous".into());
        }
        candidates[0]
    };
    let section = table + index * 24;
    let at = u32le(bytes, section)? as usize;
    if u32le(bytes, section + 8)? < 32 {
        return Err("short native raster section".into());
    }
    let raster = bytes
        .get(at..at.checked_add(32).ok_or("raster offset overflow")?)
        .ok_or("truncated native raster")?;
    let format = u32le(raster, 0)? as i32;
    let flags = u16le(raster, 4)?;
    if u16le(raster, 6)? > 1 {
        return Err("native volume textures not implemented".into());
    }
    let width = u16le(raster, 12)? as u32;
    let height = u16le(raster, 14)? as u32;
    let count = raster[17] as u32;
    levels(width, height, count)?;
    let reference = u32le(raster, 28)?;
    if reference >> 22 != 0 || reference as usize >= n {
        return Err("unsupported native raster resource reference".into());
    }
    let raw = table + reference as usize * 24;
    if u32le(bytes, raw + 20)? != 0x10030 {
        return Err("native raster references non-buffer section".into());
    }
    let start = base
        .checked_add(u32le(bytes, raw)? as usize)
        .ok_or("texture buffer overflow")?;
    let size = u32le(bytes, raw + 8)? as usize;
    let data = bytes
        .get(start..start.checked_add(size).ok_or("texture buffer overflow")?)
        .ok_or("truncated texture buffer")?;
    let faces = if flags & 0x1000 != 0 {
        if width != height {
            return Err("nonsquare native cube".into());
        }
        6
    } else {
        1
    };
    let mut result = vec![];
    let mut at = 0usize;
    for _ in 0..faces {
        let mut mips = vec![];
        for level in 0..count {
            let (row, rows) =
                surface_layout(format, (width >> level).max(1), (height >> level).max(1))?;
            let end = at.checked_add(row * rows).ok_or("mip offset overflow")?;
            mips.push(Mip {
                row_pitch: row,
                bytes: data
                    .get(at..end)
                    .ok_or("truncated native face/mip")?
                    .to_vec(),
            });
            at = end;
        }
        result.push(mips);
    }
    if at != data.len() {
        return Err("unparsed native texture buffer bytes".into());
    }
    Ok(Texture {
        width,
        height,
        format,
        faces: result,
    })
}
pub fn decode(bytes: &[u8], raster_section: Option<usize>) -> Result<Texture> {
    if bytes.starts_with(&[0x89, b'R', b'W', b'4']) {
        rw4(bytes, raster_section)
    } else {
        if raster_section.is_some() {
            return Err("raster record has no section index".into());
        }
        raster(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_mips_are_preserved_and_truncations_fail() {
        let mut bytes = vec![];
        for v in [1u32, 5, 3, 3, 8, 0x31545844] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for (size, value) in [(16u32, 1u8), (8, 2), (8, 3)] {
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend(vec![value; size as usize]);
        }
        let tex = raster(&bytes).unwrap();
        assert_eq!(tex.faces[0].len(), 3);
        assert_eq!(tex.faces[0][2].bytes, vec![3; 8]);
        for size in 0..bytes.len() {
            assert!(raster(&bytes[..size]).is_err());
        }
        bytes.push(0);
        assert!(raster(&bytes).is_err());
    }
    #[test]
    fn cube_face_mip_order_is_lossless() {
        let mut bytes = vec![0; 256];
        bytes[..4].copy_from_slice(&[0x89, b'R', b'W', b'4']);
        for (at, v) in [
            (36, 2u32),
            (48, 96),
            (68, 192),
            (96, 144),
            (104, 32),
            (116, 0x20003),
            (120, 0),
            (128, 96),
            (140, 0x10030),
            (144, 0x31545844),
            (172, 1),
        ] {
            bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        bytes[148..150].copy_from_slice(&0x1000u16.to_le_bytes());
        bytes[156..158].copy_from_slice(&4u16.to_le_bytes());
        bytes[158..160].copy_from_slice(&4u16.to_le_bytes());
        bytes[161] = 2;
        bytes.resize(288, 0);
        for i in 0..12 {
            bytes[192 + i * 8..200 + i * 8].fill(i as u8);
        }
        let tex = rw4(&bytes, None).unwrap();
        assert_eq!(tex.faces.len(), 6);
        assert_eq!(tex.faces[5][1].bytes, vec![11; 8]);
        bytes[150..152].copy_from_slice(&2u16.to_le_bytes());
        assert!(rw4(&bytes, None).is_err());
    }
}
