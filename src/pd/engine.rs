//! The EAPd runtime: object arena, message passing, scheduler, instances.
//!
//! RETAIL behaviour followed here (`notes\pd-music-re-notes.md` 2.5, VERIFIED
//! addresses in the march2017 image):
//! * Logical time is in seconds (double). `clock_delay(x, ms)` sets the clock
//!   at `logical_now + ms * 0.001` (0xA491B0); clocks live in a time-sorted
//!   list (0xA490C0), equal times fire in the order they were set.
//! * Once per audio update the system sets the time and runs `sched_tick`
//!   (0xA492B0): `step = t - last` (at most 0.5 s), every clock with a time
//!   before `t + step` fires in time order, and while one fires the logical
//!   time is its own scheduled time (chained delays stay exact).
//! * Clocks pause and resume (0xA49140 keeps the remaining time, 0xA49170
//!   re-arms at now + remaining); pausing an instance pauses its clocks, its
//!   input objects and its voices (the mix modes' `pausegroups`).
//! * INFERRED (Pd 0.3x vanilla): depth-first message passing, an outlet's
//!   connections fire in the order they were made (file order), subpatch
//!   inlets/outlets are ordered by their x position, `$0` is a per-abstraction
//!   counter, loadbang order = abstractions, then subpatches, then the
//!   canvas's own objects.
//!
//! Rust: an object is taken out of its slot while one of its methods runs; a
//! message that reaches a busy object (a feedback loop such as `until`'s stop
//! inlet) waits in that slot's mailbox and is delivered when the method
//! returns, or read by the object itself (`until`).

use super::objects;
use super::parse::{BoxKind, Canvas, Tok};
use super::{fmt_g, fnv, sym, Atom, Msg, Sel, Sym};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

pub type InstId = u32;
pub type VoiceId = u32;

/// What the patches ask of the host audio engine (and the game).
#[derive(Clone, Debug, PartialEq)]
pub enum HostCmd {
    /// Start a voice of `sound` (a soundProp, SNR sample or instrument name)
    /// with the parameters the play object holds.
    Start { voice: VoiceId, inst: InstId, sound: Sym, params: Vec<(Sym, Atom)> },
    /// Stop a voice (it fades out over its `fadeout`).
    Stop { voice: VoiceId },
    /// A voice parameter (`gain`, `pitch`, `lowpass`, `seektime`, ...).
    Param { voice: VoiceId, name: Sym, value: Atom },
    Pause { voice: VoiceId, paused: bool },
    /// A mixer group (bus) parameter (`mixer <group>`).
    Mixer { inst: InstId, group: Sym, name: Sym, value: Atom },
    /// `sendgame <name>`: a value for the game.
    SendGame { inst: InstId, name: Sym, value: f32 },
    /// `print` output and engine errors.
    Print { inst: InstId, text: String },
}

/// A native object.
pub trait Object: Send + Sync {
    fn recv(&mut self, inlet: usize, m: &Msg, cx: &mut Cx);
    /// Messages from outside the patch graph: `receive` buses, game
    /// parameters and events.
    fn external(&mut self, m: &Msg, cx: &mut Cx) {
        let _ = (m, cx);
    }
    /// A clock this object set fired.
    fn clock(&mut self, tag: u32, cx: &mut Cx) {
        let _ = (tag, cx);
    }
    fn loadbang(&mut self, cx: &mut Cx) {
        let _ = cx;
    }
    /// The host finished a voice this object started.
    fn voice_ended(&mut self, voice: VoiceId, cx: &mut Cx) {
        let _ = (voice, cx);
    }
    /// The instance paused or resumed (input objects stop listening, play
    /// objects pause their voices).
    fn pause(&mut self, paused: bool, cx: &mut Cx) {
        let _ = (paused, cx);
    }
}

/// `coll` storage: entries by key (ints, or symbols) in insertion order.
#[derive(Clone, Debug, Default)]
pub struct Coll {
    pub entries: Vec<(Atom, Vec<Atom>)>,
}

