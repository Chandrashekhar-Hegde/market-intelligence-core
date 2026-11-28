# Phase 0.11: maths plus AI, Simons-style. Morning-to-close, 3-day and day-trade answers, honestly tested

## Context: what the owner asked for (28 Nov 2026, internal team only)
- "Everywhere it says Hold." Readable, plain English: the Monte Carlo is unreadable. "Predictions are wrong most of the time; have you tested it?"
- "Gemini is not in production."
- Diversification: pick your own stocks, and get a mix that changes with the money you have.
- No chart or pattern reading ("astrology"). Instead: hard maths (stochastic calculus, physics methods, quantum-style maths), combined with AI, studied the way Jim Simons ran Medallion.
- "If I buy in the morning, what do I have at the close?" (1-day). The same for 3 days, and for day trading.
- Research it properly, read the references, correct me where I'm wrong, plan first, then report back.

## Part 1: what I checked in our own system

**Gemini is live in production.**
- The Railway variables are set, and `/api/v1/desk` says `gemini-3.1-flash-lite (137 of 150 headlines…)`.
- The UI hides it: the Desk page never shows the reader.

**"Hold everywhere" is honest, but useless as presented.**
- Six timing rules were tried; on 49 of 50 stocks none clears the bar.
- What's missing is not courage but **the right questions and the right costs** (Parts 2 and 4).

**The predictions, tested by me today.**
- NIFTY 50 plus the index, 10 years, walk-forward, no peeking, 25,000+ forecasts.

| | 1 week | 1 month | 3 months |
|---|---|---|---|
| "9 in 10" range from the stock's own past stretches: held | **90.2%** | **88.9%** | **86.2%** |
| Up/down guess: right | 53.0% | 56.3% | 59.7% |
| … versus always saying "up" | 53.9% | 57.3% | 61.6% |

- The ranges are honest. The up/down "prediction" is a coin toss, so the owner's complaint is correct.
- Production had only ever checked one day ahead.

**Costs (corrected 28 Nov after the plan was approved: delivery stays 0.25%; stamp duty is 0.015% of ₹1 lakh = ₹15, not ₹150).** As first written: The app uses 0.25% for every round trip. Real Indian statutory charges, per ₹1 lakh bought and sold:
- **Intraday:** STT 0.025% (sell side only), stamp duty 0.003%, exchange fee about 0.003%. That is ≈ 0.06%, plus ₹40 brokerage and spread: **≈ 0.10–0.15% all in**.
- **Delivery (held overnight or longer):** STT 0.1% on each side, stamp duty 0.015%, plus DP charges. That is **≈ 0.36% plus spread: ≈ 0.40%+**.
- So day trades were charged too much in our tests, and 1–3 day trades too little. The day-trading verdict must be re-run.

**The mix is broken.**
- The picker hides every NIFTY 50 stock.
- The weights are the same for ₹10k and ₹10 lakh.
- No whole shares, no correlation.

## Part 2: Where you're right, and where I'd correct you

| You said | What the evidence says |
|---|---|
| Chart patterns are astrology | **Mostly agree for visual patterns.** But Simons' own signals *were* price and volume patterns. Mercer: "signals that make no intuitive sense do work". What made them science was strict statistics on clean data, not the source. We use price and volume as **numbers in models**, never pictures. |
| Use quantum physics | **Correct this one.** A 2026 review: quantum optimisation "has not demonstrated decisive empirical advantage over advanced classical heuristics". Quantum Monte Carlo speed-ups need hardware that doesn't exist yet. Quantum finance models restate classical ones. The physics that *does* work in markets: **random-matrix theory** (cleaning noise from correlations, Laloux et al. 1999); **statistical mechanics of price impact** (the square-root law, Bouchaud); **stochastic processes** (Brownian motion and Ornstein–Uhlenbeck, which come from physics); and **hidden Markov models** (from speech recognition, and the origin of Renaissance's methods via Baum). Those are the plan. |
| Stochastic calculus will predict | It **describes** risk (we already use GBM, GARCH and bootstraps). On its own it doesn't forecast direction. The edge comes from statistical signals; stochastic calculus sizes and prices them. Example: the OU model below turns "how far from normal" into an expected pull-back and its speed. |
| Do what Simons did | **Yes, but know the scale.** Medallion was right on about **50.75%** of trades (Mercer, in Zuckerman 2019). It made billions from millions of trades, near-zero costs, leverage and an average holding period of about 2 days, and it is capacity-capped at about $10bn with outsiders removed in 2005. Renaissance's *public* funds were ordinary (RIEF −22.6% in 2020, when Medallion made +76%). For us: a 51% edge on a single trade **does not pay** after 0.10–0.40% costs. Example: a stock that moves ±1.5% a day at a 51% hit rate earns ≈ 0.03% per trade, below even the intraday cost. Any 1-day or 3-day list must show **expected ₹ after costs**, and many days the honest answer is "nothing clears costs today". |
| Chart traders beat maths traders | There is no clean evidence for NSE. Indian studies of moving-average rules disagree (Sundhar & Kakani 2006 positive; Mitra 2011 mixed; Muruganandan 2020 negative). We won't assume either way; we test. |

