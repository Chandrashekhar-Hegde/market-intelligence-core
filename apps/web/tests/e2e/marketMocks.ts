// Deterministic /api/v1 responses for e2e specs, so CI never depends on Yahoo being up.
import type { Page } from '@playwright/test'

const meta = (symbol: string, name: string) => ({
  symbol,
  name,
  currency: 'INR',
  exchange: 'NSE',
  instrument_type: symbol.startsWith('^') ? 'INDEX' : 'EQUITY',
  timezone: 'Asia/Kolkata',
  last_price: 1377.5,
  last_trade_at: '2026-11-20T09:58:10Z',
  change: 12.4,
  change_pct: 0.91,
  day_volume: 5_400_000,
  market_open: false,
  tradingview_symbol: symbol === 'RELIANCE.NS' ? 'NSE:RELIANCE' : symbol === '^GSPC' ? 'SP:SPX' : null,
})

function bars(symbol: string) {
  // 60 weekday sessions of a gentle random-walk-ish series (deterministic).
  const out = []
  const day = new Date('2026-08-24T00:00:00Z')
  let close = 1300
  while (out.length < 60) {
    if (day.getUTCDay() % 6 !== 0) {
      close += Math.sin(out.length) * 12
      out.push({
        symbol,
        date: day.toISOString().slice(0, 10),
        open: close - 5,
        high: close + 15,
        low: close - 18,
        close,
        adj_close: close,
        volume: 1_000_000 + out.length * 1000,
        source: 'yahoo-chart-v8',
      })
    }
    day.setUTCDate(day.getUTCDate() + 1)
  }
  return out
}

export const LIVE_TICK = {
  symbol: 'RELIANCE.NS',
  price: 1391.25,
  change: 13.75,
  change_pct: 1.0,
  day_high: 1395,
  day_low: 1370.1,
  day_volume: 5_400_000,
  currency: 'INR',
  exchange: 'NSI',
  tick_time: '', // filled at request time so it is always fresh
  source: 'yahoo-streamer',
}

