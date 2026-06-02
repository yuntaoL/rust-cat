//! Exercise `ExternalPluginViewer` against the real JSON plugin binary.

use assert_cmd::cargo::cargo_bin;
use rcat_core::dump::DumpOptions;
use rcat_core::external_plugin::ExternalPluginViewer;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::view::ViewContext;
use rcat_core::viewer::FileViewer;
use rcat_core::{FileInfo, FileSession, supports_protocol_v2};
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

    let viewer = ExternalPluginViewer::with_timeout(plugin, Duration::from_secs(5)).unwrap();

    // Small file → pretty tier (formatted output, not necessarily key order).
    let mut small = NamedTempFile::new().unwrap();
    write!(small, r#"{{"z":1,"a":2}}"#).unwrap();
    let small_info = FileInfo::from_path(small.path()).unwrap();
    let pretty_lines = viewer.render_lines(&small_info, 0, 8, 120);
    let pretty_joined = pretty_lines.join("\n");
    assert!(pretty_joined.contains("z") && pretty_joined.contains("a"));
    let pretty_status = viewer.status(&small_info, 0);
    assert!(pretty_status.contains("pretty"));

    // Large file → raw tier (preserves on-disk key order).
    let mut large = NamedTempFile::new().unwrap();
    let mut data = br#"{"z":1,"a":2}"#.to_vec();
    data.resize(2 * 1024 * 1024 + 1, b'\n');
    large.write_all(&data).unwrap();
    let large_info = FileInfo::from_path(large.path()).unwrap();
    let raw_lines = viewer.render_lines(&large_info, 0, 5, 120);
    let raw_joined = raw_lines.join("\n");
    let z = raw_joined.find("z").expect("z in raw output");
    let a = raw_joined.find("a").expect("a in raw output");
    assert!(z < a, "raw tier must preserve file order: {raw_joined}");
    let raw_status = viewer.status(&large_info, 0);
    assert!(raw_status.contains("raw"));

    let mut buf = Vec::new();
    viewer
        .dump(
            &small_info,
            &mut buf,
            &DumpOptions {
                offset: 0,
                length: None,
            },
        )
        .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("z"));

    let advanced = viewer.advance_lines(&small_info, 0, 1, 80);
    assert!(advanced > 0);
}

#[test]
fn external_json_v2_uses_session_for_render_viewport() {
    let Some(plugin) = json_plugin_path() else {
        eprintln!("skip: rcat-viewer-json not built");
        return;
    };

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"key":42}}"#).unwrap();
    let session = FileSession::open(f.path()).unwrap();
    let viewer = ExternalPluginViewer::with_timeout(plugin, Duration::from_secs(5)).unwrap();
    assert!(
        supports_protocol_v2(viewer.info()),
        "json plugin should advertise v2"
    );

    let ctx = ViewContext::at_byte(&session, 0, 80, 5);
    let vp = viewer.render_viewport(&ctx);
    assert!(vp.lines.iter().any(|l| l.contains("key")));
    assert!(vp.status.contains("JSON"));

    // Second render reuses the same session (no error).
    let vp2 = viewer.render_viewport(&ctx);
    assert!(!vp2.lines.is_empty());
}