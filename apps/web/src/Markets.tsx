import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, Search } from 'lucide-react'
import {
  AreaSeries,
  CandlestickSeries,
  createChart,
  LineStyle,
  type IPriceLine,
  type ISeriesApi,
  type UTCTimestamp,
} from 'lightweight-charts'

import {
  CHART_RANGES,
  type BarRange,
  type BarsResponse,
  type ChartRange,
  type IntradayData,
  type IntradayRange,
  type NewsResponse,
  type QualityReport,
  type QuoteResponse,
  type SearchResponse,
  type Tick,
  describeApiError,
  fetchBars,
  fetchIntraday,
  fetchNews,
  fetchQuote,
  isIntraday,
  searchInstruments,
  streamUrl,
  useAnalysis,
  useNow,
} from './api'
import {
  QUICK_PICKS,
  type Candle,
  timeAgo,
  appendTick,
  dayChange,
  displayPrice,
  formatClock,
  formatCompact,
  formatIstTime,
  formatPrice,
  formatSigned,
  liveState,
  mergeTick,
  priceCurrency,
  toCandles,
} from './marketData'
import { AnalysisPanels } from './Analysis'
import { relativeVolume, sessionLeft, sessionRange } from './plainLanguage'

const isDomTest = typeof navigator !== 'undefined' && navigator.userAgent.includes('jsdom')

const RANGE_LABEL: Record<ChartRange, string> = {
  '1d': '1D',
  '5d': '5D',
  '1mo': '1M',
  '6mo': '6M',
  '1y': '1Y',
  '5y': '5Y',
  max: 'All',
}

