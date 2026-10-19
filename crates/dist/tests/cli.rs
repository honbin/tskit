//! End-to-end smoke tests: run the built binary against fixtures on stdin.
//! Uses `CARGO_BIN_EXE_ts-dist` (set by Cargo) — std only, no dev-dependencies.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-dist"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dist");
    // ignore BrokenPipe: the child may exit before reading stdin (e.g. arg error)
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait dist")
}

#[test]
fn reports_percentiles() {
    let out = run(include_str!("fixtures/numbers.txt"));
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("count"), "stdout:\n{stdout}");
    assert!(stdout.contains("p99"), "stdout:\n{stdout}");
}

#[test]
fn skips_garbage_but_still_succeeds() {
    let out = run(include_str!("fixtures/invalid.txt"));
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("skipped"), "stderr:\n{stderr}");
}

#[test]
fn empty_input_fails_cleanly() {
    let out = run("");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no numbers"), "stderr:\n{stderr}");
}

#[test]
fn closed_stdout_exits_quietly() {
    // reader dropped before spawn, so every write hits a closed pipe (`| head`)
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-dist"))
        .stdin(Stdio::piped())
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dist");
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(include_str!("fixtures/numbers.txt").as_bytes());
    let out = child.wait_with_output().expect("wait dist");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
}
