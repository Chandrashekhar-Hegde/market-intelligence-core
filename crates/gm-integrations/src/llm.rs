//! Optional: a language model reads headlines instead of the built-in rules.
//!
//! Any OpenAI-compatible chat endpoint works: free tiers such as Groq or Google Gemini (its
//! OpenAI-compatible endpoint), OpenRouter's free models, or a self-hosted Ollama. Claude is
//! planned once a key exists. Configured by `GM_LLM_BASE_URL`, `GM_LLM_MODEL` and (except for
//! Ollama) `GM_LLM_API_KEY`. Each headline is read once and cached; any failure falls back to
//! the built-in reader, so news never depends on the model being up.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use gm_domain::analysis::news_reader::Reading;
use serde::Deserialize;

use crate::ProviderError;

const KINDS: [&str; 14] = [
    "results",
    "order_win",
    "guidance",
    "rating",
    "legal",
    "credit",
    "management",
    "stake",
    "payout",
    "fundraise",
    "merger",
    "approval",
    "price_move",
    "general",
];

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Gap between background requests (free tiers allow about 15 a minute).
    pub pace: Duration,
}

impl LlmConfig {
    /// None unless both `GM_LLM_BASE_URL` and `GM_LLM_MODEL` are set.
    pub fn from_env() -> Option<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Some(Self {
            base_url: get("GM_LLM_BASE_URL")?,
            model: get("GM_LLM_MODEL")?,
            api_key: get("GM_LLM_API_KEY"),
            pace: PACE,
        })
    }
}

