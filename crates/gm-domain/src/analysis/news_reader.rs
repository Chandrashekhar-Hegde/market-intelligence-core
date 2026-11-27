//! Read a headline the way an analyst skims one: what kind of event it is, whether it is good
//! or bad for the company, and how much it usually matters. Pure, free and instant: hand-written
//! event patterns with negation handling, then a small finance word list for the rest.
//!
//! The news vote turns today's readings into a stance. Owner decision (27 Nov 2026): strong news
//! may drive a call on its own, always labelled "based on today's news; no track record yet".
//! The weights and the threshold below are **uncalibrated** judgement calls, documented in
//! docs/METHODOLOGY.md ("News reading"); #107 calibrates them once news history is stored.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// results, order_win, guidance, rating, legal, credit, management, stake, payout,
    /// fundraise, merger, approval, general
    pub kind: String,
    /// +1 good for the company, −1 bad, 0 unclear.
    pub direction: i8,
    /// 1 low, 2 medium, 3 high.
    pub materiality: u8,
    /// One plain sentence: why it reads this way.
    pub reason: String,
    /// "built-in" or the model that read it.
    pub reader: String,
}

pub(super) const UP: &[&str] = &[
    "rise", "rises", "rose", "jump", "jumps", "jumped", "surge", "surges", "surged", "soar",
    "soars", "beat", "beats", "up", "grow", "grows", "grew", "growth", "record", "higher", "gain",
    "gains", "rally", "rallies", "strong", "boost", "boosts", "climbs", "doubles", "triples",
];
pub(super) const DOWN: &[&str] = &[
    "fall", "falls", "fell", "drop", "drops", "dropped", "decline", "declines", "declined", "miss",
    "misses", "missed", "slump", "slumps", "down", "lower", "loss", "losses", "plunge", "plunges",
    "plunged", "tumble", "tumbles", "sink", "sinks", "weak", "slides", "slid", "crash", "halves",
];
/// Words that undo a legal or regulatory threat.
const RELIEF: &[&str] = &[
    "dismiss",
    "dismisses",
    "dismissed",
    "quash",
    "quashes",
    "quashed",
    "drop",
    "drops",
    "dropped",
    "withdraw",
    "withdraws",
    "withdrawn",
    "clear",
    "clears",
    "cleared",
    "relief",
    "acquit",
    "acquitted",
    "set aside",
    "stays",
    "stayed",
    "denies",
    "rejects plea",
    "wins case",
    "closes probe",
    "no wrongdoing",
];
const POSITIVE_WORDS: &[&str] = &[
    "profit",
    "expansion",
    "expands",
    "launch",
    "launches",
    "partnership",
    "invest",
    "invests",
    "investment",
    "milestone",
    "leader",
    "upbeat",
    "optimistic",
    "outperform",
    "robust",
    "wins",
    "award",
    "approved",
    "breakthrough",
    "recovery",
];
const NEGATIVE_WORDS: &[&str] = &[
    "concern",
    "concerns",
    "warning",
    "warns",
    "risk",
    "risks",
    "cut",
    "cuts",
    "layoffs",
    "struggles",
    "struggling",
    "delay",
    "delays",
    "shutdown",
    "strike",
    "pressure",
    "slowdown",
    "underperform",
    "pessimistic",
    "glitch",
    "outage",
    "recall",
    "probe",
];

pub(super) fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '&')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn has_word(ws: &[String], list: &[&str]) -> bool {
    ws.iter().any(|w| list.contains(&w.as_str()))
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

fn reading(kind: &str, direction: i8, materiality: u8, reason: impl Into<String>) -> Reading {
    Reading {
        kind: kind.into(),
        direction,
        materiality,
        reason: reason.into(),
        reader: "built-in".into(),
    }
}

/// Up/down from direction words; 0 if both or neither appear.
fn tilt(ws: &[String]) -> i8 {
    match (has_word(ws, UP), has_word(ws, DOWN)) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    }
}

