//! News from several free, keyless sources, so one blocked provider never means "no news":
//! - Google News RSS search for the company name (India edition, last 3 days);
//! - The Economic Times Markets and Mint Markets RSS feeds, kept where an item names the company;
//! - GDELT (see `gdelt.rs`), merged in by the API.
//!
//! RSS is read with a few regular expressions rather than an XML library.
//! ponytail: fine for these feeds' flat <item> lists; switch to quick-xml if a feed nests or
//! escapes items in ways the patterns miss (a parser test fails first).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::ProviderError;

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
pub const GOOGLE_NEWS: &str = "Google News";
pub const ET_MARKETS: &str = "Economic Times";
pub const MINT_MARKETS: &str = "Mint";
pub const BUSINESS_STANDARD: &str = "Business Standard";
pub const BUSINESSLINE: &str = "BusinessLine";
pub const CNBC: &str = "CNBC";
pub const BLOOMBERG: &str = "Bloomberg";
const GOOGLE_BASE: &str = "https://news.google.com";
/// Publisher feeds: (name, stub path, live URL). Headlines and links only.
/// Moneycontrol's RSS was dropped on 27 Nov 2026: it served one item from 2017.
const FEEDS: [(&str, &str, &str); 6] = [
    (
        ET_MARKETS,
        "et.xml",
        "https://economictimes.indiatimes.com/markets/rssfeeds/1977021501.cms",
    ),
    (
        MINT_MARKETS,
        "mint.xml",
        "https://www.livemint.com/rss/markets",
    ),
    (
        BUSINESS_STANDARD,
        "bs.xml",
        "https://www.business-standard.com/rss/markets-106.rss",
    ),
    (
        BUSINESSLINE,
        "businessline.xml",
        "https://www.thehindubusinessline.com/markets/feeder/default.rss",
    ),
    (
        CNBC,
        "cnbc.xml",
        "https://www.cnbc.com/id/10001147/device/rss/rss.html",
    ),
    (
        BLOOMBERG,
        "bloomberg.xml",
        "https://feeds.bloomberg.com/markets/news.rss",
    ),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub title: String,
    pub url: String,
    /// Who published it (Google News names the outlet; feeds are their own publisher).
    pub publisher: String,
    /// Which of our sources found it.
    pub source: String,
    pub seen_at: DateTime<Utc>,
    pub summary: Option<String>,
}

fn re(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static pattern"))
}

fn tag<'a>(item: &'a str, name: &str) -> Option<&'a str> {
    let open = item.find(&format!("<{name}"))?;
    let start = open + item[open..].find('>')? + 1;
    let end = start + item[start..].find(&format!("</{name}>"))?;
    Some(&item[start..end])
}

/// Text of an RSS field: CDATA unwrapped, HTML tags dropped, entities decoded, spaces tidied.
fn text(raw: &str) -> String {
    static TAGS: OnceLock<Regex> = OnceLock::new();
    static NUMERIC: OnceLock<Regex> = OnceLock::new();
    let s = raw.trim();
    let s = s
        .strip_prefix("<![CDATA[")
        .and_then(|x| x.strip_suffix("]]>"))
        .unwrap_or(s);
    // Entities first: Google escapes the HTML inside <description>.
    let decoded = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    let decoded = re(r"&#(\d+);", &NUMERIC).replace_all(&decoded, |c: &regex::Captures| {
        c[1].parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map_or(String::new(), String::from)
    });
    let no_tags = re(r"<[^>]*>", &TAGS).replace_all(&decoded, " ");
    no_tags
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse an RSS 2.0 feed. `source` names which of our sources this is.
pub fn parse_rss(body: &str, source: &str) -> Result<Vec<Story>, ProviderError> {
    let snippet = || body.chars().take(200).collect::<String>();
    if !body.contains("<rss") && !body.contains("<channel") {
        return Err(ProviderError::Unavailable(format!(
            "{source}: not an RSS feed: {}",
            snippet()
        )));
    }
    let mut stories = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<item>").or_else(|| rest.find("<item ")) {
        let Some(len) = rest[start..].find("</item>") else {
            break;
        };
        let item = &rest[start..start + len];
        rest = &rest[start + len + 7..];
        let (Some(title), Some(link), Some(date)) =
            (tag(item, "title"), tag(item, "link"), tag(item, "pubDate"))
        else {
            continue;
        };
        let Ok(seen) = DateTime::parse_from_rfc2822(&text(date)) else {
            continue;
        };
        let outlet = tag(item, "source").map(text);
        let mut title = text(title);
        // Google News titles end with " - Outlet".
        if let Some(o) = &outlet
            && let Some(stripped) = title.strip_suffix(&format!(" - {o}"))
        {
            title = stripped.to_string();
        }
        let summary = tag(item, "description")
            .map(text)
            .filter(|s| !s.is_empty() && *s != title);
        stories.push(Story {
            title,
            url: text(link),
            publisher: outlet.unwrap_or_else(|| source.to_string()),
            source: source.to_string(),
            seen_at: seen.with_timezone(&Utc),
            summary,
        });
    }
    Ok(stories)
}

