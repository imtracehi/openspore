//! EA SNR audio (Spore-era EAAudioCore, version 0).
//!
//! Header layout (big-endian, verified against retail records):
//! - header1: version(4b) | codec(4b) | channels-1(6b) | rate(18b)
//! - header2: type(2b) | loop(1b) | num_samples(29b)
//! - V0 RAM: 8B header, then one data block [id:u8][size:u24 BE].
//!
//! Codecs in the installed packages (census of all 20,411 SNR headers, `notes\audio-codec.md`):
//! 4 = EA-XAS v1 (6,606 records), 5 = EALayer3 v1 (13,034), 0 = XAS "v0" (291), 1 = EALayer3 with a
//! one-byte frame prefix (470), 2 = 16-bit big-endian PCM (10). All are decoded here.
//!
//! XAS v1 (ported from the documented vgmstream decoder): frames of 0x4C
//! bytes per channel = 128 samples; 4 group headers (coef idx, two
//! 12-bit history samples, shift) + 4x15 nibble rows, high nibble first.
//!
//! Codec 0 (XAS "v0", ADAPTED naming; layout derived from the installed records, no public document
//! was used): the same 32-sample group (12-bit history samples, 4-bit coefficient index and shift,
//! 15 nibble rows) but a frame holds one group per channel: `[coef word | h2] x channels`,
//! `[shift word | h1] x channels` (planar, u16 little-endian), then 15 rows of one byte per channel.
//! A frame is 19 bytes per channel and decodes to 32 samples per channel. VERIFIED: the group seams
//! of 291 installed records are as smooth as the group interiors and no record clips.
//!
//! Codec 1 (ADAPTED naming): the EALayer3 frames of codec 5 (MPEG-2, 22.05 kHz, mono or joint
//! stereo) each preceded by one byte (always 1 in the records checked); the frames tile every block
//! exactly. The prefix byte is skipped (its meaning is not known).
//!
//! Codec 2: interleaved 16-bit big-endian PCM.

use crate::dbpf;
use crate::decode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnrCodec {
    /// Codec 0: XAS groups, one per channel in every frame (see the module docs).
    Xas0,
    /// Codec 1: EALayer3 frames with a one-byte prefix each.
    EaLayer3V0,
    /// Codec 2: 16-bit big-endian PCM.
    Pcm16Be,
    Xas1,
    EaLayer3V1,
    Other(u8),
}

impl SnrCodec {
    /// The EALayer3 family (frames rebuilt to MPEG and decoded with puremp3).
    pub fn is_eal3(self) -> bool {
        matches!(self, SnrCodec::EaLayer3V0 | SnrCodec::EaLayer3V1)
    }

    /// Bytes in front of every EALayer3 frame.
    fn frame_prefix(self) -> usize {
        usize::from(self == SnrCodec::EaLayer3V0)
    }

    /// Whether `decode_snr_sound` can decode it.
    pub fn supported(self) -> bool {
        !matches!(self, SnrCodec::Other(_))
    }

    /// The header's codec number.
    pub fn number(self) -> u8 {
        match self {
            SnrCodec::Xas0 => 0,
            SnrCodec::EaLayer3V0 => 1,
            SnrCodec::Pcm16Be => 2,
            SnrCodec::Xas1 => 4,
            SnrCodec::EaLayer3V1 => 5,
            SnrCodec::Other(n) => n,
        }
    }

    /// Census label.
    pub fn label(self) -> String {
        match self {
            SnrCodec::Xas0 => "XAS0".into(),
            SnrCodec::EaLayer3V0 => "EAL3-0".into(),
            SnrCodec::Pcm16Be => "PCM16BE".into(),
            SnrCodec::Xas1 => "XAS1".into(),
            SnrCodec::EaLayer3V1 => "EAL3".into(),
            SnrCodec::Other(n) => format!("codec{n}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnrHeader {
    pub codec: SnrCodec,
    pub channels: usize,
    pub sample_rate: u32,
    pub num_samples: usize,
    pub is_ram: bool,
}

pub fn parse_snr_header(blob: &[u8]) -> Option<SnrHeader> {
    if blob.len() < 8 {
        return None;
    }
    let h1 = u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]);
    let h2 = u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]);
    if (h1 >> 28) & 0x0F != 0 {
        return None; // not version 0
    }
    let codec = match ((h1 >> 24) & 0x0F) as u8 {
        0 => SnrCodec::Xas0,
        1 => SnrCodec::EaLayer3V0,
        2 => SnrCodec::Pcm16Be,
        4 => SnrCodec::Xas1,
        5 => SnrCodec::EaLayer3V1,
        other => SnrCodec::Other(other),
    };
    let channels = (((h1 >> 18) & 0x3F) + 1) as usize;
    let rate = h1 & 0x03FFFF;
    if !(1..=8).contains(&channels) || rate < 8000 || rate > 200000 {
        return None;
    }
    let typ = ((h2 >> 30) & 0x03) as u8;
    if typ > 2 {
        return None;
    }
    Some(SnrHeader {
        codec,
        channels,
        sample_rate: rate,
        num_samples: (h2 & 0x1FFFFFFF) as usize,
        is_ram: typ == 0,
    })
}

const XA_COEFS: [[f64; 2]; 4] = [
    [0.0, 0.0],
    [0.9375, 0.0],
    [1.796875, -0.8125],
    [1.53125, -0.859375],
];

fn clamp16(v: f64) -> i16 {
    (v as i32).clamp(-32768, 32767) as i16
}

/// Decode one 32-sample XAS group: `coef_word` is the low half of the group header (4-bit
/// coefficient index, 12-bit older history sample), `shift_word` the high half (4-bit shift, 12-bit
/// newer history sample), `row(r)` the group's byte of nibble row `r` (high nibble first).
fn decode_xas_group(coef_word: u32, shift_word: u32, row: impl Fn(usize) -> u8, out: &mut Vec<i16>) {
    // Only 4 filter pairs exist; bad indexes mean no prediction.
    let (c1, c2) = match (coef_word & 0x0F) as usize {
        i if i < 4 => (XA_COEFS[i][0], XA_COEFS[i][1]),
        _ => (0.0, 0.0),
    };
    let mut h2 = ((coef_word & 0xFFF0) as u16) as i16;
    let mut h1 = ((shift_word & 0xFFF0) as u16) as i16;
    let shift = shift_word & 0x0F;
    out.push(h2);
    out.push(h1);
    for r in 0..15 {
        let nb = row(r);
        for i in 0..2 {
            let nib = if i & 1 == 1 { nb & 0x0F } else { (nb >> 4) & 0x0F };
            let s = (((nib as u16) << 12) as i16 as i32) >> shift;
            let v = s as f64 + h1 as f64 * c1 + h2 as f64 * c2;
            let v = clamp16(v);
            out.push(v);
            h2 = h1;
            h1 = v;
        }
    }
}

/// Decode one 0x4C-byte XAS v1 frame (mono, 128 samples).
pub(crate) fn decode_xas_frame(frame: &[u8], out: &mut Vec<i16>) {
    if frame.len() < 0x4C {
        return;
    }
    for g in 0..4 {
        let gh = u32::from_le_bytes([
            frame[g * 4],
            frame[g * 4 + 1],
            frame[g * 4 + 2],
            frame[g * 4 + 3],
        ]);
        decode_xas_group(gh & 0xFFFF, gh >> 16, |r| frame[16 + r * 4 + g], out);
    }
}

/// Bytes of one codec 0 frame: 19 per channel (a 4-byte header and 15 nibble bytes).
pub(crate) const fn xas0_frame_bytes(channels: usize) -> usize {
    0x13 * channels
}

/// Decode one codec 0 frame (see the module docs): one 32-sample group per channel, the group
/// headers planar (all coefficient words, then all shift words), nibble rows interleaved by channel.
pub(crate) fn decode_xas0_frame(frame: &[u8], channels: usize, out: &mut [Vec<i16>]) {
    if frame.len() < xas0_frame_bytes(channels) || out.len() < channels {
        return;
    }
    let word = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]) as u32;
    for (g, dst) in out.iter_mut().enumerate().take(channels) {
        decode_xas_group(
            word(2 * g),
            word(2 * channels + 2 * g),
            |r| frame[4 * channels + r * channels + g],
            dst,
        );
    }
}

/// Decode a RAM XAS record (codec 4, or the codec 0 layout) to interleaved i16 PCM.
/// Returns (pcm, channels, rate).
///
/// The audio follows the SNR header (8 bytes, +4 loop start when looped) as
/// `[flag|size:u32 BE][samples:u32 BE]` blocks; frames start after that
/// 8-byte block header (see [`decode_snr_sound`]).
pub fn decode_xas_ram(blob: &[u8]) -> Option<(Vec<i16>, usize, u32)> {
    let h = parse_snr_header(blob)?;
    if !matches!(h.codec, SnrCodec::Xas1 | SnrCodec::Xas0) || !h.is_ram {
        return None;
    }
    if h.num_samples == 0 || h.num_samples > 100_000_000 {
        return None;
    }
    let sound = decode_snr_sound(blob, None, None).ok()?;
    if sound.frames() != h.num_samples {
        return None;
    }
    Some((sound.samples, sound.channels, sound.rate))
}

/// Write 16-bit PCM WAV bytes.
pub fn write_wav(pcm: &[i16], channels: usize, rate: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + pcm.len() * 2) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&((channels * 2) as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32 * 2).to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Convenience: decode a RAM DBPF record of any supported codec to WAV bytes.
pub fn wav_from_package_record(
    pkg: &std::path::Path,
    entry: &dbpf::DbpfEntry,
) -> Option<(Vec<u8>, usize, u32, usize)> {
    let bytes = dbpf::read_entry_bytes(pkg, entry).ok()?;
    let sound = decode_snr_sound(&bytes, None, None).ok()?;
    Some((sound.to_wav(), sound.channels, sound.rate, sound.frames()))
}

/// MSB-first bit reader over a byte slice.
pub struct BitReader<'a> {
    buf: &'a [u8],
    pub pos: usize, // bit position
}

