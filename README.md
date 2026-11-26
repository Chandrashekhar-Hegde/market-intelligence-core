# Market Intelligence Core

Market Intelligence Core is a Rust service for turning market events and
as-of market facts into deterministic, explainable trading decisions.

The core rule is simple: the decision path never calls live services, reads the
clock, or generates random identifiers. Every decision is computed from explicit
inputs: a normalized event, rule definitions, macro context, price facts,
technical features, prediction records, and bounded relationship modifiers.

License: PolyForm Noncommercial 1.0.0. This repository is source-available for
noncommercial use and is not open-source licensed.

## What Works Now

**Start with [docs/STATUS.md](docs/STATUS.md)**: what is live, what runs on fixtures, and
known limits. Planned work, in order: [docs/ROADMAP.md](docs/ROADMAP.md).

- Live market data for any exchange (Yahoo): search, adjusted daily history, delayed quotes,
  a Server-Sent Events price stream, and a data-quality report.
- An analysis engine for any symbol: fat-tail statistics, predictability tests, GARCH-t
  volatility, Monte Carlo forecast ranges, VaR/ES, drawdowns and home-market beta. It is
  checked against scipy/statsmodels/arch; see [docs/METHODOLOGY.md](docs/METHODOLOGY.md).
  Both are shown in the **Markets** view.

- **Compare** 2–10 instruments: correlation, the shared market factor, cointegrated pairs and
  relative performance.
- **Out-of-sample validation:** every forecast model is scored against a baseline before it is
  presented as useful.
- **How it works:** a plain-language guide in the app covering data sources, what is stored, and
  how to read each number.
- **Event lab (fixtures, experimental):** the original deterministic event scoring and decision
  workbench. Its BUY/SELL/HOLD output uses uncalibrated constants and is labelled as such in the
  app; #65 decides whether to calibrate it or retire it.

## Release Status

Phase 0 (0.1–0.5) is live as a private beta: real market data, the analysis engine, validation,
comparison and the guide. Next comes Phase 1 (events and fundamentals), then Phase 2 (portfolio);
see [docs/ROADMAP.md](docs/ROADMAP.md). There is no live broker order placement, ever.

## Roadmap and contributing

- **What's done and what's next, in order:** [docs/ROADMAP.md](docs/ROADMAP.md)
- **Design plans:** [docs/plans/](docs/plans/README.md) (Phase 0 history, Phase 1 events and fundamentals, Phase 2 portfolio)
- **How to pick up an issue, and the Definition of Done:** [CONTRIBUTING.md](CONTRIBUTING.md)
- **Setup, commands and gotchas, for humans and AI agents:** [AGENTS.md](AGENTS.md)
- **Ready-to-start work:** issues labelled [`agent-ready`](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/issues?q=is%3Aopen+label%3Aagent-ready)
- **Questions and ideas:** [Discussions](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/discussions)

## Workspace

| Crate | Purpose |
| --- | --- |
| `gm-domain` | Pure domain logic: rules, classification, scoring, indicators, features, predictions, event studies, risk checks. |
| `gm-integrations` | Provider traits and deterministic adapters for market data, events, filings, entity mapping, payments, and paper execution. |
| `gm-api` | HTTP API over the domain core. |
| `gm-persistence` | PostgreSQL repository and migration helpers. |
| `gm-worker` | Operational commands such as migration checks and future batch jobs. |

## Quick Start

Agents and new contributors: read [AGENTS.md](AGENTS.md) first.

```bash
git config core.hooksPath .githooks          # pre-push quality gate
(cd apps/web && npm ci && npx playwright install chromium)
scripts/check.sh                             # full gate, same as CI minus Docker
make run-api                                 # API on :8000
(cd apps/web && npm run dev)                 # UI on :5173
```

In another terminal:

```bash
make smoke-api
```

For PostgreSQL migration verification:

```bash
make verify-postgres
```

## Container

Build the combined API and web image:

```bash
make docker-build
```

Run it locally without PostgreSQL persistence:

```bash
docker run --rm -p 8000:8000 market-intelligence-core:local
```

The image serves API routes and the built web app from the same process. Set
`DATABASE_URL` to enable PostgreSQL persistence and `GM_MIGRATIONS` if the
migration directory is mounted somewhere other than `/app/migrations`.

## API

Full contract: [docs/API.md](docs/API.md) and `GET /openapi.json`.

Market data and analysis (behind the access gate):

- `GET /api/v1/instruments/search?q=`
- `GET /api/v1/bars?symbol=&range=`
- `GET /api/v1/quote?symbol=`
- `GET /api/v1/stream?symbols=` (Server-Sent Events)
- `GET /api/v1/analyze?symbol=&range=&benchmark=`
- `GET /api/v1/compare?symbols=&range=`

Operational and Event lab (fixtures):

- `GET /health`
- `GET /ready`
- `GET /version`
- `GET /openapi.json`
- `GET /rules`
- `POST /score`
- `POST /decide` (retired: 410 Gone)
- `POST /quant/features`
- `POST /predict/gbm`
- `POST /macro/context`
- `GET /payments/state`
- `POST /payments/orders`
- `POST /payments/verify`
- `POST /payments/webhooks/razorpay`

## Documentation

- [Agent guide](AGENTS.md)
- [Status](docs/STATUS.md)
- [History and decisions](docs/HISTORY.md)
- [Methodology](docs/METHODOLOGY.md)
- [API](docs/API.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Algorithm](docs/ALGORITHM.md)
- [Decision Model](docs/MODEL.md)
- [UI/UX](docs/UI_UX.md)
- [Knowledge Base](docs/KNOWLEDGE_BASE.md)
- [Integrations](docs/INTEGRATIONS.md)
- [Payments](docs/PAYMENTS.md)
- [Testing](docs/TESTING.md)
- [Debugging](docs/DEBUGGING.md)
- [Release Plan](docs/RELEASE_PLAN.md)
- [Release Checklist](docs/RELEASE_CHECKLIST.md)
- [Public Readiness](docs/PUBLIC_READINESS.md)
- [Roadmap](docs/ROADMAP.md)
