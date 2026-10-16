//! Composition: a stream through `window` into `burst`.

use integration_tests::run;

const STREAM: &str = "\
2026-09-03T10:21:29+09:00 before
2026-09-03T10:21:30+09:00 in-1
2026-09-03T10:21:31+09:00 in-2
2026-09-03T10:21:31+09:00 in-3
2026-09-03T10:21:40+09:00 after
";

#[test]
fn window_then_burst_shapes_only_the_kept_lines() {
    // narrow to [10:21:30, 10:21:35], then look at the shape
    let windowed = run(
        "ts-window",
        &[
            "--from",
            "2026-09-03T10:21:30+09:00",
            "--to",
            "2026-09-03T10:21:35+09:00",
        ],
        STREAM.as_bytes(),
    );
    assert!(windowed.status.success());

    let shaped = run("ts-burst", &[], &windowed.stdout);
    assert!(shaped.status.success());
    let out = String::from_utf8(shaped.stdout).unwrap();

    // kept: 10:21:30 (x1) and 10:21:31 (x2) -> peak at :31
    assert!(out.contains("/s at 10:21:31"), "out:\n{out}");
    assert!(out.contains("10:21:30"), "out:\n{out}");
    // dropped by window, so burst never sees them
    assert!(!out.contains("10:21:29"), "out:\n{out}");
    assert!(!out.contains("10:21:40"), "out:\n{out}");
}

#[test]
fn window_filtering_everything_makes_burst_fail() {
    // a window far in the future keeps nothing; burst on empty stdin exits 1
    let windowed = run(
        "ts-window",
        &["--from", "2027-01-01T00:00:00+09:00"],
        STREAM.as_bytes(),
    );
    assert!(windowed.status.success());
    assert!(windowed.stdout.is_empty());

    let shaped = run("ts-burst", &[], &windowed.stdout);
    assert!(!shaped.status.success());
}
