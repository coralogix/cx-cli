# Add Fleet configuration group and overlay commands

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current as implementation proceeds.

## Purpose / Big Picture

After this change, cx CLI users can inspect and manage Fleet Manager configuration groups and configuration overlays from the terminal. They can list and retrieve the complete server representation, create and update a resource from a JSON file or stdin, and archive it. List commands expose pagination and archived-resource filters, and every operation follows the CLI's existing multi-profile and text, JSON, and TOON output behavior. A user can verify the feature by running `cx configuration-groups --help` and `cx configuration-overlays --help`, then using the documented subcommands against an account with Fleet Manager access.

The API treats deleting these resources as archiving. Archive is irreversible through these APIs, so the CLI must require the global `--yes` flag before sending that request. Create sends the caller's complete JSON body because family contents, selectors, schedules, and raw Collector YAML are domain data that must be preserved without CLI-specific reshaping. Update requires an explicit `--update-mask`; only those fields are changed, and any masked field omitted from the JSON body may be cleared or reset to its default.

## Progress

- [x] (2026-11-26) Located the clean `coralogix/cx-cli` checkout and created a separate worktree from `origin/master` on `codex/manage-config-groups-overlays`.
- [x] (2026-11-26) Confirmed the CLI's REST command patterns and found the Fleet configuration group and overlay REST definitions in the local OpenAPI source.
- [x] (2026-11-26) Recorded the endpoint methods, list options, pagination, and resource response shapes in this plan.
- [x] (2026-11-26) Implemented REST API modules and CLI handlers for configuration groups and overlays, including focused query and nested-response tests.
- [x] (2026-11-26) Registered subcommands, destructive-action confirmation, help examples, and command dispatch.
- [x] (2026-11-26) Added request-level mock tests for each route family and updated the CLI schema assertion for both new top-level commands.
- [x] (2026-11-26) Passed the full Rust test suite, formatting check, and Clippy with warnings denied; verified both update help screens expose the required mask.
- [x] (2026-11-27) Added ignored E2E list/lifecycle tests for both resource families, registered them in `tests/e2e.rs`, and added stale-resource archiving support to the E2E sweeper.
- [x] (2026-11-27) Compiled the E2E target without running its live-account tests; focused mock/schema/write-gating tests and Clippy pass after the additions.
- [x] (2026-11-27) Fixed follow-up review findings by preserving list options/profile in continuation commands and paginating the sweeper before archiving stale resources.
- [x] (2026-11-27) Recorded final results and remaining limitations.

## Surprises & Discoveries

- Observation: The CX shippers workspace is an umbrella directory, but `cx-cli` is a clean Git checkout nested inside it. The task's starting branch is an integration branch, so the new worktree uses the repository's `origin/master` default branch.
  Evidence: `git -C cx-cli status --short --branch` showed no changes on `codex/CX-71310-integration`; `git -C cx-cli symbolic-ref refs/remotes/origin/HEAD` returned `origin/master`.
- Observation: Fleet configuration group and overlay removal is an archive operation, not a DELETE endpoint.
  Evidence: `openapi-facade/may26/openapi.yaml` defines `POST /fleet-management/configuration-groups/v1/{id}/archive` and `POST /fleet-management/configuration-overlays/v1/{id}/archive`.
- Observation: Both list APIs are paginated and support `include_archived`; group and overlay JSON includes nested resource-specific data.
  Evidence: The same OpenAPI file documents `page_size`, `page_token`, and `include_archived` on both list routes, and shows group `family` and overlay `custom` and `schedules` objects.
- Observation: The service returns list envelopes named `groups` and `overlays`, with an optional `nextPageToken`; create/get/update return the complete resource object.
  Evidence: The OpenAPI examples and schema references for `ListConfigurationGroupsResponse` and `ListConfigurationOverlaysResponse` use these fields. The CLI keeps the full nested resource JSON and includes outstanding next-page tokens in JSON/TOON output.
- Observation: Both PATCH routes require an `update_mask`, and an update-mask field omitted from the JSON body may be cleared or reset.
  Evidence: `openapi-facade/may26/proto/com/coralogixapis/fleet_management/v1/configuration_group.proto` documents `update_mask` as required and describes partial updates; `configuration_overlay.proto` documents the same query parameter and clear/reset semantics. The CLI now requires `--update-mask` and sends it as a query parameter.

## Decision Log

- Decision: Implement two separate REST command families named `configuration-groups` and `configuration-overlays`.
  Rationale: Each maps directly to one Fleet Manager API resource and avoids making two distinct object schemas look like parsing rule groups or alerts.
  Date/Author: 2026-11-26 / Codex.
