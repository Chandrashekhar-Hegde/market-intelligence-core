"""Phase 0.11 research: can maths find money in the morning-to-close, 3-day or day-trade windows?

The owner's questions (28 Nov 2026): "If I buy at the open, what do I have at the close?", the
same over 3 days, and day trading, using hard maths rather than chart reading. Each idea comes
from a paper, is tested on 10 years of NSE's own daily prices (bhavcopy, fetch_bhavcopy.py) for
today's NIFTY 50, walk forward, after Indian costs, and graded against how many things were
tried (deflated Sharpe, Bailey & López de Prado 2014).

Signals, all known at 09:15 IST on the trading day:
  gap      today's opening gap ln(open/prev close)                 (Lou, Polk & Skouras 2019)
  gap_rel  the gap beyond the market's (NIFTYBEES) gap
  r1       yesterday's close-to-close return                        (Lehmann 1990; Jegadeesh 1990)
  sscore   Ornstein-Uhlenbeck s-score of the 60-day residual vs the market (Avellaneda & Lee 2010)
  volr     EWMA volatility / its 1-year level                       (Moreira & Muir 2017)
  range1   yesterday's high-low range, log
  us       the S&P 500's last session (closes 01:30-02:30 IST)
  storm    probability of the stormy state of a 2-state Gaussian HMM on NIFTY (Hamilton 1989;
           Baum-Welch, the Renaissance lineage), filtered with data up to yesterday only
Targets: open->close (intraday cost 0.11% round trip) and open->close 3 days later (delivery 0.25%).

Method: (1) Fama-MacBeth: each day a cross-sectional regression of the target on each
standardised signal; the mean slope and its Newey-West t. (2) A walk-forward ridge model of all
signals, refit each January on everything before; each morning buy the 5 stocks with the highest
forecast if it beats the cost (and, intraday only, sell short the 5 lowest), against simply buying
all 50 at the open. (3) Deflated Sharpe for the number of variants tried.

Usage: .refenv/bin/python scripts/research/quant_study.py <dir with bhav.csv and gspc.json>
"""
import json, math, sys

import warnings

import numpy as np
import pandas as pd
from scipy import stats

warnings.filterwarnings("ignore")
D = sys.argv[1]
COST_INTRA, COST_DELIV = 0.0011, 0.0025
ALIAS = {"ZOMATO": "ETERNAL"}

b = pd.read_csv(f"{D}/bhav.csv", header=None, names=["date", "sym", "o", "h", "l", "c", "pc", "vol"])
b["sym"] = b["sym"].replace(ALIAS)
b["date"] = pd.to_datetime(b["date"])
b = b.drop_duplicates(["date", "sym"]).sort_values(["sym", "date"])
for col in "ohlc":
    b[col] = pd.to_numeric(b[col], errors="coerce")
b["pc"] = pd.to_numeric(b["pc"], errors="coerce")
b = b.dropna(subset=["o", "c", "pc"])
b = b[(b.o > 0) & (b.c > 0) & (b.pc > 0)]
b["oc"] = np.log(b.c / b.o)
b["gap"] = np.log(b.o / b.pc)
b["cc"] = np.log(b.c / b.pc)
b["rng"] = np.log(b.h / b.l)

wide = lambda col: b.pivot(index="date", columns="sym", values=col)
oc, gap, cc, rng = wide("oc"), wide("gap"), wide("cc"), wide("rng")
mkt = "NIFTYBEES"
stocks = [s for s in oc.columns if s not in (mkt, "GOLDBEES")]
m_cc, m_gap, m_oc = cc[mkt], gap[mkt], oc[mkt]

# --- signals known at the open of day t (shifted so nothing from day t's session leaks) ---------
sig = {}
sig["gap"] = gap[stocks]
sig["gap_rel"] = gap[stocks].sub(m_gap, axis=0)
sig["r1"] = cc[stocks].shift(1)
sig["range1"] = rng[stocks].shift(1)
ew = cc[stocks].pow(2).ewm(alpha=0.06).mean().shift(1)
yr = cc[stocks].pow(2).rolling(250).mean().shift(1)
sig["volr"] = np.sqrt(ew / yr)


