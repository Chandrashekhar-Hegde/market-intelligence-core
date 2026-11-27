import json, sys
import numpy as np, pandas as pd
d = json.load(open(sys.argv[1] + '/daily.json'))
CASH = np.log(1.065) / 252        # FD/T-bill on uninvested money
COST = 0.00125                    # one way, delivery (half the 0.25% round trip)
EPISODES = {'Covid crash': ('2020-02-19', '2020-03-23'), 'Covid 2020 full': ('2020-02-19', '2020-12-31'),
            '2022 H1 sell-off': ('2022-01-17', '2022-06-17'), 'Adani (Jan-Feb 2023)': ('2023-01-24', '2023-02-28'),
            'Election day 2024': ('2024-06-03', '2024-06-04'), 'Sep 2024-Mar 2025 fall': ('2024-09-26', '2025-03-03'),
            'Apr 2025 tariff shock': ('2025-04-01', '2025-04-07')}
def series(sym):
    x = d[sym]; idx = pd.to_datetime(x['t'], unit='s').normalize()
    s = pd.Series(x['adj'], index=idx).dropna(); s = s[~s.index.duplicated()]
    r = np.log(s).diff().dropna()
    return r[r.abs() < np.log(2)]   # drop vendor bad prints
def backtest(r, rebalance=5):
    ewma = (r**2).ewm(alpha=0.06, adjust=False).mean().shift(1)          # forecast for day t from data to t-1
    longrun = (r**2).expanding(min_periods=250).mean().shift(1)          # "normal" from history so far
    target_w = np.minimum(1.0, np.sqrt(longrun / ewma)).fillna(1.0)
    w = target_w.copy()
    w[:] = np.nan; w.iloc[::rebalance] = target_w.iloc[::rebalance]; w = w.ffill().fillna(1.0)
    turnover = w.diff().abs().fillna(0)
    vt = w * r + (1 - w) * CASH - turnover * COST
    bh = r.copy()
    return vt, bh, w
def stats(x):
    yrs = len(x) / 252; cum = x.cumsum()
    dd = (cum - cum.cummax()).min()
    ex = x - CASH
    return dict(cagr=np.expm1(x.sum() / yrs), vol=x.std() * np.sqrt(252), sharpe=ex.mean() / x.std() * np.sqrt(252), maxdd=np.expm1(dd))
def show(name, r):
    vt, bh, w = backtest(r)
    a, b = stats(vt.iloc[250:]), stats(bh.iloc[250:])
    return name, a, b, vt, bh, w
rows = []
for sym in d:
    r = series(sym)
    if len(r) < 1000: continue
    rows.append(show(sym, r))
# NIFTY detail
name, a, b, vt, bh, w = [x for x in rows if x[0] == '^NSEI'][0]
print(f"NIFTY 50 2017-2026  app sizing: CAGR {a['cagr']:.1%} vol {a['vol']:.1%} Sharpe {a['sharpe']:.2f} worst fall {a['maxdd']:.1%} | buy&hold: CAGR {b['cagr']:.1%} vol {b['vol']:.1%} Sharpe {b['sharpe']:.2f} worst fall {b['maxdd']:.1%} | avg exposure {w.iloc[250:].mean():.0%}")
for ep, (s, e) in EPISODES.items():
    m = (vt.index >= s) & (vt.index <= e)
    if m.sum():
        print(f"  {ep:24} app {np.expm1(vt[m].sum()):+.1%}  buy&hold {np.expm1(bh[m].sum()):+.1%}  exposure at start {w[m].iloc[0]:.0%}")
stocks = [x for x in rows if x[0] != '^NSEI']
sharpe_up = sum(a['sharpe'] > b['sharpe'] for _, a, b, *_ in stocks)
dd_less = sum(a['maxdd'] > b['maxdd'] for _, a, b, *_ in stocks)
both = sum(a['sharpe'] > b['sharpe'] and a['maxdd'] > b['maxdd'] for _, a, b, *_ in stocks)
beat_fd = sum(a['cagr'] > 0.065 for _, a, b, *_ in stocks)
more_money = sum(a['cagr'] > b['cagr'] for _, a, b, *_ in stocks)
print(f"NIFTY 50 stocks ({len(stocks)}): better Sharpe {sharpe_up}, smaller worst fall {dd_less}, both {both}, beat 6.5% FD {beat_fd}, more total money than buy&hold {more_money}")
print(f"  median worst fall: app {np.median([a['maxdd'] for _, a, b, *_ in stocks]):.1%} vs buy&hold {np.median([b['maxdd'] for _, a, b, *_ in stocks]):.1%}; median CAGR app {np.median([a['cagr'] for _, a, b, *_ in stocks]):.1%} vs {np.median([b['cagr'] for _, a, b, *_ in stocks]):.1%}; median Sharpe {np.median([a['sharpe'] for _, a, b, *_ in stocks]):.2f} vs {np.median([b['sharpe'] for _, a, b, *_ in stocks]):.2f}")
for sym in ['ADANIENT.NS', 'HDFCBANK.NS', 'RELIANCE.NS', 'INFY.NS']:
    name, a, b, vt, bh, w = [x for x in rows if x[0] == sym][0]
    s, e = EPISODES['Covid crash']
    m = (vt.index >= s) & (vt.index <= e)
    extra = ''
    if sym == 'ADANIENT.NS':
        s2, e2 = EPISODES['Adani (Jan-Feb 2023)']; m2 = (vt.index >= s2) & (vt.index <= e2)
        extra = f" | Adani fall: app {np.expm1(vt[m2].sum()):+.1%} vs {np.expm1(bh[m2].sum()):+.1%}"
    print(f"  {sym:12} Sharpe {a['sharpe']:.2f} vs {b['sharpe']:.2f}, worst fall {a['maxdd']:.0%} vs {b['maxdd']:.0%}, CAGR {a['cagr']:.1%} vs {b['cagr']:.1%}; Covid crash app {np.expm1(vt[m].sum()):+.1%} vs {np.expm1(bh[m].sum()):+.1%}{extra}")
