# Status

Last updated: 2026-11-24 (analysis engine and global markets). This page is the honest answer to "what actually works?".
Planned work is tracked on the
[project board](https://github.com/users/Chandrashekhar-Hegde/projects/3) and in the
[milestones](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/milestones).

## Works (real data, tested)

| Area | What you get | Verified by |
|---|---|---|
| Market data | **Any exchange** via Yahoo: stocks, ETFs, indices, funds, crypto, FX, futures. Split- and dividend-adjusted daily bars (up to `max`), splits/dividends, delayed last price | Rust tests on recorded responses; API tests on in-process stubs; `scripts/smoke_api.sh`; live check |
| Live prices | `GET /api/v1/stream` pushes ticks from Yahoo's streamer over Server-Sent Events. One shared upstream connection, reconnects, last tick replayed to new viewers | Stub WebSocket test with a forced reconnect; live test (BTC-USD tick, 2 s lag) |
| Market state | `market_open` from Yahoo's own trading period, so weekends **and exchange holidays** show CLOSED without a holiday calendar | Unit test; confirmed on the 24 Nov 2026 NSE holiday |
| Data quality | Every bar series is checked: impossible prices, gaps vs the home-market index's trading days (34 exchanges + US), stale data, extreme returns (robust z > 8) | Unit tests; shown in the UI |
| Analysis engine | `GET /api/v1/analyze` for any symbol: return distribution (fat tails, Jarque–Bera, Hill), predictability (Ljung–Box, variance ratios, ADF), EWMA + GARCH(1,1)-t volatility and term structure, seeded Monte Carlo fans (GBM, GARCH-t), VaR/ES three ways, drawdown, Sharpe ± SE, beta/alpha vs an automatically chosen home-market benchmark, and plain-language findings. See [METHODOLOGY.md](METHODOLOGY.md) | Matches scipy/statsmodels/arch on 5y of AAPL, S&P 500, NIFTY (`scripts/gen_reference.py`); GARCH parameter recovery on simulated data; determinism per seed; API tests on stubs; live checks across 9 markets |
| Web app | **Markets** home for any exchange: search, global quick picks, live/delayed/closed quote pill with measured lag, candles moved by ticks, quality badge, corporate actions, analysis panels (findings, forecast fan, VaR/ES, volatility, tests, benchmark exposure), opt-in TradingView live chart | Vitest, Playwright on desktop + mobile with mocked data, axe accessibility check, live spec on real data |
| Hosting | Railway behind a shared access token (bearer or cookie login form); CORS closed; same-origin bundle | Smoke script checks the gate returns 401 |
| Engineering | Pre-push gate running everything CI runs | `scripts/check.sh` |

## Works, but on fixtures (not real data)

The **Event lab** tab (labelled in the app): event inbox (5 hard-coded events), the `/decide`
workbench, and Razorpay **test-mode** payments. `/decide` uses about 40 hand-picked,
uncalibrated constants; don't trade on it. Tracked in Phase 1.

## Doesn't work yet / limits to know

- **NSE live ticks are unverified.** The Yahoo stream works (crypto/FX ticks arrive 1–3 s
  after the trade), but 24 Nov 2026 was an NSE holiday, so no NSE tick could be observed.
  Until #44 measures it in market hours, assume NSE may be delayed. When no tick arrives,
  the app falls back to polling and says **DELAYED**.
- **Yahoo is unofficial.** It can rate-limit, block cloud IPs or change format. The fallback
  plan is NSE bhavcopy for end-of-day (#45) and a free broker feed for exchange-licensed live
  ticks (#68; needs your Upstox/Angel/Breeze/Dhan account and keys).
- **TradingView**: no data API exists. We embed its free widget for viewing only; its data
  can't be used in our analysis.
- **Forecasts are not yet validated out of sample.** The fans show the range a model implies;
  whether a model *beats a random walk* on unseen data is milestone 0.3 (walk-forward scoring,
  VaR backtests). Until then the app says so and treats forecasts as risk ranges.
- **Single instrument at a time.** Multi-ticker comparison (correlation, cointegration) is 0.4.
- **No accounts**: one shared beta token. No database on Railway (nothing needs one yet).
- Railway deploys from `main` automatically (#43 done).

## Planned

| Milestone | Scope |
|---|---|
| 0.1 Data foundation | Remaining: NSE stream measurement (#44), bhavcopy cross-check (#45), chrono replacement (#71) |
| 0.2 Analysis engine | Done in this release |
| 0.3 Validation harness | Walk-forward scoring vs baselines, Diebold–Mariano, VaR backtests |
| 0.4 Multi-instrument | Correlation, Ledoit–Wolf, PCA, cointegration |
| 0.5 Analyze web app | Fan charts, analysis panels, comparison view, methodology page |
| Phase 1 | Real events and news, market-model event study, calibrate or retire `/decide`, fundamentals |
| Phase 2 | Broker live ticks, accounts, portfolio analytics. Still no live orders |
