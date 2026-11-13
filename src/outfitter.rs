//! The Tribal, Civ and Space outfitters on the shared editor layer.
//!
//! Retail runs every creator through one `cEditor` engine; an editor differs
//! only by its setup PROP (`creature_editor_data`: palette, budget, camera,
//! scene, manipulators, currency). The outfitters are the creature editor
//! with `editorOnlyEditFromPalette`: the creature's own parts are fixed and
//! only accessories (`tribal_outfit_parts~`, palettes `!CA` / `!C9` / `!C8`)
//! are bought from the palette and placed on its body
//! (`notes/creator-re-notes.md` sections 0 and 5).
//!
//! The session reuses the Creature Creator's chrome, camera, scene, placement
//! and undo code, all of which work on `RunProgress::parts`. While it runs:
//!
//! * `run.parts` holds the creature's parts (the locked prefix, see
//!   `EditorState::locked_parts`) followed by the accessories being edited;
//!   `RunProgress::outfits.session` keeps the originals so a crash or a save
//!   written during the session never loses the creature (`repair`);
//! * a preview creature (the free creator's `MenuPreview` pattern: body mesh
//!   with the `Player` marker) stands in the isolated scene, the real player
//!   entities lose the marker and are hidden by `editor::scene`;
//! * the budget is the stage's currency (food in the Tribal stage, spice in
//!   Civ and Space), as in retail: buying spends it, selling refunds it, and
//!   Cancel restores everything.
//!
//! Accepting stores the accessories in `RunProgress::outfits` (the record the
//! stages read: `creature::mount_saved_parts` mounts them on every member).
//! The editor opens with F2 in these stages (the same key as the other
//! creators; new, the stages had no outfitter).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    creature::Accessory,
    creature_editor::CreatureEditor,
    creature_editor_data::{CreatureEditorData, SetupKind},
    creature_editor_paint::CreaturePaint,
    editor::EditorState,
    game::{Health, Hunger, Mover, PartInstance, Player, RunProgress},
    stages::{GameStage, StageOwned},
    Originals,
};

/// Stages whose creature editor is an outfitter.
pub fn routes(stage: &GameStage) -> bool {
    matches!(stage, GameStage::Tribal | GameStage::Civilization | GameStage::Space)
        // An outfitter opened from the galaxy screen's creator wheel.
        || (*stage == GameStage::MainMenu && crate::front_end::creators::menu_outfitter().is_some())
}

/// The editor setup of the stage's outfitter.
pub fn setup_of(stage: &GameStage) -> Option<SetupKind> {
    match stage {
        GameStage::Tribal => Some(SetupKind::Tribal),
        GameStage::Civilization => Some(SetupKind::Civ),
        GameStage::Space => Some(SetupKind::Space),
        GameStage::MainMenu => crate::front_end::creators::menu_outfitter(),
        _ => None,
    }
}

/// An outfitter session is running (the editor's part list is the creature's
/// parts plus accessories, and purchases come from the accessory group).
pub fn in_session(run: &RunProgress) -> bool {
    run.outfits.session.is_some()
}

/// Part PROP group of what the open editor sells: accessories in an outfitter
/// session, else the stage's own group (`editor::group`).
pub fn edit_group(run: &RunProgress, stage_group: u32) -> u32 {
    if in_session(run) {
        crate::asset_catalog::TOOL_PARTS
    } else {
        stage_group
    }
}

/// What a stage's outfitter saved: the accessories on the creature, their
/// tribe capability levels and the paint the outfitted creature wears.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Outfit {
    pub parts: Vec<PartInstance>,
    /// The outfit's tribe capability levels ([`levels_of`]); `None` until
    /// computed (records written before the levels existed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<TribeLevels>,
    /// The paint chosen in the outfitter's Paint mode (`None`: the creature's
    /// own paint). Retail repaints the creature and the accessories' tint
    /// masks with the same three colours (`notes/outfitter-notes.md` 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint: Option<CreaturePaint>,
}

/// What an interrupted session must give back (`repair`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionBackup {
    /// The creature's own parts.
    pub base: Vec<PartInstance>,
    pub dna: f32,
    pub focused_part: u32,
    /// The creature's own paint (the session paints the outfit).
    #[serde(default)]
    pub paint: Option<CreaturePaint>,
}

/// The outfitter records of a run (`RunProgress::outfits`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OutfitRecords {
    pub tribal: Outfit,
    pub civ: Outfit,
    pub space: Outfit,
    pub session: Option<SessionBackup>,
    /// Tribal accessories unlocked by resolved tribes ([`process_unlocks`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unlocked: Vec<u32>,
    /// Resolved tribes whose unlocks were handed out.
    pub unlock_events: u32,
}

// ---------------------------------------------------------------------------
// Tribe capability levels (VERIFIED, `notes/outfitter-notes.md` 2)
// ---------------------------------------------------------------------------

/// `modelCapabilityTribe*` part properties (ModAPI `EditorRigblock.h`); the
/// creature data carries them as the 4CC capabilities `tatk`, `tsoc`, `tarm`,
/// `tgth`, `tfsh` that the species profile reads (FUN_004D5080).
pub mod cap {
    pub const ATTACK: u32 = 0x055D_370E;
    pub const SOCIAL: u32 = 0x055D_3747;
    pub const ARMOR: u32 = 0x055D_374C;
    pub const GATHER: u32 = 0x055D_3750;
    pub const FISHING: u32 = 0x055D_3754;
}

/// The tribe levels of the outfitted species (`cSpeciesProfile` +0x628 attack,
/// +0x62C social, +0x630 armor, +0x634 gather, +0x638 fishing): per capability
/// the highest level among its blocks, at most 5 (FUN_004D3630 reads the
/// capability table's max entry, clamped to 5).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TribeLevels {
    pub attack: u8,
    pub social: u8,
    pub armor: u8,
    pub gather: u8,
    pub fishing: u8,
}

/// The cap of a species profile level (FUN_004D3630).
pub const LEVEL_CAP: u8 = 5;

impl TribeLevels {
    /// Levels of one part's properties, `[attack, social, armor, gather, fishing]`.
    pub fn merge(&mut self, levels: [u8; 5]) {
        let max = |a: &mut u8, b: u8| *a = (*a).max(b.min(LEVEL_CAP));
        max(&mut self.attack, levels[0]);
        max(&mut self.social, levels[1]);
        max(&mut self.armor, levels[2]);
        max(&mut self.gather, levels[3]);
        max(&mut self.fishing, levels[4]);
    }
}

/// `[attack, social, armor, gather, fishing]` of one accessory PROP.
pub fn part_levels(index: &crate::asset_catalog::ResourceIndex, group: u32, definition: u32) -> [u8; 5] {
    let Ok(p) = index.properties(crate::cell_editor_data::prop_key(group, definition)) else {
        return [0; 5];
    };
    let level = |id: u32| crate::cell_editor_data::number(&p, id).map_or(0, |v| v.round().clamp(0.0, 255.0) as u8);
    [level(cap::ATTACK), level(cap::SOCIAL), level(cap::ARMOR), level(cap::GATHER), level(cap::FISHING)]
}

/// The tribe levels an outfit gives the species.
pub fn levels_of(originals: &Originals, parts: &[PartInstance]) -> TribeLevels {
    let mut out = TribeLevels::default();
    for p in parts {
        if let Some(def) = originals.part_catalog.by_part_in(crate::asset_catalog::TOOL_PARTS, p) {
            out.merge(part_levels(&originals.resource_index, def.key.group_id, def.key.instance_id));
        }
    }
    out
}

/// The tribe levels of the player's tribe: the Tribal outfit's (the other
/// stages' accessories carry no simulation effect: `CAE_VerbTrayCollection`
/// is empty and no Civ / Space code reads the levels).
pub fn tribe_levels(run: &RunProgress) -> TribeLevels {
    run.outfits.tribal.levels.unwrap_or_default()
}

/// `creatureHealthIncrement` (speciestuning, 2): hit points per armor level
/// (FUN_004D5080 at 0x4D7EC6: `mAdditionalHealth += mTribeArmorLevel x
/// creatureHealthIncrement`; the citizen's max HP 0xC22880 adds it).
pub fn armor_health(armor: u8) -> f32 {
    crate::creature_rules::HEALTH_INCREMENT * armor.min(LEVEL_CAP) as f32
}

