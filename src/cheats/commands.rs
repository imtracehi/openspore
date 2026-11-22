//! The cheat commands and their parser. Spore's own cheats keep the names,
//! arguments and help text of the original console (`App::ICheatManager`);
//! OpenSpore adds its own cheats and debug switches next to them. Names are
//! matched without regard to case, like the original.

/// `on`, `off`, or no argument (flip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    On,
    Off,
    Flip,
}

impl Toggle {
    pub fn apply(self, value: &mut bool) -> bool {
        *value = match self {
            Toggle::On => true,
            Toggle::Off => false,
            Toggle::Flip => !*value,
        };
        *value
    }
}

/// The stages a `gotoStage` cheat can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageName {
    Cell,
    Creature,
    Tribal,
    Civilization,
    Space,
}

impl StageName {
    pub const ALL: [StageName; 5] = [
        StageName::Cell,
        StageName::Creature,
        StageName::Tribal,
        StageName::Civilization,
        StageName::Space,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StageName::Cell => "Cell",
            StageName::Creature => "Creature",
            StageName::Tribal => "Tribal",
            StageName::Civilization => "Civilization",
            StageName::Space => "Space",
        }
    }

    fn parse(text: &str) -> Option<StageName> {
        let t = text.to_ascii_lowercase();
        StageName::ALL.into_iter().find(|s| {
            let l = s.label().to_ascii_lowercase();
            l == t || (t.len() >= 3 && l.starts_with(&t)) || (t == "tribe" && *s == StageName::Tribal)
        })
    }
}

/// One cheat to run. The console parses typed text into these; the debug
/// menu sends them directly.
#[derive(Clone, Debug, PartialEq)]
pub enum Cheat {
    // ---- Spore's cheats ----
    /// `help [<pattern>] [-full]`
    Help { pattern: Option<String>, full: bool },
    /// `addDNA`: increases your DNA points.
    AddDna,
    /// `moreMoney`: increases your money in Civilization or Space.
    MoreMoney,
    /// `refillMotives`: replenishes depleted health and other motives.
    RefillMotives,
    /// `freeCam`: toggles free camera mode.
    FreeCam,
    /// `pauseUIVisible`: whether the UI stays visible while paused.
    PauseUiVisible(Toggle),
    /// `spaceCreate`: unlocks and recharges creation tools in Space.
    SpaceCreate,
    /// `unlockSuperWeapons`: all superweapons for your Civilization type.
    UnlockSuperWeapons,
    /// `setConsequenceTrait <trait>`: the trait deciding the super powers.
    SetConsequenceTrait(String),
    /// `setTime (h, m)`: time of day at the avatar's position.
    SetTime { hour: u32, minute: u32 },
    /// `setTime -speed <multiplier>`
    SetTimeSpeed(f32),
    /// `evoAdvantage`
    EvoAdvantage,
    /// `freedom [on | off]`: editor complexity limits off (on) or back on.
    Freedom(Toggle),
    /// `toggleEditorBackground`
    ToggleEditorBackground,

