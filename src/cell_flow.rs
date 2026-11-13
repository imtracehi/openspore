//! Cell-stage flow (`notes\cell-re-notes.md` sections 4B, 6, 8): hatching at
//! level start and after the editor or a death, the mate button and its
//! countdown, the Mating Call (mate spawned just outside the visible box),
//! the mate trigger and mating sequence (Eater NPCs flee meanwhile),
//! GoToEditor (egg, black screen, editor with DNA = earned - spent), the
//! player's death and the level re-initialisation that keeps the food
//! progression, the Advance button at food 1000, and the HUD outputs.
//!
//! Original values: mateSpawnDistance / mateTriggerDistance (4 / 4); mate
//! call event 8 s; mate button after the first meal past 149 food + 6 s, or
//! 16 s after the first part pickup; anim request codes 67 (call), 65 / 66
//! (mating pair), 41 (go to editor), 43 (hatch); effects
//! `cell_egg_L0_autohatch`, `black_screen`; the HUD bar = food / 1000 in five
//! 200-food segments; DNA shown = earned - mEvolutionPointsSpent.
//! Event durations are the clip lengths `PlayAnimation` returned when the
//! event was created (RE 10F): [`cell_anim::CellClipTimes`] answers them per
//! request code, so every phase is as long as its retail clip x the
//! `anim_dilation` factor (hatch 3.33 s x 0.75, intro 7.43 s x 0.75, the
//! mating clip 6.67 s x 0.75, ...). The `*_SECONDS` constants are only the
//! fallback while the clip table is not loaded (no installed clips).
//! "Tutorials on" reads Settings > Game > Tutorials (option 0x04EA96CB).
//!
//! ADAPTED: the visible box uses the default perspective; the skip / dismiss
//! keys and the Advance button (F4) are bound to keys (HUD buttons press keys
//! in OpenSpore; the retail skip message 0xE7F855 -> 0xE7F5A0 has no decoded
//! key binding).

use bevy::prelude::*;

use crate::{
    cell_contract::{CellAnimState, CellAnimation, CellEffectRequest, CellGoalCard, CellHudState, CellRollover},
    cell_data::{CellDatabase, CellScale, LootEntryType},
    cell_engine::{
        anim_code, heading_of, player_scale, request_anim, CellGame, CellKind, CellMateObject, CellObject,
        CellSpawnQueue, SpawnRequest, SpawnRole, STAGE_FOOD, UNIT,
    },
    cell_rules::dna_earned,
    game::{Player, RunProgress},
    stages::GameStage,
};

/// HUD button keys (OpenSpore HUD buttons press keys).
pub const MATE_KEY: KeyCode = KeyCode::F2;
/// Skips the intro / death cinematics (0xE7F5A0). ADAPTED key.
pub const SKIP_KEY: KeyCode = KeyCode::Space;
pub const ADVANCE_KEY: KeyCode = KeyCode::F4;
/// Dismisses the front callout / closes the summary window. ADAPTED key.
pub const DISMISS_KEY: KeyCode = KeyCode::Enter;
/// "Tutorials on" (config option 0x04EA96CB, Settings > Game > Tutorials): the
/// opening / ending pauses and callouts only run while it is on (0xE7CBB0,
/// 0xE53C00; 0xE50430 runs a callout's callback at once when it is off).
pub fn tutorials_on() -> bool {
    crate::front_end::settings::tutorials_enabled()
}
/// First-minute hints after 15 s without steering / eating (RE 10B).
pub const HINT_SECONDS: f32 = 15.0;
/// Events 0x31 / 0x30: before the Opening / Ending callouts (RE 10F).
pub const OPENING_PAUSE_SECONDS: f32 = 0.75;
pub const ENDING_PAUSE_SECONDS: f32 = 0.5;
/// Clip-length events (RE 10F), fallback durations used until the clip table
/// is loaded: the clips of 0x2C (HatchIce AAAA0069), 0x44 (BrainUpgrade
/// AAAA0124), 0x2E (MovToBeach AAAA0081) times the player's 0.75.
pub const INTRO_SECONDS: f32 = 4.0;
pub const ENDING_SECONDS: f32 = 4.0;
pub const LEAVE_SECONDS: f32 = 3.0;
/// DNA bonus handed to the Creature stage (RE 10C step 7).
pub const ADVANCE_DNA_BONUS: i32 = 50;
/// How long one frame's lock of the avatar holds: retail's event updates set the
/// lock bytes every frame and nothing clears them but the next frame's rules, so
/// the lock is renewed each frame; this window only has to cover one frame.
const SEQUENCE_LOCK: f32 = 0.1;
/// Fallback event durations (retail times them by the clip, see the module doc).
pub const HATCH_SECONDS: f32 = 2.0;
pub const MATING_SECONDS: f32 = 3.0;
pub const GO_TO_EDITOR_SECONDS: f32 = 2.5;
pub const DEATH_SECONDS: f32 = 2.0;
/// Mate call event (0x2E): the mate is "called" for 8 s.
pub const MATE_CALL_SECONDS: f32 = 8.0;
/// Mate button delays (cCellGame+0x51D0): after a meal past 149 food / after
/// the first part pickup.
pub const MATE_COUNTDOWN_MEAL: f32 = 6.0;
pub const MATE_COUNTDOWN_UNLOCK: f32 = 16.0;
/// Number of parts the "Collect" goal counts (tutorial mission 1: 6).
pub const COLLECT_GOAL: usize = 6;
/// HUD progress bar segments of 200 food (RE 4B).
pub const BAR_SEGMENTS: usize = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlowPhase {
    /// New stage (food 0): the intro (event 0x1B, request 0x2C HatchIce).
    Intro,
    /// Event 0x31 before the Opening callouts.
    OpeningPause,
    /// The avatar hatches (level start, after the editor, after a death).
    Hatching,
    #[default]
    Play,
    /// Mating sequence (event 0x2D).
    Mating,
    /// GoToEditor event (0x1E).
    GoToEditor,
    /// The Cell Creator is open.
    Editing,
    Dying,
    /// The `cell_ending` cinematic (event 0x2F, request 0x44).
    Ending,
    /// Event 0x30 before the Ending callout.
    EndingPause,
    /// Advance clicked: the summary window (HUD hidden, paused).
    Summary,
    /// Leaving the stage (event 0x1F, request 0x2E MovToBeach, letterbox).
    LeaveStage,
    /// The Creature stage takes over.
    Advancing,
}

#[derive(Resource, Default)]
pub struct CellFlow {
    pub phase: FlowPhase,
    pub timer: f32,
    /// The called mate (cCellGame+0x51D4) and its call timer.
    pub mate: Option<Entity>,
    pub mate_called: f32,
    /// Mate button countdown (cCellGame+0x51D0).
    pub mate_countdown: f32,
    pub egg: Option<Entity>,
    pub bitten: bool,
    /// DNA handed to the editor when it opened.
    pub editor_dna: f32,
    /// DNA earned at the last creator visit (cCellGame serializable
    /// `field_30`, written on the editor return 0xE809A0 as `0xE51410(food)`),
    /// saved with the game (`CellSave::dna_at_creator`). A save that predates the
    /// field starts at the earned DNA of a game that has already spent some (the
    /// creator was visited), else at 0.
    pub dna_at_creator: Option<i32>,
    /// The population must be re-initialised (death).
    pub reset_level: bool,
    /// The avatar's current decoration effect.
    pub decoration: Option<u32>,
    /// Callouts shown, front first; the game is paused while any is shown.
    pub callouts: Vec<String>,
    /// The simulation is paused (callouts, the summary window).
    pub paused: bool,
    /// First-minute hint and its timers (cCellGame+0x5194+0x30/+0x34).
    pub hint: Option<&'static str>,
    pub not_moved: f32,
    pub not_eaten: f32,
    /// The ending has played: the Advance button is shown.
    pub ending_done: bool,
    pub summary: Option<crate::cell_contract::CellSummary>,
    pub summary_closed: bool,
    pub letterbox: bool,
    pub hud_hidden: bool,
    /// The intro / death cinematic was skipped.
    pub skipped: bool,
    /// The hold cinematic the flow is showing (`CLG_intro` during the intro,
    /// `cell_ending` during the ending); stopped by code at the event's end.
    pub cinematic: Option<&'static str>,
    /// Goal strings resolved from the install (title, body) by locale key.
    pub strings: std::collections::HashMap<(u32, u32), String>,
    /// The phase whose entry effect has been played (retail plays the hatch /
    /// intro effect once, when the event is created: 0xE6EB60 / 0xE6ECB0).
    pub fx_phase: Option<FlowPhase>,
    /// The phase whose event time has been taken from its clip.
    pub clip_phase: Option<FlowPhase>,
}

