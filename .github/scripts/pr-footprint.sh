#!/usr/bin/env bash
# Classify every file a PR changes so a reviewer (human or agent) can tell
# at a glance which files belong to the PR's stated purpose and which need
# a justification.
#
# Usage: pr-footprint.sh <base-ref> <head-ref> [repo-dir]
#
# Output: a TSV table on stdout, one row per changed file, then a summary
# block on stderr. Columns:
#
#   path         the changed file
#   domain       where it lives in the cx layout (see classify_domain)
#   footprint    yes    = expected for the commands this PR touches
#                judge  = outside the expected footprint; the PR text
#                         must explain it, or it is a scope finding
#                sensitive = like judge, but on a security / governance /
#                         release surface, or the added lines match a
#                         security pattern; unexplained changes are blockers
#   note         ws-only       the diff is whitespace only
#                generated     a synced copy of skills/shared/<file>
#                tests-shrunk  a test file lost more lines than it gained
#                version-bump  the version line in Cargo.toml changed
#                secret-like   an added line looks like a real token or key
#                pipe-to-shell an added line pipes a download into a shell,
#                              other than the official cx installer
#                safety-bypass an added line turns off read-only mode, or a
#                              shipped skill gains a --yes (skips confirmation)
#                new-crate     Cargo.lock gains a package (supply chain)
#   +/-          added / deleted lines
#
# The script never reads the PR description. Judging whether a "judge"
# row is justified is the reviewer's job; this script only makes the
# footprint mechanical and repeatable.

set -euo pipefail

BASE="${1:?usage: pr-footprint.sh <base-ref> <head-ref> [repo-dir]}"
HEAD="${2:?usage: pr-footprint.sh <base-ref> <head-ref> [repo-dir]}"
REPO_DIR="${3:-.}"

cd "$REPO_DIR"
MERGE_BASE="$(git merge-base "$BASE" "$HEAD")"

# Core files: shared infrastructure every command depends on.
CORE_RE='^src/(main|safety|config|api_client|execution|render|spill|error|time|tier|lib)\.rs$'
# Governance / CI / release surfaces: unexplained edits are blockers.
SENSITIVE_RE='^(\.github/|CODEOWNERS$|deny\.toml$|rust-toolchain\.toml$|flake\.(nix|lock)$|install\.sh$|homebrew/|SECURITY\.md$|LICENSE$)'
# Security surfaces, checked before any other rule so no later rule can
# downgrade them:
#   - credentials, auth, and where the key is sent: safety, config, api_client,
#     execution, oauth, keyring_store, macos_security, code_sign, identity,
#     region, onboarding
#   - what cx downloads or tells the user to run: update_check, install_method
#   - code that runs at build time: build.rs, .cargo/
#   - the e2e write guard and the sweeper that deletes test-team resources
#   - agent instructions and settings that run on contributors' machines:
#     .claude/, .codex/, AGENTS.md, scripts/
SECURITY_RE='^(src/(safety|config|api_client|execution|oauth|keyring_store|macos_security|code_sign|identity|region|onboarding|update_check|install_method)\.rs$|build\.rs$|\.cargo/|tests/e2e/(harness|sweeper)\.rs$|tests/e2e/common/|\.claude/|\.codex/|AGENTS\.md$|scripts/|\.env\.example$)'

# Patterns on added lines. A match makes the row sensitive whatever the path.
SECRET_RE='(cxtp|cxup)_[A-Za-z0-9]{16,}|BEGIN [A-Z ]*PRIVATE KEY|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36}|sk-ant-[A-Za-z0-9_-]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}'
PIPE_SHELL_RE='(curl|wget)[^|]*\|[[:space:]]*(sudo[[:space:]]+)?(ba|z|da)?sh'
# The official installer is the one pipe-to-shell the docs may show.
OFFICIAL_INSTALL_RE='https://get\.coralogix\.dev/cli|https://raw\.githubusercontent\.com/coralogix/cx-cli/master/install\.sh'

# tests/<name>/ is a command's integration test only when src/commands/<name>/
# exists in the head tree. Otherwise it is a cross-cutting test (read_only,
# agent_mode, help_redaction, ...) and belongs to no single command.
is_command() {
    git cat-file -e "$HEAD:src/commands/$1/mod.rs" 2>/dev/null
}

