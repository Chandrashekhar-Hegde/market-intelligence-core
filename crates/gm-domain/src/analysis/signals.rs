//! Buy/Hold/Sell, but only from rules that have worked on this instrument.
//!
//! A small, fixed library of rules from the literature (parameters are the published ones, never
//! tuned here) is tested walk-forward: the position at the close of day t uses data up to t only,
//! and is scored against the return over the next h sessions. A rule earns its place only if its
//! timing effect is significant after a Holm correction across every rule × horizon tested,
//! larger than a round-trip trading cost, present in both halves of the history, and seen over
//! enough independent calls. Advice is built only from rules that earned their place.
//! See docs/METHODOLOGY.md ("Signals and advice") for sources and every constant.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use statrs::distribution::{ContinuousCDF, StudentsT};

/// Holding periods tested, in sessions: 1, 2 and 3 days, a week, two weeks, a month, a quarter
/// (short horizons added 28 Nov 2026 for people who trade over days rather than months).
pub const HORIZONS: [usize; 7] = [1, 2, 3, 5, 10, 21, 63];
/// Round-trip cost of an Indian equity delivery trade with a zero-brokerage broker: STT 0.1% on
/// each side, stamp duty 0.015% on the buy, exchange and SEBI fees, GST, plus a spread allowance.
/// Rounded up to 0.25% (sources in METHODOLOGY.md). Used as the minimum edge worth acting on.
pub const ROUND_TRIP_COST: f64 = 0.0025;
/// Fewer independent calls than this and a hit rate means little (normal approximation to the
/// binomial needs n ≳ 30).
pub const MIN_INDEPENDENT: usize = 30;
/// "Unusual" is judged against the previous trading year.
pub const RANK_WINDOW: usize = 252;
/// Family-wise error rate for the Holm correction.
pub const FAMILY_ALPHA: f64 = 0.05;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalTest {
    pub horizon_days: usize,
    /// Days with a defined position and a known forward return.
    pub n_obs: usize,
    /// Days with a non-zero position, divided by the horizon (overlap removed).
    pub n_independent: usize,
    /// Slope of the h-day forward log return on the position (−1/0/+1): the timing effect.
    pub effect: f64,
    /// t-statistic of `effect` with Hansen–Hodrick standard errors (overlapping h-day returns).
    pub t_stat: f64,
    /// One-sided p-value (H1: effect > 0), Student t with n_independent − 1 df.
    pub p_value: f64,
    /// Holm-adjusted p-value across every test on this instrument.
    pub p_holm: f64,
    pub effect_first_half: f64,
    pub effect_second_half: f64,
    /// Share of non-overlapping calls where the next h-day move had the position's sign.
    pub hit_rate: Option<f64>,
    pub hit_n: usize,
    /// Wilson 95% interval for the hit rate.
    pub hit_low: Option<f64>,
    pub hit_high: Option<f64>,
    /// Average h-day forward return on days the rule said +1 / −1.
    pub mean_return_long: Option<f64>,
    pub mean_return_short: Option<f64>,
    pub earns_place: bool,
    /// Plain-language reason for the verdict.
    pub verdict: String,
    /// Standard error of `effect` (Hansen–Hodrick).
    #[serde(default)]
    pub se: f64,
    /// P(the timing edge beats a round trip's cost), from this instrument's estimate pulled
    /// toward the NIFTY 50 panel's (empirical Bayes). None without a panel prior; then the
    /// strict test (`earns_place`) decides.
    #[serde(default)]
    pub p_edge: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub name: String,
    pub rule: String,
    pub source: String,
    /// +1 buy, −1 sell/avoid, 0 no view, as of the last close.
    pub position_today: i8,
    /// What the rule sees today, in words.
    pub today: String,
    /// The close at which today's position would flip, when the rule has one.
    pub flip_level: Option<f64>,
    pub tests: Vec<SignalTest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    pub n_days: usize,
    pub round_trip_cost: f64,
    pub n_tests: usize,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Buy,
    Sell,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// A rule that earned its place is active today, and none points the other way.
    Proven,
    /// Proven rules point in opposite directions today.
    Conflict,
    /// Some rules earned their place but none is active today.
    ProvenButQuiet,
    /// No rule earned its place on this instrument.
    NoEvidence,
    /// No proven rule is active, and strong news today points one way. No track record.
    NewsDriven,
    /// A proven rule is active, but strong news today points the other way.
    NewsConflict,
}

/// Today's news as the advice needs it: the vote and the headlines behind it.
pub struct NewsInput<'a> {
    pub vote: &'a super::news_reader::NewsVote,
    /// Headlines of the vote's drivers, strongest first.
    pub headlines: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    pub hit_rate: f64,
    pub low: f64,
    pub high: f64,
    pub n: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    pub stance: Stance,
    pub basis: Basis,
    pub headline: String,
    /// Signal ids behind the stance, strongest first.
    pub drivers: Vec<String>,
    pub horizon_days: Option<usize>,
    /// Last close + horizon sessions: when to look again.
    pub review_date: Option<NaiveDate>,
    pub confidence: Option<Confidence>,
    pub flip_level: Option<f64>,
    /// One line per rule: what it sees today and whether it has worked here.
    pub reasons: Vec<String>,
    /// Headlines behind a news-driven or news-conflicted call, strongest first.
    pub news_headlines: Vec<String>,
}

/// Daily closes (split- and dividend-adjusted) and volumes, oldest first.
pub struct SignalInput<'a> {
    pub dates: &'a [NaiveDate],
    pub close: &'a [f64],
    /// None when the instrument has no meaningful volume (indices, FX).
    pub volume: Option<&'a [f64]>,
}

struct Rule {
    id: &'static str,
    name: &'static str,
    rule: &'static str,
    source: &'static str,
    positions: Vec<Option<i8>>,
    today: String,
    flip_level: Option<f64>,
}

