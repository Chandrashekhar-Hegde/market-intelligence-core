# Make it live: real data in the UI, tracked work, local gate

> **Status: Done** (PRs #72, #76). This is a condensed record of the plan approved on 2026-11-24.

## Why

The hosted app showed only fixtures; the backend had real data but nothing used it. The owner
asked for:
- a straight status report;
- the repo kept current, with issues, milestones and a board;
- detailed tests and a local gate that runs before any push;
- real-time data, from Yahoo and any other free source.

## Decisions

- **Linter:** keep oxlint, which is ESLint-compatible and already in CI.
- **How work lands:** a branch and PR, and the owner merges. Later the owner authorised the agent
  to merge green PRs.
- **Order:** build the live-data slice first, then 0.2 onwards as tracked issues.
- **Free real-time sources:**

| Source | Verdict |
|---|---|
| Yahoo streamer (WebSocket, protobuf) | Built. NSE about 2 s and BSE 15 min (measured 2026-11-25, #44) |
| Yahoo chart polling | The fallback when there are no ticks |
| TradingView widget | Display only; it has no data API |
| NSE bhavcopy | The official end-of-day file; cross-check and fallback (#45) |
| Free broker APIs (Upstox/Angel/Breeze/Dhan) | Need the owner's keys (#68) |
| NSE website JSON | **Rejected**: its terms forbid automated access |

## What was built

- `/api/v1/stream` (SSE) fed by one shared Yahoo WebSocket.
- `market_open`, taken from Yahoo's trading period, so holidays need no calendar of our own.
- The Markets view: search, live quote pill, candles moved by ticks, quality badge, TradingView
  widget. The old console moved to the Event lab tab.
- Tests at every layer: Rust stubs for HTTP and WebSocket, Vitest with a fake `EventSource`,
  Playwright with mocked `/api/v1`, axe, and `@live` specs.
- `scripts/check.sh` and `.githooks/pre-push`; `scripts/smoke_api.sh` runs against production
  with a token.
- GitHub milestones, issues with acceptance criteria, and the project board.
- `docs/STATUS.md`.

## Bugs the plan's tests caught

- A rustls provider panic that would have killed the stream in production.
- Two accessibility bugs.
- Nine RUSTSEC advisories, fixed by `cargo update`.