impl Coll {
    pub fn get(&self, key: &Atom) -> Option<&Vec<Atom>> {
        self.entries.iter().find(|(k, _)| key_eq(k, key)).map(|(_, v)| v)
    }
    pub fn set(&mut self, key: Atom, value: Vec<Atom>) {
        if let Some(e) = self.entries.iter_mut().find(|(k, _)| key_eq(k, &key)) {
            e.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }
}

fn key_eq(a: &Atom, b: &Atom) -> bool {
    match (a, b) {
        (Atom::F(x), Atom::F(y)) => (*x as i64) == (*y as i64),
        (Atom::S(x), Atom::S(y)) => x == y,
        _ => false,
    }
}

struct Slot {
    obj: Option<Box<dyn Object>>,
    outs: Vec<Vec<(u32, u16)>>,
    inst: InstId,
    mail: VecDeque<(usize, Msg)>,
    alive: bool,
}

/// One running patch.
pub struct Instance {
    pub name: String,
    objects: Vec<usize>,
    pub paused: bool,
    voices: HashSet<VoiceId>,
    /// Instance parameters (`fparam`), by name hash.
    pub params: HashMap<u32, f32>,
    pub dollar_zero: u32,
}

/// Loadbang order tree (abstractions first, then subpatches, then objects).
enum Lb {
    Obj(usize),
    Canvas { abstraction: bool, items: Vec<Lb> },
}

/// Where a box's inlets / outlets are in the arena.
enum Node {
    None,
    Obj(usize),
    Canvas { inlets: Vec<usize>, outlets: Vec<usize> },
}

/// The engine: every instance's objects share one arena (global `send`
/// names, groups and colls cross instances as in the retail engine).
pub struct Engine {
    slots: Vec<Slot>,
    free: Vec<usize>,
    instances: Vec<Option<Instance>>,
    receivers: HashMap<Sym, Vec<(usize, bool)>>,
    gparams: HashMap<u32, Vec<usize>>,
    iparams: HashMap<(InstId, u32), Vec<usize>>,
    gevents: HashMap<u32, Vec<usize>>,
    ievents: HashMap<(InstId, u32), Vec<usize>>,
    /// Last value of every global parameter set (`gfparam` bang).
    pub gvalues: HashMap<u32, f32>,
    pub gsymbols: HashMap<u32, Sym>,
    /// `group <name> <members...>`.
    pub groups: HashMap<Sym, Vec<Sym>>,
    pub colls: HashMap<Sym, Coll>,
    pub tables: HashMap<Sym, Vec<f32>>,
    pub values: HashMap<Sym, f32>,
    clocks: BTreeMap<(u64, u64), (usize, u32)>,
    clock_index: HashMap<(usize, u32), (u64, u64)>,
    paused_clocks: HashMap<(usize, u32), f64>,
    seq: u64,
    /// Logical time (seconds).
    pub logical: f64,
    last_tick: Option<f64>,
    cmds: Vec<HostCmd>,
    voice_owner: HashMap<VoiceId, usize>,
    next_voice: VoiceId,
    dollar_zero: u32,
    /// Pd's seed generator (`makeseed`) and the engine-level generator used
    /// by `frandom`, `randweighted` and group picks.
    seed: u32,
    rng: u32,
    depth: u32,
    pub errors: Vec<String>,
    warned: HashSet<String>,
    /// Diagnostics: log every `send` whose bus name contains this text
    /// (`*` = all) as a `Print` command.
    pub trace_sends: Option<String>,
    /// Diagnostics: log every message a `play` / `dplay` receives.
    pub trace_play: bool,
    /// Installed `eapdPixie` data files by FNV of their name (`trixie
    /// tracks`, `pixie <file>` read them when created); set by the host.
    pub data_files: HashMap<u32, Arc<str>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(1_489_853_723)
    }
}

/// Lookup of abstraction sources by class name (the bundle's own entries,
/// then the installed `pd` records).
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<Arc<Canvas>>;

impl Engine {
    pub fn new(seed: u32) -> Self {
        Engine {
            slots: vec![],
            free: vec![],
            instances: vec![],
            receivers: HashMap::new(),
            gparams: HashMap::new(),
            iparams: HashMap::new(),
            gevents: HashMap::new(),
            ievents: HashMap::new(),
            gvalues: HashMap::new(),
            gsymbols: HashMap::new(),
            groups: HashMap::new(),
            colls: HashMap::new(),
            tables: HashMap::new(),
            values: HashMap::new(),
            clocks: BTreeMap::new(),
            clock_index: HashMap::new(),
            paused_clocks: HashMap::new(),
            seq: 0,
            logical: 0.0,
            last_tick: None,
            cmds: vec![],
            voice_owner: HashMap::new(),
            next_voice: 1,
            dollar_zero: 1000,
            seed: seed | 1,
            rng: seed.wrapping_mul(2_654_435_761) | 1,
            depth: 0,
            errors: vec![],
            warned: HashSet::new(),
            trace_sends: None,
            trace_play: false,
            data_files: HashMap::new(),
        }
    }

    // ------------------------------------------------------------------
    // Instances
    // ------------------------------------------------------------------

    /// Build a patch instance: create every object, connect, fire the
    /// loadbangs, then set the instance parameters (the soundProp's `gain`
    /// and `pitch` reach `fparam gain` / `fparam pitch`).
    pub fn create_instance(&mut self, name: &str, root: &Canvas, resolve: Resolver, params: &[(&str, f32)]) -> InstId {
        let inst = match self.instances.iter().position(|i| i.is_none()) {
            Some(i) => i as InstId,
            None => {
                self.instances.push(None);
                (self.instances.len() - 1) as InstId
            }
        };
        self.dollar_zero += 1;
        let dz = self.dollar_zero;
        self.instances[inst as usize] = Some(Instance {
            name: name.to_string(),
            objects: vec![],
            paused: false,
            voices: HashSet::new(),
            params: HashMap::new(),
            dollar_zero: dz,
        });
        let (_, lb) = self.build_canvas(inst, root, dz, &[], resolve, 0, true);
        let mut order = vec![];
        collect_loadbangs(&lb, &mut order);
        for id in order {
            self.with_obj(id, |o, cx| o.loadbang(cx));
        }
        for (k, v) in params {
            self.set_instance_param(inst, k, *v);
        }
        inst
    }

