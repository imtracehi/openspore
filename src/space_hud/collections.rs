//! My Collections, the badge page (helper `space-visuals`, `notes/space-visuals-notes.md`).
//!
//! Original data / layouts (VERIFIED): the Space "My Collections" button
//! 0x06244208 opens the collections manager (0xE190A0 -> 0xE18DB0), whose
//! layout is picked per game mode by the switch at 0xE19850: Space =
//! 0x40464200!0xD9B09D9D (tabs Missions FFFFFFF2, Badges FFFFFFF4, Rares
//! FFFFFFF3, Tools FFFFFFF5, close FFFFFFF0, header text FFFFFFF6, content
//! FFFFFFFC). Badge cards are `SpaceBadgeItem` 0xC1CB2A9B (button 05EE04C0,
//! five level stars under 05FCA1F0, progress bar 05FCA190); each badge file
//! (`space_badges~` 0x02CB089D!<family>-<level>) names its picture 0x02CB2286
//! and its title 0x043B3DC0; the counters and thresholds are
//! `space_rules::BADGE_FAMILIES`.
//!
//! The other pages (helper `space-final`, `notes\space-final-notes.md` F2,
//! VERIFIED data): each tab is a Sporepedia feed list of group 0x06B38241
//! (`feedListItems` 0x744717C1 -> categories in group 0x4E5892EB, each with
//! `feedListItemName` 0x744717C5 and `feedListItemKeyList` 0x0662081E; key
//! 0x140B116F is a blank separator): Rares `spacerares_category` 0x62C622F3
//! (16 categories of `spacetrading~` 0x034D97FA rares), Tools
//! `SpaceTools_category` 0x62822E59 (9 categories of `spacetools~` 0x30608F0B
//! tools), Missions `missions_inprogress` 0x9E5ECCCF / `missions_completed`
//! 0x897576DA (item type 0xAABE8769, no key list: filled by code). Items show
//! their picture (`spaceToolThumbnailID` 0x0653A6CF of a rare, else
//! `spaceToolImageID` 0x3068D95C) and `spaceToolDescription` 0x3068D95D name.
//!
//! The category column (helper `space-final4`, `notes\space-final4-notes.md`
//! C1, VERIFIED code): the manager 0xE19830 gives the Space window four page
//! objects (Missions 0xE1B2B0, Rares 0x1064A40, Tools 0x107AEA0, Badges
//! 0x1063370); each loads its page layout into the content window (Rares
//! 0x40464200!0xEA479D2A, Tools 0xE53F0420, Badges 0xA3C81581, Missions
//! 0x1CFACA92) and a `cSPUIFeedList` (0x663820, the feed config 0x6EDC12D4
//! `spacerares_list` / `spacetools_list` -> the feed page) whose categories
//! are `AssetBrowserFeedListCategory` rows (0x41464200!0x5A71FB79: name text
//! B3C6EFCA, row button B48E2560) stacked in the page's column (window
//! 0x05F00660, x 2..182 of the content window). Rares (0x10655B0): a
//! category shows only with a rare found, with the count "found/total"; the
//! last category "???????" (0x56624A99, no keys) shows while some category
//! is still empty and, when selected, replaces the cards with "There are more
//! rare sets to be found..." (F5E156F8!06034698); the selection stays while
//! its row shows, else the first row. The Rares cards (0x1064FE0) are every
//! item of the selected category: a found rare with its thumbnail and name,
//! the others with the placeholder 0x011C0BF5!0x6E76B39A and "???????"
//! (2DB6DAD3!03E81C65), disabled. Tools (0x107AFD0): every category, no
//! count, the first selected. The page titles: "Artifacts" F5E156F8!06038566,
//! "Tools" 060383CF, "Badges" 06038551.
//!
//! ADAPTED: the cards are laid out in the content area as a grid of the
//! badge card's authored size (`SpaceBadgeItem` also carries the rares and
//! tools: `SpaceRareItem` 0xB923E28C and the page layouts are not spawned),
//! eight per page right of the category column; a click on the open tab
//! turns the page of the selected category (the retail grid scrolls); the
//! category count follows the name in the row's text (the retail count text
//! is placed by code beside it); rows past the column height are not shown
//! (the retail column scrolls); the "???????" line is shown in the header;
//! the Badges page keeps its family pages (the retail column lists the ten
//! master badges); a badge family not yet earned or a tool not owned is
//! dimmed; the Missions page lists the missions as text in the header. A
//! rare card is lit when the retail rare is found
//! (`space_rares::RareState::found`) or it is a galactic object
//! `rare-galacticobject1..10` found by `space_endgame`; an older save's
//! invented-set pieces still light item n of category i.
use bevy::prelude::*;

use crate::{
    asset_catalog::ResourceIndex,
    dbpf::ResourceKey,
    game::RunProgress,
    spui::{model::ImageRef, view::{SpuiAssets, SpuiState}},
};

use super::SpaceHudRoot;

