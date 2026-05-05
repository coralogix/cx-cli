# Plan: Fix E2E Test Suite to Pass in CI

| Field | Value |
|-------|-------|
| Status | draft |
| Created | 2026-05-05 |
| Ticket | N/A |
| Branch | fix/e2e-test-suite |

## Context

PR #51 ("Add all cx APIs to CLI") added 29 new E2E test files but the CI API key (`CX_E2E_API_KEY`) lacks scopes for the new APIs. All 51 new tests fail with "Permission denied" in CI while passing locally with a broader key. PR #51 also introduced `run_tolerant` / `run_tolerant_json` bypass helpers that silently skip tests on auth errors - masking failures instead of surfacing them. The goal is to restore the pre-PR#51 contract: all E2E tests use strict helpers, all must pass, no silent skips.

## Architecture Decisions

- **Strict-only harness:** Remove `run_tolerant` and `run_tolerant_json` entirely. Every test uses `run_ok` / `run_ok_json`. If a test can't pass, it should fail loudly - not skip silently.
- **Read-only safety:** All E2E tests are already read-only (list, get, search, status, settings, types, limits). The two dashboard delete tests target nonexistent IDs. No production data risk.
- **CI API key:** The key must be updated manually (GitHub Secret) to have read scopes for all tested APIs. This is a manual step outside the code plan.
- **E2E as post-merge canary:** Keep the existing model (runs on push to master, not a required PR check) but add failure notifications so regressions are caught immediately.

## Milestones Overview

1. **Remove bypass mechanisms** - Eliminate `run_tolerant` from harness and all test files, restoring strict-only test contract
2. **Fix failing tests** - Fix the 2 tests that fail even with correct permissions (`olly::olly_ask_text_output`, `users::users_search`)
3. **Add CI failure visibility** - Ensure E2E failures are noticed by adding Slack notification on failure

---

## Milestone 1: Remove Bypass Mechanisms

**Why this matters:** The `run_tolerant` mechanism introduced in PR #51 silently skips tests when the API key lacks permissions. This masks real failures - the CI has been red for 2 days with nobody noticing because some tests "pass" by skipping. Restoring strict-only helpers means a failure is always visible.

**Success criteria:** The harness contains only `run_ok`, `run_ok_nonempty`, and `run_ok_json`. No test silently skips on auth/API errors. Running with an insufficient key produces hard failures, not silent skips.

**Key decisions:** We remove `run_tolerant` entirely rather than keeping it for "known flaky" endpoints. If an endpoint is flaky, the test should fail and we fix the flakiness - not hide it.

### Before/After

Currently 3 tests (`incidents_list`, `quota_rules_get`, `saml_get`, `saml_sp_params`) use `run_tolerant` which returns `None` on auth errors, silently passing. After this milestone, all tests use `run_ok_json` and fail hard on any error.

### 1.1 [ ] Remove `run_tolerant` and `run_tolerant_json` from harness

- **Files:** `tests/e2e/harness.rs`
- **What:** Delete the `run_tolerant` function (lines 148-183) and `run_tolerant_json` function (lines 187-200) from the harness. These are the bypass mechanisms that allow tests to silently skip on auth/API errors.
- **Acceptance:** `cargo test --test e2e -- --ignored --test-threads=1` compiles. `grep -r "run_tolerant" tests/e2e/` returns nothing.

### 1.2 [ ] Convert `incidents.rs` from tolerant to strict

- **Files:** `tests/e2e/incidents.rs`
- **What:** Replace `run_tolerant_json` with `run_ok_json`. The test currently does `let _v = run_tolerant_json(...)` and returns early on `None`. Change to use `run_ok_json` like all other test files. Follow the same pattern as other list tests (e.g., `slos.rs`, `enrichments.rs`). The test should call `run_ok_json(&["incidents", "list", "-o", "json"])` and assert the result is a JSON array using `assert_array`.
- **Acceptance:** Test compiles and passes locally with valid credentials.
- **Dependencies:** 1.1

### 1.3 [ ] Convert `quota_rules.rs` from tolerant to strict

- **Files:** `tests/e2e/quota_rules.rs`
- **What:** Replace `run_tolerant_json` with `run_ok_json`. Currently does `let _v = run_tolerant_json(&["quotas", "get", "-o", "json"], ...)`. Change to `let v = run_ok_json(&["quotas", "get", "-o", "json"])` and add a shape assertion (the quotas get response is a JSON object).
- **Acceptance:** Test compiles and passes locally with valid credentials.
- **Dependencies:** 1.1

### 1.4 [ ] Convert `saml.rs` from tolerant to strict

- **Files:** `tests/e2e/saml.rs`
- **What:** Replace both `run_tolerant_json` calls with `run_ok_json`. The `saml_get` test should use `run_ok_json(&["iam", "saml", "get", "-o", "json"])` and the `saml_sp_params` test should use `run_ok_json(&["iam", "saml", "sp-params", "-o", "json"])`. Add shape assertions for both responses.
- **Acceptance:** Tests compile and pass locally with valid credentials.
- **Dependencies:** 1.1

