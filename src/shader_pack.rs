//! Native CompiledShaders packs: BE metadata surrounding opaque D3D9 programs.
//! Independent layout reader based on SporeModder-FX's shader read methods.
//! Program decoding/retrieval is distinct from translation and GPU execution.
use crate::{asset_catalog::ResourceIndex, resource_audit::key_json};
pub const TYPE_ID: u32 = 0x0469A3F7;
pub const GROUP_COMPILED: u32 = 0x40212004;
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub id: u32,
    pub unknown: [u32; 3],
    pub vertex_version: u32,
    pub pixel_version: u32,
    pub flags: u32,
    pub name: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Uniform {
    pub data_index: i16,
    pub field_2: i16,
    pub register_size: i16,
    pub register: i16,
    pub flags: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct StandardEntry {
    pub render_type: u8,
    pub vertex: Vec<u8>,
    pub pixel: Vec<u8>,
    pub vertex_uniforms: Vec<Uniform>,
    pub pixel_uniforms: Vec<Uniform>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Standard {
    pub header: Header,
    pub entries: Vec<StandardEntry>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    pub check_type: i8,
    pub data: [i16; 3],
    pub vertex_usage: u32,
    pub required_flags: u32,
    pub excluded_flags: u32,
    pub flags: u32,
    pub field_18: i8,
    pub fragment_index: u8,
    pub legacy: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct BuilderEntry {
    pub render_type: u8,
    pub vertex: Vec<Selector>,
    pub pixel: Vec<Selector>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Builder {
    pub header: Header,
    pub entries: Vec<BuilderEntry>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub fragments: [u8; 32],
    pub program: Vec<u8>,
    pub uniforms: Vec<Uniform>,
    pub start_registers: Vec<u32>,
    pub data_flags: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub version: u32,
    pub standards: Vec<Standard>,
    pub builders: Vec<Builder>,
    pub unused: [u32; 2],
    pub vertex: Vec<Compiled>,
    pub pixel: Vec<Compiled>,
    pub name: String,
    pub trailing: Vec<u8>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("shader offset overflow")?;
        let b = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("truncated shader pack at {} need {n}", self.at))?;
        self.at = end;
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn count(&mut self, min: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > 100_000 || n > self.bytes.len().saturating_sub(self.at) / min.max(1) {
            Err("shader count exceeds remaining data or limit".into())
        } else {
            Ok(n)
        }
    }
    fn blob(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        if n > 4 * 1024 * 1024 {
            return Err("shader program exceeds limit".into());
        }
        Ok(self.take(n)?.to_vec())
    }
    fn text(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return Err("shader name exceeds limit".into());
        }
        let bytes = self.take(n)?;
        if !bytes.is_ascii() {
            return Err("non-ASCII shader name".into());
        }
        Ok(String::from_utf8(bytes.to_vec()).unwrap())
    }
    fn header(&mut self) -> Result<Header, String> {
        Ok(Header {
            id: self.u32()?,
            unknown: [self.u32()?, self.u32()?, self.u32()?],
            vertex_version: self.u32()?,
            pixel_version: self.u32()?,
            flags: self.u32()?,
            name: None,
        })
    }
    fn named_header(&mut self) -> Result<Header, String> {
        let mut h = self.header()?;
        if h.flags & 0x10 != 0 {
            h.name = Some(self.text()?);
        }
        Ok(h)
    }
    fn uniforms(&mut self) -> Result<Vec<Uniform>, String> {
        let n = self.count(12)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(Uniform {
                data_index: self.i16()?,
                field_2: self.i16()?,
                register_size: self.i16()?,
                register: self.i16()?,
                flags: self.u32()?,
            });
        }
        Ok(v)
    }
    fn selectors(&mut self, version: u32) -> Result<Vec<Selector>, String> {
        let n = self.count(if version <= 6 { 32 } else { 25 })?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let check_type = self.u8()? as i8;
            let mut legacy = vec![];
            if version <= 6 {
                legacy.extend_from_slice(self.take(1)?);
            }
            let data = [self.i16()?, self.i16()?, self.i16()?];
            if version <= 6 {
                legacy.extend_from_slice(self.take(6)?);
            }
            v.push(Selector {
                check_type,
                data,
                vertex_usage: self.u32()?,
                required_flags: self.u32()?,
                excluded_flags: self.u32()?,
                flags: self.u32()?,
                field_18: self.u8()? as i8,
                fragment_index: self.u8()?,
                legacy,
            });
        }
        Ok(v)
    }
    fn compiled(&mut self) -> Result<Vec<Compiled>, String> {
        let n = self.count(44)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let fragments = self.take(32)?.try_into().unwrap();
            let program = self.blob()?;
            let uniforms = self.uniforms()?;
            let mut start_registers = Vec::with_capacity(uniforms.len());
            for _ in &uniforms {
                start_registers.push(self.u32()?);
            }
            v.push(Compiled {
                fragments,
                program,
                uniforms,
                start_registers,
                data_flags: self.u32()?,
            });
        }
        Ok(v)
    }
}
pub fn parse(bytes: &[u8]) -> Result<Pack, String> {
    let mut r = Reader { bytes, at: 0 };
    let version = r.u32()?;
    if !(1..=8).contains(&version) {
        return Err(format!("unsupported shader pack version {version}"));
    }
    let n = r.count(29)?;
    let mut standards = Vec::with_capacity(n);
    for _ in 0..n {
        let header = r.named_header()?;
        let mut entries = vec![];
        loop {
            let render_type = r.u8()?;
            if render_type == 255 {
                break;
            }
            if entries
                .iter()
                .any(|e: &StandardEntry| e.render_type == render_type)
            {
                return Err("duplicate standard render type".into());
            }
            entries.push(StandardEntry {
                render_type,
                vertex: r.blob()?,
                pixel: r.blob()?,
                vertex_uniforms: r.uniforms()?,
                pixel_uniforms: r.uniforms()?,
            });
        }
        standards.push(Standard { header, entries });
    }
    let n = r.count(29)?;
    let mut builders = Vec::with_capacity(n);
    for _ in 0..n {
        let header = r.named_header()?;
        let mut entries = vec![];
        loop {
            let render_type = r.u8()?;
            if render_type == 255 {
                break;
            }
            if entries
                .iter()
                .any(|e: &BuilderEntry| e.render_type == render_type)
            {
                return Err("duplicate builder render type".into());
            }
            entries.push(BuilderEntry {
                render_type,
                vertex: r.selectors(version)?,
                pixel: r.selectors(version)?,
            });
        }
        builders.push(Builder { header, entries });
    }
    let unused_vertex = r.u32()?;
    let vertex = r.compiled()?;
    let unused_pixel = r.u32()?;
    let pixel = r.compiled()?;
    let name = r.text()?;
    Ok(Pack {
        version,
        standards,
        builders,
        unused: [unused_vertex, unused_pixel],
        vertex,
        pixel,
        name,
        trailing: bytes[r.at..].to_vec(),
    })
}
pub fn audit(index: &ResourceIndex) -> serde_json::Value {
    let mut keys: Vec<_> = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == TYPE_ID && k.group_id == GROUP_COMPILED)
        .collect();
    keys.sort_by_key(|k| k.instance_id);
    let mut rows = vec![];
    let mut errors = vec![];
    for key in keys {
        match index.read(key).and_then(|b| parse(&b)) {
            Ok(pack) => {
                let programs: Vec<_> = pack
                    .vertex
                    .iter()
                    .map(|v| &v.program)
                    .chain(pack.pixel.iter().map(|v| &v.program))
                    .chain(
                        pack.standards
                            .iter()
                            .flat_map(|s| &s.entries)
                            .flat_map(|e| [&e.vertex, &e.pixel]),
                    )
                    .collect();
                let mut versions = std::collections::BTreeMap::<String, usize>::new();
                let mut opcodes = std::collections::BTreeMap::<String, usize>::new();
                let mut program_errors = vec![];
                let mut decoded_programs = 0;
                for (program_index, program) in programs.iter().enumerate() {
                    if let Some(word) = program.get(..4) {
                        *versions
                            .entry(format!(
                                "{:08X}",
                                u32::from_le_bytes(word.try_into().unwrap())
                            ))
                            .or_default() += 1;
                    }
                    if program.is_empty() {
                        continue;
                    }
                    match crate::d3d9_shader::parse(program) {
                        Ok(p) => {
                            decoded_programs += 1;
                            for i in p.instructions {
                                *opcodes.entry(format!("{:04X}", i.opcode)).or_default() += 1;
                            }
                        }
                        Err(e) => program_errors
                            .push(serde_json::json!({"program_index":program_index,"error":e})),
                    }
                }
                rows.push(serde_json::json!({"key":key_json(key),"name":pack.name,"version":pack.version,"standard_shaders":pack.standards.len(),"builders":pack.builders.len(),"vertex_variants":pack.vertex.len(),"pixel_variants":pack.pixel.len(),"programs":programs.len(),"decoded_programs":decoded_programs,"empty_programs":programs.iter().filter(|p|p.is_empty()).count(),"program_errors":program_errors,"opcodes":opcodes,"bytecode_bytes":programs.iter().map(|v|v.len()).sum::<usize>(),"program_versions":versions,"renderer_ids":pack.standards.iter().map(|s|&s.header).chain(pack.builders.iter().map(|s|&s.header)).map(|h|serde_json::json!({"id":format!("{:08X}",h.id),"name":h.name})).collect::<Vec<_>>(),"trailing_bytes":pack.trailing.len()}));
            }
            Err(e) => errors.push(serde_json::json!({"key":key_json(key),"error":e})),
        }
    }
    serde_json::json!({"packs":rows,"errors":errors,"index_errors":index.errors})
}

