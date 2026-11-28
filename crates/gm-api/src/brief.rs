//! The morning brief (`/api/v1/brief`): what to do today, in a few lines, answering the owner's
//! list (28 Nov 2026): do now, invest in, flagged / going bad, get out or get in now, and the
//! 1-day / 3-day / day-trading picture. The facts are computed by the app (the Today scan and
//! the news desk). A plain template always writes the brief; Gemini may rewrite it in better
//! prose, but only if every number it uses is in the facts, so it can't invent a forecast.

use std::time::{Duration, Instant};

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AppState;

/// The brief is rebuilt at most this often (the Today scan refreshes on the same clock).
pub const TTL: Duration = Duration::from_secs(30 * 60);
/// A day's fall is "going bad" beyond this many of its usual daily swings.
const BAD_DAY_SDS: f64 = 2.0;
/// "Jumpy": today's volatility this many times its usual level (the trim rule's own measure).
const JUMPY: f64 = 1.3;
/// Lines per section, so the brief stays a brief.
const MAX_LINES: usize = 6;

pub type Cache = std::sync::Arc<std::sync::Mutex<Option<(Instant, Value)>>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub bullets: Vec<String>,
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

fn name(v: &Value) -> String {
    v["name"]
        .as_str()
        .or(v["symbol"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// Facts for the brief, every number already written as it should be read.
pub fn facts(items: &[Value], desk: &Value) -> Value {
    let ok: Vec<&Value> = items.iter().filter(|i| i["error"].is_null()).collect();
    let action = |i: &Value| s(i, "action");
    let ratio = |i: &Value| i["vol_ratio"].as_f64().unwrap_or(0.0);

    let mut act: Vec<&Value> = ok
        .iter()
        .copied()
        .filter(|i| matches!(action(i).as_str(), "trim" | "buy" | "sell"))
        .collect();
    act.sort_by(|a, b| ratio(b).total_cmp(&ratio(a)));
    let do_now: Vec<Value> = act
        .iter()
        .take(MAX_LINES)
        .map(|i| {
            // The holder's answer, not the whole headline: "do now" is about what you own.
            let own = &i["answers"][1];
            let line = if action(i) == "trim" {
                {
                    let why = s(own, "why");
                    let first = why
                        .split(". ")
                        .next()
                        .unwrap_or_default()
                        .trim_end_matches('.')
                        .to_string();
                    format!(
                        "if you own it, {}: {}",
                        s(own, "verdict").to_lowercase(),
                        first.to_lowercase()
                    )
                }
            } else {
                s(i, "headline")
            };
            serde_json::json!({ "name": name(i), "action": action(i), "line": line })
        })
        .collect();

    let invest_in: Vec<Value> = ok
        .iter()
        .filter(|i| i["single_stock"] != true)
        .filter_map(|i| {
            let long = &i["answers"][0];
            let verdict = s(long, "verdict");
            (verdict.starts_with("Buy")).then(|| serde_json::json!({ "name": name(i), "verdict": verdict, "why": s(long, "why") }))
        })
        .take(MAX_LINES)
        .collect();
    let index_instead = ok.iter().filter(|i| action(i) == "index_instead").count();

    let mut bad: Vec<(f64, Value)> = ok
        .iter()
        .filter_map(|i| {
            let change = i["change_pct"].as_f64()? / 100.0;
            let daily = i["vol_annual"].as_f64()? / 252f64.sqrt();
            let sds = change / daily.max(1e-9);
            (sds <= -BAD_DAY_SDS).then(|| {
                (
                    sds,
                    serde_json::json!({
                        "name": name(i),
                        "line": format!(
                            "fell {:.1}% today, {:.1} times its usual daily swing",
                            -change * 100.0,
                            -sds
                        ),
                    }),
                )
            })
        })
        .collect();
    bad.sort_by(|a, b| a.0.total_cmp(&b.0));
    let going_bad: Vec<Value> = bad.into_iter().take(MAX_LINES).map(|(_, v)| v).collect();

    // Jumpy holdings not already in "do now" (those are already being trimmed there).
    let in_do_now: Vec<String> = act.iter().take(MAX_LINES).map(|i| name(i)).collect();
    let mut jumpy: Vec<&Value> = ok
        .iter()
        .copied()
        .filter(|i| ratio(i) >= JUMPY && !in_do_now.contains(&name(i)))
        .collect();
    jumpy.sort_by(|a, b| ratio(b).total_cmp(&ratio(a)));
    let flagged: Vec<Value> = jumpy
        .iter()
        .take(MAX_LINES)
        .map(|i| {
            serde_json::json!({
                "name": name(i),
                "line": format!("is swinging {:.1} times its usual amount, so hold less of it", ratio(i)),
            })
        })
        .collect();

    let calls: Vec<Value> = ok
        .iter()
        .filter(|i| matches!(i["advice"]["stance"].as_str(), Some("buy" | "sell")) && i["advice"]["basis"] == "proven")
        .map(|i| serde_json::json!({ "name": name(i), "call": s(&i["advice"], "stance"), "line": s(&i["advice"], "headline") }))
        .collect();

    let leans: Vec<String> = desk["leans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["lean"].as_i64().unwrap_or(0) != 0)
        .map(|l| {
            format!(
                "{} {}",
                s(l, "label"),
                if l["lean"].as_i64().unwrap_or(0) > 0 {
                    "up"
                } else {
                    "down"
                }
            )
        })
        .collect();
    let headlines: Vec<String> = desk["stories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|st| st["impacts"].as_array().is_some_and(|a| !a.is_empty()))
        .take(5)
        .map(|st| s(st, "title"))
        .collect();

    serde_json::json!({
        "do_now": do_now,
        "invest_in": invest_in,
        "index_instead_count": index_instead,
        "going_bad": going_bad,
        "flagged": flagged,
        "tested_calls": calls,
        "short_term": gm_domain::analysis::plain::SHORT_TERM_EVIDENCE,
        "news_leans": leans,
        "news_headlines": headlines,
        "news_reader": desk["reader"],
    })
}

/// The brief written from the facts by rule: always available, every line traceable.
pub fn template(f: &Value) -> Vec<Section> {
    let list = |k: &str| f[k].as_array().cloned().unwrap_or_default();
    let mut do_now: Vec<String> = list("do_now")
        .iter()
        .map(|d| format!("{}: {}", s(d, "name"), s(d, "line")))
        .collect();
    if do_now.is_empty() {
        do_now.push("Nothing to change today: no holding is swinging above its usual level and no tested call is active.".into());
    }
    let mut invest: Vec<String> = list("invest_in")
        .iter()
        .map(|d| format!("{}: {}. {}", s(d, "name"), s(d, "verdict"), s(d, "why")))
        .collect();
    let n = f["index_instead_count"].as_u64().unwrap_or(0);
    if n > 0 {
        invest.push(format!("{n} single stocks: a NIFTY 50 or S&P 500 index fund has the same expected growth with less risk."));
    }
    let mut flagged: Vec<String> = list("going_bad")
        .iter()
        .map(|d| format!("{} {}.", s(d, "name"), s(d, "line")))
        .collect();
    flagged.extend(
        list("flagged")
            .iter()
            .map(|d| format!("{} {}.", s(d, "name"), s(d, "line"))),
    );
    flagged.extend(
        list("news_leans")
            .iter()
            .map(|l| format!("Today's news leans: {}.", l.as_str().unwrap_or_default())),
    );
    if flagged.is_empty() {
        flagged.push("Nothing unusual today.".into());
    }
    let calls: Vec<String> = list("tested_calls")
        .iter()
        .map(|d| format!("{} {}: {}", s(d, "call"), s(d, "name"), s(d, "line")))
        .collect();
    let now = if calls.is_empty() {
        vec!["Nothing qualifies: no rule that has proven itself on that stock's own past is active today. Trims above are the risk actions.".into()]
    } else {
        calls
    };
    vec![
        Section {
            title: "Do now".into(),
            bullets: do_now,
        },
        Section {
            title: "Invest in (3+ years)".into(),
            bullets: invest,
        },
        Section {
            title: "Flagged / going bad".into(),
            bullets: flagged,
        },
        Section {
            title: "Get out now / get in now".into(),
            bullets: now,
        },
        Section {
            title: "1 day · 3 days · day trading".into(),
            bullets: vec![s(f, "short_term")],
        },
    ]
}

const PROMPT: &str = "You write the morning brief for a small internal investment team in India. \
Use ONLY the facts in the JSON you are given. Write five sections with exactly these titles: \
\"Do now\", \"Invest in (3+ years)\", \"Flagged / going bad\", \"Get out now / get in now\", \
\"1 day · 3 days · day trading\". Each section has at most 6 short bullets in plain English. \
Each fact carries a ready-written \"line\": keep its meaning, never merge two items' advice, \
and never show a field name. If tested_calls is empty, say that no tested signal says get out \
or get in today and that the trims are the risk actions. Copy numbers exactly as written in the \
facts; never add a number, a price target or a forecast \
that is not in the facts. If a list is empty, say so in one bullet. Reply with JSON only: \
{\"sections\":[{\"title\":\"...\",\"bullets\":[\"...\"]}]}";

/// Every number in the model's bullets must appear in the facts (or the fixed section titles).
pub fn numbers_traceable(sections: &[Section], facts_text: &str) -> bool {
    // Names with numbers in them are not figures: NIFTY 50, S&P 500, Nasdaq 100.
    let allowed = format!("{facts_text} 1 day 3 days 3+ years 6 NIFTY 50 S&P 500 Nasdaq 100");
    sections
        .iter()
        .flat_map(|s| &s.bullets)
        .all(|b| numbers(b).iter().all(|n| allowed.contains(n.as_str())))
}

/// Runs of digits, with a decimal point or thousands comma between digits ("27,000", "1.6").
fn numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (matches!(chars[i], '.' | ',')
                        && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

fn parse_sections(reply: &str) -> Option<Vec<Section>> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    #[derive(Deserialize)]
    struct Reply {
        sections: Vec<Section>,
    }
    let parsed: Reply = serde_json::from_str(&reply[start..=end]).ok()?;
    (parsed.sections.len() == 5 && parsed.sections.iter().all(|s| !s.bullets.is_empty()))
        .then_some(parsed.sections)
}

pub async fn brief(State(state): State<AppState>) -> Json<Value> {
    if let Some((at, cached)) = state.brief.lock().expect("brief lock").clone()
        && at.elapsed() < TTL
    {
        return Json(cached);
    }
    let items = crate::today::scan(&state, crate::today::universe()).await;
    let Json(desk) = crate::desk(State(state.clone())).await;
    let f = facts(&items, &desk);
    let facts_text = f.to_string();
    let mut writer = "template".to_string();
    let mut note = None;
    let mut sections = template(&f);
    if let Some(llm) = &state.llm {
        match llm.complete(PROMPT, &facts_text).await {
            Ok(reply) => {
                match parse_sections(&reply) {
                    Some(s) if numbers_traceable(&s, &facts_text) => {
                        sections = s;
                        writer = llm.model().to_string();
                    }
                    Some(_) => note = Some(
                        "the model used a number that isn't in the facts, so the template wrote it"
                            .to_string(),
                    ),
                    None => {
                        note = Some(
                            "the model's reply wasn't usable, so the template wrote it".to_string(),
                        )
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "brief: model unavailable; template used");
                note = Some("the model was unavailable, so the template wrote it".to_string());
            }
        }
    }
    let body = serde_json::json!({
        "as_of": chrono::Utc::now(),
        "writer": writer,
        "note": note,
        "sections": sections,
        "facts": f,
    });
    *state.brief.lock().expect("brief lock") = Some((Instant::now(), body.clone()));
    Json(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(symbol: &str, action: &str, change: f64, ratio: f64) -> Value {
        serde_json::json!({
            "symbol": symbol, "name": symbol, "action": action, "headline": format!("{symbol} line"),
            "change_pct": change, "vol_annual": 0.16, "vol_ratio": ratio, "single_stock": action == "index_instead",
            "answers": [{ "question": "q", "verdict": if action == "sip" { "Buy monthly (SIP) for 3+ years" } else { "x" }, "why": "w 62" }],
            "advice": { "stance": "hold", "basis": "no_evidence", "headline": "" },
        })
    }

    #[test]
    fn facts_pick_the_actions_and_the_bad_days() {
        let items = vec![
            item("A", "trim", -0.5, 1.6),
            item("B", "index_instead", -3.0, 1.0), // −3% on a 1% daily swing: going bad
            item("C", "sip", 0.2, 0.9),
        ];
        let desk = serde_json::json!({ "leans": [{ "label": "Gold", "lean": 1 }], "stories": [], "reader": "built-in" });
        let f = facts(&items, &desk);
        assert_eq!(f["do_now"][0]["name"], "A");
        assert_eq!(f["going_bad"][0]["name"], "B");
        // A is already in "do now": flagged doesn't repeat it.
        assert!(f["flagged"].as_array().unwrap().is_empty());
        assert_eq!(f["invest_in"][0]["name"], "C");
        assert_eq!(f["index_instead_count"], 1);
        let t = template(&f);
        assert_eq!(t.len(), 5);
        assert!(t[3].bullets[0].starts_with("Nothing qualifies"));
        assert!(
            numbers_traceable(&t, &f.to_string()),
            "the template only uses the facts' numbers"
        );
    }

    #[test]
    fn an_invented_number_is_caught() {
        let facts = r#"{"do_now":[{"line":"trim to 70%"}]}"#;
        let ok = vec![Section {
            title: "Do now".into(),
            bullets: vec!["Trim to 70%.".into()],
        }];
        let bad = vec![Section {
            title: "Do now".into(),
            bullets: vec!["NIFTY to 27,000 by Friday.".into()],
        }];
        assert!(numbers_traceable(&ok, facts));
        assert!(!numbers_traceable(&bad, facts));
        assert!(parse_sections("```json\n{\"sections\":[]}\n```").is_none());
    }
}
