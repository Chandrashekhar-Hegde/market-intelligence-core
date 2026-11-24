# Architecture

Market Intelligence Core is split into a pure domain layer and thin operational
layers around it. This keeps decision behavior testable and replayable.

## Stack

- Rust 2024 workspace.
- Axum and Tokio for HTTP.
- SQLx and PostgreSQL for append-only facts.
- Docker Compose or local PostgreSQL tools for migration verification.
- Minimal CI for format, lint, tests, and dependency audit.

## System Shape

```mermaid
flowchart LR
    Source["Market / Macro / Broker Inputs"] --> Normalize["Normalize"]
    Normalize --> Event["NormalizedEvent"]
    Event --> Domain["Pure Domain Core"]
    Facts["As-Of Facts: PriceBar / FeatureVector / PredictionRecord / MacroContext"] --> Domain
    Domain --> Decision["Decision"]
    Decision --> Risk["Risk Check"]
    Risk --> Execution["Execution Adapter"]
    API["HTTP API"] --> Domain
    Worker["Worker Commands"] --> Storage["PostgreSQL"]
```

## Market data and analysis path (current product)

```mermaid
flowchart LR
    Yahoo["Yahoo chart / search (HTTP)"] --> Client["gm-integrations::yahoo\nretries · TTL cache · parsing"]
    Streamer["Yahoo streamer (WebSocket, protobuf)"] --> Hub["gm-integrations::yahoo_stream\none shared upstream · broadcast"]
    Markets["gm-integrations::markets\nsuffix → home benchmark · TradingView name"] --> API
    Client --> API["gm-api /api/v1\nsearch · bars · quote · analyze\naccess gate · CORS"]
    Hub --> SSE["gm-api /api/v1/stream (SSE)"]
    API -->|"prices in, report out"| Engine["gm-domain::analysis (pure)\ndistribution · predictability · volatility\nmontecarlo · risk · validation"]
    API --> Quality["gm-domain::quality (pure)"]
    API --> Web["apps/web Markets + Analysis panels"]
    SSE --> Web
```

- **Providers fetch, the domain computes.** `analyze_instrument` in `gm-api/src/lib.rs` fetches the
  symbol's bars and the auto-selected benchmark's bars. It then calls
  `gm_domain::analysis::analyze` on a blocking thread and returns the report together with data
  provenance and the quality report.
- **The stream** keeps one upstream WebSocket for all viewers, using ref-counted subscriptions,
  15-second renewals and reconnect with backoff. New viewers first receive the last tick per symbol.
- **No database is involved.** The 15-minute in-process chart cache is enough for one instance.
  Yahoo restates adjusted closes after dividends, so persisting bars would need invalidation.

The older event and decision path below still powers the "Event lab (fixtures)" tab.

## Boundaries

- `gm-domain` contains pure functions and serializable data types.
- `gm-integrations` defines provider traits, the Yahoo chart/search client, the live stream, the
  market table, and deterministic provider fixtures.
- `gm-api` converts HTTP requests into domain inputs and returns domain outputs.
- `gm-api` also performs optional audit persistence when `DATABASE_URL` is set.
- `gm-persistence` owns database connection, migration helpers, and append-only
  audit writes.
- `gm-worker` owns operational commands and future batch jobs.

## Invariants

- Scoring and decision functions do not call networks, providers, or databases.
- Scoring and decision functions do not read wall-clock time.
- Decision IDs are deterministic for the same event, facts, score, and action.
- Price is always an injected as-of fact.
- Derived facts are written upstream, then passed into the decision path.
- Database tables that represent facts are append-only by design.

## Minimal Runtime

Development requires Rust and Cargo. PostgreSQL is only required for migration
and persistence checks; the domain and API smoke tests can run without a
database.

When `DATABASE_URL` is configured for `gm-api`, startup applies migrations by
default. `POST /decide` then stores the normalized event, rule traces, decision,
and replay input snapshot. The domain crate remains database-free.

Provider adapters live outside the domain. They normalize external market,
filing, payment, event, entity, and broker surfaces into explicit inputs that
can be reviewed, persisted, and replayed.
