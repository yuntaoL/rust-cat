//! Viewer trait (the primary extension point for file type support).
//!
//! Phase 1: basic trait definition only. Real implementations come in later phases.

use crate::file_info::FileInfo;

/// How strongly a viewer wants to handle a particular file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewerPriority {
    /// This viewer cannot handle the file at all.
    None,
    /// Weak interest (fallback).
    Low,
    /// Normal interest.
    Normal,
    /// This viewer is the best choice for the file.
    Preferred,
}

/// Core trait that all viewers (built-in and plugins) must implement.
pub trait FileViewer: Send + Sync {
    /// Human-readable name of this viewer (e.g. "Text", "Hex", "ELF").
    fn name(&self) -> &'static str;

    /// Return how suitable this viewer is for the given file.
    fn can_handle(&self, info: &FileInfo) -> ViewerPriority;

    // Future methods (Phase 2+):
    // fn render(&self, ctx: &RenderContext) -> Result<()>;
    // fn handle_input(&mut self, key: KeyEvent) -> Action;
}
