//! The decision most people face is not "can I time this?" but "should this money be here at
//! all, rather than in a fixed deposit?". For 1, 3 and 5 years this gives the distribution of
//! total log returns two ways:
//! - **resampled**: a stationary bootstrap (Politis & Romano 1994) of the instrument's own daily
//!   returns, block length from Politis & White (2004) on the returns themselves, so streaks and
//!   crashes are kept;
//! - **as it happened**: every overlapping window in the history, with how many separate
//!   (non-overlapping) periods that really is, so a "97 in 100" from two periods reads as rough.
//!
//! Percentiles are returned, so the web can compare with any deposit rate the user types.
//!
//! **Growth is uncertain, so each resampled future draws its own.** The returns are resampled
//! with their mean removed, and each path adds a drift drawn from N(drift, se²): ten years pin
//! an average down to about ±σ/√10, which for a single stock is ±10% a year.
//!
//! **A single stock grows like its market** (`Drift::from` the home index). Fitted on the NIFTY
//! 50, 2016–2026: the spread of the stocks' 10-year average returns (8% a year) is smaller than
//! sampling noise alone (10% a year), so the data can't tell their true growth apart
//! (method-of-moments τ̂ = 0). The group average (16%) is inflated because today's members are
//! past winners, so the index's own growth is used instead.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal, StandardUniform};
use serde::{Deserialize, Serialize};

use super::{montecarlo::optimal_block_length, numeric};

pub const YEARS: [usize; 3] = [1, 3, 5];
const PATHS: usize = 2_000;

/// Daily log drift and its standard error (sampling uncertainty of the historical mean).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Drift {
    pub mean_daily: f64,
    pub se_daily: f64,
}

