"""Official NSE daily open/high/low/close (bhavcopy) for the NIFTY 50, 2016 on (28 Nov 2026).

Yahoo's daily "open" for NSE stocks is unreliable (about 0.13% off; phase 0.10), which matters
when the trade is "buy at the open, sell at the close" and costs ~0.11%. Bhavcopy files are NSE's
own end-of-day reports on nsearchives (roadmap #45). Old format until early July 2024, UDiFF
after. Paced, one file at a time; holidays answer 404. Same-day open->close and close->close via
the exchange's PREVCLOSE are both unaffected by splits.

Usage: python3 -I scripts/research/fetch_bhavcopy.py <out dir>   (resumable; writes bhav.csv)
"""
import csv, datetime as dt, io, os, sys, time, urllib.request, zipfile

OUT = sys.argv[1]
SYMS = {
    "ADANIENT", "ADANIPORTS", "APOLLOHOSP", "ASIANPAINT", "AXISBANK", "BAJAJ-AUTO", "BAJFINANCE",
    "BAJAJFINSV", "BEL", "BHARTIARTL", "CIPLA", "COALINDIA", "DRREDDY", "EICHERMOT", "ETERNAL",
    "ZOMATO", "GRASIM", "HCLTECH", "HDFCBANK", "HDFCLIFE", "HINDALCO", "HINDUNILVR", "ICICIBANK",
    "INDIGO", "INFY", "ITC", "JIOFIN", "JSWSTEEL", "KOTAKBANK", "LT", "M&M", "MARUTI", "MAXHEALTH",
    "NESTLEIND", "NTPC", "ONGC", "POWERGRID", "RELIANCE", "SBILIFE", "SHRIRAMFIN", "SBIN",
    "SUNPHARMA", "TATACONSUM", "TMPV", "TATAMOTORS", "TATASTEEL", "TCS", "TECHM", "TITAN", "TRENT",
    "ULTRACEMCO", "WIPRO", "NIFTYBEES", "GOLDBEES",
}
UA = {"User-Agent": "Mozilla/5.0 (research script; end-of-day files only)"}
MON = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"]


def urls(d):
    new = f"https://nsearchives.nseindia.com/content/cm/BhavCopy_NSE_CM_0_0_0_{d:%Y%m%d}_F_0000.csv.zip"
    old = (f"https://nsearchives.nseindia.com/content/historical/EQUITIES/{d.year}/{MON[d.month-1]}/"
           f"cm{d:%d}{MON[d.month-1]}{d.year}bhav.csv.zip")
    return [new, old] if d >= dt.date(2024, 7, 1) else [old]


def rows(d):
    for u in urls(d):
        try:
            data = urllib.request.urlopen(urllib.request.Request(u, headers=UA), timeout=30).read()
        except Exception:
            continue
        z = zipfile.ZipFile(io.BytesIO(data))
        text = z.read(z.namelist()[0]).decode("utf-8", "replace")
        for r in csv.DictReader(io.StringIO(text)):
            r = {k.strip(): (v or "").strip() for k, v in r.items() if k}
            sym = r.get("SYMBOL") or r.get("TckrSymb")
            series = r.get("SERIES") or r.get("SctySrs")
            if sym in SYMS and series == "EQ":
                g = lambda a, b: r.get(a) or r.get(b)
                yield [d.isoformat(), sym, g("OPEN", "OpnPric"), g("HIGH", "HghPric"), g("LOW", "LwPric"),
                       g("CLOSE", "ClsPric"), g("PREVCLOSE", "PrvsClsgPric"), g("TOTTRDQTY", "TtlTradgVol")]
        return True
    return False


path = os.path.join(OUT, "bhav.csv")
done = set()
if os.path.exists(path):
    done = {line.split(",")[0] for line in open(path)}
with open(path, "a", newline="") as f:
    w = csv.writer(f)
    d, end = dt.date(2016, 1, 1), dt.date.today()
    n = 0
    while d <= end:
        if d.weekday() < 5 and d.isoformat() not in done:
            got = list(rows(d))
            w.writerows(got)
            f.flush()
            n += 1
            if n % 100 == 0:
                print(d, "files tried", n, flush=True)
            time.sleep(0.6)
        d += dt.timedelta(days=1)
print("done")