impl CellFlow {
    /// The flow at level start: the avatar hatches first.
    pub fn hatching() -> Self {
        Self {
            phase: FlowPhase::Hatching,
            timer: HATCH_SECONDS,
            ..default()
        }
    }
    /// LevelInit (0xE7FD00): a new stage (food 0) plays the intro, any other
    /// start hatches.
    pub fn start(first_start: bool) -> Self {
        if first_start {
            Self {
                phase: FlowPhase::Intro,
                timer: INTRO_SECONDS,
                hud_hidden: true,
                ..default()
            }
        } else {
            Self::hatching()
        }
    }
    /// Pause the game and queue callouts (0xE53640(2,1) + 0xE50430).
    pub fn callout(&mut self, names: &[&str]) {
        self.callouts.extend(names.iter().map(|n| n.to_string()));
        self.paused = !self.callouts.is_empty();
    }
    pub fn player_dead(&self) -> bool {
        self.phase == FlowPhase::Dying
    }
    pub fn kill_player(&mut self, bitten: bool) {
        self.phase = FlowPhase::Dying;
        self.timer = DEATH_SECONDS;
        self.bitten = bitten;
    }
    /// Camera zoom factor goal is forced to 1 (4C; 0xE600A0 at 0xE60802: the
    /// game-mode state 1 / 2, or `sCellUI+0x936` = `field_934[2]`, the cinematic /
    /// HUD-hidden flag: the intro, the summary, leaving the stage).
    pub fn camera_frozen(&self) -> bool {
        self.hud_hidden || matches!(self.phase, FlowPhase::Dying | FlowPhase::GoToEditor | FlowPhase::Editing)
    }
    /// The simulation runs (not paused by a callout or the summary).
    pub fn running(&self) -> bool {
        !self.paused
    }
    /// Whether the avatar is free (not in a sequence).
    pub fn player_free(&self) -> bool {
        self.phase == FlowPhase::Play
    }
}

/// The egg the cell lays before the editor.
#[derive(Component, Clone, Debug, Default)]
pub struct CellEgg {
    pub age: f32,
}

/// Locale tables of the Cell goal cards (installed `Text.package`).
const GOAL_STRINGS: [(u32, u32); 4] = [
    (0xAD56_080C, 0xF4DA_F5CB), // Unlock Cell Parts
    (0xAD56_080C, 0xF4DA_F5CA), // Hunt cells or break open meteor shards...
    (0xC015_2A6D, 0x0300_0001), // Call a mate to enter the Cell Creator!
    (0x9369_A29A, 0x0000_0001), // You have earned enough DNA to evolve...
];

fn load_strings(index: &crate::asset_catalog::ResourceIndex) -> std::collections::HashMap<(u32, u32), String> {
    let mut out = std::collections::HashMap::new();
    for (table, id) in GOAL_STRINGS {
        let mut keys: Vec<_> = index
            .records
            .keys()
            .filter(|k| k.type_id == 0x02FA_C0B6 && k.instance_id == table)
            .copied()
            .collect();
        keys.sort_by_key(|k| k.group_id);
        if let Some(text) = keys
            .into_iter()
            .filter_map(|k| index.read(k).ok())
            .flat_map(|bytes| crate::decode::parse_locale_table(&bytes))
            .find(|(text_id, _)| *text_id == id)
            .map(|(_, text)| text.trim().to_string())
        {
            out.insert((table, id), text);
        }
    }
    out
}

fn text(flow: &CellFlow, key: (u32, u32)) -> String {
    flow.strings.get(&key).cloned().unwrap_or_default()
}

/// The request whose clip times a phase's event, the seconds added after it
/// and whether it is the mate's clip (RE 10F, request codes re-read at the
/// `PlayAnimation` call sites): hatch 0x1C <- 0x2B (0xE6EBA3), intro 0x1B <-
/// 0x2C (0xE6ED0B), go to editor 0x1E <- 0x29 + 1 s (0xE7798F; RE 10F's
/// "0x2D" is a misread: the code is 41, AAAA0067), leave 0x1F <- 0x2E
/// (0xE72D84), ending 0x2F <- 0x44 (0xE6EA4D), mating 0x2D <- the mate's 0x41
/// (0xE75FD1), player death <- 7 / 9 (the avatar's death requests).
fn phase_request(flow: &CellFlow) -> Option<(i32, f32, bool)> {
    Some(match flow.phase {
        FlowPhase::Hatching => (anim_code::HATCH, 0.0, false),
        FlowPhase::Intro => (anim_code::HATCH_ICE, 0.0, false),
        FlowPhase::GoToEditor => (anim_code::GO_TO_EDITOR, 1.0, false),
        FlowPhase::LeaveStage => (anim_code::MOV_TO_BEACH, 0.0, false),
        FlowPhase::Ending => (anim_code::BRAIN_UPGRADE, 0.0, false),
        FlowPhase::Dying => (if flow.bitten { anim_code::DEATH_EATEN } else { anim_code::DEATH }, 0.0, false),
        FlowPhase::Mating => (anim_code::MATE_A, 0.0, true),
        _ => return None,
    })
}

/// Take a phase's event time from its clip the first frame it runs: the event
/// is created with `PlayAnimation`'s return value (clip length x dilation).
fn time_phase(flow: &mut CellFlow, clips: &crate::cell_anim::CellClipTimes, player: CellScale) {
    if flow.clip_phase == Some(flow.phase) {
        return;
    }
    let Some((code, extra, mate)) = phase_request(flow) else {
        flow.clip_phase = Some(flow.phase);
        return;
    };
    if !clips.is_loaded() {
        // The fallback constant runs until the table is there.
        return;
    }
    flow.clip_phase = Some(flow.phase);
    // The mate is a cell of the player's level (ratio 1), like the avatar.
    let ctx = crate::cell_anim::SelectContext { avatar: !mate, ..default() };
    let dilation = crate::cell_anim::dilation_factor(mate.then_some(player), player);
    if let Some(seconds) = clips.play_seconds(code, &ctx, dilation) {
        flow.timer = seconds + extra;
    }
}

fn effect(effects: &mut MessageWriter<CellEffectRequest>, name: &str, attach: Option<Entity>, position: Vec3, scale: f32) {
    effects.write(CellEffectRequest {
        effect: crate::world_objects::name_id(name),
        position,
        attach_to: attach,
        scale,
    });
}

/// The avatar decoration effect (RE 1E, 0xE6C330): `cell_hero_NoEyes`
/// without eyes, else `cell_hero_L<scale>_decorations` (L1 .. L400; the 1K
/// scale reuses L400).
pub fn player_decoration(scale: CellScale, eyes: u32) -> u32 {
    if eyes == 0 {
        return crate::world_objects::name_id("cell_hero_NoEyes");
    }
    const LEVELS: [&str; 10] = ["1", "2", "4", "10", "20", "40", "100", "200", "400", "400"];
    let i = (scale.index() - 1).clamp(0, 9) as usize;
    crate::world_objects::name_id(&format!("cell_hero_L{}_decorations", LEVELS[i]))
}

