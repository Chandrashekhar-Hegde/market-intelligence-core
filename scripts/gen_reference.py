"""Reference outputs for the Rust analysis engine, computed with established libraries.

    uv venv .refenv && VIRTUAL_ENV=.refenv uv pip install numpy scipy statsmodels arch pandas scikit-learn
    .refenv/bin/python scripts/gen_reference.py

Reads crates/gm-domain/tests/fixtures/*_5y.csv (real adjusted closes) and writes
crates/gm-domain/tests/fixtures/reference.json. Rust tests (gm-domain analysis::tests) assert
the engine matches within stated tolerances. Re-run only when fixtures or definitions change.
"""

import json
import math
from pathlib import Path

import numpy as np
import pandas as pd
from arch import arch_model
from arch.unitroot import VarianceRatio
from scipy import stats
from statsmodels.regression.linear_model import OLS
from statsmodels.stats.diagnostic import acorr_ljungbox
from statsmodels.tsa.stattools import adfuller

FIXTURES = Path(__file__).resolve().parents[1] / "crates/gm-domain/tests/fixtures"


def load(name):
    df = pd.read_csv(FIXTURES / f"{name}_5y.csv", parse_dates=["date"])
    return df.set_index("date")["adj_close"]


def hill(r):
    losses = np.sort(-r[r < 0])[::-1]
    k = max(10, round(len(r) * 0.05))
    xi = np.mean(np.log(losses[:k] / losses[k]))
    return 1 / xi, k


def t_var_es(level, mu, sigma, nu):
    a = 1 - level
    s = math.sqrt((nu - 2) / nu)
    q = stats.t.ppf(a, nu)
    r_var = mu + sigma * s * q
    # ES by numerical integration of the tail, independent of the closed form used in Rust.
    tail_mean = stats.t.expect(lambda x: x, args=(nu,), ub=q, conditional=True)
    r_es = mu + sigma * s * tail_mean
    return 1 - math.exp(r_var), 1 - math.exp(r_es)


def reference(name):
    prices = load(name)
    logp = np.log(prices.to_numpy())
    r = np.diff(logp)
    n = len(r)
    years = (prices.index[-1] - prices.index[0]).days / 365.25
    ppy = n / years
    out = {"n_returns": n, "periods_per_year": ppy}

    out["mean_daily"] = r.mean()
    out["vol_daily"] = r.std(ddof=1)
    out["skewness"] = stats.skew(r)
    out["excess_kurtosis"] = stats.kurtosis(r)
    jb = stats.jarque_bera(r)
    out["jarque_bera"] = [jb.statistic, jb.pvalue]
    out["hill_tail_index"], out["hill_k"] = hill(r)

    lb = acorr_ljungbox(r, lags=[10])
    lb2 = acorr_ljungbox(r**2, lags=[10])
    out["ljung_box"] = [float(lb["lb_stat"].iloc[0]), float(lb["lb_pvalue"].iloc[0])]
    out["ljung_box_squared"] = [float(lb2["lb_stat"].iloc[0]), float(lb2["lb_pvalue"].iloc[0])]

    out["variance_ratio"] = {}
    for q in (2, 5, 10):
        vr = VarianceRatio(logp, lags=q, trend="c", debiased=True, robust=True, overlap=True)
        out["variance_ratio"][str(q)] = [vr.vr, vr.stat, vr.pvalue]

    lags = int(12 * (len(logp) / 100) ** 0.25)
    adf = adfuller(logp, maxlag=lags, regression="c", autolag=None)
    out["adf"] = {"stat": adf[0], "nobs": adf[3], "crit": [adf[4]["1%"], adf[4]["5%"], adf[4]["10%"]], "lags": lags}

    e = (r - r.mean()) * 100
    fit = arch_model(e, mean="Zero", vol="GARCH", p=1, q=1, dist="t").fit(disp="off")
    out["garch"] = {
        "omega": fit.params["omega"] / 1e4,
        "alpha": fit.params["alpha[1]"],
        "beta": fit.params["beta[1]"],
        "nu": fit.params["nu"],
        # loglik of the unscaled data
        "loglik": fit.loglikelihood + n * math.log(100),
    }

    var = r.var(ddof=1)
    for x in r - r.mean():
        var = 0.94 * var + 0.06 * x * x
    out["ewma_annual"] = math.sqrt(var * ppy)

    losses = 1 - np.exp(r)
    out["historical"] = {}
    for level in (0.95, 0.99):
        v = np.quantile(losses, level)
        out["historical"][str(level)] = [v, losses[losses >= v].mean()]
    kurt = stats.kurtosis(r)
    nu_mm = 4 + 6 / kurt if kurt > 0 else 1e6
    out["student_t"] = {str(l): t_var_es(l, r.mean(), r.std(ddof=1), nu_mm) for l in (0.95, 0.99)}

    p = prices.to_numpy()
    dd = 1 - p / np.maximum.accumulate(p)
    out["max_drawdown"] = float(dd.max())
    out["sharpe"] = r.mean() / r.std(ddof=1) * math.sqrt(ppy)
    return out