def sscores(win=60):
    """Avellaneda-Lee: residual of each stock vs the market over `win` days, cumulated, fitted as
    AR(1) X_{k+1} = a + b X_k + e; s = (X_last - a/(1-b)) / sqrt(var(e)/(1-b^2)), using days up
    to t-1 only. Positive s: stretched up versus the market (expected to come back)."""
    out = pd.DataFrame(index=cc.index, columns=stocks, dtype=float)
    R, M = cc[stocks].values, m_cc.values
    for j, s in enumerate(stocks):
        r = R[:, j]
        for t in range(win + 1, len(r)):
            y, x = r[t - win:t], M[t - win:t]
            ok = ~(np.isnan(y) | np.isnan(x))
            if ok.sum() < win * 0.8:
                continue
            y, x = y[ok], x[ok]
            beta = np.cov(y, x)[0, 1] / max(np.var(x), 1e-12)
            res = y - beta * x
            X = np.cumsum(res)
            bb, aa = np.polyfit(X[:-1], X[1:], 1)
            if not 0 < bb < 1:
                continue
            e = X[1:] - (aa + bb * X[:-1])
            seq = math.sqrt(np.var(e) / (1 - bb * bb))
            if seq > 0:
                out.iat[t, j] = (X[-1] - aa / (1 - bb)) / seq
    return out


print("computing s-scores…", file=sys.stderr)
sig["sscore"] = sscores()

us = json.load(open(f"{D}/gspc.json"))["chart"]["bars"]
us = pd.Series({pd.Timestamp(x["date"]): x["adj_close"] or x["close"] for x in us}).sort_index()
us_ret = np.log(us).diff()
# The last US session that closed before 09:15 IST on day t: the US date strictly before t.
us_on = us_ret.reindex(oc.index.union(us_ret.index)).ffill().shift(1).reindex(oc.index)
sig["us"] = pd.DataFrame({s: us_on for s in stocks})


def hmm_storm(x, iters=50):
    """2-state Gaussian HMM (Hamilton 1989) on returns x: Baum-Welch on all of x, then the filtered
    P(stormy | data up to t). Refit yearly on data before the year (walk-forward)."""
    x = np.asarray(x)
    mu, sd = np.array([x.mean(), x.mean()]), np.array([x.std() * 0.7, x.std() * 1.6])
    A, pi = np.array([[0.98, 0.02], [0.05, 0.95]]), np.array([0.8, 0.2])
    for _ in range(iters):
        B = np.stack([stats.norm.pdf(x, mu[k], sd[k]) for k in (0, 1)], 1) + 1e-300
        al = np.zeros((len(x), 2)); c = np.zeros(len(x))
        al[0] = pi * B[0]; c[0] = al[0].sum(); al[0] /= c[0]
        for t in range(1, len(x)):
            al[t] = (al[t - 1] @ A) * B[t]; c[t] = al[t].sum(); al[t] /= c[t]
        be = np.ones((len(x), 2))
        for t in range(len(x) - 2, -1, -1):
            be[t] = (A @ (B[t + 1] * be[t + 1])) / c[t + 1]
        g = al * be; g /= g.sum(1, keepdims=True)
        xi = np.zeros((2, 2))
        for t in range(len(x) - 1):
            m = np.outer(al[t], B[t + 1] * be[t + 1]) * A
            xi += m / m.sum()
        A = xi / xi.sum(1, keepdims=True); pi = g[0]
        mu = (g * x[:, None]).sum(0) / g.sum(0)
        sd = np.sqrt((g * (x[:, None] - mu) ** 2).sum(0) / g.sum(0))
    storm = int(np.argmax(sd))
    return mu, sd, A, storm


