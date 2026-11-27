import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { orderedIds } from './marketData'
import { ModularLayout } from './Modular'

const modules = [
  { id: 'a', title: 'Alpha', node: <p>alpha body</p> },
  { id: 'b', title: 'Beta', node: <p>beta body</p> },
  { id: 'c', title: 'Gamma', node: <p>gamma body</p> },
]

const titles = () => [...document.querySelectorAll('.module-title')].map((t) => t.textContent)

beforeEach(() => window.localStorage.clear())

test('move, hide and narrow panels; the layout is remembered and can be reset', async () => {
  const { unmount } = render(<ModularLayout page="test" modules={modules} />)
  expect(titles()).toEqual(['Alpha', 'Beta', 'Gamma'])
  expect(screen.getByRole('button', { name: 'Move Alpha up' })).toBeDisabled()
  await userEvent.click(screen.getByRole('button', { name: 'Move Alpha down' }))
  expect(titles()).toEqual(['Beta', 'Alpha', 'Gamma'])
  await userEvent.click(screen.getByRole('button', { name: 'Hide Gamma' }))
  expect(screen.queryByText('gamma body')).not.toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Show Gamma' })).toHaveAttribute('aria-expanded', 'false')
  await userEvent.click(screen.getByRole('button', { name: 'Narrow Beta' }))
  expect(document.querySelector('[data-module="b"]')).toHaveClass('module-narrow')

  // A new visit restores it.
  unmount()
  render(<ModularLayout page="test" modules={modules} />)
  expect(titles()).toEqual(['Beta', 'Alpha', 'Gamma'])
  expect(screen.queryByText('gamma body')).not.toBeInTheDocument()

  await userEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
  expect(titles()).toEqual(['Alpha', 'Beta', 'Gamma'])
  expect(screen.getByText('gamma body')).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Reset layout' })).not.toBeInTheDocument()
})

test('broken or blocked storage falls back to the default layout', () => {
  window.localStorage.setItem('layout:test', '{not json')
  render(<ModularLayout page="test" modules={modules} />)
  expect(titles()).toEqual(['Alpha', 'Beta', 'Gamma'])
})

test('orderedIds keeps the saved order, drops gone panels, appends new ones', () => {
  expect(orderedIds(['a', 'b', 'c', 'd'], ['c', 'x', 'a'])).toEqual(['c', 'a', 'b', 'd'])
  expect(orderedIds(['a', 'b'], [])).toEqual(['a', 'b'])
})
