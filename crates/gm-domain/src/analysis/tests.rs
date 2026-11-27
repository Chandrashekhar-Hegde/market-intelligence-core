//! The engine against independent reference outputs (scipy, statsmodels, arch) on real data:
//! AAPL and the S&P 500 (US), NIFTY 50 (India), 5 years of daily adjusted closes.
//! Regenerate with scripts/gen_reference.py.

use chrono::NaiveDate;
use serde_json::Value;

use super::*;

const AAPL: &str = include_str!("../../tests/fixtures/aapl_5y.csv");
const GSPC: &str = include_str!("../../tests/fixtures/gspc_5y.csv");
const NSEI: &str = include_str!("../../tests/fixtures/nsei_5y.csv");
const REFERENCE: &str = include_str!("../../tests/fixtures/reference.json");

fn load(csv: &str) -> Vec<(NaiveDate, f64)> {
    csv.lines()
        .skip(1)
        .map(|line| {
            let (d, p) = line.split_once(',').unwrap();
            (d.parse().unwrap(), p.parse().unwrap())
        })
        .collect()
}

fn reference() -> Value {
    serde_json::from_str(REFERENCE).unwrap()
}

fn report(csv: &str, bench: Option<(&str, &[(NaiveDate, f64)])>) -> AnalysisReport {
    let prices = load(csv);
    analyze(&AnalysisInput {
        symbol: "X",
        prices: &prices,
        benchmark: bench,
        horizons: &DEFAULT_HORIZONS,
        n_paths: 2000,
        seed: 7,
    })
    .unwrap()
}

#[track_caller]
fn close(actual: f64, expected: &Value, rel: f64, what: &str) {
    let expected = expected.as_f64().unwrap();
    let err = (actual - expected).abs() / expected.abs().max(1e-12);
    assert!(
        err <= rel,
        "{what}: rust {actual} vs reference {expected} (rel err {err:.2e} > {rel:.0e})"
    );
}

