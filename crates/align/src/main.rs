//! `ts-align` — inner-join timestamped series on their timestamp.
//!
//! Each file is `RFC3339 <ws> number` (same format as `series`). For every
//! timestamp present in ALL files, prints one tab-separated row:
//!
//!     <RFC3339>\t<value from A>\t<value from B>\t...
//!
//! sorted by time. Only the join lives here — the awkward part to do in shell
//! (nested `join`). The derived metric and its peak are a downstream step, which
//! keeps align single-purpose and its output re-parseable by `series`:
//!
//!     ts-align req.txt lat.txt tasks.txt \
//!       | awk -F'\t' '{printf "%s\t%.2f\n", $1, $2/60*$3/$4}' \
//!       | ts-series

use std::collections::BTreeMap;
use std::process::ExitCode;

use align::inner_join;
use chrono::{DateTime, FixedOffset, Utc};
use tspoint::parse_point;

/// A `(timestamp, value)` series keyed by UTC instant.
type Series = BTreeMap<DateTime<Utc>, f64>;

const HELP: &str = "\
ts-align — inner-join timestamped series on their timestamp

usage:
    ts-align <A> <B> [C ...]

Each file is `RFC3339 <ws> number`, one point per line (same format as series;
order doesn't matter). For every timestamp present in ALL files, prints:

    <RFC3339>  <value from A>  <value from B>  ...   (tab-separated)

Rows re-parse cleanly, so a derived metric and its peak are a downstream step
(awk + series) — align does only the join. See the README for an example.";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("ts-align: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut files: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(ExitCode::SUCCESS);
            }
            s if s.starts_with("--") => return Err(format!("unknown argument: {s}\n\n{HELP}")),
            _ => files.push(arg),
        }
    }
    if files.len() < 2 {
        return Err(format!(
            "need at least two files (got {})\n\n{HELP}",
            files.len()
        ));
    }

    let mut maps: Vec<Series> = Vec::with_capacity(files.len());
    let mut totals: Vec<usize> = Vec::with_capacity(files.len());
    let mut offset: Option<FixedOffset> = None;
    for path in &files {
        let (map, off, n) = load(path)?;
        offset.get_or_insert(off);
        totals.push(n);
        maps.push(map);
    }
    // every file had ≥1 point (load errors otherwise), so the offset is set
    let offset = offset.expect("at least one point parsed");

    let rows = inner_join(&maps);

    for (instant, values) in &rows {
        // render in the input's display offset; values go out at full precision
        // (align feeds downstream math — rounding here would corrupt it)
        let mut line = instant.with_timezone(&offset).to_rfc3339();
        for v in values {
            line.push('\t');
            line.push_str(&v.to_string());
        }
        println!("{line}");
    }

    // report what the inner join dropped, so non-common timestamps aren't silent
    let per_file = files
        .iter()
        .zip(&totals)
        .map(|(f, n)| format!("{f}={n}"))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "ts-align: {} timestamps common to all {} series ({per_file})",
        rows.len(),
        files.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Read a file of `timestamp value` points into a map keyed by UTC instant
/// (last value wins on a duplicate timestamp), plus the input's offset and point
/// count. Errors if the file holds no points.
fn load(path: &str) -> Result<(Series, FixedOffset, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut map = BTreeMap::new();
    let mut offset = None;
    for line in text.lines() {
        if let Some((dt, value)) = parse_point(line) {
            offset.get_or_insert_with(|| *dt.offset());
            // key by the UTC instant (full sub-second precision): truncating to
            // seconds would collide distinct points and join non-equal instants
            map.insert(dt.to_utc(), value);
        }
    }
    match offset {
        Some(off) => {
            let n = map.len();
            Ok((map, off, n))
        }
        None => Err(format!("{path}: no 'timestamp value' points found")),
    }
}
