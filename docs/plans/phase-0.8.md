# Phase 0.8 (planned 2026-11-27): today's actions, every asset, news desk, live probabilities, modular UI

## Context
Owner feedback after 0.7 (27 Nov 2026):
- **A list of what to do today:** buy, sell, hold, and the best position.
- **Coverage beyond Indian stocks:** gold and other assets, US investing from India, and "Groq, Zero" (meaning unclear, asked).
- **A news desk like Bloomberg** that the app reads and acts on.
- **A calibration run now.** Plug in an LLM URL later and fit the weights from the data.
- **The Monte Carlo still looks fake.** The model that validation rates best should be primary, not a fixed default. It should update with every live tick, and the most probable pathway should be highlighted.
- **Probabilities everywhere:** think in probabilities, not possibilities.
- **A modular UI** with panels the user can resize.

## Research and real-data findings (27 Nov, production, market closed)

**Works**
- Yahoo returns 10 years of daily data for gold (GC=F, GOLDBEES.NS), silver (SI=F), the US 10y yield (^TNX), USD/INR (INR=X), US stocks (AAPL, NVDA), the S&P 500, MON100.NS (an Indian-listed Nasdaq ETF) and BTC.
- Free news feeds return headlines from here:

  | Feed | Items |
  |---|---|
  | Bloomberg Markets RSS | 20 |
  | Google News `site:bloomberg.com` | 100/day |
  | Business Standard Markets | 35 |
  | BusinessLine | 60 |
  | CNBC | 30 |
  | Moneycontrol | 18 |
  | Mint | 35 |
  | Google News for topics (e.g. "gold price india") | 62 |

  Bloomberg gives headlines and links only; the articles are paywalled.
- The forecast already computes P(gain), P(loss over 10%) and P(drawdown over 10%) per horizon. The UI barely shows them.

**Not working, or misleading**
- **Speed.** A 10y `/analyze` takes 13–23 s in production and is not cached; 5y takes 1.7 s. `/news` takes 13.7 s, mostly GDELT, which still returns 429. A daily list over about 60 instruments is impossible until this is fixed.
- **No proven rules.** None of the 10 assets above has a proven rule, so all are Hold (earlier: 1 of 37). An honest "today's actions" list will mostly say "nothing proven today". The value is in the probabilities and the few real calls.
- **The P(up) figures are inflated by drift.** P(up at 3 months) is 64–72% for AAPL, NVDA, gold and the S&P, largely because of their 10-year average growth, and the UI must say so.
- **The primary fan ignores validation.** It is always the bootstrap, even where GARCH-t "earns its place" (AAPL, NVDA). This is the "fake default" complaint.
- **ET and Mint found no stories for TCS today.** Google News carries the per-stock news.

**Rules found** (secondary sources, to be verified on official sites)
- **US investing from India:**
  - the LRS limit is $250k per person per financial year;
  - from 1 Apr 2026, TCS is 20% on the amount above ₹10 lakh;
  - capital gains on foreign shares are taxed at slab rates up to 24 months and 12.5% LTCG after;
  - the routes are a direct US broker (INDmoney etc.), NSE IFSC in GIFT City, or Indian-listed international ETFs and funds.
  - Groww's and Zerodha's current US offerings were not confirmed; check their own pages before writing the card.
- **SEBI:** publishing stock-specific buy/sell recommendations requires Research Analyst or Investment Adviser registration. SEBI enforced this against unregistered "educators" in Dec 2025 (₹546 cr impounded). A private beta for the owner is one thing; a public "buy these today" list is another. Owner decision.

## Owner decisions (27 Nov)
- **"Groq, Zero" = both.**
  - The routes for buying US stocks from India: Groww, Zerodha, INDmoney, GIFT City and Indian-listed ETFs.
  - An AI-theme view: listed AI plays (NVDA, AMD, AVGO, TSM, MSFT, Indian IT) analysed normally. Private companies (Groq etc.) are covered by news only.
- **"Today's actions" stays in the private beta** behind the token. It carries a visible notice: "research signals, not SEBI-registered advice". Revisit before any public launch.
- **Calibration storage is a Railway volume** mounted on gm-api, holding a JSONL file (~$0.25/GB-month). Creating it is the one infra change in this phase.
- **Today's universe:** NIFTY 50, gold, silver and the rupee, US exposure (MON100 plus major US and AI stocks in INR), and the user's watchlist. The browser sends the watchlist symbols with the request.

## Plan (one PR each, the existing gate, deploy and production smoke)

### 1. `0.8-speed` (#123): analysis in under 3 s, news in under 3 s (blocks the rest)
- Profile `analyze` at 10y (signals walk-forward vs GARCH validation refits). Run independent parts in parallel with `spawn_blocking` and `join`.
- Cache the result per (symbol, range, last bar date). Daily bars change once a day, so live re-anchoring (PR 3) reuses it.
- GDELT circuit breaker: after a 429, skip it for 15 min. Put a 4 s per-source cap on news.
- Smoke-test timings against production.

### 2. `0.8-probabilities` (#124): every number as a chance; the rated model leads
- **Primary fan = the best-rated model:**
  - if a model "earns its place" in validation, it is used;
  - otherwise the historical bootstrap, labelled "nothing beat history here";
  - a rating badge goes on each model, replacing `preferredProjection` in `apps/web/src/plainLanguage.ts`.
