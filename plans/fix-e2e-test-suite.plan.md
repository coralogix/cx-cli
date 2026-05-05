# Plan: Fix E2E Test Suite to Pass in CI

| Field | Value |
|-------|-------|
| Status | draft |
| Created | 2026-05-05 |
| Ticket | N/A |
| Branch | fix/e2e-test-suite |

## Context

PR #51 ("Add all cx APIs to CLI") added 29 new E2E test files but the CI API key (`CX_E2E_API_KEY`) lacked scopes for the new APIs - this has now been fixed (key updated 2026-05-05). PR #51 also introduced `run_tolerant` / `run_tolerant_json` bypass helpers that silently skip tests on auth errors - masking failures instead of surfacing them. The goal is to restore the pre-PR#51 contract: all E2E tests use strict helpers, all must pass, no silent skips.

## Architecture Decisions

- **Strict-only harness:** Remove `run_tolerant` and `run_tolerant_json` entirely. Every test uses `run_ok` / `run_ok_json`. If a test can't pass, it should fail loudly - not skip silently.
- **Read-only safety:** All E2E tests are already read-only (list, get, search, status, settings, types, limits). The two dashboard delete tests target nonexistent IDs. No production data risk.
- **CI API key:** Already updated (2026-05-05) with read scopes for all tested APIs.

## Milestones Overview

1. **Remove bypass mechanisms** - Eliminate `run_tolerant` from harness and all test files, restoring strict-only test contract
2. **Fix failing tests** - Fix the 2 tests that fail even with correct permissions (`olly::olly_ask_text_output`, `users::users_search`)

---

## Milestone 1: Remove Bypass Mechanisms

**Why this matters:** The `run_tolerant` mechanism introduced in PR #51 silently skips tests when the API key lacks permissions. This masks real failures - the CI has been red for 2 days with nobody noticing because some tests "pass" by skipping. Restoring strict-only helpers means a failure is always visible.

**Success criteria:** The harness contains only `run_ok`, `run_ok_nonempty`, and `run_ok_json`. No test silently skips on auth/API errors. Running with an insufficient key produces hard failures, not silent skips.

**Key decisions:** We remove `run_tolerant` entirely rather than keeping it for "known flaky" endpoints. If an endpoint is flaky, the test should fail and we fix the flakiness - not hide it.

### Before/After

Currently 3 tests (`incidents_list`, `quota_rules_get`, `saml_get`, `saml_sp_params`) use `run_tolerant` which returns `None` on auth errors, silently passing. After this milestone, all tests use `run_ok_json` and fail hard on any error.

### 1.1 [x] Remove `run_tolerant` and `run_tolerant_json` from harness *(completed 2026-05-05)*

- **Files:** `tests/e2e/harness.rs`
- **What:** Delete the `run_tolerant` function (lines 148-183) and `run_tolerant_json` function (lines 187-200) from the harness. These are the bypass mechanisms that allow tests to silently skip on auth/API errors.
- **Acceptance:** `cargo test --test e2e -- --ignored --test-threads=1` compiles. `grep -r "run_tolerant" tests/e2e/` returns nothing.

### 1.2 [x] Convert `incidents.rs` from tolerant to strict *(completed 2026-05-05)*

- **Files:** `tests/e2e/incidents.rs`
- **What:** Replace `run_tolerant_json` with `run_ok_json`. The test currently does `let _v = run_tolerant_json(...)` and returns early on `None`. Change to use `run_ok_json` like all other test files. Follow the same pattern as other list tests (e.g., `slos.rs`, `enrichments.rs`). The test should call `run_ok_json(&["incidents", "list", "-o", "json"])` and assert the result is a JSON array using `assert_array`.
- **Acceptance:** Test compiles and passes locally with valid credentials.
- **Dependencies:** 1.1

### 1.3 [x] Convert `quota_rules.rs` from tolerant to strict *(completed 2026-05-05)*

- **Files:** `tests/e2e/quota_rules.rs`
- **What:** Replace `run_tolerant_json` with `run_ok_json`. Currently does `let _v = run_tolerant_json(&["quotas", "get", "-o", "json"], ...)`. Change to `let v = run_ok_json(&["quotas", "get", "-o", "json"])` and add a shape assertion (the quotas get response is a JSON object).
- **Acceptance:** Test compiles and passes locally with valid credentials.
- **Dependencies:** 1.1

### 1.4 [x] Convert `saml.rs` from tolerant to strict *(completed 2026-05-05)*

