//! Font facts the text renderer needs and Bevy does not expose: the family
//! name, the vertical metrics (retail line heights follow them) and which
//! characters a face covers (retail falls through the style sheet's font list,
//! which is how the DNA symbol of `SporeCurrencyEA` appears inside a Trebuchet
//! caption).
//!
//! Only the tables `name`, `hhea`, `head` and `cmap` (formats 4 and 12) are read.

/// What the renderer keeps per installed UI font.
#[derive(Debug, Clone, PartialEq)]
pub struct FontInfo {
    /// Typographic family (`name` id 1) as used in the style sheet.
    pub family: String,
    pub units_per_em: f32,
    /// `hhea` ascent, in font units.
    pub ascent: f32,
    /// `hhea` descent as a positive number, in font units.
    pub descent: f32,
    /// Sorted, non-overlapping inclusive code point ranges the face covers.
    ranges: Vec<(u32, u32)>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn i16_at(b: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn table<'a>(b: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = u16_at(b, 4)? as usize;
    for i in 0..count {
        let rec = 12 + i * 16;
        if b.get(rec..rec + 4)? == tag {
            let offset = u32_at(b, rec + 8)? as usize;
            let length = u32_at(b, rec + 12)? as usize;
            return b.get(offset..offset.checked_add(length)?);
        }
    }
    None
}

fn family_name(name: &[u8]) -> Option<String> {
    let count = u16_at(name, 2)? as usize;
    let strings = u16_at(name, 4)? as usize;
    let mut best: Option<(u32, String)> = None;
    for i in 0..count {
        let rec = 6 + i * 12;
        let platform = u16_at(name, rec)?;
        let id = u16_at(name, rec + 6)?;
        if id != 1 && id != 16 {
            continue;
        }
        let length = u16_at(name, rec + 8)? as usize;
        let offset = u16_at(name, rec + 10)? as usize;
        let raw = name.get(strings + offset..strings + offset + length)?;
        let text = match platform {
            0 | 3 => {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units)
            }
            1 => raw.iter().map(|&c| c as char).collect(),
            _ => continue,
        };
        // Prefer the Windows record of the typographic family.
        let rank = u32::from(platform == 3) * 2 + u32::from(id == 16);
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, t)| t)
}

fn coverage(cmap: &[u8]) -> Option<Vec<(u32, u32)>> {
    let count = u16_at(cmap, 2)? as usize;
    // Prefer full Unicode (3,10), then BMP (3,1), then any Unicode platform.
    let mut chosen: Option<(u32, usize)> = None;
    for i in 0..count {
        let rec = 4 + i * 8;
        let platform = u16_at(cmap, rec)?;
        let encoding = u16_at(cmap, rec + 2)?;
        let offset = u32_at(cmap, rec + 4)? as usize;
        let rank = match (platform, encoding) {
            (3, 10) => 4,
            (3, 1) => 3,
            (0, _) => 2,
            _ => continue,
        };
        if chosen.is_none_or(|(r, _)| rank > r) {
            chosen = Some((rank, offset));
        }
    }
    let sub = cmap.get(chosen?.1..)?;
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    match u16_at(sub, 0)? {
        4 => {
            let segments = u16_at(sub, 6)? as usize / 2;
            let ends = 14;
            let starts = ends + segments * 2 + 2;
            for s in 0..segments {
                let end = u32::from(u16_at(sub, ends + s * 2)?);
                let start = u32::from(u16_at(sub, starts + s * 2)?);
                if start <= end && start != 0xFFFF {
                    ranges.push((start, end.min(0xFFFE)));
                }
            }
        }
        12 => {
            let groups = u32_at(sub, 12)? as usize;
            for g in 0..groups {
                let at = 16 + g * 12;
                ranges.push((u32_at(sub, at)?, u32_at(sub, at + 4)?));
            }
        }
        _ => return None,
    }
    ranges.sort_unstable();
    Some(ranges)
}

impl FontInfo {
    pub fn parse(bytes: &[u8]) -> Option<FontInfo> {
        let head = table(bytes, b"head")?;
        let hhea = table(bytes, b"hhea")?;
        let units_per_em = f32::from(u16_at(head, 18)?);
        if units_per_em <= 0.0 {
            return None;
        }
        Some(FontInfo {
            family: family_name(table(bytes, b"name")?)?,
            units_per_em,
            ascent: f32::from(i16_at(hhea, 4)?),
            descent: -f32::from(i16_at(hhea, 6)?),
            ranges: coverage(table(bytes, b"cmap")?)?,
        })
    }

