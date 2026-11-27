import json, sys
import numpy as np, pandas as pd
I = json.load(open(sys.argv[1] + '/intraday.json')); D = json.load(open(sys.argv[1] + '/daily.json'))
CHECKS = [615, 675, 735, 795, 855, 900]   # 10:15, 11:15, 12:15, 13:15, 14:15, 15:00
OPEN, CLOSE = 555, 930
def bars(sym):
    x = I[sym]; off = x.get('gmtoffset', 19800)
    df = pd.DataFrame({'o': x['o'], 'c': x['c']}, index=pd.to_datetime(np.array(x['t']) + off, unit='s')).dropna()
    df['day'] = df.index.normalize(); df['tod'] = df.index.hour * 60 + df.index.minute
    return df[(df.tod >= OPEN) & (df.tod < CLOSE)]
def daily(sym):
    x = D[sym]; idx = pd.to_datetime(np.array(x['t']) + 19800, unit='s').normalize()
    c = pd.Series(x['c'], index=idx, dtype=float); o = pd.Series(x['o'], index=idx, dtype=float)
    return c[~c.index.duplicated()], o[~o.index.duplicated()]
res = {k: [] for k in ['A app (daily quantiles x sqrt(left))', 'B open-to-close vol x sqrt(left)', 'C same-time quantiles x today vol ratio']}
for sym in I:
    b = bars(sym); dc, do = daily(sym)
    days = sorted(b.day.unique())
    # per day: price at each checkpoint, close, and realised 5-min variance up to each checkpoint
    info = {}
    for day, g in b.groupby('day'):
        if len(g) < 70: continue
        close = g.c.iloc[-1]; r5 = np.log(g.c).diff().dropna()
        pts = {}
        for t in CHECKS:
            upto = g[g.tod < t]
            if len(upto) < 3: continue
            pts[t] = (upto.c.iloc[-1], (r5[g.tod.iloc[1:] < t] ** 2).sum())
        info[day] = (close, pts)
    ds = sorted(info)
    for k, day in enumerate(ds):
        if k < 20: continue                         # warm-up: 20 past days of intraday history
        past = ds[k - 20:k]
        hist_c = dc[dc.index < day].iloc[-500:]
        r1 = np.log(hist_c).diff().dropna()
        q5, q95 = np.quantile(r1, [0.05, 0.95])
        oc = np.log(dc / do)[(dc.index < day)].dropna().iloc[-250:]
        sd_oc = oc.std()
        close, pts = info[day]
        for t, (price, rv_today) in pts.items():
            left = (CLOSE - t) / (CLOSE - OPEN)
            y = np.log(close / price)
            # A: the app today
            lo, hi = q5 * np.sqrt(left), q95 * np.sqrt(left)
            res['A app (daily quantiles x sqrt(left))'].append((lo <= y <= hi, hi - lo, t))
            # B: open-to-close vol, sqrt(time left), normal 90%
            w = 1.645 * sd_oc * np.sqrt(left)
            res['B open-to-close vol x sqrt(left)'].append((-w <= y <= w, 2 * w, t))
            # C: empirical same-time-to-close moves on past days, scaled by today's vol so far vs theirs
            moves, rvs = [], []
            for p in past:
                if t in info[p][1]:
                    pc, prv = info[p][1][t]; moves.append(np.log(info[p][0] / pc)); rvs.append(prv)
            if len(moves) >= 15 and np.mean(rvs) > 0:
                ratio = np.sqrt(max(rv_today, 1e-12) / np.mean(rvs)); ratio = np.clip(ratio, 0.5, 2.5)
                lo, hi = np.quantile(moves, [0.05, 0.95]) * ratio
                # widen for small samples: 20 days give coarse tails
                res['C same-time quantiles x today vol ratio'].append((lo <= y <= hi, hi - lo, t))
print("Target: 90 in 100 closes inside the range. Width = average range width (smaller is more useful).")
for k, v in res.items():
    a = np.array([(x[0], x[1]) for x in v], dtype=float)
    by_t = {t: np.mean([x[0] for x in v if x[2] == t]) for t in CHECKS}
    print(f"{k:42} inside {a[:,0].mean():.1%}  avg width {a[:,1].mean()*100:.2f}%  n={len(a)}  by time: " + ", ".join(f"{t//60}:{t%60:02d} {p:.0%}" for t, p in by_t.items()))

# ---- D: open-to-close vol x sqrt(share of the day's variance still to come), self-calibrated ----
# Intraday variance profile from 5-minute bars: share of a day's open-to-close variance in each slot.
slot_var = {}
for sym in I:
    b = bars(sym)
    r = np.log(b.c).diff()
    r[b.day != b.day.shift()] = np.nan
    for tod, v in (r ** 2).groupby(b.tod).mean().items():
        slot_var.setdefault(tod, []).append(v)
