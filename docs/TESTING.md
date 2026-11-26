# Testing

Testing focuses on proving that the domain math is deterministic and that the
runtime paths work locally.

## Local Gate (runs before every push)

```bash
git config core.hooksPath .githooks   # once per clone: enables the pre-push hook
scripts/check.sh                      # what the hook runs; same checks as CI minus Docker
scripts/check.sh --fast               # skip Playwright while iterating
scripts/check.sh --live               # also hit real Yahoo (run before a deploy)
```

`scripts/check.sh` runs, and stops at the first failure:

| Step | Command |
|---|---|
| Rust format | `cargo fmt --all -- --check` |
| Rust lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Rust tests | `cargo test --workspace --all-features` |
| Dependency audit | `cargo audit --deny warnings` (skipped if not installed; CI always runs it) |
| Web lint | `npm run lint` (oxlint with react, react-hooks, jsx-a11y, typescript, vitest rules; warnings fail) |
| Web types + build | `npm run build` (`tsc -b && vite build`) |
| Web unit tests | `npm run test` (Vitest + Testing Library) |
| Browser tests | `npx playwright test` (desktop + mobile Chrome, axe accessibility, p95 budget) |

Browser tests mock `/api/v1/*` (`tests/e2e/marketMocks.ts`), so they never depend on Yahoo.
Real-network checks are opt-in:

- `cargo test --workspace -- --ignored live` hits Yahoo chart, search and the streamer.
  Set `LIVE_STREAM_SYMBOL=RELIANCE.NS` in NSE hours (09:15–15:30 IST) to measure NSE lag.
- `npm run test:e2e:live` (`@live` specs) drives the UI against a local API on real data.

### What the market-data tests cover

- Provider parsing on recorded Yahoo responses: adjustment, null rows, partial session bar,
  splits, not-found, search filtering, input validation.
- Stream decoding on recorded protobuf frames, plus an in-process WebSocket stub that hangs
  up after each replay to prove reconnects. A regression test makes sure TLS works with
  both rustls backends linked (sqlx brings ring, reqwest brings aws-lc-rs).
- API behaviour against in-process Yahoo stubs: bars + quality report, quote and market
  state, 404 / 503 mapping, search filtering, access gate on market routes, stream limits.
- UI: live/delayed/closed logic, tick-to-candle merging across exchange dates, TradingView
  symbol mapping, search → select flow, readable errors, failing quality reports.

## API Smoke Test

Start the API:

```bash
make run-api
```

In another terminal:

```bash
make smoke-api
```

The smoke test verifies:

- `GET /health` returns `ok`.
- `GET /ready` returns readiness and persistence status.
- `GET /version` returns service and model versions.
- `GET /openapi.json` exposes the contract paths.
- Payment state reports Razorpay test mode with live billing disabled.
- Payment order creation returns a deterministic test order and checkout
  signature fixture.
- Checkout signature verification succeeds for the fixture and webhook
  verification succeeds for a signed raw body.
- `POST /decide` answers 410 Gone and points to `/api/v1/analyze` (retired, #65).

## Browser Smoke Test

```bash
make web-e2e
```

This starts the Rust API without persistence, starts the Vite app with
`VITE_API_BASE_URL` pointed at that API, filters the Event Inbox fixture feed,
checks selected event detail metadata, submits the Decision Workbench fixture,
runs the visible payment test-mode verification flow, and checks that the
evidence, risk gates, replay metadata, similar-event history, and missing-fact
state render in Chromium.

The browser suite also runs:

- desktop and mobile projects for responsive coverage
- primary-screen accessibility scan for WCAG 2.x A/AA serious and critical
  violations
- UI decision-flow p95 performance check; default budget:
  `UI_DECISION_P95_MAX_MS=2500`

## Performance Gates

```bash
make perf-check
```

This runs:

- `cargo test -p gm-domain --test decision_perf --all-features`
- `npm run test:e2e --prefix apps/web -- --grep p95`

The Rust decision-path check measures score plus decision generation over a
deterministic fixture. Default budget: `DECISION_P95_MAX_MICROS=25000`.

The browser p95 check of the old `/decide` workflow was removed with the Event lab (#100).

## PostgreSQL Migration Check

```bash
make verify-postgres
```

This starts local PostgreSQL through Docker Compose when Docker is available.
When Docker is not installed and `DATABASE_URL` is not set, it starts a
temporary local PostgreSQL cluster, applies SQLx migrations through `gm-worker`,
and checks that the expected schema exists.

## Domain Coverage

Current unit tests cover:

- keyword word-boundary matching
- event classification
- macro normalization and sector weighting
- indicators and feature as-of cutoffs
- GBM quantile determinism
- flow adjustment caps
- event-study forward-return math
- BUY/SELL/HOLD decision behavior
- deterministic decision IDs
- provider adapter fixtures for market data, events, filings, entity mapping,
  payments, and paper execution
- payment API state, order, checkout verification, signed webhook, and
  bad-signature contracts
- Event Inbox API list/detail contracts and component filtering
- Decision Workbench component and browser smoke coverage
- primary-screen accessibility scan
- pure decision-path and UI decision-flow p95 performance gates

## What Still Needs Coverage

- repository read integration tests against PostgreSQL
- replay tests that rebuild projections from append-only facts
- live broker execution state-machine tests
- NSE tick coverage and lag of the Yahoo stream in market hours (#44)
