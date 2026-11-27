# Methodology

Every number in `GET /api/v1/analyze` and the Markets analysis panels comes from
`crates/gm-domain/src/analysis/`. This page explains how each one is computed and gives its source.
The engine is pure: prices go in, a report comes out. It never touches the network or the clock,
and its randomness is seeded.

**Verification.** `scripts/gen_reference.py` recomputes every statistic with scipy, statsmodels
and arch on 5 years of real daily data: AAPL, the S&P 500 and NIFTY 50
(`crates/gm-domain/tests/fixtures/`). The Rust tests compare against those outputs. Most values
match to 1e-9 relative error. The GARCH log-likelihood matches to 1e-6, and the GARCH parameters
to within 1–3%, because the two optimizers stop at slightly different points on a flat surface.

## Data

- **Prices.** Daily adjusted closes from Yahoo Finance. "Adjusted" means split- and
  dividend-adjusted, so returns across ex-dates are total returns. A session that is still
  trading is excluded from the history.
- **Returns.** Log returns, rₜ = ln(Pₜ / Pₜ₋₁).
- **Annualisation.** Uses the series' own frequency: periods per year = number of returns ÷
  years spanned. That comes out at about 245–255 for exchanges and 365 for crypto, so no market
  is forced onto a 252-day calendar.
- **Minimum length.** 60 returns. GARCH needs at least 250.

## Distribution

| Quantity | Definition | Source |
|---|---|---|
| Volatility | Sample standard deviation (n − 1) × √(periods per year) | |
| Drift standard error | σ / √n × periods per year. It is shown next to the mean because drift is estimated far less precisely than volatility | |
| Skewness, excess kurtosis | Population central moments: m₃ / m₂^{3/2} and m₄ / m₂² − 3 | as `scipy.stats` |
| Jarque–Bera | n/6 · (S² + K²/4), compared with χ²(2) | Jarque & Bera (1980) |
| Hill tail index | 1 / mean ln(xᵢ / x₍ₖ₊₁₎) over the k largest losses, with k = 5% of n (minimum 10). Lower means a fatter tail | Hill (1975) |

## Predictability

| Test | Definition | Source |
|---|---|---|
| Ljung–Box | Q = n(n+2) Σₖ ρₖ²/(n−k), k = 1..10, compared with χ²(10). Run on r (autocorrelation) and r² (volatility clustering) | Ljung & Box (1978) |
| Variance ratio | Overlapping, bias-corrected VR(q) for q = 2, 5, 10, with a heteroskedasticity-robust z*(q) | Lo & MacKinlay (1988) |
| ADF | t-statistic of γ in Δyₜ = a + γyₜ₋₁ + Σδᵢ Δyₜ₋ᵢ + eₜ, on log price. Lag order is ⌊12·(n/100)^¼⌋. Critical values are finite-sample | Dickey & Fuller (1979); Schwert (1989); MacKinnon (2010) |

When no test rejects at 5%, the report says there is **no evidence of predictability**, and the
forecast is presented as a range of risk, not a direction. For most liquid stocks this is the
honest finding (Fama 1970).

## Volatility

- **EWMA.** σ²ₜ = λσ²ₜ₋₁ + (1−λ)(rₜ₋₁ − r̄)², with λ = 0.94 (J.P. Morgan RiskMetrics 1996). It is
  seeded with the sample variance.
- **GARCH(1,1) with Student-t errors.** σ²ₜ = ω + αe²ₜ₋₁ + βσ²ₜ₋₁, where eₜ = σₜzₜ and z follows
  a standardised t(ν) (Bollerslev 1986, 1987).
  - **Fit.** Maximum likelihood on demeaned returns scaled ×100. The optimizer is Nelder–Mead
    (Nelder & Mead 1965) with two restarts.
  - **Constraints.** ω > 0, α, β ≥ 0, α + β < 1 and ν > 2, enforced by reparameterisation. The
    starting variance uses arch's backcast.
  - **Reported.** Persistence α+β, half-life ln½ / ln(α+β), and long-run volatility
    √(ω/(1−α−β)). Also the term structure: E[σ²ₜ₊ᵢ] = V_L + (α+β)^{i−1}(σ²ₜ₊₁ − V_L), averaged
    over 1, 5, 21 and 63 sessions.