/// None when there are too few days for any rule (needs a year plus the longest horizon).
pub fn evaluate(input: &SignalInput) -> Option<Signals> {
    let n = input.close.len();
    let longest = HORIZONS[HORIZONS.len() - 1];
    if n != input.dates.len() || n < RANK_WINDOW + longest + MIN_INDEPENDENT {
        return None;
    }
    let close = input.close;
    let mut rules = vec![
        trend(close),
        momentum(close),
        reversal(close),
        rsi_rule(close),
    ];
    if let Some(volume) = input.volume.filter(|v| v.len() == n) {
        rules.push(volume_spike(volume));
        rules.push(pressure(close, volume));
    }

    let mut signals: Vec<Signal> = rules
        .into_iter()
        .map(|r| Signal {
            id: r.id.into(),
            name: r.name.into(),
            rule: r.rule.into(),
            source: r.source.into(),
            position_today: r.positions[n - 1].unwrap_or(0),
            today: r.today,
            flip_level: r.flip_level,
            tests: HORIZONS
                .iter()
                .filter_map(|&h| backtest(&r.positions, close, h))
                .collect(),
        })
        .collect();

    // Holm (1979) step-down across every test on this instrument.
    let mut all: Vec<(usize, usize, f64)> = signals
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            s.tests
                .iter()
                .enumerate()
                .map(move |(j, t)| (i, j, t.p_value))
        })
        .collect();
    all.sort_by(|a, b| a.2.total_cmp(&b.2));
    let m = all.len();
    let mut running = 0.0_f64;
    for (rank, (i, j, p)) in all.iter().enumerate() {
        running = running.max(((m - rank) as f64 * p).min(1.0));
        let test = &mut signals[*i].tests[*j];
        test.p_holm = running;
        (test.earns_place, test.verdict) = verdict(test, m);
    }

    Some(Signals {
        first_date: input.dates[0],
        last_date: input.dates[n - 1],
        n_days: n,
        round_trip_cost: ROUND_TRIP_COST,
        n_tests: m,
        signals,
    })
}

fn verdict(t: &SignalTest, m: usize) -> (bool, String) {
    let horizon = horizon_words(t.horizon_days);
    if t.n_independent < MIN_INDEPENDENT {
        return (
            false,
            format!(
                "Not enough history to judge over {horizon}: {} separate calls, {MIN_INDEPENDENT} needed.",
                t.n_independent
            ),
        );
    }
    if t.p_holm >= FAMILY_ALPHA {
        return (
            false,
            format!(
                "No reliable edge over {horizon}: after checking {m} rule-and-horizon combinations, this could be luck (adjusted p = {:.2}).",
                t.p_holm
            ),
        );
    }
    if t.effect < ROUND_TRIP_COST {
        return (
            false,
            format!(
                "Real but too small over {horizon}: {:.2}% per call, less than the {:.2}% cost of trading.",
                t.effect * 100.0,
                ROUND_TRIP_COST * 100.0
            ),
        );
    }
    if t.effect_first_half <= 0.0 || t.effect_second_half <= 0.0 {
        return (
            false,
            format!(
                "Worked in only one half of the history over {horizon}, so it may have stopped working."
            ),
        );
    }
    let hit = t.hit_rate.map_or(String::new(), |h| {
        format!(
            " It pointed the right way {:.0}% of the time over {} calls.",
            h * 100.0,
            t.hit_n
        )
    });
    (
        true,
        format!(
            "Has worked on this instrument over {horizon} (adjusted p = {:.3}).{hit}",
            t.p_holm
        ),
    )
}

pub fn horizon_words(days: usize) -> &'static str {
    match days {
        0 | 1 => "a day",
        2 => "two days",
        3 | 4 => "three days",
        5..=7 => "about a week",
        8..=14 => "about two weeks",
        15..=30 => "about a month",
        _ => "about three months",
    }
}

// ---- the rules ------------------------------------------------------------------------------

fn trend(close: &[f64]) -> Rule {
    const W: usize = 200;
    let n = close.len();
    let mut positions = vec![None; n];
    let mut sum = 0.0;
    let mut sma = vec![None; n];
    for t in 0..n {
        sum += close[t];
        if t >= W {
            sum -= close[t - W];
        }
        if t + 1 >= W {
            let avg = sum / W as f64;
            sma[t] = Some(avg);
            positions[t] = Some(if close[t] > avg { 1 } else { -1 });
        }
    }
    let avg = sma[n - 1].unwrap_or(f64::NAN);
    let above = close[n - 1] > avg;
    Rule {
        id: "trend_200d",
        name: "Long-term trend",
        rule: "Buy while the price is above its 200-day average; stay out below it.",
        source: "Faber (2007); Moskowitz, Ooi & Pedersen (2012)",
        positions,
        today: format!(
            "The price ({:.2}) is {} its 200-day average ({avg:.2}).",
            close[n - 1],
            if above { "above" } else { "below" }
        ),
        flip_level: Some(avg),
    }
}

fn momentum(close: &[f64]) -> Rule {
    const LOOKBACK: usize = 252;
    const SKIP: usize = 21;
    let n = close.len();
    let ret = |t: usize| (close[t - SKIP] / close[t - LOOKBACK]).ln();
    let positions = (0..n)
        .map(|t| (t >= LOOKBACK).then(|| if ret(t) > 0.0 { 1 } else { -1 }))
        .collect();
    let r = ret(n - 1);
    Rule {
        id: "momentum_12_1",
        name: "12-month momentum",
        rule: "Buy if the past 12 months (skipping the latest month) gained; stay out if they lost.",
        source: "Jegadeesh & Titman (1993); Moskowitz, Ooi & Pedersen (2012)",
        positions,
        today: format!(
            "From 12 months ago to 1 month ago it {} {:.1}%.",
            if r > 0.0 { "rose" } else { "fell" },
            ((r.exp() - 1.0) * 100.0).abs()
        ),
        flip_level: None,
    }
}

