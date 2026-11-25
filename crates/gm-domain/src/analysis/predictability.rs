//! Is there any evidence the series is predictable? For most liquid stocks the honest answer is
//! "no", and the report says so.

use serde::{Deserialize, Serialize};
use statrs::distribution::{ChiSquared, ContinuousCDF, Normal};

use super::{TestResult, numeric};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Predictability {
    pub lags: usize,
    /// H0: no autocorrelation in returns up to `lags` (Ljung & Box 1978).
    pub ljung_box_returns: TestResult,
    /// H0: no autocorrelation in squared returns, i.e. no volatility clustering (ARCH effects).
    pub ljung_box_squared: TestResult,
    /// Lo & MacKinlay (1988) variance ratios with heteroskedasticity-robust z. H0: random walk.
    pub variance_ratios: Vec<VarianceRatio>,
    /// Augmented Dickey–Fuller on log price with a constant. H0: unit root (price wanders).
    pub adf: Adf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarianceRatio {
    pub q: usize,
    /// > 1: trending (momentum); < 1: mean-reverting.
    pub ratio: f64,
    pub test: TestResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adf {
    pub statistic: f64,
    pub lags: usize,
    pub n_obs: usize,
    /// MacKinnon (2010) finite-sample critical values at 1%, 5%, 10%.
    pub critical_values: [f64; 3],
    pub reject_unit_root_5pct: bool,
}

pub fn test(returns: &[f64], log_prices: &[f64]) -> Predictability {
    let lags = 10.min(returns.len() / 5);
    let squared = returns.iter().map(|r| r * r).collect::<Vec<_>>();
    Predictability {
        lags,
        ljung_box_returns: ljung_box(returns, lags),
        ljung_box_squared: ljung_box(&squared, lags),
        variance_ratios: [2, 5, 10]
            .into_iter()
            .filter(|q| returns.len() > q * 10)
            .map(|q| variance_ratio(log_prices, q))
            .collect(),
        adf: adf(log_prices),
    }
}

/// Q = n(n+2) Σ ρ_k² / (n−k), χ² with `lags` d.o.f.
pub fn ljung_box(x: &[f64], lags: usize) -> TestResult {
    let n = x.len() as f64;
    let m = numeric::mean(x);
    let denom = x.iter().map(|v| (v - m).powi(2)).sum::<f64>();
    let q = (1..=lags)
        .map(|k| {
            let rho = (k..x.len())
                .map(|t| (x[t] - m) * (x[t - k] - m))
                .sum::<f64>()
                / denom;
            rho * rho / (n - k as f64)
        })
        .sum::<f64>()
        * n
        * (n + 2.0);
    let p = ChiSquared::new(lags as f64)
        .map(|d| d.sf(q))
        .unwrap_or(f64::NAN);
    TestResult::new(q, p)
}

/// Overlapping, bias-corrected variance ratio with Lo–MacKinlay's heteroskedasticity-robust
/// z*(q). `y` is log price (nq + 1 points).
pub fn variance_ratio(y: &[f64], q: usize) -> VarianceRatio {
    let nq = (y.len() - 1) as f64;
    let qf = q as f64;
    let mu = (y[y.len() - 1] - y[0]) / nq;
    let diffs = y.windows(2).map(|w| w[1] - w[0] - mu).collect::<Vec<_>>();
    let sq_sum = diffs.iter().map(|d| d * d).sum::<f64>();
    let var_a = sq_sum / (nq - 1.0);
    let m = qf * (nq - qf + 1.0) * (1.0 - qf / nq);
    let var_c = (q..y.len())
        .map(|k| (y[k] - y[k - q] - qf * mu).powi(2))
        .sum::<f64>()
        / m;
    let ratio = var_c / var_a;
    let theta = (1..q)
        .map(|j| {
            let delta = nq
                * (j..diffs.len())
                    .map(|k| diffs[k].powi(2) * diffs[k - j].powi(2))
                    .sum::<f64>()
                / sq_sum.powi(2);
            (2.0 * (qf - j as f64) / qf).powi(2) * delta
        })
        .sum::<f64>();
    let z = (ratio - 1.0) / (theta / nq).sqrt();
    let p = Normal::standard().sf(z.abs()) * 2.0;
    VarianceRatio {
        q,
        ratio,
        test: TestResult::new(z, p),
    }
}

/// ADF with constant, fixed lag order 12·(n/100)^¼ (Schwert 1989), the statsmodels default
/// maximum. The statistic is t(γ) in Δy_t = a + γ·y_{t−1} + Σ δ_i·Δy_{t−i} + e_t.
pub fn adf(y: &[f64]) -> Adf {
    let n = y.len();
    let lags = (12.0 * (n as f64 / 100.0).powf(0.25)).floor() as usize;
    let (statistic, n_obs) = adf_statistic(y, lags, true);
    // MacKinnon (2010), Table 1, constant term, one variable: β∞ + β1/T + β2/T².
    let t = n_obs as f64;
    let crit = |b: [f64; 3]| b[0] + b[1] / t + b[2] / (t * t);
    let critical_values = [
        crit([-3.43035, -6.5393, -16.786]),
        crit([-2.86154, -2.8903, -4.234]),
        crit([-2.56677, -1.5384, -2.809]),
    ];
    Adf {
        statistic,
        lags,
        n_obs,
        critical_values,
        reject_unit_root_5pct: statistic < critical_values[1],
    }
}

/// ADF t-statistic of γ with a fixed lag order, with or without a constant. Returns
/// (statistic, observations used). No constant is the Engle–Granger residual test.
pub fn adf_statistic(y: &[f64], lags: usize, constant: bool) -> (f64, usize) {
    let dy = y.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut target = Vec::new();
    for t in lags..dy.len() {
        let mut row = vec![y[t]];
        if constant {
            row.push(1.0);
        }
        row.extend((1..=lags).map(|i| dy[t - i]));
        rows.push(row);
        target.push(dy[t]);
    }
    let n_obs = rows.len();
    let statistic = numeric::ols(&rows, &target).map_or(f64::NAN, |fit| fit.coef[0] / fit.se[0]);
    (statistic, n_obs)
}
