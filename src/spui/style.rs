//! Text style sheet (`SporeUITextStyles`, resource type `0x0248F226`).
//!
//! The installed style sheet is a small CSS-like file:
//!
//! ```text
//! DefaultStyle(0x00aebb69){ font-family:"Trebuchet EA"; font-size:10px; ... }
//! DefaultStyleCentered(0x00aebb70) : DefaultStyle{ text-align:center; }
//! ```
//!
//! A window references its style by id (`TextFont`). Styles inherit from a
//! parent style. The reader follows SporeModder-FX `StyleSheet`.

use std::collections::HashMap;

use super::model::DEFAULT_STYLE;

/// Resource key parts of the installed style sheet.
pub const STYLE_SHEET_TYPE: u32 = 0x0248_F226;
pub const STYLE_SHEET_GROUP: u32 = 0x0248_E873;
pub const STYLE_SHEET_INSTANCE: u32 = 0x94DA_47E5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    None,
    Clip,
    Ellipsis,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub name: String,
    pub family: String,
    /// Em size in pixels.
    pub size_px: f32,
    pub bold: bool,
    pub italic: bool,
    pub align: HAlign,
    pub valign: VAlign,
    /// `color` as `0xAARRGGBB` when the style sets one.
    pub color: Option<u32>,
    pub background: Option<u32>,
    pub no_wrap: bool,
    pub overflow: Overflow,
    pub line_spacing: f32,
    pub letter_spacing: f32,
    pub underline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            name: "DefaultStyle".into(),
            family: "Trebuchet EA".into(),
            size_px: 10.0,
            bold: true,
            italic: false,
            align: HAlign::Left,
            valign: VAlign::Middle,
            color: None,
            background: None,
            no_wrap: false,
            overflow: Overflow::None,
            line_spacing: 0.0,
            letter_spacing: 0.0,
            underline: false,
        }
    }
}

#[derive(Debug, Clone)]
struct StyleDef {
    name: String,
    id: u32,
    parent: Option<usize>,
    props: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    styles: Vec<StyleDef>,
    by_id: HashMap<u32, usize>,
    by_name: HashMap<String, usize>,
    /// Font files named by `@font` statements.
    pub fonts: Vec<String>,
}

fn fnv1(name: &str) -> u32 {
    name.to_lowercase()
        .chars()
        .fold(0x811c_9dc5u32, |h, c| h.wrapping_mul(0x0100_0193) ^ c as u32)
}

/// `#RGB`, `#RRGGBB` or `#RRGGBBAA` (CSS order) to `0xAARRGGBB`.
pub fn parse_color(text: &str) -> Option<u32> {
    let hex = text.trim().strip_prefix('#')?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(match hex.len() {
        3 => {
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            0xFF00_0000 | (r * 17) << 16 | (g * 17) << 8 | b * 17
        }
        6 => 0xFF00_0000 | v,
        8 => (v << 24) | (v >> 8),
        _ => return None,
    })
}

fn parse_size(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(px) = t.strip_suffix("px") {
        px.trim().parse().ok()
    } else if let Some(pt) = t.strip_suffix("pt") {
        pt.trim().parse::<f32>().ok().map(|p| p * 96.0 / 72.0)
    } else {
        t.parse().ok()
    }
}

impl StyleSheet {
    pub fn parse(text: &str) -> StyleSheet {
        let mut sheet = StyleSheet::default();
        // Strip `//` comments line by line.
        let cleaned: String = text
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let mut rest = cleaned.as_str();
        while let Some(close) = rest.find('}') {
            let chunk = &rest[..close];
            rest = &rest[close + 1..];
            let Some(open) = chunk.find('{') else {
                continue;
            };
            let (head, body) = (&chunk[..open], &chunk[open + 1..]);
            // `@font "x.ttf" ;` statements may precede the style header.
            let mut head_text = head;
            while let Some(semi) = head_text.find(';') {
                let stmt = head_text[..semi].trim();
                if let Some(font) = stmt.strip_prefix("@font") {
                    sheet.fonts.push(font.trim().trim_matches('"').to_string());
                }
                head_text = &head_text[semi + 1..];
            }
            let head_text = head_text.trim();
            if head_text.is_empty() {
                continue;
            }
            let (name_part, parent_part) = match head_text.split_once(':') {
                Some((a, b)) => (a.trim(), Some(b.trim())),
                None => (head_text, None),
            };
            let (name, id) = match name_part.split_once('(') {
                Some((n, rest_id)) => {
                    let raw = rest_id.trim_end_matches(')').trim();
                    let id = raw
                        .strip_prefix("0x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| raw.parse().ok())
                        .unwrap_or_else(|| fnv1(n.trim()));
                    (n.trim().to_string(), id)
                }
                None => (name_part.to_string(), fnv1(name_part)),
            };
            let parent = parent_part.and_then(|p| sheet.by_name.get(p.trim()).copied());
            let mut props = HashMap::new();
            for entry in body.split([';', '\n']) {
                if let Some((k, v)) = entry.split_once(':') {
                    let (k, v) = (k.trim(), v.trim());
                    if !k.is_empty() && !v.is_empty() {
                        props.insert(k.to_string(), v.to_string());
                    }
                }
            }
            let index = sheet.styles.len();
            sheet.by_id.entry(id).or_insert(index);
            sheet.by_name.insert(name.clone(), index);
            sheet.styles.push(StyleDef {
                name,
                id,
                parent,
                props,
            });
        }
        sheet
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    pub fn id_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).map(|i| self.styles[*i].id)
    }

