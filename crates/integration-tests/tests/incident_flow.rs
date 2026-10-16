//! Golden regression: a synthetic 502 incident. Piping the toolbox the
//! way we would during triage must still rediscover the known peak.
//!
//! `events.txt` is the raw stream; `expected.txt` is burst's summary block for
//! the incident window. We compare the summary (the *finding*), not the histogram
//! bars (cosmetic). If you change burst's summary format on purpose, regenerate:
//!
//!     cat events.txt \
//!       | ts-window --from 2026-09-03T10:21:29+09:00 --to 2026-09-03T10:21:33+09:00 \
//!       | ts-burst | awk 'f{print} /^$/{f=1}' > expected.txt

use integration_tests::{fixture, run};

#[test]
fn rediscovers_the_502_peak() {
    let events = std::fs::read(fixture("alb-502-incident/events.txt")).unwrap();

    // triage flow: narrow to the incident window, then find the burst
    let windowed = run(
        "ts-window",
        &[
            "--from",
            "2026-09-03T10:21:29+09:00",
            "--to",
            "2026-09-03T10:21:33+09:00",
        ],
        &events,
    );
    assert!(windowed.status.success());

    let shaped = run("ts-burst", &[], &windowed.stdout);
    assert!(shaped.status.success());
    let out = String::from_utf8(shaped.stdout).unwrap();

    let summary = out.split("\n\n").nth(1).unwrap_or_default().trim_end();
    let expected = std::fs::read_to_string(fixture("alb-502-incident/expected.txt")).unwrap();
    assert_eq!(summary, expected.trim_end(), "\nfull burst output:\n{out}");
}
