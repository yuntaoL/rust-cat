# External plugin protocol

> **Status:** v1 one-shot requests remain supported. **v2 session mode** (`--session`) is implemented in PR3 — long-lived subprocess, host mmap via `ReadBytes`, combined `render_viewport`. See [`docs/plan.md` §15](plan.md#15-execution-track-unified-session--large-files-pr1pr5).

Plugins are executables named `rcat-viewer-*` or `rcat-plugin-*`, discovered next to the `rcat` binary and in `~/.config/rcat/plugins/`.

## Discovery

1. Host runs `plugin --plugin-info` once at registration (JSON metadata).
2. For each file, host may run `CanHandle` with up to 16 KiB of initial data (one-shot process).
3. Interactive TUI uses v2 session when `protocol_version` is `"2"` or `session_v2` capability is declared.
4. Non-interactive mode uses `Dump` (protocol) or `plugin dump <path>` (CLI fallback).

## Timeouts

Subprocess requests honor `plugin_timeout_secs` in `~/.config/rcat/config.toml` (default: 5). Session mode uses the same per-request timeout for each JSON round-trip.

## Invocation modes

| Mode | How to start | Use case |
|------|----------------|----------|
| **v1 one-shot** | `rcat-viewer-foo` with no args, one JSON line on stdin | `CanHandle`, legacy TUI without v2 |
| **v2 session** | `rcat-viewer-foo --session`, line-delimited JSON loop | Interactive TUI (host keeps process alive) |
| **CLI** | `rcat-viewer-foo dump <path>` | Scripts, fallback |

The host runs plugins **with no other CLI arguments** in protocol modes. Plugins must not treat “no args” as an error when stdin is piped (non-TTY).

## Protocol v1 (one-shot)

One JSON request on stdin, one JSON response on stdout.

| Request | Purpose |
|---------|---------|
| `can_handle` | Return `ViewerPriority` |
| `render_lines` | TUI viewport (`file_path`, `start_offset`, `max_rows`, `width`) |
| `advance_lines` | Scroll position |
| `status` | Footer text |
| `dump` | Non-interactive output |
| `byte_at_display_line` / `display_line_at_byte` | Display-line sync (legacy pretty-print viewers) |

## Protocol v2 (session)

Advertise with `"protocol_version": "2"` and/or `"session_v2"` in capabilities (see `rcat-viewer-json`).

### Lifecycle

1. Host spawns `plugin --session` once per active external viewer (reused across scroll/redraw while PR2 cache misses).
2. **`open`** — host sends file path, size, `preliminary` detection, and up to 16 KiB prefix from mmap.
3. **`render_viewport`** — combined lines + status + optional `source_byte` (preferred for TUI).
4. v1-shaped **`render_lines` / `advance_lines` / `status`** still work inside a session (no `file_path` required on plugin side after open).
5. **`read_bytes`** — host pushes `{ offset, data }` when the plugin returned `need_read_bytes`.
6. **`close`** — drop current file; process may continue for another `open`.

### Session requests (host → plugin)

| Request | Fields | Notes |
|---------|--------|-------|
| `open` | `file_path`, `file_size`, `preliminary`, `initial_data` | Host mmap prefix |
| `close` | — | End current file |
| `render_viewport` | `start_offset`, `max_rows`, `width` | Combined TUI response |
| `read_bytes` | `offset`, `data` | Host supplies bytes (not a pull from plugin disk) |
| `render_lines` / `advance_lines` / `status` | Same as v1 | Optional; use open state |

### Session responses (plugin → host)

| Response | Purpose |
|----------|---------|
| `open_result` | File accepted |
| `close_result` | File closed |
| `render_viewport_result` | `{ lines, status, source_byte? }` |
| `read_bytes_result` | Ack after host data |
| `need_read_bytes` | `{ offset, length }` — host replies with `read_bytes` |

### Offset semantics

- **Byte viewers** (JSON raw view, hex): `start_offset` is a **byte offset** into the file.
- **Display-line viewers**: line indices (legacy).

## Plugin metadata (`--plugin-info`)

| Field | Purpose |
|-------|---------|
| `protocol_version` | `"1"` or `"2"` |
| `capabilities` | `can_handle`, `dump`, `render_lines`, `session_v2` |
| `position_kind` | `byte`, `display_line`, or `frame` |

Example (JSON plugin):

```json
{
  "name": "JSON",
  "protocol_version": "2",
  "capabilities": ["can_handle", "dump", "render_lines", "session_v2"],
  "position_kind": "byte"
}
```

## Reference implementation

- Library: `crates/rcat-viewers-json` (`JsonViewerLogic` + `tiers.rs`)
- Plugin binary: `rcat-viewer-json` (`--plugin-info`, `--session`, v1 stdin mode)
- Host session driver: `crates/rcat-core/src/plugin_session.rs`

### JSON tiers (PR5)

Tier **detection** (`SmallPretty`, `Ndjson`, `LargeRaw`, `InvalidRaw`) is implemented in `tiers.rs` for future opt-in formatting. The **interactive TUI always renders on-disk bytes** (M1) so key order and byte offsets match Text and Hex. Invalid small files get a parse-error hint in the status line only.