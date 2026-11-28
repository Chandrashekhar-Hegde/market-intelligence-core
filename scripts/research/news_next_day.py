"""Does a news reading predict the NEXT day? (Lopez-Lira & Tang 2023, on our NSE data; 28 Nov 2026)

Headlines from the 3-year backfill (Google News, dated by day), each read by the built-in reader
and, where the free quota allowed, by Gemini (gemini-3.1-flash-lite). A headline dated day d is
only acted on at the open of the next trading day after d, so nothing published during the
trading window leaks in. Returns from NSE's own prices (bhavcopy): next day open->close and
open->close two days later, minus the average NIFTY 50 stock over the same window (what the market did anyway).
Trade the direction the reader gave (materiality 2 or 3), after Indian costs.

Usage: .refenv/bin/python scripts/research/news_next_day.py <backfill dir> <bhav dir>
"""
import json, math, sys, warnings

import numpy as np
import pandas as pd

warnings.filterwarnings("ignore")
BF, BH = sys.argv[1], sys.argv[2]
COST = {"next day": 0.0011, "3 days": 0.0025}

gem = {}
for line in open(f"{BF}/model_readings.jsonl"):
    r = json.loads(line)
    gem[r["title"]] = r["reading"]

events = []
for line in open(f"{BF}/news_backfill.jsonl"):
    r = json.loads(line)
    day = pd.Timestamp(r["seen_at"][:10])
    for s in r.get("symbols") or []:
        events.append((day, s.removesuffix(".NS"), r["title"], r["reading"], gem.get(r["title"])))

b = pd.read_csv(f"{BH}/bhav.csv", header=None, names=["date", "sym", "o", "h", "l", "c", "pc", "vol"])
b["sym"] = b["sym"].replace({"ZOMATO": "ETERNAL"})
b["date"] = pd.to_datetime(b["date"])
b = b.drop_duplicates(["date", "sym"])
for col in ("o", "c", "pc"):
    b[col] = pd.to_numeric(b[col], errors="coerce")
b = b[(b.o > 0) & (b.c > 0) & (b.pc > 0)]
oc = b.pivot(index="date", columns="sym", values="o")
oc = np.log(b.pivot(index="date", columns="sym", values="c") / oc)
cc = np.log(b.pivot(index="date", columns="sym", values="c") / b.pivot(index="date", columns="sym", values="pc"))
days = oc.index
# The market over the same window: the average NIFTY 50 stock. Not NIFTYBEES: the ETF's official
# open sits above where it trades, so its open->close averages -0.41% a day against -0.04% for the
# average stock (Jun 2023 - Nov 2026), which would make every stock look 0.37% better.
stock_cols = [c for c in oc.columns if c not in ("NIFTYBEES", "GOLDBEES")]
oc["MKT"] = oc[stock_cols].mean(axis=1)
cc["MKT"] = cc[stock_cols].mean(axis=1)


def window(sym, day, n):
    """Abnormal log return from the open of the first trading day after `day`, n days long."""
    i = days.searchsorted(day, side="right")
    if i + n > len(days) or sym not in oc.columns:
        return None
    d0 = days[i]
    r = oc.at[d0, sym] + sum(cc.at[days[i + k], sym] for k in range(1, n))
    m = oc.at[d0, "MKT"] + sum(cc.at[days[i + k], "MKT"] for k in range(1, n))
    return None if np.isnan(r) or np.isnan(m) else r - m


def test(label, pick):
    for name, n in (("next day", 1), ("3 days", 3)):
        rows = []
        for day, sym, title, built, model in events:
            reading = pick(built, model)
            if not reading or reading["direction"] == 0 or reading["materiality"] < 2:
                continue
            w = window(sym, day, n)
            if w is not None:
                rows.append((reading["direction"], w))
        if len(rows) < 30:
            print(f"  {label:34} {name:8}: only {len(rows)} events")
            continue
        a = np.array(rows)
        net = a[:, 0] * a[:, 1] - COST[name]
        good, bad = a[a[:, 0] > 0, 1], a[a[:, 0] < 0, 1]
        t = net.mean() / net.std() * math.sqrt(len(net))
        contra = bad - COST[name]  # buying after bad news instead (a reversal)
        tc = contra.mean() / contra.std() * math.sqrt(len(contra)) if len(contra) > 1 else 0
        print(f"  {label:34} {name:8}: {len(net):5} trades, after costs {net.mean()*100:+.3f}% (t {t:+.1f}); "
              f"after good news {good.mean()*100:+.3f}% (n {len(good)}), after bad news {bad.mean()*100:+.3f}% (n {len(bad)}); buying after bad news nets {contra.mean()*100:+.3f}% (t {tc:+.1f})")


print("Trading the reader's direction from the next open (beyond the average NIFTY 50 stock):")
test("built-in reader, all headlines", lambda b_, m: b_)
test("built-in reader, Gemini-read subset", lambda b_, m: b_ if m else None)
test("Gemini, same subset", lambda b_, m: m)
print(f"(Gemini has read {len(gem)} of the backfill's headlines so far; the free quota stopped it.)")
