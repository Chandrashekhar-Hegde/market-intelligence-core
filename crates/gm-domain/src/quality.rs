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
mod tests {
    use super::*;

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
