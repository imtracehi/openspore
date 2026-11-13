//! Indexed original resources and inherited part definitions. No EA data is
//! embedded: every name, model, price, capability and texture comes from the
//! selected install. Property IDs are documented in SporeModder-FX/reg_property.txt.
use crate::{
    dbpf::{self, DbpfEntry, ResourceKey},
    prop::{self, PropertyList},
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

pub const PROP: u32 = 0x00b1b104;
pub const RW4: u32 = 0x2f4e681b;
pub const CELL_PARTS: u32 = 0x40616000;
pub const CREATURE_PARTS: u32 = 0x40626000;
pub const BUILDING_PARTS: u32 = 0x40636000;
pub const VEHICLE_PARTS: u32 = 0x40646000;
pub const TOOL_PARTS: u32 = 0x40686000;
pub const PARENT: u32 = 0x00b2cccb;

#[derive(Debug, Clone, Default)]
pub struct ResourceIndex {
    pub records: HashMap<ResourceKey, (PathBuf, DbpfEntry)>,
    pub errors: Vec<String>,
}
impl ResourceIndex {
    pub fn load(packages: &[PathBuf]) -> Self {
        let mut paths = packages.to_vec();
        // English tables are a language selection, not competing patches.
        paths.retain(|p| {
            !p.components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case("locale"))
                || p.components()
                    .any(|c| c.as_os_str().eq_ignore_ascii_case("en-us"))
        });
        // First definition wins. Patches override the shipped base resources.
        paths.sort_by_key(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            (
                if name == "patchdata.package" {
                    0
                } else if name == "spore_pack_03.package" {
                    1
                } else if p
                    .components()
                    .any(|c| c.as_os_str().eq_ignore_ascii_case("DataEP1"))
                {
                    2
                } else if name == "boosterpack_01.package" {
                    3
                } else {
                    4
                },
                p.clone(),
            )
        });
        let mut out = Self::default();
        for path in paths {
            match dbpf::read_package_index(&path) {
                Ok(index) => {
                    for entry in index.entries {
                        out.records
                            .entry(entry.key)
                            .or_insert((path.clone(), entry));
                    }
                }
                Err(err) => out.errors.push(err),
            }
        }
        out
    }
    pub fn read(&self, key: ResourceKey) -> Result<Vec<u8>, String> {
        let (path, entry) = self.records.get(&key).ok_or_else(|| {
            format!(
                "missing resource {:08x}!{:08x}.{:08x}",
                key.group_id, key.instance_id, key.type_id
            )
        })?;
        if entry.mem_size > 64 * 1024 * 1024 {
            return Err("resource exceeds decode limit".into());
        }
        dbpf::read_entry_bytes(path, entry)
    }
    /// Resolve world object definitions through the same native inheritance
    /// rules as editor parts, without restricting them to editor group IDs.
    pub fn properties(&self, key: ResourceKey) -> Result<PropertyList, String> {
        self.inherited(key, &mut HashMap::new(), &mut HashSet::new())
    }
    fn inherited(
        &self,
        key: ResourceKey,
        cache: &mut HashMap<ResourceKey, PropertyList>,
        stack: &mut HashSet<ResourceKey>,
    ) -> Result<PropertyList, String> {
        if let Some(p) = cache.get(&key) {
            return Ok(p.clone());
        }
        if stack.len() >= 64 || !stack.insert(key) {
            return Err("cyclic part inheritance".into());
        }
        let child = prop::parse(&self.read(key)?)?;
        let mut out = if let Some(mut parent) = child
            .0
            .get(&PARENT)
            .and_then(|p| p.key())
            .filter(|p| p.instance_id != 0)
        {
            // PropManager.GetPropertyList resolves instance and group only.
            // Some booster records carry a mesh type in their parent key;
            // inheritance still resolves a PROP, preserving the authored IDs.
            parent.type_id = PROP;
            // Native world-object PROP files sometimes store a group-zero
            // parent reference even though the parent PROP is scoped to the
            // child group's namespace. Prefer a real global definition when
            // present; only use the child group when the exact global key is
            // absent and that scoped key is actually indexed.
            if parent.group_id == 0 && !self.records.contains_key(&parent) {
                let scoped = ResourceKey {
                    group_id: key.group_id,
                    ..parent
                };
                if self.records.contains_key(&scoped) {
                    parent = scoped;
                }
            }
            self.inherited(parent, cache, stack)?
        } else {
            PropertyList::default()
        };
        out.0.extend(child.0);
        stack.remove(&key);
        cache.insert(key, out.clone());
        Ok(out)
    }
}

