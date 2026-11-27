# Research scripts behind the advice

These one-off studies decided what the app does. Their results are quoted, with dates, in
`docs/METHODOLOGY.md`. Run them with the reference venv (`.refenv`, see AGENTS.md):

```bash
.refenv/bin/python scripts/research/fetch_nifty50.py /tmp/bt                # 10y daily OHLCV + 60d 5-min bars
.refenv/bin/python scripts/research/vol_sizing_backtest.py /tmp/bt          # "hold less when jumpy" vs holding
.refenv/bin/python scripts/research/short_horizons.py /tmp/bt               # 1–21 day rules, day trades on daily bars
.refenv/bin/python scripts/research/day_trading_5min.py /tmp/bt             # gap fades and intraday momentum on 5-min bars
.refenv/bin/python scripts/research/close_range.py /tmp/bt                  # today's close range: coverage of each method
```

`panel_rules.py` and `beat_fd.py` read the older `prices.json` format (adjusted closes only) from
the Phase 0.9 study. They are kept as written.

Results move as data accrues. Re-run them before changing a conclusion in the app.
