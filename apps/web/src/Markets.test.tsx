import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import type { Advice, AnalyzeResponse, NewsResponse, BarsResponse, InstrumentMeta, IntradayData, QuoteResponse, SignalTest, Signals, Tick } from './api'
import { maxPosition, meaningCards, relativeVolume, sessionLeft, sessionRange, stanceGuidance } from './plainLanguage'
import { Markets } from './Markets'
import {
  appendTick,
  dayChange,
  displayPrice,
  formatCompact,
  liveState,
  mergeTick,
  priceCurrency,
  toCandles,
} from './marketData'

// ---- fixtures ---------------------------------------------------------------------------

const NOW = Date.parse('2026-11-25T06:00:00Z') // 11:30 IST, a Wednesday session

function meta(overrides: Partial<InstrumentMeta> = {}): InstrumentMeta {
  return {
    symbol: '^NSEI',
    name: 'NIFTY 50',
    currency: 'INR',
    exchange: 'NSE',
    instrument_type: 'INDEX',
    timezone: 'Asia/Kolkata',
    last_price: 25349.65,
    last_trade_at: '2026-11-23T09:59:00Z',
    change: null,
    change_pct: null,
    day_volume: null,
    market_open: false,
    tradingview_symbol: 'SP:SPX',
    ...overrides,
  }
}

function tick(overrides: Partial<Tick> = {}): Tick {
  return {
    symbol: '^NSEI',
    price: 25410.5,
    change: 60.85,
    change_pct: 0.24,
    day_high: 25450,
    day_low: 25300,
    day_volume: null,
    currency: 'INR',
    exchange: 'NSI',
    tick_time: new Date(NOW - 3_000).toISOString(),
    source: 'yahoo-streamer',
    ...overrides,
  }
}

function bar(date: string, close: number) {
  return { symbol: '^NSEI', date, open: close - 10, high: close + 20, low: close - 30, close, adj_close: close, volume: 0, source: 'yahoo-chart-v8' }
}

function barsResponse(overrides: Partial<BarsResponse['quality']> = {}): BarsResponse {
  return {
    chart: {
      meta: meta(),
      bars: [bar('2026-11-19', 25200), bar('2026-11-20', 25300), bar('2026-11-23', 25349.65)],
      corporate_actions: [{ kind: 'dividend', date: '2026-06-05', amount: 6 }],
      dropped_partial_bar: false,
      source: 'yahoo-chart-v8',
      fetched_at: '2026-11-24T09:00:00Z',
    },
    calendar_reference: '^NSEI',
    quality: {
      passed: true,
      n_bars: 3,
      first_date: '2026-11-19',
      last_date: '2026-11-23',
      ohlc_violations: [],
      non_positive_prices: [],
      zero_volume_days: 3,
      missing_days: [],
      is_stale: false,
      outliers: [],
      warnings: ['3 zero-volume days (normal for indices)'],
      ...overrides,
    },
  }
}

const quoteResponse = (overrides: Partial<InstrumentMeta> = {}): QuoteResponse => ({
  quote: meta(overrides),
  source: 'yahoo-chart-v8',
  fetched_at: '2026-11-24T09:00:00Z',
  delayed: true,
})

function analysis(overrides: Partial<AnalyzeResponse['report']> = {}): AnalyzeResponse {
  const result = (statistic: number, p: number) => ({ statistic, p_value: p, reject_5pct: p < 0.05 })
  const fan = Array.from({ length: 63 }, (_, i) => ({
    day: i + 1,
    price: [0.9, 0.95, 1, 1.05, 1.1].map((f) => 25349.65 * (1 + (f - 1) * Math.sqrt((i + 1) / 63))) as [number, number, number, number, number],
  }))
  const samples = [0.97, 1.03].map((f) => Array.from({ length: 63 }, (_, i) => 25349.65 * (1 + (f - 1) * Math.sqrt((i + 1) / 63))))
  const horizons = [5, 21, 63].map((h) => ({
    horizon_days: h,
    return_quantiles: [-0.08, -0.02, 0.004, 0.03, 0.09] as [number, number, number, number, number],
    prob_gain: 0.54,
    prob_loss_over_10pct: 0.03,
    prob_drawdown_over_10pct: 0.07,
  }))
  return {
    report: {
      symbol: '^NSEI',
      first_date: '2021-11-24',
      last_date: '2026-11-23',
      n_returns: 1253,
      last_price: 25349.65,
      periods_per_year: 251,
      distribution: {
        mean_annual: 0.11, mean_annual_se: 0.078, vol_annual: 0.174, skewness: -0.3, excess_kurtosis: 6.1,
        jarque_bera: result(1950, 0), hill_tail_index: 3.4, worst_day: -0.061, best_day: 0.09,
      },
      predictability: {
        lags: 10,
        ljung_box_returns: result(14.2, 0.164),
        ljung_box_squared: result(812, 0),
        variance_ratios: [{ q: 2, ratio: 0.97, test: result(-0.6, 0.55) }],
        adf: { statistic: -1.1, critical_values: [-3.44, -2.86, -2.57], reject_unit_root_5pct: false },
      },
      volatility: {
        ewma_annual: 0.19,
        garch: {
          alpha: 0.1, beta: 0.88, nu: 7.2, persistence: 0.982, half_life_days: 38, long_run_vol_annual: 0.17,
          current_vol_annual: 0.21,
          term_structure: [1, 5, 21, 63].map((h) => ({ horizon_days: h, vol_annual: 0.2 })),
        },
      },
      projections: [
        { model: 'gbm', n_paths: 2000, seed: 42, fan, horizons, sample_paths: samples, block_length: null },
        { model: 'garch_t', n_paths: 2000, seed: 42, fan, horizons: horizons.map((h) => ({ ...h, prob_gain: 0.52 })), sample_paths: samples, block_length: null },
        { model: 'bootstrap', n_paths: 2000, seed: 42, fan, horizons: horizons.map((h) => ({ ...h, prob_gain: 0.51 })), sample_paths: samples, block_length: 12.4 },
      ],
      risk: {
        var_es: [
          { method: 'historical', confidence: 0.95, var: 0.0171, es: 0.0254 },
          { method: 'historical', confidence: 0.99, var: 0.0298, es: 0.0391 },
          { method: 'garch_t', confidence: 0.99, var: 0.0335, es: 0.0442 },
        ],
        max_drawdown: { max: 0.254, peak_date: '2022-01-03', trough_date: '2022-10-12', recovery_date: '2024-01-19', duration_days: 746, current: 0.01 },
        sharpe: 0.63, sharpe_se: 0.45, sortino: 0.88,
      },
      benchmark: null,
      validation: {
        n_forecasts: 753,
        first_forecast_index: 500,
        baseline: 'historical_sim',
        models: [
          {
            model: 'historical_sim', mean_pinball: 0.0021, vs_baseline: null, coverage_90: 0.896,
            var_backtests: [
              { level: 0.99, expected_rate: 0.01, exceptions: 12, rate: 0.016, kupiec: result(2.1, 0.15), christoffersen: result(0.4, 0.53), pass: true },
              { level: 0.95, expected_rate: 0.05, exceptions: 40, rate: 0.053, kupiec: result(0.2, 0.65), christoffersen: result(9.8, 0.002), pass: false },
            ],
            direction: { n: 750, hits: 397, accuracy: 0.529, binomial: result(397, 0.11) },
            verdict: 'Baseline: what any model has to beat.',
          },
          {
            model: 'garch_t', mean_pinball: 0.00195, vs_baseline: result(-3.1, 0.002), coverage_90: 0.9,
            var_backtests: [
              { level: 0.99, expected_rate: 0.01, exceptions: 8, rate: 0.011, kupiec: result(0.1, 0.8), christoffersen: result(0.2, 0.7), pass: true },
              { level: 0.95, expected_rate: 0.05, exceptions: 37, rate: 0.049, kupiec: result(0.0, 0.9), christoffersen: result(0.5, 0.5), pass: true },
            ],
            direction: null,
            verdict: 'Earns its place: forecasts beat historical simulation out of sample and its VaR holds up.',
          },
        ],
      },
      findings: [
        'Annualised volatility 17.4% over 1253 trading days.',
        'No evidence of predictability in returns: Ljung–Box p = 0.164 and no variance ratio is significant.',
      ],
      ...overrides,
    },
    data: {
      symbol: '^NSEI', name: 'NIFTY 50', currency: 'INR', exchange: 'NSE', instrument_type: 'INDEX',
      tradingview_symbol: 'SP:SPX', range: '5y', source: 'yahoo-chart-v8', fetched_at: '2026-11-24T09:00:00Z',
      quality: barsResponse().quality,
    },
    benchmark: null,
    signals: signalsFixture(),
    advice: HOLD_NO_EVIDENCE,
    signals_note: null,
    news_vote: { score: 0.2, stance: 0, n_stories: 3, drivers: [] },
  }
}

