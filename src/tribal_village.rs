//! Tribal visuals: the village objects of each tribe level, their model
//! effects, the hut damage looks, the hut "unpack" effect at a level-up and the
//! carried food platters (helper `tribal-visuals`, notes T9L in
//! `notes\tribal-final-re-notes.md`).
//!
//! RETAIL (VERIFIED in code unless marked):
//! * Hut model (cTribe 0xC99DC0, called by the tribe's SetLevel override after
//!   0xFF0260 stores the level at tribe+0x38): the property list
//!   `hutstyles!MasterHutList` (0x2BE764E2!0xC42E83F6) holds four lists of three
//!   hut PROPs, indexed by the tribe level 0..2: `npcHutList` (canvas) for every
//!   tribe that is not the player's (vtable +0x58 of tribe+0x120), else by the
//!   player species' mouths (cSpeciesProfile +0x608 mMouthCarnivorous, +0x60C
//!   mMouthHerbivorous): both -> `omnivoreHutList` (simple), herbivorous only ->
//!   `herbivoreHutList` (yurt), otherwise `carnivoreHutList` (timber).
//! * NPC tribe level (0xC9A7AF..0xC9A7E2 -> 0xC00BE0): the first level whose
//!   `CommunityMax_Tribe<n>` (prop `community` 0x16D4DFF8: 6, 9, 12, 15, 30, 45,
//!   60) holds the archetype's max population -> 6 / 9 / 12 = level 0 / 1 / 2.
//! * Camp fire (0xC91BD0, same SetLevel path): the tribe's tool of type 10 shows
//!   `TribalTools!tt_firepit_home_01/02/03` by level (Tribe mode).
//! * Totem pole (0xC999D0): `trg_totem_pole1/2/3` by level. Only the player's
//!   tribe has one (0xC992D0 is called by the player-tribe creation 0xC99F50
//!   alone); the food-mat view (0xE8F530) shows `trg_totem_foodmat1/2/3`
//!   matching the totem, `toolgather1!tt_food_mat` for a tribe without a totem.
//! * Hut damage (the hut view 0xE93F30): health fraction f; f <= 0 destroyed
//!   (`he_destroyed_hut`), f < tuning +0x1B0 (`tribeHutHiDamage` 0.33) the hut
//!   PROP's `hutDamageHiKey`, f < +0x1B4 (`tribeHutLoDamage` 0.66) `...MdKey`,
//!   f < 0.999 `...LoKey`, else the hut itself (keys read by 0xC9C810 into
//!   hut +0x200 / +0x1F4 / +0x1E8 / +0x20C). The two threshold names are read
//!   from the struct offsets' neighbours (INFERRED, same values either way).
//! * Unpack effect (0xC93190 / 0xC91CF0): the player's hut plays
//!   `MasterHutList!<diet>UnpackEffects[level - 1]` once, coloured by the tribe
//!   (INFERRED: at an upgrade; level 0 has no entry).
//! * Model effects: every tribal PROP lists `modelEffect` (0x02A907B5) with
//!   `modelEffectTransforms` (0x02A907B6): the camp fire's
//!   `trg_fire_campfire2` and tool pad, the racks' torches, the totem's
//!   `trg_tool_totemNN`, the huts' smoke; the damaged huts add fire and smoke.
//! * Carried food (the bundle view 0xE8CD10): a cGameBundle of type t (0 seaweed,
//!   1 fruit, 2 meat, 3 fish, 4 egg) shows `food!platter_<kind>_0<n>` from the
//!   table at 0x14863F8 with n by 0xC35CC0: amount < 2 % of the kind's
//!   `carryBundleAmountMax` -> 0, < 30 % -> 1, < 60 % -> 2, else 3.
//!
//! ADAPTED (marked at the code): the props keep OpenSpore's fitted widths
//! (effects are attached to the fitted root and scale with it); an NPC
//! village's slot-0 piece (the one with hut health that falls last) is drawn as
//! its camp fire and its main hut shows that piece's damage; effect ranges
//! (`modelEffectRange`) are not culled.

use bevy::prelude::*;

use crate::dbpf::ResourceKey;
use crate::game::RunProgress;
use crate::stages::StageOwned;
use crate::swarm_fx::SwarmEffect;

/// `hutstyles` group (hut PROPs and the master lists).
#[allow(dead_code)]
pub const HUT_GROUP: u32 = crate::world_objects::HUT_GROUP;
/// `toolgather1` group (`tt_food_mat`, the food mat of a tribe without a totem).
pub const FOOD_MAT_GROUP: u32 = 0xC279_C707;
/// `food` group (`platter_*`, `foodpile_*`).
pub const FOOD_GROUP: u32 = crate::world_objects::FOOD_GROUP;
/// PROP `modelEffect` / `modelEffectTransforms`.
pub const P_MODEL_EFFECT: u32 = 0x02A9_07B5;
pub const P_MODEL_EFFECT_TRANSFORMS: u32 = 0x02A9_07B6;
/// Tag of the PROP model effects (`SwarmStop`).
pub const PROP_EFFECT_TAG: u64 = 0x5452_475F_5052_4F50;
/// `CommunityMax_Tribe1..3`, `_City1..2`, `_Civ1..2` (prop `community`).
pub const COMMUNITY_MAX: [u32; 7] = [6, 9, 12, 15, 30, 45, 60];
/// `tribeHutHiDamage` / `tribeHutLoDamage` (TribeTuning).
pub const HUT_HI_DAMAGE: f32 = 0.33;
pub const HUT_LO_DAMAGE: f32 = 0.66;