- Decision: Use complete JSON request bodies for create, and require explicit update masks for PATCH requests while preserving API response JSON for get and mutations.
  Rationale: Group families and overlay configuration/schedules are nested, evolving schemas; lossless JSON input avoids silently dropping fields. The API defines PATCH as selective and warns that masked but omitted fields are cleared/reset, so the CLI makes the update mask explicit.
  Date/Author: 2026-11-26 / Codex.
- Decision: Name the destructive subcommand `archive` and guard it with the CLI's global `--yes` convention.
  Rationale: The service archives resources and does not expose unarchive or physical deletion; command language should communicate the actual effect.
  Date/Author: 2026-11-26 / Codex.
- Decision: Return one requested list page instead of automatically walking every page.
  Rationale: The REST APIs expose opaque continuation tokens, and making the page boundary visible gives users control over large account inventories and lets scripts request specific pages.
  Date/Author: 2026-11-26 / Codex.

## Outcomes & Retrospective

Both command families now support list, get, create, update, and archive. List supports archived resources and one-page pagination; output preserves full nested data and continuation tokens. Text-mode continuation commands preserve the profile, archived filter, page size, and page token for each profile independently. The sweeper traverses every page before archiving stale groups and overlays. Create accepts a JSON file or stdin. Update requires an explicit API update mask. Create, update, and archive use the existing destructive-action confirmation gate. The Rust unit/integration suite, focused checks, formatting, and Clippy pass. The E2E target compiles and has ignored list/lifecycle tests in `tests/e2e/configuration_management.rs`. Live E2E tests were not run because they require designated writable test-team credentials. No live account mutations were made during this task.

## Context and Orientation

This repository is the Rust CLI `cx` in `cx-cli`. `src/main.rs` defines user-facing command names, arguments, and dispatch. `src/commands/mod.rs` registers Rust command modules. A REST feature normally has an `api.rs` file that calls the shared HTTP client in `crate::api_client::CxClient`, and a `mod.rs` file that fans operations out over selected profiles and renders text, JSON, or TOON. `src/commands/parsing_rules/` and `src/commands/alerts/` are references for those conventions. Use the same shared `ExecutionTarget`, `fan_out`, `report_errors_and_collect_successes`, `OutputFormat`, and `render` helpers rather than making direct network calls.

The Fleet REST routes are under `/mgmt/openapi/5/fleet-management/`. Configuration groups use `/configuration-groups/v1`; overlays use `/configuration-overlays/v1`. A group has metadata plus a latest `family`, which contains raw Collector configurations and selectors. An overlay has metadata plus custom YAML configuration and targets, and may also contain schedules. A page token is an opaque string returned by a list response; pass it back unchanged, with the same archived filter, to retrieve the next page. List JSON/TOON is an object with a `groups` or `overlays` array and, when more pages exist, a `next_page_tokens` object keyed by profile name.

There is no `AGENTS.md` inside the cx-cli checkout. The repository guide `contributing/adding-a-command.md` requires REST integrations to use HTTP through `CxClient` and describes the module registration and CLI dispatch pattern.

## Plan of Work

First, inspect the exact OpenAPI schemas for list, get, create, update, and archive to confirm query names and response envelopes. This is complete: list returns `groups` or `overlays` with `nextPageToken`; create is POST, update is PATCH with required `update_mask`, and archive is POST to the `/archive` subresource. Each API client uses JSON `Value` for the evolving nested resource representations. List methods accept `include_archived`, `page_size`, and `page_token`, and return both resource values and the next page token. Create accepts a complete body. Update accepts a field-value body and the caller's explicit comma-separated update mask.

Next, add command modules that mirror existing REST commands. This is complete: list reads one requested page, handles profile fan-out, renders useful text columns, and retains the full resource objects and continuation tokens in JSON/TOON. Get and create/update return full resource payloads in machine-readable formats. File input accepts `-` for stdin, matching parsing rules and integrations.

Finally, register both modules in `src/commands/mod.rs`, define `ConfigurationGroupsCmd` and `ConfigurationOverlaysCmd` in `src/main.rs`, add help text with examples, wire every subcommand to its handler, and use the existing confirmation helper for create/update/archive. This is complete. Unit and request-level mock tests cover query construction, nested response preservation, HTTP routes, methods, update masks, and request bodies; validation will confirm CLI help and the Rust test/lint suites.

## Concrete Steps

Run commands from `/Users/povilas.versockas/.codex/worktrees/cx-cli/six-cli-config-management`.

Inspect the API source definitions before finalizing implementation:

    rg -n "configuration-groups/v1|configuration-overlays/v1" /Users/povilas.versockas/Desktop/coralogix/iac-dev/openapi-facade/may26/openapi.yaml

Implement the modules and command wiring, then format and validate with the repository's pinned toolchain:

    cargo fmt --check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    cargo run -- --help
    cargo run -- configuration-groups --help
    cargo run -- configuration-overlays --help

