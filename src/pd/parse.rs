//! Pd patch text and EAPD (`pdr`) bundles.
//!
//! RETAIL formats (`notes\pd-music-re-notes.md` 1.1, VERIFIED on the installed
//! records):
//! * `pd` (0x617715D9): Pd 0.3x text, CRLF, records `#N canvas ...;`,
//!   `#X obj x y class args;`, `#X msg`, `#X floatatom`, `#X text`,
//!   `#X restore x y pd name;`, `#X connect a o b i;`, `#C list idx atoms;`
//!   (Max `coll` data), `#X array` / `#A` (table data), `#X coords`.
//! * `pdr` (0x022D2C83): `"EAPD"` bundle of a top patch plus abstractions.
//!   v3: `u32 ver, u32 count, u32 total, u32, u32` then per entry
//!   `u32 nameLen, name[nameLen], tag[4], u32 size, data`; v2: 24-byte header
//!   then `name\0, u32 size, data`; legacy `"EAPD 1.0"`: `name[25]` (space
//!   padded), `u32 size, data` (test patches only).
//!
//! Tokens follow Pd's `binbuf_text` + `binbuf_restore`: whitespace separates,
//! `\` escapes, an unescaped `;` ends a record, `\,` / `\;` are the comma and
//! semicolon atoms of message boxes, `$n` / `\$n` are dollar arguments and a
//! symbol containing `$<digit>` is a dollar symbol (`\$0-name`).

use super::{Atom, Sym};
use std::sync::Arc;

/// A parse-time atom.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    F(f32),
    S(Sym),
    /// `$n` (n = 0 is the canvas id `$0`).
    Dollar(u32),
    /// A symbol containing `$n` (`$0-gain`, `$1-spulse-beatnum`).
    DollSym(Sym),
    Semi,
    Comma,
}

impl Tok {
    pub fn as_atom(&self) -> Option<Atom> {
        match self {
            Tok::F(f) => Some(Atom::F(*f)),
            Tok::S(s) => Some(Atom::S(s.clone())),
            _ => None,
        }
    }
    pub fn float(&self) -> Option<f32> {
        match self {
            Tok::F(f) => Some(*f),
            _ => None,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Tok::F(f) => super::fmt_g(*f),
            Tok::S(s) | Tok::DollSym(s) => s.to_string(),
            Tok::Dollar(n) => format!("${n}"),
            Tok::Semi => ";".into(),
            Tok::Comma => ",".into(),
        }
    }
}

fn is_pd_float(t: &str) -> Option<f32> {
    let first = t.chars().next()?;
    if !(first.is_ascii_digit() || first == '-' || first == '.') {
        return None;
    }
    if !t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) {
        return None;
    }
    t.parse::<f32>().ok().filter(|f| f.is_finite())
}

fn classify(text: String, escaped_only: bool) -> Tok {
    if escaped_only {
        // The whole atom was an escaped character: `\,` `\;`.
        match text.as_str() {
            "," => return Tok::Comma,
            ";" => return Tok::Semi,
            _ => {}
        }
    }
    if let Some(rest) = text.strip_prefix('$') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Tok::Dollar(rest.parse().unwrap_or(0));
        }
    }
    let bytes = text.as_bytes();
    if bytes.windows(2).any(|w| w[0] == b'$' && w[1].is_ascii_digit()) {
        return Tok::DollSym(Arc::from(text.as_str()));
    }
    if !escaped_only {
        if let Some(f) = is_pd_float(&text) {
            return Tok::F(f);
        }
    }
    Tok::S(Arc::from(text.as_str()))
}

/// Split Pd text into records of tokens (one per unescaped `;`).
pub fn records(text: &str) -> Vec<Vec<Tok>> {
    let mut out = vec![];
    let mut rec: Vec<Tok> = vec![];
    let mut cur = String::new();
    let mut in_atom = false;
    // An escape made the atom a symbol (`\,` alone is a comma atom).
    let mut escaped = false;
    let mut only_escaped = true;
    let mut chars = text.chars().peekable();
    let flush = |cur: &mut String, rec: &mut Vec<Tok>, in_atom: &mut bool, escaped: &mut bool, only: &mut bool| {
        if *in_atom {
            let t = std::mem::take(cur);
            let tok = if *escaped && *only && (t == "," || t == ";") {
                classify(t, true)
            } else if *escaped && !t.contains('$') {
                // Escaped characters make a symbol (Pd never reads `\1` as a number).
                Tok::S(Arc::from(t.as_str()))
            } else {
                classify(t, false)
            };
            rec.push(tok);
        }
        *in_atom = false;
        *escaped = false;
        *only = true;
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    in_atom = true;
                    if n != '$' {
                        escaped = true;
                    } else {
                        only_escaped = false;
                    }
                }
            }
            ';' => {
                flush(&mut cur, &mut rec, &mut in_atom, &mut escaped, &mut only_escaped);
                if !rec.is_empty() {
                    out.push(std::mem::take(&mut rec));
                }
            }
            ',' => {
                flush(&mut cur, &mut rec, &mut in_atom, &mut escaped, &mut only_escaped);
                rec.push(Tok::Comma);
            }
            c if c.is_whitespace() => {
                flush(&mut cur, &mut rec, &mut in_atom, &mut escaped, &mut only_escaped);
            }
            c => {
                cur.push(c);
                in_atom = true;
                only_escaped = false;
            }
        }
    }
    flush(&mut cur, &mut rec, &mut in_atom, &mut escaped, &mut only_escaped);
    if !rec.is_empty() {
        out.push(rec);
    }
    out
}