impl Drift {
    pub fn of(returns: &[f64]) -> Self {
        let n = returns.len().max(1) as f64;
        Self {
            mean_daily: numeric::mean(returns),
            se_daily: numeric::variance(returns).sqrt() / n.sqrt(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongRun {
    pub years: usize,
    /// Total log return over the period at percentiles 0, 1, …, 100 (resampled).
    pub resampled: Vec<f64>,
    /// The same from every overlapping past window; empty when the history is shorter.
    pub historical: Vec<f64>,
    pub n_windows: usize,
    /// Non-overlapping periods in the history: how much evidence the windows really are.
    pub n_independent: usize,
    pub worst: Option<f64>,
    pub block_length: f64,
    /// Annualised drift used for the resampled futures, its SE, and where it came from.
    pub drift_annual: f64,
    pub drift_se_annual: f64,
    pub drift_from: String,
}

fn percentiles(mut x: Vec<f64>) -> Vec<f64> {
    if x.is_empty() {
        return Vec::new();
    }
    x.sort_by(f64::total_cmp);
    (0..=100)
        .map(|p| numeric::quantile_sorted(&x, f64::from(p) / 100.0))
        .collect()
}

/// `returns`: daily log returns, oldest first; `drift`: the growth to assume, `drift_from` where
/// it came from ("own history", or the home index for a single stock). None below a year.
pub fn long_run(
    returns: &[f64],
    periods_per_year: f64,
    drift: Drift,
    drift_from: &str,
    seed: u64,
) -> Option<Vec<LongRun>> {
    let ppy = periods_per_year.round().max(1.0) as usize;
    if returns.len() < ppy {
        return None;
    }
    let own_mean = numeric::mean(returns);
    let demeaned: Vec<f64> = returns.iter().map(|r| r - own_mean).collect();
    let block = optimal_block_length(returns).max(1.0);
    let restart = 1.0 / block;
    let mut cum = Vec::with_capacity(returns.len() + 1);
    cum.push(0.0);
    for r in returns {
        cum.push(cum[cum.len() - 1] + r);
    }
    Some(
        YEARS
            .iter()
            .map(|&years| {
                let len = years * ppy;
                let mut rng =
                    ChaCha8Rng::seed_from_u64(seed ^ (years as u64).wrapping_mul(0x9E37_79B9));
                let n = demeaned.len();
                let resampled: Vec<f64> = (0..PATHS)
                    .map(|_| {
                        let z: f64 = StandardNormal.sample(&mut rng);
                        let mu = drift.mean_daily + drift.se_daily * z;
                        let mut uniform = || -> f64 { StandardUniform.sample(&mut rng) };
                        let mut pos = ((uniform() * n as f64) as usize).min(n - 1);
                        let mut total = mu * len as f64;
                        for _ in 0..len {
                            total += demeaned[pos];
                            pos = if uniform() < restart {
                                ((uniform() * n as f64) as usize).min(n - 1)
                            } else {
                                (pos + 1) % n
                            };
                        }
                        total
                    })
                    .collect();
                let windows: Vec<f64> = (0..returns.len().saturating_sub(len - 1))
                    .map(|i| cum[i + len] - cum[i])
                    .collect();
                LongRun {
                    years,
                    resampled: percentiles(resampled),
                    worst: windows.iter().copied().reduce(f64::min),
                    n_windows: windows.len(),
                    n_independent: returns.len() / len,
                    historical: percentiles(windows),
                    block_length: block,
                    drift_annual: drift.mean_daily * periods_per_year,
                    drift_se_annual: drift.se_daily * periods_per_year,
                    drift_from: drift_from.to_string(),
                }
            })
            .collect(),
    )
}

/// Share of outcomes beating a deposit at `rate` a year (compounded), from 0–100 percentiles.
pub fn chance_to_beat(percentiles: &[f64], years: usize, rate: f64) -> Option<f64> {
    if percentiles.is_empty() {
        return None;
    }
    let hurdle = years as f64 * (1.0 + rate).ln();
    let above = percentiles.iter().filter(|x| **x > hurdle).count();
    Some(above as f64 / percentiles.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_beats_a_deposit_and_evidence_is_counted() {
        // 10 years of 12%-a-year drift with 15% volatility, deterministic noise.
        let n = 2520;
        let returns: Vec<f64> = (0..n)
            .map(|i| {
                0.12 / 252.0
                    + 0.15 / 252f64.sqrt() * (((i * 7919) % 1000) as f64 / 1000.0 - 0.5) * 3.46
            })
            .collect();
        let out = long_run(&returns, 252.0, Drift::of(&returns), "own history", 42).unwrap();
        assert_eq!(out.iter().map(|l| l.years).collect::<Vec<_>>(), [1, 3, 5]);
        let five = &out[2];
        assert_eq!((five.n_independent, five.n_windows), (2, 2520 - 1260 + 1));
        assert_eq!(five.resampled.len(), 101);
        // Median 5-year log return ≈ 5 × 12% = 0.60.
        assert!(
            (five.resampled[50] - 0.60).abs() < 0.08,
            "{}",
            five.resampled[50]
        );
        // Longer horizons beat a 7% deposit more often.
        let p = |l: &LongRun| chance_to_beat(&l.resampled, l.years, 0.07).unwrap();
        // Longer horizons beat a deposit at least as often (within the 1-in-101 resolution).
        assert!(p(&out[0]) < p(&out[1]) && p(&out[2]) >= p(&out[1]) - 0.03);
        assert!(p(&out[2]) > 0.7);
        // Same seed, same answer.
        assert_eq!(
            long_run(&returns, 252.0, Drift::of(&returns), "own history", 42).unwrap(),
            out
        );
        assert!(
            long_run(
                &returns[..100],
                252.0,
                Drift::of(&returns),
                "own history",
                42
            )
            .is_none()
        );
        assert!((five.drift_annual - 0.12).abs() < 0.01 && five.drift_se_annual > 0.04);
    }

    /// A stock's spectacular past growth doesn't carry over when its market's growth is used,
    /// and drift uncertainty pulls a noisy estimate's chances toward a coin flip.
    #[test]
    fn market_drift_and_drift_uncertainty_temper_past_winners() {
        let noise = |i: usize| (((i * 7919) % 1000) as f64 / 1000.0 - 0.5) * 3.46;
        let star: Vec<f64> = (0..2520)
            .map(|i| 0.40 / 252.0 + 0.45 / 252f64.sqrt() * noise(i))
            .collect();
        let market: Vec<f64> = (0..2520)
            .map(|i| 0.11 / 252.0 + 0.15 / 252f64.sqrt() * noise(i + 3))
            .collect();
        let p3 = |d: Drift| {
            let lr = long_run(&star, 252.0, d, "test", 7).unwrap();
            chance_to_beat(&lr[1].resampled, 3, 0.07).unwrap()
        };
        let own = p3(Drift::of(&star));
        let index = p3(Drift::of(&market));
        let no_uncertainty = p3(Drift {
            se_daily: 0.0,
            ..Drift::of(&star)
        });
        assert!(
            own > 0.9 && index < own - 0.25,
            "own {own}, with the index's growth {index}"
        );
        assert!(no_uncertainty > own, "{no_uncertainty} vs {own}");
    }

    #[test]
    fn chance_to_beat_counts_percentiles_above_the_hurdle() {
        let pct: Vec<f64> = (0..=100).map(|p| f64::from(p) / 100.0).collect(); // 0.00 … 1.00
        // Hurdle over 1 year at e^0.5 − 1: percentiles 51…100 are above.
        assert!(
            (chance_to_beat(&pct, 1, 0.5f64.exp() - 1.0).unwrap() - 50.0 / 101.0).abs() < 1e-12
        );
        assert_eq!(chance_to_beat(&[], 1, 0.07), None);
    }
}
