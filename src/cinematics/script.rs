//! The ArgScript subset of the cinematic state scripts (`scripts3!*.trigger`).
//!
//! Original data: the 14 `trigger` resources (type 0x024A0E52) of group
//! `scripts3` (0x40890200); the root file `config` only `sinclude`s the other
//! 13 (notes\cinematics-re-notes.md section 1). Format, as compiled by the
//! generic state-machine loader 0x7E2370:
//!
//! ```text
//! state <Name> [-id <hash>]      id = FNV-1 of the lowercase name unless -id
//!    <command> <args...> [-option values...] [-flag...]
//! end
//! sinclude "<file>.txt"          another trigger resource of the same group
//! transition <from> <to> ... end generic FSM feature, unused by the cinematics
//! ```
//!
//! Tokens: whitespace separated words, `"quoted names"`, `( ... )` groups
//! (vectors and id lists) and `#` comments. A word starting with `-` and a
//! letter is an option; the words after it (up to the next option) are its
//! values. Keywords and option names are compared case-insensitively.
//! INFERRED: the scripts mix `mixevent`/`mixEvent`, `-escstop`/`-escStop` and
//! `-targetpos`/`-targetPos`; the engine's ArgScript comparison was not traced,
//! so every spelling is accepted.

use std::collections::HashMap;

use crate::world_objects::name_id;

/// Resource type of the state scripts.
pub const TRIGGER_TYPE: u32 = 0x024A_0E52;
/// `scripts3` group.
pub const SCRIPTS_GROUP: u32 = 0x4089_0200;
/// Root script (`config`), which includes all the others.
pub const ROOT_SCRIPT: &str = "config";

/// One token of a script line.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// A bare word (`avatar`, `.5`, `0x053d9bc9`, `cinematics_crg!0x04864c3c`).
    Word(String),
    /// A `"quoted"` string, without the quotes.
    Quoted(String),
    /// A `( ... )` group, without the parentheses.
    Group(String),
}

impl Token {
    /// The text of the token (quotes and parentheses removed).
    pub fn text(&self) -> &str {
        match self {
            Token::Word(s) | Token::Quoted(s) | Token::Group(s) => s,
        }
    }

    pub fn float(&self) -> Option<f32> {
        parse_float(self.text())
    }

    /// A `(x, y, z)` vector (Spore frame: x right, y forward, z up).
    pub fn vector(&self) -> Option<[f32; 3]> {
        let values: Vec<f32> = split_group(self.text()).filter_map(parse_float).collect();
        match values.as_slice() {
            [x, y, z, ..] => Some([*x, *y, *z]),
            _ => None,
        }
    }

    /// A hash: `0x...` hex is taken as is, anything else is a name (FNV-1 of
    /// the lowercase text, `name_id`).
    pub fn hash(&self) -> u32 {
        parse_hash(self.text())
    }

    /// The ids of a `(0x... 0x...)` list, or of a single hex word.
    pub fn id_list(&self) -> Vec<u32> {
        split_group(self.text()).map(parse_hash).collect()
    }

    fn is_option(&self) -> bool {
        match self {
            Token::Word(s) => {
                let mut chars = s.chars();
                chars.next() == Some('-') && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            }
            _ => false,
        }
    }
}

fn split_group(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty())
}

pub fn parse_float(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok().map(|v| v as f32);
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

pub fn parse_hash(text: &str) -> u32 {
    let t = text.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).unwrap_or_else(|_| name_id(t)),
        None => name_id(t),
    }
}

/// `table!0xINSTANCE` (a locale table name and a string id).
pub fn parse_resource_ref(text: &str) -> Option<(u32, u32)> {
    let (table, id) = text.split_once('!')?;
    Some((parse_hash(table), parse_hash(id)))
}

/// One parsed command line: keyword, positional arguments, options.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The keyword in lowercase.
    pub keyword: String,
    pub args: Vec<Token>,
    /// Option name (without the dash, lowercase) and its values, in order.
    pub options: Vec<(String, Vec<Token>)>,
    /// 1-based line number in its file (diagnostics).
    pub line: usize,
}

impl Line {
    /// Split one script line (comments removed). `None` for blank lines.
    pub fn parse(text: &str, line: usize) -> Option<Line> {
        let tokens = tokenize(text);
        let mut iter = tokens.into_iter();
        let keyword = iter.next()?.text().to_ascii_lowercase();
        let mut args = Vec::new();
        let mut options: Vec<(String, Vec<Token>)> = Vec::new();
        for token in iter {
            if token.is_option() {
                options.push((token.text()[1..].to_ascii_lowercase(), Vec::new()));
            } else if let Some((_, values)) = options.last_mut() {
                values.push(token);
            } else {
                args.push(token);
            }
        }
        Some(Line { keyword, args, options, line })
    }

