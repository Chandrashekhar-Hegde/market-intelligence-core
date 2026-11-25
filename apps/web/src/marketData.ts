// Pure helpers for the Markets view. Kept out of Markets.tsx so they are unit-testable and the
// component file only exports components.
import type { InstrumentMeta, PriceBar, Tick } from './api'

export const QUICK_PICKS = [
  { symbol: '^GSPC', label: 'S&P 500' },
  { symbol: '^NSEI', label: 'NIFTY 50' },
  { symbol: 'AAPL', label: 'Apple' },
  { symbol: 'MSFT', label: 'Microsoft' },
  { symbol: 'RELIANCE.NS', label: 'Reliance' },
  { symbol: '7203.T', label: 'Toyota' },
  { symbol: 'SAP.DE', label: 'SAP' },
  { symbol: 'BTC-USD', label: 'Bitcoin' },
] as const

/** A tick older than this is not "live" (exchange time vs this browser's clock). */
export const LIVE_TICK_MAX_AGE_MS = 120_000
/**
 * A streamed tick up to this old during market hours is an exchange-delayed feed, not a stale
 * one. Measured 2026-11-25: Yahoo streams NSE at ~2 s and BSE at exactly 15 min (#44).
 */
export const DELAYED_FEED_MAX_AGE_MS = 25 * 60_000

export type LiveState = {
  kind: 'live' | 'delayed' | 'closed' | 'loading'
  label: string
  detail: string
}

export function liveState(
  meta: InstrumentMeta | undefined,
  tick: Tick | null,
  now: number,
): LiveState {
  if (tick) {
    const age = now - Date.parse(tick.tick_time)
    if (age < LIVE_TICK_MAX_AGE_MS) {
      return {
        kind: 'live',
        label: 'LIVE',
        detail: `Tick ${formatClock(tick.tick_time)} · lag ${Math.max(0, Math.round(age / 1000))}s · Yahoo stream`,
      }
    }
  }
  if (!meta) return { kind: 'loading', label: 'LOADING', detail: 'Fetching quote' }
  if (tick && meta.market_open) {
    const age = now - Date.parse(tick.tick_time)
    if (age < DELAYED_FEED_MAX_AGE_MS) {
      const minutes = Math.max(1, Math.round(age / 60_000))
      return {
        kind: 'delayed',
        label: `DELAYED ${minutes} MIN`,
        detail: `Streaming, but this exchange's feed is delayed ~${minutes} min on Yahoo (BSE is 15 min; NSE is live) · tick ${formatClock(tick.tick_time)}`,
      }
    }
  }
  const lastTrade = meta.last_trade_at ? formatClock(meta.last_trade_at, true) : 'unknown'
  if (meta.market_open) {
    return {
      kind: 'delayed',
      label: 'DELAYED',
      detail: `No live ticks right now; polling Yahoo every 15 s (may lag up to 15 min) · last trade ${lastTrade}`,
    }
  }
  return {
    kind: 'closed',
    label: 'CLOSED',
    detail: `Market closed (weekend, holiday or after hours) · last trade ${lastTrade}`,
  }
}

/** The price to show: the tick if it is newer than the polled quote. */
export function displayPrice(meta: InstrumentMeta | undefined, tick: Tick | null): number | null {
  if (tick && (!meta?.last_trade_at || Date.parse(tick.tick_time) >= Date.parse(meta.last_trade_at))) {
    return tick.price
  }
  return meta?.last_price ?? null
}

export type Candle = { time: string; open: number; high: number; low: number; close: number }

/** Exchange-local calendar date (YYYY-MM-DD) of an instant. */
export function exchangeDate(iso: string, timeZone: string): string {
  return new Intl.DateTimeFormat('en-CA', { timeZone }).format(new Date(iso))
}

export function toCandles(bars: PriceBar[]): Candle[] {
  return bars.map(({ date, open, high, low, close }) => ({ time: date, open, high, low, close }))
}

/**
 * Fold a live tick into the daily candles: extend today's candle, or open a new one when the
 * tick belongs to a session after the last completed bar. Older ticks change nothing.
 */
export function mergeTick(candles: Candle[], tick: Tick | null, timeZone: string): Candle[] {
  const last = candles.at(-1)
  if (!tick || !last) return candles
  const day = exchangeDate(tick.tick_time, timeZone)
  if (day < last.time) return candles
  if (day === last.time) {
    return [
      ...candles.slice(0, -1),
      { ...last, high: Math.max(last.high, tick.price), low: Math.min(last.low, tick.price), close: tick.price },
    ]
  }
  return [...candles, { time: day, open: tick.price, high: tick.price, low: tick.price, close: tick.price }]
}

export function formatClock(iso: string, withDate = false): string {
  const options: Intl.DateTimeFormatOptions = {
    timeZone: 'Asia/Kolkata',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
    ...(withDate ? { weekday: 'short', day: 'numeric', month: 'short' } : {}),
  }
  return `${new Intl.DateTimeFormat('en-IN', options).format(new Date(iso))} IST`
}

export function formatPrice(value: number | null | undefined, currency: string | null | undefined): string {
  if (value === null || value === undefined) return '—'
  const digits = value >= 1000 ? 2 : value >= 1 ? 2 : 4
  const formatted = value.toLocaleString('en-IN', { minimumFractionDigits: digits, maximumFractionDigits: digits })
  return currency === 'INR' ? `₹${formatted}` : currency ? `${formatted} ${currency}` : formatted
}

/**
 * Calendar dates for the next `days` sessions after `lastDate`. Exchanges skip weekends;
 * series that trade every day (crypto, ~365 periods a year) don't. Holidays are ignored:
 * the fan is a distribution over sessions, and the dates only place it on the chart.
 */
export function futureSessions(lastDate: string, days: number, periodsPerYear: number): string[] {
  const everyDay = periodsPerYear > 300
  const out: string[] = []
  const d = new Date(`${lastDate}T00:00:00Z`)
  while (out.length < days) {
    d.setUTCDate(d.getUTCDate() + 1)
    const weekday = d.getUTCDay()
    if (everyDay || (weekday !== 0 && weekday !== 6)) out.push(d.toISOString().slice(0, 10))
  }
  return out
}

export type FanLines = { time: string; value: number }[][]

/** Five quantile lines (5/25/50/75/95%), each starting at the last close so they join the history. */
export function fanLines(
  fan: { day: number; price: number[] }[],
  lastDate: string,
  lastPrice: number,
  periodsPerYear: number,
): FanLines {
  const dates = futureSessions(lastDate, fan.length, periodsPerYear)
  return [0, 1, 2, 3, 4].map((q) => [
    { time: lastDate, value: lastPrice },
    ...fan.map((point, i) => ({ time: dates[i] ?? lastDate, value: point.price[q] ?? lastPrice })),
  ])
}

export function formatPct(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—'
  return `${(value * 100).toFixed(digits)}%`
}

export function formatP(p: number): string {
  return p < 0.001 ? '< 0.001' : p.toFixed(3)
}
