//! Regression: host spawns the plugin with no CLI args (only piped stdin).

use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

#[test]
fn protocol_mode_when_spawned_with_no_subcommand() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"hello":"world"}}"#).unwrap();
    let path = f.path().display().to_string();

    let request = format!(
        r#"{{"type":"render_lines","file_path":"{path}","start_offset":0,"max_rows":10,"width":80}}"#
    );

    let mut child = Command::new(plugin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn plugin");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{request}").unwrap();
    }

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(!stdout.trim().is_empty(), "expected JSON on stdout");
    assert!(
        stdout.contains("render_lines_result") || stdout.contains("hello"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn render_preserves_file_key_order() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"z_key":1,"a_key":2}}"#).unwrap();
    let path = f.path().display().to_string();

    let request = format!(
        r#"{{"type":"render_lines","file_path":"{path}","start_offset":0,"max_rows":5,"width":120}}"#
    );

    let mut child = Command::new(plugin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn plugin");

    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let z = stdout.find("z_key").expect("z_key in output");
    let a = stdout.find("a_key").expect("a_key in output");
    assert!(z < a, "raw view must preserve key order; stdout={stdout}");
}

#[test]
fn plugin_info_uses_byte_position_kind() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let output = Command::new(plugin)
        .arg("--plugin-info")
        .output()
        .expect("plugin-info");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("byte"),
        "plugin-info should use byte anchors: {stdout}"
    );
}
