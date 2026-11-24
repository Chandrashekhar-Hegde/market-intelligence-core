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
  market_open: false,
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
          { symbol: 'RELIANCE.NS', name: 'Reliance Industries Limited', exchange: 'NSE', quote_type: 'EQUITY' },
          { symbol: 'RELIANCE.BO', name: 'Reliance Industries Limited', exchange: 'Bombay', quote_type: 'EQUITY' },
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
  await page.route('**/api/v1/stream**', (route) => {
    const symbols = new URL(route.request().url()).searchParams.get('symbols') ?? ''
    const body =
      liveTicks && symbols.includes(LIVE_TICK.symbol)
        ? `event: tick\ndata: ${JSON.stringify({ ...LIVE_TICK, tick_time: new Date().toISOString() })}\n\n`
        : ': keep-alive\n\n'
    return route.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body })
  })
  // Never load the real third-party widget in tests.
  await page.route('https://s3.tradingview.com/**', (route) =>
    route.fulfill({ contentType: 'text/javascript', body: '' }),
  )
}

/** The fixture console lives behind the "Event lab" tab. */
export async function openEventLab(page: Page) {
  await page.getByRole('tab', { name: /event lab/i }).click()
}
