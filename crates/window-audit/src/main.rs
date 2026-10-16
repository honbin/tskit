//! `ts-window-audit` — report where observed events sit inside a requested
//! window, and whether the input reached the fetch's row limit.
//!
//! A Unix filter: reads RFC3339 timestamps from stdin (leading token per line),
//! and with the stated --from/--to (and optional --limit) reports the observed
//! range, the gap at each window edge, and verdict-free signals (EMPTY /
//! LIMIT_REACHED / OUT_OF_WINDOW). It draws no conclusion about *why* a gap
//! exists — the reader connects it to the query's sort order and conditions.
//!
//!     cat alb.jsonl | jq -r .time |
//!       ts-window-audit --from … --to … --limit 10000
//!
//! `--limit` is compared against the input row count (parsed + unparseable, since
//! the fetch cap counts both), which may still differ from the original fetch
//! count if blank lines, headers, or an upstream filter sit in between. Rows
//! dropped before this input are invisible to it.

use std::fmt::Write as _;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use chrono::{DateTime, Duration, FixedOffset};
use tstamp::parse_line;
use window_audit::{Audit, Observed, Signal, Window, audit};

const HELP: &str = "\
ts-window-audit — report the observed range of stdin timestamps in a window

usage:
    ts-window-audit --from <RFC3339> --to <RFC3339> [--limit <N>]

Reads RFC3339 timestamps from stdin (leading token per line). Reports the window,
the observed first/last, the gap at each edge, and signals (EMPTY, LIMIT_REACHED,
OUT_OF_WINDOW). Facts only, no verdict: a gap means 'nothing observed here', not
that events were lost; LIMIT_REACHED means the input rows reached --limit, not
that rows were truncated. --limit must be positive; it is compared against input
rows (parsed + unparseable), which may differ from the original fetch count.

example:
    cat alb.jsonl | jq -r .time |
      ts-window-audit --from 2026-10-10T10:00:00+09:00 \\
                            --to   2026-10-10T11:00:00+09:00 --limit 10000";

struct Args {
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
    limit: Option<u64>,
}

fn parse_args() -> Result<Args, String> {
    let mut from = None;
    let mut to = None;
    let mut limit = None;
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
            "--limit" => {
                let v = args.next().ok_or("--limit requires a number")?;
                let n = v.parse::<u64>().map_err(|e| format!("--limit: {e}"))?;
                if n == 0 {
                    return Err("--limit must be a positive number".into());
                }
                limit = Some(n);
            }
            other => return Err(format!("unknown argument: {other}\n\n{HELP}")),
        }
    }
    match (from, to) {
        (Some(f), Some(t)) if f > t => Err(format!("--from ({f}) is after --to ({t})")),
        (Some(from), Some(to)) => Ok(Args { from, to, limit }),
        _ => Err(format!("both --from and --to are required\n\n{HELP}")),
    }
}

/// Human-readable signed duration: `8m`, `1h 5m`, `45s`, `-2m`, `-500ms`, `<1ms`,
/// `>-1ms`, `0s`. Sub-second magnitudes are shown as ms, and a non-zero gap under
/// 1ms as `<1ms` / `>-1ms`, so a non-zero gap never prints as `0s`.
fn fmt_dur(d: Duration) -> String {
    let ms = d.num_milliseconds();
    // num_milliseconds() truncates sub-ms to 0; tell a true zero from a tiny gap.
    if ms == 0 {
        return if d.is_zero() {
            "0s".to_string()
        } else if d < Duration::zero() {
            ">-1ms".to_string()
        } else {
            "<1ms".to_string()
        };
    }
    let sign = if ms < 0 { "-" } else { "" };
    let total = ms.unsigned_abs();
    if total < 1000 {
        return format!("{sign}{total}ms");
    }
    let s = total / 1000;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if sec > 0 || parts.is_empty() {
        parts.push(format!("{sec}s"));
    }
    format!("{sign}{}", parts.join(" "))
}

/// RFC3339 in the input's own offset; sub-second digits appear only when the
/// timestamp has them, so an observed range at ms resolution isn't shown as whole
/// seconds while the gaps are computed from the full-precision instant.
fn fmt_ts(t: DateTime<FixedOffset>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

fn signal_name(s: &Signal) -> &'static str {
    match s {
        Signal::Empty => "EMPTY",
        Signal::LimitReached => "LIMIT_REACHED",
        Signal::OutOfWindow => "OUT_OF_WINDOW",
    }
}