/// The hut style the player's tribe builds (from the species' mouths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HutDiet {
    Herbivore,
    #[default]
    Carnivore,
    Omnivore,
}

/// 0xC99DC0: mouth ranks -> hut list. Both mouths -> omnivore, herbivorous
/// only -> herbivore, anything else (carnivorous only, or no mouth) ->
/// carnivore.
pub fn hut_diet(herbivore: u8, carnivore: u8) -> HutDiet {
    match (carnivore > 0, herbivore > 0) {
        (true, true) => HutDiet::Omnivore,
        (false, true) => HutDiet::Herbivore,
        _ => HutDiet::Carnivore,
    }
}

/// The player species' hut diet (its mounted mouth parts).
pub fn player_hut_diet(run: &RunProgress, originals: &crate::Originals) -> HutDiet {
    let caps = crate::creature_actions::player_abilities(run, originals);
    hut_diet(caps.herbivore, caps.carnivore)
}

/// `MasterHutList` lists (installed: herbivore yurt, carnivore timber, omnivore
/// simple, NPC canvas), indexed by the tribe level 0..2.
pub fn player_hut_name(diet: HutDiet, level: u8) -> &'static str {
    const LISTS: [[&str; 3]; 3] = [
        ["he_yurt_1", "he_yurt_2", "he_yurt_3"],
        ["he_timber_1", "he_timber_2", "he_timber_3"],
        ["he_simple_1", "he_simple_2", "he_simple_3"],
    ];
    let row = match diet {
        HutDiet::Herbivore => 0,
        HutDiet::Carnivore => 1,
        HutDiet::Omnivore => 2,
    };
    LISTS[row][(level as usize).min(2)]
}

/// `npcHutList`: the hut of every tribe that is not the player's.
pub fn npc_hut_name(level: u8) -> &'static str {
    ["he_canvas_1", "he_canvas_2", "he_canvas_3"][(level as usize).min(2)]
}

/// 0xC91BD0: the camp fire (tool type 10) of a tribe of `level`.
pub fn firepit_name(level: u8) -> &'static str {
    ["tt_firepit_home_01", "tt_firepit_home_02", "tt_firepit_home_03"][(level as usize).min(2)]
}

/// 0xC999D0: the player's totem pole of `level`.
pub fn totem_name(level: u8) -> &'static str {
    ["trg_totem_pole1", "trg_totem_pole2", "trg_totem_pole3"][(level as usize).min(2)]
}

/// 0xE8F530: the food mat matching the tribe's totem (`group`, name); a tribe
/// without a totem (every NPC tribe) shows `toolgather1!tt_food_mat`.
pub fn food_mat(totem_level: Option<u8>) -> (u32, &'static str) {
    match totem_level {
        Some(l) => (
            crate::tribal_tools::TRIBAL_ART_GROUP,
            ["trg_totem_foodmat1", "trg_totem_foodmat2", "trg_totem_foodmat3"][(l as usize).min(2)],
        ),
        None => (FOOD_MAT_GROUP, "tt_food_mat"),
    }
}

/// 0xC00BE0: the level of a community of `max_population` (the first
/// `CommunityMax` entry it fits in; 0 beyond the table, as the retail loop).
pub fn community_level(max_population: u32) -> u8 {
    COMMUNITY_MAX
        .iter()
        .position(|m| max_population <= *m)
        .map_or(0, |i| i as u8)
}

/// The look of a hut at a health fraction (0xE93F30 states 1..5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HutLook {
    Undamaged,
    /// `hutDamageLoKey` (f < 0.999).
    Lo,
    /// `hutDamageMdKey` (f < 0.66).
    Md,
    /// `hutDamageHiKey` (f < 0.33).
    Hi,
    /// `he_destroyed_hut` (f <= 0).
    Destroyed,
}

pub fn hut_look(fraction: f32) -> HutLook {
    if fraction <= 0.0 {
        HutLook::Destroyed
    } else if fraction < HUT_HI_DAMAGE {
        HutLook::Hi
    } else if fraction < HUT_LO_DAMAGE {
        HutLook::Md
    } else if fraction < 0.999 {
        HutLook::Lo
    } else {
        HutLook::Undamaged
    }
}

/// The hut PROP name for `base` (a `hutstyles` hut) in `look`: the damage keys
/// of the installed huts are `<base>_dmglo / _dmgmd / _dmghi`.
pub fn hut_look_name(base: &str, look: HutLook) -> String {
    match look {
        HutLook::Undamaged => base.to_string(),
        HutLook::Lo => format!("{base}_dmglo"),
        HutLook::Md => format!("{base}_dmgmd"),
        HutLook::Hi => format!("{base}_dmghi"),
        HutLook::Destroyed => "he_destroyed_hut".to_string(),
    }
}