/// Whether the mate button shows (0xE52D60): mShowMateButton and the
/// countdown has run out.
pub fn mate_available(run: &RunProgress, flow: &CellFlow) -> bool {
    run.cell.show_mate_button && flow.mate_countdown <= 0.0 && flow.player_free() && flow.mate.is_none()
}

/// Mate spawn point (0xE53E10): where the ray from the visible-box centre
/// along `dir` leaves the box, plus `dir * mateSpawnDistance`.
pub fn mate_spawn_point(center: Vec2, half: Vec2, dir: Vec2, distance: f32) -> Vec2 {
    let tx = if dir.x.abs() > 1e-6 { half.x / dir.x.abs() } else { f32::INFINITY };
    let ty = if dir.y.abs() > 1e-6 { half.y / dir.y.abs() } else { f32::INFINITY };
    center + dir * (tx.min(ty) + distance)
}

/// HUD bar (0xE5BFA0): food / 1000 over five 200-food segments ->
/// (segment, fill within it).
pub fn bar_position(food: i32) -> (usize, f32) {
    let f = (food.clamp(0, STAGE_FOOD) as f32) / (STAGE_FOOD as f32 / BAR_SEGMENTS as f32);
    let segment = (f.floor() as usize).min(BAR_SEGMENTS - 1);
    (segment, (f - segment as f32).clamp(0.0, 1.0))
}