function signalTest(h: number, overrides: Partial<SignalTest> = {}): SignalTest {
  return {
    horizon_days: h, n_obs: 2000, n_independent: 400 / h, effect: 0.001, t_stat: 0.4, p_value: 0.34, p_holm: 1,
    effect_first_half: 0.001, effect_second_half: 0.001, hit_rate: 0.52, hit_n: 400 / h, hit_low: 0.47, hit_high: 0.57,
    mean_return_long: 0.002, mean_return_short: 0.001, earns_place: false,
    verdict: 'No reliable edge over about a week: after checking 12 rule-and-horizon combinations, this could be luck (adjusted p = 1.00).',
    ...overrides,
  }
}

function signalsFixture(): Signals {
  return {
    first_date: '2016-11-24', last_date: '2026-11-23', n_days: 2466, round_trip_cost: 0.0025, n_tests: 6,
    signals: [
      {
        id: 'trend_200d', name: 'Long-term trend', rule: 'Buy while the price is above its 200-day average; stay out below it.',
        source: 'Faber (2007)', position_today: 1, today: 'The price (25349.65) is above its 200-day average (24800.00).',
        flip_level: 24800, tests: [5, 21, 63].map((h) => signalTest(h)),
      },
      {
        id: 'rsi_14', name: 'Overbought / oversold (RSI)', rule: 'Buy when the 14-day RSI is below 30.', source: 'Wilder (1978)',
        position_today: 0, today: 'RSI is 52: neither overbought nor oversold.', flip_level: null,
        tests: [5, 21, 63].map((h) => signalTest(h)),
      },
    ],
  }
}

const HOLD_NO_EVIDENCE: Advice = {
  stance: 'hold', basis: 'no_evidence',
  headline: 'Hold. No rule has a proven edge on this instrument: its past moves have not predicted its next ones well enough to beat trading costs.',
  drivers: [], horizon_days: null, review_date: null, confidence: null, flip_level: null, reasons: [], news_headlines: [],
}

/** Trend proven over a month and long today: a Buy. */
function buyAnalysis(): AnalyzeResponse {
  const base = analysis()
  const signals = signalsFixture()
  signals.signals[0]!.tests[1] = signalTest(21, {
    effect: 0.012, p_holm: 0.01, earns_place: true, hit_rate: 0.64, hit_n: 110, hit_low: 0.55, hit_high: 0.72,
    verdict: 'Has worked on this instrument over about a month (adjusted p = 0.010).',
  })
  return {
    ...base,
    signals,
    advice: {
      stance: 'buy', basis: 'proven',
      headline: 'Buy for about a month. Long-term trend has worked on this instrument: right 64% of the time over 110 past calls.',
      drivers: ['trend_200d'], horizon_days: 21, review_date: '2026-12-22',
      confidence: { hit_rate: 0.64, low: 0.55, high: 0.72, n: 110 }, flip_level: 24800, reasons: [], news_headlines: [],
    },
  }
}

const SESSION_OPEN = Math.floor(Date.parse('2026-11-25T03:45:00Z') / 1000) // 09:15 IST

function intradayResponse(): { chart: IntradayData } {
  return {
    chart: {
      meta: meta(),
      interval: '1m',
      previous_close: 25349.65,
      session_start: SESSION_OPEN,
      session_end: SESSION_OPEN + 375 * 60,
      points: [0, 1, 2].map((i) => ({
        t: SESSION_OPEN + i * 60,
        open: 25350 + i,
        high: 25360 + i,
        low: 25340 + i,
        close: 25355 + i,
        volume: 1000,
      })),
      source: 'yahoo-chart-v8',
      fetched_at: '2026-11-25T03:48:00Z',
    },
  }
}

