//! SPUI binary container (resource type `0x250FE9A2`), ported from
//! SporeModder-FX `SporeUserInterface.read`.
//!
//! The file is little endian:
//!
//! ```text
//! u32 0xE3FE3FB8 magic            u16 version (<= 3)
//! u16 imageCount, atlasCount, hitMaskCount, elementCount
//! imageCount x key, atlasCount x key (instance, type, group as u32 each)
//! hitMaskCount x hit mask         elementCount x u32 class proxy id
//! repeated { u16 0x5FF5, i16 reference index (-1 ends), u16 propertyCount | 0x8000 root, properties }
//! u32 0x1C01C047 end magic
//! ```
//!
//! Every property is self describing (`u32 id, i16 type code, u16 count`,
//! then the values), so this reader needs no per-class schema: it produces a
//! generic property bag per element and the typed interpretation lives in
//! [`super::model`]. References are `i16` indices into one combined table:
//! images, atlas files, hit masks, then elements (`-1` is null).

use crate::dbpf::ResourceKey;

pub const MAGIC: u32 = 0xE3FE_3FB8;
pub const MAGIC_END: u32 = 0x1C01_C047;
pub const CLASS_MAGIC: u16 = 0x5FF5;
pub const ROOT_FLAG: u16 = 0x8000;
pub const LAST_SUPPORTED_VERSION: u16 = 3;
/// SporeModder-FX refuses files above this combined object count.
pub const MAX_OBJECTS: usize = 6000;

/// Resource type of an SPUI layout.
pub const SPUI_TYPE: u32 = 0x250F_E9A2;

/// Property value type codes of the container.
pub mod ty {
    pub const BOOL: i16 = 2;
    pub const I8: i16 = 3;
    pub const I16: i16 = 4;
    pub const I32: i16 = 5;
    pub const I64: i16 = 6;
    pub const U8: i16 = 7;
    pub const U16: i16 = 8;
    pub const U32: i16 = 9;
    pub const U64: i16 = 10;
    pub const F32: i16 = 11;
    pub const F64: i16 = 12;
    pub const POINT: i16 = 15;
    pub const RECT: i16 = 16;
    pub const VEC2: i16 = 17;
    pub const TEXT: i16 = 18;
    pub const REF: i16 = 19;
    pub const STRUCT: i16 = 20;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpuiError(pub String);

impl std::fmt::Display for SpuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpuiError {}

type Result<T> = std::result::Result<T, SpuiError>;

fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(SpuiError(message.into()))
}

/// A string property: either literal UTF-16 text or a reference into a
/// locale table (`table` is the table name hash, `id` the string id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    Literal(String),
    Localized { table: u32, id: u32 },
}

impl Text {
    pub fn is_empty(&self) -> bool {
        matches!(self, Text::Literal(s) if s.is_empty())
    }
}

/// One decoded property value (always an array; scalars have one entry).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(Vec<bool>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Point(Vec<[i32; 2]>),
    /// `rectf`: x1, y1, x2, y2.
    Rect(Vec<[f32; 4]>),
    Vec2(Vec<[f32; 2]>),
    Text(Vec<Text>),
    Ref(Vec<i16>),
    Struct(Vec<Vec<Prop>>),
}

