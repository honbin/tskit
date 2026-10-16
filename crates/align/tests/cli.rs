//! End-to-end smoke tests: run the built binary against fixture files.
//! Uses `CARGO_BIN_EXE_ts-align` and `CARGO_MANIFEST_DIR` (set by Cargo).

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-align"))
        .args(args)
        .output()
        .expect("run align")
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn joins_on_common_timestamps() {
    let req = fixture("req.txt");
    let lat = fixture("lat.txt");
    let tasks = fixture("tasks.txt");
    let out = run(&[req.as_str(), lat.as_str(), tasks.as_str()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();

    // one row per timestamp present in all three, values in file order
    assert_eq!(
        stdout,
        "2026-09-03T10:20:00+09:00\t6000\t0.5\t6\n\
         2026-09-03T10:21:00+09:00\t9000\t1\t6\n\
         2026-09-03T10:22:00+09:00\t12000\t2\t8\n"
    );
    // 10:23 is only in lat.txt -> dropped by the inner join
    assert!(!stdout.contains("10:23"));

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("3 timestamps common to all 3"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn preserves_small_value_precision() {
    // align feeds downstream math, so a small value must not be rounded to 0.000
    let a = fixture("prec-a.txt");
    let b = fixture("prec-b.txt");
    let out = run(&[a.as_str(), b.as_str()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("0.0004"), "stdout:\n{stdout}");
}

#[test]
fn distinct_subsecond_instants_do_not_join() {
    // .100 and .200 are the same second but different instants, and neither
    // equals .900 — truncating to seconds would wrongly produce a joined row
    let a = fixture("subsec-a.txt");
    let b = fixture("subsec-b.txt");
    let out = run(&[a.as_str(), b.as_str()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.is_empty(), "expected no joined rows, got:\n{stdout}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("0 timestamps common to all 2"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn needs_two_files() {
    let req = fixture("req.txt");
    let out = run(&[req.as_str()]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("at least two files"), "stderr:\n{stderr}");
}
