//! Exercise `ExternalPluginViewer` against the real JSON plugin binary.

use assert_cmd::cargo::cargo_bin;
use rcat_core::dump::DumpOptions;
use rcat_core::external_plugin::ExternalPluginViewer;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::viewer::FileViewer;
use rcat_core::FileInfo;
use std::io::Write;
use std::time::Duration;
use tempfile::NamedTempFile;

fn json_plugin_path() -> Option<std::path::PathBuf> {
    let rcat = cargo_bin("rcat");
    let plugin = rcat.parent()?.join("rcat-viewer-json");
    if plugin.exists() {
        Some(plugin)
    } else {
        None
    }
}

#[test]
fn external_json_plugin_loads_and_can_handle() {
    let Some(plugin) = json_plugin_path() else {
        eprintln!("skip: rcat-viewer-json not built");
        return;
    };

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"k":1}}"#).unwrap();
    let info = FileInfo::from_path(f.path()).unwrap();
    let prefix = PrefixProbe::from_path(f.path()).unwrap();
    let mut probe = FileProbeWithInfo::new(&info, prefix);

    let viewer = ExternalPluginViewer::with_timeout(plugin, Duration::from_secs(5)).unwrap();
    assert_eq!(viewer.name(), "JSON");
    let prio = viewer.can_handle(&mut probe);
    assert_ne!(prio, rcat_core::ViewerPriority::None);
}

#[test]
fn external_json_plugin_render_and_dump() {
    let Some(plugin) = json_plugin_path() else {
        eprintln!("skip: rcat-viewer-json not built");
        return;
    };

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"z":1,"a":2}}"#).unwrap();
    let info = FileInfo::from_path(f.path()).unwrap();
    let viewer = ExternalPluginViewer::with_timeout(plugin, Duration::from_secs(5)).unwrap();

    let lines = viewer.render_lines(&info, 0, 5, 120);
    let joined = lines.join("\n");
    let z = joined.find("z").expect("z_key fragment in output");
    let a = joined.find("a").expect("a_key fragment in output");
    assert!(z < a, "plugin render must preserve file order: {joined}");

    let mut buf = Vec::new();
    viewer
        .dump(
            &info,
            &mut buf,
            &DumpOptions {
                offset: 0,
                length: None,
            },
        )
        .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("z"));

    let status = viewer.status(&info, 0);
    assert!(status.contains("JSON"));
    let advanced = viewer.advance_lines(&info, 0, 1, 80);
    assert!(advanced > 0);
}