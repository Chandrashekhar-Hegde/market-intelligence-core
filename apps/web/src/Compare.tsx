import { useEffect, useMemo, useRef, useState } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { createChart, LineSeries, type LineData } from 'lightweight-charts'

import { type AnalyzeResponse, type CompareResponse, describeApiError, fetchAnalysis, fetchComparison } from './api'
import { InstrumentSearch } from './Markets'
import { SERIES_COLORS, correlationColor, formatPct } from './marketData'
import { correlationWords, equalMix } from './plainLanguage'

const isDomTest = typeof navigator !== 'undefined' && navigator.userAgent.includes('jsdom')
const RANGES = ['1y', '2y', '5y', '10y'] as const
const MAX = 10
type Matrix = 'weekly' | 'daily' | 'rank'
const MATRIX_LABEL: Record<Matrix, string> = {
  weekly: 'Weekly returns',
  daily: 'Daily returns',
  rank: 'Daily ranks (Spearman)',
}

/** Compare up to ten instruments from any exchange on their common trading days. */
export function Compare() {
  const [symbols, setSymbols] = useState<string[]>(['RELIANCE.NS', 'TCS.NS', '^NSEI'])
  const [range, setRange] = useState<(typeof RANGES)[number]>('5y')
  const ready = symbols.length >= 2
  const comparison = useQuery<CompareResponse>({
    queryKey: ['compare', symbols, range],
    queryFn: () => fetchComparison(symbols, range),
    enabled: ready,
    staleTime: 15 * 60_000,
    retry: (count, error) => count < 1 && !/^4\d\d/.test(error.message),
  })
  const add = (symbol: string) =>
    setSymbols((current) => (current.includes(symbol) || current.length >= MAX ? current : [...current, symbol]))

  return (
    <>
      <header className="topbar">
        <div>
          <p className="eyebrow">Compare · any exchange</p>
          <h2>Which did better, which is riskier, and do they move together?</h2>
        </div>
      </header>

      <section className="panel chart-panel" aria-labelledby="compare-pick-heading">
        <h3 id="compare-pick-heading">Instruments ({symbols.length} of {MAX})</h3>
        <ul className="chip-list" aria-label="Selected instruments">
          {symbols.map((s, i) => (
            <li key={s} className="chip">
              <span className="chip-swatch" style={{ background: SERIES_COLORS[i % SERIES_COLORS.length] }} aria-hidden="true" />
              {s}
              <button
                type="button"
                aria-label={`Remove ${s}`}
                onClick={() => setSymbols((current) => current.filter((x) => x !== s))}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
        {symbols.length < MAX ? (
          <InstrumentSearch onPick={add} id="compare-search" label="Add an instrument" />
        ) : (
          <p className="quiet-copy">Ten is the maximum. Remove one to add another.</p>
        )}
        <div className="range-picker" role="group" aria-label="Range">
          {RANGES.map((r) => (
            <button key={r} type="button" className="ghost-button" aria-pressed={r === range} onClick={() => setRange(r)}>
              {r}
            </button>
          ))}
        </div>
      </section>

      {!ready ? (
        <section className="panel chart-panel">
          <p className="empty-state">Pick at least two instruments to compare.</p>
        </section>
      ) : comparison.error ? (
        <section className="panel chart-panel">
          <p className="error-state" role="alert">
            {describeApiError(comparison.error)}
          </p>
        </section>
      ) : !comparison.data ? (
        <section className="panel chart-panel">
          <p className="empty-state">Aligning {symbols.length} price histories…</p>
        </section>
      ) : (
        <CompareResults data={comparison.data} />
      )}
    </>
  )
}

function CompareResults({ data }: { data: CompareResponse }) {
  const c = data.comparison
  const [matrix, setMatrix] = useState<Matrix>(data.notes.some((n) => n.includes('different times')) ? 'weekly' : 'daily')
  const values = matrix === 'weekly' ? c.correlation_weekly : matrix === 'daily' ? c.correlation_daily : c.spearman_daily
  return (
    <>
      <PlainComparison data={data} />

      <section className="panel chart-panel" aria-labelledby="perf-heading">
        <p className="eyebrow">₹100 (or 100 in each one’s own currency) put in on {c.first_date}, dividends included</p>
        <h3 id="perf-heading">How each one grew</h3>
        <RebasedChart data={data} />
      </section>

      <details className="panel chart-panel maths-all">
        <summary>
          <span className="maths-title">Show all the maths</span>
          <span className="quiet-copy"> Findings, correlation matrices, shared factors, covariance and pairs tests</span>
        </summary>
        <div className="maths-grid">
      <section className="panel chart-panel" aria-labelledby="compare-findings-heading">
        <p className="eyebrow">
          {c.n_common_days} common trading days · {c.first_date} → {c.last_date}
        </p>
        <h3 id="compare-findings-heading">What the data says</h3>
        <ul className="plain-list findings" data-testid="compare-findings">
          {c.findings.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>
        {data.notes.map((n) => (
          <p key={n} className="lab-banner" role="note">
            {n}
          </p>
        ))}
      </section>

      <section className="panel chart-panel" aria-labelledby="perf-table-heading">
        <h3 id="perf-table-heading">Performance over the common period</h3>
        <table className="event-table data-table" data-testid="performance-table">
          <caption className="visually-hidden">Performance over the common period</caption>
          <thead>
            <tr>
              <th scope="col">Instrument</th>
              <th scope="col">Total</th>
              <th scope="col">Per year</th>
              <th scope="col">Volatility</th>
              <th scope="col">Sharpe</th>
              <th scope="col">Worst drawdown</th>
            </tr>
          </thead>
          <tbody>
            {c.performance.map((p) => (
              <tr key={p.symbol}>
                <th scope="row">{p.symbol}</th>
                <td>{formatPct(p.total_return)}</td>
                <td>{formatPct(p.annual_return)}</td>
                <td>{formatPct(p.annual_vol)}</td>
                <td>{p.sharpe.toFixed(2)}</td>
                <td>{formatPct(p.max_drawdown)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="panel chart-panel" aria-labelledby="corr-heading">
        <div className="section-heading">
          <div>
            <p className="eyebrow">{MATRIX_LABEL[matrix]} · 1 = move together, 0 = unrelated, −1 = opposite</p>
            <h3 id="corr-heading">Correlation</h3>
          </div>
          <div className="range-picker" role="group" aria-label="Correlation basis">
            {(Object.keys(MATRIX_LABEL) as Matrix[]).map((m) => (
              <button key={m} type="button" className="ghost-button" aria-pressed={m === matrix} onClick={() => setMatrix(m)}>
                {m === 'rank' ? 'Rank' : m[0].toUpperCase() + m.slice(1)}
              </button>
            ))}
          </div>
        </div>
        <table className="data-table heatmap" data-testid="heatmap">
          <caption className="visually-hidden">{MATRIX_LABEL[matrix]} correlation matrix</caption>
          <thead>
            <tr>
              <th scope="col">
                <span className="visually-hidden">Instrument</span>
              </th>
              {c.symbols.map((s) => (
                <th key={s} scope="col">
                  {s}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {c.symbols.map((row, i) => (
              <tr key={row}>
                <th scope="row">{row}</th>
                {values[i]?.map((v, j) => (
                  <td key={c.symbols[j]} style={{ background: correlationColor(v) }}>
                    {v.toFixed(2)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="panel" aria-labelledby="pca-heading">
        <p className="eyebrow">Principal components of the correlation matrix</p>
        <h3 id="pca-heading">How much is one shared factor?</h3>
        <ul className="plain-list" data-testid="pca">
          {c.pca.explained.slice(0, 3).map((share, i) => (
            <li key={i}>
              <span className="bar-label">Factor {i + 1}: {formatPct(share)}</span>
              <span className="bar" style={{ width: `${Math.round(share * 100)}%` }} aria-hidden="true" />
            </li>
          ))}
        </ul>
        <p className="quiet-copy">
          Factor 1 is usually "the market": when it explains most of the variance, these instruments rise and fall together and
          holding all of them diversifies little.
        </p>
      </section>

      <section className="panel" aria-labelledby="lw-heading">
        <p className="eyebrow">Covariance for portfolio maths</p>
        <h3 id="lw-heading">Shrinkage</h3>
        <p className="quiet-copy">
          Ledoit–Wolf shrinks the sample covariance {formatPct(c.covariance_shrinkage)} of the way toward a simple target. Sample
          covariances overfit noise, and shrinking makes portfolio risk estimates more stable out of sample.
        </p>
      </section>

      <section className="panel chart-panel" aria-labelledby="pairs-heading">
        <p className="eyebrow">Engle–Granger cointegration · 5% level</p>
        <h3 id="pairs-heading">Pairs</h3>
        <p className="quiet-copy">
          Correlation is about daily moves; cointegration is about prices staying tied over the long run. A cointegrated pair's
          spread keeps returning to its average (the half-life says how fast), which is the basis of pairs trading.
        </p>
        <table className="event-table data-table" data-testid="pairs-table">
          <caption className="visually-hidden">Pairwise correlation and cointegration</caption>
          <thead>
            <tr>
              <th scope="col">Pair</th>
              <th scope="col">Correlation (weekly / daily)</th>
              <th scope="col">Cointegration test</th>
              <th scope="col">Half-life</th>
              <th scope="col">Spread today</th>
            </tr>
          </thead>
          <tbody>
            {c.pairs.map((p) => {
              const ci = p.cointegration
              return (
                <tr key={`${p.a}-${p.b}`}>
                  <th scope="row">
                    {p.a} / {p.b}
                  </th>
                  <td>
                    {p.correlation_weekly.toFixed(2)} / {p.correlation_daily.toFixed(2)}
                  </td>
                  <td>
                    {ci.statistic === null ? '—' : ci.statistic.toFixed(2)} vs {ci.critical_values[1].toFixed(2)}{' '}
                    <span className={`status-chip tone-${ci.cointegrated_5pct ? 'green' : 'neutral'}`}>
                      {ci.cointegrated_5pct ? 'cointegrated' : 'not tied'}
                    </span>
                  </td>
                  <td>{ci.cointegrated_5pct && ci.half_life_days ? `${ci.half_life_days.toFixed(0)} days` : '—'}</td>
                  <td>{ci.cointegrated_5pct ? `${ci.spread_z_now.toFixed(1)} SD` : '—'}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </section>
        </div>
      </details>
    </>
  )
}

const STANCE_WORD = { buy: 'Buy', sell: 'Sell / stay out', hold: 'Hold' } as const

/** The comparison in plain words: a head-to-head table, then what it means to own them together. */
function PlainComparison({ data }: { data: CompareResponse }) {
  const c = data.comparison
  const analyses = useQueries({
    queries: c.symbols.map((symbol) => ({
      queryKey: ['analysis', symbol, '10y'],
      queryFn: () => fetchAnalysis(symbol, '10y'),
      staleTime: 15 * 60_000,
      retry: false,
    })),
  })
  const name = (symbol: string) => data.instruments.find((i) => i.symbol === symbol)?.name ?? symbol
  const years = Math.max(1, Math.round((Date.parse(c.last_date) - Date.parse(c.first_date)) / (365.25 * 86_400_000)))
  const byReturn = [...c.performance].sort((a, b) => b.annual_return - a.annual_return)
  const byVol = [...c.performance].sort((a, b) => a.annual_vol - b.annual_vol)
  const byFall = [...c.performance].sort((a, b) => b.max_drawdown - a.max_drawdown)
  // Labelled only when strictly so: ties get no label.
  const calmest = byVol.length > 1 && byVol[0]!.annual_vol < byVol[1]!.annual_vol ? byVol[0]!.symbol : null
  const bumpiest =
    byVol.length > 1 && byVol.at(-1)!.annual_vol > byVol.at(-2)!.annual_vol ? byVol.at(-1)!.symbol : null
  const mix = equalMix(c.covariance_annual)
  const weekly = data.notes.some((n) => n.includes('different times'))
  return (
    <section className="panel chart-panel" aria-labelledby="plain-compare-heading" data-testid="plain-compare">
      <p className="eyebrow">
        Over the last {years} {years === 1 ? 'year' : 'years'} ({c.n_common_days.toLocaleString('en-IN')} trading days all
        of them traded)
      </p>
      <h3 id="plain-compare-heading">Side by side</h3>
      <table className="event-table data-table" data-testid="head-to-head">
        <caption className="visually-hidden">Head-to-head comparison</caption>
        <thead>
          <tr>
            <th scope="col">Instrument</th>
            <th scope="col">Grew per year</th>
            <th scope="col">How bumpy (yearly swing)</th>
            <th scope="col">Worst fall</th>
            <th scope="col">Today’s advice</th>
          </tr>
        </thead>
        <tbody>
          {c.performance.map((p, i) => {
            const advice = (analyses[i]?.data as AnalyzeResponse | undefined)?.advice
            return (
              <tr key={p.symbol}>
                <th scope="row">
                  {name(p.symbol)}
                  {name(p.symbol) !== p.symbol ? <span className="quiet-copy"> {p.symbol}</span> : null}
                </th>
                <td>{formatPct(p.annual_return)}</td>
                <td>
                  ±{formatPct(p.annual_vol, 0)}
                  {p.symbol === calmest ? ' · calmest' : p.symbol === bumpiest ? ' · bumpiest' : ''}
                </td>
                <td>−{formatPct(p.max_drawdown, 0)}</td>
                <td>{advice ? STANCE_WORD[advice.stance] : analyses[i]?.isError ? '—' : '…'}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      <ul className="plain-list" data-testid="plain-compare-findings">
        {byReturn[0] && byReturn.at(-1) && byReturn[0] !== byReturn.at(-1) ? (
          <li>
            <strong>{name(byReturn[0].symbol)}</strong> grew the most ({formatPct(byReturn[0].annual_return)} a year);{' '}
            <strong>{name(byReturn.at(-1)!.symbol)}</strong> the least ({formatPct(byReturn.at(-1)!.annual_return)}).
          </li>
        ) : null}
        {byFall[0] ? (
          <li>
            The deepest fall was <strong>{name(byFall[0].symbol)}</strong>’s: −{formatPct(byFall[0].max_drawdown, 0)} from a
            high at its worst.
          </li>
        ) : null}
        {c.pairs.slice(0, 6).map((pair) => {
          const rho = weekly ? pair.correlation_weekly : pair.correlation_daily
          return (
            <li key={`${pair.a}-${pair.b}`}>
              {name(pair.a)} and {name(pair.b)} {correlationWords(rho)}{' '}
              <span className="quiet-copy">(correlation {rho.toFixed(2)})</span>.
            </li>
          )
        })}
        {mix ? (
          <li>
            Holding all {c.symbols.length} in equal amounts swings about ±{formatPct(mix.equalMixVol, 0)} a year, against
            ±{formatPct(mix.averageAloneVol, 0)} for a typical one alone:{' '}
            {mix.cut > 0.02
              ? `mixing them cuts the swing by about ${formatPct(mix.cut, 0)}.`
              : 'mixing them barely reduces the swing, because they move together.'}
          </li>
        ) : null}
      </ul>
      <p className="quiet-copy">Past growth is not a forecast. The advice column is each one’s own call on the Markets tab.</p>
    </section>
  )
}

function RebasedChart({ data }: { data: CompareResponse }) {
  const ref = useRef<HTMLDivElement | null>(null)
  const c = data.comparison
  const series = useMemo(
    () => c.symbols.map((_, i) => c.rebased.map((p) => ({ time: p.date, value: p.values[i] ?? 100 }))),
    [c],
  )
  useEffect(() => {
    if (!ref.current || isDomTest) return undefined
    const chart = createChart(ref.current, {
      autoSize: true,
      height: 320,
      layout: { background: { color: '#ffffff' }, textColor: '#3d4242' },
      grid: { vertLines: { color: '#eef0ef' }, horzLines: { color: '#eef0ef' } },
      rightPriceScale: { borderColor: '#d7dcda' },
      timeScale: { borderColor: '#d7dcda' },
    })
    series.forEach((points, i) => {
      chart
        .addSeries(LineSeries, { color: SERIES_COLORS[i % SERIES_COLORS.length], lineWidth: 2, priceLineVisible: false, title: c.symbols[i] })
        .setData(points as LineData<string>[])
    })
    chart.timeScale().fitContent()
    return () => chart.remove()
  }, [series, c.symbols])
  const last = c.rebased.at(-1)
  return (
    <>
      <div className="chart-accessible-summary" role="img" aria-label="Rebased performance chart">
        {last ? c.symbols.map((s, i) => `${s} ${last.values[i]?.toFixed(0)}`).join(', ') : ''}
      </div>
      {isDomTest ? (
        <div className="chart-fallback" data-testid="rebased-points">
          {c.rebased.length}
        </div>
      ) : (
        <div className="impact-chart candle-chart" ref={ref} data-testid="rebased-chart" />
      )}
    </>
  )
}
