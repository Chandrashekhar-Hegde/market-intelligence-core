import json, sys
import numpy as np, pandas as pd
raw = json.load(open(sys.argv[1]))
FD = 0.07
def series(s):
    x = pd.Series({pd.Timestamp(int(t), unit='s').normalize(): v for t, v in raw[s].items()}).sort_index()
    return np.log(x[~x.index.duplicated()])
for s in ['^NSEI', 'RELIANCE.NS', 'HDFCBANK.NS', 'TCS.NS', 'GOLDBEES.NS', 'MON100.NS', 'ITC.NS']:
    lx = series(s); out = []
    for years in (1, 3, 5):
        n = 252 * years
        ret = (lx.shift(-n) - lx).dropna()
        beat = (ret > years * np.log(1 + FD)).mean()
        indep = len(ret) // n
        out.append(f"{years}y: beat FD {beat*100:.0f} in 100 (worst {np.expm1(ret.min())*100:+.0f}%, ~{indep} separate periods)")
    print(f"{s:12} " + " | ".join(out))
