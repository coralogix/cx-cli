#!/usr/bin/env bash
# Runs the CLI telemetry collector config (contributing/telemetry-design-draft.md
# § 2, copied to scripts/telemetry-collector/config.yaml) in Docker, with the
# Coralogix exporter swapped for a file (local.yaml). Then:
#   1. a real `cx` binary runs a scenario covering all eight event kinds
#      against a stub API and flushes to the collector; every event it records
#      (learned from the same scenario in CX_TELEMETRY=log mode) must be
#      exported with all its attributes, with service.name = cx-cli;
#   2. hand-made bad requests are posted; none may be exported, and an unknown
#      attribute must be stripped.
#
# Usage: scripts/telemetry-collector-check.sh [path/to/cx]   (default: target/debug/cx)
# Needs: docker, curl, python3.
set -euo pipefail

IMAGE=otel/opentelemetry-collector-contrib:0.165.0
HERE=$(cd "$(dirname "$0")" && pwd)
CX=${1:-$HERE/../target/debug/cx}
PORT=${PORT:-14318}
ENDPOINT=http://localhost:$PORT/v1/logs
NAME=cx-telemetry-check-$$
WORK=$(mktemp -d)
OUT=$WORK/out/events.jsonl

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 1' INT TERM # run the EXIT trap on Ctrl-C or kill too

[ -x "$CX" ] || { echo "cx binary not found at $CX (run cargo build)"; exit 1; }
mkdir -p "$WORK/out" "$WORK/home/project"
chmod 777 "$WORK/out" # the collector runs as a non-root user

echo "== validate config"
docker run --rm -v "$HERE/telemetry-collector:/cfg:ro" \
  -e CORALOGIX_DOMAIN=coralogix.com -e CORALOGIX_PRIVATE_KEY=unused \
  "$IMAGE" validate --config=/cfg/config.yaml --config=/cfg/local.yaml

echo "== start collector on :$PORT"
docker run -d --name "$NAME" -p "$PORT:4318" \
  -v "$HERE/telemetry-collector:/cfg:ro" -v "$WORK/out:/out" \
  -e CORALOGIX_DOMAIN=coralogix.com -e CORALOGIX_PRIVATE_KEY=unused \
  "$IMAGE" --config=/cfg/config.yaml --config=/cfg/local.yaml >/dev/null
for _ in $(seq 1 50); do
  curl -s -o /dev/null "$ENDPOINT" && break
  sleep 0.2
done

# ── 1. Real CLI events ────────────────────────────────────────────────────────
echo "== real cx events"
# A stub Coralogix API: every GET answers an empty PromQL result, so
# `metrics query` succeeds; anything else gets 501.
STUB_PORT=$((PORT + 1))
python3 -c '
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"{\"status\":\"success\",\"data\":{\"resultType\":\"vector\",\"result\":[]}}"
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args):
        pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
' "$STUB_PORT" &
STUB_PID=$!
disown "$STUB_PID" # no "Terminated" line when it is killed
trap 'kill $STUB_PID 2>/dev/null; cleanup' EXIT
# A fake npx for `cx skills install`: lists nothing, installs successfully.
mkdir -p "$WORK/bin"
printf '#!/bin/sh\nif [ "$1" = "--version" ]; then echo 10.0.0; exit 0; fi\nif [ "$3" = "ls" ]; then echo "[]"; fi\nexit 0\n' \
  >"$WORK/bin/npx"
chmod +x "$WORK/bin/npx"