pub const LAYOUT: &str = "0xD9B09D9D";
pub const ITEM_LAYOUT: &str = "0xC1CB2A9B";
/// `AssetBrowserFeedListCategory`: one row of the category column.
pub const CATEGORY_LAYOUT: &str = "0x5A71FB79";
/// Category rows spawned (the Rares page has 16 categories).
pub const ROWS: usize = 16;
/// The column inside the content window (the page layout's window E2 at
/// x 24..204 of the page root at x 22).
pub const COLUMN_X: [f32; 2] = [2.0, 182.0];
/// Row height: the row button B48E2560 (y 201..229).
pub const ROW_HEIGHT: f32 = 28.0;
/// The Rares category without keys: "???????".
pub const UNKNOWN_CATEGORY: u32 = 0x5662_4A99;
/// The card placeholder of a rare not found (0x1064FE0, DAT_015B90CC).
pub const PLACEHOLDER: ResourceKey =
    ResourceKey { group_id: 0x011C_0BF5, instance_id: 0x6E76_B39A, type_id: crate::ui_art::UI_PNG_TYPE };
/// Cards per page: 4 x 2 fit the content area right of the category column
/// (the header bar's span) at the card's size.
pub const PAGE: usize = 8;

#[allow(dead_code)]
pub mod cid {
    pub const CLOSE: u32 = 0xFFFF_FFF0;
    pub const TAB_MISSIONS: u32 = 0xFFFF_FFF2;
    pub const TAB_RARES: u32 = 0xFFFF_FFF3;
    pub const TAB_BADGES: u32 = 0xFFFF_FFF4;
    pub const TAB_TOOLS: u32 = 0xFFFF_FFF5;
    pub const HEADER: u32 = 0xFFFF_FFF6;
    pub const CONTENT: u32 = 0xFFFF_FFFC;
    /// `SpaceBadgeItem`.
    pub const STARS: u32 = 0x05FC_A1F0;
    pub const PROGRESS: u32 = 0x05FC_A190;
    /// `AssetBrowserFeedListCategory` (0x665D00 binds these).
    pub const ROW_ROOT: u32 = 0xF3C6_DC19;
    pub const ROW_NAME: u32 = 0xB3C6_EFCA;
    pub const ROW_ITEMS: u32 = 0xF3C8_39E5;
    pub const ROW_EXPAND: u32 = 0xB3C6_EFC2;
    pub const ROW_BUTTON: u32 = 0xB48E_2560;
}

/// Locale strings of the pages (table, id, English).
pub mod text {
    pub const ARTIFACTS: (u32, u32, &str) = (0xF5E1_56F8, 0x0603_8566, "Artifacts");
    pub const TOOLS: (u32, u32, &str) = (0xF5E1_56F8, 0x0603_83CF, "Tools");
    pub const MORE_RARES: (u32, u32, &str) =
        (0xF5E1_56F8, 0x0603_4698, "There are more rare sets to be found. Explore the universe and collect them all!");
    pub const UNKNOWN: (u32, u32, &str) = (0x2DB6_DAD3, 0x03E8_1C65, "???????");
}

const BADGE_GROUP: u32 = 0x02CB_089D;
const PROP_PICTURE: u32 = 0x02CB_2286;

/// The four tabs of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    Missions,
    #[default]
    Badges,
    Rares,
    Tools,
}

impl Tab {
    fn of(control: u32) -> Option<Tab> {
        Some(match control {
            cid::TAB_MISSIONS => Tab::Missions,
            cid::TAB_BADGES => Tab::Badges,
            cid::TAB_RARES => Tab::Rares,
            cid::TAB_TOOLS => Tab::Tools,
            _ => return None,
        })
    }
}

#[derive(Resource, Default)]
pub struct CollectionsState {
    pub open: bool,
    pub page: usize,
    pub tab: Tab,
    /// The selected category of the Rares / Tools column (+0xD4 / +0x148 of
    /// the page objects), by index in the feed page.
    pub rares_category: Option<usize>,
    pub tools_category: Option<usize>,
    /// The selected row of the Badges column ("All Badges" first).
    pub badges_category: Option<usize>,
    /// The category each shown row stands for (written by `bind`).
    pub rows: Vec<usize>,
}

impl CollectionsState {
    pub fn click(&mut self, control: u32) {
        match control {
            cid::CLOSE => self.open = false,
            c => {
                if let Some(tab) = Tab::of(c) {
                    if tab == self.tab {
                        self.page += 1;
                    } else {
                        self.tab = tab;
                        self.page = 0;
                        self.rows.clear();
                    }
                }
            }
        }
    }

    /// A click on category row `row` selects its category.
    pub fn select_row(&mut self, row: usize) {
        let Some(&category) = self.rows.get(row) else {
            return;
        };
        match self.tab {
            Tab::Rares => self.rares_category = Some(category),
            Tab::Tools => self.tools_category = Some(category),
            Tab::Badges => self.badges_category = Some(category),
            _ => return,
        }
        self.page = 0;
    }
}

