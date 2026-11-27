# Phase 0.9 (planned 2026-11-27, issues #138–#143): actions that mean something, numbers from data, Gemini reads the news

## Context
Owner feedback after 0.8 (27 Nov 2026):
1. **Use Gemini** (free-tier key supplied) to read the news.
2. **Remove the hardcoded numbers.**
3. **The advice is superficial and general.** "Everything has to pass, so no action today" is wrong. Verify this and refine the plan.

## What the research found (27 Nov 2026, real data)

**Gemini works.**
- Through the OpenAI-compatible endpoint, `gemini-3.1-flash-lite` answers in about 2 s per batch.
- `gemini-2.5-flash` is retired for new keys; `gemini-flash-latest` and `3.8-flash` returned 503 under load.
- On the headlines the keyword reader got wrong, it read correctly: "Infosys wins $1.5 billion deal" → order win +; the SEBI probe of the Tata Steel deal → legal −; the Jefferies tariff call → +.
- Error bodies come back as a JSON *array*; the client must handle that.
- **The key was pasted in chat, so rotate it once it's in Railway.**

**"No action today" was checked.**
- *Timing edge:* nothing works. Tested on 10 years of the NIFTY 50 as a panel (all 50 stocks together, about 105 monthly rebalances), not one stock at a time:

  | Rule (top 10 vs NIFTY 50 average, per month) | Mean | t |
  |---|---|---|
  | Momentum 12-1 | −0.04% | −0.2 |
  | Low volatility | −0.36% | −1.3 |
  | 1-week reversal | −0.20% | −0.6 |
  | 1-month reversal | +0.19% | +0.8 |
  | Near 52-week high | −0.22% | −0.8 |
  | 200-day trend, pooled | +0.23% | +1.6 (p = 0.12) |

  None is significant. So "no proven timing edge" is correct, and loosening the test would only produce false calls.
- *The framing is the real problem:* the app only asks "can we time it?" and never "should this money be invested at all versus the alternative?". NIFTY rose in 61% of months (+0.86% a month). Against a 7% FD:

  | | 1 year | 3 years | 5 years |
  |---|---|---|---|
  | NIFTY beat the FD (out of 100) | 60 | 88 | 97 |
  | ITC beat the FD (out of 100) | 51 | 50 | 85 |

  The 3- and 5-year windows overlap heavily (only about 2 separate 3-year periods in 10 years), and the app must say so. These are real, decision-relevant actions ("for 3+ years, an index SIP has usually beaten an FD; this stock less so") that the app never says.
- *Holders get no risk actions.* When today's volatility is high versus normal, a holder's safe position size changes today. The app already computes this (GARCH and expected shortfall) but never turns it into "reduce or keep".

**A data bug: one-day bad prices from Yahoo poison the analysis.**
- GOLDBEES on 19 Dec 2019 sits at 1/100 of its price, restored the next day; MON100 on 17 Jun 2021 sits at 1/10.
- The quality check flags them, but the analysis, risk figures and own-history Monte Carlo use them. Gold's fan replays a fake −99% day.

**Hardcoded numbers, by kind** (audit of every constant):

| Kind | Examples | Plan |
|---|---|---|
| Judgement weights | news materiality 0.25/0.5/1, half-life 48 h, "strong" 0.9, macro weight 0.5, lean 0.25 | **Estimate from data** (backfilled event study, below) |
| Market facts | 0.25% round-trip cost, FD rate (missing), LRS/TCS text, NIFTY 50 list | **Per-stock cost from data** (statutory charges + spread estimated from daily highs and lows, Corwin–Schultz 2012); FD rate as a setting with its source; LRS/TCS kept in one config file with source and date |
| Literature parameters | 200-day average, RSI 14 at 30/70, 12-1 momentum | **Keep, deliberately.** Choosing them from the same data is overfitting. Shown with citations. |
| Statistical conventions | 5% tests, Holm, 30 events, 5–95% bands, VaR 95/99 | Keep (standard), all documented in one place |
| Engineering | cache times, timeouts, parallelism | Keep |

**Past news is available.** Google News date-range search (`"HDFC Bank" after:2024-01-01 before:2024-01-31`) returns 10–30 headlines per company-month, about 3 years back. That lets the news weights be calibrated **now** instead of in 2–3 months.

## Owner decisions (27 Nov)
- **Call bar:** a Buy/Sell call needs P(edge beats costs) ≥ **80 in 100**; under that, the rule shows as a lean with its chance.
- **FD comparison:** editable, **default 7%** (cited typical 1–3 year big-bank FD).
- **News backfill:** **3 years** of NIFTY 50 headlines from Google News, read by Gemini on the free tier.
- **Order:**
  1. bad-price fix;
  2. **real actions**;
  3. Gemini;
  4. backfill;
  5. graded rules;
  6. constants register.

  The sections below follow that order (PR 4 runs second).

## Plan (one PR each, the usual gate, deploy and smoke)

