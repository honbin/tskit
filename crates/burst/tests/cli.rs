//! End-to-end smoke tests: run the built binary against fixtures on stdin.
//! Uses `CARGO_BIN_EXE_ts-burst` (set by Cargo for integration tests) so there are
//! no extra dev-dependencies — just std.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-burst"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn burst");
    // Dropping the stdin handle at the end of this statement closes the pipe,
    // so burst sees EOF and finishes.
    // ignore BrokenPipe: the child may exit before reading stdin (e.g. arg error)
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait burst")
}

#[test]
fn reports_peak_and_total() {
    let out = run(include_str!("fixtures/simple.txt"));
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("events"), "stdout:\n{stdout}");
    assert!(stdout.contains("peak"), "stdout:\n{stdout}");
}

#[test]
fn skips_garbage_but_still_succeeds() {
    let out = run(include_str!("fixtures/invalid.txt"));
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("skipped"), "stderr:\n{stderr}");
}

#[test]
fn rejects_mixed_offsets() {
    let out = run(include_str!("fixtures/mixed-offsets.txt"));
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("mixed UTC offsets"), "stderr:\n{stderr}");
}

#[test]
fn empty_input_fails_cleanly() {
    let out = run("");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("no parseable timestamps"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn closed_stdout_exits_quietly() {
    // reader dropped before spawn, so every write hits a closed pipe (`| head`)
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-burst"))
        .stdin(Stdio::piped())
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn burst");
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(include_str!("fixtures/simple.txt").as_bytes());
    let out = child.wait_with_output().expect("wait burst");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
}
