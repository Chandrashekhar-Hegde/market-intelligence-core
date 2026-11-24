//! Single-instrument analysis engine. Pure: callers pass prices in; nothing here touches the
//! network, the clock, or unseeded randomness. Works on any price series (stock, index, ETF,
//! crypto, FX, any exchange). A benchmark is optional and only adds the exposure section.

pub mod distribution;
pub mod montecarlo;
pub mod numeric;
pub mod predictability;
pub mod risk;
pub mod validation;
pub mod volatility;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use distribution::Distribution;
pub use montecarlo::Projection;
pub use predictability::Predictability;
pub use risk::{BenchmarkExposure, Risk};
pub use validation::Validation;
pub use volatility::Volatility;

/// Below this many daily returns the statistics are too noisy to report.
pub const MIN_RETURNS: usize = 60;
pub const DEFAULT_HORIZONS: [usize; 3] = [5, 21, 63];
pub const DEFAULT_PATHS: usize = 2_000;
pub const MAX_PATHS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub statistic: f64,
    pub p_value: f64,
    pub reject_5pct: bool,
}

impl TestResult {
    pub fn new(statistic: f64, p_value: f64) -> Self {
        Self {
            statistic,
            p_value,
            reject_5pct: p_value < 0.05,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum AnalysisError {
    #[error("need at least {MIN_RETURNS} daily returns, got {0}")]
    TooShort(usize),
    #[error("prices must be positive and finite")]
    BadPrices,
}

pub struct AnalysisInput<'a> {
    pub symbol: &'a str,
    /// Adjusted closes, oldest first, one per trading day.
    pub prices: &'a [(NaiveDate, f64)],
    pub benchmark: Option<(&'a str, &'a [(NaiveDate, f64)])>,
    pub horizons: &'a [usize],
    pub n_paths: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub symbol: String,
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    pub n_returns: usize,
    pub last_price: f64,
    /// Trading days per year observed in this series (≈252 for most exchanges, 365 for crypto).
    pub periods_per_year: f64,
    pub distribution: Distribution,
    pub predictability: Predictability,
    pub volatility: Volatility,
    pub projections: Vec<Projection>,
    pub risk: Risk,
    pub benchmark: Option<BenchmarkExposure>,
    /// Out-of-sample scores of the 1-day forecasts; None below 600 returns.
    pub validation: Option<Validation>,
    /// Plain-language conclusions, each tied to a number above.
    pub findings: Vec<String>,
}

pub fn analyze(input: &AnalysisInput) -> Result<AnalysisReport, AnalysisError> {
    let prices = input.prices;
    if prices.iter().any(|(_, p)| !p.is_finite() || *p <= 0.0) {
        return Err(AnalysisError::BadPrices);
    }
    if prices.len() < MIN_RETURNS + 1 {
        return Err(AnalysisError::TooShort(prices.len().saturating_sub(1)));
    }
    let log_prices = prices.iter().map(|(_, p)| p.ln()).collect::<Vec<_>>();
    let returns = log_prices
        .windows(2)
        .map(|w| w[1] - w[0])
        .collect::<Vec<_>>();
    let (first, last) = (prices[0].0, prices[prices.len() - 1].0);
    let years = (last - first).num_days() as f64 / 365.25;
    let periods_per_year = returns.len() as f64 / years;

    let distribution = distribution::describe(&returns, periods_per_year);
    let predictability = predictability::test(&returns, &log_prices);
    let volatility = volatility::estimate(&returns, periods_per_year);
    let projections = montecarlo::project(&montecarlo::Inputs {
        last_price: prices[prices.len() - 1].1,
        mu: distribution.mean_daily,
        sigma: distribution.vol_daily,
        garch: volatility.garch.as_ref(),
        horizons: input.horizons,
        n_paths: input.n_paths.clamp(100, MAX_PATHS),
        seed: input.seed,
    });
    let risk = risk::measure(
        &returns,
        prices,
        volatility.garch.as_ref(),
        periods_per_year,
    );
    let benchmark = input
        .benchmark
        .and_then(|(symbol, bars)| risk::exposure(symbol, prices, bars, periods_per_year));

    let validation = validation::validate(&returns);

    let mut report = AnalysisReport {
        symbol: input.symbol.to_string(),
        first_date: first,
        last_date: last,
        n_returns: returns.len(),
        last_price: prices[prices.len() - 1].1,
        periods_per_year,
        distribution,
        predictability,
        volatility,
        projections,
        risk,
        benchmark,
        validation,
        findings: Vec::new(),
    };
    report.findings = findings(&report);
    Ok(report)
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

fn p(x: f64) -> String {
    if x < 0.001 {
        "p < 0.001".into()
    } else {
        format!("p = {x:.3}")
    }
}

fn findings(r: &AnalysisReport) -> Vec<String> {
    let d = &r.distribution;
    let pr = &r.predictability;
    let mut out = vec![format!(
        "Annualised volatility {} over {} trading days; average return {} a year (± {} standard error, so the drift is poorly known).",
        pct(d.vol_annual),
        r.n_returns,
        pct(d.mean_annual),
        pct(d.mean_annual_se)
    )];
    out.push(if d.jarque_bera.reject_5pct {
        format!(
            "Returns are not normal: excess kurtosis {:.1}, skew {:.2} (Jarque–Bera {}). Expect more extreme days than a bell curve implies{}.",
            d.excess_kurtosis,
            d.skewness,
            p(d.jarque_bera.p_value),
            d.hill_tail_index.map_or(String::new(), |a| format!("; loss-tail index {a:.1}"))
        )
    } else {
        format!("Normality is not rejected (Jarque–Bera {}).", p(d.jarque_bera.p_value))
    });
    let vr_sig = pr
        .variance_ratios
        .iter()
        .filter(|v| v.test.reject_5pct)
        .collect::<Vec<_>>();
    out.push(if !pr.ljung_box_returns.reject_5pct && vr_sig.is_empty() {
        format!(
            "No evidence of predictability in returns: Ljung–Box {} and no variance ratio is significant. Treat forecasts as risk ranges, not directional calls.",
            p(pr.ljung_box_returns.p_value)
        )
    } else {
        let direction = vr_sig
            .first()
            .map_or("autocorrelated", |v| if v.ratio > 1.0 { "trending (momentum)" } else { "mean-reverting" });
        format!(
            "Some evidence returns are {direction}: Ljung–Box {}{}. Small, and it must survive out-of-sample validation before it means anything.",
            p(pr.ljung_box_returns.p_value),
            vr_sig.first().map_or(String::new(), |v| format!(", VR({}) = {:.2} ({})", v.q, v.ratio, p(v.test.p_value)))
        )
    });
    if pr.ljung_box_squared.reject_5pct {
        out.push(match &r.volatility.garch {
            Some(g) => format!(
                "Volatility clusters (ARCH effects {}): GARCH persistence {:.3}, shocks halve in about {:.0} trading days. Volatility now {} vs long-run {}.",
                p(pr.ljung_box_squared.p_value),
                g.persistence,
                g.half_life_days,
                pct(g.current_vol_annual),
                pct(g.long_run_vol_annual)
            ),
            None => format!("Volatility clusters (ARCH effects {}).", p(pr.ljung_box_squared.p_value)),
        });
    }
    if let Some(v) = r
        .risk
        .var_es
        .iter()
        .find(|v| v.method == "historical" && v.confidence == 0.99)
    {
        out.push(format!(
            "On 1 day in 100 the loss has historically exceeded {}; the average of those days was {}. Worst drawdown {} ({} → {}){}.",
            pct(v.var),
            pct(v.es),
            pct(r.risk.max_drawdown.max),
            r.risk.max_drawdown.peak_date,
            r.risk.max_drawdown.trough_date,
            if r.risk.max_drawdown.recovery_date.is_none() { ", not yet recovered" } else { "" }
        ));
    }
    if let Some(b) = &r.benchmark {
        out.push(format!(
            "Beta {:.2} ± {:.2} against {}; the benchmark explains {} of daily moves. Alpha {} a year (± {}).",
            b.beta,
            b.beta_se,
            b.symbol,
            pct(b.r_squared),
            pct(b.alpha_annual),
            pct(b.alpha_annual_se)
        ));
    }
    if let Some(v) = &r.validation {
        let winners = v
            .models
            .iter()
            .filter(|m| m.verdict.starts_with("Earns its place"))
            .map(|m| m.model.as_str())
            .collect::<Vec<_>>();
        out.push(if winners.is_empty() {
            format!(
                "Out of sample ({} daily forecasts), no model beat historical simulation with VaR that holds up. Treat the fans as plausible ranges only.",
                v.n_forecasts
            )
        } else {
            format!(
                "Out of sample ({} daily forecasts), {} beat historical simulation and passed the VaR backtests.",
                v.n_forecasts,
                winners.join(" and ")
            )
        });
    }
    if !pr.adf.reject_unit_root_5pct {
        out.push("Price behaves like a random walk (ADF does not reject a unit root): past levels don't pull it back.".into());
    }
    out
}

#[cfg(test)]
mod tests;
