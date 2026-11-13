//! Original standalone ANIM containers. All time keys and the header length are
//! measured in frames; frame_length converts playback seconds to that domain.
//! Format references: SporeModder-FX file/anim and the Blender ANIM exporter.
//! Authored curve evaluation is in animation_sampling; linear sampling remains
//! available as a diagnostic comparison.
use bevy::math::Quat;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    pub flags: u32,
    pub capability: String,
    pub id: u32,
    pub fields: [u32; 2],
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Interpolator {
    pub next_factor: f32,
    pub previous_factor: f32,
    pub next_mode: u8,
    pub previous_mode: u8,
    pub reserved: [u8; 6],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InfoKey {
    pub frame: u32,
    pub event_start: u16,
    pub event_count: u8,
    pub flags: u32,
    pub raw: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PositionKey {
    pub position: [f32; 3],
    pub weight: f32,
    pub interpolators: [Interpolator; 4],
}
#[derive(Debug, Clone, PartialEq)]
pub struct RotationKey {
    pub rotation: [f32; 4],
    pub weight: f32,
    pub next_mode: u8,
    pub previous_mode: u8,
    pub weight_interpolator: Interpolator,
    pub raw: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeformKey {
    pub value: f32,
    pub weight: f32,
    pub interpolators: [Interpolator; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub enum Keys {
    Info(Vec<InfoKey>),
    Position(Vec<PositionKey>),
    Rotation(Vec<RotationKey>),
    Deform(Vec<DeformKey>),
    Unknown(Vec<Vec<u8>>),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub flags: u32,
    pub authored_flags: u32,
    pub id: u32,
    pub authored_id: u32,
    pub index: u32,
    pub reserved: [u8; 12],
    pub keys: Keys,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub movement_flags: u32,
    pub primary: Context,
    pub secondary: Context,
    pub bind_flags: u32,
    pub reserved: Vec<u8>,
    pub components: Vec<Component>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventSelector {
    pub flags: u32,
    pub select_flags: u32,
    pub capability: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub flags: u32,
    pub selectors: [EventSelector; 4],
    pub archetype: u32,
    pub group: u32,
    pub id: u32,
    pub name: String,
    pub parameter0: u32,
    pub max_squared_distance: f32,
    pub parameter1: u32,
    pub predicate: [u32; 2],
    pub raw: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub version: u32,
    pub declared_size: u32,
    pub id: u32,
    pub source_path: String,
    pub flags: u32,
    pub length_frames: f32,
    pub frame_length: f32,
    pub predicate: [u32; 2],
    pub channels: Vec<Channel>,
    pub events: Vec<Event>,
}
impl Animation {
    pub fn duration_seconds(&self) -> f32 {
        self.length_frames * self.frame_length
    }
    pub fn frame_at_seconds(&self, seconds: f32) -> Result<f32, String> {
        if !seconds.is_finite() {
            return Err("nonfinite animation playback time".into());
        }
        Ok((seconds / self.frame_length).clamp(0., self.length_frames))
    }
}
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn bytes(&self, o: usize, n: usize) -> Result<&[u8], String> {
        let end = o.checked_add(n).ok_or("ANIM offset overflow")?;
        self.0
            .get(o..end)
            .ok_or_else(|| format!("truncated ANIM at {o:08X}, need {n} bytes"))
    }
    fn u32(&self, o: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(o, 4)?.try_into().unwrap()))
    }
    fn u16(&self, o: usize) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(o, 2)?.try_into().unwrap()))
    }
    fn f32(&self, o: usize) -> Result<f32, String> {
        let f = f32::from_bits(self.u32(o)?);
        if f.is_finite() {
            Ok(f)
        } else {
            Err(format!("nonfinite ANIM value at {o:08X}"))
        }
    }
    fn string(&self, o: usize, limit: usize, terminated: bool) -> Result<String, String> {
        let bytes = self.bytes(o, limit)?;
        let end = bytes.iter().position(|b| *b == 0);
        if terminated && end.is_none() {
            return Err(format!("unterminated ANIM string at {o:08X}"));
        }
        Ok(String::from_utf8_lossy(&bytes[..end.unwrap_or(limit)]).into_owned())
    }
    fn array<const N: usize>(&self, o: usize) -> Result<[f32; N], String> {
        let mut out = [0.; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32(o + i * 4)?;
        }
        Ok(out)
    }
    fn interpolator(&self, o: usize) -> Result<Interpolator, String> {
        let b = self.bytes(o, 16)?;
        Ok(Interpolator {
            next_factor: self.f32(o)?,
            previous_factor: self.f32(o + 4)?,
            next_mode: b[8],
            previous_mode: b[9],
            reserved: b[10..].try_into().unwrap(),
        })
    }
    fn context(&self, o: usize) -> Result<Context, String> {
        Ok(Context {
            flags: self.u32(o)?,
            capability: self.string(o + 4, 4, false)?,
            id: self.u32(o + 4)?,
            fields: [self.u32(o + 8)?, self.u32(o + 12)?],
        })
    }
}