    /// Remove an instance: its voices stop (with their fadeout), its clocks
    /// and registrations go away.
    pub fn destroy_instance(&mut self, inst: InstId) {
        let Some(instance) = self.instances.get_mut(inst as usize).and_then(|i| i.take()) else {
            return;
        };
        let mut voices: Vec<_> = instance.voices.into_iter().collect();
        voices.sort_unstable();
        for v in voices {
            self.cmds.push(HostCmd::Stop { voice: v });
            self.voice_owner.remove(&v);
        }
        let ids: HashSet<usize> = instance.objects.iter().copied().collect();
        for id in &instance.objects {
            if let Some(s) = self.slots.get_mut(*id) {
                s.obj = None;
                s.outs.clear();
                s.mail.clear();
                s.alive = false;
            }
            self.free.push(*id);
        }
        let keep = |v: &mut Vec<usize>| v.retain(|i| !ids.contains(i));
        self.receivers.values_mut().for_each(|v| v.retain(|(i, _)| !ids.contains(i)));
        self.gparams.values_mut().for_each(keep);
        self.gevents.values_mut().for_each(keep);
        self.iparams.retain(|(i, _), _| *i != inst);
        self.ievents.retain(|(i, _), _| *i != inst);
        let dead: Vec<_> = self.clock_index.keys().filter(|(o, _)| ids.contains(o)).copied().collect();
        for k in dead {
            if let Some(key) = self.clock_index.remove(&k) {
                self.clocks.remove(&key);
            }
        }
        self.paused_clocks.retain(|(o, _), _| !ids.contains(o));
    }

    pub fn instance(&self, inst: InstId) -> Option<&Instance> {
        self.instances.get(inst as usize).and_then(|i| i.as_ref())
    }

    pub fn instance_ids(&self) -> Vec<InstId> {
        (0..self.instances.len() as InstId).filter(|i| self.instance(*i).is_some()).collect()
    }