export function Markets() {
  const [symbol, setSymbol] = useState<string>('^NSEI')
  const [range, setRange] = useState<ChartRange>('1d')
  const now = useNow(5_000)
  // One stream for the watchlist and the selected symbol (the server takes up to 20).
  const streamed = useMemo(
    () => [...new Set([symbol.toUpperCase(), ...QUICK_PICKS.map((pick) => pick.symbol)])],
    [symbol],
  )
  const ticks = useLiveTicks(streamed)
  const tick = ticks[symbol.toUpperCase()] ?? null

  const quote = useQuery<QuoteResponse>({
    queryKey: ['quote', symbol],
    queryFn: () => fetchQuote(symbol),
    // Poll fast only while the exchange is in session; the stream carries the live price.
    refetchInterval: (query) => (query.state.data?.quote.market_open ? 15_000 : 300_000),
    retry: retryUnlessClientError,
  })
  // Daily history also feeds the quality report and the forecast chart, so it loads even
  // while the chart shows today.
  const dailyRange: BarRange = isIntraday(range) ? '1y' : range
  const bars = useQuery<BarsResponse>({
    queryKey: ['bars', symbol, dailyRange],
    queryFn: () => fetchBars(symbol, dailyRange),
    retry: retryUnlessClientError,
  })

  const meta = quote.data?.quote
  const state = liveState(meta, tick, now)
  const price = displayPrice(meta, tick)
  const change = dayChange(meta, tick)
  const currency = priceCurrency(meta)
  const name = meta?.name ?? bars.data?.chart.meta.name ?? symbol

  return (
    <>
      <header className="topbar">
        <div>
          <p className="eyebrow">Markets · NSE, BSE and any exchange</p>
          <h2>Live prices, history and analysis</h2>
        </div>
      </header>

      <Watchlist ticks={ticks} selected={symbol} onPick={setSymbol} />

      <section className="panel markets-search-panel" aria-labelledby="search-heading">
        <h3 id="search-heading" className="visually-hidden">
          Find an instrument
        </h3>
        <InstrumentSearch onPick={setSymbol} />
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
              {formatPrice(price, currency)}
              {change ? <ChangeBadge change={change.change} pct={change.pct} /> : null}
            </p>
            <p className="quiet-copy">{state.detail}</p>
            {tick && (tick.day_low !== null || tick.day_high !== null) ? (
              <p className="quiet-copy">
                Today’s range {formatPrice(tick.day_low, currency)} – {formatPrice(tick.day_high, currency)}
                {tick.day_volume ? ` · traded ${formatCompact(tick.day_volume)}` : ''}
              </p>
            ) : meta?.day_volume ? (
              <p className="quiet-copy">Traded {formatCompact(meta.day_volume)} in the latest session</p>
            ) : null}
            <LiveOutlook
              symbol={symbol}
              price={price}
              currency={currency}
              marketOpen={meta?.market_open ?? false}
              volumeToday={tick?.day_volume ?? meta?.day_volume ?? null}
              bars={bars.data}
              now={now}
            />
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
            <p className="eyebrow">
              {isIntraday(range) ? 'Minute by minute · moves with live prices' : 'Daily candles · split-adjusted'}
            </p>
            <h3 id="history-heading">{range === '1d' ? 'Today' : 'Price history'}</h3>
          </div>
          <div className="range-picker" role="group" aria-label="Range">
            {CHART_RANGES.map((option) => (
              <button
                key={option}
                type="button"
                className="ghost-button"
                aria-pressed={option === range}
                onClick={() => setRange(option)}
              >
                {RANGE_LABEL[option]}
              </button>
            ))}
          </div>
        </div>
        {isIntraday(range) ? (
          <IntradayPanel symbol={symbol} range={range} tick={tick} name={name} />
        ) : bars.error ? (
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

      {bars.error && isIntraday(range) ? (
        <section className="panel" aria-label="Daily history">
          <p className="error-state" role="alert">
            Daily history: {describeApiError(bars.error)}
          </p>
        </section>
      ) : null}

      <AnalysisPanels
        symbol={symbol}
        history={bars.data?.chart.bars}
        livePrice={tick ? price : null}
        extraMaths={
          bars.data ? (
            <>
              <QualityPanel report={bars.data.quality} calendar={bars.data.calendar_reference} />
              <CorporateActionsPanel bars={bars.data} />
            </>
          ) : null
        }
      />

      <NewsPanel symbol={symbol} />

      <section className="panel chart-panel" aria-labelledby="tv-heading">
        <h3 id="tv-heading" className="visually-hidden">
          TradingView chart
        </h3>
        <TradingViewWidget tvSymbol={meta?.tradingview_symbol ?? null} />
      </section>
    </>
  )
}

/**
 * Where the price is likely to be by the close (or by tomorrow's close when the market is shut),
 * recomputed on every tick, and how busy trading is compared with a normal day.
 */
function LiveOutlook({
  symbol,
  price,
  currency,
  marketOpen,
  volumeToday,
  bars,
  now,
}: {
  symbol: string
  price: number | null
  currency: string | null
  marketOpen: boolean
  volumeToday: number | null
  bars: BarsResponse | undefined
  now: number
}) {
  const analysis = useAnalysis(symbol)
  const intraday = useQuery<{ chart: IntradayData }>({
    queryKey: ['intraday', symbol, '1d'],
    queryFn: () => fetchIntraday(symbol, '1d'),
    retry: retryUnlessClientError,
  })
  if (!analysis.data || price === null) return null
  const chart = intraday.data?.chart
  const left = marketOpen ? sessionLeft(chart?.session_start ?? null, chart?.session_end ?? null, now / 1000) : null
  const today = left !== null && left > 0 ? sessionRange(analysis.data, price, left) : null
  const next = sessionRange(analysis.data, price, (today && left !== null ? left : 0) + 1)
  const recent = bars?.chart.bars.slice(-20) ?? []
  const avgVolume = recent.length ? recent.reduce((sum, b) => sum + b.volume, 0) / recent.length : 0
  const relVol =
    marketOpen && left !== null && volumeToday !== null ? relativeVolume(volumeToday, avgVolume, 1 - left) : null
  return (
    <ul className="plain-list live-outlook" data-testid="live-outlook">
      {today ? (
        <li>
          Likely close today (9 in 10): <strong>{formatPrice(today[0], currency)}</strong> to{' '}
          <strong>{formatPrice(today[1], currency)}</strong>
        </li>
      ) : null}
      {next ? (
        <li>
          {today ? 'Likely close tomorrow' : 'Likely range by the next close'} (9 in 10):{' '}
          {formatPrice(next[0], currency)} to {formatPrice(next[1], currency)}
        </li>
      ) : null}
      {relVol !== null ? (
        <li>
          Trading is {relVol >= 1.5 ? 'busier than usual' : relVol <= 0.6 ? 'quieter than usual' : 'about normal'}:{' '}
          {relVol.toFixed(1)}× a typical day’s volume so far
        </li>
      ) : null}
    </ul>
  )
}


const KIND_LABEL: Record<string, string> = {
  results: 'Results',
  order_win: 'Order win',
  guidance: 'Outlook',
  rating: 'Broker view',
  legal: 'Legal / regulator',
  credit: 'Debt',
  management: 'Management',
  stake: 'Big holders',
  payout: 'Dividend / buyback',
  fundraise: 'Fund-raising',
  merger: 'Deal',
  approval: 'Approval',
  general: 'General',
}

/** Headlines that mention the instrument, and how the price moved after each beyond the market. */
function NewsPanel({ symbol }: { symbol: string }) {
  const now = useNow(60_000)
  const news = useQuery<NewsResponse>({
    queryKey: ['news', symbol],
    queryFn: () => fetchNews(symbol),
    staleTime: 10 * 60_000,
    // GDELT allows one request every 5 s; never hammer it with retries.
    retry: false,
  })
  return (
    <section className="panel chart-panel" aria-labelledby="news-heading" data-testid="news">
      <p className="eyebrow">News · last 3 days · Google News, Economic Times, Mint, GDELT</p>
      <h3 id="news-heading">What the news says, and how the price took it</h3>
      {news.error ? (
        <p className="error-state" role="alert">
          News is unavailable right now ({describeApiError(news.error)}). The rest of the page does not depend on it.
        </p>
      ) : !news.data ? (
        <p className="empty-state">Searching the news…</p>
      ) : (
        <>
          <p className="quiet-copy news-sources" data-testid="news-sources">
            Sources:{' '}
            {news.data.sources.map((source, i) => (
              <span key={source.name} title={source.error ?? undefined}>
                {i > 0 ? ' · ' : ''}
                {source.name} {source.ok ? `✓ ${source.count}` : '✗ unavailable'}
              </span>
            ))}
          </p>
          {news.data.articles.length === 0 ? (
            <p className="quiet-copy">No stories naming it in the last 3 days.</p>
          ) : (
            <ul className="news-list">
              {news.data.articles.slice(0, 12).map(({ article, reading, impact }) => (
                <li key={article.url}>
                  <a href={article.url} target="_blank" rel="noopener noreferrer">
                    {article.title}
                  </a>
                  <span className="quiet-copy">
                    {' '}
                    {article.publisher}
                    {article.source !== article.publisher ? ` via ${article.source}` : ''} · {timeAgo(article.seen_at, now)}
                  </span>
                  <p className="news-reading">
                    <span className={`status-chip tone-${reading.direction > 0 ? 'green' : reading.direction < 0 ? 'red' : 'neutral'}`}>
                      {reading.direction > 0 ? 'Good' : reading.direction < 0 ? 'Bad' : 'Unclear'} for the company
                    </span>{' '}
                    <span className="quiet-copy">
                      {KIND_LABEL[reading.kind] ?? reading.kind} · {['', 'minor', 'notable', 'major'][reading.materiality]}
                    </span>{' '}
                    {reading.reason}
                  </p>
                  {impact ? (
                    <p className="news-impact">
                      Since then: {formatSigned((Math.exp(impact.instrument_move) - 1) * 100, 1)}%
                      {impact.market_move !== null && news.data.benchmark
                        ? `, ${formatSigned((Math.exp(impact.abnormal_move) - 1) * 100, 1)}% beyond what ${news.data.benchmark} explains`
                        : ''}
                      .{' '}
                      {Math.abs(impact.z) < 1.96
                        ? 'An ordinary move for this instrument; it could easily be chance.'
                        : `Unusual: a move this size happens about 1 in ${Math.round(impact.one_in)} times without news.`}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
          <p className="quiet-copy">
            Stories are found by searching for “{news.data.query}”, so some may be off-topic. Each headline is read by{' '}
            {news.data.reader === 'built-in' ? 'the app’s built-in news reader' : news.data.reader}. Strong news can change
            the call above; when it does, the call says it has no track record yet.
          </p>
        </>
      )}
    </section>
  )
}

/** Live strip of the markets most people check first; tap one to open it. */
function Watchlist({
  ticks,
  selected,
  onPick,
}: {
  ticks: Record<string, Tick>
  selected: string
  onPick: (symbol: string) => void
}) {
  const quotes = useQueries({
    queries: QUICK_PICKS.map((pick) => ({
      queryKey: ['quote', pick.symbol],
      queryFn: () => fetchQuote(pick.symbol),
      refetchInterval: 60_000,
      retry: retryUnlessClientError,
    })),
  })
  return (
    <section className="watchlist" aria-label="Watchlist">
      {QUICK_PICKS.map((pick, i) => {
        const meta = quotes[i]?.data?.quote
        const tick = ticks[pick.symbol] ?? null
        const price = displayPrice(meta, tick)
        const change = dayChange(meta, tick)
        const tone = !change ? 'flat' : change.pct >= 0 ? 'up' : 'down'
        return (
          <button
            key={pick.symbol}
            type="button"
            className={`watch-item watch-${tone}`}
            aria-pressed={pick.symbol === selected}
            onClick={() => onPick(pick.symbol)}
          >
            <span className="watch-label">{pick.label}</span>
            <span className="watch-price">{formatPrice(price, priceCurrency(meta))}</span>
            <span className="watch-change">{change ? `${formatSigned(change.pct)}%` : '—'}</span>
          </button>
        )
      })}
    </section>
  )
}

const INTERVAL_SECONDS = { '1m': 60, '5m': 300 } as const

function IntradayPanel({
  symbol,
  range,
  tick,
  name,
}: {
  symbol: string
  range: IntradayRange
  tick: Tick | null
  name: string
}) {
  const intraday = useQuery<{ chart: IntradayData }>({
    queryKey: ['intraday', symbol, range],
    queryFn: () => fetchIntraday(symbol, range),
    // Refill from the server each minute while trading; ticks fill in between.
    refetchInterval: (query) => (query.state.data?.chart.meta.market_open ? 60_000 : 300_000),
    retry: retryUnlessClientError,
  })
  if (intraday.error) {
    return (
      <p className="error-state" role="alert">
        {describeApiError(intraday.error)}
      </p>
    )
  }
  if (!intraday.data) return <p className="empty-state">Loading today’s prices…</p>
  const chart = intraday.data.chart
  if (chart.points.length === 0) {
    return <p className="empty-state">No trades in this period yet.</p>
  }
  return (
    <>
      <IntradayChart data={chart} tick={tick} label={`${name}, ${range === '1d' ? 'today' : 'last 5 days'}`} />
      <p className="quiet-copy mono">
        {chart.interval === '1m' ? '1-minute' : '5-minute'} bars · times in IST
        {chart.previous_close !== null
          ? ` · dashed line: ${formatPrice(chart.previous_close, null)}, where today’s change is measured from`
          : ''}{' '}
        · {chart.source} · fetched {formatClock(chart.fetched_at)}
      </p>
    </>
  )
}

const UP = '#1f7a68'
const DOWN = '#b4443a'

function IntradayChart({ data, tick, label }: { data: IntradayData; tick: Tick | null; label: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const seriesRef = useRef<ISeriesApi<'Area'> | null>(null)
  const baseLineRef = useRef<IPriceLine | null>(null)
  const points = useMemo(
    () => appendTick(data.points, tick, INTERVAL_SECONDS[data.interval]),
    [data, tick],
  )
  const last = points.at(-1)
  const reference = data.previous_close ?? data.points[0]?.open ?? null
  const up = last && reference !== null ? last.close >= reference : true

  useEffect(() => {
    if (!containerRef.current || isDomTest) return undefined
    const chart = createChart(containerRef.current, {
      autoSize: true,
      height: 340,
      layout: { background: { color: '#ffffff' }, textColor: '#3d4242' },
      grid: { vertLines: { color: '#eef0ef' }, horzLines: { color: '#eef0ef' } },
      rightPriceScale: { borderColor: '#d7dcda' },
      timeScale: {
        borderColor: '#d7dcda',
        timeVisible: true,
        tickMarkFormatter: (time: UTCTimestamp) => formatIstTime(time),
      },
      localization: { timeFormatter: (time: UTCTimestamp) => formatIstTime(time, true) },
    })
    seriesRef.current = chart.addSeries(AreaSeries, { lineWidth: 2, priceLineVisible: true })
    return () => {
      seriesRef.current = null
      baseLineRef.current = null
      chart.remove()
    }
  }, [])

  useEffect(() => {
    const series = seriesRef.current
    if (!series) return
    series.applyOptions({
      lineColor: up ? UP : DOWN,
      topColor: up ? 'rgba(31, 122, 104, 0.28)' : 'rgba(180, 68, 58, 0.28)',
      bottomColor: 'rgba(255, 255, 255, 0)',
    })
  }, [up])

  useEffect(() => {
    const series = seriesRef.current
    if (!series) return
    series.setData(data.points.map((p) => ({ time: p.t as UTCTimestamp, value: p.close })))
    if (baseLineRef.current) series.removePriceLine(baseLineRef.current)
    baseLineRef.current =
      data.previous_close === null
        ? null
        : series.createPriceLine({
            price: data.previous_close,
            color: '#6b7370',
            lineStyle: LineStyle.Dashed,
            lineWidth: 1,
            axisLabelVisible: true,
            title: 'day start',
          })
  }, [data])

  useEffect(() => {
    if (tick && last) seriesRef.current?.update({ time: last.t as UTCTimestamp, value: last.close })
  }, [last, tick])

  return (
    <>
      <div className="chart-accessible-summary" role="img" aria-label={label}>
        {last
          ? `Latest ${formatIstTime(last.t, true)} IST: ${formatPrice(last.close, null)}${reference !== null ? `, ${up ? 'up' : 'down'} from ${formatPrice(reference, null)}` : ''}.`
          : 'No points.'}
      </div>
      {isDomTest ? (
        <div className="chart-fallback" data-testid="intraday-count">
          {points.length}
        </div>
      ) : (
        <div className="impact-chart candle-chart" ref={containerRef} data-testid="intraday-chart" />
      )}
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
    <span className={change >= 0 ? 'change-up' : 'change-down'}>
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
  const [country, setCountry] = useState('all')
  const [exchange, setExchange] = useState('all')
  const query = useDebounced(text.trim(), 300)
  const hits = useQuery<SearchResponse>({
    queryKey: ['search', query, country, exchange],
    queryFn: () => searchInstruments(query, country, exchange),
    enabled: query.length >= 2,
    staleTime: 300_000,
    placeholderData: (previous) => previous,
  })
  const markets = hits.data?.markets ?? []
  const countries = countByKey(markets, (m) => m.country)
  const exchanges = countByKey(
    markets.filter((m) => m.country === country),
    (m) => m.exchange,
  )
  const pickCountry = (value: string) => {
    setCountry(value)
    setExchange('all')
  }

  return (
    <div className="instrument-search">
      <label htmlFor={id}>
        <Search size={16} aria-hidden="true" /> {label}
      </label>
      <input
        id={id}
        type="search"
        autoComplete="off"
        placeholder="e.g. reliance, tata, infosys, apple, bitcoin, or a ticker like 500325.BO"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      {query.length >= 2 && countries.length > 0 ? (
        <div className="search-filters">
          <div className="filter-row" role="group" aria-label={`${id} country`}>
            <span className="filter-label">Market</span>
            <FilterChip active={country === 'all'} onClick={() => pickCountry('all')}>
              All
            </FilterChip>
            {countries.map(([name, count]) => (
              <FilterChip key={name} active={country === name} onClick={() => pickCountry(name)}>
                {name === 'Global' ? 'Crypto & FX' : name} <span className="chip-count">{count}</span>
              </FilterChip>
            ))}
          </div>
          {country !== 'all' && exchanges.length > 1 ? (
            <div className="filter-row" role="group" aria-label={`${id} exchange`}>
              <span className="filter-label">Exchange</span>
              <FilterChip active={exchange === 'all'} onClick={() => setExchange('all')}>
                All
              </FilterChip>
              {exchanges.map(([name, count]) => (
                <FilterChip key={name} active={exchange === name} onClick={() => setExchange(name)}>
                  {name} <span className="chip-count">{count}</span>
                </FilterChip>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
      {query.length >= 2 ? (
        hits.error ? (
          <p className="error-state" role="alert">
            {describeApiError(hits.error)}
          </p>
        ) : hits.data && hits.data.results.length === 0 ? (
          <p className="quiet-copy">
            No matches for “{query}”{country !== 'all' ? ` in ${country}` : ''}.
            {country !== 'all' && countries.length > 0 ? ' Try another market above.' : ''}
          </p>
        ) : (
          <ul className="search-results" aria-label="Search results">
            {(hits.data?.results ?? []).map((hit) => (
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
                    {hit.exchange} · {hit.country === 'Global' ? 'Global' : hit.country} · {hit.quote_type}
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

/** [key, total count] pairs, biggest first, India always leading. */
function countByKey<T extends { count: number }>(items: T[], key: (item: T) => string): [string, number][] {
  const totals = new Map<string, number>()
  for (const item of items) totals.set(key(item), (totals.get(key(item)) ?? 0) + item.count)
  return [...totals].sort((a, b) => Number(b[0] === 'India') - Number(a[0] === 'India') || b[1] - a[1])
}

function FilterChip({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className="ghost-button filter-chip" aria-pressed={active} onClick={onClick}>
      {children}
    </button>
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

/** Latest tick per symbol from the server's SSE stream (one connection for all of them). */
function useLiveTicks(symbols: string[]): Record<string, Tick> {
  const [latest, setLatest] = useState<Record<string, Tick>>({})
  const key = symbols.join(',')
  useEffect(() => {
    if (typeof EventSource === 'undefined') return undefined
    const wanted = new Set(key.split(','))
    const source = new EventSource(streamUrl([...wanted]))
    const onTick = (event: MessageEvent<string>) => {
      const tick = JSON.parse(event.data) as Tick
      if (wanted.has(tick.symbol)) setLatest((current) => ({ ...current, [tick.symbol]: tick }))
    }
    source.addEventListener('tick', onTick)
    return () => source.close()
  }, [key])
  return latest
}


function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(value), delayMs)
    return () => window.clearTimeout(id)
  }, [value, delayMs])
  return debounced
}