Expected behavior: help shows list/get/create/update/archive for each family. Archive without `--yes` exits before an API call and explains the confirmation requirement. JSON create/update accept either a file path or `-` for stdin; every update also requires `--update-mask`. A successful list displays groups or overlays and, when present, a next-page token; JSON and TOON output preserve full nested fields.

Observed validation on 2026-11-26 and 2026-11-27: `cargo fmt --check` succeeded; the full `cargo test` run passed (1098 library tests, 12 binary tests, all active integration tests passed; 13 library tests and 262 E2E tests were ignored); and Clippy with warnings denied succeeded. After adding E2E coverage, focused configuration, schema, write-confirmation, and continuation-command tests passed. `cargo test --test e2e --no-run` compiled the new E2E tests, and `cargo clippy --locked --all-targets --all-features -- -D warnings` succeeded. A pure sweeper helper test verifies page-token extraction from the per-profile token map. The ignored live E2E tests were not run; use the documented `CX_E2E_ALLOW_WRITES` and `CX_E2E_TEAM_ID` safeguards with the designated writable test team.

Validation must include the full test suite and lint/build checks above. Do not send live create, update, or archive requests during tests; exercise HTTP methods and bodies through local test fixtures or the existing mock client patterns.

## Validation and Acceptance

Acceptance is observable in both CLI help and API behavior. `cx configuration-groups list` and `cx configuration-overlays list` must request only the selected resource family, honor archived and pagination options, and render results for one or more profiles. `get <id>` must return the full nested object. `create --from-file file.json` sends the complete file body; `update <id> --update-mask <fields> --from-file file.json` sends that body and the required mask as a query parameter. `archive <id>` must POST to the archive route only after global `--yes` is supplied. The equivalent commands must appear under both top-level help entries.

Automated tests must cover query encoding, response parsing including empty pages and absent optional fields, correct HTTP method/path/body for each operation, and the archive confirmation gate. `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings` must all succeed. If an exact count is not stable, report the actual test output rather than hard-coding a count here.

## Idempotence and Recovery

The worktree is separate from the existing cx-cli checkout. Re-running formatting, tests, and help commands is safe. Local mock tests do not alter a Coralogix account. Never include an empty or inferred archive target; require the explicit ID as a positional argument. If implementation is interrupted, resume from this file, inspect `git status`, and continue the unchecked progress entries without altering unrelated files.

## Artifacts and Notes

The created worktree is `/Users/povilas.versockas/.codex/worktrees/cx-cli/six-cli-config-management` on branch `codex/manage-config-groups-overlays`, starting at `origin/master`.

For hands-on account testing in a disposable environment, follow `plans/manual-testing-fleet-config-cli.md`. It creates inactive resources and exercises their CLI lifecycle without applying configuration to agents.

## Interfaces and Dependencies

The implementation uses existing `CxClient` HTTP methods and current Tokio/Serde dependencies. The modules `configuration_groups` and `configuration_overlays` are registered in `src/commands/mod.rs`. Each exposes runner functions to `src/main.rs` and keeps transport code in `api.rs`.

Group routes:

    GET    /mgmt/openapi/5/fleet-management/configuration-groups/v1
    GET    /mgmt/openapi/5/fleet-management/configuration-groups/v1/{id}
    POST   /mgmt/openapi/5/fleet-management/configuration-groups/v1
    PATCH  /mgmt/openapi/5/fleet-management/configuration-groups/v1/{id}?update_mask=<fields>
    POST   /mgmt/openapi/5/fleet-management/configuration-groups/v1/{id}/archive

Overlay routes:

    GET    /mgmt/openapi/5/fleet-management/configuration-overlays/v1
    GET    /mgmt/openapi/5/fleet-management/configuration-overlays/v1/{id}
    POST   /mgmt/openapi/5/fleet-management/configuration-overlays/v1
    PATCH  /mgmt/openapi/5/fleet-management/configuration-overlays/v1/{id}?update_mask=<fields>
    POST   /mgmt/openapi/5/fleet-management/configuration-overlays/v1/{id}/archive

Both list routes accept optional query parameters `include_archived`, `page_size`, and `page_token`. The ID get routes accept `include_archived`. Each PATCH route requires an explicit `update_mask` query parameter listing the fields to modify. Keep opaque page tokens unchanged. Do not use DELETE or PUT unless the checked-in schema contradicts these route definitions.

Revision note: Updated the plan after inspecting the detailed PATCH definitions. Both updates require `update_mask`, and fields named in the mask but omitted from the body may be cleared or reset; CLI help and acceptance now make this behavior explicit. Local Codex review identified missing E2E coverage, so ignored list/lifecycle tests and stale-resource sweeper entries were added. Follow-up reviews found two pagination issues: text continuation commands now preserve per-profile options, and the sweeper collects all pages before archiving resources. Focused tests and local checks pass; live E2E execution remains gated by the designated writable test-team credentials.
