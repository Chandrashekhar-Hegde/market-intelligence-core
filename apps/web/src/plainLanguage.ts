// Plain-language reading of an analysis: what the numbers mean for someone putting money in.
// Pure and unit-tested. Every number is one the API computed, or an exact identity of them
// (documented in docs/METHODOLOGY.md, "Plain-language cards"); nothing is estimated here.
import type { Advice, AnalyzeResponse, Projection } from './api'

export type Card = { id: string; title: string; headline: string; why: string }

const HORIZON_WORDS: Record<number, string> = { 5: '1 week', 21: '1 month', 63: '3 months' }

export function horizonWords(sessions: number): string {
  return HORIZON_WORDS[sessions] ?? `${sessions} trading days`
}

/** Money in the listing currency: ₹ for INR, otherwise the code after the number. */
export function formatMoney(value: number, currency: string | null | undefined): string {
  const digits = Math.abs(value) >= 100 ? 0 : 2
  const text = Math.abs(value).toLocaleString('en-IN', { maximumFractionDigits: digits, minimumFractionDigits: digits })
  const sign = value < 0 ? '−' : ''
  if (currency === 'INR' || !currency) return `${sign}₹${text}`
  return `${sign}${text} ${currency}`
}

const pct = (x: number, digits = 1) => `${(x * 100).toFixed(digits)}%`

function monthYear(date: string): string {
  return new Date(`${date}T00:00:00Z`).toLocaleDateString('en-IN', { month: 'short', year: 'numeric', timeZone: 'UTC' })
}

/**
 * The forecast to read: the instrument's own history replayed (its real crashes and calm
 * spells), else GARCH-t (today's volatility), else GBM.
 */
export function preferredProjection(analysis: AnalyzeResponse): Projection | undefined {
  const { projections } = analysis.report
  return projections.find((p) => p.model === 'bootstrap') ?? projections.find((p) => p.model === 'garch_t') ?? projections[0]
}

/** One line on how a projection's futures were made. */
export function projectionSource(projection: Projection): string {
  const n = projection.n_paths.toLocaleString('en-IN')
  if (projection.model === 'bootstrap') {
    return `From ${n} possible futures built by replaying this stock’s own past days in runs of about ${Math.round(
      projection.block_length ?? 1,
    )} days, so its real crashes and calm spells shape the range.`
  }
  if (projection.model === 'garch_t') return `From ${n} simulated futures that start from today’s volatility, with fat tails.`
  return `From ${n} simulated futures with constant volatility and bell-curve shocks.`
}

function historical95(analysis: AnalyzeResponse) {
  return analysis.report.risk.var_es.find((v) => v.method === 'historical' && v.confidence === 0.95)
}

