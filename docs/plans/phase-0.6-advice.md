# Phase 0.6: an app a regular investor can act on

## Context
The owner's original intent is an app that tells a user **what to do**: Buy / Hold / Sell, when, how much, how confident, and why, with real-time prices, news and their impact. Today the app does none of that, and what it does show can't be read by a regular investor:
- **Live data is invisible.** The SSE stream (~2 s lag on NSE) only nudges the last *daily* candle. There is no intraday chart, one symbol at a time, and the default is `^GSPC`, which is closed in Indian hours.
- **No advice.** The project's rule 1 ("distributions, not calls") ruled calls out. The old `/decide` (40 uncalibrated constants) sits in the "Event lab".
- **No news.** GDELT (#81) is blocked on Postgres (#80), and Yahoo news returns nothing for Indian stocks (checked 2026-11-26).
- **Unreadable numbers**: Ljung–Box p-values, GARCH ν, pinball loss.

### Owner decisions (2026-11-26)
- **Audience:** Indian retail investor (NIFTY default, ₹, IST).
- **Real-time:** an intraday chart and a live watchlist on the existing Yahoo feed.
- **Calls only when proven.** Buy/Hold/Sell comes from a fixed set of literature signals, each backtested walk-forward on this instrument. A call is shown only if the signal earned its place. Otherwise: "Hold: no proven edge right now", with the evidence. Confidence is the measured historical hit rate, never a made-up score.
- **News:** GDELT, free and stateless (verified 2026-11-26: it returns Economic Times stories for Reliance). Tone comes from GDELT; impact is the measured price reaction.
- **Volume:** unusual volume, buying vs selling pressure, and how much to buy (position size).
- **Technical detail** stays behind "Show the maths".

**Expected honest outcome:** for most liquid stocks, most of the time, the engine will say Hold / no proven edge. That is the point of the validation; the UI explains it in one line.

Compliance note (in the docs, not legal advice): publishing buy/sell calls to the public in India may need SEBI Research Analyst registration. Fine for the private beta; check before a public launch.

### Housekeeping before PR 1
- Commit this plan as `docs/plans/phase-0.6-advice.md`.
- Amend AGENTS.md rule 1 to "Calls only from signals that passed out-of-sample validation, always shown with the distribution and the evidence".
- Open one GitHub issue per PR below and put them at the top of ROADMAP "Next".
- Note on #65 that this supersedes `/decide`; retire it in PR 3.
- Note on #81 that stateless GDELT ships here; storage still waits on #80.

---

## PR 1 `0.6-intraday-live`: data that visibly moves
- **Backend, `yahoo.rs`:**
  - `YahooClient::intraday(symbol, "1d"|"5d")` using `interval` 1m/5m with a 30 s cache. Reuse `validate_symbol`, `get_with_retry` and `ChartEnvelope`.
  - `parse_intraday` → `IntradayData { meta, points: [{t, open, high, low, close, volume}] }`. Skip all-null rows; a missing field is an error with the body.
  - `PriceBar` and the domain stay daily.
  - Record fixtures `yahoo_chart_reliance_1d_1m.json` and `yahoo_chart_btc_1d_1m.json`.
- **API:** gated `GET /api/v1/intraday?symbol=&range=`. Extend `yahoo_stub` to branch on the `interval` query. Tests for 200, 400 and 404; update openapi and `docs/API.md`.
- **Web:**
  - `useLiveTicks(symbols)`: one `EventSource` for ≤ 20 symbols (the server already takes comma lists).
  - A **watchlist strip**: NIFTY 50, Sensex, Bank Nifty, Reliance, HDFC Bank, TCS, Infosys, Gold, USD/INR, Bitcoin, S&P 500. Prices come from `useQueries(fetchQuote)` every 60 s, and ticks override them.
  - Chart ranges `1D` (default) and `5D` as an intraday area line with ticks appended live, then `1M 6M 1Y 5Y` as the existing candles. The default symbol becomes `^NSEI`.
- **Pure helpers in `marketData.ts`:**
  - `appendTick`;
  - `formatCompact` via `Intl.NumberFormat('en-IN',{notation:'compact'})` (lakh/crore);
  - plain `liveState` text: "Live · updated 2 s ago" / "Market closed · last price Thu 3:30 PM".
- **Tests:**
  - Rust parser on both fixtures;
  - Vitest for the helpers, plus a fake EventSource with two symbols;
  - a Playwright `/intraday` mock, watchlist and range specs, axe and phone width.

## PR 2 `0.6-signals`: the evidence engine (pure domain)
New `crates/gm-domain/src/analysis/signals.rs`. It takes `&[PriceBar]` (needs volume, high and low), is pure, and reuses `numeric::ols` and `validation::TestResult`.

**Signal library.** Fixed parameters from the literature; no tuning on the instrument:

| Signal | Rule (position +1 long / 0 out / −1 avoid) | Source |
|---|---|---|
| Trend | close vs 200-day average (and 50/200 cross) | Faber 2007; Moskowitz–Ooi–Pedersen 2012 |
| Momentum | 12-month return skipping the last month | Jegadeesh–Titman 1993 |
| Short-term reversal | 5-day return in its own bottom/top decile; RSI(14) < 30 / > 70 | Lehmann 1990; Wilder 1978 |
| Unusual volume | volume in its own top decile (calibrated) × day's direction | Gervais–Kaniel–Mingelgrin 2001 |
| Buying/selling pressure | OBV and accumulation/distribution 20-day slope sign | Granville 1963; Chaikin |

**Walk-forward backtest** for each signal × horizon h ∈ {5, 21, 63} sessions:
- *Data:* the position at close t uses data ≤ t only; the outcome is the forward h-day return.
- *Statistics:* hit rate with a Wilson 95% CI, and mean return when on vs buy-and-hold, net of a cited round-trip cost (STT + brokerage + charges, a parameter documented in METHODOLOGY). Use a Newey–West t-stat (lag h−1) for overlapping returns and count independent trades.
- *"Earns its place"* requires all of:
  - Holm-adjusted p < 0.05 across all 15 tests on this instrument;
  - a positive net edge;
  - the same sign in both halves of the sample;
  - at least 30 independent trades.

**Advice** (`analysis/advice.rs`, pure):
- *Stance:* **Buy** if a validated signal is long today and none validated is short. **Sell / avoid** if the reverse. **Hold** on conflict or when nothing is validated. A "Hold" from no evidence and a "Hold" from conflict are worded differently.
- *Timeline:* the validated horizon ("about 1 month"), a review date, and **exit levels** read off the signal rules ("the call flips if it closes below ₹2,410, its 200-day average"). Plus a risk line from the horizon VaR.
- *Confidence:* hit rate, its CI and n ("right 58% of the time over 41 past trades, 95% range 43–72%").
- *Position size:* the user's acceptable bad-day loss L ÷ the 1-day 95% ES → the maximum amount, plus the horizon-matched loss.
- *Reasons:* one plain sentence per signal, with its backtest row.
- *Context, not voting:* GARCH-t volatility regime, beta, today's relative volume and pressure.

**Wiring:** `analyze_instrument` adds `signals` and `advice` to the `/api/v1/analyze` response, so there is no new route.

**Tests:**
- Reference values for the indicators and backtest stats from pandas/statsmodels, added to `scripts/gen_reference.py` → `reference.json`.
- Behavioural tests:
  - a simulated series with planted momentum → Trend earns its place;
  - 200 seeded random walks → the false call rate stays ≤ 5% (Holm works);
  - the same seed gives a byte-identical advice.
- Docs: `docs/METHODOLOGY.md` (every rule, source, cost and criterion) and `docs/API.md`.

## PR 3 `0.6-advice-ui`: plain language, the advice card, the live forecast
- **Top of Markets: the advice card.** Stance in big type, a one-sentence why, the timeline and review date, confidence as a hit-rate bar with its range, exit levels, and the "If you put in ₹___ / can lose ₹___ on a bad day" position-size inputs. The disclaimer sits under it. "Show the maths" holds the signal backtest table.
- **Live forecast, recomputed on every tick, client-side** (pure, in `marketData.ts`):
  - **rest-of-today range:** current price × exp(±z·σ_GARCH,daily·√(remaining session fraction)), plus the tomorrow-close range;
  - **"would flip if it closes here"** when the live price crosses an exit level;
  - **live relative volume:** today's `day_volume` vs the 20-day average, scaled by elapsed session time. Mark it `ponytail:` as linear-in-time; intraday volume is U-shaped, so a profile is the upgrade path.
- **"What this means for you" cards.** A pure `plainLanguage.ts` that only uses API values or exact identities:
  - **typical day:** σ/√periods, shown in ₹ on the amount;
  - **vs the market:** benchmark σ = corr·σ/β;
  - **a bad day:** 1 in 20, from VaR/ES;
  - **worst fall:** with dates and recovery time;
  - **where it could be:** fan quantiles in ₹ at 1 week / 1 month / 3 months;
  - **can it be predicted?** and **can you trust these ranges?**, from validation;
  - **moves with the market:** β.
  - Each card has a "Show the maths" holding today's table, moved there rather than deleted.
- **Layout:**
  1. watchlist;
  2. header;
  3. chart;
  4. advice;
  5. cards;
  6. news (PR 4);
  7. "Show all the maths" (quality, actions, tests, volatility, validation, VaR), collapsed;
  8. TradingView.
- **Shell:** rename "Command Center" to "Market Intelligence", show the API URL only in dev, and **retire `/decide` and the Event lab tab** (remove the route, its UI and tests; close #65 with the reason).
- **Docs and tests:** update the Guide tab and STATUS. Vitest for `plainLanguage.ts` and the live-forecast helpers (fixture response, index-without-benchmark, short history, amount scaling); Playwright for the advice card states (Buy, Hold-no-edge, Hold-conflict), the maths expanding, axe and phone width.

## PR 4 `0.6-news`: real-time news and its impact
- **Spike first:** is GDELT reachable from Railway, and what is its rate limit (about 1 request / 5 s)? Record the answer in the issue.
- **Backend:** `gm-integrations/src/gdelt.rs` (client + parser, recorded fixtures).
  - `artlist`: query is the quoted company name from the quote meta with legal suffixes stripped, plus `sourcelang:english`, over the last 3 days.
  - `timelinetone`: over 3 months.
  - A 10-minute cache and a single-flight request queue. Gated `GET /api/v1/news?symbol=`.
- **Impact** (pure domain, reusing the intraday points):
  - for each headline, the stock's move since `seendate` minus β × the benchmark's move;
  - expressed as a z-score against σ√(elapsed fraction): "since this story, it is up 1.6% more than NIFTY, a move seen on about 1 day in 12 without news";
  - also this week's tone vs its 3-month average.
- **Honest limit, stated in the UI:** GDELT's search window is about 3 months, so news tone **cannot yet be a validated signal** and does not vote in the advice. It becomes one once daily tone is stored (#80 Postgres) and has enough history. File that follow-up issue.
- **UI:** a news panel with headline, source, time ago, tone chip and measured reaction. Note that headlines are those mentioning "<name>", so some may be off-topic.
- **Tests:** parser fixtures, API stub, impact maths on constructed data, and a Playwright mock.

---

## Verification (each PR)
1. `scripts/check.sh` (long timeout) must print "All checks passed."
2. `scripts/check.sh --live` before deploy.
3. **Real run** (`cargo run -p gm-api` + `npm run dev`), with desktop and phone screenshots:
   - BTC-USD for live ticks at any hour;
   - NSE symbols during 09:15–15:30 IST;
   - for PR 2/3, the advice on ^NSEI, RELIANCE.NS, AAPL and BTC-USD, and whether any signal earns its place (report the counts as they are).
4. **After merge:** wait for the Railway deploy SUCCESS, then run the production smoke. Add `/intraday`, and later `/news`, checks to `scripts/smoke_api.sh`.
5. Update STATUS and ROADMAP in each PR.

## Out of scope
- **News tone as a voting signal:** needs stored history (#80).
- **Broker-licensed ticks (#68):** needs owner keys.
- **Claude-based headline classification:** possible later if GDELT tone proves too coarse.
- **Plain-language pass on the Compare tab:** a follow-up issue.
- **No live order placement** (rule 6 unchanged).
