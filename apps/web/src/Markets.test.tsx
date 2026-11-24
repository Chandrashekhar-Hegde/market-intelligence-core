import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import type { BarsResponse, InstrumentMeta, QuoteResponse, Tick } from './api'
import { Markets } from './Markets'
import {
  displayPrice,
  liveState,
  mergeTick,
  toCandles,
  toTradingViewSymbol,
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
    market_open: false,
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

type Routes = Record<string, () => Response>

function mockApi(routes: Routes = {}) {
  const defaults: Routes = {
    '/api/v1/quote': () => Response.json(quoteResponse()),
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
    return all[url.pathname]?.() ?? new Response('not found', { status: 404 })
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

describe('toTradingViewSymbol', () => {
  test.each([
    ['RELIANCE.NS', 'NSE:RELIANCE'],
    ['reliance.bo', 'BSE:RELIANCE'],
    ['M&M.NS', 'NSE:M_M'],
    ['BAJAJ-AUTO.NS', 'NSE:BAJAJ_AUTO'],
    ['^NSEI', 'NSE:NIFTY'],
    ['^BSESN', 'BSE:SENSEX'],
    ['^NSEBANK', 'NSE:BANKNIFTY'],
    ['AAPL', 'AAPL'],
    ['^UNKNOWN', null],
    ['VOD.L', null],
  ])('%s -> %s', (yahoo, tradingView) => {
    expect(toTradingViewSymbol(yahoo)).toBe(tradingView)
  })
})

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
    expect(screen.getByText(/TradingView live chart \(NSE:NIFTY\)/)).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/bars?symbol=%5ENSEI&range=1y'),
      expect.anything(),
    )
    expect(FakeEventSource.instances[0]?.url).toContain('/api/v1/stream?symbols=%5ENSEI')
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

    await userEvent.type(screen.getByLabelText(/search nse \/ bse/i), 'infy')
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
    await userEvent.type(screen.getByLabelText(/search nse \/ bse/i), 'zzzz')
    expect(await screen.findByText(/No NSE\/BSE matches for “zzzz”/)).toBeInTheDocument()
  })
})
