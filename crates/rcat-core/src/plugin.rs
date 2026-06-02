//! Plugin protocol types and discovery logic for external command plugins.
//!
//! This module defines the interface between the main `rcat` binary and
//! external viewer plugins. Plugins are separate executables that communicate
//! via JSON over stdin/stdout.

use serde::{Deserialize, Serialize};

use crate::view::PositionKind;

/// Response from a plugin's `--plugin-info` command.
/// This must be implemented by all external plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Human-readable name of the plugin (e.g. "Markdown", "ELF", "PNG").
    pub name: String,

    /// Plugin version (semantic versioning recommended).
    pub version: String,

    /// Protocol version. Start with "1".
    pub protocol_version: String,

    /// What this plugin can do.
    pub capabilities: Vec<PluginCapability>,

    /// Hints for fast pre-filtering before spawning the plugin.
    #[serde(default)]
    pub handles: PluginHandles,

    /// Suggested default priority when this plugin matches.
    #[serde(default)]
    pub default_priority: PluginDefaultPriority,

    /// How scroll position is interpreted in TUI requests (v2+; optional in v1 plugins).
    #[serde(default)]
    pub position_kind: Option<PositionKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    CanHandle,
    Dump,
    RenderLines,
    /// Long-lived `--session` subprocess; host serves bytes via `ReadBytes`.
    SessionV2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginHandles {
    /// File extensions this plugin is interested in (without the dot).
    #[serde(default)]
    pub extensions: Vec<String>,

    /// MIME types this plugin is interested in.
    #[serde(default)]
    pub mime_types: Vec<String>,

    /// Magic byte prefixes (as hex strings, e.g. "7f454c46" for ELF).
    #[serde(default)]
    pub magic: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginDefaultPriority {
    Low,
    #[default]
    Normal,
    Preferred,
}

/// Request sent from host to plugin during detection (pull model).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginRequest {
    /// Ask the plugin whether it can handle the current file.
    /// The host may have already sent some initial data.
    CanHandle {
        file_size: u64,
        preliminary: crate::detection::PreliminaryDetection,
        /// Initial data the host proactively provides (usually first 4K~8K).
        initial_data: Vec<u8>,
    },

    /// Host provides file bytes to the plugin (v2 session pull from host mmap).
    ReadBytes {
        offset: u64,
        data: Vec<u8>,
    },

    /// Render a viewport for the interactive TUI.
    ///
    /// `start_offset` meaning is viewer-specific (byte offset for hex-oriented
    /// plugins, line index for line-oriented plugins such as JSON).
    RenderLines {
        file_path: String,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    },

    /// Advance the viewport position by display rows (same offset semantics as `RenderLines`).
    AdvanceLines {
        file_path: String,
        current: u64,
        delta: i64,
        width: u16,
    },

    /// Human-readable position string for the status bar.
    Status { file_path: String, position: u64 },

    /// Non-interactive dump with optional range (protocol alternative to `dump` CLI).
    Dump {
        file_path: String,
        offset: u64,
        length: Option<u64>,
    },

    /// Source file byte offset for a pretty-printed display line (JSON sync).
    ByteAtDisplayLine { file_path: String, line: u64 },

    /// Display line index closest to a source byte offset (JSON sync).
    DisplayLineAtByte { file_path: String, byte: u64 },

    // --- Protocol v2 (session subprocess) ---

    /// Open a file in an existing `--session` plugin process.
    Open {
        file_path: String,
        file_size: u64,
        preliminary: crate::detection::PreliminaryDetection,
        /// Prefix bytes from host mmap (usually first 16 KiB).
        initial_data: Vec<u8>,
    },

    /// Close the current file in the session (process may keep running).
    Close,

    /// Combined TUI viewport (lines + status) for v2 sessions.
    RenderViewport {
        start_offset: u64,
        max_rows: u16,
        width: u16,
    },
}

/// Response from plugin to host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginResponse {
    /// Result of a `CanHandle` request.
    CanHandleResult {
        priority: crate::viewer::ViewerPriority,
    },

    /// Acknowledgement after the host supplied `ReadBytes` data.
    ReadBytesResult,

    /// Plugin needs more bytes from the host (v2 session); host should reply with `ReadBytes`.
    NeedReadBytes {
        offset: u64,
        length: usize,
    },

    /// Lines to display in the TUI content area.
    RenderLinesResult {
        lines: Vec<String>,
    },

    /// New viewport position after `AdvanceLines`.
    AdvanceLinesResult {
        position: u64,
    },

    /// Status text for the footer (viewer may include offset / line hints).
    StatusResult {
        status: String,
    },

    /// UTF-8 dump output for `Dump` requests.
    DumpResult {
        output: String,
    },

    ByteAtDisplayLineResult {
        byte_offset: u64,
    },

    DisplayLineAtByteResult {
        line: u64,
    },

    /// Generic error.
    Error {
        message: String,
    },

    // --- Protocol v2 (session subprocess) ---

    OpenResult,

    CloseResult,

    /// Combined viewport for v2 `RenderViewport` requests.
    RenderViewportResult {
        lines: Vec<String>,
        status: String,
        source_byte: Option<u64>,
    },
}

/// Protocol version string for v2 (long-lived session + host `ReadBytes`).
pub const PROTOCOL_VERSION_V2: &str = "2";

/// Default timeout for external plugin subprocesses.
pub const DEFAULT_PLUGIN_TIMEOUT_SECS: u64 = 5;

/// Returns true when the plugin advertises protocol v2 session support.
pub fn supports_protocol_v2(info: &PluginInfo) -> bool {
    info.protocol_version == PROTOCOL_VERSION_V2
        || info.capabilities.contains(&PluginCapability::SessionV2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_request_types_serialize_with_snake_case_type() {
        let open = PluginRequest::Open {
            file_path: "/tmp/x.json".into(),
            file_size: 10,
            preliminary: Default::default(),
            initial_data: vec![b'{'],
        };
        let json = serde_json::to_string(&open).unwrap();
        assert!(json.contains("\"type\":\"open\""));

        let rv = PluginRequest::RenderViewport {
            start_offset: 0,
            max_rows: 5,
            width: 80,
        };
        assert!(serde_json::to_string(&rv).unwrap().contains("render_viewport"));

        let close = PluginResponse::CloseResult;
        assert!(
            serde_json::to_string(&close)
                .unwrap()
                .contains("close_result")
        );
    }

    #[test]
    fn supports_v2_by_protocol_version_or_capability() {
        let mut info = PluginInfo {
            name: "T".into(),
            version: "0".into(),
            protocol_version: "1".into(),
            capabilities: vec![PluginCapability::CanHandle],
            handles: PluginHandles::default(),
            default_priority: PluginDefaultPriority::default(),
            position_kind: None,
        };
        assert!(!supports_protocol_v2(&info));
        info.protocol_version = PROTOCOL_VERSION_V2.to_string();
        assert!(supports_protocol_v2(&info));
        info.protocol_version = "1".into();
        info.capabilities.push(PluginCapability::SessionV2);
        assert!(supports_protocol_v2(&info));
    }
}
