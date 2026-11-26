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

#[cfg(test)]
mod tests {
    use super::*;

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