pub fn parse(bytes: &[u8]) -> Result<Animation, String> {
    let r = Reader(bytes);
    if r.bytes(0, 4)? != b"ANIM" {
        return Err("bad ANIM magic".into());
    }
    r.bytes(0, 0x15C)?;
    let version = r.u32(8)?;
    if !(0x14..=0x19).contains(&version) {
        return Err(format!("unsupported ANIM version {version:08X}"));
    }
    let length_frames = r.f32(0x120)?;
    let frame_length = r.f32(0x11C)?;
    if length_frames < 0.
        || frame_length <= 0.
        || frame_length > 1.
        || !(length_frames * frame_length).is_finite()
    {
        return Err("invalid ANIM duration/frame length".into());
    }
    let event_count = r.u32(0x13C)? as usize;
    let event_ptr = r.u32(0x140)? as usize;
    let count = r.u32(0x144)? as usize;
    let table = r.u32(0x148)? as usize;
    if count > 1024 || event_count > 100_000 {
        return Err("implausible ANIM channel/event count".into());
    }
    r.bytes(
        table,
        count.checked_mul(4).ok_or("ANIM channel table overflow")?,
    )?;
    r.bytes(
        event_ptr,
        event_count
            .checked_mul(0x60)
            .ok_or("ANIM event table overflow")?,
    )?;
    let mut events = Vec::with_capacity(event_count);
    for i in 0..event_count {
        let o = event_ptr + i * 0x60;
        let id = r.u32(o + 0x3C)?;
        let name_ptr = r.u32(o + 0x40)? as usize;
        let name = if id == 0 {
            String::new()
        } else {
            let available = bytes
                .len()
                .checked_sub(name_ptr)
                .ok_or("ANIM event name outside record")?
                .min(4096);
            r.string(name_ptr, available, true)?
        };
        let mut selectors = std::array::from_fn(|_| EventSelector::default());
        for (j, s) in selectors.iter_mut().enumerate() {
            let p = o + 4 + j * 12;
            *s = EventSelector {
                flags: r.u32(p)?,
                select_flags: r.u32(p + 4)?,
                capability: r.string(p + 8, 4, false)?,
            };
        }
        events.push(Event {
            flags: r.u32(o)?,
            selectors,
            archetype: r.u32(o + 0x34)?,
            group: r.u32(o + 0x38)?,
            id,
            name,
            parameter0: r.u32(o + 0x44)?,
            max_squared_distance: r.f32(o + 0x48)?,
            parameter1: r.u32(o + 0x4C)?,
            predicate: [r.u32(o + 0x50)?, r.u32(o + 0x54)?],
            raw: r.bytes(o, 0x60)?.to_vec(),
        });
    }
    let mut channels = Vec::with_capacity(count);
    for i in 0..count {
        let o = r.u32(table + i * 4)? as usize;
        channels.push(
            parse_channel(&r, o, version, event_count).map_err(|e| format!("channel {i}: {e}"))?,
        );
    }
    Ok(Animation {
        version,
        declared_size: r.u32(4)?,
        id: r.u32(0x110)?,
        source_path: r.string(12, 256, true)?,
        flags: r.u32(0x118)?,
        length_frames,
        frame_length,
        predicate: [r.u32(0x154)?, r.u32(0x158)?],
        channels,
        events,
    })
}

