//! CellToCreature: the first creature is the player's own cell converted part
//! by part (`notes/skin-re-notes.md` section 6, VERIFIED: FUN_004BA690, called
//! from the Cell stage FUN_00E83910). Retail copies the cell resource as a
//! creature resource and, for every block:
//! 1. measures its transformed bounding box (the lowest z of all blocks),
//! 2. lifts every block by -minZ so the creature stands on z = 0,
//! 3. resets the user rotation of parts with
//!    `modelCellApplyBehaviorToUserRotation` (stalk eyes on the symmetry plane
//!    get one of two fixed frames),
//! 4. swaps the part for `modelBlockUpgradesTo` (`cl_mouth_filter_01` ->
//!    `ce_cell_mouth_filter_01`, ...); the cell body (a skin of `cl_vertebra`
//!    blocks) becomes the creature torso unchanged.
//! The editor then opens with `CellToCreatureEditor` (the `A_*` startup
//! torsos are only used for New).
//!
//! OpenSpore's cell is not a rigblock chain: its membrane is a fixed body of
//! radius `cell::CELL_BODY_RADIUS` and its parts sit at `cell_slot(bone) +
//! offset` (Y up, front -Z, yaw about Y). ADAPTED: the torso is a round
//! [`BodyProfile`] of the same proportions (`cell_torso`), the parts keep their
//! cell positions scaled into it ([`CELL_TO_CREATURE`]) and are moved onto the
//! creature's skin; rule 2 puts the torso's lowest point on the creature
//! ground ([`GROUND_Y`]); rules 3 and 4 are applied from the parts' own PROPs.

use bevy::prelude::*;

use crate::{
    asset_catalog::{PartCatalog, ResourceIndex, CELL_PARTS, CREATURE_PARTS},
    body::{BodyProfile, SpinePoint},
    cell_editor_data::{key_of, number, prop_key},
    game::{PartInstance, RunProgress},
    Originals,
};

/// `modelBlockUpgradesTo` (cell part -> creature part).
pub const UPGRADES_TO: u32 = 0xF45F_59D7;
/// `modelCellApplyBehaviorToUserRotation`: the user rotation is reset.
pub const APPLY_BEHAVIOR_TO_USER_ROTATION: u32 = 0x0655_296A;
/// `modelCellStalkEye`.
pub const STALK_EYE: u32 = 0x0655_4604;

/// The creature's ground in the player frame: `baked_model` fits the starter
/// so its lowest vertex (the feet) sits at local y = -1.3.
pub const GROUND_Y: f32 = -1.3;
/// ADAPTED: the cell membrane (radius 0.9) becomes a torso as wide as the
/// starter's widest vertebra (0.72), so cell positions scale by 0.8.
pub const CELL_TO_CREATURE: f32 = 0.72 / crate::cell::CELL_BODY_RADIUS;
/// ADAPTED: parts keep their size relative to the body. A cell part is drawn
/// at `0.35 x scale` on a membrane of radius 0.9, a creature part at
/// `0.5 x scale` on the 0.72 torso (`editor::refresh_parts`).
pub const CELL_PART_SCALE: f32 = (0.35 / crate::cell::CELL_BODY_RADIUS) / (0.5 / 0.72);

/// What a cell part becomes (its PROP's conversion rules).
#[derive(Clone, Debug, PartialEq)]
pub struct Upgrade {
    /// Creature part PROP instance (`modelBlockUpgradesTo`), if any.
    pub creature: Option<u32>,
    pub reset_rotation: bool,
    pub stalk_eye: bool,
}

/// The conversion rules of a cell part PROP (group `cell_parts~`).
pub fn upgrade_of(index: &ResourceIndex, cell_definition: u32) -> Option<Upgrade> {
    let p = index.properties(prop_key(CELL_PARTS, cell_definition)).ok()?;
    let creature = key_of(&p, UPGRADES_TO).map(|k| k.instance_id).filter(|i| *i != 0 && *i != u32::MAX);
    let flag = |id: u32| number(&p, id).is_some_and(|v| v != 0.0);
    Some(Upgrade { creature, reset_rotation: flag(APPLY_BEHAVIOR_TO_USER_ROTATION), stalk_eye: flag(STALK_EYE) })
}

