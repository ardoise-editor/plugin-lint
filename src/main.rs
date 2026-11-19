//! The Lint plugin of Ardoise: the problems that linters (ESLint, ruff,
//! ShellCheck, Clippy…) find in a file, with their fixes. The editor shows
//! them, with those of its language servers.

mod parse;
mod run;
mod settings;
mod sys;
mod text;
mod tools;

use std::path::{Path, PathBuf};

use simpleedit_plugin::lint::{FixParams, FixResult, LintParams, LintResult};
use simpleedit_plugin::protocol::{InitializeParams, Settings as EditorSettings, UiEvent};
use simpleedit_plugin::ui::*;
use simpleedit_plugin::{run, Host, Plugin};

use settings::Settings;
use text::t;
use tools::{Availability, GROUPS, LSP};

/// The panel of its settings, in Preferences › Plugins.
const SETTINGS: &str = "settings";

struct Lint {
    settings: Settings,
    data_dir: PathBuf,
    /// Where the availability of the tools is shown for.
    near: PathBuf,
    french: bool,
    runtime: tokio::runtime::Runtime,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

impl Lint {
    fn new() -> Self {
        Lint {
            settings: Settings::default(),
            data_dir: PathBuf::new(),
            near: PathBuf::from("/"),
            french: false,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime"),
        }
    }

    /// The tools to run on a file and whether the language server is wanted
    /// (nothing for a language that is off).
    fn plan(&self, path: &Path, trusted: bool) -> Option<tools::Plan> {
        let ext = ext_of(path);
        let group = tools::group_of(&ext)?;
        if !self.settings.languages.contains(group.id) {
            return None;
        }
        let manual = self.settings.tools.get(group.id).map(Vec::as_slice);
        Some(tools::plan(
            path,
            &ext,
            manual,
            &self.settings.programs,
            trusted,
        ))
    }

    fn lint_file(&self, params: &LintParams) -> LintResult {
        let Some(plan) = self.plan(&params.path, params.trusted) else {
            return LintResult::default();
        };
        // The tools that read the disk say nothing of text not saved yet.
        let (now, later): (Vec<_>, Vec<_>) = plan
            .tools
            .into_iter()
            .partition(|(tool, _)| tool.stdin || !params.dirty);
        let skipped: Vec<String> = later.iter().map(|(t, _)| t.name.to_string()).collect();
        let mut ran: Vec<String> = now.iter().map(|(t, _)| t.name.to_string()).collect();
        ran.extend(skipped.iter().cloned());
        let job = run::Job {
            path: params.path.clone(),
            text: params.text.clone(),
        };
        let (diagnostics, failed) = self.runtime.block_on(run::all(now, job));
        LintResult {
            diagnostics,
            failed,
            ran,
            skipped,
            language_server: plan.lsp,
        }
    }

    fn fix_file(&self, params: &FixParams) -> FixResult {
        let Some(mut plan) = self.plan(&params.path, params.trusted) else {
            return FixResult::default();
        };
        // The tools that read the disk would fix another text.
        if params.dirty {
            plan.tools.retain(|(tool, _)| tool.stdin);
        }
        let (fixed, language_server) = self.runtime.block_on(run::fix(
            plan.tools,
            plan.lsp,
            &params.path,
            &params.text,
            params.level,
            params.source.as_deref(),
        ));
        FixResult {
            text: (fixed != params.text).then_some(fixed),
            language_server,
        }
    }

    fn state_of(&self, availability: Availability) -> &'static str {
        match availability {
            Availability::Ready(_) => t(self.french, "ready"),
            Availability::NoConfig(_) => t(self.french, "no_config"),
            Availability::NoProgram => t(self.french, "missing"),
        }
    }

    /// The settings page: a box per language, and when one is chosen by
    /// hand, its tools.
    fn view(&self) -> Node {
        let s = &self.settings;
        let mut rows = vec![text(t(self.french, "intro")).style(TextStyle::Muted)];
        for group in GROUPS {
            let on = s.languages.contains(group.id);
            rows.push(separator());
            rows.push(checkbox(format!("lang:{}", group.id), group.name, on));
            if !on {
                continue;
            }
            let by_hand = s.tools.get(group.id);
            rows.push(
                row([
                    space(0.0),
                    checkbox(
                        format!("auto:{}", group.id),
                        t(self.french, "automatic"),
                        by_hand.is_none(),
                    ),
                ])
                .spacing(24.0),
            );
            let Some(chosen) = by_hand else {
                continue;
            };
            let has = |id: &str| chosen.iter().any(|c| c == id);
            for tool in group.tools.iter().filter_map(|id| tools::tool_by_id(id)) {
                let typed = s.programs.get(tool.id).cloned().unwrap_or_default();
                let chosen_path = s.programs.get(tool.id).map(String::as_str);
                let state =
                    self.state_of(tools::availability(tool, &self.near, chosen_path, false));
                let mut line = vec![
                    space(0.0),
                    checkbox(
                        format!("tool:{}:{}", group.id, tool.id),
                        tool.name,
                        has(tool.id),
                    ),
                    text(state).style(TextStyle::Muted),
                ];
                if !tool.program.is_empty() {
                    let found = tools::program_path(tool, &self.near, None, false)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| t(self.french, "path").to_string());
                    line.push(input(format!("program:{}", tool.id), typed, found));
                }
                rows.push(row(line).spacing(24.0));
            }
            rows.push(
                row([
                    space(0.0),
                    checkbox(
                        format!("tool:{}:{LSP}", group.id),
                        t(self.french, "language_server"),
                        has(LSP),
                    ),
                ])
                .spacing(24.0),
            );
        }
        column(rows).spacing(8.0)
    }

    /// Applies what was done on the settings page; `false` if it was not
    /// one of its controls.
    fn apply(&mut self, event: &UiEvent) -> bool {
        let parts: Vec<&str> = event.action.splitn(3, ':').collect();
        let checked = event.checked().unwrap_or(false);
        let known_group = |g: &str| GROUPS.iter().any(|x| x.id == g);
        match parts.as_slice() {
            ["lang", group] if known_group(group) => self.settings.set_language(group, checked),
            ["auto", group] if known_group(group) => self.settings.set_auto(group, checked),
            ["tool", group, tool] if known_group(group) => {
                self.settings.set_tool(group, tool, checked)
            }
            ["program", tool] if tools::tool_by_id(tool).is_some() => self
                .settings
                .set_program(tool, event.text().unwrap_or_default()),
            _ => return false,
        }
        true
    }
}