/// One box of a canvas, in file order (the index connections use).
#[derive(Clone, Debug)]
pub struct BoxDef {
    pub x: f32,
    pub y: f32,
    pub kind: BoxKind,
}

#[derive(Clone, Debug)]
pub enum BoxKind {
    /// `#X obj`: class (a symbol, a float for number boxes such as `[5]`, or
    /// nothing for an empty box) and arguments; `data` holds the `#C` lines
    /// that follow (coll contents) and `#A` lines (table contents).
    Obj { class: Option<Tok>, args: Vec<Tok>, data: Vec<Vec<Tok>>, array: Vec<Vec<Tok>> },
    Msg(Vec<Tok>),
    /// `#X floatatom x y width min max flag label receive send`.
    FloatAtom { min: f32, max: f32, receive: Option<Tok>, send: Option<Tok> },
    SymbolAtom { receive: Option<Tok>, send: Option<Tok> },
    /// Comments count as boxes for connection indices.
    Text,
    /// `#X restore x y pd name` (subpatch) or `graph` (array holder).
    Sub(Arc<Canvas>),
}

/// `#X array name size float flags` (+ `#A start values...`) inside a graph.
#[derive(Clone, Debug)]
pub struct ArrayDef {
    pub name: Tok,
    pub size: usize,
    pub values: Vec<(usize, Vec<f32>)>,
}

#[derive(Clone, Debug, Default)]
pub struct Canvas {
    pub name: String,
    pub boxes: Vec<BoxDef>,
    /// (from box, outlet, to box, inlet), file order.
    pub connects: Vec<(usize, usize, usize, usize)>,
    pub arrays: Vec<ArrayDef>,
}

fn f_at(r: &[Tok], i: usize) -> f32 {
    r.get(i).and_then(|t| t.float()).unwrap_or(0.0)
}

fn name_at(r: &[Tok], i: usize) -> Option<Tok> {
    match r.get(i) {
        None => None,
        Some(Tok::S(s)) if &**s == "-" || &**s == "empty" => None,
        Some(t) => Some(t.clone()),
    }
}

/// Parse one patch text into its root canvas (subpatches nested).
pub fn parse_patch(name: &str, text: &str) -> Result<Canvas, String> {
    let mut stack: Vec<Canvas> = vec![];
    // The last record defined an array (else `#A` belongs to the last box).
    let mut last_array = false;
    for rec in records(text) {
        let head = |i: usize| match rec.get(i) {
            Some(Tok::S(s)) => Some(&**s),
            _ => None,
        };
        match (head(0), head(1)) {
            (Some("#N"), Some("canvas")) => {
                let mut c = Canvas::default();
                // `#N canvas x y w h name vis` for subpatches, font size for the root.
                c.name = rec.get(6).map(|t| t.text()).unwrap_or_default();
                stack.push(c);
            }
            (Some("#X"), Some("restore")) => {
                let Some(sub) = stack.pop() else {
                    return Err(format!("{name}: restore without canvas"));
                };
                let parent = stack.last_mut().ok_or_else(|| format!("{name}: restore at top level"))?;
                parent.boxes.push(BoxDef { x: f_at(&rec, 2), y: f_at(&rec, 3), kind: BoxKind::Sub(Arc::new(sub)) });
            }
            (Some("#X"), Some(kind)) => {
                let Some(c) = stack.last_mut() else {
                    return Err(format!("{name}: #X before #N canvas"));
                };
                let (x, y) = (f_at(&rec, 2), f_at(&rec, 3));
                match kind {
                    "obj" => c.boxes.push(BoxDef {
                        x,
                        y,
                        kind: BoxKind::Obj {
                            class: rec.get(4).cloned(),
                            args: rec.get(5..).map(|s| s.to_vec()).unwrap_or_default(),
                            data: vec![],
                            array: vec![],
                        },
                    }),
                    "msg" => c.boxes.push(BoxDef { x, y, kind: BoxKind::Msg(rec.get(4..).map(|s| s.to_vec()).unwrap_or_default()) }),
                    "floatatom" => c.boxes.push(BoxDef {
                        x,
                        y,
                        kind: BoxKind::FloatAtom { min: f_at(&rec, 5), max: f_at(&rec, 6), receive: name_at(&rec, 9), send: name_at(&rec, 10) },
                    }),
                    "symbolatom" => c.boxes.push(BoxDef { x, y, kind: BoxKind::SymbolAtom { receive: name_at(&rec, 9), send: name_at(&rec, 10) } }),
                    "text" => c.boxes.push(BoxDef { x, y, kind: BoxKind::Text }),
                    "connect" => {
                        let n = |i: usize| f_at(&rec, i).max(0.0) as usize;
                        c.connects.push((n(2), n(3), n(4), n(5)));
                    }
                    "array" => c.arrays.push(ArrayDef {
                        name: rec.get(2).cloned().unwrap_or(Tok::S(Arc::from("array"))),
                        size: f_at(&rec, 3).max(0.0) as usize,
                        values: vec![],
                    }),
                    // coords, scalar, struct, ...: no runtime meaning here.
                    _ => {}
                }
            }
            (Some("#C"), _) => {
                if let Some(BoxDef { kind: BoxKind::Obj { data, .. }, .. }) = stack.last_mut().and_then(|c| c.boxes.last_mut()) {
                    data.push(rec[1..].to_vec());
                }
            }
            (Some("#A"), _) => {
                let Some(c) = stack.last_mut() else { continue };
                // `#A` follows its owner: a `#X array` of a graph, or an EAPd
                // `table` / `randweighted` box (`#A setproperties ...`,
                // `#A <start> values...`).
                let start = f_at(&rec, 1).max(0.0) as usize;
                let values: Vec<f32> = rec[2.min(rec.len())..].iter().filter_map(|t| t.float()).collect();
                if last_array {
                    if let Some(a) = c.arrays.last_mut() {
                        a.values.push((start, values));
                    }
                } else if let Some(BoxDef { kind: BoxKind::Obj { array, .. }, .. }) = c.boxes.last_mut() {
                    array.push(rec[1..].to_vec());
                }
                continue;
            }
            _ => {}
        }
        last_array = matches!((rec.first(), rec.get(1)), (Some(Tok::S(a)), Some(Tok::S(b))) if &**a == "#X" && &**b == "array");
    }
    // Unclosed subpatches: fold them into their parents.
    while stack.len() > 1 {
        let sub = stack.pop().unwrap();
        stack.last_mut().unwrap().boxes.push(BoxDef { x: 0.0, y: 0.0, kind: BoxKind::Sub(Arc::new(sub)) });
    }
    let mut root = stack.pop().ok_or_else(|| format!("{name}: no canvas"))?;
    root.name = name.to_string();
    Ok(root)
}

