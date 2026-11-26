//! GDELT DOC 2.0 news search: recent English-language articles that mention a company. Free,
//! keyless, about 15 minutes behind. One of several sources (see `news.rs`); it rate-limits
//! shared cloud IPs such as Railway's. Nothing is stored.
//!
//! GDELT allows one request every 5 seconds per IP and answers faster callers with a plain-text
//! notice (HTTP 200), so requests are paced through one lock and results cached for 15 minutes.
//! ponytail: in-process pacing and cache, right for one instance. Several instances would share
//! the IP budget without knowing it; move the cache to Postgres (#80) if we scale out.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ProviderError;

pub const SOURCE: &str = "gdelt-doc-2.0";
const BASE_URL: &str = "https://api.gdeltproject.org";
const MIN_SPACING: Duration = Duration::from_millis(5_500);
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Article {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub source_country: Option<String>,
    /// When GDELT first saw the article.
    pub seen_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct ArticleList {
    articles: Option<Vec<WireArticle>>,
}

#[derive(Deserialize)]
struct WireArticle {
    title: String,
    url: String,
    domain: String,
    sourcecountry: Option<String>,
    seendate: String,
}

fn snippet(body: &str) -> String {
    body.chars().take(200).collect()
}

/// GDELT signals errors (rate limit, bad query) as plain text with status 200.
fn json_or_error(body: &str) -> Result<&str, ProviderError> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        Ok(trimmed)
    } else {
        Err(ProviderError::Unavailable(format!(
            "gdelt: {}",
            snippet(body)
        )))
    }
}

fn parse_seen(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|dt| dt.and_utc())
}

/// Articles newest first. GDELT's documented "no matches" answer is an empty object `{}`; any
/// other object without `articles` is an upstream failure, not "no news".
pub fn parse_articles(body: &str) -> Result<Vec<Article>, ProviderError> {
    let json = json_or_error(body)?;
    if json.trim() == "{}" {
        return Ok(Vec::new());
    }
    let list: ArticleList = serde_json::from_str(json).map_err(|e| {
        ProviderError::Unavailable(format!("malformed gdelt articles ({e}): {}", snippet(body)))
    })?;
    let wire = list.articles.ok_or_else(|| {
        ProviderError::Unavailable(format!("gdelt response has no articles: {}", snippet(body)))
    })?;
    let mut articles: Vec<Article> = wire
        .into_iter()
        .filter_map(|a| {
            Some(Article {
                seen_at: parse_seen(&a.seendate)?,
                title: a.title.trim().to_string(),
                url: a.url,
                domain: a.domain,
                source_country: a.sourcecountry.filter(|c| !c.is_empty()),
            })
        })
        .collect();
    articles.sort_by_key(|a| std::cmp::Reverse(a.seen_at));
    Ok(articles)
}

/// GDELT's query for an instrument: its company phrase, quoted, English only.
/// "Reliance Industries Limited" → `"Reliance Industries" sourcelang:english`.
pub fn news_query(name: &str) -> Option<String> {
    company_phrase(name).map(|phrase| format!("\"{phrase}\" sourcelang:english"))
}

/// An instrument's name without legal suffixes: "Reliance Industries Limited" →
/// "Reliance Industries". None when what is left is too short to search for.
pub fn company_phrase(name: &str) -> Option<String> {
    const SUFFIXES: [&str; 14] = [
        "limited",
        "ltd.",
        "ltd",
        "inc.",
        "inc",
        "corporation",
        "corp.",
        "corp",
        "plc",
        "ag",
        "s.a.",
        "n.v.",
        "co.",
        "usd",
    ];
    let mut words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| w.chars().all(|c| c.is_alphanumeric() || "&.-'".contains(c)))
        .collect();
    while words
        .last()
        .is_some_and(|w| SUFFIXES.contains(&w.to_lowercase().trim_end_matches(',')))
    {
        words.pop();
    }
    let phrase = words.join(" ");
    // Very short phrases match everything (and GDELT rejects them).
    (phrase.len() >= 3).then_some(phrase)
}