function newsResponse(): NewsResponse {
  const story = (title: string, publisher: string, source: string, hoursAgo: number) => ({
    title, url: `https://example.test/${title.length}`, publisher, source, summary: null,
    seen_at: new Date(Date.now() - hoursAgo * 3600_000).toISOString(),
  })
  return {
    symbol: '^NSEI',
    query: 'NIFTY 50',
    articles: [
      {
        article: { ...story('Indian stocks hit fresh highs', 'Economic Times', 'Economic Times', 2), url: 'https://example.test/a' },
        reading: { kind: 'general', direction: 1, materiality: 1, reason: 'Upbeat wording, but no clear market-moving event.', reader: 'built-in' },
        impact: { instrument_move: 0.0099, market_move: null, abnormal_move: 0.0099, sessions_elapsed: 0.3, z: 1.1, one_in: 3.7 },
      },
      {
        article: story('Budget surprise', 'Livemint', 'Google News', 26),
        reading: { kind: 'legal', direction: -1, materiality: 3, reason: 'Legal or regulatory trouble usually hits the share price.', reader: 'built-in' },
        impact: { instrument_move: -0.0305, market_move: null, abnormal_move: -0.0305, sessions_elapsed: 1, z: -2.6, one_in: 107 },
      },
      { article: story('Old story', 'reuters.com', 'GDELT', 70), reading: { kind: 'general', direction: 0, materiality: 1, reason: 'No clear event.', reader: 'built-in' }, impact: null },
    ],
    sources: [
      { name: 'Google News', ok: true, count: 1, error: null },
      { name: 'Economic Times', ok: true, count: 1, error: null },
      { name: 'Mint', ok: true, count: 0, error: null },
      { name: 'GDELT', ok: false, count: 0, error: 'gdelt status 429 Too Many Requests' },
    ],
    benchmark: null,
    beta: null,
    vote: { score: -0.4, stance: 0, n_stories: 3, drivers: [] },
    reader: 'built-in',
    fetched_at: new Date().toISOString(),
  }
}

// ---- harness -----------------------------------------------------------------------------

class FakeEventSource {
  static instances: FakeEventSource[] = []
  listeners = new Map<string, (event: MessageEvent<string>) => void>()
  closed = false
  url: string
  constructor(url: string) {
    this.url = url
    FakeEventSource.instances.push(this)
  }
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void) {
    this.listeners.set(type, listener)
  }
  close() {
    this.closed = true
  }
  emit(type: string, data: unknown) {
    act(() => this.listeners.get(type)?.(new MessageEvent(type, { data: JSON.stringify(data) })))
  }
}

type Routes = Record<string, (url: URL) => Response>

function mockApi(routes: Routes = {}) {
  const defaults: Routes = {
    '/api/v1/quote': (url) => {
      const symbol = url.searchParams.get('symbol') ?? '^NSEI'
      const tv = symbol === 'INFY.NS' ? 'NSE:INFY' : 'SP:SPX'
      return Response.json(quoteResponse({ symbol, tradingview_symbol: tv }))
    },
    '/api/v1/analyze': () => Response.json(analysis()),
    '/api/v1/bars': () => Response.json(barsResponse()),
    '/api/v1/intraday': () => Response.json(intradayResponse()),
    '/api/v1/news': () => Response.json(newsResponse()),
    '/api/v1/instruments/search': (url) => {
      // Mirrors the server: `markets` counts every match, `results` honours the filters.
      const all = [
        { symbol: 'INFY.NS', name: 'Infosys Limited', exchange: 'NSE', country: 'India', quote_type: 'EQUITY' },
        { symbol: 'INFY.BO', name: 'Infosys Limited', exchange: 'BSE', country: 'India', quote_type: 'EQUITY' },
        { symbol: 'INFY', name: 'Infosys Limited ADR', exchange: 'NYSE', country: 'USA', quote_type: 'EQUITY' },
      ]
      const keep = (filter: string | null, value: string) => !filter || filter === 'all' || filter === value
      return Response.json({
        results: all.filter((h) => keep(url.searchParams.get('country'), h.country) && keep(url.searchParams.get('exchange'), h.exchange)),
        markets: [
          { country: 'India', exchange: 'BSE', count: 1 },
          { country: 'India', exchange: 'NSE', count: 1 },
          { country: 'USA', exchange: 'NYSE', count: 1 },
        ],
      })
    },
  }
  const all = { ...defaults, ...routes }
  const fetchMock = vi.fn<typeof fetch>(async (input) => {
    const url = new URL(input.toString())
    return all[url.pathname]?.(url) ?? new Response('not found', { status: 404 })
  })
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('EventSource', FakeEventSource)
  FakeEventSource.instances = []
  return fetchMock
}

function renderMarkets() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Markets />
    </QueryClientProvider>,
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

// ---- pure helpers ------------------------------------------------------------------------

describe('liveState', () => {
  test('a fresh tick is LIVE with its lag', () => {
    const state = liveState(meta(), tick(), NOW)
    expect(state.kind).toBe('live')
    expect(state.detail).toMatch(/updated 3 s ago/)
  })

  test('a 15-minute-old tick in market hours is an exchange-delayed feed (BSE), not LIVE', () => {
    const bse = tick({ tick_time: new Date(NOW - 902_000).toISOString() })
    const state = liveState(meta({ market_open: true }), bse, NOW)
    expect(state).toMatchObject({ kind: 'delayed', label: 'DELAYED 15 MIN' })
    expect(state.detail).toMatch(/BSE is 15 min; NSE is live/)
    expect(liveState(meta({ market_open: false }), bse, NOW).kind).toBe('closed')
  })

  test('a very old tick falls back to the polled quote', () => {
    const old = tick({ tick_time: new Date(NOW - 60 * 60_000).toISOString() })
    expect(liveState(meta({ market_open: true }), old, NOW)).toMatchObject({ kind: 'delayed', label: 'DELAYED' })
    expect(liveState(meta({ market_open: false }), old, NOW).kind).toBe('closed')
  })

  test('closed market and loading states', () => {
    expect(liveState(meta(), null, NOW)).toMatchObject({ kind: 'closed', label: 'CLOSED' })
    expect(liveState(undefined, null, NOW).kind).toBe('loading')
  })
})