    fn lookup(&self, index: usize, key: &str) -> Option<&str> {
        let mut at = Some(index);
        let mut hops = 0;
        while let Some(i) = at {
            if let Some(v) = self.styles[i].props.get(key) {
                return Some(v.as_str());
            }
            at = self.styles[i].parent;
            hops += 1;
            if hops > 32 {
                break;
            }
        }
        None
    }

    /// Resolve a style by id; unknown ids fall back to `DefaultStyle`.
    pub fn style(&self, id: u32) -> TextStyle {
        let index = self
            .by_id
            .get(&id)
            .or_else(|| self.by_id.get(&DEFAULT_STYLE))
            .copied();
        let Some(index) = index else {
            return TextStyle::default();
        };
        let d = TextStyle::default();
        let get = |k: &str| self.lookup(index, k);
        TextStyle {
            name: self.styles[index].name.clone(),
            family: get("font-family")
                .map(|f| f.trim_matches('"').to_string())
                .unwrap_or(d.family),
            size_px: get("font-size").and_then(parse_size).unwrap_or(d.size_px),
            bold: get("font-weight")
                .map(|w| matches!(w.to_lowercase().as_str(), "bold" | "bolder"))
                .unwrap_or(d.bold),
            italic: get("font-style")
                .map(|s| matches!(s, "italic" | "oblique"))
                .unwrap_or(false),
            align: match get("text-align") {
                Some("center") => HAlign::Center,
                Some("right") => HAlign::Right,
                Some("justify") => HAlign::Justify,
                _ => HAlign::Left,
            },
            valign: match get("text-valign").map(|v| v.to_lowercase()).as_deref() {
                Some("top") => VAlign::Top,
                Some("bottom") => VAlign::Bottom,
                _ => VAlign::Middle,
            },
            color: get("color").and_then(parse_color),
            background: get("background-color").and_then(parse_color),
            no_wrap: matches!(get("wrap-option"), Some("no-wrap")),
            overflow: match get("text-overflow-mode") {
                Some("clip") => Overflow::Clip,
                Some("ellipsis") | Some("ellipsis-word") => Overflow::Ellipsis,
                _ => Overflow::None,
            },
            line_spacing: get("line-spacing").and_then(parse_size).unwrap_or(0.0),
            letter_spacing: get("letter-spacing").and_then(parse_size).unwrap_or(0.0),
            underline: get("text-decoration").is_some_and(|d| d.contains("underline")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
// comment
@font "PalatinoSansEAbold.ttf" ;
@font "TrebuchetEAbold.ttf" ;   // trailing comment

DefaultStyle(0x00aebb69){
   font-family:"Trebuchet EA";
   font-weight:bold;
   font-size:10px;
   text-valign:middle;
}

DefaultStyleCentered(0x00aebb70) : DefaultStyle{
   text-align:center;
}

Big(0x1234) : DefaultStyleCentered{
	font-size:14px; font-family:"Palatino Sans EA";
	text-align:right
	color:#ffcc2280;
}
"#;

    #[test]
    fn parses_styles_ids_parents_and_fonts() {
        let sheet = StyleSheet::parse(SAMPLE);
        assert_eq!(sheet.len(), 3);
        assert_eq!(sheet.fonts, vec!["PalatinoSansEAbold.ttf", "TrebuchetEAbold.ttf"]);
        let d = sheet.style(0x00aebb69);
        assert_eq!((d.family.as_str(), d.size_px, d.bold), ("Trebuchet EA", 10.0, true));
        assert_eq!(d.align, HAlign::Left);
        let c = sheet.style(0x00aebb70);
        assert_eq!(c.align, HAlign::Center);
        assert_eq!(c.family, "Trebuchet EA", "inherits from DefaultStyle");
        let big = sheet.style(0x1234);
        assert_eq!(big.size_px, 14.0);
        assert_eq!(big.family, "Palatino Sans EA");
        assert_eq!(big.align, HAlign::Right, "newline separated properties parse");
        assert_eq!(big.color, Some(0x80FF_CC22), "RRGGBBAA -> AARRGGBB");
        assert_eq!(sheet.id_of("Big"), Some(0x1234));
    }

    #[test]
    fn unknown_ids_fall_back_to_the_default_style() {
        let sheet = StyleSheet::parse(SAMPLE);
        assert_eq!(sheet.style(0xDEAD).name, "DefaultStyle");
        assert_eq!(StyleSheet::default().style(1).size_px, 10.0);
    }

    #[test]
    fn colour_forms() {
        assert_eq!(parse_color("#fff"), Some(0xFFFF_FFFF));
        assert_eq!(parse_color("#cc2255"), Some(0xFFCC_2255));
        assert_eq!(parse_color("#cc2255ff"), Some(0xFFCC_2255));
        assert_eq!(parse_color("cc2255"), None);
    }

    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn installed_style_sheet_parses() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let bytes = index
            .read(crate::dbpf::ResourceKey {
                type_id: STYLE_SHEET_TYPE,
                group_id: STYLE_SHEET_GROUP,
                instance_id: STYLE_SHEET_INSTANCE,
            })
            .expect("style sheet");
        let sheet = StyleSheet::parse(&String::from_utf8_lossy(&bytes));
        assert!(sheet.len() > 100, "{} styles", sheet.len());
        let t = sheet.style(0x323B_4096); // EditorBudgetLeft
        assert_eq!(t.size_px, 14.0);
        assert_eq!(sheet.style(0x015A_5D10).size_px, 14.0); // GuiData
        assert!(sheet.fonts.len() >= 4, "{:?}", sheet.fonts);
    }
}
