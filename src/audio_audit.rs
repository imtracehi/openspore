//! Retail sounds the code of the stages plays that OpenSpore did not (helper
//! `audio-audit`, `notes\audio-audit.md`).
//!
//! The sound names come from a scan of the retail code image (`.text` / `.rdata`: every
//! 4-byte immediate or table entry that equals the FNV-1 id of an installed `soundProp` /
//! SNR, and every string that names one) and of the installed property files. Each hook
//! below is a read-only observer of OpenSpore's stage state that queues the sound on
//! `cell_audio::NamedSounds` (the retail soundProp: gain, pitch, polyphony, start delay).
//!
//! VERIFIED (name + site in the code image, march2017 addresses; the sound exists):
//! * Creature: `sfx_creature_stealth` / `sfx_creature_unstealth` (0xC1B0C3, 0xC1B167) when
//!   the creature enters / leaves stealth.
//! * Civilization: `sfx_civ_losecity` (0xBE8DD3). The vehicle voices (select, stance, take
//!   fire, orders) moved to `civ_voices` with the decoded readers of the id tables.
//! * Tribal: `sfx_trg_gift_start` (0xDBBC70) and `music_gifting` (0xC239CE, 0xDBBBFC) in the
//!   same routine.
//! * Space: `ui_camera_zoom_in` (0x101AF23, the counterpart of `ui_camera_zoom_out` at
//!   0x101AF84), `ui_space_alert` (the alert definitions `IncomingTransmission` ... of the
//!   installed `alerts` property files name it), `ui_spg_tool_rollover` /
//!   `ui_spg_tool_disabled_rollover` (0x106FFDC, 0x1070149) and `ui_spg_menu_category`
//!   (0x1073244, 0x1075075).
//!
//! Round 3 (`audio-round3`, `notes\audio-audit.md` section 8):
//! * Space: the terraform slots of the planet panel (command 0x037E9018, the 18 windows
//!   `world::pid::ROW_SLOTS`). Hover proc 0x106FCC0: a slot that holds a species item plays
//!   `ui_spg_foodweb_rollover` (0x106FE5B, id 0xD0840DB7) before its tooltip. Click proc 0x1072D70:
//!   the slot toggles the species the terraforming manager holds for its kind (plant 0xBBC5D0 / animal
//!   0xBBC580; the current one is read with 0xBBC660 / 0xBBC680): the slot's species already selected is
//!   cleared (`ui_spg_foodweb_deselect`, 0x1073055, id 0x53ECA8BD), any other becomes the selection
//!   (`ui_spg_foodweb_select`, 0x10730B7, id 0x3C7C63BC).
//!
//! ADAPTED (the trigger is not traced to its call site, the sound is retail):
//! * Space: the tool roll-over plays when the pointer enters a palette tool button, the
//!   category sound when a palette tab becomes the selected one; the alert plays when a
//!   transmission arrives in the inbox.
//! * Tribal: the gift sounds play when a gift command is issued.
//!
//! Every name is checked against the installed audio by the ignored test
//! `installed_names_resolve`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    creature::mobility::CreatureMobility,
    game::RunProgress,
    game_audio::Bus,
    space_hud::{world, SpaceHudRoot},
    spui::{
        ids::{flag, state},
        view::SpuiState,
    },
    stages::GameStage,
    tribal_controls::{CitizenAction, TribalControls},
};

// ---------------------------------------------------------------------------
// Original data: the names
// ---------------------------------------------------------------------------

pub const STEALTH: &str = "sfx_creature_stealth";
pub const UNSTEALTH: &str = "sfx_creature_unstealth";
pub const GIFT_START: &str = "sfx_trg_gift_start";
pub const GIFT_MUSIC: &str = "music_gifting";
pub const ZOOM_IN: &str = "ui_camera_zoom_in";
pub const SPACE_ALERT: &str = "ui_space_alert";
pub const TOOL_ROLLOVER: &str = "ui_spg_tool_rollover";
pub const TOOL_DISABLED_ROLLOVER: &str = "ui_spg_tool_disabled_rollover";
pub const MENU_CATEGORY: &str = "ui_spg_menu_category";
pub const LOSE_CITY: &str = "sfx_civ_losecity";
pub const FOODWEB_ROLLOVER: &str = "ui_spg_foodweb_rollover";
pub const FOODWEB_SELECT: &str = "ui_spg_foodweb_select";
pub const FOODWEB_DESELECT: &str = "ui_spg_foodweb_deselect";

