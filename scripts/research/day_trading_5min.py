import json, sys
import numpy as np, pandas as pd
I = json.load(open(sys.argv[1] + '/intraday.json')); D = json.load(open(sys.argv[1] + '/daily.json'))
INTRADAY = 0.0006
def bars(sym):
    x = I[sym]; off = x.get('gmtoffset', 19800)
    df = pd.DataFrame({'o': x['o'], 'h': x['h'], 'l': x['l'], 'c': x['c']}, index=pd.to_datetime(np.array(x['t']) + off, unit='s')).dropna()
    df['day'] = df.index.normalize(); df['tod'] = df.index.hour * 60 + df.index.minute
    return df[(df.tod >= 555) & (df.tod < 930)]       # 09:15 to 15:30
def daily_close(sym):
    x = D[sym]; idx = pd.to_datetime(np.array(x['t']) + 19800, unit='s').normalize()
    s = pd.Series(x['c'], index=idx, dtype=float); return s[~s.index.duplicated()]
gap_rows, mom_rows = [], []
for sym in I:
    if sym == '^NSEI': continue
    b = bars(sym); dc = daily_close(sym)
    for day, g in b.groupby('day'):
        if len(g) < 70: continue
        prev = dc[dc.index < day]
        if prev.empty: continue
        pc = prev.iloc[-1]
        first_open = g.o.iloc[0]
        entry = g.c.iloc[0]            # 09:20, after the first 5 minutes
        exit_ = g.c.iloc[-1]           # 15:25 bar close
        gap = np.log(first_open / pc)
        r = np.log(exit_ / entry)
        gap_rows.append((day, sym, gap, r))
        # Intraday momentum: first 30 min (09:15-09:45) vs last 30 min (15:00-15:25)
        f30 = np.log(g[g.tod < 585].c.iloc[-1] / g.o.iloc[0])
        last = g[g.tod >= 900]
        if len(last) >= 2:
            l30 = np.log(last.c.iloc[-1] / last.o.iloc[0])
            mom_rows.append((day, sym, f30, l30))
G = pd.DataFrame(gap_rows, columns=['day', 'sym', 'gap', 'r'])
print(f"5-minute data: {G.day.nunique()} days x {G.sym.nunique()} stocks ({G.day.min().date()} to {G.day.max().date()})")
for name, cond, side in [('buy at 09:20 after a gap down >1%', G.gap < -0.01, 1), ('short at 09:20 after a gap up >1%', G.gap > 0.01, -1)]:
    per_day = G[cond].assign(x=lambda f: side * f.r).groupby('day').x.mean() - INTRADAY
    t = per_day.mean() / (per_day.std(ddof=1) / np.sqrt(len(per_day))) if len(per_day) > 2 else float('nan')
    print(f"  {name:36} after costs {per_day.mean()*100:+.3f}%/day (t {t:+.1f}), winning days {np.mean(per_day>0):.0%}, {len(per_day)} days, {cond.sum()} trades")
M = pd.DataFrame(mom_rows, columns=['day', 'sym', 'f30', 'l30'])
per_day = M.assign(x=np.sign(M.f30) * M.l30).groupby('day').x.mean() - INTRADAY
t = per_day.mean() / (per_day.std(ddof=1) / np.sqrt(len(per_day)))
print(f"  intraday momentum (last 30 min in the direction of the first 30) after costs {per_day.mean()*100:+.3f}%/day (t {t:+.1f})")
# Daily-open check: Yahoo's daily open vs the first 5-minute bar's open
diffs = []
for sym in I:
    if sym == '^NSEI': continue
    x = D[sym]; idx = pd.to_datetime(np.array(x['t']) + 19800, unit='s').normalize()
    dopen = pd.Series(x['o'], index=idx, dtype=float)
    for day, g in bars(sym).groupby('day'):
        if day in dopen.index and not np.isnan(dopen[day]):
            diffs.append(abs(np.log(dopen[day] / g.o.iloc[0])))
print(f"  daily 'open' vs first 5-min open: median gap {np.median(diffs)*100:.3f}%, 90th pct {np.percentile(diffs,90)*100:.3f}%")
