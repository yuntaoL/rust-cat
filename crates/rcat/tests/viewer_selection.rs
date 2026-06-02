//! Integration: auto mode must pick JSON for `.json` files (not Text).

use assert_cmd::prelude::*;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::{FileInfo, ViewerRegistry};
use rcat_viewers_hex::HexViewer;
use rcat_viewers_json::JsonViewerLogic;
use rcat_viewers_text::TextViewer;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

fn builtin_registry() -> ViewerRegistry {
    let mut registry = ViewerRegistry::new();
    registry.register(Box::new(TextViewer));
    registry.register(Box::new(HexViewer));
    registry.register(Box::new(JsonViewerLogic));
    registry
}

#[test]
fn find_best_selects_json_for_json_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.json");
    std::fs::write(&path, br#"{"x":1}"#).unwrap();
    let info = FileInfo::from_path(&path).unwrap();
    let prefix = PrefixProbe::from_path(&path).unwrap();
    let mut probe = FileProbeWithInfo::new(&info, prefix);
    let reg = builtin_registry();
    let best = reg.find_best(&mut probe).unwrap();
    assert_eq!(best.name(), "JSON");
}

#[test]
fn json_stdout_dump_matches_file_bytes() {
    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"a":1}}"#).unwrap();

    let mut cmd = Command::cargo_bin("rcat").unwrap();
    cmd.arg("--stdout").arg(f.path());
    cmd.assert()
        .success()
        .stdout(predicates::str::contains(r#"{"a":1}"#));
}