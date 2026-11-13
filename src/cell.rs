//! Cell stage: compatibility surface over the retail engine.
//!
//! The simulation itself is the data-driven retail engine in `cell_engine`
//! (with `cell_rules`, `cell_ai`, `cell_motion`, `cell_population`,
//! `cell_flow`, and the decoded data in `cell_data`). This module keeps the
//! small public surface the rest of OpenSpore uses (HUD, audio, editor gate,
//! campaign plumbing): the diet choice at the start of a lineage, notices,
//! the mating flags, the part-kind names, and pure helpers whose old
//! DNA-based growth is replaced by the retail model (the player keeps a
//! constant world size; the world is rescaled instead).

use bevy::prelude::*;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use crate::asset_catalog::Capabilities;
use crate::game::*;

pub use crate::cell_engine::setup as setup_cell;

/// Radius of the player's body in Bevy units (the editor slots, mounted parts
/// and the player body mesh are authored for it).
pub const CELL_BODY_RADIUS: f32 = 0.9;
/// Height of the top-down camera at zoom 1, Bevy units. VERIFIED
/// (`notes\cell-final-notes.md` 1): every frame 0xE600A0 puts the camera
/// 20 world units above the look point (offset (0, 0, 20) x zoomFactor) and
/// calls 0x7C5400(50): the viewer's vertical half-angle tangent becomes
/// 0.75 * tan(25 deg) = 0.34973 on any non-square window (50 deg is the
/// horizontal FOV at 4:3; wider windows widen horizontally), so the view is
/// 2 * 20 * 0.34973 = 13.99 world units high at zoom 1. With Bevy's 45 degree
/// vertical FOV the same view needs 20 * 0.34973 * UNIT / tan(22.5 deg) =
/// 6.9946 * 1.8895 / 0.41421 = 31.91. (cell-polish2 had 34.21 from the
/// +-(10, 7.5) visible box, which is a cull box 7% larger than the view; the
/// first value, 24, made cells 1.43x too large.)
pub const CAMERA_HEIGHT_BASE: f32 = 31.91;
/// Campaign DNA goal for the Cell stage: `dna_this_stage` mirrors the retail
/// food progression / 10, so 100 = 1000 food = stage complete.
pub const CELL_DNA_GOAL: f32 = 100.0;
/// Short biome labels for the HUD.
pub const GROWTH_TIER_NAMES: [&str; 5] = ["ice", "rock", "plant", "water", "beach"];

/// Pure: biome pair `0..=4` reached at this campaign DNA (food / 10).
pub fn growth_tier(dna: f32) -> usize {
    crate::cell_engine::biome((dna.clamp(0.0, CELL_DNA_GOAL) * 10.0) as i32)
}

/// Pure: body scale relative to the base body. Retail never enlarges the
/// player in world units (the world shrinks around it), so this is 1.
pub fn growth_scale(_dna: f32) -> f32 {
    1.0
}

/// Pure: short name of the biome a DNA total sits in.
pub fn tier_name(dna: f32) -> &'static str {
    GROWTH_TIER_NAMES[growth_tier(dna).min(GROWTH_TIER_NAMES.len() - 1)]
}

/// Pure: world radius of the player cell (constant, see `growth_scale`).
pub fn player_radius(_dna: f32) -> f32 {
    CELL_BODY_RADIUS
}

/// Pure: camera height at zoom 1 (constant; the engine applies user zoom).
pub fn camera_height(_dna: f32) -> f32 {
    CAMERA_HEIGHT_BASE
}

// ---------------------------------------------------------------------------
// Part kinds (the editor's gate on undiscovered parts)
// ---------------------------------------------------------------------------

/// Part kinds found in the pool (retail Unlock types), with names — the six
/// the "Collect x/6" goal counts. The mandible (`"mandible"`, the jaw a
/// herbivore must find) is gated too but not counted.
pub const DISCOVERABLE_KINDS: [(&str, &str); 6] = [
    ("spike", "Spike"),
    ("jet", "Jet"),
    ("poison", "Poison"),
    ("cilia", "Cilia"),
    ("electric", "Electric"),
    ("proboscis", "Proboscis"),
];

