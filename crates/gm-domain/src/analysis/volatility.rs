//! Time-varying volatility: EWMA (RiskMetrics 1996) and GARCH(1,1) with Student-t errors
//! (Bollerslev 1986, 1987) fitted by maximum likelihood.

use serde::{Deserialize, Serialize};
use statrs::function::gamma::ln_gamma;

use super::numeric;

/// RiskMetrics (J.P. Morgan 1996) daily decay factor.
pub const EWMA_LAMBDA: f64 = 0.94;
/// Horizons (trading days) for the volatility term structure.
pub const TERM_HORIZONS: [usize; 4] = [1, 5, 21, 63];
/// Returns are scaled by 100 for fitting, as the `arch` package recommends, for numerical range.
const SCALE: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volatility {
    /// Next-day EWMA volatility, annualised.
    pub ewma_annual: f64,
    pub ewma_lambda: f64,
    pub garch: Option<Garch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Garch {
    /// Daily-decimal parameters: σ²_t = ω + α·e²_{t−1} + β·σ²_{t−1}, e_t = σ_t·z_t, z ~ std. t(ν).
    pub omega: f64,
    pub alpha: f64,
    pub beta: f64,
    pub nu: f64,
    pub persistence: f64,
    /// Days for a volatility shock to halve: ln ½ / ln(α+β).
    pub half_life_days: f64,
    pub long_run_vol_annual: f64,
    pub current_vol_annual: f64,
    /// Annualised average volatility expected over each horizon.
    pub term_structure: Vec<TermPoint>,
    pub log_likelihood: f64,
    pub converged: bool,
    /// Next-day conditional variance (daily decimal) and last residual, for simulation.
    pub next_variance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermPoint {
    pub horizon_days: usize,
    pub vol_annual: f64,
}

pub fn estimate(returns: &[f64], periods_per_year: f64) -> Volatility {
    Volatility {
        ewma_annual: ewma(returns).sqrt() * periods_per_year.sqrt(),
        ewma_lambda: EWMA_LAMBDA,
        garch: fit_garch(returns, periods_per_year),
    }
}

/// Next-day EWMA variance on demeaned returns, seeded with the sample variance.
pub fn ewma(returns: &[f64]) -> f64 {
    let m = numeric::mean(returns);
    returns.iter().fold(numeric::variance(returns), |var, r| {
        EWMA_LAMBDA * var + (1.0 - EWMA_LAMBDA) * (r - m).powi(2)
    })
}

struct Params {
    omega: f64,
    alpha: f64,
    beta: f64,
    nu: f64,
}

/// Unconstrained → constrained: ω > 0, α, β ≥ 0, α + β < 1, ν > 2.
fn transform(x: &[f64]) -> Params {
    let sig = |v: f64| 1.0 / (1.0 + (-v).exp());
    let persistence = 0.9999 * sig(x[1]);
    let share = sig(x[2]);
    Params {
        omega: x[0].exp(),
        alpha: persistence * share,
        beta: persistence * (1.0 - share),
        nu: 2.05 + x[3].exp(),
    }
}

/// `arch`-style backcast: exponentially weighted mean of the first 75 squared residuals.
fn backcast(e: &[f64]) -> f64 {
    let tau = e.len().min(75);
    let w = (0..tau).map(|i| 0.94f64.powi(i as i32)).collect::<Vec<_>>();
    let total = w.iter().sum::<f64>();
    w.iter().zip(e).map(|(w, x)| w * x * x).sum::<f64>() / total
}

/// Conditional variances σ²_0..σ²_{n} (the last is the one-step-ahead forecast).
fn variances(p: &Params, e: &[f64], bc: f64) -> Vec<f64> {
    let mut s2 = Vec::with_capacity(e.len() + 1);
    let mut prev_e2 = bc;
    let mut prev_s2 = bc;
    for x in e {
        let v = p.omega + p.alpha * prev_e2 + p.beta * prev_s2;
        s2.push(v);
        prev_e2 = x * x;
        prev_s2 = v;
    }
    s2.push(p.omega + p.alpha * prev_e2 + p.beta * prev_s2);
    s2
}

fn log_likelihood(p: &Params, e: &[f64], bc: f64) -> f64 {
    let s2 = variances(p, e, bc);
    let nu = p.nu;
    let c = ln_gamma((nu + 1.0) / 2.0)
        - ln_gamma(nu / 2.0)
        - 0.5 * (std::f64::consts::PI * (nu - 2.0)).ln();
    e.iter()
        .zip(&s2)
        .map(|(x, v)| c - 0.5 * v.ln() - (nu + 1.0) / 2.0 * (1.0 + x * x / (v * (nu - 2.0))).ln())
        .sum()
}

pub fn fit_garch(returns: &[f64], periods_per_year: f64) -> Option<Garch> {
    if returns.len() < 250 {
        return None; // too few observations to identify α, β and ν
    }
    let m = numeric::mean(returns);
    let e = returns.iter().map(|r| (r - m) * SCALE).collect::<Vec<_>>();
    let var = numeric::variance(&e);
    let bc = backcast(&e);
    let objective = |x: &[f64]| {
        let p = transform(x);
        let ll = log_likelihood(&p, &e, bc);
        if ll.is_finite() { -ll } else { f64::MAX }
    };
    // Start at α=0.05, β=0.90, ν=8; ω matching the sample variance. Restart from the optimum
    // twice, which lets the simplex escape early collapse (standard practice for NM).
    let logit = |p: f64| (p / (1.0 - p)).ln();
    let mut x = vec![
        (var * 0.05).ln(),
        logit(0.95 / 0.9999),
        logit(0.05 / 0.95),
        (8.0f64 - 2.05).ln(),
    ];
    let mut converged = false;
    let mut best = f64::MAX;
    for _ in 0..3 {
        let (next, value, ok) = numeric::nelder_mead(objective, &x, 0.5, 1e-12, 4000);
        converged = ok && (best - value).abs() < 1e-7;
        x = next;
        best = value;
    }
    let p = transform(&x);
    let s2 = variances(&p, &e, bc);
    let next = s2[s2.len() - 1];
    let persistence = p.alpha + p.beta;
    let long_run = p.omega / (1.0 - persistence);
    let scale2 = SCALE * SCALE;
    let term_structure = super::volatility::TERM_HORIZONS
        .iter()
        .map(|&h| {
            // E[σ²_{T+i}] = VL + (α+β)^{i−1}·(σ²_{T+1} − VL)
            let avg = (1..=h)
                .map(|i| long_run + persistence.powi(i as i32 - 1) * (next - long_run))
                .sum::<f64>()
                / h as f64;
            TermPoint {
                horizon_days: h,
                vol_annual: (avg / scale2 * periods_per_year).sqrt(),
            }
        })
        .collect();
    Some(Garch {
        omega: p.omega / scale2,
        alpha: p.alpha,
        beta: p.beta,
        nu: p.nu,
        persistence,
        half_life_days: 0.5f64.ln() / persistence.ln(),
        long_run_vol_annual: (long_run / scale2 * periods_per_year).sqrt(),
        current_vol_annual: (next / scale2 * periods_per_year).sqrt(),
        term_structure,
        // Report the likelihood of the unscaled data: each obs gains ln(SCALE).
        log_likelihood: -best + e.len() as f64 * SCALE.ln(),
        converged,
        next_variance: next / scale2,
    })
}
