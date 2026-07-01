# FORGE-104 — Enable control of reasoning level in Olly (backend)

## Goal

Expose an optional per-message `reasoning_effort` parameter that is threaded from the request → the agent runner, silently degrades to `"medium"` when the supplied effort is not supported by the chosen model, and is persisted on the interaction row for observability.

Backend only. No FE work (that is FORGE-105).

---

## Current behaviour (grounding)

Verified by reading the code. Nothing to reproduce because this is a feature‑add.

- `SendMessageRequest` (`libs/common/src/common/schemas/chats.py`) currently has no `reasoning_effort` field. FE/API callers cannot influence reasoning at all.
- The agent runner already has full support: `run_agent(..., reasoning_effort: ReasoningEffort | type[DefaultReasoning] = DefaultReasoning)` at `apps/api/src/api/agent/agent_runner.py:900`, and `run_agent_with_compaction_retry` at `:1006` — both accept and pipe the value.
- `DefaultReasoning` sentinel resolves to `"medium"` via `resolve_reasoning_effort` (line 119).
- `get_run_config` defaults to `"medium"` at line 638 when nothing is passed.
- `get_reasoning_settings` in `apps/api/src/api/agent/run_agent_utils.py:268` is the last mile: for non-reasoning models it returns `None` regardless of the requested effort, so passing `"medium"` for a non-reasoning model is a no-op.
- `handle_user_message` (`apps/api/src/api/agent/handlers/agent_interaction.py:746`) → `_handle_agent_interaction` → `run_agent_with_compaction_retry` never sets `reasoning_effort`, so the runner falls back to `DefaultReasoning → "medium"` on every call.
- `interactions_service.create_interaction` (`apps/api/src/api/services/interactions_service.py:116`) is where `SendMessageRequest` is built for Restate and where the `interactions` row is created. This is the natural place to (a) resolve the effective effort, (b) persist it, (c) pass it to Restate.
- DB: the `interactions` table (aliased view `team_interactions`) has `model_choice` but no `reasoning_effort`. `interactions_repository.create_interaction` at `apps/api/src/api/repositories/interactions_repository.py:41` writes `(team_id, chat_id, entity_id, model_choice, data_sources)`.
- Alembic head is `b7c4e1f92a30` (nothing else lists it as `down_revision`). The new migration must chain from it.
- SDK's `ReasoningEffort` = `Optional[Literal["none", "minimal", "low", "medium", "high", "xhigh"]]` (`.venv/.../openai/types/shared/reasoning_effort.py`). The ticket mentions `"max"` — that is **not** an SDK value; ceiling is `"xhigh"`.

## Run / verify commands (found in repo)

- Start service: `just dev` (runs migrations, then hypercorn on :8000).
- Create + apply migration: `just db-create-migration "add_reasoning_effort_to_interactions"` then `just db-migrate`.
- Generate the sibling platform SQL migration: `just db-generate-migrations` (required after adding/changing an Alembic file — see `.claude/skills/database-migrations/SKILL.md`).
- Lint + unit + integration checks: `just lint-fix`, `just test-api`, `just test-common`, `just test-integration`. Or the umbrella `/code-checks` skill.

The project cannot be booted end-to-end from inside this planning worktree (no `.env`, no AWS creds, no Docker running), so verification for this plan relies on unit + integration tests, which the repo's `/code-checks` skill treats as the authoritative check.

---

## Design decisions (from the ticket, cemented here)

| Question | Decision |
|---|---|
| Valid values | SDK's `ReasoningEffort` — `'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh'`. Per-model allowlist enforced. |
| Field type | `reasoning_effort: ReasoningEffort | None = None`. `None` = "use default". |
| Default | `"medium"` for all models. |
| Incompatible value | **Silently** degrade to `"medium"` (default). No 422. |
| What gets persisted | The **resolved / effective** effort, i.e. the value that actually ran. Storing an ignored value would defeat the observability goal. |
| Where to resolve | In `interactions_service.create_interaction`, once, before persisting and before forwarding to Restate. Do it once so DB and runner never disagree. |
| Duplicate on `HandleUserMessageRequest`? | **No.** `SendMessageRequest` already flows through the Restate payload verbatim; adding a second field would just create drift risk. Callers of the agent runner will read `user_message.reasoning_effort`. |

