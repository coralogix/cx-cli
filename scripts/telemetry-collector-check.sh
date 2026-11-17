#!/usr/bin/env bash
# Builds the CLI telemetry collector image (scripts/telemetry-collector/) and
# runs it in Docker with scripts/telemetry-collector/config.yaml, the
# Coralogix exporter swapped for a file (local.yaml). Then:
#   1. a real `cx` binary runs a scenario covering all eight event kinds
#      against a stub API and its sender posts to the collector; every event it
#      records (learned from the same scenario in CX_TELEMETRY=log mode) must
#      be exported with all its attributes, with service.name = cx-cli;
#   2. every event kind is posted with and without cx.team.id; all must pass;
#   3. hand-made bad requests are posted; none may be exported, and an unknown
#      attribute must be stripped;
#   4. a burst from one source IP must be limited while another IP passes.
#
# Usage: scripts/telemetry-collector-check.sh [path/to/cx]   (default: target/debug/cx)
#   CONFIG=path/to/config.yaml checks another copy of the collector config,
#   for example the deployed one (contributing/telemetry-design-draft.md § 5.2).
# Needs: docker, curl, python3, and a debug build of cx (release builds have
# no CX_TELEMETRY=log mode).
set -euo pipefail

IMAGE=cx-telemetry-otelcol:0.165.0
HERE=$(cd "$(dirname "$0")" && pwd)
CX=${1:-$HERE/../target/debug/cx}
CONFIG=${CONFIG:-$HERE/telemetry-collector/config.yaml}
CONFIG=$(cd "$(dirname "$CONFIG")" && pwd)/$(basename "$CONFIG")
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

echo "== build collector image"
docker build -q -t "$IMAGE" "$HERE/telemetry-collector" >/dev/null

echo "== validate $CONFIG"
MOUNTS=(-v "$CONFIG:/cfg/config.yaml:ro" -v "$HERE/telemetry-collector/local.yaml:/cfg/local.yaml:ro"
  -e CORALOGIX_DOMAIN=coralogix.com -e CORALOGIX_PRIVATE_KEY=unused)
# On its own, as it is deployed, and with the local exporters.
docker run --rm "${MOUNTS[@]}" "$IMAGE" validate --config=/cfg/config.yaml
docker run --rm "${MOUNTS[@]}" "$IMAGE" validate --config=/cfg/config.yaml --config=/cfg/local.yaml

echo "== start collector on :$PORT"
docker run -d --name "$NAME" -p "$PORT:4318" "${MOUNTS[@]}" -v "$WORK/out:/out" \
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
  run_cx "$CX" profiles add check --api-key k --url "http://127.0.0.1:$STUB_PORT"  # profile_added
  run_cx env PATH="$WORK/bin" "$CX" skills install --global  # skills_install
  run_cx "$CX" alerts delete abc --yes                 # confirm, no team id yet
  # As after a sign-in: from here on, events carry cx.team.id.
  echo 'cached_team_id = 4015532' >>"$home/.cx/profiles/check.toml"
  run_cx "$CX" metrics query up                        # first_query_succeeded, with a team id
  [ "$mode" = on ] || return 0
  # Each run started a detached sender. Run it here too until nothing is left.
  for _ in $(seq 1 50); do
    run_cx "$CX" __telemetry-send
    ls "$home/.cx"/telemetry-spool.jsonl* >/dev/null 2>&1 || return 0
    sleep 0.2
  done
  echo "events left unsent in the spool"
  exit 1
}
scenario "$WORK/log-home" log
grep '^cx telemetry: ' "$WORK/log-home/stderr.txt" | sed 's/^cx telemetry: //' >"$WORK/expected.jsonl" || true
[ -s "$WORK/expected.jsonl" ] || { echo "$CX ignores CX_TELEMETRY=log: this check needs a debug build (cargo build)"; exit 1; }
scenario "$WORK/home" on

# ── Hand-made requests ────────────────────────────────────────────────────────
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
base() { # $1: invocation id, $2: extra attributes, $3: event name (default cli.first_run)
  printf '[{"key":"event.name","value":{"stringValue":"%s"}},
    {"key":"cx.invocation.id","value":{"stringValue":"%s"}},
    {"key":"cx.agent","value":{"stringValue":"direct"}},
    {"key":"cx.ci","value":{"boolValue":false}},
    {"key":"cx.tty","value":{"boolValue":true}}%s]' "${3:-cli.first_run}" "$1" "$2"
}
INSTALL=11111111-1111-4111-8111-111111111111

# ── 2. Every event kind, with and without a team id ──────────────────────────
echo "== team id"
# Extra attributes go through variables: bash 3.2 mangles \" inside nested $(...).
team=',{"key":"cx.team.id","value":{"stringValue":"4015532"}}'
cmdpath=',{"key":"cx.command.path","value":{"stringValue":"logs"}}'
i=0
for name in first_run command parse_error onboarding skills_install update_notice version_changed confirm; do
  i=$((i + 1))
  extra=''
  [ "$name" != command ] || extra=$cmdpath
  post "$(request "$(base "9999999$i-0000-4000-8000-000000000000" "$extra$team" "cli.$name")" "$now" $INSTALL)" >/dev/null
  post "$(request "$(base "9999999$i-1111-4000-8000-000000000000" "$extra" "cli.$name")" "$now" $INSTALL)" >/dev/null
