//! Has the app's advice paid off on this instrument? Its standing advice to a holder is "hold
//! less when it moves more than usual": exposure = min(1, normal volatility / forecast
//! volatility), the volatility-managed rule of Moreira & Muir (2017), never above fully invested.
//! This replays that rule day by day on the instrument's own history and sets it against simply
//! holding: growth, worst fall, return per unit of risk, and what each did in the instrument's
//! three worst falls (found in the data, not picked).
//!
//! - Forecast: EWMA variance (λ = 0.94, RiskMetrics) from returns up to the day before.
//! - "Normal": the average squared return over all history so far (after a year's warm-up).
//! - Rebalanced weekly (every 5 sessions); each change pays half a round trip (0.125%).
//! - The money not invested earns the fixed-deposit rate.
//!
//! Checked on the NIFTY 50 index, 2017–2026: worst fall −23% vs −38% holding, growth 10.4% vs
//! 11.0% a year, Sharpe 0.27 vs 0.24, Covid crash −21% vs −37%. On single NIFTY 50 stocks the worst
//! fall was smaller for 44 of 49, but growth higher for only 10: insurance, not extra return
//! (consistent with Liu, Tang & Zhou 2019). One-day shocks aren't avoided: the rule reacts to
//! volatility once it has started.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::{signals::ROUND_TRIP_COST, volatility::EWMA_LAMBDA};

