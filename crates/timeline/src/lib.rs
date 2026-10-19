//! Core rendering for `timeline`: lay out `(timestamp, value)` series on a
//! shared time axis as inline SVG inside a self-contained HTML page.
//!
//! Every input is the same shape — a series of points — so a per-second log
//! count (`stats count() by bin(1s)`) and a metric are drawn identically.
//!
//! Kept free of I/O so it can be unit-tested without the binary: the binary
//! parses, the library renders.

use std::fmt::Write as _;

use chrono::{DateTime, FixedOffset};

const WIDTH: f64 = 900.0;
const CHART_H: f64 = 120.0;
const PAD_L: f64 = 8.0;
const PAD_R: f64 = 8.0;
const PAD_T: f64 = 10.0;
const PAD_B: f64 = 22.0; // room under the baseline for the time labels

/// A named `(epoch nanosecond, value)` series, drawn as one line chart.
/// Nanoseconds (the parser's full resolution) so points within the same second
/// — even the same millisecond — keep their order and position.
pub struct Series {
    pub name: String,
    pub points: Vec<(i64, f64)>,
}

/// The time axis every chart shares, so they line up vertically. `offset` is
/// only the display zone for the axis labels; positions are computed in epoch
/// nanoseconds.
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
    pub offset: FixedOffset,
}

impl TimeRange {
    /// Map an epoch nanosecond to an x pixel inside the drawing area. A
    /// zero-width range (one instant) pins everything to the left edge.
    pub fn x(&self, t: i64) -> f64 {
        // widen to i128: two epoch-nanosecond timestamps can be ~585 years
        // apart, which overflows an i64 subtraction (panics in debug).
        let span = (self.end as i128 - self.start as i128) as f64;
        let frac = if span <= 0.0 {
            0.0
        } else {
            (t as i128 - self.start as i128) as f64 / span
        };
        PAD_L + frac * (WIDTH - PAD_L - PAD_R)
    }
}

/// Map a value to a y pixel, inverted so larger values sit higher on screen. A
/// flat series (`max == min`) sits in the middle.
fn y(value: f64, min: f64, max: f64) -> f64 {
    let span = max - min;
    let frac = if span <= 0.0 {
        0.5
    } else {
        (value - min) / span
    };
    (CHART_H - PAD_B) - frac * (CHART_H - PAD_T - PAD_B)
}

/// Render the whole page: one chart per series, all sharing the time axis
/// spanned by every point across every series.
pub fn render_html(series: &[Series], offset: FixedOffset) -> String {
    let range = time_range(series, offset);
    let mut body = String::new();
    for s in series {
        let vr = value_range(&s.points);
        let svg = render_series_svg(s, &range, vr);
        section(&mut body, &s.name, vr, &svg);
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>ts-timeline</title>\n<style>{CSS}</style>\n</head>\n\
         <body>\n{body}</body>\n</html>\n"
    )
}

/// Draw one series as a polyline plus a marker per observed point, scaled to
/// the series' own min/max. Markers make lone points visible, show where the
/// real data is versus interpolation, and carry each point's time and value as
/// a native SVG hover tooltip (`<title>`, no JavaScript).
pub fn render_series_svg(series: &Series, range: &TimeRange, vr: Option<(f64, f64)>) -> String {
    let mut svg = svg_open();
    if let Some((min, max)) = vr {
        let mut pts = String::new();
        for &(t, v) in &series.points {
            let _ = write!(pts, "{:.1},{:.1} ", range.x(t), y(v, min, max));
        }
        let _ = write!(
            svg,
            "<polyline fill=\"none\" stroke=\"#36c\" stroke-width=\"1.5\" points=\"{}\"/>",
            pts.trim_end()
        );
        for &(t, v) in &series.points {
            let _ = write!(
                svg,
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"2\" fill=\"#36c\"><title>{} {}</title></circle>",
                range.x(t),
                y(v, min, max),
                fmt_time_full(t, range.offset),
                v
            );
        }
    }
    axis(&mut svg, range);
    svg.push_str("</svg>");
    svg
}

/// A `viewBox` (not a fixed pixel width) so the chart scales down on narrow
/// screens without clipping the right edge.
fn svg_open() -> String {
    format!("<svg viewBox=\"0 0 {WIDTH} {CHART_H}\" xmlns=\"http://www.w3.org/2000/svg\">")
}

