//! Several instruments together: how they move with each other, what they share, which pairs
//! are tied by a long-run relationship, and how they performed side by side.
//! Pure, like the rest of `analysis`: price series in, report out.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use super::{numeric, predictability, risk};

pub const MIN_SYMBOLS: usize = 2;
pub const MAX_SYMBOLS: usize = 10;
/// Fewer common days than this and correlations are too noisy to report.
pub const MIN_COMMON_DAYS: usize = 120;
/// Window for rolling correlation (≈ one quarter).
pub const ROLLING_WINDOW: usize = 63;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub symbols: Vec<String>,
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    /// Trading days every instrument has a price for.
    pub n_common_days: usize,
    pub n_weekly: usize,
    /// Pearson correlation of daily log returns on common days.
    pub correlation_daily: Vec<Vec<f64>>,
    /// Rank (Spearman) correlation of daily returns: robust to outliers.
    pub spearman_daily: Vec<Vec<f64>>,
    /// Pearson correlation of weekly returns. Use this across time zones: a Tokyo close and a
    /// New York close on the "same" date are 14 hours apart, which biases daily correlation down.
    pub correlation_weekly: Vec<Vec<f64>>,
    /// Ledoit–Wolf (2004) shrinkage of the daily covariance toward a scaled identity.
    pub covariance_shrinkage: f64,
    pub covariance_annual: Vec<Vec<f64>>,
    pub pca: Pca,
    pub pairs: Vec<Pair>,
    pub rolling_correlation: Vec<RollingPair>,
    pub performance: Vec<Performance>,
    /// Each symbol rebased to 100 at the first common date, sampled for charting.
    pub rebased: Vec<RebasedPoint>,
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pca {
    /// Share of total variance explained by each component (of the correlation matrix).
    pub explained: Vec<f64>,
    /// Loadings of the first component, sign-normalised so most are positive.
    pub first_loadings: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub correlation_daily: f64,
    pub correlation_weekly: f64,
    pub cointegration: Cointegration,
}