export function meaningCards(analysis: AnalyzeResponse, amount: number): Card[] {
  const { report, data, benchmark: chosen } = analysis
  const currency = data.currency
  const money = (x: number) => formatMoney(x, currency)
  const on = `on ${money(amount)}`
  const cards: Card[] = []

  const dailyVol = report.distribution.vol_annual / Math.sqrt(report.periods_per_year)
  cards.push({
    id: 'typical-day',
    title: 'A typical day',
    headline: `It usually moves about ${pct(dailyVol)} up or down in a day: ${money(amount * dailyVol)} ${on}.`,
    why: `This is the day-to-day wobble to expect. It is the standard deviation of daily returns over ${yearsCovered(analysis)} years.`,
  })

  const b = report.benchmark
  if (b && b.beta > 0 && b.correlation > 0) {
    // σ_benchmark = ρ·σ_stock/β, so σ_stock/σ_benchmark = β/ρ exactly.
    const ratio = b.beta / b.correlation
    const name = chosen?.name ?? b.symbol
    cards.push({
      id: 'vs-market',
      title: 'Compared with the market',
      headline:
        ratio >= 1
          ? `It is about ${ratio.toFixed(1)}× as bumpy as the ${name}.`
          : `It is calmer than the ${name}: about ${ratio.toFixed(1)}× its daily swings.`,
      why: 'A quick sense of how much more (or less) risk you take than by owning the whole market.',
    })
  }

  const bad = historical95(analysis)
  if (bad) {
    cards.push({
      id: 'bad-day',
      title: 'A bad day',
      headline: `About 1 day in 20 it has fallen more than ${pct(bad.var)} (${money(amount * bad.var)} ${on}). On those days the average fall was ${pct(bad.es)} (${money(amount * bad.es)}).`,
      why: 'If a fall of this size would make you sell in a panic, the amount is too large for you.',
    })
  }

  const dd = report.risk.max_drawdown
  cards.push({
    id: 'worst-fall',
    title: 'The worst fall',
    headline: `Its biggest drop from a high was ${pct(dd.max, 0)} (${monthYear(dd.peak_date)} to ${monthYear(dd.trough_date)}), ${
      dd.recovery_date ? `and it recovered by ${monthYear(dd.recovery_date)}` : 'and it has not fully recovered yet'
    }. Today it is ${dd.current > 0.001 ? `${pct(dd.current, 0)} below its high` : 'at or near its high'}.`,
    why: `That would have turned ${money(amount)} into ${money(amount * (1 - dd.max))} at the bottom. Ask yourself whether you could hold through it.`,
  })

  const projection = preferredProjection(analysis)
  if (projection) {
    const lines = projection.horizons.map((h) => {
      const [low, , , , high] = h.return_quantiles
      return `in ${horizonWords(h.horizon_days)}, between ${money(amount * (1 + low))} and ${money(amount * (1 + high))} (chance of being higher: ${pct(h.prob_gain, 0)})`
    })
    cards.push({
      id: 'where-it-could-be',
      title: 'Where it could be',
      headline: `9 times in 10, ${money(amount)} ends up ${lines.join('; ')}.`,
      why: `${projectionSource(projection)} These are ranges, not predictions.`,
    })
  }

  const pr = report.predictability
  const pattern = pr.ljung_box_returns.reject_5pct || pr.variance_ratios.some((v) => v.test.reject_5pct)
  cards.push({
    id: 'predictable',
    title: 'Can its next move be predicted?',
    headline: pattern
      ? 'There are faint patterns in its past moves, too weak on their own to trade on. The trading-rule tests above decide whether any of them is worth acting on.'
      : 'No. Its past daily moves have not predicted the next ones, which is normal for a widely traded instrument.',
    why: 'Anyone who claims to know tomorrow’s direction for sure is guessing.',
  })

  const v = report.validation
  if (v) {
    const winners = v.models.filter((m) => m.verdict.startsWith('Earns'))
    cards.push({
      id: 'trust',
      title: 'Can you trust these ranges?',
      headline: winners.length
        ? `Yes, as far as tests can tell. The risk model was checked on ${v.n_forecasts.toLocaleString('en-IN')} past days it had never seen, and its ranges held up.`
        : `Only roughly. On ${v.n_forecasts.toLocaleString('en-IN')} past days the models had never seen, none did better than simply replaying history.`,
      why: 'Every forecast model here has to prove itself on data it was not built from.',
    })
  }

  if (b) {
    const name = chosen?.name ?? b.symbol
    cards.push({
      id: 'with-market',
      title: 'Does it move with the market?',
      headline: `When the ${name} moves 1%, this has moved about ${b.beta.toFixed(1)}% the same way on average. The market explains ${pct(b.r_squared, 0)} of its daily moves; the rest is down to the company itself.`,
      why: 'If most of the movement is the market, owning it adds little that an index fund wouldn’t.',
    })
  }
  return cards
}

/**
 * The most to put in so that an average bad day (the worst 1 in 20) costs no more than
 * `maxLoss`: maxLoss / expected shortfall at 95%.
 */
