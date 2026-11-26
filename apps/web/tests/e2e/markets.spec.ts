import { expect, test } from '@playwright/test'

import { mockMarketData } from './marketMocks'

test('search, pick an instrument, and see quote, charts, quality and TradingView', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')

  await expect(page.getByRole('tab', { name: /markets/i })).toHaveAttribute('aria-selected', 'true')
  // Today's minute chart is the default; the watchlist sits above it.
  await expect(page.getByTestId('intraday-chart').locator('canvas').first()).toBeVisible()
  await expect(page.getByRole('region', { name: 'Watchlist' }).getByRole('button', { name: /^Reliance/ })).toContainText('+0.91%')

  await page.getByLabel(/search stocks/i).fill('reliance')
  await page.getByRole('button', { name: /RELIANCE\.NS/ }).click()

  await expect(page.getByRole('heading', { name: 'Reliance Industries Limited' })).toBeVisible()
  await expect(page.getByTestId('quote-price')).toHaveText(/₹1,377\.50/)
  await expect(page.getByTestId('live-state')).toHaveText('CLOSED')
  // What to do comes first, in plain words; the statistics are folded away until asked for.
  await expect(page.getByTestId('stance')).toHaveText('Hold')
  await expect(page.getByTestId('advice')).toContainText('No proven edge')
  await expect(page.getByTestId('meaning-cards')).toContainText('A bad day')
  await expect(page.getByTestId('findings')).toBeHidden()
  await expect(page.getByTestId('news')).toContainText('beyond what ^NSEI explains')
  await page.getByText('Show all the maths').click()
  await expect(page.getByTestId('quality-badge')).toHaveText(/Usable/)
  await expect(page.getByText(/60 sessions/).first()).toBeVisible()
  await expect(page.getByText(/Dividend ₹5\.50/)).toBeVisible()

  // Analysis of the picked listing: findings, forecast fan, risk, auto benchmark.
  await expect(page.getByTestId('findings')).toContainText('No evidence of predictability')
  await expect(page.getByTestId('fan-chart').locator('canvas').first()).toBeVisible()
  await expect(page.getByTestId('var-table')).toContainText('2.90%')
  await expect(page.getByRole('heading', { name: 'Against NIFTY 50 (^NSEI)' })).toBeVisible()
  await expect(page.getByTestId('benchmark')).toContainText('Beta 1.08')
  await expect(page.getByTestId('validation-table')).toContainText('Earns its place')
  // Nothing on the page (tables, charts) may push it wider than the viewport, phone included.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBeLessThanOrEqual(0)

  await page.getByRole('button', { name: '5Y' }).click()
  await expect(page.getByRole('button', { name: '5Y' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByTestId('candle-chart').locator('canvas').first()).toBeVisible()

  await page.getByText(/TradingView live chart \(NSE:RELIANCE\)/).click()
  const widgetScript = page.getByTestId('tradingview').locator('script')
  await expect(widgetScript).toHaveCount(1)
  expect(await widgetScript.textContent()).toContain('"symbol":"NSE:RELIANCE"')
})

test('a streamed tick shows LIVE and moves the price', async ({ page }) => {
  await mockMarketData(page, { liveTicks: true })
  await page.goto('/')
  await page.getByRole('region', { name: 'Watchlist' }).getByRole('button', { name: /^Reliance/ }).click()

  await expect(page.getByTestId('live-state')).toHaveText('LIVE')
  await expect(page.getByTestId('quote-price')).toHaveText(/₹1,391\.25/)
  await expect(page.getByTestId('quote-price')).toContainText('+13.75 (+1.00%)')
})

test('market filters narrow the search to India and then BSE', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  await page.getByLabel(/search stocks/i).fill('reliance')
  await page.getByRole('group', { name: 'instrument-search country' }).getByRole('button', { name: /India/ }).click()
  await page.getByRole('group', { name: 'instrument-search exchange' }).getByRole('button', { name: /BSE/ }).click()
  const results = page.getByRole('list', { name: 'Search results' })
  await expect(results.getByRole('button')).toHaveCount(1)
  await expect(results.getByRole('button')).toContainText('RELIANCE.BO')
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})
