//! The tool reward window of the Space HUD (`0664C270` of `GlobalUISpace2-1`,
//! RE S5A): a dialog "Reward / Tools Unlocked" that lists the tools a finished
//! mission or the galactic core gave the player, with the first tool's icon,
//! name and description. Enter or the check button close it.
//!
//! ADAPTED: the retail code opens it from the mission reward; here a tool that
//! enters the UFO's loadout in the frame a tutorial mission finishes or the
//! galactic core is reached is a reward (tools the player buys are not).

use std::collections::HashSet;

use bevy::prelude::*;

use super::{palette, SpaceHudActive, SpaceHudRoot, SpaceHudState};
use crate::{
    game::RunProgress,
    spui::{model::ImageRef, view::SpuiAssets, view::SpuiState},
    stages::GameStage,
};

/// Control ids of the reward window.
#[allow(dead_code)]
pub mod rid {
    pub const WINDOW: u32 = 0x0664_C270;
    pub const PANEL: u32 = 0x0674_AA78;
    /// The big picture of the first tool.
    pub const ICON: u32 = 0x0664_A9D4;
    /// First tool's name (centred under the picture) and description.
    pub const NAME: u32 = 0x0678_CD68;
    pub const DESCRIPTION: u32 = 0x0677_9EE0;
    /// The reward's title (gold, left) and the "Tools Unlocked" header.
    pub const TITLE: u32 = 0x0664_A9D3;
    pub const HEADER: u32 = 0x0664_A9D5;
    /// The lines of the tools unlocked (five).
    pub const LINES: [u32; 5] = [0x0664_A9D6, 0x0664_A9D7, 0x0664_A9D8, 0x0664_A9D9, 0x0664_A9DA];
    pub const MESSAGE: u32 = 0x0664_A9D0;
    pub const CLOSE: u32 = 0x0674_C4D0;
}

/// Tools the passive upgrades of the store are not announced.
fn announced(tool: &str) -> bool {
    !(tool.starts_with("healthupgrade") || tool.starts_with("energyupgrade") || tool.starts_with("cargoupgrade"))
}

/// The tools of `now` that `before` did not have and that deserve a window.
pub fn new_tools(before: &HashSet<String>, now: &[String]) -> Vec<String> {
    now.iter().filter(|t| !before.contains(*t) && announced(t)).cloned().collect()
}

#[derive(Resource, Default)]
pub struct RewardState {
    /// The loadout of the last frame (`None` before the HUD's first frame:
    /// the loadout the game starts with is not a reward).
    known: Option<HashSet<String>>,
    /// Tutorial step and core flag of the last frame.
    step: Option<(crate::space_rules::TutorialStep, bool)>,
    /// Rewards waiting, the first one is shown.
    queue: Vec<Vec<String>>,
}

impl RewardState {
    pub fn dismiss(&mut self) {
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
    }

    /// The tool ids shown now.
    pub fn open(&self) -> Option<&Vec<String>> {
        self.queue.first()
    }
}

/// Watch the loadout and fill [`SpaceHudState::reward`].
pub fn publish_reward(
    stage: Res<State<GameStage>>,
    active: Res<SpaceHudActive>,
    run: Res<RunProgress>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<RewardState>,
    mut hud: ResMut<SpaceHudState>,
) {
    if *stage.get() != GameStage::Space || !active.0 {
        *state = RewardState::default();
        hud.reward = None;
        return;
    }
    let campaign = &run.space_campaign;
    let now_step = (campaign.tutorial.step, campaign.reached_core);
    if let (Some(before), Some(step)) = (state.known.as_ref(), state.step) {
        if step != now_step {
            let new = new_tools(before, &campaign.tools);
            if !new.is_empty() {
                state.queue.push(new);
            }
        }
    }
    state.known = Some(campaign.tools.iter().cloned().collect());
    state.step = Some(now_step);
    if keys.just_pressed(crate::cell_flow::DISMISS_KEY) {
        state.dismiss();
    }
    // The demo flag shows a reward.
    let demo = std::env::args().any(|a| a == "--space-hud-reward");
    hud.reward =
        state.open().cloned().or_else(|| demo.then(|| vec!["interplanetarydrive".to_string(), "seti".to_string()]));
}

