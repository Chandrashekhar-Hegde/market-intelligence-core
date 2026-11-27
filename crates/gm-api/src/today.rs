//! The daily list: every instrument in the universe (NIFTY 50, gold, silver, the rupee, US and
//! AI names, and the user's watchlist) with its call, its chances and the evidence for both.
//! Research signals for a private beta, not registered advice (owner decision, 27 Nov 2026).
//! Each entry is computed like `/analyze` (10 years, no benchmark) and cached for 30 minutes;
//! a background pass keeps the universe warm. Company news comes from the desk's feeds, so the
//! list makes no per-company news searches.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Query, State},
};
use futures_util::StreamExt;
use gm_domain::analysis::{
    AnalysisInput, DEFAULT_HORIZONS, analyze,
    news_reader::NewsVote,
    signals::{NewsInput, advise_with_news},
};
use gm_integrations::universe::{NIFTY50, OTHER_ASSETS, companies_named};
use serde::Deserialize;

use super::{
    ApiError, AppState, desk_stories, is_single_stock, read_stories, trading_signals,
    validate_symbol, vote_on,
};

pub const TTL: Duration = Duration::from_secs(30 * 60);
/// Instruments computed at once: the maths is CPU-bound and Yahoo dislikes bursts.
const PARALLEL: usize = 4;
const MAX_EXTRA: usize = 20;
const PATHS: usize = 1_000;

pub type Cache = Arc<Mutex<HashMap<String, (Instant, serde_json::Value)>>>;

#[derive(Debug, Deserialize)]
pub struct TodayQuery {
    /// Comma-separated watchlist symbols to include besides the universe.
    extra: Option<String>,
}

fn universe() -> Vec<(String, String)> {
    NIFTY50
        .iter()
        .map(|(s, _)| (s.to_string(), "NIFTY 50".to_string()))
        .chain(
            OTHER_ASSETS
                .iter()
                .map(|(s, g)| (s.to_string(), g.to_string())),
        )
        .collect()
}

/// Today's news vote per NIFTY 50 company, from the desk's headlines that name it.
async fn company_news(state: &AppState) -> HashMap<String, (NewsVote, Vec<String>)> {
    let (_, stories) = desk_stories(state).await;
    let mut by_symbol: HashMap<String, Vec<gm_integrations::news::Story>> = HashMap::new();
    for story in stories {
        for symbol in companies_named(&story.title) {
            by_symbol
                .entry(symbol.to_string())
                .or_default()
                .push(story.clone());
        }
    }
    let mut out = HashMap::new();
    for (symbol, stories) in by_symbol {
        // read_stories never waits on the model, so this loop stays fast.
        let readings = read_stories(state, &stories).await;
        out.insert(symbol, vote_on(state, &stories, &readings));
    }
    out
}

