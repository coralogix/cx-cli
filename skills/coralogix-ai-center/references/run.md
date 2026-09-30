# Phase 3 of 4 — Run

Get the instrumented application to actually send spans, either the skill runs it or the user does.

**Input:** the instrument handoff — files changed, flags added, app/subsystem names, how to run, per-environment leftovers, and who-runs from [assess.md](assess.md).

## 1. Shared contract

Both modes must produce the same evidence for [verify.md](verify.md):

| Element | Requirement |
|---|---|
| Probe marker | A unique conversation id / user id / prompt token the skill defines, included in the interaction, so Verify can prove a trace came from this run |
| Entry point | The one a customer would hit — the product's public API or UI — crossing every process boundary the change touches (queue/worker/background-task hops). Never a dev shortcut (`just`/make target, direct service invocation, an internal test harness) — unless it is the only local runner, in which case use it AND label it as such in the report's Entry-point line; unlabelled shortcuts are omissions |
| Both switches on | Trace export AND message-content capture enabled for the run — a run without content capture proves nothing about messages |
| Time window | Recorded for the query in Verify |

**Handoff to Verify:** probe marker, entry point used, time window, app/subsystem names, expected span types, who ran it.

## 2. Skill-run

### Credentials — one questionnaire

- **Send-Your-Data key (`cxtp_...`)** — the export credential (collector `private_key` / OTLP `Authorization` header). Ingests only, cannot query.
- **Query key (`cxup_...`) or a configured cx profile** — needed by Verify to read spans back. Queries only, cannot ingest.
- **LLM/gateway API keys** the app needs to run.
- **Tenant and region.**

Check the prefix of whatever the user supplies and request the right kind when it doesn't match.

### Live validation (before the real run)

| Step | What | Evidence to record |
|---|---|---|
| 1 | Send one throwaway span with the `cxtp_` key via OTLP/HTTP JSON — payload example in `cx docs fetch "developer-portal/apis/data-ingestion/opentelemetry-custom-traces.md"` | Quota/plan failures (HTTP 402, gRPC `ResourceExhausted`) surface now, not mid-run |
| 2 | Run one trivial `cx spans "limit 1"` with the query credential | Query access confirmed |
| 3 | Make one live call with the LLM/gateway key | Call succeeded — present is not valid |
| 4 | Key prefix check | Each supplied key matches its expected prefix (`cxtp_` export, `cxup_` query) |

If the user has no Send-Your-Data key, walk them through creating one: `cx docs fetch "user-guides/account-management/api-keys/api-keys.md"`, then guide them click-by-click (Data Flow → API Keys, which key type, what to name it). Keys go into a gitignored `.env` or the collector's environment — never chat.

### Dependency preflight

Before the end-to-end run, verify everything it will touch: docker daemon and required containers up, DB reachable and migrated, no placeholder values in env files (team id, region), and both enablement flags on. Collect anything missing in one questionnaire, re-check, then run.

### Run the interaction

A synthetic round-trip (a stubbed model call) is a useful bootstrap to confirm the pipeline works, but the completion bar is a real application run through the entry point from §1 — multi-turn and tool-using wherever the app supports it. Prefer one multi-turn probe conversation per verification round over many one-shot runs.

Restart only processes you started, by PID. Never `pkill -f <name>` or kill whatever holds a port — on a shared machine that is someone else's session.

## 3. User-run

Hand the user this checklist, as a file in the repo (ask before adding it) or printed inline, then stop and wait:

| Block | Contents |
|---|---|
| **Enable** | Flag names and files per environment, from the instrument handoff (both trace export and content-capture flags); a Send-Your-Data key (`cxtp_...`) wired as the export credential; offer the key-creation walkthrough (`cx docs fetch "user-guides/account-management/api-keys/api-keys.md"`) if they need one |
| **Drive** | The interaction to run — through the entry point a customer would hit, multi-turn and tool-using wherever the app supports it, crossing any queue/worker boundary the product has |
| **Mark** | The probe marker to include, agreed now, so Verify can attribute the spans |
| **Signal** | What to send back: the marker, the time window, the entry point used, which environments were run |

Do not proceed to [verify.md](verify.md) until the user signals done. When they do, continue with the probe marker, entry point, and time window they report.

## 4. Re-run on failure

If a MUST gate FAILs in Verify after either mode, the re-run follows the same who-runs decision made here — skill-run reruns itself, user-run gets the checklist again.
