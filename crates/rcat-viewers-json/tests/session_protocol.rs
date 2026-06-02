//! Protocol v2 `--session` integration tests.

use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn session_open_render_viewport_preserves_order_and_byte() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let dir = tempdir().unwrap();
    let path = dir.path().join("data.json");
    std::fs::write(&path, br#"{"z":1,"a":2}"#).unwrap();
    let path_str = path.display().to_string();
    let size = std::fs::metadata(&path).unwrap().len();
    let mid = size / 2;

    let mut child = Command::new(plugin)
        .arg("--session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn session");

    let open = format!(
        r#"{{"type":"open","file_path":"{path_str}","file_size":{size},"preliminary":{{"extension":"json","mime_type":"application/json","kind":"Text"}},"initial_data":[]}}"#
    );
    writeln!(child.stdin.as_mut().unwrap(), "{open}").unwrap();
    let open_out = read_line(&mut child);
    assert!(
        open_out.contains("open_result"),
        "expected open_result, got {open_out}"
    );

    let render = format!(
        r#"{{"type":"render_viewport","start_offset":{mid},"max_rows":5,"width":120}}"#
    );
    writeln!(child.stdin.as_mut().unwrap(), "{render}").unwrap();
    let render_out = read_line(&mut child);
    assert!(
        render_out.contains("render_viewport_result"),
        "got {render_out}"
    );
    let z = render_out.find("z").expect("z in output");
    let a = render_out.find("a").expect("a in output");
    assert!(z < a, "session render must preserve file order");
    assert!(
        render_out.contains(&format!("\"source_byte\":{mid}")),
        "expected source_byte={mid} in {render_out}"
    );

    writeln!(
        child.stdin.as_mut().unwrap(),
        r#"{{"type":"close"}}"#
    )
    .unwrap();
    let _ = read_line(&mut child);
    let status = child.wait().expect("wait");
    assert!(status.success(), "stderr should be empty on success");
}

#[test]
fn plugin_info_advertises_protocol_v2() {
    let plugin = env!("CARGO_BIN_EXE_rcat-viewer-json");
    let output = Command::new(plugin)
        .arg("--plugin-info")
        .output()
        .expect("plugin-info");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"protocol_version\": \"2\"") || stdout.contains("\"protocol_version\":\"2\""));
    assert!(stdout.contains("session_v2"));
}

fn read_line(child: &mut std::process::Child) -> String {
    use std::io::BufRead;
    let stdout = child.stdout.as_mut().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read line");
    line
}