const FEED_ITEMS_GROUP: u32 = 0x4E58_92EB;
const FEED_GROUP: u32 = 0x06B3_8241;
const PROP_FEED_ITEMS: u32 = 0x7447_17C1;
const PROP_ITEM_KEYS: u32 = 0x0662_081E;
const PROP_ITEM_NAME: u32 = 0x7447_17C5;
const SEPARATOR: u32 = 0x140B_116F;
pub const RARES_PAGE: u32 = 0x62C6_22F3;
/// `spacebadges_category`: "All Badges" and the ten master badges.
pub const BADGES_PAGE: u32 = 0x70CF_E28C;
pub const TOOLS_PAGE: u32 = 0x6282_2E59;
const RARE_GROUP: u32 = 0x034D_97FA;
const TOOL_GROUP: u32 = 0x3060_8F0B;
const PROP_NAME: u32 = 0x3068_D95D;
const PROP_ICON: u32 = 0x3068_D95C;
const PROP_THUMBNAIL: u32 = 0x0653_A6CF;

/// One category of a feed-list page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Category {
    /// The category's instance in group 0x4E5892EB.
    pub id: u32,
    pub name: String,
    /// Item instances (separators dropped).
    pub items: Vec<u32>,
}

/// One row of the category column.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnRow {
    /// Index of the category in the feed page.
    pub category: usize,
    /// (found, total) of the Rares counter.
    pub count: Option<(usize, usize)>,
}

/// 0x10655B0: the Rares column. A category with a rare found shows with its
/// count; "???????" (a category without keys) shows while a category before
/// it has none found.
pub fn rares_column(categories: &[Category], found: &dyn Fn(usize, usize, u32) -> bool) -> Vec<ColumnRow> {
    let mut rows = Vec::new();
    let mut empty_before = false;
    for (c, cat) in categories.iter().enumerate() {
        if cat.items.is_empty() {
            if empty_before {
                rows.push(ColumnRow { category: c, count: None });
            }
            continue;
        }
        let n = cat.items.iter().enumerate().filter(|&(i, &item)| found(c, i, item)).count();
        if n >= 1 {
            rows.push(ColumnRow { category: c, count: Some((n, cat.items.len())) });
        } else {
            empty_before = true;
        }
    }
    rows
}

/// 0x107AFD0: the Tools column, every category.
pub fn tools_column(categories: &[Category]) -> Vec<ColumnRow> {
    categories
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.items.is_empty())
        .map(|(category, _)| ColumnRow { category, count: None })
        .collect()
}

/// The ten master badge categories of the Badges page (DAT_0149C000).
pub const MASTER_BADGE_CATEGORIES: [u32; 10] =
    [0x4468_C6BF, 0x4468_C6BC, 0x4468_C6BD, 0x4468_C6BA, 0x4468_C6BB, 0x4468_C6B8, 0x4468_C6B9, 0x4468_C6B6, 0x4468_C6B7, 0x4568_C811];

/// The master level (1..10) of category `c`, if it is one.
pub fn master_level(categories: &[Category], c: usize) -> Option<u8> {
    let id = categories.get(c)?.id;
    MASTER_BADGE_CATEGORIES.iter().position(|m| *m == id).map(|i| i as u8 + 1)
}

/// 0x10635E0: the Badges column, every category; a master badge shows its
/// progress (0xFE6690: points and the points it needs; ADAPTED "n/m"
/// text) and the masters after the first one not earned are locked
/// (FUN_00667660). Rows: (category, label, locked).
pub fn badges_column(categories: &[Category], points: u32, rank: u8) -> Vec<(usize, String, bool)> {
    let mut first_open = true;
    categories
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.items.is_empty())
        .map(|(i, cat)| match master_level(categories, i) {
            Some(n) => {
                let need = crate::space_rules::RANK_BADGE_POINTS[n as usize - 1];
                let earned = rank >= n;
                let locked = !earned && !first_open;
                if !earned {
                    first_open = false;
                }
                (i, format!("{}  {}/{}", cat.name, points.min(need), need), locked)
            }
            None => (i, cat.name.clone(), false),
        })
        .collect()
}

/// The selected category: kept while its row shows, else the first row.
pub fn selected(rows: &[ColumnRow], current: Option<usize>) -> Option<usize> {
    match current {
        Some(c) if rows.iter().any(|r| r.category == c) => Some(c),
        _ => rows.first().map(|r| r.category),
    }
}

fn text(props: &crate::prop::PropertyList, id: u32, index: &ResourceIndex, assets: &mut SpuiAssets) -> String {
    match props.0.get(&id).and_then(|p| p.text()) {
        Some((table, text_id, literal)) if table != 0 => assets.localized(index, table, text_id).unwrap_or(literal),
        Some((_, _, literal)) => literal,
        None => String::new(),
    }
}

fn key_instances(p: &crate::prop::Property) -> Vec<u32> {
    p.values.iter().filter_map(|v| Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?))).collect()
}

