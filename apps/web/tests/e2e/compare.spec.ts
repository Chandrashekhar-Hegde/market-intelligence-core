import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

import { mockMarketData, openCompare } from './marketMocks'

test('compare instruments: plain side-by-side first, the maths on request; accessible and no overflow', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  await openCompare(page)

  // Plain words first: who grew, who is bumpier, do they move together, what a mix does.
  const plain = page.getByTestId('plain-compare')
  await expect(plain.getByTestId('head-to-head').getByRole('row', { name: /^RELIANCE\.NS/ })).toBeVisible()
  await expect(plain.getByTestId('plain-compare-findings')).toContainText('RELIANCE.NS and TCS.NS move somewhat together')
  await expect(page.getByTestId('rebased-chart').locator('canvas').first()).toBeVisible()
  await expect(page.getByTestId('heatmap')).toBeHidden()

  await page.getByText('Show all the maths').click()
  await expect(page.getByTestId('compare-findings')).toContainText('One shared factor explains 70%')
  await expect(page.getByTestId('heatmap')).toContainText('0.55')
  await expect(page.getByTestId('pairs-table').getByRole('row', { name: /RELIANCE\.NS \/ TCS\.NS/ })).toContainText('cointegrated')

  await page.getByRole('button', { name: 'Remove TCS.NS' }).click()
  await expect(page.getByRole('list', { name: 'Selected instruments' })).not.toContainText('TCS.NS')

  const results = await new AxeBuilder({ page }).include('main').withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
  const blocking = results.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))
  expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(' | ')}`)).toEqual([])
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})
