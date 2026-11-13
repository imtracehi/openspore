//! EAPd: the control-rate Pure Data engine Spore runs its adaptive music on.
//!
//! RETAIL (`notes\pd-music-re-notes.md`): Spore embeds EA's port of Pd 0.3x
//! with **message objects only** (no `~` signal objects exist in any installed
//! patch or in the engine's class table). Patches start and stop sounds and
//! set voice parameters by name; the host audio engine does all the sample
//! playback and filtering per voice. Game -> patch: global float parameters
//! (`0x5CA970(FNV(name), value)` -> `gfparam <name>`), instance parameters
//! (`fparam gain/pitch`), events; patch -> game: `sendgame`.
//!
//! Layout:
//! * [`parse`]: `pd` text records and `pdr` (`EAPD`) bundles -> [`parse::Canvas`].
//! * [`engine`]: object arena, depth-first message passing, `$0`/`$n`,
//!   subpatches and abstractions, the clock scheduler (logical seconds,
//!   fire everything due before now + frame delta capped at 0.5 s, 0xA492B0),
//!   instance pause, parameter registries and the host command queue.
//! * [`objects`]: the native classes (Pd vanilla control objects, GUI boxes as
//!   pass-through objects, Max-style `coll`/`gate`, and the EA classes
//!   `play/dplay/group/groupsize/mixer`, the `gfparam` input family,
//!   `sendgame`, `thisinstance`, `function`, `randweighted`).
//! * [`library`]: installed data (pd/pdr/mode/soundProp/instrument records).
//! * [`voice`]: the host side voice (a rodio source: gain, pitch, filters,
//!   pan, fades, seek, ring modulation, instrument notes).
//! * [`trace`]: headless runs (`--pd-trace <patch>`).

pub mod engine;
pub mod library;
pub mod objects;
pub mod parse;
pub mod trace;
pub mod voice;

#[cfg(test)]
mod tests;

use std::sync::Arc;

/// Interned-by-value symbol (patch names are short; clones are cheap).
pub type Sym = Arc<str>;

pub fn sym(s: &str) -> Sym {
    Arc::from(s)
}

/// A runtime atom.
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    F(f32),
    S(Sym),
}

impl Atom {
    pub fn float(&self) -> f32 {
        match self {
            Atom::F(f) => *f,
            Atom::S(_) => 0.0,
        }
    }
    pub fn symbol(&self) -> Option<&Sym> {
        match self {
            Atom::S(s) => Some(s),
            Atom::F(_) => None,
        }
    }
    /// Pd text form (`%g` floats).
    pub fn text(&self) -> String {
        match self {
            Atom::F(f) => fmt_g(*f),
            Atom::S(s) => s.to_string(),
        }
    }
}

/// Message selector. The common Pd types get their own variants so the hot
/// path never allocates a selector.
#[derive(Clone, Debug, PartialEq)]
pub enum Sel {
    Bang,
    Float,
    Symbol,
    List,
    Any(Sym),
}

/// A Pd message: selector plus arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub sel: Sel,
    pub args: Vec<Atom>,
}

impl Msg {
    pub fn bang() -> Self {
        Msg { sel: Sel::Bang, args: vec![] }
    }
    pub fn float(f: f32) -> Self {
        Msg { sel: Sel::Float, args: vec![Atom::F(f)] }
    }
    pub fn symbol(s: Sym) -> Self {
        Msg { sel: Sel::Symbol, args: vec![Atom::S(s)] }
    }
    pub fn list(args: Vec<Atom>) -> Self {
        Msg { sel: Sel::List, args }
    }
    pub fn any(sel: &str, args: Vec<Atom>) -> Self {
        Msg::from_selector(sym(sel), args)
    }
    /// A selector given as a symbol: the Pd type names map to their variants.
    pub fn from_selector(sel: Sym, args: Vec<Atom>) -> Self {
        let sel = match &*sel {
            "bang" => Sel::Bang,
            "float" => Sel::Float,
            "symbol" => Sel::Symbol,
            "list" => Sel::List,
            _ => Sel::Any(sel),
        };
        Msg { sel, args }
    }
    /// The message a run of atoms forms (message boxes, `coll` lines): a
    /// leading float makes a float (one atom) or a list, a leading symbol is
    /// the selector; nothing is a bang.
    pub fn from_atoms(mut atoms: Vec<Atom>) -> Self {
        match atoms.first() {
            None => Msg::bang(),
            Some(Atom::F(_)) if atoms.len() == 1 => Msg { sel: Sel::Float, args: atoms },
            Some(Atom::F(_)) => Msg::list(atoms),
            Some(Atom::S(s)) => {
                let s = s.clone();
                atoms.remove(0);
                Msg::from_selector(s, atoms)
            }
        }
    }
    /// The float a message carries: a float, or a list starting with one.
    pub fn as_float(&self) -> Option<f32> {
        match (&self.sel, self.args.first()) {
            (Sel::Float, Some(a)) => Some(a.float()),
            (Sel::List, Some(Atom::F(f))) => Some(*f),
            _ => None,
        }
    }
    /// The symbol a message carries: `symbol x`, a list starting with one,
    /// or (Max style, also how EAPd `coll` hands out names) a bare selector.
    pub fn as_symbol(&self) -> Option<Sym> {
        match (&self.sel, self.args.first()) {
            (Sel::Symbol, Some(Atom::S(s))) => Some(s.clone()),
            (Sel::List, Some(Atom::S(s))) => Some(s.clone()),
            (Sel::Any(s), _) => Some(s.clone()),
            _ => None,
        }
    }
    /// The selector as text (`float`, `bang`, `gain`, ...).
    pub fn selector(&self) -> &str {
        match &self.sel {
            Sel::Bang => "bang",
            Sel::Float => "float",
            Sel::Symbol => "symbol",
            Sel::List => "list",
            Sel::Any(s) => s,
        }
    }
    /// The atoms as a list (the selector of an `Any` message first).
    pub fn to_list(&self) -> Vec<Atom> {
        match &self.sel {
            Sel::Any(s) => {
                let mut v = vec![Atom::S(s.clone())];
                v.extend(self.args.iter().cloned());
                v
            }
            Sel::Bang => vec![],
            _ => self.args.clone(),
        }
    }
    /// Human readable (`gain 0.5`, `float 3`).
    pub fn text(&self) -> String {
        let mut out = self.selector().to_string();
        for a in &self.args {
            out.push(' ');
            out.push_str(&a.text());
        }
        out
    }
}

/// C `%g` formatting (6 significant digits, trailing zeros removed), the way
/// Pd prints floats into symbols (`$1-name`, `makefilename`).
pub fn fmt_g(f: f32) -> String {
    if f == 0.0 {
        return "0".into();
    }
    if !f.is_finite() {
        return if f.is_nan() { "nan".into() } else if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let exp = (f.abs() as f64).log10().floor() as i32;
    if !(-5..6).contains(&exp) {
        let s = format!("{:.5e}", f);
        // Rust: 1.23450e6 -> C: 1.2345e+06
        let (mant, e) = s.split_once('e').unwrap_or((&s, "0"));
        let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{mant}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, f);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Lowercase FNV-1 (the id of every named Spore resource and audio
/// parameter).
pub fn fnv(name: &str) -> u32 {
    crate::world_objects::name_id(name)
}