export async function mockMarketData(page: Page, { liveTicks = false } = {}) {
  await page.route('**/api/v1/instruments/search**', (route) =>
    route.fulfill({
      json: {
        results: [
          { symbol: 'RELIANCE.NS', name: 'Reliance Industries Limited', exchange: 'NSE', country: 'India', quote_type: 'EQUITY' },
          { symbol: 'RELIANCE.BO', name: 'Reliance Industries Limited', exchange: 'BSE', country: 'India', quote_type: 'EQUITY' },
        ].filter((h) => {
          const params = new URL(route.request().url()).searchParams
          const keep = (f: string | null, v: string) => !f || f === 'all' || f === v
          return keep(params.get('country'), h.country) && keep(params.get('exchange'), h.exchange)
        }),
        markets: [
          { country: 'India', exchange: 'BSE', count: 1 },
          { country: 'India', exchange: 'NSE', count: 1 },
        ],
      },
    }),
  )
  await page.route('**/api/v1/quote**', (route) => {
    const symbol = new URL(route.request().url()).searchParams.get('symbol') ?? '^NSEI'
    return route.fulfill({
      json: {
        quote: meta(symbol, symbol === 'RELIANCE.NS' ? 'Reliance Industries Limited' : 'NIFTY 50'),
        source: 'yahoo-chart-v8',
        fetched_at: '2026-11-24T09:00:00Z',
        delayed: true,
      },
    })
  })
  await page.route('**/api/v1/bars**', (route) => {
    const symbol = new URL(route.request().url()).searchParams.get('symbol') ?? '^NSEI'
    const series = bars(symbol)
    return route.fulfill({
      json: {
        chart: {
          meta: meta(symbol, 'Mock'),
          bars: series,
          corporate_actions: [{ kind: 'dividend', date: '2026-09-15', amount: 5.5 }],
          dropped_partial_bar: false,
          source: 'yahoo-chart-v8',
          fetched_at: '2026-11-24T09:00:00Z',
        },
        calendar_reference: symbol === 'RELIANCE.NS' ? '^NSEI' : null,
        quality: {
          passed: true,
          n_bars: series.length,
          first_date: series[0]?.date,
          last_date: series.at(-1)?.date,
          ohlc_violations: [],
          non_positive_prices: [],
          zero_volume_days: 0,
          missing_days: [],
          is_stale: false,
          outliers: [],
          warnings: [],
        },
      },
    })
  })
  await page.route('**/api/v1/intraday**', (route) => {
    const symbol = new URL(route.request().url()).searchParams.get('symbol') ?? '^NSEI'
    // One NSE session of 1-minute bars, 09:15-15:30 IST on 2026-11-20.
    const open = Date.parse('2026-11-20T03:45:00Z') / 1000
    const points = Array.from({ length: 375 }, (_, i) => {
      const close = 1365 + Math.sin(i / 20) * 8 + i * 0.03
      return { t: open + i * 60, open: close - 0.5, high: close + 1, low: close - 1, close, volume: 10_000 }
    })
    return route.fulfill({
      json: {
        chart: {
          meta: meta(symbol, 'Mock'),
          interval: '1m',
          previous_close: 1365.1,
          session_start: open,
          session_end: open + 375 * 60,
          points,
          source: 'yahoo-chart-v8',
          fetched_at: '2026-11-24T09:00:00Z',
        },
      },
    })
  })
  await page.route('**/api/v1/news**', (route) =>
    route.fulfill({
      json: {
        symbol: 'RELIANCE.NS', query: 'Reliance Industries',
        articles: [{
          article: {
            title: 'Reliance wins a large order', url: 'https://example.test/r', publisher: 'Livemint', source: 'Google News',
            seen_at: new Date(Date.now() - 3600_000).toISOString(), summary: null,
          },
          reading: { kind: 'order_win', direction: 1, materiality: 2, reason: 'A new order or contract adds future revenue.', reader: 'built-in' },
          impact: { instrument_move: 0.021, market_move: 0.002, abnormal_move: 0.019, sessions_elapsed: 0.2, z: 2.9, one_in: 270 },
        }],
        vote: { score: 0.5, stance: 0, n_stories: 1, drivers: [] },
        reader: 'built-in',
        sources: [
          { name: 'Google News', ok: true, count: 1, error: null },
          { name: 'Economic Times', ok: true, count: 0, error: null },
          { name: 'Mint', ok: true, count: 0, error: null },
          { name: 'GDELT', ok: false, count: 0, error: 'rate-limited' },
        ],
        benchmark: '^NSEI', beta: 1.1, fetched_at: new Date().toISOString(),
      },
    }),
  )
  await page.route('**/api/v1/stream**', (route) => {
    const symbols = new URL(route.request().url()).searchParams.get('symbols') ?? ''
    const body =
      liveTicks && symbols.includes(LIVE_TICK.symbol)
        ? `event: tick\ndata: ${JSON.stringify({ ...LIVE_TICK, tick_time: new Date().toISOString() })}\n\n`
        : ': keep-alive\n\n'
    return route.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body })
  })
  await page.route('**/api/v1/analyze**', (route) => {
    const symbol = new URL(route.request().url()).searchParams.get('symbol') ?? '^GSPC'
    return route.fulfill({ json: analysis(symbol) })
  })
  await page.route('**/api/v1/desk**', (route) => route.fulfill({ json: deskResponse() }))
  await page.route('**/api/v1/today**', (route) => route.fulfill({ json: todayResponse() }))
  await page.route('**/api/v1/compare**', (route) => {
    const symbols = (new URL(route.request().url()).searchParams.get('symbols') ?? '').split(',')
    return route.fulfill({ json: comparison(symbols) })
  })
  // Never load the real third-party widget in tests.
  await page.route('https://s3.tradingview.com/**', (route) =>
    route.fulfill({ contentType: 'text/javascript', body: '' }),
  )
}

