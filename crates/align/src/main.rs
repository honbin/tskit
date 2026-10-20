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
use std::io::{self, Write};
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
    let mut offset: Option<FixedOffset> = None;
    for path in &files {
        let (map, off) = load(path)?;
        offset.get_or_insert(off);
        maps.push(map);
    }
    // every file had ≥1 point (load errors otherwise), so the offset is set
    let offset = offset.expect("at least one point parsed");

    let rows = inner_join(&maps);

    let mut out = io::BufWriter::new(io::stdout().lock());
    let written = rows
        .iter()
        .try_for_each(|(instant, values)| {
            // render in the input's display offset; values go out at full precision
            // (align feeds downstream math — rounding here would corrupt it)
            let mut line = instant.with_timezone(&offset).to_rfc3339();
            for v in values {
                line.push('\t');
                line.push_str(&v.to_string());
            }
            writeln!(out, "{line}")
        })
        .and_then(|()| out.flush());
    // A closed pipe (e.g. `| head`) only stops the rows: the summary below
    // describes the whole input, and is most needed when the rows were cut short.
    if let Err(e) = written
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        return Err(format!("write error: {e}"));
    }

    // report what the inner join dropped, so non-common timestamps aren't silent
    let per_file = files
        .iter()
        .zip(&maps)
        .map(|(f, map)| format!("{f}={}", map.len()))
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
/// (last value wins on a duplicate timestamp), plus the input's offset. Errors if
/// the file holds no points.
///
/// Unparseable lines and duplicate timestamps are reported on stderr: the
/// per-file count is of distinct timestamps, so without this both would vanish
/// from the summary unnoticed.
fn load(path: &str) -> Result<(Series, FixedOffset), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut map = BTreeMap::new();
    let mut offset = None;
    let (mut skipped, mut duplicates) = (0u64, 0u64);
    for line in text.lines() {
        match parse_point(line) {
            Some((dt, value)) => {
                offset.get_or_insert_with(|| *dt.offset());
                // key by the UTC instant (full sub-second precision): truncating to
                // seconds would collide distinct points and join non-equal instants
                if map.insert(dt.to_utc(), value).is_some() {
                    duplicates += 1;
                }
            }
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }
    if skipped > 0 {
        eprintln!("ts-align: {path}: skipped {skipped} unparseable line(s)");
    }
    if duplicates > 0 {
        eprintln!("ts-align: {path}: {duplicates} duplicate timestamp(s), last value kept");
    }
    match offset {
        Some(off) => Ok((map, off)),
        None => Err(format!("{path}: no 'timestamp value' points found")),
    }
}
