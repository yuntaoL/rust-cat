# External plugin protocol (v1)

> **Execution status:** PR1 complete — host uses `FileSession` + `render_viewport`; plugins still use v1 one-shot JSON and `file_path`. **PR3** will add protocol v2 (session + `ReadBytes`). See [`docs/plan.md` §15](plan.md#15-execution-track-unified-session--large-files-pr1pr5).

Plugins are executables named `rcat-viewer-*` or `rcat-plugin-*`, discovered next to the `rcat` binary and in `~/.config/rcat/plugins/`.

## Discovery

1. Host runs `plugin --plugin-info` once at registration (JSON metadata).
2. For each file, host may run `CanHandle` with up to 16 KiB of initial data.
3. Interactive TUI uses `RenderLines`, `AdvanceLines`, and `Status` when declared.
4. Non-interactive mode uses `Dump` (protocol) or `plugin dump <path>` (CLI fallback).

## Timeouts

Subprocess requests honor `plugin_timeout_secs` in `~/.config/rcat/config.toml` (default: 5).

## Invocation

The host runs `path/to/rcat-viewer-foo` **with no CLI arguments** and sends a single JSON line on stdin. Plugins must not treat “no args” as an error when stdin is piped (non-TTY).

## JSON requests (one line on stdin, one line on stdout)

| Request | Purpose |
|---------|---------|
| `can_handle` | Return `ViewerPriority` |
| `render_lines` | TUI viewport (`file_path`, `start_offset`, `max_rows`, `width`) |
| `advance_lines` | Scroll position (`current`, `delta`) |
| `status` | Footer text for current position |
| `dump` | Non-interactive output (`offset`, `length`) |

### Offset semantics

- **Line-oriented viewers** (e.g. JSON): `start_offset` / `current` are **line indices** (0-based).
- **Byte-oriented viewers** (future binary plugins): use byte offsets.

## Plugin metadata (v1 extensions)

`--plugin-info` JSON may include:

| Field | Purpose |
|-------|---------|
| `position_kind` | `byte` (default), `display_line`, or `frame` — how TUI anchors map to scroll position |

Example: JSON plugin sets `"position_kind": "display_line"`.

## Planned: protocol v2 (PR3)

- Long-lived plugin process per active TUI viewer
- Host serves file bytes via `ReadBytes` from mmap (no per-frame `read_to_end` in plugins)
- Combined `RenderViewport` response (lines + status)
- Documented alongside v1 in this file when implemented

## Reference implementation

See `crates/rcat-viewers-json` (`rcat-viewer-json`).