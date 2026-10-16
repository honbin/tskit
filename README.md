# tskit

Small Unix-style tools for exploring the shape of timestamped data — waveforms,
distributions, co-occurrence, and window coverage in logs and time series.
Written in Rust, born out of incident triage.

Each tool reads from stdin, does one thing, and writes to stdout — so they
compose naturally with `grep` / `jq` / `awk` and with each other.

## Tools

| tool              | what it does                                        |
|-------------------|-----------------------------------------------------|
| `ts-burst`        | Detect bursts in timestamped events                 |
| `ts-window`       | Pass through lines whose timestamp is in a range    |
| `ts-window-audit` | Report the observed timestamp range inside a window |
| `ts-dist`         | Summarize the distribution of a column of numbers   |
| `ts-correlate`    | How often two timestamped streams co-occur in time  |
| `ts-series`       | View a `timestamp value` series as a sparkline      |
| `ts-align`        | Inner-join timestamped series on their timestamp    |
| `ts-timeline`     | Stack timestamp/value series on a shared time axis  |

The binaries are namespaced (`ts-*`) so they don't clash with other tools
on your `PATH`; alias them to shorter names if you like.

## Install

Install the `ts-*` binaries straight from GitHub:

```
cargo install --git https://github.com/honbin/tskit --locked \
  burst window window-audit dist correlate series align timeline
```

Cargo installs them to its default bin directory (`~/.cargo/bin`) — make sure
that's on your `PATH`.

Install only the tools you want — just name fewer:

```
cargo install --git https://github.com/honbin/tskit --locked burst
```

Developing from a local clone? Use `--path` instead:

```
cargo install --locked --path crates/burst
```

## Timestamps

Every tool expects RFC3339 timestamps with an explicit offset, such as
`2026-10-03T10:00:00+09:00` or `…Z`. A shared timestamp contract keeps the
tools composable and avoids guessing:

- **unit** — bare epoch values don't say whether they are seconds,
  milliseconds, or microseconds.
- **display offset** — `ts-burst`, `ts-series`, and
  `ts-timeline` render times in the input's offset, which an epoch value
  doesn't carry.

Other formats should be normalized to RFC3339 before piping them into
tskit — a job for the shell. The examples use `gawk` (macOS: `brew
install gawk`):

```sh
# epoch seconds -> RFC3339 UTC ($1/1000 for ms)
gawk '{ print strftime("%Y-%m-%dT%H:%M:%SZ", $1, 1) }'

# Apache/CLF  [03/Oct/2026:10:21:31 +0900]  ->  2026-10-03T10:21:31+09:00
gawk 'BEGIN { split("Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec", a, " ")
              for (i = 1; i <= 12; i++) m[a[i]] = sprintf("%02d", i) }
      match($0, /([0-9]{2})\/([A-Za-z]{3})\/([0-9]{4}):([0-9:]{8}) ([+-][0-9]{2})([0-9]{2})/, x) {
          print x[3]"-"m[x[2]]"-"x[1]"T"x[4] x[5]":"x[6] }'
```

## Layout

A Cargo workspace; one crate per tool under `crates/`, plus two small shared
crates: `tstamp` (just the RFC3339 timestamp parser) and `tspoint` (parses one
`timestamp value` line into a `(timestamp, value)` point, on top of `tstamp`).
Each was extracted on the rule of three, once a third tool needed the identical
code; both stay narrow on purpose — durations, histograms, and formatting do not
belong in them. Tools still duplicate trivial helpers until duplication is
proven.

## Build & test

```
cargo build
cargo test
```

Three tiers, each narrower and cheaper than the last:

- **unit** (`src/lib.rs` per crate) — pure logic: parse / bucket / summarize.
- **per-tool CLI** (`crates/<tool>/tests/cli.rs`) — one binary: stdin → stdout →
  exit code.
- **integration** (`crates/integration-tests/`) — the tools *composed*: real
  pipelines like `ts-window | ts-burst`, kept to a few representative
  flows plus a golden fixture (`fixtures/alb-502-incident/`) that must still
  rediscover a known peak. Run via a whole-workspace `cargo test` so every binary
  is built.

## ts-burst

```
# ALB 502s per second
jq -r 'select(.status == 502) | .timestamp' alb.jsonl | ts-burst

# SIGTERM occurrences
grep SIGTERM app.log | awk '{print $1}' | ts-burst
```

Input: one RFC3339 timestamp per line, offset required (e.g.
`2026-09-03T10:21:31.124+09:00` or `…Z`). All lines must share the same offset —
mixed offsets are rejected.
Output: a per-second histogram, then total / peak rate / first / last / span.
Times are shown in the input's offset so they line up with the source log and
CloudWatch without conversion.

```
10:21:29     2  ████████████
10:21:30     3  ██████████████████
10:21:31     5  ██████████████████████████████
10:21:32     2  ████████████
10:21:33     1  ██████

events         13
peak            5  /s at 10:21:31
first    10:21:29
last     10:21:33
span     4s (first→last)
```

## ts-window

```
# narrow a raw log to the incident window, then look at the shape
cat alb.jsonl \
  | ts-window --from 2026-09-03T10:20:00+09:00 --to 2026-09-03T10:30:00+09:00 \
  | jq -r 'select(.elb_status_code == 502) | .time' \
  | ts-burst
```

Reads the RFC3339 timestamp from each line (leading token, quotes stripped) and
prints the original line unchanged when it falls in `[--from, --to]` (inclusive;
at least one bound required). Comparison is on the instant, so the bounds and the
lines may use any offset. A `kept N / total` summary goes to stderr.

## ts-window-audit

