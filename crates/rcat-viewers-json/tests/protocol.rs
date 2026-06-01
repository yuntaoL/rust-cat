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
