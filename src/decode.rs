//! Decoders for identified Spore resource types (all empirically probed
//! against retail packages — see `--probe-type`):
//!
//! | type_id    | kind  | evidence |
//! |------------|-------|----------|
//! | 0x00B1B104 | PROP  | `B2 CC CA/CB` marker, UTF-16LE text, LE resource refs |
//! | 0x021407EE | audio | audio-package-only, MB sizes, entropy ~7.9 |
//! | 0x02B9F662 | audio meta | tiny, grouped under audio type |
//! | 0x2F4E681B | RW4 model | `89 52 57 34` (".RW4") magic |
//! | 0x2F7D0004 | PNG | raw `89 50 4E 47` IHDR |
//! | 0x2F7D0002 | JPEG | extension map "jpeg" |
//! | 0xEE17C6AD | ANIM | `"ANIM"` + `c:\spore\...\*.animation` paths |
//! | 0x02FAC0B6 | locale text | UTF-8 BOM + `# file` / `0xID text` tables |
//! | 0x0461227D | namegen | UTF-8 BOM + CRLF word lists (planets, people) |
//! | 0x2F4E681C | raster | "DXT5"/DXT GPU textures ("raster") |
//! | 0x00E6BCE5 | gmdl | model assembly referencing RW4 + raster |
//! | 0x01C135DA | gmsh | "gmsh" mesh |
//! | 0x030BDEE3 | pollen metadata | extension map "pollen_metadata" |
//! | 0x01AD2416/17/18 | traits | creature/building/vehicle traits |
//! | 0x02D5C9AF/B0 | summary | "summary"/"summary_pill" |
//!
//! PNGs are raw and loadable as-is. PROP strings (UTF-16LE runs) give us
//! original game text. RW4/ANIM get header parsers; full mesh/skinning
//! playback is roadmap (documented below).

use std::path::Path;

use crate::dbpf;

pub const TYPE_PROP: u32 = 0x00B1_B104;
pub const TYPE_AUDIO: u32 = 0x0214_07EE;
pub const TYPE_AUDIO_META: u32 = 0x02B9_F662;
pub const TYPE_RW4_MODEL: u32 = 0x2F4E_681B;
pub const TYPE_PNG: u32 = 0x2F7D_0004;
pub const TYPE_JPEG: u32 = 0x2F7D_0002;
pub const TYPE_ANIM: u32 = 0xEE17_C6AD;
pub const TYPE_LOCALE_TEXT: u32 = 0x2FAC_0B6;
pub const TYPE_NAMEGEN: u32 = 0x0461_227D;
pub const TYPE_RASTER: u32 = 0x2F4E_681C; // "raster": DXT1/DXT5 GPU textures
pub const TYPE_GMDL: u32 = 0x00E6_BCE5; // "gmdl": model assembly (refs RW4 + raster)
pub const TYPE_GMSH: u32 = 0x01C1_35DA; // "gmsh"
pub const TYPE_RW4_MESH: u32 = 0x2F4E_681B; // ".RW4" geometry (distinct from raster)

pub const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
pub const RW4_MAGIC: &[u8] = &[0x89, b'R', b'W', b'4'];

#[derive(Debug, Clone)]
pub struct PngRecord {
    pub group_id: u32,
    pub instance_id: u32,
    pub package: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// Collect up to `limit` raw PNG records (stored uncompressed — no QFS cost).
/// Returns metadata + raw bytes for the first `with_bytes` of them.
pub fn collect_pngs(
    install: &crate::spore_install::SporeInstall,
    limit: usize,
    with_bytes: usize,
) -> Vec<(PngRecord, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    for pkg in &install.packages {
        if out.len() >= limit {
            break;
        }
        let pname = pkg
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index.entries.iter().filter(|e| e.key.type_id == TYPE_PNG) {
            if out.len() >= limit {
                break;
            }
            let bytes = if out.len() < with_bytes {
                dbpf::read_entry_bytes(pkg, e).ok()
            } else {
                None
            };
            let (w, h) = bytes
                .as_ref()
                .and_then(|b| parse_png_dims(b))
                .unwrap_or((0, 0));
            // If we need dims but skipped bytes, read dims only (cheap: PNGs are raw).
            let (w, h) = if w == 0 {
                read_png_dims_only(pkg, e).unwrap_or((0, 0))
            } else {
                (w, h)
            };
            out.push((
                PngRecord {
                    group_id: e.key.group_id,
                    instance_id: e.key.instance_id,
                    package: pname.clone(),
                    width: w,
                    height: h,
                    bytes: e.mem_size as usize,
                },
                bytes,
            ));
        }
    }
    out
}

fn read_png_dims_only(pkg: &Path, e: &dbpf::DbpfEntry) -> Option<(u32, u32)> {
    // PNGs are stored raw: IHDR width/height live at fixed offsets 16..24.
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(pkg).ok()?;
    f.seek(SeekFrom::Start(e.offset as u64 + 16)).ok()?;
    let mut b = [0u8; 8];
    f.read_exact(&mut b).ok()?;
    Some((
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
    ))
}

/// Parse PNG width/height from IHDR (bytes 16..24, big-endian).
pub fn parse_png_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[0..8] != PNG_MAGIC || &b[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes([b[16], b[17], b[18], b[19]]),
        u32::from_be_bytes([b[20], b[21], b[22], b[23]]),
    ))
}