    // ---- OpenSpore's cheats ----
    /// `godMode [on|off]`: the player cannot be hurt.
    GodMode(Toggle),
    /// `infiniteEnergy [on|off]`: abilities and ship energy never run out.
    InfiniteEnergy(Toggle),
    /// `addDNA` with an amount, for the menu: `dna <amount>`.
    Dna(i64),
    /// `money <amount>`
    Money(i64),
    /// `food <amount>` (Tribal)
    Food(i64),
    /// `completeStage`: fills the progress bar and ends the stage.
    CompleteStage,
    /// `progress <percent>`: adds to the stage progress bar.
    Progress(f32),
    /// `gotoStage <stage>`: starts a stage directly.
    GotoStage(StageName),
    /// `unlockParts`: every creature / cell part in the editors.
    UnlockParts,
    /// `teleport`: moves the player to the cursor.
    Teleport,
    /// `smite [radius]`: hostile creatures / units near the player die.
    Smite(f32),
    /// `healAll`: the player's tribe, units or allies are healed.
    HealAll,
    /// `addMember`: one more tribe member / pack member.
    AddMember,
    /// `captureCity`: the nearest rival city joins you (Civilization).
    CaptureCity,
    /// `revealGalaxy`: every star is explored (Space).
    RevealGalaxy,
    /// `warpHome`: the ship returns to its homeworld (Space).
    WarpHome,
    /// `spice <amount>` (Space)
    Spice(i64),
    /// `timeScale <x>`: game speed (0.1 - 8).
    TimeScale(f32),
    /// `stats [on|off]`: frame rate, stage and player readout.
    Stats(Toggle),
    /// `hud [on|off]`: hides every HUD (screenshots).
    Hud(Toggle),
    /// `screenshot`: saves the screen to the screenshots folder.
    Screenshot,
    /// `clear`: empties the console.
    Clear,
}

impl Cheat {
    /// The console command name.
    pub fn name(&self) -> &'static str {
        match self {
            Cheat::Help { .. } => "help",
            Cheat::AddDna => "addDNA",
            Cheat::MoreMoney => "moreMoney",
            Cheat::RefillMotives => "refillMotives",
            Cheat::FreeCam => "freeCam",
            Cheat::PauseUiVisible(_) => "pauseUIVisible",
            Cheat::SpaceCreate => "spaceCreate",
            Cheat::UnlockSuperWeapons => "unlockSuperWeapons",
            Cheat::SetConsequenceTrait(_) => "setConsequenceTrait",
            Cheat::SetTime { .. } | Cheat::SetTimeSpeed(_) => "setTime",
            Cheat::EvoAdvantage => "evoAdvantage",
            Cheat::Freedom(_) => "freedom",
            Cheat::ToggleEditorBackground => "toggleEditorBackground",
            Cheat::GodMode(_) => "godMode",
            Cheat::InfiniteEnergy(_) => "infiniteEnergy",
            Cheat::Dna(_) => "dna",
            Cheat::Money(_) => "money",
            Cheat::Food(_) => "food",
            Cheat::CompleteStage => "completeStage",
            Cheat::Progress(_) => "progress",
            Cheat::GotoStage(_) => "gotoStage",
            Cheat::UnlockParts => "unlockParts",
            Cheat::Teleport => "teleport",
            Cheat::Smite(_) => "smite",
            Cheat::HealAll => "healAll",
            Cheat::AddMember => "addMember",
            Cheat::CaptureCity => "captureCity",
            Cheat::RevealGalaxy => "revealGalaxy",
            Cheat::WarpHome => "warpHome",
            Cheat::Spice(_) => "spice",
            Cheat::TimeScale(_) => "timeScale",
            Cheat::Stats(_) => "stats",
            Cheat::Hud(_) => "hud",
            Cheat::Screenshot => "screenshot",
            Cheat::Clear => "clear",
        }
    }
}

/// Where a command comes from (the help list groups them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Spore,
    OpenSpore,
}

/// One entry of the command list.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub name: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
    pub origin: Origin,
}

const fn spore(name: &'static str, usage: &'static str, help: &'static str) -> Entry {
    Entry { name, usage, help, origin: Origin::Spore }
}

const fn ours(name: &'static str, usage: &'static str, help: &'static str) -> Entry {
    Entry { name, usage, help, origin: Origin::OpenSpore }
}

