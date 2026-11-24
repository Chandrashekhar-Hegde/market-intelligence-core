// Real network: the local API fetches from Yahoo. Excluded by default; run with
//   npm run test:e2e:live
// before deploying. Outside NSE hours the quote is CLOSED, which is still a pass.
import { expect, test } from '@playwright/test'

test.describe('@live real Yahoo data', () => {
  test('search INFY, load real bars, quote and quality', async ({ page }) => {
    await page.goto('/')
    await page.getByLabel(/search nse \/ bse/i).fill('infosys')
    await page.getByRole('button', { name: /INFY\.NS/ }).click()

    await expect(page.getByTestId('quote-price')).toHaveText(/₹[\d,]+\.\d{2}/, { timeout: 20_000 })
    await expect(page.getByTestId('live-state')).toHaveText(/LIVE|DELAYED|CLOSED/)
    await expect(page.getByTestId('quality-badge')).toBeVisible({ timeout: 20_000 })
    await expect(page.getByTestId('candle-chart').locator('canvas').first()).toBeVisible()
    await expect(page.getByText(/\d+ sessions · yahoo-chart-v8/)).toBeVisible()
  })
})
