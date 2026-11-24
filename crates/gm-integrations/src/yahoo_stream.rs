//! Yahoo live price stream: one upstream WebSocket shared by every browser connection.
//!
//! ponytail: unofficial endpoint (the one yfinance's `WebSocket` uses). It is proven for crypto
//! and FX (1–3 s lag); NSE coverage must be re-checked in market hours, see docs/STATUS.md.
//! Swap for a broker tick feed (Upstox/Angel/Breeze/Dhan) behind the same `Tick` if it isn't.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::Engine;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};
use tokio_tungstenite::tungstenite::Message;

use crate::ProviderError;

pub const STREAM_URL: &str = "wss://streamer.finance.yahoo.com/?version=2";
pub const SOURCE: &str = "yahoo-streamer";
/// Per browser connection; keeps one client from widening the shared upstream subscription.
pub const MAX_SYMBOLS: usize = 20;
/// yfinance re-sends the subscription on this cadence or Yahoo stops pushing.
const RENEW_EVERY: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Wire message, field numbers from Yahoo's `PricingData` proto. Unlisted fields are skipped.
#[derive(Clone, PartialEq, prost::Message)]
struct PricingData {
    #[prost(string, tag = "1")]
    id: String,
    #[prost(float, tag = "2")]
    price: f32,
    /// Epoch milliseconds.
    #[prost(sint64, tag = "3")]
    time: i64,
    #[prost(string, tag = "4")]
    currency: String,
    #[prost(string, tag = "5")]
    exchange: String,
    #[prost(float, tag = "8")]
    change_percent: f32,
    #[prost(sint64, tag = "9")]
    day_volume: i64,
    #[prost(float, tag = "10")]
    day_high: f32,
    #[prost(float, tag = "11")]
    day_low: f32,
    #[prost(float, tag = "12")]
    change: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub symbol: String,
    pub price: f64,
    pub change: f64,
    pub change_pct: f64,
    /// Absent (None) when Yahoo omits the field, e.g. FX has no volume.
    pub day_high: Option<f64>,
    pub day_low: Option<f64>,
    pub day_volume: Option<i64>,
    pub currency: Option<String>,
    pub exchange: Option<String>,
    pub tick_time: DateTime<Utc>,
    pub source: String,
}

#[derive(Deserialize)]
struct Envelope {
    message: String,
}

/// Decode one frame: `{"type":"pricing","message":"<base64 protobuf>"}` (v2) or bare base64 (v1).
pub fn decode_tick(frame: &str) -> Result<Tick, ProviderError> {
    let encoded = match serde_json::from_str::<Envelope>(frame) {
        Ok(envelope) => envelope.message,
        Err(_) => frame.trim().to_string(),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| ProviderError::Unavailable(format!("stream frame is not base64: {e}")))?;
    let data = PricingData::decode(bytes.as_slice())
        .map_err(|e| ProviderError::Unavailable(format!("stream frame is not PricingData: {e}")))?;
    if data.id.is_empty() || !data.price.is_finite() || data.price <= 0.0 {
        return Err(ProviderError::Unavailable(
            "stream frame has no price".to_string(),
        ));
    }
    let tick_time = DateTime::from_timestamp_millis(data.time)
        .ok_or_else(|| ProviderError::Unavailable("stream frame has no time".to_string()))?;
    let positive = |v: f32| (v > 0.0).then_some(f64::from(v));
    let text = |s: String| (!s.is_empty()).then_some(s);
    Ok(Tick {
        symbol: data.id,
        price: f64::from(data.price),
        change: f64::from(data.change),
        change_pct: f64::from(data.change_percent),
        day_high: positive(data.day_high),
        day_low: positive(data.day_low),
        day_volume: (data.day_volume > 0).then_some(data.day_volume),
        currency: text(data.currency),
        exchange: text(data.exchange),
        tick_time,
        source: SOURCE.to_string(),
    })
}

