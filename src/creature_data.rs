//! Native creaturedata file (0x0F43029A), versions 9/10. Layout follows
//! the fingerprinted installed x86 Read/ReadCompressedBuffer functions, not
//! the community reader's 68-byte properties assumption. This is decoded
//! anatomy data, not the CRT genome or a completed procedural mesh generator.
use serde::{Deserialize, Serialize};

// Installed LoadCreatureData at captured 0xDB500 substitutes this file type.
// The SDK's 0x03E1C247 TYPE identifies the class, not its package extension.
pub const TYPE: u32 = 0x0f43029a;
const MAGIC: u32 = 0xabb455b7;
const MAX_BYTES: usize = 64 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("creature data size overflow")?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| format!("truncated creature data at {} (need {count})", self.offset))?;
        self.offset = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn words(&mut self, count: usize) -> Result<Vec<u32>> {
        Ok(self
            .take(count.checked_mul(4).ok_or("word count overflow")?)?
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect())
    }
}

/// Return decoded bytes and the stream cursor after all chunks read. Native
/// behavior consumes the whole final encoded chunk, even when output fills
/// before its last token; FE at a chunk boundary is invalid, not carried over.
pub fn decode_compressed_buffer(input: &[u8], output_len: usize) -> Result<(Vec<u8>, usize)> {
    if output_len > MAX_BYTES {
        return Err("compressed output exceeds byte limit".into());
    }
    let mut cursor = Cursor {
        bytes: input,
        offset: 0,
    };
    let mut output = Vec::with_capacity(output_len);
    let mut previous = 0;
    while output.len() < output_len {
        let size = u16::from_le_bytes(cursor.take(2)?.try_into().unwrap()) as usize;
        if size == 0 || size > 0x800 {
            return Err("invalid native compressed chunk size".into());
        }
        let chunk = cursor.take(size)?;
        let mut offset = 0;
        while offset < chunk.len() && output.len() < output_len {
            let value = chunk[offset];
            offset += 1;
            if value != 0xfe {
                previous = value;
                output.push(value);
            } else {
                let repeat = *chunk
                    .get(offset)
                    .ok_or("FE escape crosses chunk boundary")?;
                offset += 1;
                if repeat == 0 {
                    previous = 0xfe;
                    output.push(previous);
                } else {
                    if repeat as usize > output_len - output.len() {
                        return Err("native compressed repeat exceeds output".into());
                    }
                    output.resize(output.len() + repeat as usize, previous);
                }
            }
        }
    }
    Ok((output, cursor.offset))
}

