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
    // Future: RenderRich, Metadata, Search, etc.
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

    /// Plugin is asking for more file data.
    ReadBytes { offset: u64, length: usize },

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
}

/// Response from plugin to host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginResponse {
    /// Result of a `CanHandle` request.
    CanHandleResult {
        priority: crate::viewer::ViewerPriority,
    },

    /// Data returned for a `ReadBytes` request.
    ReadBytesResult { data: Vec<u8> },

    /// Lines to display in the TUI content area.
    RenderLinesResult { lines: Vec<String> },

    /// New viewport position after `AdvanceLines`.
    AdvanceLinesResult { position: u64 },

    /// Status text for the footer (viewer may include offset / line hints).
    StatusResult { status: String },

    /// UTF-8 dump output for `Dump` requests.
    DumpResult { output: String },

    /// Generic error.
    Error { message: String },
}

/// Default timeout for external plugin subprocesses.
pub const DEFAULT_PLUGIN_TIMEOUT_SECS: u64 = 5;
