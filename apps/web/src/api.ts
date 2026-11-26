import { useQuery } from '@tanstack/react-query'

export type HealthResponse = {
  status: string
  service: string
}

const apiBaseUrl = (import.meta.env.VITE_API_BASE_URL ?? 'http://127.0.0.1:8000').replace(
  /\/$/,
  '',
)

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBaseUrl}${path}`, {
    headers: {
      'Content-Type': 'application/json',
      ...init?.headers,
    },
    ...init,
  })

  if (!response.ok) {
    const body = await response.text()
    throw new Error(`${response.status} ${response.statusText}: ${body}`)
  }

  return (await response.json()) as T
}

export function fetchHealth(): Promise<HealthResponse> {
  return request<HealthResponse>('/health')
}

export function getApiBaseUrl(): string {
  return apiBaseUrl
}

// ---- market data (/api/v1) -------------------------------------------------------------

export type SearchHit = {
  symbol: string
  name: string
  exchange: string
  quote_type: string
}

export type InstrumentMeta = {
  symbol: string
  name: string | null
  currency: string | null
  exchange: string | null
  instrument_type: string | null
  timezone: string | null
  last_price: number | null
  last_trade_at: string | null
  /** Change vs the previous session's close (Yahoo's figures). */
  change: number | null
  change_pct: number | null
  day_volume: number | null
  market_open: boolean
  /** TradingView's name for the listing, when known (server-side table). */
  tradingview_symbol: string | null
}

export type PriceBar = {
  symbol: string
  date: string
  open: number
  high: number
  low: number
  close: number
  adj_close: number | null
  volume: number
  source: string
}

export type CorporateAction =
  | { kind: 'dividend'; date: string; amount: number }
  | { kind: 'split'; date: string; numerator: number; denominator: number }

export type ReturnOutlier = {
  date: string
  log_return: number
  robust_z: number
  near_corporate_action: boolean
}

export type QualityReport = {
  passed: boolean
  n_bars: number
  first_date: string | null
  last_date: string | null
  ohlc_violations: string[]
  non_positive_prices: string[]
  zero_volume_days: number
  missing_days: string[]
  is_stale: boolean
  outliers: ReturnOutlier[]
  warnings: string[]
}

export type ChartData = {
  meta: InstrumentMeta
  bars: PriceBar[]
  corporate_actions: CorporateAction[]
  dropped_partial_bar: boolean
  source: string
  fetched_at: string
}

export type BarsResponse = {
  chart: ChartData
  quality: QualityReport
  /** Home-market index whose trading days were the calendar for gap checks, if any. */
  calendar_reference: string | null
}

export type QuoteResponse = {
  quote: InstrumentMeta
  source: string
  fetched_at: string
  delayed: boolean
}

export type Tick = {
  symbol: string
  price: number
  change: number
  change_pct: number
  day_high: number | null
  day_low: number | null
  day_volume: number | null
  currency: string | null
  exchange: string | null
  tick_time: string
  source: string
}

export const BAR_RANGES = ['1mo', '3mo', '6mo', '1y', '2y', '5y', '10y', 'max'] as const
export type BarRange = (typeof BAR_RANGES)[number]

export type IntradayPoint = { t: number; open: number; high: number; low: number; close: number; volume: number }

export type IntradayData = {
  meta: InstrumentMeta
  interval: '1m' | '5m'
  previous_close: number | null
  session_start: number | null
  session_end: number | null
  points: IntradayPoint[]
  source: string
  fetched_at: string
}

export type IntradayRange = '1d' | '5d'
/** What the chart's range picker offers: today and this week intraday, then daily history. */
export const CHART_RANGES = ['1d', '5d', '1mo', '6mo', '1y', '5y', 'max'] as const
export type ChartRange = (typeof CHART_RANGES)[number]
export const isIntraday = (range: ChartRange): range is IntradayRange => range === '1d' || range === '5d'

export function fetchIntraday(symbol: string, range: IntradayRange): Promise<{ chart: IntradayData }> {
  return request<{ chart: IntradayData }>(`/api/v1/intraday?symbol=${encodeURIComponent(symbol)}&range=${range}`)
}

export async function searchInstruments(query: string): Promise<SearchHit[]> {
  const body = await request<{ results: SearchHit[] }>(
    `/api/v1/instruments/search?q=${encodeURIComponent(query)}`,
  )
  return body.results
}

export function fetchBars(symbol: string, range: BarRange): Promise<BarsResponse> {
  return request<BarsResponse>(
    `/api/v1/bars?symbol=${encodeURIComponent(symbol)}&range=${range}`,
  )
}

export function fetchQuote(symbol: string): Promise<QuoteResponse> {
  return request<QuoteResponse>(`/api/v1/quote?symbol=${encodeURIComponent(symbol)}`)
}

export function streamUrl(symbols: string[]): string {
  return `${apiBaseUrl}/api/v1/stream?symbols=${encodeURIComponent(symbols.join(','))}`
}

/** Turn `request()`'s "404 Not Found: {...}" errors into something a reader can act on. */
export function describeApiError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error)
  if (message.startsWith('404')) return 'Unknown symbol. Search for it by name instead.'
  if (message.startsWith('401')) return 'Access token missing or expired. Reload to sign in.'
  if (message.startsWith('503')) return 'Market data provider is unavailable right now. Try again shortly.'
  if (message.startsWith('400')) {
    const detail = message.match(/"error":"([^"]+)"/)?.[1]
    return detail ? `Invalid request: ${detail}` : 'Invalid request.'
  }
  return message
}

// ---- analysis (/api/v1/analyze) ----------------------------------------------------------

export type TestResult = { statistic: number; p_value: number; reject_5pct: boolean }

export type Projection = {
  model: 'gbm' | 'garch_t'
  n_paths: number
  seed: number
  fan: { day: number; price: [number, number, number, number, number] }[]
  horizons: {
    horizon_days: number
    return_quantiles: [number, number, number, number, number]
    prob_gain: number
    prob_loss_over_10pct: number
    prob_drawdown_over_10pct: number
  }[]
}

export type AnalysisReport = {
  symbol: string
  first_date: string
  last_date: string
  n_returns: number
  last_price: number
  periods_per_year: number
  distribution: {
    mean_annual: number
    mean_annual_se: number
    vol_annual: number
    skewness: number
    excess_kurtosis: number
    jarque_bera: TestResult
    hill_tail_index: number | null
    worst_day: number
    best_day: number
  }
  predictability: {
    lags: number
    ljung_box_returns: TestResult
    ljung_box_squared: TestResult
    variance_ratios: { q: number; ratio: number; test: TestResult }[]
    adf: { statistic: number; critical_values: [number, number, number]; reject_unit_root_5pct: boolean }
  }
  volatility: {
    ewma_annual: number
    garch: null | {
      alpha: number
      beta: number
      nu: number
      persistence: number
      half_life_days: number
      long_run_vol_annual: number
      current_vol_annual: number
      term_structure: { horizon_days: number; vol_annual: number }[]
    }
  }
  projections: Projection[]
  risk: {
    var_es: { method: 'historical' | 'student_t' | 'garch_t'; confidence: number; var: number; es: number }[]
    max_drawdown: { max: number; peak_date: string; trough_date: string; recovery_date: string | null; duration_days: number; current: number }
    sharpe: number
    sharpe_se: number
    sortino: number
  }
  benchmark: null | {
    symbol: string
    n_common_days: number
    beta: number
    beta_se: number
    alpha_annual: number
    alpha_annual_se: number
    r_squared: number
    correlation: number
    idiosyncratic_vol_annual: number
  }
  validation: Validation | null
  findings: string[]
}

export type Validation = {
  n_forecasts: number
  first_forecast_index: number
  baseline: string
  models: {
    model: 'historical_sim' | 'ewma_normal' | 'garch_t'
    mean_pinball: number
    vs_baseline: TestResult | null
    coverage_90: number
    var_backtests: {
      level: number
      expected_rate: number
      exceptions: number
      rate: number
      kupiec: TestResult
      christoffersen: TestResult
      pass: boolean
    }[]
    direction: { n: number; hits: number; accuracy: number; binomial: TestResult } | null
    verdict: string
  }[]
}

export type SignalTest = {
  horizon_days: number
  n_obs: number
  n_independent: number
  effect: number
  t_stat: number
  p_value: number
  p_holm: number
  effect_first_half: number
  effect_second_half: number
  hit_rate: number | null
  hit_n: number
  hit_low: number | null
  hit_high: number | null
  mean_return_long: number | null
  mean_return_short: number | null
  earns_place: boolean
  verdict: string
}

export type Signal = {
  id: string
  name: string
  rule: string
  source: string
  position_today: -1 | 0 | 1
  today: string
  flip_level: number | null
  tests: SignalTest[]
}

export type Signals = {
  first_date: string
  last_date: string
  n_days: number
  round_trip_cost: number
  n_tests: number
  signals: Signal[]
}

export type Advice = {
  stance: 'buy' | 'sell' | 'hold'
  basis: 'proven' | 'conflict' | 'proven_but_quiet' | 'no_evidence'
  headline: string
  drivers: string[]
  horizon_days: number | null
  review_date: string | null
  confidence: { hit_rate: number; low: number; high: number; n: number } | null
  flip_level: number | null
  reasons: string[]
}

export type AnalyzeResponse = {
  report: AnalysisReport
  signals: Signals | null
  advice: Advice | null
  signals_note: string | null
  data: {
    symbol: string
    name: string | null
    currency: string | null
    exchange: string | null
    instrument_type: string | null
    tradingview_symbol: string | null
    range: string
    source: string
    fetched_at: string
    quality: QualityReport
  }
  benchmark: null | {
    symbol: string
    name: string | null
    selection: 'auto' | 'requested'
    used: boolean
    currency: string | null
    note: string | null
  }
}

export function fetchAnalysis(symbol: string, range = '5y'): Promise<AnalyzeResponse> {
  return request<AnalyzeResponse>(`/api/v1/analyze?symbol=${encodeURIComponent(symbol)}&range=${range}`)
}

/** One cached analysis per symbol, shared by every panel that needs it. */
export function useAnalysis(symbol: string) {
  return useQuery<AnalyzeResponse>({
    queryKey: ['analysis', symbol],
    queryFn: () => fetchAnalysis(symbol),
    staleTime: 15 * 60_000,
    retry: (count, error) => count < 1 && !/^4\d\d/.test(error.message),
  })
}

// ---- news (/api/v1/news) ----------------------------------------------------------------

export type NewsImpact = {
  instrument_move: number
  market_move: number | null
  abnormal_move: number
  sessions_elapsed: number
  z: number
  one_in: number
}

export type NewsResponse = {
  symbol: string
  query: string
  articles: {
    article: { title: string; url: string; domain: string; source_country: string | null; seen_at: string }
    impact: NewsImpact | null
  }[]
  tone: null | {
    recent_mean: number
    baseline_mean: number
    baseline_sd: number
    n_recent: number
    n_baseline: number
    z: number
    label: string
  }
  tone_note: string | null
  benchmark: string | null
  beta: number | null
  source: string
  fetched_at: string
}

export function fetchNews(symbol: string): Promise<NewsResponse> {
  return request<NewsResponse>(`/api/v1/news?symbol=${encodeURIComponent(symbol)}`)
}

// ---- comparison (/api/v1/compare) --------------------------------------------------------

export type Comparison = {
  symbols: string[]
  first_date: string
  last_date: string
  n_common_days: number
  n_weekly: number
  correlation_daily: number[][]
  spearman_daily: number[][]
  correlation_weekly: number[][]
  covariance_shrinkage: number
  covariance_annual: number[][]
  pca: { explained: number[]; first_loadings: number[] }
  pairs: {
    a: string
    b: string
    correlation_daily: number
    correlation_weekly: number
    cointegration: {
      dependent: string
      regressor: string
      hedge_ratio: number
      statistic: number | null
      lags: number
      critical_values: [number, number, number]
      cointegrated_5pct: boolean
      half_life_days: number | null
      spread_z_now: number
    }
  }[]
  rolling_correlation: { a: string; b: string; points: [string, number][] }[]
  performance: {
    symbol: string
    total_return: number
    annual_return: number
    annual_vol: number
    sharpe: number
    max_drawdown: number
  }[]
  rebased: { date: string; values: number[] }[]
  findings: string[]
}

export type CompareResponse = {
  comparison: Comparison
  instruments: {
    symbol: string
    name: string | null
    currency: string | null
    exchange: string | null
    timezone: string | null
    source: string
    fetched_at: string
  }[]
  range: string
  notes: string[]
}

export function fetchComparison(symbols: string[], range = '5y'): Promise<CompareResponse> {
  return request<CompareResponse>(
    `/api/v1/compare?symbols=${encodeURIComponent(symbols.join(','))}&range=${range}`,
  )
}
