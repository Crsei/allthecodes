#!/usr/bin/env bash
set -euo pipefail

BIN="${ALLTHECODES_BIN:-allthecodes}"
PORT="${PORT:-19846}"
TIMEOUT_SECS="${TIMEOUT_SECS:-120}"
PROMPT="${PROMPT:-KAIROS live smoke: reply with exactly 'kairos live smoke ok'.}"
BASE_URL="http://127.0.0.1:${PORT}"

cleanup() {
  FEATURE_KAIROS=1 "$BIN" daemon stop >/dev/null 2>&1 || true
}
trap cleanup EXIT

FEATURE_KAIROS=1 "$BIN" --port "$PORT" daemon start
TOKEN="$(FEATURE_KAIROS=1 "$BIN" daemon token)"

SUBMIT_BODY="$(
  PROMPT="$PROMPT" python3 - <<'PY'
import json
import os

print(json.dumps({
    "text": os.environ["PROMPT"],
    "idempotency_key": "kairos-live-smoke",
}))
PY
)"

SUBMIT_RESPONSE="$(
  curl -fsS \
    -H "content-type: application/json" \
    -H "x-allthecodes-daemon-token: ${TOKEN}" \
    -d "$SUBMIT_BODY" \
    "${BASE_URL}/api/submit"
)"
COMMAND_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["command_id"])' <<<"$SUBMIT_RESPONSE")"
echo "queued command: ${COMMAND_ID}"

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  HISTORY="$(curl -fsS "${BASE_URL}/api/history")"
  if HISTORY="$HISTORY" python3 - "$COMMAND_ID" <<'PY'; then
import json
import os
import sys

command_id = sys.argv[1]
history = json.loads(os.environ["HISTORY"])
terminal_events = {"stream_end", "submit_completed", "command_failed"}
for event in history.get("daemon_events", []):
    if event.get("command_id") == command_id and event.get("event_type") in terminal_events:
        print(f"terminal event: {event['event_type']}")
        sys.exit(0)
sys.exit(1)
PY
    break
  fi
  sleep 2
done

if (( SECONDS >= deadline )); then
  echo "timed out waiting for command ${COMMAND_ID}" >&2
  exit 1
fi

EVENTS_FILE="$(mktemp)"
curl -fsS -N --max-time 3 \
  "${BASE_URL}/events?client_id=kairos-live-smoke&last_event_id=0" \
  >"$EVENTS_FILE" || true

if ! grep -E 'event: (daemon_)?(stream_end|submit_completed|command_failed)' "$EVENTS_FILE" >/dev/null; then
  echo "SSE replay did not include a terminal submit event" >&2
  cat "$EVENTS_FILE" >&2
  exit 1
fi

echo "live smoke passed"
