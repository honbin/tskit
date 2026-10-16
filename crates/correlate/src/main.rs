//! `ts-correlate` — how often two timestamped event streams co-occur in time.
//!
//! Reads one RFC3339 timestamp per line from two files and reports, in both
//! directions, what fraction of one stream's events have an event in the other
//! within ±window. Both directions matter: when the streams differ in size,
//! "71% of 502s were near a SIGTERM" and "83% of SIGTERMs were near a 502" are
//! different facts.
//!
//! example:
//!     ts-correlate 502.txt sigterm.txt --window 5s

use std::process::ExitCode;

use correlate::{count_matched, parse_duration_secs};

const HELP: &str = "\
ts-correlate — how often two timestamped event streams co-occur in time

usage:
    ts-correlate <A> <B> [--window <dur>]

Reads one RFC3339 timestamp per line from files A and B, and reports — in both
directions — what fraction of one stream's events have an event in the other
within ±window. --window accepts 5s / 2m / 1h / bare seconds (default 1s).

example:
    ts-correlate 502.txt sigterm.txt --window 5s";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("ts-correlate: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut files: Vec<String> = Vec::new();
    let mut window_arg: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--window" => window_arg = Some(args.next().ok_or("--window requires a value")?),
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(ExitCode::SUCCESS);
            }
            s if s.starts_with("--") => return Err(format!("unknown argument: {s}\n\n{HELP}")),
            _ => files.push(arg),
        }
    }

    if files.len() != 2 {
        return Err(format!(
            "need exactly two files (got {})\n\n{HELP}",
            files.len()
        ));
    }
    let window_secs = match window_arg {
        Some(s) => parse_duration_secs(&s)?,
        None => 1,
    };
    // checked, not saturating: silently shrinking a huge --window gives a wrong
    // result (not just a cap), so reject an over-range window instead
    let window_ns = window_secs
        .checked_mul(1_000_000_000)
        .ok_or_else(|| format!("--window {window_secs}s is too large (max ~292 years)"))?;

    let (a, a_skipped) = load(&files[0])?;
    let (b, b_skipped) = load(&files[1])?;

    let a_matched = count_matched(&a, &b, window_ns);
    let b_matched = count_matched(&b, &a, window_ns);

    println!("window    ±{window_secs}s");
    println!();
    report(&files[0], a.len(), a_matched, a_skipped);
    println!();
    report(&files[1], b.len(), b_matched, b_skipped);
    Ok(ExitCode::SUCCESS)
}

/// Read a file of timestamps into sorted epoch nanoseconds, plus the count of
/// unparseable (non-blank) lines. Errors if the file holds no timestamps.
///
/// Nanoseconds (the parser's full resolution): any coarser unit would collapse
/// sub-unit gaps and compare distinct instants as within the window.
fn load(path: &str) -> Result<(Vec<i64>, u64), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut nanos: Vec<i64> = Vec::new();
    let mut skipped = 0u64;
    for line in text.lines() {
        // timestamp_nanos_opt is None only for dates outside ~1677–2262; treat
        // those like unparseable lines.
        match tstamp::parse_line(line).and_then(|dt| dt.timestamp_nanos_opt()) {
            Some(ns) => nanos.push(ns),
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }
    if nanos.is_empty() {
        return Err(format!("{path}: no timestamps found"));
    }
    nanos.sort_unstable();
    Ok((nanos, skipped))
}

fn report(name: &str, events: usize, matched: usize, skipped: u64) {
    let pct = if events > 0 {
        matched as f64 / events as f64 * 100.0
    } else {
        0.0
    };
    println!("{name}");
    println!("  events   {events:>8}");
    println!("  matched  {matched:>8}   {pct:.1}%");
    if skipped > 0 {
        eprintln!("ts-correlate: {name}: skipped {skipped} unparseable line(s)");
    }
}
