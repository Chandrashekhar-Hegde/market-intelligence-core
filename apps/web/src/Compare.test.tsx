import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import type { CompareResponse } from './api'
import { Compare } from './Compare'
import { correlationColor } from './marketData'

function comparison(symbols: string[], notes: string[] = []): CompareResponse {
  const k = symbols.length
  const m = (base: number) =>
    symbols.map((_, i) => symbols.map((__, j) => (i === j ? 1 : base + 0.01 * (i + j))))
  const pairs = []
  for (let i = 0; i < k; i++) {
    for (let j = i + 1; j < k; j++) {
      const tied = i === 0 && j === 1
      pairs.push({
        a: symbols[i]!,
        b: symbols[j]!,
        correlation_daily: 0.5,
        correlation_weekly: 0.6,
        cointegration: {
          dependent: symbols[i]!,
          regressor: symbols[j]!,
          hedge_ratio: 1.1,
          statistic: tied ? -3.9 : -1.2,
          lags: 22,
          critical_values: [-3.9, -3.34, -3.05] as [number, number, number],
          cointegrated_5pct: tied,
          half_life_days: tied ? 18 : null,
          spread_z_now: 1.4,
        },
      })
    }
  }
  return {
    comparison: {
      symbols,
      first_date: '2021-11-26',
      last_date: '2026-11-20',
      n_common_days: 1114,
      n_weekly: 260,
      correlation_daily: m(0.3),
      spearman_daily: m(0.28),
      correlation_weekly: m(0.4),
      covariance_shrinkage: 0.023,
      covariance_annual: m(0.01),
      pca: { explained: [0.62, 0.25, 0.13], first_loadings: symbols.map(() => 0.5) },
      pairs,
      rolling_correlation: [],
      performance: symbols.map((symbol, i) => ({
        symbol,
        total_return: 0.5 + i,
        annual_return: 0.1,
        annual_vol: 0.25,
        sharpe: 0.6 - i * 0.1,
        max_drawdown: 0.3,
      })),
      rebased: [
        { date: '2021-11-26', values: symbols.map(() => 100) },
        { date: '2026-11-20', values: symbols.map((_, i) => 150 + i) },
      ],
      findings: ['1114 trading days in common.', `Most correlated: ${symbols[0]} and ${symbols[1]}.`],
    },
    instruments: symbols.map((symbol) => ({
      symbol, name: symbol, currency: 'USD', exchange: 'NMS', timezone: 'America/New_York', source: 'yahoo-chart-v8', fetched_at: '2026-11-25T07:00:00Z',
    })),
    range: '5y',
    notes,
  }
}

