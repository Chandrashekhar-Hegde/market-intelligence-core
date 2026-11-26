import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from 'react'
import { createChart, LineSeries, LineStyle, type LineData } from 'lightweight-charts'

import { type AnalysisRange, type AnalyzeResponse, type PriceBar, type Projection, type Signal, describeApiError, useAnalysis } from './api'
import { fanLines, formatP, formatPct, formatPrice } from './marketData'
import {
  formatMoney,
  horizonWords,
  maxPosition,
  meaningCards,
  preferredProjection,
  stanceGuidance,
  yearsCovered,
} from './plainLanguage'

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

/**
 * What to do, what it means for your money, and (folded away) all the statistics behind it,
 * for up to ten years of daily data on any symbol.
 */
export function AnalysisPanels({
  symbol,
  history,
  livePrice,
  extraMaths,
}: {
  symbol: string
  history: PriceBar[] | undefined
  livePrice?: number | null
  extraMaths?: ReactNode
}) {
  const [range, setRange] = useState<AnalysisRange>('10y')
  const analysis = useAnalysis(symbol, range)

  if (analysis.error) {
    return (
      <section className="panel chart-panel" aria-labelledby="analysis-heading">
        <h3 id="analysis-heading">Analysis</h3>
        <p className="error-state" role="alert">
          {describeApiError(analysis.error)}
        </p>
        {extraMaths}
      </section>
    )
  }
  if (!analysis.data) {
    return (
      <section className="panel chart-panel" aria-labelledby="analysis-heading">
        <h3 id="analysis-heading">Analysis</h3>
        <p className="empty-state">Testing trading rules and analysing ten years of daily data…</p>
      </section>
    )
  }
  const { report, data, benchmark } = analysis.data
  return (
    <>
      <AdviceCard analysis={analysis.data} livePrice={livePrice ?? null} />
      <MeaningPanel analysis={analysis.data} history={history} range={range} onRange={setRange} />
      <details className="panel chart-panel maths-all">
        <summary>
          <span className="maths-title">Show all the maths</span>
          <span className="quiet-copy"> Statistical tests, model scores and data checks behind everything above</span>
        </summary>
        <div className="maths-grid">
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
          <SignalsPanel analysis={analysis.data} />
          <ForecastPanel analysis={analysis.data} />
          <RiskPanel analysis={analysis.data} />
          <VolatilityPanel analysis={analysis.data} />
          <ValidationPanel analysis={analysis.data} />
          <TestsPanel analysis={analysis.data} />
          <BenchmarkPanel analysis={analysis.data} />
          {extraMaths}
        </div>
      </details>
    </>
  )
}

const STANCE_LABEL = { buy: 'Buy', sell: 'Sell / stay out', hold: 'Hold' } as const
const STANCE_TONE = { buy: 'green', sell: 'red', hold: 'neutral' } as const

function lean(position: number): { label: string; tone: string } {
  if (position > 0) return { label: 'Leans buy', tone: 'green' }
  if (position < 0) return { label: 'Leans sell', tone: 'red' }
  return { label: 'No view', tone: 'neutral' }
}

function provenTest(signal: Signal) {
  return signal.tests.filter((t) => t.earns_place).sort((a, b) => a.p_holm - b.p_holm)[0]
}

