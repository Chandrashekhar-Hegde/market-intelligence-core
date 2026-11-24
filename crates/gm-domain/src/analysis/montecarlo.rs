//! Forward price distributions by simulation: GBM (constant volatility, normal shocks) and
//! GARCH(1,1)-t (volatility clustering, fat tails). Seeded ChaCha8, so identical inputs and
//! seed give byte-identical output.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal, StudentT};
use serde::{Deserialize, Serialize};

use super::{numeric, volatility::Garch};

pub const QUANTILES: [f64; 5] = [0.05, 0.25, 0.5, 0.75, 0.95];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub model: String,
    pub n_paths: usize,
    pub seed: u64,
    /// Price quantiles (5/25/50/75/95%) for every day up to the longest horizon.
    pub fan: Vec<FanPoint>,
    pub horizons: Vec<HorizonSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanPoint {
    pub day: usize,
    pub price: [f64; 5],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonSummary {
    pub horizon_days: usize,
    /// Simple-return quantiles at 5/25/50/75/95%.
    pub return_quantiles: [f64; 5],
    pub prob_gain: f64,
    pub prob_loss_over_10pct: f64,
    /// P(the path falls ≥ 10% below its running peak at some point within the horizon).
    pub prob_drawdown_over_10pct: f64,
}

pub struct Inputs<'a> {
    pub last_price: f64,
    /// Daily mean and sd of log returns.
    pub mu: f64,
    pub sigma: f64,
    pub garch: Option<&'a Garch>,
    pub horizons: &'a [usize],
    pub n_paths: usize,
    pub seed: u64,
}

pub fn project(inputs: &Inputs) -> Vec<Projection> {
    let days = inputs.horizons.iter().copied().max().unwrap_or(0);
    let mut out = vec![simulate("gbm", inputs, days, |rng, _| {
        inputs.mu + inputs.sigma * normal(rng)
    })];
    if let Some(g) = inputs.garch {
        let t = StudentT::new(g.nu).ok();
        let scale = ((g.nu - 2.0) / g.nu).sqrt();
        out.push(simulate("garch_t", inputs, days, |rng, var: &mut f64| {
            let z = match &t {
                Some(t) => t.sample(rng) * scale,
                None => normal(rng),
            };
            let e = var.sqrt() * z;
            *var = g.omega + g.alpha * e * e + g.beta * *var;
            inputs.mu + e
        }));
    }
    out
}

fn normal(rng: &mut ChaCha8Rng) -> f64 {
    StandardNormal.sample(rng)
}

fn simulate(
    model: &str,
    inputs: &Inputs,
    days: usize,
    step: impl Fn(&mut ChaCha8Rng, &mut f64) -> f64,
) -> Projection {
    // Separate stream per model so adding a model never changes another model's numbers.
    let stream = model
        .bytes()
        .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    let mut rng = ChaCha8Rng::seed_from_u64(inputs.seed ^ stream);
    let start_var = inputs.garch.map_or(0.0, |g| g.next_variance);
    let n = inputs.n_paths;
    // log-price paths, day-major: paths[d][p]
    let mut paths = vec![vec![0.0f64; n]; days];
    let mut worst_dd = vec![vec![0.0f64; n]; days];
    for p in 0..n {
        let (mut log_price, mut peak, mut dd, mut var) = (0.0f64, 0.0f64, 0.0f64, start_var);
        for d in 0..days {
            log_price += step(&mut rng, &mut var);
            peak = peak.max(log_price);
            dd = dd.max(1.0 - (log_price - peak).exp());
            paths[d][p] = log_price;
            worst_dd[d][p] = dd;
        }
    }
    let fan = paths
        .iter()
        .enumerate()
        .map(|(d, day)| {
            let s = numeric::sorted(day);
            FanPoint {
                day: d + 1,
                price: QUANTILES.map(|q| inputs.last_price * numeric::quantile_sorted(&s, q).exp()),
            }
        })
        .collect();
    let horizons = inputs
        .horizons
        .iter()
        .filter(|&&h| h >= 1 && h <= days)
        .map(|&h| {
            let s = numeric::sorted(&paths[h - 1]);
            let share = |pred: &dyn Fn(f64) -> bool| {
                s.iter().filter(|x| pred(**x)).count() as f64 / n as f64
            };
            HorizonSummary {
                horizon_days: h,
                return_quantiles: QUANTILES.map(|q| numeric::quantile_sorted(&s, q).exp() - 1.0),
                prob_gain: share(&|x| x > 0.0),
                prob_loss_over_10pct: share(&|x| x.exp() - 1.0 <= -0.10),
                prob_drawdown_over_10pct: worst_dd[h - 1].iter().filter(|dd| **dd >= 0.10).count()
                    as f64
                    / n as f64,
            }
        })
        .collect();
    Projection {
        model: model.to_string(),
        n_paths: n,
        seed: inputs.seed,
        fan,
        horizons,
    }
}
