//! The Creature stage cinematics, in one place.
//!
//! Retail plays a named cinematic (`PlayCinematic("CRG_BrainUpgrade1")`) at
//! every stage moment. The scripts are the `creature.trigger`, `clg2crg.trigger`
//! and `crg2trg.trigger` texts of the installed game (decoded in
//! `notes\xstage-re\cinematics\scripts3`): per state they start the camera,
//! actor animations, effects, letterbox, mixer events (`mixevent <name> 1`
//! sets the audio parameter `<name>` that the music patches listen to),
//! sounds (`startsound`), pause the game and show a text window (`text` with a
//! `cinematics_crg` / `cinematics_trg` string id) that the player closes with a
//! button.
//!
//! For every cinematic this table keeps what the stage flow plays itself: the
//! mixer events, the sounds and music the script starts, and the closing text
//! window. The visual part (camera paths, avatar animations, fades, effects)
//! plays in the cinematic engine (`cinematics::hooks` starts it at the stage
//! moments, `cinematics::host` runs the installed script). All the names below
//! are read from the scripts; the strings are the installed English
//! `cinematics_*` locale ids.

/// Which locale table holds a cinematic's closing text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTable {
    /// `cinematics_crg` (Creature moments).
    Crg,
    /// `cinematics_trg` (the Tribe transition).
    Trg,
}

impl TextTable {
    pub fn name(self) -> &'static str {
        match self {
            TextTable::Crg => "cinematics_crg",
            TextTable::Trg => "cinematics_trg",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cinematic {
    /// The retail cinematic (script state) name.
    pub name: &'static str,
    /// Retail `description` of the first state (the caption shown by the
    /// cinematic UI), when the script has one.
    pub caption: Option<&'static str>,
    /// `mixevent <name> 1` at the start, `0` at the end: audio parameters the
    /// music patches read (`gfparam crg_cin_start`, ...).
    pub mix: &'static [&'static str],
    /// Sounds the script starts (`startsound`), in script order. Music and
    /// stingers (`music_brain_upgrade`) and effect sounds.
    pub sounds: &'static [&'static str],
    /// The text window the script ends on (`text ... cinematics_xxx!id`).
    pub text: Option<(TextTable, u32)>,
    /// Where the visual part plays (camera, actor animations, fades, effects).
    pub visual: &'static str,
}

const SKIPPED: &str = "the camera, actor animations, fades and effects play in the cinematic engine \
                       (`cinematics::hooks` / `host`, from the installed script); this table keeps \
                       the sounds, mixer events and the text window";

