//! rcat-core
//!
//! Core domain types and logic for the rust-cat viewer.
//!
//! This crate is intentionally free of TUI dependencies so it can be used
//! both by the interactive TUI and by the non-interactive dump path.

pub mod detection;
pub mod dump;
pub mod file_info;
pub mod viewer;

pub use file_info::FileInfo;
pub use viewer::{FileViewer, ViewerPriority};