/// Every sound name this module can play (the prefetch list and the install test).
pub fn all_names() -> Vec<&'static str> {
    vec![
        STEALTH,
        UNSTEALTH,
        GIFT_START,
        GIFT_MUSIC,
        ZOOM_IN,
        SPACE_ALERT,
        TOOL_ROLLOVER,
        TOOL_DISABLED_ROLLOVER,
        MENU_CATEGORY,
        LOSE_CITY,
        FOODWEB_ROLLOVER,
        FOODWEB_SELECT,
        FOODWEB_DESELECT,
    ]
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// A bool that reports its changes (the first value is only remembered).
#[derive(Default, Clone, Copy, Debug)]
pub struct Edge(Option<bool>);

/// What `Edge::set` saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    None,
    Rose,
    Fell,
}

impl Edge {
    pub fn set(&mut self, now: bool) -> Change {
        match self.0.replace(now) {
            Some(before) if before != now => {
                if now {
                    Change::Rose
                } else {
                    Change::Fell
                }
            }
            _ => Change::None,
        }
    }
}

/// Minimum seconds between two plays of the same name.
#[allow(dead_code)]
#[derive(Default)]
pub struct Cooldowns(HashMap<&'static str, f64>);

impl Cooldowns {
    /// True (and starts the wait) when `name` may play at `now`.
    pub fn ready(&mut self, name: &'static str, now: f64, every: f64) -> bool {
        if self.0.get(name).is_some_and(|at| now - at < every) {
            return false;
        }
        self.0.insert(name, now);
        true
    }
}

/// The kind of a food web slot: the first three of a row are plants (0xBBC5D0), the other three
/// animals (0xBBC580); `None` when the control is no slot.
pub fn foodweb_kind(control: u32) -> Option<usize> {
    world::pid::ROW_SLOTS.iter().find_map(|row| row.iter().position(|c| *c == control)).map(|col| usize::from(col >= 3))
}

/// Pure model of a slot click (0x1072D70 -> 0x1073055 / 0x10730B7): `selected` holds the slot the
/// manager has chosen for each kind. The slot that is already the selection is cleared, any other
/// replaces it. ADAPTED: the retail manager holds a species id, OpenSpore's slots hold no species, so
/// the slot's control id stands for it.
pub fn foodweb_click(selected: &mut [Option<u32>; 2], control: u32) -> Option<&'static str> {
    let kind = foodweb_kind(control)?;
    if selected[kind] == Some(control) {
        selected[kind] = None;
        Some(FOODWEB_DESELECT)
    } else {
        selected[kind] = Some(control);
        Some(FOODWEB_SELECT)
    }
}

fn play(sounds: &mut NamedSounds, now: f64, name: &'static str, bus: Bus) {
    let mut p = Play::named(name, "audit");
    p.bus = bus;
    sounds.play(p, now, 0.0);
}

/// Decode the module's sounds once the decoder runs.
fn prefetch(mut sounds: ResMut<NamedSounds>, mut done: Local<bool>) {
    if *done || !sounds.is_running() {
        return;
    }
    *done = true;
    for name in all_names() {
        sounds.request(crate::world_objects::name_id(name), Some(name), false);
    }
}

// ---------------------------------------------------------------------------
// Creature
// ---------------------------------------------------------------------------

fn creature_audit(
    time: Res<Time<Real>>,
    mobility: Option<Res<CreatureMobility>>,
    mut sounds: ResMut<NamedSounds>,
    mut edge: Local<Edge>,
) {
    if !sounds.is_running() {
        return;
    }
    let Some(mobility) = mobility else {
        *edge = Edge::default();
        return;
    };
    let now = time.elapsed_secs_f64();
    match edge.set(mobility.sneaking) {
        Change::Rose => play(&mut sounds, now, STEALTH, Bus::Sfx),
        Change::Fell => play(&mut sounds, now, UNSTEALTH, Bus::Sfx),
        Change::None => {}
    }
}

// ---------------------------------------------------------------------------
// Tribal
// ---------------------------------------------------------------------------

fn tribal_audit(time: Res<Time<Real>>, controls: Option<Res<TribalControls>>, mut sounds: ResMut<NamedSounds>) {
    let Some(controls) = controls else { return };
    if !sounds.is_running() {
        return;
    }
    let gift = controls
        .issued
        .iter()
        .any(|i| matches!(i.action, CitizenAction::GiftFood | CitizenAction::GiftMember));
    if gift {
        let now = time.elapsed_secs_f64();
        play(&mut sounds, now, GIFT_START, Bus::Sfx);
        // With the Pd engine the gift music is the retail positional loop of the gifting
        // group (`tribal_audio::feed_raid_music`, registered at 0xDBBC02).
        if !crate::pd_audio::stage_uses_pd(&GameStage::Tribal) {
            play(&mut sounds, now, GIFT_MUSIC, Bus::Music);
        }
    }
}

