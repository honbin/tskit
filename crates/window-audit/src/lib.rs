//! Core logic for `window-audit`: report where observed events sit inside a
//! requested time window, plus one fetch-condition signal.
//!
//! It reports *facts*, never a verdict or a cause. A gap only says "nothing was
//! observed between this window edge and the nearest event" — not whether events
//! existed there and were lost. `LIMIT_REACHED` only says "the input rows reached
//! the stated cap" — not that the fetch was truncated (exactly `limit` rows may
//! simply exist), and rows dropped upstream before this input are invisible to it.
//! Kept free of I/O so it can be unit-tested without spawning the binary.

use chrono::{DateTime, Duration, FixedOffset};

/// The requested query window. Both bounds are known — the audit is about a
/// specific window, and both gaps are reported.
pub struct Window {
    pub from: DateTime<FixedOffset>,
    pub to: DateTime<FixedOffset>,
}

/// The span of observed (parsed) events. Absent when nothing parsed.
pub struct Observed {
    pub first: DateTime<FixedOffset>,
    pub last: DateTime<FixedOffset>,
}

/// Verdict-free signals. Each states a fact, not a conclusion about causes.
#[derive(Debug, PartialEq, Eq)]
pub enum Signal {
    /// No event parsed — nothing to place in the window.
    Empty,
    /// The input row count reached or exceeded the stated cap (`rows >= limit`).
    /// Does not confirm truncation; exactly `limit` rows may simply exist.
    LimitReached,
    /// An observation fell outside the stated window (a gap is negative).
    OutOfWindow,
}

/// Gaps (as signed `Duration`) and signals for the observations vs the window.
pub struct Audit {
    /// `first − from` (None when nothing observed). Negative ⇒ out of window.
    pub start_gap: Option<Duration>,
    /// `to − last` (None when nothing observed). Negative ⇒ out of window.
    pub end_gap: Option<Duration>,
    pub signals: Vec<Signal>,
}

/// Report the observed range against the window.
///
/// `rows` is the number of *input rows* the `limit` caps — unparseable rows are
/// included, since the fetch cap counts them too; it is not the parsed-event
/// count. `limit` is the query's row cap; pass `None` when there is no cap to
/// assess (then `LIMIT_REACHED` is never raised).
pub fn audit(window: &Window, observed: Option<&Observed>, rows: u64, limit: Option<u64>) -> Audit {
    let mut signals = Vec::new();
    if observed.is_none() {
        signals.push(Signal::Empty);
    }
    if let Some(l) = limit
        && l > 0
        && rows >= l
    {
        signals.push(Signal::LimitReached);
    }
    let (start_gap, end_gap) = match observed {
        Some(o) => {
            if o.first < window.from || o.last > window.to {
                signals.push(Signal::OutOfWindow);
            }
            (Some(o.first - window.from), Some(window.to - o.last))
        }
        None => (None, None),
    };
    Audit {
        start_gap,
        end_gap,
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<FixedOffset> {
        tstamp::parse_line(s).unwrap()
    }

    fn win() -> Window {
        Window {
            from: ts("2026-10-10T10:00:00+09:00"),
            to: ts("2026-10-10T11:00:00+09:00"),
        }
    }

    fn obs(first: &str, last: &str) -> Observed {
        Observed {
            first: ts(first),
            last: ts(last),
        }
    }

    #[test]
    fn gaps_and_no_signals_when_inside_and_under_limit() {
        let o = obs("2026-10-10T10:08:00+09:00", "2026-10-10T10:47:00+09:00");
        let a = audit(&win(), Some(&o), 500, Some(10000));
        assert_eq!(a.start_gap, Some(Duration::minutes(8)));
        assert_eq!(a.end_gap, Some(Duration::minutes(13)));
        assert!(a.signals.is_empty());
    }

    #[test]
    fn observations_on_window_edges_have_zero_gaps() {
        // from/to are inclusive (strict < / >), so an observation exactly on each edge
        // gives a zero gap and no OUT_OF_WINDOW.
        let o = obs("2026-10-10T10:00:00+09:00", "2026-10-10T11:00:00+09:00");
        let a = audit(&win(), Some(&o), 2, None);
        assert_eq!(a.start_gap, Some(Duration::zero()));
        assert_eq!(a.end_gap, Some(Duration::zero()));
        assert!(a.signals.is_empty());
    }

    #[test]
    fn limit_reached_when_rows_hit_cap() {
        let o = obs("2026-10-10T10:08:00+09:00", "2026-10-10T10:47:00+09:00");
        // Only 2 parsed events, but 10000 input rows reached the cap.
        let a = audit(&win(), Some(&o), 10000, Some(10000));
        assert_eq!(a.signals, vec![Signal::LimitReached]);
    }

    #[test]
    fn no_limit_signal_without_a_cap() {
        let o = obs("2026-10-10T10:08:00+09:00", "2026-10-10T10:47:00+09:00");
        let a = audit(&win(), Some(&o), 10000, None);
        assert!(a.signals.is_empty());
        let zero = audit(&win(), Some(&o), 10000, Some(0));
        assert!(zero.signals.is_empty());
    }

    #[test]
    fn empty_when_nothing_observed() {
        let a = audit(&win(), None, 0, Some(10000));
        assert_eq!(a.signals, vec![Signal::Empty]);
        assert!(a.start_gap.is_none() && a.end_gap.is_none());
    }

    #[test]
    fn empty_rows_at_cap_also_flags_limit() {
        // All fetched rows were unparseable, and the fetch reached the cap.
        let a = audit(&win(), None, 10000, Some(10000));
        assert_eq!(a.signals, vec![Signal::Empty, Signal::LimitReached]);
    }

    #[test]
    fn out_of_window_before_start_gives_negative_start_gap() {
        let o = obs("2026-10-10T09:58:00+09:00", "2026-10-10T10:47:00+09:00");
        let a = audit(&win(), Some(&o), 3, None);
        assert_eq!(a.start_gap, Some(Duration::minutes(-2)));
        assert_eq!(a.signals, vec![Signal::OutOfWindow]);
    }

    #[test]
    fn out_of_window_after_end_gives_negative_end_gap() {
        let o = obs("2026-10-10T10:08:00+09:00", "2026-10-10T11:05:00+09:00");
        let a = audit(&win(), Some(&o), 3, None);
        assert_eq!(a.end_gap, Some(Duration::minutes(-5)));
        assert_eq!(a.signals, vec![Signal::OutOfWindow]);
    }

    #[test]
    fn limit_and_out_of_window_can_both_fire() {
        let o = obs("2026-10-10T09:58:00+09:00", "2026-10-10T10:47:00+09:00");
        let a = audit(&win(), Some(&o), 10000, Some(10000));
        assert_eq!(a.signals, vec![Signal::LimitReached, Signal::OutOfWindow]);
    }

    #[test]
    fn same_instant_other_offset_is_inside() {
        // 01:05Z == 10:05+09:00, inside [10:00, 11:00]+09:00 — not out of window.
        let o = obs("2026-10-10T01:05:00Z", "2026-10-10T01:50:00Z");
        let a = audit(&win(), Some(&o), 3, None);
        assert!(a.signals.is_empty());
    }
}