/// The categories of the feed-list page `page` (group 0x06B38241).
pub fn read_page(index: &ResourceIndex, assets: &mut SpuiAssets, page: u32) -> Vec<Category> {
    let key = |group_id, instance_id| ResourceKey { group_id, instance_id, type_id: crate::asset_catalog::PROP };
    let Ok(props) = index.properties(key(FEED_GROUP, page)) else { return Vec::new() };
    let children = props.0.get(&PROP_FEED_ITEMS).map(key_instances).unwrap_or_default();
    children
        .into_iter()
        .filter_map(|child| {
            let p = index.properties(key(FEED_ITEMS_GROUP, child)).ok()?;
            let items: Vec<u32> = p
                .0
                .get(&PROP_ITEM_KEYS)
                .map(key_instances)
                .unwrap_or_default()
                .into_iter()
                .filter(|i| *i != SEPARATOR)
                .collect();
            // Categories without keys stay ("???????" of the Rares page).
            Some(Category { id: child, name: text(&p, PROP_ITEM_NAME, index, assets), items })
        })
        .collect()
}

/// (category, first item) of each page: one category per page, `PAGE` items
/// at most (the earlier paging, kept for its test).
#[cfg(test)]
pub fn pages(categories: &[Category]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (c, cat) in categories.iter().enumerate() {
        for start in (0..cat.items.len()).step_by(PAGE) {
            out.push((c, start));
        }
    }
    out
}

fn locale(index: &ResourceIndex, assets: &mut SpuiAssets, (table, id, english): (u32, u32, &str)) -> String {
    assets.localized(index, table, id).unwrap_or_else(|| english.to_string())
}

/// What a rare / tool card shows.
#[derive(Clone, Debug, Default)]
pub struct ItemInfo {
    pub name: String,
    pub icon: Option<ResourceKey>,
}

fn read_item(index: &ResourceIndex, assets: &mut SpuiAssets, group: u32, instance: u32) -> ItemInfo {
    let key = ResourceKey { group_id: group, instance_id: instance, type_id: crate::asset_catalog::PROP };
    let Ok(props) = index.properties(key) else { return ItemInfo::default() };
    // Rares carry a card picture (`spaceToolThumbnailID`, group 0x0542E15D
    // like the badge pictures); the image id is their small inventory icon.
    let picture = props.0.get(&PROP_THUMBNAIL).or_else(|| props.0.get(&PROP_ICON));
    let icon = picture.and_then(|p| p.key()).map(|mut k| {
        if k.type_id == 0 {
            k.type_id = crate::ui_art::UI_PNG_TYPE;
        }
        k
    });
    ItemInfo { name: text(&props, PROP_NAME, index, assets), icon }
}

/// Feed pages and item details read from the install (once).
#[derive(Default)]
pub struct FeedCache {
    rares: Option<Vec<Category>>,
    tools: Option<Vec<Category>>,
    badges: Option<Vec<Category>>,
    items: std::collections::HashMap<(u32, u32), ItemInfo>,
}

/// Whether the player has rare `item` (category `category` of the Rares page).
pub fn rare_found(campaign: &crate::space::SpaceCampaign, category: usize, position: usize, item: u32) -> bool {
    let galactic = (1..=10u8).find(|n| crate::world_objects::name_id(&format!("rare-galacticobject{n}")) == item);
    if let Some(n) = galactic {
        return campaign.endgame.galactic_objects.contains(&n);
    }
    // The retail rares found (`space_rares`, mRareMap).
    if campaign.rares.is_found(item) {
        return true;
    }
    // Older saves: piece n of invented set i stood for item n of category i.
    let Some(set) = crate::space::ArtifactSet::ALL.get(category) else { return false };
    let piece = position as u8 + 1;
    campaign.vault_collections.get(set).is_some_and(|p| p.contains(&piece))
        || campaign.artifacts.iter().any(|a| a.set == Some(*set) && a.piece == Some(piece))
}

/// Whether the player owns tool `item` (an owned tool or charges of it).
pub fn tool_owned(campaign: &crate::space::SpaceCampaign, item: u32) -> bool {
    let id = crate::world_objects::name_id;
    campaign.tools.iter().any(|t| id(t) == item) || campaign.tool_charges.iter().any(|(t, n)| *n > 0 && id(t) == item)
}

/// The Missions page as text: in progress, then completed.
pub fn missions_text(campaign: &crate::space::SpaceCampaign) -> String {
    let mut lines = vec!["Missions in progress:".to_string()];
    if campaign.tutorial.step != crate::space_rules::TutorialStep::Done {
        lines.push(format!("  {}", campaign.tutorial.step.title()));
    }
    if let Some(m) = &campaign.active_alien_mission {
        lines.push(format!("  {}", m.kind.title()));
    }
    if lines.len() == 1 {
        lines.push("  none".into());
    }
    lines.push(format!(
        "Missions completed: {}",
        campaign.badge_counter(crate::space_rules::BadgeCounter::MissionsComplete)
    ));
    lines.join("\n")
}

/// Progress toward the next level of a family (1 at the top level).
pub fn progress(thresholds: &[u64; 5], level: u8, count: u64) -> f32 {
    if level as usize >= thresholds.len() {
        return 1.0;
    }
    let lo = if level == 0 { 0 } else { thresholds[level as usize - 1] };
    let hi = thresholds[level as usize];
    if hi <= lo {
        return 1.0;
    }
    (count.saturating_sub(lo) as f32 / (hi - lo) as f32).clamp(0.0, 1.0)
}

