//! Original action-to-animation lists. Numeric fields are big endian, strings
//! are UTF-16LE. Choice probabilities are cumulative thresholds, not weights.
pub const TYPE_ID: u32 = 0x4AEB6BC6;
#[derive(Debug, Clone, PartialEq)]
pub struct Tlsa {
    pub version: u32,
    pub groups: Vec<Group>,
    pub trailing: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub priority_override: f32,
    pub blend_in_time: f32,
    pub idle: bool,
    pub allow_locomotion: bool,
    pub randomize_choice_per_loop: bool,
    pub match_variant_for_tool_mask: u32,
    pub disable_tool_overlay_mask: u32,
    pub end_mode: u32,
    pub choices: Vec<Choice>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub probability_threshold: f32,
    pub animations: Vec<Animation>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub id: u32,
    pub description: String,
    pub duration_scale: f32,
    pub duration: f32,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("TLSA offset overflow")?;
        let data = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("truncated TLSA at {} (need {n})", self.at))?;
        self.at = end;
        Ok(data)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err("nonfinite TLSA value".into())
        }
    }
    fn boolean(&mut self) -> Result<bool, String> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("invalid TLSA boolean".into()),
        }
    }
    fn count(&mut self, max: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > max {
            Err(format!("TLSA count {n} exceeds {max}"))
        } else {
            Ok(n)
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.count(16384)?;
        let b = self.take(n.checked_mul(2).ok_or("TLSA string overflow")?)?;
        let units: Vec<_> = b
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| "invalid TLSA UTF-16 string".into())
    }
}
pub fn parse(bytes: &[u8]) -> Result<Tlsa, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != b"tsla" {
        return Err("bad TLSA magic".into());
    }
    let version = r.u32()?;
    if !(7..=10).contains(&version) {
        return Err(format!("unsupported TLSA version {version}"));
    }
    let count = r.count(100000)?;
    let mut groups = Vec::with_capacity(count.min(bytes.len() / 32));
    for index in 0..count {
        let id = r.u32()?;
        let mut group = Group {
            id,
            name: String::new(),
            priority_override: 0.,
            blend_in_time: -1.,
            idle: false,
            allow_locomotion: false,
            randomize_choice_per_loop: false,
            match_variant_for_tool_mask: 0,
            disable_tool_overlay_mask: 0,
            end_mode: 4,
            choices: vec![],
        };
        if version <= 8 {
            if version == 8 {
                group.priority_override = r.f32()?;
            }
            let n = r.count(100000)?;
            let mut animations = Vec::with_capacity(n.min(bytes.len() / 8));
            for _ in 0..n {
                animations.push(Animation {
                    id: 0,
                    description: r.string()?,
                    duration_scale: 1.,
                    duration: -1.,
                });
            }
            for animation in &mut animations {
                animation.id = r.u32()?;
            }
            let scale = r.f32()?;
            let duration = r.f32()?;
            for animation in &mut animations {
                animation.duration_scale = scale;
                animation.duration = duration;
            }
            group.idle = r.boolean()?;
            group.blend_in_time = r.f32()?;
            group.allow_locomotion = r.boolean()?;
            group.disable_tool_overlay_mask = r.u32()?;
            group.match_variant_for_tool_mask = r.u32()?;
            group.end_mode = r.u32()?;
            group.choices.push(Choice {
                probability_threshold: 1.,
                animations,
            });
        } else {
            group.name = r.string()?;
            group.priority_override = r.f32()?;
            group.blend_in_time = r.f32()?;
            group.idle = r.boolean()?;
            group.allow_locomotion = r.boolean()?;
            if version == 10 {
                group.randomize_choice_per_loop = r.boolean()?;
            }
            group.match_variant_for_tool_mask = r.u32()?;
            group.disable_tool_overlay_mask = r.u32()?;
            group.end_mode = r.u32()?;
            let choices = r.count(10000)?;
            let mut previous = 0.;
            for _ in 0..choices {
                let threshold = r.f32()?;
                if threshold < previous {
                    return Err(format!(
                        "TLSA group {index} has decreasing probability thresholds"
                    ));
                }
                previous = threshold;
                let n = r.count(100000)?;
                let mut animations = Vec::with_capacity(n.min(bytes.len() / 16));
                for _ in 0..n {
                    animations.push(Animation {
                        duration_scale: r.f32()?,
                        duration: r.f32()?,
                        id: r.u32()?,
                        description: r.string()?,
                    });
                }
                group.choices.push(Choice {
                    probability_threshold: threshold,
                    animations,
                });
            }
        }
        groups.push(group);
    }
    Ok(Tlsa {
        version,
        groups,
        trailing: bytes[r.at..].to_vec(),
    })
}
impl Group {
    /// Select the authored sequence without renormalizing thresholds, choosing
    /// a random sequence member, or treating overlay masks as eligibility rules.
    /// Gaps can select no choice; boundaries choose the next probability bin.
    pub fn choose(&self, random: f32) -> Result<Option<&Choice>, String> {
        if !random.is_finite() || !(0.0..1.0).contains(&random) {
            return Err("TLSA random value must be in [0,1)".into());
        }
        Ok(self
            .choices
            .iter()
            .find(|c| random < c.probability_threshold))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn int(out: &mut Vec<u8>, v: u32) {
        out.extend(v.to_be_bytes());
    }
    fn float(out: &mut Vec<u8>, v: f32) {
        int(out, v.to_bits());
    }
    fn string(out: &mut Vec<u8>, v: &str) {
        let units: Vec<_> = v.encode_utf16().collect();
        int(out, units.len() as u32);
        for unit in units {
            out.extend(unit.to_le_bytes());
        }
    }
    fn fixture(version: u32) -> Vec<u8> {
        let mut b = b"tsla".to_vec();
        int(&mut b, version);
        int(&mut b, 1);
        int(&mut b, 0x12345678);
        if version <= 8 {
            if version == 8 {
                float(&mut b, 3.);
            }
            int(&mut b, 2);
            string(&mut b, "first 🌱");
            string(&mut b, "second");
            int(&mut b, 11);
            int(&mut b, 22);
            float(&mut b, 0.75);
            float(&mut b, -1.);
            b.push(1);
            float(&mut b, 0.25);
            b.push(0);
            int(&mut b, 0x80000000);
            int(&mut b, 0xFF);
            int(&mut b, 4);
        } else {
            string(&mut b, "action");
            float(&mut b, 3.);
            float(&mut b, 0.25);
            b.extend([1, 0]);
            if version == 10 {
                b.push(1);
            }
            int(&mut b, 0xFF);
            int(&mut b, 0x80000000);
            int(&mut b, 4);
            int(&mut b, 2);
            for (threshold, id) in [(0.25, 11), (0.75, 22)] {
                float(&mut b, threshold);
                int(&mut b, 1);
                float(&mut b, 0.75);
                float(&mut b, -1.);
                int(&mut b, id);
                string(&mut b, "first 🌱");
            }
        }
        b
    }
    #[test]
    fn all_supported_layouts_keep_authored_metadata() {
        for version in 7..=10 {
            let parsed = parse(&fixture(version)).unwrap();
            assert_eq!(parsed.version, version);
            assert!(parsed.trailing.is_empty());
            let g = &parsed.groups[0];
            assert_eq!(g.id, 0x12345678);
            assert_eq!(g.disable_tool_overlay_mask, 0x80000000);
            assert_eq!(g.match_variant_for_tool_mask, 0xFF);
            assert_eq!(g.end_mode, 4);
            assert!(g.idle);
            assert!(!g.allow_locomotion);
            assert_eq!(g.choices[0].animations[0].description, "first 🌱");
            assert_eq!(g.choices[0].animations[0].duration_scale, 0.75);
            if version <= 8 {
                assert_eq!(g.choices.len(), 1);
                assert_eq!(g.choices[0].animations[1].id, 22);
            } else {
                assert_eq!(g.choices[1].animations[0].id, 22);
            }
        }
    }
    #[test]
    fn every_truncation_fails_without_panicking() {
        for version in [7, 10] {
            let b = fixture(version);
            for n in 0..b.len() {
                assert!(parse(&b[..n]).is_err(), "v{version} accepted length {n}");
            }
        }
    }
    #[test]
    fn cumulative_selection_keeps_gaps_and_boundaries() {
        let t = parse(&fixture(10)).unwrap();
        let g = &t.groups[0];
        assert_eq!(g.choose(0.).unwrap().unwrap().animations[0].id, 11);
        assert_eq!(g.choose(0.25).unwrap().unwrap().animations[0].id, 22);
        assert!(g.choose(0.75).unwrap().is_none());
        assert!(g.choose(f32::NAN).is_err());
        assert!(g.choose(1.).is_err());
    }
    #[test]
    fn bad_versions_counts_and_nonfinite_values_are_rejected() {
        let mut b = fixture(10);
        b[4..8].copy_from_slice(&11u32.to_be_bytes());
        assert!(parse(&b).is_err());
        b = fixture(10);
        b[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse(&b).is_err());
        b = fixture(7);
        let n = b.len();
        b[n - 17..n - 13].copy_from_slice(&f32::NAN.to_bits().to_be_bytes());
        assert!(parse(&b).is_err());
    }
}
