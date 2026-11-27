//! How the price reacted after a news story, beyond what the market explains
//! (see docs/METHODOLOGY.md, "News").

use serde::{Deserialize, Serialize};
use statrs::distribution::{ContinuousCDF, Normal};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Impact {
    /// Log return of the instrument from the last bar completed before the story to now.
    pub instrument_move: f64,
    /// Same window for the benchmark, when there is one.
    pub market_move: Option<f64>,
    /// instrument_move − β·market_move.
    pub abnormal_move: f64,
    /// Trading time since the story, in sessions.
    pub sessions_elapsed: f64,
    /// abnormal_move / (σ·√sessions_elapsed), σ the daily volatility not explained by the market.
    pub z: f64,
    /// A move at least this large (either way) happens about once in this many such windows
    /// without news: 1 / (2·(1 − Φ(|z|))).
    pub one_in: f64,
}

pub struct ImpactInput<'a> {
    /// (bar start, Unix s; close), oldest first.
    pub instrument: &'a [(i64, f64)],
    pub market: Option<&'a [(i64, f64)]>,
    pub beta: f64,
    /// Daily volatility of the part not explained by the market (total when no market).
    pub sigma_daily: f64,
    pub bar_seconds: i64,
    pub session_seconds: f64,
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// Close of the last bar that had finished by `t`, and the number of bars after it.
fn price_before(points: &[(i64, f64)], bar: i64, t: i64) -> Option<(f64, usize)> {
    let i = points.iter().rposition(|(start, _)| start + bar <= t)?;
    Some((points[i].1, points.len() - 1 - i))
}

pub fn impact(input: &ImpactInput, story_at: i64) -> Option<Impact> {
    let now = input.instrument.last()?.1;
    let (before, bars_after) = price_before(input.instrument, input.bar_seconds, story_at)?;
    if bars_after == 0 || !positive(input.sigma_daily) || !positive(input.session_seconds) {
        return None;
    }
    let instrument_move = (now / before).ln();
    let market_move = input.market.and_then(|m| {
        let (b, _) = price_before(m, input.bar_seconds, story_at)?;
        Some((m.last()?.1 / b).ln())
    });
    let abnormal_move = instrument_move - input.beta * market_move.unwrap_or(0.0);
    let sessions_elapsed = bars_after as f64 * input.bar_seconds as f64 / input.session_seconds;
    let z = abnormal_move / (input.sigma_daily * sessions_elapsed.sqrt());
    let tail = 2.0 * (1.0 - Normal::standard().cdf(z.abs()));
    Some(Impact {
        instrument_move,
        market_move,
        abnormal_move,
        sessions_elapsed,
        z,
        one_in: if tail > 0.0 {
            1.0 / tail
        } else {
            f64::INFINITY
        },
    })
}

/// One recorded headline and what the price did after it, for calibrating the news weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventOutcome {
    /// Event kind (company news) or asset (market news).
    pub group: String,
    pub direction: i8,
    pub materiality: u8,
    /// Log return after the story, beyond the market's (company news) or raw (market news).
    pub abnormal: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventGroup {
    pub group: String,
    pub direction: i8,
    pub n: usize,
    /// Mean abnormal log return and its standard error.
    pub mean: f64,
    pub se: f64,
    /// Share of events where the price went the way the reading said.
    pub hit_rate: f64,
    /// At least `MIN_EVENTS` events: enough to start replacing a judgement weight.
    pub enough: bool,
}

/// Events needed before a group's average move is used instead of a judgement weight.
pub const MIN_EVENTS: usize = 30;