/** The fixture console lives behind the "Event lab" tab. */
function analysis(symbol: string) {
  const result = (statistic: number, p: number) => ({ statistic, p_value: p, reject_5pct: p < 0.05 })
  const last = bars(symbol).at(-1)!
  const fan = Array.from({ length: 63 }, (_, i) => ({
    day: i + 1,
    price: [-0.1, -0.04, 0, 0.04, 0.1].map((f) => last.close * (1 + f * Math.sqrt((i + 1) / 63))),
  }))
  const horizons = [5, 21, 63].map((h) => ({
    horizon_days: h,
    return_quantiles: [-0.08, -0.02, 0.004, 0.03, 0.09],
    prob_gain: 0.53,
    prob_loss_over_10pct: 0.03,
    prob_drawdown_over_10pct: 0.07,
  }))
  const isEquity = !symbol.startsWith('^')
  return {
    report: {
      symbol,
      first_date: '2026-08-24',
      last_date: last.date,
      n_returns: 59,
      last_price: last.close,
      periods_per_year: 251,
      distribution: {
        mean_annual: 0.08, mean_annual_se: 0.2, vol_annual: 0.22, skewness: -0.2, excess_kurtosis: 1.1,
        jarque_bera: result(4.2, 0.12), hill_tail_index: null, worst_day: -0.03, best_day: 0.03,
      },
      predictability: {
        lags: 10,
        ljung_box_returns: result(9.1, 0.52),
        ljung_box_squared: result(12.3, 0.27),
        variance_ratios: [{ q: 2, ratio: 1.02, test: result(0.2, 0.84) }],
        adf: { statistic: -1.5, critical_values: [-3.5, -2.9, -2.6], reject_unit_root_5pct: false },
      },
      volatility: { ewma_annual: 0.21, garch: null },
      projections: [
        { model: 'gbm', n_paths: 2000, seed: 42, fan, horizons, sample_paths: [], typical_path: [], block_length: null },
        {
          model: 'bootstrap', n_paths: 2000, seed: 42, fan, horizons, block_length: 12,
          typical_path: fan.map((p) => p.price[2]),
          sample_paths: [0.95, 1.04, 1.0].map((f) => fan.map((p) => p.price[2] * (1 + (f - 1) * Math.sqrt(p.day / 63)))),
        },
      ],
      risk: {
        var_es: [
          { method: 'historical', confidence: 0.95, var: 0.021, es: 0.027 },
          { method: 'historical', confidence: 0.99, var: 0.029, es: 0.031 },
        ],
        max_drawdown: { max: 0.06, peak_date: '2026-09-01', trough_date: '2026-09-20', recovery_date: null, duration_days: 84, current: 0.02 },
        sharpe: 0.36, sharpe_se: 2.1, sortino: 0.5,
      },
      benchmark: isEquity
        ? { symbol: '^NSEI', n_common_days: 60, beta: 1.08, beta_se: 0.09, alpha_annual: 0.01, alpha_annual_se: 0.2, r_squared: 0.48, correlation: 0.69, idiosyncratic_vol_annual: 0.15 }
        : null,
      validation: {
        n_forecasts: 753,
        first_forecast_index: 500,
        baseline: 'historical_sim',
        models: ['historical_sim', 'ewma_normal', 'garch_t'].map((model, i) => ({
          model,
          mean_pinball: 0.002 - i * 0.0001,
          vs_baseline: i === 0 ? null : result(-1.2 * i, i === 2 ? 0.01 : 0.2),
          coverage_90: 0.9,
          var_backtests: [0.99, 0.95].map((level) => ({
            level, expected_rate: 1 - level, exceptions: Math.round((1 - level) * 753), rate: 1 - level,
            kupiec: result(0.1, 0.8), christoffersen: result(0.2, 0.7), pass: true,
          })),
          direction: model === 'ewma_normal' ? null : { n: 750, hits: 390, accuracy: 0.52, binomial: result(390, 0.29) },
          verdict: i === 0
            ? 'Baseline: what any model has to beat.'
            : i === 2
              ? 'Earns its place: forecasts beat historical simulation out of sample and its VaR holds up.'
              : 'No significant difference from historical simulation out of sample.',
        })),
        chance_of_gain: [5, 21, 63].map((h) => ({
          horizon_days: h, n: 30, mean_forecast: 0.6, realised: 0.57, brier: 0.24, skill_vs_coin: 0.04, vs_coin: result(-0.8, 0.42),
        })),
      },
      lead_model: 'bootstrap',
      lead_reason: 'No model forecast this instrument’s past days better than simply replaying its history, so the forecast replays its own past days.',
      findings: [
        'Annualised volatility 22.0% over 59 trading days.',
        'No evidence of predictability in returns: Ljung–Box p = 0.520 and no variance ratio is significant.',
      ],
    },
    data: {
      symbol, name: 'Mock', currency: 'INR', exchange: 'NSE', instrument_type: isEquity ? 'EQUITY' : 'INDEX',
      tradingview_symbol: null, range: '5y', source: 'yahoo-chart-v8', fetched_at: '2026-11-24T09:00:00Z',
      quality: { passed: true, n_bars: 60, first_date: null, last_date: null, ohlc_violations: [], non_positive_prices: [], zero_volume_days: 0, missing_days: [], is_stale: false, outliers: [], warnings: [] },
    },
    benchmark: isEquity ? { symbol: '^NSEI', name: 'NIFTY 50', selection: 'auto', used: true, currency: 'INR', note: null } : null,
    signals: {
      first_date: '2016-11-24', last_date: last.date, n_days: 2466, round_trip_cost: 0.0025, n_tests: 6,
      signals: [
        {
          id: 'trend_200d', name: 'Long-term trend', rule: 'Buy while the price is above its 200-day average; stay out below it.',
          source: 'Faber (2007)', position_today: 1, today: 'The price is above its 200-day average.', flip_level: 1300,
          tests: [5, 21, 63].map((h) => ({
            horizon_days: h, n_obs: 2000, n_independent: Math.floor(400 / h), effect: 0.001, t_stat: 0.4, p_value: 0.34,
            p_holm: 1, effect_first_half: 0.001, effect_second_half: 0.001, hit_rate: 0.52, hit_n: Math.floor(400 / h),
            hit_low: 0.47, hit_high: 0.57, mean_return_long: 0.002, mean_return_short: 0.001, earns_place: false,
            verdict: 'No reliable edge: this could be luck.',
          })),
        },
      ],
    },
    advice: {
      stance: 'hold', basis: 'no_evidence',
      headline: 'Hold. No rule has a proven edge on this instrument: its past moves have not predicted its next ones well enough to beat trading costs.',
      drivers: [], horizon_days: null, review_date: null, confidence: null, flip_level: null, reasons: [], news_headlines: [],
    },
    signals_note: null,
    news_vote: { score: 0.5, stance: 0, n_stories: 1, drivers: [] },
  }
}