---

## Files to change

### 1. `libs/common/src/common/models.py` — allowlist helper + resolver

Add, next to `get_lowest_reasoning_effort`:

```python
def is_reasoning_effort_supported(self, effort: ReasoningEffort) -> bool:
    """Return True if the model natively supports the given effort.

    - Baseline for every reasoning-supporting model: {"low","medium","high","xhigh"}.
    - "minimal" — allowed for models where is_minimal_reasoning_supported() is
      True, or for other OpenAI reasoning models where get_reasoning_settings
      silently promotes minimal -> low (see run_agent_utils.py:280).
    - "none" — only for models where is_none_reasoning_supported() is True.
    - Non-reasoning models: no effort is "supported"; caller falls back to the
      default (which get_reasoning_settings ignores anyway for these models).
    """
    if not self.is_reasoning_supported():
        return False
    if effort in ("low", "medium", "high", "xhigh"):
        return True
    if effort == "minimal":
        # OpenAI has a silent minimal->low promotion; Gemini has similar mapping
        # in LiteLLM. Treating "minimal" as supported everywhere reasoning is
        # supported matches existing runtime behaviour.
        return self.is_openai_model() or self.is_gemini_model() \
            or self.is_minimal_reasoning_supported()
    if effort == "none":
        return self.is_none_reasoning_supported()
    return False
```

Add a module-level constant + resolver (module level, not on the enum, so callers can import cleanly):

```python
DEFAULT_REASONING_EFFORT: ReasoningEffort = "medium"

def resolve_supported_reasoning_effort(
    model: Model,
    requested: ReasoningEffort | None,
) -> ReasoningEffort:
    """Resolve the effective reasoning effort for (model, requested).

    - requested is None -> DEFAULT_REASONING_EFFORT ("medium").
    - requested not in model's allowlist -> DEFAULT_REASONING_EFFORT (silent).
    - requested valid -> requested.

    Non-reasoning models: returns "medium" but the value is ultimately a no-op
    (get_reasoning_settings returns None for these models).
    """
    if requested is None:
        return DEFAULT_REASONING_EFFORT
    if not model.is_reasoning_supported():
        return DEFAULT_REASONING_EFFORT
    if not model.is_reasoning_effort_supported(requested):
        return DEFAULT_REASONING_EFFORT
    return requested
```

Rationale for placing this in `common.models`:
- It's provider/model logic, next to the existing family predicates.
- `common` is imported by both the schema layer and the API layer, so both can share it.

### 2. `libs/common/src/common/schemas/chats.py` — add field to `SendMessageRequest`

```python
from openai.types.shared.reasoning_effort import ReasoningEffort  # add import

class SendMessageRequest(BaseModel):
    ...
    reasoning_effort: ReasoningEffort | None = Field(
        default=None,
        description=(
            "Optional reasoning effort for the chosen model. When omitted or "
            "when the value is not supported by the selected model, the server "
            "silently uses \"medium\". Valid: none, minimal, low, medium, high, xhigh."
        ),
    )
```

`EditMessageRequest(SendMessageRequest)` inherits automatically. `ChatResponseRead.user_message: SendMessageRequest | None` also picks it up for free (nice side effect: read models expose what was requested).

### 3. `libs/common/src/common/schemas/interactions_schema.py` — add to V2 request schemas

Add `reasoning_effort: ReasoningEffort | None = Field(default=None, description=...)` to:
- `CreateInteractionRequest`
- `UpdateInteractionRequest`

