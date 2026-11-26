import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import App from './App'

// The shell only needs /health; the views' own calls answer 404, which they show as errors.
function mockFetch() {
  const fetchMock = vi.fn<typeof fetch>(async (input) =>
    input.toString().endsWith('/health')
      ? Response.json({ status: 'ok', service: 'gm-api' })
      : new Response('not found', { status: 404 }),
  )
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('EventSource', class { addEventListener() {} close() {} })
  return fetchMock
}

function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })

  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  )
}

test('opens on the live Markets view with three plain tabs and a standing disclaimer', async () => {
  mockFetch()
  renderApp()

  expect(screen.getByRole('heading', { name: 'Market Intelligence' })).toBeInTheDocument()
  expect(screen.getByRole('tab', { name: /markets/i })).toHaveAttribute('aria-selected', 'true')
  expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual(['Markets', 'Compare', 'How it works'])
  expect(screen.getByRole('heading', { name: /live prices, history and analysis/i })).toBeInTheDocument()
  expect(screen.getByText(/not personal financial advice/i)).toBeInTheDocument()
  expect(await screen.findByText('Online')).toBeInTheDocument()
})

test('How it works explains the data sources, what is stored, and the terms', async () => {
  mockFetch()
  renderApp()
  await userEvent.click(screen.getByRole('tab', { name: /how it works/i }))

  expect(screen.getByRole('heading', { name: /what is stored \(the database question\)/i })).toBeInTheDocument()
  expect(screen.getByTestId('guide-storage')).toHaveTextContent(/market data is not kept in a database/i)
  expect(screen.getByTestId('guide-storage')).toHaveTextContent(/hosted beta runs without a database/i)
  expect(screen.getByText(/NSE stocks and NIFTY at about 2\s+seconds/i)).toBeInTheDocument()
  expect(screen.getByTestId('glossary')).toHaveTextContent('Cointegration')
  expect(screen.getByRole('link', { name: /methodology document/i })).toHaveAttribute('href', expect.stringContaining('METHODOLOGY.md'))
})

test('the Compare tab opens the comparison view', async () => {
  mockFetch()
  renderApp()
  await userEvent.click(screen.getByRole('tab', { name: /compare/i }))
  expect(screen.getByRole('heading', { name: /how instruments move together/i })).toBeInTheDocument()
})
