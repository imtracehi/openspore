//! Original compiled material states (RW4 section 2000b). LE payload layout
//! follows MaterialStateCompiler.decompile. Unknown flags/data remain available;
//! decoding renderer IDs is distinct from native shader execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelTransform {
    Object(u32),
    Matrix([f32; 16]),
}
#[derive(Debug, Clone, PartialEq)]
pub struct VertexDescription {
    pub header: [u8; 24],
    pub elements: Vec<[u8; 12]>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderData {
    pub index: i16,
    pub offset: u16,
    pub padding: Vec<u8>,
    pub words: Vec<u32>,
    pub object_reference: Option<u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSlot {
    pub sampler: u32,
    pub raster_reference: u32,
    pub stage_mask: u32,
    pub stage_states: Vec<(u32, u32)>,
    pub sampler_mask: u32,
    pub sampler_states: Vec<(u32, u32)>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialState {
    pub declared_size: u32,
    pub primitive: u32,
    pub flags: [u32; 3],
    pub field_14: u32,
    pub renderer_id: u32,
    pub model_transform: Option<ModelTransform>,
    pub vertex_description: Option<VertexDescription>,
    pub shader_data: Vec<ShaderData>,
    pub shader_markers: Vec<i16>,
    pub material_color: Option<[f32; 4]>,
    pub ambient_color: Option<[f32; 3]>,
    pub optional_floats: [Option<f32>; 8],
    pub booleans: Option<[u8; 17]>,
    pub unknown4: Option<u32>,
    pub unknown5: Option<[f32; 3]>,
    pub unknown6: Option<f32>,
    pub unknown7: Option<f32>,
    pub unknown_arrays: Vec<(u32, Vec<u32>)>,
    pub render_states: Vec<(u32, Vec<(u32, u32)>)>,
    pub palette_reference: u32,
    pub texture_slots: Vec<TextureSlot>,
    pub trailing: Vec<u8>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("material range overflow")?;
        let b = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("truncated material at {} need {n}", self.at))?;
        self.at = end;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() {
            return Err("nonfinite material float".into());
        }
        Ok(v)
    }
    fn array<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut a = [0.; N];
        for v in &mut a {
            *v = self.f32()?;
        }
        Ok(a)
    }
    fn pairs(&mut self) -> Result<Vec<(u32, u32)>, String> {
        let mut a = vec![];
        loop {
            let key = self.u32()?;
            if key == u32::MAX {
                return Ok(a);
            }
            if a.len() >= 4096 {
                return Err("material state list exceeds limit".into());
            }
            a.push((key, self.u32()?));
        }
    }
}
pub fn parse(bytes: &[u8]) -> Result<MaterialState, String> {
    let mut r = Reader { bytes, at: 0 };
    let declared_size = r.u32()?;
    let primitive = r.u32()?;
    let flags = [r.u32()?, r.u32()?, r.u32()?];
    let field_14 = r.u32()?;
    let renderer_id = r.u32()?;
    r.take(4)?;
    let model_transform = if flags[0] & 1 == 0 {
        None
    } else if flags[0] & 2 != 0 {
        Some(ModelTransform::Object(r.u32()?))
    } else {
        Some(ModelTransform::Matrix(r.array()?))
    };
    let vertex_description = if flags[0] & 0x100000 != 0 {
        let header: [u8; 24] = r.take(24)?.try_into().unwrap();
        let n = i16::from_le_bytes([header[12], header[13]]);
        if !(0..=128).contains(&n) {
            return Err("invalid material vertex declaration count".into());
        }
        let mut elements = Vec::with_capacity(n as usize);
        for _ in 0..n {
            elements.push(r.take(12)?.try_into().unwrap());
        }
        Some(VertexDescription { header, elements })
    } else {
        None
    };
    let mut shader_data = vec![];
    let mut shader_markers = vec![];
    if flags[0] & 8 != 0 {
        loop {
            let index = r.i16()?;
            if index == 0 {
                break;
            }
            if shader_data.len() + shader_markers.len() >= 4096 {
                return Err("material uniform count exceeds limit".into());
            }
            if index < 0 {
                shader_markers.push(index);
                continue;
            }
            let offset = r.i16()? as u16;
            let n = r.u32()? as usize;
            if n % 4 != 0 || n > 16 * 1024 * 1024 {
                return Err("invalid material uniform byte count".into());
            }
            let padding = r.take(offset as usize)?.to_vec();
            let words = r
                .take(n)?
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let object_reference = if n == 0 { Some(r.u32()?) } else { None };
            shader_data.push(ShaderData {
                index,
                offset,
                padding,
                words,
                object_reference,
            });
        }
        r.take(6)?;
    }
    let material_color = if flags[0] & 0x10 != 0 {
        Some(r.array()?)
    } else {
        None
    };
    let ambient_color = if flags[0] & 0x20 != 0 {
        Some(r.array()?)
    } else {
        None
    };
    let mut optional_floats = [None; 8];
    for (i, v) in optional_floats.iter_mut().enumerate() {
        if flags[0] & (1 << (6 + i)) != 0 {
            *v = Some(r.f32()?);
        }
    }
    let booleans = if flags[0] & 0x8000 != 0 {
        Some(r.take(17)?.try_into().unwrap())
    } else {
        None
    };
    let unknown4 = if flags[0] & 0x10000 != 0 {
        Some(r.u32()?)
    } else {
        None
    };
    let unknown5 = if flags[0] & 0x20000 != 0 {
        Some(r.array()?)
    } else {
        None
    };
    let unknown6 = if flags[0] & 0x40000 != 0 {
        Some(r.f32()?)
    } else {
        None
    };
    let unknown7 = if flags[0] & 0x80000 != 0 {
        Some(r.f32()?)
    } else {
        None
    };
    let mut unknown_arrays = vec![];
    for (bit, n) in [(0x20000, 7), (0x40000, 11), (0x80000, 11)] {
        if field_14 & bit != 0 {
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u32()?);
            }
            unknown_arrays.push((bit, a));
        }
    }
    let mut render_states = vec![];
    if flags[2] & 0x20000 != 0 {
        loop {
            let group = r.u32()?;
            if group == u32::MAX {
                break;
            }
            if render_states.len() >= 4096 {
                return Err("material render group count exceeds limit".into());
            }
            render_states.push((group, r.pairs()?));
        }
    }
    let palette_reference = r.u32()?;
    let mut texture_slots = vec![];
    if flags[2] & 0x1FFFF != 0 {
        loop {
            let sampler = r.u32()?;
            if sampler == u32::MAX {
                break;
            }
            if texture_slots.len() >= 4096 {
                return Err("material texture slot count exceeds limit".into());
            }
            let raster_reference = r.u32()?;
            let stage_mask = r.u32()?;
            let stage_states = if stage_mask != 0 { r.pairs()? } else { vec![] };
            let sampler_mask = r.u32()?;
            let sampler_states = if sampler_mask != 0 {
                r.pairs()?
            } else {
                vec![]
            };
            texture_slots.push(TextureSlot {
                sampler,
                raster_reference,
                stage_mask,
                stage_states,
                sampler_mask,
                sampler_states,
            });
        }
    }
    Ok(MaterialState {
        declared_size,
        primitive,
        flags,
        field_14,
        renderer_id,
        model_transform,
        vertex_description,
        shader_data,
        shader_markers,
        material_color,
        ambient_color,
        optional_floats,
        booleans,
        unknown4,
        unknown5,
        unknown6,
        unknown7,
        unknown_arrays,
        render_states,
        palette_reference,
        texture_slots,
        trailing: bytes[r.at..].to_vec(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn word(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![];
        for v in [0, 4, 8 | 0x10, 0, 0x20001, 0, 0x80000002, 0] {
            word(&mut b, v);
        }
        b.extend_from_slice(&(-1i16).to_le_bytes());
        b.extend_from_slice(&0x210i16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        word(&mut b, 0);
        b.extend_from_slice(&[7, 8]);
        word(&mut b, 0x800000);
        b.extend_from_slice(&0i16.to_le_bytes());
        b.extend_from_slice(&[0; 6]);
        for v in [1f32, 0.5, 0.25, 1.] {
            word(&mut b, v.to_bits());
        }
        for v in [
            0,
            22,
            1,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            0,
            7,
            1,
            1,
            4,
            u32::MAX,
            1,
            5,
            2,
            u32::MAX,
            u32::MAX,
        ] {
            word(&mut b, v);
        }
        b
    }
    #[test]
    fn native_zero_length_uniform_object_refs_markers_and_slot_states_survive() {
        let p = parse(&fixture()).unwrap();
        assert_eq!(p.renderer_id, 0x80000002);
        assert_eq!(p.shader_data[0].padding, [7, 8]);
        assert_eq!(p.shader_data[0].object_reference, Some(0x800000));
        assert_eq!(p.shader_markers, [-1]);
        assert_eq!(p.material_color, Some([1., 0.5, 0.25, 1.]));
        assert_eq!(p.texture_slots[0].stage_states, [(1, 4)]);
        assert_eq!(p.texture_slots[0].sampler_states, [(5, 2)]);
        assert!(p.trailing.is_empty());
    }
    #[test]
    fn every_prefix_truncation_and_nonfinite_color_fails() {
        let b = fixture();
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err(), "accepted prefix {n}");
        }
        let mut b = b;
        b[56..60].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse(&b).is_err());
    }
    #[test]
    fn declared_size_is_not_the_section_boundary_and_optional_matrix_is_retained() {
        let mut b = vec![];
        for v in [1, 4, 1 | 0x20, 0, 0, 0, 2, 0] {
            word(&mut b, v);
        }
        for i in 0..16 {
            word(&mut b, (i as f32).to_bits());
        }
        for v in [0.1f32, 0.2, 0.3] {
            word(&mut b, v.to_bits());
        }
        word(&mut b, u32::MAX);
        let p = parse(&b).unwrap();
        assert_eq!(p.declared_size, 1);
        assert!(matches!(p.model_transform, Some(ModelTransform::Matrix(_))));
        assert_eq!(p.ambient_color, Some([0.1, 0.2, 0.3]));
    }
}
