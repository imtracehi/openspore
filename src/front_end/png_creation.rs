//! Creations hidden in their card PNGs ("model in picture"): the installed
//! Maxis creations' 128 x 128 PNGs carry the whole creation file in the low
//! bits of their pixels, the way retail Sporepedia stores and shares every
//! creation.
//!
//! VERIFIED (code, `notes\front-end-5-notes.md` section 2; the install's
//! PNGs decode with it):
//! * `Thumbnail_cImportExport::DecodePNG` 0x5FBB90 reads the image
//!   (`PngEncoder::ReadImageData` 0x68EB40: 24- or 32-bit pixels, rows top
//!   down, channels in memory order B, G, R, A), takes the payload size
//!   (0x68E660: an 8-byte header = magic 0x02450D16 + size, little endian;
//!   when the magic is wrong it retries with stream mask = !bit mask), reads
//!   the payload (0x68E3A0 / 0x68DFC0), checks the trailer (0x68E480: padding
//!   to the size, magic again, CRC-64 of the payload), then inflates it
//!   (zlib) and parses a text header.
//! * The bit reader (reset 0x68DDB0, byte 0x68DFC0): `max` = pixel buffer
//!   bytes, a Galois LFSR (taps from the table 0x1403688 indexed by the top
//!   bit of `max`, start = taps) walks the buffer bytes, skipping offsets >=
//!   `max`; each full LFSR cycle moves to the next bit plane (bit mask 1, 2,
//!   4; stream mask !(8 - 1) shifted with it). Per bit: `fnv = ((!stream &
//!   offset & 0xFF) | (byte & stream)) ^ fnv * 0x01000193` (start
//!   0x811C9DC5), data bit = (byte & bit mask != 0) xor (fnv & 0x8000 != 0),
//!   bits fill each byte from the top (LSB first).
//! * Header (0x5FBB90, text): `spore`, version (4 decimal digits, < 7), type
//!   (8 hex), group (8 hex, version >= 3), instance and machine id (8 hex
//!   each, version >= 4), asset id (16 hex), parent asset id (16 hex,
//!   version >= 6), time created (16 hex), author (2-hex length + text),
//!   author id (16 hex, version >= 2), name (2-hex length), description
//!   (3-hex length), tags (2-hex length), consequence traits (2-hex count x
//!   8 hex, version >= 5); then the creation file (`<sporemodel>` XML).
//!
//! Not yet used outside its tests: the base for opening Maxis creations in
//! the creators (their XML still has to become OpenSpore records).
#![cfg_attr(not(test), allow(dead_code))]

use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};

/// LFSR taps by the index of the buffer size's top bit (0x1403688).
const TAPS: [u32; 32] = [
    0, 0, 0x3, 0x6, 0xC, 0x14, 0x30, 0x60, 0xB8, 0x110, 0x240, 0x500, 0xCA0, 0x1B00, 0x3500, 0x6000, 0xB400, 0x1_2000, 0x2_0400,
    0x7_2000, 0x9_0000, 0x14_0000, 0x30_0000, 0x40_0000, 0xD8_0000, 0x120_0000, 0x388_0000, 0x720_0000, 0x900_0000, 0x1400_0000,
    0x3280_0000, 0x4800_0000,
];
pub const MAGIC: u32 = 0x0245_0D16;
/// `mnMaxMask`: at most three bit planes.
const MAX_MASK: u32 = 8;

/// The pixel bit reader (`PngEncoder` 0x68DDB0 / 0x68DFC0).
struct BitReader<'a> {
    buf: &'a [u8],
    max: u32,
    taps: u32,
    offset: u32,
    bit: u32,
    stream: u32,
    fnv: u32,
}

impl<'a> BitReader<'a> {
    fn new(buf: &'a [u8], stream: Option<u32>) -> Option<Self> {
        let max = u32::try_from(buf.len()).ok().filter(|m| *m >= 4)?;
        let taps = TAPS[(31 - max.leading_zeros()) as usize];
        Some(Self { buf, max, taps, offset: taps, bit: 1, stream: stream.unwrap_or(!(MAX_MASK - 1)), fnv: 0x811C_9DC5 })
    }

    fn step(&mut self) {
        self.offset = if self.offset & 1 != 0 { (self.offset >> 1) ^ self.taps } else { self.offset >> 1 };
    }