// Preserve authored levels; they are maxima, never summed for duplicate parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Capabilities {
    pub herbivore: u8,
    pub carnivore: u8,
    pub health: u8,
    pub speed: u8,
    pub bite: u8,
    pub strike: u8,
    pub charge: u8,
    pub spit: u8,
    pub sing: u8,
    pub dance: u8,
    pub charm: u8,
    pub pose: u8,
    pub jump: u8,
    pub sprint: u8,
    pub glide: u8,
    pub sneak: u8,
    pub mouth: u8,
    pub eye: u8,
    pub foot: u8,
    pub grasper: u8,
    pub cell_flagella: u8,
    pub cell_cilia: u8,
    pub cell_jet: u8,
    pub cell_spike: u8,
    pub cell_poison: u8,
    pub cell_electric: u8,
}
impl Capabilities {
    pub fn merge(&mut self, other: Self) {
        macro_rules! maximum {($($f:ident),*)=>{$(self.$f=self.$f.max(other.$f);)*};}
        maximum!(
            herbivore,
            carnivore,
            health,
            speed,
            bite,
            strike,
            charge,
            spit,
            sing,
            dance,
            charm,
            pose,
            jump,
            sprint,
            glide,
            sneak,
            mouth,
            eye,
            foot,
            grasper,
            cell_flagella,
            cell_cilia,
            cell_jet,
            cell_spike,
            cell_poison,
            cell_electric
        );
    }
    pub fn summary(&self) -> String {
        let mut labels = Vec::new();
        for (name, level) in [
            ("Bite", self.bite),
            ("Strike", self.strike),
            ("Charge", self.charge),
            ("Spit", self.spit),
            ("Sing", self.sing),
            ("Dance", self.dance),
            ("Charm", self.charm),
            ("Pose", self.pose),
            ("Health", self.health),
            ("Speed", self.speed),
            ("Jump", self.jump),
            ("Sprint", self.sprint),
            ("Glide", self.glide),
            ("Sneak", self.sneak),
            ("Flagella", self.cell_flagella),
            ("Cilia", self.cell_cilia),
            ("Jet", self.cell_jet),
            ("Spike", self.cell_spike),
            ("Poison", self.cell_poison),
            ("Electric", self.cell_electric),
        ] {
            if level > 0 {
                labels.push(format!("{name} {level}"));
            }
        }
        if self.herbivore > 0 {
            labels.push("Herbivore".into());
        }
        if self.carnivore > 0 {
            labels.push("Carnivore".into());
        }
        labels.join(" | ")
    }
}