#[derive(Debug, Clone, Serialize)]
pub struct RigBlock {
    /// Includes native padding and unknown fields. Never convert/repack these
    /// bytes through floating-point values or a Bevy coordinate system.
    pub raw_bytes: Vec<u8>,
}
impl RigBlock {
    fn i16(&self, offset: usize) -> i16 {
        i16::from_le_bytes(self.raw_bytes[offset..offset + 2].try_into().unwrap())
    }
    fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.raw_bytes[offset..offset + 4].try_into().unwrap())
    }
    pub fn index(&self) -> i16 {
        self.i16(0)
    }
    pub fn parent_index(&self) -> i16 {
        self.i16(4)
    }
    pub fn symmetric_index(&self) -> i16 {
        self.i16(6)
    }
    pub fn flags(&self) -> u16 {
        self.i16(8) as u16
    }
    pub fn block_type(&self) -> i8 {
        self.raw_bytes[0x0b] as i8
    }
    pub fn capability_index(&self) -> i16 {
        self.i16(0x0c)
    }
    pub fn morph_index(&self) -> i16 {
        self.i16(0x0e)
    }
    pub fn capability_count(&self) -> i8 {
        self.raw_bytes[0x10] as i8
    }
    pub fn position_bits(&self) -> [u32; 3] {
        std::array::from_fn(|i| self.u32(0x54 + i * 4))
    }
    pub fn orientation_bits(&self) -> [[u32; 3]; 3] {
        std::array::from_fn(|r| std::array::from_fn(|c| self.u32(0x30 + r * 12 + c * 4)))
    }
    pub fn bounding_box_bits(&self) -> [u32; 6] {
        std::array::from_fn(|i| self.u32(0x14 + i * 4))
    }
    pub fn group_id(&self) -> u32 {
        self.u32(0x84)
    }
    pub fn instance_id(&self) -> u32 {
        self.u32(0x88)
    }
    pub fn scale_bits(&self) -> u32 {
        self.u32(0x2c)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatureData {
    pub version: u32,
    /// All 128 native property bytes, including the 60 unknown/padding bytes.
    pub properties: Vec<u8>,
    pub encoded_properties: Vec<u8>,
    pub rigblocks: Vec<RigBlock>,
    pub capability_ids: Vec<[u8; 4]>,
    pub capability_levels: Vec<i8>,
    pub animation_ids: Vec<u32>,
    pub animation_value_bits: Vec<u32>,
    pub animation_weight_bits: Vec<u32>,
    pub field_110: u32,
    pub effects: Vec<Vec<u8>>,
    /// Native Read stops at consumed_bytes. Preserve rather than silently
    /// discard container-specific trailing data or unknown extensions.
    pub consumed_bytes: usize,
    pub trailing_bytes: Vec<u8>,
}
impl CreatureData {
    pub fn model_type(&self) -> u32 {
        u32::from_le_bytes(self.properties[0..4].try_into().unwrap())
    }
    /// Return original layout, including compressed chunk padding and trailing
    /// bytes. This is a losslessness check, not an authoring/export interface.
    pub fn original_layout(&self) -> Result<Vec<u8>> {
        let (properties, consumed) = decode_compressed_buffer(&self.encoded_properties, 128)?;
        if properties != self.properties
            || consumed != self.encoded_properties.len()
            || self.capability_ids.len() != self.capability_levels.len()
            || self.animation_ids.len() != self.animation_value_bits.len()
            || self.animation_ids.len() != self.animation_weight_bits.len()
            || !(self.version == 9 || self.version == 10)
            || (self.version == 9 && !self.effects.is_empty())
        {
            return Err("creature data no longer matches its original layout".into());
        }
        let mut out = vec![];
        for word in [
            MAGIC,
            self.version,
            self.rigblocks.len() as u32,
            self.capability_ids.len() as u32,
            self.animation_ids.len() as u32,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        if self.version == 10 {
            out.extend_from_slice(&(self.effects.len() as u32).to_le_bytes());
        }
        out.extend_from_slice(&self.encoded_properties);
        for block in &self.rigblocks {
            if block.raw_bytes.len() != 140 {
                return Err("invalid raw rig block length".into());
            }
            out.extend_from_slice(&block.raw_bytes);
        }
        for tag in &self.capability_ids {
            out.extend_from_slice(tag);
        }
        out.extend(self.capability_levels.iter().map(|v| *v as u8));
        for words in [
            &self.animation_ids,
            &self.animation_value_bits,
            &self.animation_weight_bits,
        ] {
            for word in words {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out.extend_from_slice(&self.field_110.to_le_bytes());
        for effect in &self.effects {
            if effect.len() != 56 {
                return Err("invalid raw effect length".into());
            }
            out.extend_from_slice(effect);
        }
        if out.len() != self.consumed_bytes {
            return Err("original consumed-byte count changed".into());
        }
        out.extend_from_slice(&self.trailing_bytes);
        Ok(out)
    }
    /// Validate references separately from byte parsing: native Read accepts
    /// structurally invalid/placeholder rig blocks. Do not invent a remapping
    /// of indices or silently clamp capabilities during import.
    pub fn validate_anatomy(&self) -> Result<()> {
        let mut indices = std::collections::BTreeMap::new();
        for (slot, b) in self.rigblocks.iter().enumerate() {
            if b.index() < 0 || indices.insert(b.index(), slot).is_some() {
                return Err(format!("negative/duplicate rig block index at slot {slot}"));
            }
            let start = b.capability_index();
            let count = b.capability_count();
            if count < 0
                || (count > 0
                    && (start < 0 || start as usize + count as usize > self.capability_ids.len()))
            {
                return Err(format!(
                    "invalid capability range at rig block {}",
                    b.index()
                ));
            }
            // This is a range START, not an element reference. Native records
            // use the vector end for an empty range (including zero at len 0).
            if b.morph_index() < -1 || b.morph_index() as i32 > self.animation_ids.len() as i32 {
                return Err(format!("invalid morph start at rig block {}", b.index()));
            }
        }
        for b in &self.rigblocks {
            for reference in [b.parent_index(), b.symmetric_index()] {
                if reference < -1 || (reference >= 0 && !indices.contains_key(&reference)) {
                    return Err(format!(
                        "unknown rig reference {reference} at {}",
                        b.index()
                    ));
                }
            }
            let mut parent = b.parent_index();
            for depth in 0..=self.rigblocks.len() {
                if parent == -1 {
                    break;
                }
                if parent == b.index() || depth == self.rigblocks.len() {
                    return Err(format!("cyclic rig parent at {}", b.index()));
                }
                parent = self.rigblocks[indices[&parent]].parent_index();
            }
        }
        Ok(())
    }

    /// Offsets within the 128-byte decoded `properties` buffer for the native
    /// SkinPaint palette (`cEditorResourceProperties`, SIZE 0x80):
    /// `mSkinColor1 @ 0x20`, `mSkinColor2 @ 0x2c`, `mSkinColor3 @ 0x38`.
    /// Each is 3 little-endian floats (`Math::ColorRGB`).
    pub const SKIN_COLOR_1: usize = 0x20;
    pub const SKIN_COLOR_2: usize = 0x2c;
    pub const SKIN_COLOR_3: usize = 0x38;
    /// Byte length of one `ColorRGB` (3 f32 LE).
    const COLOR_RGB_LEN: usize = 12;
    /// Properties must cover all three color blocks (0x20..0x44).
    const SKIN_COLORS_END: usize = Self::SKIN_COLOR_3 + Self::COLOR_RGB_LEN;

    /// Read the native starter paint palette from the decoded 128-byte
    /// properties buffer. Returns three native RGB triples, each f32
    /// validated finite and in 0..=1. The native color transfer function is not established here; no color-space conversion is applied.
    pub fn skin_colors(&self) -> Result<[[f32; 3]; 3]> {
        if self.properties.len() < Self::SKIN_COLORS_END {
            return Err(format!(
                "creaturedata properties {} bytes too short for skin colors (need {})",
                self.properties.len(),
                Self::SKIN_COLORS_END
            ));
        }
        let read = |off: usize| -> Result<f32> {
            let bytes = self
                .properties
                .get(off..off + 4)
                .ok_or_else(|| format!("properties range overflow at {off:#x}"))?;
            let v = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(format!("skin color at {off:#x} is {v}, not finite 0..=1"));
            }
            Ok(v)
        };
        Ok([
            [
                read(Self::SKIN_COLOR_1)?,
                read(Self::SKIN_COLOR_1 + 4)?,
                read(Self::SKIN_COLOR_1 + 8)?,
            ],
            [
                read(Self::SKIN_COLOR_2)?,
                read(Self::SKIN_COLOR_2 + 4)?,
                read(Self::SKIN_COLOR_2 + 8)?,
            ],
            [
                read(Self::SKIN_COLOR_3)?,
                read(Self::SKIN_COLOR_3 + 4)?,
                read(Self::SKIN_COLOR_3 + 8)?,
            ],
        ])
    }

    /// Quantize validated native RGB floats to u8 for the existing
    /// skinpaint API (`native_part_tint` / `preview_part` take `[[u8;3];3]`
    /// and divide by 255). This is **quantization adaptation only**, not
    /// color-space conversion. `/255` approximately restores the input values,
    /// without establishing a color transfer curve. Preserve the floats when a
    /// float-based path is available instead.
    pub fn quantize_skin_colors(linear: [[f32; 3]; 3]) -> [[u8; 3]; 3] {
        linear.map(|c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
    }
}

pub fn parse(input: &[u8]) -> Result<CreatureData> {
    if input.len() > MAX_BYTES {
        return Err("creature data exceeds byte limit".into());
    }
    let mut cursor = Cursor {
        bytes: input,
        offset: 0,
    };
    if cursor.u32()? != MAGIC {
        return Err("invalid native creature data magic".into());
    }
    let version = cursor.u32()?;
    if version != 9 && version != 10 {
        return Err(format!(
            "unsupported native creature data version {version}"
        ));
    }
    let blocks = cursor.u32()? as usize;
    let capabilities = cursor.u32()? as usize;
    let animations = cursor.u32()? as usize;
    let effects = if version == 10 {
        cursor.u32()? as usize
    } else {
        0
    };
    if blocks > 256 || capabilities > 4096 || animations > 1024 {
        return Err("native creature data count exceeds native reader limit".into());
    }
    let properties_start = cursor.offset;
    let (properties, encoded_size) = decode_compressed_buffer(&input[properties_start..], 128)?;
    let encoded_properties = cursor.take(encoded_size)?.to_vec();
    // Native effect count is uncapped. Check all required bytes BEFORE reserve
    // or allocating vectors; malformed counts must not cause enormous mallocs.
    let required = blocks
        .checked_mul(140)
        .and_then(|n| capabilities.checked_mul(5).and_then(|v| n.checked_add(v)))
        .and_then(|n| animations.checked_mul(12).and_then(|v| n.checked_add(v)))
        .and_then(|n| effects.checked_mul(56).and_then(|v| n.checked_add(v)))
        .and_then(|n| n.checked_add(4))
        .ok_or("native creature data count overflow")?;
    if required > input.len() - cursor.offset {
        return Err("truncated native creature data arrays".into());
    }
    let rigblocks = cursor
        .take(blocks * 140)?
        .chunks_exact(140)
        .map(|b| RigBlock {
            raw_bytes: b.to_vec(),
        })
        .collect();
    let capability_ids = cursor
        .take(capabilities * 4)?
        .chunks_exact(4)
        .map(|b| b.try_into().unwrap())
        .collect();
    let capability_levels = cursor
        .take(capabilities)?
        .iter()
        .map(|b| *b as i8)
        .collect();
    let animation_ids = cursor.words(animations)?;
    let animation_value_bits = cursor.words(animations)?;
    let animation_weight_bits = cursor.words(animations)?;
    let field_110 = cursor.u32()?;
    let effects = cursor
        .take(effects * 56)?
        .chunks_exact(56)
        .map(|b| b.to_vec())
        .collect();
    Ok(CreatureData {
        version,
        properties,
        encoded_properties,
        rigblocks,
        capability_ids,
        capability_levels,
        animation_ids,
        animation_value_bits,
        animation_weight_bits,
        field_110,
        effects,
        consumed_bytes: cursor.offset,
        trailing_bytes: input[cursor.offset..].to_vec(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompressionOracle {
    version: u32,
    image_sha256: String,
    entry: u32,
    engine: String,
    replacements: Vec<String>,
    cases: Vec<CompressionCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompressionCase {
    name: String,
    input: Vec<u8>,
    output_len: usize,
    accepted: bool,
    consumed: usize,
    output: Vec<u8>,
}
#[derive(Serialize)]
pub struct CompressionComparison {
    pub version: u32,
    pub kind: &'static str,
    pub image_sha256: String,
    pub compared_cases: usize,
    pub accepted_cases: usize,
    pub compared_bytes: usize,
    pub passed: bool,
    pub first_divergence: Option<String>,
}
pub fn compare_compression(oracle: CompressionOracle) -> Result<CompressionComparison> {
    if oracle.version != 1
        || oracle.image_sha256 != "72931d2180bf90ed265c32eb790f100a7257717b3e7e94831e69ec5934383be9"
        || oracle.entry != 0xdf7e0
        || oracle.engine != "Unicorn 2.1.4 x86"
        || oracle.replacements != ["IStream::Read synthetic bounded stream", "CRT memset"]
        || oracle.cases.is_empty()
    {
        return Err("invalid installed compressed-reader oracle identity".into());
    }
    let mut report = CompressionComparison {
        version: 1,
        kind: "NativeCompressedBufferProbe",
        image_sha256: oracle.image_sha256,
        compared_cases: 0,
        accepted_cases: 0,
        compared_bytes: 0,
        passed: true,
        first_divergence: None,
    };
    for case in oracle.cases {
        if case.name.is_empty()
            || case.output_len > 4096
            || case.input.len() > MAX_BYTES
            || case.consumed > case.input.len()
            || (case.accepted && case.output.len() != case.output_len)
            || (!case.accepted && !case.output.is_empty())
        {
            return Err("invalid compressed-reader oracle case".into());
        }
        let actual = decode_compressed_buffer(&case.input, case.output_len);
        let divergence = match actual {
            Ok(_) if !case.accepted => Some("accepted native-rejected stream".into()),
            Ok((output, _)) if output != case.output => Some(format!(
                "output byte {}",
                output
                    .iter()
                    .zip(&case.output)
                    .position(|(a, b)| a != b)
                    .unwrap_or(output.len().min(case.output.len()))
            )),
            Ok((_, consumed)) if consumed != case.consumed => {
                Some(format!("cursor {consumed}, expected {}", case.consumed))
            }
            Err(error) if case.accepted => {
                Some(format!("rejected native-accepted stream: {error}"))
            }
            _ => None,
        };
        report.compared_cases += 1;
        if case.accepted {
            report.accepted_cases += 1;
            report.compared_bytes += case.output.len();
        }
        if let Some(divergence) = divergence {
            report.passed = false;
            if report.first_divergence.is_none() {
                report.first_divergence = Some(format!("{}: {divergence}", case.name));
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(version: u32) -> Vec<u8> {
        let mut bytes = vec![];
        for v in [MAGIC, version, 2, 2, 1] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        if version == 10 {
            bytes.extend_from_slice(&1u32.to_le_bytes());
        }
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&[7, 0xfe, 127, 0xee]);
        for i in 0..2i16 {
            let mut b = vec![0; 140];
            b[0..2].copy_from_slice(&i.to_le_bytes());
            b[4..6].copy_from_slice(&(i - 1).to_le_bytes());
            b[6..8].copy_from_slice(&(1 - i).to_le_bytes());
            b[0xc..0xe].copy_from_slice(&i.to_le_bytes());
            b[0xe..0x10].copy_from_slice(&(-1i16).to_le_bytes());
            b[0x10] = 1;
            b[0x54..0x58].copy_from_slice(&0x80000000u32.to_le_bytes());
            b[0x58..0x5c].copy_from_slice(&0x7fc12345u32.to_le_bytes());
            b[0x84..0x88].copy_from_slice(&0x40626000u32.to_le_bytes());
            b[0x88..0x8c].copy_from_slice(&0x12345678u32.to_le_bytes());
            bytes.extend(b);
        }
        bytes.extend_from_slice(b"footwing");
        bytes.extend_from_slice(&[1, 0xff]);
        for word in [42u32, 0x80000000, 0x7fc12345, 17] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if version == 10 {
            bytes.extend(0..56);
        }
        bytes
    }
    #[test]
    fn both_versions_preserve_every_byte_and_float_bits() {
        for version in [9, 10] {
            let mut bytes = fixture(version);
            let consumed = bytes.len();
            bytes.extend_from_slice(b"unknown extension");
            let data = parse(&bytes).unwrap();
            assert_eq!(data.properties, vec![7; 128]);
            assert_eq!(data.consumed_bytes, consumed);
            assert_eq!(
                data.rigblocks[0].position_bits(),
                [0x80000000, 0x7fc12345, 0]
            );
            assert_eq!(data.rigblocks[1].parent_index(), 0);
            assert_eq!(data.rigblocks[0].symmetric_index(), 1);
            assert_eq!(data.rigblocks[0].group_id(), 0x40626000);
            assert_eq!(data.rigblocks[0].instance_id(), 0x12345678);
            assert_eq!(data.capability_ids, vec![*b"foot", *b"wing"]);
            assert_eq!(data.capability_levels, vec![1, -1]);
            assert_eq!(data.animation_weight_bits, vec![0x7fc12345]);
            data.validate_anatomy().unwrap();
            assert_eq!(data.original_layout().unwrap(), bytes);
        }
    }
    #[test]
    fn native_chunk_boundaries_escape_and_initial_previous() {
        assert_eq!(
            decode_compressed_buffer(&[2, 0, 0xfe, 3], 3).unwrap(),
            (vec![0; 3], 4)
        );
        assert_eq!(
            decode_compressed_buffer(&[2, 0, 0xfe, 0, 2, 0, 0xfe, 3], 4).unwrap(),
            (vec![0xfe; 4], 8)
        );
        assert_eq!(
            decode_compressed_buffer(&[3, 0, 4, 0xfe, 0xff], 1).unwrap(),
            (vec![4], 5)
        );
        for input in [
            &[0, 0][..],
            &[1, 8],
            &[1, 0, 0xfe, 1, 0, 0],
            &[2, 0, 0xfe, 129],
            &[2, 0, 1],
        ] {
            assert!(decode_compressed_buffer(input, 128).is_err());
        }
        assert_eq!(decode_compressed_buffer(&[], 0).unwrap(), (vec![], 0));
    }
    #[test]
    fn all_required_truncations_and_native_count_caps_rejected() {
        let bytes = fixture(10);
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_err(), "end {end}");
        }
        for (offset, value) in [
            (4, 8),
            (4, 11),
            (8, 257),
            (12, 4097),
            (16, 1025),
            (20, u32::MAX),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bad).is_err());
        }
    }
    #[test]
    fn anatomy_diagnostics_do_not_change_or_prevent_lossless_read() {
        let bytes = fixture(10);
        let mut data = parse(&bytes).unwrap();
        data.rigblocks[0].raw_bytes[4..6].copy_from_slice(&1i16.to_le_bytes());
        assert!(data.validate_anatomy().unwrap_err().contains("cyclic"));
        let bytes = data.original_layout().unwrap();
        assert!(parse(&bytes).is_ok());
        data.rigblocks[1].raw_bytes[0xc..0xe].copy_from_slice(&4096i16.to_le_bytes());
        assert!(data.validate_anatomy().unwrap_err().contains("capability"));
    }
    #[test]
    fn empty_morph_range_can_start_at_vector_end() {
        let mut data = parse(&fixture(10)).unwrap();
        data.rigblocks[0].raw_bytes[0xe..0x10].copy_from_slice(&1i16.to_le_bytes());
        data.validate_anatomy().unwrap();
        data.rigblocks[0].raw_bytes[0xe..0x10].copy_from_slice(&2i16.to_le_bytes());
        assert!(data.validate_anatomy().unwrap_err().contains("morph start"));
    }

    // ---- skin_colors pure offset / validation tests ----

    fn skin_properties() -> Vec<u8> {
        // 128 bytes: offsets 0x20-0x43 hold three ColorRGB LE-float triples.
        let mut p = vec![0u8; 128];
        // Color1 @ 0x20: [0.25, 0.5, 0.75]
        p[0x20..0x24].copy_from_slice(&0.25f32.to_le_bytes());
        p[0x24..0x28].copy_from_slice(&0.5f32.to_le_bytes());
        p[0x28..0x2c].copy_from_slice(&0.75f32.to_le_bytes());
        // Color2 @ 0x2c: [0.1, 0.2, 0.3]
        p[0x2c..0x30].copy_from_slice(&0.1f32.to_le_bytes());
        p[0x30..0x34].copy_from_slice(&0.2f32.to_le_bytes());
        p[0x34..0x38].copy_from_slice(&0.3f32.to_le_bytes());
        // Color3 @ 0x38: [1.0, 0.0, 0.0031308]
        p[0x38..0x3c].copy_from_slice(&1.0f32.to_le_bytes());
        p[0x3c..0x40].copy_from_slice(&0.0f32.to_le_bytes());
        p[0x40..0x44].copy_from_slice(&0.0031308f32.to_le_bytes());
        p
    }

    #[test]
    fn skin_colors_reads_native_rgb_from_property_offsets() {
        let data = CreatureData {
            version: 9,
            properties: skin_properties(),
            encoded_properties: vec![],
            rigblocks: vec![],
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        };
        let colors = data.skin_colors().unwrap();
        assert_eq!(colors[0], [0.25, 0.5, 0.75]);
        assert_eq!(colors[1], [0.1, 0.2, 0.3]);
        assert_eq!(colors[2], [1.0, 0.0, 0.0031308]);
    }

    #[test]
    fn skin_colors_rejects_short_properties() {
        let data = CreatureData {
            version: 9,
            properties: vec![0u8; 0x43], // one byte short
            encoded_properties: vec![],
            rigblocks: vec![],
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        };
        assert!(data.skin_colors().is_err());
    }

    #[test]
    fn skin_colors_rejects_nonfinite_and_out_of_range() {
        let mut p = skin_properties();
        // NaN at Color1 R (0x20)
        p[0x20..0x24].copy_from_slice(&f32::NAN.to_le_bytes());
        let data = CreatureData {
            version: 9,
            properties: p,
            encoded_properties: vec![],
            rigblocks: vec![],
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        };
        assert!(data.skin_colors().is_err());

        // >1.0 at Color2 G (0x30)
        let mut p = skin_properties();
        p[0x30..0x34].copy_from_slice(&1.5f32.to_le_bytes());
        let data = CreatureData {
            version: 9,
            properties: p,
            encoded_properties: vec![],
            rigblocks: vec![],
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        };
        assert!(data.skin_colors().is_err());
    }

    #[test]
    fn quantize_skin_colors_converts_known_values() {
        // Direct quantization: 0.0 -> 0, 1.0 -> 255, 0.18 -> 46, 0.5 -> 128, 0.75 -> 191.
        let linear = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.18, 0.5, 0.75]];
        let u8 = CreatureData::quantize_skin_colors(linear);
        assert_eq!(u8[0], [0, 0, 0]);
        assert_eq!(u8[1], [255, 255, 255]);
        assert_eq!(u8[2], [46, 128, 191]);
    }
}
