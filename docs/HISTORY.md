# History and decisions

The chronological record of how the project got where it is and why. Read it before reversing a
decision. Newest entries are last.

## Before Nov 2026: v0.1.0-mv fixture console (PRs #27–#33)

- **Shipped:**
  - a deterministic event-scoring and decision engine (`/score`, `/decide`);
  - an operator console with Event Inbox, Decision Workbench, Knowledge and Integrations views;
  - Razorpay test-mode payments, Docker packaging, release automation, and the CI gate.
- **Limits:** everything ran on fixtures. `MockMarketDataProvider` held four RELIANCE bars, and
  there was no HTTP client.
- **Assessment (22 Nov 2026):** the maths primitives were sound, but `/decide` sums about 40
  uncalibrated hand-picked constants. The new analysis path doesn't build on it. It stays as the
  "Event lab (fixtures)", labelled experimental (#65 decides whether to calibrate or retire it).

## 22 Nov 2026: plan and direction

The owner decided:
- India first (later widened to global);
- free end-of-day data plus delayed quotes;
- Railway hosting;
- a private beta behind one shared token, with no accounts yet.

The plan set out phases 0.1–0.5, then Phase 1 (events and fundamentals) and Phase 2 (portfolio).
Its five principles are in `AGENTS.md`.

## 22–24 Nov: Phase 0.1, real data (PR #72)

- **Yahoo chart/search provider:** v8 chart and v1 search, with retries, an in-process TTL cache,
  split- and dividend-adjusted bars, and corporate actions. A still-forming session bar is dropped.
- **Adjusted closes:** indicators and GBM switched to `adj_close`; splits had been corrupting
  returns.
- **Data-quality report:** OHLC sanity, gaps against an exchange calendar, staleness, robust-z
  outliers.
- **Hosting:** the access gate (bearer or cookie form), a CORS allow-list, and `PORT` support.
  The first Railway deploy (22 Nov) was a local `railway up`.
- **Skipped on purpose:** a Postgres cache for bars. Yahoo restates adjusted closes after every
  dividend, so cached rows would go stale silently. The in-process cache is enough for one
  instance.
- **Real-time sources investigated (24 Nov):**
  - **TradingView** has no data API, so it is embedded as a display-only widget.
  - **Yahoo's unofficial WebSocket** streamer works: crypto and FX ticks arrive with 1–3 s lag.
    It is built as an SSE fan-out (`/api/v1/stream`).
  - **NSE ticks** couldn't be measured because 24 Nov was an NSE holiday (#44).
  - **Free broker APIs** (Upstox, Angel, Breeze, Dhan) need the owner's keys (#68).
  - **The NSE website JSON API** was rejected: its terms forbid automated access.
- **UI:** the Markets view became the home screen. The old console moved to the Event lab tab.
- **Local gate:** `scripts/check.sh` with a pre-push hook. CI lint was raised to oxlint with
  `--deny-warnings`.
- **Bugs the new tests caught:**
  - a rustls provider panic that would have killed the live stream in production;
  - two accessibility bugs (sidebar contrast, and a button hidden by `role="cell"`);
  - nine RUSTSEC advisories, fixed by `cargo update`. The chrono "unmaintained" advisory is
    ignored, tracked in #71.
- **GitHub:** 7 milestones, issues with acceptance criteria, and the project board (#3).
- **Deploys:** Railway was connected to GitHub `main` (#43). Since then, every merge deploys
  automatically.

## 24 Nov: Phase 0.2, global analysis engine (PR #73)

- **Scope widened:** the owner asked for any stock on any exchange, analysable without a fixed
  reference index.
  - Search was opened to all exchanges and instrument types (options excluded).
  - `markets.rs` maps the listing suffix to the home-market benchmark (34 exchanges). US listings
    use the S&P 500, and unknown exchanges use MSCI ACWI. Every benchmark symbol was verified
    against Yahoo.
  - Indices, FX, futures and crypto use no benchmark.
- **The engine** (`gm-domain::analysis`) covers:
  - distribution, with Jarque–Bera and the Hill tail index;
  - predictability: Ljung–Box, Lo–MacKinlay variance ratios and ADF;
  - volatility: EWMA and GARCH(1,1)-t by maximum likelihood, using a hand-written Nelder–Mead
    (no optimisation crate);
  - seeded Monte Carlo fans (GBM and GARCH-t);
  - risk: VaR/ES three ways, drawdown, and Sharpe with Lo's standard error;
  - exposure: beta and alpha against the benchmark;
  - plain-language findings.
- **Verification:** `scripts/gen_reference.py` (scipy, statsmodels, arch) on committed 5-year
  data for AAPL, ^GSPC and ^NSEI. Toyota's unusual GARCH fit (persistence 0.60) was checked
  against arch and matches to 7 digits: it is real, not an optimiser bug.
- **Bugs caught:**
  - statrs' t quantile hangs for huge ν (near-normal data). It now falls back to the normal above
    ν = 200.
  - Scrollable tables on mobile failed axe. They now wrap.
- **Decision:** the endpoint is `GET /api/v1/analyze`, one symbol per request. The plan's
  multi-symbol POST was deferred to 0.4's comparison view.

## 24 Nov: Phase 0.3, out-of-sample validation (PR #74)

- **Method:** walk-forward 1-day forecasts after a 500-day warm-up, for three models:
  - historical simulation (the baseline);
  - EWMA-normal;
  - GARCH-t, refit every 63 days and filtered daily.
- **Scores:**
  - pinball loss over 7 quantiles;
  - Diebold–Mariano with Newey–West variance;
  - Kupiec and Christoffersen VaR backtests at 99% and 95%;
  - 90% interval coverage;
  - direction, with an exact binomial test.
- **Verdict rule:** a model earns its place only if it beats the baseline **and** its VaR passes.
- **Real results (10 years of data):**
  - **AAPL:** GARCH-t earns its place.
  - **S&P 500 and BTC:** sharper, but the 95% VaR fails.
  - **Toyota and Reliance:** no better than the baseline.
- **Verification:** the backtest statistics match numpy/scipy, and the harness rates the true
  model correctly on simulated GARCH-t data.

## 24 Nov: documentation hand-off

`AGENTS.md` became the canonical operating guide, with `CLAUDE.md` importing it. This file was
added, `docs/ARCHITECTURE.md` was refreshed for the market and analysis path, and the clean-clone
run of the documented setup was verified (see `docs/STATUS.md`).