impl<'a> BitReader<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    pub fn get(&mut self, n: usize) -> Option<u32> {
        if n > 32 {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.buf.get(self.pos / 8)?;
            v = (v << 1) | ((byte >> (7 - self.pos % 8)) & 1) as u32;
            self.pos += 1;
        }
        Some(v)
    }

    pub fn byte_pos(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EalFrame {
    /// Byte offset of the frame start (pcm_flag).
    pub offset: usize,
    /// Total frame size in bytes (pre + common + pcm).
    pub size: usize,
    pub channels: usize,
    pub granule: u32,
    pub main_data_bits: Vec<u32>,
    pub others_1: Vec<u32>,
    pub others_2: Vec<u32>,
    pub scfsi: Vec<u32>,
    pub version_index: u32,
    pub sample_rate_index: u32,
    pub channel_mode: u32,
    pub mode_extension: u32,
    pub pcm_samples: u32,
    pub has_common: bool,
}

/// Parse one EALayer3 v1 frame at `offset` (V1b-in-RAM layout).
/// Returns the frame; caller advances by `frame.size`.
pub fn parse_eal_frame(blob: &[u8], offset: usize) -> Option<EalFrame> {
    if offset >= blob.len() {
        return None;
    }
    let flag = blob[offset];
    if flag != 0x00 && flag != 0xEE {
        return None;
    }
    let mut f = EalFrame {
        offset,
        ..Default::default()
    };
    let mut pre = 1usize; // flag byte
    let mut br = BitReader::new(blob, (offset + 1) * 8);
    // V1b always has a common frame, even with PCM flag.
    f.version_index = br.get(2)?;
    f.sample_rate_index = br.get(2)?;
    f.channel_mode = br.get(2)?;
    f.mode_extension = br.get(2)?;
    if f.version_index == 0
        && f.sample_rate_index == 0
        && f.channel_mode == 0
        && f.mode_extension == 0
    {
        return None; // empty frame
    }
    // MPEG1 (index 3) and MPEG2 (index 2), mono/stereo channel modes.
    if f.version_index != 2 && f.version_index != 3 {
        return None;
    }
    f.channels = if f.channel_mode == 3 { 1 } else { 2 };
    let mpeg1 = f.version_index == 3;
    f.granule = br.get(1)?;
    // MPEG1 granule-1 frames carry scfsi (4 bits per channel) up front,
    // before any channel data. Side info: 12 + 32 + (15/19) per channel.
    let o2bits = if mpeg1 { 15 } else { 19 };
    for _ in 0..f.channels {
        if mpeg1 && f.granule == 1 {
            f.scfsi.push(br.get(4)?);
        } else {
            f.scfsi.push(0);
        }
    }
    let mut data_bits = 0u32;
    for _ in 0..f.channels {
        let mds = br.get(12)?;
        let o1 = br.get(32)?;
        let o2 = br.get(o2bits)?;
        f.main_data_bits.push(mds);
        f.others_1.push(o1);
        f.others_2.push(o2);
        data_bits += mds;
    }
    let base_bits = (br.pos - (offset + 1) * 8) as u32;
    let pad = (8 - (base_bits + data_bits) % 8) % 8;
    // Skip main data (kept as offsets; rebuild reads it back).
    br.pos += (data_bits + pad) as usize;
    // Consume the bytes read so far.
    let consumed = br.byte_pos() - (offset + 1);
    pre += consumed;
    f.size = pre; // extended below by PCM block
    f.has_common = true;
    if flag == 0xEE {
        // PCM block: offset_samples(16) + pcm_samples(16) + unknown(32, v1b).
        let mut br2 = BitReader::new(blob, (offset + pre) * 8);
        let _off_samp = br2.get(16)?;
        f.pcm_samples = br2.get(16)?;
        let _unk = br2.get(32)?;
        pre += 8;
        f.size = pre + f.pcm_samples as usize * 2 * f.channels;
    }
    Some(f)
}

/// Walk all EALayer3 v1 frames in a record body. Returns frames on full tiling.
pub fn walk_eal_frames(blob: &[u8], start: usize) -> Option<Vec<EalFrame>> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos < blob.len() {
        match parse_eal_frame(blob, pos) {
            Some(f) => {
                if f.size == 0 || pos + f.size > blob.len() {
                    return None;
                }
                pos += f.size;
                out.push(f);
            }
            None => return None,
        }
        if out.len() > 100000 {
            return None;
        }
    }
    Some(out)
}

/// Debug variant: reports (frames_tiled, break_byte, reason).
pub fn tiling_break(blob: &[u8], start: usize) -> (usize, usize, &'static str) {
    let mut pos = start;
    let mut n = 0;
    loop {
        if pos >= blob.len() {
            return (n, pos, "tiled-exact");
        }
        let flag = blob[pos];
        if flag != 0x00 && flag != 0xEE {
            return (n, pos, "bad-flag");
        }
        match parse_eal_frame(blob, pos) {
            Some(f) => {
                if f.size == 0 || pos + f.size > blob.len() {
                    return (n, pos, "bad-size");
                }
                pos += f.size;
                n += 1;
            }
            None => return (n, pos, "parse-fail"),
        }
        if n > 100000 {
            return (n, pos, "too-many");
        }
    }
}

