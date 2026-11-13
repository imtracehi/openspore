//! Interface sounds of the front end, the creators and the Space HUD (helper `sound-ui`,
//! `notes\audio-audit.md` gap G2 / G3 and the Shell / UI list).
//!
//! The retail layouts carry no `Sound Effect` procedure: the code plays every interface
//! sound from the handler of the control. The handlers that were decompiled are
//! switch statements on the control id (message 0x287259F6 = click, a hover message for
//! the roll-overs); this module attaches the same names to the same control ids of the same
//! layouts and plays them through `cell_audio::NamedSounds` (the retail soundProp: gain,
//! pitch, polyphony). Three kinds of triggers:
//!
//! * **SPUI rules** ([`rules`]): `(layout, control id, event) -> sound`, read from the
//!   `SpuiState` of every spawned screen (`SpuiState::activations` for clicks, the HOVER
//!   bit, the visibility of a window or of the whole screen). A rule with an empty layout
//!   name holds for every layout (the in-stage options menu is one winproc in retail).
//! * **State observers**: creator palette / mode / purchase events, Sporepedia cards,
//!   the galaxy screen, the Space HUD and comm screen, for events that are not an SPUI
//!   button of an installed layout.
//! * **A player for the front end**: `NamedSounds` is decoded and played by the stage's own
//!   audio chain in every stage but the main menu; [`UiSoundsPlugin`] runs the same two
//!   systems there.
//!
//! Round 2 (`ui-sounds2`): several retail sound ids are not plain immediates but the two ends of
//! `neg; sbb; and MASK; add BASE` (a state picks `BASE` or `BASE + MASK`): `ui_spg_menu_open` /
//! `_close` (0x1073321, 0x10736E6), `ui_spg_tool_select` / `_deselect` (0x10793B1, 0x107A895),
//! the Sporepedia feed category expand / contract pair (0x663D29, 0x663D96) and the star map filter
//! pair (0x106B41B); the earlier immediate scan missed the first name of each pair.
//!
//! VERIFIED = the sound name, the control id and the handler are in the retail code image
//! (march2017 addresses in the comments; the handlers were decoded with the control-id
//! compare chains, `notes\front-end\dec`, `notes\creator-re\dec`). INFERRED = the name and
//! the neighbouring code say what plays, the exact control is not decoded. ADAPTED = the
//! OpenSpore trigger is a stand-in (the sound is retail).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    front_end::{self, pedia::PediaState, FrontEnd, Overlay, Tab},
    game_audio::Bus,
    spui::{
        ids::{flag, state},
        model::WinId,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
    world_objects::name_id,
};

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// What happens to the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The pointer enters the window (the HOVER bit rises).
    Hover,
    /// A click (the button's activation).
    Click,
    /// The window (control 0: the whole screen) appears.
    Open,
    /// The window (control 0: the whole screen) goes away.
    Close,
}

/// One interface sound: the control of a layout and the retail sound it plays.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// Installed layout name; empty = every layout.
    pub layout: &'static str,
    pub control: u32,
    pub trigger: Trigger,
    pub sound: &'static str,
}

const fn rule(layout: &'static str, control: u32, trigger: Trigger, sound: &'static str) -> Rule {
    Rule { layout, control, trigger, sound }
}

/// Layouts the tables name.
pub const NEW_GAME: &str = front_end::new_game::LAYOUT;
pub const STAR: &str = front_end::gge::STAR_LAYOUT;
pub const WHEEL_HUD: &str = front_end::gge::HUD_LAYOUT;
pub const EDITOR_FRAME: &str = "GlobalEditorUI";
pub const PEDIA_GRID: &str = front_end::pedia::GRID_LAYOUT;
pub const PEDIA_VERBS: &str = front_end::pedia::VERBS_LAYOUT;
pub const SPACE_MAIN: &str = crate::space_hud::MAIN_LAYOUT;

/// Sort buttons of `AssetBrowserAssetGrid` (Time Created ... Social, radio group
/// 1413496560); the two list toggles (`07CE2700/01`) only expand the list.
const SORT_BUTTONS: [u32; 18] = [
    0xC3FD_21B1, 0x1DC1_C781, 0xE6AA_9254, 0x4543_C5EC, 0x339D_2BF7, 0xA924_06A6, 0xE6B0_FDD0, 0x6DCF_5398, 0x33D2_FCF4,
    0x67DE_1872, 0x0103_B547, 0xB1A9_59B4, 0x9780_173B, 0xAB6C_82DC, 0xA7AF_2DF4, 0x2DC4_9BDB, 0xD95F_A1B1, 0xE63E_3E5C,
];