describe('displayPrice', () => {
  test('prefers a tick newer than the last polled trade', () => {
    expect(displayPrice(meta(), tick())).toBe(25410.5)
    expect(displayPrice(meta({ last_trade_at: new Date(NOW).toISOString() }), tick())).toBe(25349.65)
    expect(displayPrice(meta(), null)).toBe(25349.65)
    expect(displayPrice(undefined, null)).toBeNull()
  })
})

describe('appendTick', () => {
  const points = intradayResponse().chart.points
  const at = (seconds: number) => new Date((SESSION_OPEN + seconds) * 1000).toISOString()

  test('a tick inside the last minute extends that bar', () => {
    const merged = appendTick(points, tick({ tick_time: at(150), price: 25400 }), 60)
    expect(merged).toHaveLength(3)
    expect(merged.at(-1)).toMatchObject({ t: SESSION_OPEN + 120, close: 25400, high: 25400, low: 25342 })
  })

  test('a tick in a later minute opens a bar aligned to the minute', () => {
    const merged = appendTick(points, tick({ tick_time: at(301), price: 25300 }), 60)
    expect(merged).toHaveLength(4)
    expect(merged.at(-1)).toEqual({ t: SESSION_OPEN + 300, open: 25300, high: 25300, low: 25300, close: 25300, volume: 0 })
  })

  test('older ticks and empty series change nothing', () => {
    expect(appendTick(points, tick({ tick_time: at(-60) }), 60)).toBe(points)
    expect(appendTick([], tick(), 60)).toEqual([])
  })
})

describe('number helpers', () => {
  test('compact numbers use lakh and crore', () => {
    expect(formatCompact(5_400_000)).toMatch(/54\s?L/)
    expect(formatCompact(12_000_000)).toMatch(/1\.2\s?Cr/)
    expect(formatCompact(null)).toBe('—')
  })

  test('indices carry no currency sign; day change prefers the tick', () => {
    expect(priceCurrency(meta())).toBeNull()
    expect(priceCurrency(meta({ instrument_type: 'EQUITY' }))).toBe('INR')
    expect(dayChange(meta({ change: 10, change_pct: 0.5 }), null)).toEqual({ change: 10, pct: 0.5 })
    expect(dayChange(meta({ change: 10, change_pct: 0.5 }), tick())).toEqual({ change: 60.85, pct: 0.24 })
    expect(dayChange(meta(), null)).toBeNull()
  })
})

describe('mergeTick', () => {
  const candles = toCandles(barsResponse().chart.bars)

  test('a tick on the last bar day extends that candle', () => {
    const sameDay = tick({ tick_time: '2026-11-23T08:00:00Z', price: 25600 })
    const merged = mergeTick(candles, sameDay, 'Asia/Kolkata')
    expect(merged).toHaveLength(3)
    expect(merged.at(-1)).toMatchObject({ close: 25600, high: 25600, low: 25349.65 - 30 })
  })

  test('a tick in a later session opens a new candle', () => {
    const merged = mergeTick(candles, tick(), 'Asia/Kolkata')
    expect(merged).toHaveLength(4)
    expect(merged.at(-1)).toEqual({ time: '2026-11-25', open: 25410.5, high: 25410.5, low: 25410.5, close: 25410.5 })
  })

  test('older ticks and empty history change nothing', () => {
    expect(mergeTick(candles, tick({ tick_time: '2026-11-01T05:00:00Z' }), 'Asia/Kolkata')).toBe(candles)
    expect(mergeTick([], tick(), 'Asia/Kolkata')).toEqual([])
  })

  test('the exchange date, not UTC, decides the session', () => {
    // 23:00 UTC on the 23rd is 04:30 IST on the 24th: a new session, not the 23rd's candle.
    const lateUtc = tick({ tick_time: '2026-11-23T23:00:00Z' })
    expect(mergeTick(candles, lateUtc, 'Asia/Kolkata').at(-1)?.time).toBe('2026-11-24')
  })
})

// ---- the view ----------------------------------------------------------------------------

