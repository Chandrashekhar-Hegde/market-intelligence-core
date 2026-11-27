# API Contract

The HTTP API is a thin runtime wrapper over the deterministic domain core. The
decision path accepts explicit input facts and does not fetch data on its own.

## Operational Endpoints

- `GET /health` returns a liveness response for the API process.
- `GET /ready` reports service readiness and optional PostgreSQL status.
- `GET /version` returns the API version and current model version.
- `GET /openapi.json` returns the OpenAPI 3.1 contract.
- `GET /events` returns fixture-backed normalized event review summaries.
- `GET /events/{event_id}` returns raw metadata, normalized facts, entity
  mappings, and source reliability for one fixture event.
- `GET /payments/state` returns Razorpay test-mode state and recent verified
  payment events.
- `POST /payments/orders` creates a deterministic test-mode order.
- `POST /payments/verify` verifies a checkout signature.
- `POST /payments/webhooks/razorpay` verifies a signed raw webhook body.

When `DATABASE_URL` is unset, `/ready` still returns ready because persistence is
optional for local domain and smoke-test workflows. When `DATABASE_URL` is set,
the API connects to PostgreSQL at startup, applies migrations by default, and
`/ready` verifies that the database remains reachable.

## Market Data Endpoints

Live data for any exchange from Yahoo Finance (free end-of-day data and quotes delayed by about 15 minutes; unofficial
endpoints). Responses are cached in-process for 60 s (`5d`/`1mo`) or 15 min.

- `GET /api/v1/instruments/search?q=&country=&exchange=` returns
  `{results: [{symbol, name, exchange, country, quote_type}], markets: [{country, exchange, count}]}`.
  - **Coverage:** every exchange, covering equities, ETFs, indices, funds, crypto, FX and futures;
    options are excluded. `q` is 1–64 characters.
  - **Filters:** `country` (India, USA, UK, Japan, Global for crypto/FX, …) and `exchange` (NSE,
    BSE, NASDAQ, NYSE, London, …) are case-insensitive; `all` or omitted means no filter. Indian
    listings come first. `markets` counts every match before filtering, for the UI's chips.
  - **Added beyond Yahoo's search:** Yahoo's search rarely returns BSE listings. For the first six
    NSE equities the server tries the `.BO` twin through the quote endpoint and adds those that
    exist. A typed exact ticker (`500325.BO`, `INR=X`) is resolved the same way.
- `GET /api/v1/bars?symbol=&range=` returns `{chart, quality}`. `range` is one of
  `5d 1mo 3mo 6mo 1y 2y 5y 10y max` (default `1y`). `chart.bars` are completed
  daily bars, oldest first: `close` is split-adjusted and `adj_close` is split- and
  dividend-adjusted. If the session is still open, the forming bar is dropped.
  `chart` also carries `corporate_actions`, `source` and `fetched_at`. `quality` is
  the data-quality report: OHLC consistency, non-positive prices, zero-volume days,
  missing days versus NIFTY's trading days (INR listings only), staleness, and
  return outliers above a robust z of 8.
- `GET /api/v1/quote?symbol=` returns `{quote, source, fetched_at, delayed: true}`.
  `quote.market_open` is true only during the regular session; it uses Yahoo's trading
  period, so it is also false on exchange holidays.
  It also carries Yahoo's `change`, `change_pct` and `day_volume` for the latest session.
- `GET /api/v1/intraday?symbol=&range=1d|5d` returns `{chart: {meta, interval, previous_close,
  session_start, session_end, points: [{t, open, high, low, close, volume}], source, fetched_at}}`
  for the live chart: `1d` is 1-minute bars of the latest session, `5d` is 5-minute bars. `t` is
  Unix seconds. `previous_close` is the price today's change is measured from (last price minus
  Yahoo's day change). Cached for 30 s; display only, never used in the analysis.
- `GET /api/v1/news?symbol=` returns `{query, articles: [{article: {title, url, publisher, source,
  seen_at, summary}, impact}], sources: [{name, ok, count, error}], benchmark, beta, fetched_at}`.
  - **Sources**, all free and keyless, fetched at once:
    - Google News RSS (the company name, India edition, last 3 days);
    - the Economic Times and Mint markets feeds (items naming the company);
    - GDELT.
  - **Merging:** stories are de-duplicated by title and sorted newest first.
  - **Failures:** each source reports `ok`, `count` and any `error`. A failed source never empties
    the list; the response is `503` only when every source fails.
  - **`impact`:** the price move since the story from 5-minute bars, the part beyond β × the home
    index, and how often a move that size happens by chance. It is null outside the intraday
    window.
  - **`reading`** (per story): `{kind, direction, materiality, reason, reader}`.
  - **`vote`** is the news score. **`reader`** is `built-in` or the configured model
    (`GM_LLM_BASE_URL`, `GM_LLM_MODEL`, `GM_LLM_API_KEY`; any OpenAI-compatible endpoint).
