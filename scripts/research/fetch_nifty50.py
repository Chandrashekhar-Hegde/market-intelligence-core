import json, re, sys, time, urllib.request, urllib.parse
src = open('crates/gm-integrations/src/universe.rs').read()
block = src[src.index('NIFTY50'):src.index('OTHER_ASSETS')]
syms = re.findall(r'\("([A-Z0-9&\-]+\.NS)",', block) + ['^NSEI']
out_d, out_i = {}, {}
def get(u):
    for _ in range(3):
        try:
            return json.load(urllib.request.urlopen(urllib.request.Request(u, headers={"User-Agent": "Mozilla/5.0"}), timeout=30))['chart']['result'][0]
        except Exception:
            time.sleep(2)
for s in syms:
    q = urllib.parse.quote(s)
    r = get(f"https://query1.finance.yahoo.com/v8/finance/chart/{q}?range=10y&interval=1d&events=div%2Csplits")
    if r:
        qq = r['indicators']['quote'][0]
        out_d[s] = {'t': r['timestamp'], 'o': qq['open'], 'h': qq['high'], 'l': qq['low'], 'c': qq['close'], 'v': qq['volume'], 'adj': r['indicators']['adjclose'][0]['adjclose']}
    r = get(f"https://query1.finance.yahoo.com/v8/finance/chart/{q}?range=60d&interval=5m")
    if r:
        qq = r['indicators']['quote'][0]
        out_i[s] = {'t': r['timestamp'], 'o': qq['open'], 'h': qq['high'], 'l': qq['low'], 'c': qq['close'], 'v': qq['volume'], 'gmtoffset': r['meta'].get('gmtoffset', 19800)}
    time.sleep(0.4)
json.dump(out_d, open(sys.argv[1] + '/daily.json', 'w')); json.dump(out_i, open(sys.argv[1] + '/intraday.json', 'w'))
print(len(out_d), 'daily,', len(out_i), 'intraday')
