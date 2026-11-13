//! Bounded, read-only parsing of installed Spore BLD/VCL/UFO creation XML.
//! Positions and orientation rows retain the original Z-up basis. Rendering,
//! paint approximation and morph application belong to separate modules.
use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};
use quick_xml::{events::Event, Reader};
use std::collections::HashMap;

pub const BLD: u32 = 0x2399be55;
pub const VCL: u32 = 0x24682294;
pub const UFO: u32 = 0x476a98c7;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 16_384;
const MAX_BLOCKS: usize = 512;

#[derive(Debug, Clone, Default)]
pub struct CreationCatalog {
    pub creations: Vec<CreationRecord>,
    pub errors: Vec<String>,
}

impl CreationCatalog {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| matches!(k.type_id, BLD | VCL | UFO))
            .collect();
        keys.sort_by_key(|k| (k.type_id, k.group_id, k.instance_id));
        let mut catalog = Self::default();
        for key in keys {
            let result = index
                .read(key)
                .and_then(|bytes| parse_creation(key, &bytes));
            match result {
                Ok(record) => catalog.creations.push(record),
                Err(error) => catalog.errors.push(format!(
                    "{:08x}!{:08x}.{:08x}: {error}",
                    key.group_id, key.instance_id, key.type_id,
                )),
            }
        }
        catalog
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreationRecord {
    pub key: ResourceKey,
    pub format_version: u32,
    pub name: String,
    pub model_type: u32,
    pub blocks: Vec<CreationBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreationBlock {
    pub definition: ResourceKey,
    pub scale: f32,
    pub position: [f32; 3],
    /// Raw XML row0, row1, row2, without transposition or basis conversion.
    pub orientation: [[f32; 3]; 3],
    pub paints: Vec<CreationPaint>,
    pub handles: Vec<(u32, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreationPaint {
    pub region: u32,
    pub paint_id: u32,
    pub color1: [f32; 3],
    pub color2: [f32; 3],
}

#[derive(Default)]
struct Node {
    name: String,
    attributes: HashMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn optional(&self, name: &str) -> Result<Option<&Node>, String> {
        let mut matches = self.children.iter().filter(|n| n.name == name);
        let first = matches.next();
        if matches.next().is_some() {
            return Err(format!("duplicate {name} in {}", self.name));
        }
        Ok(first)
    }
    fn required(&self, name: &str) -> Result<&Node, String> {
        self.optional(name)?
            .ok_or_else(|| format!("missing {name} in {}", self.name))
    }
    fn scalar(&self) -> Result<&str, String> {
        if !self.children.is_empty() {
            return Err(format!("nested elements in scalar {}", self.name));
        }
        Ok(self.text.trim())
    }
    fn value(&self, name: &str) -> Result<&str, String> {
        self.required(name)?.scalar()
    }
    fn entries(&self, name: &str, limit: usize) -> Result<Vec<&Node>, String> {
        let count = self
            .attributes
            .get("count")
            .ok_or_else(|| format!("missing count in {}", self.name))?
            .parse::<usize>()
            .map_err(|_| format!("invalid count in {}", self.name))?;
        if count > limit {
            return Err(format!("{} count exceeds limit {limit}", self.name));
        }
        let entries: Vec<_> = self.children.iter().filter(|n| n.name == name).collect();
        if entries.len() != count {
            return Err(format!(
                "{} count {count} differs from {} {name} entries",
                self.name,
                entries.len()
            ));
        }
        Ok(entries)
    }
}

fn append_node(stack: &mut [Node], root: &mut Option<Node>, node: Node) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err("multiple XML roots".into());
    }
    Ok(())
}

fn append_text(stack: &mut [Node], text: &str) -> Result<(), String> {
    if let Some(node) = stack.last_mut() {
        node.text.push_str(text);
    } else if !text.trim().is_empty() {
        return Err("text outside XML root".into());
    }
    Ok(())
}

fn xml_tree(bytes: &[u8]) -> Result<Node, String> {
    if bytes.len() > MAX_BYTES {
        return Err("creation XML exceeds byte limit".into());
    }
    // The installed records are UTF-8; do not silently replace invalid bytes.
    std::str::from_utf8(bytes).map_err(|_| "creation XML is not UTF-8")?;
    let mut reader = Reader::from_reader(bytes);
    let mut stack = Vec::new();
    let mut root = None;
    let mut nodes = 0;
    let mut declared = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("invalid creation XML: {e}"))?;
        let is_empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                nodes += 1;
                if nodes > MAX_NODES || stack.len() >= MAX_DEPTH {
                    return Err("creation XML exceeds node/depth limit".into());
                }
                let name = std::str::from_utf8(start.name().as_ref())
                    .map_err(|_| "invalid XML element name")?
                    .to_owned();
                let mut node = Node {
                    name,
                    ..Default::default()
                };
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|e| format!("invalid XML attribute: {e}"))?;
                    let name = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|_| "invalid XML attribute name")?
                        .to_owned();
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|e| format!("invalid XML attribute value: {e}"))?
                        .into_owned();
                    if node.attributes.insert(name, value).is_some() {
                        return Err("duplicate XML attribute".into());
                    }
                }
                if is_empty {
                    append_node(&mut stack, &mut root, node)?;
                } else {
                    stack.push(node);
                }
            }
            Event::End(end) => {
                let node = stack.pop().ok_or("unexpected XML closing element")?;
                if end.name().as_ref() != node.name.as_bytes() {
                    return Err("mismatched XML closing element".into());
                }
                append_node(&mut stack, &mut root, node)?;
            }
            Event::Text(text) => append_text(
                &mut stack,
                &text.xml10_content().map_err(|e| e.to_string())?,
            )?,
            Event::CData(text) => append_text(
                &mut stack,
                &text.xml10_content().map_err(|e| e.to_string())?,
            )?,
            Event::GeneralRef(reference) => {
                let reference = reference.decode().map_err(|e| e.to_string())?;
                let escaped = format!("&{reference};");
                let text = quick_xml::escape::unescape(&escaped)
                    .map_err(|e| format!("invalid XML reference: {e}"))?;
                append_text(&mut stack, &text)?;
            }
            Event::DocType(_) => return Err("creation XML DTDs are unsupported".into()),
            Event::Decl(declaration) => {
                if declared || root.is_some() || !stack.is_empty() {
                    return Err("misplaced XML declaration".into());
                }
                declared = true;
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|e| e.to_string())?;
                    if !encoding.eq_ignore_ascii_case(b"UTF-8") {
                        return Err("creation XML encoding must be UTF-8".into());
                    }
                }
            }
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err("unclosed creation XML element".into());
    }
    root.ok_or_else(|| "missing creation XML root".into())
}

