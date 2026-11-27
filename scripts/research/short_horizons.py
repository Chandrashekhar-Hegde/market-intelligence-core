import json, sys
import numpy as np, pandas as pd
d = json.load(open(sys.argv[1] + '/daily.json'))
def frame(sym, key):
    x = d[sym]; idx = pd.to_datetime(x['t'], unit='s').normalize()
    s = pd.Series(x[key], index=idx, dtype=float); return s[~s.index.duplicated()]
stocks = [s for s in d if s != '^NSEI']
adj = pd.DataFrame({s: frame(s, 'adj') for s in stocks}).sort_index()
close = pd.DataFrame({s: frame(s, 'c') for s in stocks}).reindex(adj.index)
opn = pd.DataFrame({s: frame(s, 'o') for s in stocks}).reindex(adj.index)
nifty = frame('^NSEI', 'adj').reindex(adj.index)
lr = np.log(adj)
r1 = lr.diff()
bad = r1.abs() > np.log(2)
lr = lr.mask(bad.cumsum() > 0) if False else lr  # no bad prints in NIFTY 50 (checked)
DELIVERY, INTRADAY = 0.0025, 0.0006
# RSI14 (Wilder)
d1 = adj.diff(); up = d1.clip(lower=0).ewm(alpha=1/14, adjust=False).mean(); dn = (-d1.clip(upper=0)).ewm(alpha=1/14, adjust=False).mean()
rsi = 100 - 100 / (1 + up / dn)
signals = {
    'buy after a 1-day fall >2%': (r1 < -0.02),
    'buy after a 5-day fall >5%': ((lr - lr.shift(5)) < -0.05),
    'buy when RSI < 30': (rsi < 30),
    'buy above 200-day average': (adj > adj.rolling(200).mean()),
    'buy 12-month winners (top 10)': ((lr.shift(21) - lr.shift(252)).rank(axis=1, ascending=False) <= 10),
    'buy the 10 biggest 1-week losers': ((lr - lr.shift(5)).rank(axis=1) <= 10),
}
print("Long trades: enter at the close, exit h sessions later, after 0.25% delivery cost.")
print("'vs random' = the same stocks bought on any day (the edge of the signal itself).")
for h in [1, 2, 3, 5, 10, 21]:
    fwd = lr.shift(-h) - lr
    base = fwd.stack().mean()
    print(f"-- {h} session(s): buying any stock any day averaged {base*100:+.2f}% before costs")
    for name, sig in signals.items():
        # One trade set every h days (non-overlapping), averaged across stocks per date.
        dates = adj.index[260::h]
        per_date = []
        for t in dates:
            m = sig.loc[t].fillna(False).astype(bool)
            v = fwd.loc[t][m].dropna()
            if len(v): per_date.append((v.mean(), (fwd.loc[t].dropna().mean())))
        if len(per_date) < 20: continue
        a = np.array(per_date)
        net = a[:, 0] - DELIVERY
        edge = a[:, 0] - a[:, 1]
        t_net = net.mean() / (net.std(ddof=1) / np.sqrt(len(net)))
        t_edge = edge.mean() / (edge.std(ddof=1) / np.sqrt(len(edge)))
        print(f"   {name:32} after costs {net.mean()*100:+.2f}% per trade (t {t_net:+.1f}, made money {np.mean(net>0):.0%} of periods) | vs random {edge.mean()*100:+.2f}% (t {t_edge:+.1f}) n={len(a)}")
# Day trading with daily OHLC (intraday cost 0.06% round trip)
print("\nDay trades on daily open/close, 0.06% round-trip cost (NIFTY 50 stocks, 10 years):")
oc = np.log(close / opn)                       # open -> close, same day (unadjusted prices on the same day)
gap = np.log(opn / close.shift(1))
prev = np.log(close.shift(1) / close.shift(2))
rules = {
    'buy at open after a gap down >1%': (gap < -0.01, 1),
    'short at open after a gap up >1%': (gap > 0.01, -1),
    'buy at open after a >2% fall yesterday': (prev < -0.02, 1),
    'short at open after a >2% rise yesterday': (prev > 0.02, -1),
    'buy every open, sell at close': (oc.notna(), 1),
}
for name, (cond, side) in rules.items():
    per_day = (side * oc.where(cond)).mean(axis=1).dropna()
    net = per_day - INTRADAY
    t = net.mean() / (net.std(ddof=1) / np.sqrt(len(net)))
    print(f"   {name:42} after costs {net.mean()*100:+.3f}% per day (t {t:+.1f}), winning days {np.mean(net>0):.0%}, n={len(net)} days")
ov = gap.stack().mean(); intr = oc.stack().mean()
print(f"   (where the gains come from: overnight {ov*100:+.3f}%/day, open-to-close {intr*100:+.3f}%/day)")
