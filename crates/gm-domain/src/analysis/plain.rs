//! The analysis in plain words: one action per instrument (never a bare "Hold"), three short
//! answers (buying for the long term, already owning it, trading short term), and the price
//! range with how often that range has held. Every sentence carries the number it rests on.
//! Pure: the web app and the desktop app both show exactly these words.

use serde::{Deserialize, Serialize};

use super::{
    AnalysisReport,
    long_run::LongRun,
    signals::{Advice, Basis, Stance},
};

/// How the instrument is held, which changes what "buying it" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One company: its own long-run growth can't be told apart from its market's.
    SingleStock,
    /// A price, not an investment (exchange rates).
    Currency,
    /// Indices, funds, ETFs, commodities, crypto.
    Other,
}

/// What the headline action is, so a UI can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Buy,
    Sell,
    Trim,
    Keep,
    Sip,
    IndexInstead,
    Fd,
    Watch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question: String,
    pub verdict: String,
    pub why: String,
}

/// Where the price could be after `horizon_days`, as simple returns, and how often the outer
/// range held when tested on past dates without looking ahead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeRow {
    pub horizon_days: usize,
    pub label: String,
    /// 5th, 25th, 50th, 75th and 95th percentiles.
    pub returns: [f64; 5],
    pub held: f64,
    pub n_tested: usize,
    pub n_stretches: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plain {
    pub action: Action,
    /// The one line to read: what to do.
    pub headline: String,
    pub answers: Vec<Answer>,
    pub ranges: Vec<RangeRow>,
    /// Can the direction be told? With the numbers.
    pub direction: Option<String>,
    /// How the range is worked out, in one sentence.
    pub how: Option<String>,
    /// Has the holder rule paid off here?
    pub track_record: Option<String>,
}

/// A chance as natural frequency ("62 in 100"; Gigerenzer & Hoffrage 1995).
pub fn in_100(p: f64) -> String {
    format!("{} in 100", (p * 100.0).round() as i64)
}

fn signed_pct(x: f64) -> String {
    format!(
        "{}{:.0}%",
        if x >= 0.0 { "+" } else { "−" },
        (x * 100.0).abs()
    )
}

fn horizon_label(days: usize) -> String {
    match days {
        1 => "1 day".into(),
        5 => "1 week".into(),
        10 => "2 weeks".into(),
        21 => "1 month".into(),
        63 => "3 months".into(),
        d => format!("{d} trading days"),
    }
}

/// "Buy monthly (SIP)" → "buy monthly (SIP)": only the first letter changes.
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_lowercase().chain(c).collect())
}

/// Readable name of an index the long-run growth is borrowed from.
fn index_name(symbol: &str) -> &str {
    match symbol {
        "^NSEI" => "NIFTY 50",
        "^BSESN" => "SENSEX",
        "^GSPC" => "S&P 500",
        "^NDX" => "Nasdaq 100",
        s => s,
    }
}

/// Share of the long-run outcomes (log growth over `lr.years`) that beat a deposit at `rate`.
pub fn beat_chance(lr: &LongRun, rate: f64) -> Option<f64> {
    if lr.resampled.is_empty() {
        return None;
    }
    let hurdle = lr.years as f64 * rate.ln_1p();
    Some(lr.resampled.iter().filter(|x| **x > hurdle).count() as f64 / lr.resampled.len() as f64)
}

/// The holder rule the track record replays: hold min(1, usual swing ÷ today's swing).
pub fn keep_share(report: &AnalysisReport) -> Option<(f64, f64)> {
    let (now, usual) = (
        report.volatility.ewma_annual,
        report.distribution.vol_annual,
    );
    (now > 0.0 && usual > 0.0).then(|| ((usual / now).min(1.0), now / usual))
}

/// Above this share of the usual amount, "trim" would be a rounding error: say "keep".
const KEEP_ABOVE: f64 = 0.95;
/// The owner's bar for a firm long-term statement, and the softer SIP tier (27 Nov 2026).
const FIRM: f64 = 0.8;
const LEAN: f64 = 0.6;