#[derive(Debug, Clone)]
pub struct PartDefinition {
    pub key: ResourceKey,
    pub model: ResourceKey,
    pub name: String,
    pub is_null: bool,
    pub price: u32,
    pub unlock_level: u32,
    pub min_scale: f32,
    pub max_scale: f32,
    pub default_scale: f32,
    pub model_scale: f32,
    pub model_offset: [f32; 3],
    pub model_rotation: [f32; 3],
    pub diffuse: Option<ResourceKey>,
    pub tint: Option<ResourceKey>,
    pub spec_bump: Option<ResourceKey>,
    pub abilities: Capabilities,
}
impl PartDefinition {
    /// Material-aware rendering of a standalone original model. This helper
    /// creates no gameplay part or catalog entry and grants no abilities.
    pub fn raw_model(model: ResourceKey) -> Self {
        Self {
            key: model,
            model,
            name: format!("Model {:08X}", model.instance_id),
            is_null: true,
            price: 0,
            unlock_level: 0,
            min_scale: 1.,
            max_scale: 1.,
            default_scale: 1.,
            model_scale: 1.,
            model_offset: [0.; 3],
            model_rotation: [0.; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities::default(),
        }
    }
    pub fn category(&self) -> &'static str {
        let a = self.abilities;
        if a.mouth > 0 || a.herbivore > 0 || a.carnivore > 0 {
            "Mouths"
        } else if a.eye > 0 {
            "Eyes"
        } else if a.cell_flagella > 0 || a.cell_cilia > 0 || a.cell_jet > 0 {
            "Movement"
        } else if a.foot > 0 {
            "Feet"
        } else if a.grasper > 0 {
            "Hands"
        } else if a.cell_spike > 0
            || a.cell_poison > 0
            || a.cell_electric > 0
            || a.strike > 0
            || a.charge > 0
            || a.spit > 0
        {
            "Weapons"
        } else {
            "Details"
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PartCatalog {
    pub parts: Vec<PartDefinition>,
    pub errors: Vec<String>,
}
impl PartCatalog {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let mut result = Self::default();
        let mut props = HashMap::new();
        let mut locales: HashMap<u32, BTreeMap<u32, String>> = HashMap::new();
        let mut keys: Vec<_> = index
            .records
            .keys()
            .filter(|k| {
                k.type_id == PROP
                    && [
                        CELL_PARTS,
                        CREATURE_PARTS,
                        BUILDING_PARTS,
                        VEHICLE_PARTS,
                        TOOL_PARTS,
                    ]
                    .contains(&k.group_id)
            })
            .copied()
            .collect();
        keys.sort_by_key(|k| (k.group_id, k.instance_id));
        for key in keys {
            let p = match index.inherited(key, &mut props, &mut HashSet::new()) {
                Ok(p) => p,
                Err(err) => {
                    result
                        .errors
                        .push(format!("{:08x}: {err}", key.instance_id));
                    continue;
                }
            };
            let Some(model) =
                p.0.get(&0x00f9efbb)
                    .and_then(|v| v.key())
                    .filter(|k| k.type_id == RW4 && index.records.contains_key(k))
            else {
                continue;
            };
            let number = |id, default| {
                p.0.get(&id)
                    .and_then(|v| v.number())
                    .filter(|n| n.is_finite())
                    .unwrap_or(default)
            };
            let level = |id| number(id, 0.).clamp(0., 255.) as u8;
            let vector = |id| {
                p.0.get(&id)
                    .and_then(|v| v.vector3())
                    .filter(|v| v.iter().all(|n| n.is_finite()))
                    .unwrap_or([0.; 3])
            };
            let texture = |id| {
                p.0.get(&id)
                    .and_then(|v| v.key())
                    .filter(|k| k.instance_id != 0)
            };
            let (table, text_id, fallback) = p
                .0
                .get(&0x8f6fc401)
                .and_then(|v| v.text())
                .unwrap_or((0, 0, format!("Part {:08X}", key.instance_id)));
            let name = locales
                .entry(table)
                .or_insert_with(|| {
                    // Locale table IDs are the resource instance IDs, not groups.
                    let mut records: Vec<_> = index
                        .records
                        .keys()
                        .filter(|k| k.type_id == 0x02fac0b6 && k.instance_id == table)
                        .copied()
                        .collect();
                    records.sort_by_key(|k| k.group_id);
                    records
                        .into_iter()
                        .filter_map(|k| index.read(k).ok())
                        .flat_map(|b| crate::decode::parse_locale_table(&b))
                        .collect()
                })
                .get(&text_id)
                .cloned()
                .unwrap_or(fallback);
            result.parts.push(PartDefinition {
                key,
                model,
                name,
                is_null: number(0x02437197, 0.) != 0.,
                price: number(0x02166464, 0.).max(0.) as u32,
                unlock_level: number(0x03a289ac, 0.).max(0.) as u32,
                min_scale: number(0xf023ed73, 0.5).max(0.01),
                max_scale: number(0xf023ed79, 2.).max(0.01),
                default_scale: number(0x0536c50c, 1.).max(0.01),
                model_scale: number(0x00fba611, 1.).max(0.001),
                model_offset: vector(0x00fba610),
                model_rotation: vector(0x00fba613),
                diffuse: texture(0x02424655),
                tint: texture(0x02424657),
                spec_bump: texture(0x02424656),
                abilities: Capabilities {
                    herbivore: level(0x022e785c)
                        .max(level(0x11b79a70))
                        .max(level(0x11b79a78)),
                    carnivore: level(0x022e7847)
                        .max(level(0x11b79a74))
                        .max(level(0x11b79a78)),
                    health: level(0x31c3e5b2),
                    speed: level(0xb354a87c),
                    bite: level(0xb1c3e5b7),
                    strike: level(0xb1c3e5c0),
                    charge: level(0xb1c3e5b8),
                    spit: level(0xb1c3e5b9),
                    sing: level(0xb1c3e5c2),
                    dance: level(0xb1c3e5c1),
                    charm: level(0xb1c3e5c3),
                    pose: level(0xb1c3e5c4),
                    jump: level(0x330c117a),
                    sprint: level(0x3386c531),
                    glide: level(0x04f4e1b4),
                    sneak: level(0x51c3e5b4),
                    mouth: level(0xb00f0fec).max(level(0x11b79301)),
                    eye: level(0xb00f0fe9).max(level(0x11b79a73)),
                    foot: level(0xb00f0fef),
                    grasper: level(0xb00f0fe2),
                    cell_flagella: level(0x032f92e6),
                    cell_cilia: level(0x11b79a77),
                    cell_jet: level(0x11b79a72),
                    cell_spike: level(0x11b79a71),
                    cell_poison: level(0x11b79a76),
                    cell_electric: level(0x11b79a75),
                },
            });
        }
        result
            .parts
            .sort_by_key(|p| (p.key.group_id, p.category(), p.price, p.key.instance_id));
        result
    }
    pub fn by_model(&self, instance: u32) -> Option<&PartDefinition> {
        self.parts.iter().find(|p| p.model.instance_id == instance)
    }
    pub fn by_model_in(&self, group: u32, instance: u32) -> Option<&PartDefinition> {
        self.parts
            .iter()
            .find(|p| p.key.group_id == group && p.model.instance_id == instance)
    }
    pub fn by_definition_in(&self, group: u32, definition_id: u32) -> Option<&PartDefinition> {
        self.parts
            .iter()
            .find(|p| p.key.group_id == group && p.key.instance_id == definition_id && !p.is_null)
    }
    pub fn by_part_in(
        &self,
        group: u32,
        part: &crate::game::PartInstance,
    ) -> Option<&PartDefinition> {
        part.definition_id
            .and_then(|def_id| self.by_definition_in(group, def_id))
            .or_else(|| self.by_model_in(group, part.instance))
    }
    pub fn abilities_for_parts_in<'a>(
        &self,
        group: u32,
        parts: impl IntoIterator<Item = &'a crate::game::PartInstance>,
    ) -> Capabilities {
        let mut out = Capabilities::default();
        for part in parts {
            if let Some(p) = self.by_part_in(group, part) {
                out.merge(p.abilities);
            }
        }
        out
    }
    pub fn abilities_in(&self, group: u32, instances: impl Iterator<Item = u32>) -> Capabilities {
        let mut out = Capabilities::default();
        for instance in instances {
            if let Some(p) = self.by_model_in(group, instance) {
                out.merge(p.abilities);
            }
        }
        out
    }
    pub fn abilities(&self, instances: impl Iterator<Item = u32>) -> Capabilities {
        let mut out = Capabilities::default();
        for instance in instances {
            if let Some(p) = self.by_model(instance) {
                out.merge(p.abilities);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    const TEST_MARKER: u32 = 0x5000_0001;
    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    fn key(instance_id: u32, group_id: u32) -> ResourceKey {
        ResourceKey {
            type_id: PROP,
            group_id,
            instance_id,
        }
    }

    fn prop_bytes(properties: &[(u32, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(properties.len() as u32).to_be_bytes());
        for (id, kind, value) in properties {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&kind.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes());
            if *kind == 0x12 || *kind == 0x13 {
                let char_count = if *kind == 0x13 {
                    value.len() / 2
                } else {
                    value.len()
                };
                out.extend_from_slice(&(char_count as u32).to_be_bytes());
            }
            out.extend_from_slice(value);
        }
        out
    }

    fn fixture_index(resources: Vec<(ResourceKey, Vec<u8>)>) -> ResourceIndex {
        let path = std::env::temp_dir().join(format!(
            "openspore-parent-index-{}-{}.bin",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut bytes = Vec::new();
        let mut records = HashMap::new();
        for (key, data) in resources {
            let offset = bytes.len() as u64;
            let len = data.len() as u32;
            bytes.extend_from_slice(&data);
            records.insert(
                key,
                (
                    path.clone(),
                    DbpfEntry {
                        key,
                        offset,
                        disk_size: len,
                        mem_size: len,
                        is_compressed: false,
                    },
                ),
            );
        }
        fs::write(&path, bytes).unwrap();
        ResourceIndex {
            records,
            errors: Vec::new(),
        }
    }

    fn marker(value: u8) -> (u32, u16, Vec<u8>) {
        (TEST_MARKER, 1, vec![value])
    }

    fn child_with_parent(parent: ResourceKey) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&parent.instance_id.to_le_bytes());
        raw.extend_from_slice(&parent.type_id.to_le_bytes());
        raw.extend_from_slice(&parent.group_id.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        prop_bytes(&[(PARENT, 0x20, raw)])
    }

    #[test]
    fn abilities_use_best_part_instead_of_stacking_duplicates() {
        let mut a = Capabilities {
            bite: 3,
            sing: 1,
            ..Default::default()
        };
        a.merge(a);
        a.merge(Capabilities {
            bite: 1,
            sing: 5,
            ..Default::default()
        });
        assert_eq!((a.bite, a.sing), (3, 5));
    }

    #[test]
    fn group_zero_parent_prefers_an_exact_global_definition() {
        let child = key(1, 0x1111);
        let global_parent = key(2, 0);
        let scoped_parent = key(2, child.group_id);
        let index = fixture_index(vec![
            (child, child_with_parent(global_parent)),
            (global_parent, prop_bytes(&[marker(7)])),
            (scoped_parent, prop_bytes(&[marker(9)])),
        ]);
        let resolved = index.properties(child).unwrap();
        assert_eq!(resolved.0[&TEST_MARKER].number(), Some(7.0));
    }

    #[test]
    fn missing_group_zero_parent_falls_back_to_same_child_group() {
        let child = key(3, 0x2222);
        let global_parent = key(4, 0);
        let scoped_parent = key(4, child.group_id);
        let index = fixture_index(vec![
            (child, child_with_parent(global_parent)),
            (scoped_parent, prop_bytes(&[marker(11)])),
        ]);
        let resolved = index.properties(child).unwrap();
        assert_eq!(resolved.0[&TEST_MARKER].number(), Some(11.0));
    }

    #[test]
    fn missing_group_zero_parent_does_not_guess_an_unrelated_group() {
        let child = key(5, 0x3333);
        let global_parent = key(6, 0);
        let unrelated_parent = key(6, 0x4444);
        let index = fixture_index(vec![
            (child, child_with_parent(global_parent)),
            (unrelated_parent, prop_bytes(&[marker(13)])),
        ]);
        let error = index.properties(child).unwrap_err();
        assert!(
            error.contains("missing resource 00000000!00000006.00b1b104"),
            "{error}"
        );
    }

    #[test]
    #[ignore = "requires the installed original Spore package indexes"]
    fn installed_wall_and_geyser_resolve_their_group_scoped_parents() {
        let install = crate::spore_install::discover().expect("original install");
        let index = ResourceIndex::load(&install.packages);
        let wall = index
            .properties(ResourceKey {
                type_id: PROP,
                group_id: 0x40828100,
                instance_id: 0x25a92685,
            })
            .expect("wall PROP inheritance");
        assert!(
            wall.0.contains_key(&0x00b2ccca),
            "wall inherits its native name"
        );
        assert!(
            wall.0.contains_key(&0x00f9efbb),
            "wall keeps its child properties"
        );

        let geyser = index
            .properties(ResourceKey {
                type_id: PROP,
                group_id: 0x41842000,
                instance_id: 0xa5aab70a,
            })
            .expect("geyser PROP inheritance");
        assert!(
            geyser.0.contains_key(&0x0442151e),
            "geyser inherits its terrain model reference"
        );
        assert!(
            geyser.0.contains_key(&0x03abc381),
            "geyser keeps its child decal"
        );
        assert!(
            geyser.0.contains_key(&0x03b28c1b),
            "geyser keeps its child stamp"
        );
    }
}
