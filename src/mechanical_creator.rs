//! Modal later-stage mechanical creator.
//!
//! This is a presentation/input shell over the existing persisted builds and
//! original part catalog. Browsing does not buy parts or edit a build; a part
//! is committed only after its authored RW4 definition imports successfully.

use bevy::{prelude::*, ui::Pressed, ui_widgets::Button};

use crate::{
    asset_catalog::{self, PartDefinition},
    game::{self, RunProgress, ShipModule, VehiclePart},
    mechanical_assets,
    stages::GameStage,
    Originals,
};

const PARTS_PER_PAGE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreatorKind {
    #[default]
    Vehicle,
    Building,
    Ship,
}

impl CreatorKind {
    fn group(self) -> u32 {
        match self {
            Self::Vehicle | Self::Ship => asset_catalog::VEHICLE_PARTS,
            Self::Building => asset_catalog::BUILDING_PARTS,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Vehicle => "VEHICLE CREATOR",
            Self::Building => "BUILDING CREATOR",
            Self::Ship => "SPACESHIP CREATOR",
        }
    }

    fn slot_count(self) -> usize {
        match self {
            Self::Vehicle => game::VEHICLE_SLOTS.len(),
            Self::Building => crate::civ::CITY_SLOTS.len(),
            Self::Ship => game::SHIP_SLOTS.len(),
        }
    }

    fn slot_name(self, slot: u32) -> &'static str {
        match self {
            Self::Vehicle => game::VEHICLE_SLOTS.get(slot as usize).map(|s| s.name),
            Self::Building => crate::civ::CITY_SLOTS.get(slot as usize).map(|s| s.name),
            Self::Ship => game::SHIP_SLOTS.get(slot as usize).map(|s| s.name),
        }
        .unwrap_or("?")
    }

    fn limit(self) -> usize {
        match self {
            Self::Vehicle | Self::Building => crate::civ::MAX_CIV_PARTS,
            Self::Ship => crate::space::MAX_SHIP_MODULES,
        }
    }
}

/// Runtime-only modal state. `revision` lets the root-owned subject preview
/// rebuild on opening, mode changes, and committed assembly edits.
#[derive(Resource, Debug)]
pub struct MechanicalCreator {
    pub open: bool,
    pub kind: CreatorKind,
    pub revision: u64,
    pub selected_model: Option<u32>,
    pub page: usize,
    pub note: String,
}

impl Default for MechanicalCreator {
    fn default() -> Self {
        Self {
            open: false,
            kind: CreatorKind::Vehicle,
            revision: 0,
            selected_model: None,
            page: 0,
            note: String::new(),
        }
    }
}

