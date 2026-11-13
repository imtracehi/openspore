//! Game stages (Spore's 5 stages + editors). Each stage is a Bevy `State`
//! so systems, assets and UI can be scoped per stage.

use bevy::prelude::*;

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameStage {
    #[default]
    MainMenu,
    Cell,
    Creature,
    Tribal,
    Civilization,
    Space,
}

impl GameStage {
    pub fn all() -> &'static [GameStage] {
        use GameStage::*;
        &[MainMenu, Cell, Creature, Tribal, Civilization, Space]
    }

    pub fn title(&self) -> &'static str {
        match self {
            GameStage::MainMenu => "Main Menu",
            GameStage::Cell => "Cell Stage",
            GameStage::Creature => "Creature Stage",
            GameStage::Tribal => "Tribal Stage",
            GameStage::Civilization => "Civilization Stage",
            GameStage::Space => "Space Stage",
        }
    }

    pub fn blurb(&self) -> &'static str {
        match self {
            GameStage::MainMenu => {
                "Pick a stage. Original Spore assets load read-only from your install."
            }
            GameStage::Cell => "Eat, grow, evolve. Placeholder ocean + currents.",
            GameStage::Creature => "Procedural creature placeholder. Editors come later.",
            GameStage::Tribal => "Tribe management placeholder.",
            GameStage::Civilization => "City / vehicle placeholder.",
            GameStage::Space => "Galaxy placeholder. Terraforming later.",
        }
    }

    /// Distinct clear-ish accent color per stage for the placeholder scene.
    pub fn accent(&self) -> Color {
        match self {
            GameStage::MainMenu => Color::srgb(0.12, 0.14, 0.20),
            GameStage::Cell => Color::srgb(0.05, 0.35, 0.45),
            GameStage::Creature => Color::srgb(0.55, 0.72, 0.84),
            GameStage::Tribal => Color::srgb(0.55, 0.72, 0.84),
            GameStage::Civilization => Color::srgb(0.35, 0.35, 0.40),
            GameStage::Space => Color::srgb(0.02, 0.03, 0.10),
        }
    }
}

/// Marker for entities owned by the current stage scene (despawned on switch).
#[derive(Component)]
pub struct StageOwned;