    pub fn object_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }

    /// Pause / resume an instance: its clocks keep their remaining time, its
    /// input objects ignore the game while paused and its voices pause.
    pub fn pause_instance(&mut self, inst: InstId, paused: bool) {
        let Some(instance) = self.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) else {
            return;
        };
        if instance.paused == paused {
            return;
        }
        instance.paused = paused;
        let ids: Vec<usize> = instance.objects.clone();
        let idset: HashSet<usize> = ids.iter().copied().collect();
        if paused {
            let keys: Vec<_> = self.clock_index.keys().filter(|(o, _)| idset.contains(o)).copied().collect();
            for k in keys {
                if let Some(key) = self.clock_index.remove(&k) {
                    self.clocks.remove(&key);
                    let t = f64::from_bits(key.0);
                    self.paused_clocks.insert(k, (t - self.logical).max(0.0));
                }
            }
        } else {
            let keys: Vec<_> = self.paused_clocks.keys().filter(|(o, _)| idset.contains(o)).copied().collect();
            for k in keys {
                if let Some(rem) = self.paused_clocks.remove(&k) {
                    self.set_clock(k.0, k.1, self.logical + rem);
                }
            }
        }
        for id in ids {
            self.with_obj(id, |o, cx| o.pause(paused, cx));
        }
    }

    // ------------------------------------------------------------------
    // Game -> patch
    // ------------------------------------------------------------------

    /// `0x5CA970(id, value)`: every `gfparam` whose FNV-1(name) == id outputs
    /// the value (every set, not only changes: the patches filter with
    /// `[change]`).
    pub fn set_global_param_hash(&mut self, hash: u32, value: f32) {
        self.gvalues.insert(hash, value);
        let list = self.gparams.get(&hash).cloned().unwrap_or_default();
        let m = Msg::float(value);
        for id in list {
            if self.paused_obj(id) {
                continue;
            }
            self.with_obj(id, |o, cx| o.external(&m, cx));
        }
    }

    pub fn set_global_param(&mut self, name: &str, value: f32) {
        self.set_global_param_hash(fnv(name), value);
    }

    pub fn set_global_symbol(&mut self, name: &str, value: Sym) {
        let hash = fnv(name);
        self.gsymbols.insert(hash, value.clone());
        let list = self.gparams.get(&hash).cloned().unwrap_or_default();
        let m = Msg::symbol(value);
        for id in list {
            if !self.paused_obj(id) {
                self.with_obj(id, |o, cx| o.external(&m, cx));
            }
        }
    }

    /// `fparam <name>` of one instance.
    pub fn set_instance_param(&mut self, inst: InstId, name: &str, value: f32) {
        let hash = fnv(name);
        let Some(instance) = self.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) else {
            return;
        };
        instance.params.insert(hash, value);
        if instance.paused {
            return;
        }
        let list = self.iparams.get(&(inst, hash)).cloned().unwrap_or_default();
        let m = Msg::float(value);
        for id in list {
            self.with_obj(id, |o, cx| o.external(&m, cx));
        }
    }

    /// `gevent <name>` (bang).
    pub fn global_event(&mut self, name: &str) {
        let list = self.gevents.get(&fnv(name)).cloned().unwrap_or_default();
        for id in list {
            if !self.paused_obj(id) {
                self.with_obj(id, |o, cx| o.external(&Msg::bang(), cx));
            }
        }
    }

    /// `event <name>` of one instance.
    pub fn instance_event(&mut self, inst: InstId, name: &str) {
        let list = self.ievents.get(&(inst, fnv(name))).cloned().unwrap_or_default();
        for id in list {
            if !self.paused_obj(id) {
                self.with_obj(id, |o, cx| o.external(&Msg::bang(), cx));
            }
        }
    }

    /// Send to a `receive` bus from outside (tests, diagnostics).
    pub fn send(&mut self, name: &str, m: &Msg) {
        self.send_to_receivers(name, m);
    }

    /// The host finished a voice (one-shot ended).
    pub fn voice_ended(&mut self, voice: VoiceId) {
        let Some(owner) = self.voice_owner.remove(&voice) else { return };
        if let Some(inst) = self.slots.get(owner).map(|s| s.inst) {
            if let Some(i) = self.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) {
                i.voices.remove(&voice);
            }
        }
        self.with_obj(owner, |o, cx| o.voice_ended(voice, cx));
    }

    /// The instance owning a voice.
    pub fn voice_instance(&self, voice: VoiceId) -> Option<InstId> {
        self.voice_owner.get(&voice).and_then(|o| self.slots.get(*o)).map(|s| s.inst)
    }

    /// Host commands produced since the last call.
    pub fn take_commands(&mut self) -> Vec<HostCmd> {
        std::mem::take(&mut self.cmds)
    }

    // ------------------------------------------------------------------
    // Scheduler
    // ------------------------------------------------------------------

    /// One audio update at time `now` (seconds): fire every clock due before
    /// `now + step` (step = time since the last update, at most 0.5 s).
    pub fn tick(&mut self, now: f64) {
        let step = match self.last_tick {
            Some(last) => (now - last).clamp(0.0, 0.5),
            None => 0.0,
        };
        self.last_tick = Some(now);
        self.logical = self.logical.max(now);
        let limit = now + step;
        let mut fired = 0usize;
        loop {
            let Some((&key, &(obj, tag))) = self.clocks.iter().next() else { break };
            let t = f64::from_bits(key.0);
            if t >= limit {
                break;
            }
            self.clocks.remove(&key);
            self.clock_index.remove(&(obj, tag));
            self.logical = t;
            self.with_obj(obj, |o, cx| o.clock(tag, cx));
            fired += 1;
            if fired > 200_000 {
                self.error("scheduler: more than 200000 clocks in one tick (runaway metro?)".into());
                break;
            }
        }
        self.logical = now;
    }

    /// Fire everything up to `until` in steps of `dt` (tests, traces).
    pub fn run_for(&mut self, from: f64, until: f64, dt: f64) {
        let mut t = from;
        while t <= until + 1e-9 {
            self.tick(t);
            t += dt;
        }
    }

    fn set_clock(&mut self, obj: usize, tag: u32, time: f64) {
        if let Some(old) = self.clock_index.remove(&(obj, tag)) {
            self.clocks.remove(&old);
        }
        self.paused_clocks.remove(&(obj, tag));
        self.seq += 1;
        let key = (time.max(0.0).to_bits(), self.seq);
        self.clocks.insert(key, (obj, tag));
        self.clock_index.insert((obj, tag), key);
    }

    fn unset_clock(&mut self, obj: usize, tag: u32) {
        if let Some(old) = self.clock_index.remove(&(obj, tag)) {
            self.clocks.remove(&old);
        }
        self.paused_clocks.remove(&(obj, tag));
    }

    // ------------------------------------------------------------------
    // Dispatch
    // ------------------------------------------------------------------

    fn paused_obj(&self, id: usize) -> bool {
        self.slots
            .get(id)
            .and_then(|s| self.instances.get(s.inst as usize))
            .and_then(|i| i.as_ref())
            .is_some_and(|i| i.paused)
    }

    fn with_obj(&mut self, id: usize, f: impl FnOnce(&mut dyn Object, &mut Cx)) {
        let Some(slot) = self.slots.get_mut(id) else { return };
        let Some(mut obj) = slot.obj.take() else { return };
        self.depth += 1;
        {
            let mut cx = Cx { eng: self, me: id };
            f(obj.as_mut(), &mut cx);
        }
        self.depth -= 1;
        if let Some(slot) = self.slots.get_mut(id) {
            if slot.alive {
                slot.obj = Some(obj);
            }
        }
        self.drain_mail(id);
    }

    fn drain_mail(&mut self, id: usize) {
        while let Some((inlet, m)) = self.slots.get_mut(id).and_then(|s| s.mail.pop_front()) {
            if inlet == usize::MAX {
                self.with_obj(id, |o, cx| o.external(&m, cx));
            } else {
                self.deliver(id, inlet, &m);
            }
        }
    }

    fn deliver(&mut self, id: usize, inlet: usize, m: &Msg) {
        if self.depth > 1000 {
            self.error("stack overflow (message loop deeper than 1000)".into());
            return;
        }
        let Some(slot) = self.slots.get_mut(id) else { return };
        if !slot.alive {
            return;
        }
        if slot.obj.is_none() {
            // Busy: a feedback loop reached the running object.
            slot.mail.push_back((inlet, m.clone()));
            return;
        }
        self.with_obj(id, |o, cx| o.recv(inlet, m, cx));
    }

    fn send_to_receivers(&mut self, name: &str, m: &Msg) {
        if let Some(f) = &self.trace_sends {
            if f == "*" || name.contains(f.as_str()) {
                let n = self.receivers.get(name).map_or(0, |v| v.len());
                self.cmds.push(HostCmd::Print { inst: u32::MAX - 1, text: format!("send {name} ({n} receivers): {}", m.text()) });
            }
        }
        let Some(list) = self.receivers.get(name).cloned() else {
            return;
        };
        for (id, external) in list {
            if external {
                if self.slots.get(id).is_some_and(|s| s.obj.is_none() && s.alive) {
                    // A `receive` that is busy: deliver after it returns.
                    if let Some(s) = self.slots.get_mut(id) {
                        s.mail.push_back((usize::MAX, m.clone()));
                    }
                    continue;
                }
                self.with_obj(id, |o, cx| o.external(m, cx));
            } else {
                self.deliver(id, 0, m);
            }
        }
    }

    pub(crate) fn error(&mut self, e: String) {
        if self.warned.insert(e.clone()) {
            if self.errors.len() < 500 {
                self.errors.push(e.clone());
            }
            self.cmds.push(HostCmd::Print { inst: u32::MAX, text: format!("error: {e}") });
        }
    }

    /// Pd's `random` LCG over the engine generator: an integer in [0, n).
    pub fn rand_int(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.rng = self.rng.wrapping_mul(472_940_017).wrapping_add(832_416_023);
        ((n as f64) * (self.rng as f64) / 4_294_967_296.0) as u32
    }

    /// A float in [0, 1).
    pub fn rand_unit(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(472_940_017).wrapping_add(832_416_023);
        self.rng as f64 / 4_294_967_296.0
    }

    /// Pd's `makeseed` (a fresh seed per `random` object).
    pub fn make_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(435_898_247).wrapping_add(938_284_287);
        self.seed & 0x7fff_ffff
    }

    // ------------------------------------------------------------------
    // Building
    // ------------------------------------------------------------------

    fn alloc(&mut self, inst: InstId) -> usize {
        let id = match self.free.pop() {
            Some(id) => {
                self.slots[id] = Slot { obj: None, outs: vec![], inst, mail: VecDeque::new(), alive: true };
                id
            }
            None => {
                self.slots.push(Slot { obj: None, outs: vec![], inst, mail: VecDeque::new(), alive: true });
                self.slots.len() - 1
            }
        };
        if let Some(i) = self.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) {
            i.objects.push(id);
        }
        id
    }

    fn place(&mut self, id: usize, obj: Box<dyn Object>) {
        self.slots[id].obj = Some(obj);
    }

    #[allow(clippy::too_many_arguments)]
    fn build_canvas(
        &mut self,
        inst: InstId,
        canvas: &Canvas,
        dz: u32,
        args: &[Atom],
        resolve: Resolver,
        depth: u32,
        abstraction: bool,
    ) -> ((Vec<usize>, Vec<usize>), Lb) {
        // Arrays of graph-on-parent canvases are named tables.
        for a in &canvas.arrays {
            let name = realize(&a.name, dz, args).text();
            let mut v = vec![0.0f32; a.size];
            for (start, vals) in &a.values {
                for (k, x) in vals.iter().enumerate() {
                    if let Some(slot) = v.get_mut(start + k) {
                        *slot = *x;
                    }
                }
            }
            self.tables.insert(sym(&name), v);
        }
        let mut nodes: Vec<Node> = Vec::with_capacity(canvas.boxes.len());
        let mut items: Vec<Lb> = vec![];
        let mut inlets: Vec<(f32, usize, usize)> = vec![];
        let mut outlets: Vec<(f32, usize, usize)> = vec![];
        for (index, b) in canvas.boxes.iter().enumerate() {
            match &b.kind {
                BoxKind::Text => nodes.push(Node::None),
                BoxKind::Msg(toks) => {
                    let id = self.alloc(inst);
                    self.place(id, Box::new(objects::MsgBox::new(toks.clone(), dz)));
                    nodes.push(Node::Obj(id));
                    items.push(Lb::Obj(id));
                }
                BoxKind::FloatAtom { min, max, receive, send } => {
                    let id = self.alloc(inst);
                    let receive = receive.as_ref().map(|t| sym(&realize(t, dz, args).text()));
                    let send = send.as_ref().map(|t| sym(&realize(t, dz, args).text()));
                    if let Some(r) = &receive {
                        self.receivers.entry(r.clone()).or_default().push((id, false));
                    }
                    self.place(id, Box::new(objects::FloatAtom::new(*min, *max, send)));
                    nodes.push(Node::Obj(id));
                    items.push(Lb::Obj(id));
                }
                BoxKind::SymbolAtom { receive, send } => {
                    let id = self.alloc(inst);
                    let receive = receive.as_ref().map(|t| sym(&realize(t, dz, args).text()));
                    let send = send.as_ref().map(|t| sym(&realize(t, dz, args).text()));
                    if let Some(r) = &receive {
                        self.receivers.entry(r.clone()).or_default().push((id, false));
                    }
                    self.place(id, Box::new(objects::SymbolAtom::new(send)));
                    nodes.push(Node::Obj(id));
                    items.push(Lb::Obj(id));
                }
                BoxKind::Sub(sub) => {
                    let ((i, o), lb) = self.build_canvas(inst, sub, dz, args, resolve, depth + 1, false);
                    nodes.push(Node::Canvas { inlets: i, outlets: o });
                    items.push(lb);
                }
                BoxKind::Obj { class, args: oargs, data, array } => {
                    let atoms: Vec<Atom> = oargs.iter().map(|t| realize(t, dz, args)).collect();
                    let class_name = match class {
                        None => {
                            nodes.push(Node::None);
                            continue;
                        }
                        Some(Tok::F(f)) => {
                            // A number as class: a float box (`[5]`).
                            let id = self.alloc(inst);
                            let mut setup = Setup { eng: self, id, inst, dz };
                            let obj = objects::create("float", &[Atom::F(*f)], &[], &[], &mut setup);
                            if let Some(obj) = obj {
                                self.place(id, obj);
                            }
                            nodes.push(Node::Obj(id));
                            items.push(Lb::Obj(id));
                            continue;
                        }
                        Some(t) => realize(t, dz, args).text(),
                    };
                    if class_name == "inlet" || class_name == "outlet" {
                        let id = self.alloc(inst);
                        self.place(id, Box::new(objects::Thru));
                        if class_name == "inlet" {
                            inlets.push((b.x, index, id));
                        } else {
                            outlets.push((b.x, index, id));
                        }
                        nodes.push(Node::Obj(id));
                        continue;
                    }
                    if objects::is_native(&class_name) {
                        let id = self.alloc(inst);
                        let mut setup = Setup { eng: self, id, inst, dz };
                        match objects::create(&class_name, &atoms, data, array, &mut setup) {
                            Some(obj) => self.place(id, obj),
                            None => {
                                let e = format!("{class_name}: could not create");
                                self.error(e);
                            }
                        }
                        nodes.push(Node::Obj(id));
                        items.push(Lb::Obj(id));
                        continue;
                    }
                    // Abstraction.
                    let sub = if depth < 48 { resolve(&class_name) } else { None };
                    match sub {
                        Some(sub) => {
                            self.dollar_zero += 1;
                            let sub_dz = self.dollar_zero;
                            let ((i, o), lb) = self.build_canvas(inst, &sub, sub_dz, &atoms, resolve, depth + 1, true);
                            nodes.push(Node::Canvas { inlets: i, outlets: o });
                            items.push(lb);
                        }
                        None => {
                            // Not registered in the retail engine either
                            // (drum_interface, melplayer_city, ...): a dead box.
                            self.error(format!("{class_name} ... couldn't create"));
                            nodes.push(Node::None);
                        }
                    }
                }
            }
        }
        for &(from, outlet, to, inlet) in &canvas.connects {
            let src = match nodes.get(from) {
                Some(Node::Obj(id)) => Some((*id, outlet)),
                Some(Node::Canvas { outlets, .. }) => outlets.get(outlet).map(|id| (*id, 0)),
                _ => None,
            };
            let dst = match nodes.get(to) {
                Some(Node::Obj(id)) => Some((*id, inlet)),
                Some(Node::Canvas { inlets, .. }) => inlets.get(inlet).map(|id| (*id, 0)),
                _ => None,
            };
            if let (Some((s, so)), Some((d, di))) = (src, dst) {
                let outs = &mut self.slots[s].outs;
                if outs.len() <= so {
                    outs.resize(so + 1, vec![]);
                }
                outs[so].push((d as u32, di as u16));
            }
        }
        // Subpatch inlets/outlets in x order (ties: creation order).
        inlets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        outlets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        (
            (inlets.into_iter().map(|t| t.2).collect(), outlets.into_iter().map(|t| t.2).collect()),
            Lb::Canvas { abstraction, items },
        )
    }
}

