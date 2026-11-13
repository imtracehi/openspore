//! The Creature stage's history window (the retail timeline UI) and the
//! cinematic letterbox of the stage moments.
//!
//! Evolve at the last brain level ends in the stage timeline
//! (`creature_flow::Phase::Summary`, retail 0xE3E350 with kGameCreature): the
//! installed `timeline` layout (stage tabs and evolution clock on top, the five
//! stage columns, the green check button) with the Creature tab selected and
//! the history events of the stage as cards, the trait bar of the earned
//! disposition (`crg_social` / `crg_mix` / `crg_attack`) and the
//! `Callout_Timeline_TransitionOutOfCreature` dialog. The check button (or
//! Enter) closes it and Tribe starts. The Cell stage's window
//! ([`crate::cell_hud::timeline`]) is the same layout; this module reuses its
//! pieces.
//!
//! Each history event is a property file of group `TimelineEvents`
//! (`crg_gamestart`, `crg_grow`, `crg_killedsomething`, `crg_befriendedsomething`,
//! `crg_death`, `crg_migrated`, `crg_packmemberadded`, ...) naming the card
//! layout and icon.
//!
//! The layout, scroll and intro sequencer are the retail machinery decoded by
//! cell-timeline3 ([`cell::axis`]: cards one behind the other from x = 5 with a
//! 5 px gap, the stage's column as wide as its events, the follow-smoothed
//! scroll, the `0xE3B990` sequencer) and the clock is the retail date formula
//! (`0xE36FA0`).
//!
//! ADAPTED (what the decode did not reach): the cards sit on one centre line,
//! kills / befriended creatures / deaths / pack members are one card each
//! carrying the count, the brain upgrades and the migrations are one card each,
//! the part, allied-species and extinct-species cards are not drawn, the Cell
//! column before the Creature one is the 5 px default (its events are not
//! recorded). The trait is the retail one (0xD38780): the balance of the attack
//! and social DNA the rewards filled (`creature_flow::trait_slot`).

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CreatureHudRoot, CreatureHudState};
use crate::{
    cell_hud::{
        dialogs,
        timeline::{self as cell, axis, card, win, CALLOUT_CENTRE_Y, CARD_GAP, CARD_ROW_X, CARD_ROW_Y, SHOW_AFTER_FRAMES, TRAIT_BAR_AT},
    },
    creature_flow::CreatureSummary,
    dbpf::ResourceKey,
    game::RunProgress,
    spui::{
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const TRANSITION_CALLOUT: &str = "Callout_Timeline_TransitionOutOfCreature";
/// The Creature tab of the timeline.
const CREATURE_STAGE: usize = 1;
const LAYOUT_BRAIN: &str = "timelineEventBrainUpgrade";
const LAYOUT_MIGRATED: &str = "timelineEventMigrated";
const LAYOUT_PACK: &str = "timelineEventPackMemberAdded";
/// `ConsequenceTraits` instances: `crg_social`, `crg_mix`, `crg_attack`.
const TRAITS: [u32; 3] = [0xFD15_9D91, 0xB9DE_15F2, 0x17E5_EF84];

/// What one card shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Card {
    /// `crg_gamestart`: the creature on land ("Generation n").
    Start,
    /// `crg_grow`: brain level `n` reached.
    Brain(u8),
    /// `crg_packmemberadded`: pack members recruited.
    Pack(u32),
    /// `crg_killedsomething`.
    Kills(u32),
    /// `crg_befriendedsomething`.
    Befriended(u32),
    /// `crg_migrated`: migration `n` done.
    Migrated(u8),
    /// `crg_death`.
    Deaths(u32),
}

impl Card {
    fn layout(&self) -> &'static str {
        match self {
            Card::Start => cell::LAYOUT_LARGE,
            Card::Brain(_) => LAYOUT_BRAIN,
            Card::Pack(_) => LAYOUT_PACK,
            Card::Kills(_) | Card::Befriended(_) => cell::LAYOUT_ICON,
            Card::Migrated(_) => LAYOUT_MIGRATED,
            Card::Deaths(_) => cell::LAYOUT_DEATH,
        }
    }

    /// Position in the row (the order the events happened in a typical run).
    fn order(&self) -> (u8, u32) {
        match self {
            Card::Start => (0, 0),
            Card::Brain(n) => (1, *n as u32),
            Card::Pack(_) => (2, 0),
            Card::Kills(_) => (3, 0),
            Card::Befriended(_) => (4, 0),
            Card::Migrated(n) => (5, *n as u32),
            Card::Deaths(_) => (6, 0),
        }
    }

    /// The event whose property file gives the icon.
    fn event(&self) -> Option<&'static str> {
        match self {
            Card::Kills(_) => Some("crg_killedsomething"),
            Card::Befriended(_) => Some("crg_befriendedsomething"),
            _ => None,
        }
    }
}