describe('Markets view', () => {
  test('shows the default index with quote, history, quality and corporate actions', async () => {
    const fetchMock = mockApi()
    renderMarkets()

    // An index is shown in points, without a currency sign.
    await waitFor(() => expect(screen.getByTestId('quote-price')).toHaveTextContent(/^25,349\.65/))
    expect(screen.getByTestId('live-state')).toHaveTextContent('CLOSED')
    // Today's minute chart is the default view.
    expect(await screen.findByTestId('intraday-count')).toHaveTextContent('3')
    expect(screen.getByText(/dashed line: 25,349\.65, where today’s change is measured from/)).toBeInTheDocument()
    expect(screen.getByTestId('quality-badge')).toHaveTextContent('Usable')
    expect(screen.getByText(/3 zero-volume days/)).toBeInTheDocument()
    expect(screen.getByText(/Dividend ₹6.00/)).toBeInTheDocument()
    expect(screen.getByText(/TradingView live chart \(SP:SPX\)/)).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/bars?symbol=%5ENSEI&range=1y'),
      expect.anything(),
    )
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/intraday?symbol=%5ENSEI&range=1d'),
      expect.anything(),
    )
    // One stream carries the selected symbol and the whole watchlist.
    const url = decodeURIComponent(FakeEventSource.instances[0]?.url ?? '')
    expect(url).toContain('/api/v1/stream?symbols=^NSEI,')
    expect(url).toContain('RELIANCE.NS')
    expect(url).toContain('INR=X')
    expect(FakeEventSource.instances).toHaveLength(1)
  })

  test('the watchlist shows each pick with its day change, and ticks update it', async () => {
    mockApi({
      '/api/v1/quote': (url) =>
        Response.json(quoteResponse({ symbol: url.searchParams.get('symbol') ?? '', change: -12.5, change_pct: -0.9, instrument_type: 'EQUITY' })),
    })
    renderMarkets()
    const watchlist = screen.getByRole('region', { name: 'Watchlist' })
    const reliance = within(watchlist).getByRole('button', { name: /^Reliance/ })
    await waitFor(() => expect(reliance).toHaveTextContent('-0.90%'))
    expect(reliance).toHaveClass('watch-down')
    FakeEventSource.instances.at(-1)!.emit('tick', tick({ symbol: 'RELIANCE.NS', price: 1400, change_pct: 1.25 }))
    expect(reliance).toHaveTextContent('₹1,400.00')
    expect(reliance).toHaveTextContent('+1.25%')
    expect(reliance).toHaveClass('watch-up')
  })

  test('a live tick turns the pill LIVE and moves the price and today’s chart', async () => {
    mockApi()
    renderMarkets()
    await screen.findByTestId('intraday-count')

    FakeEventSource.instances.at(-1)!.emit('tick', tick({ tick_time: new Date().toISOString() }))

    expect(screen.getByTestId('live-state')).toHaveTextContent('LIVE')
    expect(screen.getByTestId('quote-price')).toHaveTextContent('25,410.50')
    expect(screen.getByTestId('quote-price')).toHaveTextContent('+60.85 (+0.24%)')
    expect(screen.getByText(/Today’s range 25,300.00 – 25,450.00/)).toBeInTheDocument()
    expect(screen.getByTestId('intraday-count')).toHaveTextContent('4')

    // The daily view folds the same tick into today's candle.
    await userEvent.click(screen.getByRole('button', { name: '1Y' }))
    expect(await screen.findByTestId('candle-count')).toHaveTextContent('4')
  })

  test('ticks for other symbols are ignored', async () => {
    mockApi()
    renderMarkets()
    await screen.findByTestId('intraday-count')
    FakeEventSource.instances.at(-1)!.emit('tick', tick({ symbol: 'TCS.NS', tick_time: new Date().toISOString() }))
    expect(screen.getByTestId('live-state')).toHaveTextContent('CLOSED')
  })

  test('search picks an instrument and reloads quote, bars and stream for it', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    await waitFor(() => expect(screen.getByTestId('quote-price')).toHaveTextContent('25,349.65'))

    await userEvent.type(screen.getByLabelText(/search stocks/i), 'infy')
    const results = await screen.findByRole('list', { name: 'Search results' })
    await userEvent.click(await within(results).findByRole('button', { name: /INFY\.NS/ }))

    expect(await screen.findByText('INFY.NS', { selector: '.eyebrow' })).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/quote?symbol=INFY.NS'), expect.anything())
    expect(FakeEventSource.instances[0]?.closed).toBe(true)
    expect(FakeEventSource.instances.at(-1)?.url).toMatch(/symbols=INFY\.NS%2C/)
    expect(screen.getByText(/TradingView live chart \(NSE:INFY\)/)).toBeInTheDocument()
  })

  test('watchlist picks and range buttons are toggle buttons', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    await screen.findByTestId('intraday-count')

    await userEvent.click(screen.getByRole('button', { name: '5D' }))
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/intraday?symbol=%5ENSEI&range=5d'), expect.anything())
    await userEvent.click(screen.getByRole('button', { name: '5Y' }))
    expect(screen.getByRole('button', { name: '5Y' })).toHaveAttribute('aria-pressed', 'true')
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/bars?symbol=%5ENSEI&range=5y'), expect.anything())

    await userEvent.click(screen.getByRole('button', { name: /^Reliance/ }))
    expect(screen.getByRole('button', { name: /^Reliance/ })).toHaveAttribute('aria-pressed', 'true')
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('symbol=RELIANCE.NS'), expect.anything())
  })

  test('market-open quote without ticks says DELAYED', async () => {
    mockApi({ '/api/v1/quote': () => Response.json(quoteResponse({ market_open: true })) })
    renderMarkets()
    expect(await screen.findByText('DELAYED')).toBeInTheDocument()
    expect(screen.getByText(/checking every 15 s/)).toBeInTheDocument()
  })

  test('failed quality checks and outliers are shown, not hidden', async () => {
    mockApi({
      '/api/v1/bars': () =>
        Response.json(
          barsResponse({
            passed: false,
            is_stale: true,
            missing_days: ['2026-11-18'],
            outliers: [{ date: '2026-11-02', log_return: 0.1097, robust_z: 9.68, near_corporate_action: false }],
          }),
        ),
    })
    renderMarkets()
    expect(await screen.findByTestId('quality-badge')).toHaveTextContent('Not usable')
    expect(screen.getByText(/Stale: the market traded after the last bar/)).toBeInTheDocument()
    expect(screen.getByText(/1 missing trading days/)).toBeInTheDocument()
    const row = screen.getByRole('row', { name: /2026-11-02/ })
    expect(row).toHaveTextContent('11.6%')
    expect(row).toHaveTextContent('9.7')
  })

  test('unknown symbol and provider outage get readable errors', { timeout: 8_000 }, async () => {
    mockApi({
      '/api/v1/quote': () => Response.json({ error: 'symbol not found' }, { status: 404 }),
      '/api/v1/bars': () => Response.json({ error: 'provider unavailable' }, { status: 503 }),
    })
    renderMarkets()
    // 404 fails at once; 503 is retried twice (1 s + 2 s backoff) before it is shown.
    await waitFor(
      () =>
        expect(screen.getAllByRole('alert').map((a) => a.textContent)).toEqual(
          expect.arrayContaining([
            expect.stringMatching(/Unknown symbol/),
            expect.stringMatching(/provider is unavailable/),
          ]),
        ),
      { timeout: 5_000 },
    )
  })

  test('market and exchange chips filter the results, India first', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    await userEvent.type(screen.getByLabelText(/search stocks/i), 'infy')
    const results = await screen.findByRole('list', { name: 'Search results' })
    expect(await within(results).findByRole('button', { name: /^INFY Infosys Limited ADR NYSE · USA/ })).toBeInTheDocument()
    expect(within(results).getByRole('button', { name: /INFY\.BO/ })).toHaveTextContent('BSE · India')

    const markets = screen.getByRole('group', { name: 'instrument-search country' })
    expect(within(markets).getAllByRole('button').map((b) => b.textContent)).toEqual(['All', 'India 2', 'USA 1'])
    await userEvent.click(within(markets).getByRole('button', { name: /India/ }))
    const exchanges = await screen.findByRole('group', { name: 'instrument-search exchange' })
    await userEvent.click(within(exchanges).getByRole('button', { name: /BSE/ }))
    await waitFor(() =>
      expect(within(screen.getByRole('list', { name: 'Search results' })).getAllByRole('button').map((b) => b.textContent)).toEqual([
        'INFY.BO Infosys Limited BSE · India · EQUITY',
      ]),
    )
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('country=India&exchange=BSE'), expect.anything())
  })

  test('empty search results say so', async () => {
    mockApi({ '/api/v1/instruments/search': () => Response.json({ results: [], markets: [] }) })
    renderMarkets()
    await userEvent.type(screen.getByLabelText(/search stocks/i), 'zzzz')
    expect(await screen.findByText(/No matches for “zzzz”/)).toBeInTheDocument()
  })
})