/** "Tue, 22 Dec 2026" */
function formatDate(date: string): string {
  const d = new Date(`${date}T00:00:00Z`)
  const dayMonth = d.toLocaleDateString('en-IN', { weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC' })
  return `${dayMonth} ${d.getUTCFullYear()}`
}

/** Buy / Sell / Hold, only from rules that have worked on this instrument, with the evidence. */
function AdviceCard({ analysis, livePrice }: { analysis: AnalyzeResponse; livePrice: number | null }) {
  const { advice, signals, signals_note: note, data } = analysis
  const [maxLoss, setMaxLoss] = useState(500)
  const lossId = useId()
  const currency = data.currency
  if (!advice || !signals) {
    return (
      <section className="panel chart-panel advice-card" aria-labelledby="advice-heading">
        <p className="eyebrow">What to do</p>
        <h3 id="advice-heading">No call possible yet</h3>
        <p>{note ?? 'The trading rules could not be tested on this instrument.'}</p>
      </section>
    )
  }
  const cap = maxPosition(analysis, maxLoss)
  const guidance = stanceGuidance(advice, cap, currency)
  const confidence = advice.confidence
  return (
    <section className="panel chart-panel advice-card" aria-labelledby="advice-heading" data-testid="advice">
      <div className="section-heading">
        <div>
          <p className="eyebrow">What to do · tested on {signals.n_days.toLocaleString('en-IN')} trading days</p>
          <h3 id="advice-heading">
            <span className={`stance stance-${advice.stance}`} data-testid="stance">
              {STANCE_LABEL[advice.stance]}
            </span>
          </h3>
        </div>
        <span className={`status-chip tone-${STANCE_TONE[advice.stance]}`}>
          {advice.basis === 'proven' ? 'Backed by a proven rule' : advice.basis === 'conflict' ? 'Rules disagree' : 'No proven edge'}
        </span>
      </div>
      <p className="advice-headline">{advice.headline}</p>
      <dl className="guidance" data-testid="guidance">
        <div>
          <dt>If you already own it</dt>
          <dd>{guidance.owner}</dd>
        </div>
        <div>
          <dt>If you don’t own it yet</dt>
          <dd>{guidance.buyer}</dd>
        </div>
      </dl>

      {confidence ? (
        <div className="confidence" aria-label="How often this rule was right">
          <p>
            Right <strong>{formatPct(confidence.hit_rate, 0)}</strong> of the time over {confidence.n} past calls (likely
            between {formatPct(confidence.low, 0)} and {formatPct(confidence.high, 0)}).
          </p>
          <div className="confidence-bar" aria-hidden="true">
            <span className="confidence-range" style={{ left: `${confidence.low * 100}%`, width: `${(confidence.high - confidence.low) * 100}%` }} />
            <span className="confidence-mark" style={{ left: `${confidence.hit_rate * 100}%` }} />
            <span className="confidence-coin" style={{ left: '50%' }} />
          </div>
          <p className="quiet-copy">The dashed line is a coin toss (50%).</p>
        </div>
      ) : null}

      {advice.horizon_days && advice.review_date ? (
        <p>
          Time frame: <strong>{horizonWords(advice.horizon_days)}</strong>. Look again on{' '}
          <strong>{formatDate(advice.review_date)}</strong>.
          {advice.flip_level !== null
            ? ` The call changes if the price closes ${advice.stance === 'buy' ? 'below' : 'above'} ${formatPrice(advice.flip_level, currency)}.`
            : ''}
        </p>
      ) : null}

      <h4 className="advice-subhead">What each rule sees today</h4>
      <ul className="label-legend" aria-label="What the labels mean">
        <li>
          <span className="status-chip tone-green">Leans buy</span> / <span className="status-chip tone-red">Leans sell</span>{' '}
          the rule’s reading today points up / down. <span className="status-chip tone-neutral">No view</span> it has nothing
          to say today.
        </li>
        <li>
          <span className="rule-proven">✓ Has worked here</span> following this rule on this stock’s own past beat trading
          costs reliably, so its reading counts towards the call.
        </li>
        <li>
          <span className="rule-unproven">Not proven here</span> it did not, so its reading is shown for information only
          and does not change the call.
        </li>
      </ul>
      <ul className="rule-list" data-testid="rules">
        {signals.signals.map((signal) => {
          const proven = provenTest(signal)
          const l = lean(signal.position_today)
          const liveFlip =
            livePrice !== null && signal.flip_level !== null && signal.position_today !== 0
              ? (livePrice > signal.flip_level) !== signal.position_today > 0
              : false
          return (
            <li key={signal.id}>
              <div className="rule-head">
                <strong>{signal.name}</strong>
                <span className={`status-chip tone-${l.tone}`}>{l.label}</span>
                <span className={proven ? 'rule-proven' : 'rule-unproven'}>
                  {proven ? `✓ Has worked here (${horizonWords(proven.horizon_days)})` : 'Not proven here'}
                </span>
              </div>
              <p className="quiet-copy">{signal.today}</p>
              {liveFlip && livePrice !== null ? (
                <p className="live-flip" role="status">
                  Live: at {formatPrice(livePrice, currency)} this rule would switch to “
                  {signal.position_today > 0 ? 'leans sell' : 'leans buy'}” if the day closed here.
                </p>
              ) : null}
            </li>
          )
        })}
      </ul>
      <p className="quiet-copy">
        A rule counts only if, on this instrument’s own history, it beat trading costs, held up in both halves of the
        data and passed a test that allows for having tried {signals.n_tests} rule-and-time-frame combinations. Most
        rules fail on most stocks; that is normal.
      </p>

      <div className="position-size">
        <label htmlFor={lossId}>The most I’m willing to lose on a bad day</label>
        <input
          id={lossId}
          type="number"
          min={0}
          step={100}
          inputMode="numeric"
          value={maxLoss}
          onChange={(event) => setMaxLoss(Math.max(0, Number(event.target.value) || 0))}
        />
        <p data-testid="position-size">
          {cap !== null
            ? `Put in at most ${formatMoney(cap, currency)}. On its bad days (the worst 1 in 20) it has fallen ${formatPct(
                maxLoss / cap,
              )} on average, which on that amount is ${formatMoney(maxLoss, currency)}.`
            : 'Enter an amount to see a size.'}
        </p>
      </div>
    </section>
  )
}

/** The numbers translated into money: what a typical day, a bad day and the next months look like. */
function MeaningPanel({
  analysis,
  history,
  range,
  onRange,
}: {
  analysis: AnalyzeResponse
  history: PriceBar[] | undefined
  range: AnalysisRange
  onRange: (range: AnalysisRange) => void
}) {
  const [amount, setAmount] = useState(10_000)
  const amountId = useId()
  const cards = meaningCards(analysis, amount)
  const projection = preferredProjection(analysis)
  return (
    <section className="panel chart-panel" aria-labelledby="meaning-heading">
      <div className="section-heading">
        <div>
          <p className="eyebrow">From {yearsCovered(analysis)} years of daily prices</p>
          <h3 id="meaning-heading">What this means for your money</h3>
          <div className="range-picker history-picker" role="group" aria-label="History used">
            {(['10y', '5y'] as const).map((option) => (
              <button
                key={option}
                type="button"
                className="ghost-button"
                aria-pressed={option === range}
                onClick={() => onRange(option)}
              >
                {option === '10y' ? '10 years' : '5 years'}
              </button>
            ))}
          </div>
        </div>
        <div className="amount-input">
          <label htmlFor={amountId}>If you put in</label>
          <input
            id={amountId}
            type="number"
            min={0}
            step={1000}
            inputMode="numeric"
            value={amount}
            onChange={(event) => setAmount(Math.max(0, Number(event.target.value) || 0))}
          />
        </div>
      </div>
      <ul className="meaning-cards" data-testid="meaning-cards">
        {cards.map((card) => (
          <li key={card.id} className="meaning-card" data-card={card.id}>
            <h4>{card.title}</h4>
            <p>{card.headline}</p>
            <p className="quiet-copy">{card.why}</p>
          </li>
        ))}
      </ul>
      {projection ? (
        <>
          <h4 className="advice-subhead">Range of outcomes over the next three months</h4>
          <p className="quiet-copy">
            The middle line is the typical path; half of the simulated futures stay between the dotted lines and 9 in 10
            between the dashed ones.
          </p>
          <FanChart projection={projection} report={analysis.report} history={history} />
        </>
      ) : null}
    </section>
  )
}

/** Every rule at every time frame, with the test numbers. */
function SignalsPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const { signals } = analysis
  if (!signals) return null
  return (
    <section className="panel chart-panel" aria-labelledby="signals-heading">
      <p className="eyebrow">
        Walk-forward tests · {signals.first_date} → {signals.last_date} · Holm-corrected over {signals.n_tests} tests ·
        cost {formatPct(signals.round_trip_cost, 2)} a round trip
      </p>
      <h3 id="signals-heading">Trading rules, tested on this instrument</h3>
      <table className="event-table data-table" data-testid="signals-table">
        <caption className="visually-hidden">Trading rule backtests</caption>
        <thead>
          <tr>
            <th scope="col">Rule</th>
            <th scope="col">Time frame</th>
            <th scope="col">Effect per call</th>
            <th scope="col">Hit rate</th>
            <th scope="col">Adjusted p</th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          {signals.signals.flatMap((signal) =>
            signal.tests.map((t) => (
              <tr key={`${signal.id}-${t.horizon_days}`}>
                <th scope="row">{signal.name}</th>
                <td>{horizonWords(t.horizon_days)}</td>
                <td>{formatPct(t.effect, 2)}</td>
                <td>{t.hit_rate === null ? '—' : `${formatPct(t.hit_rate, 0)} of ${t.hit_n}`}</td>
                <td>{formatP(t.p_holm)}</td>
                <td>{t.verdict}</td>
              </tr>
            )),
          )}
        </tbody>
      </table>
      <ul className="plain-list quiet-copy">
        {signals.signals.map((signal) => (
          <li key={signal.id}>
            <strong>{signal.name}:</strong> {signal.rule} ({signal.source})
          </li>
        ))}
      </ul>
    </section>
  )
}

function ForecastPanel({ analysis }: { analysis: AnalyzeResponse }) {
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

const VALIDATION_LABEL: Record<string, string> = {
  historical_sim: 'Historical simulation (baseline)',
  ewma_normal: 'EWMA, normal (RiskMetrics)',
  garch_t: 'GARCH-t',
}

/** Out-of-sample scores: does any model beat the baseline on days it never saw? */
function ValidationPanel({ analysis }: { analysis: AnalyzeResponse }) {
  const v = analysis.report.validation
  return (
    <section className="panel chart-panel" aria-labelledby="validation-heading">
      <p className="eyebrow">Out-of-sample validation · 1-day forecasts</p>
      <h3 id="validation-heading">Does any model earn its place?</h3>
      {v ? (
        <>
          <p className="quiet-copy">
            Walk-forward over {v.n_forecasts.toLocaleString()} trading days: each forecast used only earlier data. Loss is
            the pinball loss across seven quantiles (lower is better), compared with historical simulation by a
            Diebold–Mariano test. VaR passes when the Kupiec and Christoffersen tests both hold at 5%.
          </p>
          <table className="event-table data-table" data-testid="validation-table">
            <caption className="visually-hidden">Out-of-sample model scores</caption>
            <thead>
              <tr>
                <th scope="col">Model</th>
                <th scope="col">Loss vs baseline</th>
                <th scope="col">90% interval hit rate</th>
                <th scope="col">99% VaR breaches</th>
                <th scope="col">95% VaR breaches</th>
                <th scope="col">Direction</th>
                <th scope="col">Verdict</th>
              </tr>
            </thead>
            <tbody>
              {v.models.map((m) => {
                const base = v.models.find((x) => x.model === v.baseline)
                const delta = base ? m.mean_pinball / base.mean_pinball - 1 : 0
                const verdictTone = m.verdict.startsWith('Earns') ? 'green' : m.vs_baseline ? 'neutral' : 'blue'
                return (
                  <tr key={m.model}>
                    <th scope="row">{VALIDATION_LABEL[m.model] ?? m.model}</th>
                    <td>
                      {m.vs_baseline
                        ? `${delta > 0 ? '+' : ''}${formatPct(delta)} (p ${formatP(m.vs_baseline.p_value)})`
                        : 'reference'}
                    </td>
                    <td>{formatPct(m.coverage_90)}</td>
                    {m.var_backtests.map((b) => (
                      <td key={b.level}>
                        {b.exceptions} vs {Math.round(b.expected_rate * v.n_forecasts)} expected{' '}
                        <span className={`status-chip tone-${b.pass ? 'green' : 'red'}`}>{b.pass ? 'pass' : 'fail'}</span>
                      </td>
                    ))}
                    <td>
                      {m.direction
                        ? `${formatPct(m.direction.accuracy)} (p ${formatP(m.direction.binomial.p_value)})`
                        : 'none forecast'}
                    </td>
                    <td>
                      <span className={`tone-${verdictTone}`}>{m.verdict}</span>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </>
      ) : (
        <p className="quiet-copy">
          Validation needs at least 600 daily returns (two years to warm up, then 100 out-of-sample days). Pick a longer
          history.
        </p>
      )}
    </section>
  )
}
