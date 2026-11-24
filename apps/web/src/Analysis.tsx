import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createChart, LineSeries, LineStyle, type LineData } from 'lightweight-charts'

import { type AnalyzeResponse, type PriceBar, type Projection, describeApiError, fetchAnalysis } from './api'
import { fanLines, formatP, formatPct, formatPrice } from './marketData'

const isDomTest = typeof navigator !== 'undefined' && navigator.userAgent.includes('jsdom')

const MODEL_LABEL: Record<Projection['model'], string> = {
  gbm: 'GBM (constant volatility, normal shocks)',
  garch_t: 'GARCH-t (volatility clustering, fat tails)',
}

const METHOD_LABEL: Record<string, string> = {
  historical: 'Historical',
  student_t: 'Student t',
  garch_t: 'GARCH-t (today’s volatility)',
}

/** Analysis of five years of daily data for any symbol: findings, forecast fan, risk, tests. */
export function AnalysisPanels({ symbol, history }: { symbol: string; history: PriceBar[] | undefined }) {
  const analysis = useQuery<AnalyzeResponse>({
    queryKey: ['analysis', symbol],
    queryFn: () => fetchAnalysis(symbol),
    staleTime: 15 * 60_000,
    retry: (count, error) => count < 1 && !/^4\d\d/.test(error.message),
  })

  if (analysis.error) {
    return (
      <section className="panel chart-panel" aria-labelledby="analysis-heading">
        <h3 id="analysis-heading">Analysis</h3>
        <p className="error-state" role="alert">
          {describeApiError(analysis.error)}
        </p>
      </section>
    )
  }
  if (!analysis.data) {
    return (
      <section className="panel chart-panel" aria-labelledby="analysis-heading">
        <h3 id="analysis-heading">Analysis</h3>
        <p className="empty-state">Analysing five years of daily data…</p>
      </section>
    )
  }
  const { report, data, benchmark } = analysis.data
  return (
    <>
      <section className="panel chart-panel" aria-labelledby="findings-heading">
        <p className="eyebrow">
          Analysis · {report.first_date} → {report.last_date} · {report.n_returns} sessions · {data.source}
        </p>
        <h3 id="findings-heading">What the data says</h3>
        <ul className="plain-list findings" data-testid="findings">
          {report.findings.map((finding) => (
            <li key={finding}>{finding}</li>
          ))}
        </ul>
        {benchmark?.note ? <p className="quiet-copy">{benchmark.note}</p> : null}
      </section>
      <ForecastPanel analysis={analysis.data} history={history} />
      <RiskPanel analysis={analysis.data} />
      <VolatilityPanel analysis={analysis.data} />
      <TestsPanel analysis={analysis.data} />
      <BenchmarkPanel analysis={analysis.data} />
    </>
  )
}

