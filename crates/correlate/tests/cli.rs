//! End-to-end smoke tests: run the built binary against fixture files.
//! Uses `CARGO_BIN_EXE_ts-correlate` and `CARGO_MANIFEST_DIR` (set by Cargo).

use std::process::{Command, Output, Stdio};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-correlate"))
        .args(args)
        .output()
        .expect("run correlate")
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn reports_both_directions() {
    let a = fixture("a.txt");
    let b = fixture("b.txt");
    let out = run(&[a.as_str(), b.as_str(), "--window", "5s"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("±5s"), "stdout:\n{stdout}");
    assert!(stdout.contains("66.7"), "stdout:\n{stdout}"); // 2/3 of a within 5s of b
    assert!(stdout.contains("50.0"), "stdout:\n{stdout}"); // 1/2 of b within 5s of a
}

#[test]
fn subsecond_gap_outside_window_is_not_matched() {
    // 10:20:00.000100 vs 10:20:01.000900 = 1.0008s apart; ±1s must NOT match
    // (regression: truncating to whole seconds — or milliseconds — collapsed this).
    let a = fixture("subsecond-a.txt");
    let b = fixture("subsecond-b.txt");
    let out = run(&[a.as_str(), b.as_str(), "--window", "1s"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("0.0%"), "stdout:\n{stdout}");
    assert!(!stdout.contains("100.0%"), "stdout:\n{stdout}");
}

#[test]
fn window_too_large_errors_cleanly() {
    // an over-range --window must error (not silently shrink, not panic)
    let a = fixture("a.txt");
    let b = fixture("b.txt");
    let out = run(&[a.as_str(), b.as_str(), "--window", "10000000000s"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("too large"), "stderr:\n{stderr}");
}

#[test]
fn needs_two_files() {
    let a = fixture("a.txt");
    let out = run(&[a.as_str()]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("two files"), "stderr:\n{stderr}");
}

#[test]
fn rejects_bad_window() {
    let a = fixture("a.txt");
    let b = fixture("b.txt");
    let out = run(&[a.as_str(), b.as_str(), "--window", "soon"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("invalid duration"), "stderr:\n{stderr}");
}

#[test]
fn rejects_repeated_window() {
    let a = fixture("a.txt");
    let b = fixture("b.txt");
    let out = run(&[a.as_str(), b.as_str(), "--window", "5s", "--window", "1s"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("--window given more than once"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn closed_stdout_exits_quietly() {
    // reader dropped before spawn, so every write hits a closed pipe (`| head`)
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let a = fixture("a.txt");
    let b = fixture("b.txt");
    let out = Command::new(env!("CARGO_BIN_EXE_ts-correlate"))
        .args([a.as_str(), b.as_str()])
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .expect("run correlate");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
}