describe('analysis panels', () => {
  test('show findings, forecast fan, risk, volatility and tests for the default index', async () => {
    const fetchMock = mockApi()
    renderMarkets()

    const findings = await screen.findByTestId('findings')
    expect(findings).toHaveTextContent('No evidence of predictability')
    expect(screen.getByTestId('fan-days')).toHaveTextContent('63')
    // The stock's own history replayed is preferred; GARCH-t and GBM are one click away.
    expect(screen.getByRole('button', { name: 'Own history' })).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByRole('row', { name: /^21 sessions/ })).toHaveTextContent('51%')
    await userEvent.click(screen.getByRole('button', { name: 'GBM' }))
    expect(screen.getByRole('row', { name: /^21 sessions/ })).toHaveTextContent('54%')
    expect(screen.getByRole('row', { name: /^21 sessions/ })).toHaveTextContent('-8.0% to 9.0%')

    const varTable = screen.getByTestId('var-table')
    expect(within(varTable).getByRole('row', { name: /GARCH-t/ })).toHaveTextContent('3.35%')
    expect(screen.getByText(/Worst drawdown 25.4%/)).toHaveTextContent('recovered 2024-01-19')
    expect(screen.getByText(/shocks halve in 38 sessions/)).toBeInTheDocument()
    expect(within(screen.getByTestId('tests-table')).getByRole('row', { name: /Volatility clustering/ })).toHaveTextContent('Clusters')
    // An index is the market itself: no benchmark needed, and the UI says why.
    expect(screen.getByText(/Indices, currencies, futures and crypto are the market themselves/)).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/analyze?symbol=%5ENSEI&range=10y'), expect.anything())
  })

  test('shows benchmark exposure and the currency note for a foreign listing', async () => {
    mockApi({
      '/api/v1/analyze': () => {
        const base = analysis({
          symbol: 'NOKIA.HE',
          benchmark: {
            symbol: 'ACWI', n_common_days: 1240, beta: 0.68, beta_se: 0.04, alpha_annual: -0.05, alpha_annual_se: 0.09,
            r_squared: 0.21, correlation: 0.46, idiosyncratic_vol_annual: 0.33,
          },
        })
        return Response.json({
          ...base,
          benchmark: {
            symbol: 'ACWI', name: 'MSCI ACWI (global)', selection: 'auto', used: true, currency: 'USD',
            note: 'NOKIA.HE trades in EUR and the benchmark in USD; beta and alpha include currency moves.',
          },
        })
      },
    })
    renderMarkets()
    const exposure = await screen.findByTestId('benchmark')
    expect(exposure).toHaveTextContent('Beta 0.68 ± 0.04')
    expect(exposure).toHaveTextContent('chosen automatically')
    expect(screen.getByRole('heading', { name: 'Against MSCI ACWI (global) (ACWI)' })).toBeInTheDocument()
    expect(screen.getByText(/beta and alpha include currency moves/)).toBeInTheDocument()
  })

  test('short history: no GARCH, GBM only, and the reason is stated', async () => {
    mockApi({
      '/api/v1/analyze': () => {
        const base = analysis()
        base.report.volatility.garch = null
        base.report.projections = base.report.projections.filter((p) => p.model === 'gbm')
        return Response.json(base)
      },
    })
    renderMarkets()
    expect(await screen.findByText(/GARCH needs at least 250 daily returns/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'GBM' })).toHaveAttribute('aria-pressed', 'true')
    expect(screen.queryByRole('button', { name: 'GARCH-t' })).not.toBeInTheDocument()
  })

  test('analysis errors are readable and leave the rest of the page working', async () => {
    mockApi({ '/api/v1/analyze': () => Response.json({ error: 'need at least 60 daily returns, got 12' }, { status: 400 }) })
    renderMarkets()
    expect(await screen.findByText(/Invalid request: need at least 60 daily returns, got 12/)).toBeInTheDocument()
    expect(await screen.findByTestId('quality-badge')).toBeInTheDocument()
  })
})

describe('validation panel', () => {
  test('scores each model against the baseline with VaR backtests and a verdict', async () => {
    mockApi()
    renderMarkets()
    const table = await screen.findByTestId('validation-table')
    const garch = within(table).getByRole('row', { name: /^GARCH-t/ })
    expect(garch).toHaveTextContent('-7.1% (p 0.002)')
    expect(garch).toHaveTextContent('8 vs 8 expected')
    expect(garch).toHaveTextContent('Earns its place')
    expect(garch).toHaveTextContent('none forecast')
    const base = within(table).getByRole('row', { name: /^Historical simulation/ })
    expect(base).toHaveTextContent('reference')
    expect(within(base).getAllByText('fail')).toHaveLength(1)
    expect(screen.getByText(/Walk-forward over 753 trading days/)).toBeInTheDocument()
  })

  test('short histories say why there is no validation', async () => {
    mockApi({ '/api/v1/analyze': () => Response.json(analysis({ validation: null })) })
    renderMarkets()
    expect(await screen.findByText(/Validation needs at least 600 daily returns/)).toBeInTheDocument()
  })
})

