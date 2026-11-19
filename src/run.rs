//! Running a linter on a file.

use crate::parse;
use crate::tools::{Format, Tool};
use ardoise_plugin::lint::{apply, fixes_at, Diagnostic, Level};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// What a run is about.
#[derive(Debug, Clone)]
pub struct Job {
    pub path: PathBuf,
    /// The text as typed.
    pub text: String,
}

/// The shell a script is written for, from its first line.
fn shell_of(text: &str) -> &'static str {
    let first = text.lines().next().unwrap_or("");
    if first.contains("dash") {
        "dash"
    } else if first.contains("ksh") {
        "ksh"
    } else if first.starts_with("#!") && (first.ends_with("/sh") || first.ends_with(" sh")) {
        "sh"
    } else {
        "bash"
    }
}

fn project_dir(path: &Path, tool: &Tool) -> PathBuf {
    if tool.in_file_dir {
        return path.parent().map(Path::to_path_buf).unwrap_or_default();
    }
    if tool.format == Format::Clippy {
        if let Some(dir) = path
            .ancestors()
            .skip(1)
            .find(|d| d.join("Cargo.toml").is_file())
        {
            return dir.to_path_buf();
        }
    }
    // The tools that take their setup from where they run (ESLint's flat
    // config) are run from the folder that holds it.
    if let Some(dir) = crate::tools::config_dir(tool, path) {
        return dir;
    }
    crate::sys::project_root(path)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn arguments(args: &[&str], job: &Job) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("{file}", &job.path.to_string_lossy())
                .replace("{shell}", shell_of(&job.text))
        })
        .collect()
}