## Forward distribution (Monte Carlo)

- **Paths.** 2,000 by default (range 100–20,000), using a ChaCha8 generator.
  - **Seed:** `seed` XOR a hash of the model name XOR FNV-1a(symbol). The same inputs, symbol and
    seed always give byte-identical output, but two instruments never share random draws.
  - **Why the symbol is in the seed:** until 27 Nov 2026 every symbol used the same draws, so
    every GBM fan had the same shape, only stretched by σ, and looked fake.
- **Which model leads (`lead_model`, `lead_reason`).** Chosen by validation, not fixed:
  GARCH-t leads when it *earns its place* (below); otherwise the own-history bootstrap, which is
  what the historical-simulation baseline does. GBM is never validated and leads only when the
  history is too short for the others.
- **Own history (`bootstrap`).** A stationary bootstrap (Politis & Romano
  1994) of the instrument's own daily log returns.
  - **How it runs:** each path starts on a random past day. Each next day continues the same run of
    history, with probability 1 − 1/*L*, or jumps to a new random day.
  - ***L*:** the Politis & White (2004) automatic block length with the Patton, Politis & White
    (2009) correction, computed on **squared** returns. Volatility clustering is the dependence
    that matters for risk, and plain returns barely show it.
  - **Check:** *L* matches `arch.bootstrap.optimal_block_length` to 1e-9 on the AAPL, S&P 500 and
    NIFTY fixtures (17, 27 and 8 days).
  - **Effect:** the stock's own crash spells and calm periods shape its fan. It is also the
    historical-simulation family that every model is scored against in validation.
- **Sample paths.** The first 30 simulated paths of each model are returned and drawn faintly
  behind the bands, so the fan is visibly made of possible futures.
- **Typical path (`typical_path`).** Of all simulated paths, the one with the least total
  distance Σ<sub>d</sub>|log P<sub>d</sub> − median<sub>d</sub>| from the day-by-day median (the
  medoid under that distance), drawn in orange. A random walk has no single "most likely
  path"; this is the most ordinary of the simulated ones, and the chart says so.
- **GBM.** Log returns are μ + σZ, with Z ~ N(0,1) and μ, σ the historical daily mean and
  standard deviation.
- **GARCH-t.** rₜ = μ + σₜzₜ, with σ² updated by the fitted GARCH recursion from today's
  conditional variance.
- **Output.**
  - Price quantiles (5, 25, 50, 75, 95%) for every session up to 63.
  - At 5, 21 and 63 sessions: return quantiles, P(gain), P(loss ≥ 10%), and P(drawdown ≥ 10%
    at any point along the path).
- **Live fan (`/api/v1/projection`).** During a session the web recomputes every projection from
  the live price, at most every 5 s. The 10-year daily fit (returns, GARCH) is cached until the
  next daily close. The paths start at the live price. GARCH variance takes today's move so far
  as the shock of the session in progress, σ² = ω + α·e² + β·σ²<sub>next</sub> with
  e = ln(live / last close) − μ, so a sharp morning move widens the GARCH fan at once. The
  bootstrap and GBM fans keep their shape in % terms and move with the price. Approximation:
  the partial-day move counts as a full day's shock, and the session in progress counts as the
  first forecast day. Measured: about 0.8 s for the first fit, then about 17 ms per update
  (local, real data).
- **Caveat.** The drift μ is the historical average, and its standard error is printed next to
  it. The width of the fan is the reliable part; where the centre sits is not.

## Out-of-sample validation

Does any model forecast better than a simple baseline, on days it never saw? This is where models
earn their place. Validation runs when there are at least 600 daily returns.

- **Walk-forward.** The first 500 days are warm-up. After that, every day's 1-day return
  distribution is forecast from earlier data only.
- **Models.**
  - *Historical simulation* (the baseline): empirical quantiles of the last 500 returns.
  - *EWMA-normal*: RiskMetrics, zero mean, λ = 0.94.
  - *GARCH-t*: re-estimated every 63 days on all data so far, and filtered daily in between.
- **Pinball loss.** Mean of (p − 1{y < q})(y − q) over the 1, 5, 25, 50, 75, 95 and 99%
  quantiles. This is a proper scoring rule for quantile forecasts (Gneiting & Raftery 2007).
- **Diebold–Mariano.** Tests the daily loss difference against the baseline, using a Newey–West
  variance (Bartlett lag ⌊4(T/100)^{2/9}⌋) (Diebold & Mariano 1995).
- **VaR backtests at 99% and 95%.** Kupiec's proportion-of-failures LR test (Kupiec 1995) and
  Christoffersen's independence LR test (Christoffersen 1998), each against χ²(1). A level
  passes when neither test rejects at 5%.
- **Interval coverage.** The share of outcomes inside the forecast's central 90% interval.
- **Direction.** The sign of the forecast median against the sign of the outcome, with an exact
  two-sided binomial test against 50%. It is undefined for zero-median models.
- **Verdict.** A model *earns its place* only if its loss is significantly below the baseline's
  (DM, 5%) **and** both VaR levels pass. Being sharper but failing a VaR backtest is reported
  separately, as "don't use its tail numbers".

- **Is the chance of a gain honest? (`chance_of_gain`).** At 5, 21 and 63 sessions, walk
  forward over non-overlapping windows from day 500. At each start, the forecast is the share of
  all earlier windows of that length that gained (what replaying the stock's own history says,
  which is mostly its past average growth). The outcome is whether this window gained. Scored by
  the Brier (1950) score against a coin flip's 0.25, with skill = 1 − Brier/0.25 and a DM test on
  the per-window difference. The UI says "its history said about X in 100, it actually rose Y in
  100", and whether that beats a coin flip. On simulated data with real drift the check finds
  skill; with none, it does not (`chance_check_rewards_real_drift_only`).

Verification: Kupiec, Christoffersen, DM and the binomial p-values match numpy/scipy computations
on fixed sequences. On simulated GARCH-t data, the harness rates GARCH-t as earning its place,
with 90% coverage within 2 points.

## Risk

- **One-day VaR and expected shortfall** at 95% and 99%, shown as positive losses. Losses are
  simple returns, 1 − e^r.
  - **Historical.** Empirical quantile with linear interpolation (numpy's default); ES is the mean
    loss beyond it.
  - **Student t (unconditional).** ν comes from the method of moments, ν = 4 + 6/excess kurtosis.
    ES uses E[T | T < q] = −f(q)/α · (ν+q²)/(ν−1) (McNeil, Frey & Embrechts 2005). Above ν = 200
    the normal distribution is used instead.
  - **GARCH-t (conditional).** The same formula with today's GARCH σ and the fitted ν.
- **Maximum drawdown.** Peak, trough and recovery dates, plus how far the price is below its peak
  today.
- **Sharpe ratio.** Uses a zero risk-free rate, and says so. The standard error is Lo (2002)'s iid
  formula, √((1 + ½SR²)/n), annualised. Sortino uses downside deviation.

## Benchmark exposure

- **Benchmark choice.** An equity is measured against its home-market index, looked up by listing
  suffix. There are 34 exchanges in `crates/gm-integrations/src/markets.rs`, for example
  `.NS`→NIFTY 50, `.T`→Nikkei 225, `.DE`→DAX and `.L`→FTSE 100. Other choices:
  - US listings (no suffix) use the S&P 500.
  - Unknown exchanges use MSCI ACWI, the global market.
  - Indices, FX, futures and crypto are the market themselves, so no benchmark is used.
  - The caller can pass `benchmark=none` or any symbol.
- **Regression.** OLS of the asset's returns on the benchmark's, using common dates only. It
  reports beta and alpha (Jensen 1968) with their standard errors, R², correlation and
  stock-specific volatility.
- **Currency.** When the asset and the benchmark trade in different currencies, the response says
  that beta includes currency moves.

## Comparing instruments (`/api/v1/compare`)

Compares 2–10 instruments on the trading days they all share (at least 120). Every value is
checked against pandas, scikit-learn, numpy and statsmodels on the AAPL, S&P 500 and NIFTY
fixtures.

- **Correlation.** Pearson on daily log returns, Spearman (rank) for robustness to outliers, and
  Pearson on weekly returns (the last common close of each ISO week). Markets in different time
  zones close hours apart, which biases daily correlation down (asynchronous trading, Scholes &
  Williams 1977). For them the UI shows weekly correlation first.
- **Rolling correlation** over a 63-day window, sampled weekly.
- **Covariance.** Ledoit–Wolf (2004) shrinkage toward μ·I, exactly as scikit-learn's
  `ledoit_wolf`.
- **PCA.** Eigenvalues of the correlation matrix (cyclic Jacobi) give the share of variance
  explained by each factor. The first factor is usually "the market".
- **Cointegration.**
  - Engle & Granger (1987): OLS of one log price on the other, then ADF on the residual spread
    (no constant, lag ⌊12·(n/100)^¼⌋). Critical values are MacKinnon (2010) τ_c for two variables,
    at statsmodels `coint`'s sample size of n − 1.
  - Both orderings are tested and the more significant is reported, with its hedge ratio.
  - The spread's OU half-life comes from Δsₜ = a + φsₜ₋₁ (−ln 2 / ln(1+φ)), and its z-score today
    is also reported.
  - **Multiple testing.** k instruments give k(k−1)/2 tests, so about 5% of them pass by chance.
    The findings say how many to expect.
- **Performance.** Total and annual return, volatility, Sharpe and maximum drawdown on the common
  period, each in its own currency. When currencies differ the response says so.

**Plain words on the Compare tab** (`plainLanguage.ts`).
- **Moving together:** a correlation is described with the Evans (1996) verbal scale on |ρ|:
  - ≥ 0.8 very closely; ≥ 0.6 closely; ≥ 0.4 somewhat; ≥ 0.2 loosely;
  - below 0.2 independently;
  - with "in opposite directions" when ρ < 0.
  - Weekly correlations are used when the markets close at different times.
- **Mixing them:** the equal mix's yearly swing is √(wᵀΣw), with w = 1/n and Σ the annualised
  covariance. It is compared with the average of each instrument's own swing.

## Data quality (`/api/v1/bars` and `/analyze`)

Every series is checked for:
- impossible prices (OHLC inconsistency, non-positive values);
- gaps against the home-market index's trading days;
- staleness;
- extreme returns. A robust z above 8 (MAD-based) is flagged, and flagged moves within 3 days of a
  split or dividend are marked as such.

## Signals and advice (`signals`, `advice` in `/api/v1/analyze`)

Buy/Hold/Sell appears only when a rule has worked **on this instrument**, out of sample. Code:
`crates/gm-domain/src/analysis/signals.rs`.

**Data.** Up to 10 years of daily split- and dividend-adjusted closes and volumes (a separate,
cached fetch), because the 3-month horizon needs 30+ separate calls.

**Rules.** Published parameters, never tuned here. Each gives +1 (buy), −1 (sell / stay out) or 0
(no view) at the close of day *t*, from data up to *t* only.

| Rule | Position | Source |
|---|---|---|
| `trend_200d` | +1 if close > 200-day simple average, else −1 | Faber (2007); Moskowitz, Ooi & Pedersen (2012) |
| `momentum_12_1` | sign of ln(P<sub>t−21</sub>/P<sub>t−252</sub>) | Jegadeesh & Titman (1993); Moskowitz et al. (2012) |
| `reversal_5d` | +1 if the 5-day log return is in the bottom 10% of the previous 252 days' 5-day returns, −1 if in the top 10% | Lehmann (1990); Jegadeesh (1990) |
| `rsi_14` | +1 if Wilder's 14-day RSI < 30, −1 if > 70 | Wilder (1978) |
| `volume_spike` | +1 if volume is in the top 10% of the previous 252 days | Gervais, Kaniel & Mingelgrin (2001) |
| `pressure_obv` | +1 if on-balance volume is above its value 20 sessions ago, else −1 | Granville (1963) |

"Top/bottom 10% of the previous year" is calibrated on the instrument's own trailing 252 days.
Volume rules run only when at least 90% of days report volume (indices and FX usually don't).

**Test.** For each rule and horizon *h* ∈ {5, 21, 63} sessions:
1. Regress the *h*-day forward log return on the position: *r*<sub>t→t+h</sub> = *a* + *b*·pos<sub>t</sub>.
   The slope *b* is the timing effect.
2. Forward returns overlap, so the error is MA(*h*−1). Standard errors use equal weights on lags
   1…*h*−1 (Hansen & Hodrick 1980), with Newey–West (1987) as the fallback if that sum is not
   positive. Bartlett (Newey–West) weights understated the variance here: on simulated random walks
   they gave 6.5% false "proven" rules.
3. One-sided p-value (H1: *b* > 0) from Student *t* with *n*<sub>ind</sub> − 1 degrees of freedom,
   where *n*<sub>ind</sub> = (days with a non-zero position) / *h*.
4. Holm (1979) step-down correction across every rule × horizon tested on the instrument (18 with
   volume, 12 without).
5. Hit rate on non-overlapping calls (every *h*-th active day), with a Wilson (1927) 95% interval.

**A rule earns its place** only if all four hold:
- Holm-adjusted p < 0.05;
- *b* ≥ 0.25%, the round-trip cost of an Indian delivery trade: STT 0.1% each side (Finance (No. 2)
  Act 2004, as amended), stamp duty 0.015% on the buy (Indian Stamp Act, 2020 amendment), NSE
  transaction and SEBI fees, GST on fees, and a spread allowance, rounded up;
- *b* > 0 in both halves of the sample;
- *n*<sub>ind</sub> ≥ 30.

**Advice.**
- **Buy:** a proven rule says +1 today and none says −1.
- **Sell / stay out:** the reverse.
- **Hold:** proven rules disagree (`conflict`), none is active today (`proven_but_quiet`), or none
  is proven (`no_evidence`).

The leading rule is the one with the smallest adjusted p. It sets:
- the horizon, and a review date *h* sessions ahead (weekdays, or calendar days when the history
  has weekend dates);
- the confidence: its hit rate, Wilson interval and number of calls;
- the flip level, if the rule has one (the 200-day average).

**Checks.**
- Positions, RSI, OBV and every test statistic match pandas/statsmodels (HAC, uniform kernel) on 5
  years of AAPL to 1e-8.
- A planted ±0.4%/day year-long trend makes the trend rule earn its place.
- 200 seeded fat-tailed random walks with random volume produce a proven rule in ≤ 5% of cases
  (measured: 1.5%).

**Measured on real data (26 Nov 2026):** 37 instruments were tested. They were 30 large NSE stocks,
NIFTY 50, Reliance, TCS, HDFC Bank, Infosys, AAPL, Bitcoin and the gold ETF. One rule earned its
place on one stock: Power Grid, RSI over a week. Every other instrument got "Hold: no proven edge".
That is the expected result for liquid markets, and it is why calls are rare.

## Plain-language cards and live ranges (web app)

These are computed in `apps/web/src/plainLanguage.ts`, from API numbers only.
- **Typical day:** σ<sub>daily</sub> = annual volatility / √(sessions per year), times the amount.
- **Compared with the market:** β = ρ·σ<sub>stock</sub>/σ<sub>market</sub>, so
  σ<sub>stock</sub>/σ<sub>market</sub> = β/ρ exactly. Shown only when β and ρ are positive.
- **Bad day:** historical 95% one-day VaR and ES, times the amount.
- **How much to put in:** maximum amount = the user's bad-day loss limit / 95% ES. The average loss
  on the worst 1 day in 20 then equals the limit.
- **Chances as natural frequencies.** Every chance is written "64 in 100", or "1 in 12" below
  9.5%, because people reason about frequencies far more accurately than about percentages
  (Gigerenzer & Hoffrage 1995).
- **Where it could be:** the 5% and 95% return quantiles of the lead projection at 5, 21 and 63
  sessions, with the share of paths that gain. **The chances** card adds the chance of ending
  more than 10% lower and of a 10% fall along the way, followed by the honesty check above.
- **Likely close today / tomorrow:** the 5% and 95% one-session log-return quantiles of the same
  projection, scaled by √(sessions left) and applied to the live price. For today, "sessions left"
  is the share of the regular session still to trade. This square-root-of-time rule ignores
  intraday seasonality, and the app uses it only for a day or two ahead.
- **Relative volume:** today's volume / (20-day average daily volume × share of session elapsed).
  This assumes volume accrues evenly. Real intraday volume is U-shaped, so the figure overstates
  activity early and understates it midday.

## News (`/api/v1/news`)

**Sources.** All are matched by the instrument's name, with legal suffixes such as "Limited" or
"Inc." dropped, so some stories are off-topic.
- Google News RSS search, India edition, last 3 days.
- The Economic Times and Mint markets RSS feeds: items whose title or summary contains the name.
- GDELT DOC 2.0 English articles.

Stories are de-duplicated by their first 60 alphanumeric characters of title.

**Reaction to a story.** Code: `crates/gm-domain/src/analysis/news.rs`.
1. Take the close of the last 5-minute bar that had finished when GDELT first saw the story
   (`seen_at`), and the latest close.
   - instrument move *r* = ln(now / before);
   - market move *r*<sub>m</sub>, the same window for the home index;
   - abnormal move = *r* − β·*r*<sub>m</sub>, with β from a year of daily returns.
2. Express time since the story in sessions: bars after it × bar length / session length.
3. *z* = abnormal / (σ·√sessions). σ is the daily volatility the market does not explain
   (idiosyncratic σ from the same regression), or total daily volatility when there is no index.
4. "About 1 in *N* without news": *N* = 1 / (2(1 − Φ(|*z*|))). Below |*z*| = 1.96 the UI calls the
   move ordinary.

This is the market-model abnormal return (MacKinlay 1997), applied intraday.

**How news enters the advice:** see "News reading" (Phase 0.7, #114).

## News reading and the news vote (`analysis/news_reader.rs`)

**Reading a headline.**
- **Built-in reader** (default, free, instant): hand-written event patterns in priority order:
  - legal and regulatory, with relief wording such as "dismisses", "quashes" or "probe dropped"
    flipping it to good news;
  - court matters, debt and insolvency, credit ratings;
  - results (beat/miss or up/down wording), guidance, broker ratings, order wins, management exits;
  - stake sales and purchases, payouts, fund-raising, deals, approvals.
  - Anything else falls back to a small finance word list. The list is our own, not Loughran–McDonald:
    that list's licence for redistribution is unclear.
- **Each reading** has a kind, a direction (good, bad or unclear for the company), a materiality
  (major, notable or minor) and a one-line reason.
- **Optional model:** any OpenAI-compatible chat endpoint (`GM_LLM_BASE_URL`, `GM_LLM_MODEL`,
  `GM_LLM_API_KEY`), e.g. a free Groq, Gemini or OpenRouter tier or a local Ollama.
  - It returns the same fields.
  - Each headline is read once and cached.
  - Any failure falls back to the built-in reader.

**What doesn't vote.** Two kinds are shown to the user but never count:
- **price reports** ("surges 3% to day's high", "52-week high", "top gainers"), which describe a
  move that already happened;
- **general wording** without a clear event.

**The vote.** Score = Σ direction × weight × 0.5^(hours since the story / 48). Within each event
kind, the stories together count for no more than the strongest single one, because one event is
covered by many outlets.
- **Measured on 27 Nov 2026** (10 large NSE stocks): one news-driven call, a Sell on HDFC Bank. The
  driver was a court order about prosecution sanction in a fraud case, where the bank is probably
  not the accused. Keyword reading can't tell, which is why a model can be plugged in.
- **Weights:** major 1.0, notable 0.5, minor 0.25.
- **Strong news** means |score| ≥ 0.9 with at least one notable or major story pointing that way. One
  fresh major story qualifies; minor chatter alone never does.
- **These constants are judgement, not estimates.** They are uncalibrated (owner decision, 27 Nov
  2026, to let news vote before it can be tested) and are to be calibrated by #107 once news
  history is stored (#80).

**Only for companies.** News votes only for single company stocks (`EQUITY`). For an index,
ETF, fund, currency or crypto, a name search returns market commentary, which the reader isn't
built for. On 27 Nov 2026 NIFTY 50 got a news-driven Buy from a story about exchange operators'
shares.

**Combining with the trading rules** (`signals::advise_with_news`):
| Situation | Call |
|---|---|
| A proven rule is active and agrees with strong news | The rule's call, noting the news agrees |
| A proven rule is active, strong news points the other way | Hold (`news_conflict`) |
| No proven rule is active, strong news | Buy or Sell (`news_driven`) with a 5-session review, labelled "no track record yet", headlines shown |
| Weak or no news | The rules' advice unchanged |

## References

Bollerslev (1986) J. Econometrics 31; Bollerslev (1987) REStat 69; Dickey & Fuller (1979) JASA 74;
Fama (1970) J. Finance 25; Hill (1975) Ann. Statist. 3; Jarque & Bera (1980) Econ. Letters 6;
Engle & Granger (1987) Econometrica 55; Jensen (1968) J. Finance 23; Ledoit & Wolf (2004) J.
Multivariate Analysis 88; Scholes & Williams (1977) J. Financial Economics 5; J.P. Morgan/Reuters (1996) RiskMetrics Technical Document; Ljung & Box
(1978) Biometrika 65; Lo (2002) FAJ 58; Lo & MacKinlay (1988) RFS 1; MacKinnon (2010) Queen's
Economics WP 1227; McNeil, Frey & Embrechts (2005) Quantitative Risk Management; Nelder & Mead (1965)
Computer J. 7; Schwert (1989) JBES 7; Christoffersen (1998) IER 39; Diebold & Mariano (1995)
JBES 13; Gneiting & Raftery (2007) JASA 102; Kupiec (1995) J. Derivatives 3; Faber (2007) J. Wealth Management 9(4); Gervais, Kaniel & Mingelgrin (2001) J. Finance 56;
Granville (1963) New Key to Stock Market Profits; Hansen & Hodrick (1980) JPE 88; Holm (1979) Scand.
J. Statistics 6; Jegadeesh (1990) J. Finance 45; Jegadeesh & Titman (1993) J. Finance 48; Lehmann
(1990) QJE 105; Moskowitz, Ooi & Pedersen (2012) JFE 104; Newey & West (1987) Econometrica 55;
Wilder (1978) New Concepts in Technical Trading Systems; Wilson (1927) JASA 22. MacKinlay (1997) J. Economic Literature 35. Evans (1996) Straightforward Statistics for the Behavioral Sciences. Brier (1950) Monthly Weather Review 78; Gigerenzer & Hoffrage (1995) Psychological Review 102.