/// One decoded animation channel: its name, bind context, and the position /
/// rotation keyframe streams we can use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimChannel {
    pub name: String,
    /// primaryContext.flags (select flags: left/right, front/back, top/bottom).
    pub select_flags: u32,
    /// primaryContext.capability (4-char tag, e.g. "root").
    pub capability: String,
    /// Rotation keyframes (unit quaternions, xyzw).
    pub rot: Vec<[f32; 4]>,
    /// Position keyframes (xyz).
    pub pos: Vec<[f32; 3]>,
}

/// A decoded standalone `.animation` container (type 0xEE17C6AD). Layout per
/// SporeModder-FX's `SPAnimation`: header at 0x110 (id), 0x11C (1/30 frame),
/// 0x120 (length), 0x13C/0x140 (events), 0x144/0x148 (channel count/table);
/// each channel is a `"CHAN"` chunk with a name padded to 0x80, contexts at
/// 0x8C/0x9C, and at 0xD4/0xD8/0xDC/0xE0 the keyframe count, keyframe data
/// pointer, component count and 32-byte component metadata array; a rot
/// component (type 2) keyframe is 16 bytes of quaternion + weight.
pub fn parse_anim_clip(blob: &[u8]) -> Option<AnimClip> {
    if blob.len() < 0x160 || &blob[0..4] != b"ANIM" {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
    let version = u32_at(8);
    if !(0x14..=0x19).contains(&version) {
        return None;
    }
    let length = f32::from_le_bytes([blob[0x120], blob[0x121], blob[0x122], blob[0x123]]);
    let channel_count = u32_at(0x144) as usize;
    let channel_table = u32_at(0x148) as usize;
    if channel_count == 0 || channel_count > 1024 || channel_table + 4 * channel_count > blob.len()
    {
        return None;
    }
    let mut channels = Vec::with_capacity(channel_count);
    for i in 0..channel_count {
        let co = u32_at(channel_table + i * 4) as usize;
        let ch = parse_anim_channel(blob, co)?;
        channels.push(ch);
    }
    Some(AnimClip { length, channels })
}

/// A decoded clip: length (seconds) plus per-channel keyframe streams.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimClip {
    pub length: f32,
    pub channels: Vec<AnimChannel>,
}

/// Parse one `"CHAN"` chunk at `co` into name + rot/pos keyframes.
fn parse_anim_channel(blob: &[u8], co: usize) -> Option<AnimChannel> {
    if co + 0xE4 > blob.len() || &blob[co..co + 4] != b"CHAN" {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
    // Name: ASCII after the magic+pointer, padded to 0x80.
    let start = co + 8;
    let mut end = start;
    while end < blob.len() && end - start < 0x80 && blob[end] != 0 {
        end += 1;
    }
    let name = String::from_utf8_lossy(&blob[start..end]).into_owned();
    let select_flags = u32_at(co + 0x8C);
    let capability = String::from_utf8_lossy(
        &blob[co + 0x90..(co + 0x94).min(blob.len())]
            .iter()
            .copied()
            .take_while(|b| *b != 0)
            .collect::<Vec<u8>>(),
    )
    .into_owned();
    let keyframe_count = u32_at(co + 0xD4) as usize;
    let keyframe_ptr = u32_at(co + 0xD8) as usize;
    let comp_count = u32_at(co + 0xDC) as usize;
    let comp_ptr = u32_at(co + 0xE0) as usize;
    let mut ch = AnimChannel {
        name,
        select_flags,
        capability,
        rot: Vec::new(),
        pos: Vec::new(),
    };
    if keyframe_count == 0 || keyframe_count > 100_000 {
        return Some(ch);
    }
    for i in 0..comp_count {
        let meta = comp_ptr + i * 32;
        if meta + 20 > blob.len() {
            break;
        }
        let flags = u32_at(meta);
        let comp_type = flags & 0xF;
        let key_offset = u32_at(meta + 8) as usize;
        let stride = u32_at(meta + 12) as usize;
        if stride == 0 || key_offset + stride * keyframe_count > 0x4000_0000 {
            continue;
        }
        for j in 0..keyframe_count {
            let k = keyframe_ptr + key_offset + stride * j;
            match comp_type {
                2 => {
                    // rot: 16-byte quaternion + f32 weight.
                    if k + 20 <= blob.len() {
                        let q = f32le4(blob, k);
                        if q.iter().all(|v| v.is_finite()) {
                            ch.rot.push(q);
                        }
                    }
                }
                1 => {
                    // pos: 12-byte vector + weight.
                    if k + 16 <= blob.len() {
                        let p = f32le3(blob, k);
                        if p.iter().all(|v| v.is_finite()) {
                            ch.pos.push(p);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Some(ch)
}

fn f32le3(b: &[u8], o: usize) -> [f32; 3] {
    [
        f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]),
        f32::from_le_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]),
        f32::from_le_bytes([b[o + 8], b[o + 9], b[o + 10], b[o + 11]]),
    ]
}

fn f32le4(b: &[u8], o: usize) -> [f32; 4] {
    [
        f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]),
        f32::from_le_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]),
        f32::from_le_bytes([b[o + 8], b[o + 9], b[o + 10], b[o + 11]]),
        f32::from_le_bytes([b[o + 12], b[o + 13], b[o + 14], b[o + 15]]),
    ]
}