The `*Advanced` subclasses inherit. Also add to `InteractionMetadataRead` so poll/list endpoints surface the resolved value alongside `model_choice`:

```python
reasoning_effort: ReasoningEffort | None = Field(
    default=None,
    description="Effective reasoning effort applied to this interaction. "
                "None for non-reasoning models or legacy rows.",
)
```

### 4. `apps/api/src/api/repositories/interactions_repository.py`

- `create_interaction(..., reasoning_effort: ReasoningEffort | None)` new keyword arg; add the column to `INSERT`:
  ```sql
  INSERT INTO team_interactions
      (team_id, chat_id, entity_id, model_choice, data_sources, reasoning_effort)
  VALUES ($1, $2, $3, $4, $5, $6)
  RETURNING id, created_at
  ```
- Add `reasoning_effort` to the `SELECT` list in `_INTERACTION_STATUS_SQL`'s outer queries (`list_interactions`, `get_interaction_metadata`) so it's returned as part of `InteractionMetadataRead`.

### 5. `apps/api/src/api/services/interactions_service.py` — resolve, persist, forward

In `create_interaction`:

1. Resolve once:
   ```python
   from common.models import Model, resolve_supported_reasoning_effort
   effective_reasoning_effort = resolve_supported_reasoning_effort(
       model=message.model_choice.to_model(),
       requested=message.reasoning_effort,
   )
   ```
2. Persist the resolved value:
   ```python
   interaction_id, created_at = await interactions_repository.create_interaction(
       db=db,
       team_id=team_id,
       chat_id=chat_id,
       entity_id=entity_id,
       model_choice=message.model_choice,
       data_sources=message.data_sources,
       reasoning_effort=effective_reasoning_effort,
   )
   ```
3. Overwrite the field on the outgoing `SendMessageRequest` so the runner receives the already-resolved value (single source of truth):
   ```python
   agent_message = SendMessageRequest(
       ...
       reasoning_effort=effective_reasoning_effort,
   )
   ```
4. Include `reasoning_effort=effective_reasoning_effort` in the `InteractionReadAdvanced` return.

In `update_interaction`: pass `request.reasoning_effort` into the reconstructed `CreateAdvancedInteractionRequest`. (It flows through `create_interaction` and gets resolved there — no duplicate logic.)

### 6. `apps/api/src/api/agent/handlers/agent_interaction.py` — use it

`HandleUserMessageRequest` needs **no** new field (SendMessageRequest already carries it). In `_handle_agent_interaction`, at the two call sites for the runner:

```python
await run_agent_with_compaction_retry(
    ...,
    reasoning_effort=(
        user_message.reasoning_effort
        if user_message.reasoning_effort is not None
        else DefaultReasoning
    ),
)
```

- If FE didn't supply anything → falls back to `DefaultReasoning` → runner uses `"medium"` (identical to today).
- If FE supplied `"high"` and the service already resolved it → the same `"high"` reaches the runner.

Import `DefaultReasoning` from `api.agent.agent_runner` (already re-exported at the top of that module).

### 7. Alembic migration — new file

Create via `just db-create-migration "add_reasoning_effort_to_interactions"`. Contents:

```python
"""add_reasoning_effort_to_interactions

Revision ID: <auto>
Revises: b7c4e1f92a30
Create Date: <auto>
"""

from collections.abc import Sequence
import sqlalchemy as sa
from alembic import op

from api.utils.migrations import refresh_view

revision: str = "<auto>"
down_revision: str | None = "b7c4e1f92a30"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    with refresh_view("interactions"):
        op.add_column(
            "interactions",
            sa.Column("reasoning_effort", sa.String(), nullable=True),
        )


def downgrade() -> None:
    with refresh_view("interactions"):
        op.drop_column("interactions", "reasoning_effort")
```

Nullable — pre-existing rows have no reasoning_effort and we do not backfill; the read model already has `reasoning_effort: ReasoningEffort | None`.