fn axis(svg: &mut String, range: &TimeRange) {
    let base = CHART_H - PAD_B;
    let _ = write!(
        svg,
        "<line x1=\"{:.1}\" y1=\"{base:.1}\" x2=\"{:.1}\" y2=\"{base:.1}\" stroke=\"#ccc\"/>",
        PAD_L,
        WIDTH - PAD_R
    );
    let label_y = CHART_H - 6.0;
    let _ = write!(
        svg,
        "<text x=\"{:.1}\" y=\"{label_y:.1}\" font-size=\"10\" fill=\"#666\">{}</text>",
        PAD_L,
        fmt_time(range.start, range.offset)
    );
    let _ = write!(
        svg,
        "<text x=\"{:.1}\" y=\"{label_y:.1}\" font-size=\"10\" fill=\"#666\" \
         text-anchor=\"end\">{}</text>",
        WIDTH - PAD_R,
        fmt_time(range.end, range.offset)
    );
}

/// Heading (the filename) plus the series' min/max, so an independent y-scale
/// can't be misread — a tiny wiggle and a big spike are drawn the same height.
fn section(body: &mut String, name: &str, vr: Option<(f64, f64)>, svg: &str) {
    let meta = match vr {
        Some((min, max)) => {
            let (lo, hi) = fmt_minmax(min, max);
            format!("min {lo} · max {hi}")
        }
        None => "no points".to_string(),
    };
    let _ = write!(
        body,
        "<section>\n<h2>{}</h2>\n<div class=\"meta\">{}</div>\n{}\n</section>\n",
        esc(name),
        esc(&meta),
        svg
    );
}

/// Span the axis over every point across all series. Falls back to a zero-width
/// range when there is nothing (the binary guards against that).
fn time_range(series: &[Series], offset: FixedOffset) -> TimeRange {
    let mut min = i64::MAX;
    let mut max = i64::MIN;
    for s in series {
        for &(t, _) in &s.points {
            min = min.min(t);
            max = max.max(t);
        }
    }
    if min > max {
        min = 0;
        max = 0;
    }
    TimeRange {
        start: min,
        end: max,
        offset,
    }
}

fn value_range(points: &[(i64, f64)]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let min = points.iter().map(|&(_, v)| v).fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|&(_, v)| v)
        .fold(f64::NEG_INFINITY, f64::max);
    Some((min, max))
}

fn fmt_time(ns: i64, offset: FixedOffset) -> String {
    DateTime::from_timestamp_nanos(ns)
        .with_timezone(&offset)
        .format("%H:%M:%S")
        .to_string()
}

/// Full RFC3339 instant (date, sub-second, offset) for the point tooltips —
/// a shared report may be read on another day or in another zone, where the
/// short axis label alone can't identify a point.
fn fmt_time_full(ns: i64, offset: FixedOffset) -> String {
    DateTime::from_timestamp_nanos(ns)
        .with_timezone(&offset)
        .to_rfc3339()
}

/// Format min/max to three decimals, preserving tiny values and distinct bounds.
/// Only affects labels; coordinates and tooltips retain the original values.
fn fmt_minmax(min: f64, max: f64) -> (String, String) {
    let (lo, hi) = (fmt_val(min), fmt_val(max));
    // compare the rounded labels as numbers, not as strings: "1" and "1.000"
    // read as the same value, so a distinct min/max must still fall back to full.
    let reads_same = lo.parse::<f64>().ok() == hi.parse::<f64>().ok();
    let values_differ = format!("{min}") != format!("{max}");
    if reads_same && values_differ {
        return (format!("{min}"), format!("{max}"));
    }
    (lo, hi)
}

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

/// Parse a fixed UTC offset written as `±HH:MM` (e.g. `+09:00`). Region names
/// and DST are out of scope — a fixed offset needs no timezone database.
pub fn parse_offset(s: &str) -> Result<FixedOffset, String> {
    fn bad(s: &str) -> String {
        format!("--display-offset must be ±HH:MM, e.g. +09:00 (got {s:?})")
    }
    // exact ±HH:MM shape — reject +9:00, +009:00, +09:0 etc.
    if s.len() != 6 || s.as_bytes().get(3) != Some(&b':') {
        return Err(bad(s));
    }
    let sign = match s.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(bad(s)),
    };
    let (h, m) = s[1..].split_once(':').ok_or_else(|| bad(s))?;
    // HH and MM must be ASCII digits — parse() alone would accept "+9" or "-0"
    if !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return Err(bad(s));
    }
    let hours: i32 = h.parse().map_err(|_| bad(s))?;
    let mins: i32 = m.parse().map_err(|_| bad(s))?;
    if !(0..60).contains(&mins) {
        return Err(bad(s));
    }
    FixedOffset::east_opt(sign * (hours * 3600 + mins * 60))
        .ok_or_else(|| format!("--display-offset {s:?} is out of range"))
}

