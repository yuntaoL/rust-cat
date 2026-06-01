# External plugin protocol (v1)

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

## Reference implementation

See `crates/rcat-viewers-json` (`rcat-viewer-json`).