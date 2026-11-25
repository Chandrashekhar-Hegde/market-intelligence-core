<!-- Status header added 2026-11-25; the plan text below is unchanged from what was approved on 2026-11-22. -->
> **Status: Phase 0 shipped (0.1–0.5).** Remaining Phase 0 items: #45 (bhavcopy fallback),
> #71 (replace chrono), #9 (ops status panel), #94 (guide links). Phases 1 and 2 are now detailed
> in [phase-1-events-and-fundamentals.md](phase-1-events-and-fundamentals.md) and
> [phase-2-portfolio.md](phase-2-portfolio.md).
>
> **What changed against this plan while building it** (reasons are in [docs/HISTORY.md](../HISTORY.md)):
> - *India first* became *any exchange*: a per-market benchmark table, with MSCI ACWI as the fallback.
> - *Postgres read-through cache for bars* was dropped. Adjusted prices are restated after
>   dividends, so cached rows would go stale; an in-memory TTL cache is used instead.
> - *`POST /api/v1/analyze {symbols:[...]}`* became `GET /api/v1/analyze?symbol=` for one
>   symbol, plus `GET /api/v1/compare?symbols=` for several.
> - *nalgebra for PCA* was not needed: a cyclic Jacobi rotation handles matrices up to 10×10.
> - *Real-time data:* we measured that Yahoo streams NSE in about 2 s, while BSE is 15 min
>   delayed (#44). TradingView has no data API; it is display-only.
>
> **Shipped in:** #72 (0.1), #73 (0.2), #74 (0.3), #76 (live labels), #77 (0.4 and the guide),
> #78 (search fix).

# Market Intelligence Core: from fixture console to a live quant analysis app

## Context

The goal is a hosted app. In Phase 0, a user types any NSE/BSE stock or index, or several at once, and gets analysis built on real data. The analysis must be honest about uncertainty: forecasts come as probability distributions, every statistical test is shown, and every model is scored against a baseline. Phase 2 adds connecting a broker portfolio.

Decisions already made: **India first**, **free end-of-day data plus delayed quotes**, **Railway hosting**, and **private beta** (one shared access gate, no user accounts yet).

### What the repo is today (from scanning it)

- **The maths primitives are sound and reusable.** They live in `crates/gm-domain`:
  - `indicators.rs`: log returns, sample variance/covariance, beta, ATR, RSI, drawdown, z-score.
  - `stochastic.rs`: GBM calibration and forecast with correct lognormal mean `exp(μh + σ²h/2) − 1`, and Acklam's inverse normal.
  - `event_study.rs`: aggregating cumulative abnormal returns (CAR) with t-stats.
  - `features.rs`: computes features only from data up to the as-of date.
  - These follow a good rule: no network, clock or randomness inside the domain code.
- **Everything else is a fixture.** There is no HTTP client in the workspace. `MockMarketDataProvider` (`crates/gm-integrations/src/lib.rs:334`) holds 4 RELIANCE bars. `/quant/features` and `/predict/gbm` (`crates/gm-api/src/lib.rs:439,447`) only work on bars the caller sends. `/events` returns 5 hard-coded events.
- **The UI cannot analyse anything.** `apps/web/src/App.tsx` has no input box. It sends a single fixed RELIANCE request (`fixtures.ts`) to `/decide`.
- **`/decide` is the "assumptive" layer the brief rules out.** It only runs when a keyword rule matches a headline. It then adds up signals using about 40 hand-picked constants (`0.30·macro`, `×3`, `×10`, the `0.70` threshold, and confidence weights in `model.rs:195-370`). None of these is calibrated, so the new analysis path will not build on it.
- **Correctness problems to fix:**
  - The indicators use `close`, not `adj_close`, so splits and dividends will corrupt returns on real data.
  - `abnormal_return` is the stock return minus the index return, not the alpha + β market model that `docs/MODEL.md` claims. A missing index is silently treated as 0.
  - The `price_bars` primary key is `(symbol, date)` with no exchange column.
- **Hosting blockers:**
  - The web bundle defaults to `VITE_API_BASE_URL=http://127.0.0.1:8000` (`api.ts:257`), so every call fails once hosted.
  - The server reads `GM_PORT`, but Railway supplies `PORT`.
  - There is no authentication and CORS allows any origin.
- **Licence:** PolyForm Noncommercial. The current Razorpay plans would charge money, so the licence needs revisiting before any paid launch.

## Principles (these govern every model we add)

1. **Distributions, not calls.** Every forecast is a set of quantiles or a fan chart with its horizon, never a bare "BUY".
2. **A model must earn its place.** Each forecast model is tested walk-forward, out of sample, against a random-walk or historical-simulation baseline, using proper scoring rules. The UI shows the result. If a model has no edge on a ticker, the app says "no evidence of predictability". That is the factual answer for most liquid stocks (Fama). Simons' edge came from many small, statistically validated effects.
3. **Cite or calibrate.** Every constant is either estimated from data or tied to a published source (for example RiskMetrics λ=0.94). No unexplained magic numbers.
4. **Data provenance is visible.** Each analysis shows its source, when the data was fetched, any gaps, and the results of data-quality checks.
5. **The domain layer stays pure.** The provider fetches the data, the API passes it into pure `gm-domain` functions, and Monte Carlo uses a seeded ChaCha generator. The same inputs always give the same output, which keeps the existing replay guarantee.

## Phase 0: analyse any instrument or set of instruments (milestones are each shippable, in order)

### 0.1 Data foundation and an early deploy
- **Provider:** add `YahooChartProvider` behind the existing `MarketDataProvider` trait (`gm-integrations/src/lib.rs:278`).
  - Uses the v8 chart endpoint: `RELIANCE.NS`, `.BO`, `^NSEI`, `^BSESN`.
  - Returns adjusted OHLCV plus split and dividend events, and the delayed last price.
  - New dependency: `reqwest` with rustls. Retries with backoff, and a timeout.
- **Migration:**
  - Add `exchange` and `fetched_at` to `price_bars` and change the primary key to `(symbol, exchange, date)`.
  - Read-through cache: serve from Postgres and only fetch what is missing. Wire up the existing `save_price_bar` (`gm-persistence/src/lib.rs:189`) and add a reader.
- **Indicators:** switch `features.rs` and `stochastic.rs` to use `adj_close.unwrap_or(close)`.
- **Data-quality report** (pure domain function). It checks:
  - OHLC consistency (low ≤ open, close ≤ high).
  - Non-positive prices and zero-volume days.
  - Gaps compared with the trading days observed in NIFTY.
  - Stale last bar.
  - Return outliers above 8σ (robust MAD) with no corporate action on that date.
- **Endpoints:**
  - `GET /api/v1/instruments/search?q=` (Yahoo search, filtered to NSE/BSE)
  - `GET /api/v1/bars?symbol=&range=`
  - `GET /api/v1/quote?symbol=`
- **Deploy to Railway now**, so we learn early whether Yahoo blocks requests from Railway's servers. That is the main risk; the fallback is NSE bhavcopy.
  - Build the web app with `VITE_API_BASE_URL=""` so it calls the same origin.
  - `main.rs` reads `PORT` before `GM_PORT`, and the host is `0.0.0.0`.
  - Railway Postgres supplies `DATABASE_URL`.
  - `GM_ACCESS_TOKEN` gates access with a small cookie/bearer middleware.
  - CORS is restricted to our own origin.

### 0.2 Single-instrument analysis engine (new `gm-domain/src/analysis/`, pure)
- **Distribution:** annualised μ and σ, skew, excess kurtosis, Jarque–Bera, and the Hill tail index for fat tails.
- **Is it predictable at all?**
  - Ljung–Box on r and r² (autocorrelation and ARCH effects).
  - Lo–MacKinlay variance ratio with a heteroskedasticity-robust z-statistic.
  - ADF test on log price.
- **Volatility:** EWMA (RiskMetrics 1996), and GARCH(1,1) with Student-t errors estimated by maximum likelihood (Bollerslev 1986/87), giving a volatility term structure.
  - The optimiser is a hand-written Nelder–Mead of about 60 lines, so no optimisation crate is needed.
- **Stochastic projection:** Monte Carlo price paths, one set from GBM (existing) and one from GARCH-t, with seeded ChaCha. Output is a fan chart of 5/25/50/75/95% quantiles over 5, 21 and 63 days, plus P(return > 0) and P(drawdown > x).
- **Risk:**
  - VaR and expected shortfall at 95% and 99%: historical, parametric (t) and Monte Carlo.
  - Max drawdown and how long it lasted.
  - Sharpe and Sortino with Lo (2002) standard errors.
  - Alpha and β against NIFTY, with standard errors and idiosyncratic volatility.
- **New dependencies:**
  - `statrs` for the distributions behind p-values.
  - `rand`, `rand_chacha` and `rand_distr` for seeded simulation.
- **Endpoint:** `POST /api/v1/analyze {symbols:[...], range, horizon, seed}` returns one report per symbol.

### 0.3 Validation harness (this is what makes it "evidence-based")
- Walk-forward forecasting from a rolling origin, with no look-ahead (reuses the as-of filtering).
- **Scoring:**
  - Pinball loss and CRPS for the forecast distributions, compared with the random-walk and historical-simulation baselines.
  - Diebold–Mariano test of whether the difference is significant.
  - Directional accuracy with a binomial test against 50%.
  - VaR backtests: Kupiec proportion-of-failures and Christoffersen independence.
- Each analysis report gets a `validation` block, and the UI shows a pass or fail per model per ticker.

### 0.4 Multi-instrument and multi-market comparison
- Align returns on the intersection of trading days.
- Pearson and Spearman correlation, plus rolling 63-day correlation.
- Ledoit–Wolf shrinkage covariance.
- PCA: how much variance the first ("market") factor explains.
- Engle–Granger cointegration for any pair, with the Ornstein–Uhlenbeck half-life of the spread (the classic stat-arb construction).
- Relative performance and relative drawdown.
- New dependency: `nalgebra`, for the symmetric eigen-decomposition and matrix work.
- Indices are included (NIFTY, BANKNIFTY, SENSEX, sector indices). Non-INR markets come later with FX conversion.

### 0.5 Web app
- A new **Analyze** view becomes the home page:
  - Ticker search with multi-select chips.
  - Candlestick chart plus the Monte Carlo fan chart, using `lightweight-charts`, which is already installed.
  - Panels for distribution, predictability tests, volatility, risk and validation.
  - A comparison view with a correlation heatmap and cointegrated pairs.
  - A data-quality badge.
  - A **Methodology** page giving each formula and its citation.
- The delayed quote refreshes every 60 seconds with TanStack Query (already installed).
- The current fixture console stays as a secondary "Event lab" tab, unchanged.
- The `/decide` UI is labelled experimental and uncalibrated.

## Phase 1: events and fundamentals (planned in detail after Phase 0 ships)
- Real corporate announcements (NSE) and news (GDELT).
- Fix the event study to a proper market model, with an estimation window and alpha + β, and test CAR significance across events.
- Replace the hand-tuned `/decide` constants with calibrated estimates, or retire it.
- Fundamentals (the Buffett/Graham layer): ROE, ROIC, owner earnings, leverage, and a Monte Carlo DCF where uncertainty in the inputs gives a distribution of value.
  - The free fundamentals data source for India is undecided. We'll choose it at the start of Phase 1.
- Optional: a 2-state hidden Markov model for market regimes (Hamilton 1989; Baum–Welch, since Leonard Baum worked at Renaissance).

## Phase 2: connect a portfolio
- Read-only Zerodha Kite Connect and/or Upstox: an OAuth-style login, broker tokens stored encrypted (Kite tokens expire daily), and syncing holdings and positions.
- User accounts and authentication arrive at this point, together with a `portfolios` table (the `decisions.portfolio_id` column already exists).
- Portfolio analytics reuse the 0.2–0.4 engine:
  - Portfolio VaR and expected shortfall.
  - Euler risk contributions.
  - Factor exposure (PCA and β).
  - Drawdown.
  - Bounds on Kelly position sizes (Kelly 1956, Thorp).
  - Rebalancing suggestions that account for trading costs.
- Still **no live order placement**. The existing `ExecutionMode::Live` stays disabled.

## Files that change in Phase 0
- `Cargo.toml` (workspace dependencies: reqwest, statrs, rand, rand_chacha, rand_distr, nalgebra)
- `crates/gm-integrations/src/lib.rs`: the Yahoo provider (or a new `yahoo.rs` if lib.rs gets too big)
- `crates/gm-domain/src/analysis/{mod,distribution,tests_stat,volatility,montecarlo,risk,validation,multi,quality}.rs` and a fix to `features.rs`/`stochastic.rs` to use adjusted prices
- `crates/gm-persistence/src/lib.rs` and a new migration `migrations/2026112x_price_bars_exchange.sql`
- `crates/gm-api/src/lib.rs` (`/api/v1/*` routes, access-gate middleware, CORS) and `main.rs` (`PORT`)
- `apps/web/src/` (Analyze view, chart components, a relative API base) and the `Dockerfile` (`VITE_API_BASE_URL`)
- `docs/METHODOLOGY.md` (formulas and citations), and updates to `docs/API.md` and `README.md`

## Verification
- **Reference checks for the maths:**
  - Generate reference outputs once with Python (`arch` for GARCH, `statsmodels` for Ljung–Box, ADF and DM, `sklearn` for Ledoit–Wolf) on a committed 10-year NIFTY CSV, using `scripts/gen_reference.py`.
  - Rust tests check against these references within stated tolerances.
  - Simulate GARCH paths with known parameters and confirm the fit recovers them.
  - Run Monte Carlo twice with the same seed and confirm the output is byte-identical.
- `cargo test --workspace`, clippy and fmt. CI already runs these.
- Integration test for the provider against a recorded response stored as a fixture, plus one live check behind `#[ignore]`.
- Extend `scripts/smoke_api.sh` to cover `/api/v1/analyze` for RELIANCE.NS, TCS.NS and ^NSEI.
- Playwright end-to-end test: search, add 3 tickers, check the fan chart, risk table and validation badges render, and that the axe accessibility check passes.
- On Railway: `/health` responds, the access gate blocks requests without the token, and a real analysis of `RELIANCE.NS` and `^NSEI` completes from the hosted URL, which confirms Yahoo is reachable from Railway.