/// Sessions between rebalances.
const REBALANCE: usize = 5;
/// History used before the first decision, to know what "normal" is.
const WARM_UP: usize = 250;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Perf {
    /// Compound growth a year.
    pub growth: f64,
    pub vol: f64,
    /// Return over the deposit per unit of volatility, a year.
    pub sharpe: f64,
    /// Worst peak-to-trough fall (a negative fraction).
    pub worst_fall: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// Total return over the episode, as a fraction.
    pub app: f64,
    pub hold: f64,
    /// Share invested at the start of the episode.
    pub exposure_at_start: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackRecord {
    pub first: NaiveDate,
    pub last: NaiveDate,
    pub cash_rate: f64,
    pub app: Perf,
    pub hold: Perf,
    pub average_exposure: f64,
    /// The three worst falls of simply holding, worst first.
    pub episodes: Vec<Episode>,
}

fn perf(r: &[f64], cash_daily: f64, periods_per_year: f64) -> Perf {
    let n = r.len() as f64;
    let mean = r.iter().sum::<f64>() / n;
    let sd = (r.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    let (mut cum, mut peak, mut worst) = (0.0f64, 0.0f64, 0.0f64);
    for x in r {
        cum += x;
        peak = peak.max(cum);
        worst = worst.min(cum - peak);
    }
    Perf {
        growth: (mean * periods_per_year).exp() - 1.0,
        vol: sd * periods_per_year.sqrt(),
        sharpe: if sd > 0.0 {
            (mean - cash_daily) / sd * periods_per_year.sqrt()
        } else {
            0.0
        },
        worst_fall: worst.exp() - 1.0,
    }
}

/// The `k` worst non-overlapping falls of a log-return path: (peak index, trough index).
fn worst_falls(r: &[f64], k: usize) -> Vec<(usize, usize)> {
    let mut cum = vec![0.0];
    for x in r {
        cum.push(cum[cum.len() - 1] + x);
    }
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for _ in 0..k {
        let free = |i: usize| taken.iter().all(|(a, b)| i < *a || i > *b);
        let mut best: Option<(f64, usize, usize)> = None;
        let mut peak: Option<usize> = None;
        for i in 0..cum.len() {
            if !free(i) {
                peak = None;
                continue;
            }
            if peak.is_none_or(|p| cum[i] > cum[p]) {
                peak = Some(i);
            }
            let p = peak.expect("set above");
            let fall = cum[i] - cum[p];
            if best.is_none_or(|(b, _, _)| fall < b) {
                best = Some((fall, p, i));
            }
        }
        match best {
            Some((fall, p, t)) if fall < 0.0 => taken.push((p, t)),
            _ => break,
        }
    }
    taken
}

/// `dates[i]` is the day of `returns[i]` (daily log returns, oldest first). None below two years.
pub fn track_record(
    dates: &[NaiveDate],
    returns: &[f64],
    periods_per_year: f64,
    cash_rate: f64,
) -> Option<TrackRecord> {
    let n = returns.len();
    if n != dates.len() || n < WARM_UP * 2 {
        return None;
    }
    let cash = (1.0 + cash_rate).ln() / periods_per_year;
    let mut ewma = returns[..WARM_UP].iter().map(|r| r * r).sum::<f64>() / WARM_UP as f64;
    let mut sum_sq = 0.0;
    let mut w = 1.0f64;
    let (mut app, mut hold, mut exposure) = (Vec::new(), Vec::new(), Vec::new());
    for (t, r) in returns.iter().enumerate() {
        if t >= WARM_UP {
            if (t - WARM_UP).is_multiple_of(REBALANCE) {
                let normal = sum_sq / t as f64;
                let target = if ewma > 0.0 {
                    (normal / ewma).sqrt().min(1.0)
                } else {
                    1.0
                };
                let cost = (target - w).abs() * ROUND_TRIP_COST / 2.0;
                w = target;
                app.push(w * r + (1.0 - w) * cash - cost);
            } else {
                app.push(w * r + (1.0 - w) * cash);
            }
            hold.push(*r);
            exposure.push(w);
        }
        ewma = EWMA_LAMBDA * ewma + (1.0 - EWMA_LAMBDA) * r * r;
        sum_sq += r * r;
    }
    let d = &dates[WARM_UP..];
    let total = |x: &[f64]| x.iter().sum::<f64>().exp() - 1.0;
    let episodes = worst_falls(&hold, 3)
        .into_iter()
        .map(|(p, t)| Episode {
            // Cumulative index p is "after p returns": the episode's returns are p..t.
            start: d[p.min(d.len() - 1)],
            end: d[(t.max(1) - 1).min(d.len() - 1)],
            app: total(&app[p..t]),
            hold: total(&hold[p..t]),
            exposure_at_start: exposure[p.min(exposure.len() - 1)],
        })
        .collect();
    Some(TrackRecord {
        first: d[0],
        last: d[d.len() - 1],
        cash_rate,
        app: perf(&app, cash, periods_per_year),
        hold: perf(&hold, cash, periods_per_year),
        average_exposure: exposure.iter().sum::<f64>() / exposure.len() as f64,
        episodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days(n: usize) -> Vec<NaiveDate> {
        let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
        (0..n)
            .map(|i| start + chrono::Days::new(i as u64))
            .collect()
    }

    /// Calm years with a slow drift, then a violent, drawn-out crash: holding less once the
    /// swings start saves most of the fall.
    #[test]
    fn hold_less_when_jumpy_saves_the_drawn_out_crash() {
        let mut r: Vec<f64> = (0..1500)
            .map(|i| 0.0004 + 0.008 * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        // 40 days of large swings that net to a deep fall, then calm again.
        for i in 0..40 {
            r.push(if i % 2 == 0 { -0.06 } else { 0.035 });
        }
        r.extend((0..300).map(|i| 0.0004 + 0.008 * if i % 2 == 0 { 1.0 } else { -1.0 }));
        let tr = track_record(&days(r.len()), &r, 252.0, 0.07).unwrap();
        assert!(
            tr.app.worst_fall > tr.hold.worst_fall + 0.15,
            "{:?} vs {:?}",
            tr.app,
            tr.hold
        );
        let crash = &tr.episodes[0];
        assert!(
            crash.hold < -0.35 && crash.app > crash.hold + 0.15,
            "{crash:?}"
        );
        assert!(
            crash.exposure_at_start > 0.9,
            "the first day of a shock isn't avoided"
        );
        assert!(tr.average_exposure > 0.8 && tr.average_exposure <= 1.0);
        assert!(track_record(&days(300), &r[..300], 252.0, 0.07).is_none());
    }

    #[test]
    fn worst_falls_are_separate_and_ordered() {
        let r = [0.0, -0.3, 0.4, 0.0, -0.1, 0.2, -0.2, 0.25];
        let falls = worst_falls(&r, 3);
        assert_eq!(
            falls[0],
            (0, 2),
            "the first of two equal peaks, down to the trough"
        );
        assert_eq!(falls.len(), 3);
        assert!(falls.iter().all(|(p, t)| p < t));
    }
}
