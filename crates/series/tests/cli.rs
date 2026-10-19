//! End-to-end smoke tests: run the built binary against fixtures on stdin.
//! Uses `CARGO_BIN_EXE_ts-series` (set by Cargo) — std only, no dev-dependencies.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-series"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn series");
    // ignore BrokenPipe: the child may exit before reading stdin (e.g. arg error)
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait series")
}

#[test]
fn sorts_and_summarizes() {
    let out = run(include_str!("fixtures/series.txt"));
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();

    // unordered input is sorted by time
    let at_20 = stdout.find("10:20:00").expect("10:20:00 row");
    let at_22 = stdout.find("10:22:00").expect("10:22:00 row");
    assert!(at_20 < at_22, "rows not sorted:\n{stdout}");

    // extremes tagged with their time; ramp spans min..max
    assert!(stdout.contains("103  at 10:22:00"), "stdout:\n{stdout}");
    assert!(stdout.contains("12  at 10:20:00"), "stdout:\n{stdout}");
    assert!(stdout.contains('█'), "no peak block:\n{stdout}");
    assert!(stdout.contains('▁'), "no trough block:\n{stdout}");
}

#[test]
fn sorts_within_the_same_second() {
    // rows print at second resolution, so the values tell the order apart
    let out = run("2026-09-03T10:20:00.900+09:00 9\n2026-09-03T10:20:00.100+09:00 1\n");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let values: Vec<&str> = stdout
        .lines()
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_whitespace().last())
        .collect();
    assert_eq!(values, ["1", "9"], "stdout:\n{stdout}");
}

#[test]
fn negative_zero_prints_as_zero() {
    let out = run("2026-09-03T10:20:00+09:00 -0\n2026-09-03T10:21:00+09:00 1\n");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("-0"), "stdout:\n{stdout}");
    assert!(stdout.contains("0  at 10:20:00"), "stdout:\n{stdout}");
}

#[test]
fn skips_garbage_but_still_succeeds() {
    let out = run(include_str!("fixtures/invalid.txt"));
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("skipped 2"), "stderr:\n{stderr}");
}

#[test]
fn empty_input_fails_cleanly() {
    let out = run("");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no 'timestamp value'"), "stderr:\n{stderr}");
}

#[test]
fn closed_stdout_exits_quietly() {
    // reader dropped before spawn, so every write hits a closed pipe (`| head`)
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-series"))
        .stdin(Stdio::piped())
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn series");
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(include_str!("fixtures/series.txt").as_bytes());
    let out = child.wait_with_output().expect("wait series");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
}
