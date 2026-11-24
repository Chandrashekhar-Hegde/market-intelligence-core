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

- **Paths.** 2,000 by default (range 100–20,000), using a ChaCha8 generator seeded by `seed`.
  Each model draws its own stream, so the same inputs always give byte-identical output.
- **GBM.** Log returns are μ + σZ, with Z ~ N(0,1) and μ, σ the historical daily mean and
  standard deviation.
- **GARCH-t.** rₜ = μ + σₜzₜ, with σ² updated by the fitted GARCH recursion from today's
  conditional variance.
- **Output.**
  - Price quantiles (5, 25, 50, 75, 95%) for every session up to 63.
  - At 5, 21 and 63 sessions: return quantiles, P(gain), P(loss ≥ 10%), and P(drawdown ≥ 10%
    at any point along the path).
- **Caveat.** The drift μ is the historical average, and its standard error is printed next to
  it. The width of the fan is the reliable part; where the centre sits is not.

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

## Data quality (`/api/v1/bars` and `/analyze`)

Every series is checked for:
- impossible prices (OHLC inconsistency, non-positive values);
- gaps against the home-market index's trading days;
- staleness;
- extreme returns. A robust z above 8 (MAD-based) is flagged, and flagged moves within 3 days of a
  split or dividend are marked as such.

## References

Bollerslev (1986) J. Econometrics 31; Bollerslev (1987) REStat 69; Dickey & Fuller (1979) JASA 74;
Fama (1970) J. Finance 25; Hill (1975) Ann. Statist. 3; Jarque & Bera (1980) Econ. Letters 6;
Jensen (1968) J. Finance 23; J.P. Morgan/Reuters (1996) RiskMetrics Technical Document; Ljung & Box
(1978) Biometrika 65; Lo (2002) FAJ 58; Lo & MacKinlay (1988) RFS 1; MacKinnon (2010) Queen's
Economics WP 1227; McNeil, Frey & Embrechts (2005) Quantitative Risk Management; Nelder & Mead (1965)
Computer J. 7; Schwert (1989) JBES 7.