cmd_or_crosscutting() {
    if is_command "$1"; then echo "cmd:$1"; else echo "tests-crosscutting"; fi
}

classify_domain() {
    local p="$1"
    if [[ "$p" =~ $SECURITY_RE || "$p" =~ $SENSITIVE_RE ]]; then
        echo "security"; return
    fi
    case "$p" in
        src/commands/mod.rs)            echo "wiring" ;;
        src/main.rs)                    echo "wiring" ;;
        tests/e2e.rs)                   echo "wiring" ;;
        tests/schema/*)                 echo "wiring" ;;
        src/commands/*/*)               echo "cmd:$(echo "$p" | cut -d/ -f3)" ;;
        tests/e2e/common/*)             echo "test-support" ;;
        tests/common/*)                 echo "test-support" ;;
        tests/e2e/*/*)                  cmd_or_crosscutting "$(echo "$p" | cut -d/ -f3)" ;;
        tests/e2e/*.rs)                 cmd_or_crosscutting "$(basename "$p" .rs)" ;;
        tests/*/*)                      cmd_or_crosscutting "$(echo "$p" | cut -d/ -f2)" ;;
        tests/*.rs)                     cmd_or_crosscutting "$(basename "$p" .rs)" ;;
        skills/shared/*)                echo "skills-shared" ;;
        skills/README.md)               echo "catalog" ;;
        skills/*/*)                     echo "skill:$(echo "$p" | cut -d/ -f2)" ;;
        CLAUDE.md)                      echo "catalog" ;;
        README.md|docs/*|contributing/*|CONTRIBUTING.md) echo "docs" ;;
        Cargo.toml|Cargo.lock)          echo "deps" ;;
        assets/*)                       echo "dataprime-docs" ;;
        *)
            if [[ "$p" =~ $CORE_RE ]]; then echo "core"
            else echo "other"
            fi ;;
    esac
}

# --- collect numstat for the full diff and for the whitespace-insensitive diff
numstat="$(git diff --no-renames --numstat "$MERGE_BASE" "$HEAD" --)"
numstat_w="$(git diff --no-renames -w --numstat "$MERGE_BASE" "$HEAD" --)"

if [[ -z "$numstat" ]]; then
    echo "no changes between $BASE and $HEAD" >&2
    exit 0
fi

# Files whose -w diff is empty or 0/0 changed only whitespace.
ws_only_files="$(
    while IFS=$'\t' read -r a d p; do
        [[ -z "$p" ]] && continue
        w_line="$(printf '%s\n' "$numstat_w" | awk -F'\t' -v f="$p" '$3==f')"
        if [[ -z "$w_line" ]]; then echo "$p"; continue; fi
        wa="$(printf '%s' "$w_line" | cut -f1)"; wd="$(printf '%s' "$w_line" | cut -f2)"
        if [[ "$wa" == "0" && "$wd" == "0" ]]; then echo "$p"; fi
    done <<< "$numstat"
)"

# Shared reference files changed in this PR (for the "generated" note).
shared_changed="$(printf '%s\n' "$numstat" | cut -f3 | grep '^skills/shared/' | xargs -n1 basename 2>/dev/null || true)"

# Did the version line of Cargo.toml change?
version_bump=false
if git diff --no-renames "$MERGE_BASE" "$HEAD" -- Cargo.toml | grep -qE '^[+-]version = "'; then
    version_bump=true
fi

# Security patterns in the lines this file adds. Prints one note per match.
content_notes() {
    local p="$1" added
    added="$(git diff --no-renames -U0 "$MERGE_BASE" "$HEAD" -- "$p" | grep '^+' | grep -v '^+++' || true)"
    [[ -z "$added" ]] && return 0
    if grep -qE "$SECRET_RE" <<< "$added"; then echo "secret-like"; fi
    if grep -E "$PIPE_SHELL_RE" <<< "$added" | grep -vqE "$OFFICIAL_INSTALL_RE"; then echo "pipe-to-shell"; fi
    if grep -qE 'CX_READ_ONLY=(false|0)' <<< "$added" \
        || { [[ "$p" == skills/* ]] && grep -qE -- '--yes' <<< "$added"; }; then
        echo "safety-bypass"
    fi
    if [[ "$p" == "Cargo.lock" ]] && grep -qE '^\+name = ' <<< "$added"; then echo "new-crate"; fi
    return 0
}

# --- first pass: which command domains does this PR touch?
# bash 3.2 (macOS) has no associative arrays, so keep a space-separated list.
primary_cmds=""
while IFS=$'\t' read -r a d p; do
    [[ -z "$p" ]] && continue
    dom="$(classify_domain "$p")"
    if [[ "$dom" == cmd:* ]]; then
        c="${dom#cmd:}"
        case " $primary_cmds " in *" $c "*) ;; *) primary_cmds="${primary_cmds:+$primary_cmds }$c" ;; esac
    fi
done <<< "$numstat"
primary_count="$(wc -w <<< "$primary_cmds" | tr -d ' ')"

# --- second pass: emit rows
printf 'path\tdomain\tfootprint\tnote\t+\t-\n'
rows=0; judge=0; sensitive=0
while IFS=$'\t' read -r adds dels p; do
    [[ -z "$p" ]] && continue
    rows=$((rows + 1))
    dom="$(classify_domain "$p")"
    fp="judge"
    notes=()

    case "$dom" in
        cmd:*)
            # Expected if this is one of the commands the PR is about. With
            # several commands touched, every one of them counts as primary;
            # the reviewer decides whether touching several was justified.
            fp="yes" ;;
        wiring|catalog)
            # main.rs, commands/mod.rs, e2e.rs, schema tests, CLAUDE.md,
            # skills/README.md: expected whenever a command is added or changed.
            if (( primary_count > 0 )); then fp="yes"; else fp="judge"; fi ;;
        skill:*)
            # A command change is expected to ship with its skill.
            if (( primary_count > 0 )); then fp="yes"; else fp="judge"; fi ;;
        docs|dataprime-docs|test-support|tests-crosscutting|skills-shared|deps|core|other)
            fp="judge" ;;
        security)
            fp="sensitive" ;;
    esac

    # Notes
    if printf '%s\n' "$ws_only_files" | grep -qxF "$p"; then notes+=("ws-only"); fi
    if [[ "$p" =~ ^skills/[^/]+/references/([^/]+)$ ]]; then
        ref="${BASH_REMATCH[1]}"
        if git cat-file -e "$HEAD:skills/shared/$ref" 2>/dev/null && printf '%s\n' "$shared_changed" | grep -qxF "$ref"; then
            notes+=("generated"); fp="yes"
        fi
    fi
    if [[ "$p" == tests/* && "$adds" != "-" && "$dels" != "-" ]] && (( dels > adds )); then
        notes+=("tests-shrunk")
    fi
    if [[ "$p" == "Cargo.toml" && "$version_bump" == true ]]; then
        notes+=("version-bump"); fp="sensitive"
    fi
    # Last, so no rule above can turn a security match back into yes/judge.
    while read -r n; do
        [[ -n "$n" ]] && { notes+=("$n"); fp="sensitive"; }
    done < <(content_notes "$p")

    [[ "$fp" == "judge" ]] && judge=$((judge + 1))
    [[ "$fp" == "sensitive" ]] && sensitive=$((sensitive + 1))

    note_str="$(IFS=,; echo "${notes[*]:-}")"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$p" "$dom" "$fp" "${note_str:--}" "$adds" "$dels"
done <<< "$numstat"

{
    echo "---"
    echo "merge-base: $MERGE_BASE"
    if (( primary_count > 0 )); then
        echo "commands touched: $primary_cmds"
    else
        echo "commands touched: none"
    fi
    echo "files: $rows   judge: $judge   sensitive: $sensitive"
    if [[ "$version_bump" == true ]]; then
        echo "WARNING: Cargo.toml version line changed in this PR"
    fi
    # Every change to a command must come with its e2e test. The e2e module
    # name usually equals the command directory; when it does not (api_keys,
    # data_usage, data_archive, ...) the reviewer greps tests/e2e by hand.
    if (( primary_count > 0 )); then
        e2e_yes=""; e2e_no=""
        for c in $primary_cmds; do
            if printf '%s\n' "$numstat" | cut -f3 | grep -qE "^tests/e2e/$c(\.rs|/)"; then
                e2e_yes="${e2e_yes:+$e2e_yes }$c"
            else
                e2e_no="${e2e_no:+$e2e_no }$c"
            fi
        done
        echo "e2e updated for: ${e2e_yes:-none}"
        echo "e2e NOT updated for: ${e2e_no:-none}"
    fi
} >&2
exit 0