function mockApi(handler?: (symbols: string[]) => Response) {
  const fetchMock = vi.fn<typeof fetch>(async (input) => {
    const url = new URL(input.toString())
    if (url.pathname === '/api/v1/compare') {
      const symbols = (url.searchParams.get('symbols') ?? '').split(',')
      return handler?.(symbols) ?? Response.json(comparison(symbols))
    }
    if (url.pathname === '/api/v1/instruments/search') {
      return Response.json({
        results: [{ symbol: '7203.T', name: 'Toyota Motor Corporation', exchange: 'Tokyo', country: 'Japan', quote_type: 'EQUITY' }],
        markets: [{ country: 'Japan', exchange: 'Tokyo', count: 1 }],
      })
    }
    return new Response('not found', { status: 404 })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function renderCompare() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Compare />
    </QueryClientProvider>,
  )
}

afterEach(() => vi.unstubAllGlobals())

describe('Compare view', () => {
  test('compares the default set: findings, performance, heatmap, pairs and PCA', async () => {
    const fetchMock = mockApi()
    renderCompare()
    expect(await screen.findByTestId('compare-findings')).toHaveTextContent('Most correlated: RELIANCE.NS and TCS.NS')
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/compare?symbols=RELIANCE.NS%2CTCS.NS%2C%5ENSEI&range=5y'), expect.anything())

    const perf = screen.getByTestId('performance-table')
    expect(within(perf).getByRole('row', { name: /^RELIANCE\.NS/ })).toHaveTextContent('50.0%')
    // Same time zone → daily correlation by default.
    expect(screen.getByRole('button', { name: 'Daily' })).toHaveAttribute('aria-pressed', 'true')
    const heatmap = screen.getByTestId('heatmap')
    expect(within(heatmap).getByRole('row', { name: /^RELIANCE\.NS/ })).toHaveTextContent('1.000.310.32')
    await userEvent.click(screen.getByRole('button', { name: 'Weekly' }))
    expect(within(heatmap).getByRole('row', { name: /^RELIANCE\.NS/ })).toHaveTextContent('1.000.410.42')

    const pairs = screen.getByTestId('pairs-table')
    const tied = within(pairs).getByRole('row', { name: /RELIANCE\.NS \/ TCS\.NS/ })
    expect(tied).toHaveTextContent('cointegrated')
    expect(tied).toHaveTextContent('18 days')
    expect(within(pairs).getByRole('row', { name: /TCS\.NS \/ \^NSEI/ })).toHaveTextContent('not tied')
    expect(screen.getByTestId('pca')).toHaveTextContent('Factor 1: 62.0%')
  })

  test('leads with a plain side-by-side and what owning them together means', async () => {
    mockApi()
    renderCompare()
    const plain = await screen.findByTestId('plain-compare')
    const table = within(plain).getByTestId('head-to-head')
    expect(within(table).getByRole('row', { name: /^RELIANCE\.NS/ })).toHaveTextContent('RELIANCE.NS10.0%±25%−30%')
    const findings = within(plain).getByTestId('plain-compare-findings')
    // Weekly-free fixture: daily correlation 0.5 is "somewhat" on the Evans (1996) scale.
    expect(findings).toHaveTextContent('RELIANCE.NS and TCS.NS move somewhat together (correlation 0.50).')
    expect(findings).toHaveTextContent(/Holding all 3 in equal amounts swings about ±\d+% a year/)
    expect(findings).toHaveTextContent('The deepest fall was')
  })

  test('different time zones: the caveat is shown and weekly correlation is the default', async () => {
    mockApi((symbols) =>
      Response.json(comparison(symbols, ['These markets close at different times of day. Daily correlations are biased down; use the weekly figures.'])),
    )
    renderCompare()
    expect(await screen.findByRole('note')).toHaveTextContent('different times of day')
    expect(screen.getByRole('button', { name: 'Weekly' })).toHaveAttribute('aria-pressed', 'true')
  })

  test('chips: add from search, remove, and fewer than two stops the request', async () => {
    const fetchMock = mockApi()
    renderCompare()
    await screen.findByTestId('compare-findings')

    await userEvent.type(screen.getByLabelText(/add an instrument/i), 'toyota')
    await userEvent.click(await screen.findByRole('button', { name: /7203\.T/ }))
    expect(screen.getByRole('list', { name: 'Selected instruments' })).toHaveTextContent('7203.T')
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('symbols=RELIANCE.NS%2CTCS.NS%2C%5ENSEI%2C7203.T'), expect.anything()),
    )

    for (const s of ['RELIANCE.NS', 'TCS.NS', '^NSEI']) {
      await userEvent.click(screen.getByRole('button', { name: `Remove ${s}` }))
    }
    expect(screen.getByText('Pick at least two instruments to compare.')).toBeInTheDocument()
  })

  test('errors from the API are readable', async () => {
    mockApi(() => Response.json({ error: 'NOPE.NS: symbol not found' }, { status: 404 }))
    renderCompare()
    expect(await screen.findByRole('alert')).toHaveTextContent('Unknown symbol')
  })
})

describe('correlationColor', () => {
  test('teal for positive, red for negative, stronger for larger magnitude', () => {
    expect(correlationColor(1)).toBe('rgba(31, 122, 104, 0.500)')
    expect(correlationColor(-0.5)).toBe('rgba(180, 68, 58, 0.250)')
    expect(correlationColor(Number.NaN)).toBe('transparent')
  })
})
