//! Arithmetica scripts (`arth`, resource type 0x0472329B, group 0x024A4F5A).
//!
//! The game keeps small tuning programs as "Arithmetica" resources: a stack
//! machine over named float variables with `if / else / endif`, calls and a
//! few math functions (SporeModder-FX `ArithmeticaFile`, ModAPI
//! `App::cArithmeticaResource`). The planet cameras are scripts of this kind:
//! the code sets the camera's inputs as named variables (`direction`,
//! `distance`, `keyboard_scroll_x`, ...), runs the whole program and reads the
//! outputs back (`notes\civ-camera-notes.md`). The installed `civ_camera`,
//! `tribe_camera`, `civ_follow_camera` and the city planner cameras are all
//! files of this format.
//!
//! File layout (all little endian after the first two big-endian words):
//! `u32be words`, `"HTRA"`, version 1, 2, `words`, then `count` operations
//! `(line, f32 value, i32 string index or -1, u32 opcode)`, `count` functions
//! `(string index, first op)` and the string table (count twice, NUL strings).
//!
//! The interpreter follows the retail semantics documented in
//! SporeModder-FX; `if` skips to the matching `else` / `endif`, comparisons
//! push 1.0 or 0.0, a missing variable reads 0.0 and `set` creates it.

use std::collections::HashMap;

/// Resource type of an Arithmetica file (`arth`).
pub const TYPE_ARITHMETICA: u32 = 0x0472_329B;
/// Group of the installed camera scripts.
pub const GROUP_SCRIPTS: u32 = 0x024A_4F5A;

pub mod op {
    pub const ADD: u32 = 0;
    pub const SUB: u32 = 1;
    pub const MUL: u32 = 2;
    pub const DIV: u32 = 3;
    pub const POW: u32 = 4;
    pub const PUSH: u32 = 5;
    pub const PUSH_VARIABLE: u32 = 6;
    pub const POP_INTO_VARIABLE: u32 = 7;
    pub const POP_INTO_VARIABLE2: u32 = 8;
    pub const NOP: u32 = 9;
    pub const RANDOM: u32 = 10;
    pub const GREATER: u32 = 11;
    pub const LESS: u32 = 12;
    pub const GREATER_EQUAL: u32 = 13;
    pub const LESS_EQUAL: u32 = 14;
    pub const EQUAL: u32 = 15;
    pub const NOT_EQUAL: u32 = 16;
    pub const BEGIN_IF: u32 = 17;
    pub const ELSE: u32 = 18;
    pub const END_IF: u32 = 19;
    pub const AND: u32 = 20;
    pub const OR: u32 = 21;
    pub const GOTO: u32 = 22;
    pub const CALL: u32 = 23;
    pub const RETURN: u32 = 24;
    pub const FLOOR: u32 = 25;
    pub const CEIL: u32 = 26;
    pub const SIN: u32 = 27;
    pub const COS: u32 = 28;
    pub const TAN: u32 = 29;
    pub const ASIN: u32 = 30;
    pub const ACOS: u32 = 31;
    pub const ATAN: u32 = 32;
}

/// One decoded operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Operation {
    pub line: u32,
    pub value: f32,
    /// String index (variable name) or operation index (`goto` / `call`);
    /// -1 when unused.
    pub index: i32,
    pub code: u32,
}

/// A parsed script (immutable; shareable between evaluations).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub operations: Vec<Operation>,
    /// `(string index, first operation)` of every `def`.
    pub functions: Vec<(u32, u32)>,
    pub strings: Vec<String>,
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or("offset overflow")?;
    let chunk = bytes.get(*at..end).ok_or("truncated arithmetica file")?;
    *at = end;
    Ok(u32::from_le_bytes(chunk.try_into().unwrap()))
}