/// Card cells of a page in a content rectangle `[x1, y1, x2, y2]` for cards of
/// `size`: as many columns and rows as fit, spread evenly.
pub fn grid(content: [f32; 4], size: [f32; 2], count: usize) -> Vec<[f32; 2]> {
    let (w, h) = (content[2] - content[0], content[3] - content[1]);
    let cols = ((w / size[0]).floor() as usize).max(1);
    let rows = ((h / size[1]).floor() as usize).max(1);
    let gx = (w - cols as f32 * size[0]) / (cols as f32 + 1.0);
    let gy = (h - rows as f32 * size[1]) / (rows as f32 + 1.0);
    (0..count.min(cols * rows))
        .map(|i| {
            let (c, r) = (i % cols, i / cols);
            [content[0] + gx + c as f32 * (size[0] + gx), content[1] + gy + r as f32 * (size[1] + gy)]
        })
        .collect()
}

/// The badge picture of a family level (badge file property 0x02CB2286).
#[derive(Resource, Default)]
pub struct BadgePictures(std::collections::HashMap<(String, u8), Option<ResourceKey>>);

fn picture(index: &ResourceIndex, cache: &mut BadgePictures, family: &str, level: u8) -> Option<ResourceKey> {
    cache
        .0
        .entry((family.to_string(), level))
        .or_insert_with(|| {
            let key = ResourceKey {
                group_id: BADGE_GROUP,
                instance_id: crate::world_objects::name_id(&format!("{family}-{level}")),
                type_id: crate::asset_catalog::PROP,
            };
            let props = crate::prop::parse(&index.read(key).ok()?).ok()?;
            props.0.get(&PROP_PICTURE)?.key()
        })
        .clone()
}

/// The em size of a category row name by its length (`None` = the style's
/// own size): names with their counts up to 18 characters fit at the style
/// size, the rest shrink to stay on one line.
fn row_text_size(chars: usize) -> Option<f32> {
    match chars {
        0..=18 => None,
        19..=24 => Some(10.0),
        _ => Some(8.5),
    }
}

/// What one card slot shows.
struct Card {
    icon: Option<ResourceKey>,
    lit: bool,
    tooltip: String,
    /// Badge level and progress toward the next one.
    badge: Option<(u8, f32)>,
}

