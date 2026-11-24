//! Small numerical kernels: moments, quantiles, OLS with standard errors, Nelder–Mead.
//! ponytail: hand-written because each is ~30 lines and the problem sizes are tiny
//! (≤ 30 regressors, 4 GARCH parameters). Reach for nalgebra/argmin if that changes.

pub fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

/// Sample variance (n − 1 denominator).
pub fn variance(x: &[f64]) -> f64 {
    let m = mean(x);
    x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (x.len() as f64 - 1.0)
}

/// Quantile with linear interpolation between order statistics (numpy's default, "type 7").
pub fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

pub fn sorted(x: &[f64]) -> Vec<f64> {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

#[derive(Debug, Clone)]
pub struct Ols {
    pub coef: Vec<f64>,
    pub se: Vec<f64>,
    pub residuals: Vec<f64>,
    pub r_squared: f64,
}

/// Ordinary least squares, `rows` are regressor vectors (include a 1.0 for the intercept).
/// Classical (homoskedastic) standard errors, as in statsmodels' default `OLS().fit()`.
pub fn ols(rows: &[Vec<f64>], y: &[f64]) -> Option<Ols> {
    let k = rows.first()?.len();
    let n = rows.len();
    if n <= k {
        return None;
    }
    let mut xtx = vec![vec![0.0; k]; k];
    let mut xty = vec![0.0; k];
    for (row, yi) in rows.iter().zip(y) {
        for i in 0..k {
            xty[i] += row[i] * yi;
            for j in 0..k {
                xtx[i][j] += row[i] * row[j];
            }
        }
    }
    let inv = invert(xtx)?;
    let coef = (0..k)
        .map(|i| (0..k).map(|j| inv[i][j] * xty[j]).sum::<f64>())
        .collect::<Vec<_>>();
    let residuals = rows
        .iter()
        .zip(y)
        .map(|(row, yi)| yi - row.iter().zip(&coef).map(|(a, b)| a * b).sum::<f64>())
        .collect::<Vec<_>>();
    let rss = residuals.iter().map(|e| e * e).sum::<f64>();
    let s2 = rss / (n - k) as f64;
    let se = (0..k).map(|i| (s2 * inv[i][i]).sqrt()).collect();
    let ym = mean(y);
    let tss = y.iter().map(|v| (v - ym).powi(2)).sum::<f64>();
    Some(Ols {
        coef,
        se,
        residuals,
        r_squared: if tss > 0.0 { 1.0 - rss / tss } else { 0.0 },
    })
}

/// Gauss–Jordan inverse with partial pivoting; None when (numerically) singular.
fn invert(mut a: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut inv = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| f64::from(u8::from(i == j)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for col in 0..n {
        let pivot = (col..n).max_by(|&r, &s| a[r][col].abs().total_cmp(&a[s][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        inv.swap(col, pivot);
        let p = a[col][col];
        for j in 0..n {
            a[col][j] /= p;
            inv[col][j] /= p;
        }
        for r in 0..n {
            if r != col {
                let f = a[r][col];
                if f != 0.0 {
                    for j in 0..n {
                        a[r][j] -= f * a[col][j];
                        inv[r][j] -= f * inv[col][j];
                    }
                }
            }
        }
    }
    Some(inv)
}

/// Nelder–Mead simplex minimiser (Nelder & Mead 1965, standard coefficients).
/// Returns (argmin, min, converged).
pub fn nelder_mead(
    f: impl Fn(&[f64]) -> f64,
    start: &[f64],
    step: f64,
    tol: f64,
    max_iter: usize,
) -> (Vec<f64>, f64, bool) {
    let n = start.len();
    let mut simplex = vec![start.to_vec()];
    for i in 0..n {
        let mut p = start.to_vec();
        p[i] += step;
        simplex.push(p);
    }
    let mut values = simplex.iter().map(|p| f(p)).collect::<Vec<_>>();
    for _ in 0..max_iter {
        let mut order = (0..=n).collect::<Vec<_>>();
        order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        simplex = order.iter().map(|&i| simplex[i].clone()).collect();
        values = order.iter().map(|&i| values[i]).collect();
        if (values[n] - values[0]).abs() <= tol * (values[0].abs() + tol) {
            return (simplex[0].clone(), values[0], true);
        }
        let centroid = (0..n)
            .map(|j| simplex[..n].iter().map(|p| p[j]).sum::<f64>() / n as f64)
            .collect::<Vec<_>>();
        let along = |t: f64| {
            centroid
                .iter()
                .zip(&simplex[n])
                .map(|(c, w)| c + t * (w - c))
                .collect::<Vec<_>>()
        };
        let reflected = along(-1.0);
        let fr = f(&reflected);
        if fr < values[0] {
            let expanded = along(-2.0);
            let fe = f(&expanded);
            (simplex[n], values[n]) = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < values[n - 1] {
            (simplex[n], values[n]) = (reflected, fr);
        } else {
            let contracted = if fr < values[n] {
                along(-0.5)
            } else {
                along(0.5)
            };
            let fc = f(&contracted);
            if fc < values[n].min(fr) {
                (simplex[n], values[n]) = (contracted, fc);
            } else {
                for i in 1..=n {
                    simplex[i] = simplex[0]
                        .iter()
                        .zip(&simplex[i])
                        .map(|(b, p)| b + 0.5 * (p - b))
                        .collect();
                    values[i] = f(&simplex[i]);
                }
            }
        }
    }
    let best = (0..=n)
        .min_by(|&a, &b| values[a].total_cmp(&values[b]))
        .unwrap_or(0);
    (simplex[best].clone(), values[best], false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_matches_numpy_linear() {
        let s = sorted(&[4.0, 1.0, 3.0, 2.0]);
        assert_eq!(quantile_sorted(&s, 0.0), 1.0);
        assert!((quantile_sorted(&s, 0.25) - 1.75).abs() < 1e-12);
        assert!((quantile_sorted(&s, 0.5) - 2.5).abs() < 1e-12);
        assert_eq!(quantile_sorted(&s, 1.0), 4.0);
    }

    #[test]
    fn ols_recovers_exact_line_and_standard_errors() {
        // y = 2 + 3x with known residual pattern.
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        let noise = [0.1, -0.1, 0.0, 0.1, -0.1];
        let rows = xs.iter().map(|x| vec![1.0, *x]).collect::<Vec<_>>();
        let y = xs
            .iter()
            .zip(noise)
            .map(|(x, e)| 2.0 + 3.0 * x + e)
            .collect::<Vec<_>>();
        let fit = ols(&rows, &y).unwrap();
        // statsmodels OLS on the same data (reference.json "ols_small").
        assert!(
            (fit.coef[0] - 2.06).abs() < 1e-9 && (fit.coef[1] - 2.98).abs() < 1e-9,
            "{:?}",
            fit.coef
        );
        assert!(
            (fit.se[0] - 0.114_891_252_930_760_6).abs() < 1e-9,
            "{:?}",
            fit.se
        );
        assert!(
            (fit.se[1] - 0.034_641_016_151_377_57).abs() < 1e-9,
            "{:?}",
            fit.se
        );
    }

    #[test]
    fn nelder_mead_finds_rosenbrock_minimum() {
        let rosen = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let (x, fx, ok) = nelder_mead(rosen, &[-1.2, 1.0], 0.5, 1e-14, 10_000);
        assert!(
            ok && fx < 1e-8 && (x[0] - 1.0).abs() < 1e-3 && (x[1] - 1.0).abs() < 1e-3,
            "{x:?} {fx}"
        );
    }
}