/// Pure: the discoverable kind a single cell part belongs to. Filter mouths,
/// eyes and flagella are free (the install has no unlock cell for them).
pub fn discoverable_kind(abilities: &Capabilities) -> Option<&'static str> {
    if abilities.cell_electric > 0 {
        Some("electric")
    } else if abilities.cell_poison > 0 {
        Some("poison")
    } else if abilities.cell_spike > 0 {
        Some("spike")
    } else if abilities.cell_jet > 0 {
        Some("jet")
    } else if abilities.cell_cilia > 0 {
        Some("cilia")
    } else if abilities.herbivore > 0 && abilities.carnivore > 0 {
        Some("proboscis")
    } else if abilities.carnivore > 0 {
        Some("mandible")
    } else {
        None
    }
}

/// Pure: every discoverable kind in a creature's merged abilities.
pub fn kinds_carried(abilities: &Capabilities) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = DISCOVERABLE_KINDS
        .iter()
        .map(|(kind, _)| *kind)
        .filter(|kind| match *kind {
            "spike" => abilities.cell_spike > 0,
            "jet" => abilities.cell_jet > 0,
            "poison" => abilities.cell_poison > 0,
            "cilia" => abilities.cell_cilia > 0,
            "electric" => abilities.cell_electric > 0,
            _ => abilities.herbivore > 0 && abilities.carnivore > 0,
        })
        .collect();
    if abilities.carnivore > 0 && abilities.herbivore == 0 {
        kinds.push("mandible");
    }
    kinds
}

/// Display name of a discoverable kind.
pub fn kind_name(kind: &str) -> &'static str {
    if kind == "mandible" {
        return "Jaw";
    }
    DISCOVERABLE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("part", |(_, name)| *name)
}

// ---------------------------------------------------------------------------
// Resources and components other modules read
// ---------------------------------------------------------------------------

/// Legacy randomness resource (registered by `configure_game`).
#[derive(Resource)]
pub struct CellRng(pub ChaCha8Rng);

impl Default for CellRng {
    fn default() -> Self {
        Self(ChaCha8Rng::seed_from_u64(0))
    }
}

/// Legacy timer resource (registered by `configure_game`; unused).
#[derive(Resource)]
pub struct CellSpawnTimer(pub Timer);

impl Default for CellSpawnTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.7, TimerMode::Repeating))
    }
}

/// Weapon recharge readout for the HUD/audio (the engine updates it).
#[derive(Resource, Default)]
pub struct CellWeapon {
    pub electric_cooldown: f32,
    pub invulnerable: f32,
    pub shard_timer: f32,
}

/// Mating state readout for audio/music (the engine's `cell_flow` drives it).
#[derive(Resource, Debug, Default)]
pub struct CellMating {
    pub mate: Option<Entity>,
    pub editing: bool,
    /// Set once the stage is complete (the evolve prompt shows once).
    pub ready_announced: bool,
}

/// Set by the final Cell mating: the Creature stage opens in the creator.
#[derive(Resource)]
pub struct OpenCreatorOnLand;

/// On reaching land after the Cell stage, open the Creature creator first:
/// the "Early Creature Creator" setup `CellToCreatureEditor` (budget 145),
/// as retail does before the Creature stage begins. The creature it edits is
/// the player's cell converted part by part (`creature_cell_convert`).
pub fn creator_after_landfall(
    mut commands: Commands,
    flag: Option<Res<OpenCreatorOnLand>>,
    mut editor: ResMut<crate::editor::EditorState>,
    creator: Option<ResMut<crate::creature_editor::CreatureEditor>>,
    (run, originals): (Option<ResMut<RunProgress>>, Option<Res<crate::Originals>>),
) {
    if flag.is_some() {
        commands.remove_resource::<OpenCreatorOnLand>();
        if let (Some(mut run), Some(originals)) = (run, originals) {
            crate::creature_cell_convert::apply(&mut run, &originals);
        }
        match creator {
            Some(mut c) => crate::creature_editor::request_setup(
                &mut c,
                &mut editor,
                crate::creature_editor_data::SetupKind::CellToCreature,
            ),
            None => editor.show(),
        }
        editor.visuals_dirty = true;
    }
}