/// Bind the window: the first tool's picture, name and description and one
/// line per tool.
#[allow(clippy::too_many_arguments)]
pub fn bind_reward(
    hud: Res<SpaceHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<palette::ToolInfoCache>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // The tool ids are static in the palette cache; the reward lists any id,
    // so the names are read through a local cache of owned strings.
    for (mut ui, kind) in &mut roots {
        if *kind != SpaceHudRoot::Main || !ui.ready {
            continue;
        }
        let Some(window) = ui.find(rid::WINDOW) else {
            continue;
        };
        let Some(tools) = hud.reward.as_ref() else {
            ui.set_visible(window, false);
            continue;
        };

        ui.set_visible(window, true);
        // The panel inside the window is hidden in the layout as well; the
        // retail code centres it on the screen (ADAPTED: by moving the window
        // by what is left of the way, as `space_hud::centre_at_x`).
        if let Some(panel) = ui.find(rid::PANEL) {
            ui.set_visible(panel, true);
            let (cx, cy) = ui.rect(panel).center();
            let (tx, ty) = (ui.screen[0] * 0.5, ui.screen[1] * 0.5);
            if ui.screen[0] > 1.0 && ((tx - cx).abs() > 0.5 || (ty - cy).abs() > 0.5) {
                let mut area = ui.area(window);
                area[0] += tx - cx;
                area[2] += tx - cx;
                area[1] += ty - cy;
                area[3] += ty - cy;
                ui.set_area(window, area);
            }
        }
        let infos: Vec<(String, palette::ToolInfo)> = tools
            .iter()
            .map(|id| {
                let info = cache.get_any(id, index, &mut assets).unwrap_or_default();
                (id.clone(), info)
            })
            .collect();
        if let Some((id, first)) = infos.first() {
            let name = if first.name.is_empty() { id.clone() } else { first.name.clone() };
            if let Some(w) = ui.find(rid::TITLE) {
                ui.set_text(w, name.clone());
            }
            if let Some(w) = ui.find(rid::DESCRIPTION) {
                ui.set_text(w, first.description.clone());
            }
            if let Some(w) = ui.find(rid::ICON) {
                let icon = first.icon.and_then(|key| {
                    assets.texture(index, &mut images, key).map(|_| ImageRef {
                        file: key,
                        uv: [0.0, 0.0, 1.0, 1.0],
                        dims: None,
                    })
                });
                ui.set_icon(w, icon);
            }
        }
        for (i, control) in rid::LINES.iter().enumerate() {
            if let Some(w) = ui.find(*control) {
                let line =
                    infos.get(i).map(|(id, info)| if info.name.is_empty() { id.clone() } else { info.name.clone() });
                ui.set_visible(w, line.is_some());
                if let Some(line) = line {
                    ui.set_text(w, line);
                }
            }
        }
        // The window's own message and name windows are not used.
        for control in [rid::NAME, rid::MESSAGE] {
            if let Some(w) = ui.find(control) {
                ui.set_visible(w, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_new_announced_tools_are_rewards() {
        let before = set(&["scan", "laserlevel1"]);
        let now: Vec<String> = ["scan", "laserlevel1", "interplanetarydrive", "healthupgrade2", "seti"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(new_tools(&before, &now), ["interplanetarydrive", "seti"]);
        assert!(new_tools(&before, &["scan".to_string()]).is_empty());
    }

    #[test]
    fn the_queue_shows_one_reward_at_a_time() {
        let mut state = RewardState::default();
        assert!(state.open().is_none());
        state.queue.push(vec!["a".into()]);
        state.queue.push(vec!["b".into()]);
        assert_eq!(state.open(), Some(&vec!["a".to_string()]));
        state.dismiss();
        assert_eq!(state.open(), Some(&vec!["b".to_string()]));
        state.dismiss();
        state.dismiss();
        assert!(state.open().is_none());
    }
}
