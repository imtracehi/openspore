//! The tool palette of the Space HUD: eight tabs, a paged panel per tab and one
//! tool button per slot.
//!
//! The layout `GlobalUISpace2-1` carries the tab buttons (`B0000000..07`, radio
//! buttons), the panels (`B0000010..17`, each with two rows of slots between
//! page arrows) and the Hide button; the retail code fills the rows with one
//! button per tool. Here each slot is a root of its own (`SpaceMainTool`, a
//! 40x42 button with a count and a key label) that [`bind_items`] lays over the
//! slot frames of the active panel, with the tool's installed icon, name and
//! description (`spacetools~` property files, strings of `spacegame_tools`).
//!
//! The rewrite has no tool inventory yet (`space-rules` adds one): the tools
//! shown are today's Space actions under the retail tool each one stands for
//! ([`TOOLS`], ADAPTED), and a click presses the key the action already has.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{SpaceHudRoot, SpaceHudState, SpaceHudUi};
use crate::{
    asset_catalog::ResourceIndex,
    dbpf::ResourceKey,
    game::RunProgress,
    space::{PlanetaryTool, SpaceArchetype, SpaceCampaign},
    space_rules::{self as rules, tool as retail},
    spui::{
        model::ImageRef,
        solve::Rect,
        view::{SpuiAssets, SpuiState},
    },
};

pub const TOOL_LAYOUT: &str = "SpaceMainTool";
/// Slots shown at once: two rows of eight.
pub const COLUMNS: usize = 8;
pub const ROWS: usize = 2;
pub const SLOTS: usize = COLUMNS * ROWS;
/// Size of a tool button (the authored size of `SpaceMainTool`).
const SLOT_W: f32 = 40.0;
const SLOT_H: f32 = 42.0;

/// Control ids inside the palette and the tool button layout.
#[allow(dead_code)]
pub mod pid {
    pub const PALETTE: u32 = 0x05D4_2AF0;
    pub const HIDE: u32 = 0x453E_F531;
    /// Previous / next page arrows of a row.
    pub const PAGE_PREV: u32 = 0x05F7_2D8A;
    pub const PAGE_NEXT: u32 = 0x05F7_2D7C;
    /// Slot frame (the clip window of a row of slots).
    pub const SLOT_FRAME: u32 = 0x05F7_C9B3;
    pub const JETTISON: u32 = 0xB200_1000;
    // SpaceMainTool: count text and key label.
    pub const TOOL_COUNT: u32 = 0x05F7_2E19;
    pub const TOOL_KEY: u32 = 0x0628_40D8;
}

/// The eight tabs of the palette, in the order of the tab ids `B0000000..07`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Weapons,
    Social,
    Main,
    Coloring,
    Sculpting,
    Favorites,
    Cargo,
    /// Planet Atmospheric Tools (terraforming).
    Planet,
}

impl Tab {
    pub const ALL: [Tab; 8] =
        [Tab::Weapons, Tab::Social, Tab::Main, Tab::Coloring, Tab::Sculpting, Tab::Favorites, Tab::Cargo, Tab::Planet];

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// Radio button of the tab.
    pub fn tab_control(self) -> u32 {
        0xB000_0000 + self.index() as u32
    }

    /// Panel the tab shows.
    pub fn panel_control(self) -> u32 {
        0xB000_0010 + self.index() as u32
    }

    pub fn from_control(control: u32) -> Option<Tab> {
        Tab::ALL.iter().copied().find(|t| t.tab_control() == control)
    }

    /// The two row containers of a panel of the standard shape (zero: the
    /// panel is a special one, see [`slot_rects`]).
    pub fn rows(self) -> [u32; 2] {
        match self {
            Tab::Weapons => [0x05F4_CFF4, 0x05F4_D038],
            Tab::Social => [0x05F7_DE22, 0x05F7_DE2C],
            Tab::Main => [0x05F4_D21F, 0x05F4_D24C],
            Tab::Favorites => [0x05F4_C6B8, 0x05F4_C6E8],
            Tab::Planet => [0x05F4_BD68, 0x05F4_C5A0],
            Tab::Coloring | Tab::Sculpting | Tab::Cargo => [0, 0],
        }
    }

    /// Container of the grid of a special panel.
    fn grid(self) -> u32 {
        match self {
            Tab::Coloring => 0x05F4_CE63,
            Tab::Sculpting => 0x05F4_D12F,
            Tab::Cargo => 0x05F4_C660,
            _ => 0,
        }
    }
}

/// Where a tool's ownership comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A tool the UFO owns (`SpaceCampaign::has_tool`): scan, abduct, the laser.
    Owned,
    /// A consumable item (`charges`, bought at the store price when none is
    /// carried): the colony pack.
    Consumable,
    /// A planetary tool of the Space module (selected with T, used with Y).
    Planetary(PlanetaryTool),
    /// The archetype's superpower (R).
    Superpower,
    /// A tool of the installed table (`space_tool_use::generic_palette_tools`):
    /// a kept tool (`has_tool`) or one with charges, placed by its record's own
    /// `spaceToolPanelKey`.
    Generic,
}

/// One tool the palette can show: the retail tool it stands for, where it sits
/// and the Space action behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolDef {
    /// Name of the retail tool (`spacetools~` property file).
    pub id: &'static str,
    pub tab: Tab,
    /// Row of the tab's panel: 0 the ability row, 1 the consumable row (the
    /// retail panels, see [`panel_placement`]).
    pub row: u8,
    pub source: Source,
    /// The key of the action today.
    pub key: Option<KeyCode>,
    /// The planetary tool a click selects (T cycles, Y uses it).
    pub planetary: Option<PlanetaryTool>,
    /// The action repeats while the button is held (the laser).
    pub hold: bool,
    /// The archetype superpower (R).
    pub superpower: bool,
    /// A combat tool F fires once it is selected (`SpaceCampaign::weapon_tool`).
    pub weapon: bool,
    /// A ship tool used the moment its button is pressed (`toolUseOnActivate`:
    /// shield, cloak, potions; `space_tool_use::space_self_tools`).
    pub self_use: bool,
}

const fn owned(id: &'static str, tab: Tab, row: u8, key: KeyCode) -> ToolDef {
    ToolDef { id, tab, row, source: Source::Owned, key: Some(key), planetary: None, hold: false, superpower: false, weapon: false, self_use: false }
}

/// A combat tool F fires while its button is held (the lasers, the pulse and
/// proton weapons, the bombs: the combat ability panel).
const fn weapon_tool(id: &'static str) -> ToolDef {
    ToolDef {
        id,
        tab: Tab::Weapons,
        row: 0,
        source: Source::Owned,
        key: Some(KeyCode::KeyF),
        planetary: None,
        hold: true,
        superpower: false,
        weapon: true,
        self_use: false,
    }
}