function comparison(symbols: string[]) {
  const k = symbols.length
  const matrix = (base: number) => symbols.map((_, i) => symbols.map((__, j) => (i === j ? 1 : base)))
  const pairs = []
  for (let i = 0; i < k; i++) {
    for (let j = i + 1; j < k; j++) {
      pairs.push({
        a: symbols[i], b: symbols[j], correlation_daily: 0.55, correlation_weekly: 0.62,
        cointegration: {
          dependent: symbols[i], regressor: symbols[j], hedge_ratio: 1.02, statistic: i === 0 && j === 1 ? -3.8 : -1.5, lags: 22,
          critical_values: [-3.9, -3.34, -3.05], cointegrated_5pct: i === 0 && j === 1,
          half_life_days: i === 0 && j === 1 ? 21 : null, spread_z_now: 0.7,
        },
      })
    }
  }
  const series = bars('X')
  return {
    comparison: {
      symbols, first_date: series[0]!.date, last_date: series.at(-1)!.date, n_common_days: 60, n_weekly: 12,
      correlation_daily: matrix(0.55), spearman_daily: matrix(0.5), correlation_weekly: matrix(0.62),
      covariance_shrinkage: 0.03, covariance_annual: matrix(0.02),
      pca: { explained: [0.7, 0.2, 0.1], first_loadings: symbols.map(() => 0.5) },
      pairs, rolling_correlation: [],
      performance: symbols.map((symbol, i) => ({ symbol, total_return: 0.1 * (i + 1), annual_return: 0.1, annual_vol: 0.2, sharpe: 0.5, max_drawdown: 0.1 })),
      rebased: series.map((b, t) => ({ date: b.date, values: symbols.map((_, i) => 100 + t * (i + 1) * 0.3) })),
      findings: [`60 trading days in common. One shared factor explains 70% of the daily variance of these ${k} instruments.`],
    },
    instruments: symbols.map((symbol) => ({ symbol, name: symbol, currency: 'USD', exchange: 'NMS', timezone: 'America/New_York', source: 'yahoo-chart-v8', fetched_at: '2026-11-25T07:00:00Z' })),
    range: '5y',
    notes: [],
  }
}

/** The Compare tab. */
export async function openCompare(page: Page) {
  await page.getByRole('tab', { name: /compare/i }).click()
}