/// `gatherCapacityForAccessories`, `meatCapacityForAccessories`,
/// `fishingCapacityForAccessories`, `eggCapacityForAccessories` (TribeTuning,
/// all `[1, 2, 3, 4, 5]`): items a bundle gains at gather level 1..5. Retail
/// (0xC226C0, the citizen's carry capacity) indexes all four with the tribe's
/// *gather* level (+0x634), the fishing level is not read.
pub const CAPACITY_FOR_ACCESSORIES: [f32; 5] = [1.0, 2.0, 3.0, 4.0, 5.0];

/// Food in a full bundle at the tribe's gather level:
/// `carryBundleAmountMax_<kind> + capacity[gather - 1]` items (0xC226C0), at
/// most `carryBundleAmountMax` food (the port's bundle rule,
/// `tribal_rules::bundle_food`).
pub fn bundle_food(haul: crate::tribal_rules::Haul, gather: u8) -> f32 {
    use crate::tribal_rules::{bundle_items, food_per_item, CARRY_BUNDLE_AMOUNT_MAX};
    let extra = match gather.min(LEVEL_CAP) {
        0 => 0.0,
        g => CAPACITY_FOR_ACCESSORIES[g as usize - 1],
    };
    ((bundle_items(haul) + extra) * food_per_item(haul)).min(CARRY_BUNDLE_AMOUNT_MAX)
}

/// `scoreContributionPerAccessoryLevel` (TribeTuning 35): the social
/// minigame's base score per tribe social level (FUN_00D9CCF0, added once
/// when the performance starts, with `scoreContributionPerCreature` 25 per
/// member).
pub const SCORE_PER_SOCIAL_LEVEL: f32 = 35.0;
/// The score a fully answered beat adds (FUN_00DA0100: the performance's
/// +0x3C, initial 175 = TribeTuning 0x459CE162).
pub const SCORE_PER_BEAT: f32 = 175.0;

/// Score the social accessories add to a performance when it starts (0xD9CCF0:
/// the species' social level times `scoreContributionPerAccessoryLevel` 35,
/// level 5 = 175 = one whole beat); `tribal::TribalCampaign::outfit_social_bonus`.
pub fn social_head_start(social: u8) -> f32 {
    social.min(LEVEL_CAP) as f32 * SCORE_PER_SOCIAL_LEVEL
}

/// TribeTuning 0x895B917A `[0, 5, 5, 6, 7, 0]`: accessories unlocked when the
/// stage starts (index 0) and when the n-th tribe is allied or conquered
/// (index n; FUN_00CD9A60 from the totem 0xCDAC90).
pub const UNLOCKS_PER_TRIBE: [u32; 6] = [0, 5, 5, 6, 7, 0];

/// Hand out the accessories of the tribes resolved since the last call
/// (FUN_00CD9A60: for each, `UNLOCKS_PER_TRIBE[n]` times FUN_00597910 picks a
/// random palette row that still has a locked item and unlocks its next
/// column). ADAPTED: a seeded pick instead of `Math::sMainRNG`; the unlocks are
/// resolved when the outfitter opens (only the outfitter shows them).
/// Returns the definitions unlocked now.
pub fn process_unlocks(run: &mut RunProgress, data: &CreatureEditorData) -> Vec<u32> {
    let mut out = Vec::new();
    let resolved = run.tribal_campaign.resolved_count() as u32;
    let Some(palette) = data.parts_palette(SetupKind::Tribal) else { return out };
    while run.outfits.unlock_events < resolved {
        run.outfits.unlock_events += 1;
        let n = UNLOCKS_PER_TRIBE.get(run.outfits.unlock_events as usize).copied().unwrap_or(0);
        for i in 0..n {
            // Rows (category, page, row) with a locked item, in palette order.
            let mut rows: Vec<Vec<(u32, u32)>> = Vec::new();
            for c in &palette.categories {
                for &pi in &c.pages {
                    let Some(page) = palette.pages.get(pi) else { continue };
                    let mut by_row: std::collections::BTreeMap<u32, Vec<(u32, u32)>> = Default::default();
                    for item in &page.items {
                        by_row.entry(item.row).or_default().push((item.column, item.part));
                    }
                    for (_, mut items) in by_row {
                        items.sort();
                        let locked = items.iter().any(|(_, part)| {
                            !run.outfits.unlocked.contains(part) && data.parts.get(part).is_some_and(|x| !x.starts_unlocked)
                        });
                        if locked {
                            rows.push(items);
                        }
                    }
                }
            }
            if rows.is_empty() {
                break;
            }
            let seed = (run.outfits.unlock_events as u64) << 32 | i as u64;
            let pick = (splitmix(seed ^ 0x0F17_7E85) % rows.len() as u64) as usize;
            if let Some(&(_, part)) = rows[pick]
                .iter()
                .find(|(_, part)| !run.outfits.unlocked.contains(part) && data.parts.get(part).is_some_and(|x| !x.starts_unlocked))
            {
                run.outfits.unlocked.push(part);
                out.push(part);
            }
        }
    }
    out
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// The paint the creature shows now: the session's while an outfitter runs,
/// else the current stage's outfit paint, else the creature's own.
pub fn shown_paint(run: &RunProgress) -> &CreaturePaint {
    if !in_session(run) {
        if let Some(p) = run.outfits.of(&run.current_stage()).and_then(|o| o.paint.as_ref()) {
            return p;
        }
    }
    &run.creature_creation.paint
}

impl OutfitRecords {
    pub fn of(&self, stage: &GameStage) -> Option<&Outfit> {
        match &crate::front_end::creators::outfit_stage(stage) {
            GameStage::Tribal => Some(&self.tribal),
            GameStage::Civilization => Some(&self.civ),
            GameStage::Space => Some(&self.space),
            _ => None,
        }
    }
    pub fn of_mut(&mut self, stage: &GameStage) -> Option<&mut Outfit> {
        match &crate::front_end::creators::outfit_stage(stage) {
            GameStage::Tribal => Some(&mut self.tribal),
            GameStage::Civilization => Some(&mut self.civ),
            GameStage::Space => Some(&mut self.space),
            _ => None,
        }
    }
}

/// The accessories the current stage's members wear (mounted by
/// `creature::mount_saved_parts`).
pub fn worn(run: &RunProgress) -> &[PartInstance] {
    run.outfits.of(&run.current_stage()).map_or(&[], |o| o.parts.as_slice())
}

/// Currency of the stage's outfitter (`editorCurrencyChar`: food in the Tribal
/// stage, Sporebucks in Civ and Space; the stages keep them in
/// `dna_this_stage` (food) and `spice`).
pub fn currency(run: &RunProgress, stage: &GameStage) -> f32 {
    match stage {
        GameStage::Tribal => run.dna_this_stage,
        _ => run.spice,
    }
}

fn set_currency(run: &mut RunProgress, stage: &GameStage, value: f32) {
    let value = value.max(0.0);
    match stage {
        GameStage::Tribal => run.dna_this_stage = value,
        _ => run.spice = value,
    }
}

/// Accessory availability (`cCollectableItems`, loaded for the Tribal stage
/// from `TribeGame!CollectablePartsConfig` by 0xCDE470): items with
/// `itemUnlockFindPercentage` 1 are unlocked from the start
/// (`AddUnlockableItemFromProp`), the others when resolved tribes hand them out
/// ([`process_unlocks`], VERIFIED). INFERRED: Civ and Space load no
/// collectable config (`CivGame` / `SpaceGame!CollectablePartsConfig` do not
/// exist), so their outfitters sell every accessory.
pub fn unlocked(run: &RunProgress, info: &crate::creature_editor_data::CreaturePartInfo) -> bool {
    if info.starts_unlocked {
        return true;
    }
    match run.current_stage() {
        GameStage::Tribal => run.outfits.unlocked.contains(&info.key.instance_id),
        _ => true,
    }
}

/// Where the preview creature stands (ADAPTED: the retail editor has its own
/// world; here the stage's world is hidden and left far below).
pub const PREVIEW_AT: Vec3 = Vec3::new(0.0, 4000.0, 0.0);

/// Marker of the creature standing in the outfitter's scene.
#[derive(Component)]
pub struct OutfitterPreview;

/// An accessory mounted on a member of the stage (`creature::mount_saved_parts`).
#[derive(Component)]
pub struct OutfitMounted;

/// Import the models of the accessories the stage's members wear (before the
/// stage mounts them).
pub fn load_worn(
    run: &RunProgress,
    originals: &mut Originals,
    stage: &GameStage,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Some(outfit) = run.outfits.of(stage) else { return };
    let definitions: Vec<_> = outfit
        .parts
        .iter()
        .filter_map(|p| originals.part_catalog.by_part_in(crate::asset_catalog::TOOL_PARTS, p).cloned())
        .collect();
    for definition in definitions {
        if let Err(err) = crate::editor::ensure_model(originals, &definition, meshes, images, materials) {
            warn!("outfit part import: {err}");
        }
    }
}

/// A real player entity that lost its `Player` marker for the session.
#[derive(Component)]
pub struct OutfitterShelved;

/// Runtime state of the running session.
#[derive(Resource, Default)]
pub struct Outfitter {
    pub active: bool,
    pub stage: Option<GameStage>,
    /// `editor_dna` and the stage currency when the session opened.
    dna_open: f32,
    currency_open: f32,
    /// Visible descendants of the shelved players hidden for the session.
    hidden: Vec<Entity>,
    /// `editorBackgroundModel` and `editorPedestalModel` of the setup (model
    /// instances in `editor_rigblocks~`), drawn by `editor_environment`.
    pub scene: Vec<u32>,
}

/// Put the creature back after a session that did not end (a save written
/// during the session, a crash): the creature's parts return, the session's
/// edits are dropped.
pub fn repair(run: &mut RunProgress) -> bool {
    let Some(backup) = run.outfits.session.take() else {
        return false;
    };
    run.parts = backup.base;
    run.editor_dna = backup.dna;
    run.focused_part = backup.focused_part;
    if let Some(paint) = backup.paint {
        run.creature_creation.paint = paint;
    }
    run.sync_legacy_accessory();
    run.request_save();
    true
}

/// Price of a worn outfit (a symmetric pair is one purchase).
pub fn outfit_cost(data: &CreatureEditorData, parts: &[PartInstance]) -> f32 {
    let mut seen: Vec<u32> = Vec::new();
    let mut total = 0.0;
    for p in parts {
        if p.pair_id != 0 {
            if seen.contains(&p.pair_id) {
                continue;
            }
            seen.push(p.pair_id);
        }
        total += data.info_of(p).map_or(0, |i| i.price) as f32;
    }
    total
}

pub struct OutfitterPlugin;

impl Plugin for OutfitterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Outfitter>().add_systems(
            Update,
            (repair_system, sync)
                .chain()
                .after(crate::editor::input)
                .before(crate::editor::scene),
        )
        .add_systems(Update, hold_scene.after(crate::editor::scene).before(crate::editor::drag))
        .add_systems(Update, (apply_worn, refresh_levels, tribe_effects).chain().after(sync))
        .add_systems(PostUpdate, tint_accessories.run_if(resource_exists::<Originals>))
        .add_systems(Startup, demo_seed.after(crate::load_original_models));
    }
}

