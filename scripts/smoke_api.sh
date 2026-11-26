#!/usr/bin/env bash
set -euo pipefail

# Usage: BASE_URL=https://host GM_ACCESS_TOKEN=... scripts/smoke_api.sh
# SMOKE_MARKET_DATA=0 skips the /api/v1 checks (they need Yahoo to be reachable).
BASE_URL="${BASE_URL:-http://127.0.0.1:8000}"
AUTH=()
if [ -n "${GM_ACCESS_TOKEN:-}" ]; then
  AUTH=(-H "Authorization: Bearer $GM_ACCESS_TOKEN")
  # The gate must actually block requests without the token.
  code="$(command curl -s -o /dev/null -w '%{http_code}' "$BASE_URL/version")"
  [ "$code" = "401" ] || { echo "access gate not enforced: /version without token returned $code" >&2; exit 1; }
fi
# bash 3.2 (macOS) treats an empty array as unbound under `set -u`, hence the ${AUTH[@]+...} form.
curl() { command curl ${AUTH[@]+"${AUTH[@]}"} "$@"; }

health="$(curl -fsS "$BASE_URL/health")"
python3 - "$health" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["status"] == "ok", payload
assert payload["service"] == "gm-api", payload
PY

ready="$(curl -fsS "$BASE_URL/ready")"
python3 - "$ready" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["status"] == "ready", payload
assert payload["service"] == "gm-api", payload
assert "persistence" in payload, payload
PY

version="$(curl -fsS "$BASE_URL/version")"
python3 - "$version" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["service"] == "gm-api", payload
assert payload["version"], payload
assert payload["model_version"], payload
PY

openapi="$(curl -fsS "$BASE_URL/openapi.json")"
python3 - "$openapi" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["openapi"] == "3.1.0", payload
assert "/decide" in payload["paths"], payload
assert "/ready" in payload["paths"], payload
assert "/payments/state" in payload["paths"], payload
assert "/payments/orders" in payload["paths"], payload
assert "/payments/verify" in payload["paths"], payload
assert "/payments/webhooks/razorpay" in payload["paths"], payload
PY

payment_state="$(curl -fsS "$BASE_URL/payments/state")"
python3 - "$payment_state" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["mode"] == "TEST_MODE", payload
assert payload["live_billing_enabled"] is False, payload
assert payload["provider"]["name"] == "razorpay-test", payload
assert payload["provider"]["mode"] == "TEST_MODE", payload
PY

payment_order_request='{
  "account_id": "acct_release_smoke",
  "amount_paise": 49900,
  "currency": "INR",
  "description": "MV access",
  "success_url": "https://example.test/payments/success"
}'

payment_order="$(curl -fsS -X POST "$BASE_URL/payments/orders" -H "Content-Type: application/json" -d "$payment_order_request")"
python3 - "$payment_order" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["provider"] == "razorpay-test", payload
assert payload["key_id"], payload
assert payload["order_id"].startswith("order_test_"), payload
assert payload["test_payment_id"].startswith("pay_test_"), payload
assert payload["test_signature"], payload
PY

payment_verify_request="$(python3 - "$payment_order" <<'PY'
import json
import sys

order = json.loads(sys.argv[1])
print(json.dumps({
    "order_id": order["order_id"],
    "payment_id": order["test_payment_id"],
    "signature": order["test_signature"],
}))
PY
)"

payment_verification="$(curl -fsS -X POST "$BASE_URL/payments/verify" -H "Content-Type: application/json" -d "$payment_verify_request")"
python3 - "$payment_verification" "$payment_order" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
order = json.loads(sys.argv[2])
assert payload["verified"] is True, payload
assert payload["order_id"] == order["order_id"], payload
assert payload["payment_id"] == order["test_payment_id"], payload
PY

webhook_body='{"event":"payment.captured","payload":{"payment":{"entity":{"id":"pay_test_webhook","order_id":"order_test_webhook","amount":49900,"currency":"INR","captured":true}}}}'
webhook_signature="$(RAZORPAY_WEBHOOK_SECRET="${RAZORPAY_WEBHOOK_SECRET:-local_webhook_signing_key}" RAW_BODY="$webhook_body" python3 - <<'PY'
import hashlib
import hmac
import os

print(hmac.new(
    os.environ["RAZORPAY_WEBHOOK_SECRET"].encode(),
    os.environ["RAW_BODY"].encode(),
    hashlib.sha256,
).hexdigest())
PY
)"

webhook_verification="$(curl -fsS -X POST "$BASE_URL/payments/webhooks/razorpay" -H "Content-Type: application/json" -H "x-razorpay-signature: $webhook_signature" -d "$webhook_body")"
python3 - "$webhook_verification" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["verification"]["verified"] is True, payload
assert payload["verification"]["event"] == "payment.captured", payload
assert payload["event"]["provider_order_id"] == "order_test_webhook", payload
assert payload["event"]["provider_payment_id"] == "pay_test_webhook", payload
PY

# /decide is retired (#65): it must answer 410 and point to validated advice.
decide_code="$(curl -s -o /tmp/gm_decide.json -w '%{http_code}' -X POST "$BASE_URL/decide" -H "Content-Type: application/json" -d '{}')"
test "$decide_code" = "410" || { echo "/decide returned $decide_code, expected 410"; exit 1; }
grep -q "/api/v1/analyze" /tmp/gm_decide.json

