use crate::files::{read_json, write_atomic};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How much an entity matters for the story. `Minor` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    /// A core character. Every prompt and package lists it.
    Main,
    /// A character who comes back, or an organization. Every prompt and package lists it.
    Recurring,
    /// Everything else.
    Minor,
}

/// An answer that gives the importance of an entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub episode: u32,
    pub importance: Importance,
}

/// The importance of every entity: the declarations of the answers, and the settings that the user
/// made by hand.
#[derive(Debug, Clone, Default)]
pub struct Importances {
    declarations: Vec<Declaration>,
    overrides: BTreeMap<String, Importance>,
}

impl Importances {
    /// `declarations` are in the order of the answers: by episode, and then in the answer.
    pub fn new(
        declarations: Vec<Declaration>,
        overrides: BTreeMap<String, Importance>,
    ) -> Importances {
        Importances {
            declarations,
            overrides,
        }
    }

    /// The hand setting, else the last declaration at or before `up_to_episode`, else `Minor`.
    pub fn of(&self, name: &str, up_to_episode: u32) -> Importance {
        if let Some(importance) = self.manual(name) {
            return importance;
        }
        self.declarations
            .iter()
            .rev()
            .find(|declaration| declaration.name == name && declaration.episode <= up_to_episode)
            .map_or(Importance::Minor, |declaration| declaration.importance)
    }

    pub fn manual(&self, name: &str) -> Option<Importance> {
        self.overrides.get(name).copied()
    }
}

fn overrides_path(series_dir: &Path) -> PathBuf {
    series_dir.join("importance.json")
}

/// Reads the hand settings, `importance.json`. A missing file gives no settings.
pub fn load_overrides(series_dir: &Path) -> Result<BTreeMap<String, Importance>> {
    let path = overrides_path(series_dir);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    read_json(&path)
}

pub fn save_overrides(series_dir: &Path, overrides: &BTreeMap<String, Importance>) -> Result<()> {
    write_atomic(
        &overrides_path(series_dir),
        &serde_json::to_string_pretty(overrides)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(name: &str, episode: u32, importance: Importance) -> Declaration {
        Declaration {
            name: name.to_string(),
            episode,
            importance,
        }
    }

    #[test]
    fn the_last_declaration_up_to_the_episode_wins() {
        let importances = Importances::new(
            vec![
                declaration("宇都見", 1, Importance::Minor),
                declaration("宇都見", 20, Importance::Main),
            ],
            BTreeMap::new(),
        );

        assert_eq!(importances.of("宇都見", 19), Importance::Minor);
        assert_eq!(importances.of("宇都見", 20), Importance::Main);
    }

    #[test]
    fn a_hand_setting_wins_over_every_declaration() {
        let importances = Importances::new(
            vec![declaration("宇都見", 20, Importance::Main)],
            BTreeMap::from([("宇都見".to_string(), Importance::Recurring)]),
        );

        assert_eq!(importances.of("宇都見", 20), Importance::Recurring);
        assert_eq!(importances.manual("宇都見"), Some(Importance::Recurring));
    }

    #[test]
    fn an_entity_with_no_declaration_is_minor() {
        let importances = Importances::default();

        assert_eq!(importances.of("誰か", 5), Importance::Minor);
        assert_eq!(importances.manual("誰か"), None);
    }
}