/// Capture option `--outfit-demo`: a run without parts gets the starter
/// parts before the stage spawns its members (so they wear them).
#[allow(clippy::type_complexity)]
fn demo_seed(
    run: Option<ResMut<RunProgress>>,
    originals: Option<ResMut<Originals>>,
    assets: (Option<ResMut<Assets<Mesh>>>, Option<ResMut<Assets<Image>>>, Option<ResMut<Assets<StandardMaterial>>>),
) {
    if !std::env::args().any(|a| a == "--outfit-demo") {
        return;
    }
    let (Some(mut run), Some(mut originals), (Some(mut meshes), Some(mut images), Some(mut materials))) = (run, originals, assets) else {
        return;
    };
    if originals.part_catalog.parts.is_empty() {
        return;
    }
    if run.parts.is_empty() {
        run.parts = crate::editor::starter_creature_parts(&run, &originals.part_catalog);
    }
    for p in run.parts.clone() {
        if let Some(d) = originals.part_catalog.by_part_in(crate::asset_catalog::CREATURE_PARTS, &p).cloned() {
            let _ = crate::editor::ensure_model(&mut originals, &d, &mut meshes, &mut images, &mut materials);
        }
    }
    // `--outfit-demo-worn`: every outfit stage starts outfitted (an
    // accessory on the head and a paint), as if its outfitter had been
    // visited.
    if !std::env::args().any(|a| a == "--outfit-demo-worn") {
        return;
    }
    let data = CreatureEditorData::load(&originals.resource_index);
    let paint_data = crate::creature_editor_paint::CreaturePaintData::load(&originals.resource_index);
    let native = crate::creature_editor_paint::native_skin_colors(&originals).map(|c| c.map(|v| v as f32 / 255.0));
    for (i, stage) in [GameStage::Tribal, GameStage::Civilization, GameStage::Space].into_iter().enumerate() {
        let Some(kind) = setup_of(&stage) else { continue };
        let mut scratch = run.clone();
        demo_accessory(&mut scratch, &data, kind, &mut originals, &mut meshes, &mut images, &mut materials);
        let worn = scratch.parts[run.parts.len()..].to_vec();
        let paint = paint_data.randomize(&run.creature_creation.paint, native, 0x5BACE + i as u64);
        let levels = levels_of(&originals, &worn);
        if let Some(o) = run.outfits.of_mut(&stage) {
            o.parts = worn;
            o.levels = Some(levels);
            o.paint = paint;
        }
    }
}

/// Records written before the levels existed get them (the outfit's PROPs).
fn refresh_levels(mut run: ResMut<RunProgress>, originals: Option<Res<Originals>>) {
    let Some(originals) = originals else { return };
    if originals.part_catalog.parts.is_empty() {
        return;
    }
    for stage in [GameStage::Tribal, GameStage::Civilization, GameStage::Space] {
        let stale = run.outfits.of(&stage).is_some_and(|o| o.levels.is_none() && !o.parts.is_empty());
        if stale {
            let levels = levels_of(&originals, &run.outfits.of(&stage).unwrap().parts);
            if let Some(o) = run.bypass_change_detection().outfits.of_mut(&stage) {
                o.levels = Some(levels);
            }
        }
    }
}

/// Armor hit points already given to a tribe unit ([`armor_health`]).
#[derive(Component, Default)]
pub struct ArmorBonus(pub f32);

/// The Tribal outfit's simulation effects that live outside the rules
/// modules: the armor's hit points on every unit of the player's tribe
/// (members and the chieftain, x `chieftainHealthScale` like the rest of his
/// hit points; INFERRED: the scale's use is not traced) and the social bonus
/// the performance reads (`tribal::TribalCampaign::outfit_social_bonus`).
/// Gathering reads [`tribe_levels`] directly (`tribal_rts`).
#[allow(clippy::type_complexity)]
fn tribe_effects(
    mut commands: Commands,
    stage: Option<Res<State<GameStage>>>,
    outfitter: Res<Outfitter>,
    mut run: ResMut<RunProgress>,
    mut units: Query<
        (Entity, &crate::tribal_rts::TribeUnit, &mut Health, Option<&mut ArmorBonus>),
        Without<crate::tribal_rts::RivalMember>,
    >,
) {
    if stage.is_none_or(|s| *s.get() != GameStage::Tribal) || outfitter.active {
        return;
    }
    let levels = tribe_levels(&run);
    let social = social_head_start(levels.social);
    if run.tribal_campaign.outfit_social_bonus != social {
        run.bypass_change_detection().tribal_campaign.outfit_social_bonus = social;
    }
    let armor = armor_health(levels.armor);
    for (e, unit, mut health, bonus) in &mut units {
        let want = if unit.chieftain { crate::tribal_rules::chieftain_health(armor) } else { armor };
        let had = bonus.as_ref().map_or(0.0, |b| b.0);
        if (want - had).abs() < 1e-4 {
            continue;
        }
        let delta = want - had;
        health.max = (health.max + delta).max(1.0);
        health.hp = (health.hp + delta.max(0.0)).min(health.max);
        match bonus {
            Some(mut b) => b.0 = want,
            None => {
                commands.entity(e).insert(ArmorBonus(want));
            }
        }
    }
}

