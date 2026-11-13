//! Read-only DBPF packages and native DBBF save/cache archives for Spore.
//!
//! Spore stores almost all game data in `Data/*.package` files using the
//! DBPF v2 container. This module implements read-only parsing:
//! header -> bounded index (shared-field flags) -> records (+ QFS/RefPack).
//!
//! Reference: SimsWiki `Spore:DBPF` (DBPF magic, v2 header, index variants),
//! plus the public QFS/RefPack algorithm used by DBPFSharp / SporeModder-FX.
//!
//! We never write game files and never ship original assets (OpenRCT2 model).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DBPF_MAGIC: u32 = 0x4650_4244; // "DBPF" bytes 44 42 50 46 read as LE u32
pub const DBBF_MAGIC: u32 = 0x4642_4244; // Native save/cache files use 64-bit offsets.
pub const SUPPORTED_MAJORS: &[u32] = &[2, 3]; // SimsWiki documents v2; retail Spore ships v3.0

#[derive(Debug, Clone)]
pub struct DbpfHeader {
    pub is_dbbf: bool,
    pub major: u32,
    pub minor: u32,
    pub entry_count: u32,
    pub index_size: u64,
    pub index_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    pub type_id: u32,
    pub group_id: u32,
    pub instance_id: u32,
}

#[derive(Debug, Clone)]
pub struct DbpfEntry {
    pub key: ResourceKey,
    pub offset: u64,
    pub disk_size: u32,
    pub mem_size: u32,
    pub is_compressed: bool,
}

#[derive(Debug, Clone)]
pub struct PackageIndex {
    pub path: PathBuf,
    pub header: DbpfHeader,
    pub entries: Vec<DbpfEntry>,
}

fn read_u32_le<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u16_le<R: Read>(r: &mut R) -> std::io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}
fn read_u64_le<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

pub fn read_header(path: &Path) -> Result<DbpfHeader, String> {
    let mut f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let magic = read_u32_le(&mut f).map_err(|e| format!("read magic: {e}"))?;
    if magic != DBPF_MAGIC && magic != DBBF_MAGIC {
        return Err(format!(
            "not a DBPF/DBBF file (bad magic {:#X}): {}",
            magic,
            path.display()
        ));
    }
    let major = read_u32_le(&mut f).map_err(|e| e.to_string())?;
    let minor = read_u32_le(&mut f).map_err(|e| e.to_string())?;
    // Skip 6 dwords: ??? x3, created, modified, index_major
    let mut skip = [0u8; 6 * 4];
    f.read_exact(&mut skip).map_err(|e| e.to_string())?;
    let entry_count = read_u32_le(&mut f).map_err(|e| e.to_string())?;
    let is_dbbf = magic == DBBF_MAGIC;
    let (index_size, index_offset) = if is_dbbf {
        let size = read_u64_le(&mut f).map_err(|e| e.to_string())?;
        let mut reserved = [0u8; 8];
        f.read_exact(&mut reserved).map_err(|e| e.to_string())?;
        (size, read_u64_le(&mut f).map_err(|e| e.to_string())?)
    } else {
        let _old_index_offset = read_u32_le(&mut f).map_err(|e| e.to_string())?;
        let size = read_u32_le(&mut f).map_err(|e| e.to_string())? as u64;
        let mut hole = [0u8; 16];
        f.read_exact(&mut hole).map_err(|e| e.to_string())?;
        (size, read_u32_le(&mut f).map_err(|e| e.to_string())? as u64)
    };
    // Remaining header (padding) is ignored.
    Ok(DbpfHeader {
        is_dbbf,
        major,
        minor,
        entry_count,
        index_size,
        index_offset,
    })
}

