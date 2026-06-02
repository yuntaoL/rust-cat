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
fn byte_at_display_line_returns_offset() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");

    let mut f = NamedTempFile::new().unwrap();
    write!(f, r#"{{"first":1,"second":2,"third":3}}"#).unwrap();
    let path = f.path().display().to_string();

    let request = format!(r#"{{"type":"byte_at_display_line","file_path":"{path}","line":2}}"#);

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
    assert!(
        stdout.contains("byte_at_display_line_result"),
        "stdout={stdout}"
    );
}

#[test]
fn display_line_at_byte_maps_mid_file() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");

    let mut f = NamedTempFile::new().unwrap();
    let mut obj = String::from("{");
    for i in 0..30 {
        if i > 0 {
            obj.push(',');
        }
        obj.push_str(&format!("\n  \"field{i:02}\": {i}"));
    }
    obj.push_str("\n}\n");
    write!(f, "{obj}").unwrap();
    let path = f.path().display().to_string();
    let raw = std::fs::read(f.path()).unwrap();
    let mid_byte = raw.len() / 2;

    let request =
        format!(r#"{{"type":"display_line_at_byte","file_path":"{path}","byte":{mid_byte}}}"#);

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
    let resp: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    let line = resp
        .get("line")
        .and_then(|v| v.as_u64())
        .expect("line field");
    assert!(line > 0, "mid-file byte should map past line 0, got {line}");
}