/// Every accessory model is tinted through its own tint mask with the colours
/// the creature shows ([`shown_paint`], else the installed creature's skin
/// colours), as retail paints the accessories with the creature's paint.
/// (Their import used the editor's fixed default colours.)
fn tint_accessories(
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut images: ResMut<Assets<Image>>,
    materials: Res<Assets<StandardMaterial>>,
    mut done: Local<std::collections::HashMap<AssetId<Image>, [[u8; 3]; 3]>>,
    mut native: Local<Option<[[u8; 3]; 3]>>,
) {
    use crate::asset_catalog::TOOL_PARTS;
    if originals.models.is_empty() {
        return;
    }
    let paint = shown_paint(&run);
    let native = *native.get_or_insert_with(|| crate::creature_editor_paint::native_skin_colors(&originals));
    let colors = if paint.painted { paint.colors.map(|c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)) } else { native };
    for model in &originals.models {
        let Some(key) = model.source_definition.filter(|k| k.group_id == TOOL_PARTS) else { continue };
        let Some(handle) = model.material.as_ref().and_then(|m| materials.get(m)).and_then(|m| m.base_color_texture.clone()) else {
            continue;
        };
        if done.get(&handle.id()) == Some(&colors) {
            continue;
        }
        done.insert(handle.id(), colors);
        let Some(def) = originals.part_catalog.parts.iter().find(|d| d.key == key) else { continue };
        let read = |k: Option<crate::dbpf::ResourceKey>| {
            k.and_then(|k| originals.resource_index.read(k).ok()).and_then(|b| crate::texture::decode_rw4(&b))
        };
        let (Some(diffuse), Some(mask)) = (read(def.diffuse), read(def.tint)) else { continue };
        let painted = match &originals.native_painter {
            Some(painter) => painter.preview_part(&diffuse, &mask, colors).unwrap_or_else(|_| crate::texture::skinpaint(&diffuse, Some(&mask), colors)),
            None => crate::texture::skinpaint(&diffuse, Some(&mask), colors),
        };
        if let Some(mut image) = images.get_mut(&handle) {
            if image.width() == painted.width && image.height() == painted.height {
                let levels = image.texture_descriptor.mip_level_count;
                let (w, h) = (painted.width, painted.height);
                image.data = Some(if levels > 1 { crate::cell_paint::with_mips(&painted.rgba, w, h, levels) } else { painted.rgba });
            }
        }
    }
}

/// `editor::scene` hides what exists when the editor opens; the stages keep
/// spawning (the Space stage's ships), so what appears later is hidden too.
fn hold_scene(
    mut commands: Commands,
    outfitter: Res<Outfitter>,
    roots: Query<
        (Entity, Option<&Visibility>),
        (
            With<StageOwned>,
            With<Transform>,
            Without<ChildOf>,
            Without<Node>,
            Without<Player>,
            Without<Accessory>,
            Without<OutfitterPreview>,
            Without<crate::editor::EditorHidden>,
        ),
    >,
) {
    if !outfitter.active {
        return;
    }
    for (e, visibility) in &roots {
        commands
            .entity(e)
            .insert((crate::editor::EditorHidden(visibility.copied().unwrap_or(Visibility::Inherited)), Visibility::Hidden));
    }
}

/// The outfit changed (a session was accepted): the members of the stage
/// wear the new accessories. The stage mounts them itself when it spawns a
/// member (`creature::mount_saved_parts`).
#[allow(clippy::too_many_arguments)]
fn apply_worn(
    mut commands: Commands,
    run: Res<RunProgress>,
    outfitter: Res<Outfitter>,
    originals: Res<Originals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    actors: Query<Entity, (Or<(With<Player>, With<crate::game::TribeMate>)>, Without<OutfitterPreview>)>,
    mounted: Query<(Entity, &ChildOf), With<OutfitMounted>>,
    mut last: Local<Option<(usize, Vec<PartInstance>)>>,
) {
    if outfitter.active {
        return;
    }
    let stage = run.current_stage();
    let now = (run.stage_order_index, worn(&run).to_vec());
    let Some(previous) = last.replace(now.clone()) else { return };
    if previous == now || previous.0 != now.0 || stage != GameStage::Tribal {
        return;
    }
    for actor in &actors {
        for (child, parent) in &mounted {
            if parent.parent() == actor {
                commands.entity(child).despawn();
            }
        }
        crate::creature::mount_outfit(&mut commands, &mut materials, actor, &originals, &run, 0);
    }
}

fn repair_system(outfitter: Res<Outfitter>, mut run: ResMut<RunProgress>) {
    if !outfitter.active && run.outfits.session.is_some() {
        repair(&mut run);
    }
}

/// Begin and end the session with the editor's `open` flag (F2 in these
/// stages, `editor::input`).
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    mut state: ResMut<EditorState>,
    mut outfitter: ResMut<Outfitter>,
    mut data: ResMut<CreatureEditorData>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    editor: Res<CreatureEditor>,
    players: Query<Entity, With<Player>>,
    shelved: Query<Entity, With<OutfitterShelved>>,
    children: Query<&Children>,
    mut visibility: Query<&mut Visibility>,

    leftovers: Query<Entity, Or<(With<OutfitterPreview>, With<Accessory>, With<crate::editor::SpineHandle>)>>,
) {
    let here = stage.get().clone();
    if !outfitter.active {
        // The vehicle / building / UFO editors share F3 and `state.open`.
        if !(state.open && routes(&here)) || state.rigid {
            return;
        }
        if !data.loaded && !originals.resource_index.records.is_empty() {
            *data = CreatureEditorData::load(&originals.resource_index);
            for e in &data.errors {
                warn!("creature editor data: {e}");
            }
        }
        let Some(kind) = setup_of(&here) else { return };
        // Captures (`--outfit-demo`): a creature without parts gets the
        // starter parts.
        let demo = std::env::args().any(|a| a == "--outfit-demo");
        if demo && run.parts.is_empty() {
            run.parts = crate::editor::starter_creature_parts(&run, &originals.part_catalog);
            for p in run.parts.clone() {
                if let Some(d) = originals.part_catalog.by_part_in(crate::asset_catalog::CREATURE_PARTS, &p).cloned() {
                    let _ = crate::editor::ensure_model(&mut originals, &d, &mut meshes, &mut images, &mut materials);
                }
            }
        }
        // Accessories the resolved tribes unlocked since the last visit.
        if here == GameStage::Tribal {
            let new = process_unlocks(&mut run, &data);
            if !new.is_empty() {
                info!("outfitter: {} accessories unlocked by resolved tribes", new.len());
            }
        }
        // The creature's parts stay; the accessories follow them.
        let base = run.parts.clone();
        let worn_parts = run.outfits.of(&here).map(|o| o.parts.clone()).unwrap_or_default();
        let own_paint = run.creature_creation.paint.clone();
        run.outfits.session = Some(SessionBackup {
            base: base.clone(),
            dna: run.editor_dna,
            focused_part: run.focused_part,
            paint: Some(own_paint.clone()),
        });
        // The session paints the outfit: it starts from the outfit's paint.
        run.creature_creation.paint = run.outfits.of(&here).and_then(|o| o.paint.clone()).unwrap_or(own_paint);
        outfitter.scene = data
            .setup(kind)
            .map(|s| [s.background_model, s.pedestal_model].into_iter().flatten().collect())
            .unwrap_or_default();
        outfitter.currency_open = currency(&run, &here);
        let max = data.setup(kind).and_then(|s| s.max_budget).unwrap_or(9999) as f32;
        outfitter.dna_open = outfitter.currency_open.clamp(0.0, max);
        run.editor_dna = outfitter.dna_open;
        run.focused_part = (base.len() + worn_parts.len()).saturating_sub(1) as u32;
        state.locked_parts = base.len();
        run.parts.extend(worn_parts);
        run.sync_legacy_accessory();
        // Import the accessories on the creature.
        let definitions: Vec<_> = run
            .parts
            .iter()
            .skip(base.len())
            .filter_map(|p| originals.part_catalog.by_part_in(crate::asset_catalog::TOOL_PARTS, p).cloned())
            .collect();
        for definition in definitions {
            if let Err(err) = crate::editor::ensure_model(&mut originals, &definition, &mut meshes, &mut images, &mut materials) {
                warn!("outfit part import: {err}");
            }
        }
        // The isolated scene: the creature stands in a preview (the real
        // player entities lose the marker and are hidden by `editor::scene`).
        for e in &players {
            commands.entity(e).remove::<Player>().insert(OutfitterShelved);
            // Children that were made explicitly visible (a ship's model)
            // would show through the hidden parent.
            let mut stack = vec![e];
            while let Some(parent) = stack.pop() {
                if let Ok(kids) = children.get(parent) {
                    for kid in kids.iter() {
                        stack.push(kid);
                        if let Ok(mut v) = visibility.get_mut(kid) {
                            if *v == Visibility::Visible {
                                *v = Visibility::Hidden;
                                outfitter.hidden.push(kid);
                            }
                        }
                    }
                }
            }
        }
        let body = crate::baked_model::prepare_body(&mut originals, &run.body, &mut meshes, &mut images, &mut materials).ok();
        commands.spawn((
            StageOwned,
            OutfitterPreview,
            Player,
            Mover { speed: 0. },
            Health::new(120.),
            Hunger::new(120.),
            Visibility::Inherited,
            Mesh3d(
                body.as_ref()
                    .map(|m| m.draw_mesh())
                    .unwrap_or_else(|| meshes.add(crate::body::mesh(&run.body))),
            ),
            MeshMaterial3d(body.and_then(|m| m.material).unwrap_or_else(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.43, 0.68, 0.33),
                    perceptual_roughness: 0.6,
                    ..default()
                })
            })),
            // Far above the stage, so nothing of the stage can be in view.
            Transform::from_translation(PREVIEW_AT),
            Name::new("outfitter preview creature"),
        ));
        if demo {
            demo_accessory(&mut run, &data, kind, &mut originals, &mut meshes, &mut images, &mut materials);
        }
        state.visuals_dirty = true;
        outfitter.active = true;
        outfitter.stage = Some(here);
        info!("outfitter opened ({kind:?}): {} creature parts, {} accessories", base.len(), run.parts.len() - base.len());
        return;
    }
    if state.open && routes(&here) && outfitter.stage.as_ref() == Some(&here) {
        return;
    }
    // End of the session: Cancel keeps the record, anything else stores it.
    let stage = outfitter.stage.take().unwrap_or(here);
    if let Some(backup) = run.outfits.session.take() {
        let base_len = backup.base.len().min(run.parts.len());
        let own_paint = backup.paint.clone().unwrap_or_else(|| run.creature_creation.paint.clone());
        if !editor.cancelled {
            let accessories = run.parts[base_len..].to_vec();
            let spent = run.editor_dna - outfitter.dna_open;
            let levels = levels_of(&originals, &accessories);
            let paint = run.creature_creation.paint.clone();
            if let Some(outfit) = run.outfits.of_mut(&stage) {
                outfit.parts = accessories;
                outfit.levels = Some(levels);
                outfit.paint = (paint != own_paint).then_some(paint);
            }
            let money = outfitter.currency_open + spent;
            set_currency(&mut run, &stage, money);
            info!("outfitter accepted ({stage:?}): tribe levels {levels:?}");
        }
        run.parts = backup.base;
        run.editor_dna = backup.dna;
        run.focused_part = backup.focused_part;
        run.creature_creation.paint = own_paint;
        run.sync_legacy_accessory();
        run.request_save();
    }
    state.locked_parts = 0;
    outfitter.active = false;
    for e in &leftovers {
        commands.entity(e).despawn();
    }
    for e in &shelved {
        commands.entity(e).remove::<OutfitterShelved>().insert(Player);
    }
    for e in std::mem::take(&mut outfitter.hidden) {
        if let Ok(mut v) = visibility.get_mut(e) {
            *v = Visibility::Visible;
        }
    }
    state.visuals_dirty = true;
}

