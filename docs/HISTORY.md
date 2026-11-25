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

## 25 Nov: live NSE measurement (#44)

The first NSE session after the holiday was measured on Yahoo's streamer. NSE stocks and NIFTY
arrive about 2 s after the trade (about 1 tick/s, and the polled quote is within 1–5 s). BSE
symbols are exactly 15 minutes delayed.

The UI had shown BSE as "no live ticks; polling", which was wrong. It now labels the feed
"DELAYED 15 MIN" and keeps streaming the delayed ticks. Because NSE real time works for free, the
broker-feed issue (#68) stays at p1; it is needed only for licensed or BSE real-time data.

## 25 Nov: Phase 0.4, comparison, and the "How it works" guide

- **Engine:** `GET /api/v1/compare` and `gm-domain::analysis::multi` cover:
  - daily, rank and weekly correlation;
  - Ledoit–Wolf covariance (sklearn-exact);
  - PCA by Jacobi rotation (no nalgebra; the matrices are 10×10 at most);
  - Engle–Granger cointegration in both orderings, with MacKinnon critical values checked against
    statsmodels;
  - relative performance.
- **Errors caught while building it:**
  - My remembered 1% critical-value coefficient (−22.527) was wrong; statsmodels has −33.527.
  - statsmodels uses n − 1 as the sample size for those critical values.
  - Duplicate series names now return an error instead of hitting a debug assertion.
- **Multiple-testing caveat:** a live 6-instrument comparison flagged RELIANCE/BTC as
  "cointegrated". With 15 pairs, about 0.75 false positives are expected, so the findings now
  state how many hits chance would produce.
- **UI:** the Compare tab (chips, rebased chart, heatmap, PCA, pairs). Weekly correlation is the
  default when time zones differ.
- **"How it works" tab (#63):** where the data comes from and how live it is, what is stored (no
  market-data database, only a sign-in cookie; the Postgres schema belongs to the Event lab and
  isn't attached in production), how to read each panel, how models earn trust, limits, and a
  glossary.

## 25 Nov: production search incident (#78)

- **What happened:** after #77 deployed, the production smoke test failed. Production answered
  `search?q=reliance` with `{"results":[]}` and status 200, while Yahoo answered the same query
  normally from other networks.
- **Root defect:** the search parser defaulted a missing `quotes` field to an empty list, so any
  non-result body from Yahoo looked like "no matches".
- **Fix:** a missing `quotes` field is now a 503 that includes the upstream body, and quotes that
  all get filtered out are logged at warn level with the body.
- **Upstream cause:** unknown. After redeploying, the query worked and nothing was logged. If it
  recurs, the error message and the `search returned quotes but none were usable` log line will
  show what Yahoo sent.

## 25 Nov: everything on GitHub for collaborators and agents

The owner asked that plans and the full "done / next" picture live on GitHub, so other developers
and AI agents can collaborate.

- **Plans:** committed to `docs/plans/`. That covers the Phase 0 plan with a status header, the
  live-data plan, this collaboration plan, and new step-by-step Phase 1 and Phase 2 plans.
- **Roadmap:** `docs/ROADMAP.md` became the public, ordered queue, with done items linked to PRs
  and a dependency graph. The board stays the owner's private view.
- **Issues:** all open issues were rewritten as work packages (why, scope, approach, acceptance
  criteria, verification, dependencies, owner input). The broken `\n` bodies were fixed. 15
  issues were added (#80–#94) and linked as GitHub sub-issues under #17, #66, #69 and #70. Every
  open issue carries exactly one of `agent-ready`, `needs-owner-input` or `blocked`.
- **Milestones:** 0.2, 0.3, 0.4 and v0.1.0-mv were closed, and the descriptions now give status
  and order.
- **Contributor plumbing:** CONTRIBUTING has *how to pick up work*, a *Definition of Done* and
  *working with AI agents*. There is a new Work package issue template and a new PR template.
- **CODEOWNERS** moved to `@Chandrashekhar-Hegde` (`@llMr-Sweetll` was the old username).
  Discussions are enabled, and `main` is protected for everyone: PR plus green CI, no
  force-push.
