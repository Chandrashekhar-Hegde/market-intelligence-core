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
    AsOfFacts, DECISION_MODEL_VERSION, DecisionInput, DecisionThresholds, EntityType, EventClass,
    MacroContext, NormalizedEvent, PriceBar, RuleRegistry, build_macro_context, classify,
    compute_features, decide, gbm_flow_prediction, score_event,
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
        }
    }
}

#[derive(Clone)]
struct AppState {
    registry: Arc<RuleRegistry>,
    thresholds: DecisionThresholds,
    store: Option<PgStore>,
    payment_provider: Arc<MockPaymentProvider>,
    yahoo: YahooClient,
    stream: YahooStream,
    access_token: Option<Arc<str>>,
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
struct DecideRequest {
    event: NormalizedEvent,
    facts: Option<AsOfFacts>,
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
        registry: Arc::new(RuleRegistry::builtin()),
        thresholds: DecisionThresholds::default(),
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
        access_token: config
            .access_token
            .filter(|token| !token.trim().is_empty())
            .map(Arc::from),
    };
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
        .route("/decide", post(decide_route))
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
        .route("/api/v1/stream", get(stream))
        .route("/api/v1/analyze", get(analyze_instrument))
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
    let hits = state
        .yahoo
        .search(&query.q)
        .await
        .map_err(ApiError::provider)?;
    Ok(Json(serde_json::json!({ "results": hits })))
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
struct AnalyzeQuery {
    symbol: String,
    range: Option<String>,
    /// "auto" (default): home-market index, S&P 500 for US, MSCI ACWI otherwise;
    /// "none": skip exposure; or any symbol.
    benchmark: Option<String>,
    seed: Option<u64>,
    paths: Option<usize>,
}

/// Full single-instrument analysis for any symbol Yahoo knows: distribution, predictability,
/// volatility (EWMA, GARCH-t), Monte Carlo fan charts, VaR/ES, drawdown, and benchmark
/// exposure when a benchmark applies.
async fn analyze_instrument(
    State(state): State<AppState>,
    Query(query): Query<AnalyzeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use gm_domain::analysis::{AnalysisInput, DEFAULT_HORIZONS, DEFAULT_PATHS, MAX_PATHS, analyze};
    let range = query.range.as_deref().unwrap_or("5y");
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
    let symbol = meta.symbol.clone();
    let bench_symbol = benchmark.as_ref().map(|(s, _)| s.clone());
    let report = tokio::task::spawn_blocking(move || {
        analyze(&AnalysisInput {
            symbol: &symbol,
            prices: &prices,
            benchmark: bench_symbol.as_deref().zip(bench_prices.as_deref()),
            horizons: &DEFAULT_HORIZONS,
            n_paths: paths,
            seed,
        })
    })
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
    Ok(Json(serde_json::json!({
        "report": report,
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

fn quality_report(
    chart: &ChartData,
    reference_days: Option<&[chrono::NaiveDate]>,
) -> gm_domain::quality::QualityReport {
    let action_days = chart
        .corporate_actions
        .iter()
        .map(|a| a.date())
        .collect::<Vec<_>>();
    gm_domain::quality::assess(
        &chart.bars,
        reference_days,
        &action_days,
        chart.fetched_at.date_naive(),
    )
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

async fn decide_route(
    State(state): State<AppState>,
    Json(request): Json<DecideRequest>,
) -> Result<Json<gm_domain::Decision>, ApiError> {
    let score = score_event(&request.event, &state.registry);
    let input = DecisionInput {
        event: request.event,
        score,
        facts: request.facts.unwrap_or_default(),
        thresholds: state.thresholds,
    };
    let decision = decide(input.clone());

    if let Some(store) = state.store.as_ref() {
        store
            .save_decision_audit(&input, &decision, DECISION_MODEL_VERSION)
            .await
            .map_err(ApiError::persistence)?;
    }

    Ok(Json(decision))
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
                    "summary": "Search NSE/BSE stocks and indices (Yahoo search)",
                    "parameters": [{ "name": "q", "in": "query", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Matching NSE/BSE listings" },
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
                        { "name": "paths", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 100, "maximum": 20000, "default": 2000 } }
                    ],
                    "responses": {
                        "200": { "description": "report (distribution, predictability, volatility, projections, risk, benchmark exposure, findings), data provenance and quality, benchmark choice" },
                        "400": { "description": "Invalid input or too little history" },
                        "404": { "description": "Unknown symbol or requested benchmark" },
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
                    "summary": "Create a deterministic decision from an event and as-of facts",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/DecideRequest" }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Decision" },
                        "500": { "description": "Persistence failed when configured" }
                    }
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
                "DecideRequest": {
                    "type": "object",
                    "required": ["event"],
                    "properties": {
                        "event": { "$ref": "#/components/schemas/NormalizedEvent" },
                        "facts": { "type": ["object", "null"] }
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
    async fn decide_contract_returns_deterministic_buy() {
        let (status, payload) =
            json_request(app().await, "POST", "/decide", decision_request()).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["action"], "BUY");
        assert_eq!(payload["execution_ready"], true);
        assert_eq!(payload["quantity"], 20);
        assert_eq!(payload["target_price"], 1030.0);
        assert_eq!(payload["stop_loss"], 980.0);
        assert_eq!(payload["total_score"], 0.72);
        assert_eq!(payload["model_version"], DECISION_MODEL_VERSION);
        assert!(payload["input_hash"].as_str().unwrap().len() > 20);
        assert!(payload["explanation"]["utilities"].is_array());
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
    const FRAMES: &str =
        include_str!("../../gm-integrations/tests/fixtures/yahoo_stream_frames.txt");

    /// Recorded chart/search responses on an ephemeral port. `fail` answers 500 to everything.
    async fn yahoo_stub(fail: bool) -> String {
        let chart = move |Path(symbol): Path<String>| async move {
            match (fail, symbol.as_str()) {
                (true, _) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
                // ^NSEI doubles as the trading calendar; the RELIANCE fixture has the same days.
                (false, "RELIANCE.NS" | "^NSEI") => (StatusCode::OK, CHART.to_string()),
                (false, _) => (StatusCode::NOT_FOUND, NOT_FOUND.to_string()),
            }
        };
        let search = move || async move {
            if fail {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new())
            } else {
                (StatusCode::OK, SEARCH.to_string())
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

    async fn market_app(fail: bool, access_token: Option<&str>) -> Router {
        build_app(ApiConfig {
            yahoo_base_url: Some(yahoo_stub(fail).await),
            yahoo_stream_url: Some(stream_stub().await),
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
        // 247 returns is below GARCH's 250 minimum: EWMA and GBM only, said plainly.
        assert!(report["volatility"]["garch"].is_null());
        assert_eq!(report["projections"].as_array().unwrap().len(), 1);
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
}
