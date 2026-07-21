## FORGE-217 — Add notification delivery details to verbose Case output

### Goal
Surface full notification delivery info (router, connector type/name/id, per-attempt outcome) in `case_to_dict` output, only when `is_verbose=True`, sourced from the `CasesNotificationService.ListNotificationDeliveries` RPC via a new `CoralogixGrpcClient` wrapper.

### Environment / run blocker (must read)
- Proto-generated stubs (`libs/common/src/common/generated/`) are **gitignored and absent** in this worktree; regenerating needs `just common::proto` → `protofetch` → SSH to Coralogix proto repos. In this sandbox neither `just` nor `protofetch` is on PATH, so **unit tests cannot be run locally** (`common.generated` import fails at collection). The `.proto` files themselves (`cases_notification_service.proto`, `notification_delivery.proto`) already exist in the repo, so the generated `*_pb2*` modules WILL exist once `just common::proto` runs.
- **Implementation step must run in a provisioned dev env** (with `just`+SSH) or rely on CI. Do NOT commit generated stubs (they stay gitignored — out of scope).
- Check/run commands: `just common::proto` (once, to generate stubs), `just test-common` (libs/common UT), `just test-api` (apps/api UT), `just common::lint` + `apps/api` `just lint`. Whole-repo checks are heavy — scope to the two changed packages; CI runs the authoritative gate.

### Generated stub names (from the two .proto files)
- `common.generated.com.coralogixapis.cases.v1.cases_notification_service_pb2`: `ListNotificationDeliveriesRequest`, `ListNotificationDeliveriesResponse` (`.deliveries_by_case` is `map<string, CaseNotificationDeliveries>`; `CaseNotificationDeliveries.notification_deliveries` is repeated).
- `..._pb2_grpc`: `CasesNotificationServiceStub` (rpc `ListNotificationDeliveries`).
- `common.generated.com.coralogixapis.cases.v1.notification_delivery_pb2`: `NotificationDelivery`, `RoutedDelivery`, `DeliveryAttempt`, `DeliveryOutcome`, `DeliverySuccess`, `DeliveryFailure`, `NoNotificationCreatedResult`, `NoRouterMatchedResult`, `ConnectorDetails`, `ConnectorType` (enum), `RouterInfo`.

### Changes (in dependency order)

**1. `libs/common/src/common/clients/coralogix_grpc_client.py`** — new wrapper method.
- Add imports (mirror the existing cases imports at lines 181–219): from `cases_notification_service_pb2` import `ListNotificationDeliveriesRequest`, `ListNotificationDeliveriesResponse`; from `cases_notification_service_pb2_grpc` import `CasesNotificationServiceStub`; from `notification_delivery_pb2` import `NotificationDelivery`.
- Add method after `get_case_alert_event_ids` (~line 1396), following the exact pattern of `list_case_events`:
  ```python
  async def list_notification_deliveries(self, case_id: str) -> list[NotificationDelivery]:
      request = ListNotificationDeliveriesRequest(case_ids=[case_id])
      async with self.create_grpc_channel(self.grpc_url) as channel:
          stub = CasesNotificationServiceStub(channel)
          response: ListNotificationDeliveriesResponse = await stub.ListNotificationDeliveries(
              request, timeout=self.timeout
          )
          if case_id not in response.deliveries_by_case:
              return []
          return list(response.deliveries_by_case[case_id].notification_deliveries)
  ```
  Note: `deliveries_by_case` is a proto map — guard membership before indexing (indexing a missing key auto-inserts a default; membership check avoids that and returns `[]` cleanly).

**2. `libs/common/src/common/tools/alerts_tools.py`** — thread data through.
- Import `NotificationDelivery` (from the generated `notification_delivery_pb2`).
- Add field to `CaseObjectData` (line 36-40): `notification_deliveries: list[NotificationDelivery]`.
- In `_get_case` (138-159): add the 4th call to the `asyncio.gather` and unpack it:
  ```python
  case, case_events, alert_event_ids, notification_deliveries = await asyncio.gather(
      coralogix_grpc_client.get_case(case_id),
      coralogix_grpc_client.list_case_events(case_id),
      coralogix_grpc_client.get_case_alert_event_ids(case_id, limit=events_limit),
      coralogix_grpc_client.list_notification_deliveries(case_id),
  )
  ```
  Pass `notification_deliveries=notification_deliveries` into `CaseObjectData(...)`.

**3. `apps/api/src/api/agent/shared/alerts/tools.py`** — pass to formatter.
- Update the `CaseObjectData` destructure in `_format_found_object` (lines 63-67) to bind `notification_deliveries` and pass it:
  `case_to_dict(case_obj, case_events, ids, notification_deliveries, is_verbose=is_verbose)` (prefer keyword: `notification_deliveries=notification_deliveries`).

**4. `apps/api/src/api/agent/shared/alerts/formatting.py`** — formatting + verbose gating.
- Add imports from `notification_delivery_pb2`: `NotificationDelivery`, `ConnectorType` (need `ConnectorType.Name(...)`), and any message types used for annotations.
- Extend `case_to_dict` signature (line 846) with `notification_deliveries: list[NotificationDelivery] | None = None` (place before `is_verbose`). Update docstring.
- After the `impacted_entities` verbose block (~937), following the same `if is_verbose and <truthy>` gating pattern:
  ```python
  if is_verbose and notification_deliveries:
      output["notification_deliveries"] = _format_notification_deliveries(notification_deliveries)
  ```