/// Show / fill the collections window and its cards for the open tab.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut state: ResMut<CollectionsState>,
    run: Res<RunProgress>,
    originals: Option<Res<crate::Originals>>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut pictures: Local<BadgePictures>,
    mut feed: Local<FeedCache>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
) {
    let Some(originals) = originals else {
        return;
    };
    let index = &originals.resource_index;
    // `OPENSPORE_SPACE_COLLECTIONS=1` (development captures): shown from the
    // start; `=rares` / `=tools` / `=missions` opens that tab.
    let env = std::env::var("OPENSPORE_SPACE_COLLECTIONS").unwrap_or_default();
    let open = state.open || !env.is_empty();
    let tab = match env.as_str() {
        "rares" if !state.open => Tab::Rares,
        "tools" if !state.open => Tab::Tools,
        "missions" if !state.open => Tab::Missions,
        _ => state.tab,
    };
    let campaign = &run.space_campaign;
    let mut cards: Vec<Card> = Vec::new();
    // The column rows: label and selected.
    let mut rows: Vec<(String, bool, bool)> = Vec::new();
    let mut return_header: Option<String> = None;
    let header = if !open {
        String::new()
    } else {
        match tab {
            Tab::Badges => {
                // The Badges column (0x10635E0): "All Badges" and the ten
                // master badges with their progress, the masters past the
                // next one locked.
                let categories = feed.badges.get_or_insert_with(|| read_page(index, &mut assets, BADGES_PAGE)).clone();
                let points = campaign.total_badge_points();
                let rank = campaign.captain_rank();
                let column = badges_column(&categories, points, rank);
                let chosen = selected(
                    &column.iter().map(|r| ColumnRow { category: r.0, count: None }).collect::<Vec<_>>(),
                    state.badges_category,
                );
                state.badges_category = chosen;
                state.rows = column.iter().map(|r| r.0).collect();
                for (c, label, locked) in &column {
                    rows.push((label.clone(), Some(*c) == chosen, *locked));
                }
                if let Some(n) = chosen.and_then(|c| master_level(&categories, c)) {
                    // A master badge: its card (badgecollector-n).
                    cards.push(Card {
                        icon: picture(index, &mut pictures, "badgecollector", n),
                        lit: rank >= n,
                        tooltip: format!(
                            "{} ({}/{})",
                            crate::space_rules::rank_title(n),
                            points.min(crate::space_rules::RANK_BADGE_POINTS[n as usize - 1]),
                            crate::space_rules::RANK_BADGE_POINTS[n as usize - 1]
                        ),
                        badge: None,
                    });
                    return_header = Some(categories[chosen.unwrap_or(0)].name.clone());
                }
                let families = &crate::space_rules::BADGE_FAMILIES;
                let pages = families.len().div_ceil(PAGE).max(1);
                let page = state.page % pages;
                let all = cards.is_empty();
                for family in families.iter().skip(page * PAGE).take(PAGE).filter(|_| all) {
                    let level = campaign.badge_family_level(family);
                    let count = campaign.badge_counter(family.counter);
                    cards.push(Card {
                        icon: picture(index, &mut pictures, family.id, level.max(1)),
                        // Not earned yet: dimmed (ADAPTED).
                        lit: level > 0,
                        tooltip: if level == 0 {
                            format!("{} ({}/{})", family.name, count, family.thresholds[0])
                        } else {
                            format!("{} {} ({})", family.name, level, count)
                        },
                        badge: Some((level, progress(&family.thresholds, level, count))),
                    });
                }
                return_header.take().unwrap_or_else(|| format!("Badges ({}/{})", page + 1, pages))
            }
            Tab::Rares | Tab::Tools => {
                let (slot, page_id, group) = match tab {
                    Tab::Rares => (&mut feed.rares, RARES_PAGE, RARE_GROUP),
                    _ => (&mut feed.tools, TOOLS_PAGE, TOOL_GROUP),
                };
                let categories = slot.get_or_insert_with(|| read_page(index, &mut assets, page_id)).clone();
                // The column (0x10655B0 / 0x107AFD0) and its selection.
                let column = match tab {
                    Tab::Rares => rares_column(&categories, &|c, i, item| rare_found(campaign, c, i, item)),
                    _ => tools_column(&categories),
                };
                let current = if tab == Tab::Rares { state.rares_category } else { state.tools_category };
                let chosen = selected(&column, current);
                if tab == Tab::Rares {
                    state.rares_category = chosen;
                } else {
                    state.tools_category = chosen;
                }
                state.rows = column.iter().map(|r| r.category).collect();
                for row in &column {
                    let cat = &categories[row.category];
                    let name = if cat.items.is_empty() { locale(index, &mut assets, text::UNKNOWN) } else { cat.name.clone() };
                    let label = match row.count {
                        Some((n, total)) => format!("{name}  {n}/{total}"),
                        None => name,
                    };
                    rows.push((label, Some(row.category) == chosen, false));
                }
                let unknown = chosen.is_some_and(|c| categories[c].id == UNKNOWN_CATEGORY || categories[c].items.is_empty());
                if let (Some(c), false) = (chosen, unknown) {
                    let cat = &categories[c];
                    let pages = cat.items.len().div_ceil(PAGE).max(1);
                    let start = (state.page % pages) * PAGE;
                    for (i, &item) in cat.items.iter().enumerate().skip(start).take(PAGE) {
                        let info = feed
                            .items
                            .entry((group, item))
                            .or_insert_with(|| read_item(index, &mut assets, group, item))
                            .clone();
                        let card = match tab {
                            // 0x1064FE0: a rare not found shows the
                            // placeholder and "???????".
                            Tab::Rares if !rare_found(campaign, c, i, item) => Card {
                                icon: Some(PLACEHOLDER),
                                lit: false,
                                tooltip: locale(index, &mut assets, text::UNKNOWN),
                                badge: None,
                            },
                            Tab::Rares => Card { icon: info.icon, lit: true, tooltip: info.name, badge: None },
                            _ => Card { icon: info.icon, lit: tool_owned(campaign, item), tooltip: info.name, badge: None },
                        };
                        cards.push(card);
                    }
                }
                match (tab, unknown) {
                    (Tab::Rares, true) => locale(index, &mut assets, text::MORE_RARES),
                    (Tab::Rares, false) => locale(index, &mut assets, text::ARTIFACTS),
                    _ => locale(index, &mut assets, text::TOOLS),
                }
            }
            Tab::Missions => missions_text(campaign),
        }
    };
    if !matches!(tab, Tab::Rares | Tab::Tools | Tab::Badges) || !open {
        state.rows.clear();
    }
    let mut content = None;
    let mut column: Option<[f32; 4]> = None;
    for (mut ui, kind) in &mut roots {
        if *kind != SpaceHudRoot::Collections {
            continue;
        }
        if let Some(&root) = ui.layout.roots.first() {
            ui.set_visible(root, open);
        }
        if let Some(w) = ui.find(cid::HEADER) {
            if ui.captions[w].as_deref() != Some(header.as_str()) {
                ui.set_text(w, header.clone());
            }
        }
        if open && ui.ready {
            let header = ui.find(cid::HEADER).map(|h| ui.rect(h).x1);
            content = ui.find(cid::CONTENT).map(|w| {
                let r = ui.rect(w);
                column = Some([r.x1 + COLUMN_X[0], r.y1, r.x1 + COLUMN_X[1], r.y2]);
                // The cards stay right of the left category column.
                [header.map_or(r.x1, |x| x.max(r.x1)), r.y1, r.x2, r.y2]
            });
        }
    }
    // The category rows (`AssetBrowserFeedListCategory`), stacked in the
    // column from its top.
    for (mut ui, kind) in &mut roots {
        let SpaceHudRoot::Category(slot) = *kind else {
            continue;
        };
        let Some(&root) = ui.layout.roots.first() else {
            continue;
        };
        let row = rows.get(slot as usize);
        let place = column.and_then(|c| {
            let y = c[1] + slot as f32 * ROW_HEIGHT;
            (y + ROW_HEIGHT <= c[3] + 0.5).then_some([c[0], y, c[2], y + ROW_HEIGHT])
        });
        let (Some((label, chosen, locked)), Some(at), true) = (row, place, ui.ready) else {
            ui.set_visible(root, false);
            continue;
        };
        ui.set_visible(root, true);
        let width = {
            let a = ui.layout.window(root).area;
            (a[2] - a[0]).min(at[2] - at[0])
        };
        ui.set_area(root, [at[0], at[1], at[0] + width, at[3]]);
        // The row has no items list or expand arrow here (the cards show the
        // items).
        for hide in [cid::ROW_ITEMS, cid::ROW_EXPAND] {
            if let Some(w) = ui.find(hide) {
                ui.set_visible(w, false);
            }
        }
        if let Some(w) = ui.find(cid::ROW_NAME) {
            if ui.captions[w].as_deref() != Some(label.as_str()) {
                ui.set_text(w, label.clone());
            }
            // ADAPTED: the name takes the hidden arrow's room and a long
            // name (with its count) shrinks to stay on one line (the retail
            // rows of My Collections were not seen). Written every frame, not
            // only when the caption changes: the window keeps its size
            // whatever happened to the caption in between (an earlier build
            // of the Badges column drew the "Title of Admiral" row at the
            // large default size and wrapped it onto two lines).
            let a = ui.layout.window(w).area;
            ui.set_area(w, [a[0] - 16.0, a[1], a[2] + 8.0, a[3]]);
            ui.set_text_size(w, row_text_size(label.chars().count()));
        }
        if let Some(w) = ui.find(cid::ROW_BUTTON) {
            ui.set_selected(w, *chosen);
            // A locked master badge (FUN_00667660): disabled and dimmed.
            ui.set_enabled(w, !*locked);
            ui.set_shade(w, if *locked { 0xFF80_8080 } else { 0xFFFF_FFFF });
        }
    }
    for (mut ui, kind) in &mut roots {
        let SpaceHudRoot::Badge(slot) = *kind else {
            continue;
        };
        let Some(&button) = ui.layout.roots.first() else {
            continue;
        };
        let card = cards.get(slot as usize);
        let (Some(content), Some(card), true) = (content, card, ui.ready) else {
            ui.set_visible(button, false);
            continue;
        };
        let area = ui.layout.window(button).area;
        let size = [area[2] - area[0], area[3] - area[1]];
        let cells = grid(content, size, PAGE);
        let Some(at) = cells.get(slot as usize).copied() else {
            ui.set_visible(button, false);
            continue;
        };
        ui.set_visible(button, true);
        ui.set_area(button, [at[0], at[1], at[0] + size[0], at[1] + size[1]]);
        let icon = card.icon.and_then(|key| {
            assets.texture(index, &mut images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
        });
        ui.set_icon(button, icon);
        ui.set_shade(button, if card.lit { 0xFFFF_FFFF } else { 0xFF60_6060 });
        ui.set_tooltip(button, Some(card.tooltip.clone()));
        let (level, fill) = card.badge.unwrap_or((0, 0.0));
        if let Some(stars) = ui.find(cid::STARS) {
            let children = ui.layout.window(stars).children.clone();
            for (i, star) in children.into_iter().enumerate() {
                ui.set_visible(star, i < level as usize);
            }
        }
        if let Some(bar) = ui.find_all(cid::PROGRESS).first().copied() {
            ui.set_visible(bar, card.badge.is_some());
            let full = ui.layout.window(bar).area;
            ui.set_area(bar, [full[0], full[1], full[0] + (full[2] - full[0]) * fill, full[3]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Badges row label ("Title of The Legendary  0/140") shrinks to
    /// one line: only names up to 18 characters keep the style size.
    #[test]
    fn row_names_shrink_by_length() {
        assert_eq!(row_text_size("All Badges".chars().count()), None);
        // "Title of Admiral  0/30" (22 characters), the row that once drew large.
        assert_eq!(row_text_size("Title of Admiral  0/30".chars().count()), Some(10.0));
        assert_eq!(row_text_size("Title of Ultimate Being  0/225".chars().count()), Some(8.5));
        let sizes: Vec<f32> = (0..40).map(|n| row_text_size(n).unwrap_or(100.0)).collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "sizes only shrink with length");
    }

    #[test]
    fn progress_runs_between_the_thresholds() {
        let t = [2, 5, 10, 20, 35];
        assert_eq!(progress(&t, 0, 1), 0.5);
        assert_eq!(progress(&t, 1, 2), 0.0);
        assert!((progress(&t, 1, 4) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(progress(&t, 5, 99), 1.0);
    }

    #[test]
    fn tabs_switch_pages_and_a_second_click_turns_the_page() {
        let mut s = CollectionsState::default();
        assert_eq!(s.tab, Tab::Badges);
        s.click(cid::TAB_RARES);
        assert_eq!((s.tab, s.page), (Tab::Rares, 0));
        s.click(cid::TAB_RARES);
        assert_eq!((s.tab, s.page), (Tab::Rares, 1));
        s.click(cid::TAB_TOOLS);
        assert_eq!((s.tab, s.page), (Tab::Tools, 0));
        // One category per page, `PAGE` items at most.
        let cats = [
            Category { name: "a".into(), items: (0..12).collect(), ..Default::default() },
            Category { name: "b".into(), items: vec![7], ..Default::default() },
        ];
        assert_eq!(pages(&cats), [(0, 0), (0, PAGE), (1, 0)]);
    }

    #[test]
    fn the_badges_column_locks_the_masters_after_the_next_one() {
        let mut cats = vec![Category { id: 0x6214_6E6D, name: "All Badges".into(), items: vec![1, 2, 3] }];
        for (i, id) in MASTER_BADGE_CATEGORIES.iter().enumerate() {
            cats.push(Category { id: *id, name: format!("Title {}", i + 1), items: vec![100 + i as u32] });
        }
        // Rank 2 (20 points): masters 1-2 earned, 3 open, 4.. locked.
        let rows = badges_column(&cats, 20, 2);
        assert_eq!(rows.len(), 11);
        assert_eq!(rows[0], (0, "All Badges".to_string(), false));
        assert_eq!(rows[1].1, "Title 1  5/5");
        assert!(!rows[2].2 && !rows[3].2 && rows[4].2 && rows[10].2);
        assert_eq!(rows[3].1, "Title 3  20/30");
        assert_eq!(master_level(&cats, 3), Some(3));
        assert_eq!(master_level(&cats, 0), None);
    }

    #[test]
    fn the_rares_column_shows_found_sets_and_the_unknown_entry() {
        let cat = |id, items: Vec<u32>| Category { id, name: format!("{id}"), items };
        let cats = vec![cat(1, (10..20).collect()), cat(2, (20..30).collect()), cat(3, (30..40).collect()), cat(UNKNOWN_CATEGORY, vec![])];
        // Nothing found: only "???????" (an earlier category is empty).
        let none = rares_column(&cats, &|_, _, _| false);
        assert_eq!(none, [ColumnRow { category: 3, count: None }]);
        // Two of set 2 found: its row with "2/10", and "???????" still.
        let some = rares_column(&cats, &|_, _, item| item == 21 || item == 25);
        assert_eq!(some, [ColumnRow { category: 1, count: Some((2, 10)) }, ColumnRow { category: 3, count: None }]);
        // Every set started: no "???????".
        let all = rares_column(&cats, &|_, i, _| i == 0);
        assert_eq!(all.iter().map(|r| r.category).collect::<Vec<_>>(), [0, 1, 2]);
        // The selection stays while its row shows, else the first row.
        assert_eq!(selected(&some, Some(1)), Some(1));
        assert_eq!(selected(&some, Some(0)), Some(1));
        assert_eq!(selected(&none, None), Some(3));
        // Tools: every category with items.
        assert_eq!(tools_column(&cats).len(), 3);
        // A row click selects its category on the open tab.
        let mut s = CollectionsState::default();
        s.click(cid::TAB_RARES);
        s.rows = vec![1, 3];
        s.page = 2;
        s.select_row(1);
        assert_eq!((s.rares_category, s.page), (Some(3), 0));
        s.select_row(7);
        assert_eq!(s.rares_category, Some(3));
    }

    #[test]
    fn rares_and_tools_are_found_from_the_campaign() {
        let mut c = crate::space::SpaceCampaign::default();
        let n3 = crate::world_objects::name_id("rare-galacticobject3");
        assert!(!rare_found(&c, 15, 2, n3));
        c.endgame.galactic_objects.push(3);
        assert!(rare_found(&c, 15, 2, n3));
        let set = crate::space::ArtifactSet::ALL[1];
        c.vault_collections.entry(set).or_default().insert(4);
        assert!(rare_found(&c, 1, 3, 0x1234) && !rare_found(&c, 1, 2, 0x1234));
        // A retail rare found lights its card wherever it is listed.
        let jewel = crate::world_objects::name_id("rare-jewel3");
        assert!(!rare_found(&c, 0, 2, jewel));
        c.rares.obtain(crate::space_rares::rare("rare-jewel3").unwrap());
        assert!(rare_found(&c, 0, 2, jewel));
        c.tools.push("seti".into());
        assert!(tool_owned(&c, crate::world_objects::name_id("seti")));
        assert!(missions_text(&c).starts_with("Missions in progress:"));
    }

    #[test]
    fn a_page_of_cards_fits_right_of_the_category_column() {
        // The Space collections content area right of the category column
        // (the header bar starts at x 482) and the SpaceBadgeItem card.
        let cells = grid([482.0, 219.0, 986.0, 560.0], [106.0, 119.0], PAGE);
        assert_eq!(cells.len(), PAGE);
        assert!(cells.iter().all(|c| c[0] >= 482.0 && c[0] + 106.0 <= 986.0 && c[1] + 119.0 <= 560.0));
    }
}
