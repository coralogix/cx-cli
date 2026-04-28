#!/usr/bin/env bash
set -euo pipefail

# Verify all skills in skills/ for correctness.
# Checks: frontmatter fields, trigger phrase count, command validation,
# cross-reference validation, and line count.

SKILLS_DIR="$(cd "$(dirname "$0")/../skills" && pwd)"
ERRORS=0
PASS=0
TOTAL=0

red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }

fail() {
    echo "  $(red FAIL): $1"
    ERRORS=$((ERRORS + 1))
}

# Cache cx schema in a temp file
SCHEMA_FILE=$(mktemp)
trap 'rm -f "$SCHEMA_FILE"' EXIT

SCHEMA_AVAILABLE=false
if command -v cx &>/dev/null && cx schema > "$SCHEMA_FILE" 2>/dev/null; then
    if [ -s "$SCHEMA_FILE" ]; then
        SCHEMA_AVAILABLE=true
    fi
fi

if ! $SCHEMA_AVAILABLE; then
    echo "WARNING: cx schema unavailable, skipping command validation"
fi

for skill_dir in "$SKILLS_DIR"/*/; do
    [ -d "$skill_dir" ] || continue
    skill_file="$skill_dir/SKILL.md"
    skill_name=$(basename "$skill_dir")

    [ -f "$skill_file" ] || continue
    TOTAL=$((TOTAL + 1))
    skill_errors=0

    echo "Checking: $skill_name"

    # 1. Frontmatter check
    if head -1 "$skill_file" | grep -q '^---'; then
        frontmatter=$(awk '/^---$/{n++; next} n==1{print} n>=2{exit}' "$skill_file")

        if ! echo "$frontmatter" | grep -q '^name:'; then
            fail "missing 'name' in frontmatter"
            skill_errors=$((skill_errors + 1))
        fi
        if ! echo "$frontmatter" | grep -q 'description'; then
            fail "missing 'description' in frontmatter"
            skill_errors=$((skill_errors + 1))
        fi
        if ! echo "$frontmatter" | grep -q '^version:'; then
            echo "  WARN: missing 'version' in frontmatter"
        fi
    else
        fail "no YAML frontmatter found"
        skill_errors=$((skill_errors + 1))
    fi

    # 2. Trigger phrase count (quoted strings in description)
    # Some skills use quoted trigger phrases, others use prose descriptions
    desc_block=$(awk '/^description/,/^(version|---)/' "$skill_file" | sed '$d')
    phrase_count=$(echo "$desc_block" | { grep -o '"[^"]*"' || true; } | wc -l | tr -d ' ')
    desc_length=$(echo "$desc_block" | wc -c | tr -d ' ' || echo 0)
    if [ "$phrase_count" -lt 10 ] && [ "$desc_length" -lt 100 ]; then
        fail "description too short ($desc_length chars) and only $phrase_count trigger phrases (need ≥10 phrases or ≥100 char description)"
        skill_errors=$((skill_errors + 1))
    fi

    # 3. Command validation (top-level command only)
    if $SCHEMA_AVAILABLE; then
        top_cmds=$(grep -oE 'cx [a-z][-a-z0-9]+' "$skill_file" | \
            awk '{print $2}' | sort -u)

        while IFS= read -r top_cmd; do
            [ -z "$top_cmd" ] && continue
            # Skip common non-command words that follow 'cx'
            case "$top_cmd" in
                schema|profile|profiles) continue ;;
            esac
            if ! grep -q "\"name\": \"$top_cmd\"" "$SCHEMA_FILE"; then
                fail "top-level command '$top_cmd' not found in cx schema"
                skill_errors=$((skill_errors + 1))
            fi
        done <<< "$top_cmds"
    fi

    # 4. Cross-reference validation (skill references in Related Skills sections)
    referenced_skills=$(awk '/[Rr]elated [Ss]kills/,0' "$skill_file" | \
        grep -oE '`[a-z][-a-z0-9]+`' | tr -d '`' | sort -u || true)

    if [ -n "$referenced_skills" ]; then
        while IFS= read -r ref; do
            [ -z "$ref" ] && continue
            # Skip known non-skill references
            case "$ref" in
                cx|json|jq|from-file|o|p) continue ;;
            esac
            if [ ! -d "$SKILLS_DIR/$ref" ]; then
                fail "referenced skill '$ref' not found in skills/"
                skill_errors=$((skill_errors + 1))
            fi
        done <<< "$referenced_skills"
    fi

    # 5. Line count check
    lines=$(wc -l < "$skill_file" | tr -d ' ')
    if [ "$lines" -gt 400 ]; then
        fail "SKILL.md is $lines lines (max 400)"
        skill_errors=$((skill_errors + 1))
    fi

    if [ "$skill_errors" -eq 0 ]; then
        echo "  $(green PASS) ($phrase_count triggers, $lines lines)"
        PASS=$((PASS + 1))
    fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results: $PASS/$TOTAL passed, $ERRORS errors"
if [ "$ERRORS" -gt 0 ]; then
    echo "$(red 'FAILED')"
    exit 1
else
    echo "$(green 'ALL PASSED')"
fi