- Add module-level helpers (place them near the other `_format_*` case helpers, e.g. after `_format_notification_failed_event`):
  - `_format_notification_deliveries(deliveries) -> list[dict]`: map each `NotificationDelivery` via `_format_notification_delivery`.
  - `_format_notification_delivery(d) -> dict`: base = `{"timestamp": _format_proto_timestamp(d.timestamp) if d.HasField("timestamp") else None, "request_notification_id": d.request_notification_id}`. Then `result_kind = d.WhichOneof("result")` and dispatch:
    - `"no_router_matched"` → `result["result"] = "no_router_matched"`.
    - `"no_notification_created"` → `result["result"] = "no_notification_created"`, `result["matched_routers"] = [_format_router_info(r) for r in d.no_notification_created.matched_routers]`.
    - `"attempted"` → `result["result"] = "attempted"`, `result["router"] = _format_router_info(d.attempted.router)`, `result["attempts"] = [_format_delivery_attempt(a) for a in d.attempted.attempts]`.
    - `None` (unset oneof) → `result["result"] = None`.
  - `_format_router_info(r)` → `{"router_id": r.router_id, "router_name": r.router_name}`.
  - `_format_delivery_attempt(a)` → `{"connector": _format_connector(a.connector), "outcome": _format_delivery_outcome(a.outcome)}`.
  - `_format_connector(c)` → `{"connector_id": c.connector_id, "connector_type": ConnectorType.Name(c.connector_type), "connector_name": c.connector_name}`.
  - `_format_delivery_outcome(o)`: `kind = o.WhichOneof("result")`; if `"success"` → `{"result": "success", "evidence_url": o.success.evidence_url if o.success.HasField("evidence_url") else None}` (`evidence_url` is proto3 `optional` → use `HasField`); if `"failure"` → `{"result": "failure", "error_message": o.failure.error_message}`; else `{"result": None}`.

**5. `libs/common/tests/test_alerts_tools.py`** — extend the existing case test.
- In `test_get_alerts_object_impl_fetches_watch_data_for_case` (and any other CASE-path test), add `grpc_client.list_notification_deliveries.return_value = [<mock NotificationDelivery>]` alongside the existing mocks (lines 72-75), and assert `result.found_object.notification_deliveries == [...]`. (Without this mock, `_get_case`'s new gather arg on an `AsyncMock` still resolves to a coroutine returning a `Mock`, but assert explicitly to lock behavior.)

**6. New UT for verbose gating (apps/api)** — satisfies the ticket's explicit "unit test asserting field absent when `is_verbose=False`, present with expected shape when `is_verbose=True`" criterion.
- There is currently **no** test file exercising `case_to_dict`/alerts formatting in `apps/api/tests`. Create `apps/api/tests/ut/test_alerts_formatting.py`.
- Build a minimal `Case` proto plus a `NotificationDelivery` (one `attempted` with a `DeliveryAttempt` → connector `CONNECTOR_TYPE_SLACK`, outcome `success` + `evidence_url`; optionally a second `failure` case). Assert:
  - `case_to_dict(case, notification_deliveries=[d], is_verbose=False)` → `"notification_deliveries" not in output`.
  - `case_to_dict(case, notification_deliveries=[d], is_verbose=True)` → field present with the expected nested shape (timestamp, request_notification_id, result="attempted", router, attempts[].connector.connector_type=="CONNECTOR_TYPE_SLACK", outcome).
  - Cover the `no_router_matched` and `no_notification_created` oneof branches too.

### Edge cases / risks
- Empty deliveries: `list_notification_deliveries` returns `[]` when the case key is absent; `if is_verbose and notification_deliveries` then omits the field entirely (matches `kpi_breaches`/`impacted_entities` pattern).
- Unset oneofs: `WhichOneof` returns `None` for both `NotificationDelivery.result` and `DeliveryOutcome.result` — handled explicitly, never raises.
- `evidence_url` is proto3 `optional` — must use `HasField`, not truthiness alone (though truthiness is acceptable; prefer HasField for correctness).
- `deliveries_by_case` map indexing side-effect — use membership check.
- gather ordering: the new call is appended 4th; keep unpack order aligned.
- Do NOT touch `_format_notification_sent_event`/`_format_notification_failed_event` (out of scope) or the alert-event/alert-def paths.

### Verification (in a provisioned env / CI)
1. `just common::proto` to generate stubs (needs SSH; skip if already generated).
2. `just test-common` — new `list_notification_deliveries` mock + `CaseObjectData.notification_deliveries` assertions pass; existing tests still pass.
3. `just test-api` — new `test_alerts_formatting.py` verifies verbose-gating (absent when False, present+shape when True) and all three oneof branches.
4. `just common::lint` and `apps/api` `just lint` clean (types: new method returns `list[NotificationDelivery]`, `case_to_dict` param typed).
5. Behavioral check: for a verbose Case with a Slack success delivery, `get_alerts_object(..., is_verbose=True)` output includes `notification_deliveries` with connector_type `CONNECTOR_TYPE_SLACK` and `evidence_url`; the same call with `is_verbose=False` omits it.