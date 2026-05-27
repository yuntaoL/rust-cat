//! Simple registry for `FileViewer` implementations.
//!
//! This makes it easy to collect viewers (both built-in and future plugins)
//! and select the most appropriate one for a given file.

use crate::probe::FileProbe;
use crate::viewer::FileViewer;

/// A registry that holds multiple `FileViewer` implementations.
pub struct ViewerRegistry {
    viewers: Vec<Box<dyn FileViewer>>,
}

impl ViewerRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            viewers: Vec::new(),
        }
    }

    /// Register a new viewer.
    pub fn register(&mut self, viewer: Box<dyn FileViewer>) {
        self.viewers.push(viewer);
    }

    /// Find the viewer with the highest `ViewerPriority` for the given probe.
    ///
    /// Returns `None` only if the registry is empty.
    pub fn find_best(&self, probe: &mut dyn FileProbe) -> Option<&dyn FileViewer> {
        self.viewers
            .iter()
            .map(|viewer| (viewer.as_ref(), viewer.can_handle(probe)))
            .max_by_key(|(_, priority)| *priority)
            .map(|(viewer, _)| viewer)
    }

    /// Returns all registered viewers.
    pub fn all_viewers(&self) -> &[Box<dyn FileViewer>] {
        &self.viewers
    }

    /// Returns the number of registered viewers.
    pub fn len(&self) -> usize {
        self.viewers.len()
    }

    /// Returns true if no viewers are registered.
    pub fn is_empty(&self) -> bool {
        self.viewers.is_empty()
    }
}

impl Default for ViewerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
