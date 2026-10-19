//! `ts-window` — pass through stdin lines whose timestamp falls in a time range.
//!
//! A Unix filter: it reads the RFC3339 timestamp from each line, and if it lies
//! within [--from, --to] (inclusive), prints the *original line unchanged*. So
//! it composes in front of other tools:
//!
//!     cat alb.jsonl | ts-window --field time --from … --to … | jq -r .time | ts-burst
//!
//! Comparison is on the instant, so --from/--to may use any offset and lines may
//! mix offsets — window never reformats timestamps, it only decides keep/drop.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use chrono::{DateTime, FixedOffset};
use tstamp::parse_line;
use window::{in_range, parse_field};

const HELP: &str = "\
ts-window — pass through stdin lines whose RFC3339 timestamp is in [from, to]

usage:
    ts-window [--from <RFC3339>] [--to <RFC3339>] [--field <name>]

At least one bound is required; bounds are inclusive. The timestamp is read from
each line (leading token, surrounding quotes stripped) and the original line is
printed unchanged. --from/--to may use any UTC offset.

--field <name> treats each line as a JSON object (JSONL) and reads the timestamp
from its top-level string field <name> instead of the leading token.

examples:
    ts-window --from 2026-09-03T10:20:00+09:00 --to 2026-09-03T10:30:00+09:00
    cat alb.jsonl | ts-window --field time --from 2026-09-03T10:20:00+09:00";

struct Args {
    from: Option<DateTime<FixedOffset>>,
    to: Option<DateTime<FixedOffset>>,
    field: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut from = None;
    let mut to = None;
    let mut field = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => {
                let v = args.next().ok_or("--from requires an RFC3339 value")?;
                from = Some(DateTime::parse_from_rfc3339(&v).map_err(|e| format!("--from: {e}"))?);
            }
            "--to" => {
                let v = args.next().ok_or("--to requires an RFC3339 value")?;
                to = Some(DateTime::parse_from_rfc3339(&v).map_err(|e| format!("--to: {e}"))?);
            }
            "--field" => {
                let name = args.next().ok_or("--field requires a field name")?;
                if name.is_empty() {
                    return Err("--field requires a non-empty field name".into());
                }
                field = Some(name);
            }
            other => return Err(format!("unknown argument: {other}\n\n{HELP}")),
        }
    }
    match (from, to) {
        (None, None) => Err(format!("need at least one of --from / --to\n\n{HELP}")),
        (Some(f), Some(t)) if f > t => Err(format!("--from ({f}) is after --to ({t})")),
        _ => Ok(Args { from, to, field }),
    }
}

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|a| a == "-h" || a == "--help") {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("ts-window: {msg}");
            return ExitCode::FAILURE;
        }
    };

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let (mut kept, mut skipped, mut total) = (0u64, 0u64, 0u64);

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("ts-window: read error: {e}");
                return ExitCode::FAILURE;
            }
        };
        total += 1;
        let ts = match &args.field {
            Some(field) => parse_field(&line, field),
            None => parse_line(&line),
        };
        match ts {
            Some(ts) if in_range(ts, args.from, args.to) => {
                if let Err(e) = writeln!(out, "{line}") {
                    // Downstream closed the pipe (e.g. `| head`): stop quietly.
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return ExitCode::SUCCESS;
                    }
                    eprintln!("ts-window: write error: {e}");
                    return ExitCode::FAILURE;
                }
                kept += 1;
            }
            Some(_) => {} // parsed but out of range: drop
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }

    // With buffering, a downstream close (e.g. `| head`) can surface here instead
    // of mid-loop; treat it as a clean stop too, so the exit code stays consistent.
    if let Err(e) = out.flush() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return ExitCode::SUCCESS;
        }
        eprintln!("ts-window: write error: {e}");
        return ExitCode::FAILURE;
    }

    if skipped > 0 {
        eprintln!("ts-window: kept {kept} / {total} lines (skipped {skipped} unparseable)");
    } else {
        eprintln!("ts-window: kept {kept} / {total} lines");
    }
    ExitCode::SUCCESS
}