- `GET /api/v1/stream?symbols=A,B` is a Server-Sent Events stream of `tick` events
  `{symbol, price, change, change_pct, day_high, day_low, day_volume, currency, exchange,
  tick_time, source}` from Yahoo's live streamer. It takes 1–20 symbols. The last known
  tick per symbol is sent first, and keep-alive comments are sent while idle. NSE
  coverage is still being measured; see `docs/STATUS.md`.

Symbols use Yahoo suffixes: `RELIANCE.NS`, `RELIANCE.BO`, `^NSEI`, `^BSESN`, `INR=X` (FX), `GC=F` (futures).
Invalid input returns `400`, an unknown symbol `404`, and an upstream failure `503`.

## Analysis Endpoint

`GET /api/v1/desk` is the market news desk: `{sources, stories, leans, reader, as_of}`.
- `sources`: each feed and search with `ok`, `count` and `error`.
- `stories`: `{title, url, publisher, source, seen_at, theme, impacts: [{asset, direction, reason}]}`.
- `leans`: one per asset, `{asset, label, symbol, score, lean, drivers}`.

The leans are uncalibrated and never feed the advice.

`GET /api/v1/today?extra=SYM,...` is the daily list: `{as_of, notice, items}`. Each item is
`{symbol, name, group, currency, instrument_type, last_price, change_pct, last_date, return_1y,
vol_annual, lead_model, prob_gain_1m, prob_gain_3m, prob_loss_10pct_3m, range_3m,
chance_check_3m, advice, news_vote}`, or `{symbol, group, error}` when an instrument can't be
analysed. `extra` takes at most 20 watchlist symbols. Items are cached for 30 minutes.

`GET /api/v1/calibration?reader=built-in|llm` reports the calibration run: `{recording, reader,
n_records, first_seen, last_recorded, company_events, market_events, min_events,
after_1_session, after_5_sessions, note}`. Each `after_*` holds `company` and `market` lists of
`{group, direction, n, mean, se, hit_rate, enough}`. `recording` is false when `GM_DATA_DIR` is
unset. `reader=llm` needs `GM_LLM_*` and answers 400 without them.

`GET /api/v1/projection?symbol=&price=&seed=42` returns the forecast fans recomputed from a live
`price`: `{symbol, price, last_close, last_date, computed_at, projections}`, with `projections`
shaped as in `/analyze`. The 10-year daily fit is cached per symbol until the next close. A
price more than about 50% away from the last close is a 400.

`GET /api/v1/analyze?symbol=&range=10y&benchmark=auto&seed=42&paths=2000` analyses any symbol (default `10y` since 27 Nov 2026; the app offers 5 or 10 years):

- `report` holds `distribution`, `predictability`, `volatility` (EWMA and GARCH-t),
  `projections` (Monte Carlo fans for `gbm`, `garch_t` and `bootstrap`, each with 30
  `sample_paths` and a highlighted `typical_path`), `lead_model` and `lead_reason` (the
  projection validation rates best, shown first), `risk` (VaR/ES, drawdown, Sharpe),
  `benchmark` (beta/alpha), `validation` (walk-forward scores, plus `chance_of_gain`: whether the
  history-based chance of a gain beat a coin flip at 5, 21 and 63 sessions) and `findings`
  (plain-language conclusions).
- `data` carries provenance: source, fetch time, currency, exchange, TradingView symbol and the
  data-quality report.
- `benchmark` records which index was used (`selection`: `auto` or `requested`), whether it
  loaded, and a currency note when currencies differ.
- `signals` holds six literature trading rules, each tested walk-forward on up to 10 years of this
  instrument at 5, 21 and 63 sessions (effect, Hansen–Hodrick t, Holm-adjusted p, hit rate with a
  Wilson interval, `earns_place`, and a plain `verdict`). `advice` holds `stance` (`buy` / `sell` /
  `hold`), `basis` (`proven`, `conflict`, `proven_but_quiet`, `no_evidence`), `headline`,
  `horizon_days`, `review_date`, `confidence`, `flip_level` and one `reasons` line per rule. Both
  are null when the history is too short, and `signals_note` then says why. See METHODOLOGY.md.
  - **News in the advice:** `basis` can also be `news_driven` (no proven rule active, strong news,
    labelled "no track record yet") or `news_conflict` (a proven rule contradicted by strong news →
    Hold).
  - `advice.news_headlines` lists the stories behind such a call.
  - `news_vote` is today's news score. It is null when no news source answered, and for anything
    that isn't a single company stock (news votes only for `EQUITY`).