/// Sample a position keyframe stream at normalized time `t` ∈ [0,1]:
/// keys are spread evenly across the clip (linear between neighbours).
/// Returns None for empty streams.
pub fn sample_vec3(keys: &[[f32; 3]], t: f32) -> Option<[f32; 3]> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() == 1 {
        return Some(keys[0]);
    }
    let n = keys.len() - 1;
    let x = (t.clamp(0.0, 1.0) * n as f32).floor() as usize;
    let i = x.min(n - 1);
    let f = (t.clamp(0.0, 1.0) * n as f32) - i as f32;
    let a = keys[i];
    let b = keys[i + 1];
    Some([
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ])
}

/// Sample a rotation keyframe stream at normalized time `t` ∈ [0,1] with
/// normalized-lerp between neighbours (robust to non-unit input).
pub fn sample_quat(keys: &[[f32; 4]], t: f32) -> Option<[f32; 4]> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() == 1 {
        return Some(keys[0]);
    }
    let n = keys.len() - 1;
    let scaled = t.clamp(0.0, 1.0) * n as f32;
    let i = (scaled.floor() as usize).min(n - 1);
    let f = scaled - i as f32;
    let mut a = keys[i];
    let b = keys[i + 1];
    // Shortest path.
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        a = [-a[0], -a[1], -a[2], -a[3]];
    }
    let mut q = [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
        a[3] + (b[3] - a[3]) * f,
    ];
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len > 1e-6 {
        for v in q.iter_mut() {
            *v /= len;
        }
    }
    Some(q)
}

/// Decode an animation channel's primary selector into a readable
/// (capability, side) pair, e.g. `("foot", "right")`. Flags per
/// SporeModder-FX `AnimationChannel`: SELECT_TYPE_ROOT=1, CAP=0x100003,
/// LEFT=4/RIGHT=8/CENTER=0xC and the 0x2000x "local" variants.
pub fn channel_selector(ch: &AnimChannel) -> (String, String) {
    let f = ch.select_flags;
    let sb = f & 0x2000C;
    let side = if sb & 0x2000_0 != 0 {
        // "local" variants (bit 0x20000 set).
        match sb {
            0x20004 => "right2",
            0x20000 => "left2",
            0x20008 => "middle2",
            _ => "?",
        }
    } else if sb & 0xC != 0 {
        match sb {
            8 => "right",
            4 => "left",
            12 => "middle",
            _ => "?",
        }
    } else {
        ""
    };
    let cap = if f & 0x1_0003 == 1 {
        "root".to_string()
    } else if !ch.capability.is_empty() {
        ch.capability.clone()
    } else {
        String::new()
    };
    (cap, side.to_string())
}

/// Capability/side census over an install's animation catalog (bounded scan):
/// `(capability -> count, (capability, side) -> count)`.
pub fn anim_selector_census(
    install: &crate::spore_install::SporeInstall,
    budget: usize,
) -> (
    std::collections::HashMap<String, usize>,
    std::collections::HashMap<(String, String), usize>,
) {
    use std::collections::HashMap;
    let mut caps: HashMap<String, usize> = HashMap::new();
    let mut sides: HashMap<(String, String), usize> = HashMap::new();
    let mut seen = 0usize;
    'outer: for pkg in &install.packages {
        let Ok(index) = crate::dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index.entries.iter().filter(|e| e.key.type_id == TYPE_ANIM) {
            seen += 1;
            if seen > budget {
                break 'outer;
            }
            let Ok(bytes) = crate::dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            // Cheap pre-check: only full clips parse.
            let Some(clip) = parse_anim_clip(&bytes) else {
                continue;
            };
            for ch in &clip.channels {
                let (cap, side) = channel_selector(ch);
                *caps.entry(cap.clone()).or_insert(0) += 1;
                *sides.entry((cap, side)).or_insert(0) += 1;
            }
        }
    }
    (caps, sides)
}

/// The build path stored in a standalone `.animation` container (the file's
/// original source path, e.g. `c:\spore\...\creature\idle.animation`).
pub fn anim_path(blob: &[u8]) -> String {
    if blob.len() < 16 || &blob[0..4] != b"ANIM" {
        return String::new();
    }
    // Path starts at 0x0C and is NUL-terminated ASCII.
    let mut end = 12usize;
    while end < blob.len() && end < 12 + 256 && blob[end] != 0 {
        end += 1;
    }
    String::from_utf8_lossy(&blob[12..end]).into_owned()
}

