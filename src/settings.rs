//! What the user chose: the languages looked at, the tools of each one, and
//! where their programs are. Kept in `settings.json` in the plugin's data
//! folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tools::{self, GROUPS, LSP};

pub const FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The groups of [`tools::GROUPS`] that are looked at.
    #[serde(default)]
    pub languages: BTreeSet<String>,
    /// The tools chosen by hand for a group (none: those ready by themselves).
    #[serde(default)]
    pub tools: BTreeMap<String, Vec<String>>,
    /// The paths of programs typed by the user, by tool.
    #[serde(default)]
    pub programs: BTreeMap<String, String>,
}

impl Default for Settings {
    /// Every language, each with the tools it finds ready.
    fn default() -> Self {
        Settings {
            languages: GROUPS.iter().map(|g| g.id.to_string()).collect(),
            tools: BTreeMap::new(),
            programs: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, dir.join(FILE))
    }

    pub fn set_language(&mut self, group: &str, on: bool) {
        if on {
            self.languages.insert(group.to_string());
        } else {
            self.languages.remove(group);
        }
    }

    /// Back to automatic, or by hand starting from what was automatic.
    pub fn set_auto(&mut self, group: &str, auto: bool) {
        if auto {
            self.tools.remove(group);
            return;
        }
        let chosen = GROUPS
            .iter()
            .find(|g| g.id == group)
            .map(|g| {
                g.tools
                    .iter()
                    .copied()
                    .filter(|id| tools::tool_by_id(id).is_some_and(|t| t.auto))
                    .chain(std::iter::once(LSP))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        self.tools.insert(group.to_string(), chosen);
    }

    pub fn set_tool(&mut self, group: &str, tool: &str, on: bool) {
        let list = self.tools.entry(group.to_string()).or_default();
        list.retain(|t| t != tool);
        if on {
            list.push(tool.to_string());
        }
    }

    pub fn set_program(&mut self, tool: &str, path: &str) {
        if path.trim().is_empty() {
            self.programs.remove(tool);
        } else {
            self.programs.insert(tool.to_string(), path.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_is_on_until_turned_off_and_by_hand_starts_from_automatic() {
        let mut s = Settings::default();
        assert!(s.languages.contains("py") && s.languages.contains("js"));
        s.set_language("py", false);
        assert!(!s.languages.contains("py"));
        s.set_auto("py", false);
        assert_eq!(s.tools["py"], vec!["ruff".to_string(), LSP.to_string()]);
        s.set_tool("py", "mypy", true);
        s.set_tool("py", "ruff", false);
        assert_eq!(s.tools["py"], vec![LSP.to_string(), "mypy".to_string()]);
        s.set_auto("py", true);
        assert!(!s.tools.contains_key("py"));
        s.set_program("ruff", "/opt/ruff");
        s.set_program("ruff", " ");
        assert!(s.programs.is_empty());
    }

    #[test]
    fn settings_are_kept_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let mut s = Settings::default();
        s.set_language("rs", false);
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
        // What the editor wrote for a user of the lint before the plugin.
        std::fs::write(dir.path().join(FILE), r#"{"languages": ["py"]}"#).unwrap();
        let old = Settings::load(dir.path());
        assert_eq!(old.languages.len(), 1);
    }
}
