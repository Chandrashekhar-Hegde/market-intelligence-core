// The market news desk: today's market news by theme, each headline read for what it usually
// means for gold, the rupee, banks, IT, oil marketers and US shares, and where the news leans.
import { describeApiError, useDesk, useNow } from './api'
import { timeAgo } from './marketData'

const THEMES = ['Rates', 'Currency', 'Commodities', 'Flows', 'AI & tech', 'Market report', 'Other']

const THEME_NOTE: Record<string, string> = {
  Rates: 'RBI and Fed policy',
  Currency: 'Rupee and dollar',
  Commodities: 'Crude, gold, silver',
  Flows: 'Foreign investors',
  'AI & tech': 'Listed and private AI companies (private ones are news only: no share price)',
  'Market report': 'Reports of index moves: shown, never counted',
  Other: 'Everything else',
}

export function Desk() {
  const now = useNow(60_000)
  const desk = useDesk()
  if (desk.error) {
    return (
      <section className="panel chart-panel" aria-labelledby="desk-heading">
        <h2 id="desk-heading">News desk</h2>
        <p className="error-state" role="alert">
          The news desk is unavailable ({describeApiError(desk.error)}).
        </p>
      </section>
    )
  }
  if (!desk.data) {
    return (
      <section className="panel chart-panel" aria-labelledby="desk-heading">
        <h2 id="desk-heading">News desk</h2>
        <p className="empty-state">Reading today’s market news…</p>
      </section>
    )
  }
  const { stories, leans, sources } = desk.data
  const label = Object.fromEntries(leans.map((l) => [l.asset, l.label]))
  const moving = leans.filter((l) => l.lean !== 0)
  return (
    <>
      <section className="panel chart-panel" aria-labelledby="desk-heading" data-testid="desk">
        <p className="eyebrow">
          {stories.length} stories · as of {timeAgo(desk.data.as_of, now)} · research signals, not advice
        </p>
        <h2 id="desk-heading">Where today’s market news leans</h2>
        {moving.length === 0 ? (
          <p className="quiet-copy">Nothing in today’s market news points clearly one way.</p>
        ) : (
          <ul className="desk-leans" data-testid="desk-leans">
            {moving.map((l) => (
              <li key={l.asset}>
                <span className={`status-chip tone-${l.lean > 0 ? 'green' : 'red'}`}>
                  {l.lean > 0 ? 'Leans up' : 'Leans down'}
                </span>{' '}
                <strong>{l.label}</strong>{' '}
                <span className="quiet-copy">
                  ({l.drivers.length} {l.drivers.length === 1 ? 'story' : 'stories'}, e.g. “{stories[l.drivers[0] ?? 0]?.title}”)
                </span>
              </li>
            ))}
          </ul>
        )}
        <p className="quiet-copy">
          Each headline is matched to the usual effect of its theme (a dearer crude weighs on oil marketers and the
          rupee; a weaker rupee helps IT exporters; an RBI cut helps banks). Many outlets covering one event count once,
          and stories fade over two days. These weights are untested judgement: the app is recording outcomes to test
          them, so treat a lean as a pointer to read, not a reason to trade.
        </p>
        <p className="quiet-copy news-sources" data-testid="desk-sources">
          Sources:{' '}
          {sources.map((source, i) => (
            <span key={source.name} title={source.error ?? undefined}>
              {i > 0 ? ' · ' : ''}
              {source.name} {source.ok ? `✓ ${source.count}` : '✗ unavailable'}
            </span>
          ))}
        </p>
      </section>
      {THEMES.map((theme) => {
        const items = stories.filter((s) => s.theme === theme)
        if (items.length === 0) return null
        const id = `desk-${theme.replace(/\W+/g, '-').toLowerCase()}`
        return (
          <section key={theme} className="panel chart-panel" aria-labelledby={id} data-theme={theme}>
            <p className="eyebrow">{THEME_NOTE[theme]}</p>
            <h3 id={id}>
              {theme} <span className="quiet-copy">({items.length})</span>
            </h3>
            <ul className="news-list">
              {items.slice(0, 15).map((story) => (
                <li key={story.url}>
                  <a href={story.url} target="_blank" rel="noopener noreferrer">
                    {story.title}
                  </a>
                  <span className="quiet-copy">
                    {' '}
                    {story.publisher}
                    {story.source !== story.publisher ? ` via ${story.source}` : ''} · {timeAgo(story.seen_at, now)}
                  </span>
                  {story.impacts.length ? (
                    <p className="news-reading">
                      {story.impacts.map((i) => (
                        <span key={i.asset} className={`status-chip tone-${i.direction > 0 ? 'green' : 'red'}`}>
                          {label[i.asset] ?? i.asset} {i.direction > 0 ? '↑' : '↓'}
                        </span>
                      ))}{' '}
                      {story.impacts[0]?.reason}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
          </section>
        )
      })}
    </>
  )
}