fn check_series(csv: &str, key: &str) {
    let r = report(csv, None);
    let x = &reference()[key];
    let d = &r.distribution;
    assert_eq!(
        r.n_returns as u64,
        x["n_returns"].as_u64().unwrap(),
        "{key} n"
    );
    close(
        r.periods_per_year,
        &x["periods_per_year"],
        1e-12,
        "periods/year",
    );
    close(d.mean_daily, &x["mean_daily"], 1e-10, "mean");
    close(d.vol_daily, &x["vol_daily"], 1e-10, "vol");
    close(d.skewness, &x["skewness"], 1e-9, "skew");
    close(d.excess_kurtosis, &x["excess_kurtosis"], 1e-9, "kurtosis");
    close(d.jarque_bera.statistic, &x["jarque_bera"][0], 1e-9, "JB");
    close(
        d.hill_tail_index.unwrap(),
        &x["hill_tail_index"],
        1e-9,
        "Hill",
    );

    let p = &r.predictability;
    close(
        p.ljung_box_returns.statistic,
        &x["ljung_box"][0],
        1e-9,
        "LB stat",
    );
    close(
        p.ljung_box_returns.p_value,
        &x["ljung_box"][1],
        1e-6,
        "LB p",
    );
    close(
        p.ljung_box_squared.statistic,
        &x["ljung_box_squared"][0],
        1e-9,
        "LB² stat",
    );
    for vr in &p.variance_ratios {
        let rx = &x["variance_ratio"][vr.q.to_string()];
        close(vr.ratio, &rx[0], 1e-9, &format!("VR({}) ratio", vr.q));
        close(vr.test.statistic, &rx[1], 1e-6, &format!("VR({}) z", vr.q));
        close(vr.test.p_value, &rx[2], 1e-5, &format!("VR({}) p", vr.q));
    }
    assert_eq!(p.adf.lags as u64, x["adf"]["lags"].as_u64().unwrap());
    assert_eq!(p.adf.n_obs as u64, x["adf"]["nobs"].as_u64().unwrap());
    close(p.adf.statistic, &x["adf"]["stat"], 1e-6, "ADF stat");
    for i in 0..3 {
        close(
            p.adf.critical_values[i],
            &x["adf"]["crit"][i],
            1e-6,
            "ADF crit",
        );
    }

    close(r.volatility.ewma_annual, &x["ewma_annual"], 1e-9, "EWMA");
    // GARCH: two optimisers on a flat likelihood agree on the fit, not every digit of every
    // parameter. The log-likelihood is the sharp check; parameters get looser bounds.
    let g = r.volatility.garch.as_ref().unwrap();
    let gx = &x["garch"];
    assert!(g.converged, "{key} GARCH did not converge");
    let ll_ref = gx["loglik"].as_f64().unwrap();
    assert!(
        g.log_likelihood >= ll_ref - 1e-3,
        "{key} loglik {} < arch {}",
        g.log_likelihood,
        ll_ref
    );
    close(g.log_likelihood, &gx["loglik"], 1e-6, "GARCH loglik");
    close(g.alpha, &gx["alpha"], 0.03, "GARCH alpha");
    close(g.beta, &gx["beta"], 0.01, "GARCH beta");
    close(g.nu, &gx["nu"], 0.03, "GARCH nu");
    close(g.omega, &gx["omega"], 0.15, "GARCH omega");

    for level in ["0.95", "0.99"] {
        let c: f64 = level.parse().unwrap();
        let find = |m: &str| {
            r.risk
                .var_es
                .iter()
                .find(|v| v.method == m && v.confidence == c)
                .unwrap()
        };
        let h = find("historical");
        close(h.var, &x["historical"][level][0], 1e-12, "hist VaR");
        close(h.es, &x["historical"][level][1], 1e-12, "hist ES");
        let t = find("student_t");
        close(t.var, &x["student_t"][level][0], 1e-8, "t VaR");
        close(t.es, &x["student_t"][level][1], 1e-6, "t ES");
    }
    close(
        r.risk.max_drawdown.max,
        &x["max_drawdown"],
        1e-12,
        "max drawdown",
    );
    close(r.risk.sharpe, &x["sharpe"], 1e-9, "Sharpe");

    // The lead forecast follows validation: GARCH-t only when it earns its place.
    let v = r.validation.as_ref().expect("5 years validates");
    let garch_earns = v
        .models
        .iter()
        .any(|m| m.model == "garch_t" && m.verdict.starts_with("Earns its place"));
    assert_eq!(
        r.lead_model,
        if garch_earns { "garch_t" } else { "bootstrap" },
        "{key}"
    );
    assert_eq!(
        v.chance_of_gain.len(),
        3,
        "{key}: 5 years scores all horizons"
    );
    for c in &v.chance_of_gain {
        assert!((0.0..=1.0).contains(&c.brier) && c.n >= 10, "{key}: {c:?}");
    }
}

#[test]
fn aapl_matches_reference_libraries() {
    check_series(AAPL, "aapl");
}

#[test]
fn sp500_matches_reference_libraries() {
    check_series(GSPC, "gspc");
}

#[test]
fn nifty_matches_reference_libraries() {
    check_series(NSEI, "nsei");
}

#[test]
fn beta_against_benchmark_matches_statsmodels() {
    let gspc = load(GSPC);
    let r = report(AAPL, Some(("^GSPC", &gspc)));
    let b = r.benchmark.unwrap();
    let x = &reference()["beta_aapl_gspc"];
    assert_eq!(b.n_common_days as u64, x["n_common_days"].as_u64().unwrap());
    close(b.beta, &x["beta"], 1e-9, "beta");
    close(b.beta_se, &x["beta_se"], 1e-9, "beta se");
    close(b.r_squared, &x["r_squared"], 1e-9, "R²");
    close(
        b.alpha_annual / r.periods_per_year,
        &x["alpha_daily"],
        1e-9,
        "alpha",
    );
    assert!(
        r.findings.iter().any(|f| f.contains("Beta 1.14")),
        "{:?}",
        r.findings
    );
}

