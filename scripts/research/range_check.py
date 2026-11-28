"""Has the "9 in 10" range held, and can the direction be told? (28 Nov 2026)

Walk forward over the NIFTY 50 and the index (10 years, daily, from fetch_nifty50.py), with
non-overlapping windows from day 500. At each start: the range is the 5th-95th percentile of
every earlier stretch of the same length; the direction guess is "up" if most earlier stretches
rose. Compared with a normal curve on today's EWMA volatility (close to the old fan), and with
the same range rescaled by today's volatility.

Usage: python3 -I scripts/research/range_check.py <dir with daily.json>
"""
import json, math, sys

d = json.load(open(sys.argv[1] + "/daily.json"))
WARM, LAM = 500, 0.94


def series(v):
    a = [x for x in v["adj"] if x]
    r = [math.log(a[i] / a[i - 1]) for i in range(1, len(a)) if a[i - 1] > 0]
    return [x for x in r if abs(x) < math.log(2)]  # halving/doubling glitches


for h in (5, 21, 63):
    held = {"hist": 0, "histvol": 0, "ewma": 0}
    n = right = up_n = 0
    for v in d.values():
        r = series(v)
        cum = [0.0]
        for x in r:
            cum.append(cum[-1] + x)
        ew, var = [0.0] * (len(r) + 1), sum(x * x for x in r[:60]) / 60
        for i, x in enumerate(r):
            ew[i] = var
            var = LAM * var + (1 - LAM) * x * x
        t = WARM
        while t + h <= len(r):
            past = sorted(cum[i + h] - cum[i] for i in range(0, t - h + 1))
            k = len(past)
            q = lambda p: past[min(k - 1, int(p * k))]
            y = cum[t + h] - cum[t]
            lo, hi = q(0.05), q(0.95)
            held["hist"] += lo <= y <= hi
            sd_all, sd_now, m = math.sqrt(sum(x * x for x in r[:t]) / t), math.sqrt(ew[t]), sum(past) / k
            sc = sd_now / sd_all
            held["histvol"] += m + (lo - m) * sc <= y <= m + (hi - m) * sc
            mu, w = cum[t] / t * h, 1.645 * sd_now * math.sqrt(h)
            held["ewma"] += mu - w <= y <= mu + w
            p = sum(1 for x in past if x > 0) / k
            right += (p > 0.5) == (y > 0)
            up_n += y > 0
            n += 1
            t += h
    print(f"h={h:>2} n={n}: range held past-stretches {held['hist']/n:.3f}, rescaled {held['histvol']/n:.3f}, "
          f"ewma-normal {held['ewma']/n:.3f}; direction right {right/n:.3f} vs always-up {up_n/n:.3f}")