```
# before trusting the numbers: where does the fetched data sit in the window?
jq -r .time alb.jsonl \
  | ts-window-audit --from 2026-09-03T10:20:00+09:00 --to 2026-09-03T10:30:00+09:00 --limit 10000
```

Reads RFC3339 timestamps from stdin and reports the observed first/last against
the requested `[--from, --to]`, the gap at each edge, and verdict-free signals:
`EMPTY`, `LIMIT_REACHED`, `OUT_OF_WINDOW` (an observation fell outside the stated
window — a gap is negative). Facts, not causes: a gap means *nothing was observed
there*, not that events were lost; `LIMIT_REACHED` only means the input rows
reached the cap — it does not confirm truncation, and which edge a cap cut depends
on the query's sort order, which this tool doesn't know.

`--limit` must be positive and is compared against the **input row count** (parsed
*and* unparseable lines, since the fetch cap counts both). That may still differ
from the original fetch count if blank lines, a header, or an upstream filter sit
between the fetch and this tool; rows dropped before this input are invisible to
it. Durations print in seconds or larger units, with sub-second magnitudes in
milliseconds and non-zero sub-millisecond magnitudes as `<1ms` / `>-1ms`, so a
non-zero gap never shows as `0s`. Timestamps are printed in RFC3339 with their
input offset, keeping sub-second precision, so the displayed range
stays consistent with the gaps. Unparseable lines are reported as `unparsed N`.

```
window    2026-09-03T10:20:00+09:00 → 2026-09-03T10:30:00+09:00
observed  2026-09-03T10:20:04+09:00 → 2026-09-03T10:30:00+09:00
rows      10000 / 10000

start gap 4s
end gap   0s

signals:
  LIMIT_REACHED
```

## ts-dist

```
# response-time distribution over a time window
cat alb.jsonl \
  | ts-window --from 2026-09-03T10:20:00+09:00 --to 2026-09-03T10:30:00+09:00 \
  | jq -r .target_processing_time \
  | ts-dist
```

Reads one number per line (leading token, quotes stripped; NaN/inf rejected) and
prints count / min / mean / p50 / p90 / p95 / p99 / max / stddev. Unit-agnostic —
feed ms, seconds, or counts and read the result in the same unit. Percentiles are
nearest-rank (no interpolation), so each is an actually-observed value.

## ts-correlate

```
# did the 502s and the SIGTERMs cluster at the same time?
ts-correlate 502.txt sigterm.txt --window 5s
```

Reads one RFC3339 timestamp per line from two files and reports, in both
directions, what fraction of one stream's events have an event in the other
within the specified ±window (`5s` / `2m` / `1h` / bare seconds; default 1s).
Both directions matter — "71% of 502s were near a SIGTERM" and "83% of SIGTERMs
were near a 502"
are different facts. It counts matched events, not pairs.

## ts-series

```
# shape of a CloudWatch metric over a time window, with the peak minute
aws cloudwatch get-metric-statistics ... \
  --query 'Datapoints[].[Timestamp,Sum]' --output text \
  | ts-series
```

Reads `RFC3339 <whitespace> number` lines (order doesn't matter — sorted
internally), one point per line. Prints a row per point (time, sparkline level,
value), then points / sum / min / max with the time of each extreme. The
sparkline is a linear map onto `▁▂▃▄▅▆▇█`. Unlike `ts-burst` (which counts
raw event occurrences), `ts-series` consumes values that are already
aggregated per timestamp — e.g. CloudWatch datapoints.

```
10:20:00  ▁          12
10:21:00  ▄          47
10:22:00  █         103

points            3
sum             162
min              12  at 10:20:00
max             103  at 10:22:00
```

## ts-align

Inner-join N `timestamp value` files on their timestamp. For every timestamp
present in *all* inputs it prints `<RFC3339>` then one tab-separated value per
file, sorted by time. Only the join lives in `ts-align` — the fragile part
to do in shell (nested `join`); the derived metric and its peak are a downstream
step, which keeps the output re-parseable by `ts-series`:

```
# per-task concurrency ≈ (rate × latency) / tasks, and when it peaked
ts-align req.txt lat.txt tasks.txt \
  | awk -F'\t' '{printf "%s\t%.2f\n", $1, $2/60*$3/$4}' \
  | ts-series
```

where req / lat / tasks are CloudWatch RequestCount (Sum), TargetResponseTime
(Average) and LiveTaskCount (Minimum). A `common N / per-file totals` line goes to
stderr so dropped (non-common) timestamps are visible.

## ts-timeline

![ts-timeline: four timestamp/value series stacked on one shared time axis](docs/timeline.png)

```
# a 502 burst next to CPU / latency / task count on one time axis
ts-timeline \
  --series 502.txt --series cpu.txt --series latency.txt --series tasks.txt \
  > report.html
```

Reads one or more `RFC3339 <whitespace> number` files via `--series` (same format
as `ts-series`; a per-second log count from `stats count() by bin(1s)` drops
in exactly like a metric). Writes a self-contained HTML page — inline SVG, no
JavaScript, no external assets — to stdout, ready to open or share.

Each series becomes one line chart with a marker per point, on its own y-scale,
stacked on a shared time axis, so you can see what moved together and when. The
line only connects the points you supply — it doesn't imply a value in the gaps,
so zero-fill upstream if you need that. Markers carry the full timestamp and value
as a hover tooltip; each chart labels its own min / max.

Axis and tooltip times default to the input's offset; `--display-offset +09:00`
shows them at a fixed offset instead (e.g. JST), leaving values and positions
unchanged.

Unlike the summary tools, this one is for *shape and simultaneity* across signals —
the part a per-metric summary drops. Like `ts-correlate` and `ts-align`
it reads files (not stdin), and it emits HTML rather than text.

## License

[MIT](LICENSE)