fn collect_loadbangs(lb: &Lb, out: &mut Vec<usize>) {
    let Lb::Canvas { items, .. } = lb else {
        if let Lb::Obj(id) = lb {
            out.push(*id);
        }
        return;
    };
    // 1. abstractions (each completely), recursing into subpatches for
    //    their abstractions; 2. subpatches' objects; 3. own objects.
    fn abstractions(items: &[Lb], out: &mut Vec<usize>) {
        for it in items {
            if let Lb::Canvas { abstraction, items } = it {
                if *abstraction {
                    collect_loadbangs(it, out);
                } else {
                    abstractions(items, out);
                }
            }
        }
    }
    fn subpatches(items: &[Lb], out: &mut Vec<usize>) {
        for it in items {
            if let Lb::Canvas { abstraction: false, items } = it {
                subpatches(items, out);
            }
        }
        for it in items {
            if let Lb::Obj(id) = it {
                out.push(*id);
            }
        }
    }
    abstractions(items, out);
    subpatches(items, out);
}

/// `canvas_realizedollar`: creation arguments with `$0` / `$n` replaced.
pub fn realize(t: &Tok, dz: u32, args: &[Atom]) -> Atom {
    match t {
        Tok::F(f) => Atom::F(*f),
        Tok::S(s) => Atom::S(s.clone()),
        Tok::Dollar(0) => Atom::F(dz as f32),
        Tok::Dollar(n) => args.get(*n as usize - 1).cloned().unwrap_or(Atom::F(0.0)),
        Tok::DollSym(s) => Atom::S(sym(&realize_dollsym(s, dz, args))),
        Tok::Semi => Atom::S(sym(";")),
        Tok::Comma => Atom::S(sym(",")),
    }
}