export function maxPosition(analysis: AnalyzeResponse, maxLoss: number): number | null {
  const es = historical95(analysis)?.es
  return es && es > 0 && maxLoss > 0 ? maxLoss / es : null
}

/**
 * Likely range (5%–95% of simulated outcomes) from `price` over `sessions` sessions (fractions
 * allowed: the rest of today is the share of the session left). The 1-day quantiles of the
 * preferred projection are scaled by √sessions (square-root-of-time; ignores intraday
 * seasonality and is meant for a day or two, not weeks: the fan covers those).
 */
export function sessionRange(analysis: AnalyzeResponse, price: number, sessions: number): [number, number] | null {
  const first = preferredProjection(analysis)?.fan[0]
  const last = analysis.report.last_price
  if (!first || !(price > 0) || !(last > 0)) return null
  const f = Math.sqrt(Math.max(sessions, 0))
  const [lowQ, , , , highQ] = first.price.map((p) => Math.log(p / last))
  return [price * Math.exp((lowQ ?? 0) * f), price * Math.exp((highQ ?? 0) * f)]
}

/** Fraction of the regular session still to trade at `nowSec`; 1 before the open, 0 after. */
export function sessionLeft(start: number | null, end: number | null, nowSec: number): number | null {
  if (start === null || end === null || end <= start) return null
  if (nowSec <= start) return 1
  if (nowSec >= end) return 0
  return (end - nowSec) / (end - start)
}

/**
 * Today's volume against a normal day, adjusted for how much of the session has passed.
 * ponytail: assumes volume accrues evenly through the day. Real volume is U-shaped (busy
 * open and close), so this overstates early-morning and understates midday activity; use an
 * intraday volume profile if it misleads.
 */
export function relativeVolume(todayVolume: number, avgDailyVolume: number, elapsedFraction: number): number | null {
  if (!(avgDailyVolume > 0) || !(todayVolume > 0) || elapsedFraction < 0.05) return null
  return todayVolume / (avgDailyVolume * Math.min(elapsedFraction, 1))
}

/** Whole years of history the analysis used. */
export function yearsCovered(analysis: AnalyzeResponse): number {
  const { first_date: first, last_date: last } = analysis.report
  return Math.max(1, Math.round((Date.parse(last) - Date.parse(first)) / (365.25 * 86_400_000)))
}

export type Guidance = { owner: string; buyer: string }

/**
 * What the call means for someone who already holds the instrument and for someone who
 * doesn't: the same evidence, two different actions. `cap` is the most to hold for the
 * user's bad-day loss limit (see maxPosition).
 */
export function stanceGuidance(advice: Advice, cap: number | null, currency: string | null): Guidance {
  const upTo = cap !== null ? `, keeping the total under ${formatMoney(cap, currency)} so a bad day stays within your limit` : ''
  if (advice.stance === 'buy') {
    return {
      owner: `You can add more${upTo}.`,
      buyer: `You can buy${upTo}.`,
    }
  }
  if (advice.stance === 'sell') {
    return {
      owner: 'Consider selling or reducing it: the evidence points down from here.',
      buyer: 'Don’t buy it now.',
    }
  }
  if (advice.basis === 'conflict') {
    return {
      owner: 'Keep it for now. The signals disagree, so there is no clear reason to sell.',
      buyer: 'Wait. Buy only when the signals agree.',
    }
  }
  if (advice.basis === 'proven_but_quiet') {
    return {
      owner: 'Keep it. The rules that have worked on this stock are not saying sell.',
      buyer: 'Wait. The rules that have worked on this stock are not saying buy today; check again in a few days.',
    }
  }
  return {
    owner: 'Keep it if it still fits your plan. Nothing tested on its history says sell.',
    buyer: `No tested reason to buy now. If you buy anyway as a long-term holding, keep it${
      cap !== null ? ` under ${formatMoney(cap, currency)}` : ' small'
    } and be ready to sit through falls like its worst one.`,
  }
}