/// Stage flow (runs in the simulation step).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    db: Res<CellDatabase>,
    originals: Res<crate::Originals>,
    mut run: ResMut<RunProgress>,
    mut game: ResMut<CellGame>,
    mut flow: ResMut<CellFlow>,
    (mut mating, mut notice): (ResMut<crate::cell::CellMating>, ResMut<crate::cell::CellNotice>),
    (mut editor, mut next): (ResMut<crate::editor::EditorState>, ResMut<NextState<GameStage>>),
    mut queue: ResMut<CellSpawnQueue>,
    mut effects: MessageWriter<CellEffectRequest>,
    clips: Res<crate::cell_anim::CellClipTimes>,
    mut player: Query<
        (
            Entity,
            &mut Transform,
            &mut CellObject,
            &mut CellAnimState,
            Option<&mut Visibility>,
            Option<&mut crate::cell_rules::CellCombat>,
        ),
        With<Player>,
    >,
    mut mates: Query<
        (Entity, &Transform, &mut CellObject, &mut crate::cell_ai::CellAiState, &mut CellAnimState),
        (With<CellMateObject>, Without<Player>),
    >,
    eggs: Query<Entity, (With<CellEgg>, Without<Player>)>,
) {
    let dt = time.delta_secs().min(0.1);
    notice.remaining = (notice.remaining - dt).max(0.0);
    if flow.strings.is_empty() {
        flow.strings = load_strings(&originals.resource_index);
    }
    let Some(globals) = db.globals.clone() else {
        return;
    };
    let Ok((player_entity, mut pt, mut po, mut panim, mut pvis, mut pcombat)) = player.single_mut() else {
        if std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
            eprintln!("cell flow: {} players matched", player.iter().count());
        }
        return;
    };
    let here = Vec2::new(pt.translation.x, pt.translation.z);
    let food = run.cell.food_progression;
    po.scale = player_scale(food);
    // The avatar's persistent decoration effect (RE 1E / 9): re-created when
    // the player's scale or eye count changes.
    let caps = crate::cell_motion::player_caps(&run, &originals, &db);
    let decoration = player_decoration(po.scale, caps.n[11]);
    if flow.decoration != Some(decoration) {
        flow.decoration = Some(decoration);
        effects.write(CellEffectRequest {
            effect: decoration,
            position: Vec3::ZERO,
            attach_to: Some(player_entity),
            scale: 1.0,
        });
    }
    if flow.dna_at_creator.is_none() {
        flow.dna_at_creator = Some(
            run.cell
                .dna_at_creator
                .unwrap_or(if run.cell.evolution_points_spent > 0 { dna_earned(food) } else { 0 }),
        );
    }
    flow.timer -= dt;
    if flow.fx_phase.is_some_and(|p| p != flow.phase) {
        flow.fx_phase = None;
    }
    flow.mate_countdown = (flow.mate_countdown - dt).max(0.0);
    flow.mate_called = (flow.mate_called - dt).max(0.0);
    if food >= STAGE_FOOD && !mating.ready_announced {
        mating.ready_announced = true;
        notice.show(text(&flow, GOAL_STRINGS[3]));
        run.request_save();
    }
    let start_hp = db
        .cell(globals.start_cell)
        .map_or(crate::cell_rules::MAX_HP, |r| r.hp.clamp(1, crate::cell_rules::MAX_HP));
    // RE 10F: an event lasts as long as the clip of its request (hatch 0x1C,
    // intro 0x1B, go to editor 0x1E (+1 s), leave 0x1F, ending 0x2F, death
    // 5 / 7, mating 0x2D = the mate's clip), taken when the phase starts.
    time_phase(&mut flow, &clips, po.scale);
    // The per-frame updates of the sequence events lock the avatar like the leave
    // event does (VERIFIED, `notes\cell-final2-audit.md`): the opening / ending
    // callouts (types 0x31 / 0x30, 0xE569B0), the ending (0x2F, 0xE53C50), mating
    // (0x2D, 0xE75C30), go-to-editor (0x1E, 0xE598E0) and the avatar's death (0x1D,
    // 0xE591C0) set +0x17C (hit lock), +0x178 (acting lock) and, but for the death,
    // +0x17F / +0x17B / +0x188 every frame; `Damage`, EatKill, the zap search and
    // the avatar's own bites refuse a locked cell.
    if matches!(
        flow.phase,
        FlowPhase::OpeningPause | FlowPhase::Ending | FlowPhase::EndingPause | FlowPhase::Mating | FlowPhase::GoToEditor | FlowPhase::Dying
    ) {
        if let Some(c) = pcombat.as_mut() {
            c.hit_cooldown = c.hit_cooldown.max(SEQUENCE_LOCK);
            c.busy = c.busy.max(SEQUENCE_LOCK);
        }
    }
    match flow.phase {
        FlowPhase::Hatching => {
            game.locked = true;
            if let Some(v) = pvis.as_mut() {
                **v = Visibility::Inherited;
            }
            // 0xE6EB60: the effect once with the event; per frame (0xE5EC10)
            // the avatar is held (0xE5E590) in state 0x2B and flagged
            // already-hit (+0x17C: no damage).
            po.velocity = Vec2::ZERO;
            po.target = None;
            po.invulnerable = po.invulnerable.max(crate::cell_rules::HATCH_GRACE);
            if flow.fx_phase != Some(FlowPhase::Hatching) {
                flow.fx_phase = Some(FlowPhase::Hatching);
                effect(&mut effects, "cell_egg_L0_autohatch", Some(player_entity), Vec3::ZERO, 1.0);
            }
            if panim.request_code != anim_code::HATCH {
                request_anim(&mut panim, anim_code::HATCH, CellAnimation::CellHatchEgg);
            }
            if flow.timer <= 0.0 {
                // 0xE503E0: HUD back; no grace after the hatch.
                flow.phase = FlowPhase::Play;
                game.locked = false;
                request_anim(&mut panim, anim_code::IDLE, CellAnimation::CellIdle);
            }
        }
        FlowPhase::Intro => {
            // 0xE6ECB0 / 0xE5EC70: `cell_hatch_ice` on the avatar, request 0x2C
            // held at the origin, locked; skippable.
            game.locked = true;
            po.velocity = Vec2::ZERO;
            po.target = None;
            po.invulnerable = po.invulnerable.max(crate::cell_rules::HATCH_GRACE);
            if flow.fx_phase != Some(FlowPhase::Intro) {
                flow.fx_phase = Some(FlowPhase::Intro);
                effect(&mut effects, "cell_hatch_ice", Some(player_entity), Vec3::ZERO, 1.0);
            }
            if panim.request_code != anim_code::HATCH_ICE {
                request_anim(&mut panim, anim_code::HATCH_ICE, CellAnimation::CellHatchIce);
            }
            // 0xE6ECB0 also plays `CLG_intro` (letterbox hold, pause off); the
            // event's end stops it (0xE7CBB0 -> 0xAD7D40). ADAPTED: Escape on
            // the cinematic is the intro's skip (the cinematic takes the input).
            if flow.cinematic.is_none() && crate::cinematics::request("CLG_intro") {
                flow.cinematic = Some("CLG_intro");
            }
            let skipped = keys.just_pressed(SKIP_KEY) || crate::cinematics::take_skipped("CLG_intro");
            if flow.timer <= 0.0 || skipped {
                if let Some(name) = flow.cinematic.take() {
                    crate::cinematics::request_stop(name);
                }
                // 0xE7CBB0: skipped -> pause + the two callouts now; natural
                // end -> (tutorials on) event 0x31 first.
                flow.hud_hidden = false;
                request_anim(&mut panim, anim_code::IDLE, CellAnimation::CellIdle);
                if skipped || !tutorials_on() {
                    flow.phase = FlowPhase::Play;
                    game.locked = false;
                    if tutorials_on() {
                        flow.callout(&["CAllout_CLG_Opening", "CAllout_CLG_ProgressBar"]);
                    }
                } else {
                    flow.phase = FlowPhase::OpeningPause;
                    flow.timer = OPENING_PAUSE_SECONDS;
                }
            }
        }
        FlowPhase::OpeningPause => {
            game.locked = true;
            if flow.timer <= 0.0 {
                flow.phase = FlowPhase::Play;
                game.locked = false;
                flow.callout(&["CAllout_CLG_Opening", "CAllout_CLG_ProgressBar"]);
            }
        }
        FlowPhase::Ending => {
            // 0xE53C50: locked, request 0x44, camera held, Eaters flee.
            game.locked = true;
            game.escape = true;
            po.target = None;
            if panim.request_code != anim_code::BRAIN_UPGRADE {
                request_anim(&mut panim, anim_code::BRAIN_UPGRADE, CellAnimation::CellBrainUpgrade);
            }
            // 0xE6E8C0: `PlayCinematic("cell_ending")` with the event.
            if flow.cinematic.is_none() && crate::cinematics::request("cell_ending") {
                flow.cinematic = Some("cell_ending");
            }
            game.camera_hold = Some(here);
            game.camera_speed = 3.0;
            if flow.timer <= 0.0 {
                // 0xE53C00: cinematic stopped, camera released; (tutorials
                // on) event 0x30.
                if let Some(name) = flow.cinematic.take() {
                    crate::cinematics::request_stop(name);
                }
                game.camera_hold = None;
                if tutorials_on() {
                    flow.phase = FlowPhase::EndingPause;
                    flow.timer = ENDING_PAUSE_SECONDS;
                } else {
                    game.escape = false;
                    game.locked = false;
                    flow.phase = FlowPhase::Play;
                    flow.ending_done = true;
                }
            }
        }
        FlowPhase::EndingPause => {
            game.locked = true;
            game.escape = true;
            if flow.timer <= 0.0 {
                // 0xE5DAF0: pause + "Callout_CLG_Ending" (method 2); the Advance
                // button shows and play continues.
                game.escape = false;
                game.locked = false;
                flow.phase = FlowPhase::Play;
                flow.ending_done = true;
                flow.callout(&["Callout_CLG_Ending"]);
                request_anim(&mut panim, anim_code::IDLE, CellAnimation::CellIdle);
            }
        }
        FlowPhase::Summary => {
            game.locked = true;
            if flow.summary_closed {
                // 0xE75B80 -> LeaveStage 0xE72C80: cinematic mode (letterbox,
                // HUD hidden), request 0x2E for event 0x1F.
                flow.summary_closed = false;
                flow.summary = None;
                flow.phase = FlowPhase::LeaveStage;
                flow.timer = LEAVE_SECONDS;
                flow.letterbox = true;
                flow.hud_hidden = true;
                request_anim(&mut panim, anim_code::MOV_TO_BEACH, CellAnimation::CellMovToBeach);
            }
        }
        FlowPhase::LeaveStage => {
            game.locked = true;
            flow.letterbox = true;
            flow.hud_hidden = true;
            // Event 0x1F's per-frame update (0xE59A30, VERIFIED) sets the avatar's hit
            // lock (+0x17C, with +0x17B / +0x188) every frame: `Damage`, EatKill and
            // the zap search refuse it while the cinematic runs.
            if let Some(c) = pcombat.as_mut() {
                c.hit_cooldown = c.hit_cooldown.max(0.1);
            }
            if flow.timer <= 0.0 {
                // 0x1F end (0xE62440 / 0xE61140): cinematic mode off; the
                // creature, DNA (earned - spent + 50) and diet go to the next
                // stage; Creature mode with the black screen.
                flow.letterbox = false;
                run.editor_dna = (dna_earned(food) - run.cell.evolution_points_spent + ADVANCE_DNA_BONUS).max(0) as f32;
                effects.write(CellEffectRequest {
                    effect: 0xA1B0_293D,
                    position: Vec3::ZERO,
                    attach_to: None,
                    scale: 1.0,
                });
                flow.phase = FlowPhase::Advancing;
                run.advance();
                commands.insert_resource(crate::cell::OpenCreatorOnLand);
                next.set(GameStage::Creature);
                return;
            }
        }
        FlowPhase::Play => {
            game.locked = false;
            // First-minute hints (0xE697E0, RE 10B).
            flow.hint = None;
            if !run.cell.player_has_moved {
                flow.not_moved += dt;
                if flow.not_moved > HINT_SECONDS {
                    flow.hint = Some("clg-move");
                }
            } else if !run.cell.player_has_eaten {
                flow.not_eaten += dt;
                if flow.not_eaten > HINT_SECONDS {
                    flow.hint = Some(if caps.n[2] > 0 { "clg-carnivore" } else { "clg-herbivore" });
                }
            }
            // End of stage (0xE6E8C0): food 1000 with the avatar free.
            if food >= STAGE_FOOD && !flow.ending_done {
                if run.cell.missions[3].state == 1 {
                    run.cell.missions[3].state = 3;
                }
                flow.phase = FlowPhase::Ending;
                flow.timer = ENDING_SECONDS;
                flow.hint = None;
                return;
            }
            if keys.just_pressed(ADVANCE_KEY) && flow.ending_done {
                // Advance (0xE5DAA0): the consequence trait by diet, then the
                // summary window (0xE539A0: HUD hidden, paused).
                let summary = crate::cell_contract::CellSummary {
                    game_time_seconds: run.cell.game_time_ms as f32 / 1000.0,
                    kills: run.cell.kill_count,
                    deaths: run.cell.death_count,
                    diet: diet_class(&run).to_string(),
                    dna: (dna_earned(food) - run.cell.evolution_points_spent + ADVANCE_DNA_BONUS).max(0),
                };
                flow.summary = Some(summary);
                flow.phase = FlowPhase::Summary;
                flow.paused = true;
                flow.hud_hidden = true;
                flow.hint = None;
                game.locked = true;
                return;
            }
            if keys.just_pressed(MATE_KEY) && std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
                eprintln!(
                    "cell flow: mate key, available {} (show {} countdown {:.1} mate {:?})",
                    mate_available(&run, &flow),
                    run.cell.show_mate_button,
                    flow.mate_countdown,
                    flow.mate
                );
            }
            if keys.just_pressed(MATE_KEY) && mate_available(&run, &flow) {
                // Mating Call (0xE760F0): the mate appears just outside the
                // visible box, mateSpawnDistance beyond its edge.
                let a = rand::Rng::random_range(&mut game.rng, 0.0..std::f32::consts::TAU);
                let dir = Vec2::new(a.cos(), a.sin());
                // The box is `sVisibleBackgroundBBox` (0xE760F0 reads it):
                // the look point +-(10, 7.5) world units.
                let half = crate::cell_engine::VISIBLE_BOX_HALF * UNIT;
                let at = mate_spawn_point(game.camera_look, half, dir, globals.mate_spawn_distance * UNIT);
                queue.0.push(SpawnRequest {
                    cell: crate::world_objects::name_id("cell_mate"),
                    scale: po.scale,
                    position: at,
                    heading: heading_of(here - at),
                    size: Some(po.abs_size),
                    invulnerable: 999.0,
                    role: SpawnRole::Mate,
                    ..default()
                });
                request_anim(&mut panim, anim_code::MATING_CALL, CellAnimation::CellMatingCall);
                flow.mate_called = MATE_CALL_SECONDS;
                notice.show(text(&flow, GOAL_STRINGS[2]));
            }
            let Some((mate_entity, mt, mut mo, mut mai, mut manim)) = mates.iter_mut().next() else {
                return;
            };
            flow.mate = Some(mate_entity);
            mating.mate = Some(mate_entity);
            let mate_at = Vec2::new(mt.translation.x, mt.translation.z);
            // The approach (0xE71C00, VERIFIED): every frame until the trigger
            // `MoveToTarget` towards the avatar's position, facing it (0xE56F80).
            let caps = crate::cell_motion::player_caps(&run, &originals, &db);
            // RE 10E (0xE71C00): MoveToTarget(speed = 0xE67CB0 = the capability
            // speed of the cell's own model (caps + 0x1258; the mate's model is
            // the avatar's creature, so the player's caps), speedFactor =
            // block + 0x40 = the AI block's `turnFactor` (the ModAPI field at
            // 0x40, not `speed` at 0x18), propulsion style 0xE67C40).
            let turn_factor = db
                .cell(mo.cell)
                .map_or(1.0, |r| crate::cell_ai::ai_block(r, game.difficulty).turn_factor);
            mai.drive = crate::cell_motion::Drive {
                speed: caps.speed,
                speed_factor: turn_factor,
                style: caps.style,
            };
            mo.target = Some(here);
            mo.scale = po.scale;
            mo.abs_size = po.abs_size;
            if mate_at.distance(here) <= globals.mate_trigger_distance * UNIT {
                // MatingStart (0xE75EA0): both locked, the mating sequence;
                // tutorial mission 3 completes; every Eater flees.
                flow.phase = FlowPhase::Mating;
                flow.timer = MATING_SECONDS;
                game.locked = true;
                game.escape = true;
                run.cell.missions[3].state = 2;
                po.target = None;
                // MatingStart (10E): mate request 0x41 (Mate01), avatar 0x42.
                request_anim(&mut manim, anim_code::MATE_A, CellAnimation::CellMate01);
                request_anim(&mut panim, anim_code::MATE_B, CellAnimation::CellMate02);
            }
        }
        FlowPhase::Mating => {
            game.locked = true;
            game.escape = true;
            // Face each other and approach (MoveTo speed 1, style 8) while
            // more than 4 apart.
            if let Some((_, mt, mut mo, mut mai, _)) = mates.iter_mut().next() {
                let mate_at = Vec2::new(mt.translation.x, mt.translation.z);
                let apart = mate_at.distance(here) > 4.0 * UNIT;
                mai.drive = crate::cell_motion::Drive {
                    speed: 1.0,
                    speed_factor: 1.0,
                    style: 8,
                };
                mo.target = apart.then_some(here);
                po.heading = heading_of(mate_at - here);
                mo.heading = heading_of(here - mate_at);
                // 0xE75C30 (every frame of the event): the camera is held on the
                // midpoint of the two cells (ui+0xE4 = 1, +0xE8 = (mate + avatar)
                // x 0.5) and the follow speed is 3 (+0xF4).
                game.camera_hold = Some((mate_at + here) * 0.5);
                game.camera_speed = 3.0;
            }
            if flow.timer <= 0.0 {
                // MatingEnd (0xE78DE0) -> GoToEditor (0xE77930): ui+0xE4 = 0.
                game.camera_hold = None;
                game.escape = false;
                flow.phase = FlowPhase::GoToEditor;
                flow.timer = GO_TO_EDITOR_SECONDS;
                request_anim(&mut panim, anim_code::GO_TO_EDITOR, CellAnimation::CellHatchLayEgg);
            }
        }
        FlowPhase::GoToEditor => {
            game.locked = true;
            if flow.timer <= 0.0 {
                // Callback (0xE778B0): the egg at the player; end (0xE72AC0):
                // black screen, then the editor with DNA = earned - spent.
                queue.0.push(SpawnRequest {
                    cell: crate::world_objects::name_id("cell_hero_lay_egg"),
                    scale: po.scale,
                    position: here,
                    size: Some(po.abs_size),
                    invulnerable: 999.0,
                    role: SpawnRole::Egg,
                    ..default()
                });
                for (mate, ..) in &mates {
                    commands.entity(mate).despawn();
                }
                flow.mate = None;
                mating.mate = None;
                effects.write(CellEffectRequest {
                    effect: 0xA1B0_293D,
                    position: Vec3::ZERO,
                    attach_to: None,
                    scale: 1.0,
                });
                let dna = (dna_earned(food) - run.cell.evolution_points_spent).max(0) as f32;
                run.editor_dna = dna;
                flow.editor_dna = dna;
                flow.phase = FlowPhase::Editing;
                if let Some(v) = pvis.as_mut() {
                    **v = Visibility::Hidden;
                }
                mating.editing = true;
                run.cell.first_editor_entry = false;
                editor.show();
                editor.visuals_dirty = true;
                run.request_save();
            }
        }
        FlowPhase::Editing => {
            game.locked = true;
            if editor.open {
                return;
            }
            mating.editing = false;
            let spent = (flow.editor_dna - run.editor_dna).round() as i32;
            run.cell.evolution_points_spent += spent.max(0);
            run.editor_dna = (dna_earned(food) - run.cell.evolution_points_spent).max(0) as f32;
            // 0xE809A0: the editor return caches the earned DNA (field_30).
            flow.dna_at_creator = Some(dna_earned(food));
            run.cell.dna_at_creator = flow.dna_at_creator;
            // 0xE809A0 records `clg_evolveineditor` (a generation card, and a
            // barrier of the timeline's merged cards).
            run.cell
                .record_event(crate::cell_hud::timeline::events::EventKind::EvolveInEditor);
            for egg in &eggs {
                commands.entity(egg).despawn();
            }
            // The level restarts and the new generation hatches.
            po.hp = start_hp;
            po.velocity = Vec2::ZERO;
            po.target = None;
            run.cell.generation += 1;
            flow.phase = FlowPhase::Hatching;
            flow.timer = HATCH_SECONDS;
            run.request_save();
        }
        FlowPhase::Dying => {
            game.locked = true;
            po.target = None;
            if panim.request_code != anim_code::DEATH_EATEN && panim.request_code != anim_code::DEATH {
                if flow.bitten {
                    request_anim(&mut panim, anim_code::DEATH_EATEN, CellAnimation::CellDeathEaten);
                } else {
                    request_anim(&mut panim, anim_code::DEATH, CellAnimation::CellDeathAvatar);
                }
            }
            // The death cinematic is skippable (0xE7F550); skipping it skips
            // the hatch too (10D). ADAPTED: Space is the skip input.
            let skipped = keys.just_pressed(SKIP_KEY);
            if flow.timer <= 0.0 || skipped {
                // Respawn (0xE806B0 -> LevelInit, firstStart 0): same level,
                // food, DNA, unlocks and missions; the world is rebuilt with
                // the avatar at the origin, then the hatch (event 0x1C).
                po.hp = start_hp;
                po.velocity = Vec2::ZERO;
                po.target = None;
                game.origin_abs = game.to_abs(here);
                pt.translation.x = 0.0;
                pt.translation.z = 0.0;
                (game.camera_look, game.camera_lead, game.camera_focus) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
                game.camera_speed = crate::cell_engine::CAMERA_SPEED_DEFAULT;
                // LevelInit (0xE801B4..0xE801E7): the offset is the default and no focus object.
                game.camera_object = None;
                game.camera_focus_avatar = false;
                (game.offset_z, game.offset_z_target) = (crate::cell_engine::CAMERA_OFFSET_Z, crate::cell_engine::CAMERA_OFFSET_Z);
                flow.reset_level = true;
                if skipped {
                    flow.phase = FlowPhase::Play;
                    game.locked = false;
                    request_anim(&mut panim, anim_code::IDLE, CellAnimation::CellIdle);
                } else {
                    flow.phase = FlowPhase::Hatching;
                    flow.timer = HATCH_SECONDS;
                }
                for (mate, ..) in &mates {
                    commands.entity(mate).despawn();
                }
                flow.mate = None;
                mating.mate = None;
                run.request_save();
            }
        }
        FlowPhase::Advancing => {}
    }
}

