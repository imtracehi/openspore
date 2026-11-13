//! Native classes of the EAPd engine.
//!
//! RETAIL class table (`notes\xstage-re\pd\eapd_class_setups.txt`, setup
//! functions 0xA51F60 / 0xA51FD0): Pd 0.3x vanilla control objects, the GUI
//! boxes `bng tgl hsl vsl` acting as control objects, Max-style `coll`,
//! `gate`, `switch`, and the EA classes `play dplay group groupsize mixer`,
//! the input family (`event fparam sparam gevent gfparam gsparam`),
//! `sendgame`, `thisinstance`, `function`, `randweighted`.
//!
//! Semantics follow Pd 0.39 vanilla where the class is vanilla. EA classes
//! follow the RE notes (2.4) and, where the notes are silent, how the
//! installed patches use them (INFERRED, marked):
//! * `gate n init`: inlet 0 = data, inlet 1 = open outlet (1-based, 0 shut)
//!   (`onebang`, the Cell egg-lay / unlock subpatches drive it that way).
//! * `coll [name] ...`: a symbol first argument names shared storage
//!   (`collread $0-scale-degrees` reads `coll $0-scale-degrees`); numeric
//!   arguments are sizes (the Cell patch has three unrelated `coll 11`).
//!   A single-symbol entry comes out as `symbol x` (so `[t b s]` passes it).
//! * `function w h c c c c xmax ymin ymax xmin y0 (x y)...`: points
//!   normalised to 0..1, output `ymin + y * (ymax - ymin)`, input clamped to
//!   the domain (every Cell function has ymin = xmin = 0: output y * ymax).
//! * `play`/`dplay` group picks: random = uniform, inorder = (last + 1) mod n,
//!   norepeat = random in [0, n-2], n-1 if it equals the last (0xA54150).
//!   `startselect i` is 0-based (`random 10 -> startselect $1` in tribe_drums3).
//! * `trigger`: Pd 0.39 types (`t b 1` = `t b f`: `uzi` needs that); `s` also
//!   takes a bare selector.
//! * `hsl`/`vsl` clip to their range (Pd vanilla); EA saves the slider value
//!   itself (`... -1 -1 0.614173 1 1`), output on load when `init` is set.

use super::engine::{Cx, HostCmd, Object, Setup, VoiceId};
use super::parse::Tok;
use super::{fmt_g, sym, Atom, Msg, Sel, Sym};
use std::collections::VecDeque;

const NATIVE: &[&str] = &[
    "+", "-", "*", "/", "pow", "max", "min", "==", "!=", ">", "<", ">=", "<=", "&", "&&", "|", "||", "<<", ">>", "%", "mod",
    "div", "sin", "cos", "tan", "atan", "atan2", "sqrt", "log", "exp", "abs", "clip", "mtof", "ftom", "powtodb", "rmstodb",
    "dbtopow", "dbtorms", "int", "i", "float", "f", "symbol", "bang", "b", "send", "s", "receive", "r", "forward", "select",
    "sel", "route", "pack", "unpack", "trigger", "t", "spigot", "gate", "switch", "moses", "until", "makefilename", "swap",
    "fswap", "change", "value", "v", "list", "delay", "del", "metro", "line", "timer", "pipe", "random", "frandom", "print",
    "loadbang", "bng", "tgl", "toggle", "hsl", "vsl", "hslider", "vslider", "table", "tabread", "tabread4", "tabwrite",
    "coll", "collread", "collwrite", "makenote", "stripnote", "poly", "bag", "namecanvas", "cputime", "serial", "menu",
    "openpanel", "savepanel", "play", "dplay", "group", "groupsize", "control", "mixer", "pixie", "trixie", "sendgame",
    "thisinstance", "event", "floatparam", "fparam", "symbolparam", "sparam", "globalevent", "gevent", "globalfloatparam",
    "gfparam", "globalsymbolparam", "gsparam", "function", "randweighted",
];

pub fn is_native(class: &str) -> bool {
    NATIVE.contains(&class)
}

fn f_arg(args: &[Atom], i: usize, default: f32) -> f32 {
    match args.get(i) {
        Some(Atom::F(f)) => *f,
        _ => default,
    }
}

fn s_arg(args: &[Atom], i: usize) -> Option<Sym> {
    match args.get(i) {
        Some(Atom::S(s)) if &**s != "empty" && &**s != "-" => Some(s.clone()),
        _ => None,
    }
}

fn atom_msg(a: &Atom) -> Msg {
    match a {
        Atom::F(f) => Msg::float(*f),
        Atom::S(s) => Msg::symbol(s.clone()),
    }
}