struct Hub {
    url: String,
    ticks: broadcast::Sender<Tick>,
    /// Symbol -> number of live subscriptions wanting it.
    wanted: Mutex<HashMap<String, usize>>,
    last: Mutex<HashMap<String, Tick>>,
    changed: Notify,
    started: std::sync::Once,
}

/// Cheap to clone; the upstream task starts on the first subscription.
#[derive(Clone)]
pub struct YahooStream {
    hub: Arc<Hub>,
}

impl Default for YahooStream {
    fn default() -> Self {
        Self::new(STREAM_URL)
    }
}

/// Live interest in a set of symbols. Dropping it releases them upstream.
pub struct Subscription {
    pub symbols: BTreeSet<String>,
    pub ticks: broadcast::Receiver<Tick>,
    hub: Arc<Hub>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut wanted = self.hub.wanted.lock().expect("wanted lock");
        for symbol in &self.symbols {
            if let Some(count) = wanted.get_mut(symbol) {
                *count -= 1;
                if *count == 0 {
                    wanted.remove(symbol);
                }
            }
        }
        drop(wanted);
        self.hub.changed.notify_one();
    }
}

impl YahooStream {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            hub: Arc::new(Hub {
                url: url.into(),
                ticks: broadcast::channel(1024).0,
                wanted: Mutex::default(),
                last: Mutex::default(),
                changed: Notify::new(),
                started: std::sync::Once::new(),
            }),
        }
    }

    /// Must be called inside a Tokio runtime (it lazily spawns the upstream task).
    pub fn subscribe(&self, symbols: BTreeSet<String>) -> Subscription {
        let ticks = self.hub.ticks.subscribe();
        {
            let mut wanted = self.hub.wanted.lock().expect("wanted lock");
            for symbol in &symbols {
                *wanted.entry(symbol.clone()).or_default() += 1;
            }
        }
        let hub = Arc::clone(&self.hub);
        self.hub.started.call_once(|| {
            tokio::spawn(run(hub));
        });
        self.hub.changed.notify_one();
        Subscription {
            symbols,
            ticks,
            hub: Arc::clone(&self.hub),
        }
    }

    /// Most recent tick seen for `symbol`, so a new client doesn't wait for the next trade.
    pub fn last(&self, symbol: &str) -> Option<Tick> {
        self.hub
            .last
            .lock()
            .expect("last lock")
            .get(symbol)
            .cloned()
    }
}

/// The workspace links both rustls backends (sqlx brings ring, reqwest aws-lc-rs), so rustls
/// can't choose one itself and panics on the first TLS handshake. Pick aws-lc-rs, process-wide.
pub fn install_crypto_provider() {
    // Err means a provider is already installed, which is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn wanted_set(hub: &Hub) -> BTreeSet<String> {
    hub.wanted
        .lock()
        .expect("wanted lock")
        .keys()
        .cloned()
        .collect()
}

fn subscribe_message(key: &str, symbols: &BTreeSet<String>) -> Message {
    Message::text(serde_json::json!({ key: symbols }).to_string())
}

