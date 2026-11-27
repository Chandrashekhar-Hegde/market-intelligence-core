//! Market news, read for what it means beyond one company: an RBI or Fed rate move, crude,
//! gold, silver, the rupee, the dollar and foreign-investor flows, each mapped to the assets
//! it usually moves (textbook channels, listed in docs/METHODOLOGY.md "Market news desk").
//!
//! Each theme's direction comes from the words right after (or just before) its subject, so
//! "Rupee slides as oil rallies" reads as rupee down and oil up. Reports of an index move
//! ("Sensex rises 300 points") are shown but never count: they describe, not explain.
//! The leans are **uncalibrated** (one medium weight, the company reader's half-life) and are
//! shown as leans, never as calls; #127 records outcomes to calibrate them.

use serde::Serialize;

use super::news_reader::{DOWN, HALF_LIFE_HOURS, UP, words};

/// The assets market news is mapped to, with the symbol the app analyses for each.
pub const ASSETS: [(&str, &str, &str); 10] = [
    ("nifty", "Indian shares (NIFTY 50)", "^NSEI"),
    ("banks", "Indian banks", "^NSEBANK"),
    ("it", "Indian IT exporters", "^CNXIT"),
    ("omc", "Oil marketers (BPCL, HPCL, IOC)", "BPCL.NS"),
    ("gold", "Gold", "GOLDBEES.NS"),
    ("silver", "Silver", "SILVERBEES.NS"),
    ("usdinr", "Dollar in rupees (up = weaker rupee)", "INR=X"),
    ("crude", "Crude oil", "BZ=F"),
    ("us", "US shares (S&P 500)", "^GSPC"),
    ("nasdaq", "US tech (Nasdaq 100)", "^NDX"),
];

/// Weight of one market story (the company reader's "medium"), and the |score| from which a
/// lean is shown: about one fresh story. Both uncalibrated.
pub const STORY_WEIGHT: f64 = 0.5;
pub const LEAN_SCORE: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Words {
    /// Moves: rises / falls and the like.
    Price,
    /// Policy rates: hike / cut.
    Rate,
    /// Investor flows: buying / selling.
    Flow,
}

struct Rule {
    theme: &'static str,
    /// Word sequences naming the subject.
    subject: &'static [&'static [&'static str]],
    words: Words,
    /// Effects when the subject goes up (rate hike, buying); reversed when it goes down.
    effects: &'static [(&'static str, i8)],
    why_up: &'static str,
    why_down: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        theme: "Rates",
        subject: &[
            &["rbi"],
            &["repo"],
            &["reserve", "bank", "of", "india"],
            &["mpc"],
        ],
        words: Words::Rate,
        effects: &[("banks", -1), ("nifty", -1)],
        why_up: "A higher RBI rate raises borrowing costs, which usually weighs on banks and shares.",
        why_down: "An RBI rate cut lowers borrowing costs, which usually helps banks and shares.",
    },
    Rule {
        theme: "Rates",
        subject: &[&["fed"], &["federal", "reserve"], &["fomc"], &["powell"]],
        words: Words::Rate,
        effects: &[("us", -1), ("nasdaq", -1), ("gold", -1), ("usdinr", 1)],
        why_up: "Higher US rates usually weigh on US shares and gold and strengthen the dollar.",
        why_down: "Lower US rates usually help US shares and gold and weaken the dollar.",
    },
    Rule {
        theme: "Commodities",
        subject: &[
            &["crude"],
            &["brent"],
            &["oil", "price"],
            &["oil", "prices"],
        ],
        words: Words::Price,
        effects: &[("crude", 1), ("omc", -1), ("usdinr", 1)],
        why_up: "Dearer crude squeezes India's oil marketers and weighs on the rupee (India imports most of its oil).",
        why_down: "Cheaper crude helps India's oil marketers and the rupee.",
    },
    Rule {
        theme: "Commodities",
        subject: &[&["gold"]],
        words: Words::Price,
        effects: &[("gold", 1)],
        why_up: "Gold prices rising.",
        why_down: "Gold prices falling.",
    },
    Rule {
        theme: "Commodities",
        subject: &[&["silver"]],
        words: Words::Price,
        effects: &[("silver", 1)],
        why_up: "Silver prices rising.",
        why_down: "Silver prices falling.",
    },
    Rule {
        theme: "Currency",
        subject: &[&["rupee"], &["inr"]],
        words: Words::Price,
        effects: &[("usdinr", -1), ("it", -1)],
        why_up: "A stronger rupee trims the rupee value of IT exporters' dollar revenue.",
        why_down: "A weaker rupee lifts the rupee value of IT exporters' dollar revenue.",
    },
    Rule {
        theme: "Currency",
        subject: &[&["dollar"], &["greenback"], &["dxy"]],
        words: Words::Price,
        effects: &[("usdinr", 1), ("gold", -1)],
        why_up: "A stronger dollar usually weighs on gold and the rupee.",
        why_down: "A weaker dollar usually helps gold and the rupee.",
    },
    Rule {
        theme: "Flows",
        subject: &[
            &["fii"],
            &["fiis"],
            &["fpi"],
            &["fpis"],
            &["foreign", "investors"],
            &["foreign", "funds"],
        ],
        words: Words::Flow,
        effects: &[("nifty", 1)],
        why_up: "Foreign investors buying Indian shares supports the market.",
        why_down: "Foreign investors selling Indian shares weighs on the market.",
    },
];