/// Runs `program` with `args`, the text on its input if `text` is given.
async fn exec(
    program: &Path,
    args: &[String],
    dir: &Path,
    text: Option<&str>,
    timeout: u64,
) -> Result<(String, String), String> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .stdin(if text.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if let (Some(text), Some(mut stdin)) = (text, child.stdin.take()) {
        let text = text.to_string();
        // Written on its own: a tool that answers before reading it all
        // would block a write done first.
        tokio::spawn(async move {
            let _ = stdin.write_all(text.as_bytes()).await;
        });
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| "timeout".to_string())?
    .map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// The first useful line of what a tool complained about.
fn complaint(stderr: &str, stdout: &str) -> String {
    stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or_else(
            || "unreadable output".to_string(),
            |l| l.chars().take(300).collect(),
        )
}

/// The problems `tool` finds in the job's file.
pub async fn run_tool(tool: &Tool, program: &Path, job: &Job) -> Result<Vec<Diagnostic>, String> {
    if tool.format == Format::Json {
        return Ok(parse::json(&job.text, tool.name));
    }
    let args = arguments(tool.args, job);
    let dir = project_dir(&job.path, tool);
    let (stdout, stderr) = exec(
        program,
        &args,
        &dir,
        tool.stdin.then_some(job.text.as_str()),
        tool.timeout,
    )
    .await?;
    let (text, name) = (job.text.as_str(), tool.name);
    let file_name = job
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let both = format!("{stdout}\n{stderr}");
    let parsed = match tool.format {
        Format::Eslint => parse::eslint(&stdout, text, name),
        Format::Ruff => parse::ruff(&stdout, text, name),
        Format::Flake8 => Some(parse::flake8(&stdout, text, name)),
        Format::Pylint => parse::pylint(&stdout, text, name),
        Format::Mypy => Some(parse::mypy(&stdout, text, name, &job.path)),
        Format::ShellCheck => parse::shellcheck(&stdout, text, name),
        // The report goes to either output, by version.
        Format::Stylelint => {
            parse::stylelint(&stdout, text, name).or_else(|| parse::stylelint(&stderr, text, name))
        }
        Format::Rubocop => parse::rubocop(&stdout, text, name),
        Format::YamlLint => Some(parse::yamllint(&stdout, text, name)),
        Format::MarkdownLint => Some(parse::markdownlint(&both, text, name)),
        Format::Cppcheck => Some(parse::cppcheck(&both, text, name)),
        Format::GoVet => Some(parse::govet(&both, text, name, &file_name)),
        Format::GolangCi => parse::golangci(&stdout, text, name, &file_name),
        Format::Php => Some(parse::php(&both, text, name)),
        Format::Luacheck => Some(parse::luacheck(&stdout, text, name)),
        Format::Ktlint => parse::ktlint(&stdout, text, name),
        Format::Swiftlint => parse::swiftlint(&stdout, text, name),
        Format::SqlFluff => parse::sqlfluff(&stdout, text, name),
        Format::Hlint => parse::hlint(&stdout, text, name),
        Format::Xmllint => Some(parse::xmllint(&both, text, name)),
        Format::Clippy => Some(parse::clippy(&stdout, text, name, &job.path)),
        Format::Json => unreachable!("checked above"),
    };
    let mut found = parsed.ok_or_else(|| complaint(&stderr, &stdout))?;
    // A file the configuration does not cover is not a clean file.
    if tool.format == Format::Eslint && found.is_empty() {
        if let Some(i) = stdout.find("File ignored") {
            let notice: String = stdout[i..].chars().take_while(|c| *c != '"').collect();
            return Err(notice);
        }
    }
    // Printed twice (to both outputs) or reported twice: shown once.
    let mut seen = std::collections::HashSet::new();
    found.retain(|d| seen.insert((d.line, d.col, d.message.clone())));
    Ok(found)
}

/// The text with every fix of `tool` applied, if the tool can print it.
pub async fn fix_all(tool: &Tool, program: &Path, job: &Job) -> Result<Option<String>, String> {
    let Some(args) = tool.fix_all else {
        return Ok(None);
    };
    let args = arguments(args, job);
    let (stdout, stderr) = exec(
        program,
        &args,
        &project_dir(&job.path, tool),
        Some(&job.text),
        tool.timeout,
    )
    .await?;
    // RuboCop prints its report, a line of equal signs, then the code.
    let fixed = match stdout.split_once("====================\n") {
        Some((_, code)) => code.to_string(),
        None => stdout,
    };
    if fixed.is_empty() && !job.text.is_empty() {
        return Err(complaint(&stderr, ""));
    }
    Ok(Some(fixed))
}

/// Every problem found for a job by `tools`, and the complaints of those
/// that failed (`name: what`). They run one after the other: a project's
/// tools share its caches and locks.
pub async fn all(tools: Vec<(&'static Tool, PathBuf)>, job: Job) -> (Vec<Diagnostic>, Vec<String>) {
    let mut found = Vec::new();
    let mut failed = Vec::new();
    for (tool, program) in &tools {
        match run_tool(tool, program, &job).await {
            Ok(mut list) => found.append(&mut list),
            Err(e) => failed.push(format!("{}: {e}", tool.name)),
        }
    }
    found.sort_by_key(|d| (d.line, d.col));
    (found, failed)
}

/// `text` with the problems fixed that `level` covers by the tools: those
/// found again in the text, then what the tools that print the fixed file
/// give. `source` limits it to the problems of one tool. Also says whether
/// the language server may fix the rest (not when one tool was asked).
pub async fn fix(
    tools: Vec<(&'static Tool, PathBuf)>,
    lsp: bool,
    path: &Path,
    text: &str,
    level: Level,
    source: Option<&str>,
) -> (String, bool) {
    let mut now = text.to_string();
    // A tool named by the source, or all of them.
    let tools: Vec<_> = tools
        .into_iter()
        .filter(|(tool, _)| source.is_none_or(|s| s == tool.name))
        .collect();
    let by_a_tool =
        source.is_some_and(|s| !tools.is_empty() && tools.iter().all(|(t, _)| t.name == s));
    let job = Job {
        path: path.to_path_buf(),
        text: now.clone(),
    };
    let (found, _) = all(tools.clone(), job).await;
    now = apply(&now, &fixes_at(&found, level)).0;
    // Those that print the fixed file fix everything, warnings too: only
    // when that is what is asked for.
    if level != Level::Errors {
        for (tool, program) in &tools {
            let job = Job {
                path: path.to_path_buf(),
                text: now.clone(),
            };
            if let Ok(Some(fixed)) = fix_all(tool, program, &job).await {
                now = fixed;
            }
        }
    }
    (now, lsp && !by_a_tool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{plan, program_path, tool_by_id};

    #[test]
    fn a_script_names_its_shell() {
        assert_eq!(shell_of("#!/bin/sh\necho"), "sh");
        assert_eq!(shell_of("#!/usr/bin/env bash\n"), "bash");
        assert_eq!(shell_of("#!/bin/dash\n"), "dash");
        assert_eq!(shell_of("echo hi\n"), "bash");
    }

    #[tokio::test]
    async fn json_is_checked_in_the_program() {
        let job = Job {
            path: PathBuf::from("/x/a.json"),
            text: "{\"a\": }".into(),
        };
        let tool = tool_by_id("json").unwrap();
        let found = run_tool(tool, Path::new(""), &job).await.unwrap();
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    async fn ruff_is_run_on_the_text_as_typed() {
        let Some(ruff) =
            tool_by_id("ruff").and_then(|t| program_path(t, Path::new("/x/a.py"), None, false))
        else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let job = Job {
            path: dir.path().join("a.py"),
            text: "import os\nprint(undefined_name)\n".into(),
        };
        let tool = tool_by_id("ruff").unwrap();
        let found = run_tool(tool, &ruff, &job).await.unwrap();
        assert!(
            found.iter().any(|d| d.code.as_deref() == Some("F401")),
            "{found:?}"
        );
        let fixes = fixes_at(&found, Level::All);
        let (fixed, _) = apply(&job.text, &fixes);
        assert!(!fixed.contains("import os"), "{fixed}");
    }

    #[tokio::test]
    async fn a_missing_program_is_an_error_not_a_crash() {
        let job = Job {
            path: PathBuf::from("/x/a.sh"),
            text: "echo\n".into(),
        };
        let tool = tool_by_id("shellcheck").unwrap();
        let result = run_tool(tool, Path::new("/nowhere/shellcheck"), &job).await;
        assert!(result.is_err());
    }

    /// With `LINT_SANDBOX` set to a folder holding a TypeScript project with
    /// eslint installed (`a.ts` in it), the real tool is run.
    #[tokio::test]
    async fn eslint_finds_and_fixes_the_problems_of_a_real_project() {
        let Some(dir) = std::env::var_os("LINT_SANDBOX").map(PathBuf::from) else {
            return;
        };
        let path = dir.join("a.ts");
        let text = std::fs::read_to_string(&path).unwrap();
        let job = Job {
            path: path.clone(),
            text: text.clone(),
        };
        let tools = || plan(&path, "ts", None, &Default::default(), true).tools;
        let (found, failed) = all(tools(), job).await;
        assert!(failed.is_empty(), "{failed:?}");
        assert!(
            found
                .iter()
                .any(|d| d.code.as_deref() == Some("comma-spacing")),
            "{found:?}"
        );
        let (fixed, n) = apply(&text, &fixes_at(&found, Level::All));
        assert!(n >= 2 && fixed.contains("[1, 2]"), "{fixed}");
        // Fixing one tool's problems leaves the language server out.
        let (eslint_only, server) =
            fix(tools(), true, &path, &text, Level::Errors, Some("ESLint")).await;
        assert!(eslint_only.contains("[1, 2]") && !server, "{eslint_only}");
        let (_, server) = fix(tools(), true, &path, &text, Level::Errors, None).await;
        assert!(server);
    }

    /// With `LINT_MONO` set to a monorepo whose ESLint and configuration are
    /// at its root (a `package.json` in the app's folder too), a `.jsx` of the
    /// app is checked.
    #[tokio::test]
    async fn eslint_at_the_root_of_a_monorepo_checks_a_jsx_of_an_app() {
        let Some(dir) = std::env::var_os("LINT_MONO").map(PathBuf::from) else {
            return;
        };
        let path = dir.join("apps/web/src/components/a.jsx");
        let text = std::fs::read_to_string(&path).unwrap();
        let plan = plan(&path, "jsx", None, &Default::default(), true);
        assert_eq!(plan.tools.len(), 1, "eslint is found from the root");
        let (found, failed) = all(plan.tools, Job { path, text }).await;
        assert!(failed.is_empty(), "{failed:?}");
        let codes: Vec<_> = found.iter().filter_map(|d| d.code.clone()).collect();
        assert!(
            codes.contains(&"semi".to_string()) && codes.contains(&"no-undef".to_string()),
            "{codes:?}"
        );
    }
}
