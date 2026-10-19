//! End-to-end smoke tests: run the built binary against fixture files.
//! Uses `CARGO_BIN_EXE_ts-timeline` and `CARGO_MANIFEST_DIR` (set by Cargo).

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-timeline"))
        .args(args)
        .output()
        .expect("run timeline")
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn renders_self_contained_html() {
    let cpu = fixture("cpu.txt");
    let out = run(&["--series", cpu.as_str()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("<!doctype html"), "stdout:\n{stdout}");
    assert!(stdout.contains("<svg"), "stdout:\n{stdout}");
    assert!(stdout.contains("<polyline"), "stdout:\n{stdout}");
    assert!(
        !stdout.contains("<script"),
        "must not emit JavaScript:\n{stdout}"
    );
}

#[test]
fn needs_at_least_one_series() {
    let out = run(&[]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("need at least one --series"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn rejects_malformed_display_offset() {
    let cpu = fixture("cpu.txt");
    let out = run(&["--display-offset", "0900", "--series", cpu.as_str()]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("±HH:MM"), "stderr:\n{stderr}");
}

#[test]
fn display_offset_shifts_axis_and_tooltip() {
    let cpu = fixture("cpu.txt");

    // default: the axis label (a <text>) is in the input's offset (fixture is UTC)
    let utc = run(&["--series", cpu.as_str()]);
    assert!(utc.status.success());
    let utc_out = String::from_utf8(utc.stdout).unwrap();
    assert!(
        utc_out.contains(">00:00:00<"),
        "axis not input offset:\n{utc_out}"
    );

    // +09:00 shifts both the axis <text> and the tooltip <title> (00:00 UTC ->
    // 09:00 JST) — checked separately so one changing can't mask the other
    let jst = run(&["--display-offset", "+09:00", "--series", cpu.as_str()]);
    assert!(jst.status.success());
    let jst_out = String::from_utf8(jst.stdout).unwrap();
    assert!(
        jst_out.contains(">09:00:00<"),
        "axis not shifted:\n{jst_out}"
    );
    assert!(
        jst_out.contains("T09:00:00+09:00"),
        "tooltip not shifted:\n{jst_out}"
    );
}

#[test]
fn errors_on_a_file_with_no_points() {
    let bad = fixture("invalid.txt");
    let out = run(&["--series", bad.as_str()]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("no 'timestamp value' points found"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn rejects_repeated_display_offset() {
    let cpu = fixture("cpu.txt");
    let out = run(&[
        "--display-offset",
        "+09:00",
        "--display-offset",
        "+00:00",
        "--series",
        cpu.as_str(),
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("--display-offset given more than once"),
        "stderr:\n{stderr}"
    );
}
