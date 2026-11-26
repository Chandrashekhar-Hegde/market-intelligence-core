# Phase 0.7: find any listing, understand the advice, real forecasts, news that counts

## Context
Owner feedback on the live 0.6 app (27 Nov 2026):
1. **Search** must reach NSE/BSE and every exchange, with **market filters** (country, exchange), India first.
2. **"Hold" is ambiguous.** It means different things to someone who already owns the stock and to someone thinking of buying. "Leans buy / leans sell / not proven here" are not explained.
3. **Analyse 5–10 years** of daily prices. Analysis is 5y today; signals already use 10y.
4. **The Monte Carlo looks fake.** It is, in a sense. Every symbol uses seed 42, so every stock gets identical random draws: GBM gives the same fan shape, only stretched by volatility.
5. **News doesn't work in production** (GDELT returns 429 to Railway). The app must read news itself and **news votes directly** in the advice (owner decision). No Claude key yet: use something free now, with a slot for Claude later.
6. **The Compare tab is confusing**: PCA, shrinkage and cointegration up front.
7. **Advise, never decide.** The app says what to do and why; it places no orders.

Already verified (27 Nov):
- Yahoo search with `region=IN` returns NSE listings properly ("tata" → TATASTEEL.NS, TCS.NS, …).
- Google News RSS returned 42 Reliance stories over 3 days; the ET Markets and Mint RSS feeds work; Moneycontrol RSS returned nothing.

Six PRs in order, each behind the gate, deployed and smoke-tested. Housekeeping first:
- commit this plan as `docs/plans/phase-0.7.md`;
- open one issue per PR (milestone "0.7");
- amend AGENTS rule 1 for news-driven calls;
- update ROADMAP and the board.

---

## PR 1 `0.7-search-markets`: search any listing, filter by country and exchange
- `crates/gm-integrations/src/markets.rs`:
  - add `country` and `exchange_name` to each `Market` row (NS → India/NSE, BO → India/BSE, …);
  - add a small table of Yahoo US exchange codes (NMS, NYQ, ASE, …) → USA;
  - add `listing_market(symbol, exchange_code) -> (country, exchange)`, reused everywhere.
- `yahoo.rs` search:
  - pass `region`/`lang` (default `IN`/`en-IN`) and `quotesCount=25`;
  - `SearchHit` gains `country` and `exchange_name`.
- `GET /api/v1/instruments/search?q=&country=&exchange=` filters server-side; the market list is returned with the hits.
- **UI** (`InstrumentSearch` in `Markets.tsx`, also used by Compare):
  - **Country** chips: India (default), USA, UK, Japan, …, All;
  - **Exchange** chips for the chosen country (India → NSE, BSE);
  - each result shows "NSE · India", and Indian listings sort first;
  - a typed exact ticker (e.g. `500325.BO`) is accepted directly.
- **Tests:**
  - mapping on the recorded search fixture;
  - API filter test with the stub;
  - Vitest for the chips and filtering;
  - Playwright for "reliance" + India + BSE → RELIANCE.BO.

## PR 2 `0.7-advice-explained`: say what Hold, Leans and "Not proven" mean, on 10 years
- **Advice card** (`Analysis.tsx`): every stance gets two plain lines.
  - Buy: "If you own it: you can add, up to ₹X. If you don't: you can buy, up to ₹X."
  - Hold: "If you own it: keep it; nothing proven says sell. If you don't: no proven reason to buy now; wait."
  - Sell: "If you own it: consider selling or reducing. If you don't: don't buy now."
  - The position-size figure is reused in these lines.
- **Rule rows, in words:**
  - **Leans buy:** "this rule's reading points up today".
  - **Not proven here:** "when we tested this rule on this stock's last 10 years, it wasn't right often enough to beat trading costs, so it doesn't count".
  - A one-time "What do these labels mean?" explainer sits above the list.
- **10 years by default:**
  - `/api/v1/analyze` and the web default become `10y`, which gives validation more data;
  - a 5y / 10y switch in "What this means for your money";
  - card copy says the period actually used, falling back to 5y if the history is shorter.
- **Tests:** Vitest for the wording of each stance and the switch; Playwright updates.

## PR 3 `0.7-monte-carlo-own-history`: forecasts built from each stock's own past
- `crates/gm-domain/src/analysis/montecarlo.rs`:
  - **seed per symbol:** seed = FNV-1a(symbol) XOR the request seed. It stays deterministic, but each stock gets its own draws;
  - **new model `bootstrap`:** a stationary bootstrap of the stock's own daily returns (Politis & Romano 1994). The mean block length comes from Politis & White (2004) automatic selection, so the stock's crash days and calm spells drive the fan. This is the same history-based baseline the validation already scores against, so it is the default display. GARCH-t and GBM stay as alternatives.
  - return ~30 **sample paths** per projection for display, not only quantiles.
- **UI fan** (`FanChart`): faint sample paths behind the 50% and 90% bands; the axis is % change from today; the caption reads "each grey line is one possible future built from this stock's own past days". Also: "how wide vs NIFTY: 1.6×".
- **Tests:**
  - same seed + symbol gives byte-identical output; two symbols give different paths;
  - bootstrap mean and σ match the historical sample within tolerance;
  - the block length matches a reference (`gen_reference.py`, arch `optimal_block_length`).
