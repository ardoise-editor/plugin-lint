# Lint — an Ardoise Editor plugin

The problems that linters find in a file, shown in
[Ardoise Editor](https://github.com/ardoise-editor/ardoise) as you type, with
their fixes: ESLint, ruff, flake8, pycodestyle, pylint, mypy, Clippy, go vet,
golangci-lint, cppcheck, ShellCheck, stylelint, yamllint, markdownlint,
RuboCop, PHP, luacheck, ktlint, SwiftLint, SQLFluff, HLint, xmllint, and JSON
itself. The editor adds what its language servers find.

Install it from **Preferences › Plugins** in Ardoise Editor. Its settings are
there too: the languages looked at, the tools of each one (those found ready
in the project by default), and the path of a program that is not found by
itself. A project's own programs (`node_modules/.bin`, a virtualenv) run only
once the folder is trusted.

## Build

```sh
cargo test
cargo build --release && mkdir -p bin && cp target/release/lint bin/
```

Then **Preferences › Plugins › Link a folder…** on this folder to try it.

## Release

Set the version in `plugin.json` and `Cargo.toml`, then push the tag
`vX.Y.Z`. The release is signed with the key of Ardoise Editor's releases
(`UPDATE_SIGNING_KEY`, in the `release` environment of this repository).

## License

MIT