/// The history cards of a run, in row order.
pub fn cards(summary: &CreatureSummary) -> Vec<Card> {
    let mut out = vec![Card::Start];
    out.extend((1..=summary.brain_level.min(4)).map(Card::Brain));
    if summary.pack > 0 {
        out.push(Card::Pack(summary.pack as u32));
    }
    if summary.kills > 0 {
        out.push(Card::Kills(summary.kills));
    }
    if summary.befriended > 0 {
        out.push(Card::Befriended(summary.befriended));
    }
    out.extend((1..=summary.migrations.min(2)).map(Card::Migrated));
    if summary.deaths > 0 {
        out.push(Card::Deaths(summary.deaths));
    }
    out.sort_by_key(Card::order);
    out
}

/// The Creature stage's evolution clock (`0xE36FA0`): the Cell stage counts as
/// its fixed 1000 game seconds, then `play_seconds` of the Creature stage at
/// 1000 years per second, cut to a multiple of 1 000 000 years.
pub fn clock_numbers(play_seconds: f32) -> [u32; 4] {
    cell::clock_digits(cell::date_years(CREATURE_STAGE, play_seconds.max(0.0) as u64, None))
}

/// Which layout a spawned root is.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub enum CrgTimelineRoot {
    /// The opaque backdrop node ([`cell::backdrop_node`]); no layout.
    Backdrop,
    Main,
    Callout,
    TraitBar,
    Card(Card),
}

/// Loaded event definitions and the spawn clock.
#[derive(Resource, Default)]
pub struct CrgTimelineCache {
    cell: cell::TimelineCache,
    trait_icons: [Option<ResourceKey>; 3],
    traits_loaded: bool,
    callout: Option<dialogs::CalloutDef>,
    callout_loaded: bool,
    generation_title: Option<String>,
    /// Retail icons of the event files (`crg_*`).
    icons: HashMap<&'static str, Option<ResourceKey>>,
}

/// Spawn the timeline layouts when the summary appears, despawn them when it
/// is closed.
#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    hud: Res<CreatureHudState>,
    stage: Res<State<GameStage>>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut cache: ResMut<CrgTimelineCache>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CrgTimelineRoot)>,
) {
    let want = *stage.get() == GameStage::Creature && hud.summary.is_some();
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        cache.cell.frames = 0;
        cache.cell.seq = None;
        cache.cell.bands = [[0.0; 4]; 3];
        return;
    }
    if !roots.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let (Ok(camera), Some(summary)) = (camera.single(), hud.summary.as_ref()) else {
        return;
    };
    let index = &originals.resource_index;
    let mut spawn = |commands: &mut Commands, name: &str, kind: CrgTimelineRoot, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert((kind, crate::stages::StageOwned));
        }
    };
    commands.spawn((cell::backdrop_node(CREATURE_STAGE), UiTargetCamera(camera), CrgTimelineRoot::Backdrop));
    spawn(&mut commands, cell::TIMELINE_LAYOUT, CrgTimelineRoot::Main, RootPolicy::Fit, 104);
    spawn(&mut commands, cell::LAYOUT_TRAIT_BAR, CrgTimelineRoot::TraitBar, RootPolicy::Authored, 105);
    for card in cards(summary) {
        let layout = card.layout();
        spawn(&mut commands, layout, CrgTimelineRoot::Card(card), RootPolicy::Authored, 105);
    }
    spawn(&mut commands, dialogs::CALLOUT_LAYOUT, CrgTimelineRoot::Callout, RootPolicy::Authored, 106);
    cache.cell.frames = 0;
}

/// Fill one card.
fn bind_card(
    ui: &mut SpuiState,
    kind: &Card,
    rect: Rect,
    show: bool,
    generation: u32,
    index: &crate::asset_catalog::ResourceIndex,
    assets: &mut SpuiAssets,
    icon: Option<ResourceKey>,
) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, show);
    cell::place_card_root(ui, (rect.x1, rect.y1));
    let set_text = |ui: &mut SpuiState, control: u32, text: String| {
        if let Some(w) = ui.find(control) {
            ui.set_text(w, text);
        }
    };
    match kind {
        Card::Start => {
            // "Generation ~timelineGenerationCount~".
            let title = assets
                .localized(index, crate::world_objects::name_id("timeline"), 0x060B_4210)
                .unwrap_or_default()
                .replace("~timelineGenerationCount~", &generation.max(1).to_string());
            set_text(ui, card::LABEL, title);
        }
        Card::Kills(n) | Card::Befriended(n) => {
            if let Some(w) = ui.find(card::ICON) {
                ui.set_image(w, icon.map(cell::image_ref));
            }
            if let Some(w) = ui.find(card::NUMBER) {
                ui.set_visible(w, true);
            }
            set_text(ui, card::NUMBER, n.to_string());
        }
        Card::Pack(n) | Card::Deaths(n) => set_text(ui, card::NUMBER, n.to_string()),
        Card::Brain(_) | Card::Migrated(_) => {}
    }
}