/// The food balance (0xE511C0(plant, animal)): the animal surplus over a
/// thousand food, clamped to -1..1 (the decompiled `(1000 - plant - animal) *
/// 0` term is dead).
pub fn diet_balance(plant: i32, animal: i32) -> f32 {
    ((animal - plant) as f32 * 0.001).clamp(-1.0, 1.0)
}

/// Diet class (0xE5C9D0, VERIFIED): the balance of plant = overPlant + plant
/// and animal = overAnimal - plant + food against percentPlantFood /
/// percentAnimalFood (-0.35 / 0.35 in the installed globals): below the first
/// `clg_plant`, above the second `clg_meat`, else `clg_mix`.
pub fn diet_class(run: &RunProgress) -> &'static str {
    let c = &run.cell;
    let plant = c.over_plant_food_progression + c.plant_food_progression;
    let animal = c.over_animal_food_progression + c.food_progression - c.plant_food_progression;
    let x = diet_balance(plant, animal);
    if x < -0.35 {
        "clg_plant"
    } else if x > 0.35 {
        "clg_meat"
    } else {
        "clg_mix"
    }
}

/// Run condition of the simulation: not paused by a callout or the summary.
pub fn sim_running(flow: Option<Res<CellFlow>>) -> bool {
    flow.is_none_or(|f| f.running())
}