### 1.5 [ ] Verify full E2E suite passes locally

- **Files:** None (verification only)
- **What:** Run `cargo test --test e2e -- --ignored --test-threads=1` with valid credentials and confirm all 83 tests pass (except the 2 known failures addressed in Milestone 2). Run `cargo clippy` and `cargo fmt --check` to ensure no warnings.
- **Acceptance:** 81+ tests pass, 0 silently skipped, `cargo clippy` clean.
- **Dependencies:** 1.2, 1.3, 1.4

---

## Milestone 2: Fix Failing Tests

**Why this matters:** Two tests fail even with correct API key permissions. These must be fixed so the full suite is green when the CI key is updated.

**Success criteria:** All 83 E2E tests pass locally with valid credentials. Zero failures, zero silent skips.

**Key decisions:** The `olly_ask_text_output` test failure is a rendering bug in the olly command (outputs TOON instead of text). The `users_search` test failure is because the users search command triggers a SAML team-ID resolution that requires a scope the key may not have - the command should handle this gracefully or the test should be adjusted.

### Before/After

Currently `olly_ask_text_output` fails because text output produces TOON format instead of human-readable text with "Chat ID:" prefix. `users_search` fails because stderr contains an auth error from SAML resolution even though the command partially succeeds. After this milestone both tests pass.

### 2.1 [ ] Fix `olly_ask_text_output` test or rendering

- **Files:** `tests/e2e/olly/mod.rs`, possibly `src/commands/olly/mod.rs`
- **What:** Investigate why `cx olly ask "Reply with OK"` outputs TOON-encoded format (`[1]{fields}: values`) instead of human-readable text with "Chat ID:" prefix. Either: (a) fix the text rendering in the olly command to produce the expected format, or (b) update the test assertion to match the actual (correct) output format. Check what `-o text` vs default output looks like and whether the test specifies an output format. The test at line 37 asserts `stdout contains "Chat ID:"`.
- **Acceptance:** `cargo test --test e2e olly::olly_ask_text_output -- --ignored` passes.
- **Dependencies:** None

### 2.2 [ ] Fix `users_search` test

- **Files:** `tests/e2e/users.rs`, possibly `src/commands/iam/mod.rs`
- **What:** The `users_search` test runs `cx iam users search -o json` which returns `[]` on stdout but also prints an auth error on stderr from a SAML team-ID resolution step. The harness `run_ok` catches this auth error and panics. Investigate: (a) why does `users search` trigger SAML resolution? (b) can the SAML resolution failure be made non-fatal for this command? (c) if the SAML scope is genuinely needed, can the error be suppressed from stderr when it's a secondary lookup? The fix should ensure the command succeeds cleanly without auth errors on stderr, OR the test should be restructured to account for this behavior.
- **Acceptance:** `cargo test --test e2e users::users_search -- --ignored` passes.
- **Dependencies:** None

---

## Milestone 3: Add CI Failure Visibility

**Why this matters:** The E2E canary has been red since May 3 (2+ days) with no one noticing. The workflow runs on push to master as a post-merge canary, but failures are invisible unless someone checks the Actions tab. Adding a notification ensures failures get immediate attention.

**Success criteria:** When E2E fails on master, a Slack message is sent to the team channel so the failure is noticed within minutes, not days.

**Key decisions:** Use a simple Slack webhook notification step in the existing `e2e.yml` workflow rather than adding E2E as a required PR check (which would create a hard dependency on external infrastructure for all merges).

### Before/After

Currently E2E failures are silent - they only appear in the GitHub Actions tab. After this milestone, failures trigger a Slack notification to the team channel.

### 3.1 [ ] Add Slack notification on E2E failure

- **Files:** `.github/workflows/e2e.yml`
- **What:** Add a step at the end of the `e2e` job that runs `if: failure()` and sends a Slack notification via the `slackapi/slack-github-action` (or a simple `curl` to a webhook URL). The notification should include: workflow name, commit SHA, link to the failed run, and the branch. The Slack webhook URL should be stored as a GitHub Secret (`SLACK_E2E_WEBHOOK_URL`). If no secret is configured, the step should be skipped gracefully (use `if: failure() && secrets.SLACK_E2E_WEBHOOK_URL != ''`). Also add a comment in the workflow explaining why the notification exists.
- **Acceptance:** The workflow YAML is valid. The notification step is conditional on failure and on the secret existing. `act` or manual inspection confirms the YAML is syntactically correct.
- **Dependencies:** None

### 3.2 [ ] Document E2E CI requirements

- **Files:** `contributing/development.md`
- **What:** Add a section documenting: (1) the E2E CI workflow runs on push to master as a post-merge canary, (2) the `CX_E2E_API_KEY` secret must have read scopes for ALL APIs tested in the E2E suite, (3) when adding new E2E tests for new commands, verify the CI key has the required scopes, (4) all tests must use `run_ok` / `run_ok_json` - never `run_tolerant` (which was removed). (5) list of scopes the CI key needs. This prevents future PRs from reintroducing the same problem.
- **Acceptance:** Documentation is clear and actionable. Mentions the key update requirement.
- **Dependencies:** None
