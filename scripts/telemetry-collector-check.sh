#!/usr/bin/env bash
# Runs the CLI telemetry collector config (contributing/telemetry-design-draft.md
# § 2, copied to scripts/telemetry-collector/config.yaml) in Docker, with the
# Coralogix exporter swapped for a file (local.yaml). Then:
#   1. a real `cx` binary records events and flushes them to the collector;
#      every one must be exported, with service.name = cx-cli;
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
cx() {
  env -u CX_API_KEY -u CX_REGION -u CX_PROFILE -u CX_TELEMETRY -u DO_NOT_TRACK \
    HOME="$WORK/home" CX_HOME="$WORK/home" CX_NO_UPDATE_NOTIFIER=1 \
    CX_TELEMETRY_ENDPOINT="$ENDPOINT" "$CX" "$@" >/dev/null 2>&1 || true
}
# Local commands only spool. A profile whose API is unreachable still builds
# execution targets, so `alerts list` fails but flushes everything.
cx dataprime list --name SENTINEL_SECRET
cx schema
cx alertz list
mkdir -p "$WORK/home/.cx/profiles"
printf 'auth = "api_key"\ncredential_storage = "file"\napi_key = "k"\nregion = "http://127.0.0.1:1"\n' \
  >"$WORK/home/.cx/profiles/check.toml"
printf 'default_profile = "check"\n' >"$WORK/home/.cx/config.toml"
cx alerts list
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
big=$(python3 -c 'print("x" * 70000)')
echo "body over 64 KiB:  $(post "$(request "$(base 77777777-7777-4777-8777-777777777777 ",{\"key\":\"cx.query\",\"value\":{\"stringValue\":\"$big\"}}")" "$now" $INSTALL)")"
sleep 2

# ── Assertions over what the collector exported ───────────────────────────────
echo "== exported"
python3 - "$OUT" "$GOOD" <<'EOF'
import json, sys
path, good = sys.argv[1], sys.argv[2]
records = []
for line in open(path):
    for rl in json.loads(line)["resourceLogs"]:
        res = {a["key"]: a["value"] for a in rl["resource"]["attributes"]}
        for sl in rl["scopeLogs"]:
            for r in sl["logRecords"]:
                attrs = {a["key"]: next(iter(a["value"].values())) for a in r.get("attributes", [])}
                records.append((res, attrs, r))

cli = [a for _, a, _ in records if a.get("cx.invocation.id") != good]
names = [a["event.name"] for a in cli]
print(f"cx events exported: {names}")
for res, attrs, r in records:
    assert res["service.name"]["stringValue"] == "cx-cli", res
    assert r.get("body", {}).get("stringValue", "") == "", r
    assert "SENTINEL_SECRET" not in json.dumps(attrs)
assert sorted(names) == sorted(["cli.first_run", "cli.command", "cli.command", "cli.parse_error", "cli.command"]), names
cmd = {a["cx.command.path"]: a for a in cli if a["event.name"] == "cli.command"}
assert set(cmd) == {"dataprime list", "schema", "alerts list"}, cmd
print("alerts list:", {k: v for k, v in cmd["alerts list"].items() if k.startswith("cx.") and k != "cx.invocation.id"})

bad = [a for _, a, _ in records if a.get("cx.invocation.id", "").startswith(("3333", "4444", "5555", "6666", "7777"))]
assert not bad, f"bad records exported: {bad}"
kept = [a for _, a, _ in records if a.get("cx.invocation.id") == good]
assert len(kept) == 1 and "cx.query" not in kept[0], kept
print("bad requests: none exported; unknown attribute stripped")
print("OK")
EOF
