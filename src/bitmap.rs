//! Native world bitmaps: BE header, LSB-first bits, BGRA8 or LE RGB16 pixels.
//! Raw payload is retained, including full 48-bit precision. Layout reference:
//! SporeModder-FX BitmapImage.read/processImage.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub type_id: u32,
    pub width: u32,
    pub height: u32,
    pub channel_count: u32,
    pub data: Vec<u8>,
    pub unknown: Option<u32>,
    pub trailing: Vec<u8>,
}
pub fn supported(type_id: u32) -> bool {
    matches!(type_id, 0x03E421E9 | 0x03E421EC | 0x03E421ED | 0x03E421EF)
}
pub fn parse(type_id: u32, bytes: &[u8]) -> Result<Bitmap, String> {
    let header = bytes.get(..20).ok_or("truncated bitmap header")?;
    let words: Vec<_> = header
        .chunks_exact(4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .collect();
    let (version, width, height, channel_count, size) =
        (words[0], words[1], words[2], words[3], words[4]);
    if version != 0 || width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("invalid bitmap version or dimensions".into());
    }
    let count = width
        .checked_mul(height)
        .ok_or("bitmap pixel count overflow")?;
    let (channels, expected) = match type_id {
        0x03E421E9 => {
            if width % 8 != 0 {
                return Err("bit-map width must be a multiple of eight".into());
            }
            (0, count / 8)
        }
        0x03E421EC => (1, count),
        0x03E421ED => (2, count.checked_mul(4).ok_or("bitmap size overflow")?),
        0x03E421EF => (3, count.checked_mul(6).ok_or("bitmap size overflow")?),
        _ => return Err("unsupported bitmap type".into()),
    };
    if channel_count != channels || size != expected || size > 64 * 1024 * 1024 {
        return Err("bitmap channel/size mismatch or decode limit".into());
    }
    let end = 20usize
        .checked_add(size as usize)
        .ok_or("bitmap range overflow")?;
    let data = bytes
        .get(20..end)
        .ok_or("truncated bitmap pixels")?
        .to_vec();
    let (unknown, tail) = if channels == 0 {
        (
            Some(u32::from_be_bytes(
                bytes
                    .get(end..end + 4)
                    .ok_or("missing bit-map trailer")?
                    .try_into()
                    .unwrap(),
            )),
            end + 4,
        )
    } else {
        (None, end)
    };
    Ok(Bitmap {
        type_id,
        width,
        height,
        channel_count,
        data,
        unknown,
        trailing: bytes[tail..].to_vec(),
    })
}
impl Bitmap {
    pub fn rgba8(&self) -> Vec<u8> {
        let count = self.width as usize * self.height as usize;
        let mut out = Vec::with_capacity(count * 4);
        for i in 0..count {
            let pixel = match self.channel_count {
                0 => {
                    let v = if self.data[i / 8] & (1 << (i % 8)) != 0 {
                        255
                    } else {
                        0
                    };
                    [v, v, v, 255]
                }
                1 => {
                    let v = self.data[i];
                    [v, v, v, 255]
                }
                2 => {
                    let p = &self.data[i * 4..i * 4 + 4];
                    [p[2], p[1], p[0], p[3]]
                }
                3 => {
                    let p = &self.data[i * 6..i * 6 + 6];
                    let c = |j| {
                        ((u16::from_le_bytes([p[j], p[j + 1]]) as u32 * 255 + 32767) / 65535) as u8
                    };
                    [c(0), c(2), c(4), 255]
                }
                _ => unreachable!("parser validates native bitmap channels"),
            };
            out.extend_from_slice(&pixel);
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn header(width: u32, height: u32, channels: u32, size: u32) -> Vec<u8> {
        [0, width, height, channels, size]
            .into_iter()
            .flat_map(|v| v.to_be_bytes())
            .collect()
    }
    #[test]
    fn bgra_rgb16_and_bit_order_keep_native_values() {
        let mut b = header(2, 1, 2, 8);
        b.extend_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
        assert_eq!(
            parse(0x03E421ED, &b).unwrap().rgba8(),
            [30, 20, 10, 40, 70, 60, 50, 80]
        );
        let mut b = header(2, 1, 3, 12);
        for v in [0u16, 32768, 65535, 65535, 257, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let p = parse(0x03E421EF, &b).unwrap();
        assert_eq!(p.rgba8(), [0, 128, 255, 255, 255, 1, 0, 255]);
        assert_eq!(p.data.len(), 12);
        let mut b = header(8, 1, 0, 1);
        b.push(0b00000101);
        b.extend_from_slice(&123u32.to_be_bytes());
        let p = parse(0x03E421E9, &b).unwrap();
        assert_eq!(p.unknown, Some(123));
        assert_eq!(
            &p.rgba8()[..12],
            &[255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255]
        );
    }
    #[test]
    fn every_truncation_dimension_mismatch_and_type_mismatch_fails() {
        let mut b = header(8, 1, 0, 1);
        b.push(0);
        b.extend_from_slice(&1u32.to_be_bytes());
        for n in 0..b.len() {
            assert!(parse(0x03E421E9, &b[..n]).is_err());
        }
        assert!(parse(0x03E421EC, &b).is_err());
        assert!(parse(0x03E421F0, &b).is_err());
        assert!(parse(0x03E421EC, &header(0, 1, 1, 0)).is_err());
        assert!(parse(0x03E421E9, &header(9, 1, 0, 1)).is_err());
        assert!(parse(0x03E421EF, &header(8192, 8192, 3, 402653184)).is_err());
    }
    #[test]
    fn grayscale_and_trailing_metadata_survive() {
        let mut b = header(2, 1, 1, 2);
        b.extend_from_slice(&[15, 200, 7, 8]);
        let p = parse(0x03E421EC, &b).unwrap();
        assert_eq!(p.rgba8(), [15, 15, 15, 255, 200, 200, 200, 255]);
        assert_eq!(p.trailing, [7, 8]);
    }
}
