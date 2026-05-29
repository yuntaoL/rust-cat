# Development Guide for rust-cat

This is the local development handbook.

## Environment

- Rust 1.85+ (edition 2024)
- Recommended terminals for full Unicode + color: Ghostty, Kitty, WezTerm, iTerm2, Alacritty
- `just` (highly recommended for DX)
- `cargo-watch` (optional but nice): `cargo install cargo-watch`

## Common Commands

Use `just` (see root justfile):

```bash
just check
just lint
just test
just coverage          # Generate HTML coverage report
just run some-file.txt
just run -- --hex -o 0x200 firmware.bin
```

Without just:

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo fmt -- --check
cargo test --workspace
cargo run --bin rcat -- README.md
```

### Code Coverage

We use `cargo-llvm-cov` for coverage reporting.

```bash
just coverage          # Generates HTML report in target/llvm-cov/html/
```

In CI we enforce a minimum line coverage (currently **50%**, planned to increase to **75%**).

To run the CI-style check locally:

```bash
just coverage-check
```

## Architecture Notes

See the full design in the plan document (session `plan.md` during active development, or `docs/plan.md` snapshot in the repo).

Core principles:
- Unified byte offset as the single source of truth for navigation.
- `memmap2` for all file-backed viewing (zero-copy, OS paging).
- Ratatui best practices: pure `update(Action)` + pure `render(&Frame, &App)`.
- `Viewer` trait is the primary extension point.

## Large File Testing

Create test fixtures:

```bash
# 100MB random file
dd if=/dev/urandom of=tests/fixtures/large-random.bin bs=1m count=100

# Text log
yes "this is a very long line of text for scrolling tests" | head -n 500000 > tests/fixtures/large-log.txt
```

Run with `just run-release tests/fixtures/large-*.bin` and verify startup < ~150ms and smooth scrolling.

## Debugging the TUI

**Important TUI safety rule**: When you use `--log-file` (or `RCAT_LOG_FILE`), rcat and all plugins switch to **file-only logging**. Nothing is written to stderr at all. This guarantees the TUI (which takes over the terminal with raw mode + alternate screen) can never be corrupted by log output.

- Logs from the **host + external plugins** (rcat-viewer-*) are merged into the same file.
- Recommended workflow:
  ```bash
  # Terminal 1
  RCAT_LOG=debug rcat --log-file /tmp/rcat.log some.json

  # Terminal 2 — watch everything (host + JSON plugin etc.) live
  tail -f /tmp/rcat.log
  ```
- Without a log file: normal stderr-only logging (classic behavior).
- `RCAT_LOG=debug cargo run -- README.md`
- `-v` / `-vv` still works to increase verbosity when no `RCAT_LOG` is set.
- Plugins respect the same rules.

Use `better-panic` or `color-backtrace` on panic in debug builds. Resize the terminal frequently while developing.

## Adding Dependencies

Prefer workspace-level dependencies in root `Cargo.toml`.

Keep the dependency graph small in v0.1. Heavy crates (syntect, tree-sitter, image decoders) go behind feature flags.

## Release Process (Maintainers)

See Section 13 of the plan and the release GitHub workflow.

## Questions?

Open a Discussion or issue, or ping the maintainer.