/// Capture helper (`--outfit-demo`): buy the first available item of the
/// first page and drop it on the top of the head, as a palette drag would.
fn demo_accessory(
    run: &mut RunProgress,
    data: &CreatureEditorData,
    kind: SetupKind,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Some(item) = data
        .parts_palette(kind)
        .and_then(|p| p.pages.iter().find(|pg| !pg.items.is_empty()))
        .and_then(|pg| pg.items.iter().find(|i| data.parts.get(&i.part).is_some_and(|x| unlocked(run, x))))
        .cloned()
    else {
        return;
    };
    let Some(definition) = originals.part_catalog.by_definition_in(item.group, item.part).cloned() else {
        return;
    };
    if let Err(err) = crate::editor::ensure_model(originals, &definition, meshes, images, materials) {
        warn!("outfit demo import: {err}");
        return;
    }
    let bone = 7;
    let mut part = PartInstance::new(definition.model.instance_id, bone).with_definition(definition.key.instance_id);
    let tris = crate::baked_model::body_triangles(originals, &run.body);
    let head = crate::baked_model::spine_center(originals, &run.body, run.body.spine.len().saturating_sub(1)).0;
    if let Some((_, point, _)) = crate::creature_editor_edit::ray_hull(&tris, head + Vec3::Y * 5.0, Vec3::NEG_Y) {
        let slot = crate::baked_model::attachment_point(originals, &run.body, bone);
        part.offset = crate::game::clamp_offset((point - slot).to_array(), crate::game::OFFSET_LIMIT);
    }
    run.focused_part = run.parts.len() as u32;
    run.parts.push(part);
    run.editor_dna -= data.parts.get(&item.part).map_or(0, |i| i.price) as f32;
}

// ---------------------------------------------------------------------------
// Render checks (GPU test)
// ---------------------------------------------------------------------------