#[allow(dead_code)] // variables(): inspection helper
impl Program {
    /// Decode an `arth` resource.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 28 {
            return Err("arithmetica file too short".into());
        }
        let words = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        if bytes.len() - 4 != words * 4 {
            return Err("inconsistent dword count in HTRA file".into());
        }
        if u32::from_be_bytes(bytes[4..8].try_into().unwrap()) != 0x4854_5241 {
            return Err("wrong magic word in HTRA file".into());
        }
        let mut at = 8;
        if read_u32(bytes, &mut at)? != 1 {
            return Err("unsupported HTRA version".into());
        }
        if read_u32(bytes, &mut at)? != 2 {
            return Err("unexpected HTRA field 8".into());
        }
        if read_u32(bytes, &mut at)? as usize != words {
            return Err("unexpected HTRA field C".into());
        }
        let count = read_u32(bytes, &mut at)? as usize;
        if count > bytes.len() / 16 {
            return Err("operation count exceeds the file".into());
        }
        let mut operations = Vec::with_capacity(count);
        for _ in 0..count {
            let line = read_u32(bytes, &mut at)?;
            let value = f32::from_bits(read_u32(bytes, &mut at)?);
            let index = read_u32(bytes, &mut at)? as i32;
            let code = read_u32(bytes, &mut at)?;
            operations.push(Operation {
                line,
                value,
                index,
                code,
            });
        }
        let function_count = read_u32(bytes, &mut at)? as usize;
        if function_count > bytes.len() / 8 {
            return Err("function count exceeds the file".into());
        }
        let mut functions = Vec::with_capacity(function_count);
        for _ in 0..function_count {
            let name = read_u32(bytes, &mut at)?;
            let first = read_u32(bytes, &mut at)?;
            functions.push((name, first));
        }
        let string_count = read_u32(bytes, &mut at)? as usize;
        if read_u32(bytes, &mut at)? as usize != string_count {
            return Err("inconsistent string count".into());
        }
        let mut strings = Vec::with_capacity(string_count.min(4096));
        for _ in 0..string_count {
            let rest = bytes.get(at..).ok_or("truncated string table")?;
            let end = rest
                .iter()
                .position(|b| *b == 0)
                .ok_or("unterminated string")?;
            strings.push(String::from_utf8_lossy(&rest[..end]).into_owned());
            at += end + 1;
        }
        Ok(Self {
            operations,
            functions,
            strings,
        })
    }

    fn name(&self, index: i32) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.strings.get(i))
            .map(String::as_str)
    }

    /// The variable names the program assigns or reads (no number literals).
    pub fn variables(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for o in &self.operations {
            if matches!(
                o.code,
                op::PUSH_VARIABLE | op::POP_INTO_VARIABLE | op::POP_INTO_VARIABLE2
            ) {
                if let Some(n) = self.name(o.index) {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        out
    }
}

/// Epsilon of the retail `==` / `!=` (not decoded; ADAPTED: 1e-5).
const EQUAL_EPSILON: f32 = 1.0e-5;
/// Operation budget of one evaluation (the camera scripts are ~500 long).
const STEP_LIMIT: usize = 100_000;

/// A program with its named variable values: `set` the inputs, `compute_all`,
/// `get` the outputs. Values persist between evaluations (the camera keeps
/// its scratch variables there).
#[derive(Clone, Debug, Default)]
pub struct Arithmetica {
    program: Program,
    values: HashMap<String, f32>,
    random: u32,
}

#[allow(dead_code)] // program / compute_function: used by other cameras later
impl Arithmetica {
    pub fn new(program: Program) -> Self {
        Self {
            program,
            values: HashMap::new(),
            random: 0x9E37_79B9,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Program::parse(bytes).map(Self::new)
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Create or overwrite a variable.
    pub fn set(&mut self, name: &str, value: f32) {
        if let Some(slot) = self.values.get_mut(name) {
            *slot = value;
        } else {
            self.values.insert(name.to_owned(), value);
        }
    }

    /// A variable's value, 0.0 when the program never defined it.
    pub fn get(&self, name: &str) -> f32 {
        self.values.get(name).copied().unwrap_or(0.0)
    }

    fn next_random(&mut self) -> f32 {
        // xorshift32, deterministic.
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Run the whole program from its first operation (scripts without
    /// functions are one top-level block).
    pub fn compute_all(&mut self) -> Result<(), String> {
        self.run(0)
    }

    /// Run one `def` by name (the retail `ComputeFunction`).
    pub fn compute_function(&mut self, name: &str) -> Result<(), String> {
        let start = self
            .program
            .functions
            .iter()
            .find(|(n, _)| self.program.name(*n as i32) == Some(name))
            .map(|(_, first)| *first as usize)
            .ok_or_else(|| format!("no function {name}"))?;
        self.run(start)
    }

    /// Index of the `else` / `endif` that closes the `if` whose body starts
    /// at `from`, honouring nesting.
    fn skip_false(&self, from: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in from..self.program.operations.len() {
            match self.program.operations[i].code {
                op::BEGIN_IF => depth += 1,
                op::ELSE if depth == 0 => return Some(i),
                op::END_IF if depth == 0 => return Some(i),
                op::END_IF => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// Index of the `endif` that closes the current `else` branch point.
    fn skip_else(&self, from: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in from..self.program.operations.len() {
            match self.program.operations[i].code {
                op::BEGIN_IF => depth += 1,
                op::END_IF if depth == 0 => return Some(i),
                op::END_IF => depth -= 1,
                _ => {}
            }
        }
        None
    }

    fn run(&mut self, start: usize) -> Result<(), String> {
        let mut stack: Vec<f32> = Vec::with_capacity(16);
        let mut calls: Vec<usize> = Vec::new();
        let mut pc = start;
        let mut steps = 0usize;
        while pc < self.program.operations.len() {
            steps += 1;
            if steps > STEP_LIMIT {
                return Err("arithmetica step limit exceeded".into());
            }
            let o = self.program.operations[pc];
            let mut next = pc + 1;
            macro_rules! pop {
                () => {
                    stack
                        .pop()
                        .ok_or_else(|| format!("stack underflow at operation {pc} (op {})", o.code))?
                };
            }
            macro_rules! binary {
                ($f:expr) => {{
                    let b = pop!();
                    let a = pop!();
                    stack.push($f(a, b));
                }};
            }
            let flag = |on: bool| if on { 1.0 } else { 0.0 };
            match o.code {
                op::ADD => binary!(|a: f32, b: f32| a + b),
                op::SUB => binary!(|a: f32, b: f32| a - b),
                op::MUL => binary!(|a: f32, b: f32| a * b),
                op::DIV => binary!(|a: f32, b: f32| a / b),
                op::POW => binary!(|a: f32, b: f32| a.powf(b)),
                op::PUSH => stack.push(o.value),
                op::PUSH_VARIABLE => {
                    let name = self
                        .program
                        .name(o.index)
                        .ok_or_else(|| format!("bad variable index at operation {pc}"))?;
                    stack.push(self.values.get(name).copied().unwrap_or(0.0));
                }
                op::POP_INTO_VARIABLE | op::POP_INTO_VARIABLE2 => {
                    let value = pop!();
                    let name = self
                        .program
                        .name(o.index)
                        .ok_or_else(|| format!("bad variable index at operation {pc}"))?
                        .to_owned();
                    self.values.insert(name, value);
                }
                op::NOP => {}
                op::RANDOM => {
                    let r = self.next_random();
                    stack.push(r);
                }
                op::GREATER => binary!(|a: f32, b: f32| flag(a > b)),
                op::LESS => binary!(|a: f32, b: f32| flag(a < b)),
                op::GREATER_EQUAL => binary!(|a: f32, b: f32| flag(a >= b)),
                op::LESS_EQUAL => binary!(|a: f32, b: f32| flag(a <= b)),
                op::EQUAL => binary!(|a: f32, b: f32| flag((a - b).abs() <= EQUAL_EPSILON)),
                op::NOT_EQUAL => binary!(|a: f32, b: f32| flag((a - b).abs() > EQUAL_EPSILON)),
                op::AND => binary!(|a: f32, b: f32| flag(a > 0.0 && b > 0.0)),
                op::OR => binary!(|a: f32, b: f32| flag(a > 0.0 || b > 0.0)),
                op::BEGIN_IF => {
                    let condition = pop!();
                    if condition < 1.0 {
                        let at = self
                            .skip_false(pc + 1)
                            .ok_or_else(|| format!("unterminated if at operation {pc}"))?;
                        // Resume after the else (run the else body) or after
                        // the endif.
                        next = at + 1;
                    }
                }
                op::ELSE => {
                    // The true branch ended: skip the else body.
                    let at = self
                        .skip_else(pc + 1)
                        .ok_or_else(|| format!("unterminated else at operation {pc}"))?;
                    next = at + 1;
                }
                op::END_IF => {}
                op::GOTO => {
                    next = usize::try_from(o.index)
                        .map_err(|_| format!("bad goto at operation {pc}"))?;
                }
                op::CALL => {
                    calls.push(pc + 1);
                    next = usize::try_from(o.index)
                        .map_err(|_| format!("bad call at operation {pc}"))?;
                }
                op::RETURN => match calls.pop() {
                    Some(back) => next = back,
                    None => return Ok(()),
                },
                op::FLOOR => {
                    let a = pop!();
                    stack.push(a.floor());
                }
                op::CEIL => {
                    let a = pop!();
                    stack.push(a.ceil());
                }
                op::SIN => {
                    let a = pop!();
                    stack.push(a.sin());
                }
                op::COS => {
                    let a = pop!();
                    stack.push(a.cos());
                }
                op::TAN => {
                    let a = pop!();
                    stack.push(a.tan());
                }
                op::ASIN => {
                    let a = pop!();
                    stack.push(a.asin());
                }
                op::ACOS => {
                    let a = pop!();
                    stack.push(a.acos());
                }
                op::ATAN => {
                    let a = pop!();
                    stack.push(a.atan());
                }
                other => return Err(format!("unknown arithmetica op {other} at operation {pc}")),
            }
            pc = next;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Assemble a program (the writer of `ArithmeticaFile.write`).
    pub(crate) fn assemble(
        operations: &[(u32, f32, i32, u32)],
        functions: &[(u32, u32)],
        strings: &[&str],
    ) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        let push = |body: &mut Vec<u8>, v: u32| body.extend_from_slice(&v.to_le_bytes());
        push(&mut body, operations.len() as u32);
        for (line, value, index, code) in operations {
            push(&mut body, *line);
            push(&mut body, value.to_bits());
            push(&mut body, *index as u32);
            push(&mut body, *code);
        }
        push(&mut body, functions.len() as u32);
        for (name, first) in functions {
            push(&mut body, *name);
            push(&mut body, *first);
        }
        push(&mut body, strings.len() as u32);
        push(&mut body, strings.len() as u32);
        for s in strings {
            body.extend_from_slice(s.as_bytes());
            body.push(0);
        }
        while body.len() % 4 != 0 {
            body.push(0);
        }
        // The stored size counts every dword after the first word: HTRA,
        // version, 2, the size itself and the body.
        let words = (body.len() / 4 + 4) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(&words.to_be_bytes());
        out.extend_from_slice(&0x4854_5241u32.to_be_bytes());
        push(&mut out, 1);
        push(&mut out, 2);
        push(&mut out, words);
        out.extend_from_slice(&body);
        out
    }

    fn ops(list: &[(u32, f32, i32)]) -> Vec<(u32, f32, i32, u32)> {
        list.iter()
            .enumerate()
            .map(|(i, (code, value, index))| (i as u32 + 1, *value, *index, *code))
            .collect()
    }

    #[test]
    fn parses_and_runs_arithmetic() {
        // x = (a + 2) * 3 ; y = x ^ 2 ; z = 10 / 4
        let list = ops(&[
            (op::PUSH_VARIABLE, 0.0, 0),
            (op::PUSH, 2.0, -1),
            (op::ADD, 0.0, -1),
            (op::PUSH, 3.0, -1),
            (op::MUL, 0.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 1),
            (op::PUSH_VARIABLE, 0.0, 1),
            (op::PUSH, 2.0, -1),
            (op::POW, 0.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 2),
            (op::PUSH, 10.0, -1),
            (op::PUSH, 4.0, -1),
            (op::DIV, 0.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 3),
        ]);
        let bytes = assemble(&list, &[], &["a", "x", "y", "z"]);
        let mut a = Arithmetica::from_bytes(&bytes).unwrap();
        a.set("a", 1.0);
        a.compute_all().unwrap();
        assert_eq!(a.get("x"), 9.0);
        assert_eq!(a.get("y"), 81.0);
        assert_eq!(a.get("z"), 2.5);
        assert_eq!(a.get("missing"), 0.0);
        assert_eq!(a.program().variables(), vec!["a", "x", "y", "z"]);
    }

    #[test]
    fn if_else_endif_and_nesting() {
        // if (a > 1) { if (a > 5) r = 3 else r = 2 } else { r = 1 }
        let list = ops(&[
            (op::PUSH_VARIABLE, 0.0, 0),
            (op::PUSH, 1.0, -1),
            (op::GREATER, 0.0, -1),
            (op::BEGIN_IF, 0.0, -1),
            (op::PUSH_VARIABLE, 0.0, 0),
            (op::PUSH, 5.0, -1),
            (op::GREATER, 0.0, -1),
            (op::BEGIN_IF, 0.0, -1),
            (op::PUSH, 3.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 1),
            (op::ELSE, 0.0, -1),
            (op::PUSH, 2.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 1),
            (op::END_IF, 0.0, -1),
            (op::ELSE, 0.0, -1),
            (op::PUSH, 1.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 1),
            (op::END_IF, 0.0, -1),
        ]);
        let bytes = assemble(&list, &[], &["a", "r"]);
        let mut p = Arithmetica::from_bytes(&bytes).unwrap();
        for (a, expect) in [(0.0, 1.0), (1.0, 1.0), (2.0, 2.0), (5.0, 2.0), (9.0, 3.0)] {
            p.set("a", a);
            p.compute_all().unwrap();
            assert_eq!(p.get("r"), expect, "a = {a}");
        }
    }

    #[test]
    fn comparisons_logic_functions_and_calls() {
        // r = (2 ~= 2) and (3 != 2) ; s = floor(2.7) + ceil(2.1) ; call f: t = 7
        let list = ops(&[
            (op::PUSH, 2.0, -1),
            (op::PUSH, 2.0, -1),
            (op::EQUAL, 0.0, -1),
            (op::PUSH, 3.0, -1),
            (op::PUSH, 2.0, -1),
            (op::NOT_EQUAL, 0.0, -1),
            (op::AND, 0.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 0),
            (op::PUSH, 2.7, -1),
            (op::FLOOR, 0.0, -1),
            (op::PUSH, 2.1, -1),
            (op::CEIL, 0.0, -1),
            (op::ADD, 0.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 1),
            (op::RETURN, 0.0, -1),
            (op::PUSH, 7.0, -1),
            (op::POP_INTO_VARIABLE, 0.0, 2),
            (op::RETURN, 0.0, -1),
        ]);
        let bytes = assemble(&list, &[(3, 15)], &["r", "s", "t", "f"]);
        let mut p = Arithmetica::from_bytes(&bytes).unwrap();
        p.compute_all().unwrap();
        assert_eq!((p.get("r"), p.get("s"), p.get("t")), (1.0, 5.0, 0.0));
        p.compute_function("f").unwrap();
        assert_eq!(p.get("t"), 7.0);
        assert!(p.compute_function("nope").is_err());
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(Arithmetica::from_bytes(&[]).is_err());
        assert!(Arithmetica::from_bytes(&[0u8; 40]).is_err());
        let mut good = assemble(&ops(&[(op::PUSH, 1.0, -1)]), &[], &[]);
        good[5] = b'X';
        assert!(Program::parse(&good).is_err());
        // Underflow is reported, not a panic.
        let bad = assemble(&ops(&[(op::ADD, 0.0, -1)]), &[], &[]);
        assert!(Arithmetica::from_bytes(&bad)
            .unwrap()
            .compute_all()
            .is_err());
        // A goto loop hits the step budget instead of hanging.
        let looped = assemble(&[(1, 0.0, 0, op::GOTO)], &[], &[]);
        assert!(Arithmetica::from_bytes(&looped)
            .unwrap()
            .compute_all()
            .is_err());
    }
}