impl MechanicalCreator {
    pub fn selected_model(&self) -> Option<u32> {
        self.selected_model
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

pub fn simulation_active(creator: Res<MechanicalCreator>) -> bool {
    !creator.open
}

pub fn is_open(creator: Res<MechanicalCreator>) -> bool {
    creator.open
}

#[derive(Component)]
pub struct CreatorRoot {
    revision: u64,
}

#[derive(Component, Clone, Copy)]
pub(crate) enum CreatorAction {
    Done,
    Kind(CreatorKind),
    Page(i32),
    Select(u32),
    Add,
    Replace,
    Remove,
    Focus,
    Slot(i32),
    Rotate,
    Scale(f32),
    Offset(u8, f32),
}

/// F3 opens/closes the modal in Civilization or Space. While open, the same
/// assembly controls as the stage editor remain available; UI buttons provide
/// the matching actions for mouse users.
#[allow(clippy::too_many_arguments)]
pub fn input(
    keys: Res<ButtonInput<KeyCode>>,
    stage: Res<State<GameStage>>,
    mut creator: ResMut<MechanicalCreator>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    actions: Query<&CreatorAction, Added<Pressed>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    editor: Option<Res<crate::editor::EditorState>>,
) {
    let stage_kind = match stage.get() {
        GameStage::Civilization => CreatorKind::Vehicle,
        GameStage::Space => CreatorKind::Ship,
        _ => {
            if creator.open {
                close(&mut creator);
            }
            return;
        }
    };

    if !creator.open && pause.is_some_and(|pause| pause.open) {
        return;
    }
    // Not over an open outfitter (F2).
    if !creator.open && editor.is_some_and(|e| e.open) {
        return;
    }
    // F3 is the retail-chrome rigid editor (`rigid_editor`); this older
    // assembly modal opens with Shift+F3 (and F3 or Escape still close it).
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::F3) && (creator.open || shift) {
        creator.open = !creator.open;
        if creator.open {
            creator.kind = stage_kind;
            creator.selected_model = candidates(&originals, stage_kind)
                .first()
                .map(|part| part.model.instance_id);
            creator.page = 0;
            creator.note.clear();
        }
        creator.bump();
    }
    if !creator.open {
        return;
    }
    let previous_note = creator.note.clone();
    if keys.just_pressed(KeyCode::Escape) {
        close(&mut creator);
        return;
    }

    let mut changed = false;
    let mut selected_changed = false;
    for action in &actions {
        match *action {
            CreatorAction::Done => {
                close(&mut creator);
                return;
            }
            CreatorAction::Kind(kind) if allowed_kind(stage.get(), kind) => {
                if creator.kind != kind {
                    creator.kind = kind;
                    creator.selected_model = candidates(&originals, kind)
                        .first()
                        .map(|part| part.model.instance_id);
                    creator.page = 0;
                    creator.note.clear();
                    selected_changed = true;
                }
            }
            CreatorAction::Page(delta) => {
                creator.page = page_step(
                    creator.page,
                    delta,
                    candidates(&originals, creator.kind).len(),
                );
                selected_changed = true;
            }
            CreatorAction::Select(model) => {
                if candidates(&originals, creator.kind)
                    .iter()
                    .any(|part| part.model.instance_id == model)
                    && creator.selected_model != Some(model)
                {
                    creator.selected_model = Some(model);
                    creator.note = selected_name(&originals, creator.kind, model)
                        .map(|name| format!("Selected {name}; choose Add or Replace to commit."))
                        .unwrap_or_default();
                    selected_changed = true;
                }
            }
            CreatorAction::Add => {
                changed |= add_selected(
                    &mut creator,
                    &mut run,
                    &mut originals,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                )
            }
            CreatorAction::Replace => {
                changed |= replace_selected(
                    &mut creator,
                    &mut run,
                    &mut originals,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                )
            }
            CreatorAction::Remove => changed |= remove_focused(&mut creator, &mut run),
            CreatorAction::Focus => changed |= focus_next(&creator, &mut run),
            CreatorAction::Slot(dir) => {
                changed |= edit_focused(&creator, &mut run, |slot| {
                    *slot = game::cycle_bone(*slot, dir, creator.kind.slot_count() as u32)
                })
            }
            CreatorAction::Rotate => {
                changed |= edit_transform(&creator, &mut run, |yaw, _, _| {
                    *yaw = game::wrap_yaw(*yaw + std::f32::consts::FRAC_PI_4)
                })
            }
            CreatorAction::Scale(factor) => {
                changed |= edit_transform(&creator, &mut run, |_, scale, _| {
                    *scale = game::scaled(*scale, factor)
                })
            }
            CreatorAction::Offset(axis, amount) => {
                changed |= edit_transform(&creator, &mut run, |_, _, offset| {
                    offset[axis as usize] += amount;
                    *offset = game::clamp_offset(*offset, game::OFFSET_LIMIT);
                })
            }
            CreatorAction::Kind(_) => {}
        }
    }

    if keys.just_pressed(KeyCode::KeyV) && stage.get() == &GameStage::Civilization {
        let next = match creator.kind {
            CreatorKind::Vehicle => CreatorKind::Building,
            CreatorKind::Building => CreatorKind::Vehicle,
            CreatorKind::Ship => CreatorKind::Vehicle,
        };
        creator.kind = next;
        creator.selected_model = candidates(&originals, next)
            .first()
            .map(|part| part.model.instance_id);
        creator.page = 0;
        selected_changed = true;
    }

    if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::BracketRight) {
        let dir = if keys.just_pressed(KeyCode::BracketRight) {
            1
        } else {
            -1
        };
        selected_changed |= select_next(&mut creator, &originals, dir);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        changed |= add_selected(
            &mut creator,
            &mut run,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
        );
    }
    if keys.just_pressed(KeyCode::Backspace) {
        changed |= remove_focused(&mut creator, &mut run);
    }
    if keys.just_pressed(KeyCode::Tab) {
        changed |= focus_next(&creator, &mut run);
    }
    if keys.just_pressed(KeyCode::Comma) || keys.just_pressed(KeyCode::Period) {
        let dir = if keys.just_pressed(KeyCode::Period) {
            1
        } else {
            -1
        };
        changed |= edit_focused(&creator, &mut run, |slot| {
            *slot = game::cycle_bone(*slot, dir, creator.kind.slot_count() as u32)
        });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        changed |= edit_transform(&creator, &mut run, |yaw, _, _| {
            *yaw = game::wrap_yaw(*yaw + std::f32::consts::FRAC_PI_4)
        });
    }
    if keys.just_pressed(KeyCode::Minus) {
        changed |= edit_transform(&creator, &mut run, |_, scale, _| {
            *scale = game::scaled(*scale, 0.9)
        });
    }
    if keys.just_pressed(KeyCode::Equal) {
        changed |= edit_transform(&creator, &mut run, |_, scale, _| {
            *scale = game::scaled(*scale, 1.1)
        });
    }
    changed |= slide_with_keys(&creator, &mut run, &keys);

