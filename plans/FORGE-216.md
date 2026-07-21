# FORGE-216 — Improve case events handling

## Goal
1. `get_alerts_object` (Case) stops inlining full case-event payloads; returns an ordered summary of event **types** covering ALL events (not just last 30), aggregated when >30.
2. New MCP + agent tool to fetch full case events with `case_id`, optional `event_type` filter, and client-side paging.
3. Update the `single-case` skill "Summarizing comments" section (both copies) to call the new tool instead of reading comments from `get_alerts_object`.

## Current behavior (verified)
- Agent tool: `apps/api/src/api/agent/shared/alerts/tools.py:72` → `get_alerts_object_impl` (`libs/common/src/common/tools/alerts_tools.py:252`) → `_get_case` (`:138`) calls `coralogix_grpc_client.list_case_events(case_id)` (`libs/common/src/common/clients/coralogix_grpc_client.py:1351`, returns ALL events chronologically, no server paging/filter — proto confirmed) and stores them in `CaseObjectData.case_events`.
- Rendering: agent → `case_to_dict` (`formatting.py:846`) → `_case_events_to_list` (`:741`) reverses to newest-first, caps at `MAX_CASE_EVENTS_TO_SHOW=30`, emits full per-event payloads under key `last_30_case_events` (`:969`).
- MCP tool: `apps/ws-ai-mcp/src/mcp_server/tools/alerts/get_alerts_object_tool.py:58` `_render_found_object` serializes the entire `case_events` list via `MessageToDict`.
- Both surfaces share `get_alerts_object_impl` / `CaseObjectData` in `libs/common`.
- `apps/case_analysis_service` has its OWN prompt (`analyzer/prompt.py:74` `_view_event`) independent of this path — its tests (`test_prompt.py`) are unaffected.
- **`cx-cli/skills/cx-cases/references/single-case.md` does not exist in this repo** → the ticket's "confirm during planning" resolves to: only the two skill files below need updating.
- `apps/ws-ai-mcp` has no test suite; testable logic must live in `libs/common` (tested in `libs/common/tests/test_alerts_tools.py`).

## Design decision: shape of the event-type summary
Follow the DoD literally (dual-branch), keyed as `case_event_types`:
- **≤ 30 events total:** ordered list of event-type strings (original chronological order, duplicates kept positionally). e.g. `["created","status_changed","comment","comment"]`.
- **> 30 events total:** aggregated — iterate in order, dedup by first occurrence, emit `[{"event_type": "...", "count": N}, ...]` counting occurrences across the FULL list.

