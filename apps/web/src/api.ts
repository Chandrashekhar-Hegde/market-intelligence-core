export type NormalizedEvent = {
  event_id: string
  version: number
  causal_parent_id: string | null
  event_type: string | null
  headline: string
  body: string
  occurred_at: string
  symbol: string | null
  sector: string | null
  source: string | null
  region: string | null
  impact_level: string | null
  impact_category: string | null
}

export type MacroContext = {
  sp500_futures_change: number
  nasdaq_futures_change: number
  brent_crude_change: number
  usd_inr_change: number
  fii_net_flow: number
  gold_change: number
  total_macro_score: number
}

export type DecisionRequest = {
  event: NormalizedEvent
  facts: {
    macro_context: MacroContext
    entry_price: number | null
    exchange: string | null
    features: unknown | null
    prediction: unknown | null
    event_study: { abnormal_returns: number[] } | null
    kg_modifier: number
  }
}

export type HealthResponse = {
  status: string
  service: string
}

export type ProviderStatus = {
  name: string
  kind: string
  mode: string
  health: string
  rate_limit: {
    limit: number
    remaining: number
    reset_at: string | null
  }
  retry: {
    max_attempts: number
    attempts_used: number
    backoff_ms: number
  }
  circuit_breaker: {
    state: string
    failure_count: number
    opened_at: string | null
  }
  last_error: string | null
}

export type PaymentEvent = {
  event_id: string
  provider: string
  event_type: string
  provider_order_id: string | null
  provider_payment_id: string | null
  verified: boolean
  received_at: string
}

export type PaymentState = {
  provider: ProviderStatus
  mode: string
  live_billing_enabled: boolean
  checkout_verification: string
  webhook_verification: string
  recent_events: PaymentEvent[]
}

export type PaymentOrderRequest = {
  account_id: string
  amount_paise: number
  currency: string
  description: string
  success_url: string
}

export type PaymentOrder = {
  provider: string
  key_id: string
  account_id: string
  order_id: string
  checkout_id: string
  receipt: string
  amount_paise: number
  currency: string
  status: string
  test_payment_id: string
  test_signature: string
}

export type PaymentVerificationRequest = {
  order_id: string
  payment_id: string
  signature: string
}

export type PaymentVerification = {
  provider: string
  order_id: string
  payment_id: string
  verified: boolean
}

export type SourceReliability = {
  tier: string
  score: number
  rationale: string
}

export type EventReviewSummary = {
  event_id: string
  version: number
  headline: string
  occurred_at: string
  source: string | null
  region: string | null
  sector: string | null
  symbol: string | null
  event_class: string
  confidence: number
  severity: string
  entity_mapping_status: string
  source_reliability: SourceReliability
}

export type EventReviewDetail = {
  summary: EventReviewSummary
  event: NormalizedEvent
  raw_source: {
    provider: string
    source_id: string
    url: string | null
    received_at: string
    language: string
    raw_headline: string
  }
  normalized_facts: {
    event_type: string | null
    symbol: string | null
    sector: string | null
    region: string | null
    impact_level: string | null
    impact_category: string | null
  }
  entity_mappings: Array<{
    entity_id: string
    entity_type: string
    label: string
    confidence: number
  }>
  source_reliability: SourceReliability
}

export type DecisionAction = 'BUY' | 'SELL' | 'HOLD'

export type CandidateAction = DecisionAction | 'PAPER'

export type EvidenceItem = {
  evidence_type: string
  label: string
  contribution: number
  confidence: number
}

export type EventStudySummary = {
  sample_count: number
  cumulative_abnormal_return: number | null
  mean_abnormal_return: number | null
  hit_rate: number | null
  t_stat: number | null
  calibrated_weight: number
  calibrated_confidence: number
}

export type ImpactEstimate = {
  combined_score: number
  expected_return: number
  downside: number
  event_study: EventStudySummary | null
}

export type GateReport = {
  name: string
  passed: boolean
  reason: string
}

export type UtilityEstimate = {
  action: CandidateAction
  expected_utility: number
}

export type ModelExplanation = {
  model_version: string
  input_hash: string
  pipeline: string[]
  entity_resolution: {
    symbol: string | null
    sector: string | null
    region: string | null
    confidence: number
  }
  evidence: EvidenceItem[]
  impact: ImpactEstimate
  gates: GateReport[]
  utilities: UtilityEstimate[]
  recommended_action: DecisionAction
  confidence: number
  missing_facts: string[]
  summary: string
}

export type Decision = {
  decision_id: string
  parent_event_id: string
  parent_event_version: number
  action: DecisionAction
  total_score: number
  confidence: number
  position_size: number
  quantity: number | null
  entry_price: number | null
  target_price: number | null
  stop_loss: number | null
  timing: string | null
  exchange: string | null
  symbol: string | null
  sector: string | null
  thesis: string
  reasons: Array<{ rule_id?: string; contribution?: number; rationale?: string }>
  model_version: string
  input_hash: string
  expected_return: number | null
  downside: number | null
  explanation: ModelExplanation
  execution_ready: boolean
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

export function fetchPaymentState(): Promise<PaymentState> {
  return request<PaymentState>('/payments/state')
}

export function createPaymentOrder(payload: PaymentOrderRequest): Promise<PaymentOrder> {
  return request<PaymentOrder>('/payments/orders', {
    method: 'POST',
    body: JSON.stringify(payload),
  })
}

export function verifyPayment(payload: PaymentVerificationRequest): Promise<PaymentVerification> {
  return request<PaymentVerification>('/payments/verify', {
    method: 'POST',
    body: JSON.stringify(payload),
  })
}

export function fetchEvents(): Promise<EventReviewSummary[]> {
  return request<EventReviewSummary[]>('/events')
}

export function fetchEvent(eventId: string): Promise<EventReviewDetail> {
  return request<EventReviewDetail>(`/events/${encodeURIComponent(eventId)}`)
}

export function createDecision(payload: DecisionRequest): Promise<Decision> {
  return request<Decision>('/decide', {
    method: 'POST',
    body: JSON.stringify(payload),
  })
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
  market_open: boolean
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

export type BarsResponse = { chart: ChartData; quality: QualityReport }

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