# The same scenario runs twice: in inspect mode (CX_TELEMETRY=log), to learn
# which events the CLI records, and for real against the collector. All eight
# event kinds occur. $1: home dir, $2: log | on.
scenario() {
  local home=$1 mode=$2
  mkdir -p "$home/project"
  run_cx() {
    local telemetry=()
    if [ "$mode" = log ]; then telemetry=(CX_TELEMETRY=log); else telemetry=(CX_TELEMETRY_ENDPOINT="$ENDPOINT"); fi
    (cd "$home/project" && env -u CX_API_KEY -u CX_REGION -u CX_PROFILE -u CX_TELEMETRY \
      -u DO_NOT_TRACK -u CX_NO_UPDATE_NOTIFIER -u CX_TELEMETRY_ENDPOINT \
      HOME="$home" CX_HOME="$home" CX_CREDENTIAL_STORAGE=file "${telemetry[@]}" "$@" \
      >/dev/null 2>>"$home/stderr.txt") || true
  }
  run_cx "$CX" dataprime list --name SENTINEL_SECRET   # first_run (fresh home), command
  # Pretend the install was upgraded (version_changed) and a newer release is
  # cached (update_notice), keeping the install id the first run created.
  python3 -c '
import datetime, json, sys
path = sys.argv[1]
state = json.load(open(path))
state.update(last_run_version="0.0.1", latest_binary="99.0.0",
             last_checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
json.dump(state, open(path, "w"))
' "$home/.cx/state.json"
  run_cx "$CX" alertz list                             # version_changed, parse_error
  run_cx "$CX" profiles add check --api-key k --url "http://127.0.0.1:$STUB_PORT"  # profile_added (flushes)
  run_cx env PATH="$WORK/bin" "$CX" skills install --global  # skills_install
  run_cx "$CX" alerts delete abc --yes                 # confirm (flushes)
  run_cx "$CX" metrics query up                        # first_query_succeeded (flushes)
}
scenario "$WORK/log-home" log
scenario "$WORK/home" on
grep '^cx telemetry: ' "$WORK/log-home/stderr.txt" | sed 's/^cx telemetry: //' >"$WORK/expected.jsonl"
[ ! -s "$WORK/home/.cx/telemetry-spool.jsonl" ] || { echo "events left unsent in the spool"; exit 1; }
sleep 2

# ── 2. Bad requests ───────────────────────────────────────────────────────────
echo "== bad requests"
now=$(python3 -c 'import time; print(time.time_ns())')
post() { curl -s -o /dev/null -w '%{http_code}' -H 'Content-Type: application/json' --data-binary "$1" "$ENDPOINT"; }
# One OTLP request with one record. $1: record attributes JSON, $2: time, $3: install id.
request() {
  printf '{"resourceLogs":[{"resource":{"attributes":[
    {"key":"service.name","value":{"stringValue":"evil"}},
    {"key":"service.version","value":{"stringValue":"0.1.34"}},
    {"key":"os.type","value":{"stringValue":"linux"}},
    {"key":"host.arch","value":{"stringValue":"amd64"}},
    {"key":"cx.install.id","value":{"stringValue":"%s"}},
    {"key":"cx.install.method","value":{"stringValue":"curl"}}]},
    "scopeLogs":[{"logRecords":[{"timeUnixNano":"%s","body":{"stringValue":"free text"},"attributes":%s}]}]}]}' \
    "$3" "$2" "$1"
}
base() { # $1: invocation id, $2: extra attributes
  printf '[{"key":"event.name","value":{"stringValue":"cli.first_run"}},
    {"key":"cx.invocation.id","value":{"stringValue":"%s"}},
    {"key":"cx.agent","value":{"stringValue":"direct"}},
    {"key":"cx.ci","value":{"boolValue":false}},
    {"key":"cx.tty","value":{"boolValue":true}}%s]' "$1" "$2"
}
INSTALL=11111111-1111-4111-8111-111111111111
GOOD=22222222-2222-4222-8222-222222222222
echo "unknown attribute (kept, stripped): $(post "$(request "$(base $GOOD ',{"key":"cx.query","value":{"stringValue":"SECRET-QUERY"}}')" "$now" $INSTALL)")"
echo "bad enum:          $(post "$(request "$(base 33333333-3333-4333-8333-333333333333 ',{"key":"cx.outcome","value":{"stringValue":"pwned"}}')" "$now" $INSTALL)")"
echo "stale timestamp:   $(post "$(request "$(base 44444444-4444-4444-8444-444444444444 '')" "$((now - 8 * 86400 * 1000000000))" $INSTALL)")"
echo "wrong type:        $(post "$(request "$(base 55555555-5555-4555-8555-555555555555 ',{"key":"cx.duration_ms","value":{"stringValue":"12"}}')" "$now" $INSTALL)")"
echo "bad install id:    $(post "$(request "$(base 66666666-6666-4666-8666-666666666666 '')" "$now" not-a-uuid)")"
# Extra attributes go through variables: bash 3.2 mangles \" inside nested $(...).
flags31=$(python3 -c 'print(",".join("--f%d" % i for i in range(31)))')
extra=",{\"key\":\"cx.command.flags\",\"value\":{\"stringValue\":\"$flags31\"}}"
echo "31 flags:          $(post "$(request "$(base 88888888-8888-4888-8888-888888888888 "$extra")" "$now" $INSTALL)")"
big=$(python3 -c 'print("x" * 70000)')
extra=",{\"key\":\"cx.query\",\"value\":{\"stringValue\":\"$big\"}}"
echo "body over 64 KiB:  $(post "$(request "$(base 77777777-7777-4777-8777-777777777777 "$extra")" "$now" $INSTALL)")"
sleep 2