/// One entry of an EAPD bundle.
#[derive(Clone, Debug)]
pub struct BundleEntry {
    pub name: String,
    pub tag: String,
    pub data: Vec<u8>,
}

/// Parse a `pdr` record into its entries.
pub fn parse_bundle(b: &[u8]) -> Result<Vec<BundleEntry>, String> {
    let u32_at = |p: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?)) };
    if b.get(..4) != Some(b"EAPD") {
        return Err("not an EAPD bundle".into());
    }
    let mut out = vec![];
    if b.get(4..8) == Some(b" 1.0") {
        // Legacy test patches: name[25] (space padded), u32 size, data.
        let mut pos = 24;
        while pos + 29 <= b.len() {
            let name = String::from_utf8_lossy(&b[pos..pos + 25]).trim().trim_end_matches('\0').to_string();
            pos += 25;
            let size = u32_at(pos).ok_or("truncated")? as usize;
            pos += 4;
            let data = b.get(pos..pos + size).ok_or("truncated entry")?.to_vec();
            pos += size;
            out.push(BundleEntry { name, tag: "pd".into(), data });
        }
        return Ok(out);
    }
    let ver = u32_at(4).ok_or("truncated header")?;
    if ver == 2 {
        let count = u32_at(16).ok_or("truncated header")? as usize;
        let mut pos = 24;
        for _ in 0..count {
            let end = b[pos..].iter().position(|c| *c == 0).ok_or("unterminated name")? + pos;
            let name = String::from_utf8_lossy(&b[pos..end]).to_string();
            pos = end + 1;
            let size = u32_at(pos).ok_or("truncated")? as usize;
            pos += 4;
            let data = b.get(pos..pos + size).ok_or("truncated entry")?.to_vec();
            pos += size;
            out.push(BundleEntry { name, tag: "pd".into(), data });
        }
        return Ok(out);
    }
    let mut pos = 24;
    while pos + 8 <= b.len() {
        let nlen = u32_at(pos).ok_or("truncated")? as usize;
        pos += 4;
        let raw = b.get(pos..pos + nlen).ok_or("truncated name")?;
        let name = String::from_utf8_lossy(raw.split(|c| *c == 0).next().unwrap_or(raw)).to_string();
        pos += nlen;
        let tagb = b.get(pos..pos + 4).ok_or("truncated tag")?;
        let tag = String::from_utf8_lossy(tagb.split(|c| *c == 0).next().unwrap_or(tagb)).to_string();
        pos += 4;
        let size = u32_at(pos).ok_or("truncated")? as usize;
        pos += 4;
        let data = b.get(pos..pos + size).ok_or("truncated entry")?.to_vec();
        pos += size;
        out.push(BundleEntry { name, tag, data });
    }
    Ok(out)
}
