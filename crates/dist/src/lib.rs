//! Core logic for `dist`: parse numbers and summarize their distribution.
//!
//! Unit-agnostic — it never assumes ms vs s, it just reports the numbers it is
//! given. Kept free of I/O so it can be unit-tested without spawning the binary.

/// Parse a finite number from the first whitespace-delimited token.
pub fn parse_number(line: &str) -> Option<f64> {
    let token = line.split_whitespace().next()?.trim_matches('"');
    token.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Headline distribution numbers.
#[derive(Debug, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Nearest-rank percentile on an ascending-sorted slice. `p` is in `[0, 100]`.
///
/// Rank = ceil(p/100 · n), clamped to `[1, n]`, read 1-indexed. No interpolation
/// — the result is always an actual observed value, which is easy to reason about
/// and matches "the value at or below which p% of samples fall".
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    debug_assert!(!sorted.is_empty(), "percentile of empty slice");
    debug_assert!(
        sorted.is_sorted(),
        "percentile requires an ascending-sorted slice"
    );
    let n = sorted.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    sorted[rank.clamp(1, n) - 1]
}

/// Sort `values` in place and summarize. Returns `None` for an empty slice.
/// stddev is population (divided by n), so a single sample reports 0.
pub fn summarize(values: &mut [f64]) -> Option<Stats> {
    if values.is_empty() {
        return None;
    }
    // parse_number filtered out non-finite values, so partial_cmp is total here.
    values.sort_by(|a, b| a.partial_cmp(b).expect("values are finite"));

    let count = values.len();
    let sum: f64 = values.iter().sum();
    let mean = sum / count as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;

    Some(Stats {
        count,
        min: values[0],
        max: values[count - 1],
        mean,
        stddev: variance.sqrt(),
        p50: percentile(values, 50.0),
        p90: percentile(values, 90.0),
        p95: percentile(values, 95.0),
        p99: percentile(values, 99.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_forgivingly() {
        assert_eq!(parse_number("0.012"), Some(0.012));
        assert_eq!(parse_number("\"1.5\""), Some(1.5));
        assert_eq!(parse_number("123 ms"), Some(123.0));
        assert_eq!(parse_number("\"1.5\" ms"), Some(1.5));
        assert_eq!(parse_number("1e3"), Some(1000.0));
    }

    #[test]
    fn rejects_blank_garbage_and_non_finite() {
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("   "), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("NaN"), None);
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 50.0), 5.0);
        assert_eq!(percentile(&v, 90.0), 9.0);
        assert_eq!(percentile(&v, 99.0), 10.0);
        assert_eq!(percentile(&v, 100.0), 10.0);
    }

    #[test]
    fn summarizes_distribution() {
        let mut v: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let s = summarize(&mut v).unwrap();
        assert_eq!(s.count, 10);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 10.0);
        assert_eq!(s.mean, 5.5);
        assert_eq!(s.stddev, 8.25_f64.sqrt());
        assert_eq!(s.p50, 5.0);
        assert_eq!(s.p90, 9.0);
        assert_eq!(s.p95, 10.0);
        assert_eq!(s.p99, 10.0);
    }

    #[test]
    fn summarize_empty_is_none() {
        assert!(summarize(&mut []).is_none());
    }
}