describe('plain language', () => {
  test('cards put the numbers in money for the amount chosen', () => {
    const cards = Object.fromEntries(meaningCards(analysis(), 10_000).map((c) => [c.id, c.headline]))
    // 17.4% a year over 251 sessions is 1.1% a day: ₹110 on ₹10,000.
    expect(cards['typical-day']).toMatch(/about 1\.1% up or down in a day: ₹110 on ₹10,000/)
    expect(cards['bad-day']).toMatch(/1 day in 20 it has fallen more than 1\.7% \(₹171 on ₹10,000\).*average fall was 2\.5% \(₹254\)/)
    expect(cards['worst-fall']).toMatch(/25% \(Jan 2022 to Oct 2022\), and it recovered by Jan 2024/)
    expect(cards['where-it-could-be']).toMatch(/in 1 month, between ₹9,200 and ₹10,900 \(chance of being higher: 51%\)/)
    expect(meaningCards(analysis(), 10_000).find((c) => c.id === 'where-it-could-be')?.why).toMatch(
      /^From 2,000 possible futures built by replaying this stock’s own past days in runs of about 12 days/,
    )
    expect(cards['predictable']).toMatch(/^No\./)
    expect(cards['trust']).toMatch(/^Yes/)
    // An index is the market: no comparison with itself.
    expect(cards['vs-market']).toBeUndefined()
    expect(meaningCards(analysis(), 20_000).find((c) => c.id === 'typical-day')?.headline).toMatch(/₹220 on ₹20,000/)
  })

  test('a stock is compared with its market using β/ρ', () => {
    const stock = analysis({
      benchmark: { symbol: '^NSEI', n_common_days: 1240, beta: 1.2, beta_se: 0.04, alpha_annual: 0, alpha_annual_se: 0.05, r_squared: 0.36, correlation: 0.6, idiosyncratic_vol_annual: 0.2 },
    })
    const cards = Object.fromEntries(meaningCards(stock, 10_000).map((c) => [c.id, c.headline]))
    expect(cards['vs-market']).toMatch(/about 2\.0× as bumpy as the \^NSEI/)
    expect(cards['with-market']).toMatch(/moves 1%, this has moved about 1\.2%.*explains 36%/)
  })

  test('position size, session ranges and relative volume', () => {
    // ₹500 bad-day loss at a 2.54% average bad-day fall: at most ₹19,685.
    expect(maxPosition(analysis(), 500)).toBeCloseTo(500 / 0.0254, 6)
    expect(maxPosition(analysis(), 0)).toBeNull()
    // The fixture's 1-day quantiles are ±10%·√(1/63); half a session scales them by √0.5.
    const [lo, hi] = sessionRange(analysis(), 100, 0.5)!
    expect(lo).toBeCloseTo(100 * Math.exp(Math.log(1 - 0.1 / Math.sqrt(63)) * Math.sqrt(0.5)), 9)
    expect(hi).toBeGreaterThan(100)
    expect(sessionRange(analysis(), 100, 0)).toEqual([100, 100])
    expect(sessionLeft(0, 100, 25)).toBeCloseTo(0.75)
    expect(sessionLeft(0, 100, -5)).toBe(1)
    expect(sessionLeft(0, 100, 200)).toBe(0)
    expect(sessionLeft(null, 100, 5)).toBeNull()
    expect(relativeVolume(500, 1000, 0.25)).toBeCloseTo(2)
    expect(relativeVolume(500, 1000, 0.01)).toBeNull()
  })
})

describe('owner and non-owner guidance', () => {
  const base = { headline: '', drivers: [], horizon_days: null, review_date: null, confidence: null, flip_level: null, reasons: [], news_headlines: [] }
  test('the same call means different actions for holders and buyers', () => {
    const buy = stanceGuidance({ ...base, stance: 'buy', basis: 'proven' }, 19_685, 'INR')
    expect(buy.owner).toBe('You can add more, keeping the total under ₹19,685 so a bad day stays within your limit.')
    expect(buy.buyer).toMatch(/^You can buy, keeping the total under ₹19,685/)
    const sell = stanceGuidance({ ...base, stance: 'sell', basis: 'proven' }, 19_685, 'INR')
    expect(sell).toEqual({ owner: 'Consider selling or reducing it: the evidence points down from here.', buyer: 'Don’t buy it now.' })
    const hold = stanceGuidance({ ...base, stance: 'hold', basis: 'no_evidence' }, 19_685, 'INR')
    expect(hold.owner).toMatch(/^Keep it if it still fits your plan/)
    expect(hold.buyer).toMatch(/^No tested reason to buy now.*under ₹19,685/)
    expect(stanceGuidance({ ...base, stance: 'hold', basis: 'conflict' }, null, 'INR').buyer).toMatch(/^Wait/)
    expect(stanceGuidance({ ...base, stance: 'hold', basis: 'proven_but_quiet' }, null, 'INR').owner).toMatch(/^Keep it/)
  })
})

