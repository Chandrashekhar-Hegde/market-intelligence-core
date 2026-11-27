import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'

import { deskResponse } from '../tests/e2e/marketMocks'
import { Desk } from './Desk'

function renderDesk(body: unknown, status = 200) {
  vi.stubGlobal(
    'fetch',
    vi.fn<typeof fetch>(async () => Response.json(body, { status })),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Desk />
    </QueryClientProvider>,
  )
}

afterEach(() => vi.unstubAllGlobals())

test('shows where the news leans, each theme with its readings, and every source', async () => {
  renderDesk(deskResponse())
  const leans = await screen.findByTestId('desk-leans')
  // Only assets the news actually moves are listed.
  expect(within(leans).getAllByRole('listitem')).toHaveLength(2)
  expect(leans).toHaveTextContent('Leans up Crude oil (1 story, e.g. “Crude Oil Futures Rise on Positive Global Trends”)')
  expect(leans).toHaveTextContent('Leans down Oil marketers (BPCL, HPCL, IOC)')
  const commodities = screen.getByRole('heading', { name: /Commodities/ }).closest('section')!
  expect(commodities).toHaveTextContent('Crude oil ↑')
  expect(commodities).toHaveTextContent('Oil marketers (BPCL, HPCL, IOC) ↓')
  expect(screen.getByRole('heading', { name: /Market report/ }).closest('section')).toHaveTextContent('never counted')
  expect(screen.getByTestId('desk-sources')).toHaveTextContent('Bloomberg ✓ 20 · Google News: India ✗ unavailable')
  expect(screen.getByText(/research signals, not advice/)).toBeInTheDocument()
})

test('a quiet day and an outage are both said plainly', async () => {
  const quiet = { ...deskResponse(), leans: deskResponse().leans.map((l) => ({ ...l, lean: 0, drivers: [] })) }
  renderDesk(quiet)
  expect(await screen.findByText('Nothing in today’s market news points clearly one way.')).toBeInTheDocument()
})

test('desk errors are readable', async () => {
  renderDesk({ error: 'every source failed' }, 503)
  expect(await screen.findByRole('alert')).toHaveTextContent('The news desk is unavailable')
})
