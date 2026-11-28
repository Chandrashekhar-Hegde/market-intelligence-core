# Research scripts behind the advice

These one-off studies decided what the app does. Their results are quoted, with dates, in
`docs/METHODOLOGY.md`. Run them with the reference venv (`.refenv`, see AGENTS.md):

```bash
.refenv/bin/python scripts/research/fetch_nifty50.py /tmp/bt                # 10y daily OHLCV + 60d 5-min bars
.refenv/bin/python scripts/research/vol_sizing_backtest.py /tmp/bt          # "hold less when jumpy" vs holding
.refenv/bin/python scripts/research/short_horizons.py /tmp/bt               # 1–21 day rules, day trades on daily bars
.refenv/bin/python scripts/research/day_trading_5min.py /tmp/bt             # gap fades and intraday momentum on 5-min bars
.refenv/bin/python scripts/research/close_range.py /tmp/bt                  # today's close range: coverage of each method
python3 -I scripts/research/range_check.py /tmp/bt                          # 0.11: has the 9-in-10 range held? can direction be told?
python3 -I scripts/research/fetch_bhavcopy.py /tmp/bhav                     # 0.11: NSE's official daily OHLC, 2016 on (~40 min, paced)
curl -s "$API/api/v1/bars?symbol=%5EGSPC&range=10y" > /tmp/bhav/gspc.json   # the US session the morning study uses
.refenv/bin/python scripts/research/quant_study.py /tmp/bhav                # 0.11: gap, OU s-score, HMM, US cue... open->close and 3 days
.refenv/bin/python scripts/research/news_next_day.py <backfill dir> /tmp/bhav  # 0.11: does a news reading predict the next day?
```

`panel_rules.py` and `beat_fd.py` read the older `prices.json` format (adjusted closes only) from
the Phase 0.9 study. They are kept as written.

Results move as data accrues. Re-run them before changing a conclusion in the app.