const fn planetary(id: &'static str, tab: Tab, row: u8, which: PlanetaryTool) -> ToolDef {
    ToolDef {
        id,
        tab,
        row,
        source: Source::Planetary(which),
        key: None,
        planetary: Some(which),
        hold: false,
        superpower: false,
        weapon: false,
        self_use: false,
    }
}

/// The tabs and rows of the retail palette panels, from the `spaceToolPanelKey`
/// property of the tools (read from the install: the key hashes the panel
/// names): ability panels hold the reusable tools, consumable panels the
/// ones that are used up. The Cargo tab holds the abduct tool and the cargo.
#[allow(dead_code)] // checked against the install by a test (the tab and row of `TOOLS`)
pub fn panel_placement(panel: &str) -> Option<(Tab, u8)> {
    Some(match panel {
        "TravelAbilityPanel" => (Tab::Main, 0),
        "ColonizeConsumablePanel" => (Tab::Main, 1),
        "CombatAbilityPanel" => (Tab::Weapons, 0),
        "CombatConsumablePanel" => (Tab::Weapons, 1),
        "SocialAbilityPanel" => (Tab::Social, 0),
        "SocialConsumablePanel" => (Tab::Social, 1),
        "AtmosphericAbilityPanel" => (Tab::Planet, 0),
        "AtmosphericConsumablePanel" => (Tab::Planet, 1),
        "CargoAbductPanel" => (Tab::Cargo, 0),
        "ColorPanel" => (Tab::Coloring, 0),
        "SculptingPanel" => (Tab::Sculpting, 0),
        _ => return None,
    })
}

/// The panel names of the retail palette (the keys of `spaceToolPanelKey`).
#[allow(dead_code)]
pub const PANELS: [&str; 11] = [
    "TravelAbilityPanel",
    "ColonizeConsumablePanel",
    "CombatAbilityPanel",
    "CombatConsumablePanel",
    "SocialAbilityPanel",
    "SocialConsumablePanel",
    "AtmosphericAbilityPanel",
    "AtmosphericConsumablePanel",
    "CargoAbductPanel",
    "ColorPanel",
    "SculptingPanel",
];

/// Tab and row of the palette panel whose name hashes to `hash`.
#[allow(dead_code)]
pub fn panel_of(hash: u32) -> Option<(Tab, u8)> {
    PANELS.iter().find(|name| crate::world_objects::name_id(name) == hash).and_then(|name| panel_placement(name))
}

/// Today's Space actions as retail tools (ADAPTED stand-ins): the tab and row
/// of each are the retail tool's own panel (checked against the install by a
/// test), the key is the action's.
pub const TOOLS: &[ToolDef] = &[
    // Travel ability panel (Main Tools) and the colony pack.
    owned(retail::SCAN, Tab::Main, 0, KeyCode::KeyV),
    ToolDef {
        id: retail::PLACE_COLONY,
        tab: Tab::Main,
        row: 1,
        source: Source::Consumable,
        key: Some(KeyCode::KeyC),
        planetary: None,
        hold: false,
        superpower: false,
        weapon: false,
        self_use: false,
    },
    // Combat panels.
    weapon_tool(retail::LASER_1),
    weapon_tool(retail::LASER_2),
    weapon_tool(retail::LASER_3),
    weapon_tool("pulselevel1"),
    weapon_tool("pulselevel2"),
    weapon_tool("pulselevel3"),
    weapon_tool("protonmissilelevel1"),
    weapon_tool("protonmissilelevel2"),
    weapon_tool("protonmissilelevel3"),
    weapon_tool("dumbbomblevel1"),
    weapon_tool("dumbbomblevel2"),
    weapon_tool("dumbbomblevel3"),
    planetary(retail::PLANET_BUSTER, Tab::Weapons, 1, PlanetaryTool::PlanetBuster),
    // Social panels.
    planetary(retail::HAPPY_RAY_1, Tab::Social, 0, PlanetaryTool::HappyRay),
    planetary(retail::MONOLITH, Tab::Social, 1, PlanetaryTool::Monolith),
    planetary(retail::CREATURE_EDIT, Tab::Social, 1, PlanetaryTool::CreatureTweaker),
    // Atmospheric panels.
    planetary(retail::HEAT_RAY, Tab::Planet, 0, PlanetaryTool::Warm),
    planetary(retail::COLD_RAY, Tab::Planet, 0, PlanetaryTool::Cool),
    planetary(retail::CLOUD_RAY, Tab::Planet, 0, PlanetaryTool::ThickenAtmosphere),
    planetary(retail::CLOUD_VACUUM, Tab::Planet, 0, PlanetaryTool::ThinAtmosphere),
    planetary("hotcloudray", Tab::Planet, 0, PlanetaryTool::HeatRay),
    planetary("coldcloudvacuum", Tab::Planet, 0, PlanetaryTool::AtmosphereFreeze),
    planetary("coldcloudray", Tab::Planet, 0, PlanetaryTool::ColdCloudRay),
    planetary("hotcloudvacuum", Tab::Planet, 0, PlanetaryTool::HotCloudVacuum),
    planetary(retail::GENESIS_DEVICE, Tab::Planet, 1, PlanetaryTool::StaffOfLife),
    planetary(retail::ASTEROID, Tab::Planet, 1, PlanetaryTool::AsteroidCaller),
    planetary("volcano", Tab::Planet, 1, PlanetaryTool::Volcano),
    planetary("icecomet", Tab::Planet, 1, PlanetaryTool::IceComet),
    planetary("icecapseeder", Tab::Planet, 1, PlanetaryTool::IceCapSeeder),
    planetary("rainoffire", Tab::Planet, 1, PlanetaryTool::RainOfFire),
    planetary("dryice", Tab::Planet, 1, PlanetaryTool::DryIce),
    planetary(retail::ATMOSPHERE_GENERATOR, Tab::Planet, 1, PlanetaryTool::AtmosphereGenerator),
    planetary(retail::DROUGHT_GENERATOR, Tab::Planet, 1, PlanetaryTool::DroughtGenerator),
    // Cargo tab: the abduct tool.
    owned(retail::ABDUCT, Tab::Cargo, 0, KeyCode::KeyU),
];

/// The tab of an archetype's superpower: the retail superpower tool sits in
/// the ability panel of its category (travel, combat or social).
pub fn superpower_tab(archetype: SpaceArchetype) -> Tab {
    match archetype {
        SpaceArchetype::Warrior | SpaceArchetype::Scientist | SpaceArchetype::Diplomat | SpaceArchetype::Knight => {
            Tab::Weapons
        }
        SpaceArchetype::Zealot | SpaceArchetype::Trader | SpaceArchetype::Bard => Tab::Social,
        SpaceArchetype::Ecologist | SpaceArchetype::Shaman | SpaceArchetype::Wanderer => Tab::Main,
    }
}

