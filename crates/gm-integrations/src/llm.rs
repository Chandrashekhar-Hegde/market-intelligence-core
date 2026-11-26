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

const KINDS: [&str; 13] = [
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
    "general",
];

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl LlmConfig {
    /// None unless both `GM_LLM_BASE_URL` and `GM_LLM_MODEL` are set.
    pub fn from_env() -> Option<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Some(Self {
            base_url: get("GM_LLM_BASE_URL")?,
            model: get("GM_LLM_MODEL")?,
            api_key: get("GM_LLM_API_KEY"),
        })
    }
}

#[derive(Clone)]
pub struct LlmReader {
    http: reqwest::Client,
    config: LlmConfig,
    cache: Arc<Mutex<HashMap<String, Reading>>>,
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
rating, legal, credit, management, stake, payout, fundraise, merger, approval, general; \
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

impl LlmReader {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("static reqwest config is valid"),
            config,
            cache: Arc::default(),
        }
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Readings for `titles`, in order. Cached titles are not sent again.
    pub async fn read(&self, titles: &[String]) -> Result<Vec<Reading>, ProviderError> {
        let missing: Vec<String> = {
            let cache = self.cache.lock().expect("cache lock");
            let mut seen = std::collections::HashSet::new();
            titles
                .iter()
                .filter(|t| !cache.contains_key(*t) && seen.insert(t.as_str()))
                .cloned()
                .collect()
        };
        if !missing.is_empty() {
            let numbered = missing
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}. {t}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let url = format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            );
            let mut request = self.http.post(url).json(&serde_json::json!({
                "model": self.config.model,
                "temperature": 0,
                "messages": [
                    { "role": "system", "content": PROMPT },
                    { "role": "user", "content": numbered },
                ],
            }));
            if let Some(key) = &self.config.api_key {
                request = request.bearer_auth(key);
            }
            let response = request
                .send()
                .await
                .map_err(|e| ProviderError::Unavailable(format!("llm: {e}")))?;
            let status = response.status();
            if !status.is_success() {
                return Err(ProviderError::Unavailable(format!("llm status {status}")));
            }
            let body: ChatResponse = response
                .json()
                .await
                .map_err(|e| ProviderError::Unavailable(format!("llm: {e}")))?;
            let content = body
                .choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .ok_or_else(|| ProviderError::Unavailable("llm reply has no choices".into()))?;
            let readings = parse_reply(&content, missing.len(), &self.config.model)?;
            let mut cache = self.cache.lock().expect("cache lock");
            for (title, reading) in missing.into_iter().zip(readings) {
                cache.insert(title, reading);
            }
        }
        let cache = self.cache.lock().expect("cache lock");
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