/// Share of the previous `RANK_WINDOW` values strictly below `x[t]`.
fn trailing_rank(x: &[f64], t: usize) -> Option<f64> {
    (t >= RANK_WINDOW).then(|| {
        x[t - RANK_WINDOW..t].iter().filter(|v| **v < x[t]).count() as f64 / RANK_WINDOW as f64
    })
}

fn reversal(close: &[f64]) -> Rule {
    const W: usize = 5;
    let n = close.len();
    let r5: Vec<f64> = (0..n)
        .map(|t| {
            if t >= W {
                (close[t] / close[t - W]).ln()
            } else {
                0.0
            }
        })
        .collect();
    let positions: Vec<Option<i8>> = (0..n)
        .map(|t| {
            (t >= W + RANK_WINDOW)
                .then(|| trailing_rank(&r5, t))
                .flatten()
                .map(|rank| {
                    if rank < 0.1 {
                        1
                    } else if rank >= 0.9 {
                        -1
                    } else {
                        0
                    }
                })
        })
        .collect();
    let today = match positions[n - 1] {
        Some(1) => "The last week's fall is among the sharpest 10% of the past year.".to_string(),
        Some(-1) => "The last week's rise is among the sharpest 10% of the past year.".to_string(),
        _ => "The last week's move is ordinary for this instrument.".to_string(),
    };
    Rule {
        id: "reversal_5d",
        name: "Short-term bounce",
        rule: "Buy after a week's fall in the worst 10% of the past year; avoid after a rise in the best 10%.",
        source: "Lehmann (1990); Jegadeesh (1990)",
        positions,
        today,
        flip_level: None,
    }
}

/// Wilder's RSI, seeded with simple averages of the first `period` changes.
pub fn rsi_series(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if n <= period {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for t in 1..=period {
        let d = close[t] - close[t - 1];
        gain += d.max(0.0);
        loss += (-d).max(0.0);
    }
    gain /= period as f64;
    loss /= period as f64;
    let rsi = |g: f64, l: f64| {
        if l == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + g / l)
        }
    };
    out[period] = Some(rsi(gain, loss));
    for t in period + 1..n {
        let d = close[t] - close[t - 1];
        gain = (gain * (period - 1) as f64 + d.max(0.0)) / period as f64;
        loss = (loss * (period - 1) as f64 + (-d).max(0.0)) / period as f64;
        out[t] = Some(rsi(gain, loss));
    }
    out
}

fn rsi_rule(close: &[f64]) -> Rule {
    let n = close.len();
    let rsi = rsi_series(close, 14);
    let positions = rsi
        .iter()
        .map(|v| {
            v.map(|r| {
                if r < 30.0 {
                    1
                } else if r > 70.0 {
                    -1
                } else {
                    0
                }
            })
        })
        .collect();
    let now = rsi[n - 1].unwrap_or(f64::NAN);
    Rule {
        id: "rsi_14",
        name: "Overbought / oversold (RSI)",
        rule: "Buy when the 14-day RSI is below 30 (oversold); avoid above 70 (overbought).",
        source: "Wilder (1978)",
        positions,
        today: format!(
            "RSI is {now:.0}: {}.",
            if now < 30.0 {
                "oversold"
            } else if now > 70.0 {
                "overbought"
            } else {
                "neither overbought nor oversold"
            }
        ),
        flip_level: None,
    }
}

fn volume_spike(volume: &[f64]) -> Rule {
    let n = volume.len();
    let positions: Vec<Option<i8>> = (0..n)
        .map(|t| trailing_rank(volume, t).map(|rank| i8::from(rank >= 0.9)))
        .collect();
    let avg20 = volume[n.saturating_sub(21)..n - 1].iter().sum::<f64>() / 20.0;
    Rule {
        id: "volume_spike",
        name: "Unusual trading volume",
        rule: "Buy after a day whose volume is in the top 10% of the past year.",
        source: "Gervais, Kaniel & Mingelgrin (2001), the high-volume return premium",
        positions,
        today: format!(
            "The latest session traded {:.1}× its 20-day average volume{}.",
            volume[n - 1] / avg20.max(1.0),
            if n >= 1 && matches!(trailing_rank(volume, n - 1), Some(r) if r >= 0.9) {
                ", in the top 10% of the past year"
            } else {
                ""
            }
        ),
        flip_level: None,
    }
}

/// On-balance volume (Granville 1963): running total of volume signed by the day's direction.
pub fn obv_series(close: &[f64], volume: &[f64]) -> Vec<f64> {
    let mut obv = vec![0.0; close.len()];
    for t in 1..close.len() {
        // f64::signum(0.0) is 1.0, so flat days are matched explicitly: they add nothing.
        let sign = match close[t].partial_cmp(&close[t - 1]) {
            Some(std::cmp::Ordering::Greater) => 1.0,
            Some(std::cmp::Ordering::Less) => -1.0,
            _ => 0.0,
        };
        obv[t] = obv[t - 1] + sign * volume[t];
    }
    obv
}

fn pressure(close: &[f64], volume: &[f64]) -> Rule {
    const W: usize = 20;
    let n = close.len();
    let obv = obv_series(close, volume);
    let positions = (0..n)
        .map(|t| (t >= W).then(|| if obv[t] > obv[t - W] { 1 } else { -1 }))
        .collect();
    let buying = obv[n - 1] > obv[n - 1 - W];
    Rule {
        id: "pressure_obv",
        name: "Buying vs selling pressure",
        rule: "Buy while on-balance volume is higher than 20 sessions ago (more volume on up days); stay out while it is lower.",
        source: "Granville (1963)",
        positions,
        today: format!(
            "Over the last 20 sessions more volume traded on {} days.",
            if buying { "up" } else { "down" }
        ),
        flip_level: None,
    }
}

// ---- the backtest ---------------------------------------------------------------------------