    if changed {
        run.request_save();
        creator.bump();
    } else if selected_changed || creator.note != previous_note {
        creator.bump();
    }
}

fn allowed_kind(stage: &GameStage, kind: CreatorKind) -> bool {
    match stage {
        GameStage::Civilization => kind != CreatorKind::Ship,
        GameStage::Space => kind == CreatorKind::Ship,
        _ => false,
    }
}

fn close(creator: &mut MechanicalCreator) {
    creator.open = false;
    creator.bump();
}

#[cfg(test)]
#[path = "mechanical_creator_tests.rs"]
mod tests;

fn candidates(originals: &Originals, kind: CreatorKind) -> Vec<PartDefinition> {
    let mut seen = std::collections::HashSet::new();
    mechanical_assets::eligible(originals, kind.group())
        .into_iter()
        .filter(|part| seen.insert(part.model.instance_id))
        .cloned()
        .collect()
}

fn selected_name(originals: &Originals, kind: CreatorKind, model: u32) -> Option<String> {
    candidates(originals, kind)
        .into_iter()
        .find(|part| part.model.instance_id == model)
        .map(|part| part.name)
}

fn page_step(page: usize, delta: i32, candidate_count: usize) -> usize {
    let pages = candidate_count.div_ceil(PARTS_PER_PAGE).max(1);
    (page as i64 + delta as i64).rem_euclid(pages as i64) as usize
}

fn select_next(creator: &mut MechanicalCreator, originals: &Originals, dir: i32) -> bool {
    let options = candidates(originals, creator.kind);
    if options.is_empty() {
        creator.note = "No supported original parts are available in this category.".into();
        return false;
    }
    let current = creator.selected_model.and_then(|model| {
        options
            .iter()
            .position(|part| part.model.instance_id == model)
    });
    let next = current
        .map(|index| (index as i64 + dir as i64).rem_euclid(options.len() as i64) as usize)
        .unwrap_or(0);
    let model = options[next].model.instance_id;
    if creator.selected_model == Some(model) {
        return false;
    }
    creator.selected_model = Some(model);
    creator.page = next / PARTS_PER_PAGE;
    creator.note = format!(
        "Selected {}; choose Add or Replace to commit.",
        options[next].name
    );
    true
}