// ---------------------------------------------------------------------------
// Civilization
// ---------------------------------------------------------------------------

fn civ_audit(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    mut owned: Local<Option<usize>>,
) {
    let now = time.elapsed_secs_f64();
    // A city of yours is lost.
    let count = run.civ_campaign.owned_count();
    let lost = owned.is_some_and(|before| count < before);
    *owned = Some(count);
    if sounds.is_running() && lost {
        play(&mut sounds, now, LOSE_CITY, Bus::Sfx);
    }
}

// ---------------------------------------------------------------------------
// Space
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SpaceMemory {
    inbox: Option<usize>,
    hover: HashMap<Entity, Edge>,
    tabs: HashMap<(Entity, usize), Edge>,
    slots: HashMap<(Entity, u32), Edge>,
    /// The selected plant / animal slot of the planet panel.
    species: [Option<u32>; 2],
}

/// The palette tab buttons of the main HUD (`B0000000..07`, radio buttons).
const TAB_CONTROLS: std::ops::RangeInclusive<u32> = 0xB000_0000..=0xB000_0007;

fn space_audit(
    time: Res<Time<Real>>,
    run: Res<RunProgress>,
    mut sounds: ResMut<NamedSounds>,
    roots: Query<(Entity, &SpaceHudRoot, &SpuiState)>,
    mut memory: Local<SpaceMemory>,
) {
    let now = time.elapsed_secs_f64();
    let memory = &mut *memory;
    let inbox = run.space_campaign.universe.inbox.len();
    let arrived = memory.inbox.is_some_and(|before| inbox > before);
    memory.inbox = Some(inbox);
    let running = sounds.is_running();
    if arrived && running {
        play(&mut sounds, now, SPACE_ALERT, Bus::Ui);
    }
    for (entity, kind, ui) in &roots {
        if !ui.ready {
            continue;
        }
        match kind {
            SpaceHudRoot::Tool(_) => {
                // The pointer on the tool button (any button window of the slot).
                let button = (0..ui.layout.windows.len()).find(|&i| ui.layout.windows[i].button().is_some());
                let Some(id) = button else { continue };
                let hovered = ui.states[id] & state::HOVER != 0;
                if memory.hover.entry(entity).or_default().set(hovered) == Change::Rose && running {
                    let enabled = ui.flags[id] & flag::ENABLED != 0;
                    play(&mut sounds, now, if enabled { TOOL_ROLLOVER } else { TOOL_DISABLED_ROLLOVER }, Bus::Ui);
                }
            }
            SpaceHudRoot::Main => {
                for (slot, control) in TAB_CONTROLS.enumerate() {
                    let Some(id) = ui.find(control) else { continue };
                    let on = ui.states[id] & state::SELECTED != 0;
                    if memory.tabs.entry((entity, slot)).or_default().set(on) == Change::Rose && running {
                        play(&mut sounds, now, MENU_CATEGORY, Bus::Ui);
                    }
                }
                // The terraform slots: only a slot with a species (the disc) has an item (0x1067F00).
                let filled = |id: usize| ui.image_overrides.get(id).is_some_and(|i| i.is_some());
                for control in world::pid::ROW_SLOTS.iter().flatten().copied() {
                    let Some(id) = ui.find(control) else { continue };
                    let hovered = filled(id) && ui.states[id] & state::HOVER != 0 && ui.flags[id] & flag::ENABLED != 0;
                    if memory.slots.entry((entity, control)).or_default().set(hovered) == Change::Rose && running {
                        play(&mut sounds, now, FOODWEB_ROLLOVER, Bus::Ui);
                    }
                }
                for &win in &ui.activations {
                    let control = ui.layout.window(win).control_id;
                    if filled(win) {
                        if let Some(name) = foodweb_click(&mut memory.species, control) {
                            if running {
                                play(&mut sounds, now, name, Bus::Ui);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct AuditAudioPlugin;

impl Plugin for AuditAudioPlugin {
    fn build(&self, app: &mut App) {
        // `NamedSounds` is decoded and played by the stage's own audio chain
        // (`creature_audio`, `anim_audio`): the observers only queue sounds.
        app.add_systems(
            Update,
            (
                prefetch,
                creature_audit.run_if(in_state(GameStage::Creature)),
                tribal_audit.run_if(in_state(GameStage::Tribal)),
                civ_audit.run_if(in_state(GameStage::Civilization)),
                space_audit.run_if(in_state(GameStage::Space)),
            )
                .run_if(resource_exists::<RunProgress>)
                .run_if(resource_exists::<NamedSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_report_changes_only() {
        let mut e = Edge::default();
        assert_eq!(e.set(true), Change::None, "the first value is a baseline");
        assert_eq!(e.set(true), Change::None);
        assert_eq!(e.set(false), Change::Fell);
        assert_eq!(e.set(true), Change::Rose);
    }

    #[test]
    fn cooldowns_hold_a_name_back() {
        let mut c = Cooldowns::default();
        assert!(c.ready("a", 10.0, 3.0));
        assert!(!c.ready("a", 12.0, 3.0));
        assert!(c.ready("b", 12.0, 3.0));
        assert!(c.ready("a", 13.1, 3.0));
    }

    /// The food web slots of the planet panel: kinds by column, a second click clears.
    #[test]
    fn food_web_slots_select_and_deselect_by_kind() {
        let [plant_a, plant_b, _, herb_a, _, carn] = world::pid::ROW_SLOTS[0];
        let plant_c = world::pid::ROW_SLOTS[2][0];
        assert_eq!(foodweb_kind(plant_a), Some(0));
        assert_eq!(foodweb_kind(herb_a), Some(1));
        assert_eq!(foodweb_kind(carn), Some(1));
        assert_eq!(foodweb_kind(world::pid::HIDE), None);
        let mut selected = [None; 2];
        assert_eq!(foodweb_click(&mut selected, plant_a), Some(FOODWEB_SELECT));
        // Another plant replaces the selection (0xBBC5D0 sets the manager's plant).
        assert_eq!(foodweb_click(&mut selected, plant_b), Some(FOODWEB_SELECT));
        assert_eq!(foodweb_click(&mut selected, plant_b), Some(FOODWEB_DESELECT));
        assert_eq!(foodweb_click(&mut selected, plant_c), Some(FOODWEB_SELECT));
        // The animal selection is independent.
        assert_eq!(foodweb_click(&mut selected, herb_a), Some(FOODWEB_SELECT));
        assert_eq!(selected, [Some(plant_c), Some(herb_a)]);
        assert_eq!(foodweb_click(&mut selected, world::pid::HIDE), None);
    }

    #[test]
    fn module_names_are_unique() {
        let names = all_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no duplicates");
    }

    /// The creature's stealth, the tribal gift and the Space inbox queue their sounds
    /// (headless; a decoder-less `NamedSounds` only queues).
    #[test]
    fn stage_changes_queue_their_sounds() {
        use crate::world_objects::name_id;
        let queued = |app: &App| app.world().resource::<NamedSounds>().queued();
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.init_resource::<NamedSounds>();
        app.world_mut().resource_mut::<NamedSounds>().pretend_running();
        app.init_resource::<CreatureMobility>();
        app.init_resource::<TribalControls>();
        app.add_systems(Update, (creature_audit.run_if(in_state(GameStage::Creature)), tribal_audit.run_if(in_state(GameStage::Tribal))));
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        for _ in 0..4 {
            app.update();
        }
        assert!(queued(&app).is_empty(), "{:?}", queued(&app));
        app.world_mut().resource_mut::<CreatureMobility>().sneaking = true;
        app.update();
        assert_eq!(queued(&app), vec![name_id(STEALTH)]);
        app.world_mut().resource_mut::<CreatureMobility>().sneaking = false;
        app.update();
        assert_eq!(queued(&app), vec![name_id(STEALTH), name_id(UNSTEALTH)]);
        // A gift command in the Tribal stage.
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        for _ in 0..3 {
            app.update();
        }
        app.world_mut().resource_mut::<TribalControls>().issued.push(crate::tribal_controls::Issued {
            action: CitizenAction::GiftFood,
            count: 1,
            point: Vec3::ZERO,
        });
        app.update();
        let q = queued(&app);
        // The gift music is the Pd engine's positional loop when the engine plays the stage.
        let pd = crate::pd_audio::stage_uses_pd(&GameStage::Tribal);
        assert!(q.contains(&name_id(GIFT_START)) && q.contains(&name_id(GIFT_MUSIC)) != pd, "{q:?}");
    }

    /// Every name the module plays is installed.
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
