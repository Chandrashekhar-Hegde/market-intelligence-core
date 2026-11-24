"""Reference outputs for the Rust analysis engine, computed with established libraries.

    uv venv .refenv && VIRTUAL_ENV=.refenv uv pip install numpy scipy statsmodels arch pandas
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


def main():
    ref = {name: reference(name) for name in ("aapl", "gspc", "nsei")}
    ref["beta_aapl_gspc"] = beta("aapl", "gspc")
    xs = np.array([1.0, 2, 3, 4, 5])
    y = 2 + 3 * xs + np.array([0.1, -0.1, 0, 0.1, -0.1])
    small = OLS(y, np.column_stack([np.ones(5), xs])).fit()
    ref["ols_small"] = {"coef": small.params.tolist(), "se": small.bse.tolist()}
    (FIXTURES / "reference.json").write_text(json.dumps(ref, indent=1, default=float) + "\n")
    print(json.dumps(ref["aapl"]["garch"]), json.dumps(ref["ols_small"]))


if __name__ == "__main__":
    main()
