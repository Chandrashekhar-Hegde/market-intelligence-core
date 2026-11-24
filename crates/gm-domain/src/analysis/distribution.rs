//! What do the returns look like? Moments, normality, tail heaviness.

use serde::{Deserialize, Serialize};
use statrs::distribution::{ChiSquared, ContinuousCDF};

use super::{TestResult, numeric};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub mean_daily: f64,
    pub vol_daily: f64,
    /// Annualised with the series' own observed trading days per year.
    pub mean_annual: f64,
    pub vol_annual: f64,
    /// Standard error of the annual mean: drift is estimated far less precisely than volatility.
    pub mean_annual_se: f64,
    pub skewness: f64,
    pub excess_kurtosis: f64,
    /// H0: returns are normal (Jarque & Bera 1980).
    pub jarque_bera: TestResult,
    /// Hill (1975) tail index of the loss tail: lower = fatter. Normal-like tails give
    /// large values; equities are typically 2–5. None when there are too few losses.
    pub hill_tail_index: Option<f64>,
    pub hill_k: usize,
    pub worst_day: f64,
    pub best_day: f64,
}

/// `returns` are daily log returns; `periods_per_year` from the series' own calendar.
pub fn describe(returns: &[f64], periods_per_year: f64) -> Distribution {
    let n = returns.len() as f64;
    let m = numeric::mean(returns);
    let sd = numeric::variance(returns).sqrt();
    // Population (biased) central moments, as scipy.stats.skew/kurtosis and the JB statistic use.
    let m2 = returns.iter().map(|r| (r - m).powi(2)).sum::<f64>() / n;
    let m3 = returns.iter().map(|r| (r - m).powi(3)).sum::<f64>() / n;
    let m4 = returns.iter().map(|r| (r - m).powi(4)).sum::<f64>() / n;
    let skewness = m3 / m2.powf(1.5);
    let excess_kurtosis = m4 / (m2 * m2) - 3.0;
    let jb = n / 6.0 * (skewness.powi(2) + excess_kurtosis.powi(2) / 4.0);
    let jb_p = ChiSquared::new(2.0).map(|d| d.sf(jb)).unwrap_or(f64::NAN);
    let (hill_tail_index, hill_k) = hill(returns);
    Distribution {
        mean_daily: m,
        vol_daily: sd,
        mean_annual: m * periods_per_year,
        vol_annual: sd * periods_per_year.sqrt(),
        mean_annual_se: sd / n.sqrt() * periods_per_year,
        skewness,
        excess_kurtosis,
        jarque_bera: TestResult::new(jb, jb_p),
        hill_tail_index,
        hill_k,
        worst_day: returns.iter().copied().fold(f64::INFINITY, f64::min),
        best_day: returns.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

/// Hill estimator on the k largest losses, k = 5% of the sample (min 10), a common rule of
/// thumb; the estimate is sensitive to k, so it's reported with k.
fn hill(returns: &[f64]) -> (Option<f64>, usize) {
    let mut losses = returns
        .iter()
        .filter(|r| **r < 0.0)
        .map(|r| -r)
        .collect::<Vec<_>>();
    losses.sort_by(|a, b| b.total_cmp(a));
    let k = ((returns.len() as f64 * 0.05).round() as usize).max(10);
    if losses.len() <= k {
        return (None, k);
    }
    let threshold = losses[k];
    let xi = losses[..k]
        .iter()
        .map(|x| (x / threshold).ln())
        .sum::<f64>()
        / k as f64;
    ((xi > 0.0).then(|| 1.0 / xi), k)
}