    pub fn flag(&self, name: &str) -> bool {
        self.options.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
    }

    /// The values of an option (empty slice when given without values).
    pub fn option(&self, name: &str) -> Option<&[Token]> {
        self.options
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }

    pub fn option_float(&self, name: &str) -> Option<f32> {
        self.option(name)?.first()?.float()
    }

    pub fn option_vector(&self, name: &str) -> Option<[f32; 3]> {
        self.option(name)?.first()?.vector()
    }

    pub fn option_hash(&self, name: &str) -> Option<u32> {
        Some(self.option(name)?.first()?.hash())
    }

    pub fn option_text(&self, name: &str) -> Option<&str> {
        Some(self.option(name)?.first()?.text())
    }

    pub fn arg(&self, i: usize) -> Option<&Token> {
        self.args.get(i)
    }

    /// Positional arguments followed by the values a flag swallowed (a flag
    /// such as `-snap` written before a positional value).
    pub fn arg_text(&self, i: usize) -> Option<&str> {
        self.args.get(i).map(Token::text)
    }
}

/// Split a line into tokens; `#` starts a comment outside quotes and groups.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '#' {
            break;
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            out.push(Token::Quoted(s));
        } else if c == '(' {
            chars.next();
            let mut depth = 1;
            let mut s = String::new();
            for c in chars.by_ref() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                s.push(c);
            }
            out.push(Token::Group(s.trim().to_string()));
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '"' || c == '(' || c == '#' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(Token::Word(s));
        }
    }
    out
}

/// One state of the cinematic state machine.
#[derive(Clone, Debug)]
pub struct State {
    pub name: String,
    pub id: u32,
    /// Script file the state came from (diagnostics, dump test).
    pub file: String,
    pub lines: Vec<Line>,
}

/// Every state of the installed scripts, by id.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub states: HashMap<u32, State>,
    /// Script files in load order.
    pub files: Vec<String>,
    /// Problems found while loading (missing includes, nested states...).
    pub errors: Vec<String>,
}

impl Library {
    pub fn state(&self, id: u32) -> Option<&State> {
        self.states.get(&id)
    }

    pub fn state_by_name(&self, name: &str) -> Option<&State> {
        self.states.get(&name_id(name))
    }

    /// Load the root script and everything it includes. `read` returns the
    /// text of a script by its file name hash (instance id).
    pub fn load(mut read: impl FnMut(u32) -> Option<String>) -> Library {
        let mut library = Library::default();
        let mut stack = vec![ROOT_SCRIPT.to_string()];
        let mut seen = std::collections::HashSet::new();
        // Includes are processed in file order (depth first).
        while let Some(file) = stack.pop() {
            let id = name_id(&file);
            if !seen.insert(id) {
                continue;
            }
            let Some(text) = read(id) else {
                library.errors.push(format!("script {file} ({id:08X}) is not installed"));
                continue;
            };
            let includes = library.add_file(&file, &text);
            for include in includes.into_iter().rev() {
                stack.push(include);
            }
        }
        library
    }

    /// Parse one script file; returns its `sinclude` file names (extension
    /// stripped).
    pub fn add_file(&mut self, file: &str, text: &str) -> Vec<String> {
        self.files.push(file.to_string());
        let mut includes = Vec::new();
        let mut current: Option<State> = None;
        let mut in_transition = false;
        for (n, raw) in text.lines().enumerate() {
            let Some(line) = Line::parse(raw, n + 1) else {
                continue;
            };
            match line.keyword.as_str() {
                "state" => {
                    if let Some(open) = current.take() {
                        self.errors.push(format!(
                            "{file}:{}: state {} opened inside {} (can't nest states)",
                            n + 1,
                            line.arg_text(0).unwrap_or("?"),
                            open.name
                        ));
                        self.insert(open);
                    }
                    let name = line.arg_text(0).unwrap_or("").to_string();
                    let id = line.option_hash("id").unwrap_or_else(|| name_id(&name));
                    current = Some(State { name, id, file: file.to_string(), lines: Vec::new() });
                }
                "end" => {
                    if in_transition {
                        in_transition = false;
                    } else if let Some(state) = current.take() {
                        self.insert(state);
                    } else {
                        self.errors.push(format!("{file}:{}: end without state", n + 1));
                    }
                }
                "transition" => in_transition = true,
                "sinclude" if current.is_none() => {
                    if let Some(name) = line.arg_text(0) {
                        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                        includes.push(stem.to_string());
                    }
                }
                _ if in_transition => {}
                _ => match current.as_mut() {
                    Some(state) => state.lines.push(line),
                    None => self.errors.push(format!(
                        "{file}:{}: `{}` outside a state",
                        n + 1,
                        line.keyword
                    )),
                },
            }
        }
        if let Some(state) = current.take() {
            self.errors.push(format!("{file}: state {} has no end", state.name));
            self.insert(state);
        }
        includes
    }