/// ADAPTED torso of a converted cell (tail first, like every `BodyProfile`):
/// five vertebrae on a straight horizontal spine whose radii follow a circle
/// as wide as the membrane, lifted so the widest one touches the ground.
pub fn cell_torso() -> BodyProfile {
    let r = crate::cell::CELL_BODY_RADIUS * CELL_TO_CREATURE;
    let y = GROUND_Y + r;
    let spine = [0.75f32, 0.4, 0.0, -0.4, -0.75]
        .iter()
        .map(|u| SpinePoint { position: [0.0, y, u * r], radius: r * (1.0 - u * u).sqrt() })
        .collect();
    BodyProfile { spine }
}

/// Centre of the converted torso (where the cell's centre goes).
pub fn torso_centre() -> Vec3 {
    Vec3::new(0.0, GROUND_Y + crate::cell::CELL_BODY_RADIUS * CELL_TO_CREATURE, 0.0)
}

/// One converted part before it is pinned to the skin: the creature part and
/// the point (player frame) its cell part occupied.
#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    pub part: PartInstance,
    pub point: Vec3,
}

/// Rules 3 and 4 over the cell's parts (pure): every cell part with a
/// creature counterpart becomes that part at its scaled cell position; parts
/// without one are dropped (retail keeps such a block as it is; no OpenSpore
/// cell part lacks an upgrade, see the installed-data test).
pub fn convert(
    cell_parts: &[PartInstance],
    catalog: &PartCatalog,
    upgrade: impl Fn(&PartInstance) -> Option<Upgrade>,
) -> Vec<Converted> {
    let centre = torso_centre();
    cell_parts
        .iter()
        .filter_map(|cp| {
            let rules = upgrade(cp)?;
            let def = catalog.by_definition_in(CREATURE_PARTS, rules.creature?)?;
            let mut part = PartInstance::new(def.model.instance_id, cp.bone).with_definition(def.key.instance_id);
            part.yaw = if rules.reset_rotation { 0.0 } else { cp.yaw };
            part.scale = (cp.scale * CELL_PART_SCALE).clamp(def.min_scale, def.max_scale.max(def.min_scale));
            part.pair_id = cp.pair_id;
            part.mirrored = cp.mirrored;
            let cell_point = crate::editor::cell_slot(cp.bone) + Vec3::from_array(cp.offset);
            Some(Converted { part, point: centre + cell_point * CELL_TO_CREATURE })
        })
        .collect()
}

/// Pin converted parts to the creature body: each point moves onto the skin
/// (`surface`), takes the nearest attachment slot and keeps the rest as its
/// offset (the creature's part placement, `editor::refresh_parts`).
pub fn pin(
    converted: Vec<Converted>,
    surface: impl Fn(Vec3) -> Vec3,
    slot: impl Fn(u32) -> Vec3,
) -> Vec<PartInstance> {
    converted
        .into_iter()
        .map(|c| {
            let p = surface(c.point);
            let bone = (0..8u32).min_by(|a, b| slot(*a).distance_squared(p).total_cmp(&slot(*b).distance_squared(p))).unwrap_or(0);
            let mut part = c.part;
            part.bone = bone;
            part.offset = crate::game::clamp_offset((p - slot(bone)).to_array(), crate::game::OFFSET_LIMIT);
            part
        })
        .collect()
}