#[test]
fn same_seed_same_projection_different_seed_different() {
    let a = report(NSEI, None);
    let b = report(NSEI, None);
    assert_eq!(
        serde_json::to_string(&a.projections).unwrap(),
        serde_json::to_string(&b.projections).unwrap()
    );
    let prices = load(NSEI);
    let c = analyze(&AnalysisInput {
        symbol: "X",
        prices: &prices,
        benchmark: None,
        horizons: &DEFAULT_HORIZONS,
        n_paths: 2000,
        seed: 8,
    })
    .unwrap();
    assert_ne!(a.projections[0].fan, c.projections[0].fan);
}

#[test]
fn projections_are_coherent_distributions() {
    let r = report(AAPL, None);
    let models: Vec<_> = r.projections.iter().map(|p| p.model.as_str()).collect();
    assert_eq!(models, ["gbm", "garch_t", "bootstrap"]);
    for proj in &r.projections {
        assert_eq!(proj.sample_paths.len(), super::montecarlo::SAMPLE_PATHS);
        assert_eq!(proj.fan.len(), 63);
        for pt in &proj.fan {
            assert!(
                pt.price.windows(2).all(|w| w[0] <= w[1]),
                "quantiles ordered"
            );
        }
        // Wider fan further out.
        let width = |d: usize| proj.fan[d].price[4] - proj.fan[d].price[0];
        assert!(width(62) > width(20) && width(20) > width(4));
        for h in &proj.horizons {
            assert!((0.0..=1.0).contains(&h.prob_gain));
            assert!(
                h.prob_drawdown_over_10pct >= h.prob_loss_over_10pct,
                "drawdown ⊇ terminal loss"
            );
        }
    }
    // GBM's 63-day 5–95% band should be close to ±1.645·σ·√63 around the median.
    let gbm = &r.projections[0].horizons[2];
    let sigma = r.distribution.vol_daily * 63f64.sqrt();
    let implied = ((1.0 + gbm.return_quantiles[4]).ln() - (1.0 + gbm.return_quantiles[0]).ln())
        / (2.0 * 1.645);
    assert!(
        (implied / sigma - 1.0).abs() < 0.05,
        "implied {implied} vs {sigma}"
    );
}

#[test]
fn garch_recovers_known_parameters_from_simulation() {
    use rand::SeedableRng;
    use rand_distr::{Distribution, StudentT};
    let (omega, alpha, beta, nu): (f64, f64, f64, f64) = (2e-6, 0.08, 0.90, 6.0);
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
    let t = StudentT::new(nu).unwrap();
    let scale = ((nu - 2.0) / nu).sqrt();
    let mut var: f64 = omega / (1.0 - alpha - beta);
    let returns = (0..6000)
        .map(|_| {
            let e = var.sqrt() * t.sample(&mut rng) * scale;
            var = omega + alpha * e * e + beta * var;
            e
        })
        .collect::<Vec<_>>();
    let g = volatility::fit_garch(&returns, 252.0).unwrap();
    assert!((g.alpha - alpha).abs() < 0.03, "alpha {}", g.alpha);
    assert!((g.beta - beta).abs() < 0.03, "beta {}", g.beta);
    assert!((g.nu - nu).abs() < 1.5, "nu {}", g.nu);
}

