//! `ts-timeline` — lay out `timestamp value` series on a shared time axis
//! as a self-contained HTML page (inline SVG, no JavaScript, no external
//! assets). Every signal is the same shape, so a per-second log count and a
//! metric drop in the same way:
//!
//!     ts-timeline --series 502.txt --series cpu.txt --series latency.txt > report.html

use std::io::{self, Write};
use std::process::ExitCode;

use chrono::FixedOffset;
use timeline::{Series, parse_offset, render_html};

const HELP: &str = "\
ts-timeline — lay out timestamp/value series on a shared time axis

usage:
    ts-timeline [--display-offset ±HH:MM] --series <file> [--series <file>]... > report.html

Each --series file is `RFC3339 value` per line (same format as ts-series;
feed a 502 count via `stats count() by bin(1s)` the same way as a metric). Every
series is drawn as a line with a marker per point, on its own y-scale, stacked on
one shared time axis. The line only connects the points you supply — it does not
imply a value between them, so zero-fill gaps upstream if you need them. Writes a
self-contained HTML page (inline SVG, no JavaScript) to stdout. At least one
--series is required.

--display-offset shows axis and tooltip times at a fixed UTC offset (e.g.
`+09:00` for JST), leaving values and positions unchanged; defaults to the
input's offset.

example:
    ts-timeline --series 502.txt --series cpu.txt --series latency.txt > report.html";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("ts-timeline: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut files: Vec<String> = Vec::new();
    let mut display_offset: Option<FixedOffset> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--series" => files.push(args.next().ok_or("--series requires a file")?),
            "--display-offset" => {
                let v = args.next().ok_or("--display-offset requires a value")?;
                display_offset = Some(parse_offset(&v)?);
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(ExitCode::SUCCESS);
            }
            s if s.starts_with("--") => return Err(format!("unknown argument: {s}\n\n{HELP}")),
            other => return Err(format!("unexpected argument: {other}\n\n{HELP}")),
        }
    }

    if files.is_empty() {
        return Err(format!("need at least one --series\n\n{HELP}"));
    }

    // Display offset defaults to the first input timestamp's; --display-offset overrides.
    let mut input_offset: Option<FixedOffset> = None;
    let mut series = Vec::with_capacity(files.len());
    for path in &files {
        series.push(load_series(path, &mut input_offset)?);
    }
    let input_offset = input_offset.ok_or("no parseable timestamps in any input")?;

    let html = render_html(&series, display_offset.unwrap_or(input_offset));
    let mut out = io::stdout().lock();
    if let Err(e) = out.write_all(html.as_bytes()) {
        // Downstream closed the pipe (e.g. `| head`): stop quietly.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return Ok(ExitCode::SUCCESS);
        }
        return Err(format!("write error: {e}"));
    }
    Ok(ExitCode::SUCCESS)
}

/// Read `RFC3339 value` lines into a time-sorted series keyed by epoch
/// nanosecond (the parser's full resolution). Errors if the file holds no points.
fn load_series(path: &str, offset: &mut Option<FixedOffset>) -> Result<Series, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut points: Vec<(i64, f64)> = Vec::new();
    let mut skipped = 0u64;
    for line in text.lines() {
        match tspoint::parse_point(line) {
            // nanos keeps sub-millisecond points distinct; None only for dates
            // outside chrono's ~1677–2262 range, treated like an unparseable line.
            Some((ts, value)) => match ts.timestamp_nanos_opt() {
                Some(ns) => {
                    offset.get_or_insert(*ts.offset());
                    points.push((ns, value));
                }
                None => skipped += 1,
            },
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }
    if points.is_empty() {
        return Err(format!("{path}: no 'timestamp value' points found"));
    }
    if skipped > 0 {
        eprintln!("ts-timeline: {path}: skipped {skipped} unparseable line(s)");
    }
    points.sort_by_key(|&(t, _)| t);
    Ok(Series {
        name: path.to_string(),
        points,
    })
}