def hmm_filter(x, mu, sd, A, storm):
    p = np.array([0.5, 0.5]); out = []
    for v in x:
        out.append(p[storm])  # before seeing day t's return: known at the open
        b_ = np.array([stats.norm.pdf(v, mu[k], sd[k]) for k in (0, 1)]) + 1e-300
        p = (p @ A) * b_ if not np.isnan(v) else p @ A
        p = p / p.sum()
    return np.array(out)


print("fitting the HMM walk-forward…", file=sys.stderr)
mret = m_cc.dropna()
storm = pd.Series(np.nan, index=mret.index)
for year in sorted(set(mret.index.year))[2:]:
    train = mret[mret.index.year < year]
    params = hmm_storm(train.values)
    upto = mret[mret.index.year <= year]
    f = hmm_filter(upto.values, *params)
    storm.loc[upto.index[upto.index.year == year]] = f[upto.index.year == year]
sig["storm"] = pd.DataFrame({s: storm.reindex(oc.index) for s in stocks})

# --- targets ---------------------------------------------------------------------------------
y1 = oc[stocks]
y3 = (oc[stocks] + cc[stocks].shift(-1) + cc[stocks].shift(-2))
names = list(sig)


def zs(df):
    return df.sub(df.mean(1), axis=0).div(df.std(1).replace(0, np.nan), axis=0)


def nw_t(series, lag):
    x = series.dropna().values
    n = len(x); m = x.mean(); e = x - m
    v = e @ e / n
    for k in range(1, lag + 1):
        v += 2 * (1 - k / (lag + 1)) * (e[k:] @ e[:-k]) / n
    return m, m / math.sqrt(v / n), n


print("\n1) Fama-MacBeth: does the signal sort tomorrow's winners? (slope per 1 s.d., % ; NW t)")
print(f"{'signal':8} | {'open->close':>22} | {'open->3 days':>22}")
for s_ in names:
    X = zs(sig[s_]) if s_ not in ("us", "storm") else None
    cells = []
    for y, lag in ((y1, 1), (y3, 3)):
        if X is None:
            # Same value for every stock that day: a time-series regression of the average stock.
            avg = y.mean(1)
            df = pd.concat([sig[s_].iloc[:, 0], avg], axis=1).dropna()
            beta = np.polyfit(df.iloc[:, 0] / df.iloc[:, 0].std(), df.iloc[:, 1], 1)[0]
            res = df.iloc[:, 1] - beta * (df.iloc[:, 0] / df.iloc[:, 0].std())
            se = math.sqrt(np.var(res) / len(df)) * math.sqrt(1 + 2 * lag)
            cells.append(f"{beta*100:+.3f}% t {beta/se:+.1f} (TS)")
            continue
        slopes = []
        for d in y.index:
            xv, yv = X.loc[d], y.loc[d]
            ok = xv.notna() & yv.notna()
            if ok.sum() >= 20:
                slopes.append((d, np.polyfit(xv[ok], yv[ok], 1)[0]))
        m, t, n = nw_t(pd.Series(dict(slopes)), lag)
        cells.append(f"{m*100:+.3f}% t {t:+.1f}")
    print(f"{s_:8} | {cells[0]:>22} | {cells[1]:>22}")