### 1. `0.9-clean-prices` (#138): drop one-day bad prices before any maths (bug, first)
- In `gm-domain/src/quality.rs`, detect spike-and-revert bars: a robust |z| above the outlier threshold that reverses the next session to within the same tolerance. Remove them from the analysis series (not from the chart), and list them in the quality report as "bad prices removed".
- Used by `/analyze`, `/projection`, `/today`, compare and signals.
- Test on the real GOLDBEES and MON100 glitches (recorded fixture). Re-check the gold fan, VaR and Today numbers before and after.

### 2. `0.9-gemini` (#140): Gemini reads the news
- Railway secrets:
  - `GM_LLM_BASE_URL=https://generativelanguage.googleapis.com/v1beta/openai`;
  - `GM_LLM_MODEL=gemini-3.1-flash-lite`;
  - `GM_LLM_API_KEY` set with `--stdin`, never in the repo or logs.
- `gm-integrations/src/llm.rs`:
  - handle array-shaped error bodies;
  - one retry on 503 or 429 with back-off;
  - add `price_move` to the prompt's kinds;
  - batches of at most 25;
  - pacing within the free tier.
- The built-in reader stays as the fallback.
- **Macro desk via Gemini too:** a second prompt returns theme and asset effects (same JSON as `macro_news`), falling back to the keyword reader.
- Today's company news uses the same reader, so news-driven calls come from Gemini readings.

### 3. `0.9-news-backfill` (#141): calibrate news weights on 3 years now
- `gm-worker backfill-news` (the existing CLI crate) fetches Google News month by month for the NIFTY 50 names (about 1,800 queries, paced at roughly 1 every 3 s, so about 1.5 hours, run once from a local machine) and writes the same JSONL records as the recorder.
- Gemini reads them in batches over about 1–2 days of free quota. Both readings are kept.
- The file is uploaded to the Railway volume and merged with the live recording.
- `/api/v1/calibration` then has hundreds of events per kind.
- **Fit the weights:**
  - materiality weight = the average signed move per kind and direction;
  - half-life from how the move decays over 1, 2, 5 and 10 sessions;
  - "strong" threshold = the score above which past news-driven calls beat costs.
- Weights are written to `calibration.json` (dated, with n and SE) and read by the reader. A kind that is not significant gets weight 0, so it doesn't vote.
- The UI shows "weights fitted on N events, 2023–2026".

### 4. `0.9-real-actions` (#139): advice for the decision people actually face
For each instrument, three concrete answers, each with a chance:
- **Investing for the long term (3+ years), not owned:**
  - P(beat FD) over 1, 3 and 5 years from history, with a block bootstrap to account for the overlapping windows;
  - worst past outcome;
  - "Invest gradually (SIP)", "Lump sum is fine" or "Prefer an index fund to this single stock", by rule: single stocks are compared with NIFTY on the same chance.
- **Already own it:**
  - today's volatility versus normal, giving a size action: "keep under ₹X; that is lower than usual because volatility is 1.4× normal";
  - drawdown position;
  - concentration (from the mix).
- **Short-term trading:** a timing call only where a rule passes; otherwise "no timing edge: 50:50 after costs", which the panel test above backs.
- The FD rate is a setting (default from a cited source); the user can change it on the page.
- The Today page gets a "Your best moves today" list ranked by these chances, e.g.:
  - "Start or continue a NIFTY SIP: 88 in 100 to beat FD over 3 years (about 2 separate periods, so treat as rough)";
  - "Gold: volatility 1.6× normal, keep under ₹X".

### 5. `0.9-graded-rules` (#142): probabilities instead of pass/fail for timing rules
- Empirical-Bayes shrinkage: each rule's per-stock effect is pulled toward the NIFTY 50 pooled effect, weighted by precision. This gives the posterior P(edge > cost) per stock and rule.
- The call shows "Trend: 64 in 100 that following it beats costs here". A call needs P ≥ the action threshold (owner decides below); under it, the rule shows as a lean with its chance.
- Replaces the binary "earns its place" in the UI. The strict test stays in the maths drawer.

### 6. `0.9-constants` (#143): one register of every number
- `docs/CONSTANTS.md` lists each constant: value, kind, source or "fitted on <data>", and where it is used. A test fails if a new `const` in `gm-domain` isn't in the register.
- Market facts (LRS/TCS, the cost schedule, the FD default) go into one `config/market_facts.json` with source and date, served by the API, so the UI holds no copies.

## Verification
- `scripts/check.sh` on each PR, `--live` before deploy, and production smoke after.
- **Before/after numbers:** the gold ETF's vol, VaR and fan (bad bar removed); Today's chances; how many instruments get a long-term action versus "no timing edge".
- **Backfill report:** events per kind, fitted weights with SE; built-in reader versus Gemini hit rates on the same events.
- **Real-run screenshots** of the Today "best moves" list and the advice card's three answers.

## Not in scope
- Placing orders.
- Fundamentals and valuation (#85).
- Public launch: SEBI remains an owner decision.