const RATE_UP: &[&str] = &[
    "hike",
    "hikes",
    "hiked",
    "raise",
    "raises",
    "raised",
    "tightening",
    "hawkish",
];
const RATE_DOWN: &[&str] = &[
    "cut", "cuts", "lowers", "lowered", "easing", "eases", "dovish",
];
const FLOW_UP: &[&str] = &[
    "buy", "buys", "buying", "bought", "inflow", "inflows", "pour", "comeback", "return", "returns",
];
const FLOW_DOWN: &[&str] = &[
    "sell", "sells", "selling", "sold", "outflow", "outflows", "dump", "dumps", "pull", "exit",
];
const MORE_UP: &[&str] = &[
    "strengthens",
    "firms",
    "recovers",
    "recovery",
    "rebounds",
    "rebound",
    "advances",
    "strength",
];
const MORE_DOWN: &[&str] = &[
    "weakens", "weaker", "weakness", "slips", "slide", "eases", "corrects", "retreats", "low",
    "softens", "softer",
];
/// Rate talk that is hedged or receding ("eases pressure on RBI to use rate hikes") has no
/// direction we can read from keywords.
const RATE_DOUBT: &[&str] = &[
    "eases pressure",
    "unlikely",
    "rules out",
    "ruled out",
    "no need",
    "no rush",
    "less likely",
    "not to ",
];
/// The look-back stops at these: what comes before belongs to another clause
/// ("Silver extends rebound as dollar stalls").
const CLAUSE_BREAKS: &[&str] = &[
    "as", "and", "amid", "while", "but", "against", "after", "despite", "on", "with", "vs",
];
const REPORTS: &[&str] = &["sensex", "nifty", "wall", "nasdaq", "dow", "s&p"];
const AI: &[&str] = &[
    "ai",
    "openai",
    "nvidia",
    "groq",
    "xai",
    "anthropic",
    "chip",
    "chips",
    "semiconductor",
];

fn direction(w: &str, kind: Words) -> i8 {
    let (up, down): (&[&[&str]], &[&[&str]]) = match kind {
        Words::Rate => (&[RATE_UP], &[RATE_DOWN]),
        Words::Flow => (&[FLOW_UP], &[FLOW_DOWN]),
        Words::Price => (&[UP, MORE_UP], &[DOWN, MORE_DOWN]),
    };
    if up.iter().any(|l| l.contains(&w)) {
        1
    } else if down.iter().any(|l| l.contains(&w)) {
        -1
    } else {
        0
    }
}