/// The average move after each (group, direction), the event-study mean (MacKinlay 1997) with
/// its standard error. Events must already be one per (company, kind, day): many outlets
/// covering one event would otherwise count as many.
pub fn event_study(events: &[EventOutcome]) -> Vec<EventGroup> {
    let mut groups: std::collections::BTreeMap<(String, i8), Vec<f64>> = Default::default();
    for e in events
        .iter()
        .filter(|e| e.direction != 0 && e.abnormal.is_finite())
    {
        groups
            .entry((e.group.clone(), e.direction))
            .or_default()
            .push(e.abnormal);
    }
    groups
        .into_iter()
        .map(|((group, direction), x)| {
            let n = x.len();
            let mean = x.iter().sum::<f64>() / n as f64;
            let se = if n > 1 {
                (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64 / n as f64)
                    .sqrt()
            } else {
                f64::NAN
            };
            let hits = x.iter().filter(|v| v.signum() as i8 == direction).count();
            EventGroup {
                group,
                direction,
                n,
                mean,
                se,
                hit_rate: hits as f64 / n as f64,
                enough: n >= MIN_EVENTS,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_study_groups_by_kind_and_direction() {
        let ev = |group: &str, direction: i8, abnormal: f64| EventOutcome {
            group: group.into(),
            direction,
            materiality: 3,
            abnormal,
        };
        let out = event_study(&[
            ev("results", 1, 0.02),
            ev("results", 1, 0.04),
            ev("results", 1, -0.01),
            ev("results", -1, -0.03),
            ev("legal", -1, f64::NAN),
            ev("general", 0, 0.05),
        ]);
        assert_eq!(out.len(), 2, "{out:?}");
        let up = &out[1];
        assert_eq!((up.group.as_str(), up.direction, up.n), ("results", 1, 3));
        assert!((up.mean - 0.016_666_666).abs() < 1e-6);
        // sd of (0.02, 0.04, -0.01) = 0.025166; se = sd/√3.
        assert!((up.se - 0.025_166_114 / 3f64.sqrt()).abs() < 1e-6);
        assert!((up.hit_rate - 2.0 / 3.0).abs() < 1e-12);
        assert!(!up.enough && out[0].se.is_nan());
    }

    /// One 375-minute NSE session of 5-minute bars starting at t = 0.
    fn session(prices: impl Fn(usize) -> f64) -> Vec<(i64, f64)> {
        (0..75).map(|i| (i as i64 * 300, prices(i))).collect()
    }

    #[test]
    fn abnormal_move_removes_the_market_and_scales_by_time() {
        // The stock jumps 2% at bar 30; the market rises 0.5% at the same moment.
        let stock = session(|i| if i >= 30 { 102.0 } else { 100.0 });
        let market = session(|i| if i >= 30 { 1005.0 } else { 1000.0 });
        let input = ImpactInput {
            instrument: &stock,
            market: Some(&market),
            beta: 1.2,
            sigma_daily: 0.015,
            bar_seconds: 300,
            session_seconds: 22_500.0,
        };
        // Story seen at the start of bar 30 (t = 9000 s): the last completed bar is 29.
        let got = impact(&input, 9_000).unwrap();
        let expected_abnormal = (1.02f64).ln() - 1.2 * (1.005f64).ln();
        assert!((got.abnormal_move - expected_abnormal).abs() < 1e-12);
        // Bars 30..=74 after it: 45 × 5 min of a 375-min session = 0.6 sessions.
        assert!((got.sessions_elapsed - 0.6).abs() < 1e-12);
        let z = expected_abnormal / (0.015 * 0.6f64.sqrt());
        assert!((got.z - z).abs() < 1e-12);
        // Two-sided normal tail of z ≈ 1.15 is ≈ 0.25: about 1 window in 4.
        assert!((got.one_in - 1.0 / (2.0 * (1.0 - Normal::standard().cdf(z)))).abs() < 1e-9);
        assert!(got.one_in > 3.5 && got.one_in < 4.5, "{}", got.one_in);
    }

    #[test]
    fn stories_outside_the_window_or_too_recent_have_no_impact() {
        let stock = session(|_| 100.0);
        let input = ImpactInput {
            instrument: &stock,
            market: None,
            beta: 0.0,
            sigma_daily: 0.01,
            bar_seconds: 300,
            session_seconds: 22_500.0,
        };
        assert!(impact(&input, -10_000).is_none(), "before the first bar");
        assert!(
            impact(&input, 75 * 300).is_none(),
            "no bar after the story yet"
        );
        assert_eq!(impact(&input, 9_000).unwrap().z, 0.0);
    }
}
