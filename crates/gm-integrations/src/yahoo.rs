//! Yahoo Finance chart + search client.
//!
//! ponytail: unofficial public endpoints, free EOD + ~15 min delayed quotes. Fine for a
//! private beta; swap for a licensed feed if it rate-limits, blocks cloud IPs, or goes paid.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDate, Utc};
use gm_domain::PriceBar;
use serde::{Deserialize, Serialize};

use crate::ProviderError;

pub const SOURCE: &str = "yahoo-chart-v8";
const BASE_URL: &str = "https://query1.finance.yahoo.com";
const USER_AGENT: &str = "Mozilla/5.0 (compatible; market-intelligence-core)";
/// Ranges Yahoo accepts for daily bars. Anything else is rejected before it reaches the network.
pub const VALID_RANGES: [&str; 9] = ["5d", "1mo", "3mo", "6mo", "1y", "2y", "5y", "10y", "max"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentMeta {
    pub symbol: String,
    pub name: Option<String>,
    pub currency: Option<String>,
    pub exchange: Option<String>,
    pub instrument_type: Option<String>,
    pub timezone: Option<String>,
    /// Last traded price (delayed).
    pub last_price: Option<f64>,
    pub last_trade_at: Option<DateTime<Utc>>,
    /// Change and % change vs the previous session's close, as Yahoo reports them.
    pub change: Option<f64>,
    pub change_pct: Option<f64>,
    pub day_volume: Option<i64>,
    /// Regular session in progress at fetch time. Yahoo keeps the previous session's window on
    /// exchange holidays, so this is also false on holidays without a calendar of our own.
    pub market_open: bool,
    /// TradingView's name for this listing, when known (for the embedded live chart).
    pub tradingview_symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorporateAction {
    Dividend {
        date: NaiveDate,
        amount: f64,
    },
    Split {
        date: NaiveDate,
        numerator: f64,
        denominator: f64,
    },
}

impl CorporateAction {
    pub fn date(&self) -> NaiveDate {
        match self {
            Self::Dividend { date, .. } | Self::Split { date, .. } => *date,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartData {
    pub meta: InstrumentMeta,
    /// Completed daily bars, oldest first. `close` is split-adjusted; `adj_close` is split+dividend adjusted.
    pub bars: Vec<PriceBar>,
    pub corporate_actions: Vec<CorporateAction>,
    /// True when today's still-forming bar was removed because the session was open at fetch time.
    pub dropped_partial_bar: bool,
    /// Bad prints (a price that jumps away and straight back) removed before anyone uses the bars.
    #[serde(default)]
    pub bad_prints_removed: Vec<NaiveDate>,
    pub source: String,
    pub fetched_at: DateTime<Utc>,
}

/// One intraday bar (1 or 5 minutes). Display-only: the analysis path stays on daily bars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntradayPoint {
    /// Bar start, Unix seconds (UTC).
    pub t: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntradayData {
    pub meta: InstrumentMeta,
    /// "1m" or "5m".
    pub interval: String,
    /// The price today's change is measured from: last price minus Yahoo's day change. Yahoo's
    /// own `previousClose` can be stale (on 26 Nov 2026 it was two sessions old for NIFTY after a
    /// holiday), and for crypto the change is over 24 hours; this keeps the line and the change
    /// shown next to it consistent. Falls back to `chartPreviousClose`.
    pub previous_close: Option<f64>,
    /// Regular session window of the latest session (Unix seconds), when Yahoo gives one.
    pub session_start: Option<i64>,
    pub session_end: Option<i64>,
    pub points: Vec<IntradayPoint>,
    pub source: String,
    pub fetched_at: DateTime<Utc>,
}

/// Intraday ranges and the bar size used for each.
pub const INTRADAY_RANGES: [(&str, &str); 2] = [("1d", "1m"), ("5d", "5m")];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub symbol: String,
    pub name: String,
    /// Display name of the exchange (NSE, BSE, NASDAQ, Tokyo, …).
    pub exchange: String,
    pub country: String,
    pub quote_type: String,
}

// ---- wire format -------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChartEnvelope {
    chart: ChartBody,
}

#[derive(Deserialize)]
struct ChartBody {
    result: Option<Vec<ChartResult>>,
    error: Option<YahooError>,
}

#[derive(Deserialize)]
struct YahooError {
    code: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartResult {
    meta: WireMeta,
    #[serde(default)]
    timestamp: Vec<i64>,
    indicators: Indicators,
    events: Option<WireEvents>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMeta {
    symbol: String,
    currency: Option<String>,
    full_exchange_name: Option<String>,
    instrument_type: Option<String>,
    exchange_timezone_name: Option<String>,
    long_name: Option<String>,
    short_name: Option<String>,
    regular_market_price: Option<f64>,
    regular_market_time: Option<i64>,
    chart_previous_close: Option<f64>,
    fullday_change: Option<f64>,
    regular_market_change_percent: Option<f64>,
    regular_market_volume: Option<i64>,
    #[serde(default)]
    gmtoffset: i64,
    current_trading_period: Option<TradingPeriods>,
}

#[derive(Deserialize)]
struct TradingPeriods {
    regular: Option<TradingPeriod>,
}

#[derive(Deserialize)]
struct TradingPeriod {
    start: i64,
    end: i64,
}

#[derive(Deserialize)]
struct Indicators {
    quote: Vec<WireQuote>,
    adjclose: Option<Vec<WireAdjClose>>,
}

#[derive(Deserialize)]
struct WireQuote {
    #[serde(default)]
    open: Vec<Option<f64>>,
    #[serde(default)]
    high: Vec<Option<f64>>,
    #[serde(default)]
    low: Vec<Option<f64>>,
    #[serde(default)]
    close: Vec<Option<f64>>,
    #[serde(default)]
    volume: Vec<Option<i64>>,
}

#[derive(Deserialize)]
struct WireAdjClose {
    #[serde(default)]
    adjclose: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct WireEvents {
    #[serde(default)]
    dividends: BTreeMap<String, WireDividend>,
    #[serde(default)]
    splits: BTreeMap<String, WireSplit>,
}

#[derive(Deserialize)]
struct WireDividend {
    amount: f64,
    date: i64,
}

#[derive(Deserialize)]
struct WireSplit {
    date: i64,
    numerator: f64,
    denominator: f64,
}

#[derive(Deserialize)]
struct SearchEnvelope {
    // No serde default: a body without `quotes` (an error object, a block page) is an upstream
    // failure, not "no matches". Defaulting it hid exactly that in production (Nov 2026).
    quotes: Option<Vec<WireSearchQuote>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSearchQuote {
    symbol: Option<String>,
    shortname: Option<String>,
    longname: Option<String>,
    exchange: Option<String>,
    exch_disp: Option<String>,
    quote_type: Option<String>,
}

// ---- parsing (pure, tested against recorded responses) -----------------------------------

fn local_date(ts: i64, gmtoffset: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp(ts + gmtoffset, 0).map(|dt| dt.date_naive())
}

/// Parse a v8 chart response. `now` decides whether the last bar is a still-forming session bar.
fn chart_result(body: &str) -> Result<ChartResult, ProviderError> {
    let envelope: ChartEnvelope = serde_json::from_str(body).map_err(|e| {
        let snippet = body.chars().take(200).collect::<String>();
        ProviderError::Unavailable(format!("malformed chart response ({e}): {snippet}"))
    })?;
    if let Some(error) = envelope.chart.error {
        let code = error.code.unwrap_or_default();
        let description = error.description.unwrap_or_default();
        return Err(if code == "Not Found" {
            ProviderError::SymbolNotFound(description)
        } else {
            ProviderError::Unavailable(format!("{code}: {description}"))
        });
    }
    envelope
        .chart
        .result
        .and_then(|mut results| results.pop())
        .ok_or_else(|| ProviderError::Unavailable("empty chart result".to_string()))
}

fn instrument_meta(meta: &WireMeta, market_open: bool) -> InstrumentMeta {
    InstrumentMeta {
        symbol: meta.symbol.clone(),
        name: meta.long_name.clone().or_else(|| meta.short_name.clone()),
        currency: meta.currency.clone(),
        exchange: meta.full_exchange_name.clone(),
        instrument_type: meta.instrument_type.clone(),
        timezone: meta.exchange_timezone_name.clone(),
        last_price: meta.regular_market_price,
        last_trade_at: meta
            .regular_market_time
            .and_then(|ts| DateTime::from_timestamp(ts, 0)),
        change: meta.fullday_change,
        change_pct: meta.regular_market_change_percent,
        day_volume: meta.regular_market_volume,
        market_open,
        tradingview_symbol: crate::markets::tradingview_symbol(&meta.symbol),
    }
}

fn regular_session(meta: &WireMeta) -> Option<&TradingPeriod> {
    meta.current_trading_period
        .as_ref()
        .and_then(|periods| periods.regular.as_ref())
}

/// Parse a v8 chart response at an intraday interval. Rows with any missing price are gaps in
/// trading (Yahoo emits all-null minutes), not bars.
pub fn parse_intraday(
    body: &str,
    interval: &str,
    now: DateTime<Utc>,
) -> Result<IntradayData, ProviderError> {
    let result = chart_result(body)?;
    let quote = result
        .indicators
        .quote
        .first()
        .ok_or_else(|| ProviderError::Unavailable("chart has no quote block".to_string()))?;
    let at = |v: &[Option<f64>], i: usize| v.get(i).copied().flatten();
    let points = result
        .timestamp
        .iter()
        .enumerate()
        .filter_map(|(i, &t)| {
            Some(IntradayPoint {
                t,
                open: at(&quote.open, i)?,
                high: at(&quote.high, i)?,
                low: at(&quote.low, i)?,
                close: at(&quote.close, i)?,
                volume: quote.volume.get(i).copied().flatten().unwrap_or(0),
            })
        })
        .collect();
    let session = regular_session(&result.meta);
    let now_ts = now.timestamp();
    let market_open = session.is_some_and(|s| now_ts >= s.start && now_ts < s.end);
    Ok(IntradayData {
        meta: instrument_meta(&result.meta, market_open),
        interval: interval.to_string(),
        previous_close: match (result.meta.regular_market_price, result.meta.fullday_change) {
            (Some(price), Some(change)) => Some(price - change),
            _ => result.meta.chart_previous_close,
        },
        session_start: session.map(|s| s.start),
        session_end: session.map(|s| s.end),
        points,
        source: SOURCE.to_string(),
        fetched_at: now,
    })
}

pub fn parse_chart(body: &str, now: DateTime<Utc>) -> Result<ChartData, ProviderError> {
    let result = chart_result(body)?;
    let meta = &result.meta;
    let offset = meta.gmtoffset;
    let quote = result
        .indicators
        .quote
        .first()
        .ok_or_else(|| ProviderError::Unavailable("chart has no quote block".to_string()))?;
    let adj = result
        .indicators
        .adjclose
        .as_ref()
        .and_then(|blocks| blocks.first())
        .map(|block| block.adjclose.as_slice())
        .unwrap_or(&[]);
    let at = |v: &[Option<f64>], i: usize| v.get(i).copied().flatten();

    // BTreeMap keyed by date: sorts, and Yahoo's occasional duplicate last row keeps the later one.
    let mut by_date = BTreeMap::new();
    for (i, ts) in result.timestamp.iter().enumerate() {
        let (Some(date), Some(open), Some(high), Some(low), Some(close)) = (
            local_date(*ts, offset),
            at(&quote.open, i),
            at(&quote.high, i),
            at(&quote.low, i),
            at(&quote.close, i),
        ) else {
            continue; // Yahoo emits all-null rows for non-trading days; they are not bars.
        };
        by_date.insert(
            date,
            PriceBar {
                symbol: meta.symbol.clone(),
                date,
                open,
                high,
                low,
                close,
                adj_close: at(adj, i),
                volume: quote.volume.get(i).copied().flatten().unwrap_or(0),
                source: SOURCE.to_string(),
            },
        );
    }
    let mut bars = by_date.into_values().collect::<Vec<_>>();
    let bad_prints_removed = gm_domain::quality::bad_prints(&bars);
    bars.retain(|b| !bad_prints_removed.contains(&b.date));

    let session = regular_session(meta);
    let now_ts = now.timestamp();
    let market_open = session.is_some_and(|s| now_ts >= s.start && now_ts < s.end);
    let mut dropped_partial_bar = false;
    if let Some(session) = session
        && now_ts >= session.start
        && now_ts < session.end
        && bars.last().map(|bar| bar.date) == local_date(session.start, offset)
    {
        bars.pop();
        dropped_partial_bar = true;
    }

    let mut corporate_actions = Vec::new();
    if let Some(events) = &result.events {
        for dividend in events.dividends.values() {
            if let Some(date) = local_date(dividend.date, offset) {
                corporate_actions.push(CorporateAction::Dividend {
                    date,
                    amount: dividend.amount,
                });
            }
        }
        for split in events.splits.values() {
            if let Some(date) = local_date(split.date, offset) {
                corporate_actions.push(CorporateAction::Split {
                    date,
                    numerator: split.numerator,
                    denominator: split.denominator,
                });
            }
        }
    }
    corporate_actions.sort_by_key(CorporateAction::date);

    Ok(ChartData {
        meta: instrument_meta(meta, market_open),
        bars,
        corporate_actions,
        dropped_partial_bar,
        bad_prints_removed,
        source: SOURCE.to_string(),
        fetched_at: now,
    })
}

/// Instrument types the analysis engine can work with (any exchange). Options are excluded:
/// their prices are driven by the underlying and expiry, not a return process of their own.
const ANALYSABLE_TYPES: [&str; 7] = [
    "EQUITY",
    "ETF",
    "INDEX",
    "MUTUALFUND",
    "CRYPTOCURRENCY",
    "CURRENCY",
    "FUTURE",
];

/// Parse a search response: every exchange, analysable instrument types only.
pub fn parse_search(body: &str) -> Result<Vec<SearchHit>, ProviderError> {
    let snippet = || body.chars().take(200).collect::<String>();
    let envelope: SearchEnvelope = serde_json::from_str(body).map_err(|e| {
        ProviderError::Unavailable(format!("malformed search response ({e}): {}", snippet()))
    })?;
    let quotes = envelope.quotes.ok_or_else(|| {
        ProviderError::Unavailable(format!("search response has no quotes: {}", snippet()))
    })?;
    let raw_count = quotes.len();
    let hits = quotes
        .into_iter()
        .filter(|q| {
            q.quote_type
                .as_deref()
                .is_some_and(|t| ANALYSABLE_TYPES.contains(&t))
        })
        .filter_map(|q| {
            let symbol = q.symbol?;
            let (exchange, country) =
                crate::markets::listing_market(&symbol, q.exchange.as_deref());
            let display = if exchange == "Other" {
                q.exch_disp.or(q.exchange)?
            } else {
                exchange.to_string()
            };
            Some(SearchHit {
                name: q.longname.or(q.shortname).unwrap_or_else(|| symbol.clone()),
                symbol,
                exchange: display,
                country: country.to_string(),
                quote_type: q.quote_type.unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    if raw_count > 0 && hits.is_empty() {
        tracing::warn!(raw_count, body = %snippet(), "search returned quotes but none were usable");
    }
    Ok(hits)
}

/// Symbols reach a URL path, so they are whitelisted: Yahoo tickers are short and use
/// A-Z 0-9 ^ & . - _ and = (FX `INR=X`, futures `GC=F`).
pub fn validate_symbol(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();
    let ok = (1..=32).contains(&symbol.len())
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "^&.-_=".contains(c));
    if ok {
        Ok(symbol)
    } else {
        Err(ProviderError::InvalidRequest(format!(
            "invalid symbol: {symbol:?}"
        )))
    }
}

pub fn validate_range(range: &str) -> Result<&'static str, ProviderError> {
    VALID_RANGES
        .iter()
        .find(|valid| **valid == range)
        .copied()
        .ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "invalid range {range:?}; expected one of {}",
                VALID_RANGES.join(", ")
            ))
        })
}

// ---- client --------------------------------------------------------------------------------

type ChartCache = HashMap<(String, &'static str), (Instant, Arc<ChartData>)>;
type IntradayCache = HashMap<(String, &'static str), (Instant, Arc<IntradayData>)>;

#[derive(Clone)]
pub struct YahooClient {
    http: reqwest::Client,
    base_url: String,
    // ponytail: unbounded in-process cache, fine for one instance + a private beta.
    // Move to Postgres/Redis when there are multiple instances or the key space grows.
    cache: Arc<Mutex<ChartCache>>,
    intraday_cache: Arc<Mutex<IntradayCache>>,
}

impl Default for YahooClient {
    fn default() -> Self {
        Self::new(BASE_URL)
    }
}

impl YahooClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("static reqwest config is valid");
        Self {
            http,
            base_url: base_url.into(),
            cache: Arc::default(),
            intraday_cache: Arc::default(),
        }
    }

    /// Daily bars + corporate actions. Short ranges are cached briefly (they carry the delayed
    /// quote); long ranges change at most once a day.
    pub async fn chart(&self, symbol: &str, range: &str) -> Result<Arc<ChartData>, ProviderError> {
        let symbol = validate_symbol(symbol)?;
        let range = validate_range(range)?;
        let ttl = Duration::from_secs(if matches!(range, "5d" | "1mo") {
            60
        } else {
            15 * 60
        });
        let key = (symbol.clone(), range);
        if let Some((at, data)) = self.cache.lock().expect("cache lock").get(&key)
            && at.elapsed() < ttl
        {
            return Ok(Arc::clone(data));
        }

        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::InvalidRequest("base url cannot be a base".to_string()))?
            .extend(["v8", "finance", "chart", &symbol]);
        url.query_pairs_mut()
            .append_pair("range", range)
            .append_pair("interval", "1d")
            .append_pair("events", "div,splits");
        let body = self.get_with_retry(url).await?;
        let data = Arc::new(parse_chart(&body, Utc::now())?);
        self.cache
            .lock()
            .expect("cache lock")
            .insert(key, (Instant::now(), Arc::clone(&data)));
        Ok(data)
    }

    /// Today's (or the last session's) 1-minute bars, or 5 days of 5-minute bars. Cached for
    /// 30 s: the live stream fills the gap between refreshes.
    pub async fn intraday(
        &self,
        symbol: &str,
        range: &str,
    ) -> Result<Arc<IntradayData>, ProviderError> {
        let symbol = validate_symbol(symbol)?;
        let (range, interval) = INTRADAY_RANGES
            .iter()
            .find(|(r, _)| *r == range)
            .copied()
            .ok_or_else(|| {
                ProviderError::InvalidRequest(format!(
                    "invalid intraday range {range:?}; expected 1d or 5d"
                ))
            })?;
        let key = (symbol.clone(), range);
        if let Some((at, data)) = self.intraday_cache.lock().expect("cache lock").get(&key)
            && at.elapsed() < Duration::from_secs(30)
        {
            return Ok(Arc::clone(data));
        }
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::InvalidRequest("base url cannot be a base".to_string()))?
            .extend(["v8", "finance", "chart", &symbol]);
        url.query_pairs_mut()
            .append_pair("range", range)
            .append_pair("interval", interval);
        let body = self.get_with_retry(url).await?;
        let data = Arc::new(parse_intraday(&body, interval, Utc::now())?);
        self.intraday_cache
            .lock()
            .expect("cache lock")
            .insert(key, (Instant::now(), Arc::clone(&data)));
        Ok(data)
    }

    /// `region` ("IN", "US", …) steers Yahoo's ranking towards that market's listings.
    pub async fn search(&self, query: &str, region: &str) -> Result<Vec<SearchHit>, ProviderError> {
        let query = query.trim();
        if query.is_empty() || query.len() > 64 {
            return Err(ProviderError::InvalidRequest(
                "query must be 1-64 characters".to_string(),
            ));
        }
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
        url.set_path("/v1/finance/search");
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("quotesCount", "25")
            .append_pair("newsCount", "0")
            .append_pair("region", region)
            .append_pair("lang", if region == "IN" { "en-IN" } else { "en-US" });
        parse_search(&self.get_with_retry(url).await?)
    }

    /// GET with two retries on 429/5xx/transport errors (0.5 s, then 1.5 s backoff).
    async fn get_with_retry(&self, url: reqwest::Url) -> Result<String, ProviderError> {
        let mut last_error = String::new();
        for delay_ms in [0, 500, 1500] {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            match self.http.get(url.clone()).send().await {
                Ok(response) => {
                    let status = response.status();
                    // 404 carries a JSON "Not Found" body that parse_chart turns into SymbolNotFound.
                    if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
                        return response
                            .text()
                            .await
                            .map_err(|e| ProviderError::Unavailable(e.to_string()));
                    }
                    last_error = format!("upstream status {status}");
                    if !(status.is_server_error()
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
                    {
                        break;
                    }
                }
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(ProviderError::Unavailable(format!("yahoo: {last_error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELIANCE: &str = include_str!("../tests/fixtures/yahoo_chart_reliance_1y.json");
    const NOT_FOUND: &str = include_str!("../tests/fixtures/yahoo_chart_not_found.json");
    const SEARCH: &str = include_str!("../tests/fixtures/yahoo_search_nifty.json");

    fn after_close() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    /// Recorded 2026-11-27: GOLDBEES sits at 1/100 of its price on 19–20 Dec 2019, and gold
    /// really did jump 7% and fall 8% on 21–22 Jan 2026 (kept).
    #[test]
    fn bad_prints_are_removed_before_anyone_uses_the_bars() {
        const GOLDBEES: &str = include_str!("../tests/fixtures/yahoo_chart_goldbees_10y.json");
        let data = parse_chart(GOLDBEES, after_close()).unwrap();
        let d = |m: u32, day: u32| NaiveDate::from_ymd_opt(2019, m, day).unwrap();
        assert_eq!(data.bad_prints_removed, vec![d(12, 19), d(12, 20)]);
        assert!(
            data.bars
                .iter()
                .all(|b| !data.bad_prints_removed.contains(&b.date))
        );
        // No day left moves more than 15%: gold doesn't.
        let worst = data
            .bars
            .windows(2)
            .map(|w| (w[1].adjusted_close() / w[0].adjusted_close()).ln().abs())
            .fold(0.0, f64::max);
        assert!(worst < 0.15, "{worst}");
        // Clean series are untouched.
        assert!(
            parse_chart(RELIANCE, after_close())
                .unwrap()
                .bad_prints_removed
                .is_empty()
        );
    }

    #[test]
    fn parses_recorded_chart() {
        let data = parse_chart(RELIANCE, after_close()).unwrap();
        assert_eq!(data.meta.symbol, "RELIANCE.NS");
        assert_eq!(data.meta.currency.as_deref(), Some("INR"));
        assert_eq!(data.bars.len(), 248);
        assert!(!data.dropped_partial_bar);
        assert!(data.bars.windows(2).all(|w| w[0].date < w[1].date));
        assert!(
            data.bars
                .iter()
                .all(|b| b.adj_close.is_some() && b.close > 0.0)
        );
        assert!(data.corporate_actions.contains(&CorporateAction::Dividend {
            date: local_date(1_780_631_100, 19_800).unwrap(),
            amount: 6.0,
        }));
        // Dividend adjustment only scales history before the ex-date.
        let first = &data.bars[0];
        let last = data.bars.last().unwrap();
        assert!(first.adj_close.unwrap() < first.close);
        assert!((last.adj_close.unwrap() - last.close).abs() < 1e-9);
    }

    #[test]
    fn drops_bar_of_open_session() {
        // Inside the session recorded in the fixture's currentTradingPeriod.
        let during = DateTime::from_timestamp(1_795_150_000, 0).unwrap();
        let open = parse_chart(RELIANCE, during).unwrap();
        assert!(open.dropped_partial_bar);
        assert!(open.meta.market_open);
        assert!(
            !parse_chart(RELIANCE, after_close())
                .unwrap()
                .meta
                .market_open
        );
        assert_eq!(open.bars.len(), 247);
    }

    const RELIANCE_1M: &str = include_str!("../tests/fixtures/yahoo_chart_reliance_1d_1m.json");
    const BTC_1M: &str = include_str!("../tests/fixtures/yahoo_chart_btc_1d_1m.json");

    #[test]
    fn parses_recorded_intraday_and_skips_null_minutes() {
        let data = parse_intraday(RELIANCE_1M, "1m", after_close()).unwrap();
        assert_eq!(data.meta.symbol, "RELIANCE.NS");
        assert_eq!(data.interval, "1m");
        // 375 minutes in the NSE session, 13 of them all-null in the recording.
        assert_eq!(data.points.len(), 362);
        assert_eq!(data.points[0].t, 1_795_664_700);
        assert_eq!(data.session_start, Some(1_795_664_700));
        assert_eq!(data.session_end, Some(1_795_687_200));
        assert!((data.previous_close.unwrap() - 1373.9).abs() < 1e-9);
        assert_eq!(data.meta.change, Some(22.3));
        assert_eq!(data.meta.change_pct, Some(1.623));
        assert!(data.meta.day_volume.unwrap() > 0);
        assert!(data.points.windows(2).all(|w| w[0].t < w[1].t));
        assert!(
            data.points
                .iter()
                .all(|p| p.low <= p.close && p.close <= p.high)
        );
        assert!(!data.meta.market_open);
        // Inside the recorded session the market reads as open.
        let during = DateTime::from_timestamp(1_795_670_000, 0).unwrap();
        assert!(
            parse_intraday(RELIANCE_1M, "1m", during)
                .unwrap()
                .meta
                .market_open
        );
    }

    #[test]
    fn parses_round_the_clock_intraday() {
        let data = parse_intraday(BTC_1M, "1m", after_close()).unwrap();
        assert_eq!(data.meta.symbol, "BTC-USD");
        assert!(data.points.len() > 700);
        assert!(data.points.iter().all(|p| p.close > 0.0));
    }

    #[test]
    fn intraday_errors_carry_the_body() {
        assert!(matches!(
            parse_intraday(NOT_FOUND, "1m", after_close()),
            Err(ProviderError::SymbolNotFound(_))
        ));
        let Err(ProviderError::Unavailable(message)) =
            parse_intraday("<html>blocked</html>", "1m", after_close())
        else {
            panic!("a block page must be an error");
        };
        assert!(message.contains("blocked"), "{message}");
    }

    #[test]
    fn not_found_maps_to_symbol_not_found() {
        assert!(matches!(
            parse_chart(NOT_FOUND, after_close()),
            Err(ProviderError::SymbolNotFound(_))
        ));
    }

    #[test]
    fn parses_splits_and_skips_null_rows() {
        let body = r#"{"chart":{"result":[{"meta":{"symbol":"X.NS","gmtoffset":19800},
            "timestamp":[1700000000,1700086400,1700172800],
            "indicators":{"quote":[{"open":[10,null,5],"high":[11,null,6],"low":[9,null,4],"close":[10,null,5],"volume":[1,null,2]}]},
            "events":{"splits":{"1700172800":{"date":1700172800,"numerator":2,"denominator":1}}}}],"error":null}}"#;
        let data = parse_chart(body, after_close()).unwrap();
        assert_eq!(data.bars.len(), 2);
        assert!(data.bars.iter().all(|b| b.adj_close.is_none()));
        assert!(matches!(
            data.corporate_actions[..],
            [CorporateAction::Split {
                numerator: 2.0,
                denominator: 1.0,
                ..
            }]
        ));
    }

    #[test]
    fn search_errors_are_not_silent_empty_results() {
        let err = parse_search(r#"{"finance":{"error":{"code":"Unauthorized"}}}"#).unwrap_err();
        assert!(
            matches!(&err, ProviderError::Unavailable(m) if m.contains("no quotes") && m.contains("Unauthorized")),
            "{err:?}"
        );
        assert!(parse_search("<html>blocked</html>").is_err());
        // A genuine empty result is still fine.
        assert!(parse_search(r#"{"quotes":[]}"#).unwrap().is_empty());
    }

    const SEARCH_RELIANCE_IN: &str =
        include_str!("../tests/fixtures/yahoo_search_reliance_in.json");

    #[test]
    fn search_hits_carry_exchange_and_country() {
        let hits = parse_search(SEARCH_RELIANCE_IN).unwrap();
        let reliance = hits.iter().find(|h| h.symbol == "RELIANCE.NS").unwrap();
        assert_eq!(
            (reliance.exchange.as_str(), reliance.country.as_str()),
            ("NSE", "India")
        );
        let nfty = parse_search(SEARCH).unwrap();
        let us = nfty.iter().find(|h| h.symbol == "NFTY").unwrap();
        assert_eq!(
            (us.exchange.as_str(), us.country.as_str()),
            ("NASDAQ", "USA")
        );
    }

    #[test]
    fn search_covers_every_exchange() {
        let hits = parse_search(SEARCH).unwrap();
        assert!(
            hits.iter()
                .any(|h| h.symbol == "^NSEI" && h.exchange == "NSE")
        );
        // A US-listed ETF on NIFTY is a valid result now; the app is global.
        assert!(
            hits.iter()
                .any(|h| h.symbol == "NFTY" && h.quote_type == "ETF")
        );
        assert_eq!(hits.len(), 7);
        let options = r#"{"quotes":[{"symbol":"AAPL261218C00200000","quoteType":"OPTION","exchange":"OPR"}]}"#;
        assert!(parse_search(options).unwrap().is_empty());
    }

    #[test]
    fn rejects_unsafe_symbols_and_ranges() {
        assert_eq!(validate_symbol(" reliance.ns ").unwrap(), "RELIANCE.NS");
        assert!(validate_symbol("^NSEI").is_ok());
        assert_eq!(validate_symbol("inr=x").unwrap(), "INR=X");
        for bad in ["", "A/B", "../x", "X?y=1", &"A".repeat(33)] {
            assert!(validate_symbol(bad).is_err(), "{bad}");
        }
        assert!(validate_range("10y").is_ok());
        assert!(validate_range("1h").is_err());
    }

    #[tokio::test]
    #[ignore = "hits the live Yahoo API"]
    async fn live_chart_and_search() {
        let client = YahooClient::default();
        let data = client.chart("RELIANCE.NS", "1mo").await.unwrap();
        assert!(data.bars.len() > 10);
        assert!(!client.search("reliance", "IN").await.unwrap().is_empty());
        let intraday = client.intraday("BTC-USD", "1d").await.unwrap();
        assert!(intraday.points.len() > 100, "crypto trades round the clock");
    }
}