#[derive(Clone)]
pub struct LlmReader {
    http: reqwest::Client,
    config: LlmConfig,
    cache: Arc<Mutex<HashMap<String, Reading>>>,
    market_cache: Arc<Mutex<HashMap<String, MarketReading>>>,
    /// Titles queued or being read in the background, so none is sent twice.
    pending: Arc<Mutex<std::collections::HashSet<(bool, String)>>>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[derive(Deserialize)]
struct WireReading {
    kind: String,
    direction: i64,
    materiality: i64,
    reason: String,
}

const PROMPT: &str = "You read stock-market headlines for an Indian retail investor. For each \
headline, in order, return one JSON object: {\"kind\": one of results, order_win, guidance, \
rating, legal, credit, management, stake, payout, fundraise, merger, approval, price_move \
(a report of the share price moving, e.g. \"shares rise 3%\", \"profit booking\"), general; \
\"direction\": 1 if it is good for the company's share price, -1 if bad, 0 if unclear; \
\"materiality\": 3 if this kind of news usually moves the price a lot, 2 somewhat, 1 hardly; \
\"reason\": one short plain-English sentence}. Read negations carefully (a probe being dropped \
is good news). Reply with only a JSON array, nothing else.";

/// Parse the model's reply into one reading per headline, or an error if it doesn't fit.
pub fn parse_reply(
    content: &str,
    expected: usize,
    model: &str,
) -> Result<Vec<Reading>, ProviderError> {
    let bad = |why: &str| {
        ProviderError::Unavailable(format!(
            "llm reply {why}: {}",
            content.chars().take(200).collect::<String>()
        ))
    };
    let start = content.find('[').ok_or_else(|| bad("has no JSON array"))?;
    let end = content.rfind(']').ok_or_else(|| bad("has no JSON array"))?;
    let wire: Vec<WireReading> =
        serde_json::from_str(&content[start..=end]).map_err(|_| bad("is not the expected JSON"))?;
    if wire.len() != expected {
        return Err(bad("has the wrong number of readings"));
    }
    Ok(wire
        .into_iter()
        .map(|w| Reading {
            kind: if KINDS.contains(&w.kind.as_str()) {
                w.kind
            } else {
                "general".into()
            },
            direction: w.direction.clamp(-1, 1) as i8,
            materiality: w.materiality.clamp(1, 3) as u8,
            reason: w.reason.chars().take(240).collect(),
            reader: model.to_string(),
        })
        .collect())
}

/// Headlines sent per request: small enough to keep replies well-formed and under free-tier
/// token limits.
pub const BATCH: usize = 25;

const MARKET_PROMPT: &str = "You read market headlines for an Indian retail investor. For each \
headline, in order, return one JSON object: {\"theme\": one of Rates, Currency, Commodities, \
Flows, AI & tech, Market report, Other; \"impacts\": a list of {\"asset\": one of nifty \
(Indian shares), banks (Indian banks), it (Indian IT exporters), omc (Indian oil marketers), \
gold, silver, usdinr (dollar in rupees; up = weaker rupee), crude, us (US shares), nasdaq (US \
tech); \"direction\": 1 if the news usually pushes it up, -1 down; \"reason\": one short \
plain-English sentence}}. Be strict: list an asset only when the headline reports a new event \
that moves it through a well-known channel (a rate decision, a change in crude or gold prices, \
a currency move, foreign-investor flows, a policy that changes earnings). Give NO impacts for: \
reports that a price or index already moved (\"stock slips\", \"shares rally\", \"futures \
climb\"), forecasts and analyst chatter, speeches without a decision, routine regulatory \
housekeeping, or one company's news. Most headlines have no impacts. Always give the theme, \
even with an empty impacts list. Reply with only a JSON array, nothing else.";

#[derive(Deserialize)]
struct WireMarket {
    #[serde(default)]
    theme: String,
    #[serde(default)]
    impacts: Vec<WireImpact>,
}

#[derive(Deserialize)]
struct WireImpact {
    asset: String,
    direction: i64,
    #[serde(default)]
    reason: String,
}

/// A market-news reading from the model: theme and (asset, direction, reason).
pub type MarketReading = (String, Vec<(String, i8, String)>);

/// The JSON array in a reply, whatever text surrounds it.
fn json_array(content: &str) -> Option<&str> {
    let start = content.find('[')?;
    let end = content.rfind(']')?;
    (end > start).then(|| &content[start..=end])
}

/// Parse a market reply into one reading per headline.
pub fn parse_market_reply(
    content: &str,
    expected: usize,
) -> Result<Vec<MarketReading>, ProviderError> {
    let bad = |why: &str| ProviderError::Unavailable(format!("llm market reply {why}"));
    let wire: Vec<WireMarket> =
        serde_json::from_str(json_array(content).ok_or_else(|| bad("has no JSON array"))?)
            .map_err(|_| bad("is not the expected JSON"))?;
    if wire.len() != expected {
        return Err(bad("has the wrong number of readings"));
    }
    Ok(wire
        .into_iter()
        .map(|w| {
            (
                w.theme,
                w.impacts
                    .into_iter()
                    .map(|i| (i.asset, i.direction.clamp(-1, 1) as i8, i.reason))
                    .collect(),
            )
        })
        .collect())
}

impl LlmReader {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("static reqwest config is valid"),
            config,
            cache: Arc::default(),
            market_cache: Arc::default(),
            pending: Arc::default(),
        }
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// One chat completion. Free tiers answer 429 or 503 under load: one retry after a pause.
    async fn chat(&self, system: &str, titles: &[String]) -> Result<String, ProviderError> {
        let numbered = titles
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}. {t}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let body = serde_json::json!({
            "model": self.config.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": numbered },
            ],
        });
        let mut last_error = String::new();
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
            let mut request = self.http.post(&url).json(&body);
            if let Some(key) = &self.config.api_key {
                request = request.bearer_auth(key);
            }
            let response = request
                .send()
                .await
                .map_err(|e| ProviderError::Unavailable(format!("llm: {e}")))?;
            let status = response.status();
            let text = response
                .text()
                .await
                .map_err(|e| ProviderError::Unavailable(format!("llm: {e}")))?;
            if status.is_success() {
                let parsed: ChatResponse = serde_json::from_str(&text)
                    .map_err(|e| ProviderError::Unavailable(format!("llm reply: {e}")))?;
                return parsed
                    .choices
                    .into_iter()
                    .next()
                    .map(|c| c.message.content)
                    .ok_or_else(|| ProviderError::Unavailable("llm reply has no choices".into()));
            }
            // Gemini wraps errors in an array; others send an object. Keep the message either way.
            last_error = format!("llm status {status}: {}", error_message(&text));
            if !(status.as_u16() == 429 || status.as_u16() == 503) {
                break;
            }
        }
        Err(ProviderError::Unavailable(last_error))
    }

    /// Company readings for `titles`, in order. Cached titles are not sent again.
    pub async fn read(&self, titles: &[String]) -> Result<Vec<Reading>, ProviderError> {
        let missing = missing(titles, &self.cache);
        for chunk in missing.chunks(BATCH) {
            let content = self.chat(PROMPT, chunk).await?;
            let readings = parse_reply(&content, chunk.len(), &self.config.model)?;
            let mut cache = self.cache.lock().expect("cache lock");
            for (title, reading) in chunk.iter().cloned().zip(readings) {
                cache.insert(title, reading);
            }
        }
        collect(titles, &self.cache)
    }

    /// Market readings (theme and asset effects) for `titles`, in order, cached likewise.
    pub async fn read_market(
        &self,
        titles: &[String],
    ) -> Result<Vec<MarketReading>, ProviderError> {
        let missing = missing(titles, &self.market_cache);
        for chunk in missing.chunks(BATCH) {
            let content = self.chat(MARKET_PROMPT, chunk).await?;
            let readings = parse_market_reply(&content, chunk.len())?;
            let mut cache = self.market_cache.lock().expect("cache lock");
            for (title, reading) in chunk.iter().cloned().zip(readings) {
                cache.insert(title, reading);
            }
        }
        collect(titles, &self.market_cache)
    }
}

/// Background requests go out at most this often by default: free tiers allow ~15 a minute.
pub const PACE: Duration = Duration::from_secs(5);

impl LlmReader {
    /// What the model has already read, without asking it anything.
    pub fn cached(&self, titles: &[String]) -> Vec<Option<Reading>> {
        let cache = self.cache.lock().expect("cache lock");
        titles.iter().map(|t| cache.get(t).cloned()).collect()
    }

