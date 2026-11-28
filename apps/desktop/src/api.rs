//! The same API the web app uses, over plain blocking HTTP on GPUI's background executor.
use gm_domain::analysis::plain::{Action, Plain};
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_API: &str = "https://gm-api-production-911a7e90.up.railway.app";

pub fn get(path: &str) -> Result<Value, String> {
    let base = std::env::var("GM_API_URL").unwrap_or_else(|_| DEFAULT_API.into());
    let mut req = ureq::get(format!("{base}{path}"));
    // The beta token lives in Railway; the app reads it from the environment, never from disk.
    if let Ok(token) = std::env::var("GM_ACCESS_TOKEN") {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    req.call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401) => "Not signed in: set GM_ACCESS_TOKEN.".to_string(),
            e => e.to_string(),
        })?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())
}

fn enc(symbol: &str) -> String {
    symbol
        .replace('^', "%5E")
        .replace('=', "%3D")
        .replace('&', "%26")
        .replace(' ', "%20")
}

#[derive(Clone, Deserialize)]
pub struct Bar {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone)]
pub struct Instrument {
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub plain: Plain,
    pub findings: Vec<String>,
    pub bars: Vec<Bar>,
}

/// A typed name ("reliance") or a symbol ("RELIANCE.NS", "^NSEI") to a symbol.
pub fn resolve(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Type a name or symbol.".into());
    }
    if t.contains(['.', '^', '=', '-'])
        || (t.len() <= 5 && t.chars().all(|c| c.is_ascii_uppercase()))
    {
        return Ok(t.to_string());
    }
    let hits = get(&format!("/api/v1/instruments/search?q={}", enc(t)))?;
    hits["results"][0]["symbol"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Nothing found for “{t}”."))
}

pub fn instrument(symbol: &str, range: &str, fd: f64) -> Result<Instrument, String> {
    let s = enc(symbol);
    let a = get(&format!("/api/v1/analyze?symbol={s}&range=10y&fd={fd}"))?;
    let bars = get(&format!("/api/v1/bars?symbol={s}&range={range}"))?;
    let plain: Plain =
        serde_json::from_value(a["plain"].clone()).map_err(|e| format!("plain: {e}"))?;
    let quote = get(&format!("/api/v1/quote?symbol={s}")).ok();
    let q = quote.as_ref().map(|q| &q["quote"]);
    Ok(Instrument {
        symbol: a["data"]["symbol"].as_str().unwrap_or(symbol).into(),
        name: a["data"]["name"].as_str().unwrap_or(symbol).into(),
        currency: a["data"]["currency"].as_str().unwrap_or("").into(),
        price: q
            .and_then(|q| q["last_price"].as_f64())
            .or(a["report"]["last_price"].as_f64())
            .unwrap_or_default(),
        change_pct: q.and_then(|q| q["change_pct"].as_f64()),
        plain,
        findings: serde_json::from_value(a["report"]["findings"].clone()).unwrap_or_default(),
        bars: serde_json::from_value(bars["chart"]["bars"].clone())
            .map_err(|e| format!("bars: {e}"))?,
    })
}

#[derive(Clone, Deserialize)]
pub struct TodayItem {
    pub symbol: String,
    pub group: String,
    pub name: Option<String>,
    pub last_price: Option<f64>,
    pub change_pct: Option<f64>,
    pub vol_ratio: Option<f64>,
    pub action: Option<Action>,
    pub headline: Option<String>,
    pub error: Option<String>,
}

pub fn today() -> Result<Vec<TodayItem>, String> {
    let v = get("/api/v1/today")?;
    serde_json::from_value(v["items"].clone()).map_err(|e| format!("today: {e}"))
}

#[derive(Clone, Deserialize)]
pub struct Impact {
    pub asset: String,
    pub direction: i32,
}

#[derive(Clone, Deserialize)]
pub struct Story {
    pub title: String,
    pub publisher: String,
    pub theme: String,
    #[serde(default)]
    pub impacts: Vec<Impact>,
}

#[derive(Clone, Deserialize)]
pub struct Lean {
    pub label: String,
    pub lean: i32,
}

#[derive(Clone, Deserialize)]
pub struct Desk {
    pub reader: String,
    pub stories: Vec<Story>,
    pub leans: Vec<Lean>,
}

pub fn desk() -> Result<Desk, String> {
    serde_json::from_value(get("/api/v1/desk")?).map_err(|e| format!("desk: {e}"))
}