/// Create a native object (`None` if the class exists but cannot be made).
pub fn create(class: &str, args: &[Atom], data: &[Vec<Tok>], array: &[Vec<Tok>], s: &mut Setup) -> Option<Box<dyn Object>> {
    let b: Box<dyn Object> = match class {
        "+" | "-" | "*" | "/" | "pow" | "max" | "min" | "==" | "!=" | ">" | "<" | ">=" | "<=" | "&" | "&&" | "|" | "||" | "<<"
        | ">>" | "%" | "mod" | "div" | "atan2" => Box::new(Binop { op: binop(class), f1: 0.0, f2: f_arg(args, 0, 0.0) }),
        "sin" | "cos" | "tan" | "atan" | "sqrt" | "log" | "exp" | "abs" | "mtof" | "ftom" | "powtodb" | "rmstodb" | "dbtopow"
        | "dbtorms" => Box::new(Unop { op: unop(class) }),
        "clip" => Box::new(Clip { f: 0.0, lo: f_arg(args, 0, 0.0), hi: f_arg(args, 1, 0.0) }),
        "int" | "i" => Box::new(Value1 { f: f_arg(args, 0, 0.0).trunc(), int: true }),
        "float" | "f" => Box::new(Value1 { f: f_arg(args, 0, 0.0), int: false }),
        "symbol" => Box::new(SymbolBox { s: s_arg(args, 0).unwrap_or_else(|| sym("")) }),
        "bang" | "b" => Box::new(BangBox),
        "send" | "s" | "forward" => Box::new(Send { name: s_arg(args, 0).or_else(|| args.first().map(|a| sym(&a.text()))) }),
        "receive" | "r" => {
            let name = args.first().map(|a| sym(&a.text()));
            if let Some(n) = &name {
                s.bind(n, true);
            }
            Box::new(Receive)
        }
        "select" | "sel" => Box::new(Select { keys: if args.is_empty() { vec![Atom::F(0.0)] } else { args.to_vec() } }),
        "route" => Box::new(Route { keys: if args.is_empty() { vec![Atom::F(0.0)] } else { args.to_vec() } }),
        "pack" => Box::new(Pack::new(args)),
        "unpack" => Box::new(Unpack { n: if args.is_empty() { 2 } else { args.len() } }),
        "trigger" | "t" => Box::new(Trigger::new(args)),
        "spigot" => Box::new(Spigot { open: f_arg(args, 0, 0.0) != 0.0 }),
        "gate" => {
            let n = (f_arg(args, 0, 1.0) as i32).max(1) as usize;
            Box::new(Gate { n, state: (f_arg(args, 1, 0.0) as i32).clamp(0, n as i32) as usize })
        }
        "switch" => {
            let n = (f_arg(args, 0, 1.0) as i32).max(1) as usize;
            Box::new(Switch { n, state: (f_arg(args, 1, 0.0) as i32).clamp(0, n as i32) as usize })
        }
        "moses" => Box::new(Moses { n: f_arg(args, 0, 0.0) }),
        "until" => Box::new(Until),
        "makefilename" => Box::new(MakeFilename { fmt: s_arg(args, 0).unwrap_or_else(|| sym("file.%d")) }),
        "swap" | "fswap" => Box::new(Swap { f1: 0.0, f2: f_arg(args, 0, 0.0) }),
        "change" => Box::new(Change { last: f_arg(args, 0, 0.0) }),
        "value" | "v" => Box::new(ValueBox { name: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")) }),
        "list" => Box::new(List::new(args)),
        "delay" | "del" => Box::new(Delay { ms: f_arg(args, 0, 0.0) }),
        "metro" => Box::new(Metro { ms: f_arg(args, 0, 0.0).max(1.0), on: false }),
        "line" => Box::new(Line::new(f_arg(args, 0, 0.0), f_arg(args, 1, 20.0))),
        "timer" => Box::new(Timer { start: 0.0, paused_at: None }),
        "pipe" => Box::new(Pipe::new(args)),
        "random" => Box::new(Random { n: f_arg(args, 0, 1.0), state: s.make_seed() }),
        "frandom" => Box::new(FRandom { n: f_arg(args, 0, 1.0), state: s.make_seed() }),
        "print" => Box::new(Print { name: args.first().map(|a| a.text()).unwrap_or_else(|| "print".into()) }),
        "loadbang" => Box::new(Loadbang),
        "bng" => {
            let init = f_arg(args, 3, 0.0) != 0.0;
            let send = s_arg(args, 4);
            if let Some(r) = s_arg(args, 5) {
                s.bind(&r, false);
            }
            Box::new(Bng { init, send })
        }
        "tgl" | "toggle" => {
            let init = f_arg(args, 1, 0.0) != 0.0;
            let send = s_arg(args, 2);
            if let Some(r) = s_arg(args, 3) {
                s.bind(&r, false);
            }
            let saved = f_arg(args, 12, 0.0);
            let nonzero = f_arg(args, 13, 1.0);
            Box::new(Tgl { on: if init { saved } else { 0.0 }, nonzero: if nonzero != 0.0 { nonzero } else { 1.0 }, init, send })
        }
        "hsl" | "vsl" | "hslider" | "vslider" => {
            let (lo, hi) = (f_arg(args, 2, 0.0), f_arg(args, 3, 127.0));
            let init = f_arg(args, 5, 0.0) != 0.0;
            let send = s_arg(args, 6);
            if let Some(r) = s_arg(args, 7) {
                s.bind(&r, false);
            }
            let saved = f_arg(args, 15, lo);
            let mut sl = Slider { lo, hi, value: lo, init, send };
            sl.value = sl.clip(if init { saved } else { lo });
            Box::new(sl)
        }
        "table" => {
            // EAPd `table [name] [size]` (0xA62D00): a numeric first argument
            // is the size of an anonymous table (`table 16`); its contents
            // follow as `#A <start> values...`.
            let (name, size) = match args.first() {
                Some(Atom::S(n)) => (n.clone(), f_arg(args, 1, 100.0)),
                Some(Atom::F(f)) => (sym(&format!("#table-{}", s.id)), *f),
                None => (sym(&format!("#table-{}", s.id)), 100.0),
            };
            s.table(&name, (size as i32).max(1) as usize);
            for (start, vals) in array_lines(array) {
                if let Some(t) = s.table_mut(&name) {
                    for (k, v) in vals.into_iter().enumerate() {
                        if let Some(slot) = t.get_mut(start + k) {
                            *slot = v;
                        }
                    }
                }
            }
            Box::new(Table { name })
        }
        "tabread" | "tabread4" => Box::new(TabRead { name: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")) }),
        "tabwrite" => Box::new(TabWrite { name: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")), index: 0.0 }),
        "coll" => {
            let name = match args.first() {
                Some(Atom::S(n)) => n.clone(),
                _ => sym(&format!("#coll-{}", s.id)),
            };
            let coll = s.coll(&name);
            for line in data {
                let mut atoms: Vec<Atom> = line.iter().filter_map(|t| t.as_atom()).collect();
                if matches!(atoms.first(), Some(Atom::S(l)) if &**l == "list") {
                    atoms.remove(0);
                }
                if atoms.is_empty() {
                    continue;
                }
                let key = atoms.remove(0);
                coll.set(key, atoms);
            }
            Box::new(Coll { name })
        }
        "collread" | "collwrite" => Box::new(CollRead { name: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")) }),
        "play" | "dplay" => Box::new(Play::new(class == "dplay", args)),
        "group" => {
            if let Some(name) = args.first().map(|a| sym(&a.text())) {
                s.define_group(name, args[1..].iter().map(|a| sym(&a.text())).collect());
            }
            Box::new(Dead)
        }
        "groupsize" => Box::new(GroupSize { name: args.first().map(|a| sym(&a.text())) }),
        "mixer" => Box::new(Mixer { group: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")) }),
        "event" | "globalevent" | "gevent" | "floatparam" | "fparam" | "globalfloatparam" | "gfparam" | "symbolparam"
        | "sparam" | "globalsymbolparam" | "gsparam" => {
            let name = args.first().map(|a| a.text()).unwrap_or_default();
            let global = matches!(class, "globalevent" | "gevent" | "globalfloatparam" | "gfparam" | "globalsymbolparam" | "gsparam");
            let event = matches!(class, "event" | "globalevent" | "gevent");
            match (global, event) {
                (true, true) => s.bind_global_event(&name),
                (true, false) => s.bind_global_param(&name),
                (false, true) => s.bind_instance_event(&name),
                (false, false) => s.bind_instance_param(&name),
            }
            Box::new(Input { name, global, event })
        }
        "sendgame" => Box::new(SendGame { name: args.first().map(|a| sym(&a.text())).unwrap_or_else(|| sym("")) }),
        "thisinstance" => Box::new(ThisInstance),
        "function" => Box::new(Function::new(args)),
        "randweighted" => {
            let n = (f_arg(args, 0, 1.0) as i32).max(1) as usize;
            let mut w = vec![1.0; n];
            for (start, vals) in array_lines(array) {
                for (k, v) in vals.into_iter().enumerate() {
                    if let Some(slot) = w.get_mut(start + k) {
                        *slot = v;
                    }
                }
            }
            Box::new(RandWeighted { w })
        }
        "trixie" => {
            // `trixie <file>` (setup 0xA40E30, new 0xA40D80): the track list
            // of an installed eapdPixie file.
            let name = args.first().map(|a| a.text()).unwrap_or_default();
            let tracks = s.data_file(&name).map(|x| parse_tracks(&x)).unwrap_or_default();
            if tracks.is_empty() {
                s.error(format!("trixie {name}: no track data"));
            }
            Box::new(Trixie { tracks })
        }
        "pixie" => {
            // `pixie <file>` (setup 0xA3ED60): Max pattrstorage presets. The
            // only in-game user (`music_creature_editor4`: `pixie
            // new_creature_editor`) names a file that is not installed, so it
            // never outputs in retail; installed files load but recall
            // nothing (no in-game patch asks).
            let name = args.first().map(|a| a.text()).unwrap_or_default();
            if s.data_file(&name).is_none() {
                s.error(format!("pixie {name}: data file not installed (inert, as in retail)"));
            }
            Box::new(Dead)
        }
        _ => {
            // Registered but not used by the in-game patches (poly,
            // makenote, ...): accept and ignore.
            s.error(format!("{class}: not implemented (ignored)"));
            Box::new(Dead)
        }
    };
    Some(b)
}

/// `#A <start> values...` lines (EAPd also stores `#A setproperties ...`).
fn array_lines(lines: &[Vec<Tok>]) -> Vec<(usize, Vec<f32>)> {
    lines
        .iter()
        .filter_map(|l| {
            let start = l.first()?.float()?;
            Some((start.max(0.0) as usize, l[1..].iter().filter_map(|t| t.float()).collect()))
        })
        .collect()
}

/// EAPd `table`: an index in, the stored value out (clamped index, as
/// `tabread`); `fromlist` / `tolist` / `resize` / `const` / `normalize`
/// (0xA62D00). Named tables are shared with `tabread` / `tabwrite`.
struct Table {
    name: Sym,
}

impl Object for Table {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let name = self.name.clone();
        match m.selector() {
            "fromlist" => {
                if let Some(t) = cx.table(&name) {
                    for (i, a) in m.args.iter().enumerate() {
                        if i < t.len() {
                            t[i] = a.float();
                        } else {
                            t.push(a.float());
                        }
                    }
                }
            }
            "tolist" | "bang" => {
                let v: Vec<Atom> = cx.table(&name).map(|t| t.iter().map(|x| Atom::F(*x)).collect()).unwrap_or_default();
                cx.out(0, &Msg::list(v));
            }
            "resize" => {
                let n = f_arg(&m.args, 0, 1.0).max(1.0) as usize;
                if let Some(t) = cx.table(&name) {
                    t.resize(n, 0.0);
                }
            }
            "const" => {
                let v = f_arg(&m.args, 0, 0.0);
                if let Some(t) = cx.table(&name) {
                    t.iter_mut().for_each(|x| *x = v);
                }
            }
            "normalize" => {
                if let Some(t) = cx.table(&name) {
                    let max = t.iter().fold(0.0f32, |a, x| a.max(x.abs()));
                    if max > 0.0 {
                        t.iter_mut().for_each(|x| *x /= max);
                    }
                }
            }
            "setproperties" | "set" => {}
            "list" if m.args.len() >= 2 => {
                // (index value): write.
                let (i, v) = (m.args[0].float(), m.args[1].float());
                if let Some(t) = cx.table(&name) {
                    if !t.is_empty() {
                        let n = (i as i64).clamp(0, t.len() as i64 - 1) as usize;
                        t[n] = v;
                    }
                }
            }
            _ => {
                if let Some(i) = m.as_float() {
                    let v = match cx.table(&name) {
                        Some(t) if !t.is_empty() => t[(i as i64).clamp(0, t.len() as i64 - 1) as usize],
                        _ => 0.0,
                    };
                    cx.out_f(0, v);
                }
            }
        }
    }
}

/// Accepts everything, does nothing.
pub struct Dead;
impl Object for Dead {
    fn recv(&mut self, _: usize, _: &Msg, _: &mut Cx) {}
}

/// `inlet` / `outlet` (and anything that only passes messages on).
pub struct Thru;
impl Object for Thru {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        cx.out(0, m);
    }
}

// ---------------------------------------------------------------------------
// Message box, number boxes
// ---------------------------------------------------------------------------

/// `#X msg`: `$n` from the incoming message, `,` separates messages, `;`
/// sends the rest to a named receiver. `$0` is 0 in message boxes (Pd 0.3x).
pub struct MsgBox {
    toks: Vec<Tok>,
}

impl MsgBox {
    pub fn new(toks: Vec<Tok>, _dz: u32) -> Self {
        MsgBox { toks }
    }
}

fn eval_dollar(t: &Tok, argv: &[Atom], cx: &mut Cx) -> Option<Atom> {
    Some(match t {
        Tok::F(f) => Atom::F(*f),
        Tok::S(s) => Atom::S(s.clone()),
        Tok::Dollar(0) => Atom::F(0.0),
        Tok::Dollar(n) => match argv.get(*n as usize - 1) {
            Some(a) => a.clone(),
            None => {
                cx.error(format!("${n}: argument number out of range"));
                Atom::F(0.0)
            }
        },
        Tok::DollSym(s) => Atom::S(sym(&super::engine::realize_dollsym(s, 0, argv))),
        Tok::Semi | Tok::Comma => return None,
    })
}

impl Object for MsgBox {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if let Sel::Any(s) = &m.sel {
            match &**s {
                "set" => {
                    self.toks = m.args.iter().map(|a| match a {
                        Atom::F(f) => Tok::F(*f),
                        Atom::S(s) => Tok::S(s.clone()),
                    }).collect();
                    return;
                }
                "add" | "add2" => {
                    self.toks.extend(m.args.iter().map(|a| match a {
                        Atom::F(f) => Tok::F(*f),
                        Atom::S(s) => Tok::S(s.clone()),
                    }));
                    if &**s == "add" {
                        self.toks.push(Tok::Semi);
                    }
                    return;
                }
                _ => {}
            }
        }
        // message_list: the selector of an `Any` message is dropped.
        let argv: Vec<Atom> = m.args.clone();
        let mut target: Option<Sym> = None;
        let mut expect_target = false;
        let mut cur: Vec<Atom> = vec![];
        let toks = self.toks.clone();
        let flush = |cur: &mut Vec<Atom>, target: &Option<Sym>, cx: &mut Cx| {
            if cur.is_empty() {
                return;
            }
            let msg = Msg::from_atoms(std::mem::take(cur));
            match target {
                None => cx.out(0, &msg),
                Some(name) => cx.send(name, &msg),
            }
        };
        for t in &toks {
            match t {
                Tok::Semi => {
                    flush(&mut cur, &target, cx);
                    expect_target = true;
                }
                Tok::Comma => flush(&mut cur, &target, cx),
                t => {
                    let Some(a) = eval_dollar(t, &argv, cx) else { continue };
                    if expect_target {
                        expect_target = false;
                        target = Some(sym(&a.text()));
                    } else {
                        cur.push(a);
                    }
                }
            }
        }
        flush(&mut cur, &target, cx);
    }
}

/// `#X floatatom`: a number box (clips when min/max are set).
pub struct FloatAtom {
    value: f32,
    min: f32,
    max: f32,
    send: Option<Sym>,
}

impl FloatAtom {
    pub fn new(min: f32, max: f32, send: Option<Sym>) -> Self {
        FloatAtom { value: 0.0, min, max, send }
    }
    fn emit(&mut self, cx: &mut Cx) {
        let m = Msg::float(self.value);
        match &self.send {
            Some(s) => {
                let s = s.clone();
                cx.send(&s, &m);
            }
            None => cx.out(0, &m),
        }
    }
}

impl Object for FloatAtom {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match &m.sel {
            Sel::Bang => self.emit(cx),
            Sel::Any(s) if &**s == "set" => {
                self.value = m.args.first().map(|a| a.float()).unwrap_or(0.0);
            }
            _ => {
                if let Some(mut f) = m.as_float() {
                    if self.min != 0.0 || self.max != 0.0 {
                        f = f.clamp(self.min.min(self.max), self.max.max(self.min));
                    }
                    self.value = f;
                    self.emit(cx);
                }
            }
        }
    }
}

pub struct SymbolAtom {
    value: Sym,
    send: Option<Sym>,
}

impl SymbolAtom {
    pub fn new(send: Option<Sym>) -> Self {
        SymbolAtom { value: sym(""), send }
    }
}

impl Object for SymbolAtom {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if let Some(s) = m.as_symbol() {
            if &*s == "set" {
                if let Some(Atom::S(v)) = m.args.first() {
                    self.value = v.clone();
                }
                return;
            }
            self.value = s;
        } else if m.sel != Sel::Bang {
            return;
        }
        let msg = Msg::symbol(self.value.clone());
        match &self.send {
            Some(s) => {
                let s = s.clone();
                cx.send(&s, &msg)
            }
            None => cx.out(0, &msg),
        }
    }
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum B {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Max,
    Min,
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    BitAnd,
    And,
    BitOr,
    Or,
    Shl,
    Shr,
    Rem,
    Mod,
    IDiv,
    Atan2,
}

fn binop(c: &str) -> B {
    match c {
        "+" => B::Add,
        "-" => B::Sub,
        "*" => B::Mul,
        "/" => B::Div,
        "pow" => B::Pow,
        "max" => B::Max,
        "min" => B::Min,
        "==" => B::Eq,
        "!=" => B::Ne,
        ">" => B::Gt,
        "<" => B::Lt,
        ">=" => B::Ge,
        "<=" => B::Le,
        "&" => B::BitAnd,
        "&&" => B::And,
        "|" => B::BitOr,
        "||" => B::Or,
        "<<" => B::Shl,
        ">>" => B::Shr,
        "%" => B::Rem,
        "mod" => B::Mod,
        "div" => B::IDiv,
        _ => B::Atan2,
    }
}

/// Pd 0.39 `x_arithmetic.c` binary operators.
pub fn binop_eval(op: &str, f1: f32, f2: f32) -> f32 {
    eval_b(binop(op), f1, f2)
}

fn eval_b(op: B, f1: f32, f2: f32) -> f32 {
    let b = |c: bool| if c { 1.0 } else { 0.0 };
    match op {
        B::Add => f1 + f2,
        B::Sub => f1 - f2,
        B::Mul => f1 * f2,
        B::Div => {
            if f2 != 0.0 {
                f1 / f2
            } else {
                0.0
            }
        }
        B::Pow => {
            if f1 > 0.0 {
                f1.powf(f2)
            } else {
                0.0
            }
        }
        B::Max => f1.max(f2),
        B::Min => f1.min(f2),
        B::Eq => b(f1 == f2),
        B::Ne => b(f1 != f2),
        B::Gt => b(f1 > f2),
        B::Lt => b(f1 < f2),
        B::Ge => b(f1 >= f2),
        B::Le => b(f1 <= f2),
        B::BitAnd => ((f1 as i32) & (f2 as i32)) as f32,
        B::And => b((f1 as i32) != 0 && (f2 as i32) != 0),
        B::BitOr => ((f1 as i32) | (f2 as i32)) as f32,
        B::Or => b((f1 as i32) != 0 || (f2 as i32) != 0),
        B::Shl => ((f1 as i32).wrapping_shl(f2 as u32)) as f32,
        B::Shr => ((f1 as i32).wrapping_shr(f2 as u32)) as f32,
        B::Rem => {
            let n2 = match (f2 as i32).abs() {
                0 => 1,
                n => n,
            };
            ((f1 as i32) % n2) as f32
        }
        B::Mod => {
            let n2 = match (f2 as i32).abs() {
                0 => 1,
                n => n,
            };
            let mut r = (f1 as i32) % n2;
            if r < 0 {
                r += n2;
            }
            r as f32
        }
        B::IDiv => {
            let n2 = match (f2 as i32).abs() {
                0 => 1,
                n => n,
            };
            let mut n1 = f1 as i32;
            if n1 < 0 {
                n1 -= n2 - 1;
            }
            (n1 / n2) as f32
        }
        B::Atan2 => {
            if f1 == 0.0 && f2 == 0.0 {
                0.0
            } else {
                f1.atan2(f2)
            }
        }
    }
}

struct Binop {
    op: B,
    f1: f32,
    f2: f32,
}

impl Object for Binop {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if let Some(f) = m.as_float() {
                self.f2 = f;
            }
            return;
        }
        match m.sel {
            Sel::Bang => {}
            Sel::List if m.args.len() > 1 => {
                let a = m.args.clone();
                cx.distribute(self, &a);
                return;
            }
            _ => match m.as_float() {
                Some(f) => self.f1 = f,
                None => return,
            },
        }
        cx.out_f(0, eval_b(self.op, self.f1, self.f2));
    }
}

#[derive(Clone, Copy)]
enum U {
    Sin,
    Cos,
    Tan,
    Atan,
    Sqrt,
    Log,
    Exp,
    Abs,
    Mtof,
    Ftom,
    PowToDb,
    RmsToDb,
    DbToPow,
    DbToRms,
}

fn unop(c: &str) -> U {
    match c {
        "sin" => U::Sin,
        "cos" => U::Cos,
        "tan" => U::Tan,
        "atan" => U::Atan,
        "sqrt" => U::Sqrt,
        "log" => U::Log,
        "exp" => U::Exp,
        "abs" => U::Abs,
        "mtof" => U::Mtof,
        "ftom" => U::Ftom,
        "powtodb" => U::PowToDb,
        "rmstodb" => U::RmsToDb,
        "dbtopow" => U::DbToPow,
        _ => U::DbToRms,
    }
}

pub fn mtof(f: f32) -> f32 {
    if f <= -1500.0 {
        0.0
    } else {
        8.175_798_9 * (0.057_762_265 * f.min(1499.0)).exp()
    }
}

struct Unop {
    op: U,
}

impl Object for Unop {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let Some(f) = m.as_float() else { return };
        let r = match self.op {
            U::Sin => f.sin(),
            U::Cos => f.cos(),
            U::Tan => f.tan(),
            U::Atan => f.atan(),
            U::Sqrt => {
                if f > 0.0 {
                    f.sqrt()
                } else {
                    0.0
                }
            }
            U::Log => {
                if f > 0.0 {
                    f.ln()
                } else {
                    -1000.0
                }
            }
            U::Exp => f.min(87.3365).exp(),
            U::Abs => f.abs(),
            U::Mtof => mtof(f),
            U::Ftom => {
                if f > 0.0 {
                    17.312_34 * (0.122_312_2 * f).ln()
                } else {
                    -1500.0
                }
            }
            U::PowToDb => {
                if f <= 0.0 {
                    0.0
                } else {
                    (100.0 + 10.0 / std::f32::consts::LN_10 * f.ln()).max(0.0)
                }
            }
            U::RmsToDb => {
                if f <= 0.0 {
                    0.0
                } else {
                    (100.0 + 20.0 / std::f32::consts::LN_10 * f.ln()).max(0.0)
                }
            }
            U::DbToPow => {
                if f <= 0.0 {
                    0.0
                } else {
                    ((std::f32::consts::LN_10 * 0.1) * (f.min(870.0) - 100.0)).exp()
                }
            }
            U::DbToRms => {
                if f <= 0.0 {
                    0.0
                } else {
                    ((std::f32::consts::LN_10 * 0.05) * (f.min(485.0) - 100.0)).exp()
                }
            }
        };
        cx.out_f(0, r);
    }
}

struct Clip {
    f: f32,
    lo: f32,
    hi: f32,
}

impl Object for Clip {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        match inlet {
            1 => self.lo = m.as_float().unwrap_or(self.lo),
            2 => self.hi = m.as_float().unwrap_or(self.hi),
            _ => {
                if m.sel == Sel::List && m.args.len() > 1 {
                    let a = m.args.clone();
                    cx.distribute(self, &a);
                    return;
                }
                if let Some(f) = m.as_float() {
                    self.f = f;
                } else if m.sel != Sel::Bang {
                    return;
                }
                // Pd: x < lo ? lo : (x > hi ? hi : x)
                let r = if self.f < self.lo {
                    self.lo
                } else if self.f > self.hi {
                    self.hi
                } else {
                    self.f
                };
                cx.out_f(0, r);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Storage and flow
// ---------------------------------------------------------------------------

/// `float` / `int`.
struct Value1 {
    f: f32,
    int: bool,
}

impl Object for Value1 {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        let conv = |int: bool, f: f32| if int { f.trunc() } else { f };
        if inlet == 1 {
            if let Some(f) = m.as_float() {
                self.f = conv(self.int, f);
            }
            return;
        }
        match &m.sel {
            Sel::Bang => {}
            Sel::List if m.args.len() > 1 => {
                let a = m.args.clone();
                cx.distribute(self, &a);
                return;
            }
            Sel::Any(s) if &**s == "set" => {
                self.f = conv(self.int, m.args.first().map(|a| a.float()).unwrap_or(0.0));
                return;
            }
            _ => match m.as_float() {
                Some(f) => self.f = conv(self.int, f),
                None => return,
            },
        }
        cx.out_f(0, self.f);
    }
}

struct SymbolBox {
    s: Sym,
}

impl Object for SymbolBox {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if let Some(s) = m.as_symbol() {
                self.s = s;
            }
            return;
        }
        if m.sel != Sel::Bang {
            match m.as_symbol() {
                Some(s) => self.s = s,
                None => return,
            }
        }
        cx.out_sym(0, self.s.clone());
    }
}

struct BangBox;
impl Object for BangBox {
    fn recv(&mut self, _: usize, _: &Msg, cx: &mut Cx) {
        cx.out_bang(0);
    }
}

struct Send {
    name: Option<Sym>,
}

impl Object for Send {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if let Some(s) = m.as_symbol() {
                self.name = Some(s);
            }
            return;
        }
        if let Some(n) = self.name.clone() {
            cx.send(&n, m);
        }
    }
}

struct Receive;
impl Object for Receive {
    fn recv(&mut self, _: usize, _: &Msg, _: &mut Cx) {}
    fn external(&mut self, m: &Msg, cx: &mut Cx) {
        cx.out(0, m);
    }
}

struct Select {
    keys: Vec<Atom>,
}

impl Object for Select {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if self.keys.len() == 1 {
                if let Some(f) = m.as_float() {
                    self.keys[0] = Atom::F(f);
                } else if let Some(s) = m.as_symbol() {
                    self.keys[0] = Atom::S(s);
                }
            }
            return;
        }
        let reject = self.keys.len();
        if m.sel == Sel::List && m.args.len() > 1 {
            let a = m.args.clone();
            cx.distribute(self, &a);
            return;
        }
        if let Some(f) = m.as_float() {
            if let Some(k) = self.keys.iter().position(|a| matches!(a, Atom::F(x) if *x == f)) {
                cx.out_bang(k);
            } else {
                cx.out_f(reject, f);
            }
        } else if m.sel == Sel::Symbol || matches!(m.sel, Sel::List) {
            if let Some(s) = m.as_symbol() {
                if let Some(k) = self.keys.iter().position(|a| matches!(a, Atom::S(x) if *x == s)) {
                    cx.out_bang(k);
                } else {
                    cx.out_sym(reject, s);
                }
            }
        }
    }
}

