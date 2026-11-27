import json, sys, re
import numpy as np, pandas as pd
from scipy import stats
raw = json.load(open(sys.argv[1]))
src = open('crates/gm-integrations/src/universe.rs').read()
block = src[src.index('NIFTY50'):src.index('OTHER_ASSETS')]
names = re.findall(r'\("([A-Z0-9&\-]+\.NS)",', block)
px = pd.DataFrame({s: pd.Series({pd.Timestamp(int(t), unit='s').normalize(): v for t, v in raw[s].items()}) for s in names + ['^NSEI'] if s in raw}).sort_index()
px = px[~px.index.duplicated()].ffill(limit=3)
nifty = px.pop('^NSEI')
r = np.log(px).diff()
COST = 0.0025
print(f"{px.shape[1]} stocks, {px.index[0].date()} to {px.index[-1].date()}")
H = 21
dates = px.index[260::H]  # monthly rebalances after a year of history
def portfolio_test(score_fn, name, top=10, higher_better=True):
    rows = []
    for i, d in enumerate(dates[:-1]):
        nxt = dates[i+1]
        sc = score_fn(d).dropna()
        if len(sc) < 30: continue
        fwd = (np.log(px.loc[nxt]) - np.log(px.loc[d]))[sc.index].dropna()
        sc = sc[fwd.index]
        ranked = sc.sort_values(ascending=not higher_better)
        best = fwd[ranked.index[:top]].mean(); worst = fwd[ranked.index[-top:]].mean(); allm = fwd.mean()
        rows.append((d, best - allm, best - worst, best, allm))
    df = pd.DataFrame(rows, columns=['d', 'top_minus_all', 'top_minus_bottom', 'top', 'all']).set_index('d')
    for col in ['top_minus_all', 'top_minus_bottom']:
        x = df[col]
        t = x.mean() / (x.std(ddof=1) / np.sqrt(len(x)))
        half = len(x)//2
        print(f"  {name:28} {col:17} mean/month {x.mean()*100:+.2f}%  t={t:+.2f}  p={2*stats.t.sf(abs(t), len(x)-1):.3f}  hit={np.mean(x>0):.0%}  halves {x[:half].mean()*100:+.2f}% / {x[half:].mean()*100:+.2f}%  n={len(x)}")
    return df
lr = np.log(px)
print("Cross-sectional (monthly, top 10 of the current NIFTY 50; survivorship-biased universe):")
portfolio_test(lambda d: lr.shift(21).loc[d] - lr.shift(252).loc[d], "momentum 12-1")
portfolio_test(lambda d: -r.loc[:d].iloc[-252:].std(), "low volatility (1y)")
portfolio_test(lambda d: -(lr.loc[d] - lr.shift(5).loc[d]), "1-week reversal")
portfolio_test(lambda d: -(lr.loc[d] - lr.shift(21).loc[d]), "1-month reversal")
portfolio_test(lambda d: lr.loc[d] - lr.rolling(252).max().loc[d], "near 52-week high")
# pooled time-series 200d trend: per stock, position = price > 200d MA; next-21d excess return over NIFTY
print("Pooled time-series rule across all stocks (monthly, non-overlapping):")
ma = px.rolling(200).mean()
nl = np.log(nifty)
res = []
for i, d in enumerate(dates[:-1]):
    nxt = dates[i+1]
    pos = (px.loc[d] > ma.loc[d]).astype(float)*2-1
    fwd = (lr.loc[nxt]-lr.loc[d]) - (nl.loc[nxt]-nl.loc[d])
    ok = pos.notna() & fwd.notna() & ma.loc[d].notna()
    res.append(((pos[ok]*fwd[ok]).mean()))
x = pd.Series(res); t = x.mean()/(x.std(ddof=1)/np.sqrt(len(x)))
print(f"  trend 200d (vs NIFTY)       mean/month {x.mean()*100:+.2f}%  t={t:+.2f}  p={2*stats.t.sf(abs(t), len(x)-1):.3f}")
# what the drift says: share of months NIFTY rose
m = nl.loc[dates].diff().dropna()
print(f"NIFTY: rose in {np.mean(m>0):.0%} of {len(m)} months; mean {m.mean()*100:+.2f}%/month")
# today's ranks for the momentum and low-vol
d = px.index[-1]
mom = (lr.shift(21).loc[d] - lr.shift(252).loc[d]).sort_values(ascending=False)
lv = r.iloc[-252:].std().sort_values()
print("Today top momentum:", ', '.join(f"{s[:-3]} {v*100:+.0f}%" for s, v in mom.head(8).items()))
print("Today lowest vol:", ', '.join(f"{s[:-3]} {v*np.sqrt(252)*100:.0f}%" for s, v in lv.head(8).items()))
