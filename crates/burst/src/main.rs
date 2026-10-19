//! `ts-burst` — detect bursts in a stream of timestamped events.
//!
//! Reads one timestamp per line on stdin, buckets events per second, and prints
//! a histogram plus headline numbers (total / peak rate / first / last / span).
//!
//! Timestamps are parsed as RFC3339 (offset required). Every line must carry the
//! same UTC offset — mixed offsets are rejected — and output is shown in that
//! offset, so the times line up with the source log and CloudWatch without any
//! mental conversion. (A `--utc` / `--offset` flag can relax this later.)
//!
//! Examples:
//!     jq -r 'select(.status == 502) | .timestamp' alb.jsonl | ts-burst
//!     grep SIGTERM app.log | awk '{print $1}' | ts-burst

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use burst::{Summary, counts_by_second, summarize};
use chrono::{FixedOffset, TimeZone};
use tstamp::parse_line;

/// The longest bar renders at this many blocks; everything else scales to it.
const BAR_WIDTH: usize = 30;
/// Fill empty seconds (so the shape is visible) only when the span is at most
/// this many buckets. Beyond it, print just the non-empty seconds to avoid
/// spraying thousands of zero rows.
const FILL_LIMIT: i64 = 3600;

const HELP: &str = "\
ts-burst — per-second histogram of timestamped events

usage:
    <timestamps> | ts-burst
    ts-burst -h | --help

Reads one RFC3339 timestamp per line on stdin and prints a per-second histogram
plus total / peak / first / last / span. All lines must share one UTC offset.

example:
    jq -r 'select(.status==502) | .time' alb.jsonl | ts-burst";

fn main() -> io::Result<()> {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => {
                eprintln!("ts-burst: unexpected argument: {other}\n\n{HELP}");
                std::process::exit(1);
            }
        }
    }
    let mut epoch_secs: Vec<i64> = Vec::new();
    let mut offset: Option<FixedOffset> = None;
    let mut skipped: u64 = 0;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        match parse_line(&line) {
            Some(dt) => {
                let this = *dt.offset();
                match offset {
                    None => offset = Some(this),
                    // Keep one display offset for the whole stream so rendered
                    // times stay aligned with the source log.
                    Some(first) if first != this => {
                        eprintln!("ts-burst: mixed UTC offsets: {first} and {this}");
                        std::process::exit(1);
                    }
                    Some(_) => {}
                }
                epoch_secs.push(dt.timestamp());
            }
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }

    if skipped > 0 {
        eprintln!("ts-burst: skipped {skipped} unparseable line(s)");
    }

    let counts = counts_by_second(&epoch_secs);
    let Some(summary) = summarize(&counts) else {
        eprintln!(
            "ts-burst: no parseable timestamps on stdin \
             (expected RFC3339 with offset, e.g. 2026-09-03T10:21:31.124+09:00)"
        );
        std::process::exit(1);
    };
    // summarize returned Some, so at least one line parsed and set the offset.
    let offset = offset.expect("offset is set whenever a timestamp parses");

    match render(&counts, &summary, offset) {
        // Downstream closed the pipe (e.g. `| head`): stop quietly.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn render(counts: &BTreeMap<i64, u64>, summary: &Summary, offset: FixedOffset) -> io::Result<()> {
    let span = summary.last_sec - summary.first_sec;
    let fill = span <= FILL_LIMIT;
    if !fill {
        eprintln!(
            "ts-burst: span {span}s over fill limit {FILL_LIMIT}s; showing non-empty seconds only"
        );
    }

    let mut out = io::BufWriter::new(io::stdout().lock());

    let scale = summary.peak_count.max(1);
    if fill {
        for sec in summary.first_sec..=summary.last_sec {
            write_row(
                &mut out,
                offset,
                sec,
                *counts.get(&sec).unwrap_or(&0),
                scale,
            )?;
        }
    } else {
        for (&sec, &count) in counts {
            write_row(&mut out, offset, sec, count, scale)?;
        }
    }

    writeln!(out)?;
    writeln!(out, "events   {:>8}", summary.total)?;
    writeln!(
        out,
        "peak     {:>8}  /s at {}",
        summary.peak_count,
        fmt_time(offset, summary.peak_sec)
    )?;
    writeln!(out, "first    {}", fmt_time(offset, summary.first_sec))?;
    writeln!(out, "last     {}", fmt_time(offset, summary.last_sec))?;
    writeln!(out, "span     {span}s (first→last)")?;
    out.flush()
}

fn write_row(
    out: &mut impl Write,
    offset: FixedOffset,
    sec: i64,
    count: u64,
    scale: u64,
) -> io::Result<()> {
    let blocks = (count as f64 / scale as f64 * BAR_WIDTH as f64).round() as usize;
    writeln!(
        out,
        "{}  {:>4}  {}",
        fmt_time(offset, sec),
        count,
        "█".repeat(blocks)
    )
}

/// Format an epoch second as `HH:MM:SS` in the input's (validated single) UTC
/// offset, so the histogram reads in the same local time as the source log.
fn fmt_time(offset: FixedOffset, epoch_sec: i64) -> String {
    offset
        .timestamp_opt(epoch_sec, 0)
        .single()
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| epoch_sec.to_string())
}