/// Channel names from a standalone `.animation` container (type 0xEE17C6AD).
/// Layout: `"ANIM"` magic, a build path, then a header whose channel-count /
/// channel-table fields point at `"CHAN"` chunks, each carrying a
/// NUL-terminated ASCII channel name (e.g. `root`, `L foot`, `MouthMorphs`).
/// Returns the names in table order; empty when the layout doesn't validate.
pub fn parse_anim_channels(blob: &[u8]) -> Vec<String> {
    if blob.len() < 64 || &blob[0..4] != b"ANIM" {
        return Vec::new();
    }
    // Find the header: a u32 == 1 followed by [bodySize, channelCount,
    // channelTableOffset, ...]; validate that the table's first entry points
    // at a "CHAN" chunk.
    let mut header = None;
    let mut o = 12usize;
    while o + 20 <= blob.len() && o <= 512 {
        let v = u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
        if v == 1 {
            let count = u32::from_le_bytes([blob[o + 8], blob[o + 9], blob[o + 10], blob[o + 11]]);
            let table = u32::from_le_bytes([blob[o + 12], blob[o + 13], blob[o + 14], blob[o + 15]])
                as usize;
            if (2..=256).contains(&count)
                && table >= 8
                && table + (count as usize) * 4 <= blob.len()
            {
                let first = u32::from_le_bytes([
                    blob[table],
                    blob[table + 1],
                    blob[table + 2],
                    blob[table + 3],
                ]) as usize;
                if first + 8 <= blob.len() && &blob[first..first + 4] == b"CHAN" {
                    header = Some((count as usize, table));
                    break;
                }
            }
        }
        o += 4;
    }
    let Some((count, table)) = header else {
        return Vec::new();
    };
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let co = u32::from_le_bytes([
            blob[table + i * 4],
            blob[table + i * 4 + 1],
            blob[table + i * 4 + 2],
            blob[table + i * 4 + 3],
        ]) as usize;
        if co + 8 > blob.len() || &blob[co..co + 4] != b"CHAN" {
            return Vec::new();
        }
        // NUL-terminated ASCII name after the 4-byte tag + 4 bytes padding.
        let start = co + 8;
        let mut end = start;
        while end < blob.len() && end - start < 64 && blob[end] != 0 {
            end += 1;
        }
        if end == start {
            return Vec::new();
        }
        names.push(String::from_utf8_lossy(&blob[start..end]).into_owned());
    }
    names
}

/// Extract UTF-16LE string runs (len >= `min_chars`, mostly printable) from a
/// decoded PROP blob. Returns owned Strings (lossy for the odd codepoint).
pub fn extract_utf16_strings(blob: &[u8], min_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut i = 0;
    // Step by 2 over LE pairs; resync on odd offsets by also trying i+1 starts.
    while i + 1 < blob.len() {
        let unit = u16::from_le_bytes([blob[i], blob[i + 1]]);
        let printable = unit == 0x0A
            || unit == 0x0D
            || unit == 0x09
            || (0x20..0x7F).contains(&(unit as u32))
            || (0xA0..0x2500).contains(&(unit as u32));
        if printable && unit != 0 {
            cur.push(unit);
            i += 2;
        } else {
            if cur.len() >= min_chars {
                out.push(String::from_utf16_lossy(&cur));
            }
            cur.clear();
            // If we hit a lone zero inside text (UTF-16 NUL), skip it; else step 1 to resync.
            i += if unit == 0 { 2 } else { 1 };
        }
    }
    if cur.len() >= min_chars {
        out.push(String::from_utf16_lossy(&cur));
    }
    out
}

#[derive(Debug, Clone)]
pub struct Rw4Header {
    pub magic: [u8; 4],
    pub version: u32,
    pub body_len: usize,
}

/// Parse the RW4 model header: magic `.RW4` + u32-LE-ish version field.
/// Observed: `89 52 57 34 77 33 32 00 ...` — `77 33 32 00` reads as ASCII "w32".
pub fn parse_rw4_header(b: &[u8]) -> Option<Rw4Header> {
    if b.len() < 12 || &b[0..4] != RW4_MAGIC {
        return None;
    }
    Some(Rw4Header {
        magic: [b[0], b[1], b[2], b[3]],
        version: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        body_len: b.len(),
    })
}

#[derive(Debug, Clone)]
pub struct AnimHeader {
    pub version: u32,
    pub source_path: String,
    pub body_len: usize,
}

/// ANIM catalog entry + collector (headers are small; totals come index-only).
#[derive(Debug, Clone)]
pub struct AnimEntry {
    pub version: u32,
    pub path: String,
    pub bytes: usize,
}

