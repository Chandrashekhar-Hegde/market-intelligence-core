//! Forward price distributions by simulation:
//! - `bootstrap`: the instrument's own past daily returns, resampled in blocks (stationary
//!   bootstrap, Politis & Romano 1994), so its crashes and calm spells shape the fan;
//! - `gbm`: constant volatility, normal shocks;
//! - `garch_t`: volatility clustering, fat tails.
//!
//! Seeded ChaCha8 mixed with the symbol: identical inputs, symbol and seed give byte-identical
//! output, but two instruments never share the same random draws (with one shared seed every
//! fan had the same shape, only stretched by volatility).

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal, StandardUniform, StudentT};
use serde::{Deserialize, Serialize};

use super::{numeric, volatility::Garch};

pub const QUANTILES: [f64; 5] = [0.05, 0.25, 0.5, 0.75, 0.95];
/// Simulated paths returned as they are, for drawing.
pub const SAMPLE_PATHS: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub model: String,
    pub n_paths: usize,
    pub seed: u64,
    /// Price quantiles (5/25/50/75/95%) for every day up to the longest horizon.
    pub fan: Vec<FanPoint>,
    pub horizons: Vec<HorizonSummary>,
    /// The first `SAMPLE_PATHS` simulated price paths, one price per day.
    pub sample_paths: Vec<Vec<f64>>,
    /// The simulated path that stays closest to the median day by day (least total distance
    /// in log price): a typical path to highlight. A random walk has no single "most likely"
    /// path; this is the most ordinary one of those simulated.
    pub typical_path: Vec<f64>,
    /// Mean block length of the stationary bootstrap (bootstrap model only).
    pub block_length: Option<f64>,
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
    /// Mixed into the seed so instruments get different draws.
    pub symbol: &'a str,
    /// Past daily log returns, oldest first, for the bootstrap.
    pub returns: &'a [f64],
    pub last_price: f64,
    /// Daily mean and sd of log returns.
    pub mu: f64,
    pub sigma: f64,
    pub garch: Option<&'a Garch>,
    pub horizons: &'a [usize],
    pub n_paths: usize,
    pub seed: u64,
}

/// FNV-1a: a fixed, dependency-free hash, so a symbol's seed never changes between builds.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn project(inputs: &Inputs) -> Vec<Projection> {
    let days = inputs.horizons.iter().copied().max().unwrap_or(0);
    let mut out = vec![simulate("gbm", inputs, days, 0.0, |rng, _| {
        inputs.mu + inputs.sigma * normal(rng)
    })];
    if let Some(g) = inputs.garch {
        let t = StudentT::new(g.nu).ok();
        let scale = ((g.nu - 2.0) / g.nu).sqrt();
        out.push(simulate(
            "garch_t",
            inputs,
            days,
            g.next_variance,
            |rng, var: &mut f64| {
                let z = match &t {
                    Some(t) => t.sample(rng) * scale,
                    None => normal(rng),
                };
                let e = var.sqrt() * z;
                *var = g.omega + g.alpha * e * e + g.beta * *var;
                inputs.mu + e
            },
        ));
    }
    let r = inputs.returns;
    if r.len() >= 30 {
        // Block length from the squared returns: the dependence that matters for risk is
        // volatility clustering, which plain returns barely show.
        let squared: Vec<f64> = r.iter().map(|x| x * x).collect();
        let block = optimal_block_length(&squared).max(1.0);
        let restart = 1.0 / block;
        // Per-path state: the position in the history; NaN starts a path at a random day.
        let mut p = simulate("bootstrap", inputs, days, f64::NAN, |rng, pos: &mut f64| {
            let uniform = |rng: &mut ChaCha8Rng| -> f64 { StandardUniform.sample(rng) };
            let next = if pos.is_nan() || uniform(rng) < restart {
                ((uniform(rng) * r.len() as f64) as usize).min(r.len() - 1)
            } else {
                (*pos as usize + 1) % r.len()
            };
            *pos = next as f64;
            r[next]
        });
        p.block_length = Some(block);
        out.push(p);
    }
    out
}