    fn insert(&mut self, state: State) {
        if let Some(old) = self.states.get(&state.id) {
            self.errors.push(format!(
                "state {} ({}) redefined in {} (first in {})",
                state.name, state.id, state.file, old.file
            ));
        }
        self.states.insert(state.id, state);
    }

    /// Load from the installed packages (PatchData.package holds them).
    pub fn from_index(index: &crate::asset_catalog::ResourceIndex) -> Library {
        Library::load(|instance| {
            let key = crate::dbpf::ResourceKey {
                type_id: TRIGGER_TYPE,
                group_id: SCRIPTS_GROUP,
                instance_id: instance,
            };
            let bytes = index.read(key).ok()?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_keep_quotes_groups_and_drop_comments() {
        let line = Line::parse(
            r#"   moveActor "Buddy1" (-2, -8, 0) -relative actor "TheAvatar" -face actor "TheAvatar" -noWait -snap # off shot"#,
            7,
        )
        .unwrap();
        assert_eq!(line.keyword, "moveactor");
        assert_eq!(line.args, vec![Token::Quoted("Buddy1".into()), Token::Group("-2, -8, 0".into())]);
        assert_eq!(line.args[1].vector(), Some([-2.0, -8.0, 0.0]));
        let relative = line.option("relative").unwrap();
        assert_eq!(relative[0].text(), "actor");
        assert_eq!(relative[1].text(), "TheAvatar");
        assert!(line.flag("noWait") && line.flag("SNAP"));
        assert!(!line.flag("ease"));
        assert_eq!(line.line, 7);
    }

    #[test]
    fn negative_numbers_are_values_not_options() {
        let line = Line::parse("startcamera -target \"TheAvatar\" -pitch -.05 -heading .5 -offset -15", 1).unwrap();
        assert_eq!(line.option_float("pitch"), Some(-0.05));
        assert_eq!(line.option_float("heading"), Some(0.5));
        assert_eq!(line.option_float("offset"), Some(-15.0));
        assert_eq!(line.option_hash("target"), Some(name_id("TheAvatar")));
        assert_eq!(name_id("TheAvatar"), 0xEFE7_6209);
    }

    #[test]
    fn hashes_ids_and_resource_refs() {
        assert_eq!(parse_hash("0x053d9bc9"), 0x053D_9BC9);
        assert_eq!(parse_hash("null"), 0x2EBC_D6A6);
        assert_eq!(parse_resource_ref("cinematics_crg!0x04864c3c"), Some((name_id("cinematics_crg"), 0x0486_4C3C)));
        let list = Line::parse("playAnimation \"A\" (0x065652d4 0x065652d1) -noWait", 1).unwrap();
        assert_eq!(list.args[1].id_list(), vec![0x0656_52D4, 0x0656_52D1]);
        let single = Line::parse("playAnimation \"TheAvatar\" 0x0541ba87 # gen_brainup_admire", 1).unwrap();
        assert_eq!(single.args[1].id_list(), vec![0x0541_BA87]);
    }

    #[test]
    fn states_includes_and_transitions() {
        let mut library = Library::default();
        let includes = library.add_file(
            "config",
            "sinclude \"global.txt\"\r\nsinclude \"CLG2CRG.txt\"\r\n\r\nstate A\r\n   letterbox -on\r\n   nextState B # next\r\nend\r\n\
             transition A B\r\n  foo\r\nend\r\nstate B -id 0x1234\r\nend\r\n",
        );
        assert_eq!(includes, vec!["global".to_string(), "CLG2CRG".to_string()]);
        let a = library.state_by_name("a").unwrap();
        assert_eq!(a.lines.len(), 2);
        assert_eq!(a.lines[1].keyword, "nextstate");
        assert!(library.state(0x1234).is_some());
        assert!(library.errors.is_empty(), "{:?}", library.errors);
    }

    #[test]
    fn load_follows_includes_and_reports_missing_files() {
        let files: HashMap<u32, &str> = [
            (name_id("config"), "sinclude \"global.txt\"\nsinclude \"missing.txt\"\n"),
            (name_id("global"), "state stopCurrentCinematic\n stopCinematic\nend\n"),
        ]
        .into_iter()
        .collect();
        let library = Library::load(|id| files.get(&id).map(|s| s.to_string()));
        assert_eq!(library.files, vec!["config".to_string(), "global".to_string()]);
        assert!(library.state_by_name("stopCurrentCinematic").is_some());
        assert_eq!(library.errors.len(), 1, "{:?}", library.errors);
    }
}
