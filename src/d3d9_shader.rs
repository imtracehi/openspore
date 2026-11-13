//! Structural D3D9 shader-model 2/3 bytecode reader. Tokens are LE DWORDs.
//! Microsoft d3d9types.h defines lengths/masks; instruction length excludes
//! the opcode token. Operand tokens remain intact for a future translator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Pixel,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub word_offset: usize,
    pub token: u32,
    pub opcode: u16,
    pub parameters: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub word_offset: usize,
    pub token: u32,
    pub data: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub version: u32,
    pub stage: Stage,
    pub major: u8,
    pub minor: u8,
    pub instructions: Vec<Instruction>,
    pub comments: Vec<Comment>,
}
pub fn parse(bytes: &[u8]) -> Result<Program, String> {
    if bytes.len() < 8 || bytes.len() % 4 != 0 || bytes.len() > 4 * 1024 * 1024 {
        return Err("invalid shader bytecode size".into());
    }
    let words: Vec<_> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let version = words[0];
    let stage = match version >> 16 {
        0xFFFE => Stage::Vertex,
        0xFFFF => Stage::Pixel,
        _ => return Err("invalid D3D9 shader version token".into()),
    };
    let major = (version >> 8) as u8;
    let minor = version as u8;
    if !matches!(major, 2 | 3) {
        return Err(format!("unsupported shader model {major}.{minor}"));
    }
    let mut program = Program {
        version,
        stage,
        major,
        minor,
        instructions: vec![],
        comments: vec![],
    };
    let mut at = 1;
    loop {
        let token = *words.get(at).ok_or("missing shader END token")?;
        if token == 0x0000FFFF {
            if at + 1 != words.len() {
                return Err("data follows shader END token".into());
            }
            return Ok(program);
        }
        let opcode = token as u16;
        if opcode == 0xFFFF {
            return Err("invalid shader END token".into());
        }
        let n = if opcode == 0xFFFE {
            ((token >> 16) & 0x7FFF) as usize
        } else {
            ((token >> 24) & 0xF) as usize
        };
        let end = at
            .checked_add(1)
            .and_then(|v| v.checked_add(n))
            .ok_or("shader instruction range overflow")?;
        let parameters = words
            .get(at + 1..end)
            .ok_or("truncated shader instruction or comment")?
            .to_vec();
        if opcode == 0xFFFE {
            program.comments.push(Comment {
                word_offset: at,
                token,
                data: parameters,
            });
        } else {
            program.instructions.push(Instruction {
                word_offset: at,
                token,
                opcode,
                parameters,
            });
        }
        at = end;
    }
}
pub fn register_type(token: u32) -> u32 {
    ((token >> 28) & 7) | ((token >> 8) & 0x18)
}
pub fn register_number(token: u32) -> u32 {
    token & 0x7FF
}
pub fn source_swizzle(token: u32) -> [usize; 4] {
    std::array::from_fn(|i| ((token >> (16 + i * 2)) & 3) as usize)
}
pub fn destination_mask(token: u32) -> u32 {
    (token >> 16) & 15
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
    #[test]
    fn comments_do_not_terminate_and_instruction_lengths_exclude_opcode() {
        let b = bytes(&[
            0xFFFE0300, 0x0001FFFE, 0x0000FFFF, 0x02000001, 0x800F0000, 0x90E40000, 0xFFFF,
        ]);
        let p = parse(&b).unwrap();
        assert_eq!(p.stage, Stage::Vertex);
        assert_eq!(p.comments[0].data, [0xFFFF]);
        assert_eq!(p.instructions[0].word_offset, 3);
        assert_eq!(p.instructions[0].parameters, [0x800F0000, 0x90E40000]);
        assert_eq!(register_type(0x90E40000), 1);
        assert_eq!(register_type(0xA0E40800), 10);
        assert_eq!(register_number(0x90E40007), 7);
        assert_eq!(source_swizzle(0x90E40000), [0, 1, 2, 3]);
        assert_eq!(destination_mask(0x80050000), 5);
    }
    #[test]
    fn truncations_unknown_model_and_trailing_data_fail() {
        let b = bytes(&[
            0xFFFF0200, 0x03000042, 0x800F0000, 0xB0E40000, 0xA0E40800, 0xFFFF,
        ]);
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err(), "accepted prefix {n}");
        }
        assert!(parse(&bytes(&[0xFFFF0104, 0xFFFF])).is_err());
        assert!(parse(&bytes(&[0xFFFF0300, 0xFFFF, 1])).is_err());
        assert!(parse(&bytes(&[0xFFFF0300, 0x7FFFFFFE, 0xFFFF])).is_err());
    }
}