    fn byte(&mut self) -> Option<u8> {
        let mut out = 0u8;
        for _ in 0..8 {
            while self.offset >= self.max {
                self.step();
                if self.offset == self.taps {
                    self.bit <<= 1;
                    self.stream <<= 1;
                }
            }
            let v = u32::from(self.buf[self.offset as usize]);
            self.fnv = ((!self.stream & self.offset & 0xFF) | (v & self.stream)) ^ self.fnv.wrapping_mul(0x0100_0193);
            let mut b = if v & self.bit != 0 { 0x80 } else { 0 };
            if self.fnv & 0x8000 != 0 {
                b ^= 0x80;
            }
            out = (out >> 1) | b;
            self.step();
            if self.offset == self.taps {
                self.bit <<= 1;
                self.stream <<= 1;
                if self.bit == MAX_MASK {
                    return None;
                }
            }
        }
        Some(out)
    }

    fn read(&mut self, n: usize) -> Option<Vec<u8>> {
        (0..n).map(|_| self.byte()).collect()
    }
}

/// The pixel buffer as retail holds it (B, G, R, A per pixel) from RGBA8.
pub fn retail_pixels(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
}

/// The hidden payload (zlib data) of a pixel buffer, if it has one.
pub fn payload(pixels: &[u8]) -> Option<Vec<u8>> {
    for stream in [None, Some(!1u32)] {
        let mut r = BitReader::new(pixels, stream)?;
        let head = r.read(8)?;
        let magic = u32::from_le_bytes(head[0..4].try_into().ok()?);
        let size = u32::from_le_bytes(head[4..8].try_into().ok()?) as usize;
        if magic != MAGIC || size == 0 || size > 640_000 {
            continue;
        }
        return r.read(size);
    }
    None
}

/// What the text header says about the creation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    pub version: u32,
    pub type_id: u32,
    pub group_id: u32,
    pub instance_id: u32,
    pub machine_id: u32,
    pub asset_id: u64,
    pub parent_id: u64,
    pub time_created: u64,
    pub author: String,
    pub author_id: u64,
    pub name: String,
    pub description: String,
    pub tags: String,
    pub traits: Vec<u32>,
}

/// A decoded creation: the header and the creation file.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub header: Header,
    /// The creation file (`<sporemodel>` XML).
    pub model: String,
}

struct Text<'a> {
    b: &'a [u8],
    at: usize,
}