/// Fill the trait bar slots with the three trait icons, the earned one bright.
fn bind_trait_bar(ui: &mut SpuiState, earned: usize, icons: &[Option<ResourceKey>; 3], show: bool) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, show);
    let a = ui.layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    ui.set_area(root, [TRAIT_BAR_AT.0, TRAIT_BAR_AT.1, TRAIT_BAR_AT.0 + w, TRAIT_BAR_AT.1 + h]);
    for (slot, control) in cell::TRAIT_SLOTS.iter().enumerate() {
        let Some(w) = ui.find(*control) else {
            continue;
        };
        ui.set_image(w, icons[slot].map(cell::image_ref));
        ui.set_tint(w, if slot == earned { 0xFFFF_FFFF } else { 0x70FF_FFFF });
    }
}

/// Bind every timeline layout and turn the check button into the dismiss key.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    hud: Res<CreatureHudState>,
    time: Res<Time>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CrgTimelineCache>,
    mut roots: Query<(&mut SpuiState, &CrgTimelineRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
) {
    let Some(summary) = hud.summary.clone() else {
        return;
    };
    let index = &originals.resource_index;
    cache.cell.frames += 1;
    let show = cache.cell.frames > SHOW_AFTER_FRAMES;
    if !cache.traits_loaded {
        cache.traits_loaded = true;
        for (i, instance) in TRAITS.iter().enumerate() {
            let key = ResourceKey {
                group_id: cell::TRAIT_GROUP,
                instance_id: *instance,
                type_id: crate::asset_catalog::PROP,
            };
            cache.trait_icons[i] =
                index.properties(key).ok().and_then(|p| p.0.get(&cell::PROP_TRAIT_ICON).and_then(|v| v.key()));
        }
    }
    if !cache.callout_loaded {
        cache.callout_loaded = true;
        cache.callout = dialogs::read_callout(index, &mut assets, TRANSITION_CALLOUT);
    }
    for name in ["crg_killedsomething", "crg_befriendedsomething"] {
        if !cache.icons.contains_key(name) {
            let icon = cell::event_def(&mut cache.cell, name, index, &mut assets).and_then(|(icon, _)| icon);
            cache.icons.insert(name, icon);
        }
    }
    let trait_icons = cache.trait_icons;
    let callout = cache.callout.clone();
    let icons = cache.icons.clone();
    for icon in trait_icons.iter().flatten().chain(icons.values().flatten()) {
        let _ = assets.texture(index, &mut images, *icon);
    }
    // Cards in event order, laid out one behind the other (`0xE3F970`).
    let mut placed: Vec<(Card, Vec2)> = roots
        .iter()
        .filter_map(|(ui, kind)| match kind {
            CrgTimelineRoot::Card(k) => Some((k.clone(), cell::card_size(&ui))),
            _ => None,
        })
        .collect();
    placed.sort_by_key(|(k, _)| k.order());
    let sizes: Vec<Vec2> = placed.iter().map(|(_, s)| *s).collect();
    let widths: Vec<f32> = sizes.iter().map(|s| s.x).collect();
    let (_, extent) = axis::layout_events(&widths);
    let dt_ms = (time.delta_secs() * 1000.0).round().max(0.0) as i32;
    cache.cell.capture.read_once();
    let clicked_callout =
        cache.cell.capture.dismiss_callout_after.is_some_and(|n| cache.cell.frames > SHOW_AFTER_FRAMES + n);
    let frame = cell::step_frame(
        &mut cache.cell,
        CREATURE_STAGE,
        extent,
        dt_ms,
        show,
        callout.is_some(),
        clicked_callout,
        summary.trait_slot.min(2),
    );
    let bands = cache.cell.bands;
    let rects = cell::place_cards(
        &sizes,
        frame.columns.left[CREATURE_STAGE] + CARD_ROW_X - frame.shift,
        CARD_ROW_Y,
        CARD_GAP,
    );
    let mut callout_clicked = false;
    let mut dismiss = false;
    for (mut ui, kind) in &mut roots {
        match kind {
            CrgTimelineRoot::Main => {
                cell::bind_main_columns(
                    &mut ui,
                    CREATURE_STAGE,
                    clock_numbers(summary.game_time_seconds),
                    show,
                    &frame,
                    &bands,
                );
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == win::OK {
                        dismiss = true;
                    }
                }
            }
            CrgTimelineRoot::Callout => {
                dialogs::bind_callout(&mut ui, callout.as_ref().filter(|_| show && frame.callout_up), None, CALLOUT_CENTRE_Y);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        callout_clicked = true;
                    }
                }
            }
            CrgTimelineRoot::TraitBar => bind_trait_bar(&mut ui, summary.trait_slot, &trait_icons, show),
            CrgTimelineRoot::Backdrop => {}
            CrgTimelineRoot::Card(k) => {
                let Some(i) = placed.iter().position(|(c, _)| c == k) else {
                    continue;
                };
                let icon = k.event().and_then(|e| icons.get(e).copied().flatten());
                bind_card(&mut ui, k, rects[i], show, run.creature_lifecycle.generation, index, &mut assets, icon);
            }
        }
    }
    // `0xE36F80`: the dismissed callout moves the sequencer on.
    if callout_clicked {
        if let Some(seq) = cache.cell.seq.as_mut() {
            seq.close_callout();
        }
    }
    if dismiss && !keys.pressed(crate::creature_flow::DISMISS_KEY) {
        keys.press(crate::creature_flow::DISMISS_KEY);
        injected.push(crate::creature_flow::DISMISS_KEY);
    }
}