- **`benchmark=auto`** picks the home-market index from the listing suffix: the S&P 500 for US
  listings, MSCI ACWI for unknown exchanges, and none for indices, FX, futures and crypto.
  `none` skips the exposure section; any other value is used as a symbol.
- **Errors.** `paths` outside 100–20,000, a bad `range`/`symbol`, or too little history (under
  60 returns) gives `400`. An unknown symbol or an unknown requested benchmark gives `404`.
- **Determinism.** The same inputs and `seed` give byte-identical projections.

Formulas, sources and verification: [METHODOLOGY.md](METHODOLOGY.md).

## Compare Endpoint

`GET /api/v1/compare?symbols=A,B,C&range=5y` compares 2–10 distinct symbols (any exchange) on
their common trading days. The response has three parts:
- `comparison`:
  - correlations: `correlation_daily`, `spearman_daily`, `correlation_weekly`;
  - `covariance_shrinkage` and `covariance_annual` (Ledoit–Wolf);
  - `pca`;
  - `pairs`, each with correlation and Engle–Granger cointegration (hedge ratio, half-life, spread
    z-score);
  - `rolling_correlation`, `performance`, `rebased` (normalised to 100, sampled to at most about
    400 points) and `findings`.
- `instruments`: provenance and currency for each symbol.
- `notes`: currency and time-zone caveats.

Duplicates are dropped. Fewer than 2 or more than 10 symbols, invalid input, or fewer than 120
common days returns `400`. An unknown symbol returns `404`, and the message names it.

## Access Gate

When `GM_ACCESS_TOKEN` is set, every route except `GET /health`, `POST /access`
and the signed Razorpay webhook requires the token. Send it as
`Authorization: Bearer <token>` or as the `gm_access` cookie. Browsers without the
token get a `401` login form, which posts to `/access`. A correct token sets an
`HttpOnly; Secure; SameSite=Lax` cookie that lasts 30 days.

CORS is off unless `GM_CORS_ORIGINS` lists origins. It defaults to the local Vite dev
origins, and the Dockerfile sets it to `none` because the deployed bundle is
same-origin. The server listens on `PORT` when it is set (Railway), otherwise on
`GM_PORT`.

## Decision Endpoint (retired)

`POST /decide` answers `410 Gone` since 26 Nov 2026 (#65). It turned events into BUY/SELL calls with
about 40 hand-picked, uncalibrated constants. Advice now comes from `GET /api/v1/analyze` →
`advice`, built only from trading rules validated walk-forward on that instrument.

## Event Review Endpoints

`GET /events` is the release fixture feed for the Event Inbox. Each summary
includes:

- event ID and version
- headline and occurrence time
- source, region, sector, symbol, and event class
- confidence and severity
- entity mapping status
- source reliability score and tier

`GET /events/{event_id}` returns the selected review context:

- normalized event payload
- raw source metadata
- normalized facts
- entity mappings with entity type and confidence
- source reliability rationale

These endpoints are fixture-backed until live ingestion is enabled.

## Payment Endpoints

The payment endpoints are test-mode only. They do not enable live billing,
refunds, payouts, subscriptions, or entitlement enforcement.

`POST /payments/orders` accepts:

- `account_id`
- `amount_paise`
- `currency`
- `description`
- `success_url`

The response includes a public key ID, test order ID, test payment ID, and test
signature fixture for local smoke workflows. `POST /payments/verify` validates
the checkout signature using HMAC-SHA256 over `order_id|payment_id`.

`POST /payments/webhooks/razorpay` requires `X-Razorpay-Signature` and validates
the raw body with HMAC-SHA256 before accepting the event. If persistence is
configured, order and verification events are stored in PostgreSQL.

## Local Contract Check

```bash
make run-api
make smoke-api
```

The smoke script checks `/health`, `/ready`, `/version`, `/openapi.json`, the
payment test-mode order and signature flow, signed webhook verification, that `/decide`
answers 410, intraday bars, and news (a GDELT 503 is tolerated). API unit tests also cover `/events`,
`/events/{event_id}`, and payment bad-signature paths.