/// Drive the stage's outfitter through its real chrome in the GPU render test
/// (`runtime::render_original_install`): F2 opens it, a palette accessory is
/// bought (one price), undone, redone, sold back, bought again, captured, and
/// F2 accepts: the record is stored and the stage's members wear it.
pub(crate) fn render_smoke_checks(
    app: &mut App,
    window: Entity,
    step: &dyn Fn(&mut App),
    capture: &dyn Fn(&mut App, &str),
) {
    use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};
    use crate::{creature_editor::{refusal, Refusal}, editor::Action};
    let press = |app: &mut App, keys: &[KeyCode]| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            for k in keys {
                app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
                    key_code: *k,
                    logical_key: Key::Character("".into()),
                    state,
                    text: None,
                    repeat: false,
                    window,
                });
            }
            step(app);
        }
        step(app);
    };
    let stage = app.world().resource::<State<GameStage>>().get().clone();
    let kind = setup_of(&stage).expect("an outfitter stage");
    {
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        run.dna_this_stage = 1000.0;
        run.spice = 1000.0;
    }
    let base = app.world().resource::<RunProgress>().parts.clone();
    press(app, &[KeyCode::F2]);
    for _ in 0..12 {
        step(app);
    }
    assert!(app.world().resource::<Outfitter>().active, "{stage:?}: F2 opens the outfitter");
    assert!(app.world().resource::<CreatureEditor>().active, "{stage:?}: the editor session runs");
    assert_eq!(app.world().resource::<CreatureEditor>().kind, kind);
    assert_eq!(app.world().resource::<RunProgress>().parts, base, "the creature's parts lead the list");
    let data = app.world().resource::<CreatureEditorData>().clone();
    assert!(data.loaded, "creature editor data");
    let palette = data.parts_palette(kind).cloned().expect("the outfitter palette");
    assert_eq!(palette.categories.len(), 6, "hats, details, chest, shoulders, masks, symbols");
    {
        let world = app.world_mut();
        let roots = world.query::<&crate::creature_editor_ui::EditorRoot>().iter(world).count();
        assert!(roots >= 6, "the retail editor layouts are spawned ({roots})");
        let mut players = world.query_filtered::<Entity, With<Player>>();
        assert_eq!(players.iter(world).count(), 1, "the preview creature is the only player");
    }
    // Buy the first affordable accessory of the first page.
    let item = {
        let run = app.world().resource::<RunProgress>();
        let originals = app.world().resource::<Originals>();
        palette
            .pages
            .iter()
            .flat_map(|p| p.items.iter())
            .find(|i| refusal(&data, kind, run, originals, i.part).is_none())
            .cloned()
            .expect("an accessory the outfitter sells")
    };
    let price = data.parts[&item.part].price as f32;
    let dna = app.world().resource::<RunProgress>().editor_dna;
    assert_eq!(dna, 1000.0, "the budget is the stage currency");
    app.world_mut().resource_mut::<EditorState>().queued.push(Action::Buy(item.part));
    step(app);
    step(app);
    let bought = app.world().resource::<RunProgress>().parts.clone();
    assert!(bought.len() > base.len(), "the accessory is bought");
    assert_eq!(bought[..base.len()], base[..], "the creature's own parts are untouched");
    assert_eq!(app.world().resource::<RunProgress>().editor_dna, dna - price, "one price (a symmetric pair is one purchase)");
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert_eq!(app.world().resource::<RunProgress>().parts, base, "undo");
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
    assert_eq!(app.world().resource::<RunProgress>().parts, bought, "redo");
    press(app, &[KeyCode::Delete]);
    assert_eq!(app.world().resource::<RunProgress>().parts, base, "Delete sells the accessory back");
    assert_eq!(app.world().resource::<RunProgress>().editor_dna, dna, "full refund");
    // The creature's own part cannot be sold.
    if !base.is_empty() {
        app.world_mut().resource_mut::<RunProgress>().focused_part = 0;
        press(app, &[KeyCode::Delete]);
        assert_eq!(app.world().resource::<RunProgress>().parts, base, "the creature's part stays");
    }
    // Buy again, look at it, accept.
    app.world_mut().resource_mut::<EditorState>().queued.push(Action::Buy(item.part));
    for _ in 0..10 {
        step(app);
    }
    capture(app, &format!("{}-outfitter.png", format!("{stage:?}").to_lowercase()));
    let worn = app.world().resource::<RunProgress>().parts[base.len()..].to_vec();
    assert!(!worn.is_empty());
    // Paint mode: the retail creature paint palette paints the outfitted
    // creature and its accessories.
    let own_paint = app.world().resource::<RunProgress>().creature_creation.paint.clone();
    app.world_mut().resource_scope(|world, mut editor: Mut<CreatureEditor>| {
        let state = world.resource::<EditorState>();
        let data = world.resource::<CreatureEditorData>();
        let originals = world.resource::<Originals>();
        crate::creature_editor::set_mode(&mut editor, state, data, originals, crate::creature_editor::Mode::Paint);
    });
    for _ in 0..6 {
        step(app);
    }
    let painted = {
        let paint_data = app.world().resource::<crate::creature_editor_paint::CreaturePaintData>().clone();
        let originals = app.world().resource::<Originals>();
        let native = crate::creature_editor_paint::native_skin_colors(originals).map(|c| c.map(|v| v as f32 / 255.0));
        paint_data.randomize(&own_paint, native, 0x0FF1_77E5).expect("the creature paint palette loads in the outfitter")
    };
    app.world_mut().resource_mut::<RunProgress>().creature_creation.paint = painted.clone();
    for _ in 0..12 {
        step(app);
    }
    {
        let world = app.world_mut();
        let roots = world.query::<&crate::creature_editor_paint::PaintRoot>().iter(world).count();
        assert!(roots >= 3, "{stage:?}: the paint palette is shown ({roots} roots)");
    }
    capture(app, &format!("{}-outfitter-paint.png", format!("{stage:?}").to_lowercase()));
    press(app, &[KeyCode::F2]);
    for _ in 0..12 {
        step(app);
    }
    assert!(!app.world().resource::<Outfitter>().active, "F2 closes the outfitter");
    let run = app.world().resource::<RunProgress>();
    assert_eq!(run.parts, base, "the creature is restored");
    assert_eq!(run.outfits.of(&stage).unwrap().parts, worn, "the accessories are stored in the stage's record");
    assert_eq!(run.outfits.of(&stage).unwrap().paint.as_ref(), Some(&painted), "the outfit keeps its paint");
    assert_eq!(run.creature_creation.paint, own_paint, "the creature's own paint is back");
    assert_eq!(shown_paint(run), &painted, "the stage shows the outfit's paint");
    let levels = run.outfits.of(&stage).unwrap().levels.expect("the outfit's tribe levels");
    let money = currency(run, &stage);
    assert_eq!(money, 1000.0 - price, "the stage currency paid for it");
    assert!(run.outfits.session.is_none());
    if stage == GameStage::Tribal {
        {
            let world = app.world_mut();
            let mounted = world.query_filtered::<Entity, With<OutfitMounted>>().iter(world).count();
            assert!(mounted > 0, "the tribe wears the accessories ({mounted} mounted)");
        }
        // The painted tribe shows its skin, posed by the tribe clips.
        for _ in 0..30 {
            step(app);
        }
        let world = app.world_mut();
        let actors: Vec<u32> = world.query::<&crate::outfit_crowd::CrowdActor>().iter(world).map(|a| a.playing).collect();
        let joints: Vec<Transform> = world
            .query_filtered::<&Transform, With<crate::outfit_crowd::CrowdJoint>>()
            .iter(world)
            .copied()
            .collect();
        let units = world.query_filtered::<Entity, With<crate::tribal_rts::TribeUnit>>().iter(world).count();
        if crate::creature_skin_view::enabled() && crate::outfit_crowd::enabled() {
            assert!(!actors.is_empty(), "the painted tribe is a posed crowd ({units} units)");
            assert!(joints.iter().any(|t| *t != Transform::IDENTITY), "clips pose the joints");
        }
        println!("PASS Tribal crowd: {} of {units} units clip-driven, clips {actors:08X?}", actors.len());
    }
    println!("PASS {stage:?} outfitter: retail palette, one-price purchase, undo/redo, sell, locked creature parts, Paint mode, accept stores the record (levels {levels:?}, painted)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The app tests share the process's GPU-less app setup: run one at a time.
    static APP: Mutex<()> = Mutex::new(());

    #[test]
    fn records_are_per_stage() {
        let mut r = OutfitRecords::default();
        r.of_mut(&GameStage::Tribal).unwrap().parts.push(PartInstance::new(1, 0));
        assert_eq!(r.of(&GameStage::Tribal).unwrap().parts.len(), 1);
        assert!(r.of(&GameStage::Civilization).unwrap().parts.is_empty());
        assert!(r.of(&GameStage::Creature).is_none());
        assert_eq!(setup_of(&GameStage::Space), Some(SetupKind::Space));
        assert_eq!(setup_of(&GameStage::Cell), None);
    }

    #[test]
    fn repair_gives_the_creature_back() {
        let mut run = RunProgress::default();
        run.parts = vec![PartInstance::new(1, 0)];
        run.editor_dna = 55.0;
        run.outfits.session = Some(SessionBackup {
            base: run.parts.clone(),
            dna: run.editor_dna,
            focused_part: 0,
            paint: Some(run.creature_creation.paint.clone()),
        });
        run.parts.push(PartInstance::new(2, 1));
        run.editor_dna = 3.0;
        run.creature_creation.paint.painted = true;
        assert!(repair(&mut run));
        assert_eq!(run.parts.len(), 1);
        assert_eq!(run.editor_dna, 55.0);
        assert!(!run.creature_creation.paint.painted, "the creature's own paint is back");
        assert!(run.outfits.session.is_none());
        assert!(!repair(&mut run));
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }

    fn players(app: &mut App) -> (usize, usize) {
        let world = app.world_mut();
        let all = world.query_filtered::<Entity, With<Player>>().iter(world).count();
        let previews = world.query_filtered::<Entity, With<OutfitterPreview>>().iter(world).count();
        (all, previews)
    }

    fn open_in(stage: GameStage, food: f32) -> App {
        let mut run = RunProgress::default();
        run.parts = vec![PartInstance::new(1, 0)];
        let mut app = crate::runtime::headless_app(run);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(stage);
        app.update();
        app.update();
        // (Entering a stage resets its food.)
        let mut run = app.world_mut().resource_mut::<RunProgress>();
        run.dna_this_stage = food;
        run.spice = food;
        app
    }

    #[test]
    fn f2_opens_the_outfitter_and_accept_stores_the_accessories() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        for stage in [GameStage::Tribal, GameStage::Civilization, GameStage::Space] {
            let mut app = open_in(stage.clone(), 100.0);
            let before = players(&mut app);
            assert_eq!(before, (1, 0), "{stage:?}");
            tap(&mut app, KeyCode::F2);
            tap(&mut app, KeyCode::Space);
            assert!(app.world().resource::<Outfitter>().active, "{stage:?}");
            assert_eq!(players(&mut app), (1, 1), "the preview is the only player");
            {
                let run = app.world().resource::<RunProgress>();
                assert_eq!(run.parts.len(), 1);
                assert_eq!(run.editor_dna, 100.0);
                assert!(run.outfits.session.is_some());
            }
            assert_eq!(app.world().resource::<EditorState>().locked_parts, 1);
            // The editor buys an accessory (what a palette press does).
            {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                run.parts.push(PartInstance::new(2, 7));
                run.editor_dna -= 30.0;
            }
            tap(&mut app, KeyCode::F2);
            tap(&mut app, KeyCode::Space);
            assert!(!app.world().resource::<Outfitter>().active, "{stage:?}");
            assert_eq!(players(&mut app), (1, 0), "the real player is back, no preview");
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.parts, vec![PartInstance::new(1, 0)], "the creature's own parts are untouched");
            assert_eq!(run.outfits.of(&stage).unwrap().parts, vec![PartInstance::new(2, 7)]);
            let money = if stage == GameStage::Tribal { run.dna_this_stage } else { run.spice };
            assert_eq!(money, 70.0, "{stage:?}: the purchase is paid from the stage currency");
            assert!(run.outfits.session.is_none());
            assert_eq!(app.world().resource::<EditorState>().locked_parts, 0);
        }
    }

    #[test]
    fn cancel_keeps_the_record_and_the_money() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Tribal, 100.0);
        app.world_mut().resource_mut::<RunProgress>().outfits.tribal.parts = vec![PartInstance::new(5, 7)];
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::Space);
        assert!(app.world().resource::<Outfitter>().active);
        {
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.parts.len(), 2, "the worn accessory follows the creature's part");
        }
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.parts.pop();
            run.parts.push(PartInstance::new(9, 7));
            run.editor_dna -= 50.0;
        }
        app.world_mut().resource_mut::<CreatureEditor>().cancelled = true;
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::Space);
        assert!(!app.world().resource::<Outfitter>().active);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.outfits.tribal.parts, vec![PartInstance::new(5, 7)]);
        assert_eq!(run.dna_this_stage, 100.0);
        assert_eq!(run.parts, vec![PartInstance::new(1, 0)]);
    }

    #[test]
    fn the_creatures_own_parts_cannot_be_sold() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Tribal, 100.0);
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::Space);
        app.world_mut().resource_mut::<RunProgress>().focused_part = 0;
        tap(&mut app, KeyCode::Delete);
        tap(&mut app, KeyCode::Space);
        assert_eq!(app.world().resource::<RunProgress>().parts.len(), 1, "locked part stays");
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.parts.push(PartInstance::new(2, 7));
            run.focused_part = 1;
        }
        tap(&mut app, KeyCode::Delete);
        tap(&mut app, KeyCode::Space);
        assert_eq!(app.world().resource::<RunProgress>().parts.len(), 1, "the accessory is sold");
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_outfitter_setups_and_palettes_match_retail() {
        use crate::{asset_catalog::ResourceIndex, creature_editor_data::SetupKind};
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CreatureEditorData::load(&index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        for (kind, palette_id, budget, columns) in [
            (SetupKind::Tribal, 0xCA, 200, 2),
            (SetupKind::Civ, 0xC9, 1000, 3),
            (SetupKind::Space, 0xC8, 1000, 4),
        ] {
            let setup = data.setup(kind).unwrap();
            assert_eq!(setup.parts_palette, Some(palette_id), "{kind:?}");
            assert_eq!(setup.initial_budget, Some(budget), "{kind:?}");
            assert!(setup.only_edit_from_palette, "{kind:?}");
            assert!(!setup.edits_spine(), "{kind:?}: the outfitters enable Pinning / DeformHandle / RotationHandle only");
            let palette = data.parts_palette(kind).unwrap();
            let names: Vec<&str> = palette.categories.iter().map(|c| c.name.as_str()).collect();
            println!("{kind:?}: {names:?} max budget {:?}", setup.max_budget);
            assert_eq!(names.len(), 6, "{kind:?}");
            let mut max_column = 0;
            let mut items = 0;
            for page in &palette.pages {
                for item in &page.items {
                    items += 1;
                    max_column = max_column.max(item.column);
                    assert_eq!(item.group, crate::asset_catalog::TOOL_PARTS, "{kind:?}");
                    let info = data.parts.get(&item.part).unwrap_or_else(|| panic!("{kind:?}: part {:08X}", item.part));
                    assert!(info.price > 0 && info.model != 0, "{kind:?}: {}", info.name);
                }
            }
            println!("{kind:?}: {items} items, {} columns", max_column + 1);
            assert_eq!(max_column + 1, columns, "{kind:?}");
        }
        assert_eq!(data.setup(SetupKind::Tribal).unwrap().currency_char, Some('\u{2690}'));
        // Level-1 accessories are free, the others open with the stage's progress.
        let tribal = data.parts_palette(SetupKind::Tribal).unwrap();
        let infos: Vec<_> = tribal.pages.iter().flat_map(|p| p.items.iter()).filter_map(|i| data.parts.get(&i.part)).collect();
        assert!(infos.iter().any(|i| i.starts_unlocked) && infos.iter().any(|i| !i.starts_unlocked));
        let mut run = RunProgress::default();
        run.stage_order_index = 2;
        assert_eq!(run.current_stage(), GameStage::Tribal);
        assert!(infos.iter().filter(|i| i.starts_unlocked).all(|i| unlocked(&run, i)));
        assert!(infos.iter().filter(|i| i.unlock_level >= 2 && !i.starts_unlocked).all(|i| !unlocked(&run, i)));
        // The worn accessories' cost counts a symmetric pair once.
        let item = tribal.pages.iter().flat_map(|p| p.items.iter()).next().unwrap();
        let mut a = PartInstance::new(data.parts[&item.part].model, 7).with_definition(item.part);
        let mut b = a.clone();
        a.pair_id = 1;
        b.pair_id = 1;
        assert_eq!(outfit_cost(&data, &[a.clone(), b]), data.parts[&item.part].price as f32);
        assert_eq!(outfit_cost(&data, &[a.clone(), PartInstance { pair_id: 0, ..a }]), 2.0 * data.parts[&item.part].price as f32);
        // The accessories carry the tribe capabilities the levels are made of.
        let mut caps = [0u32; 5];
        let mut max = [0u8; 5];
        for item in tribal.pages.iter().flat_map(|p| p.items.iter()) {
            let l = part_levels(&index, item.group, item.part);
            for k in 0..5 {
                caps[k] += (l[k] > 0) as u32;
                max[k] = max[k].max(l[k]);
            }
        }
        println!("Tribal accessories with attack/social/armor/gather/fishing: {caps:?}, best levels {max:?}");
        assert!(caps[1] > 0 && caps[2] > 0 && caps[3] > 0, "{caps:?}");
        assert!(max.iter().all(|&m| m <= LEVEL_CAP));
        // Rows and unlocks: column 0 starts unlocked, the tribes hand out the rest.
        let locked = infos.iter().filter(|i| !i.starts_unlocked).count();
        println!("Tribal palette: {} items, {} locked, unlocks {:?}", infos.len(), locked, UNLOCKS_PER_TRIBE);
        // The tuning behind the effects (TribeGame!TribeTuning).
        let tuning = index.properties(crate::cell_editor_data::prop_key(0xF71F_A311, 0x4553_FCC0)).unwrap();
        let floats = |id: u32| tuning.0.get(&id).map(|p| p.values.iter().map(|v| f32::from_be_bytes(v[..4].try_into().unwrap())).collect::<Vec<_>>());
        let uints = |id: u32| tuning.0.get(&id).map(crate::cell_editor_data::ints);
        assert_eq!(uints(0x895B_917A), Some(UNLOCKS_PER_TRIBE.iter().map(|&n| n as i64).collect()), "accessory unlocks per tribe");
        assert_eq!(floats(crate::world_objects::name_id("scoreContributionPerAccessoryLevel")), Some(vec![SCORE_PER_SOCIAL_LEVEL]));
        assert_eq!(floats(0x459C_E162), Some(vec![SCORE_PER_BEAT]));
        for name in ["gatherCapacityForAccessories", "meatCapacityForAccessories", "fishingCapacityForAccessories", "eggCapacityForAccessories"] {
            assert_eq!(floats(crate::world_objects::name_id(name)), Some(CAPACITY_FOR_ACCESSORIES.to_vec()), "{name}");
        }
    }

    #[test]
    fn tribe_levels_take_the_best_part_up_to_five() {
        let mut l = TribeLevels::default();
        l.merge([2, 0, 1, 0, 0]);
        l.merge([1, 3, 0, 7, 0]);
        assert_eq!(l, TribeLevels { attack: 2, social: 3, armor: 1, gather: 5, fishing: 0 });
        assert_eq!(armor_health(3), 6.0, "creatureHealthIncrement 2 per armor level");
        assert_eq!(armor_health(9), 10.0);
    }

    #[test]
    fn gather_level_enlarges_every_bundle() {
        use crate::tribal_rules::Haul;
        // Without accessories the port's bundles stay as they were.
        for haul in [Haul::Fruit, Haul::Fish, Haul::Meat, Haul::Eggs] {
            assert_eq!(bundle_food(haul, 0), crate::tribal_rules::bundle_food(haul));
        }
        assert_eq!(bundle_food(Haul::Fruit, 1), 6.0);
        assert_eq!(bundle_food(Haul::Meat, 3), 8.0);
        assert_eq!(bundle_food(Haul::Fruit, 5), 10.0);
        // Fish (1.5 food each) reach carryBundleAmountMax first.
        assert_eq!(bundle_food(Haul::Fish, 1), 9.0);
        assert_eq!(bundle_food(Haul::Fish, 2), 10.0);
        assert_eq!(crate::tribal_rts::FoodKind::Fruit.capacity_at(2), 7.0);
    }

    #[test]
    fn social_level_adds_35_points_to_the_start_score_of_a_song() {
        assert_eq!(social_head_start(0), 0.0);
        assert_eq!(social_head_start(5), SCORE_PER_BEAT, "level 5 = one whole beat");
        assert_eq!(social_head_start(1), 35.0);
        let start = |bonus: f32| {
            let mut c = crate::tribal::TribalCampaign::default();
            c.ensure_ready();
            c.outfit_social_bonus = bonus;
            c.start_performance(0).unwrap();
            c.performance.unwrap().score
        };
        assert_eq!(start(social_head_start(5)) - start(0.0), 175.0);
    }

    #[test]
    fn outfit_paint_is_shown_in_its_stage() {
        let mut run = RunProgress::default();
        run.stage_order_index = 2;
        assert_eq!(run.current_stage(), GameStage::Tribal);
        run.creature_creation.paint.colors[0] = [0.1, 0.2, 0.3];
        assert_eq!(shown_paint(&run).colors[0], [0.1, 0.2, 0.3]);
        let mut outfit_paint = CreaturePaint { painted: true, ..Default::default() };
        outfit_paint.colors[0] = [0.9, 0.0, 0.0];
        run.outfits.civ.paint = Some(outfit_paint.clone());
        assert_eq!(shown_paint(&run).colors[0], [0.1, 0.2, 0.3], "another stage's outfit");
        run.outfits.tribal.paint = Some(outfit_paint);
        assert_eq!(shown_paint(&run).colors[0], [0.9, 0.0, 0.0]);
        // During a session the editor's paint is shown.
        run.outfits.session = Some(SessionBackup { base: vec![], dna: 0.0, focused_part: 0, paint: None });
        assert_eq!(shown_paint(&run).colors[0], [0.1, 0.2, 0.3]);
    }

    fn part_info(part: u32, starts_unlocked: bool) -> crate::creature_editor_data::CreaturePartInfo {
        use crate::dbpf::ResourceKey;
        let key = ResourceKey { type_id: 0, group_id: crate::asset_catalog::TOOL_PARTS, instance_id: part };
        crate::creature_editor_data::CreaturePartInfo {
            key,
            name: format!("part {part}"),
            price: 10,
            complexity: 1,
            bones: 0,
            bake_level: 0,
            geom: 0.0,
            unlock_level: if starts_unlocked { 1 } else { 2 },
            find_percentage: if starts_unlocked { 1.0 } else { 0.99 },
            min_scale: 0.5,
            max_scale: 2.0,
            sell_back: 1.0,
            icon: key,
            model: part,
            abilities: vec![],
            carnivore: 0,
            herbivore: 0,
            mouth: false,
            orient_to_surface: false,
            prefer_symmetry_plane: false,
            symmetry_snap_delta: None,
            acts_like_foot: false,
            acts_like_grasper: false,
            starts_unlocked,
        }
    }

    #[test]
    fn resolved_tribes_unlock_rows_in_order() {
        use crate::creature_editor_data::{Palette, PaletteCategory, PaletteItem, PalettePage};
        let mut data = CreatureEditorData::default();
        let mut page = PalettePage::default();
        // 4 rows x 2 columns: column 0 starts unlocked.
        for row in 0..4u32 {
            for column in 0..2u32 {
                let part = 100 + row * 10 + column;
                page.items.push(PaletteItem { part, group: crate::asset_catalog::TOOL_PARTS, row, column });
                data.parts.insert(part, part_info(part, column == 0));
            }
        }
        let palette = Palette {
            id: 0xCA,
            thumbnail_group: None,
            categories: vec![PaletteCategory { instance: 1, name: "Hats".into(), sequence: 0, icons: vec![], pages: vec![0] }],
            pages: vec![page],
        };
        data.palettes.insert(0xCA, palette);
        data.setups.insert(
            SetupKind::Tribal,
            crate::creature_editor_data::CreatureSetup { parts_palette: Some(0xCA), ..Default::default() },
        );
        let mut run = RunProgress::default();
        run.tribal_campaign.ensure_ready();
        assert!(process_unlocks(&mut run, &data).is_empty(), "no tribe resolved");
        run.tribal_campaign.villages[0].status = crate::tribal::RivalStatus::Allied;
        let first = process_unlocks(&mut run, &data);
        // The first tribe hands out 5: only 4 rows have a locked item.
        assert_eq!(first.len(), 4, "{first:?}");
        assert!(first.iter().all(|p| p % 10 == 1), "the next column of each row");
        assert_eq!(run.outfits.unlock_events, 1);
        assert!(process_unlocks(&mut run, &data).is_empty(), "handed out once");
        run.stage_order_index = 2;
        let info = data.parts[&111].clone();
        assert!(unlocked(&run, &info));
        run.outfits.unlocked.clear();
        assert!(!unlocked(&run, &info));
        run.stage_order_index = 3;
        assert!(unlocked(&run, &info), "Civ sells every accessory");
    }

    #[test]
    fn armor_raises_the_tribes_hit_points() {
        let _guard = APP.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = open_in(GameStage::Tribal, 100.0);
        for _ in 0..3 {
            app.update();
        }
        let hp = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(Entity, &crate::tribal_rts::TribeUnit, &Health)>()
                .iter(world)
                .map(|(e, u, h)| (e, (u.chieftain, h.max)))
                .collect::<std::collections::HashMap<_, _>>()
        };
        let before = hp(&mut app);
        assert!(!before.is_empty());
        app.world_mut().resource_mut::<RunProgress>().outfits.tribal.levels =
            Some(TribeLevels { armor: 3, social: 2, ..Default::default() });
        app.update();
        app.update();
        let after = hp(&mut app);
        for (e, (chief, a)) in &before {
            let Some((_, b)) = after.get(e) else { continue };
            let want = if *chief { 9.0 } else { 6.0 };
            assert!((b - a - want).abs() < 1e-3, "chieftain {chief}: {a} -> {b}");
        }
        // Social level 2: 2 x 35 points on the start score of a song.
        assert!((app.world().resource::<RunProgress>().tribal_campaign.outfit_social_bonus - 70.0).abs() < 1e-4);
        // Taking the armor off gives it back once.
        app.world_mut().resource_mut::<RunProgress>().outfits.tribal.levels = Some(TribeLevels::default());
        app.update();
        app.update();
        let reset = hp(&mut app);
        for (e, (_, a)) in &before {
            let Some((_, b)) = reset.get(e) else { continue };
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn old_saves_load_without_outfits() {
        let json = serde_json::to_string(&RunProgress::default()).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut().unwrap().remove("outfits");
        let run: RunProgress = serde_json::from_value(v).unwrap();
        assert_eq!(run.outfits, OutfitRecords::default());
    }
}