impl Value {
    pub fn len(&self) -> usize {
        match self {
            Value::Bool(v) => v.len(),
            Value::I8(v) => v.len(),
            Value::I16(v) => v.len(),
            Value::I32(v) => v.len(),
            Value::I64(v) => v.len(),
            Value::U8(v) => v.len(),
            Value::U16(v) => v.len(),
            Value::U32(v) => v.len(),
            Value::U64(v) => v.len(),
            Value::F32(v) => v.len(),
            Value::F64(v) => v.len(),
            Value::Point(v) => v.len(),
            Value::Rect(v) => v.len(),
            Value::Vec2(v) => v.len(),
            Value::Text(v) => v.len(),
            Value::Ref(v) => v.len(),
            Value::Struct(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Any integer typed entry widened to `i64` (booleans as 0/1).
    pub fn int_at(&self, index: usize) -> Option<i64> {
        Some(match self {
            Value::Bool(v) => *v.get(index)? as i64,
            Value::I8(v) => *v.get(index)? as i64,
            Value::I16(v) => *v.get(index)? as i64,
            Value::I32(v) => *v.get(index)? as i64,
            Value::I64(v) => *v.get(index)?,
            Value::U8(v) => *v.get(index)? as i64,
            Value::U16(v) => *v.get(index)? as i64,
            Value::U32(v) => *v.get(index)? as i64,
            Value::U64(v) => *v.get(index)? as i64,
            _ => return None,
        })
    }

    pub fn float_at(&self, index: usize) -> Option<f32> {
        match self {
            Value::F32(v) => v.get(index).copied(),
            Value::F64(v) => v.get(index).map(|f| *f as f32),
            _ => self.int_at(index).map(|i| i as f32),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub id: u32,
    pub value: Value,
}

/// Lookup helpers shared by element and struct property lists.
pub trait PropList {
    fn props(&self) -> &[Prop];

    fn get(&self, id: u32) -> Option<&Value> {
        self.props().iter().find(|p| p.id == id).map(|p| &p.value)
    }
    fn u32_at(&self, id: u32, index: usize) -> Option<u32> {
        self.get(id)?.int_at(index).map(|v| v as u32)
    }
    fn i32_at(&self, id: u32, index: usize) -> Option<i32> {
        self.get(id)?.int_at(index).map(|v| v as i32)
    }
    fn bool_at(&self, id: u32, index: usize) -> Option<bool> {
        self.get(id)?.int_at(index).map(|v| v != 0)
    }
    fn f32_at(&self, id: u32, index: usize) -> Option<f32> {
        self.get(id)?.float_at(index)
    }
    fn rect(&self, id: u32) -> Option<[f32; 4]> {
        match self.get(id)? {
            Value::Rect(v) => v.first().copied(),
            _ => None,
        }
    }
    fn vec2(&self, id: u32) -> Option<[f32; 2]> {
        match self.get(id)? {
            Value::Vec2(v) => v.first().copied(),
            _ => None,
        }
    }
    fn point(&self, id: u32) -> Option<[i32; 2]> {
        match self.get(id)? {
            Value::Point(v) => v.first().copied(),
            _ => None,
        }
    }
    fn text(&self, id: u32) -> Option<&Text> {
        match self.get(id)? {
            Value::Text(v) => v.first(),
            _ => None,
        }
    }
    fn refs(&self, id: u32) -> &[i16] {
        match self.get(id) {
            Some(Value::Ref(v)) => v,
            _ => &[],
        }
    }
    fn reference(&self, id: u32, index: usize) -> Option<i16> {
        self.refs(id).get(index).copied().filter(|r| *r >= 0)
    }
    fn structs(&self, id: u32) -> &[Vec<Prop>] {
        match self.get(id) {
            Some(Value::Struct(v)) => v,
            _ => &[],
        }
    }
}

impl PropList for [Prop] {
    fn props(&self) -> &[Prop] {
        self
    }
}

impl PropList for Vec<Prop> {
    fn props(&self) -> &[Prop] {
        self
    }
}

/// A run-length hit mask: `runs` holds `(first, last)` linear pixel indices
/// (`y * width + x`, inclusive) of covered pixels, or the mask is an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawHitMask {
    Rle {
        width: u32,
        height: u32,
        runs: Vec<u16>,
    },
    Image(ResourceKey),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawElement {
    /// Class proxy id (see [`super::schema`]).
    pub class: u32,
    pub props: Vec<Prop>,
    /// Root windows of the layout carry the root flag.
    pub root: bool,
}

impl PropList for RawElement {
    fn props(&self) -> &[Prop] {
        &self.props
    }
}

/// What a reference index points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Null,
    /// Direct image resource (index into [`RawFile::images`]).
    Image(usize),
    /// Atlas image file (index into [`RawFile::atlases`]).
    AtlasFile(usize),
    HitMask(usize),
    Element(usize),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawFile {
    pub version: u16,
    pub images: Vec<ResourceKey>,
    pub atlases: Vec<ResourceKey>,
    pub masks: Vec<RawHitMask>,
    pub elements: Vec<RawElement>,
}

impl RawFile {
    pub fn resolve(&self, index: i16) -> Target {
        if index < 0 {
            return Target::Null;
        }
        let mut i = index as usize;
        if i < self.images.len() {
            return Target::Image(i);
        }
        i -= self.images.len();
        if i < self.atlases.len() {
            return Target::AtlasFile(i);
        }
        i -= self.atlases.len();
        if i < self.masks.len() {
            return Target::HitMask(i);
        }
        i -= self.masks.len();
        if i < self.elements.len() {
            Target::Element(i)
        } else {
            Target::Null
        }
    }

    pub fn roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.root)
            .map(|(i, _)| i)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return err(format!(
                "unexpected end of data at byte {} (wanted {n})",
                self.pos
            ));
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn key(&mut self) -> Result<ResourceKey> {
        // ResourceKey.readLE: instance, type, group.
        let instance_id = self.u32()?;
        let type_id = self.u32()?;
        let group_id = self.u32()?;
        Ok(ResourceKey {
            type_id,
            group_id,
            instance_id,
        })
    }

    fn many<T>(&mut self, count: usize, mut one: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        // Counts are 16 bit, but a corrupt header must not allocate blindly.
        let mut out = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            out.push(one(self)?);
        }
        Ok(out)
    }

    fn value(&mut self, code: i16, count: usize) -> Result<Value> {
        Ok(match code {
            ty::BOOL => Value::Bool(self.many(count, |r| Ok(r.u8()? != 0))?),
            ty::I8 => Value::I8(self.many(count, |r| Ok(r.u8()? as i8))?),
            ty::I16 => Value::I16(self.many(count, |r| r.i16())?),
            ty::I32 => Value::I32(self.many(count, |r| r.i32())?),
            ty::I64 => Value::I64(self.many(count, |r| r.i64())?),
            ty::U8 => Value::U8(self.many(count, |r| r.u8())?),
            ty::U16 => Value::U16(self.many(count, |r| r.u16())?),
            ty::U32 => Value::U32(self.many(count, |r| r.u32())?),
            ty::U64 => Value::U64(self.many(count, |r| r.u64())?),
            ty::F32 => Value::F32(self.many(count, |r| r.f32())?),
            ty::F64 => Value::F64(self.many(count, |r| r.f64())?),
            ty::POINT => Value::Point(self.many(count, |r| Ok([r.i32()?, r.i32()?]))?),
            ty::RECT => Value::Rect(self.many(count, |r| {
                Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?])
            })?),
            ty::VEC2 => Value::Vec2(self.many(count, |r| Ok([r.f32()?, r.f32()?]))?),
            ty::TEXT => Value::Text(self.many(count, |r| {
                let len = r.i16()?;
                if len == -1 {
                    Ok(Text::Localized {
                        table: r.u32()?,
                        id: r.u32()?,
                    })
                } else {
                    let bytes = r.take(len as usize * 2)?;
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    Ok(Text::Literal(String::from_utf16_lossy(&units)))
                }
            })?),
            ty::REF => Value::Ref(self.many(count, |r| r.i16())?),
            ty::STRUCT => {
                // Four unknown bytes, then `count` structures, each a short
                // property count followed by that many properties.
                let _unknown = self.i32()?;
                Value::Struct(self.many(count, |r| {
                    let n = r.i16()?;
                    if n < 0 {
                        return err("negative structure property count");
                    }
                    r.props(n as usize)
                })?)
            }
            other => return err(format!("unknown property type code {other}")),
        })
    }

    fn props(&mut self, count: usize) -> Result<Vec<Prop>> {
        self.many(count, |r| {
            let id = r.u32()?;
            let code = r.i16()?;
            let n = r.u16()? as usize;
            let value = r
                .value(code, n)
                .map_err(|e| SpuiError(format!("property 0x{id:08X}: {e}")))?;
            Ok(Prop { id, value })
        })
    }

    fn hit_mask(&mut self, version: u16) -> Result<RawHitMask> {
        if version >= 3 {
            let rle = self.u8()? != 0;
            if rle {
                let width = self.i32()?;
                let height = self.i32()?;
                let count = self.i32()?;
                if width < 0 || height < 0 || !(0..=1 << 22).contains(&count) {
                    return err("corrupt run-length hit mask header");
                }
                let runs = self.many(count as usize, |r| r.u16())?;
                return Ok(RawHitMask::Rle {
                    width: width as u32,
                    height: height as u32,
                    runs,
                });
            }
        }
        Ok(RawHitMask::Image(self.key()?))
    }
}

/// Parse an SPUI container into its generic form.
pub fn parse_raw(data: &[u8]) -> Result<RawFile> {
    let mut r = Reader { data, pos: 0 };
    if r.u32()? != MAGIC {
        return err("not an SPUI file (bad header magic)");
    }
    let version = r.u16()?;
    if version > LAST_SUPPORTED_VERSION {
        return err(format!(
            "SPUI version {version} is not supported (maximum {LAST_SUPPORTED_VERSION})"
        ));
    }
    let counts = [r.u16()?, r.u16()?, r.u16()?, r.u16()?].map(usize::from);
    if counts.iter().sum::<usize>() > MAX_OBJECTS {
        return err("too many resources in SPUI");
    }
    let images = r.many(counts[0], |r| r.key())?;
    let atlases = r.many(counts[1], |r| r.key())?;
    let masks = r.many(counts[2], |r| r.hit_mask(version))?;
    let classes = r.many(counts[3], |r| r.u32())?;
    let other = counts[0] + counts[1] + counts[2];
    let mut elements: Vec<RawElement> = classes
        .into_iter()
        .map(|class| RawElement {
            class,
            props: Vec::new(),
            root: false,
        })
        .collect();
    let mut magic = r.u16()?;
    loop {
        let index = r.i16()?;
        if index == -1 {
            break;
        }
        if magic != CLASS_MAGIC {
            return err(format!("bad element magic 0x{magic:04X} at byte {}", r.pos));
        }
        let slot = (index as i64) - other as i64;
        if slot < 0 || slot as usize >= elements.len() {
            return err(format!("element reference {index} out of range"));
        }
        let word = r.u16()?;
        let props = r
            .props((word & !ROOT_FLAG) as usize)
            .map_err(|e| SpuiError(format!("element {slot}: {e}")))?;
        let element = &mut elements[slot as usize];
        element.root = word & ROOT_FLAG != 0;
        element.props = props;
        magic = r.u16()?;
    }
    if r.u32()? != MAGIC_END {
        return err("bad end magic (file truncated or unsupported)");
    }
    Ok(RawFile {
        version,
        images,
        atlases,
        masks,
        elements,
    })
}

/// Test support: a small writer producing valid containers.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    #[derive(Default)]
    pub struct Writer {
        pub bytes: Vec<u8>,
    }

