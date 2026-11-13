//! Bounded execution of the arithmetic used by the installed SkinPaint pixel
//! programs. This is not a general D3D9 renderer or a native vertex pipeline.
use crate::d3d9_shader::{self as dx, Program, Stage};
type V = [f32; 4];
#[derive(Debug, Clone, Copy)]
struct Register {
    bank: u32,
    index: usize,
}
#[derive(Debug, Clone, Copy)]
struct Source {
    register: Register,
    swizzle: [usize; 4],
    modifier: u32,
}
#[derive(Debug, Clone, Copy)]
struct Destination {
    register: Register,
    mask: u32,
    saturate: bool,
}
#[derive(Debug, Clone)]
enum Op {
    Def(usize, V),
    Arithmetic(u16, Destination, Vec<Source>),
    Texture(Destination, Source, usize),
}
#[derive(Debug, Clone)]
pub struct PixelKernel {
    ops: Vec<Op>,
    pub version: u32,
    pub partial_precision: bool,
}
fn register(token: u32) -> Result<Register, String> {
    if token & 0x80000000 == 0 || token & 0x2000 != 0 {
        return Err("invalid or relative shader register".into());
    }
    let r = Register {
        bank: dx::register_type(token),
        index: dx::register_number(token) as usize,
    };
    let limit = match r.bank {
        0 => 32,
        1 => 16,
        2 => 224,
        3 => 8,
        8 => 1,
        10 => 16,
        _ => return Err("unsupported shader register bank".into()),
    };
    if r.index >= limit {
        return Err("shader register exceeds bank limit".into());
    }
    Ok(r)
}
fn source(token: u32) -> Result<Source, String> {
    let r = register(token)?;
    if !matches!(r.bank, 0 | 1 | 2 | 3) {
        return Err("unsupported source bank".into());
    }
    let modifier = (token >> 24) & 15;
    if !matches!(modifier, 0 | 1 | 11 | 12) {
        return Err("unsupported shader source modifier".into());
    }
    Ok(Source {
        register: r,
        swizzle: dx::source_swizzle(token),
        modifier,
    })
}
fn destination(token: u32) -> Result<Destination, String> {
    let r = register(token)?;
    if !matches!(r.bank, 0 | 8) || (token >> 24) & 15 != 0 || (token >> 20) & 15 > 3 {
        return Err("unsupported shader destination".into());
    }
    let mask = dx::destination_mask(token);
    if mask == 0 {
        return Err("empty shader destination mask".into());
    }
    Ok(Destination {
        register: r,
        mask,
        saturate: token & 0x100000 != 0,
    })
}
impl PixelKernel {
    pub fn compile(program: &Program) -> Result<Self, String> {
        if program.stage != Stage::Pixel || program.instructions.len() > 4096 {
            return Err("expected bounded pixel shader".into());
        }
        let mut ops = vec![];
        let mut partial_precision = false;
        let mut output = false;
        for instruction in &program.instructions {
            if instruction.token & 0xf0ff0000 != 0 {
                return Err("unsupported shader instruction controls".into());
            }
            let p = &instruction.parameters;
            let count = match instruction.opcode {
                1 | 6 | 7 => 2,
                2 | 5 | 8 | 11 | 66 => 3,
                4 | 18 => 4,
                31 => 2,
                81 => 5,
                _ => return Err(format!("unsupported pixel opcode {}", instruction.opcode)),
            };
            if p.len() != count {
                return Err("invalid pixel instruction arity".into());
            }
            match instruction.opcode {
                31 => {
                    let r = register(p[1])?;
                    if !matches!(r.bank, 1 | 3 | 10) || p[0] & 0x80000000 == 0 {
                        return Err("unsupported pixel declaration".into());
                    }
                    if r.bank == 10 && p[0] & 0x78000000 != 0x10000000 {
                        return Err("only 2D texture samplers are supported".into());
                    }
                }
                81 => {
                    let r = register(p[0])?;
                    if r.bank != 2 || p[0] & 0x0fff0000 != 0x000f0000 {
                        return Err("invalid pixel constant definition".into());
                    }
                    let v = std::array::from_fn(|i| f32::from_bits(p[i + 1]));
                    if !v.iter().all(|n| n.is_finite()) {
                        return Err("nonfinite pixel constant".into());
                    }
                    ops.push(Op::Def(r.index, v));
                }
                opcode => {
                    let dst = destination(p[0])?;
                    partial_precision |= p[0] & 0x200000 != 0;
                    output |= dst.register.bank == 8;
                    if opcode == 66 {
                        let sampler = register(p[2])?;
                        if sampler.bank != 10 || p[2] & 0x0f000000 != 0 {
                            return Err("invalid texture sampler operand".into());
                        }
                        ops.push(Op::Texture(dst, source(p[1])?, sampler.index));
                    } else {
                        let sources = p[1..]
                            .iter()
                            .map(|p| source(*p))
                            .collect::<Result<Vec<_>, _>>()?;
                        ops.push(Op::Arithmetic(opcode, dst, sources));
                    }
                }
            }
        }
        if !output {
            return Err("pixel program never writes oC0".into());
        }
        Ok(Self {
            ops,
            version: program.version,
            partial_precision,
        })
    }
    pub fn run(
        &self,
        inputs: &[V],
        texture_coordinates: &[V],
        constants: &[V],
        mut sample: impl FnMut(usize, [f32; 2]) -> V,
    ) -> Result<V, String> {
        if inputs.len() > 16
            || texture_coordinates.len() > 8
            || constants.len() > 224
            || !inputs
                .iter()
                .chain(texture_coordinates)
                .chain(constants)
                .flatten()
                .all(|n| n.is_finite())
        {
            return Err("invalid pixel inputs/constants".into());
        }
        let mut temporary = [[0.; 4]; 32];
        let mut c = [[0.; 4]; 224];
        c[..constants.len()].copy_from_slice(constants);
        let mut output = [0.; 4];
        fn read(
            s: Source,
            t: &[V; 32],
            c: &[V; 224],
            inputs: &[V],
            coordinates: &[V],
        ) -> Result<V, String> {
            let raw = match s.register.bank {
                0 => t[s.register.index],
                2 => c[s.register.index],
                1 => *inputs.get(s.register.index).ok_or("missing pixel input")?,
                3 => *coordinates
                    .get(s.register.index)
                    .ok_or("missing texture coordinate")?,
                _ => unreachable!(),
            };
            Ok(std::array::from_fn(|i| {
                let v = raw[s.swizzle[i]];
                match s.modifier {
                    0 => v,
                    1 => -v,
                    11 => v.abs(),
                    12 => -v.abs(),
                    _ => unreachable!(),
                }
            }))
        }
        for op in &self.ops {
            let (dst, value) = match op {
                Op::Def(index, v) => {
                    c[*index] = *v;
                    continue;
                }
                Op::Texture(dst, coord, sampler) => {
                    let uv = read(*coord, &temporary, &c, inputs, texture_coordinates)?;
                    (*dst, sample(*sampler, [uv[0], uv[1]]))
                }
                Op::Arithmetic(opcode, dst, sources) => {
                    let a = read(sources[0], &temporary, &c, inputs, texture_coordinates)?;
                    let b = if sources.len() > 1 {
                        read(sources[1], &temporary, &c, inputs, texture_coordinates)?
                    } else {
                        [0.; 4]
                    };
                    let d = if sources.len() > 2 {
                        read(sources[2], &temporary, &c, inputs, texture_coordinates)?
                    } else {
                        [0.; 4]
                    };
                    let value = match opcode {
                        1 => a,
                        2 => std::array::from_fn(|i| a[i] + b[i]),
                        4 => std::array::from_fn(|i| a[i] * b[i] + d[i]),
                        5 => std::array::from_fn(|i| a[i] * b[i]),
                        18 => std::array::from_fn(|i| a[i] * b[i] + (1. - a[i]) * d[i]),
                        6 => [1. / a[0]; 4],
                        // D3D9 RSQ takes abs before sqrt and broadcasts the scalar.
                        // Zero/nonfinite results stay explicitly outside this bounded evaluator.
                        7 => [1. / a[0].abs().sqrt(); 4],
                        8 => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; 4],
                        11 => std::array::from_fn(|i| a[i].max(b[i])),
                        _ => unreachable!(),
                    };
                    (*dst, value)
                }
            };
            if !value.iter().all(|n| n.is_finite()) {
                return Err("nonfinite pixel execution result".into());
            }
            let value = if dst.saturate {
                value.map(|v| v.clamp(0., 1.))
            } else {
                value
            };
            let target = if dst.register.bank == 8 {
                &mut output
            } else {
                &mut temporary[dst.register.index]
            };
            for i in 0..4 {
                if dst.mask & (1 << i) != 0 {
                    target[i] = value[i];
                }
            }
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn kernel(words: &[u32]) -> Result<PixelKernel, String> {
        PixelKernel::compile(&dx::parse(
            &words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )?)
    }
    #[test]
    fn numeric_vectors_swizzle_masks_saturation_and_reciprocal() {
        let k = kernel(&[
            0xffff0200, 0x02000001, 0x800f0000, 0xa0e40000, 0x04000004, 0x80150000, 0xa01b0000,
            0xa0e40001, 0xa1e40002, 0x02000006, 0x80080000, 0xa0aa0001, 0x02000001, 0x800f0800,
            0x80e40000, 0xffff,
        ])
        .unwrap();
        let c = [[2., 4., 6., 8.], [0.25, 0.5, 2., 4.], [1., 2., 3., 4.]];
        // x=8*.25-1=1 (sat); z=4*2-3=5 (sat); untouched y=4;
        // reciprocal of c1.z overwrites w=.5.
        assert_eq!(
            k.run(&[], &[], &c, |_, _| panic!()).unwrap(),
            [1., 4., 1., 0.5]
        );
    }
    #[test]
    fn input_colors_and_legacy_texture_coordinates_have_separate_banks() {
        let k = kernel(&[
            0xffff0200, 0x03000002, 0x800f0800, 0x90e40000, 0xb0e40000, 0xffff,
        ])
        .unwrap();
        assert_eq!(
            k.run(
                &[[0.1, 0.2, 0.3, 0.4]],
                &[[0.4, 0.3, 0.2, 0.1]],
                &[],
                |_, _| panic!()
            )
            .unwrap(),
            [0.5; 4]
        );
        assert!(k.run(&[[0.; 4]], &[], &[], |_, _| panic!()).is_err());
    }
    #[test]
    fn reciprocal_sqrt_uses_absolute_scalar_before_vector_maximum() {
        let k = kernel(&[
            0xffff0200, 0x02000007, 0x800f0000, 0xa0000000, 0x0300000b, 0x800f0800, 0x80e40000,
            0xa0e40001, 0xffff,
        ])
        .unwrap();
        assert_eq!(
            k.run(
                &[],
                &[],
                &[[-4.; 4], [0.1, 0.75, 0.9, 0.25]],
                |_, _| panic!()
            )
            .unwrap(),
            [0.5, 0.75, 0.9, 0.5]
        );
        assert!(k
            .run(&[], &[], &[[0.; 4], [0.; 4]], |_, _| panic!())
            .is_err());
    }
    #[test]
    #[ignore = "requires installed original SkinPaint programs"]
    fn all_installed_skinpaint_pixel_programs_compile_and_run_bounded_inputs() {
        use crate::{asset_catalog::ResourceIndex, shader_pack};
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut count = 0;
        for key in index.records.keys().filter(|k| {
            k.type_id == shader_pack::TYPE_ID && k.group_id == shader_pack::GROUP_COMPILED
        }) {
            let pack = shader_pack::parse(&index.read(*key).unwrap()).unwrap();
            for shader in pack.standards.iter().filter(|s| {
                s.header
                    .name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().starts_with("skp"))
            }) {
                for entry in &shader.entries {
                    let kernel = PixelKernel::compile(&dx::parse(&entry.pixel).unwrap())
                        .unwrap_or_else(|e| panic!("{} {:?}: {e}", pack.name, shader.header.name));
                    let pixel = kernel
                        .run(
                            &[[0.25, 0.5, 0.75, 1.]; 16],
                            &[[0.5, 0.5, 0., 1.]; 8],
                            &[
                                [0.25, 0.5, 0.75, 1.],
                                [0.75, 0.5, 0.25, 1.],
                                [0.4, 0.6, 0.2, 1.],
                            ],
                            |s, _| {
                                if s == 0 {
                                    [0.3, 0.5, 0.7, 1.]
                                } else {
                                    [0.6, 0.2, 0.4, 1.]
                                }
                            },
                        )
                        .unwrap_or_else(|e| panic!("{} {:?}: {e}", pack.name, shader.header.name));
                    assert!(pixel.iter().all(|v| v.is_finite()));
                    count += 1;
                }
            }
        }
        assert_eq!(count, 80);
    }
    #[test]
    fn unsupported_program_fails_before_sampling() {
        assert!(kernel(&[0xffff0200, 0x01000041, 0x800f0000, 0xffff]).is_err());
        assert!(kernel(&[0xffff0200, 0x02000001, 0x800f0800, 0xa0e42000, 0xffff]).is_err());
    }
}
