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
fn render_preserves_file_key_order_in_raw_tier() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");

    let mut f = NamedTempFile::new().unwrap();
    let mut data = br#"{"z_key":1,"a_key":2}"#.to_vec();
    data.resize(2 * 1024 * 1024 + 1, b'\n');
    f.write_all(&data).unwrap();
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
fn can_handle_json_file_via_protocol() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"x":1}}"#).unwrap();
    let request = format!(
        r#"{{"type":"can_handle","file_size":{size},"preliminary":{{"mime_type":"application/json","extension":"json","kind":"Text"}},"initial_data":[]}}"#,
        size = std::fs::metadata(f.path()).unwrap().len()
    );

    let mut child = Command::new(plugin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("preferred") || stdout.contains("Preferred"),
        "expected preferred priority: {stdout}"
    );
}

#[test]
fn can_handle_accepts_brace_prefix_without_extension() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let request = r#"{"type":"can_handle","file_size":10,"preliminary":{"kind":"Text"},"initial_data":[123,34,97,34,58,49,125]}"#;
    let mut child = Command::new(plugin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("preferred") || stdout.contains("Preferred"));
}

#[test]
fn protocol_status_and_advance_lines() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.json");
    std::fs::write(&path, b"line0\nline1\n").unwrap();
    let path_str = path.display().to_string();

    for (req_type, field) in [
        (
            format!(
                r#"{{"type":"status","file_path":"{path_str}","position":0}}"#
            ),
            "status_result",
        ),
        (
            format!(
                r#"{{"type":"advance_lines","file_path":"{path_str}","current":0,"delta":1,"width":80}}"#
            ),
            "advance_lines_result",
        ),
    ] {
        let mut child = Command::new(plugin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        writeln!(child.stdin.as_mut().unwrap(), "{req_type}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{field} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains(field),
            "expected {field} in {stdout}"
        );
    }
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