fn parse_channel(
    r: &Reader<'_>,
    o: usize,
    version: u32,
    event_count: usize,
) -> Result<Channel, String> {
    r.bytes(o, 0xE4)?;
    if r.bytes(o, 4)? != b"CHAN" {
        return Err("bad channel magic".into());
    }
    let key_count = r.u32(o + 0xD4)? as usize;
    let key_ptr = r.u32(o + 0xD8)? as usize;
    let comp_count = r.u32(o + 0xDC)? as usize;
    let comp_ptr = r.u32(o + 0xE0)? as usize;
    if key_count > 100_000
        || comp_count > 128
        || key_count
            .checked_mul(comp_count)
            .ok_or("ANIM key count overflow")?
            > 1_000_000
    {
        return Err("implausible ANIM key/component count".into());
    }
    r.bytes(comp_ptr, comp_count * 32)?;
    let mut ch = Channel {
        name: r.string(o + 8, 128, true)?,
        movement_flags: r.u32(o + 0x88)?,
        primary: r.context(o + 0x8C)?,
        secondary: r.context(o + 0x9C)?,
        bind_flags: r.u32(o + 0xAC)?,
        reserved: r.bytes(o + 0xB0, 0x24)?.to_vec(),
        components: Vec::with_capacity(comp_count),
    };
    for c in 0..comp_count {
        let meta = comp_ptr + c * 32;
        let flags = r.u32(meta)?;
        let id = r.u32(meta + 4)?;
        let offset = r.u32(meta + 8)? as usize;
        let stride = r.u32(meta + 12)? as usize;
        let kind = flags & 0xF;
        let size = match kind {
            0 => 20,
            1 => 80,
            2 => 100,
            3 => 40,
            _ => stride,
        };
        if key_count > 1 && stride < size {
            return Err(format!(
                "component {c} stride {stride} smaller than payload {size}"
            ));
        }
        if key_count != 0 && size == 0 {
            return Err("unknown zero-sized ANIM component".into());
        }
        let base = key_ptr
            .checked_add(offset)
            .ok_or("ANIM key offset overflow")?;
        let mut payloads = Vec::with_capacity(key_count);
        for k in 0..key_count {
            let at = base
                .checked_add(stride.checked_mul(k).ok_or("ANIM key stride overflow")?)
                .ok_or("ANIM key offset overflow")?;
            payloads.push((at, r.bytes(at, size)?));
        }
        let keys = match kind {
            0 => Keys::Info(
                payloads
                    .iter()
                    .map(|(at, b)| {
                        let frame = r.u32(*at)?;
                        let event_start = r.u16(*at + 8)?;
                        let event_n = b[10];
                        if event_n != 0 && event_start as usize + event_n as usize > event_count {
                            return Err("key references missing ANIM events".into());
                        }
                        Ok(InfoKey {
                            frame,
                            event_start,
                            event_count: event_n,
                            flags: r.u32(*at + 12)?,
                            raw: b.to_vec(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            1 => Keys::Position(
                payloads
                    .iter()
                    .map(|(at, _)| {
                        Ok(PositionKey {
                            position: r.array(*at)?,
                            weight: r.f32(*at + 12)?,
                            interpolators: [
                                r.interpolator(*at + 16)?,
                                r.interpolator(*at + 32)?,
                                r.interpolator(*at + 48)?,
                                r.interpolator(*at + 64)?,
                            ],
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            2 => Keys::Rotation(
                payloads
                    .iter()
                    .map(|(at, b)| {
                        Ok(RotationKey {
                            rotation: r.array(*at)?,
                            weight: r.f32(*at + 16)?,
                            next_mode: b[28],
                            previous_mode: b[29],
                            weight_interpolator: r.interpolator(*at + 84)?,
                            raw: b.to_vec(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            3 => Keys::Deform(
                payloads
                    .iter()
                    .map(|(at, _)| {
                        Ok(DeformKey {
                            value: r.f32(*at)?,
                            weight: r.f32(*at + 4)?,
                            interpolators: [r.interpolator(*at + 8)?, r.interpolator(*at + 24)?],
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            _ => Keys::Unknown(payloads.iter().map(|(_, b)| b.to_vec()).collect()),
        };
        ch.components.push(Component {
            flags,
            authored_flags: flags,
            id,
            authored_id: id,
            index: r.u32(meta + 16)?,
            reserved: r.bytes(meta + 20, 12)?.try_into().unwrap(),
            keys,
        });
    }
    let times: Vec<_> = ch
        .components
        .iter()
        .filter_map(|c| {
            if let Keys::Info(t) = &c.keys {
                Some(t)
            } else {
                None
            }
        })
        .collect();
    if key_count != 0 && times.len() != 1 {
        return Err("ANIM channel needs exactly one INFO time stream".into());
    }
    if times
        .first()
        .is_some_and(|t| t.windows(2).any(|w| w[0].frame > w[1].frame))
    {
        return Err("ANIM key times decrease".into());
    }
    fix_version(&mut ch, version);
    Ok(ch)
}

fn legacy_deforms(capability: &str) -> &'static [u32] {
    match capability {
        "grsp" => &[0x89E06A31, 0x75D4C8CD],
        "skrs" | "gest" => &[0x89E06A31, 0x998BBF67],
        "mout" => &[0x89E06A31, 0x5D8D0055, 0x892788C6, 0xDD0DCEF4],
        "eye" => &[0x89E06A31, 0x9891EEC7, 0x30EE8F49],
        "wing" => &[0xAC04E296, 0x47F0B3DC],
        "slsh" | "liqd" | "slid" | "shot" => &[0x998BBF67],
        "ear" => &[0x39E912E1],
        "foot" => &[0x47F0B3DC],
        "root" => &[
            0x70E47545, 0x98C942B6, 0x98C942B5, 0x98C942B4, 0x98C942B3, 0x98C942B2,
        ],
        "mtcl" => &[0x89E06A31, 0x114BB90C, 0x15054351, 0xC2299AA7],
        "mvcl" => &[0x6FB760FF, 0xB37B55B2, 0x15054351, 0xC2299AA7, 0x2F056C5D],
        "wpcl" => &[0x6FB760FF, 0x15054351, 0xC2299AA7, 0x0AC4AEED],
        "wpel" => &[0x15054351, 0xC2299AA7, 0xFD29BD8E, 0x2D7AE6C2, 0x0EFD249C],
        _ => &[],
    }
}
fn fix_version(ch: &mut Channel, version: u32) {
    let is_root = ch.primary.flags & 0x100003 == 1
        || (ch.primary.flags == 0 && ch.primary.capability == "root");
    if version <= 0x17 {
        let deforms = legacy_deforms(if is_root {
            "root"
        } else {
            &ch.primary.capability
        });
        let mut deform_index = 0;
        for c in &mut ch.components {
            match c.flags & 0xF {
                3 => {
                    c.flags &= !0x40;
                    if let Some(&id) = deforms.get(deform_index) {
                        c.id = id;
                    } else {
                        c.id = 0;
                        c.flags &= !0x10;
                    }
                    deform_index += 1;
                }
                1 if ch.movement_flags & 1 != 0 => c.flags &= !0xC0,
                _ => {}
            }
        }
    }
    if version <= 0x18 && is_root {
        for c in &mut ch.components {
            if let Keys::Info(keys) = &mut c.keys {
                for key in keys {
                    key.flags &= !3;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub position: Option<([f32; 3], f32, u32)>,
    pub rotation: Option<([f32; 4], f32, u32)>,
    pub deforms: Vec<(u32, f32, f32)>,
}

/// Preview approximation at real source timing, retaining component weights.
/// Duplicate keys choose the last key at that time. Full interpolation modes
/// remain available in the decoded tracks for the native-equivalent evaluator.
pub fn sample_linear(ch: &Channel, frame: f32) -> Result<Sample, String> {
    if !frame.is_finite() {
        return Err("nonfinite ANIM sample time".into());
    }
    let times = ch.components.iter().find_map(|c| {
        if let Keys::Info(t) = &c.keys {
            Some(t)
        } else {
            None
        }
    });
    let Some(times) = times.filter(|t| !t.is_empty()) else {
        return Ok(Sample::default());
    };
    let upper = times.partition_point(|k| k.frame as f32 <= frame);
    let a = upper.saturating_sub(1);
    let b = upper.min(times.len() - 1);
    let span = (times[b].frame as f32) - (times[a].frame as f32);
    let t = if span > 0. {
        ((frame - times[a].frame as f32) / span).clamp(0., 1.)
    } else {
        0.
    };
    // Interpolate in f64 so large finite keys of opposite signs do not
    // overflow their f32 difference before the result is formed.
    let lerp = |x: f32, y: f32| ((1. - t as f64) * x as f64 + t as f64 * y as f64) as f32;
    let mut out = Sample::default();
    for c in &ch.components {
        if c.flags & 0x10 == 0 {
            continue;
        }
        match &c.keys {
            Keys::Position(k) => {
                let (x, y) = (
                    k.get(a).ok_or("position/time stream mismatch")?,
                    k.get(b).ok_or("position/time stream mismatch")?,
                );
                out.position = Some((
                    std::array::from_fn(|i| lerp(x.position[i], y.position[i])),
                    lerp(x.weight, y.weight),
                    c.flags,
                ));
            }
            Keys::Rotation(k) => {
                let (x, y) = (
                    k.get(a).ok_or("rotation/time stream mismatch")?,
                    k.get(b).ok_or("rotation/time stream mismatch")?,
                );
                let mut qa = Quat::from_array(x.rotation);
                let mut qb = Quat::from_array(y.rotation);
                if !qa.length_squared().is_finite()
                    || !qb.length_squared().is_finite()
                    || qa.length_squared() < 1e-12
                    || qb.length_squared() < 1e-12
                {
                    return Err("invalid quaternion in ANIM rotation track".into());
                }
                qa = qa.normalize();
                qb = qb.normalize();
                out.rotation = Some((
                    qa.slerp(qb, t).normalize().to_array(),
                    lerp(x.weight, y.weight),
                    c.flags,
                ));
            }
            Keys::Deform(k) => {
                let (x, y) = (
                    k.get(a).ok_or("deform/time stream mismatch")?,
                    k.get(b).ok_or("deform/time stream mismatch")?,
                );
                out.deforms
                    .push((c.id, lerp(x.value, y.value), lerp(x.weight, y.weight)));
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_interpolation_metadata_census() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        let mut modes = std::collections::BTreeMap::<String, usize>::new();
        let mut explicit_factors = 0;
        for key in &catalog.clips {
            let clip = parse(&index.read(*key).unwrap()).unwrap();
            for channel in &clip.channels {
                for component in &channel.components {
                    let mut record = |name: &str, next: u8, previous: u8| {
                        *modes
                            .entry(format!("{name}/{next}/{previous}"))
                            .or_default() += 1;
                    };
                    let mut scalar = |name: &str, interpolator: &Interpolator| {
                        record(name, interpolator.next_mode, interpolator.previous_mode);
                        if interpolator.next_mode & 3 == 3 || interpolator.previous_mode & 3 == 3 {
                            explicit_factors += 1;
                        }
                    };
                    match &component.keys {
                        Keys::Position(keys) => {
                            for key in keys {
                                for (axis, interpolator) in key.interpolators.iter().enumerate() {
                                    scalar(
                                        if axis == 3 {
                                            "position_weight"
                                        } else {
                                            "position"
                                        },
                                        interpolator,
                                    );
                                }
                            }
                        }
                        Keys::Rotation(keys) => {
                            for key in keys {
                                scalar("rotation_weight", &key.weight_interpolator);
                            }
                        }
                        Keys::Deform(keys) => {
                            for key in keys {
                                scalar("deform", &key.interpolators[0]);
                                scalar("deform_weight", &key.interpolators[1]);
                            }
                        }
                        _ => {}
                    }
                    if let Keys::Rotation(keys) = &component.keys {
                        for key in keys {
                            record("rotation", key.next_mode, key.previous_mode);
                        }
                    }
                }
            }
        }
        assert!(!modes.is_empty());
        println!(
            "{}",
            serde_json::json!({"clips":catalog.clips.len(),"mode_pairs":modes,"explicit_scalar_factors":explicit_factors})
        );
    }
    fn int(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn float(b: &mut [u8], o: usize, v: f32) {
        int(b, o, v.to_bits());
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x885];
        b[..4].copy_from_slice(b"ANIM");
        let size = b.len() as u32;
        int(&mut b, 4, size);
        int(&mut b, 8, 0x19);
        int(&mut b, 0x118, 8);
        b[12..14].copy_from_slice(b"x\0");
        float(&mut b, 0x11C, 1. / 30.);
        float(&mut b, 0x120, 31.);
        int(&mut b, 0x13C, 1);
        int(&mut b, 0x140, 0x800);
        int(&mut b, 0x144, 1);
        int(&mut b, 0x148, 0x200);
        int(&mut b, 0x200, 0x240);
        b[0x240..0x244].copy_from_slice(b"CHAN");
        b[0x248..0x24D].copy_from_slice(b"head\0");
        b[0x2D0..0x2D4].copy_from_slice(b"mout");
        int(&mut b, 0x2DC, 2);
        int(&mut b, 0x314, 3);
        int(&mut b, 0x318, 0x400);
        int(&mut b, 0x31C, 4);
        int(&mut b, 0x320, 0x340);
        for (i, (flags, offset, id)) in [
            (0, 0, 0x4F464E49),
            (0x31, 20, 0x534F50),
            (0x12, 100, 0x544F52),
            (0x13, 200, 0x89E06A31),
        ]
        .into_iter()
        .enumerate()
        {
            let o = 0x340 + i * 32;
            int(&mut b, o, flags);
            int(&mut b, o + 4, id);
            int(&mut b, o + 8, offset);
            int(&mut b, o + 12, 256);
            int(&mut b, o + 16, i as u32);
        }
        for (i, frame) in [0, 3, 30].into_iter().enumerate() {
            let o = 0x400 + i * 256;
            int(&mut b, o, frame);
            float(&mut b, o + 20, frame as f32);
            float(&mut b, o + 32, i as f32 * 0.5);
            float(&mut b, o + 112, 2.);
            float(&mut b, o + 116, 1.);
            float(&mut b, o + 200, frame as f32 / 30.);
            float(&mut b, o + 204, 0.6);
        }
        b[0x50A] = 1;
        int(&mut b, 0x800, 7);
        int(&mut b, 0x83C, 55);
        int(&mut b, 0x840, 0x880);
        b[0x880..].copy_from_slice(b"step\0");
        b
    }
    #[test]
    fn retrieves_all_streams_and_actual_frame_time() {
        let a = parse(&fixture()).unwrap();
        assert_eq!(a.flags, 8);
        assert!((a.duration_seconds() - 31. / 30.).abs() < 1e-6);
        assert_eq!(a.events[0].name, "step");
        let frame = a.frame_at_seconds(0.1).unwrap();
        assert!((frame - 3.).abs() < 1e-6);
        let s = sample_linear(&a.channels[0], frame).unwrap();
        assert!((s.position.unwrap().0[0] - 3.).abs() < 1e-5);
        assert!((s.position.unwrap().1 - 0.5).abs() < 1e-5);
        assert_eq!(s.rotation.unwrap().0, [0., 0., 0., 1.]);
        assert!((s.deforms[0].1 - 0.1).abs() < 1e-6);
        assert_eq!(s.deforms[0].2, 0.6);
        assert!(a.frame_at_seconds(f32::NAN).is_err());
        assert_eq!(a.frame_at_seconds(-3.).unwrap(), 0.);
    }
    #[test]
    fn nonuniform_and_duplicate_keys_are_not_spread_evenly() {
        let mut b = fixture();
        int(&mut b, 0x600, 3);
        let a = parse(&b).unwrap();
        let c = &a.channels[0];
        assert_eq!(sample_linear(c, 3.).unwrap().position.unwrap().1, 1.);
        assert_eq!(sample_linear(c, -1.).unwrap().position.unwrap().1, 0.);
        assert!(sample_linear(c, f32::NAN).is_err());
    }
    #[test]
    fn truncated_ranges_bad_times_and_invalid_events_fail() {
        let b = fixture();
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err(), "accepted prefix {n}");
        }
        let mut bad = b.clone();
        int(&mut bad, 0x600, 2);
        assert!(parse(&bad).unwrap_err().contains("decrease"));
        bad = b.clone();
        bad[0x50A] = 2;
        assert!(parse(&bad).unwrap_err().contains("events"));
        bad = b.clone();
        int(&mut bad, 0x34C, u32::MAX);
        assert!(parse(&bad).is_err());
        bad = b.clone();
        float(&mut bad, 0x414, f32::NAN);
        assert!(parse(&bad).is_err());
        bad = b.clone();
        float(&mut bad, 0x11C, 0.);
        assert!(parse(&bad).is_err());
    }
    #[test]
    fn original_legacy_deform_ids_and_root_info_flags_are_migrated() {
        assert_eq!(
            legacy_deforms("wpcl"),
            &[0x6FB760FF, 0x15054351, 0xC2299AA7, 0x0AC4AEED]
        );
        let mut b = fixture();
        int(&mut b, 8, 0x17);
        int(&mut b, 0x3A4, 0);
        let a = parse(&b).unwrap();
        assert_eq!(a.channels[0].components[3].id, 0x89E06A31);
        assert_eq!(a.channels[0].components[3].authored_id, 0);
        int(&mut b, 0x2CC, 1);
        int(&mut b, 0x40C, 3);
        let a = parse(&b).unwrap();
        assert_eq!(a.channels[0].components[3].id, 0x70E47545);
        if let Keys::Info(info) = &a.channels[0].components[0].keys {
            assert_eq!(info[0].flags & 3, 0);
        } else {
            panic!("missing INFO");
        }
    }
}