/// Collect up to `limit` ANIM headers (small reads; counts come free from index).
pub fn collect_anims(
    install: &crate::spore_install::SporeInstall,
    limit: usize,
) -> (usize, Vec<AnimEntry>) {
    let mut total = 0;
    let mut out = Vec::new();
    for pkg in &install.packages {
        if out.len() >= limit {
            // Still count the rest index-only.
            if let Ok(index) = dbpf::read_package_index(pkg) {
                total += index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == TYPE_ANIM)
                    .count();
            }
            continue;
        }
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index.entries.iter().filter(|e| e.key.type_id == TYPE_ANIM) {
            total += 1;
            if out.len() >= limit {
                continue;
            }
            let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            if let Some(h) = parse_anim_header(&bytes) {
                out.push(AnimEntry {
                    version: h.version,
                    path: h.source_path,
                    bytes: h.body_len,
                });
            }
        }
    }
    (total, out)
}

/// ANIM stores its file size at +4 and its format version at +8. The source
/// path occupies the following fixed 256-byte field (not a prefix length).
pub fn parse_anim_header(b: &[u8]) -> Option<AnimHeader> {
    if b.len() < 12 || &b[0..4] != b"ANIM" {
        return None;
    }
    let version = u32::from_le_bytes(b[8..12].try_into().ok()?);
    let rest = b.get(12..b.len().min(268))?;
    let end = rest.iter().position(|&c| c == 0)?;
    let path = String::from_utf8_lossy(&rest[..end]).into_owned();
    if path.is_empty() {
        return None;
    }
    Some(AnimHeader {
        version,
        source_path: path,
        body_len: b.len(),
    })
}

fn strip_utf8_bom(b: &[u8]) -> &[u8] {
    if b.len() >= 3 && &b[0..3] == b"\xEF\xBB\xBF" {
        &b[3..]
    } else {
        b
    }
}

/// Parse a locale string table: `# comment` lines + `0x<8hex> <text>` entries.
/// Returns (instance_id, text) pairs in file order.
pub fn parse_locale_table(blob: &[u8]) -> Vec<(u32, String)> {
    let text = String::from_utf8_lossy(strip_utf8_bom(blob));
    let mut out = Vec::new();
    for line in text.split(['\r', '\n']) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("0x") {
            if rest.len() >= 9 {
                if let Ok(id) = u32::from_str_radix(&rest[..8], 16) {
                    out.push((id, rest[9..].to_string()));
                }
            }
        }
    }
    out
}