fn slope(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
    (sxx > 0.0).then(|| {
        x.iter()
            .zip(y)
            .map(|(a, b)| (a - mx) * (b - my))
            .sum::<f64>()
            / sxx
    })
}

/// Wilson (1927) score interval at 95%.
pub fn wilson(hits: usize, n: usize) -> (f64, f64) {
    let z = 1.959_963_984_540_054;
    let (n, p) = (n as f64, hits as f64 / n as f64);
    let centre = (p + z * z / (2.0 * n)) / (1.0 + z * z / n);
    let half = z / (1.0 + z * z / n) * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    (centre - half, centre + half)
}

pub fn backtest(positions: &[Option<i8>], close: &[f64], h: usize) -> Option<SignalTest> {
    let days: Vec<usize> = (0..close.len().saturating_sub(h))
        .filter(|&t| positions[t].is_some())
        .collect();
    let x: Vec<f64> = days
        .iter()
        .map(|&t| f64::from(positions[t].unwrap_or(0)))
        .collect();
    let y: Vec<f64> = days
        .iter()
        .map(|&t| (close[t + h] / close[t]).ln())
        .collect();
    let n = x.len();
    let active = x.iter().filter(|v| **v != 0.0).count();
    if n < 2 * h + 10 || active == 0 {
        return None;
    }
    let effect = slope(&x, &y)?;
    let mx = x.iter().sum::<f64>() / n as f64;
    let my = y.iter().sum::<f64>() / n as f64;
    let intercept = my - effect * mx;
    let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
    // Overlapping h-day returns are MA(h − 1), so the slope's long-run variance uses equal
    // weights on lags 1..h − 1 (Hansen & Hodrick 1980). Bartlett weights (Newey–West) understate
    // it here: on simulated random walks they gave 6.5% false "proven" rules instead of ≤ 5%.
    // Newey–West is the fallback when the uniform sum is not positive.
    let u: Vec<f64> = x
        .iter()
        .zip(&y)
        .map(|(a, b)| (a - mx) * (b - intercept - effect * a))
        .collect();
    let lags = h - 1;
    let autocov: Vec<f64> = (1..=lags)
        .map(|l| (l..n).map(|t| u[t] * u[t - l]).sum::<f64>())
        .collect();
    let s0: f64 = u.iter().map(|v| v * v).sum();
    let uniform = s0 + 2.0 * autocov.iter().sum::<f64>();
    let s = if uniform > 0.0 {
        uniform
    } else {
        s0 + 2.0
            * autocov
                .iter()
                .enumerate()
                .map(|(i, c)| (1.0 - (i + 1) as f64 / (lags + 1) as f64) * c)
                .sum::<f64>()
    };
    let se = s.max(0.0).sqrt() / sxx;
    let t_stat = if se > 0.0 { effect / se } else { 0.0 };
    let n_independent = active / h;
    let df = (n_independent.max(2) - 1) as f64;
    let p_value = StudentsT::new(0.0, 1.0, df).map_or(1.0, |d| 1.0 - d.cdf(t_stat));

    let half = n / 2;
    let effect_first_half = slope(&x[..half], &y[..half]).unwrap_or(0.0);
    let effect_second_half = slope(&x[half..], &y[half..]).unwrap_or(0.0);

    // Hit rate on non-overlapping calls only: every h-th day that had a position.
    let (mut hits, mut calls) = (0usize, 0usize);
    let mut next = 0usize;
    for (k, &t) in days.iter().enumerate() {
        if t < next || x[k] == 0.0 || y[k] == 0.0 {
            continue;
        }
        calls += 1;
        hits += usize::from((x[k] > 0.0) == (y[k] > 0.0));
        next = t + h;
    }
    let (hit_low, hit_high) = if calls > 0 {
        wilson(hits, calls)
    } else {
        (f64::NAN, f64::NAN)
    };
    let mean_when = |side: f64| {
        let v: Vec<f64> = x
            .iter()
            .zip(&y)
            .filter(|(a, _)| **a == side)
            .map(|(_, b)| *b)
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    Some(SignalTest {
        horizon_days: h,
        n_obs: n,
        n_independent,
        effect,
        t_stat,
        p_value,
        p_holm: p_value,
        effect_first_half,
        effect_second_half,
        hit_rate: (calls > 0).then(|| hits as f64 / calls as f64),
        hit_n: calls,
        hit_low: (calls > 0).then_some(hit_low),
        hit_high: (calls > 0).then_some(hit_high),
        mean_return_long: mean_when(1.0),
        mean_return_short: mean_when(-1.0),
        earns_place: false,
        verdict: String::new(),
        se,
        p_edge: None,
    })
}

// ---- graded rules: empirical Bayes across the NIFTY 50 -----------------------------------------

/// A rule-and-horizon's effect across the panel: random-effects mean and the spread of true
/// effects between instruments (DerSimonian & Laird 1986).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RulePrior {
    pub mean: f64,
    /// Variance of the mean itself.
    pub mean_var: f64,
    /// Between-instrument variance of true effects.
    pub tau2: f64,
    pub k: usize,
}

/// Owner decision (27 Nov 2026): a rule calls only when P(edge beats costs) ≥ 80 in 100.
pub const CALL_PROB: f64 = 0.80;

/// Average pairwise correlation of equally long, date-aligned return series, from the variance
/// of their equal-weighted mean: var(mean) = σ̄²(1 + (k − 1)ρ̄)/k (equal variances assumed).
pub fn average_correlation(series: &[Vec<f64>]) -> f64 {
    let k = series.len();
    let n = series.iter().map(Vec::len).min().unwrap_or(0);
    if k < 2 || n < 30 {
        return 0.0;
    }
    let var = |x: &[f64]| {
        let m = x.iter().sum::<f64>() / x.len() as f64;
        x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (x.len() - 1) as f64
    };
    let mean_var = series.iter().map(|s| var(&s[..n])).sum::<f64>() / k as f64;
    let avg: Vec<f64> = (0..n)
        .map(|t| series.iter().map(|s| s[t]).sum::<f64>() / k as f64)
        .collect();
    let rho = (k as f64 * var(&avg) / mean_var - 1.0) / (k as f64 - 1.0);
    rho.clamp(0.0, 1.0)
}