/// Retrieve the exact native SkinPainter programs for a focused interpreter
/// investigation. Shader metadata and bytecode are both retained in the dump.
pub fn run_dump_skinpaint() {
    let result = (|| {
        let install = crate::spore_install::discover()?;
        let index = ResourceIndex::load(&install.packages);
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == TYPE_ID && k.group_id == GROUP_COMPILED)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let mut rows = vec![];
        for key in keys {
            let pack = parse(&index.read(key)?)?;
            for standard in pack.standards.iter().filter(|s| {
                s.header.id == 0x700000ad
                    || s.header
                        .name
                        .as_ref()
                        .is_some_and(|n| n.to_lowercase().starts_with("skp"))
            }) {
                for entry in &standard.entries {
                    let pixel = crate::d3d9_shader::parse(&entry.pixel)?;
                    rows.push(serde_json::json!({"pack_key":key_json(key),"pack":pack.name,"shader_id":format!("{:08X}",standard.header.id),"name":standard.header.name,"render_type":entry.render_type,"vertex_uniforms":entry.vertex_uniforms.iter().map(|u|serde_json::json!({"index":u.data_index,"register":u.register,"size":u.register_size,"flags":u.flags})).collect::<Vec<_>>(),"pixel_uniforms":entry.pixel_uniforms.iter().map(|u|serde_json::json!({"index":u.data_index,"register":u.register,"size":u.register_size,"flags":u.flags})).collect::<Vec<_>>(),"pixel_version":format!("{:08X}",pixel.version),"instructions":pixel.instructions.iter().map(|i|serde_json::json!({"offset":i.word_offset,"opcode":i.opcode,"token":format!("{:08X}",i.token),"parameters":i.parameters.iter().map(|p|format!("{p:08X}")).collect::<Vec<_>>()})).collect::<Vec<_>>(),"pixel_words":entry.pixel.chunks_exact(4).map(|b|format!("{:08X}",u32::from_le_bytes(b.try_into().unwrap()))).collect::<Vec<_>>()}));
                }
            }
        }
        Ok::<_, String>(serde_json::json!({"programs":rows,"index_errors":index.errors}))
    })();
    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