/// Enter (or the HUD's click on it) dismisses the front callout; when none is
/// left the game resumes. The summary window closes the same way.
pub fn dismiss(keys: Res<ButtonInput<KeyCode>>, flow: Option<ResMut<CellFlow>>) {
    let Some(mut flow) = flow else {
        return;
    };
    if !keys.just_pressed(DISMISS_KEY) {
        return;
    }
    if !flow.callouts.is_empty() {
        flow.callouts.remove(0);
        flow.paused = !flow.callouts.is_empty() || flow.summary.is_some();
    } else if flow.phase == FlowPhase::Summary && flow.summary.is_some() {
        flow.summary_closed = true;
        flow.paused = false;
        flow.hud_hidden = false;
    }
}

/// Fill `CellHudState` (every frame).
#[allow(clippy::too_many_arguments)]
pub fn publish_hud(
    run: Res<RunProgress>,
    db: Res<CellDatabase>,
    game: Res<CellGame>,
    flow: Res<CellFlow>,
    pause: Res<crate::pause_menu::PauseMenu>,
    mut hud: ResMut<CellHudState>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    player: Query<(Entity, &GlobalTransform, &CellObject), With<Player>>,
    objects: Query<(Entity, &GlobalTransform, &CellObject), Without<Player>>,
    combats: Query<&crate::cell_rules::CellCombat>,
    time: Res<Time>,
    mut watch: Local<RolloverWatch>,
) {
    let food = run.cell.food_progression;
    let (segment, fill) = bar_position(food);
    hud.dna_points = (dna_earned(food) - run.cell.evolution_points_spent).max(0);
    hud.dna_since_creator = (dna_earned(food) - flow.dna_at_creator.unwrap_or(0)).max(0);
    hud.level = segment;
    hud.level_count = BAR_SEGMENTS;
    hud.level_progress = fill;
    hud.stage_progress = (food as f32 / STAGE_FOOD as f32).clamp(0.0, 1.0);
    hud.biome = crate::cell_engine::biome(food);
    hud.food_progression = food;
    if let Ok((_, _, p)) = player.single() {
        hud.health = p.hp.max(0);
    }
    hud.max_health = crate::cell_rules::MAX_HP;
    hud.show_mate_button = mate_available(&run, &flow) && !flow.hud_hidden;
    hud.callouts = flow.callouts.clone();
    hud.hint = flow.hint.map(str::to_string);
    hud.summary = flow.summary.clone();
    hud.letterbox = flow.letterbox;
    hud.hud_hidden = flow.hud_hidden;
    hud.advance_visible = flow.ending_done && flow.phase == FlowPhase::Play && !flow.hud_hidden;
    hud.zoom = game.zoom_factor;
    hud.paused = pause.open;
    // Goal cards: collect parts (tutorial mission 1), then the mating call,
    // then the evolve message at food 1000.
    hud.goal_cards.clear();
    let collected = run.cell.unlocked_count().min(COLLECT_GOAL);
    if collected < COLLECT_GOAL {
        hud.goal_cards.push(CellGoalCard {
            kind: "Collect".into(),
            progress: Some((collected as i32, COLLECT_GOAL as i32)),
            title: text(&flow, GOAL_STRINGS[0]),
            body: text(&flow, GOAL_STRINGS[1]),
        });
    }
    if run.cell.show_mate_button {
        hud.goal_cards.push(CellGoalCard {
            kind: "Mate".into(),
            progress: None,
            title: text(&flow, GOAL_STRINGS[2]),
            body: String::new(),
        });
    }
    if food >= STAGE_FOOD {
        hud.goal_cards.push(CellGoalCard {
            kind: "Evolve".into(),
            progress: None,
            title: text(&flow, GOAL_STRINGS[3]),
            body: String::new(),
        });
    }
    hud.goals = hud
        .goal_cards
        .iter()
        .map(|g| match g.progress {
            Some((n, of)) => (format!("{} {n}/{of} - {}", g.kind, g.title), g.body.clone()),
            None => (g.title.clone(), g.body.clone()),
        })
        .collect();
    // Health rollover records (`cCellUI`, pool `0xE62200` / `0xE582C0` /
    // `0xE57FD0`): the cell under the cursor, plus the cells whose health just
    // changed (the `ShowHealthRollover` call sites, below).
    hud.rollover = None;
    hud.rollovers.clear();
    let Ok((camera, camera_t)) = cameras.single() else {
        watch.prev_hp.clear();
        return;
    };
    let unlocked = run.cell.unlocked_parts;
    let make = |entity: Entity, t: &GlobalTransform, o: &CellObject, avatar: bool| -> Option<CellRollover> {
        let screen = camera.world_to_viewport(camera_t, t.translation()).ok()?;
        // The still locked parts the cell carries (0xE57E10 flags per unlock
        // slot, 0xE57EC0 shows the first two whose `mUnlockedParts` entry is 0).
        let mut part_slots: Vec<usize> = if avatar {
            Vec::new()
        } else {
            db.cell(o.cell)
                .and_then(|r| db.loot_tables.get(&r.loot))
                .map(|loot| {
                    loot.entries
                        .iter()
                        .filter(|e| e.kind == LootEntryType::Cell)
                        .filter_map(|e| db.cell(e.cell))
                        .filter(|c| c.cell_type == crate::cell_data::CellType::Unlock)
                        .map(|c| c.unlock_type.raw() as usize)
                        .filter(|slot| unlocked.get(*slot).is_some_and(|v| *v == 0))
                        .collect()
                })
                .unwrap_or_default()
        };
        part_slots.sort_unstable();
        part_slots.dedup();
        let carries_part = !part_slots.is_empty();
        // 0xE58561: the rollover hangs 0.8 radius above the cell (up on the
        // screen), or on the cell itself when it is far larger than the player.
        let giant = !avatar && relative_scale(o.scale, food) == 4;
        let anchor = if giant {
            screen
        } else {
            camera
                .world_to_viewport(camera_t, t.translation() + camera_t.up() * (0.8 * o.radius))
                .unwrap_or(screen)
        };
        Some(CellRollover {
            entity,
            name: if avatar { String::new() } else { db.cell_name(o.cell).unwrap_or_default() },
            health: o.hp.max(0),
            max_health: crate::cell_rules::MAX_HP,
            carries_part,
            part_slots,
            giant,
            screen: anchor,
            hovered: false,
            trigger: None,
            avatar,
        })
    };
    // The cell under the cursor.
    let cursor = windows.iter().find_map(|w| w.cursor_position());
    let mut best: Option<(f32, Entity)> = None;
    if let Some(cursor) = cursor {
        for (entity, t, o) in &objects {
            if o.background || !(o.kind.is_creature() || matches!(o.kind, CellKind::Unlock | CellKind::Fragment)) {
                continue;
            }
            let Ok(screen) = camera.world_to_viewport(camera_t, t.translation()) else {
                continue;
            };
            let Ok(edge) = camera.world_to_viewport(camera_t, t.translation() + Vec3::X * o.radius) else {
                continue;
            };
            let pick = (edge - screen).length().max(12.0);
            let d = screen.distance(cursor);
            if d > pick || best.is_some_and(|(b, _)| d >= b) {
                continue;
            }
            best = Some((d, entity));
        }
    }
    // `ShowHealthRollover(cell, initialHealth)` is called before the health
    // changes: by the damage routine `0xE7A7C0` when the avatar is the victim
    // or the attacker, and by `PlayerAte` `0xE7D070` while the avatar's health
    // is below the maximum. The engine applies those changes elsewhere, so
    // they are seen here as a change of `hp` between frames.
    let dt = time.delta_secs();
    let avatar = player.single().ok().map(|(e, t, o)| (e, t.translation(), o.radius, o.hp));
    // Damage by the avatar: the last attacker of the cell (the damage routine knows it)
    // is the avatar or a weapon cloud the avatar owns (+0x10C), and the hit is recent.
    let avatar_entity = avatar.map(|a| a.0);
    let by_avatar = |e: Entity| {
        combats.get(e).is_ok_and(|c| {
            c.since_hurt < ROLLOVER_RECENT
                && c.last_attacker.is_some_and(|a| {
                    Some(a) == avatar_entity || combats.get(a).is_ok_and(|ca| ca.owner.is_some() && ca.owner == avatar_entity)
                })
        })
    };
    let others = objects
        .iter()
        .map(|(e, t, o)| (e, t.translation(), o.radius, o.hp, o.background, by_avatar(e)));
    let triggers = watch.observe(avatar, others, dt);
    let mut out: Vec<CellRollover> = Vec::new();
    let mut add = |entity: Entity, hovered: bool| {
        let trigger = triggers.iter().find(|(e, _)| *e == entity).map(|(_, prev)| *prev);
        if let Some(existing) = out.iter_mut().find(|r| r.entity == entity) {
            existing.hovered |= hovered;
            existing.trigger = existing.trigger.or(trigger);
            return;
        }
        let made = if let Ok((e, t, o)) = player.get(entity) {
            make(e, t, o, true)
        } else if let Ok((e, t, o)) = objects.get(entity) {
            make(e, t, o, false)
        } else {
            None
        };
        if let Some(mut r) = made {
            r.hovered = hovered;
            r.trigger = trigger;
            out.push(r);
        }
    };
    if let Some((_, entity)) = best {
        add(entity, true);
    }
    let watched: Vec<Entity> = watch.keep.keys().copied().collect();
    for entity in watched {
        add(entity, false);
    }
    hud.rollover = out.iter().find(|r| r.hovered).cloned();
    hud.rollovers = out;
}