describe('advice card', () => {
  test('explains Hold for owners and buyers, and what the labels mean', async () => {
    mockApi()
    renderMarkets()
    const guidance = await screen.findByTestId('guidance')
    expect(guidance).toHaveTextContent('If you already own itKeep it if it still fits your plan')
    expect(guidance).toHaveTextContent('If you don’t own it yetNo tested reason to buy now')
    const legend = screen.getByRole('list', { name: 'What the labels mean' })
    expect(legend).toHaveTextContent('the rule’s reading today points up / down')
    expect(legend).toHaveTextContent('does not change the call')
  })

  test('the history switch reloads the analysis on 5 years', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    const picker = await screen.findByRole('group', { name: 'History used' })
    expect(within(picker).getByRole('button', { name: '10 years' })).toHaveAttribute('aria-pressed', 'true')
    await userEvent.click(within(picker).getByRole('button', { name: '5 years' }))
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/analyze?symbol=%5ENSEI&range=5y'), expect.anything()),
    )
  })

  test('no proven rule: Hold, with what each rule sees and why none counts', async () => {
    mockApi()
    renderMarkets()
    const card = await screen.findByTestId('advice')
    expect(within(card).getByTestId('stance')).toHaveTextContent('Hold')
    expect(card).toHaveTextContent('No proven edge')
    expect(card).toHaveTextContent(/No rule has a proven edge/)
    const rules = within(card).getByTestId('rules')
    expect(within(rules).getAllByRole('listitem')).toHaveLength(2)
    expect(rules).toHaveTextContent('Long-term trendLeans buyNot proven here')
    expect(within(card).getByTestId('position-size')).toHaveTextContent('Put in at most ₹19,685')
    await userEvent.clear(within(card).getByLabelText(/most i’m willing to lose/i))
    await userEvent.type(within(card).getByLabelText(/most i’m willing to lose/i), '1000')
    expect(within(card).getByTestId('position-size')).toHaveTextContent('Put in at most ₹39,370')
  })

  test('a proven rule: Buy with its hit rate, time frame, review date and flip level', async () => {
    mockApi({ '/api/v1/analyze': () => Response.json(buyAnalysis()) })
    renderMarkets()
    const card = await screen.findByTestId('advice')
    expect(within(card).getByTestId('stance')).toHaveTextContent('Buy')
    expect(card).toHaveTextContent('Right 64% of the time over 110 past calls (likely between 55% and 72%)')
    expect(card).toHaveTextContent(/Time frame: 1 month\. Look again on Tue, 22 Dec 2026/)
    expect(card).toHaveTextContent(/changes if the price closes below ₹24,800\.00/)
    expect(card).toHaveTextContent('✓ Has worked here (1 month)')
  })

  test('a live price across the flip level says the rule would switch', async () => {
    mockApi({ '/api/v1/analyze': () => Response.json(buyAnalysis()) })
    renderMarkets()
    await screen.findByTestId('advice')
    FakeEventSource.instances.at(-1)!.emit('tick', tick({ price: 24500, tick_time: new Date().toISOString() }))
    expect(await screen.findByText(/would switch to “leans sell” if the day closed here/)).toBeInTheDocument()
  })

  test('strong news with no proven rule: a labelled news-driven call with its headlines', async () => {
    mockApi({
      '/api/v1/analyze': () =>
        Response.json({
          ...analysis(),
          news_vote: { score: -1.4, stance: -1, n_stories: 9, drivers: [2] },
          advice: {
            ...HOLD_NO_EVIDENCE,
            stance: 'sell',
            basis: 'news_driven',
            headline: 'Sell or stay out, based on today’s news. No track record yet.',
            horizon_days: 5,
            review_date: '2026-12-04',
            news_headlines: ['SEBI orders forensic audit of the company'],
          },
        }),
    })
    renderMarkets()
    const card = await screen.findByTestId('advice')
    expect(within(card).getByTestId('stance')).toHaveTextContent('Sell / stay out')
    expect(card).toHaveTextContent('Based on today’s news · no track record')
    expect(within(card).getByTestId('news-behind')).toHaveTextContent('“SEBI orders forensic audit of the company”')
    expect(within(card).getByTestId('guidance')).toHaveTextContent('If you already own itConsider reducing it: today’s news is clearly bad')
    expect(within(card).getByTestId('guidance')).toHaveTextContent('Don’t buy until the news settles.')
  })

  test('too little history: no call, and the reason', async () => {
    mockApi({
      '/api/v1/analyze': () =>
        Response.json({ ...analysis(), signals: null, advice: null, signals_note: 'Not enough history to test the trading rules (needs about 1.5 years).' }),
    })
    renderMarkets()
    expect(await screen.findByRole('heading', { name: 'No call possible yet' })).toBeInTheDocument()
    expect(screen.getByText(/needs about 1\.5 years/)).toBeInTheDocument()
  })

  test('the meaning cards and the folded maths both render', async () => {
    mockApi()
    renderMarkets()
    expect(await screen.findByTestId('meaning-cards')).toHaveTextContent('A typical day')
    expect(screen.getByText('Show all the maths')).toBeInTheDocument()
    expect(screen.getByTestId('signals-table')).toHaveTextContent('Long-term trend')
  })
})

describe('news panel', () => {
  test('headlines from every source with outlet, time and measured reaction', async () => {
    mockApi()
    renderMarkets()
    const news = await screen.findByTestId('news')
    expect(await within(news).findByTestId('news-sources')).toHaveTextContent(
      'Sources: Google News ✓ 1 · Economic Times ✓ 1 · Mint ✓ 0 · GDELT ✗ unavailable',
    )
    expect(within(news).getByRole('link', { name: 'Indian stocks hit fresh highs' })).toHaveAttribute('href', 'https://example.test/a')
    expect(news).toHaveTextContent('Economic Times · 2 h ago')
    expect(news).toHaveTextContent('Livemint via Google News · 26 h ago')
    expect(news).toHaveTextContent('Since then: +1.0%. An ordinary move for this instrument')
    expect(news).toHaveTextContent('Since then: -3.0%. Unusual: a move this size happens about 1 in 107 times without news.')
    expect(news).toHaveTextContent('searching for “NIFTY 50”')
    expect(news).toHaveTextContent('Bad for the company Legal / regulator · major Legal or regulatory trouble usually hits the share price.')
    expect(news).toHaveTextContent('read by the app’s built-in news reader')
  })

  test('every source down is said plainly and does not break the page', async () => {
    mockApi({ '/api/v1/news': () => Response.json({ error: 'Google News: status 503; gdelt status 429' }, { status: 503 }) })
    renderMarkets()
    expect(await screen.findByText(/News is unavailable right now/)).toBeInTheDocument()
    expect(screen.getByTestId('advice')).toBeInTheDocument()
  })
})

describe('comparison words', () => {
  test('correlation in words (Evans 1996) and the equal-mix swing', async () => {
    const { correlationWords, equalMix } = await import('./plainLanguage')
    expect(correlationWords(0.85)).toBe('move very closely together')
    expect(correlationWords(0.45)).toBe('move somewhat together')
    expect(correlationWords(-0.3)).toBe('move loosely in opposite directions')
    expect(correlationWords(0.05)).toBe('move independently of each other')
    // Two instruments, each ±20% a year, uncorrelated: an equal mix swings 20%/√2.
    const mix = equalMix([[0.04, 0], [0, 0.04]])!
    expect(mix.equalMixVol).toBeCloseTo(0.2 / Math.SQRT2, 12)
    expect(mix.averageAloneVol).toBeCloseTo(0.2, 12)
    expect(mix.cut).toBeCloseTo(1 - 1 / Math.SQRT2, 12)
  })
})
