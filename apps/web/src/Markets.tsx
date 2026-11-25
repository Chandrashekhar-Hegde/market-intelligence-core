import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, Search } from 'lucide-react'
import { CandlestickSeries, createChart, type ISeriesApi } from 'lightweight-charts'

import {
  BAR_RANGES,
  type BarRange,
  type BarsResponse,
  type QualityReport,
  type QuoteResponse,
  type SearchHit,
  type Tick,
  describeApiError,
  fetchBars,
  fetchQuote,
  searchInstruments,
  streamUrl,
} from './api'
import {
  QUICK_PICKS,
  type Candle,
  displayPrice,
  formatClock,
  formatPrice,
  liveState,
  mergeTick,
  toCandles,
} from './marketData'
import { AnalysisPanels } from './Analysis'

const isDomTest = typeof navigator !== 'undefined' && navigator.userAgent.includes('jsdom')

export function Markets() {
  const [symbol, setSymbol] = useState<string>('^GSPC')
  const [range, setRange] = useState<BarRange>('1y')
  const now = useNow(5_000)

  const quote = useQuery<QuoteResponse>({
    queryKey: ['quote', symbol],
    queryFn: () => fetchQuote(symbol),
    // Poll fast only while the exchange is in session; the stream carries the live price.
    refetchInterval: (query) => (query.state.data?.quote.market_open ? 15_000 : 300_000),
    retry: retryUnlessClientError,
  })
  const bars = useQuery<BarsResponse>({
    queryKey: ['bars', symbol, range],
    queryFn: () => fetchBars(symbol, range),
    retry: retryUnlessClientError,
  })
  const tick = useLiveTick(symbol)

  const meta = quote.data?.quote
  const state = liveState(meta, tick, now)
  const price = displayPrice(meta, tick)
  const name = meta?.name ?? bars.data?.chart.meta.name ?? symbol

  return (
    <>
      <header className="topbar">
        <div>
          <p className="eyebrow">Markets · any exchange</p>
          <h2>Live prices, history and analysis</h2>
        </div>
      </header>

      <section className="panel markets-search-panel" aria-labelledby="search-heading">
        <h3 id="search-heading" className="visually-hidden">
          Find an instrument
        </h3>
        <InstrumentSearch onPick={setSymbol} />
        <div className="quick-picks" role="group" aria-label="Quick picks">
          {QUICK_PICKS.map((pick) => (
            <button
              key={pick.symbol}
              type="button"
              className="ghost-button"
              aria-pressed={pick.symbol === symbol}
              onClick={() => setSymbol(pick.symbol)}
            >
              {pick.label}
            </button>
          ))}
        </div>
      </section>

      <section className="panel quote-panel" aria-labelledby="quote-heading" aria-live="polite">
        <div className="section-heading">
          <div>
            <p className="eyebrow">{symbol}</p>
            <h3 id="quote-heading">{name}</h3>
          </div>
          <span className={`status-chip tone-${toneFor(state.kind)}`} data-testid="live-state">
            {state.label}
          </span>
        </div>
        {quote.error ? (
          <p className="error-state" role="alert">
            {describeApiError(quote.error)}
          </p>
        ) : (
          <>
            <p className="quote-price" data-testid="quote-price">
              {formatPrice(price, meta?.currency)}
              {tick ? <ChangeBadge change={tick.change} pct={tick.change_pct} /> : null}
            </p>
            <p className="quiet-copy">{state.detail}</p>
            {tick && (tick.day_low !== null || tick.day_high !== null) ? (
              <p className="quiet-copy">
                Day range {formatPrice(tick.day_low, meta?.currency)} – {formatPrice(tick.day_high, meta?.currency)}
                {tick.day_volume ? ` · volume ${tick.day_volume.toLocaleString('en-IN')}` : ''}
              </p>
            ) : null}
            {quote.data ? (
              <p className="quiet-copy mono">
                {meta?.exchange ?? 'exchange unknown'} · {quote.data.source} · fetched{' '}
                {formatClock(quote.data.fetched_at)}
              </p>
            ) : null}
          </>
        )}
      </section>

      <section className="panel chart-panel" aria-labelledby="history-heading">
        <div className="section-heading">
          <div>
            <p className="eyebrow">Daily candles · split-adjusted</p>
            <h3 id="history-heading">Price history</h3>
          </div>
          <div className="range-picker" role="group" aria-label="Range">
            {BAR_RANGES.map((option) => (
              <button
                key={option}
                type="button"
                className="ghost-button"
                aria-pressed={option === range}
                onClick={() => setRange(option)}
              >
                {option}
              </button>
            ))}
          </div>
        </div>
        {bars.error ? (
          <p className="error-state" role="alert">
            {describeApiError(bars.error)}
          </p>
        ) : bars.data ? (
          <>
            <CandleChart
              bars={bars.data}
              tick={tick}
              label={`${name} daily candles, ${bars.data.chart.bars.length} sessions`}
            />
            <p className="quiet-copy mono">
              {bars.data.chart.bars.length} sessions · {bars.data.chart.source} · fetched{' '}
              {formatClock(bars.data.chart.fetched_at)}
              {bars.data.chart.dropped_partial_bar ? ' · today’s forming bar excluded from history' : ''}
            </p>
          </>
        ) : (
          <p className="empty-state">Loading history…</p>
        )}
      </section>

      {bars.data ? (
        <>
          <QualityPanel report={bars.data.quality} calendar={bars.data.calendar_reference} />
          <CorporateActionsPanel bars={bars.data} />
        </>
      ) : null}

      <AnalysisPanels symbol={symbol} history={bars.data?.chart.bars} />

      <section className="panel chart-panel" aria-labelledby="tv-heading">
        <h3 id="tv-heading" className="visually-hidden">
          TradingView chart
        </h3>
        <TradingViewWidget tvSymbol={meta?.tradingview_symbol ?? null} />
      </section>
    </>
  )
}

