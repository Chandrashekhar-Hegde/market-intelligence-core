use std::{collections::BTreeSet, convert::Infallible, path::PathBuf, sync::Arc};

use axum::{
    Form, Json, Router,
    body::Bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{
        Html, IntoResponse, Redirect, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use gm_domain::{
    DECISION_MODEL_VERSION, EntityType, EventClass, MacroContext, NormalizedEvent, PriceBar,
    RuleRegistry, build_macro_context, classify, compute_features, gbm_flow_prediction,
    score_event,
};
use gm_integrations::{
    CheckoutRequest, CheckoutSession, CheckoutVerification, CheckoutVerificationRequest,
    MockPaymentProvider, PaymentProvider, ProviderError, ProviderStatus, WebhookPayload,
    WebhookVerification, markets,
    yahoo::{ChartData, YahooClient, validate_symbol},
    yahoo_stream::{MAX_SYMBOLS, YahooStream},
};
use gm_persistence::{PaymentEventRecord, PaymentOrderRecord, PgStore};
use serde::{Deserialize, Serialize};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use uuid::Uuid;

const SERVICE_NAME: &str = "gm-api";
const RAZORPAY_SIGNATURE_HEADER: &str = "x-razorpay-signature";
const ACCESS_COOKIE: &str = "gm_access";

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub database_url: Option<String>,
    pub migrations: PathBuf,
    pub run_migrations: bool,
    pub web_assets: Option<PathBuf>,
    pub payment_key_id: String,
    pub payment_checkout_secret: String,
    pub payment_webhook_secret: String,
    /// Private-beta gate: when set, every route except `/health`, `/access` and the signed
    /// Razorpay webhook needs this token as a bearer header or `gm_access` cookie.
    pub access_token: Option<String>,
    /// Extra origins allowed to call the API cross-origin (local Vite dev). Empty = same-origin only.
    pub cors_origins: Vec<String>,
    /// Override Yahoo endpoints (tests point these at local stubs).
    pub yahoo_base_url: Option<String>,
    pub yahoo_stream_url: Option<String>,
    /// Override the GDELT news endpoint (tests point it at a local stub).
    pub gdelt_base_url: Option<String>,
    /// Serve Google News and the publisher feeds from one stub (tests).
    pub news_base_url: Option<String>,
    /// Optional OpenAI-compatible model that reads headlines (GM_LLM_* env vars).
    pub llm: Option<gm_integrations::llm::LlmConfig>,
    /// Where the news recorder keeps `news.jsonl` (GM_DATA_DIR, a Railway volume). None = off.
    pub data_dir: Option<PathBuf>,
    /// Keep the daily list warm in the background (on in the server, off in tests).
    pub warm_today: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            database_url: None,
            migrations: PathBuf::from("migrations"),
            run_migrations: true,
            web_assets: None,
            payment_key_id: "rzp_test_local".to_string(),
            payment_checkout_secret: "local_checkout_signing_key".to_string(),
            payment_webhook_secret: "local_webhook_signing_key".to_string(),
            access_token: None,
            cors_origins: Vec::new(),
            yahoo_base_url: None,
            yahoo_stream_url: None,
            gdelt_base_url: None,
            news_base_url: None,
            llm: None,
            data_dir: None,
            warm_today: false,
        }
    }
}

mod brief;
pub mod recorder;
mod today;

#[derive(Clone)]
struct AppState {
    registry: Arc<RuleRegistry>,
    store: Option<PgStore>,
    payment_provider: Arc<MockPaymentProvider>,
    yahoo: YahooClient,
    stream: YahooStream,
    gdelt: gm_integrations::gdelt::GdeltClient,
    news: gm_integrations::news::NewsClient,
    llm: Option<gm_integrations::llm::LlmReader>,
    live_fits: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<LiveFit>>>>,
    recorder: Option<Arc<recorder::Recorder>>,
    /// The historical backfill file beside the live recording (filled by `/calibration/import`).
    backfill: Option<Arc<recorder::Recorder>>,
    /// NIFTY 50 panel priors for the trading rules (refreshed daily by the Today pass).
    rule_priors: today::Priors,
    /// News weights fitted by the calibration run; None until enough has been recorded.
    news_weights: Arc<std::sync::RwLock<Option<gm_domain::analysis::news_reader::FittedWeights>>>,
    today: today::Cache,
    brief: brief::Cache,
    access_token: Option<Arc<str>>,
}

/// What a live forecast needs from the daily history, fitted once per new daily close.
/// ponytail: one entry per symbol ever viewed, never evicted; fine for a private beta, add an
/// LRU if memory matters.
struct LiveFit {
    last_date: chrono::NaiveDate,
    last_close: f64,
    returns: Vec<f64>,
    garch: Option<gm_domain::analysis::volatility::Garch>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[derive(Debug, Serialize)]
struct VersionResponse {
    service: &'static str,
    version: &'static str,
    model_version: &'static str,
}

#[derive(Debug, Serialize)]
struct ReadyResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    persistence: PersistenceStatus,
}

