import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { deskResponse, todayResponse } from '../tests/e2e/marketMocks'
import type { TodayItem } from './api'
import { riskBalancedMix } from './plainLanguage'
import { Today } from './Today'

function renderToday(today: unknown, status = 200) {
  const fetchMock = vi.fn<typeof fetch>(async (input) => {
    const url = new URL(input.toString())
    if (url.pathname === '/api/v1/desk') return Response.json(deskResponse())
    return Response.json(today, { status })
  })
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <Today />
    </QueryClientProvider>,
  )
  return fetchMock
}

afterEach(() => vi.unstubAllGlobals())

test('a quiet day says so plainly and still gives the chances', async () => {
  const fetchMock = renderToday(todayResponse())
  expect(await screen.findByTestId('today-notice')).toHaveTextContent('Not SEBI-registered investment advice')
  expect(screen.getByTestId('today-calls')).toHaveTextContent('No Buy or Sell today.')
  const table = screen.getByTestId('today-table')
  expect(within(table).getByRole('row', { name: /Reliance Industries/ })).toHaveTextContent('61 in 100')
  expect(within(table).getByRole('row', { name: /Reliance Industries/ })).toHaveTextContent('history said 69, rose 55')
  expect(within(table).getByRole('row', { name: /Gold ETF/ })).toHaveTextContent('1 in 33')
  // The watchlist travels with the request.
  expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/today?extra=%5ENSEI%2C'), expect.anything())
})

test('calls lead, with their evidence or the lack of it', async () => {
  const base = todayResponse()
  const items = (base.items as TodayItem[]).map((i) =>
    i.symbol === 'POWERGRID.NS'
      ? {
          ...i,
          advice: {
            ...i.advice!,
            stance: 'buy',
            basis: 'proven',
            horizon_days: 5,
            confidence: { hit_rate: 0.58, low: 0.52, high: 0.64, n: 120 },
            headline: 'Buy for about a week.',
          },
        }
      : i,
  )
  renderToday({ ...base, items })
  const calls = await screen.findByTestId('today-calls')
  expect(calls).toHaveTextContent('Buy Power Grid')
  expect(calls).toHaveTextContent('Right 58 times in 100 over 120 past calls.')
})

test('US from India shows rupee returns; AI shows listed chances and private-company news', async () => {
  renderToday(todayResponse())
  const us = await screen.findByTestId('today-us')
  expect(us).toHaveTextContent('the dollar gained 4.0% against the rupee')
  // (1 + 30%) × (1 + 4%) − 1 = 35.2%.
  expect(within(screen.getByTestId('us-inr-table')).getByRole('row', { name: /NVIDIA/ })).toHaveTextContent('30.0%35.2%')
  const ai = screen.getByTestId('today-ai')
  expect(ai).toHaveTextContent('NVIDIA: 62 in 100 chance higher in 3 months')
  expect(await within(ai).findByText('No stories about them in today’s feeds.')).toBeInTheDocument()
})

test('the mix balances risk and follows the picks', async () => {
  renderToday(todayResponse())
  const mix = await screen.findByTestId('mix-result')
  // NIFTY σ 15%, gold 13%, MON100 20%: weights ∝ 1/σ.
  expect(mix).toHaveTextContent('Gold ETF: ₹39,735 (40%)')
  await userEvent.click(screen.getByRole('checkbox', { name: 'Nasdaq 100 ETF' }))
  expect(screen.getByTestId('mix-result')).toHaveTextContent('Gold ETF: ₹53,571 (54%)')
})

test('errors are readable', async () => {
  renderToday({ error: 'boom' }, 503)
  expect(await screen.findByRole('alert')).toHaveTextContent('Today’s list is unavailable')
})

test('riskBalancedMix: inverse volatility, nothing for nothing', () => {
  const mix = riskBalancedMix(
    [
      { symbol: 'A', vol_annual: 0.1 },
      { symbol: 'B', vol_annual: 0.3 },
      { symbol: 'C', vol_annual: 0 },
    ],
    1000,
  )
  expect(mix.map((m) => [m.symbol, Math.round(m.amount)])).toEqual([
    ['A', 750],
    ['B', 250],
  ])
  expect(riskBalancedMix([], 1000)).toEqual([])
  expect(riskBalancedMix([{ symbol: 'A', vol_annual: 0.2 }], 0)).toEqual([])
})
