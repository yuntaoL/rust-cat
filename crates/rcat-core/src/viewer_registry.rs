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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_info::FileInfo;
    use crate::probe::{FileProbe, PrefixProbe};
    use crate::viewer::{FileViewer, ViewerPriority};
    use std::io::Write;

    // Simple test viewer for registry testing
    struct TestViewer {
        name: &'static str,
        priority: ViewerPriority,
    }

    impl TestViewer {
        fn new(name: &'static str, priority: ViewerPriority) -> Self {
            Self { name, priority }
        }
    }

    impl FileViewer for TestViewer {
        fn name(&self) -> &'static str {
            self.name
        }

        fn can_handle(&self, _probe: &mut dyn FileProbe) -> ViewerPriority {
            self.priority
        }

        fn dump(
            &self,
            _info: &FileInfo,
            _writer: &mut dyn Write,
            _opts: &crate::dump::DumpOptions,
        ) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn make_dummy_probe() -> (FileInfo, PrefixProbe) {
        // Use a temp file so PrefixProbe works
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dummy.bin");
        std::fs::write(&path, b"hello").unwrap();
        let info = FileInfo::from_path(&path).unwrap();
        let probe = PrefixProbe::from_path(&path).unwrap();
        (info, probe)
    }

    #[test]
    fn empty_registry_returns_none() {
        let reg = ViewerRegistry::new();
        let (_info, mut probe) = make_dummy_probe();
        assert!(reg.find_best(&mut probe).is_none());
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registers_and_finds_highest_priority() {
        let mut reg = ViewerRegistry::new();
        reg.register(Box::new(TestViewer::new("Low", ViewerPriority::Low)));
        reg.register(Box::new(TestViewer::new("Normal", ViewerPriority::Normal)));
        reg.register(Box::new(TestViewer::new(
            "Preferred",
            ViewerPriority::Preferred,
        )));

        let (_info, mut probe) = make_dummy_probe();
        let best = reg.find_best(&mut probe).unwrap();
        assert_eq!(best.name(), "Preferred");
    }

    #[test]
    fn find_best_returns_some_even_when_all_none() {
        // When all viewers return None, find_best still returns one of them
        // (the first in max_by_key tie-breaking)
        let mut reg = ViewerRegistry::new();
        reg.register(Box::new(TestViewer::new("None1", ViewerPriority::None)));
        reg.register(Box::new(TestViewer::new("None2", ViewerPriority::None)));

        let (_info, mut probe) = make_dummy_probe();
        let best = reg.find_best(&mut probe);
        assert!(best.is_some());
        assert!(best.unwrap().name().starts_with("None"));
    }

    #[test]
    fn all_viewers_returns_registered_in_order() {
        let mut reg = ViewerRegistry::new();
        reg.register(Box::new(TestViewer::new("A", ViewerPriority::Normal)));
        reg.register(Box::new(TestViewer::new("B", ViewerPriority::Low)));

        let all = reg.all_viewers();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name(), "A");
        assert_eq!(all[1].name(), "B");
    }

    #[test]
    fn default_registry_is_empty() {
        let reg: ViewerRegistry = Default::default();
        assert!(reg.is_empty());
    }
}