fn selected_definition(
    originals: &Originals,
    kind: CreatorKind,
    model: u32,
) -> Option<PartDefinition> {
    candidates(originals, kind)
        .into_iter()
        .find(|part| part.model.instance_id == model)
}

fn import_selected(
    creator: &mut MechanicalCreator,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<(u32, String)> {
    let Some(model) = creator.selected_model else {
        creator.note = "Select an original part first.".into();
        return None;
    };
    let Some(definition) = selected_definition(originals, creator.kind, model) else {
        creator.note = "The selected original part is no longer available.".into();
        return None;
    };
    match mechanical_assets::import(originals, &definition, meshes, images, materials) {
        Ok(imported) => Some((imported.model.instance, imported.name)),
        Err(error) => {
            warn!("mechanical creator part {:?}: {error}", definition.key);
            creator.note = format!("{} is unavailable in this install.", definition.name);
            None
        }
    }
}

fn add_selected(
    creator: &mut MechanicalCreator,
    run: &mut RunProgress,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> bool {
    let Some((instance, name)) = import_selected(creator, originals, meshes, images, materials)
    else {
        return false;
    };
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            if build.parts.len() >= creator.kind.limit() {
                creator.note = format!("The {} part limit is reached.", creator.kind.limit());
                return false;
            }
            if creator.kind == CreatorKind::Vehicle
                && build.parts.iter().any(|part| part.instance == instance)
            {
                creator.note = "That vehicle part is already fitted.".into();
                return false;
            }
            let slot = build.parts.len() as u32 % creator.kind.slot_count().max(1) as u32;
            build.parts.push(VehiclePart::new(instance, slot));
            build.focused = build.parts.len() as u32 - 1;
        }
        CreatorKind::Ship => {
            if run.ship.modules.len() >= creator.kind.limit() {
                creator.note = format!("The {} part limit is reached.", creator.kind.limit());
                return false;
            }
            if run
                .ship
                .modules
                .iter()
                .any(|part| part.instance == instance)
            {
                creator.note = "That ship module is already fitted.".into();
                return false;
            }
            let slot = run.ship.modules.len() as u32 % creator.kind.slot_count().max(1) as u32;
            run.ship.modules.push(ShipModule::new(instance, slot));
            run.ship.focused = run.ship.modules.len() as u32 - 1;
        }
    }
    creator.note = format!(
        "Added {name} to the {}.",
        creator.kind.title().to_lowercase()
    );
    true
}

fn replace_selected(
    creator: &mut MechanicalCreator,
    run: &mut RunProgress,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> bool {
    let Some((instance, name)) = import_selected(creator, originals, meshes, images, materials)
    else {
        return false;
    };
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            let index = game::focus_index(build.focused, build.parts.len()) as usize;
            let Some(part) = build.parts.get_mut(index) else {
                creator.note = "Add a part before replacing the focused part.".into();
                return false;
            };
            let old = part.instance;
            if old == instance {
                creator.note = format!("{name} is already fitted in the focused slot.");
                return false;
            }
            if creator.kind == CreatorKind::Vehicle
                && build
                    .parts
                    .iter()
                    .enumerate()
                    .any(|(other, part)| other != index && part.instance == instance)
            {
                creator.note = "That vehicle part is already fitted.".into();
                return false;
            }
            let part = &mut build.parts[index];
            part.instance = instance;
        }
        CreatorKind::Ship => {
            let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
            let Some(part) = run.ship.modules.get_mut(index) else {
                creator.note = "Add a module before replacing the focused module.".into();
                return false;
            };
            let old = part.instance;
            if old == instance {
                creator.note = format!("{name} is already fitted in the focused slot.");
                return false;
            }
            if run
                .ship
                .modules
                .iter()
                .enumerate()
                .any(|(other, part)| other != index && part.instance == instance)
            {
                creator.note = "That ship module is already fitted.".into();
                return false;
            }
            let part = &mut run.ship.modules[index];
            part.instance = instance;
        }
    }
    creator.note = format!("Replaced the focused part with {name}.");
    true
}

