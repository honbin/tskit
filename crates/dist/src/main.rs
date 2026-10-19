//! `ts-dist` — summarize the distribution of a column of numbers on stdin.
//!
//! Reads one number per line and prints count / min / mean / p50 / p90 / p95 /
//! p99 / max / stddev. Unit-agnostic: pipe whatever you want (ms, seconds,
//! request counts) and read the output in the same unit.
//!
//! Examples:
//!     jq -r .target_processing_time alb.jsonl | ts-dist
//!     grep responseTime app.log | awk '{print $NF}' | ts-dist

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use dist::{Stats, parse_number, summarize};

const HELP: &str = "\
ts-dist — summarize the distribution of a column of numbers

usage:
    <numbers> | ts-dist
    ts-dist -h | --help

Reads one number per line on stdin and prints count / min / mean / p50 / p90 /
p95 / p99 / max / stddev. Unit-agnostic (ms, seconds, counts — same unit out).

example:
    jq -r .target_processing_time alb.jsonl | ts-dist";

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("ts-dist: unexpected argument: {other}\n\n{HELP}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut values: Vec<f64> = Vec::new();
    let mut skipped: u64 = 0;

    for line in io::stdin().lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("ts-dist: read error: {e}");
                return ExitCode::FAILURE;
            }
        };
        match parse_number(&line) {
            Some(n) => values.push(n),
            None => {
                if !line.trim().is_empty() {
                    skipped += 1;
                }
            }
        }
    }

    if skipped > 0 {
        eprintln!("ts-dist: skipped {skipped} unparseable line(s)");
    }

    let Some(s) = summarize(&mut values) else {
        eprintln!("ts-dist: no numbers on stdin");
        return ExitCode::FAILURE;
    };

    let mut out = io::BufWriter::new(io::stdout().lock());
    if let Err(e) = render(&mut out, &s).and_then(|()| out.flush()) {
        // Downstream closed the pipe (e.g. `| head`): stop quietly.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return ExitCode::SUCCESS;
        }
        eprintln!("ts-dist: write error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn render(out: &mut impl Write, s: &Stats) -> io::Result<()> {
    writeln!(out, "count    {:>10}", s.count)?;
    writeln!(out, "min      {:>10.3}", s.min)?;
    writeln!(out, "mean     {:>10.3}", s.mean)?;
    writeln!(out, "p50      {:>10.3}", s.p50)?;
    writeln!(out, "p90      {:>10.3}", s.p90)?;
    writeln!(out, "p95      {:>10.3}", s.p95)?;
    writeln!(out, "p99      {:>10.3}", s.p99)?;
    writeln!(out, "max      {:>10.3}", s.max)?;
    writeln!(out, "stddev   {:>10.3}", s.stddev)
}