/// Whether a story names the company: the full phrase, case-insensitive, in title or summary.
pub fn mentions(story: &Story, phrase: &str) -> bool {
    let p = phrase.to_lowercase();
    story.title.to_lowercase().contains(&p)
        || story
            .summary
            .as_deref()
            .is_some_and(|s| s.to_lowercase().contains(&p))
}

/// A key that treats "Reliance: Q2 profit up" and "Reliance Q2 Profit Up" as one story.
pub fn dedupe_key(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .take(60)
        .collect()
}

type Cache = HashMap<String, (Instant, Arc<Vec<Story>>)>;

#[derive(Clone)]
pub struct NewsClient {
    http: reqwest::Client,
    google_base: String,
    feeds: Arc<[(&'static str, String)]>,
    cache: Arc<Mutex<Cache>>,
}

impl Default for NewsClient {
    fn default() -> Self {
        Self::new(
            GOOGLE_BASE,
            FEEDS
                .iter()
                .map(|(name, _, url)| (*name, (*url).to_string()))
                .collect(),
        )
    }
}

impl NewsClient {
    pub fn new(google_base: &str, feeds: Vec<(&'static str, String)>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (compatible; market-intelligence-core)")
                .timeout(Duration::from_secs(5))
                .build()
                .expect("static reqwest config is valid"),
            google_base: google_base.trim_end_matches('/').to_string(),
            feeds: Arc::from(feeds),
            cache: Arc::default(),
        }
    }

    /// Point every source at one stub server: /rss/search, /et.xml, /mint.xml, /bs.xml, ...
    pub fn stub(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self::new(
            base,
            FEEDS
                .iter()
                .map(|(name, path, _)| (*name, format!("{base}/{path}")))
                .collect(),
        )
    }

    /// Google News stories from the last 3 days that match `phrase` exactly.
    pub async fn google(&self, phrase: &str) -> Result<Arc<Vec<Story>>, ProviderError> {
        self.google_query(&format!("\"{phrase}\" when:3d")).await
    }

    /// Google News (India edition) for a raw query, e.g. `RBI OR rupee when:1d`.
    pub async fn google_query(&self, query: &str) -> Result<Arc<Vec<Story>>, ProviderError> {
        let mut url = reqwest::Url::parse(&self.google_base)
            .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
        url.set_path("/rss/search");
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("hl", "en-IN")
            .append_pair("gl", "IN")
            .append_pair("ceid", "IN:en");
        self.cached(format!("google:{query}"), url.as_str(), GOOGLE_NEWS)
            .await
    }

    /// Every publisher feed, each fetched once per cache period, each result
    /// reported on its own so one failure doesn't hide the others.
    pub async fn feeds(&self) -> Vec<(&'static str, Result<Arc<Vec<Story>>, ProviderError>)> {
        let calls = self.feeds.iter().map(|(name, url)| async move {
            (*name, self.cached(format!("feed:{name}"), url, name).await)
        });
        futures_util::future::join_all(calls).await
    }

    async fn cached(
        &self,
        key: String,
        url: &str,
        source: &str,
    ) -> Result<Arc<Vec<Story>>, ProviderError> {
        if let Some((at, stories)) = self.cache.lock().expect("cache lock").get(&key)
            && at.elapsed() < CACHE_TTL
        {
            return Ok(Arc::clone(stories));
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("{source}: {e}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("{source}: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError::Unavailable(format!(
                "{source}: status {status}"
            )));
        }
        let stories = Arc::new(parse_rss(&body, source)?);
        self.cache
            .lock()
            .expect("cache lock")
            .insert(key, (Instant::now(), Arc::clone(&stories)));
        Ok(stories)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Datelike;

    use super::*;

    // Recorded 2026-11-27.
    const GOOGLE: &str = include_str!("../tests/fixtures/googlenews_reliance.xml");
    const ET: &str = include_str!("../tests/fixtures/et_markets.xml");
    const MINT: &str = include_str!("../tests/fixtures/mint_markets.xml");

    #[test]
    fn google_news_items_carry_outlet_and_clean_titles() {
        let stories = parse_rss(GOOGLE, GOOGLE_NEWS).unwrap();
        assert_eq!(stories.len(), 27);
        let first = &stories[0];
        assert_eq!(first.publisher, "DD News");
        assert!(
            first
                .title
                .starts_with("Reliance and Jio to invest Rs 10 lakh crore"),
            "{}",
            first.title
        );
        assert!(!first.title.ends_with(" - DD News"));
        assert!(
            first
                .url
                .starts_with("https://news.google.com/rss/articles/")
        );
        assert_eq!(first.seen_at.to_rfc3339(), "2026-11-26T09:41:36+00:00");
        assert!(
            stories
                .iter()
                .all(|s| !s.title.contains("&amp;") && !s.title.contains('<'))
        );
    }

    #[test]
    fn publisher_feeds_unwrap_cdata_and_parse_ist_dates() {
        let et = parse_rss(ET, ET_MARKETS).unwrap();
        assert_eq!(et.len(), 50);
        assert!(
            et[0]
                .title
                .starts_with("Ashiana Housing JV buys 10-acre land")
        );
        assert_eq!(et[0].publisher, ET_MARKETS);
        assert_eq!(et[0].seen_at.to_rfc3339(), "2026-11-26T15:51:45+00:00");
        let mint = parse_rss(MINT, MINT_MARKETS).unwrap();
        assert_eq!(mint.len(), 35);
        assert_eq!(
            mint[0].title,
            "Gold drifts lower as Fed rate-hike fears loom"
        );
        assert!(mint[0].url.starts_with("https://www.livemint.com/market/"));
    }

    /// Recorded 2026-11-27. Every feed parses, with dates, clean titles and links.
    #[test]
    fn every_publisher_feed_parses() {
        for (body, name, n) in [
            (
                include_str!("../tests/fixtures/bs_markets.xml"),
                BUSINESS_STANDARD,
                35,
            ),
            (
                include_str!("../tests/fixtures/businessline_markets.xml"),
                BUSINESSLINE,
                60,
            ),
            (include_str!("../tests/fixtures/cnbc_top.xml"), CNBC, 30),
            (
                include_str!("../tests/fixtures/bloomberg_markets.xml"),
                BLOOMBERG,
                20,
            ),
            (
                include_str!("../tests/fixtures/googlenews_topic_macro.xml"),
                GOOGLE_NEWS,
                100,
            ),
        ] {
            let stories = parse_rss(body, name).unwrap();
            assert_eq!(stories.len(), n, "{name}");
            assert!(
                stories.iter().all(|s| !s.title.is_empty()
                    && s.url.starts_with("http")
                    && !s.title.contains("CDATA")
                    && s.seen_at.year() == 2026),
                "{name}: {:?}",
                stories.first()
            );
        }
    }

    #[test]
    fn mentions_and_dedupe() {
        let story = parse_rss(GOOGLE, GOOGLE_NEWS).unwrap().remove(0);
        assert!(mentions(&story, "reliance"));
        assert!(!mentions(&story, "Infosys"));
        assert_eq!(
            dedupe_key("Reliance: Q2 profit up!"),
            dedupe_key("reliance q2 PROFIT up")
        );
    }

    #[test]
    fn non_rss_bodies_are_errors_with_the_body() {
        let Err(ProviderError::Unavailable(m)) = parse_rss("<html>captcha</html>", GOOGLE_NEWS)
        else {
            panic!("an HTML page must be an error");
        };
        assert!(m.contains("captcha"), "{m}");
        assert_eq!(
            parse_rss("<rss><channel></channel></rss>", ET_MARKETS).unwrap(),
            vec![]
        );
    }
}
