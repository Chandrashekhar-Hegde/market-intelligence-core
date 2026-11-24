import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

import { mockMarketData, openEventLab } from './marketMocks'

test('primary operator screens have no serious accessibility violations', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
  await openEventLab(page)
  await expect(page.getByText('Online')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Normalized review queue' })).toBeVisible()

  await page.getByLabel('Event class').selectOption('MEDICAL_CLASSIFICATION')
  await expect(page.getByText('ICD-11 respiratory classification')).toBeVisible()

  await page.getByRole('button', { name: /run fixture/i }).click()
  await expect(page.getByRole('heading', { name: 'BUY' })).toBeVisible()
  await expect(page.getByRole('img', { name: 'Market response chart' })).toBeAttached()

  const results = await new AxeBuilder({ page })
    .include('main')
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()

  const blockingViolations = results.violations.filter((violation) =>
    ['critical', 'serious'].includes(violation.impact ?? ''),
  )

  expect(blockingViolations).toEqual([])
})

test('Markets view has no serious accessibility violations', async ({ page }) => {
  await mockMarketData(page)
  await page.goto('/')
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