/// Every SPUI rule.
pub fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        use Trigger::*;
        let mut r: Vec<Rule> = Vec::new();
        // --- The in-stage options menu and HUD global buttons (every layout) -----------
        // VERIFIED 0xE02EE4..0xE0336E (the global UI message handler, a switch on the
        // control id): the orb 010ED852 -> `ui_global_options` (0xE03007), Quit 010F366D
        // -> `ui_global_exit_game` (0xE02F59), the pause toggle D305CA84 ->
        // `ui_global_pause` (0xE0307A), Save 010F366E -> `ui_global_save` (0xE030B9),
        // Exit 02268BA4 and Settings 043B72D0 share one block -> `ui_global_save`
        // (0xE0315D), Spore Guide 0447060A -> `ui_gge_guide` (0xE031E4). History
        // 044827B8 opens the timeline (`ui_global_timeline_open` 0xE442C0: INFERRED, the
        // timeline screens play it when they open).
        r.push(rule("", 0x010E_D852, Click, "ui_global_options"));
        r.push(rule("", 0x010F_366D, Click, "ui_global_exit_game"));
        r.push(rule("", 0xD305_CA84, Click, "ui_global_pause"));
        r.push(rule("", 0x010F_366E, Click, "ui_global_save"));
        r.push(rule("", 0x0226_8BA4, Click, "ui_global_save"));
        r.push(rule("", 0x043B_72D0, Click, "ui_global_save"));
        r.push(rule("", 0x0447_060A, Click, "ui_gge_guide"));
        r.push(rule("", 0x0448_27B8, Click, "ui_global_timeline_open"));
        // The timeline's OK button closes it (`ui_global_timeline_close` 0xE3795C: VERIFIED the
        // name, INFERRED the button).
        r.push(rule("timeline", crate::cell_hud::timeline::win::OK, Click, "ui_global_timeline_close"));
        // --- The tribe / pack list (`PosseBar`: the Creature and Tribal HUDs) ---------------
        // `ui_trg_name_select` (0xE23D8E) is played by 0xE23D30, the Select All toggle of the list
        // (it counts the members in state 2 / 4 and runs the select or the deselect callback on
        // all of them). Callers: the list's window proc slot 27 (0xE27B00, vtable 0x1480608), on the
        // Select All command 0x03DAE412 in every game mode but Tribe (0x1654C02) and Civilization
        // (0x1654C04) - the Creature pack bar - and its OnMouseUp slot (0xE24CA0), on the windows
        // 0x03DAE410 / 0x03DAE411. VERIFIED the sound and the shared handler; ADAPTED the Tribal
        // binding: the Tribal HUD hides those two edge strips (placeholders for the list length) and
        // its Select All button (0x03DAE412) stands for them.
        r.push(rule("PosseBar", 0x03DA_E412, Click, "ui_trg_name_select"));
        // --- Galaxy screen: star panels (VERIFIED 0xDF7800, Delete and Play Game) ----
        r.push(rule(STAR, 0x03C4_925E, Click, "ui_gge_game_select"));
        r.push(rule(STAR, 0x0375_408C, Click, "ui_gge_game_select"));
        // `ui_gge_make_stuff_exit` is played by the OnExit method (0xDE0F40, at 0xDE0FBB) of the
        // Make Stuff state, whose OnEnter (0xDE16C0) plays `ui_gge_make_stuff` (both in the vtable at
        // 0x147DB74): [`gge_events`] plays the pair when the Create tab opens / closes. The wheel's
        // Launch button (0xDE0E40 submits the `EditorRequest` that leaves the state) ends the state
        // too: VERIFIED the chain, the button stands for the exit because the tab stays open while the
        // editor starts.
        r.push(rule(WHEEL_HUD, front_end::creators::id::LAUNCH, Click, "ui_gge_make_stuff_exit"));
        // --- New game flow, VERIFIED 0xDF3B90..0xDF4720 (click switch) and 0xDED8E0 (hover) -
        for stage in front_end::new_game::id::STAGES {
            r.push(rule(NEW_GAME, stage, Click, "ui_gge_gamelevelaccept"));
            // INFERRED: the stage buttons roll over like the diet buttons (0xDEDB0E).
            r.push(rule(NEW_GAME, stage, Hover, "ui_gge_gamelevelrollover"));
        }
        for c in [front_end::new_game::id::FORWARD, front_end::new_game::id::BACK] {
            r.push(rule(NEW_GAME, c, Click, "ui_gge_panelarrow"));
        }
        for c in [0x0594_34C0, 0x0594_34C1, 0x0645_1450, 0x0645_1451, front_end::new_game::id::NAME_RANDOM] {
            r.push(rule(NEW_GAME, c, Click, "ui_gge_randomize"));
        }
        for c in [0x0594_61B0, 0x0594_61B1, 0x0594_61B2] {
            r.push(rule(NEW_GAME, c, Click, "ui_gge_lock"));
        }
        r.push(rule(NEW_GAME, front_end::new_game::id::GO, Click, "ui_gge_selectplayoption"));
        r.push(rule(NEW_GAME, front_end::new_game::id::GO, Hover, "ui_gge_playoptionrollover"));
        r.push(rule(NEW_GAME, front_end::new_game::id::CANCEL, Click, "ui_gge_exitnewgame"));
        for c in front_end::new_game::id::DIFFICULTY_BUTTONS.into_iter().chain(front_end::new_game::id::SPECIALTY_BUTTONS) {
            r.push(rule(NEW_GAME, c, Click, "ui_gge_options"));
        }
        // The diet buttons: roll-over VERIFIED (0xDEDACF); the click plays the panel arrow
        // sound in the same handler (0xDF4244, INFERRED which buttons).
        for c in [0x0677_5E1F, 0x0677_5E20, 0x0677_5E21, 0x0677_5E22, 0x0677_5E23] {
            r.push(rule(NEW_GAME, c, Hover, "ui_gge_gamelevelrollover"));
            r.push(rule(NEW_GAME, c, Click, "ui_gge_panelarrow"));
        }
        // --- Sporepedia ------------------------------------------------------------------
        // VERIFIED 0x64DB40 (verbs: Ban E0, Create Your Own 067A32D0, Edit F1 and
        // Outfitter F2, Share F5 -> publish, Delete F6) and 0x650300 (sort buttons,
        // Create Your Own card 14580F81).
        let pid = front_end::pedia::id::VERB_BAN;
        r.push(rule(PEDIA_VERBS, pid, Click, "spd_verb_ban_click"));
        r.push(rule(PEDIA_VERBS, front_end::pedia::id::VERB_CREATE_OWN, Click, "spd_makenew_click"));
        r.push(rule(PEDIA_VERBS, front_end::pedia::id::VERB_EDIT, Click, "spd_verb_edit_click"));
        r.push(rule(PEDIA_VERBS, front_end::pedia::id::VERB_OUTFITTER, Click, "spd_verb_edit_click"));
        r.push(rule(PEDIA_VERBS, front_end::pedia::id::VERB_SHARE, Click, "spd_verb_publish_click"));
        r.push(rule(PEDIA_VERBS, front_end::pedia::id::VERB_DELETE, Click, "spd_verb_delete_click"));
        r.push(rule(PEDIA_GRID, front_end::pedia::id::CREATE_OWN, Click, "spd_makenew_click"));
        for c in SORT_BUTTONS {
            r.push(rule(PEDIA_GRID, c, Click, "spd_sort_click"));
        }
        // INFERRED from the names (0x663D32 contract, 0x667B12 item): the category row's
        // expand / collapse arrow and its label.
        r.push(rule("AssetBrowserFeedListCategory", front_end::pedia::id::CAT_TOGGLE, Click, "spd_feedcategory_contract"));
        r.push(rule("AssetBrowserFeedListCategory", front_end::pedia::id::CAT_CLICK, Click, "spd_feeditem_click"));
        // --- Editors: the frame of every creator (`GlobalEditorUI`) -----------------------
        // VERIFIED 0x5E0040: Undo B006EF6E -> `ui_ed_undo` (0xC355901A), Redo F006EFA5 ->
        // `ui_ed_redo`; 0x5DFD40 (the command handler) plays `editor_general_click` for
        // Sporepedia (0x100), New (0x104), Settings (0x106), Spore Guide (0x108), the options
        // toggle (0x10D); accept (0x102, 0x5DFB80) plays `editor_check` (0x5DFBC7).
        r.push(rule(EDITOR_FRAME, 0xB006_EF6E, Click, "ui_ed_undo"));
        r.push(rule(EDITOR_FRAME, 0xF006_EFA5, Click, "ui_ed_redo"));
        for c in [0x058A_3350, 0x058A_3352, 0x0615_FF74, 0x0615_FF75] {
            r.push(rule(EDITOR_FRAME, c, Click, "editor_check"));
        }
        for c in [
            0x05B6_E49C, 0x0631_3DEA, 0x0476_6FF0, 0x063C_290C, 0x047B_C978, 0x05FC_FEE8, 0x047B_C8D8, 0x0595_E0A8,
            // INFERRED: Cancel (0x103) and the options-panel Exit / Quit share the handler.
            0x9043_9D7C, 0x055F_DCA8, 0x047B_C958, 0x05B6_E484, 0x05D0_EF68, 0x05D0_EF69, 0x0612_EFEA,
        ] {
            r.push(rule(EDITOR_FRAME, c, Click, "editor_general_click"));
        }
        // Test Drive: "Look at me" (0x63AD78 `ui_ed_whistle`, 0x0044EEC00 in
        // `PlayModeActorModule_HandleUIButton`).
        r.push(rule("playmode", 0x044E_EC00, Click, "ui_ed_whistle"));
        // --- Space HUD (`GlobalUISpace2-1`) ------------------------------------------------
        // `ui_space_filter_click` 0x1072F0F: the star-map filter buttons (INFERRED).
        for c in [0x02E1_B6F8, 0x02E1_CBD7, 0x02E1_B708, 0x02E1_B720, 0x02E1_B72C, 0x02E1_B73C] {
            r.push(rule(SPACE_MAIN, c, Click, "ui_space_filter_click"));
        }
        // The badge dialog: 0x106ADD0 (open, `ui_badge_dialog_open` at 0x106AE5D) and 0x106B660
        // (close, `ui_badge_dialog_close` at 0x106B7D2) both look up `GlobalUI_FindWindowByID(0x664C270)`
        // (0x106AE12 / 0x106B679): the window of this layout, `space_hud::id::REWARD`. VERIFIED the window,
        // the sound and the moment (the window appearing / going away); OpenSpore's reason for showing it
        // (a tool reward, `space_hud::reward`) is that module's ADAPTED item.
        r.push(rule(SPACE_MAIN, crate::space_hud::id::REWARD, Open, "ui_badge_dialog_open"));
        r.push(rule(SPACE_MAIN, crate::space_hud::id::REWARD, Close, "ui_badge_dialog_close"));
        // The tool palette's own controls: the handler 0x1072D70 of the Space HUD compares the control id
        // of every click (message 0x287259F6, `notes\space-re\dec\SPUI_1072D70.c`). The Terraform /
        // Minimap toggle 04359752 shares the tail 0x107323D with the palette tabs: `ui_spg_menu_category`
        // (0x1073243). The two page arrows 05F72D8A / 05F72D7C (their command ids are the same) play
        // `ui_menu_scroll` and turn the page in the palette handler 0x1079290 (0x10792EA, 0x1079352);
        // the same sound plays for the page hot keys (0x107A939, 0x107A960).
        r.push(rule(SPACE_MAIN, crate::space_hud::world::pid::MODE_TOGGLE, Click, "ui_spg_menu_category"));
        r.push(rule(SPACE_MAIN, crate::space_hud::palette::pid::PAGE_PREV, Click, "ui_menu_scroll"));
        r.push(rule(SPACE_MAIN, crate::space_hud::palette::pid::PAGE_NEXT, Click, "ui_menu_scroll"));
        r
    })
}

