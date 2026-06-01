//! rcat-core
//!
//! Core domain types and logic for the rust-cat viewer.
//!
//! This crate is intentionally free of TUI dependencies so it can be used
//! both by the interactive TUI and by the non-interactive dump path.

pub mod backing;
pub mod detection;
pub mod dump;
pub mod external_plugin;
pub mod file_info;
pub mod plugin;
pub mod probe;
pub mod session;
pub mod view;
pub mod viewer;
pub mod viewer_registry;

pub use file_info::FileInfo;
pub use probe::{DETECTION_READ_LIMIT, FileProbe, FileProbeWithInfo, PrefixProbe};
pub use session::FileSession;
pub use view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};
pub use viewer::{FileViewer, ViewerPriority};
pub use viewer_registry::ViewerRegistry;