fn render(
    window: &Window,
    observed: &Option<Observed>,
    rows: u64,
    skipped: u64,
    limit: Option<u64>,
    result: &Audit,
) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{:<10}{} → {}",
        "window",
        fmt_ts(window.from),
        fmt_ts(window.to)
    );
    match observed {
        Some(o) => {
            let _ = writeln!(
                s,
                "{:<10}{} → {}",
                "observed",
                fmt_ts(o.first),
                fmt_ts(o.last)
            );
        }
        None => {
            let _ = writeln!(s, "{:<10}(none)", "observed");
        }
    }
    match limit {
        Some(l) => {
            let _ = writeln!(s, "{:<10}{rows} / {l}", "rows");
        }
        None => {
            let _ = writeln!(s, "{:<10}{rows}", "rows");
        }
    }
    if skipped > 0 {
        let _ = writeln!(s, "{:<10}{skipped}", "unparsed");
    }
    if let (Some(sg), Some(eg)) = (result.start_gap, result.end_gap) {
        let _ = writeln!(s);
        let _ = writeln!(s, "{:<10}{}", "start gap", fmt_dur(sg));
        let _ = writeln!(s, "{:<10}{}", "end gap", fmt_dur(eg));
    }
    let _ = writeln!(s);
    if result.signals.is_empty() {
        let _ = writeln!(s, "{:<10}(none)", "signals:");
    } else {
        let _ = writeln!(s, "signals:");
        for sig in &result.signals {
            let _ = writeln!(s, "  {}", signal_name(sig));
        }
    }
    s
}

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|a| a == "-h" || a == "--help") {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("ts-window-audit: {msg}");
            return ExitCode::FAILURE;
        }
    };

    let mut parsed = 0u64;
    let mut skipped = 0u64;
    let mut first: Option<DateTime<FixedOffset>> = None;
    let mut last: Option<DateTime<FixedOffset>> = None;
    for line in io::stdin().lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("ts-window-audit: read error: {e}");
                return ExitCode::FAILURE;
            }
        };
        match parse_line(&line) {
            Some(ts) => {
                parsed += 1;
                if first.is_none_or(|f| ts < f) {
                    first = Some(ts);
                }
                if last.is_none_or(|l| ts > l) {
                    last = Some(ts);
                }
            }
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }
    let rows = parsed + skipped;

    let window = Window {
        from: args.from,
        to: args.to,
    };
    let observed = match (first, last) {
        (Some(first), Some(last)) => Some(Observed { first, last }),
        _ => None,
    };
    let result = audit(&window, observed.as_ref(), rows, args.limit);
    let report = render(&window, &observed, rows, skipped, args.limit, &result);

    if let Err(e) = io::stdout().write_all(report.as_bytes()) {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return ExitCode::SUCCESS;
        }
        eprintln!("ts-window-audit: write error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_dur_sub_millisecond_is_not_zero() {
        assert_eq!(fmt_dur(Duration::zero()), "0s");
        assert_eq!(fmt_dur(Duration::microseconds(500)), "<1ms");
        assert_eq!(fmt_dur(Duration::microseconds(-500)), ">-1ms");
        assert_eq!(fmt_dur(Duration::nanoseconds(1)), "<1ms");
    }

    #[test]
    fn fmt_ts_keeps_sub_second_only_when_present() {
        let whole = DateTime::parse_from_rfc3339("2026-10-10T10:08:00+09:00").unwrap();
        assert_eq!(fmt_ts(whole), "2026-10-10T10:08:00+09:00");
        let frac = DateTime::parse_from_rfc3339("2026-10-10T10:08:00.600+09:00").unwrap();
        assert_eq!(fmt_ts(frac), "2026-10-10T10:08:00.600+09:00");
    }

    #[test]
    fn fmt_dur_scales() {
        assert_eq!(fmt_dur(Duration::milliseconds(500)), "500ms");
        assert_eq!(fmt_dur(Duration::milliseconds(-500)), "-500ms");
        assert_eq!(fmt_dur(Duration::seconds(45)), "45s");
        assert_eq!(fmt_dur(Duration::minutes(8)), "8m");
        assert_eq!(fmt_dur(Duration::minutes(-2)), "-2m");
        assert_eq!(fmt_dur(Duration::seconds(3665)), "1h 1m 5s");
    }
}