/// Engle & Granger (1987): regress log price of `dependent` on `regressor`, then test the residual
/// spread for a unit root. Both orderings are tested; the more significant one is reported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cointegration {
    pub dependent: String,
    pub regressor: String,
    /// log(dependent) ≈ intercept + hedge_ratio · log(regressor)
    pub hedge_ratio: f64,
    pub statistic: f64,
    pub lags: usize,
    /// MacKinnon (2010) critical values, two variables with a constant, at 1/5/10%.
    pub critical_values: [f64; 3],
    pub cointegrated_5pct: bool,
    /// Ornstein–Uhlenbeck half-life of the spread in trading days, when it mean-reverts.
    pub half_life_days: Option<f64>,
    /// How far the spread is from its mean today, in standard deviations.
    pub spread_z_now: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollingPair {
    pub a: String,
    pub b: String,
    /// (date, correlation over the trailing window), sampled weekly.
    pub points: Vec<(NaiveDate, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub symbol: String,
    pub total_return: f64,
    pub annual_return: f64,
    pub annual_vol: f64,
    pub sharpe: f64,
    pub max_drawdown: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RebasedPoint {
    pub date: NaiveDate,
    pub values: Vec<f64>,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum CompareError {
    #[error("compare {MIN_SYMBOLS}-{MAX_SYMBOLS} instruments, got {0}")]
    Count(usize),
    #[error("only {0} trading days are common to every instrument; need {MIN_COMMON_DAYS}")]
    TooFewCommonDays(usize),
    #[error("symbol listed twice: {0}")]
    Duplicate(String),
}

/// `series`: (symbol, adjusted closes oldest first). Symbols must be distinct.
pub fn compare(series: &[(String, Vec<(NaiveDate, f64)>)]) -> Result<Comparison, CompareError> {
    let k = series.len();
    if !(MIN_SYMBOLS..=MAX_SYMBOLS).contains(&k) {
        return Err(CompareError::Count(k));
    }
    let mut seen = BTreeSet::new();
    if let Some((dup, _)) = series.iter().find(|(s, _)| !seen.insert(s.clone())) {
        return Err(CompareError::Duplicate(dup.clone()));
    }
    let maps = series
        .iter()
        .map(|(_, p)| {
            p.iter()
                .copied()
                .filter(|(_, v)| v.is_finite() && *v > 0.0)
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let common = maps[0]
        .keys()
        .filter(|d| maps[1..].iter().all(|m| m.contains_key(d)))
        .copied()
        .collect::<Vec<_>>();
    if common.len() < MIN_COMMON_DAYS {
        return Err(CompareError::TooFewCommonDays(common.len()));
    }
    let symbols = series.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>();
    let prices = maps
        .iter()
        .map(|m| common.iter().map(|d| m[d]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let logp = prices
        .iter()
        .map(|p| p.iter().map(|v| v.ln()).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let returns = logp
        .iter()
        .map(|l| l.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let years = (common[common.len() - 1] - common[0]).num_days() as f64 / 365.25;
    let ppy = returns[0].len() as f64 / years;

    let weekly = weekly_returns(&common, &logp);
    let correlation_daily = corr_matrix(&returns);
    let spearman_daily = corr_matrix(&returns.iter().map(|r| ranks(r)).collect::<Vec<_>>());
    let correlation_weekly = corr_matrix(&weekly);
    let (cov, covariance_shrinkage) = ledoit_wolf(&returns);
    let covariance_annual = cov
        .iter()
        .map(|row| row.iter().map(|c| c * ppy).collect())
        .collect();
    let pca = pca(&correlation_daily);

    let mut pairs = Vec::new();
    let mut rolling_correlation = Vec::new();
    for i in 0..k {
        for j in i + 1..k {
            pairs.push(Pair {
                a: symbols[i].clone(),
                b: symbols[j].clone(),
                correlation_daily: correlation_daily[i][j],
                correlation_weekly: correlation_weekly[i][j],
                cointegration: engle_granger(&symbols, i, j, &logp),
            });
            rolling_correlation.push(RollingPair {
                a: symbols[i].clone(),
                b: symbols[j].clone(),
                points: rolling(&common[1..], &returns[i], &returns[j]),
            });
        }
    }
    let performance = symbols
        .iter()
        .zip(&prices)
        .zip(&returns)
        .map(|((s, p), r)| {
            let dated = common
                .iter()
                .copied()
                .zip(p.iter().copied())
                .collect::<Vec<_>>();
            let mean = numeric::mean(r);
            let sd = numeric::variance(r).sqrt();
            Performance {
                symbol: s.clone(),
                total_return: p[p.len() - 1] / p[0] - 1.0,
                annual_return: (p[p.len() - 1] / p[0]).powf(1.0 / years) - 1.0,
                annual_vol: sd * ppy.sqrt(),
                sharpe: mean / sd * ppy.sqrt(),
                max_drawdown: risk::drawdown(&dated).max,
            }
        })
        .collect::<Vec<_>>();
    // Sample the rebased series to at most ~400 points so the payload stays small.
    let stride = common.len().div_ceil(400).max(1);
    let rebased = (0..common.len())
        .filter(|t| t % stride == 0 || *t == common.len() - 1)
        .map(|t| RebasedPoint {
            date: common[t],
            values: prices.iter().map(|p| 100.0 * p[t] / p[0]).collect(),
        })
        .collect();

    let mut report = Comparison {
        symbols,
        first_date: common[0],
        last_date: common[common.len() - 1],
        n_common_days: common.len(),
        n_weekly: weekly[0].len(),
        correlation_daily,
        spearman_daily,
        correlation_weekly,
        covariance_shrinkage,
        covariance_annual,
        pca,
        pairs,
        rolling_correlation,
        performance,
        rebased,
        findings: Vec::new(),
    };
    report.findings = findings(&report);
    Ok(report)
}

fn corr(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (numeric::mean(x), numeric::mean(y));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
    }
    sxy / (sxx * syy).sqrt()
}

fn corr_matrix(series: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let k = series.len();
    (0..k)
        .map(|i| {
            (0..k)
                .map(|j| {
                    if i == j {
                        1.0
                    } else {
                        corr(&series[i], &series[j])
                    }
                })
                .collect()
        })
        .collect()
}

/// Ranks 1..n with ties given their average rank (as scipy.stats.rankdata).
fn ranks(x: &[f64]) -> Vec<f64> {
    let mut idx = (0..x.len()).collect::<Vec<_>>();
    idx.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut out = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && x[idx[j + 1]] == x[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for &p in &idx[i..=j] {
            out[p] = avg;
        }
        i = j + 1;
    }
    out
}

/// Last common close of each ISO week, then week-over-week log returns.
fn weekly_returns(dates: &[NaiveDate], logp: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut week_end = Vec::new();
    for (t, d) in dates.iter().enumerate() {
        let w = d.iso_week();
        if t + 1 == dates.len() || dates[t + 1].iso_week() != w {
            week_end.push(t);
        }
    }
    logp.iter()
        .map(|l| week_end.windows(2).map(|w| l[w[1]] - l[w[0]]).collect())
        .collect()
}

/// Ledoit & Wolf (2004) shrinkage toward μ·I, exactly as scikit-learn's `ledoit_wolf`
/// (daily returns centred, covariance with 1/n). Returns (shrunk covariance, shrinkage).
pub fn ledoit_wolf(returns: &[Vec<f64>]) -> (Vec<Vec<f64>>, f64) {
    let p = returns.len();
    let n = returns[0].len();
    let nf = n as f64;
    let x = returns
        .iter()
        .map(|r| {
            let m = numeric::mean(r);
            r.iter().map(|v| v - m).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let emp = (0..p)
        .map(|i| {
            (0..p)
                .map(|j| (0..n).map(|t| x[i][t] * x[j][t]).sum::<f64>() / nf)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let trace = (0..p).map(|i| emp[i][i]).sum::<f64>();
    let mu = trace / p as f64;
    // beta_ = Σ (X²ᵀX²), delta_ = Σ (XᵀX)² / n²
    let beta_sum = (0..p)
        .map(|i| {
            (0..p)
                .map(|j| {
                    (0..n)
                        .map(|t| x[i][t].powi(2) * x[j][t].powi(2))
                        .sum::<f64>()
                })
                .sum::<f64>()
        })
        .sum::<f64>();
    let delta_sum = emp.iter().flatten().map(|c| (c * nf).powi(2)).sum::<f64>() / (nf * nf);
    let beta = (beta_sum / nf - delta_sum) / (p as f64 * nf);
    let delta = (delta_sum - 2.0 * mu * trace + p as f64 * mu * mu) / p as f64;
    let shrinkage = if beta == 0.0 {
        0.0
    } else {
        beta.min(delta) / delta
    };
    let shrunk = (0..p)
        .map(|i| {
            (0..p)
                .map(|j| (1.0 - shrinkage) * emp[i][j] + if i == j { shrinkage * mu } else { 0.0 })
                .collect()
        })
        .collect();
    (shrunk, shrinkage)
}

/// Eigen-decomposition of the (symmetric) correlation matrix by cyclic Jacobi rotations.
/// ponytail: Jacobi is ~30 lines and exact enough for ≤ 10×10; nalgebra if matrices grow.
// Index loops mirror the textbook Jacobi rotation (rows and columns p, q); iterators obscure it.
#[allow(clippy::needless_range_loop)]
fn pca(corr: &[Vec<f64>]) -> Pca {
    let k = corr.len();
    let mut a = corr.to_vec();
    let mut v = (0..k)
        .map(|i| {
            (0..k)
                .map(|j| f64::from(u8::from(i == j)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for _ in 0..100 {
        let off = (0..k)
            .flat_map(|i| (0..k).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j].powi(2))
            .sum::<f64>();
        if off < 1e-22 {
            break;
        }
        for p in 0..k {
            for q in p + 1..k {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
                for r in 0..k {
                    let (arp, arq) = (a[r][p], a[r][q]);
                    a[r][p] = c * arp - s * arq;
                    a[r][q] = s * arp + c * arq;
                }
                for r in 0..k {
                    let (apr, aqr) = (a[p][r], a[q][r]);
                    a[p][r] = c * apr - s * aqr;
                    a[q][r] = s * apr + c * aqr;
                }
                for r in 0..k {
                    let (vrp, vrq) = (v[r][p], v[r][q]);
                    v[r][p] = c * vrp - s * vrq;
                    v[r][q] = s * vrp + c * vrq;
                }
            }
        }
    }
    let mut order = (0..k).collect::<Vec<_>>();
    order.sort_by(|&x, &y| a[y][y].total_cmp(&a[x][x]));
    let total = (0..k).map(|i| a[i][i]).sum::<f64>();
    let explained = order.iter().map(|&i| a[i][i] / total).collect();
    let first = order[0];
    let mut loadings = (0..k).map(|r| v[r][first]).collect::<Vec<_>>();
    if loadings.iter().sum::<f64>() < 0.0 {
        loadings.iter_mut().for_each(|l| *l = -*l);
    }
    Pca {
        explained,
        first_loadings: loadings,
    }
}

fn engle_granger(symbols: &[String], i: usize, j: usize, logp: &[Vec<f64>]) -> Cointegration {
    let one = |dep: usize, reg: usize| {
        let rows = logp[reg].iter().map(|x| vec![1.0, *x]).collect::<Vec<_>>();
        let fit = numeric::ols(&rows, &logp[dep]);
        let (hedge, resid) = fit.map_or((f64::NAN, vec![0.0; logp[dep].len()]), |f| {
            (f.coef[1], f.residuals)
        });
        let lags = (12.0 * (resid.len() as f64 / 100.0).powf(0.25)).floor() as usize;
        let (statistic, _) = predictability::adf_statistic(&resid, lags, false);
        (dep, reg, hedge, resid, statistic, lags)
    };
    let a = one(i, j);
    let b = one(j, i);
    let (dep, reg, hedge, resid, statistic, lags) = if a.4 <= b.4 { a } else { b };
    // MacKinnon (2010) τ_c for N = 2, at statsmodels' coint() sample size: series length − 1.
    let t = (logp[dep].len() as f64 - 1.0).max(1.0);
    let crit = |c: [f64; 3]| c[0] + c[1] / t + c[2] / (t * t);
    let critical_values = [
        crit([-3.89644, -10.9519, -33.527]),
        crit([-3.33613, -6.1101, -6.823]),
        crit([-3.04445, -4.2412, -2.720]),
    ];
    let cointegrated_5pct = statistic < critical_values[1];
    // OU half-life from Δs_t = a + φ·s_{t−1}: −ln 2 / ln(1 + φ), only when φ < 0.
    let rows = resid[..resid.len() - 1]
        .iter()
        .map(|s| vec![1.0, *s])
        .collect::<Vec<_>>();
    let ds = resid.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>();
    let half_life_days = numeric::ols(&rows, &ds)
        .map(|f| f.coef[1])
        .filter(|phi| *phi < 0.0 && *phi > -1.0)
        .map(|phi| -std::f64::consts::LN_2 / (1.0 + phi).ln());
    let sd = numeric::variance(&resid).sqrt();
    Cointegration {
        dependent: symbols[dep].clone(),
        regressor: symbols[reg].clone(),
        hedge_ratio: hedge,
        statistic,
        lags,
        critical_values,
        cointegrated_5pct,
        half_life_days,
        spread_z_now: if sd > 0.0 {
            resid[resid.len() - 1] / sd
        } else {
            0.0
        },
    }
}

fn rolling(dates: &[NaiveDate], x: &[f64], y: &[f64]) -> Vec<(NaiveDate, f64)> {
    (ROLLING_WINDOW..=x.len())
        .filter(|end| (end - ROLLING_WINDOW).is_multiple_of(5) || *end == x.len())
        .map(|end| {
            (
                dates[end - 1],
                corr(&x[end - ROLLING_WINDOW..end], &y[end - ROLLING_WINDOW..end]),
            )
        })
        .collect()
}

fn findings(c: &Comparison) -> Vec<String> {
    let mut out = Vec::new();
    let pc1 = c.pca.explained.first().copied().unwrap_or(0.0);
    out.push(format!(
        "{} trading days in common ({} → {}). One shared factor explains {:.0}% of the daily variance of these {} instruments{}.",
        c.n_common_days,
        c.first_date,
        c.last_date,
        pc1 * 100.0,
        c.symbols.len(),
        if pc1 > 0.6 { ": they mostly move together, so holding them together diversifies little" } else { "" }
    ));
    if let Some(p) = c
        .pairs
        .iter()
        .max_by(|a, b| a.correlation_weekly.total_cmp(&b.correlation_weekly))
    {
        out.push(format!(
            "Most correlated: {} and {} (weekly {:.2}, daily {:.2}).",
            p.a, p.b, p.correlation_weekly, p.correlation_daily
        ));
    }
    if let Some(p) = c
        .pairs
        .iter()
        .min_by(|a, b| a.correlation_weekly.total_cmp(&b.correlation_weekly))
        && c.pairs.len() > 1
    {
        out.push(format!(
            "Least correlated: {} and {} (weekly {:.2}): the better diversifier of the set.",
            p.a, p.b, p.correlation_weekly
        ));
    }
    let gap = c
        .pairs
        .iter()
        .filter(|p| p.correlation_weekly - p.correlation_daily > 0.15)
        .map(|p| format!("{}/{}", p.a, p.b))
        .collect::<Vec<_>>();
    if !gap.is_empty() {
        out.push(format!(
            "Weekly correlation is well above daily for {}: these trade in different time zones, so trust the weekly figure.",
            gap.join(", ")
        ));
    }
    let coint = c
        .pairs
        .iter()
        .filter(|p| p.cointegration.cointegrated_5pct)
        .collect::<Vec<_>>();
    out.push(if coint.is_empty() {
        "No pair is cointegrated at 5%: none shows a stable long-run price relationship to trade around.".into()
    } else {
        coint
            .iter()
            .map(|p| {
                let ci = &p.cointegration;
                format!(
                    "{} and {} are cointegrated (EG {:.2} < {:.2}){}; the spread is {:.1} SD from its mean today.",
                    ci.dependent,
                    ci.regressor,
                    ci.statistic,
                    ci.critical_values[1],
                    ci.half_life_days.map_or(String::new(), |h| format!(", spread half-life {h:.0} days")),
                    ci.spread_z_now
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    });
    // Multiple testing: each pair is a separate 5% test, so some hits are expected by chance.
    let tested = c.pairs.len();
    if !coint.is_empty() && tested >= 3 {
        out.push(format!(
            "{tested} pairs were tested, so about {:.1} would look cointegrated at 5% by chance alone. Trust a hit only if there's an economic reason the two should move together (same business, index and its member, dual listing).",
            tested as f64 * 0.05
        ));
    }
    let best = c
        .performance
        .iter()
        .max_by(|a, b| a.sharpe.total_cmp(&b.sharpe));
    if let Some(b) = best {
        out.push(format!(
            "Best risk-adjusted performer over the period: {} (Sharpe {:.2}, {:.1}% a year, worst drawdown {:.1}%). Past performance, not a forecast.",
            b.symbol,
            b.sharpe,
            b.annual_return * 100.0,
            b.max_drawdown * 100.0
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn load(csv: &str) -> Vec<(NaiveDate, f64)> {
        csv.lines()
            .skip(1)
            .map(|l| {
                let (d, p) = l.split_once(',').unwrap();
                (d.parse().unwrap(), p.parse().unwrap())
            })
            .collect()
    }

    fn fixtures() -> Vec<(String, Vec<(NaiveDate, f64)>)> {
        vec![
            (
                "aapl".into(),
                load(include_str!("../../tests/fixtures/aapl_5y.csv")),
            ),
            (
                "gspc".into(),
                load(include_str!("../../tests/fixtures/gspc_5y.csv")),
            ),
            (
                "nsei".into(),
                load(include_str!("../../tests/fixtures/nsei_5y.csv")),
            ),
        ]
    }

    fn reference() -> Value {
        let all: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/reference.json")).unwrap();
        all["compare_aapl_gspc_nsei"].clone()
    }

    #[track_caller]
    fn close(a: f64, b: &Value, tol: f64, what: &str) {
        let b = b.as_f64().unwrap();
        assert!(
            (a - b).abs() <= tol * b.abs().max(1e-12),
            "{what}: {a} vs {b}"
        );
    }

    #[test]
    fn matches_pandas_sklearn_numpy_and_statsmodels() {
        let c = compare(&fixtures()).unwrap();
        let x = reference();
        assert_eq!(c.n_common_days as u64, x["n_common_days"].as_u64().unwrap());
        assert_eq!(c.n_weekly as u64, x["n_weekly"].as_u64().unwrap());
        for i in 0..3 {
            for j in 0..3 {
                close(
                    c.correlation_daily[i][j],
                    &x["pearson_daily"][i][j],
                    1e-10,
                    "pearson",
                );
                close(
                    c.spearman_daily[i][j],
                    &x["spearman_daily"][i][j],
                    1e-10,
                    "spearman",
                );
                close(
                    c.correlation_weekly[i][j],
                    &x["pearson_weekly"][i][j],
                    1e-10,
                    "weekly",
                );
            }
        }
        close(
            c.covariance_shrinkage,
            &x["ledoit_wolf_shrinkage"],
            1e-9,
            "LW shrinkage",
        );
        for (i, e) in c.pca.explained.iter().enumerate() {
            close(*e, &x["pca_explained"][i], 1e-9, "PCA");
        }
        for p in &c.pairs {
            let pair = &x["pairs"][format!("{}|{}", p.a, p.b)];
            let ci = &p.cointegration;
            let r = &pair[format!("{}~{}", ci.dependent, ci.regressor)];
            close(ci.statistic, &r["stat"], 1e-6, "EG statistic");
            close(ci.hedge_ratio, &r["hedge"], 1e-9, "hedge ratio");
            for k in 0..3 {
                close(
                    ci.critical_values[k],
                    &r["crit"][k],
                    1e-6,
                    "EG critical value",
                );
            }
            // The reported ordering is the more significant of the two.
            let other = &pair[format!("{}~{}", ci.regressor, ci.dependent)];
            assert!(ci.statistic <= other["stat"].as_f64().unwrap() + 1e-9);
            match (ci.half_life_days, r["half_life"].as_f64()) {
                (Some(h), Some(_)) => close(h, &r["half_life"], 1e-9, "half-life"),
                (None, None) => {}
                other => panic!("half-life mismatch {other:?}"),
            }
        }
        for perf in &c.performance {
            let r = &x["performance"][&perf.symbol];
            close(perf.total_return, &r["total_return"], 1e-10, "total return");
            close(
                perf.annual_return,
                &r["annual_return"],
                1e-10,
                "annual return",
            );
            close(perf.sharpe, &r["sharpe"], 1e-9, "Sharpe");
        }
    }

    #[test]
    fn ledoit_wolf_covariance_matches_sklearn() {
        let fx = fixtures();
        let c = compare(&fx).unwrap();
        let x = reference();
        // covariance_annual = daily LW covariance × periods per year.
        let ppy = c.covariance_annual[0][0] / x["ledoit_wolf_cov"][0][0].as_f64().unwrap();
        for i in 0..3 {
            for j in 0..3 {
                close(
                    c.covariance_annual[i][j] / ppy,
                    &x["ledoit_wolf_cov"][i][j],
                    1e-9,
                    "LW cov",
                );
            }
        }
    }

    #[test]
    fn detects_a_constructed_cointegrated_pair() {
        use rand::SeedableRng;
        use rand_distr::{Distribution, StandardNormal};
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(5);
        let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let (mut x, mut spread) = (4.0f64, 0.0f64);
        let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
        let mut walk = 4.0f64;
        for i in 0..1000u64 {
            let d = start + chrono::Days::new(i);
            let z: [f64; 3] = [
                StandardNormal.sample(&mut rng),
                StandardNormal.sample(&mut rng),
                StandardNormal.sample(&mut rng),
            ];
            x += 0.01 * z[0];
            spread = 0.9 * spread + 0.01 * z[1]; // AR(1), half-life ln2/−ln0.9 ≈ 6.6 days
            walk += 0.01 * z[2];
            a.push((d, x.exp()));
            b.push((d, (0.5 + 1.5 * x + spread).exp()));
            c.push((d, walk.exp()));
        }
        let r = compare(&[("X".into(), a), ("Y".into(), b), ("W".into(), c)]).unwrap();
        let xy = r.pairs.iter().find(|p| p.a == "X" && p.b == "Y").unwrap();
        assert!(xy.cointegration.cointegrated_5pct, "{:?}", xy.cointegration);
        let h = xy.cointegration.half_life_days.unwrap();
        assert!((4.0..10.0).contains(&h), "half-life {h}");
        if xy.cointegration.dependent == "Y" {
            assert!((xy.cointegration.hedge_ratio - 1.5).abs() < 0.05);
        }
        let xw = r.pairs.iter().find(|p| p.a == "X" && p.b == "W").unwrap();
        assert!(
            !xw.cointegration.cointegrated_5pct,
            "independent walks are not cointegrated"
        );
        assert!(r.findings.iter().any(|f| f.contains("cointegrated")));
        // Three pairs were tested and one hit: the chance-hit caveat is stated.
        assert!(
            r.findings.iter().any(|f| f.contains("3 pairs were tested")),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn rejects_bad_requests() {
        let fx = fixtures();
        assert_eq!(compare(&fx[..1]).unwrap_err(), CompareError::Count(1));
        let dup = vec![fx[0].clone(), fx[0].clone()];
        assert_eq!(
            compare(&dup).unwrap_err(),
            CompareError::Duplicate("aapl".into())
        );
        let short = fx
            .iter()
            .map(|(s, p)| (s.clone(), p[..50].to_vec()))
            .collect::<Vec<_>>();
        assert!(matches!(
            compare(&short),
            Err(CompareError::TooFewCommonDays(_))
        ));
    }

    #[test]
    fn rebased_series_start_at_100_and_stay_small() {
        let c = compare(&fixtures()).unwrap();
        assert!(
            c.rebased[0]
                .values
                .iter()
                .all(|v| (*v - 100.0).abs() < 1e-9)
        );
        assert!(c.rebased.len() <= 401);
        assert_eq!(c.rebased.last().unwrap().date, c.last_date);
        assert_eq!(c.rolling_correlation.len(), 3);
    }
}