fn identifier(value: &str) -> Result<u32, String> {
    let hex = value
        .trim()
        .strip_prefix("0x")
        .or_else(|| value.trim().strip_prefix("0X"))
        .ok_or_else(|| format!("expected hexadecimal resource identifier: {value}"))?;
    u32::from_str_radix(hex, 16).map_err(|_| format!("invalid resource identifier: {value}"))
}

fn number(value: &str) -> Result<f32, String> {
    let value = value
        .trim()
        .parse::<f32>()
        .map_err(|_| "invalid creation number")?;
    if !value.is_finite() {
        return Err("non-finite creation number".into());
    }
    Ok(value)
}

fn vector(value: &str) -> Result<[f32; 3], String> {
    let values: Vec<_> = value.split(',').map(number).collect::<Result<_, _>>()?;
    values
        .try_into()
        .map_err(|_| "creation vector must have three values".into())
}

pub fn parse_creation(key: ResourceKey, bytes: &[u8]) -> Result<CreationRecord, String> {
    if !matches!(key.type_id, BLD | VCL | UFO) {
        return Err("unsupported creation resource type".into());
    }
    let root = xml_tree(bytes)?;
    if root.name != "sporemodel" {
        return Err("expected sporemodel root".into());
    }
    let format_version = root
        .value("formatversion")?
        .parse::<u32>()
        .map_err(|_| "invalid creation format version")?;
    if !matches!(format_version, 15 | 16) {
        return Err(format!(
            "unsupported creation format version {format_version}"
        ));
    }
    let properties = root.required("properties")?;
    let model_type = identifier(properties.value("modeltype")?)?;
    let name = properties
        .optional("name")?
        .map(Node::scalar)
        .transpose()?
        .unwrap_or("")
        .to_owned();
    let mut blocks = Vec::new();
    for block in root.required("blocks")?.entries("blockref", MAX_BLOCKS)? {
        let ids: Vec<_> = block
            .value("blockid")?
            .split(',')
            .map(identifier)
            .collect::<Result<_, _>>()?;
        if ids.len() != 2 {
            return Err("blockid must contain group and instance".into());
        }
        let transform = block.required("transform")?;
        let scale = number(transform.value("scale")?)?;
        if scale <= 0.0 {
            return Err("creation scale must be positive".into());
        }
        let orientation = transform.required("orientation")?;
        let mut paints = Vec::new();
        if let Some(list) = block.optional("paintlist")? {
            for paint in list.entries("paint", 256)? {
                paints.push(CreationPaint {
                    region: identifier(paint.value("paintregion")?)?,
                    paint_id: identifier(paint.value("paintid")?)?,
                    color1: vector(paint.value("color1")?)?,
                    color2: vector(paint.value("color2")?)?,
                });
            }
        }
        let mut handles = Vec::new();
        if let Some(list) = block.optional("handles")? {
            for weight in list.entries("weight", 256)? {
                let channel = weight
                    .attributes
                    .get("channel")
                    .ok_or("missing weight channel")?;
                let channel = identifier(channel)?;
                if handles.iter().any(|(existing, _)| *existing == channel) {
                    return Err("duplicate morph handle channel".into());
                }
                handles.push((channel, number(weight.scalar()?)?));
            }
        }
        blocks.push(CreationBlock {
            definition: ResourceKey {
                type_id: crate::asset_catalog::PROP,
                group_id: ids[0],
                instance_id: ids[1],
            },
            scale,
            position: vector(transform.value("position")?)?,
            orientation: [
                vector(orientation.value("row0")?)?,
                vector(orientation.value("row1")?)?,
                vector(orientation.value("row2")?)?,
            ],
            paints,
            handles,
        });
    }
    Ok(CreationRecord {
        key,
        format_version,
        name,
        model_type,
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> ResourceKey {
        ResourceKey {
            type_id: VCL,
            group_id: 11,
            instance_id: 22,
        }
    }
    fn fixture() -> String {
        // Synthetic fixture: no installed game payload is embedded in source.
        "<sporemodel><formatversion>15</formatversion><properties><modeltype>0x1234</modeltype><name>A &amp; B &#x43;</name></properties><blocks count=\"1\"><blockref><blockid>0x11,0x22</blockid><transform><scale>2</scale><position>1,2,3</position><orientation><row0>0,-1,0</row0><row1>1,0,0</row1><row2>0,0,1</row2></orientation></transform><paintlist count=\"1\"><paint><paintregion>0x5</paintregion><paintid>0x6</paintid><color1>0.1,0.2,0.3</color1><color2>0.4,0.5,0.6</color2></paint></paintlist><handles count=\"1\"><weight channel=\"0x7\">0.75</weight></handles></blockref></blocks></sporemodel>".into()
    }
    #[test]
    fn retains_exact_keys_rows_paints_and_handles() {
        let record = parse_creation(key(), fixture().as_bytes()).unwrap();
        assert_eq!(record.key, key());
        assert_eq!(record.name, "A & B C");
        assert_eq!(record.model_type, 0x1234);
        assert_eq!(
            record.blocks[0].definition,
            ResourceKey {
                type_id: crate::asset_catalog::PROP,
                group_id: 0x11,
                instance_id: 0x22
            }
        );
        assert_eq!(record.blocks[0].scale, 2.0);
        assert_eq!(record.blocks[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(record.blocks[0].orientation[0], [0.0, -1.0, 0.0]);
        assert_eq!(record.blocks[0].paints[0].color2, [0.4, 0.5, 0.6]);
        assert_eq!(record.blocks[0].paints[0].region, 5);
        assert_eq!(record.blocks[0].paints[0].paint_id, 6);
        assert_eq!(record.blocks[0].handles, [(7, 0.75)]);
    }
    #[test]
    fn supports_v16_optional_name_empty_lists_and_unknown_fields() {
        let xml = fixture()
            .replace("<formatversion>15", "<formatversion>16")
            .replace(
                "<name>A &amp; B &#x43;</name>",
                "<future><nested/></future>",
            )
            .replace(
                "<handles count=\"1\"><weight channel=\"0x7\">0.75</weight></handles>",
                "<handles count=\"0\"/>",
            );
        let record = parse_creation(key(), xml.as_bytes()).unwrap();
        assert_eq!(record.format_version, 16);
        assert!(record.name.is_empty());
        assert!(record.blocks[0].handles.is_empty());
    }
    #[test]
    fn rejects_missing_duplicate_and_misnested_required_fields() {
        for xml in [
            fixture().replace("<scale>2</scale>", ""),
            fixture().replace("<scale>2</scale>", "<scale>2</scale><scale>3</scale>"),
            fixture().replace("<scale>2</scale>", "<scale><inner>2</inner></scale>"),
            fixture().replace("</orientation>", "</transform>"),
            format!("{}<sporemodel/>", fixture()),
        ] {
            assert!(parse_creation(key(), xml.as_bytes()).is_err());
        }
    }
    #[test]
    fn rejects_count_mismatch_invalid_versions_and_numbers() {
        for (from, to) in [
            ("blocks count=\"1\"", "blocks count=\"2\""),
            ("paintlist count=\"1\"", "paintlist count=\"0\""),
            ("handles count=\"1\"", "handles count=\"2\""),
            ("<formatversion>15", "<formatversion>17"),
            ("<scale>2", "<scale>NaN"),
            ("<scale>2", "<scale>0"),
            ("<position>1,2,3", "<position>1,inf,3"),
            ("<row0>0,-1,0", "<row0>0,-1"),
            ("channel=\"0x7\"", "channel=\"garbage\""),
        ] {
            assert!(
                parse_creation(key(), fixture().replace(from, to).as_bytes()).is_err(),
                "{to}"
            );
        }
    }
    #[test]
    fn rejects_dtd_unknown_entities_and_oversized_documents() {
        for xml in [
            format!(
                "<!DOCTYPE sporemodel SYSTEM 'file:///never-read'>{}",
                fixture()
            ),
            fixture().replace("&amp;", "&unknown;"),
            "x".repeat(MAX_BYTES + 1),
            format!(
                "{}{}{}",
                "<nested>".repeat(MAX_DEPTH + 1),
                fixture(),
                "</nested>".repeat(MAX_DEPTH + 1)
            ),
            fixture().replace("count=\"1\"", "count=\"513\""),
        ] {
            assert!(parse_creation(key(), xml.as_bytes()).is_err());
        }
    }
}
