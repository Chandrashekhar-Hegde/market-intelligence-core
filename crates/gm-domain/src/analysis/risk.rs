//! Downside risk: VaR / expected shortfall, drawdowns, risk-adjusted return, and exposure to a
//! benchmark when one is supplied.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use statrs::distribution::{Continuous, ContinuousCDF, Normal, StudentsT};

use super::{numeric, volatility::Garch};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Risk {
    /// One-day Value-at-Risk and expected shortfall, as positive loss fractions.
    pub var_es: Vec<VarEs>,
    pub max_drawdown: Drawdown,
    /// Annualised, with a zero risk-free rate (stated, not hidden).
    pub sharpe: f64,
    /// Lo (2002) iid standard error of the annualised Sharpe ratio.
    pub sharpe_se: f64,
    pub sortino: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarEs {
    /// historical | student_t (unconditional) | garch_t (conditional on today's volatility)
    pub method: String,
    pub confidence: f64,
    pub var: f64,
    pub es: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawdown {
    pub max: f64,
    pub peak_date: NaiveDate,
    pub trough_date: NaiveDate,
    /// None while the price is still below the old peak.
    pub recovery_date: Option<NaiveDate>,
    pub duration_days: i64,
    pub current: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkExposure {
    pub symbol: String,
    pub n_common_days: usize,
    pub beta: f64,
    pub beta_se: f64,
    /// Jensen's alpha, annualised (zero risk-free rate).
    pub alpha_annual: f64,
    pub alpha_annual_se: f64,
    pub r_squared: f64,
    pub correlation: f64,
    pub idiosyncratic_vol_annual: f64,
}

const LEVELS: [f64; 2] = [0.95, 0.99];
/// Above this many degrees of freedom the Student t is treated as normal.
const NORMAL_NU: f64 = 200.0;

pub fn measure(
    returns: &[f64],
    prices: &[(NaiveDate, f64)],
    garch: Option<&Garch>,
    periods_per_year: f64,
) -> Risk {
    let n = returns.len() as f64;
    let mu = numeric::mean(returns);
    let sd = numeric::variance(returns).sqrt();

    let mut var_es = Vec::new();
    let losses = numeric::sorted(&returns.iter().map(|r| 1.0 - r.exp()).collect::<Vec<_>>());
    for level in LEVELS {
        let var = numeric::quantile_sorted(&losses, level);
        let tail = losses
            .iter()
            .filter(|l| **l >= var)
            .copied()
            .collect::<Vec<_>>();
        var_es.push(VarEs {
            method: "historical".into(),
            confidence: level,
            var,
            es: numeric::mean(&tail),
        });
    }
    // Unconditional Student t: ν by the method of moments from excess kurtosis (6/(ν−4)).
    let kurt = {
        let m2 = returns.iter().map(|r| (r - mu).powi(2)).sum::<f64>() / n;
        returns.iter().map(|r| (r - mu).powi(4)).sum::<f64>() / n / (m2 * m2) - 3.0
    };
    let nu_mm = if kurt > 0.0 {
        4.0 + 6.0 / kurt
    } else {
        f64::INFINITY
    };
    for level in LEVELS {
        if let Some(v) = student_t_var_es("student_t", level, mu, sd, nu_mm) {
            var_es.push(v);
        }
    }
    if let Some(g) = garch {
        for level in LEVELS {
            if let Some(v) = student_t_var_es("garch_t", level, mu, g.next_variance.sqrt(), g.nu) {
                var_es.push(v);
            }
        }
    }

    let sharpe_daily = mu / sd;
    let downside = (returns.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / n).sqrt();
    Risk {
        var_es,
        max_drawdown: drawdown(prices),
        sharpe: sharpe_daily * periods_per_year.sqrt(),
        sharpe_se: ((1.0 + 0.5 * sharpe_daily * sharpe_daily) / n).sqrt() * periods_per_year.sqrt(),
        sortino: if downside > 0.0 {
            mu / downside * periods_per_year.sqrt()
        } else {
            f64::NAN
        },
    }
}

/// Left-tail VaR/ES of μ + σ·z, z ~ standardised t(ν), returned as positive simple losses.
pub(super) fn student_t_var_es(
    method: &str,
    level: f64,
    mu: f64,
    sigma: f64,
    nu: f64,
) -> Option<VarEs> {
    let alpha = 1.0 - level;
    if !nu.is_finite() || nu > NORMAL_NU {
        // t(ν) is numerically normal here, and statrs' t quantile fails to terminate for huge ν.
        let n = Normal::standard();
        let q = n.inverse_cdf(alpha);
        let (r_var, r_es) = (mu + sigma * q, mu - sigma * n.pdf(q) / alpha);
        return Some(VarEs {
            method: method.into(),
            confidence: level,
            var: 1.0 - r_var.exp(),
            es: 1.0 - r_es.exp(),
        });
    }
    let t = StudentsT::new(0.0, 1.0, nu).ok()?;
    let q = t.inverse_cdf(alpha);
    let s = ((nu - 2.0) / nu).sqrt();
    let r_var = mu + sigma * s * q;
    // E[T | T < q] = −f(q)/α · (ν + q²)/(ν − 1) for a standard t.
    let r_es = mu - sigma * s * t.pdf(q) / alpha * (nu + q * q) / (nu - 1.0);
    Some(VarEs {
        method: method.into(),
        confidence: level,
        var: 1.0 - r_var.exp(),
        es: 1.0 - r_es.exp(),
    })
}

pub fn drawdown(prices: &[(NaiveDate, f64)]) -> Drawdown {
    let (mut peak_i, mut best) = (0usize, (0.0f64, 0usize, 0usize));
    for (i, (_, p)) in prices.iter().enumerate() {
        if *p > prices[peak_i].1 {
            peak_i = i;
        }
        let dd = 1.0 - p / prices[peak_i].1;
        if dd > best.0 {
            best = (dd, peak_i, i);
        }
    }
    let (max, peak, trough) = best;
    let recovery = prices[trough..]
        .iter()
        .find(|(_, p)| *p >= prices[peak].1)
        .map(|(d, _)| *d);
    let end = recovery.unwrap_or(prices[prices.len() - 1].0);
    let running_peak = prices.iter().map(|(_, p)| *p).fold(f64::MIN, f64::max);
    Drawdown {
        max,
        peak_date: prices[peak].0,
        trough_date: prices[trough].0,
        recovery_date: recovery,
        duration_days: (end - prices[peak].0).num_days(),
        current: 1.0 - prices[prices.len() - 1].1 / running_peak,
    }
}

/// OLS of the asset's returns on the benchmark's, on common dates only.
pub fn exposure(
    symbol: &str,
    asset: &[(NaiveDate, f64)],
    benchmark: &[(NaiveDate, f64)],
    periods_per_year: f64,
) -> Option<BenchmarkExposure> {
    let bench = benchmark
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    let common = asset
        .iter()
        .filter_map(|(d, p)| bench.get(d).map(|b| (*p, *b)))
        .collect::<Vec<_>>();
    if common.len() < 60 {
        return None;
    }
    let pairs = common
        .windows(2)
        .map(|w| ((w[1].0 / w[0].0).ln(), (w[1].1 / w[0].1).ln()))
        .collect::<Vec<_>>();
    let rows = pairs.iter().map(|(_, m)| vec![1.0, *m]).collect::<Vec<_>>();
    let y = pairs.iter().map(|(a, _)| *a).collect::<Vec<_>>();
    let fit = numeric::ols(&rows, &y)?;
    Some(BenchmarkExposure {
        symbol: symbol.to_string(),
        n_common_days: common.len(),
        beta: fit.coef[1],
        beta_se: fit.se[1],
        alpha_annual: fit.coef[0] * periods_per_year,
        alpha_annual_se: fit.se[0] * periods_per_year,
        r_squared: fit.r_squared,
        correlation: fit.coef[1].signum() * fit.r_squared.sqrt(),
        idiosyncratic_vol_annual: numeric::variance(&fit.residuals).sqrt()
            * periods_per_year.sqrt(),
    })
}