/// 0xC91CF0: the player's hut unpack effect at tribe level `level` (1 or 2).
pub fn unpack_effect(diet: HutDiet, level: u8) -> Option<u32> {
    let list: [u32; 2] = match diet {
        HutDiet::Herbivore => [0x7A59_E4D5, 0x7A59_E4D6],
        HutDiet::Carnivore => [0xAAC7_D7B2, 0xAAC7_D7B1],
        HutDiet::Omnivore => [0x50DF_EC7F, 0x50DF_EC7C],
    };
    (level as usize).checked_sub(1).and_then(|i| list.get(i).copied())
}

/// The look of the player's home hut this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HomeLook {
    pub diet: HutDiet,
    /// The home hut's health fraction (1 = undamaged).
    pub health: f32,
}

impl HomeLook {
    #[allow(dead_code)]
    pub const UNDAMAGED: HomeLook = HomeLook { diet: HutDiet::Carnivore, health: 1.0 };

    pub fn of(run: &RunProgress, originals: &crate::Originals) -> HomeLook {
        let c = &run.tribal_campaign;
        let max = c.home_hut_health().max(1.0);
        HomeLook {
            diet: player_hut_diet(run, originals),
            health: ((max - c.home_hut_damage) / max).clamp(0.0, 1.0),
        }
    }
}

/// The home hut PROP at `resolved` tribes settled.
pub fn home_hut_name(look: &HomeLook, resolved: usize) -> String {
    let level = crate::tribal_rules::tribe_level(resolved);
    hut_look_name(player_hut_name(look.diet, level), hut_look(look.health))
}

/// The main hut of NPC village `index` (`npcHutList` at the village's level,
/// in the damage look of the piece with hut health; destroyed once conquered).
/// ADAPTED: OpenSpore's village pieces fall from the last slot; slot 0 carries
/// the hut's health and falls last, so the main hut shows its damage.
pub fn village_hut_look(index: usize, state: &crate::tribal::RivalVillageState) -> String {
    use crate::tribal::RivalStatus;
    let level = (crate::tribal::village_hut_level(index).clamp(1, 3) - 1) as u8;
    if state.status == RivalStatus::Conquered || state.huts == 0 {
        return hut_look_name(npc_hut_name(level), HutLook::Destroyed);
    }
    let fraction = if state.huts == 1 {
        let hp = crate::tribal_rules::village_piece_hp(crate::tribal::village_tier(index), 0).max(1.0);
        ((hp - state.hurt) / hp).clamp(0.0, 1.0)
    } else {
        1.0
    };
    // A standing hut never shows the destroyed model.
    let look = match hut_look(fraction) {
        HutLook::Destroyed => HutLook::Hi,
        l => l,
    };
    hut_look_name(npc_hut_name(level), look)
}

/// The carried-food platter (`food!platter_<kind>_0<n>`) for a load of
/// `amount` out of a full bundle of `capacity` (0xC35CC0 / 0x14863F8).
/// ADAPTED: the fraction is taken in food units (a fish counts 1.5 there and
/// one item in the retail capacity, so the steps match per item).
pub fn platter_name(kind: crate::tribal_rts::FoodKind, amount: f32, capacity: f32) -> String {
    use crate::tribal_rts::FoodKind;
    let cap = capacity.max(1e-3);
    let n = if amount < cap * 0.02 {
        0
    } else if amount < cap * 0.3 {
        1
    } else if amount < cap * 0.6 {
        2
    } else {
        3
    };
    let kind = match kind {
        FoodKind::Fruit => "fruit",
        FoodKind::Meat => "meat",
        FoodKind::Fish => "fish",
    };
    format!("platter_{kind}_0{n}")
}

// ---------------------------------------------------------------------------
// PROP model effects
// ---------------------------------------------------------------------------

/// One `modelEffect` entry of a PROP in the model's own frame (Bevy axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelEffect {
    pub effect: u32,
    pub offset: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

/// Decode a `modelEffectTransforms` entry (PROP type 0x38: flags u16, count
/// u16, offset 3 x f32, scale f32, rotation 3 x 3 f32, little endian) into the
/// Bevy model frame: the model importer turns Spore's z-up model space with
/// `rot_x(-90 deg)`, so offsets and rotations get the same basis change.
pub fn parse_effect_transform(bytes: &[u8]) -> Option<(Vec3, Quat, f32)> {
    let f = |i: usize| bytes.get(4 + i * 4..8 + i * 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()));
    let offset = Vec3::new(f(0)?, f(1)?, f(2)?);
    let scale = f(3).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let rows = [Vec3::new(f(4)?, f(5)?, f(6)?), Vec3::new(f(7)?, f(8)?, f(9)?), Vec3::new(f(10)?, f(11)?, f(12)?)];
    let basis = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let m = Mat3::from_cols(rows[0], rows[1], rows[2]).transpose();
    let spore = if m.determinant().abs() > 1e-4 { Quat::from_mat3(&m).normalize() } else { Quat::IDENTITY };
    Some((basis * offset, (basis * spore * basis.inverse()).normalize(), scale))
}