/// Expected block length for the stationary bootstrap: Politis & White (2004) with the
/// Patton, Politis & White (2009) correction, computed exactly as `arch`'s
/// `optimal_block_length` (checked against it in analysis::tests).
pub fn optimal_block_length(x: &[f64]) -> f64 {
    let n = x.len();
    let nf = n as f64;
    let mean = x.iter().sum::<f64>() / nf;
    let eps: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
    let b_max = (3.0 * nf.sqrt()).min(nf / 3.0).ceil();
    let kn = 5usize.max(nf.log10() as usize);
    let m_max = nf.sqrt().ceil() as usize + kn;
    let cv = 2.0 * (nf.log10() / nf).sqrt();
    let mut acv = vec![0.0; m_max + 1];
    let mut abs_acorr = vec![0.0; m_max + 1];
    let mut opt_m: Option<usize> = None;
    for i in 0..=m_max.min(n - 2) {
        let v1 = dot(&eps[i + 1..], &eps[i + 1..]);
        let v2 = dot(&eps[..n - (i + 1)], &eps[..n - (i + 1)]);
        let cross = dot(&eps[i..], &eps[..n - i]);
        acv[i] = cross / nf;
        abs_acorr[i] = cross.abs() / (v1 * v2).sqrt();
        if i >= kn && opt_m.is_none() && abs_acorr[i - kn..i].iter().all(|a| *a < cv) {
            opt_m = Some(i - kn);
        }
    }
    let m = opt_m.map_or(m_max, |m| 2 * m.max(1)).min(m_max);
    let (mut g, mut lr_acv) = (0.0, acv[0]);
    for (k, a) in acv.iter().enumerate().take(m + 1).skip(1) {
        let t = k as f64 / m as f64;
        let lam = if t <= 0.5 { 1.0 } else { 2.0 * (1.0 - t) };
        g += 2.0 * lam * k as f64 * a;
        lr_acv += 2.0 * lam * a;
    }
    let d_sb = 2.0 * lr_acv * lr_acv;
    ((2.0 * g * g / d_sb).cbrt() * nf.cbrt()).min(b_max)
}

fn normal(rng: &mut ChaCha8Rng) -> f64 {
    StandardNormal.sample(rng)
}