/// The creature thumbnail of the first card: the portrait render shown by a
/// plain node over the card's image slot.
#[derive(Component)]
pub struct CrgTimelineThumbnail;

pub fn thumbnail(
    mut commands: Commands,
    hud: Res<CreatureHudState>,
    state: Res<super::portrait::PortraitState>,
    roots: Query<(&SpuiState, &CrgTimelineRoot)>,
    mut nodes: Query<(Entity, &mut Node, &mut Visibility), With<CrgTimelineThumbnail>>,
    cache: Res<CrgTimelineCache>,
) {
    let target = (hud.summary.is_some() && cache.cell.frames > SHOW_AFTER_FRAMES + 20)
        .then(|| {
            roots.iter().find_map(|(ui, kind)| match kind {
                CrgTimelineRoot::Card(Card::Start) if ui.ready => ui.find(card::IMAGE).map(|w| (ui.rect(w), ui.screen)),
                _ => None,
            })
        })
        .flatten();
    let (Some((rect, screen)), Some(image)) = (target, state.image()) else {
        for (e, _, _) in &nodes {
            commands.entity(e).despawn();
        }
        return;
    };
    let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
    if let Ok((_, mut node, mut visibility)) = nodes.single_mut() {
        node.left = pct(rect.x1, screen[0]);
        node.top = pct(rect.y1, screen[1]);
        node.width = pct(rect.width(), screen[0]);
        node.height = pct(rect.height(), screen[1]);
        *visibility = Visibility::Inherited;
    } else {
        commands.spawn((
            CrgTimelineThumbnail,
            Node {
                position_type: PositionType::Absolute,
                left: pct(rect.x1, screen[0]),
                top: pct(rect.y1, screen[1]),
                width: pct(rect.width(), screen[0]),
                height: pct(rect.height(), screen[1]),
                ..default()
            },
            ImageNode::new(image),
            Pickable::IGNORE,
            GlobalZIndex(107),
            crate::stages::StageOwned,
        ));
    }
}

/// Cinematic letterbox bars of the stage moments (11% of the screen, as the
/// Cell stage's; ADAPTED: the retail code switches the engine's cinematic
/// mode) and the HUD hiding behind the stage timeline.
#[derive(Component)]
pub struct CreatureOverlayRoot;