fn remove_focused(creator: &mut MechanicalCreator, run: &mut RunProgress) -> bool {
    let removed = match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            if build.parts.is_empty() {
                None
            } else {
                let index = game::focus_index(build.focused, build.parts.len()) as usize;
                let removed = build.parts.remove(index).instance;
                build.focused = build
                    .focused
                    .min(build.parts.len().saturating_sub(1) as u32);
                Some(removed)
            }
        }
        CreatorKind::Ship => {
            if run.ship.modules.is_empty() {
                None
            } else {
                let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
                let removed = run.ship.modules.remove(index).instance;
                run.ship.focused = run
                    .ship
                    .focused
                    .min(run.ship.modules.len().saturating_sub(1) as u32);
                Some(removed)
            }
        }
    };
    if let Some(instance) = removed {
        creator.note = format!("Removed part {instance:08X}.");
        true
    } else {
        creator.note = "There is no focused part to remove.".into();
        false
    }
}

fn focus_next(creator: &MechanicalCreator, run: &mut RunProgress) -> bool {
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            if build.parts.is_empty() {
                false
            } else {
                let previous = build.focused;
                build.focused = (game::focus_index(build.focused, build.parts.len()) + 1)
                    % build.parts.len() as u32;
                previous != build.focused
            }
        }
        CreatorKind::Ship => {
            if run.ship.modules.is_empty() {
                false
            } else {
                let previous = run.ship.focused;
                run.ship.focused = (game::focus_index(run.ship.focused, run.ship.modules.len())
                    + 1)
                    % run.ship.modules.len() as u32;
                previous != run.ship.focused
            }
        }
    }
}

fn edit_focused(
    creator: &MechanicalCreator,
    run: &mut RunProgress,
    edit: impl FnOnce(&mut u32),
) -> bool {
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            let index = game::focus_index(build.focused, build.parts.len()) as usize;
            if let Some(part) = build.parts.get_mut(index) {
                let old = part.slot;
                edit(&mut part.slot);
                old != part.slot
            } else {
                false
            }
        }
        CreatorKind::Ship => {
            let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
            if let Some(part) = run.ship.modules.get_mut(index) {
                let old = part.slot;
                edit(&mut part.slot);
                old != part.slot
            } else {
                false
            }
        }
    }
}

fn edit_transform(
    creator: &MechanicalCreator,
    run: &mut RunProgress,
    edit: impl FnOnce(&mut f32, &mut f32, &mut [f32; 3]),
) -> bool {
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            let index = game::focus_index(build.focused, build.parts.len()) as usize;
            if let Some(part) = build.parts.get_mut(index) {
                let old = (part.yaw, part.scale, part.offset);
                edit(&mut part.yaw, &mut part.scale, &mut part.offset);
                old != (part.yaw, part.scale, part.offset)
            } else {
                false
            }
        }
        CreatorKind::Ship => {
            let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
            if let Some(part) = run.ship.modules.get_mut(index) {
                let old = (part.yaw, part.scale, part.offset);
                edit(&mut part.yaw, &mut part.scale, &mut part.offset);
                old != (part.yaw, part.scale, part.offset)
            } else {
                false
            }
        }
    }
}

fn slide_with_keys(
    creator: &MechanicalCreator,
    run: &mut RunProgress,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    match creator.kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if creator.kind == CreatorKind::Vehicle {
                &mut run.vehicle
            } else {
                &mut run.building
            };
            let index = game::focus_index(build.focused, build.parts.len()) as usize;
            build.parts.get_mut(index).is_some_and(|part| {
                let old = part.offset;
                game::apply_slide_offset(keys, &mut part.offset) && old != part.offset
            })
        }
        CreatorKind::Ship => {
            let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
            run.ship.modules.get_mut(index).is_some_and(|part| {
                let old = part.offset;
                game::apply_slide_offset(keys, &mut part.offset) && old != part.offset
            })
        }
    }
}

