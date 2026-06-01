//! Integration test: JSON file viewed via external plugin (when built).

use assert_cmd::assert::OutputAssertExt;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn plugin_binary() -> Option<PathBuf> {
    let rcat = cargo_bin("rcat");
    let plugin = rcat.parent()?.join("rcat-viewer-json");
    if plugin.exists() { Some(plugin) } else { None }
}

#[test]
fn json_file_dump_stdout_uses_plugin_when_present() {
    let Some(_plugin) = plugin_binary() else {
        eprintln!("skip: rcat-viewer-json not built (run cargo build -p rcat-viewer-json)");
        return;
    };

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"hello":"world"}}"#).unwrap();

    let mut cmd = std::process::Command::new(cargo_bin("rcat"));
    cmd.arg("--stdout").arg(f.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"hello\""));
}