function ForecastPanel({ analysis, history }: { analysis: AnalyzeResponse; history: PriceBar[] | undefined }) {
  const { report, data } = analysis
  const [model, setModel] = useState<Projection['model']>(
    report.projections.some((p) => p.model === 'garch_t') ? 'garch_t' : 'gbm',
  )
  const projection = report.projections.find((p) => p.model === model) ?? report.projections[0]
  if (!projection) return null
  return (
    <section className="panel chart-panel" aria-labelledby="forecast-heading">
      <div className="section-heading">
        <div>
          <p className="eyebrow">
            Monte Carlo · {projection.n_paths.toLocaleString()} paths · seed {projection.seed}
          </p>
          <h3 id="forecast-heading">Range of outcomes, not a prediction</h3>
        </div>
        <div className="range-picker" role="group" aria-label="Model">
          {report.projections.map((p) => (
            <button
              key={p.model}
              type="button"
              className="ghost-button"
              aria-pressed={p.model === model}
              onClick={() => setModel(p.model)}
            >
              {p.model === 'gbm' ? 'GBM' : 'GARCH-t'}
            </button>
          ))}
        </div>
      </div>
      <p className="quiet-copy">
        {MODEL_LABEL[projection.model]}. Bands hold 50% and 90% of simulated paths. The drift is the
        historical average return, which is known only to ± {formatPct(report.distribution.mean_annual_se)} a year.
      </p>
      <FanChart projection={projection} report={report} history={history} />
      <table className="event-table data-table">
        <caption className="visually-hidden">Simulated outcomes by horizon</caption>
        <thead>
          <tr>
            <th scope="col">Horizon</th>
            <th scope="col">Median</th>
            <th scope="col">90% range</th>
            <th scope="col">P(gain)</th>
            <th scope="col">P(loss &gt; 10%)</th>
            <th scope="col">P(drawdown &gt; 10%)</th>
          </tr>
        </thead>
        <tbody>
          {projection.horizons.map((h) => (
            <tr key={h.horizon_days}>
              <th scope="row">{h.horizon_days} sessions</th>
              <td>{formatPct(h.return_quantiles[2])}</td>
              <td>
                {formatPct(h.return_quantiles[0])} to {formatPct(h.return_quantiles[4])}
              </td>
              <td>{formatPct(h.prob_gain, 0)}</td>
              <td>{formatPct(h.prob_loss_over_10pct, 0)}</td>
              <td>{formatPct(h.prob_drawdown_over_10pct, 0)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="quiet-copy mono">Prices in {data.currency ?? 'the listing currency'}.</p>
    </section>
  )
}

function FanChart({
  projection,
  report,
  history,
}: {
  projection: Projection
  report: AnalyzeResponse['report']
  history: PriceBar[] | undefined
}) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const lines = useMemo(
    () => fanLines(projection.fan, report.last_date, report.last_price, report.periods_per_year),
    [projection, report],
  )
  const last = projection.fan.at(-1)

  useEffect(() => {
    if (!containerRef.current || isDomTest) return undefined
    const chart = createChart(containerRef.current, {
      autoSize: true,
      height: 320,
      layout: { background: { color: '#ffffff' }, textColor: '#3d4242' },
      grid: { vertLines: { color: '#eef0ef' }, horzLines: { color: '#eef0ef' } },
      rightPriceScale: { borderColor: '#d7dcda' },
      timeScale: { borderColor: '#d7dcda' },
    })
    const past = (history ?? [])
      .filter((b) => b.date <= report.last_date)
      .slice(-126)
      .map((b) => ({ time: b.date, value: b.adj_close ?? b.close }))
    chart.addSeries(LineSeries, { color: '#202423', lineWidth: 2, priceLineVisible: false }).setData(
      past as LineData<string>[],
    )
    const style = [
      { color: '#b4443a', lineStyle: LineStyle.Dashed },
      { color: '#1f7a68', lineStyle: LineStyle.Dotted },
      { color: '#335c81', lineStyle: LineStyle.Solid },
      { color: '#1f7a68', lineStyle: LineStyle.Dotted },
      { color: '#b4443a', lineStyle: LineStyle.Dashed },
    ]
    lines.forEach((points, q) => {
      chart
        .addSeries(LineSeries, { ...style[q], lineWidth: 2, priceLineVisible: false, lastValueVisible: q === 2 })
        .setData(points as LineData<string>[])
    })
    chart.timeScale().fitContent()
    return () => chart.remove()
  }, [history, lines, report.last_date])

  return (
    <>
      <div className="chart-accessible-summary" role="img" aria-label="Simulated price fan chart">
        {last
          ? `After ${last.day} sessions: 5% of paths end below ${last.price[0].toFixed(2)}, half below ${last.price[2].toFixed(2)}, 95% below ${last.price[4].toFixed(2)}.`
          : 'No projection.'}
      </div>
      {isDomTest ? (
        <div className="chart-fallback" data-testid="fan-days">
          {projection.fan.length}
        </div>
      ) : (
        <div className="impact-chart candle-chart" ref={containerRef} data-testid="fan-chart" />
      )}
    </>
  )
}

function RiskPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const { risk } = analysis.report
  const dd = risk.max_drawdown
  return (
    <section className="panel" aria-labelledby="risk-heading">
      <p className="eyebrow">Downside, one trading day</p>
      <h3 id="risk-heading">Value at risk and expected shortfall</h3>
      <table className="event-table data-table" data-testid="var-table">
        <caption className="visually-hidden">One-day value at risk and expected shortfall</caption>
        <thead>
          <tr>
            <th scope="col">Method</th>
            <th scope="col">Level</th>
            <th scope="col">VaR</th>
            <th scope="col">Expected shortfall</th>
          </tr>
        </thead>
        <tbody>
          {risk.var_es.map((v) => (
            <tr key={`${v.method}-${v.confidence}`}>
              <th scope="row">{METHOD_LABEL[v.method] ?? v.method}</th>
              <td>{Math.round(v.confidence * 100)}%</td>
              <td>{formatPct(v.var, 2)}</td>
              <td>{formatPct(v.es, 2)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <ul className="plain-list">
        <li>
          Worst drawdown {formatPct(dd.max)} ({dd.peak_date} → {dd.trough_date}
          {dd.recovery_date ? `, recovered ${dd.recovery_date}` : ', not yet recovered'}); now{' '}
          {formatPct(dd.current)} below the peak
        </li>
        <li>
          Sharpe {risk.sharpe.toFixed(2)} ± {risk.sharpe_se.toFixed(2)} (zero risk-free rate) · Sortino{' '}
          {Number.isFinite(risk.sortino) ? risk.sortino.toFixed(2) : '—'}
        </li>
      </ul>
    </section>
  )
}

function VolatilityPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const { volatility, distribution } = analysis.report
  const g = volatility.garch
  return (
    <section className="panel" aria-labelledby="vol-heading">
      <p className="eyebrow">Volatility, annualised</p>
      <h3 id="vol-heading">How much it moves</h3>
      <ul className="plain-list">
        <li>Historical {formatPct(distribution.vol_annual)} · EWMA now {formatPct(volatility.ewma_annual)}</li>
        {g ? (
          <>
            <li>
              GARCH-t now {formatPct(g.current_vol_annual)} vs long-run {formatPct(g.long_run_vol_annual)}; shocks
              halve in {g.half_life_days.toFixed(0)} sessions (persistence {g.persistence.toFixed(3)}, tail ν{' '}
              {g.nu.toFixed(1)})
            </li>
            <li>
              Expected over{' '}
              {g.term_structure.map((t) => `${t.horizon_days}d ${formatPct(t.vol_annual)}`).join(' · ')}
            </li>
          </>
        ) : (
          <li>GARCH needs at least 250 daily returns; not fitted on this history.</li>
        )}
      </ul>
    </section>
  )
}

function TestsPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const { distribution: d, predictability: p } = analysis.report
  const rows: [string, string, string, string][] = [
    ['Normality (Jarque–Bera)', d.jarque_bera.statistic.toFixed(1), formatP(d.jarque_bera.p_value), d.jarque_bera.reject_5pct ? 'Not normal' : 'Normal not rejected'],
    [`Autocorrelation (Ljung–Box, ${p.lags} lags)`, p.ljung_box_returns.statistic.toFixed(1), formatP(p.ljung_box_returns.p_value), p.ljung_box_returns.reject_5pct ? 'Some dependence' : 'None found'],
    ['Volatility clustering (Ljung–Box on r²)', p.ljung_box_squared.statistic.toFixed(1), formatP(p.ljung_box_squared.p_value), p.ljung_box_squared.reject_5pct ? 'Clusters' : 'None found'],
    ...p.variance_ratios.map((v): [string, string, string, string] => [
      `Variance ratio, ${v.q} days (${v.ratio.toFixed(2)})`,
      v.test.statistic.toFixed(2),
      formatP(v.test.p_value),
      v.test.reject_5pct ? (v.ratio > 1 ? 'Trending' : 'Mean-reverting') : 'Random walk not rejected',
    ]),
    ['Unit root (ADF, log price)', p.adf.statistic.toFixed(2), `5% crit ${p.adf.critical_values[1].toFixed(2)}`, p.adf.reject_unit_root_5pct ? 'Stationary' : 'Random walk'],
  ]
  return (
    <section className="panel" aria-labelledby="tests-heading">
      <p className="eyebrow">Statistical tests · 5% level</p>
      <h3 id="tests-heading">Shape and predictability</h3>
      <p className="quiet-copy">
        Skew {d.skewness.toFixed(2)} · excess kurtosis {d.excess_kurtosis.toFixed(1)}
        {d.hill_tail_index ? ` · loss-tail index ${d.hill_tail_index.toFixed(1)}` : ''} · worst day{' '}
        {formatPct(Math.exp(d.worst_day) - 1)} · best day {formatPct(Math.exp(d.best_day) - 1)}
      </p>
      <table className="event-table data-table" data-testid="tests-table">
        <caption className="visually-hidden">Statistical tests</caption>
        <thead>
          <tr>
            <th scope="col">Test</th>
            <th scope="col">Statistic</th>
            <th scope="col">p-value</th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(([name, stat, pv, verdict]) => (
            <tr key={name}>
              <th scope="row">{name}</th>
              <td>{stat}</td>
              <td>{pv}</td>
              <td>{verdict}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  )
}

function BenchmarkPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const exposure = analysis.report.benchmark
  const chosen = analysis.benchmark
  return (
    <section className="panel" aria-labelledby="bench-heading">
      <p className="eyebrow">Market exposure</p>
      <h3 id="bench-heading">
        {chosen ? `Against ${chosen.name ?? chosen.symbol} (${chosen.symbol})` : 'No benchmark'}
      </h3>
      {exposure ? (
        <ul className="plain-list" data-testid="benchmark">
          <li>
            Beta {exposure.beta.toFixed(2)} ± {exposure.beta_se.toFixed(2)}: a 1% market move has come with a{' '}
            {exposure.beta.toFixed(2)}% move here
          </li>
          <li>Market explains {formatPct(exposure.r_squared)} of daily variance (correlation {exposure.correlation.toFixed(2)})</li>
          <li>
            Alpha {formatPct(exposure.alpha_annual)} a year ± {formatPct(exposure.alpha_annual_se)}; stock-specific
            volatility {formatPct(exposure.idiosyncratic_vol_annual)}
          </li>
          <li className="quiet-copy">
            {exposure.n_common_days} common sessions ·{' '}
            {chosen?.selection === 'auto' ? 'chosen automatically from the listing’s home market' : 'requested'}
          </li>
        </ul>
      ) : (
        <p className="quiet-copy">
          {chosen
            ? 'The benchmark could not be loaded, so exposure is not shown; the rest of the analysis does not depend on it.'
            : 'Indices, currencies, futures and crypto are the market themselves; the analysis needs no reference.'}
        </p>
      )}
      <p className="quiet-copy mono">
        Last price {formatPrice(analysis.report.last_price, analysis.data.currency)}
      </p>
    </section>
  )
}