- **Files:** `tests/e2e/saml.rs`
- **What:** Replace both `run_tolerant_json` calls with `run_ok_json`. The `saml_get` test should use `run_ok_json(&["iam", "saml", "get", "-o", "json"])` and the `saml_sp_params` test should use `run_ok_json(&["iam", "saml", "sp-params", "-o", "json"])`. Add shape assertions for both responses.
- **Acceptance:** Tests compile and pass locally with valid credentials.
- **Dependencies:** 1.1

### 1.5 [x] Verify the 51 previously-failing tests now pass with the updated API key *(completed 2026-05-05)*

- **Files:** None (verification only)
- **What:** Run the full E2E suite and verify the previously-failing tests.
- **Result:** 73 passed, 10 failed, 0 silently skipped. The bypass removal is working - all failures are now loud. Of the 10 failures:
  - **8 are API key scope issues** (local `.env` key missing scopes): `api_keys::api_keys_get`, `api_keys::api_keys_send_data_keys`, `extensions::extensions_list`, `roles::roles_get`, `saml::saml_get`, `saml::saml_sp_params`, `users::users_search`, `views::views_get`
  - **2 are olly auth failures** (olly API scope missing from key): `olly::olly_ask_basic`, `olly::olly_ask_text_output`
  - All 10 are "Permission denied" - no code bugs beyond the olly/users issues already in Milestone 2.
  - The CI key (updated separately) may already have these scopes. These will be validated when the branch is pushed.
- **Dependencies:** 1.2, 1.3, 1.4

---

## Milestone 2: Fix Failing Tests

**Why this matters:** After removing bypass mechanisms, 10 tests still fail. 8 are purely API key scope issues that will resolve when the CI key has all scopes. 2 have code-level issues that need investigation regardless of key scopes: `users_search` triggers a SAML resolution that leaks auth errors to stderr, and `olly` commands fail because the olly API requires a separate scope. The code fixes ensure the suite is green once the key is correct.

**Success criteria:** All 83 E2E tests pass in CI with the updated API key. Zero failures, zero silent skips.

**Key decisions:** The `users_search` failure is a code issue - the SAML team-ID resolution error leaks to stderr even when the primary command succeeds. The `olly` tests may just need the right API scope, but we should verify the text output format is correct too. The remaining 6 failures (api_keys, extensions, roles, saml, views) are purely key scope issues - no code changes needed.

### Before/After

Currently `users_search` fails because stderr contains an auth error from a secondary SAML lookup. `olly_ask_text_output` may have a rendering issue (TOON vs text format) in addition to the auth scope issue. After this milestone, the code issues are fixed and any remaining failures are purely key scope problems to be resolved by updating the CI secret.

### 2.1 [ ] Fix `olly` E2E tests

- **Files:** `tests/e2e/olly/mod.rs`, possibly `src/commands/olly/mod.rs`
- **What:** Both `olly_ask_basic` and `olly_ask_text_output` fail with "Permission denied" - the olly API requires a specific scope. First verify: is this purely a key scope issue, or is there also a text rendering bug? Check: (a) does the test specify `-o text` or default output? (b) what does the text output look like when the command succeeds (from the earlier local run where olly worked)? From our earlier run, `olly_ask_basic` passed but `olly_ask_text_output` failed with a TOON format assertion. So there are two issues: the key scope (both tests) and the text rendering (text_output test). Fix the text rendering so that when `-o text` is used, output includes "Chat ID:" header instead of TOON format. The key scope is a manual step.
- **Acceptance:** With a valid olly-scoped key, both `olly_ask_basic` and `olly_ask_text_output` pass.
- **Dependencies:** None

### 2.2 [ ] Fix `users_search` stderr leaking auth errors

- **Files:** `src/commands/iam/mod.rs` (or wherever users search is implemented)
- **What:** The `users_search` test runs `cx iam users search -o json` which returns `[]` on stdout but also prints "Authentication failed: Permission denied" on stderr from a SAML team-ID resolution step. The harness `run_ok` catches this auth error and panics. The SAML lookup is a secondary/optional step - its failure should not leak "Authentication failed" to stderr when the primary command succeeds. Investigate the code path and either: (a) make the SAML resolution failure silent/logged at debug level, or (b) catch the error and continue without it. The primary search should work without SAML team-ID resolution.
- **Acceptance:** `cx iam users search -o json` succeeds without "Authentication failed" on stderr, even when the key lacks SAML scope.
- **Dependencies:** None

