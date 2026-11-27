// Today: every instrument scanned, the few real calls first, the chances for everything, and a
// risk-balanced mix to consider. Research signals for a private beta, not registered advice.
import { useState } from 'react'

import { type TodayItem, describeApiError, useDesk, useNow, useToday } from './api'
import { QUICK_PICKS, formatPct, timeAgo } from './marketData'
import { chanceWords, formatMoney, riskBalancedMix } from './plainLanguage'

const GROUP_ORDER = ['India index', 'NIFTY 50', 'Gold, silver, rupee', 'US from India', 'AI', 'Your watchlist']
const PRIVATE_AI = /\b(groq|openai|xai|anthropic)\b/i

function label(item: TodayItem) {
  return item.name ?? item.symbol
}

function callText(item: TodayItem): string {
  const a = item.advice
  if (!a) return 'No call: too little history'
  if (a.stance === 'hold') return a.basis === 'proven_but_quiet' ? 'Hold (proven rule, quiet today)' : 'Hold'
  const verb = a.stance === 'buy' ? 'Buy' : 'Sell'
  return a.basis === 'news_driven' ? `${verb} (today’s news, no track record)` : verb
}

function honesty(item: TodayItem): string {
  const c = item.chance_check_3m
  if (!c) return ''
  if (c.vs_coin.reject_5pct && c.vs_coin.statistic < 0) return 'held up in the past'
  if (c.vs_coin.reject_5pct) return 'worse than a coin flip in the past'
  return `history said ${Math.round(c.mean_forecast * 100)}, rose ${Math.round(c.realised * 100)}`
}

function CallsSection({ items }: { items: TodayItem[] }) {
  const calls = items.filter((i) => i.advice && i.advice.stance !== 'hold')
  const conflicts = items.filter((i) => i.advice?.basis === 'news_conflict')
  return (
    <section className="panel chart-panel" aria-labelledby="today-calls" data-testid="today-calls">
      <h3 id="today-calls">Today’s calls</h3>
      {calls.length === 0 ? (
        <p>
          <strong>No Buy or Sell today.</strong> None of the {items.length} instruments has a rule that has proven itself
          on its own past and points one way today, and no news is strong enough on its own. That is the normal state of
          liquid markets: the rest of this page gives the chances instead.
        </p>
      ) : (
        <ul className="today-calls">
          {calls.map((i) => (
            <li key={i.symbol}>
              <span className={`status-chip tone-${i.advice!.stance === 'buy' ? 'green' : 'red'}`}>{callText(i)}</span>{' '}
              <strong>{label(i)}</strong>{' '}
              {i.advice!.horizon_days ? <span className="quiet-copy">for about {i.advice!.horizon_days} sessions. </span> : null}
              {i.advice!.confidence ? (
                <span className="quiet-copy">
                  Right {Math.round(i.advice!.confidence.hit_rate * 100)} times in 100 over {i.advice!.confidence.n} past
                  calls.{' '}
                </span>
              ) : (
                <span className="quiet-copy">Chance not measured yet. </span>
              )}
              <span>{i.advice!.headline}</span>
            </li>
          ))}
        </ul>
      )}
      {conflicts.length ? (
        <p className="quiet-copy">
          Held back by news pointing the other way: {conflicts.map(label).join(', ')}.
        </p>
      ) : null}
    </section>
  )
}