/// Read only the index (no record data). Fast enough to scan all packages at startup.
pub fn read_package_index(path: &Path) -> Result<PackageIndex, String> {
    let header = read_header(path)?;
    if !SUPPORTED_MAJORS.contains(&header.major) {
        return Err(format!(
            "{}: unsupported DBPF major {} (expected 2 or 3)",
            path.display(),
            header.major
        ));
    }
    let mut f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let file_size = f.metadata().map_err(|e| e.to_string())?.len();
    if header.entry_count > 1_000_000
        || header.index_size > 64 * 1024 * 1024
        || header
            .index_offset
            .checked_add(header.index_size)
            .is_none_or(|end| end > file_size)
    {
        return Err("package index exceeds file/count/byte bounds".into());
    }
    if header.entry_count == 0 && header.index_size == 0 {
        return Ok(PackageIndex {
            path: path.to_path_buf(),
            header,
            entries: vec![],
        });
    }
    f.seek(SeekFrom::Start(header.index_offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; header.index_size as usize];
    f.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let mut input = std::io::Cursor::new(bytes);
    let flags = read_u32_le(&mut input).map_err(|e| e.to_string())?;
    if flags & !7 != 0 {
        return Err(format!("unsupported package index flags {flags}"));
    }
    let common_type = if flags & 1 != 0 {
        Some(read_u32_le(&mut input).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let common_group = if flags & 2 != 0 {
        Some(read_u32_le(&mut input).map_err(|e| e.to_string())?)
    } else {
        None
    };
    if flags & 4 != 0 {
        read_u32_le(&mut input).map_err(|e| e.to_string())?;
    }
    let record_size = 16u64
        + if header.is_dbbf { 8 } else { 4 }
        + if common_type.is_none() { 4 } else { 0 }
        + if common_group.is_none() { 4 } else { 0 };
    if input.position() + record_size * header.entry_count as u64 > header.index_size {
        return Err("package index record count exceeds declared index size".into());
    }
    let mut entries = Vec::with_capacity(header.entry_count as usize);
    for _ in 0..header.entry_count {
        let type_id = match common_type {
            Some(v) => v,
            None => read_u32_le(&mut input).map_err(|e| e.to_string())?,
        };
        let group_id = match common_group {
            Some(v) => v,
            None => read_u32_le(&mut input).map_err(|e| e.to_string())?,
        };
        let instance_id = read_u32_le(&mut input).map_err(|e| e.to_string())?;
        let offset = if header.is_dbbf {
            read_u64_le(&mut input).map_err(|e| e.to_string())?
        } else {
            read_u32_le(&mut input).map_err(|e| e.to_string())? as u64
        };
        let disk_size = read_u32_le(&mut input).map_err(|e| e.to_string())? & 0x7fff_ffff;
        let mem_size = read_u32_le(&mut input).map_err(|e| e.to_string())?;
        let compression = read_u16_le(&mut input).map_err(|e| e.to_string())?;
        let _saved_padding = read_u16_le(&mut input).map_err(|e| e.to_string())?;
        if compression != 0 && compression != 0xffff {
            return Err(format!(
                "unsupported package compression label {compression:04x}"
            ));
        }
        if offset
            .checked_add(disk_size as u64)
            .is_none_or(|end| end > file_size)
        {
            return Err("package resource points outside file".into());
        }
        entries.push(DbpfEntry {
            key: ResourceKey {
                type_id,
                group_id,
                instance_id,
            },
            offset,
            disk_size,
            mem_size,
            is_compressed: compression != 0,
        });
    }

    Ok(PackageIndex {
        path: path.to_path_buf(),
        header,
        entries,
    })
}

/// Read + decompress a single record. `entry` must come from `read_package_index` on the same file.
pub fn read_entry_bytes(package_path: &Path, entry: &DbpfEntry) -> Result<Vec<u8>, String> {
    let mut f =
        File::open(package_path).map_err(|e| format!("open {}: {e}", package_path.display()))?;
    f.seek(SeekFrom::Start(entry.offset as u64))
        .map_err(|e| format!("seek record: {e}"))?;
    let mut raw = vec![0u8; entry.disk_size as usize];
    f.read_exact(&mut raw)
        .map_err(|e| format!("read record: {e}"))?;
    if !entry.is_compressed {
        return Ok(raw);
    }
    // Spore QFS header is 5 bytes: 0x10FB or 0x50FB + 3-byte BE decompressed size.
    if raw.len() >= 5 && (raw[0] == 0x10 || raw[0] == 0x50) && raw[1] == 0xFB {
        return qfs_decompress(&raw);
    }
    // Some records flag compressed but are stored raw; return as-is as fallback.
    Ok(raw)
}

/// QFS / RefPack decompressor (Spore 5-byte header variant).
///
/// Port of the public-domain algorithm documented on SimsWiki
/// (`DBPF/Compression`) and used by DBPFSharp / SporeModder-FX:
/// opcodes 0x00-0x7F (2-byte), 0x80-0xBF (3-byte), 0xC0-0xDF (4-byte),
/// 0xE0-0xFB (literal), 0xFC-0xFF (stop + 0-3 trailing literals).
pub fn qfs_decompress(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() < 5 {
        return Err("QFS block too short".to_string());
    }
    if !((input[0] == 0x10 || input[0] == 0x50) && input[1] == 0xFB) {
        return Err(format!("bad QFS magic {:02X} {:02X}", input[0], input[1]));
    }
    let out_len = ((input[2] as usize) << 16) | ((input[3] as usize) << 8) | (input[4] as usize);
    if out_len == 0 || out_len > 256 * 1024 * 1024 {
        return Err(format!("implausible QFS output size {out_len}"));
    }
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    let mut pos = 5usize;

    // Byte-at-a-time copy to allow overlapping matches (LZ-style).
    fn copy_from_output(out: &mut Vec<u8>, offset: usize, count: usize) -> Result<(), String> {
        if offset == 0 || offset > out.len() {
            return Err(format!(
                "bad QFS copy offset {offset} at out_len {}",
                out.len()
            ));
        }
        for _ in 0..count {
            let src = out.len() - offset;
            let b = out[src];
            out.push(b);
        }
        Ok(())
    }

    while out.len() < out_len {
        if pos >= input.len() {
            return Err("truncated QFS stream".to_string());
        }
        let cc = input[pos];
        pos += 1;
        if cc >= 0xFC {
            let plain = (cc & 0x03) as usize;
            if pos + plain > input.len() {
                return Err("truncated QFS trailing literals".to_string());
            }
            out.extend_from_slice(&input[pos..pos + plain]);
            break;
        } else if cc >= 0xE0 {
            let plain = (((cc & 0x1F) as usize) << 2) + 4;
            if pos + plain > input.len() {
                return Err("truncated QFS literal run".to_string());
            }
            out.extend_from_slice(&input[pos..pos + plain]);
            pos += plain;
        } else if cc >= 0xC0 {
            if pos + 3 > input.len() {
                return Err("truncated QFS 4-byte op".to_string());
            }
            let b1 = input[pos];
            let b2 = input[pos + 1];
            let b3 = input[pos + 2];
            pos += 3;
            let plain = (cc & 0x03) as usize;
            let copy_count = (((cc & 0x0C) as usize) << 6) + (b3 as usize) + 5;
            let copy_offset =
                (((cc & 0x10) as usize) << 12) + ((b1 as usize) << 8) + (b2 as usize) + 1;
            if pos + plain > input.len() {
                return Err("truncated QFS 4-byte literals".to_string());
            }
            out.extend_from_slice(&input[pos..pos + plain]);
            pos += plain;
            copy_from_output(&mut out, copy_offset, copy_count)?;
        } else if cc >= 0x80 {
            if pos + 2 > input.len() {
                return Err("truncated QFS 3-byte op".to_string());
            }
            let b1 = input[pos];
            let b2 = input[pos + 1];
            pos += 2;
            let plain = ((b1 & 0xC0) >> 6) as usize;
            let copy_count = ((cc & 0x3F) as usize) + 4;
            let copy_offset = (((b1 & 0x3F) as usize) << 8) + (b2 as usize) + 1;
            if pos + plain > input.len() {
                return Err("truncated QFS 3-byte literals".to_string());
            }
            out.extend_from_slice(&input[pos..pos + plain]);
            pos += plain;
            copy_from_output(&mut out, copy_offset, copy_count)?;
        } else {
            if pos >= input.len() {
                return Err("truncated QFS 2-byte op".to_string());
            }
            let b1 = input[pos];
            pos += 1;
            let plain = (cc & 0x03) as usize;
            let copy_count = (((cc & 0x1C) >> 2) as usize) + 3;
            let copy_offset = (((cc & 0x60) as usize) << 3) + (b1 as usize) + 1;
            if pos + plain > input.len() {
                return Err("truncated QFS 2-byte literals".to_string());
            }
            out.extend_from_slice(&input[pos..pos + plain]);
            pos += plain;
            copy_from_output(&mut out, copy_offset, copy_count)?;
        }
    }

    if out.len() != out_len {
        return Err(format!(
            "QFS size mismatch: got {} expected {out_len}",
            out.len()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dbbf: bool, flags: u32) -> Vec<u8> {
        let mut bytes = vec![0; 120];
        let put = |bytes: &mut Vec<u8>, offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
        };
        put(&mut bytes, 0, if dbbf { DBBF_MAGIC } else { DBPF_MAGIC });
        put(&mut bytes, 4, 2);
        put(&mut bytes, 36, 1);
        let payload = b"abcd";
        bytes.extend_from_slice(payload);
        let index_offset = bytes.len();
        bytes.extend_from_slice(&flags.to_le_bytes());
        if flags & 1 != 0 {
            bytes.extend_from_slice(&17u32.to_le_bytes());
        }
        if flags & 2 != 0 {
            bytes.extend_from_slice(&23u32.to_le_bytes());
        }
        if flags & 4 != 0 {
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        if flags & 1 == 0 {
            bytes.extend_from_slice(&17u32.to_le_bytes());
        }
        if flags & 2 == 0 {
            bytes.extend_from_slice(&23u32.to_le_bytes());
        }
        bytes.extend_from_slice(&42u32.to_le_bytes());
        bytes.extend_from_slice(&120u32.to_le_bytes());
        if dbbf {
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 1, 0]);
        let size = (bytes.len() - index_offset) as u32;
        put(&mut bytes, if dbbf { 40 } else { 44 }, size);
        put(&mut bytes, if dbbf { 56 } else { 64 }, index_offset as u32);
        bytes
    }
    fn with_package<T>(bytes: &[u8], f: impl FnOnce(&Path) -> T) -> T {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/dbpf-fixtures-056");
        std::fs::create_dir_all(&root).unwrap();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = root.join(format!(
            "{}-{}.package",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        f(&path)
    }
    #[test]
    fn dbpf_and_dbbf_all_shared_field_flags_read_correctly() {
        for dbbf in [false, true] {
            for flags in 0..=7 {
                with_package(&fixture(dbbf, flags), |path| {
                    let index = read_package_index(path).unwrap();
                    assert_eq!(index.header.is_dbbf, dbbf);
                    assert_eq!(index.entries.len(), 1);
                    assert_eq!(
                        index.entries[0].key,
                        ResourceKey {
                            type_id: 17,
                            group_id: 23,
                            instance_id: 42
                        }
                    );
                    assert_eq!(read_entry_bytes(path, &index.entries[0]).unwrap(), b"abcd");
                });
            }
        }
    }
    #[test]
    fn dbbf_64bit_fields_retained_and_malformed_ranges_rejected() {
        let original = fixture(true, 4);
        for (offset, value) in [(40, u64::MAX), (56, 0x10000007c)] {
            let mut bytes = original.clone();
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            with_package(&bytes, |path| {
                let header = read_header(path).unwrap();
                assert_eq!(
                    if offset == 40 {
                        header.index_size
                    } else {
                        header.index_offset
                    },
                    value
                );
                assert!(read_package_index(path).is_err());
            });
        }
        let mut bytes = original.clone();
        bytes[148..152].copy_from_slice(&1u32.to_le_bytes());
        with_package(&bytes, |path| assert!(read_package_index(path).is_err())); // record's offset high word
        for offset in [36, 124] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            with_package(&bytes, |path| assert!(read_package_index(path).is_err()));
        }
        for end in [0, 3, 67, 124, original.len() - 1] {
            with_package(&original[..end], |path| {
                assert!(read_package_index(path).is_err())
            });
        }
    }

    #[test]
    fn qfs_literal_roundtrip_vectors() {
        // Hand-built: header for 4 bytes + 0xE0 literal op (4 plain bytes) + stop 0xFC.
        let input = vec![
            0x10, 0xFB, 0x00, 0x00, 0x04, 0xE0, b'A', b'B', b'C', b'D', 0xFC,
        ];
        let out = qfs_decompress(&input).expect("decompress");
        assert_eq!(out, b"ABCD");
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(qfs_decompress(&[0, 1, 2, 3, 4]).is_err());
    }
}