/// `$n` inside a symbol (`$0-gain` -> `1003-gain`, `$1-spulse` -> `myclock-spulse`).
pub fn realize_dollsym(s: &str, dz: u32, args: &[Atom]) -> String {
    let mut out = String::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '$' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let n: usize = b[i + 1..j].iter().collect::<String>().parse().unwrap_or(0);
            if n == 0 {
                out.push_str(&dz.to_string());
            } else {
                match args.get(n - 1) {
                    Some(Atom::F(f)) => out.push_str(&fmt_g(*f)),
                    Some(Atom::S(s)) => out.push_str(s),
                    None => out.push('0'),
                }
            }
            i = j;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// What an object can register while it is created.
pub struct Setup<'a> {
    pub(crate) eng: &'a mut Engine,
    pub id: usize,
    pub inst: InstId,
    pub dz: u32,
}

impl Setup<'_> {
    /// A `receive` bus (`external` = the object's `external` method gets the
    /// messages; otherwise they arrive at inlet 0, as for GUI boxes).
    pub fn bind(&mut self, name: &Sym, external: bool) {
        self.eng.receivers.entry(name.clone()).or_default().push((self.id, external));
    }
    pub fn bind_global_param(&mut self, name: &str) {
        self.eng.gparams.entry(fnv(name)).or_default().push(self.id);
    }
    pub fn bind_instance_param(&mut self, name: &str) {
        self.eng.iparams.entry((self.inst, fnv(name))).or_default().push(self.id);
    }
    pub fn bind_global_event(&mut self, name: &str) {
        self.eng.gevents.entry(fnv(name)).or_default().push(self.id);
    }
    pub fn bind_instance_event(&mut self, name: &str) {
        self.eng.ievents.entry((self.inst, fnv(name))).or_default().push(self.id);
    }
    /// An installed `eapdPixie` data file by name.
    pub fn data_file(&self, name: &str) -> Option<Arc<str>> {
        self.eng.data_files.get(&fnv(name)).cloned()
    }
    pub fn define_group(&mut self, name: Sym, members: Vec<Sym>) {
        self.eng.groups.insert(name, members);
    }
    pub fn coll(&mut self, name: &Sym) -> &mut Coll {
        self.eng.colls.entry(name.clone()).or_default()
    }
    pub fn table(&mut self, name: &Sym, size: usize) {
        let t = self.eng.tables.entry(name.clone()).or_default();
        if t.len() < size {
            t.resize(size, 0.0);
        }
    }
    pub fn table_mut(&mut self, name: &Sym) -> Option<&mut Vec<f32>> {
        self.eng.tables.get_mut(name)
    }
    pub fn make_seed(&mut self) -> u32 {
        self.eng.make_seed()
    }
    pub fn error(&mut self, e: String) {
        self.eng.error(e);
    }
}

