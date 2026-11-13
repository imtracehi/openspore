//! Sporepedia feeds and filters as the installed property files define them
//! (read once when Sporepedia opens).
//!
//! * A **feed** is a property file of group `4E5892EB` (`feed_all`,
//!   `feed_MyCreations`, `feed_ShippingContent` = "Maxis"): its name (text
//!   `744717C5`, locale table 21671745), its icon (`F4906970`, a 25 x 25 PNG
//!   of group `CA14DE92`) and the root filter it shows (`05E90865` /
//!   `05E9086C`: `UserCreated` for My Creations, `ShippingContent` for Maxis,
//!   none for Everything).
//! * A **filter** is a property file of group `CC489C6F`: its name (`7435A2D0`,
//!   text) and its child filters (`7435A2D3`, ordered). `ShippingContent` and
//!   `UserCreated` ("Home") have the types as children (cells, creatures,
//!   buildings, vehicles, flora); `Vehicles` has land / sea / air / `ufo`.
//!   The filter's id is also the control id of its icon button in the
//!   `AssetBrowserFeedFilter` layout, which the filter strip above the grid
//!   shows after the feed's icon (the strip's breadcrumb: feed icon, then one
//!   icon per filter from the root down; `0x65F7D0`..`0x65F920` handle its four
//!   slots).

use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey, prop};

/// Property file type.
const PROP_TYPE: u32 = 0x00B1_B104;
pub const FEED_GROUP: u32 = 0x4E58_92EB;
pub const FILTER_GROUP: u32 = 0xCC48_9C6F;
const FEED_NAME: u32 = 0x7447_17C5;
const FEED_ICON: u32 = 0xF490_6970;
const FEED_FILTER: u32 = 0x05E9_0865;
const FILTER_NAME: u32 = 0x7435_A2D0;
const FILTER_CHILDREN: u32 = 0x7435_A2D3;

/// `feed_all`, `feed_MyCreations`, `feed_ShippingContent` ("Maxis"), in the
/// order of the grid's feed filters (Everything, My Creations, Maxis).
pub const FEEDS: [u32; 3] = [0xE241_5C7D, 0x146C_1D8C, 0x8F71_F965];

/// The type filters (`Creatures`, `Buildings`, `Vehicles`, `ufo`, `flora`,
/// `cell`).
pub const CREATURES: u32 = 0xD49D_E19D;
pub const BUILDINGS: u32 = 0x484F_F0B8;
pub const VEHICLES: u32 = 0x39E6_4A16;
pub const UFO: u32 = 0x476A_98C7;
pub const FLORA: u32 = 0x5BDC_EC35;
pub const CELL: u32 = 0xDFAD_9F51;

/// The filters a category stands for, root-most first (after the feed's
/// icon): the types are children of the feeds' root filters; spaceships are
/// the `ufo` child of Vehicles.
pub fn chain(category: usize) -> &'static [u32] {
    match category {
        0 => &[CREATURES],
        1 => &[BUILDINGS],
        2 => &[VEHICLES],
        3 => &[VEHICLES, UFO],
        4 => &[FLORA],
        _ => &[CELL],
    }
}

/// The type (index into `pedia::CATEGORIES`) a type filter shows: the inverse
/// of [`chain`]'s last filter (the strip's icon buttons carry the filter id as
/// their control id).
pub fn category_of(filter: u32) -> Option<usize> {
    [CREATURES, BUILDINGS, VEHICLES, UFO, FLORA, CELL].iter().position(|f| *f == filter)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub instance: u32,
    /// Locale (table, id) of the feed's name.
    pub name: Option<(u32, u32)>,
    /// The feed's 25 x 25 icon PNG.
    pub icon: Option<ResourceKey>,
    /// Instance of the root filter (`UserCreated`, `ShippingContent`).
    pub filter: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub id: u32,
    pub name: Option<(u32, u32)>,
    pub children: Vec<u32>,
}

fn read(index: &ResourceIndex, group: u32, instance: u32) -> Option<prop::PropertyList> {
    prop::parse(&index.read(ResourceKey { group_id: group, instance_id: instance, type_id: PROP_TYPE }).ok()?).ok()
}

/// Every key of a key property (scalar or array).
fn keys(p: &prop::Property) -> Vec<ResourceKey> {
    if p.kind != 0x20 {
        return Vec::new();
    }
    let le = |v: &[u8], at: usize| v.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    p.values
        .iter()
        .filter_map(|v| Some(ResourceKey { instance_id: le(v, 0)?, type_id: le(v, 4)?, group_id: le(v, 8)? }))
        .collect()
}