    pub fn cached_market(&self, titles: &[String]) -> Vec<Option<MarketReading>> {
        let cache = self.market_cache.lock().expect("cache lock");
        titles.iter().map(|t| cache.get(t).cloned()).collect()
    }

    /// Reads the titles not yet read in the background, paced, so no request waits for the
    /// model. `market` picks the market prompt. Failures are logged; the built-in reader covers.
    pub fn read_in_background(&self, titles: &[String], market: bool) {
        let fresh: Vec<String> = {
            let done_company = self.cache.lock().expect("cache lock");
            let done_market = self.market_cache.lock().expect("cache lock");
            let mut pending = self.pending.lock().expect("pending lock");
            titles
                .iter()
                .filter(|t| {
                    let done = if market {
                        done_market.contains_key(*t)
                    } else {
                        done_company.contains_key(*t)
                    };
                    !done && pending.insert((market, (*t).clone()))
                })
                .cloned()
                .collect()
        };
        if fresh.is_empty() {
            return;
        }
        let reader = self.clone();
        tokio::spawn(async move {
            for chunk in fresh.chunks(BATCH) {
                let result = if market {
                    reader.read_market(chunk).await.map(|_| ())
                } else {
                    reader.read(chunk).await.map(|_| ())
                };
                if let Err(error) = result {
                    tracing::warn!(%error, market, "llm reader: batch failed; built-in reader covers it");
                }
                {
                    let mut pending = reader.pending.lock().expect("pending lock");
                    for t in chunk {
                        pending.remove(&(market, t.clone()));
                    }
                }
                tokio::time::sleep(reader.config.pace).await;
            }
        });
    }
}

fn missing<T>(titles: &[String], cache: &Mutex<HashMap<String, T>>) -> Vec<String> {
    let cache = cache.lock().expect("cache lock");
    let mut seen = std::collections::HashSet::new();
    titles
        .iter()
        .filter(|t| !cache.contains_key(*t) && seen.insert(t.as_str()))
        .cloned()
        .collect()
}

fn collect<T: Clone>(
    titles: &[String],
    cache: &Mutex<HashMap<String, T>>,
) -> Result<Vec<T>, ProviderError> {
    let cache = cache.lock().expect("cache lock");
    titles
        .iter()
        .map(|t| {
            cache
                .get(t)
                .cloned()
                .ok_or_else(|| ProviderError::Unavailable("llm reading missing".into()))
        })
        .collect()
}

/// The message in an error body: `{"error":{"message":..}}`, `[{"error":{..}}]` (Gemini), or text.
fn error_message(body: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let error = value.as_ref().map(|v| match v {
        serde_json::Value::Array(items) => items.first().cloned().unwrap_or_default(),
        other => other.clone(),
    });
    error
        .as_ref()
        .and_then(|e| e["error"]["message"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_replies_and_error_bodies() {
        let reply = "```json\n[{\"theme\":\"Commodities\",\"impacts\":[{\"asset\":\"crude\",\"direction\":1,\"reason\":\"Supply risk.\"},{\"asset\":\"omc\",\"direction\":-2}]},{\"theme\":\"Market report\"}]\n```";
        let r = parse_market_reply(reply, 2).unwrap();
        assert_eq!(r[0].0, "Commodities");
        assert_eq!(
            r[0].1,
            vec![
                ("crude".to_string(), 1, "Supply risk.".to_string()),
                ("omc".to_string(), -1, String::new())
            ]
        );
        assert!(r[1].1.is_empty());
        assert!(parse_market_reply(reply, 3).is_err());
        // Gemini sometimes answers {} for a headline with no effects.
        let sparse =
            parse_market_reply("[{}, {\"theme\": \"Rates\", \"impacts\": []}]", 2).unwrap();
        assert_eq!((sparse[0].0.as_str(), sparse[0].1.len()), ("", 0));
        // Gemini sends errors as an array.
        let gemini = r#"[{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}]"#;
        assert_eq!(
            error_message(gemini),
            "This model is currently experiencing high demand."
        );
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
        assert_eq!(error_message("oops"), "oops");
    }

    #[test]
    fn replies_are_validated_and_clamped() {
        let reply = "Here you go:\n[{\"kind\":\"legal\",\"direction\":1,\"materiality\":2,\"reason\":\"Probe dropped.\"},\
                     {\"kind\":\"weird\",\"direction\":-5,\"materiality\":9,\"reason\":\"Bad.\"}]";
        let r = parse_reply(reply, 2, "test-model").unwrap();
        assert_eq!(
            (r[0].kind.as_str(), r[0].direction, r[0].materiality),
            ("legal", 1, 2)
        );
        assert_eq!(
            (r[1].kind.as_str(), r[1].direction, r[1].materiality),
            ("general", -1, 3)
        );
        assert_eq!(r[0].reader, "test-model");
        assert!(parse_reply(reply, 3, "m").is_err(), "count must match");
        assert!(parse_reply("I cannot help with that.", 1, "m").is_err());
    }
}
