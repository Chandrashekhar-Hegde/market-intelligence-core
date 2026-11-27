import { describe, expect, test } from 'vitest'

import { closeRangeToday, openToCloseVol } from './plainLanguage'

describe('likely close today', () => {
  test('half-way through the session: the fitted band over the variance still to come', () => {
    // 187.5 minutes in: variance left 0.4558 (between 0.469 and 0.416), band 1.40.
    const [lo, hi] = closeRangeToday(100, 0.01, 0.5)!
    const half = 1.4 * 0.01 * Math.sqrt(0.469 + ((0.416 - 0.469) * 7.5) / 30)
    expect(hi).toBeCloseTo(100 * Math.exp(half), 10)
    expect(lo).toBeCloseTo(100 * Math.exp(-half), 10)
  })

  test('narrows through the day but keeps the busy last half hour in it', () => {
    const width = (e: number) => {
      const [lo, hi] = closeRangeToday(100, 0.012, e)!
      return hi - lo
    }
    expect(width(0.1)).toBeGreaterThan(width(0.5))
    expect(width(0.5)).toBeGreaterThan(width(0.9))
    // With 8% of the time left, about 19% of the variance is still to come: the range stays
    // wider than time-based scaling (sqrt of the time left) would make it.
    expect(width(0.92) / width(0.1)).toBeGreaterThan(Math.sqrt(0.08 / 0.9))
    expect(closeRangeToday(100, 0.012, 1)).toBeNull()
    expect(closeRangeToday(100, 0, 0.5)).toBeNull()
  })

  test('open-to-close volatility needs enough days', () => {
    const bars = Array.from({ length: 100 }, (_, i) => ({ open: 100, close: i % 2 ? 101 : 99 }))
    expect(openToCloseVol(bars)).toBeCloseTo(0.01005, 4)
    expect(openToCloseVol(bars.slice(0, 30))).toBeNull()
  })
})
