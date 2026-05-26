# rust-cat

[![CI](https://github.com/yuntaolu/rust-cat/actions/workflows/ci.yml/badge.svg)](https://github.com/yuntaolu/rust-cat/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg?logo=rust)](https://www.rust-lang.org)

**A modern, extensible terminal file viewer — text, hex, and beyond.**

`rcat` (the binary) is a fast, keyboard-driven replacement for the mental model of juggling `cat`, `less`, `xxd`/`hexyl`, and ad-hoc tools when inspecting files on the command line.

---

## Features (v0.1 target)

- **Two native viewers**:
  - Beautiful text pager (UTF-8, graceful fallback, line-based virtual scrolling)
  - Professional hex + ASCII view (16-byte rows, color-coded like hexyl)
- **Seamless mode toggle** — stay at the same byte offset when switching between text and hex
- **Excellent large-file support** — uses memory mapping (`memmap2`); works comfortably on multi-GB files
- **Full keyboard navigation** — arrows, PageUp/Down, Home/End, vim-style (`j`/`k`/`gg`/`G`), and less-style bindings
- **Interactive TUI by default** when stdout is a tty; clean non-interactive dump when piped or `--stdout`
- **Metadata sidebar** — size, detected type, magic bytes, simple statistics
- **Proper terminal hygiene** — clean restore on exit or panic
- **Designed for extensibility** from day one (internal `Viewer` trait + clear path to external command plugins)

---

## Installation

### From source (current)

```bash
git clone https://github.com/yuntaolu/rust-cat.git
cd rust-cat
cargo install --path crates/rcat
```

### Planned

- `cargo install rcat`
- Homebrew, etc. (after first release)

---

## Usage

```bash
# Interactive TUI (default when stdout is a tty)
rcat README.md
rcat /bin/ls
rcat large.log
rcat firmware.bin          # auto-detects binary → hex view

# Force a specific viewer
rcat --text some-binary.dat
rcat --hex   document.txt

# Start at a specific offset (great for binary forensics)
rcat --hex -o 0x1000 -l 256 firmware.bin

# Non-interactive / pipe-friendly mode
rcat --stdout file.bin | xxd
some-producer | rcat
rcat file | head -n 50
```

Press `?` inside the TUI for the full keybinding cheat sheet.

---

## Keyboard Navigation (Core Bindings)

| Key                  | Action                          |
|----------------------|---------------------------------|
| `q` / `Ctrl-C`       | Quit                            |
| `?`                  | Help overlay                    |
| `Tab` / `h`          | Toggle Text ↔ Hex               |
| `j` / `↓`            | Down one line / row             |
| `k` / `↑`            | Up                              |
| `Ctrl-d` / `PageDown`| Page down                       |
| `Ctrl-u` / `PageUp`  | Page up                         |
| `g` / `Home`         | Go to start of file             |
| `G` / `End`          | Go to end of file               |
| `m`                  | Toggle metadata sidebar         |

Hex mode has additional column navigation (`h`/`l`) and goto (`Ctrl-j`).

---

## Project Status

**Phase 0 complete** (2026-05-26): Professional Git + GitHub scaffolding + full workspace layout + working `rcat --version` / `--help` / non-interactive dump binary.

We are now entering **Phase 1** (core foundations: `FileInfo`, detection, real `Viewer` trait).

**The full project plan** (including the detailed list of work items for every implementation phase) lives in **[docs/plan.md](docs/plan.md)**. This is the canonical document.

It contains:
- Program definition & scope
- Architecture (Viewer trait, unified navigation model, extensibility strategy)
- **Concrete deliverables for Phase 0 through Phase 5**
- Verification & quality requirements
- GitHub / DevEx / CI recommendations

Read the plan first if you want to understand the roadmap or contribute to a specific phase.

---

## Extensibility Vision

`rcat` is built around a clean `Viewer` trait. New file-type support can be added as:

1. **Built-in viewers** (workspace crates) — compile-time, zero overhead
2. **External command plugins** (`rcat-*` binaries) — discovered at runtime, any language, process-isolated (recommended first runtime mechanism)
3. **WASM plugins** (future) — sandboxed, rich integration via Extism or Wasmtime + Component Model

The external plugin protocol will be documented early so the community can experiment even before the TUI is feature-complete.

---

## Development

### Prerequisites

- Rust 1.85+ (2024 edition)
- A modern terminal (iTerm2, WezTerm, Alacritty, Kitty, Ghostty, etc. recommended for best Unicode / color experience)

### Quick start

```bash
cargo run -- --help
cargo run -- some-file.txt
```

### Useful commands (once `just` is installed)

```bash
just build
just test
just lint
just fmt
just run README.md
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full development guide.

---

## License

MIT © 2026 Yuntao Lu

---

## Acknowledgments

- [Ratatui](https://ratatui.rs) — the excellent TUI framework this project is built on
- [heh](https://github.com/ndd7xv/heh) — inspirational embeddable hex editor widget
- [bat](https://github.com/sharkdp/bat), [hexyl](https://github.com/sharkdp/hexyl), [binsider](https://github.com/orhun/binsider) — tools that raised the bar for terminal file inspection
- The broader Rust CLI / TUI community for crates, patterns, and best practices