pub fn read_feed(index: &ResourceIndex, instance: u32) -> Option<Feed> {
    let list = read(index, FEED_GROUP, instance)?;
    Some(Feed {
        instance,
        name: list.0.get(&FEED_NAME).and_then(|p| p.text()).map(|(table, id, _)| (table, id)),
        icon: list.0.get(&FEED_ICON).and_then(|p| keys(p).first().copied()),
        filter: list.0.get(&FEED_FILTER).and_then(|p| keys(p).first().map(|k| k.instance_id)),
    })
}

pub fn read_filter(index: &ResourceIndex, id: u32) -> Option<Filter> {
    let list = read(index, FILTER_GROUP, id)?;
    Some(Filter {
        id,
        name: list.0.get(&FILTER_NAME).and_then(|p| p.text()).map(|(table, id, _)| (table, id)),
        children: list.0.get(&FILTER_CHILDREN).map(|p| keys(p).iter().map(|k| k.instance_id).collect()).unwrap_or_default(),
    })
}

/// The three feeds the grid offers.
pub fn read_feeds(index: &ResourceIndex) -> Vec<Feed> {
    FEEDS.iter().map(|i| read_feed(index, *i).unwrap_or(Feed { instance: *i, ..Feed::default() })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_name_the_type_filters() {
        assert_eq!(chain(0), &[CREATURES]);
        assert_eq!(chain(3), &[VEHICLES, UFO]);
        assert_eq!(chain(5), &[CELL]);
        assert_eq!(FEEDS.len(), 3);
        // A strip icon shows the type of its filter: the last filter of a
        // chain is its category's own.
        for category in 0..6 {
            assert_eq!(category_of(*chain(category).last().unwrap()), Some(category));
        }
        assert_eq!(category_of(0x1234), None);
    }

    #[test]
    fn key_arrays_read_every_entry() {
        let mut v = vec![];
        for (i, g) in [(0x11u32, 0u32), (0x22, 0)] {
            let mut b = Vec::new();
            b.extend_from_slice(&i.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&g.to_le_bytes());
            v.push(b);
        }
        let p = prop::Property { kind: 0x20, array: true, values: v };
        assert_eq!(keys(&p).iter().map(|k| k.instance_id).collect::<Vec<_>>(), vec![0x11, 0x22]);
    }

    /// The installed feeds and filters (needs the game install).
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_feeds_and_filters() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        let feeds = read_feeds(&index);
        // Everything has no root filter, My Creations shows UserCreated, Maxis ShippingContent.
        assert_eq!(feeds[0].filter, None);
        assert_eq!(feeds[1].filter, Some(0x37216F9C));
        assert_eq!(feeds[2].filter, Some(0x976CCB9A));
        assert_eq!(feeds[0].name, Some((0x2167_1745, 0x0001_0001)));
        assert_eq!(feeds[2].name, Some((0x2167_1745, 0x0001_0003)));
        assert!(feeds.iter().all(|f| f.icon.is_some()), "every feed has an icon PNG");
        for f in &feeds {
            let icon = f.icon.unwrap();
            assert!(index.records.contains_key(&icon), "feed icon {icon:?} is installed");
        }
        // The roots list the types; the first filter of every chain is one of
        // them (Maxis has flora too, the player's creations do not).
        for (root, categories) in [(0x976CCB9A_u32, &[0, 1, 2, 4, 5][..]), (0x37216F9C, &[0, 1, 2, 5][..])] {
            let r = read_filter(&index, root).expect("root filter");
            for &cat in categories {
                assert!(r.children.contains(&chain(cat)[0]), "{root:08X} lists {:08X}", chain(cat)[0]);
            }
        }
        let mine = read_filter(&index, 0x37216F9C).unwrap();
        assert!(!mine.children.contains(&FLORA), "My Creations has no flora type");
        let vehicles = read_filter(&index, VEHICLES).unwrap();
        assert!(vehicles.children.contains(&UFO));
        // The type names are the feed strings the column already shows.
        for (cat, id) in super::super::CATEGORY_IDS.iter().enumerate() {
            let f = read_filter(&index, *chain(cat).last().unwrap()).unwrap();
            assert_eq!(f.name.map(|n| n.1), Some(*id), "category {cat}");
        }
    }
}