/** The news desk: two themed stories and the leans they produce. */
export function deskResponse() {
  return {
    sources: [
      { name: 'Bloomberg', ok: true, count: 20, error: null },
      { name: 'Google News: India', ok: false, count: 0, error: 'no answer in time' },
    ],
    stories: [
      {
        title: 'Crude Oil Futures Rise on Positive Global Trends', url: 'https://example.com/crude', publisher: 'Rediff',
        source: 'Google News', seen_at: '2026-11-25T06:00:00Z', theme: 'Commodities',
        impacts: [
          { asset: 'crude', direction: 1, reason: 'Dearer crude squeezes India’s oil marketers and weighs on the rupee (India imports most of its oil).' },
          { asset: 'omc', direction: -1, reason: 'Dearer crude squeezes India’s oil marketers and weighs on the rupee (India imports most of its oil).' },
        ],
      },
      {
        title: 'Sensex rises 300 points', url: 'https://example.com/sensex', publisher: 'Mint', source: 'Mint',
        seen_at: '2026-11-25T05:00:00Z', theme: 'Market report', impacts: [],
      },
    ],
    leans: [
      { asset: 'crude', label: 'Crude oil', symbol: 'BZ=F', score: 0.5, lean: 1, drivers: [0] },
      { asset: 'omc', label: 'Oil marketers (BPCL, HPCL, IOC)', symbol: 'BPCL.NS', score: -0.5, lean: -1, drivers: [0] },
      { asset: 'gold', label: 'Gold', symbol: 'GOLDBEES.NS', score: 0, lean: 0, drivers: [] },
    ],
    reader: 'built-in',
    as_of: '2026-11-25T06:00:00Z',
  }
}

/** Today's list: a quiet day across a few groups. */
export function todayResponse() {
  const check = (said: number, rose: number) => ({
    horizon_days: 63, n: 31, mean_forecast: said, realised: rose, brier: 0.24, skill_vs_coin: 0.02,
    vs_coin: { statistic: -0.5, p_value: 0.6, reject_5pct: false },
  })
  const hold = (basis: string) => ({
    stance: 'hold', basis, headline: 'Hold. No rule has a proven edge on this instrument.', drivers: [], horizon_days: null,
    review_date: null, confidence: null, flip_level: null, reasons: [], news_headlines: [],
  })
  // Long-run percentiles spread evenly from lo to hi (total log return over the period).
  const runs = (lo3: number, hi3: number) =>
    [3, 5].map((years) => ({
      years,
      resampled: Array.from({ length: 101 }, (_, p) => ((lo3 + ((hi3 - lo3) * p) / 100) * years) / 3),
      historical: Array.from({ length: 101 }, (_, p) => ((lo3 + ((hi3 - lo3) * p) / 100) * years) / 3),
      n_windows: 1000,
      n_independent: Math.floor(10 / years),
      worst: lo3,
      block_length: 10,
      drift_annual: 0.12,
      drift_se_annual: 0.05,
      drift_from: 'own history',
    }))
  const item = (symbol: string, name: string, group: string, extra: Record<string, unknown> = {}) => ({
    symbol, name, group, currency: 'INR', instrument_type: 'EQUITY', last_price: 100, change_pct: 0.5,
    last_date: '2026-11-26', return_1y: 0.1, vol_annual: 0.2, lead_model: 'bootstrap', prob_gain_1m: 0.55,
    prob_gain_3m: 0.6, prob_loss_10pct_3m: 0.1, range_3m: [-0.15, 0.2], chance_check_3m: check(0.6, 0.6),
    advice: hold('no_evidence'), news_vote: null, long_run: runs(-0.3, 0.5), vol_ratio: 1, es95: 0.025, ...extra,
  })
  return {
    as_of: '2026-11-27T09:00:00Z',
    notice: 'Research signals from past data, for a private beta. Not SEBI-registered investment advice: the app advises, you decide.',
    items: [
      item('^NSEI', 'NIFTY 50', 'India index', { instrument_type: 'INDEX', vol_annual: 0.15, long_run: runs(-0.2, 1.0) }),
      item('RELIANCE.NS', 'Reliance Industries', 'NIFTY 50', { prob_gain_3m: 0.61, chance_check_3m: check(0.69, 0.55), long_run: runs(-0.1, 1.0), single_stock: true }),
      item('POWERGRID.NS', 'Power Grid', 'NIFTY 50', { advice: hold('proven_but_quiet'), single_stock: true }),
      item('GOLDBEES.NS', 'Gold ETF', 'Gold, silver, rupee', { instrument_type: 'ETF', vol_annual: 0.13, prob_loss_10pct_3m: 0.03, long_run: runs(0, 1.2), vol_ratio: 1.6, es95: 0.02 }),
      item('INR=X', 'USD/INR', 'Gold, silver, rupee', { instrument_type: 'CURRENCY', return_1y: 0.04, vol_annual: 0.05 }),
      item('MON100.NS', 'Nasdaq 100 ETF', 'US from India', { instrument_type: 'ETF', vol_annual: 0.2 }),
      item('NVDA', 'NVIDIA', 'AI', { currency: 'USD', return_1y: 0.3, vol_annual: 0.5, prob_gain_3m: 0.62 }),
      { symbol: 'BADX', group: 'Your watchlist', error: 'symbol not found' },
    ],
  }
}