/// Rebuild the modal tree only when its state changes. Part thumbnails use
/// the same exact-definition requests and native icon/model fallback as the
/// creature creator.
pub fn ui(
    mut commands: Commands,
    creator: Res<MechanicalCreator>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    existing: Query<(Entity, &CreatorRoot)>,
) {
    let current = existing.iter().next();
    if !creator.open && current.is_none() {
        return;
    }
    if creator.open && current.is_some_and(|(_, root)| root.revision == creator.revision) {
        return;
    }
    for (entity, _) in &existing {
        commands.entity(entity).despawn();
    }
    if !creator.open {
        return;
    }

    let options = candidates(&originals, creator.kind);
    let pages = options.len().div_ceil(PARTS_PER_PAGE).max(1);
    let page = creator.page.min(pages - 1);
    let focused = focused_summary(creator.kind, &run);
    commands
        .spawn((
            CreatorRoot {
                revision: creator.revision,
            },
            GlobalZIndex(600),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.),
                height: Val::Percent(100.),
                padding: UiRect::all(Val::Px(28.)),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.025, 0.045, 0.34)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Percent(58.),
                    height: Val::Percent(96.),
                    max_width: Val::Px(740.),
                    padding: UiRect::all(Val::Px(22.)),
                    border_radius: BorderRadius::all(Val::Px(12.)),
                    row_gap: Val::Px(16.),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BorderColor::all(Color::srgb(0.75, 0.62, 0.35)),
                BackgroundColor(Color::srgba(0.055, 0.075, 0.10, 0.97)),
            ))
            .with_children(|panel| {
                panel.spawn(text_node(creator.kind.title(), 28., Color::srgb(0.95, 0.84, 0.58)));
                panel.spawn(text_node(
                    "Choose a part, then add it to your design.",
                    14.,
                    Color::srgb(0.72, 0.79, 0.83),
                ));
                panel.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.),
                    ..default()
                })
                .with_children(|row| {
                    if creator.kind != CreatorKind::Ship {
                        action_button(row, "Vehicle", CreatorAction::Kind(CreatorKind::Vehicle), creator.kind == CreatorKind::Vehicle);
                        action_button(row, "Building", CreatorAction::Kind(CreatorKind::Building), creator.kind == CreatorKind::Building);
                    }
                    if creator.kind == CreatorKind::Ship {
                        action_button(row, "Ship", CreatorAction::Kind(CreatorKind::Ship), true);
                    }
                    action_button(row, "Previous page", CreatorAction::Page(-1), false);
                    row.spawn(text_node(&format!("Page {} / {pages}", page + 1), 15., Color::WHITE));
                    action_button(row, "Next page", CreatorAction::Page(1), false);
                });

                panel.spawn(Node {
                    flex_grow: 1.,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(10.),
                    column_gap: Val::Px(10.),
                    ..default()
                })
                .with_children(|grid| {
                    let start = page * PARTS_PER_PAGE;
                    for definition in options.iter().skip(start).take(PARTS_PER_PAGE) {
                        let selected = creator.selected_model == Some(definition.model.instance_id);
                        grid.spawn((
                            Button,
                            CreatorAction::Select(definition.model.instance_id),
                            Node {
                                width: Val::Percent(24.),
                                min_height: Val::Px(96.),
                                padding: UiRect::all(Val::Px(8.)),
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(8.),
                                ..default()
                            },
                            BackgroundColor(if selected { Color::srgb(0.27, 0.23, 0.15) } else { Color::srgb(0.10, 0.14, 0.18) }),
                            BorderColor::all(if selected { Color::srgb(0.95, 0.76, 0.34) } else { Color::srgb(0.25, 0.33, 0.38) }),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                ImageNode::default(),
                                crate::part_thumbnails::ThumbnailRequest { definition: definition.key },
                                Visibility::Inherited,
                                Node { width: Val::Px(72.), height: Val::Px(72.), ..default() },
                            ));
                            card.spawn(text_node(&definition.name, 15., Color::WHITE));
                        });
                    }
                });

                panel.spawn(text_node(&focused, 16., Color::srgb(0.86, 0.88, 0.84)));
                panel.spawn(text_node(&creator.note, 14., Color::srgb(0.95, 0.78, 0.43)));
                panel.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(7.),
                    row_gap: Val::Px(7.),
                    ..default()
                })
                .with_children(|row| {
                    action_button(row, "Add selected", CreatorAction::Add, false);
                    action_button(row, "Replace focused", CreatorAction::Replace, false);
                    action_button(row, "Remove", CreatorAction::Remove, false);
                    action_button(row, "Focus next", CreatorAction::Focus, false);
                    action_button(row, "Previous slot", CreatorAction::Slot(-1), false);
                    action_button(row, "Next slot", CreatorAction::Slot(1), false);
                    action_button(row, "Rotate", CreatorAction::Rotate, false);
                    action_button(row, "Scale −", CreatorAction::Scale(0.9), false);
                    action_button(row, "Scale +", CreatorAction::Scale(1.1), false);
                    action_button(row, "Slide X−", CreatorAction::Offset(0, -0.1), false);
                    action_button(row, "Slide X+", CreatorAction::Offset(0, 0.1), false);
                    action_button(row, "Slide Z−", CreatorAction::Offset(2, -0.1), false);
                    action_button(row, "Slide Z+", CreatorAction::Offset(2, 0.1), false);
                    action_button(row, "Slide Y−", CreatorAction::Offset(1, -0.1), false);
                    action_button(row, "Slide Y+", CreatorAction::Offset(1, 0.1), false);
                    action_button(row, "Done · F3 / Esc", CreatorAction::Done, true);
                });
                panel.spawn(text_node(
                    "Right-drag the preview to rotate; wheel to zoom.  N add; Tab focus; R rotate; - / = scale; IJKL / UO move; V switch vehicle / building",
                    12.,
                    Color::srgb(0.59, 0.67, 0.72),
                ));
            });
        });
}