#[test]
fn white_noise_is_reported_unpredictable_and_normal() {
    use rand::SeedableRng;
    use rand_distr::{Distribution, StandardNormal};
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(3);
    let mut price = 100.0f64;
    let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let prices = (0..1000)
        .map(|i| {
            let z: f64 = StandardNormal.sample(&mut rng);
            price *= (0.01 * z).exp();
            (start + chrono::Days::new(i), price)
        })
        .collect::<Vec<_>>();
    let r = analyze(&AnalysisInput {
        symbol: "NOISE",
        prices: &prices,
        benchmark: None,
        horizons: &[5],
        n_paths: 200,
        seed: 1,
    })
    .unwrap();
    assert!(!r.predictability.ljung_box_returns.reject_5pct);
    assert!(!r.distribution.jarque_bera.reject_5pct);
    assert!(!r.predictability.adf.reject_unit_root_5pct);
    // Calendar-day data: ~365 periods per year, as for crypto.
    assert!((r.periods_per_year - 365.25).abs() < 1.0);
    assert!(
        r.findings
            .iter()
            .any(|f| f.starts_with("No evidence of predictability"))
    );
}

#[test]
fn rejects_short_or_bad_series() {
    let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let short = (0..30)
        .map(|i| (start + chrono::Days::new(i), 100.0))
        .collect::<Vec<_>>();
    let input = |p| AnalysisInput {
        symbol: "X",
        prices: p,
        benchmark: None,
        horizons: &[5],
        n_paths: 100,
        seed: 0,
    };
    assert_eq!(
        analyze(&input(&short)).unwrap_err(),
        AnalysisError::TooShort(29)
    );
    let mut bad = load(NSEI);
    bad[10].1 = -1.0;
    assert_eq!(analyze(&input(&bad)).unwrap_err(), AnalysisError::BadPrices);
}

#[test]
fn near_normal_returns_do_not_hang_the_t_quantile() {
    // Regression: kurtosis ≈ 0 gave ν ≈ 1e6 and statrs' t inverse CDF never returned.
    let at = |nu: f64| risk::student_t_var_es("t", 0.99, 0.0, 0.01, nu).unwrap();
    for nu in [f64::INFINITY, 1e6, 250.0, 150.0, 5.0] {
        let v = at(nu);
        assert!(v.var.is_finite() && v.es > v.var, "nu {nu}: {v:?}");
    }
    // Continuity across the switch to the normal: ν = 150 (t) vs ν = 250 (normal).
    assert!((at(150.0).var / at(250.0).var - 1.0).abs() < 0.01);
    // Fat tails cost more: t(5) 99% VaR exceeds the normal's.
    assert!(at(5.0).var > at(f64::INFINITY).var);
}