/// Parse a name-generator word list: one name per CRLF line.
pub fn parse_name_list(blob: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(strip_utf8_bom(blob));
    text.split(['\r', '\n'])
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Parse a "raster" (GPU texture) header.
/// Observed layout: u32LE(1) | width u32LE | height u32LE | ... | "DXT1/3/5"
/// fourcc within the first 64 bytes. Returns (width, height, fourcc or "RAW").
pub fn parse_raster_header(b: &[u8]) -> Option<(u32, u32, String)> {
    decode_raster_base(b).map(|r| (r.width, r.height, r.format.label().to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    Bc1,
    Bc2,
    Bc3,
    Rgba8,
}

impl RasterFormat {
    pub fn label(&self) -> &'static str {
        match self {
            RasterFormat::Bc1 => "DXT1",
            RasterFormat::Bc2 => "DXT3",
            RasterFormat::Bc3 => "DXT5",
            RasterFormat::Rgba8 => "RAW",
        }
    }

    /// Bytes of the base mip level.
    pub fn base_bytes(&self, w: u32, h: u32) -> Option<usize> {
        let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
        match self {
            RasterFormat::Bc1 => Some(bw * bh * 8),
            RasterFormat::Bc2 | RasterFormat::Bc3 => Some(bw * bh * 16),
            RasterFormat::Rgba8 => Some(w as usize * h as usize * 4),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RasterTexture {
    pub width: u32,
    pub height: u32,
    pub format: RasterFormat,
    /// Base mip level bytes only (GPU-uploadable as-is).
    pub data: Vec<u8>,
}

/// Decode the base mip of a raster record: 28-byte preamble, pixel data,
/// optional footer. The record's own u32[6] is the base-mip byte size and
/// must match the format-implied size (checked).
pub fn decode_raster_base(blob: &[u8]) -> Option<RasterTexture> {
    if blob.len() < 28 {
        return None;
    }
    let w = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
    let h = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let magic = u32::from_le_bytes([blob[20], blob[21], blob[22], blob[23]]);
    let format = match magic {
        0x31545844 => RasterFormat::Bc1, // "DXT1"
        0x33545844 => RasterFormat::Bc2, // "DXT3"
        0x35545844 => RasterFormat::Bc3, // "DXT5"
        21 => RasterFormat::Rgba8,
        _ => return None,
    };
    let base_size_field = u32::from_le_bytes([blob[24], blob[25], blob[26], blob[27]]) as usize;
    let base_size = format.base_bytes(w, h)?;
    if base_size_field != base_size {
        return None;
    }
    if blob.len() < 28 + base_size {
        return None;
    }
    Some(RasterTexture {
        width: w,
        height: h,
        format,
        data: blob[28..28 + base_size].to_vec(),
    })
}

/// Parse the extension<->type registry: alternating `"0x<8hex>"` / name strings
/// (from the PROP record titled "File extension <-> resource type mapping...").
pub fn parse_extension_map(strings: &[String]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < strings.len() {
        let a = strings[i].trim();
        let b = strings[i + 1].trim();
        // Strings may carry a stray leading char from resync; search for 0x.........
        let hexpos = a.find("0x").or_else(|| a.find("0X"));
        if let Some(p) = hexpos {
            let hex = &a[p + 2..];
            if hex.len() >= 8 {
                if let Ok(id) = u32::from_str_radix(&hex[..8], 16) {
                    let clean = b
                        .trim_matches(|c: char| {
                            !c.is_alphanumeric() && c != '_' && c != '.' && c != '-'
                        })
                        .to_string();
                    if !clean.is_empty() && !clean.starts_with("0x") && !clean.starts_with("0X") {
                        out.push((id, clean));
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dims_parse() {
        // Minimal IHDR for 128x64.
        let mut b = Vec::new();
        b.extend_from_slice(PNG_MAGIC);
        b.extend_from_slice(&[0, 0, 0, 13]);
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&128u32.to_be_bytes());
        b.extend_from_slice(&64u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(parse_png_dims(&b), Some((128, 64)));
        assert_eq!(parse_png_dims(b"nope"), None);
    }

    #[test]
    fn anim_channels_parse() {
        // Build a minimal container: magic + path + header + CHAN table.
        let mut blob = vec![0u8; 512];
        blob[0..4].copy_from_slice(b"ANIM");
        let path = b"c:\\spore\\test\\idle.animation";
        let path_start = 12usize;
        blob[path_start..path_start + path.len()].copy_from_slice(path);
        let z = path_start + path.len();
        blob[z] = 0; // NUL-terminate the path
        let hdr = (z + 1 + 15) / 16 * 16; // 16-byte aligned after the path
        blob[hdr..hdr + 4].copy_from_slice(&1u32.to_le_bytes()); // version
        blob[hdr + 8..hdr + 12].copy_from_slice(&3u32.to_le_bytes()); // channel count
        let table = hdr + 32;
        blob[hdr + 12..hdr + 16].copy_from_slice(&(table as u32).to_le_bytes());
        // Three CHAN chunks, each: tag + 4 pad + name + NUL.
        let names = ["root", "L foot", "MouthMorphs"];
        let mut chunk = table + 3 * 4;
        for (i, nm) in names.iter().enumerate() {
            let o = chunk;
            blob[o..o + 4].copy_from_slice(b"CHAN");
            let s = o + 8;
            blob[s..s + nm.len()].copy_from_slice(nm.as_bytes());
            let entry = table + i * 4;
            blob[entry..entry + 4].copy_from_slice(&(o as u32).to_le_bytes());
            chunk = (s + nm.len() + 1 + 15) / 16 * 16;
        }
        blob.resize(chunk + 16, 0);
        assert_eq!(parse_anim_channels(&blob), names);

        // Rejects junk and a bogus table offset.
        assert!(parse_anim_channels(&[0u8; 64]).is_empty());
        let mut bad = vec![0u8; 256];
        bad[0..4].copy_from_slice(b"ANIM");
        let h = 32usize;
        bad[h..h + 4].copy_from_slice(&1u32.to_le_bytes());
        bad[h + 8..h + 12].copy_from_slice(&4u32.to_le_bytes());
        bad[h + 12..h + 16].copy_from_slice(&0xDEAD_0000u32.to_le_bytes());
        assert!(parse_anim_channels(&bad).is_empty());
    }

    #[test]
    fn anim_clip_keyframes_decode() {
        // Build a minimal-but-spec-shaped container: header fields, one
        // channel with 2 rot keyframes and 2 pos keyframes.
        let mut blob = vec![0u8; 0x800];
        blob[0..4].copy_from_slice(b"ANIM");
        blob[8..12].copy_from_slice(&0x19u32.to_le_bytes()); // version
        blob[0x11C..0x120].copy_from_slice(&0x3D08_8889u32.to_le_bytes()); // 1/30
        blob[0x120..0x124].copy_from_slice(&1.5f32.to_le_bytes()); // length
        let table = 0x200usize;
        blob[0x144..0x148].copy_from_slice(&1u32.to_le_bytes()); // channel count
        blob[0x148..0x14C].copy_from_slice(&(table as u32).to_le_bytes());
        // Channel chunk at 0x210: tag, pointer, name padded to 0x80.
        let co = 0x210usize;
        blob[table..table + 4].copy_from_slice(&(co as u32).to_le_bytes());
        blob[co..co + 4].copy_from_slice(b"CHAN");
        let name = b"root";
        blob[co + 8..co + 8 + name.len()].copy_from_slice(name);
        blob[co + 0x8C..co + 0x90].copy_from_slice(&1u32.to_le_bytes()); // select: root
        blob[co + 0x90..co + 0x94].copy_from_slice(b"root"); // capability
                                                             // 2 rot keyframes at kptr (stride 64) then 1 pos keyframe.
        let kptr = 0x300usize;
        blob[co + 0xD4..co + 0xD8].copy_from_slice(&2u32.to_le_bytes()); // keyframe count
        blob[co + 0xD8..co + 0xDC].copy_from_slice(&(kptr as u32).to_le_bytes());
        blob[co + 0xDC..co + 0xE0].copy_from_slice(&2u32.to_le_bytes()); // 2 components
        let meta = 0x700usize; // component metadata (clear of the keyframes)
        blob[co + 0xE0..co + 0xE4].copy_from_slice(&(meta as u32).to_le_bytes());
        // Component 0: rotation (type 2), offset 0, stride 64.
        blob[meta..meta + 4].copy_from_slice(&2u32.to_le_bytes());
        blob[meta + 8..meta + 12].copy_from_slice(&0u32.to_le_bytes());
        blob[meta + 12..meta + 16].copy_from_slice(&64u32.to_le_bytes());
        // Component 1: position (type 1), offset 128, stride 64.
        blob[meta + 32..meta + 36].copy_from_slice(&1u32.to_le_bytes());
        blob[meta + 40..meta + 44].copy_from_slice(&128u32.to_le_bytes());
        blob[meta + 44..meta + 48].copy_from_slice(&64u32.to_le_bytes());
        // Keyframes: identity quat, then a 90° Y quat, then a position.
        fn put_f32s(blob: &mut [u8], o: usize, q: &[f32]) {
            for (i, v) in q.iter().enumerate() {
                blob[o + i * 4..o + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        put_f32s(&mut blob, kptr, &[0.0, 0.0, 0.0, 1.0]);
        put_f32s(
            &mut blob,
            kptr + 64,
            &[0.0, 0.707_106_77, 0.0, 0.707_106_77],
        );
        put_f32s(&mut blob, kptr + 128, &[1.0, 2.0, 3.0]);
        let clip = parse_anim_clip(&blob).expect("clip");
        assert!((clip.length - 1.5).abs() < 1e-6);
        assert_eq!(clip.channels.len(), 1);
        let c = &clip.channels[0];
        assert_eq!(c.name, "root");
        assert_eq!(c.select_flags, 1);
        assert_eq!(c.capability, "root");
        assert_eq!(c.rot.len(), 2);
        // Every component carries `keyframeCount` keys (per the format); the
        // second pos key falls past what we wrote and reads zeros.
        assert_eq!(c.pos.len(), 2);
        assert_eq!(c.rot[0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(c.pos[0], [1.0, 2.0, 3.0]);
        // Junk is rejected.
        assert!(parse_anim_clip(&[0u8; 0x200]).is_none());
    }

    #[test]
    fn channel_selectors_decode() {
        let mk = |select_flags: u32, capability: &str| AnimChannel {
            select_flags,
            capability: capability.to_string(),
            ..Default::default()
        };
        assert_eq!(
            channel_selector(&mk(1, "")),
            ("root".to_string(), String::new())
        );
        assert_eq!(channel_selector(&mk(0x20004, "foot")).0, "foot");
        assert_eq!(channel_selector(&mk(0x20004, "foot")).1, "right2");
        assert_eq!(channel_selector(&mk(0x20000, "foot")).1, "left2");
        assert_eq!(
            channel_selector(&mk(8, "grsp")),
            ("grsp".to_string(), "right".to_string())
        );
        assert_eq!(channel_selector(&mk(4, "grsp")).1, "left");
        assert_eq!(channel_selector(&mk(0, "")), (String::new(), String::new()));
    }

    #[test]
    fn clip_streams_sample() {
        // Positions lerp; rotations nlerp with shortest-path + normalization.
        let pos = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];
        assert_eq!(sample_vec3(&pos, 0.0), Some([0.0, 0.0, 0.0]));
        let mid = sample_vec3(&pos, 0.5).unwrap();
        assert!((mid[0] - 0.5).abs() < 1e-6 && (mid[1] - 1.0).abs() < 1e-6);
        assert_eq!(sample_vec3(&pos, 1.0), Some([1.0, 2.0, 3.0]));
        assert_eq!(sample_vec3(&pos, 2.0), Some([1.0, 2.0, 3.0])); // clamped
        assert!(sample_vec3(&[], 0.5).is_none());
        // Identity -> identity.
        let q = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(sample_quat(&q, 0.5), Some([0.0, 0.0, 0.0, 1.0]));
        // Halfway between identity and a 180° +Y flip stays unit length.
        let q2 = [[0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0]];
        let s = sample_quat(&q2, 0.5).unwrap();
        let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2] + s[3] * s[3]).sqrt();
        assert!((len - 1.0).abs() < 1e-6);
        // Shortest-path: antipodal keys (same rotation, opposite sign) still
        // interpolate to an identity rotation, just possibly with negated w.
        let q3 = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, -1.0]];
        let t = sample_quat(&q3, 0.5).unwrap();
        assert!(t[0].abs() < 1e-6 && t[1].abs() < 1e-6 && t[2].abs() < 1e-6);
        assert!((t[3].abs() - 1.0).abs() < 1e-6);
        assert!(sample_quat(&[], 0.5).is_none());
    }

    #[test]
    fn utf16_extracts_words() {
        let mut blob = Vec::new();
        for c in "Hello".encode_utf16() {
            blob.extend_from_slice(&c.to_le_bytes());
        }
        blob.extend_from_slice(&[0, 0]);
        blob.extend_from_slice(&[0x01, 0x02, 0x03]);
        for c in "Spore".encode_utf16() {
            blob.extend_from_slice(&c.to_le_bytes());
        }
        let out = extract_utf16_strings(&blob, 4);
        assert!(out.iter().any(|s| s.contains("Hello")));
        assert!(out.iter().any(|s| s.contains("Spore")));
    }

    #[test]
    fn rw4_rejects_non_model() {
        assert!(parse_rw4_header(b"\x89PNG....").is_none());
        let mut b = vec![0x89, b'R', b'W', b'4', 0x77, 0x33, 0x32, 0x00, 1, 2, 3, 4];
        assert!(parse_rw4_header(&b).is_some());
        let _ = b.pop();
    }

    #[test]
    fn anim_parses_path() {
        let mut b = b"ANIM".to_vec();
        b.extend_from_slice(&0x55u32.to_le_bytes());
        b.extend_from_slice(&25u32.to_le_bytes()); // format version
        b.extend_from_slice(b"c:\\spore\\data\\creatures\\walk.animation\0");
        b.extend_from_slice(&[0u8; 16]);
        let h = parse_anim_header(&b).unwrap();
        assert_eq!(h.version, 25);
        assert!(h.source_path.contains("walk.animation"));
        assert!(h.source_path.contains("creatures"));
        assert!(parse_anim_header(b"ANIM\x55\0\0\0\x19\0\0\0no-terminator-here!").is_none());
    }

    #[test]
    fn locale_table_parses() {
        let blob = b"\xEF\xBB\xBF# WARNING: generated\n\n0x06731782 Tribal chief\n0x067213A6 Another line\n# comment\n";
        let out = parse_locale_table(blob);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 0x06731782);
        assert!(out[0].1.contains("Tribal chief"));
    }

    #[test]
    fn name_list_parses() {
        let blob = b"\xEF\xBB\xBFabelard\r\nAberdeen\r\n\r\nAelius\r\n";
        let out = parse_name_list(blob);
        assert_eq!(out, vec!["abelard", "Aberdeen", "Aelius"]);
    }

    #[test]
    fn raster_parses_dxt5_512() {
        // Synthesized 512x512 DXT5 record (base mip zero-filled).
        let mut b = vec![1, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 10, 0, 0, 0, 8, 0, 0, 0];
        b.extend_from_slice(b"DXT5");
        b.extend_from_slice(&262144u32.to_le_bytes());
        b.extend_from_slice(&vec![0u8; 262144]);
        assert_eq!(
            parse_raster_header(&b),
            Some((512, 512, "DXT5".to_string()))
        );
        assert!(parse_raster_header(&[0u8; 8]).is_none());
    }

    #[test]
    fn raster_base_roundtrip_bc3() {
        // 4x4 BC3: one 16-byte block.
        let mut b = vec![1, 0, 0, 0, 4, 0, 0, 0, 4, 0, 0, 0, 10, 0, 0, 0, 1, 0, 0, 0];
        b.extend_from_slice(b"DXT5");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[0xABu8; 16]);
        let r = decode_raster_base(&b).expect("decode");
        assert_eq!((r.width, r.height, r.format), (4, 4, RasterFormat::Bc3));
        assert_eq!(r.data.len(), 16);
    }

    #[test]
    fn raster_rejects_size_mismatch() {
        let mut b = vec![1, 0, 0, 0, 4, 0, 0, 0, 4, 0, 0, 0, 10, 0, 0, 0, 1, 0, 0, 0];
        b.extend_from_slice(b"DXT5");
        b.extend_from_slice(&15u32.to_le_bytes()); // lies about base size
        b.extend_from_slice(&[0u8; 32]);
        assert!(decode_raster_base(&b).is_none());
    }

    #[test]
    fn raster_raw_rgba_dims() {
        let mut b = vec![1, 0, 0, 0, 32, 0, 0, 0, 32, 0, 0, 0, 6, 0, 0, 0, 8, 0, 0, 0];
        b.extend_from_slice(&21u32.to_le_bytes());
        b.extend_from_slice(&4096u32.to_le_bytes());
        b.extend_from_slice(&vec![0x80u8; 4096]);
        let r = decode_raster_base(&b).expect("decode");
        assert_eq!((r.width, r.height, r.format), (32, 32, RasterFormat::Rgba8));
        assert_eq!(r.data.len(), 4096);
    }

    #[test]
    fn extension_map_pairs() {
        let strings = vec![
            "File extension <-> resource type mapping applied".to_string(),
            "0x2f7d0004".to_string(),
            "png".to_string(),
            "0x00b1b104".to_string(),
            "prop".to_string(),
        ];
        let map = parse_extension_map(&strings);
        assert_eq!(
            map,
            vec![
                (0x2F7D0004, "png".to_string()),
                (0x00B1B104, "prop".to_string())
            ]
        );
    }
}