    pub fn covers(&self, c: char) -> bool {
        let c = c as u32;
        match self.ranges.binary_search_by(|&(a, b)| {
            if c < a {
                std::cmp::Ordering::Greater
            } else if c > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Height of one text line at the given em size. The retail text engine
    /// rounds ascent and descent up to whole pixels before adding them.
    pub fn line_height(&self, em_px: f32) -> f32 {
        (self.ascent / self.units_per_em * em_px).ceil() + (self.descent / self.units_per_em * em_px).ceil()
    }
}

/// Split `text` into runs, each in the first face (the style's own family
/// first, then the sheet's font order) that has every character of the run.
/// Spaces and line breaks stay with the run before them.
pub fn split_runs(text: &str, primary: &str, fonts: &[FontInfo]) -> Vec<(String, String)> {
    let order: Vec<&FontInfo> = fonts
        .iter()
        .filter(|f| f.family == primary)
        .chain(fonts.iter().filter(|f| f.family != primary))
        .collect();
    let mut runs: Vec<(String, String)> = Vec::new();
    for c in text.chars() {
        let family = if c.is_whitespace() {
            runs.last().map_or_else(|| primary.to_string(), |r| r.0.clone())
        } else {
            order
                .iter()
                .find(|f| f.covers(c))
                .map_or_else(|| primary.to_string(), |f| f.family.clone())
        };
        match runs.last_mut() {
            Some((f, t)) if *f == family => t.push(c),
            _ => runs.push((family, c.to_string())),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(family: &str, ranges: &[(u32, u32)]) -> FontInfo {
        FontInfo {
            family: family.into(),
            units_per_em: 2048.0,
            ascent: 1923.0,
            descent: 455.0,
            ranges: ranges.to_vec(),
        }
    }

    #[test]
    fn line_height_rounds_ascent_and_descent_up() {
        let f = info("Trebuchet EA", &[(0x20, 0x7E)]);
        // 11 px: ascent 10.33 -> 11, descent 2.44 -> 3.
        assert_eq!(f.line_height(11.0), 14.0);
        // 12 px: 11.27 -> 12, 2.67 -> 3.
        assert_eq!(f.line_height(12.0), 15.0);
    }

    #[test]
    fn runs_fall_through_the_font_list() {
        let fonts = vec![
            info("Trebuchet EA", &[(0x20, 0x7E)]),
            info("SporeCurrencyEA", &[(0x30, 0x39), (0x268A, 0x2692)]),
        ];
        let runs = split_runs("\u{268B} 18 dna", "Trebuchet EA", &fonts);
        assert_eq!(
            runs,
            vec![
                // The space after the symbol stays with the symbol's run.
                ("SporeCurrencyEA".to_string(), "\u{268B} ".to_string()),
                ("Trebuchet EA".to_string(), "18 dna".to_string())
            ]
        );
        // Everything the primary face covers stays in one run.
        assert_eq!(split_runs("hello world", "Trebuchet EA", &fonts).len(), 1);
        assert!(split_runs("", "Trebuchet EA", &fonts).is_empty());
    }

    #[test]
    fn coverage_lookup() {
        let f = info("X", &[(0x20, 0x7E), (0x268A, 0x2692)]);
        assert!(f.covers('a') && f.covers('\u{268B}'));
        assert!(!f.covers('\u{2693}') && !f.covers('\u{7F}'));
    }

    /// The installed UI fonts parse (needs the Spore install).
    #[test]
    #[ignore = "needs the Spore install"]
    fn installed_fonts_parse() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        for name in ["TrebuchetEAbold", "PalatinoSansEAbold", "SporeCurrencyEA"] {
            let key = crate::dbpf::ResourceKey {
                type_id: super::super::view::FONT_TYPE,
                group_id: super::super::view::FONT_GROUP,
                instance_id: crate::world_objects::name_id(name),
            };
            let f = FontInfo::parse(&index.read(key).unwrap()).expect(name);
            println!("{name}: family={} upm={} asc={} desc={}", f.family, f.units_per_em, f.ascent, f.descent);
            // The currency face only has digits and symbols.
            assert!(f.covers(if name == "SporeCurrencyEA" { '7' } else { 'A' }));
        }
        let key = crate::dbpf::ResourceKey {
            type_id: super::super::view::FONT_TYPE,
            group_id: super::super::view::FONT_GROUP,
            instance_id: crate::world_objects::name_id("SporeCurrencyEA"),
        };
        let currency = FontInfo::parse(&index.read(key).unwrap()).unwrap();
        assert_eq!(currency.family, "SporeCurrencyEA");
        assert!(currency.covers('\u{268B}'));
        let key = crate::dbpf::ResourceKey {
            type_id: super::super::view::FONT_TYPE,
            group_id: super::super::view::FONT_GROUP,
            instance_id: crate::world_objects::name_id("TrebuchetEAbold"),
        };
        let trebuchet = FontInfo::parse(&index.read(key).unwrap()).unwrap();
        assert_eq!(trebuchet.family, "Trebuchet EA");
        assert!(!trebuchet.covers('\u{268B}'));
        assert_eq!(trebuchet.line_height(11.0), 14.0);
    }
}