function ScanTable({ items }: { items: TodayItem[] }) {
  const groups = GROUP_ORDER.filter((g) => items.some((i) => i.group === g))
  return (
    <section className="panel chart-panel" aria-labelledby="today-all">
      <h3 id="today-all">Everything scanned, with its chances</h3>
      <p className="quiet-copy">
        Chances come from each instrument’s own last 10 years, replayed many times (or the volatility model where it
        tested better). They lean on its past average growth, so the last column says whether that held up before.
      </p>
      <div className="table-scroll">
        <table className="event-table data-table" data-testid="today-table">
          <caption className="visually-hidden">Every instrument scanned today</caption>
          <thead>
            <tr>
              <th scope="col">Instrument</th>
              <th scope="col">Today</th>
              <th scope="col">Call</th>
              <th scope="col">Chance higher in 3 months</th>
              <th scope="col">Chance down &gt; 10%</th>
              <th scope="col">Did such chances hold up?</th>
            </tr>
          </thead>
          {groups.map((g) => (
            <tbody key={g}>
              <tr className="group-row">
                <th scope="rowgroup" colSpan={6}>
                  {g}
                </th>
              </tr>
              {items
                .filter((i) => i.group === g)
                .map((i) => (
                  <tr key={i.symbol}>
                    <th scope="row">
                      {label(i)} <span className="quiet-copy mono">{i.symbol}</span>
                    </th>
                    {i.error ? (
                      <td colSpan={5} className="quiet-copy">
                        Unavailable: {i.error}
                      </td>
                    ) : (
                      <>
                        <td>{i.change_pct === null || i.change_pct === undefined ? '—' : formatPct(i.change_pct / 100)}</td>
                        <td>{callText(i)}</td>
                        <td>{i.prob_gain_3m === null || i.prob_gain_3m === undefined ? '—' : chanceWords(i.prob_gain_3m)}</td>
                        <td>
                          {i.prob_loss_10pct_3m === null || i.prob_loss_10pct_3m === undefined
                            ? '—'
                            : chanceWords(i.prob_loss_10pct_3m)}
                        </td>
                        <td className="quiet-copy">{honesty(i)}</td>
                      </>
                    )}
                  </tr>
                ))}
            </tbody>
          ))}
        </table>
      </div>
    </section>
  )
}

