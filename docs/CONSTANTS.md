# Every number in the analysis, and where it comes from

The owner asked for no unexplained hardcoded numbers (27 Nov 2026). Each constant has a kind:

| Kind | Meaning |
|---|---|
| **Fitted** | Estimated from data by the app, or once by us with the method and date given |
| **Literature** | A published parameter, kept fixed on purpose: choosing it from the same data would overfit |
| **Convention** | A standard statistical choice |
| **Market fact** | A rule or price of the market, with source and date; check it when the rule changes |
| **Judgement** | Our choice, not yet measured. The plan to replace it is given |
| **Product** | What the app shows, not a claim about markets |
| **Engineering** | Speed, limits and caching, with no effect on the numbers |

`gm-domain`'s test `every_constant_is_registered` fails if a `const` there is missing below.

## Data quality (`quality.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `OUTLIER_ROBUST_Z` | 8 | Convention | P(\|Z\| > 8) ≈ 1e-15 under a normal, ≈ 0.4% under t(3): an outlier worth a look |
| `MAD_TO_SIGMA` | 1.4826 | Convention | Consistent scale of the median absolute deviation under normality |
| `STALE_CALENDAR_DAYS` | 5 | Market fact | A weekend plus a two-day exchange holiday |
| `OHLC_TOLERANCE` | 1e-6 | Engineering | Vendors round prices to the tick |
| `BAD_PRINT_JUMP` | ln 2 | Market fact | A halving or doubling in a day is beyond every exchange band read (NSE: 20%) |
| `BAD_PRINT_MAX_RUN` | 5 | Fitted | The longest vendor bad-print runs seen in Yahoo data: 2 days (GOLDBEES 2019, MON100 2021); 5 leaves room |

## Distribution, volatility, risk, forecasts
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `MIN_RETURNS` | 60 | Convention | Below about three months, moments and tests mean little |
| `EWMA_LAMBDA` | 0.94 | Literature | RiskMetrics (J.P. Morgan/Reuters 1996), daily data |
| `SCALE` | 100 | Engineering | GARCH is fitted on percentage returns for numerical stability |
| `TERM_HORIZONS` | 1, 5, 21, 63 | Product | A day, a week, a month, a quarter |
| `LEVELS` | 95%, 99% | Convention | Basel and industry VaR/ES levels |
| `NORMAL_NU` | 200 | Engineering | Above this, t ≈ normal; statrs' t quantile hangs for huge ν |
| `DEFAULT_HORIZONS`, `CHANCE_HORIZONS` | 5, 21, 63 | Product | A week, a month, three months |
| `HORIZONS` (rules) | 1, 2, 3, 5, 10, 21, 63 | Product | Holding periods traders use, from a day to a quarter (short ones added 28 Nov 2026) |
| `DEFAULT_PATHS` | 2,000 | Engineering | Quantile error under 1 point at the 5/95% levels |
| `MAX_PATHS` | 20,000 | Engineering | Request limit |
| `PATHS` (long run) | 2,000 | Engineering | As above |
| `QUANTILES` (montecarlo) | 5, 25, 50, 75, 95% | Product | The 50% and 90% bands drawn |
| `SAMPLE_PATHS` | 30 | Product | Paths drawn faintly behind the fan |
| `YEARS` | 1, 3, 5 | Product | Holding periods compared with a fixed deposit |

## Validation (`validation.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `WARMUP` | 500 | Convention | About two years before the first out-of-sample forecast |
| `GARCH_REFIT_EVERY` | 63 | Convention | "Refit quarterly, filter daily" practice |
| `HISTORICAL_WINDOW` | 500 | Convention | Two-year historical-simulation window |
| `QUANTILES` (validation) | 1, 5, 25, 50, 75, 95, 99% | Convention | Pinball grid; 1% and 5% double as VaR levels |