/// Every Creature cinematic the stage code starts, from the retail scripts.
pub const CINEMATICS: &[Cinematic] = &[
    // clg2crg.trigger: the cell crawls onto land; the goals text ends it.
    Cinematic {
        name: "CLG2CRG",
        caption: None,
        mix: &["cell_trans_end", "cell_to_crg_beach_music"],
        sounds: &["c2crg_vo_appear", "c2crg_vo_callbuddy", "c2crg_vo_buddies"],
        text: Some((TextTable::Crg, 0x0486_4C3C)),
        visual: SKIPPED,
    },
    // creature.trigger, general moments (first time hints).
    Cinematic {
        name: "CRG_FirstDNA",
        caption: Some("Earning DNA Points"),
        mix: &["crg_cin_start"],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F0E)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_FirstPartUnlock",
        caption: None,
        mix: &[],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F0F)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_FirstPosse",
        caption: Some("Posse"),
        mix: &["crg_cin_start"],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F10)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_SocialFailure",
        caption: Some("Social Failure"),
        mix: &["crg_cin_start"],
        sounds: &[],
        // The script's text id 0x93E071DC is not in the installed table.
        text: None,
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_TargetedCreature",
        caption: None,
        mix: &["crg_cin_start"],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F12)),
        visual: SKIPPED,
    },
    // Migration and nests.
    Cinematic {
        name: "CRG_SpeciesMigrate",
        caption: Some("CRG_SpeciesMigrate"),
        mix: &["crg_migrate_mix"],
        sounds: &["music_crg_migrate"],
        text: Some((TextTable::Crg, 0x0459_9F1A)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_ClaimNest",
        caption: None,
        mix: &["crg_migrate_mix"],
        sounds: &["music_crg_claimnest", "sfx_crg_cin_nest_claim_foley", "sfx_crg_cin_nest_claim"],
        text: Some((TextTable::Crg, 0x0540_222F)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_LaidEgg",
        caption: None,
        mix: &["play_mate_music"],
        sounds: &[],
        text: None,
        visual: SKIPPED,
    },
    // Deaths (the script of the starvation death has its own text).
    Cinematic {
        name: "CRG_AvatarDied",
        caption: Some("Avatar Died"),
        mix: &["crg_cin_death"],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F18)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_AvatarDied_Starvation",
        caption: Some("Avatar Died"),
        mix: &["crg_cin_death"],
        sounds: &[],
        text: Some((TextTable::Crg, 0x0459_9F29)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_AvatarDied_SeaMonster",
        caption: None,
        mix: &["crg_cin_death"],
        sounds: &["music_crg_cin_waterdeath"],
        text: Some((TextTable::Crg, 0x0508_C713)),
        visual: SKIPPED,
    },
    // Brain upgrades: `startsound music_brain_upgrade` for the whole cinematic
    // (stopped before the text), then the level's text window.
    Cinematic {
        name: "CRG_BrainUpgrade1",
        caption: Some("Brain Upgrade 1"),
        mix: &[],
        sounds: &["music_brain_upgrade"],
        text: Some((TextTable::Crg, 0x0540_76AA)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_BrainUpgrade2",
        caption: Some("Brain Upgrade 2"),
        mix: &[],
        sounds: &["music_brain_upgrade"],
        text: Some((TextTable::Crg, 0x0541_B1D1)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_BrainUpgrade3",
        caption: Some("Brain Upgrade 3"),
        mix: &[],
        sounds: &["music_brain_upgrade"],
        text: Some((TextTable::Crg, 0x0541_B208)),
        visual: SKIPPED,
    },
    Cinematic {
        name: "CRG_BrainUpgrade4",
        caption: Some("Brain Upgrade 4"),
        mix: &[],
        sounds: &["music_brain_upgrade"],
        text: Some((TextTable::Crg, 0x0541_B19F)),
        visual: SKIPPED,
    },
    // crg2trg.trigger: Evolve at the last brain level. The stick examining
    // clip carries `music_crg_to_trg` as an animation event (`startsound` is
    // commented out in the script); the freeze frame ends on the Tribal text.
    Cinematic {
        name: "CRG2TRG",
        caption: Some("CRG2TRG Timeline"),
        mix: &["crg_to_trg"],
        sounds: &["music_crg_to_trg"],
        text: Some((TextTable::Trg, 0x0000_0199)),
        visual: SKIPPED,
    },
];

/// The cinematic named `name`.
pub fn cinematic(name: &str) -> Option<&'static Cinematic> {
    CINEMATICS.iter().find(|c| c.name == name)
}

/// The brain upgrade cinematic of a brain level (1..=4).
pub fn brain_upgrade(level: u8) -> Option<&'static str> {
    match level {
        1 => Some("CRG_BrainUpgrade1"),
        2 => Some("CRG_BrainUpgrade2"),
        3 => Some("CRG_BrainUpgrade3"),
        4 => Some("CRG_BrainUpgrade4"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_table_names_every_cinematic_once() {
        let mut seen = HashSet::new();
        for c in CINEMATICS {
            assert!(seen.insert(c.name), "duplicate {}", c.name);
            assert!(!c.visual.is_empty());
        }
        // The cinematics the stage flow starts.
        for name in [
            "CLG2CRG",
            "CRG_FirstDNA",
            "CRG_FirstPartUnlock",
            "CRG_FirstPosse",
            "CRG_TargetedCreature",
            "CRG_SocialFailure",
            "CRG_BrainUpgrade1",
            "CRG_BrainUpgrade4",
            "CRG2TRG",
        ] {
            assert!(cinematic(name).is_some(), "{name}");
        }
    }

    #[test]
    fn brain_levels_map_to_their_upgrade_cinematic() {
        assert_eq!(brain_upgrade(0), None);
        for level in 1..=4u8 {
            let name = brain_upgrade(level).unwrap();
            assert_eq!(name, format!("CRG_BrainUpgrade{level}"));
            let c = cinematic(name).unwrap();
            assert_eq!(c.sounds, &["music_brain_upgrade"]);
            assert!(c.text.is_some());
        }
        assert_eq!(brain_upgrade(5), None);
    }

    /// Every sound the scripts start and every closing text exists in the
    /// installed data (a missing sound is silent in retail as well, so only
    /// the texts are required).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_texts_and_sounds_exist() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut assets = crate::spui::view::SpuiAssets::default();
        for c in CINEMATICS {
            if let Some((table, id)) = c.text {
                let text = assets.localized(&index, crate::world_objects::name_id(table.name()), id);
                assert!(text.is_some_and(|t| t.len() > 10), "{} text {id:08X}", c.name);
            }
        }
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        for c in CINEMATICS {
            for sound in c.sounds {
                let found = library.resolve(sound).is_ok();
                println!("cinematic {} sound {sound}: {}", c.name, if found { "installed" } else { "not installed" });
            }
        }
        for sound in ["music_brain_upgrade", "music_crg_to_trg", "music_crg_migrate", "music_crg_claimnest"] {
            assert!(library.resolve(sound).is_ok(), "{sound}");
        }
    }
}