/// Priors per (rule id, horizon) from many instruments' tests. `rho` is the average correlation
/// between the instruments' returns: the tests share market days, so their estimates move
/// together, and the panel mean is only as certain as k / (1 + (k − 1)ρ) independent ones
/// (Kish's design effect). Ignoring it made one rule look certain on a 0.4% ± 1.5% estimate.
pub fn panel_priors(panel: &[&Signals], rho: f64) -> Vec<((String, usize), RulePrior)> {
    let mut by_key: std::collections::BTreeMap<(String, usize), Vec<(f64, f64)>> =
        Default::default();
    for signals in panel {
        for s in &signals.signals {
            for t in &s.tests {
                if t.se > 0.0 && t.effect.is_finite() {
                    by_key
                        .entry((s.id.clone(), t.horizon_days))
                        .or_default()
                        .push((t.effect, t.se));
                }
            }
        }
    }
    by_key
        .into_iter()
        .filter(|(_, v)| v.len() >= 5)
        .map(|(key, v)| {
            let w: Vec<f64> = v.iter().map(|(_, se)| 1.0 / (se * se)).collect();
            let sw: f64 = w.iter().sum();
            let fixed = v.iter().zip(&w).map(|((y, _), w)| w * y).sum::<f64>() / sw;
            let q: f64 = v
                .iter()
                .zip(&w)
                .map(|((y, _), w)| w * (y - fixed).powi(2))
                .sum();
            let c = sw - w.iter().map(|w| w * w).sum::<f64>() / sw;
            let tau2 = ((q - (v.len() as f64 - 1.0)) / c).max(0.0);
            let w_re: Vec<f64> = v.iter().map(|(_, se)| 1.0 / (se * se + tau2)).collect();
            let s_re: f64 = w_re.iter().sum();
            let mean = v.iter().zip(&w_re).map(|((y, _), w)| w * y).sum::<f64>() / s_re;
            (
                key,
                RulePrior {
                    mean,
                    mean_var: (1.0 + (v.len() as f64 - 1.0) * rho.clamp(0.0, 1.0)) / s_re,
                    tau2,
                    k: v.len(),
                },
            )
        })
        .collect()
}

/// P(true effect > cost) after pulling `effect` (±`se`) toward the prior.
pub fn edge_probability(effect: f64, se: f64, prior: &RulePrior, cost: f64) -> f64 {
    use statrs::distribution::Normal;
    let v0 = prior.tau2 + prior.mean_var;
    let (m, v) = if se > 0.0 && v0 > 0.0 {
        let prec = 1.0 / (se * se) + 1.0 / v0;
        ((effect / (se * se) + prior.mean / v0) / prec, 1.0 / prec)
    } else {
        (prior.mean, v0)
    };
    if v <= 0.0 {
        return if m > cost { 1.0 } else { 0.0 };
    }
    1.0 - Normal::new(m, v.sqrt()).map_or(0.5, |n| n.cdf(cost))
}

/// Grade every test against its panel prior. A test then counts when P(edge beats costs) ≥
/// `CALL_PROB` with enough separate calls; the hierarchical model handles the many
/// rule-and-horizon combinations (Gelman, Hill & Yajima 2012), so no Holm correction is needed.
pub fn apply_priors(signals: &mut Signals, priors: &[((String, usize), RulePrior)]) {
    for s in &mut signals.signals {
        for t in &mut s.tests {
            let Some((_, prior)) = priors
                .iter()
                .find(|((id, h), _)| *id == s.id && *h == t.horizon_days)
            else {
                continue;
            };
            let p = edge_probability(t.effect, t.se, prior, signals.round_trip_cost);
            t.p_edge = Some(p);
            let enough = t.n_independent >= MIN_INDEPENDENT;
            t.earns_place = enough && p >= CALL_PROB;
            t.verdict = format!(
                "{} in 100 chance that following it beats trading costs over {}, judged with how it did on all NIFTY 50 stocks{}.",
                (p * 100.0).round(),
                horizon_words(t.horizon_days),
                if !enough {
                    format!(" (too few separate calls: {})", t.n_independent)
                } else if p >= CALL_PROB {
                    String::new()
                } else {
                    format!("; a call needs {}", (CALL_PROB * 100.0).round())
                }
            );
        }
    }
}

// ---- advice ---------------------------------------------------------------------------------

/// The date `sessions` trading sessions after `from`: weekdays, or every day when the history
/// itself has weekend dates (crypto). Holidays are ignored; it is a reminder, not a calendar.
pub fn add_sessions(from: NaiveDate, sessions: usize, every_day: bool) -> NaiveDate {
    let mut d = from;
    let mut left = sessions;
    while left > 0 {
        d += Duration::days(1);
        if every_day || !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            left -= 1;
        }
    }
    d
}

