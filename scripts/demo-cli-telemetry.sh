#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
cargo test --locked -p cli-otel --all-features
cargo test --locked --test telemetry --test telemetry_delivery --test telemetry_failure_logs \
  --test telemetry_official_exporter