struct Route {
    keys: Vec<Atom>,
}

impl Object for Route {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let reject = self.keys.len();
        let float_type = matches!(self.keys.first(), Some(Atom::F(_)));
        match &m.sel {
            Sel::Any(sel) => {
                if !float_type {
                    if let Some(k) = self.keys.iter().position(|a| matches!(a, Atom::S(x) if x == sel)) {
                        match m.args.first() {
                            Some(Atom::S(s)) => {
                                let mm = Msg::from_selector(s.clone(), m.args[1..].to_vec());
                                cx.out(k, &mm);
                            }
                            _ => cx.out_list(k, m.args.clone()),
                        }
                        return;
                    }
                }
                cx.out(reject, m);
            }
            _ => {
                let argv: Vec<Atom> = if m.sel == Sel::Bang { vec![] } else { m.args.clone() };
                if float_type {
                    if let Some(Atom::F(f)) = argv.first() {
                        if let Some(k) = self.keys.iter().position(|a| matches!(a, Atom::F(x) if x == f)) {
                            match argv.get(1) {
                                Some(Atom::S(s)) => {
                                    let mm = Msg::from_selector(s.clone(), argv[2..].to_vec());
                                    cx.out(k, &mm);
                                }
                                _ => cx.out_list(k, argv[1..].to_vec()),
                            }
                            return;
                        }
                    }
                } else {
                    let want = if argv.len() > 1 {
                        "list"
                    } else if argv.is_empty() {
                        "bang"
                    } else if matches!(argv[0], Atom::F(_)) {
                        "float"
                    } else {
                        "symbol"
                    };
                    if let Some(k) = self.keys.iter().position(|a| matches!(a, Atom::S(x) if &**x == want)) {
                        match want {
                            "list" => match &argv[0] {
                                Atom::S(s) => {
                                    let mm = Msg::from_selector(s.clone(), argv[1..].to_vec());
                                    cx.out(k, &mm)
                                }
                                _ => cx.out(k, &Msg::list(argv)),
                            },
                            "bang" => cx.out_bang(k),
                            "float" => cx.out_f(k, argv[0].float()),
                            _ => cx.out_sym(k, argv[0].symbol().cloned().unwrap_or_else(|| sym(""))),
                        }
                        return;
                    }
                }
                match argv.len() {
                    0 => cx.out_bang(reject),
                    _ => cx.out_list(reject, argv),
                }
            }
        }
    }
}