/// The model effects of the PROP `key` (its own and inherited lists).
pub fn model_effects(originals: &crate::Originals, key: ResourceKey) -> Vec<ModelEffect> {
    let Ok(props) = originals.resource_index.properties(key) else { return Vec::new() };
    let effects = crate::cell_editor_data::key_list(&props, P_MODEL_EFFECT);
    let transforms: Vec<(Vec3, Quat, f32)> = props
        .0
        .get(&P_MODEL_EFFECT_TRANSFORMS)
        .map(|p| p.values.iter().filter_map(|v| parse_effect_transform(v)).collect())
        .unwrap_or_default();
    effects
        .iter()
        .enumerate()
        .filter(|(_, k)| k.instance_id != 0)
        .map(|(i, k)| {
            let (offset, rotation, scale) = transforms.get(i).copied().unwrap_or((Vec3::ZERO, Quat::IDENTITY, 1.0));
            ModelEffect { effect: k.instance_id, offset, rotation, scale }
        })
        .collect()
}

/// Start the model effects of a spawned PROP on its (fitted) root: each effect
/// rides the root, so the root's scale and placement carry its offset and size.
/// ADAPTED: always on (retail culls them beyond `modelEffectRange`).
pub fn attach_model_effects(commands: &mut Commands, originals: &crate::Originals, key: ResourceKey, root: Entity) -> usize {
    let list = model_effects(originals, key);
    for (i, e) in list.iter().enumerate() {
        let mut fx = SwarmEffect::new(e.effect);
        fx.attach_to = Some(root);
        fx.position = e.offset;
        fx.rotation = e.rotation;
        fx.scale = e.scale;
        fx.preroll = true;
        fx.tag = PROP_EFFECT_TAG | i as u64;
        commands.spawn((fx, StageOwned, Name::new("tribal prop effect")));
    }
    list.len()
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// The village visuals: hut unpack effects at a level-up and the NPC villages'
/// hut looks (the home props follow in `tribal_rts::tribal_presentation`).
pub struct TribalVillagePlugin;

impl Plugin for TribalVillagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VillageLooks>()
            .add_systems(OnEnter(crate::stages::GameStage::Tribal), reset_village_looks)
            .add_systems(
                Update,
                (capture_demo, village_looks, unpack_on_level_up, carried_platters, totem_carvings)
                    .chain()
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(crate::stages::GameStage::Tribal))
                    .run_if(resource_exists::<crate::Originals>),
            );
    }
}

/// What the village visuals showed last (per stage visit).
#[derive(Resource, Default)]
pub struct VillageLooks {
    /// The tribe level seen last frame (`None` until the stage's first frame).
    pub level: Option<u8>,
    /// Unpack effects started (diagnostics, tests).
    pub unpacked: u32,
}

/// A rival village root showing a hut look (the shown PROP name).
#[derive(Component, Debug, Clone)]
pub struct ShownHut(pub String);

/// Keep each NPC village's main hut in the look of its siege state.
#[allow(clippy::too_many_arguments)]
fn village_looks(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<(Entity, &crate::tribal::RivalVillage, Option<&ShownHut>, &Transform)>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    for (entity, village, shown, transform) in &roots {
        let Some(state) = run.tribal_campaign.village(village.0) else { continue };
        // The conquest ruin is spawned by `tribal_campaign_update`.
        if state.status == crate::tribal::RivalStatus::Conquered {
            continue;
        }
        let want = village_hut_look(village.0, state);
        match shown {
            Some(s) if s.0 == want => continue,
            None => {
                // The scene spawned the undamaged / ruined hut; adopt it when
                // it already shows the wanted look.
                let first = crate::tribal::village_root_name(village.0, state.status);
                if first == want {
                    commands.entity(entity).insert(ShownHut(want));
                    continue;
                }
            }
            _ => {}
        }
        let pos = transform.translation;
        commands.entity(entity).despawn();
        if let Some(e) = crate::tribal::respawn_village_root(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            village.0,
            state.status,
            &want,
            Vec3::new(pos.x, -0.6, pos.z),
        ) {
            commands.entity(e).insert(ShownHut(want));
        }
    }
}

/// INFERRED (0xC93190): the player's hut plays its diet's unpack effect when
/// the tribe reaches level 1 and level 2.
fn unpack_on_level_up(
    mut looks: ResMut<VillageLooks>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    huts: Query<(Entity, &crate::tribal_rts::HomeProp)>,
    layout: Option<Res<crate::tribal_planet::StageLayout>>,
    mut plays: MessageWriter<crate::swarm_fx::SwarmPlay>,
) {
    let _layout = crate::tribal_planet::scope(layout.as_deref());
    let level = crate::tribal_rules::tribe_level(run.tribal_campaign.resolved_count());
    let before = looks.level.replace(level);
    let Some(before) = before else { return };
    if level <= before {
        return;
    }
    let diet = player_hut_diet(&run, &originals);
    let Some(effect) = unpack_effect(diet, level) else { return };
    let hut = huts
        .iter()
        .find(|(_, p)| p.kind == crate::tribal_rts::HomePropKind::Hut)
        .map(|(e, _)| e);
    let mut play = crate::swarm_fx::SwarmPlay::at(effect, crate::tribal::home_position());
    if let Some(hut) = hut {
        play.position = Vec3::ZERO;
        play.attach_to = Some(hut);
    }
    plays.write(play);
    looks.unpacked += 1;
}