(If the assignee prefers a single always-aggregated shape for simplicity, that's a trivial change — but DoD wording distinguishes the two, so plan targets dual-branch.)

## Ordering
New case-events tool returns events in **chronological order** (oldest→newest, as the RPC provides). Paging is offset/limit over that full ordered list (filtered first if `event_type` given).

---

## Changes (in dependency order)

### 1. `libs/common/src/common/tools/alerts_tools.py` (shared logic — do first)
- Add `summarize_case_event_types(case_events: list[CaseEvent]) -> list`:
  - Extract each event's type via `event.event_data.WhichOneof("data")` (skip events with no data type).
  - If `len <= 30`: return ordered list of type strings.
  - Else: return first-occurrence-ordered list of `{"event_type", "count"}`.
- Add `AGGREGATE_CASE_EVENTS_THRESHOLD = 30` constant here (replaces reliance on formatting's `MAX_CASE_EVENTS_TO_SHOW`).
- Add `@dataclass CaseEventsPage`: `events: list[CaseEvent]`, `total_count: int`, `filtered_count: int`, `offset: int`, `limit: int`, `has_more: bool`.
- Add `get_case_events_impl(*, coralogix_grpc_client, case_id, event_type=None, offset=0, limit=30, error_factory) -> CaseEventsPage`:
  - `events = await client.list_case_events(case_id)` (chronological).
  - If `event_type`: validate against the known set of oneof names (see list below); on invalid value raise `error_factory(...)` listing valid types. Filter `events` to matching type.
  - Slice `[offset:offset+limit]`; compute `filtered_count`, `has_more`.
  - Return `CaseEventsPage`.
- Valid `event_type` values (proto `case_event.proto` oneof `data`): `assigned, unassigned, comment, status_changed, notification_sent, notification_failed, created, priority_details_changed, title_changed_event, resolution_reason_changed_event, change_assignee_failed, kpi_breached`.
- Keep `CaseObjectData.case_events` as-is (still fetched); rendering layers now summarize rather than inline.

### 2. `apps/api/src/api/agent/shared/alerts/formatting.py`
- Refactor `_case_events_to_list` → extract a single-event formatter `case_event_to_dict(event: CaseEvent) -> dict` (the existing per-event body incl. actor + event-type-specific `_format_*` handlers), used by the new tool for full detail. No reverse/cap inside it.
- `case_to_dict`: replace the `last_30_case_events` block (`:968-971`) with:
  `output["case_event_types"] = summarize_case_event_types(case_events)` (import from common). Update docstring. `case_events=None` → omit key (keep current guard).
- `MAX_CASE_EVENTS_TO_SHOW` no longer used for inlining; remove if unreferenced (grep first).
- Add `case_events_page_to_dict(page: CaseEventsPage, is_verbose) -> dict`: `{search_results_metadata:{returned, filtered_count, total_count, offset, limit, has_more}, events:[case_event_to_dict(e) ...]}`.

### 3. `apps/api/src/api/agent/shared/alerts/tools.py`
- Add agent tool `get_case_events` via `@metered_function_tool(tool_name="get_case_events")`:
  - Params: `case_id: str`, `event_type: str | None = None`, `offset: int = 0`, `limit: int = 30`, `is_verbose: bool = False`.
  - Call `get_case_events_impl(...)` with `error_factory=ToolError`; return `format_dict_to_llm(case_events_page_to_dict(page, is_verbose), is_verbose)`.
  - Docstring: explain it fetches full case-event detail (comments, actors, timestamps) with optional type filter + paging; note `get_alerts_object` only returns event-type counts.

### 4. `apps/api/src/api/agent/agents/skills_agent/skills_agent.py`
- Import `get_case_events` and add to the `tools=[...]` list (near `get_alerts_object`).
- Update the "Alerts" tool line in `INSTRUCTIONS_TEMPLATE` (`:165`) to mention `get_case_events`.

### 5. `apps/ws-ai-mcp/src/mcp_server/tools_description_v2.py`
- Add `GET_CASE_EVENTS_TOOL_DESCRIPTION_V2` describing case_id/event_type/offset/limit and that it returns full event detail.
- Optionally tweak `GET_ALERTS_OBJECT_TOOL_DESCRIPTION_V2` to note cases now return event-type counts + to use `get_case_events` for detail.

### 6. `apps/ws-ai-mcp/src/mcp_server/tools/alerts/get_alerts_object_tool.py`
- In `_render_found_object`, for `CaseObjectData` replace `"case_events": _serialize_value(case_events)` with `"case_event_types": summarize_case_event_types(case_events)` (import from common). Keep `case` and `alert_event_ids`.

### 7. `apps/ws-ai-mcp/src/mcp_server/tools/alerts/get_case_events_tool.py` (new)
- Mirror `search_alert_definitions_tool.py` structure: `@mcp.tool(name="get_case_events", version=V2, annotations=readOnly)`.
- Params: `case_id: str`, `event_type: str | None = None`, `offset: int = 0`, `limit: int = 30`.
- Call `get_case_events_impl(...)` with `error_factory=ToolError` (fastmcp); serialize the page (events via `MessageToDict`/`_serialize_value`, plus metadata) and `json.dumps`.

### 8. `apps/ws-ai-mcp/src/mcp_server/main.py`
- Import and register `get_case_events_tool` alongside `get_alerts_object_tool` (the `from mcp_server.tools.alerts...` import block at `:37`).

### 9. Skill docs — "Summarizing comments" (task 3)
Update BOTH files identically (`apps/api/src/api/agent/agents/skills_agent/skills/single-case.md:55-61` and `apps/ws-ai-mcp/src/mcp_server/skills/single-case/SKILL.md` same section):
- Replace step 1 "Read the `comment` case events in the `get_alerts_object` output" with: call `get_case_events` with `event_type="comment"` (and paging) for the case; `get_alerts_object` now only returns event-type counts, not comment text.
- Keep steps 2–4 (capture actor/timestamp/comment_text; topic-organized chronological summary; Slack caveat).

### 10. Tests
- `libs/common/tests/test_alerts_tools.py` (add):
  - `summarize_case_event_types`: (a) ≤30 mixed events → ordered type-string list; (b) >30 repeating types → aggregated `{event_type,count}` in first-occurrence order, counts sum to total. Build `CaseEvent` protos with the relevant oneof set (e.g. `CaseEvent(event_data=CaseEventData(comment=CommentCaseEvent(...)))`).
  - `get_case_events_impl`: filter by `event_type`, offset/limit slicing, `has_more`/counts, invalid `event_type` raises via error_factory. Use `AsyncMock` client with `list_case_events` returning a crafted list.
- `apps/api/tests/conftest.py`: `list_case_events` mock already returns `[]` (line 482) — verify still valid; no change expected. If any existing test asserts `last_30_case_events`, update to `case_event_types` (grep — none found currently).
- `apps/case_analysis_service/tests/test_prompt.py`: independent path, expected to pass unchanged — run to confirm.

---

## Edge cases / risks
- Events with no `event_data` oneof set → skip in type summary (don't emit blank type).
- `offset` beyond length → empty page, `has_more=False` (no error).
- `event_type` value must match proto oneof field names exactly; validation error must list valid values.
- Very large event lists: fetching all then slicing is unavoidable (RPC has no paging) — acceptable per ticket.
- Keep the `case` field and `alert_event_ids` in both `get_alerts_object` renderers unchanged; only the events portion changes.
- Two output shapes for `case_event_types` — make the tool docstring/skill note both so the LLM handles list-of-strings vs list-of-objects.

## Run / check commands (from `justfile`, scope to changed packages)
- Lint: `just common::lint`, `just api::lint`, and `just ws_ai_mcp::lint`.
- Unit tests: `just test-common` (new shared-logic tests — primary verification), `just test-api`, `just test-case-analysis`.
- Full gate per repo rule: `/code-checks` (lint + UT + integration). Integration tests are the authoritative verification.
- Before/after behavior to observe: for a case with >30 events, `get_alerts_object` output `found_object` must contain `case_event_types` (aggregated `{event_type,count}`) and NO full per-event payloads; `get_case_events(case_id, event_type="comment")` returns full comment detail (text/actor/timestamp) for the requested page.

## Out of scope (confirmed)
- No change to Coralogix `ListEvents` RPC (client-side paging/filter only).
- No change to `case-analytics` skill or `system/labs.cases.state_updates` dataset.
- `cx-cli` single-case doc: file absent in repo → nothing to update.