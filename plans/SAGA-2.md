# SAGA-2 — CX CLI: Return correct error when Dataprime queries fail

## Context

`cx logs`, `cx spans`, and `cx dataprime query` all stream NDJSON from `/api/v1/dataprime/query`. The shared parser `parse_ndjson_response` in `src/commands/dataprime/api.rs:145-168` recognizes only two NDJSON line shapes — `result` rows and `compileWarning` lines — and silently drops everything else. When the backend rejects a query (e.g. an archive-tier query that hits OOM), it emits an error NDJSON line. The parser ignores it, `merged.rows` stays empty, and the user sees a misleading `"No results found."` printed by `render_log_text` (`src/commands/logs/mod.rs:19-22`) on stdout. The underlying HTTP is 200, so `checked_text` in `src/api_client.rs:148-192` does not catch it.

The fix makes `parse_ndjson_response` detect error lines and propagate them, surfaces the message on stderr, and short-circuits the misleading stdout output so the user sees the actual failure reason and a non-zero exit code. The change automatically applies to all three callers (`cx logs`, `cx spans`, `cx dataprime query`) via the shared `run_query` → `execute_query` → `post_query` path.

## Current ("before") behavior

I cannot reproduce the live failure in this worktree (no `c4c` profile and no API key are configured), so the before-state is grounded in code reading:

1. `parse_ndjson_response` (`src/commands/dataprime/api.rs:145-168`) walks NDJSON lines; only `result.results` and `warning.*.warningMessage` are handled.
2. `merge_results` (`src/commands/dataprime/mod.rs:283-321`) prints per-profile errors to stderr but otherwise treats them as soft.
3. `render_results` (`src/commands/dataprime/mod.rs:329-378`) → `render_log_text` / `render_span_text` prints `"No results found."` / `"No spans found."` when `merged.rows.is_empty()`.

Expected (from ticket, on a single failing profile):

```
Error: query failed: query ran out of memory
```

Actual:

```
Querying...
No results found.
```

(exit code 0)

## Decision summary

- **Error behavior**: parser returns `Err` on the first error line. Single-profile failures propagate the underlying `CxError` up to `main` and exit non-zero, with no `"No results found."` on stdout. Multi-profile partial failures retain the existing soft-error pattern (per-profile error on stderr, successful profiles render normally). Multi-profile full failure exits non-zero. Matches success criterion 3 in the ticket.
- **Parser shape**: defensive. The exact JSON shape of the Dataprime error line is unverified in this worktree (no `DataprimeError` proto found alongside `olly/libs/common/proto/com/coralogix/dataprime/v1/warnings.proto`). The plan assumes a top-level `"error"` key analogous to `"warning"` and parses opportunistically; the raw error JSON is always appended to the message so no information is lost if the shape differs.
- **Run/check commands** (from `cx-cli/CLAUDE.md` and `.claude/skills/run-tests/SKILL.md`):
  - Build: `cargo build`
  - Tests: `cargo test --locked` (full suite); `cargo test --test dataprime` (this fix); `cargo test --test dataprime query` (integration); `cargo test --test dataprime output` (parser units)
  - Lint: `cargo clippy --locked -- -D warnings`
  - Format: `cargo fmt --check`
  - Full CI gate: `cargo fmt --check && cargo clippy --locked -- -D warnings && cargo test --locked`

## Implementation plan

### Step 1 — Detect error lines in the parser

File: `src/commands/dataprime/api.rs` (`parse_ndjson_response`, around lines 145-168).

Add a new branch after the existing `warning` branch:

```rust
} else if let Some(err_value) = value.get("error") {
    // Treat as fatal. Extract a human-readable message defensively because
    // the exact JSON shape from the Dataprime backend is not pinned down
    // in the proto we have access to (compare `compileWarning` → `warningMessage`).
    let message = extract_error_message(err_value);
    return Err(CxError::Api {
        status: 200,
        message: format!("query failed: {message}"),
    });
}
```

Add a private helper `fn extract_error_message(v: &Value) -> String` to `api.rs` that probes — in order:

1. `v.as_str()` → returns the string directly.
2. `v.as_object()` then walks each child value looking for `errorMessage`, `message`, or `warningMessage` keys and returns the first non-empty string.
3. Fallback: returns `v.to_string()` (the raw JSON serialization) so the message is never lost when the shape is unexpected.

