import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { LineChart, ListChecks, Newspaper, Scale } from 'lucide-react'

import { type HealthResponse, fetchHealth, getApiBaseUrl } from './api'
import { Compare } from './Compare'
import { Desk } from './Desk'
import { Guide } from './Guide'
import { Markets } from './Markets'

type View = 'markets' | 'desk' | 'compare' | 'guide'

const TABS: { view: View; label: string; Icon: typeof LineChart }[] = [
  { view: 'markets', label: 'Markets', Icon: LineChart },
  { view: 'desk', label: 'News desk', Icon: Newspaper },
  { view: 'compare', label: 'Compare', Icon: Scale },
  { view: 'guide', label: 'How it works', Icon: ListChecks },
]

function App() {
  const [view, setView] = useState<View>('markets')
  const health = useQuery<HealthResponse>({
    queryKey: ['health'],
    queryFn: fetchHealth,
    retry: false,
    refetchInterval: 30_000,
  })
  const online = health.isSuccess && health.data.status === 'ok'

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="Primary">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            MI
          </div>
          <div>
            <p className="eyebrow">Private beta</p>
            <h1>Market Intelligence</h1>
          </div>
        </div>

        <div className="view-tabs" role="tablist" aria-label="View">
          {TABS.map(({ view: tab, label, Icon }) => (
            <button key={tab} type="button" role="tab" aria-selected={view === tab} onClick={() => setView(tab)}>
              <Icon size={18} aria-hidden="true" />
              {label}
            </button>
          ))}
        </div>

        <div className={`status-box tone-${health.isPending ? 'neutral' : online ? 'green' : 'red'}`}>
          <span className="status-dot" aria-hidden="true" />
          <div>
            <strong>{health.isPending ? 'Checking' : online ? 'Online' : 'Offline'}</strong>
            {import.meta.env.DEV ? <span>{getApiBaseUrl()}</span> : null}
          </div>
        </div>
      </aside>

      <main className="workspace">
        {view === 'markets' ? <Markets /> : view === 'desk' ? <Desk /> : view === 'compare' ? <Compare /> : <Guide />}
        <p className="disclaimer" role="note">
          Ranges and calls come from past data and are shown with their evidence. They are not
          personal financial advice, and past patterns can stop working.
        </p>
      </main>
    </div>
  )
}

export default App