pub fn advise(signals: &Signals, every_day: bool) -> Advice {
    // Each rule's best horizon among those where it earned its place.
    let proven: Vec<(&Signal, &SignalTest)> = signals
        .signals
        .iter()
        .filter_map(|s| {
            s.tests
                .iter()
                .filter(|t| t.earns_place)
                .min_by(|a, b| a.p_holm.total_cmp(&b.p_holm))
                .map(|t| (s, t))
        })
        .collect();
    let mut active: Vec<&(&Signal, &SignalTest)> = proven
        .iter()
        .filter(|(s, _)| s.position_today != 0)
        .collect();
    active.sort_by(|a, b| a.1.p_holm.total_cmp(&b.1.p_holm));
    let longs = active.iter().any(|(s, _)| s.position_today > 0);
    let shorts = active.iter().any(|(s, _)| s.position_today < 0);

    let reasons = signals
        .signals
        .iter()
        .map(|s| {
            let worked = proven.iter().find(|(p, _)| p.id == s.id);
            let verdict = match worked {
                Some((_, t)) => t.verdict.clone(),
                None => s
                    .tests
                    .iter()
                    .min_by(|a, b| a.p_holm.total_cmp(&b.p_holm))
                    .map_or("Could not be tested on this history.".into(), |t| {
                        format!("Not proven here. {}", t.verdict)
                    }),
            };
            format!("{}: {} {}", s.name, s.today, verdict)
        })
        .collect();

    let (stance, basis) = match (longs, shorts, proven.is_empty()) {
        (true, false, _) => (Stance::Buy, Basis::Proven),
        (false, true, _) => (Stance::Sell, Basis::Proven),
        (true, true, _) => (Stance::Hold, Basis::Conflict),
        (false, false, false) => (Stance::Hold, Basis::ProvenButQuiet),
        (false, false, true) => (Stance::Hold, Basis::NoEvidence),
    };
    let lead = (basis == Basis::Proven).then(|| active[0]);
    let headline = match (basis, lead) {
        (Basis::Proven, Some((s, t))) => format!(
            "{} for {}. {} has worked on this instrument{}.",
            if stance == Stance::Buy { "Buy" } else { "Sell or stay out" },
            horizon_words(t.horizon_days),
            s.name,
            t.hit_rate
                .map_or(String::new(), |h| format!(": right {:.0}% of the time over {} past calls", h * 100.0, t.hit_n))
        ),
        (Basis::Conflict, _) => "Hold. Rules that have worked on this instrument disagree today.".into(),
        (Basis::ProvenButQuiet, _) => {
            "Hold. The rules that have worked on this instrument give no signal today.".into()
        }
        _ => "Hold. No rule has a proven edge on this instrument: its past moves have not predicted its next ones well enough to beat trading costs.".into(),
    };
    Advice {
        stance,
        basis,
        headline,
        drivers: active
            .iter()
            .filter(|(s, _)| {
                basis == Basis::Proven && (s.position_today > 0) == (stance == Stance::Buy)
            })
            .map(|(s, _)| s.id.clone())
            .collect(),
        horizon_days: lead.map(|(_, t)| t.horizon_days),
        review_date: lead.map(|(_, t)| add_sessions(signals.last_date, t.horizon_days, every_day)),
        confidence: lead.and_then(|(_, t)| {
            Some(Confidence {
                hit_rate: t.hit_rate?,
                low: t.hit_low?,
                high: t.hit_high?,
                n: t.hit_n,
            })
        }),
        flip_level: lead.and_then(|(s, _)| s.flip_level),
        reasons,
        news_headlines: Vec::new(),
    }
}

