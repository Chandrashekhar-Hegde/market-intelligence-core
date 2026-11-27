# AGENTS.md

Operating guide for anyone, human or AI agent, picking this repo up cold. Read this first, then
`docs/STATUS.md` (what works today) and
[docs/ROADMAP.md](docs/ROADMAP.md) (what's next, in order).
The history of how we got here is in `docs/HISTORY.md`.

## What this is

A hosted market-analysis app. A user types **any stock, index, ETF, fund, crypto or FX symbol on
any exchange** and gets the following, built on real data:
- a live quote and daily history, with data-quality checks;
- an evidence-based analysis: return distribution, predictability tests, GARCH volatility, Monte
  Carlo forecast ranges, VaR/ES and benchmark beta;
- **out-of-sample validation** of every forecast model against a baseline.

Forecasts are always distributions. Buy/Hold/Sell advice appears only when a signal has proven itself
out of sample on that instrument (Phase 0.6).

- **Live:** https://gm-api-production-911a7e90.up.railway.app (private beta, token-gated).
- **Repo:** https://github.com/Chandrashekhar-Hegde/market-intelligence-core.
- **License:** PolyForm Noncommercial. No paid features without revisiting the license.

## Principles (non-negotiable)

1. **Calls only when proven.** Every forecast is quantiles or a fan with a horizon. A Buy/Sell call
   comes only from a signal that passed walk-forward validation on that instrument, and is always
   shown with its evidence (hit rate, sample size, horizon). Otherwise the app says "Hold: no proven
   edge". Changed by the owner on 2026-11-26; see `docs/plans/phase-0.6-advice.md`.
   Since 2026-11-27 (owner decision, `docs/plans/phase-0.7.md`) strong news may also produce a
   call. Such a call is always labelled "based on today's news; no track record yet", its
   thresholds are documented as uncalibrated, and a proven rule pointing the other way turns it
   into Hold. The app advises; it never decides or places orders for the user.
2. **A model earns its place.** It must beat historical simulation out of sample (Diebold–Mariano)
   and pass VaR backtests (Kupiec, Christoffersen). Otherwise the app says "no evidence".
3. **Cite or calibrate.** Every constant is estimated from data or cites a source, and is listed
   with its kind in `docs/CONSTANTS.md` (a test enforces it for `gm-domain`). No magic numbers.
4. **Provenance is visible.** Every result shows its source, fetch time and data-quality report.
5. **The domain is pure.** `gm-domain` never touches the network, the clock or unseeded randomness.
   Providers fetch, the API passes data in, and Monte Carlo uses seeded ChaCha8 (same seed gives
   byte-identical output).
6. **No live order placement.** `ExecutionMode::Live` stays disabled.

## Repo map

```
crates/
  gm-domain/          pure logic (no I/O)
    src/analysis/     the engine: distribution, predictability, volatility (EWMA, GARCH-t),
                      montecarlo, risk, validation (walk-forward), multi (comparison),
                      numeric (OLS, Nelder–Mead)
    src/quality.rs    data-quality report for bar series
    src/{features,stochastic,indicators,event_study,model,...}.rs   older decision path (its /decide route is retired: 410)
    tests/fixtures/   5y real adjusted closes (AAPL, ^GSPC, ^NSEI) + reference.json
  gm-integrations/    providers
    src/yahoo.rs        Yahoo chart/search client (retries, TTL cache), parsing, validation
    src/yahoo_stream.rs Yahoo live WebSocket → protobuf ticks → broadcast (shared upstream)
    src/markets.rs      suffix → home-market benchmark + TradingView name (34 exchanges + US)
    src/lib.rs          provider traits, mock providers, Razorpay test mode
    tests/fixtures/     recorded Yahoo responses and stream frames
  gm-api/             Axum HTTP: /api/v1/{instruments/search,bars,quote,stream,analyze,compare},
                      access gate, CORS, legacy fixture routes (/events, /payments/*; /decide answers 410)
  gm-persistence/     Postgres (optional; nothing in the market path needs it yet)
  gm-worker/          migration CLI
apps/web/             React + Vite + TanStack Query + lightweight-charts
  src/Markets.tsx     home view: search, live quote, candles, quality, TradingView widget
  src/Analysis.tsx    analysis panels (findings, fan, risk, volatility, validation, tests, beta)
  src/Compare.tsx     Compare tab: chips, rebased chart, correlation heatmap, PCA, pairs
  src/Guide.tsx       "How it works": data sources, what is stored, how to read it, glossary
  src/marketData.ts   pure UI helpers (unit-tested); keep components out of it
  src/App.tsx         shell; "Event lab (fixtures)" tab holds the old fixture console
  tests/e2e/          Playwright; marketMocks.ts mocks /api/v1 so CI never hits Yahoo
scripts/
  check.sh            the local gate (what the pre-push hook runs)
  smoke_api.sh        end-to-end API checks; works against Railway with a token
  gen_reference.py    regenerates analysis reference values with scipy/statsmodels/arch
.githooks/pre-push    runs scripts/check.sh
docs/                 STATUS, METHODOLOGY, CONSTANTS (every number and its source), API, TESTING, HISTORY, ...
```

## Setup from a clean clone

Needs:
- Rust 1.96+ and `cargo install cargo-audit --locked` (without it the local gate skips the audit
  that CI runs);
- Node 24 (see `.nvmrc`);
- Playwright Chromium, for browser tests;
- only when regenerating maths references: Python 3.12 and `uv`.

Steps that need production access are done by a maintainer: Railway spikes, the production smoke
test, reading the access token. Outside contributors say in their PR which of these are left for a
maintainer.

```bash
git clone https://github.com/Chandrashekhar-Hegde/market-intelligence-core.git
cd market-intelligence-core
git config core.hooksPath .githooks        # enables the pre-push gate
(cd apps/web && npm ci && npx playwright install chromium)
scripts/check.sh                            # full gate: must print "All checks passed."
```

Run locally (two terminals):

```bash
cargo run -p gm-api                         # API on :8000; no DB, no token needed locally
(cd apps/web && npm run dev)                # UI on :5173 (allowed by default CORS)
```

Production-like (one origin, gated): build the web app with `VITE_API_BASE_URL=` (empty), then run
`gm-api` with `WEB_ASSETS_DIR=apps/web/dist GM_ACCESS_TOKEN=... GM_CORS_ORIGINS=none`.

## Commands you'll use

| Goal | Command |
|---|---|
| Full gate (same as CI minus Docker) | `scripts/check.sh` or `make gate` |
| Quick iteration (skip Playwright) | `scripts/check.sh --fast` |
| Before a deploy: also hit real Yahoo | `scripts/check.sh --live` or `make gate-live` |
| One Rust test | `cargo test -p gm-domain analysis::tests::aapl` |
| Web unit tests | `cd apps/web && npm run test` |
| Browser tests (mocked data) | `cd apps/web && npx playwright test` |
| Browser test on real data | `cd apps/web && npm run test:e2e:live` |
| Smoke a running API | `BASE_URL=http://127.0.0.1:8000 scripts/smoke_api.sh` |
| Smoke production | `BASE_URL=https://gm-api-production-911a7e90.up.railway.app GM_ACCESS_TOKEN=$(railway variable list --service gm-api --json \| jq -r .GM_ACCESS_TOKEN) scripts/smoke_api.sh` |
| Measure NSE stream lag (09:15–15:30 IST) | `LIVE_STREAM_SYMBOL=RELIANCE.NS cargo test -p gm-integrations -- --ignored live_stream --nocapture` |
| Backfill news history, then load it into production | `cargo run --release -p gm-api --bin backfill_news -- <dir>`, then `backfill_news read <dir>` (needs `GM_LLM_*`), then `BASE_URL=… GM_ACCESS_TOKEN=… scripts/upload_backfill.sh <dir>` |
| Regenerate analysis references | `uv venv .refenv && VIRTUAL_ENV=.refenv uv pip install numpy scipy statsmodels arch pandas && .refenv/bin/python scripts/gen_reference.py` |

## Workflow (how work lands)

1. Pick the first `agent-ready` row in [docs/ROADMAP.md](docs/ROADMAP.md) whose blockers are
   closed. **ROADMAP order wins**: `gh issue list` sorts by date, not priority. Claim it with a
   comment. Maintainers also move it to In Progress on their private board.
2. Branch from `main` as `<milestone>-<short-slug>` (e.g. `0.1-bhavcopy`). `main` is protected,
   so every change goes through a PR.
3. Build with tests at every layer you touch (see "Testing rules"). Update docs in the same change:
   - `docs/STATUS.md` when what works changes;
   - `docs/METHODOLOGY.md` for any new statistic;
   - `docs/API.md` and the `/openapi.json` handler for any route.
4. `git push` runs the full gate through the pre-push hook. Don't bypass it with `--no-verify`.
5. Open a PR with "Closes #N". CI (`.github/workflows/ci.yml`) must be green.
6. Merge with a merge commit. **Railway deploys `main` automatically.** Wait for that deployment
   to reach SUCCESS (`railway deployment list --service gm-api --json`), then run the production
   smoke command above.
7. Closed issues move to Done on the board automatically. Partially done issues get a comment
   saying what's left.
8. Commit messages: imperative subject, a body that says why. **No AI co-author trailers and no
   "Generated with" lines** in commits or PRs (owner's standing rule).

## Testing rules

- **Domain maths.** Every statistic is checked against an independent implementation
  (`reference.json`, tolerances in `analysis/tests.rs`). It also gets a behavioural test on
  simulated data where the truth is known (for example GARCH parameter recovery, or the harness
  rating the true model as earning its place).
- **Determinism.** The same seed must give byte-identical projections.
- **API.** Tests run against in-process Yahoo stubs (`yahoo_stub`, `stream_stub` in
  `gm-api/src/lib.rs`). No network in non-ignored tests. Live checks are `#[ignore]` with a
  `live` name.
- **Web.**
  - Vitest with mocked `fetch` and a fake `EventSource` (`Markets.test.tsx`).
  - Playwright with `tests/e2e/marketMocks.ts`, on desktop and phone width. It includes an axe
    accessibility check and a "no horizontal overflow" check.
  - `@live` specs only run via `npm run test:e2e:live`.
- **Lint.** oxlint with warnings as errors (react, react-hooks, jsx-a11y, typescript, vitest).
  `react-in-jsx-scope` and `prefer-tag-over-role` are off on purpose, and `no-noninteractive-tabindex`
  allows labelled `region`s, because resizable (scrollable) panels must be keyboard-reachable for axe
  (see `.oxlintrc.json`).

## Deployment and secrets

- **Railway:**
  - Project `market-intelligence-core`, service `gm-api`, environment `production`.
  - It is connected to GitHub `main` and builds from the `Dockerfile`.
  - No database is attached. A small volume holds the news recorder's file (`/data/news.jsonl`).
- **Environment variables:** `GM_ACCESS_TOKEN` (the beta gate) and `RUST_LOG`. Optional:
  `GM_LLM_BASE_URL`, `GM_LLM_MODEL`, `GM_LLM_API_KEY` turn on a model that reads news headlines
  (any OpenAI-compatible endpoint; production uses Gemini `gemini-3.1-flash-lite` at
  `https://generativelanguage.googleapis.com/v1beta/openai`, key in Railway only); without them the built-in reader is used. `GM_DATA_DIR`
  turns on the news recorder (Phase 0.8). In production it points at a Railway volume mounted at
  `/data`, with `RAILWAY_RUN_UID=0` so `docker-entrypoint.sh` can give the volume to the `gm`
  user before dropping root. The server itself never runs as root. The Dockerfile sets
  `GM_CORS_ORIGINS=none`. Railway injects `PORT`, which the server reads before `GM_PORT`.
- **The access token lives only in Railway.** Read it with `railway variable list --service gm-api`.
  Never commit it, print it in logs, or paste it into issues or PRs.
- **Railway CLI login:** `railway login`. If the browser OAuth times out, the human runs it with `!`.
- **GitHub:** `gh` is authenticated with the `project` scope. The project board is the owner's
  *private* working view; the public queue is `docs/ROADMAP.md`, so keep both in step.
  - The board is user project #3, node `PVT_kwHOAydiks4Bs_d6`.
  - Its Status field is `PVTSSF_lAHOAydiks4Bs_d6zhrK-6w`, with options Backlog `3537a9a7`,
    Todo `5e35da4d`, In Progress `d963a666`, In Review `daf0f9f5`, Done `1cf45f5c`.

## Gotchas we already paid for

- **rustls provider.** Both rustls backends are linked (sqlx brings ring, reqwest brings
  aws-lc-rs). `yahoo_stream::install_crypto_provider()` must run before any `wss://` connect, or
  the stream task panics. A regression test exists.
- **statrs Student-t quantile.** It never returns for huge ν (from near-normal data).
  `risk.rs` and `validation.rs` switch to the normal above ν = 200. Keep that guard on any new
  t-quantile call.
- **macOS has a case-insensitive filesystem.** `Markets.tsx` and `markets.ts` collide, which is
  why the helpers live in `marketData.ts`.
- **macOS has no `timeout`.** Use `perl -e 'alarm N; exec @ARGV' cargo test ...`. A test that
  hangs keeps spinning after cargo dies; `pkill -f gm_domain-` cleans it up.
- **Yahoo trading periods.**
  - Holidays and weekends show the previous session's `currentTradingPeriod`, so
    `market_open` is computed from it (no holiday calendar needed).
  - 24 Nov 2026 was an NSE holiday.
  - Measured live on 25 Nov 2026: the Yahoo streamer delivers NSE at about 2 s lag and about
    1 tick/s, BSE at exactly 15 min, and crypto/FX at 1–3 s. `liveState` labels an exchange-delayed
    feed "DELAYED N MIN" rather than LIVE.
- **Yahoo is unofficial.** It can rate-limit or block. The fallbacks are NSE bhavcopy for
  end-of-day data (#45) and a broker feed for licensed live ticks (#68, which needs the owner's
  account). The NSE website JSON API is off-limits: its terms forbid automated access.
- **Yahoo can send odd bodies from cloud IPs.** Never default a missing upstream field to
  "empty". Surface it as an error carrying the body (see the search incident in `docs/HISTORY.md`).
  If production search or bars look empty, check `railway logs --service gm-api` for warnings.
- **TradingView has no data API.** We only embed its widget for display.
- **Analysis on short histories.**
  - Under 60 returns the request is rejected with 400.
  - Under 250 there is no GARCH (the UI says so).
  - Under 600 there is no validation.
  - Ranges `1y` and below are too short for validation; the default is `10y` (trading rules always use 10y).
- **`gm-api` test helper names.** In `gm-api` tests the request helper is `get_req`, not `get`,
  because `get` shadows axum's router function.

## Where we are and what's next

**The single source of truth is [docs/ROADMAP.md](docs/ROADMAP.md).** It lists what's done, with
PRs, and the ordered queue, with each item's state and blockers. Each item is a GitHub issue
written as a full work package. Pick the first `agent-ready` row whose blockers are closed
(`gh issue list --label agent-ready`), claim it, and follow [CONTRIBUTING.md](CONTRIBUTING.md)
including its Definition of Done.

The design behind the queue is in [docs/plans/](docs/plans/README.md). Commit a plan there
*before* starting significant new work.

Snapshot (2026-11-27):
- **Shipped:** 0.1–0.8 (0.8: Today page, news desk, live chances, calibration run, modular screen).
- **In progress:** Phase 0.9 (#138–#143, [plan](docs/plans/phase-0.9.md)).
- **Ready after 0.9:** #45, #71, #64, #91, #9, #94, #67.
- **Waiting on owner decisions:** #80 (Postgres cost), #83, #85, #89, #109 (GDELT from Railway), #121 (Claude news reader: key).