# --- 2) walk-forward ridge ----------------------------------------------------------------------
print("\n2) Walk-forward model of all signals (ridge, refit each January), net of costs:")
feat = {s_: zs(sig[s_]) if s_ not in ("us", "storm") else sig[s_] for s_ in names}
long = pd.concat({s_: f.stack(future_stack=True) for s_, f in feat.items()}, axis=1)
long["y1"], long["y3"] = y1.stack(future_stack=True), y3.stack(future_stack=True)
long = long.dropna()
dates = long.index.get_level_values(0)
results = {}
N_TRIED = 0
for target, cost, shorts in (("y1", COST_INTRA, True), ("y3", COST_DELIV, False)):
    daily_long, daily_ls, base = [], [], []
    for year in range(2019, dates.max().year + 1):
        tr = long[dates.year < year]; te = long[dates.year == year]
        if len(te) == 0:
            continue
        Xtr = tr[names].values; ytr = tr[target].values
        lam = 10.0 * len(Xtr) / 1000
        w = np.linalg.solve(Xtr.T @ Xtr + lam * np.eye(len(names)), Xtr.T @ (ytr - ytr.mean()))
        pred = pd.Series(te[names].values @ w + ytr.mean(), index=te.index)
        for d, p in pred.groupby(level=0):
            p = p.droplevel(0).sort_values()
            yv = te.loc[d, target]
            top = p.tail(5); top = top[top > cost]
            gl = (yv[top.index] - cost).mean() if len(top) else 0.0
            daily_long.append((d, gl, len(top)))
            if shorts:
                bot = p.head(5); bot = bot[bot < -cost]
                gs = (-yv[bot.index] - cost).mean() if len(bot) else 0.0
                daily_ls.append((d, (gl + gs) / 2))
            base.append((d, yv.mean() - cost))
    for label, rows_ in (("long top 5", daily_long), ("long-short 5/5", daily_ls), ("buy all 50 at the open", base)):
        if not rows_:
            continue
        N_TRIED += 1
        if label == "long top 5":
            r = pd.Series({d: g for d, g, k in rows_ if k})  # days with a trade
        else:
            r = pd.Series(dict(rows_))
        hold = 1 if target == "y1" else 3
        m, t, n = nw_t(r, hold)
        sharpe = r.mean() / r.std() * math.sqrt(252 / hold) if r.std() > 0 else 0
        results[(target, label)] = (r, sharpe)
        extra = f", traded on {n} days" if label == "long top 5" else ""
        print(f"  {'open->close' if target=='y1' else 'open->3 days':12} {label:24} {m*100:+.3f}% a trade (t {t:+.1f}), "
              f"won {np.mean(r>0):.0%}, Sharpe {sharpe:+.2f}{extra}")

# --- 3) deflated Sharpe -------------------------------------------------------------------------
print("\n3) Deflated Sharpe (chance the best variant's Sharpe is real, given everything tried):")
n_variants = len(names) * 2 + N_TRIED  # every signal x target in (1) plus the strategies in (2)
srs = [s for (_, lab), (_, s) in results.items() if lab != "buy all 50 at the open"]
if not srs:
    sys.exit("  (no walk-forward years yet)")
best_key = max((k for k in results if k[1] != "buy all 50 at the open"), key=lambda k: results[k][1])
r, sr_ann = results[best_key]
hold = 1 if best_key[0] == "y1" else 3
sr = r.mean() / r.std()  # per trade
T = len(r); sk = stats.skew(r); ku = stats.kurtosis(r, fisher=False)
var_sr = np.var([s / math.sqrt(252 / hold) for s in srs]) if len(srs) > 1 else 1 / T
emc = 0.5772156649
sr0 = math.sqrt(max(var_sr, 1e-12)) * ((1 - emc) * stats.norm.ppf(1 - 1 / n_variants) + emc * stats.norm.ppf(1 - 1 / (n_variants * math.e)))
dsr = stats.norm.cdf((sr - sr0) * math.sqrt(T - 1) / math.sqrt(1 - sk * sr + (ku - 1) / 4 * sr * sr))
print(f"  best: {best_key}, Sharpe {sr_ann:+.2f} a year over {T} trades; {n_variants} variants tried; "
      f"deflated Sharpe probability {dsr:.2f} (real edge needs > 0.95)")

print("\n4) Year by year (net % a trade):")
for key in [k for k in results if k[1] != "buy all 50 at the open"]:
    r = results[key][0]
    by = r.groupby(r.index.year).mean() * 100
    print(f"  {key[0]} {key[1]:16} " + "  ".join(f"{y}:{v:+.2f}" for y, v in by.items()))
