// Pure helpers for the Markets view. Kept out of Markets.tsx so they are unit-testable and the
// component file only exports components.
import type { InstrumentMeta, IntradayPoint, PriceBar, Tick } from './api'

/** The live watchlist: India first, then the global markets an Indian investor watches. */
export const QUICK_PICKS = [
  { symbol: '^NSEI', label: 'NIFTY 50' },
  { symbol: '^BSESN', label: 'Sensex' },
  { symbol: '^NSEBANK', label: 'Bank Nifty' },
  { symbol: 'RELIANCE.NS', label: 'Reliance' },
  { symbol: 'HDFCBANK.NS', label: 'HDFC Bank' },
  { symbol: 'TCS.NS', label: 'TCS' },
  { symbol: 'INFY.NS', label: 'Infosys' },
  { symbol: 'GOLDBEES.NS', label: 'Gold ETF' },
  { symbol: 'INR=X', label: 'USD/INR' },
  { symbol: 'BTC-USD', label: 'Bitcoin' },
  { symbol: '^GSPC', label: 'S&P 500' },
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
        detail: `Live · updated ${Math.max(0, Math.round(age / 1000))} s ago (${formatClock(tick.tick_time)})`,
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
        detail: `Prices from this exchange arrive about ${minutes} min late (BSE is 15 min; NSE is live) · last update ${formatClock(tick.tick_time)}`,
      }
    }
  }
  const lastTrade = meta.last_trade_at ? formatClock(meta.last_trade_at, true) : 'unknown'
  if (meta.market_open) {
    return {
      kind: 'delayed',
      label: 'DELAYED',
      detail: `Market open, but no live prices are coming in; checking every 15 s (can be up to 15 min old) · last trade ${lastTrade}`,
    }
  }
  return {
    kind: 'closed',
    label: 'CLOSED',
    detail: `Market closed (weekend, holiday or after hours) · last price from ${lastTrade}`,
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

/** Heatmap cell background for a correlation: teal for positive, red for negative. Alpha is
 * capped so dark text stays readable (WCAG AA) on every cell. */
export function correlationColor(value: number): string {
  if (!Number.isFinite(value)) return 'transparent'
  const alpha = Math.min(Math.abs(value), 1) * 0.5
  return value >= 0 ? `rgba(31, 122, 104, ${alpha.toFixed(3)})` : `rgba(180, 68, 58, ${alpha.toFixed(3)})`
}

/** Distinct, colour-blind-friendlier series colours (Okabe–Ito based). */
export const SERIES_COLORS = ['#0072B2', '#D55E00', '#009E73', '#CC79A7', '#E69F00', '#56B4E9', '#202423', '#8C510A', '#7B3294', '#1B7837']

/**
 * Fold a live tick into intraday bars of `intervalSec`: extend the current bar, or open the next
 * one. Ticks older than the last bar change nothing.
 */
export function appendTick(points: IntradayPoint[], tick: Tick | null, intervalSec: number): IntradayPoint[] {
  const last = points.at(-1)
  if (!tick || !last) return points
  const t = Math.floor(Date.parse(tick.tick_time) / 1000)
  if (t < last.t) return points
  if (t < last.t + intervalSec) {
    return [
      ...points.slice(0, -1),
      { ...last, high: Math.max(last.high, tick.price), low: Math.min(last.low, tick.price), close: tick.price },
    ]
  }
  const start = t - (t % intervalSec)
  return [...points, { t: start, open: tick.price, high: tick.price, low: tick.price, close: tick.price, volume: 0 }]
}

/** Indian-style short numbers: 54 L, 1.2 Cr. */
export function formatCompact(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—'
  return new Intl.NumberFormat('en-IN', { notation: 'compact', maximumFractionDigits: 1 }).format(value)
}

/** Indices are points, not money: no currency sign. */
export function priceCurrency(meta: InstrumentMeta | undefined): string | null {
  return meta?.instrument_type === 'INDEX' ? null : (meta?.currency ?? null)
}

/** Day change: the live tick if there is one, else what the polled quote says. */
export function dayChange(meta: InstrumentMeta | undefined, tick: Tick | null): { change: number; pct: number } | null {
  if (tick) return { change: tick.change, pct: tick.change_pct }
  if (meta?.change === null || meta?.change === undefined || meta.change_pct === null) return null
  return { change: meta.change, pct: meta.change_pct }
}

export function formatSigned(value: number, digits = 2): string {
  return `${value > 0 ? '+' : ''}${value.toFixed(digits)}`
}

/** Clock time in IST for chart axes (lightweight-charts works in UTC seconds). */
export function formatIstTime(unixSeconds: number, withDate = false): string {
  return new Intl.DateTimeFormat('en-IN', {
    timeZone: 'Asia/Kolkata',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
    ...(withDate ? { day: 'numeric', month: 'short' } : {}),
  }).format(new Date(unixSeconds * 1000))
}

/** "5 min ago", "3 h ago", "2 days ago". */
export function timeAgo(iso: string, now: number): string {
  const minutes = Math.max(0, Math.round((now - Date.parse(iso)) / 60_000))
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  return hours < 48 ? `${hours} h ago` : `${Math.round(hours / 24)} days ago`
}