/// Upstream loop: idle while nobody listens, otherwise stay connected and reconnect with backoff.
async fn run(hub: Arc<Hub>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        while wanted_set(&hub).is_empty() {
            hub.changed.notified().await;
        }
        match session(&hub).await {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(error) => {
                tracing::warn!(%error, "yahoo stream disconnected; retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// One connection. Returns Ok when everyone unsubscribed, Err when the connection broke.
async fn session(hub: &Hub) -> Result<(), String> {
    install_crypto_provider();
    let (mut ws, _) = tokio_tungstenite::connect_async(hub.url.as_str())
        .await
        .map_err(|e| e.to_string())?;
    let mut current = wanted_set(hub);
    ws.send(subscribe_message("subscribe", &current))
        .await
        .map_err(|e| e.to_string())?;
    let mut renew = tokio::time::interval(RENEW_EVERY);
    renew.tick().await; // first tick fires immediately; we just subscribed

    loop {
        tokio::select! {
            frame = ws.next() => match frame {
                Some(Ok(Message::Text(text))) => match decode_tick(&text) {
                    // Ticks for symbols nobody wants any more (in-flight after unsubscribe) are dropped.
                    Ok(tick) if current.contains(&tick.symbol) => {
                        hub.last.lock().expect("last lock").insert(tick.symbol.clone(), tick.clone());
                        let _ = hub.ticks.send(tick); // Err only means no receivers right now.
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!(%error, "skipping undecodable stream frame"),
                },
                Some(Ok(Message::Ping(payload))) => {
                    ws.send(Message::Pong(payload)).await.map_err(|e| e.to_string())?;
                }
                Some(Ok(Message::Close(_))) | None => return Err("upstream closed".to_string()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.to_string()),
            },
            () = hub.changed.notified() => {
                let next = wanted_set(hub);
                if next.is_empty() {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
                let added = next.difference(&current).cloned().collect::<BTreeSet<_>>();
                let removed = current.difference(&next).cloned().collect::<BTreeSet<_>>();
                if !added.is_empty() {
                    ws.send(subscribe_message("subscribe", &added)).await.map_err(|e| e.to_string())?;
                }
                if !removed.is_empty() {
                    ws.send(subscribe_message("unsubscribe", &removed)).await.map_err(|e| e.to_string())?;
                }
                current = next;
            }
            _ = renew.tick() => {
                ws.send(subscribe_message("subscribe", &current)).await.map_err(|e| e.to_string())?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: &str = include_str!("../tests/fixtures/yahoo_stream_frames.txt");

    #[test]
    fn decodes_recorded_frames() {
        let ticks = FRAMES
            .lines()
            .map(|line| decode_tick(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ticks.len(), 6);
        let eth = ticks.iter().find(|t| t.symbol == "ETH-USD").unwrap();
        assert!((eth.price - 1682.43).abs() < 0.01);
        assert!(eth.day_low.unwrap() <= eth.price && eth.price <= eth.day_high.unwrap());
        assert_eq!(eth.currency.as_deref(), Some("USD"));
        assert!(eth.day_volume.unwrap() > 0);
        assert!(eth.change < 0.0 && eth.change_pct < 0.0);
        // Recorded 2026-11-24; epoch-ms decoding (sint64 zigzag) must land on that day.
        assert_eq!(eth.tick_time.date_naive().to_string(), "2026-11-24");
        let fx = ticks.iter().find(|t| t.symbol == "EURUSD=X").unwrap();
        assert!(fx.day_volume.is_none(), "FX frames carry no volume");
    }

    #[test]
    fn rejects_garbage_frames() {
        assert!(decode_tick("not base64 !!").is_err());
        assert!(decode_tick(r#"{"type":"pricing","message":""}"#).is_err());
        // Valid protobuf, but no symbol/price.
        let empty = base64::engine::general_purpose::STANDARD
            .encode(PricingData::default().encode_to_vec());
        assert!(decode_tick(&empty).is_err());
    }

    #[test]
    fn bare_base64_v1_frames_decode() {
        let line = FRAMES.lines().next().unwrap();
        let message = serde_json::from_str::<serde_json::Value>(line).unwrap()["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(decode_tick(&message).unwrap(), decode_tick(line).unwrap());
    }

    #[tokio::test]
    #[ignore = "live network; run in NSE hours: cargo test -p gm-integrations -- --ignored live_stream"]
    async fn live_stream_delivers_a_tick() {
        let symbol = std::env::var("LIVE_STREAM_SYMBOL").unwrap_or_else(|_| "BTC-USD".to_string());
        let stream = YahooStream::default();
        let mut sub = stream.subscribe(BTreeSet::from([symbol.clone()]));
        let tick = tokio::time::timeout(Duration::from_secs(30), sub.ticks.recv())
            .await
            .expect("no tick within 30 s")
            .unwrap();
        let lag = (Utc::now() - tick.tick_time).num_seconds();
        println!("{symbol} tick {} lag {lag}s", tick.price);
        assert_eq!(tick.symbol, symbol);
    }
}
