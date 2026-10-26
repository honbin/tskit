//! End-to-end smoke tests: run the built binary with args and stdin.
//! Uses `CARGO_BIN_EXE_ts-window` (set by Cargo) — std only, no dev-dependencies.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-window"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn window");
    // ignore BrokenPipe: the child may exit before reading stdin (e.g. arg error)
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait window")
}

#[test]
fn keeps_in_range_lines_verbatim() {
    let out = run(
        &[
            "--from",
            "2026-09-03T10:21:30+09:00",
            "--to",
            "2026-09-03T10:21:35+09:00",
        ],
        include_str!("fixtures/log.txt"),
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    // original lines (with trailing text) preserved, out-of-range dropped
    assert_eq!(
        stdout,
        "2026-09-03T10:21:31+09:00 inside A\n2026-09-03T10:21:32+09:00 inside B\n"
    );
}

#[test]
fn open_ended_from_only() {
    let out = run(
        &["--from", "2026-09-03T10:21:32+09:00"],
        include_str!("fixtures/log.txt"),
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "2026-09-03T10:21:32+09:00 inside B\n2026-09-03T10:21:40+09:00 after window\n"
    );
}

#[test]
fn requires_a_bound() {
    let out = run(&[], "2026-09-03T10:21:31+09:00 x\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("at least one of --from / --to"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn rejects_bad_bound() {
    let out = run(&["--from", "nonsense"], "");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--from:"), "stderr:\n{stderr}");
}

#[test]
fn rejects_from_after_to() {
    let out = run(
        &[
            "--from",
            "2026-10-26T10:21:35+09:00",
            "--to",
            "2026-10-26T10:21:30+09:00",
        ],
        "",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("is after"), "stderr:\n{stderr}");
}

#[test]
fn equal_bounds_keep_that_instant() {
    let at = "2026-10-26T10:21:31+09:00";
    let out = run(
        &["--from", at, "--to", at],
        "2026-10-26T10:21:30+09:00 before\n2026-10-26T10:21:31+09:00 at\n",
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "2026-10-26T10:21:31+09:00 at\n");
}

#[test]
fn field_reads_jsonl_and_keeps_records_verbatim() {
    let input = r#"{"time":"2026-09-03T10:21:29+09:00","status":502}
{"time":"2026-09-03T10:21:31+09:00","status":502}
{"status":200}
{"time":1788398491}
"#;
    let out = run(
        &["--field", "time", "--from", "2026-09-03T10:21:30+09:00"],
        input,
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "{\"time\":\"2026-09-03T10:21:31+09:00\",\"status\":502}\n"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("kept 1 / 4 lines (skipped 2 unparseable)"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn rejects_empty_field() {
    let out = run(&["--field", "", "--from", "2026-09-03T10:21:30+09:00"], "");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("non-empty field name"), "stderr:\n{stderr}");
}

#[test]
fn rejects_repeated_flag() {
    let out = run(
        &[
            "--from",
            "2026-09-03T10:21:30+09:00",
            "--from",
            "2026-09-03T10:00:00+09:00",
        ],
        "",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("--from given more than once"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn closed_stdout_exits_quietly() {
    // reader dropped before spawn, so every write hits a closed pipe (`| head`);
    // a small output first fails at the final flush, a large one mid-stream
    let line = "2026-10-26T10:21:31+09:00 x\n";
    for stdin in [line.to_string(), line.repeat(1000)] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut child = Command::new(env!("CARGO_BIN_EXE_ts-window"))
            .args(["--from", "2026-10-26T10:21:30+09:00"])
            .stdin(Stdio::piped())
            .stdout(writer)
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn window");
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        let out = child.wait_with_output().expect("wait window");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(out.status.success(), "stderr:\n{stderr}");
        assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
    }
}