pub fn read_headline(title: &str) -> Reading {
    let t = format!(" {} ", title.to_lowercase());
    let ws = words(title);
    let relief = has_any(&t, RELIEF);

    // Order matters: the most consequential event types first.
    if has_any(
        &t,
        &[
            " fraud",
            " scam",
            " raid",
            " probe",
            " investigation",
            " penalty",
            " fine ",
            " fined",
            " lawsuit",
            " sued",
            " charges",
            " charge-sheet",
            " chargesheet",
            " insider trading",
            " ban ",
            " bans ",
            " barred",
            " show cause",
            " show-cause",
            " forensic audit",
        ],
    ) {
        return if relief {
            reading(
                "legal",
                1,
                2,
                "A legal or regulatory threat is being dropped or eased, which removes a risk.",
            )
        } else {
            reading(
                "legal",
                -1,
                3,
                "Legal or regulatory trouble (probe, penalty, fraud allegation) usually hits the share price.",
            )
        };
    }
    // Court matters without a threat word: relief is good news, otherwise the direction is unclear.
    if has_any(
        &t,
        &[
            " court",
            " sc ",
            " slp",
            " petition",
            " plea",
            " tribunal",
            " nclat",
            " sat ",
            " arbitration",
        ],
    ) {
        return if relief {
            reading(
                "legal",
                1,
                2,
                "A court or tribunal ruled in the company's favour, removing a risk.",
            )
        } else {
            reading(
                "legal",
                0,
                2,
                "A court matter; the headline doesn't say which way it went.",
            )
        };
    }
    if has_any(
        &t,
        &[
            " default",
            " insolvency",
            " bankruptcy",
            " nclt",
            " missed payment",
        ],
    ) {
        return if relief {
            reading(
                "credit",
                1,
                2,
                "A debt or insolvency threat is being resolved.",
            )
        } else {
            reading(
                "credit",
                -1,
                3,
                "Debt trouble (default, insolvency) is among the worst news for shareholders.",
            )
        };
    }
    if has_any(
        &t,
        &[
            "crisil",
            "icra",
            "moody",
            "fitch",
            "s&p ",
            "care ratings",
            "india ratings",
        ],
    ) && has_any(&t, &["upgrade", "downgrade", "outlook"])
    {
        let d = if has_any(&t, &["downgrade", "negative outlook"]) {
            -1
        } else {
            1
        };
        return reading(
            "credit",
            d,
            2,
            if d > 0 {
                "A credit rating agency rated its debt higher."
            } else {
                "A credit rating agency rated its debt lower."
            },
        );
    }
    // Market-move reports that mention "profit" without being results: "snaps winning streak
    // on profit booking; HCL Tech, TCS lead losses" describes trading, not the companies.
    if has_any(
        &t,
        &[
            "profit booking",
            "profit-booking",
            "profit taking",
            "profit-taking",
            "lead losses",
            "lead gains",
            "winning streak",
            "losing streak",
        ],
    ) {
        return reading(
            "price_move",
            0,
            1,
            "A report of share prices moving (profit booking, a streak), not news about the company itself.",
        );
    }
    if has_any(
        &t,
        &[
            " q1",
            " q2",
            " q3",
            " q4",
            "quarter",
            " results",
            " earnings",
            "net profit",
            " profit ",
            " revenue",
            " ebitda",
            " pat ",
        ],
    ) {
        let d = if has_any(&t, &["beats estimates", "beat estimates", "tops estimates"]) {
            1
        } else if has_any(
            &t,
            &["misses estimates", "miss estimates", "below estimates"],
        ) {
            -1
        } else {
            tilt(&ws)
        };
        return match d {
            1 => reading(
                "results",
                1,
                3,
                "Better results (profit or revenue up, or beating estimates).",
            ),
            -1 => reading(
                "results",
                -1,
                3,
                "Weaker results (profit or revenue down, or missing estimates).",
            ),
            _ => reading(
                "results",
                0,
                2,
                "A results story without a clear better/worse signal in the headline.",
            ),
        };
    }
    if has_any(&t, &["guidance", "outlook", "forecast"])
        && has_any(
            &t,
            &[
                "raise", "raises", "lifts", "upgrades", "cut", "cuts", "lowers", "slashes", "trims",
            ],
        )
    {
        let d = if has_any(&t, &["raise", "raises", "lifts", "upgrades"]) {
            1
        } else {
            -1
        };
        return reading(
            "guidance",
            d,
            2,
            if d > 0 {
                "The company expects to do better than it said before."
            } else {
                "The company expects to do worse than it said before."
            },
        );
    }
    // Reports of the share price itself ("surges 3% to day's high") describe a move that already
    // happened; reading them as news would just chase the price.
    let percent_move = t.split_whitespace().any(|w| {
        w.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '%')
            .ends_with('%')
    }) && tilt(&ws) != 0;
    let price_report = [
        "day's high",
        "day's low",
        "day’s high",
        "day’s low",
        "52-week",
        "52 week",
        "outperforms sector",
        "underperforms sector",
        "top gainers",
        "top losers",
        "stocks to watch",
        "in focus",
        "share price today",
        "stock price today",
        "trading higher",
        "trading lower",
    ];
    if percent_move || has_any(&t, &price_report) {
        return reading(
            "price_move",
            0,
            1,
            "A report of the share price moving, not news about the company itself.",
        );
    }
    if has_any(
        &t,
        &[
            "upgrade",
            "downgrade",
            "target price",
            "price target",
            "outperform",
            "underperform",
            " buy rating",
            " sell rating",
            "overweight",
            "underweight",
        ],
    ) {
        let d = if has_any(
            &t,
            &[
                "downgrade",
                "underperform",
                "sell rating",
                "underweight",
                "cuts target",
                "cut target",
                "target cut",
                "lowers target",
            ],
        ) {
            -1
        } else {
            1
        };
        return reading(
            "rating",
            d,
            2,
            if d > 0 {
                "A broker became more positive on the stock."
            } else {
                "A broker became more negative on the stock."
            },
        );
    }
    if has_any(
        &t,
        &[
            " order",
            " contract",
            " deal worth",
            " project worth",
            " tender",
        ],
    ) && has_any(
        &t,
        &[
            "wins", "win ", "bags", "secures", "lands", "awarded", "receives", "bagged", "won",
        ],
    ) {
        return reading(
            "order_win",
            1,
            2,
            "A new order or contract adds future revenue.",
        );
    }
    if has_any(
        &t,
        &[
            " ceo",
            " cfo",
            " md ",
            " managing director",
            " chairman",
            " director",
            " auditor",
        ],
    ) && has_any(
        &t,
        &[
            "resigns",
            "quits",
            "steps down",
            "exits",
            "resignation",
            "sacked",
            "removed",
        ],
    ) {
        return reading(
            "management",
            -1,
            2,
            "A senior departure creates uncertainty about leadership.",
        );
    }
    if has_any(
        &t,
        &[
            "block deal",
            "bulk deal",
            "stake sale",
            "sells stake",
            "offloads",
            "pledge",
            "trims stake",
        ],
    ) {
        return reading(
            "stake",
            -1,
            1,
            "A large holder selling or pledging shares can weigh on the price.",
        );
    }
    if has_any(
        &t,
        &[
            "buys stake",
            "acquires stake",
            "raises stake",
            "ups stake",
            "promoter buys",
        ],
    ) {
        return reading(
            "stake",
            1,
            1,
            "Insiders or large investors buying in is read as confidence.",
        );
    }
    if has_any(
        &t,
        &[
            "dividend",
            "buyback",
            "buy-back",
            "bonus issue",
            "bonus shares",
            "stock split",
        ],
    ) {
        return reading(
            "payout",
            1,
            2,
            "Returning cash to shareholders (dividend, buyback, bonus) is usually welcomed.",
        );
    }
    if has_any(
        &t,
        &[
            " qip",
            "rights issue",
            "bond issue",
            "raise funds",
            "fundraise",
            "fund raise",
            "ncd",
            "share sale",
            " ofs",
        ],
    ) {
        return reading(
            "fundraise",
            0,
            1,
            "Raising money: good if well used, but new shares dilute existing holders.",
        );
    }
    if has_any(
        &t,
        &[
            "acquire",
            "acquires",
            "acquisition",
            "merger",
            "merge",
            "takeover",
            "to buy ",
        ],
    ) {
        return reading(
            "merger",
            0,
            2,
            "A deal to buy or merge: its effect depends on the price paid.",
        );
    }
    if has_any(
        &t,
        &[
            "approval",
            "approves",
            " nod",
            "clearance",
            "licence",
            "license",
        ],
    ) {
        return reading(
            "approval",
            1,
            2,
            "A regulator or partner approved something the company needed.",
        );
    }
    let pos = has_word(&ws, POSITIVE_WORDS) || tilt(&ws) > 0;
    let neg = has_word(&ws, NEGATIVE_WORDS) || tilt(&ws) < 0;
    match (pos, neg) {
        (true, false) => reading(
            "general",
            1,
            1,
            "Upbeat wording, but no clear market-moving event.",
        ),
        (false, true) => reading(
            "general",
            -1,
            1,
            "Downbeat wording, but no clear market-moving event.",
        ),
        _ => reading(
            "general",
            0,
            1,
            "No clear market-moving event in the headline.",
        ),
    }
}

