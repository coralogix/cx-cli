#!/usr/bin/env bash
# Decides whether a release must pass the collector contract check
# (contributing/telemetry-design-draft.md § 5.2).
#
# A release sends telemetry when BUILD_ENDPOINT in src/telemetry/consent.rs is
# anything but `None`. Such a release must run the check, so this fails when
# the collector repo or config path is not configured. Prints `check=true` or
# `check=false` (for $GITHUB_OUTPUT).
#
# Usage: telemetry-release-gate.sh [path/to/consent.rs]
# Env: COLLECTOR_REPO, COLLECTOR_CONFIG_PATH
set -euo pipefail

SOURCE=${1:-src/telemetry/consent.rs}
# Anything but this exact line counts as "sends": a renamed or moved constant
# must not switch the check off.
if grep -qx 'const BUILD_ENDPOINT: Option<&str> = None;' "$SOURCE"; then
  sends=false
else
  sends=true
fi

if [ "$sends" = true ] && { [ -z "${COLLECTOR_REPO:-}" ] || [ -z "${COLLECTOR_CONFIG_PATH:-}" ]; }; then
  echo "::error::This release has a telemetry endpoint ($SOURCE), but TELEMETRY_COLLECTOR_REPO or TELEMETRY_COLLECTOR_CONFIG_PATH is not set. A release that sends telemetry must pass the collector contract check." >&2
  exit 1
fi

if [ "$sends" = true ] || [ -n "${COLLECTOR_REPO:-}" ]; then
  echo "check=true"
else
  echo "check=false"
fi
