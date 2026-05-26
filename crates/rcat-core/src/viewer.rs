//! Viewer trait definition (the primary extension point).
//! Phase 0: stub only.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewerPriority {
    None,
    Low,
    Normal,
    Preferred,
}

pub trait FileViewer: Send + Sync {
    fn name(&self) -> &'static str;
    // fn can_handle(&self, info: &FileInfo) -> ViewerPriority;
    // fn render(...) ...
}