type Cache = HashMap<(String, &'static str), (Instant, Arc<String>)>;

#[derive(Clone)]
pub struct GdeltClient {
    http: reqwest::Client,
    base_url: String,
    cache: Arc<Mutex<Cache>>,
    /// Held for the whole request, so at most one call is in flight and calls are spaced.
    last_call: Arc<tokio::sync::Mutex<Option<Instant>>>,
}

impl Default for GdeltClient {
    fn default() -> Self {
        Self::new(BASE_URL)
    }
}

impl GdeltClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("market-intelligence-core (+https://github.com/Chandrashekhar-Hegde/market-intelligence-core)")
                .timeout(Duration::from_secs(20))
                .build()
                .expect("static reqwest config is valid"),
            base_url: base_url.into(),
            cache: Arc::default(),
            last_call: Arc::default(),
        }
    }

    pub async fn articles(&self, query: &str) -> Result<Vec<Article>, ProviderError> {
        parse_articles(
            &self
                .get(
                    query,
                    "artlist",
                    &[
                        ("maxrecords", "20"),
                        ("timespan", "3d"),
                        ("sort", "datedesc"),
                    ],
                )
                .await?,
        )
    }

    async fn get(
        &self,
        query: &str,
        mode: &'static str,
        extra: &[(&str, &str)],
    ) -> Result<Arc<String>, ProviderError> {
        let key = (query.to_string(), mode);
        let cached = |cache: &Mutex<Cache>| {
            cache
                .lock()
                .expect("cache lock")
                .get(&key)
                .filter(|(at, _)| at.elapsed() < CACHE_TTL)
                .map(|(_, body)| Arc::clone(body))
        };
        if let Some(body) = cached(&self.cache) {
            return Ok(body);
        }
        let mut last = self.last_call.lock().await;
        // Another request may have filled the cache while this one waited for the lock.
        if let Some(body) = cached(&self.cache) {
            return Ok(body);
        }
        if let Some(wait) = last.map(|at| MIN_SPACING.saturating_sub(at.elapsed())) {
            tokio::time::sleep(wait).await;
        }
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
        url.set_path("/api/v2/doc/doc");
        url.query_pairs_mut()
            .append_pair("query", query)
            .append_pair("mode", mode)
            .append_pair("format", "json")
            .extend_pairs(extra);
        let result = self.http.get(url).send().await;
        *last = Some(Instant::now());
        let response = result.map_err(|e| ProviderError::Unavailable(format!("gdelt: {e}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("gdelt: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError::Unavailable(format!(
                "gdelt status {status}: {}",
                snippet(&body)
            )));
        }
        // Only good answers are cached; a rate-limit notice must not stick for 15 minutes.
        json_or_error(&body)?;
        let body = Arc::new(body);
        self.cache
            .lock()
            .expect("cache lock")
            .insert(key, (Instant::now(), Arc::clone(&body)));
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded from GDELT on 2026-11-26 (it answers too-fast callers with this text and HTTP 200).
    const RATE_LIMITED: &str = include_str!("../tests/fixtures/gdelt_rate_limited.txt");
    // Constructed in GDELT's documented shape (artlist fields as seen live on 2026-11-26); GDELT
    // was rate-limiting this machine when the fixtures were made. Replace with recordings.
    const ARTICLES: &str = include_str!("../tests/fixtures/gdelt_articles_reliance.json");

    #[test]
    fn parses_articles_newest_first() {
        let articles = parse_articles(ARTICLES).unwrap();
        assert_eq!(articles.len(), 3);
        assert!(articles.windows(2).all(|w| w[0].seen_at >= w[1].seen_at));
        assert_eq!(articles[0].domain, "economictimes.indiatimes.com");
        assert_eq!(
            articles[0].seen_at.to_rfc3339(),
            "2026-11-26T05:30:00+00:00"
        );
    }

    #[test]
    fn queries_drop_legal_suffixes() {
        let q = |n| news_query(n).unwrap();
        assert_eq!(
            q("Reliance Industries Limited"),
            "\"Reliance Industries\" sourcelang:english"
        );
        assert_eq!(q("HDFC Bank Limited"), "\"HDFC Bank\" sourcelang:english");
        assert_eq!(q("Apple Inc."), "\"Apple\" sourcelang:english");
        assert_eq!(q("Bitcoin USD"), "\"Bitcoin\" sourcelang:english");
        assert_eq!(q("NIFTY 50"), "\"NIFTY 50\" sourcelang:english");
        assert!(news_query("AB").is_none());
        assert_eq!(
            company_phrase("Infosys Limited").as_deref(),
            Some("Infosys")
        );
    }

    #[test]
    fn rate_limit_notice_is_an_error_with_its_text() {
        let Err(ProviderError::Unavailable(message)) = parse_articles(RATE_LIMITED) else {
            panic!("a rate-limit notice must be an error");
        };
        assert!(message.contains("one every 5 seconds"), "{message}");
    }

    #[test]
    fn empty_object_is_no_news_but_other_objects_are_errors() {
        assert_eq!(parse_articles("{}").unwrap(), Vec::new());
        assert!(parse_articles("{\"status\":\"error\"}").is_err());
    }
}
