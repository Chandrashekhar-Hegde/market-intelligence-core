// Plain-language reading of an analysis: what the numbers mean for someone putting money in.
// Pure and unit-tested. Every number is one the API computed, or an exact identity of them
// (documented in docs/METHODOLOGY.md, "Plain-language cards"); nothing is estimated here.
import type { Advice, AnalyzeResponse, LongRun, Projection, TodayItem } from './api'

export type Card = { id: string; title: string; headline: string; why: string }

const HORIZON_WORDS: Record<number, string> = {
  1: '1 day',
  2: '2 days',
  3: '3 days',
  5: '1 week',
  10: '2 weeks',
  21: '1 month',
  63: '3 months',
}

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

/** The forecast to read first: the one validation rates best (chosen by the API). */
export function preferredProjection(analysis: AnalyzeResponse): Projection | undefined {
  const { projections, lead_model: lead } = analysis.report
  return projections.find((p) => p.model === lead) ?? projections[0]
}

/**
 * A chance as a natural frequency, which people read correctly far more often than a
 * percentage (Gigerenzer & Hoffrage 1995): "64 in 100", or "1 in 12" when it is rare.
 */
export function chanceWords(p: number): string {
  if (!Number.isFinite(p)) return '—'
  if (p > 0 && p < 0.095) {
    const n = Math.round(1 / p)
    return n > 100 ? 'under 1 in 100' : `1 in ${n}`
  }
  return `${Math.round(p * 100)} in 100`
}

/**
 * Did the history-based chance of a gain hold up? From the walk-forward check, in words.
 * The chance mostly reflects the stock's long-run growth, so this is what keeps it honest.
 */