/// Seconds a cell stays published to the rollover pool after its health
/// changed (longer than any record lives).
const ROLLOVER_WATCH_SECONDS: f32 = 4.0;
/// How recent (seconds) a cell's last hurt must be for its loss of health to count as
/// that attacker's damage: the damage routine (0xE7A7C0) shows the rollover in the
/// call itself, here the HUD sees the health change one frame later.
const ROLLOVER_RECENT: f32 = 0.2;

/// Per-entity health of the last frame and the cells being followed.
#[derive(Default)]
pub struct RolloverWatch {
    prev_hp: std::collections::HashMap<Entity, i32>,
    keep: std::collections::HashMap<Entity, f32>,
}

impl RolloverWatch {
    /// Compare this frame's health with the last frame's and return the
    /// `ShowHealthRollover(cell, initialHealth)` calls to make: the avatar's
    /// damage (also a killing blow) and heals (not a respawn from 0), and the
    /// damage the avatar dealt to other cells (the damage routine 0xE7A7C0 shows the
    /// rollover when the avatar is the attacker). `avatar` is (entity, position,
    /// radius, hp), `others` (entity, position, radius, hp, background, hurt by the
    /// avatar just now). The cells triggered are then followed for
    /// [`ROLLOVER_WATCH_SECONDS`].
    fn observe(
        &mut self,
        avatar: Option<(Entity, Vec3, f32, i32)>,
        others: impl Iterator<Item = (Entity, Vec3, f32, i32, bool, bool)>,
        dt: f32,
    ) -> Vec<(Entity, i32)> {
        let mut triggers: Vec<(Entity, i32)> = Vec::new();
        let mut seen: Vec<Entity> = Vec::new();
        if let Some((e, _, _, hp)) = avatar {
            seen.push(e);
            match self.prev_hp.insert(e, hp) {
                Some(prev) if hp < prev || (hp > prev && prev > 0) => triggers.push((e, prev)),
                _ => {}
            }
        }
        for (entity, _, _, hp, background, hurt_by_avatar) in others {
            seen.push(entity);
            let prev = self.prev_hp.insert(entity, hp);
            if let (Some(prev), Some(_)) = (prev, avatar) {
                if hp < prev && !background && hurt_by_avatar {
                    triggers.push((entity, prev));
                }
            }
        }
        self.prev_hp.retain(|e, _| seen.contains(e));
        for (e, _) in &triggers {
            self.keep.insert(*e, ROLLOVER_WATCH_SECONDS);
        }
        self.keep.retain(|e, left| {
            *left -= dt;
            *left > 0.0 && seen.contains(e)
        });
        triggers
    }
}