pub fn plain(report: &AnalysisReport, advice: Option<&Advice>, fd_rate: f64, kind: Kind) -> Plain {
    let fd = format!("{}%", (fd_rate * 1000.0).round() / 10.0);
    let three = report
        .long_run
        .as_ref()
        .and_then(|l| l.iter().find(|r| r.years == 3));
    let p3 = three.and_then(|t| beat_chance(t, fd_rate));

    // Buying for 3+ years.
    let (long_action, long) = match (kind, p3) {
        (Kind::Currency, _) => (
            Action::Watch,
            Answer {
                question: "Buying for 3+ years".into(),
                verdict: "Not an investment".into(),
                why: "An exchange rate has no growth of its own; it matters for what you hold abroad.".into(),
            },
        ),
        (_, None) => (
            Action::Watch,
            Answer {
                question: "Buying for 3+ years".into(),
                verdict: "Too little history to say".into(),
                why: "It needs at least a year of prices to compare with a fixed deposit.".into(),
            },
        ),
        (Kind::SingleStock, Some(p)) => (
            Action::IndexInstead,
            Answer {
                question: "Buying for 3+ years".into(),
                verdict: "Buy the index fund instead".into(),
                why: format!(
                    "One company's past growth can't be told apart from its market's ({}), so on its own it adds risk without adding expected return. Built from its own swings, it beat a {fd} FD over 3 years in {} of 100 futures.",
                    three.map_or("the index", |t| index_name(&t.drift_from)),
                    (p * 100.0).round()
                ),
            },
        ),
        (Kind::Other, Some(p)) => {
            let (action, verdict) = if p >= FIRM {
                (Action::Buy, "Buy and hold for 3+ years")
            } else if p >= LEAN {
                (Action::Sip, "Buy monthly (SIP) for 3+ years")
            } else {
                (Action::Fd, "An FD has done as well")
            };
            let actual = three
                .filter(|t| t.n_independent > 0)
                .map(|t| format!(" Only {} separate 3-year periods actually happened, so treat it as rough.", t.n_independent))
                .unwrap_or_default();
            (
                action,
                Answer {
                    question: "Buying for 3+ years".into(),
                    verdict: verdict.into(),
                    why: format!("Built from its own past swings, it beat a {fd} FD over 3 years in {} of 100 futures.{actual}", (p * 100.0).round()),
                },
            )
        }
    };

    // Already own it.
    let share = keep_share(report);
    let owner = match share {
        None => Answer {
            question: "If you own it".into(),
            verdict: "Keep your usual amount".into(),
            why: "Too little history to judge its normal swings.".into(),
        },
        Some((s, ratio)) if s >= KEEP_ABOVE => Answer {
            question: "If you own it".into(),
            verdict: "Keep it all".into(),
            why: format!(
                "It is moving {ratio:.1}× its usual amount, so there is no reason to hold less."
            ),
        },
        Some((s, ratio)) => Answer {
            question: "If you own it".into(),
            verdict: format!("Trim to {}% of your usual amount", (s * 20.0).round() * 5.0),
            why: format!(
                "It is swinging {ratio:.1}× more than usual. Holding less when it is jumpier is the rule tested below."
            ),
        },
    };

    // Trading short term.
    let proven = advice.filter(|a| a.stance != Stance::Hold && matches!(a.basis, Basis::Proven));
    let trader = match (proven, advice.map(|a| a.basis)) {
        (Some(a), _) => Answer {
            question: "Trading in the next days".into(),
            verdict: format!(
                "{} for {}",
                if a.stance == Stance::Buy { "Buy" } else { "Sell / stay out" },
                a.horizon_days.map_or("the rule's period".into(), horizon_label)
            ),
            why: match &a.confidence {
                Some(c) => format!("{} Right {} over {} past calls.", a.headline, in_100(c.hit_rate), c.n),
                None => a.headline.clone(),
            },
        },
        (None, Some(Basis::ProvenButQuiet)) => Answer {
            question: "Trading in the next days".into(),
            verdict: "Wait for the signal".into(),
            why: "A rule that has worked here has nothing to say today.".into(),
        },
        _ => Answer {
            question: "Trading in the next days".into(),
            verdict: "Don't trade it short term".into(),
            why: "No tested rule beat the 0.25% trading cost here, from 1 day to 3 months; up or down over days is close to a coin toss.".into(),
        },
    };

    let (action, headline) = match (proven, share) {
        (Some(_), _) => (
            if proven.is_some_and(|a| a.stance == Stance::Buy) {
                Action::Buy
            } else {
                Action::Sell
            },
            format!("{}: {}", trader.verdict, trader.why),
        ),
        (None, Some((s, _))) if s < KEEP_ABOVE => (
            Action::Trim,
            format!(
                "If you own it: {}. Otherwise: {}.",
                lower_first(&owner.verdict),
                lower_first(&long.verdict)
            ),
        ),
        _ => (
            long_action,
            format!(
                "{}. If you own it: {}.",
                long.verdict,
                lower_first(&owner.verdict)
            ),
        ),
    };

    let checks = report
        .validation
        .as_ref()
        .map(|v| v.chance_of_gain.as_slice())
        .unwrap_or(&[]);
    let ranges: Vec<RangeRow> = checks
        .iter()
        .map(|c| RangeRow {
            horizon_days: c.horizon_days,
            label: horizon_label(c.horizon_days),
            returns: c.range_now,
            held: c.range_held,
            n_tested: c.n,
            n_stretches: c.n_stretches,
        })
        .collect();
    let month = checks
        .iter()
        .find(|c| c.horizon_days == 21)
        .or(checks.first());
    let direction = month.map(|c| {
        if c.direction_right > c.realised + 0.05 {
            format!(
                "Guessing the direction {} ahead from its past did a little better than always guessing up ({} vs {}).",
                horizon_label(c.horizon_days),
                in_100(c.direction_right),
                in_100(c.realised)
            )
        } else {
            format!(
                "Up or down: can't be told. Guessing the direction {} ahead from its past was right {}, no better than always guessing up ({}).",
                horizon_label(c.horizon_days),
                in_100(c.direction_right),
                in_100(c.realised)
            )
        }
    });
    let how = month.map(|c| {
        format!(
            "Worked out from every {} stretch in its history ({} of them): what the money became in each, leaving out the best 5 and the worst 5 in 100. Checked on {} past dates without looking ahead, the range held {}.",
            horizon_label(c.horizon_days).replace(' ', "-"),
            c.n_stretches,
            c.n,
            in_100(c.range_held)
        )
    });
    let track_record = report.track_record.as_ref().map(|tr| {
        let verdict = if tr.app.worst_fall > tr.hold.worst_fall + 0.02 {
            if tr.app.sharpe >= tr.hold.sharpe - 0.01 { "It saved money in the big falls." } else { "It saved money in the big falls but cost some growth (insurance, not extra return)." }
        } else {
            "It made little difference here."
        };
        format!(
            "Holding less when it's jumpier, since {}: worst fall {} instead of {} just holding; {:.1}% a year instead of {:.1}%. {verdict}",
            tr.first.format("%b %Y"),
            signed_pct(tr.app.worst_fall),
            signed_pct(tr.hold.worst_fall),
            tr.app.growth * 100.0,
            tr.hold.growth * 100.0
        )
    });

    Plain {
        action,
        headline,
        answers: vec![long, owner, trader],
        ranges,
        direction,
        how,
        track_record,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{AnalysisInput, analyze};
    use chrono::NaiveDate;

    fn report(vol_now_scale: f64) -> AnalysisReport {
        use rand::SeedableRng;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(3);
        let n = 2600;
        let mut price = 100.0;
        let start = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap();
        let prices: Vec<(NaiveDate, f64)> = (0..n)
            .map(|i| {
                let z: f64 =
                    rand_distr::Distribution::sample(&rand_distr::StandardNormal, &mut rng);
                let sd = if i > n - 40 {
                    0.01 * vol_now_scale
                } else {
                    0.01
                };
                price *= (0.0004 + sd * z).exp();
                (start + chrono::Days::new(i as u64), price)
            })
            .collect();
        analyze(&AnalysisInput {
            symbol: "TEST",
            prices: &prices,
            benchmark: None,
            horizons: &[5, 21, 63],
            n_paths: 200,
            seed: 1,
            long_run_drift_from: None,
        })
        .unwrap()
    }

    #[test]
    fn never_a_bare_hold_and_every_answer_has_a_number_or_reason() {
        let r = report(1.0);
        for kind in [Kind::SingleStock, Kind::Currency, Kind::Other] {
            let p = plain(&r, None, 0.07, kind);
            assert!(!p.headline.trim().eq_ignore_ascii_case("hold"), "{p:?}");
            assert_eq!(p.answers.len(), 3);
            assert!(
                p.answers
                    .iter()
                    .all(|a| !a.verdict.is_empty() && !a.why.is_empty())
            );
        }
        let stock = plain(&r, None, 0.07, Kind::SingleStock);
        assert_eq!(stock.action, Action::IndexInstead);
        assert_eq!(stock.answers[2].verdict, "Don't trade it short term");
        assert_eq!(stock.ranges.len(), 3);
        assert!(
            stock
                .ranges
                .iter()
                .all(|row| row.returns[0] < row.returns[4] && row.held > 0.7)
        );
        assert!(stock.direction.is_some() && stock.how.is_some() && stock.track_record.is_some());
    }

    #[test]
    fn jumpy_holding_is_trimmed_by_the_tested_rule_and_never_raised() {
        let calm = plain(&report(0.5), None, 0.07, Kind::Other);
        assert_eq!(
            calm.answers[1].verdict, "Keep it all",
            "calm never means hold more: {calm:?}"
        );
        let stormy = plain(&report(3.0), None, 0.07, Kind::Other);
        assert_eq!(stormy.action, Action::Trim, "{stormy:?}");
        assert!(
            stormy.answers[1].verdict.starts_with("Trim to "),
            "{stormy:?}"
        );
    }
}