pub fn overlays(
    mut commands: Commands,
    hud: Res<CreatureHudState>,
    stage: Res<State<GameStage>>,
    mut last: Local<Option<bool>>,
    roots: Query<Entity, With<CreatureOverlayRoot>>,
    mut layouts: Query<(&mut Visibility, &CreatureHudRoot)>,
) {
    let in_creature = *stage.get() == GameStage::Creature;
    let hidden = hud.summary.is_some();
    for (mut v, _) in &mut layouts {
        let want = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
    let bars = in_creature && (hud.letterbox || hidden);
    if *last == Some(bars) && (!bars || !roots.is_empty()) {
        return;
    }
    *last = Some(bars);
    for e in &roots {
        commands.entity(e).despawn();
    }
    if !bars {
        return;
    }
    let root = commands
        .spawn((
            CreatureOverlayRoot,
            crate::stages::StageOwned,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(150),
        ))
        .id();
    commands.entity(root).with_children(|ui| {
        for top in [true, false] {
            let mut node = Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Percent(11.0),
                ..default()
            };
            if top {
                node.top = Val::Px(0.0);
            } else {
                node.bottom = Val::Px(0.0);
            }
            ui.spawn((node, BackgroundColor(Color::BLACK)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(kills: u32, befriended: u32, deaths: u32, brain: u8) -> CreatureSummary {
        CreatureSummary {
            game_time_seconds: 1800.0,
            kills,
            befriended,
            deaths,
            brain_level: brain,
            pack: 2,
            migrations: 2,
            trait_slot: 1,
            ..default()
        }
    }

    #[test]
    fn history_cards_follow_the_run() {
        assert_eq!(cards(&CreatureSummary::default()), vec![Card::Start]);
        let all = cards(&summary(7, 3, 1, 4));
        assert_eq!(
            all,
            vec![
                Card::Start,
                Card::Brain(1),
                Card::Brain(2),
                Card::Brain(3),
                Card::Brain(4),
                Card::Pack(2),
                Card::Kills(7),
                Card::Befriended(3),
                Card::Migrated(1),
                Card::Migrated(2),
                Card::Deaths(1),
            ]
        );
    }

    #[test]
    fn every_card_has_an_installed_layout_name() {
        for card in cards(&summary(1, 1, 1, 4)) {
            assert!(!card.layout().is_empty());
        }
        assert_eq!(Card::Kills(1).event(), Some("crg_killedsomething"));
        assert_eq!(Card::Befriended(1).event(), Some("crg_befriendedsomething"));
        assert_eq!(Card::Brain(1).event(), None);
    }

    #[test]
    fn a_long_history_scrolls_instead_of_tightening() {
        // 14 cards of 105 px: the Creature column is wider than the screen, the
        // gap stays 5 and the scroll range is the overhang.
        let widths = [105.0f32; 14];
        let (at, extent) = axis::layout_events(&widths);
        assert_eq!(at[1] - at[0], 110.0);
        let columns = axis::stage_columns(CREATURE_STAGE, extent);
        assert_eq!(columns.left[CREATURE_STAGE], 5.0);
        let max = axis::max_scroll(columns.content_width(), axis::VIEW_WIDTH);
        assert_eq!(max, 5.0 + 5.0 + 14.0 * 110.0 + axis::RIGHT_MARGIN - 1280.0);
        assert_eq!(CARD_GAP, 5.0);
    }

    #[test]
    fn the_clock_follows_the_retail_date_formula() {
        // The Cell stage counts 1000 s at 10 000 years/s, then the Creature
        // stage 1000 years/s cut to 1 000 000.
        assert_eq!(clock_numbers(0.0), [0, 10, 0, 0]);
        assert_eq!(clock_numbers(600.0), [0, 10, 0, 0]);
        assert_eq!(clock_numbers(1800.0), [0, 11, 0, 0]);
    }

    /// The layouts, the trait files and the history events of the Creature
    /// stage exist in the install and carry what this module binds.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_creature_history_layouts_and_events_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        for name in [LAYOUT_BRAIN, LAYOUT_MIGRATED, LAYOUT_PACK, cell::LAYOUT_ICON, cell::LAYOUT_DEATH, cell::LAYOUT_LARGE] {
            let layout = assets.layout(&index, name).unwrap_or_else(|| panic!("{name}"));
            assert!(layout.find(card::ROOT).is_some(), "{name} root");
        }
        let mut cache = cell::TimelineCache::default();
        for name in ["crg_killedsomething", "crg_befriendedsomething"] {
            let event = cell::event_def(&mut cache, name, &index, &mut assets).unwrap_or_else(|| panic!("{name}"));
            assert!(event.0.is_some(), "{name} icon");
        }
        for instance in TRAITS {
            let key = ResourceKey {
                group_id: cell::TRAIT_GROUP,
                instance_id: instance,
                type_id: crate::asset_catalog::PROP,
            };
            let props = index.properties(key).expect("trait property");
            assert!(props.0.get(&cell::PROP_TRAIT_ICON).and_then(|p| p.key()).is_some());
        }
        let callout = dialogs::read_callout(&index, &mut assets, TRANSITION_CALLOUT).expect("history callout");
        assert_eq!(callout.title, "History");
        assert!(!callout.text.is_empty());
    }
}
