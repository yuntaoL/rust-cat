use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn help_works() {
    let mut cmd = Command::cargo_bin("rcat").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn list_viewers_works() {
    let mut cmd = Command::cargo_bin("rcat").unwrap();
    cmd.arg("--list-viewers")
        .assert()
        .success()
        .stdout(predicate::str::contains("Built-in viewers"));
}

#[test]
fn version_works() {
    let mut cmd = Command::cargo_bin("rcat").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rcat"));
}

#[test]
fn stdout_on_text_file_works() {
    // Use the workspace Cargo.toml by going up from the test binary location
    // A more robust way: use a temp file we control
    use std::io::Write;
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    writeln!(temp, "[package]\nname = \"test\"").unwrap();
    let path = temp.path();

    let mut cmd = Command::cargo_bin("rcat").unwrap();
    cmd.arg("--stdout")
        .arg(path)
        .assert()
        .success()
        .stdout(predicate::str::contains("[package]"));
}
