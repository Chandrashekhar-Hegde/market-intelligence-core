import { expect, test } from '@playwright/test'

import { mockMarketData } from './marketMocks'

test('search, pick an instrument, and see quote, candles, quality and TradingView', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')

  await expect(page.getByRole('tab', { name: /markets/i })).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByTestId('candle-chart').locator('canvas').first()).toBeVisible()

  await page.getByLabel(/search nse \/ bse/i).fill('reliance')
  await page.getByRole('button', { name: /RELIANCE\.NS/ }).click()

  await expect(page.getByRole('heading', { name: 'Reliance Industries Limited' })).toBeVisible()
  await expect(page.getByTestId('quote-price')).toHaveText(/₹1,377\.50/)
  await expect(page.getByTestId('live-state')).toHaveText('CLOSED')
  await expect(page.getByTestId('quality-badge')).toHaveText(/Usable/)
  await expect(page.getByText(/60 sessions/).first()).toBeVisible()
  await expect(page.getByText(/Dividend ₹5\.50/)).toBeVisible()

  await page.getByRole('button', { name: '5y' }).click()
  await expect(page.getByRole('button', { name: '5y' })).toHaveAttribute('aria-pressed', 'true')

  await page.getByText(/TradingView live chart \(NSE:RELIANCE\)/).click()
  const widgetScript = page.getByTestId('tradingview').locator('script')
  await expect(widgetScript).toHaveCount(1)
  expect(await widgetScript.textContent()).toContain('"symbol":"NSE:RELIANCE"')
})

test('a streamed tick shows LIVE and moves the price', async ({ page }) => {
  await mockMarketData(page, { liveTicks: true })
  await page.goto('/')
  await page.getByRole('button', { name: 'Reliance' }).click()

  await expect(page.getByTestId('live-state')).toHaveText('LIVE')
  await expect(page.getByTestId('quote-price')).toHaveText(/₹1,391\.25/)
  await expect(page.getByTestId('quote-price')).toContainText('+13.75 (+1.00%)')
})