/// Reset the per-visit state when the stage is entered.
pub fn reset_village_looks(mut looks: ResMut<VillageLooks>) {
    *looks = VillageLooks::default();
}

/// Height of the installed totem poles' centre pole (`trg_totem_pole1..3`, model
/// units: every base reaches 3.49 over its centre).
pub const TOTEM_POLE_TOP: f32 = 3.491;
/// A carved segment's height (`tt_totempole_combat` / `_social` bounding box).
pub const TOTEM_SEGMENT_HEIGHT: f32 = 1.75;
/// `cTotemPole::mTotalPossibleSegments` (default 5).
pub const TOTEM_SEGMENTS: usize = 5;

/// The segment PROP of a carving (0xC8DDA0: type 0 = `tt_totempole_combat`,
/// the red carving of a conquest; type 1 = `tt_totempole_social`, the blue one
/// of an alliance).
pub fn totem_segment_name(carving: crate::tribal::TotemSegment) -> &'static str {
    match carving {
        crate::tribal::TotemSegment::Conquered => "tt_totempole_combat",
        crate::tribal::TotemSegment::Allied => "tt_totempole_social",
    }
}

/// The base of segment `i` above the pole's foot (model units). INFERRED: the
/// segments are sub-models of the totem with no offset of their own in code;
/// 0xC8DDA0 grows the totem's bounding box by each segment's height, so they
/// stack on the centre pole.
pub fn totem_segment_base(i: usize) -> f32 {
    TOTEM_POLE_TOP + i as f32 * TOTEM_SEGMENT_HEIGHT
}