prof = pd.Series({k: np.nanmean(v) for k, v in slot_var.items()}).sort_index()
prof = prof / prof.sum()
left_var = lambda t: prof[prof.index >= t].sum()
print("share of the day's variance still to come: " + ", ".join(f"{t//60}:{t%60:02d} {left_var(t):.0%} (time left {(CLOSE-t)/(CLOSE-OPEN):.0%})" for t in CHECKS))
rows = []
for sym in I:
    b = bars(sym); dc, do = daily(sym)
    for day, g in b.groupby('day'):
        if len(g) < 70: continue
        oc = np.log(dc / do)[(dc.index < day)].dropna().iloc[-250:]
        sd = oc.std(); close = g.c.iloc[-1]
        for t in CHECKS:
            upto = g[g.tod < t]
            if len(upto) < 3: continue
            z = np.log(close / upto.c.iloc[-1]) / (sd * np.sqrt(left_var(t)))
            rows.append((day, t, z))
Z = pd.DataFrame(rows, columns=['day', 'tod', 'z']).sort_values('day')
days = sorted(Z.day.unique()); hits = []; widths = []
for k, day in enumerate(days):
    if k < 20: continue
    past = Z[Z.day.isin(days[:k])]
    lo, hi = np.quantile(past.z, [0.05, 0.95])        # calibrated on past days only
    today = Z[Z.day == day]
    hits += list((today.z >= lo) & (today.z <= hi)); widths.append(hi - lo)
print(f"D open-to-close vol x sqrt(variance left), calibrated   inside {np.mean(hits):.1%}  (z band {np.mean(widths):.2f} sd vs 3.29 for a normal 90%)")
byt = Z[Z.day.isin(days[20:])]
lo, hi = np.quantile(Z[Z.day.isin(days[:20])].z, [0.05, 0.95])
print("   by time: " + ", ".join(f"{t//60}:{t%60:02d} {((g.z>=lo)&(g.z<=hi)).mean():.0%}" for t, g in byt.groupby('tod')))

# ---- E: as D, with the band calibrated separately at each time of day (walk-forward) ----
hitsE, widthE, byE = [], [], {}
sd_by = {}
for k, day in enumerate(days):
    if k < 20: continue
    past = Z[Z.day.isin(days[:k])]
    for t, g in Z[Z.day == day].groupby('tod'):
        lo, hi = np.quantile(past[past.tod == t].z, [0.05, 0.95])
        h = list((g.z >= lo) & (g.z <= hi)); hitsE += h; byE.setdefault(t, []).extend(h); widthE.append(hi - lo)
print(f"E as D, band per time of day                          inside {np.mean(hitsE):.1%}  by time: " + ", ".join(f"{t//60}:{t%60:02d} {np.mean(v):.0%}" for t, v in sorted(byE.items())))
full = Z.groupby('tod').z.quantile([0.05, 0.95]).unstack()
print("fitted bands (all 60 days, z units): " + ", ".join(f"{t//60}:{t%60:02d} [{r[0.05]:.2f}, {r[0.95]:.2f}]" for t, r in full.iterrows()))
print("variance profile by half hour (share of open-to-close variance): " + ", ".join(f"{h//60}:{h%60:02d} {prof[(prof.index>=h)&(prof.index<h+30)].sum():.0%}" for h in range(555, 930, 30)))

# ---- Final constants for the app: share of variance left and the symmetric 90% |z| at each half hour ----
rows = []
for sym in I:
    b = bars(sym); dc, do = daily(sym)
    for day, g in b.groupby('day'):
        if len(g) < 70: continue
        oc = np.log(dc / do)[(dc.index < day)].dropna().iloc[-250:]
        sd = oc.std(); close = g.c.iloc[-1]
        for t in range(585, 930, 30):
            upto = g[g.tod < t]
            if len(upto) < 3: continue
            lv = left_var(t)
            rows.append((t, lv, np.log(close / upto.c.iloc[-1]) / (sd * np.sqrt(lv))))
F = pd.DataFrame(rows, columns=['tod', 'lv', 'z'])
out = F.groupby('tod').agg(lv=('lv', 'first'), z90=('z', lambda z: np.quantile(np.abs(z), 0.90)), n=('z', 'size'))
print("FINAL (minutes since 09:15, share of variance left, |z| 90%):")
for t, r in out.iterrows():
    print(f"  {t-555:4d}  {r.lv:.3f}  {r.z90:.2f}  n={r.n}")
# Out-of-sample check of the symmetric version: fit on the first 30 days, test on the last 30
days = sorted(set(Z.day)); first, last = days[:30], days[30:]
tab = {}
for t in CHECKS:
    zz = Z[(Z.tod == t) & Z.day.isin(first)].z
    tab[t] = np.quantile(np.abs(zz), 0.90)
test = Z[Z.day.isin(last)]
inside = [abs(r.z) <= tab[r.tod] for r in test.itertuples()]
print(f"symmetric band fitted on days 1-30, tested on days 31-60: inside {np.mean(inside):.1%}")