/// Materiality → weight in the news score. Uncalibrated (see module docs).
pub const MATERIALITY_WEIGHT: [f64; 4] = [0.0, 0.25, 0.5, 1.0];
/// A story's weight halves every this many hours.
pub const HALF_LIFE_HOURS: f64 = 48.0;
/// |score| at or above this, with at least one medium or high story pointing that way, is
/// strong news. One fresh high-materiality story reaches it; low-materiality chatter alone
/// never does. Uncalibrated.
pub const STRONG_SCORE: f64 = 0.9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsVote {
    pub score: f64,
    /// +1 strong good news, −1 strong bad news, 0 neither.
    pub stance: i8,
    pub n_stories: usize,
    /// Indices (into the readings given) of the material stories behind the stance, strongest first.
    pub drivers: Vec<usize>,
}

/// `readings`: one per story, with how many hours ago it appeared.
/// Kinds that are context, not events: shown to the user but never voting.
pub const NON_VOTING: [&str; 2] = ["general", "price_move"];

pub fn news_vote(readings: &[(Reading, f64)]) -> NewsVote {
    let contribution = |(r, hours): &(Reading, f64)| {
        if NON_VOTING.contains(&r.kind.as_str()) {
            return 0.0;
        }
        f64::from(r.direction)
            * MATERIALITY_WEIGHT[usize::from(r.materiality.min(3))]
            * 0.5f64.powf(hours.max(0.0) / HALF_LIFE_HOURS)
    };
    // One event is usually covered by many outlets: within each kind, the stories together count
    // for no more than the strongest single one.
    let mut by_kind: std::collections::BTreeMap<&str, (f64, f64)> = Default::default();
    for item in readings {
        let c = contribution(item);
        let entry = by_kind.entry(item.0.kind.as_str()).or_default();
        entry.0 += c;
        entry.1 = entry.1.max(c.abs());
    }
    let score: f64 = by_kind
        .values()
        .map(|(sum, strongest)| sum.signum() * sum.abs().min(*strongest))
        .sum();
    let sign = if score > 0.0 { 1 } else { -1 };
    let mut drivers: Vec<usize> = (0..readings.len())
        .filter(|&i| {
            readings[i].0.direction == sign
                && readings[i].0.materiality >= 2
                && !NON_VOTING.contains(&readings[i].0.kind.as_str())
        })
        .collect();
    drivers.sort_by(|&a, &b| {
        contribution(&readings[b])
            .abs()
            .total_cmp(&contribution(&readings[a]).abs())
    });
    let stance = if score.abs() >= STRONG_SCORE && !drivers.is_empty() {
        sign
    } else {
        0
    };
    NewsVote {
        score,
        stance,
        n_stories: readings.len(),
        drivers: if stance == 0 { Vec::new() } else { drivers },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn reads(title: &str, kind: &str, direction: i8) {
        let r = read_headline(title);
        assert_eq!(
            (r.kind.as_str(), r.direction),
            (kind, direction),
            "{title}: {r:?}"
        );
    }

    #[test]
    fn reads_real_style_headlines() {
        reads(
            "Reliance Q2 net profit rises 12% on retail, Jio strength",
            "results",
            1,
        );
        reads(
            "TCS Q3 results: profit falls 5%, misses estimates",
            "results",
            -1,
        );
        reads(
            "Infosys beats estimates, raises FY27 revenue guidance",
            "results",
            1,
        );
        reads(
            "L&T bags order worth Rs 2,500 crore from Middle East",
            "order_win",
            1,
        );
        reads(
            "SEBI orders forensic audit of XYZ Ltd over fund diversion",
            "legal",
            -1,
        );
        reads(
            "ED raids premises of ABC Industries in money laundering probe",
            "legal",
            -1,
        );
        reads(
            "SC dismisses Revenue's SLP against Reliance Industries over delay",
            "legal",
            1,
        );
        reads(
            "Court quashes fraud charges against company founder",
            "legal",
            1,
        );
        reads(
            "Moody's downgrades Vedanta outlook to negative",
            "credit",
            -1,
        );
        reads(
            "Jefferies upgrades HDFC Bank to buy, raises target price",
            "rating",
            1,
        );
        reads(
            "Morgan Stanley downgrades Infosys to underweight",
            "rating",
            -1,
        );
        reads(
            "Company CFO resigns with immediate effect",
            "management",
            -1,
        );
        reads(
            "Promoter sells stake via block deal worth Rs 1,200 crore",
            "stake",
            -1,
        );
        reads(
            "Board approves buyback of shares at Rs 4,500 each",
            "payout",
            1,
        );
        reads(
            "Adani Enterprises plans up to 20 billion rupee bond issue in December",
            "fundraise",
            0,
        );
        reads(
            "Praveen George joins Reliance Industries as VP and head-HR",
            "general",
            0,
        );
        reads(
            "Black Friday Apple deals: record prices on AirPods",
            "general",
            1,
        );
        reads(
            "Infosys Ltd Surges 3.08% to Day's High of Rs 1166.45 — Outperforms Sector by 1.0",
            "price_move",
            0,
        );
        reads(
            "Adani Enterprises, Adani Power shares rise 2%: Here's why",
            "price_move",
            0,
        );
        reads("HDFC Bank hits 52-week high", "price_move", 0);
        // Recorded 27 Nov 2026: produced two news-driven Sells before this rule.
        reads(
            "Nifty IT snaps 4-day winning streak on profit booking; HCL Tech, TCS lead losses",
            "price_move",
            0,
        );
        // A price move explained by results is a results story.
        reads(
            "TCS shares fall 3% after Q2 profit misses estimates",
            "results",
            -1,
        );
    }

    fn story(direction: i8, materiality: u8, hours: f64) -> (Reading, f64) {
        (reading("legal", direction, materiality, ""), hours)
    }

    #[test]
    fn many_stories_about_one_event_count_once_and_chatter_never_votes() {
        let one = news_vote(&[(reading("rating", 1, 2, ""), 1.0)]);
        let ten: Vec<_> = (0..10)
            .map(|_| (reading("rating", 1, 2, ""), 1.0))
            .collect();
        assert!((news_vote(&ten).score - one.score).abs() < 1e-12);
        let chatter: Vec<_> = (0..30)
            .map(|_| (reading("general", 1, 1, ""), 1.0))
            .collect();
        assert_eq!(news_vote(&chatter).score, 0.0);
        // Two different notable good events do add up to strong news.
        let two = news_vote(&[
            (reading("order_win", 1, 2, ""), 1.0),
            (reading("payout", 1, 2, ""), 1.0),
        ]);
        assert_eq!(two.stance, 1);
    }

    #[test]
    fn one_fresh_major_story_is_strong_but_chatter_is_not() {
        let v = news_vote(&[story(-1, 3, 2.0), story(0, 1, 1.0)]);
        assert_eq!(v.stance, -1);
        assert_eq!(v.drivers, vec![0]);
        // Five upbeat but immaterial stories never add up to a call.
        let chatter: Vec<_> = (0..5).map(|_| story(1, 1, 1.0)).collect();
        assert_eq!(news_vote(&chatter).stance, 0);
        // The same major story three days old has faded below the threshold.
        assert_eq!(news_vote(&[story(1, 3, 72.0)]).stance, 0);
        // Good and bad news cancel.
        assert_eq!(news_vote(&[story(1, 3, 1.0), story(-1, 3, 1.0)]).stance, 0);
        assert_eq!(news_vote(&[]).stance, 0);
    }
}