/// Short on-screen message (part found, eaten, mate nearby...).
#[derive(Resource, Debug, Default)]
pub struct CellNotice {
    pub text: String,
    pub remaining: f32,
}

impl CellNotice {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.remaining = 4.0;
    }
}

/// Spore opens a new lineage by asking whether the first cell eats plants
/// (filter mouth) or meat (jaw). The pool waits until the choice is made.
#[derive(Resource, Debug, Default)]
pub struct CellIntro {
    pub choosing: bool,
}

/// Run condition: the opening diet choice is not on screen.
pub fn intro_done(intro: Option<Res<CellIntro>>) -> bool {
    intro.is_none_or(|intro| !intro.choosing)
}

/// The new-game diet step (retail ChooseDiet, a setup-wizard step before the
/// stage, RE 10A): 1 picks the herbivore filter mouth, 2 the carnivore jaw (a
/// carnivore starts with the mandible unlocked); then the Cell stage starts.
#[allow(clippy::too_many_arguments)]
pub fn cell_intro(
    keys: Res<ButtonInput<KeyCode>>,
    mut intro: ResMut<CellIntro>,
    mut run: ResMut<RunProgress>,
    originals: Res<crate::Originals>,
    mut editor: ResMut<crate::editor::EditorState>,
    notice: Option<ResMut<CellNotice>>,
    state: Res<State<crate::stages::GameStage>>,
    mut next: ResMut<NextState<crate::stages::GameStage>>,
) {
    if !intro.choosing {
        return;
    }
    let carnivore = if keys.just_pressed(KeyCode::Digit2) {
        true
    } else if keys.just_pressed(KeyCode::Digit1) {
        false
    } else {
        return;
    };
    intro.choosing = false;
    choose_diet(&mut run, &originals, &mut editor, carnivore);
    if let Some(mut notice) = notice {
        notice.show(if carnivore {
            "A carnivore is born. Hunt smaller cells and eat their meat!"
        } else {
            "A herbivore is born. Graze on the green plant specks!"
        });
    }
    if *state.get() != crate::stages::GameStage::Cell {
        next.set(crate::stages::GameStage::Cell);
    }
}

/// Apply the new-game diet choice (also the front end's "Choose Your Diet"
/// panel): a carnivore swaps the filter mouth for the jaw and unlocks the
/// mandible.
pub fn choose_diet(
    run: &mut RunProgress,
    originals: &crate::Originals,
    editor: &mut crate::editor::EditorState,
    carnivore: bool,
) {
    run.cell_diet_chosen = true;
    if carnivore {
        let jaw = originals.part_catalog.parts.iter().find(|p| {
            p.key.group_id == crate::asset_catalog::CELL_PARTS
                && !p.is_null
                && p.category() == "Mouths"
                && p.abilities.carnivore > 0
                && p.abilities.herbivore == 0
        });
        if let Some(jaw) = jaw {
            for part in &mut run.cell_parts {
                let is_filter = originals
                    .part_catalog
                    .by_model_in(crate::asset_catalog::CELL_PARTS, part.instance)
                    .is_some_and(|d| d.abilities.herbivore > 0 && d.abilities.carnivore == 0);
                if is_filter {
                    part.instance = jaw.model.instance_id;
                }
            }
        }
        if !run.cell_found_parts.iter().any(|k| k == "mandible") {
            run.cell_found_parts.push("mandible".into());
        }
        run.cell.unlocked_parts[crate::cell_data::UnlockType::Mandible.raw() as usize] = 1;
    }
    editor.visuals_dirty = true;
    run.request_save();
}

/// The mate summoned by a mating call (audio listens for it being added).
#[derive(Component)]
pub struct CellMate {
    pub age: f32,
}