Then run `just db-generate-migrations` to regenerate the sibling `V<N>__add_reasoning_effort_to_interactions.sql` under `apps/api/platform/defaults/datastores/migrations/` (mandatory per the migrations skill).

### 8. Tests

**Unit tests (`apps/api/tests/ut/`):**

- `test_agent_runner.py`: extend `TestResolveReasoningEffort` and/or add tests for the new `common.models.resolve_supported_reasoning_effort` covering:
  - `None → "medium"`.
  - Supported value passes through (e.g. `"high"` on GPT_5_2 → `"high"`).
  - Unsupported combo silently degrades — pick a real mismatch, e.g. `"none"` on a Claude model where `is_none_reasoning_supported() is False` → `"medium"`.
  - Non-reasoning model (e.g. `GPT_4O`) → `"medium"` regardless of input.
- `test_interactions_service.py`: mirror the existing `test_create_interaction__timezone_forwarded_to_user_message` pattern and add:
  - `test_create_interaction__reasoning_effort_default_medium_when_absent`.
  - `test_create_interaction__reasoning_effort_valid_forwarded_to_user_message_and_persisted`.
  - `test_create_interaction__reasoning_effort_incompatible_falls_back_to_medium` — assert both the DB persistence call and the outgoing `HandleUserMessageRequest.user_message.reasoning_effort` show `"medium"`.

**Integration tests (`tests/integration/test_interactions_route_v2.py`):**

Because `mock_restate_call` short-circuits the agent, integration coverage focuses on the schema/DB behaviour, which is exactly what the ticket calls out:

- (a) Omitted field: POST `/chats/{id}/interactions/` with no `reasoning_effort` → row in `interactions` has `reasoning_effort = 'medium'` (or NULL if we choose not to persist the default — see "Edge cases" below). Also assert the returned `InteractionDetailRead.reasoning_effort` matches.
- (b) Valid non-default: `reasoning_effort: "high"` with `model_choice: GPT_5_4` → persisted `"high"`, surfaced in `GET /interactions/{id}`.
- (c) Incompatible: `reasoning_effort: "none"` with `model_choice: CLAUDE_SONNET_4_5` (Claude doesn't support "none") → persisted `"medium"`. Request succeeds (no 422).

**Existing tests to sanity-check:**

- Any test constructing `SendMessageRequest(...)` positional or with all-kwargs — they should still pass because the new field defaults to `None`. Grep `SendMessageRequest(` and eyeball.
- `test_agent_interaction.py::_handle_user_message_request` builds a `HandleUserMessageRequest` — no change required (no new field on that type).

---

## Order of implementation

Dependency-first:

1. `libs/common/src/common/models.py` — helper + resolver + `DEFAULT_REASONING_EFFORT`.
2. `libs/common/src/common/schemas/chats.py` — add `reasoning_effort` to `SendMessageRequest`.
3. `libs/common/src/common/schemas/interactions_schema.py` — add to request + read schemas.
4. Alembic migration file, then `just db-migrate` locally, then `just db-generate-migrations`.
5. `apps/api/src/api/repositories/interactions_repository.py` — update `create_interaction` INSERT + SELECT columns.
6. `apps/api/src/api/services/interactions_service.py` — resolve once, persist, forward.
7. `apps/api/src/api/agent/handlers/agent_interaction.py` — read from `user_message`, thread into `run_agent_with_compaction_retry`.
8. Tests (unit + integration).
9. `just lint-fix` → `just test-api` → `just test-common` → `just test-integration`.

---

## Edge cases / risks

- **Default persistence vs NULL.** Storing `"medium"` for every new row is explicit and observable but noisy in the DB. Storing `NULL` when the user didn't set anything preserves "user asked for nothing" as data. I recommend **always persisting the resolved value** (`"medium"` for defaults) so a single query answers "what did we run this with" — matches the ticket's observability wording. NULL is reserved for pre-migration legacy rows.
- **`resolve_supported_reasoning_effort` runs *before* the DB insert.** If model resolution ever raises (e.g. a `ModelChoice` value not backed by a `Model` enum member), it must not fail the request — but this cannot happen today because `model_choice.to_model()` is enum construction from the same string. Still, keep the resolver pure and side-effect-free.
- **`is_reasoning_effort_supported("minimal")` on OpenAI models that don't natively support it.** The runtime silently promotes to `"low"` (see `get_reasoning_settings` line 280). I treat that as "supported" in the resolver so the persisted value matches what the user asked. If the reviewer prefers strict allow-listing (persist `"low"` after promotion), we can flip that branch — keep the choice explicit in the PR description.
- **`update_interaction` path.** When a user edits a message, they can now change reasoning too. The plan routes through `create_interaction` which re-resolves — no special handling needed.
- **Restate payload compatibility.** Adding a new optional field to `SendMessageRequest` is compatible: old serialized payloads deserialize with `reasoning_effort=None`, new ones round-trip fine.
- **Read models (`InteractionMetadataRead.reasoning_effort`).** Old rows return `NULL`, which serializes as `None` in JSON — safe for FE (FORGE-105 will decide how to render "unknown"). Not blocking.
- **`Model.is_reasoning_effort_supported` and its `xhigh` semantics for Anthropic.** LiteLLM currently mirrors `xhigh → high` in `ANTHROPIC_THINKING_BUDGET_BY_EFFORT` (see `run_agent_utils.py:48`). Accepting `xhigh` on Claude is fine — it just yields the same budget as `high`. Documented in-code; nothing to enforce.
- **Not in scope.** No FE changes (FORGE-105). No changes to `EvalsRunCompactionRequest`. No changes to sub-agent internal `run_agent(...)` call sites (they already default to `DefaultReasoning`).

---

## How to verify after implementation

Prefer tests (per `.claude/rules/verification.md` — "Integration tests ARE the verification").

1. `just lint-fix` — clean.
2. `just test-common` — new unit tests for `resolve_supported_reasoning_effort` pass.
3. `just test-api` — new unit tests in `test_interactions_service.py` and `test_agent_runner.py` pass; nothing regressed.
4. `just test-integration` — the three interaction-route integration cases (default / valid / incompatible) pass and confirm the row landed in `team_interactions` with the expected value.
5. Optional manual smoke via `curl` against `just dev` (after `just db-migrate`):
   ```bash
   curl -X POST http://localhost:8000/v2/chats/$CHAT_ID/interactions/advanced \
        -H "Content-Type: application/json" \
        -d '{"content":[{"type":"input_text","text":"hi"}],
             "model_choice":"gpt-5.4",
             "reasoning_effort":"high"}'
   ```
   Then `SELECT id, model_choice, reasoning_effort FROM interactions ORDER BY created_at DESC LIMIT 1;` — expect `high`. Repeat with `reasoning_effort: "none"` + `claude-sonnet-4-5` and expect `medium`.

---

## Success criteria mapping

| Ticket criterion | Where addressed |
|---|---|
| 1. Optional field on `SendMessageRequest` | `libs/common/src/common/schemas/chats.py` |
| 2. Threads request → HandleUserMessageRequest → handle_user_message → run_agent_with_compaction_retry, replacing hardcoded default | via `user_message.reasoning_effort` in `_handle_agent_interaction`; hard-coded `"medium"` at agent_runner.py:642 stays as the *sentinel default* but is no longer the *effective* default when callers supply a value |
| 3. Per-model allowlist + silent fallback to `"medium"` | `Model.is_reasoning_effort_supported` + `resolve_supported_reasoning_effort` in `common.models`, called in `interactions_service.create_interaction` |
| 4. Alembic migration adds nullable `reasoning_effort` to interactions; populated on every new interaction | New migration + repo `INSERT` update |
| 5. Integration tests (default / valid / incompatible-fallback) | `tests/integration/test_interactions_route_v2.py` |