/// Escape the few characters that would break HTML/SVG text (names are
/// filenames, which can contain `&` or `<`).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const CSS: &str = "body{font-family:ui-monospace,Menlo,monospace;margin:2rem;color:#222}\
h2{font-size:14px;margin:1.5rem 0 .1rem;font-weight:600}\
.meta{font-size:12px;color:#888;margin-bottom:.25rem}\
svg{display:block;width:100%;height:auto;max-width:900px;border:1px solid #eee}";

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange {
            start,
            end,
            offset: FixedOffset::east_opt(0).unwrap(),
        }
    }

    #[test]
    fn x_maps_endpoints_to_the_drawing_area() {
        let r = range(100, 200);
        assert!((r.x(100) - PAD_L).abs() < 1e-9);
        assert!((r.x(200) - (WIDTH - PAD_R)).abs() < 1e-9);
    }

    #[test]
    fn x_zero_span_pins_to_left() {
        let r = range(100, 100);
        assert!((r.x(100) - PAD_L).abs() < 1e-9);
    }

    #[test]
    fn x_sub_second_points_separate() {
        // two distinct instants a fraction of a second apart must not collapse
        let r = range(1000, 1800);
        assert!((r.x(1000) - PAD_L).abs() < 1e-9);
        assert!((r.x(1800) - (WIDTH - PAD_R)).abs() < 1e-9);
    }

    #[test]
    fn x_extreme_range_does_not_overflow() {
        // epoch-nanosecond timestamps can be ~585 years apart — wider than an
        // i64 subtraction, so the span must be computed in i128.
        let r = range(i64::MIN, i64::MAX);
        let x = r.x(0);
        assert!((PAD_L..=WIDTH - PAD_R).contains(&x));
    }

    #[test]
    fn y_inverts_the_value_axis() {
        assert!((y(10.0, 0.0, 10.0) - PAD_T).abs() < 1e-9); // max -> top
        assert!((y(0.0, 0.0, 10.0) - (CHART_H - PAD_B)).abs() < 1e-9); // min -> baseline
    }

    #[test]
    fn y_flat_series_sits_mid() {
        let mid = (CHART_H - PAD_B) - 0.5 * (CHART_H - PAD_T - PAD_B);
        assert!((y(5.0, 5.0, 5.0) - mid).abs() < 1e-9);
    }

    #[test]
    fn min_max_label_rounds_but_never_hides() {
        assert_eq!(fmt_val(3.049744988363931), "3.050");
        assert_eq!(fmt_val(13158.0), "13158");
        assert_eq!(fmt_val(0.0004), "0.0004"); // tiny: not shown as 0.000
        assert_eq!(fmt_val(-0.0), "0");
        // distinct values that both round to 1.000 fall back to full
        assert_eq!(
            fmt_minmax(1.0001, 1.0002),
            ("1.0001".to_string(), "1.0002".to_string())
        );
        // "1" and "1.000" read as the same number → still fall back to full
        assert_eq!(
            fmt_minmax(1.0, 1.0001),
            ("1".to_string(), "1.0001".to_string())
        );
        // a genuinely flat series shows them equal
        assert_eq!(fmt_minmax(5.0, 5.0), ("5".to_string(), "5".to_string()));
    }

    #[test]
    fn parse_offset_accepts_signed_hh_mm() {
        assert_eq!(
            parse_offset("+09:00"),
            Ok(FixedOffset::east_opt(9 * 3600).unwrap())
        );
        assert_eq!(
            parse_offset("-05:30"),
            Ok(FixedOffset::east_opt(-(5 * 3600 + 30 * 60)).unwrap())
        );
        assert_eq!(
            parse_offset("+00:00"),
            Ok(FixedOffset::east_opt(0).unwrap())
        );
    }

    #[test]
    fn parse_offset_rejects_bad_input() {
        assert!(parse_offset("09:00").is_err()); // no sign
        assert!(parse_offset("+0900").is_err()); // no colon
        assert!(parse_offset("+9:00").is_err()); // hours not two digits
        assert!(parse_offset("+09:0").is_err()); // minutes not two digits
        assert!(parse_offset("++9:00").is_err()); // sign where a digit belongs
        assert!(parse_offset("Asia/Tokyo").is_err()); // region name
        assert!(parse_offset("+09:99").is_err()); // minutes out of range
        assert!(parse_offset("+40:00").is_err()); // offset out of range
        assert!(parse_offset("").is_err());
    }
}