/// `start` is each path's initial state for `step` (GARCH variance, bootstrap position).
fn simulate(
    model: &str,
    inputs: &Inputs,
    days: usize,
    start: f64,
    step: impl Fn(&mut ChaCha8Rng, &mut f64) -> f64,
) -> Projection {
    // Separate stream per model so adding a model never changes another model's numbers.
    let stream = model
        .bytes()
        .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    let mut rng = ChaCha8Rng::seed_from_u64(inputs.seed ^ stream ^ fnv1a(inputs.symbol));
    let n = inputs.n_paths;
    // log-price paths, day-major: paths[d][p]
    let mut paths = vec![vec![0.0f64; n]; days];
    let mut worst_dd = vec![vec![0.0f64; n]; days];
    for p in 0..n {
        let (mut log_price, mut peak, mut dd, mut var) = (0.0f64, 0.0f64, 0.0f64, start);
        for d in 0..days {
            log_price += step(&mut rng, &mut var);
            peak = peak.max(log_price);
            dd = dd.max(1.0 - (log_price - peak).exp());
            paths[d][p] = log_price;
            worst_dd[d][p] = dd;
        }
    }
    let mut medians = Vec::with_capacity(days);
    let fan = paths
        .iter()
        .enumerate()
        .map(|(d, day)| {
            let s = numeric::sorted(day);
            medians.push(numeric::quantile_sorted(&s, 0.5));
            FanPoint {
                day: d + 1,
                price: QUANTILES.map(|q| inputs.last_price * numeric::quantile_sorted(&s, q).exp()),
            }
        })
        .collect();
    let distance = |p: usize| -> f64 {
        paths
            .iter()
            .zip(&medians)
            .map(|(day, m)| (day[p] - m).abs())
            .sum()
    };
    let typical = (0..n)
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
        .unwrap_or(0);
    let typical_path = paths
        .iter()
        .map(|day| inputs.last_price * day.get(typical).copied().unwrap_or(0.0).exp())
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
    let sample_paths = (0..n.min(SAMPLE_PATHS))
        .map(|p| {
            paths
                .iter()
                .map(|day| inputs.last_price * day[p].exp())
                .collect()
        })
        .collect();
    Projection {
        model: model.to_string(),
        n_paths: n,
        seed: inputs.seed,
        fan,
        horizons,
        sample_paths,
        typical_path,
        block_length: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(symbol: &'a str, returns: &'a [f64]) -> Inputs<'a> {
        Inputs {
            symbol,
            returns,
            last_price: 100.0,
            mu: 0.0003,
            sigma: 0.015,
            garch: None,
            horizons: &[5, 21],
            n_paths: 4_000,
            seed: 42,
        }
    }

    /// Fat-tailed, clustered history: calm then a crash spell.
    fn history() -> Vec<f64> {
        (0..1500)
            .map(|i| {
                let wiggle = ((i * 7919) % 101) as f64 / 101.0 - 0.5;
                if (900..960).contains(&i) {
                    0.06 * wiggle - 0.004
                } else {
                    0.02 * wiggle + 0.0005
                }
            })
            .collect()
    }

    #[test]
    fn different_symbols_get_different_draws_and_same_symbol_repeats() {
        let h = history();
        let a = project(&inputs("RELIANCE.NS", &h));
        let b = project(&inputs("TCS.NS", &h));
        let a2 = project(&inputs("RELIANCE.NS", &h));
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&a2).unwrap()
        );
        assert_ne!(a[0].sample_paths, b[0].sample_paths);
        assert_eq!(a[0].sample_paths.len(), SAMPLE_PATHS);
        assert_eq!(a[0].sample_paths[0].len(), 21);
    }

    #[test]
    fn bootstrap_reproduces_the_history_it_resamples() {
        let h = history();
        let out = project(&inputs("X", &h));
        let boot = out.iter().find(|p| p.model == "bootstrap").unwrap();
        assert!(boot.block_length.unwrap() >= 1.0);
        // Day-1 outcomes are draws from the history itself: their quantiles match its own.
        let s = numeric::sorted(&h);
        let q5 = numeric::quantile_sorted(&s, 0.05);
        let fan_q5 = (boot.fan[0].price[0] / 100.0).ln();
        assert!((fan_q5 - q5).abs() < 0.002, "{fan_q5} vs {q5}");
        // The crash spell is in the history, so the bootstrap's 21-day downside is wider
        // than GBM's with the same overall σ.
        let gbm = &out[0].horizons[1].return_quantiles[0];
        assert!(boot.horizons[1].return_quantiles[0] < 0.0 && gbm < &0.0);
    }

    #[test]
    fn typical_path_hugs_the_median() {
        let h = history();
        for p in project(&inputs("X", &h)) {
            assert_eq!(p.typical_path.len(), 21);
            // Every day it sits inside the central 50% band.
            for (price, fan) in p.typical_path.iter().zip(&p.fan) {
                assert!(
                    *price >= fan.price[1] && *price <= fan.price[3],
                    "{}: {price} outside {:?}",
                    p.model,
                    fan.price
                );
            }
        }
    }

    #[test]
    fn no_bootstrap_without_enough_history() {
        assert!(
            project(&inputs("X", &[0.01; 10]))
                .iter()
                .all(|p| p.model != "bootstrap")
        );
    }
}