struct Pack {
    slots: Vec<Atom>,
}

impl Pack {
    fn new(args: &[Atom]) -> Self {
        let slots = if args.is_empty() {
            vec![Atom::F(0.0), Atom::F(0.0)]
        } else {
            args.iter()
                .map(|a| match a {
                    Atom::F(f) => Atom::F(*f),
                    Atom::S(s) if s.starts_with('s') => Atom::S(sym("symbol")),
                    Atom::S(_) => Atom::F(0.0),
                })
                .collect()
        };
        Pack { slots }
    }
}

impl Object for Pack {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet > 0 {
            if let Some(slot) = self.slots.get_mut(inlet) {
                match slot {
                    Atom::F(_) => {
                        if let Some(f) = m.as_float() {
                            *slot = Atom::F(f);
                        }
                    }
                    Atom::S(_) => {
                        if let Some(s) = m.as_symbol() {
                            *slot = Atom::S(s);
                        }
                    }
                }
            }
            return;
        }
        match &m.sel {
            Sel::Bang => {}
            Sel::List | Sel::Any(_) => {
                let a = m.to_list();
                if a.len() > 1 {
                    cx.distribute(self, &a);
                    return;
                }
            }
            _ => {
                if let Some(slot) = self.slots.get_mut(0) {
                    match (slot, m.args.first()) {
                        (slot @ Atom::F(_), Some(Atom::F(f))) => *slot = Atom::F(*f),
                        (slot @ Atom::S(_), Some(Atom::S(s))) => *slot = Atom::S(s.clone()),
                        _ => {}
                    }
                }
            }
        }
        cx.out(0, &Msg::list(self.slots.clone()));
    }
}

struct Unpack {
    n: usize,
}

impl Object for Unpack {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let atoms = m.to_list();
        let n = atoms.len().min(self.n);
        for i in (0..n).rev() {
            cx.out(i, &atom_msg(&atoms[i]));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tr {
    F,
    B,
    S,
    L,
    A,
}

struct Trigger {
    types: Vec<Tr>,
}

impl Trigger {
    fn new(args: &[Atom]) -> Self {
        let types = if args.is_empty() {
            vec![Tr::F, Tr::F]
        } else {
            args.iter()
                .map(|a| match a {
                    Atom::F(_) => Tr::F,
                    Atom::S(s) => match s.chars().next() {
                        Some('b') => Tr::B,
                        Some('s') => Tr::S,
                        Some('l') => Tr::L,
                        Some('a') => Tr::A,
                        _ => Tr::F,
                    },
                })
                .collect()
        };
        Trigger { types }
    }
}

impl Object for Trigger {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let types = self.types.clone();
        for (i, t) in types.iter().enumerate().rev() {
            match (&m.sel, t) {
                (_, Tr::B) => cx.out_bang(i),
                (Sel::Any(_), Tr::A) => cx.out(i, m),
                (Sel::Any(s), Tr::S) => cx.out_sym(i, s.clone()),
                (Sel::Any(_), _) => cx.error("trigger: can only convert 's' to 'b' or 'a'".into()),
                (_, Tr::F) => cx.out_f(i, m.args.first().map(|a| a.float()).unwrap_or(0.0)),
                (_, Tr::S) => cx.out_sym(i, m.args.first().and_then(|a| a.symbol().cloned()).unwrap_or_else(|| sym(""))),
                (_, Tr::A) | (_, Tr::L) => {
                    let a = if m.sel == Sel::Bang { vec![] } else { m.args.clone() };
                    cx.out_list(i, a)
                }
            }
        }
    }
}

struct Spigot {
    open: bool,
}

impl Object for Spigot {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.open = m.as_float().unwrap_or(0.0) != 0.0;
        } else if self.open {
            cx.out(0, m);
        }
    }
}

/// EAPd `gate n init`: inlet 0 data, inlet 1 selects the outlet (INFERRED).
struct Gate {
    n: usize,
    state: usize,
}

impl Object for Gate {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if let Some(f) = m.as_float() {
                self.state = (f as i32).clamp(0, self.n as i32) as usize;
            }
        } else if self.state > 0 {
            cx.out(self.state - 1, m);
        }
    }
}

struct Switch {
    n: usize,
    state: usize,
}

impl Object for Switch {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 0 {
            if let Some(f) = m.as_float() {
                self.state = (f as i32).clamp(0, self.n as i32) as usize;
            }
        } else if inlet == self.state {
            cx.out(0, m);
        }
    }
}

struct Moses {
    n: f32,
}

impl Object for Moses {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.n = m.as_float().unwrap_or(self.n);
            return;
        }
        if m.sel == Sel::List && m.args.len() > 1 {
            let a = m.args.clone();
            cx.distribute(self, &a);
            return;
        }
        if let Some(f) = m.as_float() {
            cx.out_f(if f < self.n { 0 } else { 1 }, f);
        }
    }
}

/// `until`: bangs until its right inlet is banged (or n times).
struct Until;

impl Object for Until {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            return; // not running: nothing to stop
        }
        let limit = match m.as_float() {
            Some(f) => (f.max(0.0) as usize).min(1_000_000),
            None => 1_000_000,
        };
        for _ in 0..limit {
            cx.out_bang(0);
            let mail = cx.take_mail();
            if mail.iter().any(|(i, _)| *i == 1) {
                return;
            }
        }
        if m.as_float().is_none() {
            cx.error("until: 1000000 iterations without a stop".into());
        }
    }
}

struct MakeFilename {
    fmt: Sym,
}