/// The archetype's superpower as a palette tool (retail id and tab); the
/// Wanderer has none.
pub fn superpower_tool(archetype: SpaceArchetype) -> Option<ToolDef> {
    let power = rules::archetype_superpower(archetype)?;
    Some(ToolDef {
        id: power.id,
        tab: superpower_tab(archetype),
        row: 0,
        source: Source::Superpower,
        key: Some(KeyCode::KeyR),
        planetary: None,
        hold: false,
        superpower: true,
        weapon: false,
        self_use: false,
    })
}

/// The palette entries read from the installed table (`space-tools3`): every
/// tool with a `spaceToolPanelKey` whose class the Space rules run generically
/// ([`crate::space_tool_use::generic_palette_tools`]: the antimatter weapons
/// F fires, the shield, the cloak and the potions used at activation). The tab
/// and row are the record's own panel ([`panel_of`]), the Space action behind
/// them is the generic tool use.
pub fn generic_tools() -> &'static [ToolDef] {
    static LIST: std::sync::OnceLock<Vec<ToolDef>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        crate::space_tool_use::generic_palette_tools()
            .into_iter()
            .filter_map(|record| {
                let (tab, row) = panel_of(record.panel)?;
                let weapon = crate::space_tool_use::weapon(record.name).is_some();
                Some(ToolDef {
                    id: record.name,
                    tab,
                    row,
                    source: Source::Generic,
                    key: weapon.then_some(KeyCode::KeyF),
                    planetary: None,
                    hold: weapon,
                    superpower: false,
                    weapon,
                    self_use: !weapon,
                })
            })
            .collect()
    })
}

/// A tool the palette shows, with its state.
#[derive(Clone, Debug, PartialEq)]
pub struct HudTool {
    pub def: ToolDef,
    /// The button is enabled.
    pub usable: bool,
    /// The selected planetary tool or combat tool.
    pub active: bool,
    /// Remaining uses, when the tool counts them.
    pub count: Option<u32>,
    /// Why the tool cannot be used right now.
    pub locked: Option<String>,
    /// Recharge left, `0.0..=1.0` (1.0 just used).
    pub recharge: f32,
}

/// Sporebucks price of a planetary tool bought on first use (the cost
/// constants of the Space module).
fn planetary_cost(which: PlanetaryTool) -> Option<f32> {
    use crate::space::*;
    match which {
        PlanetaryTool::Monolith => Some(MONOLITH_COST),
        PlanetaryTool::PlanetBuster => Some(PLANET_BUSTER_COST),
        PlanetaryTool::AtmosphereFreeze => Some(ATMOSPHERE_FREEZE_COST),
        PlanetaryTool::HeatRay => Some(HEAT_RAY_COST),
        PlanetaryTool::AsteroidCaller => Some(ASTEROID_CALLER_COST),
        PlanetaryTool::CreatureTweaker => Some(CREATURE_TWEAKER_COST),
        // The other throwing terraformers are bought at their store row's price.
        which if which.is_throwable() => {
            let name = crate::space_tool_fx::planetary_tool_id(which);
            rules::store_price(name).or_else(|| crate::space_tools::record(name).map(|r| r.buy_cost as f32))
        }
        _ => None,
    }
}

/// The tools of the palette from the campaign: the tools the UFO owns that
/// have an action, the consumables and planetary tools (with the price the
/// action asks when none is owned) and the archetype's superpower. Reads
/// `SpaceCampaign::{tools, has_tool, charges, archetype, superweapon_cooldown,
/// planetary_tool, staff_of_life_charges}` and
/// `space_rules::{store_price, laser_level, archetype_superpower}`.
pub fn build_tools(campaign: &SpaceCampaign, wallet: f32) -> Vec<HudTool> {
    let mut out = Vec::new();
    let best_laser = rules::laser_level(&campaign.tools);
    let mut push = |def: ToolDef| {
        let mut usable = true;
        let mut locked = None;
        let mut count = None;
        let mut recharge = 0.0;
        match def.source {
            Source::Owned => {}
            Source::Consumable => {
                let charges = campaign.charges(def.id);
                count = Some(charges);
                let price = rules::store_price(def.id).unwrap_or(0.0);
                if !campaign.has_tool(retail::INTERPLANETARY_DRIVE) {
                    usable = false;
                    locked = Some("Needs the Interplanetary Drive".to_string());
                } else if charges == 0 && wallet < price {
                    usable = false;
                    locked = Some(format!("Costs {price:.0} Sporebucks (you have {wallet:.0})"));
                }
            }
            Source::Planetary(which) => {
                if which == PlanetaryTool::StaffOfLife {
                    count = Some(campaign.staff_of_life_charges as u32);
                    if campaign.staff_of_life_charges == 0 {
                        usable = false;
                        locked = Some("The Staff of Life is granted at the galactic core".into());
                    }
                } else if let Some(cost) = planetary_cost(which) {
                    let owned = campaign.has_tool(def.id) || campaign.charges(def.id) > 0;
                    if !owned && wallet < cost {
                        usable = false;
                        locked = Some(format!("Costs {cost:.0} Sporebucks (you have {wallet:.0})"));
                    }
                }
            }
            Source::Generic => {
                let record = crate::space_tools::record(def.id);
                if record.is_some_and(|r| r.uses_ammo()) {
                    count = Some(campaign.charges(def.id));
                }
                // A ship tool's own recharge (`RechargeRate` after a use).
                if let (Some(clock), Some(record)) = (campaign.generic_clocks.get(def.id), record) {
                    if clock.recharge_left() > 0.0 && record.recharge_rate > 0.0 {
                        recharge = (clock.recharge_left() / record.recharge_rate).clamp(0.0, 1.0);
                        usable = false;
                        locked = Some(format!("Recharging ({:.0} s)", clock.recharge_left()));
                    }
                }
            }
            Source::Superpower => {
                if campaign.superweapon_cooldown > 0.0 {
                    let total = campaign.archetype.cooldown_secs().max(1.0);
                    recharge = (campaign.superweapon_cooldown / total).clamp(0.0, 1.0);
                    usable = false;
                    locked = Some(format!("Recharging ({:.0} s)", campaign.superweapon_cooldown));
                }
            }
        }
        out.push(HudTool {
            def,
            usable,
            active: def.planetary.is_some_and(|p| p == campaign.planetary_tool)
                || (def.weapon && campaign.selected_weapon().id == def.id),
            count,
            locked,
            recharge,
        });
    };
    for def in TOOLS.iter().chain(generic_tools()) {
        let show = match def.source {
            // The table tools: kept, or carried as charges.
            Source::Generic => campaign.has_tool(def.id) || campaign.charges(def.id) > 0,
            // Only the best laser is shown, and only the tools the UFO owns.
            Source::Owned => {
                campaign.has_tool(def.id)
                    && match def.id {
                        retail::LASER_1 => best_laser == 1,
                        retail::LASER_2 => best_laser == 2,
                        retail::LASER_3 => best_laser == 3,
                        id if def.weapon => best_level(campaign, id) == Some(id),
                        _ => true,
                    }
            }
            _ => true,
        };
        if show {
            push(*def);
        }
    }
    if let Some(def) = superpower_tool(campaign.archetype) {
        push(def);
    }
    out
}

