//! A mix of the holdings someone picked, for the money they have: equal risk from each
//! (Maillard, Roncalli & Teïletche 2010), with correlations, bought in whole shares.
//! With no proven edge between them this is the evidence-based default, not a forecast.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub symbol: String,
    pub price: f64,
    pub shares: u64,
    pub amount: f64,
    /// Share of the money invested, and the equal-risk target it was rounded from.
    pub weight: f64,
    pub target: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Allocation {
    pub lines: Vec<Line>,
    pub invested: f64,
    pub cash_left: f64,
    /// Holdings dropped because the amount can't buy a sensible number of shares: (symbol, price).
    pub too_dear: Vec<(String, f64)>,
    /// Volatility a year of the mix as bought, and the average of its holdings alone.
    pub vol_annual: f64,
    pub vol_alone_avg: f64,
}

/// Fixed-point iterations for equal risk contribution; it converges in tens for positive
/// covariance matrices (Chaves, Hsu, Li & Shakernia 2012). Damped by half for stability.
const ERC_ITERATIONS: usize = 500;

/// Weights where each holding contributes the same share of the mix's variance:
/// w_i·(Σw)_i equal for all i, weights summing to one.
pub fn erc_weights(cov: &[Vec<f64>]) -> Vec<f64> {
    let n = cov.len();
    let mut w: Vec<f64> = (0..n).map(|i| 1.0 / cov[i][i].max(1e-12).sqrt()).collect();
    normalise(&mut w);
    for _ in 0..ERC_ITERATIONS {
        let sw = mat_vec(cov, &w);
        let mut next: Vec<f64> = sw.iter().map(|x| 1.0 / x.max(1e-12)).collect();
        normalise(&mut next);
        w.iter_mut()
            .zip(&next)
            .for_each(|(a, b)| *a = 0.5 * *a + 0.5 * b);
    }
    w
}

fn normalise(w: &mut [f64]) {
    let s: f64 = w.iter().sum();
    w.iter_mut().for_each(|x| *x /= s);
}

fn mat_vec(m: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    m.iter()
        .map(|row| row.iter().zip(v).map(|(a, b)| a * b).sum())
        .collect()
}

fn sub_cov(cov: &[Vec<f64>], keep: &[usize]) -> Vec<Vec<f64>> {
    keep.iter()
        .map(|&i| keep.iter().map(|&j| cov[i][j]).collect())
        .collect()
}