function UsFromIndia({ items }: { items: TodayItem[] }) {
  const rupee = items.find((i) => i.symbol === 'INR=X')
  const fx = rupee?.return_1y ?? null
  const us = items.filter((i) => i.currency === 'USD' && i.instrument_type === 'EQUITY')
  return (
    <section className="panel chart-panel" aria-labelledby="today-us" data-testid="today-us">
      <h3 id="today-us">Investing in the US from India</h3>
      <ul className="plain-list">
        <li>
          <strong>Without sending money abroad:</strong> Indian-listed US funds and ETFs on NSE (for example the Nasdaq
          100 ETF, MON100). When these funds hit RBI’s overseas-investment limit they can trade above their real
          value, so check the premium before buying.
        </li>
        <li>
          <strong>Through an Indian app’s US partner broker</strong> (offered by several Indian investing apps; check
          what yours offers today), or <strong>NSE IX in GIFT City</strong>. Both use your Liberalised Remittance
          Scheme allowance.
        </li>
        <li>
          <strong>Rules (check the latest notification before acting):</strong> up to $250,000 per person per financial
          year under LRS; TCS of 20% on remittances above ₹10 lakh a year (from 1 Apr 2026); gains held up to 24 months
          taxed at your slab rate, after that at 12.5%.
        </li>
      </ul>
      {fx !== null ? (
        <p>
          Over the last year the dollar {fx >= 0 ? 'gained' : 'lost'} {formatPct(Math.abs(fx))} against the rupee, so
          a US share’s return in rupees was about that much {fx >= 0 ? 'higher' : 'lower'} than in dollars.
        </p>
      ) : null}
      {us.length && fx !== null ? (
        <table className="event-table data-table" data-testid="us-inr-table">
          <caption className="visually-hidden">US shares: last year in dollars and in rupees</caption>
          <thead>
            <tr>
              <th scope="col">Share</th>
              <th scope="col">Last year in $</th>
              <th scope="col">In ₹</th>
            </tr>
          </thead>
          <tbody>
            {us.map((i) => (
              <tr key={i.symbol}>
                <th scope="row">{label(i)}</th>
                <td>{i.return_1y === null || i.return_1y === undefined ? '—' : formatPct(i.return_1y)}</td>
                <td>
                  {i.return_1y === null || i.return_1y === undefined ? '—' : formatPct((1 + i.return_1y) * (1 + fx) - 1)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  )
}

function AiTheme({ items }: { items: TodayItem[] }) {
  const now = useNow(60_000)
  const desk = useDesk()
  const listed = items.filter((i) => i.group === 'AI' && !i.error)
  const privateNews = (desk.data?.stories ?? []).filter((s) => PRIVATE_AI.test(s.title)).slice(0, 6)
  return (
    <section className="panel chart-panel" aria-labelledby="today-ai" data-testid="today-ai">
      <h3 id="today-ai">AI: where it is going</h3>
      <p className="quiet-copy">
        Listed AI companies have prices, so they get chances like everything else. Private ones (Groq, OpenAI, xAI,
        Anthropic) can’t be bought on an exchange: only their news is shown.
      </p>
      {listed.length ? (
        <ul className="plain-list">
          {listed.map((i) => (
            <li key={i.symbol}>
              <strong>{label(i)}</strong>:{' '}
              {i.prob_gain_3m !== null && i.prob_gain_3m !== undefined
                ? `${chanceWords(i.prob_gain_3m)} chance higher in 3 months`
                : 'no forecast'}
              {i.return_1y !== null && i.return_1y !== undefined ? `; ${formatPct(i.return_1y)} over the last year` : ''}.
            </li>
          ))}
        </ul>
      ) : null}
      <h4 className="advice-subhead">Private AI companies in the news</h4>
      {privateNews.length === 0 ? (
        <p className="quiet-copy">No stories about them in today’s feeds.</p>
      ) : (
        <ul className="news-list">
          {privateNews.map((s) => (
            <li key={s.url}>
              <a href={s.url} target="_blank" rel="noopener noreferrer">
                {s.title}
              </a>{' '}
              <span className="quiet-copy">
                {s.publisher} · {timeAgo(s.seen_at, now)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function Mix({ items }: { items: TodayItem[] }) {
  const usable = items.filter((i) => !i.error && (i.vol_annual ?? 0) > 0)
  const [picked, setPicked] = useState<string[]>(['^NSEI', 'GOLDBEES.NS', 'MON100.NS'])
  const [budget, setBudget] = useState(100_000)
  const chosen = usable.filter((i) => picked.includes(i.symbol))
  const mix = riskBalancedMix(
    chosen.map((i) => ({ symbol: i.symbol, vol_annual: i.vol_annual ?? 0 })),
    budget,
  )
  const name = Object.fromEntries(usable.map((i) => [i.symbol, label(i)]))
  return (
    <section className="panel chart-panel" aria-labelledby="today-mix" data-testid="today-mix">
      <h3 id="today-mix">A mix to consider</h3>
      <p className="quiet-copy">
        With no proven edge today, the evidence-based way to combine holdings is to balance their risk: put less in
        the bumpy ones so each moves your total about equally. It is a starting point for you to decide on, not an
        order.
      </p>
      <label className="amount-input">
        Amount to spread{' '}
        <input
          type="number"
          min={0}
          step={10000}
          inputMode="numeric"
          value={budget}
          onChange={(e) => setBudget(Math.max(0, Number(e.target.value) || 0))}
        />
      </label>
      <fieldset className="mix-picks">
        <legend>Holdings</legend>
        {usable
          .filter((i) => i.group !== 'NIFTY 50' || picked.includes(i.symbol))
          .map((i) => (
            <label key={i.symbol}>
              <input
                type="checkbox"
                checked={picked.includes(i.symbol)}
                onChange={(e) =>
                  setPicked((p) => (e.target.checked ? [...p, i.symbol] : p.filter((s) => s !== i.symbol)))
                }
              />{' '}
              {label(i)}
            </label>
          ))}
      </fieldset>
      {mix.length ? (
        <ul className="plain-list" data-testid="mix-result">
          {mix.map((m) => (
            <li key={m.symbol}>
              <strong>{name[m.symbol]}</strong>: {formatMoney(m.amount, 'INR')} ({Math.round(m.weight * 100)}%)
            </li>
          ))}
        </ul>
      ) : (
        <p className="quiet-copy">Pick at least one holding.</p>
      )}
    </section>
  )
}

export function Today() {
  const today = useToday(QUICK_PICKS.map((p) => p.symbol))
  if (today.error) {
    return (
      <section className="panel chart-panel" aria-labelledby="today-heading">
        <h2 id="today-heading">Today</h2>
        <p className="error-state" role="alert">
          Today’s list is unavailable ({describeApiError(today.error)}).
        </p>
      </section>
    )
  }
  if (!today.data) {
    return (
      <section className="panel chart-panel" aria-labelledby="today-heading">
        <h2 id="today-heading">Today</h2>
        <p className="empty-state">Scanning about 70 instruments (the first scan of the day takes up to a minute)…</p>
      </section>
    )
  }
  const { items, notice } = today.data
  return (
    <>
      <section className="panel chart-panel" aria-labelledby="today-heading">
        <p className="eyebrow">{items.length} instruments · 10 years each</p>
        <h2 id="today-heading">Today: what to do, and the chances</h2>
        <p className="notice" role="note" data-testid="today-notice">
          {notice}
        </p>
      </section>
      <CallsSection items={items} />
      <ScanTable items={items} />
      <UsFromIndia items={items} />
      <AiTheme items={items} />
      <Mix items={items} />
    </>
  )
}