/// The best owned level of the weapon family `id` belongs to (`pulselevel1..3`,
/// `protonmissilelevel1..3`, `dumbbomblevel1..3`): the palette shows one
/// button per family, like the laser.
fn best_level(campaign: &SpaceCampaign, id: &str) -> Option<&'static str> {
    const FAMILIES: [[&str; 3]; 3] = [
        ["pulselevel1", "pulselevel2", "pulselevel3"],
        ["protonmissilelevel1", "protonmissilelevel2", "protonmissilelevel3"],
        ["dumbbomblevel1", "dumbbomblevel2", "dumbbomblevel3"],
    ];
    let family = FAMILIES.iter().find(|f| f.contains(&id))?;
    family.iter().rev().copied().find(|level| campaign.has_tool(level))
}

/// Tools of a tab, standard panels row by row (indices into `tools`: the
/// ability row first, then the consumable row), the special panels in order.
pub fn tab_tools(tools: &[HudTool], tab: Tab) -> Vec<usize> {
    let mut list: Vec<usize> = tools.iter().enumerate().filter(|(_, t)| t.def.tab == tab).map(|(i, _)| i).collect();
    list.sort_by_key(|i| tools[*i].def.row);
    list
}

/// Pages a tab needs: the longest row (standard panels) or the whole list
/// (special panels) over the slots one page holds.
pub fn page_count(tools: &[HudTool], tab: Tab) -> usize {
    let list = tab_tools(tools, tab);
    if tab.rows()[0] == 0 {
        return list.len().div_ceil(SLOTS).max(1);
    }
    let longest =
        (0..ROWS as u8).map(|row| list.iter().filter(|i| tools[**i].def.row == row).count()).max().unwrap_or(0);
    longest.div_ceil(COLUMNS).max(1)
}

/// The tool each of the sixteen slots shows on `page`: standard panels hold
/// one row of tools per panel row (so a row never mixes ability and
/// consumable tools), the special panels fill the slots in order.
pub fn slot_assignment(tools: &[HudTool], tab: Tab, page: usize) -> Vec<Option<usize>> {
    let list = tab_tools(tools, tab);
    if tab.rows()[0] == 0 {
        return (0..SLOTS).map(|slot| list.get(page * SLOTS + slot).copied()).collect();
    }
    (0..SLOTS)
        .map(|slot| {
            let (row, col) = (slot / COLUMNS, slot % COLUMNS);
            list.iter().copied().filter(|i| tools[*i].def.row as usize == row).nth(page * COLUMNS + col)
        })
        .collect()
}