- **Wording uses natural frequencies**, as in Gigerenzer & Hoffrage (1995), cited in METHODOLOGY:
  - forecasts: "64 in 100 chance higher in 3 months";
  - risk: "1 in 9 chance of a fall over 10%";
  - rules: "right 58 times in 100 (likely 52–64)";
  - news-driven calls: "chance not measured yet".
- **Drift is separated out:** the "how much of this is just its long-run growth" line compares the forecast with zero-drift P(up).
- **Are the probabilities honest?** Add the Brier score of P(gain) vs a coin flip to the walk-forward validation in `analysis/validation.rs`. The UI says "when this said 60%, it happened X% of the time".
- **Typical path:**
  - highlight the sample path closest to the pointwise median across the horizon (the medoid), labelled "a typical path, not a promise";
  - show the 50% band as "1 in 2 chance it ends inside here".
  - A random walk has no single "most likely path", and the text says so.

### 3. `0.8-live-forecast` (#125): the fan moves with every tick
- New `GET /api/v1/projection?symbol=&price=`. It uses the cached fit from PR 1, re-anchors at the live price, updates GARCH variance with today's partial return, and reruns the paths (milliseconds).
- The web calls it on each SSE tick, throttled to one call per 5 s. The fan, the probabilities and the typical path redraw, showing "updated 09:42:13 IST".
- Measure the compute time. Verify during NSE hours (09:15–15:30 IST).

### 4. `0.8-news-desk` (#126): market news like a terminal, read and acted on
- **Sources:** add Bloomberg Markets, Business Standard, BusinessLine, Moneycontrol and CNBC to `crates/gm-integrations/src/news.rs` (reusing `parse_rss` and the per-source status). Add Google News topic queries for gold, rupee, RBI, Fed, crude and US markets.
- **Desk tab:** headlines grouped by theme (India market, global, rates, currency, commodities), each with a source, a time and the reader's verdict.
- **Macro reader** (`analysis/news_reader.rs`): documented patterns map themes to affected assets, then each asset gets a news vote using the existing rules:
  - RBI cut → banks and bonds;
  - crude up → OMCs and the rupee;
  - Fed hike → US stocks and gold;
  - and so on.
- The weights stay uncalibrated and labelled until PR 5 fits them.

### 5. `0.8-news-record` (#127): the calibration run (on a Railway volume, JSONL)
- Record every headline read: time, symbol or theme, the built-in reading, and later the LLM reading.
- After each one, record the abnormal return at 1, 5 and 21 days (the existing market-model impact).
- **Fit weights:** an event study per kind and direction (mean abnormal return with CI), with at least 30 events per kind before a weight replaces its constant.
- **Compare readers:** once `GM_LLM_*` is set, re-read the stored headlines with the LLM. Keep whichever reader predicts the hold-out returns better.
- Expect about 2–3 months to reach 30 events per kind on about 60 stocks.

### 6. `0.8-today` (#128): today's actions, assets beyond stocks, the best position
- **Universe:** NIFTY 50, gold (GOLDBEES), silver (SILVERBEES), NIFTY, US via MON100 plus direct US stocks in INR, USD/INR and BTC (final list per answers). It is precomputed after the NSE close and at the open, and cached.
- **The "Today" page:**
  - lists Proven Buy, Proven Sell, News-driven (no track record) and Hold (nothing proven), each item with its chance, horizon and reason;
  - says plainly when the Buy and Sell lists are empty.
- **AI-theme card:**
  - the listed AI plays with their chances;
  - a news strip for private AI names (Groq, OpenAI, xAI) from Google News, labelled "not tradable".
- **US from India card:**
  - the routes (Groww, Zerodha, INDmoney, GIFT City, Indian ETFs);
  - returns in INR (the USD move plus the rupee move);
  - the LRS and TCS rules with their sources;
  - a warning about Indian-ETF premiums to NAV (verify).
- **Best position:**
  - the user enters holdings or a budget;
  - with no proven edge, the honest mix is risk-balanced (equal risk contribution, using the shrinkage covariance already in `analysis/multi.rs`), not return-chasing;
  - proven calls tilt within the existing position-size limits;
  - shown as "a mix to consider", never an order.

### 7. `0.8-modular-ui` (#129): panels you arrange and resize
- `react-grid-layout`: each panel is a card the user can drag, resize, collapse or hide, with the layout saved per browser (localStorage, in try/catch).
- Phone width falls back to a single stack with collapse only.
- "Reset layout" is available. axe and the no-horizontal-overflow Playwright checks are kept.

## Verification
- `scripts/check.sh` on each PR, plus `--live` before deploy.
- Production smoke with timing checks:
  - analyze under 3 s warm;
  - projection under 300 ms.
- Real-data runs on Reliance, HDFC Bank, GOLDBEES, AAPL and MON100.
- One NSE-hours session to watch the live fan update. Screenshots at desktop and phone widths.
- STATUS, ROADMAP, METHODOLOGY and API.md updated per PR. The plan is committed as `docs/plans/phase-0.8.md`, with an issue per PR.

## Not in scope
- Placing orders.
- Full Bloomberg articles (paywalled; headlines and links only).
- Private companies (not tradable, no prices).