fn focused_summary(kind: CreatorKind, run: &RunProgress) -> String {
    let (count, focused, _instance, slot, yaw, scale) = match kind {
        CreatorKind::Vehicle | CreatorKind::Building => {
            let build = if kind == CreatorKind::Vehicle {
                &run.vehicle
            } else {
                &run.building
            };
            let index = game::focus_index(build.focused, build.parts.len()) as usize;
            let Some(part) = build.parts.get(index) else {
                return format!(
                    "Added parts: 0/{} · select a part and choose Add",
                    kind.limit()
                );
            };
            (
                build.parts.len(),
                index,
                part.instance,
                part.slot,
                part.yaw,
                part.scale,
            )
        }
        CreatorKind::Ship => {
            let index = game::focus_index(run.ship.focused, run.ship.modules.len()) as usize;
            let Some(part) = run.ship.modules.get(index) else {
                return format!(
                    "Added parts: 0/{} · select a part and choose Add",
                    kind.limit()
                );
            };
            (
                run.ship.modules.len(),
                index,
                part.instance,
                part.slot,
                part.yaw,
                part.scale,
            )
        }
    };
    format!(
        "Added parts: {count}/{} · focus {} · {} · yaw {:.0}° · scale {:.2}",
        kind.limit(),
        focused + 1,
        kind.slot_name(slot),
        yaw.to_degrees(),
        scale
    )
}

fn text_node(text: &str, font_size: f32, color: Color) -> (Text, TextFont, TextColor) {
    (
        Text::new(text.to_owned()),
        TextFont {
            font_size: bevy::text::FontSize::Px(font_size),
            ..default()
        },
        TextColor(color),
    )
}

fn action_button(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    action: CreatorAction,
    primary: bool,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(Val::Px(11.), Val::Px(7.)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(if primary {
                Color::srgb(0.31, 0.25, 0.13)
            } else {
                Color::srgb(0.13, 0.19, 0.23)
            }),
            BorderColor::all(Color::srgb(0.38, 0.49, 0.54)),
        ))
        .with_children(|button| {
            button.spawn(text_node(text, 13., Color::WHITE));
        });
}