/// Direction of the subject at `at..at+len`: the first direction word in the 4 words after it,
/// else in the 3 words before ("weaker rupee", "rally in global crude").
fn subject_direction(ws: &[String], at: usize, len: usize, kind: Words) -> i8 {
    let after = ws.iter().skip(at + len).take(4);
    let before = ws[at.saturating_sub(3)..at]
        .iter()
        .rev()
        .take_while(|w| !CLAUSE_BREAKS.contains(&w.as_str()));
    after
        .chain(before)
        .map(|w| direction(w, kind))
        .find(|d| *d != 0)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MacroImpact {
    pub asset: &'static str,
    pub direction: i8,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MacroReading {
    /// Rates, Commodities, Currency, Flows, Market report, AI & tech, or Other.
    pub theme: &'static str,
    pub impacts: Vec<MacroImpact>,
}

pub fn read_macro(title: &str) -> MacroReading {
    let ws = words(title);
    let lower = title.to_lowercase();
    let doubt = RATE_DOUBT.iter().any(|d| lower.contains(d));
    // The theme is the first subject that moved; failing that, the first one named.
    let (mut theme, mut named) = (None, None);
    let mut impacts: Vec<MacroImpact> = Vec::new();
    for rule in RULES {
        let found = rule.subject.iter().find_map(|seq| {
            (0..ws.len().saturating_sub(seq.len() - 1))
                .find(|&i| seq.iter().zip(&ws[i..]).all(|(a, b)| *a == b))
                .map(|i| (i, seq.len()))
        });
        let Some((at, len)) = found else { continue };
        named.get_or_insert(rule.theme);
        let d = if rule.words == Words::Rate && doubt {
            0
        } else {
            subject_direction(&ws, at, len, rule.words)
        };
        if d == 0 {
            continue;
        }
        theme.get_or_insert(rule.theme);
        for (asset, effect) in rule.effects {
            if impacts.iter().all(|i| i.asset != *asset) {
                impacts.push(MacroImpact {
                    asset,
                    direction: d * effect,
                    reason: if d > 0 { rule.why_up } else { rule.why_down },
                });
            }
        }
    }
    let has = |list: &[&str]| ws.iter().any(|w| list.contains(&w.as_str()));
    let theme = theme.or(named).unwrap_or(if has(REPORTS) {
        "Market report"
    } else if has(AI) {
        "AI & tech"
    } else {
        "Other"
    });
    MacroReading { theme, impacts }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssetLean {
    pub asset: &'static str,
    pub label: &'static str,
    pub symbol: &'static str,
    pub score: f64,
    /// +1 today's market news leans up for it, −1 down, 0 quiet or mixed.
    pub lean: i8,
    /// Indices of the stories behind the lean, strongest first.
    pub drivers: Vec<usize>,
}

/// Per asset: Σ direction × weight × ½^(hours/half-life), where each theme counts for no more
/// than its strongest single story (many outlets, one event), as in the company news vote.
pub fn asset_leans(readings: &[(MacroReading, f64)]) -> Vec<AssetLean> {
    ASSETS
        .iter()
        .map(|&(asset, label, symbol)| {
            let mut by_theme: std::collections::BTreeMap<&str, (f64, f64)> = Default::default();
            let mut drivers: Vec<(usize, f64)> = Vec::new();
            for (i, (r, hours)) in readings.iter().enumerate() {
                for impact in r.impacts.iter().filter(|x| x.asset == asset) {
                    let c = f64::from(impact.direction)
                        * STORY_WEIGHT
                        * 0.5f64.powf(hours.max(0.0) / HALF_LIFE_HOURS);
                    let e = by_theme.entry(r.theme).or_default();
                    e.0 += c;
                    e.1 = e.1.max(c.abs());
                    drivers.push((i, c));
                }
            }
            let score: f64 = by_theme
                .values()
                .map(|(sum, strongest)| sum.signum() * sum.abs().min(*strongest))
                .sum();
            let lean = if score >= LEAN_SCORE {
                1
            } else if score <= -LEAN_SCORE {
                -1
            } else {
                0
            };
            drivers.retain(|(_, c)| lean != 0 && c.signum() as i8 == lean);
            drivers.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
            AssetLean {
                asset,
                label,
                symbol,
                score,
                lean,
                drivers: drivers.into_iter().map(|(i, _)| i).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn reads(title: &str, theme: &str, expect: &[(&str, i8)]) {
        let r = read_macro(title);
        let got: Vec<(&str, i8)> = r.impacts.iter().map(|i| (i.asset, i.direction)).collect();
        assert_eq!((r.theme, got.as_slice()), (theme, expect), "{title}");
    }

    /// Real headlines from the recorded feeds (27 Nov 2026), plus textbook cases.
    #[test]
    fn reads_market_news_into_asset_effects() {
        reads(
            "Rupee plunges 24 paise to 92.62 against U.S. dollar",
            "Currency",
            &[("usdinr", 1), ("it", 1)],
        );
        // Two subjects, each with its own direction.
        reads(
            "Rupee Slides as Oil Rally & Hormuz Tensions Hit Mood",
            "Currency",
            &[("usdinr", 1), ("it", 1)],
        );
        reads(
            "Houthi Attack on Saudi Refinery Sparks Fresh Rally in Global Crude Oil Prices",
            "Commodities",
            &[("crude", 1), ("omc", -1), ("usdinr", 1)],
        );
        reads(
            "Crude Oil Futures Rise on Positive Global Trends",
            "Commodities",
            &[("crude", 1), ("omc", -1), ("usdinr", 1)],
        );
        reads(
            "Gold price rebounds from one-year low, but experts feel sentiment remains fragile",
            "Commodities",
            &[("gold", 1)],
        );
        reads(
            "Gold price forecast: Fed rate hike in December may limit recovery",
            "Rates",
            &[("us", -1), ("nasdaq", -1), ("gold", -1), ("usdinr", 1)],
        );
        reads(
            "RBI cuts repo rate by 25 basis points",
            "Rates",
            &[("banks", 1), ("nifty", 1)],
        );
        reads(
            "FIIs turn net sellers, pull out Rs 5,000 crore",
            "Flows",
            &[("nifty", -1)],
        );
        // No direction: theme only.
        reads(
            "RBI governor flags MSME credit needs, calls for stronger lending support",
            "Rates",
            &[],
        );
        // Hedged rate talk has no direction; the theme follows the subject that moved.
        reads(
            "Rupee recovery eases pressure on RBI to use rate hikes as currency defence",
            "Currency",
            &[("usdinr", -1), ("it", -1)],
        );
        reads(
            "Gold Price Recovers as Dollar Softens, Fed Risks Remain",
            "Commodities",
            &[("gold", 1), ("usdinr", -1)],
        );
        // The look-back stops at a clause break; "recovery" and "weakness" have a direction.
        reads(
            "Silver Extends Rebound as Dollar Stalls Near Key Levels",
            "Commodities",
            &[("silver", 1)],
        );
        reads(
            "Gold Price Rises Amid Dollar Weakness and Geopolitical Tensions",
            "Commodities",
            &[("gold", 1), ("usdinr", -1)],
        );
        reads(
            "Rupee weakens against US dollar in early trade on Friday",
            "Currency",
            &[("usdinr", 1), ("it", 1)],
        );
        reads(
            "Sensex rises 300 points, Nifty above 25,900",
            "Market report",
            &[],
        );
        reads("Nvidia results lift AI trade", "AI & tech", &[]);
        reads("Goldman Sachs hires new India head", "Other", &[]);
    }

    #[test]
    fn leans_need_a_story_and_one_event_counts_once() {
        let crude_up = read_macro("Crude oil prices rise for a third day");
        let gold_down = read_macro("Gold falls as dollar strengthens");
        let leans = asset_leans(&[
            (crude_up.clone(), 1.0),
            (crude_up.clone(), 2.0),
            (crude_up, 3.0),
            (gold_down, 1.0),
        ]);
        let get = |a: &str| leans.iter().find(|l| l.asset == a).unwrap();
        // Three copies of one event: no more than the strongest one.
        assert!(
            (get("crude").score - STORY_WEIGHT * 0.5f64.powf(1.0 / HALF_LIFE_HOURS)).abs() < 1e-12
        );
        assert_eq!((get("crude").lean, get("omc").lean), (1, -1));
        assert_eq!(get("crude").drivers, vec![0, 1, 2]);
        assert_eq!(get("gold").lean, -1);
        assert_eq!(get("nifty").lean, 0);
        // A two-day-old story alone is too faint to lean.
        let old = asset_leans(&[(read_macro("Silver jumps 3%"), 60.0)]);
        assert_eq!(old.iter().find(|l| l.asset == "silver").unwrap().lean, 0);
    }
}