if [ "${SMOKE_MARKET_DATA:-1}" = "1" ]; then
  search="$(curl -fsS "$BASE_URL/api/v1/instruments/search?q=reliance")"
  python3 - "$search" <<'PY'
import json
import sys

hits = json.loads(sys.argv[1])["results"]
assert any(h["symbol"] == "RELIANCE.NS" for h in hits), hits
PY

  for symbol in RELIANCE.NS TCS.NS %5ENSEI; do
    bars="$(curl -fsS "$BASE_URL/api/v1/bars?symbol=$symbol&range=1y")"
    python3 - "$bars" "$symbol" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
bars = payload["chart"]["bars"]
assert len(bars) > 200, (sys.argv[2], len(bars))
assert [b["date"] for b in bars] == sorted(b["date"] for b in bars), "bars out of order"
assert payload["quality"]["passed"] is True, payload["quality"]
assert payload["chart"]["source"] == "yahoo-chart-v8", payload["chart"]["source"]
PY
  done

  quote="$(curl -fsS "$BASE_URL/api/v1/quote?symbol=RELIANCE.NS")"
  python3 - "$quote" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["quote"]["last_price"] > 0, payload
assert payload["quote"]["currency"] == "INR", payload
assert isinstance(payload["quote"]["market_open"], bool), payload
PY

  # Intraday for the live chart: crypto trades round the clock, so 1-minute bars always exist.
  intraday="$(curl -fsS "$BASE_URL/api/v1/intraday?symbol=BTC-USD&range=1d")"
  python3 - "$intraday" <<'PY'
import json
import sys

chart = json.loads(sys.argv[1])["chart"]
assert chart["interval"] == "1m", chart["interval"]
assert len(chart["points"]) > 100, len(chart["points"])
assert [p["t"] for p in chart["points"]] == sorted(p["t"] for p in chart["points"]), "points out of order"
assert chart["previous_close"] > 0, chart["previous_close"]
PY

  # News from GDELT: it rate-limits shared IPs, so 503 is tolerated; a 200 must be well-formed.
  news_code="$(curl -s -o /tmp/gm_news.json -w '%{http_code}' "$BASE_URL/api/v1/news?symbol=RELIANCE.NS")"
  if [ "$news_code" = "200" ]; then
    python3 - /tmp/gm_news.json <<'PY'
import json
import sys

body = json.load(open(sys.argv[1]))
assert body["source"] == "gdelt-doc-2.0", body
assert body["query"].startswith('"Reliance Industries"'), body["query"]
assert isinstance(body["articles"], list), body
PY
  else
    test "$news_code" = "503" || { echo "news returned $news_code"; exit 1; }
    echo "  news: GDELT unavailable or rate-limited (503), tolerated"
  fi

  # The engine on three markets: US stock (auto S&P 500), Japanese stock (auto Nikkei), crypto (none).
  for spec in "AAPL:^GSPC" "7203.T:^N225" "BTC-USD:"; do
    symbol="${spec%%:*}"; bench="${spec#*:}"
    analysis="$(curl -fsS "$BASE_URL/api/v1/analyze?symbol=$symbol&paths=500")"
    python3 - "$analysis" "$symbol" "$bench" <<'PY'
import json
import sys

payload, symbol, bench = json.loads(sys.argv[1]), sys.argv[2], sys.argv[3]
report = payload["report"]
assert report["symbol"] == symbol, report["symbol"]
assert report["n_returns"] > 1000, report["n_returns"]
assert report["volatility"]["garch"] is not None, "GARCH not fitted"
assert len(report["projections"]) == 2 and len(report["projections"][1]["fan"]) == 63
assert report["findings"], "no findings"
v = report["validation"]
assert v and v["n_forecasts"] > 500 and len(v["models"]) == 3, v and v["n_forecasts"]
if bench:
    assert payload["benchmark"]["symbol"] == bench and payload["benchmark"]["used"], payload["benchmark"]
    assert 0.0 < report["benchmark"]["beta"] < 3.0, report["benchmark"]
else:
    assert payload["benchmark"] is None, payload["benchmark"]
PY
  done

  comparison="$(curl -fsS "$BASE_URL/api/v1/compare?symbols=AAPL,MSFT,RELIANCE.NS,7203.T")"
  python3 - "$comparison" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
c = payload["comparison"]
assert c["symbols"] == ["AAPL", "MSFT", "RELIANCE.NS", "7203.T"], c["symbols"]
assert c["n_common_days"] > 900, c["n_common_days"]
assert len(c["pairs"]) == 6 and len(c["correlation_weekly"]) == 4
assert any("different times of day" in n for n in payload["notes"]), payload["notes"]
assert c["findings"], "no findings"
PY

  code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE_URL/api/v1/quote?symbol=NOTAREALTICKER.NS")"
  [ "$code" = "404" ] || { echo "unknown symbol returned $code, expected 404" >&2; exit 1; }

  # The stream must answer with an event-stream; BTC trades 24/7 so a tick is expected.
  stream="$(curl -sN --max-time 15 "$BASE_URL/api/v1/stream?symbols=BTC-USD" | head -c 2000 || true)"
  case "$stream" in
    *"event: tick"*) ;;
    *) echo "no tick on /api/v1/stream within 15 s: ${stream:0:200}" >&2; exit 1 ;;
  esac
fi

echo "API smoke test passed against $BASE_URL"