/// Spend `budget` on `symbols` (prices in the budget's currency, covariance a year of returns):
/// equal-risk targets, floored to whole shares, the leftover spent one share at a time on the
/// holding furthest below its target. A holding whose single share costs more than its target
/// is dropped (dearest first) and the rest re-balanced, so a small amount gives fewer, buyable
/// holdings instead of rounding them to zero.
pub fn allocate(symbols: &[String], prices: &[f64], cov: &[Vec<f64>], budget: f64) -> Allocation {
    let mut keep: Vec<usize> = (0..symbols.len()).filter(|&i| prices[i] > 0.0).collect();
    let mut too_dear = Vec::new();
    let weights = loop {
        if keep.is_empty() {
            break Vec::new();
        }
        let w = erc_weights(&sub_cov(cov, &keep));
        // The holding whose share price most exceeds its target amount goes first.
        let worst = keep
            .iter()
            .zip(&w)
            .map(|(&i, wi)| (i, prices[i] / (wi * budget)))
            .filter(|(_, r)| *r > 1.0)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match worst {
            Some((i, _)) => {
                too_dear.push((symbols[i].clone(), prices[i]));
                keep.retain(|&k| k != i);
            }
            None => break w,
        }
    };
    let mut shares: Vec<u64> = keep
        .iter()
        .zip(&weights)
        .map(|(&i, w)| (w * budget / prices[i]).floor() as u64)
        .collect();
    let spent = |s: &[u64]| {
        keep.iter()
            .zip(s)
            .map(|(&i, n)| prices[i] * *n as f64)
            .sum::<f64>()
    };
    loop {
        let cash = budget - spent(&shares);
        let next = keep
            .iter()
            .enumerate()
            .filter(|(_, i)| prices[**i] <= cash)
            .map(|(k, &i)| (k, weights[k] * budget - prices[i] * shares[k] as f64))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match next {
            Some((k, gap)) if gap > 0.0 => shares[k] += 1,
            _ => break,
        }
    }
    let invested = spent(&shares);
    let lines: Vec<Line> = keep
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let amount = prices[i] * shares[k] as f64;
            Line {
                symbol: symbols[i].clone(),
                price: prices[i],
                shares: shares[k],
                amount,
                weight: if invested > 0.0 {
                    amount / invested
                } else {
                    0.0
                },
                target: weights[k],
            }
        })
        .collect();
    let actual: Vec<f64> = lines.iter().map(|l| l.weight).collect();
    let sub = sub_cov(cov, &keep);
    let vol_annual = mat_vec(&sub, &actual)
        .iter()
        .zip(&actual)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .max(0.0)
        .sqrt();
    let vol_alone_avg = keep
        .iter()
        .zip(&actual)
        .map(|(&i, w)| w * cov[i][i].sqrt())
        .sum();
    Allocation {
        lines,
        invested,
        cash_left: budget - invested,
        too_dear,
        vol_annual,
        vol_alone_avg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cov3() -> Vec<Vec<f64>> {
        // Vols 15%, 30%, 20%; the first two correlated 0.6, the third 0.1 with each.
        let v = [0.15, 0.30, 0.20];
        let r = [[1.0, 0.6, 0.1], [0.6, 1.0, 0.1], [0.1, 0.1, 1.0]];
        (0..3)
            .map(|i| (0..3).map(|j| r[i][j] * v[i] * v[j]).collect())
            .collect()
    }

    #[test]
    fn each_holding_contributes_the_same_risk() {
        let c = cov3();
        let w = erc_weights(&c);
        let sw = mat_vec(&c, &w);
        let rc: Vec<f64> = w.iter().zip(&sw).map(|(a, b)| a * b).collect();
        let total: f64 = rc.iter().sum();
        assert!(
            rc.iter().all(|x| (x / total - 1.0 / 3.0).abs() < 0.01),
            "{rc:?}"
        );
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        // The jumpiest holding (30%) gets the least.
        assert!(w[1] < w[0] && w[1] < w[2], "{w:?}");
    }

    #[test]
    fn whole_shares_within_budget_and_the_amount_changes_the_answer() {
        let c = cov3();
        let symbols: Vec<String> = ["A", "B", "C"].map(String::from).to_vec();
        let prices = [2500.0, 13_800.0, 100.0];
        let small = allocate(&symbols, &prices, &c, 10_000.0);
        let large = allocate(&symbols, &prices, &c, 1_000_000.0);
        for a in [&small, &large] {
            assert!(a.cash_left >= 0.0, "{a:?}");
            assert!((a.invested - a.lines.iter().map(|l| l.amount).sum::<f64>()).abs() < 1e-6);
        }
        assert!(small.invested <= 10_000.0 && large.invested <= 1_000_000.0);
        // ₹10,000 can't buy a ₹13,800 share: it is listed, and the rest re-balanced.
        assert_eq!(small.too_dear, vec![("B".to_string(), 13_800.0)]);
        assert_eq!(small.lines.len(), 2);
        assert!(large.too_dear.is_empty() && large.lines.iter().all(|l| l.shares > 0));
        // At ₹10 lakh the rounding is small: actual weights within a point of the targets.
        assert!(
            large
                .lines
                .iter()
                .all(|l| (l.weight - l.target).abs() < 0.015),
            "{large:?}"
        );
        assert!(large.cash_left < 13_800.0);
        // Mixing cuts the swing below the holdings' average alone.
        assert!(large.vol_annual < large.vol_alone_avg);
    }
}
