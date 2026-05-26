# rust-cat: High-Level Design & Implementation Plan

> **This is the canonical, authoritative plan for the project.**  
> It is maintained in `docs/plan.md` inside the repository and should be kept up to date.  
> During active development a working copy may also exist in the AI session folder, but **this file in the repo is the one that ships with the code**.

**Project**: `rust-cat` (binary name: `rcat`)  
**Location**: `/Users/yuntaolu/dev/rust/rust-cat`  
**Date**: 2026-05-26 (last major update)  
**Status**: Implementation in progress — Phase 0 complete (first commit done). Moving into Phase 1.

## Quick Navigation

- [Program Definition & Scope](#2-program-definition-what-we-are-building)
- [Architecture Overview](#3-high-level-architecture)
- **[Implementation Phases & Detailed Content (the main plan)](#8-implementation-phases--milestones)**
- [Verification Strategy](#9-verification--quality-strategy)
- [GitHub & DevEx Requirements](#13-git-github-hosting-and-development-experience-added-per-review-feedback)
- [Progress Log](#14-progress-log-implementation)

---

## Phases at a Glance (see Section 8 for full details)

| Phase | Focus                              | Est. Effort |
|-------|------------------------------------|-------------|
| 0     | Setup, workspace, first working `rcat --version` binary, GitHub scaffolding | 1–2 days |
| 1     | Core foundations (`FileInfo`, detection, `Viewer` trait) | 3–5 days |
| 2     | TUI shell + event loop + first viewer | 4–6 days |
| 3     | Second viewer + unified navigation + polish | 4–6 days |
| 4     | UX, colors, metadata sidebar, help, error handling | 3–4 days |
| 5     | Extensibility hooks, config, release packaging | 3–5 days |

**Total for v0.1**: ~3–5 weeks focused work.

---

## 1. Context & Motivation

The current directory (`/Users/yuntaolu/dev/rust/rust-cat`) is a fresh, empty folder. The user wants a purpose-built Rust command-line application that replaces the mental model of juggling `cat`, `less`, `xxd`/`hexyl`, and ad-hoc tools when inspecting files.

**Core idea**: One fast, beautiful, keyboard-driven tool that:
- Shows **text** files with excellent paging (the `less` experience).
- Shows **binary** files with a professional hex + ASCII view (the `hexyl` experience) by default.
- Is **natively extensible** so new file types (images, ELF, JSON, archives, custom binary formats, etc.) can add rich views, metadata panels, and inspectors without forking the core.

This aligns with the user's existing Rust workspace patterns (see `rustai-forge`) and modern 2025–2026 Rust TUI ecosystem (Ratatui + crossterm).

**Why now?**
- Ratatui 0.30 + crossterm 0.29 provide a mature, high-performance TUI foundation.
- `heh` crate (embeddable Ratatui hex editor widget) dramatically reduces the cost of a high-quality hex view.
- `memmap2` makes safe, zero-copy handling of multi-GB files straightforward.
- Community consensus favors clean trait-based modularity + external-command plugins first, with WASM plugins as a future power-user path.

---

## 2. Program Definition (What We Are Building)

### 2.1 Name & Identity
- **Workspace / crate family**: `rust-cat`
- **Binary**: `rcat` (short, memorable, follows `bat`/`hexyl` naming tradition)
- **Tagline**: "A modern, extensible file viewer for the terminal — text, hex, and beyond."

### 2.2 Primary Use Cases
1. **Daily inspection** — `rcat README.md`, `rcat /bin/ls`, `rcat large.log`
2. **Binary forensics / reverse engineering** — quick hex navigation + data inspector + later architecture-specific views (ELF sections, Mach-O, PE, etc.)
3. **Learning / teaching** — beautiful, color-coded hex + ASCII that is easy to reason about
4. **Scripting / CI** — non-interactive mode that behaves like `cat` or `xxd` when piped or when `--stdout` is passed
5. **Extensibility experiments** — add support for new formats (e.g., a `rcat-png` external viewer or later a WASM plugin that renders image previews as sixel/kitty + metadata)

### 2.3 Scope — MVP (v0.1) vs Future

**v0.1 Goals (shippable, delightful core experience)**
- Single file argument (multiple files and tabs are v0.2+)
- Two built-in viewers: **Text** (UTF-8 with graceful fallback) and **Hex+ASCII** (16-byte rows, color-coded like hexyl)
- Seamless toggle between Text ↔ Hex with unified byte-offset navigation model
- Full keyboard navigation (arrows + PageUp/Down + Home/End + vim-style `j/k/gg/G` + less-style)
- Interactive TUI by default when stdout is a tty
- Non-interactive streaming dump when piped or `--stdout`
- Correct handling of huge files (≥1 GB) with low memory usage
- Clean status bar, position percentage, filename, mode indicator
- Basic metadata sidebar (size, detected type, magic signature, simple stats)
- Graceful degradation on invalid UTF-8 / control characters
- Proper terminal state restore on panic/exit

**Explicit Non-Goals for v0.1**
- Syntax highlighting (syntect is heavy; add behind feature flag in v0.2+)
- In-place editing (hex editor mode is future work)
- Image / PDF / archive rendering (extension territory)
- Mouse support (nice-to-have, not required for v0.1)
- Search / goto dialog (can land early in v0.2 if easy)
- Config file / keybinding customization (design the hooks, implement in v0.2)

**Roadmap Highlights (after v0.1)**
- v0.2: Search (`/`), goto (`g`), multiple files / buffer list, horizontal scroll or wrap toggle, external command plugins (first-class extensibility)
- v0.3: WASM plugin host (Extism or Wasmtime + Component Model), richer metadata inspectors, data type overlays in hex view
- Later: sixel/kitty image previews, structured binary parsers (via plugins), integration with `bat` styles, etc.

### 2.4 User Experience Principles
- **One mental model**: Everything is bytes + a viewport over those bytes. Text mode is just a decoder + line layout on top of the same byte cursor.
- **Zero surprises on large files**: Startup < 100 ms even on 2 GB files; scrolling feels instant.
- **Keyboard first, discoverable**: Every important action has a visible hint or `?` help overlay.
- **Pipe-friendly**: `rcat file | head` and `some-tool | rcat` should "just work" without TUI artifacts.
- **Extensibility is a first-class feature**, not an afterthought.

---

## 3. High-Level Architecture

### 3.1 Core Abstraction: The `Viewer` Trait

```rust
pub trait FileViewer: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_handle(&self, info: &FileInfo) -> ViewerPriority; // None / Low / Normal / Preferred
    fn render(&mut self, ctx: &RenderContext) -> Result<()>;
    fn metadata_panel(&self, info: &FileInfo) -> Option<MetadataPanel>;
    // future: handle_input, search, etc.
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewerPriority { None, Low, Normal, Preferred }
```

- Built-in: `TextViewer`, `HexViewer`
- Later plugins register additional implementations (via registry or dynamic loading)
- The TUI holds a `Vec<Box<dyn FileViewer>>` or an enum-dispatch equivalent and selects the highest-priority handler (or lets the user force via `--text` / `--hex`).

### 3.2 Unified Navigation Model (Critical Decision)
**Single source of truth**: byte `offset: u64` + viewport height/width.

- Text mode derives the visible line range from the byte offset (using a line-start index built once at load).
- Hex mode derives the visible 16-byte rows directly from the offset.
- Switching modes preserves the user's position (you stay "looking at the same bytes").
- This makes large-file handling and extensibility dramatically simpler.

### 3.3 File Handling Strategy
- Always use `memmap2::Mmap` (read-only) for the backing store.
- `FileInfo` struct contains: path, size, detected MIME/magic/extension, is_text_heuristic, etc.
- Text detection: `content_inspector` crate + extension hints + BOM/UTF-8 validity sampling. Never trust extension alone.
- For text: build a `Vec<u64>` of line-start byte offsets (one pass, O(n) but fast and done once). Enables O(1) "what line is at byte X?" queries.
- Streaming / stdin support: fall back to a limited in-memory buffer or temp file when input is not a seekable file.

### 3.4 TUI Architecture (Ratatui Best Practices)
Follow the widely recommended pattern from the Ratatui community (2025 discussions):

```
App (owns state: current offset, active viewer, file mmap, layout, search state, ...)
  ├── update(Action)          // pure state transitions
  └── render(&mut Frame)      // pure, called every frame

Action enum                     // all possible state changes (key, resize, tick, ...)
Event loop (crossterm) → Action
```

Components / widgets:
- Header bar (filename, size, mode, %)
- Main content area (delegates to active `Viewer::render`)
- Optional right sidebar (metadata + data inspector)
- Footer / key hint bar
- Modal overlays (`?` help, goto dialog, search)

**Hex rendering options (recommend evaluating both):**
1. Depend on `heh` crate (0.6+) as an embedded widget — already a high-quality, color-coded, keyboard-navigable hex view. Binsider uses it successfully.
2. Custom `HexWidget` (more control, simpler dependency graph, easier to add custom overlays later).

Recommendation for v0.1: Start with a thin custom widget (inspired by hexyl + heh), then decide whether to adopt heh after seeing the integration cost. This keeps the core dependency surface small initially.

### 3.5 Two Execution Paths

**Interactive TUI Path** (default on tty)
- Full ratatui + crossterm event loop
- 60 fps target, immediate-mode rendering

**Non-interactive / Dump Path**
- Detect when stdout is not a tty or `--stdout` flag is present
- Text mode → write lines (respecting `--offset` / `--length`)
- Hex mode → classic `xxd`-style or hexyl-style output
- No terminal manipulation, no alternate screen

### 3.6 Extensibility Strategy (Phased — Very Important)

**Phase 1 (v0.1–v0.2)**: Compile-time + registration
- `Viewer` trait lives in `rcat-core`
- New viewers added as workspace crates behind feature flags or just additional `mod` entries
- Simple registry (static `inventory` crate or explicit `register_viewer!` macro)

**Phase 2 (v0.2+)**: External command plugins (recommended first runtime extension mechanism)
- Discover executables named `rcat-*` in `PATH` or `~/.config/rcat/plugins/`
- Simple JSON protocol over stdin/stdout:
  - `can_handle(file_path, magic, size)` → priority + display name
  - `render_text(file, offset, height, width)` → lines + cursor info
  - `metadata(file)` → key/value tree
- Extremely safe (process isolation), any language, easy to debug, matches the `git-*` / `cargo-*` pattern that Rust users already understand.
- This is the pragmatic winner for most CLI tools in 2025–2026.

**Phase 3 (later, power users)**: Sandboxed WASM plugins
- Use **Extism** (best DX) or **Wasmtime + WebAssembly Component Model + WIT** (most future-proof, capability-based security via WASI).
- Enables custom parsers, rich inspectors, even small custom widgets (if we expose a drawing protocol).
- Avoid raw `libloading` / dylibs for user-facing plugins — unstable ABI + security disaster.

**Design principle**: The `Viewer` trait + `RenderContext` should be the stable boundary. External plugins talk to a small host adapter that implements the trait on their behalf.

---

## 4. Recommended Project Structure (Workspace)

Following the user's existing workspace habits while keeping the design clean for a real CLI tool:

```
rust-cat/
├── Cargo.toml                      # workspace root
├── README.md
├── docs/
│   └── design.md                   # (this doc can evolve here too)
├── crates/
│   ├── rcat/                       # binary crate (thin main.rs)
│   │   └── src/
│   │       └── main.rs
│   ├── rcat-cli/                   # clap definitions, config loading, entry orchestration
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── args.rs
│   │       └── config.rs
│   ├── rcat-core/                  # domain model, detection, Viewer trait, errors
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── file_info.rs
│   │       ├── viewer.rs           # the trait + registry
│   │       └── detection.rs
│   ├── rcat-tui/                   # ratatui application, event loop, layout, App state
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── app.rs
│   │       ├── action.rs
│   │       ├── ui/
│   │       │   ├── mod.rs
│   │       │   ├── header.rs
│   │       │   ├── content.rs
│   │       │   └── sidebar.rs
│   │       └── keymap.rs
│   ├── rcat-viewers-text/
│   │   └── src/
│   │       └── lib.rs              # TextViewer impl
│   ├── rcat-viewers-hex/
│   │   └── src/
│   │       └── lib.rs              # HexViewer (or re-export from heh)
│   └── rcat-common/                # small shared utilities (optional, like user's "common" crate)
│       └── src/
│           └── lib.rs
├── tests/
│   └── integration/                # golden files, large-file fixtures
└── .github/                        # CI later
```

**Alternative (simpler start)**: Single `rcat` binary crate with internal modules under `src/viewers/`, `src/tui/`, etc. Split into crates only when the boundaries are proven.  
**Recommendation**: Use the multi-crate layout from day one. The user already works this way, the `Viewer` boundary is natural, and it makes the extensibility story credible.

### 4.1 Key Dependencies (Initial)

**Core**
- `clap` (v4, derive) + `clap_complete` later
- `anyhow` + `thiserror`
- `memmap2`
- `content_inspector` (or lightweight custom detector)
- `directories` (XDG config paths)
- `serde` + `toml` (config + future plugin manifests)

**TUI**
- `ratatui` (0.30) with `crossterm` backend
- `crossterm` (0.29)
- `unicode-width`, `unicode-segmentation`

**Optional / Future**
- `heh` (evaluate for hex widget)
- `syntect` (behind `syntax-highlight` feature)
- `regex` (search)
- `tracing` + `tracing-subscriber` (structured logging)
- `inventory` or custom macro for viewer registration
- Later: `extism` or `wasmtime` + `wit-bindgen`

**Avoid in v0.1**: heavy image libs, full tree-sitter, etc.

---

## 5. Detailed Component Responsibilities

### 5.1 `rcat-cli`
- Parse arguments (`clap`)
- Load config (merge CLI flags > env > TOML > defaults)
- Decide execution mode (interactive vs dump)
- Instantiate `FileInfo`, select initial viewer, hand off to TUI or dump renderer

### 5.2 `rcat-core`
- `FileInfo` + detection logic
- `Viewer` trait definition + priority system
- `RenderContext` (mmap slice for visible window, terminal size, theme, etc.)
- Error types
- Registry (static list of built-in viewers + later plugin adapters)

### 5.3 `rcat-tui`
- `App` struct (owns mmap, current offset, active viewer handle, layout state, mode)
- `Action` enum (all mutations)
- Event loop (crossterm poll + key → Action)
- Main `ui(frame, &app)` function + sub-renderers
- Keymap (hard-coded in v0.1, configurable later)
- Terminal lifecycle (enter alternate screen, raw mode, restore on drop/panic using `better-panic` or custom hook)

### 5.4 Viewers (text & hex)
- Implement `FileViewer`
- Own any per-viewer transient state (search matches, preferred column for text, cursor nibble in hex, etc.)
- Pure rendering into the provided area (they receive a `Rect` and a writer-like context or direct `Buffer` access via Ratatui)

---

## 6. Keyboard Navigation (v0.1 Target)

**Universal (both modes)**
- `q` / `Ctrl-C` — quit
- `?` — help overlay
- `Tab` or `h` — toggle Text ↔ Hex (preserve byte offset)
- `j` / `↓` — down one logical unit (line or hex row)
- `k` / `↑` — up
- `Ctrl-d` / `PageDown` — half or full page down
- `Ctrl-u` / `PageUp` — up
- `g` / `Home` — start of file
- `G` / `End` — end of file
- `m` — toggle metadata sidebar

**Hex-specific**
- `h` / `l` — move within hex/ASCII columns (nibble cursor later)
- `Ctrl-j` (or `g` in command mode) — goto byte offset (hex or decimal input)

**Text-specific**
- `0` / `^` / `$` — start / end of logical line (if we add horizontal movement)
- (Search `n` / `N` deferred to v0.2)

**Command mode (stretch for v0.1)**
- `:` opens mini command line (goto, save selection later, etc.)

---

## 7. CLI Surface (Proposed)

```
rcat [OPTIONS] <FILE>

Options:
  -H, --hex                 Force hex viewer
  -T, --text                Force text viewer
  -o, --offset <OFFSET>     Start at byte offset (decimal or 0x hex)
  -l, --length <LEN>        Limit bytes/lines rendered
      --stdout              Force non-interactive dump mode (useful in scripts)
  -c, --config <PATH>       Override config file
      --list-viewers        Print registered viewers and exit
  -v, --verbose             Increase logging
  -h, --help
  -V, --version

Examples:
  rcat README.md
  rcat /bin/ls
  rcat --hex -o 0x1000 -l 256 firmware.bin | xxd   # still useful with pipes
  some-producer | rcat                               # read from stdin (limited)
```

---

## 8. Implementation Phases & Milestones

**Phase 0 — Setup (1–2 days)**
- Workspace `Cargo.toml`, all crates with correct `workspace.package` + `workspace.dependencies`
- Basic `rcat --version` + `--help` binary
- CI skeleton (GitHub Actions: build, test, clippy, fmt on macOS + Linux)
- `.gitignore`, `rustfmt.toml`, `clippy.toml` aligned with user's preferences

**Phase 1 — Core Foundations (3–5 days)**
- `FileInfo` + detection logic with good test coverage
- `Viewer` trait + registry + two trivial stub viewers
- Non-interactive dump path for both text and hex (produces correct output for pipes)
- `memmap2` integration + basic large-file test

**Phase 2 — TUI Shell (4–6 days)**
- Ratatui + crossterm event loop skeleton (following Action/update/render pattern)
- Layout (header + main + footer)
- Basic keyboard handling + resize handling
- One working viewer (start with Hex — often easier to get right visually)

**Phase 3 — Second Viewer + Navigation Polish (4–6 days)**
- Text viewer with line index
- Unified byte-offset navigation across modes
- Toggle, scrolling, Home/End/Page keys all working smoothly
- Status bar showing position, percentage, file info

**Phase 4 — UX & Metadata (3–4 days)**
- Color scheme (inspired by hexyl + modern Ratatui themes)
- Metadata sidebar (toggleable)
- Help overlay (`?`)
- Graceful error screens (bad file, permission, etc.)
- Panic recovery + clean terminal restore

**Phase 5 — Extensibility Hooks + Release (3–5 days)**
- Document the `Viewer` trait as the extension point
- Skeleton for external command plugin discovery (even if no real plugins yet)
- Config file loading (even if only colors / defaults)
- Packaging (homebrew formula stub, cargo-dist or manual release)
- README with screenshots + keybinding cheat sheet

**Total estimated effort for a high-quality v0.1**: 3–5 weeks of focused work (can be faster with pair programming or heavy use of Ratatui recipes).

---

## 9. Verification & Quality Strategy

### 9.1 Automated
- Unit tests: detection, line indexing, offset math, hex formatting, priority selection
- Property-based tests (quickcheck/proptest) for byte offset ↔ line mapping
- Integration tests: small + medium + "large" (generated) files under `tests/fixtures/`
- Snapshot tests for non-interactive hex/text output (using `insta` crate)
- `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt -- --check`

### 9.2 Manual / UX Checklist (required before v0.1 release)
- 0-byte file
- 1-byte file
- Small text (ASCII + UTF-8 + CJK + emoji)
- Large text log (≥100 MB)
- Binary (PNG, ELF binary, random bytes, Mach-O universal binary)
- File with invalid UTF-8 sequences in "text" mode
- Permission denied / non-existent path
- Terminal resize while viewing
- Very narrow (< 40 cols) and very wide terminals
- Pipe output correctness (`rcat file | cat`)
- Stdin mode (limited but should not crash)

### 9.3 Performance Targets
- Startup + first frame on 2 GB file: < 150 ms on modern MacBook
- Scrolling (any key repeat): ≥ 60 fps perceived
- Memory: roughly constant + size of the line index (text mode) or negligible (hex)

### 9.4 Cross-Platform
- Primary: macOS (Apple Silicon)
- Secondary: Linux (Ubuntu/Debian recent)
- Tertiary: Windows (MSVC) — crossterm handles most of it; test in CI if possible

---

## 10. Open Questions & Risks (for Discussion)

1. **Hex widget**: Adopt `heh` early (saves time, proven) or build custom (more ownership, simpler deps)?  
   → Recommendation in plan: prototype a minimal custom one first, then decide.

2. **How far should v0.1 extensibility go?**  
   → Strong recommendation: solid internal trait + clear external command plugin protocol on paper. Defer actual WASM until after v0.1 ships and real user feedback arrives.

3. **Binary name** — `rcat`, `rustcat`, `catr`, or something else? (Avoid conflict with any existing popular tool.)

4. **Text wrapping vs horizontal scroll** in text mode? (Many power users prefer horizontal scroll + `h`/`l`.)

5. **Should the tool ever attempt to "fix" or pretty-print** (e.g., JSON, XML)? Or stay strictly "view what is on disk"?

6. **Config & theming surface** — how much do we expose in v0.2?

7. **Licensing & distribution** — MIT (matching user's other projects) is fine. Consider cargo-dist or homebrew for easy installs.

---

## 11. Success Criteria for v0.1

- A user can `cargo install --path .` (or from crates.io) and run `rcat <any-file>` and have a delightful experience for both text and binary files.
- The experience feels "native" and competitive with `bat` + `hexyl` + `less` combined for the common case.
- Adding a new internal viewer is a clear, documented 30–60 minute task for a contributor.
- The external plugin protocol is specified (even if only one reference implementation exists).
- No major memory or CPU surprises on large files.
- Codebase is clean, well-tested, and ready for the extensibility phases.

---

## 12. Next Steps (After Plan Approval)

1. User reviews this document and provides feedback / decisions on the open questions.
2. We iterate on the plan (edit this file) until both parties are aligned.
3. Enter implementation phase (exit plan mode).
4. Begin with Phase 0 (workspace + binary skeleton) — smallest possible vertical slice that produces a working `rcat --version`.
5. Regular checkpoint reviews (especially after Phase 2 when the first TUI loop + one viewer is running).

---

## 13. Git, GitHub Hosting, and Development Experience (Added per Review Feedback)

The project **must** be managed with Git from day zero and prepared for high-quality open-source collaboration on GitHub.

### 13.1 Repository Setup

1. **Initialize Git immediately** (part of Phase 0):
   ```bash
   git init
   git add .
   git commit -m "chore: initial project skeleton"
   ```

2. **GitHub repository**:
   - Recommended name on GitHub: `rust-cat` (matches the folder) or `rcat` (matches the binary).
   - Owner: `yuntaolu` (or the organization of choice).
   - Visibility: Start private while core is developed, then make public (or public from day 1 — recommended for visibility and contributor attraction).
   - Description: "A modern, extensible terminal file viewer — text, hex, and beyond. Written in Rust with Ratatui."
   - Topics/tags (GitHub): `rust`, `tui`, `terminal`, `cli`, `hex`, `pager`, `file-viewer`, `ratatui`, `crossterm`.

3. **Initial remote**:
   ```bash
   git remote add origin https://github.com/yuntaolu/rust-cat.git
   git push -u origin main
   ```

### 13.2 GitHub Repository Settings & Branch Protection (Recommended)

Configure these in the GitHub web UI under **Settings → General** and **Settings → Branches**:

- Enable **Issues**, **Projects** (optional), and **Discussions** (great for extensibility ideas).
- Enable **Vulnerability alerts** + **Dependabot alerts** + **Dependabot security updates**.
- **Branch protection on `main`**:
  - Require pull request reviews (minimum 1 reviewer, dismiss stale reviews).
  - Require status checks to pass before merging (at minimum: "CI / build-and-test", "Clippy", "Format").
  - Require branches to be up to date before merging.
  - Include administrators (recommended for small personal projects).
  - Restrict force pushes and branch deletion.

- **Environments** (for future releases): Create a `release` environment that requires approval or status checks before publishing crates.io / GitHub Releases.

### 13.3 Files & Tooling That Improve Development Flow

Create the following from the very first commit (Phase 0):

**Core hygiene (mandatory)**
- `.gitignore` — comprehensive Rust + editor + OS + secrets (see standard template below)
- `LICENSE` — MIT (to match user's other projects)
- `rustfmt.toml` — opinionated but consistent formatting
- `clippy.toml` — allow some lints while keeping high signal
- `.editorconfig` — consistent indentation / line endings across all editors

**Developer experience (strongly recommended)**
- `justfile` (or `Makefile`) — the single most impactful DX improvement for Rust projects. Common recipes:
  - `just build`, `just test`, `just lint`, `just fmt`, `just run <file>`, `just check`, `just doc`, `just release-check`
- GitHub Actions workflows (`.github/workflows/`):
  - `ci.yml` — cargo check + test + clippy + fmt + doc on push/PR (matrix: macOS, Linux, optional Windows)
  - `audit.yml` — weekly `cargo-deny` + `cargo-audit` + `cargo-outdated`
  - `release.yml` — triggered on version tags; uses `cargo-dist` or `release-plz` for GitHub Releases + crates.io publish (optional but professional)
- `.github/dependabot.yml` — automatic PRs for Cargo and GitHub Actions updates
- `.github/renovate.json` (alternative or complement to Dependabot)
- `.github/CODEOWNERS` (simple: `@yuntaolu` as owner for now)
- Pre-commit configuration (`.pre-commit-config.yaml`) — run rustfmt, clippy, cargo-check, typos on every commit (optional but excellent)

**Documentation & contribution (high value)**
- `CONTRIBUTING.md` — how to build, test, add a new viewer, submit PRs, and the plugin protocol
- `.github/ISSUE_TEMPLATE/`:
  - `bug_report.yml`
  - `feature_request.yml`
  - `new_viewer.yml` (specific template for proposing a new file type handler)
- `.github/PULL_REQUEST_TEMPLATE.md`
- `SECURITY.md` — responsible disclosure for a tool that opens arbitrary binary files
- `CHANGELOG.md` — Keep a Changelog format (or let `release-plz` / `git-cliff` maintain it)
- `docs/development.md` — local development guide + architecture notes

**Optional but nice (add early when convenient)**
- `typos.toml` or `codespell` configuration for documentation spelling
- `cargo-deny.toml` (deny.toml) with strict duplicate / license / advisory policy
- VS Code recommended settings + extensions (`.vscode/extensions.json`, `settings.json`) — especially useful because the user already uses `.code-workspace` files
- `benches/` directory + criterion (performance regression tracking for large files)
- `examples/` directory with small demo files

### 13.4 Recommended GitHub Actions Matrix (CI)

Minimum green checks on every PR:
- `cargo fmt -- --check`
- `cargo clippy -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo check --workspace --all-targets`
- Build on macOS (Apple Silicon) + Ubuntu latest (x86_64 + aarch64 if possible)

Future expansions:
- `cargo test -- --ignored` (large file integration tests)
- Cross-compilation checks (x86_64-unknown-linux-musl, aarch64-apple-darwin, etc.)
- `cargo install --path . --locked` smoke test
- Binary size tracking (optional)

### 13.5 Release & Distribution Readiness

- Use `cargo-dist` (or `release-plz`) for professional GitHub Releases + auto-generated installers (homebrew, winget, apt, etc.).
- Publish to crates.io on every tagged release.
- Homebrew tap (optional later): `yuntaolu/homebrew-tap` or request addition to `homebrew-core`.
- Consider a simple shell completion script (`rcat completions`) generated via `clap_complete`.

### 13.6 Summary — "GitHub-Ready from Commit #1"

By the end of Phase 0 the repository should look like a mature, inviting open-source Rust CLI project even before the first real viewer is written. This dramatically improves long-term maintainability, contributor onboarding, and the author's own development velocity.

---

**This document (plan.md) is the single source of truth for scope, architecture, sequencing, and project hygiene until we ship v0.1.**

---

## 14. Progress Log (Implementation)

### 2026-05-26 — Phase 0 Complete (First Commit)

**Achieved in this session (resumed from stuck state):**

- Full Git + GitHub-ready scaffolding created and committed:
  - `.gitignore`, `LICENSE` (MIT), `.editorconfig`, `rustfmt.toml` (stable), `clippy.toml`
  - `justfile` (excellent DX), `CONTRIBUTING.md`, `docs/development.md`
- Rust workspace fully laid out with all 7 planned crates (placeholders ready for Phase 1+ wiring).
- `crates/rcat` binary:
  - Full clap definition matching the proposed CLI surface in Section 7.
  - `rcat --version` and `rcat --help` work beautifully.
  - All flags (`--hex`, `--text`, `--offset` with 0x support, `--length`, `--stdout`, `--list-viewers`, etc.).
  - Non-interactive dump path already functional for both text and hex (basic but correct xxd-style output).
- First clean commit on `main`: `69c89d5` — "chore: initial project skeleton, Rust workspace, and Phase 0 binary" (27 files, fmt + clippy -D warnings clean).
- Git status clean.
- Plan updated with GitHub hosting recommendations (Section 13) and this progress log.

**Next immediate work (per todos):**
- Add `.github/workflows/ci.yml` + Dependabot.
- Update repo README links and status.
- Begin Phase 1: `rcat-core` (FileInfo, detection logic, real Viewer trait + registry).

The project is now in a professional, committable state and ready for active feature development.

We can now discuss, refine, and continue execution with confidence.

---

## Note on This Document

This file (`docs/plan.md`) **inside the repository** is the official plan that travels with the codebase.

- It contains the complete high-level design, the detailed **list of deliverables for each implementation phase** (Section 8), architecture decisions, the `Viewer` trait vision, GitHub/DevEx requirements (Section 13), and the running progress log.
- During active development with AI assistants, a working copy may live temporarily in a session folder (e.g. `~/.grok/sessions/...`). That copy should be merged back into this file when significant updates are made.
- Contributors and future maintainers should treat `docs/plan.md` as the source of truth for "what does each phase actually contain?"

If you are looking for the concrete work breakdown per phase, jump directly to **[Section 8: Implementation Phases & Milestones](#8-implementation-phases--milestones)**.
