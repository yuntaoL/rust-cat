# Contributing to rust-cat

Thank you for your interest in improving `rcat` / rust-cat!

This document explains how to build, test, and contribute, including how the extensibility system is designed to work.

## Quick Start (Development)

Prerequisites:
- Rust 1.85+ (2024 edition)
- A good terminal (iTerm2, WezTerm, Kitty, Ghostty, Alacritty recommended)

```bash
git clone https://github.com/yuntaolu/rust-cat.git
cd rust-cat
just check          # or cargo check --workspace
just run README.md  # or cargo run --bin rcat -- README.md
```

See the [justfile](justfile) for the full set of convenient recipes (`just lint`, `just test`, `just fmt`, etc.).

## Project Structure

See the high-level design in `docs/plan.md` (or the authoritative session copy during active development).

Key crates (workspace):
- `crates/rcat` — thin binary entrypoint
- `crates/rcat-cli` — clap definitions, config, orchestration
- `crates/rcat-core` — `FileInfo`, detection, `Viewer` trait, errors
- `crates/rcat-tui` — Ratatui app, event loop, layout, key handling
- `crates/rcat-viewers-text` / `rcat-viewers-hex` — the two built-in viewers
- `crates/rcat-common` — small shared utilities

## Adding a New Built-in Viewer (Compile-time)

1. Create a new crate under `crates/` (or add to an existing one for small viewers).
2. Implement the `FileViewer` trait from `rcat-core`.
3. Register it in the viewer registry (see `rcat-core/src/viewer.rs`).
4. Add the crate to the workspace `Cargo.toml` members.
5. Add feature flag if optional.
6. Document the new viewer in README and tests.

The `Viewer` trait + priority system is deliberately the stable extension boundary.

## External Command Plugins (Runtime)

`rcat` discovers executables named `rcat-viewer-*` or `rcat-plugin-*` next to the `rcat` binary and in `~/.config/rcat/plugins/`.

The v1 JSON protocol (stdin/stdout, one request per process) is documented in **[docs/plugins.md](docs/plugins.md)**.

Capabilities: `can_handle`, `dump`, `render_lines` (plus `advance_lines` / `status` for TUI scrolling).

Reference plugin: `crates/rcat-viewers-json` → `rcat-viewer-json`.

## Code Formatting

We enforce consistent code formatting using `rustfmt` with the settings defined in [rustfmt.toml](rustfmt.toml).

**Before committing any code**, always run:

```bash
just fmt
# or equivalently
cargo fmt --all
```

The CI pipeline includes a dedicated **Format** job that runs `cargo fmt --all -- --check`. Any PR that fails this check will be blocked.

### Why strict formatting matters

- Reduces noise in code reviews (no more "formatting nits" comments).
- Prevents the all-too-common situation where good contributions are blocked purely because of formatting drift.
- Makes the codebase feel polished and professional.

**Tip**: Configure your editor to run `cargo fmt` on save. Many people also set up a pre-commit hook (see the [justfile](justfile) for convenient recipes).

## Code Style & Quality

- Run `just lint` (or `cargo fmt -- --check` + `cargo clippy -- -D warnings`) before every PR.
- Prefer small, pure functions.
- All public APIs should have docs.
- Add tests for new detection logic, offset math, and rendering helpers.
- Large-file behavior and UTF-8 edge cases are first-class concerns.

## Commit & PR Guidelines

- Use conventional commits when possible (`feat:`, `fix:`, `chore:`, `docs:`).
- Keep PRs focused. One logical change per PR.
- Update `CHANGELOG.md` (or let the release automation handle it) for user-visible changes.
- If your change affects keyboard behavior or UX, update the help text and README.

## Reporting Issues

Use the GitHub issue templates:
- Bug report
- Feature request
- "New viewer" proposal (very welcome!)

For security issues involving opening untrusted binaries, please use the process in `SECURITY.md`.

## License

By contributing, you agree that your contributions will be licensed under the MIT license that covers the project.

---

Thank you for helping make file inspection in the terminal delightful!
