//! What the linters need of the system: where programs are, and where the
//! project of a file starts.

use std::path::{Path, PathBuf};

/// The folders of a project that hold its own programs. They run only when
/// the editor says the project is trusted.
pub const LOCAL_BINS: [&str; 4] = ["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"];

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Folders where programs are often installed but which a program started
/// from a launcher may not have in its `PATH`.
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home() {
        for sub in [".cargo/bin", ".local/bin", "go/bin", ".npm-global/bin"] {
            dirs.push(home.join(sub));
        }
    }
    for fixed in ["/opt/homebrew/bin", "/usr/local/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(fixed));
    }
    dirs
}

/// A program on the `PATH` (or in the usual folders).
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(extra_bin_dirs())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "Gemfile",
    "CMakeLists.txt",
    "Makefile",
    "mix.exs",
    "pubspec.yaml",
    "Package.swift",
    "deno.json",
    "tsconfig.json",
];

/// The folder that holds the project of `file`: the nearest parent with a
/// project marker, or the file's own folder.
pub fn project_root(file: &Path) -> Option<PathBuf> {
    let home = home();
    let start = file.parent()?;
    let marked = start
        .ancestors()
        .take_while(|dir| Some(*dir) != home.as_deref() && dir.parent().is_some())
        .find(|dir| PROJECT_MARKERS.iter().any(|m| dir.join(m).exists()));
    Some(marked.unwrap_or(start).to_path_buf())
}