/// Every command, Spore's first. The Spore help texts are the original ones.
pub const COMMANDS: &[Entry] = &[
    spore("help", "[<pattern>] [-full]", "Lists help for all cheats, or those that match <pattern>"),
    spore("addDNA", "", "increases your DNA points"),
    spore("moreMoney", "", "Increases your money in Civilization or Space"),
    spore("refillMotives", "", "Replenishes depleted health and other motives."),
    spore("freeCam", "", "Toggles free camera mode."),
    spore("pauseUIVisible", "[-show | -hide | -toggle]", "Shows or hides the user interface while the game is paused."),
    spore("spaceCreate", "", "Unlocks and recharges creation tools while playing in space."),
    spore("unlockSuperWeapons", "", "Unlocks all superweapons for your Civilization type."),
    spore(
        "setConsequenceTrait",
        "<trait> (cell_carnivore ... space_knight)",
        "Sets the trait which will determine what super powers are available.",
    ),
    spore(
        "setTime",
        "(h, m) | -speed <multiplier>",
        "Sets the time of day at the avatar's position, and optionally a speed multiplier.",
    ),
    spore(
        "evoAdvantage",
        "",
        "unlocks the ability to choose evolutionarily advantaged creatures from the Sporepedia when starting creature game",
    ),
    spore(
        "freedom",
        "[on | off]",
        "Toggles, disables (on) or re-enables (off) editor complexity limits.",
    ),
    spore("toggleEditorBackground", "", "Shows or hides the editor's room and backdrop."),
    ours("godMode", "[on | off]", "The player cannot be hurt."),
    ours("infiniteEnergy", "[on | off]", "Abilities, tools and ship energy never run out."),
    ours("dna", "<amount>", "Adds (or with a minus sign removes) DNA points."),
    ours("money", "<amount>", "Adds (or removes) Sporebucks."),
    ours("food", "<amount>", "Adds (or removes) food in the Tribal stage."),
    ours("spice", "<amount>", "Adds spice to the ship's cargo."),
    ours("progress", "<percent>", "Moves the stage progress bar ahead."),
    ours("completeStage", "", "Fills the progress bar so the stage can end."),
    ours("gotoStage", "<cell | creature | tribal | civ | space>", "Starts a new game directly in that stage."),
    ours("unlockParts", "", "Unlocks every part in the creature and cell editors."),
    ours("teleport", "", "Moves the player to the point under the cursor."),
    ours("smite", "[radius]", "Other species' creatures, hostile animals, rival tribes or rival vehicles near the player die."),
    ours("healAll", "", "Heals the player and every member, unit or ally."),
    ours("addMember", "", "Adds a member to your tribe or pack."),
    ours("captureCity", "", "The nearest rival city joins your nation."),
    ours("revealGalaxy", "", "Every star in the galaxy counts as explored."),
    ours("warpHome", "", "The ship jumps back to its homeworld."),
    ours("timeScale", "<0.1 - 8>", "Game speed multiplier (1 = normal)."),
    ours("stats", "[on | off]", "Shows frame rate, stage, position and time of day."),
    ours("hud", "[on | off]", "Shows or hides the whole HUD."),
    ours("screenshot", "", "Saves the screen to the OpenSpore screenshots folder."),
    ours("clear", "", "Empties the console."),
];

/// The command named `name` (any case).
pub fn find(name: &str) -> Option<&'static Entry> {
    COMMANDS.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// Command names starting with `prefix` (any case), for Tab completion.
pub fn complete(prefix: &str) -> Vec<&'static str> {
    let p = prefix.to_ascii_lowercase();
    COMMANDS
        .iter()
        .filter(|e| e.name.to_ascii_lowercase().starts_with(&p))
        .map(|e| e.name)
        .collect()
}

fn toggle(args: &[&str]) -> Result<Toggle, String> {
    match args.first().map(|a| a.to_ascii_lowercase()) {
        None => Ok(Toggle::Flip),
        // `-show` / `-hide` / `-toggle` are pauseUIVisible's own switches.
        Some(a) if a == "on" || a == "1" || a == "true" || a == "-show" => Ok(Toggle::On),
        Some(a) if a == "off" || a == "0" || a == "false" || a == "-hide" => Ok(Toggle::Off),
        Some(a) if a == "-toggle" => Ok(Toggle::Flip),
        Some(a) => Err(format!("expected on or off, got \"{a}\"")),
    }
}

