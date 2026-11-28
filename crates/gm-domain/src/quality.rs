//! Data-quality checks on a daily bar series. Pure: every input, including "today", is explicit.
//!
//! `passed` means the series is usable for analysis (no impossible prices, not stale).
//! Missing days and return outliers are reported as warnings, because both happen in real
//! data (suspensions, genuine crashes) and the user should see them rather than have them hidden.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{indicators, types::PriceBar};

/// |robust z| above this flags a return. Under a normal law P(|Z| > 8) ≈ 1e-15, and even a
/// Student-t with 3 d.o.f. (fatter-tailed than most equities) makes it ≈ 0.4%, so hits are
/// either genuine extreme events or bad prints — worth a human look either way.
const OUTLIER_ROBUST_Z: f64 = 8.0;
/// Consistent-estimator scale for MAD under normality: σ ≈ 1.4826 · MAD.
const MAD_TO_SIGMA: f64 = 1.4826;
/// Without a reference calendar: a weekend plus a two-day exchange holiday is ≤ 4 calendar days.
const STALE_CALENDAR_DAYS: i64 = 5;
/// Relative tolerance for OHLC consistency; vendors round prices to the tick.
const OHLC_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnOutlier {
    pub date: NaiveDate,
    pub log_return: f64,
    pub robust_z: f64,
    /// A split or dividend within 3 calendar days explains most "outliers" in unadjusted data.
    pub near_corporate_action: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityReport {
    pub passed: bool,
    pub n_bars: usize,
    pub first_date: Option<NaiveDate>,
    pub last_date: Option<NaiveDate>,
    pub ohlc_violations: Vec<NaiveDate>,
    pub non_positive_prices: Vec<NaiveDate>,
    pub zero_volume_days: usize,
    /// Reference-calendar trading days inside the series span that have no bar.
    pub missing_days: Vec<NaiveDate>,
    pub is_stale: bool,
    pub outliers: Vec<ReturnOutlier>,
    /// Bad prints removed from the series before anything was computed (see `bad_prints`).
    #[serde(default)]
    pub bad_prints_removed: Vec<NaiveDate>,
    pub warnings: Vec<String>,
}

/// `bars` sorted oldest first. `reference_days` is an exchange calendar proxy (e.g. NIFTY's bars).
pub fn assess(
    bars: &[PriceBar],
    reference_days: Option<&[NaiveDate]>,
    corporate_action_days: &[NaiveDate],
    today: NaiveDate,
) -> QualityReport {
    let first_date = bars.first().map(|b| b.date);
    let last_date = bars.last().map(|b| b.date);

    let mut ohlc_violations = Vec::new();
    let mut non_positive_prices = Vec::new();
    for bar in bars {
        if [bar.open, bar.high, bar.low, bar.close]
            .iter()
            .any(|p| *p <= 0.0 || !p.is_finite())
        {
            non_positive_prices.push(bar.date);
            continue;
        }
        let tol = OHLC_TOLERANCE * bar.high;
        let consistent = bar.low <= bar.open.min(bar.close) + tol
            && bar.high + tol >= bar.open.max(bar.close)
            && bar.low <= bar.high + tol;
        if !consistent {
            ohlc_violations.push(bar.date);
        }
    }
    let zero_volume_days = bars.iter().filter(|b| b.volume == 0).count();

    let (missing_days, is_stale) = match (reference_days, first_date, last_date) {
        (Some(reference), Some(first), Some(last)) => {
            let have = bars
                .iter()
                .map(|b| b.date)
                .collect::<std::collections::BTreeSet<_>>();
            let missing = reference
                .iter()
                .filter(|d| **d >= first && **d <= last && !have.contains(d))
                .copied()
                .collect();
            // Stale when the market traded after our last bar.
            (missing, reference.iter().any(|d| *d > last))
        }
        (None, _, Some(last)) => (Vec::new(), (today - last).num_days() > STALE_CALENDAR_DAYS),
        _ => (Vec::new(), true),
    };

    let outliers = return_outliers(bars, corporate_action_days);

    let mut warnings = Vec::new();
    if bars.len() < 2 {
        warnings.push("fewer than 2 bars: no returns can be computed".to_string());
    }
    if !missing_days.is_empty() {
        warnings.push(format!(
            "{} trading days missing vs reference calendar",
            missing_days.len()
        ));
    }
    if zero_volume_days > 0 {
        warnings.push(format!(
            "{zero_volume_days} zero-volume days (normal for indices)"
        ));
    }
    if !outliers.is_empty() {
        warnings.push(format!(
            "{} extreme daily returns (|robust z| > {OUTLIER_ROBUST_Z})",
            outliers.len()
        ));
    }

    QualityReport {
        passed: bars.len() >= 2
            && ohlc_violations.is_empty()
            && non_positive_prices.is_empty()
            && !is_stale,
        n_bars: bars.len(),
        first_date,
        last_date,
        ohlc_violations,
        non_positive_prices,
        zero_volume_days,
        missing_days,
        is_stale,
        outliers,
        bad_prints_removed: Vec::new(),
        warnings,
    }
}

fn return_outliers(bars: &[PriceBar], corporate_action_days: &[NaiveDate]) -> Vec<ReturnOutlier> {
    let valid = bars
        .iter()
        .filter(|b| b.adjusted_close() > 0.0)
        .collect::<Vec<_>>();
    let prices = valid.iter().map(|b| b.adjusted_close()).collect::<Vec<_>>();
    let returns = indicators::log_returns(&prices);
    let (Some(median), Some(mad)) = (median(&returns), median_abs_deviation(&returns)) else {
        return Vec::new();
    };
    let scale = MAD_TO_SIGMA * mad;
    // Degenerate dispersion (flat or periodic series): MAD collapses to float noise and every
    // return would look extreme. No meaningful robust z exists, so report none.
    if scale < 1e-12 {
        return Vec::new();
    }
    returns
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let robust_z = (r - median) / scale;
            (robust_z.abs() > OUTLIER_ROBUST_Z).then(|| {
                let date = valid[i + 1].date;
                ReturnOutlier {
                    date,
                    log_return: *r,
                    robust_z,
                    near_corporate_action: corporate_action_days
                        .iter()
                        .any(|d| (*d - date).num_days().abs() <= 3),
                }
            })
        })
        .collect()
}

