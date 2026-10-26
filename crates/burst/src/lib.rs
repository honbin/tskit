//! Core logic for `burst`: bucket timestamps by second and summarize.
//!
//! Timestamp parsing lives in the shared `tstamp` crate. Kept free of I/O so it
//! can be unit-tested without spawning the binary.

use std::collections::BTreeMap;

/// Count events per one-second bucket, keyed by epoch second.
///
/// `BTreeMap` keeps the buckets in chronological order for free.
pub fn counts_by_second(epoch_secs: &[i64]) -> BTreeMap<i64, u64> {
    let mut counts = BTreeMap::new();
    for &sec in epoch_secs {
        *counts.entry(sec).or_insert(0) += 1;
    }
    counts
}

/// Headline numbers over the bucketed counts.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub total: u64,
    pub peak_count: u64,
    /// The earliest second that reached `peak_count`: when the burst first peaked.
    pub peak_sec: i64,
    pub first_sec: i64,
    pub last_sec: i64,
}

/// Summarize the non-empty buckets. Returns `None` for an empty map.
pub fn summarize(counts: &BTreeMap<i64, u64>) -> Option<Summary> {
    let mut iter = counts.iter();
    let (&first_sec, &first_count) = iter.next()?;
    let mut summary = Summary {
        total: first_count,
        peak_count: first_count,
        peak_sec: first_sec,
        first_sec,
        last_sec: first_sec,
    };
    for (&sec, &count) in iter {
        summary.total += count;
        summary.last_sec = sec;
        if count > summary.peak_count {
            summary.peak_count = count;
            summary.peak_sec = sec;
        }
    }
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_and_summarizes() {
        let base = tstamp::parse_line("2026-09-03T10:21:31+09:00")
            .unwrap()
            .timestamp();
        let secs = vec![base, base, base + 2];
        let counts = counts_by_second(&secs);
        assert_eq!(counts.len(), 2);

        let summary = summarize(&counts).unwrap();
        assert_eq!(
            summary,
            Summary {
                total: 3,
                peak_count: 2,
                peak_sec: base,
                first_sec: base,
                last_sec: base + 2,
            }
        );
    }

    #[test]
    fn a_tied_peak_stays_at_its_earliest_second() {
        let summary = summarize(&counts_by_second(&[10, 11, 11, 12, 12])).unwrap();
        assert_eq!((summary.peak_count, summary.peak_sec), (2, 11));
    }

    #[test]
    fn summarize_empty_is_none() {
        assert!(summarize(&counts_by_second(&[])).is_none());
    }
}
