# Status

Last updated: 2026-11-26.
Planned work, in order, is in [ROADMAP.md](ROADMAP.md) and the
[milestones](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/milestones).

## Works (real data, tested)

| Area | What you get | Verified by |
|---|---|---|
| Search | Any listing on any exchange, **Indian listings first**, with **Market and Exchange filters** (India → NSE / BSE, USA → NASDAQ / NYSE, …). BSE twins of NSE stocks and typed tickers (`500325.BO`) are resolved, because Yahoo's search rarely returns BSE | Mapping on a recorded Yahoo search; API stub tests (filters, twin found, missing twin dropped, typed ticker); Vitest; Playwright; smoke test |
| Market data | **Any exchange** via Yahoo: stocks, ETFs, indices, funds, crypto, FX, futures. Split- and dividend-adjusted daily bars (up to `max`), splits/dividends, delayed last price | Rust tests on recorded responses; API tests on in-process stubs; `scripts/smoke_api.sh`; live check |
| Live prices | `GET /api/v1/stream` pushes ticks from Yahoo's streamer over Server-Sent Events. One shared upstream connection, reconnects, last tick replayed to new viewers | Stub WebSocket test with a forced reconnect; live test (BTC-USD tick, 2 s lag) |
| Intraday + watchlist | `GET /api/v1/intraday` (1-minute today, 5-minute for 5 days). The Markets view opens on **today's minute chart for NIFTY 50**, with ticks appended live, and a **live watchlist** (NIFTY, Sensex, Bank Nifty, Reliance, HDFC Bank, TCS, Infosys, Gold ETF, USD/INR, Bitcoin, S&P 500) on one shared stream | Parser on recorded 1-minute responses; API stub test; Vitest with a fake EventSource; Playwright; live run (BTC chart moved with ticks, 26 Nov 2026) |
| Market state | `market_open` from Yahoo's own trading period, so weekends **and exchange holidays** show CLOSED without a holiday calendar | Unit test; confirmed on the 24 Nov 2026 NSE holiday |
| Data quality | Every bar series is checked: impossible prices, gaps vs the home-market index's trading days (34 exchanges + US), stale data, extreme returns (robust z > 8) | Unit tests; shown in the UI |
| Analysis engine | `GET /api/v1/analyze` for any symbol: return distribution (fat tails, Jarque–Bera, Hill), predictability (Ljung–Box, variance ratios, ADF), EWMA + GARCH(1,1)-t volatility and term structure, seeded Monte Carlo fans (GBM, GARCH-t), VaR/ES three ways, drawdown, Sharpe ± SE, beta/alpha vs an automatically chosen home-market benchmark, and plain-language findings. See [METHODOLOGY.md](METHODOLOGY.md) | Matches scipy/statsmodels/arch on 5y of AAPL, S&P 500, NIFTY (`scripts/gen_reference.py`); GARCH parameter recovery on simulated data; determinism per seed; API tests on stubs; live checks across 9 markets |
| Validated advice (API) | `/api/v1/analyze` returns `signals` and `advice`: six literature rules (trend, momentum, short-term reversal, RSI, unusual volume, OBV pressure), each backtested walk-forward on up to 10 years. Buy/Sell only when a rule earns its place (Holm-corrected, beats trading costs, stable, 30+ calls). Otherwise Hold, with the reason. On 26 Nov 2026, 1 of 37 instruments had a proven rule | Matches statsmodels on AAPL; planted trend found; 1.5% false rules on 200 random walks; API test on 10 years of RELIANCE |
| News | `GET /api/v1/news` and a news panel: English headlines from the last 3 days (GDELT). For each, the price move since the story beyond what the home index explains, and how unusual it is; this week's coverage tone vs the previous 3 months. Paced at one GDELT call per 5 s and cached 15 min; a rate limit shows as "news unavailable". News does not vote in the advice (needs stored tone history, #80). **In production GDELT currently answers 429 to Railway's shared outbound IP (checked 27 Nov 2026), so the panel says news is unavailable there**; fixing it needs a static egress IP or another source (owner decision, #83, #109) | Domain maths on constructed data; parser on GDELT's recorded rate-limit notice and on constructed fixtures in its documented shape; API stub tests (200 and 503); Vitest; Playwright |
| Comparison | `GET /api/v1/compare` for 2–10 instruments: daily, rank and weekly correlation (weekly first across time zones), rolling correlation, Ledoit–Wolf covariance, PCA market factor, Engle–Granger cointegration with half-life and a multiple-testing caveat, relative performance | Matches pandas/scikit-learn/numpy/statsmodels; a constructed cointegrated pair is found and an independent walk isn't; API tests on stubs |
| Plain-language guide | **How it works** tab: where the data comes from and how live it is, what is and isn't stored, how to read each panel, how models earn trust, limits, glossary | Vitest + axe |
| Web app (plain language) | Markets opens with **what to do** (Buy/Hold/Sell from proven rules, what each rule sees today, a live note when a tick would flip a rule, and how much to put in for a chosen bad-day loss). Then **likely close today and tomorrow**, recomputed on every tick, and cards that put risk in rupees for any amount. All statistics sit behind "Show all the maths". Three tabs; a compact top bar on phones | Vitest (plain-language maths, every advice state, live flip), Playwright desktop and phone with axe and the overflow check, live spec, screenshots |
| Web app | **Markets** home for any exchange: live watchlist, search, today's minute chart, live/delayed/closed quote pill with measured lag, candles moved by ticks, quality badge, corporate actions, analysis panels (findings, forecast fan, VaR/ES, volatility, tests, benchmark exposure), opt-in TradingView live chart | Vitest, Playwright on desktop + mobile with mocked data, axe accessibility check, live spec on real data |
| Hosting | Railway behind a shared access token (bearer or cookie login form); CORS closed; same-origin bundle | Smoke script checks the gate returns 401 |
| Engineering | Pre-push gate running everything CI runs; a fresh clone set up exactly as `AGENTS.md` says passes it (verified 2026-11-24, 85 s) | `scripts/check.sh` |

## Retired from the UI

The **Event lab** fixture tab (5 hard-coded events, the `/decide` workbench, Razorpay test mode) was
removed from the app on 26 Nov 2026; validated advice replaces it. `POST /decide` now answers
410 Gone (#65). The payment test-mode and `/events` fixture routes remain in the API only.

## Doesn't work yet / limits to know

- **Live coverage depends on the exchange.** Measured in a live session on 25 Nov 2026 (#44):
  - NSE stocks and NIFTY stream in **real time, about 2 s behind**, at roughly 1 tick per second.
  - BSE (`.BO`, `^BSESN`) is **exactly 15 minutes delayed** on Yahoo, and the app labels it
    "DELAYED 15 MIN".
  - Crypto and FX lag 1–3 s.
  - Other exchanges follow Yahoo's own terms; many are delayed.
- **Yahoo is unofficial.** It can rate-limit, block cloud IPs or change format. The fallback
  plan is NSE bhavcopy for end-of-day (#45) and a free broker feed for exchange-licensed live
  ticks (#68; needs your Upstox/Angel/Breeze/Dhan account and keys).
- **News in production:** GDELT rate-limits Railway's shared outbound IP (HTTP 429 on every try,
  27 Nov 2026). The news panel says "unavailable" there; locally it works when GDELT isn't
  throttling that machine. Options: a Railway static outbound IP (paid), or another lawful
  source (#83). Tracked in #109.
- **TradingView**: no data API exists. We embed its free widget for viewing only; its data
  can't be used in our analysis.
- **Validation covers 1-day forecasts.** Every analysis with 600+ returns walks forward and
  scores the 1-day forecast distributions against historical simulation, with VaR backtests.
  The multi-day fans are not yet scored separately; they come from the same models.
- **Comparison is descriptive.** Cointegrated pairs aren't backtested as a trading strategy yet,
  and correlations are full-period plus a rolling window, with no regime detection.
- **No accounts**: one shared beta token. No database on Railway (nothing needs one yet).
- Railway deploys from `main` automatically (#43 done).

## Planned

The ordered queue, with each item's state and blockers, is in [ROADMAP.md](ROADMAP.md). The design
is in [plans/](plans/README.md). In short:
- **Phase 1** adds real events and news, a correct event study, and fundamentals with a valuation
  given as a distribution.
- **Phase 2** adds portfolio risk (starting stateless, with nothing stored), then accounts, saved
  portfolios and read-only broker connections.