/// Animation bookkeeping outside the simulation step.
pub fn animate(time: Res<Time>, mut eggs: Query<(&mut CellEgg, &mut CellAnimState)>) {
    let dt = time.delta_secs();
    for (mut egg, mut anim) in &mut eggs {
        egg.age += dt;
        anim.elapsed += dt;
    }
}

/// Scale of a newly spawned object relative to the player (helper for tests).
pub fn relative_scale(object: CellScale, food: i32) -> usize {
    crate::cell_engine::scale_diff(object, player_scale(food))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mate_button_needs_the_flag_and_the_countdown() {
        let mut run = RunProgress::default();
        let mut flow = CellFlow::default();
        assert!(!mate_available(&run, &flow));
        run.cell.show_mate_button = true;
        flow.mate_countdown = MATE_COUNTDOWN_MEAL;
        assert!(!mate_available(&run, &flow), "countdown still running");
        flow.mate_countdown = 0.0;
        assert!(mate_available(&run, &flow));
        flow.phase = FlowPhase::Mating;
        assert!(!mate_available(&run, &flow));
    }

    #[test]
    fn mate_spawns_beyond_the_visible_box_edge() {
        let p = mate_spawn_point(Vec2::ZERO, Vec2::new(16.0, 9.0), Vec2::X, 4.0);
        assert!(p.abs_diff_eq(Vec2::new(20.0, 0.0), 1e-5));
        let p = mate_spawn_point(Vec2::ZERO, Vec2::new(16.0, 9.0), Vec2::NEG_Y, 4.0);
        assert!(p.abs_diff_eq(Vec2::new(0.0, -13.0), 1e-5));
    }

    #[test]
    fn decoration_follows_scale_and_eyes() {
        use crate::world_objects::name_id;
        assert_eq!(player_decoration(CellScale(1), 2), name_id("cell_hero_L1_decorations"));
        assert_eq!(player_decoration(CellScale(3), 2), name_id("cell_hero_L4_decorations"));
        assert_eq!(player_decoration(CellScale(10), 1), name_id("cell_hero_L400_decorations"));
        assert_eq!(player_decoration(CellScale(5), 0), name_id("cell_hero_NoEyes"));
    }

    #[test]
    fn bar_has_five_200_food_segments() {
        assert_eq!(bar_position(0), (0, 0.0));
        assert_eq!(bar_position(100), (0, 0.5));
        assert_eq!(bar_position(200), (1, 0.0));
        assert_eq!(bar_position(999).0, 4);
        assert_eq!(bar_position(1000), (4, 1.0));
    }

    #[test]
    fn diet_follows_the_thousand_food_balance() {
        assert_eq!(diet_balance(0, 0), 0.0);
        assert_eq!(diet_balance(400, 0), -0.4);
        assert_eq!(diet_balance(0, 1500), 1.0, "clamped");
        let mut run = RunProgress::default();
        run.cell.food_progression = 600;
        run.cell.plant_food_progression = 0;
        assert_eq!(diet_class(&run), "clg_meat", "600 animal food: 0.6 > 0.35");
        run.cell.plant_food_progression = 600;
        assert_eq!(diet_class(&run), "clg_plant", "600 plant food");
        run.cell.plant_food_progression = 300;
        assert_eq!(diet_class(&run), "clg_mix", "300 plant / 300 animal");
        run.cell.plant_food_progression = 200;
        run.cell.food_progression = 700;
        assert_eq!(diet_class(&run), "clg_mix", "500 animal vs 200 plant: 0.3 is below the 0.35 limit");
    }

    #[test]
    fn phases_take_their_time_from_the_clip() {
        use crate::cell_anim::{CellClipTimes, SelectContext};
        // Without the table the fallback constants run.
        let mut flow = CellFlow::hatching();
        let before = flow.timer;
        time_phase(&mut flow, &CellClipTimes::default(), CellScale(2));
        assert_eq!((flow.timer, flow.clip_phase), (before, None), "table not loaded yet");
        let _ = SelectContext::default();
        // Phases without a clip event are marked taken at once.
        let mut play = CellFlow::default();
        assert_eq!(play.phase, FlowPhase::Play);
        time_phase(&mut play, &CellClipTimes::default(), CellScale(2));
        assert_eq!(play.clip_phase, Some(FlowPhase::Play));
        // Every timed phase names the request its event uses.
        let mut flow = CellFlow::default();
        for (phase, code, extra) in [
            (FlowPhase::Hatching, anim_code::HATCH, 0.0),
            (FlowPhase::Intro, anim_code::HATCH_ICE, 0.0),
            (FlowPhase::GoToEditor, 41, 1.0),
            (FlowPhase::LeaveStage, 46, 0.0),
            (FlowPhase::Ending, 68, 0.0),
            (FlowPhase::Mating, 65, 0.0),
        ] {
            flow.phase = phase;
            let (c, e, _) = phase_request(&flow).unwrap();
            assert_eq!((c, e), (code, extra), "{phase:?}");
        }
    }

    #[test]
    fn dying_phase_bookkeeping() {
        let mut flow = CellFlow::hatching();
        assert_eq!(flow.phase, FlowPhase::Hatching);
        assert!(!flow.player_dead());
        flow.kill_player(true);
        assert!(flow.player_dead() && flow.bitten && flow.camera_frozen());
        assert_eq!(relative_scale(CellScale(1), 0), 2);
    }

    #[test]
    fn health_changes_of_the_avatar_trigger_rollovers_with_the_old_health() {
        let mut world = World::new();
        let avatar = world.spawn_empty().id();
        let at = |hp: i32| Some((avatar, Vec3::ZERO, 1.0, hp));
        let none = || std::iter::empty::<(Entity, Vec3, f32, i32, bool, bool)>();
        let mut watch = RolloverWatch::default();
        // First sight of the avatar: nothing to compare with.
        assert!(watch.observe(at(6), none(), 0.016).is_empty());
        // A bite (6 -> 4): `ShowHealthRollover(avatar, 6)` before the change.
        assert_eq!(watch.observe(at(4), none(), 0.016), vec![(avatar, 6)]);
        // No change, no call.
        assert!(watch.observe(at(4), none(), 0.016).is_empty());
        // Eating heals (4 -> 6): the call carries the health before.
        assert_eq!(watch.observe(at(6), none(), 0.016), vec![(avatar, 4)]);
        // A killing blow counts, the respawn (0 -> 6) does not.
        assert_eq!(watch.observe(at(0), none(), 0.016), vec![(avatar, 6)]);
        assert!(watch.observe(at(6), none(), 0.016).is_empty());
    }

    #[test]
    fn other_cells_trigger_only_when_the_avatar_dealt_the_damage() {
        let mut world = World::new();
        let avatar = world.spawn_empty().id();
        let prey = world.spawn_empty().id();
        let far = world.spawn_empty().id();
        let player = Some((avatar, Vec3::ZERO, 1.0, 6));
        let cells = |prey_hp: i32, far_hp: i32| {
            vec![
                (prey, Vec3::new(1.5, 0.0, 0.0), 1.0, prey_hp, false, true),
                (far, Vec3::new(30.0, 0.0, 0.0), 1.0, far_hp, false, false),
            ]
        };
        let mut watch = RolloverWatch::default();
        assert!(watch.observe(player, cells(6, 6).into_iter(), 0.016).is_empty());
        // Both lose health; only the one the avatar hurt shows its rollover.
        assert_eq!(watch.observe(player, cells(3, 3).into_iter(), 0.016), vec![(prey, 6)]);
        // The triggered cell is followed for a while, then dropped.
        assert!(watch.keep.contains_key(&prey) && !watch.keep.contains_key(&far));
        for _ in 0..10 {
            watch.observe(player, cells(3, 3).into_iter(), 0.5);
        }
        assert!(watch.keep.is_empty());
    }
}