done
extra=',{"key":"cx.team.id","value":{"stringValue":"not-a-number"}}'
echo "bad team id:       $(post "$(request "$(base 99999999-2222-4000-8000-000000000000 "$extra")" "$now" $INSTALL)")"

# ── 3. Bad requests ───────────────────────────────────────────────────────────
echo "== bad requests"
GOOD=22222222-2222-4222-8222-222222222222
echo "unknown attribute (kept, stripped): $(post "$(request "$(base $GOOD ',{"key":"cx.query","value":{"stringValue":"SECRET-QUERY"}}')" "$now" $INSTALL)")"
echo "bad enum:          $(post "$(request "$(base 33333333-3333-4333-8333-333333333333 ',{"key":"cx.outcome","value":{"stringValue":"pwned"}}')" "$now" $INSTALL)")"
echo "stale timestamp:   $(post "$(request "$(base 44444444-4444-4444-8444-444444444444 '')" "$((now - 8 * 86400 * 1000000000))" $INSTALL)")"
echo "wrong type:        $(post "$(request "$(base 55555555-5555-4555-8555-555555555555 ',{"key":"cx.duration_ms","value":{"stringValue":"12"}}')" "$now" $INSTALL)")"
echo "bad install id:    $(post "$(request "$(base 66666666-6666-4666-8666-666666666666 '')" "$now" not-a-uuid)")"
flags31=$(python3 -c 'print(",".join("--f%d" % i for i in range(31)))')
extra=",{\"key\":\"cx.command.flags\",\"value\":{\"stringValue\":\"$flags31\"}}"
echo "31 flags:          $(post "$(request "$(base 88888888-8888-4888-8888-888888888888 "$extra")" "$now" $INSTALL)")"
big=$(python3 -c 'print("x" * 70000)')
extra=",{\"key\":\"cx.query\",\"value\":{\"stringValue\":\"$big\"}}"
echo "body over 64 KiB:  $(post "$(request "$(base 77777777-7777-4777-8777-777777777777 "$extra")" "$now" $INSTALL)")"

# ── 4. Per-IP rate limit ──────────────────────────────────────────────────────
# 10 requests/s with burst 300 per x-envoy-external-address. One process on one
# connection sends 400 requests in well under a second.
echo "== burst"
request "$(base aaaaaaaa-0000-4000-8000-000000000000 '')" "$now" $INSTALL >"$WORK/burst.json"
python3 - "$PORT" "$WORK/burst.json" "$WORK/burst-accepted" <<'EOF'
import http.client, sys
from collections import Counter
body = open(sys.argv[2]).read()
conn = http.client.HTTPConnection("localhost", int(sys.argv[1]))
def burst(count, ip):
    codes = Counter()
    for _ in range(count):
        conn.request("POST", "/v1/logs", body, {"Content-Type": "application/json", "x-envoy-external-address": ip})
        response = conn.getresponse()
        response.read()
        codes[response.status] += 1
    return codes
first = burst(400, "203.0.113.7")
other = burst(5, "198.51.100.9")
print("400 from 203.0.113.7:", dict(first))
print("5 from 198.51.100.9: ", dict(other))
assert first[200] >= 300 and first[429] >= 1 and first[200] + first[429] == 400, first
assert other == {200: 5}, "a limited IP must not limit another one"
open(sys.argv[3], "w").write(str(first[200] + other[200]))
EOF
sleep 2

# ── Assertions over what the collector exported ───────────────────────────────
echo "== exported"
python3 - "$OUT" "$GOOD" "$WORK/expected.jsonl" "$(cat "$WORK/burst-accepted")" <<'EOF'
import json, sys
from collections import Counter
path, good, expected_path, burst_accepted = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
RESOURCE_KEYS = {"service.name", "service.version", "os.type", "host.arch", "cx.install.id", "cx.install.method"}
HAND_MADE = ("2222", "3333", "4444", "5555", "6666", "7777", "8888", "9999", "aaaa")

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
exported = [a for _, a, _ in records if not a.get("cx.invocation.id", "").startswith(HAND_MADE)]
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
teams = Counter(a.get("cx.team.id") for a in exported)
assert teams["4015532"] >= 1 and teams[None] >= 1 and len(teams) == 2, f"real events with and without a team id: {teams}"

# Hand-made: each event kind once with a team id and once without.
made = [a for _, a, _ in records if a.get("cx.invocation.id", "").startswith("9999")]
with_team = sorted(a["event.name"] for a in made if a.get("cx.team.id") == "4015532")
without = sorted(a["event.name"] for a in made if "cx.team.id" not in a)
assert with_team == without == sorted(want), f"with: {with_team}, without: {without}"
assert len(made) == 16, "the bad team id must be dropped"
print("every event kind exported with and without cx.team.id")

bad = [a for _, a, _ in records if a.get("cx.invocation.id", "").startswith(("3333", "4444", "5555", "6666", "7777", "8888"))]
assert not bad, f"bad records exported: {bad}"
kept = [a for _, a, _ in records if a.get("cx.invocation.id") == good]
assert len(kept) == 1 and "cx.query" not in kept[0], kept
print("bad requests: none exported; unknown attribute stripped")

burst = [a for _, a, _ in records if a.get("cx.invocation.id", "").startswith("aaaa")]
assert len(burst) == burst_accepted, f"a rejected request must export nothing: {len(burst)} vs {burst_accepted}"
print(f"burst: {burst_accepted} requests accepted and exported, the rest rejected with 429")
print("OK")
EOF