impl Object for MakeFilename {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if let Sel::Any(s) = &m.sel {
            if &**s == "set" {
                if let Some(Atom::S(f)) = m.args.first() {
                    self.fmt = f.clone();
                }
                return;
            }
        }
        let arg = m.args.first().cloned().or_else(|| m.as_symbol().map(Atom::S));
        let Some(arg) = arg else { return };
        let mut out = String::new();
        let mut it = self.fmt.chars().peekable();
        let mut used = false;
        while let Some(c) = it.next() {
            if c == '%' && !used {
                let mut spec = String::new();
                while let Some(&n) = it.peek() {
                    it.next();
                    if n.is_ascii_alphabetic() {
                        spec.push(n);
                        break;
                    }
                }
                used = true;
                match spec.as_str() {
                    "d" | "i" | "u" | "ld" => out.push_str(&(arg.float() as i64).to_string()),
                    "x" => out.push_str(&format!("{:x}", arg.float() as i64)),
                    "c" => out.push(char::from_u32(arg.float() as u32).unwrap_or('?')),
                    "f" | "g" | "e" => out.push_str(&fmt_g(arg.float())),
                    _ => out.push_str(&arg.text()),
                }
            } else {
                out.push(c);
            }
        }
        cx.out_sym(0, sym(&out));
    }
}

struct Swap {
    f1: f32,
    f2: f32,
}

impl Object for Swap {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.f2 = m.as_float().unwrap_or(self.f2);
            return;
        }
        if m.sel == Sel::List && m.args.len() > 1 {
            let a = m.args.clone();
            cx.distribute(self, &a);
            return;
        }
        if let Some(f) = m.as_float() {
            self.f1 = f;
        } else if m.sel != Sel::Bang {
            return;
        }
        cx.out_f(1, self.f1);
        cx.out_f(0, self.f2);
    }
}

struct Change {
    last: f32,
}

impl Object for Change {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match &m.sel {
            Sel::Bang => cx.out_f(0, self.last),
            Sel::Any(s) if &**s == "set" => self.last = m.args.first().map(|a| a.float()).unwrap_or(0.0),
            _ => {
                if let Some(f) = m.as_float() {
                    if f != self.last {
                        self.last = f;
                        cx.out_f(0, f);
                    }
                }
            }
        }
    }
}

struct ValueBox {
    name: Sym,
}

impl Object for ValueBox {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match &m.sel {
            Sel::Bang => {
                let v = *cx.value(&self.name);
                cx.out_f(0, v);
            }
            Sel::Any(s) if &**s == "set" => {
                if let Some(Atom::S(n)) = m.args.first() {
                    self.name = n.clone();
                }
            }
            _ => {
                if let Some(f) = m.as_float() {
                    *cx.value(&self.name) = f;
                }
            }
        }
    }
}

enum ListOp {
    Append,
    Prepend,
    Split,
    Trim,
    /// EAPd `list message <selector>`: the list becomes `<selector> atoms...`
    /// (`list message fromlist` feeds `randweighted`).
    Message,
}

struct List {
    op: ListOp,
    stored: Vec<Atom>,
    n: f32,
}

impl List {
    fn new(args: &[Atom]) -> Self {
        let (op, rest) = match args.first() {
            Some(Atom::S(s)) => match &**s {
                "append" => (ListOp::Append, &args[1..]),
                "prepend" => (ListOp::Prepend, &args[1..]),
                "split" => (ListOp::Split, &args[1..]),
                "trim" => (ListOp::Trim, &args[1..]),
                "message" => (ListOp::Message, &args[1..]),
                _ => (ListOp::Append, args),
            },
            _ => (ListOp::Append, args),
        };
        let n = f_arg(rest, 0, 0.0);
        List { op, stored: rest.to_vec(), n }
    }
}

impl Object for List {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        let atoms = m.to_list();
        if inlet == 1 {
            match self.op {
                ListOp::Split => self.n = m.as_float().unwrap_or(self.n),
                _ => self.stored = atoms,
            }
            return;
        }
        match self.op {
            ListOp::Append => {
                let mut v = atoms;
                v.extend(self.stored.iter().cloned());
                cx.out(0, &list_msg(v));
            }
            ListOp::Prepend => {
                let mut v = self.stored.clone();
                v.extend(atoms);
                cx.out(0, &list_msg(v));
            }
            ListOp::Split => {
                let n = self.n.max(0.0) as usize;
                if atoms.len() >= n {
                    cx.out(1, &list_msg(atoms[n..].to_vec()));
                    cx.out(0, &list_msg(atoms[..n].to_vec()));
                } else {
                    cx.out(2, &list_msg(atoms));
                }
            }
            ListOp::Trim => cx.out(0, &Msg::from_atoms(atoms)),
            ListOp::Message => {
                let sel = self.stored.first().map(|a| a.text()).unwrap_or_else(|| "list".into());
                cx.out(0, &Msg::from_selector(sym(&sel), atoms));
            }
        }
    }
}

/// `outlet_list` (a list message, even of one element: receivers convert).
fn list_msg(v: Vec<Atom>) -> Msg {
    match v.len() {
        0 => Msg::bang(),
        1 => atom_msg(&v[0]),
        _ => Msg::list(v),
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

struct Delay {
    ms: f32,
}

impl Object for Delay {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.ms = m.as_float().unwrap_or(self.ms);
            return;
        }
        match &m.sel {
            Sel::Any(s) if &**s == "stop" => cx.unset(0),
            Sel::Bang => cx.delay(0, self.ms.max(0.0) as f64 * 0.001),
            _ => {
                if let Some(f) = m.as_float() {
                    self.ms = f;
                    cx.delay(0, self.ms.max(0.0) as f64 * 0.001);
                }
            }
        }
    }
    fn clock(&mut self, _: u32, cx: &mut Cx) {
        cx.out_bang(0);
    }
}

struct Metro {
    ms: f32,
    on: bool,
}

impl Object for Metro {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            if let Some(f) = m.as_float() {
                self.ms = f.max(1.0);
            }
            return;
        }
        let start = match &m.sel {
            Sel::Bang => true,
            Sel::Any(s) if &**s == "stop" => false,
            _ => match m.as_float() {
                Some(f) => f != 0.0,
                None => return,
            },
        };
        if start {
            self.on = true;
            cx.out_bang(0);
            if self.on {
                cx.delay(0, self.ms as f64 * 0.001);
            }
        } else {
            self.on = false;
            cx.unset(0);
        }
    }
    fn clock(&mut self, _: u32, cx: &mut Cx) {
        if !self.on {
            return;
        }
        // Reschedule first: a stop that arrives through a feedback loop
        // waits in the mailbox and unsets it after this method returns.
        cx.delay(0, self.ms as f64 * 0.001);
        cx.out_bang(0);
    }
}

/// Pd 0.39 `line` (control ramp, `grain` ms between outputs).
struct Line {
    setval: f64,
    targetval: f64,
    prevtime: f64,
    targettime: f64,
    one_over: f64,
    in1: f64,
    got_inlet: bool,
    grain: f64,
    paused_at: Option<f64>,
}

impl Line {
    fn new(init: f32, grain: f32) -> Self {
        Line {
            setval: init as f64,
            targetval: init as f64,
            prevtime: 0.0,
            targettime: 0.0,
            one_over: 0.0,
            in1: 0.0,
            got_inlet: false,
            grain: if grain > 0.0 { grain as f64 } else { 20.0 },
            paused_at: None,
        }
    }
    fn tick(&mut self, cx: &mut Cx) {
        let now = cx.now() * 1000.0;
        let msectogo = self.targettime - now;
        if msectogo < 1e-9 {
            cx.out_f(0, self.targetval as f32);
        } else {
            let v = self.setval + self.one_over * (now - self.prevtime) * (self.targetval - self.setval);
            cx.out_f(0, v as f32);
            cx.delay(0, self.grain.min(msectogo) * 0.001);
        }
    }
    fn go(&mut self, f: f64, cx: &mut Cx) {
        let now = cx.now() * 1000.0;
        if self.got_inlet && self.in1 > 0.0 {
            if now > self.targettime {
                self.setval = self.targetval;
            } else {
                self.setval += self.one_over * (now - self.prevtime) * (self.targetval - self.setval);
            }
            self.prevtime = now;
            self.targettime = now + self.in1;
            self.targetval = f;
            self.tick(cx);
            self.got_inlet = false;
            self.one_over = 1.0 / (self.targettime - now);
            cx.delay(0, self.grain.min(self.in1) * 0.001);
        } else {
            cx.unset(0);
            self.targetval = f;
            self.setval = f;
            cx.out_f(0, f as f32);
        }
        self.got_inlet = false;
    }
}

impl Object for Line {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        match inlet {
            1 => {
                if let Some(f) = m.as_float() {
                    self.in1 = f as f64;
                    self.got_inlet = true;
                }
            }
            2 => {
                if let Some(f) = m.as_float() {
                    self.grain = if f > 0.0 { f as f64 } else { 20.0 };
                }
            }
            _ => match &m.sel {
                Sel::Any(s) if &**s == "stop" => {
                    if cx.clock_set(0) {
                        let now = cx.now() * 1000.0;
                        if now < self.targettime {
                            self.setval += self.one_over * (now - self.prevtime) * (self.targetval - self.setval);
                        } else {
                            self.setval = self.targetval;
                        }
                        self.targetval = self.setval;
                    }
                    cx.unset(0);
                }
                Sel::Any(s) if &**s == "set" => {
                    cx.unset(0);
                    let f = m.args.first().map(|a| a.float()).unwrap_or(0.0) as f64;
                    self.targetval = f;
                    self.setval = f;
                }
                Sel::List if m.args.len() > 1 => {
                    let a = m.args.clone();
                    cx.distribute(self, &a);
                }
                _ => {
                    if let Some(f) = m.as_float() {
                        self.go(f as f64, cx);
                    }
                }
            },
        }
    }
    fn clock(&mut self, _: u32, cx: &mut Cx) {
        self.tick(cx);
    }
    fn pause(&mut self, paused: bool, cx: &mut Cx) {
        let now = cx.now() * 1000.0;
        if paused {
            self.paused_at = Some(now);
        } else if let Some(at) = self.paused_at.take() {
            // The ramp resumes where it stopped (EAPd line `pause`).
            let d = now - at;
            self.prevtime += d;
            self.targettime += d;
        }
    }
}

