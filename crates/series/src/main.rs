//! `ts-series` — view a `timestamp value` series as a sparkline over time.
//!
//! Reads `RFC3339 <ws> number` lines (order doesn't matter; sorted internally)
//! and prints one row per point — time, sparkline level, value — then points /
//! sum / min / max, each extreme tagged with when it happened. Takes any
//! `(timestamp, value)` series — e.g. CloudWatch datapoints:
//!
//!     aws cloudwatch get-metric-statistics ... \
//!       --query 'Datapoints[].[Timestamp,Sum]' --output text | ts-series
//!
//! Times are shown in the input's offset (like `burst`), and all points must
//! share one offset — mixed offsets are rejected.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use chrono::{DateTime, FixedOffset};
use series::block_index;
use tspoint::parse_point;

const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

const HELP: &str = "\
ts-series — view a timestamp/value series as a sparkline

usage:
    <RFC3339 value> | ts-series
    ts-series -h | --help

Reads `timestamp value` lines on stdin (order doesn't matter, sorted internally)
and prints a sparkline over time plus points / sum / min / max.

example:
    aws cloudwatch get-metric-statistics ... --output text | ts-series";

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("ts-series: unexpected argument: {other}\n\n{HELP}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut points: Vec<(DateTime<FixedOffset>, f64)> = Vec::new();
    let mut offset: Option<FixedOffset> = None;
    let mut skipped: u64 = 0;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("ts-series: read error: {e}");
                return ExitCode::FAILURE;
            }
        };
        match parse_point(&line) {
            Some((ts, value)) => {
                let this = *ts.offset();
                match offset {
                    None => offset = Some(this),
                    // Keep one display offset for the whole stream (same reasoning as burst).
                    Some(first) if first != this => {
                        eprintln!("ts-series: mixed UTC offsets: {first} and {this}");
                        return ExitCode::FAILURE;
                    }
                    Some(_) => {}
                }
                points.push((ts, value));
            }
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }

    if skipped > 0 {
        eprintln!("ts-series: skipped {skipped} unparseable line(s)");
    }
    if points.is_empty() {
        eprintln!("ts-series: no 'timestamp value' points on stdin");
        return ExitCode::FAILURE;
    }

    points.sort_by_key(|(ts, _)| *ts);
    let mut out = io::BufWriter::new(io::stdout().lock());
    if let Err(e) = render(&mut out, &points).and_then(|()| out.flush()) {
        // Downstream closed the pipe (e.g. `| head`): stop quietly.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return ExitCode::SUCCESS;
        }
        eprintln!("ts-series: write error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn render(out: &mut impl Write, points: &[(DateTime<FixedOffset>, f64)]) -> io::Result<()> {
    // Strict comparisons keep the first occurrence of each extreme.
    let (first, rest) = points.split_first().expect("points is non-empty");
    let (mut lo, mut hi) = (first, first);
    for p in rest {
        if p.1 < lo.1 {
            lo = p;
        }
        if p.1 > hi.1 {
            hi = p;
        }
    }
    let ((min_ts, min), (max_ts, max)) = (*lo, *hi);
    let sum: f64 = points.iter().map(|(_, v)| *v).sum();

    for (ts, value) in points {
        writeln!(
            out,
            "{}  {}  {:>10}",
            fmt_time(*ts),
            RAMP[block_index(*value, min, max)],
            fmt_val(*value)
        )?;
    }

    writeln!(out)?;
    writeln!(out, "points   {:>10}", points.len())?;
    writeln!(out, "sum      {:>10}", fmt_val(sum))?;
    writeln!(
        out,
        "min      {:>10}  at {}",
        fmt_val(min),
        fmt_time(min_ts)
    )?;
    writeln!(
        out,
        "max      {:>10}  at {}",
        fmt_val(max),
        fmt_time(max_ts)
    )?;
    Ok(())
}

/// Format an integer-valued float without a decimal tail (`103`, not `103.000`),
/// otherwise with three decimals (`0.145`).
fn fmt_val(v: f64) -> String {
    if v == 0.0 {
        // -0.0 compares equal to 0.0 but formats as "-0"
        return "0".to_string();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    if v.abs() < 0.0005 {
        return format!("{v}"); // would vanish as 0.000 at 3 decimals — keep the real value
    }
    format!("{v:.3}")
}

/// Format a timestamp as `HH:MM:SS` in its own offset, matching `burst` so the
/// toolbox reads times consistently.
fn fmt_time(ts: DateTime<FixedOffset>) -> String {
    ts.format("%H:%M:%S").to_string()
}