/// Screen rectangles of the slots of a tab's panel, row-major (row 0 first).
/// Standard panels have two rows of eight between their page arrows; the
/// special panels (cargo, sculpting, colouring) are a grid inside one frame.
pub fn slot_rects(ui: &SpuiState, tab: Tab) -> Vec<Rect> {
    let frames_in = |container: u32| -> Vec<Rect> {
        ui.find_all(pid::SLOT_FRAME)
            .into_iter()
            .filter(|w| ui.layout.window(*w).parent.is_some_and(|p| ui.layout.window(p).control_id == container))
            .map(|w| ui.rect(w))
            .collect()
    };
    let mut out = Vec::new();
    let rows = tab.rows();
    if rows[0] != 0 {
        for container in rows {
            if let Some(frame) = frames_in(container).first() {
                let cy = (frame.y1 + frame.y2) * 0.5;
                for c in 0..COLUMNS {
                    let x = frame.x1 + c as f32 * SLOT_W;
                    out.push(Rect::new(x, cy - SLOT_H * 0.5, x + SLOT_W, cy + SLOT_H * 0.5));
                }
            }
        }
    } else {
        let grid = tab.grid();
        // The cargo panel's tool slot (the abduct tool) sits left of the grid.
        if tab == Tab::Cargo {
            if let Some(w) = ui.find(0x05F4_C608) {
                let r = ui.rect(w);
                let cy = (r.y1 + r.y2) * 0.5;
                out.push(Rect::new(r.x1, cy - SLOT_H * 0.5, r.x1 + SLOT_W, cy + SLOT_H * 0.5));
            }
        }
        let frame = ui
            .find_all(pid::SLOT_FRAME)
            .into_iter()
            .filter(|w| {
                super::within(ui, *w, grid)
                    || ui.layout.window(*w).parent.is_some_and(|p| ui.layout.window(p).control_id == grid)
            })
            .map(|w| ui.rect(w))
            .next();
        if let Some(frame) = frame {
            let columns = ((frame.width() / SLOT_W).floor() as usize).clamp(1, COLUMNS);
            let height = frame.height() / ROWS as f32;
            for r in 0..ROWS {
                for c in 0..COLUMNS {
                    if c >= columns {
                        // Keep the row-major layout of eight per row: unused columns have no rect.
                        out.push(Rect::new(0.0, 0.0, 0.0, 0.0));
                        continue;
                    }
                    let x = frame.x1 + c as f32 * SLOT_W;
                    let cy = frame.y1 + height * (r as f32 + 0.5);
                    out.push(Rect::new(x, cy - SLOT_H * 0.5, x + SLOT_W, cy + SLOT_H * 0.5));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tool information from the install.
// ---------------------------------------------------------------------------

/// Property ids of a `spacetools~` property file.
const TOOL_GROUP: u32 = 0x3060_8F0B;
const PROP_NAME: u32 = 0x3068_D95D;
const PROP_ICON: u32 = 0x3068_D95C;
const PROP_DESCRIPTION: u32 = 0x04CA_D19B;
const PROP_BLOCKED: u32 = 0x05E6_2E33;
/// `spaceToolPanelKey`: the hash of the name of the palette panel of the tool.
const PROP_PANEL: u32 = 0xA6A3_7FC4;

/// What the palette shows of one retail tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// The tool's own "you cannot use it now" message.
    pub blocked: String,
    pub icon: Option<ResourceKey>,
    /// Hash of the tool's palette panel name (see [`panel_of`]).
    pub panel: Option<u32>,
}

#[derive(Resource, Default)]
pub struct ToolInfoCache {
    tools: HashMap<&'static str, Option<ToolInfo>>,
    /// Tools named at run time (the reward window lists any tool id).
    named: HashMap<String, Option<ToolInfo>>,
}

fn text_of(props: &crate::prop::PropertyList, id: u32, index: &ResourceIndex, assets: &mut SpuiAssets) -> String {
    props
        .0
        .get(&id)
        .and_then(|p| p.text())
        .map(
            |(table, text_id, literal)| {
                if table != 0 {
                    assets.localized(index, table, text_id).unwrap_or(literal)
                } else {
                    literal
                }
            },
        )
        .unwrap_or_default()
}

/// Read the information of the retail tool `name` from the install.
pub fn read_tool(index: &ResourceIndex, assets: &mut SpuiAssets, name: &str) -> Option<ToolInfo> {
    let key = ResourceKey {
        group_id: TOOL_GROUP,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let props = index.properties(key).ok()?;
    let icon = props.0.get(&PROP_ICON).and_then(|p| p.key()).map(|mut k| {
        if k.type_id == 0 {
            k.type_id = crate::ui_art::UI_PNG_TYPE;
        }
        k
    });
    Some(ToolInfo {
        name: text_of(&props, PROP_NAME, index, assets),
        description: text_of(&props, PROP_DESCRIPTION, index, assets),
        blocked: text_of(&props, PROP_BLOCKED, index, assets),
        icon,
        panel: props.0.get(&PROP_PANEL).and_then(|p| p.key()).map(|k| k.instance_id),
    })
}

impl ToolInfoCache {
    /// The information of a tool named at run time.
    pub fn get_any(&mut self, name: &str, index: &ResourceIndex, assets: &mut SpuiAssets) -> Option<ToolInfo> {
        if !self.named.contains_key(name) {
            let info = read_tool(index, assets, name);
            self.named.insert(name.to_string(), info);
        }
        self.named.get(name).cloned().flatten()
    }

    pub fn get(&mut self, name: &'static str, index: &ResourceIndex, assets: &mut SpuiAssets) -> Option<ToolInfo> {
        self.tools.entry(name).or_insert_with(|| read_tool(index, assets, name)).clone()
    }
}

/// Tooltip text of a tool: its name, the key and the description, then why it
/// is not usable.
pub fn tool_tooltip(tool: &HudTool, info: Option<&ToolInfo>) -> String {
    let name = info.map(|i| i.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(tool.def.id);
    let key = tool.def.key.map(|k| format!(" [{}]", key_label(k))).unwrap_or_default();
    let mut text = format!("{name}{key}");
    if let Some(info) = info {
        if !info.description.is_empty() {
            text.push('\n');
            text.push_str(&info.description);
        }
    }
    if let Some(reason) = &tool.locked {
        text.push('\n');
        text.push_str(reason);
    }
    text
}

/// The label of a key on a button.
pub fn key_label(key: KeyCode) -> String {
    match key {
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyK => "K",
        KeyCode::KeyM => "M",
        KeyCode::KeyR => "R",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyY => "Y",
        _ => "",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Binding.
// ---------------------------------------------------------------------------

/// Show the tab buttons, the panel of the selected tab and its page arrows in
/// the main layout.
pub fn bind_panels(
    ui: &mut SpuiState,
    hud: &SpaceHudState,
    hud_ui: &SpaceHudUi,
    _assets: &mut SpuiAssets,
    _images: &mut Assets<Image>,
    _index: &ResourceIndex,
) {
    if let Some(w) = super::window(ui, pid::HIDE) {
        ui.set_selected(w, hud_ui.palette_hidden);
    }
    for tab in Tab::ALL {
        if let Some(w) = super::window(ui, tab.tab_control()) {
            ui.set_selected(w, tab == hud_ui.tab);
            ui.set_enabled(w, true);
        }
        if let Some(panel) = super::window(ui, tab.panel_control()) {
            let show = tab == hud_ui.tab && !hud_ui.palette_hidden;
            ui.set_visible(panel, show);
            ui.set_enabled(panel, true);
            // Page arrows only when the tab has more tools than one page holds.
            let pages = page_count(&hud.tools, tab);
            for arrow in ui.find_all(pid::PAGE_PREV).into_iter().chain(ui.find_all(pid::PAGE_NEXT)) {
                if super::within(ui, arrow, tab.panel_control()) {
                    ui.set_visible(arrow, show && pages > 1);
                }
            }
        }
    }
    super::set_visible(ui, pid::JETTISON, false);
}

/// Lay the tool buttons over the slots of the active panel.
#[allow(clippy::too_many_arguments)]
pub fn bind_items(
    hud: Res<SpaceHudState>,
    mut hud_ui: ResMut<SpaceHudUi>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<ToolInfoCache>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    let tab = hud_ui.tab;
    let pages = page_count(&hud.tools, tab);
    let page = hud_ui.page[tab.index()].min(pages - 1);
    hud_ui.page[tab.index()] = page;
    let assignment = slot_assignment(&hud.tools, tab, page);
    let rects = roots
        .iter()
        .find(|(ui, kind)| **kind == SpaceHudRoot::Main && ui.ready)
        .map(|(ui, _)| slot_rects(ui, tab))
        .unwrap_or_default();
    let mut slot_tools = vec![None; SLOTS];
    for (mut ui, kind) in &mut roots {
        let SpaceHudRoot::Tool(slot) = kind else {
            continue;
        };
        if !ui.ready {
            continue;
        }
        let Some(&button) = ui.layout.roots.first() else {
            continue;
        };
        let slot = *slot as usize;
        let rect = rects.get(slot).copied().filter(|r| r.width() > 0.0);
        let tool_index = assignment.get(slot).copied().flatten().filter(|_| !hud_ui.palette_hidden);
        let (Some(rect), Some(tool_index)) = (rect, tool_index) else {
            ui.set_visible(button, false);
            continue;
        };
        let tool = &hud.tools[tool_index];
        slot_tools[slot] = Some(tool_index);
        let info = cache.get(tool.def.id, index, &mut assets);
        ui.set_visible(button, true);
        ui.set_area(button, [rect.x1, rect.y1, rect.x1 + SLOT_W, rect.y1 + SLOT_H]);
        ui.set_enabled(button, tool.usable);
        ui.set_selected(button, tool.active);
        let icon = info.as_ref().and_then(|i| i.icon).and_then(|key| {
            assets.texture(index, &mut images, key).map(|_| ImageRef {
                file: key,
                uv: [0.0, 0.0, 1.0, 1.0],
                dims: None,
            })
        });
        ui.set_icon(button, icon);
        ui.set_tooltip(button, Some(tool_tooltip(tool, info.as_ref())));
        if let Some(w) = ui.find(pid::TOOL_KEY) {
            let label = tool.def.key.map(key_label).unwrap_or_default();
            ui.set_visible(w, !label.is_empty());
            ui.set_text(w, label);
        }
        if let Some(w) = ui.find(pid::TOOL_COUNT) {
            ui.set_visible(w, tool.count.is_some());
            ui.set_text(w, tool.count.map(|c| c.to_string()).unwrap_or_default());
        }
    }
    if hud_ui.slot_tools != slot_tools {
        hud_ui.slot_tools = slot_tools;
    }
}

// ---------------------------------------------------------------------------
// Clicks.
// ---------------------------------------------------------------------------

/// A click on one of the palette's own controls of the main layout.
pub fn click(hud_ui: &mut SpaceHudUi, control: u32) {
    if let Some(tab) = Tab::from_control(control) {
        hud_ui.tab = tab;
        return;
    }
    match control {
        pid::HIDE => hud_ui.palette_hidden = !hud_ui.palette_hidden,
        pid::PAGE_PREV => {
            let page = &mut hud_ui.page[hud_ui.tab.index()];
            *page = page.saturating_sub(1);
        }
        pid::PAGE_NEXT => hud_ui.page[hud_ui.tab.index()] += 1,
        _ => {}
    }
}

/// A click on a tool button: select the planetary tool or press the key of
/// the action. The planetary tool is the one write into Space state the HUD
/// makes (the player's own choice, like T).
pub fn select(
    tool: &HudTool,
    run: &mut RunProgress,
    keys: &mut ButtonInput<KeyCode>,
    injected: &mut crate::player_hud::InjectedKeys,
) {
    if !tool.usable {
        return;
    }
    if let Some(which) = tool.def.planetary {
        if run.space_campaign.planetary_tool != which {
            run.space_campaign.planetary_tool = which;
            run.request_save();
        }
        run.space_campaign.message = format!("{} selected: press Y near a world to use it", which.label());
        return;
    }
    // A ship tool used at activation: the click is the use.
    if tool.def.self_use {
        crate::space_tool_use::request_self_use(run, tool.def.id);
        return;
    }
    // A combat tool: the click selects what F fires (the held button fires it).
    if tool.def.weapon {
        if run.space_campaign.weapon_tool.as_deref() != Some(tool.def.id) {
            run.space_campaign.weapon_tool = Some(tool.def.id.to_string());
            run.request_save();
        }
        run.space_campaign.message = format!("{} selected: hold F to fire", tool.def.id);
        return;
    }
    // Held tools are pressed every frame by `actions` while the button is down.
    if let Some(key) = tool.def.key.filter(|_| !tool.def.hold) {
        super::press(keys, injected, key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A campaign that owns the default loadout (`spaceEnabledTools`).
    fn loaded() -> SpaceCampaign {
        let mut campaign = SpaceCampaign::default();
        for id in rules::ENABLED_TOOLS {
            campaign.grant_tool(id);
        }
        campaign
    }

    fn find<'a>(tools: &'a [HudTool], id: &str) -> Option<&'a HudTool> {
        tools.iter().find(|t| t.def.id == id)
    }

    #[test]
    fn tabs_and_panels_follow_the_layout_ids() {
        assert_eq!(Tab::Weapons.tab_control(), 0xB000_0000);
        assert_eq!(Tab::Planet.tab_control(), 0xB000_0007);
        assert_eq!(Tab::Weapons.panel_control(), 0xB000_0010);
        assert_eq!(Tab::Planet.panel_control(), 0xB000_0017);
        for tab in Tab::ALL {
            assert_eq!(Tab::from_control(tab.tab_control()), Some(tab));
            assert_eq!(Tab::ALL[tab.index()], tab);
        }
        assert_eq!(Tab::from_control(0xB000_0010), None, "a panel id is not a tab");
    }

    #[test]
    fn every_tool_is_listed_once_and_the_actions_keep_their_keys() {
        let mut ids: Vec<&str> = TOOLS.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TOOLS.len(), "no tool is listed twice");
        let key = |id: &str| TOOLS.iter().find(|t| t.id == id).and_then(|t| t.key);
        assert_eq!(key(retail::SCAN), Some(KeyCode::KeyV));
        assert_eq!(key(retail::ABDUCT), Some(KeyCode::KeyU));
        assert_eq!(key(retail::LASER_1), Some(KeyCode::KeyF));
        assert_eq!(key(retail::PLACE_COLONY), Some(KeyCode::KeyC));
        // Every planetary tool of the Space module that has a retail stand-in is there once.
        let tools: Vec<_> = TOOLS.iter().filter_map(|t| t.planetary).collect();
        for p in
            [PlanetaryTool::Warm, PlanetaryTool::Cool, PlanetaryTool::ThickenAtmosphere, PlanetaryTool::ThinAtmosphere]
        {
            assert_eq!(tools.iter().filter(|t| **t == p).count(), 1, "{p:?}");
        }
    }

    #[test]
    fn the_retail_panels_place_tools_in_tabs_and_rows() {
        assert_eq!(panel_placement("TravelAbilityPanel"), Some((Tab::Main, 0)));
        assert_eq!(panel_placement("ColonizeConsumablePanel"), Some((Tab::Main, 1)));
        assert_eq!(panel_placement("CombatConsumablePanel"), Some((Tab::Weapons, 1)));
        assert_eq!(panel_placement("CargoAbductPanel"), Some((Tab::Cargo, 0)));
        assert_eq!(panel_placement("NoSuchPanel"), None);
        for def in TOOLS {
            assert!(def.row <= 1, "{}", def.id);
        }
    }

    #[test]
    fn the_player_archetype_adds_its_superpower() {
        let mut campaign = loaded();
        campaign.archetype = SpaceArchetype::Bard;
        let tools = build_tools(&campaign, 0.0);
        let power = tools.iter().find(|t| t.def.superpower).expect("the superpower tool");
        assert_eq!(power.def.id, "soothingsong");
        assert_eq!(power.def.tab, Tab::Social);
        assert_eq!(power.def.key, Some(KeyCode::KeyR));
        assert!(power.usable && power.recharge == 0.0);
        // Recharging: unusable with the remaining fraction.
        campaign.superweapon_cooldown = campaign.archetype.cooldown_secs() * 0.5;
        let tools = build_tools(&campaign, 0.0);
        let power = tools.iter().find(|t| t.def.superpower).unwrap();
        assert!(!power.usable);
        assert!((power.recharge - 0.5).abs() < 1e-6);
        // The Wanderer has none; the others sit in the panel of their category.
        campaign.archetype = SpaceArchetype::Wanderer;
        assert!(build_tools(&campaign, 0.0).iter().all(|t| !t.def.superpower));
        assert_eq!(superpower_tab(SpaceArchetype::Warrior), Tab::Weapons);
        assert_eq!(superpower_tab(SpaceArchetype::Shaman), Tab::Main);
        assert_eq!(superpower_tab(SpaceArchetype::Trader), Tab::Social);
    }

    #[test]
    fn only_owned_tools_with_an_action_are_shown_and_the_best_laser_only() {
        let mut campaign = SpaceCampaign::default();
        assert!(find(&build_tools(&campaign, 0.0), "scan").is_none(), "nothing owned, nothing shown");
        let campaign_loaded = loaded();
        let tools = build_tools(&campaign_loaded, 0.0);
        for id in ["scan", "abduct", "laserlevel1"] {
            assert!(find(&tools, id).is_some_and(|t| t.usable), "{id}");
        }
        assert!(find(&tools, "laserlevel1").unwrap().def.hold);
        campaign.grant_tool("laserlevel1");
        campaign.grant_tool("laserlevel2");
        let tools = build_tools(&campaign, 0.0);
        assert!(find(&tools, "laserlevel1").is_none() && find(&tools, "laserlevel2").is_some());
        campaign.grant_tool("laserlevel3");
        let tools = build_tools(&campaign, 0.0);
        assert!(find(&tools, "laserlevel2").is_none() && find(&tools, "laserlevel3").is_some());
    }

    #[test]
    fn the_colony_pack_needs_the_drive_and_a_charge_or_the_price() {
        let mut campaign = loaded();
        let pack = |c: &SpaceCampaign, wallet: f32| find(&build_tools(c, wallet), "placecolony").cloned().unwrap();
        let p = pack(&campaign, 1e6);
        assert!(!p.usable && p.locked.as_deref().is_some_and(|l| l.contains("Interplanetary Drive")));
        campaign.grant_tool(retail::INTERPLANETARY_DRIVE);
        let poor = pack(&campaign, 10.0);
        assert!(!poor.usable && poor.locked.as_deref().is_some_and(|l| l.contains("150000")), "{poor:?}");
        assert!(pack(&campaign, 150_000.0).usable);
        campaign.add_charges(retail::PLACE_COLONY, 2);
        let carried = pack(&campaign, 0.0);
        assert!(carried.usable);
        assert_eq!(carried.count, Some(2));
    }

    #[test]
    fn planetary_tools_need_their_price_and_the_staff_its_charges() {
        let mut campaign = loaded();
        let monolith = |c: &SpaceCampaign, wallet: f32| find(&build_tools(c, wallet), "monolith").cloned().unwrap();
        let poor = monolith(&campaign, 10.0);
        assert!(!poor.usable && poor.locked.is_some());
        assert!(monolith(&campaign, crate::space::MONOLITH_COST).usable);
        campaign.grant_tool("monolith");
        assert!(monolith(&campaign, 0.0).usable, "an owned tool needs no money");
        let staff = |c: &SpaceCampaign| find(&build_tools(c, 1e9), "genesisdevice").cloned().unwrap();
        assert!(!staff(&campaign).usable);
        campaign.staff_of_life_charges = 3;
        let granted = staff(&campaign);
        assert!(granted.usable);
        assert_eq!(granted.count, Some(3));
        // The free climate tools are always there, the selected one is active.
        campaign.planetary_tool = PlanetaryTool::Cool;
        let tools = build_tools(&campaign, 0.0);
        assert!(find(&tools, "coldray").unwrap().usable && find(&tools, "coldray").unwrap().active);
        assert!(!find(&tools, "heatray").unwrap().active);
    }

    #[test]
    fn standard_panels_keep_ability_and_consumable_rows_apart_and_page_per_row() {
        let mut campaign = loaded();
        campaign.grant_tool(retail::INTERPLANETARY_DRIVE);
        campaign.staff_of_life_charges = 1;
        let tools = build_tools(&campaign, 0.0);
        // Atmospheric tab: the eight rays on the ability row, the throwing
        // terraformers (nine: two pages) on the consumable row.
        let slots = slot_assignment(&tools, Tab::Planet, 0);
        assert_eq!(slots.len(), SLOTS);
        let ids: Vec<_> = slots.iter().map(|s| s.map(|i| tools[i].def.id)).collect();
        assert_eq!(
            &ids[..COLUMNS],
            &[
                Some("heatray"),
                Some("coldray"),
                Some("cloudray"),
                Some("cloudvacuum"),
                Some("hotcloudray"),
                Some("coldcloudvacuum"),
                Some("coldcloudray"),
                Some("hotcloudvacuum")
            ]
        );
        assert_eq!(
            &ids[COLUMNS..],
            &[
                Some("genesisdevice"),
                Some("asteroid"),
                Some("volcano"),
                Some("icecomet"),
                Some("icecapseeder"),
                Some("rainoffire"),
                Some("dryice"),
                Some("atmospheregenerator")
            ]
        );
        assert_eq!(page_count(&tools, Tab::Planet), 2);
        let second = slot_assignment(&tools, Tab::Planet, 1);
        assert_eq!(second[COLUMNS].map(|i| tools[i].def.id), Some("droughtgenerator"));
        // Main tab: scan above, the colony pack on the consumable row.
        let main = slot_assignment(&tools, Tab::Main, 0);
        assert_eq!(main[0].map(|i| tools[i].def.id), Some("scan"));
        assert_eq!(main[COLUMNS].map(|i| tools[i].def.id), Some("placecolony"));
        // The cargo tab is a special panel: its tools fill the slots in order.
        let cargo = slot_assignment(&tools, Tab::Cargo, 0);
        assert_eq!(cargo[0].map(|i| tools[i].def.id), Some("abduct"));
        assert!(cargo[1..].iter().all(Option::is_none));
        // A tab with nothing is one empty page.
        assert!(slot_assignment(&tools, Tab::Coloring, 0).iter().all(Option::is_none));
        assert_eq!(page_count(&tools, Tab::Coloring), 1);
        // Paging works per row: nine tools on a row need two pages.
        let tool = |n: usize| HudTool {
            def: ToolDef { row: 0, ..TOOLS[0] },
            usable: true,
            active: false,
            count: Some(n as u32),
            locked: None,
            recharge: 0.0,
        };
        let many: Vec<HudTool> = (0..9).map(tool).collect();
        assert_eq!(page_count(&many, Tab::Main), 2);
        assert_eq!(slot_assignment(&many, Tab::Main, 0)[7], Some(7));
        assert_eq!(slot_assignment(&many, Tab::Main, 1)[0], Some(8));
        assert_eq!(slot_assignment(&many, Tab::Main, 1)[1], None);
    }

    #[test]
    fn clicks_switch_tabs_page_and_fold_the_palette() {
        let mut ui = SpaceHudUi::default();
        assert_eq!(ui.tab, Tab::Main);
        click(&mut ui, Tab::Weapons.tab_control());
        assert_eq!(ui.tab, Tab::Weapons);
        click(&mut ui, pid::PAGE_NEXT);
        click(&mut ui, pid::PAGE_NEXT);
        assert_eq!(ui.page[Tab::Weapons.index()], 2);
        click(&mut ui, pid::PAGE_PREV);
        click(&mut ui, pid::PAGE_PREV);
        click(&mut ui, pid::PAGE_PREV);
        assert_eq!(ui.page[Tab::Weapons.index()], 0, "the page never goes below the first");
        assert_eq!(ui.page[Tab::Main.index()], 0, "pages belong to their tab");
        click(&mut ui, pid::HIDE);
        assert!(ui.palette_hidden);
        click(&mut ui, pid::HIDE);
        assert!(!ui.palette_hidden);
    }

    #[test]
    fn a_click_selects_the_planetary_tool_or_presses_the_action_key() {
        let mut campaign = loaded();
        campaign.grant_tool(retail::INTERPLANETARY_DRIVE);
        let tools = build_tools(&campaign, 1e9);
        let mut run = RunProgress::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut injected = crate::player_hud::InjectedKeys::default();
        let cool = find(&tools, "coldray").unwrap();
        select(cool, &mut run, &mut keys, &mut injected);
        assert_eq!(run.space_campaign.planetary_tool, PlanetaryTool::Cool);
        assert!(!keys.pressed(KeyCode::KeyY), "selecting does not use the tool");
        let scan = find(&tools, "scan").unwrap();
        select(scan, &mut run, &mut keys, &mut injected);
        assert!(keys.just_pressed(KeyCode::KeyV));
        // An unusable tool does nothing.
        let mut locked = find(&tools, "abduct").unwrap().clone();
        locked.usable = false;
        select(&locked, &mut run, &mut keys, &mut injected);
        assert!(!keys.pressed(KeyCode::KeyU));
        // The held laser is pressed by the frame loop, not by the click.
        let laser = find(&tools, "laserlevel1").unwrap();
        assert!(laser.def.hold);
        select(laser, &mut run, &mut keys, &mut injected);
        assert!(!keys.pressed(KeyCode::KeyF));
    }

    #[test]
    fn combat_tools_show_their_best_level_and_a_click_selects_what_f_fires() {
        let mut campaign = loaded();
        // Pulse 1 and 2, proton 1 and the bomb 3 owned: one button per family, the best level.
        for id in ["pulselevel1", "pulselevel2", "protonmissilelevel1", "dumbbomblevel3"] {
            campaign.grant_tool(id);
        }
        let tools = build_tools(&campaign, 1e9);
        assert!(find(&tools, "pulselevel1").is_none(), "only the best pulse level shows");
        assert!(find(&tools, "pulselevel2").is_some());
        assert!(find(&tools, "protonmissilelevel1").is_some());
        assert!(find(&tools, "dumbbomblevel3").is_some());
        assert!(find(&tools, "pulselevel3").is_none(), "not owned");
        // The laser is the default selection; a click on the pulse selects it.
        assert!(find(&tools, "laserlevel1").unwrap().active);
        assert!(!find(&tools, "pulselevel2").unwrap().active);
        let mut run = RunProgress::default();
        run.space_campaign = campaign.clone();
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut injected = crate::player_hud::InjectedKeys::default();
        select(find(&tools, "pulselevel2").unwrap(), &mut run, &mut keys, &mut injected);
        assert_eq!(run.space_campaign.weapon_tool.as_deref(), Some("pulselevel2"));
        assert_eq!(run.space_campaign.selected_weapon().id, "pulselevel2");
        assert!(!keys.pressed(KeyCode::KeyF), "the held button fires, the click only selects");
        let tools = build_tools(&run.space_campaign, 1e9);
        assert!(find(&tools, "pulselevel2").unwrap().active);
        assert!(!find(&tools, "laserlevel1").unwrap().active);
        // Every combat tool sits on the combat ability panel and F fires it.
        for def in TOOLS.iter().filter(|t| t.weapon) {
            assert_eq!((def.tab, def.row, def.key), (Tab::Weapons, 0, Some(KeyCode::KeyF)), "{}", def.id);
            assert!(def.hold);
            assert!(rules::player_weapon(def.id).is_some(), "{} has a weapon row", def.id);
        }
    }

    #[test]
    fn tooltips_name_the_tool_its_key_and_why_it_is_locked() {
        let tool = HudTool {
            def: owned(retail::SCAN, Tab::Main, 0, KeyCode::KeyV),
            usable: false,
            active: false,
            count: None,
            locked: Some("Costs 5 Sporebucks".into()),
            recharge: 0.0,
        };
        let info = ToolInfo { name: "Scanner".into(), description: "Scans a planet.".into(), ..default() };
        assert_eq!(tool_tooltip(&tool, Some(&info)), "Scanner [V]\nScans a planet.\nCosts 5 Sporebucks");
        assert_eq!(tool_tooltip(&tool, None), "scan [V]\nCosts 5 Sporebucks");
    }

    #[test]
    fn the_table_tools_the_ship_owns_join_the_combat_panels_with_their_own_rows() {
        let mut campaign = loaded();
        let ids = |c: &SpaceCampaign| -> Vec<&'static str> { build_tools(c, 0.0).iter().map(|t| t.def.id).collect() };
        for id in ["shield", "cloak", "repairpotion", "antimatterbomb"] {
            assert!(!ids(&campaign).contains(&id), "{id} is not owned");
        }
        campaign.grant_tool("shield");
        campaign.add_charges("repairpotion", 2);
        campaign.add_charges("antimatterbomb", 1);
        let tools = build_tools(&campaign, 0.0);
        let shield = find(&tools, "shield").expect("the shield is shown once owned");
        // The record's own panel: the combat ability row; used by its click.
        assert_eq!((shield.def.tab, shield.def.row, shield.def.self_use), (Tab::Weapons, 0, true));
        let potion = find(&tools, "repairpotion").expect("a carried potion");
        assert_eq!((potion.def.tab, potion.def.row, potion.count), (Tab::Weapons, 1, Some(2)));
        let bomb = find(&tools, "antimatterbomb").expect("a carried bomb");
        assert_eq!((bomb.def.tab, bomb.def.row, bomb.count), (Tab::Weapons, 1, Some(1)));
        assert!(bomb.def.weapon && bomb.def.hold && bomb.def.key == Some(KeyCode::KeyF));
        // A click on the shield asks for its use; a click on the bomb selects what F fires.
        let mut run = RunProgress::default();
        run.space_campaign = campaign.clone();
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut injected = crate::player_hud::InjectedKeys::default();
        select(find(&tools, "shield").unwrap(), &mut run, &mut keys, &mut injected);
        assert_eq!(run.space_campaign.pending_self_tool.as_deref(), Some("shield"));
        select(find(&tools, "antimatterbomb").unwrap(), &mut run, &mut keys, &mut injected);
        assert_eq!(run.space_campaign.weapon_tool.as_deref(), Some("antimatterbomb"));
        assert_eq!(run.space_campaign.selected_weapon().id, "antimatterbomb");
        // Recharging shows the bar and locks the button.
        run.space_campaign.generic_clocks.entry("shield".into()).or_default().commit(180.0);
        let tools = build_tools(&run.space_campaign, 0.0);
        let shield = find(&tools, "shield").unwrap();
        assert!(!shield.usable && shield.recharge > 0.99 && shield.locked.is_some());
    }
}
