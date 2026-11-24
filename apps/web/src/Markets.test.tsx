import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import type { AnalyzeResponse, BarsResponse, InstrumentMeta, QuoteResponse, Tick } from './api'
import { Markets } from './Markets'
import {
  displayPrice,
  liveState,
  mergeTick,
  toCandles,
} from './marketData'

// ---- fixtures ---------------------------------------------------------------------------

const NOW = Date.parse('2026-11-25T06:00:00Z') // 11:30 IST, a Wednesday session

function meta(overrides: Partial<InstrumentMeta> = {}): InstrumentMeta {
  return {
    symbol: '^GSPC',
    name: 'NIFTY 50',
    currency: 'INR',
    exchange: 'NSE',
    instrument_type: 'INDEX',
    timezone: 'Asia/Kolkata',
    last_price: 25349.65,
    last_trade_at: '2026-11-23T09:59:00Z',
    market_open: false,
    tradingview_symbol: 'SP:SPX',
    ...overrides,
  }
}

function tick(overrides: Partial<Tick> = {}): Tick {
  return {
    symbol: '^GSPC',
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
  return { symbol: '^GSPC', date, open: close - 10, high: close + 20, low: close - 30, close, adj_close: close, volume: 0, source: 'yahoo-chart-v8' }
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
    calendar_reference: '^GSPC',
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
  const horizons = [5, 21, 63].map((h) => ({
    horizon_days: h,
    return_quantiles: [-0.08, -0.02, 0.004, 0.03, 0.09] as [number, number, number, number, number],
    prob_gain: 0.54,
    prob_loss_over_10pct: 0.03,
    prob_drawdown_over_10pct: 0.07,
  }))
  return {
    report: {
      symbol: '^GSPC',
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
        { model: 'gbm', n_paths: 2000, seed: 42, fan, horizons },
        { model: 'garch_t', n_paths: 2000, seed: 42, fan, horizons: horizons.map((h) => ({ ...h, prob_gain: 0.51 })) },
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
      symbol: '^GSPC', name: 'S&P 500', currency: 'USD', exchange: 'SNP', instrument_type: 'INDEX',
      tradingview_symbol: 'SP:SPX', range: '5y', source: 'yahoo-chart-v8', fetched_at: '2026-11-24T09:00:00Z',
      quality: barsResponse().quality,
    },
    benchmark: null,
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
      const symbol = url.searchParams.get('symbol') ?? '^GSPC'
      const tv = symbol === 'INFY.NS' ? 'NSE:INFY' : 'SP:SPX'
      return Response.json(quoteResponse({ symbol, tradingview_symbol: tv }))
    },
    '/api/v1/analyze': () => Response.json(analysis()),
    '/api/v1/bars': () => Response.json(barsResponse()),
    '/api/v1/instruments/search': () =>
      Response.json({
        results: [
          { symbol: 'INFY.NS', name: 'Infosys Limited', exchange: 'NSE', quote_type: 'EQUITY' },
          { symbol: 'INFY.BO', name: 'Infosys Limited', exchange: 'Bombay', quote_type: 'EQUITY' },
        ],
      }),
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
    expect(state.detail).toMatch(/lag 3s/)
  })

  test('a stale tick falls back to the quote', () => {
    const old = tick({ tick_time: new Date(NOW - 10 * 60_000).toISOString() })
    expect(liveState(meta({ market_open: true }), old, NOW).kind).toBe('delayed')
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

    await waitFor(() => expect(screen.getByTestId('quote-price')).toHaveTextContent('₹25,349.65'))
    expect(screen.getByTestId('live-state')).toHaveTextContent('CLOSED')
    expect(await screen.findByTestId('candle-count')).toHaveTextContent('3')
    expect(screen.getByTestId('quality-badge')).toHaveTextContent('Usable')
    expect(screen.getByText(/3 zero-volume days/)).toBeInTheDocument()
    expect(screen.getByText(/Dividend ₹6.00/)).toBeInTheDocument()
    expect(screen.getByText(/TradingView live chart \(SP:SPX\)/)).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/bars?symbol=%5EGSPC&range=1y'),
      expect.anything(),
    )
    expect(FakeEventSource.instances[0]?.url).toContain('/api/v1/stream?symbols=%5EGSPC')
  })

  test('a live tick turns the pill LIVE and updates price and today’s candle', async () => {
    mockApi()
    renderMarkets()
    await screen.findByTestId('candle-count')

    FakeEventSource.instances.at(-1)!.emit('tick', tick({ tick_time: new Date().toISOString() }))

    expect(screen.getByTestId('live-state')).toHaveTextContent('LIVE')
    expect(screen.getByTestId('quote-price')).toHaveTextContent('₹25,410.50')
    expect(screen.getByTestId('quote-price')).toHaveTextContent('+60.85 (+0.24%)')
    expect(screen.getByText(/Day range ₹25,300.00 – ₹25,450.00/)).toBeInTheDocument()
    expect(screen.getByTestId('candle-count')).toHaveTextContent('4')
  })

  test('ticks for other symbols are ignored', async () => {
    mockApi()
    renderMarkets()
    await screen.findByTestId('candle-count')
    FakeEventSource.instances.at(-1)!.emit('tick', tick({ symbol: 'TCS.NS', tick_time: new Date().toISOString() }))
    expect(screen.getByTestId('live-state')).toHaveTextContent('CLOSED')
  })

  test('search picks an instrument and reloads quote, bars and stream for it', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    await waitFor(() => expect(screen.getByTestId('quote-price')).toHaveTextContent('₹'))

    await userEvent.type(screen.getByLabelText(/search stocks/i), 'infy')
    const results = await screen.findByRole('list', { name: 'Search results' })
    await userEvent.click(await within(results).findByRole('button', { name: /INFY\.NS/ }))

    expect(await screen.findByText('INFY.NS', { selector: '.eyebrow' })).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/quote?symbol=INFY.NS'), expect.anything())
    expect(FakeEventSource.instances[0]?.closed).toBe(true)
    expect(FakeEventSource.instances.at(-1)?.url).toContain('symbols=INFY.NS')
    expect(screen.getByText(/TradingView live chart \(NSE:INFY\)/)).toBeInTheDocument()
  })

  test('quick picks and range buttons are toggle buttons', async () => {
    const fetchMock = mockApi()
    renderMarkets()
    await screen.findByTestId('candle-count')

    await userEvent.click(screen.getByRole('button', { name: '5y' }))
    expect(screen.getByRole('button', { name: '5y' })).toHaveAttribute('aria-pressed', 'true')
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('range=5y'), expect.anything())

    await userEvent.click(screen.getByRole('button', { name: 'Reliance' }))
    expect(screen.getByRole('button', { name: 'Reliance' })).toHaveAttribute('aria-pressed', 'true')
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('symbol=RELIANCE.NS'), expect.anything())
  })

  test('market-open quote without ticks says DELAYED', async () => {
    mockApi({ '/api/v1/quote': () => Response.json(quoteResponse({ market_open: true })) })
    renderMarkets()
    expect(await screen.findByText('DELAYED')).toBeInTheDocument()
    expect(screen.getByText(/polling Yahoo every 15 s/)).toBeInTheDocument()
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

  test('empty search results say so', async () => {
    mockApi({ '/api/v1/instruments/search': () => Response.json({ results: [] }) })
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
    // GARCH-t is preferred when it was fitted; GBM is one click away.
    expect(screen.getByRole('button', { name: 'GARCH-t' })).toHaveAttribute('aria-pressed', 'true')
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
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/analyze?symbol=%5EGSPC&range=5y'), expect.anything())
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