/// True when a retail click sound is attached to the control (in any layout): the generic
/// `UiClick` stand-in of the front-end screens stays silent for it.
pub fn has_retail_click(control: u32) -> bool {
    rules().iter().any(|r| r.control == control && r.trigger == Trigger::Click)
}

/// Every sound name this module can play (the prefetch list and the install test).
pub fn all_names() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = rules().iter().map(|r| r.sound).collect();
    out.extend(STATE_SOUNDS);
    out.sort_unstable();
    out.dedup();
    out
}

/// Sounds the state observers play (not in the SPUI rule table).
const STATE_SOUNDS: [&str; 39] = [
    // Editors
    "editor_category_switch",
    "editor_part_rollover",
    "editor_part_select",
    "editor_mode_switch",
    "editor_partbuy",
    "editor_cantafford",
    "ui_complexity_limit_appear",
    "editor_part_unlock",
    "editor_panel_slide",
    "editor_shopping_pageturn",
    "ui_ed_colorselect",
    "ui_ed_patternselect",
    "ui_ed_magicpaint",
    "editor_color_popup",
    "editor_handles_appear_hard",
    // Galaxy screen
    "ui_gge_menu_zoom",
    "ui_gge_menu_zoom_saved",
    "ui_gge_planet_appear",
    "ui_gge_make_stuff",
    "ui_gge_typeplanetname",
    // Sporepedia
    "spd_launch",
    "spd_exit",
    "spd_assetcard_rollover",
    "spd_assetcard_click",
    "spd_largecard_show",
    "spd_largecard_hide",
    "spd_filter_click",
    "ui_sporepedia_card_appear",
    // Space HUD
    "ui_spg_menu_open",
    "ui_spg_menu_close",
    "ui_spg_tool_select",
    "ui_spg_tool_deselect",
    "ui_spg_system_ready_for_capture",
    "ui_mycollections_open",
    "ui_mycollections_close",
    "ui_mycollections_rollover",
    "ui_mycollections_select",
    "ui_mycollections_category",
    "ui_ufo_com_rollover",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn play(sounds: &mut NamedSounds, now: f64, name: &'static str) {
    play_after(sounds, now, name, 0.0);
}

/// Queue a sound that starts `delay` seconds from now.
fn play_after(sounds: &mut NamedSounds, now: f64, name: &'static str, delay: f32) {
    let mut p = Play::named(name, "ui_sounds");
    p.bus = Bus::Ui;
    sounds.play(p, now, delay);
}

/// Decode the module's sounds once the decoder runs.
fn prefetch(mut sounds: ResMut<NamedSounds>, mut done: Local<bool>) {
    if *done || !sounds.is_running() {
        return;
    }
    *done = true;
    for name in all_names() {
        sounds.request(name_id(name), Some(name), false);
    }
}

/// Last value of a watched state (the first sight is only a baseline).
#[derive(Default, Clone, Copy, Debug)]
struct Watch<T: Copy + PartialEq>(Option<T>);

impl<T: Copy + PartialEq> Watch<T> {
    /// The previous value when `now` differs from it.
    fn changed(&mut self, now: T) -> Option<T> {
        match self.0.replace(now) {
            Some(before) if before != now => Some(before),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SPUI rules
// ---------------------------------------------------------------------------

/// A window a rule watches in one spawned screen.
struct Watched {
    rule: usize,
    win: WinId,
    was: bool,
}

/// What the observer knows about one spawned screen.
struct Screen {
    watched: Vec<Watched>,
    /// Rules of the layout (indices into [`rules`]) that hold for the whole screen.
    screen_rules: Vec<usize>,
    /// Click rules by control id.
    clicks: HashMap<u32, Vec<usize>>,
    needs_visibility: bool,
    /// The first frame of the screen only records the windows' state (a window shown from the
    /// start does not play its Open sound).
    primed: bool,
}

#[derive(Default)]
struct Screens(HashMap<Entity, Screen>);

fn spui_events(
    time: Res<Time<Real>>,
    assets: Res<SpuiAssets>,
    mut sounds: ResMut<NamedSounds>,
    states: Query<(Entity, &SpuiState)>,
    mut screens: Local<Screens>,
) {
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    let table = rules();
    let mut seen: HashSet<Entity> = HashSet::with_capacity(states.iter().len());
    for (entity, ui) in &states {
        seen.insert(entity);
        if !screens.0.contains_key(&entity) {
            let instance = assets.layout_instance(&ui.layout);
            let mut screen = Screen { watched: vec![], screen_rules: vec![], clicks: HashMap::new(), needs_visibility: false, primed: false };
            for (i, r) in table.iter().enumerate() {
                if !(r.layout.is_empty() || instance == Some(name_id(r.layout))) {
                    continue;
                }
                match (r.trigger, r.control) {
                    (Trigger::Open | Trigger::Close, 0) => screen.screen_rules.push(i),
                    (Trigger::Click, c) => screen.clicks.entry(c).or_default().push(i),
                    (trigger, c) => {
                        for win in ui.layout.find_all(c) {
                            screen.needs_visibility |= trigger != Trigger::Hover;
                            screen.watched.push(Watched { rule: i, win, was: false });
                        }
                    }
                }
            }
            // A screen that appears: its Open rules play (not for the first frame of the program's
            // already-visible screens: the sound system is not running yet then).
            if running {
                for &i in &screen.screen_rules {
                    if table[i].trigger == Trigger::Open {
                        play(&mut sounds, now, table[i].sound);
                    }
                }
            }
            screens.0.insert(entity, screen);
        }
        let screen = screens.0.get_mut(&entity).expect("inserted above");
        if !ui.ready {
            continue;
        }
        if running {
            for &win in &ui.activations {
                if let Some(list) = screen.clicks.get(&ui.layout.window(win).control_id) {
                    if let Some(&i) = list.first() {
                        play(&mut sounds, now, table[i].sound);
                    }
                }
            }
        }
        let live = screen.needs_visibility.then(|| ui.interactive());
        let primed = std::mem::replace(&mut screen.primed, true);
        for w in &mut screen.watched {
            let r = &table[w.rule];
            let on = match r.trigger {
                Trigger::Hover => ui.states[w.win] & state::HOVER != 0 && ui.flags[w.win] & flag::ENABLED != 0,
                _ => live.as_ref().is_some_and(|l| l[w.win]),
            };
            let edge = on != w.was;
            w.was = on;
            if !edge || !running || !primed {
                continue;
            }
            match (r.trigger, on) {
                (Trigger::Hover, true) | (Trigger::Open, true) | (Trigger::Close, false) => play(&mut sounds, now, r.sound),
                _ => {}
            }
        }
    }
    // Screens that went away: their Close rules play.
    let gone: Vec<Entity> = screens.0.keys().filter(|e| !seen.contains(*e)).copied().collect();
    for entity in gone {
        if let Some(screen) = screens.0.remove(&entity) {
            if running {
                for &i in &screen.screen_rules {
                    if table[i].trigger == Trigger::Close {
                        play(&mut sounds, now, table[i].sound);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Galaxy screen
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GgeMemory {
    selected: Watch<bool>,
    new_game: Watch<bool>,
    hover: Watch<Option<usize>>,
    tab: Watch<u8>,
    name_len: Watch<usize>,
}

/// The sound a star plays when it is entered (clicked): 0xDF7AC0 (`SetEnteredEntry`) sets the star
/// panel's mode 4 for a star without a saved game (`ui_gge_menu_zoom`, 0xDF6EE9) and mode 5 for a
/// saved game (`ui_gge_menu_zoom_saved`, 0xDF6F41); the mode is chosen by the entry's saved flag
/// (+0xAC, 0xDF7BC5).
fn star_entered_sound(saved: bool) -> &'static str {
    if saved {
        "ui_gge_menu_zoom_saved"
    } else {
        "ui_gge_menu_zoom"
    }
}

fn gge_events(time: Res<Time<Real>>, fe: Res<FrontEnd>, mut sounds: ResMut<NamedSounds>, mut memory: Local<GgeMemory>) {
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    // The star panel (0xDF6D80, `SetMode`, a jump table on the mode): mode 2 / 3 (the pointer is over a
    // star without / with a saved game, set by 0xDF7BF0 while no star is entered, `[GGE+0xFC] == 0`) play
    // `ui_gge_planet_appear` (0xDF6DB5, 0xDF6E2B); mode 4 / 5 (the star is clicked: 0xDF7800 opens the
    // New Game screen or selects the saved game, then 0xDF7AC0) play the zoom sounds.
    let entered = fe.selected.is_some() || fe.new_game.is_some();
    let hover = fe.hovered.or(fe.hovered_starter);
    if memory.hover.changed(hover).is_some() && hover.is_some() && !entered && running {
        play(&mut sounds, now, "ui_gge_planet_appear");
    }
    if memory.selected.changed(fe.selected.is_some()) == Some(false) && fe.selected.is_some() && running {
        play(&mut sounds, now, star_entered_sound(true));
    }
    if memory.new_game.changed(fe.new_game.is_some()) == Some(false) && fe.new_game.is_some() && running {
        play(&mut sounds, now, star_entered_sound(false));
    }
    // The Make Stuff state (the creator wheel, ADAPTED: OpenSpore's Create tab stands for the state):
    // OnEnter 0xDE16C0 plays `ui_gge_make_stuff` (0xDE17D1), OnExit 0xDE0F40 plays `_exit` (0xDE0FBB).
    let tab = match fe.tab {
        Tab::Play => 0,
        Tab::Create => 1,
        Tab::Share => 2,
    };
    if let Some(before) = memory.tab.changed(tab) {
        if running && tab == 1 {
            play(&mut sounds, now, "ui_gge_make_stuff");
        }
        if running && before == 1 {
            play(&mut sounds, now, "ui_gge_make_stuff_exit");
        }
    }
    // 0xDF456F: a character typed into the planet name (the name field message).
    let len = fe.new_game.as_ref().map(|d| d.planet.chars().count());
    if let Some(len) = len {
        if let Some(before) = memory.name_len.changed(len) {
            if running && before.abs_diff(len) == 1 {
                play(&mut sounds, now, "ui_gge_typeplanetname");
            }
        }
    } else {
        memory.name_len.0 = None;
    }
}

// ---------------------------------------------------------------------------
// Sporepedia
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PediaMemory {
    open: Watch<bool>,
    hovered: Watch<Option<usize>>,
    detail: Watch<bool>,
    filter: Watch<u8>,
    listing: Watch<(usize, usize, u8, u8, bool)>,
}

fn pedia_events(
    time: Res<Time<Real>>,
    fe: Res<FrontEnd>,
    pedia: Res<PediaState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sounds: ResMut<NamedSounds>,
    mut memory: Local<PediaMemory>,
) {
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    let open = fe.overlay == Some(Overlay::Sporepedia);
    // 0x64A3E0 (VERIFIED): the window's shown flag selects `spd_launch` (shown) or
    // `spd_exit` (hidden).
    if let Some(was) = memory.open.changed(open) {
        if running {
            play(&mut sounds, now, if was { "spd_exit" } else { "spd_launch" });
        }
    }
    if !open {
        memory.hovered.0 = None;
        memory.detail.0 = None;
        memory.filter.0 = None;
        memory.listing.0 = None;
        return;
    }
    // Cards: roll-over / click (0x65843C, 0x658D0D, INFERRED by name) and the large card
    // (0x647CCB show, 0x64CD99 hide).
    let hovered = pedia.hovered;
    if memory.hovered.changed(hovered).is_some() && hovered.is_some() && running {
        play(&mut sounds, now, "spd_assetcard_rollover");
    }
    if hovered.is_some() && mouse.just_pressed(MouseButton::Left) && running {
        play(&mut sounds, now, "spd_assetcard_click");
    }
    if let Some(was) = memory.detail.changed(pedia.detail.is_some()) {
        if running {
            play(&mut sounds, now, if was { "spd_largecard_hide" } else { "spd_largecard_show" });
        }
    }
    // 0x6627B0: a filter button (Everything / My Creations / Maxis).
    let filter = pedia.filter as u8;
    if memory.filter.changed(filter).is_some() && running {
        play(&mut sounds, now, "spd_filter_click");
    }
    // `ui_sporepedia_card_appear` has two retail sites. 0x657330 (referenced only by 0x6590A8, the
    // animation built by 0x658E40 for a card window, itself called from the card layout 0x64F9E0 when a
    // card is shown with its pop-in) is the callback that plays it (0x657369): a card appearing, once
    // per card. 0x65AD2E sits in the card's per-frame update 0x65A950, on a pointer-distance easing
    // branch (INFERRED role). The cards pop in one per `AssetLoadInterval` (30 ms, 0x6560B0:
    // `pedia::pop_in_start`), each with its sound; the grid builds its cards when the Sporepedia opens
    // and when the listing changes (page, category, filter or sort).
    let listing = pedia.listing_key();
    let opened = memory.listing.0.is_none();
    if (memory.listing.changed(listing).is_some() || opened) && running {
        for card in 0..pedia.page_cards() {
            play_after(&mut sounds, now, "ui_sporepedia_card_appear", crate::front_end::pedia::pop_in_start(card));
        }
    }
}

// ---------------------------------------------------------------------------
// Creators
// ---------------------------------------------------------------------------

#[derive(Default)]
struct EditorMemory {
    active: Watch<bool>,
    mode: Watch<u8>,
    category: Watch<(usize, usize)>,
    hover_item: Watch<Option<u32>>,
    hover_part: Watch<Option<usize>>,
    anims_panel: Watch<bool>,
    carry: Watch<bool>,
    /// Newly unlocked parts whose palette flare already played (per session).
    announced: HashSet<u32>,
}

/// Creature, vehicle, building and UFO editors (`creature_editor`) and the Cell editor.
#[allow(clippy::too_many_arguments)]
fn editor_events(
    time: Res<Time<Real>>,
    mouse: Res<ButtonInput<MouseButton>>,
    creature: Option<Res<crate::creature_editor::CreatureEditor>>,
    cell: Option<Res<crate::cell_editor::CellEditor>>,
    cell_data: Option<Res<crate::cell_editor_data::CellEditorData>>,
    creature_data: Option<Res<crate::creature_editor_data::CreatureEditorData>>,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<crate::game::RunProgress>>,
    editor_state: Option<Res<crate::editor::EditorState>>,
    palette_layout: Option<Res<crate::creature_editor_ui::CreatureEditorUiLayout>>,
    paint: Query<(&SpuiState, &crate::cell_editor_ui::EditorRoot)>,
    mut sounds: ResMut<NamedSounds>,
    mut creature_memory: Local<EditorMemory>,
    mut cell_memory: Local<EditorMemory>,
) {
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    let pressed = mouse.just_pressed(MouseButton::Left);
    // --- Creature / rigid editors ---------------------------------------------------------
    if let Some(e) = creature.as_deref().filter(|e| e.active) {
        let m = &mut *creature_memory;
        let first = m.active.changed(true).is_none() && m.mode.0.is_none();
        if first {
            m.mode.0 = Some(e.mode as u8);
            m.category.0 = Some((e.category, e.page));
            m.hover_item.0 = Some(e.hover_item);
            m.anims_panel.0 = Some(e.play.anims_panel);
            m.announced.clear();
        }
        if running {
            // 0x5DDAAE: the mode changed (EditorUI::SetActiveMode, after the change was accepted).
            if m.mode.changed(e.mode as u8).is_some() {
                play(&mut sounds, now, "editor_mode_switch");
            }
            // 0x5C3E3D / 0x5C4247 / 0x5CB3D6: a palette category was chosen; 0x5C3B7F: a page turn.
            if let Some((before_category, before_page)) = m.category.changed((e.category, e.page)) {
                if before_category != e.category {
                    play(&mut sounds, now, "editor_category_switch");
                } else if before_page != e.page {
                    play(&mut sounds, now, "editor_shopping_pageturn");
                }
            }
            // 0x5F2AC7: the pointer enters a palette item.
            if let Some(item) = e.hover_item {
                if m.hover_item.changed(Some(item)).is_some() {
                    play(&mut sounds, now, "editor_part_rollover");
                }
            } else {
                m.hover_item.0 = Some(None);
            }
            // 0x5F4ADD / 0x5F4B07 / 0x5F4B54, one routine (VERIFIED): a click on a palette item
            // plays the complexity warning, the "cannot afford" sound or the select sound.
            if pressed && e.mode == crate::creature_editor::Mode::Build && !editor_state.as_ref().is_some_and(|s| s.dragging()) {
                if let (Some(part), Some(data), Some(originals), Some(run)) =
                    (e.hover_item, creature_data.as_deref(), originals.as_deref(), run.as_deref())
                {
                    use crate::creature_editor::Refusal;
                    match crate::creature_editor::refusal(data, e.kind, run, originals, part) {
                        None => {
                            play(&mut sounds, now, "editor_part_select");
                            play(&mut sounds, now, "editor_partbuy");
                        }
                        Some(Refusal::NoDna) => play(&mut sounds, now, "editor_cantafford"),
                        Some(Refusal::TooComplex) => play(&mut sounds, now, "ui_complexity_limit_appear"),
                        Some(_) => {}
                    }
                }
            }
            // 0x636709 .. 0x636DB5: the Test Drive panels slide (animations <-> backgrounds).
            if m.anims_panel.changed(e.play.anims_panel).is_some() && e.mode == crate::creature_editor::Mode::Play {
                play(&mut sounds, now, "editor_panel_slide");
            }
            // 0x5EEE30 (the palette item's update 0x5EED60) and 0x5F7622 (0x5F7520, the twin of the
            // second palette item class): the "just unlocked" flare of an item starts the first
            // time its window is on screen (its page is shown) and plays `editor_part_unlock`.
            // ADAPTED: one sound per frame in which newly found parts of the shown page appear
            // (the retail items each play it; two at the same instant only add gain).
            if e.mode == crate::creature_editor::Mode::Build {
                if let Some(layout) = palette_layout.as_deref() {
                    let mut fresh = false;
                    for (_, part) in &layout.items {
                        fresh |= e.new_parts.contains(part) && m.announced.insert(*part);
                    }
                    if fresh {
                        play(&mut sounds, now, "editor_part_unlock");
                    }
                }
            }
        } else {
            m.mode.0 = Some(e.mode as u8);
            m.category.0 = Some((e.category, e.page));
        }
    } else {
        *creature_memory = EditorMemory::default();
    }
    // --- Cell editor ----------------------------------------------------------------------
    if let Some(e) = cell.as_deref().filter(|e| e.active) {
        let m = &mut *cell_memory;
        if m.active.changed(true).is_none() && m.mode.0.is_none() {
            m.mode.0 = Some(e.mode as u8);
            m.hover_item.0 = Some(e.hover_item);
            m.hover_part.0 = Some(e.hover_part);
            m.carry.0 = Some(e.carry.is_some());
        }
        if running {
            if m.mode.changed(e.mode as u8).is_some() {
                play(&mut sounds, now, "editor_mode_switch");
            }
            if let Some(item) = e.hover_item {
                if m.hover_item.changed(Some(item)).is_some() {
                    play(&mut sounds, now, "editor_part_rollover");
                }
            } else {
                m.hover_item.0 = Some(None);
            }
            // 0x573D29: the pointer enters a part of the cell.
            if let Some(part) = e.hover_part {
                if m.hover_part.changed(Some(part)).is_some() {
                    play(&mut sounds, now, "editor_part_rollover");
                }
            } else {
                m.hover_part.0 = Some(None);
            }
            // The palette click (same routine as the creature editor's).
            if pressed && e.mode == crate::cell_editor::Mode::Build && e.carry.is_none() {
                if let (Some(part), Some(data), Some(run)) = (e.hover_item, cell_data.as_deref(), run.as_deref()) {
                    use crate::cell_editor::Refusal;
                    match crate::cell_editor::refusal(data, run, e.level, part) {
                        None => {
                            play(&mut sounds, now, "editor_part_select");
                            play(&mut sounds, now, "editor_partbuy");
                        }
                        Some(Refusal::NoDna) => play(&mut sounds, now, "editor_cantafford"),
                        Some(Refusal::TooComplex | Refusal::TooManyParts) => play(&mut sounds, now, "ui_complexity_limit_appear"),
                        Some(_) => {}
                    }
                }
            }
            // The paint palette: a style (pattern, 0x591AB3) or a colour swatch (0x5932D3).
            for (ui, root) in &paint {
                use crate::cell_editor_ui::EditorRoot;
                if ui.activations.is_empty() {
                    continue;
                }
                match root {
                    EditorRoot::PaintPage => play(&mut sounds, now, "ui_ed_patternselect"),
                    EditorRoot::PaintSwatches => play(&mut sounds, now, "ui_ed_colorselect"),
                    _ => {}
                }
            }
        } else {
            m.mode.0 = Some(e.mode as u8);
        }
    } else {
        *cell_memory = EditorMemory::default();
    }
}

/// A palette press counter that changed since the last frame (`before` is the previous value when it
/// differs): a press, unless the counter went back to 0 (a reset session).
fn counter_pressed(before: Option<u32>, now: u32) -> bool {
    before.is_some() && now != 0
}

/// The handles of a part appear (INFERRED trigger of `editor_handles_appear_hard`): in the rigid editors
/// the selected block changed and one is selected; in the Creature Creator the focused part changed
/// (it also settles while a session opens, so the first second is skipped).
fn handles_appeared(rigid: bool, selection_changed: bool, selected: bool, focus_changed: bool, clock: f32) -> bool {
    if rigid {
        selection_changed && selected
    } else {
        focus_changed && clock > 1.0
    }
}

#[derive(Default)]
struct EditorPaintMemory {
    active: Watch<bool>,
    styles: Watch<u32>,
    brushes: Watch<u32>,
    popup: Watch<bool>,
    selected: Watch<Option<usize>>,
    focus: Watch<u32>,
}

/// Editor sounds of the paint palettes, the colour popup and the manipulation handles
/// (helper `editor-polish`; the retail sites are in `cEditor::HandleMessage` and the rigblock):
///
/// * `ui_ed_colorselect` (0x5932D3, message `ColorPicked`): a colour swatch press in the skin paint
///   palette and in the rigid editors' paint palette (the Cell editor's is in [`editor_events`]);
/// * `ui_ed_magicpaint` (0x591834, item class `ItemBlockPaintTheme`, 0x17D37D90; 0x591A4B for the
///   second route of the same message): a Complete / Partial Styles press in the vehicle,
///   building and UFO editors. The skin theme route (`ItemSkinPaintTheme`, 0x591C65) plays nothing;
/// * `ui_ed_patternselect` (0x591AB3, `ItemBlockPaint`, 0xBD110A25): a Paintbrush paint chosen in
///   the rigid editors;
/// * `editor_color_popup` (0x5A6939, `ColorSwatchUI::Update` right after `GenerateExpansionArea`):
///   the shade panel of a Cell Creator swatch opens;
/// * `editor_handles_appear_hard` (0x43CEBA, the rigblock routine that shows the active part's
///   handles, called by `cEditor` 0x5757B0 for the active part): the handles of a newly chosen part
///   appear. INFERRED trigger: a part selected in the vehicle / building / UFO editors, the focus
///   moving to another part in the Creature Creator and the outfitters (Build mode).
///
/// Not hooked (no OpenSpore feature to attach them to): `ui_ed_eyedrop` (0x588754, the paint
/// eyedropper of `cEditor::OnMouseDown`), the Test Drive picture / video / mail tray sounds
/// (`PlayModeUI::HandleUIMessage`, its buttons are hidden here) and `editor_unsnap` (its only
/// play sites, 0xD109D2 / 0xD10C66, are the Civilization city layout slots, not the creators).
#[allow(clippy::too_many_arguments)]
fn editor_paint_events(
    time: Res<Time<Real>>,
    mut sounds: ResMut<NamedSounds>,
    creature: Option<Res<crate::creature_editor::CreatureEditor>>,
    rigid: Option<Res<crate::rigid_editor::RigidEditor>>,
    rigid_paint: Option<Res<crate::rigid_paint::RigidPaintSession>>,
    run: Option<Res<crate::game::RunProgress>>,
    cell_swatches: Option<Res<crate::cell_editor_swatch::SwatchUi>>,
    cell: Option<Res<crate::cell_editor::CellEditor>>,
    skin_roots: Query<(&SpuiState, &crate::creature_editor_paint::PaintRoot)>,
    block_roots: Query<(&SpuiState, &crate::rigid_paint::RigidPaintRoot)>,
    mut memory: Local<EditorPaintMemory>,
) {
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    let m = &mut *memory;
    // The Cell Creator's shade panel (the retail popup of `ColorSwatchUI`).
    let cell_open = cell.as_deref().is_some_and(|e| e.active);
    let popup = cell_open && cell_swatches.as_deref().is_some_and(|s| s.panel.is_some());
    if m.popup.changed(popup) == Some(false) && running {
        play(&mut sounds, now, "editor_color_popup");
    }
    let Some(e) = creature.as_deref().filter(|e| e.active) else {
        *m = EditorPaintMemory { popup: std::mem::take(&mut m.popup), ..Default::default() };
        return;
    };
    // A session's first frame is only a baseline.
    let first = m.active.changed(true).is_none() && m.styles.0.is_none();
    let styles = rigid_paint.as_deref().map_or(0, |s| s.style_presses);
    let brushes = rigid_paint.as_deref().map_or(0, |s| s.brush_presses);
    let rigid_on = e.kind.is_rigid() && rigid.as_deref().is_some_and(|r| r.active);
    let selected = rigid.as_deref().filter(|_| rigid_on).and_then(|r| r.selected);
    let focus = run.as_deref().map_or(0, |r| r.focused_part);
    let style_before = m.styles.changed(styles);
    let brush_before = m.brushes.changed(brushes);
    let selected_before = m.selected.changed(selected);
    let focus_before = m.focus.changed(focus);
    if first || !running {
        return;
    }
    if counter_pressed(style_before, styles) {
        play(&mut sounds, now, "ui_ed_magicpaint");
    }
    if counter_pressed(brush_before, brushes) {
        play(&mut sounds, now, "ui_ed_patternselect");
    }
    // Colour swatches of both palettes (`ColorPicked`).
    let swatch_press = skin_roots
        .iter()
        .any(|(ui, kind)| *kind == crate::creature_editor_paint::PaintRoot::Swatches && !ui.activations.is_empty())
        || block_roots
            .iter()
            .any(|(ui, kind)| *kind == crate::rigid_paint::RigidPaintRoot::Swatches && !ui.activations.is_empty());
    if swatch_press && e.mode == crate::creature_editor::Mode::Paint {
        play(&mut sounds, now, "ui_ed_colorselect");
    }
    if e.mode == crate::creature_editor::Mode::Build {
        if handles_appeared(rigid_on, selected_before.is_some(), selected.is_some(), focus_before.is_some(), e.clock) {
            play(&mut sounds, now, "editor_handles_appear_hard");
        }
    }
}

// ---------------------------------------------------------------------------
// Space HUD
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SpaceMemory {
    tool_selected: HashMap<Entity, bool>,
    collections: Watch<bool>,
    hovered_rows: HashMap<Entity, bool>,
    comm_hover: HashMap<Entity, Option<WinId>>,
    capture: Watch<Option<bool>>,
    palette_hidden: Watch<bool>,
    planet_hidden: Watch<bool>,
}

/// `ui_spg_system_ready_for_capture` plays when the last planet of a system surrenders: 0xC8D1F0 runs
/// after the capture progress of a planet reaches 100 (0xBE9C70, `AddEmpireInfluence`: the call at
/// 0xBE9DE0) and, when every planet of the star is held by one empire and that is the player's, plays
/// the sound (0xC8D406) and shows the first-capture callout. OpenSpore's `CapturePanel::can_capture`
/// is exactly "every colony of the system has surrendered": it rising while the panel is shown.
fn capture_became_ready(before: Option<bool>, now: Option<bool>) -> bool {
    before == Some(false) && now == Some(true)
}

/// The Hide buttons of the palette (453EF531, handler 0x1068150) and of the planet panel (0562A141,
/// 0x1068090) toggle the panel and return the toggle's state, which picks the sound at 0x1073321 /
/// 0x10736E6 (`neg; sbb; and 0x6C491D1A; add 0x98B8139D`): true = `ui_spg_menu_open`, false =
/// `ui_spg_menu_close`. The tooltip those routines set for the same state is "Hide" (string 0615FFA8)
/// when it is true, "Show" (0615FFA7) when false, so true = the panel is shown (INFERRED polarity; the
/// sound pair and the click are VERIFIED).
fn menu_sound(hidden: bool) -> &'static str {
    if hidden {
        "ui_spg_menu_close"
    } else {
        "ui_spg_menu_open"
    }
}

fn space_events(
    time: Res<Time<Real>>,
    mut sounds: ResMut<NamedSounds>,
    roots: Query<(Entity, &crate::space_hud::SpaceHudRoot, &SpuiState)>,
    hud: Option<Res<crate::space_hud::SpaceHudState>>,
    hud_ui: Option<Res<crate::space_hud::SpaceHudUi>>,
    mut memory: Local<SpaceMemory>,
) {
    use crate::space_hud::SpaceHudRoot;
    let now = time.elapsed_secs_f64();
    let running = sounds.is_running();
    let mut collections_open = false;
    let mut tool_rose = false;
    let mut tool_fell = false;
    let mut tool_clicked = false;
    let mut main_ready = false;
    for (entity, kind, ui) in &roots {
        match kind {
            SpaceHudRoot::Main if ui.ready => main_ready = true,
            SpaceHudRoot::Tool(slot) if ui.ready => {
                // 0x1079290 (the tool palette handler), the tool button events 034EF68D / 70648EAE: the
                // click toggles the tool (0x1077930) and plays `ui_spg_tool_select` when it ends up
                // selected (0x10793B8: `neg; sbb; and 0xC7A71BCF; add 0x42F52773`), `_deselect` when not;
                // the hot key twin (0x107A6E0, message 06274486) plays the same pair for an enabled
                // slot and `ui_spg_error` for a disabled one. ADAPTED: a click on a usable tool is a
                // selection (OpenSpore's planetary tools stay selected when clicked again, its action
                // tools fire); the deselect sound plays when a selected tool's button falls away
                // without another tool being chosen.
                if running && !ui.activations.is_empty() {
                    let index = hud_ui.as_deref().and_then(|u| u.slot_tools.get(*slot as usize).copied().flatten());
                    let usable = hud.as_deref().zip(index).and_then(|(h, i)| h.tools.get(i)).is_some_and(|t| t.usable);
                    tool_clicked |= usable;
                }
                let button = (0..ui.layout.windows.len()).find(|&i| ui.layout.windows[i].button().is_some());
                if let Some(id) = button {
                    let on = ui.states[id] & state::SELECTED != 0;
                    match memory.tool_selected.insert(entity, on) {
                        Some(true) if !on => tool_fell = true,
                        Some(false) if on => tool_rose = true,
                        _ => {}
                    }
                }
            }
            SpaceHudRoot::Collections | SpaceHudRoot::Category(_) => {
                collections_open |= ui.ready && ui.layout.roots.first().is_some_and(|r| ui.visible(*r));
                // 0xE19068..: rows roll over / are chosen (INFERRED by name).
                if let SpaceHudRoot::Category(_) = kind {
                    let hovered = (0..ui.layout.windows.len()).any(|i| ui.states[i] & state::HOVER != 0);
                    let before = memory.hovered_rows.insert(entity, hovered).unwrap_or(false);
                    if running && hovered && !before {
                        play(&mut sounds, now, "ui_mycollections_rollover");
                    }
                    if running && !ui.activations.is_empty() {
                        play(&mut sounds, now, "ui_mycollections_category");
                    }
                } else if running && !ui.activations.is_empty() {
                    play(&mut sounds, now, "ui_mycollections_select");
                }
            }
            SpaceHudRoot::Comm if ui.ready => {
                // 0xAE9089: a comm screen button rolls over (INFERRED by name).
                let over = (0..ui.layout.windows.len()).find(|&i| ui.states[i] & state::HOVER != 0);
                let before = memory.comm_hover.insert(entity, over).flatten();
                if running && over.is_some() && over != before {
                    play(&mut sounds, now, "ui_ufo_com_rollover");
                }
            }
            _ => {}
        }
    }
    if running && tool_clicked {
        play(&mut sounds, now, "ui_spg_tool_select");
    } else if running && tool_fell && !tool_rose {
        play(&mut sounds, now, "ui_spg_tool_deselect");
    }
    if main_ready {
        // A system's last planet surrendered (see [`capture_became_ready`]).
        let capture = hud.as_deref().and_then(|h| h.capture.as_ref()).map(|c| c.can_capture);
        if let Some(before) = memory.capture.changed(capture) {
            if running && capture_became_ready(before, capture) {
                play(&mut sounds, now, "ui_spg_system_ready_for_capture");
            }
        }
        // The Hide buttons (see [`menu_sound`]); the flags only change by those clicks.
        if let Some(ui) = hud_ui.as_deref() {
            if memory.palette_hidden.changed(ui.palette_hidden).is_some() && running {
                play(&mut sounds, now, menu_sound(ui.palette_hidden));
            }
            if memory.planet_hidden.changed(ui.planet_hidden).is_some() && running {
                play(&mut sounds, now, menu_sound(ui.planet_hidden));
            }
        }
    } else {
        // Between Space sessions the baselines start over (a new HUD begins unfolded).
        memory.capture = Watch::default();
        memory.palette_hidden = Watch::default();
        memory.planet_hidden = Watch::default();
    }
    // 0xE18EA2 / 0xE19068: My Collections opens / closes.
    if let Some(was) = memory.collections.changed(collections_open) {
        if running {
            play(&mut sounds, now, if was { "ui_mycollections_close" } else { "ui_mycollections_open" });
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct UiSoundsPlugin;

impl Plugin for UiSoundsPlugin {
    fn build(&self, app: &mut App) {
        // `NamedSounds` is decoded and played by the stage's audio chain in every stage but the main
        // menu (`cell_audio`, `creature_audio`, `anim_audio`): the observers only queue sounds; the
        // front end gets the same two systems here.
        app.init_resource::<NamedSounds>().add_systems(
            Update,
            (
                (crate::cell_audio::pump_decoder, crate::cell_audio::play_sounds)
                    .chain()
                    .run_if(in_state(GameStage::MainMenu))
                    .after(spui_events),
                (
                    prefetch,
                    spui_events.after(view::input),
                    gge_events.run_if(resource_exists::<FrontEnd>),
                    pedia_events.run_if(resource_exists::<FrontEnd>).run_if(resource_exists::<PediaState>),
                    editor_events,
                    editor_paint_events,
                    space_events.run_if(in_state(GameStage::Space)),
                ),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_name_installed_layouts_and_unique_controls() {
        let table = rules();
        assert!(table.len() > 100, "{}", table.len());
        // No rule asks the same layout and control for the same trigger twice.
        let mut seen = HashSet::new();
        for r in table {
            assert!(seen.insert((r.layout, r.control, r.trigger as u8, r.sound)), "duplicate {r:?}");
        }
        let names = all_names();
        let mut sorted = names.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn paint_presses_and_handles_follow_their_counters() {
        // A counter that moved is a press; a counter reset to 0 or one that did not move is not.
        assert!(counter_pressed(Some(3), 4));
        assert!(counter_pressed(Some(0), 1));
        assert!(!counter_pressed(Some(5), 0));
        assert!(!counter_pressed(None, 5));
        // Rigid editors: a new selection shows handles, deselecting does not.
        assert!(handles_appeared(true, true, true, false, 0.0));
        assert!(!handles_appeared(true, true, false, false, 9.0));
        assert!(!handles_appeared(true, false, true, true, 9.0));
        // Creature Creator: a focus change after the first second.
        assert!(handles_appeared(false, false, false, true, 1.5));
        assert!(!handles_appeared(false, false, false, true, 0.5));
        assert!(!handles_appeared(false, true, true, false, 9.0));
    }

    #[test]
    fn watch_reports_changes_only() {
        let mut w = Watch::<u8>::default();
        assert_eq!(w.changed(1), None, "the first value is a baseline");
        assert_eq!(w.changed(1), None);
        assert_eq!(w.changed(2), Some(1));
    }

    /// A one-window-per-control layout: window 0 is the root, the others its buttons.
    fn screen(controls: &[u32]) -> SpuiState {
        use crate::spui::{
            ids::class,
            model::{ButtonData, Extra, Layout, Window, WindowKind},
            solve::RootPolicy,
        };
        let mut windows = vec![crate::cell_editor_ui::plain(0, None, [0.0, 0.0, 100.0, 100.0], None, 0xFFFF_FFFF)];
        for (i, control) in controls.iter().enumerate() {
            windows.push(Window {
                class: class::BUTTON,
                kind: WindowKind::Button,
                flags: flag::VISIBLE | flag::ENABLED,
                control_id: *control,
                extra: Extra::Button(ButtonData::default()),
                ..crate::cell_editor_ui::plain(i + 1, Some(0), [0.0, 0.0, 10.0, 10.0], None, 0xFFFF_FFFF)
            });
            windows[0].children.push(i + 1);
        }
        let layout = Layout { version: 0, windows, roots: vec![0], hit_masks: vec![], warnings: vec![], unknown_classes: vec![] };
        let mut ui = SpuiState::new(std::sync::Arc::new(layout), crate::spui::solve::RootPolicy::Fit);
        ui.ready = true;
        let _ = RootPolicy::Fit;
        ui
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<SpuiAssets>().init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.add_systems(Update, spui_events);
        app
    }

    fn queued(app: &App) -> Vec<u32> {
        app.world().resource::<NamedSounds>().queued()
    }

    /// A click on a control plays its retail sound in any layout (the global menu rules).
    #[test]
    fn global_menu_clicks_queue_their_sounds() {
        let mut app = app();
        let e = app.world_mut().spawn(screen(&[0x010E_D852, 0x0447_060A, 0x1234_5678])).id();
        app.update();
        assert!(queued(&app).is_empty());
        for (win, name) in [(1, "ui_global_options"), (2, "ui_gge_guide")] {
            app.world_mut().get_mut::<SpuiState>(e).unwrap().activations = vec![win];
            app.update();
            assert_eq!(queued(&app).last(), Some(&name_id(name)), "{name}");
        }
        let before = queued(&app).len();
        app.world_mut().get_mut::<SpuiState>(e).unwrap().activations = vec![3];
        app.update();
        assert_eq!(queued(&app).len(), before, "an unknown control stays silent");
    }

    /// Hover and visibility rules of a named layout.
    #[test]
    fn layout_rules_follow_hover_and_visibility() {
        let mut app = app();
        let go = front_end::new_game::id::GO;
        let ui = screen(&[go, crate::space_hud::id::CAPTURE_PANEL]);
        let layout = ui.layout.clone();
        app.world_mut().resource_mut::<SpuiAssets>().insert_layout(name_id(NEW_GAME), layout);
        let e = app.world_mut().spawn(ui).id();
        app.update();
        app.update();
        assert!(queued(&app).is_empty(), "{:?}", queued(&app));
        // The pointer enters the Go button.
        app.world_mut().get_mut::<SpuiState>(e).unwrap().states[1] |= state::HOVER;
        app.update();
        assert_eq!(queued(&app), vec![name_id("ui_gge_playoptionrollover")]);
        // Staying over it plays nothing more; a click plays the select sound.
        app.update();
        assert_eq!(queued(&app).len(), 1);
        app.world_mut().get_mut::<SpuiState>(e).unwrap().activations = vec![1];
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("ui_gge_selectplayoption")));
    }

    /// The badge dialog's window plays when it appears / vanishes.
    #[test]
    fn window_visibility_rules_play_on_the_edge() {
        let mut app = app();
        let mut ui = screen(&[crate::space_hud::id::REWARD]);
        ui.flags[1] &= !flag::VISIBLE;
        app.world_mut().resource_mut::<SpuiAssets>().insert_layout(name_id(SPACE_MAIN), ui.layout.clone());
        let e = app.world_mut().spawn(ui).id();
        app.update();
        app.update();
        assert!(queued(&app).is_empty());
        app.world_mut().get_mut::<SpuiState>(e).unwrap().flags[1] |= flag::VISIBLE;
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("ui_badge_dialog_open")));
        app.world_mut().get_mut::<SpuiState>(e).unwrap().flags[1] &= !flag::VISIBLE;
        app.update();
        assert_eq!(queued(&app).last(), Some(&name_id("ui_badge_dialog_close")));
    }

    /// The palette's page arrows and the Terraform / Minimap toggle play their retail sounds.
    #[test]
    fn space_palette_controls_click_with_retail_sounds() {
        let mut app = app();
        let ui = screen(&[
            crate::space_hud::palette::pid::PAGE_NEXT,
            crate::space_hud::palette::pid::PAGE_PREV,
            crate::space_hud::world::pid::MODE_TOGGLE,
        ]);
        app.world_mut().resource_mut::<SpuiAssets>().insert_layout(name_id(SPACE_MAIN), ui.layout.clone());
        let e = app.world_mut().spawn(ui).id();
        app.update();
        for (win, name) in [(1, "ui_menu_scroll"), (2, "ui_menu_scroll"), (3, "ui_spg_menu_category")] {
            app.world_mut().get_mut::<SpuiState>(e).unwrap().activations = vec![win];
            app.update();
            assert_eq!(queued(&app).last(), Some(&name_id(name)), "{name}");
        }
    }

    /// The decoded pairs: Hide toggles (open when shown), the zoom sound by the saved flag, the capture edge.
    #[test]
    fn decoded_sound_pairs_follow_their_state() {
        assert_eq!(menu_sound(false), "ui_spg_menu_open");
        assert_eq!(menu_sound(true), "ui_spg_menu_close");
        assert_eq!(star_entered_sound(false), "ui_gge_menu_zoom");
        assert_eq!(star_entered_sound(true), "ui_gge_menu_zoom_saved");
        assert!(capture_became_ready(Some(false), Some(true)));
        assert!(!capture_became_ready(None, Some(true)), "a panel that opens already complete is not a surrender");
        assert!(!capture_became_ready(Some(true), Some(true)));
        assert!(!capture_became_ready(Some(true), Some(false)));
    }

    #[test]
    fn retail_click_sounds_are_known_to_the_front_end() {
        assert!(has_retail_click(front_end::new_game::id::GO));
        assert!(has_retail_click(front_end::pedia::id::VERB_DELETE));
        assert!(!has_retail_click(0xDEAD_BEEF));
    }

    /// Every sound the module plays is installed.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_names_resolve() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let missing: Vec<_> = all_names().into_iter().filter(|n| library.resolve(n).is_err()).collect();
        assert!(missing.is_empty(), "not installed: {missing:?}");
    }
}
