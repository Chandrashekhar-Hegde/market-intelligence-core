# Phase 0.10 (28 Nov 2026): does the advice pay, at every holding period?

## Context
The owner asked:
- **Track record:** on real history, would the app's advice have made money **and** saved money,
  including through volatile dates?
- **Short holding periods:** does it work for 1, 2 and 3 days, 1 and 2 weeks, 1 month, and for
  day trading?
- **Close range:** the live view should say where today's close is likely to land.

The owner asked for decisions without more questions, each backed by evidence. The research
scripts are in `scripts/research/`.

## What the evidence said (NIFTY 50, 10 years of daily bars; 60 days of 5-minute bars)

### 1. The app's holder rule
"Hold less when it moves more than usual" (volatility-managed exposure, never above fully
invested; weekly rebalance, costs, cash at the deposit rate):
- **NIFTY 50 index:** worst fall −23% vs −38% just holding, and better return per unit of risk
  (Sharpe 0.22 vs 0.20).
  - Covid crash: −21% vs −37%; all of 2020 a little better.
  - Growth 10.2% vs 10.7% a year.
  - One-day shocks (election day 2024, the April 2025 tariff shock) aren't avoided.
- **Single stocks:** smaller worst fall in 44 of 49, better Sharpe in 22, more money in 10.
  This is insurance, not extra return, consistent with Moreira & Muir (2017) and Liu, Tang &
  Zhou (2019).
- **Decision:** show this replay for every instrument ("Has this advice paid off here?") with an
  honest verdict, and size the holder's advice with exactly the replayed measure.

### 2. Short holding periods (1, 2, 3, 5, 10, 21 days)
- Rules tested: falls of a day or a week, RSI < 30, trend, 12-month winners, the week's losers.
- **Panel result, delivery costs:** none beat buying the same stocks on a random day.
- **Decision:** the engine now tests the rules at 1, 2, 3 and 10 days too (42 tests per
  instrument, graded with the panel as in 0.9). On the NIFTY 50 this adds no passing rule. The
  short-term answer says so with this evidence.

### 3. Day trading
- **On 10 years of daily bars:** gap fades looked profitable (+0.2% a day after 0.06% costs,
  t ≈ 4). But Yahoo's daily open differs from the first 5-minute open by a median 0.13%, as large
  as the effect.
- **On 5-minute bars, entering at 09:20:**
  - buying a gap down: +0.32% a day, t 1.9, 47 days;
  - shorting a gap up: −0.30%;
  - intraday momentum (Gao et al. 2018) failed (−0.05%, t −3.4).
- **Decision:** no day-trading call. The app keeps every trading day's 1-minute bars for the
  NIFTY 50 from now on, so the gap-down pattern can be tested on enough days.

### 4. Today's close range
- **The old range** (daily quantiles × √time left) held 94% of closes: too wide midday, too
  narrow at 15:00.
- **Fitted replacement:** open-to-close volatility × √(share of the day's variance still to
  come), using the U-shaped NSE profile (the first and last half hours carry a third of it),
  with a symmetric band per half hour. Fitted on 30 days, it held 88.8% on the next 30.
- **Decision:** use it, and say how often it held.

## Shipped in one PR (`0.10-track-record`)
- `analysis/track_record.rs` and `report.track_record`, shown in the advice card.
- Holder sizing switches to the EWMA ÷ usual-volatility ratio (the replayed measure).
- `signals::HORIZONS` = 1, 2, 3, 5, 10, 21, 63; references regenerated.
- The fitted close range (`closeRangeToday`).
- The trader answer states the short-term and day-trading evidence.
- The recorder keeps 1-minute bars daily.
- `scripts/research/` holds the studies.
