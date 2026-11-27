import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

import { mockMarketData } from './marketMocks'

test('Markets view has no serious accessibility violations', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  await expect(page.getByTestId('advice')).toBeVisible()
  // Check the folded statistics too, not only what shows by default.
  await page.getByText('Show all the maths').click()
  await expect(page.getByTestId('quality-badge')).toBeVisible()
  await expect(page.getByTestId('findings')).toBeVisible()
  await page.getByLabel(/search stocks/i).fill('reliance')
  await expect(page.getByRole('button', { name: /RELIANCE\.NS/ })).toBeVisible()

  const results = await new AxeBuilder({ page })
    .include('main')
    .include('aside')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  const blocking = results.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))
  expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(' | ')}`)).toEqual([])
})

test('News desk, How it works and Compare pages have no serious accessibility violations', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  for (const tab of [/news desk/i, /how it works/i, /compare/i]) {
    await page.getByRole('tab', { name: tab }).click()
    await expect(page.getByRole('main')).toBeVisible()
    const results = await new AxeBuilder({ page })
      .include('main')
      .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
      .analyze()
    const blocking = results.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))
    expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(' | ')}`)).toEqual([])
  }
})