## Part 3: Research read, and what we take from each

**How Renaissance worked (Zuckerman 2019, Simons' MIT talk 2010, Bloomberg 2019):**
- clean data first;
- many weak signals combined, never one rule;
- hidden Markov models and regressions;
- the transaction-cost model was the "secret weapon";
- short holding periods;
- strict significance;
- bet sizing.

**Maths models:**

| Model | Paper | What it answers | Horizon |
|---|---|---|---|
| Residual mean reversion: stock minus market and sector factors, modelled as an OU process, giving an **s-score** | Avellaneda & Lee 2010, *Quant. Finance* (Sharpe 1.44 after costs, 1997–2007) | "This stock is stretched versus its peers: expected pull-back X% over D days" | 1–5 days |
| Market intraday momentum: the first 30 minutes predict the last 30 | Gao, Han, Li & Zhou 2018, *JFE*; found in 12 of 16 developed markets; **never tested on NIFTY** | Day trading in the index (NIFTYBEES or NIFTY futures) | Intraday |
| Overnight versus intraday returns | Lou, Polk & Skouras 2019, *JFE* | "Buy at the open, sell at the close": which stocks earn their returns by day and which overnight | 1 day |
| Regime switching: a 2-state HMM (calm and stormy), fitted with Baum–Welch | Hamilton 1989; Nystrup, Madsen & Lindström 2015–19 (out-of-sample gains after costs); the Renaissance lineage | Probability we're in a stormy regime, then how much to hold, and for direction-neutral sizing | Days–weeks |
| Volatility from intraday data: HAR-RV | Corsi 2009 | Sharper ranges for today's close and the next 3 days | 1–5 days |
| Correlation cleaning: random-matrix theory, or Ledoit–Wolf shrinkage | Laloux, Cizeau, Bouchaud & Potters 1999; Bun, Bouchaud & Potters 2017 | A mix whose risk doesn't come out smaller on paper than it really is | Mix |
| Equal risk contribution | Maillard, Roncalli & Teïletche 2010 | How much of each picked stock to buy | Mix |
| Bet size: fractional Kelly | Kelly 1956; Thorp | How much to put on a signal, given its edge and its uncertainty | All |
| Price impact: the square-root law | Bouchaud, Farmer & Lillo | Cost of a larger order (for amounts above about ₹10 lakh) | All |

**AI models:**

| Use | Paper | How |
|---|---|---|
| LLM news score as a next-day signal | Lopez-Lira & Tang 2023–25 (out of sample; stronger after bad news, in smaller stocks, gross of costs); Chen, Kelly & Xiu 2023 (LLM embeddings, returns react slowly to news) | Gemini reads each headline (already running). The reading becomes **one feature** in the combined model, tested on our 3-year backfill. |
| Non-linear combination of many weak signals | Gu, Kelly & Xiu 2020, *RFS* (trees and neural nets beat linear; the main signals are momentum, liquidity and volatility) | A gradient-boosted or ridge-logistic **ensemble** across the NIFTY 50 panel, which is Simons' "many weak signals" |
| Plain-English brief | TradingAgents (Xiao et al. 2024): analysts, then risk, then manager; ai-hedge-fund (conviction plus thesis) | Gemini writes the daily brief **from computed facts only**; any number not in the facts is rejected |

**Guarding against fooling ourselves:**
- the **deflated Sharpe ratio** (Bailey & López de Prado 2014), which corrects for how many things we tried;
- the **probability of backtest overfitting** (combinatorially symmetric cross-validation, Bailey et al.);
- **purged and embargoed walk-forward** (López de Prado), so that windows which overlap don't leak.

These are added to the existing panel empirical-Bayes method (`panel_priors`, `edge_probability`) and the 80-in-100 bar.

## Part 4: Plan

### Track U: native Rust UI with GPUI (owner, 28 Nov: "render without any browser")
**What exists (checked today):**
- [`gpui` 0.2.2](https://docs.rs/crate/gpui/0.2.2) on crates.io: Zed's GPU UI. It runs on macOS (Metal), Linux (Wayland/X11) and Windows, and has a `runtime_shaders` feature.
- [`gpui-component` 0.11](https://github.com/longbridge/gpui-kit) (Apache-2.0, Longbridge): 60+ components, including the chart module (line, area, bar and **candlestick** OHLC), tables, resizable docks and inputs. It powers **Longbridge Pro**, a real trading desktop app. It pins its own GPUI snapshot (`gpui-base`/`gpui-pre =0.3.14`), so we use its GPUI, not crates.io `gpui`.
- **This Mac:** Command Line Tools only, so there is no `metal` shader compiler. GPUI's `runtime_shaders` feature compiles shaders at run time, which avoids needing full Xcode.

**U0 spike** (first, about half a day, kept in a branch until it passes). New crate `apps/desktop` (`gm-desktop`), which opens one window with:
1. a search box;
2. a live quote from `gm-api` (token from the keychain or an environment variable, never stored in the repo);
3. a `gpui-component` candlestick of 10 years of daily bars;
4. a text panel of the analysis.

The spike passes if all of these hold:
- it builds here with `runtime_shaders`;
- the window renders and I take a screenshot (`screencapture`);
- it starts in under 1 s;
- 2,500 candles pan smoothly;
- the release binary size is noted.

If it fails here (for example a shader or toolchain problem), the fallback is that the owner installs full Xcode. It is not a reason to drop GPUI.

**Architecture (laziest path that avoids writing things twice):**
- The desktop app is a thin client of the existing `gm-api` (same routes and token), and it deserialises into the **same `gm-domain` types**.
- The plain-English logic that lives in `apps/web/src/plainLanguage.ts` (verdicts, range rows, sizes, mix) **moves into Rust** (`gm-domain/src/advice_words.rs`, pure, unit-tested). Desktop and web then show identical words, and it is tested once.
- **Screens:**
  - **Today** (brief, picks, best moves);
  - **Instrument** (price now plus today's close range, candles, three answers, "Where ₹10,000 could be", "Show the maths");
  - **Mix** (pick stocks, amount, whole shares);
  - **News desk** (Gemini chip).
- These use `gpui-component` docks, so panels are resizable and movable, replacing `Modular.tsx`.
- **Live:** the existing SSE `/api/v1/stream` is read with reqwest's streaming body on GPUI's executor.

**Web app:** frozen once the desktop app reaches parity, then retired. Until then it gets bug fixes only, so no UI is built twice. Track A's UI items below are built **in GPUI**.

**Testing:**
- `gpui` `test-support` (`TestAppContext`) for headless view tests (state changes, the words shown);
- `advice_words.rs` unit tests;
- a real-run screenshot per screen;
- CI: `cargo check -p gm-desktop` and the view tests on macOS and Linux runners (with the Wayland/X11 development packages), added to `scripts/check.sh`.
- Distribution for the internal team: a macOS `.app` built with `cargo bundle`, unsigned for internal use; Linux and Windows builds if the team needs them.

### Track A: fix what's wrong now (no research needed; 2 PRs)
**A1 `0.11-plain-answers`**
- **No bare "Hold".** Each instrument gets one action:
  - a tested call; otherwise
  - "Keep it all" or "Trim to 70% of usual" (the track-record rule, capped at 100%); or
  - "Buy monthly (SIP)", "Buy the NIFTY index fund instead" (single stock), or "An FD has done as well".
- Every lean is stated as "N in 100 over D days, on M past cases".
- **The range comes from past stretches** (`validation.rs` `chance_check` gains `range_held`, `direction_right`, `range_now` and `n_stretches`, plus a behavioural test).
- **"Where ₹10,000 could be":**
  - three rows (1 week, 1 month, 3 months), each with a CSS range bar and "tested here: held N in 100";
  - "Up or down: can't be told";
  - a one-sentence "how it's worked out".
  - The fan chart, Monte Carlo and rule tables move into "Show the maths". `FanChart`, `useLiveProjection` and the duplicate card are deleted.
- **Gemini made visible:** a "Read by Gemini (137 of 150)" chip on the Desk and News panels.
- **Markets:** drop the TradingView module; "Private beta" becomes "Internal".
- **The cost model becomes two numbers:** intraday ≈ 0.12% and delivery ≈ 0.40% (statutory schedule plus spread), in `docs/CONSTANTS.md` with sources. All tests re-run with these costs.

**A2 `0.11-your-mix`**
- **Picker:** all 66 instruments.
- **Weights:** Ledoit–Wolf-shrunk covariance (RMT cleaning only once the universe exceeds about 100 names; at 66 names over 10 years the noise share is small). Equal risk contribution by fixed-point iteration.
- **Whole shares at the live price:**
  1. Floor each holding to whole shares.
  2. Spend the leftover greedily on the holding furthest under its target.
  3. Show the cash left.
  4. List any stock you can't buy one share of as "needs ₹X".
- **Shown:** shares, ₹ in each, the 1-in-20 bad day in ₹, and the comparison with all-NIFTY.

### Track B: research, then build only what passes (Simons' process)
**B1 Data and costs** (`scripts/research/`, then `gm-integrations`):
- **NSE bhavcopy** (official daily open, high, low and close from nsearchives, roadmap #45) for 10 years of the NIFTY 50. This fixes the unreliable Yahoo open, so "buy at open, sell at close" can be tested over 2,500 days instead of 47.
- Yahoo 5-minute bars (60 days) plus our own 1-minute archive (from 27 Nov) for the first-half-hour tests.
- Overnight cues already in the app: S&P 500, Nasdaq, crude oil and the rupee from the previous US session.

**B2 Feature library** (pure, in `gm-domain/src/analysis/features_quant.rs`):
- the s-score;
- overnight gap and its z-score;
- yesterday's and the last 5 days' residual return;
- HMM storm probability;
- HAR-RV forecast and volatility ratio;
- the first-30-minute return (intraday only);
- the US close and the rupee overnight;
- Gemini news score and its materiality (headlines up to 09:15 for the morning test).

**B3 One combined model per horizon**, so multiple testing is controlled once rather than per rule:
- **Targets:**
  - **1 day:** open-to-close (intraday cost);
  - **3 days:** open to the close 3 days later (delivery cost);
  - **day trade:** a 10:15 entry to the 15:15 exit on NIFTYBEES (intraday cost).
- **Methods:** ridge-logistic first (transparent), gradient boosting second; it has to beat the ridge out of sample to be kept.
- **Validation:** purged and embargoed walk-forward, deflated Sharpe, probability of backtest overfitting, panel empirical-Bayes and the 80-in-100 bar.
- **Output per stock per day:** P(profit after costs), the expected return after costs, and a range.
- Script `scripts/research/quant_ensemble.py`; findings into `METHODOLOGY.md` before any app code.

**B4 Gemini news as a feature:**
- Finish the backfill read: 3,790 of about 48,000 done, at night on the free tier.
- Test the Lopez-Lira–Tang next-day effect on our events (built-in reader versus Gemini on identical events), both on its own and inside B3.

**B5 Build only what passes:** `GET /api/v1/picks?horizon=1d|3d|intraday`, and a Today panel with three lists: **"Buy at open, sell at close"**, **"3-day"** and **"Day trade NIFTY"**. Each pick shows:
- P(profit after costs) "in 100";
- expected ₹ on ₹10,000 after costs;
- the range;
- a fractional-Kelly size;
- its tested record (hit rate, deflated Sharpe, number of days).

If nothing passes on a given day, the list says "Nothing clears costs today", with the best candidate's numbers.

**B6 Gemini daily brief** (`GET /api/v1/brief`, cached for 30 minutes):
- **Facts JSON:** actions, picks, flags (Gemini news with materiality ≥ 2, volatility ≥ 1.3× usual, a fall of 2 standard deviations or more today, a high storm probability) and the tested records.
- **Sections:** "Do now", "Invest in", "Flagged / going bad", "Get out now / get in now" (only from B5 picks or flags), and "1 day · 3 days · day trade".
- **Safeguards:** a number check rejects any figure not in the facts; a template is the fallback.

### Order and checkpoints
0. U0 GPUI spike: report with a screenshot. Then A1 and A2: the backend logic and words in Rust, the screens in GPUI.
1. A1, A2 (deployable within days).
2. B1–B3 research. **I report back with the result tables before building B5.**
3. B4 runs in parallel at night.
4. B5, B6.

Each PR gets `scripts/check.sh`, a deploy, production smoke and real-run screenshots.

## What I expect, so nobody is surprised
- From the literature, an edge after costs, if any, is most likely in:
  - **3-day residual reversal** (Avellaneda–Lee);
  - **index intraday momentum** (Gao et al.);
  - **news after bad headlines** (Lopez-Lira & Tang).
- Single-stock "buy at open, sell at close" has the weakest prior.
- An edge for a retail Indian investor will be small (tenths of a percent), not on every day, and it needs many trades to show.
- The app will say exactly that, with numbers. It will not invent picks.

## Not doing
- Visual chart patterns.
- Quantum computing.
- Live orders.
- Linking AGPL code (OpenAlgo stays a separate service if brokers are added later).
- A paid Gemini tier.

## Verification
- **Track A:** gate, smoke (`range_held`, desk reader is Gemini, mix tests at ₹10k versus ₹10 lakh), screenshots.
- **Track B:**
  - each study reproducible from `scripts/research/` with fixed seeds;
  - result tables (hit rate, return after cost, deflated Sharpe, probability of overfitting) shown to the owner before B5;
  - after launch, the app logs every pick and scores it against what happened, as a live track record.
- Not checkable until the next NSE session (today is Saturday): live intraday features and the close range.

## References
- GPUI ([docs.rs](https://docs.rs/crate/gpui/0.2.2)); gpui-component / GPUI Kit ([repo](https://github.com/longbridge/gpui-kit), [chart docs](https://longbridge.github.io/gpui-component/docs/components/chart))
- Zuckerman, *The Man Who Solved the Market* (2019)
- Simons, MIT talk "Mathematics, Common Sense and Good Luck" (2010)
- Bloomberg, "Jim Simons Reveals Clues to Medallion Fund's Unrivaled Run" (2019)
- Avellaneda & Lee, *Quant. Finance* 10(7) (2010)
- Gao, Han, Li & Zhou, *JFE* 129(2) (2018)
- Lou, Polk & Skouras, *JFE* 134 (2019)
- Hamilton, *Econometrica* (1989)
- Nystrup, Madsen & Lindström, *Quant. Finance* (2017)
- Corsi, *J. Fin. Econometrics* (2009)
- Laloux, Cizeau, Bouchaud & Potters, *PRL* 83 (1999)
- Bun, Bouchaud & Potters, *Phys. Rep.* (2017)
- Maillard, Roncalli & Teïletche (2010)
- Ledoit & Wolf (2004)
- Kelly (1956); Thorp
- Bouchaud, Farmer & Lillo, "How markets slowly digest changes in supply and demand"
- Lopez-Lira & Tang, arXiv 2304.07619
- Chen, Kelly & Xiu, "Expected Returns and Large Language Models" (2023)
- Gu, Kelly & Xiu, *RFS* 33(5) (2020)
- Bailey & López de Prado, "The Deflated Sharpe Ratio" (2014)
- Bailey, Borwein, López de Prado & Zhu, "The Probability of Backtest Overfitting"
- Xiao et al., TradingAgents (arXiv 2412.20138)
- Quantum finance review, arXiv 2604.08180 (2026)
- Indian cost schedule: Zerodha STT page and similar
- Sundhar & Kakani (2006), Mitra (2011), Muruganandan (2020)
