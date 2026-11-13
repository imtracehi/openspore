//! The Sporepedia store: creations made in the creators opened from the
//! galaxy screen (retail saves every menu creation to Sporepedia). Kept as
//! `<save stem>-sporepedia.json` beside the saves, never inside a saved game.
//!
//! A creature keeps what its creator edits (body, parts, name, paint, as
//! `creature_editor::EntrySnapshot`) and, for an outfitter creation, the
//! outfitter and the accessories; a building, vehicle or UFO keeps its rigid
//! design and the editor setup it belongs to.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    body::BodyProfile,
    creature_editor_paint::CreaturePaint,
    game::{PartInstance, RunProgress},
    rigid_editor::RigidDesign,
};

/// A creature as its creator saves it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StoredCreature {
    pub body: BodyProfile,
    pub parts: Vec<PartInstance>,
    pub paint: CreaturePaint,
    /// The outfitter's setup (`TribalAccessoriesEditorSetup`, `Civ...`,
    /// `Space...`) and the worn accessories (empty = a plain creature).
    pub outfitter: String,
    pub outfit: Vec<PartInstance>,
}

/// What a stored creation is.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StoredKind {
    Creature(StoredCreature),
    /// A rigid design and its editor setup (`SetupKind::setup_name`).
    Design { setup: String, design: RigidDesign },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredCreation {
    pub id: u64,
    pub name: String,
    /// Seconds since 1970.
    pub created: u64,
    pub modified: u64,
    pub kind: StoredKind,
}

impl StoredCreation {
    pub fn is_creature(&self) -> bool {
        matches!(self.kind, StoredKind::Creature(_))
    }

    /// A scratch record holding the creation, for its creator and its
    /// thumbnail.
    pub fn to_run(&self) -> RunProgress {
        let mut run = RunProgress::default();
        match &self.kind {
            StoredKind::Creature(c) => {
                run.body = c.body.clone();
                run.parts = c.parts.clone();
                run.creature_creation.name = self.name.clone();
                run.creature_creation.paint = c.paint.clone();
                if let Some(outfit) = outfit_record(&mut run, &c.outfitter) {
                    outfit.parts = c.outfit.clone();
                }
                run.sync_legacy_accessory();
            }
            StoredKind::Design { setup, design } => {
                run.rigid.designs.insert(setup.clone(), design.clone());
            }
        }
        run
    }
}

/// The record an outfitter writes in a run.
pub fn outfit_record<'a>(run: &'a mut RunProgress, outfitter: &str) -> Option<&'a mut crate::outfitter::Outfit> {
    use crate::creature_editor_data::SetupKind;
    match outfitter {
        o if o == SetupKind::Tribal.setup_name() => Some(&mut run.outfits.tribal),
        o if o == SetupKind::Civ.setup_name() => Some(&mut run.outfits.civ),
        o if o == SetupKind::Space.setup_name() => Some(&mut run.outfits.space),
        _ => None,
    }
}

/// A creature of a run as the store keeps it.
pub fn creature_of(run: &RunProgress) -> StoredCreature {
    StoredCreature {
        body: run.body.clone(),
        parts: run.parts.clone(),
        paint: run.creature_creation.paint.clone(),
        outfitter: String::new(),
        outfit: Vec::new(),
    }
}

/// The store file.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Store {
    pub creations: Vec<StoredCreation>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Store {
    pub fn path_for(save: &Path) -> PathBuf {
        let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
        save.with_file_name(format!("{stem}-sporepedia.json"))
    }

    pub fn load_beside(save: &Path) -> Self {
        let path = Self::path_for(save);
        let mut store = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Store>(&b).ok()).unwrap_or_default();
        store.path = Some(path);
        store
    }

    pub fn store(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        std::fs::write(path, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    pub fn get(&self, id: u64) -> Option<&StoredCreation> {
        self.creations.iter().find(|c| c.id == id)
    }

    /// Add a creation (a new id) or replace the one with `replace`'s id.
    pub fn put(&mut self, name: String, kind: StoredKind, replace: Option<u64>) -> u64 {
        let now = now_secs();
        if let Some(c) = replace.and_then(|id| self.creations.iter_mut().find(|c| c.id == id)) {
            c.name = name;
            c.kind = kind;
            c.modified = now;
            return c.id;
        }
        // Ids follow the creation time; a second creation in the same
        // second takes the next free id.
        let mut id = now.max(1) * 1000;
        while self.creations.iter().any(|c| c.id == id) {
            id += 1;
        }
        self.creations.push(StoredCreation { id, name, created: now, modified: now, kind });
        id
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.creations.len();
        self.creations.retain(|c| c.id != id);
        self.creations.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creations_are_added_replaced_deleted_and_kept_beside_the_saves() {
        let dir = std::env::temp_dir().join(format!("openspore-fe-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = dir.join("qc_save.json");
        assert_eq!(Store::path_for(&save), dir.join("qc_save-sporepedia.json"));
        let mut store = Store::load_beside(&save);
        assert!(store.creations.is_empty());
        let mut run = RunProgress::default();
        run.creature_creation.name = "Blib".into();
        let a = store.put("Blib".into(), StoredKind::Creature(creature_of(&run)), None);
        let b = store.put("Hut".into(), StoredKind::Design { setup: "BuildingHouse".into(), design: RigidDesign::default() }, None);
        assert_ne!(a, b);
        assert_eq!(store.put("Blob".into(), StoredKind::Creature(creature_of(&run)), Some(a)), a);
        assert_eq!(store.get(a).map(|c| c.name.as_str()), Some("Blob"));
        store.store().unwrap();
        let mut again = Store::load_beside(&save);
        assert_eq!(again.creations.len(), 2);
        assert!(again.get(b).unwrap().to_run().rigid.designs.contains_key("BuildingHouse"));
        assert!(again.delete(b));
        assert!(!again.delete(b));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_outfitter_creation_dresses_its_creature() {
        let mut c = creature_of(&RunProgress::default());
        c.outfitter = crate::creature_editor_data::SetupKind::Civ.setup_name().into();
        c.outfit = vec![serde_json::from_str::<PartInstance>(r#"{"instance":1,"bone":0}"#).unwrap()];
        let stored = StoredCreation { id: 1, name: "Dressed".into(), created: 0, modified: 0, kind: StoredKind::Creature(c) };
        let run = stored.to_run();
        assert_eq!(run.outfits.civ.parts.len(), 1);
        assert_eq!(run.creature_creation.name, "Dressed");
    }
}