- METHODOLOGY section.

## PR 4 `0.7-news-sources`: news that works from Railway
- `crates/gm-integrations/src/news.rs` (new; GDELT moves behind it):
  - **sources:**
    - Google News RSS: `q="<name>" when:3d`, `hl=en-IN&gl=IN&ceid=IN:en`;
    - ET Markets RSS and Mint Markets RSS, kept only where an item mentions the name;
    - GDELT as an extra.
  - each source is fetched independently with its own cache (15 min) and pacing. A source failure is reported per source and never empties the list;
  - stories are merged and de-duplicated by normalised title;
  - RSS is parsed with a small XML reader, or `quick-xml` if it is already in `Cargo.lock`.
- `/api/v1/news` returns `sources: [{name, ok, count, error}]` plus the merged stories with the existing impact maths (`analysis/news.rs`).
- **Fixtures:** record real Google News, ET and Mint responses (this works from here) and replace the constructed GDELT fixtures where possible (#109).
- **Spike after deploy:** check each source from Railway, and note the results in STATUS.

## PR 5 `0.7-news-reader-advice`: the app reads the news, and news votes
**Reader** (`crates/gm-domain/src/analysis/news_reader.rs`, pure, free, in-process), one result per headline:
- **Event type**, from documented patterns: results beat/miss, order or contract win, guidance up/down, rating or broker upgrade/downgrade, fraud/probe/raid/penalty, default/downgrade of debt, management exit, block/bulk deal, dividend/buyback, merger/acquisition, regulatory approval/ban.
- **Direction**, using negation and reversal handling ("probe dropped", "denies", "no impact"), plus the Loughran–McDonald (2011) finance sentiment word lists for the rest. Confirm the licence allows non-commercial use, then cite it in METHODOLOGY.
- **Materiality** (high/medium/low) by event type, and a one-line "why" in plain words.

**Optional model slot (free tiers, Claude later):**
- Any OpenAI-compatible chat endpoint set by env: `GM_LLM_BASE_URL`, `GM_LLM_API_KEY`, `GM_LLM_MODEL`.
- Free-tier examples to evaluate at build time: Groq, Google Gemini (OpenAI-compatible endpoint), OpenRouter free models, or a self-hosted Ollama.
- It returns the same JSON fields, is cached per headline, and falls back to the built-in reader on any error.
- A follow-up issue adds a Claude adapter when a key exists.

**News vote** (owner decision 2026-11-27: news votes directly):
- **News score** = Σ over distinct stories in the last 3 days of direction × materiality weight × recency decay.
- Thresholds turn the score into strong positive, strong negative or neutral. **These constants are uncalibrated.** METHODOLOGY says so, and #107 calibrates them once news history is stored (#80).

**Combining with the proven rules** (`analysis/signals.rs::advise` gains a news input; still pure):
| Situation | Call |
|---|---|
| A proven rule is active, and news agrees or is neutral | the rule's call; the news is mentioned |
| A proven rule is active, but strong news points the other way | Hold, labelled "conflict: rule vs today's news" |
| No proven rule, strong news | Buy or Sell **"based on today's news; no track record yet"**, with the headlines behind it |
| Otherwise | Hold, as now |

- The advice card gets a "News today" block: each material headline with its type, direction, the one-line reason, and the measured price reaction since publication.
- AGENTS rule 1 is amended for news-driven calls (always labelled, never hidden).
- **Tests:**
  - reader tests over a table of real-style headlines, including negations;
  - combination tests for each row above;
  - an LLM-slot test against a stub server, including the fallback;
  - Vitest and Playwright for the news-driven Buy and Sell, and the conflict.

## PR 6 `0.7-compare-plain` (#108): Compare in plain words
- Lead with a **head-to-head table**: 10-year return, typical day, worst fall, and today's advice for each instrument.
- Then two sentences:
  - "Do they move together?", in words, from correlation;
  - "Owning both instead of one cuts the typical swing by X%", from the covariance already computed.
- Matrices, PCA, shrinkage and cointegration fold under "Show all the maths", as on Markets.
- The search uses the PR 1 filters.

---

## Verification (each PR)
1. `scripts/check.sh` (long timeout) must print "All checks passed."
2. `scripts/check.sh --live` before deploy.
3. **Real run** (`cargo run -p gm-api` + `npm run dev`), with desktop and phone screenshots:
   - PR 1: a BSE-only search;
   - PR 3: two stocks' fans side by side, now visibly different;
   - PR 4: news panel per source;
   - PR 5: a news-driven call on a stock with real material news that day.
4. **After merge:** wait for the Railway deploy SUCCESS, then run the production smoke. Extend `smoke_api.sh`: search filter, news `sources` (a failed source is tolerated if any succeeds).
5. Check one session in NSE hours (09:15–15:30 IST): live ticks, "likely close today", fresh news.
6. Update STATUS, ROADMAP, METHODOLOGY and API.md in each PR.

## Not in scope
- **Claude news reading:** follow-up issue; needs a key.
- **Storing news and testing it walk-forward (#107):** needs Postgres (#80).
- **Placing orders:** never. The app advises; the user decides.