fn number<T: std::str::FromStr>(args: &[&str], what: &str) -> Result<T, String> {
    let Some(a) = args.first() else {
        return Err(format!("needs {what}"));
    };
    a.replace(',', "").parse::<T>().map_err(|_| format!("\"{a}\" is not {what}"))
}

/// `setTime` arguments: `(h, m)`, `h m`, `h:m` or `h`; `-speed x`.
fn set_time(args: &[&str]) -> Result<Cheat, String> {
    let joined = args.join(" ");
    let lower = joined.to_ascii_lowercase();
    if let Some(rest) = lower.trim().strip_prefix("-speed") {
        let speed: f32 = rest.trim().parse().map_err(|_| "-speed needs a number".to_string())?;
        if !speed.is_finite() {
            return Err("-speed needs a number".into());
        }
        // The original keeps at least 2^-16 (no upper limit); OpenSpore's
        // own "stop the clock" button sends 0.
        return Ok(Cheat::SetTimeSpeed(speed.max(0.0)));
    }
    let cleaned: String = joined
        .chars()
        .map(|c| if c == '(' || c == ')' || c == ',' || c == ':' { ' ' } else { c })
        .collect();
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err("usage: setTime (h, m) or setTime -speed <multiplier>".into());
    }
    let hour: u32 = parts[0].parse().map_err(|_| format!("\"{}\" is not an hour", parts[0]))?;
    let minute: u32 = match parts.get(1) {
        Some(m) => m.parse().map_err(|_| format!("\"{m}\" is not a minute"))?,
        None => 0,
    };
    if hour > 24 || minute > 59 || (hour == 24 && minute > 0) {
        return Err("the time must be between 0:00 and 24:00".into());
    }
    Ok(Cheat::SetTime { hour: hour % 24, minute })
}

/// Parses one console line. `Ok(None)` for an empty line.
pub fn parse(line: &str) -> Result<Option<Cheat>, String> {
    let mut words = line.split_whitespace();
    let Some(name) = words.next() else {
        return Ok(None);
    };
    let args: Vec<&str> = words.collect();
    let Some(entry) = find(name) else {
        return Err(format!("Unknown cheat \"{name}\". Type help for the list."));
    };
    let cheat = match entry.name {
        "help" => {
            let full = args.iter().any(|a| a.eq_ignore_ascii_case("-full"));
            let pattern = args.iter().find(|a| !a.starts_with('-')).map(|a| a.to_string());
            Cheat::Help { pattern, full }
        }
        "addDNA" => Cheat::AddDna,
        "moreMoney" => Cheat::MoreMoney,
        "refillMotives" => Cheat::RefillMotives,
        "freeCam" => Cheat::FreeCam,
        "pauseUIVisible" => Cheat::PauseUiVisible(toggle(&args)?),
        "spaceCreate" => Cheat::SpaceCreate,
        "unlockSuperWeapons" => Cheat::UnlockSuperWeapons,
        "setConsequenceTrait" => match args.first() {
            Some(t) => Cheat::SetConsequenceTrait(t.to_ascii_lowercase()),
            None => return Err("usage: setConsequenceTrait <trait>".into()),
        },
        "setTime" => set_time(&args)?,
        "evoAdvantage" => Cheat::EvoAdvantage,
        "freedom" => Cheat::Freedom(toggle(&args)?),
        "toggleEditorBackground" => Cheat::ToggleEditorBackground,
        "godMode" => Cheat::GodMode(toggle(&args)?),
        "infiniteEnergy" => Cheat::InfiniteEnergy(toggle(&args)?),
        "dna" => Cheat::Dna(number(&args, "a whole number")?),
        "money" => Cheat::Money(number(&args, "a whole number")?),
        "food" => Cheat::Food(number(&args, "a whole number")?),
        "spice" => Cheat::Spice(number(&args, "a whole number")?),
        "progress" => {
            let p: f32 = number(&args, "a percentage")?;
            if !p.is_finite() {
                return Err("progress needs a percentage".into());
            }
            Cheat::Progress(p.clamp(-100.0, 100.0))
        }
        "completeStage" => Cheat::CompleteStage,
        "gotoStage" => match args.first().and_then(|a| StageName::parse(a)) {
            Some(s) => Cheat::GotoStage(s),
            None => return Err("usage: gotoStage <cell | creature | tribal | civ | space>".into()),
        },
        "unlockParts" => Cheat::UnlockParts,
        "teleport" => Cheat::Teleport,
        "smite" => {
            let r: f32 = if args.is_empty() { 40.0 } else { number(&args, "a radius")? };
            Cheat::Smite(r.clamp(1.0, 500.0))
        }
        "healAll" => Cheat::HealAll,
        "addMember" => Cheat::AddMember,
        "captureCity" => Cheat::CaptureCity,
        "revealGalaxy" => Cheat::RevealGalaxy,
        "warpHome" => Cheat::WarpHome,
        "timeScale" => {
            let s: f32 = number(&args, "a number")?;
            if !s.is_finite() {
                return Err("timeScale needs a number".into());
            }
            Cheat::TimeScale(s.clamp(0.1, 8.0))
        }
        "stats" => Cheat::Stats(toggle(&args)?),
        "hud" => Cheat::Hud(toggle(&args)?),
        "screenshot" => Cheat::Screenshot,
        "clear" => Cheat::Clear,
        other => return Err(format!("\"{other}\" has no handler")),
    };
    Ok(Some(cheat))
}