def beta(asset, bench):
    a, b = load(asset), load(bench)
    df = pd.concat([a, b], axis=1, join="inner").dropna()
    rets = np.log(df).diff().dropna()
    fit = OLS(rets.iloc[:, 0].to_numpy(), np.column_stack([np.ones(len(rets)), rets.iloc[:, 1].to_numpy()])).fit()
    return {"n_common_days": len(df), "alpha_daily": fit.params[0], "beta": fit.params[1],
            "beta_se": fit.bse[1], "alpha_se_daily": fit.bse[0], "r_squared": fit.rsquared}


def compare_reference(names):
    from sklearn.covariance import ledoit_wolf
    from statsmodels.tsa.stattools import coint

    frames = [load(n).rename(n) for n in names]
    px = pd.concat(frames, axis=1, join="inner").dropna()
    logp = np.log(px)
    r = logp.diff().dropna()
    iso = px.index.isocalendar()
    week_last = logp.groupby([iso.year, iso.week]).tail(1)
    w = week_last.diff().dropna()
    out = {
        "n_common_days": len(px),
        "n_weekly": len(w),
        "pearson_daily": r.corr().to_numpy().tolist(),
        "spearman_daily": r.corr(method="spearman").to_numpy().tolist(),
        "pearson_weekly": w.corr().to_numpy().tolist(),
    }
    shrunk, shrinkage = ledoit_wolf(r.to_numpy())
    out["ledoit_wolf_shrinkage"] = float(shrinkage)
    out["ledoit_wolf_cov"] = shrunk.tolist()
    eig = np.sort(np.linalg.eigvalsh(r.corr().to_numpy()))[::-1]
    out["pca_explained"] = (eig / eig.sum()).tolist()
    lags = int(12 * (len(px) / 100) ** 0.25)
    pairs = {}
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            res = {}
            for dep, reg in ((a, b), (b, a)):
                stat, _, crit = coint(logp[dep], logp[reg], trend="c", maxlag=lags, autolag=None)
                X = np.column_stack([np.ones(len(logp)), logp[reg].to_numpy()])
                beta = np.linalg.lstsq(X, logp[dep].to_numpy(), rcond=None)[0]
                resid = logp[dep].to_numpy() - X @ beta
                ds = np.diff(resid)
                phi = np.linalg.lstsq(np.column_stack([np.ones(len(ds)), resid[:-1]]), ds, rcond=None)[0][1]
                half = -math.log(2) / math.log(1 + phi) if -1 < phi < 0 else None
                res[f"{dep}~{reg}"] = {"stat": stat, "crit": list(crit), "hedge": beta[1], "half_life": half}
            pairs[f"{a}|{b}"] = res
    out["pairs"] = pairs
    years = (px.index[-1] - px.index[0]).days / 365.25
    out["performance"] = {
        n: {
            "total_return": px[n].iloc[-1] / px[n].iloc[0] - 1,
            "annual_return": (px[n].iloc[-1] / px[n].iloc[0]) ** (1 / years) - 1,
            "sharpe": r[n].mean() / r[n].std(ddof=1) * math.sqrt(len(r) / years),
        }
        for n in names
    }
    return out


def main():
    ref = {name: reference(name) for name in ("aapl", "gspc", "nsei")}
    ref["beta_aapl_gspc"] = beta("aapl", "gspc")
    ref["compare_aapl_gspc_nsei"] = compare_reference(["aapl", "gspc", "nsei"])
    xs = np.array([1.0, 2, 3, 4, 5])
    y = 2 + 3 * xs + np.array([0.1, -0.1, 0, 0.1, -0.1])
    small = OLS(y, np.column_stack([np.ones(5), xs])).fit()
    ref["ols_small"] = {"coef": small.params.tolist(), "se": small.bse.tolist()}
    (FIXTURES / "reference.json").write_text(json.dumps(ref, indent=1, default=float) + "\n")
    print(json.dumps(ref["aapl"]["garch"]), json.dumps(ref["ols_small"]))


if __name__ == "__main__":
    main()