pub fn run_audit() {
    match crate::spore_install::discover() {
        Ok(i) => println!(
            "{}",
            serde_json::to_string_pretty(&audit(&ResourceIndex::load(&i.packages))).unwrap()
        ),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test survey_window_builders -- --ignored --nocapture`: the
    /// fragment chains of the window builders (and the lit rigblock ones).
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_window_builders() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        for tier in [3u32] {
            let key = crate::dbpf::ResourceKey { type_id: TYPE_ID, group_id: GROUP_COMPILED, instance_id: tier };
            let pack = parse(&index.read(key).unwrap()).unwrap();
            for b in &pack.builders {
                if !matches!(b.header.id, 0x4000_0005 | 0x4000_0007 | 0x8000_0005) {
                    continue;
                }
                println!("builder {:08x} {:?}", b.header.id, b.header.name);
                for (i, e) in b.entries.iter().enumerate() {
                    let vs: Vec<(u8, u32, u32)> = e.vertex.iter().map(|s| (s.fragment_index, s.required_flags, s.excluded_flags)).collect();
                    let ps: Vec<(u8, u32, u32)> = e.pixel.iter().map(|s| (s.fragment_index, s.required_flags, s.excluded_flags)).collect();
                    println!("  entry {i} type {} vs {vs:?} ps {ps:?}", e.render_type);
                }
            }
            for s in &pack.standards {
                if matches!(s.header.id, 0x8000_0005 | 0x8000_0004) {
                    println!("standard {:08x} {:?} entries {}", s.header.id, s.header.name, s.entries.len());
                }
            }
        }
    }
    fn word(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_be_bytes());
    }
    fn fixture(version: u32) -> Vec<u8> {
        let mut b = vec![];
        word(&mut b, version);
        word(&mut b, 1);
        for v in [0x80000002, 11, 12, 13, 0xFFFE0300, 0xFFFF0300, 0x10] {
            word(&mut b, v);
        }
        word(&mut b, 1);
        b.push(b'S');
        b.push(3);
        word(&mut b, 8);
        b.extend_from_slice(&[0, 3, 254, 255, 255, 255, 0, 0]);
        word(&mut b, 0);
        word(&mut b, 0);
        word(&mut b, 0);
        b.push(255);
        word(&mut b, 1);
        for v in [0x80000004, 0, 0, 0, 0xFFFE0300, 0xFFFF0300, 0] {
            word(&mut b, v);
        }
        b.push(7);
        word(&mut b, 1);
        b.push(10);
        if version <= 6 {
            b.push(91);
        }
        for v in [1i16, 2, 3] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        if version <= 6 {
            b.extend_from_slice(&[92; 6]);
        }
        for v in [21, 22, 23, 24] {
            word(&mut b, v);
        }
        b.extend_from_slice(&[255, 4]);
        word(&mut b, 0);
        b.push(255);
        word(&mut b, 71);
        word(&mut b, 1);
        b.extend_from_slice(&[9; 32]);
        word(&mut b, 8);
        b.extend_from_slice(&[0, 3, 254, 255, 255, 255, 0, 0]);
        word(&mut b, 1);
        for v in [0x210i16, -1, 4, 2] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        word(&mut b, 123);
        word(&mut b, 7);
        word(&mut b, 456);
        word(&mut b, 72);
        word(&mut b, 0);
        word(&mut b, 1);
        b.push(b'P');
        b
    }
    #[test]
    fn native_metadata_bytecode_uniforms_and_old_selector_padding_survive() {
        for version in [6, 7] {
            let p = parse(&fixture(version)).unwrap();
            assert_eq!(p.standards[0].header.name.as_deref(), Some("S"));
            assert_eq!(
                p.standards[0].entries[0].vertex,
                [0, 3, 254, 255, 255, 255, 0, 0]
            );
            let s = &p.builders[0].entries[0].vertex[0];
            assert_eq!(s.data, [1, 2, 3]);
            assert_eq!(s.field_18, -1);
            assert_eq!(s.fragment_index, 4);
            assert_eq!(s.legacy.len(), if version == 6 { 7 } else { 0 });
            assert_eq!(p.vertex[0].uniforms[0].data_index, 0x210);
            assert_eq!(p.vertex[0].start_registers, [7]);
            assert_eq!(p.unused, [71, 72]);
        }
    }
    #[test]
    fn all_truncations_and_oversized_counts_fail() {
        let b = fixture(7);
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err(), "accepted prefix {n}");
        }
        let mut bad = b.clone();
        bad[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse(&bad).is_err());
        bad = b;
        bad[..4].copy_from_slice(&99u32.to_be_bytes());
        assert!(parse(&bad).is_err());
    }
}