async fn entry(
    state: &AppState,
    symbol: &str,
    group: &str,
    news: &HashMap<String, (NewsVote, Vec<String>)>,
) -> Result<serde_json::Value, String> {
    let chart = state
        .yahoo
        .chart(symbol, "10y")
        .await
        .map_err(|e| e.to_string())?;
    let meta = chart.meta.clone();
    let prices: Vec<_> = chart
        .bars
        .iter()
        .map(|b| (b.date, b.adjusted_close()))
        .collect();
    let sym = meta.symbol.clone();
    // A single stock's long-run growth is its home index's (see long_run.rs).
    let index = match is_single_stock(meta.instrument_type.as_deref(), meta.name.as_deref())
        .then(|| {
            gm_integrations::markets::default_benchmark(
                &meta.symbol,
                meta.instrument_type.as_deref(),
                meta.currency.as_deref(),
            )
        })
        .flatten()
    {
        Some((bench, _)) => state.yahoo.chart(bench, "10y").await.ok().map(|c| {
            let closes: Vec<_> = c
                .bars
                .iter()
                .map(|b| (b.date, b.adjusted_close()))
                .collect();
            (bench.to_string(), closes)
        }),
        None => None,
    };
    let report = tokio::task::spawn_blocking(move || {
        analyze(&AnalysisInput {
            symbol: &sym,
            prices: &prices,
            benchmark: None,
            horizons: &DEFAULT_HORIZONS,
            n_paths: PATHS,
            seed: 42,
            long_run_drift_from: index.as_ref().map(|(s, p)| (s.as_str(), p.as_slice())),
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let long = Arc::clone(&chart);
    let signals = tokio::task::spawn_blocking(move || trading_signals(&long))
        .await
        .ok()
        .flatten();
    let single = is_single_stock(meta.instrument_type.as_deref(), meta.name.as_deref());
    let vote = single.then(|| news.get(&meta.symbol)).flatten();
    let advice = signals.as_ref().map(|(signals, every_day)| {
        let input = vote.map(|(vote, headlines)| NewsInput { vote, headlines });
        advise_with_news(signals, *every_day, input.as_ref())
    });
    let lead = report
        .projections
        .iter()
        .find(|p| p.model == report.lead_model)
        .or(report.projections.first());
    let horizon = |h: usize| lead.and_then(|p| p.horizons.iter().find(|x| x.horizon_days == h));
    let check = report
        .validation
        .as_ref()
        .and_then(|v| v.chance_of_gain.iter().find(|c| c.horizon_days == 63));
    let closes: Vec<f64> = chart.bars.iter().map(|b| b.adjusted_close()).collect();
    let back = |n: usize| {
        let last = *closes.last()?;
        let then = *closes.get(closes.len().checked_sub(n + 1)?)?;
        Some(last / then - 1.0)
    };
    let year = (report.periods_per_year.round() as usize).max(1);
    // 3 and 5 years only, rounded: enough for the page to compare with any deposit rate.
    let round = |v: &[f64]| {
        v.iter()
            .map(|x| (x * 1e4).round() / 1e4)
            .collect::<Vec<_>>()
    };
    let long_run: Vec<_> = report
        .long_run
        .iter()
        .flatten()
        .filter(|l| l.years >= 3)
        .map(|l| {
            serde_json::json!({
                "years": l.years,
                "resampled": round(&l.resampled),
                "historical": round(&l.historical),
                "n_windows": l.n_windows,
                "n_independent": l.n_independent,
                "worst": l.worst,
                "block_length": l.block_length,
                "drift_annual": l.drift_annual,
                "drift_se_annual": l.drift_se_annual,
                "drift_from": l.drift_from,
            })
        })
        .collect();
    let vol_ratio = report
        .volatility
        .garch
        .as_ref()
        .filter(|g| g.long_run_vol_annual > 0.0)
        .map(|g| g.current_vol_annual / g.long_run_vol_annual);
    let es95 = report
        .risk
        .var_es
        .iter()
        .find(|v| v.method == "historical" && (v.confidence - 0.95).abs() < 1e-9)
        .map(|v| v.es);
    Ok(serde_json::json!({
        "symbol": meta.symbol,
        "name": meta.name,
        "group": group,
        "currency": meta.currency,
        "instrument_type": meta.instrument_type,
        "last_price": meta.last_price.unwrap_or(report.last_price),
        "change_pct": meta.change_pct,
        "last_date": report.last_date,
        "return_1y": back(year),
        "vol_annual": report.distribution.vol_annual,
        "lead_model": report.lead_model,
        "prob_gain_1m": horizon(21).map(|h| h.prob_gain),
        "prob_gain_3m": horizon(63).map(|h| h.prob_gain),
        "prob_loss_10pct_3m": horizon(63).map(|h| h.prob_loss_over_10pct),
        "range_3m": horizon(63).map(|h| [h.return_quantiles[0], h.return_quantiles[4]]),
        "chance_check_3m": check,
        "long_run": long_run,
        "single_stock": single,
        "vol_ratio": vol_ratio,
        "es95": es95,
        "advice": advice,
        "news_vote": vote.map(|(v, _)| v),
    }))
}

/// Cached entry, or a fresh one (errors are returned as an entry with `error`, not cached).
async fn cached_entry(
    state: &AppState,
    symbol: &str,
    group: &str,
    news: &HashMap<String, (NewsVote, Vec<String>)>,
) -> serde_json::Value {
    if let Some((at, value)) = state.today.lock().expect("today lock").get(symbol)
        && at.elapsed() < TTL
    {
        return value.clone();
    }
    match entry(state, symbol, group, news).await {
        Ok(value) => {
            state
                .today
                .lock()
                .expect("today lock")
                .insert(symbol.to_string(), (Instant::now(), value.clone()));
            value
        }
        Err(error) => serde_json::json!({ "symbol": symbol, "group": group, "error": error }),
    }
}

async fn scan(state: &AppState, list: Vec<(String, String)>) -> Vec<serde_json::Value> {
    let news = company_news(state).await;
    let news = &news;
    futures_util::stream::iter(list)
        .map(|(symbol, group)| async move { cached_entry(state, &symbol, &group, news).await })
        .buffered(PARALLEL)
        .collect()
        .await
}

/// Keeps the universe warm: one pass at start-up, then one per cache period.
pub fn spawn_warmer(state: AppState) {
    tokio::spawn(async move {
        loop {
            let started = Instant::now();
            let items = scan(&state, universe()).await;
            let failed = items.iter().filter(|i| i.get("error").is_some()).count();
            tracing::info!(
                n = items.len(),
                failed,
                secs = started.elapsed().as_secs(),
                "today list refreshed"
            );
            tokio::time::sleep(TTL.saturating_sub(Duration::from_secs(60))).await;
        }
    });
}

pub async fn today(
    State(state): State<AppState>,
    Query(query): Query<TodayQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut list = universe();
    let extra = query
        .extra
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(validate_symbol)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::provider)?;
    if extra.len() > MAX_EXTRA {
        return Err(ApiError::bad_request(format!(
            "extra lists at most {MAX_EXTRA} symbols"
        )));
    }
    for symbol in extra {
        if list.iter().all(|(s, _)| *s != symbol) {
            list.push((symbol, "Your watchlist".to_string()));
        }
    }
    let items = scan(&state, list).await;
    Ok(Json(serde_json::json!({
        "as_of": chrono::Utc::now(),
        "items": items,
        "notice": "Research signals from past data, for a private beta. Not SEBI-registered investment advice: the app advises, you decide.",
    })))
}