impl Text<'_> {
    fn take(&mut self, n: usize) -> Option<&str> {
        let s = std::str::from_utf8(self.b.get(self.at..self.at + n)?).ok()?;
        self.at += n;
        Some(s)
    }
    fn hex(&mut self, n: usize) -> Option<u64> {
        u64::from_str_radix(self.take(n)?, 16).ok()
    }
    fn string(&mut self, len_digits: usize) -> Option<String> {
        let n = self.hex(len_digits)? as usize;
        let bytes = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Parse the inflated payload (0x5FBB90).
pub fn parse(data: &[u8]) -> Option<Decoded> {
    let mut t = Text { b: data, at: 0 };
    if t.take(5)? != "spore" {
        return None;
    }
    let version: u32 = t.take(4)?.parse().ok()?;
    if version >= 7 {
        return None;
    }
    let mut h = Header { version, type_id: t.hex(8)? as u32, ..Default::default() };
    // Version 1 / 2 derive the group from the type (not seen in the install).
    h.group_id = if version >= 3 { t.hex(8)? as u32 } else { 0 };
    if version >= 4 {
        h.instance_id = t.hex(8)? as u32;
        h.machine_id = t.hex(8)? as u32;
    }
    h.asset_id = t.hex(16)?;
    h.parent_id = if version >= 6 { t.hex(16)? } else { u64::MAX };
    h.time_created = t.hex(16)?;
    h.author = t.string(2)?;
    h.author_id = if version >= 2 { t.hex(16)? } else { u64::MAX };
    h.name = t.string(2)?;
    h.description = t.string(3)?;
    h.tags = t.string(2)?;
    if version >= 5 {
        let n = t.hex(2)? as usize;
        for _ in 0..n {
            h.traits.push(t.hex(8)? as u32);
        }
    }
    let model = String::from_utf8_lossy(&data[t.at..]).into_owned();
    Some(Decoded { header: h, model })
}

/// Decode a creation PNG (file bytes).
pub fn decode_png(bytes: &[u8]) -> Option<Decoded> {
    let (rgba, _) = super::thumbs::decode_png(bytes)?;
    let zipped = payload(&retail_pixels(&rgba))?;
    parse(&zlib_inflate(&zipped)?)
}

/// The creation inside an installed creation PNG.
pub fn decode_installed(index: &ResourceIndex, png: ResourceKey) -> Option<Decoded> {
    decode_png(&index.read(png).ok()?)
}

// --- zlib / DEFLATE (RFC 1950 / 1951), a small canonical-Huffman inflater ---

/// Inflate a zlib stream (the payload's container).
pub fn zlib_inflate(data: &[u8]) -> Option<Vec<u8>> {
    let (cmf, flg) = (*data.first()?, *data.get(1)?);
    if cmf & 0x0F != 8 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 || flg & 0x20 != 0 {
        return None;
    }
    inflate(&data[2..])
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Option<u32> {
        while self.cnt < n {
            let b = *self.d.get(self.pos)?;
            self.pos += 1;
            self.buf |= u32::from(b) << self.cnt;
            self.cnt += 8;
        }
        let v = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.cnt -= n;
        Some(v)
    }
}

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Self {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Self { count, symbol }
    }

    fn decode(&self, bits: &mut Bits) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.need(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Inflate raw DEFLATE data.
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut bits = Bits { d: data, pos: 0, buf: 0, cnt: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.need(1)?;
        match bits.need(2)? {
            0 => {
                bits.buf = 0;
                bits.cnt = 0;
                let len = u16::from_le_bytes([*data.get(bits.pos)?, *data.get(bits.pos + 1)?]) as usize;
                bits.pos += 4;
                out.extend_from_slice(data.get(bits.pos..bits.pos + len)?);
                bits.pos += len;
            }
            1 => {
                let mut l = [0u8; 288];
                l[..144].fill(8);
                l[144..256].fill(9);
                l[256..280].fill(7);
                l[280..].fill(8);
                codes(&mut bits, &mut out, &Huffman::new(&l), &Huffman::new(&[5u8; 30]))?;
            }
            2 => {
                let nlen = bits.need(5)? as usize + 257;
                let ndist = bits.need(5)? as usize + 1;
                let ncode = bits.need(4)? as usize + 4;
                let mut cl = [0u8; 19];
                for &o in ORDER.iter().take(ncode) {
                    cl[o] = bits.need(3)? as u8;
                }
                let ch = Huffman::new(&cl);
                let mut lengths = vec![0u8; nlen + ndist];
                let mut i = 0;
                while i < nlen + ndist {
                    let sym = ch.decode(&mut bits)?;
                    let (val, rep) = match sym {
                        0..=15 => (sym as u8, 1),
                        16 => (*lengths.get(i.checked_sub(1)?)?, 3 + bits.need(2)? as usize),
                        17 => (0, 3 + bits.need(3)? as usize),
                        _ => (0, 11 + bits.need(7)? as usize),
                    };
                    for _ in 0..rep {
                        *lengths.get_mut(i)? = val;
                        i += 1;
                    }
                }
                codes(&mut bits, &mut out, &Huffman::new(&lengths[..nlen]), &Huffman::new(&lengths[nlen..]))?;
            }
            _ => return None,
        }
        if last == 1 {
            return Some(out);
        }
    }
}

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Option<()> {
    loop {
        let sym = lit.decode(bits)? as usize;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Some(());
        } else {
            let s = sym - 257;
            let len = *LBASE.get(s)? as usize + bits.need(u32::from(LEXT[s]))? as usize;
            let d = dist.decode(bits)? as usize;
            let back = *DBASE.get(d)? as usize + bits.need(u32::from(DEXT[d]))? as usize;
            let from = out.len().checked_sub(back)?;
            for k in 0..len {
                let b = out[from + k];
                out.push(b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflates_stored_and_fixed_blocks() {
        // zlib of "hello hello hello" with fixed Huffman codes and a back
        // reference (made by zlib.compress(b"hello hello hello", 9)).
        let z = [0x78, 0xDA, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x90, 0x00, 0x3A, 0x2E, 0x06, 0x7D];
        assert_eq!(zlib_inflate(&z).as_deref(), Some(&b"hello hello hello"[..]));
        // A stored block.
        let stored = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&stored).as_deref(), Some(&b"abc"[..]));
    }

    #[test]
    fn parses_the_text_header() {
        let text = b"spore00052b978c46406262008933cc090e06ab84ffffffffffffffff0000000ebfd9890d0asporebuildfffffffffffffffe09DivaBette0000000<?xml version=\"1.0\"?><sporemodel/>";
        let d = parse(text).expect("header");
        assert_eq!(d.header.version, 5);
        assert_eq!((d.header.type_id, d.header.group_id, d.header.instance_id), (0x2B97_8C46, 0x4062_6200, 0x8933_CC09));
        assert_eq!(d.header.author, "sporebuild");
        assert_eq!(d.header.name, "DivaBette");
        assert!(d.header.traits.is_empty());
        assert!(d.model.starts_with("<?xml") && d.model.contains("<sporemodel"));
    }

    #[test]
    fn the_bit_reader_round_trips_an_encoded_buffer() {
        // Encode with the inverse of the reader (same walk and keystream),
        // then read the bytes back.
        let mut buf: Vec<u8> = (0..64 * 64 * 4).map(|i| ((i * 7919) % 251) as u8 & !7).collect();
        let message: Vec<u8> = [MAGIC.to_le_bytes(), 3u32.to_le_bytes()].concat().into_iter().chain([1, 2, 3]).collect();
        {
            let max = buf.len() as u32;
            let taps = TAPS[(31 - max.leading_zeros()) as usize];
            let (mut offset, bit, stream, mut fnv) = (taps, 1u32, !(MAX_MASK - 1), 0x811C_9DC5u32);
            for &byte in &message {
                for k in 0..8 {
                    while offset >= max {
                        offset = if offset & 1 != 0 { (offset >> 1) ^ taps } else { offset >> 1 };
                    }
                    let v = u32::from(buf[offset as usize]);
                    fnv = ((!stream & offset & 0xFF) | (v & stream)) ^ fnv.wrapping_mul(0x0100_0193);
                    let want = (byte >> k) & 1 != 0;
                    let flip = fnv & 0x8000 != 0;
                    if want != flip {
                        buf[offset as usize] |= bit as u8;
                    }
                    offset = if offset & 1 != 0 { (offset >> 1) ^ taps } else { offset >> 1 };
                }
            }
        }
        assert_eq!(payload(&buf).as_deref(), Some(&[1u8, 2, 3][..]));
    }

    /// The installed Maxis creation PNGs hold their creation files.
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_maxis_pngs_decode_to_their_creations() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        for (group, type_id) in [
            (0x4062_6200, super::super::maxis::CRT),
            (0x4063_6200, crate::creation_records::BLD),
            (0x4064_6200, crate::creation_records::VCL),
            (0x4065_6200, crate::creation_records::UFO),
            (0x4061_6200, super::super::maxis::CLL),
            (0x4066_6200, super::super::maxis::FLR),
        ] {
            let mut keys: Vec<ResourceKey> =
                index.records.keys().filter(|k| k.type_id == super::super::maxis::PNG_TYPE && k.group_id == group).copied().collect();
            keys.sort_by_key(|k| k.instance_id);
            assert!(!keys.is_empty(), "{group:08X}");
            for key in keys.iter().take(4) {
                let d = decode_installed(&index, *key).unwrap_or_else(|| panic!("{key:?}"));
                assert_eq!(d.header.type_id, type_id, "{key:?}");
                assert_eq!(d.header.instance_id, key.instance_id, "{key:?}");
                assert!(d.model.contains("<sporemodel"), "{key:?}");
                if matches!(type_id, crate::creation_records::BLD | crate::creation_records::VCL | crate::creation_records::UFO) {
                    let record = crate::creation_records::parse_creation(
                        ResourceKey { type_id, group_id: key.group_id, instance_id: key.instance_id },
                        d.model.as_bytes(),
                    )
                    .unwrap_or_else(|e| panic!("{key:?}: {e}"));
                    assert!(!record.blocks.is_empty(), "{key:?}");
                }
            }
        }
    }
}