/// A carved segment on the home totem (its index and PROP).
#[derive(Component, Debug, Clone)]
pub struct TotemCarving(pub usize, pub &'static str);

/// The home totem's carved segments, one per tribe allied (blue) or conquered
/// (red), at most five, stacked on the pole (ADAPTED: untinted; retail colours
/// each with `mSegmentColors`).
#[allow(clippy::too_many_arguments)]
pub fn totem_carvings(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    totems: Query<(Entity, &crate::tribal_rts::HomeProp)>,
    carved: Query<(Entity, &TotemCarving, &ChildOf)>,
) {
    let Some((totem, _)) = totems.iter().find(|(_, p)| p.kind == crate::tribal_rts::HomePropKind::Totem) else { return };
    let want: Vec<&'static str> = run
        .tribal_campaign
        .totem_carvings
        .iter()
        .take(TOTEM_SEGMENTS)
        .map(|c| totem_segment_name(*c))
        .collect();
    let have: Vec<(Entity, usize, &'static str)> =
        carved.iter().filter(|(_, _, c)| c.parent() == totem).map(|(e, t, _)| (e, t.0, t.1)).collect();
    let matches = have.len() == want.len() && have.iter().all(|(_, i, n)| want.get(*i) == Some(n));
    if matches {
        return;
    }
    for (e, ..) in &have {
        commands.entity(*e).despawn();
    }
    for (i, name) in want.iter().enumerate() {
        let Some(model) = crate::tribal_tools::import_prop(
            &mut originals,
            crate::tribal_tools::TRIBAL_ART_GROUP,
            name,
            &mut meshes,
            &mut images,
            &mut materials,
        ) else {
            continue;
        };
        let pivot = Transform::from_xyz(0.0, totem_segment_base(i), 0.0);
        crate::tribal_tools::spawn_model_child(&mut commands, totem, &model, pivot, Transform::IDENTITY, TotemCarving(i, name));
    }
}

/// Capture option `--tribal-visuals-demo <mode>` (quick captures only):
/// `village` frames NPC village 1 with its hut half razed; `home` damages the
/// home hut to its medium look, builds the camp fire and has the members walk
/// off with loads; `fire` does the same and frames the camp fire;
/// `totem` carves three segments and frames the totem;
/// `vignette:<name>` makes the player's idle members play that idle vignette
/// at the camp fire (`tribal_vignettes`).
#[allow(clippy::too_many_arguments)]
fn capture_demo(
    mut frames: Local<u32>,
    mut run: ResMut<RunProgress>,
    controls: Option<ResMut<crate::tribal_controls::TribalControls>>,
    vignettes: Option<ResMut<crate::tribal_vignettes::TribalVignettes>>,
    layout: Option<Res<crate::tribal_planet::StageLayout>>,
    mut units: Query<(&mut crate::tribal_rts::TribeUnit, &mut crate::tribal_rts::UnitTask), Without<crate::tribal_rts::RivalMember>>,
) {
    let args: Vec<String> = std::env::args().collect();
    let Some(mode) = args.iter().position(|a| a == "--tribal-visuals-demo").and_then(|i| args.get(i + 1)).cloned() else {
        return;
    };
    let _layout = crate::tribal_planet::scope(layout.as_deref());
    *frames += 1;
    let f = *frames;
    match mode.as_str() {
        "village" => {
            if f == 20 {
                if let Some(v) = run.tribal_campaign.villages.get_mut(0) {
                    v.appeared = true;
                    v.huts = 1;
                    v.hurt = crate::tribal_rules::village_piece_hp(crate::tribal::village_tier(0), 0) * 0.5;
                }
            }
            if f % 60 == 25 {
                if let Some(mut c) = controls {
                    c.goto_request = Some(crate::tribal::village_position(0));
                }
            }
        }
        "home" | "fire" => {
            if f == 20 {
                let max = run.tribal_campaign.home_hut_health();
                run.tribal_campaign.home_hut_damage = max * 0.5;
                if !run.tribal_campaign.has_shack(crate::tribal_tools::ToolKind::Campfire) {
                    run.tribal_campaign.shacks.push(crate::tribal_tools::ToolKind::Campfire);
                }
            }
            if f == 300 && mode == "fire" {
                // Look at the camp fire (with `--wheel N@F` to come closer).
                if let (Some(mut c), Some(slot)) = (
                    controls,
                    run.tribal_campaign.shacks.iter().position(|k| *k == crate::tribal_tools::ToolKind::Campfire),
                ) {
                    c.goto_request = Some(crate::tribal_tools::shack_position(slot));
                }
            }
            if f == 440 {
                let kinds = [crate::tribal_rts::FoodKind::Fruit, crate::tribal_rts::FoodKind::Meat, crate::tribal_rts::FoodKind::Fish];
                for (i, (mut unit, mut task)) in units.iter_mut().filter(|(u, _)| !u.chieftain).enumerate() {
                    unit.carry_kind = kinds[i % 3];
                    unit.carrying = unit.carry_kind.capacity() * [1.0, 0.45, 0.2][i % 3];
                    // Walk away from the mat with the load (a delivery next to
                    // it would end at once).
                    let away = crate::tribal::home_position() + Vec3::new(-6.0 + 3.0 * i as f32, 0.0, 9.0);
                    *task = crate::tribal_rts::UnitTask::MoveTo(away);
                }
            }
        }
        "totem" => {
            if f == 20 {
                use crate::tribal::TotemSegment::*;
                run.tribal_campaign.totem_carvings = vec![Conquered, Allied, Conquered];
            }
            if f == 300 {
                if let Some(mut c) = controls {
                    c.goto_request = Some(crate::tribal::totem_position());
                }
            }
        }
        m if m.starts_with("vignette:") => {
            if f == 30 {
                if let Some(mut v) = vignettes {
                    v.demo = Some(m.trim_start_matches("vignette:").to_string());
                }
            }
        }
        _ => {}
    }
}

/// The carried food of every tribe unit: its bundle's platter
/// (`food!platter_<kind>_0<n>`, 0xE8CD10 / 0xC35CC0) on the bundle grasper
/// (cCreatureCitizen +0xFC8, the crowd's `BundleAnchor`), else above the
/// unit when it has no posed body (ADAPTED, as before). The carrier walks the
/// default locomotion with `platter_grasper_overlay` (0x05F08F08) laid over its
/// arms (`outfit_crowd::ClipPlan::overlay`, `creature_clips::pose_layered`); the
/// raiders' stolen load still plays `walk_carry` (`tribal_vignettes`, ADAPTED).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn carried_platters(
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(Entity, &crate::tribal_rts::TribeUnit)>,
    anchors: Query<(Entity, &ChildOf), With<crate::outfit_crowd::BundleAnchor>>,
    shown: Query<(Entity, &crate::tribal_rts::CarriedFood, &ChildOf)>,
) {
    for (unit_entity, unit) in &units {
        let anchor = anchors.iter().find(|(_, c)| c.parent() == unit_entity).map(|(e, _)| e);
        let parent = anchor.unwrap_or(unit_entity);
        let current = shown.iter().find(|(_, _, c)| c.parent() == unit_entity || Some(c.parent()) == anchor);
        let want = (unit.carrying > 0.0)
            .then(|| (unit.carry_kind, platter_name(unit.carry_kind, unit.carrying, unit.carry_kind.capacity())));
        match (current, &want) {
            (Some((_, food, c)), Some((_, name))) if food.1 == *name && c.parent() == parent => continue,
            (Some((e, ..)), _) => commands.entity(e).despawn(),
            (None, None) => continue,
            (None, Some(_)) => {}
        }
        let Some((kind, name)) = want else { continue };
        let pivot = if anchor.is_some() { Transform::IDENTITY } else { Transform::from_xyz(0.0, 1.05, -0.1) };
        let marker = crate::tribal_rts::CarriedFood(kind, name.clone());
        if let Some(model) = crate::tribal_tools::import_prop(&mut originals, FOOD_GROUP, &name, &mut meshes, &mut images, &mut materials) {
            if let Some(fitted) = crate::tribal_tools::fit_largest_axis(&model, &meshes, Vec3::ZERO, 0.75) {
                crate::tribal_tools::spawn_model_child(&mut commands, parent, &model, pivot, fitted, marker);
                continue;
            }
        }
        let color = match kind {
            crate::tribal_rts::FoodKind::Fruit => Color::srgb(0.9, 0.35, 0.2),
            crate::tribal_rts::FoodKind::Fish => Color::srgb(0.6, 0.75, 0.85),
            crate::tribal_rts::FoodKind::Meat => Color::srgb(0.7, 0.2, 0.2),
        };
        commands.spawn((
            StageOwned,
            marker,
            Mesh3d(meshes.add(Sphere::new(0.3))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
            pivot,
            ChildOf(parent),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tribal_rts::FoodKind;

    #[test]
    fn huts_follow_the_master_hut_list_by_diet_and_level() {
        // 0xC99DC0: both mouths -> omnivore, herbivorous only -> herbivore,
        // otherwise carnivore.
        assert_eq!(hut_diet(1, 1), HutDiet::Omnivore);
        assert_eq!(hut_diet(2, 0), HutDiet::Herbivore);
        assert_eq!(hut_diet(0, 3), HutDiet::Carnivore);
        assert_eq!(hut_diet(0, 0), HutDiet::Carnivore);
        assert_eq!(player_hut_name(HutDiet::Herbivore, 0), "he_yurt_1");
        assert_eq!(player_hut_name(HutDiet::Carnivore, 1), "he_timber_2");
        assert_eq!(player_hut_name(HutDiet::Omnivore, 2), "he_simple_3");
        assert_eq!(npc_hut_name(1), "he_canvas_2");
        assert_eq!(firepit_name(2), "tt_firepit_home_03");
        assert_eq!(totem_name(0), "trg_totem_pole1");
        assert_eq!(food_mat(Some(1)).1, "trg_totem_foodmat2");
        assert_eq!(food_mat(None), (FOOD_MAT_GROUP, "tt_food_mat"));
        // The registered names hash to the installed instances.
        assert_eq!(crate::world_objects::name_id("he_yurt_1"), 0x6EE5_3747);
        assert_eq!(crate::world_objects::name_id("he_timber_3"), 0x7152_3FB4);
        assert_eq!(crate::world_objects::name_id("tt_firepit_home_02"), 0x1AE0_06A8);
        assert_eq!(crate::world_objects::name_id("trg_totem_foodmat3"), 0x512D_FC22);
        assert_eq!(crate::world_objects::name_id("tt_food_mat"), 0xB5EF_EDBB);
    }

    #[test]
    fn npc_tribe_levels_come_from_the_community_maximums() {
        assert_eq!(community_level(6), 0);
        assert_eq!(community_level(9), 1);
        assert_eq!(community_level(12), 2);
        assert_eq!(community_level(13), 3);
        assert_eq!(community_level(1000), 0);
        // Every NPC tier's max population gives the hut level the village
        // shows (tier 1: 6, tiers 2-4: 9, tier 5: 12).
        let levels: Vec<usize> = (1..=5u8).map(crate::tribal_rules::tribe_hut_level).collect();
        assert_eq!(levels, vec![1, 2, 2, 2, 3]);
    }

    #[test]
    fn hut_damage_looks_follow_the_hut_view_thresholds() {
        assert_eq!(hut_look(1.0), HutLook::Undamaged);
        assert_eq!(hut_look(0.995), HutLook::Lo);
        assert_eq!(hut_look(0.66), HutLook::Lo);
        assert_eq!(hut_look(0.65), HutLook::Md);
        assert_eq!(hut_look(0.33), HutLook::Md);
        assert_eq!(hut_look(0.2), HutLook::Hi);
        assert_eq!(hut_look(0.0), HutLook::Destroyed);
        assert_eq!(hut_look_name("he_canvas_1", HutLook::Md), "he_canvas_1_dmgmd");
        assert_eq!(hut_look_name("he_yurt_3", HutLook::Destroyed), "he_destroyed_hut");
        assert_eq!(crate::world_objects::name_id("he_canvas_1_dmghi"), 0x8861_39FB);
        let look = HomeLook { diet: HutDiet::Herbivore, health: 0.5 };
        assert_eq!(home_hut_name(&look, 1), "he_yurt_2_dmgmd");
        assert_eq!(home_hut_name(&HomeLook::UNDAMAGED, 0), "he_timber_1");
    }

    #[test]
    fn npc_main_huts_show_the_last_pieces_damage() {
        use crate::tribal::{RivalStatus, RivalVillageState};
        let mut s = RivalVillageState::default();
        assert_eq!(village_hut_look(0, &s), "he_canvas_1");
        s.huts = 1;
        let hp = crate::tribal_rules::village_piece_hp(crate::tribal::village_tier(0), 0);
        s.hurt = hp * 0.5;
        assert_eq!(village_hut_look(0, &s), "he_canvas_1_dmgmd");
        s.hurt = hp * 0.999;
        assert_eq!(village_hut_look(0, &s), "he_canvas_1_dmghi", "a standing hut is never the ruin");
        s.status = RivalStatus::Conquered;
        s.huts = 0;
        assert_eq!(village_hut_look(0, &s), "he_destroyed_hut");
        // Tier 5 lives in a level-3 hut.
        assert_eq!(village_hut_look(4, &RivalVillageState::default()), "he_canvas_3");
    }

    #[test]
    fn totem_carvings_stack_on_the_pole() {
        use crate::tribal::TotemSegment;
        assert_eq!(totem_segment_name(TotemSegment::Conquered), "tt_totempole_combat");
        assert_eq!(totem_segment_name(TotemSegment::Allied), "tt_totempole_social");
        assert_eq!(crate::world_objects::name_id("tt_totempole_combat"), 0xA100_1C58);
        assert_eq!(crate::world_objects::name_id("tt_totempole_social"), 0xBCB8_A199);
        assert!((totem_segment_base(0) - TOTEM_POLE_TOP).abs() < 1e-6);
        assert!((totem_segment_base(2) - (TOTEM_POLE_TOP + 3.5)).abs() < 1e-5);
    }

    #[test]
    fn unpack_effects_and_platters() {
        assert_eq!(unpack_effect(HutDiet::Herbivore, 0), None);
        assert_eq!(unpack_effect(HutDiet::Herbivore, 1), Some(0x7A59_E4D5));
        assert_eq!(unpack_effect(HutDiet::Omnivore, 2), Some(0x50DF_EC7C));
        assert_eq!(platter_name(FoodKind::Fruit, 0.0, 5.0), "platter_fruit_00");
        assert_eq!(platter_name(FoodKind::Fruit, 1.0, 5.0), "platter_fruit_01");
        assert_eq!(platter_name(FoodKind::Meat, 2.0, 5.0), "platter_meat_02");
        assert_eq!(platter_name(FoodKind::Fish, 7.5, 7.5), "platter_fish_03");
        assert_eq!(crate::world_objects::name_id("platter_fruit_00"), 0xCA21_F5B7);
        assert_eq!(crate::world_objects::name_id("platter_meat_00"), 0x3B51_194A);
    }

    /// The installed tribal props' model effects resolve in the Swarm library
    /// (prints the camp fire's blocks: their kinds, app flags and LODs).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_prop_effects_resolve() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::swarm_fx::SwarmLibrary::load(&index);
        let mut originals = crate::Originals::default();
        originals.resource_index = index;
        for (group, name) in [
            (crate::tribal_tools::TRIBAL_ART_GROUP, "tt_firepit_home_01"),
            (crate::tribal_tools::TRIBAL_ART_GROUP, "tt_hunt_home_01"),
            (crate::tribal_tools::TRIBAL_ART_GROUP, "trg_totem_pole1"),
            (HUT_GROUP, "he_canvas_1"),
            (HUT_GROUP, "he_canvas_1_dmgmd"),
            (HUT_GROUP, "he_timber_1"),
        ] {
            let key = ResourceKey { group_id: group, instance_id: crate::world_objects::name_id(name), type_id: crate::asset_catalog::PROP };
            let list = model_effects(&originals, key);
            assert!(!list.is_empty(), "{name} lists model effects");
            for e in &list {
                let found = library.find(e.effect);
                println!("{name}: {:08X} at {} scale {} -> {:?}", e.effect, e.offset, e.scale, found);
                if let Some(r) = found {
                    let def = &library.dir(r.dir).effects[r.index as usize];
                    println!(
                        "   flags {:08X} appmask {:08X} lods {:?} screen {:?} blocks {:?}",
                        def.flags,
                        def.app_flags_mask,
                        def.lod_distances,
                        def.screen_size_range,
                        def.blocks.iter().map(|b| (b.kind, b.app_flags, b.app_flags_mask, b.lod_begin, b.lod_end)).collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    /// Where a carved segment can sit on the installed totem poles: the
    /// highest base vertex over the segment's footprint (prints).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_totem_segment_footprint() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let model = |name: &str| ResourceKey { group_id: 0, instance_id: crate::world_objects::name_id(name), type_id: 0x2F4E_681B };
        for name in ["trg_totem_pole1", "trg_totem_pole2", "trg_totem_pole3", "totempole-red", "totempole-blue", "totempole-base"] {
            let Ok(bytes) = index.read(model(name)) else {
                println!("{name}: not found");
                continue;
            };
            let mut stats = crate::rw4::MeshStats::default();
            let meshes = crate::rw4::extract_meshes(&bytes, &mut stats);
            let pts: Vec<[f32; 3]> = meshes.iter().flat_map(|m| m.positions.iter().copied()).collect();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &pts {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            let centre_top = pts.iter().filter(|p| p[0].abs() < 1.2 && p[1].abs() < 1.0).map(|p| p[2]).fold(f32::MIN, f32::max);
            println!("{name}: {} verts, lo {lo:?} hi {hi:?}, top over the centre {centre_top}", pts.len());
        }
    }

    #[test]
    fn effect_transforms_change_basis_like_the_model() {
        // Installed tt_firepit_home_02 entry 0: offset (0, 0, 0.6) z-up, scale 1.5.
        let hex = "0400020000000000000000009a99193f0000c03f0000803f0000000000000000000000000000803f0000000000000000000000000000803f";
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let (offset, rotation, scale) = parse_effect_transform(&bytes).unwrap();
        assert!((offset - Vec3::new(0.0, 0.6, 0.0)).length() < 1e-5, "{offset}");
        assert!(rotation.angle_between(Quat::IDENTITY) < 1e-5);
        assert_eq!(scale, 1.5);
    }
}
