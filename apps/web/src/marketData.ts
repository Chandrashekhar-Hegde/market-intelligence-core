// Pure helpers for the Markets view. Kept out of Markets.tsx so they are unit-testable and the
// component file only exports components.
import type { InstrumentMeta, PriceBar, Tick } from './api'

export const QUICK_PICKS = [
  { symbol: '^NSEI', label: 'NIFTY 50' },
  { symbol: '^BSESN', label: 'SENSEX' },
  { symbol: '^NSEBANK', label: 'BANK NIFTY' },
  { symbol: 'RELIANCE.NS', label: 'Reliance' },
  { symbol: 'TCS.NS', label: 'TCS' },
  { symbol: 'HDFCBANK.NS', label: 'HDFC Bank' },
  { symbol: 'INFY.NS', label: 'Infosys' },
] as const

/** A tick older than this is not "live" (exchange time vs this browser's clock). */
export const LIVE_TICK_MAX_AGE_MS = 120_000

const TRADINGVIEW_INDICES: Record<string, string> = {
  '^NSEI': 'NSE:NIFTY',
  '^BSESN': 'BSE:SENSEX',
  '^NSEBANK': 'NSE:BANKNIFTY',
  '^CNXIT': 'NSE:CNXIT',
}

/** Yahoo symbol -> TradingView symbol, or null when we don't know the mapping. */
export function toTradingViewSymbol(symbol: string): string | null {
  const s = symbol.trim().toUpperCase()
  if (s.startsWith('^')) return TRADINGVIEW_INDICES[s] ?? null
  // TradingView spells M&M and BAJAJ-AUTO as M_M and BAJAJ_AUTO.
  const code = (yahooCode: string) => yahooCode.replace(/[&-]/g, '_')
  if (s.endsWith('.NS')) return `NSE:${code(s.slice(0, -3))}`
  if (s.endsWith('.BO')) return `BSE:${code(s.slice(0, -3))}`
  // ponytail: other suffixes (.L, .T, ...) need an exchange table; add one with non-Indian markets.
  return s.includes('.') ? null : s
}

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
