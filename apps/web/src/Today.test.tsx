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

test('best moves: long term against the deposit, holders sized for today, and no-case names', async () => {
  window.localStorage.removeItem('fd-rate')
  renderToday(todayResponse())
  const list = await screen.findByTestId('moves-list')
  const items = within(list).getAllByRole('listitem').map((li) => li.textContent ?? '')
  // Best 3-year chance first: gold (0…1.2), then Reliance (−0.1…1.0), then NIFTY (−0.2…1.0).
  expect(items[0]).toMatch(/^Long term For 3\+ years, Gold ETF beat a 7% FD in 83 in 100/)
  expect(items[1]).toMatch(/Reliance Industries beat a 7% FD in 72 in 100.*Ahead of NIFTY \(66 in 100\), but one company can fall much further/)
  expect(items.some((t) => /^Long term For 3\+ years, NIFTY 50 beat a 7% FD in 66 in 100/.test(t))).toBe(true)
  // Gold is 1.6× as jumpy as usual: ₹500 / 2% = ₹25,000 normally, ₹15,625 today.
  expect(items.find((t) => t.startsWith('If you hold it'))).toBe(
    'If you hold it If you hold Gold ETF: it is moving 1.6× more than usual, so keep it under ₹15,625 for a bad day of ₹500 (₹25,000 in normal times).',
  )
  expect(items).toContain(
    'Long term: no case For the long term, an FD at 7% has done better than Nasdaq 100 ETF (it beat the FD in 38 in 100 over 3 years). No clear case to buy it for the long run.',
  )
  // Stock-pickers are told how the single stocks compare with the index fund.
  expect(items.some((t) => t.includes('Picking single NIFTY 50 stocks: 1 of 2 had a lower long-term chance than a NIFTY index fund (66 in 100)'))).toBe(true)
  // A currency is a price, not a place for long-term money.
  expect(items.some((t) => t.includes('USD/INR'))).toBe(false)
})