/// The first creature's paint from the cell's (VERIFIED, `notes/limb-re-notes.md`
/// 6: FUN_004BA690 copies the cell model, so its brushes, seeds and colours
/// stay; the paint request fills the missing coat and detail layers from
/// `editorDefaultSkinPaintScripts`). ADAPTED: the cell brushes
/// `skinpaint_cell_<name>` are effect components of the `editors` directory
/// that this project does not run, so the creature brush of the same name
/// (`skinpaint_base_<name>`, 15 of the 25 cell styles) stands in, else the
/// default base; the cell's colour is the base colour either way. `None` for
/// a cell never painted (it keeps the installed look).
pub fn creature_paint_of_cell(index: &ResourceIndex, cell: &crate::cell_paint::CellPaint) -> Option<crate::creature_editor_paint::CreaturePaint> {
    if !cell.is_painted() {
        return None;
    }
    let pfx = |id: u32| {
        let key = crate::dbpf::ResourceKey {
            type_id: crate::creature_skin_paint::PFX_TYPE,
            group_id: crate::creature_skin_paint::PFX_GROUP,
            instance_id: id,
        };
        index.records.contains_key(&key)
    };
    let name = index
        .properties(prop_key(crate::cell_paint::PAINT_THEME_GROUP, cell.theme))
        .ok()
        .and_then(|p| p.0.get(&crate::cell_editor_data::prop_id::DESCRIPTION).and_then(|d| d.text()).map(|(_, _, t)| t));
    let base = name
        .as_deref()
        .and_then(|n| n.strip_prefix("skinpaint_cell_"))
        .map(|rest| crate::world_objects::name_id(&format!("skinpaint_base_{rest}")))
        .filter(|id| pfx(*id))
        .unwrap_or(0);
    Some(crate::creature_editor_paint::CreaturePaint {
        painted: true,
        // 0 = the default brush of that layer (`effective_paint`).
        brushes: [base, 0, 0],
        colors: [cell.color, [1.0; 3], [1.0; 3]],
        theme: 0,
        // The cell was painted from its palette: seed 1234 (0x4B301A).
        seeds: Some([crate::creature_editor_paint::STYLE_SEED; 3]),
    })
}

/// Build the starting creature from the saved cell (`run.cell_parts`) when the
/// creature is still the untouched starter. Returns whether it converted.
pub fn apply(run: &mut RunProgress, originals: &Originals) -> bool {
    if run.cell_parts.is_empty() || !crate::editor::is_starter_creature_parts(&run.parts, &originals.part_catalog) {
        return false;
    }
    let catalog = &originals.part_catalog;
    let converted = convert(&run.cell_parts, catalog, |cp| {
        let def = catalog.by_part_in(CELL_PARTS, cp)?;
        upgrade_of(&originals.resource_index, def.key.instance_id)
    });
    if converted.is_empty() {
        return false;
    }
    // Rule 2 on the skin itself: its lowest point goes onto the ground.
    let mut body = cell_torso();
    let lowest = crate::baked_model::body_triangles(originals, &body)
        .iter()
        .flat_map(|t| t.iter().map(|v| v.y))
        .fold(f32::MAX, f32::min);
    let lift = if lowest.is_finite() { GROUND_Y - lowest } else { 0.0 };
    for p in &mut body.spine {
        p.position[1] += lift;
    }
    let converted = converted.into_iter().map(|c| Converted { point: c.point + Vec3::Y * lift, ..c }).collect();
    let parts = pin(
        converted,
        |p| crate::baked_model::nearest_body_surface(originals, &body, p),
        |bone| crate::baked_model::attachment_point(originals, &body, bone),
    );
    info!("CellToCreature: {} cell part(s) -> {} creature part(s)", run.cell_parts.len(), parts.len());
    run.body = body;
    run.parts = parts;
    // The cell's paint stays on the first creature (unless it was painted in
    // a Creature Creator already).
    if !run.creature_creation.paint.painted {
        if let Some(paint) = creature_paint_of_cell(&originals.resource_index, &run.cell_creation.paint) {
            info!("CellToCreature: cell paint {:08X} -> brushes {:08X?}", run.cell_creation.paint.theme, paint.brushes);
            run.creature_creation.paint = paint;
        }
    }
    run.focused_part = 0;
    run.sync_legacy_accessory();
    run.request_save();
    true
}