    impl Writer {
        pub fn u8(&mut self, v: u8) -> &mut Self {
            self.bytes.push(v);
            self
        }
        pub fn u16(&mut self, v: u16) -> &mut Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn i16(&mut self, v: i16) -> &mut Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn u32(&mut self, v: u32) -> &mut Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn i32(&mut self, v: i32) -> &mut Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn f32(&mut self, v: f32) -> &mut Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn key(&mut self, k: ResourceKey) -> &mut Self {
            self.u32(k.instance_id).u32(k.type_id).u32(k.group_id)
        }
        /// Property header: id, type code, count.
        pub fn head(&mut self, id: u32, code: i16, count: u16) -> &mut Self {
            self.u32(id).i16(code).u16(count)
        }
        pub fn prop_u32(&mut self, id: u32, v: u32) -> &mut Self {
            self.head(id, ty::U32, 1).u32(v)
        }
        pub fn prop_rect(&mut self, id: u32, r: [f32; 4]) -> &mut Self {
            self.head(id, ty::RECT, 1).f32(r[0]).f32(r[1]).f32(r[2]).f32(r[3])
        }
        pub fn prop_refs(&mut self, id: u32, refs: &[i16]) -> &mut Self {
            self.head(id, ty::REF, refs.len() as u16);
            for r in refs {
                self.i16(*r);
            }
            self
        }
    }