/** 4xx (bad symbol, no access) won't fix itself; upstream 5xx often does. */
function retryUnlessClientError(failureCount: number, error: Error): boolean {
  return failureCount < 2 && !/^4\d\d/.test(error.message)
}

function toneFor(kind: string): string {
  return { live: 'green', delayed: 'amber', closed: 'neutral', loading: 'blue' }[kind] ?? 'neutral'
}

function ChangeBadge({ change, pct }: { change: number; pct: number }) {
  const sign = change > 0 ? '+' : ''
  return (
    <span className={change >= 0 ? 'utility-positive' : 'utility-negative'}>
      {' '}
      {sign}
      {change.toFixed(2)} ({sign}
      {pct.toFixed(2)}%)
    </span>
  )
}

export function InstrumentSearch({
  onPick,
  id = 'instrument-search',
  label = 'Search stocks, ETFs, indices, crypto and FX',
}: {
  onPick: (symbol: string) => void
  id?: string
  label?: string
}) {
  const [text, setText] = useState('')
  const query = useDebounced(text.trim(), 300)
  const hits = useQuery<SearchHit[]>({
    queryKey: ['search', query],
    queryFn: () => searchInstruments(query),
    enabled: query.length >= 2,
    staleTime: 300_000,
  })

  return (
    <div className="instrument-search">
      <label htmlFor={id}>
        <Search size={16} aria-hidden="true" /> {label}
      </label>
      <input
        id={id}
        type="search"
        autoComplete="off"
        placeholder="e.g. apple, toyota, infosys, bitcoin"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      {query.length >= 2 ? (
        hits.error ? (
          <p className="error-state" role="alert">
            {describeApiError(hits.error)}
          </p>
        ) : hits.data && hits.data.length === 0 ? (
          <p className="quiet-copy">No matches for “{query}”.</p>
        ) : (
          <ul className="search-results" aria-label="Search results">
            {(hits.data ?? []).map((hit) => (
              <li key={hit.symbol}>
                <button
                  type="button"
                  className="event-title-button"
                  onClick={() => {
                    onPick(hit.symbol)
                    setText('')
                  }}
                >
                  <strong>{hit.symbol}</strong> {hit.name}{' '}
                  <span className="quiet-copy">
                    {hit.exchange} · {hit.quote_type}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )
      ) : null}
    </div>
  )
}

function CandleChart({ bars, tick, label }: { bars: BarsResponse; tick: Tick | null; label: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const seriesRef = useRef<ISeriesApi<'Candlestick'> | null>(null)
  const timeZone = bars.chart.meta.timezone ?? 'Asia/Kolkata'
  const base = useMemo(() => toCandles(bars.chart.bars), [bars])
  const candles: Candle[] = useMemo(() => mergeTick(base, tick, timeZone), [base, tick, timeZone])

  useEffect(() => {
    if (!containerRef.current || isDomTest) return undefined
    const chart = createChart(containerRef.current, {
      autoSize: true,
      height: 340,
      layout: { background: { color: '#ffffff' }, textColor: '#3d4242' },
      grid: { vertLines: { color: '#eef0ef' }, horzLines: { color: '#eef0ef' } },
      rightPriceScale: { borderColor: '#d7dcda' },
      timeScale: { borderColor: '#d7dcda' },
    })
    seriesRef.current = chart.addSeries(CandlestickSeries, {
      upColor: '#1f7a68',
      downColor: '#b4443a',
      borderVisible: false,
      wickUpColor: '#1f7a68',
      wickDownColor: '#b4443a',
    })
    return () => {
      seriesRef.current = null
      chart.remove()
    }
  }, [])

  // Full redraw when the history changes; ticks only touch the last candle.
  useEffect(() => {
    seriesRef.current?.setData(base)
  }, [base])
  useEffect(() => {
    const last = candles.at(-1)
    if (tick && last) seriesRef.current?.update(last)
  }, [candles, tick])

  const last = candles.at(-1)
  return (
    <>
      <div className="chart-accessible-summary" role="img" aria-label={label}>
        {last ? `Latest ${last.time}: open ${last.open}, high ${last.high}, low ${last.low}, close ${last.close}.` : 'No bars.'}
      </div>
      {isDomTest ? (
        <div className="chart-fallback" data-testid="candle-count">
          {candles.length}
        </div>
      ) : (
        <div className="impact-chart candle-chart" ref={containerRef} data-testid="candle-chart" />
      )}
    </>
  )
}

function QualityPanel({ report, calendar }: { report: QualityReport; calendar: string | null }) {
  return (
    <section className="panel" aria-labelledby="quality-heading">
      <div className="section-heading">
        <div>
          <p className="eyebrow">Data quality</p>
          <h3 id="quality-heading">{report.passed ? 'Checks passed' : 'Data issues found'}</h3>
        </div>
        <span className={`status-chip tone-${report.passed ? 'green' : 'red'}`} data-testid="quality-badge">
          {report.passed ? (
            <CheckCircle2 size={14} aria-hidden="true" />
          ) : (
            <AlertTriangle size={14} aria-hidden="true" />
          )}{' '}
          {report.passed ? 'Usable' : 'Not usable'}
        </span>
      </div>
      <ul className="plain-list">
        <li>
          {report.n_bars} sessions{report.first_date ? `, ${report.first_date} → ${report.last_date}` : ''}
        </li>
        <li>{report.is_stale ? 'Stale: the market traded after the last bar' : 'Up to date with the exchange calendar'}</li>
        <li>
          {calendar
            ? `${report.missing_days.length} missing trading days (vs the ${calendar} calendar)`
            : 'No home-market index calendar; gaps not checked'}
        </li>
        <li>
          {report.ohlc_violations.length + report.non_positive_prices.length} impossible prices
        </li>
        {report.warnings.map((warning) => (
          <li key={warning}>
            <AlertTriangle size={14} aria-hidden="true" /> {warning}
          </li>
        ))}
      </ul>
      {report.outliers.length > 0 ? (
        <table className="event-table data-table">
          <caption className="quiet-copy">Extreme daily returns (robust z above 8): check for news or bad prints</caption>
          <thead>
            <tr>
              <th scope="col">Date</th>
              <th scope="col">Return</th>
              <th scope="col">Robust z</th>
              <th scope="col">Near split/dividend</th>
            </tr>
          </thead>
          <tbody>
            {report.outliers.map((o) => (
              <tr key={o.date}>
                <td>{o.date}</td>
                <td>{((Math.exp(o.log_return) - 1) * 100).toFixed(1)}%</td>
                <td>{o.robust_z.toFixed(1)}</td>
                <td>{o.near_corporate_action ? 'yes' : 'no'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  )
}

function CorporateActionsPanel({ bars }: { bars: BarsResponse }) {
  const actions = bars.chart.corporate_actions
  return (
    <section className="panel" aria-labelledby="actions-heading">
      <p className="eyebrow">Corporate actions in range</p>
      <h3 id="actions-heading">Splits and dividends</h3>
      {actions.length === 0 ? (
        <p className="quiet-copy">None in this range.</p>
      ) : (
        <ul className="plain-list">
          {actions.map((action) => (
            <li key={`${action.kind}-${action.date}`}>
              {action.date} ·{' '}
              {action.kind === 'dividend'
                ? `Dividend ${formatPrice(action.amount, bars.chart.meta.currency)}`
                : `Split ${action.numerator}:${action.denominator}`}
            </li>
          ))}
        </ul>
      )}
      <p className="quiet-copy">History is adjusted for these, so returns across ex-dates are total returns.</p>
    </section>
  )
}

const TRADINGVIEW_SCRIPT = 'https://s3.tradingview.com/external-embedding/embed-widget-advanced-chart.js'

function TradingViewWidget({ tvSymbol }: { tvSymbol: string | null }) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const [open, setOpen] = useState(false)

  useEffect(() => {
    const container = containerRef.current
    if (!open || !container || !tvSymbol || isDomTest) return undefined
    const widget = document.createElement('div')
    widget.className = 'tradingview-widget-container__widget'
    const script = document.createElement('script')
    script.src = TRADINGVIEW_SCRIPT
    script.async = true
    script.textContent = JSON.stringify({
      symbol: tvSymbol,
      interval: '5',
      timezone: 'Asia/Kolkata',
      theme: 'light',
      style: '1',
      locale: 'en',
      allow_symbol_change: false,
      autosize: true,
      support_host: 'https://www.tradingview.com',
    })
    container.replaceChildren(widget, script)
    return () => container.replaceChildren()
  }, [open, tvSymbol])

  return (
    <details className="tradingview" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>
        TradingView live chart {tvSymbol ? `(${tvSymbol})` : '(not available for this symbol)'}
      </summary>
      <div className="tradingview-widget-container" ref={containerRef} data-testid="tradingview" />
      <p className="quiet-copy">
        Chart and data by{' '}
        <a href="https://www.tradingview.com/" target="_blank" rel="noopener noreferrer">
          TradingView
        </a>
        , streamed to your browser only. It is not used in this app’s analysis.
      </p>
    </details>
  )
}

/** Latest tick for `symbol` from the server's SSE stream; null until one arrives. */
function useLiveTick(symbol: string): Tick | null {
  const [latest, setLatest] = useState<Tick | null>(null)
  useEffect(() => {
    if (typeof EventSource === 'undefined') return undefined
    const wanted = symbol.toUpperCase()
    const source = new EventSource(streamUrl([symbol]))
    const onTick = (event: MessageEvent<string>) => {
      const tick = JSON.parse(event.data) as Tick
      if (tick.symbol === wanted) setLatest(tick)
    }
    source.addEventListener('tick', onTick)
    return () => {
      source.close()
      setLatest(null)
    }
  }, [symbol])
  return latest
}

function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), everyMs)
    return () => window.clearInterval(id)
  }, [everyMs])
  return now
}

function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(value), delayMs)
    return () => window.clearTimeout(id)
  }, [value, delayMs])
  return debounced
}