## Trading rules (`signals.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `W` (trend) | 200 | Literature | Faber (2007): 200-day average |
| `LOOKBACK`, `SKIP` | 252, 21 | Literature | Jegadeesh & Titman (1993): 12-1 month momentum |
| `W` (reversal) | 5 | Literature | Lehmann (1990): weekly reversal |
| `W` (volume) | 20 | Literature | Gervais, Kaniel & Mingelgrin (2001): volume against the recent month |
| RSI 14 at 30/70 | 14, 30, 70 | Literature | Wilder (1978) |
| `RANK_WINDOW` | 252 | Convention | "Unusual" against the previous trading year |
| `ROUND_TRIP_COST` | 0.25% | Market fact | STT 0.1% each side, stamp duty 0.015%, exchange and SEBI fees, GST, spread allowance (zero-brokerage delivery, 2026). Per-stock spreads from daily ranges are a planned refinement |
| `MIN_INDEPENDENT` | 30 | Convention | Normal approximation to the binomial needs n ≳ 30 |
| `FAMILY_ALPHA` | 5% | Convention | Holm family-wise error rate (strict test) |
| `CALL_PROB` | 80 in 100 | Judgement (owner decision, 27 Nov 2026) | The chance of beating costs a call needs |

## News (`news_reader.rs`, `news.rs`, `macro_news.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `NON_VOTING` | general, price_move | Convention | Price reports and chatter describe, not explain |
| `FIT_T` | 1.96 | Convention | Two-sided 5% |
| `MIN_FIT_EVENTS`, `MIN_EVENTS` | 30 | Convention | As `MIN_INDEPENDENT` |
| `MATERIALITY_WEIGHT` | 0.25/0.5/1 | Judgement, **superseded** | Used only until the calibration fit is authoritative (30+ events in any kind). On 3 years of backfill no kind of news moved prices after it was published, so news no longer votes |
| `STRONG_SCORE` | 0.9 | Judgement, **superseded** | As above; the fitted vote compares the expected move with `ROUND_TRIP_COST` |
| `HALF_LIFE_HOURS` | 48 | Judgement | Fade of a story's weight. Only matters if a kind of news earns a weight; then fit it from the 1/2/5/10-session decay |
| `STORY_WEIGHT`, `LEAN_SCORE` | 0.5, 0.25 | Judgement | Desk leans (shown as pointers, never calls). To be fitted on market events once 30+ per asset |

## Track record (`track_record.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `REBALANCE` | 5 sessions | Convention | Weekly rebalancing keeps trading costs low (daily changes cost more than they save) |
| `WARM_UP` | 250 sessions | Convention | A year of history before "normal volatility" is known |
| `DEPOSIT_RATE` | 7% | Market fact | What un-invested money earns in the replay; the page's FD default |

## Comparison (`multi.rs`)
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `MIN_SYMBOLS`, `MAX_SYMBOLS` | 2, 10 | Product | |
| `MIN_COMMON_DAYS` | 120 | Convention | About six months in common before correlations are shown |
| `ROLLING_WINDOW` | 63 | Product | Three-month rolling correlation |

## Legacy and numerical
| Constant | Value | Kind | Basis |
|---|---|---|---|
| `DECISION_MODEL_VERSION` | rules-impact-v1 | Engineering | The retired `/decide` path |
| `A`, `B`, `C`, `D` (`stochastic.rs`) | coefficients | Literature | Acklam's rational approximation to the normal quantile |

## Outside the domain crate
| Number | Value | Kind | Where and basis |
|---|---|---|---|
| Fixed-deposit default | 7% | Market fact | Web `DEFAULT_FD_RATE`: a typical big-bank 1–3 year FD (Nov 2026); each user can change it |
| LRS limit, TCS | $250,000 a year; 20% above ₹10 lakh (from 1 Apr 2026) | Market fact | Today page; from secondary sources (Business Today, INDmoney), check the latest notification |
| Long-run tiers | 80, 60, 45 in 100 | Judgement | Wording of the long-run answer: 80 is the owner's bar for a firm statement |
| Single-stock growth | home index's | Fitted | Across the NIFTY 50 (2016–2026), the spread of 10-year mean returns is smaller than sampling noise (τ̂ = 0) |
| Panel correlation ρ̄ | measured daily (0.25 on 27 Nov 2026) | Fitted | Design effect for the rule priors |
| Model reader pacing | 5 s, batches of 25 | Engineering | Free-tier limits |
| Close-range profile (web `CLOSE_PROFILE`) | share of variance left and 90% band per half hour | Fitted | 50 NIFTY 50 stocks × 60 days of 5-minute bars (31 Aug–27 Nov 2026); held 88.8% out of sample. Refit when the kept 1-minute bars cover a year |
| Cache times, timeouts, parallelism | various | Engineering | `gm-api`, `gm-integrations` |