/// Trading rules and their walk-forward tests against pandas/statsmodels (HAC, uniform kernel)
/// on AAPL closes with the same synthetic volume series as scripts/gen_reference.py.
#[test]
fn signals_match_statsmodels_on_aapl() {
    use super::signals::{SignalInput, evaluate, obv_series, rsi_series};
    let prices = load(AAPL);
    let dates: Vec<NaiveDate> = prices.iter().map(|(d, _)| *d).collect();
    let closes: Vec<f64> = prices.iter().map(|(_, p)| *p).collect();
    let volume: Vec<f64> = (0..closes.len())
        .map(|i| 1e6 * (1.0 + 0.5 * (i as f64 / 7.0).sin()) + 1e5 * (i % 13) as f64)
        .collect();
    let x = &reference()["signals_aapl"];

    let rsi = rsi_series(&closes, 14);
    close(rsi[20].unwrap(), &x["rsi_20"], 1e-9, "rsi[20]");
    close(
        rsi.last().unwrap().unwrap(),
        &x["rsi_last"],
        1e-9,
        "rsi last",
    );
    close(
        *obv_series(&closes, &volume).last().unwrap(),
        &x["obv_last"],
        1e-9,
        "obv",
    );

    let s = evaluate(&SignalInput {
        dates: &dates,
        close: &closes,
        volume: Some(&volume),
    })
    .unwrap();
    for sig in &s.signals {
        assert_eq!(
            f64::from(sig.position_today),
            x["positions_today"][&sig.id].as_f64().unwrap(),
            "{} today",
            sig.id
        );
    }
    let expected = x["tests"].as_array().unwrap();
    assert_eq!(s.n_tests, expected.len());
    for e in expected {
        let id = e["signal"].as_str().unwrap();
        let h = e["h"].as_u64().unwrap() as usize;
        let t = s
            .signals
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.tests.iter().find(|t| t.horizon_days == h))
            .unwrap_or_else(|| panic!("{id} h={h} missing"));
        let what = |f: &str| format!("{id} h={h} {f}");
        assert_eq!(
            t.n_obs as u64,
            e["n_obs"].as_u64().unwrap(),
            "{}",
            what("n_obs")
        );
        assert_eq!(
            t.n_independent as u64,
            e["n_independent"].as_u64().unwrap(),
            "{}",
            what("n_ind")
        );
        assert_eq!(
            t.hit_n as u64,
            e["hit_n"].as_u64().unwrap(),
            "{}",
            what("hit_n")
        );
        close(t.effect, &e["effect"], 1e-8, &what("effect"));
        close(t.t_stat, &e["t_stat"], 1e-8, &what("t"));
        close(t.p_value, &e["p_value"], 1e-6, &what("p"));
        close(t.p_holm, &e["p_holm"], 1e-6, &what("p_holm"));
        close(
            t.effect_first_half,
            &e["first_half"],
            1e-8,
            &what("first half"),
        );
        close(
            t.effect_second_half,
            &e["second_half"],
            1e-8,
            &what("second half"),
        );
        close(t.hit_rate.unwrap(), &e["hit_rate"], 1e-12, &what("hit"));
        close(t.hit_low.unwrap(), &e["hit_low"], 1e-9, &what("hit low"));
        close(t.hit_high.unwrap(), &e["hit_high"], 1e-9, &what("hit high"));
    }
}

/// The stationary-bootstrap block length (on squared returns) matches arch's
/// optimal_block_length on all three real series.
#[test]
fn bootstrap_block_length_matches_arch() {
    for (csv, key) in [(AAPL, "aapl"), (GSPC, "gspc"), (NSEI, "nsei")] {
        let prices = load(csv);
        let squared: Vec<f64> = prices
            .windows(2)
            .map(|w| (w[1].1 / w[0].1).ln().powi(2))
            .collect();
        close(
            super::montecarlo::optimal_block_length(&squared),
            &reference()[key]["block_length_sq"],
            1e-9,
            key,
        );
    }
}

/// The live fan starts at the live price, and a big move today widens the GARCH fan at once;
/// the history-only models just move with the price.
#[test]
fn live_projection_reanchors_and_reacts_to_todays_move() {
    let prices = load(AAPL);
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1].1 / w[0].1).ln()).collect();
    let garch = volatility::fit_garch(&returns, 252.0).unwrap();
    let last = prices.last().unwrap().1;
    let run = |live: f64| {
        live_projections(&LiveInput {
            symbol: "AAPL",
            returns: &returns,
            garch: Some(&garch),
            last_close: last,
            live_price: live,
            horizons: &[5],
            n_paths: 2000,
            seed: 42,
        })
    };
    let calm = run(last);
    let jump = run(last * 1.08);
    let width = |ps: &[Projection], m: &str| {
        let h = &ps.iter().find(|p| p.model == m).unwrap().horizons[0];
        h.return_quantiles[4] - h.return_quantiles[0]
    };
    assert!(width(&jump, "garch_t") > 1.3 * width(&calm, "garch_t"));
    // Own-history and GBM fans keep their shape in % terms and only move with the price.
    for m in ["bootstrap", "gbm"] {
        assert!((width(&jump, m) - width(&calm, m)).abs() < 1e-12, "{m}");
        let a = &calm.iter().find(|p| p.model == m).unwrap().fan[0].price[2];
        let b = &jump.iter().find(|p| p.model == m).unwrap().fan[0].price[2];
        assert!((b / a - 1.08).abs() < 1e-9, "{m}: {a} → {b}");
    }
}
