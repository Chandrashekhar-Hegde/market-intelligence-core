import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

import { mockMarketData, openCompare } from './marketMocks'

test('compare instruments: chips, performance chart, heatmap, pairs; accessible and no overflow', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  await openCompare(page)

  await expect(page.getByTestId('compare-findings')).toContainText('One shared factor explains 70%')
  await expect(page.getByTestId('rebased-chart').locator('canvas').first()).toBeVisible()
  await expect(page.getByTestId('heatmap')).toContainText('0.55')
  await expect(page.getByTestId('pairs-table').getByRole('row', { name: /AAPL \/ MSFT/ })).toContainText('cointegrated')

  await page.getByLabel(/add an instrument/i).fill('reliance')
  await page.getByRole('button', { name: /RELIANCE\.NS/ }).first().click()
  await expect(page.getByRole('list', { name: 'Selected instruments' })).toContainText('RELIANCE.NS')
  await expect(page.getByTestId('heatmap').getByRole('row', { name: /^RELIANCE\.NS/ })).toBeVisible()

  await page.getByRole('button', { name: 'Remove MSFT' }).click()
  await expect(page.getByRole('list', { name: 'Selected instruments' })).not.toContainText('MSFT')

  const results = await new AxeBuilder({ page }).include('main').withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
  const blocking = results.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))
  expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(' | ')}`)).toEqual([])
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})
