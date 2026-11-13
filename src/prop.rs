//! Read-only property lists. Headers/numbers use big endian; keys, vectors,
//! and localized text use little endian. Scalar keys/vectors have 16-byte
//! storage; array entries do not. Format reference: SporeModder-FX's PROP
//! reader (https://github.com/Spore-Community/SporeModder-FX/tree/master/src/sporemodder/file/prop).
use crate::dbpf::ResourceKey;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct PropertyList(pub BTreeMap<u32, Property>);

#[derive(Debug, Clone)]
pub struct Property {
    pub kind: u16,
    pub array: bool,
    pub values: Vec<Vec<u8>>,
}

impl Property {
    pub fn number(&self) -> Option<f32> {
        let v = self.values.first()?;
        Some(match self.kind {
            1 | 5 | 6 => *v.first()? as f32,
            7 => i16::from_be_bytes(v.get(..2)?.try_into().ok()?) as f32,
            8 => u16::from_be_bytes(v.get(..2)?.try_into().ok()?) as f32,
            9 => i32::from_be_bytes(v.get(..4)?.try_into().ok()?) as f32,
            10 => u32::from_be_bytes(v.get(..4)?.try_into().ok()?) as f32,
            13 => f32::from_be_bytes(v.get(..4)?.try_into().ok()?),
            _ => return None,
        })
    }
    pub fn key(&self) -> Option<ResourceKey> {
        if self.kind != 0x20 {
            return None;
        }
        let v = self.values.first()?;
        Some(ResourceKey {
            instance_id: u32::from_le_bytes(v.get(..4)?.try_into().ok()?),
            type_id: u32::from_le_bytes(v.get(4..8)?.try_into().ok()?),
            group_id: u32::from_le_bytes(v.get(8..12)?.try_into().ok()?),
        })
    }
    pub fn vector3(&self) -> Option<[f32; 3]> {
        if self.kind != 0x31 {
            return None;
        }
        let v = self.values.first()?;
        v.get(..12)?;
        Some([0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
    }
    pub fn text(&self) -> Option<(u32, u32, String)> {
        let v = self.values.first()?;
        match self.kind {
            0x12 => Some((0, 0, String::from_utf8_lossy(v).into_owned())),
            0x13 => Some((0, 0, utf16(v))),
            0x22 => Some((
                u32::from_le_bytes(v.get(..4)?.try_into().ok()?),
                u32::from_le_bytes(v.get(4..8)?.try_into().ok()?),
                utf16(v.get(8..)?),
            )),
            _ => None,
        }
    }
    pub fn color_rgb(&self) -> Option<[f32; 3]> {
        if self.kind != 0x32 {
            return None;
        }
        let v = self.values.first()?.get(..12)?;
        Some([0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
    }
    pub fn vector4(&self) -> Option<[f32; 4]> {
        if self.kind != 0x33 && self.kind != 0x34 {
            return None;
        }
        let v = self.values.first()?.get(..16)?;
        Some([0, 4, 8, 12].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
    }
}
fn utf16(v: &[u8]) -> String {
    let chars: Vec<_> = v
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&chars)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("property size overflow")?;
        let out = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| format!("truncated property at {} (need {n})", self.pos))?;
        self.pos = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
}

pub fn parse(bytes: &[u8]) -> Result<PropertyList, String> {
    let mut r = Reader { bytes, pos: 0 };
    let count = r.u32()? as usize;
    if count > 65536 {
        return Err("excessive property count".into());
    }
    let mut list = PropertyList::default();
    for _ in 0..count {
        let id = r.u32()?;
        let kind = r.u16()?;
        let flags = r.u16()?;
        let array = flags & 0x30 != 0;
        if array && flags & 0x40 != 0 {
            return Err("unsupported property flags".into());
        }
        let (items, stride) = if array {
            (r.u32()? as usize, r.u32()? as usize)
        } else {
            (1, 0)
        };
        if items > 65536 || stride > 1_048_576 {
            return Err("excessive property array".into());
        }
        let fixed = match kind {
            0 => 16,
            1 | 2 | 5 | 6 => 1,
            3 | 7 | 8 => 2,
            9 | 10 | 13 => 4,
            11 | 12 | 14 => 8,
            0x12 | 0x13 => 0,
            0x20 => {
                if array {
                    12
                } else {
                    16
                }
            }
            0x22 => 520,
            0x30 => {
                if array {
                    8
                } else {
                    16
                }
            }
            0x31 | 0x32 => {
                if array {
                    12
                } else {
                    16
                }
            }
            0x33 | 0x34 => 16,
            0x38 => 56,
            0x39 => 24,
            _ => return Err(format!("unknown property type {kind:#x} for {id:#x}")),
        };
        let mut values = Vec::new();
        for _ in 0..items {
            if kind == 0x12 || kind == 0x13 {
                let chars = r.u32()? as usize;
                let size = chars
                    .checked_mul(if kind == 0x13 { 2 } else { 1 })
                    .ok_or("string size overflow")?;
                values.push(r.take(size)?.to_vec());
            } else {
                // Reject stride mismatches instead of losing property boundaries.
                if array && stride != fixed {
                    return Err(format!(
                        "invalid property stride {stride} for type {kind:#x}"
                    ));
                }
                values.push(r.take(fixed)?.to_vec());
            }
        }
        if list
            .0
            .insert(
                id,
                Property {
                    kind,
                    array,
                    values,
                },
            )
            .is_some()
        {
            return Err("duplicate property id".into());
        }
    }
    if r.pos != bytes.len() {
        return Err(format!("{} trailing property bytes", bytes.len() - r.pos));
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_endian_scalars_and_arrays() {
        let mut b = 3u32.to_be_bytes().to_vec();
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&[0, 9, 0, 0]);
        b.extend_from_slice(&75i32.to_be_bytes());
        b.extend_from_slice(&2u32.to_be_bytes());
        b.extend_from_slice(&[0, 0x20, 0, 0]);
        for v in [0x12345678u32, 0x2f4e681b, 0x40606000, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&[0, 0x31, 0, 0x30]);
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&12u32.to_be_bytes());
        for v in [1f32, 2., 3.] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let p = parse(&b).unwrap();
        assert_eq!(p.0[&1].number(), Some(75.));
        assert_eq!(p.0[&2].key().unwrap().instance_id, 0x12345678);
        assert_eq!(p.0[&3].vector3(), Some([1., 2., 3.]));
        for end in 0..b.len() {
            assert!(parse(&b[..end]).is_err(), "end {end}");
        }
    }
    #[test]
    fn unknown_type_and_oversized_arrays_rejected() {
        let mut b = vec![0, 0, 0, 1, 0, 0, 0, 1, 0, 0xff, 0, 0];
        assert!(parse(&b).is_err());
        b[9] = 9;
        b[11] = 0x30;
        b.extend_from_slice(&u32::MAX.to_be_bytes());
        b.extend_from_slice(&4u32.to_be_bytes());
        assert!(parse(&b).is_err());
    }
}