#[derive(Debug, Serialize)]
struct PersistenceStatus {
    configured: bool,
    connected: bool,
    migrations: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct EventReviewSummary {
    event_id: String,
    version: i32,
    headline: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    source: Option<String>,
    region: Option<String>,
    sector: Option<String>,
    symbol: Option<String>,
    event_class: EventClass,
    confidence: f64,
    severity: String,
    entity_mapping_status: String,
    source_reliability: SourceReliability,
}

#[derive(Debug, Clone, Serialize)]
struct EventReviewDetail {
    summary: EventReviewSummary,
    event: NormalizedEvent,
    raw_source: RawSourceMetadata,
    normalized_facts: NormalizedFacts,
    entity_mappings: Vec<EntityMapping>,
    source_reliability: SourceReliability,
}

#[derive(Debug, Clone, Serialize)]
struct RawSourceMetadata {
    provider: String,
    source_id: String,
    url: Option<String>,
    received_at: chrono::DateTime<chrono::Utc>,
    language: String,
    raw_headline: String,
}

#[derive(Debug, Clone, Serialize)]
struct NormalizedFacts {
    event_type: Option<String>,
    symbol: Option<String>,
    sector: Option<String>,
    region: Option<String>,
    impact_level: Option<String>,
    impact_category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EntityMapping {
    entity_id: String,
    entity_type: EntityType,
    label: String,
    confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
struct SourceReliability {
    tier: String,
    score: f64,
    rationale: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct FeatureRequest {
    symbol: String,
    as_of: chrono::NaiveDate,
    bars: Vec<PriceBar>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PredictionRequest {
    symbol: String,
    as_of: chrono::NaiveDate,
    horizon: u32,
    fii_flow_norm: f64,
    bars: Vec<PriceBar>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MacroContextRequest {
    sector: String,
    inputs: gm_domain::MacroInputs,
}

#[derive(Debug, Deserialize, Serialize)]
struct PaymentOrderRequest {
    account_id: String,
    amount_paise: u64,
    currency: String,
    description: String,
    success_url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct PaymentVerifyRequest {
    order_id: String,
    payment_id: String,
    signature: String,
}

#[derive(Debug, Serialize)]
struct PaymentStateResponse {
    provider: ProviderStatus,
    mode: &'static str,
    live_billing_enabled: bool,
    checkout_verification: &'static str,
    webhook_verification: &'static str,
    recent_events: Vec<PaymentEventResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct PaymentEventResponse {
    event_id: String,
    provider: String,
    event_type: String,
    provider_order_id: Option<String>,
    provider_payment_id: Option<String>,
    verified: bool,
    received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
struct PaymentWebhookResponse {
    verification: WebhookVerification,
    event: PaymentEventResponse,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn persistence(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("persistence failed: {error}"),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn provider(error: ProviderError) -> Self {
        let status = match error {
            ProviderError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ProviderError::VerificationFailed(_) | ProviderError::InvalidRequest(_) => {
                StatusCode::BAD_REQUEST
            }
            ProviderError::SymbolNotFound(_) => StatusCode::NOT_FOUND,
            ProviderError::UnsupportedMode(_) => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

pub async fn build_app(config: ApiConfig) -> anyhow::Result<Router> {
    let store = match config.database_url.as_deref() {
        Some(database_url) if !database_url.trim().is_empty() => {
            let store = PgStore::connect(database_url).await?;
            if config.run_migrations {
                store.run_migrations(&config.migrations).await?;
            }
            Some(store)
        }
        _ => None,
    };

    let state = AppState {
        live_fits: Arc::default(),
        news_weights: Arc::default(),
        rule_priors: Arc::default(),
        today: Arc::default(),
        brief: Arc::default(),
        registry: Arc::new(RuleRegistry::builtin()),
        store,
        payment_provider: Arc::new(MockPaymentProvider::test_mode(
            config.payment_key_id,
            config.payment_checkout_secret,
            config.payment_webhook_secret,
        )),
        yahoo: config
            .yahoo_base_url
            .map(YahooClient::new)
            .unwrap_or_default(),
        stream: config
            .yahoo_stream_url
            .map(YahooStream::new)
            .unwrap_or_default(),
        gdelt: config
            .gdelt_base_url
            .map(gm_integrations::gdelt::GdeltClient::new)
            .unwrap_or_default(),
        news: config
            .news_base_url
            .as_deref()
            .map(gm_integrations::news::NewsClient::stub)
            .unwrap_or_default(),
        llm: config.llm.map(gm_integrations::llm::LlmReader::new),
        // A recorder that cannot open its file is logged and switched off; it never stops the
        // server from starting.
        backfill: config
            .data_dir
            .as_deref()
            .and_then(|dir| recorder::Recorder::open_backfill(dir).ok().map(Arc::new)),
        recorder: config.data_dir.as_deref().and_then(|dir| {
            recorder::Recorder::open(dir)
                .inspect_err(
                    |error| tracing::warn!(%error, ?dir, "news recorder off: cannot open its file"),
                )
                .ok()
                .map(Arc::new)
        }),
        access_token: config
            .access_token
            .filter(|token| !token.trim().is_empty())
            .map(Arc::from),
    };
    if config.warm_today {
        today::spawn_warmer(state.clone());
    }
    if let Some(rec) = state.recorder.clone() {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let (_, stories) = desk_stories(&state).await;
                // Keep the model's readings warm for the desk and company news.
                if let Some(llm) = &state.llm {
                    let titles: Vec<String> = stories.iter().map(|s| s.title.clone()).collect();
                    let named: Vec<String> = titles
                        .iter()
                        .filter(|t| !gm_integrations::universe::companies_named(t).is_empty())
                        .cloned()
                        .collect();
                    llm.read_in_background(&named, false);
                    llm.read_in_background(&titles, true);
                }
                // Keep the model's readings of company headlines with the recording.
                if let Some(llm) = &state.llm {
                    let known = rec.model_readings();
                    let named: Vec<String> = stories
                        .iter()
                        .map(|s| s.title.clone())
                        .filter(|t| {
                            !known.contains_key(t)
                                && !gm_integrations::universe::companies_named(t).is_empty()
                        })
                        .collect();
                    let rows: Vec<_> = named
                        .iter()
                        .cloned()
                        .zip(llm.cached(&named))
                        .filter_map(|(t, r)| r.map(|r| (t, r)))
                        .collect();
                    if let Err(error) = rec.append_model_readings(&rows) {
                        tracing::warn!(%error, "could not store model readings");
                    }
                }
                let writer = Arc::clone(&rec);
                match tokio::task::spawn_blocking(move || writer.record(&stories)).await {
                    Ok(Ok(new)) => tracing::info!(new, "news recorder"),
                    Ok(Err(error)) => tracing::warn!(%error, "news recorder could not write"),
                    Err(error) => tracing::warn!(%error, "news recorder task failed"),
                }
                // After the NSE close, keep today's 1-minute bars for the NIFTY 50 and NIFTY.
                keep_intraday(&state, &rec).await;
                // Refit the news weights on everything recorded so far.
                match calibrate(&state, Arc::clone(&rec), ReaderChoice::Best).await {
                    Ok((_, weights)) => {
                        tracing::info!(
                            events = weights.n_events,
                            voting_kinds = weights.per_kind.len(),
                            "news weights refitted"
                        );
                        *state.news_weights.write().expect("weights lock") = Some(weights);
                    }
                    Err(error) => tracing::warn!(error = ?error, "news weights not refitted"),
                }
                tokio::time::sleep(RECORD_EVERY).await;
            }
        });
    }
    let cors_origins = config
        .cors_origins
        .iter()
        .map(|origin| HeaderValue::from_str(origin.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    let router = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/version", get(version))
        .route("/openapi.json", get(openapi))
        .route("/events", get(events))
        .route("/events/{event_id}", get(event_detail))
        .route("/rules", get(rules))
        .route("/score", post(score))
        .route("/decide", post(decide_retired))
        .route("/quant/features", post(features))
        .route("/predict/gbm", post(predict_gbm))
        .route("/macro/context", post(macro_context))
        .route("/payments/state", get(payment_state))
        .route("/payments/orders", post(create_payment_order))
        .route("/payments/verify", post(verify_payment))
        .route("/payments/webhooks/razorpay", post(razorpay_webhook))
        .route("/api/v1/instruments/search", get(search_instruments))
        .route("/api/v1/bars", get(bars))
        .route("/api/v1/quote", get(quote))
        .route("/api/v1/intraday", get(intraday))
        .route("/api/v1/news", get(news))
        .route("/api/v1/desk", get(desk))
        .route("/api/v1/calibration", get(calibration))
        .route("/api/v1/calibration/import", post(import_calibration))
        .route("/api/v1/today", get(today::today))
        .route("/api/v1/brief", get(brief::brief))
        .route("/api/v1/stream", get(stream))
        .route("/api/v1/analyze", get(analyze_instrument))
        .route("/api/v1/projection", get(live_projection))
        .route("/api/v1/compare", get(compare_instruments))
        .route("/access", post(grant_access));
    let router = match config.web_assets {
        Some(web_assets) => router.fallback_service(ServeDir::new(web_assets)),
        None => router,
    };
    // Gate wraps the static fallback too, so the bundle is private as well.
    let router = router
        .layer(middleware::from_fn_with_state(state.clone(), access_gate))
        .layer(TraceLayer::new_for_http());
    let router = if cors_origins.is_empty() {
        router
    } else {
        router.layer(
            CorsLayer::new()
                .allow_origin(cors_origins)
                .allow_methods(Any)
                .allow_headers(Any),
        )
    };
    Ok(router.with_state(state))
}

// ---- private-beta access gate ----------------------------------------------------------------

// ponytail: one shared token, no accounts. Real auth arrives with portfolios in Phase 2.
async fn access_gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(token) = state.access_token.as_deref() else {
        return next.run(request).await;
    };
    let path = request.uri().path();
    // Health for the platform probe; the webhook carries its own HMAC; /access is the login form.
    let exempt = matches!(path, "/health" | "/access" | "/payments/webhooks/razorpay");
    if exempt || presented_token(request.headers()).is_some_and(|t| constant_time_eq(t, token)) {
        return next.run(request).await;
    }
    access_form(StatusCode::UNAUTHORIZED, "")
}

fn presented_token(headers: &HeaderMap) -> Option<&str> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    bearer.or_else(|| {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .find_map(|pair| pair.trim().strip_prefix("gm_access="))
    })
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn access_form(status: StatusCode, message: &str) -> Response {
    let page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Private beta</title></head>
<body style="font-family:system-ui;max-width:24rem;margin:4rem auto;padding:0 1rem">
<h1>Private beta</h1><p>{message}</p>
<form method="post" action="/access"><label for="token">Access token</label><br>
<input id="token" name="token" type="password" autocomplete="current-password" required style="width:100%;margin:.5rem 0">
<button type="submit">Enter</button></form></body></html>"#
    );
    (status, Html(page)).into_response()
}

#[derive(Debug, Deserialize)]
struct AccessForm {
    token: String,
}

async fn grant_access(State(state): State<AppState>, Form(form): Form<AccessForm>) -> Response {
    let Some(token) = state.access_token.as_deref() else {
        return Redirect::to("/").into_response();
    };
    if !constant_time_eq(form.token.trim(), token) {
        return access_form(StatusCode::UNAUTHORIZED, "That token is not valid.");
    }
    let cookie =
        format!("{ACCESS_COOKIE}={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=2592000");
    match HeaderValue::from_str(&cookie) {
        Ok(cookie) => ([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response(),
        Err(_) => access_form(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Server token is not cookie-safe.",
        ),
    }
}

// ---- /api/v1 market data ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    /// Keep only listings in this country ("India", "USA", …); case-insensitive.
    country: Option<String>,
    /// Keep only listings on this exchange ("NSE", "BSE", "NASDAQ", …); case-insensitive.
    exchange: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BarsQuery {
    symbol: String,
    range: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuoteQuery {
    symbol: String,
}

async fn search_instruments(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_integrations::yahoo::SearchHit;
    let wants = |filter: &Option<String>, value: &str| {
        filter.as_deref().map(str::trim).is_none_or(|f| {
            f.is_empty() || f.eq_ignore_ascii_case("all") || f.eq_ignore_ascii_case(value)
        })
    };
    let india_ok = wants(&query.country, "India");
    // Yahoo ranks by region; India first unless the user asked for another market.
    let region = if india_ok { "IN" } else { "US" };
    let mut hits = state
        .yahoo
        .search(&query.q, region)
        .await
        .map_err(ApiError::provider)?;

    // Yahoo's search rarely returns BSE listings, and a typed ticker may not be "found" by
    // name. Both are resolved through the (cached) quote endpoint and kept only if they exist.
    let mut candidates = Vec::new();
    let typed = query.q.trim();
    if typed.contains(['.', '^', '=', '-'])
        && !hits.iter().any(|h| h.symbol.eq_ignore_ascii_case(typed))
    {
        candidates.push(typed.to_uppercase());
    }
    if india_ok && wants(&query.exchange, "BSE") {
        candidates.extend(
            hits.iter()
                .filter(|h| h.symbol.ends_with(".NS") && h.quote_type == "EQUITY")
                .map(|h| h.symbol.replace(".NS", ".BO"))
                .filter(|twin| !hits.iter().any(|h| &h.symbol == twin))
                .take(6),
        );
    }
    let resolved = futures_util::future::join_all(candidates.iter().map(|symbol| {
        let yahoo = state.yahoo.clone();
        async move { yahoo.chart(symbol, "5d").await.ok() }
    }))
    .await;
    for chart in resolved.into_iter().flatten() {
        let meta = &chart.meta;
        let (exchange, country) = markets::listing_market(&meta.symbol, None);
        hits.push(SearchHit {
            symbol: meta.symbol.clone(),
            name: meta.name.clone().unwrap_or_else(|| meta.symbol.clone()),
            exchange: exchange.to_string(),
            country: country.to_string(),
            quote_type: meta.instrument_type.clone().unwrap_or_default(),
        });
    }

    // Which markets the matches came from, before filtering, so the UI can offer them.
    let mut summary: std::collections::BTreeMap<(String, String), usize> = Default::default();
    for h in &hits {
        *summary
            .entry((h.country.clone(), h.exchange.clone()))
            .or_default() += 1;
    }
    let markets: Vec<_> = summary
        .into_iter()
        .map(|((country, exchange), count)| serde_json::json!({ "country": country, "exchange": exchange, "count": count }))
        .collect();

    hits.retain(|h| wants(&query.country, &h.country) && wants(&query.exchange, &h.exchange));
    // Indian listings first, each BSE twin right after its NSE listing; otherwise Yahoo's
    // relevance order (the sort is stable).
    let base = |symbol: &str| {
        symbol
            .rsplit_once('.')
            .map_or(symbol, |(code, _)| code)
            .to_string()
    };
    let first_seen: Vec<String> = hits.iter().map(|h| base(&h.symbol)).collect();
    let rank = |h: &SearchHit| first_seen.iter().position(|b| *b == base(&h.symbol));
    hits.sort_by_key(|h| (h.country != "India", rank(h)));
    Ok(Json(
        serde_json::json!({ "results": hits, "markets": markets }),
    ))
}

async fn bars(
    State(state): State<AppState>,
    Query(query): Query<BarsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let range = query.range.as_deref().unwrap_or("1y");
    let chart = state
        .yahoo
        .chart(&query.symbol, range)
        .await
        .map_err(ApiError::provider)?;
    let calendar = home_calendar(&state, &chart, range).await;
    let quality = quality_report(&chart, calendar.as_ref().map(|(_, days)| days.as_slice()));
    Ok(Json(serde_json::json!({
        "chart": &*chart,
        "quality": quality,
        "calendar_reference": calendar.map(|(symbol, _)| symbol),
    })))
}

/// Trading days of the instrument's home-market index, as the exchange calendar for gap
/// checks (any exchange in `markets::MARKETS`, plus US). None when there is no local index or
/// it can't be fetched; the quality report then falls back to a calendar-day staleness check.
async fn home_calendar(
    state: &AppState,
    chart: &ChartData,
    range: &str,
) -> Option<(String, Vec<chrono::NaiveDate>)> {
    let meta = &chart.meta;
    let (benchmark, _) = markets::default_benchmark(
        &meta.symbol,
        meta.instrument_type.as_deref(),
        meta.currency.as_deref(),
    )?;
    if !markets::is_local_benchmark(&meta.symbol, benchmark) {
        return None;
    }
    let reference = state.yahoo.chart(benchmark, range).await.ok()?;
    Some((
        benchmark.to_string(),
        reference.bars.iter().map(|b| b.date).collect(),
    ))
}

#[derive(Debug, Deserialize)]
struct CompareQuery {
    symbols: String,
    range: Option<String>,
}

/// Compare 2–10 instruments on their common trading days: correlation (daily, rank, weekly),
/// Ledoit–Wolf covariance, PCA, Engle–Granger cointegration per pair, relative performance.
async fn compare_instruments(
    State(state): State<AppState>,
    Query(query): Query<CompareQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_domain::analysis::multi::{MAX_SYMBOLS as MAX, MIN_SYMBOLS as MIN, compare};
    let range = query.range.as_deref().unwrap_or("5y");
    let mut symbols = Vec::new();
    for raw in query.symbols.split(',').filter(|s| !s.trim().is_empty()) {
        let symbol = validate_symbol(raw).map_err(ApiError::provider)?;
        if !symbols.contains(&symbol) {
            symbols.push(symbol);
        }
    }
    if !(MIN..=MAX).contains(&symbols.len()) {
        return Err(ApiError::bad_request(format!(
            "compare {MIN}-{MAX} distinct symbols"
        )));
    }
    let charts =
        futures_util::future::join_all(symbols.iter().map(|s| state.yahoo.chart(s, range))).await;
    let mut loaded = Vec::new();
    for (symbol, chart) in symbols.iter().zip(charts) {
        let chart = chart.map_err(|e| {
            let mut err = ApiError::provider(e);
            err.message = format!("{symbol}: {}", err.message);
            err
        })?;
        loaded.push(chart);
    }
    // Name each series by the symbol requested (validated, distinct), not Yahoo's echo of it.
    let series = symbols
        .iter()
        .zip(&loaded)
        .map(|(symbol, c)| {
            (
                symbol.clone(),
                c.bars
                    .iter()
                    .map(|b| (b.date, b.adjusted_close()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let comparison = tokio::task::spawn_blocking(move || compare(&series))
        .await
        .map_err(|e| ApiError::bad_request(format!("comparison task failed: {e}")))?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let currencies = loaded
        .iter()
        .filter_map(|c| c.meta.currency.clone())
        .collect::<BTreeSet<_>>();
    let zones = loaded
        .iter()
        .filter_map(|c| c.meta.timezone.clone())
        .collect::<BTreeSet<_>>();
    let mut notes = Vec::new();
    if currencies.len() > 1 {
        notes.push(format!(
            "Prices are in different currencies ({}). Each return is in its own currency, so performance includes currency moves and isn't directly comparable.",
            currencies.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if zones.len() > 1 {
        notes.push("These markets close at different times of day. Daily correlations are biased down; use the weekly figures.".to_string());
    }
    Ok(Json(serde_json::json!({
        "comparison": comparison,
        "instruments": loaded.iter().map(|c| serde_json::json!({
            "symbol": c.meta.symbol,
            "name": c.meta.name,
            "currency": c.meta.currency,
            "exchange": c.meta.exchange,
            "timezone": c.meta.timezone,
            "source": c.source,
            "fetched_at": c.fetched_at,
        })).collect::<Vec<_>>(),
        "range": range,
        "notes": notes,
    })))
}

#[derive(Debug, Deserialize)]
struct AnalyzeQuery {
    symbol: String,
    range: Option<String>,
    /// "auto" (default): home-market index, S&P 500 for US, MSCI ACWI otherwise;
    /// "none": skip exposure; or any symbol.
    benchmark: Option<String>,
    seed: Option<u64>,
    paths: Option<usize>,
    /// Fixed-deposit rate a year the long-term answer compares with (default `DEPOSIT_RATE`).
    fd: Option<f64>,
}

/// How the plain-words module should treat an instrument.
fn plain_kind(
    instrument_type: Option<&str>,
    name: Option<&str>,
) -> gm_domain::analysis::plain::Kind {
    use gm_domain::analysis::plain::Kind;
    if instrument_type == Some("CURRENCY") {
        Kind::Currency
    } else if is_single_stock(instrument_type, name) {
        Kind::SingleStock
    } else {
        Kind::Other
    }
}

/// Full single-instrument analysis for any symbol Yahoo knows: distribution, predictability,
/// volatility (EWMA, GARCH-t), Monte Carlo fan charts, VaR/ES, drawdown, and benchmark
/// exposure when a benchmark applies.
async fn analyze_instrument(
    State(state): State<AppState>,
    Query(query): Query<AnalyzeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_domain::analysis::{AnalysisInput, DEFAULT_HORIZONS, DEFAULT_PATHS, MAX_PATHS, analyze};
    // Ten years by default: more out-of-sample days for validation (owner, 27 Nov 2026).
    let range = query.range.as_deref().unwrap_or("10y");
    let paths = query.paths.unwrap_or(DEFAULT_PATHS);
    if !(100..=MAX_PATHS).contains(&paths) {
        return Err(ApiError::bad_request(format!(
            "paths must be 100-{MAX_PATHS}"
        )));
    }
    let seed = query.seed.unwrap_or(42);
    let chart = state
        .yahoo
        .chart(&query.symbol, range)
        .await
        .map_err(ApiError::provider)?;
    let meta = &chart.meta;

    let (benchmark, selection) = match query.benchmark.as_deref().map(str::trim) {
        Some("none") => (None, "none"),
        Some(symbol) if !symbol.is_empty() && symbol != "auto" => (
            Some((validate_symbol(symbol).map_err(ApiError::provider)?, None)),
            "requested",
        ),
        _ => (
            markets::default_benchmark(
                &meta.symbol,
                meta.instrument_type.as_deref(),
                meta.currency.as_deref(),
            )
            .map(|(symbol, name)| (symbol.to_string(), Some(name.to_string()))),
            "auto",
        ),
    };
    let benchmark = benchmark.filter(|(symbol, _)| *symbol != meta.symbol);
    let bench_chart = match &benchmark {
        // A benchmark the caller asked for must work; an automatic one is best-effort.
        Some((symbol, _)) if selection == "requested" => Some(
            state
                .yahoo
                .chart(symbol, range)
                .await
                .map_err(ApiError::provider)?,
        ),
        Some((symbol, _)) => state.yahoo.chart(symbol, range).await.ok(),
        None => None,
    };

    let calendar = match (&benchmark, &bench_chart) {
        (Some((symbol, _)), Some(b)) if markets::is_local_benchmark(&meta.symbol, symbol) => {
            Some(b.bars.iter().map(|bar| bar.date).collect::<Vec<_>>())
        }
        _ => None,
    };
    let quality = quality_report(&chart, calendar.as_deref());

    let prices = chart
        .bars
        .iter()
        .map(|b| (b.date, b.adjusted_close()))
        .collect::<Vec<_>>();
    let bench_prices = bench_chart.as_ref().map(|b| {
        b.bars
            .iter()
            .map(|bar| (bar.date, bar.adjusted_close()))
            .collect::<Vec<_>>()
    });
    // The maths runs while the rules are tested and the news is fetched.
    let single = is_single_stock(meta.instrument_type.as_deref(), meta.name.as_deref());
    let symbol = meta.symbol.clone();
    let bench_symbol = benchmark.as_ref().map(|(s, _)| s.clone());
    let report_task = tokio::task::spawn_blocking(move || {
        analyze(&AnalysisInput {
            symbol: &symbol,
            prices: &prices,
            benchmark: bench_symbol.as_deref().zip(bench_prices.as_deref()),
            horizons: &DEFAULT_HORIZONS,
            n_paths: paths,
            seed,
            long_run_drift_from: single
                .then(|| bench_symbol.as_deref().zip(bench_prices.as_deref()))
                .flatten(),
        })
    });
    // Rules are tested on up to 10 years: the 3-month horizon needs 30+ separate calls.
    // Best-effort: the rest of the analysis does not depend on it.
    let mut news_vote = None;
    let (signals, advice, signals_note) = match state.yahoo.chart(&meta.symbol, "10y").await {
        Ok(long) => match tokio::task::spawn_blocking(move || trading_signals(&long))
            .await
            .ok()
            .flatten()
        {
            Some((mut signals, every_day)) => {
                today::grade(&state, &meta.symbol, &mut signals);
                // Today's news can drive or veto the call (owner decision, 27 Nov 2026), but
                // only for a company: news found for "NIFTY 50" or "Bitcoin" is market
                // commentary, which the reader isn't built for.
                // Best-effort: a news outage leaves the rules' advice as it is.
                let name = meta.name.clone().unwrap_or_else(|| meta.symbol.clone());
                let news =
                    match is_single_stock(meta.instrument_type.as_deref(), meta.name.as_deref()) {
                        false => None,
                        true => match gather_news(&state, &name).await {
                            Ok((_, stories, _)) => {
                                let readings = read_stories(&state, &stories).await;
                                Some(vote_on(&state, &stories, &readings))
                            }
                            Err(_) => None,
                        },
                    };
                let input = news.as_ref().map(|(vote, headlines)| {
                    gm_domain::analysis::signals::NewsInput { vote, headlines }
                });
                let advice = gm_domain::analysis::signals::advise_with_news(
                    &signals,
                    every_day,
                    input.as_ref(),
                );
                news_vote = news.map(|(vote, _)| vote);
                (Some(signals), Some(advice), None)
            }
            None => (
                None,
                None,
                Some(
                    "Not enough history to test the trading rules (needs about 1.5 years)."
                        .to_string(),
                ),
            ),
        },
        Err(e) => (
            None,
            None,
            Some(format!("Trading rules could not be tested: {e}")),
        ),
    };

    let report = report_task
        .await
        .map_err(|e| ApiError::bad_request(format!("analysis task failed: {e}")))?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let bench_currency = bench_chart.as_ref().and_then(|b| b.meta.currency.clone());
    let currency_note = match (&meta.currency, &bench_currency) {
        (Some(a), Some(b)) if a != b => Some(format!(
            "{} trades in {a} and the benchmark in {b}; beta and alpha include currency moves.",
            meta.symbol
        )),
        _ => None,
    };
    let fd = query
        .fd
        .filter(|r| (0.0..0.5).contains(r))
        .unwrap_or(gm_domain::analysis::DEPOSIT_RATE);
    let plain = gm_domain::analysis::plain::plain(
        &report,
        advice.as_ref(),
        fd,
        plain_kind(meta.instrument_type.as_deref(), meta.name.as_deref()),
    );
    Ok(Json(serde_json::json!({
        "report": report,
        "plain": plain,
        "signals": signals,
        "advice": advice,
        "signals_note": signals_note,
        "news_vote": news_vote,
        "news_weights": weights_summary(&state),
        "data": {
            "symbol": meta.symbol,
            "name": meta.name,
            "currency": meta.currency,
            "exchange": meta.exchange,
            "instrument_type": meta.instrument_type,
            "tradingview_symbol": meta.tradingview_symbol,
            "range": range,
            "source": chart.source,
            "fetched_at": chart.fetched_at,
            "quality": quality,
        },
        "benchmark": benchmark.map(|(symbol, name)| serde_json::json!({
            "symbol": symbol,
            "name": name.or_else(|| bench_chart.as_ref().and_then(|b| b.meta.name.clone())),
            "selection": selection,
            "used": report.benchmark.is_some(),
            "currency": bench_currency,
            "note": currency_note,
        })),
    })))
}

#[derive(Debug, Deserialize)]
struct ProjectionQuery {
    symbol: String,
    price: f64,
    seed: Option<u64>,
}

/// The forecast fan from a live price: the daily fit (10 years, refitted once per new close)
/// re-anchored at `price`, with today's move so far fed into GARCH. Milliseconds once fitted.
async fn live_projection(
    State(state): State<AppState>,
    Query(query): Query<ProjectionQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_domain::analysis::{DEFAULT_HORIZONS, DEFAULT_PATHS, LiveInput, live_projections};
    let chart = state
        .yahoo
        .chart(&query.symbol, "10y")
        .await
        .map_err(ApiError::provider)?;
    let symbol = chart.meta.symbol.clone();
    let Some(last) = chart.bars.last() else {
        return Err(ApiError::bad_request(format!("{symbol}: no daily history")));
    };
    let (last_date, last_close) = (last.date, last.adjusted_close());
    // A live price more than 50% from the last close is a bad input, not a market move.
    if !(query.price.is_finite()
        && query.price > 0.0
        && (query.price / last_close).ln().abs() < 0.7)
    {
        return Err(ApiError::bad_request(format!(
            "price must be a positive live price near the last close ({last_close:.2})"
        )));
    }
    let cached = state
        .live_fits
        .lock()
        .expect("live fits lock")
        .get(&symbol)
        .filter(|f| f.last_date == last_date)
        .cloned();
    let fit = match cached {
        Some(fit) => fit,
        None => {
            let closes: Vec<f64> = chart.bars.iter().map(|b| b.adjusted_close()).collect();
            let fit = tokio::task::spawn_blocking(move || {
                let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
                let garch = gm_domain::analysis::volatility::fit_garch(&returns, 252.0);
                Arc::new(LiveFit {
                    last_date,
                    last_close,
                    returns,
                    garch,
                })
            })
            .await
            .map_err(|e| ApiError::bad_request(format!("fit failed: {e}")))?;
            state
                .live_fits
                .lock()
                .expect("live fits lock")
                .insert(symbol.clone(), Arc::clone(&fit));
            fit
        }
    };
    if fit.returns.len() < 60 {
        return Err(ApiError::bad_request(format!(
            "{symbol}: too little history"
        )));
    }
    let (price, seed, sym) = (query.price, query.seed.unwrap_or(42), symbol.clone());
    let projections = tokio::task::spawn_blocking(move || {
        live_projections(&LiveInput {
            symbol: &sym,
            returns: &fit.returns,
            garch: fit.garch.as_ref(),
            last_close: fit.last_close,
            live_price: price,
            horizons: &DEFAULT_HORIZONS,
            n_paths: DEFAULT_PATHS,
            seed,
        })
    })
    .await
    .map_err(|e| ApiError::bad_request(format!("projection failed: {e}")))?;
    Ok(Json(serde_json::json!({
        "symbol": symbol,
        "price": query.price,
        "last_close": last_close,
        "last_date": last_date,
        "computed_at": chrono::Utc::now(),
        "projections": projections,
    })))
}

fn quality_report(
    chart: &ChartData,
    reference_days: Option<&[chrono::NaiveDate]>,
) -> gm_domain::quality::QualityReport {
    let action_days = chart
        .corporate_actions
        .iter()
        .map(|a| a.date())
        .collect::<Vec<_>>();
    let mut report = gm_domain::quality::assess(
        &chart.bars,
        reference_days,
        &action_days,
        chart.fetched_at.date_naive(),
    );
    if !chart.bad_prints_removed.is_empty() {
        report.warnings.push(format!(
            "{} bad price{} from the data vendor removed (a price that jumps away and straight back): {}",
            chart.bad_prints_removed.len(),
            if chart.bad_prints_removed.len() == 1 { "" } else { "s" },
            chart.bad_prints_removed.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(", ")
        ));
        report.bad_prints_removed = chart.bad_prints_removed.clone();
    }
    report
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    symbols: String,
}

/// Server-Sent Events of live ticks for up to `MAX_SYMBOLS` symbols. The last known tick per
/// symbol is sent first so the UI isn't blank between trades.
async fn stream(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let symbols = query
        .symbols
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(validate_symbol)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(ApiError::provider)?;
    if symbols.is_empty() || symbols.len() > MAX_SYMBOLS {
        return Err(ApiError::bad_request(format!(
            "symbols must list 1-{MAX_SYMBOLS} instruments"
        )));
    }
    let initial = symbols
        .iter()
        .filter_map(|s| state.stream.last(s))
        .collect::<Vec<_>>();
    let subscription = state.stream.subscribe(symbols);
    let live = futures_util::stream::unfold(subscription, |mut sub| async move {
        loop {
            match sub.ticks.recv().await {
                Ok(tick) if sub.symbols.contains(&tick.symbol) => return Some((tick, sub)),
                // Lagged: a slow client skipped ticks; the next one is still current.
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let events = futures_util::StreamExt::map(
        futures_util::StreamExt::chain(futures_util::stream::iter(initial), live),
        |tick| {
            Ok(Event::default()
                .event("tick")
                .json_data(&tick)
                .unwrap_or_else(|_| Event::default().comment("unserializable tick")))
        },
    );
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

async fn quote(
    State(state): State<AppState>,
    Query(query): Query<QuoteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chart = state
        .yahoo
        .chart(&query.symbol, "5d")
        .await
        .map_err(ApiError::provider)?;
    Ok(Json(serde_json::json!({
        "quote": chart.meta,
        "source": chart.source,
        "fetched_at": chart.fetched_at,
        "delayed": true,
    })))
}

/// Stories naming the company from every news source, newest first and de-duplicated, with
/// each source's outcome. Errors only when every source fails.
const NEWS_WAIT: std::time::Duration = std::time::Duration::from_secs(6);

async fn gather_news(
    state: &AppState,
    name: &str,
) -> Result<
    (
        String,
        Vec<gm_integrations::news::Story>,
        Vec<serde_json::Value>,
    ),
    ApiError,
> {
    use gm_integrations::news::Story;
    use gm_integrations::{
        gdelt::{company_phrase, news_query},
        news::{dedupe_key, mentions},
    };
    let phrase = company_phrase(name).ok_or_else(|| {
        ApiError::bad_request(format!("{name:?} is too short to search news for"))
    })?;
    let gdelt_query = news_query(name).unwrap_or_default();

    // Every source at once; each one's outcome is reported, and one failing never hides others.
    // GDELT paces callers through one lock; a request never waits more than this for it.
    let gdelt = async {
        tokio::time::timeout(NEWS_WAIT, state.gdelt.articles(&gdelt_query))
            .await
            .unwrap_or_else(|_| {
                Err(gm_integrations::ProviderError::Unavailable(
                    "gdelt: busy, skipped for this request".into(),
                ))
            })
    };
    let (google, feeds, gdelt) =
        tokio::join!(state.news.google(&phrase), state.news.feeds(), gdelt,);
    let mut sources = Vec::new();
    let mut stories: Vec<Story> = Vec::new();
    let mut report = |name: &str, result: Result<Vec<Story>, String>| match result {
        Ok(found) => {
            sources.push(serde_json::json!({ "name": name, "ok": true, "count": found.len(), "error": null }));
            stories.extend(found);
        }
        Err(e) => {
            sources.push(serde_json::json!({ "name": name, "ok": false, "count": 0, "error": e }))
        }
    };
    report(
        gm_integrations::news::GOOGLE_NEWS,
        google.map(|s| s.to_vec()).map_err(|e| e.to_string()),
    );
    for (name, result) in feeds {
        report(
            name,
            result
                .map(|s| s.iter().filter(|x| mentions(x, &phrase)).cloned().collect())
                .map_err(|e| e.to_string()),
        );
    }
    report(
        "GDELT",
        gdelt
            .map(|found| {
                found
                    .into_iter()
                    .map(|a| Story {
                        title: a.title,
                        url: a.url,
                        publisher: a.domain,
                        source: "GDELT".to_string(),
                        seen_at: a.seen_at,
                        summary: None,
                    })
                    .collect()
            })
            .map_err(|e| e.to_string()),
    );
    if sources.iter().all(|s| s["ok"] == false) {
        let errors: Vec<_> = sources.iter().filter_map(|s| s["error"].as_str()).collect();
        return Err(ApiError::provider(
            gm_integrations::ProviderError::Unavailable(errors.join("; ")),
        ));
    }
    // Newest first, one entry per story across sources. Every source is already recent
    // (Google News `when:3d`, GDELT 3 days, publisher feeds hold their latest items).
    stories.sort_by_key(|s| std::cmp::Reverse(s.seen_at));
    let mut seen = std::collections::HashSet::new();
    stories.retain(|s| seen.insert(dedupe_key(&s.title)));
    Ok((phrase, stories, sources))
}

/// One reading per story: the configured model's when it answers, otherwise the built-in reader.
async fn read_stories(
    state: &AppState,
    stories: &[gm_integrations::news::Story],
) -> Vec<gm_domain::analysis::news_reader::Reading> {
    use gm_domain::analysis::news_reader::read_headline;
    if let Some(llm) = &state.llm
        && !stories.is_empty()
    {
        // Never wait for the model: use what it has read, queue the rest, and let the built-in
        // reader cover until then.
        let titles: Vec<String> = stories.iter().map(|s| s.title.clone()).collect();
        llm.read_in_background(&titles, false);
        return llm
            .cached(&titles)
            .into_iter()
            .zip(stories)
            .map(|(cached, s)| cached.unwrap_or_else(|| read_headline(&s.title)))
            .collect();
    }
    stories.iter().map(|s| read_headline(&s.title)).collect()
}

/// A single company's shares. Company news votes only for these, and their long-run growth is
/// taken from their market. Yahoo labels Indian ETFs (GOLDBEES, MON100) "EQUITY", so names that
/// say ETF, BeES, fund or index are not single companies.
fn is_single_stock(instrument_type: Option<&str>, name: Option<&str>) -> bool {
    let fund = name.is_some_and(|n| {
        let n = n.to_lowercase();
        ["etf", "bees", " fund", "index", "nifty", "sensex"]
            .iter()
            .any(|w| n.contains(w))
    });
    instrument_type == Some("EQUITY") && !fund
}

/// Today's news vote from the readings, and the headlines behind it.
/// What the news vote is weighed with, for the page to say.
fn weights_summary(state: &AppState) -> serde_json::Value {
    match state.news_weights.read().expect("weights lock").as_ref() {
        Some(w) if w.is_authoritative() => serde_json::json!({
            "fitted": true,
            "n_events": w.n_events,
            "first": w.first,
            "last": w.last,
            "voting_kinds": w.per_kind,
        }),
        _ => serde_json::json!({ "fitted": false }),
    }
}

/// Fitted weights from the calibration run when it has any voting kind (the vote is then the
/// expected move beyond the market, against a round trip's cost); judgement weights otherwise.
fn vote_on(
    state: &AppState,
    stories: &[gm_integrations::news::Story],
    readings: &[gm_domain::analysis::news_reader::Reading],
) -> (gm_domain::analysis::news_reader::NewsVote, Vec<String>) {
    let now = chrono::Utc::now();
    let aged: Vec<_> = readings
        .iter()
        .cloned()
        .zip(stories)
        .map(|(r, s)| (r, (now - s.seen_at).num_minutes() as f64 / 60.0))
        .collect();
    let fitted = state.news_weights.read().expect("weights lock").clone();
    let vote = match fitted.filter(|w| w.is_authoritative()) {
        Some(w) => gm_domain::analysis::news_reader::news_vote_fitted(
            &aged,
            &w,
            gm_domain::analysis::signals::ROUND_TRIP_COST,
        ),
        None => gm_domain::analysis::news_reader::news_vote(&aged),
    };
    let headlines = vote
        .drivers
        .iter()
        .map(|&i| stories[i].title.clone())
        .collect();
    (vote, headlines)
}

/// Recent news that mentions the instrument (GDELT), the tone of its coverage, and how the price
/// moved after each story beyond what the market explains. Measurement only; nothing is stored.
/// Google News topic searches behind the desk (last day), besides the publisher feeds.
const DESK_QUERIES: [(&str, &str); 3] = [
    (
        "Google News: India",
        "RBI OR rupee OR FII OR FPI OR Sensex OR Nifty when:1d",
    ),
    (
        "Google News: global",
        "\"Federal Reserve\" OR \"crude oil\" OR \"gold price\" OR \"silver price\" OR \"Wall Street\" OR dollar when:1d",
    ),
    (
        "Google News: AI",
        "Groq OR OpenAI OR Nvidia OR xAI OR Anthropic OR \"AI stocks\" when:1d",
    ),
];
/// Stories older than this leave the desk.
const DESK_HOURS: f64 = 36.0;
const DESK_MAX: usize = 150;

/// The market news desk: every publisher feed and a few topic searches, newest first, each
/// headline read for what it means for gold, the rupee, banks, IT, oil marketers and US shares,
/// and the resulting lean per asset (uncalibrated; shown as a lean, never as a call).
/// How often the news recorder reads the desk sources.
const RECORD_EVERY: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Every desk source at once (each with its outcome), stories newest first, one per title.
async fn desk_stories(
    state: &AppState,
) -> (Vec<serde_json::Value>, Vec<gm_integrations::news::Story>) {
    use gm_integrations::news::{Story, dedupe_key};
    let topics = DESK_QUERIES.map(|(name, query)| {
        let news = state.news.clone();
        async move {
            let result = tokio::time::timeout(NEWS_WAIT, news.google_query(query))
                .await
                .unwrap_or_else(|_| {
                    Err(gm_integrations::ProviderError::Unavailable(
                        "no answer in time".into(),
                    ))
                });
            (name, result)
        }
    });
    let (feeds, topics) = tokio::join!(state.news.feeds(), futures_util::future::join_all(topics));
    let mut sources = Vec::new();
    let mut stories: Vec<Story> = Vec::new();
    for (name, result) in feeds.into_iter().chain(topics) {
        match result {
            Ok(found) => {
                sources.push(serde_json::json!({ "name": name, "ok": true, "count": found.len(), "error": null }));
                stories.extend(found.iter().cloned());
            }
            Err(e) => sources.push(
                serde_json::json!({ "name": name, "ok": false, "count": 0, "error": e.to_string() }),
            ),
        }
    }
    stories.sort_by_key(|s| std::cmp::Reverse(s.seen_at));
    let mut seen = std::collections::HashSet::new();
    stories.retain(|s| seen.insert(dedupe_key(&s.title)));
    (sources, stories)
}

/// Once a trading day, after 16:00 IST: the day's 1-minute bars for every NIFTY 50 stock and
/// NIFTY, so day-trading rules can be tested on more than the 60 days Yahoo keeps.
async fn keep_intraday(state: &AppState, rec: &recorder::Recorder) {
    let ist = chrono::Utc::now() + chrono::Duration::minutes(330);
    let today = ist.date_naive();
    let weekend = matches!(
        chrono::Datelike::weekday(&today),
        chrono::Weekday::Sat | chrono::Weekday::Sun
    );
    if weekend
        || ist.time() < chrono::NaiveTime::from_hms_opt(16, 0, 0).expect("valid time")
        || rec.intraday_path(today).exists()
    {
        return;
    }
    let mut lines = Vec::new();
    let symbols = gm_integrations::universe::NIFTY50
        .iter()
        .map(|(s, _)| *s)
        .chain(std::iter::once("^NSEI"));
    for symbol in symbols {
        let Ok(data) = state.yahoo.intraday(symbol, "1d").await else {
            continue;
        };
        // Only a session that happened today (holidays repeat the previous one).
        let session_day = data
            .points
            .first()
            .and_then(|p| chrono::DateTime::from_timestamp(p.t + 330 * 60, 0))
            .map(|d| d.date_naive());
        if session_day == Some(today) {
            lines.push(serde_json::json!({ "symbol": symbol, "points": data.points }).to_string());
        }
    }
    let rec_lines = lines.len();
    match rec.write_intraday(today, &lines) {
        Ok(true) => tracing::info!(instruments = rec_lines, "kept today's 1-minute bars"),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "could not keep today's 1-minute bars"),
    }
}

#[derive(Debug, Deserialize)]
struct ImportQuery {
    /// "backfill" (recorded headlines) or "model" (model readings).
    file: String,
}

/// Bring the workstation backfill onto the server's volume, in chunks under the 2 MB body limit
/// (`scripts/upload_backfill.sh`). Only records not already there are appended.
async fn import_calibration(
    State(state): State<AppState>,
    Query(query): Query<ImportQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (Some(live), Some(backfill)) = (state.recorder.clone(), state.backfill.clone()) else {
        return Err(ApiError::bad_request(
            "the recorder is off: set GM_DATA_DIR",
        ));
    };
    let file = query.file.clone();
    let added = tokio::task::spawn_blocking(move || match file.as_str() {
        "backfill" => backfill.import_records(&body).map_err(|e| e.to_string()),
        "model" => live.import_model_readings(&body).map_err(|e| e.to_string()),
        other => Err(format!("file must be backfill or model, not {other:?}")),
    })
    .await
    .map_err(|e| ApiError::bad_request(format!("import task failed: {e}")))?
    .map_err(ApiError::bad_request)?;
    Ok(Json(
        serde_json::json!({ "file": query.file, "added": added }),
    ))
}

/// The calibration run so far: what was recorded, and what prices did 1 and 5 sessions after
/// each kind of news (company news beyond NIFTY, market news raw), with how many events each
/// group has. A group with `enough` events can start replacing a judgement weight.
#[derive(Debug, Deserialize)]
struct CalibrationQuery {
    /// "best" (default: the model's stored reading where there is one, else the built-in
    /// reader, as the live vote reads), "built-in", or "model" (only headlines it has read).
    reader: Option<String>,
}

/// Which readings the calibration uses.
#[derive(Clone, Copy, PartialEq)]
enum ReaderChoice {
    Best,
    BuiltIn,
    Model,
}

/// The calibration study: events from the recording (live and backfill), what prices did 1 and
/// 5 sessions after (company news beyond NIFTY 50, market news raw), and the weights fitted on
/// the 5-session company moves.
async fn calibrate(
    state: &AppState,
    rec: Arc<recorder::Recorder>,
    choice: ReaderChoice,
) -> Result<
    (
        serde_json::Value,
        gm_domain::analysis::news_reader::FittedWeights,
    ),
    ApiError,
> {
    use futures_util::StreamExt;
    use gm_domain::analysis::{
        macro_news::ASSETS,
        news::{EventOutcome, MIN_EVENTS, event_study},
        news_reader::{NON_VOTING, fit_weights},
    };
    use recorder::{event_date, window_after, window_return};
    let (records, model) = tokio::task::spawn_blocking(move || {
        rec.read_all()
            .map(|records| (records, rec.model_readings()))
    })
    .await
    .map_err(|e| ApiError::bad_request(format!("calibration task failed: {e}")))?
    .map_err(|e| ApiError::bad_request(format!("cannot read the recording: {e}")))?;

    // One event per (instrument, kind or theme, day): many outlets, one event.
    let mut seen = std::collections::HashSet::new();
    let mut company = Vec::new(); // (symbol, kind, direction, materiality, day)
    let mut market = Vec::new(); // (symbol, asset, direction, day)
    let mut by_model = 0;
    for r in &records {
        let day = event_date(r.seen_at);
        let reading = match (choice, model.get(&r.title)) {
            (ReaderChoice::BuiltIn, _) | (ReaderChoice::Best, None) => Some(&r.reading),
            (_, Some(m)) => Some(m),
            (ReaderChoice::Model, None) => None,
        };
        if let Some(reading) = reading
            && reading.direction != 0
            && !NON_VOTING.contains(&reading.kind.as_str())
        {
            for symbol in &r.symbols {
                if seen.insert((symbol.clone(), reading.kind.clone(), day)) {
                    by_model += usize::from(reading.reader != "built-in");
                    company.push((
                        symbol.clone(),
                        reading.kind.clone(),
                        reading.direction,
                        reading.materiality,
                        day,
                    ));
                }
            }
        }
        for (asset, direction) in &r.impacts {
            if let Some((_, _, symbol)) = ASSETS.iter().find(|(key, _, _)| key == asset)
                && seen.insert((symbol.to_string(), format!("theme:{}", r.theme), day))
            {
                market.push((symbol.to_string(), asset.clone(), *direction, day));
            }
        }
    }
    let mut symbols: std::collections::BTreeSet<String> =
        company.iter().map(|e| e.0.clone()).collect();
    symbols.extend(market.iter().map(|e| e.0.clone()));
    symbols.insert("^NSEI".to_string());
    // Five years of prices: the backfill reaches three years back.
    let bars: std::collections::HashMap<String, Vec<(chrono::NaiveDate, f64)>> =
        futures_util::stream::iter(symbols)
            .map(|symbol| {
                let yahoo = state.yahoo.clone();
                async move {
                    let chart = yahoo.chart(&symbol, "5y").await.ok()?;
                    let closes = chart
                        .bars
                        .iter()
                        .map(|b| (b.date, b.adjusted_close()))
                        .collect();
                    Some((symbol, closes))
                }
            })
            .buffer_unordered(4)
            .filter_map(|x| async move { x })
            .collect()
            .await;
    let nifty = bars.get("^NSEI");
    let company_at = |h: usize| -> Vec<(String, EventOutcome)> {
        company
            .iter()
            .filter_map(|(symbol, kind, direction, materiality, day)| {
                let own = window_return(bars.get(symbol)?, *day, h)?;
                let index = window_return(nifty?, *day, h)?;
                Some((
                    symbol.clone(),
                    EventOutcome {
                        group: kind.clone(),
                        direction: *direction,
                        materiality: *materiality,
                        abnormal: own - index,
                    },
                ))
            })
            .collect()
    };
    let market_at = |h: usize| -> Vec<EventOutcome> {
        market
            .iter()
            .filter_map(|(symbol, asset, direction, day)| {
                Some(EventOutcome {
                    group: asset.clone(),
                    direction: *direction,
                    materiality: 2,
                    abnormal: window_return(bars.get(symbol)?, *day, h)?,
                })
            })
            .collect()
    };
    let study = |h: usize| {
        let c: Vec<EventOutcome> = company_at(h).into_iter().map(|(_, e)| e).collect();
        serde_json::json!({ "company": event_study(&c), "market": event_study(&market_at(h)) })
    };
    // Weights are fitted on what was still to be earned after the news was out (from the event
    // day's close), not on the same-day reaction nobody reading the headline could catch.
    let five: Vec<(String, i8, f64)> = company
        .iter()
        .filter_map(|(symbol, kind, direction, _, day)| {
            let own = window_after(bars.get(symbol)?, *day, 5)?;
            let index = window_after(nifty?, *day, 5)?;
            Some((kind.clone(), *direction, own - index))
        })
        .collect();
    let days: Vec<chrono::NaiveDate> = company.iter().map(|e| e.4).collect();
    let weights = fit_weights(
        &five,
        days.iter().min().copied(),
        days.iter().max().copied(),
    );
    let json = serde_json::json!({
        "recording": true,
        "reader": match choice { ReaderChoice::Best => "best", ReaderChoice::BuiltIn => "built-in", ReaderChoice::Model => "model" },
        "n_records": records.len(),
        "model_readings": model.len(),
        "first_seen": records.iter().map(|r| r.seen_at).min(),
        "last_recorded": records.iter().map(|r| r.recorded_at).max(),
        "company_events": company.len(),
        "company_events_read_by_model": by_model,
        "market_events": market.len(),
        "min_events": MIN_EVENTS,
        "after_1_session": study(1),
        "after_5_sessions": study(5),
        "fitted_weights": &weights,
        "note": "after_* studies include the day the news came out (the reaction). Fitted weights use only the move still to come after that day's close, beyond NIFTY 50 over 5 sessions: a kind of company news votes only with 30+ events and a significant move in the direction it was read.",
    });
    Ok((json, weights))
}

/// The calibration run so far: what was recorded, what prices did after each kind of news, and
/// the fitted weights the news vote uses.
async fn calibration(
    State(state): State<AppState>,
    Query(query): Query<CalibrationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(rec) = state.recorder.clone() else {
        return Ok(Json(serde_json::json!({
            "recording": false,
            "note": "The news recorder is off: set GM_DATA_DIR to a persistent directory.",
        })));
    };
    let choice = match query.reader.as_deref().unwrap_or("best") {
        "best" => ReaderChoice::Best,
        "built-in" => ReaderChoice::BuiltIn,
        "model" | "llm" => ReaderChoice::Model,
        other => {
            return Err(ApiError::bad_request(format!(
                "reader must be best, built-in or model, not {other:?}"
            )));
        }
    };
    Ok(Json(calibrate(&state, rec, choice).await?.0))
}

async fn desk(State(state): State<AppState>) -> Json<serde_json::Value> {
    use gm_domain::analysis::macro_news::{asset_leans, read_macro};
    use gm_integrations::news::Story;
    let (sources, mut stories) = desk_stories(&state).await;
    // Ages run from the newest story (never later than now): in a session that is minutes ago;
    // over a weekend the desk reads "as of" Friday's news instead of decaying it to nothing.
    let now = stories
        .iter()
        .map(|s| s.seen_at)
        .max()
        .unwrap_or_else(chrono::Utc::now)
        .min(chrono::Utc::now());
    let hours = |s: &Story| (now - s.seen_at).num_seconds() as f64 / 3600.0;
    stories.retain(|s| hours(s) <= DESK_HOURS);
    stories.truncate(DESK_MAX);
    // The model's market readings where it has read the headline (queued otherwise); the
    // keyword reader for the rest.
    let titles: Vec<String> = stories.iter().map(|s| s.title.clone()).collect();
    let model: Vec<Option<_>> = match &state.llm {
        Some(llm) => {
            llm.read_in_background(&titles, true);
            llm.cached_market(&titles)
        }
        None => vec![None; titles.len()],
    };
    let n_model = model.iter().filter(|m| m.is_some()).count();
    let readings: Vec<_> = stories
        .iter()
        .zip(model)
        .map(|(s, m)| {
            let reading = match m {
                Some((theme, impacts)) => {
                    gm_domain::analysis::macro_news::reading_from(&theme, &impacts)
                }
                None => read_macro(&s.title),
            };
            (reading, hours(s))
        })
        .collect();
    let leans = asset_leans(&readings);
    let items: Vec<_> = stories
        .iter()
        .zip(&readings)
        .map(|(story, (reading, _))| {
            serde_json::json!({
                "title": story.title,
                "url": story.url,
                "publisher": story.publisher,
                "source": story.source,
                "seen_at": story.seen_at,
                "theme": reading.theme,
                "impacts": reading.impacts,
            })
        })
        .collect();
    Json(serde_json::json!({
        "sources": sources,
        "stories": items,
        "leans": leans,
        "reader": match &state.llm {
            Some(llm) if n_model == stories.len() => llm.model().to_string(),
            Some(llm) if n_model > 0 => format!(
                "{} ({n_model} of {} headlines so far; the rest by the built-in reader)",
                llm.model(),
                stories.len()
            ),
            Some(llm) => match llm.trouble() {
                Some(why) => format!("built-in ({} {why})", llm.model()),
                None => "built-in".to_string(),
            },
            None => "built-in".to_string(),
        },
        "as_of": now,
    }))
}

async fn news(
    State(state): State<AppState>,
    Query(query): Query<QuoteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_domain::analysis::news::{ImpactInput, impact};
    let chart = state
        .yahoo
        .chart(&query.symbol, "1y")
        .await
        .map_err(ApiError::provider)?;
    let meta = &chart.meta;
    let name = meta.name.clone().unwrap_or_else(|| meta.symbol.clone());
    let (phrase, stories, sources) = gather_news(&state, &name).await?;
    let readings = read_stories(&state, &stories).await;

    // Reaction: 5-minute bars of the instrument and its home index over the last 5 sessions.
    let bench_symbol = markets::default_benchmark(
        &meta.symbol,
        meta.instrument_type.as_deref(),
        meta.currency.as_deref(),
    )
    .map(|(s, _)| s.to_string())
    .filter(|s| *s != meta.symbol);
    let intraday = state.yahoo.intraday(&meta.symbol, "5d").await.ok();
    let (bench_daily, bench_intraday) = match &bench_symbol {
        Some(b) => (
            state.yahoo.chart(b, "1y").await.ok(),
            state.yahoo.intraday(b, "5d").await.ok(),
        ),
        None => (None, None),
    };
    let daily: Vec<_> = chart
        .bars
        .iter()
        .map(|b| (b.date, b.adjusted_close()))
        .collect();
    let ppy = match (daily.first(), daily.last()) {
        (Some(a), Some(b)) if b.0 > a.0 => {
            (daily.len() - 1) as f64 / ((b.0 - a.0).num_days() as f64 / 365.25)
        }
        _ => 252.0,
    };
    let exposure = bench_symbol
        .as_deref()
        .zip(bench_daily.as_ref())
        .and_then(|(symbol, bd)| {
            let bench: Vec<_> = bd
                .bars
                .iter()
                .map(|b| (b.date, b.adjusted_close()))
                .collect();
            gm_domain::analysis::risk::exposure(symbol, &daily, &bench, ppy)
        });
    let returns: Vec<f64> = daily.windows(2).map(|w| (w[1].1 / w[0].1).ln()).collect();
    let sigma_daily = match &exposure {
        Some(e) => e.idiosyncratic_vol_annual / ppy.sqrt(),
        None => gm_domain::analysis::numeric::variance(&returns).sqrt(),
    };
    let points = |d: &gm_integrations::yahoo::IntradayData| -> Vec<(i64, f64)> {
        d.points.iter().map(|p| (p.t, p.close)).collect()
    };
    let instrument_points = intraday.as_deref().map(points).unwrap_or_default();
    let market_points = bench_intraday.as_deref().map(points);
    let session_seconds = intraday
        .as_deref()
        .and_then(|d| Some((d.session_end? - d.session_start?) as f64))
        .filter(|s| *s > 0.0)
        .unwrap_or(86_400.0);
    let input = ImpactInput {
        instrument: &instrument_points,
        market: market_points.as_deref().filter(|_| exposure.is_some()),
        beta: exposure.as_ref().map_or(0.0, |e| e.beta),
        sigma_daily,
        bar_seconds: if intraday.as_deref().is_some_and(|d| d.interval == "1m") {
            60
        } else {
            300
        },
        session_seconds,
    };
    let (vote, _) = vote_on(&state, &stories, &readings);
    let articles: Vec<_> = stories
        .into_iter()
        .zip(readings)
        .map(|(story, reading)| {
            let reaction = impact(&input, story.seen_at.timestamp());
            serde_json::json!({ "article": story, "reading": reading, "impact": reaction })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "symbol": meta.symbol,
        "query": phrase,
        "articles": articles,
        "sources": sources,
        "vote": vote,
        "reader": state.llm.as_ref().map_or("built-in", |l| l.model()),
        "benchmark": exposure.as_ref().map(|e| e.symbol.clone()),
        "beta": exposure.as_ref().map(|e| e.beta),
        "fetched_at": chrono::Utc::now(),
    })))
}

/// Literature trading rules tested walk-forward on this history, and the advice they support.
/// Also returns whether the history trades every day (crypto), for review dates.
fn trading_signals(chart: &ChartData) -> Option<(gm_domain::analysis::signals::Signals, bool)> {
    use gm_domain::analysis::signals::{SignalInput, evaluate};
    let dates: Vec<_> = chart.bars.iter().map(|b| b.date).collect();
    let closes: Vec<f64> = chart.bars.iter().map(|b| b.adjusted_close()).collect();
    let volume: Vec<f64> = chart.bars.iter().map(|b| b.volume as f64).collect();
    // Indices and FX report no (or patchy) volume; volume rules need it on nearly every day.
    let has_volume = volume.iter().filter(|v| **v > 0.0).count() * 10 >= volume.len() * 9;
    // Crypto trades every day; the review date then counts calendar days.
    let every_day = dates.iter().any(|d| {
        matches!(
            chrono::Datelike::weekday(d),
            chrono::Weekday::Sat | chrono::Weekday::Sun
        )
    });
    let signals = evaluate(&SignalInput {
        dates: &dates,
        close: &closes,
        volume: has_volume.then_some(volume.as_slice()),
    })?;
    Some((signals, every_day))
}

/// Intraday bars for the live chart: `1d` (1-minute) or `5d` (5-minute). Display-only.
async fn intraday(
    State(state): State<AppState>,
    Query(query): Query<BarsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chart = state
        .yahoo
        .intraday(&query.symbol, query.range.as_deref().unwrap_or("1d"))
        .await
        .map_err(ApiError::provider)?;
    Ok(Json(serde_json::json!({ "chart": &*chart })))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: SERVICE_NAME,
    })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: SERVICE_NAME,
        version: env!("CARGO_PKG_VERSION"),
        model_version: DECISION_MODEL_VERSION,
    })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<ReadyResponse>) {
    match state.store.as_ref() {
        Some(store) => match store.ping().await {
            Ok(()) => (
                StatusCode::OK,
                Json(ReadyResponse {
                    status: "ready",
                    service: SERVICE_NAME,
                    version: env!("CARGO_PKG_VERSION"),
                    persistence: PersistenceStatus {
                        configured: true,
                        connected: true,
                        migrations: "applied",
                    },
                }),
            ),
            Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse {
                    status: "not_ready",
                    service: SERVICE_NAME,
                    version: env!("CARGO_PKG_VERSION"),
                    persistence: PersistenceStatus {
                        configured: true,
                        connected: false,
                        migrations: "unknown",
                    },
                }),
            ),
        },
        None => (
            StatusCode::OK,
            Json(ReadyResponse {
                status: "ready",
                service: SERVICE_NAME,
                version: env!("CARGO_PKG_VERSION"),
                persistence: PersistenceStatus {
                    configured: false,
                    connected: false,
                    migrations: "not_configured",
                },
            }),
        ),
    }
}

async fn events() -> Json<Vec<EventReviewSummary>> {
    Json(
        event_review_fixtures()
            .into_iter()
            .map(|review| review.summary)
            .collect(),
    )
}

async fn event_detail(Path(event_id): Path<String>) -> Result<Json<EventReviewDetail>, ApiError> {
    event_review_fixtures()
        .into_iter()
        .find(|review| review.summary.event_id == event_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("event not found: {event_id}")))
}

async fn rules(State(state): State<AppState>) -> Json<Vec<gm_domain::RuleDefinition>> {
    Json(state.registry.rules().to_vec())
}

async fn score(
    State(state): State<AppState>,
    Json(event): Json<NormalizedEvent>,
) -> Json<gm_domain::scoring::ScoreOutput> {
    Json(score_event(&event, &state.registry))
}

/// Retired 2026-11-26 (#65): about 40 hand-picked, uncalibrated constants turned events into
/// BUY/SELL calls. Advice now comes only from rules that pass walk-forward validation
/// (`/api/v1/analyze` → `advice`).
async fn decide_retired() -> ApiError {
    ApiError {
        status: StatusCode::GONE,
        message: "/decide was retired: its calls came from uncalibrated constants. Use /api/v1/analyze?symbol=… and read `advice`, which comes only from rules validated on that instrument.".to_string(),
    }
}

async fn features(Json(request): Json<FeatureRequest>) -> Json<gm_domain::FeatureVector> {
    Json(compute_features(
        &request.symbol,
        request.as_of,
        &request.bars,
    ))
}

async fn predict_gbm(Json(request): Json<PredictionRequest>) -> Json<gm_domain::PredictionRecord> {
    Json(gbm_flow_prediction(
        &request.symbol,
        request.as_of,
        &request.bars,
        request.horizon,
        request.fii_flow_norm,
    ))
}

async fn macro_context(Json(request): Json<MacroContextRequest>) -> Json<MacroContext> {
    Json(build_macro_context(&request.sector, request.inputs))
}

async fn payment_state(
    State(state): State<AppState>,
) -> Result<Json<PaymentStateResponse>, ApiError> {
    let recent_events = match state.store.as_ref() {
        Some(store) => store
            .recent_payment_events(10)
            .await
            .map_err(ApiError::persistence)?
            .into_iter()
            .map(payment_event_response)
            .collect(),
        None => Vec::new(),
    };

    Ok(Json(PaymentStateResponse {
        provider: state.payment_provider.status().await,
        mode: "TEST_MODE",
        live_billing_enabled: false,
        checkout_verification: "HMAC_SHA256_ORDER_ID_PAYMENT_ID",
        webhook_verification: "HMAC_SHA256_RAW_BODY",
        recent_events,
    }))
}

async fn create_payment_order(
    State(state): State<AppState>,
    Json(request): Json<PaymentOrderRequest>,
) -> Result<Json<CheckoutSession>, ApiError> {
    let checkout = state
        .payment_provider
        .create_checkout(CheckoutRequest {
            account_id: request.account_id,
            amount_paise: request.amount_paise,
            currency: request.currency,
            description: request.description,
            success_url: request.success_url,
            receipt: None,
            notes: std::collections::BTreeMap::from([(
                "release".to_string(),
                "v0.1.0-mv".to_string(),
            )]),
        })
        .await
        .map_err(ApiError::provider)?;

    if let Some(store) = state.store.as_ref() {
        let amount_paise = i64::try_from(checkout.amount_paise)
            .map_err(|_| ApiError::bad_request("amount is too large"))?;
        store
            .save_payment_order(&PaymentOrderRecord {
                provider_order_id: checkout.order_id.clone(),
                provider: checkout.provider.clone(),
                account_id: checkout.account_id.clone(),
                receipt: checkout.receipt.clone(),
                amount_paise,
                currency: checkout.currency.clone(),
                status: checkout.status.clone(),
                test_mode: true,
            })
            .await
            .map_err(ApiError::persistence)?;
    }

    Ok(Json(checkout))
}

async fn verify_payment(
    State(state): State<AppState>,
    Json(request): Json<PaymentVerifyRequest>,
) -> Result<Json<CheckoutVerification>, ApiError> {
    let verification = state
        .payment_provider
        .verify_checkout(CheckoutVerificationRequest {
            order_id: request.order_id,
            payment_id: request.payment_id,
            signature: request.signature,
        })
        .await
        .map_err(ApiError::provider)?;

    let payload_json = serde_json::json!({
        "source": "checkout_return",
        "order_id": verification.order_id,
        "payment_id": verification.payment_id,
        "verified": verification.verified,
    });
    let event = payment_event_record(
        &verification.provider,
        "checkout.verified",
        Some(verification.order_id.clone()),
        Some(verification.payment_id.clone()),
        verification.verified,
        payload_json,
    );
    if let Some(store) = state.store.as_ref() {
        store
            .save_payment_event(&event)
            .await
            .map_err(ApiError::persistence)?;
    }

    Ok(Json(verification))
}

async fn razorpay_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<PaymentWebhookResponse>, ApiError> {
    let signature = headers
        .get(RAZORPAY_SIGNATURE_HEADER)
        .ok_or_else(|| ApiError::bad_request("missing x-razorpay-signature header"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid x-razorpay-signature header"))?
        .to_string();
    let raw_body = String::from_utf8(body.to_vec())
        .map_err(|_| ApiError::bad_request("webhook body must be utf-8 json"))?;
    let payload_json: serde_json::Value = serde_json::from_str(&raw_body)
        .map_err(|error| ApiError::bad_request(format!("invalid webhook json: {error}")))?;
    let verification = state
        .payment_provider
        .verify_webhook(WebhookPayload {
            raw_body,
            signature,
        })
        .await
        .map_err(ApiError::provider)?;
    let event = payment_event_record(
        &verification.provider,
        &verification.event,
        verification.order_id.clone(),
        verification.payment_id.clone(),
        verification.verified,
        payload_json,
    );

    if let Some(store) = state.store.as_ref() {
        store
            .save_payment_event(&event)
            .await
            .map_err(ApiError::persistence)?;
    }

    Ok(Json(PaymentWebhookResponse {
        verification,
        event: payment_event_response(event),
    }))
}

fn payment_event_record(
    provider: &str,
    event_type: &str,
    provider_order_id: Option<String>,
    provider_payment_id: Option<String>,
    verified: bool,
    payload_json: serde_json::Value,
) -> PaymentEventRecord {
    let seed = format!(
        "{provider}:{event_type}:{}:{}",
        provider_order_id.as_deref().unwrap_or("no-order"),
        provider_payment_id.as_deref().unwrap_or("no-payment")
    );
    PaymentEventRecord {
        event_id: Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string(),
        provider: provider.to_string(),
        event_type: event_type.to_string(),
        provider_order_id,
        provider_payment_id,
        verified,
        payload_json,
        received_at: chrono::Utc::now(),
    }
}

fn payment_event_response(event: PaymentEventRecord) -> PaymentEventResponse {
    PaymentEventResponse {
        event_id: event.event_id,
        provider: event.provider,
        event_type: event.event_type,
        provider_order_id: event.provider_order_id,
        provider_payment_id: event.provider_payment_id,
        verified: event.verified,
        received_at: event.received_at,
    }
}

fn event_review_fixtures() -> Vec<EventReviewDetail> {
    vec![
        build_event_review(EventReviewSeed {
            event_id: "norm-smoke-earnings",
            version: 1,
            causal_parent_id: Some("raw-smoke-earnings"),
            event_type: Some("EARNINGS"),
            headline: "Quarterly earnings beat estimates",
            body: "Profit rose and revenue grew higher than expected.",
            occurred_at: "2026-07-06T09:15:00Z",
            symbol: Some("RELIANCE"),
            sector: Some("Oil & Gas"),
            source: Some("NSE"),
            region: Some("IN"),
            impact_level: Some("HIGH"),
            impact_category: Some("EARNINGS"),
            provider: "NSE",
            source_id: "raw-smoke-earnings",
            url: Some("https://www.nseindia.com/"),
            severity: "High",
            confidence: 0.91,
            reliability_tier: "primary",
            reliability_score: 0.90,
            reliability_rationale: "Exchange filing fixture with direct company symbol mapping.",
            mappings: vec![
                entity_mapping(
                    "company:reliance",
                    EntityType::Company,
                    "Reliance Industries",
                    0.95,
                ),
                entity_mapping(
                    "instrument:reliance-nse",
                    EntityType::Instrument,
                    "Reliance NSE equity",
                    0.93,
                ),
                entity_mapping("sector:energy", EntityType::Sector, "Energy", 0.86),
            ],
        }),
        build_event_review(EventReviewSeed {
            event_id: "norm-policy-liquidity",
            version: 1,
            causal_parent_id: Some("raw-rbi-liquidity"),
            event_type: Some("POLICY_CHANGE"),
            headline: "Central bank policy statement updates liquidity stance",
            body: "Rate decision commentary changes liquidity stance and bank funding expectations.",
            occurred_at: "2026-07-06T10:00:00Z",
            symbol: Some("BANKNIFTY"),
            sector: Some("Banking"),
            source: Some("RBI"),
            region: Some("IN"),
            impact_level: Some("REVIEW"),
            impact_category: Some("MACRO_POLICY"),
            provider: "RBI",
            source_id: "raw-rbi-liquidity",
            url: Some("https://www.rbi.org.in/"),
            severity: "Review",
            confidence: 0.82,
            reliability_tier: "primary",
            reliability_score: 0.88,
            reliability_rationale: "Policy-body fixture with direct macro-policy source.",
            mappings: vec![
                entity_mapping(
                    "policy:rbi",
                    EntityType::PolicyBody,
                    "Reserve Bank of India",
                    0.92,
                ),
                entity_mapping("country:in", EntityType::Country, "India", 0.88),
                entity_mapping("index:nifty50", EntityType::Index, "NIFTY 50", 0.65),
            ],
        }),
        build_event_review(EventReviewSeed {
            event_id: "norm-medical-classification",
            version: 1,
            causal_parent_id: Some("raw-who-icd11"),
            event_type: Some("MEDICAL_CLASSIFICATION"),
            headline: "Therapy classification update affects reimbursement basket",
            body: "ICD-11 medical classification and reimbursement code update affects healthcare exposure.",
            occurred_at: "2026-07-06T11:30:00Z",
            symbol: Some("PHARMA"),
            sector: Some("Healthcare"),
            source: Some("WHO"),
            region: Some("GLOBAL"),
            impact_level: Some("WATCH"),
            impact_category: Some("HEALTH_CLASSIFICATION"),
            provider: "WHO",
            source_id: "raw-who-icd11",
            url: Some("https://icd.who.int/"),
            severity: "Watch",
            confidence: 0.78,
            reliability_tier: "reference",
            reliability_score: 0.80,
            reliability_rationale: "Reference taxonomy fixture used for market categorization only.",
            mappings: vec![
                entity_mapping(
                    "classification:icd11-respiratory",
                    EntityType::DiseaseClassification,
                    "ICD-11 respiratory classification",
                    0.84,
                ),
                entity_mapping("sector:healthcare", EntityType::Sector, "Healthcare", 0.76),
            ],
        }),
        build_event_review(EventReviewSeed {
            event_id: "norm-company-structure",
            version: 1,
            causal_parent_id: Some("raw-company-restructure"),
            event_type: Some("COMPANY_STRUCTURE"),
            headline: "Company board approves structure and reporting change",
            body: "Subsidiary restructuring updates ownership structure and segment reporting.",
            occurred_at: "2026-07-06T12:45:00Z",
            symbol: Some("RELIANCE"),
            sector: Some("Oil & Gas"),
            source: Some("BSE"),
            region: Some("IN"),
            impact_level: Some("REVIEW"),
            impact_category: Some("COMPANY_STRUCTURE"),
            provider: "BSE",
            source_id: "raw-company-restructure",
            url: Some("https://www.bseindia.com/"),
            severity: "Review",
            confidence: 0.74,
            reliability_tier: "primary",
            reliability_score: 0.84,
            reliability_rationale: "Company disclosure fixture with known issuer and sector path.",
            mappings: vec![
                entity_mapping(
                    "company:reliance",
                    EntityType::Company,
                    "Reliance Industries",
                    0.91,
                ),
                entity_mapping("sector:energy", EntityType::Sector, "Energy", 0.80),
            ],
        }),
        build_event_review(EventReviewSeed {
            event_id: "norm-conflict-shipping",
            version: 1,
            causal_parent_id: Some("raw-acled-shipping"),
            event_type: Some("CONFLICT"),
            headline: "Shipping lane conflict raises crude supply risk",
            body: "Geopolitical tension and shipping lane risk affect Brent crude movement.",
            occurred_at: "2026-07-06T13:20:00Z",
            symbol: Some("BRENT"),
            sector: Some("Energy"),
            source: Some("ACLED"),
            region: Some("GLOBAL"),
            impact_level: Some("HIGH"),
            impact_category: Some("CONFLICT_MARKET"),
            provider: "ACLED",
            source_id: "raw-acled-shipping",
            url: Some("https://acleddata.com/"),
            severity: "High",
            confidence: 0.72,
            reliability_tier: "corroborated",
            reliability_score: 0.72,
            reliability_rationale: "Conflict fixture linked to commodity and sector exposure.",
            mappings: vec![
                entity_mapping(
                    "actor:red-sea-shipping-risk",
                    EntityType::ConflictActor,
                    "Red Sea shipping risk",
                    0.78,
                ),
                entity_mapping(
                    "commodity:brent",
                    EntityType::Commodity,
                    "Brent crude oil",
                    0.86,
                ),
                entity_mapping("sector:energy", EntityType::Sector, "Energy", 0.74),
            ],
        }),
    ]
}

struct EventReviewSeed {
    event_id: &'static str,
    version: i32,
    causal_parent_id: Option<&'static str>,
    event_type: Option<&'static str>,
    headline: &'static str,
    body: &'static str,
    occurred_at: &'static str,
    symbol: Option<&'static str>,
    sector: Option<&'static str>,
    source: Option<&'static str>,
    region: Option<&'static str>,
    impact_level: Option<&'static str>,
    impact_category: Option<&'static str>,
    provider: &'static str,
    source_id: &'static str,
    url: Option<&'static str>,
    severity: &'static str,
    confidence: f64,
    reliability_tier: &'static str,
    reliability_score: f64,
    reliability_rationale: &'static str,
    mappings: Vec<EntityMapping>,
}

fn build_event_review(seed: EventReviewSeed) -> EventReviewDetail {
    let event = NormalizedEvent {
        event_id: seed.event_id.to_string(),
        version: seed.version,
        causal_parent_id: seed.causal_parent_id.map(str::to_string),
        event_type: seed.event_type.map(str::to_string),
        headline: seed.headline.to_string(),
        body: seed.body.to_string(),
        occurred_at: parse_timestamp(seed.occurred_at),
        symbol: seed.symbol.map(str::to_string),
        sector: seed.sector.map(str::to_string),
        source: seed.source.map(str::to_string),
        region: seed.region.map(str::to_string),
        impact_level: seed.impact_level.map(str::to_string),
        impact_category: seed.impact_category.map(str::to_string),
    };
    let source_reliability = SourceReliability {
        tier: seed.reliability_tier.to_string(),
        score: seed.reliability_score,
        rationale: seed.reliability_rationale.to_string(),
    };
    let entity_mapping_status = if seed
        .mappings
        .iter()
        .all(|mapping| mapping.confidence >= 0.75)
    {
        "resolved"
    } else {
        "review"
    };
    let summary = EventReviewSummary {
        event_id: event.event_id.clone(),
        version: event.version,
        headline: event.headline.clone(),
        occurred_at: event.occurred_at,
        source: event.source.clone(),
        region: event.region.clone(),
        sector: event.sector.clone(),
        symbol: event.symbol.clone(),
        event_class: classify(&event),
        confidence: seed.confidence,
        severity: seed.severity.to_string(),
        entity_mapping_status: entity_mapping_status.to_string(),
        source_reliability: source_reliability.clone(),
    };
    let normalized_facts = NormalizedFacts {
        event_type: event.event_type.clone(),
        symbol: event.symbol.clone(),
        sector: event.sector.clone(),
        region: event.region.clone(),
        impact_level: event.impact_level.clone(),
        impact_category: event.impact_category.clone(),
    };
    let raw_source = RawSourceMetadata {
        provider: seed.provider.to_string(),
        source_id: seed.source_id.to_string(),
        url: seed.url.map(str::to_string),
        received_at: event.occurred_at,
        language: "en".to_string(),
        raw_headline: seed.headline.to_string(),
    };

    EventReviewDetail {
        summary,
        event,
        raw_source,
        normalized_facts,
        entity_mappings: seed.mappings,
        source_reliability,
    }
}

fn entity_mapping(
    entity_id: &str,
    entity_type: EntityType,
    label: &str,
    confidence: f64,
) -> EntityMapping {
    EntityMapping {
        entity_id: entity_id.to_string(),
        entity_type,
        label: label.to_string(),
        confidence,
    }
}

fn parse_timestamp(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .expect("valid fixture timestamp")
        .with_timezone(&chrono::Utc)
}

async fn openapi() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Market Intelligence Core API",
            "version": env!("CARGO_PKG_VERSION"),
            "summary": "Deterministic market event scoring and decision service"
        },
        "paths": {
            "/health": {
                "get": {
                    "summary": "Liveness probe",
                    "responses": {
                        "200": { "description": "API process is alive" }
                    }
                }
            },
            "/ready": {
                "get": {
                    "summary": "Readiness probe including optional persistence",
                    "responses": {
                        "200": { "description": "API is ready" },
                        "503": { "description": "Configured persistence is unavailable" }
                    }
                }
            },
            "/version": {
                "get": {
                    "summary": "Service and model version",
                    "responses": {
                        "200": { "description": "Version metadata" }
                    }
                }
            },
            "/api/v1/instruments/search": {
                "get": {
                    "summary": "Search listings on any exchange, India first, with BSE twins and typed tickers resolved",
                    "parameters": [
                        { "name": "q", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "country", "in": "query", "required": false, "description": "India, USA, UK, Japan, Global (crypto/FX), … or all", "schema": { "type": "string" } },
                        { "name": "exchange", "in": "query", "required": false, "description": "NSE, BSE, NASDAQ, NYSE, London, … or all", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "`results` (filtered, India first) and `markets` (country, exchange, count before filtering)" },
                        "400": { "description": "Query empty or longer than 64 characters" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/api/v1/bars": {
                "get": {
                    "summary": "Adjusted daily bars, corporate actions and a data-quality report",
                    "parameters": [
                        { "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "range", "in": "query", "required": false, "schema": { "type": "string", "enum": ["5d", "1mo", "3mo", "6mo", "1y", "2y", "5y", "10y", "max"], "default": "1y" } }
                    ],
                    "responses": {
                        "200": { "description": "Chart data with provenance and quality report" },
                        "400": { "description": "Invalid symbol or range" },
                        "404": { "description": "Unknown symbol" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/api/v1/quote": {
                "get": {
                    "summary": "Delayed last price and instrument metadata",
                    "parameters": [{ "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Delayed quote" },
                        "400": { "description": "Invalid symbol" },
                        "404": { "description": "Unknown symbol" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/api/v1/intraday": {
                "get": {
                    "summary": "Intraday bars for the live chart (1d = 1-minute, 5d = 5-minute); display only",
                    "parameters": [
                        { "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "range", "in": "query", "required": false, "schema": { "type": "string", "enum": ["1d", "5d"], "default": "1d" } }
                    ],
                    "responses": {
                        "200": { "description": "Intraday points, previous close and session window, with provenance" },
                        "400": { "description": "Invalid symbol or range" },
                        "404": { "description": "Unknown symbol" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/api/v1/desk": {
                "get": {
                    "summary": "Market news desk: publisher feeds (ET, Mint, Business Standard, BusinessLine, CNBC, Bloomberg) and Google News topic searches, each headline read for its usual effect on gold, the rupee, banks, IT, oil marketers and US shares, with an uncalibrated lean per asset",
                    "responses": {
                        "200": { "description": "Sources with status, stories (newest first, last 36 h) with theme and impacts, leans per asset" }
                    }
                }
            },
            "/api/v1/brief": {
                "get": {
                    "summary": "Morning brief: do now, invest in, flagged / going bad, get out or in now, and the 1-day / 3-day / day-trading picture",
                    "description": "Facts from the Today scan and the news desk; written by a template, or worded by the configured model when every number it uses is in the facts. Cached 30 minutes.",
                    "responses": { "200": { "description": "as_of, writer (\"template\" or the model), note, sections [{title, bullets}], facts" } }
                }
            },
            "/api/v1/today": {
                "get": {
                    "summary": "Today's list: NIFTY 50, gold, silver, the rupee, US and AI names and the given watchlist, each with its call, chances and evidence (research signals, not registered advice)",
                    "parameters": [{ "name": "extra", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated watchlist symbols (at most 20)" }],
                    "responses": {
                        "200": { "description": "items (one per instrument, or an error entry) and a notice" },
                        "400": { "description": "An invalid symbol or more than 20 extras" }
                    }
                }
            },
            "/api/v1/calibration": {
                "get": {
                    "summary": "The news calibration run: recorded headlines and the average move 1 and 5 sessions after each kind of news",
                    "parameters": [{ "name": "reader", "in": "query", "schema": { "type": "string", "enum": ["built-in", "llm"], "default": "built-in" } }],
                    "responses": {
                        "200": { "description": "recording flag, counts, and event-study groups for company and market news" },
                        "400": { "description": "reader=llm without a configured model" }
                    }
                }
            },
            "/api/v1/calibration/import": {
                "post": {
                    "summary": "Append backfilled headlines (file=backfill) or model readings (file=model), JSONL under 2 MB per request; records already present are skipped",
                    "parameters": [{ "name": "file", "in": "query", "required": true, "schema": { "type": "string", "enum": ["backfill", "model"] } }],
                    "responses": {
                        "200": { "description": "{file, added}" },
                        "400": { "description": "Recorder off (no GM_DATA_DIR) or an unknown file" }
                    }
                }
            },
            "/api/v1/projection": {
                "get": {
                    "summary": "Forecast fans recomputed from a live price (10-year daily fit, cached until the next close)",
                    "parameters": [
                        { "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "price", "in": "query", "required": true, "schema": { "type": "number" } },
                        { "name": "seed", "in": "query", "schema": { "type": "integer", "default": 42 } }
                    ],
                    "responses": {
                        "200": { "description": "symbol, price, last_close, last_date, computed_at and projections as in /analyze" },
                        "400": { "description": "Invalid symbol, a price far from the last close, or too little history" },
                        "404": { "description": "Unknown symbol" }
                    }
                }
            },
            "/api/v1/news": {
                "get": {
                    "summary": "Recent news naming the instrument (Google News, publisher feeds, GDELT), each headline read, and the price reaction after each story",
                    "parameters": [{ "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Articles (newest first) with measured reaction, tone summary, provenance" },
                        "400": { "description": "Invalid symbol, or a name too short to search" },
                        "404": { "description": "Unknown symbol" },
                        "503": { "description": "Every news source unavailable" }
                    }
                }
            },
            "/api/v1/stream": {
                "get": {
                    "summary": "Live price ticks as Server-Sent Events (Yahoo streamer)",
                    "parameters": [{ "name": "symbols", "in": "query", "required": true, "description": "Comma-separated, 1-20 symbols", "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "text/event-stream of `tick` events" },
                        "400": { "description": "Empty, invalid, or more than 20 symbols" }
                    }
                }
            },
            "/api/v1/analyze": {
                "get": {
                    "summary": "Full single-instrument analysis for any symbol on any exchange",
                    "parameters": [
                        { "name": "symbol", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "range", "in": "query", "required": false, "schema": { "type": "string", "default": "5y" } },
                        { "name": "benchmark", "in": "query", "required": false, "description": "auto (home-market index, S&P 500 for US, MSCI ACWI otherwise), none, or a symbol", "schema": { "type": "string", "default": "auto" } },
                        { "name": "seed", "in": "query", "required": false, "schema": { "type": "integer", "default": 42 } },
                        { "name": "paths", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 100, "maximum": 20000, "default": 2000 } },
                        { "name": "fd", "in": "query", "required": false, "description": "Fixed-deposit rate a year that plain.answers compares with", "schema": { "type": "number", "minimum": 0, "maximum": 0.5, "default": 0.07 } }
                    ],
                    "responses": {
                        "200": { "description": "report (distribution, predictability, volatility, projections, risk, benchmark exposure, findings), data provenance and quality, benchmark choice" },
                        "400": { "description": "Invalid input or too little history" },
                        "404": { "description": "Unknown symbol or requested benchmark" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/api/v1/compare": {
                "get": {
                    "summary": "Compare 2-10 instruments: correlation, shrinkage covariance, PCA, cointegration, relative performance",
                    "parameters": [
                        { "name": "symbols", "in": "query", "required": true, "description": "Comma-separated, 2-10 distinct symbols on any exchange", "schema": { "type": "string" } },
                        { "name": "range", "in": "query", "required": false, "schema": { "type": "string", "default": "5y" } }
                    ],
                    "responses": {
                        "200": { "description": "comparison, instruments (provenance), notes (currency / time-zone caveats)" },
                        "400": { "description": "Wrong number of symbols, invalid input, or too few common trading days" },
                        "404": { "description": "A symbol is unknown (the message names it)" },
                        "503": { "description": "Upstream data provider unavailable" }
                    }
                }
            },
            "/rules": {
                "get": {
                    "summary": "Built-in scoring rules",
                    "responses": {
                        "200": { "description": "Rule definitions" }
                    }
                }
            },
            "/events": {
                "get": {
                    "summary": "Fixture-backed normalized event review list",
                    "responses": {
                        "200": {
                            "description": "Event review summaries",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/EventReviewSummary" }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/events/{event_id}": {
                "get": {
                    "summary": "Fixture-backed normalized event review detail",
                    "parameters": [
                        {
                            "name": "event_id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": {
                        "200": { "description": "Event review detail" },
                        "404": { "description": "Event not found" }
                    }
                }
            },
            "/score": {
                "post": {
                    "summary": "Score a normalized event",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/NormalizedEvent" }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Score output" }
                    }
                }
            },
            "/decide": {
                "post": {
                    "summary": "Retired (410). Use /api/v1/analyze and its validated `advice`",
                    "deprecated": true,
                    "responses": { "410": { "description": "Gone" } }
                }
            },
            "/quant/features": {
                "post": {
                    "summary": "Compute technical features from supplied price bars",
                    "responses": {
                        "200": { "description": "Feature vector" }
                    }
                }
            },
            "/predict/gbm": {
                "post": {
                    "summary": "Compute deterministic GBM flow-adjusted prediction",
                    "responses": {
                        "200": { "description": "Prediction record" }
                    }
                }
            },
            "/macro/context": {
                "post": {
                    "summary": "Build sector-weighted macro context",
                    "responses": {
                        "200": { "description": "Macro context" }
                    }
                }
            },
            "/payments/state": {
                "get": {
                    "summary": "Return Razorpay test-mode payment state and recent verified events",
                    "responses": {
                        "200": { "description": "Payment state" }
                    }
                }
            },
            "/payments/orders": {
                "post": {
                    "summary": "Create a deterministic Razorpay-compatible test-mode order",
                    "responses": {
                        "200": { "description": "Test-mode checkout session" },
                        "400": { "description": "Invalid payment order request" }
                    }
                }
            },
            "/payments/verify": {
                "post": {
                    "summary": "Verify Razorpay checkout signature in test mode",
                    "responses": {
                        "200": { "description": "Checkout signature verified" },
                        "400": { "description": "Checkout signature mismatch" }
                    }
                }
            },
            "/payments/webhooks/razorpay": {
                "post": {
                    "summary": "Verify Razorpay webhook signature from the raw request body",
                    "responses": {
                        "200": { "description": "Webhook signature verified and event accepted" },
                        "400": { "description": "Webhook signature mismatch or invalid payload" }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "NormalizedEvent": {
                    "type": "object",
                    "required": ["event_id", "version", "headline", "body", "occurred_at"],
                    "properties": {
                        "event_id": { "type": "string" },
                        "version": { "type": "integer" },
                        "causal_parent_id": { "type": ["string", "null"] },
                        "event_type": { "type": ["string", "null"] },
                        "headline": { "type": "string" },
                        "body": { "type": "string" },
                        "occurred_at": { "type": "string", "format": "date-time" },
                        "symbol": { "type": ["string", "null"] },
                        "sector": { "type": ["string", "null"] },
                        "source": { "type": ["string", "null"] },
                        "region": { "type": ["string", "null"] },
                        "impact_level": { "type": ["string", "null"] },
                        "impact_category": { "type": ["string", "null"] }
                    }
                },
                "Decision": {
                    "type": "object",
                    "required": [
                        "decision_id",
                        "model_version",
                        "input_hash",
                        "action",
                        "total_score",
                        "confidence",
                        "expected_return",
                        "downside",
                        "explanation",
                        "execution_ready"
                    ],
                    "properties": {
                        "decision_id": { "type": "string" },
                        "model_version": { "type": "string" },
                        "input_hash": { "type": "string" },
                        "action": { "type": "string", "enum": ["BUY", "SELL", "HOLD"] },
                        "total_score": { "type": "number" },
                        "confidence": { "type": "number" },
                        "expected_return": { "type": ["number", "null"] },
                        "downside": { "type": ["number", "null"] },
                        "explanation": { "type": "object" },
                        "execution_ready": { "type": "boolean" }
                    }
                },
                "EventReviewSummary": {
                    "type": "object",
                    "required": [
                        "event_id",
                        "version",
                        "headline",
                        "occurred_at",
                        "event_class",
                        "confidence",
                        "severity",
                        "entity_mapping_status",
                        "source_reliability"
                    ],
                    "properties": {
                        "event_id": { "type": "string" },
                        "version": { "type": "integer" },
                        "headline": { "type": "string" },
                        "occurred_at": { "type": "string", "format": "date-time" },
                        "source": { "type": ["string", "null"] },
                        "region": { "type": ["string", "null"] },
                        "sector": { "type": ["string", "null"] },
                        "symbol": { "type": ["string", "null"] },
                        "event_class": { "type": "string" },
                        "confidence": { "type": "number" },
                        "severity": { "type": "string" },
                        "entity_mapping_status": { "type": "string" },
                        "source_reliability": { "type": "object" }
                    }
                },
                "EventReviewDetail": {
                    "type": "object",
                    "required": [
                        "summary",
                        "event",
                        "raw_source",
                        "normalized_facts",
                        "entity_mappings",
                        "source_reliability"
                    ],
                    "properties": {
                        "summary": { "$ref": "#/components/schemas/EventReviewSummary" },
                        "event": { "$ref": "#/components/schemas/NormalizedEvent" },
                        "raw_source": { "type": "object" },
                        "normalized_facts": { "type": "object" },
                        "entity_mappings": {
                            "type": "array",
                            "items": { "type": "object" }
                        },
                        "source_reliability": { "type": "object" }
                    }
                }
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use gm_integrations::razorpay_webhook_signature;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    async fn app() -> Router {
        build_app(ApiConfig::default()).await.unwrap()
    }

    async fn json_request(
        app: Router,
        method: &str,
        uri: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload = serde_json::from_slice(&bytes).unwrap();
        (status, payload)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload = serde_json::from_slice(&bytes).unwrap();
        (status, payload)
    }

    async fn raw_request(
        app: Router,
        method: &str,
        uri: &str,
        body: String,
        signature: &str,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header(RAZORPAY_SIGNATURE_HEADER, signature)
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload = serde_json::from_slice(&bytes).unwrap();
        (status, payload)
    }

    fn decision_request() -> Value {
        json!({
          "event": {
            "event_id": "norm-smoke-earnings",
            "version": 1,
            "causal_parent_id": "raw-smoke-earnings",
            "event_type": "EARNINGS",
            "headline": "Quarterly earnings beat estimates",
            "body": "Profit rose and revenue grew higher than expected.",
            "occurred_at": "2026-07-06T09:15:00Z",
            "symbol": "RELIANCE",
            "sector": "Oil & Gas",
            "source": "NSE",
            "region": "IN",
            "impact_level": null,
            "impact_category": null
          },
          "facts": {
            "macro_context": {
              "sp500_futures_change": 0,
              "nasdaq_futures_change": 0,
              "brent_crude_change": 0,
              "usd_inr_change": 0,
              "fii_net_flow": 0,
              "gold_change": 0,
              "total_macro_score": 0
            },
            "entry_price": 1000,
            "exchange": "NSE",
            "features": null,
            "prediction": null,
            "kg_modifier": 0
          }
        })
    }

    #[tokio::test]
    async fn readiness_reports_optional_persistence() {
        let (status, payload) = get_json(app().await, "/ready").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["status"], "ready");
        assert_eq!(payload["persistence"]["configured"], false);
        assert_eq!(payload["persistence"]["migrations"], "not_configured");
    }

    #[tokio::test]
    async fn version_reports_service_and_model() {
        let (status, payload) = get_json(app().await, "/version").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["service"], SERVICE_NAME);
        assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(payload["model_version"], DECISION_MODEL_VERSION);
    }

    #[tokio::test]
    async fn openapi_lists_release_paths() {
        let (status, payload) = get_json(app().await, "/openapi.json").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["openapi"], "3.1.0");
        assert!(payload["paths"].get("/ready").is_some());
        assert!(payload["paths"].get("/version").is_some());
        assert!(payload["paths"].get("/decide").is_some());
        assert!(payload["paths"].get("/events").is_some());
        assert!(payload["paths"].get("/events/{event_id}").is_some());
        assert!(payload["paths"].get("/payments/state").is_some());
        assert!(payload["paths"].get("/payments/orders").is_some());
        assert!(payload["paths"].get("/payments/verify").is_some());
        assert!(
            payload["paths"]
                .get("/payments/webhooks/razorpay")
                .is_some()
        );
    }

    #[tokio::test]
    async fn events_contract_returns_fixture_summaries() {
        let (status, payload) = get_json(app().await, "/events").await;

        assert_eq!(status, StatusCode::OK);
        assert!(payload.as_array().unwrap().len() >= 5);
        assert_eq!(payload[0]["event_id"], "norm-smoke-earnings");
        assert_eq!(payload[0]["event_class"], "EARNINGS");
        assert_eq!(payload[0]["entity_mapping_status"], "resolved");
        assert!(payload[0]["source_reliability"]["score"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn event_detail_contract_returns_review_context() {
        let (status, payload) = get_json(app().await, "/events/norm-medical-classification").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["summary"]["event_class"], "MEDICAL_CLASSIFICATION");
        assert_eq!(payload["raw_source"]["provider"], "WHO");
        assert_eq!(
            payload["normalized_facts"]["impact_category"],
            "HEALTH_CLASSIFICATION"
        );
        assert!(payload["entity_mappings"].as_array().unwrap().len() >= 2);
        assert_eq!(
            payload["entity_mappings"][0]["entity_type"],
            "DISEASE_CLASSIFICATION"
        );
    }

    #[tokio::test]
    async fn missing_event_returns_not_found() {
        let (status, payload) = get_json(app().await, "/events/does-not-exist").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(payload["error"], "event not found: does-not-exist");
    }

    #[tokio::test]
    async fn decide_is_retired_with_a_pointer_to_validated_advice() {
        let (status, payload) =
            json_request(app().await, "POST", "/decide", decision_request()).await;
        assert_eq!(status, StatusCode::GONE);
        assert!(
            payload["error"]
                .as_str()
                .unwrap()
                .contains("/api/v1/analyze")
        );
    }

    #[tokio::test]
    async fn payment_state_reports_test_mode() {
        let (status, payload) = get_json(app().await, "/payments/state").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["mode"], "TEST_MODE");
        assert_eq!(payload["live_billing_enabled"], false);
        assert_eq!(payload["provider"]["name"], "razorpay-test");
        assert_eq!(payload["provider"]["mode"], "TEST_MODE");
        assert_eq!(
            payload["checkout_verification"],
            "HMAC_SHA256_ORDER_ID_PAYMENT_ID"
        );
        assert_eq!(payload["webhook_verification"], "HMAC_SHA256_RAW_BODY");
    }

    #[tokio::test]
    async fn payment_order_and_checkout_signature_verify() {
        let (order_status, order) = json_request(
            app().await,
            "POST",
            "/payments/orders",
            json!({
                "account_id": "acct_release",
                "amount_paise": 49900,
                "currency": "INR",
                "description": "MV access",
                "success_url": "https://example.test/payments/success"
            }),
        )
        .await;

        assert_eq!(order_status, StatusCode::OK);
        assert_eq!(order["provider"], "razorpay-test");
        assert_eq!(order["key_id"], "rzp_test_local");
        assert!(
            order["order_id"]
                .as_str()
                .unwrap()
                .starts_with("order_test_")
        );
        assert!(
            order["test_payment_id"]
                .as_str()
                .unwrap()
                .starts_with("pay_test_")
        );

        let (verify_status, verification) = json_request(
            app().await,
            "POST",
            "/payments/verify",
            json!({
                "order_id": order["order_id"],
                "payment_id": order["test_payment_id"],
                "signature": order["test_signature"]
            }),
        )
        .await;

        assert_eq!(verify_status, StatusCode::OK);
        assert_eq!(verification["verified"], true);
        assert_eq!(verification["order_id"], order["order_id"]);
        assert_eq!(verification["payment_id"], order["test_payment_id"]);
    }

    #[tokio::test]
    async fn payment_checkout_rejects_bad_signature() {
        let (status, payload) = json_request(
            app().await,
            "POST",
            "/payments/verify",
            json!({
                "order_id": "order_test_bad",
                "payment_id": "pay_test_bad",
                "signature": "bad-signature"
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            payload["error"]
                .as_str()
                .unwrap()
                .contains("signature mismatch")
        );
    }

    #[tokio::test]
    async fn razorpay_webhook_verifies_raw_body_signature() {
        let body = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_webhook",
                        "order_id": "order_test_webhook",
                        "amount": 49900,
                        "currency": "INR",
                        "captured": true
                    }
                }
            }
        })
        .to_string();
        let signature = razorpay_webhook_signature(&body, "local_webhook_signing_key");
        let (status, payload) = raw_request(
            app().await,
            "POST",
            "/payments/webhooks/razorpay",
            body,
            &signature,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["verification"]["verified"], true);
        assert_eq!(payload["verification"]["event"], "payment.captured");
        assert_eq!(payload["event"]["provider_payment_id"], "pay_test_webhook");
        assert_eq!(payload["event"]["provider_order_id"], "order_test_webhook");
    }

    async fn gated_app() -> Router {
        build_app(ApiConfig {
            access_token: Some("beta-secret".to_string()),
            ..ApiConfig::default()
        })
        .await
        .unwrap()
    }

    async fn status_of(app: Router, request: Request<Body>) -> (StatusCode, HeaderMap) {
        let response = app.oneshot(request).await.unwrap();
        (response.status(), response.headers().clone())
    }

    fn get_req(uri: &str) -> axum::http::request::Builder {
        Request::builder().method("GET").uri(uri)
    }

    #[tokio::test]
    async fn access_gate_blocks_without_token_and_admits_bearer_or_cookie() {
        let (status, _) = status_of(
            gated_app().await,
            get_req("/version").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _) = status_of(
            gated_app().await,
            get_req("/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "platform health probe stays open");

        let wrong = get_req("/version")
            .header("authorization", "Bearer nope")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status_of(gated_app().await, wrong).await.0,
            StatusCode::UNAUTHORIZED
        );

        let bearer = get_req("/version")
            .header("authorization", "Bearer beta-secret")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_of(gated_app().await, bearer).await.0, StatusCode::OK);

        let cookie = get_req("/version")
            .header("cookie", "a=b; gm_access=beta-secret")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_of(gated_app().await, cookie).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn access_form_sets_cookie_only_for_the_right_token() {
        let form = |token: &str| {
            Request::builder()
                .method("POST")
                .uri("/access")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("token={token}")))
                .unwrap()
        };
        let (status, headers) = status_of(gated_app().await, form("nope")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(headers.get("set-cookie").is_none());

        let (status, headers) = status_of(gated_app().await, form("beta-secret")).await;
        assert!(status.is_redirection());
        let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
        assert!(cookie.starts_with("gm_access=beta-secret;") && cookie.contains("HttpOnly"));
    }

    #[tokio::test]
    async fn market_data_routes_reject_bad_input_before_the_network() {
        for uri in [
            "/api/v1/bars?symbol=RELIANCE.NS&range=1h",
            "/api/v1/bars?symbol=..%2Fetc",
            "/api/v1/quote?symbol=",
            "/api/v1/instruments/search?q=",
        ] {
            let (status, payload) = get_json(app().await, uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {payload}");
        }
    }

    #[tokio::test]
    async fn openapi_lists_market_data_paths() {
        let (_, payload) = get_json(app().await, "/openapi.json").await;
        for path in [
            "/api/v1/instruments/search",
            "/api/v1/bars",
            "/api/v1/quote",
            "/api/v1/intraday",
            "/api/v1/news",
            "/api/v1/desk",
            "/api/v1/projection",
            "/api/v1/calibration",
            "/api/v1/today",
        ] {
            assert!(payload["paths"].get(path).is_some(), "{path}");
        }
    }

    // ---- market data against in-process Yahoo stubs (no network) -------------------------

    const CHART: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_chart_reliance_1y.json");
    const NOT_FOUND: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_chart_not_found.json");
    const SEARCH: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_search_nifty.json");
    const CHART_10Y: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_chart_reliance_10y.json");
    const INTRADAY: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_chart_reliance_1d_1m.json");
    const SEARCH_RELIANCE: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_search_reliance_in.json");
    const FRAMES: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_stream_frames.txt");

    /// Recorded chart/search responses on an ephemeral port. `fail` answers 500 to everything.
    async fn yahoo_stub(fail: bool) -> String {
        let chart =
            move |Path(symbol): Path<String>,
                  Query(params): Query<std::collections::HashMap<String, String>>| async move {
                let intraday = params.get("interval").is_some_and(|i| i != "1d");
                let ten_years = params.get("range").is_some_and(|r| r == "10y");
                match (fail, symbol.as_str()) {
                    (true, _) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
                    (false, "RELIANCE.NS" | "^NSEI") if intraday => {
                        (StatusCode::OK, INTRADAY.to_string())
                    }
                    (false, "RELIANCE.NS") if ten_years => (StatusCode::OK, CHART_10Y.to_string()),
                    // RELIANCE.BO: the BSE twin of the NSE fixture.
                    (false, "RELIANCE.BO") => (
                        StatusCode::OK,
                        CHART.replace("\"symbol\":\"RELIANCE.NS\"", "\"symbol\":\"RELIANCE.BO\""),
                    ),
                    // ^NSEI doubles as the trading calendar; the RELIANCE fixture has the same days.
                    (false, "RELIANCE.NS" | "^NSEI") => (StatusCode::OK, CHART.to_string()),
                    (false, _) => (StatusCode::NOT_FOUND, NOT_FOUND.to_string()),
                }
            };
        let search = move |Query(params): Query<std::collections::HashMap<String, String>>| async move {
            match (fail, params.get("q").map(String::as_str)) {
                (true, _) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
                (false, Some("reliance")) => (StatusCode::OK, SEARCH_RELIANCE.to_string()),
                _ => (StatusCode::OK, SEARCH.to_string()),
            }
        };
        let stub = Router::new()
            .route("/v8/finance/chart/{symbol}", get(chart))
            .route("/v1/finance/search", get(search));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
        format!("http://{addr}")
    }

    /// WebSocket stub: per connection, wait for a subscribe message, replay the recorded frames,
    /// then hang up, so the client must reconnect to see them again.
    async fn stream_stub() -> String {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                    while let Some(Ok(msg)) = ws.next().await {
                        if msg.to_text().unwrap_or_default().contains("subscribe") {
                            break;
                        }
                    }
                    for frame in FRAMES.lines() {
                        ws.send(Message::text(frame)).await.unwrap();
                    }
                    let _ = ws.close(None).await;
                });
            }
        });
        format!("ws://{addr}")
    }

    /// GDELT stub: articles and tone from fixtures, or the recorded rate-limit notice.
    async fn gdelt_stub(rate_limited: bool) -> String {
        let doc = move |Query(params): Query<std::collections::HashMap<String, String>>| async move {
            if rate_limited {
                return GDELT_LIMITED.to_string();
            }
            match params.get("mode").map(String::as_str) {
                Some("artlist") => GDELT_ARTICLES.to_string(),
                _ => "unknown mode".to_string(),
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/api/v2/doc/doc", get(doc)))
                .await
                .unwrap()
        });
        format!("http://{addr}")
    }

    const GDELT_ARTICLES: &str =
        include_str!("../../gm-integrations/tests/fixtures/gdelt_articles_reliance.json");
    const GDELT_LIMITED: &str =
        include_str!("../../gm-integrations/tests/fixtures/gdelt_rate_limited.txt");

    const GOOGLE_RSS: &str =
        include_str!("../../gm-integrations/tests/fixtures/googlenews_reliance.xml");
    const ET_RSS: &str = include_str!("../../gm-integrations/tests/fixtures/et_markets.xml");
    const MINT_RSS: &str = include_str!("../../gm-integrations/tests/fixtures/mint_markets.xml");
    const BS_RSS: &str = include_str!("../../gm-integrations/tests/fixtures/bs_markets.xml");
    const BUSINESSLINE_RSS: &str =
        include_str!("../../gm-integrations/tests/fixtures/businessline_markets.xml");
    const CNBC_RSS: &str = include_str!("../../gm-integrations/tests/fixtures/cnbc_top.xml");
    const BLOOMBERG_RSS: &str =
        include_str!("../../gm-integrations/tests/fixtures/bloomberg_markets.xml");

    /// Google News and the publisher feeds from recordings; `google_down` answers an HTML page.
    async fn news_stub(google_down: bool) -> String {
        let google = move || async move {
            if google_down {
                "<html>unusual traffic</html>"
            } else {
                GOOGLE_RSS
            }
        };
        let stub = Router::new()
            .route("/rss/search", get(google))
            .route("/et.xml", get(|| async { ET_RSS }))
            .route("/mint.xml", get(|| async { MINT_RSS }))
            .route("/bs.xml", get(|| async { BS_RSS }))
            .route("/businessline.xml", get(|| async { BUSINESSLINE_RSS }))
            .route("/cnbc.xml", get(|| async { CNBC_RSS }))
            .route("/bloomberg.xml", get(|| async { BLOOMBERG_RSS }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
        format!("http://{addr}")
    }

    async fn news_app(google_down: bool, gdelt_limited: bool) -> Router {
        build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(false).await),
            yahoo_stream_url: Some(stream_stub().await),
            gdelt_base_url: Some(gdelt_stub(gdelt_limited).await),
            news_base_url: Some(news_stub(google_down).await),
            ..ApiConfig::default()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn backfill_chunks_import_once_and_need_a_data_dir() {
        let line = serde_json::json!({
            "recorded_at": "2026-11-27T10:00:00Z", "seen_at": "2024-01-15T05:00:00Z",
            "title": "HDFC Bank beats Q3 profit forecast", "url": "https://example.com", "publisher": "Reuters",
            "source": "Google News", "symbols": ["HDFCBANK.NS"],
            "reading": { "kind": "results", "direction": 1, "materiality": 3, "reason": "x", "reader": "built-in" },
            "theme": "Other", "impacts": []
        })
        .to_string();
        let (status, _) = raw_request(
            news_app(false, true).await,
            "POST",
            "/api/v1/calibration/import?file=backfill",
            line.clone(),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "no data dir");
        let dir = std::env::temp_dir().join(format!("gm-import-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = build_app(ApiConfig {
            data_dir: Some(dir.clone()),
            ..ApiConfig::default()
        })
        .await
        .unwrap();
        let (status, body) = raw_request(
            app.clone(),
            "POST",
            "/api/v1/calibration/import?file=backfill",
            line.clone(),
            "",
        )
        .await;
        assert_eq!(
            (status, body["added"].as_u64()),
            (StatusCode::OK, Some(1)),
            "{body}"
        );
        let (_, again) = raw_request(
            app.clone(),
            "POST",
            "/api/v1/calibration/import?file=backfill",
            line,
            "",
        )
        .await;
        assert_eq!(again["added"], 0);
        let (status, _) = raw_request(
            app,
            "POST",
            "/api/v1/calibration/import?file=other",
            String::new(),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn the_recorder_keeps_headlines_and_calibration_reads_them() {
        // Off unless a data directory is set.
        let (_, off) = get_json(news_app(false, true).await, "/api/v1/calibration").await;
        assert_eq!(off["recording"], false);

        let dir = std::env::temp_dir().join(format!("gm-calibration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(false).await),
            yahoo_stream_url: Some(stream_stub().await),
            gdelt_base_url: Some(gdelt_stub(true).await),
            news_base_url: Some(news_stub(false).await),
            data_dir: Some(dir.clone()),
            ..ApiConfig::default()
        })
        .await
        .unwrap();
        // The first recording pass runs at start-up.
        let mut body = serde_json::Value::Null;
        for _ in 0..50 {
            body = get_json(app.clone(), "/api/v1/calibration").await.1;
            if body["n_records"].as_u64().unwrap_or(0) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(body["recording"], true, "{body}");
        let lines = std::fs::read_to_string(dir.join("news.jsonl")).unwrap();
        assert_eq!(
            body["n_records"].as_u64().unwrap() as usize,
            lines.lines().count()
        );
        assert!(lines.lines().count() > 100, "{}", lines.lines().count());
        // Headlines naming NIFTY 50 companies become company events.
        assert!(body["company_events"].as_u64().unwrap() > 0, "{body}");
        assert!(body["after_5_sessions"]["company"].is_array());
        assert_eq!(body["reader"], "best");
        // Too little recorded to fit anything yet: no kind votes on fitted weights.
        assert!(
            body["fitted_weights"]["per_kind"]
                .as_object()
                .unwrap()
                .is_empty(),
            "{body}"
        );
        let (status, _) = get_json(app.clone(), "/api/v1/calibration?reader=nonsense").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, model) = get_json(app.clone(), "/api/v1/calibration?reader=model").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(model["company_events"], 0, "no stored model readings yet");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn desk_reads_market_news_into_asset_leans() {
        let (status, body) = get_json(news_app(false, true).await, "/api/v1/desk").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let sources = body["sources"].as_array().unwrap();
        // Six publisher feeds and three topic searches, each reported.
        assert_eq!(sources.len(), 9, "{sources:?}");
        assert!(sources.iter().all(|s| s["ok"] == true), "{sources:?}");
        let stories = body["stories"].as_array().unwrap();
        assert!(
            stories.len() > 50 && stories.len() <= 150,
            "{}",
            stories.len()
        );
        // Newest first, all within 36 hours of the newest.
        let times: Vec<&str> = stories
            .iter()
            .map(|s| s["seen_at"].as_str().unwrap())
            .collect();
        assert!(times.windows(2).all(|w| w[0] >= w[1]));
        assert!(
            stories
                .iter()
                .any(|s| s["theme"] == "Currency" && !s["impacts"].as_array().unwrap().is_empty())
        );
        let leans = body["leans"].as_array().unwrap();
        assert_eq!(leans.len(), 10);
        assert!(
            leans
                .iter()
                .all(|l| l["symbol"].is_string() && l["score"].is_number())
        );
        assert_eq!(body["reader"], "built-in");
    }

    #[tokio::test]
    async fn news_merges_every_source_and_reports_each() {
        let (status, body) = get_json(
            news_app(false, false).await,
            "/api/v1/news?symbol=RELIANCE.NS",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["query"], "Reliance Industries");
        let sources = body["sources"].as_array().unwrap();
        let count = |name: &str| {
            let s = sources.iter().find(|s| s["name"] == name).unwrap();
            assert_eq!(s["ok"], true, "{s}");
            s["count"].as_u64().unwrap()
        };
        assert_eq!(count("Google News"), 27);
        assert_eq!(count("GDELT"), 3);
        // The recorded ET and Mint feeds don't name Reliance Industries: kept out.
        assert_eq!(count("Economic Times"), 0);
        assert_eq!(count("Mint"), 0);
        let articles = body["articles"].as_array().unwrap();
        // 27 + 3, less one headline Google News carried twice (two outlets, same title).
        assert_eq!(articles.len(), 29);
        let times: Vec<_> = articles
            .iter()
            .map(|a| a["article"]["seen_at"].as_str().unwrap())
            .collect();
        assert!(times.windows(2).all(|w| w[0] >= w[1]), "newest first");
        // GDELT's 05:30 UTC story falls inside the recorded intraday session: its reaction is
        // measured. The stub's NIFTY is the same series, so the move beyond the market is zero.
        let gdelt = articles
            .iter()
            .find(|a| a["article"]["source"] == "GDELT" && !a["impact"].is_null())
            .unwrap();
        assert!(
            gdelt["impact"]["abnormal_move"].as_f64().unwrap().abs() < 1e-9,
            "{gdelt}"
        );
        assert_eq!(body["benchmark"], "^NSEI");
    }

    #[test]
    fn single_stocks_exclude_funds_yahoo_calls_equity() {
        assert!(is_single_stock(
            Some("EQUITY"),
            Some("Reliance Industries Limited")
        ));
        for etf in [
            "Nippon India ETF Gold BeES",
            "Motilal Oswal NASDAQ 100 ETF",
            "Mirae Asset NYSE FANG+ETF",
        ] {
            assert!(!is_single_stock(Some("EQUITY"), Some(etf)), "{etf}");
        }
        for other in [
            "INDEX",
            "ETF",
            "MUTUALFUND",
            "CRYPTOCURRENCY",
            "CURRENCY",
            "FUTURE",
        ] {
            assert!(!is_single_stock(Some(other), None), "{other}");
        }
        assert!(!is_single_stock(None, None));
    }

    #[tokio::test]
    async fn news_is_read_and_votes_in_the_analysis() {
        let app = news_app(false, false).await;
        let (status, body) = get_json(app.clone(), "/api/v1/news?symbol=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["reader"], "built-in");
        // The Supreme Court ruling in the recorded feed reads as relief: a good legal outcome.
        let sc = body["articles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| {
                a["article"]["title"]
                    .as_str()
                    .unwrap()
                    .starts_with("SC Dismisses")
            })
            .unwrap();
        assert_eq!(sc["reading"]["kind"], "legal");
        assert_eq!(sc["reading"]["direction"], 1);
        assert!(body["vote"]["n_stories"].as_u64().unwrap() > 20);

        let (status, analysis) = get_json(app, "/api/v1/analyze?symbol=RELIANCE.NS&range=1y").await;
        assert_eq!(status, StatusCode::OK, "{analysis}");
        // The vote reached the advice (its stance depends on how old the recorded stories are).
        assert!(
            analysis["news_vote"]["n_stories"].as_u64().unwrap() > 20,
            "{}",
            analysis["news_vote"]
        );
        let basis = analysis["advice"]["basis"].as_str().unwrap();
        assert!(
            [
                "proven",
                "conflict",
                "proven_but_quiet",
                "no_evidence",
                "news_driven",
                "news_conflict"
            ]
            .contains(&basis)
        );
    }

    /// An OpenAI-compatible chat stub: reads every headline as bad legal news, or fails.
    async fn llm_stub(fail: bool) -> String {
        let chat = move |Json(request): Json<serde_json::Value>| async move {
            if fail {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({})),
                );
            }
            let lines = request["messages"][1]["content"]
                .as_str()
                .unwrap_or("")
                .lines()
                .count();
            let readings: Vec<_> = (0..lines)
                .map(|_| serde_json::json!({ "kind": "legal", "direction": -1, "materiality": 3, "reason": "stub" }))
                .collect();
            let content = serde_json::to_string(&readings).unwrap();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "choices": [{ "message": { "content": content } }] })),
            )
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/chat/completions", post(chat)),
            )
            .await
            .unwrap()
        });
        format!("http://{addr}/v1")
    }

    /// Many small background requests share full batches: a daily request quota is spent per
    /// call, not per headline (Gemini's free tier: 500 a day).
    #[tokio::test]
    async fn background_reading_merges_small_requests_into_full_batches() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let chat = move |Json(request): Json<serde_json::Value>| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let n = request["messages"][1]["content"]
                    .as_str()
                    .unwrap_or("")
                    .lines()
                    .count();
                let readings: Vec<_> = (0..n)
                    .map(|_| serde_json::json!({ "kind": "legal", "direction": -1, "materiality": 3, "reason": "stub" }))
                    .collect();
                let content = serde_json::to_string(&readings).unwrap();
                Json(serde_json::json!({ "choices": [{ "message": { "content": content } }] }))
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/chat/completions", post(chat)),
            )
            .await
            .unwrap()
        });
        let reader = gm_integrations::llm::LlmReader::new(gm_integrations::llm::LlmConfig {
            base_url: format!("http://{addr}/v1"),
            model: "stub-model".into(),
            api_key: Some("k".into()),
            pace: std::time::Duration::ZERO,
        });
        let titles: Vec<String> = (0..30)
            .map(|i| format!("Company {i} wins an order"))
            .collect();
        for chunk in titles.chunks(3) {
            reader.read_in_background(chunk, false);
        }
        for _ in 0..100 {
            if reader.cached(&titles).iter().all(Option::is_some) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(reader.cached(&titles).iter().all(Option::is_some));
        // 30 headlines from 10 callers: one batch of 25 and one of 5 (the first caller's 3 may
        // start alone before the others arrive), never 10 requests.
        let n = calls.load(Ordering::SeqCst);
        assert!((2..=3).contains(&n), "{n} requests");
    }

    /// Two keys: when the first is refused for its daily limit, the reader moves to the second
    /// and stays there.
    #[tokio::test]
    async fn a_spent_key_hands_over_to_the_next() {
        use axum::http::HeaderMap;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let seen_a = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen_a);
        let chat = move |headers: HeaderMap| {
            let counter = Arc::clone(&counter);
            async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if auth.ends_with("key-a") {
                    counter.fetch_add(1, Ordering::SeqCst);
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(
                            serde_json::json!([{ "error": { "message": "Quota exceeded: requests per day limit" } }]),
                        ),
                    );
                }
                (
                    StatusCode::OK,
                    Json(serde_json::json!({ "choices": [{ "message": { "content": "fine" } }] })),
                )
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/chat/completions", post(chat)),
            )
            .await
            .unwrap()
        });
        let reader = gm_integrations::llm::LlmReader::new(gm_integrations::llm::LlmConfig {
            base_url: format!("http://{addr}/v1"),
            model: "stub-model".into(),
            api_key: Some("key-a, key-b".into()),
            pace: std::time::Duration::ZERO,
        });
        assert_eq!(reader.complete("s", "u").await.unwrap(), "fine");
        assert_eq!(reader.complete("s", "u").await.unwrap(), "fine");
        assert_eq!(
            seen_a.load(Ordering::SeqCst),
            1,
            "the spent key is asked once, then skipped"
        );
    }

    async fn llm_news_app(fail: bool) -> Router {
        build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(false).await),
            yahoo_stream_url: Some(stream_stub().await),
            gdelt_base_url: Some(gdelt_stub(true).await),
            news_base_url: Some(news_stub(false).await),
            llm: Some(gm_integrations::llm::LlmConfig {
                base_url: llm_stub(fail).await,
                model: "stub-model".into(),
                api_key: Some("k".into()),
                pace: std::time::Duration::ZERO,
            }),
            ..ApiConfig::default()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_configured_model_reads_the_news_and_falls_back_when_it_fails() {
        // The first request is answered at once by the built-in reader while the model reads
        // in the background; later requests carry the model's readings.
        let app = llm_news_app(false).await;
        let mut body = serde_json::Value::Null;
        for _ in 0..50 {
            let (status, b) = get_json(app.clone(), "/api/v1/news?symbol=RELIANCE.NS").await;
            assert_eq!(status, StatusCode::OK, "{b}");
            body = b;
            if body["articles"][0]["reading"]["reader"] == "stub-model" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(body["reader"], "stub-model");
        let first = &body["articles"][0]["reading"];
        assert_eq!(
            (first["kind"].as_str(), first["reader"].as_str()),
            (Some("legal"), Some("stub-model"))
        );

        let (status, body) =
            get_json(llm_news_app(true).await, "/api/v1/news?symbol=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // The model failed: every headline was still read, by the built-in reader.
        assert!(
            body["articles"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["reading"]["reader"] == "built-in")
        );
    }

    #[tokio::test]
    async fn news_survives_a_failed_source_and_503s_only_when_all_fail() {
        let (status, body) = get_json(
            news_app(true, true).await,
            "/api/v1/news?symbol=RELIANCE.NS",
        )
        .await;
        // Google answered a block page and GDELT a rate limit; ET and Mint still worked.
        assert_eq!(status, StatusCode::OK, "{body}");
        let google = body["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "Google News")
            .unwrap()
            .clone();
        assert_eq!(google["ok"], false);
        assert!(
            google["error"]
                .as_str()
                .unwrap()
                .contains("unusual traffic"),
            "{google}"
        );

        // Every source down: a 503 that carries each reason.
        let app = build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(false).await),
            yahoo_stream_url: Some(stream_stub().await),
            gdelt_base_url: Some(gdelt_stub(true).await),
            news_base_url: Some("http://127.0.0.1:9".to_string()),
            ..ApiConfig::default()
        })
        .await
        .unwrap();
        let (status, body) = get_json(app.clone(), "/api/v1/news?symbol=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("one every 5 seconds"),
            "{body}"
        );
        // After a refusal GDELT is skipped for a while instead of being asked again (and
        // paced) on every request.
        let started = std::time::Instant::now();
        let (_, body) = get_json(app, "/api/v1/news?symbol=RELIANCE.NS").await;
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("gdelt skipped for"),
            "{body}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    async fn market_app(fail: bool, access_token: Option<&str>) -> Router {
        build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(fail).await),
            yahoo_stream_url: Some(stream_stub().await),
            // No network in tests: every news source fails fast, so advice ignores news.
            gdelt_base_url: Some(gdelt_stub(true).await),
            news_base_url: Some("http://127.0.0.1:9".to_string()),
            access_token: access_token.map(str::to_string),
            ..ApiConfig::default()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn bars_return_adjusted_history_with_quality_report() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/bars?symbol=RELIANCE.NS&range=1y",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let bars = body["chart"]["bars"].as_array().unwrap();
        assert_eq!(bars.len(), 248);
        assert!(bars.iter().all(|b| b["adj_close"].as_f64().unwrap() > 0.0));
        assert_eq!(body["chart"]["source"], "yahoo-chart-v8");
        assert!(body["chart"]["fetched_at"].is_string());
        assert_eq!(body["chart"]["corporate_actions"][0]["kind"], "dividend");
        assert_eq!(body["quality"]["passed"], true);
        assert_eq!(body["quality"]["n_bars"], 248);
        assert_eq!(body["quality"]["missing_days"], serde_json::json!([]));
        assert_eq!(body["quality"]["is_stale"], false);
    }

    #[tokio::test]
    async fn quote_returns_delayed_last_price_and_market_state() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/quote?symbol=reliance.ns",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["quote"]["symbol"], "RELIANCE.NS");
        assert_eq!(body["quote"]["currency"], "INR");
        assert!(body["quote"]["last_price"].as_f64().unwrap() > 0.0);
        assert_eq!(body["quote"]["market_open"], false);
        assert_eq!(body["delayed"], true);
    }

    #[tokio::test]
    async fn intraday_returns_minute_points_and_rejects_bad_ranges() {
        let app = market_app(false, None).await;
        let (status, body) = get_json(app.clone(), "/api/v1/intraday?symbol=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let chart = &body["chart"];
        assert_eq!(chart["interval"], "1m");
        assert_eq!(chart["points"].as_array().unwrap().len(), 362);
        assert!((chart["previous_close"].as_f64().unwrap() - 1373.9).abs() < 1e-9);
        assert_eq!(chart["source"], "yahoo-chart-v8");
        assert!(chart["fetched_at"].is_string());

        let (status, body) =
            get_json(app.clone(), "/api/v1/intraday?symbol=RELIANCE.NS&range=1y").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let (status, body) = get_json(app, "/api/v1/intraday?symbol=NOPE.NS").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }

    #[tokio::test]
    async fn unknown_symbol_is_404_and_upstream_failure_is_503() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/quote?symbol=NOPE.NS",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

        let failing = market_app(true, None).await;
        let (status, body) = get_json(failing.clone(), "/api/v1/bars?symbol=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(body["error"].as_str().unwrap().contains("500"));
        let (status, _) = get_json(failing, "/api/v1/instruments/search?q=nifty").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn search_filters_by_country_and_exchange_and_finds_bse_twins() {
        let app = market_app(false, None).await;
        let (status, body) = get_json(app.clone(), "/api/v1/instruments/search?q=reliance").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let symbols: Vec<_> = body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["symbol"].as_str().unwrap().to_string())
            .collect();
        // Yahoo returned NSE only; the BSE twin was checked against the quote endpoint and
        // added, while twins that don't exist (the stub 404s them) were dropped.
        assert!(symbols.contains(&"RELIANCE.BO".to_string()), "{symbols:?}");
        assert!(!symbols.contains(&"RPOWER.BO".to_string()), "{symbols:?}");
        let markets = body["markets"].as_array().unwrap();
        assert!(
            markets
                .iter()
                .any(|m| m["exchange"] == "BSE" && m["country"] == "India")
        );

        let (_, bse) = get_json(
            app.clone(),
            "/api/v1/instruments/search?q=reliance&country=india&exchange=BSE",
        )
        .await;
        let bse = bse["results"].as_array().unwrap();
        assert!(
            !bse.is_empty() && bse.iter().all(|r| r["exchange"] == "BSE"),
            "{bse:?}"
        );

        // A typed exact ticker is resolved even if name search would not find it.
        let (_, typed) = get_json(app.clone(), "/api/v1/instruments/search?q=RELIANCE.BO").await;
        assert!(
            typed["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["symbol"] == "RELIANCE.BO")
        );

        // Another country: India's listings are filtered out, US ones kept.
        let (_, us) = get_json(app, "/api/v1/instruments/search?q=nifty&country=USA").await;
        let us = us["results"].as_array().unwrap();
        assert!(
            !us.is_empty() && us.iter().all(|r| r["country"] == "USA"),
            "{us:?}"
        );
    }

    #[tokio::test]
    async fn search_covers_every_exchange() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/instruments/search?q=nifty",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let results = body["results"].as_array().unwrap();
        assert!(!results.is_empty());
        // Global search: a US-listed NIFTY ETF is a valid hit alongside NSE listings.
        assert!(results.iter().any(|r| r["symbol"] == "NFTY"));
    }

    #[tokio::test]
    async fn market_data_is_behind_the_access_gate() {
        let app = market_app(false, Some("beta-secret")).await;
        let (status, _) = status_of(
            app.clone(),
            get_req("/api/v1/quote?symbol=RELIANCE.NS")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = status_of(
            app,
            get_req("/api/v1/quote?symbol=RELIANCE.NS")
                .header("authorization", "Bearer beta-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn stream_rejects_bad_symbol_lists() {
        let too_many = (0..21)
            .map(|i| format!("S{i}.NS"))
            .collect::<Vec<_>>()
            .join(",");
        for uri in [
            "/api/v1/stream?symbols=".to_string(),
            "/api/v1/stream?symbols=..%2Fetc".to_string(),
            format!("/api/v1/stream?symbols={too_many}"),
        ] {
            let (status, payload) = get_json(app().await, &uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {payload}");
        }
    }

    #[tokio::test]
    async fn stream_relays_decoded_ticks_and_survives_upstream_reconnects() {
        use futures_util::StreamExt;
        let response = market_app(false, None)
            .await
            .oneshot(
                get_req("/api/v1/stream?symbols=ETH-USD")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let mut body = response.into_body().into_data_stream();
        let mut text = String::new();
        // The stub hangs up after each replay, so a second tick proves the reconnect (~1 s backoff).
        let read = async {
            while text.matches("event: tick").count() < 2 {
                let chunk = body.next().await.unwrap().unwrap();
                text.push_str(std::str::from_utf8(&chunk).unwrap());
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), read)
            .await
            .expect("two ticks within 10 s");
        let first = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let tick: Value = serde_json::from_str(first).unwrap();
        assert_eq!(tick["symbol"], "ETH-USD");
        assert!(tick["price"].as_f64().unwrap() > 1000.0);
        assert_eq!(tick["source"], "yahoo-streamer");
        assert!(
            !text.contains("EURUSD"),
            "unsubscribed symbols are filtered out"
        );
    }

    /// Regression: gm-api links both rustls backends; without an explicit provider the first
    /// wss:// handshake to Yahoo panicked and killed the live stream.
    #[test]
    fn tls_client_config_builds_with_every_linked_backend() {
        gm_integrations::yahoo_stream::install_crypto_provider();
        let _ = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
    }

    // ---- /api/v1/analyze --------------------------------------------------------------------

    #[tokio::test]
    async fn analyze_reports_on_any_symbol_with_auto_benchmark_and_provenance() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/analyze?symbol=RELIANCE.NS&range=1y",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let report = &body["report"];
        assert_eq!(report["symbol"], "RELIANCE.NS");
        assert_eq!(report["n_returns"], 247);
        assert!(report["distribution"]["vol_annual"].as_f64().unwrap() > 0.0);
        assert!(report["predictability"]["ljung_box_returns"]["p_value"].is_number());
        assert!(!report["findings"].as_array().unwrap().is_empty());
        // Plain words: three answers and an action, never a bare "Hold".
        assert_eq!(
            body["plain"]["answers"].as_array().unwrap().len(),
            3,
            "{body}"
        );
        assert_ne!(body["plain"]["headline"], "Hold");
        // 247 returns is below GARCH's 250 minimum: EWMA and GBM only, said plainly.
        assert!(report["volatility"]["garch"].is_null());
        assert!(
            report["validation"].is_null(),
            "1y is too short to validate"
        );
        // No GARCH on 247 returns: GBM plus the bootstrap of the stock's own history.
        let models: Vec<_> = report["projections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["model"].as_str().unwrap())
            .collect();
        assert_eq!(models, ["gbm", "bootstrap"]);
        assert_eq!(
            report["projections"][0]["fan"].as_array().unwrap().len(),
            63
        );
        // Home-market benchmark chosen automatically; the stub serves identical data, so beta = 1.
        assert_eq!(body["benchmark"]["symbol"], "^NSEI");
        assert_eq!(body["benchmark"]["selection"], "auto");
        assert_eq!(body["benchmark"]["used"], true);
        assert!((report["benchmark"]["beta"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(body["data"]["tradingview_symbol"], "NSE:RELIANCE");
        assert_eq!(body["data"]["quality"]["passed"], true);
        assert_eq!(body["data"]["source"], "yahoo-chart-v8");

        // Trading rules run on 10 years of real RELIANCE closes; advice always comes with reasons.
        let signals = &body["signals"];
        assert!(signals["n_days"].as_u64().unwrap() > 2400, "{signals}");
        let ids: Vec<_> = signals["signals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&"trend_200d") && ids.contains(&"pressure_obv"),
            "{ids:?}"
        );
        let advice = &body["advice"];
        assert!(["buy", "sell", "hold"].contains(&advice["stance"].as_str().unwrap()));
        assert_eq!(advice["reasons"].as_array().unwrap().len(), ids.len());
        assert!(body["signals_note"].is_null());
        // No news source answered: no vote, and the rules' advice stands on its own.
        assert!(body["news_vote"].is_null());
        assert!(advice["news_headlines"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn today_scans_the_universe_and_reports_each_instrument() {
        let app = market_app(false, None).await;
        let (status, body) = get_json(app.clone(), "/api/v1/today?extra=RELIANCE.NS").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let items = body["items"].as_array().unwrap();
        // NIFTY 50 and 15 other assets; RELIANCE.NS is already in, so not added twice.
        assert_eq!(items.len(), 65);
        let reliance = items.iter().find(|i| i["symbol"] == "RELIANCE.NS").unwrap();
        assert_eq!(reliance["group"], "NIFTY 50");
        assert!(
            reliance["prob_gain_3m"].as_f64().unwrap() > 0.0,
            "{reliance}"
        );
        assert!(reliance["advice"]["stance"].is_string(), "{reliance}");
        // Instruments the stub doesn't know are reported, not dropped.
        assert!(items.iter().any(|i| i["error"].is_string()));
        assert!(
            body["notice"]
                .as_str()
                .unwrap()
                .contains("Not SEBI-registered")
        );
        // Cached: the second scan is instant and identical.
        let (_, again) = get_json(app.clone(), "/api/v1/today?extra=RELIANCE.NS").await;
        assert_eq!(
            reliance,
            again["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|i| i["symbol"] == "RELIANCE.NS")
                .unwrap()
        );
        let (status, _) = get_json(app.clone(), "/api/v1/today?extra=bad%20symbol").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let many = (0..21)
            .map(|i| format!("X{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, _) = get_json(app, &format!("/api/v1/today?extra={many}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn live_projection_starts_at_the_live_price_and_rejects_nonsense() {
        let app = market_app(false, None).await;
        let (_, base) = get_json(app.clone(), "/api/v1/bars?symbol=RELIANCE.NS&range=10y").await;
        let bars = base["chart"]["bars"].as_array().unwrap();
        let last = bars.last().unwrap()["adj_close"].as_f64().unwrap();
        let live = last * 1.03;
        let (status, body) = get_json(
            app.clone(),
            &format!("/api/v1/projection?symbol=RELIANCE.NS&price={live}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["last_close"].as_f64().unwrap(), last);
        let projections = body["projections"].as_array().unwrap();
        let models: Vec<_> = projections
            .iter()
            .map(|p| p["model"].as_str().unwrap())
            .collect();
        assert_eq!(models, ["gbm", "garch_t", "bootstrap"]);
        // Day-1 median sits near the live price, not yesterday's close.
        let median = projections[2]["fan"][0]["price"][2].as_f64().unwrap();
        assert!((median / live - 1.0).abs() < 0.01, "{median} vs {live}");
        assert_eq!(projections[2]["typical_path"].as_array().unwrap().len(), 63);
        // Same price twice: identical (cached fit, seeded draws).
        let (_, again) = get_json(
            app.clone(),
            &format!("/api/v1/projection?symbol=RELIANCE.NS&price={live}"),
        )
        .await;
        assert_eq!(body["projections"], again["projections"]);
        for bad in ["0", "-5", "NaN", &format!("{}", last * 3.0)] {
            let (status, _) = get_json(
                app.clone(),
                &format!("/api/v1/projection?symbol=RELIANCE.NS&price={bad}"),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "price={bad}");
        }
    }

    #[tokio::test]
    async fn analyze_is_deterministic_per_seed_and_benchmark_is_optional() {
        let app = market_app(false, None).await;
        let uri = "/api/v1/analyze?symbol=RELIANCE.NS&range=1y&benchmark=none&seed=9&paths=500";
        let (_, a) = get_json(app.clone(), uri).await;
        let (_, b) = get_json(app.clone(), uri).await;
        assert_eq!(a["report"]["projections"], b["report"]["projections"]);
        assert!(a["benchmark"].is_null() && a["report"]["benchmark"].is_null());
        assert_eq!(a["report"]["projections"][0]["n_paths"], 500);
        let (_, c) = get_json(app, &uri.replace("seed=9", "seed=10")).await;
        assert_ne!(a["report"]["projections"], c["report"]["projections"]);
    }

    #[tokio::test]
    async fn analyze_rejects_bad_input_and_maps_unknown_symbols() {
        let app = market_app(false, None).await;
        for uri in [
            "/api/v1/analyze?symbol=RELIANCE.NS&paths=5",
            "/api/v1/analyze?symbol=RELIANCE.NS&paths=50000",
            "/api/v1/analyze?symbol=RELIANCE.NS&range=1h",
            "/api/v1/analyze?symbol=..%2Fetc",
        ] {
            let (status, body) = get_json(app.clone(), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        }
        let (status, _) = get_json(app.clone(), "/api/v1/analyze?symbol=NOPE.NS").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // A benchmark the caller asked for must exist; an automatic one is best-effort.
        let (status, _) = get_json(
            app,
            "/api/v1/analyze?symbol=RELIANCE.NS&range=1y&benchmark=NOPE.NS",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // ---- /api/v1/compare --------------------------------------------------------------------

    #[tokio::test]
    async fn compare_returns_correlations_pairs_and_provenance() {
        let (status, body) = get_json(
            market_app(false, None).await,
            "/api/v1/compare?symbols=RELIANCE.NS,%5ENSEI,RELIANCE.NS&range=1y",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let c = &body["comparison"];
        // Duplicates are dropped; the stub serves identical data for both, so correlation is 1.
        assert_eq!(c["symbols"].as_array().unwrap().len(), 2);
        assert!((c["correlation_daily"][0][1].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(c["n_common_days"], 248);
        assert_eq!(c["pairs"].as_array().unwrap().len(), 1);
        assert!(!c["findings"].as_array().unwrap().is_empty());
        assert_eq!(body["instruments"][0]["source"], "yahoo-chart-v8");
        assert_eq!(
            body["notes"],
            serde_json::json!([]),
            "same currency and time zone"
        );
    }

    #[tokio::test]
    async fn compare_validates_symbol_count_and_names_unknown_symbols() {
        let app = market_app(false, None).await;
        let eleven = (0..11)
            .map(|i| format!("S{i}.NS"))
            .collect::<Vec<_>>()
            .join(",");
        for uri in [
            "/api/v1/compare?symbols=RELIANCE.NS".to_string(),
            "/api/v1/compare?symbols=RELIANCE.NS,RELIANCE.NS".to_string(),
            format!("/api/v1/compare?symbols={eleven}"),
            "/api/v1/compare?symbols=RELIANCE.NS,..%2Fetc".to_string(),
        ] {
            let (status, body) = get_json(app.clone(), &uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        }
        let (status, body) = get_json(app, "/api/v1/compare?symbols=RELIANCE.NS,NOPE.NS").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body["error"].as_str().unwrap().starts_with("NOPE.NS:"),
            "{body}"
        );
    }
}
