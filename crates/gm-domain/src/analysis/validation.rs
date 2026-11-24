//! Does a model earn its place? Walk forward through the history: each day, forecast the next
//! day's return distribution from data before that day only, then score it against what
//! happened. Models are compared with historical simulation, the standard baseline.

use serde::{Deserialize, Serialize};
use statrs::distribution::{Binomial, ChiSquared, ContinuousCDF, DiscreteCDF, Normal, StudentsT};

use super::{TestResult, numeric, volatility};

/// Days of history before the first out-of-sample forecast (≈ 2 trading years).
pub const WARMUP: usize = 500;
/// GARCH is re-estimated this often (≈ a quarter) and filtered daily in between, the usual
/// "refit periodically, update daily" practice; refitting daily changes little and costs a lot.
pub const GARCH_REFIT_EVERY: usize = 63;
/// Historical simulation looks back this many days (≈ 2 years).
pub const HISTORICAL_WINDOW: usize = 500;
/// Quantiles scored with the pinball loss; 1% and 5% double as the VaR levels.
pub const QUANTILES: [f64; 7] = [0.01, 0.05, 0.25, 0.5, 0.75, 0.95, 0.99];
const NORMAL_NU: f64 = 200.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    pub n_forecasts: usize,
    pub first_forecast_index: usize,
    pub baseline: String,
    pub models: Vec<ModelScore>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelScore {
    /// historical_sim (baseline) | ewma_normal | garch_t
    pub model: String,
    /// Mean pinball loss over the quantile grid; lower is better.
    pub mean_pinball: f64,
    /// Diebold–Mariano (1995) test of loss vs the baseline (Newey–West variance).
    /// Negative statistic = this model's loss is lower. None for the baseline itself.
    pub vs_baseline: Option<TestResult>,
    /// Share of outcomes inside the central 90% interval (target 0.90).
    pub coverage_90: f64,
    pub var_backtests: Vec<VarBacktest>,
    /// Sign of the forecast median vs the sign of the outcome. None for zero-median models
    /// (RiskMetrics forecasts no direction).
    pub direction: Option<Direction>,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarBacktest {
    pub level: f64,
    pub expected_rate: f64,
    pub exceptions: usize,
    pub rate: f64,
    /// Kupiec (1995) proportion of failures, H0: exception rate = 1 − level.
    pub kupiec: TestResult,
    /// Christoffersen (1998) independence, H0: exceptions don't cluster.
    pub christoffersen: TestResult,
    pub pass: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    pub n: usize,
    pub hits: usize,
    pub accuracy: f64,
    /// Two-sided exact binomial test against 50%.
    pub binomial: TestResult,
}

/// Per-day predictive quantiles (on the `QUANTILES` grid) for each model.
struct Forecasts {
    model: &'static str,
    quantiles: Vec<[f64; 7]>,
}

/// None when the history is too short for a meaningful out-of-sample sample (WARMUP + 100).
pub fn validate(returns: &[f64]) -> Option<Validation> {
    if returns.len() < WARMUP + 100 {
        return None;
    }
    let outcomes = &returns[WARMUP..];
    let forecasts = [
        historical_sim(returns),
        ewma_normal(returns),
        garch_t(returns),
    ];
    let losses = forecasts
        .iter()
        .map(|f| {
            f.quantiles
                .iter()
                .zip(outcomes)
                .map(|(q, y)| pinball(q, *y))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let baseline = &losses[0];
    let models = forecasts
        .iter()
        .zip(&losses)
        .enumerate()
        .map(|(i, (f, loss))| score(f, loss, (i > 0).then_some(baseline.as_slice()), outcomes))
        .collect();
    Some(Validation {
        n_forecasts: outcomes.len(),
        first_forecast_index: WARMUP,
        baseline: "historical_sim".into(),
        models,
    })
}

fn historical_sim(r: &[f64]) -> Forecasts {
    let quantiles = (WARMUP..r.len())
        .map(|t| {
            let window = numeric::sorted(&r[t.saturating_sub(HISTORICAL_WINDOW)..t]);
            QUANTILES.map(|p| numeric::quantile_sorted(&window, p))
        })
        .collect();
    Forecasts {
        model: "historical_sim",
        quantiles,
    }
}

/// RiskMetrics: zero mean, EWMA variance, normal shocks.
fn ewma_normal(r: &[f64]) -> Forecasts {
    let z = standard_quantiles(f64::INFINITY);
    let mut var = numeric::variance(&r[..WARMUP]);
    for x in &r[..WARMUP] {
        var = volatility::EWMA_LAMBDA * var + (1.0 - volatility::EWMA_LAMBDA) * x * x;
    }
    let mut quantiles = Vec::with_capacity(r.len() - WARMUP);
    for x in &r[WARMUP..] {
        let s = var.sqrt();
        quantiles.push(z.map(|q| s * q));
        var = volatility::EWMA_LAMBDA * var + (1.0 - volatility::EWMA_LAMBDA) * x * x;
    }
    Forecasts {
        model: "ewma_normal",
        quantiles,
    }
}

/// GARCH(1,1)-t re-estimated every `GARCH_REFIT_EVERY` days on all data so far, filtered daily.
fn garch_t(r: &[f64]) -> Forecasts {
    let mut quantiles = Vec::with_capacity(r.len() - WARMUP);
    let mut t = WARMUP;
    while t < r.len() {
        let end = (t + GARCH_REFIT_EVERY).min(r.len());
        let history = &r[..t];
        let mu = numeric::mean(history);
        match volatility::fit_garch(history, 252.0) {
            Some(g) => {
                let z = standard_quantiles(g.nu);
                let mut var = g.next_variance;
                for x in &r[t..end] {
                    let s = var.sqrt();
                    quantiles.push(z.map(|q| mu + s * q));
                    let e = x - mu;
                    var = g.omega + g.alpha * e * e + g.beta * var;
                }
            }
            // Not identifiable on this window: fall back to the sample distribution, normal.
            None => {
                let s = numeric::variance(history).sqrt();
                let z = standard_quantiles(f64::INFINITY);
                quantiles.extend((t..end).map(|_| z.map(|q| mu + s * q)));
            }
        }
        t = end;
    }
    Forecasts {
        model: "garch_t",
        quantiles,
    }
}

/// Quantiles of a unit-variance t(ν) (normal for large ν) on the QUANTILES grid.
fn standard_quantiles(nu: f64) -> [f64; 7] {
    if !nu.is_finite() || nu > NORMAL_NU {
        let n = Normal::standard();
        return QUANTILES.map(|p| n.inverse_cdf(p));
    }
    let scale = ((nu - 2.0) / nu).sqrt();
    match StudentsT::new(0.0, 1.0, nu) {
        Ok(t) => QUANTILES.map(|p| t.inverse_cdf(p) * scale),
        Err(_) => standard_quantiles(f64::INFINITY),
    }
}

/// Mean pinball (quantile) loss over the grid: (p − 1{y < q})(y − q).
pub fn pinball(q: &[f64; 7], y: f64) -> f64 {
    QUANTILES
        .iter()
        .zip(q)
        .map(|(p, q)| (p - f64::from(u8::from(y < *q))) * (y - q))
        .sum::<f64>()
        / QUANTILES.len() as f64
}

fn score(f: &Forecasts, loss: &[f64], baseline: Option<&[f64]>, outcomes: &[f64]) -> ModelScore {
    let vs_baseline = baseline.map(|b| {
        let d = loss.iter().zip(b).map(|(l, b)| l - b).collect::<Vec<_>>();
        diebold_mariano(&d)
    });
    let coverage_90 = f
        .quantiles
        .iter()
        .zip(outcomes)
        .filter(|(q, y)| **y >= q[1] && **y <= q[5])
        .count() as f64
        / outcomes.len() as f64;
    // VaR at 99% and 95% = the 1% and 5% quantiles of the forecast.
    let var_backtests = [(0.99, 0), (0.95, 1)]
        .map(|(level, qi)| {
            let hits = f
                .quantiles
                .iter()
                .zip(outcomes)
                .map(|(q, y)| *y < q[qi])
                .collect::<Vec<_>>();
            var_backtest(level, &hits)
        })
        .to_vec();
    let direction = direction(f, outcomes);
    let mean_pinball = numeric::mean(loss);
    let var_ok = var_backtests.iter().all(|v| v.pass);
    let verdict = match vs_baseline {
        None => "Baseline: what any model has to beat.".to_string(),
        Some(t) if t.reject_5pct && t.statistic < 0.0 && var_ok => {
            "Earns its place: forecasts beat historical simulation out of sample and its VaR holds up.".into()
        }
        Some(t) if t.reject_5pct && t.statistic < 0.0 => {
            "Sharper than the baseline overall, but its VaR fails a backtest; don't use its tail numbers.".into()
        }
        Some(t) if t.reject_5pct => "Worse than historical simulation out of sample.".into(),
        Some(_) => "No significant difference from historical simulation out of sample.".into(),
    };
    ModelScore {
        model: f.model.into(),
        mean_pinball,
        vs_baseline,
        coverage_90,
        var_backtests,
        direction,
        verdict,
    }
}

/// DM statistic with a Newey–West long-run variance (Bartlett kernel, lag ⌊4(T/100)^{2/9}⌋).
pub fn diebold_mariano(d: &[f64]) -> TestResult {
    let n = d.len() as f64;
    let m = numeric::mean(d);
    let lag = (4.0 * (n / 100.0).powf(2.0 / 9.0)).floor() as usize;
    let gamma = |k: usize| {
        (k..d.len())
            .map(|t| (d[t] - m) * (d[t - k] - m))
            .sum::<f64>()
            / n
    };
    let lrv = gamma(0)
        + 2.0
            * (1..=lag)
                .map(|k| (1.0 - k as f64 / (lag as f64 + 1.0)) * gamma(k))
                .sum::<f64>();
    let stat = m / (lrv / n).sqrt();
    TestResult::new(stat, 2.0 * Normal::standard().sf(stat.abs()))
}

/// x·ln(y) with 0·ln 0 = 0.
fn xlny(x: f64, y: f64) -> f64 {
    if x == 0.0 { 0.0 } else { x * y.ln() }
}

pub fn var_backtest(level: f64, hits: &[bool]) -> VarBacktest {
    let n = hits.len() as f64;
    let x = hits.iter().filter(|h| **h).count();
    let xf = x as f64;
    let p = 1.0 - level;
    let rate = xf / n;
    let chi1 = ChiSquared::new(1.0).ok();
    let sf = |v: f64| chi1.as_ref().map_or(f64::NAN, |c| c.sf(v.max(0.0)));
    let lr_pof =
        -2.0 * (xlny(n - xf, 1.0 - p) + xlny(xf, p) - xlny(n - xf, 1.0 - rate) - xlny(xf, rate));
    // Transition counts n_ij: state i yesterday → j today.
    let mut c = [[0.0f64; 2]; 2];
    for w in hits.windows(2) {
        c[usize::from(w[0])][usize::from(w[1])] += 1.0;
    }
    let pi0 = c[0][1] / (c[0][0] + c[0][1]).max(1.0);
    let pi1 = c[1][1] / (c[1][0] + c[1][1]).max(1.0);
    let pi = (c[0][1] + c[1][1]) / (c.iter().flatten().sum::<f64>()).max(1.0);
    let lr_ind = -2.0
        * (xlny(c[0][0] + c[1][0], 1.0 - pi) + xlny(c[0][1] + c[1][1], pi)
            - xlny(c[0][0], 1.0 - pi0)
            - xlny(c[0][1], pi0)
            - xlny(c[1][0], 1.0 - pi1)
            - xlny(c[1][1], pi1));
    let kupiec = TestResult::new(lr_pof, sf(lr_pof));
    let christoffersen = TestResult::new(lr_ind, sf(lr_ind));
    VarBacktest {
        level,
        expected_rate: p,
        exceptions: x,
        rate,
        pass: !kupiec.reject_5pct && !christoffersen.reject_5pct,
        kupiec,
        christoffersen,
    }
}

fn direction(f: &Forecasts, outcomes: &[f64]) -> Option<Direction> {
    let (mut n, mut hits) = (0usize, 0usize);
    for (q, y) in f.quantiles.iter().zip(outcomes) {
        let median = q[3];
        if median != 0.0 && *y != 0.0 {
            n += 1;
            hits += usize::from((median > 0.0) == (*y > 0.0));
        }
    }
    if n == 0 {
        return None;
    }
    // Exact two-sided binomial test vs 0.5: double the smaller tail, capped at 1.
    let b = Binomial::new(0.5, n as u64).ok()?;
    let p = (2.0 * b.cdf(hits.min(n - hits) as u64)).min(1.0);
    Some(Direction {
        n,
        hits,
        accuracy: hits as f64 / n as f64,
        binomial: TestResult::new(hits as f64, p),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_distr::{Distribution, StudentT};

    #[track_caller]
    fn close(a: f64, b: f64, rel: f64) {
        assert!((a - b).abs() <= rel * b.abs().max(1e-300), "{a} vs {b}");
    }

    /// Reference values computed independently with numpy/scipy (see the PR for the script).
    #[test]
    fn kupiec_and_christoffersen_match_reference() {
        let mut hits = (0..1000).map(|i| i % 37 == 0).collect::<Vec<_>>();
        hits[500..503].iter_mut().for_each(|h| *h = true);
        let b = var_backtest(0.99, &hits);
        assert_eq!(b.exceptions, 31);
        close(b.kupiec.statistic, 28.595_568_973_457_29, 1e-10);
        close(b.kupiec.p_value, 8.918_603_695_938_275e-8, 1e-6);
        close(b.christoffersen.statistic, 1.077_208_813_288_351, 1e-10);
        close(b.christoffersen.p_value, 0.299_322_803_662_137_26, 1e-8);
        assert!(!b.pass, "31 exceptions where 10 were expected must fail");
    }

    #[test]
    fn diebold_mariano_matches_reference() {
        let d = (0..800)
            .map(|i| {
                let i = f64::from(i);
                (i * 0.7).sin() * 0.01 - 0.002 + 0.003 * (i * 0.13).cos()
            })
            .collect::<Vec<_>>();
        let t = diebold_mariano(&d);
        close(t.statistic, -7.608_196_793_801_828, 1e-9);
        assert!(t.reject_5pct);
    }

    #[test]
    fn exact_binomial_direction_test_matches_scipy() {
        let p = |hits: usize, n: u64| {
            let b = Binomial::new(0.5, n).unwrap();
            (2.0 * b.cdf(hits.min(n as usize - hits) as u64)).min(1.0)
        };
        close(p(560, 1000), 0.000_165_049_870_550_452_86, 1e-6);
        close(p(37, 80), 0.576_430_626_188_393_1, 1e-9);
    }

    #[test]
    fn pinball_is_zero_only_at_a_perfect_point_forecast() {
        let q = [0.0; 7];
        assert_eq!(pinball(&q, 0.0), 0.0);
        // y above every quantile: loss = mean(p)·y = 0.5·y.
        close(pinball(&q, 0.02), 0.01, 1e-12);
        close(pinball(&q, -0.02), 0.01, 1e-12);
    }

    /// On data that really is GARCH-t, the GARCH-t forecaster must beat historical simulation
    /// and pass its VaR backtests: the harness can tell a right model from a baseline.
    #[test]
    fn correct_model_earns_its_place_on_its_own_data() {
        let (omega, alpha, beta, nu): (f64, f64, f64, f64) = (4e-6, 0.10, 0.86, 6.0);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(11);
        let t = StudentT::new(nu).unwrap();
        let scale = ((nu - 2.0) / nu).sqrt();
        let mut var = omega / (1.0 - alpha - beta);
        let r = (0..2500)
            .map(|_| {
                let e = var.sqrt() * t.sample(&mut rng) * scale;
                var = omega + alpha * e * e + beta * var;
                e
            })
            .collect::<Vec<_>>();
        let v = validate(&r).unwrap();
        assert_eq!(v.n_forecasts, 2000);
        let garch = v.models.iter().find(|m| m.model == "garch_t").unwrap();
        let dm = garch.vs_baseline.unwrap();
        assert!(
            dm.statistic < 0.0 && dm.reject_5pct,
            "GARCH-t should beat the baseline: {dm:?}"
        );
        assert!(
            garch.var_backtests.iter().all(|b| b.pass),
            "{:?}",
            garch.var_backtests
        );
        assert!(
            (garch.coverage_90 - 0.90).abs() < 0.02,
            "coverage {}",
            garch.coverage_90
        );
        assert!(garch.verdict.starts_with("Earns its place"));
        // Zero-mean RiskMetrics forecasts no direction.
        assert!(
            v.models
                .iter()
                .find(|m| m.model == "ewma_normal")
                .unwrap()
                .direction
                .is_none()
        );
    }

    #[test]
    fn short_history_is_not_validated() {
        assert!(validate(&vec![0.001; WARMUP + 99]).is_none());
    }
}