/// The retail starting cell (the Cell stage's free filter mouth, eye and
/// flagellum, `editor::seed_parts`), for captures of a run without a cell.
pub fn default_cell(catalog: &PartCatalog) -> Vec<PartInstance> {
    [("Mouths", 0), ("Eyes", 1), ("Movement", 3)]
        .into_iter()
        .filter_map(|(category, bone)| {
            catalog
                .parts
                .iter()
                .find(|p| {
                    p.key.group_id == CELL_PARTS
                        && p.category() == category
                        && !p.is_null
                        && (category != "Mouths" || p.abilities.herbivore > 0)
                        && (category != "Movement" || p.abilities.cell_flagella > 0)
                })
                .map(|p| PartInstance::new(p.model.instance_id, bone).with_definition(p.key.instance_id))
        })
        .collect()
}

/// Capture option `--cell-to-creature`: on entering the Creature stage, convert
/// the saved cell (the retail starting cell when the run has none) and open
/// the Early Creature Creator, as the end of the Cell stage does.
pub fn capture_hook(
    state: Option<Res<State<crate::stages::GameStage>>>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    mut editor: ResMut<crate::editor::EditorState>,
    creator: Option<ResMut<crate::creature_editor::CreatureEditor>>,
    mut done: Local<bool>,
) {
    if *done || state.is_none_or(|s| *s.get() != crate::stages::GameStage::Creature) || originals.part_catalog.parts.is_empty() {
        return;
    }
    if !std::env::args().any(|a| a == "--cell-to-creature") {
        *done = true;
        return;
    }
    *done = true;
    if run.cell_parts.is_empty() {
        run.cell_parts = default_cell(&originals.part_catalog);
    }
    apply(&mut run, &originals);
    if let Some(mut c) = creator {
        crate::creature_editor::request_setup(&mut c, &mut editor, crate::creature_editor_data::SetupKind::CellToCreature);
    }
    editor.visuals_dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_torso_is_round_and_stands_on_the_ground() {
        let body = cell_torso();
        assert!(body.problem().is_none());
        let r = crate::cell::CELL_BODY_RADIUS * CELL_TO_CREATURE;
        assert!((r - 0.72).abs() < 1e-5);
        // Straight, horizontal, tail (+z) first, head (-z) last.
        assert!(body.spine.windows(2).all(|w| w[0].position[2] > w[1].position[2]));
        let lowest = body.spine.iter().map(|p| p.position[1] - p.radius).fold(f32::MAX, f32::min);
        assert!((lowest - GROUND_Y).abs() < 1e-5);
        let widest = body.spine.iter().map(|p| p.radius).fold(0.0, f32::max);
        assert!((widest - r).abs() < 1e-5);
    }

    #[test]
    fn convert_swaps_parts_and_keeps_layout() {
        use crate::asset_catalog::PartDefinition;
        let mut catalog = PartCatalog::default();
        let mut def = PartDefinition::raw_model(crate::dbpf::ResourceKey { type_id: 0, group_id: 0, instance_id: 0xAA });
        def.key = prop_key(CREATURE_PARTS, 0xC0);
        def.is_null = false;
        def.min_scale = 0.5;
        def.max_scale = 2.0;
        catalog.parts.push(def);
        let mut mouth = PartInstance::new(1, 0);
        mouth.yaw = 0.7;
        mouth.scale = 5.0;
        let mut eye = PartInstance::new(2, 1);
        eye.yaw = 0.4;
        let tail = PartInstance::new(3, 3);
        let rules = |p: &PartInstance| match p.instance {
            1 => Some(Upgrade { creature: Some(0xC0), reset_rotation: false, stalk_eye: false }),
            2 => Some(Upgrade { creature: Some(0xC0), reset_rotation: true, stalk_eye: false }),
            _ => Some(Upgrade { creature: None, reset_rotation: false, stalk_eye: false }),
        };
        let out = convert(&[mouth, eye, tail], &catalog, rules);
        assert_eq!(out.len(), 2, "a part without an upgrade is dropped");
        assert_eq!(out[0].part.instance, 0xAA);
        assert_eq!(out[0].part.definition_id, Some(0xC0));
        assert_eq!(out[0].part.yaw, 0.7);
        assert_eq!(out[0].part.scale, 2.0, "scale clamped to the creature part");
        assert!((CELL_PART_SCALE - 0.56).abs() < 1e-3);
        assert_eq!(out[1].part.yaw, 0.0, "modelCellApplyBehaviorToUserRotation resets the rotation");
        // The mouth stays in front (-z), the eye on the left top.
        assert!(out[0].point.z < -0.6);
        assert!(out[1].point.x < 0.0 && out[1].point.y > torso_centre().y);
        // Pinning onto a sphere around the torso centre keeps the point's
        // direction and stores it as slot + offset.
        let centre = torso_centre();
        let slot = |b: u32| centre + Vec3::new(b as f32 * 0.1 - 0.3, 0.0, 0.72);
        let parts = pin(out, |p| centre + (p - centre).normalize() * 0.72, slot);
        for p in &parts {
            let at = slot(p.bone) + Vec3::from_array(p.offset);
            assert!((at.distance(centre) - 0.72).abs() < 1e-4);
        }
    }

    /// Installed data: a painted cell's style carries over to the first
    /// creature (the creature brush of the same name, the cell's colour, the
    /// palette seed); an unpainted cell leaves the creature unpainted.
    #[test]
    fn installed_cell_paint_carries_over() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let id = crate::world_objects::name_id;
        let cell = crate::cell_paint::CellPaint { theme: id("cl_skinpaint_cell_amphibian"), color: [0.2, 0.4, 0.6] };
        let p = creature_paint_of_cell(&index, &cell).expect("painted");
        assert!(p.painted);
        assert_eq!(p.brushes[0], id("skinpaint_base_amphibian"));
        assert_eq!(p.colors[0], cell.color);
        assert_eq!(p.effective_seeds(), [crate::creature_editor_paint::STYLE_SEED; 3]);
        let mesh = crate::cell_paint::CellPaint { theme: id("cl_skinpaint_cell_mesh"), color: [1.0, 0.0, 0.0] };
        assert_eq!(creature_paint_of_cell(&index, &mesh).expect("painted").brushes[0], 0, "no creature brush: the default base");
        assert!(creature_paint_of_cell(&index, &crate::cell_paint::CellPaint::default()).is_none());
    }

    /// Installed data: every OpenSpore cell part upgrades to an installed
    /// creature part with a model; the retail starting cell converts to a
    /// mouth, an eye and a tail-end part.
    #[test]
    fn installed_cell_parts_upgrade_to_creature_parts() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = PartCatalog::from_index(&index);
        let mut missing = Vec::new();
        let mut count = 0;
        for def in catalog.parts.iter().filter(|p| p.key.group_id == CELL_PARTS && !p.is_null) {
            let rules = upgrade_of(&index, def.key.instance_id).expect("cell part PROP");
            let target = rules.creature.and_then(|c| catalog.by_definition_in(CREATURE_PARTS, c));
            println!(
                "{:<32} -> {:<36} reset {} stalk {}",
                def.name,
                target.map_or("(none)".to_string(), |t| t.name.clone()),
                rules.reset_rotation,
                rules.stalk_eye
            );
            count += 1;
            if target.is_none() {
                missing.push(def.name.clone());
            }
        }
        assert!(count > 10);
        println!("{} cell parts, {} without an installed creature counterpart: {missing:?}", count, missing.len());
        let cell = default_cell(&catalog);
        assert_eq!(cell.len(), 3);
        let out = convert(&cell, &catalog, |cp| upgrade_of(&index, catalog.by_part_in(CELL_PARTS, cp)?.key.instance_id));
        assert_eq!(out.len(), 3, "the starting cell converts completely");
        for c in &out {
            let def = catalog.by_definition_in(CREATURE_PARTS, c.part.definition_id.unwrap()).unwrap();
            println!("starting cell -> {} at {:?}", def.name, c.point);
        }
    }
}