struct Timer {
    start: f64,
    paused_at: Option<f64>,
}

impl Object for Timer {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if m.sel != Sel::Bang {
            return;
        }
        if inlet == 0 {
            self.start = cx.now();
        } else {
            let now = self.paused_at.unwrap_or(cx.now());
            cx.out_f(0, ((now - self.start) * 1000.0) as f32);
        }
    }
    fn pause(&mut self, paused: bool, cx: &mut Cx) {
        if paused {
            self.paused_at = Some(cx.now());
        } else if let Some(at) = self.paused_at.take() {
            self.start += cx.now() - at;
        }
    }
}

/// `pipe [types...] delay`: delays messages.
struct Pipe {
    slots: Vec<Atom>,
    ms: f32,
    pending: VecDeque<(u32, Vec<Atom>)>,
    next_tag: u32,
}

impl Pipe {
    fn new(args: &[Atom]) -> Self {
        let (types, ms) = match args.len() {
            0 => (vec![Atom::F(0.0)], 0.0),
            1 => (vec![Atom::F(0.0)], args[0].float()),
            n => (
                args[..n - 1]
                    .iter()
                    .map(|a| match a {
                        Atom::S(s) if s.starts_with('s') => Atom::S(sym("symbol")),
                        Atom::F(f) => Atom::F(*f),
                        _ => Atom::F(0.0),
                    })
                    .collect(),
                args[n - 1].float(),
            ),
        };
        Pipe { slots: types, ms, pending: VecDeque::new(), next_tag: 1 }
    }
}

impl Object for Pipe {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == self.slots.len() {
            self.ms = m.as_float().unwrap_or(self.ms);
            return;
        }
        if inlet > 0 {
            if let Some(slot) = self.slots.get_mut(inlet) {
                *slot = match slot {
                    Atom::F(_) => Atom::F(m.as_float().unwrap_or(0.0)),
                    Atom::S(_) => Atom::S(m.as_symbol().unwrap_or_else(|| sym(""))),
                };
            }
            return;
        }
        match &m.sel {
            Sel::Any(s) if &**s == "flush" => {
                while let Some((tag, v)) = self.pending.pop_front() {
                    cx.unset(tag);
                    cx.out_list(0, v);
                }
                return;
            }
            Sel::Any(s) if &**s == "clear" => {
                while let Some((tag, _)) = self.pending.pop_front() {
                    cx.unset(tag);
                }
                return;
            }
            Sel::Bang => {}
            _ => {
                let a = m.to_list();
                for (k, v) in a.into_iter().enumerate() {
                    if k == self.slots.len() {
                        self.ms = v.float();
                    } else if let Some(slot) = self.slots.get_mut(k) {
                        *slot = v;
                    }
                }
            }
        }
        let tag = self.next_tag;
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        self.pending.push_back((tag, self.slots.clone()));
        cx.delay(tag, self.ms.max(0.0) as f64 * 0.001);
    }
    fn clock(&mut self, tag: u32, cx: &mut Cx) {
        if let Some(pos) = self.pending.iter().position(|(t, _)| *t == tag) {
            let (_, v) = self.pending.remove(pos).unwrap();
            cx.out_list(0, v);
        }
    }
}

// ---------------------------------------------------------------------------
// Random
// ---------------------------------------------------------------------------

/// Pd 0.39 `random` (its own LCG, seeded by `makeseed`).
struct Random {
    n: f32,
    state: u32,
}

impl Object for Random {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.n = m.as_float().unwrap_or(self.n);
            return;
        }
        if let Sel::Any(s) = &m.sel {
            if &**s == "seed" {
                self.state = m.args.first().map(|a| a.float() as i32 as u32).unwrap_or(0);
            }
            return;
        }
        let range = (self.n as i32).max(1);
        self.state = self.state.wrapping_mul(472_940_017).wrapping_add(832_416_023);
        let mut v = ((range as f64) * (self.state as f64) / 4_294_967_296.0) as i32;
        if v >= range {
            v = range - 1;
        }
        cx.out_f(0, v as f32);
    }
}

/// EAPd `frandom n`: a float in [0, n).
struct FRandom {
    n: f32,
    state: u32,
}

impl Object for FRandom {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.n = m.as_float().unwrap_or(self.n);
            return;
        }
        if let Sel::Any(s) = &m.sel {
            if &**s == "seed" {
                self.state = m.args.first().map(|a| a.float() as i32 as u32).unwrap_or(0);
            }
            return;
        }
        self.state = self.state.wrapping_mul(472_940_017).wrapping_add(832_416_023);
        cx.out_f(0, (self.n as f64 * self.state as f64 / 4_294_967_296.0) as f32);
    }
}

// ---------------------------------------------------------------------------
// Misc vanilla
// ---------------------------------------------------------------------------

struct Print {
    name: String,
}

impl Object for Print {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let body = match m.sel {
            Sel::Float | Sel::List => m.args.iter().map(|a| a.text()).collect::<Vec<_>>().join(" "),
            _ => m.text(),
        };
        cx.print(format!("{}: {}", self.name, body));
    }
}

struct Loadbang;
impl Object for Loadbang {
    fn recv(&mut self, _: usize, _: &Msg, _: &mut Cx) {}
    fn loadbang(&mut self, cx: &mut Cx) {
        cx.out_bang(0);
    }
}

fn gui_out(send: &Option<Sym>, m: &Msg, cx: &mut Cx) {
    cx.out(0, m);
    if let Some(s) = send {
        let s = s.clone();
        cx.send(&s, m);
    }
}

struct Bng {
    init: bool,
    send: Option<Sym>,
}

impl Object for Bng {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if matches!(&m.sel, Sel::Any(s) if &**s == "set") {
            return;
        }
        gui_out(&self.send, &Msg::bang(), cx);
    }
    fn loadbang(&mut self, cx: &mut Cx) {
        if self.init {
            gui_out(&self.send, &Msg::bang(), cx);
        }
    }
}

struct Tgl {
    on: f32,
    nonzero: f32,
    init: bool,
    send: Option<Sym>,
}

impl Object for Tgl {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match &m.sel {
            Sel::Bang => self.on = if self.on != 0.0 { 0.0 } else { self.nonzero },
            Sel::Any(s) if &**s == "set" => {
                self.on = m.args.first().map(|a| a.float()).unwrap_or(0.0);
                return;
            }
            Sel::Any(s) if &**s == "nonzero" => {
                let v = m.args.first().map(|a| a.float()).unwrap_or(1.0);
                if v != 0.0 {
                    self.nonzero = v;
                }
                return;
            }
            _ => match m.as_float() {
                Some(f) => {
                    self.on = f;
                    if f != 0.0 {
                        self.nonzero = f;
                    }
                }
                None => return,
            },
        }
        gui_out(&self.send, &Msg::float(self.on), cx);
    }
    fn loadbang(&mut self, cx: &mut Cx) {
        if self.init {
            gui_out(&self.send, &Msg::float(self.on), cx);
        }
    }
}

struct Slider {
    lo: f32,
    hi: f32,
    value: f32,
    init: bool,
    send: Option<Sym>,
}

impl Slider {
    fn clip(&self, f: f32) -> f32 {
        let (a, b) = if self.lo <= self.hi { (self.lo, self.hi) } else { (self.hi, self.lo) };
        f.clamp(a, b)
    }
}

impl Object for Slider {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match &m.sel {
            Sel::Bang => {}
            Sel::Any(s) if &**s == "set" => {
                self.value = self.clip(m.args.first().map(|a| a.float()).unwrap_or(0.0));
                return;
            }
            Sel::Any(s) if &**s == "range" => {
                self.lo = f_arg(&m.args, 0, self.lo);
                self.hi = f_arg(&m.args, 1, self.hi);
                self.value = self.clip(self.value);
                return;
            }
            _ => match m.as_float() {
                Some(f) => self.value = self.clip(f),
                None => return,
            },
        }
        gui_out(&self.send, &Msg::float(self.value), cx);
    }
    fn loadbang(&mut self, cx: &mut Cx) {
        if self.init {
            gui_out(&self.send, &Msg::float(self.value), cx);
        }
    }
}

struct TabRead {
    name: Sym,
}

impl Object for TabRead {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if let Sel::Any(s) = &m.sel {
            if &**s == "set" {
                if let Some(Atom::S(n)) = m.args.first() {
                    self.name = n.clone();
                }
            }
            return;
        }
        let Some(f) = m.as_float() else { return };
        let name = self.name.clone();
        let v = match cx.table(&name) {
            Some(t) if !t.is_empty() => t[(f as i64).clamp(0, t.len() as i64 - 1) as usize],
            Some(_) => 0.0,
            None => {
                cx.error(format!("tabread: {name}: no such array"));
                return;
            }
        };
        cx.out_f(0, v);
    }
}

struct TabWrite {
    name: Sym,
    index: f32,
}

impl Object for TabWrite {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx) {
        if inlet == 1 {
            self.index = m.as_float().unwrap_or(self.index);
            return;
        }
        if let Sel::Any(s) = &m.sel {
            if &**s == "set" {
                if let Some(Atom::S(n)) = m.args.first() {
                    self.name = n.clone();
                }
            }
            return;
        }
        if m.sel == Sel::List && m.args.len() > 1 {
            let a = m.args.clone();
            cx.distribute(self, &a);
            return;
        }
        let Some(f) = m.as_float() else { return };
        let name = self.name.clone();
        let index = self.index;
        match cx.table(&name) {
            Some(t) if !t.is_empty() => {
                let n = (index as i64).clamp(0, t.len() as i64 - 1) as usize;
                t[n] = f;
            }
            Some(_) => {}
            None => cx.error(format!("tabwrite: {name}: no such array")),
        }
    }
}

// ---------------------------------------------------------------------------
// coll
// ---------------------------------------------------------------------------

fn coll_entry_msg(v: &[Atom]) -> Msg {
    match v {
        [Atom::S(s)] => Msg::symbol(s.clone()),
        _ => Msg::from_atoms(v.to_vec()),
    }
}

struct Coll {
    name: Sym,
}