impl Plugin for Lint {
    fn initialize(&mut self, _: &Host, params: &InitializeParams) {
        self.data_dir = params.data_dir.clone();
        self.settings = Settings::load(&self.data_dir);
        self.french = params.settings.locale.starts_with("fr");
        if let Some(dir) = params.workspace.first() {
            // A file in it: the tools are looked for from there up.
            self.near = dir.join("_");
        }
    }

    fn panel_opened(&mut self, host: &Host, panel: &str) {
        if panel == SETTINGS {
            host.render(SETTINGS, self.view());
        }
    }

    fn ui_event(&mut self, host: &Host, event: &UiEvent) {
        if event.panel != SETTINGS || !self.apply(event) {
            return;
        }
        if self.settings.save(&self.data_dir).is_err() {
            host.error(t(self.french, "not_saved"));
        }
        // A field is not drawn again at each key typed.
        if !event.action.starts_with("program:") {
            host.render(SETTINGS, self.view());
        }
    }

    fn settings_changed(&mut self, host: &Host, settings: &EditorSettings) {
        self.french = settings.locale.starts_with("fr");
        host.render(SETTINGS, self.view());
    }

    fn lint(&mut self, _: &Host, params: &LintParams) -> Result<LintResult, String> {
        Ok(self.lint_file(params))
    }

    fn fix(&mut self, _: &Host, params: &FixParams) -> Result<FixResult, String> {
        Ok(self.fix_file(params))
    }
}

fn main() -> std::io::Result<()> {
    run(Lint::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(action: &str, value: serde_json::Value) -> UiEvent {
        UiEvent {
            panel: SETTINGS.into(),
            action: action.into(),
            value: Some(value),
        }
    }

    #[test]
    fn json_is_checked_as_typed_and_a_language_turned_off_is_not() {
        let mut lint = Lint::new();
        let params = LintParams {
            path: PathBuf::from("/tmp/none/a.json"),
            text: "{\"a\": }".into(),
            dirty: true,
            trusted: false,
        };
        let found = lint.lint_file(&params);
        assert_eq!(found.diagnostics.len(), 1);
        assert_eq!(found.ran, vec!["JSON".to_string()]);
        assert!(found.language_server);
        assert!(lint.apply(&event("lang:json", json!(false))));
        assert_eq!(lint.lint_file(&params), LintResult::default());
        // A file of no language is left to the editor.
        let other = LintParams {
            path: PathBuf::from("/tmp/none/a.zzz"),
            ..params
        };
        assert_eq!(lint.lint_file(&other), LintResult::default());
    }

    #[test]
    fn the_settings_page_changes_the_settings() {
        let mut lint = Lint::new();
        assert!(lint.apply(&event("auto:py", json!(false))));
        assert!(lint.apply(&event("tool:py:mypy", json!(true))));
        assert!(lint.apply(&event("program:mypy", json!("/opt/mypy"))));
        assert_eq!(lint.settings.programs["mypy"], "/opt/mypy");
        assert!(lint.settings.tools["py"].contains(&"mypy".to_string()));
        assert!(!lint.apply(&event("tool:nothing:x", json!(true))));
        assert!(!lint.apply(&event("other", json!(true))));
        // The page shows the tools of a language chosen by hand.
        let page = serde_json::to_string(&lint.view()).unwrap();
        assert!(page.contains("tool:py:mypy") && page.contains("program:mypy"));
        assert!(!page.contains("tool:js:eslint"));
    }

    #[test]
    fn a_fix_says_nothing_when_nothing_changes() {
        let lint = Lint::new();
        let params = FixParams {
            path: PathBuf::from("/tmp/none/a.json"),
            text: "{}".into(),
            dirty: false,
            trusted: false,
            level: simpleedit_plugin::lint::Level::All,
            source: None,
        };
        let fixed = lint.fix_file(&params);
        assert_eq!(fixed.text, None);
        assert!(fixed.language_server);
    }
}