    /// Element description for [`container`]: class, root flag, property bytes.
    pub struct El {
        pub class: u32,
        pub root: bool,
        pub count: u16,
        pub body: Vec<u8>,
    }

    pub fn el(class: u32, root: bool, build: impl FnOnce(&mut Writer) -> u16) -> El {
        let mut w = Writer::default();
        let count = build(&mut w);
        El {
            class,
            root,
            count,
            body: w.bytes,
        }
    }

    /// Assemble a version 3 container. `masks` are RLE masks `(w, h, runs)`.
    pub fn container(
        images: &[ResourceKey],
        atlases: &[ResourceKey],
        masks: &[(i32, i32, Vec<u16>)],
        elements: &[El],
    ) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(MAGIC).u16(3);
        w.u16(images.len() as u16)
            .u16(atlases.len() as u16)
            .u16(masks.len() as u16)
            .u16(elements.len() as u16);
        for k in images.iter().chain(atlases) {
            w.key(*k);
        }
        for (mw, mh, runs) in masks {
            w.u8(1).i32(*mw).i32(*mh).i32(runs.len() as i32);
            for r in runs {
                w.u16(*r);
            }
        }
        for e in elements {
            w.u32(e.class);
        }
        let other = (images.len() + atlases.len() + masks.len()) as i16;
        for (i, e) in elements.iter().enumerate() {
            w.u16(CLASS_MAGIC).i16(other + i as i16);
            w.u16(e.count | if e.root { ROOT_FLAG } else { 0 });
            w.bytes.extend_from_slice(&e.body);
        }
        w.u16(CLASS_MAGIC).i16(-1).u32(MAGIC_END);
        w.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn reads_a_minimal_container() {
        let atlas = ResourceKey {
            type_id: 0x2F7D_0004,
            group_id: 0x31A4_4893,
            instance_id: 0x1234_5678,
        };
        let bytes = container(
            &[],
            &[atlas],
            &[(4, 2, vec![1, 6])],
            &[el(0x4EC1_B8D8, true, |w| {
                w.prop_u32(0xEEC1_B000, 3)
                    .prop_rect(0xEEC1_B005, [1.0, 2.0, 30.0, 40.0]);
                2
            })],
        );
        let file = parse_raw(&bytes).unwrap();
        assert_eq!(file.version, 3);
        assert_eq!(file.atlases, vec![atlas]);
        assert_eq!(
            file.masks,
            vec![RawHitMask::Rle {
                width: 4,
                height: 2,
                runs: vec![1, 6]
            }]
        );
        let root = &file.elements[0];
        assert!(root.root);
        assert_eq!(root.class, 0x4EC1_B8D8);
        assert_eq!(root.u32_at(0xEEC1_B000, 0), Some(3));
        assert_eq!(root.rect(0xEEC1_B005), Some([1.0, 2.0, 30.0, 40.0]));
        assert_eq!(file.roots().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn references_resolve_across_the_combined_table() {
        let key = ResourceKey {
            type_id: 1,
            group_id: 2,
            instance_id: 3,
        };
        let bytes = container(
            &[key],
            &[key],
            &[(1, 1, vec![0, 0])],
            &[
                el(0x4EC1_B8D8, true, |w| {
                    w.prop_refs(0xEEC1_B00B, &[4, -1]);
                    1
                }),
                el(0x4EC1_B8D8, false, |_| 0),
            ],
        );
        let file = parse_raw(&bytes).unwrap();
        // 1 image + 1 atlas + 1 mask = 3 resources, so element 1 is index 4.
        assert_eq!(file.resolve(0), Target::Image(0));
        assert_eq!(file.resolve(1), Target::AtlasFile(0));
        assert_eq!(file.resolve(2), Target::HitMask(0));
        assert_eq!(file.resolve(3), Target::Element(0));
        assert_eq!(file.resolve(4), Target::Element(1));
        assert_eq!(file.resolve(-1), Target::Null);
        assert_eq!(file.resolve(40), Target::Null);
        assert_eq!(file.elements[0].reference(0xEEC1_B00B, 0), Some(4));
        assert_eq!(file.elements[0].reference(0xEEC1_B00B, 1), None);
    }

    #[test]
    fn text_and_struct_properties_round_trip() {
        let mut w = Writer::default();
        // Text: one literal "Hi", one localized.
        w.head(0xEEC1_B00A, ty::TEXT, 2).i16(2).u16(b'H' as u16).u16(b'i' as u16);
        w.i16(-1).u32(0xABCD).u32(0x42);
        // Struct: two structures with one float property each.
        w.head(0x0000_0001, ty::STRUCT, 2).i32(0);
        w.i16(1).head(7, ty::F32, 1).f32(0.5);
        w.i16(1).head(7, ty::F32, 1).f32(0.25);
        let body = w.bytes;
        let bytes = container(&[], &[], &[], &[el(0x4EC1_B8D8, true, |w| {
            w.bytes.extend_from_slice(&body);
            2
        })]);
        let file = parse_raw(&bytes).unwrap();
        let e = &file.elements[0];
        match e.get(0xEEC1_B00A).unwrap() {
            Value::Text(t) => {
                assert_eq!(t[0], Text::Literal("Hi".into()));
                assert_eq!(t[1], Text::Localized { table: 0xABCD, id: 0x42 });
            }
            other => panic!("unexpected {other:?}"),
        }
        let structs = e.structs(1);
        assert_eq!(structs.len(), 2);
        assert_eq!(structs[0].f32_at(7, 0), Some(0.5));
        assert_eq!(structs[1].f32_at(7, 0), Some(0.25));
    }

    #[test]
    fn rejects_bad_headers_and_truncation() {
        assert!(parse_raw(b"nope").is_err());
        let good = container(&[], &[], &[], &[el(0x4EC1_B8D8, true, |_| 0)]);
        for cut in 0..good.len() {
            assert!(parse_raw(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad_version = good.clone();
        bad_version[4] = 9;
        assert!(parse_raw(&bad_version).is_err());
        let mut bad_end = good.clone();
        let n = bad_end.len();
        bad_end[n - 1] ^= 0xFF;
        assert!(parse_raw(&bad_end).is_err());
    }

    #[test]
    fn int_and_float_helpers_widen() {
        assert_eq!(Value::U8(vec![200]).int_at(0), Some(200));
        assert_eq!(Value::I16(vec![-3]).int_at(0), Some(-3));
        assert_eq!(Value::Bool(vec![true]).int_at(0), Some(1));
        assert_eq!(Value::F32(vec![1.5]).float_at(0), Some(1.5));
        assert_eq!(Value::I32(vec![2]).float_at(0), Some(2.0));
        assert_eq!(Value::Text(vec![]).int_at(0), None);
    }
}