impl Object for Coll {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let name = self.name.clone();
        match &m.sel {
            Sel::Float => {
                let key = Atom::F(m.args[0].float());
                let entry = cx.coll(&name).get(&key).cloned();
                if let Some(v) = entry {
                    cx.out(0, &coll_entry_msg(&v));
                }
            }
            Sel::Symbol => {
                if let Some(Atom::S(s)) = m.args.first() {
                    let entry = cx.coll(&name).get(&Atom::S(s.clone())).cloned();
                    if let Some(v) = entry {
                        cx.out(0, &coll_entry_msg(&v));
                    }
                }
            }
            Sel::List => {
                if m.args.len() >= 2 {
                    let key = m.args[0].clone();
                    cx.coll(&name).set(key, m.args[1..].to_vec());
                } else if let Some(a) = m.args.first() {
                    let entry = cx.coll(&name).get(a).cloned();
                    if let Some(v) = entry {
                        cx.out(0, &coll_entry_msg(&v));
                    }
                }
            }
            Sel::Bang => {}
            Sel::Any(s) => match &**s {
                "clear" => cx.coll(&name).entries.clear(),
                "delete" => {
                    if let Some(k) = m.args.first().cloned() {
                        let c = cx.coll(&name);
                        c.entries.retain(|(e, _)| !matches!((e, &k), (Atom::F(a), Atom::F(b)) if (*a as i64) == (*b as i64)) && !matches!((e, &k), (Atom::S(a), Atom::S(b)) if a == b));
                    }
                }
                "length" => {
                    let n = cx.coll(&name).entries.len();
                    cx.out_f(0, n as f32);
                }
                "nth" => {
                    if let (Some(k), Some(n)) = (m.args.first().cloned(), m.args.get(1)) {
                        let idx = n.float() as i64;
                        let v = cx.coll(&name).get(&k).and_then(|v| v.get((idx - 1).max(0) as usize).cloned());
                        if let Some(a) = v {
                            cx.out(0, &atom_msg(&a));
                        }
                    }
                }
                "store" | "insert" => {
                    if m.args.len() >= 2 {
                        let key = m.args[0].clone();
                        cx.coll(&name).set(key, m.args[1..].to_vec());
                    }
                }
                "dump" => {
                    let entries = cx.coll(&name).entries.clone();
                    for (k, v) in entries {
                        cx.out(1, &atom_msg(&k));
                        cx.out(0, &coll_entry_msg(&v));
                    }
                    cx.out_bang(3);
                }
                _ => {}
            },
        }
    }
}

/// `collread <coll>`: `length k` and `nth k n` (1-based) of a named coll.
struct CollRead {
    name: Sym,
}

impl Object for CollRead {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let name = self.name.clone();
        let Some(c) = cx.coll_ref(&name).cloned() else {
            cx.error(format!("collread: {name}: no such coll"));
            return;
        };
        match m.selector() {
            "length" => {
                let n = match m.args.first() {
                    Some(k) => c.get(k).map_or(0, |v| v.len()),
                    None => c.entries.len(),
                };
                cx.out_f(0, n as f32);
            }
            "nth" => {
                if let (Some(k), Some(n)) = (m.args.first(), m.args.get(1)) {
                    if let Some(a) = c.get(k).and_then(|v| v.get((n.float() as i64 - 1).max(0) as usize)) {
                        cx.out(0, &atom_msg(a));
                    }
                }
            }
            "set" => {
                if let Some(Atom::S(n)) = m.args.first() {
                    self.name = n.clone();
                }
            }
            _ => {
                if let Some(f) = m.as_float() {
                    if let Some(v) = c.get(&Atom::F(f)) {
                        cx.out(0, &coll_entry_msg(v));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EA sound classes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Random,
    InOrder,
    NoRepeat,
}

/// `play <sound|group> [polyphony] [steal|block] [setplaying|setnext]
/// [inorder|random|norepeat]` and `dplay <label> [polyphony] ...` (sound
/// chosen by `use`). RETAIL 0xA542F0 (args), 0xA54580 / 0xA54150 (start and
/// group pick), 0xA53EC0 (parameters).
pub struct Play {
    sound: Option<Sym>,
    polyphony: usize,
    steal: bool,
    setnext: bool,
    pick: Pick,
    last: Option<usize>,
    params: Vec<(Sym, Atom)>,
    voices: VecDeque<VoiceId>,
    /// Stopped voices still fading out.
    releasing: Vec<VoiceId>,
    /// A start armed the end callback (0xA54580 registers 0xA53A30).
    watching: bool,
    paused: bool,
}

impl Play {
    fn new(dynamic: bool, args: &[Atom]) -> Self {
        // RETAIL 0xA542F0: a symbol first argument is the sound of `play`
        // and `dplay` alike (`dplay hh 2` starts group `hh` until a `use`);
        // only `play` requires it.
        let first = match args.first() {
            Some(Atom::S(s)) => Some(s.clone()),
            Some(a) if !dynamic => Some(sym(&a.text())),
            _ => None,
        };
        let mut p = Play {
            sound: first,
            polyphony: 1,
            steal: true,
            setnext: false,
            pick: Pick::Random,
            last: None,
            params: vec![],
            voices: VecDeque::new(),
            releasing: vec![],
            watching: false,
            paused: false,
        };
        let mut got_poly = false;
        let skip = usize::from(p.sound.is_some());
        for a in args.iter().skip(skip) {
            match a {
                Atom::F(f) if !got_poly => {
                    p.polyphony = (*f).max(0.0) as usize;
                    got_poly = true;
                }
                Atom::F(_) => {}
                Atom::S(s) => match &**s {
                    "steal" => p.steal = true,
                    "block" => p.steal = false,
                    "setplaying" => p.setnext = false,
                    "setnext" => p.setnext = true,
                    "inorder" => p.pick = Pick::InOrder,
                    "random" => p.pick = Pick::Random,
                    "norepeat" => p.pick = Pick::NoRepeat,
                    _ => {}
                },
            }
        }
        p
    }

    fn choose(&mut self, select: Option<usize>, cx: &mut Cx) -> Option<Sym> {
        let sound = self.sound.clone()?;
        let Some(members) = cx.groups().get(&sound).cloned() else {
            return Some(sound);
        };
        let n = members.len();
        if n == 0 {
            return None;
        }
        let i = match select {
            Some(i) => i.min(n - 1),
            None => match self.pick {
                Pick::Random => cx.rand_int(n as u32) as usize,
                Pick::InOrder => self.last.map_or(0, |l| (l + 1) % n),
                Pick::NoRepeat => {
                    if n == 1 {
                        0
                    } else {
                        let r = cx.rand_int(n as u32 - 1) as usize;
                        if Some(r) == self.last {
                            n - 1
                        } else {
                            r
                        }
                    }
                }
            },
        };
        self.last = Some(i);
        Some(members[i].clone())
    }

    fn start(&mut self, select: Option<usize>, cx: &mut Cx) {
        let Some(sound) = self.choose(select, cx) else { return };
        if self.polyphony > 0 && self.voices.len() >= self.polyphony {
            if !self.steal {
                return;
            }
            while self.voices.len() >= self.polyphony {
                if let Some(old) = self.voices.pop_front() {
                    cx.host(HostCmd::Stop { voice: old });
                    self.releasing.push(old);
                }
            }
        }
        let voice = cx.new_voice();
        let inst = cx.instance();
        cx.host(HostCmd::Start { voice, inst, sound, params: self.params.clone() });
        if self.paused {
            cx.host(HostCmd::Pause { voice, paused: true });
        }
        self.voices.push_back(voice);
        self.watching = true;
    }

    fn stop(&mut self, cx: &mut Cx) {
        while let Some(v) = self.voices.pop_front() {
            cx.host(HostCmd::Stop { voice: v });
            self.releasing.push(v);
        }
    }

    fn set_paused(&mut self, paused: bool, cx: &mut Cx) {
        self.paused = paused;
        for v in &self.voices {
            cx.host(HostCmd::Pause { voice: *v, paused });
        }
    }
}

impl Object for Play {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if cx.eng.trace_play {
            let label = self.sound.as_deref().unwrap_or("(dplay)").to_string();
            cx.print(format!("play {label} <- {}", m.text()));
        }
        match &m.sel {
            // RETAIL (setup 0xA54630): no float or bang method; Pd's default
            // hands them to the anything method (0xA54020) as a parameter
            // named `float` / a bang without argument, which the host rejects
            // (the Tribe social patch sends `1` to its backing tracks: no-op).
            Sel::Bang | Sel::Float | Sel::List | Sel::Symbol => {
                cx.error(format!("play: no method for '{}'", m.selector()));
            }
            Sel::Any(s) => match &**s {
                "start" => self.start(None, cx),
                "startselect" => {
                    let i = m.args.first().map(|a| a.float().max(0.0) as usize).unwrap_or(0);
                    self.start(Some(i), cx);
                }
                "stop" => self.stop(cx),
                "pause" => {
                    let p = m.args.first().map(|a| a.float() != 0.0).unwrap_or(true);
                    self.set_paused(p, cx);
                }
                "use" => {
                    if let Some(a) = m.args.first() {
                        self.sound = Some(sym(&a.text()));
                        self.last = None;
                    }
                }
                name => {
                    let Some(value) = m.args.first().cloned() else {
                        cx.error(format!("play: {name}: missing value"));
                        return;
                    };
                    let name = s.clone();
                    match self.params.iter_mut().find(|(k, _)| *k == name) {
                        Some(e) => e.1 = value.clone(),
                        None => self.params.push((name.clone(), value.clone())),
                    }
                    if !self.setnext {
                        for v in &self.voices {
                            cx.host(HostCmd::Param { voice: *v, name: name.clone(), value: value.clone() });
                        }
                    }
                }
            },
            _ => {}
        }
    }
    /// RETAIL 0xA53A30: once every voice of the object has finished, the
    /// outlet bangs (through a clock: next scheduler tick).
    fn voice_ended(&mut self, voice: VoiceId, cx: &mut Cx) {
        self.voices.retain(|v| *v != voice);
        self.releasing.retain(|v| *v != voice);
        if self.watching && self.voices.is_empty() && self.releasing.is_empty() {
            self.watching = false;
            cx.delay(1, 0.0);
        }
    }
    fn clock(&mut self, _: u32, cx: &mut Cx) {
        cx.out_bang(0);
    }
    fn pause(&mut self, paused: bool, cx: &mut Cx) {
        if !self.voices.is_empty() {
            for v in &self.voices {
                cx.host(HostCmd::Pause { voice: *v, paused: paused || self.paused });
            }
        }
    }
}

struct GroupSize {
    name: Option<Sym>,
}

impl Object for GroupSize {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if m.sel != Sel::Bang {
            match m.as_symbol() {
                Some(s) if &*s == "set" => {
                    self.name = m.args.first().map(|a| sym(&a.text()));
                    return;
                }
                Some(s) => self.name = Some(s),
                None => return,
            }
        }
        let n = self.name.as_ref().and_then(|n| cx.groups().get(n)).map_or(0, |g| g.len());
        cx.out_f(0, n as f32);
    }
}

/// `mixer <group>`: bus parameters of a mixer group (gain, hipass, ring
/// modulation, delay).
struct Mixer {
    group: Sym,
}

impl Object for Mixer {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if let Sel::Any(s) = &m.sel {
            if matches!(&**s, "start" | "stop" | "use") {
                return;
            }
            if let Some(v) = m.args.first().cloned() {
                let inst = cx.instance();
                cx.host(HostCmd::Mixer { inst, group: self.group.clone(), name: s.clone(), value: v });
            }
        }
    }
}

/// The input family: `gfparam`/`fparam`/`gsparam`/`sparam` output what the
/// game sets, `gevent`/`event` bang. A bang asks for the current value.
struct Input {
    name: String,
    global: bool,
    event: bool,
}

impl Object for Input {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if m.sel == Sel::Bang && !self.event {
            let v = if self.global { cx.global_value(&self.name) } else { cx.instance_value(&self.name) };
            if let Some(v) = v {
                cx.out_f(0, v);
            }
        }
    }
    fn external(&mut self, m: &Msg, cx: &mut Cx) {
        cx.out(0, m);
    }
}

