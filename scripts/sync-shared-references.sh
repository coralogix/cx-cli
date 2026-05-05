#!/usr/bin/env bash
set -euo pipefail

# Copies shared reference files from skills/shared/ into each consuming skill's
# references/ directory. Run this after editing any file in skills/shared/.
#
# Usage: scripts/sync-shared-references.sh

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SHARED_DIR="$REPO_ROOT/skills/shared"
SKILLS_DIR="$REPO_ROOT/skills"

COPIED=0
ERRORS=0

copy_refs() {
    local skill="$1"
    shift
    local files=("$@")

    local skill_dir="$SKILLS_DIR/$skill"
    local refs_dir="$skill_dir/references"

    if [ ! -d "$skill_dir" ]; then
        echo "ERROR: skill directory not found: $skill_dir"
        ERRORS=$((ERRORS + 1))
        return
    fi

    mkdir -p "$refs_dir"

    for ref_file in "${files[@]}"; do
        local src="$SHARED_DIR/$ref_file"
        local dst="$refs_dir/$ref_file"

        if [ ! -f "$src" ]; then
            echo "ERROR: shared file not found: $src"
            ERRORS=$((ERRORS + 1))
            continue
        fi

        cp "$src" "$dst"
        echo "  copied $ref_file -> $skill/references/"
        COPIED=$((COPIED + 1))
    done
}

# cx-telemetry-querying: all shared references
copy_refs "cx-telemetry-querying" \
    "dataprime-reference.md" \
    "promql-guidelines.md" \
    "logs-querying.md" \
    "spans-querying.md" \
    "metrics-querying.md" \
    "rum-querying.md" \
    "rum-fields.md"

# cx-create-dashboard: query language and data-source references
copy_refs "cx-create-dashboard" \
    "dataprime-reference.md" \
    "promql-guidelines.md" \
    "logs-querying.md" \
    "spans-querying.md"

# cx-alerts: DataPrime syntax and log querying for alert conditions
copy_refs "cx-alerts" \
    "dataprime-reference.md" \
    "logs-querying.md"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Synced $COPIED file(s)"

if [ "$ERRORS" -gt 0 ]; then
    echo "FAILED: $ERRORS error(s)"
    exit 1
fi