/// The rules' advice, adjusted for strong news (owner decision, 27 Nov 2026):
/// - a proven call that news contradicts becomes Hold;
/// - with no proven call active, strong news makes the call, labelled as having no track record.
pub fn advise_with_news(signals: &Signals, every_day: bool, news: Option<&NewsInput>) -> Advice {
    let mut advice = advise(signals, every_day);
    let Some(news) = news.filter(|n| n.vote.stance != 0) else {
        return advice;
    };
    let news_stance = if news.vote.stance > 0 {
        Stance::Buy
    } else {
        Stance::Sell
    };
    let top = news
        .headlines
        .first()
        .map_or(String::new(), |h| format!(" Leading story: “{h}”."));
    advice.news_headlines = news.headlines.to_vec();
    match advice.basis {
        Basis::Proven if advice.stance == news_stance => {
            advice
                .headline
                .push_str(" Today's news points the same way.");
        }
        Basis::Proven => {
            advice.headline = format!(
                "Hold. A rule that has worked on this stock says {}, but today's news points the other way.{top}",
                if advice.stance == Stance::Buy {
                    "buy"
                } else {
                    "sell"
                }
            );
            advice.stance = Stance::Hold;
            advice.basis = Basis::NewsConflict;
            advice.confidence = None;
            advice.horizon_days = None;
            advice.review_date = None;
            advice.flip_level = None;
        }
        Basis::Conflict => {}
        Basis::ProvenButQuiet | Basis::NoEvidence | Basis::NewsDriven | Basis::NewsConflict => {
            advice.stance = news_stance;
            advice.basis = Basis::NewsDriven;
            advice.headline = format!(
                "{}, based on today's news. No track record yet: news-driven calls haven't been tested on past data.{top}",
                if news_stance == Stance::Buy {
                    "Buy"
                } else {
                    "Sell or stay out"
                }
            );
            // News is read as a few-days effect; look again within a week.
            advice.horizon_days = Some(5);
            advice.review_date = Some(add_sessions(signals.last_date, 5, every_day));
            advice.confidence = None;
            advice.flip_level = None;
            advice.drivers = Vec::new();
        }
    }
    advice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graded(effect: f64, se: f64) -> Signals {
        let t = SignalTest {
            horizon_days: 21,
            n_obs: 2000,
            n_independent: 60,
            effect,
            t_stat: effect / se,
            p_value: 0.01,
            p_holm: 0.01,
            effect_first_half: effect,
            effect_second_half: effect,
            hit_rate: Some(0.55),
            hit_n: 60,
            hit_low: Some(0.45),
            hit_high: Some(0.65),
            mean_return_long: None,
            mean_return_short: None,
            earns_place: false,
            verdict: String::new(),
            se,
            p_edge: None,
        };
        Signals {
            first_date: NaiveDate::from_ymd_opt(2016, 11, 25).unwrap(),
            last_date: NaiveDate::from_ymd_opt(2026, 11, 25).unwrap(),
            n_days: 2500,
            round_trip_cost: ROUND_TRIP_COST,
            n_tests: 1,
            signals: vec![Signal {
                id: "trend_200d".into(),
                name: "Long-term trend".into(),
                rule: String::new(),
                source: String::new(),
                position_today: 1,
                today: String::new(),
                flip_level: None,
                tests: vec![t],
            }],
        }
    }

    /// The panel says the rule does nothing anywhere: one stock's lucky t = 2.5 is pulled back
    /// to nothing. Where stocks really differ (large τ²), its own strong estimate stands.
    #[test]
    fn panel_shrinkage_tempers_a_lucky_stock_and_grades_the_call() {
        let flat: Vec<Signals> = (0..30)
            .map(|i| graded(if i % 2 == 0 { 0.0005 } else { -0.0005 }, 0.004))
            .collect();
        let priors = panel_priors(&flat.iter().collect::<Vec<_>>(), 0.0);
        let prior = priors[0].1;
        assert_eq!(priors[0].0, ("trend_200d".to_string(), 21));
        assert!(prior.mean.abs() < 0.001 && prior.tau2 < 1e-9, "{prior:?}");
        let lucky = edge_probability(0.01, 0.004, &prior, ROUND_TRIP_COST);
        assert!(lucky < 0.05, "{lucky}");
        let varied = RulePrior {
            mean: 0.0,
            mean_var: 1e-6,
            tau2: 0.01 * 0.01,
            k: 30,
        };
        let own = edge_probability(0.01, 0.004, &varied, ROUND_TRIP_COST);
        assert!(own > 0.9, "{own}");
        // Grading decides the call: 80 in 100 or more counts.
        let mut s = graded(0.01, 0.004);
        apply_priors(&mut s, &[(("trend_200d".into(), 21), varied)]);
        assert!(s.signals[0].tests[0].earns_place);
        assert!(
            s.signals[0].tests[0]
                .verdict
                .starts_with(&format!("{} in 100", (own * 100.0).round()))
        );
        let mut s = graded(0.01, 0.004);
        apply_priors(&mut s, &priors);
        assert!(!s.signals[0].tests[0].earns_place);
        assert!(s.signals[0].tests[0].verdict.contains("a call needs 80"));
    }

    /// Correlated estimates: a panel mean from 50 stocks that move together is far less certain
    /// than from 50 independent ones, so a weak own estimate doesn't become a sure call.
    #[test]
    fn correlated_panels_count_as_fewer_stocks() {
        let panel: Vec<Signals> = (0..50).map(|_| graded(0.01, 0.014)).collect();
        let refs: Vec<&Signals> = panel.iter().collect();
        let independent = panel_priors(&refs, 0.0)[0].1;
        let correlated = panel_priors(&refs, 0.3)[0].1;
        assert!((correlated.mean_var / independent.mean_var - (1.0 + 49.0 * 0.3)).abs() < 1e-9);
        let p_ind = edge_probability(0.004, 0.0147, &independent, ROUND_TRIP_COST);
        let p_cor = edge_probability(0.004, 0.0147, &correlated, ROUND_TRIP_COST);
        assert!(p_ind > 0.99 && p_cor < p_ind - 0.05, "{p_ind} vs {p_cor}");
        // A common factor plus independent noise of equal size: ρ ≈ 0.5.
        let common: Vec<f64> = (0..500)
            .map(|t| ((t * 7919) % 1000) as f64 / 1000.0 - 0.5)
            .collect();
        let series: Vec<Vec<f64>> = (0..10)
            .map(|i| {
                common
                    .iter()
                    .enumerate()
                    .map(|(t, c)| {
                        c + ((t * (i + 3) * 104_729 + i * 31) % 1000) as f64 / 1000.0 - 0.5
                    })
                    .collect()
            })
            .collect();
        let rho = average_correlation(&series);
        assert!((rho - 0.5).abs() < 0.15, "{rho}");
    }
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;
    use rand_distr::{Distribution, Normal, StudentT};

    fn dates(n: usize) -> Vec<NaiveDate> {
        let mut d = NaiveDate::from_ymd_opt(2015, 1, 5).unwrap();
        (0..n)
            .map(|_| {
                let out = d;
                d = add_sessions(d, 1, false);
                out
            })
            .collect()
    }

    fn walk(seed: u64, n: usize) -> Vec<f64> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let shock = StudentT::<f64>::new(4.0).unwrap();
        let mut p = 100.0_f64;
        (0..n)
            .map(|_| {
                // Fat-tailed shocks scaled to ~1.4% daily volatility, small positive drift.
                p *= (0.0003 + 0.01 * shock.sample(&mut rng)).exp();
                p
            })
            .collect()
    }

    #[test]
    fn wilson_matches_closed_form() {
        // statsmodels proportion_confint(30, 50, alpha=0.05, method="wilson")
        let (lo, hi) = wilson(30, 50);
        assert!((lo - 0.461_814_377_475_893_6).abs() < 1e-12, "{lo}");
        assert!((hi - 0.723_916_102_697_434_6).abs() < 1e-12, "{hi}");
    }

    #[test]
    fn sessions_skip_weekends_unless_every_day() {
        let fri = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap();
        assert_eq!(
            add_sessions(fri, 1, false),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()
        );
        assert_eq!(
            add_sessions(fri, 5, false),
            NaiveDate::from_ymd_opt(2026, 12, 4).unwrap()
        );
        assert_eq!(
            add_sessions(fri, 1, true),
            NaiveDate::from_ymd_opt(2026, 11, 28).unwrap()
        );
    }

    #[test]
    fn too_short_history_gives_nothing() {
        let close = walk(1, 300);
        let d = dates(300);
        assert!(
            evaluate(&SignalInput {
                dates: &d,
                close: &close,
                volume: None
            })
            .is_none()
        );
    }

    /// A planted trend: drift switches sign in long regimes. The trend rule must find it.
    #[test]
    fn planted_trend_earns_its_place_and_gives_a_call() {
        let n = 2520;
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let noise = Normal::<f64>::new(0.0, 0.01).unwrap();
        let mut p = 100.0_f64;
        let close: Vec<f64> = (0..n)
            .map(|t| {
                // ±0.4% a day for a year at a time against 1% daily noise: a trend worth trading.
                let drift = if (t / 250) % 2 == 0 { 0.004 } else { -0.004 };
                p *= (drift + noise.sample(&mut rng)).exp();
                p
            })
            .collect();
        let d = dates(n);
        let s = evaluate(&SignalInput {
            dates: &d,
            close: &close,
            volume: None,
        })
        .unwrap();
        let trend = s.signals.iter().find(|x| x.id == "trend_200d").unwrap();
        assert!(
            trend.tests.iter().any(|t| t.earns_place),
            "{:?}",
            trend.tests.iter().map(|t| &t.verdict).collect::<Vec<_>>()
        );
        let advice = advise(&s, false);
        assert_ne!(advice.basis, Basis::NoEvidence, "{advice:#?}");
        if advice.basis == Basis::Proven {
            assert!(advice.confidence.is_some() && advice.review_date.is_some());
        }
    }

    /// Random walks have no edge: the engine must almost always say Hold / no evidence.
    #[test]
    fn random_walks_rarely_get_a_call() {
        let n = 2520;
        let d = dates(n);
        let runs = 200;
        let mut calls = 0;
        for seed in 0..runs {
            let close = walk(1000 + seed, n);
            let volume: Vec<f64> = {
                let mut rng = ChaCha8Rng::seed_from_u64(5000 + seed);
                let v = Normal::<f64>::new(0.0, 0.3).unwrap();
                (0..n).map(|_| 1e6 * v.sample(&mut rng).exp()).collect()
            };
            let s = evaluate(&SignalInput {
                dates: &d,
                close: &close,
                volume: Some(&volume),
            })
            .unwrap();
            if s.signals
                .iter()
                .any(|x| x.tests.iter().any(|t| t.earns_place))
            {
                calls += 1;
            }
        }
        // Holm controls the family-wise error at 5% per instrument.
        assert!(
            calls as f64 / runs as f64 <= 0.05,
            "{calls} of {runs} random walks got a proven rule"
        );
    }

    #[test]
    fn deterministic() {
        let close = walk(3, 1500);
        let d = dates(1500);
        let a = serde_json::to_string(&evaluate(&SignalInput {
            dates: &d,
            close: &close,
            volume: None,
        }))
        .unwrap();
        let b = serde_json::to_string(&evaluate(&SignalInput {
            dates: &d,
            close: &close,
            volume: None,
        }))
        .unwrap();
        assert_eq!(a, b);
    }

    mod news {
        use super::super::*;
        use crate::analysis::news_reader::NewsVote;

        fn signals(position: i8, proven: bool) -> Signals {
            let test = SignalTest {
                horizon_days: 21,
                n_obs: 2000,
                n_independent: 90,
                effect: 0.01,
                t_stat: 4.0,
                p_value: 0.0001,
                p_holm: 0.001,
                effect_first_half: 0.01,
                effect_second_half: 0.01,
                hit_rate: Some(0.6),
                hit_n: 90,
                hit_low: Some(0.5),
                hit_high: Some(0.7),
                mean_return_long: Some(0.01),
                mean_return_short: None,
                earns_place: proven,
                verdict: String::new(),
                se: 0.002,
                p_edge: None,
            };
            Signals {
                first_date: NaiveDate::from_ymd_opt(2016, 11, 25).unwrap(),
                last_date: NaiveDate::from_ymd_opt(2026, 11, 27).unwrap(),
                n_days: 2470,
                round_trip_cost: ROUND_TRIP_COST,
                n_tests: 3,
                signals: vec![Signal {
                    id: "trend_200d".into(),
                    name: "Long-term trend".into(),
                    rule: String::new(),
                    source: String::new(),
                    position_today: position,
                    today: String::new(),
                    flip_level: Some(100.0),
                    tests: vec![test],
                }],
            }
        }

        fn vote(stance: i8) -> NewsVote {
            NewsVote {
                score: f64::from(stance),
                stance,
                n_stories: 3,
                drivers: vec![0],
            }
        }

        #[test]
        fn news_drives_a_call_only_without_a_proven_rule() {
            let v = vote(-1);
            let headlines = ["SEBI orders forensic audit".to_string()];
            let input = NewsInput {
                vote: &v,
                headlines: &headlines,
            };
            // Nothing proven: the news makes the call, labelled, with a short review.
            let a = advise_with_news(&signals(1, false), false, Some(&input));
            assert_eq!((a.stance, a.basis), (Stance::Sell, Basis::NewsDriven));
            assert!(
                a.headline.contains("No track record yet") && a.headline.contains("forensic audit"),
                "{}",
                a.headline
            );
            assert_eq!(a.horizon_days, Some(5));
            assert!(a.confidence.is_none());
            // A proven Buy contradicted by strong bad news: Hold.
            let b = advise_with_news(&signals(1, true), false, Some(&input));
            assert_eq!((b.stance, b.basis), (Stance::Hold, Basis::NewsConflict));
            // A proven Buy with good news: unchanged, and says so.
            let good = vote(1);
            let agree = advise_with_news(
                &signals(1, true),
                false,
                Some(&NewsInput {
                    vote: &good,
                    headlines: &headlines,
                }),
            );
            assert_eq!((agree.stance, agree.basis), (Stance::Buy, Basis::Proven));
            assert!(
                agree
                    .headline
                    .ends_with("Today's news points the same way.")
            );
            // Weak news changes nothing.
            let quiet = vote(0);
            let same = advise_with_news(
                &signals(1, false),
                false,
                Some(&NewsInput {
                    vote: &quiet,
                    headlines: &[],
                }),
            );
            assert_eq!(same, advise(&signals(1, false), false));
        }
    }
}