/// Bit writer (MSB-first) for MPEG rebuild.
pub struct BitWriter {
    pub buf: Vec<u8>,
    acc: u32,
    nbits: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            acc: 0,
            nbits: 0,
        }
    }

    pub fn put(&mut self, n: usize, mut v: u32) {
        if n > 32 {
            return;
        }
        if n == 32 {
            // Avoid overflow: two 16-bit halves.
            let hi = (v >> 16) & 0xFFFF;
            let lo = v & 0xFFFF;
            self.put(16, hi);
            self.put(16, lo);
            return;
        }
        v &= if n >= 32 { u32::MAX } else { (1u32 << n) - 1 };
        self.acc = (self.acc << n) | v;
        self.nbits += n;
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.buf.push((self.acc >> self.nbits) as u8);
        }
        // Keep only the undrained remainder.
        self.acc &= if self.nbits >= 32 {
            u32::MAX
        } else if self.nbits == 0 {
            0
        } else {
            (1u32 << self.nbits) - 1
        };
    }

    pub fn align_zero(&mut self) {
        if self.nbits % 8 != 0 {
            let pad = 8 - self.nbits % 8;
            self.put(pad, 0);
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

/// Rebuild one standard MPEG2 Layer III frame from a single-granule
/// EALayer3 frame (mono/stereo), free-format sized. Returns frame bytes.
/// PCM-block-only frames yield an empty vec (caller skips them).
pub fn rebuild_mpeg_frame(blob: &[u8], f: &EalFrame) -> Vec<u8> {
    rebuild_mpeg_frame_cbr(blob, f, 22050, 0x00)
}

/// Rebuild one standard MPEG1 Layer III frame from a granule pair.
/// Returns empty on mismatch (versions/rates/channels/granules must pair).
pub fn rebuild_mpeg_frame_mpeg1(
    blob: &[u8],
    f0: &EalFrame,
    f1: &EalFrame,
    rate: u32,
    bitrate_idx: u32,
) -> Vec<u8> {
    if !f0.has_common
        || !f1.has_common
        || f0.version_index != 3
        || f1.version_index != 3
        || f0.sample_rate_index != f1.sample_rate_index
        || f0.channel_mode != f1.channel_mode
        || f0.granule == f1.granule
    {
        return Vec::new();
    }
    // Order: granule0 first regardless of storage order.
    let (g0, g1) = if f0.granule == 0 { (f0, f1) } else { (f1, f0) };
    if g0.granule != 0 || g1.granule != 1 {
        return Vec::new();
    }
    let ch = g0.channels;
    let mut os = BitWriter::new();
    os.put(11, 0x7FF);
    os.put(2, g0.version_index);
    os.put(2, 0x01);
    os.put(1, 1);
    os.put(4, bitrate_idx);
    os.put(2, g0.sample_rate_index);
    os.put(1, 0);
    os.put(1, 0);
    os.put(2, g0.channel_mode);
    os.put(2, g0.mode_extension);
    os.put(1, 1);
    os.put(1, 1);
    os.put(2, 0);
    os.put(9, 0); // main_data_begin
    os.put(if ch == 1 { 5 } else { 3 }, 0); // private bits
    for i in 0..ch {
        os.put(4, g1.scfsi[i]); // scfsi lives in granule1
    }
    for g in [g0, g1] {
        for i in 0..ch {
            os.put(12, g.main_data_bits[i]);
            os.put(32, g.others_1[i]);
            os.put(15, g.others_2[i]);
        }
    }
    // Main data: granule0 channels, then granule1 channels. Base skips
    // common(8) + granule(1) + scfsi-if-present + side info (flag already
    // skipped by the +1); same layout the parser walks.
    for g in [g0, g1] {
        let base_bits =
            (8 + 1 + if g.granule == 1 { 4 * g.channels } else { 0 } + (12 + 32 + 15) * g.channels)
                as usize;
        let mut bit = (g.offset + 1) * 8 + base_bits;
        for i in 0..g.channels {
            for _ in 0..g.main_data_bits[i] as usize {
                let byte = match blob.get(bit / 8) {
                    Some(b) => *b,
                    None => return Vec::new(),
                };
                os.put(1, ((byte >> (7 - bit % 8)) & 1) as u32);
                bit += 1;
            }
        }
    }
    os.align_zero();
    if bitrate_idx != 0 {
        let size = match mpeg_frame_size(3, rate, bitrate_idx) {
            Some(s) => s,
            None => return Vec::new(),
        };
        if os.buf.len() > size {
            return Vec::new();
        }
        os.buf.resize(size, 0);
    }
    os.buf
}
/// MPEG frame size in bytes for a CBR index (no padding bit).
/// Tables: MPEG1 [.., 320 max], MPEG2/2.5 [.., 160 max].
pub fn mpeg_frame_size(version_index: u32, rate: u32, bitrate_idx: u32) -> Option<usize> {
    let table: [u32; 16] = if version_index == 3 {
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ]
    } else if version_index == 2 || version_index == 0 {
        [
            0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
        ]
    } else {
        return None;
    };
    let br = *table.get(bitrate_idx as usize)?;
    if br == 0 || rate == 0 {
        return None;
    }
    Some(if version_index == 3 {
        (144 * br * 1000 / rate) as usize
    } else {
        (72 * br * 1000 / rate) as usize
    })
}

/// Rebuild with an explicit bitrate index (0x00 = free format).
/// Standard CBR indices let ordinary MP3 decoders parse the stream:
/// content is padded with zeros (ancillary data) to the declared size.
/// Only MPEG2 single-granule frames are supported (Spore's music/SFX case).
pub fn rebuild_mpeg_frame_cbr(blob: &[u8], f: &EalFrame, rate: u32, bitrate_idx: u32) -> Vec<u8> {
    if !f.has_common || f.version_index != 2 {
        return Vec::new();
    }
    let mut os = BitWriter::new();
    // MPEG header: sync + version + layer III + no CRC + free bitrate.
    os.put(11, 0x7FF);
    os.put(2, f.version_index);
    os.put(2, 0x01);
    os.put(1, 1);
    os.put(4, bitrate_idx);
    os.put(2, f.sample_rate_index);
    os.put(1, 0);
    os.put(1, 0);
    os.put(2, f.channel_mode);
    os.put(2, f.mode_extension);
    os.put(1, 1);
    os.put(1, 1);
    os.put(2, 0);
    // Side info (MPEG2): main_data_begin(8)=0, private, then per channel
    // 12+32+19 bits verbatim.
    os.put(8, 0);
    os.put(if f.channels == 1 { 1 } else { 2 }, 0);
    for i in 0..f.channels {
        os.put(12, f.main_data_bits[i]);
        os.put(32, f.others_1[i]);
        os.put(19, f.others_2[i]);
    }
    // Main data bits verbatim from the record (per-channel contiguous).
    // Base header bits: common(8) + granule(1) + side info. (The flag byte
    // is already skipped by the +1; an extra +8 here once caused silent
    // data shifts, caught by the synthetic content test above.)
    let base_bits = (8 + 1 + (12 + 32 + 19) * f.channels) as usize;
    let mut bit = (f.offset + 1) * 8 + base_bits;
    for i in 0..f.channels {
        for _ in 0..f.main_data_bits[i] as usize {
            let byte = match blob.get(bit / 8) {
                Some(b) => *b,
                None => return Vec::new(),
            };
            let b = (byte >> (7 - bit % 8)) & 1;
            os.put(1, b as u32);
            bit += 1;
        }
        // Padding to byte boundary lives inside common_size; the rebuild
        // accumulates raw bits and aligns at the end.
    }
    os.align_zero();
    if bitrate_idx != 0 {
        // Pad to the declared CBR size (ancillary zeros); overflow fails.
        let size = match mpeg_frame_size(f.version_index, rate, bitrate_idx) {
            Some(s) => s,
            None => return Vec::new(),
        };
        if os.buf.len() > size {
            return Vec::new();
        }
        os.buf.resize(size, 0);
    }
    os.buf
}

/// SNR sample records: type `snr` (0x01A527DB) in the `audio~` group
/// (0x021407EE, the value `decode::TYPE_AUDIO` holds). SNS stream bodies
/// (0x01EEF63A) and soundProps (0x02B9F662) share the group.
pub fn is_audio_record(entry: &dbpf::DbpfEntry) -> bool {
    entry.key.group_id == decode::TYPE_AUDIO && entry.key.type_id == 0x01A5_27DB
}

/// Valid MPEG1/2 common header at the given flag position (pure check).
fn valid_common_at(blob: &[u8], offset: usize) -> bool {
    if offset + 1 >= blob.len() {
        return false;
    }
    let flag = blob[offset];
    if flag != 0x00 && flag != 0xEE {
        return false;
    }
    let b = blob[offset + 1];
    let (vi, si, cm) = ((b >> 6) & 3, (b >> 4) & 3, (b >> 2) & 3);
    // MPEG1 idx 3 (rates 44100/48000/32000) or MPEG2 idx 2 (22050/...).
    ((vi == 3 && si <= 2) || (vi == 2 && si <= 2)) && cm >= 1 && cm <= 3
}

/// Rebuild a whole record body to standard MP3 frames (CBR).
/// MPEG2 frames rebuild singly; MPEG1 frames pair granules (pending
/// granule-0 + next granule-1, mirroring the reference decoder).
/// PCM-block bytes are skipped (documented gap: sub-50-sample transients).
/// Returns (mp3 bytes, version_index) for decoder expectations.
pub fn record_to_mp3_cbr(blob: &[u8], rate: u32) -> Option<(Vec<u8>, u32)> {
    // Body start varies (12, 16, or a small header extension): first
    // candidate that tiles the record end-to-end wins.
    let mut starts = vec![12usize, 16usize];
    let mut s = 17;
    while s < 96 && s < blob.len() {
        if valid_common_at(blob, s) && !starts.contains(&s) {
            starts.push(s);
        }
        s += 1;
    }
    let mut frames: Option<Vec<EalFrame>> = None;
    for start in starts {
        if start < blob.len() {
            if let Some(f) = walk_eal_frames(blob, start) {
                if !f.is_empty() {
                    frames = Some(f);
                    break;
                }
            }
        }
    }
    let frames = frames?;
    let ver = frames
        .iter()
        .find(|f| f.has_common)
        .map(|f| f.version_index)?;
    let mut mp3 = Vec::new();
    if ver == 2 {
        let mut skipped = 0;
        for f in &frames {
            if !f.has_common {
                continue;
            }
            let out = rebuild_mpeg_frame_cbr(blob, f, rate, 0x0E);
            if out.is_empty() {
                skipped += 1;
                continue;
            }
            mp3.extend_from_slice(&out);
        }
        if mp3.is_empty() {
            return None;
        }
        let _ = skipped;
    } else if ver == 3 {
        let mut pending: Option<&EalFrame> = None;
        let mut skipped = 0;
        for f in frames.iter().filter(|f| f.has_common) {
            if f.granule == 0 {
                pending = Some(f);
            } else if let Some(g0) = pending.take() {
                if g0.sample_rate_index != f.sample_rate_index || g0.channel_mode != f.channel_mode
                {
                    pending = Some(f);
                    skipped += 1;
                    continue;
                }
                let out = rebuild_mpeg_frame_mpeg1(blob, g0, f, rate, 0x0E);
                if out.is_empty() {
                    skipped += 1;
                    continue;
                }
                mp3.extend_from_slice(&out);
            }
        }
        if mp3.is_empty() {
            return None;
        }
    } else {
        return None;
    }
    Some((mp3, ver))
}
/// Decode an EALayer3 RAM record to interleaved i16 PCM (MPEG1/2).
/// Applies the standard first-frame discard (529 + 576 samples) and trims
/// to the header sample count. Supports mono/stereo.
pub fn decode_eal3_record(blob: &[u8]) -> Option<(Vec<i16>, usize, u32)> {
    let h = parse_snr_header(blob)?;
    if h.codec != SnrCodec::EaLayer3V1 || !h.is_ram {
        return None;
    }
    if h.channels > 2 || h.num_samples == 0 || h.num_samples > 100_000_000 {
        return None;
    }
    let (mp3, _) = record_to_mp3_cbr(blob, h.sample_rate)?;
    let (_, samples) = puremp3::read_mp3(&mp3[..]).ok()?;
    let mut left: Vec<f32> = Vec::new();
    let mut right: Vec<f32> = Vec::new();
    for (l, r) in samples {
        left.push(l);
        right.push(r);
    }
    if left.is_empty() {
        return None;
    }
    // First-frame discard, then trim/pad to the header count.
    const DISCARD: usize = 529 + 576;
    let mut l = if left.len() > DISCARD {
        left[DISCARD..].to_vec()
    } else {
        left
    };
    let mut r = if right.len() > DISCARD {
        right[DISCARD..].to_vec()
    } else {
        right
    };
    l.truncate(h.num_samples);
    r.truncate(h.num_samples);
    while l.len() < h.num_samples {
        l.push(0.0);
        r.push(0.0);
    }
    let mut pcm = Vec::with_capacity(h.num_samples * h.channels);
    for i in 0..h.num_samples {
        let li = ((l[i] * 32767.0).clamp(-32768.0, 32767.0)) as i16;
        if h.channels == 1 {
            pcm.push(li);
        } else {
            let ri = ((r[i] * 32767.0).clamp(-32768.0, 32767.0)) as i16;
            pcm.push(li);
            pcm.push(ri);
        }
    }
    Some((pcm, h.channels, h.sample_rate))
}

// ---------------------------------------------------------------------------
// Complete SNR/SNS decoding (RAM, streamed and gigasample layouts)
// ---------------------------------------------------------------------------
//
// Verified against retail records (Spore_Audio1/2.package):
// - header: 8 bytes, +4 loop start when the loop bit is set, +4 SNS loop
//   offset for looped streams, +4 prefetch sample count for gigasamples;
// - data: `[flag:u8][size:u24 BE][samples:u32 BE][payload]` blocks (size
//   counts the 8 header bytes, flag 0x80 = last block). RAM records keep
//   every block in the SNR, streams keep them in the SNS record with the
//   same instance ID, gigasamples keep a prefetch in the SNR and the rest in
//   the SNS;
// - XAS1 payloads interleave one 0x4C frame per channel; EALayer3 payloads
//   are whole frames (rebuilt to standard MPEG and decoded with puremp3).

/// Where an SNR record keeps its audio blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnrStorage {
    /// Every block follows the header inside the SNR record.
    Ram,
    /// Header only; every block lives in the matching SNS record.
    Stream,
    /// A prefetch follows the header; the remainder streams from the SNS.
    Gigasample,
}

#[derive(Debug, Clone)]
pub struct SnrLayout {
    pub header: SnrHeader,
    pub storage: SnrStorage,
    pub loop_start: Option<u32>,
    /// Offset of the first SNR-resident block (RAM data or prefetch).
    pub data_offset: usize,
}

fn be32(blob: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(blob.get(at..at + 4)?.try_into().ok()?))
}

pub fn parse_snr_layout(blob: &[u8]) -> Option<SnrLayout> {
    let header = parse_snr_header(blob)?;
    let h2 = be32(blob, 4)?;
    let storage = match (h2 >> 30) & 3 {
        0 => SnrStorage::Ram,
        1 => SnrStorage::Stream,
        2 => SnrStorage::Gigasample,
        _ => return None,
    };
    let looped = (h2 >> 29) & 1 == 1;
    let mut offset = 8;
    let loop_start = if looped {
        offset += 4;
        Some(be32(blob, 8)?)
    } else {
        None
    };
    match storage {
        SnrStorage::Stream if looped => offset += 4,
        SnrStorage::Gigasample => offset += 4,
        _ => {}
    }
    if storage != SnrStorage::Stream && offset >= blob.len() {
        return None;
    }
    Some(SnrLayout {
        header,
        storage,
        loop_start,
        data_offset: offset,
    })
}