struct SendGame {
    name: Sym,
}

impl Object for SendGame {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        let value = match m.sel {
            Sel::Bang => 0.0,
            _ => match m.as_float() {
                Some(f) => f,
                None => return,
            },
        };
        let inst = cx.instance();
        cx.host(HostCmd::SendGame { inst, name: self.name.clone(), value });
    }
}

/// `thisinstance`: the instance id on bang; `busy` is a host flag (ignored).
struct ThisInstance;
impl Object for ThisInstance {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if m.sel == Sel::Bang {
            let i = cx.instance();
            cx.out_f(0, i as f32);
        }
    }
}

/// EA `function`: a breakpoint curve (see the module notes for the format).
pub struct Function {
    xmin: f32,
    xmax: f32,
    ymin: f32,
    ymax: f32,
    pts: Vec<(f32, f32)>,
    last: f32,
}

impl Function {
    pub fn new(args: &[Atom]) -> Self {
        let f = |i: usize, d: f32| f_arg(args, i, d);
        let mut pts = vec![(0.0, f(10, 0.0))];
        let mut i = 11;
        while i + 1 < args.len() {
            pts.push((f(i, 1.0), f(i + 1, 0.0)));
            i += 2;
        }
        Function { xmax: f(6, 1.0), ymin: f(7, 0.0), ymax: f(8, 1.0), xmin: f(9, 0.0), pts, last: 0.0 }
    }

    pub fn eval(&self, x: f32) -> f32 {
        let span = self.xmax - self.xmin;
        let xn = if span.abs() > f32::EPSILON { ((x - self.xmin) / span).clamp(0.0, 1.0) } else { 0.0 };
        let mut yn = self.pts.last().map_or(0.0, |p| p.1);
        let (mut px, mut py) = self.pts[0];
        if xn <= px {
            yn = py;
        } else {
            for &(qx, qy) in &self.pts[1..] {
                if xn <= qx {
                    let t = if qx > px { (xn - px) / (qx - px) } else { 1.0 };
                    yn = py + (qy - py) * t;
                    break;
                }
                px = qx;
                py = qy;
            }
        }
        self.ymin + yn * (self.ymax - self.ymin)
    }
}

impl Object for Function {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match m.selector() {
            "inrange" => {
                self.xmin = f_arg(&m.args, 0, self.xmin);
                self.xmax = f_arg(&m.args, 1, self.xmax);
            }
            "outrange" => {
                self.ymin = f_arg(&m.args, 0, self.ymin);
                self.ymax = f_arg(&m.args, 1, self.ymax);
            }
            "bang" => {
                let v = self.eval(self.last);
                cx.out_f(0, v);
            }
            _ => {
                if let Some(x) = m.as_float() {
                    self.last = x;
                    cx.out_f(0, self.eval(x));
                }
            }
        }
    }
}

/// EA `randweighted n`: a weighted random index (0-based) on bang; weights
/// from `fromlist w...` (all 1 until set).
struct RandWeighted {
    w: Vec<f32>,
}

impl Object for RandWeighted {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        match m.selector() {
            "bang" => {
                let total: f32 = self.w.iter().map(|w| w.max(0.0)).sum();
                let n = self.w.len();
                let pick = if total <= 0.0 {
                    cx.rand_int(n as u32) as usize
                } else {
                    let mut r = cx.rand_unit() as f32 * total;
                    let mut k = n - 1;
                    for (i, w) in self.w.iter().enumerate() {
                        let w = w.max(0.0);
                        if r < w {
                            k = i;
                            break;
                        }
                        r -= w;
                    }
                    k
                };
                cx.out_f(0, pick as f32);
            }
            "fromlist" => {
                for (i, a) in m.args.iter().enumerate() {
                    if i >= self.w.len() {
                        self.w.push(a.float());
                    } else {
                        self.w[i] = a.float();
                    }
                }
            }
            "tolist" => cx.out(0, &Msg::list(self.w.iter().map(|w| Atom::F(*w)).collect())),
            "resize" => {
                let n = f_arg(&m.args, 0, 1.0).max(1.0) as usize;
                self.w.resize(n, 0.0);
            }
            "const" => {
                let v = f_arg(&m.args, 0, 0.0);
                self.w.iter_mut().for_each(|w| *w = v);
            }
            "normalize" => {
                let total: f32 = self.w.iter().sum();
                if total > 0.0 {
                    self.w.iter_mut().for_each(|w| *w /= total);
                }
            }
            _ => {}
        }
    }
}


// ---------------------------------------------------------------------------
// trixie (track lists of eapdPixie files)
// ---------------------------------------------------------------------------

/// One `<track name=...>`: its `<shuffler .../>` and `<rhythm .../>`
/// entries, each the list of its XML attributes in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrixieTrack {
    pub name: String,
    pub shufflers: Vec<Vec<(String, Atom)>>,
    pub rhythms: Vec<Vec<(String, Atom)>>,
}

fn xml_attrs(tag: &str) -> Vec<(String, Atom)> {
    let mut out = vec![];
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().rsplit(|c: char| c.is_whitespace()).next().unwrap_or("").to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let body = &after[1..];
        let Some(end) = body.find(q) else { break };
        let value = &body[..end];
        let atom = value.trim().parse::<f32>().map(Atom::F).unwrap_or_else(|_| Atom::S(sym(value)));
        if !key.is_empty() {
            out.push((key, atom));
        }
        rest = &body[end + 1..];
    }
    out
}

/// Parse an eapdPixie track list (`tracks`, `tracks_cityplanner`).
pub fn parse_tracks(xml: &str) -> Vec<TrixieTrack> {
    let mut tracks: Vec<TrixieTrack> = vec![];
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else { break };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let tag = tag.trim_end_matches('/').trim();
        let (kind, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        match kind {
            "track" => {
                let a = xml_attrs(attrs);
                let name = a.iter().find(|(k, _)| k == "name").map(|(_, v)| v.text()).unwrap_or_default();
                tracks.push(TrixieTrack { name, ..Default::default() });
            }
            "shuffler" | "rhythm" => {
                if let Some(t) = tracks.last_mut() {
                    let a = xml_attrs(attrs);
                    if kind == "shuffler" {
                        t.shufflers.push(a);
                    } else {
                        t.rhythms.push(a);
                    }
                }
            }
            _ => {}
        }
    }
    tracks
}

/// `trixie <file>`: `read <track> shuffler|rhythm` outputs the number of
/// entries; `read <track> shuffler|rhythm <i | name>` outputs each attribute
/// of entry `i` (0-based) as `list <attribute> <value>` on the left outlet, then
/// a bang on the right one (VERIFIED 0xA3F040: track lookup 0xA3F000, entry
/// counter from 0, attributes sent as atom lists, `outlet_bang` on outlet 1).
/// INFERRED: a symbol picks the entry whose `name` (or `soundset`) matches.
struct Trixie {
    tracks: Vec<TrixieTrack>,
}

impl Object for Trixie {
    fn recv(&mut self, _: usize, m: &Msg, cx: &mut Cx) {
        if m.selector() != "read" || m.args.len() < 2 {
            return;
        }
        let track = m.args[0].text();
        if std::env::var_os("OPENSPORE_PD_TRIXIE").is_some() {
            cx.print(format!("trixie <- read {:?}", m.args.iter().map(|a| a.text()).collect::<Vec<_>>()));
        }
        let Some(t) = self.tracks.iter().find(|t| t.name == track) else {
            cx.error(format!("trixie: no track {track}"));
            return;
        };
        let list = match m.args[1].text().as_str() {
            "shuffler" => &t.shufflers,
            "rhythm" => &t.rhythms,
            _ => return,
        };
        let Some(which) = m.args.get(2) else {
            cx.out_f(0, list.len() as f32);
            return;
        };
        let entry = match which {
            Atom::F(i) => list.get(i.max(0.0) as usize),
            Atom::S(name) => list
                .iter()
                .find(|e| e.iter().any(|(k, v)| (k == "name" || k == "soundset") && v.text() == **name)),
        };
        let Some(entry) = entry.cloned() else { return };
        for (k, v) in entry {
            // `list <attribute> <value>` (0xA49E10 with the `list` selector:
            // `music_planet` takes them through `route list`).
            cx.out(0, &Msg::list(vec![Atom::S(sym(&k)), v]));
        }
        cx.out_bang(1);
    }
}