Edge cases:
- Defensive against an error line that legitimately has no recognizable message (returns the raw JSON dump as the message body).
- A result row carrying a stray `"error"` field would be a false positive — verify result rows never have a top-level `"error"` key (the result envelope from `normalize_row` has only `metadata`/`labels`/`userData`, so this is safe; document the assumption in a comment).

Add unit tests in `tests/dataprime/output.rs`:
- `ndjson_error_string_returns_err` — `{"error":"query ran out of memory"}` line yields `Err` whose message contains `"query failed: query ran out of memory"`.
- `ndjson_error_object_with_message_returns_err` — `{"error":{"someSubtype":{"errorMessage":"OOM"}}}` line yields `Err` whose message contains `"OOM"`.
- `ndjson_error_unknown_shape_includes_raw_json` — `{"error":{"unknownField":42}}` line yields `Err` whose message contains the raw JSON.
- `ndjson_error_aborts_before_subsequent_results` — when an error line precedes a `result` line, the parser still returns `Err` and ignores the result.

### Step 2 — Propagate full-failure as a fatal error from `run_query`

File: `src/commands/dataprime/mod.rs`.

Change `merge_results` to also return the list of per-profile errors instead of swallowing them inside the function. Change the signature from:

```rust
pub fn merge_results(per_profile, include_profile) -> MergedResults
```

to:

```rust
pub fn merge_results(per_profile, include_profile) -> (MergedResults, Vec<(String, anyhow::Error)>)
```

Move the `eprintln!` for per-profile errors out of `merge_results` and into `run_query`, so the caller can decide whether to print, propagate, or both.

In `run_query` (around line 412-415):

```rust
let (merged, errors) = merge_results(per_profile, include_profile);
let total = targets.len();

if errors.len() == total && total > 0 {
    // Every profile failed. Print all per-profile errors and propagate.
    for (profile, err) in &errors {
        eprintln!("{}", format!("error from profile '{profile}': {err:#}").red());
    }
    if total == 1 {
        // Single-profile failure: return the underlying error so `main`
        // prints "Error: <message>" once and exits non-zero. Skip
        // render_results entirely to avoid "No results found." on stdout.
        let (_, err) = errors.into_iter().next().unwrap();
        return Err(err);
    }
    return Err(anyhow!("all profiles failed"));
}

// Partial failure: keep existing soft-error behavior.
for (profile, err) in errors {
    eprintln!("{}", format!("error from profile '{profile}': {err:#}").red());
}

render_results(&merged, output, max_direct, temp_dir, text_renderer)
```

This avoids the "double-print" hazard (the per-profile `eprintln!` plus `main`'s top-level `Error: ...` line) for the common single-profile case by deduplicating: for `total == 1`, propagate the original error and skip the prefixed `eprintln!`. Actually re-check — to avoid the duplicate, the cleanest is to skip the `eprintln!` only when `total == 1` (let `main`'s default `Error: ...` print be the single source of truth). Suggested refinement:

```rust
if errors.len() == total && total == 1 {
    let (_profile, err) = errors.into_iter().next().unwrap();
    return Err(err);            // main prints "Error: <message>" once
}
for (profile, err) in &errors {
    eprintln!("{}", format!("error from profile '{profile}': {err:#}").red());
}
if errors.len() == total && total > 1 {
    return Err(anyhow!("all profiles failed"));
}
render_results(...)
```

Update the existing `merge_results` unit tests in `src/commands/dataprime/mod.rs:483-571` to destructure the new tuple return — semantics for partial failures are unchanged.

### Step 3 — Integration test

File: `tests/dataprime/query.rs` (parallel to `dataprime_query_with_warning` at line 98).

Add `dataprime_query_error_returns_err`:

```rust
#[tokio::test]
async fn dataprime_query_error_returns_err() {
    let server = MockServer::start().await;

    let ndjson = [
        r#"{"queryId":{"queryId":"test-query-id"}}"#,
        r#"{"error":{"queryError":{"errorMessage":"query ran out of memory"}}}"#,
    ].join("\n");

    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    let targets = vec![target];

    let result = run_query(
        &targets,
        "source logs | groupby $m.logid aggregate any_value($d) as data",
        "logs",
        "2024-06-22T00:00:00Z",
        "2024-06-22T23:59:59Z",
        100,
        Some(Tier::FrequentSearch),
        OutputFormat::Json,
        None,
        "/tmp",
        None,
    ).await;

    let err = result.expect_err("query with error line should return Err");
    let msg = format!("{err:#}");
    assert!(msg.contains("query failed"), "error message: {msg}");
    assert!(msg.contains("query ran out of memory"), "error message: {msg}");
}
```

Optionally add a second integration test for the `"error"`-as-string shape if you find it during shape verification.

### Step 4 — Verification of error shape

The implementor MUST do one of the following before merging:
1. Search the Dataprime backend source for the serializer that emits the NDJSON error line on a memory failure (look for `DataprimeError` proto in upstream coralogix proto repos — not present in this worktree).
2. Run a live failing query against `c4c` (or any team) with `cx logs '... groupby ... aggregate ... ' --tier archive` and capture the raw NDJSON by temporarily logging `raw` in `post_query` (`src/commands/dataprime/api.rs:75-79`).
3. Confirm the shape with a Coralogix backend engineer.

If the live shape differs from `{"error": {<subtype>: {"errorMessage": "..."}}}`, adjust `extract_error_message` and replace the integration-test mock body with the verified shape. The defensive fallback (raw JSON in message) means a mismatch degrades gracefully rather than silently dropping the message — but the unit test for the verified shape should be tightened to assert the actual extracted message.

## Files to modify

| File | Change |
|------|--------|
| `src/commands/dataprime/api.rs` | Add `error` branch to `parse_ndjson_response` + `extract_error_message` helper |
| `src/commands/dataprime/mod.rs` | Change `merge_results` signature to return errors; update `run_query` to short-circuit on full failure; update inline unit tests |
| `tests/dataprime/output.rs` | Add unit tests for the new parser branch and helper |
| `tests/dataprime/query.rs` | Add `dataprime_query_error_returns_err` integration test |

Reused existing utilities:
- `CxError::Api { status, message }` from `src/error.rs:11-12` — no new variant needed.
- The fan-out / merge / render scaffolding in `src/commands/dataprime/mod.rs` — only `merge_results` and `run_query` change.
- The wiremock pattern in `tests/dataprime/query.rs` — same `make_ndjson_response` helper style for the new test.

Out of scope (per ticket):
- `src/api_client.rs::checked_text` — only handles HTTP-level errors; the fix belongs in the Dataprime-specific parser.
- `cx metrics` — separate PromQL pipeline.
- Warning display behavior — unchanged.

## Verification

End-to-end:
1. `cargo fmt --check && cargo clippy --locked -- -D warnings && cargo test --locked` — full CI gate must pass.
2. `cargo test --test dataprime` — new tests pass.
3. Run `cargo run -- logs 'groupby $m.logid aggregate any_value($d) as data' --tier archive --profile c4c -o text` and confirm:
   - stderr: `Error: API request failed (200): query failed: query ran out of memory` (or whatever the real message is once the shape is verified).
   - stdout: empty (no `"No results found."`).
   - Exit code: non-zero.
4. Smoke a successful query (e.g. `cx logs 'source logs | limit 1' --profile c4c`) and confirm nothing regressed — results still print, exit 0.
5. Smoke a warning-only response (e.g. via the existing `dataprime_query_with_warning` test path) and confirm warnings still surface and the command still succeeds.

If `cargo run` against a live backend is not available in CI, the integration test `dataprime_query_error_returns_err` is the ground truth for step 3 — verify locally if credentials allow.

## Risks

- **Unverified error JSON shape**: defensive parsing + raw-JSON fallback keeps the message visible even on shape mismatch, but the integration-test assertion will need updating once verified.
- **`merge_results` signature change**: 5 unit tests in `src/commands/dataprime/mod.rs` need a tuple destructure. Mechanical.
- **Behavior change for single-profile failures**: previously the command exited 0 with `"No results found."`; now it exits non-zero. Intended, but any scripts that swallow stderr and read stdout will see a behavior change.