/// One EAAC data block inside an SNR or SNS record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EaacBlock {
    /// First payload byte (after the 8-byte block header).
    pub payload: usize,
    /// One past the last payload byte.
    pub end: usize,
    /// Samples per channel this block decodes to.
    pub samples: u32,
}

/// Walk `[flag|size][samples][payload]` blocks from `start` until the last
/// block flag, the end of the record or a malformed size.
pub fn eaac_blocks(blob: &[u8], start: usize) -> Vec<EaacBlock> {
    let mut out = Vec::new();
    let mut pos = start;
    while let (Some(word), Some(samples)) = (be32(blob, pos), be32(blob, pos + 4)) {
        let flag = (word >> 24) as u8;
        let size = (word & 0x00FF_FFFF) as usize;
        if size < 8 || pos + size > blob.len() {
            break;
        }
        out.push(EaacBlock {
            payload: pos + 8,
            end: pos + size,
            samples,
        });
        pos += size;
        if flag & 0x80 != 0 || out.len() > 1_000_000 {
            break;
        }
    }
    out
}

/// Interleaved 16-bit PCM decoded from an SNR (and SNS) pair.
#[derive(Debug, Clone, Default)]
pub struct DecodedSound {
    pub samples: Vec<i16>,
    pub channels: usize,
    pub rate: u32,
}

impl DecodedSound {
    /// Samples per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
    pub fn secs(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
    pub fn to_wav(&self) -> Vec<u8> {
        write_wav(&self.samples, self.channels, self.rate)
    }
    /// Linear fade over the first and last `secs` (seam-free looping of a
    /// window cut out of a longer track).
    pub fn fade_edges(&mut self, secs: f32) {
        let ch = self.channels.max(1);
        let frames = self.frames();
        let n = ((secs * self.rate as f32) as usize).min(frames / 2);
        for i in 0..n {
            let g = i as f32 / n as f32;
            for c in 0..ch {
                for at in [i * ch + c, (frames - 1 - i) * ch + c] {
                    self.samples[at] = (self.samples[at] as f32 * g) as i16;
                }
            }
        }
    }
    /// Peak absolute sample (0 = digital silence).
    pub fn peak(&self) -> i32 {
        self.samples
            .iter()
            .map(|s| (*s as i32).abs())
            .max()
            .unwrap_or(0)
    }
}

/// Which samples to decode: `start` frames in, at most `len` frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeWindow {
    pub start: usize,
    pub len: usize,
}

/// MPEG decoders output 529 samples of delay plus one warm-up granule.
const EAL3_DISCARD: usize = 529 + 576;

/// Accumulates standard MPEG frames rebuilt from EALayer3 frames.
#[derive(Default)]
struct Mp3Builder {
    mp3: Vec<u8>,
    frames: usize,
    skipped: usize,
    reservoir_frames: Vec<(Vec<u8>, usize)>,
}

impl Mp3Builder {
    /// Keep each complete logical main-data payload. Oversized EAL frames are
    /// packed into the MPEG bit reservoir during `finish_reservoir`.
    fn push_free_format(
        &mut self,
        frame: Vec<u8>,
        version_index: u32,
        rate: u32,
        payload_bits: usize,
    ) {
        if frame.len() < 4 {
            self.skipped += 1;
            return;
        }
        let _ = (version_index, rate);
        self.reservoir_frames.push((frame, payload_bits));
        self.frames += 1;
    }

    /// Pack the stored payloads into fixed maximum-bitrate MPEG frames, using
    /// main_data_begin to reach into prior frames' unused main-data slots.
    fn finish_reservoir(&mut self, rate: u32) -> Result<(), String> {
        let mut physical_starts = Vec::with_capacity(self.reservoir_frames.len());
        let mut capacities = Vec::with_capacity(self.reservoir_frames.len());
        let mut total_capacity = 0usize;
        for (frame, _) in &self.reservoir_frames {
            let version = ((frame[1] >> 3) & 3) as u32;
            let mode = (frame[3] >> 6) & 3;
            let channels = if mode == 3 { 1 } else { 2 };
            let side_info = match (version, channels) {
                (3, 1) => 17,
                (3, 2) => 32,
                (2 | 0, 1) => 9,
                (2 | 0, 2) => 17,
                _ => return Err(format!("unsupported MPEG version {version}")),
            };
            let frame_size = mpeg_frame_size(version, rate, 14)
                .ok_or_else(|| format!("no maximum frame size for MPEG version {version}"))?;
            let capacity = frame_size.saturating_sub(4 + side_info);
            physical_starts.push(total_capacity);
            capacities.push((capacity, side_info, frame_size, version));
            total_capacity = total_capacity.saturating_add(capacity);
        }

        let mut tape = vec![0u8; total_capacity];
        let mut logical_end = 0usize;
        let mut back_pointers = Vec::with_capacity(self.reservoir_frames.len());
        for (i, (frame, payload_bits)) in self.reservoir_frames.iter().enumerate() {
            let (capacity, side_info, _, version) = capacities[i];
            let physical_start = physical_starts[i];
            let payload_len = payload_bits.div_ceil(8);
            let source_start = 4 + side_info;
            let source_end = source_start.saturating_add(payload_len);
            let payload = frame
                .get(source_start..source_end)
                .ok_or_else(|| format!("rebuilt MPEG payload {i} is truncated"))?;
            let max_back = if version == 3 { 511 } else { 255 };
            let start = logical_end.max(physical_start.saturating_sub(max_back));
            let end = start.saturating_add(payload_len);
            if start > physical_start || end > physical_start + capacity {
                return Err(format!(
                    "MPEG reservoir cannot fit frame {i}: payload={payload_len}, back={}, capacity={capacity}",
                    physical_start.saturating_sub(start)
                ));
            }
            tape[start..end].copy_from_slice(payload);
            logical_end = end;
            back_pointers.push(physical_start - start);
        }

        let mut output = Vec::with_capacity(self.reservoir_frames.len() * 522);
        for (i, (frame, _)) in self.reservoir_frames.iter().enumerate() {
            let (capacity, side_info, frame_size, version) = capacities[i];
            let physical_start = physical_starts[i];
            let source_start = 4 + side_info;
            let mut packed = frame[..source_start].to_vec();
            packed[2] = (packed[2] & 0x0F) | (14 << 4);
            let back = back_pointers[i];
            if version == 3 {
                packed[4] = (back >> 1) as u8;
                packed[5] = (packed[5] & 0x7F) | (((back & 1) as u8) << 7);
            } else {
                packed[4] = back as u8;
            }
            let data_end = physical_start + capacity;
            packed.extend_from_slice(&tape[physical_start..data_end]);
            packed.resize(frame_size, 0);
            output.extend_from_slice(&packed);
        }
        self.mp3 = output;
        self.reservoir_frames.clear();
        Ok(())
    }

    /// Rebuild every EALayer3 frame of one block. MPEG1 granules pair up
    /// (granule 0 then 1); `pending` carries an unpaired granule 0 across
    /// blocks of the same record.
    fn push_block(
        &mut self,
        blob: &[u8],
        block: &EaacBlock,
        rate: u32,
        prefix: usize,
        pending: &mut Option<EalFrame>,
    ) {
        let mut pos = block.payload;
        while pos < block.end {
            // Codec 1 puts one byte in front of every frame.
            let at = pos + prefix;
            let Some(f) = parse_eal_frame(blob, at) else {
                break;
            };
            if f.size == 0 || at + f.size > block.end {
                break;
            }
            pos = at + f.size;
            match f.version_index {
                2 => {
                    let out = rebuild_mpeg_frame_cbr(blob, &f, rate, 0);
                    self.push_free_format(
                        out,
                        2,
                        rate,
                        f.main_data_bits.iter().sum::<u32>() as usize,
                    );
                }
                3 => {
                    if f.granule == 0 {
                        *pending = Some(f);
                    } else if let Some(g0) = pending.take() {
                        if g0.sample_rate_index == f.sample_rate_index
                            && g0.channel_mode == f.channel_mode
                        {
                            let out = rebuild_mpeg_frame_mpeg1(blob, &g0, &f, rate, 0);
                            let bits = g0
                                .main_data_bits
                                .iter()
                                .chain(&f.main_data_bits)
                                .sum::<u32>() as usize;
                            self.push_free_format(out, 3, rate, bits);
                        } else {
                            self.skipped += 1;
                        }
                    }
                }
                _ => self.skipped += 1,
            }
        }
    }
}

/// Whether a block begins with an MPEG frame that can seed an empty reservoir.
/// The returned payload length is checked against the largest legal main-data
/// slot for that version and channel count.
fn block_starts_with_reservoir_seed(blob: &[u8], block: &EaacBlock, rate: u32, prefix: usize) -> bool {
    let mut pos = block.payload;
    let mut pending: Option<EalFrame> = None;
    while pos < block.end {
        let at = pos + prefix;
        let Some(frame) = parse_eal_frame(blob, at) else {
            return false;
        };
        if frame.size == 0 || at + frame.size > block.end {
            return false;
        }
        pos = at + frame.size;
        if !frame.has_common {
            continue;
        }
        let (version, channels, bits) = match frame.version_index {
            2 => (
                2,
                frame.channels,
                frame.main_data_bits.iter().sum::<u32>() as usize,
            ),
            3 if frame.granule == 0 => {
                pending = Some(frame);
                continue;
            }
            3 => {
                let Some(g0) = pending.take() else {
                    continue;
                };
                if g0.sample_rate_index != frame.sample_rate_index
                    || g0.channel_mode != frame.channel_mode
                {
                    continue;
                }
                (
                    3,
                    frame.channels,
                    g0.main_data_bits
                        .iter()
                        .chain(&frame.main_data_bits)
                        .sum::<u32>() as usize,
                )
            }
            _ => continue,
        };
        let side_info = match (version, channels) {
            (3, 1) => 17,
            (3, 2) => 32,
            (2 | 0, 1) => 9,
            (2 | 0, 2) => 17,
            _ => return false,
        };
        let Some(frame_size) = mpeg_frame_size(version, rate, 14) else {
            return false;
        };
        return bits.div_ceil(8) <= frame_size.saturating_sub(4 + side_info);
    }
    false
}