# ── Assertions over what the collector exported ───────────────────────────────
echo "== exported"
python3 - "$OUT" "$GOOD" "$WORK/expected.jsonl" <<'EOF'
import json, sys
from collections import Counter
path, good, expected_path = sys.argv[1], sys.argv[2], sys.argv[3]
RESOURCE_KEYS = {"service.name", "service.version", "os.type", "host.arch", "cx.install.id", "cx.install.method"}

records = []
for line in open(path):
    for rl in json.loads(line)["resourceLogs"]:
        res = {a["key"]: a["value"] for a in rl["resource"]["attributes"]}
        for sl in rl["scopeLogs"]:
            for r in sl["logRecords"]:
                attrs = {a["key"]: next(iter(a["value"].values())) for a in r.get("attributes", [])}
                records.append((res, attrs, r))

for res, attrs, r in records:
    assert res["service.name"]["stringValue"] == "cx-cli", res
    assert r.get("body", {}).get("stringValue", "") == "", r
    assert "SENTINEL_SECRET" not in json.dumps(attrs)

# What the CLI recorded (inspect-mode run) vs what the collector exported.
expected = [json.loads(l) for l in open(expected_path)]
exported = [a for _, a, _ in records if a.get("cx.invocation.id", "").count("-") == 4
            and not a["cx.invocation.id"][:4] in {"2222", "3333", "4444", "5555", "6666", "7777", "8888"}]
want = Counter(e["event.name"] for e in expected)
got = Counter(a["event.name"] for a in exported)
print("recorded:", dict(sorted(want.items())))
print("exported:", dict(sorted(got.items())))
assert len(want) == 8, f"scenario must cover all 8 event kinds: {sorted(want)}"
assert got == want, "every recorded event must be exported"
for name in want:
    keys = lambda events: {frozenset(k for k in e if k not in RESOURCE_KEYS | {"time_unix_nano"}) for e in events}
    sent = keys(e for e in expected if e["event.name"] == name)
    kept = keys(a for a in exported if a["event.name"] == name)
    assert sent == kept, f"{name}: collector changed the attribute keys: {sent} vs {kept}"
print("all 8 event kinds exported with every attribute kept")

bad = [a for _, a, _ in records if a.get("cx.invocation.id", "").startswith(("3333", "4444", "5555", "6666", "7777", "8888"))]
assert not bad, f"bad records exported: {bad}"
kept = [a for _, a, _ in records if a.get("cx.invocation.id") == good]
assert len(kept) == 1 and "cx.query" not in kept[0], kept
print("bad requests: none exported; unknown attribute stripped")
print("OK")
EOF