export function chanceHonesty(analysis: AnalyzeResponse, horizon: number): string | null {
  const c = analysis.report.validation?.chance_of_gain.find((x) => x.horizon_days === horizon)
  if (!c) return null
  const verdict =
    c.vs_coin.reject_5pct && c.vs_coin.statistic < 0
      ? 'That is clearly better than a coin flip.'
      : c.vs_coin.reject_5pct
        ? 'That is worse than a coin flip, so don’t lean on it.'
        : `With only ${c.n} checks, that is not yet clearly better than a coin flip.`
  const stretch = horizonWords(horizon).replace(/s$/, '').replace(' ', '-')
  return `Checked on ${c.n} past ${stretch} stretches: its history said about ${chanceWords(
    c.mean_forecast,
  )} on average, and it actually rose ${chanceWords(c.realised)}. ${verdict}`
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
      return `in ${horizonWords(h.horizon_days)}, between ${money(amount * (1 + low))} and ${money(amount * (1 + high))} (${chanceWords(h.prob_gain)} chance of being higher)`
    })
    cards.push({
      id: 'where-it-could-be',
      title: 'Where it could be',
      headline: `9 times in 10, ${money(amount)} ends up ${lines.join('; ')}.`,
      why: `${projectionSource(projection)} These are chances, not predictions.`,
    })
    const longest = projection.horizons.at(-1)
    if (longest) {
      const honest = chanceHonesty(analysis, longest.horizon_days)
      cards.push({
        id: 'chances',
        title: `The chances over ${horizonWords(longest.horizon_days)}`,
        headline: `${chanceWords(longest.prob_gain)} chance it is higher; ${chanceWords(
          longest.prob_loss_over_10pct,
        )} chance it is more than 10% lower; ${chanceWords(longest.prob_drawdown_over_10pct)} chance it falls 10% from a high at some point on the way.`,
        why:
          honest ??
          'Much of the chance of a gain comes from its average growth in the past, which may not continue.',
      })
    }
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
  if (advice.basis === 'news_driven') {
    return advice.stance === 'buy'
      ? {
          owner: `You could add a little${upTo}. This call rests on today’s news alone and has no track record, so judge whether the news changes your own view.`,
          buyer: `If the news convinces you, buy a small amount${upTo}. This call has no track record yet.`,
        }
      : {
          owner: 'Consider reducing it: today’s news is clearly bad for the company. This call has no track record, so judge how serious the news is for you.',
          buyer: 'Don’t buy until the news settles.',
        }
  }
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
  if (advice.basis === 'news_conflict') {
    return {
      owner: 'Keep it for now, and watch how the news develops.',
      buyer: 'Wait. The tested rule and today’s news disagree.',
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

// ---- comparing instruments --------------------------------------------------------------

/** Evans (1996) verbal scale for the strength of a correlation, used for |ρ|. */
export function correlationWords(rho: number): string {
  const r = Math.abs(rho)
  const strength = r >= 0.8 ? 'very closely' : r >= 0.6 ? 'closely' : r >= 0.4 ? 'somewhat' : r >= 0.2 ? 'loosely' : null
  if (strength === null) return 'move independently of each other'
  return rho > 0 ? `move ${strength} together` : `move ${strength} in opposite directions`
}

export type MixSummary = { equalMixVol: number; averageAloneVol: number; cut: number }

/**
 * Yearly swing (volatility) of an equal-weight mix, σ = √(wᵀΣw) with the API's annualised
 * covariance, against the average of each instrument's own swing.
 */
export function equalMix(covarianceAnnual: number[][]): MixSummary | null {
  const n = covarianceAnnual.length
  if (n < 2) return null
  const w = 1 / n
  let variance = 0
  for (let i = 0; i < n; i++) for (let j = 0; j < n; j++) variance += w * w * (covarianceAnnual[i]?.[j] ?? 0)
  const alone = covarianceAnnual.map((row, i) => Math.sqrt(row[i] ?? 0))
  const averageAloneVol = alone.reduce((a, b) => a + b, 0) / n
  const equalMixVol = Math.sqrt(Math.max(variance, 0))
  if (!(averageAloneVol > 0)) return null
  return { equalMixVol, averageAloneVol, cut: 1 - equalMixVol / averageAloneVol }
}

/**
 * A risk-balanced mix: each holding sized by 1/volatility, so each contributes about the same
 * swing (equal risk contribution when correlations are similar; Maillard, Roncalli & Teïletche
 * 2010). With no proven edge it is the honest default, not a forecast of which will do best.
 */
export function riskBalancedMix(
  holdings: { symbol: string; vol_annual: number }[],
  budget: number,
): { symbol: string; weight: number; amount: number }[] {
  const usable = holdings.filter((h) => h.vol_annual > 0)
  const total = usable.reduce((sum, h) => sum + 1 / h.vol_annual, 0)
  if (!(total > 0) || !(budget > 0)) return []
  return usable.map((h) => {
    const weight = 1 / h.vol_annual / total
    return { symbol: h.symbol, weight, amount: weight * budget }
  })
}

/** Share of outcomes over `lr.years` that beat a deposit at `rate` a year (compounded). */
export function beatChance(lr: LongRun, rate: number, source: 'resampled' | 'historical' = 'resampled'): number | null {
  const points = lr[source]
  if (!points.length) return null
  const hurdle = lr.years * Math.log(1 + rate)
  return points.filter((x) => x > hurdle).length / points.length
}

function rateText(rate: number) {
  return `${Number((rate * 100).toFixed(2))}%`
}

/**
 * Investing for 3+ years versus a fixed deposit, from the 3- and 5-year distributions. The
 * resampled chance leads; what actually happened is shown with how many separate periods it is.
 * Tiers at 80 and 60 in 100: 80 is the owner's bar for a firm statement (27 Nov 2026).
 */
export function longTermAnswer(analysis: AnalyzeResponse, rate: number): string | null {
  const runs = analysis.report.long_run
  const three = runs?.find((r) => r.years === 3)
  const five = runs?.find((r) => r.years === 5)
  if (!three) return null
  const p3 = beatChance(three, rate)
  const p5 = five ? beatChance(five, rate) : null
  const h3 = beatChance(three, rate, 'historical')
  if (p3 === null) return null
  const verdict =
    p3 >= 0.8
      ? 'Investing for the long term has clearly beaten an FD here.'
      : p3 >= 0.6
        ? 'For 3+ years it has usually beaten an FD, but not reliably: invest gradually (a SIP) and only money you won’t need.'
        : p3 < 0.45
          ? 'An FD has done better. There is no long-term case for this one on its own.'
          : 'An FD has done about as well. There is no clear long-term case for this one on its own.'
  const fiveText = p5 !== null ? ` (over 5 years: ${chanceWords(p5)})` : ''
  const actual =
    h3 !== null && three.n_windows > 0
      ? ` In the actual past, ${chanceWords(h3)} rolling 3-year stretches beat it, but that is only ${three.n_independent} separate period${three.n_independent === 1 ? '' : 's'}.`
      : ''
  const growth =
    three.drift_from === 'own history'
      ? ` Growth assumed: its own past average, ${(three.drift_annual * 100).toFixed(0)}% a year (could be anywhere from ${((three.drift_annual - 2 * three.drift_se_annual) * 100).toFixed(0)}% to ${((three.drift_annual + 2 * three.drift_se_annual) * 100).toFixed(0)}%).`
      : ` Growth assumed: its market’s (${three.drift_from}), because one company’s past average says little about its future.`
  return `${verdict} Over 3 years it beat a ${rateText(rate)} FD in ${chanceWords(p3)} of the futures built from its own past swings${fiveText}.${actual}${growth}`
}

/**
 * Today's volatility against its normal level, and the size that keeps a bad day within
 * `maxLoss`. Same measure as the track record replays (EWMA forecast over the history's
 * volatility), so the advice is the one that was tested.
 */
export function sizeToday(analysis: AnalyzeResponse, maxLoss: number): { ratio: number | null; today: number | null; normal: number | null } {
  const normal = maxPosition(analysis, maxLoss)
  const ewma = analysis.report.volatility.ewma_annual
  const usual = analysis.report.distribution.vol_annual
  const ratio = ewma > 0 && usual > 0 ? ewma / usual : null
  return { ratio, normal, today: normal !== null && ratio !== null ? normal / ratio : normal }
}

export function ownerAnswer(analysis: AnalyzeResponse, maxLoss: number): string {
  const currency = analysis.data.currency
  const { ratio, today, normal } = sizeToday(analysis, maxLoss)
  const advice = analysis.advice
  const stance = advice ? stanceGuidance(advice, null, currency).owner : 'Keep it if it still fits your plan.'
  if (ratio === null || today === null || normal === null) return stance
  const mood =
    ratio >= 1.05
      ? `It is moving ${ratio.toFixed(1)}× more than usual right now, so hold at most ${formatMoney(today, currency)} for a bad day of ${formatMoney(maxLoss, currency)} (${formatMoney(normal, currency)} in normal times).`
      : ratio <= 0.95
        ? `It is calmer than usual (${ratio.toFixed(1)}× its normal swings): up to ${formatMoney(today, currency)} keeps a bad day within ${formatMoney(maxLoss, currency)}.`
        : `It is moving about as usual: up to ${formatMoney(today, currency)} keeps a bad day within ${formatMoney(maxLoss, currency)}.`
  return `${stance} ${mood}`
}

export function traderAnswer(analysis: AnalyzeResponse): string {
  const advice = analysis.advice
  if (!advice) return 'Too little history to test any trading rule.'
  if (advice.stance !== 'hold') return stanceGuidance(advice, null, analysis.data.currency).buyer
  if (advice.basis === 'proven_but_quiet') return 'A rule that has worked here has no signal today: wait for it rather than trade.'
  if (advice.basis === 'news_conflict') return 'Wait: the tested rule and today’s news disagree.'
  return 'No timing edge at any holding period from 1 day to 3 months: none of the six rules beat trading costs on its own past (tested at 1, 2 and 3 days, 1 and 2 weeks, 1 and 3 months). Across the NIFTY 50 over 10 years, no short-term rule beat buying on a random day after costs, and no day-trading pattern held up on recent 5-minute data. Short-term moves here are close to a coin flip after costs, so the evidence says don’t trade it short term.'
}

export type Move = { kind: 'call' | 'long-term' | 'size' | 'avoid'; symbol: string; text: string; chance: number | null }

/**
 * Today's moves across the scanned list, strongest evidence first:
 * - timing calls (only where a rule passed);
 * - long-term: instruments whose 3-year chance of beating the deposit is at least 60 in 100,
 *   best first, with single stocks set against NIFTY (an index fund doing about as well with
 *   less single-company risk is the better move);
 * - holders: the instruments moving most above their normal swings, with today's safe size;
 * - avoid for the long term: chance under 55 in 100 (about a coin flip against the deposit).
 */
export function bestMoves(items: TodayItem[], fdRate: number, maxLoss: number): Move[] {
  const name = (i: TodayItem) => i.name ?? i.symbol
  const ok = items.filter((i) => !i.error)
  const chance3 = (i: TodayItem) => {
    const lr = i.long_run?.find((l) => l.years === 3)
    return lr ? beatChance(lr, fdRate) : null
  }
  const chance5 = (i: TodayItem) => {
    const lr = i.long_run?.find((l) => l.years === 5)
    return lr ? beatChance(lr, fdRate) : null
  }
  const nifty = ok.find((i) => i.symbol === '^NSEI')
  const niftyChance = nifty ? chance3(nifty) : null
  const rate = `${Number((fdRate * 100).toFixed(2))}%`
  const moves: Move[] = []
  for (const i of ok) {
    if (i.advice && i.advice.stance !== 'hold') {
      moves.push({ kind: 'call', symbol: i.symbol, chance: i.advice.confidence?.hit_rate ?? null, text: `${i.advice.stance === 'buy' ? 'Buy' : 'Sell'} ${name(i)}: ${i.advice.headline}` })
    }
  }
  // Currencies are prices, not places for long-term money.
  const ranked = ok
    .filter((i) => i.instrument_type !== 'CURRENCY')
    .map((i) => ({ i, c: chance3(i) }))
    .filter((x): x is { i: TodayItem; c: number } => x.c !== null)
    .sort((a, b) => b.c - a.c)
  // Best six, and NIFTY always: it is the reference most Indian investors compare with.
  const top = ranked.filter((x) => x.c >= 0.6).slice(0, 6)
  const niftyRow = ranked.find((x) => x.i.symbol === '^NSEI')
  if (niftyRow && niftyRow.c >= 0.6 && !top.includes(niftyRow)) top.push(niftyRow)
  for (const { i, c } of top) {
    const five = chance5(i)
    const vsIndex =
      i.single_stock && niftyChance !== null
        ? c <= niftyChance + 0.05
          ? ` A NIFTY index fund did about as well (${chanceWords(niftyChance)}) without the single-company risk.`
          : ` Ahead of NIFTY (${chanceWords(niftyChance)}), but one company can fall much further.`
        : ''
    moves.push({
      kind: 'long-term',
      symbol: i.symbol,
      chance: c,
      text: `For 3+ years, ${name(i)} beat a ${rate} FD in ${chanceWords(c)} of the futures built from its past swings${five !== null ? ` (5 years: ${chanceWords(five)})` : ''}.${vsIndex} Invest gradually and only money you won’t need.`,
    })
  }
  // Stock-picking: with the market's growth, a single stock only adds its own extra swings.
  const singles = ranked.filter((x) => x.i.single_stock && x.i.group === 'NIFTY 50')
  if (niftyChance !== null && singles.length > 0) {
    const behind = singles.filter((x) => x.c < niftyChance).length
    moves.push({
      kind: 'long-term',
      symbol: 'index-vs-stocks',
      chance: niftyChance,
      text: `Picking single NIFTY 50 stocks: ${behind} of ${singles.length} had a lower long-term chance than a NIFTY index fund (${chanceWords(niftyChance)}). Their past growth can’t be told apart from luck, so they mostly add risk: an index fund is the evidence-based default.`,
    })
  }
  const jumpy = ok
    .filter((i) => (i.vol_ratio ?? 0) >= 1.05 && (i.es95 ?? 0) > 0)
    .sort((a, b) => (b.vol_ratio ?? 0) - (a.vol_ratio ?? 0))
    .slice(0, 3)
  // Amounts in ₹: a bad-day loss is a share of what's held, whatever currency it trades in.
  for (const i of jumpy) {
    const normal = maxLoss / (i.es95 ?? 1)
    const today = normal / (i.vol_ratio ?? 1)
    moves.push({
      kind: 'size',
      symbol: i.symbol,
      chance: null,
      text: `If you hold ${name(i)}: it is moving ${(i.vol_ratio ?? 1).toFixed(1)}× more than usual, so keep it under ${formatMoney(today, 'INR')} for a bad day of ${formatMoney(maxLoss, 'INR')} (${formatMoney(normal, 'INR')} in normal times).`,
    })
  }
  for (const { i, c } of ranked.filter((x) => x.c < 0.55).slice(-3).reverse()) {
    const text =
      c < 0.45
        ? `For the long term, an FD at ${rate} has done better than ${name(i)} (it beat the FD in ${chanceWords(c)} over 3 years).`
        : `For the long term, ${name(i)} has been about a coin flip against a ${rate} FD (${chanceWords(c)} over 3 years).`
    moves.push({ kind: 'avoid', symbol: i.symbol, chance: c, text: `${text} No clear case to buy it for the long run.` })
  }
  return moves
}

function monthYearShort(date: string): string {
  return new Date(`${date}T00:00:00Z`).toLocaleDateString('en-IN', { month: 'short', year: 'numeric', timeZone: 'UTC' })
}

/**
 * Has the app's advice paid off here? Its holder rule ("hold less when it moves more than
 * usual") replayed on this instrument's history, set against simply holding.
 */
export function trackRecordWords(analysis: AnalyzeResponse): { verdict: string; summary: string; falls: string[] } | null {
  const tr = analysis.report.track_record
  if (!tr) return null
  const p = (x: number) => `${x >= 0 ? '+' : '−'}${Math.abs(x * 100).toFixed(0)}%`
  const saved = tr.app.worst_fall > tr.hold.worst_fall + 0.02
  const asGood = tr.app.sharpe >= tr.hold.sharpe - 0.01
  const verdict = saved
    ? asGood
      ? 'It saved money in the big falls and earned as much or more for the risk taken.'
      : 'It saved money in the big falls but cost some growth: here it works as insurance, not as extra return.'
    : 'It made little difference here: this instrument’s swings rarely ran far above normal before its falls.'
  const years = (Date.parse(tr.last) - Date.parse(tr.first)) / (365.25 * 86_400_000)
  const summary = `Replaying the app’s “hold less when it moves more than usual” over the last ${years.toFixed(0)} years (trading costs included, money not invested earning ${(tr.cash_rate * 100).toFixed(0)}%): worst fall ${p(tr.app.worst_fall)} instead of ${p(tr.hold.worst_fall)} just holding; growth ${(tr.app.growth * 100).toFixed(1)}% a year instead of ${(tr.hold.growth * 100).toFixed(1)}%.`
  const falls = tr.episodes.map(
    (e) => `${monthYearShort(e.start)} to ${monthYearShort(e.end)}: ${p(e.app)} following the app, ${p(e.hold)} just holding.`,
  )
  return { verdict, summary, falls }
}

/**
 * Where today's close is likely to land (9 in 10), from the price now. Fitted on the NIFTY 50's
 * 5-minute bars (50 stocks x 60 days, 31 Aug - 27 Nov 2026): for each half hour since the
 * 09:15 open, the share of a day's open-to-close variance still to come (it is U-shaped: the
 * first and last half hours carry a third of it) and the band, in units of that remaining
 * volatility, that held 90% of closes. Fitted on the first 30 days, it held 88.8% on the last
 * 30; the old time-based range held 94% (too wide midday, too narrow late).
 * [minutes since open, share of variance left, 90% band]
 */
const CLOSE_PROFILE: [number, number, number][] = [
  [0, 1.0, 1.54],
  [30, 0.82, 1.54],
  [60, 0.714, 1.47],
  [90, 0.636, 1.44],
  [120, 0.574, 1.45],
  [150, 0.519, 1.43],
  [180, 0.469, 1.4],
  [210, 0.416, 1.4],
  [240, 0.368, 1.39],
  [270, 0.323, 1.26],
  [300, 0.276, 1.27],
  [330, 0.227, 1.21],
  [360, 0.15, 1.1],
  [375, 0, 1.1],
]
const SESSION_MINUTES = 375

function interpolate(minutes: number, column: 1 | 2): number {
  const m = Math.min(Math.max(minutes, 0), SESSION_MINUTES)
  for (let i = 1; i < CLOSE_PROFILE.length; i++) {
    const a = CLOSE_PROFILE[i - 1]!
    const b = CLOSE_PROFILE[i]!
    if (m <= b[0]) return a[column] + ((b[column] - a[column]) * (m - a[0])) / (b[0] - a[0])
  }
  return CLOSE_PROFILE[CLOSE_PROFILE.length - 1]![column]
}

/** Standard deviation of daily open-to-close log moves over the last `days` sessions. */
export function openToCloseVol(bars: { open: number; close: number }[], days = 250): number | null {
  const moves = bars
    .slice(-days)
    .filter((b) => b.open > 0 && b.close > 0)
    .map((b) => Math.log(b.close / b.open))
  if (moves.length < 60) return null
  const mean = moves.reduce((a, b) => a + b, 0) / moves.length
  return Math.sqrt(moves.reduce((a, b) => a + (b - mean) ** 2, 0) / (moves.length - 1))
}

/** Today's close range (9 in 10) from `price`, with `elapsed` the share of the session gone. */
export function closeRangeToday(price: number, sdOpenClose: number, elapsed: number): [number, number] | null {
  if (!(price > 0) || !(sdOpenClose > 0) || elapsed >= 1) return null
  const minutes = Math.max(0, elapsed) * SESSION_MINUTES
  const half = interpolate(minutes, 2) * sdOpenClose * Math.sqrt(interpolate(minutes, 1))
  return [price * Math.exp(-half), price * Math.exp(half)]
}