/// Decode an SNR record (plus its SNS record for streamed/gigasample audio)
/// to PCM. `window` limits the work for long music: blocks entirely outside
/// it are skipped without decoding.
pub fn decode_snr_sound(
    snr: &[u8],
    sns: Option<&[u8]>,
    window: Option<DecodeWindow>,
) -> Result<DecodedSound, String> {
    let layout = parse_snr_layout(snr).ok_or("not an EA SNR v0 header")?;
    let h = &layout.header;
    if h.channels > 2 {
        return Err(format!("{} channels unsupported", h.channels));
    }
    // (source 0 = SNR, 1 = SNS, block)
    let mut blocks: Vec<(u8, EaacBlock)> = Vec::new();
    if layout.storage != SnrStorage::Stream {
        blocks.extend(
            eaac_blocks(snr, layout.data_offset)
                .into_iter()
                .map(|b| (0, b)),
        );
    }
    if layout.storage != SnrStorage::Ram {
        let sns = sns.ok_or("streamed sound needs its SNS record")?;
        blocks.extend(eaac_blocks(sns, 0).into_iter().map(|b| (1, b)));
    }
    if blocks.is_empty() {
        return Err("no audio blocks".into());
    }
    let wanted_total = h.num_samples;
    if wanted_total == 0 {
        return Err("empty sound".into());
    }
    let (want_start, want_len) = match window {
        Some(w) => (w.start.min(wanted_total.saturating_sub(1)), w.len.max(1)),
        None => (0, wanted_total),
    };
    // Keep blocks overlapping the window (one extra block of MPEG warm-up).
    let warmup = if h.codec.is_eal3() {
        EAL3_DISCARD + 1152
    } else {
        0
    };
    let keep_until = want_start.saturating_add(want_len).saturating_add(warmup);
    let mut kept = Vec::new();
    let mut origin = None;
    let mut pos = 0usize;
    let mut first_kept_block = None;
    for (block_index, (src, b)) in blocks.iter().enumerate() {
        let end = pos + b.samples as usize;
        if end + warmup > want_start && pos < keep_until {
            origin.get_or_insert(pos);
            first_kept_block.get_or_insert(block_index);
            kept.push((*src, *b));
        }
        pos = end;
        if pos >= keep_until {
            break;
        }
    }
    let mut origin = origin.ok_or("window outside the sound")?;
    let blob_of = |src: u8| if src == 0 { snr } else { sns.unwrap_or(&[]) };
    if h.codec.is_eal3() {
        let prefix = h.codec.frame_prefix();
        let mut block_index = first_kept_block.ok_or("window outside the sound")?;
        let mut preroll_frames = 0usize;
        while !block_starts_with_reservoir_seed(blob_of(kept[0].0), &kept[0].1, h.sample_rate, prefix) {
            if block_index == 0 {
                return Err("no EALayer3 block can seed the MPEG reservoir".into());
            }
            block_index -= 1;
            let (src, block) = blocks[block_index];
            preroll_frames = preroll_frames.saturating_add((block.samples as usize).div_ceil(576));
            if preroll_frames > 64 {
                return Err("EALayer3 reservoir preroll exceeded 64 frames".into());
            }
            origin = origin.saturating_sub(block.samples as usize);
            kept.insert(0, (src, block));
        }
    }
    let mut chans: Vec<Vec<i16>> = vec![Vec::new(); h.channels];
    let skip = match h.codec {
        SnrCodec::Xas1 => {
            for (src, b) in &kept {
                let blob = blob_of(*src);
                let step = 0x4C * h.channels;
                let frames = (b.end - b.payload) / step;
                let mut block_chans: Vec<Vec<i16>> = vec![Vec::new(); h.channels];
                for f in 0..frames {
                    for (c, out) in block_chans.iter_mut().enumerate() {
                        let at = b.payload + (f * h.channels + c) * 0x4C;
                        decode_xas_frame(&blob[at..at + 0x4C], out);
                    }
                }
                for (c, mut out) in block_chans.into_iter().enumerate() {
                    out.truncate(b.samples as usize);
                    chans[c].extend_from_slice(&out);
                }
            }
            want_start - origin
        }
        SnrCodec::EaLayer3V1 | SnrCodec::EaLayer3V0 => {
            let prefix = h.codec.frame_prefix();
            let mut builder = Mp3Builder::default();
            let mut pending = None;
            let mut last_src = None;
            for (src, b) in &kept {
                if last_src != Some(*src) {
                    pending = None; // granule pairs never span records
                    last_src = Some(*src);
                }
                builder.push_block(blob_of(*src), b, h.sample_rate, prefix, &mut pending);
            }
            builder.finish_reservoir(h.sample_rate)?;
            if builder.mp3.is_empty() {
                return Err(format!(
                    "no decodable EALayer3 frames ({} skipped)",
                    builder.skipped
                ));
            }
            let (_, frames) =
                puremp3::read_mp3(&builder.mp3[..]).map_err(|e| format!("mp3: {e:?}"))?;
            for (l, r) in frames {
                let l = (l * 32767.0).clamp(-32768.0, 32767.0) as i16;
                chans[0].push(l);
                if h.channels == 2 {
                    chans[1].push((r * 32767.0).clamp(-32768.0, 32767.0) as i16);
                }
            }
            want_start - origin + EAL3_DISCARD
        }
        SnrCodec::Xas0 => {
            let step = xas0_frame_bytes(h.channels);
            for (src, b) in &kept {
                let blob = blob_of(*src);
                let frames = (b.end - b.payload) / step;
                let mut block_chans: Vec<Vec<i16>> = vec![Vec::new(); h.channels];
                for f in 0..frames {
                    let at = b.payload + f * step;
                    decode_xas0_frame(&blob[at..at + step], h.channels, &mut block_chans);
                }
                for (c, mut out) in block_chans.into_iter().enumerate() {
                    out.truncate(b.samples as usize);
                    chans[c].extend_from_slice(&out);
                }
            }
            want_start - origin
        }
        SnrCodec::Pcm16Be => {
            for (src, b) in &kept {
                let blob = blob_of(*src);
                let frames = ((b.end - b.payload) / (2 * h.channels)).min(b.samples as usize);
                for f in 0..frames {
                    for (c, out) in chans.iter_mut().enumerate() {
                        let at = b.payload + (f * h.channels + c) * 2;
                        out.push(i16::from_be_bytes([blob[at], blob[at + 1]]));
                    }
                }
            }
            want_start - origin
        }
        SnrCodec::Other(c) => return Err(format!("codec {c} unsupported")),
    };
    let available = chans[0].len().saturating_sub(skip);
    let len = want_len.min(available).min(wanted_total - want_start);
    if len == 0 {
        return Err("decoded no samples".into());
    }
    let mut samples = Vec::with_capacity(len * h.channels);
    for i in skip..skip + len {
        for ch in &chans {
            samples.push(ch[i]);
        }
    }
    Ok(DecodedSound {
        samples,
        channels: h.channels,
        rate: h.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snr_header_xas() {
        // h1: ver=0 codec=4 chancfg=0 rate=44100; h2: RAM, 15224 samples.
        let mut b = vec![0x04u8, 0x00, 0xAC, 0x44, 0x00, 0x00, 0x3B, 0x78];
        let h = parse_snr_header(&b).unwrap();
        assert_eq!(h.codec, SnrCodec::Xas1);
        assert_eq!(h.channels, 1);
        assert_eq!(h.sample_rate, 44100);
        assert_eq!(h.num_samples, 15224);
        assert!(h.is_ram);
        b[0] = 0x10; // version 1
        assert!(parse_snr_header(&b).is_none());
    }

    #[test]
    fn xas_silence_frame_decodes_silent() {
        // All-zero groups: hists 0, shift 0, coefs 0 -> all samples 0.
        let frame = vec![0u8; 0x4C];
        let mut out = Vec::new();
        decode_xas_frame(&frame, &mut out);
        assert_eq!(out.len(), 128);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn bit_reader_msb_order() {
        let buf = [0b10110000u8, 0b01010000u8];
        let mut br = BitReader::new(&buf, 0);
        assert_eq!(br.get(2), Some(0b10));
        assert_eq!(br.get(2), Some(0b11));
        assert_eq!(br.get(4), Some(0b0000));
        assert_eq!(br.get(8), Some(0b01010000));
        assert_eq!(br.get(1), None); // exhausted
    }

    #[test]
    fn eal_frame_parses_empty_common() {
        // flag 0x00 + common (ver=2 MPEG2, rate=0 22050, chan=3 mono,
        // modeext=0) + granule 0 + side (mds=0, o1=0, o2=0).
        // Bits: 8 + 1 + 63 = 72 -> pre=1, common=9, size=10.
        let mut b = vec![0x00u8];
        b.push(0b10_00_11_00); // ver=2 rate=0 chan=3 modeext=0
        b.extend_from_slice(&[0u8; 8]); // granule + side info zeros
        assert_eq!(b.len(), 10);
        let f = parse_eal_frame(&b, 0).expect("parse");
        assert_eq!(f.channels, 1);
        assert_eq!(f.size, 10);
        assert_eq!(f.main_data_bits, vec![0]);
        assert_eq!(f.pcm_samples, 0);
        // Bad flag rejected; empty common rejected.
        assert!(parse_eal_frame(&[0x26u8; 10], 0).is_none());
        assert!(parse_eal_frame(&[0x00u8; 10], 0).is_none());
    }

    #[test]
    fn mpeg_frame_size_table() {
        assert_eq!(mpeg_frame_size(2, 22050, 0x0E), Some(522));
        assert_eq!(mpeg_frame_size(3, 44100, 0x0E), Some(1044));
        assert_eq!(mpeg_frame_size(2, 22050, 0x00), None); // free: no size
        assert_eq!(mpeg_frame_size(1, 22050, 0x0E), None); // reserved version
    }

    /// Synthetic MPEG1 mono granule pair rebuilds to an FF FB frame.
    /// Everything flows through BitWriter so bit alignment is exact.
    fn mpeg1_pair_bytes() -> Vec<u8> {
        // granule0: flag + common(ver3/44100/mono) + gran0 + side(mds=16).
        let mut g0 = BitWriter::new();
        g0.put(8, 0x00);
        g0.put(2, 3);
        g0.put(2, 0);
        g0.put(2, 3);
        g0.put(2, 0);
        g0.put(1, 0);
        g0.put(12, 16);
        g0.put(32, 0);
        g0.put(15, 0);
        g0.put(8, 0xAB);
        g0.put(8, 0xAB);
        g0.align_zero();
        // granule1: flag + common + gran1 + scfsi + side(mds=16).
        let mut g1 = BitWriter::new();
        g1.put(8, 0x00);
        g1.put(2, 3);
        g1.put(2, 0);
        g1.put(2, 3);
        g1.put(2, 0);
        g1.put(1, 1);
        g1.put(4, 0);
        g1.put(12, 16);
        g1.put(32, 0);
        g1.put(15, 0);
        g1.put(8, 0xCD);
        g1.put(8, 0xCD);
        g1.align_zero();
        [g0.buf, g1.buf].concat()
    }

    #[test]
    fn mpeg1_pair_rebuilds() {
        let blob = mpeg1_pair_bytes();
        eprintln!(
            "blob len={} bytes={}",
            blob.len(),
            blob.iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join("")
        );
        let frames = walk_eal_frames(&blob, 0).expect("walk pair");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].granule, 0);
        assert_eq!(frames[1].granule, 1);
        assert_eq!(frames[1].scfsi, vec![0]);
        let mp3 = rebuild_mpeg_frame_mpeg1(&blob, &frames[0], &frames[1], 44100, 0x0E);
        assert_eq!(&mp3[..4], &[0xFF, 0xFB, 0xE0, 0xCC]);
        assert_eq!(mp3.len(), 1044);
        // Mismatched granules refuse.
        assert!(rebuild_mpeg_frame_mpeg1(&blob, &frames[0], &frames[0], 44100, 0x0E).is_empty());
    }

    #[test]
    fn bit_writer_roundtrip() {
        // 64-bit pattern through mixed-size puts must come back byte-exact.
        let mut w = BitWriter::new();
        w.put(11, 0x7FF);
        w.put(2, 0b10);
        w.put(2, 0b01);
        w.put(1, 1);
        w.put(4, 0b1010);
        w.put(12, 0xABC);
        w.put(32, 0xDEADBEEF);
        assert_eq!(w.buf, vec![0xFF, 0xF3, 0xAA, 0xBC, 0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn eal_rebuild_emits_mpeg_sync() {
        // Same empty-common frame: header FF F9 00 CC + 9 zero side bytes.
        let mut b = vec![0x00u8];
        b.push(0b10_00_11_00);
        b.extend_from_slice(&[0u8; 8]);
        // Pad the blob so main-data reads stay in bounds (mds=0: unused).
        b.extend_from_slice(&[0u8; 8]);
        let f = parse_eal_frame(&b, 0).expect("parse");
        let mp3 = rebuild_mpeg_frame(&b, &f);
        // Standard MPEG2 Layer III header (FF F3 = sync+MPEG2+L3+noCRC).
        assert_eq!(&mp3[..4], &[0xFF, 0xF3, 0x00, 0xCC]);
        assert_eq!(mp3.len(), 13);
    }

    /// Synthetic MPEG2 mono frame with known main-data bytes: the rebuild
    /// must copy them verbatim (catches data-offset bugs).
    fn mpeg2_data_bytes() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.put(8, 0x00); // flag
        w.put(2, 2); // MPEG2
        w.put(2, 0); // 22050
        w.put(2, 3); // mono
        w.put(2, 0);
        w.put(1, 0); // granule 0
        w.put(12, 16); // mds = 16 bits
        w.put(32, 0);
        w.put(19, 0);
        w.put(8, 0xDE);
        w.put(8, 0xAD); // 16 bits of main data
        w.align_zero();
        w.buf
    }

    /// MPEG channel mode 00 is valid stereo, even though every mode bit is
    /// clear. A common frame with version/rate/mode-extension set to zero is
    /// still recognized as empty only when its version index is also zero.
    fn mpeg2_stereo_mode_zero_bytes() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.put(8, 0x00); // flag
        w.put(2, 2); // MPEG2
        w.put(2, 0); // 22050
        w.put(2, 0); // stereo
        w.put(2, 0); // mode extension
        w.put(1, 0); // granule 0
        for _ in 0..2 {
            w.put(12, 0); // main data bits
            w.put(32, 0);
            w.put(19, 0);
        }
        w.align_zero();
        w.buf
    }

    #[test]
    fn eal_mode_zero_is_stereo_not_an_empty_frame() {
        let bytes = mpeg2_stereo_mode_zero_bytes();
        let frame = parse_eal_frame(&bytes, 0).expect("valid stereo frame");
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.channel_mode, 0);
        let mpeg = rebuild_mpeg_frame_cbr(&bytes, &frame, 22050, 0x0E);
        assert!(!mpeg.is_empty());
        assert_eq!(mpeg[3] & 0xC0, 0); // preserve MPEG stereo mode 00
    }

    #[test]
    fn mpeg_reservoir_packs_future_payload_into_previous_frame() {
        let payloads = [
            (0..400).map(|i| i as u8).collect::<Vec<_>>(),
            (0..600).map(|i| (i * 3) as u8).collect::<Vec<_>>(),
            (0..500).map(|i| (255 - i) as u8).collect::<Vec<_>>(),
        ];
        let mut builder = Mp3Builder::default();
        for payload in &payloads {
            let mut frame = vec![0xFF, 0xF3, 0x00, 0x40]; // MPEG2 stereo
            frame.resize(4 + 17, 0);
            frame.extend_from_slice(payload);
            builder.push_free_format(frame, 2, 22050, payload.len() * 8);
        }
        builder.finish_reservoir(22050).expect("pack reservoir");

        let frame_size = mpeg_frame_size(2, 22050, 14).unwrap();
        let data_offset = 4 + 17;
        assert_eq!(frame_size, 522);
        assert_eq!(builder.mp3.len(), frame_size * payloads.len());
        let physical_data: Vec<u8> = builder
            .mp3
            .chunks_exact(frame_size)
            .flat_map(|frame| frame[data_offset..].iter().copied())
            .collect();

        let mut physical_start = 0usize;
        for (i, payload) in payloads.iter().enumerate() {
            let frame = &builder.mp3[i * frame_size..(i + 1) * frame_size];
            let back = frame[4] as usize;
            let logical_start = physical_start - back;
            assert_eq!(
                &physical_data[logical_start..logical_start + payload.len()],
                payload,
                "logical payload for MPEG frame {i}"
            );
            physical_start += frame_size - data_offset;
        }
        assert_eq!(builder.mp3[frame_size + 4], 101);
    }

    #[test]
    fn eal_rebuild_copies_main_data() {
        let b = mpeg2_data_bytes();
        let f = parse_eal_frame(&b, 0).expect("parse data frame");
        assert_eq!(f.main_data_bits, vec![16]);
        let mp3 = rebuild_mpeg_frame_cbr(&b, &f, 22050, 0x0E);
        // Header(4) + side info, then the 2 main-data bytes verbatim.
        // Side: main_begin(8) + priv(1) + 12 + 32 + 19 = 72 bits = 9 bytes.
        assert_eq!(&mp3[4 + 9..4 + 9 + 2], &[0xDE, 0xAD]);
    }

    /// Synthetic copy of a real EE frame (MPEG1 stereo granule-1):
    /// scfsi 15/15, mds [2319, 1159], pcm 47 samples. Must total 649 bytes.
    fn ee_frame_bytes() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.put(8, 0xEE);
        w.put(2, 3); // MPEG1
        w.put(2, 0); // 44100
        w.put(2, 1); // stereo
        w.put(2, 2); // mode ext
        w.put(1, 1); // granule 1
        w.put(4, 15);
        w.put(4, 15);
        for mds in [2319u32, 1159u32] {
            w.put(12, mds);
            w.put(32, 0);
            w.put(15, 0);
        }
        for _ in 0..(2319 + 1159) {
            w.put(1, 0); // main data placeholder bits
        }
        w.align_zero();
        w.put(16, 0); // offset_samples
        w.put(16, 47); // pcm_samples
        w.put(32, 0); // unknown
        for _ in 0..47 * 2 * 2 {
            w.put(8, 0); // pcm data
        }
        w.buf
    }

    #[test]
    fn eal_ee_frame_sizes() {
        let b = ee_frame_bytes();
        assert_eq!(b.len(), 649, "synthetic EE frame size");
        assert_eq!(b[0], 0xEE);
        let f = parse_eal_frame(&b, 0).expect("parse EE");
        assert_eq!(f.size, 649);
        assert_eq!(f.pcm_samples, 47);
        assert_eq!(f.channels, 2);
        assert_eq!(f.granule, 1);
        assert_eq!(f.main_data_bits, vec![2319, 1159]);
    }

    /// SNR v0 header: codec, channels, rate, storage type, loop flag, samples.
    fn snr_header(
        codec: u32,
        channels: u32,
        rate: u32,
        kind: u32,
        looped: bool,
        n: u32,
    ) -> Vec<u8> {
        let h1 = (codec << 24) | ((channels - 1) << 18) | rate;
        let h2 = (kind << 30) | ((looped as u32) << 29) | n;
        [h1.to_be_bytes(), h2.to_be_bytes()].concat()
    }

    /// One XAS frame decoding to a constant `level` (0 or -8).
    fn xas_frame(level: i16) -> Vec<u8> {
        let mut frame = vec![0u8; 0x4C];
        if level != 0 {
            for g in 0..4 {
                frame[g * 4..g * 4 + 4].copy_from_slice(&[0x00, 0x00, 0x0C, 0x00]);
            }
            for b in frame.iter_mut().skip(16) {
                *b = 0x88;
            }
        }
        frame
    }

    fn block(last: bool, samples: u32, payload: &[u8]) -> Vec<u8> {
        let size = (payload.len() + 8) as u32 | if last { 0x8000_0000 } else { 0 };
        [&size.to_be_bytes()[..], &samples.to_be_bytes(), payload].concat()
    }

    #[test]
    fn snr_layout_header_sizes_match_retail_records() {
        let pad = vec![0u8; 16];
        let at = |kind, looped| {
            let mut b = snr_header(4, 2, 44100, kind, looped, 1000);
            b.extend_from_slice(&pad);
            parse_snr_layout(&b).map(|l| (l.storage, l.data_offset))
        };
        assert_eq!(at(0, false), Some((SnrStorage::Ram, 8)));
        assert_eq!(at(0, true), Some((SnrStorage::Ram, 12)));
        assert_eq!(at(1, true), Some((SnrStorage::Stream, 16)));
        assert_eq!(at(2, false), Some((SnrStorage::Gigasample, 12)));
        assert_eq!(at(2, true), Some((SnrStorage::Gigasample, 16)));
        // A header-only stream SNR (16 bytes, as retail) still parses.
        let stream = snr_header(5, 2, 44100, 1, true, 1000);
        let stream = [stream, vec![0; 8]].concat();
        assert_eq!(
            parse_snr_layout(&stream).unwrap().storage,
            SnrStorage::Stream
        );
    }

    #[test]
    fn xas_blocks_skip_the_block_header_and_interleave_frames_per_channel() {
        // Two frame pairs (L, R) = 256 samples per channel; L is -8, R silent.
        let payload = [xas_frame(-8), xas_frame(0), xas_frame(-8), xas_frame(0)].concat();
        let mut snr = snr_header(4, 2, 44100, 0, false, 256);
        snr.extend(block(true, 256, &payload));
        let sound = decode_snr_sound(&snr, None, None).expect("decode");
        assert_eq!(
            (sound.channels, sound.rate, sound.frames()),
            (2, 44100, 256)
        );
        let left: Vec<i16> = sound.samples.iter().step_by(2).copied().collect();
        let right: Vec<i16> = sound.samples.iter().skip(1).step_by(2).copied().collect();
        // Each 32-sample group starts with its two zero history samples.
        assert_eq!(&left[..4], &[0, 0, -8, -8]);
        assert!(left[2..32].iter().all(|&s| s == -8));
        assert!(right.iter().all(|&s| s == 0));
        // The legacy entry point now reads the same payload.
        let (pcm, ch, rate) = decode_xas_ram(&snr).expect("ram decode");
        assert_eq!((pcm, ch, rate), (sound.samples.clone(), 2, 44100));
        // A window starts mid-sound without decoding outside blocks.
        let w = decode_snr_sound(
            &snr,
            None,
            Some(DecodeWindow {
                start: 130,
                len: 10,
            }),
        )
        .unwrap();
        assert_eq!(w.frames(), 10);
        assert_eq!(w.samples[0], -8);
    }

    #[test]
    fn streamed_sounds_read_blocks_from_the_sns_record() {
        let snr = [snr_header(4, 1, 22050, 1, true, 256), vec![0; 8]].concat();
        assert!(decode_snr_sound(&snr, None, None).is_err());
        let sns = [
            block(false, 128, &xas_frame(-8)),
            block(true, 128, &xas_frame(0)),
        ]
        .concat();
        let sound = decode_snr_sound(&snr, Some(&sns), None).expect("stream decode");
        assert_eq!((sound.channels, sound.frames()), (1, 256));
        assert_eq!(sound.samples[2], -8);
        assert!(sound.samples[128..].iter().all(|&s| s == 0));
        assert_eq!(eaac_blocks(&sns, 0).len(), 2);
    }

    #[test]
    fn fade_edges_silences_the_seams() {
        let mut s = DecodedSound {
            samples: vec![1000; 200],
            channels: 2,
            rate: 100,
        };
        s.fade_edges(0.2);
        assert_eq!(s.samples[0], 0);
        assert_eq!(s.samples[199], 0);
        assert_eq!(s.samples[100], 1000);
        assert_eq!(s.peak(), 1000);
    }

    #[test]
    fn xas_dc_frame_matches_reference_math() {
        // Group header: coef idx 0, hists 0, shift 12; nibbles all 0x88
        // (-8 after sign-extend >> 12, no prediction) -> constant -8.
        let mut frame = vec![0u8; 0x4C];
        for g in 0..4 {
            frame[g * 4..g * 4 + 4].copy_from_slice(&[0x00, 0x00, 0x0C, 0x00]);
        }
        for i in 16..76 {
            frame[i] = 0x88;
        }
        let mut out = Vec::new();
        decode_xas_frame(&frame, &mut out);
        assert_eq!(out.len(), 128);
        // Each of the 4 groups contributes its 2 header history samples
        // (0, 0) followed by 30 payload samples (-8 here).
        for g in 0..4 {
            assert_eq!(out[g * 32], 0);
            assert_eq!(out[g * 32 + 1], 0);
            assert!(out[g * 32 + 2..g * 32 + 32].iter().all(|&s| s == -8));
        }
    }

    // ---- codecs 0, 1 and 2 ----

    /// A codec 0 stereo frame: channel 0 has coefficient index 0, history 256 / 512 and shift 12,
    /// every nibble 8 (-8 after the shift); channel 1 has coefficient index 1 (0.9375), history
    /// 48 / 64, shift 12 and all nibbles 0.
    fn xas0_stereo_frame() -> Vec<u8> {
        let mut f = Vec::new();
        // coefficient words (index | older history) of channel 0 then 1
        f.extend_from_slice(&(0x0100u16).to_le_bytes());
        f.extend_from_slice(&(0x0030u16 | 1).to_le_bytes());
        // shift words (shift | newer history)
        f.extend_from_slice(&(0x0200u16 | 12).to_le_bytes());
        f.extend_from_slice(&(0x0040u16 | 12).to_le_bytes());
        for _ in 0..15 {
            f.extend_from_slice(&[0x88, 0x00]); // one byte per channel
        }
        assert_eq!(f.len(), xas0_frame_bytes(2));
        f
    }

    #[test]
    fn xas0_headers_are_planar_and_rows_interleave_channels() {
        let mut chans = vec![Vec::new(), Vec::new()];
        decode_xas0_frame(&xas0_stereo_frame(), 2, &mut chans);
        assert_eq!((chans[0].len(), chans[1].len()), (32, 32));
        // Channel 0: no prediction, so the history then 30 samples of -8.
        assert_eq!(&chans[0][..2], &[256, 512]);
        assert!(chans[0][2..].iter().all(|&s| s == -8));
        // Channel 1: coefficient 1 (0.9375 of the previous sample), zero residual.
        assert_eq!(&chans[1][..2], &[48, 64]);
        assert_eq!(chans[1][2], 60); // 64 * 0.9375
        assert_eq!(chans[1][3], 56); // 60 * 0.9375 = 56.25
        assert!(chans[1][2..].windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn xas0_mono_group_equals_the_xas1_group_with_the_same_header() {
        // One codec 0 mono frame is a v1 group: header u32 = coefficient word | shift word << 16.
        let coef = 0x0A40u16 | 2;
        let shift = 0xFFA0u16 | 9;
        let rows: Vec<u8> = (0..15).map(|r| (r * 17 + 3) as u8).collect();
        let mut v0 = Vec::new();
        v0.extend_from_slice(&coef.to_le_bytes());
        v0.extend_from_slice(&shift.to_le_bytes());
        v0.extend_from_slice(&rows);
        assert_eq!(v0.len(), xas0_frame_bytes(1));
        let mut v1 = vec![0u8; 0x4C];
        v1[..4].copy_from_slice(&(coef as u32 | (shift as u32) << 16).to_le_bytes());
        for (r, b) in rows.iter().enumerate() {
            v1[16 + r * 4] = *b;
        }
        let mut a = vec![Vec::new()];
        decode_xas0_frame(&v0, 1, &mut a);
        let mut b = Vec::new();
        decode_xas_frame(&v1, &mut b);
        assert_eq!(a[0].len(), 32);
        assert_eq!(a[0][..], b[..32]);
    }

    #[test]
    fn codec_numbers_map_to_decoders() {
        let codec = |n: u32| {
            let b = snr_header(n, 1, 22050, 0, false, 10);
            parse_snr_header(&b).map(|h| h.codec)
        };
        assert_eq!(codec(0), Some(SnrCodec::Xas0));
        assert_eq!(codec(1), Some(SnrCodec::EaLayer3V0));
        assert_eq!(codec(2), Some(SnrCodec::Pcm16Be));
        assert_eq!(codec(4), Some(SnrCodec::Xas1));
        assert_eq!(codec(5), Some(SnrCodec::EaLayer3V1));
        assert_eq!(codec(3), Some(SnrCodec::Other(3)));
        for c in [SnrCodec::Xas0, SnrCodec::EaLayer3V0, SnrCodec::Pcm16Be, SnrCodec::Xas1, SnrCodec::EaLayer3V1] {
            assert!(c.supported(), "{c:?}");
        }
        assert!(!SnrCodec::Other(3).supported());
        assert_eq!(SnrCodec::Pcm16Be.number(), 2);
        assert_eq!(SnrCodec::EaLayer3V0.label(), "EAL3-0");
        assert_eq!(SnrCodec::Other(9).label(), "codec9");
        assert!(SnrCodec::EaLayer3V0.is_eal3() && !SnrCodec::Xas0.is_eal3());
    }

    #[test]
    fn xas0_records_decode_through_the_block_reader() {
        // One stereo frame = 32 samples per channel.
        let mut snr = snr_header(0, 2, 22050, 0, false, 32);
        snr.extend(block(true, 32, &xas0_stereo_frame()));
        let sound = decode_snr_sound(&snr, None, None).expect("decode codec 0");
        assert_eq!((sound.channels, sound.rate, sound.frames()), (2, 22050, 32));
        let left: Vec<i16> = sound.samples.iter().step_by(2).copied().collect();
        let right: Vec<i16> = sound.samples.iter().skip(1).step_by(2).copied().collect();
        assert_eq!(&left[..3], &[256, 512, -8]);
        assert_eq!(&right[..4], &[48, 64, 60, 56]);
        // The header sample count trims the last frame; a looped header (4 more bytes) parses too.
        let mut looped = snr_header(0, 2, 22050, 0, true, 20);
        looped.extend_from_slice(&0u32.to_be_bytes());
        looped.extend(block(true, 20, &xas0_stereo_frame()));
        let trimmed = decode_snr_sound(&looped, None, None).expect("decode looped codec 0");
        assert_eq!(trimmed.frames(), 20);
        assert_eq!(parse_snr_layout(&looped).unwrap().loop_start, Some(0));
        // The legacy RAM entry point reads it as well.
        assert!(decode_xas_ram(&snr).is_some());
    }

    #[test]
    fn pcm16_be_records_decode_interleaved_stereo() {
        let pairs: [(i16, i16); 3] = [(1, -1), (256, -256), (i16::MAX, i16::MIN)];
        let payload: Vec<u8> = pairs
            .iter()
            .flat_map(|(l, r)| [l.to_be_bytes(), r.to_be_bytes()].concat())
            .collect();
        let mut snr = snr_header(2, 2, 44100, 0, false, 3);
        snr.extend(block(false, 3, &payload));
        let sound = decode_snr_sound(&snr, None, None).expect("decode PCM");
        assert_eq!((sound.channels, sound.rate, sound.frames()), (2, 44100, 3));
        assert_eq!(sound.samples, vec![1, -1, 256, -256, i16::MAX, i16::MIN]);
        // A window skips frames without decoding them.
        let tail = decode_snr_sound(&snr, None, Some(DecodeWindow { start: 1, len: 2 })).unwrap();
        assert_eq!(tail.samples, vec![256, -256, i16::MAX, i16::MIN]);
    }

    fn rebuilt_frames(blob: &[u8], prefix: usize) -> Vec<(Vec<u8>, usize)> {
        let block = EaacBlock { payload: 0, end: blob.len(), samples: 1152 };
        let mut builder = Mp3Builder::default();
        let mut pending = None;
        builder.push_block(blob, &block, 22050, prefix, &mut pending);
        builder.reservoir_frames
    }

    #[test]
    fn eal3_v0_frame_prefix_is_skipped() {
        // The same two MPEG-2 frames with and without the codec 1 prefix byte rebuild identically.
        let frame = mpeg2_data_bytes();
        let plain = [frame.clone(), frame.clone()].concat();
        let prefixed = [vec![1u8], frame.clone(), vec![1u8], frame.clone()].concat();
        let a = rebuilt_frames(&plain, 0);
        let b = rebuilt_frames(&prefixed, 1);
        assert_eq!(a.len(), 2);
        assert_eq!(a, b);
        let seed = |blob: &[u8], prefix: usize| {
            let block = EaacBlock { payload: 0, end: blob.len(), samples: 1152 };
            block_starts_with_reservoir_seed(blob, &block, 22050, prefix)
        };
        assert!(seed(&plain, 0) && seed(&prefixed, 1));
        // Without the prefix the prefixed blob is not a frame sequence.
        assert!(rebuilt_frames(&prefixed, 0).is_empty());
    }

    // ---- installed records (need the game's audio packages) ----

    fn installed_library() -> Option<crate::game_audio::AudioLibrary> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        (library.count(crate::game_audio::TYPE_SNR) > 0).then_some(library)
    }

    /// Decode a sample of the installed library and check it against its header; returns the peak.
    fn check_installed(
        library: &crate::game_audio::AudioLibrary,
        sample: u32,
        samples: usize,
        channels: usize,
        rate: u32,
    ) -> Result<i32, String> {
        let mut rng = rand::rng();
        let sound = library.decode(sample, None, &mut rng)?;
        if (sound.channels, sound.rate) != (channels, rate) {
            return Err(format!(
                "{sample:08x}: {}ch {}Hz, header {channels}ch {rate}Hz",
                sound.channels, sound.rate
            ));
        }
        if sound.frames() != samples {
            return Err(format!("{sample:08x}: decoded {} frames, header {samples}", sound.frames()));
        }
        // A wrong layout shows as a stream of clipped samples (hot masters of short blips clip
        // a few percent of their samples after the lossy codec: the editor drag ticks do).
        let clipped = sound.samples.iter().filter(|s| s.unsigned_abs() >= 32767).count();
        if clipped * 10 > sound.samples.len() {
            return Err(format!("{sample:08x}: {clipped} of {} samples clipped", sound.samples.len()));
        }
        if clipped * 200 > sound.samples.len() {
            println!("note: {sample:08x} clips {clipped} of {} samples", sound.samples.len());
        }
        Ok(sound.peak())
    }

    /// Installed samples of every new codec (codec 0 mono / stereo, 1 mono / stereo, 2) decode to
    /// the header's length with a real signal. Census: `notes/audio-codec.md`.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_codec_0_1_2_samples_decode() {
        let library = installed_library().expect("Spore install with audio");
        // (name, samples, channels, rate, codec)
        let named: [(&str, usize, usize, u32, u8); 8] = [
            ("editor_check", 14353, 2, 22050, 0),
            ("editor_snap_sound", 17157, 2, 44100, 0),
            ("vmot_panicMamAIn4", 27648, 1, 48000, 0),
            ("amb_cold_wind_lp", 1588095, 2, 48000, 0),
            ("ui_ed_pos_preview", 18118, 2, 22050, 1),
            ("editor_category_switch_sound", 38796, 1, 22050, 1),
            ("building_part_size10", 15421, 1, 22050, 1),
            ("marimba_F#3-2", 56096, 2, 44100, 2),
        ];
        let mut failures = Vec::new();
        for (name, samples, channels, rate, codec) in named {
            let def = library.resolve(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            match check_installed(&library, def.samples[0], samples, channels, rate) {
                Ok(peak) if peak < 200 => failures.push(format!("{name} (codec {codec}): silent, peak {peak}")),
                Ok(_) => {}
                Err(e) => failures.push(format!("{name} (codec {codec}): {e}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A spread of the 771 installed samples the decoder could not read before codecs 0 / 1 / 2
    /// (the smallest and largest of every layout plus every 25th by instance id). Set
    /// `OPENSPORE_AUDIO_CENSUS_IDS` to a file of `id samples channels rate codec` lines
    /// (`notes/audio-codec/undecodable_ids.txt`) to decode all of them.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_codec_census_sample_decodes() {
        let library = installed_library().expect("Spore install with audio");
        // (instance, samples, channels, rate, codec)
        const SPREAD: [(u32, usize, usize, u32, u8); 40] = [
            (0x04D08248, 103424, 1, 48000, 0),  // vsp_drop_deathMamAIn15
            (0x118C2940, 66560, 1, 48000, 0),   // vmot_panicMamAIn24
            (0x148C2E1A, 42496, 1, 48000, 0),   // vmot_panicMamAIn17
            (0x218FE93A, 27648, 1, 48000, 0),   // vmot_panicMamAIn4
            (0x37187FD9, 103424, 1, 48000, 0),  // vsp_dropMamAIn6
            (0x3A69CB94, 74752, 1, 48000, 0),   // vidle_stamp_footMamAIn25
            (0x4932B2FB, 1529856, 1, 48000, 0), // egg_shake
            (0x5BA679A9, 226304, 1, 48000, 0),  // vidle_happy_dancesoloMamAIn12
            (0x5DA67CF9, 190464, 1, 48000, 0),  // vidle_happy_dancesoloMamAIn38
            (0xD4E8D469, 203776, 1, 48000, 0),  // vsp_drop_landonfeetMamAIn7
            (0x81AF1F63, 248587, 2, 48000, 0),  // ufo_level_terrain_ray
            (0xB33F14EA, 3936, 2, 48000, 0),    // shorty_e4
            (0xC436A177, 385054, 2, 44100, 0),  // 110-aphklofibt-lr
            (0xF5A7640D, 3517672, 2, 48000, 0), // amb_lava_lake_lp
            (0x21DE2A12, 17640, 1, 22050, 1),   // creature_part_morph_x24
            (0x23DE2D7D, 17640, 1, 22050, 1),   // creature_part_morph_x41
            (0x4881C593, 15435, 1, 22050, 1),   // creature_part_size23
            (0x4881C594, 15435, 1, 22050, 1),   // creature_part_size24
            (0x4A81C8FE, 15435, 1, 22050, 1),   // creature_part_size08
            (0x4D81CD4F, 15435, 1, 22050, 1),   // creature_part_size70
            (0x84A1FAA4, 15435, 1, 22050, 1),   // creature_spine_size02
            (0x88A200DB, 15435, 1, 22050, 1),   // creature_spine_size49
            (0x992E6261, 15803, 1, 22050, 1),   // vehicle_part_size19
            (0x9B2E654B, 14784, 1, 22050, 1),   // vehicle_part_size35
            (0x9E2E6A00, 14207, 1, 22050, 1),   // vehicle_part_size47
            (0xA02E6D6A, 15596, 1, 22050, 1),   // vehicle_part_size67
            (0xA5E138BA, 13366, 1, 22050, 1),   // creature_part_morph_y03
            (0xD748CBD0, 17640, 1, 22050, 1),   // building_part_morph_z29
            (0xD948CF3B, 17640, 1, 22050, 1),   // building_part_morph_z44
            (0xDB48D202, 17640, 1, 22050, 1),   // building_part_morph_z67
            (0xDCB4FC8E, 15421, 1, 22050, 1),   // building_part_size10
            (0xDF438C0A, 13932, 1, 22050, 1),   // vehicle_part_morph_z21
            (0xE1B504AC, 15275, 1, 22050, 1),   // building_part_size41
            (0xE5439594, 15425, 1, 22050, 1),   // vehicle_part_morph_z49
            (0xE745D75D, 15244, 1, 22050, 1),   // vehicle_part_morph_y07
            (0xF3DCC2EC, 38796, 1, 22050, 1),   // editor_category_switch_sound
            (0x84DC0FEC, 23557, 2, 22050, 1),   // ui_ed_pos_payoff
            (0xEC49C0D3, 18118, 2, 22050, 1),   // ui_ed_pos_preview
            (0x285EDFAF, 87590, 2, 44100, 2),   // marimba_F#2-2
            (0xA852DD1B, 32693, 2, 44100, 2),   // marimba_F#5-1
        ];
        let mut list: Vec<(u32, usize, usize, u32, u8)> = SPREAD.to_vec();
        if let Some(path) = std::env::var_os("OPENSPORE_AUDIO_CENSUS_IDS") {
            list.clear();
            let text = std::fs::read_to_string(path).expect("census id file");
            for line in text.lines() {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() == 5 {
                    list.push((
                        u32::from_str_radix(f[0], 16).unwrap(),
                        f[1].parse().unwrap(),
                        f[2].parse().unwrap(),
                        f[3].parse().unwrap(),
                        f[4].parse().unwrap(),
                    ));
                }
            }
        }
        let mut failures = Vec::new();
        let mut silent = 0;
        let mut per_codec = std::collections::BTreeMap::<u8, usize>::new();
        for (sample, samples, channels, rate, codec) in &list {
            match check_installed(&library, *sample, *samples, *channels, *rate) {
                Ok(peak) => {
                    *per_codec.entry(*codec).or_default() += 1;
                    if peak < 8 {
                        silent += 1;
                    }
                }
                Err(e) => failures.push(format!("codec {codec}: {e}")),
            }
        }
        println!("codec census decode: {} samples, per codec {per_codec:?}, near-silent {silent}", list.len());
        assert!(failures.is_empty(), "{} of {} failed: {failures:#?}", failures.len(), list.len());
    }
}
