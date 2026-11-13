//! Lossless D3D9 packet boundary. No Bevy basis, material, or texture conversion.
//! All state values and float bit patterns are the values passed to D3D9.
use serde::{Deserialize, Serialize};

pub const PACKET_VERSION: u32 = 4;
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRenderPacket {
    pub version: u32,
    pub label: String,
    pub targets: Vec<Target>,
    pub textures: Vec<Texture>,
    pub commands: Vec<Command>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub format: i32,
    /// Zero means no depth surface. Otherwise a native D3DFORMAT.
    pub depth_format: i32,
    /// A standalone render-target surface rather than a texture level.
    #[serde(default, skip_serializing_if = "is_false")]
    pub surface_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub format: i32,
    /// One face for 2D, six faces in native D3DCUBEMAP_FACES order for a cube.
    pub faces: Vec<Vec<Mip>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mip {
    pub row_pitch: usize,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Command {
    Clear {
        target: usize,
        flags: u32,
        color: u32,
        depth_bits: u32,
        stencil: u32,
        /// Absent in v1: clear the entire target with scissor disabled.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        viewport: Option<[u32; 6]>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        rects: Vec<[i32; 4]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scissor: Option<[i32; 4]>,
    },
    Draw(Draw),
    ColorFill {
        target: usize,
        rect: Option<[i32; 4]>,
        color: u32,
    },
    StretchRect {
        source: usize,
        target: usize,
        source_rect: Option<[i32; 4]>,
        target_rect: Option<[i32; 4]>,
        filter: i32,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draw {
    pub target: usize,
    /// x, y, width, height, minZ bits, maxZ bits.
    pub viewport: [u32; 6],
    /// Native RECT when D3DRS_SCISSORTESTENABLE is set; otherwise disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scissor: Option<[i32; 4]>,
    pub vertex_shader: Option<Vec<u32>>,
    pub pixel_shader: Option<Vec<u32>>,
    /// FVF or declaration; exactly one must be supplied.
    pub fvf: Option<u32>,
    pub declaration: Vec<VertexElement>,
    pub vertices: Vec<u8>,
    pub stride: u32,
    pub vertex_count: u32,
    pub indices: Option<IndexData>,
    /// v3: original indexed arguments and the captured vertex slice's origin.
    /// Index bytes remain in the original index namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_range: Option<IndexedRange>,
    pub primitive_type: i32,
    pub primitive_count: u32,
    pub render_states: Vec<[u32; 2]>,
    /// sampler, state type, DWORD value (including float bit patterns).
    pub sampler_states: Vec<[u32; 3]>,
    pub texture_stage_states: Vec<[u32; 3]>,
    pub bindings: Vec<Binding>,
    pub constants: Vec<Constants>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VertexElement {
    pub stream: u16,
    pub offset: u16,
    pub kind: u8,
    pub method: u8,
    pub usage: u8,
    pub usage_index: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexData {
    pub bits: u8,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndexedSource {
    Buffer,
    Up,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedRange {
    pub source: IndexedSource,
    pub base_vertex_index: i32,
    pub min_vertex_index: u32,
    pub num_vertices: u32,
    pub start_index: u32,
    pub stream_offset: u32,
    /// Vertex number, relative to the original stream offset, at bytes[0].
    pub vertex_origin: u32,
}
impl IndexedRange {
    pub fn replay_base(&self) -> Result<i32, String> {
        i32::try_from(self.base_vertex_index as i64 - self.vertex_origin as i64)
            .map_err(|_| "indexed slice displacement exceeds D3D9 signed base".into())
    }
    fn validate(&self, draw: &Draw, index_width: usize) -> Result<(), String> {
        if self.num_vertices == 0 || self.num_vertices != draw.vertex_count {
            return Err("indexed range and captured vertex count differ".into());
        }
        if self.source == IndexedSource::Up
            && (self.base_vertex_index != 0 || self.start_index != 0 || self.stream_offset != 0)
        {
            return Err("UP draw contains buffer-only arguments".into());
        }
        let origin = self.base_vertex_index as i64 + self.min_vertex_index as i64;
        if origin != self.vertex_origin as i64 {
            return Err("indexed vertex origin differs from base + minimum".into());
        }
        self.replay_base()?;
        let end = self.min_vertex_index as u64 + self.num_vertices as u64;
        let vertex_end = self.stream_offset as u64
            + (self.vertex_origin as u64 + self.num_vertices as u64) * draw.stride as u64;
        let index_end = self.start_index as u64 * index_width as u64
            + draw
                .indices
                .as_ref()
                .ok_or("indexed range without indices")?
                .bytes
                .len() as u64;
        if end > u32::MAX as u64 + 1 || vertex_end > u32::MAX as u64 || index_end > u32::MAX as u64
        {
            return Err("original indexed byte/index range overflows D3D9".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub sampler: u32,
    pub source: Option<TextureSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TextureSource {
    Texture(usize),
    Target(usize),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Constants {
    Float {
        vertex: bool,
        start: u32,
        bits: Vec<[u32; 4]>,
    },
    Int {
        vertex: bool,
        start: u32,
        values: Vec<[i32; 4]>,
    },
    Bool {
        vertex: bool,
        start: u32,
        values: Vec<bool>,
    },
}

/// Meaningful bytes per row and rows for one native surface, including DXT blocks.
pub fn surface_layout(format: i32, width: u32, height: u32) -> Result<(usize, usize), String> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("surface dimensions must be 1..8192".into());
    }
    let (row, rows) = match format as u32 {
        21 | 22 | 32 | 33 => (width as usize * 4, height as usize),
        23 | 25 | 26 | 111 => (width as usize * 2, height as usize),
        28 | 50 => (width as usize, height as usize),
        113 | 115 => (width as usize * 8, height as usize),
        114 => (width as usize * 4, height as usize),
        116 => (width as usize * 16, height as usize),
        0x31545844 => (width.div_ceil(4) as usize * 8, height.div_ceil(4) as usize),
        0x33545844 | 0x35545844 => (width.div_ceil(4) as usize * 16, height.div_ceil(4) as usize),
        _ => return Err(format!("unsupported native surface format 0x{format:08X}")),
    };
    Ok((row, rows))
}
fn primitive_vertices(kind: i32, count: u32) -> Result<usize, String> {
    if count == 0 || count > 8_000_000 {
        return Err("invalid primitive count".into());
    }
    Ok(match kind {
        1 => count,
        2 => count * 2,
        3 => count + 1,
        4 => count * 3,
        5 | 6 => count + 2,
        _ => return Err("invalid D3D primitive type".into()),
    } as usize)
}
impl NativeRenderPacket {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.version, 1 | 2 | 3 | PACKET_VERSION) {
            return Err("unsupported render packet version".into());
        }
        if self.targets.is_empty()
            || self.targets.len() > 64
            || self.textures.len() > 4096
            || self.commands.len() > 100_000
        {
            return Err("packet resource/command limit exceeded".into());
        }
        for target in &self.targets {
            if target.surface_only && self.version < 4 {
                return Err("v4 standalone surface in an older packet".into());
            }
            surface_layout(target.format, target.width, target.height)?;
            if !matches!(target.depth_format, 0 | 70 | 71 | 73 | 75 | 77 | 79 | 80) {
                return Err("unsupported depth format".into());
            }
        }
        for tex in &self.textures {
            if !matches!(tex.faces.len(), 1 | 6)
                || (tex.faces.len() == 6 && tex.width != tex.height)
            {
                return Err("invalid texture face count or nonsquare cube".into());
            }
            let levels = tex.faces[0].len();
            let max_levels = 32 - tex.width.max(tex.height).leading_zeros();
            if levels == 0 || levels > max_levels as usize {
                return Err("invalid mip count".into());
            }
            for face in &tex.faces {
                if face.len() != levels {
                    return Err("cube mip counts differ".into());
                }
                for (level, mip) in face.iter().enumerate() {
                    let (row, rows) = surface_layout(
                        tex.format,
                        (tex.width >> level).max(1),
                        (tex.height >> level).max(1),
                    )?;
                    if mip.row_pitch < row
                        || mip.row_pitch > 262144
                        || mip.bytes.len() != mip.row_pitch * rows
                    {
                        return Err("invalid native mip pitch/byte length".into());
                    }
                }
            }
        }
        for command in &self.commands {
            let target = match command {
                Command::Clear {
                    target,
                    flags,
                    depth_bits,
                    viewport,
                    rects,
                    scissor,
                    ..
                } => {
                    if *flags == 0
                        || flags & !7 != 0
                        || !(0.0..=1.0).contains(&f32::from_bits(*depth_bits))
                    {
                        return Err("invalid clear".into());
                    }
                    if self.version == 1
                        && (viewport.is_some() || !rects.is_empty() || scissor.is_some())
                    {
                        return Err("v2 clear state in a v1 packet".into());
                    }
                    if rects.len() > 4096 {
                        return Err("clear rectangle count limit".into());
                    }
                    for rect in rects.iter().chain(scissor.iter()) {
                        validate_rect(*rect)?;
                    }
                    if let Some(viewport) = viewport {
                        validate_viewport(
                            *viewport,
                            self.targets.get(*target).ok_or("unknown clear target")?,
                        )?;
                    }
                    *target
                }
                Command::Draw(draw) => {
                    draw.validate(self)?;
                    draw.target
                }
                Command::ColorFill { target, rect, .. } => {
                    if self.version < 4 {
                        return Err("v4 fill in an older packet".into());
                    }
                    validate_surface_rect(
                        *rect,
                        self.targets.get(*target).ok_or("unknown fill target")?,
                    )?;
                    *target
                }
                Command::StretchRect {
                    source,
                    target,
                    source_rect,
                    target_rect,
                    filter,
                } => {
                    if self.version < 4 {
                        return Err("v4 copy in an older packet".into());
                    }
                    if source == target || !matches!(filter, 0 | 1 | 2) {
                        return Err("self-copy or unsupported native copy filter".into());
                    }
                    let from = self.targets.get(*source).ok_or("unknown copy source")?;
                    let to = self.targets.get(*target).ok_or("unknown copy target")?;
                    if from.format != to.format {
                        return Err("surface format conversion is not supported".into());
                    }
                    validate_surface_rect(*source_rect, from)?;
                    validate_surface_rect(*target_rect, to)?;
                    *target
                }
            };
            let desc = self.targets.get(target).ok_or("unknown target")?;
            if let Command::Clear { flags, .. } = command {
                if flags & 6 != 0 && desc.depth_format == 0 {
                    return Err("depth/stencil clear without surface".into());
                }
            }
        }
        Ok(())
    }
}
impl Draw {
    fn validate(&self, packet: &NativeRenderPacket) -> Result<(), String> {
        let target = packet
            .targets
            .get(self.target)
            .ok_or("unknown draw target")?;
        validate_viewport(self.viewport, target)?;
        if packet.version == 1 && self.scissor.is_some() {
            return Err("v2 scissor state in a v1 packet".into());
        }
        if let Some(rect) = self.scissor {
            validate_rect(rect)?;
        }
        // The explicit rectangle is authoritative. Conflicting raw state must
        // fail before a driver call, rather than inheriting a prior draw's clip.
        if self
            .render_states
            .iter()
            .any(|[state, value]| *state == 174 && (*value != 0) != self.scissor.is_some())
        {
            return Err("scissor rectangle and enable state disagree".into());
        }
        for (shader, stage) in [
            (&self.vertex_shader, crate::d3d9_shader::Stage::Vertex),
            (&self.pixel_shader, crate::d3d9_shader::Stage::Pixel),
        ] {
            if let Some(words) = shader {
                let bytes: Vec<u8> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
                if crate::d3d9_shader::parse(&bytes)?.stage != stage {
                    return Err("shader stage mismatch".into());
                }
            }
        }
        if self.fvf.is_some() == !self.declaration.is_empty() {
            return Err("supply exactly one FVF or declaration".into());
        }
        if let Some(fvf) = self.fvf {
            // Version 1's fixed pipeline is the verified XYZRHW/COLOR0/COLOR1/
            // TEXCOORD0..7 full-screen layout. Other native meshes use explicit
            // declarations and vertex programs, not guessed fixed transforms.
            if fvf != 0x8c4 || self.stride < 88 || self.vertex_shader.is_some() {
                return Err("unsupported fixed-pipeline FVF or stride".into());
            }
        }
        if self.stride == 0
            || self.stride > 2048
            || self.vertex_count == 0
            || self.vertices.len() as u64 != self.stride as u64 * self.vertex_count as u64
            || self.vertices.len() > 64 * 1024 * 1024
        {
            return Err("invalid vertex buffer length".into());
        }
        let element_sizes = [4, 8, 12, 16, 4, 4, 4, 8, 4, 4, 8, 4, 8, 4, 4, 4, 8];
        for element in &self.declaration {
            if element.stream != 0
                || element.method != 0
                || element.usage > 13
                || element_sizes
                    .get(element.kind as usize)
                    .is_none_or(|size| element.offset as u32 + size > self.stride)
            {
                return Err("unsupported vertex declaration (only stream 0/default method)".into());
            }
        }
        let count = primitive_vertices(self.primitive_type, self.primitive_count)?;
        if self.indexed_range.is_some() && (packet.version < 3 || self.indices.is_none()) {
            return Err("v3 indexed range requires v3 packet and indices".into());
        }
        if let Some(index) = &self.indices {
            if !matches!(index.bits, 16 | 32)
                || index.bytes.len() != count * (index.bits as usize / 8)
            {
                return Err("invalid index buffer length".into());
            }
            if self.primitive_type == 1 {
                return Err("indexed point lists are not supported by D3D9".into());
            }
            if let Some(range) = &self.indexed_range {
                range.validate(self, index.bits as usize / 8)?;
            }
            let min = self
                .indexed_range
                .as_ref()
                .map_or(0, |r| r.min_vertex_index) as u64;
            let end = min + self.vertex_count as u64;
            for bytes in index.bytes.chunks_exact(index.bits as usize / 8) {
                let i = if index.bits == 16 {
                    u16::from_le_bytes(bytes.try_into().unwrap()) as u32
                } else {
                    u32::from_le_bytes(bytes.try_into().unwrap())
                };
                if (i as u64) < min || (i as u64) >= end {
                    return Err("index outside declared vertex range".into());
                }
            }
        } else if count > self.vertex_count as usize {
            return Err("draw exceeds vertex buffer".into());
        }
        for binding in &self.bindings {
            if binding.sampler > 15 && !(256..=260).contains(&binding.sampler) {
                return Err("invalid native sampler".into());
            }
            match binding.source {
                Some(TextureSource::Texture(i)) if i >= packet.textures.len() => {
                    return Err("unknown texture".into())
                }
                Some(TextureSource::Target(i)) if i >= packet.targets.len() || i == self.target => {
                    return Err("unknown target or render feedback".into())
                }
                Some(TextureSource::Target(i)) if packet.targets[i].surface_only => {
                    return Err("standalone surface cannot be a shader texture".into())
                }
                _ => {}
            }
        }
        for c in &self.constants {
            let (start, len, max) = match c {
                Constants::Float {
                    vertex,
                    start,
                    bits,
                } => (*start, bits.len(), if *vertex { 256 } else { 224 }),
                Constants::Int { start, values, .. } => (*start, values.len(), 16),
                Constants::Bool { start, values, .. } => (*start, values.len(), 16),
            };
            if len == 0 || start as usize + len > max {
                return Err("shader constant range exceeds D3D9 registers".into());
            }
        }
        Ok(())
    }
}

fn validate_viewport([x, y, w, h, z0, z1]: [u32; 6], target: &Target) -> Result<(), String> {
    if w == 0
        || h == 0
        || x.checked_add(w).is_none_or(|v| v > target.width)
        || y.checked_add(h).is_none_or(|v| v > target.height)
        || !(0.0..=1.0).contains(&f32::from_bits(z0))
        || !(0.0..=1.0).contains(&f32::from_bits(z1))
        || f32::from_bits(z0) > f32::from_bits(z1)
    {
        return Err("invalid viewport".into());
    }
    Ok(())
}
fn validate_rect([left, top, right, bottom]: [i32; 4]) -> Result<(), String> {
    // Rectangles may extend beyond the viewport. Retain signed coordinates and
    // let D3D9 perform clipping; never clamp the captured input.
    if left > right || top > bottom {
        return Err("inverted native rectangle".into());
    }
    Ok(())
}
fn validate_surface_rect(rect: Option<[i32; 4]>, target: &Target) -> Result<(), String> {
    // Copy/fill rectangles are within a surface, unlike viewport-clipped clears.
    if let Some([left, top, right, bottom]) = rect {
        if left < 0
            || top < 0
            || left >= right
            || top >= bottom
            || right as u32 > target.width
            || bottom as u32 > target.height
        {
            return Err("native surface rectangle is empty or outside its surface".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_copies_preserve_kind_rectangles_and_filter() {
        let target = Target {
            width: 96,
            height: 64,
            format: 21,
            depth_format: 0,
            surface_only: false,
        };
        let standalone = Target {
            width: 48,
            height: 32,
            surface_only: true,
            ..target.clone()
        };
        let packet = NativeRenderPacket {
            version: PACKET_VERSION,
            label: "copy validation".into(),
            targets: vec![target, standalone],
            textures: vec![],
            commands: vec![
                Command::ColorFill {
                    target: 1,
                    rect: None,
                    color: 0xff203040,
                },
                Command::StretchRect {
                    source: 0,
                    target: 1,
                    source_rect: Some([7, 5, 89, 61]),
                    target_rect: Some([3, 2, 45, 30]),
                    filter: 2,
                },
            ],
        };
        packet.validate().unwrap();
        let round: NativeRenderPacket =
            serde_json::from_slice(&serde_json::to_vec(&packet).unwrap()).unwrap();
        round.validate().unwrap();
        for mutation in 0..7 {
            let mut invalid = packet.clone();
            match mutation {
                0 => invalid.version = 3,
                1 => invalid.targets[1].format = 22,
                2 => {
                    if let Command::StretchRect { source, .. } = &mut invalid.commands[1] {
                        *source = 1
                    }
                }
                3 => {
                    if let Command::StretchRect { filter, .. } = &mut invalid.commands[1] {
                        *filter = 3
                    }
                }
                4 => {
                    if let Command::StretchRect { target_rect, .. } = &mut invalid.commands[1] {
                        *target_rect = Some([0, 0, 49, 32])
                    }
                }
                5 => {
                    if let Command::ColorFill { rect, .. } = &mut invalid.commands[0] {
                        *rect = Some([0, 0, 0, 1])
                    }
                }
                6 => {
                    if let Command::StretchRect { source, .. } = &mut invalid.commands[1] {
                        *source = 100
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                invalid.validate().is_err(),
                "surface mutation {mutation} escaped"
            );
        }
    }
    #[test]
    fn standalone_targets_cannot_be_shader_bindings() {
        let mut packet = indexed_packet(0, 0, IndexedSource::Buffer, 16);
        packet.targets.push(Target {
            width: 96,
            height: 64,
            format: 21,
            depth_format: 0,
            surface_only: true,
        });
        let Command::Draw(draw) = &mut packet.commands[0] else {
            unreachable!()
        };
        draw.bindings.push(Binding {
            sampler: 0,
            source: Some(TextureSource::Target(1)),
        });
        assert!(packet.validate().unwrap_err().contains("standalone"));
    }
    fn indexed_packet(base: i32, min: u32, source: IndexedSource, bits: u8) -> NativeRenderPacket {
        let mut draw = crate::native_paint::fullscreen(
            96,
            64,
            vec![0xffff0200, 0xffff],
            vec![[1.; 4]],
            vec![],
        );
        draw.primitive_type = 4;
        draw.indices = Some(IndexData {
            bits,
            bytes: [0u32, 1, 2, 2, 1, 3]
                .iter()
                .flat_map(|i| (min + i).to_le_bytes()[..bits as usize / 8].to_vec())
                .collect(),
        });
        draw.indexed_range = Some(IndexedRange {
            source,
            base_vertex_index: base,
            min_vertex_index: min,
            num_vertices: 4,
            start_index: if source == IndexedSource::Buffer {
                3
            } else {
                0
            },
            stream_offset: if source == IndexedSource::Buffer {
                176
            } else {
                0
            },
            vertex_origin: (base as i64 + min as i64) as u32,
        });
        NativeRenderPacket {
            version: PACKET_VERSION,
            label: "indexed slice".into(),
            targets: vec![Target {
                width: 96,
                height: 64,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![],
            commands: vec![Command::Draw(draw)],
        }
    }
    #[test]
    fn indexed_ranges_preserve_signed_base_and_raw_index_namespace() {
        for (base, min, source, bits) in [
            (3, 2, IndexedSource::Buffer, 16),
            (-4, 4, IndexedSource::Buffer, 16),
            (-70001, 70003, IndexedSource::Buffer, 32),
            (0, 5, IndexedSource::Up, 16),
            (0, 70001, IndexedSource::Up, 32),
        ] {
            let packet = indexed_packet(base, min, source, bits);
            packet.validate().unwrap();
            let Command::Draw(draw) = &packet.commands[0] else {
                unreachable!()
            };
            let range = draw.indexed_range.as_ref().unwrap();
            assert_eq!(range.replay_base().unwrap() as i64, -(min as i64));
            let json = serde_json::to_vec(&packet).unwrap();
            let round: NativeRenderPacket = serde_json::from_slice(&json).unwrap();
            round.validate().unwrap();
            let Command::Draw(round_draw) = &round.commands[0] else {
                unreachable!()
            };
            assert_eq!(round_draw.vertices, draw.vertices);
            assert_eq!(
                round_draw.indices.as_ref().unwrap().bytes,
                draw.indices.as_ref().unwrap().bytes
            );
            assert_eq!(
                round_draw.indexed_range.as_ref().unwrap().base_vertex_index,
                base
            );
        }
    }
    #[test]
    fn malformed_indexed_ranges_are_rejected_before_driver_calls() {
        let original = indexed_packet(-4, 4, IndexedSource::Buffer, 16);
        for mutation in 0..8 {
            let mut packet = original.clone();
            let Command::Draw(draw) = &mut packet.commands[0] else {
                unreachable!()
            };
            let range = draw.indexed_range.as_mut().unwrap();
            match mutation {
                0 => packet.version = 2,
                1 => range.vertex_origin = 1,
                2 => range.num_vertices = 5,
                3 => range.stream_offset = u32::MAX,
                4 => range.start_index = u32::MAX,
                5 => range.source = IndexedSource::Up,
                6 => draw.indices.as_mut().unwrap().bytes[0] = 3,
                7 => draw.indices = None,
                _ => unreachable!(),
            }
            assert!(
                packet.validate().is_err(),
                "mutation {mutation} escaped validation"
            );
        }
        let mut overflow = indexed_packet(0, 70001, IndexedSource::Up, 32);
        let Command::Draw(draw) = &mut overflow.commands[0] else {
            unreachable!()
        };
        let range = draw.indexed_range.as_mut().unwrap();
        range.min_vertex_index = u32::MAX;
        range.vertex_origin = u32::MAX;
        assert!(overflow.validate().is_err());
    }
    #[test]
    fn legacy_packets_and_explicit_clear_clip_state_are_distinct() {
        let mut packet = NativeRenderPacket {
            version: 1,
            label: "clip validation".into(),
            targets: vec![Target {
                width: 96,
                height: 64,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![],
            commands: vec![Command::Clear {
                target: 0,
                flags: 1,
                color: 0,
                depth_bits: 1f32.to_bits(),
                stencil: 0,
                viewport: None,
                rects: vec![],
                scissor: None,
            }],
        };
        let legacy = serde_json::to_value(&packet).unwrap();
        assert!(legacy["commands"][0].get("viewport").is_none());
        assert!(serde_json::from_value::<NativeRenderPacket>(legacy)
            .unwrap()
            .validate()
            .is_ok());
        let command = &mut packet.commands[0];
        if let Command::Clear {
            viewport,
            rects,
            scissor,
            ..
        } = command
        {
            *viewport = Some([8, 4, 80, 56, 0, 1f32.to_bits()]);
            *rects = vec![[-3, 0, 100, 80], [90, 60, 96, 64]];
            *scissor = Some([12, 8, 88, 56]);
        }
        assert!(packet.validate().unwrap_err().contains("v2 clear"));
        packet.version = PACKET_VERSION;
        assert!(packet.validate().is_ok());
        if let Command::Clear { rects, .. } = &mut packet.commands[0] {
            rects[0][2] = -4;
        }
        assert!(packet.validate().unwrap_err().contains("inverted"));
    }
    #[test]
    fn scissor_enable_cannot_depend_on_previous_draw_state() {
        let mut draw = crate::native_paint::fullscreen(
            96,
            64,
            vec![0xffff0200, 0xffff],
            vec![[1.; 4]],
            vec![],
        );
        draw.render_states.retain(|[state, _]| *state != 174);
        draw.render_states.push([174, 1]);
        let mut packet = NativeRenderPacket {
            version: PACKET_VERSION,
            label: "scissor".into(),
            targets: vec![Target {
                width: 96,
                height: 64,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![],
            commands: vec![Command::Draw(draw.clone())],
        };
        assert!(packet.validate().unwrap_err().contains("disagree"));
        draw.scissor = Some([0, 0, 48, 64]);
        packet.commands = vec![Command::Draw(draw.clone())];
        assert!(packet.validate().is_ok());
        draw.render_states.push([174, 0]);
        packet.commands = vec![Command::Draw(draw)];
        assert!(packet.validate().is_err());
    }
    #[test]
    fn compressed_mips_keep_block_rows() {
        assert_eq!(surface_layout(0x31545844, 5, 3).unwrap(), (16, 1));
        assert_eq!(surface_layout(0x35545844, 1, 1).unwrap(), (16, 1));
        let p = NativeRenderPacket {
            version: 1,
            label: String::new(),
            targets: vec![Target {
                width: 5,
                height: 3,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![Texture {
                width: 5,
                height: 3,
                format: 0x31545844,
                faces: vec![vec![Mip {
                    row_pitch: 16,
                    bytes: vec![0; 16],
                }]],
            }],
            commands: vec![],
        };
        assert!(p.validate().is_ok());
        let mut malformed = p.clone();
        malformed.textures[0].faces[0][0].bytes.pop();
        assert!(malformed.validate().is_err());
    }
    #[test]
    fn malformed_geometry_and_feedback_fail_before_driver_calls() {
        let mut draw = crate::native_paint::fullscreen(
            2,
            2,
            vec![0xffff0200, 0x0000ffff],
            vec![[1.; 4]],
            vec![],
        );
        let mut packet = NativeRenderPacket {
            version: 1,
            label: "validation".into(),
            targets: vec![Target {
                width: 2,
                height: 2,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![],
            commands: vec![Command::Draw(draw.clone())],
        };
        assert!(packet.validate().is_ok());
        draw.bindings.push(Binding {
            sampler: 0,
            source: Some(TextureSource::Target(0)),
        });
        packet.commands = vec![Command::Draw(draw.clone())];
        assert!(packet.validate().is_err());
        draw.bindings.clear();
        draw.indices = Some(IndexData {
            bits: 16,
            bytes: vec![0, 0, 1, 0, 2, 0, 4, 0],
        });
        packet.commands = vec![Command::Draw(draw.clone())];
        assert!(packet.validate().is_err());
        draw.indices = None;
        draw.vertices.pop();
        packet.commands = vec![Command::Draw(draw.clone())];
        assert!(packet.validate().is_err());
        draw.vertices.push(0);
        draw.fvf = Some(0x844);
        packet.commands = vec![Command::Draw(draw.clone())];
        assert!(packet.validate().is_err());
        draw.fvf = Some(0x8c4);
        draw.pixel_shader = Some(vec![0xfffe0200, 0xffff]);
        packet.commands = vec![Command::Draw(draw)];
        assert!(packet.validate().is_err());
    }
}