/// The help lines for `help [pattern] [-full]`.
pub fn help(pattern: Option<&str>, full: bool) -> Vec<String> {
    let p = pattern.map(|p| p.to_ascii_lowercase());
    let matching: Vec<&Entry> = COMMANDS
        .iter()
        .filter(|e| p.as_ref().is_none_or(|p| e.name.to_ascii_lowercase().contains(p.as_str())))
        .collect();
    if matching.is_empty() {
        return vec![format!("No cheat matches \"{}\".", pattern.unwrap_or_default())];
    }
    let mut out = Vec::new();
    for origin in [Origin::Spore, Origin::OpenSpore] {
        let group: Vec<&&Entry> = matching.iter().filter(|e| e.origin == origin).collect();
        if group.is_empty() {
            continue;
        }
        out.push(match origin {
            Origin::Spore => "Spore cheats:".to_string(),
            Origin::OpenSpore => "OpenSpore cheats:".to_string(),
        });
        for e in group {
            if full || matching.len() <= 3 {
                let usage = if e.usage.is_empty() { String::new() } else { format!(" {}", e.usage) };
                out.push(format!("  {}{usage} - {}", e.name, e.help));
            } else {
                out.push(format!("  {} - {}", e.name, e.help));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spore_cheat_names_parse_in_any_case() {
        assert_eq!(parse("addDNA").unwrap(), Some(Cheat::AddDna));
        assert_eq!(parse("ADDDNA").unwrap(), Some(Cheat::AddDna));
        assert_eq!(parse("moremoney").unwrap(), Some(Cheat::MoreMoney));
        assert_eq!(parse("refillMotives").unwrap(), Some(Cheat::RefillMotives));
        assert_eq!(parse("freecam").unwrap(), Some(Cheat::FreeCam));
        assert_eq!(parse("spaceCreate").unwrap(), Some(Cheat::SpaceCreate));
        assert_eq!(parse("unlockSuperWeapons").unwrap(), Some(Cheat::UnlockSuperWeapons));
        assert_eq!(parse("evoadvantage").unwrap(), Some(Cheat::EvoAdvantage));
        assert_eq!(parse("pauseUIVisible off").unwrap(), Some(Cheat::PauseUiVisible(Toggle::Off)));
        assert_eq!(parse("pauseUIVisible -hide").unwrap(), Some(Cheat::PauseUiVisible(Toggle::Off)));
        assert_eq!(parse("pauseUIVisible -show").unwrap(), Some(Cheat::PauseUiVisible(Toggle::On)));
        assert_eq!(parse("pauseUIVisible -toggle").unwrap(), Some(Cheat::PauseUiVisible(Toggle::Flip)));
        assert_eq!(
            parse("setConsequenceTrait Space_Bard").unwrap(),
            Some(Cheat::SetConsequenceTrait("space_bard".into()))
        );
        assert_eq!(parse("freedom").unwrap(), Some(Cheat::Freedom(Toggle::Flip)));
        assert_eq!(parse("   ").unwrap(), None);
    }

    #[test]
    fn set_time_accepts_the_original_forms() {
        let at = |h, m| Some(Cheat::SetTime { hour: h, minute: m });
        assert_eq!(parse("setTime (13, 30)").unwrap(), at(13, 30));
        assert_eq!(parse("settime 6 5").unwrap(), at(6, 5));
        assert_eq!(parse("setTime 21:45").unwrap(), at(21, 45));
        assert_eq!(parse("setTime 24").unwrap(), at(0, 0));
        assert_eq!(parse("setTime -speed 2.5").unwrap(), Some(Cheat::SetTimeSpeed(2.5)));
        assert!(parse("setTime 25 0").is_err());
        assert!(parse("setTime").is_err());
        assert!(parse("setTime -speed fast").is_err());
    }

    #[test]
    fn arguments_are_checked() {
        assert_eq!(parse("dna 500").unwrap(), Some(Cheat::Dna(500)));
        assert_eq!(parse("money 1,000,000").unwrap(), Some(Cheat::Money(1_000_000)));
        assert_eq!(parse("timeScale 50").unwrap(), Some(Cheat::TimeScale(8.0)));
        assert_eq!(parse("gotoStage civ").unwrap(), Some(Cheat::GotoStage(StageName::Civilization)));
        assert_eq!(parse("gotostage tribe").unwrap(), Some(Cheat::GotoStage(StageName::Tribal)));
        assert_eq!(parse("godMode on").unwrap(), Some(Cheat::GodMode(Toggle::On)));
        assert!(parse("godMode maybe").is_err());
        assert!(parse("dna lots").is_err());
        assert!(parse("gotoStage moon").is_err());
        assert!(parse("flyToTheMoon").unwrap_err().contains("Unknown cheat"));
    }

    #[test]
    fn every_command_has_a_parser_and_help() {
        for e in COMMANDS {
            assert!(!e.help.is_empty(), "{} has no help", e.name);
            // Commands that need an argument fail with a usage message, never
            // with "has no handler".
            if let Err(e2) = parse(e.name) {
                assert!(!e2.contains("has no handler"), "{}: {e2}", e.name);
            }
        }
        let names: std::collections::HashSet<String> = COMMANDS.iter().map(|e| e.name.to_ascii_lowercase()).collect();
        assert_eq!(names.len(), COMMANDS.len(), "duplicate command names");
        // A parsed cheat reports the command it came from.
        for line in ["addDNA", "setTime 6", "setTime -speed 2", "dna 5", "gotoStage space", "smite", "clear"] {
            let cheat = parse(line).unwrap().unwrap();
            assert!(find(cheat.name()).is_some(), "{line} -> {}", cheat.name());
            assert!(line.to_ascii_lowercase().starts_with(&cheat.name().to_ascii_lowercase()));
        }
    }

    #[test]
    fn help_lists_and_filters() {
        let all = help(None, false);
        assert_eq!(all[0], "Spore cheats:");
        assert!(all.iter().any(|l| l == "OpenSpore cheats:"));
        let money = help(Some("money"), false);
        assert!(money.iter().any(|l| l.contains("moreMoney")));
        assert!(money.iter().any(|l| l.contains("money <amount>")));
        assert!(help(Some("zzz"), false)[0].starts_with("No cheat"));
        assert_eq!(complete("re"), vec!["refillMotives", "revealGalaxy"]);
    }
}
