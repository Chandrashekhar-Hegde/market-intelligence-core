// Panels you can arrange: collapse, widen or narrow (two columns on wide screens), move up or
// down, and resize vertically with the native handle. The layout is remembered per browser.
// ponytail: buttons and CSS, no drag-and-drop library; add react-grid-layout if dragging
// panels around is wanted.
import { type ReactNode, useEffect, useRef, useState } from 'react'

import { orderedIds } from './marketData'

export type ModuleDef = { id: string; title: string; node: ReactNode }

type Layout = { order: string[]; collapsed: string[]; narrow: string[]; heights: Record<string, number> }

const EMPTY: Layout = { order: [], collapsed: [], narrow: [], heights: {} }

function load(key: string): Layout {
  try {
    const raw = window.localStorage.getItem(key)
    return raw ? { ...EMPTY, ...(JSON.parse(raw) as Partial<Layout>) } : EMPTY
  } catch {
    return EMPTY
  }
}

function save(key: string, layout: Layout) {
  try {
    window.localStorage.setItem(key, JSON.stringify(layout))
  } catch {
    // Private windows and blocked storage: the layout just isn't remembered.
  }
}


function Frame({
  module,
  layout,
  first,
  last,
  onChange,
}: {
  module: ModuleDef
  layout: Layout
  first: boolean
  last: boolean
  onChange: (update: (l: Layout) => Layout) => void
}) {
  const ref = useRef<HTMLDivElement | null>(null)
  const collapsed = layout.collapsed.includes(module.id)
  const narrow = layout.narrow.includes(module.id)
  const height = layout.heights[module.id]
  const toggle = (list: 'collapsed' | 'narrow') => () =>
    onChange((l) => ({
      ...l,
      [list]: l[list].includes(module.id) ? l[list].filter((x) => x !== module.id) : [...l[list], module.id],
    }))
  const move = (by: number) => () =>
    onChange((l) => {
      const order = [...l.order]
      const i = order.indexOf(module.id)
      const j = i + by
      if (i < 0 || j < 0 || j >= order.length) return l
      ;[order[i], order[j]] = [order[j]!, order[i]!]
      return { ...l, order }
    })

  // The native resize handle sets an inline height; remember it.
  useEffect(() => {
    const el = ref.current
    if (!el || typeof ResizeObserver === 'undefined') return undefined
    const observer = new ResizeObserver(() => {
      const h = Number.parseInt(el.style.height, 10)
      if (Number.isFinite(h) && h > 0 && h !== layout.heights[module.id]) {
        onChange((l) => ({ ...l, heights: { ...l.heights, [module.id]: h } }))
      }
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [layout.heights, module.id, onChange])

  return (
    <div className={`module${narrow ? ' module-narrow' : ''}`} data-module={module.id}>
      <div className="module-bar">
        <span className="module-title">{module.title}</span>
        <span className="module-tools">
          <button type="button" className="ghost-button" aria-label={`Move ${module.title} up`} disabled={first} onClick={move(-1)}>
            ↑
          </button>
          <button type="button" className="ghost-button" aria-label={`Move ${module.title} down`} disabled={last} onClick={move(1)}>
            ↓
          </button>
          <button
            type="button"
            className="ghost-button module-width"
            aria-label={`${narrow ? 'Widen' : 'Narrow'} ${module.title}`}
            onClick={toggle('narrow')}
          >
            {narrow ? 'Widen' : 'Narrow'}
          </button>
          <button
            type="button"
            className="ghost-button"
            aria-label={`${collapsed ? 'Show' : 'Hide'} ${module.title}`}
            aria-expanded={!collapsed}
            onClick={toggle('collapsed')}
          >
            {collapsed ? 'Show' : 'Hide'}
          </button>
        </span>
      </div>
      {collapsed ? null : (
        <div
          ref={ref}
          className="module-body"
          // Resizable means scrollable, and a scrollable region must be reachable by keyboard.
          role="region"
          aria-label={`${module.title} panel`}
          tabIndex={0}
          style={height ? { height } : undefined}
          data-testid={`module-${module.id}`}
        >
          {module.node}
        </div>
      )}
    </div>
  )
}

export function ModularLayout({ page, modules }: { page: string; modules: ModuleDef[] }) {
  const key = `layout:${page}`
  const [layout, setLayout] = useState<Layout>(() => load(key))
  const ids = orderedIds(
    modules.map((m) => m.id),
    layout.order,
  )
  const current = { ...layout, order: ids }
  const onChange = (update: (l: Layout) => Layout) =>
    setLayout((l) => {
      const next = update({ ...l, order: orderedIds(modules.map((m) => m.id), l.order) })
      save(key, next)
      return next
    })
  const byId = Object.fromEntries(modules.map((m) => [m.id, m]))
  const changed = layout.order.length + layout.collapsed.length + layout.narrow.length + Object.keys(layout.heights).length > 0
  return (
    <div className="modular">
      {changed ? (
        <p className="modular-reset">
          <button
            type="button"
            className="ghost-button"
            onClick={() => {
              save(key, EMPTY)
              setLayout(EMPTY)
            }}
          >
            Reset layout
          </button>
        </p>
      ) : null}
      {ids.map((id, i) => (
        <Frame key={id} module={byId[id]!} layout={current} first={i === 0} last={i === ids.length - 1} onChange={onChange} />
      ))}
    </div>
  )
}