/// The context of a running method.
pub struct Cx<'a> {
    pub(crate) eng: &'a mut Engine,
    pub me: usize,
}

impl Cx<'_> {
    /// Send a message out of one of this object's outlets (depth first).
    pub fn out(&mut self, outlet: usize, m: &Msg) {
        let n = self.eng.slots[self.me].outs.get(outlet).map_or(0, |v| v.len());
        for k in 0..n {
            let Some(&(to, inlet)) = self.eng.slots[self.me].outs.get(outlet).and_then(|v| v.get(k)) else {
                break;
            };
            self.eng.deliver(to as usize, inlet as usize, m);
        }
    }
    pub fn out_f(&mut self, outlet: usize, f: f32) {
        self.out(outlet, &Msg::float(f));
    }
    pub fn out_bang(&mut self, outlet: usize) {
        self.out(outlet, &Msg::bang());
    }
    pub fn out_sym(&mut self, outlet: usize, s: Sym) {
        self.out(outlet, &Msg::symbol(s));
    }
    /// Pd `outlet_list`: an empty list is a bang, one float a float, one
    /// symbol a symbol.
    pub fn out_list(&mut self, outlet: usize, atoms: Vec<Atom>) {
        let m = match atoms.len() {
            0 => Msg::bang(),
            1 => match &atoms[0] {
                Atom::F(f) => Msg::float(*f),
                Atom::S(s) => Msg::symbol(s.clone()),
            },
            _ => Msg::list(atoms),
        };
        self.out(outlet, &m);
    }
    /// `send <name>`.
    pub fn send(&mut self, name: &str, m: &Msg) {
        self.eng.send_to_receivers(name, m);
    }
    /// Logical time in seconds.
    pub fn now(&self) -> f64 {
        self.eng.logical
    }
    /// Set (or move) this object's clock `tag` to fire `secs` from now.
    pub fn delay(&mut self, tag: u32, secs: f64) {
        let t = self.eng.logical + secs.max(0.0);
        self.eng.set_clock(self.me, tag, t);
    }
    pub fn unset(&mut self, tag: u32) {
        self.eng.unset_clock(self.me, tag);
    }
    pub fn clock_set(&self, tag: u32) -> bool {
        self.eng.clock_index.contains_key(&(self.me, tag)) || self.eng.paused_clocks.contains_key(&(self.me, tag))
    }
    pub fn instance(&self) -> InstId {
        self.eng.slots[self.me].inst
    }
    pub fn host(&mut self, cmd: HostCmd) {
        self.eng.cmds.push(cmd);
    }
    /// A new voice owned by this object.
    pub fn new_voice(&mut self) -> VoiceId {
        let v = self.eng.next_voice;
        self.eng.next_voice = self.eng.next_voice.wrapping_add(1).max(1);
        self.eng.voice_owner.insert(v, self.me);
        let inst = self.instance();
        if let Some(i) = self.eng.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) {
            i.voices.insert(v);
        }
        v
    }
    /// This object forgets a voice (stopped or stolen).
    pub fn drop_voice(&mut self, v: VoiceId) {
        self.eng.voice_owner.remove(&v);
        let inst = self.instance();
        if let Some(i) = self.eng.instances.get_mut(inst as usize).and_then(|i| i.as_mut()) {
            i.voices.remove(&v);
        }
    }
    pub fn paused(&self) -> bool {
        self.eng.paused_obj(self.me)
    }
    /// Messages that reached this object while its method runs.
    pub fn take_mail(&mut self) -> Vec<(usize, Msg)> {
        self.eng.slots[self.me].mail.drain(..).collect()
    }
    pub fn groups(&self) -> &HashMap<Sym, Vec<Sym>> {
        &self.eng.groups
    }
    pub fn coll(&mut self, name: &Sym) -> &mut Coll {
        self.eng.colls.entry(name.clone()).or_default()
    }
    pub fn coll_ref(&self, name: &Sym) -> Option<&Coll> {
        self.eng.colls.get(name)
    }
    pub fn table(&mut self, name: &Sym) -> Option<&mut Vec<f32>> {
        self.eng.tables.get_mut(name)
    }
    pub fn value(&mut self, name: &Sym) -> &mut f32 {
        self.eng.values.entry(name.clone()).or_insert(0.0)
    }
    pub fn global_value(&self, name: &str) -> Option<f32> {
        self.eng.gvalues.get(&fnv(name)).copied()
    }
    pub fn instance_value(&self, name: &str) -> Option<f32> {
        let inst = self.instance();
        self.eng.instance(inst).and_then(|i| i.params.get(&fnv(name)).copied())
    }
    pub fn rand_int(&mut self, n: u32) -> u32 {
        self.eng.rand_int(n)
    }
    pub fn rand_unit(&mut self) -> f64 {
        self.eng.rand_unit()
    }
    pub fn print(&mut self, text: String) {
        let inst = self.instance();
        self.eng.cmds.push(HostCmd::Print { inst, text });
    }
    pub fn error(&mut self, e: String) {
        self.eng.error(e);
    }
    /// Pd's `obj_list` for objects without a list method: the atoms go to
    /// the inlets right of the first, then the first atom to inlet 0.
    pub fn distribute(&mut self, obj: &mut dyn Object, atoms: &[Atom]) {
        for (k, a) in atoms.iter().enumerate().skip(1) {
            let m = match a {
                Atom::F(f) => Msg::float(*f),
                Atom::S(s) => Msg::symbol(s.clone()),
            };
            obj.recv(k, &m, self);
        }
        match atoms.first() {
            Some(Atom::F(f)) => obj.recv(0, &Msg::float(*f), self),
            Some(Atom::S(s)) => obj.recv(0, &Msg::symbol(s.clone()), self),
            None => obj.recv(0, &Msg::bang(), self),
        }
    }
    /// A selector that is not one of Pd's types (for `Any` handling).
    pub fn is_any(m: &Msg) -> bool {
        matches!(m.sel, Sel::Any(_))
    }
}