/// A bad print must move the price at least this much (log): a halving or doubling in one
/// session, beyond the daily price band of any exchange we read (NSE: 20% at most).
const BAD_PRINT_JUMP: f64 = std::f64::consts::LN_2;
/// The longest run of bad prints looked for (vendors sometimes repeat a wrong price).
const BAD_PRINT_MAX_RUN: usize = 5;

/// Bad prints: bars where the price jumps away by at least a halving or doubling and comes back
/// within `BAD_PRINT_MAX_RUN` sessions, the round trip netting to under a quarter of the jump.
/// No market does that; vendors do (Yahoo: GOLDBEES on 19–20 Dec 2019 at 1/100 of its price,
/// MON100 on 17 Jun 2021 at 1/10). Returns their dates. A run still open at the end of the
/// series can't be confirmed and is kept.
pub fn bad_prints(bars: &[PriceBar]) -> Vec<NaiveDate> {
    let valid: Vec<&PriceBar> = bars.iter().filter(|b| b.adjusted_close() > 0.0).collect();
    let prices: Vec<f64> = valid.iter().map(|b| b.adjusted_close()).collect();
    let r = indicators::log_returns(&prices);
    let mut out = Vec::new();
    let mut i = 0;
    while i < r.len() {
        if r[i].abs() >= BAD_PRINT_JUMP {
            // r[i] moves into bar i + 1; look for the move back out within the run limit.
            let back = (i + 1..r.len().min(i + 1 + BAD_PRINT_MAX_RUN)).find(|&j| {
                r[j].signum() != r[i].signum()
                    && r[j].abs() >= BAD_PRINT_JUMP
                    && (r[i..=j].iter().sum::<f64>()).abs() < 0.25 * r[i].abs()
            });
            if let Some(j) = back {
                out.extend(valid[i + 1..=j].iter().map(|b| b.date));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Price ratios (new ÷ old) of the usual splits and bonus issues: 1:2, 1:3, 1:4, 1:5, 1:10
/// splits, a 1:2 bonus (2/3), and the reverse consolidations.
const SPLIT_RATIOS: [f64; 10] = [
    0.5,
    1.0 / 3.0,
    0.25,
    0.2,
    0.1,
    2.0 / 3.0,
    2.0,
    3.0,
    5.0,
    10.0,
];
/// How close (log) a day's jump must be to a split ratio: a normal day's move on top.
const SPLIT_TOLERANCE: f64 = 0.03;
/// Smaller jumps are left alone: beyond NSE's 20% band, and 2/3 (a 1:2 bonus) still qualifies.
const SPLIT_MIN_JUMP: f64 = 0.37;

/// Splits and bonus issues the vendor hasn't adjusted for: a jump that lands on a usual split
/// ratio and stays (run after `bad_prints`, which removes jumps that come back). Yahoo:
/// TECHM.NS halved on 19 Nov 2026 with no split recorded, and its volatility read 6.7× normal.
/// Returns (first bar at the new price level, price ratio new ÷ old).
pub fn unrecorded_splits(bars: &[PriceBar]) -> Vec<(NaiveDate, f64)> {
    let valid: Vec<&PriceBar> = bars.iter().filter(|b| b.adjusted_close() > 0.0).collect();
    valid
        .windows(2)
        .filter_map(|w| {
            let r = (w[1].adjusted_close() / w[0].adjusted_close()).ln();
            if r.abs() < SPLIT_MIN_JUMP {
                return None;
            }
            SPLIT_RATIOS
                .iter()
                .find(|k| (r - k.ln()).abs() < SPLIT_TOLERANCE)
                .map(|k| (w[1].date, *k))
        })
        .collect()
}

/// Puts every bar before each split on the new price level, as a vendor's split adjustment does.
pub fn apply_splits(bars: &mut [PriceBar], splits: &[(NaiveDate, f64)]) {
    for (date, ratio) in splits {
        for b in bars.iter_mut().filter(|b| b.date < *date) {
            b.open *= ratio;
            b.high *= ratio;
            b.low *= ratio;
            b.close *= ratio;
            b.adj_close = b.adj_close.map(|a| a * ratio);
        }
    }
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    })
}

fn median_abs_deviation(values: &[f64]) -> Option<f64> {
    let m = median(values)?;
    median(&values.iter().map(|v| (v - m).abs()).collect::<Vec<_>>())
}

#[cfg(test)]
mod split_tests {
    use super::*;

    fn bars(closes: &[f64]) -> Vec<PriceBar> {
        let start = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| PriceBar {
                symbol: "X".into(),
                date: start + chrono::Days::new(i as u64),
                open: *c,
                high: *c,
                low: *c,
                close: *c,
                adj_close: Some(*c),
                volume: 1,
                source: "t".into(),
            })
            .collect()
    }

    #[test]
    fn an_unrecorded_halving_is_a_split_and_a_crash_is_not() {
        // TECHM.NS, 17-20 Nov 2026: 1762, 1750, then 861.5 and 873.1 (no split on record).
        let mut b = bars(&[1762.0, 1750.0, 861.5, 873.1]);
        let found = unrecorded_splits(&b);
        assert_eq!(found, vec![(b[2].date, 0.5)]);
        apply_splits(&mut b, &found);
        assert!((b[1].adjusted_close() - 875.0).abs() < 1e-9 && b[2].close == 861.5);
        // A 30% crash, and a 10% day, are real moves.
        assert!(unrecorded_splits(&bars(&[100.0, 70.0, 69.0])).is_empty());
        assert!(unrecorded_splits(&bars(&[100.0, 90.0])).is_empty());
        // A 1:2 bonus (price × 2/3) and a 1:10 split.
        assert_eq!(unrecorded_splits(&bars(&[300.0, 201.0]))[0].1, 2.0 / 3.0);
        assert_eq!(unrecorded_splits(&bars(&[5000.0, 496.0]))[0].1, 0.1);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Datelike;

    use super::*;

    #[test]
    fn bad_prints_are_spikes_that_revert_and_real_crashes_are_kept() {
        let bar = |d: u32, p: f64| PriceBar {
            symbol: "X".into(),
            date: day(d),
            open: p,
            high: p,
            low: p,
            close: p,
            adj_close: Some(p),
            volume: 1,
            source: "test".into(),
        };
        // Wiggles around 100, a two-day 1/100 print on days 10–11 (as GOLDBEES in Dec 2019), a real
        // 40% crash on day 20 that stays down, and a spike on the last day (unconfirmable).
        let mut bars: Vec<PriceBar> = (1..=28)
            .map(|d| bar(d, 100.0 + if d % 2 == 0 { 0.5 } else { -0.5 }))
            .collect();
        bars[9] = bar(10, 1.0);
        bars[10] = bar(11, 1.01);
        // A real 9% jump and back (as gold on 21 Jan 2026) is a market move, not a print.
        bars[14] = bar(15, 109.0);
        for b in bars.iter_mut().skip(19) {
            let p = b.close * 0.6;
            *b = bar(b.date.day0() + 1, p);
        }
        bars[27] = bar(28, 900.0);
        assert_eq!(bad_prints(&bars), vec![day(10), day(11)]);
        assert!(bad_prints(&bars[..5]).is_empty());
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, d).unwrap()
    }

    fn bar(d: u32, close: f64) -> PriceBar {
        PriceBar {
            symbol: "X".into(),
            date: day(d),
            open: close,
            high: close * 1.01,
            low: close * 0.99,
            close,
            adj_close: None,
            volume: 100,
            source: "test".into(),
        }
    }

    /// Deterministic wiggle so MAD > 0.
    fn series(n: u32) -> Vec<PriceBar> {
        (1..=n)
            .map(|d| bar(d, 100.0 * (1.0 + 0.01 * (d as f64 * 1.7).sin())))
            .collect()
    }

    #[test]
    fn clean_series_passes() {
        let bars = series(20);
        let report = assess(&bars, None, &[], day(21));
        assert!(report.passed, "{report:?}");
        assert!(report.outliers.is_empty() && report.missing_days.is_empty());
    }

    #[test]
    fn flags_impossible_prices_and_staleness() {
        let mut bars = series(10);
        bars[3].low = bars[3].high * 1.5; // low above high
        bars[5].close = 0.0;
        let report = assess(&bars, None, &[], day(30));
        assert_eq!(report.ohlc_violations, vec![day(4)]);
        assert_eq!(report.non_positive_prices, vec![day(6)]);
        assert!(report.is_stale);
        assert!(!report.passed);
    }

    #[test]
    fn missing_and_stale_against_reference_calendar() {
        let mut bars = series(10);
        bars.remove(4); // day 5 missing
        let reference = (1..=11).map(day).collect::<Vec<_>>(); // market traded on day 11 too
        let report = assess(&bars, Some(&reference), &[], day(11));
        assert_eq!(report.missing_days, vec![day(5)]);
        assert!(report.is_stale);
    }

    #[test]
    fn outlier_is_flagged_and_linked_to_split() {
        let mut bars = series(30);
        for b in &mut bars[20..] {
            b.close /= 2.0; // unadjusted 2:1 split on day 21
            b.open /= 2.0;
            b.high /= 2.0;
            b.low /= 2.0;
        }
        let report = assess(&bars, None, &[day(21)], day(30));
        assert_eq!(report.outliers.len(), 1);
        assert_eq!(report.outliers[0].date, day(21));
        assert!(report.outliers[0].near_corporate_action);
        assert!(report.passed, "outliers warn, they do not fail");
    }
}
