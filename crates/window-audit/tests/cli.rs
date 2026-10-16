//! End-to-end smoke tests: run the built binary with args and stdin.
//! Uses `CARGO_BIN_EXE_ts-window-audit` (set by Cargo) — std only.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ts-window-audit"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn window-audit");
    // ignore BrokenPipe: the child may exit before reading stdin (e.g. arg error)
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait window-audit")
}

const WINDOW: [&str; 4] = [
    "--from",
    "2026-10-10T10:00:00+09:00",
    "--to",
    "2026-10-10T11:00:00+09:00",
];

fn with<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    WINDOW
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .collect()
}

#[test]
fn reports_range_gaps_and_no_signals() {
    let out = run(
        &with(&["--limit", "10000"]),
        "2026-10-10T10:08:00+09:00\n2026-10-10T10:30:00+09:00\n2026-10-10T10:47:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("3 / 10000"), "{s}");
    assert!(s.contains("8m"), "{s}"); // start gap
    assert!(s.contains("13m"), "{s}"); // end gap
    assert!(s.contains("(none)"), "{s}");
}

#[test]
fn limit_reached_when_rows_hit_cap() {
    let out = run(
        &with(&["--limit", "3"]),
        "2026-10-10T10:08:00+09:00\n2026-10-10T10:30:00+09:00\n2026-10-10T10:47:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("LIMIT_REACHED"), "{s}");
}

#[test]
fn limit_reached_counts_unparseable_rows() {
    // 2 parsed + 1 unparseable = 3 input rows == cap: the limit must still show.
    let out = run(
        &with(&["--limit", "3"]),
        "2026-10-10T10:08:00+09:00\nnot a timestamp\n2026-10-10T10:47:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("3 / 3"), "{s}"); // rows (not parsed count)
    assert!(s.contains("unparsed"), "{s}");
    assert!(s.contains("LIMIT_REACHED"), "{s}");
}

#[test]
fn empty_input_reports_empty() {
    let out = run(&WINDOW, "");
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("observed") && s.contains("(none)"), "{s}");
    assert!(s.contains("EMPTY"), "{s}");
}

#[test]
fn unsorted_input_tracks_first_and_last() {
    // Latest line first: the observed range must still be 10:08 → 10:47.
    let out = run(
        &WINDOW,
        "2026-10-10T10:47:00+09:00\n2026-10-10T10:08:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("8m"), "{s}"); // start gap from the true earliest
    assert!(s.contains("13m"), "{s}"); // end gap from the true latest
}

#[test]
fn out_of_window_flags_and_negative_gap() {
    let out = run(
        &WINDOW,
        "2026-10-10T09:58:00+09:00\n2026-10-10T10:47:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("OUT_OF_WINDOW"), "{s}");
    assert!(s.contains("-2m"), "{s}");
}

#[test]
fn observed_keeps_sub_second_precision() {
    let out = run(
        &WINDOW,
        "2026-10-10T10:08:00.600+09:00\n2026-10-10T10:47:00+09:00\n",
    );
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("10:08:00.600"), "{s}");
}

#[test]
fn requires_both_bounds() {
    let out = run(&["--from", "2026-10-10T10:00:00+09:00"], "");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("both --from and --to"), "{stderr}");
}

#[test]
fn rejects_from_after_to() {
    let out = run(
        &[
            "--from",
            "2026-10-10T11:00:00+09:00",
            "--to",
            "2026-10-10T10:00:00+09:00",
        ],
        "",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("is after"), "{stderr}");
}

#[test]
fn rejects_bad_bound() {
    let out = run(
        &["--from", "nonsense", "--to", "2026-10-10T11:00:00+09:00"],
        "",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--from:"), "{stderr}");
}

#[test]
fn rejects_zero_limit() {
    let out = run(&with(&["--limit", "0"]), "");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--limit must be a positive"), "{stderr}");
}