/// A meteor fragment (audio listens for `hits_left` dropping).
#[derive(Component, Debug, Clone, Copy)]
pub struct CellMeteorShard {
    pub radius: f32,
    pub hits_left: u8,
    pub cooldown: f32,
}

/// Marks an unlock pickup whose part model has been attached.
#[derive(Component)]
pub struct CellPickupDressed;

/// Give unlock pickups and meteor fragments a halo and the model of a part of
/// their kind from the installed Cell part catalog, and tag fragments for the
/// shard audio cue. ADAPTED: retail draws them with effects (`cell-fx`).
pub fn cell_dress_pickups(
    mut commands: Commands,
    db: Res<crate::cell_data::CellDatabase>,
    originals: Res<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    pickups: Query<(Entity, &crate::cell_engine::CellObject), Without<CellPickupDressed>>,
    mut cache: Local<std::collections::HashMap<&'static str, Option<crate::OriginalModel>>>,
) {
    use crate::cell_engine::CellKind;
    for (entity, object) in &pickups {
        if !matches!(object.kind, CellKind::Unlock | CellKind::Fragment) {
            continue;
        }
        commands.entity(entity).insert(CellPickupDressed);
        if object.kind == CellKind::Fragment {
            commands.entity(entity).insert(CellMeteorShard {
                radius: object.radius,
                hits_left: 1,
                cooldown: 0.0,
            });
        }
        let Some(kind) = db
            .cell(object.cell)
            .and_then(|r| crate::cell_engine::unlock_kind(r.unlock_type))
        else {
            continue;
        };
        // One import per part kind (pickups are numerous at the rock level).
        let model = cache
            .entry(kind)
            .or_insert_with(|| {
                originals
                    .part_catalog
                    .parts
                    .iter()
                    .filter(|p| p.key.group_id == crate::asset_catalog::CELL_PARTS && !p.is_null)
                    .find(|p| discoverable_kind(&p.abilities) == Some(kind))
                    .and_then(|definition| {
                        crate::editor::import_definition(&originals, definition, &mut meshes, &mut images, &mut materials)
                            .ok()
                    })
            })
            .clone();
        if let Some(model) = model {
            if let Some(material) = model.material.clone() {
                // The pickup entity is scaled to its radius; the part model
                // (diagonal ~2 at `model.scale`) sits inside the halo.
                let scale = model.scale * 0.6;
                commands.entity(entity).with_children(|p| {
                    p.spawn((
                        crate::stages::StageOwned,
                        Mesh3d(model.draw_mesh()),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::splat(scale)),
                    ));
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_growth_helpers_follow_the_retail_model() {
        assert_eq!(CELL_DNA_GOAL, tuning_for(&crate::stages::GameStage::Cell).dna_to_advance);
        assert_eq!(growth_scale(0.0), 1.0);
        assert_eq!(growth_scale(80.0), 1.0, "the player keeps its world size");
        assert_eq!(camera_height(50.0), CAMERA_HEIGHT_BASE);
        assert_eq!(growth_tier(0.0), 0);
        assert_eq!(growth_tier(25.0), 1, "food 250 is the rock biome");
        assert_eq!(growth_tier(100.0), 4);
        assert_eq!(tier_name(45.0), "plant");
    }

    #[test]
    fn part_kinds_gate_the_unlockable_parts_only() {
        let jaw = Capabilities {
            carnivore: 1,
            ..default()
        };
        let filter = Capabilities {
            herbivore: 1,
            ..default()
        };
        let proboscis = Capabilities {
            herbivore: 1,
            carnivore: 1,
            ..default()
        };
        assert_eq!(discoverable_kind(&jaw), Some("mandible"));
        assert_eq!(discoverable_kind(&filter), None, "filter mouths are free");
        assert_eq!(discoverable_kind(&proboscis), Some("proboscis"));
        assert_eq!(kind_name("poison"), "Poison");
        assert_eq!(kinds_carried(&jaw), vec!["mandible"]);
    }
}
